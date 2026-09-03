//! The tom synth's voice — the instrument half of `Node::Tom`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when.
//!
//! # The path, in order
//!
//! ```text
//! bend env ─▶ sine ──▶ amp env ──┐
//! white noise ─▶ stick env ──────┴─▶ LOWPASS ─▶ SATURATOR ─▶ out
//! ```
//!
//! # The simplest drum in the rack, deliberately
//!
//! One sine, one bend, one stick, one lowpass. A tom is a kick with a
//! shorter drop and a longer tail, and the temptation is therefore to
//! give it the kick's whole panel — two pitch envelopes, a disperser, a
//! harmonic. Resist it: the reason to reach for a tom instead of a kick
//! is that a tom is the one you do not have to think about. Nine rows is
//! the budget, and every one of them changes something you can name.
//!
//! # Why the lowpass is after the sum
//!
//! It is the SKIN, not a tone control on the stick. A drumhead damps the
//! whole sound — the beater's click included — and filtering the noise
//! alone would leave a bright tick on the front of a dull drum, which is
//! the one combination a real tom never makes.

use crate::dsp::adsr::ExpDecay;
use crate::dsp::filters::OnePole;
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::tom as tp;

/// How often the bent pitch is rebuilt, in samples. The kick's figure,
/// for the kick's reason: 0.67 ms at 48 kHz is a glide rather than a
/// staircase even at the shortest bend the table allows.
pub const CHUNK: usize = 32;

/// A tom's settings, in engine units. Parallel to
/// [`params::tom::TABLE`](crate::params::tom::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TomParams {
    pub tune_hz: f32,
    pub decay_ms: f32,
    pub bend: f32,
    pub bend_ms: f32,
    pub stick: f32,
    pub stick_ms: f32,
    pub tone_hz: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for TomParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new tom sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(tp::TABLE, id).default;
        Self {
            tune_hz: at(tp::TUNE),
            decay_ms: at(tp::DECAY),
            bend: at(tp::BEND),
            bend_ms: at(tp::BEND_TIME),
            stick: at(tp::STICK),
            stick_ms: at(tp::STICK_DECAY),
            tone_hz: at(tp::TONE),
            drive: at(tp::DRIVE),
            gain: at(tp::GAIN),
        }
    }
}

impl TomParams {
    /// Write one parameter by table id. Clamped through the table, so a
    /// letter, a project file and a knob all land in the same range.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(tp::TABLE, param, value) else {
            return;
        };
        match param {
            tp::TUNE => self.tune_hz = value,
            tp::DECAY => self.decay_ms = value,
            tp::BEND => self.bend = value,
            tp::BEND_TIME => self.bend_ms = value,
            tp::STICK => self.stick = value,
            tp::STICK_DECAY => self.stick_ms = value,
            tp::TONE => self.tone_hz = value,
            tp::DRIVE => self.drive = value,
            tp::GAIN => self.gain = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            tp::TUNE => self.tune_hz,
            tp::DECAY => self.decay_ms,
            tp::BEND => self.bend,
            tp::BEND_TIME => self.bend_ms,
            tp::STICK => self.stick,
            tp::STICK_DECAY => self.stick_ms,
            tp::TONE => self.tone_hz,
            tp::DRIVE => self.drive,
            tp::GAIN => self.gain,
            _ => 0.0,
        }
    }
}

/// A note's own pitch as a multiplier on the tuned fundamental.
///
/// MIDI 45 — GM's low tom — is unity. A drum kit's toms are three notes
/// on one map, so a sequence that walks 45, 47, 50 plays a tom fill on
/// one instrument without anybody transposing anything by hand.
fn pitch_scale(pitch: u8) -> f32 {
    const ROOT: f32 = 45.0;
    ((f32::from(pitch) - ROOT) / 12.0).exp2()
}

/// The tom's single voice.
///
/// ONE voice, as the kick and snare have one: a drum retriggered before
/// it has finished cuts its own tail rather than layering on it.
pub struct TomVoice {
    sample_rate: f32,
    params: TomParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch, which a lock may hold
    /// elsewhere for one hit.
    base: TomParams,

