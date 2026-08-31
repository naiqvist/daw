//! Linkwitz–Riley band splitting — one signal in, three out, and they add
//! back up.
//!
//! # Why this and not three bandpasses
//!
//! Three independent bandpass filters do not reconstruct. Their phases
//! disagree around the corners, so summing them dips where they meet and
//! a multiband processor built that way is coloured before it has done
//! anything. A Linkwitz–Riley pair does reconstruct: LP4 and HP4 at the
//! same corner sum to an ALLPASS — flat magnitude, all the phase — so
//! whatever the bands do individually, doing nothing to them gives back
//! what came in.
//!
//! # The third band's tax
//!
//! Three bands is two splits, and the second split imposes its phase on
//! the two bands that go through it and not on the one that does not. So
//! the low band pays the same phase without the same filtering: it runs
//! through a second-order ALLPASS at the upper corner. Skip that and the
//! low band arrives early relative to the mid, and the sum notches at the
//! LOWER corner — a bug that measures as a 3 dB hole hundreds of hertz
//! away from the filter that caused it.
//!
//! ```text
//! x ──┬─ LP4(f1) ── AP2(f2) ─────────────► low
//!     └─ HP4(f1) ─┬─ LP4(f2) ───────────► mid
//!                 └─ HP4(f2) ───────────► high
//! ```
//!
//! State: 9 [`Svf`] (about 300 bytes). Per-sample cost: 9 SVF ticks.
//! Denormal-safe: relies on engine FTZ, same as [`Svf`].
//! In-place safe: N/A — the outputs are separate slices from the input.
//! Latency: 0 samples.

use crate::dsp::filters::{Mode, Svf};

/// Butterworth Q. Two of these in series is one Linkwitz–Riley 4.
const BUTTER_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// The narrowest the two corners may sit, as a ratio.
///
/// Corners that cross would put the mid band's lowpass below its own
/// highpass, which is not a narrow band but an empty one — and an empty
/// band whose gain still moves is a control that does nothing audible
/// while looking like it should.
pub const MIN_SPREAD: f32 = 1.25;

/// A pair of Butterworth sections at one corner: Linkwitz–Riley 4th
/// order, 24 dB/octave.
#[derive(Debug, Clone, Copy)]
struct Lr4 {
    a: Svf,
    b: Svf,
}

impl Lr4 {
    fn new() -> Self {
        Self {
            a: Svf::new(),
            b: Svf::new(),
        }
    }

    fn prepare(&mut self, sample_rate: f32, hz: f32) {
        self.a.prepare(sample_rate, hz, BUTTER_Q);
        self.b.prepare(sample_rate, hz, BUTTER_Q);
    }

    fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
    }

    fn process(&mut self, io: &mut [f32], mode: Mode) {
        self.a.process(io, mode);
        self.b.process(io, mode);
    }
}

/// Three bands from one signal, phase-matched so they sum flat.
#[derive(Debug, Clone, Copy)]
pub struct Crossover3 {
    low_1: Lr4,
    high_1: Lr4,
    low_2: Lr4,
    high_2: Lr4,
    /// The low band's phase tax — see the module header.
    align: Svf,
}

impl Default for Crossover3 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crossover3 {
    pub fn new() -> Self {
        Self {
            low_1: Lr4::new(),
            high_1: Lr4::new(),
            low_2: Lr4::new(),
            high_2: Lr4::new(),
            align: Svf::new(),
        }
    }

    /// Green zone. `low_hz` and `high_hz` are the two corners; they are
    /// ordered and separated here rather than trusted, because a caller
    /// with two knobs will cross them.
    pub fn prepare(&mut self, sample_rate: f32, low_hz: f32, high_hz: f32) {
        let (low, high) = Self::corners(sample_rate, low_hz, high_hz);
        self.low_1.prepare(sample_rate, low);
        self.high_1.prepare(sample_rate, low);
        self.low_2.prepare(sample_rate, high);
        self.high_2.prepare(sample_rate, high);
        self.align.prepare(sample_rate, high, BUTTER_Q);
    }

    /// The corners this kernel would actually use, in order and apart.
    ///
    /// Public because a display draws the seams a user dragged, and
    /// drawing where they were ASKED for rather than where they LANDED
    /// is how a picture stops agreeing with the sound.
    pub fn corners(sample_rate: f32, low_hz: f32, high_hz: f32) -> (f32, f32) {
        let nyquist = (sample_rate * 0.5).max(2.0);
        let ceiling = nyquist * 0.95;
        let low = if low_hz.is_finite() { low_hz } else { 200.0 };
        let high = if high_hz.is_finite() {
            high_hz
        } else {
            2_000.0
        };
        let low = low.clamp(1.0, ceiling / MIN_SPREAD);
        let high = high.clamp(low * MIN_SPREAD, ceiling);
        (low, high)
    }

