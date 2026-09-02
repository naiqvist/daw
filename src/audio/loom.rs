//! LOOM — the wavetable synth's voice bank, the instrument half of
//! `Node::Loom`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic below
//! belongs to `src/dsp/`, and this file's job is to say which kernel
//! feeds which, and when. Read `notes/20260825-synth-brief.md` for the
//! voice architecture and the lane rule; this header carries only what
//! is NEW here.
//!
//! # What makes it a wavetable synth
//!
//! Each oscillator is TWO [`LaneOsc`]s crossfaded equal-power: the
//! MORPH position walks the whole `Waveform::ALL` table set — sine to
//! metal — picking the adjacent pair and blending between them. A scan
//! is one continuous timbre rather than a list of discrete shapes, and
//! it costs one extra oscillator per voice, which the lane-major
//! registers absorb like the two-osc design already did.
//!
//! # Two rates, on purpose
//!
//! Per SAMPLE: the envelopes and the crossfade (a multiply). Per
//! CONTROL CHUNK ([`CHUNK`] samples): oscillator frequency (glide,
//! detune), the morph pair (position letters), and the filter
//! coefficients (envelope) — every one a coefficient that costs a
//! transcendental to rebuild. [`CHUNK`] at 32 samples is 0.67 ms: a
//! glide, not a staircase.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::graph::Ramp;
use crate::dsp::adsr::LaneAdsr;
use crate::dsp::filters::{LaneSvf, Mode as FilterMode};
use crate::dsp::noise::LaneWhiteNoise;
use crate::dsp::osc::{LaneOsc, Waveform, build_tables, table_len};
use crate::dsp::{LANES, LaneFrame};
use crate::params::loom as p;

/// Voices the synth owns, before unison multiplies what one note costs.
pub const LOOM_VOICES: usize = p::VOICES;
/// Lane groups the voices are split into.
pub const GROUPS: usize = LOOM_VOICES / LANES;
/// Samples between control-rate coefficient updates. See the module doc.
const CHUNK: usize = 32;

/// Envelope floor: below this a voice is silent and reusable (-80 dB).
const ENV_FLOOR: f32 = 1e-4;

/// Every waveform's table set, in one allocation — built at compile for
/// the same reason the poly's are: a wave change is a LIVE letter, and
/// building a table set is a million sine evaluations, the far side of
/// the red-zone line.
struct WaveTables {
    data: Vec<f32>,
    offsets: [usize; Waveform::ALL.len()],
    lens: [usize; Waveform::ALL.len()],
}

impl WaveTables {
    fn build() -> Self {
        let mut offsets = [0usize; Waveform::ALL.len()];
        let mut lens = [0usize; Waveform::ALL.len()];
        let mut total = 0usize;
        for (i, w) in Waveform::ALL.iter().enumerate() {
            let len = table_len(*w);
            offsets[i] = total;
            lens[i] = len;
            total += len;
        }
        let mut data = vec![0.0f32; total];
        for (i, w) in Waveform::ALL.iter().enumerate() {
            let end = offsets[i] + lens[i];
            if let Some(dst) = data.get_mut(offsets[i]..end) {
                build_tables(*w, dst);
            }
        }
        Self {
            data,
            offsets,
            lens,
        }
    }

    fn get(&self, index: usize) -> &[f32] {
        let i = index.min(Waveform::ALL.len() - 1);
        self.data
            .get(self.offsets[i]..self.offsets[i] + self.lens[i])
            .unwrap_or(&[])
    }
}

/// Everything a `ParamChange` letter can set, in ENGINE units — the
/// `params::loom` table's own, so a letter is stored after a clamp and
/// never converted twice.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LoomParams {
    pub a_morph: f32,
    pub a_oct: f32,
    pub a_semi: f32,
    pub a_level: f32,
    pub b_morph: f32,
    pub b_oct: f32,
    pub b_semi: f32,
    pub b_level: f32,
    pub n_level: f32,
    pub n_decay: f32,
    pub f_mode: f32,
    pub f_cutoff: f32,
    pub f_res: f32,
    pub f_env: f32,
    pub amp_a: f32,
    pub amp_d: f32,
    pub amp_s: f32,
    pub amp_r: f32,
    pub gain: f32,
    pub vel: f32,
    pub fenv_a: f32,
    pub fenv_d: f32,
    pub fenv_s: f32,
    pub fenv_r: f32,
    pub v_unison: f32,
    pub v_detune: f32,
    pub v_spread: f32,
    pub v_glide: f32,
    pub penv_d: f32,
    pub penv: f32,
    pub lfo_rate: f32,
    pub lfo_pitch: f32,
}

