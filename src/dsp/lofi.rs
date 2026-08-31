//! The converter path: sample-rate reduction and bit reduction, in the
//! order and with the filtering an early hardware sampler had.
//!
//! # What this models
//!
//! An Akai S900/S950 recorded and played back through a converter running
//! well below the rest of the world's rate, with a TRACKING anti-alias
//! filter in front of it — the filter moved when you moved the rate. That
//! filter is why those machines sound grainy rather than broken: what
//! reaches the converter is band-limited, so the quantisation noise sits
//! on a signal that belongs there.
//!
//! Coming back OUT is where the character is. There is no reconstruction
//! filter here, and that is deliberate: the zero-order hold's images —
//! the mirror of the programme around the hold clock and its multiples —
//! are the sound. Filter them away and what is left is a lowpass with
//! hiss, which is not what anybody means by "12-bit". **Do not "fix" this
//! by adding a post-filter.**
//!
//! # The two off switches
//!
//! Both reductions reach an EXACT identity, checked per block rather than
//! per sample:
//!
//! - `rate >= sample_rate` — the hold latches every sample, so there is
//!   nothing to hold, and the pre-filter is skipped with it.
//! - `bits >= 16.0` — sixteen-bit steps are not a no-op on `f32`
//!   material, so fine quantisation is NOT the same as no quantisation.
//!   The quantiser is bypassed, not merely made small.
//!
//! With both at the top of their ranges `process` is a no-op and the
//! block passes through bit for bit. The sampler's transparency claim
//! rests on that, so it has a test of its own.
//!
//! # Clipping
//!
//! The quantiser's lattice covers `-1..=1` and clamps outside it, because
//! a converter has rails and running out of them is part of what it
//! sounds like. At `bits >= 16` nothing is clamped, so the bypass really
//! is a bypass.
//!
//! State: two one-pole integrators, a phase accumulator, a held sample —
//! about 40 bytes. Per-sample cost: two poles, a compare, and a round.
//! Denormal-safe: relies on engine FTZ; the poles are the only decaying
//! state and they never invent NaN from finite input.
//! In-place safe: yes.
//! Latency: 0 samples — see [`Downsampler::latency`].

use crate::dsp::filters::OnePole;

/// The slowest converter clock on offer, in Hz. Below about a kilohertz
/// the hold period is long enough to be heard as a buzz at its own pitch
/// rather than as a texture on the programme.
pub const RATE_MIN: f32 = 1_000.0;

/// Resolution limits. Two bits is four levels — a square wave with a
/// vague memory of the source. Sixteen is where the quantiser switches
/// itself off.
pub const BITS_MIN: f32 = 2.0;
pub const BITS_MAX: f32 = 16.0;

/// How far below the converter clock the tracking filter sits, as a
/// fraction. Just under a half: high enough to keep the programme, low
/// enough that what folds is quiet.
const PREFILTER_FRACTION: f32 = 0.45;

#[derive(Debug, Clone, Copy)]
pub struct Downsampler {
    /// Two cascaded one-poles: 12 dB/octave, which is all the slope a
    /// tracking filter of this vintage ever had.
    pre_a: OnePole,
    pre_b: OnePole,
    sample_rate: f32,
    rate_hz: f32,
    /// `rate_hz / sample_rate` — how much of a hold period one input
    /// sample is worth.
    step: f32,
    /// Wraps past 1.0 to latch. Starts AT 1.0 so the first sample of a
    /// fresh block latches rather than reading a stale hold.
    phase: f32,
    held: f32,
    /// `2 / 2^bits` — the spacing of the lattice.
    quantum: f32,
    rate_off: bool,
    bits_off: bool,
}

impl Default for Downsampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Downsampler {
    pub fn new() -> Self {
        Self {
            pre_a: OnePole::new(),
            pre_b: OnePole::new(),
            sample_rate: 48_000.0,
            rate_hz: 48_000.0,
            step: 1.0,
            phase: 1.0,
            held: 0.0,
            quantum: 0.0,
            rate_off: true,
            bits_off: true,
        }
    }

