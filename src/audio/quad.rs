//! QUAD — the four-operator FM workhorse.
//!
//! Four sine operators, each with a ratio to the note, a fine detune,
//! a level and its own ADSR; eight routing shapes from a single serial
//! stack to four carriers side by side; feedback on the top operator.
//! Two pitch envelopes — one that bends every operator, the DX pitch
//! envelope, and one that bends only the modulators, which sweeps the
//! harmonicity while the fundamental stays put. Then a multi-mode
//! filter with its own attack-decay envelope and keytrack, per voice,
//! and after the sum, a distortion stage.
//!
//! Phase modulation, as every FM synth since the DX7 actually is: a
//! modulator's output is added to its carrier's phase, scaled by an
//! index, so a level knob is a brightness knob. The sine is a table
//! read, not a `sin` per operator per voice per sample.

use crate::dsp::adsr::Adsr;
use crate::dsp::filters::{Mode as FilterMode, Svf};
use crate::dsp::interp::linear;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::quad as p;
use crate::params::quad::{Algorithm, OPS};

pub const VOICES: usize = 8;
const TABLE: usize = 2048;
const HOP: usize = 32;
/// A modulator at full level swings its carrier's phase by this many
/// turns: an index of about nine radians, which is where a DX7 starts
/// to get harsh.
const INDEX_TURNS: f32 = 1.5;
/// Feedback's own index: a full turn is a saw already.
const FEEDBACK_TURNS: f32 = 0.5;

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
        };
        for def in p::TABLE {
            me.set(def.id, def.default);
        }
        me
    }
}

impl QuadParams {
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

struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    age: u64,
    vel: f32,
    elapsed: u64,
    base_hz: f32,
    phase: [f32; OPS],
    out: [f32; OPS],
    env: [Adsr; OPS],
    filter: Svf,
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
            phase: [0.0; OPS],
            out: [0.0; OPS],
            env: [Adsr::new(), Adsr::new(), Adsr::new(), Adsr::new()],
            filter: Svf::new(),
        }
    }
}

