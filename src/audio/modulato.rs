//! Modulato — chorus, flanger and vibrato, which are one effect.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`. This file says which kernel feeds which.
//!
//! # Why one device
//!
//! All three are a delay line whose time wobbles, mixed back with the
//! dry. They differ in WHERE they sit in three numbers:
//!
//! | | base delay | feedback | mix |
//! |---|---|---|---|
//! | chorus | 8–40 ms | none | part wet |
//! | flanger | 0.2–10 ms | lots | part wet |
//! | vibrato | 1–15 ms | none | all wet |
//!
//! Under 10 ms the delayed copy is close enough to comb-filter the dry
//! one, and feedback sharpens those notches into the flanger's sweep.
//! Past 10 ms it is heard as a second voice slightly out of tune, which
//! is a chorus. Take the dry away and there is nothing to beat against,
//! so only the wobble is left, which is vibrato.
//!
//! # What `mode` actually does
//!
//! It picks the base delay's RANGE, and nothing else. One knob spanning
//! 0.2 to 40 ms spends nine tenths of its travel in chorus territory and
//! makes a flanger impossible to tune by hand.
//!
//! Every other control stays live in every mode. The card moves feedback
//! and mix to that mode's usual values when the mode changes — as edits,
//! so they are ordinary knob positions afterwards and can be moved. That
//! is the difference between a mode and a preset, and this is honestly
//! the second: a vibrato with its mix turned down IS a short chorus, and
//! a user who finds that out has learned something true.
//!
//! # Stereo
//!
//! Two lines, two oscillators, one phase offset between them. At zero the
//! sides move together and the effect is mono-compatible; at a half turn
//! they move in opposition and the image widens as far as it goes.

use crate::dsp::delay::FeedbackDelay;
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::params::modulato as mp;

/// The longest base delay plus the deepest swing, in milliseconds. What
/// the delay lines are sized for, so neither knob can ever ask for
/// memory that is not already there.
const MAX_DELAY_MS: f32 = mp::DELAY_MAX_MS + mp::DEPTH_MAX_MS + 1.0;

/// The damping corner inside the feedback loop.
///
/// A flanger with undamped feedback builds an ever-brighter resonance
/// until it screams. Losing a little top on every trip is what real tape
/// and bucket-brigade flangers do, and it is why theirs sing where a
/// naive one shrieks.
const LOOP_DAMP_HZ: f32 = 7_500.0;

/// One modulato's settings, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ModulatoParams {
    pub mode: f32,
    pub rate_hz: f32,
    pub depth_ms: f32,
    pub delay_ms: f32,
    pub feedback: f32,
    pub spread: f32,
    pub mix: f32,
}

impl Default for ModulatoParams {
    fn default() -> Self {
        let at = |id: u32| crate::params::def(mp::TABLE, id).default;
        Self {
            mode: at(mp::MODE),
            rate_hz: at(mp::RATE),
            depth_ms: at(mp::DEPTH),
            delay_ms: at(mp::DELAY),
            feedback: at(mp::FEEDBACK),
            spread: at(mp::SPREAD),
            mix: at(mp::MIX),
        }
    }
}

impl ModulatoParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(mp::TABLE, param, value) else {
            return;
        };
        match param {
            mp::MODE => self.mode = value,
            mp::RATE => self.rate_hz = value,
            mp::DEPTH => self.depth_ms = value,
            mp::DELAY => self.delay_ms = value,
            mp::FEEDBACK => self.feedback = value,
            mp::SPREAD => self.spread = value,
            mp::MIX => self.mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            mp::MODE => self.mode,
            mp::RATE => self.rate_hz,
            mp::DEPTH => self.depth_ms,
            mp::DELAY => self.delay_ms,
            mp::FEEDBACK => self.feedback,
            mp::SPREAD => self.spread,
            _ => self.mix,
        }
    }

    /// The base delay this patch actually asks for, in milliseconds.
    ///
    /// The `delay` knob is a POSITION IN THE MODE'S WINDOW, `0..=1`, not
    /// a time. That is what lets one knob tune a flanger's tenth of a
    /// millisecond and a chorus's thirty with the same gesture — a single
    /// linear range would give the flanger two per cent of its travel.
    pub fn base_delay_ms(&self) -> f32 {
        let (low, high) = mp::window(self.mode);
        low + (high - low) * self.delay_ms.clamp(0.0, 1.0)
    }
}

