//! CLAMP — the surgical compressor.
//!
//! Wiring, not a kernel: [`RmsDetector`], [`GainComputer`],
//! [`Ballistics`] and [`Preamp`](crate::audio::preamp::Preamp) already
//! exist and this file says which feeds which.
//!
//! # Why a second compressor
//!
//! Because `glue` is the other one, and the two are opposite devices
//! wearing the same word.
//!
//! Glue is a bus compressor: three ratio detents, a program-dependent
//! release, a peak clipper at the rails, and a character you switch on.
//! It is built to move slowly and not be noticed, and every one of those
//! choices is right for holding a mix together and wrong for stopping
//! one syllable.
//!
//! This one is built for the second job. The ratio is continuous
//! because the ratio IS the decision when the source changes; the knee
//! is a control rather than a constant; the attack reaches under a tenth
//! of a millisecond, which is what "catch that transient" costs; and
//! there is no auto release, because program-dependence is exactly the
//! thing you do not want when you are aiming at one event.
//!
//! Two compressors is not duplication when they are these two. One
//! device wide enough to be both would have a bad default for each.
//!
//! # The warmth, and why it stops early
//!
//! The same `Preamp` the sampler and the strip run — tilt, an
//! asymmetric soft clip, a DC blocker, a gated hiss — driven to at most
//! [`p::WARMTH_CEILING`] of its range.
//!
//! That ceiling is the device saying what it is. At full amount the
//! stage is a CHARACTER, and the reason to reach for a surgical
//! compressor is that it does not change the sound it is controlling. A
//! third of the way is a seasoning: enough that the thing sounds like it
//! went through something, not enough to be the reason it sounds like
//! it does.
//!
//! Exact bypass at zero, the promise every colour stage here makes.
//!
//! # Red zone
//!
//! Everything but [`Clamp::new`] and [`Clamp::prepare`] runs in the
//! callback: no allocation, no locks, no panics. The detector, the
//! computer and the ballistics are all per-sample state and the
//! settings are resolved once per segment rather than per sample.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::preamp::Preamp;
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::dsp::filters::OnePole;
use crate::params::clamp as p;

/// How long the detector averages over, in milliseconds.
///
/// Short — this is a PEAK-ish detector by intent. An RMS window long
/// enough to be smooth is a window long enough to miss the transient
/// the device exists to catch, and the ballistics are where smoothing
/// belongs because that is the part with a knob on it.
const DETECT_MS: f32 = 1.0;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ClampParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    pub sc_hp_hz: f32,
    pub warmth: f32,
    pub mix: f32,
}

impl Default for ClampParams {
    fn default() -> Self {
        let get = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            threshold_db: get(p::THRESHOLD),
            ratio: get(p::RATIO),
            knee_db: get(p::KNEE),
            attack_ms: get(p::ATTACK),
            release_ms: get(p::RELEASE),
            makeup_db: get(p::MAKEUP),
            sc_hp_hz: get(p::SC_HP),
            warmth: get(p::WARMTH),
            mix: get(p::MIX),
        }
    }
}

impl ClampParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::THRESHOLD => self.threshold_db = value,
            p::RATIO => self.ratio = value,
            p::KNEE => self.knee_db = value,
            p::ATTACK => self.attack_ms = value,
            p::RELEASE => self.release_ms = value,
            p::MAKEUP => self.makeup_db = value,
            p::SC_HP => self.sc_hp_hz = value,
            p::WARMTH => self.warmth = value,
            p::MIX => self.mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::THRESHOLD => self.threshold_db,
            p::RATIO => self.ratio,
            p::KNEE => self.knee_db,
            p::ATTACK => self.attack_ms,
            p::RELEASE => self.release_ms,
            p::MAKEUP => self.makeup_db,
            p::SC_HP => self.sc_hp_hz,
            p::WARMTH => self.warmth,
            p::MIX => self.mix,
            _ => return None,
        })
    }

    /// Every row back inside its own range, non-numbers replaced.
    ///
    /// RON round-trips NaN, and a NaN threshold does not merely sound
    /// wrong — it makes the gain computer emit NaN, which the ballistics
    /// then hold forever and the master bus wears for the rest of the
    /// session.
    pub fn sanitize(&mut self) {
        for def in p::TABLE {
            let value = self.get(def.id).unwrap_or(def.default);
            self.set(
                def.id,
                if value.is_finite() {
                    value.clamp(def.min, def.max)
                } else {
                    def.default
                },
            );
        }
    }
}

