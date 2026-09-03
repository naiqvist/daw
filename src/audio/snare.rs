//! The snare drum synth's voice — the instrument half of `Node::Snare`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when. Read `notes/20260823-dsp-kernel-contract.md` for what
//! the kernels promise.
//!
//! # The path, in order
//!
//! ```text
//! bend env ─▶ sine f0 ────┐
//!          ▶ sine f0·r ───┴─▶ shell env ─┐
//! white noise ─▶ BANDPASS ─▶ snap env ───┴─▶ SATURATOR ─▶ out
//! ```
//!
//! # Why two shells, and why the ratio is free
//!
//! A struck membrane's modes are NOT harmonic. The second circular mode
//! of an ideal drumhead sits at 1.59 times the first, and a real snare
//! with wires and a rim lands somewhere near 1.8. That inharmonicity is
//! the whole reason a snare reads as a drum rather than as a note: two
//! sines an octave apart sum to a pitch, and two sines 1.83 apart sum to
//! a thump with a beat in it. So the ratio is a free knob rather than a
//! harmonic count — the useful settings are the ones between the
//! harmonics.
//!
//! # Why the noise has its OWN decay
//!
//! The wires rattle on after the head has stopped. That is not a detail;
//! it is the difference between a snare and a tom with a hiss on the
//! front. The default snap decay is longer than the default shell decay
//! for exactly this reason, and the two envelopes are independent so the
//! relationship can be inverted deliberately rather than by accident.
//!
//! # Why the envelopes are exponential
//!
//! [`ExpDecay`], not [`Adsr`](crate::dsp::adsr::Adsr). A drum's VCA is a
//! capacitor discharging; a linear ramp holds its level through the
//! middle of the sound and then stops, which is heard as a gate rather
//! than a decay. The kick predates the kernel and still runs linear;
//! everything built after it does not.
//!
//! # Two rates, on purpose
//!
//! Per SAMPLE, where modulation is a multiply: the two amplitude
//! envelopes. Per CONTROL CHUNK, where it costs a transcendental: the
//! oscillator frequencies under the bend. The same split `kick.rs` and
//! `poly.rs` make, for the same reason.

use crate::dsp::adsr::ExpDecay;
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::snare as sp;

/// How often the bent pitch is rebuilt, in samples. 32 at 48 kHz is
/// 0.67 ms — a glide rather than a staircase even at the shortest bend
/// the table allows.
pub const CHUNK: usize = 32;

/// A snare's settings, in engine units. Parallel to
/// [`params::snare::TABLE`](crate::params::snare::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SnareParams {
    pub tune_hz: f32,
    pub ratio: f32,
    pub tone_decay_ms: f32,
    pub bend: f32,
    pub bend_ms: f32,
    pub snap: f32,
    pub snap_decay_ms: f32,
    pub noise_tone_hz: f32,
    pub noise_q: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for SnareParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new snare sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(sp::TABLE, id).default;
        Self {
            tune_hz: at(sp::TUNE),
            ratio: at(sp::RATIO),
            tone_decay_ms: at(sp::TONE_DECAY),
            bend: at(sp::BEND),
            bend_ms: at(sp::BEND_TIME),
            snap: at(sp::SNAP),
            snap_decay_ms: at(sp::SNAP_DECAY),
            noise_tone_hz: at(sp::NOISE_TONE),
            noise_q: at(sp::NOISE_Q),
            drive: at(sp::DRIVE),
            gain: at(sp::GAIN),
        }
    }
}

impl SnareParams {
    /// Write one parameter by table id. Clamped through the table, so a
    /// letter, a project file and a knob all land in the same range.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(sp::TABLE, param, value) else {
            return;
        };
        match param {
            sp::TUNE => self.tune_hz = value,
            sp::RATIO => self.ratio = value,
            sp::TONE_DECAY => self.tone_decay_ms = value,
            sp::BEND => self.bend = value,
            sp::BEND_TIME => self.bend_ms = value,
            sp::SNAP => self.snap = value,
            sp::SNAP_DECAY => self.snap_decay_ms = value,
            sp::NOISE_TONE => self.noise_tone_hz = value,
            sp::NOISE_Q => self.noise_q = value,
            sp::DRIVE => self.drive = value,
            sp::GAIN => self.gain = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            sp::TUNE => self.tune_hz,
            sp::RATIO => self.ratio,
            sp::TONE_DECAY => self.tone_decay_ms,
            sp::BEND => self.bend,
            sp::BEND_TIME => self.bend_ms,
            sp::SNAP => self.snap,
            sp::SNAP_DECAY => self.snap_decay_ms,
            sp::NOISE_TONE => self.noise_tone_hz,
            sp::NOISE_Q => self.noise_q,
            sp::DRIVE => self.drive,
            sp::GAIN => self.gain,
            _ => 0.0,
        }
    }
}

