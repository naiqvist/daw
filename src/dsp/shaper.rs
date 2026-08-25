//! Family 8 of the kernel roadmap: nonlinearities.
//!
//! [`Waveshaper`] (hard/soft/cubic clip, wavefold, bitcrush — the same
//! five shapes `ui::device::shaper` draws, held to it by an agreement
//! test) and [`Oversampler2x`] (the wrapper the contract names: shape at
//! twice the rate so the harmonics a nonlinearity creates have somewhere
//! to exist before the decimation filter removes them, instead of
//! folding straight into the music).
//!
//! # What 2× honestly buys
//!
//! Oversampling does not abolish aliasing, it halves the problem: after
//! shaping at 96 k, everything the nonlinearity puts between 24 k and
//! 48 k is REAL and the decimator's stopband removes it — but harmonics
//! past 72 k still fold into the audible band. For musical drive the
//! improvement is large and MEASURED in the tests; for a screaming clip
//! on a high fundamental the honest fix is 4×, which is this same
//! wrapper applied twice. Crush is the exception: quantisation IS the
//! effect, and oversampling a bitcrusher just crushes a different
//! signal — the docs say so rather than letting a node discover it.
//!
//! # The wrapper's shape
//!
//! `up` zero-stuffs into a caller scratch slice (`scratch_len`) through
//! a 71-tap Kaiser-windowed halfband computed in f64 at prepare; the
//! node runs any nonlinearity over the 2× slice; `down` filters and
//! decimates back. Linear phase, so the price is pure delay:
//! `latency()` reports it and an impulse test pins it to the sample —
//! this and the lookahead limiter are why the contract makes every
//! kernel answer for its latency.

/// Full length of the halfband FIR. Odd; centre = (N−1)/2. Sized so the
/// Kaiser transition (≈ 8 kHz at 96 k) puts the stopband where content
/// would fold below ~20 kHz.
const HB_LEN: usize = 71;
/// The FIR history rings: next power of two above HB_LEN.
const RING: usize = 128;
/// Kaiser beta: ~90 dB stopband.
const KAISER_BETA: f64 = 9.0;

/// Drive limits, matching the display exactly (the agreement test
/// depends on the clamps agreeing too).
pub const DRIVE_MIN: f32 = 1.0;
pub const DRIVE_MAX: f32 = 32.0;
pub const BIAS_MAX: f32 = 0.9;
/// Crush resolution at minimum drive; divided by drive from there.
const CRUSH_STEPS: f32 = 48.0;

/// The five shapes — the same list the display draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    HardClip,
    SoftClip,
    Cubic,
    Fold,
    Crush,
}

/// Triangle fold: `(2/π)·asin(sin(πx/2))` — identity inside the rails,
/// reflected outside, no branches, no seams. The clamp is load-bearing:
/// `sin` can return 1.0000001 and `asin` of that is NaN.
#[inline(always)]
fn triangle_fold(x: f32) -> f32 {
    use core::f32::consts::PI;
    let s = (x * PI * 0.5).sin().clamp(-1.0, 1.0);
    (2.0 / PI) * s.asin()
}

/// The waveshaper: five nonlinearities behind one face.
///
/// Stateless — a transfer curve has no memory — so there is nothing to
/// reset and split-block equivalence is trivial rather than tested by
/// luck. Drive is input gain everywhere except Crush, where it is
/// RESOLUTION (a crusher driven harder has fewer levels, not more
/// volume); bias breaks the odd symmetry, which is where even harmonics
/// come from; mix at 0 is the identity to the bit.
///
/// State: 16 bytes of settings, no signal state. Per-sample cost: a few
/// flops for the clips; Fold pays a sin+asin, Crush a round.
/// Denormal-safe: pure arithmetic on finite settings; output clamped to
/// the rails.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Waveshaper {
    mode: Mode,
    drive: f32,
    bias: f32,
    mix: f32,
}

impl Default for Waveshaper {
    fn default() -> Self {
        Self::new()
    }
}

impl Waveshaper {
    pub fn new() -> Self {
        Self {
            mode: Mode::SoftClip,
            drive: DRIVE_MIN,
            bias: 0.0,
            mix: 1.0,
        }
    }