impl Default for LoomParams {
    fn default() -> Self {
        let d = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            a_morph: d(p::A_MORPH),
            a_oct: d(p::A_OCT),
            a_semi: d(p::A_SEMI),
            a_level: d(p::A_LEVEL),
            b_morph: d(p::B_MORPH),
            b_oct: d(p::B_OCT),
            b_semi: d(p::B_SEMI),
            b_level: d(p::B_LEVEL),
            n_level: d(p::N_LEVEL),
            n_decay: d(p::N_DECAY),
            f_mode: d(p::F_MODE),
            f_cutoff: d(p::F_CUTOFF),
            f_res: d(p::F_RES),
            f_env: d(p::F_ENV),
            amp_a: d(p::AMP_A),
            amp_d: d(p::AMP_D),
            amp_s: d(p::AMP_S),
            amp_r: d(p::AMP_R),
            gain: d(p::GAIN),
            vel: d(p::VEL),
            fenv_a: d(p::FENV_A),
            fenv_d: d(p::FENV_D),
            fenv_s: d(p::FENV_S),
            fenv_r: d(p::FENV_R),
            v_unison: d(p::V_UNISON),
            v_detune: d(p::V_DETUNE),
            v_spread: d(p::V_SPREAD),
            v_glide: d(p::V_GLIDE),
            penv_d: d(p::PENV_D),
            penv: d(p::PENV),
            lfo_rate: d(p::LFO_RATE),
            lfo_pitch: d(p::LFO_PITCH),
        }
    }
}

impl LoomParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::A_MORPH => self.a_morph = value,
            p::A_OCT => self.a_oct = value,
            p::A_SEMI => self.a_semi = value,
            p::A_LEVEL => self.a_level = value,
            p::B_MORPH => self.b_morph = value,
            p::B_OCT => self.b_oct = value,
            p::B_SEMI => self.b_semi = value,
            p::B_LEVEL => self.b_level = value,
            p::N_LEVEL => self.n_level = value,
            p::N_DECAY => self.n_decay = value,
            p::F_MODE => self.f_mode = value,
            p::F_CUTOFF => self.f_cutoff = value,
            p::F_RES => self.f_res = value,
            p::F_ENV => self.f_env = value,
            p::AMP_A => self.amp_a = value,
            p::AMP_D => self.amp_d = value,
            p::AMP_S => self.amp_s = value,
            p::AMP_R => self.amp_r = value,
            p::GAIN => self.gain = value,
            p::VEL => self.vel = value,
            p::FENV_A => self.fenv_a = value,
            p::FENV_D => self.fenv_d = value,
            p::FENV_S => self.fenv_s = value,
            p::FENV_R => self.fenv_r = value,
            p::V_UNISON => self.v_unison = value,
            p::V_DETUNE => self.v_detune = value,
            p::V_SPREAD => self.v_spread = value,
            p::V_GLIDE => self.v_glide = value,
            p::PENV_D => self.penv_d = value,
            p::PENV => self.penv = value,
            p::LFO_RATE => self.lfo_rate = value,
            p::LFO_PITCH => self.lfo_pitch = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::A_MORPH => self.a_morph,
            p::A_OCT => self.a_oct,
            p::A_SEMI => self.a_semi,
            p::A_LEVEL => self.a_level,
            p::B_MORPH => self.b_morph,
            p::B_OCT => self.b_oct,
            p::B_SEMI => self.b_semi,
            p::B_LEVEL => self.b_level,
            p::N_LEVEL => self.n_level,
            p::N_DECAY => self.n_decay,
            p::F_MODE => self.f_mode,
            p::F_CUTOFF => self.f_cutoff,
            p::F_RES => self.f_res,
            p::F_ENV => self.f_env,
            p::AMP_A => self.amp_a,
            p::AMP_D => self.amp_d,
            p::AMP_S => self.amp_s,
            p::AMP_R => self.amp_r,
            p::GAIN => self.gain,
            p::VEL => self.vel,
            p::FENV_A => self.fenv_a,
            p::FENV_D => self.fenv_d,
            p::FENV_S => self.fenv_s,
            p::FENV_R => self.fenv_r,
            p::V_UNISON => self.v_unison,
            p::V_DETUNE => self.v_detune,
            p::V_SPREAD => self.v_spread,
            p::V_GLIDE => self.v_glide,
            p::PENV_D => self.penv_d,
            p::PENV => self.penv,
            p::LFO_RATE => self.lfo_rate,
            p::LFO_PITCH => self.lfo_pitch,
            _ => return None,
        })
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE.iter() {
            if let Some(value) = self.get(def.id) {
                let fixed = if value.is_finite() {
                    value
                } else {
                    def.default
                };
                self.set(def.id, def.clamp(fixed));
            }
        }
    }

    /// The morph position split into its table PAIR: `(lo index, hi
    /// index, blend 0..1 toward hi)`. The last table sits alone at the
    /// top of the position.
    pub fn morph_pair(&self, morph: f32) -> (usize, usize, f32) {
        let m = morph.clamp(0.0, 1.0);
        let span = (Waveform::ALL.len() - 1) as f32;
        let at = m * span;
        let lo = at.floor().min(span) as usize;
        let hi = (lo + 1).min(Waveform::ALL.len() - 1);
        let blend = if hi == lo { 0.0 } else { at - lo as f32 };
        (lo, hi, blend)
    }

    /// Unison voices per note, as the table stores the index.
    pub fn unison_voices(&self) -> usize {
        let i = self.v_unison.round().max(0.0) as usize;
        i.min(p::UNISON.len() - 1) + 1
    }

    /// Osc A's pitch ratio, octave + semitone in the table's own units.
    pub fn transpose_a(&self) -> f32 {
        (self.a_oct.round() as i32 - p::OCT_CENTER as i32) as f32 * 12.0 + self.a_semi
    }

    pub fn transpose_b(&self) -> f32 {
        (self.b_oct.round() as i32 - p::OCT_CENTER as i32) as f32 * 12.0 + self.b_semi
    }

    /// The filter mode, as the kernel spells it.
    pub fn filter_mode(&self) -> FilterMode {
        match self.f_mode.round().max(0.0) as usize {
            1 => FilterMode::Highpass,
            2 => FilterMode::BandpassUnity,
            3 => FilterMode::Notch,
            _ => FilterMode::Lowpass,
        }
    }
}

