//! The kick drum synth's voice — the instrument half of `Node::Kick`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when. Read `notes/20260823-dsp-kernel-contract.md` for what
//! the kernels promise and `notes/20260825-synth-brief.md` for the ease
//! mechanics the card follows.
//!
//! # The path, in order
//!
//! ```text
//! pitch env A (fast) ─┐
//! pitch env B (slow) ─┴─▶ sine ──▶ amp env ─┐
//! white noise ── click env ─────────────────┴─▶ DISPERSER ─▶ SATURATOR ─▶ out
//! ```
//!
//! # Why two pitch envelopes
//!
//! A kick needs two drops at once and they are not the same drop. The
//! fast one — single-figure milliseconds — is heard as the beater
//! striking the skin; it is a transient, not a pitch. The slow one — a
//! few dozen — is heard as the body settling onto its tuned note. One
//! envelope can be either but never both: set it fast and the body has no
//! weight, set it slow and the attack turns into a woolly swoop.
//!
//! They ADD in semitones before the exponential, so their depths are
//! musical and independent rather than one scaling the other.
//!
//! # Why the disperser sits where it does
//!
//! After the sum and before the saturator. After, because dispersing a
//! finished drum is what turns its transient into a pitched sweep —
//! dispersing the parts separately would smear each one into the others.
//! Before the saturator, because a disperser RAISES the peak (it spreads
//! energy in time and the crest factor goes up), and the saturator is
//! what puts that back under control. Ordering them the other way round
//! gives a clean sweep and an uncontrolled peak, which is the wrong half
//! of both effects.
//!
//! # Two rates, on purpose
//!
//! Per SAMPLE, where modulation is a multiply: the amp envelope, the
//! click envelope, the mix. Per CONTROL CHUNK, where it is a coefficient
//! that costs a transcendental to rebuild: the oscillator frequency and
//! the disperser's tuning. The same split `poly.rs` makes, for the same
//! reason — a 36-semitone drop in six milliseconds is 288 samples, so a
//! per-block update would render it in one step and a per-chunk one
//! renders it in nine.

use crate::dsp::adsr::Adsr;
use crate::dsp::filters::Disperser;
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{Mode, Waveshaper};
use crate::params::kick as kp;

/// How often the pitch and the disperser's tuning are rebuilt, in
/// samples. 32 at 48 kHz is 0.67 ms — about nine steps across the
/// fastest pitch drop the table allows, which is a glide rather than a
/// staircase.
pub const CHUNK: usize = 32;

/// A kick's settings, in engine units. Parallel to
/// [`params::kick::TABLE`](crate::params::kick::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KickParams {
    pub tune_hz: f32,
    pub amp_decay_ms: f32,
    pub punch_depth: f32,
    pub punch_ms: f32,
    pub sweep_depth: f32,
    pub sweep_ms: f32,
    pub click_level: f32,
    pub click_ms: f32,
    pub disperse_stages: f32,
    pub disperse_harmonic: f32,
    pub disperse_q: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for KickParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new kick sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(kp::TABLE, id).default;
        Self {
            tune_hz: at(kp::TUNE),
            amp_decay_ms: at(kp::AMP_DECAY),
            punch_depth: at(kp::PITCH_A_DEPTH),
            punch_ms: at(kp::PITCH_A_DECAY),
            sweep_depth: at(kp::PITCH_B_DEPTH),
            sweep_ms: at(kp::PITCH_B_DECAY),
            click_level: at(kp::CLICK_LEVEL),
            click_ms: at(kp::CLICK_DECAY),
            disperse_stages: at(kp::DISP_STAGES),
            disperse_harmonic: at(kp::DISP_HARMONIC),
            disperse_q: at(kp::DISP_Q),
            drive: at(kp::DRIVE),
            gain: at(kp::GAIN),
        }
    }
}

