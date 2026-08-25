//! Family 9 of the kernel roadmap: dynamics.
//!
//! [`RmsDetector`] (the level a compressor listens to) and
//! [`GainComputer`] (what it decides to do about it). The lookahead
//! limiter — the family's latency-reporting member — follows once the
//! delay module is declared; it reuses the delay line rather than
//! growing a private one.
//!
//! # One truth, twice drawn
//!
//! The gain curve here is THE SAME curve `ui::device::dynamics` draws:
//! soft-knee, dB-domain, compress above the threshold or expand below
//! it, with the quadratic knee that keeps the first derivative
//! continuous (a hard corner is audible as a click on material sitting
//! right at the threshold). The agreement test sweeps both against each
//! other, because a display that quietly disagrees with the audio is
//! worse than no display — it is confidently wrong. The peak-flavoured
//! detector already exists as [`Follower`] in `ramps`; this adds the
//! RMS one, and deliberately does not duplicate the peak.
//!
//! [`Follower`]: crate::dsp::ramps::Follower

use crate::dsp::delay::{self, DelayLine};

/// Ratio limits: 1:1 is no processing; past ~100 the curve is a limiter
/// or a gate to more digits than a float holds.
pub const RATIO_MIN: f32 = 1.0;
pub const RATIO_MAX: f32 = 100.0;
/// Where "level" bottoms out, in dB — quiet enough that a gate can
/// close fully, finite enough that arithmetic stays finite.
pub const FLOOR_DB: f32 = -120.0;

/// Which side of the threshold the curve works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Downward compression above the threshold (a limiter at high
    /// ratio).
    Compress,
    /// Downward expansion below it (a gate at high ratio).
    Expand,
}

// -------------------------------------------------------- RMS detector ---

/// RMS envelope detector: a one-pole average of the SQUARED signal,
/// reported back as linear RMS.
///
/// The window is a time constant on the mean-square, which is what
/// "RMS with a 30 ms window" means on every console: a step of level
/// reads ~63% true after one window.
///
/// State: 8 bytes. Per-sample cost: 2 mul + 2 add + 1 sqrt.
/// Denormal-safe: the mean-square decays toward zero on silence; relies
/// on engine FTZ. Squaring never inflates a denormal.
/// In-place safe: input and output may be the same slice.
/// Latency: 0 samples (the window is smoothing, not lookahead).
#[derive(Debug, Clone, Copy)]
pub struct RmsDetector {
    /// Running mean square.
    ms: f32,
    coeff: f32,
}

impl Default for RmsDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RmsDetector {
    pub fn new() -> Self {
        Self {
            ms: 0.0,
            coeff: 0.0,
        }
    }

    /// Green zone: sample rate and averaging window.
    pub fn prepare(&mut self, sample_rate: f32, window_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let win = if window_ms.is_finite() {
            window_ms.clamp(0.1, 5_000.0)
        } else {
            30.0
        };
        let tau = win * 1e-3 * fs;
        self.coeff = 1.0 - (-1.0 / tau).exp();
    }

    /// Green zone: forget everything heard.
    pub fn reset(&mut self) {
        self.ms = 0.0;
    }

    /// The current level, linear RMS.
    pub fn current(&self) -> f32 {
        self.ms.max(0.0).sqrt()
    }

    /// Red zone: write the running linear RMS of `input` into `env`,
    /// any length (`zip` rules on mismatched slices).
    pub fn process(&mut self, input: &[f32], env: &mut [f32]) {
        let c = self.coeff;
        let mut ms = self.ms;
        for (x, e) in input.iter().zip(env.iter_mut()) {
            let x2 = x * x;
            // A NaN in is the caller's NaN and may pass through; it must
            // not lodge in the state and poison every later block.
            if x2.is_finite() {
                ms += (x2 - ms) * c;
            }
            *e = ms.max(0.0).sqrt();
        }
        self.ms = ms;
    }
}

// -------------------------------------------------------- gain computer ---

