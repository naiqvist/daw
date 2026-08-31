//! LENS — the analog poly's voice bank.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic below
//! already lives in `src/dsp/`, and this file says which kernel feeds
//! which. Read `notes/20260825-synth-brief.md` for the voice
//! architecture and the lane rule; this header carries only what is
//! specific to the machine being built.
//!
//! # The skeleton is deliberately ordinary
//!
//! Two oscillators, a sub, noise, a 24 dB ladder, two envelopes, sixteen
//! voices. That is the classic poly exactly, and it is ordinary on
//! purpose: the whole point of a workhorse is that you already know
//! where everything is. `haze` is the pad, `acid` is the mono, `tine` is
//! struck, `poly` is the modular one with the matrix. This one is the
//! machine you reach for without thinking.
//!
//! # And then there is the lens
//!
//! What it has instead of a matrix is ONE warp axis the entire
//! oscillator section passes through:
//!
//! ```text
//!  saw A ─┐
//!  pulse B┼─▶ mix ─┬─▶[ 2x ]─▶ FOLD ─▶ RING ─▶[ ÷2 ]─┬─▶ LADDER ─▶ VCA ─▶ pan
//!  sub   ─┤        │                                 │      ▲        ▲
//!  noise ─┘        └── phase warp is UPSTREAM ───────┘   filter env  amp env
//!                      (a pm input to the tables)           ▲
//!                                              DRIFT ───────┘
//! ```
//!
//! Three regimes, weighted by BEND, all three always running:
//!
//! - **phase** is a sine locked to each oscillator's own pitch and phase,
//!   fed to that oscillator's `pm` input. That is SELF-phase-modulation
//!   written feed-forward, so the synth brief's no-feedback rule holds
//!   without costing the sound anything.
//! - **fold** is [`Waveshaper`] on the triangle curve, inside a 2x
//!   oversampled block, because a folder run at the base rate is an
//!   aliasing machine with a nice name.
//! - **ring** is a sine at [`RING_RATIO`](crate::params::lens::RING_RATIO)
//!   times the note, and it lives INSIDE the oversampled block with the
//!   folder — its modulator oscillator is prepared at twice the sample
//!   rate for exactly that reason. Ring-modulating a folded signal at
//!   the base rate would fold the aliases back in after the trouble had
//!   already been taken to avoid them.
//!
//! The depths come from `params::lens`, not from here, because the card
//! draws the same warp and two copies of a transfer curve is two
//! opinions about what the knob does.
//!
//! # The pulse has no waveform
//!
//! Osc B is TWO saw readers a duty cycle apart, subtracted. That is the
//! analog trick and it needs no table of its own: the offset is fed as a
//! constant `pm`, so WIDTH is continuous, sweepable, and cannot make a
//! running voice jump — `pm` moves the read, never the accumulator.
//!
//! # Drift is per NOTE, not per destination
//!
//! One random number per voice at note-on moves that voice's pitch,
//! cutoff and warp together, and a slow global wander moves all of them
//! at once. That is what the fault actually was: one drifting reference
//! per card. Independent noise on three destinations sounds like three
//! effects; this sounds like an instrument that was switched on an hour
//! ago.
//!
//! # Ensemble splits the band first
//!
//! The classic string chorus smears bass into porridge because it
//! choruses everything. [`Crossover3`] holds everything below
//! [`ENSEMBLE_SPLIT_HZ`](crate::params::lens::ENSEMBLE_SPLIT_HZ) still
//! and lets the top swim. At zero the whole stage is skipped and the
//! path is bit-exact dry.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::adsr::LaneAdsr;
use crate::dsp::crossover::Crossover3;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::filters::{LaneCascade, LaneOnePole, LaneSvf, Mode as FilterMode};
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::dsp::noise::{LanePinkNoise, LaneWhiteNoise, WhiteNoise};
use crate::dsp::osc::{LaneOsc, Waveform, build_tables, table_len};
use crate::dsp::shaper::{LaneOversampler2x, Mode as ShapeMode, Waveshaper};
use crate::dsp::{LANES, LaneFrame};
use crate::params::lens as p;

/// Two groups of eight lanes. Sixteen voices is what a poly of this
/// shape has always had, and it is two `ymm` registers wide.
pub const GROUPS: usize = 2;
pub const VOICES: usize = GROUPS * LANES;

/// How much of a block is rendered before the filter coefficients are
/// worked out again. Shorter than `tine`'s, and it has to be: the
/// filter envelope is the fast-moving thing in a subtractive synth, and
/// at 64 a snappy sweep steps audibly.
const CHUNK: usize = 32;

/// Where the ladder tracks the keyboard. Half, which is the setting a
/// classic poly wires in rather than exposes: full tracking makes the
/// top octave shrill and none makes the bottom one mud.
const KEYTRACK: f32 = 0.5;

/// How far apart the lanes are seated. Fixed rather than a knob: the
/// machines that panned by voice did it at one width, and a knob here
/// would compete with the ensemble for the same job.
const SEAT_SPREAD: f32 = 0.55;

/// The DC blocker under the whole bank, in hertz. The fold is an even
/// nonlinearity at high drive and will happily park the sum off zero.
const HP_HZ: f32 = 22.0;

/// The slow shared wander, in hertz — the "it has been on for an hour"
/// component of drift, as opposed to the per-voice one.
const WANDER_HZ: f32 = 0.09;

/// Voices sum into one bus, so the bank is scaled for a chord rather
/// than for one note. Measured: sixteen voices of the default patch
/// peak just under one with this.
const BANK_GAIN: f32 = 0.30;

/// The node-side fold stage. Drive one is OFF, including for samples
/// outside the folder's nominal rails, so the bypass returns without
/// touching a float.
#[inline(always)]
fn process_fold(shaper: &Waveshaper, drive: f32, makeup: f32, io: &mut [LaneFrame]) {
    if drive <= 1.0 {
        return;
    }
    shaper.process_lanes(io);
    for frame in io.iter_mut() {
        for sample in frame.iter_mut() {
            *sample *= makeup;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LensParams {
    pub mix: f32,
    pub detune: f32,
    pub width: f32,
    pub sub: f32,
    pub noise: f32,
    pub warp: f32,
    pub bend: f32,
    pub drift: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub env: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub ensemble: f32,
    pub level: f32,
}

impl Default for LensParams {
    fn default() -> Self {
        let at = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            mix: at(p::MIX),
            detune: at(p::DETUNE),
            width: at(p::WIDTH),
            sub: at(p::SUB),
            noise: at(p::NOISE),
            warp: at(p::WARP),
            bend: at(p::BEND),
            drift: at(p::DRIFT),
            cutoff: at(p::CUTOFF),
            reso: at(p::RESO),
            env: at(p::ENV),
            attack: at(p::ATTACK),
            decay: at(p::DECAY),
            sustain: at(p::SUSTAIN),
            release: at(p::RELEASE),
            ensemble: at(p::ENSEMBLE),
            level: at(p::LEVEL),
        }
    }
}

impl LensParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        // `f32::clamp` deliberately propagates NaN. Parameter letters do
        // not get to: retain a useful, finite value at the source rather
        // than letting NaN reach the oscillator mix and hiding it later.
        let v = if value.is_finite() {
            def.clamp(value)
        } else {
            def.default
        };
        match param {
            p::MIX => self.mix = v,
            p::DETUNE => self.detune = v,
            p::WIDTH => self.width = v,
            p::SUB => self.sub = v,
            p::NOISE => self.noise = v,
            p::WARP => self.warp = v,
            p::BEND => self.bend = v,
            p::DRIFT => self.drift = v,
            p::CUTOFF => self.cutoff = v,
            p::RESO => self.reso = v,
            p::ENV => self.env = v,
            p::ATTACK => self.attack = v,
            p::DECAY => self.decay = v,
            p::SUSTAIN => self.sustain = v,
            p::RELEASE => self.release = v,
            p::ENSEMBLE => self.ensemble = v,
            p::LEVEL => self.level = v,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::MIX => self.mix,
            p::DETUNE => self.detune,
            p::WIDTH => self.width,
            p::SUB => self.sub,
            p::NOISE => self.noise,
            p::WARP => self.warp,
            p::BEND => self.bend,
            p::DRIFT => self.drift,
            p::CUTOFF => self.cutoff,
            p::RESO => self.reso,
            p::ENV => self.env,
            p::ATTACK => self.attack,
            p::DECAY => self.decay,
            p::SUSTAIN => self.sustain,
            p::RELEASE => self.release,
            p::ENSEMBLE => self.ensemble,
            p::LEVEL => self.level,
            _ => return None,
        })
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE {
            let now = self.get(def.id).unwrap_or(def.default);
            let ok = if now.is_finite() { now } else { def.default };
            self.set(def.id, def.clamp(ok));
        }
    }

    /// The shape the card draws, from the values the engine holds.
    pub fn shape(&self) -> p::Shape {
        p::Shape {
            mix: self.mix,
            width: self.width,
            sub: self.sub,
            noise: self.noise,
            warp: self.warp,
            bend: self.bend,
        }
    }
}

