//! ADSR envelope generator.
//!
//! The caller changes the gate only between audio blocks with [`Adsr::gate_on`]
//! and [`Adsr::gate_off`]. [`Adsr::process`] then writes one envelope value per
//! sample. This is deliberately an output-only control kernel: a voice node
//! decides which signal(s) the envelope modulates.

use crate::dsp::{LANES, LaneFrame};

/// Current segment of an [`Adsr`] envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear attack-decay-sustain-release envelope.
///
/// A gate-on retriggers from exact zero. Attack rises to exact one, decay
/// falls to the configured sustain level, and gate-off releases from the
/// current level (including during attack or decay) to exact zero. A zero or
/// invalid duration is instantaneous; an invalid sustain level is zero.
///
/// State: 32 bytes. Per-sample cost: 1 branch + 1 add while moving.
/// Denormal-safe: relies on engine FTZ; a release can pass through the
/// denormal range. Never invents NaN from finite settings.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Adsr {
    value: f32,
    sustain: f32,
    step: f32,
    attack_samples: u32,
    decay_samples: u32,
    release_samples: u32,
    remaining: u32,
    stage: AdsrStage,
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}

impl Adsr {
    /// Construct an idle envelope with instantaneous stages and sustain 0.
    pub fn new() -> Self {
        Self {
            value: 0.0,
            sustain: 0.0,
            step: 0.0,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            remaining: 0,
            stage: AdsrStage::Idle,
        }
    }

    /// Green zone: set times in milliseconds and a sustain level in [0, 1].
    ///
    /// A non-finite or non-positive time is instantaneous. Finite durations
    /// smaller than one sample round up to one sample. This does not change
    /// the current envelope; call [`reset`](Self::reset) to silence it.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    ) {
        self.attack_samples = duration_samples(sample_rate, attack_ms);
        self.decay_samples = duration_samples(sample_rate, decay_ms);
        self.release_samples = duration_samples(sample_rate, release_ms);
        self.sustain = if sustain.is_finite() {
            sustain.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Green zone: start a new note from silence, including when retriggered
    /// while another gate is active.
    pub fn gate_on(&mut self) {
        self.value = 0.0;
        self.start_attack();
    }

    /// Green zone: begin the release from the current level. Calling this
    /// while idle keeps the envelope idle.
    pub fn gate_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.start_release();
        }
    }

    /// Return to exact silence and idle, keeping configured parameters.
    pub fn reset(&mut self) {
        self.value = 0.0;
        self.step = 0.0;
        self.remaining = 0;
        self.stage = AdsrStage::Idle;
    }

    /// Current value, including the value held between process calls.
    pub fn current(&self) -> f32 {
        self.value
    }

    /// Current envelope segment.
    pub fn stage(&self) -> AdsrStage {
        self.stage
    }

    /// True from gate-on until the release reaches zero.
    pub fn active(&self) -> bool {
        self.stage != AdsrStage::Idle
    }

    /// Red zone: write the next `out.len()` envelope samples.
    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            match self.stage {
                AdsrStage::Idle => {}
                AdsrStage::Sustain => {
                    self.value = self.sustain;
                }
                AdsrStage::Attack | AdsrStage::Decay | AdsrStage::Release => {
                    if self.remaining > 1 {
                        self.value += self.step;
                        self.remaining -= 1;
                    } else {
                        match self.stage {
                            AdsrStage::Attack => {
                                self.value = 1.0;
                                self.start_decay();
                            }
                            AdsrStage::Decay => {
                                self.value = self.sustain;
                                self.step = 0.0;
                                self.remaining = 0;
                                self.stage = AdsrStage::Sustain;
                            }
                            AdsrStage::Release => {
                                self.value = 0.0;
                                self.step = 0.0;
                                self.remaining = 0;
                                self.stage = AdsrStage::Idle;
                            }
                            AdsrStage::Idle | AdsrStage::Sustain => {}
                        }
                    }
                }
            }
            *sample = self.value;
        }
    }

    fn start_attack(&mut self) {
        if self.attack_samples == 0 {
            self.value = 1.0;
            self.start_decay();
        } else {
            self.step = 1.0 / self.attack_samples as f32;
            self.remaining = self.attack_samples;
            self.stage = AdsrStage::Attack;
        }
    }

    fn start_decay(&mut self) {
        if self.decay_samples == 0 {
            self.value = self.sustain;
            self.step = 0.0;
            self.remaining = 0;
            self.stage = AdsrStage::Sustain;
        } else {
            self.step = (self.sustain - self.value) / self.decay_samples as f32;
            self.remaining = self.decay_samples;
            self.stage = AdsrStage::Decay;
        }
    }

    fn start_release(&mut self) {
        if self.release_samples == 0 {
            self.value = 0.0;
            self.step = 0.0;
            self.remaining = 0;
            self.stage = AdsrStage::Idle;
        } else {
            self.step = -self.value / self.release_samples as f32;
            self.remaining = self.release_samples;
            self.stage = AdsrStage::Release;
        }
    }
}

