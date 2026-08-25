//! Family 7 of the kernel roadmap: oscillators.
//!
//! [`MipOsc`]: the audio-grade oscillator — mip-mapped band-limited
//! wavetables, the structure every modern soft synth uses. Sine,
//! triangle, saw and square from one kernel.
//!
//! # How the quality happens
//!
//! A naive saw sprays its harmonics past Nyquist and they fold back as
//! inharmonic fizz. Here every table LEVEL is built (in f64, in the green
//! zone) containing only the harmonics that fit under Nyquist for the
//! octave that level serves — so aliasing is excluded by construction,
//! not filtered after the fact. Ten levels cover the audio band; playback
//! picks the level from the current frequency and CROSSFADES toward the
//! next as a note approaches a band edge, so sweeps never click across a
//! boundary.
//!
//! Reads are 4-point Hermite (Catmull–Rom), not linear. This is not
//! decoration: with content up to a quarter of the table rate, linear
//! interpolation's sinc² image rejection leaves the top harmonics' images
//! near −75 dB — audible on a bright saw. Hermite's sharper rolloff puts
//! them below the −90 dB floor the tests demand.
//!
//! Phase is a **u32 fixed-point accumulator**: exact wraparound, uniform
//! resolution, zero drift, and cheaper than float phase. Frequency error
//! from increment quantisation is ≤ fs/2³³ — under six micro-hertz at
//! 48 kHz, thousands of times below a cent.
//!
//! # Tables are sample-rate independent
//!
//! The harmonic count per level halves per octave, so the fs cancels out
//! of the table CONTENT entirely — only level *selection* needs the rate.
//! One table set serves 44.1 k and 96 k alike, and every voice shares it
//! read-only: eight voices, one 80 KB set.
//!
//! Caller-owned storage, per the contract and the [`Reverb`] precedent:
//! size from [`table_len`], build once with [`build_tables`], pass
//! `&[f32]` to `process`.
//!
//! [`Reverb`]: crate::dsp::reverb::Reverb

/// Samples per mip level. 2048 holds 512 harmonics at a quarter of the
/// table rate — the headroom Hermite interpolation needs to keep images
/// below the floor.
pub const TABLE_LEN: usize = 2048;
/// Mip levels for the banded waveforms. Ten octaves: at 48 kHz the bands
/// run from ~23 Hz to Nyquist, halving the harmonic count each step.
pub const LEVELS: usize = 10;
/// Top harmonic of level 0. `512 >> level` thereafter, floor 1.
const H0: usize = 512;
/// The top fraction of each band that crossfades into the next level.
const XFADE_FRAC: f32 = 0.35;
/// Bits of the phase word below the table index: 32 − log2(TABLE_LEN).
const FRAC_BITS: u32 = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    /// One harmonic; nothing to band-limit, so one level serves all.
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Waveform {
    /// How many mip levels this waveform's table set holds.
    pub fn levels(self) -> usize {
        match self {
            Self::Sine => 1,
            _ => LEVELS,
        }
    }
}

/// Floats a waveform's table set needs. Green zone; size once, at
/// compile/prepare time.
pub fn table_len(waveform: Waveform) -> usize {
    waveform.levels() * TABLE_LEN
}

