//! The DRUM voice — the first machine written for the pages.
//!
//! One voice, five MODELS, eight cells: the MODEL cell changes what the
//! other seven mean. SWEEP is a kick's pitch drop, a snare's bend, a
//! hat's band sweep and a clap's spread between hands; SNAP is a click,
//! a wire rattle, a stick, a hand count; TONE is a body harmonic, a
//! shell ratio, a band centre. That is the pages' rule made audible: a
//! cell that only scaled a level would not be here.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic belongs to
//! `src/dsp/`. The old kick, snare, hat and clap engines are the
//! reference for each model's recipe; this file replaces them on the
//! shelf and they stay as parts.
//!
//! # The path
//!
//! ```text
//! MODEL ──▶ body (sines | square bank | noise) ── body env ─┐
//!           snap (noise burst | wires | hands) ── snap env ─┼─▶ FILTER ─▶ DRIVE ─▶ AMP ─▶ out
//!           noise ─────────────────────────── body env ─────┘   (env + key)          (A/D, vel)
//! ```
//!
//! Per SAMPLE where modulation is a multiply, per CONTROL CHUNK where it
//! is a coefficient — the same split the kick makes, for the same reason.

use crate::dsp::adsr::{Adsr, ExpDecay};
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{DRIVE_MAX, DRIVE_MIN, Mode as ShapeMode, Waveshaper};
use crate::params::drum as dp;

/// How often the pitches and the filter are rebuilt, in samples.
pub const CHUNK: usize = 32;
/// The hat's bank: the 808's six inharmonic squares.
const BANK: usize = 6;
const RATIOS: [f32; BANK] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
/// A hat struck at or above this note is OPEN: its decay is longer.
pub const OPEN_NOTE: u8 = 46;
/// Where a clap's hands fall, as multiples of the spread. Not even, so
/// the hands do not sum to a flam.
const HANDS: [f32; 4] = [0.0, 1.0, 1.9, 2.7];
/// The hat's fixed high-pass, the clap's.
const HAT_HP_HZ: f32 = 7_000.0;
const CLAP_HP_HZ: f32 = 500.0;

/// The MODEL cell's positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    Kick,
    Snare,
    Hat,
    Clap,
    Tom,
}

impl Model {
    pub const ALL: [Self; 5] = [Self::Kick, Self::Snare, Self::Hat, Self::Clap, Self::Tom];

    pub fn from_value(value: f32) -> Self {
        let at = value.round().clamp(0.0, (Self::ALL.len() - 1) as f32) as usize;
        Self::ALL[at]
    }

    /// The tuned note's fundamental at TUNE zero, played at the root.
    fn base_hz(self) -> f32 {
        match self {
            Self::Kick => 50.0,
            Self::Snare => 180.0,
            Self::Tom => 100.0,
            // The hat and the clap have no fundamental; their tune moves
            // the bank and the band.
            Self::Hat | Self::Clap => 1.0,
        }
    }
}

/// A drum's settings, in engine units. Parallel to [`dp::TABLE`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DrumParams {
    pub model: f32,
    pub tune: f32,
    pub decay_ms: f32,
    pub sweep: f32,
    pub snap: f32,
    pub tone: f32,
    pub noise: f32,
    pub drive: f32,
    pub ftype: f32,
    pub cutoff_hz: f32,
    pub reso: f32,
    pub fenv: f32,
    pub fattack_ms: f32,
    pub fdecay_ms: f32,
    pub ftrack: f32,
    pub attack_ms: f32,
    pub amp_decay_ms: f32,
    pub vel: f32,
    pub level: f32,
}

