//! Family 6 of the kernel roadmap: filters.
//!
//! [`OnePole`] (6 dB/octave, the cheapest useful filter) and [`Svf`] (the
//! workhorse: lowpass, highpass, bandpass, notch, peak and allpass from
//! one pair of state variables).
//!
//! # Why these two topologies and not a direct-form biquad
//!
//! Both are **topology-preserving transforms** — the trapezoidal-integrator
//! form, sometimes called zero-delay feedback. Against a direct-form
//! biquad that costs the same per sample, TPT buys three things a DAW
//! actually needs:
//!
//! 1. **It survives modulation.** Direct-form state is a delayed *output*,
//!    so changing coefficients mid-block makes the state mean something it
//!    no longer means, and a fast filter sweep clicks or blows up. TPT
//!    state is integrator charge, which stays meaningful when the
//!    coefficients move. This is the whole reason a filter envelope is
//!    usable.
//! 2. **It is well-conditioned at low cutoffs.** Direct-form biquads lose
//!    precision badly in single precision when the poles crowd z = 1 —
//!    exactly where a 40 Hz highpass lives.
//! 3. **One structure, six outputs.** The SVF's two integrators give every
//!    response at once, so a mode switch costs nothing at runtime.
//!
//! The cost is one `tan` per coefficient update — in the green zone, once
//! per parameter change, never per sample.
//!
//! # Efficiency
//!
//! `process` is a tight scalar loop over a slice: no allocation, no
//! branches inside the loop, no coefficient arithmetic, no bounds checks
//! that cannot be elided (everything iterates, nothing indexes). The mode
//! is chosen ONCE outside the loop and the body is monomorphised per
//! output, so selecting a highpass does not cost a compare per sample.
//!
//! Measured on this machine (release, `report_cost_per_sample`):
//!
//! ```text
//!   one-pole            3.6 ns/sample
//!   SVF (any mode)      4.0-4.4 ns/sample
//!   DC blocker          1.2 ns/sample
//!   cascade 12 dB/oct   3.2 ns/sample
//!   cascade 24 dB/oct   6.2 ns/sample
//!   cascade 48 dB/oct  12.4 ns/sample     — 0.06% of one core at 48 kHz
//! ```
//!
//! A 24 dB/octave lowpass on every one of sixty-four tracks would cost
//! about 2% of a core. The cascade scales linearly with order, and a
//! 12 dB/octave cascade is *cheaper* than a bare SVF call because it runs
//! one stage over the whole block with its state in registers rather than
//! through the mode match each time.
//!
//! An IIR filter cannot be vectorised across samples — `y[n]` depends on
//! `y[n-1]`, and no compiler can undo that. The parallel axis is VOICES,
//! not time, which is why the arena is laid out the way it is. Scalar
//! first, per the contract; wide variants follow profiling.

use crate::dsp::{LANES, LaneFrame};

/// The lowest cutoff a filter will accept, in Hz.
const MIN_HZ: f32 = 1.0;
/// How close to Nyquist a cutoff may sit, as a fraction of the sample
/// rate. `tan(pi*f/fs)` goes to infinity at exactly Nyquist, so the
/// prewarp needs headroom — this is the standard guard.
const MAX_NYQUIST_FRAC: f32 = 0.49;
/// The lowest Q. Below this the SVF's `1/Q` term dominates and the filter
/// is a very wide, very quiet bump nobody asked for.
const MIN_Q: f32 = 0.05;

/// The prewarped integrator gain for a cutoff: `tan(pi * fc / fs)`.
///
/// Green zone — this is the only transcendental in the family, and it is
/// called on parameter change, never per sample.
fn prewarp(sample_rate: f32, cutoff_hz: f32) -> f32 {
    let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        // A nonsense rate must not produce a NaN coefficient that then
        // poisons every sample forever. Answer with something inert.
        return 0.0;
    };
    let nyquist_guard = fs * MAX_NYQUIST_FRAC;
    let fc = if cutoff_hz.is_finite() {
        cutoff_hz.clamp(MIN_HZ, nyquist_guard.max(MIN_HZ))
    } else {
        MIN_HZ
    };
    let g = (core::f32::consts::PI * fc / fs).tan();
    if g.is_finite() { g.max(0.0) } else { 0.0 }
}

// ------------------------------------------------------------- one-pole ---

/// One-pole 6 dB/octave filter, trapezoidal form. Lowpass and highpass
/// from the same state.
///
/// State: 8 bytes. Per-sample cost: 2 mul + 3 add (lowpass); highpass adds
/// 1 sub.
/// Denormal-safe: relies on engine FTZ — a decaying tail passes through
/// the denormal range. Never invents NaN from finite input and settings.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct OnePole {
    /// Integrator state.
    z: f32,
    /// `g / (1 + g)`, precomputed.
    coeff: f32,
}

impl Default for OnePole {
    fn default() -> Self {
        Self::new()
    }
}

impl OnePole {
    pub fn new() -> Self {
        Self { z: 0.0, coeff: 0.0 }
    }

    /// Green zone: set the sample rate and corner frequency.
    pub fn prepare(&mut self, sample_rate: f32, cutoff_hz: f32) {
        let g = prewarp(sample_rate, cutoff_hz);
        self.coeff = g / (1.0 + g);
    }

    /// Green zone: zero the state, keep the coefficient.
    pub fn reset(&mut self) {
        self.z = 0.0;
    }

    /// One sample of the shared integrator. Returns the lowpass output;
    /// the highpass is `x - lp`, which is why both come free.
    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let v = (x - self.z) * self.coeff;
        let lp = v + self.z;
        self.z = lp + v;
        lp
    }

    /// Red zone: one lowpass sample. For kernels that need the pole
    /// INSIDE a per-sample loop — a feedback delay's damping cannot be a
    /// block call, because each sample recirculates before the next
    /// exists. Everything else should use the block form below.
    // TEMPORARY allow: the only caller is delay.rs, which stays
    // undeclared in mod.rs until the post-ADSR tidy pass. Remove the
    // allow when `pub mod delay;` lands.
    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) fn tick_lowpass(&mut self, x: f32) -> f32 {
        self.tick(x)
    }

    /// Red zone: lowpass, in place, any length.
    pub fn process_lowpass(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            *s = self.tick(*s);
        }
    }

    /// Red zone: ONE sample of highpass — what the lowpass did not pass.
    ///
    /// Per sample as well as per block, for the reason
    /// [`RmsDetector::tick`] gives: a FEEDBACK compressor's sidechain
    /// filter sits inside a loop that reads the compressor's own output,
    /// so it has to be advanced one sample at a time and a block-only
    /// door would force the topology to be feedforward.
    ///
    /// [`RmsDetector::tick`]: crate::dsp::dynamics::RmsDetector::tick
    #[inline(always)]
    pub fn tick_highpass(&mut self, x: f32) -> f32 {
        x - self.tick(x)
    }

    /// Red zone: highpass, in place, any length.
    pub fn process_highpass(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            *s = self.tick_highpass(*s);
        }
    }
}

// ------------------------------------------------------------------ SVF ---

/// Which of the state-variable filter's outputs to write.
///
/// Chosen once, outside the sample loop — see [`Svf::process`]. All six
/// come from the same two integrators, so switching mode costs nothing at
/// runtime and nothing in state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lowpass,
    Highpass,
    /// The RAW bandpass, whose peak gain is **Q**, not unity.
    ///
    /// This is the mathematically natural output — the one that makes
    /// `input = highpass + Q⁻¹·bandpass + lowpass` hold exactly, which is
    /// where the notch and peak responses come from. It is also the one
    /// people get wrong: at Q = 0.707 it measures −3 dB at the corner, and
    /// reading that as a bug is how a normalisation gets "fixed" into the
    /// wrong place. Want unity? Say so — [`Mode::BandpassUnity`].
    Bandpass,
    /// The bandpass normalised to unity gain at the corner.
    ///
    /// Constant peak gain: raising Q narrows the band without making it
    /// louder, which is what a display and most callers expect a bandpass
    /// to do.
    BandpassUnity,
    Notch,
    Peak,
    Allpass,
}

/// Two-pole state-variable filter, trapezoidal form (the Simper/Cytomic
/// structure). The workhorse: every second-order response from one pair of
/// integrators, stable under fast modulation.
///
/// State: 8 bytes (two integrators) + 16 bytes of coefficients.
/// Per-sample cost: 5 mul + 6 add for the core, plus 0-2 ops for the
/// output mix depending on mode.
/// Denormal-safe: relies on engine FTZ — integrator charge decays through
/// the denormal range on a fading tail. Never invents NaN from finite
/// input and settings.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    /// `1/Q`, the damping term.
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Default for Svf {
    fn default() -> Self {
        Self::new()
    }
}

impl Svf {
    pub fn new() -> Self {
        let mut svf = Self {
            ic1: 0.0,
            ic2: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        // A filter that has never been prepared should pass signal rather
        // than silence it — an unprepared kernel in a chain is a bug, and
        // silence hides it while a wide-open filter does not.
        svf.prepare(48_000.0, 20_000.0, core::f32::consts::FRAC_1_SQRT_2);
        svf
    }

    /// Green zone: sample rate, cutoff and resonance.
    ///
    /// `q` is a true Q: `1/sqrt(2)` (about 0.707) is Butterworth — maximally
    /// flat, no peak. Higher rings.
    pub fn prepare(&mut self, sample_rate: f32, cutoff_hz: f32, q: f32) {
        let g = prewarp(sample_rate, cutoff_hz);
        let q = if q.is_finite() { q.max(MIN_Q) } else { MIN_Q };
        self.k = 1.0 / q;
        // The single division per parameter change that the whole
        // structure is arranged around: the sample loop then has none.
        let denom = 1.0 + g * (g + self.k);
        let a1 = if denom.abs() > f32::MIN_POSITIVE {
            1.0 / denom
        } else {
            0.0
        };
        self.a1 = a1;
        self.a2 = g * a1;
        self.a3 = g * self.a2;
    }

    /// Green zone: zero the integrators, keep the coefficients.
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// The shared core: one sample in, the three raw outputs out.
    ///
    /// `v1` is the bandpass, `v2` the lowpass; the highpass follows from
    /// them and the input. Everything else is a sum of these.
    #[inline(always)]
    fn tick(&mut self, v0: f32) -> (f32, f32, f32) {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        let hp = v0 - self.k * v1 - v2;
        (v2, v1, hp)
    }

    /// The sample loop, monomorphised per output.
    ///
    /// `mix` is called with `(input, lowpass, bandpass, highpass)` and is
    /// inlined, so the loop that ships has no branch in it — choosing a
    /// highpass does not cost a compare per sample.
    #[inline(always)]
    fn run<F>(&mut self, io: &mut [f32], mix: F)
    where
        F: Fn(f32, f32, f32, f32) -> f32,
    {
        for s in io.iter_mut() {
            let v0 = *s;
            let (lp, bp, hp) = self.tick(v0);
            *s = mix(v0, lp, bp, hp);
        }
    }

    /// Red zone: filter in place, any length.
    ///
    /// The mode is matched ONCE here, outside the loop.
    pub fn process(&mut self, io: &mut [f32], mode: Mode) {
        let k = self.k;
        match mode {
            Mode::Lowpass => self.run(io, |_, lp, _, _| lp),
            Mode::Highpass => self.run(io, |_, _, _, hp| hp),
            Mode::Bandpass => self.run(io, |_, _, bp, _| bp),
            Mode::BandpassUnity => self.run(io, |_, _, bp, _| k * bp),
            // A notch is everything except the band: high plus low.
            Mode::Notch => self.run(io, |_, lp, _, hp| lp + hp),
            // A peak is the band standing proud of the rest.
            Mode::Peak => self.run(io, |_, lp, _, hp| lp - hp),
            // Allpass: flat magnitude, all the phase.
            Mode::Allpass => self.run(io, |_, lp, bp, hp| lp + hp - k * bp),
        }
    }
}

// --------------------------------------------------------------- eq band ---

/// Which gain-bearing shape an [`EqBand`] takes.
///
/// The three an equaliser needs and [`Svf`] cannot make: its outputs are
/// all unity-gain, and a bell or a shelf is defined by the gain it
/// applies. Cuts and notches are NOT here — a cut is [`Cascade`] and a
/// notch is [`Svf::process`] with [`Mode::Notch`], both already unity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandShape {
    /// A symmetric boost or cut around the centre frequency.
    Bell,
    /// Everything below the corner lifted or dropped, flat above.
    LowShelf,
    /// Everything above the corner lifted or dropped, flat below.
    HighShelf,
}

/// One peaking or shelving equaliser section — the same trapezoidal
/// structure as [`Svf`], with the output mix that carries gain.
///
/// The structure is identical; what differs is that `k` absorbs the gain
/// for a bell, `g` absorbs it for a shelf, and the output is a weighted
/// sum of the input and two integrator outputs rather than a bare pick.
/// That is the whole trick: `m0·v0 + m1·bandpass + m2·lowpass` spans
/// every second-order response there is, gain included.
///
/// Everything [`Svf`]'s header says about topology-preserving transforms
/// applies here and is the reason an EQ band built this way survives a
/// swept frequency. A direct-form biquad at 30 Hz in single precision is
/// exactly the case that goes wrong.
///
/// State: 8 bytes (two integrators) + 32 bytes of coefficients.
/// Per-sample cost: 5 mul + 6 add for the core, plus 2 mul + 2 add for
/// the output mix.
/// Denormal-safe: relies on engine FTZ — integrator charge decays through
/// the denormal range on a fading tail. Never invents NaN from finite
/// input and settings.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct EqBand {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    /// The prewarped integrator gain, and `1/Q` after the shape has had
    /// its say. Kept rather than folded away because [`EqBand::coeffs`]
    /// needs them to state this section's transfer function, and a
    /// display deriving the curve from anything else would be a second
    /// opinion about what the filter does.
    g: f32,
    k: f32,
    m0: f32,
    m1: f32,
    m2: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        Self::new()
    }
}