/// Build a waveform's band-limited tables into caller-owned storage.
///
/// Green zone, f64 accumulation. Levels are built coarsest-first and each
/// finer level REUSES the running sum, so the whole set costs one pass of
/// `H0 × TABLE_LEN` sine evaluations, not one per level.
///
/// A short slice builds nothing — the mismatch is caught again at
/// `process`, which goes silent rather than reading garbage.
pub fn build_tables(waveform: Waveform, tables: &mut [f32]) {
    let levels = waveform.levels();
    if tables.len() < levels * TABLE_LEN {
        return;
    }

    // Fourier amplitude of harmonic n, or 0.0 where the series skips it.
    // Consistent scaling across levels — no per-level normalisation, or
    // the mip crossfade would breathe in loudness as well as brightness.
    let amp = |n: usize| -> f64 {
        use core::f64::consts::PI;
        let nf = n as f64;
        match waveform {
            Waveform::Sine => {
                if n == 1 {
                    1.0
                } else {
                    0.0
                }
            }
            Waveform::Saw => 2.0 / PI / nf,
            Waveform::Square => {
                if n % 2 == 1 {
                    4.0 / PI / nf
                } else {
                    0.0
                }
            }
            Waveform::Triangle => {
                if n % 2 == 1 {
                    let sign = if (n / 2).is_multiple_of(2) { 1.0 } else { -1.0 };
                    sign * 8.0 / (PI * PI) / (nf * nf)
                } else {
                    0.0
                }
            }
        }
    };

    let mut acc = [0.0f64; TABLE_LEN];
    let mut built_to = 0usize; // highest harmonic currently in `acc`

    // Coarsest level first (fewest harmonics), adding only the harmonics
    // each finer level introduces.
    for level in (0..levels).rev() {
        let target = if levels == 1 { 1 } else { (H0 >> level).max(1) };
        for n in (built_to + 1)..=target {
            let a = amp(n);
            if a != 0.0 {
                for (i, slot) in acc.iter_mut().enumerate() {
                    let x = i as f64 / TABLE_LEN as f64;
                    *slot += a * (core::f64::consts::TAU * n as f64 * x).sin();
                }
            }
        }
        built_to = target;
        let dst = &mut tables[level * TABLE_LEN..(level + 1) * TABLE_LEN];
        for (d, s) in dst.iter_mut().zip(acc.iter()) {
            *d = *s as f32;
        }
    }
}

/// The mip-mapped wavetable oscillator.
///
/// State: 20 bytes. Per-sample cost: one Hermite read (4 taps, ~10 flops)
/// on the fast path, two plus a blend while crossfading near a band edge.
/// Denormal-safe: emits table values scaled by finite weights; never
/// invents NaN from finite settings — nonsense frequencies clamp.
/// In-place safe: n/a — output-only (a generator fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct MipOsc {
    phase: u32,
    inc: u32,
    sample_rate: f32,
    levels: usize,
    lo: usize,
    /// Weight of the NEXT (duller) level, `0..=1`.
    xfade: f32,
}

impl Default for MipOsc {
    fn default() -> Self {
        Self::new()
    }
}

impl MipOsc {
    pub fn new() -> Self {
        Self {
            phase: 0,
            inc: 0,
            sample_rate: 48_000.0,
            levels: LEVELS,
            lo: 0,
            xfade: 0.0,
        }
    }