/// One lane group: the kernels every voice in it shares.
struct Group {
    /// Each oscillator is a PAIR — the wavetable's two neighbours at
    /// the current morph position, crossfaded equal-power.
    a_lo: LaneOsc,
    a_hi: LaneOsc,
    b_lo: LaneOsc,
    b_hi: LaneOsc,
    white: LaneWhiteNoise,
    noise_env: LaneAdsr,
    amp: LaneAdsr,
    filter_env: LaneAdsr,
    pitch_env: LaneAdsr,
    /// The internal LFO: one `LaneOsc` at a low rate, its output read
    /// per control chunk and bent into the voice frequencies.
    lfo: LaneOsc,
    filter: LaneSvf,
    /// Target frequency per lane, in Hz — where a glide is heading.
    target_hz: [f32; LANES],
    /// Current frequency per lane, in Hz — where the glide has got to.
    current_hz: [f32; LANES],
    /// Velocity as a gain, per lane.
    vel: [f32; LANES],
    /// Constant-power pan gains, per lane — where the NOTE put the voice.
    pan_l: [f32; LANES],
    pan_r: [f32; LANES],
    /// Detune in semitones, per lane — unison's spread around the note.
    detune: [f32; LANES],
}

impl Group {
    fn new() -> Self {
        Self {
            a_lo: LaneOsc::new(),
            a_hi: LaneOsc::new(),
            b_lo: LaneOsc::new(),
            b_hi: LaneOsc::new(),
            white: LaneWhiteNoise::new(),
            noise_env: LaneAdsr::new(),
            amp: LaneAdsr::new(),
            filter_env: LaneAdsr::new(),
            pitch_env: LaneAdsr::new(),
            lfo: LaneOsc::new(),
            filter: LaneSvf::new(),
            target_hz: [0.0; LANES],
            current_hz: [0.0; LANES],
            vel: [0.0; LANES],
            pan_l: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            pan_r: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            detune: [0.0; LANES],
        }
    }

    fn reset_lane(&mut self, lane: usize) {
        self.a_lo.reset_lane(lane);
        self.a_hi.reset_lane(lane);
        self.b_lo.reset_lane(lane);
        self.b_hi.reset_lane(lane);
        self.white.reset_lane(lane);
        self.noise_env.reset_lane(lane);
        self.amp.reset_lane(lane);
        self.filter_env.reset_lane(lane);
        self.pitch_env.reset_lane(lane);
        self.lfo.reset_lane(lane);
        self.filter.reset_lane(lane);
    }
}

