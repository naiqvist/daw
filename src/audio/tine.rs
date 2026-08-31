//! Tine — the struck-resonator synth.
//!
//! Every other instrument here starts from a waveform and subtracts. This
//! one starts from an IMPULSE and a STRUCTURE: hit a thing, let it ring,
//! and the sound is whatever the thing was. That is the other half of
//! synthesis, and it is where plucks, mallets, bells, bars, glass and
//! bowed pads live.
//!
//! ```text
//!   EXCITE ──▶ RESONATE ──▶ (voices sum) ──▶ BODY ──▶ out
//!   noise burst   modal bank                 the cabinet
//!   or a bow      8 partials                 it is mounted in
//! ```
//!
//! # Why modal and not a waveguide
//!
//! A waveguide is a delay line per voice, and eight voices reading eight
//! delay lines at eight different lengths is a gather over unrelated
//! addresses — precisely the situation `notes/20260827-sampler-brief.md`
//! calls the ONE signed-off exception to the lane rule. A modal bank
//! needs no exception: a partial is a two-pole resonator, so eight
//! partials across eight voices is
//! [`LaneSvf`](crate::dsp::filters::LaneSvf) eight times, lane-major, in
//! exactly the shape `notes/20260825-synth-brief.md` asks for.
//!
//! It is also the better fit for what this instrument is FOR. A modal
//! bank is programmed by saying what the material is, and the partials
//! follow; a waveguide is programmed by saying how long the string is.
//!
//! # Where the sound design is implicit
//!
//! Three of the nine knobs are physics rather than taste:
//!
//! - **MATERIAL** morphs the partials between a string's whole multiples
//!   and a bar's real ratios. That one axis is the difference between a
//!   note and a clang, and everything between them is a real instrument.
//! - **PLACE** is where you hit it. A partial with a node at the striking
//!   point cannot be excited, so `|sin(pi n p)|` is the whole rule — and
//!   it is why picking near the bridge is bright and thin.
//! - **DECAY** rings the partials at rates that fall with mode number and
//!   with pitch, which is why a high note dies faster than a low one on
//!   every real instrument and on this one.
//!
//! # What a partial's Q means here
//!
//! [`LaneSvf`] takes a per-lane CUTOFF and one shared Q, and that
//! constraint is the right physics rather than a compromise: ring time
//! goes as `Q / f`, so a shared Q makes high notes decay faster than low
//! ones by construction. A piano does the same thing for the same reason.
//!
//! # Not here
//!
//! The sample-analysis feature — point it at a recording and take the
//! partials from it — needs the sampler's file plumbing, which is its own
//! piece of work. It is the reason `dsp::fft` is not in this file; there
//! is no honest use for it in a modal bank that has nothing to analyse.
//! `dsp::lofi` is not here either, and would have had to be invented a
//! reason.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::adsr::LaneAdsr;
use crate::dsp::fdn::Fdn;
use crate::dsp::filters::{LaneOnePole, LaneSvf, Mode as FilterMode};
use crate::dsp::noise::LanePinkNoise;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::dsp::{LANES, LaneFrame};
use crate::params::tine as p;

/// Partials per voice.
pub const MODES: usize = p::MODES;
/// Voice groups. Two groups of eight is sixteen voices, which is what the
/// synth brief asks a workhorse for.
pub const GROUPS: usize = 2;
pub const VOICES: usize = GROUPS * LANES;
/// Lane-frames of scratch a render walks at a time.
const CHUNK: usize = 64;

/// The exciter's window, in milliseconds, from the softest mallet to the
/// hardest pick.
const STRIKE_SOFT_MS: f32 = 9.0;
const STRIKE_HARD_MS: f32 = 0.4;
/// And how bright the burst is at each end.
const STRIKE_SOFT_HZ: f32 = 700.0;
const STRIKE_HARD_HZ: f32 = 11_000.0;
/// The reference the shared Q is computed at. Above it notes ring
/// shorter, below it longer — which is the point.
const RING_REF_HZ: f32 = p::RING_REF_HZ;
/// What one struck voice is worth, once the resonator's own gain has
/// been taken back out. A narrow resonator picks up very little of a
/// four-millisecond noise burst — that is true of real ones too, which
/// is why a modal bank always needs a figure here. MEASURED so a single
/// note at the default settings lands around a third of full scale.
const VOICE_MAKEUP: f32 = 13.0;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TineParams {
    pub material: f32,
    pub strike: f32,
    pub place: f32,
    pub decay: f32,
    pub body: f32,
    pub tone: f32,
    pub spread: f32,
    pub tune: f32,
    pub level: f32,
}