/// A note's own pitch as a multiplier on the tuned fundamental.
///
/// MIDI 38 — GM's acoustic snare — is unity, so the `tune` knob means
/// what it says and a sequence written on the General MIDI drum map never
/// transposes. The kick uses 36 for the same reason; each drum's unity is
/// the note its own map puts it on.
fn pitch_scale(pitch: u8) -> f32 {
    const ROOT: f32 = 38.0;
    ((f32::from(pitch) - ROOT) / 12.0).exp2()
}

/// The snare's single voice.
///
/// ONE voice, for the reason the kick has one: a drum retriggered before
/// it has finished should cut its own tail rather than layer on it. Two
/// snares a few milliseconds apart sum to a flam with 6 dB of extra peak,
/// which is a thing you ask for by writing two notes, not a thing a voice
/// should do behind your back.
pub struct SnareVoice {
    sample_rate: f32,
    params: SnareParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch, which a lock may hold
    /// elsewhere for one hit.
    base: SnareParams,

    /// The two shell modes. One table set between them — the tables are
    /// read-only once built, so sharing costs nothing and allocates once.
    shell_a: MipOsc,
    shell_b: MipOsc,
    tables: Vec<f32>,
    noise: WhiteNoise,
    band: Svf,

    shell_env: ExpDecay,
    bend_env: ExpDecay,
    snap_env: ExpDecay,

    shaper: Waveshaper,

    /// The note's transposition, held from note-on so the chunk loop can
    /// rebuild the frequencies without re-reading the note.
    note_scale: f32,
    velocity: f32,

    /// What the bandpass is currently tuned to, so a block only pays for
    /// a re-tune when something actually moved.
    tuned_hz: f32,
    tuned_q: f32,

    /// How many samples are left in the current control chunk.
    ///
    /// The chunk boundary belongs to the VOICE'S timeline, not to
    /// whatever block length the caller happens to use — otherwise 256
    /// would stop equalling 100 + 156, which is the property the
    /// segmented transport depends on.
    chunk_left: usize,

    /// Scratch for one control chunk. Sized at compile time; nothing here
    /// allocates while running.
    env_scratch: [f32; CHUNK],
    noise_scratch: [f32; CHUNK],
}