/// Convert a green-zone time to samples without allowing invalid parameters
/// to poison the red-zone state.
fn duration_samples(sample_rate: f32, time_ms: f32) -> u32 {
    // Use f64 here so nominally integral durations such as 1 kHz × 3 ms do
    // not become 3.0000002 in f32 and accidentally gain an extra sample.
    let samples = f64::from(sample_rate) * f64::from(time_ms) / 1_000.0;
    if samples.is_finite() && samples > 0.0 {
        samples.ceil().min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

// --------------------------------------------------- exponential decay ---

/// One-shot EXPONENTIAL decay — a capacitor discharging through a
/// resistor, which is what an analogue drum machine's VCA envelope
/// physically is.
///
/// [`Adsr`] is linear, and for a sustained instrument that is the right
/// choice: a linear release is predictable and its end is exactly where
/// the number says. A DRUM is the other case. The 808's hat and clap
/// envelopes are RC discharges, and the difference is not subtle — a
/// linear ramp holds its level through the middle of the sound and then
/// stops, which is heard as a gated tail rather than a decaying one. The
/// characteristic "tsss" of a hat is entirely the shape of this curve.
///
/// `decay_ms` is the time to fall 60 dB (to a thousandth), the same
/// convention a reverb's RT60 uses — so a number here means the audible
/// length of the sound rather than the abstract time constant, which is a
/// fifth of it.
///
/// Below [`FLOOR`](Self::FLOOR) the envelope snaps to exact zero and goes
/// idle. That is what bounds it: an exponential never mathematically
/// reaches zero, so without a floor `active()` would never go false and a
/// voice would ring forever in the denormal range.
///
/// State: 12 bytes. Per-sample cost: 1 mul + 1 compare.
/// Denormal-safe: yes, and not by relying on FTZ — the floor is far above
/// the denormal range, so the state never enters it.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct ExpDecay {
    value: f32,
    coeff: f32,
}

impl Default for ExpDecay {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpDecay {
    /// Where the tail is cut to silence.
    ///
    /// -140 dB: below anything audible at any listening level, above the
    /// denormal range by many orders of magnitude, and reached in 2.3
    /// times the stated 60 dB decay — so the number on the knob is still
    /// the sound's length rather than the cut's.
    pub const FLOOR: f32 = 1.0e-7;

    /// How many time constants make up the stated decay: `ln(1000)`,
    /// because the figure is a time to fall 60 dB.
    const DECADES: f32 = 6.907_755_4;

    /// Construct an idle envelope with an instantaneous decay.
    pub fn new() -> Self {
        Self {
            value: 0.0,
            coeff: 0.0,
        }
    }

    /// Green zone: set the 60 dB decay time in milliseconds.
    ///
    /// A zero, negative or non-finite time is instantaneous — one sample
    /// at the triggered level and then silence, which is the degenerate
    /// case a click wants rather than an error.
    ///
    /// Does not disturb a decay in flight; the new rate applies from the
    /// next sample. Call [`reset`](Self::reset) to silence it.
    pub fn prepare(&mut self, sample_rate: f32, decay_ms: f32) {
        let samples = f64::from(sample_rate) * f64::from(decay_ms) / 1_000.0;
        self.coeff = if samples.is_finite() && samples > 0.0 {
            // exp(-ln(1000)/N): the per-sample ratio that lands 60 dB
            // down after exactly N samples.
            (-f64::from(Self::DECADES) / samples).exp() as f32
        } else {
            0.0
        };
        // A rate that is not a proper contraction would never terminate.
        if !self.coeff.is_finite() || self.coeff < 0.0 {
            self.coeff = 0.0;
        } else if self.coeff > Self::MAX_COEFF {
            self.coeff = Self::MAX_COEFF;
        }
    }

    /// The slowest decay the kernel will run.
    ///
    /// A coefficient of exactly 1.0 is a hold, not a decay: it would never
    /// reach the floor and `active()` would never go false. Clamping just
    /// below keeps the kernel's termination guarantee true for any
    /// `decay_ms` a caller can name, including one longer than the heat
    /// death of the session.
    const MAX_COEFF: f32 = 0.999_999_94;

    /// Green zone: strike, starting the decay from `level`.
    ///
    /// A retrigger REPLACES the tail rather than adding to it — a drum
    /// struck again before it has finished is one drum, not two summed.
    /// A non-finite or negative level reads as silence.
    pub fn trigger(&mut self, level: f32) {
        self.value = if level.is_finite() && level > Self::FLOOR {
            level
        } else {
            0.0
        };
    }

    /// Return to exact silence, keeping the configured rate.
    pub fn reset(&mut self) {
        self.value = 0.0;
    }

    /// Current value, including the value held between process calls.
    pub fn current(&self) -> f32 {
        self.value
    }

    /// True from the strike until the tail reaches the floor.
    pub fn active(&self) -> bool {
        self.value > 0.0
    }

    /// Red zone: write the next `out.len()` envelope samples.
    ///
    /// The first sample after a [`trigger`](Self::trigger) is the
    /// triggered level itself, so the strike is not delayed by a sample.
    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = self.value;
            self.value *= self.coeff;
            if self.value < Self::FLOOR {
                self.value = 0.0;
            }
        }
    }
}