impl Default for TineParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            material: d(p::MATERIAL),
            strike: d(p::STRIKE),
            place: d(p::PLACE),
            decay: d(p::DECAY),
            body: d(p::BODY),
            tone: d(p::TONE),
            spread: d(p::SPREAD),
            tune: d(p::TUNE),
            level: d(p::LEVEL),
        }
    }
}

impl TineParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::MATERIAL => self.material = value,
            p::STRIKE => self.strike = value,
            p::PLACE => self.place = value,
            p::DECAY => self.decay = value,
            p::BODY => self.body = value,
            p::TONE => self.tone = value,
            p::SPREAD => self.spread = value,
            p::TUNE => self.tune = value,
            p::LEVEL => self.level = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::MATERIAL => Some(self.material),
            p::STRIKE => Some(self.strike),
            p::PLACE => Some(self.place),
            p::DECAY => Some(self.decay),
            p::BODY => Some(self.body),
            p::TONE => Some(self.tone),
            p::SPREAD => Some(self.spread),
            p::TUNE => Some(self.tune),
            p::LEVEL => Some(self.level),
            _ => None,
        }
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

    /// Whether the exciter sustains rather than being a burst.
    pub fn is_bowed(&self) -> bool {
        self.strike >= p::BOW_AT
    }
}

/// What costs something to rebuild.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    material: f32,
    strike: f32,
    place: f32,
    decay: f32,
    tone: f32,
    body: f32,
}

impl Resolved {
    fn of(v: &TineParams) -> Self {
        Self {
            material: v.material,
            strike: v.strike,
            place: v.place,
            decay: v.decay,
            tone: v.tone,
            body: v.body,
        }
    }
}

/// One group of eight voices, lane-major throughout.
#[derive(Debug, Clone)]
struct Group {
    /// One resonator per partial. Per-lane cutoff (each voice's own
    /// pitch), shared Q (the material's own ring).
    modes: [LaneSvf; MODES],
    amp: LaneAdsr,
    /// The exciter's own envelope — a burst, or a sustain when bowed.
    exciter: LaneAdsr,
    noise: LanePinkNoise,
    colour: LaneOnePole,
    /// Per-lane bookkeeping.
    pitch: [u8; LANES],
    age: [u64; LANES],
    held: [bool; LANES],
    hz: [f32; LANES],
    /// Scratch, all lane-major.
    exc: Vec<LaneFrame>,
    ring: Vec<LaneFrame>,
    voice: Vec<LaneFrame>,
    env: Vec<LaneFrame>,
}

impl Group {
    fn new() -> Self {
        Self {
            modes: [LaneSvf::new(); MODES],
            amp: LaneAdsr::new(),
            exciter: LaneAdsr::new(),
            noise: LanePinkNoise::new(),
            colour: LaneOnePole::new(),
            pitch: [0; LANES],
            age: [0; LANES],
            held: [false; LANES],
            hz: [RING_REF_HZ; LANES],
            exc: vec![[0.0; LANES]; CHUNK],
            ring: vec![[0.0; LANES]; CHUNK],
            voice: vec![[0.0; LANES]; CHUNK],
            env: vec![[0.0; LANES]; CHUNK],
        }
    }

    fn reset(&mut self) {
        for m in self.modes.iter_mut() {
            m.reset();
        }
        self.amp.reset();
        self.exciter.reset();
        self.noise.reset();
        self.colour.reset();
        self.pitch = [0; LANES];
        self.age = [0; LANES];
        self.held = [false; LANES];
        self.hz = [RING_REF_HZ; LANES];
    }

