//! HAZE — the pad synth's voice bank.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic below
//! already lives in `src/dsp/`, and this file's whole job is to say
//! which kernel feeds which. Read `notes/20260825-synth-brief.md` for
//! the voice architecture and the lane rule; this header only carries
//! what is specific to a synth built for one job.
//!
//! # What "designed for pads" decides
//!
//! A pad is a slow, held, wide sound, and every choice here falls out of
//! that rather than out of a feature list.
//!
//! **Three detuned copies per note, always.** The stack IS the
//! instrument. One oscillator with a chorus after it is a different and
//! worse sound, because the beating starts after the filter instead of
//! being something the filter is closing over.
//!
//! **Voices drift, and they drift APART.** Each lane carries its own
//! slow random wander of pitch and cutoff, from its own noise seed. This
//! is the whole of what "analog" means to an ear: a digital polysynth is
//! one where every voice is identical, and that is exactly what it
//! sounds like. It costs one heavily-lowpassed noise source per group.
//!
//! **The voices are MONO through the filter, and the width comes after.**
//! Historically accurate — the string machines summed their voices and
//! got their stereo from the ensemble — and it halves the filter cost,
//! which is what pays for three oscillators a note.
//!
//! **Envelopes reach eight and sixteen seconds.** A pad synth whose
//! attack stops at a second is not a pad synth.
//!
//! # The texture, and where it sits
//!
//! Post-sum, all of it: ensemble, wow, grain, warmth. An output stage
//! gets more non-linear as more voices hit it, which is the right way
//! round and is free if the stage is after the sum. Per voice it would
//! be eight times as distorted for eight notes, which is the wrong way
//! round and eight times the cost.
//!
//! Every colour stage reaches an EXACT bypass at the bottom of its
//! range — the promise `preamp` and `lofi` already make and the reason
//! their amounts can be trusted. `silence_in_stays_silent` holds it.
//!
//! # Two rates
//!
//! Per SAMPLE where modulation is a multiply: the amp envelope. Per
//! CONTROL CHUNK where it is a coefficient that costs a transcendental:
//! oscillator frequency and filter cutoff. The same split `poly` makes,
//! for the same reason, and a pad needs it less than a kick does —
//! nothing here moves fast — but a chunk is cheap and a staircase is
//! forever.
//!
//! # Red zone
//!
//! Everything except [`Haze::new`] and [`Haze::prepare`] runs in the
//! callback. No allocation: every buffer is sized once and reused. No
//! panics: lengths come from compile-time constants or from slices, and
//! nothing here indexes without a bound.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::adsr::LaneAdsr;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::filters::{LaneCascade, LaneOnePole};
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::dsp::lofi::Downsampler;
use crate::dsp::noise::LaneWhiteNoise;
use crate::dsp::osc::{LaneOsc, Waveform, build_tables, table_len};
use crate::dsp::{LANES, LaneFrame};
use crate::params::haze as p;

/// Voices, and the lane groups they fall into.
pub const VOICES: usize = p::VOICES;
pub const GROUPS: usize = VOICES / LANES;
/// Detuned copies of every note.
pub const STACK: usize = p::STACK;

/// Samples between coefficient rebuilds.
///
/// Thirty-two is 0.67 ms at 48 kHz. Nothing in a pad moves fast enough
/// to need finer, and the drift this instrument is built around is
/// measured in seconds — but a chunk costs almost nothing and a
/// per-block update would put the wander on a 5 ms staircase, which is
/// audible as steps precisely because everything else is smooth.
const CHUNK: usize = 32;

/// How far drift can pull a voice, in cents, at full amount.
///
/// Twelve. Enough that eight voices are never quite a chord in the same
/// tuning; not so much that a fifth stops being a fifth. Real analog
/// polysynths were worse than this, and the ones people remember fondly
/// were worse still — but they were worse in a way that had a tuning
/// button next to it, and this does not.
const DRIFT_CENTS: f32 = 12.0;

/// How far drift can pull a voice's cutoff, in octaves, at full amount.
const DRIFT_OCTAVES: f32 = 0.22;

/// How slowly drift wanders, as a lowpass corner in hertz.
///
/// A fifth of a hertz: a five-second cycle, near enough. Faster reads as
/// vibrato — a decision, something the player did — and drift has to
/// read as the instrument failing to hold still, which is a different
/// thing and only sounds like itself when it is slower than a phrase.
const DRIFT_HZ: f32 = 0.2;

/// The ensemble's three taps, in milliseconds, and how far they swing.
///
/// Three taps at different rates and depths, which is what a bucket
/// brigade ensemble was: one modulated delay is a chorus, three at
/// incommensurate rates is the wash the string machines are remembered
/// for. The rates are deliberately not related by a simple ratio — at
/// 0.6, 1.1 and 1.7 hertz nothing lines up, and anything that lines up
/// is heard as a pulse.
const ENSEMBLE_TAPS: [(f32, f32, f32); 3] =
    [(11.0, 3.4, 0.61), (17.0, 4.8, 1.13), (23.0, 2.9, 1.69)];

/// Wow's centre delay and swing, in milliseconds, and its rate.
///
/// Slower and deeper than any ensemble tap, because it is a different
/// claim: the ensemble says "several of these at once", wow says "this
/// was recorded onto something that was not turning steadily".
const WOW_MS: f32 = 8.0;
const WOW_SWING_MS: f32 = 3.2;
const WOW_HZ: f32 = 0.37;

/// Everything a note needs after the tables are built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazeParams {
    pub spread: f32,
    pub shape: f32,
    pub sub: f32,
    pub drift: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub track: f32,
    pub env: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub filter_attack: f32,
    pub filter_decay: f32,
    pub ensemble: f32,
    pub wow: f32,
    pub grain: f32,
    pub warmth: f32,
    pub level: f32,
}

