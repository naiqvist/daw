//! Family 5 of the kernel roadmap: the delay family.
//!
//! [`DelayLine`] (integer-exact and fractional reads over a caller-owned
//! buffer, with a per-sample-modulated variant — the chorus/flanger
//! primitive) and [`FeedbackDelay`] (the echo: recirculation with damping
//! in the loop).
//!
//! # The fractional-read decision
//!
//! A delay that MOVES is the whole point of half this family, and the
//! interpolator decides how moving sounds:
//!
//! - **Linear** is a comb-with-lowpass in disguise: at a half-sample
//!   offset it is 2 dB down by 10 kHz. Shipped for the caller that truly
//!   never modulates, and as the measured baseline.
//! - **Allpass interpolation** is magnitude-flat but carries state that
//!   goes stale the moment the delay changes — it smears transients
//!   under exactly the modulation chorus needs. Deliberately omitted.
//! - **Hermite** (Catmull–Rom, the [`MipOsc`] read) is stateless, so it
//!   behaves under modulation, and its droop is ~0.5 dB at 10 kHz —
//!   measured against linear in the tests rather than asserted from a
//!   textbook.
//!
//! # Buffers
//!
//! Caller-owned, sized by [`buffer_len`] (rounded to a power of two so
//! every access is mask-bounded and in-bounds by construction — the
//! [`MipOsc`] trick). A wrong-sized buffer FAILS OPEN: the block passes
//! through dry, which is audible enough to notice and safe enough to
//! ship, where silence would read as a dead track and garbage as a
//! blown tweeter.
//!
//! Delay is the EFFECT here, not latency: a kernel that delays on
//! purpose reports `latency() == 0`, because plugin-delay compensation
//! must not "correct" an echo away.
//!
//! [`MipOsc`]: crate::dsp::osc::MipOsc

use crate::dsp::filters::OnePole;

/// Headroom past the longest delay: Hermite reads one tap newer and two
/// older than the integer delay, and read/write must never collide.
const GUARD: usize = 8;
/// The shortest fractional delay. Hermite's newest tap sits one sample
/// ahead of the delay point, and it must not read the slot being written.
const MIN_FRAC: f32 = 2.0;

/// Floats a delay buffer needs for `max_delay_samples`. Green zone.
pub fn buffer_len(max_delay_samples: usize) -> usize {
    (max_delay_samples + GUARD).next_power_of_two()
}

/// Catmull–Rom at `t` between `p1` and `p2`.
#[inline(always)]
fn hermite(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let a = 0.5 * (p2 - p0);
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    p1 + t * (a + t * (b + t * c))
}

// ----------------------------------------------------------- delay line ---

/// A delay line over a caller-owned buffer.
///
/// State: 24 bytes. Per-sample cost: 2 masked reads + 1 write
/// ([`process_exact`]); 5 reads and ~10 flops ([`process_smooth`] /
/// [`process_modulated`]).
/// Denormal-safe: stores and replays what it is given; FTZ covers
/// decayed content in the buffer.
/// In-place safe: yes — `io` is read as input and rewritten as output.
/// Latency: 0 samples (the delay is the effect; see the module note).
#[derive(Debug, Clone, Copy)]
pub struct DelayLine {
    write: usize,
    mask: usize,
    /// The buffer length `prepare` promised; a mismatched slice fails
    /// open (dry) rather than reading garbage.
    expected: usize,
    /// Longest delay this line was prepared for, in samples.
    max: f32,
    delay: f32,
}

impl Default for DelayLine {
    fn default() -> Self {
        Self::new()
    }
}

impl DelayLine {
    pub fn new() -> Self {
        Self {
            write: 0,
            mask: 0,
            expected: 0,
            max: 0.0,
            delay: MIN_FRAC,
        }
    }

    /// Green zone: commit to a maximum delay. The buffer handed to
    /// `process` must be exactly `buffer_len(max_delay_samples)` long
    /// and zeroed by the caller (`mem::clear`) before first use.
    pub fn prepare(&mut self, max_delay_samples: usize) {
        let len = buffer_len(max_delay_samples);
        self.expected = len;
        self.mask = len - 1;
        self.max = max_delay_samples.max(1) as f32;
        self.write = 0;
        self.set_delay(self.delay);
    }