/// The gain computer: level in, gain out, both in dB.
///
/// Stateless — ballistics belong to the detector in front and the
/// smoother behind, one job each. `gain_db` is ≤ 0 when reducing and 0
/// when idle; makeup is the caller's `arith` business, deliberately,
/// because makeup folded in here is how a meter ends up reporting zero
/// reduction while the compressor works.
///
/// State: 16 bytes of settings, no signal state. Per-sample cost:
/// ~6 flops, one branch pair.
/// Denormal-safe: pure arithmetic on finite settings.
/// In-place safe: input and output may be the same slice.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct GainComputer {
    mode: Mode,
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
}

impl Default for GainComputer {
    fn default() -> Self {
        Self::new()
    }
}

impl GainComputer {
    pub fn new() -> Self {
        Self {
            mode: Mode::Compress,
            threshold_db: 0.0,
            ratio: RATIO_MIN,
            knee_db: 0.0,
        }
    }

    /// Green zone: the whole rule in one call. Nonsense clamps —
    /// a NaN threshold flattens to "never engages" rather than
    /// poisoning every block after it.
    pub fn configure(&mut self, mode: Mode, threshold_db: f32, ratio: f32, knee_db: f32) {
        self.mode = mode;
        self.threshold_db = if threshold_db.is_finite() {
            threshold_db.clamp(FLOOR_DB, 24.0)
        } else {
            0.0
        };
        self.ratio = if ratio.is_finite() {
            ratio.clamp(RATIO_MIN, RATIO_MAX)
        } else {
            RATIO_MIN
        };
        self.knee_db = if knee_db.is_finite() {
            knee_db.clamp(0.0, 48.0)
        } else {
            0.0
        };
    }

    /// Gain to apply at `level_db`, in dB, ≤ 0. The soft-knee curve —
    /// identical to the one the dynamics display draws, held together
    /// by the agreement test.
    pub fn gain_db(&self, level_db: f32) -> f32 {
        let x = if level_db.is_finite() {
            level_db.max(FLOOR_DB)
        } else {
            FLOOR_DB
        };
        let t = self.threshold_db;
        let r = self.ratio;
        let w = self.knee_db;
        let over = x - t;

        let out = match self.mode {
            Mode::Compress => {
                if 2.0 * over > w {
                    t + over / r
                } else if w > 0.0 && 2.0 * over.abs() <= w {
                    // The quadratic knee: value AND slope continuous at
                    // both edges.
                    let d = over + w * 0.5;
                    x + (1.0 / r - 1.0) * d * d / (2.0 * w)
                } else {
                    x
                }
            }
            Mode::Expand => {
                if 2.0 * over < -w {
                    t + over * r
                } else if w > 0.0 && 2.0 * over.abs() <= w {
                    let d = over - w * 0.5;
                    x + (1.0 - r) * d * d / (2.0 * w)
                } else {
                    x
                }
            }
        };
        (out - x).min(0.0)
    }

    /// Red zone: gains for a block of detector levels (LINEAR in, the
    /// detector's output), written as LINEAR gain factors ready to
    /// multiply onto the signal (`arith::mul`). The dB round trip lives
    /// here, once, so the caller never touches a logarithm.
    pub fn process(&self, env: &[f32], gain: &mut [f32]) {
        for (e, g) in env.iter().zip(gain.iter_mut()) {
            let level_db = 20.0 * e.max(1e-6).log10();
            let gd = self.gain_db(level_db);
            *g = 10.0f32.powf(gd * (1.0 / 20.0));
        }
    }
}

// ---------------------------------------------------- lookahead limiter ---

/// The longest lookahead accepted, in milliseconds. Past this the delay
/// stops being "anticipation" and starts being latency nobody wanted.
pub const LOOKAHEAD_MAX_MS: f32 = 20.0;

