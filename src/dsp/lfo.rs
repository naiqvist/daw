//! Family 4 of the kernel roadmap: LFOs and modulation.
//!
//! [`Lfo`] is a naive, bipolar oscillator for modulation — LFO-grade, not
//! for audio rate. [`SampleHold`] provides deterministic stepped randomness,
//! and [`SlewLimiter`] turns steps into bounded-rate movement. Tempo syncing
//! stays at the call site through [`hz_for`]; these kernels know only Hz.

use crate::dsp::noise::WhiteNoise;

const PHASE_SCALE: f64 = (1u64 << 32) as f64;
const PHASE_SCALE_INV: f32 = 1.0 / PHASE_SCALE as f32;

/// Convert a tempo-relative modulation speed to Hz.
///
/// This is deliberately plain arithmetic rather than a transport-aware
/// kernel: the caller owns tempo and changes to it.
pub fn hz_for(tempo_bpm: f32, cycles_per_beat: f32) -> f32 {
    let hz = tempo_bpm * cycles_per_beat / 60.0;
    if hz.is_finite() && hz >= 0.0 { hz } else { 0.0 }
}

/// Naive LFO waveform. All shapes are bipolar in `-1..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoShape {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
}

/// Fixed-point, bipolar modulation oscillator.
///
/// This is LFO-grade, not for audio rate: its naive corners are intentional
/// below modulation rates, where band-limiting would only soften the shape.
/// Phase is a u32 turn accumulator, so wrapping is exact and drift-free.
///
/// State: 16 bytes. Per-sample cost: 1 u32 add plus waveform arithmetic
/// (sine calls `sin`; the other shapes use 1–2 branches and basic math).
/// Denormal-safe: outputs are bounded and never denormal for ordinary phase.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Lfo {
    phase: u32,
    inc: u32,
    sample_rate: f32,
    shape: LfoShape,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0,
            inc: 0,
            sample_rate: 48_000.0,
            shape: LfoShape::Sine,
        }
    }

    /// Green zone: set the sample rate. This resets the rate to zero; set a
    /// new rate afterward, as is normal during node preparation.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = valid_sample_rate(sample_rate);
        self.inc = 0;
    }

    /// Green zone: select a waveform without changing phase.
    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    /// Green zone: set frequency in Hz. Invalid and negative values hold;
    /// rates above Nyquist clamp to Nyquist, the highest meaningful rate for
    /// this sampled control signal.
    pub fn set_rate(&mut self, hz: f32) {
        self.inc = phase_increment(self.sample_rate, hz);
    }

    /// Green zone: set phase in turns. `1.0` is the same cycle boundary as
    /// `0.0`; invalid values become zero.
    pub fn set_phase(&mut self, turns: f32) {
        self.phase = phase_from_turns(turns);
    }

    /// Green zone: return phase to the cycle boundary, keeping rate and shape.
    pub fn reset(&mut self) {
        self.phase = 0;
    }

    /// Red zone: write one bipolar modulation value for each output sample.
    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = shape_at(self.shape, self.phase);
            self.phase = self.phase.wrapping_add(self.inc);
        }
    }
}

/// Deterministic random-value LFO. A value is drawn at reset/seed and then
/// exactly once on every phase wrap; it holds unchanged between wraps.
///
/// State: 32 bytes. Per-sample cost: 1 u32 add + one wrap comparison; a
/// wrap additionally draws one [`WhiteNoise`] sample.
/// Denormal-safe: held values come from uniform noise in `[-1, 1)`.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct SampleHold {
    phase: u32,
    inc: u32,
    sample_rate: f32,
    value: f32,
    noise: WhiteNoise,
}

impl Default for SampleHold {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleHold {
    pub fn new() -> Self {
        let mut result = Self {
            phase: 0,
            inc: 0,
            sample_rate: 48_000.0,
            value: 0.0,
            noise: WhiteNoise::new(),
        };
        result.next_value();
        result
    }

    /// Green zone: set the sample rate and hold the current value until a
    /// subsequent [`set_rate`](Self::set_rate) call.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = valid_sample_rate(sample_rate);
        self.inc = 0;
    }

    /// Green zone: select the deterministic random stream and restart its
    /// first held value at phase zero.
    pub fn seed(&mut self, seed: u64) {
        self.noise.seed(seed);
        self.phase = 0;
        self.next_value();
    }

    /// Green zone: set rate in Hz; invalid/negative values hold, and rates
    /// above Nyquist clamp to Nyquist.
    pub fn set_rate(&mut self, hz: f32) {
        self.inc = phase_increment(self.sample_rate, hz);
    }

    /// Green zone: set phase in turns. `1.0` wraps to the same boundary as
    /// `0.0`; changing phase does not draw a new held value.
    pub fn set_phase(&mut self, turns: f32) {
        self.phase = phase_from_turns(turns);
    }