/// The instrument.
pub struct QuadVoices {
    params: QuadParams,
    base: QuadParams,
    sample_rate: f32,
    sine: Vec<f32>,
    voices: Vec<Voice>,
    /// Scratch: one voice's block, its operator envelopes, the sum.
    block: Vec<f32>,
    envs: [Vec<f32>; OPS],
    left: Vec<f32>,
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
        let mut sine = vec![0.0f32; TABLE + 1];
        for (i, slot) in sine.iter_mut().enumerate() {
            *slot = (i as f32 / TABLE as f32 * core::f32::consts::TAU).sin();
        }
        let n = block.max(HOP);
        let mut me = Self {
            params,
            base: params,
            sample_rate: 48_000.0,
            sine,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            block: vec![0.0; n],
            envs: [vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            left: vec![0.0; n],
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
                env.prepare(sr, op.attack, op.decay, op.sustain, op.release);
            }
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
            v.filter.reset();
            v.out = [0.0; OPS];
        }
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
    }

    pub fn note_off(&mut self, pitch: u8) {
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
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        v.active = true;
        v.held = true;
        v.pitch = pitch;
        v.age = age;
        v.vel = (f32::from(vel) / 127.0).clamp(0.0, 1.0);
        v.elapsed = 0;
        v.base_hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        v.phase = [0.0; OPS];
        v.out = [0.0; OPS];
        v.filter.reset();
        for env in v.env.iter_mut() {
            env.gate_on();
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
        for slot in self.left.iter_mut().take(n) {
            *slot = 0.0;
        }
        let p = self.params;
        let algo = p.algorithm();
        let sr = self.sample_rate;
        let nyquist = sr * 0.45;
        let carriers = algo.carriers.len().max(1) as f32;
        let carrier_scale = 1.0 / carriers.sqrt();
        let fmode = match p.fmode.round() as u8 {
            1 => FilterMode::Highpass,
            2 => FilterMode::BandpassUnity,
            3 => FilterMode::Notch,
            _ => FilterMode::Lowpass,
        };
        let sine = &self.sine;
        let mut sounding = 0usize;
        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(HOP).min(self.block.len());
                if take == 0 {
                    break;
                }
                let v = &mut self.voices[vi];
                // The operators' envelopes, a hop at a time.
                for (k, env) in v.env.iter_mut().enumerate() {
                    if let Some(buf) = self.envs[k].get_mut(..take) {
                        env.process(buf);
                    }
                }
                // Velocity: the modulators' levels are what a harder
                // key brightens; the carriers' what it loudens.
                let touch = 1.0 - p.velocity + p.velocity * v.vel;
                let t = v.elapsed as f32 / sr;
                let bend_all = p.pitch1 * rise_fall(t, p.p1_rise, p.p1_fall);
                let bend_mod = p.pitch2 * rise_fall(t, p.p2_rise, p.p2_fall);
                // The operators' increments for this hop: ratio, fine,
                // and the two bends.
                let mut inc = [0.0f32; OPS];
                for (k, op) in p.ops.iter().enumerate() {
                    let bend = bend_all
                        + if p::is_modulator(algo, k) {
                            bend_mod
                        } else {
                            0.0
                        };
                    let hz = v.base_hz * op.ratio * (op.fine / 1200.0 + bend / 12.0).exp2();
                    inc[k] = (hz / sr).clamp(0.0, 0.5);
                }
                for s in 0..take {
                    // Top operator first: a modulator is always above the
                    // operator it feeds, so one pass in descending order
                    // is the whole graph. Feedback reads the top's own
                    // last output.
                    let mut next = [0.0f32; OPS];
                    for k in (0..OPS).rev() {
                        let op = &p.ops[k];
                        let mut turns = 0.0f32;
                        for (m, c) in algo.edges {
                            if *c == k {
                                turns += next[*m] * INDEX_TURNS;
                            }
                        }
                        if k == OPS - 1 {
                            turns += v.out[k] * p.feedback * FEEDBACK_TURNS;
                        }
                        let env = self.envs[k].get(s).copied().unwrap_or(0.0);
                        let level =
                            op.level * env * if p::is_modulator(algo, k) { touch } else { 1.0 };
                        let y = sine_at(sine, v.phase[k] + turns) * level;
                        v.phase[k] = (v.phase[k] + inc[k]).fract();
                        next[k] = y;
                    }
                    v.out = next;
                    let mut y = 0.0f32;
                    for c in algo.carriers {
                        y += next[*c];
                    }
                    if let Some(slot) = self.block.get_mut(s) {
                        *slot = y * carrier_scale * touch;
                    }
                    v.elapsed += 1;
                }
                // The filter, its cutoff riding its envelope and the key.
                let fe = rise_fall(t, p.fenv_att, p.fenv_dec) * p.fenv;
                let key = p.keytrack * (f32::from(v.pitch) - 60.0) / 12.0;
                let hz = (p.cutoff * (fe + key).exp2()).clamp(20.0, nyquist);
                v.filter.prepare(sr, hz, p.reso);
                if let Some(block) = self.block.get_mut(..take) {
                    v.filter.process(block, fmode);
                    for (i, x) in block.iter().enumerate() {
                        if let Some(slot) = self.left.get_mut(done + i) {
                            *slot += *x;
                        }
                    }
                }
                done += take;
            }
            let v = &mut self.voices[vi];
            let alive = algo.carriers.iter().any(|c| v.env[*c].active());
            if alive {
                sounding += 1;
            } else {
                v.active = false;
                v.held = false;
            }
        }
        self.said_voices = sounding as f32;
        if p.dist.round() >= 1.0
            && let Some(l) = self.left.get_mut(..n)
        {
            self.shaper.process(l);
        }
        let level = p.level;
        for i in 0..n {
            let g = gain.next() * level;
            let l = self.left.get(i).copied().unwrap_or(0.0) * g;
            let l = if l.is_finite() { l } else { 0.0 };
            if let Some(slot) = out.get_mut(i) {
                *slot = l;
            }
            if let Some(slot) = self.stash.get_mut(at + i) {
                *slot = l;
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
    }

    #[test]
    fn a_note_sounds_and_a_modulator_brightens_it() {
        let mut dull = QuadParams::default();
        dull.ops[1].level = 0.0;
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
    fn every_algorithm_renders_finite_with_feedback_and_distortion() {
        for algo in 0..p::ALGORITHMS.len() {
            let mut params = QuadParams::default();
            params.algo = algo as f32;
            params.feedback = 1.0;
            params.dist = 3.0;
            params.drive = 20.0;
            for op in params.ops.iter_mut() {
                op.level = 1.0;
            }
            let mut voices = QuadVoices::new(FS, 256, params);
            voices.note_on(48, 127, 1);
            voices.note_on(55, 90, 2);
            let out = run(&mut voices, 9_600);
            assert!(
                out.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3),
                "algo {algo}"
            );
            assert!(rms(&out) > 0.05, "algo {algo} is silent");
        }
    }

    #[test]
    fn the_pitch_envelopes_bend_and_the_second_only_bends_modulators() {
        // A carrier alone, bent an octave up at the start: the first
        // hop has twice the zero crossings of the settled tail.
        let mut params = QuadParams::default();
        params.ops[1].level = 0.0;
        params.pitch1 = 12.0;
        params.p1_rise = 0.0;
        params.p1_fall = 4_000.0;
        params.fmode = 0.0;
        params.cutoff = 20_000.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(69, 100, 1);
        let early = run(&mut v, 960);
        let crossings = |xs: &[f32]| {
            xs.windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // 440 Hz at an octave up is 880 Hz: about 35 crossings in 20 ms.
        assert!(crossings(&early) > 28, "{}", crossings(&early));
        // The second envelope bends nothing when only a carrier sounds.
        let mut params = QuadParams::default();
        params.ops[1].level = 0.0;
        params.pitch2 = 12.0;
        params.p2_fall = 4_000.0;
        params.cutoff = 20_000.0;
        let mut v = QuadVoices::new(FS, 256, params);
        v.note_on(69, 100, 1);
        let early = run(&mut v, 960);
        assert!(crossings(&early) < 22, "{}", crossings(&early));
        assert_eq!(rise_fall(0.0, 0.0, 100.0), 1.0);
        assert!(rise_fall(0.5, 0.0, 100.0) < 0.01);
        assert!((rise_fall(0.05, 100.0, 100.0) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn the_filter_envelope_and_velocity_change_the_tone_and_all_sound_off_silences() {
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
        assert!(
            brightness(&ya) > brightness(&yb) * 1.5,
            "{} vs {}",
            brightness(&ya),
            brightness(&yb)
        );
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
        assert!(brightness(&yh) > brightness(&ys));
        h.all_sound_off();
        assert!(run(&mut h, 256).iter().all(|x| *x == 0.0));
    }
}
