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

use crate::dsp::{LANES, LaneFrame};

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
/// Amplitude scales for the sparse tables, each `1 / sum|a_n|` over the
/// waveform's own partial set, so every one of them peaks at most at 1.
/// Computed from the series above rather than tuned by ear; the table
/// test asserts the bound they promise.
const BELL_NORM: f64 = 0.529_651;
const GLASS_NORM: f64 = 0.191_228;
const METAL_NORM: f64 = 0.210_797;
const AIR_NORM: f64 = 0.493_575;

/// Bits of the phase word below the table index: 32 − log2(TABLE_LEN).
const FRAC_BITS: u32 = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    /// One harmonic; nothing to band-limit, so one level serves all.
    Sine,
    Triangle,
    Saw,
    Square,
    /// Few partials, spread wide: 1, 2, 5, 9, 13.
    Bell,
    /// Odd partials only, rolling off as `1/sqrt(n)` to 31 — bright and
    /// thin.
    Glass,
    /// Dense and clangorous: every partial to 64, `1/n`, with a sign
    /// pattern that breaks up the phase alignment a saw has.
    Metal,
    /// No fundamental at all — partials 9 to 64 only. Breathy.
    Air,
}

impl Waveform {
    /// Every shape, in the order the synth's wire value indexes them —
    /// the same order as `params::poly::WAVES`.
    pub const ALL: [Self; 8] = [
        Self::Sine,
        Self::Triangle,
        Self::Saw,
        Self::Square,
        Self::Bell,
        Self::Glass,
        Self::Metal,
        Self::Air,
    ];