impl Default for HazeParams {
    fn default() -> Self {
        let get = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map_or(0.0, |def| def.default)
        };
        Self {
            spread: get(p::SPREAD),
            shape: get(p::SHAPE),
            sub: get(p::SUB),
            drift: get(p::DRIFT),
            cutoff: get(p::CUTOFF),
            resonance: get(p::RESONANCE),
            track: get(p::TRACK),
            env: get(p::ENV),
            attack: get(p::ATTACK),
            decay: get(p::DECAY),
            sustain: get(p::SUSTAIN),
            release: get(p::RELEASE),
            filter_attack: get(p::FILTER_ATTACK),
            filter_decay: get(p::FILTER_DECAY),
            ensemble: get(p::ENSEMBLE),
            wow: get(p::WOW),
            grain: get(p::GRAIN),
            warmth: get(p::WARMTH),
            level: get(p::LEVEL),
        }
    }
}

impl HazeParams {
    /// Write one parameter by wire id, clamped to its own row.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::SPREAD => self.spread = value,
            p::SHAPE => self.shape = value,
            p::SUB => self.sub = value,
            p::DRIFT => self.drift = value,
            p::CUTOFF => self.cutoff = value,
            p::RESONANCE => self.resonance = value,
            p::TRACK => self.track = value,
            p::ENV => self.env = value,
            p::ATTACK => self.attack = value,
            p::DECAY => self.decay = value,
            p::SUSTAIN => self.sustain = value,
            p::RELEASE => self.release = value,
            p::FILTER_ATTACK => self.filter_attack = value,
            p::FILTER_DECAY => self.filter_decay = value,
            p::ENSEMBLE => self.ensemble = value,
            p::WOW => self.wow = value,
            p::GRAIN => self.grain = value,
            p::WARMTH => self.warmth = value,
            p::LEVEL => self.level = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::SPREAD => self.spread,
            p::SHAPE => self.shape,
            p::SUB => self.sub,
            p::DRIFT => self.drift,
            p::CUTOFF => self.cutoff,
            p::RESONANCE => self.resonance,
            p::TRACK => self.track,
            p::ENV => self.env,
            p::ATTACK => self.attack,
            p::DECAY => self.decay,
            p::SUSTAIN => self.sustain,
            p::RELEASE => self.release,
            p::FILTER_ATTACK => self.filter_attack,
            p::FILTER_DECAY => self.filter_decay,
            p::ENSEMBLE => self.ensemble,
            p::WOW => self.wow,
            p::GRAIN => self.grain,
            p::WARMTH => self.warmth,
            p::LEVEL => self.level,
            _ => return None,
        })
    }
}

/// One group of [`LANES`] voices, every kernel lane-major.
struct Group {
    /// The detuned copies, plus the sub octave at the end.
    stack: [LaneOsc; STACK],
    sub: LaneOsc,
    amp: LaneAdsr,
    filter_env: LaneAdsr,
    cascade: LaneCascade,
    /// Drift's source: white noise, lowpassed until it is a wander.
    drift_noise: LaneWhiteNoise,
    drift_lp: LaneOnePole,
    /// A second, independently seeded wander for the cutoff, so pitch
    /// and colour do not drift in lockstep — which would read as one
    /// modulation aimed at two places rather than as two things being
    /// separately unsteady.
    colour_noise: LaneWhiteNoise,
    colour_lp: LaneOnePole,
    /// The note each lane is playing, and where it is going.
    note_hz: [f32; LANES],
    note_key: [f32; LANES],
    vel: [f32; LANES],
    /// Which note a lane holds, for note-off, and when it started.
    holding: [Option<u8>; LANES],
    age: [u64; LANES],
    /// Where the two wanders have got to, sampled at the last chunk
    /// boundary. Held rather than recomputed inside a span, because a
    /// span's length is whatever the caller's block left over and
    /// coefficients must not depend on that.
    pitch_drift: [f32; LANES],
    colour_drift: [f32; LANES],
}

impl Group {
    fn new() -> Self {
        Self {
            stack: [LaneOsc::new(); STACK],
            sub: LaneOsc::new(),
            amp: LaneAdsr::new(),
            filter_env: LaneAdsr::new(),
            cascade: LaneCascade::new(),
            drift_noise: LaneWhiteNoise::new(),
            drift_lp: LaneOnePole::new(),
            colour_noise: LaneWhiteNoise::new(),
            colour_lp: LaneOnePole::new(),
            note_hz: [0.0; LANES],
            note_key: [60.0; LANES],
            vel: [0.0; LANES],
            holding: [None; LANES],
            age: [0; LANES],
            pitch_drift: [0.0; LANES],
            colour_drift: [0.0; LANES],
        }
    }
}

/// The instrument.
pub struct Haze {
    sample_rate: f32,
    params: HazeParams,
    groups: [Group; GROUPS],
    /// Oscillator tables, built once. Shared by every lane: a table is
    /// read-only and a copy per group would be the same bytes twice.
    tables: Vec<f32>,
    sub_tables: Vec<f32>,
    /// One control chunk's worth of lane-major working space.
    voice: [LaneFrame; CHUNK],
    copy: [LaneFrame; CHUNK],
    drift: [LaneFrame; CHUNK],
    envelope: [LaneFrame; CHUNK],
    /// The mono sum of every group, one chunk at a time.
    mono: [f32; CHUNK],
    /// The output chain, all post-sum.
    ensemble: [DelayLine; 3],
    ensemble_lfo: [Lfo; 3],
    wow_line: DelayLine,
    wow_lfo: Lfo,
    grain: Downsampler,
    warmth: crate::audio::preamp::Preamp,
    /// The delay lines' RING BUFFERS, one each.
    ///
    /// The kernel does not own its memory — `DelayLine` is state and
    /// arithmetic, the samples are the caller's — so these persist for
    /// the life of the instrument and are sized once. A line handed a
    /// buffer of the wrong length fails OPEN and passes the dry signal,
    /// which is a silence that looks like working code: the ensemble
    /// simply does nothing and nothing says so.
    ensemble_buf: [Vec<f32>; 3],
    wow_buf: Vec<f32>,
    /// One tap's wet signal while it is being made.
    tapped: [f32; CHUNK],
    /// The per-sample delay times the lines read.
    delay_mod: [f32; CHUNK],
    lfo_scratch: [f32; CHUNK],
    /// The stereo the ensemble makes, one chunk at a time.
    left: [f32; CHUNK],
    right: [f32; CHUNK],
    /// Right channel of the last render, for the node to pick up.
    right_out: Vec<f32>,
    /// Rising stamp for voice stealing.
    next_age: u64,
    /// Where we are inside the control chunk, in samples.
    ///
    /// ABSOLUTE, and that is the whole point of it. Coefficients rebuild
    /// when this is zero, so the rebuild grid is a property of the
    /// instrument and not of whatever block length the caller happened
    /// to ask for — which is what makes rendering in pieces produce the
    /// same audio as rendering whole, bit for bit. Without it a 100-frame
    /// block and a 1024-frame block update the filter at different
    /// moments, and the transport splits blocks wherever a loop point
    /// falls.
    phase: usize,
}