    /// Set the delay, in samples (fractional welcome). Clamped to what
    /// the line was prepared for; nonsense clamps rather than poisons.
    pub fn set_delay(&mut self, samples: f32) {
        self.delay = if samples.is_finite() {
            samples.clamp(1.0, self.max.max(1.0))
        } else {
            MIN_FRAC
        };
    }

    /// Green zone: forget the buffer's contents are meaningful. The
    /// caller clears the slice itself (`mem::clear`) — state and storage
    /// are owned separately, per the contract.
    pub fn reset(&mut self) {
        self.write = 0;
    }

    /// Crate-internal per-sample read, `behind` samples back from the
    /// write head. For kernels that build ON the delay line inside their
    /// own per-sample loops (the lookahead limiter) — the OnePole::
    /// tick_lowpass precedent. Block callers use the process_* family.
    #[inline(always)]
    pub(crate) fn tap(&self, buf: &[f32], behind: usize) -> f32 {
        buf[(self.write.wrapping_sub(behind)) & self.mask]
    }

    /// Crate-internal: store one sample and advance. After a push the
    /// pushed sample is `tap(buf, 1)`.
    #[inline(always)]
    pub(crate) fn push(&mut self, buf: &mut [f32], x: f32) {
        buf[self.write & self.mask] = x;
        self.write = self.write.wrapping_add(1);
    }

    /// Crate-internal: does this slice match what `prepare` promised?
    /// The fail-open check, shared so builders cannot get it wrong.
    #[inline(always)]
    pub(crate) fn matches(&self, buf: &[f32]) -> bool {
        buf.len() == self.expected && !buf.is_empty()
    }

    /// Red zone: integer-exact delay, in place. The delay is
    /// `set_delay` rounded to the nearest sample — an impulse in comes
    /// back bit-identical, exactly N samples later.
    pub fn process_exact(&mut self, io: &mut [f32], buf: &mut [f32]) {
        if buf.len() != self.expected || buf.is_empty() {
            return; // fail open: io already holds the dry signal
        }
        let d = (self.delay.round() as usize).clamp(1, self.max as usize);
        for s in io.iter_mut() {
            let x = *s;
            buf[self.write & self.mask] = x;
            *s = self.tap(buf, d);
            self.write = self.write.wrapping_add(1);
        }
    }

    /// Red zone: fractional delay via Hermite, in place. The one to use
    /// whenever the delay time can move.
    pub fn process_smooth(&mut self, io: &mut [f32], buf: &mut [f32]) {
        if buf.len() != self.expected || buf.is_empty() {
            return;
        }
        let d = self.delay.max(MIN_FRAC);
        for s in io.iter_mut() {
            let x = *s;
            buf[self.write & self.mask] = x;
            *s = self.read_frac(buf, d);
            self.write = self.write.wrapping_add(1);
        }
    }

    /// Red zone: per-sample delay times — the chorus/flanger primitive.
    /// `delays` pairs with `io` (`zip` length rules); each entry clamps
    /// like `set_delay`.
    pub fn process_modulated(&mut self, io: &mut [f32], buf: &mut [f32], delays: &[f32]) {
        if buf.len() != self.expected || buf.is_empty() {
            return;
        }
        let max = self.max.max(MIN_FRAC);
        for (s, want) in io.iter_mut().zip(delays.iter()) {
            let d = if want.is_finite() {
                want.clamp(MIN_FRAC, max)
            } else {
                MIN_FRAC
            };
            let x = *s;
            buf[self.write & self.mask] = x;
            *s = self.read_frac(buf, d);
            self.write = self.write.wrapping_add(1);
        }
    }

    /// Hermite read at fractional delay `d` (callers clamp; `d ≥ 2`).
    /// Written AFTER the current input, so `p0` — one sample newer than
    /// the delay point — is this block's data, never a stale slot.
    #[inline(always)]
    fn read_frac(&self, buf: &[f32], d: f32) -> f32 {
        let base = d as usize; // ≥ 2 by the callers' clamp
        let t = d - base as f32;
        let p0 = self.tap(buf, base - 1);
        let p1 = self.tap(buf, base);
        let p2 = self.tap(buf, base + 1);
        let p3 = self.tap(buf, base + 2);
        hermite(p0, p1, p2, p3, t)
    }