impl EqBand {
    pub fn new() -> Self {
        let mut band = Self {
            ic1: 0.0,
            ic2: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            g: 0.0,
            k: 0.0,
            m0: 1.0,
            m1: 0.0,
            m2: 0.0,
        };
        // A band that has never been prepared passes signal, for the
        // reason `Svf::new` gives: an unprepared kernel in a chain is a
        // bug, and silence hides it while a wire does not.
        band.prepare(
            48_000.0,
            1_000.0,
            core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            BandShape::Bell,
        );
        band
    }

    /// Green zone: sample rate, centre or corner, Q, and the gain in dB.
    ///
    /// `gain_db` is the full boost or cut the band applies — +6 means the
    /// bell peaks at +6 dB and the shelf settles at +6 dB. The internal
    /// `A` is its SQUARE ROOT in linear terms, which is the convention
    /// every cookbook uses and the one that makes a bell's skirt and a
    /// shelf's midpoint land where a musician expects.
    pub fn prepare(&mut self, sample_rate: f32, hz: f32, q: f32, gain_db: f32, shape: BandShape) {
        let db = if gain_db.is_finite() { gain_db } else { 0.0 };
        // `A` is the HALF-gain: the linear gain is `A²`.
        let a = powf10(db / 40.0);
        let q = if q.is_finite() { q.max(MIN_Q) } else { MIN_Q };
        let base = prewarp(sample_rate, hz);
        // The square root that turns a bell's structure into a shelf's:
        // moving the corner by `sqrt(A)` is what makes the transition
        // symmetric about the half-gain point instead of hanging off one
        // end of it.
        let root = a.max(f32::MIN_POSITIVE).sqrt();
        let (g, k, m0, m1, m2) = match shape {
            // The gain rides the DAMPING: a bell is the input with its
            // own bandpass added back, and how much is added is what the
            // boost is.
            BandShape::Bell => {
                let k = 1.0 / (q * a.max(f32::MIN_POSITIVE));
                (base, k, 1.0, k * (a * a - 1.0), 0.0)
            }
            BandShape::LowShelf => {
                let k = 1.0 / q;
                (base / root, k, 1.0, k * (a - 1.0), a * a - 1.0)
            }
            BandShape::HighShelf => {
                let k = 1.0 / q;
                (base * root, k, a * a, k * (1.0 - a) * a, 1.0 - a * a)
            }
        };
        self.g = if g.is_finite() { g.max(0.0) } else { 0.0 };
        self.k = if k.is_finite() {
            k.clamp(0.0, 1.0 / MIN_Q)
        } else {
            1.0
        };
        self.m0 = finite(m0, 1.0);
        self.m1 = finite(m1, 0.0);
        self.m2 = finite(m2, 0.0);
        let g = self.g;
        let denom = 1.0 + g * (g + self.k);
        let a1 = if denom.abs() > f32::MIN_POSITIVE {
            1.0 / denom
        } else {
            0.0
        };
        self.a1 = a1;
        self.a2 = g * a1;
        self.a3 = g * self.a2;
    }

    /// Green zone: zero the integrators, keep the coefficients.
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Latency: none. Stated because the contract requires every kernel
    /// to answer, and because plugin delay compensation reads it.
    pub fn latency(&self) -> usize {
        0
    }

    /// This section as a normalised biquad: `[b0, b1, b2, a1, a2]`, with
    /// `a0` divided out.
    ///
    /// Green zone, and the reason a curve on screen cannot disagree with
    /// the audio: it is derived from the coefficients this instance is
    /// ACTUALLY running, by the bilinear substitution the trapezoidal
    /// integrators already are — not from a parallel analogue formula
    /// that happens to be nearby.
    ///
    /// Substituting `s -> (1/g)·(1-z⁻¹)/(1+z⁻¹)` into the section gives
    /// the denominator `(1+kg+g²) + (2g²-2)z⁻¹ + (1-kg+g²)z⁻²`, with the
    /// bandpass contributing `g(1-z⁻²)` and the lowpass `g²(1+z⁻¹)²`.
    pub fn coeffs(&self) -> [f32; 5] {
        let (g, k) = (self.g, self.k);
        let gg = g * g;
        let a0 = 1.0 + k * g + gg;
        let a1 = 2.0 * gg - 2.0;
        let a2 = 1.0 - k * g + gg;
        let b0 = self.m0 * a0 + self.m1 * g + self.m2 * gg;
        let b1 = self.m0 * a1 + 2.0 * self.m2 * gg;
        let b2 = self.m0 * a2 - self.m1 * g + self.m2 * gg;
        if a0.abs() <= f32::MIN_POSITIVE {
            return [1.0, 0.0, 0.0, 0.0, 0.0];
        }
        let n = 1.0 / a0;
        [b0 * n, b1 * n, b2 * n, a1 * n, a2 * n]
    }

    /// Red zone: filter in place, any length.
    pub fn process(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            let v0 = *s;
            let v3 = v0 - self.ic2;
            let v1 = self.a1 * self.ic1 + self.a2 * v3;
            let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;
            *s = self.m0 * v0 + self.m1 * v1 + self.m2 * v2;
        }
    }
}

/// `10^x` without `f32::powf`'s generality — one `exp2`, and finite for
/// every input a decibel figure can be.
fn powf10(x: f32) -> f32 {
    let y = (x * core::f32::consts::LOG2_10).exp2();
    if y.is_finite() { y.max(0.0) } else { 1.0 }
}

/// `v`, or `fallback` when the arithmetic went somewhere it should not.
fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

// ------------------------------------------------------------ DC blocker ---

/// One-pole DC blocker: a very low highpass that removes the offset a
/// nonlinearity leaves behind, without touching anything audible.
///
/// Separate from [`OnePole`] on purpose. This one is a fixed, extremely
/// low corner used as hygiene rather than as tone, and it uses the
/// classic `y = x - x[n-1] + R·y[n-1]` difference form because at 5 Hz
/// against 48 kHz that is both cheaper and better conditioned than a
/// prewarped trapezoid — the pole sits so close to z = 1 that `tan` is
/// solving a problem that does not exist here.
///
/// State: 8 bytes. Per-sample cost: 1 mul + 2 add.
/// Denormal-safe: relies on engine FTZ — the pole is very close to the
/// unit circle, so a decayed tail lingers in the denormal range longer
/// than most kernels. Never invents NaN from finite input.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct DcBlocker {
    x1: f32,
    y1: f32,
    r: f32,
}

impl Default for DcBlocker {
    fn default() -> Self {
        Self::new()
    }
}

impl DcBlocker {
    /// The corner, in Hz. Low enough to be inaudible, high enough to
    /// settle in a few tens of milliseconds rather than seconds.
    pub const CUTOFF_HZ: f32 = 5.0;

    pub fn new() -> Self {
        let mut dc = Self {
            x1: 0.0,
            y1: 0.0,
            r: 0.0,
        };
        dc.prepare(48_000.0);
        dc
    }

    /// Green zone: set the sample rate.
    pub fn prepare(&mut self, sample_rate: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            self.r = 0.0;
            return;
        };
        // r = 1 - 2*pi*fc/fs, the standard pole placement. Clamped below
        // 1.0 so a silly sample rate cannot put the pole ON the unit
        // circle, where the blocker would become an oscillator.
        let r = 1.0 - core::f32::consts::TAU * Self::CUTOFF_HZ / fs;
        self.r = if r.is_finite() {
            r.clamp(0.0, 0.9999)
        } else {
            0.0
        };
    }

    /// Green zone: zero the state.
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    /// Red zone: remove DC in place, any length.
    pub fn process(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            let x = *s;
            let y = x - self.x1 + self.r * self.y1;
            self.x1 = x;
            self.y1 = y;
            *s = y;
        }
    }
}

// -------------------------------------------------------------- cascade ---

/// The steepest cascade this supports: eight poles, 48 dB/octave.
pub const MAX_STAGES: usize = 4;

/// Butterworth Q for quadratic section `k` of an order-`n` cascade.
///
/// `1 / (2 cos φ)`, where φ is the pole's angle from the negative real
/// axis — and φ is NOT the same expression for even and odd orders. An
/// even order has no real pole, so its pairs sit at `(2k+1)π/2n`; an odd
/// order spends one pole on the real axis and its pairs sit at `(k+1)π/n`.
///
/// Using the even formula throughout is the tidy-looking mistake: it gives
/// a third-order filter Q = 0.577 instead of 1.0, which is 7.8 dB down at
/// its own corner instead of 3. Every textbook table lists 1.0 there.
pub fn butterworth_q(order: u32, k: u32) -> f32 {
    let n = order.max(1) as f32;
    let angle = if order.is_multiple_of(2) {
        (2.0 * k as f32 + 1.0) * core::f32::consts::PI / (2.0 * n)
    } else {
        (k as f32 + 1.0) * core::f32::consts::PI / n
    };
    let c = angle.cos();
    if c.abs() > f32::MIN_POSITIVE {
        1.0 / (2.0 * c)
    } else {
        1.0
    }
}

/// A cascaded Butterworth lowpass or highpass, 6 to 48 dB per octave.
///
/// Up to four [`Svf`] stages plus an optional [`OnePole`] for odd orders,
/// with the textbook Butterworth Q per stage. Resonance, when asked for,
/// goes on the LAST stage only — spreading it across the cascade widens
/// the peak into a hump and moves it off the corner, which is the one
/// thing a resonant filter must not do.
///
/// State: 4 × 40 bytes + 8 bytes + 12 bytes of shape. No allocation:
/// stages are a fixed array and `order` selects how many run.
/// Per-sample cost: `order/2` × the SVF cost, plus the one-pole for odd
/// orders. A 24 dB/octave lowpass is two SVFs — about 10 mul + 12 add.
/// Denormal-safe: inherits the stages'.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Cascade {
    stages: [Svf; MAX_STAGES],
    pole: OnePole,
    /// How many SVF stages actually run.
    biquads: usize,
    /// Whether the one-pole runs (odd orders only).
    has_pole: bool,
    highpass: bool,
}

impl Default for Cascade {
    fn default() -> Self {
        Self::new()
    }
}

impl Cascade {
    pub fn new() -> Self {
        Self {
            stages: [Svf::new(); MAX_STAGES],
            pole: OnePole::new(),
            biquads: 0,
            has_pole: false,
            highpass: false,
        }
    }

    /// Green zone: configure the whole cascade.
    ///
    /// `order` is the number of poles — 1 to 8, so 6 to 48 dB/octave —
    /// and is clamped into range rather than refused, because a caller
    /// asking for a 10-pole filter wants the steepest one available, not
    /// silence. `q` is the resonance for the final stage; `1/sqrt(2)` is
    /// flat Butterworth.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        cutoff_hz: f32,
        q: f32,
        order: u32,
        highpass: bool,
    ) {
        let order = order.clamp(1, MAX_STAGES as u32 * 2);
        self.highpass = highpass;
        self.biquads = (order / 2) as usize;
        self.has_pole = !order.is_multiple_of(2);

        // Iterated, not indexed. `k` is provably in range here and this
        // is the green zone either way — but the contract's rule is that
        // kernels iterate, and one indexed access is how a file stops
        // being obviously correct at a glance.
        for (k, stage) in self.stages.iter_mut().take(self.biquads).enumerate() {
            let base = butterworth_q(order, k as u32);
            // The resonance rides the LAST section. Scaled by how far the
            // caller's q is from flat, so a Butterworth request leaves
            // every textbook Q exactly where it belongs.
            let stage_q = if k + 1 == self.biquads {
                let asked = if q.is_finite() { q.max(MIN_Q) } else { MIN_Q };
                base * (asked / core::f32::consts::FRAC_1_SQRT_2)
            } else {
                base
            };
            stage.prepare(sample_rate, cutoff_hz, stage_q);
        }
        if self.has_pole {
            self.pole.prepare(sample_rate, cutoff_hz);
        }
    }

    /// Green zone: zero every stage's state, keep the coefficients.
    pub fn reset(&mut self) {
        for stage in self.stages.iter_mut() {
            stage.reset();
        }
        self.pole.reset();
    }

    /// The advertised steepness, in dB per octave.
    pub fn db_per_octave(&self) -> f32 {
        (self.biquads * 2 + usize::from(self.has_pole)) as f32 * 6.0
    }

    /// Latency: none. Stated because the contract requires every kernel to
    /// answer, not because a cascade of one-pole integrators could have any.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: run the cascade in place, any length.
    ///
    /// Each stage sweeps the whole block before the next starts, rather
    /// than all stages per sample. Same arithmetic, far better cache
    /// behaviour: one stage's coefficients and state stay in registers
    /// for the length of a block instead of four sets being reloaded
    /// every sample.
    pub fn process(&mut self, io: &mut [f32]) {
        let mode = if self.highpass {
            Mode::Highpass
        } else {
            Mode::Lowpass
        };
        for stage in self.stages.iter_mut().take(self.biquads) {
            stage.process(io, mode);
        }
        if self.has_pole {
            if self.highpass {
                self.pole.process_highpass(io);
            } else {
                self.pole.process_lowpass(io);
            }
        }
    }
}