/// The effect: two modulated feedback delays and their oscillators.
pub struct Modulato {
    sample_rate: f32,
    params: ModulatoParams,
    line_l: FeedbackDelay,
    line_r: FeedbackDelay,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    lfo_l: Lfo,
    lfo_r: Lfo,
    /// Per-sample delay times, one block long, filled before each pass.
    delays_l: Vec<f32>,
    delays_r: Vec<f32>,
    /// The dry copy, kept aside because the lines process in place.
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
}

impl Modulato {
    /// Green zone: every buffer the callback will ever need.
    pub fn new(sample_rate: f32, block_frames: usize, params: ModulatoParams) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let max_samples = (MAX_DELAY_MS * 0.001 * sample_rate).ceil() as usize;
        let len = FeedbackDelay::needed_len(max_samples);
        let mut effect = Self {
            sample_rate,
            params,
            line_l: FeedbackDelay::new(),
            line_r: FeedbackDelay::new(),
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            lfo_l: Lfo::new(),
            lfo_r: Lfo::new(),
            delays_l: vec![0.0; block_frames],
            delays_r: vec![0.0; block_frames],
            dry_l: vec![0.0; block_frames],
            dry_r: vec![0.0; block_frames],
        };
        effect
            .line_l
            .prepare(sample_rate, max_samples, LOOP_DAMP_HZ);
        effect
            .line_r
            .prepare(sample_rate, max_samples, LOOP_DAMP_HZ);
        effect.lfo_l.prepare(sample_rate);
        effect.lfo_r.prepare(sample_rate);
        effect.lfo_l.set_shape(LfoShape::Sine);
        effect.lfo_r.set_shape(LfoShape::Sine);
        effect.apply_all();
        effect
    }

    fn apply_all(&mut self) {
        self.lfo_l.set_rate(self.params.rate_hz);
        self.lfo_r.set_rate(self.params.rate_hz);
        self.lfo_r.set_phase(self.params.spread);
        self.line_l.set_feedback(self.params.feedback);
        self.line_r.set_feedback(self.params.feedback);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            mp::RATE => {
                self.lfo_l.set_rate(self.params.rate_hz);
                self.lfo_r.set_rate(self.params.rate_hz);
            }
            // The right oscillator's phase is set OUTRIGHT rather than
            // nudged, so the offset between the sides is exactly the
            // number asked for however many times the knob has moved.
            // Nudging accumulates, and a spread knob that drifts is a
            // stereo image that wanders over a session.
            mp::SPREAD => self.lfo_r.set_phase(self.params.spread),
            mp::FEEDBACK => {
                self.line_l.set_feedback(self.params.feedback);
                self.line_r.set_feedback(self.params.feedback);
            }
            _ => {}
        }
    }

    pub fn params(&self) -> ModulatoParams {
        self.params
    }

    /// Green zone: silence the lines and restart the oscillators.
    pub fn reset(&mut self) {
        for sample in self.buf_l.iter_mut().chain(self.buf_r.iter_mut()) {
            *sample = 0.0;
        }
        self.line_l.reset();
        self.line_r.reset();
        self.lfo_l.reset();
        self.lfo_r.reset();
        self.lfo_r.set_phase(self.params.spread);
    }

    /// Red zone: process a stereo block in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left
            .len()
            .min(right.len())
            .min(self.delays_l.len())
            .min(self.dry_l.len());
        if n == 0 {
            return;
        }
        let base = self.params.base_delay_ms() * 0.001 * self.sample_rate;
        let swing = self.params.depth_ms * 0.001 * self.sample_rate;
        let mix = self.params.mix.clamp(0.0, 1.0);

        // The oscillators write bipolar values straight into the delay
        // scratches, which are then mapped in place — one pass rather
        // than two, and no third buffer.
        let (Some(mod_l), Some(mod_r)) = (self.delays_l.get_mut(..n), self.delays_r.get_mut(..n))
        else {
            return;
        };
        self.lfo_l.process(mod_l);
        self.lfo_r.process(mod_r);
        for sample in mod_l.iter_mut().chain(mod_r.iter_mut()) {
            // The nominal delay sits in the MIDDLE of the swing, so the
            // knob names the centre of the wobble rather than its floor.
            *sample = base + *sample * swing;
        }

        // The dry, kept aside: the lines process in place.
        let (Some(dry_l), Some(dry_r)) = (self.dry_l.get_mut(..n), self.dry_r.get_mut(..n)) else {
            return;
        };
        let (Some(src_l), Some(src_r)) = (left.get(..n), right.get(..n)) else {
            return;
        };
        dry_l.copy_from_slice(src_l);
        dry_r.copy_from_slice(src_r);

        let (Some(wet_l), Some(wet_r)) = (left.get_mut(..n), right.get_mut(..n)) else {
            return;
        };
        self.line_l
            .process_modulated(wet_l, &mut self.buf_l, &self.delays_l[..n]);
        self.line_r
            .process_modulated(wet_r, &mut self.buf_r, &self.delays_r[..n]);

        for (index, (sample, dry)) in wet_l.iter_mut().zip(dry_l.iter()).enumerate() {
            let _ = index;
            *sample = *dry * (1.0 - mix) + *sample * mix;
        }
        for (sample, dry) in wet_r.iter_mut().zip(dry_r.iter()) {
            *sample = *dry * (1.0 - mix) + *sample * mix;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn effect(params: ModulatoParams) -> Modulato {
        Modulato::new(FS, BLOCK, params)
    }

    fn tone(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 / FS * 220.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    fn run(effect: &mut Modulato, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = tone(frames);
        let mut right = tone(frames);
        let mut at = 0;
        while at < frames {
            let take = BLOCK.min(frames - at);
            let (l, r) = (&mut left[at..at + take], &mut right[at..at + take]);
            effect.process(l, r);
            at += take;
        }
        (left, right)
    }

    /// THE THREE MODES ARE THREE DELAY WINDOWS, and they are the windows
    /// the effects actually live in. A flanger above ten milliseconds is
    /// a chorus, whatever the mode says.
    #[test]
    fn each_mode_tunes_the_delay_where_that_effect_lives() {
        let at = |mode: f32, knob: f32| {
            ModulatoParams {
                mode,
                delay_ms: knob,
                ..ModulatoParams::default()
            }
            .base_delay_ms()
        };
        // Flanger: comb-filtering range, under ten milliseconds.
        assert!(at(mp::MODE_FLANGER, 0.0) < 1.0);
        assert!(at(mp::MODE_FLANGER, 1.0) <= 10.0);
        // Chorus: heard as a second voice, past ten.
        assert!(at(mp::MODE_CHORUS, 0.0) >= 8.0);
        assert!(at(mp::MODE_CHORUS, 1.0) > 20.0);
        // Vibrato: short enough to stay one voice.
        assert!(at(mp::MODE_VIBRATO, 1.0) <= 15.0);

        // The knob spans its whole window in every mode, and never
        // leaves it.
        for mode in [mp::MODE_CHORUS, mp::MODE_FLANGER, mp::MODE_VIBRATO] {
            let (low, high) = mp::window(mode);
            assert!((at(mode, 0.0) - low).abs() < 1e-4);
            assert!((at(mode, 1.0) - high).abs() < 1e-4);
            for step in 0..=10 {
                let value = at(mode, step as f32 / 10.0);
                assert!(value >= low - 1e-4 && value <= high + 1e-4);
            }
        }
    }

    /// MIX AT ZERO IS A WIRE. The dry path has to be exact, or a device
    /// left in a chain at zero quietly colours everything through it.
    #[test]
    fn a_dry_setting_passes_the_signal_through_untouched() {
        let mut effect = effect(ModulatoParams {
            mix: 0.0,
            feedback: 0.6,
            ..ModulatoParams::default()
        });
        let input = tone(1_024);
        let mut left = input.clone();
        let mut right = input.clone();
        let mut at = 0;
        while at < 1_024 {
            let take = BLOCK.min(1_024 - at);
            effect.process(&mut left[at..at + take], &mut right[at..at + take]);
            at += take;
        }
        assert!(
            left.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "dry must be bit-exact"
        );
        assert!(
            right
                .iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }

    /// The two sides differ when spread is up and match when it is not —
    /// which is what makes the effect mono-compatible at one end of the
    /// knob and wide at the other.
    #[test]
    fn spread_separates_the_two_sides() {
        let difference = |spread: f32| {
            let mut effect = effect(ModulatoParams {
                spread,
                depth_ms: 4.0,
                mix: 1.0,
                ..ModulatoParams::default()
            });
            let (left, right) = run(&mut effect, 4_096);
            left.iter()
                .zip(right.iter())
                .map(|(l, r)| f64::from(l - r).abs())
                .sum::<f64>()
        };
        let together = difference(0.0);
        let apart = difference(0.5);
        assert!(
            together < 1e-3,
            "at zero the sides move together: {together}"
        );
        assert!(apart > 1.0, "at a half turn they do not: {apart}");
    }

    /// Depth actually swings the delay: with it at zero the effect is a
    /// fixed delay and the output is steady; with it up the output
    /// wavers.
    #[test]
    fn depth_makes_the_delay_move() {
        let render = |depth: f32| {
            let mut effect = effect(ModulatoParams {
                depth_ms: depth,
                mix: 1.0,
                rate_hz: 2.0,
                ..ModulatoParams::default()
            });
            run(&mut effect, 8_192).0
        };
        let still = render(0.0);
        let moving = render(5.0);
        let difference: f64 = still
            .iter()
            .zip(moving.iter())
            .map(|(a, b)| f64::from(a - b).abs())
            .sum();
        assert!(difference > 1.0, "depth did nothing: {difference}");
        assert!(moving.iter().all(|s| s.is_finite()));
    }

    /// FEEDBACK MUST NOT RUN AWAY. A flanger at the top of its feedback
    /// range sings; it does not scream and it does not go non-finite.
    #[test]
    fn full_feedback_stays_bounded() {
        for feedback in [
            crate::params::def(mp::TABLE, mp::FEEDBACK).min,
            0.0,
            crate::params::def(mp::TABLE, mp::FEEDBACK).max,
        ] {
            let mut effect = effect(ModulatoParams {
                mode: mp::MODE_FLANGER,
                feedback,
                depth_ms: 2.0,
                mix: 1.0,
                ..ModulatoParams::default()
            });
            let (left, right) = run(&mut effect, (FS as usize) * 2);
            let peak = left
                .iter()
                .chain(right.iter())
                .fold(0.0f32, |peak, s| peak.max(s.abs()));
            assert!(peak.is_finite(), "feedback {feedback} went non-finite");
            assert!(peak < 8.0, "feedback {feedback} ran away to {peak}");
        }
    }

    /// Split-block equivalence — the property the segmented transport
    /// depends on.
    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = ModulatoParams {
            depth_ms: 3.0,
            feedback: 0.4,
            mix: 0.7,
            ..ModulatoParams::default()
        };
        let input = tone(256);

        let mut whole = effect(params);
        let (mut wl, mut wr) = (input.clone(), input.clone());
        whole.process(&mut wl, &mut wr);

        let mut split = effect(params);
        let (mut sl, mut sr) = (input.clone(), input.clone());
        split.process(&mut sl[..100], &mut sr[..100]);
        split.process(&mut sl[100..], &mut sr[100..]);

        assert!(
            wl.iter().zip(&sl).all(|(a, b)| a.to_bits() == b.to_bits()),
            "left: 256 must equal 100 + 156"
        );
        assert!(
            wr.iter().zip(&sr).all(|(a, b)| a.to_bits() == b.to_bits()),
            "right: 256 must equal 100 + 156"
        );
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut effect = effect(ModulatoParams {
            mix: 0.5,
            depth_ms: 4.0,
            feedback: 0.5,
            ..ModulatoParams::default()
        });
        let mut left = vec![0.2f32; BLOCK];
        let mut right = vec![0.2f32; BLOCK];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 8 == 0 {
                    effect.set_param(mp::RATE, 0.5 + (i % 5) as f32);
                    effect.set_param(mp::SPREAD, (i % 4) as f32 * 0.16);
                    effect.set_param(mp::FEEDBACK, (i % 3) as f32 * 0.3);
                }
                effect.process(&mut left, &mut right);
            }
        });
    }

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in mp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = ModulatoParams::default();
                params.set(def.id, value);
                let mut effect = effect(params);
                let (left, right) = run(&mut effect, 4_096);
                assert!(
                    left.iter().chain(right.iter()).all(|s| s.is_finite()),
                    "{} at {value}",
                    def.name
                );
            }
        }
        // Edge block lengths, including zero.
        for len in [0usize, 1, 3, 63] {
            let mut effect = effect(ModulatoParams::default());
            let mut left = vec![0.3f32; len];
            let mut right = vec![0.3f32; len];
            effect.process(&mut left, &mut right);
            assert!(left.iter().chain(right.iter()).all(|s| s.is_finite()));
        }
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = ModulatoParams::default();
        for def in mp::TABLE {
            let want = (def.min + def.max) * 0.5;
            params.set(def.id, want);
            assert!(
                (params.get(def.id) - want).abs() < 1e-4,
                "{} did not round-trip",
                def.name
            );
        }
        let before = params;
        params.set(9_999, 1.0);
        assert_eq!(params, before, "an unknown id must change nothing");
    }
}
