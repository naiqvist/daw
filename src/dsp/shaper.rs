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

use crate::dsp::{LANES, LaneFrame};

/// Full length of the halfband FIR. Odd; centre = (N−1)/2. Sized so the
/// Kaiser transition (≈ 8 kHz at 96 k) puts the stopband where content
/// would fold below ~20 kHz.
const HB_LEN: usize = 71;
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
    // `(2/π)·asin(sin(πx/2))` is the textbook way to write this, and it
    // is the same curve — but `asin` has an infinite derivative at ±1,
    // which is EXACTLY where a folder spends its time. Measured against
    // an f64 reference, that form carried 1.5e-4 of error through the
    // unit range: a −76 dB noise floor on the one shape whose whole
    // point is the fold peaks. This is the same triangle in closed
    // form, exact to 9e-8, and it drops a sin and an asin per sample.
    let m = (x + 1.0) * 0.25;
    let t = (m - m.floor()) * 4.0;
    1.0 - (t - 2.0).abs()
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
        // Exact bypasses return the input before doing ANY arithmetic.
        // In particular, the closed-form triangle is mathematically x
        // inside the rails at drive one, but its rearranged operations can
        // still move x by an ulp.
        if self.mix == 0.0
            || (self.mode == Mode::Fold
                && self.drive == DRIVE_MIN
                && self.bias == 0.0
                && (-1.0..=1.0).contains(&x))
        {
            return x;
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

/// Non-zero taps in the halfband: the centre plus every even index.
/// A halfband's odd-offset taps are EXACTLY zero — `sin(πn/2)` vanishes
/// at every even `n` — which is what makes the polyphase form free.
const HALF_LEN: usize = HB_LEN.div_ceil(2);
/// History ring for an arm that runs the 36-tap sum. Power of two ≥
/// HALF_LEN; stored doubled so any window is a flat ascending slice.
const PHASE_RING: usize = 64;
/// History ring for the arm that is a pure delay — it only ever reads
/// `CENTRE_BACK + 1` back, so it does not need the big ring.
const DELAY_RING: usize = 32;
/// The centre tap sits at an odd offset from itself-as-origin, so its
/// arm reads the input this many samples back at the ORIGINAL rate:
/// (C − 1) / 2, where C = (HB_LEN − 1) / 2 is the centre index.
const CENTRE_BACK: usize = ((HB_LEN - 1) / 2 - 1) / 2;

/// Green zone: build the Kaiser-windowed halfband (sinc at a quarter of
/// the doubled rate) and split it into the two arms the polyphase form
/// runs. Computed in f64 and normalised so DC through the whole round
/// trip is exactly unity. One window in this file, shared by the scalar
/// and lane kernels — there is no second Kaiser anywhere.
fn halfband_arms() -> ([f32; HALF_LEN], f32) {
    let h = halfband_taps();
    let mut even = [0.0f32; HALF_LEN];
    for (j, dst) in even.iter_mut().enumerate() {
        // Reversed, so tap j pairs with the j-th OLDEST sample of the
        // window and both walk upward together.
        if let Some(src) = h.get(2 * (HALF_LEN - 1 - j)) {
            *dst = *src as f32;
        }
    }
    let centre = h.get((HB_LEN - 1) / 2).map(|t| *t as f32).unwrap_or(0.0);
    (even, centre)
}

/// The full 71-tap halfband, normalised to unit DC gain. The polyphase
/// arms are a VIEW of this, and `the_halfband_odd_taps_are_zero` is what
/// entitles them to be: change the window, the length, or the sinc and
/// that test is the thing that notices the split has stopped being
/// lossless.
fn halfband_taps() -> [f64; HB_LEN] {
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
    for tap in h.iter_mut() {
        *tap /= sum;
    }
    h
}

/// 2× oversampler: linear-phase halfband up/down, caller scratch for the
/// doubled-rate signal, honest latency.
///
/// # Why this costs a third of what it looks like
///
/// Of the 71 taps only 37 do anything: the centre, and every even index.
/// Convolving the zeros anyway was this kernel's documented shortcut,
/// and this is it paid off. Each arm keeps its own history at the
/// ORIGINAL rate, so `up` writes one filtered slot (36 taps) and one
/// DELAYED slot (the lone odd tap — a single multiply) instead of two
/// 71-tap sweeps, and `down` computes only the phase it keeps. 213
/// multiply-adds per input sample became 74, and each history is read as
/// a flat ascending slice instead of through a masked ring, so what
/// remains vectorises.
///
/// It is the same sum with its zero terms dropped: the alias figures
/// below are the ones the tests still pin.
///
/// State: two 64-slot histories, one 32-slot delay, 36 coefficients
/// (~1.4 KB). Per-sample cost, MEASURED in RELEASE (the profile the
/// engine ships; `report_cost_per_sample` under `cargo test` alone runs
/// at `opt-level = 1` and reads about three times slower) on the full
/// wrap around a hard clip: 151 ns/sample convolving the zeros, 36 now.
/// Alias improvement at full drive, measured: worst image −27.2 dB
/// naive, −43.6 dB wrapped.
/// Denormal-safe: FIR state decays through FTZ on silence.
/// In-place safe: n/a — up and down have distinct in/out slices.
/// Latency: [`Self::latency`] samples at the ORIGINAL rate (linear
/// phase: the two filter group delays sum to exactly the centre tap).
#[derive(Debug, Clone, Copy)]
pub struct Oversampler2x {
    even: [f32; HALF_LEN],
    centre: f32,
    up_hist: [f32; PHASE_RING * 2],
    down_a: [f32; PHASE_RING * 2],
    down_b: [f32; DELAY_RING * 2],
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
            even: [0.0; HALF_LEN],
            centre: 0.0,
            up_hist: [0.0; PHASE_RING * 2],
            down_a: [0.0; PHASE_RING * 2],
            down_b: [0.0; DELAY_RING * 2],
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

    /// Green zone: build the halfband arms.
    pub fn prepare(&mut self) {
        let (even, centre) = halfband_arms();
        self.even = even;
        self.centre = centre;
        self.reset();
    }

    /// Green zone: forget the signal history.
    pub fn reset(&mut self) {
        self.up_hist = [0.0; PHASE_RING * 2];
        self.down_a = [0.0; PHASE_RING * 2];
        self.down_b = [0.0; DELAY_RING * 2];
        self.up_pos = 0;
        self.down_pos = 0;
    }

    /// The round-trip delay at the ORIGINAL rate. Report to PDC.
    pub fn latency(&self) -> usize {
        (HB_LEN - 1) / 2
    }

    /// Write one sample into a doubled ring, so both copies stay valid.
    #[inline(always)]
    fn push(hist: &mut [f32], pos: usize, ring: usize, v: f32) {
        if let Some(s) = hist.get_mut(pos) {
            *s = v;
        }
        if let Some(s) = hist.get_mut(pos + ring) {
            *s = v;
        }
    }

    /// The 36-tap arm over a flat, ascending window. `pos` is the slot
    /// the newest sample went to; the doubling makes `base..base+36`
    /// contiguous for every `pos`, so this is a plain zipped MAC.
    #[inline(always)]
    fn arm(hist: &[f32], pos: usize, even: &[f32; HALF_LEN]) -> f32 {
        let base = pos + PHASE_RING + 1 - HALF_LEN;
        let mut acc = 0.0f32;
        if let Some(win) = hist.get(base..base + HALF_LEN) {
            for (c, h) in even.iter().zip(win.iter()) {
                acc += c * h;
            }
        }
        acc
    }

    /// The delay arm: one sample, `back` behind the newest.
    #[inline(always)]
    fn tapped(hist: &[f32], pos: usize, ring: usize, back: usize) -> f32 {
        hist.get(pos + ring - back).copied().unwrap_or(0.0)
    }

    /// Red zone: upsample `input` into `out2x`, which must be exactly
    /// twice as long (the `scratch_len` slice). Wrong sizes fail open —
    /// nothing written.
    pub fn up(&mut self, input: &[f32], out2x: &mut [f32]) {
        if out2x.len() != input.len() * 2 {
            return;
        }
        for (x, pair) in input.iter().zip(out2x.as_chunks_mut::<2>().0) {
            self.up_pos = (self.up_pos + 1) & (PHASE_RING - 1);
            Self::push(&mut self.up_hist, self.up_pos, PHASE_RING, *x);
            // ×2 to preserve amplitude across the zero-stuff, exactly as
            // the naive form did.
            pair[0] = 2.0 * Self::arm(&self.up_hist, self.up_pos, &self.even);
            pair[1] = 2.0
                * self.centre
                * Self::tapped(&self.up_hist, self.up_pos, PHASE_RING, CENTRE_BACK);
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
    /// impulse test caught it). In this form that shows up as the two
    /// arms taking the two slots: the even arm gets slot 0, and slot 1
    /// reaches the output only through the centre tap.
    pub fn down(&mut self, in2x: &[f32], out: &mut [f32]) {
        if in2x.len() != out.len() * 2 {
            return;
        }
        for (pair, y) in in2x.as_chunks::<2>().0.iter().zip(out.iter_mut()) {
            self.down_pos = self.down_pos.wrapping_add(1);
            let a = self.down_pos & (PHASE_RING - 1);
            let b = self.down_pos & (DELAY_RING - 1);
            Self::push(&mut self.down_a, a, PHASE_RING, pair[0]);
            Self::push(&mut self.down_b, b, DELAY_RING, pair[1]);
            *y = Self::arm(&self.down_a, a, &self.even)
                + self.centre * Self::tapped(&self.down_b, b, DELAY_RING, CENTRE_BACK + 1);
        }
    }
}

// ------------------------------------------------------------- lanes ---

impl Waveshaper {
    /// Red zone: shape a whole voice group in place, any length.
    ///
    /// A method rather than a `LaneWaveshaper` type, and that is not an
    /// inconsistency with the other lane kernels: this one is STATELESS.
    /// [`Waveshaper::shape`] is a pure function of a sample, so there is
    /// nothing per-lane to keep and nothing to get wrong — the curve is
    /// the patch's, shared, exactly as the coefficients are elsewhere.
    pub fn process_lanes(&self, io: &mut [LaneFrame]) {
        for frame in io.iter_mut() {
            for s in frame.iter_mut() {
                *s = self.shape(*s);
            }
        }
    }
}

/// [`Oversampler2x`] for a whole voice group.
///
/// Same polyphase split as the scalar kernel, and the same reason: the
/// halfband's odd taps are zero, so one arm is a 36-tap sum and the
/// other is a single multiply on a delayed sample.
///
/// The taps are shared — one patch, one filter — and each history is
/// stored SLOT-MAJOR (`[[f32; LANES]; _]`) so one tap reads every lane's
/// history from one contiguous, register-shaped slot. Stored the other
/// way round, each tap would stride across [`LANES`] separate histories.
/// The doubled ring keeps the window flat here too, so the inner loop is
/// a straight walk over slots with no masking in it.
///
/// State: (2 × PHASE_RING + DELAY_RING) × 2 × [`LANES`] floats plus the
/// arms.
/// Per-sample-per-lane cost: the scalar's — 36 multiply-adds plus one
/// multiply per arm, rather than HB_LEN per 2× slot.
/// Denormal-safe: an FIR cannot recirculate, so a decayed tail leaves the
/// ring after PHASE_RING samples rather than lingering.
/// In-place safe: no — `up` and `down` read one buffer and write another.
/// Latency: [`Self::latency`] samples, the same as the scalar kernel's,
/// and the constant-latency rule applies — a node reporting this feeds
/// plugin delay compensation.
#[derive(Debug, Clone, Copy)]
pub struct LaneOversampler2x {
    even: [f32; HALF_LEN],
    centre: f32,
    up_hist: [[f32; LANES]; PHASE_RING * 2],
    down_a: [[f32; LANES]; PHASE_RING * 2],
    down_b: [[f32; LANES]; DELAY_RING * 2],
    up_pos: usize,
    down_pos: usize,
}

impl Default for LaneOversampler2x {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneOversampler2x {
    pub fn new() -> Self {
        let mut o = Self {
            even: [0.0; HALF_LEN],
            centre: 0.0,
            up_hist: [[0.0; LANES]; PHASE_RING * 2],
            down_a: [[0.0; LANES]; PHASE_RING * 2],
            down_b: [[0.0; LANES]; DELAY_RING * 2],
            up_pos: 0,
            down_pos: 0,
        };
        o.prepare();
        o
    }

    /// Frames of scratch `up` needs for a block: twice the block.
    pub fn scratch_len(block: usize) -> usize {
        block * 2
    }

    /// Green zone: take the halfband from the one builder in this file,
    /// so there is one Kaiser window here and not two.
    pub fn prepare(&mut self) {
        let (even, centre) = halfband_arms();
        self.even = even;
        self.centre = centre;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.up_hist = [[0.0; LANES]; PHASE_RING * 2];
        self.down_a = [[0.0; LANES]; PHASE_RING * 2];
        self.down_b = [[0.0; LANES]; DELAY_RING * 2];
        self.up_pos = 0;
        self.down_pos = 0;
    }

    /// Green zone: clear ONE lane's history through every ring.
    pub fn reset_lane(&mut self, lane: usize) {
        for slot in self
            .up_hist
            .iter_mut()
            .chain(self.down_a.iter_mut())
            .chain(self.down_b.iter_mut())
        {
            if let Some(v) = slot.get_mut(lane) {
                *v = 0.0;
            }
        }
    }

    /// The round trip's group delay, in input samples. Identical to the
    /// scalar kernel's, because the taps are identical.
    pub fn latency(&self) -> usize {
        (HB_LEN - 1) / 2
    }

    #[inline(always)]
    fn push(hist: &mut [[f32; LANES]], pos: usize, ring: usize, v: &[f32; LANES]) {
        if let Some(s) = hist.get_mut(pos) {
            *s = *v;
        }
        if let Some(s) = hist.get_mut(pos + ring) {
            *s = *v;
        }
    }

    /// The 36-tap arm, swept across every lane at once.
    #[inline(always)]
    fn arm(hist: &[[f32; LANES]], pos: usize, even: &[f32; HALF_LEN]) -> [f32; LANES] {
        let base = pos + PHASE_RING + 1 - HALF_LEN;
        let mut acc = [0.0f32; LANES];
        if let Some(win) = hist.get(base..base + HALF_LEN) {
            for (c, slot) in even.iter().zip(win.iter()) {
                for (a, h) in acc.iter_mut().zip(slot.iter()) {
                    *a += c * *h;
                }
            }
        }
        acc
    }

    /// The delay arm: one slot, `back` behind the newest.
    #[inline(always)]
    fn tapped(hist: &[[f32; LANES]], pos: usize, ring: usize, back: usize) -> [f32; LANES] {
        hist.get(pos + ring - back).copied().unwrap_or([0.0; LANES])
    }

    /// Red zone: upsample `input` into `out2x`, which must be exactly
    /// twice as long. Wrong sizes fail open — nothing written.
    pub fn up(&mut self, input: &[LaneFrame], out2x: &mut [LaneFrame]) {
        if out2x.len() != input.len() * 2 {
            return;
        }
        for (x, pair) in input.iter().zip(out2x.as_chunks_mut::<2>().0) {
            self.up_pos = (self.up_pos + 1) & (PHASE_RING - 1);
            Self::push(&mut self.up_hist, self.up_pos, PHASE_RING, x);
            // ×2 to preserve amplitude across the zero-stuff.
            let filtered = Self::arm(&self.up_hist, self.up_pos, &self.even);
            let delayed = Self::tapped(&self.up_hist, self.up_pos, PHASE_RING, CENTRE_BACK);
            for (o, f) in pair[0].iter_mut().zip(filtered.iter()) {
                *o = 2.0 * *f;
            }
            for (o, d) in pair[1].iter_mut().zip(delayed.iter()) {
                *o = 2.0 * self.centre * *d;
            }
        }
    }

    /// Red zone: filter and decimate `in2x` back into `out`, which must
    /// be exactly half as long. Wrong sizes fail open.
    ///
    /// The kept phase is the FIRST slot of each pair, for the same reason
    /// the scalar kernel keeps it: the cascade's group delay is an even
    /// number of 2x slots, so sampling the odd phase reads every peak
    /// half a sample off.
    pub fn down(&mut self, in2x: &[LaneFrame], out: &mut [LaneFrame]) {
        if in2x.len() != out.len() * 2 {
            return;
        }
        for (pair, y) in in2x.as_chunks::<2>().0.iter().zip(out.iter_mut()) {
            self.down_pos = self.down_pos.wrapping_add(1);
            let a = self.down_pos & (PHASE_RING - 1);
            let b = self.down_pos & (DELAY_RING - 1);
            Self::push(&mut self.down_a, a, PHASE_RING, &pair[0]);
            Self::push(&mut self.down_b, b, DELAY_RING, &pair[1]);
            let filtered = Self::arm(&self.down_a, a, &self.even);
            let delayed = Self::tapped(&self.down_b, b, DELAY_RING, CENTRE_BACK + 1);
            for ((o, f), d) in y.iter_mut().zip(filtered.iter()).zip(delayed.iter()) {
                *o = *f + self.centre * *d;
            }
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

    /// The polyphase split is lossless ONLY because a halfband's
    /// odd-offset taps vanish — `sin(πn/2)` is zero at every even `n`.
    /// `up` and `down` now read 37 numbers and assume the other 34 are
    /// nothing. Nothing else in this file checks that assumption, so a
    /// change to `HB_LEN`, `KAISER_BETA`, or the sinc could quietly turn
    /// the arms into a different filter than the one they document,
    /// while every transparency test still passed on the arms' own terms.
    #[test]
    fn the_halfband_odd_taps_are_zero_and_the_arms_are_the_rest() {
        let h = halfband_taps();
        let centre = (HB_LEN - 1) / 2;
        assert_eq!(centre % 2, 1, "centre index parity is what splits the arms");

        // Every odd index except the centre must be exactly negligible.
        let worst = h
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != centre && i % 2 == 1)
            .map(|(_, t)| t.abs())
            .fold(0.0f64, f64::max);
        assert!(
            worst < 1e-12,
            "an odd tap is carrying signal ({worst:e}); the polyphase arms drop it on the floor"
        );

        // Unit DC through the whole filter, and the two arms split it.
        let total: f64 = h.iter().sum();
        assert!((total - 1.0).abs() < 1e-12, "halfband DC gain is {total}");

        let (even, ct) = halfband_arms();
        let arm_dc: f64 = even.iter().map(|c| *c as f64).sum();
        assert!(
            (arm_dc + ct as f64 - 1.0).abs() < 1e-6,
            "the arms must add back up to the filter: {arm_dc} + {ct}"
        );

        // The arms really are the even taps (reversed) and the centre.
        for (j, c) in even.iter().enumerate() {
            let want = h[2 * (HALF_LEN - 1 - j)] as f32;
            assert_eq!(
                *c,
                want,
                "even arm tap {j} is not halfband tap {}",
                2 * (HALF_LEN - 1 - j)
            );
        }
        assert_eq!(ct, h[centre] as f32, "delay arm is not the centre tap");
    }

    /// The fold is a triangle wave, and it has to BE one to the float.
    ///
    /// This is the reference-correctness test the contract asks for, and
    /// it exists because the obvious spelling of this curve —
    /// `(2/π)·asin(sin(πx/2))` — passes every other test in this file
    /// while carrying 1.5e-4 of error, a −76 dB floor parked on the fold
    /// peaks where `asin` goes vertical. Nothing else here looks at the
    /// curve closely enough to notice, so this does.
    #[test]
    fn the_fold_is_a_triangle_to_within_a_float() {
        fn reference(x: f64) -> f64 {
            let m = (x + 1.0) * 0.25;
            let t = (m - m.floor()) * 4.0;
            1.0 - (t - 2.0).abs()
        }
        for &(lo, hi, tol, name) in &[
            (-1.0f64, 1.0f64, 1e-6f64, "unit range"),
            (0.98, 1.02, 1e-6, "across a peak"),
            (-DRIVE_MAX as f64, DRIVE_MAX as f64, 1e-5, "full drive"),
        ] {
            let n = 100_000;
            let worst = (0..=n)
                .map(|i| {
                    let x = lo + (hi - lo) * i as f64 / n as f64;
                    (triangle_fold(x as f32) as f64 - reference(x)).abs()
                })
                .fold(0.0f64, f64::max);
            assert!(
                worst < tol,
                "fold drifts from a triangle by {worst:e} over the {name}"
            );
        }

        // The peaks are the point: they must be reached exactly.
        for k in [-3.0f32, 1.0, 5.0] {
            assert_eq!(triangle_fold(k), 1.0, "fold must reach +1 at x={k}");
        }
        for k in [-1.0f32, 3.0, 7.0] {
            assert_eq!(triangle_fold(k), -1.0, "fold must reach -1 at x={k}");
        }
        // ...and it stays a bounded, odd, period-4 triangle.
        for i in -400..=400 {
            let x = i as f32 * 0.037;
            let y = triangle_fold(x);
            assert!((-1.0..=1.0).contains(&y), "fold left the rails at {x}");
            assert!(
                (triangle_fold(-x) + y).abs() < 1e-6,
                "fold must stay odd at {x}"
            );
            assert!(
                (triangle_fold(x + 4.0) - y).abs() < 1e-5,
                "fold must stay period-4 at {x}"
            );
        }
    }

    /// The bottom of a colour control is an exact bypass, not a curve
    /// that happens to land very close to the input.
    #[test]
    fn fold_drive_one_is_bit_exact_inside_the_rails() {
        let mut shaper = Waveshaper::new();
        shaper.configure(Mode::Fold, 1.0, 0.0, 1.0);
        for i in 0..=2_000 {
            let x = i as f32 / 1_000.0 - 1.0;
            assert_eq!(shaper.shape(x).to_bits(), x.to_bits(), "x = {x}");
        }
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted.
    ///
    /// Run it in RELEASE to get the numbers the docs quote — the engine
    /// ships at `opt-level = 3` with fat LTO, while plain `cargo test`
    /// builds this crate at `opt-level = 1` and reads roughly three
    /// times slower. Comparing a change across two runs also needs a
    /// CONTROL row that the change does not touch: a build leaves the
    /// machine hot enough to move every figure here by 2x, which is
    /// exactly how a pessimisation can read as a win.
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
    // --------------------------------------------------------- lanes ---

    fn lane_column(frames: &[LaneFrame], lane: usize) -> Vec<f32> {
        frames.iter().map(|f| f[lane]).collect()
    }

    fn spread(signal: &[f32]) -> Vec<LaneFrame> {
        signal.iter().map(|s| [*s; LANES]).collect()
    }

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 / n as f32) * 4.0 - 2.0).collect()
    }

    /// REFERENCE. Every lane of the shaper is BIT-IDENTICAL to the
    /// scalar curve, in every mode.
    #[test]
    fn every_shaper_lane_is_bit_identical_to_the_scalar_curve() {
        let signal = ramp(1_024);
        for mode in [
            Mode::HardClip,
            Mode::SoftClip,
            Mode::Cubic,
            Mode::Fold,
            Mode::Crush,
        ] {
            for drive in [1.0f32, 4.0, 32.0] {
                let mut ws = Waveshaper::new();
                ws.configure(mode, drive, 0.2, 0.8);

                let mut want = signal.clone();
                ws.process(&mut want);

                let mut got = spread(&signal);
                ws.process_lanes(&mut got);

                for lane in 0..LANES {
                    assert_eq!(
                        lane_column(&got, lane),
                        want,
                        "{mode:?} drive {drive} lane {lane}"
                    );
                }
            }
        }
    }

    /// REFERENCE. The lane oversampler's round trip is bit-identical to
    /// the scalar one, so its measured transparency and latency carry
    /// over unchanged.
    #[test]
    fn the_lane_oversampler_matches_the_scalar_round_trip() {
        const N: usize = 512;
        let signal = ramp(N);

        let mut sc = Oversampler2x::new();
        let mut up = vec![0.0f32; Oversampler2x::scratch_len(N)];
        let mut want = vec![0.0f32; N];
        sc.up(&signal, &mut up);
        sc.down(&up, &mut want);

        let mut la = LaneOversampler2x::new();
        let mut lane_up = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(N)];
        let mut got = vec![[0.0f32; LANES]; N];
        la.up(&spread(&signal), &mut lane_up);
        la.down(&lane_up, &mut got);

        assert_eq!(la.latency(), sc.latency());
        for lane in 0..LANES {
            assert_eq!(lane_column(&got, lane), want, "oversampler lane {lane}");
        }
    }

    /// LANE INDEPENDENCE. The mandatory sixth test, for the stateful half
    /// — the oversampler's two delay rings.
    #[test]
    fn driving_one_oversampler_lane_leaves_the_others_silent() {
        const N: usize = 256;
        let signal = ramp(N);
        let mut sc = Oversampler2x::new();
        let mut up = vec![0.0f32; Oversampler2x::scratch_len(N)];
        let mut want = vec![0.0f32; N];
        sc.up(&signal, &mut up);
        sc.down(&up, &mut want);

        for lane in 0..LANES {
            let input: Vec<LaneFrame> = signal
                .iter()
                .map(|s| {
                    let mut f = [0.0f32; LANES];
                    f[lane] = *s;
                    f
                })
                .collect();
            let mut la = LaneOversampler2x::new();
            let mut scratch = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(N)];
            let mut got = vec![[0.0f32; LANES]; N];
            la.up(&input, &mut scratch);
            la.down(&scratch, &mut got);
            for other in 0..LANES {
                if other == lane {
                    assert_eq!(lane_column(&got, other), want, "driven lane {lane}");
                } else {
                    assert!(
                        lane_column(&got, other).iter().all(|s| *s == 0.0),
                        "lane {other} rang while only {lane} was driven"
                    );
                }
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact, through the whole chain.
    #[test]
    fn lane_shaping_splits_bit_exactly() {
        const N: usize = 256;
        const CUT: usize = 100;
        let signal = ramp(N);
        let mut ws = Waveshaper::new();
        ws.configure(Mode::SoftClip, 8.0, 0.0, 1.0);

        let run = |cut: Option<usize>| {
            let mut la = LaneOversampler2x::new();
            let mut io = spread(&signal);
            let mut scratch = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(N)];
            match cut {
                None => {
                    la.up(&io, &mut scratch);
                    ws.process_lanes(&mut scratch);
                    la.down(&scratch, &mut io);
                }
                Some(c) => {
                    let (head, tail) = io.split_at_mut(c);
                    let (s_head, s_tail) = scratch.split_at_mut(c * 2);
                    la.up(head, s_head);
                    ws.process_lanes(s_head);
                    la.down(s_head, head);
                    la.up(tail, s_tail);
                    ws.process_lanes(s_tail);
                    la.down(s_tail, tail);
                }
            }
            io
        };
        assert_eq!(run(Some(CUT)), run(None));
    }

    /// NO-ALLOC on the process path.
    #[test]
    fn lane_shaping_does_not_allocate() {
        let mut ws = Waveshaper::new();
        ws.configure(Mode::SoftClip, 6.0, 0.1, 1.0);
        let mut la = LaneOversampler2x::new();
        let mut io = vec![[0.3f32; LANES]; 256];
        let mut scratch = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(256)];
        assert_no_alloc::assert_no_alloc(|| {
            la.up(&io, &mut scratch);
            ws.process_lanes(&mut scratch);
            la.down(&scratch, &mut io);
        });
    }

    /// EDGE LENGTHS: 0, 1 and a non-power-of-two, and mismatched scratch
    /// fails open rather than panicking.
    #[test]
    fn lane_shaping_takes_any_block_length() {
        let ws = Waveshaper::new();
        for n in [0usize, 1, 3, 97] {
            let mut io = vec![[0.5f32; LANES]; n];
            ws.process_lanes(&mut io);
            assert_eq!(io.len(), n);

            let mut la = LaneOversampler2x::new();
            let mut scratch = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(n)];
            la.up(&io, &mut scratch);
            la.down(&scratch, &mut io);
        }
        // Wrong-sized scratch writes nothing at all.
        let mut la = LaneOversampler2x::new();
        let io = vec![[1.0f32; LANES]; 16];
        let mut wrong = vec![[7.0f32; LANES]; 5];
        la.up(&io, &mut wrong);
        assert!(wrong.iter().flatten().all(|s| *s == 7.0));
    }

    /// SILENCE IN, SILENCE OUT, and no invented NaN from a hard drive.
    #[test]
    fn lane_shaping_keeps_silence_silent_and_stays_finite() {
        let mut ws = Waveshaper::new();
        ws.configure(Mode::HardClip, DRIVE_MAX, 0.0, 1.0);
        let mut quiet = vec![[0.0f32; LANES]; 256];
        ws.process_lanes(&mut quiet);
        assert!(quiet.iter().flatten().all(|s| *s == 0.0));

        let mut la = LaneOversampler2x::new();
        let mut scratch = vec![[0.0f32; LANES]; LaneOversampler2x::scratch_len(256)];
        la.up(&quiet, &mut scratch);
        la.down(&scratch, &mut quiet);
        assert!(quiet.iter().flatten().all(|s| *s == 0.0));

        // Extreme input must saturate, never go non-finite.
        let mut hot = vec![[1e6f32; LANES]; 128];
        ws.process_lanes(&mut hot);
        for s in hot.iter().flatten() {
            assert!(s.is_finite(), "hard drive produced {s}");
        }
    }
}