    /// Green zone: the whole rule in one call; nonsense clamps.
    pub fn configure(&mut self, mode: Mode, drive: f32, bias: f32, mix: f32) {
        self.mode = mode;
        self.drive = if drive.is_finite() {
            drive.clamp(DRIVE_MIN, DRIVE_MAX)
        } else {
            DRIVE_MIN
        };
        self.bias = if bias.is_finite() {
            bias.clamp(-BIAS_MAX, BIAS_MAX)
        } else {
            0.0
        };
        self.mix = if mix.is_finite() {
            mix.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    /// One sample through the curve. Identical arithmetic to the
    /// display's `shape` — the agreement test holds the two together.
    #[inline(always)]
    pub fn shape(&self, x: f32) -> f32 {
        if !x.is_finite() {
            return 0.0;
        }
        let wet = match self.mode {
            Mode::Crush => {
                let steps = (CRUSH_STEPS / self.drive).round().max(2.0);
                let t = (x + self.bias).clamp(-1.0, 1.0);
                (t * steps).round() / steps
            }
            mode => {
                let driven = x * self.drive + self.bias;
                match mode {
                    Mode::HardClip => driven.clamp(-1.0, 1.0),
                    Mode::SoftClip => driven.tanh(),
                    Mode::Cubic => {
                        let t = driven.clamp(-1.0, 1.0);
                        1.5 * t - 0.5 * t * t * t
                    }
                    Mode::Fold => triangle_fold(driven),
                    Mode::Crush => 0.0, // handled above
                }
            }
        };
        let y = x * (1.0 - self.mix) + wet * self.mix;
        if y.is_finite() {
            y.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }

    /// Red zone: shape in place, any length.
    pub fn process(&self, io: &mut [f32]) {
        for s in io.iter_mut() {
            *s = self.shape(*s);
        }
    }
}

// --------------------------------------------------------- oversampler ---

/// Zeroth-order modified Bessel I0, for the Kaiser window. Green zone,
/// f64, converges in a couple of dozen terms.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let half = x * 0.5;
    for k in 1..=30 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < 1e-18 * sum {
            break;
        }
    }
    sum
}

/// 2× oversampler: linear-phase halfband up/down, caller scratch for the
/// doubled-rate signal, honest latency.
///
/// State: two 128-float history rings plus 71 coefficients (~1.3 KB).
/// Per-sample cost, MEASURED: the full wrap around a hard clip is
/// 152 ns/sample (~0.7% of a core at 48 k) — three 71-tap convolutions
/// with the halfband zeros convolved for simplicity. The polyphase
/// halving is the named, profiling-triggered upgrade, per the
/// contract's optimise-after-measuring rule. Alias improvement at full
/// drive, measured: worst image -27.2 dB naive, -43.6 dB wrapped.
/// Denormal-safe: FIR state decays through FTZ on silence.
/// In-place safe: n/a — up and down have distinct in/out slices.
/// Latency: [`Self::latency`] samples at the ORIGINAL rate (linear
/// phase: the two filter group delays sum to exactly the centre tap).
#[derive(Debug, Clone, Copy)]
pub struct Oversampler2x {
    coeffs: [f32; HB_LEN],
    up_hist: [f32; RING],
    down_hist: [f32; RING],
    up_pos: usize,
    down_pos: usize,
}

impl Default for Oversampler2x {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2x {
    pub fn new() -> Self {
        let mut o = Self {
            coeffs: [0.0; HB_LEN],
            up_hist: [0.0; RING],
            down_hist: [0.0; RING],
            up_pos: 0,
            down_pos: 0,
        };
        o.prepare();
        o
    }

    /// Floats of scratch `up` needs for a block: twice the block.
    pub fn scratch_len(block: usize) -> usize {
        block * 2
    }

    /// Green zone: build the halfband (Kaiser-windowed sinc at a quarter
    /// of the doubled rate), normalised in f64 so DC through the whole
    /// round trip is exactly unity.
    pub fn prepare(&mut self) {
        let c = (HB_LEN - 1) as f64 / 2.0;
        let denom = bessel_i0(KAISER_BETA);
        let mut sum = 0.0f64;
        let mut h = [0.0f64; HB_LEN];
        for (i, tap) in h.iter_mut().enumerate() {
            let n = i as f64 - c;
            let sinc = if n == 0.0 {
                0.5
            } else {
                (core::f64::consts::PI * n * 0.5).sin() / (core::f64::consts::PI * n)
            };
            let r = 2.0 * (i as f64) / (HB_LEN - 1) as f64 - 1.0;
            let win = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / denom;
            *tap = sinc * win;
            sum += *tap;
        }
        for (dst, src) in self.coeffs.iter_mut().zip(h.iter()) {
            *dst = (*src / sum) as f32;
        }
        self.reset();
    }

    /// Green zone: forget the signal history.
    pub fn reset(&mut self) {
        self.up_hist = [0.0; RING];
        self.down_hist = [0.0; RING];
        self.up_pos = 0;
        self.down_pos = 0;
    }