// ------------------------------------------------------------ disperser ---

/// The most allpass sections a disperser will run.
///
/// Thirty-two second-order sections is 64 poles of phase — well past the
/// point where the smear becomes a distinct pitched "pew" rather than a
/// softened transient, which is what the control is for. Bounded because
/// the contract requires every loop's count to come from a slice length
/// or a compile-time constant, and because a fixed array is what keeps
/// this allocation-free.
pub const DISPERSER_MAX_STAGES: usize = 32;

/// A chain of second-order allpasses: flat magnitude, enormous phase.
///
/// Every section passes every frequency at unity gain and does nothing
/// but delay it — by an amount that depends on frequency. Stack enough of
/// them and a transient stops arriving all at once: the highs come
/// through first and the lows trail, so a click turns into a descending
/// chirp. On a kick that is the "pew", and tuning it to a harmonic of the
/// note is what stops it sounding like a separate laser effect glued to
/// the front of a drum.
///
/// WIRING, NOT NEW ARITHMETIC. Each section is an [`Svf`] in
/// [`Mode::Allpass`], which is already the tested workhorse; this type
/// owns how many there are, how they are tuned, and nothing else. That is
/// the kernel contract's whole thesis — a new effect should be a new
/// arrangement of loops that already exist.
///
/// `q` sets how tightly the phase turns around the corner. Low Q spreads
/// the group delay over octaves and reads as a soft smear; high Q packs
/// it into a narrow band and reads as a ringing pitch. Neither changes
/// the magnitude response, which stays flat to within float error at
/// every setting — that is what makes this safe to put across a drum.
///
/// State: 32 × 40 bytes. No allocation: the sections are a fixed array
/// and `count` selects how many run.
/// Per-sample cost: `count` × the SVF cost — 8 sections is about
/// 40 multiplies and 48 adds.
/// Denormal-safe: inherits [`Svf`]'s; the integrators decay through the
/// denormal range on a fading tail and rely on engine FTZ.
/// In-place safe: yes.
/// Latency: 0 samples. The delay is dispersive, not a fixed offset, so
/// there is nothing for compensation to subtract — an impulse's ENERGY
/// spreads later in time but its onset does not move.
#[derive(Debug, Clone, Copy)]
pub struct Disperser {
    sections: [Svf; DISPERSER_MAX_STAGES],
    count: usize,
}

impl Default for Disperser {
    fn default() -> Self {
        Self::new()
    }
}

impl Disperser {
    pub fn new() -> Self {
        Self {
            sections: [Svf::new(); DISPERSER_MAX_STAGES],
            count: 0,
        }
    }

    /// Tune every running section to the same corner.
    ///
    /// `stages` is clamped rather than refused, the way [`Cascade`] treats
    /// its order: a caller asking for more smear than exists wants the
    /// most available, not silence. Zero stages is legal and is a wire.
    ///
    /// Every section shares one corner and one Q on purpose. Staggering
    /// them would widen the affected band, which sounds like a filter
    /// sweep; stacking them at one frequency multiplies the phase turn at
    /// that frequency, which is the effect being asked for.
    ///
    /// ONE TRANSCENDENTAL, whatever the stage count. The sections are
    /// identical by construction, so the coefficients are computed once
    /// and copied — thirty-two `tan` calls for one corner would be
    /// thirty-one of them computing a number that is already known. That
    /// is what makes this cheap enough to re-tune from the audio thread
    /// when a knob moves or a note arrives, which is the same thing the
    /// Filter node already does with its cascade.
    ///
    /// The STATE is deliberately left alone: only the coefficients are
    /// copied. Clearing the integrators on a re-tune would click on every
    /// note of a tuned disperser, which is exactly the case this exists
    /// for.
    pub fn prepare(&mut self, sample_rate: f32, hz: f32, q: f32, stages: u32) {
        self.count = (stages as usize).min(DISPERSER_MAX_STAGES);
        let q = if q.is_finite() { q.max(MIN_Q) } else { MIN_Q };
        let mut tuned = Svf::new();
        tuned.prepare(sample_rate, hz, q);
        for section in self.sections.iter_mut().take(self.count) {
            section.k = tuned.k;
            section.a1 = tuned.a1;
            section.a2 = tuned.a2;
            section.a3 = tuned.a3;
        }
    }

    /// Green zone: zero every section's state, keep the tuning.
    pub fn reset(&mut self) {
        for section in self.sections.iter_mut() {
            section.reset();
        }
    }

    /// How many sections are running.
    pub fn stages(&self) -> usize {
        self.count
    }

    /// Latency: none, and this one is worth stating plainly because the
    /// effect IS a delay. It is a FREQUENCY-DEPENDENT delay with no
    /// common offset to remove, so plugin delay compensation has nothing
    /// to compensate.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: run the chain in place, any length.
    ///
    /// Section by section over the whole block rather than all sections
    /// per sample — the same choice [`Cascade`] makes, and for the same
    /// reason: one section's coefficients and state stay in registers for
    /// a block instead of thirty-two sets being reloaded every sample.
    pub fn process(&mut self, io: &mut [f32]) {
        for section in self.sections.iter_mut().take(self.count) {
            section.process(io, Mode::Allpass);
        }
    }
}

// ----------------------------------------------------------------- tilt ---

/// Tilt filter: a see-saw around a pivot — highs up and lows down by the
/// same amount (or the reverse), unity AT the pivot.
///
/// Built on the [`OnePole`]'s exact complementary split: with
/// `lp + hp = x`, the tilt is `g_lo·lp + g_hi·hp`, folded to
/// `g_hi·x + (g_lo − g_hi)·lp` so it costs the one-pole plus two
/// multiplies. First order, so the transition is a gentle 6 dB/octave —
/// which is the sound tilt EQs are liked for.
///
/// The corner is NOT the pivot: a first-order see-saw's unity crossing
/// sits at `corner / g_hi` (solve `|H(jw)|² = 1` with reciprocal gains),
/// so `prepare` places the corner at `pivot × g_hi` and the crossing
/// lands where the user pointed, at every tilt amount. Skipping that
/// correction slides the pivot by an octave at ±6 dB — audible, and the
/// kind of drift a magnitude test catches and an ear blames on the
/// material.
///
/// State: 16 bytes. Per-sample cost: 4 mul + 4 add.
/// Denormal-safe: relies on engine FTZ, as [`OnePole`].
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Tilt {
    pole: OnePole,
    g_hi: f32,
    /// `g_lo − g_hi`, precomputed.
    g_delta: f32,
}

/// The steepest tilt accepted, in dB. Past this a first-order see-saw is
/// the wrong tool and a shelf pair is the honest one.
pub const TILT_MAX_DB: f32 = 24.0;

impl Default for Tilt {
    fn default() -> Self {
        Self::new()
    }
}

impl Tilt {
    pub fn new() -> Self {
        Self {
            pole: OnePole::new(),
            g_hi: 1.0,
            g_delta: 0.0,
        }
    }

    /// Green zone: sample rate, pivot frequency, and tilt in dB — the
    /// gain reached at the HIGH extreme (+6 tilts bright, −6 tilts dark;
    /// the low extreme mirrors it). Nonsense flattens to zero tilt
    /// rather than poisoning the coefficients.
    pub fn prepare(&mut self, sample_rate: f32, pivot_hz: f32, tilt_db: f32) {
        let tilt = if tilt_db.is_finite() {
            tilt_db.clamp(-TILT_MAX_DB, TILT_MAX_DB)
        } else {
            0.0
        };
        let g_hi = 10.0f32.powf(tilt / 20.0);
        let g_lo = 1.0 / g_hi;
        self.g_hi = g_hi;
        self.g_delta = g_lo - g_hi;
        // Corner at pivot·g_hi keeps the unity crossing at the pivot;
        // prewarp's own clamps absorb whatever this pushes past Nyquist.
        let pivot = if pivot_hz.is_finite() {
            pivot_hz
        } else {
            1_000.0
        };
        self.pole.prepare(sample_rate, pivot * g_hi);
    }

    /// Green zone: zero the state, keep the coefficients.
    pub fn reset(&mut self) {
        self.pole.reset();
    }

    /// Red zone: tilt in place, any length.
    pub fn process(&mut self, io: &mut [f32]) {
        let (g_hi, g_delta) = (self.g_hi, self.g_delta);
        for s in io.iter_mut() {
            let x = *s;
            let lp = self.pole.tick(x);
            *s = g_hi * x + g_delta * lp;
        }
    }
}

// ------------------------------------------------------------- lanes ---

/// [`OnePole`] for a whole voice group. Coefficient shared, state per lane.
///
/// State: 32 bytes + 4. Per-sample-per-lane cost: the scalar's.
/// Denormal-safe: relies on engine FTZ. In-place safe: yes. Latency: 0.
#[derive(Debug, Clone, Copy, Default)]
pub struct LaneOnePole {
    z: [f32; LANES],
    coeff: [f32; LANES],
}

impl LaneOnePole {
    pub fn new() -> Self {
        let mut p = Self {
            z: [0.0; LANES],
            coeff: [0.0; LANES],
        };
        p.prepare(48_000.0, 20_000.0);
        p
    }

    /// Green zone. The coefficient is taken from a prepared SCALAR
    /// [`OnePole`] rather than recomputed, so the two kernels cannot
    /// drift apart in their tuning — there is only one prewarp in the
    /// file, and this is not a second copy of it.
    pub fn prepare(&mut self, sample_rate: f32, cutoff_hz: f32) {
        self.prepare_lanes(sample_rate, &[cutoff_hz; LANES]);
    }

    /// Green zone: a DIFFERENT corner per lane — what keytrack and a
    /// per-voice filter envelope need.
    ///
    /// One scalar `OnePole::prepare` per lane, and its coefficient copied
    /// across. Eight prewarps rather than one, which at control-chunk
    /// rate is a rounding error against the per-sample cost, and it keeps
    /// the promise that there is exactly one prewarp in this file.
    pub fn prepare_lanes(&mut self, sample_rate: f32, cutoff_hz: &[f32; LANES]) {
        for (dst, hz) in self.coeff.iter_mut().zip(cutoff_hz.iter()) {
            let mut p = OnePole::new();
            p.prepare(sample_rate, *hz);
            *dst = p.coeff;
        }
    }

    pub fn reset(&mut self) {
        self.z = [0.0; LANES];
    }

    pub fn reset_lane(&mut self, lane: usize) {
        if let Some(z) = self.z.get_mut(lane) {
            *z = 0.0;
        }
    }

    /// Red zone: lowpass in place, any length.
    pub fn process_lowpass(&mut self, io: &mut [LaneFrame]) {
        let coeff = self.coeff;
        for frame in io.iter_mut() {
            for ((s, z), c) in frame.iter_mut().zip(self.z.iter_mut()).zip(coeff.iter()) {
                let v = (*s - *z) * *c;
                let lp = v + *z;
                *z = lp + v;
                *s = lp;
            }
        }
    }

    /// Red zone: highpass in place, any length.
    pub fn process_highpass(&mut self, io: &mut [LaneFrame]) {
        let coeff = self.coeff;
        for frame in io.iter_mut() {
            for ((s, z), c) in frame.iter_mut().zip(self.z.iter_mut()).zip(coeff.iter()) {
                let x = *s;
                let v = (x - *z) * *c;
                let lp = v + *z;
                *z = lp + v;
                *s = x - lp;
            }
        }
    }
}

/// [`Svf`] for a whole voice group: coefficients shared, the two
/// integrator states per lane.
///
/// This is the family that needs lanes most. A stateful filter cannot
/// vectorize across TIME — sample n+1 depends on sample n — so voices are
/// the only axis it has, and eight of them is one register's worth of
/// integrator.
///
/// Per-lane CUTOFF is deliberately not here yet: keytrack and the filter
/// envelope want it, and it turns the shared `a1/a2/a3` into three more
/// lane arrays plus a per-lane prewarp. Shared coefficients first, and
/// the node runs keytrack at 0 % until the per-lane variant lands. See
/// `notes/20260825-poly-kernel-commission.md`.
///
/// State: 64 bytes + 16 of shape.
/// Per-sample-per-lane cost: the scalar's — 4 mul + 6 add.
/// Denormal-safe: relies on engine FTZ. In-place safe: yes. Latency: 0.
#[derive(Debug, Clone, Copy)]
pub struct LaneSvf {
    ic1: [f32; LANES],
    ic2: [f32; LANES],
    k: [f32; LANES],
    a1: [f32; LANES],
    a2: [f32; LANES],
    a3: [f32; LANES],
}

impl Default for LaneSvf {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneSvf {
    pub fn new() -> Self {
        let mut f = Self {
            ic1: [0.0; LANES],
            ic2: [0.0; LANES],
            k: [0.0; LANES],
            a1: [0.0; LANES],
            a2: [0.0; LANES],
            a3: [0.0; LANES],
        };
        f.prepare(48_000.0, 20_000.0, core::f32::consts::FRAC_1_SQRT_2);
        f
    }

    /// Green zone: sample rate, cutoff and resonance — the scalar
    /// [`Svf`]'s exact meaning, because the coefficients ARE the scalar
    /// kernel's, copied from a prepared instance. One coefficient
    /// derivation in this file, used twice.
    pub fn prepare(&mut self, sample_rate: f32, cutoff_hz: f32, q: f32) {
        let mut f = Svf::new();
        f.prepare(sample_rate, cutoff_hz, q);
        self.set_coeffs(&f);
    }