    /// Green zone: sample rate and waveform (which fixes the table
    /// layout this oscillator expects).
    pub fn prepare(&mut self, sample_rate: f32, waveform: Waveform) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.levels = waveform.levels();
        self.set_freq(0.0);
    }

    /// Set the fundamental, in Hz. Cheap enough to call per block for
    /// glides and vibrato; nonsense clamps rather than poisoning state.
    ///
    /// Level selection: band `l` covers fundamentals in
    /// `[fs/2048·2^l, fs/2048·2^(l+1))`, whose level-0 top holds exactly
    /// [`H0`] harmonics under Nyquist. The top [`XFADE_FRAC`] of each band
    /// blends toward the next level so a sweep crosses band edges as a
    /// gradual dulling, never a click.
    pub fn set_freq(&mut self, hz: f32) {
        let fs = self.sample_rate;
        let hz = if hz.is_finite() {
            hz.clamp(0.0, fs * 0.45)
        } else {
            0.0
        };
        self.inc = ((hz as f64 / fs as f64) * (1u64 << 32) as f64) as u32;

        if self.levels <= 1 || hz <= 0.0 {
            self.lo = 0;
            self.xfade = 0.0;
            return;
        }
        // Position across the bands, in octaves above band 0's bottom.
        let pos = (hz * TABLE_LEN as f32 / fs).max(f32::MIN_POSITIVE).log2();
        let clamped = pos.clamp(0.0, (self.levels - 1) as f32 + 0.999);
        let level = clamped as usize; // floor of a non-negative float
        let frac = clamped - level as f32;
        self.lo = level.min(self.levels - 1);
        self.xfade = if self.lo + 1 < self.levels {
            ((frac - (1.0 - XFADE_FRAC)) / XFADE_FRAC).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Green zone: return phase to the cycle start (the click-free moment
    /// to do it is the caller's business — that is what `Fade` is for).
    pub fn reset(&mut self) {
        self.phase = 0;
    }

    /// One Hermite (Catmull–Rom) read from a single level.
    #[inline(always)]
    fn read(table: &[f32], phase: u32) -> f32 {
        let mask = TABLE_LEN - 1;
        let i = (phase >> FRAC_BITS) as usize;
        let t = (phase & ((1 << FRAC_BITS) - 1)) as f32 * (1.0 / (1u32 << FRAC_BITS) as f32);
        // Masked neighbours: always in bounds by construction.
        let p0 = table[i.wrapping_sub(1) & mask];
        let p1 = table[i & mask];
        let p2 = table[(i + 1) & mask];
        let p3 = table[(i + 2) & mask];
        let a = 0.5 * (p2 - p0);
        let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
        let c = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
        p1 + t * (a + t * (b + t * c))
    }

    /// Red zone: fill `out` with the oscillator, any length.
    ///
    /// `tables` is the set [`build_tables`] filled for the SAME waveform
    /// `prepare` was given; a wrong-sized slice writes silence rather
    /// than reading garbage — loud enough to notice, safe enough to ship.
    pub fn process(&mut self, out: &mut [f32], tables: &[f32]) {
        let lo_start = self.lo * TABLE_LEN;
        let hi_level = (self.lo + 1).min(self.levels.saturating_sub(1));
        let hi_start = hi_level * TABLE_LEN;
        let (Some(t_lo), Some(t_hi)) = (
            tables.get(lo_start..lo_start + TABLE_LEN),
            tables.get(hi_start..hi_start + TABLE_LEN),
        ) else {
            for s in out.iter_mut() {
                *s = 0.0;
            }
            return;
        };

        // The branch is hoisted: the crossfading loop only runs when a
        // note actually sits in a band's blend region.
        if self.xfade <= 0.0 || core::ptr::eq(t_lo, t_hi) {
            for s in out.iter_mut() {
                *s = Self::read(t_lo, self.phase);
                self.phase = self.phase.wrapping_add(self.inc);
            }
        } else {
            let x = self.xfade;
            for s in out.iter_mut() {
                let a = Self::read(t_lo, self.phase);
                let b = Self::read(t_hi, self.phase);
                *s = a + (b - a) * x;
                self.phase = self.phase.wrapping_add(self.inc);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    /// Tables built once per waveform for the whole test run. A build is
    /// a million f64 sines — cheap once, dominant if every test repeats
    /// it in a debug binary.
    fn built(waveform: Waveform) -> &'static [f32] {
        use std::sync::OnceLock;
        static CACHE: [OnceLock<Vec<f32>>; 4] = [
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
        ];
        let slot = match waveform {
            Waveform::Sine => 0,
            Waveform::Triangle => 1,
            Waveform::Saw => 2,
            Waveform::Square => 3,
        };
        CACHE[slot].get_or_init(|| {
            let mut t = vec![0.0f32; table_len(waveform)];
            build_tables(waveform, &mut t);
            t
        })
    }

    fn rendered(waveform: Waveform, hz: f32, n: usize) -> Vec<f32> {
        let tables = built(waveform);
        let mut osc = MipOsc::new();
        osc.prepare(FS, waveform);
        osc.set_freq(hz);
        let mut out = vec![0.0f32; n];
        osc.process(&mut out, tables);
        out
    }

    /// Windowed Goertzel amplitude at one frequency.
    ///
    /// Blackman–Harris, because the measurements below hunt −90 dB
    /// components a few hundred Hz from 0 dB ones: a rectangular window's
    /// −13 dB sidelobes would bury exactly the thing being measured.
    fn tone_amp(x: &[f32], hz: f32) -> f64 {
        let n = x.len();
        let (a0, a1, a2, a3) = (0.35875f64, 0.48829, 0.14128, 0.01168);
        let w = core::f64::consts::TAU * (hz / FS) as f64;
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
        let power = (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0);
        2.0 * power.sqrt() / norm
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    // -------------------------------------------------------- reference ---

    /// A saw's harmonics measure 1/n, a square skips the even ones, a
    /// triangle falls at 1/n² — the spectra that make the waveforms what
    /// they are, measured off the oscillator actually running.
    #[test]
    fn each_waveform_has_its_textbook_spectrum() {
        let f0 = 110.0;
        let n = 1 << 16;

        let saw = rendered(Waveform::Saw, f0, n);
        let fund = tone_amp(&saw, f0);
        for h in 2..=10 {
            let got = db(tone_amp(&saw, f0 * h as f32) / fund);
            let want = db(1.0 / h as f64);
            assert!(
                (got - want).abs() < 0.5,
                "saw harmonic {h}: {got:.2} dB, textbook {want:.2}"
            );
        }

        let square = rendered(Waveform::Square, f0, n);
        let fund = tone_amp(&square, f0);
        for h in [3usize, 5, 7, 9] {
            let got = db(tone_amp(&square, f0 * h as f32) / fund);
            let want = db(1.0 / h as f64);
            assert!((got - want).abs() < 0.5, "square harmonic {h}: {got:.2} dB");
        }
        for h in [2usize, 4, 6] {
            let got = db(tone_amp(&square, f0 * h as f32) / fund);
            assert!(got < -60.0, "square even harmonic {h} at {got:.1} dB");
        }

        let tri = rendered(Waveform::Triangle, f0, n);
        let fund = tone_amp(&tri, f0);
        for h in [3usize, 5, 7] {
            let got = db(tone_amp(&tri, f0 * h as f32) / fund);
            let want = db(1.0 / (h * h) as f64);
            assert!((got - want).abs() < 0.5, "tri harmonic {h}: {got:.2} dB");
        }
    }

    /// THE quality bar: every folded image of every out-of-band harmonic
    /// sits below −90 dB relative to the fundamental. This is the number
    /// "band-limited" has to mean, measured — including near a band top,
    /// where the mip scheme is weakest.
    #[test]
    fn aliasing_stays_below_minus_ninety_db() {
        let n = 1 << 16;
        // Awkward fundamentals on purpose: mid-band, near a band top
        // (fs/2048·2^6 = 1500 Hz at 48 k), and high.
        for f0 in [1234.5f32, 1499.0, 3456.7, 7071.0] {
            let saw = rendered(Waveform::Saw, f0, n);
            let fund = tone_amp(&saw, f0);
            let kmax = (FS * 0.5 / f0) as usize;
            for k in (kmax + 1)..=(kmax * 2) {
                let image = FS - k as f32 * f0;
                if image <= 300.0 || image >= FS * 0.5 - 300.0 {
                    continue;
                }
                // Skip probes sitting on a true harmonic — those measure
                // signal, not alias.
                let near_harmonic = (1..=kmax).any(|h| (image - h as f32 * f0).abs() < 60.0);
                if near_harmonic {
                    continue;
                }
                let rel = db(tone_amp(&saw, image) / fund);
                assert!(
                    rel < -90.0,
                    "saw at {f0} Hz: harmonic {k}'s image at {image:.0} Hz is {rel:.1} dB"
                );
            }
        }
    }

    /// The sine path's own distortion — table plus Hermite read — is
    /// below −90 dB per partial. What "computed against a clean table"
    /// buys.
    #[test]
    fn the_sine_is_clean() {
        let sine = rendered(Waveform::Sine, 997.0, 1 << 16);
        let fund = tone_amp(&sine, 997.0);
        assert!(db(fund) > -1.0, "unit amplitude");
        for h in 2..=6 {
            let rel = db(tone_amp(&sine, 997.0 * h as f32) / fund);
            assert!(rel < -90.0, "sine partial {h} at {rel:.1} dB");
        }
    }

    /// Pitch is exact. The u32 accumulator's increment error is under six
    /// micro-hertz; here the rendered signal is measured to a tenth of a
    /// cent by interpolated zero-crossing count over ten seconds.
    #[test]
    fn pitch_is_exact_over_ten_seconds() {
        let f0 = 220.0;
        let buf = rendered(Waveform::Sine, f0, (FS * 10.0) as usize);
        let mut first = None;
        let mut last = 0.0f64;
        let mut cycles = 0u32;
        for i in 1..buf.len() {
            if buf[i - 1] <= 0.0 && buf[i] > 0.0 {
                let frac = buf[i - 1] as f64 / (buf[i - 1] as f64 - buf[i] as f64);
                let at = (i - 1) as f64 + frac;
                if first.is_none() {
                    first = Some(at);
                } else {
                    cycles += 1;
                }
                last = at;
            }
        }
        let first = first.unwrap();
        let measured = cycles as f64 / ((last - first) / FS as f64);
        let cents = 1200.0 * (measured / f0 as f64).log2();
        assert!(
            cents.abs() < 0.1,
            "measured {measured:.4} Hz = {cents:+.3} cents off"
        );
    }

    /// Crossing a band edge changes loudness by nothing the ear can hold
    /// onto: RMS just below and just above every boundary agrees within
    /// half a dB. The mip crossfade's whole job.
    #[test]
    fn band_edges_do_not_step_in_loudness() {
        let rms = |hz: f32| {
            let b = rendered(Waveform::Saw, hz, 1 << 14);
            let tail = &b[1 << 12..];
            (tail.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / tail.len() as f64).sqrt()
        };
        for level in 3..8 {
            let edge = FS / TABLE_LEN as f32 * (1 << level) as f32 * 2.0;
            let below = db(rms(edge * 0.98));
            let above = db(rms(edge * 1.02));
            assert!(
                (below - above).abs() < 0.5,
                "band edge at {edge:.0} Hz steps {:.2} dB",
                below - above
            );
        }
    }

    /// Determinism: same settings, same output, bit for bit — and reset
    /// really does return to the very first sample.
    #[test]
    fn the_oscillator_is_deterministic() {
        let tables = built(Waveform::Saw);
        let run = || {
            let mut osc = MipOsc::new();
            osc.prepare(FS, Waveform::Saw);
            osc.set_freq(440.0);
            let mut out = vec![0.0f32; 512];
            osc.process(&mut out, tables);
            out
        };
        let (a, b) = (run(), run());
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));

        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Saw);
        osc.set_freq(440.0);
        let mut first = vec![0.0f32; 256];
        osc.process(&mut first, tables);
        osc.reset();
        let mut again = vec![0.0f32; 256];
        osc.process(&mut again, tables);
        assert!(
            first
                .iter()
                .zip(&again)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }

    // ------------------------------------------- split-block equivalence ---

    #[test]
    fn split_block_is_bit_exact() {
        let tables = built(Waveform::Saw);
        let mut a = MipOsc::new();
        a.prepare(FS, Waveform::Saw);
        a.set_freq(1_499.0); // in a crossfade region, so both loops run
        let mut whole = vec![0.0f32; 256];
        a.process(&mut whole, tables);

        let mut b = MipOsc::new();
        b.prepare(FS, Waveform::Saw);
        b.set_freq(1_499.0);
        let mut split = vec![0.0f32; 256];
        b.process(&mut split[..100], tables);
        b.process(&mut split[100..], tables);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "256 must equal 100 + 156"
        );
    }

    // ------------------------------------------------------------ no-alloc ---

    #[test]
    fn process_does_not_allocate() {
        let tables = built(Waveform::Square);
        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Square);
        osc.set_freq(440.0);
        let mut buf = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                osc.process(&mut buf, tables);
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let tables = built(Waveform::Triangle);
        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Triangle);
        osc.set_freq(440.0);
        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut buf = vec![0.0f32; len];
            osc.process(&mut buf, tables);
            assert!(buf.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    // ----------------------------------------------- nonsense and mismatch ---

    /// Nonsense settings clamp; a wrong-sized table slice goes silent
    /// rather than reading garbage; a zero frequency holds still.
    #[test]
    fn nonsense_never_invents_nan() {
        let tables = built(Waveform::Saw);
        for hz in [f32::NAN, f32::INFINITY, -440.0, 1e9, 0.0] {
            let mut osc = MipOsc::new();
            osc.prepare(FS, Waveform::Saw);
            osc.set_freq(hz);
            let mut buf = vec![0.0f32; 128];
            osc.process(&mut buf, tables);
            assert!(buf.iter().all(|s| s.is_finite()), "hz {hz}");
        }
        for (fs, hz) in [(0.0f32, 440.0f32), (-1.0, 440.0), (f32::NAN, 440.0)] {
            let mut osc = MipOsc::new();
            osc.prepare(fs, Waveform::Saw);
            osc.set_freq(hz);
            let mut buf = vec![0.0f32; 64];
            osc.process(&mut buf, tables);
            assert!(buf.iter().all(|s| s.is_finite()), "fs {fs}");
        }

        // A short slice is silence, not a panic and not garbage.
        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Saw);
        osc.set_freq(440.0);
        let mut buf = vec![0.9f32; 64];
        osc.process(&mut buf, &tables[..TABLE_LEN]);
        assert!(buf.iter().all(|s| *s == 0.0), "mismatched tables go silent");

        // Zero frequency holds a constant, finite value.
        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Saw);
        osc.set_freq(0.0);
        let mut buf = vec![0.0f32; 64];
        osc.process(&mut buf, tables);
        assert!(buf.windows(2).all(|w| w[0] == w[1]), "stopped means still");
    }

    /// One table set serves every sample rate: the content is
    /// fs-independent by construction, and the alias floor holds when the
    /// same tables are played at 44.1 k.
    #[test]
    fn one_table_set_serves_other_sample_rates() {
        let tables = built(Waveform::Saw);
        let fs = 44_100.0f32;
        let f0 = 1234.5f32;
        let mut osc = MipOsc::new();
        osc.prepare(fs, Waveform::Saw);
        osc.set_freq(f0);
        let mut buf = vec![0.0f32; 1 << 15];
        osc.process(&mut buf, tables);

        // A local Goertzel against the other rate.
        let amp = |hz: f32| {
            let n = buf.len();
            let (a0, a1, a2, a3) = (0.35875f64, 0.48829, 0.14128, 0.01168);
            let w = core::f64::consts::TAU * (hz / fs) as f64;
            let c = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            let mut norm = 0.0f64;
            for (i, &v) in buf.iter().enumerate() {
                let t = core::f64::consts::TAU * i as f64 / n as f64;
                let win = a0 - a1 * t.cos() + a2 * (2.0 * t).cos() - a3 * (3.0 * t).cos();
                norm += win;
                let s0 = v as f64 * win + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / norm
        };
        let fund = amp(f0);
        let kmax = (fs * 0.5 / f0) as usize;
        for k in (kmax + 1)..=(kmax + 6) {
            let image = fs - k as f32 * f0;
            if image <= 300.0 || image >= fs * 0.5 - 300.0 {
                continue;
            }
            if (1..=kmax).any(|h| (image - h as f32 * f0).abs() < 60.0) {
                continue;
            }
            let rel = db(amp(image) / fund);
            assert!(rel < -90.0, "44.1 k image at {image:.0} Hz: {rel:.1} dB");
        }
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted — a timing threshold in
    /// a test fails on a loaded box and teaches nothing.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let tables = built(Waveform::Saw);
        let mut buf = vec![0.0f32; BLOCK];

        let mut row = |name: &str, hz: f32| {
            let mut osc = MipOsc::new();
            osc.prepare(FS, Waveform::Saw);
            osc.set_freq(hz);
            for _ in 0..1_000 {
                osc.process(&mut buf, tables);
            }
            let t = Instant::now();
            for _ in 0..REPS {
                osc.process(&mut buf, tables);
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<26} {ns:6.2} ns/sample");
        };
        row("saw (single mip)", 1_000.0);
        row("saw (crossfading)", 1_499.0);
    }
}
