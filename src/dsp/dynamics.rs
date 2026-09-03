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
        self.coeff = super::ramps::one_pole_coeff(1.0 / tau);
    }

    /// Green zone: forget everything heard.
    pub fn reset(&mut self) {
        self.ms = 0.0;
    }

    /// The current level, linear RMS.
    pub fn current(&self) -> f32 {
        self.ms.max(0.0).sqrt()
    }

    /// Red zone: ONE sample in, the running linear RMS out.
    ///
    /// Exposed per sample, and not only as a block, because a FEEDBACK
    /// compressor's detector reads the compressor's own OUTPUT: sample
    /// `n`'s level depends on sample `n-1`'s result, so the loop has to
    /// be closed by the caller one sample at a time and a block-only
    /// kernel would force the topology to be feedforward.
    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x2 = x * x;
        // A NaN in is the caller's NaN and may pass through; it must not
        // lodge in the state and poison every later block.
        if x2.is_finite() {
            self.ms += (x2 - self.ms) * self.coeff;
        }
        self.ms.max(0.0).sqrt()
    }

    /// Red zone: write the running linear RMS of `input` into `env`,
    /// any length (`zip` rules on mismatched slices).
    ///
    /// The block door and [`tick`] are the same arithmetic — this is
    /// written in terms of it, so the two cannot drift.
    ///
    /// [`tick`]: RmsDetector::tick
    pub fn process(&mut self, input: &[f32], env: &mut [f32]) {
        for (x, e) in input.iter().zip(env.iter_mut()) {
            *e = self.tick(*x);
        }
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
            // The dB round trip, once, through the shared base-two
            // conversions — this runs per sample, which is why they are
            // not spelled `powf`.
            let level_db = super::arith::gain_to_db(e.max(1e-6));
            let gd = self.gain_db(level_db);
            *g = super::arith::db_to_gain(gd);
        }
    }
}

// ----------------------------------------------------------- ballistics ---

/// The two time constants auto-release runs in parallel, in ms.
///
/// A fast pole and a slow one, and the recovery is whichever is still
/// holding the gain down — see [`Ballistics`]. The pair is what makes
/// the mode "program dependent" rather than merely slow: a transient
/// leaves the slow pole barely moved, so recovery is the fast figure,
/// while sustained compression walks the slow pole down and the fast one
/// stops mattering.
pub const AUTO_FAST_MS: f32 = 80.0;
pub const AUTO_SLOW_MS: f32 = 2_000.0;

/// The attack and release of a compressor's GAIN — the stage between the
/// gain computer's opinion and the multiply.
///
/// Works in dB on the computer's own sign convention: 0 is no reduction
/// and more negative is more of it. Moving DOWN (further into reduction)
/// is the attack; moving back up is the release.
///
/// # Auto release
///
/// With [`Ballistics::set_auto`] the single release coefficient is
/// replaced by two poles, [`AUTO_FAST_MS`] and [`AUTO_SLOW_MS`], run
/// side by side; the gain is whichever of the two is holding MORE
/// reduction. That one `min` is the whole program dependence:
///
/// - A short burst moves the fast pole a long way and the slow pole
///   hardly at all, so the slow pole is already back near zero and the
///   fast one governs the recovery.
/// - Sustained compression drags the slow pole down with it. When the
///   signal stops, the fast pole springs back but the slow one is still
///   low, so IT governs and the recovery takes seconds.
///
/// The SLOW pole is slow in BOTH directions, and that is the whole
/// mechanism rather than a detail: its lag is what encodes how long the
/// compression has been going on. Made to attack at the attack rate — as
/// this did first — both poles slam to the target together, `min` picks
/// the same number either way, and the mode does nothing at all.
///
/// The grab is still the attack figure regardless, because the fast pole
/// gets there first and `min` takes it.
///
/// State: 12 bytes of signal state + 16 bytes of coefficients.
/// Per-sample cost: 3 mul + 3 add and a compare (fixed release), or
/// twice that plus a `min` (auto).
/// Denormal-safe: the state approaches its target from one side and
/// settles at exactly it; relies on engine FTZ for the approach.
/// In-place safe: yes — `process` reads and writes the same slice.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Ballistics {
    /// Where the gain is now, in dB.
    gain_db: f32,
    /// The two auto-release poles. Unused, and held at `gain_db`, while
    /// the mode is fixed — so switching to auto starts from where the
    /// gain already is rather than from wherever the poles were left.
    fast_db: f32,
    slow_db: f32,
    attack: f32,
    release: f32,
    auto_fast: f32,
    auto_slow: f32,
    auto: bool,
}

impl Default for Ballistics {
    fn default() -> Self {
        Self::new()
    }
}

impl Ballistics {
    pub fn new() -> Self {
        let mut b = Self {
            gain_db: 0.0,
            fast_db: 0.0,
            slow_db: 0.0,
            attack: 1.0,
            release: 1.0,
            auto_fast: 1.0,
            auto_slow: 1.0,
            auto: false,
        };
        b.prepare(48_000.0, 10.0, 100.0);
        b
    }

    /// Green zone: sample rate and the two times, in ms (63% convergence
    /// times, the figure every compressor's panel means).
    ///
    /// Non-finite or sub-sample times mean INSTANT, which is what a
    /// 0.01 ms attack at 48 kHz genuinely is — half a sample.
    pub fn prepare(&mut self, sample_rate: f32, attack_ms: f32, release_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            return;
        };
        self.attack = coeff(fs, attack_ms);
        self.release = coeff(fs, release_ms);
        self.auto_fast = coeff(fs, AUTO_FAST_MS);
        self.auto_slow = coeff(fs, AUTO_SLOW_MS);
    }

    /// Green zone: whether the release is program dependent.
    ///
    /// Carries the current gain into both poles, so the mode can be
    /// switched mid-signal without the gain jumping.
    pub fn set_auto(&mut self, auto: bool) {
        if auto != self.auto {
            self.fast_db = self.gain_db;
            self.slow_db = self.gain_db;
        }
        self.auto = auto;
    }

    pub fn auto(&self) -> bool {
        self.auto
    }

    /// Green zone: put the gain AT `db` with no glide.
    ///
    /// What a seek wants — a transport jump must not glide the old
    /// position's compression into the new one — and what a test wants
    /// when it needs to start from a known place.
    pub fn set_now(&mut self, db: f32) {
        let db = if db.is_finite() { db.min(0.0) } else { 0.0 };
        self.gain_db = db;
        self.fast_db = db;
        self.slow_db = db;
    }

    /// Green zone: back to no reduction, coefficients kept.
    pub fn reset(&mut self) {
        self.gain_db = 0.0;
        self.fast_db = 0.0;
        self.slow_db = 0.0;
    }

    /// The gain right now, in dB.
    pub fn current(&self) -> f32 {
        self.gain_db
    }

    /// Latency: none. Stated because the contract requires an answer.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: ONE sample of ballistics — the target gain in dB, the
    /// smoothed gain out.
    ///
    /// Exposed per sample, and not only as a block, because a FEEDBACK
    /// compressor cannot use a block form at all: its detector reads the
    /// output, so sample `n`'s gain depends on sample `n-1`'s result and
    /// the loop has to be closed by the caller, one sample at a time. A
    /// block-only kernel would force the topology to be feedforward.
    #[inline(always)]
    pub fn tick(&mut self, target_db: f32) -> f32 {
        // A NaN target is the caller's NaN. It may pass through the
        // arithmetic; it must never lodge in the state and poison every
        // sample after it.
        let target = if target_db.is_finite() {
            target_db
        } else {
            0.0
        };
        if self.auto {
            // The fast pole grabs at the attack figure and lets go at
            // its own; the slow pole moves at its own rate BOTH ways,
            // which is what makes it a memory of how long this has been
            // going on rather than a second copy of the fast one.
            let fast = if target < self.fast_db {
                self.attack
            } else {
                self.auto_fast
            };
            self.fast_db += (target - self.fast_db) * fast;
            self.slow_db += (target - self.slow_db) * self.auto_slow;
            // Whichever pole is holding MORE reduction wins. This one
            // line is the program dependence.
            self.gain_db = self.fast_db.min(self.slow_db);
        } else {
            let c = if target < self.gain_db {
                self.attack
            } else {
                self.release
            };
            self.gain_db += (target - self.gain_db) * c;
        }
        self.gain_db
    }

    /// Red zone: a block of target gains in dB, smoothed in place, any
    /// length. The feedforward form; a feedback caller wants [`tick`].
    ///
    /// [`tick`]: Ballistics::tick
    pub fn process(&mut self, gain_db: &mut [f32]) {
        for g in gain_db.iter_mut() {
            *g = self.tick(*g);
        }
    }
}