/// The parameters whose change costs a coefficient rebuild. Compared
/// every render so a knob is heard now, without rebuilding for the ones
/// that are read per sample anyway.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    noise: f32,
    ensemble: f32,
}

impl Resolved {
    fn of(v: &LensParams) -> Self {
        Self {
            attack: v.attack,
            decay: v.decay,
            sustain: v.sustain,
            release: v.release,
            noise: v.noise,
            ensemble: v.ensemble,
        }
    }
}

/// One group of eight voices, lane-major throughout.
struct Group {
    /// Osc A: the saw.
    saw: LaneOsc,
    /// Osc B: two saw readers a duty apart, which is a pulse.
    pulse_a: LaneOsc,
    pulse_b: LaneOsc,
    /// The sub, one octave down.
    sub: LaneOsc,
    /// The warp modulator, locked to each voice's own pitch.
    warp_osc: LaneOsc,
    /// The ring modulator, running at TWICE the sample rate because it
    /// is applied inside the oversampled block.
    ring_osc: LaneOsc,
    white: LaneWhiteNoise,
    pink: LanePinkNoise,
    /// Takes the edge off the noise so NOISE is a texture, not a hiss.
    noise_tone: LaneOnePole,
    amp: LaneAdsr,
    fenv: LaneAdsr,
    ladder: LaneCascade,
    hp: LaneSvf,
    os: LaneOversampler2x,

    // Scratch, all lane-major, all born in `prepare_at`.
    osc: Vec<LaneFrame>,
    leg: Vec<LaneFrame>,
    warp_buf: Vec<LaneFrame>,
    warp_duty: Vec<LaneFrame>,
    ring_buf: Vec<LaneFrame>,
    up: Vec<LaneFrame>,
    env: Vec<LaneFrame>,
    fen: Vec<LaneFrame>,
    nz: Vec<LaneFrame>,

    pitch: [u8; LANES],
    age: [u64; LANES],
    held: [bool; LANES],
    hz: [f32; LANES],
    vel: [f32; LANES],
    /// This voice's own place on the drift axis, `-1..1`, drawn once at
    /// note-on and held for the life of the note.
    drift: [f32; LANES],
}

impl Group {
    fn new() -> Self {
        Self {
            saw: LaneOsc::new(),
            pulse_a: LaneOsc::new(),
            pulse_b: LaneOsc::new(),
            sub: LaneOsc::new(),
            warp_osc: LaneOsc::new(),
            ring_osc: LaneOsc::new(),
            white: LaneWhiteNoise::new(),
            pink: LanePinkNoise::new(),
            noise_tone: LaneOnePole::new(),
            amp: LaneAdsr::new(),
            fenv: LaneAdsr::new(),
            ladder: LaneCascade::new(),
            hp: LaneSvf::new(),
            os: LaneOversampler2x::new(),
            osc: Vec::new(),
            leg: Vec::new(),
            warp_buf: Vec::new(),
            warp_duty: Vec::new(),
            ring_buf: Vec::new(),
            up: Vec::new(),
            env: Vec::new(),
            fen: Vec::new(),
            nz: Vec::new(),
            pitch: [0; LANES],
            age: [0; LANES],
            held: [false; LANES],
            hz: [220.0; LANES],
            vel: [0.0; LANES],
            drift: [0.0; LANES],
        }
    }

    /// Green zone.
    fn alloc(&mut self) {
        let zero = [0.0f32; LANES];
        for buf in [
            &mut self.osc,
            &mut self.leg,
            &mut self.warp_buf,
            &mut self.warp_duty,
            &mut self.env,
            &mut self.fen,
            &mut self.nz,
        ] {
            buf.clear();
            buf.resize(CHUNK, zero);
        }
        for buf in [&mut self.up, &mut self.ring_buf] {
            buf.clear();
            buf.resize(CHUNK * 2, zero);
        }
    }

    fn reset(&mut self) {
        self.saw.reset();
        self.pulse_a.reset();
        self.pulse_b.reset();
        self.sub.reset();
        self.warp_osc.reset();
        self.ring_osc.reset();
        self.white.reset();
        self.pink.reset();
        self.noise_tone.reset();
        self.amp.reset();
        self.fenv.reset();
        self.ladder.reset();
        self.hp.reset();
        self.os.reset();
        self.held = [false; LANES];
        self.vel = [0.0; LANES];
        self.drift = [0.0; LANES];
    }

    fn reset_lane(&mut self, lane: usize) {
        self.saw.reset_lane(lane);
        self.pulse_a.reset_lane(lane);
        self.pulse_b.reset_lane(lane);
        self.sub.reset_lane(lane);
        self.warp_osc.reset_lane(lane);
        self.ring_osc.reset_lane(lane);
        self.noise_tone.reset_lane(lane);
        self.amp.reset_lane(lane);
        self.fenv.reset_lane(lane);
        self.ladder.reset_lane(lane);
        self.hp.reset_lane(lane);
        self.os.reset_lane(lane);
    }
}

pub struct LensVoices {
    params: LensParams,
    /// The base a p-lock releases back to — the LIVE knob, not a
    /// compile-time snapshot.
    base: LensParams,
    prepared: Resolved,
    sample_rate: f32,
    groups: Vec<Group>,

    saw_tables: Vec<f32>,
    square_tables: Vec<f32>,
    sine_tables: Vec<f32>,

    /// The nonlinearity. Stateless, so one is shared by every voice.
    shaper: Waveshaper,
    /// Where the per-voice drift offsets come from. Scalar and drawn
    /// once per note-on, which is a handful of arithmetic in the red
    /// zone rather than a stream.
    dice: WhiteNoise,
    /// The slow shared wander.
    wander: Lfo,
    wander_buf: Vec<f32>,

    // The ensemble.
    split_l: Crossover3,
    split_r: Crossover3,
    low_l: Vec<f32>,
    mid_l: Vec<f32>,
    high_l: Vec<f32>,
    low_r: Vec<f32>,
    mid_r: Vec<f32>,
    high_r: Vec<f32>,
    chorus_l: DelayLine,
    chorus_r: DelayLine,
    store_l: Vec<f32>,
    store_r: Vec<f32>,
    sweep_l: Lfo,
    sweep_r: Lfo,
    delays_l: Vec<f32>,
    delays_r: Vec<f32>,

    /// The summed voices.
    left: Vec<f32>,
    right: Vec<f32>,
    /// The right channel the node reads back after a mono-shaped walk.
    stash: Vec<f32>,
    said_voices: f32,
}