impl KickParams {
    /// Write one parameter by table id. Clamped through the table, so a
    /// letter, a project file and a knob all land in the same range.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(kp::TABLE, param, value) else {
            return;
        };
        match param {
            kp::TUNE => self.tune_hz = value,
            kp::AMP_DECAY => self.amp_decay_ms = value,
            kp::PITCH_A_DEPTH => self.punch_depth = value,
            kp::PITCH_A_DECAY => self.punch_ms = value,
            kp::PITCH_B_DEPTH => self.sweep_depth = value,
            kp::PITCH_B_DECAY => self.sweep_ms = value,
            kp::CLICK_LEVEL => self.click_level = value,
            kp::CLICK_DECAY => self.click_ms = value,
            kp::DISP_STAGES => self.disperse_stages = value,
            kp::DISP_HARMONIC => self.disperse_harmonic = value,
            kp::DISP_Q => self.disperse_q = value,
            kp::DRIVE => self.drive = value,
            kp::GAIN => self.gain = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            kp::TUNE => self.tune_hz,
            kp::AMP_DECAY => self.amp_decay_ms,
            kp::PITCH_A_DEPTH => self.punch_depth,
            kp::PITCH_A_DECAY => self.punch_ms,
            kp::PITCH_B_DEPTH => self.sweep_depth,
            kp::PITCH_B_DECAY => self.sweep_ms,
            kp::CLICK_LEVEL => self.click_level,
            kp::CLICK_DECAY => self.click_ms,
            kp::DISP_STAGES => self.disperse_stages,
            kp::DISP_HARMONIC => self.disperse_harmonic,
            kp::DISP_Q => self.disperse_q,
            kp::DRIVE => self.drive,
            kp::GAIN => self.gain,
            _ => 0.0,
        }
    }
}

/// A note's own pitch as a multiplier on the tuned fundamental.
///
/// MIDI 36 — the C most drum machines put their kick on — is unity, so
/// the `tune` knob means what it says and a sequence that only ever plays
/// C1 never transposes. Playing higher notes transposes the whole drum,
/// which is how a tuned kick line is written.
fn pitch_scale(pitch: u8) -> f32 {
    const ROOT: f32 = 36.0;
    ((f32::from(pitch) - ROOT) / 12.0).exp2()
}

/// The kick's single voice.
///
/// ONE voice, and that is a decision rather than a limitation: a kick
/// retriggered before it has finished should cut its own tail, not layer
/// on top of it. Two kicks a few milliseconds apart sum to a kick with a
/// flam and 6 dB of extra peak, which is never what was wanted.
pub struct KickVoice {
    sample_rate: f32,
    params: KickParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch, which a lock may hold
    /// elsewhere for one hit.
    base: KickParams,

    osc: MipOsc,
    /// The sine's band-limited tables. Built once at prepare — green
    /// zone — and read-only afterwards.
    tables: Vec<f32>,
    noise: WhiteNoise,

    amp: Adsr,
    punch: Adsr,
    sweep: Adsr,
    click: Adsr,

    disperser: Disperser,
    shaper: Waveshaper,

    /// The note's transposition, held from note-on so the chunk loop can
    /// rebuild the frequency without re-reading the note.
    note_scale: f32,
    velocity: f32,

    /// What the disperser is currently tuned to, so a chunk only pays for
    /// a re-tune when something actually moved.
    tuned_hz: f32,
    tuned_q: f32,
    tuned_stages: u32,

    /// How many samples are left in the current control chunk.
    ///
    /// The chunk boundary belongs to the VOICE'S timeline, not to
    /// whatever block length the caller happens to use. Restarting the
    /// count at every `render_add` would put the coefficient updates in
    /// different places depending on how the transport split the block —
    /// and 256 would stop equalling 100 + 156, which is the property the
    /// segmented transport depends on.
    chunk_left: usize,

    /// Scratch for one control chunk of envelope values. Sized at compile
    /// time; nothing here allocates while running.
    env_scratch: [f32; CHUNK],
}