    /// Green zone: a DIFFERENT cutoff per lane.
    ///
    /// This is what a per-voice filter envelope and keytrack need: two
    /// voices in the same group are at different points in their
    /// envelopes and on different keys, so they cannot share a corner.
    /// Resonance stays shared — it is a patch setting, not a per-voice
    /// one.
    pub fn prepare_lanes(&mut self, sample_rate: f32, cutoff_hz: &[f32; LANES], q: f32) {
        for lane in 0..LANES {
            let mut f = Svf::new();
            f.prepare(sample_rate, cutoff_hz.get(lane).copied().unwrap_or(0.0), q);
            self.set_lane_coeffs(lane, &f);
        }
    }

    /// Adopt a prepared scalar filter's coefficients for EVERY lane,
    /// leaving state alone.
    fn set_coeffs(&mut self, from: &Svf) {
        self.k = [from.k; LANES];
        self.a1 = [from.a1; LANES];
        self.a2 = [from.a2; LANES];
        self.a3 = [from.a3; LANES];
    }

    /// Adopt a prepared scalar filter's coefficients for ONE lane.
    fn set_lane_coeffs(&mut self, lane: usize, from: &Svf) {
        if let (Some(k), Some(a1), Some(a2), Some(a3)) = (
            self.k.get_mut(lane),
            self.a1.get_mut(lane),
            self.a2.get_mut(lane),
            self.a3.get_mut(lane),
        ) {
            *k = from.k;
            *a1 = from.a1;
            *a2 = from.a2;
            *a3 = from.a3;
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = [0.0; LANES];
        self.ic2 = [0.0; LANES];
    }

    /// Green zone: zero ONE lane's integrators — stealing a voice must
    /// not drag the previous note's filter state into the new one.
    pub fn reset_lane(&mut self, lane: usize) {
        if let (Some(a), Some(b)) = (self.ic1.get_mut(lane), self.ic2.get_mut(lane)) {
            *a = 0.0;
            *b = 0.0;
        }
    }

    /// Red zone: filter in place, any length. The mode is matched ONCE,
    /// outside the loop, exactly as the scalar kernel does it.
    pub fn process(&mut self, io: &mut [LaneFrame], mode: Mode) {
        // `k` reaches the mixer as an argument rather than a capture,
        // because it is per lane now.
        match mode {
            Mode::Lowpass => self.run(io, |_, lp, _, _, _| lp),
            Mode::Highpass => self.run(io, |_, _, _, hp, _| hp),
            Mode::Bandpass => self.run(io, |_, _, bp, _, _| bp),
            Mode::BandpassUnity => self.run(io, |_, _, bp, _, k| k * bp),
            Mode::Notch => self.run(io, |_, lp, _, hp, _| lp + hp),
            Mode::Peak => self.run(io, |_, lp, _, hp, _| lp - hp),
            Mode::Allpass => self.run(io, |_, lp, bp, hp, k| lp + hp - k * bp),
        }
    }

    fn run(&mut self, io: &mut [LaneFrame], mix: impl Fn(f32, f32, f32, f32, f32) -> f32) {
        let (k, a1, a2, a3) = (self.k, self.a1, self.a2, self.a3);
        for frame in io.iter_mut() {
            let lanes = frame
                .iter_mut()
                .zip(self.ic1.iter_mut())
                .zip(self.ic2.iter_mut())
                .zip(k.iter())
                .zip(a1.iter())
                .zip(a2.iter())
                .zip(a3.iter());
            for ((((((s, ic1), ic2), k), a1), a2), a3) in lanes {
                let v0 = *s;
                let v3 = v0 - *ic2;
                let v1 = *a1 * *ic1 + *a2 * v3;
                let v2 = *ic2 + *a2 * *ic1 + *a3 * v3;
                *ic1 = 2.0 * v1 - *ic1;
                *ic2 = 2.0 * v2 - *ic2;
                let hp = v0 - *k * v1 - v2;
                *s = mix(v0, v2, v1, hp, *k);
            }
        }
    }
}

/// [`Cascade`] for a whole voice group: 6 to 48 dB per octave across
/// [`LANES`] voices.
///
/// The SHAPE — how many biquads, whether a one-pole runs, the Butterworth
/// Q per stage and where the resonance rides — is not restated here. It
/// is taken from a prepared scalar [`Cascade`], so the two kernels agree
/// on every stage by construction rather than by a test noticing later.
///
/// State: 4 × 80 bytes + 36.
/// Per-sample-per-lane cost: the scalar's.
/// Denormal-safe: inherits the stages'. In-place safe: yes. Latency: 0.
#[derive(Debug, Clone, Copy)]
pub struct LaneCascade {
    stages: [LaneSvf; MAX_STAGES],
    pole: LaneOnePole,
    biquads: usize,
    has_pole: bool,
    highpass: bool,
}

impl Default for LaneCascade {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneCascade {
    pub fn new() -> Self {
        Self {
            stages: [LaneSvf::new(); MAX_STAGES],
            pole: LaneOnePole::new(),
            biquads: 0,
            has_pole: false,
            highpass: false,
        }
    }

    /// Green zone: configure the whole cascade. Same argument meaning and
    /// same clamping as [`Cascade::prepare`], because it IS that function
    /// — this only copies the result across.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        cutoff_hz: f32,
        q: f32,
        order: u32,
        highpass: bool,
    ) {
        let mut c = Cascade::new();
        c.prepare(sample_rate, cutoff_hz, q, order, highpass);
        self.biquads = c.biquads;
        self.has_pole = c.has_pole;
        self.highpass = c.highpass;
        for (dst, src) in self.stages.iter_mut().zip(c.stages.iter()) {
            dst.set_coeffs(src);
        }
        self.pole.coeff = [c.pole.coeff; LANES];
    }

    /// Green zone: a DIFFERENT cutoff per lane, same shape for all.
    ///
    /// One scalar cascade prepared per lane, each lane taking its own
    /// stage coefficients out of it. The SHAPE — stage count, Butterworth
    /// Qs, where the resonance rides — is identical across lanes because
    /// it comes from the same arguments; only the corner moves.
    pub fn prepare_lanes(
        &mut self,
        sample_rate: f32,
        cutoff_hz: &[f32; LANES],
        q: f32,
        order: u32,
        highpass: bool,
    ) {
        for lane in 0..LANES {
            let mut c = Cascade::new();
            c.prepare(
                sample_rate,
                cutoff_hz.get(lane).copied().unwrap_or(0.0),
                q,
                order,
                highpass,
            );
            self.biquads = c.biquads;
            self.has_pole = c.has_pole;
            self.highpass = c.highpass;
            for (dst, src) in self.stages.iter_mut().zip(c.stages.iter()) {
                dst.set_lane_coeffs(lane, src);
            }
            if let Some(co) = self.pole.coeff.get_mut(lane) {
                *co = c.pole.coeff;
            }
        }
    }

    /// Green zone: zero every stage's state in every lane.
    pub fn reset(&mut self) {
        for stage in self.stages.iter_mut() {
            stage.reset();
        }
        self.pole.reset();
    }

    /// Green zone: zero ONE lane through the whole cascade.
    pub fn reset_lane(&mut self, lane: usize) {
        for stage in self.stages.iter_mut() {
            stage.reset_lane(lane);
        }
        self.pole.reset_lane(lane);
    }

    /// The advertised steepness, in dB per octave.
    pub fn db_per_octave(&self) -> f32 {
        (self.biquads * 2 + usize::from(self.has_pole)) as f32 * 6.0
    }

