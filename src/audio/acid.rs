//! The acid mono — one oscillator, one filter, one envelope, one voice.
//!
//! Wiring, not new arithmetic: [`MipOsc`](crate::dsp::osc::MipOsc) makes
//! the tone, [`Svf`](crate::dsp::filters::Svf) and
//! [`OnePole`](crate::dsp::filters::OnePole) stack into the eighteen
//! decibels the machine is known for, and
//! [`Waveshaper`](crate::dsp::shaper::Waveshaper) is the drive.
//!
//! # The two gestures that need one voice
//!
//! **SLIDE.** A note arriving while the previous one is still held does
//! not retrigger anything. The pitch GLIDES to it and the filter envelope
//! keeps falling from wherever it had got to. That is what the original's
//! slide switch does, and it is a property of a single voice being handed
//! an overlapping pair — a polyphonic instrument would simply start a
//! second note.
//!
//! A note arriving on its own SNAPS to pitch. Glide that applied to
//! everything would be a portamento knob on a different instrument.
//!
//! **ACCENT.** A note above [`ACCENT_VEL`](crate::params::acid::ACCENT_VEL)
//! is louder, opens the filter further, and rings harder — one gesture
//! reaching three destinations, which is why it is a single knob rather
//! than three. The original had a switch per step; our `Note` already
//! stores a velocity, so the threshold is the same gesture in the
//! sequencer we have.
//!
//! # The slope
//!
//! Twelve decibels of resonant `Svf` and six of `OnePole` — eighteen,
//! which is the machine's own and is not a slope a Butterworth cascade
//! offers. The resonance lives in the SVF stage; the one-pole is there
//! for the slope alone.
//!
//! Both filters are re-tuned once per [`CHUNK`] rather than per sample.
//! `prepare` on either sets coefficients and leaves the state alone —
//! their own docs say so, and `Disperser` already relies on it — so a
//! sweeping envelope costs one `tan` every thirty-two samples instead of
//! one every sample, and cannot click.
//!
//! # What has no knob
//!
//! The amplifier's envelope. The original has none: the VCA opens fast
//! when the gate does and shuts fast when it lets go, and the knob
//! labelled DECAY belongs to the filter. `params::acid` argues it.
//!
//! # Red zone
//!
//! `render` allocates nothing and cannot panic. Both wavetable sets are
//! built in [`AcidVoice::prepare`]; the chunk buffer is fixed; every
//! value has been clamped through `params::acid::TABLE` on the way in.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::filters::{Mode as SvfMode, OnePole, Svf};
use crate::dsp::osc::{MipOsc, Waveform, build_tables, table_len};
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::acid as p;
use crate::params::def;

/// How many samples share one re-tuning of the filters and one step of
/// the glide. Two thirds of a millisecond at 48 kHz — under the ear's
/// resolution for a sweep, and a thirty-second of the cost.
const CHUNK: usize = 32;

/// The drive stage's ceiling, as the shaper's own drive figure.
const DRIVE_MAX: f32 = 8.0;

/// An acid mono's editable values, in ENGINE units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AcidParams {
    /// The waveform as its wire INDEX, kept as f32 like every other
    /// engine value here.
    pub wave: f32,
    pub tune_st: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub env_mod: f32,
    pub decay_ms: f32,
    pub accent: f32,
    pub glide_ms: f32,
    pub drive: f32,
    pub level: f32,
}

impl Default for AcidParams {
    fn default() -> Self {
        Self {
            wave: def(p::TABLE, p::WAVE).default,
            tune_st: def(p::TABLE, p::TUNE).default,
            cutoff_hz: def(p::TABLE, p::CUTOFF).default,
            resonance: def(p::TABLE, p::RESONANCE).default,
            env_mod: def(p::TABLE, p::ENV_MOD).default,
            decay_ms: def(p::TABLE, p::DECAY).default,
            accent: def(p::TABLE, p::ACCENT).default,
            glide_ms: def(p::TABLE, p::GLIDE).default,
            drive: def(p::TABLE, p::DRIVE).default,
            level: def(p::TABLE, p::LEVEL).default,
        }
    }
}