/// The settings that cost something to rebuild, resolved once a segment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    attack_ms: f32,
    release_ms: f32,
    sc_hp_hz: f32,
    warmth: f32,
}

impl Resolved {
    fn of(params: &ClampParams) -> Self {
        Self {
            threshold_db: params.threshold_db,
            ratio: params.ratio,
            knee_db: params.knee_db,
            attack_ms: params.attack_ms,
            release_ms: params.release_ms,
            sc_hp_hz: params.sc_hp_hz,
            warmth: params.warmth,
        }
    }
}

fn db_to_gain(db: f32) -> f32 {
    crate::dsp::arith::db_to_gain(db)
}

pub struct Clamp {
    detector: RmsDetector,
    sc_hp: OnePole,
    computer: GainComputer,
    ballistics: Ballistics,
    warmth: Preamp,
    prepared: Resolved,
    params: ClampParams,
    /// Ramped across the segment so a knob move glides rather than steps.
    makeup: f32,
    mix: f32,
    reduction_db: f32,
    said: crate::audio::graph::Readout,
    sample_rate: f32,
}

impl Clamp {
    /// Green zone.
    pub fn new(sample_rate: f32, params: &ClampParams) -> Self {
        let mut params = *params;
        params.sanitize();
        let sample_rate = sample_rate.max(1.0);
        let resolved = Resolved::of(&params);

        let mut detector = RmsDetector::new();
        detector.prepare(sample_rate, DETECT_MS);
        let mut sc_hp = OnePole::new();
        sc_hp.prepare(sample_rate, resolved.sc_hp_hz);
        let mut computer = GainComputer::new();
        computer.configure(
            Mode::Compress,
            resolved.threshold_db,
            resolved.ratio,
            resolved.knee_db,
        );
        let mut ballistics = Ballistics::new();
        ballistics.prepare(sample_rate, resolved.attack_ms, resolved.release_ms);
        // NO AUTO RELEASE, ever. Program-dependence is exactly what you
        // do not want when you are aiming at one event — it is glue's
        // whole character and this device's opposite.
        ballistics.set_auto(false);
        let mut warmth = Preamp::new();
        warmth.prepare(sample_rate);
        warmth.set_amount(resolved.warmth * p::WARMTH_CEILING);

        Self {
            detector,
            sc_hp,
            computer,
            ballistics,
            warmth,
            prepared: resolved,
            params,
            makeup: db_to_gain(params.makeup_db),
            mix: params.mix,
            reduction_db: 0.0,
            said: crate::audio::graph::Readout::default(),
            sample_rate,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        *self = Self::new(sample_rate, &self.params);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> &ClampParams {
        &self.params
    }

    /// What the device is doing, for the card to draw.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        self.said
    }

    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Red zone: reset every piece of state, for a discontinuity.
    pub fn reset(&mut self) {
        self.detector.reset();
        self.sc_hp.reset();
        self.ballistics.reset();
        self.warmth.reset();
        self.reduction_db = 0.0;
    }

    pub fn latency(&self) -> usize {
        self.ballistics.latency() + self.warmth.latency()
    }

    /// Red zone: compress `l` and `r` in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;

        // --- the segment prologue: rebuild only what moved -------------
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            if want.threshold_db != self.prepared.threshold_db
                || want.ratio != self.prepared.ratio
                || want.knee_db != self.prepared.knee_db
            {
                self.computer.configure(
                    Mode::Compress,
                    want.threshold_db,
                    want.ratio,
                    want.knee_db,
                );
            }
            if want.attack_ms != self.prepared.attack_ms
                || want.release_ms != self.prepared.release_ms
            {
                self.ballistics
                    .prepare(self.sample_rate, want.attack_ms, want.release_ms);
            }
            if want.sc_hp_hz != self.prepared.sc_hp_hz {
                self.sc_hp.prepare(self.sample_rate, want.sc_hp_hz);
            }
            if want.warmth != self.prepared.warmth {
                self.warmth.set_amount(want.warmth * p::WARMTH_CEILING);
            }
            self.prepared = want;
        }

        // --- the two levels, ramped across the segment -----------------
        let makeup_to = db_to_gain(self.params.makeup_db);
        let mix_to = self.params.mix.clamp(0.0, 1.0);
        let makeup_step = (makeup_to - self.makeup) / n as f32;
        let mix_step = (mix_to - self.mix) / n as f32;