    /// Latency: none.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: run the cascade in place, any length. Stage-sweeps-block,
    /// like the scalar kernel, for the same cache reason.
    pub fn process(&mut self, io: &mut [LaneFrame]) {
        let mode = if self.highpass {
            Mode::Highpass
        } else {
            Mode::Lowpass
        };
        for stage in self.stages.iter_mut().take(self.biquads) {
            stage.process(io, mode);
        }
        if self.has_pole {
            if self.highpass {
                self.pole.process_highpass(io);
            } else {
                self.pole.process_lowpass(io);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;
    const FLAT_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

    /// Measure a filter's magnitude at one frequency by running a sine
    /// through it and taking the RMS of the settled tail.
    ///
    /// Analytic where it can be, measured where it cannot: this is the
    /// filter actually running, not its coefficients restated.
    fn magnitude(run: &mut impl FnMut(&mut [f32]), hz: f32) -> f32 {
        // Long enough for the transient to leave: a low-Q filter settles
        // in a few dozen cycles, and the tail is what is measured.
        let n = 16_384;
        let mut buf: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
            .collect();
        run(&mut buf);
        let tail = &buf[n / 2..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        // A unit sine has RMS 1/sqrt(2); report gain relative to that.
        rms * core::f32::consts::SQRT_2
    }

    fn db(gain: f32) -> f32 {
        20.0 * gain.max(1e-9).log10()
    }

    // -------------------------------------------------------- reference ---

    /// A one-pole is 3 dB down at its corner and falls at 6 dB/octave.
    /// The two numbers every filter table agrees on.
    #[test]
    fn one_pole_is_three_db_down_and_falls_six_per_octave() {
        let corner = 1_000.0;
        let mut f = OnePole::new();
        let at = |hz: f32| {
            let mut f2 = OnePole::new();
            f2.prepare(FS, corner);
            db(magnitude(&mut |b| f2.process_lowpass(b), hz))
        };
        f.prepare(FS, corner);
        assert!((at(corner) + 3.0).abs() < 0.4, "corner: {}", at(corner));
        assert!(at(100.0).abs() < 0.2, "passband is flat");
        // An octave apart, past the corner but well BELOW Nyquist. A
        // trapezoidal filter's response steepens toward Nyquist and
        // reaches zero exactly there — that is correct behaviour for the
        // digital filter, not a 6 dB/octave asymptote, so measuring at
        // 16 kHz reads about 9.5 dB/octave and says nothing.
        let slope = at(2_000.0) - at(4_000.0);
        assert!((slope - 6.0).abs() < 0.7, "measured {slope:.2} dB/oct");
    }

    /// The highpass is the complement: what the lowpass did not pass.
    #[test]
    fn one_pole_highpass_mirrors_the_lowpass() {
        let corner = 1_000.0;
        let mut hp = OnePole::new();
        hp.prepare(FS, corner);
        let at = |hz: f32| {
            let mut f = OnePole::new();
            f.prepare(FS, corner);
            db(magnitude(&mut |b| f.process_highpass(b), hz))
        };
        assert!((at(corner) + 3.0).abs() < 0.4, "corner: {}", at(corner));
        assert!(at(16_000.0).abs() < 0.3, "passes the top");
        assert!(at(100.0) < -18.0, "blocks the bottom");
        let _ = &mut hp;
    }

    /// The SVF's four main responses each do what their name says, and a
    /// flat Q is 3 dB down at the corner whichever way it points.
    #[test]
    fn svf_modes_do_what_their_names_say() {
        let corner = 1_000.0;
        let at = |mode: Mode, hz: f32, q: f32| {
            let mut f = Svf::new();
            f.prepare(FS, corner, q);
            db(magnitude(&mut |b| f.process(b, mode), hz))
        };

        // Lowpass: flat below, gone above, -3 at the corner.
        assert!(at(Mode::Lowpass, 100.0, FLAT_Q).abs() < 0.2);
        assert!((at(Mode::Lowpass, corner, FLAT_Q) + 3.0).abs() < 0.5);
        assert!(at(Mode::Lowpass, 8_000.0, FLAT_Q) < -30.0);

        // Highpass: the mirror.
        assert!(at(Mode::Highpass, 16_000.0, FLAT_Q).abs() < 0.4);
        assert!((at(Mode::Highpass, corner, FLAT_Q) + 3.0).abs() < 0.5);
        assert!(at(Mode::Highpass, 100.0, FLAT_Q) < -30.0);

        // Bandpass peaks AT the corner and falls both ways.
        assert!(at(Mode::Bandpass, 100.0, FLAT_Q) < -15.0);
        assert!(at(Mode::Bandpass, 10_000.0, FLAT_Q) < -15.0);

        // Notch: a hole at the corner, flat well away from it.
        assert!(at(Mode::Notch, corner, 4.0) < -20.0, "a hole");
        assert!(at(Mode::Notch, 100.0, 4.0).abs() < 0.5);
        assert!(at(Mode::Notch, 16_000.0, 4.0).abs() < 0.5);
    }

    /// The two bandpass conventions differ by exactly Q, and both are
    /// deliberate. The raw one measures Q at the corner — at Butterworth
    /// that is −3 dB, which looks like a bug and is not — and the unity
    /// one measures 0 dB whatever the Q.
    #[test]
    fn the_two_bandpasses_differ_by_exactly_q() {
        let corner = 1_000.0;
        let at = |mode: Mode, q: f32| {
            let mut f = Svf::new();
            f.prepare(FS, corner, q);
            db(magnitude(&mut |b| f.process(b, mode), corner))
        };
        for q in [FLAT_Q, 1.0, 4.0, 10.0] {
            let raw = at(Mode::Bandpass, q);
            let unity = at(Mode::BandpassUnity, q);
            assert!(
                (unity).abs() < 0.5,
                "unity bandpass at Q {q} was {unity:.2} dB"
            );
            assert!(
                (raw - unity - db(q)).abs() < 0.5,
                "raw should exceed unity by Q ({:.2} dB) at Q {q}: {raw:.2} vs {unity:.2}",
                db(q)
            );
        }
    }

    /// An allpass is FLAT — all of its work is in the phase. The one
    /// response that is wrong if it does anything visible to magnitude.
    #[test]
    fn svf_allpass_is_flat() {
        for hz in [100.0, 500.0, 1_000.0, 4_000.0, 12_000.0] {
            let mut f = Svf::new();
            f.prepare(FS, 1_000.0, FLAT_Q);
            let g = db(magnitude(&mut |b| f.process(b, Mode::Allpass), hz));
            assert!(g.abs() < 0.5, "allpass at {hz} Hz was {g:.2} dB, not flat");
        }
    }

    /// Resonance lifts the corner, and more of it lifts it further.
    #[test]
    fn svf_resonance_peaks_at_the_corner() {
        let corner = 1_000.0;
        let at = |q: f32| {
            let mut f = Svf::new();
            f.prepare(FS, corner, q);
            db(magnitude(&mut |b| f.process(b, Mode::Lowpass), corner))
        };
        let (flat, some, lots) = (at(FLAT_Q), at(2.0), at(8.0));
        assert!(some > flat + 3.0, "{some} should exceed {flat}");
        assert!(lots > some + 6.0, "{lots} should exceed {some}");
    }

    /// A DC blocker removes the offset and leaves the music.
    #[test]
    fn the_dc_blocker_removes_offset_and_keeps_audio() {
        let mut dc = DcBlocker::new();
        dc.prepare(FS);
        // A second of pure DC: the output must settle to nothing.
        let mut buf = vec![0.7f32; 48_000];
        dc.process(&mut buf);
        let settled = &buf[40_000..];
        assert!(
            settled.iter().all(|s| s.abs() < 0.01),
            "DC should be gone: {}",
            settled[0]
        );

        // And a tone well above the corner passes essentially untouched.
        let mut dc = DcBlocker::new();
        dc.prepare(FS);
        let g = magnitude(&mut |b| dc.process(b), 200.0);
        assert!((db(g)).abs() < 0.3, "200 Hz should pass: {:.2} dB", db(g));
    }

    /// What a block actually costs, in nanoseconds per sample.
    ///
    /// Printed, not asserted: a timing threshold in a test suite fails on
    /// a loaded CI box and tells you nothing. This is here so the number
    /// can be looked at deliberately.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let mut buf = vec![0.25f32; BLOCK];

        let mut row = |name: &str, run: &mut dyn FnMut(&mut [f32])| {
            // Warm the caches and the branch predictor first.
            for _ in 0..1_000 {
                run(&mut buf);
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run(&mut buf);
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            // 48 kHz of one voice, as a share of one core.
            println!(
                "{name:<28} {ns:6.2} ns/sample   {:5.3}% of a core at 48k",
                ns * 48_000.0 * 1e-9 * 100.0
            );
        };

        let mut pole = OnePole::new();
        pole.prepare(FS, 1_000.0);
        row("one-pole lowpass", &mut |b| pole.process_lowpass(b));

        let mut svf = Svf::new();
        svf.prepare(FS, 1_000.0, 2.0);
        row("svf lowpass", &mut |b| svf.process(b, Mode::Lowpass));
        row("svf allpass", &mut |b| svf.process(b, Mode::Allpass));

        let mut dc = DcBlocker::new();
        dc.prepare(FS);
        row("dc blocker", &mut |b| dc.process(b));

        let mut tilt = Tilt::new();
        tilt.prepare(FS, 1_000.0, 6.0);
        row("tilt", &mut |b| tilt.process(b));

        for order in [2u32, 4, 8] {
            let mut c = Cascade::new();
            c.prepare(FS, 1_000.0, FLAT_Q, order, false);
            row(&format!("cascade {} dB/oct", order * 6), &mut |b| {
                c.process(b)
            });
        }
    }

    /// The measurement floor.
    ///
    /// Each SVF stage adds its own f32 arithmetic noise, so a cascade of
    /// four measures about -83 dB of "output" for an input that is
    /// analytically -207 dB down. Every reference test stays above this;
    /// below it a test is measuring the noise floor and calling it a
    /// filter.
    const FLOOR_DB: f32 = -60.0;

    /// The analytic Butterworth lowpass magnitude: `1/sqrt(1 + (f/fc)^2n)`.
    fn butterworth_db(order: u32, ratio: f32) -> f32 {
        db(1.0 / (1.0 + ratio.powi(2 * order as i32)).sqrt())
    }

    /// Every order matches the textbook Butterworth magnitude across the
    /// corner.
    ///
    /// Stated against the ANALYTIC response rather than an asymptotic
    /// slope, because an asymptote is only true far from the corner and
    /// "far" for eight poles is already past the measurement floor. This
    /// checks the whole shape instead of one number, and it is what
    /// catches a wrong Q per stage.
    #[test]
    fn every_order_matches_the_textbook_butterworth() {
        let corner = 500.0;
        for order in 1..=8u32 {
            for ratio in [0.25f32, 0.5, 1.0, 2.0] {
                let want = butterworth_db(order, ratio);
                if want < FLOOR_DB {
                    continue;
                }
                let mut c = Cascade::new();
                c.prepare(FS, corner, FLAT_Q, order, false);
                let got = db(magnitude(&mut |b| c.process(b), corner * ratio));
                assert!(
                    (got - want).abs() < 1.2,
                    "order {order} at {ratio}x corner: {got:.2} dB, textbook {want:.2} dB"
                );
            }
        }
    }

    /// And the advertised dB/octave is the real one, measured on the
    /// filter running.
    ///
    /// Low orders only: an octave of a 48 dB/octave filter starting far
    /// enough out to be asymptotic already ends below the measurement
    /// floor, so the high orders are covered by the shape test above
    /// rather than by a number that could only be measured against noise.
    #[test]
    fn the_advertised_slope_is_the_real_one() {
        let corner = 200.0;
        for order in 1..=4u32 {
            let at = |hz: f32| {
                let mut c = Cascade::new();
                c.prepare(FS, corner, FLAT_Q, order, false);
                db(magnitude(&mut |b| c.process(b), hz))
            };
            let mut c = Cascade::new();
            c.prepare(FS, corner, FLAT_Q, order, false);
            let want = c.db_per_octave();
            assert!((want - order as f32 * 6.0).abs() < 0.01);
            let slope = at(corner * 2.0) - at(corner * 4.0);
            assert!(
                (slope - want).abs() < 1.2,
                "order {order} claims {want} dB/oct, measured {slope:.2}"
            );
        }
    }

    /// A flat cascade is 3 dB down at its corner whatever the order — the
    /// number every filter table agrees on, and the one that catches a
    /// wrong Butterworth Q. An odd order using the even formula reads
    /// about 7.8 dB down instead.
    #[test]
    fn a_flat_cascade_is_three_db_down_at_its_corner() {
        for order in 1..=8u32 {
            let mut c = Cascade::new();
            c.prepare(FS, 1_000.0, FLAT_Q, order, false);
            let at_corner = db(magnitude(&mut |b| c.process(b), 1_000.0));
            assert!(
                (at_corner + 3.0).abs() < 0.8,
                "order {order} is {at_corner:.2} dB at its corner, not -3"
            );
        }
        // The third-order Q specifically, since that is the one the tidy
        // formula gets wrong.
        assert!(
            (butterworth_q(3, 0) - 1.0).abs() < 1e-4,
            "3rd order Q is 1.0"
        );
        assert!(
            (butterworth_q(2, 0) - FLAT_Q).abs() < 1e-4,
            "2nd order Q is Butterworth"
        );
    }

    /// A highpass cascade is the mirror image.
    #[test]
    fn a_highpass_cascade_blocks_below_and_passes_above() {
        let mut c = Cascade::new();
        c.prepare(FS, 1_000.0, FLAT_Q, 4, true);
        let at = |hz: f32| {
            let mut c = Cascade::new();
            c.prepare(FS, 1_000.0, FLAT_Q, 4, true);
            db(magnitude(&mut |b| c.process(b), hz))
        };
        assert!(at(12_000.0).abs() < 0.6, "passes the top: {}", at(12_000.0));
        assert!((at(1_000.0) + 3.0).abs() < 0.8, "-3 at the corner");
        assert!(at(125.0) < -40.0, "three octaves down is gone");
        let _ = &mut c;
    }

    /// Resonance rides the LAST stage, so the peak sits AT the corner
    /// rather than smearing into a hump beside it.
    #[test]
    fn cascade_resonance_peaks_at_the_corner() {
        let corner = 1_000.0;
        let at = |hz: f32, q: f32| {
            let mut c = Cascade::new();
            c.prepare(FS, corner, q, 4, false);
            db(magnitude(&mut |b| c.process(b), hz))
        };
        let peak = at(corner, 8.0);
        assert!(
            peak > at(corner, FLAT_Q) + 6.0,
            "resonance lifts the corner"
        );
        for away in [0.5f32, 2.0] {
            assert!(
                at(corner * away, 8.0) < peak,
                "the peak drifted off the corner ({away}x)"
            );
        }
    }

    /// THE agreement test: the kernel and the display draw the same
    /// filter.
    ///
    /// `ui::device::filter` computes a magnitude response from RBJ biquad
    /// coefficients; this kernel is a trapezoidal SVF cascade. Two
    /// derivations, two code paths, one filter — and until now nothing
    /// forced them to agree. A display that quietly disagrees with the
    /// audio is worse than no display, because it is confidently wrong.
    #[test]
    fn the_kernel_matches_what_the_ui_draws() {
        use crate::ui::device::filter as ui;
        let corner = 1_000.0;
        for (order, slope) in [
            (2u32, ui::Slope::Db12),
            (4, ui::Slope::Db24),
            (8, ui::Slope::Db48),
        ] {
            for hz in [100.0f32, 400.0, 1_000.0, 2_000.0, 4_000.0] {
                let drawn = ui::magnitude_db(
                    &ui::Filter {
                        mode: ui::Mode::Lowpass,
                        slope,
                        cutoff_hz: corner,
                        q: FLAT_Q,
                        drive: 0.0,
                        character: crate::params::filter::CHAR_CLEAN,
                    },
                    hz,
                    FS,
                );
                // Above the measurement floor only: deeper than this the
                // cascade's own f32 noise is what gets measured, and the
                // display's arithmetic has no such floor, so the two
                // would "disagree" about numbers neither can represent.
                if drawn < FLOOR_DB {
                    continue;
                }
                let mut c = Cascade::new();
                c.prepare(FS, corner, FLAT_Q, order, false);
                let measured = db(magnitude(&mut |b| c.process(b), hz));
                assert!(
                    (measured - drawn).abs() < 1.5,
                    "order {order} at {hz} Hz: kernel {measured:.2} dB, display {drawn:.2} dB"
                );
            }
        }
    }

    /// The tilt see-saws around its pivot: unity where the user pointed,
    /// the full stated gain at the extremes, monotone in between — and
    /// the negative tilt is the positive one's mirror.
    #[test]
    fn tilt_pivots_at_unity_and_reaches_its_extremes() {
        let pivot = 1_000.0;
        let at = |tilt_db: f32, hz: f32| {
            let mut t = Tilt::new();
            t.prepare(FS, pivot, tilt_db);
            db(magnitude(&mut |b| t.process(b), hz))
        };

        for tilt in [6.0f32, -6.0, 12.0] {
            assert!(
                at(tilt, pivot).abs() < 0.4,
                "tilt {tilt}: pivot should be unity, got {:.2} dB",
                at(tilt, pivot)
            );
            let hi = at(tilt, pivot * 16.0);
            let lo = at(tilt, pivot / 16.0);
            assert!((hi - tilt).abs() < 0.5, "tilt {tilt}: top is {hi:.2} dB");
            assert!((lo + tilt).abs() < 0.5, "tilt {tilt}: bottom is {lo:.2} dB");
        }

        // Monotone across the band for a positive tilt — a see-saw has
        // no bumps.
        let mut last = f32::NEG_INFINITY;
        for hz in [
            60.0f32, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0,
        ] {
            let g = at(6.0, hz);
            assert!(g >= last - 0.1, "response dipped at {hz} Hz");
            last = g;
        }

        // Mirror: +6 and -6 cancel.
        for hz in [100.0f32, 1_000.0, 8_000.0] {
            let sum = at(6.0, hz) + at(-6.0, hz);
            assert!(
                sum.abs() < 0.3,
                "+6 and -6 should mirror at {hz} Hz: {sum:.2}"
            );
        }
    }

    /// Zero tilt is bit-exact passthrough — g_hi is 1 and the delta term
    /// vanishes, so "no tilt" means NO tilt, not nearly none.
    #[test]
    fn tilt_zero_is_bit_exact_passthrough() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.17).sin()).collect();
        let mut t = Tilt::new();
        t.prepare(FS, 1_000.0, 0.0);
        let mut buf = input.clone();
        t.process(&mut buf);
        assert!(
            buf.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "zero tilt must be the identity"
        );
    }

    /// The contract set for Tilt: split-block bit-exact, any length,
    /// silence in silence out, nonsense settings never invent NaN, and
    /// no allocation on the process path.
    #[test]
    fn tilt_meets_the_contract() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin()).collect();

        let mut a = Tilt::new();
        a.prepare(FS, 800.0, 6.0);
        let mut whole = input.clone();
        a.process(&mut whole);
        let mut b = Tilt::new();
        b.prepare(FS, 800.0, 6.0);
        let mut split = input.clone();
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "256 must equal 100 + 156"
        );

