//! ADSR envelope generator.
//!
//! The caller changes the gate only between audio blocks with [`Adsr::gate_on`]
//! and [`Adsr::gate_off`]. [`Adsr::process`] then writes one envelope value per
//! sample. This is deliberately an output-only control kernel: a voice node
//! decides which signal(s) the envelope modulates.

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

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

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
}