impl Default for SnareVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl SnareVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: SnareParams::default(),
            base: SnareParams::default(),
            shell_a: MipOsc::new(),
            shell_b: MipOsc::new(),
            tables: Vec::new(),
            noise: WhiteNoise::new(),
            band: Svf::new(),
            shell_env: ExpDecay::new(),
            bend_env: ExpDecay::new(),
            snap_env: ExpDecay::new(),
            shaper: Waveshaper::new(),
            note_scale: 1.0,
            velocity: 1.0,
            tuned_hz: 0.0,
            tuned_q: 0.0,
            chunk_left: 0,
            env_scratch: [0.0; CHUNK],
            noise_scratch: [0.0; CHUNK],
        }
    }

    /// Green zone: build the sine tables and settle every kernel.
    ///
    /// The ONE allocation in this file, and it happens at compile rather
    /// than while running — `build_tables` writes into storage this
    /// struct owns, which is the kernel contract's rule for anything that
    /// needs memory.
    pub fn prepare(&mut self, sample_rate: f32, params: SnareParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.tables);
        self.shell_a.prepare(self.sample_rate, Waveform::Sine);
        self.shell_b.prepare(self.sample_rate, Waveform::Sine);
        self.noise.seed(0x51ab_2e00_dead_beef);
        self.params = params;
        self.apply_envelopes();
        self.retune(true);
        self.reset();
    }

    /// Envelope timings, rebuilt from the params. All three are one-shots:
    /// a snare has no sustain stage to be in.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        self.shell_env.prepare(fs, self.params.tone_decay_ms);
        self.bend_env.prepare(fs, self.params.bend_ms);
        self.snap_env.prepare(fs, self.params.snap_decay_ms);
    }

    /// The noise bandpass, re-tuned only when something moved.
    ///
    /// [`Mode::BandpassUnity`] rather than the raw bandpass: raising the
    /// width knob must narrow the band without also making it louder, or
    /// "width" is secretly a second level control.
    fn retune(&mut self, force: bool) {
        let hz = self
            .params
            .noise_tone_hz
            .clamp(20.0, self.sample_rate * 0.45);
        let q = self.params.noise_q;
        let moved = force
            || (hz - self.tuned_hz).abs() > self.tuned_hz.max(1.0) * 1e-4
            || (q - self.tuned_q).abs() > 1e-4;
        if !moved {
            return;
        }
        self.tuned_hz = hz;
        self.tuned_q = q;
        self.band.prepare(self.sample_rate, hz, q);
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.shell_a.reset();
        self.shell_b.reset();
        self.noise.reset();
        self.band.reset();
        self.shell_env.reset();
        self.bend_env.reset();
        self.snap_env.reset();
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
            sp::TONE_DECAY | sp::BEND_TIME | sp::SNAP_DECAY => self.apply_envelopes(),
            sp::NOISE_TONE | sp::NOISE_Q => self.retune(false),
            _ => {}
        }
    }

    pub fn params(&self) -> SnareParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.shell_env.active() || self.snap_env.active()
    }

    /// Strike. Every envelope restarts from silence together.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.note_scale = pitch_scale(pitch);
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        // Both shells restart at a known phase so every hit has the same
        // attack. Free-running phases make the first half-cycle a
        // different shape each time — heard as an inconsistent click and
        // measured as a different peak.
        self.shell_a.reset();
        self.shell_b.reset();
        // A strike starts a fresh control chunk, so the bent pitch is
        // rebuilt on the note's very first sample rather than up to
        // CHUNK - 1 samples later.
        self.chunk_left = 0;
        self.shell_env.trigger(1.0);
        self.bend_env.trigger(1.0);
        self.snap_env.trigger(1.0);
        self.retune(false);
    }

    /// Red zone: render into `out`, ADDING to what is there.
    ///
    /// Additive because the sequencer sums voices into one buffer; a
    /// write would silence whatever else landed in this block.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.tables.is_empty() {
            return;
        }
        let mut written = 0usize;
        while written < out.len() {
            // A run stops at the CHUNK boundary or at the end of the
            // caller's block, whichever comes first — so the boundaries
            // land in the same places however the block was split.
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
        let base = self.params.tune_hz * self.note_scale * (bend / 12.0).exp2();
        let ceiling = self.sample_rate * 0.45;
        self.shell_a.set_freq(base.clamp(1.0, ceiling));
        // The second mode rides the SAME bend, because it is the same
        // drumhead. Bending only the fundamental would slide the ratio
        // during the attack, which is heard as a chirp rather than a drum.
        self.shell_b
            .set_freq((base * self.params.ratio).clamp(1.0, ceiling));
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

        // --- the shell: two inharmonic sines under one envelope --------
        //
        // Half each, so the pair peaks at the same place one at full
        // level would — the ratio knob changes the timbre, never the
        // headroom.
        self.shell_a.process(out, &self.tables);
        self.shell_b.process(noise, &self.tables);
        for (sample, other) in out.iter_mut().zip(noise.iter()) {
            *sample = (*sample + *other) * 0.5;
        }
        self.shell_env.process(env);
        let velocity = self.velocity;
        for (sample, level) in out.iter_mut().zip(env.iter()) {
            *sample *= *level * velocity;
        }

        // --- the wires: band-passed noise under its own envelope -------
        if self.params.snap > 0.0 {
            self.noise.process(noise);
            self.band.process(noise, Mode::BandpassUnity);
            self.snap_env.process(env);
            let level = self.params.snap * velocity;
            for (sample, (rattle, decay)) in out.iter_mut().zip(noise.iter().zip(env.iter())) {
                *sample += *rattle * *decay * level;
            }
        } else {
            // Advanced whether or not anything reads it, so the snap's
            // timing never depends on whether it is switched on.
            self.snap_env.process(env);
        }
        self.bend_env.process(env);

        // --- shaping ---------------------------------------------------
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

    fn voice(params: SnareParams) -> SnareVoice {
        let mut v = SnareVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut SnareVoice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn a_fresh_snare_is_silent_until_it_is_struck() {
        let mut v = voice(SnareParams::default());
        let quiet = render(&mut v, 512);
        assert!(quiet.iter().all(|s| *s == 0.0), "silent before the strike");
        assert!(!v.active());

        v.trigger(38, 100);
        assert!(v.active());
        let hit = render(&mut v, 512);
        assert!(peak(&hit) > 0.05, "the strike made sound: {}", peak(&hit));
    }

    /// THE WIRES OUTLAST THE HEAD. That relationship, not the presence of
    /// noise, is what makes this a snare rather than a tom with a hiss on
    /// the front — so it is the thing worth pinning.
    #[test]
    fn the_snap_rings_on_after_the_shell_has_stopped() {
        let params = SnareParams {
            tone_decay_ms: 60.0,
            snap_decay_ms: 400.0,
            snap: 0.8,
            drive: 1.0,
            ..SnareParams::default()
        };

        // The shell alone, and the wires alone, measured over a late
        // window where only one of them should still be moving.
        let late = 48_000 / 4; // 250 ms in
        let window = 2_400usize;

        let mut shell_only = voice(SnareParams {
            snap: 0.0,
            ..params
        });
        shell_only.trigger(38, 127);
        let shell = render(&mut shell_only, late + window);

        let mut both = voice(params);
        both.trigger(38, 127);
        let full = render(&mut both, late + window);

        let shell_late = rms(&shell[late..]);
        let full_late = rms(&full[late..]);
        assert!(
            shell_late < 1e-4,
            "the shell should be long gone at 250 ms: {shell_late}"
        );
        assert!(
            full_late > shell_late * 50.0 && full_late > 1e-3,
            "the wires should still be rattling at 250 ms: {full_late}"
        );
    }

    /// THE SECOND SHELL IS INHARMONIC AND IT MOVES WITH THE RATIO KNOB. A
    /// ratio that did nothing would leave a single sine calling itself a
    /// drum.
    #[test]
    fn the_ratio_knob_moves_the_second_shell() {
        let base = SnareParams {
            snap: 0.0,
            bend: 0.0,
            drive: 1.0,
            tone_decay_ms: 800.0,
            ..SnareParams::default()
        };
        let take = |ratio: f32| {
            let mut v = voice(SnareParams { ratio, ..base });
            v.trigger(38, 127);
            let buf = render(&mut v, 4_096);
            // Zero crossings stand in for spectral content: two sines an
            // octave apart cross more often than two in unison.
            buf.windows(2)
                .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
                .count()
        };
        let unison = take(1.0);
        let wide = take(3.0);
        assert!(
            wide > unison,
            "a wider ratio must add upper content: {wide} against {unison}"
        );
    }

    /// A RETRIGGER REPLACES THE HIT. Two snares a few milliseconds apart
    /// must not sum into one with 6 dB of extra peak.
    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let params = SnareParams {
            tone_decay_ms: 800.0,
            snap_decay_ms: 800.0,
            drive: 1.0,
            ..SnareParams::default()
        };
        let mut once = voice(params);
        once.trigger(38, 127);
        let single = peak(&render(&mut once, 4_800));

        let mut twice = voice(params);
        twice.trigger(38, 127);
        let _ = render(&mut twice, 240);
        twice.trigger(38, 127);
        let doubled = peak(&render(&mut twice, 4_800));

        assert!(
            doubled <= single * 1.05,
            "the retrigger stacked: {doubled} against {single}"
        );
    }

    /// Split-block equivalence — the property the segmented transport
    /// depends on. Chunked internally at 32, so a 100/156 split crosses
    /// chunk boundaries in a different place than a 256 does.
    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = SnareParams::default();
        let mut whole = voice(params);
        whole.trigger(38, 100);
        let mut a = vec![0.0f32; 256];
        whole.render_add(&mut a, 1.0);

        let mut split = voice(params);
        split.trigger(38, 100);
        let mut b = vec![0.0f32; 256];
        split.render_add(&mut b[..100], 1.0);
        split.render_add(&mut b[100..], 1.0);

        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "256 must equal 100 + 156"
        );
    }

    /// The render path allocates nothing, at every setting that matters.
    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(SnareParams {
            snap: 1.0,
            drive: 8.0,
            noise_q: 8.0,
            ..SnareParams::default()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(38, 100);
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    /// Nonsense in, finite out — at every extreme the table allows.
    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in sp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = SnareParams::default();
                params.set(def.id, value);
                let mut v = voice(params);
                for pitch in [0u8, 38, 127] {
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

        // And a voice never prepared renders silence rather than reading
        // tables that are not there.
        let mut bare = SnareVoice::new();
        bare.trigger(38, 127);
        let mut out = vec![0.0f32; 128];
        bare.render_add(&mut out, 1.0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Every table id round-trips through the params, and an unknown id
    /// is ignored rather than landing on whichever field is first.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = SnareParams::default();
        for def in sp::TABLE {
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

        params.set(sp::TUNE, 1e9);
        assert!((params.tune_hz - sp::TUNE_MAX).abs() < 1e-3);
        params.set(sp::TUNE, -1e9);
        assert!((params.tune_hz - sp::TUNE_MIN).abs() < 1e-3);
    }

    /// GM's acoustic snare is the tuned note, so the `tune` knob means
    /// what it says on a General MIDI drum map.
    #[test]
    fn the_general_midi_snare_is_the_tuned_note() {
        assert!((pitch_scale(38) - 1.0).abs() < 1e-6);
        assert!((pitch_scale(50) - 2.0).abs() < 1e-5);
        assert!((pitch_scale(26) - 0.5).abs() < 1e-5);
    }
}