        let mut peak_db = crate::dsp::dynamics::FLOOR_DB;
        let mut worst_db = 0.0f32;

        for index in 0..n {
            let dry_l = l.get(index).copied().unwrap_or(0.0);
            let dry_r = if stereo {
                r.get(index).copied().unwrap_or(0.0)
            } else {
                dry_l
            };
            // ONE detector over both channels, so the image cannot move.
            // Two independent ones would pull a centred source toward
            // whichever side happened to be louder, which is the classic
            // way a stereo compressor wanders.
            let sum = 0.5 * (dry_l + dry_r);
            // The sidechain high-pass deafens the DETECTOR only; the
            // audio path never sees it. A filtered signal coming out
            // would be an equaliser nobody asked for.
            let detect = self.sc_hp.tick_highpass(sum);
            let env = self.detector.tick(detect);
            let level_db = crate::dsp::arith::gain_to_db(env.max(1e-7));
            peak_db = peak_db.max(level_db);

            let target_db = self.computer.gain_db(level_db);
            let gain_db = self.ballistics.tick(target_db);
            worst_db = worst_db.min(gain_db);
            let gain = db_to_gain(gain_db);

            self.makeup += makeup_step;
            self.mix += mix_step;
            let wet_l = dry_l * gain * self.makeup;
            let wet_r = dry_r * gain * self.makeup;
            if let Some(slot) = l.get_mut(index) {
                *slot = dry_l + (wet_l - dry_l) * self.mix;
            }
            if stereo && let Some(slot) = r.get_mut(index) {
                *slot = dry_r + (wet_r - dry_r) * self.mix;
            }
        }
        self.makeup = makeup_to;
        self.mix = mix_to;

        // --- the warmth, AFTER the compression -------------------------
        //
        // After, because it is an output stage and there is one of those.
        // Before, it would be feeding the detector its own distortion and
        // the compressor would be reacting to its own colour.
        if !self.warmth.is_bypassed() {
            if stereo {
                self.warmth.process(l, r);
            } else {
                // Mono: the stage is stereo, so the same buffer cannot be
                // handed to it twice. The right channel is a scratch of
                // one sample at a time, which costs nothing and keeps the
                // stage's own arithmetic identical either way.
                let mut mirror = [0.0f32; 1];
                for sample in l.iter_mut() {
                    mirror[0] = *sample;
                    let mut one = [*sample];
                    self.warmth.process(&mut one, &mut mirror);
                    *sample = one[0];
                }
            }
        }

        self.reduction_db = worst_db;
        self.said = crate::audio::graph::Readout {
            level_db: peak_db,
            reduction_db: worst_db,
            bands: [0.0; 3],
        };
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn comp(params: ClampParams) -> Clamp {
        Clamp::new(RATE, &params)
    }