        for len in [0usize, 1, 3, 63] {
            let mut buf = vec![0.25f32; len];
            let mut t = Tilt::new();
            t.prepare(FS, 500.0, -9.0);
            t.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()), "len {len}");
        }

        let mut t = Tilt::new();
        t.prepare(FS, 1_000.0, 6.0);
        let mut silent = vec![0.0f32; 128];
        t.process(&mut silent);
        assert!(silent.iter().all(|s| *s == 0.0), "silence in, silence out");

        for (fs, hz, tilt) in [
            (FS, f32::NAN, 6.0),
            (FS, -100.0, 6.0),
            (FS, 1_000.0, f32::NAN),
            (FS, 1_000.0, 1e9),
            (0.0, 1_000.0, 6.0),
            (f32::NAN, 1_000.0, -6.0),
            (FS, 1e9, TILT_MAX_DB),
        ] {
            let mut t = Tilt::new();
            t.prepare(fs, hz, tilt);
            let mut buf = input.clone();
            t.process(&mut buf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "fs {fs} hz {hz} tilt {tilt}"
            );
        }

        let mut t = Tilt::new();
        t.prepare(FS, 1_000.0, 6.0);
        let mut buf = vec![0.1f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                t.process(&mut buf);
            }
        });
    }

    // ---------------------------------------------------------- disperser ---

    /// A disperser's magnitude, measured after it has actually settled.
    ///
    /// The shared [`magnitude`] helper warms up for a fixed 8 192 samples,
    /// which is plenty for one ordinary filter and nowhere near enough
    /// here: an allpass section rings for roughly `2Q / 2πf` seconds, and
    /// this kernel stacks up to thirty-two of them. Thirty-two sections
    /// at Q 20 and 40 Hz need over five seconds to settle, and measuring
    /// before then reads the transient as a dip — which looks exactly
    /// like a kernel that is not flat.
    ///
    /// So the warm-up is DERIVED from the settings rather than fixed. The
    /// kernel is flat; the measurement has to be long enough to see it.
    ///
    /// `2Q / 2πf` is the PEAK GROUP DELAY, not the ring-down — the tail
    /// takes several of those to fall into the noise. Measured: at twelve
    /// sections, Q 20 and the corner, one settle reads −1.12 dB, four
    /// read −0.01 and sixteen read +0.01. Eight is the factor used here,
    /// comfortably past where it converges.
    const SETTLE_FACTOR: usize = 8;

    fn disperser_magnitude(stages: u32, q: f32, hz: f32) -> f32 {
        let settle = (stages as f32 * 2.0 * q / (core::f32::consts::TAU * hz) * FS) as usize;
        let window = 8_192usize;
        let n = settle.saturating_mul(SETTLE_FACTOR).max(window * 2) + window;
        let mut buf: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
            .collect();
        let mut d = Disperser::new();
        d.prepare(FS, 200.0, q, stages);
        d.process(&mut buf);
        let tail = &buf[n - window..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        rms * core::f32::consts::SQRT_2
    }

    /// A DISPERSER MUST NOT BE AUDIBLE AS A FILTER.
    ///
    /// Flat magnitude is the whole licence for putting thirty-two
    /// second-order sections across a drum. If any of them coloured the
    /// level the effect would be a resonant sweep wearing a phase
    /// effect's name — and it would be blamed on the drum, not on this.
    #[test]
    fn a_disperser_is_flat_at_every_setting() {
        for stages in [1u32, 4, 12, 32] {
            for q in [0.3f32, FLAT_Q, 4.0, 20.0] {
                for hz in [40.0f32, 120.0, 200.0, 400.0, 2_000.0, 9_000.0] {
                    let level = db(disperser_magnitude(stages, q, hz));
                    assert!(
                        level.abs() < 0.35,
                        "{stages} stages, q {q}, {hz} Hz: {level:.3} dB — not flat"
                    );
                }
            }
        }
        // Zero stages is a WIRE, bit for bit. A disperser turned all the
        // way down must be indistinguishable from not being there.
        let input: Vec<f32> = (0..128).map(|i| ((i as f32) * 0.37).sin()).collect();
        let mut d = Disperser::new();
        d.prepare(FS, 200.0, FLAT_Q, 0);
        let mut buf = input.clone();
        d.process(&mut buf);
        assert!(
            buf.iter()
                .zip(&input)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "no stages must be a bit-exact wire"
        );
        assert_eq!(d.stages(), 0);
        assert_eq!(d.latency(), 0);
    }

    /// THE PHASE IS THE POINT, and it has to grow with the stage count —
    /// otherwise the control does nothing and the flatness test above
    /// would happily pass a chain that was silently bypassed.
    ///
    /// Measured as group delay: how much later the energy of an impulse
    /// arrives, as a centre of mass. One section already smears; more
    /// sections must smear strictly more.
    #[test]
    fn more_stages_disperse_more() {
        let centroid = |stages: u32| {
            let mut d = Disperser::new();
            d.prepare(FS, 300.0, 2.0, stages);
            let mut buf = vec![0.0f32; 4_096];
            if let Some(first) = buf.first_mut() {
                *first = 1.0;
            }
            d.process(&mut buf);
            let energy: f64 = buf.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
            if energy <= 0.0 {
                return 0.0;
            }
            let weighted: f64 = buf
                .iter()
                .enumerate()
                .map(|(i, s)| i as f64 * f64::from(*s) * f64::from(*s))
                .sum();
            weighted / energy
        };
        let none = centroid(0);
        assert!(none < 0.5, "no stages: the impulse stays put ({none})");
        let mut previous = none;
        for stages in [1u32, 2, 4, 8, 16, 32] {
            let now = centroid(stages);
            assert!(
                now > previous,
                "{stages} stages smeared to {now:.1}, no further than {previous:.1}"
            );
            previous = now;
        }
        // And it is a real amount of smear, not a rounding difference.
        assert!(
            previous > 20.0,
            "32 sections barely moved anything: {previous}"
        );
    }

    /// An impulse's ONSET does not move, which is why `latency()` is zero.
    /// The energy spreads later; nothing arrives earlier and nothing is
    /// delayed by a fixed offset for compensation to remove.
    #[test]
    fn a_disperser_reports_no_latency_because_it_has_none() {
        let mut d = Disperser::new();
        d.prepare(FS, 300.0, 2.0, 16);
        let mut buf = vec![0.0f32; 512];
        if let Some(first) = buf.first_mut() {
            *first = 1.0;
        }
        d.process(&mut buf);
        assert_eq!(d.latency(), 0);
        assert!(
            buf.first().is_some_and(|s| s.abs() > 1e-6),
            "the first sample is already non-zero: the onset did not move"
        );
    }

    /// RE-TUNING MUST NOT CLICK, because a tuned disperser is re-tuned on
    /// every note. Only the coefficients move; the integrators keep their
    /// charge, so the output stays continuous across the change.
    ///
    /// Also the check that computing the coefficients once and copying
    /// them really does give every section the same tuning — a copy that
    /// missed a field would leave sections silently detuned, which sounds
    /// like a wider effect rather than a bug.
    #[test]
    fn retuning_keeps_the_state_and_reaches_every_section() {
        // Every section identical: preparing the chain and preparing one
        // lone SVF the ordinary way must agree, bit for bit.
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.23).sin()).collect();
        let mut chain = Disperser::new();
        chain.prepare(FS, 450.0, 3.0, 3);
        let mut chained = input.clone();
        chain.process(&mut chained);

        let mut manual = input.clone();
        for _ in 0..3 {
            let mut one = Svf::new();
            one.prepare(FS, 450.0, 3.0);
            one.process(&mut manual, Mode::Allpass);
        }
        assert!(
            chained
                .iter()
                .zip(&manual)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "the copied coefficients differ from a plainly prepared section"
        );

        // Re-tuning mid-signal leaves no discontinuity: the sample after
        // the change is close to the one before it, where a state reset
        // would jump.
        let mut d = Disperser::new();
        d.prepare(FS, 200.0, 4.0, 8);
        let mut warm: Vec<f32> = (0..1_024)
            .map(|i| ((i as f32) * 0.05).sin() * 0.5)
            .collect();
        d.process(&mut warm);
        let last = warm.last().copied().unwrap_or(0.0);
        d.prepare(FS, 260.0, 4.0, 8);
        let mut next: Vec<f32> = (1_024..1_040)
            .map(|i| ((i as f32) * 0.05).sin() * 0.5)
            .collect();
        d.process(&mut next);
        let first = next.first().copied().unwrap_or(0.0);
        assert!(
            (first - last).abs() < 0.25,
            "re-tune jumped from {last:.4} to {first:.4} — the state was cleared"
        );
    }

    /// The contract's remaining four, on one kernel.
    #[test]
    fn a_disperser_survives_split_blocks_edges_and_nonsense() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin()).collect();

        // Split-block equivalence, bit for bit.
        for stages in [1u32, 5, 32] {
            let mut a = Disperser::new();
            a.prepare(FS, 640.0, 1.7, stages);
            let mut whole = input.clone();
            a.process(&mut whole);

            let mut b = Disperser::new();
            b.prepare(FS, 640.0, 1.7, stages);
            let mut split = input.clone();
            b.process(&mut split[..100]);
            b.process(&mut split[100..]);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "{stages} stages: 256 must equal 100 + 156"
            );
        }

        // Edge lengths, including zero and a non-power-of-two.
        for len in [0usize, 1, 3, 63] {
            let mut d = Disperser::new();
            d.prepare(FS, 300.0, 2.0, 8);
            let mut buf = vec![0.25f32; len];
            d.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()), "len {len}");
        }

        // Silence in, silence out.
        let mut d = Disperser::new();
        d.prepare(FS, 300.0, 2.0, 8);
        let mut silent = vec![0.0f32; 128];
        d.process(&mut silent);
        assert!(silent.iter().all(|s| *s == 0.0), "silence in, silence out");

        // A decaying tail stays finite and lands on exact zero rather
        // than grinding through denormals forever.
        let mut d = Disperser::new();
        d.prepare(FS, 120.0, 6.0, 32);
        let mut tail: Vec<f32> = (0..2_048)
            .map(|i| (-(i as f32) / 200.0).exp() * ((i as f32) * 0.2).sin())
            .collect();
        d.process(&mut tail);
        assert!(
            tail.iter().all(|s| s.is_finite()),
            "the tail went non-finite"
        );

        // Nonsense settings are clamped, never propagated as NaN.
        for (fs, hz, q, stages) in [
            (FS, f32::NAN, 2.0, 8u32),
            (FS, -100.0, 2.0, 8),
            (FS, 300.0, f32::NAN, 8),
            (FS, 300.0, 0.0, 8),
            (FS, 300.0, 2.0, 9_999),
            (FS, 1e9, 2.0, 8),
            (0.0, 300.0, 2.0, 8),
            (f32::NAN, 300.0, 2.0, 8),
        ] {
            let mut d = Disperser::new();
            d.prepare(fs, hz, q, stages);
            assert!(d.stages() <= DISPERSER_MAX_STAGES);
            let mut buf = input.clone();
            d.process(&mut buf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "fs {fs} hz {hz} q {q} stages {stages}"
            );
        }

        // Reset clears the state without losing the tuning.
        let mut d = Disperser::new();
        d.prepare(FS, 300.0, 2.0, 8);
        let mut warm = input.clone();
        d.process(&mut warm);
        d.reset();
        let mut after = input.clone();
        d.process(&mut after);
        let mut fresh_kernel = Disperser::new();
        fresh_kernel.prepare(FS, 300.0, 2.0, 8);
        let mut fresh = input.clone();
        fresh_kernel.process(&mut fresh);
        assert!(
            after
                .iter()
                .zip(&fresh)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "reset must leave the kernel exactly as new"
        );

        // No allocation on the process path.
        let mut d = Disperser::new();
        d.prepare(FS, 300.0, 2.0, 32);
        let mut buf = vec![0.1f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                d.process(&mut buf);
            }
        });
    }

    // ------------------------------------------- split-block equivalence ---

    #[test]
    fn split_block_is_bit_exact() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin()).collect();

        // Cascade and DC blocker, split the same way.
        for order in [1u32, 4, 7] {
            let mut a = Cascade::new();
            a.prepare(FS, 900.0, 2.0, order, false);
            let mut whole = input.clone();
            a.process(&mut whole);

            let mut b = Cascade::new();
            b.prepare(FS, 900.0, 2.0, order, false);
            let mut split = input.clone();
            b.process(&mut split[..100]);
            b.process(&mut split[100..]);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "cascade order {order}: 256 must equal 100 + 156"
            );
        }
        for shape in [BandShape::Bell, BandShape::LowShelf, BandShape::HighShelf] {
            let mut a = EqBand::new();
            a.prepare(FS, 700.0, 1.3, 8.0, shape);
            let mut whole = input.clone();
            a.process(&mut whole);

            let mut b = EqBand::new();
            b.prepare(FS, 700.0, 1.3, 8.0, shape);
            let mut split = input.clone();
            b.process(&mut split[..100]);
            b.process(&mut split[100..]);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "eq band {shape:?}: 256 must equal 100 + 156"
            );
        }
        {
            let mut a = DcBlocker::new();
            a.prepare(FS);
            let mut whole = input.clone();
            a.process(&mut whole);

            let mut b = DcBlocker::new();
            b.prepare(FS);
            let mut split = input.clone();
            b.process(&mut split[..100]);
            b.process(&mut split[100..]);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "dc blocker: 256 must equal 100 + 156"
            );
        }

        let mut whole_pole = OnePole::new();
        whole_pole.prepare(FS, 800.0);
        let mut whole = input.clone();
        whole_pole.process_lowpass(&mut whole);

        let mut split_pole = OnePole::new();
        split_pole.prepare(FS, 800.0);
        let mut split = input.clone();
        split_pole.process_lowpass(&mut split[..100]);
        split_pole.process_lowpass(&mut split[100..]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "one-pole: 256 must equal 100 + 156"
        );

        for mode in [
            Mode::Lowpass,
            Mode::Highpass,
            Mode::Bandpass,
            Mode::BandpassUnity,
            Mode::Notch,
            Mode::Peak,
            Mode::Allpass,
        ] {
            let mut a = Svf::new();
            a.prepare(FS, 900.0, 3.0);
            let mut whole = input.clone();
            a.process(&mut whole, mode);

            let mut b = Svf::new();
            b.prepare(FS, 900.0, 3.0);
            let mut split = input.clone();
            b.process(&mut split[..100], mode);
            b.process(&mut split[100..], mode);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "{mode:?}: 256 must equal 100 + 156"
            );
        }
    }

    // ------------------------------------------------------------ no-alloc ---

    #[test]
    fn process_does_not_allocate() {
        let mut pole = OnePole::new();
        pole.prepare(FS, 1_000.0);
        let mut svf = Svf::new();
        svf.prepare(FS, 1_000.0, 2.0);
        let mut cascade = Cascade::new();
        cascade.prepare(FS, 1_000.0, 2.0, 7, false);
        let mut dc = DcBlocker::new();
        dc.prepare(FS);
        let mut bands = [EqBand::new(); 3];
        for (band, shape) in
            bands
                .iter_mut()
                .zip([BandShape::Bell, BandShape::LowShelf, BandShape::HighShelf])
        {
            band.prepare(FS, 1_000.0, 1.2, 6.0, shape);
        }
        let mut buf = vec![0.1f32; 256];

        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                for band in bands.iter_mut() {
                    band.process(&mut buf);
                }
                cascade.process(&mut buf);
                dc.process(&mut buf);
                pole.process_lowpass(&mut buf);
                pole.process_highpass(&mut buf);
                for mode in [
                    Mode::Lowpass,
                    Mode::Highpass,
                    Mode::Bandpass,
                    Mode::BandpassUnity,
                    Mode::Notch,
                    Mode::Peak,
                    Mode::Allpass,
                ] {
                    svf.process(&mut buf, mode);
                }
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let mut pole = OnePole::new();
        pole.prepare(FS, 1_000.0);
        let mut svf = Svf::new();
        svf.prepare(FS, 1_000.0, 1.5);

        let mut cascade = Cascade::new();
        cascade.prepare(FS, 1_000.0, 2.0, 5, true);
        let mut dc = DcBlocker::new();
        dc.prepare(FS);
        let mut band = EqBand::new();
        band.prepare(FS, 1_000.0, 1.5, -9.0, BandShape::Bell);

        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut buf = vec![0.25f32; len];
            pole.process_lowpass(&mut buf);
            pole.process_highpass(&mut buf);
            svf.process(&mut buf, Mode::Lowpass);
            svf.process(&mut buf, Mode::Allpass);
            cascade.process(&mut buf);
            dc.process(&mut buf);
            band.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    // ------------------------------------------------ silence and denormals ---

    /// Silence in, silence out — and a decaying tail must reach or
    /// approach zero without ever becoming NaN or infinite.
    #[test]
    fn silence_stays_silent_and_tails_stay_finite() {
        let mut svf = Svf::new();
        svf.prepare(FS, 200.0, 6.0);
        let mut buf = vec![0.0f32; 512];
        svf.process(&mut buf, Mode::Lowpass);
        assert!(buf.iter().all(|s| *s == 0.0), "silence in, silence out");

        // A decaying impulse tail, run far past audibility.
        let mut tail = vec![0.0f32; 256];
        tail[0] = 1.0;
        svf.process(&mut tail, Mode::Lowpass);
        for _ in 0..2_000 {
            let mut quiet = vec![0.0f32; 256];
            svf.process(&mut quiet, Mode::Lowpass);
            assert!(
                quiet.iter().all(|s| s.is_finite()),
                "a decaying tail must stay finite"
            );
        }
    }

    /// A cascade survives every order and every silly setting, including
    /// the orders a caller can ask for and this cannot give.
    #[test]
    fn a_cascade_clamps_its_order_and_stays_finite() {
        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
        for order in [0u32, 1, 8, 9, 999] {
            for (fs, hz, q) in [(FS, 1_000.0, FLAT_Q), (0.0, 1_000.0, 1.0), (FS, 1e9, -1.0)] {
                for highpass in [false, true] {
                    let mut c = Cascade::new();
                    c.prepare(fs, hz, q, order, highpass);
                    let mut buf = input.clone();
                    c.process(&mut buf);
                    assert!(
                        buf.iter().all(|s| s.is_finite()),
                        "order {order} fs {fs} hz {hz} q {q}"
                    );
                    // Asking for more than exists gives the steepest
                    // available rather than silence.
                    let claimed = c.db_per_octave();
                    assert!(
                        (6.0..=48.0).contains(&claimed),
                        "order {order} advertised {claimed} dB/oct"
                    );
                    assert_eq!(c.latency(), 0);
                }
            }
        }
    }

    // --------------------------------------------------------- eq band ---

    /// The three numbers an equaliser band is bought for: a bell hits its
    /// stated gain at its centre and is flat far from it, a low shelf
    /// settles at that gain below the corner and unity above, and a high
    /// shelf does the mirror image.
    ///
    /// Measured through the running filter, not restated from its
    /// coefficients — the point is what the audio gets.
    #[test]
    fn an_eq_band_applies_the_gain_it_was_asked_for() {
        let at = |shape: BandShape, gain_db: f32, q: f32, hz: f32| {
            let mut band = EqBand::new();
            band.prepare(FS, 1_000.0, q, gain_db, shape);
            db(magnitude(&mut |b| band.process(b), hz))
        };

        for gain in [-12.0f32, -6.0, 6.0, 12.0] {
            // A bell peaks at its centre, whatever the sign.
            let peak = at(BandShape::Bell, gain, 2.0, 1_000.0);
            assert!(
                (peak - gain).abs() < 0.25,
                "bell {gain:+} dB measured {peak:+.2} at its centre"
            );
            // ...and leaves the rest of the spectrum alone. Three octaves
            // out at Q = 2 there is nothing left of it.
            let away = at(BandShape::Bell, gain, 2.0, 125.0);
            assert!(
                away.abs() < 0.6,
                "bell {gain:+} dB moved 125 Hz by {away:+.2}"
            );

            // A low shelf reaches the gain below and unity above.
            let below = at(BandShape::LowShelf, gain, FLAT_Q, 40.0);
            let above = at(BandShape::LowShelf, gain, FLAT_Q, 12_000.0);
            assert!(
                (below - gain).abs() < 0.3,
                "low shelf {gain:+} dB settled at {below:+.2}"
            );
            assert!(
                above.abs() < 0.5,
                "low shelf must be flat above: {above:+.2}"
            );

            // And a high shelf is its mirror.
            let above = at(BandShape::HighShelf, gain, FLAT_Q, 12_000.0);
            let below = at(BandShape::HighShelf, gain, FLAT_Q, 40.0);
            assert!(
                (above - gain).abs() < 0.6,
                "high shelf {gain:+} dB settled at {above:+.2}"
            );
            assert!(
                below.abs() < 0.3,
                "high shelf must be flat below: {below:+.2}"
            );
        }

        // Zero gain is a wire, in every shape.
        for shape in [BandShape::Bell, BandShape::LowShelf, BandShape::HighShelf] {
            for hz in [50.0f32, 1_000.0, 9_000.0] {
                let flat = at(shape, 0.0, 1.4, hz);
                assert!(
                    flat.abs() < 0.02,
                    "{shape:?} at 0 dB moved {hz} Hz by {flat:+.3}"
                );
            }
        }
    }

    /// The curve a display draws and the filter the audio hears are the
    /// same object.
    ///
    /// [`EqBand::coeffs`] is what the EQ card evaluates to paint its
    /// response, so it has to be this instance's own transfer function
    /// rather than a nearby analogue formula. Measured against the
    /// running filter at a spread of frequencies, for every shape.
    #[test]
    fn the_stated_transfer_function_is_the_one_that_runs() {
        /// `|H(e^{jw})|` from a normalised biquad — the same evaluation a
        /// response display does.
        fn magnitude_of(c: [f32; 5], w: f32) -> f32 {
            let (c1, s1) = (w.cos(), w.sin());
            let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
            let num_re = c[0] + c[1] * c1 + c[2] * c2;
            let num_im = -(c[1] * s1 + c[2] * s2);
            let den_re = 1.0 + c[3] * c1 + c[4] * c2;
            let den_im = -(c[3] * s1 + c[4] * s2);
            let num = (num_re * num_re + num_im * num_im).sqrt();
            let den = (den_re * den_re + den_im * den_im).sqrt();
            if den <= f32::MIN_POSITIVE {
                0.0
            } else {
                num / den
            }
        }

        for shape in [BandShape::Bell, BandShape::LowShelf, BandShape::HighShelf] {
            for (corner, q, gain) in [
                (100.0f32, 0.7f32, 9.0f32),
                (1_000.0, 2.5, -9.0),
                (6_000.0, 1.0, 4.5),
            ] {
                let mut band = EqBand::new();
                band.prepare(FS, corner, q, gain, shape);
                let coeffs = band.coeffs();
                for hz in [30.0f32, 120.0, 500.0, 1_000.0, 3_000.0, 9_000.0] {
                    let mut run = EqBand::new();
                    run.prepare(FS, corner, q, gain, shape);
                    let measured = db(magnitude(&mut |b| run.process(b), hz));
                    let stated = db(magnitude_of(coeffs, core::f32::consts::TAU * hz / FS));
                    assert!(
                        (measured - stated).abs() < 0.15,
                        "{shape:?} {corner} Hz Q{q} {gain:+} dB at {hz} Hz: \
                         the filter does {measured:+.2}, the curve says {stated:+.2}"
                    );
                }
            }
        }
    }

    /// Silence in, silence out, a tail that stays finite, and every silly
    /// setting a caller can reach refused into something inert rather
    /// than into a NaN that poisons the chain forever.
    #[test]
    fn an_eq_band_survives_silence_tails_and_nonsense() {
        let mut band = EqBand::new();
        band.prepare(FS, 200.0, 8.0, 18.0, BandShape::Bell);
        let mut buf = vec![0.0f32; 512];
        band.process(&mut buf);
        assert!(buf.iter().all(|s| *s == 0.0), "silence in, silence out");

        let mut tail = vec![0.0f32; 256];
        tail[0] = 1.0;
        band.process(&mut tail);
        for _ in 0..2_000 {
            let mut quiet = vec![0.0f32; 256];
            band.process(&mut quiet);
            assert!(
                quiet.iter().all(|s| s.is_finite()),
                "a decaying tail must stay finite"
            );
        }

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
        for shape in [BandShape::Bell, BandShape::LowShelf, BandShape::HighShelf] {
            for (fs, hz, q, gain) in [
                (FS, 0.0, FLAT_Q, 6.0),
                (FS, 1e9, FLAT_Q, 6.0),
                (0.0, 1_000.0, 1.0, 6.0),
                (FS, 1_000.0, -1.0, 6.0),
                (FS, 1_000.0, 0.0, 6.0),
                (FS, 1_000.0, 1.0, f32::NAN),
                (FS, 1_000.0, f32::NAN, 6.0),
                (FS, f32::INFINITY, 1.0, -600.0),
            ] {
                let mut b = EqBand::new();
                b.prepare(fs, hz, q, gain, shape);
                let mut buf = input.clone();
                b.process(&mut buf);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "{shape:?} fs {fs} hz {hz} q {q} gain {gain}"
                );
                assert!(b.coeffs().iter().all(|c| c.is_finite()));
                assert_eq!(b.latency(), 0);
            }
        }
    }

    /// NaN may pass through; it must never be invented from finite input
    /// and finite settings — including the settings a caller can get
    /// wrong.
    #[test]
    fn nonsense_settings_never_invent_nan() {
        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
        for (fs, hz, q) in [
            (FS, 0.0, FLAT_Q),
            (FS, -100.0, FLAT_Q),
            (FS, 1e9, FLAT_Q),
            (FS, f32::NAN, FLAT_Q),
            (FS, 1_000.0, 0.0),
            (FS, 1_000.0, -4.0),
            (FS, 1_000.0, f32::NAN),
            (0.0, 1_000.0, FLAT_Q),
            (-48_000.0, 1_000.0, FLAT_Q),
            (f32::NAN, 1_000.0, FLAT_Q),
            // Right at the guard, where `tan` would otherwise run away.
            (FS, FS * 0.5, FLAT_Q),
            (FS, FS, FLAT_Q),
        ] {
            let mut svf = Svf::new();
            svf.prepare(fs, hz, q);
            let mut buf = input.clone();
            svf.process(&mut buf, Mode::Lowpass);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "fs {fs} hz {hz} q {q} produced a non-finite sample"
            );

            let mut pole = OnePole::new();
            pole.prepare(fs, hz);
            let mut buf = input.clone();
            pole.process_lowpass(&mut buf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "one-pole fs {fs} hz {hz}"
            );
        }
    }

    /// A fresh, never-prepared filter passes signal rather than silencing
    /// it. An unprepared kernel in a chain is a bug, and silence hides it
    /// where a wide-open filter does not.
    #[test]
    fn an_unprepared_filter_passes_signal() {
        let mut svf = Svf::new();
        let mut buf = vec![0.5f32; 128];
        svf.process(&mut buf, Mode::Lowpass);
        let energy: f32 = buf.iter().map(|s| s.abs()).sum();
        assert!(energy > 1.0, "a fresh SVF should pass, not mute");
    }

    /// In-place is safe: the same slice is read and written per sample,
    /// and the result matches filtering into a separate buffer.
    #[test]
    fn processing_in_place_matches_processing_out_of_place() {
        let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.2).sin()).collect();

        let mut a = Svf::new();
        a.prepare(FS, 700.0, 2.0);
        let mut in_place = input.clone();
        a.process(&mut in_place, Mode::Bandpass);

        // The "out of place" reference: copy first, then filter the copy.
        let mut b = Svf::new();
        b.prepare(FS, 700.0, 2.0);
        let mut copy = vec![0.0f32; input.len()];
        copy.copy_from_slice(&input);
        b.process(&mut copy, Mode::Bandpass);

        assert!(
            in_place
                .iter()
                .zip(&copy)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }
    // --------------------------------------------------------- lanes ---

    fn lane_column(frames: &[LaneFrame], lane: usize) -> Vec<f32> {
        frames.iter().map(|f| f[lane]).collect()
    }

    fn spread(signal: &[f32]) -> Vec<LaneFrame> {
        signal.iter().map(|s| [*s; LANES]).collect()
    }

    /// A short deterministic test signal: an impulse, then a chirp-ish
    /// mix. Enough transient to excite the states and enough tail to
    /// expose a stuck integrator.
    fn probe(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                let impulse = if i == 0 { 1.0 } else { 0.0 };
                impulse
                    + 0.3 * (core::f32::consts::TAU * 220.0 * t).sin()
                    + 0.2 * (core::f32::consts::TAU * 3_500.0 * t).sin()
            })
            .collect()
    }

    /// REFERENCE. Every lane is BIT-IDENTICAL to the scalar filter, in
    /// every mode, at several tunings.
    ///
    /// The scalar `Svf` is already tested against measured magnitude
    /// responses, so bit-equality inherits the whole frequency-response
    /// claim instead of re-measuring it through a second kernel.
    #[test]
    fn every_svf_lane_is_bit_identical_to_the_scalar_filter() {
        const N: usize = 2_048;
        let signal = probe(N);
        for (hz, q) in [(200.0f32, 0.707f32), (1_000.0, 4.0), (12_000.0, 0.5)] {
            for mode in [
                Mode::Lowpass,
                Mode::Highpass,
                Mode::Bandpass,
                Mode::BandpassUnity,
                Mode::Notch,
                Mode::Peak,
                Mode::Allpass,
            ] {
                let mut want = signal.clone();
                let mut sc = Svf::new();
                sc.prepare(48_000.0, hz, q);
                sc.process(&mut want, mode);

                let mut got = spread(&signal);
                let mut la = LaneSvf::new();
                la.prepare(48_000.0, hz, q);
                la.process(&mut got, mode);

                for lane in 0..LANES {
                    assert_eq!(
                        lane_column(&got, lane),
                        want,
                        "svf lane {lane} {hz} {mode:?}"
                    );
                }
            }
        }
    }

    /// REFERENCE. The same, for the cascade — every order, both
    /// directions, so the odd-order one-pole and the resonant last stage
    /// are both covered.
    #[test]
    fn every_cascade_lane_is_bit_identical_to_the_scalar_cascade() {
        const N: usize = 2_048;
        let signal = probe(N);
        for order in 1..=8u32 {
            for highpass in [false, true] {
                let mut want = signal.clone();
                let mut sc = Cascade::new();
                sc.prepare(48_000.0, 800.0, 2.0, order, highpass);
                sc.process(&mut want);

                let mut got = spread(&signal);
                let mut la = LaneCascade::new();
                la.prepare(48_000.0, 800.0, 2.0, order, highpass);
                la.process(&mut got);

                assert_eq!(la.db_per_octave(), sc.db_per_octave(), "order {order}");
                assert_eq!(la.latency(), sc.latency());
                for lane in 0..LANES {
                    assert_eq!(
                        lane_column(&got, lane),
                        want,
                        "cascade lane {lane} order {order} hp={highpass}"
                    );
                }
            }
        }
    }

    /// LANE INDEPENDENCE. The mandatory sixth test.
    ///
    /// Drive ONE lane, silence the rest: the others must be exactly 0.0
    /// and the driven lane must equal the scalar result. Then move which
    /// lane is driven and get the same signal in the new position.
    ///
    /// This is the test that catches the classic port bug — an
    /// integrator left scalar, so every lane shares one state and eight
    /// voices filter each other. Every other test in this file passes
    /// happily while that is true, because they drive all lanes alike.
    #[test]
    fn driving_one_lane_leaves_the_others_silent() {
        const N: usize = 1_024;
        let signal = probe(N);
        let mut want = signal.clone();
        let mut sc = Svf::new();
        sc.prepare(48_000.0, 900.0, 3.0);
        sc.process(&mut want, Mode::Lowpass);

        for lane in 0..LANES {
            let mut io: Vec<LaneFrame> = signal
                .iter()
                .map(|s| {
                    let mut f = [0.0f32; LANES];
                    f[lane] = *s;
                    f
                })
                .collect();
            let mut la = LaneSvf::new();
            la.prepare(48_000.0, 900.0, 3.0);
            la.process(&mut io, Mode::Lowpass);
            for other in 0..LANES {
                if other == lane {
                    assert_eq!(lane_column(&io, other), want, "driven lane {lane}");
                } else {
                    assert!(
                        lane_column(&io, other).iter().all(|s| *s == 0.0),
                        "lane {other} rang while only {lane} was driven"
                    );
                }
            }
        }
    }

    /// Resetting one lane leaves the rest of the group filtering.
    #[test]
    fn resetting_one_lane_leaves_the_others_ringing() {
        let signal = probe(512);
        let mut io = spread(&signal);
        let mut la = LaneCascade::new();
        la.prepare(48_000.0, 700.0, 2.0, 4, false);
        la.process(&mut io);

        let mut cleared = la;
        cleared.reset_lane(3);
        let (mut a, mut b) = (spread(&signal), spread(&signal));
        la.process(&mut a);
        cleared.process(&mut b);
        for lane in 0..LANES {
            if lane == 3 {
                assert_ne!(
                    lane_column(&b, lane),
                    lane_column(&a, lane),
                    "lane 3 kept state"
                );
            } else {
                assert_eq!(
                    lane_column(&b, lane),
                    lane_column(&a, lane),
                    "lane {lane} hit"
                );
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact.
    #[test]
    fn lane_filters_split_bit_exactly() {
        const N: usize = 256;
        const CUT: usize = 100;
        let signal = probe(N);

        let mut whole = spread(&signal);
        let mut a = LaneSvf::new();
        a.prepare(48_000.0, 1_000.0, 2.0);
        a.process(&mut whole, Mode::Lowpass);

        let mut split = spread(&signal);
        let mut b = LaneSvf::new();
        b.prepare(48_000.0, 1_000.0, 2.0);
        let (head, tail) = split.split_at_mut(CUT);
        b.process(head, Mode::Lowpass);
        b.process(tail, Mode::Lowpass);
        assert_eq!(split, whole, "svf");

        let mut whole = spread(&signal);
        let mut a = LaneCascade::new();
        a.prepare(48_000.0, 1_000.0, 2.0, 5, false);
        a.process(&mut whole);

        let mut split = spread(&signal);
        let mut b = LaneCascade::new();
        b.prepare(48_000.0, 1_000.0, 2.0, 5, false);
        let (head, tail) = split.split_at_mut(CUT);
        b.process(head);
        b.process(tail);
        assert_eq!(split, whole, "cascade");
    }

    /// NO-ALLOC on the process path.
    #[test]
    fn lane_filters_do_not_allocate() {
        let mut svf = LaneSvf::new();
        svf.prepare(48_000.0, 1_000.0, 1.0);
        let mut casc = LaneCascade::new();
        casc.prepare(48_000.0, 1_000.0, 1.0, 4, false);
        let mut pole = LaneOnePole::new();
        pole.prepare(48_000.0, 1_000.0);
        let mut buf = vec![[0.1f32; LANES]; 512];
        assert_no_alloc::assert_no_alloc(|| {
            svf.process(&mut buf, Mode::Lowpass);
            casc.process(&mut buf);
            pole.process_lowpass(&mut buf);
            pole.process_highpass(&mut buf);
        });
    }

    /// EDGE LENGTHS: 0, 1, and a non-power-of-two; a zero-length block
    /// must not advance any state.
    #[test]
    fn lane_filters_take_any_block_length() {
        for n in [0usize, 1, 3, 97] {
            let mut svf = LaneSvf::new();
            svf.prepare(48_000.0, 1_000.0, 1.0);
            let mut buf = vec![[0.5f32; LANES]; n];
            svf.process(&mut buf, Mode::Lowpass);
            assert_eq!(buf.len(), n);

            let mut casc = LaneCascade::new();
            casc.prepare(48_000.0, 1_000.0, 1.0, 3, true);
            let mut buf = vec![[0.5f32; LANES]; n];
            casc.process(&mut buf);
            assert_eq!(buf.len(), n);
        }
        let mut svf = LaneSvf::new();
        svf.prepare(48_000.0, 1_000.0, 1.0);
        let untouched = svf.ic1;
        svf.process(&mut [], Mode::Lowpass);
        assert_eq!(svf.ic1, untouched);
    }

    /// SILENCE IN, SILENCE OUT and a finite denormal tail: a decaying
    /// input never becomes NaN and never rings forever.
    #[test]
    fn lane_filters_settle_to_silence() {
        let mut casc = LaneCascade::new();
        casc.prepare(48_000.0, 500.0, 6.0, 4, false);
        // Excite hard, then feed pure silence for a long time.
        let mut hit = vec![[1.0f32; LANES]; 64];
        casc.process(&mut hit);
        let mut tail = vec![[0.0f32; LANES]; 1 << 15];
        casc.process(&mut tail);
        for frame in &tail {
            for s in frame {
                assert!(s.is_finite(), "cascade tail went non-finite: {s}");
            }
        }
        if let Some(last) = tail.last() {
            for s in last {
                assert!(s.abs() < 1e-6, "cascade still ringing at {s}");
            }
        }
        // Silence into a freshly reset filter stays exactly silent.
        casc.reset();
        let mut quiet = vec![[0.0f32; LANES]; 256];
        casc.process(&mut quiet);
        assert!(quiet.iter().flatten().all(|s| *s == 0.0));
    }
    /// PER-LANE CUTOFF: each lane filters at its OWN corner, and each
    /// matches the scalar filter tuned to that corner.
    ///
    /// This is what a per-voice filter envelope and keytrack ride on —
    /// two voices in a group are at different points in their envelopes,
    /// so a shared corner would make one voice's envelope audible on the
    /// other's note.
    #[test]
    fn each_lane_filters_at_its_own_cutoff() {
        const N: usize = 1_024;
        let signal = probe(N);
        let cutoffs: [f32; LANES] = [
            80.0, 160.0, 320.0, 640.0, 1_280.0, 2_560.0, 5_120.0, 10_240.0,
        ];

        let mut svf = LaneSvf::new();
        svf.prepare_lanes(48_000.0, &cutoffs, 2.0);
        let mut got = spread(&signal);
        svf.process(&mut got, Mode::Lowpass);
        for (lane, hz) in cutoffs.iter().enumerate() {
            let mut want = signal.clone();
            let mut sc = Svf::new();
            sc.prepare(48_000.0, *hz, 2.0);
            sc.process(&mut want, Mode::Lowpass);
            assert_eq!(lane_column(&got, lane), want, "svf lane {lane} at {hz} Hz");
        }

        let mut casc = LaneCascade::new();
        casc.prepare_lanes(48_000.0, &cutoffs, 3.0, 4, false);
        let mut got = spread(&signal);
        casc.process(&mut got);
        for (lane, hz) in cutoffs.iter().enumerate() {
            let mut want = signal.clone();
            let mut sc = Cascade::new();
            sc.prepare(48_000.0, *hz, 3.0, 4, false);
            sc.process(&mut want);
            assert_eq!(
                lane_column(&got, lane),
                want,
                "cascade lane {lane} at {hz} Hz"
            );
        }
    }

    /// An odd-order cascade uses its one-pole, and that must be per lane
    /// too — the stage most likely to be left shared by a port.
    #[test]
    fn the_odd_order_pole_is_per_lane() {
        const N: usize = 512;
        let signal = probe(N);
        let cutoffs: [f32; LANES] = [100.0, 300.0, 900.0, 2_700.0, 8_100.0, 200.0, 600.0, 1_800.0];
        for order in [1u32, 3, 5, 7] {
            let mut casc = LaneCascade::new();
            casc.prepare_lanes(48_000.0, &cutoffs, 1.0, order, false);
            let mut got = spread(&signal);
            casc.process(&mut got);
            for (lane, hz) in cutoffs.iter().enumerate() {
                let mut want = signal.clone();
                let mut sc = Cascade::new();
                sc.prepare(48_000.0, *hz, 1.0, order, false);
                sc.process(&mut want);
                assert_eq!(lane_column(&got, lane), want, "order {order} lane {lane}");
            }
        }
    }
}