    fn reset_lane(&mut self, lane: usize) {
        for m in self.modes.iter_mut() {
            m.reset_lane(lane);
        }
        self.amp.reset_lane(lane);
        self.exciter.reset_lane(lane);
        self.noise.reset_lane(lane);
        self.colour.reset_lane(lane);
        if let Some(slot) = self.held.get_mut(lane) {
            *slot = false;
        }
    }
}

/// The instrument.
pub struct TineVoices {
    params: TineParams,
    /// The base a p-lock is released back to — the LIVE knob, not a
    /// compile-time snapshot.
    base: TineParams,
    prepared: Resolved,
    sample_rate: f32,
    groups: Vec<Group>,
    /// The partials' ratios and how loud the strike excites each.
    ratios: [f32; MODES],
    mode_gain: [f32; MODES],
    mode_q: [f32; MODES],
    /// The nonlinearity the material brings. Stateless, so it is shared.
    shaper: Waveshaper,
    body: Fdn,
    body_store: Vec<f32>,
    body_l: Vec<f32>,
    body_r: Vec<f32>,
    /// The mono send into the body. A separate buffer because `Fdn`
    /// reads its input and writes two outputs, and the input cannot be
    /// one of them — the first version copied it with `to_vec`, which is
    /// an allocation in the callback.
    body_in: Vec<f32>,
    /// The summed voices, before the body.
    left: Vec<f32>,
    right: Vec<f32>,
    /// The right channel the node reads back after a mono-shaped walk.
    stash: Vec<f32>,
    said_voices: f32,
}

