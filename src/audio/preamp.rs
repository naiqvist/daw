//! The machine's output stage — the third and last place the sampler's
//! colour comes from.
//!
//! Wiring, not a kernel: every piece of arithmetic below already exists
//! in `src/dsp/`. It lives in its own file rather than inside
//! `sampler.rs` because it is a stage a future effect device would want
//! whole, and moving it later would move arithmetic.
//!
//! # Why it is post-sum
//!
//! It is an output stage, and there is one of those. Per voice, eight
//! notes would be eight times as distorted as one, which is the wrong way
//! round: a real output stage gets MORE non-linear as more voices hit it,
//! and summing before it produces that for free.
//!
//! # What it does, in order
//!
//! 1. A tilt at 1 kHz, down towards the top. Bass up, treble down — this
//!    is the "warm" half and it is the half people actually hear.
//! 2. An asymmetric soft clip, reached through HEADROOM. The amount knob
//!    does not drive the curve harder — it moves the programme closer to
//!    the rails, which is what an output stage actually does and is the
//!    only axis that works here: `Mode::SoftClip` is level-driven, so a
//!    full-scale sine measures about 7 % THD whatever its drive is set
//!    to. Scaling INTO the curve and back out is what buys the 0.5 %
//!    this wants.
//!
//!    The bias is the other half: breaking the odd symmetry is where the
//!    second harmonic comes from, and a second harmonic above a third is
//!    what "warmth" means when you measure it. It is FIXED rather than
//!    scaled by the amount, because asymmetry is a property of a circuit
//!    and not of a knob — scaled, the harmonic ordering would invert at
//!    low settings, which is the one thing this stage must never do.
//! 3. A DC blocker, because step 2 makes DC and DC must not reach the
//!    master bus.
//! 4. Hiss, GATED by how loud the programme is. A device that hisses
//!    while idle is a device people switch off — and gating it also keeps
//!    "silence in, silence out" exactly true, which is a property worth
//!    more than the realism of a permanent noise floor.
//!
//! # The bypass
//!
//! At `amount == 0` the whole stage is skipped by a per-block branch and
//! the signal passes bit for bit. That is not a contradiction of an
//! opinionated device: the DEFAULT is coloured, and the identity is what
//! lets the colour be measured rather than merely asserted.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::filters::{DcBlocker, Tilt};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::shaper::{Mode, Waveshaper};

/// Where the tilt pivots, in Hz.
pub const PIVOT_HZ: f32 = 1_000.0;

/// How far the tilt leans at full amount, in dB. Negative is bass-up.
pub const TILT_DB: f32 = -1.5;

/// How much of the soft clipper's curve the programme reaches at full
/// amount.
///
/// Measured, not guessed: at `0.25 × this` — the device's default — a
/// full-scale sine comes out at 0.70 % THD with the second harmonic
/// clearly above the third, which is the brief's target. The calibration
/// probe in this file's tests is how that number was found and how it
/// should be re-found if the curve ever changes.
pub const HEADROOM: f32 = 0.64;

/// The asymmetry. Fixed, not scaled — see the module header.
pub const BIAS: f32 = 0.10;

/// Hiss level at full amount, in dBFS, before the programme gate.
pub const HISS_DB: f32 = -90.0;

/// How quickly the hiss gate follows the programme, as a one-pole
/// coefficient per sample at 48 kHz. About a 20 ms tail: long enough not
/// to chatter, short enough that the noise goes away with the note.
const GATE_FALL: f32 = 0.999;

#[derive(Debug, Clone)]
pub struct Preamp {
    amount: f32,
    tilt_l: Tilt,
    tilt_r: Tilt,
    shaper: Waveshaper,
    /// How far into the curve the programme is pushed, and its inverse.
    into: f32,
    makeup: f32,
    /// What the curve returns for silence. Subtracted on the way out, so
    /// the bias's DC is removed EXACTLY and instantly rather than merely
    /// settled away by the blocker downstream. That is what makes
    /// "silence in, silence out" bit-true instead of nearly true.
    zero: f32,
    dc_l: DcBlocker,
    dc_r: DcBlocker,
    noise: WhiteNoise,
    hiss: f32,
    /// The programme gate's follower — peak in, exponential out.
    gate: f32,
    sample_rate: f32,
}

impl Default for Preamp {
    fn default() -> Self {
        Self::new()
    }
}

