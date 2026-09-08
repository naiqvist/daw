//! QUAD — the four-operator FM workhorse.
//!
//! Four operators, each with a ratio to the note or a fixed frequency,
//! a fine detune, a level and its own ADSR, a shape, a velocity depth
//! and a key scaling; eight routing shapes from a single serial stack
//! to four carriers side by side; signed feedback on any operator. Two
//! pitch envelopes — one that bends every operator, the DX pitch
//! envelope, and one that bends only the modulators, which sweeps the
//! harmonicity while the fundamental stays put. Two LFOs per voice,
//! each with a delay and a fade-in, to pitch, the modulators' levels,
//! the carriers' levels and the filter. Unison stacks up to four
//! detuned copies of the whole operator stack across the field; mono
//! and legato modes with glide make it a bass. Then a multi-mode
//! filter with its own attack-decay envelope and keytrack, per voice,
//! and after the sum, a distortion stage.
//!
//! Phase modulation, as every FM synth since the DX7 actually is: a
//! modulator's output is added to its carrier's phase, scaled by an
//! index, so a level knob is a brightness knob. The waves are table
//! reads, not a `sin` per operator per voice per sample.

use crate::dsp::filters::{Mode as FilterMode, Svf};
use crate::dsp::interp::linear;
use crate::dsp::shaper::{Mode as ShapeMode, Oversampler2x, Waveshaper};
use crate::params::quad as p;
use crate::params::quad::{Algorithm, LFOS, OPS, UNISON_MAX};

pub const VOICES: usize = 8;
const TABLE: usize = 2048;
const HOP: usize = 32;
/// A modulator at full level swings its carrier's phase by this many
/// turns: an index of about nine radians, which is where a DX7 starts
/// to get harsh.
const INDEX_TURNS: f32 = 1.5;
/// Feedback's own index: a full turn is a saw already.
const FEEDBACK_TURNS: f32 = 0.5;
/// The most keys mono mode remembers as held.
const HELD: usize = 16;

/// One operator's knobs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Op {
    pub ratio: f32,
    pub fine: f32,
    pub level: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// The shape, an index into `WAVE_NAMES`.
    pub wave: f32,
    /// Whether the operator sits at `hz` rather than at `ratio` times
    /// the note.
    pub fixed: f32,
    pub hz: f32,
    /// How much of the key's velocity reaches this operator's level.
    pub vel: f32,
    /// Level scaling across the keyboard, in dB per octave from C4.
    pub keyscale: f32,
    /// The envelope's further stages: a wait before the attack, a first
    /// decay to the break level, a second to the sustain, and how the
    /// decays bend.
    pub delay: f32,
    pub break_level: f32,
    pub decay2: f32,
    pub curve: f32,
}

impl Default for Op {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            fine: 0.0,
            level: 0.0,
            attack: 1.0,
            decay: 400.0,
            sustain: 0.3,
            release: 250.0,
            wave: 0.0,
            fixed: 0.0,
            hz: 440.0,
            vel: 0.5,
            keyscale: 0.0,
            delay: 0.0,
            break_level: 1.0,
            decay2: 400.0,
            curve: 0.0,
        }
    }
}

/// An operator's envelope: delay, attack, a decay to the break level,
/// a second decay to the sustain, the sustain, and the release — the
/// stages a DX has, with a curve for the decays. Times in samples, the
/// key's rate scaling already applied.
#[derive(Debug, Clone, Copy)]
pub struct OpEnv {
    stage: EnvStage,
    /// Samples spent in the stage.
    at: u32,
    /// The stage's length in samples, and the level it started from.
    length: u32,
    from: f32,
    value: f32,
    delay: u32,
    attack: u32,
    decay: u32,
    decay2: u32,
    release: u32,
    break_level: f32,
    sustain: f32,
    curve: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvStage {
    Idle,
    Delay,
    Attack,
    Decay,
    Decay2,
    Sustain,
    Release,
}

impl OpEnv {
    fn new() -> Self {
        Self {
            stage: EnvStage::Idle,
            at: 0,
            length: 0,
            from: 0.0,
            value: 0.0,
            delay: 0,
            attack: 0,
            decay: 1,
            decay2: 1,
            release: 1,
            break_level: 1.0,
            sustain: 0.0,
            curve: 0.0,
        }
    }

    /// Green zone: the stages' lengths, `rate` scaling the decays and
    /// the release as the key asks.
    fn prepare(&mut self, sample_rate: f32, op: &Op, rate: f32) {
        let samples = |ms: f32| ((ms.max(0.0) / 1000.0 * sample_rate) as u32).max(1);
        self.delay = if op.delay > 0.0 { samples(op.delay) } else { 0 };
        self.attack = if op.attack > 0.0 {
            samples(op.attack)
        } else {
            0
        };
        self.decay = samples(op.decay * rate);
        self.decay2 = samples(op.decay2 * rate);
        self.release = samples(op.release * rate);
        self.break_level = op.break_level.clamp(0.0, 1.0);
        self.sustain = op.sustain.clamp(0.0, 1.0);
        self.curve = op.curve.clamp(-1.0, 1.0);
    }

    fn enter(&mut self, stage: EnvStage) {
        self.stage = stage;
        self.at = 0;
        self.from = self.value;
        self.length = match stage {
            EnvStage::Delay => self.delay,
            EnvStage::Attack => self.attack,
            EnvStage::Decay => self.decay,
            EnvStage::Decay2 => self.decay2,
            EnvStage::Release => self.release,
            EnvStage::Idle | EnvStage::Sustain => 0,
        };
    }

    /// Whether the break is in play: at the top it is no break at all,
    /// and the one decay runs to the sustain — the ADSR the knobs read
    /// as until BREAK is pulled down.
    fn broken(&self) -> bool {
        self.break_level < 0.999
    }

    /// The stage's target, and whether it bends.
    fn target(&self) -> (f32, bool) {
        match self.stage {
            EnvStage::Delay => (0.0, false),
            EnvStage::Attack => (1.0, false),
            EnvStage::Decay => (
                if self.broken() {
                    self.break_level
                } else {
                    self.sustain
                },
                true,
            ),
            EnvStage::Decay2 => (self.sustain, true),
            EnvStage::Sustain => (self.sustain, false),
            EnvStage::Release | EnvStage::Idle => (0.0, true),
        }
    }

    fn next_stage(&self) -> EnvStage {
        match self.stage {
            EnvStage::Delay => EnvStage::Attack,
            EnvStage::Attack => EnvStage::Decay,
            EnvStage::Decay => {
                if self.broken() {
                    EnvStage::Decay2
                } else {
                    EnvStage::Sustain
                }
            }
            EnvStage::Decay2 | EnvStage::Sustain => EnvStage::Sustain,
            EnvStage::Release | EnvStage::Idle => EnvStage::Idle,
        }
    }

    pub fn gate_on(&mut self) {
        self.value = 0.0;
        self.enter(if self.delay > 0 {
            EnvStage::Delay
        } else {
            EnvStage::Attack
        });
        // An instant stage is stepped over at once.
        while self.length == 0 && !matches!(self.stage, EnvStage::Sustain | EnvStage::Idle) {
            self.value = self.target().0;
            self.enter(self.next_stage());
        }
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.enter(EnvStage::Release);
        }
    }

    pub fn reset(&mut self) {
        self.stage = EnvStage::Idle;
        self.value = 0.0;
        self.at = 0;
    }

    pub fn active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    pub fn stage(&self) -> EnvStage {
        self.stage
    }

    /// Red zone: the next `out.len()` values.
    pub fn process(&mut self, out: &mut [f32]) {
        for slot in out.iter_mut() {
            match self.stage {
                EnvStage::Idle => self.value = 0.0,
                EnvStage::Sustain => self.value = self.sustain,
                _ => {
                    let (target, bent) = self.target();
                    self.at += 1;
                    let p = self.at as f32 / self.length.max(1) as f32;
                    let p = if bent { p::bend(self.curve, p) } else { p };
                    self.value = self.from + (target - self.from) * p.min(1.0);
                    if self.at >= self.length {
                        self.value = target;
                        let next = self.next_stage();
                        self.enter(next);
                    }
                }
            }
            *slot = self.value;
        }
    }
}