/// The one-pole coefficient for a 63% convergence time in ms. Sub-sample
/// or nonsense times mean instant, which is what they are.
fn coeff(sample_rate: f32, time_ms: f32) -> f32 {
    if !time_ms.is_finite() || time_ms <= 0.0 {
        return 1.0;
    }
    let samples = time_ms * 1e-3 * sample_rate;
    if samples <= 1.0 {
        return 1.0;
    }
    let c = super::ramps::one_pole_coeff(1.0 / samples);
    if c.is_finite() {
        c.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

// ---------------------------------------------------- lookahead limiter ---

/// The longest lookahead accepted, in milliseconds. Past this the delay
/// stops being "anticipation" and starts being latency nobody wanted.
pub const LOOKAHEAD_MAX_MS: f32 = 20.0;

/// Age stamps are kept to 24 bits, the widest integer an f32 holds
/// exactly. Any window is a rounding error beside that, so subtracting
/// modulo 2^24 recovers the true age however often the counter wraps.
const STAMP_MASK: u32 = 0x00FF_FFFF;

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
/// The sliding max is a MONOTONIC WEDGE: a deque of candidates kept in
/// non-increasing order, holding exactly those samples that could still
/// become the window's maximum. A new sample evicts everything it beats
/// from the back (they can never win again while it is in the window),
/// and the front retires once it has aged past `L`. The front IS the
/// maximum, read in one load. Every sample is pushed once and dropped
/// once, so the total work is linear no matter what the signal does.
///
/// This replaced a running (value, age) pair that RESCANNED the window
/// whenever the reigning peak expired. That was amortised O(1) too, but
/// only on material that keeps producing new peaks: on anything
/// monotonically decaying — a fade, a reverb tail, the decay of any
/// struck note — the peak expired every single sample and the rescan
/// ran every single sample. Measured at a 5 ms lookahead, release: 6 ns
/// steady against 249 ns decaying, a 40x cliff that arrived with
/// ordinary music. The wedge is 4.4 ns steady and 4.5 ns decaying — the
/// cliff is not smaller, it is gone, which is the property that matters
/// in a callback with a deadline.
///
/// The deque's STORAGE is the caller's, like every other buffer here:
/// [`Self::scratch_len`] sizes the key buffer for the delay line and
/// the wedge together, and the kernel splits it. Only the two ends and
/// a stamp counter are state, which is what keeps this struct `Copy`.
/// The two `while`-shaped loops are written as bounded `for`s so the
/// no-unbounded-loops rule is satisfied syntactically and not by
/// argument.
///
/// State: ~64 bytes plus the caller-owned buffers ([`Self::scratch_len`]).
/// Denormal-safe: gain and envelope live near 1.0; the delayed audio
/// relies on engine FTZ like any buffer.
/// In-place safe: yes.
/// Latency: `lookahead` samples — REPORTED, because this delay is not
/// the effect, it is the price of anticipation, and PDC must know.
#[derive(Debug, Clone, Copy)]
pub struct LookaheadLimiter {
    /// The detector's line. In mono this holds the audio itself; in
    /// LINKED STEREO it holds the KEY — `max(|l|, |r|)` — because the
    /// sliding-window rescan has to be able to recover the peak of
    /// whatever the gain was computed from, and that is the key rather
    /// than either channel.
    line: DelayLine,
    /// The audio lines used only by [`process_linked`](Self::process_linked).
    /// Two, because the key line is spoken for.
    line_l: DelayLine,
    line_r: DelayLine,
    /// Lookahead in samples.
    lookahead: usize,
    ceiling: f32,
    attack_coeff: f32,
    release_coeff: f32,
    /// Smoothed gain state.
    gain: f32,
    /// The current sliding-window maximum of |key| — the front of the
    /// wedge, cached so the gain maths reads a field and not a slice.
    win_max: f32,
    /// The monotonic wedge's ends, indexing the caller's wedge storage.
    /// The DEQUE lives in the scratch buffer; only these ends and the
    /// stamp counter are state, which is what keeps this struct `Copy`.
    wedge_head: usize,
    wedge_tail: usize,
    /// Samples pushed, for the age stamps. Wraps, and the comparison
    /// wraps with it.
    pushed: u32,
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
            line_l: DelayLine::new(),
            line_r: DelayLine::new(),
            lookahead: 1,
            ceiling: 1.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            gain: 1.0,
            win_max: 0.0,
            wedge_head: 0,
            wedge_tail: 0,
            pushed: 0,
            reduction_db: 0.0,
        }
    }

    /// Where the smoothed gain stands right now, in dB, at or below
    /// zero. Telemetry: the block's WORST reduction and where the gain
    /// stands at the block's end are different questions, and a meter
    /// that shows recovery needs the second one.
    pub fn gain_db(&self) -> f32 {
        20.0 * self.gain.max(1e-6).log10()
    }

    /// The loudest sample anywhere inside the lookahead window, in
    /// dBFS, floored. Telemetry: the one figure on the desk that comes
    /// from AHEAD of the playhead, which is what lets a surface draw
    /// lookahead rather than only its aftermath.
    pub fn window_peak_db(&self) -> f32 {
        if self.win_max > 1e-6 {
            20.0 * self.win_max.log10()
        } else {
            -120.0
        }
    }

    /// Floats the caller-owned buffer needs for a lookahead at a rate.
    /// Green zone, compile-time sizing.
    pub fn scratch_len(sample_rate: f32, lookahead_ms: f32) -> usize {
        Self::scratch_len_samples(Self::samples_for(sample_rate, lookahead_ms))
    }

    /// Floats each caller-owned buffer needs for a lookahead stated in
    /// SAMPLES. Green zone, compile-time sizing.
    ///
    /// One figure for all three buffers, and it is the largest of them:
    /// the KEY buffer carries the detector's wedge behind its delay
    /// line, while the two audio lines use only the line part. Sizing
    /// them all the same wastes a few hundred floats per limiter and
    /// means no caller has to know which buffer is which.
    pub fn scratch_len_samples(lookahead: usize) -> usize {
        Self::line_len_samples(lookahead) + Self::wedge_len_samples(lookahead)
    }

    /// The delay line's own length — the prefix of any of the buffers.
    fn line_len_samples(lookahead: usize) -> usize {
        delay::buffer_len(lookahead.max(1) + 4)
    }

    /// Deque slots the wedge can ever need: one per sample of the
    /// window, rounded up to a power of two so the ends mask instead of
    /// dividing, plus one so `head == tail` can only mean EMPTY.
    fn wedge_cap(lookahead: usize) -> usize {
        (lookahead.max(1) + 2).next_power_of_two()
    }

    /// Floats the wedge needs: two per slot, a value and an age stamp.
    pub fn wedge_len_samples(lookahead: usize) -> usize {
        2 * Self::wedge_cap(lookahead)
    }

    /// Split a key buffer into its line prefix and its wedge tail.
    /// `None` if the caller sized it for the line alone — fail open.
    #[inline(always)]
    fn split_key(lookahead: usize, buf: &mut [f32]) -> Option<(&mut [f32], &mut [f32])> {
        if buf.len() < Self::scratch_len_samples(lookahead) {
            return None;
        }
        Some(buf.split_at_mut(Self::line_len_samples(lookahead)))
    }

    /// An audio line's prefix inside a buffer sized by
    /// [`scratch_len_samples`](Self::scratch_len_samples).
    #[inline(always)]
    fn line_of(lookahead: usize, buf: &mut [f32]) -> Option<&mut [f32]> {
        buf.get_mut(..Self::line_len_samples(lookahead))
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
        let lookahead = Self::samples_for(sample_rate, lookahead_ms);
        self.prepare_samples(sample_rate, lookahead, release_ms);
    }

    /// Green zone: the same, with the lookahead stated in SAMPLES.
    ///
    /// The entry point for a caller that has to REPORT its latency. A
    /// device inside a compensated graph must tell the graph its delay,
    /// and the graph's latency model counts samples without knowing the
    /// sample rate — so a lookahead given in milliseconds has to be
    /// converted somewhere, and every place that conversion happens twice
    /// is a place the two answers can round differently. Given the count
    /// directly, the figure the device reports and the figure the delay
    /// line actually holds are the same integer by construction.
    pub fn prepare_samples(&mut self, sample_rate: f32, lookahead: usize, release_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.lookahead = lookahead.max(1);
        self.line.prepare(self.lookahead + 4);
        self.line_l.prepare(self.lookahead + 4);
        self.line_r.prepare(self.lookahead + 4);
        // Attack rides the lookahead: a quarter of the window reaches
        // ~98% settled by the time the peak arrives, and the hard `min`
        // in the loop covers the rest.
        let attack_tau = (self.lookahead as f32 * 0.25).max(1.0);
        self.attack_coeff = super::ramps::one_pole_coeff(1.0 / attack_tau);
        let rel = if release_ms.is_finite() {
            release_ms.clamp(1.0, 5_000.0)
        } else {
            100.0
        };
        self.release_coeff = super::ramps::one_pole_coeff(1.0 / (rel * 1e-3 * fs));
        self.reset();
    }

    /// Green zone: the release time alone, in milliseconds.
    ///
    /// Separate from [`prepare`](Self::prepare) because prepare RESETS,
    /// and a release knob that cleared the delay line every time it moved
    /// would click on every turn. Nothing here touches the line, the
    /// window or the gain, so a release changed mid-stream is heard as
    /// the recovery bending rather than as a discontinuity.
    ///
    /// The attack is deliberately NOT settable: it rides the lookahead,
    /// and the lookahead is this device's reported latency. A knob that
    /// moved either would slide the track in time as it turned.
    pub fn set_release_ms(&mut self, sample_rate: f32, release_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let rel = if release_ms.is_finite() {
            release_ms.clamp(1.0, 5_000.0)
        } else {
            100.0
        };
        self.release_coeff = super::ramps::one_pole_coeff(1.0 / (rel * 1e-3 * fs));
    }

    /// Green zone: the ceiling, in dBFS (≤ 0 in any sane session; junk
    /// clamps to unity).
    pub fn set_ceiling_db(&mut self, db: f32) {
        let db = if db.is_finite() {
            db.clamp(-60.0, 0.0)
        } else {
            0.0
        };
        self.ceiling = super::arith::db_to_gain(db);
    }

    /// Green zone: forget everything (caller clears the buffer slice).
    pub fn reset(&mut self) {
        self.line.reset();
        self.gain = 1.0;
        self.win_max = 0.0;
        self.wedge_head = 0;
        self.wedge_tail = 0;
        self.pushed = 0;
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

    /// One detector step: push into the key line, slide the window, and
    /// return the gain this sample must be multiplied by.
    ///
    /// Shared by the mono and linked paths so the two can never disagree
    /// about what "limiting" means. `pushed` goes into the key line;
    /// `ax` is its magnitude, passed separately because mono stores the
    /// SIGNED audio there (the line doubles as its delay) while linked
    /// stores an already-rectified key.
    #[inline(always)]
    fn advance(&mut self, key_buf: &mut [f32], wedge: &mut [f32], pushed: f32, ax: f32) -> f32 {
        let window = (self.lookahead + 1) as u32;
        let cap = Self::wedge_cap(self.lookahead);
        let mask = cap - 1;
        self.line.push(key_buf, pushed);
        self.pushed = self.pushed.wrapping_add(1);
        let now = self.pushed & STAMP_MASK;

        // Anything the new sample matches or beats can never be the
        // window maximum again while the new one is still in it, so it
        // leaves now. Each sample is pushed once and dropped once, which
        // is what makes the whole thing amortised O(1) — the loops are
        // bounded by `cap` so the bound is syntactic, not an argument.
        for _ in 0..cap {
            if self.wedge_head == self.wedge_tail {
                break;
            }
            let back = (self.wedge_tail + mask) & mask;
            let Some(v) = wedge.get(2 * back).copied() else {
                break;
            };
            if v > ax {
                break;
            }
            self.wedge_tail = back;
        }
        if let Some(slot) = wedge.get_mut(2 * self.wedge_tail) {
            *slot = ax;
        }
        if let Some(slot) = wedge.get_mut(2 * self.wedge_tail + 1) {
            // Stamps are 24-bit, which an f32 holds exactly, and the age
            // below subtracts modulo the same 24 bits — so the counter
            // wrapping costs nothing.
            *slot = now as f32;
        }
        self.wedge_tail = (self.wedge_tail + 1) & mask;

        // Retire the front once it has aged out of the window.
        for _ in 0..cap {
            if self.wedge_head == self.wedge_tail {
                break;
            }
            let Some(stamp) = wedge.get(2 * self.wedge_head + 1).copied() else {
                break;
            };
            let age = now.wrapping_sub(stamp as u32) & STAMP_MASK;
            if age < window {
                break;
            }
            self.wedge_head = (self.wedge_head + 1) & mask;
        }
        // The front IS the maximum: the wedge is non-increasing, and the
        // sample just pushed guarantees it is not empty.
        self.win_max = wedge.get(2 * self.wedge_head).copied().unwrap_or(0.0);

        // The gain that GUARANTEES the delayed sample fits.
        let required = if self.win_max > self.ceiling {
            self.ceiling / self.win_max
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
        self.gain
    }

    /// Red zone: limit in place, any length.
    pub fn process(&mut self, io: &mut [f32], buf: &mut [f32]) {
        let Some((buf, wedge)) = Self::split_key(self.lookahead, buf) else {
            return; // fail open, undelayed — loud enough to notice
        };
        if !self.line.matches(buf) {
            return;
        }
        // The meter wants the block's WORST reduction, and reduction is
        // a decreasing function of gain — so the deepest reduction is
        // simply the smallest gain, and the logarithm belongs out here
        // rather than on every sample. Same number, one call.
        let mut lowest = 1.0f32;
        for s in io.iter_mut() {
            let x = *s;
            let ax = if x.is_finite() { x.abs() } else { 0.0 };
            let gain = self.advance(buf, wedge, x, ax);
            *s = self.line.tap(buf, self.lookahead + 1) * gain;
            if gain < lowest {
                lowest = gain;
            }
        }
        self.reduction_db = -super::arith::gain_to_db(lowest.max(1e-6));
    }

    /// Red zone: limit a stereo pair with ONE linked gain, in place.
    ///
    /// # Why linking is not optional
    ///
    /// Two independent limiters on a stereo pair pull their channels down
    /// by different amounts, and a level difference between left and
    /// right IS the stereo image. Un-linked limiting therefore makes the
    /// image lurch toward whichever side is momentarily quieter, on every
    /// peak — the wander that gives away a mix limited by two mono
    /// plugins. One detector fed `max(|l|, |r|)` and one gain applied to
    /// both keeps the image exactly where it was and simply makes it
    /// quieter, which is the only thing a limiter should be doing to it.
    ///
    /// Three buffers, each exactly
    /// [`scratch_len`](Self::scratch_len) long and zeroed: the key line
    /// and one audio line per channel. `l` and `r` must be the same
    /// length; a mismatch fails open rather than limiting half a pair.
    pub fn process_linked(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        key_buf: &mut [f32],
        buf_l: &mut [f32],
        buf_r: &mut [f32],
    ) {
        let look = self.lookahead;
        let (Some((key_buf, wedge)), Some(buf_l), Some(buf_r)) = (
            Self::split_key(look, key_buf),
            Self::line_of(look, buf_l),
            Self::line_of(look, buf_r),
        ) else {
            return; // fail open, undelayed — loud enough to notice
        };
        if l.len() != r.len()
            || !self.line.matches(key_buf)
            || !self.line_l.matches(buf_l)
            || !self.line_r.matches(buf_r)
        {
            return;
        }
        let mut lowest = 1.0f32;
        let behind = self.lookahead + 1;
        for (left, right) in l.iter_mut().zip(r.iter_mut()) {
            let (xl, xr) = (*left, *right);
            let al = if xl.is_finite() { xl.abs() } else { 0.0 };
            let ar = if xr.is_finite() { xr.abs() } else { 0.0 };
            self.line_l.push(buf_l, xl);
            self.line_r.push(buf_r, xr);
            // The KEY is the louder side, so the pair is held below the
            // ceiling by whichever channel is closest to it.
            let key = al.max(ar);
            let gain = self.advance(key_buf, wedge, key, key);
            *left = self.line_l.tap(buf_l, behind) * gain;
            *right = self.line_r.tap(buf_r, behind) * gain;
            if gain < lowest {
                lowest = gain;
            }
        }
        // One logarithm per block; see the mono path.
        self.reduction_db = -super::arith::gain_to_db(lowest.max(1e-6));
    }
}

// ------------------------------------------------------ slew brighten ---

/// Slew-dependent high-frequency lift: brightness that arrives with the
/// transients and leaves with them.
///
/// # What this is for
///
/// A limiter DULLS. Its gain reduction is fastest exactly when a
/// transient arrives, so the moment with the most high-frequency energy
/// is the moment the gain is being pulled down hardest — the edge comes
/// off the attack and the result reads as flat and slightly behind the
/// beat. Turning up a shelf afterwards gets the brightness back but puts
/// it everywhere, including on the sustained material that was never
/// dulled, which is how a limited mix ends up harsh AND flat at once.
///
/// This puts it back where it was taken from. The lift is driven by the
/// signal's SLEW RATE — how fast it is actually moving — so a snare edge
/// gets it and a held pad does not.
///
/// # The shape
///
/// ```text
/// edge      = x - lowpass(x)              the band that gets lifted
/// slew      = |x[n] - x[n-1]|             how fast it is moving
/// env       = follow(slew)                fast attack, slow release
/// lift      = env / (env + knee)          0..1, saturating
/// out       = x + amount * lift * edge
/// ```
///
/// `lift` is a saturating ratio rather than a scaling, and that is the
/// safety property: it approaches 1 and never reaches it, whatever the
/// input does, so no signal can make this kernel multiply its own output.
/// A differentiator-based brightener without that bound is a device that
/// screams on a square wave.
///
/// [`KNEE_HZ`](Self::KNEE_HZ) sets where "fast" starts: a full-scale sine
/// at that frequency sits at half lift.
///
/// State: 32 bytes. Per-sample cost: 4 mul + 5 add + 1 divide.
/// Denormal-safe: the envelope floors to exact zero below
/// [`ENV_FLOOR`](Self::ENV_FLOOR), so a decaying tail leaves the denormal
/// range rather than crawling through it; the edge band relies on engine
/// FTZ like any one-pole.
/// In-place safe: yes.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct SlewBrighten {
    /// The one-pole whose output is subtracted to leave the edge band.
    lp: f32,
    g: f32,
    /// Previous input, for the slew.
    prev: f32,
    env: f32,
    attack: f32,
    release: f32,
    knee: f32,
    amount: f32,
}

impl Default for SlewBrighten {
    fn default() -> Self {
        Self::new()
    }
}

impl SlewBrighten {
    /// Where "fast" starts, in Hz: a full-scale sine here sits at half
    /// lift.
    ///
    /// 3 kHz, which is presence rather than air. Lower and sustained
    /// mid-range material starts triggering the lift, which is the
    /// harshness this kernel exists to avoid; much higher and only
    /// cymbals ever reach it, which is a control that does nothing on
    /// most programme.
    pub const KNEE_HZ: f32 = 3_000.0;

    /// The envelope's attack, in ms. Fast enough to be up before the edge
    /// it is lifting has finished.
    pub const ATTACK_MS: f32 = 0.2;

    /// The envelope's release, in ms. Slow enough that the lift does not
    /// chatter between the cycles of a periodic signal.
    pub const RELEASE_MS: f32 = 40.0;

    /// Below this the envelope is exact zero. Bounds the tail and keeps
    /// the state out of the denormal range.
    pub const ENV_FLOOR: f32 = 1.0e-9;

    pub fn new() -> Self {
        let mut b = Self {
            lp: 0.0,
            g: 0.0,
            prev: 0.0,
            env: 0.0,
            attack: 0.0,
            release: 0.0,
            knee: 0.0,
            amount: 0.0,
        };
        b.prepare(48_000.0, 1_500.0, 0.0);
        b
    }

    /// Green zone: sample rate, the corner of the edge band, and how much
    /// of it a fully-triggered lift adds.
    ///
    /// `amount` 0 is an exact wire — bit-exact, not merely quiet — so a
    /// device can leave this permanently in its path and a knob at zero
    /// costs nothing but the multiply.
    pub fn prepare(&mut self, sample_rate: f32, corner_hz: f32, amount: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let corner = if corner_hz.is_finite() {
            corner_hz.clamp(20.0, fs * 0.45)
        } else {
            1_500.0
        };
        // One-pole coefficient for the edge band's corner.
        self.g = super::ramps::one_pole_coeff(core::f32::consts::TAU * corner / fs);
        self.attack = super::ramps::one_pole_coeff(1.0 / (Self::ATTACK_MS * 1e-3 * fs).max(1.0));
        self.release = super::ramps::one_pole_coeff(1.0 / (Self::RELEASE_MS * 1e-3 * fs).max(1.0));
        // The per-sample slew of a full-scale sine at KNEE_HZ.
        self.knee = (core::f32::consts::TAU * Self::KNEE_HZ / fs).max(1e-6);
        self.amount = if amount.is_finite() {
            amount.clamp(0.0, 4.0)
        } else {
            0.0
        };
    }

    /// Green zone: change only how much is added, keeping the state.
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = if amount.is_finite() {
            amount.clamp(0.0, 4.0)
        } else {
            0.0
        };
    }

    /// Green zone: forget the history, keep the settings.
    pub fn reset(&mut self) {
        self.lp = 0.0;
        self.prev = 0.0;
        self.env = 0.0;
    }

    /// The current lift, `0..1` — telemetry for a display that wants to
    /// show when the brightener is working.
    pub fn lift(&self) -> f32 {
        self.env / (self.env + self.knee)
    }

    /// Red zone: brighten in place, any length.
    pub fn process(&mut self, io: &mut [f32]) {
        for sample in io.iter_mut() {
            let x = if sample.is_finite() { *sample } else { 0.0 };

            // The edge band: everything the one-pole does not follow.
            let edge = x - self.lp;
            self.lp += self.g * edge;

            // How fast the input is moving, followed with a fast attack
            // and a slow release.
            let slew = (x - self.prev).abs();
            self.prev = x;
            let coeff = if slew > self.env {
                self.attack
            } else {
                self.release
            };
            self.env += (slew - self.env) * coeff;
            if self.env < Self::ENV_FLOOR {
                self.env = 0.0;
            }

            // Saturating, so nothing the input does can run away.
            let lift = self.env / (self.env + self.knee);
            *sample = x + self.amount * lift * edge;
        }
    }
}