/// Brickwall lookahead limiter: the output NEVER exceeds the ceiling,
/// and the family's first genuinely latency-reporting kernel.
///
/// # How the guarantee works
///
/// The audio is delayed by the lookahead `L`; the gain applied to the
/// sample leaving the delay is computed from the sliding maximum of
/// |input| over the window that ENDS at the newest input — so the gain
/// already knows about every peak up to `L` samples in the future.
/// Smoothing (a one-pole attack at `L/4`, a caller-set release) makes it
/// musical; a final `min` against the raw required gain makes it
/// ABSOLUTE — smoothing may shape the gain but can never let a peak
/// through, and the never-exceeds test drives hostile material at +12 dB
/// to hold the kernel to that.
///
/// The sliding max keeps a running (value, age) pair and rescans the
/// window only when the reigning maximum expires: amortised O(1), worst
/// case one bounded `L`-tap scan on decaying material — the documented
/// cost ceiling, not an unbounded loop.
///
/// State: ~50 bytes plus the caller-owned delay buffer
/// ([`Self::scratch_len`]). Per-sample cost, MEASURED at a 5 ms
/// lookahead: 17 ns steady; 570 ns on adversarial monotonically-
/// decaying material that forces the rescan every sample — 2.7% of a
/// core at 48 k, bounded, and acceptable for the bus roles a limiter
/// plays. If a profile ever shows this mattering, the O(1)-worst-case
/// monotonic-wedge algorithm is the known upgrade; per the contract,
/// that optimisation follows profiling, not speculation.
/// Denormal-safe: gain and envelope live near 1.0; the delayed audio
/// relies on engine FTZ like any buffer.
/// In-place safe: yes.
/// Latency: `lookahead` samples — REPORTED, because this delay is not
/// the effect, it is the price of anticipation, and PDC must know.
#[derive(Debug, Clone, Copy)]
pub struct LookaheadLimiter {
    line: DelayLine,
    /// Lookahead in samples.
    lookahead: usize,
    ceiling: f32,
    attack_coeff: f32,
    release_coeff: f32,
    /// Smoothed gain state.
    gain: f32,
    /// Sliding-window maximum of |input| and how many samples ago it
    /// entered the window.
    win_max: f32,
    win_age: usize,
    /// Peak reduction this block, in dB — telemetry for a GR meter.
    reduction_db: f32,
}

impl Default for LookaheadLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LookaheadLimiter {
    pub fn new() -> Self {
        Self {
            line: DelayLine::new(),
            lookahead: 1,
            ceiling: 1.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            gain: 1.0,
            win_max: 0.0,
            win_age: 0,
            reduction_db: 0.0,
        }
    }

    /// Floats the caller-owned buffer needs for a lookahead at a rate.
    /// Green zone, compile-time sizing.
    pub fn scratch_len(sample_rate: f32, lookahead_ms: f32) -> usize {
        delay::buffer_len(Self::samples_for(sample_rate, lookahead_ms) + 4)
    }

    fn samples_for(sample_rate: f32, lookahead_ms: f32) -> usize {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let ms = if lookahead_ms.is_finite() {
            lookahead_ms.clamp(0.1, LOOKAHEAD_MAX_MS)
        } else {
            5.0
        };
        ((ms * 1e-3 * fs) as usize).max(1)
    }

    /// Green zone: sample rate, lookahead and release. The buffer handed
    /// to `process` must be exactly `scratch_len(sample_rate,
    /// lookahead_ms)` long and zeroed first (`mem::clear`).
    pub fn prepare(&mut self, sample_rate: f32, lookahead_ms: f32, release_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.lookahead = Self::samples_for(sample_rate, lookahead_ms);
        self.line.prepare(self.lookahead + 4);
        // Attack rides the lookahead: a quarter of the window reaches
        // ~98% settled by the time the peak arrives, and the hard `min`
        // in the loop covers the rest.
        let attack_tau = (self.lookahead as f32 * 0.25).max(1.0);
        self.attack_coeff = 1.0 - (-1.0 / attack_tau).exp();
        let rel = if release_ms.is_finite() {
            release_ms.clamp(1.0, 5_000.0)
        } else {
            100.0
        };
        self.release_coeff = 1.0 - (-1.0 / (rel * 1e-3 * fs)).exp();
        self.reset();
    }