impl Haze {
    /// Green zone: builds the tables and every buffer, once.
    pub fn new(sample_rate: f32, params: HazeParams) -> Self {
        let mut haze = Self {
            sample_rate: sample_rate.max(1.0),
            params,
            groups: [(); GROUPS].map(|()| Group::new()),
            tables: vec![0.0; table_len(Waveform::Saw)],
            sub_tables: vec![0.0; table_len(Waveform::Saw)],
            voice: [[0.0; LANES]; CHUNK],
            copy: [[0.0; LANES]; CHUNK],
            drift: [[0.0; LANES]; CHUNK],
            envelope: [[0.0; LANES]; CHUNK],
            mono: [0.0; CHUNK],
            ensemble: [DelayLine::new(); 3],
            ensemble_lfo: [Lfo::new(); 3],
            wow_line: DelayLine::new(),
            wow_lfo: Lfo::new(),
            grain: Downsampler::new(),
            warmth: crate::audio::preamp::Preamp::new(),
            ensemble_buf: [const { Vec::new() }; 3],
            wow_buf: Vec::new(),
            tapped: [0.0; CHUNK],
            delay_mod: [0.0; CHUNK],
            lfo_scratch: [0.0; CHUNK],
            left: [0.0; CHUNK],
            right: [0.0; CHUNK],
            right_out: Vec::new(),
            next_age: 1,
            phase: 0,
        };
        haze.prepare();
        haze
    }

    /// Green zone: rebuild everything the sample rate or the parameters
    /// decide. Called on construction and whenever a knob moves.
    pub fn prepare(&mut self) {
        build_tables(Waveform::Saw, &mut self.tables);
        build_tables(Waveform::Sine, &mut self.sub_tables);
        let rate = self.sample_rate;

        for (index, group) in self.groups.iter_mut().enumerate() {
            for osc in group.stack.iter_mut() {
                osc.prepare(rate, Waveform::Saw);
            }
            group.sub.prepare(rate, Waveform::Sine);
            group.amp.prepare(
                rate,
                self.params.attack * 1000.0,
                self.params.decay * 1000.0,
                self.params.sustain,
                self.params.release * 1000.0,
            );
            // The filter envelope borrows the amp's sustain and release.
            // Two more knobs would buy a shape nobody adjusts on a pad —
            // what matters is that the colour arrives AFTER the note,
            // which is the attack, and leaves with it, which is the
            // release. `f.atk` and `f.dec` are the two that earn a knob.
            group.filter_env.prepare(
                rate,
                self.params.filter_attack * 1000.0,
                self.params.filter_decay * 1000.0,
                self.params.sustain,
                self.params.release * 1000.0,
            );
            group.drift_lp.prepare(rate, DRIFT_HZ);
            group.colour_lp.prepare(rate, DRIFT_HZ);
            // Seeded PER GROUP and differently for the two wanders, so
            // sixteen voices are sixteen wanders rather than eight
            // played twice.
            group.drift_noise.seed(0x9E37_79B9_u64 ^ (index as u64 + 1));
            group
                .colour_noise
                .seed(0x517C_C1B7_u64 ^ ((index as u64 + 1) << 17));
        }

        let max_delay = (rate * 0.05).ceil() as usize + 4;
        let ring = buffer_len(max_delay);
        for (line, buf) in self.ensemble.iter_mut().zip(self.ensemble_buf.iter_mut()) {
            line.prepare(max_delay);
            // Exactly the length the line was prepared for, and zeroed:
            // both are the kernel's stated contract, and getting either
            // wrong makes it fail open instead of loudly.
            buf.clear();
            buf.resize(ring, 0.0);
        }
        for (lfo, (_, _, hz)) in self.ensemble_lfo.iter_mut().zip(ENSEMBLE_TAPS) {
            lfo.prepare(rate);
            lfo.set_shape(LfoShape::Sine);
            lfo.set_rate(hz);
        }
        // Phases spread around the circle so the three taps do not all
        // start at their centre and swing together for the first second.
        for (index, lfo) in self.ensemble_lfo.iter_mut().enumerate() {
            lfo.set_phase(index as f32 / 3.0);
        }
        self.wow_line.prepare(max_delay);
        self.wow_buf.clear();
        self.wow_buf.resize(ring, 0.0);
        self.wow_lfo.prepare(rate);
        self.wow_lfo.set_shape(LfoShape::Sine);
        self.wow_lfo.set_rate(WOW_HZ);

        self.grain.prepare(rate);
        // ONE knob over two axes. Bit depth falls from sixteen to six
        // and the rate from full to a fifth, together, because "how
        // lo-fi" is one question — and both reach their transparent end
        // at exactly zero, which is what makes the bypass exact.
        let grain = self.params.grain.clamp(0.0, 1.0);
        if grain <= 0.0 {
            self.grain.set_bits(0.0);
            self.grain.set_rate(rate.max(crate::dsp::lofi::RATE_MIN));
        } else {
            self.grain.set_bits(16.0 - 10.0 * grain);
            let floor = crate::dsp::lofi::RATE_MIN;
            self.grain.set_rate((rate * (1.0 - 0.8 * grain)).max(floor));
        }
        self.warmth.prepare(rate);
        self.warmth.set_amount(self.params.warmth);
    }