impl Default for DrumParams {
    fn default() -> Self {
        let at = |id: u32| crate::params::def(dp::TABLE, id).default;
        Self {
            model: at(dp::MODEL),
            tune: at(dp::TUNE),
            decay_ms: at(dp::DECAY),
            sweep: at(dp::SWEEP),
            snap: at(dp::SNAP),
            tone: at(dp::TONE),
            noise: at(dp::NOISE),
            drive: at(dp::DRIVE),
            ftype: at(dp::FTYPE),
            cutoff_hz: at(dp::CUTOFF),
            reso: at(dp::RESO),
            fenv: at(dp::FENV),
            fattack_ms: at(dp::FATTACK),
            fdecay_ms: at(dp::FDECAY),
            ftrack: at(dp::FTRACK),
            attack_ms: at(dp::ATTACK),
            amp_decay_ms: at(dp::AMP_DECAY),
            vel: at(dp::VEL),
            level: at(dp::LEVEL),
        }
    }
}

impl DrumParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(dp::TABLE, param, value) else {
            return;
        };
        match param {
            dp::MODEL => self.model = value,
            dp::TUNE => self.tune = value,
            dp::DECAY => self.decay_ms = value,
            dp::SWEEP => self.sweep = value,
            dp::SNAP => self.snap = value,
            dp::TONE => self.tone = value,
            dp::NOISE => self.noise = value,
            dp::DRIVE => self.drive = value,
            dp::FTYPE => self.ftype = value,
            dp::CUTOFF => self.cutoff_hz = value,
            dp::RESO => self.reso = value,
            dp::FENV => self.fenv = value,
            dp::FATTACK => self.fattack_ms = value,
            dp::FDECAY => self.fdecay_ms = value,
            dp::FTRACK => self.ftrack = value,
            dp::ATTACK => self.attack_ms = value,
            dp::AMP_DECAY => self.amp_decay_ms = value,
            dp::VEL => self.vel = value,
            dp::LEVEL => self.level = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            dp::MODEL => self.model,
            dp::TUNE => self.tune,
            dp::DECAY => self.decay_ms,
            dp::SWEEP => self.sweep,
            dp::SNAP => self.snap,
            dp::TONE => self.tone,
            dp::NOISE => self.noise,
            dp::DRIVE => self.drive,
            dp::FTYPE => self.ftype,
            dp::CUTOFF => self.cutoff_hz,
            dp::RESO => self.reso,
            dp::FENV => self.fenv,
            dp::FATTACK => self.fattack_ms,
            dp::FDECAY => self.fdecay_ms,
            dp::FTRACK => self.ftrack,
            dp::ATTACK => self.attack_ms,
            dp::AMP_DECAY => self.amp_decay_ms,
            dp::VEL => self.vel,
            dp::LEVEL => self.level,
            _ => 0.0,
        }
    }

    pub fn model(&self) -> Model {
        Model::from_value(self.model)
    }
}

/// A note's own pitch as a multiplier on the tuned fundamental. MIDI 36
/// is unity, as on every drum machine.
fn pitch_scale(pitch: u8) -> f32 {
    const ROOT: f32 = 36.0;
    ((f32::from(pitch) - ROOT) / 12.0).exp2()
}

/// The drum's single voice: a retrigger cuts its own tail.
pub struct DrumVoice {
    sample_rate: f32,
    params: DrumParams,
    /// The knobs as letters last set them: what a lock's restore returns to.
    base: DrumParams,

    osc_a: MipOsc,
    osc_b: MipOsc,
    sine_tables: Vec<f32>,
    bank: [MipOsc; BANK],
    square_tables: Vec<f32>,
    /// Two generators, one per consumer: the snap and the white layer
    /// each read their own stream, so where a block is cut cannot change
    /// which sample lands where.
    noise: WhiteNoise,
    white: WhiteNoise,

    body: ExpDecay,
    pitch_env: ExpDecay,
    snap_env: ExpDecay,
    filter_env: Adsr,
    amp: Adsr,

    band: Svf,
    hp: Svf,
    filter: Svf,
    shaper: Waveshaper,

    note_scale: f32,
    velocity: f32,
    open: bool,
    /// The clap's hands still to come, as samples from now.
    pending: [u32; 4],
    pending_len: usize,