// ------------------------------------------------------------- lanes ---

/// Move a lane into its decay, or straight to sustain if decay is
/// instantaneous. Free functions rather than methods because the process
/// loop holds the state arrays through disjoint iterators, and a `&mut
/// self` method would want the whole struct back.
fn begin_decay(
    value: &mut f32,
    step: &mut f32,
    remaining: &mut u32,
    stage: &mut AdsrStage,
    sustain: f32,
    decay_samples: u32,
) {
    if decay_samples == 0 {
        *value = sustain;
        *step = 0.0;
        *remaining = 0;
        *stage = AdsrStage::Sustain;
    } else {
        *step = (sustain - *value) / decay_samples as f32;
        *remaining = decay_samples;
        *stage = AdsrStage::Decay;
    }
}

fn begin_release(
    value: &mut f32,
    step: &mut f32,
    remaining: &mut u32,
    stage: &mut AdsrStage,
    release_samples: u32,
) {
    if release_samples == 0 {
        *value = 0.0;
        *step = 0.0;
        *remaining = 0;
        *stage = AdsrStage::Idle;
    } else {
        *step = -*value / release_samples as f32;
        *remaining = release_samples;
        *stage = AdsrStage::Release;
    }
}

/// [`Adsr`] for a whole voice group: [`LANES`] envelopes, lane-major.
///
/// TIMES ARE SHARED, STAGES ARE NOT. One patch has one envelope shape, so
/// attack/decay/sustain/release live once; what is per lane is where each
/// voice has got to, because voices start at different samples. That split
/// is why this is one kernel and not [`LANES`] copies of [`Adsr`].
///
/// The per-sample loop branches per lane, and deliberately: an envelope is
/// a state machine whose lanes diverge, so there is no branchless form
/// that is not just all four stages evaluated and masked. What the layout
/// buys is the CONSUMER — the value arrays come out register-shaped, and
/// everything downstream multiplies by them a whole group at a time.
///
/// State: 128 bytes + 16.
/// Per-sample-per-lane cost: the scalar kernel's — 1 branch + 1 add.
/// Denormal-safe: relies on engine FTZ; a release can pass through the
/// denormal range. Never invents NaN from finite settings.
/// In-place safe: n/a — output-only.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct LaneAdsr {
    value: [f32; LANES],
    step: [f32; LANES],
    remaining: [u32; LANES],
    stage: [AdsrStage; LANES],
    sustain: f32,
    attack_samples: u32,
    decay_samples: u32,
    release_samples: u32,
}

impl Default for LaneAdsr {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneAdsr {
    /// Construct a group of idle envelopes, instantaneous, sustain 0.
    pub fn new() -> Self {
        Self {
            value: [0.0; LANES],
            step: [0.0; LANES],
            remaining: [0; LANES],
            stage: [AdsrStage::Idle; LANES],
            sustain: 0.0,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
        }
    }