    /// Green zone: return to phase zero and the first value of the seeded
    /// stream, keeping sample rate and rate.
    pub fn reset(&mut self) {
        self.phase = 0;
        self.noise.reset();
        self.next_value();
    }

    /// Current held random value.
    pub fn current(&self) -> f32 {
        self.value
    }

    /// Red zone: write the held value, drawing the next one exactly when the
    /// fixed-point phase wraps. Nyquist clamping guarantees at most one wrap
    /// per output sample.
    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = self.value;
            let next_phase = self.phase.wrapping_add(self.inc);
            if next_phase < self.phase {
                self.next_value();
            }
            self.phase = next_phase;
        }
    }

    #[inline(always)]
    fn next_value(&mut self) {
        let mut one = [0.0; 1];
        self.noise.process(&mut one);
        self.value = one[0];
    }
}

/// Asymmetric bounded-rate control smoother.
///
/// State: 24 bytes. Per-sample cost: 2 comparisons plus one add or assign.
/// Denormal-safe: relies on engine FTZ; a very slow approach can traverse
/// denormals. Finite input and settings never invent NaN or infinity.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct SlewLimiter {
    value: f32,
    sample_rate: f32,
    rise_per_s: f32,
    fall_per_s: f32,
    rise_step: f32,
    fall_step: f32,
}

impl Default for SlewLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl SlewLimiter {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            sample_rate: 48_000.0,
            rise_per_s: 0.0,
            fall_per_s: 0.0,
            rise_step: 0.0,
            fall_step: 0.0,
        }
    }

    /// Green zone: set the sample rate, preserving the configured rates and
    /// recalculating their per-sample limits.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = valid_sample_rate(sample_rate);
        self.update_steps();
    }

    /// Green zone: set positive rise and fall speeds in units per second.
    /// Invalid or negative rates become zero (hold).
    pub fn set_rates(&mut self, rise_per_s: f32, fall_per_s: f32) {
        self.rise_per_s = valid_rate(rise_per_s);
        self.fall_per_s = valid_rate(fall_per_s);
        self.update_steps();
    }

    /// Return to exact zero, keeping sample rate and configured rates.
    pub fn reset(&mut self) {
        self.value = 0.0;
    }

    /// Current output value.
    pub fn current(&self) -> f32 {
        self.value
    }

    /// Red zone: limit each input step in place by the configured rise/fall
    /// amount. A NaN input affects only that output sample; a following finite
    /// input resumes from that finite target.
    pub fn process(&mut self, io: &mut [f32]) {
        for sample in io.iter_mut() {
            let target = *sample;
            let delta = target - self.value;
            // Accumulating a decimal step can leave the value a few ulps
            // beyond the target after its nominal arrival sample (for
            // example, ten 0.2 steps).  Treat that rounding residue as an
            // arrival so a configured finite target is reached exactly.
            let tolerance = f32::EPSILON * delta.abs().max(1.0);
            if delta > self.rise_step + tolerance {
                self.value += self.rise_step;
            } else if delta < -self.fall_step - tolerance {
                self.value -= self.fall_step;
            } else {
                self.value = target;
            }
            *sample = self.value;
        }
    }

    fn update_steps(&mut self) {
        self.rise_step = self.rise_per_s / self.sample_rate;
        self.fall_step = self.fall_per_s / self.sample_rate;
    }
}

fn valid_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        48_000.0
    }
}

fn valid_rate(rate: f32) -> f32 {
    if rate.is_finite() && rate >= 0.0 {
        rate
    } else {
        0.0
    }
}

fn phase_increment(sample_rate: f32, hz: f32) -> u32 {
    let hz = valid_rate(hz).min(sample_rate * 0.5);
    ((f64::from(hz) / f64::from(sample_rate)) * PHASE_SCALE) as u32
}

fn phase_from_turns(turns: f32) -> u32 {
    if !turns.is_finite() || turns <= 0.0 || turns >= 1.0 {
        0
    } else {
        (f64::from(turns) * PHASE_SCALE) as u32
    }
}