    /// The round-trip delay at the ORIGINAL rate. Report to PDC.
    pub fn latency(&self) -> usize {
        (HB_LEN - 1) / 2
    }

    #[inline(always)]
    fn fir(hist: &[f32; RING], pos: usize, coeffs: &[f32; HB_LEN]) -> f32 {
        let mut acc = 0.0f32;
        for (k, c) in coeffs.iter().enumerate() {
            acc += c * hist[pos.wrapping_sub(k) & (RING - 1)];
        }
        acc
    }

    /// Red zone: upsample `input` into `out2x`, which must be exactly
    /// twice as long (the `scratch_len` slice). Wrong sizes fail open —
    /// nothing written.
    pub fn up(&mut self, input: &[f32], out2x: &mut [f32]) {
        if out2x.len() != input.len() * 2 {
            return;
        }
        for (x, pair) in input.iter().zip(out2x.as_chunks_mut::<2>().0) {
            // Zero-stuff, filter, ×2 to preserve amplitude.
            for (slot, sample) in pair.iter_mut().zip([*x, 0.0]) {
                self.up_pos = self.up_pos.wrapping_add(1);
                self.up_hist[self.up_pos & (RING - 1)] = sample;
                *slot = 2.0 * Self::fir(&self.up_hist, self.up_pos, &self.coeffs);
            }
        }
    }