    /// Red zone: fractional delay via LINEAR interpolation. Cheaper, and
    /// duller the shorter and higher the content — the measured baseline
    /// the module docs quote. For unmodulated lines only.
    pub fn process_linear(&mut self, io: &mut [f32], buf: &mut [f32]) {
        if buf.len() != self.expected || buf.is_empty() {
            return;
        }
        let d = self.delay.max(MIN_FRAC);
        let base = d as usize;
        let t = d - base as f32;
        for s in io.iter_mut() {
            let x = *s;
            buf[self.write & self.mask] = x;
            let p1 = self.tap(buf, base);
            let p2 = self.tap(buf, base + 1);
            *s = p1 + (p2 - p1) * t;
            self.write = self.write.wrapping_add(1);
        }
    }
}

// ------------------------------------------------------- feedback delay ---

/// An echo: a fractional delay recirculating through a damping one-pole.
///
/// The output is the UNDAMPED read — the first echo arrives bright and
/// the darkening compounds per trip, which is how analogue repeats die.
/// Dry/wet is the caller's business (`arith::crossfade`); this emits
/// wet only, in place.
///
/// State: 40 bytes. Per-sample cost: one Hermite read + the one-pole
/// (≈ 15 flops) + 1 write.
/// Denormal-safe: the decaying recirculation is the classic denormal
/// factory; relies on engine FTZ, and the tail test runs it far past
/// audibility to prove finiteness.
/// In-place safe: yes.
/// Latency: 0 samples (see the module note).
#[derive(Debug, Clone, Copy)]
pub struct FeedbackDelay {
    line: DelayLine,
    damp: OnePole,
    feedback: f32,
}

/// The loop gain ceiling. At 1.0 an echo never dies and the buffer
/// integrates toward the rails; 0.99 keeps "almost forever" available
/// while every tail provably decays.
pub const FEEDBACK_MAX: f32 = 0.99;

impl Default for FeedbackDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackDelay {
    pub fn new() -> Self {
        Self {
            line: DelayLine::new(),
            damp: OnePole::new(),
            feedback: 0.0,
        }
    }

    /// Green zone: sample rate (for the damping filter), the longest
    /// delay this echo will be asked for, and the damping corner —
    /// repeats lose everything above it, a little more each trip.
    pub fn prepare(&mut self, sample_rate: f32, max_delay_samples: usize, damp_hz: f32) {
        self.line.prepare(max_delay_samples);
        self.damp.prepare(sample_rate, damp_hz);
    }

    /// The buffer this echo needs — same rule as [`buffer_len`].
    pub fn needed_len(max_delay_samples: usize) -> usize {
        buffer_len(max_delay_samples)
    }

    /// Set the echo time, in samples (fractional welcome).
    pub fn set_delay(&mut self, samples: f32) {
        self.line.set_delay(samples.max(MIN_FRAC));
    }