    /// Green zone: set the shape every lane shares. Same rules as the
    /// scalar kernel — a non-finite or non-positive time is
    /// instantaneous, a finite duration under one sample rounds up, and
    /// an invalid sustain is zero. Does not disturb envelopes already
    /// running.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    ) {
        self.attack_samples = duration_samples(sample_rate, attack_ms);
        self.decay_samples = duration_samples(sample_rate, decay_ms);
        self.release_samples = duration_samples(sample_rate, release_ms);
        self.sustain = if sustain.is_finite() {
            sustain.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Green zone: start one lane from exact zero, retrigger included.
    pub fn gate_on(&mut self, lane: usize) {
        let (Some(value), Some(step), Some(remaining), Some(stage)) = (
            self.value.get_mut(lane),
            self.step.get_mut(lane),
            self.remaining.get_mut(lane),
            self.stage.get_mut(lane),
        ) else {
            return;
        };
        *value = 0.0;
        if self.attack_samples == 0 {
            *value = 1.0;
            begin_decay(
                value,
                step,
                remaining,
                stage,
                self.sustain,
                self.decay_samples,
            );
        } else {
            *step = 1.0 / self.attack_samples as f32;
            *remaining = self.attack_samples;
            *stage = AdsrStage::Attack;
        }
    }

    /// Green zone: release one lane from wherever it is. A lane already
    /// idle stays idle.
    pub fn gate_off(&mut self, lane: usize) {
        let (Some(value), Some(step), Some(remaining), Some(stage)) = (
            self.value.get_mut(lane),
            self.step.get_mut(lane),
            self.remaining.get_mut(lane),
            self.stage.get_mut(lane),
        ) else {
            return;
        };
        if *stage != AdsrStage::Idle {
            begin_release(value, step, remaining, stage, self.release_samples);
        }
    }

    /// Green zone: silence every lane, keeping the configured shape.
    pub fn reset(&mut self) {
        self.value = [0.0; LANES];
        self.step = [0.0; LANES];
        self.remaining = [0; LANES];
        self.stage = [AdsrStage::Idle; LANES];
    }

    /// Green zone: silence ONE lane — what stealing a voice wants —
    /// leaving the rest of the group running.
    pub fn reset_lane(&mut self, lane: usize) {
        if let (Some(value), Some(step), Some(remaining), Some(stage)) = (
            self.value.get_mut(lane),
            self.step.get_mut(lane),
            self.remaining.get_mut(lane),
            self.stage.get_mut(lane),
        ) {
            *value = 0.0;
            *step = 0.0;
            *remaining = 0;
            *stage = AdsrStage::Idle;
        }
    }

    /// One lane's held value, including between process calls.
    pub fn current(&self, lane: usize) -> f32 {
        self.value.get(lane).copied().unwrap_or(0.0)
    }

    /// One lane's segment.
    pub fn stage(&self, lane: usize) -> AdsrStage {
        self.stage.get(lane).copied().unwrap_or(AdsrStage::Idle)
    }

    /// True from a lane's gate-on until its release reaches zero.
    pub fn active(&self, lane: usize) -> bool {
        self.stage(lane) != AdsrStage::Idle
    }

    /// True while ANY lane is sounding — what a node asks before deciding
    /// whether a whole voice group can be skipped.
    pub fn any_active(&self) -> bool {
        self.stage.iter().any(|s| *s != AdsrStage::Idle)
    }

    /// Red zone: write the next `out.len()` frames of envelope values.
    pub fn process(&mut self, out: &mut [LaneFrame]) {
        // The shared shape is copied out first so the loop below borrows
        // only the per-lane arrays, which are disjoint fields.
        let sustain = self.sustain;
        let decay_samples = self.decay_samples;
        for frame in out.iter_mut() {
            let lanes = frame
                .iter_mut()
                .zip(self.value.iter_mut())
                .zip(self.step.iter_mut())
                .zip(self.remaining.iter_mut())
                .zip(self.stage.iter_mut());
            for ((((sample, value), step), remaining), stage) in lanes {
                match *stage {
                    AdsrStage::Idle => {}
                    AdsrStage::Sustain => {
                        *value = sustain;
                    }
                    AdsrStage::Attack | AdsrStage::Decay | AdsrStage::Release => {
                        if *remaining > 1 {
                            *value += *step;
                            *remaining -= 1;
                        } else {
                            match *stage {
                                AdsrStage::Attack => {
                                    *value = 1.0;
                                    begin_decay(
                                        value,
                                        step,
                                        remaining,
                                        stage,
                                        sustain,
                                        decay_samples,
                                    );
                                }
                                AdsrStage::Decay => {
                                    *value = sustain;
                                    *step = 0.0;
                                    *remaining = 0;
                                    *stage = AdsrStage::Sustain;
                                }
                                AdsrStage::Release => {
                                    *value = 0.0;
                                    *step = 0.0;
                                    *remaining = 0;
                                    *stage = AdsrStage::Idle;
                                }
                                AdsrStage::Idle | AdsrStage::Sustain => {}
                            }
                        }
                    }
                }
                *sample = *value;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    /// The lane tests' sample rate. 1 ms is 48 samples here, so the
    /// stage lengths below are round numbers.
    const FS: f32 = 48_000.0;

    fn bits(value: f32) -> u32 {
        value.to_bits()
    }

    fn configured() -> Adsr {
        let mut env = Adsr::new();
        // At 1 kHz: attack = 4 samples, decay = 3, release = 5.
        env.prepare(1_000.0, 4.0, 3.0, 0.4, 5.0);
        env
    }

    // -------------------------------------------------------- reference ---

    #[test]
    fn stages_reach_exact_endpoints() {
        let mut env = configured();
        env.gate_on();
        let mut out = [0.0; 10];
        env.process(&mut out);

        assert_eq!(out[..4], [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(bits(out[6]), bits(0.4), "decay lands exactly on sustain");
        assert!(out[7..].iter().all(|sample| bits(*sample) == bits(0.4)));
        assert_eq!(env.stage(), AdsrStage::Sustain);

        env.gate_off();
        let mut release = [0.0; 6];
        env.process(&mut release);
        assert_eq!(bits(release[4]), bits(0.0), "release lands exactly on zero");
        assert!(release[5] == 0.0);
        assert_eq!(env.stage(), AdsrStage::Idle);
    }

    #[test]
    fn gate_off_releases_from_the_current_attack_level() {
        let mut env = configured();
        env.gate_on();
        let mut attack = [0.0; 2];
        env.process(&mut attack);
        assert_eq!(attack[1], 0.5);
        env.gate_off();
        let mut release = [0.0; 5];
        env.process(&mut release);
        assert!(release[0] < 0.5 && release[0] > 0.0);
        assert_eq!(release[4], 0.0);
    }

    // ------------------------------------------- split-block bit-exactness

    #[test]
    fn split_block_is_bit_exact() {
        let mut whole = configured();
        let mut split = configured();
        whole.gate_on();
        split.gate_on();
        let mut full = [0.0; 256];
        let mut segmented = [0.0; 256];
        whole.process(&mut full);
        split.process(&mut segmented[..100]);
        split.process(&mut segmented[100..]);
        assert!(
            full.iter()
                .zip(&segmented)
                .all(|(a, b)| bits(*a) == bits(*b))
        );

        whole.gate_off();
        split.gate_off();
        whole.process(&mut full);
        split.process(&mut segmented[..100]);
        split.process(&mut segmented[100..]);
        assert!(
            full.iter()
                .zip(&segmented)
                .all(|(a, b)| bits(*a) == bits(*b))
        );
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn process_does_not_allocate() {
        let mut env = configured();
        env.gate_on();
        let mut out = [0.0; 64];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                env.process(&mut out);
            }
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn accepts_zero_one_and_non_power_of_two_lengths() {
        let mut env = configured();
        env.gate_on();
        env.process(&mut []);
        assert_eq!(env.current(), 0.0, "empty blocks do not advance");
        let mut one = [0.0; 1];
        env.process(&mut one);
        assert_eq!(one[0], 0.25);
        let mut seven = [0.0; 7];
        env.process(&mut seven);
        assert!(seven.iter().all(|sample| sample.is_finite()));

        let mut instant = Adsr::new();
        instant.prepare(48_000.0, -1.0, f32::NAN, 2.0, 0.0);
        instant.gate_on();
        let mut out = [0.0; 7];
        instant.process(&mut out);
        assert!(out.iter().all(|sample| *sample == 1.0));
        instant.gate_off();
        instant.process(&mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn silence_and_denormal_release_stay_finite() {
        let mut idle = configured();
        let mut silence = [1.0; 64];
        idle.process(&mut silence);
        assert!(silence.iter().all(|sample| *sample == 0.0));

        let mut env = Adsr::new();
        env.prepare(48_000.0, 0.0, 0.0, 1.0, 100.0);
        env.gate_on();
        let mut start = [0.0; 1];
        env.process(&mut start);
        env.gate_off();
        let mut tail = [0.0; 8_192];
        env.process(&mut tail);
        assert!(
            tail.iter()
                .all(|sample| sample.is_finite() && *sample >= 0.0)
        );
    }
    // --------------------------------------------------------- lanes ---

    /// The scalar kernel's output for one gate-on, held `hold` samples,
    /// then released for the rest — the trace every lane must reproduce.
    fn scalar_trace(a: f32, d: f32, sus: f32, r: f32, hold: usize, n: usize) -> Vec<f32> {
        let mut e = Adsr::new();
        e.prepare(FS, a, d, sus, r);
        let mut out = vec![0.0f32; n];
        e.gate_on();
        let (head, tail) = out.split_at_mut(hold);
        e.process(head);
        e.gate_off();
        e.process(tail);
        out
    }

    fn lane_column(frames: &[LaneFrame], lane: usize) -> Vec<f32> {
        frames.iter().map(|f| f[lane]).collect()
    }

    /// REFERENCE. A lane driven exactly as the scalar kernel is produces
    /// a BIT-IDENTICAL envelope.
    ///
    /// The scalar `Adsr` is already tested against the analytic shape —
    /// exact 1.0 at the attack peak, exact sustain, exact 0.0 at the end
    /// of release — so matching it bit for bit inherits every one of
    /// those claims rather than restating them.
    #[test]
    fn a_lane_is_bit_identical_to_the_scalar_envelope() {
        const N: usize = 900;
        const HOLD: usize = 400;
        for (a, d, sus, r) in [
            (5.0f32, 3.0f32, 0.5f32, 7.0f32),
            (0.0, 0.0, 1.0, 0.0),
            (1.0, 20.0, 0.0, 2.0),
        ] {
            let want = scalar_trace(a, d, sus, r, HOLD, N);
            for lane in 0..LANES {
                let mut e = LaneAdsr::new();
                e.prepare(FS, a, d, sus, r);
                let mut out = vec![[0.0f32; LANES]; N];
                e.gate_on(lane);
                let (head, tail) = out.split_at_mut(HOLD);
                e.process(head);
                e.gate_off(lane);
                e.process(tail);
                assert_eq!(lane_column(&out, lane), want, "lane {lane} a={a} d={d}");
            }
        }
    }

    /// LANE INDEPENDENCE. The mandatory sixth test.
    ///
    /// One lane gated, the rest silent: every other lane must be EXACTLY
    /// 0.0, and the gated lane must match the scalar kernel wherever it
    /// is placed. This is what catches a stage or a step left scalar
    /// during the port — the sort of bug where all eight lanes move
    /// together and every other test is perfectly happy.
    #[test]
    fn gating_one_lane_leaves_the_others_at_exact_zero() {
        const N: usize = 600;
        const HOLD: usize = 250;
        let want = scalar_trace(4.0, 6.0, 0.4, 9.0, HOLD, N);
        for lane in 0..LANES {
            let mut e = LaneAdsr::new();
            e.prepare(FS, 4.0, 6.0, 0.4, 9.0);
            let mut out = vec![[0.0f32; LANES]; N];
            e.gate_on(lane);
            let (head, tail) = out.split_at_mut(HOLD);
            e.process(head);
            e.gate_off(lane);
            e.process(tail);
            for other in 0..LANES {
                if other == lane {
                    assert_eq!(lane_column(&out, other), want, "lane {lane} moved wrong");
                } else {
                    assert!(
                        lane_column(&out, other).iter().all(|s| *s == 0.0),
                        "lane {other} moved while only {lane} was gated"
                    );
                }
                assert_eq!(e.active(other), other == lane && e.active(lane));
            }
        }
    }

    /// Lanes at DIFFERENT stages advance independently — the real voice
    /// case, where notes start at different samples.
    #[test]
    fn lanes_hold_independent_stages() {
        let mut e = LaneAdsr::new();
        e.prepare(FS, 10.0, 10.0, 0.5, 10.0);
        let mut out = vec![[0.0f32; LANES]; 64];
        e.gate_on(0);
        e.process(&mut out);
        e.gate_on(1);
        e.process(&mut out);
        // Lane 0 started 64 samples earlier, so it is further along.
        assert!(
            e.current(0) > e.current(1),
            "{} !> {}",
            e.current(0),
            e.current(1)
        );
        assert!(e.active(0) && e.active(1));
        assert!(!e.active(2));
        assert!(e.any_active());
        // Releasing one leaves the other where it was.
        let before = e.current(0);
        e.gate_off(1);
        assert_eq!(e.current(0), before);
        assert_eq!(e.stage(1), AdsrStage::Release);
        assert_eq!(e.stage(0), AdsrStage::Attack);
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact.
    #[test]
    fn lane_adsr_splits_bit_exactly() {
        const N: usize = 256;
        let run = |cut: Option<usize>| {
            let mut e = LaneAdsr::new();
            e.prepare(FS, 2.0, 4.0, 0.3, 6.0);
            let mut out = vec![[0.0f32; LANES]; N];
            for lane in 0..LANES {
                e.gate_on(lane);
            }
            match cut {
                None => e.process(&mut out),
                Some(c) => {
                    let (head, tail) = out.split_at_mut(c);
                    e.process(head);
                    e.process(tail);
                }
            }
            out
        };
        assert_eq!(run(Some(100)), run(None));
    }

    /// NO-ALLOC on the process path.
    #[test]
    fn lane_adsr_does_not_allocate() {
        let mut e = LaneAdsr::new();
        e.prepare(FS, 1.0, 1.0, 0.5, 1.0);
        e.gate_on(0);
        let mut buf = vec![[0.0f32; LANES]; 512];
        assert_no_alloc::assert_no_alloc(|| {
            e.process(&mut buf);
        });
    }

    /// EDGE LENGTHS: 0, 1, and a non-power-of-two — and a zero-length
    /// block must not advance the envelope.
    #[test]
    fn lane_adsr_takes_any_block_length() {
        for n in [0usize, 1, 3, 97] {
            let mut e = LaneAdsr::new();
            e.prepare(FS, 5.0, 5.0, 0.5, 5.0);
            e.gate_on(0);
            let mut out = vec![[0.0f32; LANES]; n];
            e.process(&mut out);
            assert_eq!(out.len(), n);
        }
        let mut e = LaneAdsr::new();
        e.prepare(FS, 5.0, 5.0, 0.5, 5.0);
        e.gate_on(0);
        let held = e.current(0);
        e.process(&mut []);
        assert_eq!(e.current(0), held);
    }

    /// DENORMAL TAIL: a release lands on EXACT zero and idles there, and
    /// nothing in the group ever goes non-finite.
    #[test]
    fn lane_adsr_releases_to_exact_zero() {
        let mut e = LaneAdsr::new();
        e.prepare(FS, 0.1, 0.1, 0.9, 1.0);
        let mut out = vec![[0.0f32; LANES]; 4_096];
        for lane in 0..LANES {
            e.gate_on(lane);
        }
        e.process(&mut out);
        for lane in 0..LANES {
            e.gate_off(lane);
        }
        e.process(&mut out);
        for frame in &out {
            for s in frame {
                assert!(s.is_finite(), "{s}");
            }
        }
        for lane in 0..LANES {
            assert_eq!(e.current(lane), 0.0);
            assert!(!e.active(lane));
        }
        assert!(!e.any_active());
    }

    /// An out-of-range lane is ignored rather than panicking: the red
    /// zone takes no index that can trap, and a node with a stale voice
    /// number must not kill the callback.
    #[test]
    fn an_out_of_range_lane_is_ignored() {
        let mut e = LaneAdsr::new();
        e.prepare(FS, 1.0, 1.0, 0.5, 1.0);
        e.gate_on(LANES);
        e.gate_off(LANES + 3);
        e.reset_lane(usize::MAX);
        assert!(!e.any_active());
        assert_eq!(e.current(LANES), 0.0);
        assert_eq!(e.stage(LANES), AdsrStage::Idle);
        assert!(!e.active(LANES));
    }

    // --------------------------------------------- ExpDecay: the five ---

    /// REFERENCE. The curve is `level * exp(-ln(1000) * t / decay)`, and
    /// the stated decay time is where it passes 60 dB down — the whole
    /// point of the convention, and the thing a linear envelope cannot do.
    #[test]
    fn exp_decay_follows_the_rc_curve_and_is_60_db_down_at_its_time() {
        let mut env = ExpDecay::new();
        env.prepare(FS, 100.0); // 4800 samples
        env.trigger(1.0);

        let mut out = vec![0.0f32; 9_600];
        env.process(&mut out);

        // The strike is not delayed: sample zero IS the triggered level.
        assert_eq!(out[0], 1.0);

        // Against the closed form, everywhere.
        for (i, got) in out.iter().enumerate().take(4_800) {
            let want = (-ExpDecay::DECADES * i as f32 / 4_800.0).exp();
            assert!(
                (got - want).abs() < 1e-5,
                "sample {i}: {got} against {want}"
            );
        }

        // 60 dB down at the stated time, and half the level is reached at
        // a tenth of it — an exponential's half-life, not a linear ramp's
        // midpoint, which would be at 50 %.
        assert!(
            (out[4_800] - 0.001).abs() < 1e-5,
            "at the decay time: {}",
            out[4_800]
        );
        let half = out.iter().position(|v| *v <= 0.5).unwrap_or(0);
        assert!(
            (half as f32 - 4_800.0 * 0.1003).abs() < 8.0,
            "the half-life sits at a tenth of the decay, not the middle: {half}"
        );

        // A LINEAR envelope of the same length would be at 0.5 halfway
        // through. This must not be — that is the whole reason the kernel
        // exists.
        assert!(
            out[2_400] < 0.05,
            "an exponential is nearly gone by halfway: {}",
            out[2_400]
        );
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit for bit. The property the segmented
    /// transport depends on: 256 must equal 100 + 1 + 155.
    #[test]
    fn exp_decay_is_the_same_however_the_block_is_split() {
        let build = || {
            let mut env = ExpDecay::new();
            env.prepare(FS, 37.5);
            env.trigger(0.8);
            env
        };

        let mut whole = build();
        let mut a = vec![0.0f32; 256];
        whole.process(&mut a);

        let mut split = build();
        let mut b = vec![0.0f32; 256];
        split.process(&mut b[..100]);
        split.process(&mut b[100..101]);
        split.process(&mut b[101..]);

        assert!(
            a.iter().zip(&b).all(|(x, y)| bits(*x) == bits(*y)),
            "the split run diverged"
        );
        assert_eq!(bits(whole.current()), bits(split.current()));
    }

    /// NO ALLOCATION on the render path, trigger included.
    #[test]
    fn exp_decay_does_not_allocate() {
        let mut env = ExpDecay::new();
        env.prepare(FS, 80.0);
        let mut out = vec![0.0f32; 128];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..64 {
                if i % 8 == 0 {
                    env.trigger(1.0);
                }
                env.process(&mut out);
            }
        });
    }

    /// EDGE LENGTHS: zero, one, and a non-power-of-two block advance the
    /// state exactly as one long block would.
    #[test]
    fn exp_decay_handles_edge_block_lengths() {
        let mut env = ExpDecay::new();
        env.prepare(FS, 10.0);
        env.trigger(1.0);

        // A zero-length block is a no-op, not a step.
        let before = env.current();
        env.process(&mut []);
        assert_eq!(bits(env.current()), bits(before));

        let mut one = [0.0f32; 1];
        env.process(&mut one);
        assert_eq!(one[0], 1.0, "the first sample is still the strike");

        let mut odd = [0.0f32; 37];
        env.process(&mut odd);
        assert!(odd.windows(2).all(|w| w[1] < w[0]), "it only ever falls");

        // A run of odd blocks matches one long block, bit for bit.
        let mut a = ExpDecay::new();
        a.prepare(FS, 10.0);
        a.trigger(1.0);
        let mut long = vec![0.0f32; 111];
        a.process(&mut long);

        let mut b = ExpDecay::new();
        b.prepare(FS, 10.0);
        b.trigger(1.0);
        let mut pieces = vec![0.0f32; 111];
        for chunk in pieces.chunks_mut(7) {
            b.process(chunk);
        }
        assert!(long.iter().zip(&pieces).all(|(x, y)| bits(*x) == bits(*y)));
    }

    /// SILENCE AND THE DENORMAL TAIL. An exponential never mathematically
    /// reaches zero, so the floor is what makes the kernel terminate —
    /// and it must land on EXACT zero, well above the denormal range,
    /// rather than drifting down through it forever.
    #[test]
    fn exp_decay_reaches_exact_silence_and_stays_there() {
        let mut env = ExpDecay::new();
        env.prepare(FS, 5.0); // 240 samples to -60 dB
        env.trigger(1.0);

        // 2.4 times the decay time is past the -140 dB floor.
        let mut out = vec![0.0f32; 1_024];
        env.process(&mut out);
        assert!(!env.active(), "the tail never terminated");
        assert_eq!(env.current(), 0.0, "and it did not land on exact zero");
        assert!(
            out.iter().all(|v| *v == 0.0 || v.abs() >= ExpDecay::FLOOR),
            "a value was left in the denormal range"
        );

        // Idle stays idle and stays silent.
        let mut quiet = vec![0.0f32; 64];
        env.process(&mut quiet);
        assert!(quiet.iter().all(|v| *v == 0.0));

        // An untriggered envelope is silent from the start.
        let mut fresh = ExpDecay::new();
        fresh.prepare(FS, 100.0);
        assert!(!fresh.active());
        let mut none = vec![0.0f32; 32];
        fresh.process(&mut none);
        assert!(none.iter().all(|v| *v == 0.0));
    }

    /// The degenerate and hostile settings: an instantaneous decay is one
    /// sample, a retrigger replaces rather than sums, and nothing a caller
    /// can name makes the tail immortal or non-finite.
    #[test]
    fn exp_decay_survives_every_setting_a_caller_can_name() {
        // Zero and nonsense times are instantaneous: one sample, then out.
        for time in [0.0f32, -5.0, f32::NAN, f32::INFINITY] {
            let mut env = ExpDecay::new();
            env.prepare(FS, time);
            env.trigger(1.0);
            let mut out = [0.0f32; 4];
            env.process(&mut out);
            assert_eq!(out[0], 1.0, "time {time}");
            assert!(out[1..].iter().all(|v| *v == 0.0), "time {time}");
            assert!(!env.active(), "time {time}");
        }

        // A retrigger REPLACES the tail. Two strikes must not sum to 2.0.
        let mut env = ExpDecay::new();
        env.prepare(FS, 500.0);
        env.trigger(1.0);
        let mut out = [0.0f32; 64];
        env.process(&mut out);
        env.trigger(1.0);
        env.process(&mut out);
        assert_eq!(out[0], 1.0, "the second strike stacked onto the first");

        // A nonsense level reads as silence rather than poisoning the
        // state — and a hostile sample rate cannot make an immortal tail.
        env.trigger(f32::NAN);
        assert!(!env.active());
        let mut ridiculous = ExpDecay::new();
        ridiculous.prepare(1.0, 1.0e30);
        ridiculous.trigger(1.0);
        let mut long = vec![0.0f32; 4_096];
        ridiculous.process(&mut long);
        assert!(long.iter().all(|v| v.is_finite()));
        assert!(
            ridiculous.current() < 1.0,
            "the slowest decay must still be a decay"
        );
    }
}