    /// Red zone: filter and decimate `in2x` back into `out`, which must
    /// be exactly half as long. Wrong sizes fail open.
    ///
    /// The output is computed on the FIRST slot of each pair, not the
    /// second — the decimation phase. The up/down cascade's group delay
    /// is an even number of 2× slots, so the kept phase must be the even
    /// one: sampling the odd phase reads every peak half a sample off
    /// (an impulse came back at 0.63 instead of ~0.95, which is how the
    /// impulse test caught it).
    pub fn down(&mut self, in2x: &[f32], out: &mut [f32]) {
        if in2x.len() != out.len() * 2 {
            return;
        }
        for (pair, y) in in2x.as_chunks::<2>().0.iter().zip(out.iter_mut()) {
            self.down_pos = self.down_pos.wrapping_add(1);
            self.down_hist[self.down_pos & (RING - 1)] = pair[0];
            *y = Self::fir(&self.down_hist, self.down_pos, &self.coeffs);
            self.down_pos = self.down_pos.wrapping_add(1);
            self.down_hist[self.down_pos & (RING - 1)] = pair[1];
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn tone_amp(x: &[f32], hz: f32, fs: f32) -> f64 {
        let n = x.len();
        let (a0, a1, a2, a3) = (0.35875f64, 0.48829, 0.14128, 0.01168);
        let w = core::f64::consts::TAU * (hz / fs) as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let mut norm = 0.0f64;
        for (i, &v) in x.iter().enumerate() {
            let t = core::f64::consts::TAU * i as f64 / n as f64;
            let win = a0 - a1 * t.cos() + a2 * (2.0 * t).cos() - a3 * (3.0 * t).cos();
            norm += win;
            let s0 = v as f64 * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / norm
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    // -------------------------------------------------------- reference ---

    /// THE agreement test: the kernel's curve and the widget's curve are
    /// the same curve, across every mode, drive, bias and mix probed.
    #[test]
    fn the_kernel_agrees_with_what_the_ui_draws() {
        use crate::ui::device::shaper as ui;
        let pairs = [
            (Mode::HardClip, ui::Mode::HardClip),
            (Mode::SoftClip, ui::Mode::SoftClip),
            (Mode::Cubic, ui::Mode::Cubic),
            (Mode::Fold, ui::Mode::Fold),
            (Mode::Crush, ui::Mode::Crush),
        ];
        for (mode, ui_mode) in pairs {
            for (drive, bias, mix) in [
                (1.0f32, 0.0f32, 1.0f32),
                (4.0, 0.0, 1.0),
                (12.0, 0.35, 1.0),
                (32.0, -0.9, 0.5),
                (6.0, 0.2, 0.0),
            ] {
                let mut k = Waveshaper::new();
                k.configure(mode, drive, bias, mix);
                let w = ui::Shaper {
                    mode: ui_mode,
                    drive,
                    bias,
                    mix,
                };
                for i in 0..=400 {
                    let x = -1.0 + i as f32 / 200.0;
                    let (a, b) = (k.shape(x), w.shape(x));
                    assert!(
                        (a - b).abs() < 1e-6,
                        "{mode:?} d{drive} b{bias} m{mix} at {x}: kernel {a}, widget {b}"
                    );
                }
            }
        }
    }

    /// The oversampler alone is transparent: flat within a tenth of a dB
    /// through the audible band, and an impulse lands at exactly
    /// latency() — the linear-phase promise, verified to the sample.
    #[test]
    fn the_oversampler_is_transparent_and_its_latency_is_exact() {
        let os = Oversampler2x::new();
        let latency = os.latency();
        let n = 8_192;
        let mut up = vec![0.0f32; Oversampler2x::scratch_len(n)];
        for hz in [1_000.0f32, 10_000.0, 18_000.0] {
            let hz = (hz * n as f32 / FS).round() * FS / n as f32;
            let sig: Vec<f32> = (0..n)
                .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
                .collect();
            let mut os = Oversampler2x::new();
            os.up(&sig, &mut up);
            let mut out = vec![0.0f32; n];
            os.down(&up, &mut out);
            let g = db(tone_amp(&out[n / 4..], hz, FS));
            assert!(g.abs() < 0.1, "{hz} Hz through up/down: {g:.3} dB");
        }

        let mut os = Oversampler2x::new();
        let mut sig = vec![0.0f32; 256];
        sig[0] = 1.0;
        let mut up2 = vec![0.0f32; 512];
        os.up(&sig, &mut up2);
        let mut out = vec![0.0f32; 256];
        os.down(&up2, &mut out);
        let peak_at = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak_at, latency, "impulse must land at latency()");
        assert!(out[peak_at] > 0.9, "and mostly intact: {}", out[peak_at]);
    }

    /// What oversampling is FOR, measured: hard-clip a sine and compare
    /// the worst alias image naive vs oversampled. The improvement must
    /// be large; the exact figures live in the assertion so a regression
    /// in either path (shaper, filters, alignment) shows as a number.
    #[test]
    fn oversampled_clipping_aliases_far_less_than_naive() {
        // An awkward fundamental whose alias images land well away from
        // the true harmonics. The first draft used 1499 Hz — 48000/1499
        // is 32.02, so every image fell 32 Hz from a harmonic and the
        // skip-near-harmonics guard discarded exactly the aliases the
        // test was hunting, measuring the noise floor instead.
        let f0 = 1_234.5f32;
        let n = 1 << 14;
        let sig: Vec<f32> = (0..n)
            .map(|i| 0.9 * (i as f32 / FS * f0 * core::f32::consts::TAU).sin())
            .collect();
        let mut shaper = Waveshaper::new();
        // FULL drive. At drive 8 the clip's transition regions are wide
        // enough that a sinc envelope attenuates the very harmonics that
        // would alias — the first draft asserted "audible aliasing" and
        // measured -43 dB, which was the physics being right and the
        // expectation being calibrated to an ideal square. Drive 32
        // makes the transitions genuinely hard.
        shaper.configure(Mode::HardClip, 32.0, 0.0, 1.0);

        // Naive: shape at 48 k.
        let mut naive = sig.clone();
        shaper.process(&mut naive);

        // Oversampled: up, shape at 96 k, down.
        let mut os = Oversampler2x::new();
        let mut up = vec![0.0f32; Oversampler2x::scratch_len(n)];
        os.up(&sig, &mut up);
        shaper.process(&mut up);
        let mut over = vec![0.0f32; n];
        os.down(&up, &mut over);

        // Probe every fold-back image of the odd harmonics, skipping
        // anything near a true harmonic.
        let worst = |buf: &[f32]| {
            let fund = tone_amp(buf, f0, FS);
            let mut worst = f64::NEG_INFINITY;
            let kmax = (FS * 0.5 / f0) as usize;
            for k in (kmax + 1)..=(kmax * 4) {
                if k % 2 == 0 {
                    continue; // odd-symmetric clip: even harmonics absent
                }
                let f = k as f32 * f0;
                // Fold down into 0..Nyquist.
                let m = f % FS;
                let folded = if m > FS * 0.5 { FS - m } else { m };
                if folded < 300.0 || folded > FS * 0.5 - 300.0 {
                    continue;
                }
                if (1..=kmax).any(|h| (folded - h as f32 * f0).abs() < 60.0) {
                    continue;
                }
                let rel = db(tone_amp(buf, folded, FS) / fund);
                if rel > worst {
                    worst = rel;
                }
            }
            worst
        };
        let naive_worst = worst(&naive);
        let over_worst = worst(&over);
        assert!(
            naive_worst > -33.0,
            "naive full-drive clipping should alias measurably (got {naive_worst:.1} dB) — \
             if not, the probe is broken"
        );
        assert!(
            over_worst < naive_worst - 10.0,
            "oversampling must buy at least 10 dB: naive {naive_worst:.1}, over {over_worst:.1}"
        );
        println!("alias worst: naive {naive_worst:.1} dB, oversampled {over_worst:.1} dB");
    }

    /// Split-block bit-exactness through the FIR state, with the shaping
    /// in the middle — the whole up/shape/down chain, cut at an odd
    /// place.
    #[test]
    fn split_block_is_bit_exact() {
        let input: Vec<f32> = (0..256).map(|i| 0.8 * ((i as f32) * 0.11).sin()).collect();
        let mut shaper = Waveshaper::new();
        shaper.configure(Mode::SoftClip, 6.0, 0.1, 1.0);

        let run = |cuts: &[usize]| {
            let mut os = Oversampler2x::new();
            let mut out = input.clone();
            let mut at = 0;
            let ends: Vec<usize> = cuts.iter().copied().chain([input.len()]).collect();
            for &end in &ends {
                let chunk = &mut out[at..end];
                let mut up = vec![0.0f32; chunk.len() * 2];
                os.up(chunk, &mut up);
                shaper.process(&mut up);
                os.down(&up, chunk);
                at = end;
            }
            out
        };
        let whole = run(&[]);
        let split = run(&[100]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "256 must equal 100 + 156 through the whole chain"
        );
    }

    // ------------------------------------------------------------ no-alloc ---

    #[test]
    fn process_does_not_allocate() {
        let mut shaper = Waveshaper::new();
        shaper.configure(Mode::Fold, 8.0, 0.2, 1.0);
        let mut os = Oversampler2x::new();
        let mut io = vec![0.5f32; 256];
        let mut up = vec![0.0f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..50 {
                os.up(&io, &mut up);
                shaper.process(&mut up);
                os.down(&up, &mut io);
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let mut shaper = Waveshaper::new();
        shaper.configure(Mode::Cubic, 4.0, 0.0, 1.0);
        let mut os = Oversampler2x::new();
        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut io = vec![0.4f32; len];
            let mut up = vec![0.0f32; len * 2];
            os.up(&io, &mut up);
            shaper.process(&mut up);
            os.down(&up, &mut io);
            assert!(io.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    // ------------------------------------------------ silence and nonsense ---

    /// Silence in, silence out through the whole chain; nonsense clamps;
    /// wrong-sized scratch fails open (nothing written).
    #[test]
    fn silence_and_nonsense_behave() {
        let mut shaper = Waveshaper::new();
        shaper.configure(Mode::SoftClip, 8.0, 0.0, 1.0);
        let mut os = Oversampler2x::new();
        let mut io = vec![0.0f32; 128];
        let mut up = vec![0.0f32; 256];
        os.up(&io, &mut up);
        shaper.process(&mut up);
        os.down(&up, &mut io);
        assert!(io.iter().all(|s| *s == 0.0), "silence in, silence out");

        let mut k = Waveshaper::new();
        k.configure(Mode::Fold, f32::NAN, f32::INFINITY, f32::NAN);
        for x in [f32::NAN, f32::INFINITY, -1e9, 0.5] {
            let y = k.shape(x);
            assert!(y.is_finite() && (-1.0..=1.0).contains(&y), "{x} gave {y}");
        }

        // Wrong-sized scratch: fail open, nothing written.
        let sig = vec![0.5f32; 64];
        let mut wrong = vec![9.0f32; 100];
        os.up(&sig, &mut wrong);
        assert!(
            wrong.iter().all(|s| *s == 9.0),
            "up must not touch a bad slice"
        );
        let mut out = vec![9.0f32; 64];
        os.down(&wrong[..100], &mut out);
        assert!(
            out.iter().all(|s| *s == 9.0),
            "down must not touch a bad slice"
        );
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 10_000;

        let row = |name: &str, run: &mut dyn FnMut()| {
            for _ in 0..500 {
                run();
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run();
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<26} {ns:6.2} ns/sample");
        };

        let mut hard = Waveshaper::new();
        hard.configure(Mode::HardClip, 8.0, 0.0, 1.0);
        let mut io = vec![0.5f32; BLOCK];
        row("hard clip (naive)", &mut || {
            hard.process(&mut io);
            std::hint::black_box(&mut io);
        });
        let mut fold = Waveshaper::new();
        fold.configure(Mode::Fold, 8.0, 0.0, 1.0);
        row("fold (naive)", &mut || {
            fold.process(&mut io);
            std::hint::black_box(&mut io);
        });
        let mut os = Oversampler2x::new();
        let mut up = vec![0.0f32; BLOCK * 2];
        row("2x wrap + hard clip", &mut || {
            os.up(&io, &mut up);
            hard.process(&mut up);
            os.down(&up, &mut io);
            std::hint::black_box(&mut io);
        });
    }
}