/// Scratch the render walk needs. Sized once, at compile.
struct Scratch {
    a_lo: Vec<LaneFrame>,
    a_hi: Vec<LaneFrame>,
    b_lo: Vec<LaneFrame>,
    b_hi: Vec<LaneFrame>,
    noise: Vec<LaneFrame>,
    mix: Vec<LaneFrame>,
    amp: Vec<LaneFrame>,
    fenv: Vec<LaneFrame>,
    nenv: Vec<LaneFrame>,
    lfo: Vec<LaneFrame>,
    right: Vec<f32>,
}

/// The voice bank.
pub struct LoomVoices {
    params: LoomParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch.
    base: LoomParams,
    sample_rate: f32,
    tables: WaveTables,
    groups: [Group; GROUPS],
    scratch: Scratch,
    gate: [bool; LOOM_VOICES],
    pitch: [u8; LOOM_VOICES],
    age: [u64; LOOM_VOICES],
    note_id: [u32; LOOM_VOICES],
    next_note: u32,
    last_hz: f32,
    /// The morph PAIR each group's oscillators were last prepared for —
    /// `prepare` zeroes the increments, so it must run on a pair change,
    /// never every chunk.
    a_pair: [(usize, usize); GROUPS],
    b_pair: [(usize, usize); GROUPS],
}

impl LoomVoices {
    pub fn new(sample_rate: f32, block: usize, params: LoomParams) -> Self {
        let base = params;
        let mut voices = Self {
            params,
            base,
            sample_rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            tables: WaveTables::build(),
            groups: std::array::from_fn(|_| Group::new()),
            scratch: Scratch {
                a_lo: vec![LaneFrame::default(); block],
                a_hi: vec![LaneFrame::default(); block],
                b_lo: vec![LaneFrame::default(); block],
                b_hi: vec![LaneFrame::default(); block],
                noise: vec![LaneFrame::default(); block],
                mix: vec![[0.0; LANES]; block],
                amp: vec![[0.0; LANES]; block],
                fenv: vec![[0.0; LANES]; block],
                nenv: vec![[0.0; LANES]; block],
                lfo: vec![[0.0; LANES]; block],
                right: vec![0.0; block],
            },
            gate: [false; LOOM_VOICES],
            pitch: [0; LOOM_VOICES],
            age: [0; LOOM_VOICES],
            note_id: [0; LOOM_VOICES],
            next_note: 0,
            last_hz: 0.0,
            a_pair: [(usize::MAX, usize::MAX); GROUPS],
            b_pair: [(usize::MAX, usize::MAX); GROUPS],
        };
        voices.params.sanitize();
        voices.prepare();
        voices
    }

    /// Green zone: sample rate, envelope shapes and the table pair every
    /// oscillator starts on.
    pub fn prepare(&mut self) {
        let fs = self.sample_rate;
        for g in 0..GROUPS {
            let group = &mut self.groups[g];
            group.amp.prepare(
                fs,
                self.params.amp_a,
                self.params.amp_d,
                self.params.amp_s,
                self.params.amp_r,
            );
            group.filter_env.prepare(
                fs,
                self.params.fenv_a,
                self.params.fenv_d,
                self.params.fenv_s,
                self.params.fenv_r,
            );
            group
                .noise_env
                .prepare(fs, 0.1, self.params.n_decay, 0.0, 1.0);
            group
                .pitch_env
                .prepare(fs, 0.5, self.params.penv_d, 0.0, 1.0);
            group.lfo.prepare(fs, Waveform::Sine);
            group
                .filter
                .prepare(fs, self.params.f_cutoff, self.params.f_res / 100.0 * 10.0);
        }
    }

    /// A letter: the knob moves, and the live patch with it.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