    /// Green zone: the ceiling, in dBFS (≤ 0 in any sane session; junk
    /// clamps to unity).
    pub fn set_ceiling_db(&mut self, db: f32) {
        let db = if db.is_finite() {
            db.clamp(-60.0, 0.0)
        } else {
            0.0
        };
        self.ceiling = 10.0f32.powf(db * (1.0 / 20.0));
    }

    /// Green zone: forget everything (caller clears the buffer slice).
    pub fn reset(&mut self) {
        self.line.reset();
        self.gain = 1.0;
        self.win_max = 0.0;
        self.win_age = 0;
        self.reduction_db = 0.0;
    }

    /// The anticipation this kernel buys, in samples. THE latency that
    /// plugin-delay compensation must absorb.
    pub fn latency(&self) -> usize {
        self.lookahead
    }

    /// Peak gain reduction over the last processed block, in dB ≥ 0 —
    /// what a GR meter shows.
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Red zone: limit in place, any length.
    pub fn process(&mut self, io: &mut [f32], buf: &mut [f32]) {
        if !self.line.matches(buf) {
            return; // fail open, undelayed — loud enough to notice
        }
        let window = self.lookahead + 1;
        let ceiling = self.ceiling;
        let mut worst = 0.0f32; // block-local peak gain reduction
        for s in io.iter_mut() {
            let x = *s;
            self.line.push(buf, x);

            // Sliding max of |input| over the window ending now.
            let ax = if x.is_finite() { x.abs() } else { 0.0 };
            self.win_age += 1;
            if ax >= self.win_max {
                self.win_max = ax;
                self.win_age = 0;
            } else if self.win_age >= window {
                // The reigning peak fell out of the window: one bounded
                // rescan. `behind` 1 is the newest pushed sample.
                let mut m = 0.0f32;
                let mut age = window - 1;
                for behind in 1..=window {
                    let v = self.line.tap(buf, behind).abs();
                    // `>=` prefers the NEWEST equal value, which keeps
                    // the age low and rescans rare on flat material.
                    if v >= m {
                        m = v;
                        age = behind - 1;
                    }
                }
                self.win_max = m;
                self.win_age = age;
            }

            // The gain that GUARANTEES the delayed sample fits.
            let required = if self.win_max > ceiling {
                ceiling / self.win_max
            } else {
                1.0
            };
            // Musical smoothing toward it — attack down, release up...
            let coeff = if required < self.gain {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.gain += (required - self.gain) * coeff;
            // ...and the hard floor that makes the ceiling absolute:
            // smoothing shapes the gain but never lets a peak through.
            if self.gain > required {
                self.gain = required;
            }

            let delayed = self.line.tap(buf, self.lookahead + 1);
            *s = delayed * self.gain;

            let red = -20.0 * self.gain.max(1e-6).log10();
            if red > worst {
                worst = red;
            }
        }
        self.reduction_db = worst;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    // -------------------------------------------------------- reference ---

    /// A unit sine reads 1/sqrt(2); a step of level is ~63% true after
    /// one window — the meaning of "RMS with a window".
    #[test]
    fn rms_reads_sines_and_steps_the_way_consoles_mean() {
        let mut det = RmsDetector::new();
        det.prepare(FS, 10.0);
        let n = (FS * 0.5) as usize;
        let sig: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * 997.0 * core::f32::consts::TAU).sin())
            .collect();
        let mut env = vec![0.0f32; n];
        det.process(&sig, &mut env);
        let settled = env[n - 1];
        let want = 1.0 / 2.0f32.sqrt();
        assert!(
            (settled - want).abs() < want * 0.02,
            "unit sine reads {settled:.4}, want {want:.4}"
        );

        // Step response: one window from silence to unit DC is 1-1/e of
        // the MEAN SQUARE.
        let mut det = RmsDetector::new();
        det.prepare(FS, 30.0);
        let steps = (FS * 0.030) as usize;
        let ones = vec![1.0f32; steps];
        let mut env = vec![0.0f32; steps];
        det.process(&ones, &mut env);
        let ms = env[steps - 1] * env[steps - 1];
        let want = 1.0 - (-1.0f32).exp();
        assert!(
            (ms - want).abs() < 0.02,
            "one window reached {ms:.3} of the mean square, want {want:.3}"
        );
    }