impl TineVoices {
    pub fn new(sample_rate: f32, block: usize, params: TineParams) -> Self {
        let mut me = Self {
            params,
            base: params,
            prepared: Resolved::of(&params),
            sample_rate: 48_000.0,
            groups: (0..GROUPS).map(|_| Group::new()).collect(),
            ratios: p::HARMONIC,
            mode_gain: [1.0; MODES],
            mode_q: [50.0; MODES],
            shaper: Waveshaper::new(),
            body: Fdn::new(),
            body_store: Vec::new(),
            body_l: vec![0.0; CHUNK],
            body_r: vec![0.0; CHUNK],
            body_in: vec![0.0; CHUNK],
            left: vec![0.0; block.max(CHUNK)],
            right: vec![0.0; block.max(CHUNK)],
            stash: vec![0.0; block.max(CHUNK)],
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.prepare_at(sample_rate);
        me
    }

    /// Green zone: everything heap-shaped is born here.
    pub fn prepare_at(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.body_store.clear();
        self.body_store
            .resize(Fdn::buffer_len(self.sample_rate), 0.0);
        self.body.prepare(self.sample_rate, &mut self.body_store);
        for (i, g) in self.groups.iter_mut().enumerate() {
            g.noise.seed(0x71_4E_0001 + i as u64 * 977);
        }
        self.rebuild();
        self.all_sound_off();
    }

    /// The `prepare` the graph calls after a rate change.
    pub fn prepare(&mut self) {
        let fs = self.sample_rate;
        self.prepare_at(fs);
    }

    /// Green zone: the structure the knobs describe.
    fn rebuild(&mut self) {
        let fs = self.sample_rate;
        let v = &self.params;
        let material = v.material.clamp(0.0, 1.0);
        let decay = v.decay.clamp(0.0, 1.0);
        let strike = v.strike.clamp(0.0, 1.0);

        // The partials, and how hard the strike excites each of them.
        for m in 0..MODES {
            self.ratios[m] = p::ratio(material, m);
            self.mode_gain[m] = p::strike_gain(v.place, m)
                // Higher partials are quieter to begin with, or the bank
                // sounds like a bandpass sweep rather than an object.
                / (1.0 + m as f32 * 0.9);
        }

        // Ring times come from the shared table, so the lengths the card
        // draws are the lengths the resonators keep.
        for m in 0..MODES {
            let t60 = p::ring_seconds(material, decay, m);
            // T60 = 6.91 Q / (2 pi f)  =>  Q = 0.909 f T60, at the
            // reference pitch. A shared Q then makes ring time fall with
            // pitch, which is what a real bar does.
            let q = 0.909 * RING_REF_HZ * self.ratios[m] * t60;
            self.mode_q[m] = q.clamp(1.5, 900.0);
        }

        // The exciter: shorter and brighter as the mallet hardens, and
        // sustaining once it becomes a bow.
        let window = STRIKE_SOFT_MS + (STRIKE_HARD_MS - STRIKE_SOFT_MS) * strike;
        let bow = v.is_bowed();
        let colour_hz = (STRIKE_SOFT_HZ + (STRIKE_HARD_HZ - STRIKE_SOFT_HZ) * strike)
            * (2.0f32).powf(v.tone.clamp(-1.0, 1.0));
        for g in self.groups.iter_mut() {
            g.exciter.prepare(
                fs,
                0.15,
                window.max(0.1),
                if bow { 0.85 } else { 0.0 },
                if bow { 60.0 } else { 4.0 },
            );
            // The amp envelope is nearly all release: a struck thing is
            // its own envelope, and an amp stage that shaped it again
            // would be arguing with the resonator.
            g.amp
                .prepare(fs, 0.4, 20.0, 1.0, if bow { 180.0 } else { 900.0 });
            g.colour.prepare(fs, colour_hz.clamp(60.0, 18_000.0));
        }

        // The material's own nonlinearity: metal buzzes, nylon does not.
        self.shaper.configure(
            ShapeMode::SoftClip,
            1.0 + material * 2.2,
            0.0,
            material * 0.5,
        );

        // The cabinet.
        let body = v.body.clamp(0.0, 1.0);
        self.body.set_size(0.35 + body * 0.5);
        self.body.set_decay(0.35 + body * 2.4);
        self.body.set_damping(2_600.0 + 6_000.0 * (1.0 - material));
        self.body.set_diffusion(0.6);
        self.body.set_modulation(0.8);

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

    pub fn params(&self) -> &TineParams {
        &self.params
    }

    /// What the card draws: how many voices are ringing.
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

    pub fn all_sound_off(&mut self) {
        for g in self.groups.iter_mut() {
            g.reset();
        }
        self.body.reset(&mut self.body_store);
        self.said_voices = 0.0;
    }

    pub fn release_all(&mut self) {
        for g in self.groups.iter_mut() {
            for lane in 0..LANES {
                if g.held.get(lane).copied().unwrap_or(false) {
                    g.amp.gate_off(lane);
                    g.exciter.gate_off(lane);
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
                    g.exciter.gate_off(lane);
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
                let ringing = g.amp.active(lane);
                if !ringing && best.is_none() {
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
        let hz = 440.0 * ((f32::from(pitch) - 69.0 + self.params.tune) / 12.0).exp2();
        let Some(g) = self.groups.get_mut(gi) else {
            return;
        };
        // A stolen voice starts clean: a resonator carrying the last
        // note's ring into this one is a glitch, not a legato.
        g.reset_lane(lane);
        if let Some(slot) = g.pitch.get_mut(lane) {
            *slot = pitch;
        }
        if let Some(slot) = g.age.get_mut(lane) {
            *slot = age;
        }
        if let Some(slot) = g.held.get_mut(lane) {
            *slot = true;
        }
        if let Some(slot) = g.hz.get_mut(lane) {
            *slot = hz.clamp(8.0, self.sample_rate * 0.45);
        }
        let _ = vel;
        g.amp.gate_on(lane);
        g.exciter.gate_on(lane);
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
        let level = self.params.level;
        let spread = self.params.spread.clamp(0.0, 1.0);
        let body_amount = self.params.body.clamp(0.0, 1.0);
        let velocity_scale = 0.9;

        // Clear the sum for this run.
        for slot in self
            .left
            .iter_mut()
            .take(n)
            .chain(self.right.iter_mut().take(n))
        {
            *slot = 0.0;
        }

        let mut ringing = 0usize;
        for gi in 0..self.groups.len() {
            // Retune every lane's partials: the cutoffs are per-lane and
            // the Q is the material's.
            {
                let Some(g) = self.groups.get_mut(gi) else {
                    continue;
                };
                for m in 0..MODES {
                    let mut cut = [RING_REF_HZ; LANES];
                    for lane in 0..LANES {
                        let f = g.hz.get(lane).copied().unwrap_or(RING_REF_HZ)
                            * self.ratios.get(m).copied().unwrap_or(1.0);
                        if let Some(slot) = cut.get_mut(lane) {
                            *slot = f.clamp(10.0, self.sample_rate * 0.47);
                        }
                    }
                    if let Some(svf) = g.modes.get_mut(m) {
                        svf.prepare_lanes(
                            self.sample_rate,
                            &cut,
                            self.mode_q.get(m).copied().unwrap_or(50.0),
                        );
                    }
                }
                ringing += (0..LANES).filter(|l| g.amp.active(*l)).count();
            }

            // Per-lane placement, worked out once per run.
            let mut gl = [0.0f32; LANES];
            let mut gr = [0.0f32; LANES];
            for lane in 0..LANES {
                let seat = if LANES > 1 {
                    lane as f32 / (LANES - 1) as f32 * 2.0 - 1.0
                } else {
                    0.0
                };
                let (l, r) = crate::dsp::pan::spread(seat * spread);
                gl[lane] = l;
                gr[lane] = r;
            }

            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(CHUNK);
                let Some(g) = self.groups.get_mut(gi) else {
                    break;
                };
                let (Some(exc), Some(env)) = (g.exc.get_mut(..take), g.env.get_mut(..take)) else {
                    break;
                };
                // EXCITE: noise, shaped by the strike envelope and
                // coloured by how hard the mallet is.
                g.noise.process(exc);
                g.exciter.process(env);
                for (e, v) in exc.iter_mut().zip(env.iter()) {
                    for (a, b) in e.iter_mut().zip(v.iter()) {
                        *a *= *b;
                    }
                }
                g.colour.process_lowpass(exc);

                // RESONATE: every partial hears the same excitation, at
                // the strength the striking point allows it.
                if let Some(voice) = g.voice.get_mut(..take) {
                    for f in voice.iter_mut() {
                        *f = [0.0; LANES];
                    }
                }
                for m in 0..MODES {
                    // Most of the Q gain is taken back out again, or
                    // DECAY would be a volume knob as well as a time. A
                    // square root leaves the honest part of it: a bell
                    // IS louder than a damped bar struck the same way.
                    let gain_m = VOICE_MAKEUP * self.mode_gain.get(m).copied().unwrap_or(0.0)
                        / self.mode_q.get(m).copied().unwrap_or(1.0).max(1.0).sqrt();
                    if gain_m <= 1e-5 {
                        continue;
                    }
                    let (Some(ring), Some(src)) = (g.ring.get_mut(..take), g.exc.get(..take))
                    else {
                        break;
                    };
                    ring.copy_from_slice(src);
                    if let Some(svf) = g.modes.get_mut(m) {
                        // The PLAIN bandpass, whose gain rises with Q —
                        // which is the physics: a resonator that rings
                        // longer also rings louder off the same strike,
                        // and the unity-normalised output threw exactly
                        // that away and left the instrument 70 dB down.
                        svf.process(ring, FilterMode::Bandpass);
                    }
                    let (Some(voice), Some(ring)) = (g.voice.get_mut(..take), g.ring.get(..take))
                    else {
                        break;
                    };
                    for (dst, src) in voice.iter_mut().zip(ring.iter()) {
                        for (a, b) in dst.iter_mut().zip(src.iter()) {
                            *a += *b * gain_m;
                        }
                    }
                }

                // The material's buzz, then the amp envelope.
                if let Some(voice) = g.voice.get_mut(..take) {
                    self.shaper.process_lanes(voice);
                }
                let Some(env) = g.env.get_mut(..take) else {
                    break;
                };
                g.amp.process(env);

                // Collapse the lanes into the stereo sum.
                let (Some(voice), Some(env)) = (g.voice.get(..take), g.env.get(..take)) else {
                    break;
                };
                for i in 0..take {
                    let frame = voice.get(i).copied().unwrap_or([0.0; LANES]);
                    let amp = env.get(i).copied().unwrap_or([0.0; LANES]);
                    let mut l = 0.0f32;
                    let mut r = 0.0f32;
                    for lane in 0..LANES {
                        let s = frame.get(lane).copied().unwrap_or(0.0)
                            * amp.get(lane).copied().unwrap_or(0.0)
                            * velocity_scale;
                        l += s * gl.get(lane).copied().unwrap_or(0.0);
                        r += s * gr.get(lane).copied().unwrap_or(0.0);
                    }
                    if let Some(slot) = self.left.get_mut(done + i) {
                        *slot += l;
                    }
                    if let Some(slot) = self.right.get_mut(done + i) {
                        *slot += r;
                    }
                }
                done += take;
            }
        }
        self.said_voices = ringing as f32;

        // BODY: the cabinet, on the sum rather than per voice — an
        // instrument has one body however many strings are on it.
        if body_amount > 0.0 {
            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(CHUNK);
                // A mono send into the body, which is what a soundboard
                // hears.
                for i in 0..take {
                    let a = self.left.get(done + i).copied().unwrap_or(0.0);
                    let b = self.right.get(done + i).copied().unwrap_or(0.0);
                    if let Some(slot) = self.body_in.get_mut(i) {
                        *slot = 0.5 * (a + b);
                    }
                }
                let (Some(src), Some(bl), Some(br)) = (
                    self.body_in.get(..take),
                    self.body_l.get_mut(..take),
                    self.body_r.get_mut(..take),
                ) else {
                    break;
                };
                self.body.process(src, bl, br, &mut self.body_store);
                for i in 0..take {
                    let wet_l = self.body_l.get(i).copied().unwrap_or(0.0) * body_amount;
                    let wet_r = self.body_r.get(i).copied().unwrap_or(0.0) * body_amount;
                    if let Some(slot) = self.left.get_mut(done + i) {
                        *slot += wet_l;
                    }
                    if let Some(slot) = self.right.get_mut(done + i) {
                        *slot += wet_r;
                    }
                }
                done += take;
            }
        }

        // OUT. The gain ramp advances once per sample on every path,
        // silence included — the clock's contract.
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

    fn voices(edit: impl Fn(&mut TineParams)) -> TineVoices {
        let mut params = TineParams::default();
        edit(&mut params);
        TineVoices::new(FS, 512, params)
    }

    /// Render `n` samples, flat gain, and hand back the left channel.
    fn run(v: &mut TineVoices, n: usize) -> Vec<f32> {
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

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    // ------------------------------------------------- the structure ---

    /// MATERIAL is the instrument: it moves the partials off the harmonic
    /// series and onto a bar's, which is the difference between a note
    /// and a clang. A pure function, so it is checked exactly.
    #[test]
    fn material_morphs_a_string_into_a_bar() {
        for m in 0..MODES {
            assert_eq!(p::ratio(0.0, m), p::HARMONIC[m], "0 must be a string");
            assert_eq!(p::ratio(1.0, m), p::INHARMONIC[m], "1 must be a bar");
            // Monotonic in between, and the fundamental never moves.
            let mut previous = p::ratio(0.0, m);
            for i in 1..=20 {
                let now = p::ratio(i as f32 / 20.0, m);
                assert!(now >= previous - 1e-6, "mode {m} went backwards");
                previous = now;
            }
        }
        assert_eq!(p::ratio(0.5, 0), 1.0, "the fundamental is the fundamental");
        // A string's partials are whole multiples; a bar's are not.
        for m in 1..MODES {
            let bar = p::ratio(1.0, m);
            assert!(
                (bar - bar.round()).abs() > 0.05,
                "bar partial {m} landed on a whole multiple"
            );
        }
    }

    /// PLACE is physics: a partial with a node where you strike cannot be
    /// excited at all. Struck halfway, every EVEN partial is silent.
    #[test]
    fn striking_a_node_cannot_excite_that_partial() {
        // Half way: partials 2, 4, 6, 8 have a node there.
        for m in [1usize, 3, 5, 7] {
            assert!(
                p::strike_gain(0.5, m) < 1e-5,
                "partial {} should be dead at the midpoint, got {}",
                m + 1,
                p::strike_gain(0.5, m)
            );
        }
        // ...and the odd ones are at their loudest.
        for m in [0usize, 2, 4, 6] {
            assert!(
                p::strike_gain(0.5, m) > 0.99,
                "partial {} should be full at the midpoint",
                m + 1
            );
        }
        // A third kills every third partial.
        for m in [2usize, 5] {
            assert!(p::strike_gain(1.0 / 3.0, m) < 1e-3, "partial {}", m + 1);
        }
        // Every gain is a gain.
        for place in 0..=50 {
            for m in 0..MODES {
                let g = p::strike_gain(place as f32 / 50.0, m);
                assert!((0.0..=1.0).contains(&g) && g.is_finite());
            }
        }
    }

    // ---------------------------------------------------- the sound ---

    /// A struck voice rings at the pitch it was given, and the partial
    /// the material predicts is really there.
    #[test]
    fn a_struck_note_rings_at_its_own_partials() {
        // A string, struck near the end so every partial is excited.
        let mut v = voices(|p| {
            p.material = 0.0;
            p.place = 0.08;
            p.decay = 0.8;
            p.body = 0.0;
            p.strike = 0.5;
        });
        v.note_on(57, 100, 1); // A3, 220 Hz
        let out = run(&mut v, 24_000);
        let settled = out.get(2_000..).unwrap();
        let f0 = amp(settled, 220.0);
        let f2 = amp(settled, 440.0);
        let between = amp(settled, 310.0);
        assert!(
            f0 > between * 4.0,
            "no fundamental: {f0:.5} vs {between:.5}"
        );
        assert!(f2 > between * 2.0, "no second partial: {f2:.5}");
    }

    /// And a BAR does not: its second partial sits where the table says,
    /// not at twice the fundamental. This is the whole claim of MATERIAL.
    #[test]
    fn a_bar_rings_off_the_harmonic_series() {
        let mut v = voices(|p| {
            p.material = 1.0;
            p.place = 0.08;
            p.decay = 0.8;
            p.body = 0.0;
        });
        v.note_on(57, 100, 1); // 220 Hz
        let out = run(&mut v, 24_000);
        let settled = out.get(2_000..).unwrap();
        let octave = amp(settled, 440.0);
        let bar_partial = amp(settled, 220.0 * p::INHARMONIC[1]);
        assert!(
            bar_partial > octave * 3.0,
            "the bar's second partial should beat the octave: {bar_partial:.5} vs {octave:.5}"
        );
    }

    /// DECAY is a ring time: more of it rings longer.
    #[test]
    fn more_decay_rings_longer() {
        let tail = |decay: f32| {
            let mut v = voices(|p| {
                p.decay = decay;
                p.body = 0.0;
            });
            v.note_on(60, 100, 1);
            let out = run(&mut v, 48_000);
            peak(out.get(30_000..).unwrap())
        };
        let short = tail(0.05);
        let long = tail(0.95);
        assert!(
            long > short * 8.0,
            "decay should ring: short {short:.6}, long {long:.6}"
        );
    }

    /// Sixteen voices, and the seventeenth steals the oldest rather than
    /// being dropped or stacking on a lane that is still ringing.
    #[test]
    fn it_plays_sixteen_notes_and_steals_the_oldest() {
        let mut v = voices(|p| p.decay = 0.9);
        for i in 0..VOICES {
            v.note_on(48 + i as u8, 100, i as u64);
        }
        let out = run(&mut v, 4_096);
        assert!(peak(&out) > 0.05, "sixteen voices made no sound");
        assert_eq!(
            v.readout().bands[0] as usize,
            VOICES,
            "not every voice is ringing"
        );
        // One more: still sixteen, and still finite.
        v.note_on(72, 100, 999);
        let out = run(&mut v, 4_096);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(
            v.readout().bands[0] as usize <= VOICES,
            "stealing grew the bank"
        );
    }

    /// Silence in, silence out — and an all-sound-off really is off.
    #[test]
    fn silence_stays_silent_and_all_sound_off_is_off() {
        let mut v = voices(|_| {});
        let out = run(&mut v, 4_096);
        assert!(out.iter().all(|s| *s == 0.0), "an idle synth made sound");

        v.note_on(60, 100, 1);
        let _ = run(&mut v, 2_048);
        v.all_sound_off();
        let out = run(&mut v, 4_096);
        assert!(
            peak(&out) < 1e-6,
            "all-sound-off left {} ringing",
            peak(&out)
        );
    }

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for material in [0.0f32, 0.5, 1.0] {
            for strike in [0.0f32, 0.5, 1.0] {
                for decay in [0.0f32, 1.0] {
                    let mut v = voices(|p| {
                        p.material = material;
                        p.strike = strike;
                        p.decay = decay;
                        p.body = 1.0;
                        p.level = 2.0;
                    });
                    v.note_on(24, 127, 1);
                    v.note_on(108, 127, 2);
                    let out = run(&mut v, 8_192);
                    assert!(
                        out.iter().all(|s| s.is_finite()),
                        "material {material} strike {strike} decay {decay}"
                    );
                    assert!(
                        peak(&out) < 24.0,
                        "material {material} strike {strike} ran away to {}",
                        peak(&out)
                    );
                }
            }
        }
    }

    #[test]
    fn odd_render_lengths_are_accepted() {
        let mut v = voices(|_| {});
        v.note_on(60, 100, 1);
        for len in [0usize, 1, 3, 17, 63, 129, 700] {
            let mut out = vec![0.0f32; len];
            let mut ramp = Ramp::across(1.0, 1.0, len.max(1));
            v.render(&mut out, 0, &mut ramp);
            assert!(out.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voices(|p| {
            p.body = 0.6;
            p.material = 0.5;
        });
        for i in 0..VOICES {
            v.note_on(48 + i as u8, 100, i as u64);
        }
        let mut out = vec![0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, 256);
        v.render(&mut out, 0, &mut ramp);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..30 {
                let mut ramp = Ramp::across(1.0, 1.0, 256);
                v.render(&mut out, 0, &mut ramp);
            }
        });
    }

    /// A p-lock overrides for a note and releases back to the LIVE knob,
    /// not to a compile-time snapshot.
    #[test]
    fn a_plock_releases_to_the_live_knob() {
        let mut v = voices(|_| {});
        v.set_param(p::MATERIAL, 0.2);
        assert_eq!(v.params().material, 0.2);
        v.plock(p::MATERIAL, Some(0.9));
        assert_eq!(v.params().material, 0.9);
        // The knob moves while the lock is held...
        v.set_param(p::MATERIAL, 0.4);
        v.plock(p::MATERIAL, None);
        assert_eq!(v.params().material, 0.4, "released to a stale value");
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = TineParams::default();
        for def in p::TABLE.iter() {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "row {}", def.name);
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), Some(def.min), "row {}", def.name);
            params.set(def.id, def.max * 100.0 + 1.0);
            let got = params.get(def.id).unwrap();
            assert!(got <= def.max && got >= def.min, "row {} = {got}", def.name);
        }
        assert_eq!(params.get(9_999), None);

        let mut junk = TineParams {
            material: f32::NAN,
            strike: 40.0,
            place: -9.0,
            decay: f32::INFINITY,
            body: -1.0,
            tone: 12.0,
            spread: f32::NAN,
            tune: 900.0,
            level: -4.0,
        };
        junk.sanitize();
        for def in p::TABLE.iter() {
            let got = junk.get(def.id).unwrap();
            assert!(
                got.is_finite() && got >= def.min && got <= def.max,
                "row {} survived sanitize as {got}",
                def.name
            );
        }
    }
}