/// One LFO's knobs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Lfo {
    pub rate: f32,
    pub shape: f32,
    pub delay: f32,
    pub fade: f32,
    pub pitch: f32,
    pub modulators: f32,
    pub amp: f32,
    pub filter: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            rate: 5.0,
            shape: 0.0,
            delay: 0.0,
            fade: 0.0,
            pitch: 0.0,
            modulators: 0.0,
            amp: 0.0,
            filter: 0.0,
        }
    }
}

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct QuadParams {
    pub ops: [Op; OPS],
    pub algo: f32,
    pub feedback: f32,
    pub pitch1: f32,
    pub p1_rise: f32,
    pub p1_fall: f32,
    pub pitch2: f32,
    pub p2_rise: f32,
    pub p2_fall: f32,
    pub fmode: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub fenv: f32,
    pub fenv_att: f32,
    pub fenv_dec: f32,
    pub keytrack: f32,
    pub dist: f32,
    pub drive: f32,
    pub velocity: f32,
    pub level: f32,
    /// How much the envelopes' decay and release shorten up the keyboard.
    pub key_rate: f32,
    pub unison: f32,
    pub udetune: f32,
    pub width: f32,
    pub mono: f32,
    pub glide: f32,
    pub fb_op: f32,
    pub lfos: [Lfo; LFOS],
    /// `matrix[from][to]`: how much of `from` reaches `to`'s phase.
    pub matrix: [[f32; OPS]; OPS],
    /// Each operator's level to the output, in matrix mode.
    pub out: [f32; OPS],
    pub env_loop: f32,
    pub oversample: f32,
}

impl Default for QuadParams {
    fn default() -> Self {
        let mut me = Self {
            ops: [Op::default(); OPS],
            algo: 0.0,
            feedback: 0.0,
            pitch1: 0.0,
            p1_rise: 0.0,
            p1_fall: 200.0,
            pitch2: 0.0,
            p2_rise: 0.0,
            p2_fall: 200.0,
            fmode: 0.0,
            cutoff: 12_000.0,
            reso: 0.8,
            fenv: 0.0,
            fenv_att: 0.0,
            fenv_dec: 300.0,
            keytrack: 0.5,
            dist: 0.0,
            drive: 1.0,
            velocity: 0.6,
            level: 0.8,
            key_rate: 0.3,
            unison: 1.0,
            udetune: 10.0,
            width: 0.5,
            mono: 0.0,
            glide: 0.0,
            fb_op: 3.0,
            lfos: [Lfo::default(); LFOS],
            matrix: [[0.0; OPS]; OPS],
            out: [0.0; OPS],
            env_loop: 0.0,
            oversample: 0.0,
        };
        for def in p::TABLE {
            me.set(def.id, def.default);
        }
        me
    }
}