    /// THE test this module exists for: the kernel's curve and the
    /// widget's curve are the same curve. Swept across levels, modes,
    /// thresholds, ratios and knees — any drift between the two is a
    /// display lying about the audio.
    #[test]
    fn the_kernel_agrees_with_what_the_ui_draws() {
        use crate::ui::device::dynamics as ui;
        for (mode, ui_mode) in [
            (Mode::Compress, ui::Mode::Compress),
            (Mode::Expand, ui::Mode::Expand),
        ] {
            for (t, r, w) in [
                (-18.0f32, 4.0f32, 6.0f32),
                (-30.0, 2.0, 0.0),
                (-12.0, 60.0, 12.0),
                (-40.0, 10.0, 24.0),
            ] {
                let mut gc = GainComputer::new();
                gc.configure(mode, t, r, w);
                let widget = ui::Dynamics {
                    mode: ui_mode,
                    threshold_db: t,
                    ratio: r,
                    knee_db: w,
                    makeup_db: 0.0,
                    ..ui::Dynamics::default()
                };
                for i in 0..=240 {
                    let level = -120.0 + i as f32 * 0.5;
                    let kernel = gc.gain_db(level);
                    let drawn = -widget.reduction_db(level);
                    assert!(
                        (kernel - drawn).abs() < 1e-3,
                        "{mode:?} t{t} r{r} w{w} at {level} dB: kernel {kernel:.4}, widget {drawn:.4}"
                    );
                }
            }
        }
    }

    /// The slope above threshold is exactly 1/R − 1 in gain terms, 1:1
    /// is a hard zero everywhere, and the knee joins smoothly — the
    /// reference facts, independent of the widget.
    #[test]
    fn the_curve_is_the_textbook_curve() {
        let mut gc = GainComputer::new();
        gc.configure(Mode::Compress, -40.0, 4.0, 0.0);
        let slope = (gc.gain_db(-10.0) - gc.gain_db(-20.0)) / 10.0;
        assert!(
            (slope - (1.0 / 4.0 - 1.0)).abs() < 1e-4,
            "gain slope {slope} should be 1/R - 1"
        );
        assert_eq!(gc.gain_db(-60.0), 0.0, "below threshold is untouched");

        let mut unity = GainComputer::new();
        unity.configure(Mode::Compress, -20.0, 1.0, 12.0);
        for i in 0..=120 {
            let level = -120.0 + i as f32;
            assert_eq!(unity.gain_db(level), 0.0, "1:1 must be exactly zero");
        }

        // Knee: no slope jump crossing it.
        let mut soft = GainComputer::new();
        soft.configure(Mode::Compress, -20.0, 8.0, 10.0);
        let mut prev_slope: Option<f32> = None;
        let mut prev = soft.gain_db(-30.0);
        let mut x = -30.0f32 + 0.05;
        while x <= -10.0 {
            let y = soft.gain_db(x);
            let s = (y - prev) / 0.05;
            if let Some(p) = prev_slope {
                assert!((s - p).abs() < 0.05, "slope jumps at {x}");
            }
            prev_slope = Some(s);
            prev = y;
            x += 0.05;
        }

        // Expander mirror: quiet gets quieter, loud is untouched.
        let mut gate = GainComputer::new();
        gate.configure(Mode::Expand, -40.0, 10.0, 0.0);
        assert_eq!(gate.gain_db(-20.0), 0.0);
        assert!(
            gate.gain_db(-50.0) < -80.0,
            "10:1 below threshold slams shut"
        );
    }

    /// The linear block path is the dB path through the round trip: a
    /// detector level of -18 dB yields exactly 10^(gain_db/20).
    #[test]
    fn the_linear_path_matches_the_db_path() {
        let mut gc = GainComputer::new();
        gc.configure(Mode::Compress, -18.0, 4.0, 6.0);
        let env: Vec<f32> = (1..=100).map(|i| i as f32 / 100.0).collect();
        let mut gains = vec![0.0f32; env.len()];
        gc.process(&env, &mut gains);
        for (e, g) in env.iter().zip(&gains) {
            let level_db = 20.0 * e.log10();
            let want = 10.0f32.powf(gc.gain_db(level_db) / 20.0);
            assert!((g - want).abs() < 1e-5, "at {e}: {g} vs {want}");
            assert!(*g > 0.0 && *g <= 1.0, "gain factor out of range: {g}");
        }
    }