// ------------------------------------------------------ transient split ---

/// The fast envelope's ballistics. Short enough to be ON the attack
/// rather than after it, and a release brisk enough that the gap has
/// closed before the next sixteenth arrives.
const SPLIT_FAST_ATTACK_MS: f32 = 0.05;
const SPLIT_FAST_RELEASE_MS: f32 = 14.0;
/// The slow envelope only ever RELEASES slowly; its attack is the knob.
const SPLIT_SLOW_RELEASE_MS: f32 = 180.0;
/// The window the caller may ask for, in milliseconds.
pub const SPLIT_WINDOW_MIN_MS: f32 = 1.0;
pub const SPLIT_WINDOW_MAX_MS: f32 = 60.0;
/// Below this the fast envelope is silence and the ratio is meaningless.
const SPLIT_FLOOR: f32 = 1e-6;

/// How much of each sample belongs to the STRIKE rather than the BODY.
///
/// Two envelope followers watch one signal at different speeds. A hit
/// moves the fast one at once and the slow one barely at all, so the GAP
/// between them is the attack; once the hit settles the two agree and the
/// gap closes. What this writes is that gap, divided by the fast envelope
/// so it does not depend on how loud the hit was — a ghost note and a
/// rimshot open it the same amount. That is the difference between a
/// transient shaper and a compressor, and it is the property the tests
/// hold this to.
///
/// The weight is a WEIGHT: this kernel applies no gain of its own, and
/// the caller multiplies whatever shaping it wants by it. One job each,
/// per the contract — which is also why the same kernel serves an attack
/// boost, a sustain cut, and a colour that only tracks the strike.
///
/// State: 24 bytes. Per-sample cost: two compare-and-FMA pairs, one
/// divide.
/// Denormal-safe: relies on engine FTZ — both envelopes decay through the
/// denormal range on silence, and the floor keeps the ratio finite.
/// In-place safe: n/a — separate in/out slices by signature.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct TransientSplit {
    fast: f32,
    slow: f32,
    fast_attack: f32,
    fast_release: f32,
    slow_attack: f32,
    slow_release: f32,
}