    /// Set the loop gain, `0..=`[`FEEDBACK_MAX`]. Negative flips the
    /// repeat's polarity (the flanger convention) and clamps the same.
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = if feedback.is_finite() {
            feedback.clamp(-FEEDBACK_MAX, FEEDBACK_MAX)
        } else {
            0.0
        };
    }

    /// Green zone: zero the recirculation state (caller clears the
    /// buffer slice, as with [`DelayLine::reset`]).
    pub fn reset(&mut self) {
        self.line.reset();
        self.damp.reset();
    }

    /// Red zone: replace `io` with the wet echo signal, any length.
    pub fn process(&mut self, io: &mut [f32], buf: &mut [f32]) {
        if buf.len() != self.line.expected || buf.is_empty() {
            return; // fail open — io keeps the dry signal
        }
        let d = self.line.delay.max(MIN_FRAC);
        let fb = self.feedback;
        for s in io.iter_mut() {
            let x = *s;
            let wet = self.line.read_frac(buf, d);
            // Damping lives INSIDE the loop: each round trip darkens the
            // repeat again, which is the analogue behaviour. The output
            // is the bright pre-damping read.
            let recirculated = self.damp.tick_lowpass(wet);
            buf[self.line.write & self.line.mask] = x + recirculated * fb;
            *s = wet;
            self.line.write = self.line.write.wrapping_add(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn line(max: usize, delay: f32) -> (DelayLine, Vec<f32>) {
        let mut l = DelayLine::new();
        l.prepare(max);
        l.set_delay(delay);
        (l, vec![0.0f32; buffer_len(max)])
    }

    /// Goertzel magnitude — for the interpolator flatness measurements.
    fn tone_amp(x: &[f32], hz: f32) -> f64 {
        let w = core::f64::consts::TAU * (hz / FS) as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in x {
            let s0 = v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / x.len() as f64
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    /// Gain of an interpolated read at roughly `hz`, for a worst-case
    /// half-sample fractional delay.
    ///
    /// The probe frequency is SNAPPED to a bin of the measured tail:
    /// rectangular-window Goertzel scallops up to -3.9 dB off-bin, which
    /// would swamp a 0.3 dB flatness assertion. And `tone_amp` returns
    /// AMPLITUDE, not RMS — the first draft multiplied by sqrt(2) and
    /// measured every path +3.01 dB "louder" than unity.
    fn frac_gain_db(hermite_path: bool, hz: f32) -> f64 {
        let n = 8192usize;
        let tail = n / 2;
        let hz = (hz * tail as f32 / FS).round() * FS / tail as f32;
        let (mut l, mut buf) = line(64, 10.5);
        let mut sig: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
            .collect();
        if hermite_path {
            l.process_smooth(&mut sig, &mut buf);
        } else {
            l.process_linear(&mut sig, &mut buf);
        }
        db(tone_amp(&sig[tail..], hz))
    }

    // -------------------------------------------------------- reference ---

    /// The contract's own example: an impulse comes back at EXACTLY N
    /// samples, bit-identical, and nowhere else.
    #[test]
    fn an_impulse_returns_at_exactly_n_samples() {
        for d in [1usize, 7, 100, 480] {
            let (mut l, mut buf) = line(500, d as f32);
            let mut io = vec![0.0f32; 600];
            io[0] = 1.0;
            l.process_exact(&mut io, &mut buf);
            for (i, s) in io.iter().enumerate() {
                if i == d {
                    assert_eq!(s.to_bits(), 1.0f32.to_bits(), "delay {d}: the impulse");
                } else {
                    assert_eq!(*s, 0.0, "delay {d}: leakage at {i}");
                }
            }
        }
    }

    /// Hermite is the default because it is measurably flatter than
    /// linear where it matters: at a half-sample offset and 10 kHz,
    /// linear is ~2 dB down and Hermite well under 1 — measured off the
    /// kernels running, not quoted from a table.
    #[test]
    fn hermite_beats_linear_where_the_ear_lives() {
        for hz in [1_000.0f32, 5_000.0] {
            assert!(
                frac_gain_db(true, hz).abs() < 0.3,
                "hermite at {hz} Hz: {:.2} dB",
                frac_gain_db(true, hz)
            );
        }
        let h = frac_gain_db(true, 10_000.0);
        let l = frac_gain_db(false, 10_000.0);
        assert!(h.abs() < 0.8, "hermite at 10 kHz droops {h:.2} dB");
        assert!(
            l < -1.5,
            "linear at 10 kHz should be clearly down, got {l:.2}"
        );
        assert!(h > l + 1.0, "hermite must beat linear: {h:.2} vs {l:.2}");
    }

    /// A fractional delay is the delay it claims: a sine through a
    /// 10.5-sample line comes back with the phase of 10.5 samples, not
    /// 10 or 11 (measured as group delay via cross-correlation peak of
    /// an impulse-ish burst — cheaper: phase of a single tone).
    #[test]
    fn a_fractional_delay_delays_fractionally() {
        let hz = 1_000.0f32;
        let (mut l, mut buf) = line(64, 10.5);
        let n = 4096;
        let mut sig: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
            .collect();
        l.process_smooth(&mut sig, &mut buf);
        // Compare against analytically delayed sines at 10, 10.5, 11.
        let err = |d: f32| {
            let tail = n / 2..n;
            tail.map(|i| {
                let want = (((i as f32) - d) / FS * hz * core::f32::consts::TAU).sin();
                let got = sig[i];
                ((want - got) as f64).powi(2)
            })
            .sum::<f64>()
        };
        assert!(err(10.5) < err(10.0) * 0.05, "closer to 10.5 than 10");
        assert!(err(10.5) < err(11.0) * 0.05, "closer to 10.5 than 11");
    }

    /// The echo decays at exactly the feedback ratio, undamped — and
    /// damped repeats darken per trip while the first echo stays bright.
    #[test]
    fn feedback_decays_at_its_stated_ratio_and_damping_darkens_repeats() {
        let d = 100usize;
        let mut e = FeedbackDelay::new();
        // Damping corner way above Nyquist interest = effectively open.
        e.prepare(FS, 200, 23_000.0);
        e.set_delay(d as f32);
        e.set_feedback(0.5);
        let mut buf = vec![0.0f32; FeedbackDelay::needed_len(200)];
        let mut io = vec![0.0f32; d * 5];
        io[0] = 1.0;
        e.process(&mut io, &mut buf);
        let peak_near = |at: usize| {
            io[at.saturating_sub(3)..(at + 4).min(io.len())]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        let (e1, e2, e3) = (peak_near(d), peak_near(2 * d), peak_near(3 * d));
        assert!((e1 - 1.0).abs() < 0.05, "first echo ~unity, got {e1}");
        assert!((e2 / e1 - 0.5).abs() < 0.06, "ratio {:.3} != fb", e2 / e1);
        assert!((e3 / e2 - 0.5).abs() < 0.06, "ratio {:.3} != fb", e3 / e2);

        // Damped: feed a high tone, the second echo is duller than the
        // first by the loop filter.
        let mut e = FeedbackDelay::new();
        e.prepare(FS, 200, 2_000.0);
        e.set_delay(d as f32);
        e.set_feedback(0.7);
        let mut buf = vec![0.0f32; FeedbackDelay::needed_len(200)];
        let hz = 8_000.0f32;
        let mut io: Vec<f32> = (0..d * 4)
            .map(|i| {
                if i < d {
                    (i as f32 / FS * hz * core::f32::consts::TAU).sin()
                } else {
                    0.0
                }
            })
            .collect();
        e.process(&mut io, &mut buf);
        let rms = |a: usize, b: usize| {
            (io[a..b].iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / (b - a) as f64).sqrt()
        };
        let echo1 = rms(d, 2 * d);
        let echo2 = rms(2 * d, 3 * d);
        assert!(
            echo2 / echo1 < 0.7 * 0.6,
            "8 kHz repeats must lose more than the feedback alone: {:.3}",
            echo2 / echo1
        );
    }

    /// Modulated delay: a moving read stays finite and split-block
    /// bit-exact — the state is one write head, and the control slice is
    /// data, so splitting cannot change anything.
    #[test]
    fn split_block_is_bit_exact_even_while_moving() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.13).sin()).collect();
        let delays: Vec<f32> = (0..256)
            .map(|i| 12.0 + 6.0 * ((i as f32) * 0.05).sin())
            .collect();

        let (mut a, mut abuf) = line(64, 12.0);
        let mut whole = input.clone();
        a.process_modulated(&mut whole, &mut abuf, &delays);

        let (mut b, mut bbuf) = line(64, 12.0);
        let mut split = input.clone();
        b.process_modulated(&mut split[..100], &mut bbuf, &delays[..100]);
        b.process_modulated(&mut split[100..], &mut bbuf, &delays[100..]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "modulated: 256 must equal 100 + 156"
        );

        let mut e1 = FeedbackDelay::new();
        e1.prepare(FS, 64, 4_000.0);
        e1.set_delay(20.0);
        e1.set_feedback(0.6);
        let mut buf1 = vec![0.0f32; FeedbackDelay::needed_len(64)];
        let mut whole = input.clone();
        e1.process(&mut whole, &mut buf1);

        let mut e2 = FeedbackDelay::new();
        e2.prepare(FS, 64, 4_000.0);
        e2.set_delay(20.0);
        e2.set_feedback(0.6);
        let mut buf2 = vec![0.0f32; FeedbackDelay::needed_len(64)];
        let mut split = input.clone();
        e2.process(&mut split[..100], &mut buf2);
        e2.process(&mut split[100..], &mut buf2);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }

    // ------------------------------------------------------------ no-alloc ---

    #[test]
    fn process_does_not_allocate() {
        let (mut l, mut lbuf) = line(256, 33.5);
        let mut e = FeedbackDelay::new();
        e.prepare(FS, 256, 3_000.0);
        e.set_delay(120.0);
        e.set_feedback(0.8);
        let mut ebuf = vec![0.0f32; FeedbackDelay::needed_len(256)];
        let delays = vec![20.0f32; 256];
        let mut io = vec![0.1f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                l.process_exact(&mut io, &mut lbuf);
                l.process_smooth(&mut io, &mut lbuf);
                l.process_linear(&mut io, &mut lbuf);
                l.process_modulated(&mut io, &mut lbuf, &delays);
                e.process(&mut io, &mut ebuf);
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let (mut l, mut lbuf) = line(64, 9.25);
        let mut e = FeedbackDelay::new();
        e.prepare(FS, 64, 3_000.0);
        e.set_delay(30.0);
        e.set_feedback(0.5);
        let mut ebuf = vec![0.0f32; FeedbackDelay::needed_len(64)];
        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut io = vec![0.25f32; len];
            let delays = vec![10.0f32; len];
            l.process_exact(&mut io, &mut lbuf);
            l.process_smooth(&mut io, &mut lbuf);
            l.process_modulated(&mut io, &mut lbuf, &delays);
            e.process(&mut io, &mut ebuf);
            assert!(io.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    // ------------------------------------------------ silence and nonsense ---

    /// Silence in, silence out; a maxed-feedback tail runs far past
    /// audibility and stays finite (the classic denormal factory); and
    /// nonsense settings clamp instead of poisoning.
    #[test]
    fn tails_die_and_nonsense_cannot_poison() {
        let mut e = FeedbackDelay::new();
        e.prepare(FS, 64, 4_000.0);
        e.set_delay(50.0);
        e.set_feedback(FEEDBACK_MAX);
        let mut buf = vec![0.0f32; FeedbackDelay::needed_len(64)];
        let mut io = vec![0.0f32; 128];
        e.process(&mut io, &mut buf);
        assert!(io.iter().all(|s| *s == 0.0), "silence in, silence out");

        io[0] = 1.0;
        e.process(&mut io, &mut buf);
        let mut peak = 1.0f32;
        for _ in 0..2_000 {
            let mut quiet = vec![0.0f32; 128];
            e.process(&mut quiet, &mut buf);
            let p = quiet.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(p.is_finite());
            peak = p;
        }
        assert!(peak < 1e-3, "a 0.99 tail eventually dies, got {peak}");

        for (delay, fb) in [
            (f32::NAN, 0.5),
            (-10.0, 0.5),
            (1e9, 0.5),
            (30.0, f32::NAN),
            (30.0, 2.0),
            (30.0, -2.0),
        ] {
            let mut e = FeedbackDelay::new();
            e.prepare(FS, 64, 4_000.0);
            e.set_delay(delay);
            e.set_feedback(fb);
            let mut buf = vec![0.0f32; FeedbackDelay::needed_len(64)];
            // Fresh input every block — the first draft fed the wet
            // output back in as input, compounding the echo eight extra
            // times OUTSIDE the kernel and then blaming the kernel.
            for _ in 0..8 {
                let mut io: Vec<f32> = (0..512).map(|i| (i as f32 * 0.3).sin()).collect();
                e.process(&mut io, &mut buf);
                assert!(
                    io.iter().all(|s| s.is_finite() && s.abs() < 1e3),
                    "delay {delay} fb {fb} ran away"
                );
            }
        }

        // A wrong-sized buffer fails OPEN: dry passes, nothing read.
        let (mut l, _) = line(64, 10.0);
        let mut short = vec![0.0f32; 16];
        let mut io = vec![0.7f32; 32];
        l.process_smooth(&mut io, &mut short);
        assert!(io.iter().all(|s| *s == 0.7), "mismatch must pass dry");
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let mut io = vec![0.1f32; BLOCK];
        let delays = vec![30.5f32; BLOCK];

        let mut row = |name: &str, run: &mut dyn FnMut(&mut [f32])| {
            for _ in 0..1_000 {
                run(&mut io);
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run(&mut io);
                std::hint::black_box(&mut io);
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<22} {ns:6.2} ns/sample");
        };

        let (mut l, mut lbuf) = line(1 << 14, 1000.0);
        row("exact", &mut |b| l.process_exact(b, &mut lbuf));
        row("smooth (hermite)", &mut |b| l.process_smooth(b, &mut lbuf));
        row("modulated", &mut |b| {
            l.process_modulated(b, &mut lbuf, &delays)
        });
        let mut e = FeedbackDelay::new();
        e.prepare(FS, 1 << 14, 4_000.0);
        e.set_delay(1000.0);
        e.set_feedback(0.7);
        let mut ebuf = vec![0.0f32; FeedbackDelay::needed_len(1 << 14)];
        row("feedback + damping", &mut |b| e.process(b, &mut ebuf));
    }
}