#[inline(always)]
fn shape_at(shape: LfoShape, phase: u32) -> f32 {
    let turns = phase as f32 * PHASE_SCALE_INV;
    match shape {
        LfoShape::Sine => (core::f32::consts::TAU * turns).sin(),
        LfoShape::Triangle => {
            if turns < 0.25 {
                4.0 * turns
            } else if turns < 0.75 {
                2.0 - 4.0 * turns
            } else {
                4.0 * turns - 4.0
            }
        }
        LfoShape::SawUp => 2.0 * turns - 1.0,
        LfoShape::SawDown => 1.0 - 2.0 * turns,
        LfoShape::Square => {
            if phase < 0x8000_0000 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn bits(value: f32) -> u32 {
        value.to_bits()
    }

    // -------------------------------------------------------- reference ---

    #[test]
    fn lfo_shapes_are_correct_at_quarter_phases() {
        let mut lfo = Lfo::new();
        lfo.prepare(FS);
        lfo.set_rate(0.0);

        let read = |lfo: &mut Lfo, shape, phase| {
            lfo.set_shape(shape);
            lfo.set_phase(phase);
            let mut out = [0.0; 1];
            lfo.process(&mut out);
            out[0]
        };

        assert!(read(&mut lfo, LfoShape::Sine, 0.25) > 0.999_99);
        assert_eq!(read(&mut lfo, LfoShape::Triangle, 0.25), 1.0);
        assert_eq!(read(&mut lfo, LfoShape::Triangle, 0.75), -1.0);
        assert_eq!(read(&mut lfo, LfoShape::SawUp, 0.0), -1.0);
        assert_eq!(read(&mut lfo, LfoShape::SawDown, 0.0), 1.0);
        for phase in [0.0, 0.25, 0.5, 0.75] {
            assert_eq!(
                read(&mut lfo, LfoShape::SawUp, phase),
                -read(&mut lfo, LfoShape::SawDown, phase)
            );
        }
        assert_eq!(read(&mut lfo, LfoShape::Square, 0.499), 1.0);
        assert_eq!(read(&mut lfo, LfoShape::Square, 0.5), -1.0);
    }

    #[test]
    fn sample_hold_changes_once_per_period_and_seeds_decorrelate() {
        let mut held = SampleHold::new();
        held.prepare(64.0);
        held.seed(7);
        held.set_rate(8.0); // Exactly eight samples per fixed-point period.
        let mut out = [0.0; 32];
        held.process(&mut out);
        for period in out.as_chunks::<8>().0 {
            assert!(period.iter().all(|sample| bits(*sample) == bits(period[0])));
        }
        assert_ne!(bits(out[0]), bits(out[8]));
        assert_ne!(bits(out[8]), bits(out[16]));

        let mut other = SampleHold::new();
        other.prepare(64.0);
        other.seed(8);
        other.set_rate(8.0);
        let mut different = [0.0; 32];
        other.process(&mut different);
        assert!(
            out.iter()
                .zip(&different)
                .any(|(a, b)| bits(*a) != bits(*b))
        );
    }

    #[test]
    fn slew_rate_and_arrival_match_the_configuration() {
        let mut slew = SlewLimiter::new();
        slew.prepare(100.0);
        slew.set_rates(10.0, 20.0);
        let mut rising = [1.0; 12];
        slew.process(&mut rising);
        assert_eq!(rising[9], 1.0, "one unit at 10 units/s takes 0.1 s");

        let mut falling = [-1.0; 11];
        slew.process(&mut falling);
        assert_eq!(falling[9], -1.0, "two units at 20 units/s takes 0.1 s");

        slew.reset();
        slew.prepare(FS);
        slew.set_rates(3.0, 5.0);
        let mut varied = [0.0; 127];
        for (index, sample) in varied.iter_mut().enumerate() {
            *sample = ((index as f32) * 0.17).sin();
        }
        slew.process(&mut varied);
        let mut previous = 0.0;
        for sample in varied {
            let limit = if sample >= previous {
                3.0 / FS
            } else {
                5.0 / FS
            };
            let tolerance = limit.max(1.0) * f32::EPSILON;
            assert!((sample - previous).abs() <= limit + tolerance);
            previous = sample;
        }
    }

    #[test]
    fn awkward_rate_period_is_accurate_over_ten_seconds() {
        let mut lfo = Lfo::new();
        lfo.prepare(FS);
        lfo.set_rate(1.337);
        let mut out = vec![0.0; (FS * 10.0) as usize];
        lfo.process(&mut out);
        let cycles = f64::from(lfo.inc) * out.len() as f64 / PHASE_SCALE;
        let expected = 13.37;
        assert!(
            (cycles - expected).abs() / expected < 1e-4,
            "expected {expected} cycles, got {cycles}"
        );
    }

    // ------------------------------------------- split-block bit-exactness

    #[test]
    fn split_block_is_bit_exact() {
        let mut full_lfo = Lfo::new();
        let mut split_lfo = Lfo::new();
        for lfo in [&mut full_lfo, &mut split_lfo] {
            lfo.prepare(FS);
            lfo.set_shape(LfoShape::Triangle);
            lfo.set_rate(1.337);
            lfo.set_phase(0.17);
        }
        let mut full = [0.0; 256];
        let mut split = [0.0; 256];
        full_lfo.process(&mut full);
        split_lfo.process(&mut split[..100]);
        split_lfo.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let mut full_hold = SampleHold::new();
        let mut split_hold = SampleHold::new();
        for hold in [&mut full_hold, &mut split_hold] {
            hold.prepare(FS);
            hold.seed(91);
            hold.set_rate(3.0);
        }
        full_hold.process(&mut full);
        split_hold.process(&mut split[..100]);
        split_hold.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let mut full_slew = SlewLimiter::new();
        let mut split_slew = SlewLimiter::new();
        for slew in [&mut full_slew, &mut split_slew] {
            slew.prepare(FS);
            slew.set_rates(2.0, 3.0);
        }
        let source: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.13).sin()).collect();
        full.copy_from_slice(&source);
        split.copy_from_slice(&source);
        full_slew.process(&mut full);
        split_slew.process(&mut split[..100]);
        split_slew.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn process_does_not_allocate() {
        let mut lfo = Lfo::new();
        lfo.prepare(FS);
        lfo.set_rate(2.0);
        let mut hold = SampleHold::new();
        hold.prepare(FS);
        hold.set_rate(2.0);
        let mut slew = SlewLimiter::new();
        slew.prepare(FS);
        slew.set_rates(2.0, 2.0);
        let mut generated = [0.0; 64];
        let mut io = [0.5; 64];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                lfo.process(&mut generated);
                hold.process(&mut generated);
                slew.process(&mut io);
            }
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn accepts_zero_one_and_non_power_of_two_lengths() {
        let mut lfo = Lfo::new();
        lfo.prepare(FS);
        lfo.set_rate(2.0);
        let mut hold = SampleHold::new();
        hold.prepare(FS);
        hold.set_rate(2.0);
        let mut slew = SlewLimiter::new();
        slew.prepare(FS);
        slew.set_rates(2.0, 2.0);
        for length in [0usize, 1, 7] {
            let mut a = vec![0.0; length];
            let mut b = vec![0.0; length];
            let mut c = vec![0.5; length];
            lfo.process(&mut a);
            hold.process(&mut b);
            slew.process(&mut c);
            assert!(
                a.iter()
                    .chain(&b)
                    .chain(&c)
                    .all(|sample| sample.is_finite())
            );
        }

        lfo.set_rate(f32::NAN);
        hold.set_rate(-1.0);
        lfo.set_phase(f32::INFINITY);
        hold.set_phase(f32::NAN);
        let mut a = [0.0; 1];
        let mut b = [0.0; 1];
        lfo.process(&mut a);
        hold.process(&mut b);
        assert!(a[0].is_finite() && b[0].is_finite());
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn reset_is_deterministic_and_tails_stay_finite() {
        let mut hold = SampleHold::new();
        hold.prepare(FS);
        hold.seed(13);
        hold.set_rate(5.0);
        let mut first = [0.0; 64];
        hold.process(&mut first);
        hold.reset();
        let mut again = [0.0; 64];
        hold.process(&mut again);
        assert!(first.iter().zip(&again).all(|(a, b)| bits(*a) == bits(*b)));

        let mut slew = SlewLimiter::new();
        slew.prepare(FS);
        slew.set_rates(0.5, 0.5);
        let mut silence = [0.0; 64];
        slew.process(&mut silence);
        assert!(silence.iter().all(|sample| *sample == 0.0));

        let mut tail = [0.0; 256];
        let mut value = 1.0;
        for sample in tail.iter_mut() {
            *sample = value;
            value *= 0.5;
        }
        tail[255] = f32::from_bits(1);
        slew.process(&mut tail);
        assert!(tail.iter().all(|sample| sample.is_finite()));
    }

    // ---------------------------------------------------------------- cost

    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;

        const BLOCK: usize = 256;
        const REPS: usize = 10_000;
        let mut buf = vec![0.0; BLOCK];
        let mut row = |name: &str, run: &mut dyn FnMut(&mut [f32])| {
            for _ in 0..1_000 {
                run(&mut buf);
            }
            let start = Instant::now();
            for _ in 0..REPS {
                run(&mut buf);
                std::hint::black_box(&mut buf);
            }
            let ns = start.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<18} {ns:6.2} ns/sample");
        };

        let mut lfo = Lfo::new();
        lfo.prepare(FS);
        lfo.set_rate(3.0);
        row("lfo sine", &mut |out| lfo.process(out));
        let mut hold = SampleHold::new();
        hold.prepare(FS);
        hold.set_rate(3.0);
        row("sample hold", &mut |out| hold.process(out));
        let mut slew = SlewLimiter::new();
        slew.prepare(FS);
        slew.set_rates(3.0, 3.0);
        row("slew limiter", &mut |out| slew.process(out));
    }
}