    osc: MipOsc,
    /// The sine's band-limited tables. Built once at prepare — green
    /// zone — and read-only afterwards.
    tables: Vec<f32>,
    noise: WhiteNoise,
    skin: OnePole,

    amp: ExpDecay,
    bend_env: ExpDecay,
    stick_env: ExpDecay,

    shaper: Waveshaper,

    note_scale: f32,
    velocity: f32,

    /// What the lowpass is currently tuned to, so a block only pays for a
    /// re-tune when the knob actually moved.
    tuned_hz: f32,

    /// How many samples are left in the current control chunk. On the
    /// VOICE'S timeline, so 256 keeps equalling 100 + 156.
    chunk_left: usize,

    env_scratch: [f32; CHUNK],
    noise_scratch: [f32; CHUNK],
}

impl Default for TomVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl TomVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: TomParams::default(),
            base: TomParams::default(),
            osc: MipOsc::new(),
            tables: Vec::new(),
            noise: WhiteNoise::new(),
            skin: OnePole::new(),
            amp: ExpDecay::new(),
            bend_env: ExpDecay::new(),
            stick_env: ExpDecay::new(),
            shaper: Waveshaper::new(),
            note_scale: 1.0,
            velocity: 1.0,
            tuned_hz: 0.0,
            chunk_left: 0,
            env_scratch: [0.0; CHUNK],
            noise_scratch: [0.0; CHUNK],
        }
    }

    /// Green zone: build the sine tables and settle every kernel. The ONE
    /// allocation in this file, and it happens at compile.
    pub fn prepare(&mut self, sample_rate: f32, params: TomParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.tables);
        self.osc.prepare(self.sample_rate, Waveform::Sine);
        self.noise.seed(0x7031_1971_c0ff_ee01);
        self.params = params;
        self.apply_envelopes();
        self.retune(true);
        self.reset();
    }

    /// Envelope timings, rebuilt from the params. All three are one-shots:
    /// a tom has no sustain stage to be in.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        self.amp.prepare(fs, self.params.decay_ms);
        self.bend_env.prepare(fs, self.params.bend_ms);
        self.stick_env.prepare(fs, self.params.stick_ms);
    }

    /// The skin's damping, re-tuned only when the knob moved.
    fn retune(&mut self, force: bool) {
        let hz = self.params.tone_hz.clamp(20.0, self.sample_rate * 0.45);
        if !force && (hz - self.tuned_hz).abs() <= self.tuned_hz.max(1.0) * 1e-4 {
            return;
        }
        self.tuned_hz = hz;
        self.skin.prepare(self.sample_rate, hz);
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.osc.reset();
        self.noise.reset();
        self.skin.reset();
        self.amp.reset();
        self.bend_env.reset();
        self.stick_env.reset();
    }

    /// A letter: the knob moves, and the live patch with it.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

    /// A parameter LOCK at a note boundary: `Some` holds the live patch
    /// at the note's own value, `None` returns it to the knob. The knob
    /// itself never moves, so a lock is heard on its hit and no other.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let value = value.unwrap_or_else(|| self.base.get(param));
        self.apply_param(param, value);
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        let live = self.params.get(param);
        let base = self.base.get(param);
        self.apply_param(param, live + (base - live) * alpha.clamp(0.0, 1.0));
    }

    fn apply_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            tp::DECAY | tp::BEND_TIME | tp::STICK_DECAY => self.apply_envelopes(),
            tp::TONE => self.retune(false),
            _ => {}
        }
    }

    pub fn params(&self) -> TomParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.amp.active() || self.stick_env.active()
    }

    /// Strike. Every envelope restarts from silence together.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.note_scale = pitch_scale(pitch);
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        // A known starting phase, so every hit has the same attack and
        // the same peak.
        self.osc.reset();
        self.chunk_left = 0;
        self.amp.trigger(1.0);
        self.bend_env.trigger(1.0);
        self.stick_env.trigger(1.0);
    }

    /// Red zone: render into `out`, ADDING to what is there.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.tables.is_empty() {
            return;
        }
        let mut written = 0usize;
        while written < out.len() {
            if self.chunk_left == 0 {
                self.start_chunk();
                self.chunk_left = CHUNK;
            }
            let take = self.chunk_left.min(out.len() - written);
            let Some(block) = out.get_mut(written..written + take) else {
                return;
            };
            self.render_run(block, gain);
            self.chunk_left -= take;
            written += take;
        }
    }

    /// The start of a control chunk: rebuild what costs a transcendental.
    fn start_chunk(&mut self) {
        let bend = self.bend_env.current() * self.params.bend;
        let hz = self.params.tune_hz * self.note_scale * (bend / 12.0).exp2();
        self.osc.set_freq(hz.clamp(1.0, self.sample_rate * 0.45));
    }

    /// One run of samples inside a control chunk: per-sample work only.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(env), Some(noise)) = (
            self.env_scratch.get_mut(..n),
            self.noise_scratch.get_mut(..n),
        ) else {
            return;
        };

        // --- the body: sine × amp envelope ----------------------------
        self.osc.process(out, &self.tables);
        self.amp.process(env);
        let velocity = self.velocity;
        for (sample, level) in out.iter_mut().zip(env.iter()) {
            *sample *= *level * velocity;
        }

        // --- the stick: a very short noise burst -----------------------
        if self.params.stick > 0.0 {
            self.noise.process(noise);
            self.stick_env.process(env);
            let level = self.params.stick * velocity;
            for (sample, (tick, decay)) in out.iter_mut().zip(noise.iter().zip(env.iter())) {
                *sample += *tick * *decay * level;
            }
        } else {
            // Advanced whether or not anything reads it, so the stick's
            // timing never depends on whether it is switched on.
            self.stick_env.process(env);
        }
        self.bend_env.process(env);

        // --- the skin, then the shaping -------------------------------
        self.skin.process_lowpass(out);
        self.shaper
            .configure(ShapeMode::SoftClip, self.params.drive, 0.0, 1.0);
        self.shaper.process(out);

        let level = self.params.gain * gain;
        for sample in out.iter_mut() {
            *sample *= level;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(params: TomParams) -> TomVoice {
        let mut v = TomVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut TomVoice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    fn crossings(buf: &[f32]) -> usize {
        buf.windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    }

    #[test]
    fn a_fresh_tom_is_silent_until_it_is_struck() {
        let mut v = voice(TomParams::default());
        let quiet = render(&mut v, 512);
        assert!(quiet.iter().all(|s| *s == 0.0), "silent before the strike");
        assert!(!v.active());

        v.trigger(45, 100);
        assert!(v.active());
        let hit = render(&mut v, 512);
        assert!(peak(&hit) > 0.05, "the strike made sound: {}", peak(&hit));
    }

    /// THE BEND FALLS AND THEN STOPS. A tom's drop is short and shallow —
    /// if it were still bending at a quarter of a second it would be a
    /// kick, and if it never bent at all it would be a sine.
    #[test]
    fn the_pitch_bends_at_the_start_and_settles() {
        let base = TomParams {
            stick: 0.0,
            drive: 1.0,
            decay_ms: 4_000.0,
            tone_hz: 12_000.0,
            tune_hz: 120.0,
            ..TomParams::default()
        };
        let window = 480usize; // 10 ms
        let take = |bend: f32| {
            let mut v = voice(TomParams {
                bend,
                bend_ms: 40.0,
                ..base
            });
            v.trigger(45, 127);
            let buf = render(&mut v, window * 40);
            let early = crossings(&buf[..window]);
            let late = crossings(&buf[window * 30..window * 31]);
            (early, late)
        };

        let (flat_early, flat_late) = take(0.0);
        let (bent_early, bent_late) = take(12.0);

        assert!(
            bent_early > flat_early,
            "the bend must lift the start: {bent_early} against {flat_early}"
        );
        assert!(
            bent_late <= flat_late + 2,
            "and be finished by 300 ms: {bent_late} against {flat_late}"
        );
    }

    /// THE SKIN DAMPS THE WHOLE SOUND, stick included. Closing the tone
    /// knob must take the tick off the front, not just soften the body —
    /// a bright tick on a dull drum is the one thing a real tom never is.
    #[test]
    fn the_tone_knob_damps_the_stick_as_well_as_the_body() {
        let params = TomParams {
            stick: 1.0,
            stick_ms: 8.0,
            drive: 1.0,
            ..TomParams::default()
        };
        let attack = 96usize; // 2 ms, where the stick lives

        let mut open = voice(TomParams {
            tone_hz: 12_000.0,
            ..params
        });
        open.trigger(45, 127);
        let bright = render(&mut open, attack);

        let mut shut = voice(TomParams {
            tone_hz: 200.0,
            ..params
        });
        shut.trigger(45, 127);
        let dull = render(&mut shut, attack);

        assert!(
            crossings(&dull) < crossings(&bright),
            "closing the tone must take content off the stick: {} against {}",
            crossings(&dull),
            crossings(&bright)
        );
    }

    /// A RETRIGGER REPLACES THE HIT rather than stacking 6 dB onto it.
    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let params = TomParams {
            decay_ms: 2_000.0,
            drive: 1.0,
            ..TomParams::default()
        };
        let mut once = voice(params);
        once.trigger(45, 127);
        let single = peak(&render(&mut once, 4_800));

        let mut twice = voice(params);
        twice.trigger(45, 127);
        let _ = render(&mut twice, 240);
        twice.trigger(45, 127);
        let doubled = peak(&render(&mut twice, 4_800));

        assert!(
            doubled <= single * 1.05,
            "the retrigger stacked: {doubled} against {single}"
        );
    }

    /// Split-block equivalence — the property the segmented transport
    /// depends on.
    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = TomParams::default();
        let mut whole = voice(params);
        whole.trigger(45, 100);
        let mut a = vec![0.0f32; 256];
        whole.render_add(&mut a, 1.0);

        let mut split = voice(params);
        split.trigger(45, 100);
        let mut b = vec![0.0f32; 256];
        split.render_add(&mut b[..100], 1.0);
        split.render_add(&mut b[100..], 1.0);

        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "256 must equal 100 + 156"
        );
    }

    /// The render path allocates nothing.
    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(TomParams {
            stick: 1.0,
            drive: 8.0,
            ..TomParams::default()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(45, 100);
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    /// Nonsense in, finite out — at every extreme the table allows.
    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in tp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = TomParams::default();
                params.set(def.id, value);
                let mut v = voice(params);
                for pitch in [0u8, 45, 127] {
                    v.trigger(pitch, 127);
                    let out = render(&mut v, 1_024);
                    assert!(
                        out.iter().all(|s| s.is_finite()),
                        "{} at {value}, pitch {pitch}",
                        def.name
                    );
                }
            }
        }

        let mut bare = TomVoice::new();
        bare.trigger(45, 127);
        let mut out = vec![0.0f32; 128];
        bare.render_add(&mut out, 1.0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Every table id round-trips, and an unknown id is ignored.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = TomParams::default();
        for def in tp::TABLE {
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

        params.set(tp::TUNE, 1e9);
        assert!((params.tune_hz - tp::TUNE_MAX).abs() < 1e-3);
        params.set(tp::TUNE, -1e9);
        assert!((params.tune_hz - tp::TUNE_MIN).abs() < 1e-3);
    }

    /// GM's low tom is the tuned note, so a fill written 45/47/50 plays
    /// as a fill rather than as one drum transposed by hand.
    #[test]
    fn the_general_midi_low_tom_is_the_tuned_note() {
        assert!((pitch_scale(45) - 1.0).abs() < 1e-6);
        assert!((pitch_scale(57) - 2.0).abs() < 1e-5);
        assert!(pitch_scale(50) > pitch_scale(45), "a fill walks upward");
    }
}