    /// A sine at a given peak, `frames` long.
    fn tone(frames: usize, hz: f32, amp: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| (i as f32 / RATE * hz * core::f32::consts::TAU).sin() * amp)
            .collect()
    }

    fn peak(block: &[f32]) -> f32 {
        block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// SILENCE IN, SILENCE OUT — including with the colour on.
    #[test]
    fn silence_stays_silent_at_every_warmth() {
        for warmth in [0.0, 0.5, 1.0] {
            let mut clamp = comp(ClampParams {
                warmth,
                ..ClampParams::default()
            });
            let mut l = vec![0.0; 2048];
            let mut r = vec![0.0; 2048];
            clamp.process(&mut l, &mut r);
            assert_eq!(peak(&l), 0.0, "warmth {warmth} leaked");
            assert_eq!(peak(&r), 0.0);
        }
    }

    /// IT COMPRESSES: over the threshold comes down, under it does not.
    #[test]
    fn it_reduces_above_the_threshold_and_not_below() {
        let quiet = {
            let mut clamp = comp(ClampParams {
                warmth: 0.0,
                ..ClampParams::default()
            });
            // -30 dBFS, well under a -12 threshold.
            let mut l = tone(24_000, 220.0, 0.0316);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            clamp.reduction_db()
        };
        assert!(
            quiet > -0.5,
            "it compressed a signal under the threshold: {quiet} dB"
        );

        let loud = {
            let mut clamp = comp(ClampParams {
                warmth: 0.0,
                ..ClampParams::default()
            });
            // 0 dBFS, twelve over.
            let mut l = tone(24_000, 220.0, 1.0);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            clamp.reduction_db()
        };
        assert!(
            loud < -4.0,
            "twelve dB over a 4:1 threshold should come down: {loud} dB"
        );
    }

    /// A HIGHER RATIO TAKES MORE OFF. The knob has to mean its number.
    #[test]
    fn a_steeper_ratio_reduces_more() {
        let at = |ratio: f32| {
            let mut clamp = comp(ClampParams {
                ratio,
                warmth: 0.0,
                ..ClampParams::default()
            });
            let mut l = tone(24_000, 220.0, 1.0);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            clamp.reduction_db()
        };
        let gentle = at(2.0);
        let steep = at(20.0);
        assert!(
            steep < gentle - 3.0,
            "20:1 should take much more than 2:1: {steep} against {gentle}"
        );
    }

    /// IT IS FAST — which is the entire reason this device exists beside
    /// `glue`.
    ///
    /// A step from silence to full scale, and the question is how long
    /// the overshoot lasts. At the fastest attack it must be gone in a
    /// small fraction of the time the slowest one takes.
    #[test]
    fn the_fast_attack_catches_what_the_slow_one_lets_through() {
        let overshoot = |attack_ms: f32| {
            let mut clamp = comp(ClampParams {
                attack_ms,
                ratio: 20.0,
                threshold_db: -24.0,
                warmth: 0.0,
                makeup_db: 0.0,
                ..ClampParams::default()
            });
            let mut l = tone(4_800, 2_000.0, 1.0);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            // How loud the first millisecond is: what got past before
            // the compressor had hold of it.
            peak(&l[..48])
        };
        let fast = overshoot(p::ATTACK_MIN_MS);
        let slow = overshoot(p::ATTACK_MAX_MS);
        assert!(
            fast < slow * 0.6,
            "the fast attack let through {fast} and the slow one {slow} — \
             it is not catching anything"
        );
    }

    /// THE SIDECHAIN FILTER DEAFENS THE DETECTOR, NOT THE AUDIO.
    ///
    /// A filtered signal coming OUT would be an equaliser nobody asked
    /// for. What the knob does is stop the compressor hearing the bass.
    #[test]
    fn the_sidechain_filter_is_only_in_the_detector() {
        let reduction = |sc_hp_hz: f32| {
            let mut clamp = comp(ClampParams {
                sc_hp_hz,
                threshold_db: -20.0,
                ratio: 8.0,
                warmth: 0.0,
                ..ClampParams::default()
            });
            // 40 Hz, well under a 500 Hz corner.
            let mut l = tone(24_000, 40.0, 1.0);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            (clamp.reduction_db(), peak(&l))
        };
        let (heard, _) = reduction(p::SC_HP_MIN_HZ);
        let (deaf, out) = reduction(p::SC_HP_MAX_HZ);
        assert!(
            heard < -3.0,
            "with the filter open it should hear the bass: {heard}"
        );
        assert!(
            deaf > heard + 2.0,
            "a 500 Hz corner should deafen it to 40 Hz: {deaf} against {heard}"
        );
        // And the bass is still THERE, not filtered away.
        assert!(
            out > 0.5,
            "the sidechain filter reached the audio path: {out}"
        );
    }

    /// THE WARMTH IS SUBSTANTIALLY LESS THAN GLUE'S COLOUR.
    ///
    /// The device's whole claim against the other compressor. Measured
    /// as added harmonic content on a pure tone: the stage this runs is
    /// the same one the sampler and the strip run, and what makes this
    /// device surgical is that it is only allowed a third of it.
    ///
    /// Compared against the SAME stage at full amount rather than
    /// against glue's clipper, because glue's colour is a different
    /// mechanism — a peak clipper at the rails — and comparing two
    /// different distortions by number would be comparing nothing.
    /// What is pinned here is that this device cannot reach what that
    /// stage can do.
    #[test]
    fn the_warmth_stops_well_short_of_the_stage_it_runs() {
        // A tone the compressor will not touch, so what changes is the
        // colour and nothing else.
        let distortion = |amount: f32| -> f32 {
            let mut preamp = Preamp::new();
            preamp.prepare(RATE);
            preamp.set_amount(amount);
            let mut l = tone(4_096, 440.0, 0.5);
            let mut r = l.clone();
            preamp.process(&mut l, &mut r);
            // Total energy away from the input, which for a pure tone
            // through a memoryless curve is the harmonics it added.
            let clean = tone(4_096, 440.0, 0.5);
            let scale = peak(&l) / peak(&clean).max(1e-9);
            l.iter()
                .zip(clean.iter())
                .skip(512)
                .map(|(a, b)| (a - b * scale).abs())
                .sum::<f32>()
                / (l.len() - 512) as f32
        };
        let ours = distortion(p::WARMTH_CEILING);
        let full = distortion(1.0);
        assert!(
            ours < full * 0.5,
            "this device's ceiling colours {ours} where the stage reaches \
             {full} — that is not substantially less"
        );
        const {
            assert!(
                p::WARMTH_CEILING < 0.5,
                "the ceiling is the claim; it has to be under half"
            )
        };
    }

    /// AND THE WARMTH BYPASSES EXACTLY.
    #[test]
    fn no_warmth_is_bit_for_bit_no_warmth() {
        let run = |warmth: f32| {
            let mut clamp = comp(ClampParams {
                warmth,
                // Nothing to compress, so any difference is the colour.
                threshold_db: 0.0,
                ratio: 1.0,
                ..ClampParams::default()
            });
            let mut l = tone(2_048, 440.0, 0.5);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            l
        };
        let dry = run(0.0);
        let clean = tone(2_048, 440.0, 0.5);
        for (index, (a, b)) in dry.iter().zip(clean.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "warmth at zero changed sample {index}: {a} against {b}"
            );
        }
        assert!(run(1.0) != dry, "warmth at full changed nothing");
    }

    /// SPLIT-BLOCK EQUIVALENCE: the transport splits blocks wherever a
    /// loop point falls, and the same audio has to come out.
    #[test]
    fn processing_in_pieces_is_processing_whole() {
        let source = tone(1_024, 220.0, 0.9);
        let mut whole = comp(ClampParams::default());
        let mut l = source.clone();
        let mut r = source.clone();
        whole.process(&mut l, &mut r);

        let mut split = comp(ClampParams::default());
        let mut sl = source.clone();
        let mut sr = source.clone();
        let mut at = 0;
        for len in [100, 1, 411, 512] {
            let (left, right) = (&mut sl[at..at + len], &mut sr[at..at + len]);
            split.process(left, right);
            at += len;
        }
        for (index, (a, b)) in l.iter().zip(sl.iter()).enumerate() {
            assert_eq!(a, b, "left diverged at {index}");
        }
    }

    /// EDGE LENGTHS, and nothing invented from nothing.
    #[test]
    fn odd_lengths_and_nonsense_stay_finite() {
        let mut clamp = comp(ClampParams::default());
        for len in [0, 1, 2, 7, 63, 129] {
            let mut l = tone(len, 300.0, 0.8);
            let mut r = l.clone();
            clamp.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        // A patch full of nonsense comes back in range rather than
        // poisoning the bus.
        let mut params = ClampParams {
            threshold_db: f32::NAN,
            ratio: f32::INFINITY,
            attack_ms: -1.0,
            ..ClampParams::default()
        };
        params.sanitize();
        for def in p::TABLE {
            let value = params.get(def.id).unwrap_or(f32::NAN);
            assert!(
                value.is_finite() && value >= def.min && value <= def.max,
                "{} came back as {value}",
                def.name
            );
        }
    }

    /// MONO IS NOT HALF A STEREO DEVICE.
    ///
    /// The colour stage is stereo and cannot be handed one buffer twice,
    /// so the mono path takes a different route through it — and the
    /// thing that must not differ is the answer.
    #[test]
    fn a_mono_run_is_the_left_of_a_stereo_one() {
        let source = tone(1_024, 220.0, 0.9);
        let mut stereo = comp(ClampParams::default());
        let mut l = source.clone();
        let mut r = source.clone();
        stereo.process(&mut l, &mut r);

        let mut mono = comp(ClampParams::default());
        let mut m = source.clone();
        let mut empty: [f32; 0] = [];
        mono.process(&mut m, &mut empty);
        for (index, (a, b)) in l.iter().zip(m.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "mono and stereo disagree at {index}: {a} against {b}"
            );
        }
    }

    /// The red-zone promise.
    #[test]
    fn processing_does_not_allocate() {
        let mut clamp = comp(ClampParams::default());
        let mut l = tone(512, 220.0, 0.9);
        let mut r = l.clone();
        assert_no_alloc::assert_no_alloc(|| {
            clamp.process(&mut l, &mut r);
            clamp.set_param(p::THRESHOLD, -24.0);
            clamp.set_param(p::WARMTH, 1.0);
            clamp.process(&mut l, &mut r);
            clamp.reset();
        });
    }
}