impl Preamp {
    pub fn new() -> Self {
        Self {
            amount: 0.0,
            tilt_l: Tilt::new(),
            tilt_r: Tilt::new(),
            shaper: Waveshaper::new(),
            into: 1.0,
            makeup: 1.0,
            zero: 0.0,
            dc_l: DcBlocker::new(),
            dc_r: DcBlocker::new(),
            noise: WhiteNoise::new(),
            hiss: 0.0,
            gate: 0.0,
            sample_rate: 48_000.0,
        }
    }

    /// Green zone.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.dc_l.prepare(self.sample_rate);
        self.dc_r.prepare(self.sample_rate);
        // A seed that is not zero, and not the same as any other noise
        // source in the graph.
        self.noise.seed(0x5A3D_1E77_C0FF_EE01);
        self.set_amount(self.amount);
        self.reset();
    }

    /// Green zone: `0..=1`. Zero is an exact bypass.
    pub fn set_amount(&mut self, amount: f32) {
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.amount = amount;
        self.tilt_l
            .prepare(self.sample_rate, PIVOT_HZ, TILT_DB * amount);
        self.tilt_r
            .prepare(self.sample_rate, PIVOT_HZ, TILT_DB * amount);
        // The curve is configured ONCE and never driven: the amount is
        // entirely in how far the signal is scaled into it.
        self.shaper.configure(Mode::SoftClip, 1.0, BIAS, 1.0);
        self.zero = self.shaper.shape(0.0);
        self.into = (HEADROOM * amount).max(1e-4);
        self.makeup = 1.0 / self.into;
        self.hiss = if amount > 0.0 {
            10f32.powf(HISS_DB / 20.0) * amount
        } else {
            0.0
        };
    }

    pub fn amount(&self) -> f32 {
        self.amount
    }

    pub fn is_bypassed(&self) -> bool {
        self.amount <= 0.0
    }

    pub fn reset(&mut self) {
        self.tilt_l.reset();
        self.tilt_r.reset();
        self.dc_l.reset();
        self.dc_r.reset();
        self.noise.reset();
        self.gate = 0.0;
    }

    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: stereo, in place, any length. `right` may be the same
    /// length as `left` or shorter; only the overlap is processed.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.is_bypassed() {
            return;
        }
        self.tilt_l.process(left);
        self.tilt_r.process(right);
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s *= self.into;
        }
        self.shaper.process(left);
        self.shaper.process(right);
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s = (*s - self.zero) * self.makeup;
        }
        self.dc_l.process(left);
        self.dc_r.process(right);

        // The hiss, last, and gated by the programme. Two independent
        // draws so the noise is not correlated across the stereo field —
        // correlated hiss collapses to the centre and sounds like a fault
        // rather than a floor.
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let peak = l.abs().max(r.abs());
            self.gate = if peak > self.gate {
                peak
            } else {
                self.gate * GATE_FALL
            };
            let level = self.hiss * self.gate.min(1.0);
            if level > 0.0 {
                let mut draw = [0.0f32; 2];
                self.noise.process(&mut draw);
                *l += draw[0] * level;
                *r += draw[1] * level;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn armed(amount: f32) -> Preamp {
        let mut p = Preamp::new();
        p.prepare(SR);
        p.set_amount(amount);
        p.reset();
        p
    }

    /// A full-scale sine, which is what every number below is measured
    /// against.
    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin())
            .collect()
    }

    /// The amplitude of `hz` in `x`, by correlation against a matched
    /// pair. Cheaper than an FFT and exact for a single known frequency,
    /// which is all these tests ask about.
    fn amplitude_at(x: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in x.iter().enumerate() {
            let phase = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR);
            re += f64::from(*s) * phase.cos();
            im += f64::from(*s) * phase.sin();
        }
        let n = x.len() as f64;
        ((re * re + im * im).sqrt() * 2.0 / n) as f32
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// The exact bypass. Everything else here is a measurement of a
    /// colour, and a colour is only measurable against a wire.
    #[test]
    fn zero_amount_is_a_wire() {
        let mut p = armed(0.0);
        assert!(p.is_bypassed());
        let source = sine(1_000.0, 4_096);
        let (mut l, mut r) = (source.clone(), source.clone());
        p.process(&mut l, &mut r);
        assert_eq!(l, source, "left was touched");
        assert_eq!(r, source, "right was touched");
    }

    /// The distortion figure the brief commits to: audible as warmth,
    /// nowhere near audible as distortion.
    #[test]
    fn the_default_amount_distorts_between_a_half_and_one_percent() {
        let mut p = armed(0.25);
        let mut l = sine(1_000.0, 8_192);
        let mut r = l.clone();
        p.process(&mut l, &mut r);

        let fundamental = amplitude_at(&l, 1_000.0);
        let harmonics: f32 = (2..=6)
            .map(|h| amplitude_at(&l, 1_000.0 * h as f32).powi(2))
            .sum::<f32>()
            .sqrt();
        let thd = harmonics / fundamental;
        assert!(
            (0.004..=0.008).contains(&thd),
            "THD is {:.3} %, and the brief says 0.4 to 0.8",
            thd * 100.0
        );
    }

    /// SECOND above third. That ordering is the whole difference between
    /// "warm" and "gritty", and it is what the bias is there to produce —
    /// a symmetric clipper makes only odd harmonics and would fail this
    /// while passing the THD test above.
    #[test]
    fn the_second_harmonic_leads_the_third() {
        let mut p = armed(0.25);
        let mut l = sine(1_000.0, 8_192);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let second = amplitude_at(&l, 2_000.0);
        let third = amplitude_at(&l, 3_000.0);
        assert!(
            second > third,
            "H2 {second:.6} is not above H3 {third:.6} — is the bias wired?"
        );
    }

    /// The tilt leans DOWN: the top goes away, the bottom comes up. Warm
    /// is a shape, not a level.
    #[test]
    fn the_top_end_comes_down_and_the_bottom_comes_up() {
        let measure = |hz: f32| {
            let mut p = armed(1.0);
            let mut l = sine(hz, 8_192);
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            20.0 * (amplitude_at(&l, hz) / 1.0).log10()
        };
        let low = measure(100.0);
        let high = measure(10_000.0);
        assert!(
            low > high,
            "100 Hz came out at {low:.2} dB and 10 kHz at {high:.2} — the tilt is upside down"
        );
        // And the whole tilt is SMALL. An output stage that rewrote the
        // tone balance would be an EQ wearing a preamp's name.
        assert!(
            (low - high) < 4.0,
            "the tilt spans {:.2} dB, which is an equaliser",
            low - high
        );
    }

    /// The amount knob changes the TONE and not the level — which is what
    /// the makeup is for. Without it, "more warmth" would just be
    /// "louder", and every A/B would be won by whichever was hotter.
    #[test]
    fn the_amount_knob_is_not_a_volume_knob() {
        let level = |amount: f32| {
            let mut p = armed(amount);
            let mut l = sine(200.0, 8_192);
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            rms(&l)
        };
        let quiet = level(0.0);
        let loud = level(1.0);
        let change_db = 20.0 * (loud / quiet).log10();
        assert!(
            change_db.abs() < 2.0,
            "full amount moved the level by {change_db:.2} dB"
        );
    }

    /// No DC on the master bus, in STEADY STATE.
    ///
    /// An asymmetric curve on a symmetric input makes DC by construction,
    /// and the blocker is the reason it does not leave. It is a 5 Hz
    /// highpass, so its own settling is a 32 ms exponential — measuring
    /// across that tail reads the transient rather than the leak, which
    /// is what this test did on its first outing and why the number below
    /// is the LAST eighth of a much longer run.
    ///
    /// 375 Hz, and that matters: 48000 / 375 is exactly 128 samples per
    /// cycle, so a window that is a multiple of 128 averages the
    /// fundamental and every harmonic to exactly zero. At 440 Hz the
    /// window ends mid-cycle and the leftover half-cycle reads as about
    /// −54 dBFS of "DC" that is really just the tone.
    #[test]
    fn the_bias_does_not_leak_dc() {
        let mut p = armed(1.0);
        let mut l = sine(375.0, 65_536);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let tail = &l[57_344..];
        let dc = tail.iter().sum::<f32>() / tail.len() as f32;
        let dc_db = 20.0 * dc.abs().max(1e-12).log10();
        assert!(dc_db < -100.0, "DC is at {dc_db:.1} dBFS");
    }

    /// Silence in, silence out — EXACTLY. This is what the programme gate
    /// on the hiss buys, and it is worth more than the realism of a
    /// permanent noise floor: a device that hisses while idle is a device
    /// people switch off.
    #[test]
    fn an_idle_preamp_is_exactly_silent() {
        for amount in [0.0f32, 0.25, 1.0] {
            let mut p = armed(amount);
            let mut l = vec![0.0f32; 4_096];
            let mut r = vec![0.0f32; 4_096];
            p.process(&mut l, &mut r);
            assert!(
                l.iter().all(|s| *s == 0.0) && r.iter().all(|s| *s == 0.0),
                "amount {amount} hissed into silence"
            );
        }
    }

    /// And with programme present the hiss IS there — the other half of
    /// the gate actually doing something.
    #[test]
    fn the_hiss_arrives_with_the_programme() {
        let noise_floor = |amount: f32| {
            let mut p = armed(amount);
            // A tone well below the top, so the hiss is measurable
            // against it without the clipper dominating.
            let mut l: Vec<f32> = sine(1_000.0, 16_384).iter().map(|s| s * 0.5).collect();
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            // Everything that is NOT the tone or its harmonics.
            let tone: f32 = (1..=8)
                .map(|h| amplitude_at(&l, 1_000.0 * h as f32).powi(2))
                .sum();
            (rms(&l).powi(2) * 2.0 - tone).max(0.0).sqrt()
        };
        assert!(
            noise_floor(1.0) > noise_floor(0.0),
            "the hiss did not arrive"
        );
    }

    /// CALIBRATION PROBE — prints, asserts nothing. Run with
    /// `cargo test -- calibrate --ignored --nocapture` when retuning.
    #[test]
    #[ignore]
    fn calibrate() {
        for g in [0.08f32, 0.12, 0.16, 0.2, 0.3, 0.45] {
            for bias in [0.05f32, 0.1, 0.2, 0.4, 0.7] {
                let mut sh = Waveshaper::new();
                sh.configure(Mode::SoftClip, 1.0, bias, 1.0);
                let zero = sh.shape(0.0);
                let mut l: Vec<f32> = sine(1_000.0, 8_192).iter().map(|s| s * g).collect();
                sh.process(&mut l);
                for v in l.iter_mut() {
                    *v = (*v - zero) / g;
                }
                let f = amplitude_at(&l, 1_000.0);
                let h2 = amplitude_at(&l, 2_000.0);
                let h3 = amplitude_at(&l, 3_000.0);
                let thd = ((2..=6)
                    .map(|h| amplitude_at(&l, 1_000.0 * h as f32).powi(2))
                    .sum::<f32>())
                .sqrt()
                    / f;
                println!(
                    "g {g:.2} bias {bias:.2} -> THD {:.3}% gain {f:.4} H2 {h2:.5} H3 {h3:.5} h2>h3 {}",
                    thd * 100.0,
                    h2 > h3
                );
            }
        }
    }

    /// Split-block equivalence: the stage is stateful (two tilts, two DC
    /// blockers, a gate and a noise generator) and a block boundary must
    /// not be audible in any of them.
    #[test]
    fn split_blocks_match_a_whole_one() {
        let source = sine(220.0, 2_000);

        let mut whole_l = source.clone();
        let mut whole_r = source.clone();
        armed(0.6).process(&mut whole_l, &mut whole_r);

        let mut split_l = source.clone();
        let mut split_r = source.clone();
        let mut p = armed(0.6);
        {
            let (la, lb) = split_l.split_at_mut(731);
            let (ra, rb) = split_r.split_at_mut(731);
            p.process(la, ra);
            p.process(lb, rb);
        }
        assert_eq!(whole_l, split_l, "a block boundary changed the left");
        assert_eq!(whole_r, split_r, "a block boundary changed the right");
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut p = armed(0.5);
        let mut l = vec![0.3f32; 256];
        let mut r = vec![0.2f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 7 == 0 {
                    p.set_amount((i % 5) as f32 * 0.25);
                }
                p.process(&mut l, &mut r);
            }
        });
    }

    /// Edge lengths, and a mono wiring where the right slice is empty.
    #[test]
    fn every_block_length_is_survivable() {
        for len in [0usize, 1, 2, 3, 7, 63, 257] {
            let mut p = armed(0.5);
            let mut l = sine(300.0, len);
            let mut r = sine(300.0, len);
            p.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}: left");

            // The mono case the node takes when there is no right slot.
            let mut p = armed(0.5);
            let mut l = sine(300.0, len);
            let mut nothing: [f32; 0] = [];
            p.process(&mut l, &mut nothing);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}: mono");
        }
    }

    /// Hot and decaying material stays finite all the way down.
    #[test]
    fn hot_and_decaying_input_stays_finite() {
        let mut p = armed(1.0);
        let mut l: Vec<f32> = (0..8_000)
            .map(|i| {
                (std::f32::consts::TAU * 90.0 * i as f32 / SR).sin()
                    * 4.0
                    * (-(i as f32) / 800.0).exp()
            })
            .collect();
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(r.iter().all(|s| s.is_finite()));
    }
}