impl QuadParams {
    fn lfo_of(param: u32) -> Option<(usize, u32)> {
        if param < p::LFO1_RATE {
            return None;
        }
        let rel = param - p::LFO1_RATE;
        let lfo = (rel / p::PER_LFO) as usize;
        (lfo < LFOS).then_some((lfo, rel % p::PER_LFO))
    }

    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        if let Some((op, field)) = p::op_of(param) {
            let Some(op) = self.ops.get_mut(op) else {
                return;
            };
            match field {
                p::RATIO => op.ratio = value,
                p::FINE => op.fine = value,
                p::LEVEL_OP => op.level = value,
                p::ATTACK => op.attack = value,
                p::DECAY => op.decay = value,
                p::SUSTAIN => op.sustain = value,
                p::RELEASE => op.release = value,
                p::WAVE => op.wave = value,
                p::FIXED => op.fixed = value,
                p::HZ => op.hz = value,
                p::VEL => op.vel = value,
                p::KEYSCALE => op.keyscale = value,
                p::DELAY => op.delay = value,
                p::BREAK => op.break_level = value,
                p::DECAY2 => op.decay2 = value,
                p::CURVE => op.curve = value,
                _ => {}
            }
            return;
        }
        if let Some((from, to)) = p::matrix_of(param) {
            if let Some(cell) = self.matrix.get_mut(from).and_then(|row| row.get_mut(to)) {
                *cell = value;
            }
            return;
        }
        if let Some(op) = p::out_of(param) {
            if let Some(slot) = self.out.get_mut(op) {
                *slot = value;
            }
            return;
        }
        if let Some((lfo, field)) = Self::lfo_of(param) {
            let Some(lfo) = self.lfos.get_mut(lfo) else {
                return;
            };
            match field {
                p::LFO_RATE => lfo.rate = value,
                p::LFO_SHAPE => lfo.shape = value,
                p::LFO_DELAY => lfo.delay = value,
                p::LFO_FADE => lfo.fade = value,
                p::LFO_PITCH => lfo.pitch = value,
                p::LFO_MOD => lfo.modulators = value,
                p::LFO_AMP => lfo.amp = value,
                p::LFO_FILTER => lfo.filter = value,
                _ => {}
            }
            return;
        }
        match param {
            p::ALGO => self.algo = value,
            p::FEEDBACK => self.feedback = value,
            p::PITCH1 => self.pitch1 = value,
            p::PITCH1_RISE => self.p1_rise = value,
            p::PITCH1_FALL => self.p1_fall = value,
            p::PITCH2 => self.pitch2 = value,
            p::PITCH2_RISE => self.p2_rise = value,
            p::PITCH2_FALL => self.p2_fall = value,
            p::FMODE => self.fmode = value,
            p::CUTOFF => self.cutoff = value,
            p::RESO => self.reso = value,
            p::FENV => self.fenv = value,
            p::FENV_ATT => self.fenv_att = value,
            p::FENV_DEC => self.fenv_dec = value,
            p::KEYTRACK => self.keytrack = value,
            p::DIST => self.dist = value,
            p::DRIVE => self.drive = value,
            p::VELOCITY => self.velocity = value,
            p::LEVEL => self.level = value,
            p::KEY_RATE => self.key_rate = value,
            p::UNISON => self.unison = value,
            p::UDETUNE => self.udetune = value,
            p::WIDTH => self.width = value,
            p::MONO => self.mono = value,
            p::GLIDE => self.glide = value,
            p::FB_OP => self.fb_op = value,
            p::ENV_LOOP => self.env_loop = value,
            p::OVERSAMPLE => self.oversample = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        if let Some((op, field)) = p::op_of(param) {
            let op = self.ops.get(op)?;
            return Some(match field {
                p::RATIO => op.ratio,
                p::FINE => op.fine,
                p::LEVEL_OP => op.level,
                p::ATTACK => op.attack,
                p::DECAY => op.decay,
                p::SUSTAIN => op.sustain,
                p::RELEASE => op.release,
                p::WAVE => op.wave,
                p::FIXED => op.fixed,
                p::HZ => op.hz,
                p::VEL => op.vel,
                p::KEYSCALE => op.keyscale,
                p::DELAY => op.delay,
                p::BREAK => op.break_level,
                p::DECAY2 => op.decay2,
                p::CURVE => op.curve,
                _ => return None,
            });
        }
        if let Some((from, to)) = p::matrix_of(param) {
            return self.matrix.get(from).and_then(|row| row.get(to)).copied();
        }
        if let Some(op) = p::out_of(param) {
            return self.out.get(op).copied();
        }
        if let Some((lfo, field)) = Self::lfo_of(param) {
            let lfo = self.lfos.get(lfo)?;
            return Some(match field {
                p::LFO_RATE => lfo.rate,
                p::LFO_SHAPE => lfo.shape,
                p::LFO_DELAY => lfo.delay,
                p::LFO_FADE => lfo.fade,
                p::LFO_PITCH => lfo.pitch,
                p::LFO_MOD => lfo.modulators,
                p::LFO_AMP => lfo.amp,
                p::LFO_FILTER => lfo.filter,
                _ => return None,
            });
        }
        Some(match param {
            p::ALGO => self.algo,
            p::FEEDBACK => self.feedback,
            p::PITCH1 => self.pitch1,
            p::PITCH1_RISE => self.p1_rise,
            p::PITCH1_FALL => self.p1_fall,
            p::PITCH2 => self.pitch2,
            p::PITCH2_RISE => self.p2_rise,
            p::PITCH2_FALL => self.p2_fall,
            p::FMODE => self.fmode,
            p::CUTOFF => self.cutoff,
            p::RESO => self.reso,
            p::FENV => self.fenv,
            p::FENV_ATT => self.fenv_att,
            p::FENV_DEC => self.fenv_dec,
            p::KEYTRACK => self.keytrack,
            p::DIST => self.dist,
            p::DRIVE => self.drive,
            p::VELOCITY => self.velocity,
            p::LEVEL => self.level,
            p::KEY_RATE => self.key_rate,
            p::UNISON => self.unison,
            p::UDETUNE => self.udetune,
            p::WIDTH => self.width,
            p::MONO => self.mono,
            p::GLIDE => self.glide,
            p::FB_OP => self.fb_op,
            p::ENV_LOOP => self.env_loop,
            p::OVERSAMPLE => self.oversample,
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

    pub fn algorithm(&self) -> Algorithm {
        p::algorithm(self.algo)
    }

    /// How many copies of the stack a note plays.
    pub fn unison_count(&self) -> usize {
        (self.unison.round().max(1.0) as usize).min(UNISON_MAX)
    }

    pub fn is_mono(&self) -> bool {
        self.mono.round() >= p::MONO_MONO
    }

    pub fn is_legato(&self) -> bool {
        self.mono.round() >= p::MONO_LEGATO
    }

    pub fn feedback_op(&self) -> usize {
        (self.fb_op.round().max(0.0) as usize).min(OPS - 1)
    }

    pub fn is_matrix(&self) -> bool {
        p::is_matrix(self.algo)
    }

    /// Whether operator `op` reaches the output: a carrier of the
    /// shape, or an operator with an OUT level in the matrix.
    pub fn is_heard(&self, op: usize) -> bool {
        if self.is_matrix() {
            self.out.get(op).is_some_and(|level| *level > 0.001)
        } else {
            self.algorithm().carriers.contains(&op)
        }
    }

    /// Whether operator `op` modulates another: the second pitch
    /// envelope's and the LFOs' notion of a modulator.
    pub fn is_modulating(&self, op: usize) -> bool {
        if self.is_matrix() {
            self.matrix.get(op).is_some_and(|row| {
                row.iter()
                    .enumerate()
                    .any(|(to, amount)| to != op && *amount > 0.001)
            })
        } else {
            p::is_modulator(self.algorithm(), op)
        }
    }

    pub fn oversampled(&self) -> bool {
        self.oversample.round() >= 1.0
    }
}

/// A rise-then-fall envelope in seconds: linear up over `rise`, then an
/// exponential fall that is gone after `fall`.
#[inline(always)]
fn rise_fall(t: f32, rise_ms: f32, fall_ms: f32) -> f32 {
    let rise = rise_ms.max(0.0) / 1000.0;
    let fall = fall_ms.max(1.0) / 1000.0;
    if t < rise {
        (t / rise.max(1.0e-6)).clamp(0.0, 1.0)
    } else {
        // Five time constants to the floor: the fall knob is the time it
        // takes to be gone, not the time constant.
        (-(t - rise) / (fall / 5.0)).exp()
    }
}

/// The unison copies' detune positions, `-1..=1`, for `count` copies.
fn unison_positions(count: usize) -> [f32; UNISON_MAX] {
    let mut out = [0.0f32; UNISON_MAX];
    if count <= 1 {
        return out;
    }
    for (u, slot) in out.iter_mut().enumerate().take(count) {
        *slot = u as f32 / (count - 1) as f32 * 2.0 - 1.0;
    }
    out
}

struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    age: u64,
    vel: f32,
    elapsed: u64,
    base_hz: f32,
    /// Semitones still to glide, decaying to nothing.
    glide_semis: f32,
    phase: [[f32; OPS]; UNISON_MAX],
    out: [[f32; OPS]; UNISON_MAX],
    env: [OpEnv; OPS],
    filter_l: Svf,
    filter_r: Svf,
    /// Each operator's level for this key: velocity depth and key
    /// scaling, worked out once at the note.
    key_gain: [f32; OPS],
    /// The noise wave's generator, one per voice.
    noise: u32,
    lfo_phase: [f32; LFOS],
    /// The sample-and-hold's held value, per LFO.
    lfo_hold: [f32; LFOS],
    /// The halfband pair for 2x, one per side.
    over_l: Oversampler2x,
    over_r: Oversampler2x,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            held: false,
            pitch: 0,
            age: 0,
            vel: 1.0,
            elapsed: 0,
            base_hz: 440.0,
            glide_semis: 0.0,
            phase: [[0.0; OPS]; UNISON_MAX],
            out: [[0.0; OPS]; UNISON_MAX],
            env: [OpEnv::new(), OpEnv::new(), OpEnv::new(), OpEnv::new()],
            filter_l: Svf::new(),
            filter_r: Svf::new(),
            key_gain: [1.0; OPS],
            noise: 0x9E37_79B9,
            lfo_phase: [0.0; LFOS],
            lfo_hold: [0.0; LFOS],
            over_l: Oversampler2x::new(),
            over_r: Oversampler2x::new(),
        }
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.noise;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.noise = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// The LFO's value for the coming hop, `-1..=1`, with its delay and
    /// fade applied, and its phase advanced by `hop_seconds`.
    fn lfo(&mut self, i: usize, lfo: &Lfo, t: f32, hop_seconds: f32) -> f32 {
        let delay = lfo.delay.max(0.0) / 1000.0;
        let fade = lfo.fade.max(0.0) / 1000.0;
        let depth = if t < delay {
            0.0
        } else if fade > 0.0 {
            ((t - delay) / fade).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let phase = self.lfo_phase[i];
        let value = match lfo.shape.round() as u8 {
            1 => 1.0 - 4.0 * (phase - 0.5).abs(),
            2 => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            3 => self.lfo_hold[i],
            _ => (phase * core::f32::consts::TAU).sin(),
        };
        let next = phase + lfo.rate.max(0.0) * hop_seconds;
        if next >= 1.0 {
            self.lfo_hold[i] = self.rand();
        }
        self.lfo_phase[i] = next.fract();
        value * depth
    }
}

/// The instrument.
pub struct QuadVoices {
    params: QuadParams,
    base: QuadParams,
    sample_rate: f32,
    /// The shapes, one table each, in `WAVE_NAMES` order but for noise.
    waves: [Vec<f32>; 4],
    voices: Vec<Voice>,
    /// Mono mode's memory of the keys held, oldest first.
    held: [u8; HELD],
    held_len: usize,
    /// Scratch: one voice's block, both sides, its operator envelopes,
    /// the sums.
    block_l: Vec<f32>,
    block_r: Vec<f32>,
    /// The operators' run at twice the rate, before the halfband.
    up_l: Vec<f32>,
    up_r: Vec<f32>,
    envs: [Vec<f32>; OPS],
    left: Vec<f32>,
    right: Vec<f32>,
    stash: Vec<f32>,
    shaper: Waveshaper,
    said_voices: f32,
}

#[inline(always)]
fn sine_at(table: &[f32], turns: f32) -> f32 {
    let t = turns - turns.floor();
    let pos = t * TABLE as f32;
    let i = (pos as usize).min(TABLE - 1);
    linear(
        table.get(i).copied().unwrap_or(0.0),
        table.get(i + 1).copied().unwrap_or(0.0),
        pos - i as f32,
    )
}

impl QuadVoices {
    pub fn new(sample_rate: f32, block: usize, params: QuadParams) -> Self {
        let mut waves: [Vec<f32>; 4] = [
            vec![0.0f32; TABLE + 1],
            vec![0.0f32; TABLE + 1],
            vec![0.0f32; TABLE + 1],
            vec![0.0f32; TABLE + 1],
        ];
        for i in 0..=TABLE {
            let t = i as f32 / TABLE as f32 * core::f32::consts::TAU;
            let sine = t.sin();
            waves[0][i] = sine;
            // The half sine: the positive lobe, then rest.
            waves[1][i] = sine.max(0.0);
            // The rectified sine, centred: every lobe up, then the DC out.
            waves[2][i] = sine.abs() * 2.0 - 4.0 / core::f32::consts::PI;
            // A soft square: the sine leaned on, not a step.
            waves[3][i] = (sine * 4.0).tanh() / 4.0f32.tanh();
        }
        let n = block.max(HOP);
        let mut me = Self {
            params,
            base: params,
            sample_rate: 48_000.0,
            waves,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            held: [0; HELD],
            held_len: 0,
            block_l: vec![0.0; n],
            block_r: vec![0.0; n],
            up_l: vec![0.0; HOP * 2],
            up_r: vec![0.0; HOP * 2],
            envs: [vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            left: vec![0.0; n],
            right: vec![0.0; n],
            stash: vec![0.0; n],
            shaper: Waveshaper::new(),
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.prepare_at(sample_rate);
        me
    }

    pub fn prepare_at(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.settle();
    }

    fn settle(&mut self) {
        let sr = self.sample_rate;
        let p = self.params;
        for v in self.voices.iter_mut() {
            for (env, op) in v.env.iter_mut().zip(p.ops.iter()) {
                env.prepare(sr, op, 1.0);
            }
            v.over_l.prepare();
            v.over_r.prepare();
        }
        let mode = match p.dist.round() as u8 {
            2 => ShapeMode::HardClip,
            3 => ShapeMode::Fold,
            _ => ShapeMode::SoftClip,
        };
        self.shaper.configure(mode, p.drive, 0.0, 1.0);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.base.set(param, value);
        self.settle();
    }

    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        match value {
            Some(v) => self.params.set(param, v),
            None => {
                if let Some(v) = self.base.get(param) {
                    self.params.set(param, v);
                }
            }
        }
        self.settle();
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        if let (Some(live), Some(base)) = (self.params.get(param), self.base.get(param)) {
            self.params
                .set(param, live + (base - live) * alpha.clamp(0.0, 1.0));
            self.settle();
        }
    }

    pub fn params(&self) -> &QuadParams {
        &self.params
    }

    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [self.said_voices, 0.0, 0.0],
        }
    }

    pub fn right(&self, len: usize) -> &[f32] {
        self.stash.get(..len.min(self.stash.len())).unwrap_or(&[])
    }

    pub fn all_sound_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
            v.held = false;
            for env in v.env.iter_mut() {
                env.reset();
            }
            v.filter_l.reset();
            v.filter_r.reset();
            v.out = [[0.0; OPS]; UNISON_MAX];
        }
        self.held_len = 0;
        self.said_voices = 0.0;
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            if v.held {
                v.held = false;
                for env in v.env.iter_mut() {
                    env.gate_off();
                }
            }
        }
        self.held_len = 0;
    }

    fn forget_held(&mut self, pitch: u8) {
        let mut i = 0;
        while i < self.held_len {
            if self.held[i] == pitch {
                for j in i..self.held_len.saturating_sub(1) {
                    self.held[j] = self.held[j + 1];
                }
                self.held_len -= 1;
            } else {
                i += 1;
            }
        }
    }

    fn remember_held(&mut self, pitch: u8) {
        self.forget_held(pitch);
        if self.held_len == HELD {
            for j in 0..HELD - 1 {
                self.held[j] = self.held[j + 1];
            }
            self.held_len -= 1;
        }
        self.held[self.held_len] = pitch;
        self.held_len += 1;
    }

    pub fn note_off(&mut self, pitch: u8) {
        self.forget_held(pitch);
        if self.params.is_mono() {
            // A key still held takes the voice back, legato.
            if self.held_len > 0 {
                let back = self.held[self.held_len - 1];
                if let Some(v) = self.voices.iter_mut().find(|v| v.active && v.held) {
                    if v.pitch == pitch {
                        let from = v.pitch;
                        v.pitch = back;
                        v.base_hz = 440.0 * ((f32::from(back) - 69.0) / 12.0).exp2();
                        v.glide_semis += f32::from(from) - f32::from(back);
                    }
                    return;
                }
            }
        }
        for v in self.voices.iter_mut() {
            if v.held && v.pitch == pitch {
                v.held = false;
                for env in v.env.iter_mut() {
                    env.gate_off();
                }
            }
        }
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        let mono = self.params.is_mono();
        let legato = self.params.is_legato();
        if mono {
            self.remember_held(pitch);
        }
        // Mono: the one sounding voice takes the new key, gliding from
        // where it was; legato does not even restart the envelopes.
        let sounding = mono
            .then(|| self.voices.iter().position(|v| v.active))
            .flatten();
        let index = match sounding {
            Some(index) => index,
            None => {
                let slot = self.voices.iter().position(|v| !v.active).or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, v)| v.age)
                        .map(|(i, _)| i)
                });
                let Some(index) = slot else {
                    return;
                };
                index
            }
        };
        let glide_on = self.params.glide > 0.0;
        let octaves = (f32::from(pitch) - 60.0) / 12.0;
        let rate = (-octaves * self.params.key_rate).exp2().clamp(0.125, 8.0);
        let params = self.params;
        let sr = self.sample_rate;
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        let retrigger = !(sounding.is_some() && legato);
        if sounding.is_some() && glide_on {
            v.glide_semis += f32::from(v.pitch) - f32::from(pitch);
        } else {
            v.glide_semis = 0.0;
        }
        v.active = true;
        v.held = true;
        v.pitch = pitch;
        v.age = age;
        v.vel = (f32::from(vel) / 127.0).clamp(0.0, 1.0);
        v.base_hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        // The key's own levels and rates: velocity depth and dB-per-octave
        // scaling per operator, and every envelope's decay and release
        // shortened up the keyboard by KEY RATE.
        for (k, op) in params.ops.iter().enumerate() {
            let touch = 1.0 - op.vel + op.vel * v.vel;
            let scaled = (op.keyscale * octaves / 20.0 * core::f32::consts::LN_10).exp();
            v.key_gain[k] = (touch * scaled).clamp(0.0, 4.0);
            v.env[k].prepare(sr, op, rate);
        }
        if retrigger {
            v.elapsed = 0;
            v.phase = [[0.0; OPS]; UNISON_MAX];
            v.out = [[0.0; OPS]; UNISON_MAX];
            v.lfo_phase = [0.0; LFOS];
            v.filter_l.reset();
            v.filter_r.reset();
            for env in v.env.iter_mut() {
                env.gate_on();
            }
        }
    }

    /// Red zone: render the LEFT channel, stash the right. Any length.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut crate::audio::graph::Ramp) {
        let cap = self.left.len().max(1);
        let mut from = 0usize;
        while from < out.len() {
            let take = (out.len() - from).min(cap);
            let Some(block) = out.get_mut(from..from + take) else {
                break;
            };
            self.render_block(block, at + from, gain);
            from += take;
        }
    }

    fn render_block(&mut self, out: &mut [f32], at: usize, gain: &mut crate::audio::graph::Ramp) {
        let n = out.len();
        for slot in self
            .left
            .iter_mut()
            .take(n)
            .chain(self.right.iter_mut().take(n))
        {
            *slot = 0.0;
        }
        let p = self.params;
        let algo = p.algorithm();
        let matrix = p.is_matrix();
        let sr = self.sample_rate;
        let nyquist = sr * 0.45;
        let heard = (0..OPS).filter(|op| p.is_heard(*op)).count().max(1) as f32;
        let count = p.unison_count();
        let positions = unison_positions(count);
        let scale = 1.0 / (heard * count as f32).sqrt();
        let fb_op = p.feedback_op();
        let over = p.oversampled();
        let rate_mul = if over { 2.0 } else { 1.0 };
        let fmode = match p.fmode.round() as u8 {
            1 => FilterMode::Highpass,
            2 => FilterMode::BandpassUnity,
            3 => FilterMode::Notch,
            _ => FilterMode::Lowpass,
        };
        // Glide: a share of what is left, per hop, so it lands in about
        // GLIDE milliseconds.
        let glide_keep = if p.glide > 0.0 {
            (-(HOP as f32) / (sr * p.glide / 1000.0 / 4.0)).exp()
        } else {
            0.0
        };
        let waves = &self.waves;
        let looping = p.env_loop.round() >= 1.0;
        let mut sounding = 0usize;
        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(HOP).min(self.block_l.len());
                if take == 0 {
                    break;
                }
                let v = &mut self.voices[vi];
                // Looping envelopes: one that has reached its sustain
                // while the key is down starts again — a rhythm from
                // the key held.
                if looping && v.held {
                    for env in v.env.iter_mut() {
                        if env.stage() == EnvStage::Sustain {
                            env.gate_on();
                        }
                    }
                }
                for (k, env) in v.env.iter_mut().enumerate() {
                    if let Some(buf) = self.envs[k].get_mut(..take) {
                        env.process(buf);
                    }
                }
                let touch = 1.0 - p.velocity + p.velocity * v.vel;
                let t = v.elapsed as f32 / sr;
                let hop_seconds = take as f32 / sr;
                let mut lfo_pitch = 0.0f32;
                let mut lfo_mod = 0.0f32;
                let mut lfo_amp = 0.0f32;
                let mut lfo_filter = 0.0f32;
                for (i, lfo) in p.lfos.iter().enumerate() {
                    let value = v.lfo(i, lfo, t, hop_seconds);
                    lfo_pitch += value * lfo.pitch;
                    lfo_mod += value * lfo.modulators;
                    lfo_amp += value * lfo.amp;
                    lfo_filter += value * lfo.filter;
                }
                let mod_scale = (1.0 + lfo_mod).clamp(0.0, 2.0);
                let amp_scale = (1.0 + lfo_amp).clamp(0.0, 2.0);
                v.glide_semis *= glide_keep;
                let bend_all =
                    p.pitch1 * rise_fall(t, p.p1_rise, p.p1_fall) + lfo_pitch + v.glide_semis;
                let bend_mod = p.pitch2 * rise_fall(t, p.p2_rise, p.p2_fall);
                // The operators' increments for this hop, per unison
                // copy, at the rate the loop runs at.
                let mut inc = [[0.0f32; OPS]; UNISON_MAX];
                let mut is_mod = [false; OPS];
                for (k, op) in p.ops.iter().enumerate() {
                    is_mod[k] = p.is_modulating(k);
                    let bend = bend_all + if is_mod[k] { bend_mod } else { 0.0 };
                    for u in 0..count {
                        let detune = positions[u] * p.udetune / 100.0;
                        let hz = if op.fixed.round() >= 1.0 {
                            op.hz * (op.fine / 1200.0 + (bend + detune) / 12.0).exp2()
                        } else {
                            v.base_hz
                                * op.ratio
                                * (op.fine / 1200.0 + (bend + detune) / 12.0).exp2()
                        };
                        inc[u][k] = (hz / (sr * rate_mul)).clamp(0.0, 0.5);
                    }
                }
                let steps = if over { take * 2 } else { take };
                for s in 0..steps {
                    // The envelope for this step: at 2x, each envelope
                    // sample serves two steps.
                    let es = if over { s / 2 } else { s };
                    let mut l = 0.0f32;
                    let mut r = 0.0f32;
                    for u in 0..count {
                        let mut next = [0.0f32; OPS];
                        for k in (0..OPS).rev() {
                            let op = &p.ops[k];
                            let mut turns = 0.0f32;
                            if matrix {
                                // Any into any, the last sample's outputs:
                                // one sample of delay lets a loop of any
                                // shape close without an order.
                                for m in 0..OPS {
                                    let amount = p.matrix[m][k];
                                    if amount > 0.0 {
                                        let index =
                                            if m == k { FEEDBACK_TURNS } else { INDEX_TURNS };
                                        turns += v.out[u][m] * amount * index;
                                    }
                                }
                            } else {
                                for (m, c) in algo.edges {
                                    if *c == k {
                                        turns += next[*m] * INDEX_TURNS;
                                    }
                                }
                                if k == fb_op {
                                    turns += v.out[u][k] * p.feedback * FEEDBACK_TURNS;
                                }
                            }
                            let env = self.envs[k].get(es).copied().unwrap_or(0.0);
                            let level = op.level
                                * env
                                * v.key_gain[k]
                                * if is_mod[k] { mod_scale } else { amp_scale };
                            let wave = op.wave.round().max(0.0) as usize;
                            let sample = if wave >= waves.len() {
                                v.rand()
                            } else {
                                sine_at(&waves[wave], v.phase[u][k] + turns)
                            };
                            let y = sample * level;
                            v.phase[u][k] = (v.phase[u][k] + inc[u][k]).fract();
                            next[k] = y;
                        }
                        v.out[u] = next;
                        let mut y = 0.0f32;
                        if matrix {
                            for k in 0..OPS {
                                y += next[k] * p.out[k];
                            }
                        } else {
                            for c in algo.carriers {
                                y += next[*c];
                            }
                        }
                        let (gl, gr) = crate::dsp::pan::spread(positions[u] * p.width);
                        l += y * gl;
                        r += y * gr;
                    }
                    let (dst_l, dst_r) = if over {
                        (self.up_l.get_mut(s), self.up_r.get_mut(s))
                    } else {
                        (self.block_l.get_mut(s), self.block_r.get_mut(s))
                    };
                    if let Some(slot) = dst_l {
                        *slot = l * scale * touch;
                    }
                    if let Some(slot) = dst_r {
                        *slot = r * scale * touch;
                    }
                    if !over || s % 2 == 1 {
                        v.elapsed += 1;
                    }
                }
                if over {
                    if let (Some(up), Some(down)) =
                        (self.up_l.get(..take * 2), self.block_l.get_mut(..take))
                    {
                        v.over_l.down(up, down);
                    }
                    if let (Some(up), Some(down)) =
                        (self.up_r.get(..take * 2), self.block_r.get_mut(..take))
                    {
                        v.over_r.down(up, down);
                    }
                }
                // The filter, its cutoff riding its envelope, the key,
                // and the LFOs.
                let fe = rise_fall(t, p.fenv_att, p.fenv_dec) * p.fenv;
                let key = p.keytrack * (f32::from(v.pitch) - 60.0) / 12.0;
                let hz = (p.cutoff * (fe + key + lfo_filter).exp2()).clamp(20.0, nyquist);
                v.filter_l.prepare(sr, hz, p.reso);
                v.filter_r.prepare(sr, hz, p.reso);
                if let (Some(bl), Some(br)) =
                    (self.block_l.get_mut(..take), self.block_r.get_mut(..take))
                {
                    v.filter_l.process(bl, fmode);
                    v.filter_r.process(br, fmode);
                    for i in 0..take {
                        if let Some(slot) = self.left.get_mut(done + i) {
                            *slot += bl[i];
                        }
                        if let Some(slot) = self.right.get_mut(done + i) {
                            *slot += br[i];
                        }
                    }
                }
                done += take;
            }
            let v = &mut self.voices[vi];
            let alive = (0..OPS).any(|k| p.is_heard(k) && v.env[k].active());
            if alive {
                sounding += 1;
            } else {
                v.active = false;
                v.held = false;
            }
        }
        self.said_voices = sounding as f32;
        if p.dist.round() >= 1.0 {
            if let Some(l) = self.left.get_mut(..n) {
                self.shaper.process(l);
            }
            if let Some(r) = self.right.get_mut(..n) {
                self.shaper.process(r);
            }
        }
        let level = p.level;
        for i in 0..n {
            let g = gain.next() * level;
            let l = self.left.get(i).copied().unwrap_or(0.0) * g;
            let r = self.right.get(i).copied().unwrap_or(0.0) * g;
            if let Some(slot) = out.get_mut(i) {
                *slot = if l.is_finite() { l } else { 0.0 };
            }
            if let Some(slot) = self.stash.get_mut(at + i) {
                *slot = if r.is_finite() { r } else { 0.0 };
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::audio::graph::Ramp;

    const FS: f32 = 48_000.0;

    fn run(voices: &mut QuadVoices, n: usize) -> Vec<f32> {
        let mut out = vec![0.0; n];
        let mut ramp = Ramp::across(1.0, 1.0, n);
        voices.render(&mut out, 0, &mut ramp);
        out
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len().max(1) as f32).sqrt()
    }

    /// The share of energy above the fundamental: a rough brightness.
    fn brightness(xs: &[f32]) -> f32 {
        let mut hp = 0.0f32;
        let mut all = 0.0f32;
        let mut last = 0.0f32;
        for x in xs {
            let d = x - last;
            hp += d * d;
            all += x * x;
            last = *x;
        }
        if all > 0.0 { hp / all } else { 0.0 }
    }

    fn crossings(xs: &[f32]) -> usize {
        xs.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    /// A carrier alone, wide open, no envelopes in the way.
    fn plain() -> QuadParams {
        let mut params = QuadParams::default();
        params.ops[1].level = 0.0;
        params.cutoff = 20_000.0;
        params.key_rate = 0.0;
        params
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut voices = QuadVoices::new(FS, 256, QuadParams::default());
        for def in p::TABLE {
            voices.set_param(def.id, def.max);
            assert_eq!(voices.params().get(def.id), Some(def.max), "{}", def.name);
        }
        let d = QuadParams::default();
        assert_eq!(d.ops[0].level, 1.0);
        assert_eq!(d.ops[1].ratio, 2.0);
        assert_eq!(d.ops[2].vel, 0.5);
        assert_eq!(d.key_rate, 0.3);
        assert_eq!(d.lfos[1].rate, 5.0);
        assert_eq!(d.feedback_op(), 3);
    }

    #[test]
    fn a_note_sounds_and_a_modulator_brightens_it() {
        let dull = plain();
        let mut a = QuadVoices::new(FS, 256, dull);
        a.note_on(57, 100, 1);
        let quiet = run(&mut a, 4_800);
        assert!(rms(&quiet) > 0.05, "no sound: {}", rms(&quiet));
        assert!(quiet.iter().all(|x| x.is_finite() && x.abs() <= 1.5));
        assert_eq!(a.readout().bands[0], 1.0);
        let mut bright = QuadParams::default();
        bright.ops[1].level = 1.0;
        let mut b = QuadVoices::new(FS, 256, bright);
        b.note_on(57, 100, 1);
        let loud = run(&mut b, 4_800);
        assert!(
            brightness(&loud) > brightness(&quiet) * 3.0,
            "the modulator added no harmonics: {} vs {}",
            brightness(&loud),
            brightness(&quiet)
        );
        a.note_off(57);
        let _ = run(&mut a, 48_000);
        assert_eq!(
            a.readout().bands[0],
            0.0,
            "released, still sounding a second on"
        );
    }

    #[test]
    fn every_algorithm_renders_finite_with_feedback_on_any_operator_and_distortion() {
        for algo in 0..p::ALGORITHMS.len() {
            for (fb_op, feedback) in [(3usize, 1.0f32), (0, -1.0), (1, 1.0)] {
                let mut params = QuadParams::default();
                params.algo = algo as f32;
                params.feedback = feedback;
                params.fb_op = fb_op as f32;
                params.dist = 3.0;
                params.drive = 20.0;
                params.unison = 2.0;
                for op in params.ops.iter_mut() {
                    op.level = 1.0;
                }
                let mut voices = QuadVoices::new(FS, 256, params);
                voices.note_on(48, 127, 1);
                voices.note_on(55, 90, 2);
                let out = run(&mut voices, 9_600);
                assert!(
                    out.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3),
                    "algo {algo} fb {fb_op}"
                );
                assert!(rms(&out) > 0.05, "algo {algo} fb {fb_op} is silent");
            }
        }
        // Feedback on the carrier changes the sound; its sign changes it again.
        let mut none = plain();
        none.feedback = 0.0;
        let mut pos = plain();
        pos.fb_op = 0.0;
        pos.feedback = 1.0;
        let mut neg = pos;
        neg.feedback = -1.0;
        let render = |params: QuadParams| {
            let mut v = QuadVoices::new(FS, 256, params);
            v.note_on(48, 100, 1);
            run(&mut v, 4_800)
        };
        let (a, b, c) = (render(none), render(pos), render(neg));
        assert!(
            brightness(&b) > brightness(&a) * 1.5,
            "{} vs {}",
            brightness(&b),
            brightness(&a)
        );
        assert!(b.iter().zip(c.iter()).any(|(x, y)| (x - y).abs() > 0.05));
    }

    #[test]
    fn the_pitch_envelopes_bend_and_the_second_only_bends_modulators() {
        let mut params = plain();
        params.pitch1 = 12.0;
        params.p1_rise = 0.0;
        params.p1_fall = 4_000.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(69, 100, 1);
        let early = run(&mut v, 960);
        assert!(crossings(&early) > 28, "{}", crossings(&early));
        let mut params = plain();
        params.pitch2 = 12.0;
        params.p2_fall = 4_000.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(69, 100, 1);
        let early = run(&mut v, 960);
        assert!(crossings(&early) < 22, "{}", crossings(&early));
        assert_eq!(rise_fall(0.0, 0.0, 100.0), 1.0);
        assert!(rise_fall(0.5, 0.0, 100.0) < 0.01);
        assert!((rise_fall(0.05, 100.0, 100.0) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn waves_fixed_frequency_key_scaling_and_key_rate_do_what_they_say() {
        let mut last: Vec<f32> = Vec::new();
        for wave in 0..p::WAVE_NAMES.len() {
            let mut params = plain();
            params.ops[0].wave = wave as f32;
            let mut v = QuadVoices::new(FS, 256, params);
            v.note_on(57, 100, 1);
            let out = run(&mut v, 4_800);
            assert!(out.iter().all(|x| x.is_finite()), "wave {wave}");
            assert!(rms(&out) > 0.03, "wave {wave} is silent");
            if wave > 0 {
                assert!(
                    out.iter()
                        .zip(last.iter())
                        .any(|(a, b)| (a - b).abs() > 0.01),
                    "wave {wave} is wave {}",
                    wave - 1
                );
            }
            last = out;
        }
        let mut params = plain();
        params.ops[0].fixed = 1.0;
        params.ops[0].hz = 440.0;
        let mut low = QuadVoices::new(FS, 256, params);
        let mut high = QuadVoices::new(FS, 256, params);
        low.note_on(45, 100, 1);
        high.note_on(81, 100, 1);
        let a = run(&mut low, 960);
        let b = run(&mut high, 960);
        assert!((crossings(&a) as i32 - crossings(&b) as i32).abs() <= 1);
        let mut params = plain();
        params.ops[0].keyscale = -12.0;
        params.ops[0].vel = 0.0;
        params.velocity = 0.0;
        let mut c4 = QuadVoices::new(FS, 256, params);
        let mut c5 = QuadVoices::new(FS, 256, params);
        c4.note_on(60, 100, 1);
        c5.note_on(72, 100, 1);
        let a = run(&mut c4, 2_400);
        let b = run(&mut c5, 2_400);
        let ratio = rms(&b) / rms(&a);
        assert!(
            (ratio - 0.25).abs() < 0.06,
            "an octave up at -12 dB/oct came out at {ratio}"
        );
        let mut params = plain();
        params.ops[0].sustain = 0.0;
        params.ops[0].decay = 400.0;
        params.key_rate = 1.0;
        let mut slow = QuadVoices::new(FS, 256, params);
        let mut fast = QuadVoices::new(FS, 256, params);
        slow.note_on(48, 100, 1);
        fast.note_on(72, 100, 1);
        let _ = run(&mut slow, 9_600);
        let _ = run(&mut fast, 9_600);
        let a = run(&mut slow, 2_400);
        let b = run(&mut fast, 2_400);
        assert!(rms(&b) < rms(&a) * 0.5, "{} vs {}", rms(&b), rms(&a));
        for (depth, expect_more) in [(1.0f32, true), (0.0, false)] {
            let mut params = QuadParams::default();
            params.ops[1].level = 1.0;
            params.ops[1].vel = depth;
            params.velocity = 0.0;
            let mut soft = QuadVoices::new(FS, 256, params);
            let mut hard = QuadVoices::new(FS, 256, params);
            soft.note_on(48, 10, 1);
            hard.note_on(48, 127, 1);
            let ys = run(&mut soft, 2_400);
            let yh = run(&mut hard, 2_400);
            let brighter = brightness(&yh) > brightness(&ys) * 1.2;
            assert_eq!(brighter, expect_more, "depth {depth}");
        }
    }

    #[test]
    fn unison_spreads_the_field_and_width_zero_folds_it_back() {
        let mut params = plain();
        params.unison = 4.0;
        params.udetune = 20.0;
        params.width = 1.0;
        let mut wide = QuadVoices::new(FS, 256, params);
        wide.note_on(57, 100, 1);
        let l = run(&mut wide, 4_800);
        let r = wide.right(4_800).to_vec();
        assert!(l.iter().all(|x| x.is_finite()) && rms(&l) > 0.05);
        assert!(
            l.iter().zip(r.iter()).any(|(a, b)| (a - b).abs() > 0.02),
            "the sides are the same"
        );
        params.width = 0.0;
        let mut narrow = QuadVoices::new(FS, 256, params);
        narrow.note_on(57, 100, 1);
        let l = run(&mut narrow, 4_800);
        let r = narrow.right(4_800).to_vec();
        assert!(l.iter().zip(r.iter()).all(|(a, b)| (a - b).abs() < 1e-5));
        // Detuned copies beat: the level breathes where one copy is flat.
        let mut one = plain();
        one.unison = 1.0;
        let mut single = QuadVoices::new(FS, 256, one);
        single.note_on(57, 100, 1);
        let _ = run(&mut single, 4_800);
        let s = run(&mut single, 4_800);
        let mut detuned = QuadVoices::new(FS, 256, params);
        detuned.note_on(57, 100, 1);
        let _ = run(&mut detuned, 4_800);
        let d = run(&mut detuned, 4_800);
        let spread = |xs: &[f32]| {
            let chunks: Vec<f32> = xs.chunks(480).map(rms).collect();
            let hi = chunks.iter().cloned().fold(0.0f32, f32::max);
            let lo = chunks.iter().cloned().fold(1.0f32, f32::min);
            hi - lo
        };
        assert!(
            spread(&d) > spread(&s) * 2.0,
            "{} vs {}",
            spread(&d),
            spread(&s)
        );
    }

    #[test]
    fn mono_holds_one_voice_and_legato_glides_without_retriggering() {
        let mut params = plain();
        params.mono = p::MONO_MONO;
        params.glide = 200.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(45, 100, 1);
        let _ = run(&mut v, 2_400);
        v.note_on(57, 100, 2);
        let _ = run(&mut v, 64);
        assert_eq!(v.readout().bands[0], 1.0, "mono sounded two voices");
        // Just after the second key the pitch is still near the first:
        // the glide has 200 ms to go.
        let early = run(&mut v, 2_400);
        let _ = run(&mut v, 48_000);
        let late = run(&mut v, 2_400);
        assert!(
            crossings(&late) > crossings(&early) + 4,
            "no glide: {} then {}",
            crossings(&early),
            crossings(&late)
        );
        // Letting the second key go returns to the first, still held.
        v.note_off(57);
        let _ = run(&mut v, 48_000);
        assert_eq!(v.readout().bands[0], 1.0, "the held key was dropped");
        let back = run(&mut v, 2_400);
        assert!(
            crossings(&back) < crossings(&late),
            "did not return to the lower key"
        );
        v.note_off(45);
        let _ = run(&mut v, 48_000);
        assert_eq!(v.readout().bands[0], 0.0);
        // Legato: a second key while the first is held keeps the
        // envelope where it was rather than starting over.
        let mut params = plain();
        params.mono = p::MONO_LEGATO;
        params.ops[0].attack = 400.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(45, 100, 1);
        let _ = run(&mut v, 24_000);
        let settled = rms(&run(&mut v, 960));
        v.note_on(52, 100, 2);
        let after = rms(&run(&mut v, 960));
        assert!(
            after > settled * 0.7,
            "legato restarted the attack: {settled} -> {after}"
        );
        let mut params = plain();
        params.mono = p::MONO_MONO;
        params.ops[0].attack = 400.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(45, 100, 1);
        let _ = run(&mut v, 24_000);
        v.note_on(52, 100, 2);
        let after = rms(&run(&mut v, 960));
        assert!(
            after < settled * 0.5,
            "mono did not retrigger: {settled} -> {after}"
        );
    }

    #[test]
    fn the_lfos_wobble_pitch_breathe_level_and_brighten_and_wait_their_delay() {
        // Vibrato: the pitch differs between the halves of a cycle.
        let mut params = plain();
        params.lfos[0].rate = 2.0;
        params.lfos[0].pitch = 12.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(57, 100, 1);
        let a = run(&mut v, 2_400);
        let _ = run(&mut v, 9_600);
        let b = run(&mut v, 2_400);
        assert!(
            (crossings(&a) as i32 - crossings(&b) as i32).abs() > 6,
            "{} vs {}",
            crossings(&a),
            crossings(&b)
        );
        // Tremolo: the level breathes.
        let mut params = plain();
        params.lfos[1].rate = 2.0;
        params.lfos[1].amp = 1.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(57, 100, 1);
        let _ = run(&mut v, 4_800);
        let loud = rms(&run(&mut v, 1_200));
        let _ = run(&mut v, 9_600);
        let quiet = rms(&run(&mut v, 1_200));
        assert!(
            (loud - quiet).abs() > loud.max(quiet) * 0.4,
            "{loud} vs {quiet}"
        );
        // Delay: for the first half second an LFO with a delay does nothing.
        let mut params = plain();
        params.lfos[0].rate = 6.0;
        params.lfos[0].amp = 1.0;
        params.lfos[0].delay = 500.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(57, 100, 1);
        let _ = run(&mut v, 2_400);
        let steady: Vec<f32> = run(&mut v, 9_600).chunks(480).map(rms).collect();
        let wobble = steady.iter().cloned().fold(0.0f32, f32::max)
            - steady.iter().cloned().fold(1.0f32, f32::min);
        assert!(wobble < 0.05, "the LFO did not wait: {wobble}");
        // Brightness: an LFO on the modulators moves the harmonics.
        let mut params = QuadParams::default();
        params.ops[1].level = 1.0;
        params.lfos[0].rate = 1.0;
        params.lfos[0].modulators = 1.0;
        params.lfos[0].shape = 2.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(48, 100, 1);
        let up = brightness(&run(&mut v, 4_800));
        let _ = run(&mut v, 19_200);
        let down = brightness(&run(&mut v, 4_800));
        assert!((up - down).abs() > up.max(down) * 0.3, "{up} vs {down}");
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(48, 100, 1);
        v.all_sound_off();
        assert!(run(&mut v, 256).iter().all(|x| *x == 0.0));
    }

    #[test]
    fn the_matrix_routes_any_into_any_and_out_levels_choose_what_is_heard() {
        // The matrix at its defaults is the serial stack: the same sound
        // as shape 0, one sample later.
        let mut shape = QuadParams::default();
        shape.ops[1].level = 1.0;
        let mut matrix = shape;
        matrix.algo = p::ALGO_MATRIX;
        let render = |params: QuadParams| {
            let mut v = QuadVoices::new(FS, 256, params);
            v.note_on(48, 100, 1);
            run(&mut v, 4_800)
        };
        let (a, b) = (render(shape), render(matrix));
        assert!(rms(&b) > 0.05 && b.iter().all(|x| x.is_finite()));
        assert!(
            (brightness(&a) - brightness(&b)).abs() < brightness(&a) * 0.5,
            "{} vs {}",
            brightness(&a),
            brightness(&b)
        );
        // Op 2 into itself and out: heard, and fed back.
        let mut params = QuadParams::default();
        params.algo = p::ALGO_MATRIX;
        params.matrix = [[0.0; OPS]; OPS];
        params.out = [0.0, 1.0, 0.0, 0.0];
        params.ops[1].level = 1.0;
        let clean = render(params);
        params.matrix[1][1] = 1.0;
        let fed = render(params);
        assert!(rms(&clean) > 0.05 && rms(&fed) > 0.05);
        assert!(
            brightness(&fed) > brightness(&clean) * 1.5,
            "{} vs {}",
            brightness(&fed),
            brightness(&clean)
        );
        // Nothing out: silence, and the voice ends when its heard
        // envelopes do — with none heard, at once.
        params.out = [0.0; OPS];
        let none = render(params);
        assert!(none.iter().all(|x| x.abs() < 1e-6));
        // A loop between two operators closes.
        params.out = [1.0, 0.0, 0.0, 0.0];
        params.matrix = [[0.0; OPS]; OPS];
        params.matrix[0][1] = 0.6;
        params.matrix[1][0] = 0.6;
        let ring = render(params);
        assert!(ring.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3));
        assert!(rms(&ring) > 0.05);
    }

    #[test]
    fn a_looping_envelope_restarts_while_held_and_two_x_matches_one_x() {
        let mut params = plain();
        params.ops[0].attack = 0.0;
        params.ops[0].decay = 100.0;
        params.ops[0].sustain = 0.05;
        params.env_loop = 1.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(57, 100, 1);
        let out = run(&mut v, 48_000);
        // The level climbs back up again and again: many loud stretches
        // over a second rather than one fall to the floor.
        let loud: Vec<bool> = out.chunks(480).map(|c| rms(c) > 0.15).collect();
        let rises = loud.windows(2).filter(|w| !w[0] && w[1]).count();
        assert!(rises >= 4, "the envelope looped {rises} times");
        // Oversampling: the same sound, a little cleaner, no louder.
        let mut params = QuadParams::default();
        params.ops[1].level = 1.0;
        params.ops[3].level = 1.0;
        params.algo = 0.0;
        let mut one = QuadVoices::new(FS, 256, params);
        params.oversample = 1.0;
        let mut two = QuadVoices::new(FS, 256, params);
        one.note_on(84, 100, 1);
        two.note_on(84, 100, 1);
        let a = run(&mut one, 4_800);
        let b = run(&mut two, 4_800);
        assert!(b.iter().all(|x| x.is_finite()));
        let (ra, rb) = (rms(&a), rms(&b));
        assert!((ra - rb).abs() < ra.max(rb) * 0.35, "{ra} vs {rb}");
        assert!(
            brightness(&b) <= brightness(&a) * 1.1,
            "2x got harsher: {} vs {}",
            brightness(&b),
            brightness(&a)
        );
    }

    #[test]
    fn the_envelope_walks_its_stages_and_bends_its_decays() {
        let op = Op {
            delay: 10.0,
            attack: 10.0,
            decay: 100.0,
            break_level: 0.5,
            decay2: 100.0,
            sustain: 0.2,
            release: 50.0,
            curve: 0.0,
            ..Default::default()
        };
        let mut env = OpEnv::new();
        env.prepare(1_000.0, &op, 1.0);
        env.gate_on();
        assert_eq!(env.stage(), EnvStage::Delay);
        let mut out = vec![0.0; 10];
        env.process(&mut out);
        assert!(out.iter().all(|x| *x == 0.0), "the delay sounded");
        assert_eq!(env.stage(), EnvStage::Attack);
        env.process(&mut out);
        assert!((out[9] - 1.0).abs() < 1e-6, "attack peaked at {}", out[9]);
        assert_eq!(env.stage(), EnvStage::Decay);
        let mut out = vec![0.0; 100];
        env.process(&mut out);
        assert!(
            (out[49] - 0.75).abs() < 0.02,
            "a straight decay was at {} halfway",
            out[49]
        );
        assert!((out[99] - 0.5).abs() < 1e-6, "the break level: {}", out[99]);
        assert_eq!(env.stage(), EnvStage::Decay2);
        env.process(&mut out);
        assert!((out[99] - 0.2).abs() < 1e-6, "the sustain: {}", out[99]);
        assert_eq!(env.stage(), EnvStage::Sustain);
        env.gate_off();
        let mut out = vec![0.0; 50];
        env.process(&mut out);
        assert!(out[49].abs() < 1e-6 && !env.active());
        // With the break at the top there is one decay, to the sustain.
        let mut plain_env = OpEnv::new();
        plain_env.prepare(
            1_000.0,
            &Op {
                break_level: 1.0,
                attack: 0.0,
                delay: 0.0,
                ..op
            },
            1.0,
        );
        plain_env.gate_on();
        let mut out = vec![0.0; 100];
        plain_env.process(&mut out);
        assert!(
            (out[99] - 0.2).abs() < 1e-6,
            "one decay to the sustain: {}",
            out[99]
        );
        assert_eq!(plain_env.stage(), EnvStage::Sustain);
        let mut bent = OpEnv::new();
        bent.prepare(
            1_000.0,
            &Op {
                curve: 1.0,
                attack: 0.0,
                delay: 0.0,
                ..op
            },
            1.0,
        );
        bent.gate_on();
        assert_eq!(
            bent.stage(),
            EnvStage::Decay,
            "no delay, no attack: straight to the decay"
        );
        let mut out = vec![0.0; 100];
        bent.process(&mut out);
        assert!(
            out[49] > 0.9,
            "a curve of one had fallen to {} halfway",
            out[49]
        );
        let mut params = plain();
        params.ops[0].attack = 0.0;
        params.ops[0].decay = 50.0;
        params.ops[0].break_level = 0.1;
        params.ops[0].decay2 = 200.0;
        params.ops[0].sustain = 1.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(57, 100, 1);
        let _ = run(&mut v, 2_400);
        let low = rms(&run(&mut v, 480));
        let _ = run(&mut v, 24_000);
        let high = rms(&run(&mut v, 480));
        assert!(high > low * 3.0, "no second decay upward: {low} -> {high}");
    }

    #[test]
    fn the_filter_envelope_and_velocity_change_the_tone() {
        let mut open = QuadParams::default();
        open.ops[1].level = 1.0;
        open.cutoff = 300.0;
        open.fenv = 5.0;
        open.fenv_dec = 4_000.0;
        let mut closed = open;
        closed.fenv = 0.0;
        let mut a = QuadVoices::new(FS, 256, open);
        let mut b = QuadVoices::new(FS, 256, closed);
        a.note_on(48, 100, 1);
        b.note_on(48, 100, 1);
        let ya = run(&mut a, 2_400);
        let yb = run(&mut b, 2_400);
        assert!(brightness(&ya) > brightness(&yb) * 1.5);
        let mut soft = QuadParams::default();
        soft.ops[1].level = 1.0;
        soft.velocity = 1.0;
        let mut s = QuadVoices::new(FS, 256, soft);
        let mut h = QuadVoices::new(FS, 256, soft);
        s.note_on(48, 20, 1);
        h.note_on(48, 127, 1);
        let ys = run(&mut s, 2_400);
        let yh = run(&mut h, 2_400);
        assert!(rms(&yh) > rms(&ys) * 2.0);
    }
}