impl AcidParams {
    /// This state's value for a wire id. One place an id becomes a field.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::WAVE => self.wave,
            p::TUNE => self.tune_st,
            p::CUTOFF => self.cutoff_hz,
            p::RESONANCE => self.resonance,
            p::ENV_MOD => self.env_mod,
            p::DECAY => self.decay_ms,
            p::ACCENT => self.accent,
            p::GLIDE => self.glide_ms,
            p::DRIVE => self.drive,
            p::LEVEL => self.level,
            _ => return None,
        })
    }

    /// Set by wire id, ignoring anything the table does not describe.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::WAVE => self.wave = value,
            p::TUNE => self.tune_st = value,
            p::CUTOFF => self.cutoff_hz = value,
            p::RESONANCE => self.resonance = value,
            p::ENV_MOD => self.env_mod = value,
            p::DECAY => self.decay_ms = value,
            p::ACCENT => self.accent = value,
            p::GLIDE => self.glide_ms = value,
            p::DRIVE => self.drive = value,
            p::LEVEL => self.level = value,
            _ => {}
        }
    }

    /// Whether the square is selected.
    pub fn square(&self) -> bool {
        self.wave.round() as u32 == p::WAVE_SQUARE
    }
}

/// One acid voice — and there is only ever one, which is the instrument.
#[derive(Debug, Clone)]
pub struct AcidVoice {
    /// The knob positions letters have most recently set. A p-lock
    /// overrides `params` for its note; `None` restores from here.
    base: AcidParams,
    params: AcidParams,
    sample_rate: f32,
    saw: Vec<f32>,
    square: Vec<f32>,
    osc: MipOsc,
    osc_wave: bool,
    svf: Svf,
    pole: OnePole,
    shaper: Waveshaper,
    chunk: Vec<f32>,

    /// Gate: whether a note is being held. The next note-on while this is
    /// true is a SLIDE.
    gate: bool,
    /// The pitch the oscillator is at, in semitones, and where it is
    /// heading. They differ only during a slide.
    pitch: f32,
    target_pitch: f32,
    /// Whether the pitch should glide there or has already snapped.
    sliding: bool,
    /// The filter envelope: one, exponential, falling from 1 to 0.
    env: f32,
    /// The amplifier's own fast envelope.
    amp: f32,
    /// This note's accent, 0..1 — the velocity above the threshold,
    /// scaled by the knob.
    accent: f32,
}

impl Default for AcidVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl AcidVoice {
    pub fn new() -> Self {
        Self {
            base: AcidParams::default(),
            params: AcidParams::default(),
            sample_rate: 48_000.0,
            saw: Vec::new(),
            square: Vec::new(),
            osc: MipOsc::new(),
            osc_wave: false,
            svf: Svf::new(),
            pole: OnePole::new(),
            shaper: Waveshaper::new(),
            chunk: Vec::new(),
            gate: false,
            pitch: 60.0,
            target_pitch: 60.0,
            sliding: false,
            env: 0.0,
            amp: 0.0,
            accent: 0.0,
        }
    }

    /// Green zone: both wavetable sets and the chunk buffer are born
    /// here, so switching waveform in the callback is a pointer swap.
    pub fn prepare(&mut self, sample_rate: f32, params: AcidParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut params = params;
        for row in p::TABLE {
            if let Some(value) = params.get(row.id) {
                params.set(row.id, value);
            }
        }
        self.base = params;
        self.params = params;

        self.saw = vec![0.0; table_len(Waveform::Saw)];
        self.square = vec![0.0; table_len(Waveform::Square)];
        build_tables(Waveform::Saw, &mut self.saw);
        build_tables(Waveform::Square, &mut self.square);
        self.chunk = vec![0.0; CHUNK];

        self.osc_wave = params.square();
        self.osc.prepare(
            self.sample_rate,
            if self.osc_wave {
                Waveform::Square
            } else {
                Waveform::Saw
            },
        );
        self.reset();
    }