    pub fn params(&self) -> &HazeParams {
        &self.params
    }

    /// Red zone: one knob, and the coefficients it decides.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.retune();
    }

    /// Rebuild only what a knob can change without allocating.
    ///
    /// Split out of [`Self::prepare`] because that one builds tables,
    /// which allocates — this is the half a running callback may call.
    fn retune(&mut self) {
        let rate = self.sample_rate;
        for group in self.groups.iter_mut() {
            group.amp.prepare(
                rate,
                self.params.attack * 1000.0,
                self.params.decay * 1000.0,
                self.params.sustain,
                self.params.release * 1000.0,
            );
            group.filter_env.prepare(
                rate,
                self.params.filter_attack * 1000.0,
                self.params.filter_decay * 1000.0,
                self.params.sustain,
                self.params.release * 1000.0,
            );
        }
        let grain = self.params.grain.clamp(0.0, 1.0);
        if grain <= 0.0 {
            self.grain.set_bits(0.0);
            self.grain.set_rate(rate.max(crate::dsp::lofi::RATE_MIN));
        } else {
            self.grain.set_bits(16.0 - 10.0 * grain);
            let floor = crate::dsp::lofi::RATE_MIN;
            self.grain.set_rate((rate * (1.0 - 0.8 * grain)).max(floor));
        }
        self.warmth.set_amount(self.params.warmth);
    }

    /// Red zone: silence everything at once, for a transport
    /// discontinuity. The sequencing contract's all-sound-off.
    pub fn all_sound_off(&mut self) {
        for group in self.groups.iter_mut() {
            group.amp.reset();
            group.filter_env.reset();
            group.cascade.reset();
            for osc in group.stack.iter_mut() {
                osc.reset();
            }
            group.sub.reset();
            group.holding = [None; LANES];
            group.vel = [0.0; LANES];
        }
        for line in self.ensemble.iter_mut() {
            line.reset();
        }
        self.wow_line.reset();
        self.grain.reset();
        self.warmth.reset();
    }

    /// Red zone: let go of every held note, leaving the releases to ring.
    pub fn release_all(&mut self) {
        for group in self.groups.iter_mut() {
            for lane in 0..LANES {
                if group.holding.get(lane).copied().flatten().is_some() {
                    group.amp.gate_off(lane);
                    group.filter_env.gate_off(lane);
                    if let Some(slot) = group.holding.get_mut(lane) {
                        *slot = None;
                    }
                }
            }
        }
    }

    /// Red zone: start a note on the best lane available.
    ///
    /// Free lanes first, then the OLDEST sounding one. Age and not
    /// quietness, because a pad's quietest voice is usually the one that
    /// just started its long attack — stealing by level would make the
    /// instrument eat its own new notes.
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        let hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        let velocity = f32::from(vel.max(1)) / 127.0;
        let stamp = age.max(self.next_age);
        self.next_age = stamp.saturating_add(1);

        let mut free: Option<(usize, usize)> = None;
        let mut oldest: Option<(usize, usize, u64)> = None;
        for (index, group) in self.groups.iter().enumerate() {
            for lane in 0..LANES {
                let held = group.holding.get(lane).copied().flatten().is_some();
                let sounding = held || group.amp.active(lane);
                if !sounding {
                    free.get_or_insert((index, lane));
                }
                let at = group.age.get(lane).copied().unwrap_or(0);
                if oldest.is_none_or(|(_, _, best)| at < best) {
                    oldest = Some((index, lane, at));
                }
            }
        }
        let Some((index, lane)) = free.or(oldest.map(|(g, l, _)| (g, l))) else {
            return;
        };
        let Some(group) = self.groups.get_mut(index) else {
            return;
        };
        if let Some(slot) = group.note_hz.get_mut(lane) {
            *slot = hz;
        }
        if let Some(slot) = group.note_key.get_mut(lane) {
            *slot = f32::from(pitch);
        }
        if let Some(slot) = group.vel.get_mut(lane) {
            *slot = velocity;
        }
        if let Some(slot) = group.holding.get_mut(lane) {
            *slot = Some(pitch);
        }
        if let Some(slot) = group.age.get_mut(lane) {
            *slot = stamp;
        }
        // The copies start at DIFFERENT phases. Started together they
        // sum to one loud edge before the detuning pulls them apart,
        // which is a click at the top of every note — the one thing a
        // slow instrument must not have.
        for (copy, osc) in group.stack.iter_mut().enumerate() {
            osc.set_phase(lane, copy as f32 / STACK as f32);
        }
        group.sub.set_phase(lane, 0.0);
        group.amp.gate_on(lane);
        group.filter_env.gate_on(lane);
    }

    /// Red zone: release every lane holding this pitch.
    pub fn note_off(&mut self, pitch: u8) {
        for group in self.groups.iter_mut() {
            for lane in 0..LANES {
                if group.holding.get(lane).copied().flatten() == Some(pitch) {
                    group.amp.gate_off(lane);
                    group.filter_env.gate_off(lane);
                    if let Some(slot) = group.holding.get_mut(lane) {
                        *slot = None;
                    }
                }
            }
        }
    }

    /// Whether anything is still making sound.
    pub fn active(&self) -> bool {
        self.groups.iter().any(|group| group.amp.any_active())
    }

    /// The right channel of the last [`Self::render`].
    pub fn right(&self, len: usize) -> &[f32] {
        let len = len.min(self.right_out.len());
        self.right_out.get(..len).unwrap_or(&[])
    }

    /// Red zone: fill `out` (left) and the right channel, adding nothing
    /// that was not asked for.
    pub fn render(&mut self, out: &mut [f32]) {
        self.right_out.resize(out.len(), 0.0);
        let mut done = 0;
        while done < out.len() {
            // Up to the next chunk boundary, never past it.
            let len = (CHUNK - self.phase).min(out.len() - done);
            if self.phase == 0 {
                self.update_voices();
            }
            self.render_span(len);
            for (index, sample) in self.left.iter().take(len).enumerate() {
                if let Some(slot) = out.get_mut(done + index) {
                    *slot = *sample;
                }
            }
            for (index, sample) in self.right.iter().take(len).enumerate() {
                if let Some(slot) = self.right_out.get_mut(done + index) {
                    *slot = *sample;
                }
            }
            done += len;
            self.phase = (self.phase + len) % CHUNK;
        }
    }

    /// At a chunk boundary: sample the wanders and rebuild every
    /// coefficient they and the envelopes decide.
    ///
    /// Separated from the rendering because coefficients must land on an
    /// absolute grid — see [`Self::phase`].
    fn update_voices(&mut self) {
        let rate = self.sample_rate;
        let params = self.params;
        let q = 0.707 + params.resonance * 3.4;
        for group in self.groups.iter_mut() {
            if !group.amp.any_active() {
                continue;
            }
            for (copy, osc) in group.stack.iter_mut().enumerate() {
                let offset = if STACK <= 1 {
                    0.0
                } else {
                    (copy as f32 / (STACK - 1) as f32) * 2.0 - 1.0
                };
                let mut hz = [0.0f32; LANES];
                for lane in 0..LANES {
                    let base = group.note_hz.get(lane).copied().unwrap_or(0.0);
                    let cents = offset * params.spread
                        + group.pitch_drift.get(lane).copied().unwrap_or(0.0)
                            * params.drift
                            * DRIFT_CENTS;
                    if let Some(slot) = hz.get_mut(lane) {
                        *slot = base * (cents / 1200.0).exp2();
                    }
                }
                osc.set_freqs(&hz);
            }
            let mut hz = [0.0f32; LANES];
            for lane in 0..LANES {
                if let Some(slot) = hz.get_mut(lane) {
                    *slot = group.note_hz.get(lane).copied().unwrap_or(0.0) * 0.5;
                }
            }
            group.sub.set_freqs(&hz);

            let mut cutoff = [0.0f32; LANES];
            for lane in 0..LANES {
                let key = group.note_key.get(lane).copied().unwrap_or(60.0);
                // The envelope AS IT STANDS, not the end of a block:
                // asking a block for its last value would make the
                // cutoff depend on how long the block was.
                let env = group.filter_env.current(lane);
                let octaves = params.track * (key - 60.0) / 12.0
                    + params.env * env * 4.0
                    + group.colour_drift.get(lane).copied().unwrap_or(0.0)
                        * params.drift
                        * DRIFT_OCTAVES;
                if let Some(slot) = cutoff.get_mut(lane) {
                    *slot = (params.cutoff * octaves.exp2())
                        .clamp(p::CUTOFF_MIN, (rate * 0.45).min(p::CUTOFF_MAX));
                }
            }
            // Four poles. A pad's filter has to be able to take the top
            // off a three-oscillator stack and still leave something,
            // which two poles cannot do without also removing the note.
            group.cascade.prepare_lanes(rate, &cutoff, q, 4, false);
        }
    }

    /// One span of at most a chunk: the voices, summed, then the texture.
    fn render_span(&mut self, len: usize) {
        for slot in self.mono.iter_mut().take(len) {
            *slot = 0.0;
        }
        let params = self.params;

        for group in self.groups.iter_mut() {
            if !group.amp.any_active() {
                continue;
            }
            // --- the two wanders advance, for the NEXT boundary -------
            //
            // Advanced by exactly the samples rendered, whatever the
            // span's length, so the wander is a function of how much
            // audio has passed and not of how it was cut up.
            group.drift_noise.process(&mut self.drift[..len]);
            group.drift_lp.process_lowpass(&mut self.drift[..len]);
            if let Some(frame) = self.drift.get(len.saturating_sub(1)) {
                // The lowpass leaves a very small number: a one-pole at
                // a fifth of a hertz throws away almost all of white
                // noise's energy. Scaled back up here rather than by
                // asking the kernel for gain it does not have.
                for (slot, value) in group.pitch_drift.iter_mut().zip(frame) {
                    *slot = (value * 60.0).clamp(-1.0, 1.0);
                }
            }
            group.colour_noise.process(&mut self.drift[..len]);
            group.colour_lp.process_lowpass(&mut self.drift[..len]);
            if let Some(frame) = self.drift.get(len.saturating_sub(1)) {
                for (slot, value) in group.colour_drift.iter_mut().zip(frame) {
                    *slot = (value * 60.0).clamp(-1.0, 1.0);
                }
            }

            // --- the stack, at the frequencies the boundary set -------
            for slot in self.voice.iter_mut().take(len) {
                *slot = [0.0; LANES];
            }
            for osc in group.stack.iter_mut() {
                osc.process(&mut self.copy[..len], None, &self.tables);
                for (dst, src) in self.voice.iter_mut().zip(self.copy.iter()).take(len) {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d += s / STACK as f32;
                    }
                }
            }
            // The sub, an octave down and undetuned: weight, not width.
            if params.sub > 0.0 {
                group
                    .sub
                    .process(&mut self.copy[..len], None, &self.sub_tables);
                for (dst, src) in self.voice.iter_mut().zip(self.copy.iter()).take(len) {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d += s * params.sub;
                    }
                }
            }
            // SHAPE folds the saw toward a triangle by softening it: a
            // saw through a gentle odd-symmetric squash loses its even
            // harmonics fastest, which is the audible difference between
            // the two shapes and costs one multiply-and-clamp rather
            // than a second table set.
            if params.shape < 1.0 {
                let soften = 1.0 - params.shape;
                for frame in self.voice.iter_mut().take(len) {
                    for sample in frame.iter_mut() {
                        let x = *sample;
                        *sample = x * (1.0 - soften) + (x - x * x * x / 3.0) * soften;
                    }
                }
            }

            // --- the filter, at the corner the boundary set -----------
            group.filter_env.process(&mut self.envelope[..len]);
            group.cascade.process(&mut self.voice[..len]);

            // --- amp, then down to mono -------------------------------
            group.amp.process(&mut self.envelope[..len]);
            for (index, frame) in self.voice.iter().take(len).enumerate() {
                let mut sum = 0.0;
                for (lane, sample) in frame.iter().enumerate() {
                    let env = self
                        .envelope
                        .get(index)
                        .and_then(|f| f.get(lane))
                        .copied()
                        .unwrap_or(0.0);
                    sum += sample * env * group.vel.get(lane).copied().unwrap_or(0.0);
                }
                if let Some(slot) = self.mono.get_mut(index) {
                    *slot += sum;
                }
            }
        }

        self.texture(len);
    }

    /// The output stage: ensemble, wow, grain, warmth, level.
    fn texture(&mut self, len: usize) {
        // Destructured up front so the loop below can hold a mutable
        // borrow of several fields at once — the borrow checker allows
        // disjoint fields, but only when they are spelled out.
        let Self {
            sample_rate: rate,
            params,
            ensemble,
            ensemble_lfo,
            ensemble_buf,
            wow_line,
            wow_lfo,
            wow_buf,
            grain,
            warmth,
            tapped,
            delay_mod,
            lfo_scratch,
            mono,
            left,
            right,
            ..
        } = self;
        let rate = *rate;
        let params = *params;

        // --- wow, before the ensemble -----------------------------------
        //
        // The tape came first: a recording that wobbles and is then run
        // through an ensemble is what a string machine on tape sounded
        // like, and the other order puts the ensemble's own movement
        // through the wobble, which smears it into mush.
        if params.wow > 0.0 {
            wow_lfo.process(&mut lfo_scratch[..len]);
            let centre = WOW_MS * 0.001 * rate;
            let swing = WOW_SWING_MS * 0.001 * rate * params.wow;
            for (dst, src) in delay_mod.iter_mut().zip(lfo_scratch.iter()).take(len) {
                *dst = (centre + src * swing).max(1.0);
            }
            wow_line.process_modulated(&mut mono[..len], wow_buf, &delay_mod[..len]);
        }

        // --- the ensemble, three taps into two channels -----------------
        for (index, sample) in mono.iter().take(len).enumerate() {
            if let Some(slot) = left.get_mut(index) {
                *slot = *sample;
            }
            if let Some(slot) = right.get_mut(index) {
                *slot = *sample;
            }
        }
        if params.ensemble > 0.0 {
            for (tap, ((line, buf), lfo)) in ensemble
                .iter_mut()
                .zip(ensemble_buf.iter_mut())
                .zip(ensemble_lfo.iter_mut())
                .enumerate()
            {
                let Some((centre, swing, _)) = ENSEMBLE_TAPS.get(tap).copied() else {
                    continue;
                };
                lfo.process(&mut lfo_scratch[..len]);
                let centre = centre * 0.001 * rate;
                let swing = swing * 0.001 * rate * params.ensemble;
                for (dst, src) in delay_mod.iter_mut().zip(lfo_scratch.iter()).take(len) {
                    *dst = (centre + src * swing).max(1.0);
                }
                for (dst, src) in tapped.iter_mut().zip(mono.iter()).take(len) {
                    *dst = *src;
                }
                line.process_modulated(&mut tapped[..len], buf, &delay_mod[..len]);
                // Taps alternate across the field: the outer two go hard
                // either way and the middle one stays put, which is the
                // widest three taps can be without either channel losing
                // the note itself.
                let (gl, gr) = match tap {
                    0 => (0.9, 0.2),
                    1 => (0.55, 0.55),
                    _ => (0.2, 0.9),
                };
                for index in 0..len {
                    let wet = tapped.get(index).copied().unwrap_or(0.0) * params.ensemble;
                    if let Some(slot) = left.get_mut(index) {
                        *slot += wet * gl;
                    }
                    if let Some(slot) = right.get_mut(index) {
                        *slot += wet * gr;
                    }
                }
            }
            // Three wet taps on top of the dry is more signal than went
            // in, so the whole thing comes back down by what was added.
            let trim = 1.0 / (1.0 + params.ensemble * 1.65);
            for index in 0..len {
                if let Some(slot) = left.get_mut(index) {
                    *slot *= trim;
                }
                if let Some(slot) = right.get_mut(index) {
                    *slot *= trim;
                }
            }
        }

        // --- grain, then warmth, then level -----------------------------
        if !grain.is_bypassed() {
            grain.process(&mut left[..len]);
            grain.process(&mut right[..len]);
        }
        if !warmth.is_bypassed() {
            warmth.process(&mut left[..len], &mut right[..len]);
        }
        for index in 0..len {
            if let Some(slot) = left.get_mut(index) {
                *slot *= params.level;
            }
            if let Some(slot) = right.get_mut(index) {
                *slot *= params.level;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn synth() -> Haze {
        Haze::new(RATE, HazeParams::default())
    }

    fn render(haze: &mut Haze, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0; frames];
        haze.render(&mut left);
        let right = haze.right(frames).to_vec();
        (left, right)
    }

    fn peak(block: &[f32]) -> f32 {
        block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// SILENCE IN STAYS SILENT, at every setting of every colour stage.
    ///
    /// The bypass promise this codebase's colour stages all make, and
    /// the one an instrument makes twice over: an idle synth that hisses
    /// is a synth people switch off, and a texture knob that leaks is a
    /// texture knob nobody trusts at the bottom.
    #[test]
    fn silence_in_stays_silent() {
        for grain in [0.0, 0.5, 1.0] {
            for warmth in [0.0, 0.5, 1.0] {
                for ensemble in [0.0, 1.0] {
                    let mut haze = Haze::new(
                        RATE,
                        HazeParams {
                            grain,
                            warmth,
                            ensemble,
                            wow: 1.0,
                            ..HazeParams::default()
                        },
                    );
                    let (left, right) = render(&mut haze, 1024);
                    assert_eq!(
                        peak(&left),
                        0.0,
                        "grain {grain} warmth {warmth} ens {ensemble} leaked on the left"
                    );
                    assert_eq!(peak(&right), 0.0, "and on the right");
                }
            }
        }
    }

    /// A note makes sound, and stops making it.
    #[test]
    fn a_note_sounds_and_a_release_ends() {
        let mut haze = Haze::new(
            RATE,
            HazeParams {
                attack: 0.005,
                release: 0.05,
                ..HazeParams::default()
            },
        );
        haze.note_on(60, 100, 1);
        let (left, right) = render(&mut haze, 4096);
        assert!(
            peak(&left) > 0.01,
            "the note never arrived: {}",
            peak(&left)
        );
        assert!(peak(&right) > 0.01);
        assert!(haze.active());

        haze.note_off(60);
        // Long enough for a 50 ms release plus the ensemble's tail.
        let (left, _) = render(&mut haze, 48_000);
        assert!(!haze.active(), "the voice never let go");
        let tail = &left[left.len() - 512..];
        assert!(peak(tail) < 1e-4, "it is still sounding: {}", peak(tail));
    }

    /// NOTHING IS EVER NOT A NUMBER.
    ///
    /// Every parameter at both ends, a chord held through it. A synth
    /// that produces one NaN poisons the master bus for the rest of the
    /// session, and the arithmetic here has exponentials, a resonant
    /// four-pole and three feedback-free delays in it.
    #[test]
    fn every_extreme_stays_finite() {
        for def in p::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut haze = synth();
                haze.set_param(def.id, value);
                haze.note_on(36, 127, 1);
                haze.note_on(60, 64, 2);
                haze.note_on(84, 1, 3);
                let (left, right) = render(&mut haze, 4096);
                for (channel, block) in [("left", &left), ("right", &right)] {
                    assert!(
                        block.iter().all(|s| s.is_finite()),
                        "{} at {value} made a non-finite {channel}",
                        def.name
                    );
                    assert!(
                        peak(block) < 24.0,
                        "{} at {value} ran away: {}",
                        def.name,
                        peak(block)
                    );
                }
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE: the same note rendered in one block and
    /// in pieces is the same audio.
    ///
    /// The property the segmented transport depends on — a loop point or
    /// a seek splits a block wherever it likes — and the one that catches
    /// nearly every state bug in a thing this size.
    #[test]
    fn rendering_in_pieces_is_rendering_whole() {
        let bare = || {
            Haze::new(
                RATE,
                HazeParams {
                    ensemble: 0.0,
                    wow: 0.0,
                    grain: 0.0,
                    warmth: 0.0,
                    ..HazeParams::default()
                },
            )
        };
        let mut a = bare();
        a.note_on(62, 90, 1);
        let (bare_whole, _) = render(&mut a, 1024);
        let mut b = bare();
        b.note_on(62, 90, 1);
        let mut bare_split = Vec::new();
        for len in [100, 1, 411, 512] {
            let mut block = vec![0.0; len];
            b.render(&mut block);
            bare_split.extend(block);
        }
        for (index, (x, y)) in bare_whole.iter().zip(bare_split.iter()).enumerate() {
            assert_eq!(x, y, "VOICES diverged at {index}");
        }

        let mut whole = synth();
        whole.note_on(62, 90, 1);
        let (left_whole, right_whole) = render(&mut whole, 1024);

        let mut split = synth();
        split.note_on(62, 90, 1);
        let mut left_split = Vec::new();
        let mut right_split = Vec::new();
        for len in [100, 1, 411, 512] {
            let mut block = vec![0.0; len];
            split.render(&mut block);
            right_split.extend_from_slice(split.right(len));
            left_split.extend(block);
        }
        assert_eq!(left_split.len(), 1024);
        for (index, (a, b)) in left_whole.iter().zip(left_split.iter()).enumerate() {
            assert_eq!(a, b, "left diverged at {index}");
        }
        for (index, (a, b)) in right_whole.iter().zip(right_split.iter()).enumerate() {
            assert_eq!(a, b, "right diverged at {index}");
        }
    }

    /// EDGE LENGTHS: nothing here indexes past what it was given.
    #[test]
    fn odd_block_lengths_are_fine() {
        let mut haze = synth();
        haze.note_on(60, 100, 1);
        for len in [0, 1, 2, 7, 31, 33, 97, CHUNK, CHUNK + 1] {
            let mut block = vec![0.0; len];
            haze.render(&mut block);
            assert!(block.iter().all(|s| s.is_finite()), "len {len}");
            assert_eq!(haze.right(len).len(), len);
        }
    }

    /// DRIFT IS WHAT MAKES IT ANALOG, so it has to actually be there —
    /// and it has to be DIFFERENT per voice, or it is one modulation
    /// applied to everything, which is a chorus and not an instrument.
    #[test]
    fn drift_makes_voices_differ_from_each_other() {
        // The same note on two lanes, twice: once with drift and once
        // without. Without it the two lanes are identical arithmetic and
        // the pair sums to exactly double; with it they diverge.
        let steady = {
            let mut haze = Haze::new(
                RATE,
                HazeParams {
                    drift: 0.0,
                    ensemble: 0.0,
                    wow: 0.0,
                    grain: 0.0,
                    warmth: 0.0,
                    attack: 0.001,
                    ..HazeParams::default()
                },
            );
            haze.note_on(60, 100, 1);
            haze.note_on(60, 100, 2);
            render(&mut haze, 24_000).0
        };
        let wandering = {
            let mut haze = Haze::new(
                RATE,
                HazeParams {
                    drift: 1.0,
                    ensemble: 0.0,
                    wow: 0.0,
                    grain: 0.0,
                    warmth: 0.0,
                    attack: 0.001,
                    ..HazeParams::default()
                },
            );
            haze.note_on(60, 100, 1);
            haze.note_on(60, 100, 2);
            render(&mut haze, 24_000).0
        };
        let difference: f32 = steady
            .iter()
            .zip(wandering.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / steady.len() as f32;
        assert!(
            difference > 1e-3,
            "drift changed nothing: mean difference {difference}"
        );
    }

    /// THE STACK IS THE INSTRUMENT: spread has to reach the audio.
    #[test]
    fn spread_detunes_the_stack() {
        let render_at = |spread: f32| {
            let mut haze = Haze::new(
                RATE,
                HazeParams {
                    spread,
                    drift: 0.0,
                    ensemble: 0.0,
                    wow: 0.0,
                    grain: 0.0,
                    warmth: 0.0,
                    attack: 0.001,
                    ..HazeParams::default()
                },
            );
            haze.note_on(57, 100, 1);
            render(&mut haze, 24_000).0
        };
        let tight = render_at(0.0);
        let wide = render_at(p::SPREAD_MAX);
        // Detuned copies beat against each other, so the ENVELOPE of the
        // sum moves where the tight one is steady. Measured as the swing
        // of a windowed peak rather than sample by sample, because the
        // waveform differing proves only that the pitch changed.
        let swing = |block: &[f32]| {
            let mut lo = f32::MAX;
            let mut hi: f32 = 0.0;
            for window in block.chunks(2048) {
                let level = peak(window);
                lo = lo.min(level);
                hi = hi.max(level);
            }
            hi - lo
        };
        assert!(
            swing(&wide) > swing(&tight) * 2.0,
            "a wide spread should beat: {} against {}",
            swing(&wide),
            swing(&tight)
        );
    }

    /// THE ENSEMBLE AND THE WOW ACTUALLY DO SOMETHING.
    ///
    /// Both ride `DelayLine`, which does not own its memory and FAILS
    /// OPEN when handed a buffer of the wrong length — it passes the dry
    /// signal and says nothing. That is a silence which looks exactly
    /// like working code, and it is what the first version of this file
    /// did: three ensemble taps adding three dry copies of the input.
    /// Only a test that asks whether the wet differs from the dry can
    /// tell the difference.
    #[test]
    fn the_texture_stages_are_wired_to_their_buffers() {
        let dry = |ensemble: f32, wow: f32| {
            let mut haze = Haze::new(
                RATE,
                HazeParams {
                    ensemble,
                    wow,
                    grain: 0.0,
                    warmth: 0.0,
                    drift: 0.0,
                    attack: 0.001,
                    ..HazeParams::default()
                },
            );
            haze.note_on(60, 100, 1);
            render(&mut haze, 24_000)
        };
        let (flat_l, flat_r) = dry(0.0, 0.0);
        let (wide_l, wide_r) = dry(1.0, 0.0);
        let (wobbly_l, _) = dry(0.0, 1.0);

        let differs = |a: &[f32], b: &[f32]| -> f32 {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (x - y).abs())
                .sum::<f32>()
                / a.len() as f32
        };
        assert!(
            differs(&flat_l, &wide_l) > 1e-3,
            "the ensemble changed nothing — it is failing open"
        );
        assert!(
            differs(&flat_l, &wobbly_l) > 1e-4,
            "wow changed nothing — it is failing open"
        );
        // And the ensemble is what makes it STEREO: without it the two
        // channels are the same mono sum, with it they are not.
        assert!(
            differs(&flat_l, &flat_r) < 1e-9,
            "something made width before the ensemble did"
        );
        assert!(
            differs(&wide_l, &wide_r) > 1e-3,
            "the ensemble did not spread the channels apart"
        );
    }

    /// A pad is a HELD sound: the defaults have to survive being held.
    #[test]
    fn the_default_patch_is_a_pad() {
        let params = HazeParams::default();
        assert!(params.attack >= 0.5, "a pad's attack is not instant");
        assert!(params.release >= 1.0, "nor is its release");
        assert!(params.sustain >= 0.6, "a pad is held, not plucked");
        assert!(params.drift > 0.0, "the analog knob is the default");
        assert!(params.ensemble > 0.0, "so is the ensemble");

        let mut haze = synth();
        haze.note_on(48, 90, 1);
        haze.note_on(55, 90, 2);
        haze.note_on(60, 90, 3);
        haze.note_on(64, 90, 4);
        // Ten seconds of a held chord: level steady, nothing runaway.
        let (left, right) = render(&mut haze, 480_000);
        let late = &left[left.len() - 48_000..];
        assert!(peak(late) > 0.02, "the chord faded on its own");
        assert!(peak(&left) < 4.0 && peak(&right) < 4.0, "it ran away");
    }

    /// STEALING TAKES THE OLDEST, not the quietest.
    ///
    /// A pad's quietest voice is usually the one that just started its
    /// long attack, so stealing by level would make the instrument eat
    /// its own new notes.
    #[test]
    fn a_full_synth_steals_the_oldest_voice() {
        let mut haze = synth();
        for note in 0..VOICES {
            haze.note_on(48 + note as u8, 100, note as u64 + 1);
        }
        // One more than it has: the first note must be the one to go.
        haze.note_on(90, 100, VOICES as u64 + 1);
        let mut holding = Vec::new();
        for group in &haze.groups {
            for lane in 0..LANES {
                if let Some(pitch) = group.holding.get(lane).copied().flatten() {
                    holding.push(pitch);
                }
            }
        }
        assert!(holding.contains(&90), "the new note did not land");
        assert!(!holding.contains(&48), "the oldest note survived");
        assert_eq!(holding.len(), VOICES);
    }

    /// The red-zone promise: rendering allocates nothing.
    #[test]
    fn rendering_does_not_allocate() {
        let mut haze = synth();
        haze.note_on(60, 100, 1);
        let mut block = vec![0.0; 512];
        // The right-channel vec is sized on the first render, which is
        // where the one allowed allocation happens; every render after
        // it reuses the same buffer.
        haze.render(&mut block);
        assert_no_alloc::assert_no_alloc(|| {
            haze.note_on(64, 100, 2);
            haze.render(&mut block);
            haze.note_off(64);
            haze.render(&mut block);
            haze.set_param(p::CUTOFF, 800.0);
            haze.render(&mut block);
            haze.all_sound_off();
        });
    }
}