impl Default for TransientSplit {
    fn default() -> Self {
        Self::new()
    }
}

impl TransientSplit {
    pub fn new() -> Self {
        Self {
            fast: 0.0,
            slow: 0.0,
            fast_attack: 1.0,
            fast_release: 1.0,
            slow_attack: 1.0,
            slow_release: 1.0,
        }
    }

    /// Green zone: `window_ms` is how long a strike is allowed to last —
    /// the slow envelope's attack, and the only ballistic with a knob on
    /// it. Everything else is fixed, because a transient shaper with four
    /// time controls is a compressor with extra steps.
    pub fn prepare(&mut self, sample_rate: f32, window_ms: f32) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let window = if window_ms.is_finite() {
            window_ms.clamp(SPLIT_WINDOW_MIN_MS, SPLIT_WINDOW_MAX_MS)
        } else {
            12.0
        };
        let coeff = |ms: f32| {
            let samples = ms * 1e-3 * fs;
            if samples >= 1.0 {
                super::ramps::one_pole_coeff(1.0 / samples)
            } else {
                1.0
            }
        };
        self.fast_attack = coeff(SPLIT_FAST_ATTACK_MS);
        self.fast_release = coeff(SPLIT_FAST_RELEASE_MS);
        self.slow_attack = coeff(window);
        self.slow_release = coeff(SPLIT_SLOW_RELEASE_MS);
    }

    /// Green zone: forget the signal, keep the ballistics.
    pub fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
    }

    /// The weight the last processed sample produced — for a meter.
    pub fn current(&self) -> f32 {
        let denom = self.fast.max(SPLIT_FLOOR);
        ((self.fast - self.slow) / denom).clamp(0.0, 1.0)
    }

    /// Red zone: write `key`'s strike weight into `weight`, in `0..=1`,
    /// truncating to the shorter slice.
    pub fn process(&mut self, key: &[f32], weight: &mut [f32]) {
        for (w, x) in weight.iter_mut().zip(key.iter()) {
            let level = if x.is_finite() { x.abs() } else { 0.0 };
            let fc = if level > self.fast {
                self.fast_attack
            } else {
                self.fast_release
            };
            self.fast = fc.mul_add(level - self.fast, self.fast);
            // The slow envelope follows the FAST ONE, not the raw signal.
            // Chasing the signal directly, it settles somewhere between
            // the mean and the peak while the fast envelope sits on the
            // peak — so a steady tone never closes the gap and reads as a
            // permanent 11% strike. Following the fast envelope, the two
            // agree exactly on anything steady, and open only when the
            // envelope itself MOVES. Which is the definition.
            let sc = if self.fast > self.slow {
                self.slow_attack
            } else {
                self.slow_release
            };
            self.slow = sc.mul_add(self.fast - self.slow, self.slow);
            // Normalising by the fast envelope is what makes this a
            // TRANSIENT detector and not a level detector.
            let denom = self.fast.max(SPLIT_FLOOR);
            *w = ((self.fast - self.slow) / denom).clamp(0.0, 1.0);
        }
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

    // ----------------------------------------------------- ballistics ---

    /// Attack and release reach 63% of the way in the time they claim —
    /// the meaning of every attack and release figure on every panel.
    #[test]
    fn ballistics_converge_in_the_time_they_state() {
        let at = |ms: f32, samples: usize, from: f32, to: f32| {
            let mut b = Ballistics::new();
            b.prepare(FS, ms, ms);
            b.set_now(from);
            let mut buf = vec![to; samples];
            b.process(&mut buf);
            b.current()
        };
        for ms in [1.0f32, 10.0, 100.0] {
            let n = (ms * 1e-3 * FS) as usize;
            // Attack: down into reduction.
            let after = at(ms, n, 0.0, -12.0);
            let want = -12.0 * 0.632;
            assert!(
                (after - want).abs() < 0.5,
                "{ms} ms attack reached {after:+.2} dB, wanted {want:+.2}"
            );
            // Release: back up out of it.
            let after = at(ms, n, -12.0, 0.0);
            let want = -12.0 + 12.0 * 0.632;
            assert!(
                (after - want).abs() < 0.5,
                "{ms} ms release reached {after:+.2} dB, wanted {want:+.2}"
            );
        }
        // A sub-sample time is instant, which is what 0.01 ms at 48 kHz
        // genuinely is — the Glue's fastest attack is half a sample.
        let mut b = Ballistics::new();
        b.prepare(FS, 0.01, 100.0);
        let mut buf = [-20.0f32; 1];
        b.process(&mut buf);
        assert!(
            (b.current() + 20.0).abs() < 1e-6,
            "0.01 ms must land at once"
        );
    }

    /// THE test auto release exists for: recovery after a short burst is
    /// FAST, recovery after sustained compression is SLOW — from the
    /// same settings, with nothing changed but how long the signal held
    /// the gain down.
    ///
    /// A fixed release cannot tell the two apart, and that difference is
    /// the whole reason the mode is on the panel.
    #[test]
    fn auto_release_recovers_faster_after_a_burst_than_after_a_hold() {
        // How much reduction is left `after` samples of silence, having
        // held `held` samples of -12 dB first.
        let recover = |auto: bool, held: usize, after: usize| {
            let mut b = Ballistics::new();
            b.prepare(FS, 1.0, 300.0);
            b.set_auto(auto);
            let mut down = vec![-12.0f32; held];
            b.process(&mut down);
            let grabbed = b.current();
            let mut up = vec![0.0f32; after];
            b.process(&mut up);
            (grabbed, b.current())
        };
        let short = (0.02 * FS) as usize; // 20 ms — a transient
        let long = (2.0 * FS) as usize; // 2 s — a sustained passage
        let wait = (0.4 * FS) as usize; // 400 ms of quiet afterwards

        // Both cases must actually have grabbed, or the comparison below
        // is comparing two nothings.
        let (grabbed_short, left_short) = recover(true, short, wait);
        let (grabbed_long, left_long) = recover(true, long, wait);
        assert!(
            grabbed_short < -9.0 && grabbed_long < -11.0,
            "the attack must reach the target first: {grabbed_short:+.2}, {grabbed_long:+.2}"
        );
        assert!(
            left_short > left_long + 2.0,
            "auto must let go faster after a burst ({left_short:+.2} dB left) \
             than after a hold ({left_long:+.2} dB left)"
        );
        assert!(
            left_short > -1.0,
            "a transient should be nearly recovered: {left_short:+.2} dB"
        );

        // A FIXED release cannot tell them apart — same settings, same
        // recovery, whatever came before.
        let (_, fixed_short) = recover(false, short, wait);
        let (_, fixed_long) = recover(false, long, wait);
        assert!(
            (fixed_short - fixed_long).abs() < 0.2,
            "a fixed release must not be program dependent: \
             {fixed_short:+.2} against {fixed_long:+.2}"
        );
    }

    /// Switching the mode mid-signal must not jump the gain — the poles
    /// pick up where the gain already is.
    #[test]
    fn switching_the_release_mode_does_not_jump_the_gain() {
        let mut b = Ballistics::new();
        b.prepare(FS, 1.0, 300.0);
        let mut down = vec![-9.0f32; (0.5 * FS) as usize];
        b.process(&mut down);
        let before = b.current();
        b.set_auto(true);
        assert_eq!(b.current(), before, "the switch itself must move nothing");
        let mut one = [0.0f32; 1];
        b.process(&mut one);
        assert!(
            (b.current() - before).abs() < 0.05,
            "one sample after the switch: {:+.3} from {before:+.3}",
            b.current()
        );
        b.set_auto(false);
        assert!((b.current() - before).abs() < 0.05);
    }

    /// A NaN target may pass through the arithmetic; it must never lodge
    /// in the state. Nor may a nonsense sample rate or time.
    #[test]
    fn ballistics_cannot_be_poisoned() {
        let mut b = Ballistics::new();
        b.prepare(FS, 5.0, 200.0);
        let mut buf = vec![-6.0f32; 64];
        b.process(&mut buf);
        let healthy = b.current();
        assert!(healthy.is_finite());

        let mut poison = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        b.process(&mut poison);
        assert!(
            b.current().is_finite(),
            "a NaN target lodged in the state: {}",
            b.current()
        );

        for (fs, attack, release) in [
            (0.0f32, 5.0f32, 200.0f32),
            (FS, -1.0, 200.0),
            (FS, f32::NAN, 200.0),
            (FS, 5.0, f32::INFINITY),
            (-FS, 5.0, 200.0),
        ] {
            let mut b = Ballistics::new();
            b.prepare(fs, attack, release);
            for auto in [false, true] {
                b.set_auto(auto);
                let mut buf = vec![-6.0f32; 32];
                b.process(&mut buf);
                assert!(
                    buf.iter().all(|g| g.is_finite()),
                    "fs {fs} attack {attack} release {release} auto {auto}"
                );
                assert_eq!(b.latency(), 0);
            }
        }

        // Silence in — a zero target — settles at exactly zero rather
        // than crawling forever through the denormals.
        let mut b = Ballistics::new();
        b.prepare(FS, 1.0, 1.0);
        let mut buf = vec![0.0f32; 256];
        b.process(&mut buf);
        assert_eq!(b.current(), 0.0, "no reduction asked for, none applied");
    }

    /// The per-sample door and the block door are the same kernel — a
    /// feedback caller must not get different ballistics from a
    /// feedforward one.
    #[test]
    fn the_per_sample_door_matches_the_block_door() {
        let targets: Vec<f32> = (0..512)
            .map(|i| -8.0 + 8.0 * ((i as f32) * 0.03).sin())
            .collect();
        for auto in [false, true] {
            let mut block = Ballistics::new();
            block.prepare(FS, 2.0, 150.0);
            block.set_auto(auto);
            let mut blocked = targets.clone();
            block.process(&mut blocked);

            let mut single = Ballistics::new();
            single.prepare(FS, 2.0, 150.0);
            single.set_auto(auto);
            let ticked: Vec<f32> = targets.iter().map(|t| single.tick(*t)).collect();
            assert!(
                blocked
                    .iter()
                    .zip(&ticked)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "auto {auto}: tick and process must be bit-identical"
            );
        }
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

        // Ballistics, both release modes, over a target that swings so
        // the attack and the release branches are both crossed inside
        // each half.
        let targets: Vec<f32> = (0..256)
            .map(|i| -6.0 + 6.0 * ((i as f32) * 0.07).sin())
            .collect();
        for auto in [false, true] {
            let mut a = Ballistics::new();
            a.prepare(FS, 3.0, 200.0);
            a.set_auto(auto);
            let mut whole = targets.clone();
            a.process(&mut whole);

            let mut b = Ballistics::new();
            b.prepare(FS, 3.0, 200.0);
            b.set_auto(auto);
            let mut split = targets.clone();
            b.process(&mut split[..100]);
            b.process(&mut split[100..]);
            assert!(
                whole
                    .iter()
                    .zip(&split)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "ballistics (auto {auto}): 256 must equal 100 + 156"
            );
        }
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
        let mut fixed = Ballistics::new();
        fixed.prepare(FS, 3.0, 200.0);
        let mut auto = Ballistics::new();
        auto.prepare(FS, 3.0, 200.0);
        auto.set_auto(true);
        let mut smoothed = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                det.process(&input, &mut env);
                gc.process(&env, &mut gains);
                smoothed.copy_from_slice(&gains);
                fixed.process(&mut smoothed);
                auto.process(&mut smoothed);
                // And the per-sample door a feedback topology uses.
                for x in input.iter() {
                    let level = 20.0 * det.tick(*x).max(1e-6).log10();
                    let _ = fixed.tick(gc.gain_db(level));
                }
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
        let mut ball = Ballistics::new();
        ball.prepare(FS, 3.0, 200.0);
        for len in [0usize, 1, 3, 7, 63, 100] {
            let input = vec![0.5f32; len];
            let mut env = vec![0.0f32; len];
            let mut gains = vec![0.0f32; len];
            det.process(&input, &mut env);
            gc.process(&env, &mut gains);
            let mut db = vec![-3.0f32; len];
            ball.process(&mut db);
            assert!(gains.iter().all(|g| g.is_finite()), "len {len}");
            assert!(db.iter().all(|g| g.is_finite()), "ballistics len {len}");
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

    // ------------------------------------- SlewBrighten: the five ---

    /// A sine at `hz`, full scale, `n` samples.
    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    fn peak_of(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn brightener(amount: f32) -> SlewBrighten {
        let mut b = SlewBrighten::new();
        b.prepare(FS, 1_500.0, amount);
        b
    }

    /// REFERENCE. The lift follows the SLEW, which is the whole idea: a
    /// fast signal is brightened and a slow one is left alone. A shelf
    /// would raise both equally, and that is the device this is not.
    #[test]
    fn brighten_lifts_fast_signals_and_leaves_slow_ones_alone() {
        // The knee is stated in the kernel: a full-scale sine at KNEE_HZ
        // sits at half lift.
        let mut at_knee = brightener(1.0);
        let mut buf = sine(SlewBrighten::KNEE_HZ, 4_800);
        at_knee.process(&mut buf);
        assert!(
            (at_knee.lift() - 0.5).abs() < 0.05,
            "the knee is not where it says: {}",
            at_knee.lift()
        );

        // A slow signal barely moves the lift; a fast one nearly pins it.
        let mut slow = brightener(1.0);
        slow.process(&mut sine(100.0, 4_800));
        let mut fast = brightener(1.0);
        fast.process(&mut sine(12_000.0, 4_800));
        assert!(
            slow.lift() < 0.1,
            "a 100 Hz tone was called fast: {}",
            slow.lift()
        );
        // Comfortably past the knee's half, and that is the ceiling this
        // curve really has: `lift` saturates, so even a full-scale tone at
        // Nyquist only reaches about 0.84. A test demanding 0.9 would be
        // a test of a device that cannot exist.
        assert!(
            fast.lift() > 0.65,
            "a 12 kHz tone was called slow: {}",
            fast.lift()
        );
        assert!(
            fast.lift() > slow.lift() * 8.0,
            "the two were not told apart: {} against {}",
            fast.lift(),
            slow.lift()
        );

        // And that difference shows in the OUTPUT, not just the meter:
        // the fast tone gains level, the slow one essentially does not.
        let quiet = sine(100.0, 4_800);
        let mut quiet_out = quiet.clone();
        brightener(1.0).process(&mut quiet_out);
        let loud = sine(9_000.0, 4_800);
        let mut loud_out = loud.clone();
        brightener(1.0).process(&mut loud_out);
        let slow_gain = peak_of(&quiet_out) / peak_of(&quiet);
        let fast_gain = peak_of(&loud_out) / peak_of(&loud);
        assert!(
            fast_gain > slow_gain * 1.5,
            "brightening was not selective: {fast_gain} against {slow_gain}"
        );

        // THE LIFT IS BOUNDED. Nothing an input can do makes this
        // multiply its own output — the property that separates a
        // saturating ratio from a raw differentiator.
        let mut hostile = brightener(4.0);
        let mut square: Vec<f32> = (0..4_800)
            .map(|i| if (i / 2) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        hostile.process(&mut square);
        assert!(square.iter().all(|s| s.is_finite()));
        assert!(hostile.lift() < 1.0, "the lift reached its own ceiling");
        assert!(
            peak_of(&square) < 12.0,
            "a square wave ran away: {}",
            peak_of(&square)
        );
    }

    /// AMOUNT ZERO IS AN EXACT WIRE — bit-exact, so a device can leave
    /// this permanently in its path and a knob at zero really is off.
    #[test]
    fn brighten_at_zero_is_bit_exact_silence_of_effect() {
        let source = sine(1_000.0, 1_024);
        let mut out = source.clone();
        brightener(0.0).process(&mut out);
        assert!(
            source.iter().zip(&out).all(|(a, b)| bits(*a) == bits(*b)),
            "amount 0 changed the signal"
        );
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit for bit.
    #[test]
    fn brighten_is_the_same_however_the_block_is_split() {
        let source = sine(2_500.0, 512);

        let mut whole = brightener(1.0);
        let mut a = source.clone();
        whole.process(&mut a);

        let mut split = brightener(1.0);
        let mut b = source.clone();
        let mut at = 0usize;
        for take in [100usize, 1, 7, 200] {
            let end = (at + take).min(b.len());
            split.process(&mut b[at..end]);
            at = end;
        }
        split.process(&mut b[at..]);

        assert!(
            a.iter().zip(&b).all(|(x, y)| bits(*x) == bits(*y)),
            "the split run diverged"
        );
    }

    /// NO ALLOCATION on the render path.
    #[test]
    fn brighten_does_not_allocate() {
        let mut b = brightener(1.0);
        let mut io = vec![0.25f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..64 {
                b.process(&mut io);
            }
        });
    }

    /// EDGE LENGTHS: zero, one, and a non-power-of-two block advance the
    /// state exactly as one long block would.
    #[test]
    fn brighten_handles_edge_block_lengths() {
        let mut b = brightener(1.0);
        b.process(&mut []);
        assert_eq!(b.lift(), 0.0, "an empty block advanced the state");

        let mut one = [0.5f32; 1];
        b.process(&mut one);
        assert!(one[0].is_finite());

        let source = sine(4_000.0, 111);
        let mut long = source.clone();
        let mut a = brightener(1.0);
        a.process(&mut long);

        let mut pieces = source.clone();
        let mut c = brightener(1.0);
        for chunk in pieces.chunks_mut(7) {
            c.process(chunk);
        }
        assert!(long.iter().zip(&pieces).all(|(x, y)| bits(*x) == bits(*y)));
    }

    /// SILENCE AND THE DENORMAL TAIL. Silence in, exact silence out, and
    /// the envelope lands on exact zero rather than crawling down through
    /// the denormal range forever.
    #[test]
    fn brighten_settles_to_exact_silence() {
        let mut b = brightener(1.0);
        b.process(&mut sine(8_000.0, 4_800));
        assert!(b.lift() > 0.0, "nothing to decay from");

        let mut quiet = vec![0.0f32; 48_000];
        b.process(&mut quiet);

        // The edge band is a one-pole, so silence at the input takes a
        // moment to become silence at the output — the filter has to
        // discharge, and a kernel that jumped straight to zero would be
        // one that clicks. What matters is that it gets there EXACTLY,
        // rather than crawling down through the denormal range forever.
        let tail = &quiet[8_000..];
        assert!(
            tail.iter().all(|s| *s == 0.0),
            "the tail never reached exact zero: {}",
            peak_of(tail)
        );
        assert!(
            quiet.iter().all(|s| s.abs() < 1.0),
            "silence rang rather than decayed"
        );
        assert_eq!(b.env, 0.0, "the envelope never reached exact zero");
        assert_eq!(b.lift(), 0.0);

        // A reset returns it to the same state a fresh one is in.
        b.process(&mut sine(8_000.0, 480));
        b.reset();
        assert_eq!(b.env, 0.0);
        assert_eq!(b.lp, 0.0);
    }

    // ------------------------------- LookaheadLimiter: linked stereo ---

    /// THE GAIN IS SHARED, so the IMAGE DOES NOT MOVE.
    ///
    /// The reason linking is not optional: a level difference between the
    /// channels IS the stereo image, and two independent limiters make
    /// that difference wander on every peak.
    #[test]
    fn linked_limiting_holds_the_stereo_image_still() {
        const LOOKAHEAD_MS: f32 = 1.5;
        let len = LookaheadLimiter::scratch_len(FS, LOOKAHEAD_MS);
        let build = || {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, LOOKAHEAD_MS, 100.0);
            lim.set_ceiling_db(-1.0);
            lim
        };

        // A pair whose right channel is a fixed 6 dB below the left, with
        // peaks that will certainly limit.
        let n = 9_600;
        let left: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / FS;
                let burst = if (i / 2_400) % 2 == 0 { 1.8 } else { 0.2 };
                burst * (core::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();
        let right: Vec<f32> = left.iter().map(|s| s * 0.5).collect();

        let mut l = left.clone();
        let mut r = right.clone();
        let mut key = vec![0.0f32; len];
        let mut bl = vec![0.0f32; len];
        let mut br = vec![0.0f32; len];
        build().process_linked(&mut l, &mut r, &mut key, &mut bl, &mut br);

        // The ratio between the channels is preserved everywhere the
        // signal is above the noise: one gain, both channels.
        for i in 0..n {
            if l[i].abs() > 1e-3 {
                let ratio = r[i] / l[i];
                assert!(
                    (ratio - 0.5).abs() < 1e-3,
                    "the image moved at {i}: {ratio}"
                );
            }
        }

        // And the ceiling still holds, on both.
        let ceiling = 10.0f32.powf(-1.0 / 20.0);
        assert!(peak_of(&l) <= ceiling + 1e-6, "left broke the ceiling");
        assert!(peak_of(&r) <= ceiling + 1e-6, "right broke the ceiling");

        // The louder side is what drove it: limiting the pair must reduce
        // by what LEFT needed, not what right did.
        let mut solo = left.clone();
        let mut solo_buf = vec![0.0f32; len];
        build().process(&mut solo, &mut solo_buf);
        assert!(
            solo.iter().zip(&l).all(|(a, b)| (a - b).abs() < 1e-5),
            "the linked left differs from the same channel limited alone"
        );
    }

    /// Linked stereo splits bit-exactly and allocates nothing, and a
    /// mismatched pair fails OPEN rather than limiting half of it.
    #[test]
    fn linked_limiting_splits_bit_exactly_and_fails_open() {
        const LOOKAHEAD_MS: f32 = 1.0;
        let len = LookaheadLimiter::scratch_len(FS, LOOKAHEAD_MS);
        let build = || {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, LOOKAHEAD_MS, 80.0);
            lim.set_ceiling_db(-2.0);
            lim
        };
        let source: Vec<f32> = sine(300.0, 1_024).iter().map(|s| s * 1.7).collect();

        let mut al = source.clone();
        let mut ar = source.clone();
        let (mut k, mut bl, mut br) = (vec![0.0; len], vec![0.0; len], vec![0.0; len]);
        build().process_linked(&mut al, &mut ar, &mut k, &mut bl, &mut br);

        let mut cl = source.clone();
        let mut cr = source.clone();
        let (mut k2, mut bl2, mut br2) = (vec![0.0; len], vec![0.0; len], vec![0.0; len]);
        let mut split = build();
        let mut at = 0usize;
        for take in [100usize, 1, 300] {
            let end = (at + take).min(source.len());
            split.process_linked(
                &mut cl[at..end],
                &mut cr[at..end],
                &mut k2,
                &mut bl2,
                &mut br2,
            );
            at = end;
        }
        split.process_linked(&mut cl[at..], &mut cr[at..], &mut k2, &mut bl2, &mut br2);
        assert!(
            al.iter().zip(&cl).all(|(x, y)| bits(*x) == bits(*y)),
            "the split linked run diverged"
        );

        // Mismatched lengths and wrong-sized scratch both fail open.
        let mut short = vec![0.5f32; 8];
        let mut long = vec![0.5f32; 9];
        let before = short.clone();
        build().process_linked(&mut short, &mut long, &mut k, &mut bl, &mut br);
        assert_eq!(short, before, "a mismatched pair was processed anyway");

        let mut wrong = vec![0.0f32; 3];
        let mut x = source.clone();
        let mut y = source.clone();
        let untouched = x.clone();
        build().process_linked(&mut x, &mut y, &mut wrong, &mut bl, &mut br);
        assert_eq!(x, untouched, "undersized scratch was used anyway");

        // No allocation on the linked path.
        let mut lim = build();
        let mut pl = vec![0.3f32; 256];
        let mut pr = vec![0.3f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..32 {
                lim.process_linked(&mut pl, &mut pr, &mut k, &mut bl, &mut br);
            }
        });
    }

    /// THE RELEASE MOVES WITHOUT A RESET. A release knob that cleared the
    /// delay line every time it turned would click on every turn.
    #[test]
    fn setting_the_release_does_not_disturb_the_line() {
        const LOOKAHEAD_MS: f32 = 1.0;
        let len = LookaheadLimiter::scratch_len(FS, LOOKAHEAD_MS);
        let mut lim = LookaheadLimiter::new();
        lim.prepare(FS, LOOKAHEAD_MS, 100.0);
        lim.set_ceiling_db(-1.0);
        let mut buf = vec![0.0f32; len];

        // Run it into gain reduction, then move the release mid-stream.
        let mut hot = vec![0.0f32; 512];
        for (i, s) in hot.iter_mut().enumerate() {
            *s = 1.9 * (core::f32::consts::TAU * 200.0 * i as f32 / FS).sin();
        }
        lim.process(&mut hot, &mut buf);
        let gain_before = lim.reduction_db();
        let line_before = buf.clone();

        lim.set_release_ms(FS, 400.0);
        assert_eq!(buf, line_before, "the line was disturbed");
        assert!(
            (lim.reduction_db() - gain_before).abs() < 1e-6,
            "the gain jumped"
        );

        // A longer release really does recover more slowly.
        let recover = |release_ms: f32| {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, LOOKAHEAD_MS, release_ms);
            lim.set_ceiling_db(-6.0);
            let mut buf = vec![0.0f32; len];
            let mut sig = vec![0.0f32; 24_000];
            // One loud burst, then quiet: how fast does the gain come back?
            for (i, s) in sig.iter_mut().enumerate() {
                let amp = if i < 2_400 { 2.0 } else { 0.05 };
                *s = amp * (core::f32::consts::TAU * 200.0 * i as f32 / FS).sin();
            }
            lim.process(&mut sig, &mut buf);
            peak_of(&sig[12_000..])
        };
        assert!(
            recover(40.0) > recover(800.0),
            "the release time did nothing"
        );

        // Nonsense clamps rather than poisoning the coefficient.
        let mut junk = LookaheadLimiter::new();
        junk.prepare(FS, LOOKAHEAD_MS, 100.0);
        for bad in [f32::NAN, -5.0, 0.0, 1e9] {
            junk.set_release_ms(FS, bad);
            let mut io = vec![0.5f32; 64];
            let mut b = vec![0.0f32; len];
            junk.process(&mut io, &mut b);
            assert!(io.iter().all(|s| s.is_finite()), "release {bad}");
        }
    }

    /// The two prepare entry points agree, and the SAMPLE one is exact.
    ///
    /// The reason it exists: a device that reports its latency to the
    /// graph must hold exactly the delay it reported, and a millisecond
    /// figure converted in two places can round two ways.
    #[test]
    fn the_sample_and_millisecond_lookaheads_agree() {
        for (rate, ms) in [(48_000.0f32, 1.5f32), (44_100.0, 5.0), (96_000.0, 0.4)] {
            let mut by_ms = LookaheadLimiter::new();
            by_ms.prepare(rate, ms, 100.0);
            let count = by_ms.latency();

            let mut by_samples = LookaheadLimiter::new();
            by_samples.prepare_samples(rate, count, 100.0);
            assert_eq!(by_samples.latency(), count, "at {rate} Hz");
            assert_eq!(
                LookaheadLimiter::scratch_len(rate, ms),
                LookaheadLimiter::scratch_len_samples(count),
                "the two scratch sizings disagree at {rate} Hz"
            );

            // And the two limit identically, sample for sample.
            let source: Vec<f32> = sine(220.0, 512).iter().map(|s| s * 1.8).collect();
            let len = LookaheadLimiter::scratch_len_samples(count);
            let (mut a, mut b) = (source.clone(), source.clone());
            let (mut ba, mut bb) = (vec![0.0; len], vec![0.0; len]);
            by_ms.set_ceiling_db(-1.0);
            by_samples.set_ceiling_db(-1.0);
            by_ms.process(&mut a, &mut ba);
            by_samples.process(&mut b, &mut bb);
            assert!(a.iter().zip(&b).all(|(x, y)| bits(*x) == bits(*y)));
        }

        // A zero lookahead is still a working limiter, not a divide by
        // nothing.
        let mut none = LookaheadLimiter::new();
        none.prepare_samples(48_000.0, 0, 50.0);
        assert!(none.latency() >= 1);
    }

    /// The wedge has to BE the sliding maximum — every sample, on the
    /// shapes that break the naive versions.
    ///
    /// The limiter's guarantee rests entirely on `win_max` being the
    /// true peak of the window that ends at the newest sample. The
    /// never-exceeds test proves the ceiling holds, which a max that is
    /// merely too BIG would also satisfy — over-limiting is silent. So
    /// this compares against brute force directly, and the material is
    /// chosen to hit the cases a monotonic deque gets wrong: a decay
    /// (every sample evicts the front), a rise (every sample empties the
    /// back), plateaus of equal values (the tie-break), and silence.
    #[test]
    fn the_sliding_maximum_is_the_real_one() {
        const MS: f32 = 1.0;
        let window = LookaheadLimiter::samples_for(FS, MS) + 1;
        let shapes: [(&str, Box<dyn Fn(usize) -> f32>); 6] = [
            ("decay", Box::new(|i: usize| 2.0 / (1.0 + i as f32 * 1e-3))),
            ("rise", Box::new(|i: usize| i as f32 * 1e-3)),
            (
                "plateau",
                Box::new(|i: usize| if i % 97 < 40 { 0.5 } else { 0.1 }),
            ),
            ("silence", Box::new(|_: usize| 0.0)),
            (
                "spikes",
                Box::new(|i: usize| if i % 211 == 0 { 1.7 } else { 0.01 }),
            ),
            (
                "wander",
                Box::new(|i: usize| {
                    let t = i as f32;
                    (t * 0.017).sin() * (t * 0.0013).cos() * 1.3
                }),
            ),
        ];
        for (name, f) in shapes {
            let mut lim = LookaheadLimiter::new();
            lim.prepare(FS, MS, 100.0);
            lim.set_ceiling_db(-1.0);
            let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, MS)];
            let mut history: Vec<f32> = Vec::new();
            for i in 0..4_000 {
                let x = f(i);
                history.push(if x.is_finite() { x.abs() } else { 0.0 });
                let mut one = [x];
                lim.process(&mut one, &mut buf);
                let from = history.len().saturating_sub(window);
                let want = history[from..].iter().fold(0.0f32, |m, v| m.max(*v));
                assert_eq!(
                    lim.win_max, want,
                    "{name} at sample {i}: wedge says {}, window holds {want}",
                    lim.win_max
                );
            }
        }
    }

    /// The stamps are 24-bit, so the counter wraps; the age must not.
    #[test]
    fn the_wedge_survives_its_stamp_counter_wrapping() {
        const MS: f32 = 0.5;
        let window = LookaheadLimiter::samples_for(FS, MS) + 1;
        let mut lim = LookaheadLimiter::new();
        lim.prepare(FS, MS, 100.0);
        lim.set_ceiling_db(-1.0);
        let mut buf = vec![0.0f32; LookaheadLimiter::scratch_len(FS, MS)];
        // Park the counter just below the 24-bit wrap, then walk across.
        lim.pushed = STAMP_MASK - 32;
        let mut history: Vec<f32> = Vec::new();
        for i in 0..(window * 4 + 128) {
            let x = ((i as f32) * 0.37).sin() * 1.4;
            history.push(x.abs());
            let mut one = [x];
            lim.process(&mut one, &mut buf);
            let from = history.len().saturating_sub(window);
            let want = history[from..].iter().fold(0.0f32, |m, v| m.max(*v));
            assert_eq!(lim.win_max, want, "across the wrap, at {i}");
        }
    }

    // ---------------------------------------------- transient split ---

    fn split_armed(window_ms: f32) -> TransientSplit {
        let mut t = TransientSplit::new();
        t.prepare(FS, window_ms);
        t
    }

    /// A percussive hit: instant onset, exponential decay.
    fn hit(amp: f32, decay_ms: f32, n: usize) -> Vec<f32> {
        let tau = decay_ms * 1e-3 * FS;
        (0..n)
            .map(|i| {
                let env = (-(i as f32) / tau).exp();
                amp * env * ((i as f32) * 0.9).sin()
            })
            .collect()
    }

    /// REFERENCE: the gap opens on the attack and closes on the body.
    #[test]
    fn a_hit_opens_the_split_and_a_steady_tone_closes_it() {
        let mut t = split_armed(12.0);
        let signal = hit(0.8, 40.0, 4_800);
        let mut w = vec![0.0f32; signal.len()];
        t.process(&signal, &mut w);

        let onset = w.get(..240).unwrap().iter().fold(0.0f32, |m, v| m.max(*v));
        let body = w
            .get(2_400..)
            .unwrap()
            .iter()
            .fold(0.0f32, |m, v| m.max(*v));
        assert!(onset > 0.5, "the attack must open the split: {onset}");
        assert!(body < 0.1, "the decay must close it: {body}");

        // A steady tone has no attack after its first moment.
        let mut t = split_armed(12.0);
        let tone: Vec<f32> = (0..9_600)
            .map(|i| 0.5 * ((i as f32) * 0.31).sin())
            .collect();
        let mut w = vec![0.0f32; tone.len()];
        t.process(&tone, &mut w);
        let late = w
            .get(4_800..)
            .unwrap()
            .iter()
            .fold(0.0f32, |m, v| m.max(*v));
        assert!(late < 0.1, "a sustained tone must read as body: {late}");
    }

    /// The property that makes this a transient detector rather than a
    /// level detector: a quiet hit and a loud one open it the SAME.
    ///
    /// A compressor cares how loud you played; a transient shaper must
    /// not, or a ghost note gets none of the treatment the accent gets
    /// and the groove flattens out. Normalising by the fast envelope is
    /// what buys this, and nothing else in the file checks it.
    #[test]
    fn the_split_does_not_care_how_loud_the_hit_was() {
        let mut worst = 0.0f32;
        let loud = {
            let mut t = split_armed(12.0);
            let mut w = vec![0.0f32; 2_400];
            t.process(&hit(0.9, 40.0, 2_400), &mut w);
            w
        };
        for amp in [0.03f32, 0.1, 0.3, 0.6] {
            let mut t = split_armed(12.0);
            let mut w = vec![0.0f32; 2_400];
            t.process(&hit(amp, 40.0, 2_400), &mut w);
            for (a, b) in w.iter().zip(loud.iter()) {
                worst = worst.max((a - b).abs());
            }
        }
        assert!(
            worst < 1e-3,
            "the split moved by {worst} when only the level changed"
        );
    }

    /// A wider window keeps the strike open longer. That is the knob's
    /// entire meaning, so it is worth one assertion.
    #[test]
    fn a_wider_window_holds_the_strike_open_longer() {
        let signal = hit(0.8, 60.0, 4_800);
        let open_for = |ms: f32| {
            let mut t = split_armed(ms);
            let mut w = vec![0.0f32; signal.len()];
            t.process(&signal, &mut w);
            w.iter().filter(|v| **v > 0.25).count()
        };
        let narrow = open_for(2.0);
        let wide = open_for(40.0);
        assert!(
            wide > narrow,
            "40 ms held the strike for {wide} samples, 2 ms for {narrow}"
        );
    }

    #[test]
    fn transient_split_is_bit_exact_when_the_block_is_split() {
        let signal = hit(0.7, 30.0, 1_000);
        let mut whole = vec![0.0f32; signal.len()];
        split_armed(12.0).process(&signal, &mut whole);

        let mut piecewise = vec![0.0f32; signal.len()];
        let mut t = split_armed(12.0);
        let mut at = 0;
        for cut in [1usize, 7, 64, 128, 200, 600] {
            let end = (at + cut).min(signal.len());
            let (Some(src), Some(dst)) = (signal.get(at..end), piecewise.get_mut(at..end)) else {
                break;
            };
            t.process(src, dst);
            at = end;
        }
        if let (Some(src), Some(dst)) = (signal.get(at..), piecewise.get_mut(at..)) {
            t.process(src, dst);
        }
        for (i, (a, b)) in whole.iter().zip(piecewise.iter()).enumerate() {
            assert_eq!(bits(*a), bits(*b), "sample {i} differs across a split");
        }
    }

    #[test]
    fn transient_split_takes_any_block_length() {
        let mut t = split_armed(12.0);
        for len in [0usize, 1, 2, 3, 5, 17, 63, 255] {
            let signal = hit(0.5, 20.0, len);
            let mut w = vec![0.0f32; len];
            t.process(&signal, &mut w);
            assert!(w.iter().all(|v| (0.0..=1.0).contains(v)), "len {len}");
        }
        // Mismatched slices truncate rather than panic.
        let mut w = vec![0.0f32; 4];
        t.process(&[0.1; 32], &mut w);
    }

    #[test]
    fn transient_split_does_not_allocate() {
        let mut t = split_armed(12.0);
        let signal = hit(0.6, 25.0, 256);
        let mut w = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..50 {
                t.process(&signal, &mut w);
            }
        });
    }

    #[test]
    fn transient_split_stays_closed_and_finite_on_silence_and_nonsense() {
        let mut t = split_armed(12.0);
        let mut w = vec![9.0f32; 512];
        t.process(&vec![0.0f32; 512], &mut w);
        assert!(
            w.iter().all(|v| *v == 0.0),
            "silence must read as no strike"
        );

        // A decayed tail must not leave a stuck weight behind.
        let mut t = split_armed(12.0);
        let mut w = vec![0.0f32; 4_800];
        t.process(&hit(0.8, 5.0, 4_800), &mut w);
        assert!(
            w.last().is_some_and(|v| *v < 1e-3),
            "the tail left the split open"
        );

        // Nonsense in: bounded, finite, never NaN.
        let mut t = split_armed(12.0);
        let junk = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30, 0.0];
        let mut w = vec![0.0f32; junk.len()];
        t.process(&junk, &mut w);
        assert!(
            w.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "nonsense produced {w:?}"
        );
    }
}