    /// Green zone. Leaves the rate at "off" and the resolution at "off";
    /// the caller sets both.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.set_rate(self.sample_rate);
        self.set_bits(BITS_MAX);
        self.reset();
    }

    /// Green zone: the converter clock, in Hz. Clamped into
    /// `RATE_MIN..=sample_rate`; at the top the hold switches off.
    ///
    /// Moves the tracking pre-filter with it, which is the whole point of
    /// the pair.
    pub fn set_rate(&mut self, hz: f32) {
        let hz = if hz.is_finite() {
            hz.clamp(RATE_MIN, self.sample_rate)
        } else {
            self.sample_rate
        };
        self.rate_hz = hz;
        self.step = hz / self.sample_rate;
        self.rate_off = hz >= self.sample_rate;
        let corner = (hz * PREFILTER_FRACTION).min(self.sample_rate * 0.499);
        self.pre_a.prepare(self.sample_rate, corner);
        self.pre_b.prepare(self.sample_rate, corner);
    }

    /// Green zone: resolution in bits, CONTINUOUS. Twelve and a half bits
    /// is a perfectly well defined lattice and is sometimes the one that
    /// suits the material; stepping this knob to integers would offer
    /// fourteen positions of which thirteen are wrong.
    pub fn set_bits(&mut self, bits: f32) {
        let bits = if bits.is_finite() {
            bits.clamp(BITS_MIN, BITS_MAX)
        } else {
            BITS_MAX
        };
        self.bits_off = bits >= BITS_MAX;
        // 2 units of range (−1..=1) over 2^bits levels.
        self.quantum = 2.0 / bits.exp2();
    }

    /// Green zone: zero the filter state and arm the hold to latch on the
    /// next sample. Keeps the settings.
    pub fn reset(&mut self) {
        self.pre_a.reset();
        self.pre_b.reset();
        self.phase = 1.0;
        self.held = 0.0;
    }

    /// Zero, and the doc header says why: a zero-order hold delays by
    /// somewhere between nothing and one hold period depending on where
    /// in the phase a given sample fell, so there is no integer number of
    /// samples that compensating for it would be correct at. Reporting a
    /// number that is right on average and wrong every sample is worse
    /// than reporting none.
    pub fn latency(&self) -> usize {
        0
    }

    /// The converter clock actually in use, in Hz.
    pub fn rate(&self) -> f32 {
        self.rate_hz
    }

    /// Whether this instance is currently a wire.
    pub fn is_bypassed(&self) -> bool {
        self.rate_off && self.bits_off
    }

    #[inline(always)]
    fn quantise(&self, x: f32) -> f32 {
        if self.bits_off {
            return x;
        }
        let clamped = x.clamp(-1.0, 1.0);
        (clamped / self.quantum).round() * self.quantum
    }

    /// Red zone: in place, any length.
    pub fn process(&mut self, io: &mut [f32]) {
        // The bypass is decided ONCE per block. Two bools tested per
        // sample would be the same answer eight hundred times.
        if self.is_bypassed() {
            return;
        }
        if self.rate_off {
            for s in io.iter_mut() {
                *s = self.quantise(*s);
            }
            return;
        }
        for s in io.iter_mut() {
            let filtered = self.pre_b.tick_lowpass(self.pre_a.tick_lowpass(*s));
            self.phase += self.step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = self.quantise(filtered);
            }
            *s = self.held;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed(rate: f32, bits: f32) -> Downsampler {
        let mut d = Downsampler::new();
        d.prepare(48_000.0);
        d.set_rate(rate);
        d.set_bits(bits);
        d.reset();
        d
    }

    /// The identity the sampler's transparency claim rests on.
    #[test]
    fn the_top_of_both_ranges_is_a_wire() {
        let mut d = armed(48_000.0, 16.0);
        assert!(d.is_bypassed());
        let source: Vec<f32> = (0..512).map(|i| (i as f32 * 0.033).sin() * 1.7).collect();
        let mut io = source.clone();
        d.process(&mut io);
        assert_eq!(io, source, "a bypassed converter must not touch a sample");
    }

    /// Reference correctness, half one: the hold is a staircase, and its
    /// runs are the two integers either side of `sample_rate / rate`.
    /// Nothing else is a zero-order hold.
    #[test]
    fn the_hold_is_a_staircase_with_the_right_tread() {
        let mut d = armed(8_000.0, 16.0);
        let mut io: Vec<f32> = (0..4_800).map(|i| (i as f32 * 0.01).sin()).collect();
        d.process(&mut io);

        let ratio: f32 = 48_000.0 / 8_000.0;
        let low = ratio.floor() as usize;
        let high = ratio.ceil() as usize;

        let mut runs = Vec::new();
        let mut run = 1usize;
        for pair in io.windows(2) {
            if pair[0] == pair[1] {
                run += 1;
            } else {
                runs.push(run);
                run = 1;
            }
        }
        // The first and last runs are truncated by the block's edges.
        let interior = &runs[1..runs.len() - 1];
        assert!(!interior.is_empty(), "no steps at all");
        for r in interior {
            assert!(
                *r == low || *r == high,
                "tread {r} is neither {low} nor {high}"
            );
        }
    }

    /// Reference correctness, half two: quantised output lives on the
    /// lattice and nowhere else.
    #[test]
    fn quantised_output_only_takes_lattice_values() {
        for bits in [2.0f32, 4.0, 8.0, 12.0, 15.5] {
            let mut d = armed(48_000.0, bits);
            let mut io: Vec<f32> = (0..2_000).map(|i| (i as f32 * 0.017).sin()).collect();
            d.process(&mut io);
            let quantum = 2.0 / bits.exp2();
            for s in &io {
                let steps = s / quantum;
                assert!(
                    (steps - steps.round()).abs() < 1e-3,
                    "{s} is off the {bits}-bit lattice"
                );
            }
        }
    }

    /// Split-block equivalence, over the state most likely to be wrong:
    /// the phase accumulator, which a lazy implementation resets at every
    /// block boundary and which would then never straddle one.
    #[test]
    fn split_blocks_match_a_whole_one() {
        let source: Vec<f32> = (0..1_000).map(|i| (i as f32 * 0.021).sin() * 0.8).collect();

        let mut whole = source.clone();
        armed(11_025.0, 12.0).process(&mut whole);

        let mut split = source.clone();
        let mut d = armed(11_025.0, 12.0);
        let (a, b) = split.split_at_mut(377);
        d.process(a);
        d.process(b);

        assert_eq!(whole, split, "a block boundary changed the answer");
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut d = armed(22_050.0, 10.0);
        let mut io = vec![0.3f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 8 == 0 {
                    d.set_rate(4_000.0 + (i % 5) as f32 * 3_000.0);
                    d.set_bits(4.0 + (i % 9) as f32);
                }
                d.process(&mut io);
            }
        });
    }

    /// Edge lengths, and every one of them on a fresh instance so the
    /// first-sample latch is exercised too.
    #[test]
    fn every_block_length_is_survivable() {
        for len in [0usize, 1, 2, 3, 7, 63, 257] {
            let mut d = armed(6_000.0, 6.0);
            let mut io: Vec<f32> = (0..len).map(|i| (i as f32 * 0.3).sin()).collect();
            d.process(&mut io);
            assert!(
                io.iter().all(|s| s.is_finite()),
                "len {len} went non-finite"
            );
        }
    }

    /// Silence in, silence out — exactly, not nearly. A quantiser whose
    /// lattice is offset would put a DC step here and it would reach the
    /// master bus on every idle voice.
    #[test]
    fn silence_stays_exactly_silent() {
        for bits in [2.0f32, 7.0, 12.0] {
            for rate in [1_000.0f32, 8_000.0, 48_000.0] {
                let mut d = armed(rate, bits);
                let mut io = vec![0.0f32; 512];
                d.process(&mut io);
                assert!(
                    io.iter().all(|s| *s == 0.0),
                    "{bits} bits at {rate} Hz invented {:?}",
                    io.iter().find(|s| **s != 0.0)
                );
            }
        }
    }

    /// A decaying tail stays finite all the way down, and the clamp holds
    /// against material that is already over the rails.
    #[test]
    fn hot_and_decaying_input_stays_in_range() {
        let mut d = armed(16_000.0, 8.0);
        let mut io: Vec<f32> = (0..4_000)
            .map(|i| (i as f32 * 0.05).sin() * 4.0 * (-(i as f32) / 400.0).exp())
            .collect();
        d.process(&mut io);
        for s in &io {
            assert!(s.is_finite(), "went non-finite");
            assert!(s.abs() <= 1.0 + 1e-6, "{s} escaped the converter's rails");
        }
    }

    /// The tracking filter tracks. Drop the rate and the thing being
    /// sampled must lose its highs, or the aliasing is uncontrolled.
    #[test]
    fn the_prefilter_moves_with_the_rate() {
        // 6 kHz tone. At a 48 k clock it passes; at a 4 k clock the
        // pre-filter sits at 1.8 k and it must be well down.
        let tone = |n: usize| -> Vec<f32> {
            (0..n)
                .map(|i| (std::f32::consts::TAU * 6_000.0 * i as f32 / 48_000.0).sin())
                .collect()
        };
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();

        let mut open = armed(48_000.0, 16.0);
        let mut a = tone(4_800);
        open.process(&mut a);

        let mut narrow = armed(4_000.0, 16.0);
        let mut b = tone(4_800);
        narrow.process(&mut b);

        let drop_db = 20.0 * (rms(&b) / rms(&a)).log10();
        assert!(
            drop_db < -12.0,
            "6 kHz through a 4 kHz converter only dropped {drop_db:.1} dB"
        );
    }

    /// What a sample costs, in nanoseconds. Printed, not asserted: a
    /// timing threshold fails on a loaded box and tells you nothing.
    ///
    /// Run in RELEASE for the figures the header quotes — plain `cargo
    /// test` builds this crate at `opt-level = 1` and reads several
    /// times slower — and when comparing a change across two runs, keep
    /// a CONTROL row the change cannot touch: a preceding build leaves
    /// the machine hot enough to move every number here by 2x.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 8_000;
        let row = |name: &str, run: &mut dyn FnMut()| {
            for _ in 0..500 {
                run();
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run();
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!(
                "{name:<26} {ns:7.2} ns/sample   {:5.3}% of a core at 48k",
                ns * 48_000.0 * 1e-9 * 100.0
            );
        };
        let sig: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.07).sin() * 0.4).collect();

        let mut d = Downsampler::new();
        d.prepare(48_000.0);
        d.set_rate(12_000.0);
        d.set_bits(8.0);
        let mut io = sig.clone();
        row("downsampler + crush", &mut || {
            d.process(&mut io);
            std::hint::black_box(&mut io);
        });
    }
}