    /// THE limiter guarantee: hostile material at +12 dB over the
    /// ceiling, impulses and steps included, and not one output sample
    /// exceeds it. This is the assertion the whole design serves — the
    /// smoothing is clamped by the raw required gain precisely so this
    /// can be a hard bound and not a "usually".
    #[test]
    fn the_output_never_exceeds_the_ceiling() {
        for ceiling_db in [0.0f32, -1.0, -6.0] {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, 5.0, 80.0);
            lim.set_ceiling_db(ceiling_db);
            let ceiling = 10.0f32.powf(ceiling_db / 20.0);
            let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 5.0)];

            let n = 8_192;
            let mut io: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f32;
                    // A +12 dB sine with impulses and a step slammed in.
                    let mut v = 4.0 * (t / FS * 700.0 * core::f32::consts::TAU).sin();
                    if i % 1_000 == 0 {
                        v = 8.0;
                    }
                    if (2_000..2_400).contains(&i) {
                        v = -6.0;
                    }
                    v
                })
                .collect();
            lim.process(&mut io, &mut buf);
            let peak = io.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak <= ceiling * 1.000_01,
                "ceiling {ceiling_db} dB: peak escaped to {peak} vs {ceiling}"
            );
            assert!(lim.reduction_db() > 6.0, "and it was genuinely working");
        }
    }

    /// Below the ceiling the limiter is a pure delay: the output is the
    /// input shifted by exactly latency() samples, BIT for bit — gain
    /// sits at 1.0 and x * 1.0 is x.
    #[test]
    fn below_the_ceiling_it_is_a_bit_exact_delay() {
        let mut lim = LookaheadLimiter::new();
        lim.prepare(FS, 2.0, 50.0);
        lim.set_ceiling_db(0.0);
        let latency = lim.latency();
        assert_eq!(latency, (0.002 * FS) as usize, "2 ms at 48 k");
        let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 2.0)];

        let input: Vec<f32> = (0..2_048)
            .map(|i| 0.5 * ((i as f32) * 0.13).sin())
            .collect();
        let mut io = input.clone();
        lim.process(&mut io, &mut buf);
        for i in latency..io.len() {
            assert_eq!(
                io[i].to_bits(),
                input[i - latency].to_bits(),
                "sample {i} is not the input delayed by {latency}"
            );
        }
        assert!(lim.reduction_db() < 1e-4, "no reduction below the ceiling");
    }

    /// The release recovers with roughly its stated time constant after
    /// the loud material ends.
    #[test]
    fn the_release_recovers_at_its_stated_rate() {
        let mut lim = LookaheadLimiter::new();
        let release_ms = 100.0;
        lim.prepare(FS, 2.0, release_ms);
        lim.set_ceiling_db(0.0);
        let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 2.0)];

        // Hold it 6 dB into reduction, then go quiet and probe the gain
        // with a tiny carrier.
        let mut loud = vec![2.0f32; 4_800];
        lim.process(&mut loud, &mut buf);
        let gain_held = loud[loud.len() - 1] / 2.0;
        assert!((gain_held - 0.5).abs() < 0.01, "held at half gain");

        let probe_at = (release_ms * 1e-3 * FS) as usize; // one tau
        let mut quiet = vec![0.01f32; probe_at + 480];
        lim.process(&mut quiet, &mut buf);
        let gain_after_tau = quiet[probe_at] / 0.01;
        // One tau of release from 0.5 toward 1.0 is 1 - 0.5/e ≈ 0.816.
        let want = 1.0 - 0.5 * (-1.0f32).exp();
        assert!(
            (gain_after_tau - want).abs() < 0.05,
            "after one tau the gain is {gain_after_tau:.3}, want ~{want:.3}"
        );
    }

    /// Split-block bit-exactness for the limiter: delay state, sliding
    /// max (including a rescan landing mid-split) and gain smoothing all
    /// carry across the cut.
    #[test]
    fn limiter_split_block_is_bit_exact() {
        let input: Vec<f32> = (0..512)
            .map(|i| {
                // Decaying peaks force the rescan path.
                let t = i as f32;
                2.0 * (-t / 200.0).exp() * (t * 0.21).sin()
            })
            .collect();
        let run = |splits: &[usize]| {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, 1.0, 30.0);
            lim.set_ceiling_db(-3.0);
            let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 1.0)];
            let mut io = input.clone();
            let mut at = 0;
            for &cut in splits {
                lim.process(&mut io[at..cut], &mut buf);
                at = cut;
            }
            lim.process(&mut io[at..], &mut buf);
            io
        };
        let whole = run(&[]);
        let split = run(&[100, 313]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "512 must equal 100 + 213 + 199"
        );
    }

    /// Limiter housekeeping: no allocation, any block length, nonsense
    /// settings clamp, and a wrong-sized buffer fails open.
    #[test]
    fn limiter_meets_the_contract() {
        let mut lim = LookaheadLimiter::new();
        lim.prepare(FS, 5.0, 80.0);
        lim.set_ceiling_db(-1.0);
        let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 5.0)];
        let mut io = vec![0.9f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..50 {
                lim.process(&mut io, &mut buf);
            }
        });

        for len in [0usize, 1, 3, 63] {
            let mut io = vec![0.5f32; len];
            lim.process(&mut io, &mut buf);
            assert!(io.iter().all(|s| s.is_finite()), "len {len}");
        }

        for (fs, look, rel, ceil) in [
            (0.0f32, 5.0f32, 80.0f32, -1.0f32),
            (FS, f32::NAN, 80.0, -1.0),
            (FS, 500.0, 80.0, -1.0),
            (FS, 5.0, f32::NAN, f32::NAN),
        ] {
            let mut l = LookaheadLimiter::new();
            l.prepare(fs, look, rel);
            l.set_ceiling_db(ceil);
            let need = l.latency();
            assert!(need >= 1, "lookahead clamps to at least one sample");
            let mut b = vec![0.0f32; delay::buffer_len(need + 4)];
            let mut io: Vec<f32> = (0..256).map(|i| 3.0 * ((i as f32) * 0.3).sin()).collect();
            l.process(&mut io, &mut b);
            assert!(io.iter().all(|s| s.is_finite()), "fs {fs} look {look}");
        }

        // Wrong-sized buffer: dry passes untouched.
        let mut short = vec![0.0f32; 8];
        let mut io = vec![0.7f32; 32];
        lim.process(&mut io, &mut short);
        assert!(io.iter().all(|s| *s == 0.7), "mismatch must fail open");
    }

    // ------------------------------------------- split-block equivalence ---

    #[test]
    fn split_block_is_bit_exact() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin()).collect();
        let mut a = RmsDetector::new();
        a.prepare(FS, 20.0);
        let mut whole = vec![0.0f32; 256];
        a.process(&input, &mut whole);

        let mut b = RmsDetector::new();
        b.prepare(FS, 20.0);
        let mut split = vec![0.0f32; 256];
        b.process(&input[..100], &mut split[..100]);
        b.process(&input[100..], &mut split[100..]);
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
        let mut det = RmsDetector::new();
        det.prepare(FS, 30.0);
        let mut gc = GainComputer::new();
        gc.configure(Mode::Compress, -18.0, 4.0, 6.0);
        let input = vec![0.5f32; 256];
        let mut env = vec![0.0f32; 256];
        let mut gains = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                det.process(&input, &mut env);
                gc.process(&env, &mut gains);
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let mut det = RmsDetector::new();
        det.prepare(FS, 30.0);
        let gc = {
            let mut g = GainComputer::new();
            g.configure(Mode::Expand, -30.0, 4.0, 6.0);
            g
        };
        for len in [0usize, 1, 3, 7, 63, 100] {
            let input = vec![0.5f32; len];
            let mut env = vec![0.0f32; len];
            let mut gains = vec![0.0f32; len];
            det.process(&input, &mut env);
            gc.process(&env, &mut gains);
            assert!(gains.iter().all(|g| g.is_finite()), "len {len}");
        }
    }

    // ------------------------------------------------ silence and nonsense ---

    /// Silence decays to silence; NaN input may pass through the output
    /// but must not lodge in the detector's state; nonsense settings
    /// clamp to inert.
    #[test]
    fn nonsense_cannot_poison_state_or_settings() {
        let mut det = RmsDetector::new();
        det.prepare(FS, 5.0);
        let mut env = vec![0.0f32; 64];
        det.process(&[1.0f32; 64], &mut env);
        let poisoned = [f32::NAN, f32::INFINITY, 0.5, 0.5];
        let mut env4 = vec![0.0f32; 4];
        det.process(&poisoned, &mut env4);
        // After the bad samples, the state keeps working.
        det.process(&[0.5f32; 64], &mut env);
        assert!(
            env.iter().all(|e| e.is_finite()),
            "state survived NaN input"
        );

        let mut gc = GainComputer::new();
        gc.configure(Mode::Compress, f32::NAN, f32::NAN, f32::NAN);
        for level in [-200.0, -20.0, 0.0, f32::NAN, f32::INFINITY] {
            assert!(gc.gain_db(level).is_finite());
            assert!(gc.gain_db(level) <= 0.0);
        }

        for (fs, win) in [(0.0f32, 30.0f32), (-1.0, 30.0), (FS, f32::NAN), (FS, -5.0)] {
            let mut d = RmsDetector::new();
            d.prepare(fs, win);
            let mut e = vec![0.0f32; 32];
            d.process(&[0.7f32; 32], &mut e);
            assert!(e.iter().all(|v| v.is_finite()), "fs {fs} win {win}");
        }
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let input = vec![0.5f32; BLOCK];
        let mut gains = vec![0.0f32; BLOCK];

        let mut det = RmsDetector::new();
        det.prepare(FS, 30.0);
        let mut gc = GainComputer::new();
        gc.configure(Mode::Compress, -18.0, 4.0, 6.0);

        let row = |name: &str, run: &mut dyn FnMut()| {
            for _ in 0..1_000 {
                run();
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run();
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<22} {ns:6.2} ns/sample");
        };
        let mut env2 = vec![0.0f32; BLOCK];
        row("rms detector", &mut || {
            det.process(&input, &mut env2);
            std::hint::black_box(&mut env2);
        });
        let env3 = vec![0.4f32; BLOCK];
        row("gain computer", &mut || {
            gc.process(&env3, &mut gains);
            std::hint::black_box(&mut gains);
        });

        let mut lim = LookaheadLimiter::new();
        lim.prepare(FS, 5.0, 80.0);
        lim.set_ceiling_db(-1.0);
        let mut lbuf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 5.0)];
        let mut steady = vec![0.9f32; BLOCK];
        row("limiter (steady)", &mut || {
            lim.process(&mut steady, &mut lbuf);
            std::hint::black_box(&mut steady);
        });
        let mut lim2 = LookaheadLimiter::new();
        lim2.prepare(FS, 5.0, 80.0);
        lim2.set_ceiling_db(-1.0);
        let mut lbuf2 = vec![0.0f32; LookaheadLimiter::scratch_len(FS, 5.0)];
        let mut i = 0u32;
        let mut ramp = vec![0.0f32; BLOCK];
        row("limiter (worst: decay)", &mut || {
            // A falling ramp keeps expiring the window max — the rescan
            // path, which is the documented worst case. Refilled in
            // place; allocating here would time the allocator.
            for (k, s) in ramp.iter_mut().enumerate() {
                *s = 2.0 / (1.0 + (i + k as u32) as f32 * 1e-3);
            }
            i += BLOCK as u32;
            lim2.process(&mut ramp, &mut lbuf2);
            std::hint::black_box(&mut ramp);
        });
    }
}