    fn apply_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        // Coefficients that letters can move: rebuild what they feed.
        match param {
            p::AMP_A
            | p::AMP_D
            | p::AMP_S
            | p::AMP_R
            | p::FENV_A
            | p::FENV_D
            | p::FENV_S
            | p::FENV_R
            | p::N_DECAY
            | p::PENV_D => self.prepare(),
            p::A_MORPH | p::B_MORPH => {
                // The next control update sees the stale pair and
                // re-prepares the oscillators then.
                self.a_pair = [(usize::MAX, usize::MAX); GROUPS];
                self.b_pair = [(usize::MAX, usize::MAX); GROUPS];
            }
            _ => {}
        }
    }

    pub fn params(&self) -> LoomParams {
        self.params
    }

    /// A parameter LOCK at a note boundary — the wavetable synth keeps
    /// the same convention as its siblings: the lock overrides the live
    /// knob for the note, and `None` restores the knob.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let value = match value {
            Some(v) => v,
            None => match self.base.get(param) {
                Some(v) => v,
                None => return,
            },
        };
        self.apply_param(param, value);
    }

    /// The right channel the last render wrote, for the stereo node arm.
    pub fn right(&self, len: usize) -> &[f32] {
        self.scratch.right.get(..len).unwrap_or(&[])
    }

    /// Red zone. Silence everything, now — what a discontinuity demands.
    pub fn all_sound_off(&mut self) {
        for g in 0..GROUPS {
            for lane in 0..LANES {
                self.groups[g].reset_lane(lane);
            }
        }
        self.gate = [false; LOOM_VOICES];
        self.age = [0; LOOM_VOICES];
        self.note_id = [0; LOOM_VOICES];
    }

    /// Red zone. Release every gate without cutting the tails.
    pub fn release_all(&mut self) {
        for voice in 0..LOOM_VOICES {
            if self.gate[voice] {
                self.gate[voice] = false;
                self.gate_off_voice(voice);
            }
        }
    }

    fn gate_off_voice(&mut self, voice: usize) {
        let (g, lane) = (voice / LANES, voice % LANES);
        if let Some(group) = self.groups.get_mut(g) {
            group.amp.gate_off(lane);
            group.filter_env.gate_off(lane);
        }
    }

    fn sounding(&self, voice: usize) -> bool {
        let (g, lane) = (voice / LANES, voice % LANES);
        self.groups
            .get(g)
            .is_some_and(|group| group.amp.active(lane) && group.amp.current(lane) >= ENV_FLOOR)
    }

    /// Red zone. Release every voice of the OLDEST note gated on `pitch`.
    pub fn note_off(&mut self, pitch: u8) {
        let mut target = None;
        for voice in 0..LOOM_VOICES {
            if self.gate[voice] && self.pitch[voice] == pitch {
                let key = (self.age[voice], self.note_id[voice]);
                if target.is_none_or(|(a, _)| key.0 < a) {
                    target = Some(key);
                }
            }
        }
        let Some((_, note)) = target else {
            return;
        };
        for voice in 0..LOOM_VOICES {
            if self.gate[voice] && self.note_id[voice] == note {
                self.gate[voice] = false;
                self.gate_off_voice(voice);
            }
        }
    }

    /// Red zone. Start `pitch`, taking `unison_voices` voices.
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        let note = self.next_note;
        self.next_note = self.next_note.wrapping_add(1);
        let count = self.unison_count();
        let base = Self::note_hz(pitch);
        let from = if self.params.v_glide > 0.0 && self.last_hz > 0.0 {
            self.last_hz
        } else {
            base
        };
        self.last_hz = base;
        for u in 0..count {
            let voice = self.steal();
            let (g, lane) = (voice / LANES, voice % LANES);
            let (detune, pan) = self.unison_place(u, count);
            self.gate[voice] = true;
            self.pitch[voice] = pitch;
            self.age[voice] = age;
            self.note_id[voice] = note;
            let Some(group) = self.groups.get_mut(g) else {
                continue;
            };
            group.detune[lane] = detune;
            group.target_hz[lane] = base;
            group.current_hz[lane] = from;
            group.vel[lane] = f32::from(vel) / 127.0;
            let (l, r) = pan;
            group.pan_l[lane] = l;
            group.pan_r[lane] = r;
            let offset = if count > 1 {
                u as f32 / count as f32
            } else {
                0.0
            };
            group.a_lo.reset_lane(lane);
            group.a_hi.reset_lane(lane);
            group.b_lo.reset_lane(lane);
            group.b_hi.reset_lane(lane);
            group.a_lo.set_phase(lane, offset);
            group.a_hi.set_phase(lane, offset);
            group.b_lo.set_phase(lane, offset);
            group.b_hi.set_phase(lane, offset);
            group.amp.gate_on(lane);
            group.filter_env.gate_on(lane);
            group.noise_env.gate_on(lane);
            group.pitch_env.gate_on(lane);
        }
    }

    fn note_hz(pitch: u8) -> f32 {
        // 69 = A4 = 440 Hz.
        440.0 * 2.0f32.powf((f32::from(pitch) - 69.0) / 12.0)
    }

    fn unison_count(&self) -> usize {
        self.params.unison_voices().clamp(1, LOOM_VOICES)
    }

    /// Where unison voice `u` of `count` sits: detune in semitones and a
    /// constant-power pan pair, symmetric about the note.
    fn unison_place(&self, u: usize, count: usize) -> (f32, (f32, f32)) {
        if count <= 1 {
            let c = core::f32::consts::FRAC_1_SQRT_2;
            return (0.0, (c, c));
        }
        let t = (u as f32 / (count - 1) as f32) * 2.0 - 1.0;
        // `detune` is 0..100, read as cents of spread.
        let cents = t * self.params.v_detune;
        let detune = cents / 100.0;
        let spread = (self.params.v_spread / 100.0).clamp(0.0, 1.0);
        let angle = (t * spread + 1.0) * 0.5 * core::f32::consts::FRAC_PI_2;
        (detune, (angle.cos(), angle.sin()))
    }

    /// A free voice, or the best one to steal: releasing before held,
    /// oldest before newest.
    fn steal(&mut self) -> usize {
        if let Some(v) = (0..LOOM_VOICES).find(|v| !self.gate[*v] && !self.sounding(*v)) {
            return v;
        }
        let mut best = 0usize;
        let mut key = (true, u64::MAX);
        for v in 0..LOOM_VOICES {
            if (self.gate[v], self.age[v]) < key {
                key = (self.gate[v], self.age[v]);
                best = v;
            }
        }
        let (g, lane) = (best / LANES, best % LANES);
        if let Some(group) = self.groups.get_mut(g) {
            group.reset_lane(lane);
        }
        best
    }

    /// Red zone. Render `out.len()` samples of the left channel into
    /// `out`, and the right into the bank's own buffer at `at`.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        let len = out.len();
        for s in out.iter_mut() {
            *s = 0.0;
        }
        if let Some(r) = self.scratch.right.get_mut(at..at + len) {
            for s in r.iter_mut() {
                *s = 0.0;
            }
        }
        for g in 0..GROUPS {
            self.render_group(g, out, at);
        }
        for (i, s) in out.iter_mut().enumerate() {
            let g = gain.next();
            *s *= g;
            if let Some(r) = self.scratch.right.get_mut(at + i) {
                *r *= g;
            }
        }
    }

    fn render_group(&mut self, g: usize, out: &mut [f32], at: usize) {
        if !self
            .groups
            .get(g)
            .is_some_and(|group| group.amp.any_active())
        {
            return;
        }
        let params = self.params;
        let fs = self.sample_rate;
        let mut done = 0usize;
        while done < out.len() {
            let n = CHUNK.min(out.len() - done);
            self.control_update(g, params, fs, n);
            self.render_chunk(g, n);
            let Some(group) = self.groups.get(g) else {
                return;
            };
            let (pan_l, pan_r) = (group.pan_l, group.pan_r);
            let vel_amount = (params.vel / 100.0).clamp(0.0, 1.0);
            for (i, (mix, env)) in self
                .scratch
                .mix
                .iter()
                .zip(self.scratch.amp.iter())
                .take(n)
                .enumerate()
            {
                let mut acc_l = 0.0f32;
                let mut acc_r = 0.0f32;
                for ((((s, e), v), l), r) in mix
                    .iter()
                    .zip(env.iter())
                    .zip(group.vel.iter())
                    .zip(pan_l.iter())
                    .zip(pan_r.iter())
                {
                    let vel = 1.0 - vel_amount + vel_amount * *v;
                    let x = *s * *e * vel;
                    acc_l += x * *l;
                    acc_r += x * *r;
                }
                let scale = 0.25;
                if let Some(o) = out.get_mut(done + i) {
                    *o += acc_l * scale;
                }
                if let Some(r) = self.scratch.right.get_mut(at + done + i) {
                    *r += acc_r * scale;
                }
            }
            done += n;
        }
    }

    /// Control rate: glide, detune, morph pair, filter coefficients.
    fn control_update(&mut self, g: usize, params: LoomParams, fs: f32, n: usize) {
        let glide = params.v_glide.max(0.0);
        let (a_lo, a_hi, _) = params.morph_pair(params.a_morph);
        let (b_lo, b_hi, _) = params.morph_pair(params.b_morph);
        let trans_a = params.transpose_a();
        let trans_b = params.transpose_b();
        let fenv_amount = params.f_env;
        let base_cutoff = params.f_cutoff;
        let penv_semis = params.penv;
        let lfo_octaves = params.lfo_pitch;
        // The internal LFO runs one chunk ahead of the coefficients that
        // read it: its MIDPOINT is this chunk's bend, so a siren sweeps
        // rather than stair-steps. Before the group borrow below — the
        // oscillator and the scratch are two different borrows.
        {
            let group = &mut self.groups[g];
            for lane in 0..LANES {
                group.lfo.set_freq(lane, params.lfo_rate);
            }
            if let Some(buf) = self.scratch.lfo.get_mut(..n) {
                let tables = self.tables.get(0); // sine
                group.lfo.process(buf, None, tables);
            }
        }
        let lfo_mid = self.scratch.lfo.get(n / 2).copied().unwrap_or([0.0; LANES]);
        let group = &mut self.groups[g];
        // `prepare` zeroes the increments, so it runs ONLY when the
        // pair changed — a letter moved the morph knob.
        if self.a_pair[g] != (a_lo, a_hi) {
            self.a_pair[g] = (a_lo, a_hi);
            group.a_lo.prepare(fs, Waveform::ALL[a_lo]);
            group.a_hi.prepare(fs, Waveform::ALL[a_hi]);
        }
        if self.b_pair[g] != (b_lo, b_hi) {
            self.b_pair[g] = (b_lo, b_hi);
            group.b_lo.prepare(fs, Waveform::ALL[b_lo]);
            group.b_hi.prepare(fs, Waveform::ALL[b_hi]);
        }
        // Glide: one pole per control chunk toward the target.
        let glide_samples = (glide * 1e-3 * fs).max(1.0);
        let glide_coeff = crate::dsp::ramps::one_pole_coeff(CHUNK as f32 / glide_samples);
        let mut cutoffs = [0.0f32; LANES];
        for lane in 0..LANES {
            let target = group.target_hz.get(lane).copied().unwrap_or(0.0);
            let Some(cur) = group.current_hz.get_mut(lane) else {
                continue;
            };
            *cur += (target - *cur) * glide_coeff;
            let base = *cur;
            let detune = group.detune.get(lane).copied().unwrap_or(0.0);
            // The pitch envelope drops from its depth to zero over its
            // decay — the kick's clock and the snare's crack. The LFO
            // bends around that, in octaves: the dub siren's wobble.
            let penv_sweep = penv_semis * group.pitch_env.current(lane);
            let lfo_bend = lfo_octaves * lfo_mid.get(lane).copied().unwrap_or(0.0);
            let bend_a = ((trans_a + detune + penv_sweep) / 12.0 + lfo_bend).exp2();
            let bend_b = ((trans_b + detune + penv_sweep) / 12.0 + lfo_bend).exp2();
            group.a_lo.set_freq(lane, base * bend_a);
            group.a_hi.set_freq(lane, base * bend_a);
            group.b_lo.set_freq(lane, base * bend_b);
            group.b_hi.set_freq(lane, base * bend_b);
            // The filter envelope sweeps the cutoff, four octaves at full
            // depth — the knob's own scale. Its current value rides the
            // control chunk, exactly as the poly's does.
            let sweep = fenv_amount * group.filter_env.current(lane) * 4.0;
            cutoffs[lane] = (base_cutoff * sweep.exp2()).clamp(20.0, 20_000.0);
        }
        let q = crate::params::def(p::TABLE, p::F_RES).clamp(params.f_res);
        group.filter.prepare_lanes(fs, &cutoffs, q);
    }

    /// The per-sample pass: oscillators, crossfade, noise, filter. The
    /// amp envelope is applied by the summing loop in `render_group`,
    /// exactly as the poly's is.
    fn render_chunk(&mut self, g: usize, n: usize) {
        let params = self.params;
        let (a_lo, a_hi, a_blend) = params.morph_pair(params.a_morph);
        let (b_lo, b_hi, b_blend) = params.morph_pair(params.b_morph);
        let tables_a_lo = self.tables.get(a_lo);
        let tables_a_hi = self.tables.get(a_hi);
        let tables_b_lo = self.tables.get(b_lo);
        let tables_b_hi = self.tables.get(b_hi);
        // Equal-power: at blend zero the lower table alone, at one the
        // upper — and the middle is constant power, not a dip.
        let a_pair = ((a_blend * core::f32::consts::FRAC_PI_2).sin_cos());
        let b_pair = ((b_blend * core::f32::consts::FRAC_PI_2).sin_cos());
        let a_level = params.a_level / 100.0;
        let b_level = params.b_level / 100.0;
        let n_level = params.n_level / 100.0;
        let group = &mut self.groups[g];
        group
            .a_lo
            .process(&mut self.scratch.a_lo[..n], None, tables_a_lo);
        group
            .a_hi
            .process(&mut self.scratch.a_hi[..n], None, tables_a_hi);
        group
            .b_lo
            .process(&mut self.scratch.b_lo[..n], None, tables_b_lo);
        group
            .b_hi
            .process(&mut self.scratch.b_hi[..n], None, tables_b_hi);
        group.white.process(&mut self.scratch.noise[..n]);
        // All three envelopes advance every sample, whether or not this
        // chunk reads them — an envelope that only moves when something
        // listens is an envelope whose shape depends on the patch.
        group.amp.process(&mut self.scratch.amp[..n]);
        group.filter_env.process(&mut self.scratch.fenv[..n]);
        group.noise_env.process(&mut self.scratch.nenv[..n]);
        for i in 0..n {
            let a =
                (self.scratch.a_lo[i][0] * a_pair.1 + self.scratch.a_hi[i][0] * a_pair.0) * a_level;
            let b =
                (self.scratch.b_lo[i][0] * b_pair.1 + self.scratch.b_hi[i][0] * b_pair.0) * b_level;
            let noise = self.scratch.noise[i][0] * self.scratch.nenv[i][0] * n_level;
            let m = (a + b + noise) * 0.5;
            self.scratch.mix[i] = [m; LANES];
        }
        let mode = params.filter_mode();
        group.filter.process(&mut self.scratch.mix[..n], mode);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn bank(edit: impl Fn(&mut LoomParams)) -> LoomVoices {
        let mut params = LoomParams::default();
        edit(&mut params);
        let mut voices = LoomVoices::new(FS, 256, params);
        voices
    }

    /// The morph pair walks the table set exactly: 0 is sine, 1 is the
    /// last table, midpoints blend neighbours.
    #[test]
    fn the_morph_pair_walks_the_whole_table_set() {
        let params = LoomParams::default();
        let (lo, hi, blend) = params.morph_pair(0.0);
        assert_eq!((lo, hi, blend), (0, 1, 0.0), "bottom is the first pair");
        let last = Waveform::ALL.len() - 1;
        let (lo, hi, blend) = params.morph_pair(1.0);
        assert_eq!(
            (lo, hi, blend),
            (last, last, 0.0),
            "top parks on the last table"
        );
        let (lo, hi, blend) = params.morph_pair(0.5);
        assert!(
            hi == lo + 1 && (blend - 0.0).abs() < 1.0,
            "mid position is a pair with a blend"
        );
    }

    /// The default patch makes sound: gate a note and the bank renders a
    /// non-silent, finite, bounded block.
    #[test]
    fn the_default_patch_sounds() {
        let mut voices = bank(|_| {});
        voices.note_on(69, 100, 0);
        let mut out = vec![0.0f32; 256];
        let mut gain = Ramp::across(1.0, 1.0, out.len());
        voices.render(&mut out, 0, &mut gain);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 1e-3, "a gated note must make sound: {peak}");
        assert!(out.iter().all(|s| s.is_finite()), "nothing may run away");
        assert!(peak < 4.0, "the mix headroom holds: {peak}");
    }

    /// Every table position renders: a scan from sine to metal stays
    /// finite at every stop — the wavetable's whole promise.
    #[test]
    fn every_morph_position_stays_finite() {
        for stop in 0..=8 {
            let m = stop as f32 / 8.0;
            let mut voices = bank(|p| {
                p.a_morph = m;
                p.b_morph = m;
            });
            voices.note_on(57, 100, 0);
            let mut out = vec![0.0f32; 256];
            let mut gain = Ramp::across(1.0, 1.0, out.len());
            voices.render(&mut out, 0, &mut gain);
            assert!(out.iter().all(|s| s.is_finite()), "morph {m} ran away");
        }
    }

    /// Note-off releases the gate and the tail decays to silence.
    #[test]
    fn a_released_note_decays_to_silence() {
        let mut voices = bank(|p| {
            p.amp_r = 50.0;
        });
        voices.note_on(69, 100, 0);
        let mut out = vec![0.0f32; 256];
        let mut gain = Ramp::across(1.0, 1.0, out.len());
        voices.render(&mut out, 0, &mut gain);
        voices.note_off(69);
        let mut tail = vec![0.0f32; 4_096];
        let mut gain = Ramp::across(1.0, 1.0, out.len());
        voices.render(&mut tail, 0, &mut gain);
        let end_peak = tail[tail.len() - 64..]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(end_peak < 1e-4, "the release tail must end: {end_peak}");
    }
}