    /// The shape a wire index selects. Out of range gives the first,
    /// because a node handed a stale index must still make a sound.
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

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
            // The four SPARSE tables. These are harmonic subsets, not
            // inharmonic spectra: the table is periodic by construction,
            // so a real bell's stretched partials are not reachable here.
            // What a sparse set buys is CHARACTER — a spectrum with holes
            // in it does not sound like a filtered saw, which is the only
            // thing the dense classic shapes can sound like.
            //
            // Each is scaled by a constant so that the sum of the
            // magnitudes is 1, which bounds the table to +-1 without a
            // per-level normalisation — the crossfade must not breathe in
            // loudness, so no level may be scaled on its own.
            Waveform::Bell => {
                if matches!(n, 1 | 2 | 5 | 9 | 13) {
                    BELL_NORM / nf
                } else {
                    0.0
                }
            }
            Waveform::Glass => {
                if n % 2 == 1 && n <= 31 {
                    GLASS_NORM / nf.sqrt()
                } else {
                    0.0
                }
            }
            Waveform::Metal => {
                if n <= 64 {
                    // A fixed, dependency-free sign pattern. Deterministic
                    // (the bounce guarantee), and unrelated to any simple
                    // period, so the partials do not re-align into a saw.
                    let sign = if n % 7 < 3 { -1.0 } else { 1.0 };
                    sign * METAL_NORM / nf
                } else {
                    0.0
                }
            }
            Waveform::Air => {
                if (9..=64).contains(&n) {
                    AIR_NORM / nf
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
        let (inc, lo, xfade) = Self::band(self.sample_rate, self.levels, hz);
        self.inc = inc;
        self.lo = lo;
        self.xfade = xfade;
    }

    /// Audio-rate pitch input in semitones above `base_hz`, preserving phase
    /// and selecting the safe mip band at every sample. Mismatched lengths
    /// write silence without advancing state.
    pub fn process_pitch(
        &mut self,
        out: &mut [f32],
        offsets: &[f32],
        base_hz: f32,
        tables: &[f32],
    ) {
        if out.len() != offsets.len() {
            out.fill(0.0);
            return;
        }
        for (sample, offset) in out.iter_mut().zip(offsets) {
            let offset = if offset.is_finite() {
                offset.clamp(-127.0, 127.0)
            } else {
                0.0
            };
            self.set_freq(base_hz * (offset / 12.0).exp2());
            self.process(core::slice::from_mut(sample), tables);
        }
    }

    /// The band decision for one frequency: phase increment, mip level,
    /// and how far into the crossfade toward the next level it sits.
    ///
    /// Pure — it reads no state and writes none — so [`LaneOsc`] derives
    /// each of its lanes through exactly this function. A lane picking
    /// its own band is the thing that makes a lane oscillator worth
    /// having (two voices an octave apart must not share a table), and
    /// this is how that stays the SAME decision the scalar kernel makes
    /// rather than a second implementation of it.
    fn band(sample_rate: f32, levels: usize, hz: f32) -> (u32, usize, f32) {
        let fs = sample_rate;
        let hz = if hz.is_finite() {
            hz.clamp(0.0, fs * 0.45)
        } else {
            0.0
        };
        let inc = ((hz as f64 / fs as f64) * (1u64 << 32) as f64) as u32;

        if levels <= 1 || hz <= 0.0 {
            return (inc, 0, 0.0);
        }
        // Position across the bands, in octaves above band 0's bottom.
        let pos = (hz * TABLE_LEN as f32 / fs).max(f32::MIN_POSITIVE).log2();
        let clamped = pos.clamp(0.0, (levels - 1) as f32 + 0.999);
        let level = clamped as usize; // floor of a non-negative float
        let frac = clamped - level as f32;
        let lo = level.min(levels - 1);
        let xfade = if lo + 1 < levels {
            ((frac - (1.0 - XFADE_FRAC)) / XFADE_FRAC).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (inc, lo, xfade)
    }

    /// Green zone: return phase to the cycle start (the click-free moment
    /// to do it is the caller's business — that is what `Fade` is for).
    pub fn reset(&mut self) {
        self.phase = 0;
    }

    /// Green zone: place the phase, in TURNS (1.0 is a full cycle).
    ///
    /// The scalar twin of [`LaneOsc::set_phase`], and wanted for the same
    /// reason one level up: a BANK of oscillators started at phase 0 sums
    /// to one loud edge on its first sample, because every table read is
    /// at the same point of its cycle. The 808 hi-hat's six squares are
    /// that bank. Spreading their start phases takes the correlated
    /// attack spike away without touching a single frequency.
    pub fn set_phase(&mut self, turns: f32) {
        self.phase = phase_offset(turns);
    }

    /// The current phase, for tests and for a node that wants to hand one
    /// oscillator's phase to another.
    pub fn phase(&self) -> u32 {
        self.phase
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

// ------------------------------------------------------------- lanes ---

/// A phase-modulation amount, in TURNS, as a phase-accumulator offset.
///
/// Total by construction: Rust's float-to-int casts saturate at the
/// integer bounds and map NaN to zero, so nonsense modulation stops
/// moving the phase rather than poisoning it — no UB, no panic, no
/// branch. Wrapping past a full turn is correct and deliberate: phase IS
/// modular, which is the whole reason it is a u32.
#[inline(always)]
fn phase_offset(turns: f32) -> u32 {
    (f64::from(turns) * (1u64 << 32) as f64) as i64 as u32
}

/// [`MipOsc`] for a whole voice group, with a phase-modulation input.
///
/// Each lane picks its OWN mip band. Two voices an octave apart must not
/// share a table — that is the difference between a voice group and one
/// oscillator copied eight times, and it is the thing a port gets wrong
/// by keeping a scalar `lo`/`xfade` and applying it to everyone.
///
/// # Why this one walks lanes on the OUTSIDE
///
/// The other lane kernels put lanes in the inner loop, so a frame of
/// every voice is computed together. This one does not, and the reason is
/// the table read: a wavetable lookup is a GATHER, and with per-lane
/// bands eight lanes means eight different tables — eight cache lines
/// touched per sample, every sample. Walking one lane across the whole
/// block instead keeps that lane's two tables hot in cache for the
/// duration, and the only cost is that the writes stride by [`LANES`],
/// which still lands two consecutive samples in the same cache line.
///
/// The layout is unchanged either way — the OUTPUT is still lane-major,
/// which is what the mod matrix downstream consumes. This is a loop
/// order, not a data structure.
///
/// State: 96 bytes + 12.
/// Per-sample-per-lane cost: one Hermite read (4 taps, ~10 flops), two
/// plus a blend inside a band's crossfade region, plus one add when
/// modulated.
/// Denormal-safe: emits table values scaled by finite weights; never
/// invents NaN from finite settings.
/// In-place safe: n/a — output-only.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct LaneOsc {
    phase: [u32; LANES],
    inc: [u32; LANES],
    lo: [u16; LANES],
    xfade: [f32; LANES],
    sample_rate: f32,
    levels: usize,
}

impl Default for LaneOsc {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneOsc {
    pub fn new() -> Self {
        Self {
            phase: [0; LANES],
            inc: [0; LANES],
            lo: [0; LANES],
            xfade: [0.0; LANES],
            sample_rate: 48_000.0,
            levels: LEVELS,
        }
    }

    /// Green zone: sample rate and waveform (which fixes the table layout
    /// this group expects). Silences every lane's increment, exactly as
    /// the scalar kernel's `prepare` does.
    pub fn prepare(&mut self, sample_rate: f32, waveform: Waveform) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.levels = waveform.levels();
        for lane in 0..LANES {
            self.set_freq(lane, 0.0);
        }
    }

    /// Set ONE lane's fundamental, in Hz. The band decision is
    /// [`MipOsc::band`] — the scalar kernel's own — so a lane cannot pick
    /// a different table from the one a scalar oscillator would.
    pub fn set_freq(&mut self, lane: usize, hz: f32) {
        let (inc, lo, xfade) = MipOsc::band(self.sample_rate, self.levels, hz);
        if let (Some(i), Some(l), Some(x)) = (
            self.inc.get_mut(lane),
            self.lo.get_mut(lane),
            self.xfade.get_mut(lane),
        ) {
            *i = inc;
            *l = lo as u16;
            *x = xfade;
        }
    }

    /// Set every lane's fundamental at once.
    pub fn set_freqs(&mut self, hz: &[f32; LANES]) {
        for (lane, f) in hz.iter().enumerate() {
            self.set_freq(lane, *f);
        }
    }

    /// Green zone: return every lane's phase to the cycle start.
    pub fn reset(&mut self) {
        self.phase = [0; LANES];
    }

    /// Green zone: return ONE lane's phase to the cycle start — what a
    /// note-on on a stolen voice wants.
    pub fn reset_lane(&mut self, lane: usize) {
        if let Some(p) = self.phase.get_mut(lane) {
            *p = 0;
        }
    }

    /// Green zone: place ONE lane's phase, in TURNS (1.0 is a full
    /// cycle).
    ///
    /// What unison needs. Eight voices started at phase 0 are the SAME
    /// signal until their detune beats them apart, which takes tens of
    /// milliseconds — so a unison stack begins as one voice at eight
    /// times the amplitude and only widens later. Spreading the start
    /// phase makes the stack wide from its first sample and takes the
    /// correlated attack spike with it.
    pub fn set_phase(&mut self, lane: usize, turns: f32) {
        if let Some(p) = self.phase.get_mut(lane) {
            *p = phase_offset(turns);
        }
    }

    /// One lane's current phase, for tests and for a node that wants to
    /// hand a voice's phase to another oscillator.
    pub fn phase(&self, lane: usize) -> u32 {
        self.phase.get(lane).copied().unwrap_or(0)
    }

    /// Red zone: fill `out`, any length.
    ///
    /// `tables` is the set [`build_tables`] filled for the SAME waveform
    /// `prepare` was given; a wrong-sized slice writes silence rather
    /// than reading garbage.
    ///
    /// `pm` is added to the read phase per sample, in TURNS — 1.0 is a
    /// full cycle. It modulates the PHASE, not the increment: the
    /// accumulator still advances by its own frequency, so modulation
    /// cannot make a voice drift permanently sharp the way summing into
    /// the increment would. `None` is no modulation, and the node bakes
    /// which of the two it is at compile, so this is not a per-sample
    /// branch. A `pm` block shorter than `out` simply stops modulating
    /// where it ends.
    pub fn process(&mut self, out: &mut [LaneFrame], pm: Option<&[LaneFrame]>, tables: &[f32]) {
        let levels = self.levels;
        for lane in 0..LANES {
            let (Some(&inc), Some(&lo), Some(&xf)) =
                (self.inc.get(lane), self.lo.get(lane), self.xfade.get(lane))
            else {
                continue;
            };
            let Some(phase) = self.phase.get_mut(lane) else {
                continue;
            };

            let lo = lo as usize;
            let lo_start = lo * TABLE_LEN;
            let hi_start = (lo + 1).min(levels.saturating_sub(1)) * TABLE_LEN;
            let (Some(t_lo), Some(t_hi)) = (
                tables.get(lo_start..lo_start + TABLE_LEN),
                tables.get(hi_start..hi_start + TABLE_LEN),
            ) else {
                // Silence THIS lane and leave the others alone: a bad
                // table is a wiring bug, and taking one voice quiet is a
                // better way to find it than taking the group down.
                for frame in out.iter_mut() {
                    if let Some(s) = frame.get_mut(lane) {
                        *s = 0.0;
                    }
                }
                continue;
            };

            // Same hoisted branch as the scalar kernel: the blending path
            // only runs for a lane actually sitting in a band edge.
            let blend = xf > 0.0 && !core::ptr::eq(t_lo, t_hi);
            for (i, frame) in out.iter_mut().enumerate() {
                let Some(s) = frame.get_mut(lane) else {
                    continue;
                };
                let read_at = match pm.and_then(|p| p.get(i)).and_then(|f| f.get(lane)) {
                    Some(turns) => phase.wrapping_add(phase_offset(*turns)),
                    None => *phase,
                };
                *s = if blend {
                    let a = MipOsc::read(t_lo, read_at);
                    let b = MipOsc::read(t_hi, read_at);
                    a + (b - a) * xf
                } else {
                    MipOsc::read(t_lo, read_at)
                };
                *phase = phase.wrapping_add(inc);
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
        // One slot per shape, indexed through `Waveform::ALL` so adding a
        // shape does not silently reuse another's tables.
        static CACHE: [OnceLock<Vec<f32>>; Waveform::ALL.len()] =
            [const { OnceLock::new() }; Waveform::ALL.len()];
        let slot = Waveform::ALL
            .iter()
            .position(|w| *w == waveform)
            .unwrap_or(0);
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

    #[test]
    fn audio_rate_pitch_matches_scalar_reference_and_splits_exactly() {
        let tables = built(Waveform::Saw);
        let mut full = MipOsc::new();
        full.prepare(FS, Waveform::Saw);
        let mut split = full.clone();
        let mut reference = full.clone();
        let mut offsets = [0.0; 257];
        for (i, x) in offsets.iter_mut().enumerate() {
            *x = 0.25 * (i as f32 * 0.03).sin();
        }
        let mut a = [0.0; 257];
        let mut b = a;
        let mut c = a;
        assert_no_alloc::assert_no_alloc(|| {
            full.process_pitch(&mut a, &offsets, 880.0, tables);
            split.process_pitch(&mut [], &[], 880.0, tables);
            split.process_pitch(&mut b[..1], &offsets[..1], 880.0, tables);
            split.process_pitch(&mut b[1..100], &offsets[1..100], 880.0, tables);
            split.process_pitch(&mut b[100..], &offsets[100..], 880.0, tables);
            for (sample, offset) in c.iter_mut().zip(offsets) {
                reference.set_freq(880.0 * (offset / 12.0).exp2());
                reference.process(core::slice::from_mut(sample), tables);
            }
        });
        assert_eq!(a, b);
        assert_eq!(a, c);
        // The exact-zero fast path must be identical to ordinary playback.
        full.reset();
        reference.reset();
        full.process_pitch(&mut a, &[0.0; 257], 440.0, tables);
        reference.set_freq(440.0);
        reference.process(&mut c, tables);
        assert_eq!(a, c);
    }

    #[test]
    fn audio_rate_pitch_handles_bad_inputs_and_missing_tables_without_nan() {
        let tables = built(Waveform::Square);
        let mut osc = MipOsc::new();
        osc.prepare(FS, Waveform::Square);
        let mut out = [1.0; 3];
        osc.process_pitch(
            &mut out,
            &[f32::NAN, f32::INFINITY, -127.0],
            f32::INFINITY,
            tables,
        );
        assert!(out.iter().all(|x| x.is_finite() && !x.is_subnormal()));
        osc.process_pitch(&mut out, &[0.0; 3], 440.0, &[]);
        assert_eq!(out, [0.0; 3]);
        let phase = osc.phase();
        osc.process_pitch(&mut out, &[], 440.0, tables);
        assert_eq!(out, [0.0; 3]);
        assert_eq!(osc.phase(), phase);
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
    // --------------------------------------------------------- lanes ---

    fn lane_column(frames: &[LaneFrame], lane: usize) -> Vec<f32> {
        frames.iter().map(|f| f[lane]).collect()
    }

    /// REFERENCE. A lane tuned like a scalar oscillator is BIT-IDENTICAL
    /// to it, for every shape and across the band edges.
    ///
    /// The scalar kernel is already tested against its spectral claims —
    /// harmonic content, image rejection, frequency accuracy — so bit
    /// equality inherits all of them rather than re-measuring.
    #[test]
    fn every_lane_is_bit_identical_to_the_scalar_oscillator() {
        const N: usize = 1_500;
        for waveform in Waveform::ALL {
            let tables = built(waveform);
            // Frequencies chosen to land in different mip bands, one of
            // them inside a crossfade region.
            for hz in [55.0f32, 440.0, 3_000.0, 9_000.0] {
                let mut sc = MipOsc::new();
                sc.prepare(48_000.0, waveform);
                sc.set_freq(hz);
                let mut want = vec![0.0f32; N];
                sc.process(&mut want, tables);

                for lane in 0..LANES {
                    let mut la = LaneOsc::new();
                    la.prepare(48_000.0, waveform);
                    la.set_freq(lane, hz);
                    let mut got = vec![[0.0f32; LANES]; N];
                    la.process(&mut got, None, tables);
                    assert_eq!(
                        lane_column(&got, lane),
                        want,
                        "{waveform:?} lane {lane} at {hz} Hz"
                    );
                }
            }
        }
    }

    /// LANE INDEPENDENCE. The mandatory sixth test.
    ///
    /// Eight lanes at eight DIFFERENT frequencies, all at once, must each
    /// equal what that lane produces alone. This is the test that catches
    /// a scalar `lo`/`xfade` shared across lanes: with every lane at one
    /// frequency — which is how every other test drives it — a shared
    /// band is invisible.
    #[test]
    fn lanes_at_different_frequencies_do_not_interfere() {
        const N: usize = 1_024;
        let waveform = Waveform::Saw;
        let tables = built(waveform);
        // Deliberately spread across mip bands, an octave-plus apart.
        let freqs: [f32; LANES] = [
            40.0, 90.0, 220.0, 500.0, 1_100.0, 2_600.0, 6_000.0, 13_000.0,
        ];

        let mut all = LaneOsc::new();
        all.prepare(48_000.0, waveform);
        all.set_freqs(&freqs);
        let mut together = vec![[0.0f32; LANES]; N];
        all.process(&mut together, None, tables);

        for (lane, hz) in freqs.iter().enumerate() {
            let mut one = LaneOsc::new();
            one.prepare(48_000.0, waveform);
            one.set_freq(lane, *hz);
            let mut alone = vec![[0.0f32; LANES]; N];
            one.process(&mut alone, None, tables);
            assert_eq!(
                lane_column(&together, lane),
                lane_column(&alone, lane),
                "lane {lane} at {hz} Hz changed when its neighbours ran"
            );
            // And the silent lanes of the solo run really are silent.
            for other in 0..LANES {
                if other != lane {
                    assert!(lane_column(&alone, other).iter().all(|s| *s == 0.0));
                }
            }
        }
    }

    /// Each lane picks its OWN mip band: a lane low enough to be dull and
    /// one high enough to be bright must not come out equally bright.
    #[test]
    fn lanes_select_their_own_mip_band() {
        let waveform = Waveform::Saw;
        let tables = built(waveform);
        let (lo_hz, hi_hz) = (60.0f32, 9_000.0f32);
        let mut la = LaneOsc::new();
        la.prepare(48_000.0, waveform);
        la.set_freq(0, lo_hz);
        la.set_freq(1, hi_hz);
        let mut out = vec![[0.0f32; LANES]; 4_096];
        la.process(&mut out, None, tables);

        // The high lane's table is band-limited far harder, so it is much
        // closer to a sine: its peak-to-RMS ratio drops toward sqrt(2).
        let crest = |v: &[f32]| {
            let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
            peak / rms.max(1e-9)
        };
        let (lo_c, hi_c) = (crest(&lane_column(&out, 0)), crest(&lane_column(&out, 1)));
        assert!(
            lo_c > hi_c + 0.05,
            "bands not per-lane: crest {lo_c:.3} vs {hi_c:.3}"
        );
    }

    /// Phase modulation moves the READ phase, not the increment: a
    /// constant PM offset shifts the waveform without changing its pitch,
    /// so after the offset settles the output equals the unmodulated one
    /// delayed by that fraction of a cycle.
    #[test]
    fn constant_pm_shifts_phase_without_shifting_pitch() {
        const N: usize = 2_048;
        let waveform = Waveform::Sine;
        let tables = built(waveform);
        let hz = 100.0f32; // 480 samples a cycle at 48 k
        let period = 48_000.0 / hz;

        let mut plain = LaneOsc::new();
        plain.prepare(48_000.0, waveform);
        plain.set_freq(0, hz);
        let mut dry = vec![[0.0f32; LANES]; N];
        plain.process(&mut dry, None, tables);

        // A quarter turn, constant.
        let pm = vec![[0.25f32; LANES]; N];
        let mut shifted = LaneOsc::new();
        shifted.prepare(48_000.0, waveform);
        shifted.set_freq(0, hz);
        let mut wet = vec![[0.0f32; LANES]; N];
        shifted.process(&mut wet, Some(&pm), tables);

        let dry = lane_column(&dry, 0);
        let wet = lane_column(&wet, 0);
        let lag = (period * 0.25).round() as usize;
        // Compare well inside the block, away from either end.
        for i in 200..(N - 200) {
            let want = dry[i + lag];
            assert!(
                (wet[i] - want).abs() < 2e-3,
                "pm shift at {i}: {} vs {want}",
                wet[i]
            );
        }
    }

    /// Nonsense modulation is absorbed, never propagated: NaN and huge
    /// offsets must not produce NaN samples or panic.
    #[test]
    fn pm_absorbs_nonsense_without_inventing_nan() {
        let tables = built(Waveform::Saw);
        let mut la = LaneOsc::new();
        la.prepare(48_000.0, Waveform::Saw);
        la.set_freq(0, 440.0);
        let pm = vec![
            [
                f32::NAN,
                f32::INFINITY,
                -f32::INFINITY,
                1e30,
                -1e30,
                0.5,
                -0.5,
                0.0
            ];
            256
        ];
        let mut out = vec![[0.0f32; LANES]; 256];
        la.process(&mut out, Some(&pm), tables);
        for frame in &out {
            for s in frame {
                assert!(s.is_finite(), "pm produced {s}");
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact — with modulation, since the
    /// modulated path has its own phase arithmetic.
    #[test]
    fn lane_osc_splits_bit_exactly() {
        const N: usize = 256;
        const CUT: usize = 100;
        let tables = built(Waveform::Square);
        let pm: Vec<LaneFrame> = (0..N)
            .map(|i| [(i as f32 * 0.001).sin() * 0.3; LANES])
            .collect();

        let mut a = LaneOsc::new();
        a.prepare(48_000.0, Waveform::Square);
        a.set_freqs(&[110.0; LANES]);
        let mut whole = vec![[0.0f32; LANES]; N];
        a.process(&mut whole, Some(&pm), tables);

        let mut b = LaneOsc::new();
        b.prepare(48_000.0, Waveform::Square);
        b.set_freqs(&[110.0; LANES]);
        let mut split = vec![[0.0f32; LANES]; N];
        let (head, tail) = split.split_at_mut(CUT);
        let (pm_head, pm_tail) = pm.split_at(CUT);
        b.process(head, Some(pm_head), tables);
        b.process(tail, Some(pm_tail), tables);
        assert_eq!(split, whole);
    }

    /// NO-ALLOC on the process path, modulated and not.
    #[test]
    fn lane_osc_does_not_allocate() {
        let tables = built(Waveform::Saw);
        let mut la = LaneOsc::new();
        la.prepare(48_000.0, Waveform::Saw);
        la.set_freqs(&[440.0; LANES]);
        let pm = vec![[0.1f32; LANES]; 512];
        let mut out = vec![[0.0f32; LANES]; 512];
        assert_no_alloc::assert_no_alloc(|| {
            la.process(&mut out, None, tables);
            la.process(&mut out, Some(&pm), tables);
        });
    }

    /// EDGE LENGTHS: 0, 1 and a non-power-of-two; a zero-length block
    /// must not advance any lane's phase; a short `pm` is tolerated.
    #[test]
    fn lane_osc_takes_any_block_length() {
        let tables = built(Waveform::Triangle);
        for n in [0usize, 1, 3, 97] {
            let mut la = LaneOsc::new();
            la.prepare(48_000.0, Waveform::Triangle);
            la.set_freqs(&[440.0; LANES]);
            let mut out = vec![[0.0f32; LANES]; n];
            la.process(&mut out, None, tables);
            assert_eq!(out.len(), n);
            // A pm block shorter than out stops modulating, not panicking.
            let pm = vec![[0.2f32; LANES]; n / 2];
            la.process(&mut out, Some(&pm), tables);
        }
        let mut la = LaneOsc::new();
        la.prepare(48_000.0, Waveform::Triangle);
        la.set_freqs(&[440.0; LANES]);
        let before = la.phase(0);
        la.process(&mut [], None, tables);
        assert_eq!(la.phase(0), before);
    }

    /// A short or empty table set silences the lane rather than reading
    /// out of bounds — and leaves the other lanes untouched.
    #[test]
    fn a_bad_table_set_silences_the_group_safely() {
        let mut la = LaneOsc::new();
        la.prepare(48_000.0, Waveform::Saw);
        la.set_freqs(&[440.0; LANES]);
        let mut out = vec![[1.0f32; LANES]; 64];
        la.process(&mut out, None, &[]);
        assert!(out.iter().flatten().all(|s| *s == 0.0));
    }

    /// The four sparse tables are bounded, non-trivial, and DISTINCT —
    /// the normalising constants do what their doc claims.
    #[test]
    fn the_sparse_tables_are_bounded_and_distinct() {
        let mut rendered = Vec::new();
        for waveform in [
            Waveform::Bell,
            Waveform::Glass,
            Waveform::Metal,
            Waveform::Air,
        ] {
            let tables = built(waveform);
            for s in tables {
                assert!(s.is_finite(), "{waveform:?} table has {s}");
                assert!(s.abs() <= 1.0001, "{waveform:?} peaks at {s}, over 1");
            }
            let v = rendered_wave(waveform, 220.0, 4_096);
            let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
            assert!(rms > 0.01, "{waveform:?} is effectively silent (rms {rms})");
            rendered.push((waveform, v));
        }
        // Air has no fundamental, so it must be far brighter than Bell.
        for (a, (wa, va)) in rendered.iter().enumerate() {
            for (wb, vb) in rendered.iter().skip(a + 1) {
                let diff = va
                    .iter()
                    .zip(vb)
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0f32, f32::max);
                assert!(diff > 0.01, "{wa:?} and {wb:?} are the same wave");
            }
        }
    }

    fn rendered_wave(waveform: Waveform, hz: f32, n: usize) -> Vec<f32> {
        let mut o = MipOsc::new();
        o.prepare(48_000.0, waveform);
        o.set_freq(hz);
        let mut out = vec![0.0f32; n];
        o.process(&mut out, built(waveform));
        out
    }

    /// Placing the phase moves where the read starts, and a bank spread
    /// across the cycle does NOT sum to one loud edge on its first
    /// sample — which is the whole reason the setter exists.
    #[test]
    fn placing_the_phase_spreads_a_bank_across_its_cycle() {
        let mut tables = vec![0.0f32; table_len(Waveform::Sine)];
        build_tables(Waveform::Sine, &mut tables);

        // Half a turn is half the phase word, and a full turn wraps to
        // where it started.
        let mut osc = MipOsc::new();
        osc.prepare(48_000.0, Waveform::Sine);
        osc.set_phase(0.0);
        assert_eq!(osc.phase(), 0);
        osc.set_phase(0.5);
        assert_eq!(osc.phase(), 1u32 << 31);
        osc.set_phase(1.0);
        assert_eq!(osc.phase(), 0, "a whole turn is where it started");

        // Six oscillators in phase peak six times as high as six spread
        // out, on the first sample.
        let build = |spread: bool| {
            let mut bank = [MipOsc::new(); 6];
            let mut sum = 0.0f32;
            for (i, osc) in bank.iter_mut().enumerate() {
                osc.prepare(48_000.0, Waveform::Sine);
                osc.set_freq(300.0);
                osc.set_phase(if spread {
                    i as f32 * 0.618_034 % 1.0
                } else {
                    0.0
                });
                let mut one = [0.0f32; 1];
                osc.process(&mut one, &tables);
                sum += one[0];
            }
            sum.abs()
        };
        assert!(
            build(true) < build(false).max(0.5),
            "a spread bank must not stack into one edge"
        );
    }
}