    /// Green zone: silence, and forget every trace of the last note.
    pub fn reset(&mut self) {
        self.svf.reset();
        self.pole.reset();
        self.osc.reset();
        self.gate = false;
        self.env = 0.0;
        self.amp = 0.0;
        self.accent = 0.0;
        self.sliding = false;
    }

    /// Red zone: one door for every letter.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.params.set(param, value);
    }

    /// A parameter LOCK at a note boundary. `Some` overrides for this
    /// note; `None` restores the live base — the knob as letters have it
    /// now, not a compile-time snapshot.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        match value {
            Some(value) => self.params.set(param, value),
            None => {
                if let Some(base) = self.base.get(param) {
                    self.params.set(param, base);
                }
            }
        }
    }

    /// This voice's values, for the app to read back.
    ///
    /// The BASE, not `self.params`, and clippy is wrong to suggest
    /// otherwise: `params` carries whatever the last note's p-locks
    /// overrode, so returning it would make the card show a locked note's
    /// settings as if they were the knobs — and the next repaint would
    /// write them back as the knobs. `base` is what letters have set.
    #[allow(clippy::misnamed_getters)]
    pub fn params(&self) -> AcidParams {
        self.base
    }

    /// Whether anything is still sounding.
    pub fn active(&self) -> bool {
        self.gate || self.amp > 1e-4
    }

    /// Red zone: start a note.
    ///
    /// A note arriving while the gate is still up is a SLIDE: the pitch
    /// glides and NOTHING retriggers. That one branch is the whole of the
    /// original's slide behaviour.
    pub fn note_on(&mut self, pitch: u8, vel: u8) {
        let target = f32::from(pitch);
        let slide = self.gate;
        self.target_pitch = target;
        if slide {
            self.sliding = true;
        } else {
            self.pitch = target;
            self.sliding = false;
            // A fresh note restarts the filter envelope. A slid one does
            // not, which is why the second note of a slide pair is darker
            // than the first — and why that is the sound people slide
            // for.
            self.env = 1.0;
        }
        self.gate = true;
        // The accent: everything above the threshold, scaled by the knob.
        let over = f32::from(vel.saturating_sub(p::ACCENT_VEL));
        let span = f32::from(127u8.saturating_sub(p::ACCENT_VEL)).max(1.0);
        self.accent = (over / span).clamp(0.0, 1.0) * self.params.accent.clamp(0.0, 1.0);
    }

    /// Red zone: let the gate down without cutting the tail — what the
    /// transport asks for when it stops, as opposed to `reset`, which is
    /// what it asks for when the POSITION moved.
    pub fn release_all(&mut self) {
        self.gate = false;
    }

    /// Red zone: release the gate.
    pub fn note_off(&mut self, pitch: u8) {
        // Only the note that is sounding may close the gate. On a mono
        // instrument an overlapping pair releases the OLD note after the
        // new one started, and honouring that would cut the slide off at
        // the knees.
        if self.gate && (self.target_pitch - f32::from(pitch)).abs() < 0.5 {
            self.gate = false;
        }
    }

    /// Red zone: render `out.len()` samples, ADDING into the buffer.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.chunk.is_empty() || self.saw.is_empty() {
            return;
        }
        let sr = self.sample_rate;
        // Coefficients for the three one-pole-shaped envelopes. All in
        // per-CHUNK terms except the amp, which is per sample.
        let per_chunk = CHUNK as f32 / sr;
        let decay = (-per_chunk / (self.params.decay_ms * 1e-3).max(1e-6)).exp();
        let glide =
            crate::dsp::ramps::one_pole_coeff(per_chunk / (self.params.glide_ms * 1e-3).max(1e-6));
        let amp_up =
            crate::dsp::ramps::one_pole_coeff(1.0 / (p::AMP_ATTACK_MS * 1e-3 * sr).max(1.0));
        let amp_down =
            crate::dsp::ramps::one_pole_coeff(1.0 / (p::AMP_RELEASE_MS * 1e-3 * sr).max(1.0));

        // The waveform, if a letter changed it. `prepare` on the osc is
        // green-zone work in the docs' sense — it sets a table pointer
        // and a level, not an allocation — and both table sets already
        // exist.
        let want_square = self.params.square();
        if want_square != self.osc_wave {
            self.osc_wave = want_square;
            self.osc.prepare(
                sr,
                if want_square {
                    Waveform::Square
                } else {
                    Waveform::Saw
                },
            );
        }
        let tables: &[f32] = if want_square { &self.square } else { &self.saw };

        self.shaper.configure(
            ShapeMode::SoftClip,
            1.0 + self.params.drive.clamp(0.0, 1.0) * (DRIVE_MAX - 1.0),
            0.0,
            1.0,
        );

        let mut at = 0usize;
        while at < out.len() {
            let k = CHUNK.min(out.len() - at);

            // --- the glide ---------------------------------------------
            if self.sliding {
                self.pitch += (self.target_pitch - self.pitch) * glide;
                if (self.target_pitch - self.pitch).abs() < 1e-3 {
                    self.pitch = self.target_pitch;
                    self.sliding = false;
                }
            }
            let hz = 440.0 * ((self.pitch + self.params.tune_st - 69.0) / 12.0).exp2();
            self.osc.set_freq(hz.clamp(1.0, sr * 0.45));

            // --- the filter, opened by the envelope --------------------
            //
            // The accent reaches THREE destinations from one knob: it
            // opens the filter further, rings it harder, and lifts the
            // level. That is what the original's accent circuit does, and
            // splitting it into three controls would be three ways to get
            // it wrong.
            let env_amount = (self.params.env_mod + self.accent * 0.5).clamp(0.0, 1.5);
            let octaves = env_amount * self.env * p::ENV_OCTAVES;
            let cutoff = (self.params.cutoff_hz * octaves.exp2()).clamp(20.0, sr * 0.45);
            // Resonance as a true Q: the knob's top is where the filter
            // is on the edge of singing, which is where this instrument
            // is usually pointed.
            let res = (self.params.resonance + self.accent * 0.2).clamp(0.0, 1.0);
            let q = 0.7 + res * res * 12.0;
            self.svf.prepare(sr, cutoff, q);
            // The extra six decibels, a little above the resonant corner
            // so it adds slope without moving the peak.
            self.pole.prepare(sr, (cutoff * 1.5).clamp(20.0, sr * 0.45));

            // --- render ------------------------------------------------
            let chunk = &mut self.chunk[..k];
            self.osc.process(chunk, tables);
            // Both filters block-wise, per band rather than per sample —
            // `Cascade`'s choice and for its reason: one section's
            // coefficients and state stay in registers for the chunk.
            self.svf.process(chunk, SvfMode::Lowpass);
            self.pole.process_lowpass(chunk);

            // The ACCENT's share of the level only. The knob itself is
            // the node's gain ramp, exactly as the kick's is, so moving
            // it glides across the segment instead of stepping at its
            // edge.
            let level = 1.0 + self.accent * 0.6;
            for (i, sample) in chunk.iter_mut().enumerate() {
                let x = self.shaper.shape(*sample);
                // The amplifier: fast up while the gate is held, fast
                // down when it lets go. No knobs — see the module header.
                let target = if self.gate { 1.0 } else { 0.0 };
                let c = if target > self.amp { amp_up } else { amp_down };
                self.amp += (target - self.amp) * c;
                if let Some(slot) = out.get_mut(at + i) {
                    *slot += x * self.amp * level * gain;
                }
            }

            // The filter envelope falls whether or not the gate is up:
            // on this instrument it is a decay, not a sustain.
            self.env *= decay;
            at += k;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(params: AcidParams) -> AcidVoice {
        let mut v = AcidVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut AcidVoice, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    /// How bright a stretch of signal is, amplitude-independently: the
    /// RMS of its first difference over its own RMS, which is
    /// proportional to the spectral centroid.
    ///
    /// NOT the peak-normalised version this reached for first. A resonant
    /// filter closing changes a waveform's peak-to-RMS ratio as well as
    /// its brightness, so dividing by the peak measured both at once and
    /// reported the sweep going the wrong way.
    fn brightness(x: &[f32]) -> f32 {
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len().max(1) as f32).sqrt();
        let diff: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        rms(&diff) / rms(x).max(1e-12)
    }

    /// Silence until a note arrives, and silence again after it lets go.
    #[test]
    fn it_is_silent_until_it_is_played() {
        let mut v = voice(AcidParams::default());
        assert!(peak(&render(&mut v, 4_800)) < 1e-6, "it sounded unplayed");
        v.note_on(48, 100);
        assert!(peak(&render(&mut v, 4_800)) > 0.01, "a note made nothing");
        v.note_off(48);
        // Well past the amp release before measuring. The first draft
        // measured the window STARTING 100 ms after the gate closed,
        // where a 12 ms release still has 2e-4 left in it — the device
        // was right and the ruler was in the wrong place.
        let _ = render(&mut v, 24_000);
        assert!(
            peak(&render(&mut v, 4_800)) < 1e-6,
            "it kept sounding after the gate closed"
        );
    }

    /// `all_sound_off`'s job: a seek must leave nothing from before it
    /// sounding — the sequencing contract's second rule.
    #[test]
    fn reset_silences_it_immediately() {
        let mut v = voice(AcidParams::default());
        v.note_on(48, 120);
        let _ = render(&mut v, 480);
        v.reset();
        assert!(peak(&render(&mut v, 480)) < 1e-6, "reset left a tail");
        assert!(!v.active());
    }

    /// THE FIRST OF THE TWO GESTURES: a note arriving while the gate is
    /// up glides and does NOT retrigger, and one arriving on its own
    /// snaps and does.
    #[test]
    fn an_overlapping_note_slides_and_a_separate_one_does_not() {
        let mut v = voice(AcidParams::default());
        v.note_on(36, 90);
        let _ = render(&mut v, 480);
        let env_before = v.env;

        // Overlapping: the gate is still up.
        v.note_on(48, 90);
        assert!(v.sliding, "an overlapping note did not slide");
        assert!(
            v.pitch < 48.0,
            "the pitch jumped to {} instead of gliding",
            v.pitch
        );
        assert!(
            v.env <= env_before,
            "a slid note retriggered the envelope: {env_before} -> {}",
            v.env
        );

        // And it gets there.
        let _ = render(&mut v, 48_000);
        assert!(
            (v.pitch - 48.0).abs() < 0.05,
            "the glide stalled at {}",
            v.pitch
        );

        // Separate: gate down first.
        v.note_off(48);
        let _ = render(&mut v, 9_600);
        v.note_on(60, 90);
        assert!(!v.sliding, "a separate note slid");
        assert_eq!(v.pitch, 60.0, "a separate note did not snap");
        assert_eq!(v.env, 1.0, "a separate note did not retrigger");
    }

    /// THE SECOND: an accented note is louder AND brighter. One knob,
    /// three destinations — so a test that only checked the level would
    /// pass on a broken accent.
    #[test]
    fn an_accented_note_is_louder_and_brighter() {
        let quiet = {
            let mut v = voice(AcidParams::default());
            v.note_on(45, 60);
            render(&mut v, 4_800)
        };
        let loud = {
            let mut v = voice(AcidParams::default());
            v.note_on(45, 127);
            render(&mut v, 4_800)
        };
        assert!(
            peak(&loud) > peak(&quiet) * 1.1,
            "accent gained nothing: {:.4} against {:.4}",
            peak(&loud),
            peak(&quiet)
        );
        // Brighter: more energy in the difference between successive
        // samples, which is the cheapest honest proxy for high frequency.
        assert!(
            brightness(&loud) > brightness(&quiet) * 1.05,
            "accent did not open the filter: {:.4} against {:.4}",
            brightness(&loud),
            brightness(&quiet)
        );
    }

    /// The envelope opens the filter and then closes it again, which is
    /// the squelch. Measured as brightness falling over the note.
    #[test]
    fn the_envelope_opens_the_filter_and_closes_it() {
        let mut v = voice(AcidParams {
            decay_ms: 150.0,
            env_mod: 1.0,
            ..AcidParams::default()
        });
        v.note_on(45, 90);
        let early = render(&mut v, 2_400);
        let late = render(&mut v, 2_400);
        assert!(
            brightness(&early) > brightness(&late) * 1.2,
            "the filter did not close: {:.4} then {:.4}",
            brightness(&early),
            brightness(&late)
        );
    }

    /// Both waveforms make sound, and they are not the same sound.
    #[test]
    fn the_two_waveforms_differ() {
        let run = |wave: u32| {
            let mut v = voice(AcidParams {
                wave: wave as f32,
                // Open, so the difference survives the filter.
                cutoff_hz: 8_000.0,
                resonance: 0.1,
                env_mod: 0.0,
                ..AcidParams::default()
            });
            v.note_on(45, 90);
            render(&mut v, 4_800)
        };
        let saw = run(p::WAVE_SAW);
        let square = run(p::WAVE_SQUARE);
        assert!(peak(&saw) > 0.01 && peak(&square) > 0.01);
        let diff: f32 = saw
            .iter()
            .zip(square.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / saw.len() as f32;
        assert!(diff > 0.01, "the two shapes came out the same");
    }

    /// Rendering ADDS, as the trait's callers expect of a voice.
    #[test]
    fn render_adds_rather_than_writes() {
        let mut v = voice(AcidParams::default());
        v.note_on(48, 100);
        let mut out = vec![1.0f32; 480];
        v.render_add(&mut out, 1.0);
        assert!(
            out.iter().all(|s| *s != 0.0),
            "render cleared the buffer it was handed"
        );
    }

    /// Nothing it can be asked for makes it shout: every knob at every
    /// end, and the output stays in a range a mixer can use.
    #[test]
    fn no_setting_runs_away() {
        for res in [0.0f32, 1.0] {
            for drive in [0.0f32, 1.0] {
                for env_mod in [0.0f32, 1.0] {
                    let mut v = voice(AcidParams {
                        resonance: res,
                        drive,
                        env_mod,
                        level: 2.0,
                        ..AcidParams::default()
                    });
                    v.note_on(36, 127);
                    let out = render(&mut v, 24_000);
                    assert!(
                        out.iter().all(|s| s.is_finite()),
                        "res {res} drive {drive} env {env_mod} went non-finite"
                    );
                    assert!(
                        peak(&out) < 8.0,
                        "res {res} drive {drive} env {env_mod} peaked at {}",
                        peak(&out)
                    );
                }
            }
        }
    }

    /// Every table row reaches a field and comes back.
    #[test]
    fn every_row_round_trips() {
        let mut params = AcidParams::default();
        for row in p::TABLE {
            let mid = (row.min + row.max) * 0.5;
            params.set(row.id, mid);
            assert_eq!(
                params.get(row.id),
                Some(mid),
                "`{}` did not come back",
                row.name
            );
        }
    }

    /// A p-lock overrides for its note and `None` restores the LIVE base,
    /// not a snapshot — so a knob turned mid-playback is heard on every
    /// unlocked note.
    #[test]
    fn a_plock_overrides_and_then_restores_the_live_base() {
        let mut v = voice(AcidParams::default());
        v.set_param(p::CUTOFF, 500.0);
        v.plock(p::CUTOFF, Some(4_000.0));
        assert_eq!(v.params.cutoff_hz, 4_000.0);
        // The knob moves while the lock is in force.
        v.set_param(p::CUTOFF, 900.0);
        v.plock(p::CUTOFF, None);
        assert_eq!(
            v.params.cutoff_hz, 900.0,
            "the lock restored a snapshot instead of the live knob"
        );
    }
}