    tuned_band_hz: f32,
    tuned_filter_hz: f32,
    tuned_filter_q: f32,
    tuned_drive: f32,

    chunk_left: usize,
    sig: [f32; CHUNK],
    env_a: [f32; CHUNK],
    env_b: [f32; CHUNK],
}

impl Default for DrumVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: DrumParams::default(),
            base: DrumParams::default(),
            osc_a: MipOsc::new(),
            osc_b: MipOsc::new(),
            sine_tables: Vec::new(),
            bank: std::array::from_fn(|_| MipOsc::new()),
            square_tables: Vec::new(),
            noise: WhiteNoise::new(),
            white: WhiteNoise::new(),
            body: ExpDecay::new(),
            pitch_env: ExpDecay::new(),
            snap_env: ExpDecay::new(),
            filter_env: Adsr::new(),
            amp: Adsr::new(),
            band: Svf::new(),
            hp: Svf::new(),
            filter: Svf::new(),
            shaper: Waveshaper::new(),
            note_scale: 1.0,
            velocity: 1.0,
            open: false,
            pending: [0; 4],
            pending_len: 0,
            tuned_band_hz: 0.0,
            tuned_filter_hz: 0.0,
            tuned_filter_q: 0.0,
            tuned_drive: f32::NAN,
            chunk_left: 0,
            sig: [0.0; CHUNK],
            env_a: [0.0; CHUNK],
            env_b: [0.0; CHUNK],
        }
    }

    /// Green zone: build the tables and settle every kernel. The
    /// allocations in this file all happen here.
    pub fn prepare(&mut self, sample_rate: f32, params: DrumParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.sine_tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.sine_tables);
        self.square_tables
            .resize(osc::table_len(Waveform::Square), 0.0);
        osc::build_tables(Waveform::Square, &mut self.square_tables);
        self.osc_a.prepare(self.sample_rate, Waveform::Sine);
        self.osc_b.prepare(self.sample_rate, Waveform::Sine);
        for osc in &mut self.bank {
            osc.prepare(self.sample_rate, Waveform::Square);
        }
        self.noise.seed(0xd12_0d12_5eed_0001);
        self.white.seed(0xd12_0d12_5eed_0002);
        self.params = params;
        self.base = params;
        self.apply_envelopes();
        self.apply_model();
        self.reset();
    }

    /// Envelope timings, rebuilt from the params. One-shots throughout.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        let decay = self.params.decay_ms;
        let body = if self.params.model() == Model::Hat && self.open {
            decay * 4.0
        } else {
            decay
        };
        self.body.prepare(fs, body);
        // The pitch falls in the first fifth of the body: a drop, not a
        // glide, whatever the decay.
        self.pitch_env.prepare(fs, (decay * 0.2).clamp(3.0, 400.0));
        let snap = match self.params.model() {
            // The wires ring for most of the shell; a hand is a burst.
            Model::Snare => (decay * 0.6).max(10.0),
            Model::Clap => 6.0,
            Model::Kick | Model::Tom | Model::Hat => 2.5,
        };
        self.snap_env.prepare(fs, snap);
        self.filter_env
            .prepare(fs, self.params.fattack_ms, self.params.fdecay_ms, 0.0, 0.0);
        self.amp.prepare(
            fs,
            self.params.attack_ms,
            self.params.amp_decay_ms,
            0.0,
            0.0,
        );
    }

    /// What changes with the model: the fixed high-pass, the phases of
    /// the bank.
    fn apply_model(&mut self) {
        let hp = match self.params.model() {
            Model::Hat => HAT_HP_HZ,
            Model::Clap => CLAP_HP_HZ,
            Model::Kick | Model::Snare | Model::Tom => 20.0,
        };
        self.hp
            .prepare(self.sample_rate, hp.min(self.sample_rate * 0.45), 0.707);
        for (i, osc) in self.bank.iter_mut().enumerate() {
            osc.set_phase((i as f32 * 0.618_034) % 1.0);
        }
        self.tuned_band_hz = 0.0;
        self.tuned_filter_hz = 0.0;
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.osc_a.reset();
        self.osc_b.reset();
        self.noise.reset();
        self.white.reset();
        self.body = ExpDecay::new();
        self.pitch_env = ExpDecay::new();
        self.snap_env = ExpDecay::new();
        self.filter_env.reset();
        self.amp.reset();
        self.band.reset();
        self.hp.reset();
        self.filter.reset();
        self.pending_len = 0;
        self.apply_envelopes();
    }

    /// A letter: the knob moves, and the live patch with it.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

    /// A parameter LOCK at a note boundary: `Some` holds the live patch
    /// at the note's value, `None` returns it to the knob.
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
        let model_before = self.params.model();
        self.params.set(param, value);
        match param {
            dp::DECAY | dp::FATTACK | dp::FDECAY | dp::ATTACK | dp::AMP_DECAY => {
                self.apply_envelopes();
            }
            dp::MODEL if self.params.model() != model_before => {
                self.apply_model();
                self.apply_envelopes();
            }
            _ => {}
        }
    }

    pub fn params(&self) -> DrumParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.amp.active()
    }

    /// Strike. Every envelope restarts together; a hat above the open
    /// note opens; a clap queues its hands.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.note_scale = pitch_scale(pitch);
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        let model = self.params.model();
        let open = model == Model::Hat && pitch >= OPEN_NOTE;
        if open != self.open {
            self.open = open;
            self.apply_envelopes();
        }
        // The sines restart at a known phase so every hit has the same
        // attack; the bank runs free, as a hat's does.
        self.osc_a.reset();
        self.osc_b.reset();
        self.chunk_left = 0;
        self.body.trigger(1.0);
        self.pitch_env.trigger(1.0);
        self.snap_env.trigger(1.0);
        self.filter_env.gate_on();
        self.amp.gate_on();

        self.pending_len = 0;
        if model == Model::Clap {
            // SNAP is the hand count, SWEEP the spread between them.
            let hands = (1.0 + self.params.snap * 3.0).round().clamp(1.0, 4.0) as usize;
            let spread_ms = 2.0 + self.params.sweep / 48.0 * 38.0;
            let spread = self.sample_rate * spread_ms / 1_000.0;
            for offset in HANDS.iter().take(hands).skip(1) {
                let at = (offset * spread).round();
                if let Some(slot) = self.pending.get_mut(self.pending_len) {
                    *slot = if at.is_finite() && at > 0.0 {
                        at.min(u32::MAX as f32) as u32
                    } else {
                        0
                    };
                    self.pending_len += 1;
                }
            }
        }
    }

    /// Red zone: render into `out`, ADDING to what is there.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.sine_tables.is_empty() || self.square_tables.is_empty() {
            return;
        }
        let mut written = 0usize;
        while written < out.len() {
            if self.chunk_left == 0 {
                self.start_chunk();
                self.chunk_left = CHUNK;
            }
            let take = self
                .chunk_left
                .min(out.len() - written)
                .min(self.until_next_hand())
                .max(1)
                .min(out.len() - written);
            let Some(block) = out.get_mut(written..written + take) else {
                return;
            };
            self.render_run(block, gain);
            self.advance_hands(take);
            self.chunk_left = self.chunk_left.saturating_sub(take);
            written += take;
        }
    }

    fn until_next_hand(&self) -> usize {
        let mut soonest = CHUNK;
        for at in self.pending.iter().take(self.pending_len) {
            let at = *at as usize;
            if at > 0 && at < soonest {
                soonest = at;
            }
        }
        soonest.max(1)
    }

    fn advance_hands(&mut self, n: usize) {
        if self.pending_len == 0 {
            return;
        }
        let n = n as u32;
        let mut fired = false;
        let mut kept = 0usize;
        for i in 0..self.pending_len {
            let Some(at) = self.pending.get(i).copied() else {
                break;
            };
            if at <= n {
                fired = true;
            } else if let Some(slot) = self.pending.get_mut(kept) {
                *slot = at - n;
                kept += 1;
            }
        }
        self.pending_len = kept;
        if fired {
            self.snap_env.trigger(1.0);
        }
    }

    /// The start of a control chunk: the pitches and the filter.
    fn start_chunk(&mut self) {
        let fs = self.sample_rate;
        let ceiling = fs * 0.45;
        let model = self.params.model();
        let tune = (self.params.tune / 12.0).exp2() * self.note_scale;
        let drop = (self.params.sweep * self.pitch_env.current() / 12.0).exp2();
        match model {
            Model::Kick | Model::Tom | Model::Snare => {
                let f = model.base_hz() * tune * drop;
                let ratio = match model {
                    Model::Kick => 1.0 + self.params.tone,
                    Model::Tom => 1.0 + self.params.tone * 0.5,
                    _ => 1.5 + self.params.tone * 1.5,
                };
                self.osc_a.set_freq(f.clamp(1.0, ceiling));
                self.osc_b.set_freq((f * ratio).clamp(1.0, ceiling));
                if model == Model::Snare {
                    // The wires' band: around 1.8 kHz, moved by TONE.
                    let hz = 1_800.0 * ((self.params.tone - 0.5) * 2.0).exp2();
                    self.retune_band(hz.min(ceiling), 0.9);
                }
            }
            Model::Hat => {
                for (osc, ratio) in self.bank.iter_mut().zip(RATIOS) {
                    osc.set_freq((ratio * tune).clamp(1.0, ceiling));
                }
                let hz = 2_000.0 * (self.params.tone * 3.0).exp2() * drop;
                self.retune_band(hz.min(ceiling), 2.0);
            }
            Model::Clap => {
                let hz = 300.0 * (self.params.tone * 3.74).exp2() * tune;
                self.retune_band(hz.clamp(20.0, ceiling), 1.1);
            }
        }
        // The filter: the knob, moved by its envelope in semitones and
        // by the note in proportion to TRACK.
        let env = self.params.fenv * self.filter_env.current() / 12.0;
        let track = self.note_scale.powf(self.params.ftrack);
        let hz = (self.params.cutoff_hz * env.exp2() * track).clamp(20.0, ceiling);
        let q = self.params.reso;
        if (hz - self.tuned_filter_hz).abs() > self.tuned_filter_hz.max(1.0) * 1e-4
            || (q - self.tuned_filter_q).abs() > 1e-4
        {
            self.tuned_filter_hz = hz;
            self.tuned_filter_q = q;
            self.filter.prepare(fs, hz, q);
        }
        // The shaper is configured when the drive moves, never per run:
        // configuring resets its oversampler, and a reset that depended
        // on where the block was cut would break split-block equality.
        let drive = (DRIVE_MIN + 7.0 * self.params.drive).min(DRIVE_MAX);
        if drive != self.tuned_drive {
            self.tuned_drive = drive;
            self.shaper.configure(ShapeMode::SoftClip, drive, 0.0, 1.0);
        }
    }

    fn retune_band(&mut self, hz: f32, q: f32) {
        if (hz - self.tuned_band_hz).abs() > self.tuned_band_hz.max(1.0) * 1e-4 {
            self.tuned_band_hz = hz;
            self.band.prepare(self.sample_rate, hz, q);
        }
    }

    /// One run of samples inside a control chunk: per-sample work only.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(sig), Some(env_a), Some(env_b)) = (
            self.sig.get_mut(..n),
            self.env_a.get_mut(..n),
            self.env_b.get_mut(..n),
        ) else {
            return;
        };
        let velocity = self.velocity;
        let model = self.params.model();

        // --- the pitch envelope runs at sample rate; the chunk reads it
        self.pitch_env.process(env_b);

        // --- the body and the snap, into `sig` ------------------------
        self.body.process(env_a);
        self.snap_env.process(env_b);
        match model {
            Model::Kick | Model::Tom | Model::Snare => {
                self.osc_a.process(sig, &self.sine_tables);
                let mut second = [0.0f32; CHUNK];
                let Some(second) = second.get_mut(..n) else {
                    return;
                };
                self.osc_b.process(second, &self.sine_tables);
                let weight = if model == Model::Snare {
                    1.0
                } else {
                    self.params.tone
                };
                let norm = 1.0 / (1.0 + weight);
                for (a, b) in sig.iter_mut().zip(second.iter()) {
                    *a = (*a + *b * weight) * norm;
                }
                for (sample, env) in sig.iter_mut().zip(env_a.iter()) {
                    *sample *= *env * velocity;
                }
                if self.params.snap > 0.0 {
                    let mut burst = [0.0f32; CHUNK];
                    let Some(burst) = burst.get_mut(..n) else {
                        return;
                    };
                    self.noise.process(burst);
                    if model == Model::Snare {
                        self.band.process(burst, Mode::BandpassUnity);
                    }
                    let level = self.params.snap * velocity;
                    for (sample, (rattle, env)) in
                        sig.iter_mut().zip(burst.iter().zip(env_b.iter()))
                    {
                        *sample += *rattle * *env * level;
                    }
                }
            }
            Model::Hat => {
                for sample in sig.iter_mut() {
                    *sample = 0.0;
                }
                let mut square = [0.0f32; CHUNK];
                let Some(square) = square.get_mut(..n) else {
                    return;
                };
                for osc in &mut self.bank {
                    osc.process(square, &self.square_tables);
                    for (sum, s) in sig.iter_mut().zip(square.iter()) {
                        *sum += *s;
                    }
                }
                let norm = 1.0 / BANK as f32;
                for sample in sig.iter_mut() {
                    *sample *= norm;
                }
                self.band.process(sig, Mode::BandpassUnity);
                for (sample, env) in sig.iter_mut().zip(env_a.iter()) {
                    *sample *= *env * velocity;
                }
                if self.params.snap > 0.0 {
                    self.noise.process(square);
                    let level = self.params.snap * velocity;
                    for (sample, (stick, env)) in
                        sig.iter_mut().zip(square.iter().zip(env_b.iter()))
                    {
                        *sample += *stick * *env * level;
                    }
                }
            }
            Model::Clap => {
                self.noise.process(sig);
                self.band.process(sig, Mode::BandpassUnity);
                // The hands are bursts; the room is half a body's decay.
                for (sample, (burst, tail)) in sig.iter_mut().zip(env_b.iter().zip(env_a.iter())) {
                    *sample *= (*burst + *tail * 0.5) * velocity;
                }
            }
        }

        // --- noise: white, under the body, at NOISE -------------------
        if self.params.noise > 0.0 {
            let mut white = [0.0f32; CHUNK];
            let Some(white) = white.get_mut(..n) else {
                return;
            };
            self.white.process(white);
            let level = self.params.noise * velocity;
            for (sample, (w, env)) in sig.iter_mut().zip(white.iter().zip(env_a.iter())) {
                *sample += *w * *env * level;
            }
        }
        if matches!(model, Model::Hat | Model::Clap) {
            self.hp.process(sig, Mode::Highpass);
        }

        // --- the filter, the drive, the amp ---------------------------
        self.filter_env.process(env_b);
        let mode = match self.params.ftype.round() as u32 {
            1 => Mode::Highpass,
            2 => Mode::BandpassUnity,
            _ => Mode::Lowpass,
        };
        self.filter.process(sig, mode);
        self.shaper.process(sig);
        self.amp.process(env_a);
        // VEL: how much the strike's weight reaches the level.
        let weight = (1.0 - self.params.vel) + self.params.vel * velocity;
        let level = self.params.level * gain * weight / velocity.max(1e-3);
        for (out, (sample, env)) in out.iter_mut().zip(sig.iter().zip(env_a.iter())) {
            *out += *sample * *env * level;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn voice(params: DrumParams) -> DrumVoice {
        let mut v = DrumVoice::new();
        v.prepare(48_000.0, params);
        v
    }

    fn render(v: &mut DrumVoice, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn with_model(model: Model) -> DrumParams {
        DrumParams {
            model: Model::ALL.iter().position(|m| *m == model).unwrap() as f32,
            ..DrumParams::default()
        }
    }

    #[test]
    fn a_fresh_drum_is_silent_until_it_is_struck() {
        let mut v = voice(DrumParams::default());
        assert_eq!(peak(&render(&mut v, 4_800)), 0.0);
        v.trigger(36, 100);
        assert!(peak(&render(&mut v, 4_800)) > 0.05);
    }

    /// Every model sounds, and no two sound the same: MODEL changes the
    /// drum, not a level.
    #[test]
    fn every_model_speaks_and_they_all_differ() {
        let mut renders: Vec<Vec<f32>> = Vec::new();
        for model in Model::ALL {
            let mut v = voice(with_model(model));
            v.trigger(36, 110);
            let out = render(&mut v, 9_600);
            assert!(peak(&out) > 0.02, "{model:?} is silent");
            assert!(
                out.iter().all(|s| s.is_finite()),
                "{model:?} went non-finite"
            );
            renders.push(out);
        }
        for (i, a) in renders.iter().enumerate() {
            for b in renders.iter().skip(i + 1) {
                assert!(a != b, "two models rendered the same");
            }
        }
    }

    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let params = DrumParams {
            decay_ms: 1_000.0,
            amp_decay_ms: 2_000.0,
            ..DrumParams::default()
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

    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        for model in Model::ALL {
            let params = DrumParams {
                snap: 0.6,
                noise: 0.3,
                fenv: 12.0,
                ..with_model(model)
            };
            let mut whole = voice(params);
            whole.trigger(36, 100);
            let mut a = vec![0.0f32; 512];
            whole.render_add(&mut a, 1.0);
            let mut split = voice(params);
            split.trigger(36, 100);
            let mut b = vec![0.0f32; 512];
            split.render_add(&mut b[..100], 1.0);
            split.render_add(&mut b[100..357], 1.0);
            split.render_add(&mut b[357..], 1.0);
            let first = a
                .iter()
                .zip(&b)
                .position(|(x, y)| x.to_bits() != y.to_bits());
            assert!(
                first.is_none(),
                "{model:?}: 512 must equal 100 + 257 + 155; first difference at {first:?}: {:?} vs {:?}",
                first.map(|i| a[i]),
                first.map(|i| b[i])
            );
        }
    }

    #[test]
    fn rendering_does_not_allocate() {
        for model in Model::ALL {
            let mut v = voice(DrumParams {
                snap: 1.0,
                noise: 1.0,
                drive: 1.0,
                ..with_model(model)
            });
            let mut out = vec![0.0f32; 256];
            assert_no_alloc::assert_no_alloc(|| {
                for i in 0..100 {
                    if i % 16 == 0 {
                        v.trigger(if i % 32 == 0 { 36 } else { 48 }, 100);
                    }
                    v.render_add(&mut out, 0.8);
                }
            });
        }
    }

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for model in Model::ALL {
            for (id, extreme) in [
                (dp::TUNE, dp::TUNE_MIN),
                (dp::TUNE, dp::TUNE_MAX),
                (dp::CUTOFF, 20.0),
                (dp::CUTOFF, 20_000.0),
                (dp::RESO, 12.0),
                (dp::SWEEP, 48.0),
                (dp::DECAY, 10.0),
                (dp::DECAY, 2_000.0),
            ] {
                let mut params = DrumParams {
                    snap: 1.0,
                    noise: 1.0,
                    drive: 1.0,
                    fenv: 48.0,
                    ..with_model(model)
                };
                params.set(id, extreme);
                let mut v = voice(params);
                v.trigger(36, 127);
                let out = render(&mut v, 4_096);
                assert!(
                    out.iter().all(|s| s.is_finite()),
                    "{model:?} with {id} at {extreme} went non-finite"
                );
                v.trigger(96, 127);
                let out = render(&mut v, 4_096);
                assert!(out.iter().all(|s| s.is_finite()));
            }
        }
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = DrumParams::default();
        for def in dp::TABLE {
            let want = (def.min + def.max) * 0.5;
            params.set(def.id, want);
            assert!(
                (params.get(def.id) - want).abs() < 1e-3,
                "{} did not round-trip",
                def.name
            );
        }
        let before = params;
        params.set(9_999, 1.0);
        assert_eq!(params, before);
        params.set(dp::TUNE, 1e9);
        assert!((params.tune - dp::TUNE_MAX).abs() < 1e-3);
    }

    #[test]
    fn an_open_hat_outlasts_a_closed_one_and_chokes_it() {
        let mut closed = voice(with_model(Model::Hat));
        closed.trigger(42, 120);
        let _ = render(&mut closed, 4_800);
        let late_closed = peak(&render(&mut closed, 4_800));
        let mut open = voice(with_model(Model::Hat));
        open.trigger(OPEN_NOTE, 120);
        let _ = render(&mut open, 4_800);
        let late_open = peak(&render(&mut open, 4_800));
        assert!(
            late_open > late_closed * 2.0,
            "open {late_open} closed {late_closed}"
        );
        // A closed hat over the ringing open one cuts it.
        open.trigger(42, 120);
        let _ = render(&mut open, 4_800);
        assert!(peak(&render(&mut open, 4_800)) < late_open);
    }

    #[test]
    fn a_lock_is_heard_on_its_hit_and_the_knob_comes_back() {
        let mut v = voice(DrumParams::default());
        v.set_param(dp::DECAY, 200.0);
        v.plock(dp::DECAY, Some(50.0));
        assert_eq!(v.params().get(dp::DECAY), 50.0);
        v.plock(dp::DECAY, None);
        assert_eq!(v.params().get(dp::DECAY), 200.0);
        v.plock(dp::MODEL, Some(2.0));
        assert_eq!(v.params().model(), Model::Hat);
        v.plock(dp::MODEL, None);
        assert_eq!(v.params().model(), Model::Kick);
    }

    /// SWEEP is a drop, not a transposition: the kick starts high and
    /// its tail sits at the tuned note.
    #[test]
    fn the_sweep_falls_to_the_tuned_note() {
        let rate_hz = |samples: &[f32]| {
            let crossings = samples
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count();
            crossings as f32 / 2.0 * 48_000.0 / samples.len() as f32
        };
        let params = DrumParams {
            sweep: 36.0,
            decay_ms: 2_000.0,
            amp_decay_ms: 3_000.0,
            snap: 0.0,
            tone: 0.0,
            drive: 0.0,
            ..DrumParams::default()
        };
        let mut v = voice(params);
        v.trigger(36, 100);
        let early = rate_hz(&render(&mut v, 480));
        let _ = render(&mut v, 48_000);
        let late = rate_hz(&render(&mut v, 9_600));
        assert!(
            early > late * 2.0,
            "the sweep did not drop: {early} then {late}"
        );
        assert!(
            (late - 50.0).abs() < 4.0,
            "the tail sits at {late} Hz, not the tuned 50"
        );
    }

    #[test]
    fn the_drum_machine_c_is_the_tuned_note() {
        assert!((pitch_scale(36) - 1.0).abs() < 1e-6);
        assert!((pitch_scale(48) - 2.0).abs() < 1e-5);
    }
}