impl Default for KickVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl KickVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: KickParams::default(),
            base: KickParams::default(),
            osc: MipOsc::new(),
            tables: Vec::new(),
            noise: WhiteNoise::new(),
            amp: Adsr::new(),
            punch: Adsr::new(),
            sweep: Adsr::new(),
            click: Adsr::new(),
            disperser: Disperser::new(),
            shaper: Waveshaper::new(),
            note_scale: 1.0,
            velocity: 1.0,
            tuned_hz: 0.0,
            tuned_q: 0.0,
            tuned_stages: 0,
            chunk_left: 0,
            env_scratch: [0.0; CHUNK],
        }
    }

    /// Green zone: build the sine tables and settle every kernel.
    ///
    /// The ONE allocation in this file, and it happens at compile rather
    /// than while running — `build_tables` writes into storage the caller
    /// owns, which is the kernel contract's rule for anything that needs
    /// memory.
    pub fn prepare(&mut self, sample_rate: f32, params: KickParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.tables);
        self.osc.prepare(self.sample_rate, Waveform::Sine);
        self.noise.seed(0x5eed_c0de_1234_5678);
        self.params = params;
        self.apply_envelopes();
        self.retune(true);
        self.reset();
    }

    /// Envelope timings, rebuilt from the params.
    ///
    /// All four are one-shots: attack 0, sustain 0, so `gate_on` is the
    /// whole gesture and the decay is the sound. A kick has no sustain
    /// stage to be in.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        self.amp
            .prepare(fs, 0.0, self.params.amp_decay_ms, 0.0, 0.0);
        self.punch.prepare(fs, 0.0, self.params.punch_ms, 0.0, 0.0);
        self.sweep.prepare(fs, 0.0, self.params.sweep_ms, 0.0, 0.0);
        self.click.prepare(fs, 0.0, self.params.click_ms, 0.0, 0.0);
    }

    /// The disperser's corner: a HARMONIC of the note actually sounding.
    ///
    /// Re-tuned only when something moved, the way the Filter node guards
    /// its cascade. The kernel's own prepare costs one transcendental
    /// whatever the stage count, so this is cheap enough to sit on the
    /// audio thread — but paying it on every chunk for a number that has
    /// not changed would still be waste.
    fn retune(&mut self, force: bool) {
        let base = self.params.tune_hz * self.note_scale;
        let hz =
            (base * self.params.disperse_harmonic.max(1.0)).clamp(20.0, self.sample_rate * 0.45);
        let q = self.params.disperse_q;
        let stages = self.params.disperse_stages.round().max(0.0) as u32;
        let moved = force
            || (hz - self.tuned_hz).abs() > self.tuned_hz.max(1.0) * 1e-4
            || (q - self.tuned_q).abs() > 1e-4
            || stages != self.tuned_stages;
        if !moved {
            return;
        }
        self.tuned_hz = hz;
        self.tuned_q = q;
        self.tuned_stages = stages;
        self.disperser.prepare(self.sample_rate, hz, q, stages);
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.osc.reset();
        self.noise.reset();
        self.amp.reset();
        self.punch.reset();
        self.sweep.reset();
        self.click.reset();
        self.disperser.reset();
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

    fn apply_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            kp::AMP_DECAY | kp::PITCH_A_DECAY | kp::PITCH_B_DECAY | kp::CLICK_DECAY => {
                self.apply_envelopes();
            }
            _ => {}
        }
    }

    pub fn params(&self) -> KickParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.amp.active() || self.click.active()
    }

    /// Strike. Every envelope restarts from silence together.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.note_scale = pitch_scale(pitch);
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        // The oscillator restarts at a known phase so every kick has the
        // same attack. A free-running phase makes the first half-cycle a
        // different shape each time, which is heard as an inconsistent
        // click and measured as a different peak.
        self.osc.reset();
        // A strike starts a fresh control chunk, so the pitch is rebuilt
        // on the note's very first sample rather than up to CHUNK - 1
        // samples later — which at a 6 ms punch would be a tenth of the
        // whole drop rendered at the wrong frequency.
        self.chunk_left = 0;
        self.amp.gate_on();
        self.punch.gate_on();
        self.sweep.gate_on();
        self.click.gate_on();
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
        // --- pitch: two envelopes, added in SEMITONES ------------------
        //
        // Added before the exponential so the two depths are independent
        // musical amounts. Multiplying the ratios instead would make the
        // slow envelope scale the fast one, and turning the body sweep up
        // would quietly deepen the punch as well.
        let punch = self.punch.current() * self.params.punch_depth;
        let sweep = self.sweep.current() * self.params.sweep_depth;
        let hz = self.params.tune_hz * self.note_scale * ((punch + sweep) / 12.0).exp2();
        self.osc.set_freq(hz.clamp(1.0, self.sample_rate * 0.45));
    }

    /// One run of samples inside a control chunk: per-sample work only.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let Some(scratch) = self.env_scratch.get_mut(..n) else {
            return;
        };

        // --- the body: sine × amp envelope ----------------------------
        self.osc.process(out, &self.tables);
        self.amp.process(scratch);
        let velocity = self.velocity;
        for (sample, env) in out.iter_mut().zip(scratch.iter()) {
            *sample *= *env * velocity;
        }

        // --- the click: white noise × its own very short envelope -----
        //
        // Its OWN envelope, not a scaled copy of the amp one: the click
        // is over in a couple of milliseconds while the body rings for a
        // few hundred, and that ratio is the difference between a kick
        // and a tom.
        if self.params.click_level > 0.0 {
            self.click.process(scratch);
            let level = self.params.click_level * velocity;
            for (sample, env) in out.iter_mut().zip(scratch.iter()) {
                let mut noise = [0.0f32; 1];
                self.noise.process(&mut noise);
                *sample += noise[0] * *env * level;
            }
        }
        // The pitch envelopes are advanced whether or not anything reads
        // them this chunk, so their timing never depends on which parts
        // are switched on.
        self.punch.process(scratch);
        self.sweep.process(scratch);

        // --- shaping --------------------------------------------------
        self.disperser.process(out);
        self.shaper
            .configure(Mode::SoftClip, self.params.drive, 0.0, 1.0);
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

    fn voice(params: KickParams) -> KickVoice {
        let mut v = KickVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut KickVoice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    /// The zero-crossing rate over a window, as a stand-in for pitch:
    /// counting crossings is exact for a sine and needs no FFT.
    fn crossings(buf: &[f32]) -> usize {
        buf.windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    }

    #[test]
    fn a_fresh_kick_is_silent_until_it_is_struck() {
        let mut v = voice(KickParams::default());
        let quiet = render(&mut v, 512);
        assert!(quiet.iter().all(|s| *s == 0.0), "silent before the strike");
        assert!(!v.active());

        v.trigger(36, 100);
        assert!(v.active());
        let hit = render(&mut v, 512);
        assert!(peak(&hit) > 0.05, "the strike made sound: {}", peak(&hit));
    }

    /// BOTH PITCH ENVELOPES DO SOMETHING, and they do different things.
    ///
    /// The whole reason there are two is that one cannot be fast and slow
    /// at once. So the fast one must change the very beginning and be
    /// gone by the middle, and the slow one must still be bending the
    /// pitch where the fast one has finished.
    #[test]
    fn the_two_pitch_envelopes_act_on_different_parts_of_the_hit() {
        let base = KickParams {
            punch_depth: 0.0,
            sweep_depth: 0.0,
            click_level: 0.0,
            disperse_stages: 0.0,
            drive: 1.0,
            amp_decay_ms: 2_000.0,
            ..KickParams::default()
        };
        let window = 480usize; // 10 ms at 48 kHz
        let take = |params: KickParams| {
            let mut v = voice(params);
            v.trigger(36, 127);
            let buf = render(&mut v, window * 8);
            let early = crossings(&buf[..window]);
            let late = crossings(&buf[window * 5..window * 6]);
            (early, late)
        };

        let (flat_early, flat_late) = take(base);

        // The FAST envelope: 6 ms, so it raises the early pitch and has
        // finished long before the late window.
        let (punch_early, punch_late) = take(KickParams {
            punch_depth: 36.0,
            punch_ms: 6.0,
            ..base
        });
        assert!(
            punch_early > flat_early * 2,
            "the punch envelope must lift the start: {punch_early} vs {flat_early}"
        );
        assert!(
            punch_late <= flat_late + 2,
            "and be finished by 50 ms: {punch_late} vs {flat_late}"
        );

        // The SLOW envelope: 200 ms, so it is still bending at 50 ms,
        // where the fast one is long gone.
        let (_, sweep_late) = take(KickParams {
            sweep_depth: 24.0,
            sweep_ms: 200.0,
            ..base
        });
        // At 50 ms into a 200 ms decay the envelope is still around
        // three quarters up, so 24 semitones of depth is still lifting
        // 50 Hz to about 140 — nearly three times the crossings of the
        // flat drum in the same window.
        assert!(
            sweep_late >= flat_late * 2,
            "the sweep envelope must still be bending at 50 ms: {sweep_late} vs {flat_late}"
        );
    }

    /// THE CLICK IS SHORT. Its envelope is its own, so it is over while
    /// the body is still ringing — the difference between a kick and a
    /// tom.
    #[test]
    fn the_click_is_over_long_before_the_body() {
        let params = KickParams {
            click_level: 1.0,
            click_ms: 2.0,
            amp_decay_ms: 1_000.0,
            // Nothing else, so what is measured is the noise alone.
            punch_depth: 0.0,
            sweep_depth: 0.0,
            disperse_stages: 0.0,
            drive: 1.0,
            ..KickParams::default()
        };
        let mut with = voice(params);
        with.trigger(36, 127);
        let noisy = render(&mut with, 4_800);

        let mut without = voice(KickParams {
            click_level: 0.0,
            ..params
        });
        without.trigger(36, 127);
        let clean = render(&mut without, 4_800);

        // Early on the two differ; 100 ms later they do not.
        let early: f32 = noisy[..96]
            .iter()
            .zip(&clean[..96])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        let late: f32 = noisy[4_320..]
            .iter()
            .zip(&clean[4_320..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(early > 0.02, "the click is audible at the start: {early}");
        assert!(late < early * 0.02, "and gone by 90 ms: {late} vs {early}");
    }

    /// The disperser is TUNED TO THE NOTE. Playing an octave up must move
    /// its corner an octave up too, or it stops being attached to the drum
    /// the moment anyone writes a tuned kick line.
    #[test]
    fn the_disperser_follows_the_note_and_its_harmonic() {
        let params = KickParams {
            disperse_stages: 8.0,
            disperse_harmonic: 2.0,
            tune_hz: 50.0,
            ..KickParams::default()
        };
        let mut v = voice(params);

        v.trigger(36, 100);
        assert!(
            (v.tuned_hz - 100.0).abs() < 0.5,
            "second harmonic of 50 Hz: {}",
            v.tuned_hz
        );

        // An octave up doubles it.
        v.trigger(48, 100);
        assert!(
            (v.tuned_hz - 200.0).abs() < 1.0,
            "an octave up: {}",
            v.tuned_hz
        );

        // And the harmonic knob multiplies.
        v.set_param(kp::DISP_HARMONIC, 4.0);
        v.trigger(36, 100);
        assert!(
            (v.tuned_hz - 200.0).abs() < 1.0,
            "fourth harmonic of 50 Hz: {}",
            v.tuned_hz
        );
        assert_eq!(v.disperser.stages(), 8);
    }

    /// A disperser at zero stages must be a WIRE — the default, so a
    /// fresh kick is a kick and not a phase effect.
    #[test]
    fn the_disperser_is_off_by_default_and_bit_exact_when_off() {
        assert_eq!(KickParams::default().disperse_stages, 0.0);

        let params = KickParams {
            disperse_stages: 0.0,
            ..KickParams::default()
        };
        let mut a = voice(params);
        a.trigger(36, 110);
        let plain = render(&mut a, 2_048);

        let mut b = voice(KickParams {
            // A harmonic and a Q set, but no stages: still a wire.
            disperse_harmonic: 5.0,
            disperse_q: 9.0,
            ..params
        });
        b.trigger(36, 110);
        let same = render(&mut b, 2_048);
        assert!(
            plain
                .iter()
                .zip(&same)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "no stages must sound identical whatever the tuning"
        );
    }

    /// RETRIGGER CUTS ITS OWN TAIL. Two kicks a few milliseconds apart
    /// must not sum into one with a flam and 6 dB of extra peak.
    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let params = KickParams {
            amp_decay_ms: 1_000.0,
            drive: 1.0,
            ..KickParams::default()
        };
        let mut once = voice(params);
        once.trigger(36, 127);
        let single = peak(&render(&mut once, 4_800));

        let mut twice = voice(params);
        twice.trigger(36, 127);
        let _ = render(&mut twice, 240);
        twice.trigger(36, 127);
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
        let params = KickParams {
            disperse_stages: 6.0,
            click_level: 0.5,
            ..KickParams::default()
        };
        let mut whole = voice(params);
        whole.trigger(36, 100);
        let mut a = vec![0.0f32; 256];
        whole.render_add(&mut a, 1.0);

        let mut split = voice(params);
        split.trigger(36, 100);
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
        let mut v = voice(KickParams {
            disperse_stages: 32.0,
            click_level: 1.0,
            drive: 8.0,
            ..KickParams::default()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(36, 100);
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    /// Nonsense in, finite out — at every extreme the table allows, and
    /// past the ones it does not.
    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for stages in [0.0f32, 1.0, 32.0] {
            for drive in [
                crate::dsp::shaper::DRIVE_MIN,
                1.0,
                crate::dsp::shaper::DRIVE_MAX,
            ] {
                for tune in [kp::TUNE_MIN, 50.0, kp::TUNE_MAX] {
                    let mut v = voice(KickParams {
                        disperse_stages: stages,
                        drive,
                        tune_hz: tune,
                        punch_depth: kp::PITCH_DEPTH_MAX,
                        sweep_depth: kp::PITCH_DEPTH_MAX,
                        click_level: 1.0,
                        ..KickParams::default()
                    });
                    for pitch in [0u8, 36, 127] {
                        v.trigger(pitch, 127);
                        let out = render(&mut v, 1_024);
                        assert!(
                            out.iter().all(|s| s.is_finite()),
                            "stages {stages} drive {drive} tune {tune} pitch {pitch}"
                        );
                    }
                }
            }
        }

        // And a voice never prepared renders silence rather than reading
        // tables that are not there.
        let mut bare = KickVoice::new();
        bare.trigger(36, 127);
        let mut out = vec![0.0f32; 128];
        bare.render_add(&mut out, 1.0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Every table id round-trips through the params, and an unknown id
    /// is ignored rather than landing on whichever field is first.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = KickParams::default();
        for def in kp::TABLE {
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

        // Out of range clamps rather than escaping.
        params.set(kp::TUNE, 1e9);
        assert!((params.tune_hz - kp::TUNE_MAX).abs() < 1e-3);
        params.set(kp::TUNE, -1e9);
        assert!((params.tune_hz - kp::TUNE_MIN).abs() < 1e-3);
    }

    /// MIDI 36 is unity, so the `tune` knob means what it says.
    #[test]
    fn the_drum_machine_c_is_the_tuned_note() {
        assert!((pitch_scale(36) - 1.0).abs() < 1e-6);
        assert!((pitch_scale(48) - 2.0).abs() < 1e-5);
        assert!((pitch_scale(24) - 0.5).abs() < 1e-5);
    }

    /// A lock holds for its hit and no other: the knob is what the
    /// restore returns to, and the lock never moved it.
    #[test]
    fn a_lock_is_heard_on_its_hit_and_the_knob_comes_back() {
        let mut v = KickVoice::new();
        v.prepare(48_000.0, KickParams::default());
        v.set_param(kp::AMP_DECAY, 200.0);
        v.plock(kp::AMP_DECAY, Some(50.0));
        assert_eq!(v.params().get(kp::AMP_DECAY), 50.0);
        v.plock(kp::AMP_DECAY, None);
        assert_eq!(
            v.params().get(kp::AMP_DECAY),
            200.0,
            "the lock became the knob"
        );
        v.set_param(kp::AMP_DECAY, 300.0);
        v.plock(kp::AMP_DECAY, None);
        assert_eq!(
            v.params().get(kp::AMP_DECAY),
            300.0,
            "a turned knob was not what came back"
        );
    }
}