    /// Green zone: zero every integrator, keep the coefficients.
    pub fn reset(&mut self) {
        self.low_1.reset();
        self.high_1.reset();
        self.low_2.reset();
        self.high_2.reset();
        self.align.reset();
    }

    /// Zero, and stated so callers do not have to guess.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: split `input` into three bands.
    ///
    /// Every output is written over, never accumulated, and only as far
    /// as the shortest slice — a caller that hands over mismatched
    /// buffers gets less signal, never an out-of-bounds.
    pub fn process(&mut self, input: &[f32], low: &mut [f32], mid: &mut [f32], high: &mut [f32]) {
        let n = input.len().min(low.len()).min(mid.len()).min(high.len());
        let (Some(input), Some(low), Some(mid), Some(high)) = (
            input.get(..n),
            low.get_mut(..n),
            mid.get_mut(..n),
            high.get_mut(..n),
        ) else {
            return;
        };

        // The low band, and the high half that still has to be split.
        low.copy_from_slice(input);
        mid.copy_from_slice(input);
        self.low_1.process(low, Mode::Lowpass);
        self.high_1.process(mid, Mode::Highpass);

        // `mid` is carrying everything above the first corner; the top
        // of it is what becomes `high`.
        high.copy_from_slice(mid);
        self.low_2.process(mid, Mode::Lowpass);
        self.high_2.process(high, Mode::Highpass);

        // And the low band pays the upper split's phase without having
        // been through it.
        self.align.process(low, Mode::Allpass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn split(x: &mut Crossover3, input: &[f32]) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut low = vec![0.0; input.len()];
        let mut mid = vec![0.0; input.len()];
        let mut high = vec![0.0; input.len()];
        x.process(input, &mut low, &mut mid, &mut high);
        (low, mid, high)
    }

    fn tone(len: usize, hz: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    fn peak(block: &[f32]) -> f32 {
        block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// RMS, not peak, for anything measured against a TONE.
    ///
    /// A 12 kHz sine at 48 kHz is four samples a cycle, and its sampled
    /// peak depends on where the phase happens to land — so a filter
    /// that only shifted phase would measure as 0.75 dB of loss. RMS
    /// over whole cycles does not care about phase, which is the whole
    /// property being tested here.
    fn rms(block: &[f32]) -> f32 {
        if block.is_empty() {
            return 0.0;
        }
        (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
    }

    /// REFERENCE: the three bands add back up to what came in.
    ///
    /// Not sample for sample — the sum is an allpass of the input, so
    /// the phase has moved. MAGNITUDE is what reconstructs, and it is
    /// what the claim is: a multiband processor doing nothing has to be
    /// inaudible.
    ///
    /// Measured well past the warm-up, and at the corners as well as
    /// between them: the corners are where a missing alignment allpass
    /// shows, and they are exactly where a careless test does not look.
    #[test]
    fn the_bands_sum_back_to_the_signal() {
        for hz in [30.0, 90.0, 200.0, 400.0, 900.0, 2_000.0, 5_000.0, 12_000.0] {
            let mut x = Crossover3::new();
            x.prepare(FS, 200.0, 2_000.0);
            let input = tone(16_384, hz);
            let (low, mid, high) = split(&mut x, &input);

            let settled = 8_192;
            let sum: Vec<f32> = (settled..input.len())
                .filter_map(|i| Some(low.get(i)? + mid.get(i)? + high.get(i)?))
                .collect();
            let want = rms(input.get(settled..).unwrap_or(&[]));
            let got = rms(&sum);
            let db = 20.0 * (got / want).log10();
            assert!(
                db.abs() < 0.35,
                "at {hz} Hz the sum is {db:+.2} dB off flat"
            );
        }
    }

    /// And each band actually IS a band: 24 dB/octave away from its own
    /// corner, not a gentle lean.
    #[test]
    fn each_band_rejects_what_belongs_to_the_others() {
        let mut x = Crossover3::new();
        x.prepare(FS, 200.0, 2_000.0);
        // Two octaves below the lower corner: the mid and high should be
        // ~48 dB down, and the low essentially untouched.
        let input = tone(16_384, 50.0);
        let (low, mid, high) = split(&mut x, &input);
        let settled = 8_192;
        let level = |b: &[f32]| 20.0 * (rms(b.get(settled..).unwrap_or(&[])).max(1e-9)).log10();

        let reference = 20.0 * rms(input.get(settled..).unwrap_or(&[])).log10();
        let level = |b: &[f32]| level(b) - reference;

        assert!(level(&low) > -0.6, "the low band lost its own signal");
        assert!(
            level(&mid) < -35.0,
            "the mid band leaked {:.1} dB",
            level(&mid)
        );
        assert!(
            level(&high) < -70.0,
            "the high band leaked {:.1} dB",
            level(&high)
        );
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact.
    #[test]
    fn processing_in_pieces_is_processing_whole() {
        let input = tone(256, 700.0);

        let mut whole = Crossover3::new();
        whole.prepare(FS, 200.0, 2_000.0);
        let (l1, m1, h1) = split(&mut whole, &input);

        let mut pieces = Crossover3::new();
        pieces.prepare(FS, 200.0, 2_000.0);
        let mut l2 = vec![0.0; 256];
        let mut m2 = vec![0.0; 256];
        let mut h2 = vec![0.0; 256];
        let (a, b) = input.split_at(100);
        let (la, lb) = l2.split_at_mut(100);
        let (ma, mb) = m2.split_at_mut(100);
        let (ha, hb) = h2.split_at_mut(100);
        pieces.process(a, la, ma, ha);
        pieces.process(b, lb, mb, hb);

        assert_eq!(l1, l2, "the low band differs across a block boundary");
        assert_eq!(m1, m2, "the mid band differs across a block boundary");
        assert_eq!(h1, h2, "the high band differs across a block boundary");
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut x = Crossover3::new();
        x.prepare(FS, 200.0, 2_000.0);
        let input = vec![0.1f32; 256];
        let mut low = vec![0.0f32; 256];
        let mut mid = vec![0.0f32; 256];
        let mut high = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                x.process(&input, &mut low, &mut mid, &mut high);
            }
        });
    }

    /// EDGE LENGTHS, and mismatched slices, which this kernel can be
    /// handed in a way a one-slice kernel cannot.
    #[test]
    fn odd_lengths_and_mismatched_slices_are_safe() {
        let mut x = Crossover3::new();
        x.prepare(FS, 200.0, 2_000.0);
        for len in [0usize, 1, 3, 37] {
            let input = tone(len, 440.0);
            let (low, mid, high) = split(&mut x, &input);
            for band in [&low, &mid, &high] {
                assert!(band.iter().all(|s| s.is_finite()), "len {len} went wild");
            }
        }
        // Short outputs: what fits is written, the rest is left alone.
        let input = tone(64, 440.0);
        let mut low = vec![9.0f32; 8];
        let mut mid = vec![0.0f32; 64];
        let mut high = vec![0.0f32; 64];
        x.process(&input, &mut low, &mut mid, &mut high);
        assert!(low.iter().all(|s| s.is_finite()));
        assert!(
            mid.get(8..)
                .is_some_and(|tail| tail.iter().all(|s| *s == 0.0)),
            "the shortest slice did not bound the run"
        );
    }

    /// SILENCE IN, SILENCE OUT — and a decaying tail stays finite rather
    /// than going NaN or ringing forever.
    #[test]
    fn silence_stays_silent_and_a_tail_decays() {
        let mut x = Crossover3::new();
        x.prepare(FS, 200.0, 2_000.0);
        let (low, mid, high) = split(&mut x, &vec![0.0f32; 512]);
        for band in [&low, &mid, &high] {
            assert!(band.iter().all(|s| *s == 0.0), "silence came out loud");
        }

        // An impulse, then nothing: the tail must fall away.
        let mut input = vec![0.0f32; 8_192];
        if let Some(first) = input.first_mut() {
            *first = 1.0;
        }
        let (low, mid, high) = split(&mut x, &input);
        for band in [&low, &mid, &high] {
            assert!(
                band.iter().all(|s| s.is_finite()),
                "the tail went non-finite"
            );
            let tail = peak(band.get(4_096..).unwrap_or(&[]));
            assert!(
                tail < 1e-4,
                "the tail is still at {tail} after 4096 samples"
            );
        }
    }

    /// NaN IN: passed through if it must be, never invented from finite
    /// input. The corners are ordered and separated whatever is asked
    /// for, so a crossed pair cannot make an empty band.
    #[test]
    fn nonsense_corners_are_ordered_not_obeyed() {
        let (low, high) = Crossover3::corners(FS, 5_000.0, 100.0);
        assert!(high >= low * MIN_SPREAD, "{low} and {high} are crossed");
        let (low, high) = Crossover3::corners(FS, f32::NAN, f32::INFINITY);
        assert!(low.is_finite() && high.is_finite() && high > low);

        let mut x = Crossover3::new();
        x.prepare(FS, f32::NAN, 0.0);
        let input = tone(512, 440.0);
        let (low, mid, high) = split(&mut x, &input);
        for band in [&low, &mid, &high] {
            assert!(
                band.iter().all(|s| s.is_finite()),
                "nonsense corners made NaN"
            );
        }
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

        let mut x = Crossover3::new();
        x.prepare(FS, 200.0, 3_000.0);
        let (mut lo, mut mid, mut hi) = (
            vec![0.0f32; BLOCK],
            vec![0.0f32; BLOCK],
            vec![0.0f32; BLOCK],
        );
        row("crossover3 (10 sections)", &mut || {
            x.process(&sig, &mut lo, &mut mid, &mut hi);
            std::hint::black_box((&mut lo, &mut mid, &mut hi));
        });
    }
}
