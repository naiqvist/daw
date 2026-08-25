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

    /// Red zone: lowpass, in place, any length.
    pub fn process_lowpass(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            *s = self.tick(*s);
        }
    }

    /// Red zone: highpass, in place, any length.
    pub fn process_highpass(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            let x = *s;
            *s = x - self.tick(x);
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
        let mut buf = vec![0.1f32; 256];

        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
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

        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut buf = vec![0.25f32; len];
            pole.process_lowpass(&mut buf);
            pole.process_highpass(&mut buf);
            svf.process(&mut buf, Mode::Lowpass);
            svf.process(&mut buf, Mode::Allpass);
            cascade.process(&mut buf);
            dc.process(&mut buf);
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
}