impl LensVoices {
    pub fn new(sample_rate: f32, block: usize, params: LensParams) -> Self {
        let mut me = Self {
            params,
            base: params,
            prepared: Resolved::of(&params),
            sample_rate: 48_000.0,
            groups: (0..GROUPS).map(|_| Group::new()).collect(),
            saw_tables: vec![0.0; table_len(Waveform::Saw)],
            square_tables: vec![0.0; table_len(Waveform::Square)],
            sine_tables: vec![0.0; table_len(Waveform::Sine)],
            shaper: Waveshaper::new(),
            dice: WhiteNoise::new(),
            wander: Lfo::new(),
            wander_buf: Vec::new(),
            split_l: Crossover3::new(),
            split_r: Crossover3::new(),
            low_l: Vec::new(),
            mid_l: Vec::new(),
            high_l: Vec::new(),
            low_r: Vec::new(),
            mid_r: Vec::new(),
            high_r: Vec::new(),
            chorus_l: DelayLine::new(),
            chorus_r: DelayLine::new(),
            store_l: Vec::new(),
            store_r: Vec::new(),
            sweep_l: Lfo::new(),
            sweep_r: Lfo::new(),
            delays_l: Vec::new(),
            delays_r: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
            stash: Vec::new(),
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.alloc(block.max(CHUNK));
        me.prepare_at(sample_rate);
        me
    }

    /// Green zone: everything heap-shaped is born here.
    fn alloc(&mut self, block: usize) {
        let n = block.max(CHUNK);
        for buf in [
            &mut self.left,
            &mut self.right,
            &mut self.stash,
            &mut self.wander_buf,
            &mut self.low_l,
            &mut self.mid_l,
            &mut self.high_l,
            &mut self.low_r,
            &mut self.mid_r,
            &mut self.high_r,
            &mut self.delays_l,
            &mut self.delays_r,
        ] {
            buf.clear();
            buf.resize(n, 0.0);
        }
        for g in self.groups.iter_mut() {
            g.alloc();
        }
    }

    pub fn prepare_at(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;

        build_tables(Waveform::Saw, &mut self.saw_tables);
        build_tables(Waveform::Square, &mut self.square_tables);
        build_tables(Waveform::Sine, &mut self.sine_tables);

        for (i, g) in self.groups.iter_mut().enumerate() {
            g.saw.prepare(fs, Waveform::Saw);
            g.pulse_a.prepare(fs, Waveform::Saw);
            g.pulse_b.prepare(fs, Waveform::Saw);
            g.sub.prepare(fs, Waveform::Square);
            g.warp_osc.prepare(fs, Waveform::Sine);
            // TWICE the rate: this one is read inside the oversampled
            // block, so its increment has to be for that clock.
            g.ring_osc.prepare(fs * 2.0, Waveform::Sine);
            g.white.seed(0x1E45_0011 + i as u64 * 2_311);
            g.pink.seed(0x1E45_0021 + i as u64 * 3_907);
            g.hp.prepare(fs, HP_HZ, 0.707);
            g.os.prepare();
        }
        self.dice.seed(0x5EED_1E45);
        self.wander.prepare(fs);
        self.wander.set_shape(LfoShape::Triangle);
        self.wander.set_rate(WANDER_HZ);

        // The ensemble.
        self.split_l.prepare(fs, p::ENSEMBLE_SPLIT_HZ, fs * 0.45);
        self.split_r.prepare(fs, p::ENSEMBLE_SPLIT_HZ, fs * 0.45);
        let max_delay = ((p::ENSEMBLE_BASE_MS + p::ENSEMBLE_SWING_MS + 2.0) * 0.001 * fs) as usize;
        self.store_l.clear();
        self.store_l.resize(buffer_len(max_delay), 0.0);
        self.store_r.clear();
        self.store_r.resize(buffer_len(max_delay), 0.0);
        self.chorus_l.prepare(max_delay);
        self.chorus_r.prepare(max_delay);
        self.sweep_l.prepare(fs);
        self.sweep_r.prepare(fs);
        for (lfo, phase) in [(&mut self.sweep_l, 0.0f32), (&mut self.sweep_r, 0.25)] {
            lfo.set_shape(LfoShape::Sine);
            lfo.set_rate(p::ENSEMBLE_HZ);
            lfo.set_phase(phase);
        }

        self.rebuild();
        self.all_sound_off();
    }

    /// The `prepare` the graph calls after a rate change.
    pub fn prepare(&mut self) {
        let fs = self.sample_rate;
        self.prepare_at(fs);
    }

    /// Green zone in spirit, alloc-free in fact — it is also called from
    /// `render` the moment a knob moves, so nothing here may allocate.
    fn rebuild(&mut self) {
        let fs = self.sample_rate;
        let v = &self.params;
        for g in self.groups.iter_mut() {
            g.amp.prepare(fs, v.attack, v.decay, v.sustain, v.release);
            // The filter envelope is quicker than the amp's on the way in
            // and quicker on the way out. A classic poly wires that in
            // rather than giving the second envelope its own four knobs,
            // and it is most of why one set of times sounds right on
            // both.
            g.fenv.prepare(
                fs,
                v.attack * 0.55,
                v.decay * 0.7,
                v.sustain * 0.8,
                v.release * 0.8,
            );
            g.noise_tone
                .prepare(fs, (1_400.0 + 9_000.0 * v.noise).clamp(60.0, fs * 0.45));
        }
        // Fold on the triangle curve. Drive is set per chunk from WARP;
        // this only fixes the shape and the mix.
        self.shaper.configure(ShapeMode::Fold, 1.0, 0.0, 1.0);
        self.prepared = Resolved::of(&self.params);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.base.set(param, value);
    }

    /// A note's own override, or `None` to fall back to the live knob.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        match value {
            Some(v) => self.params.set(param, v),
            None => {
                if let Some(v) = self.base.get(param) {
                    self.params.set(param, v);
                }
            }
        }
    }

    pub fn params(&self) -> &LensParams {
        &self.params
    }

    /// What the card draws: how many voices are sounding.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [self.said_voices, 0.0, 0.0],
        }
    }

    /// The right channel of the last render.
    pub fn right(&self, len: usize) -> &[f32] {
        self.stash.get(..len.min(self.stash.len())).unwrap_or(&[])
    }

    /// The constant delay the oversampled fold costs. Reported to the
    /// graph so the compensator can line this instrument up with the
    /// tracks that have no lens in them.
    pub fn latency() -> usize {
        LaneOversampler2x::new().latency()
    }

    pub fn all_sound_off(&mut self) {
        for g in self.groups.iter_mut() {
            g.reset();
        }
        self.chorus_l.reset();
        self.chorus_r.reset();
        for slot in self.store_l.iter_mut().chain(self.store_r.iter_mut()) {
            *slot = 0.0;
        }
        self.split_l.reset();
        self.split_r.reset();
        self.said_voices = 0.0;
    }

    pub fn release_all(&mut self) {
        for g in self.groups.iter_mut() {
            for lane in 0..LANES {
                if g.held.get(lane).copied().unwrap_or(false) {
                    g.amp.gate_off(lane);
                    g.fenv.gate_off(lane);
                    if let Some(slot) = g.held.get_mut(lane) {
                        *slot = false;
                    }
                }
            }
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        for g in self.groups.iter_mut() {
            for lane in 0..LANES {
                if g.held.get(lane).copied().unwrap_or(false)
                    && g.pitch.get(lane).copied().unwrap_or(0) == pitch
                {
                    g.amp.gate_off(lane);
                    g.fenv.gate_off(lane);
                    if let Some(slot) = g.held.get_mut(lane) {
                        *slot = false;
                    }
                }
            }
        }
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        // A free lane if there is one, otherwise the oldest — the same
        // age-stamped stealing `Seq` uses.
        let mut best: Option<(usize, usize)> = None;
        let mut oldest: Option<(usize, usize, u64)> = None;
        for (gi, g) in self.groups.iter().enumerate() {
            for lane in 0..LANES {
                if !g.amp.active(lane) && best.is_none() {
                    best = Some((gi, lane));
                }
                let stamp = g.age.get(lane).copied().unwrap_or(0);
                if oldest.is_none_or(|(_, _, o)| stamp < o) {
                    oldest = Some((gi, lane, stamp));
                }
            }
        }
        let Some((gi, lane)) = best.or(oldest.map(|(g, l, _)| (g, l))) else {
            return;
        };
        // ONE number, before the group is borrowed: this voice's place on
        // the drift axis for as long as the note lasts.
        let mut roll = [0.0f32; 1];
        self.dice.process(&mut roll);
        let seat = roll.first().copied().unwrap_or(0.0).clamp(-1.0, 1.0);
        let hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        let Some(g) = self.groups.get_mut(gi) else {
            return;
        };
        // A stolen voice starts clean. The oscillators keep their phase —
        // free-running oscillators are half of why an analog poly does
        // not sound like a sampler — but the filter and the oversampler
        // do not, because carrying the last note's state into this one is
        // a click, not a character.
        g.reset_lane(lane);
        for (slot, value) in [
            (g.hz.get_mut(lane), hz.clamp(8.0, self.sample_rate * 0.45)),
            (g.vel.get_mut(lane), f32::from(vel.max(1)) / 127.0),
            (g.drift.get_mut(lane), seat),
        ] {
            if let Some(slot) = slot {
                *slot = value;
            }
        }
        if let Some(slot) = g.pitch.get_mut(lane) {
            *slot = pitch;
        }
        if let Some(slot) = g.age.get_mut(lane) {
            *slot = age;
        }
        if let Some(slot) = g.held.get_mut(lane) {
            *slot = true;
        }
        g.amp.gate_on(lane);
        g.fenv.gate_on(lane);
    }

    /// Red zone: render the LEFT channel, stash the right.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut crate::audio::graph::Ramp) {
        if self.prepared != Resolved::of(&self.params) {
            self.rebuild();
        }
        let n = out.len();
        if n == 0 {
            return;
        }
        let fs = self.sample_rate;
        let v = self.params;

        // The lens, once per block, from the shared functions.
        let pm_depth = p::pm_depth(v.warp, v.bend);
        let fold_drive = p::fold_drive(v.warp, v.bend);
        let ring_depth = p::ring_depth(v.warp, v.bend);
        let duty = p::duty(v.width);
        // A folder run harder is louder as well as brighter; taking the
        // square root back out leaves the part of that which is the
        // sound and removes the part which is a volume knob.
        let fold_makeup = 1.0 / fold_drive.max(1.0).sqrt();
        self.shaper.configure(ShapeMode::Fold, fold_drive, 0.0, 1.0);

        let mix = v.mix.clamp(0.0, 1.0);
        let sub_level = v.sub.clamp(0.0, 1.0) * 0.7;
        let noise_level = v.noise.clamp(0.0, 1.0) * 0.5;
        let drift = v.drift.clamp(0.0, 1.0);
        let reso = v.reso.clamp(0.0, 1.0);
        // Butterworth at the bottom, screaming at the top. The ladder
        // loses its bottom end as it goes, which is the character and not
        // a defect, so only the excess is taken back out.
        let q = core::f32::consts::FRAC_1_SQRT_2 + reso * reso * 11.0;
        let reso_makeup = 1.0 / (1.0 + reso * 1.1);

        // The shared wander, one value for the whole run.
        let wander = {
            let Some(buf) = self.wander_buf.get_mut(..1) else {
                return;
            };
            self.wander.process(buf);
            buf.first().copied().unwrap_or(0.0)
        };

        for slot in self
            .left
            .iter_mut()
            .take(n)
            .chain(self.right.iter_mut().take(n))
        {
            *slot = 0.0;
        }

        let mut sounding = 0usize;
        for gi in 0..self.groups.len() {
            let Some(g) = self.groups.get_mut(gi) else {
                continue;
            };
            sounding += (0..LANES).filter(|l| g.amp.active(*l)).count();

            // Per-lane seating and per-lane pitch, worked out once.
            let mut gl = [0.0f32; LANES];
            let mut gr = [0.0f32; LANES];
            for lane in 0..LANES {
                let seat = if LANES > 1 {
                    lane as f32 / (LANES - 1) as f32 * 2.0 - 1.0
                } else {
                    0.0
                };
                let (l, r) = crate::dsp::pan::spread(seat * SEAT_SPREAD);
                gl[lane] = l;
                gr[lane] = r;
            }

            let mut base_hz = [220.0f32; LANES];
            let mut a_hz = [220.0f32; LANES];
            let mut b_hz = [220.0f32; LANES];
            let mut sub_hz = [110.0f32; LANES];
            let mut ring_hz = [220.0f32; LANES];
            let mut warp_lane = [0.0f32; LANES];
            for lane in 0..LANES {
                // ONE drift number per voice, on three destinations. The
                // wander moves every voice together on top of it.
                let d = g.drift.get(lane).copied().unwrap_or(0.0) * drift;
                let cents = (d + wander * 0.4) * p::DRIFT_CENTS;
                let hz = g.hz.get(lane).copied().unwrap_or(220.0) * (cents / 1_200.0).exp2();
                let limit = fs * 0.45;
                base_hz[lane] = hz;
                // The two oscillators sit either side of the note rather
                // than one above it, so DETUNE widens the sound without
                // moving where it is.
                a_hz[lane] = (hz * (-v.detune * 0.35 / 1_200.0).exp2()).clamp(8.0, limit);
                b_hz[lane] = (hz * (v.detune / 1_200.0).exp2()).clamp(8.0, limit);
                sub_hz[lane] = (hz * 0.5).clamp(4.0, limit);
                ring_hz[lane] = (hz * p::RING_RATIO).clamp(8.0, limit);
                warp_lane[lane] = (d * p::DRIFT_WARP).clamp(-1.0, 1.0);
            }
            g.saw.set_freqs(&a_hz);
            g.pulse_a.set_freqs(&b_hz);
            g.pulse_b.set_freqs(&b_hz);
            g.sub.set_freqs(&sub_hz);
            g.warp_osc.set_freqs(&a_hz);
            g.ring_osc.set_freqs(&ring_hz);

            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(CHUNK);
                let Some(g) = self.groups.get_mut(gi) else {
                    break;
                };

                // The filter, per lane, per chunk: cutoff is the knob,
                // the envelope, the keyboard and this voice's drift.
                {
                    let Some(fen) = g.fen.get_mut(..take) else {
                        break;
                    };
                    g.fenv.process(fen);
                }
                let mut cut = [1_000.0f32; LANES];
                for (lane, corner) in cut.iter_mut().enumerate() {
                    let e = g
                        .fen
                        .first()
                        .and_then(|f| f.get(lane))
                        .copied()
                        .unwrap_or(0.0);
                    let key =
                        (base_hz.get(lane).copied().unwrap_or(220.0) / 261.63).log2() * KEYTRACK;
                    let d = g.drift.get(lane).copied().unwrap_or(0.0) * drift * p::DRIFT_CUTOFF;
                    let vel = g.vel.get(lane).copied().unwrap_or(0.0);
                    // Octaves, all of them, so the sum is one exponent.
                    let octaves = key + v.env * e * 6.0 + d + vel * 0.6 - 0.3;
                    *corner = (v.cutoff * octaves.exp2()).clamp(p::CUTOFF_MIN_HZ, fs * 0.45);
                }
                g.ladder.prepare_lanes(fs, &cut, q, 4, false);

                // ---- the oscillators ---------------------------------
                //
                // The warp modulator first: a sine locked to the voice's
                // own pitch, whose output IS the phase warp.
                {
                    let (Some(warp_buf), Some(warp_duty)) =
                        (g.warp_buf.get_mut(..take), g.warp_duty.get_mut(..take))
                    else {
                        break;
                    };
                    warp_buf.fill([0.0; LANES]);
                    g.warp_osc.process(warp_buf, None, &self.sine_tables);
                    for (frame, dutied) in warp_buf.iter_mut().zip(warp_duty.iter_mut()) {
                        for lane in 0..LANES {
                            let s = frame.get(lane).copied().unwrap_or(0.0);
                            let depth =
                                pm_depth * (1.0 + warp_lane.get(lane).copied().unwrap_or(0.0));
                            let turns = s * depth;
                            if let Some(slot) = frame.get_mut(lane) {
                                *slot = turns;
                            }
                            // The SAME warp plus the duty cycle: one
                            // constant offset is the whole pulse width.
                            if let Some(slot) = dutied.get_mut(lane) {
                                *slot = turns + duty;
                            }
                        }
                    }
                }

                let (Some(osc), Some(leg)) = (g.osc.get_mut(..take), g.leg.get_mut(..take)) else {
                    break;
                };
                osc.fill([0.0; LANES]);
                leg.fill([0.0; LANES]);
                let Some(warp_buf) = g.warp_buf.get(..take) else {
                    break;
                };
                // Osc A: the saw, warped.
                g.saw.process(osc, Some(warp_buf), &self.saw_tables);
                for frame in osc.iter_mut() {
                    for s in frame.iter_mut() {
                        *s *= 1.0 - mix;
                    }
                }
                // Osc B: two saw readers a duty apart, subtracted.
                g.pulse_a.process(leg, Some(warp_buf), &self.saw_tables);
                for (dst, src) in osc.iter_mut().zip(leg.iter()) {
                    for (a, b) in dst.iter_mut().zip(src.iter()) {
                        *a += *b * mix;
                    }
                }
                leg.fill([0.0; LANES]);
                let Some(warp_duty) = g.warp_duty.get(..take) else {
                    break;
                };
                g.pulse_b.process(leg, Some(warp_duty), &self.saw_tables);
                for (dst, src) in osc.iter_mut().zip(leg.iter()) {
                    for (a, b) in dst.iter_mut().zip(src.iter()) {
                        *a -= *b * mix;
                    }
                }
                // The sub.
                if sub_level > 0.0 {
                    leg.fill([0.0; LANES]);
                    g.sub.process(leg, None, &self.square_tables);
                    for (dst, src) in osc.iter_mut().zip(leg.iter()) {
                        for (a, b) in dst.iter_mut().zip(src.iter()) {
                            *a += *b * sub_level;
                        }
                    }
                }
                // The noise, tamed.
                if noise_level > 0.0 {
                    let Some(nz) = g.nz.get_mut(..take) else {
                        break;
                    };
                    // Pink under white: pink alone has no top and white
                    // alone has no body, and the crossfade is the whole
                    // of what NOISE's colour needs to be.
                    g.pink.process(nz);
                    for frame in nz.iter_mut() {
                        for s in frame.iter_mut() {
                            *s *= 0.6;
                        }
                    }
                    g.noise_tone.process_lowpass(nz);
                    for (dst, src) in osc.iter_mut().zip(nz.iter()) {
                        for (a, b) in dst.iter_mut().zip(src.iter()) {
                            *a += *b * noise_level;
                        }
                    }
                }

                // ---- THE LENS: fold and ring, oversampled ------------
                {
                    let two = take * 2;
                    let Some(up) = g.up.get_mut(..two) else {
                        break;
                    };
                    let Some(osc) = g.osc.get(..take) else {
                        break;
                    };
                    g.os.up(osc, up);
                    // Drive one is the folder's OFF position. The pulse
                    // can legitimately exceed one on its short side while
                    // remaining DC-free; passing that through even a
                    // drive-one triangle reflects it and invents DC. Off
                    // therefore skips the colour arithmetic altogether.
                    process_fold(&self.shaper, fold_drive, fold_makeup, up);
                    if ring_depth > 0.0 {
                        let Some(ring) = g.ring_buf.get_mut(..two) else {
                            break;
                        };
                        ring.fill([0.0; LANES]);
                        g.ring_osc.process(ring, None, &self.sine_tables);
                        let Some(up) = g.up.get_mut(..two) else {
                            break;
                        };
                        let Some(ring) = g.ring_buf.get(..two) else {
                            break;
                        };
                        for (dst, m) in up.iter_mut().zip(ring.iter()) {
                            for (a, b) in dst.iter_mut().zip(m.iter()) {
                                *a = *a * (1.0 - ring_depth) + *a * *b * ring_depth;
                            }
                        }
                    }
                    let Some(up) = g.up.get(..two) else {
                        break;
                    };
                    let Some(osc) = g.osc.get_mut(..take) else {
                        break;
                    };
                    g.os.down(up, osc);
                }

                // ---- the ladder, then the amp ------------------------
                let Some(osc) = g.osc.get_mut(..take) else {
                    break;
                };
                g.ladder.process(osc);
                g.hp.process(osc, FilterMode::Highpass);
                let Some(env) = g.env.get_mut(..take) else {
                    break;
                };
                g.amp.process(env);

                let (Some(osc), Some(env)) = (g.osc.get(..take), g.env.get(..take)) else {
                    break;
                };
                for i in 0..take {
                    let frame = osc.get(i).copied().unwrap_or([0.0; LANES]);
                    let amp = env.get(i).copied().unwrap_or([0.0; LANES]);
                    let mut l = 0.0f32;
                    let mut r = 0.0f32;
                    for lane in 0..LANES {
                        let s = frame.get(lane).copied().unwrap_or(0.0)
                            * amp.get(lane).copied().unwrap_or(0.0)
                            * g.vel.get(lane).copied().unwrap_or(0.0)
                            * reso_makeup;
                        l += s * gl.get(lane).copied().unwrap_or(0.0);
                        r += s * gr.get(lane).copied().unwrap_or(0.0);
                    }
                    if let Some(slot) = self.left.get_mut(done + i) {
                        *slot += l * BANK_GAIN;
                    }
                    if let Some(slot) = self.right.get_mut(done + i) {
                        *slot += r * BANK_GAIN;
                    }
                }
                done += take;
            }
        }
        self.said_voices = sounding as f32;

        // ---- ENSEMBLE -------------------------------------------------
        //
        // Skipped whole at zero, so the dry path is bit-exact rather than
        // approximately unchanged.
        let ensemble = v.ensemble.clamp(0.0, 1.0);
        if ensemble > 0.0 {
            let base = p::ENSEMBLE_BASE_MS * 0.001 * fs;
            let swing = p::ENSEMBLE_SWING_MS * 0.001 * fs * ensemble;
            for (lfo, delays) in [
                (&mut self.sweep_l, &mut self.delays_l),
                (&mut self.sweep_r, &mut self.delays_r),
            ] {
                let Some(buf) = delays.get_mut(..n) else {
                    return;
                };
                lfo.process(buf);
                for slot in buf.iter_mut() {
                    *slot = (base + *slot * swing).max(1.0);
                }
            }
            // Split first: the bottom of the sound does not swim.
            {
                let (Some(src), Some(low), Some(mid), Some(high)) = (
                    self.left.get(..n),
                    self.low_l.get_mut(..n),
                    self.mid_l.get_mut(..n),
                    self.high_l.get_mut(..n),
                ) else {
                    return;
                };
                self.split_l.process(src, low, mid, high);
            }
            {
                let (Some(src), Some(low), Some(mid), Some(high)) = (
                    self.right.get(..n),
                    self.low_r.get_mut(..n),
                    self.mid_r.get_mut(..n),
                    self.high_r.get_mut(..n),
                ) else {
                    return;
                };
                self.split_r.process(src, low, mid, high);
            }
            // Keep both the dry and chorused versions of everything above
            // the split. The crossover outputs reconstruct with flat
            // magnitude but allpass phase, so replacing the whole signal
            // with `low + wet_high` would move the bass even when the high
            // band is negligible. Adding only the high-band DELTA to the
            // original leaves the bottom alone.
            for i in 0..n {
                let top_l = self.mid_l.get(i).copied().unwrap_or(0.0)
                    + self.high_l.get(i).copied().unwrap_or(0.0);
                let top_r = self.mid_r.get(i).copied().unwrap_or(0.0)
                    + self.high_r.get(i).copied().unwrap_or(0.0);
                if let Some(slot) = self.mid_l.get_mut(i) {
                    *slot = top_l;
                }
                if let Some(slot) = self.high_l.get_mut(i) {
                    *slot = top_l;
                }
                if let Some(slot) = self.mid_r.get_mut(i) {
                    *slot = top_r;
                }
                if let Some(slot) = self.high_r.get_mut(i) {
                    *slot = top_r;
                }
            }
            {
                let (Some(io), Some(delays)) = (self.mid_l.get_mut(..n), self.delays_l.get(..n))
                else {
                    return;
                };
                self.chorus_l
                    .process_modulated(io, &mut self.store_l, delays);
            }
            {
                let (Some(io), Some(delays)) = (self.mid_r.get_mut(..n), self.delays_r.get(..n))
                else {
                    return;
                };
                self.chorus_r
                    .process_modulated(io, &mut self.store_r, delays);
            }
            for i in 0..n {
                let dl = self.mid_l.get(i).copied().unwrap_or(0.0)
                    - self.high_l.get(i).copied().unwrap_or(0.0);
                let dr = self.mid_r.get(i).copied().unwrap_or(0.0)
                    - self.high_r.get(i).copied().unwrap_or(0.0);
                if let Some(slot) = self.left.get_mut(i) {
                    *slot += dl * ensemble * 0.5;
                }
                if let Some(slot) = self.right.get_mut(i) {
                    *slot += dr * ensemble * 0.5;
                }
            }
        }

        // OUT. The gain ramp advances once per sample on every path,
        // silence included — the clock's contract.
        let level = v.level;
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

    fn voices(edit: impl Fn(&mut LensParams)) -> LensVoices {
        let mut params = LensParams::default();
        edit(&mut params);
        LensVoices::new(FS, 512, params)
    }

    /// Render `n` samples, flat gain, and hand back the left channel.
    fn run(v: &mut LensVoices, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        let mut done = 0usize;
        while done < n {
            let take = (n - done).min(512);
            let mut ramp = Ramp::across(1.0, 1.0, take);
            let Some(slice) = out.get_mut(done..done + take) else {
                break;
            };
            v.render(slice, 0, &mut ramp);
            done += take;
        }
        out
    }

    /// Energy at one frequency, by Goertzel.
    fn amp(x: &[f32], hz: f32) -> f64 {
        let n = x.len();
        if n == 0 {
            return 0.0;
        }
        let w = core::f64::consts::TAU * (hz / FS) as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for v in x {
            let s0 = *v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / n as f64
    }

    fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// Everything above `hz`, as a share of everything.
    fn brightness(x: &[f32], f0: f32, from: usize) -> f64 {
        let mut top = 0.0f64;
        let mut all = 0.0f64;
        for h in 1..=24 {
            let e = amp(x, f0 * h as f32);
            all += e;
            if h >= from {
                top += e;
            }
        }
        if all <= 0.0 { 0.0 } else { top / all }
    }

    // --------------------------------------------------- the lens -----

    /// THE LOAD-BEARING PROMISE: at WARP zero every regime is off, and
    /// off means the arithmetic identity, not "quiet".
    ///
    /// Stated over the shared functions rather than over a rendered
    /// buffer because that is where the promise lives — a colour that
    /// cannot be switched exactly off cannot be measured, only asserted.
    #[test]
    fn warp_zero_is_the_identity_in_all_three_regimes() {
        for i in 0..=40 {
            let bend = i as f32 / 40.0;
            assert_eq!(p::pm_depth(0.0, bend), 0.0, "phase at bend {bend}");
            assert_eq!(p::fold_drive(0.0, bend), 1.0, "fold at bend {bend}");
            assert_eq!(p::ring_depth(0.0, bend), 0.0, "ring at bend {bend}");
        }
        // And a folder at drive one is the identity to the bit, inside
        // the rails, which is the half of the promise the kernel owns.
        let mut shaper = Waveshaper::new();
        shaper.configure(ShapeMode::Fold, 1.0, 0.0, 1.0);
        for i in 0..=200 {
            let x = i as f32 / 100.0 - 1.0;
            assert_eq!(shaper.shape(x), x, "the folder moved {x}");
        }
        // A narrow, DC-free pulse legitimately has one rail above one.
        // The NODE bypass must leave that alone too; the shaper itself is
        // still allowed to fold out-of-rail input at drive one.
        let mut pulse = [[-0.96, 1.04, -0.96, 1.04, -0.96, 1.04, -0.96, 1.04]];
        let before = pulse;
        process_fold(&shaper, 1.0, 1.0, &mut pulse);
        assert_eq!(pulse, before, "the off fold touched the pulse rails");
    }

    /// The card draws the fold with its own copy of the curve, because
    /// the UI may not reach into `dsp`. This is the test that keeps the
    /// two from becoming two different folders.
    #[test]
    fn the_card_and_the_kernel_fold_the_same_curve() {
        let mut shaper = Waveshaper::new();
        shaper.configure(ShapeMode::Fold, 1.0, 0.0, 1.0);
        for i in 0..=2_000 {
            let x = i as f32 / 100.0 - 10.0;
            assert_eq!(
                p::fold(x),
                shaper.shape(x),
                "the card and the kernel disagree at {x}"
            );
        }
    }

    /// BEND is a morph, not a menu: the weights sum to one everywhere,
    /// each regime peaks alone, and the name follows the weights.
    #[test]
    fn bend_crosses_three_regimes_without_a_gap() {
        for i in 0..=100 {
            let bend = i as f32 / 100.0;
            let mix = p::regime_mix(bend);
            let sum: f32 = mix.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "at {bend} the weights sum to {sum}"
            );
            assert!(mix.iter().all(|w| *w >= 0.0));
        }
        assert_eq!(p::regime_mix(0.0), [1.0, 0.0, 0.0]);
        assert_eq!(p::regime_mix(1.0), [0.0, 0.0, 1.0]);
        let half = p::regime_mix(0.5);
        assert!(half[1] > 0.99, "the middle of the knob must BE the fold");
        assert_eq!(p::regime_name(0.0), "phase");
        assert_eq!(p::regime_name(0.5), "fold");
        assert_eq!(p::regime_name(1.0), "ring");
    }

    /// The whole reason the device exists: turning WARP up materially
    /// redraws the spectrum. A saw already occupies every harmonic bin,
    /// so a folder is allowed to move energy DOWN as well as up; requiring
    /// one chosen upper-band share to increase mistakes "different" for
    /// "brighter". The separate ring test below retains the stricter
    /// promise that its new energy lands between partials.
    #[test]
    fn warp_adds_harmonics_that_were_not_there() {
        let f0 = 110.0;
        let mut quiet = 0.0f32;
        for (bend, regime) in [(0.0, "phase"), (0.5, "fold"), (1.0, "ring")] {
            let sound = |warp: f32| {
                let mut v = voices(|p| {
                    // One in-tune oscillator makes the harmonic bins a
                    // measurement of the lens, not of DETUNE sidebands or
                    // pulse/saw cancellation between those bins.
                    p.mix = 0.0;
                    p.detune = 0.0;
                    p.warp = warp;
                    p.bend = bend;
                    p.cutoff = 18_000.0;
                    p.reso = 0.0;
                    p.env = 0.0;
                    p.sub = 0.0;
                    p.noise = 0.0;
                    p.drift = 0.0;
                    p.ensemble = 0.0;
                    p.attack = 1.0;
                });
                v.note_on(45, 100, 1);
                let mut out = run(&mut v, 16_384);
                out.split_off(4_096)
            };
            let dry = sound(0.0);
            let wet = sound(0.95);
            let dry_rms = rms(&dry).max(1e-12);
            let wet_rms = rms(&wet).max(1e-12);
            // Quarter-harmonic probes see both the harmonic regimes and
            // RING's deliberately inharmonic sidebands. RMS normalization
            // makes this a shape measurement rather than a level test.
            let distance: f64 = (4..=96)
                .map(|quarter| {
                    let hz = f0 * quarter as f32 * 0.25;
                    (amp(&wet, hz) / wet_rms - amp(&dry, hz) / dry_rms).abs()
                })
                .sum();
            assert!(
                distance > 0.5,
                "{regime}: normalized spectral distance was only {distance:.4}"
            );
            quiet = quiet.max(peak(&wet));
        }
        assert!(
            quiet.is_finite() && quiet < 4.0,
            "the lens ran away: {quiet}"
        );
    }

    /// RING is the one regime that is not a harmonic effect. It has to
    /// put energy BETWEEN the partials, or it is just another folder.
    #[test]
    fn ring_lands_between_the_partials() {
        let f0 = 110.0;
        let sound = |bend: f32| {
            let mut v = voices(|p| {
                p.warp = 1.0;
                p.bend = bend;
                p.cutoff = 18_000.0;
                p.env = 0.0;
                p.sub = 0.0;
                p.drift = 0.0;
                p.ensemble = 0.0;
                p.attack = 1.0;
            });
            v.note_on(45, 100, 1);
            let mut out = run(&mut v, 16_384);
            out.split_off(4_096)
        };
        // The sum tone the ring modulator makes with the fundamental:
        // 1 + RING_RATIO of the note, which is not a partial of it.
        let between = f0 * (1.0 + p::RING_RATIO);
        let fold = sound(0.5);
        let ring = sound(1.0);
        let a = amp(&fold, between) / rms(&fold).max(1e-12);
        let b = amp(&ring, between) / rms(&ring).max(1e-12);
        assert!(
            b > a * 3.0,
            "the ring put {b:.5} at {between:.0} Hz where the fold puts {a:.5}"
        );
    }

    // -------------------------------------------------- the tone ------

    /// WIDTH builds a pulse out of two saws, so the shape it makes has to
    /// actually BE a pulse: dc-free at every width, and spending the
    /// right fraction of the cycle high.
    #[test]
    fn width_makes_a_real_pulse() {
        let mut cycle = vec![0.0f32; 4_096];
        for i in 0..=20 {
            let width = i as f32 / 20.0;
            let duty = p::duty(width);
            let saw = |phase: f32| {
                let t = phase - phase.floor();
                t * 2.0 - 1.0
            };
            for (sample, at) in cycle.iter_mut().zip(0..) {
                let phase = at as f32 / 4_096.0;
                // This is the voice's pulse path before the lens. At warp
                // zero the folder is structurally bypassed, so no colour
                // stage may reflect its deliberately asymmetric rails.
                *sample = saw(phase) - saw(phase + duty);
            }
            let mean: f32 = cycle.iter().sum::<f32>() / cycle.len() as f32;
            assert!(mean.abs() < 0.02, "width {width} carries {mean} of dc");
            let high = cycle.iter().filter(|s| **s > 0.0).count() as f32 / cycle.len() as f32;
            assert!(
                (high - duty).abs() < 0.02,
                "width {width} asks for {duty:.3} duty and draws {high:.3}"
            );
        }
        // And a square is the top of the knob's other end.
        assert!((p::duty(0.0) - 0.5).abs() < 1e-6);
    }

    /// Two oscillators a few cents apart beat. That is what DETUNE is
    /// for, and it is visible as a slow envelope on a held note.
    #[test]
    fn detune_beats_and_zero_does_not() {
        let held = |cents: f32| {
            let mut v = voices(|p| {
                p.detune = cents;
                p.mix = 0.5;
                p.warp = 0.0;
                p.sub = 0.0;
                p.drift = 0.0;
                p.ensemble = 0.0;
                p.cutoff = 18_000.0;
                p.env = 0.0;
                p.attack = 1.0;
                p.sustain = 1.0;
            });
            v.note_on(57, 100, 1);
            let mut out = run(&mut v, 96_000);
            out.split_off(24_000)
        };
        // The swing of the envelope over a second, as a share of its
        // mean: beating IS that swing.
        let swing = |x: &[f32]| {
            let window = 2_048;
            let levels: Vec<f64> = x.chunks(window).map(rms).filter(|r| *r > 0.0).collect();
            let hi = levels.iter().cloned().fold(0.0f64, f64::max);
            let lo = levels.iter().cloned().fold(f64::INFINITY, f64::min);
            if hi <= 0.0 { 0.0 } else { (hi - lo) / hi }
        };
        let still = swing(&held(0.0));
        let beating = swing(&held(24.0));
        assert!(
            beating > still + 0.15,
            "detuned swings {beating:.3}, in tune swings {still:.3}"
        );
    }

    /// The ladder is a ladder: closing it takes the top off, and
    /// resonance puts a peak back at the corner.
    #[test]
    fn the_filter_closes_and_resonates() {
        let f0 = 110.0;
        let sound = |cutoff: f32, reso: f32| {
            let mut v = voices(|p| {
                p.cutoff = cutoff;
                p.reso = reso;
                p.env = 0.0;
                p.warp = 0.0;
                p.drift = 0.0;
                p.ensemble = 0.0;
                p.attack = 1.0;
                p.sustain = 1.0;
            });
            v.note_on(45, 100, 1);
            let mut out = run(&mut v, 16_384);
            out.split_off(4_096)
        };
        let open = sound(18_000.0, 0.0);
        let shut = sound(300.0, 0.0);
        assert!(
            brightness(&shut, f0, 6) < brightness(&open, f0, 6) * 0.35,
            "closing the ladder did not take the top off"
        );
        // Resonance at the corner: park the cutoff on the fifth partial
        // and it should come up relative to its neighbours.
        let corner = f0 * 5.0;
        // CUTOFF is the base corner. The voice then applies the fixed
        // keyboard tracking and velocity opening documented in `render`,
        // so compensate those before asking the fifth partial to sit at
        // the actual corner.
        let key = (f0 / 261.63).log2() * KEYTRACK;
        let velocity = (100.0f32 / 127.0) * 0.6 - 0.3;
        let cutoff_knob = corner / (key + velocity).exp2();
        let flat = sound(cutoff_knob, 0.0);
        let sharp = sound(cutoff_knob, 0.95);
        let ratio = |x: &[f32]| amp(x, corner) / (amp(x, f0 * 3.0) + 1e-12);
        assert!(
            ratio(&sharp) > ratio(&flat) * 1.8,
            "resonance did not lift the corner: {:.3} vs {:.3}",
            ratio(&sharp),
            ratio(&flat)
        );
    }

    /// The ensemble splits the band before it choruses, so a note under
    /// the split is left nearly alone and one above it swims.
    #[test]
    fn the_ensemble_leaves_the_bottom_alone() {
        let moved = |pitch: u8| {
            let patch = |ensemble: f32| {
                let mut v = voices(|p| {
                    p.ensemble = ensemble;
                    p.warp = 0.0;
                    p.drift = 0.0;
                    p.sub = 0.0;
                    p.cutoff = 18_000.0;
                    p.env = 0.0;
                    p.attack = 1.0;
                    p.sustain = 1.0;
                });
                v.note_on(pitch, 100, 1);
                let mut out = run(&mut v, 48_000);
                out.split_off(12_000)
            };
            let dry = patch(0.0);
            let wet = patch(1.0);
            let diff: Vec<f32> = dry.iter().zip(wet.iter()).map(|(a, b)| a - b).collect();
            rms(&diff) / rms(&dry).max(1e-12)
        };
        // MIDI 33 is 55 Hz, well under the split; 81 is 880 Hz, well over.
        let low = moved(33);
        let high = moved(81);
        assert!(
            high > low * 2.5,
            "the split did nothing: the bottom moved {low:.3}, the top {high:.3}"
        );
    }

    /// DRIFT gives every voice its own place, drawn once when the note
    /// starts and held. Checked at the source, because a handful of cents
    /// spread across sixteen voices is exactly the kind of thing a
    /// spectrum test would pass while broken.
    #[test]
    fn every_voice_gets_its_own_drift() {
        let mut v = voices(|p| p.drift = 1.0);
        for i in 0..VOICES {
            v.note_on(48 + i as u8, 100, i as u64);
        }
        let mut seen: Vec<f32> = v
            .groups
            .iter()
            .flat_map(|g| g.drift.iter().copied())
            .collect();
        assert_eq!(seen.len(), VOICES);
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        seen.dedup();
        assert!(
            seen.len() >= VOICES - 1,
            "sixteen voices drew only {} different seats",
            seen.len()
        );
        assert!(seen.iter().all(|d| (-1.0..=1.0).contains(d)));
        // And it really reaches the pitch: the same note with drift up
        // is not the same note.
        let held = |drift: f32| {
            let mut v = voices(|p| {
                p.drift = drift;
                p.warp = 0.0;
                p.ensemble = 0.0;
                p.cutoff = 18_000.0;
                p.env = 0.0;
                p.attack = 1.0;
                p.sustain = 1.0;
            });
            v.note_on(69, 100, 1);
            let mut out = run(&mut v, 32_768);
            amp(&out.split_off(8_192), 440.0)
        };
        let still = held(0.0);
        let moved = held(1.0);
        assert!(
            (moved / still.max(1e-12) - 1.0).abs() > 0.02,
            "drift left the fundamental exactly where it was: {still:.6} vs {moved:.6}"
        );
    }

    // ------------------------------------------------ the machine -----

    #[test]
    fn it_plays_sixteen_notes_and_steals_the_oldest() {
        let mut v = voices(|_| {});
        for i in 0..VOICES {
            v.note_on(48 + i as u8, 100, i as u64);
        }
        let sounding = v
            .groups
            .iter()
            .map(|g| (0..LANES).filter(|l| g.amp.active(*l)).count())
            .sum::<usize>();
        assert_eq!(sounding, VOICES, "the bank did not fill");
        // The seventeenth takes the first one's lane.
        v.note_on(90, 100, VOICES as u64);
        let pitches: Vec<u8> = v
            .groups
            .iter()
            .flat_map(|g| g.pitch.iter().copied())
            .collect();
        assert!(pitches.contains(&90), "the new note found no lane");
        assert!(!pitches.contains(&48), "it stole the wrong one");
    }

    #[test]
    fn the_envelope_sustains_then_releases() {
        let mut v = voices(|p| {
            p.attack = 1.0;
            p.decay = 40.0;
            p.sustain = 0.7;
            p.release = 60.0;
            p.ensemble = 0.0;
        });
        v.note_on(57, 110, 1);
        let held = run(&mut v, 24_000);
        assert!(rms(&held[12_000..]) > 1e-4, "the note never sustained");
        v.note_off(57);
        let after = run(&mut v, 24_000);
        assert!(
            rms(&after[12_000..]) < rms(&held[12_000..]) * 0.02,
            "the note never let go"
        );
    }

    #[test]
    fn silence_stays_silent_and_all_sound_off_is_off() {
        let mut v = voices(|_| {});
        assert_eq!(peak(&run(&mut v, 4_096)), 0.0, "an untouched bank rang");
        v.note_on(60, 100, 1);
        assert!(peak(&run(&mut v, 4_096)) > 1e-4);
        v.all_sound_off();
        let after = run(&mut v, 4_096);
        assert_eq!(
            peak(&after),
            0.0,
            "all-sound-off left {} behind",
            peak(&after)
        );
    }

    /// A whole chord, every stage lit, staying inside the rails.
    #[test]
    fn a_full_bank_stays_inside_the_rails() {
        for bend in [0.0, 0.5, 1.0] {
            let mut v = voices(|p| {
                p.warp = 1.0;
                p.bend = bend;
                p.reso = 1.0;
                p.sub = 1.0;
                p.noise = 0.6;
                p.ensemble = 1.0;
                p.level = p::LEVEL_MAX;
            });
            for i in 0..VOICES {
                v.note_on(36 + i as u8 * 3, 127, i as u64);
            }
            let out = run(&mut v, 24_000);
            let top = peak(&out);
            assert!(top.is_finite(), "bend {bend} went non-finite");
            assert!(top < 8.0, "bend {bend} peaked at {top}");
            assert!(top > 0.05, "bend {bend} barely sounded: {top}");
        }
    }

    /// The level knob is a level knob across its whole range.
    #[test]
    fn the_level_knob_is_monotonic() {
        let at = |level: f32| {
            let mut v = voices(|p| {
                p.level = level;
                p.ensemble = 0.0;
                p.drift = 0.0;
            });
            for i in 0..8 {
                v.note_on(48 + i as u8 * 4, 100, i as u64);
            }
            rms(&run(&mut v, 16_384))
        };
        let mut previous = 0.0;
        for i in 1..=8 {
            let now = at(p::LEVEL_MAX * i as f32 / 8.0);
            assert!(now > previous, "level step {i} did not get louder");
            previous = now;
        }
    }

    #[test]
    fn a_plock_releases_to_the_live_knob() {
        let mut v = voices(|_| {});
        let live = v.params().cutoff;
        v.plock(p::CUTOFF, Some(400.0));
        assert_eq!(v.params().cutoff, 400.0);
        // A knob moved while the lock is on is what the lock releases to.
        v.set_param(p::CUTOFF, 6_000.0);
        v.plock(p::CUTOFF, None);
        assert_eq!(v.params().cutoff, 6_000.0);
        assert_ne!(live, 6_000.0);
    }

    #[test]
    fn every_table_row_is_a_field_that_survives_a_round_trip() {
        let mut params = LensParams::default();
        for def in p::TABLE {
            assert_eq!(
                params.get(def.id),
                Some(def.default),
                "{} did not open on its default",
                def.name
            );
            for at in [def.min, def.default, def.max] {
                params.set(def.id, at);
                assert_eq!(params.get(def.id), Some(at), "{} lost {at}", def.name);
            }
            // Out of range is clamped, never stored.
            params.set(def.id, def.max + 1_000.0);
            assert_eq!(params.get(def.id), Some(def.max), "{} overran", def.name);
            params.set(def.id, f32::NAN);
            assert!(
                params.get(def.id).is_some_and(f32::is_finite),
                "{} took a NaN",
                def.name
            );
        }
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voices(|p| {
            p.warp = 0.8;
            p.bend = 0.5;
            p.noise = 0.4;
            p.sub = 0.5;
            p.ensemble = 0.7;
        });
        for i in 0..VOICES {
            v.note_on(48 + i as u8, 100, i as u64);
        }
        let mut out = vec![0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, 256);
        v.render(&mut out, 0, &mut ramp);
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..30 {
                // A knob moving mid-render is the path that rebuilds
                // coefficients, and it must not allocate either.
                v.set_param(p::CUTOFF, 400.0 + i as f32 * 300.0);
                v.set_param(p::BEND, i as f32 / 30.0);
                let mut ramp = Ramp::across(1.0, 1.0, 256);
                v.render(&mut out, 0, &mut ramp);
            }
        });
    }

    /// Note handling is red-zone too — a note arriving is a letter the
    /// audio thread reads, not a UI event.
    #[test]
    fn note_handling_does_not_allocate() {
        let mut v = voices(|_| {});
        let mut out = vec![0.0f32; 128];
        let mut ramp = Ramp::across(1.0, 1.0, 128);
        v.render(&mut out, 0, &mut ramp);
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..64u8 {
                v.note_on(36 + i % 48, 100, u64::from(i));
                v.note_off(36 + i % 48);
            }
            v.release_all();
            v.all_sound_off();
        });
    }
}
