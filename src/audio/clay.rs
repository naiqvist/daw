//! CLAY — one voice that is a kick, a tom, a snare, a hat or a bass by
//! where three macros sit, with no type switch anywhere.
//!
//! The physics: an excitation feeding a resonating body, plus noise,
//! over a decay. MATTER morphs the body's partial spacing from a skin
//! (a membrane's modes, nearly a sine at the bottom: the 808) through
//! wood to metal (inharmonic, short, mostly noise: a hat). SIZE couples
//! the fundamental and the decay the way a real object does. STRIKE is
//! how hard: the click, the noise share and its band rise together.
//! Beneath the macros: DROP (the pitch fall at the strike), DAMP (the
//! high partials die first), WIRES (the snare's rattle, gated by the
//! body), FEED (the excitation keeps feeding while the gate is held —
//! what makes a bass a bass), TONE (the second partial), KEY (how much
//! the played note overrides SIZE's fundamental).
//!
//! What keeps a snare and a hat from being TONAL: the metal ratio set is
//! inharmonic, the body's share falls with MATTER, the partial decays
//! shorten with MATTER and DAMP, and the noise's share and band rise
//! with MATTER and STRIKE. What makes the kick FAT: a skin body is one
//! long sine with a pitch drop, driven into a soft clip by STRIKE.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic is in
//! `src/dsp/`.
//!
//! ```text
//! CLICK (noise, 2 ms, HP) ─┐
//! BODY (six sines, MATTER ratios, SIZE, DROP, DAMP; FEED holds them) ─▶ DRIVE ─┼─▶ AMP ─▶ CRACK ─▶ BLOOM ─▶ out
//! NOISE (white ─▶ SVF band by MATTER/STRIKE ─▶ burst env) ─┤
//! WIRES (white ─▶ SVF band by SIZE ─▶ gated by the body) ──┘
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::adsr::{Adsr, ExpDecay};
use crate::dsp::dynamics::{SlewBrighten, TransientSplit};
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::reverb::Reverb;
use crate::dsp::shaper::{DRIVE_MAX, DRIVE_MIN, Mode as ShapeMode, Waveshaper};
use crate::pages::{Hero, HeroMark, HeroSeries};
use crate::params::clay as cp;

/// How often the partials, the bands and the drive are rebuilt.
pub const CHUNK: usize = 32;
/// The body's partials.
pub const PARTIALS: usize = 6;
/// SIZE 0 is this fundamental; SIZE 1 is six octaves up.
pub const SIZE_LOW_HZ: f32 = 30.0;
pub const SIZE_OCTAVES: f32 = 6.0;
/// SIZE 0 rings this long; SIZE 1 rings 30 ms.
pub const DECAY_LONG_MS: f32 = 4_000.0;
pub const DECAY_SHORT_MS: f32 = 30.0;
/// The ratio sets MATTER morphs between: a membrane's modes, a bar's,
/// and the 808 hat's six squares.
const SKIN: [f32; PARTIALS] = [1.0, 1.59, 2.14, 2.30, 2.65, 2.92];
const WOOD: [f32; PARTIALS] = [1.0, 2.76, 5.40, 8.93, 13.34, 18.64];
const METAL: [f32; PARTIALS] = [1.0, 1.483, 1.800, 2.546, 2.630, 3.897];
/// The click's length and its high-pass.
const CLICK_MS: f32 = 2.0;
const CLICK_HP_HZ: f32 = 2_000.0;
/// The transient split's window.
const CRACK_WINDOW_MS: f32 = 20.0;
const DECADES: f32 = 6.907_755_4;
/// A band-passed burst against a unit sine: the compensation.
const NOISE_GAIN: f32 = 5.0;

/// The voice's settings, in engine units. Parallel to [`cp::TABLE`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ClayParams {
    pub matter: f32,
    pub size: f32,
    pub strike: f32,
    pub drop: f32,
    pub damp: f32,
    pub wires: f32,
    pub feed: f32,
    pub tone: f32,
    pub key: f32,
    pub attack_ms: f32,
    pub decay: f32,
    pub release_ms: f32,
    pub vel: f32,
    pub level: f32,
    pub crack: f32,
    pub bright: f32,
    pub crack_mix: f32,
    pub bloom_size: f32,
    pub bloom_decay: f32,
    pub bloom_damp: f32,
    pub bloom_mix: f32,
}

impl Default for ClayParams {
    fn default() -> Self {
        let at = |id: u32| crate::params::def(cp::TABLE, id).default;
        Self {
            matter: at(cp::MATTER),
            size: at(cp::SIZE),
            strike: at(cp::STRIKE),
            drop: at(cp::DROP),
            damp: at(cp::DAMP),
            wires: at(cp::WIRES),
            feed: at(cp::FEED),
            tone: at(cp::TONE),
            key: at(cp::KEY),
            attack_ms: at(cp::ATTACK),
            decay: at(cp::DECAY),
            release_ms: at(cp::RELEASE),
            vel: at(cp::VEL),
            level: at(cp::LEVEL),
            crack: at(cp::CRACK),
            bright: at(cp::BRIGHT),
            crack_mix: at(cp::CRACK_MIX),
            bloom_size: at(cp::BLOOM_SIZE),
            bloom_decay: at(cp::BLOOM_DECAY),
            bloom_damp: at(cp::BLOOM_DAMP),
            bloom_mix: at(cp::BLOOM_MIX),
        }
    }
}

impl ClayParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(cp::TABLE, param, value) else {
            return;
        };
        match param {
            cp::MATTER => self.matter = value,
            cp::SIZE => self.size = value,
            cp::STRIKE => self.strike = value,
            cp::DROP => self.drop = value,
            cp::DAMP => self.damp = value,
            cp::WIRES => self.wires = value,
            cp::FEED => self.feed = value,
            cp::TONE => self.tone = value,
            cp::KEY => self.key = value,
            cp::ATTACK => self.attack_ms = value,
            cp::DECAY => self.decay = value,
            cp::RELEASE => self.release_ms = value,
            cp::VEL => self.vel = value,
            cp::LEVEL => self.level = value,
            cp::CRACK => self.crack = value,
            cp::BRIGHT => self.bright = value,
            cp::CRACK_MIX => self.crack_mix = value,
            cp::BLOOM_SIZE => self.bloom_size = value,
            cp::BLOOM_DECAY => self.bloom_decay = value,
            cp::BLOOM_DAMP => self.bloom_damp = value,
            cp::BLOOM_MIX => self.bloom_mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            cp::MATTER => self.matter,
            cp::SIZE => self.size,
            cp::STRIKE => self.strike,
            cp::DROP => self.drop,
            cp::DAMP => self.damp,
            cp::WIRES => self.wires,
            cp::FEED => self.feed,
            cp::TONE => self.tone,
            cp::KEY => self.key,
            cp::ATTACK => self.attack_ms,
            cp::DECAY => self.decay,
            cp::RELEASE => self.release_ms,
            cp::VEL => self.vel,
            cp::LEVEL => self.level,
            cp::CRACK => self.crack,
            cp::BRIGHT => self.bright,
            cp::CRACK_MIX => self.crack_mix,
            cp::BLOOM_SIZE => self.bloom_size,
            cp::BLOOM_DECAY => self.bloom_decay,
            cp::BLOOM_DAMP => self.bloom_damp,
            cp::BLOOM_MIX => self.bloom_mix,
            _ => 0.0,
        }
    }

    // --- the macros' couplings: one arithmetic for the engine and the
    // --- pictures ----------------------------------------------------

    /// The body's partial ratios at MATTER: skin, then wood, then metal,
    /// morphed in the log domain so the middle is a real object too.
    pub fn ratios(&self) -> [f32; PARTIALS] {
        let m = self.matter.clamp(0.0, 1.0);
        let (from, to, t) = if m < 0.5 {
            (SKIN, WOOD, m * 2.0)
        } else {
            (WOOD, METAL, (m - 0.5) * 2.0)
        };
        let mut out = [1.0; PARTIALS];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (from[i].ln() * (1.0 - t) + to[i].ln() * t).exp();
        }
        out
    }

    /// The partials' weights: a skin is nearly one sine (the 808), a
    /// metal is a bank. TONE adds to the second.
    pub fn weights(&self) -> [f32; PARTIALS] {
        let m = self.matter.clamp(0.0, 1.0);
        let roll = 2.4 * (1.0 - m) + 0.35 * m;
        let mut out = [0.0; PARTIALS];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (-(i as f32) * roll).exp();
        }
        out[1] += self.tone * 0.8;
        out
    }

    /// How much of the body reaches the out: a skin is all body, a
    /// metal is mostly noise.
    pub fn body_share(&self) -> f32 {
        1.0 - 0.65 * self.matter.clamp(0.0, 1.0)
    }

    /// Which of the voice's parts still sound, for a test to name.
    #[cfg(test)]
    pub fn debug_active(v: &ClayVoice) -> String {
        format!(
            "gate {} partials {:?} noise {} wires {} release {}",
            v.gate,
            v.envs.iter().map(|e| e.current()).collect::<Vec<_>>(),
            v.noise_env.current(),
            v.wires_env.current(),
            v.release_env.current()
        )
    }

    /// SIZE's fundamental, before the note.
    pub fn size_hz(&self) -> f32 {
        SIZE_LOW_HZ * (self.size.clamp(0.0, 1.0) * SIZE_OCTAVES).exp2()
    }

    /// The fundamental for a note: SIZE's, overridden by the note at KEY.
    pub fn fundamental_hz(&self, pitch: u8) -> f32 {
        let note_hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        let size = self.size_hz();
        (size.ln() * (1.0 - self.key) + note_hz.ln() * self.key).exp()
    }

    /// The body's base decay, 60 dB time: SIZE's, scaled by DECAY.
    pub fn base_decay_ms(&self) -> f32 {
        let s = self.size.clamp(0.0, 1.0);
        DECAY_LONG_MS * (DECAY_SHORT_MS / DECAY_LONG_MS).powf(s) * self.decay
    }

    /// Partial `i`'s decay: shorter up the ladder by DAMP, and shorter
    /// again toward metal.
    pub fn partial_decay_ms(&self, i: usize) -> f32 {
        let m = self.matter.clamp(0.0, 1.0);
        let metal = 1.0 - 0.75 * (m - 0.5).max(0.0) * 2.0;
        // The wires damp the head they lie on: a snare's shell ring is
        // short and its rattle outlasts it.
        let wired = 1.0 - 0.7 * self.wires.clamp(0.0, 1.0);
        self.base_decay_ms() * (-(i as f32) * self.damp * 1.5).exp() * metal * wired
    }

    /// The noise burst's band centre, quality, level and decay.
    pub fn noise_band(&self, strike: f32) -> (f32, f32) {
        let m = self.matter.clamp(0.0, 1.0);
        let hz = 400.0 * (m * 4.0 + strike * 1.5).exp2();
        (hz, 0.7 + 1.3 * m)
    }

    /// The burst's level. A band-passed white noise is quiet against a
    /// sine of the same amplitude, so the share is scaled up; a soft
    /// skin hit has almost none, a hard metal hit is all burst.
    pub fn noise_level(&self, strike: f32) -> f32 {
        let m = self.matter.clamp(0.0, 1.0);
        strike * (0.1 + 0.9 * m) * NOISE_GAIN
    }

    pub fn noise_decay_ms(&self) -> f32 {
        let m = self.matter.clamp(0.0, 1.0);
        self.base_decay_ms() * (0.12 + 0.6 * m)
    }

    /// The wires' band: follows SIZE.
    pub fn wires_hz(&self) -> f32 {
        1_200.0 * (self.size.clamp(0.0, 1.0) * 2.0).exp2()
    }

    /// The drop's time: soft hits fall slower.
    pub fn drop_ms(&self, strike: f32) -> f32 {
        40.0 + 80.0 * (1.0 - strike)
    }

    /// The body's drive: a hard-struck skin is driven into the clip.
    pub fn drive(&self, strike: f32) -> f32 {
        let m = self.matter.clamp(0.0, 1.0);
        (DRIVE_MIN + 5.0 * strike * (1.0 - m)).min(DRIVE_MAX)
    }
}

/// The voice: one, retriggered by every strike.
pub struct ClayVoice {
    sample_rate: f32,
    params: ClayParams,
    base: ClayParams,

    oscs: [MipOsc; PARTIALS],
    envs: [ExpDecay; PARTIALS],
    sine_tables: Vec<f32>,
    noise: WhiteNoise,
    wire_noise: WhiteNoise,
    click_noise: WhiteNoise,
    noise_env: ExpDecay,
    wires_env: ExpDecay,
    click_env: ExpDecay,
    drop_env: ExpDecay,
    release_env: ExpDecay,
    amp: Adsr,
    noise_band: Svf,
    wires_band: Svf,
    click_hp: Svf,
    shaper: Waveshaper,
    split: TransientSplit,
    brighten: SlewBrighten,
    bloom: Reverb,
    bloom_buffers: Vec<f32>,

    f0: f32,
    velocity: f32,
    strike: f32,
    gate: bool,

    tuned_noise: (f32, f32),
    tuned_wires: f32,
    tuned_drive: f32,
    tuned_bloom: (f32, f32, f32),
    tuned_bright: f32,

    chunk_left: usize,
    sig: [f32; CHUNK],
    body: [f32; CHUNK],
    env: [f32; CHUNK],
    burst: [f32; CHUNK],
    weight: [f32; CHUNK],
    wet: [f32; CHUNK],
}

impl Default for ClayVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl ClayVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: ClayParams::default(),
            base: ClayParams::default(),
            oscs: std::array::from_fn(|_| MipOsc::new()),
            envs: std::array::from_fn(|_| ExpDecay::new()),
            sine_tables: Vec::new(),
            noise: WhiteNoise::new(),
            wire_noise: WhiteNoise::new(),
            click_noise: WhiteNoise::new(),
            noise_env: ExpDecay::new(),
            wires_env: ExpDecay::new(),
            click_env: ExpDecay::new(),
            drop_env: ExpDecay::new(),
            release_env: ExpDecay::new(),
            amp: Adsr::new(),
            noise_band: Svf::new(),
            wires_band: Svf::new(),
            click_hp: Svf::new(),
            shaper: Waveshaper::new(),
            split: TransientSplit::new(),
            brighten: SlewBrighten::new(),
            bloom: Reverb::new(),
            bloom_buffers: Vec::new(),
            f0: SIZE_LOW_HZ,
            velocity: 1.0,
            strike: 0.0,
            gate: false,
            tuned_noise: (0.0, 0.0),
            tuned_wires: 0.0,
            tuned_drive: f32::NAN,
            tuned_bloom: (-1.0, -1.0, -1.0),
            tuned_bright: -1.0,
            chunk_left: 0,
            sig: [0.0; CHUNK],
            body: [0.0; CHUNK],
            env: [0.0; CHUNK],
            burst: [0.0; CHUNK],
            weight: [0.0; CHUNK],
            wet: [0.0; CHUNK],
        }
    }

    /// Green zone: the sine table, the reverb's lines, every kernel.
    /// The allocations in this file all happen here.
    pub fn prepare(&mut self, sample_rate: f32, params: ClayParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;
        self.sine_tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.sine_tables);
        for o in &mut self.oscs {
            o.prepare(fs, Waveform::Sine);
        }
        self.noise.seed(0xc1a7_5eed_0001);
        self.wire_noise.seed(0xc1a7_5eed_0002);
        self.click_noise.seed(0xc1a7_5eed_0003);
        self.click_hp.prepare(fs, CLICK_HP_HZ.min(fs * 0.45), 0.707);
        self.click_env.prepare(fs, CLICK_MS);
        self.split.prepare(fs, CRACK_WINDOW_MS);
        self.brighten.prepare(fs, 3_000.0, 0.0);
        self.bloom_buffers.resize(Reverb::buffer_len(fs), 0.0);
        self.bloom.prepare(fs, &mut self.bloom_buffers);
        self.params = params;
        self.base = params;
        self.apply_envelopes();
        self.reset();
    }

    /// Timings from the params: the partials' decays, the bursts', the
    /// drop, the release, the attack.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        for (i, env) in self.envs.iter_mut().enumerate() {
            env.prepare(fs, self.params.partial_decay_ms(i));
        }
        self.noise_env.prepare(fs, self.params.noise_decay_ms());
        self.wires_env
            .prepare(fs, (self.params.base_decay_ms() * 0.7).max(20.0));
        self.drop_env.prepare(fs, self.params.drop_ms(self.strike));
        self.release_env.prepare(fs, self.params.release_ms);
        self.amp.prepare(
            fs,
            self.params.attack_ms,
            1.0e6,
            1.0,
            self.params.release_ms,
        );
    }

    /// Green zone: silence, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.gate = false;
        for o in &mut self.oscs {
            o.reset();
        }
        for env in &mut self.envs {
            env.reset();
        }
        self.noise.reset();
        self.wire_noise.reset();
        self.click_noise.reset();
        self.noise_env.reset();
        self.wires_env.reset();
        self.click_env.reset();
        self.drop_env.reset();
        self.release_env.reset();
        self.amp.reset();
        self.noise_band.reset();
        self.wires_band.reset();
        self.click_hp.reset();
        self.split.reset();
        self.brighten.reset();
        self.bloom
            .prepare(self.sample_rate, &mut self.bloom_buffers);
        self.tuned_noise = (0.0, 0.0);
        self.tuned_wires = 0.0;
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

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
            cp::MATTER
            | cp::SIZE
            | cp::DAMP
            | cp::DECAY
            | cp::ATTACK
            | cp::RELEASE
            | cp::WIRES
            | cp::STRIKE => self.apply_envelopes(),
            _ => {}
        }
    }

    pub fn params(&self) -> ClayParams {
        self.params
    }

    /// Whether anything still sounds: a partial, a burst, or a held feed.
    pub fn active(&self) -> bool {
        self.gate
            || self.envs.iter().any(ExpDecay::active)
            || self.noise_env.active()
            || self.wires_env.active()
            || self.release_env.active()
    }

    /// Strike. Every partial restarts at phase zero so the hit is the
    /// same each time; the bursts restart; the gate opens for FEED.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        self.strike = (self.params.strike * (0.6 + 0.4 * self.velocity)).clamp(0.0, 1.0);
        self.f0 = self.params.fundamental_hz(pitch);
        self.drop_env
            .prepare(self.sample_rate, self.params.drop_ms(self.strike));
        for o in &mut self.oscs {
            o.reset();
        }
        for env in &mut self.envs {
            env.trigger(1.0);
        }
        self.noise_env.trigger(1.0);
        self.wires_env.trigger(1.0);
        self.click_env.trigger(1.0);
        self.drop_env.trigger(1.0);
        self.release_env.reset();
        self.gate = true;
        self.amp.gate_on();
        self.chunk_left = 0;
    }

    /// The gate goes down. A struck object ignores it; a FED one starts
    /// its release.
    pub fn release(&mut self) {
        if !self.gate {
            return;
        }
        self.gate = false;
        if self.params.feed > 0.001 {
            self.release_env.trigger(1.0);
        }
    }

    /// Red zone: render into `out`, ADDING to what is there.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.sine_tables.is_empty() {
            return;
        }
        let mut written = 0usize;
        while written < out.len() {
            if self.chunk_left == 0 {
                self.start_chunk();
                self.chunk_left = CHUNK;
            }
            let take = self.chunk_left.min(out.len() - written).max(1);
            let Some(block) = out.get_mut(written..written + take) else {
                return;
            };
            self.render_run(block, gain);
            self.chunk_left = self.chunk_left.saturating_sub(take);
            written += take;
        }
    }

    /// The start of a control chunk: the partials' pitches under the
    /// drop, the bands, the drive, the FX rules.
    fn start_chunk(&mut self) {
        let fs = self.sample_rate;
        let ceiling = fs * 0.45;
        let p = self.params;
        let drop = (p.drop * self.drop_env.current() / 12.0).exp2();
        let ratios = p.ratios();
        for (o, ratio) in self.oscs.iter_mut().zip(ratios) {
            o.set_freq((self.f0 * ratio * drop).clamp(1.0, ceiling));
        }
        let (hz, q) = p.noise_band(self.strike);
        let hz = hz.min(ceiling);
        if (hz - self.tuned_noise.0).abs() > self.tuned_noise.0.max(1.0) * 1e-4
            || (q - self.tuned_noise.1).abs() > 1e-4
        {
            self.tuned_noise = (hz, q);
            self.noise_band.prepare(fs, hz, q);
        }
        let wires = p.wires_hz().min(ceiling);
        if (wires - self.tuned_wires).abs() > self.tuned_wires.max(1.0) * 1e-4 {
            self.tuned_wires = wires;
            self.wires_band.prepare(fs, wires, 0.9);
        }
        let drive = p.drive(self.strike);
        if drive != self.tuned_drive {
            self.tuned_drive = drive;
            self.shaper.configure(ShapeMode::SoftClip, drive, 0.0, 1.0);
        }
        let bloom = (
            p.bloom_size * 0.5 + 0.2 * p.size,
            p.bloom_decay,
            p.bloom_damp,
        );
        if bloom != self.tuned_bloom {
            self.tuned_bloom = bloom;
            self.bloom.set_room(bloom.0, bloom.1, bloom.2);
        }
        if p.bright != self.tuned_bright {
            self.tuned_bright = p.bright;
            self.brighten.set_amount(p.bright);
        }
    }

    /// One run inside a chunk: the per-sample work.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(sig), Some(body), Some(env), Some(burst), Some(weight), Some(wet)) = (
            self.sig.get_mut(..n),
            self.body.get_mut(..n),
            self.env.get_mut(..n),
            self.burst.get_mut(..n),
            self.weight.get_mut(..n),
            self.wet.get_mut(..n),
        ) else {
            return;
        };
        let p = self.params;
        // The drop runs at sample rate; the chunk reads it.
        self.drop_env.process(env);

        // --- the body: six decaying sines, held up by FEED --------------
        for s in body.iter_mut() {
            *s = 0.0;
        }
        let feed = if self.gate {
            p.feed
        } else {
            p.feed * self.release_env.current()
        };
        if !self.gate {
            self.release_env.process(burst);
        }
        let weights = p.weights();
        for (i, (o, part_env)) in self.oscs.iter_mut().zip(self.envs.iter_mut()).enumerate() {
            o.process(sig, &self.sine_tables);
            part_env.process(env);
            let w = weights[i];
            for (b, (s, e)) in body.iter_mut().zip(sig.iter().zip(env.iter())) {
                *b += *s * e.max(feed * w.min(1.0)) * w;
            }
        }
        self.shaper.process(body);
        let share = p.body_share();
        for s in body.iter_mut() {
            *s *= share;
        }

        // --- the noise burst ---------------------------------------------
        self.noise.process(burst);
        self.noise_band.process(burst, Mode::BandpassUnity);
        self.noise_env.process(env);
        let noise_level = p.noise_level(self.strike);
        for (b, (x, e)) in body.iter_mut().zip(burst.iter().zip(env.iter())) {
            *b += *x * *e * noise_level;
        }

        // --- the wires: gated by the body's own first partial -----------
        // The envelope runs whether or not the wires are heard, so a
        // silent set still finishes and the voice can go idle.
        self.wires_env.process(env);
        if p.wires > 0.0 {
            self.wire_noise.process(burst);
            self.wires_band.process(burst, Mode::BandpassUnity);
            let level = p.wires * (0.3 + 0.7 * self.strike) * NOISE_GAIN;
            for (b, (x, e)) in body.iter_mut().zip(burst.iter().zip(env.iter())) {
                *b += *x * *e * level;
            }
        }

        // --- the click --------------------------------------------------
        if self.click_env.active() {
            self.click_noise.process(burst);
            self.click_hp.process(burst, Mode::Highpass);
            self.click_env.process(env);
            let level = self.strike * 0.6;
            for (b, (x, e)) in body.iter_mut().zip(burst.iter().zip(env.iter())) {
                *b += *x * *e * level;
            }
        }

        // --- the amp -----------------------------------------------------
        self.amp.process(env);
        let vel = (1.0 - p.vel) + p.vel * self.velocity;
        for (b, e) in body.iter_mut().zip(env.iter()) {
            *b *= *e * vel;
        }

        // --- CRACK: the strike's share, boosted or cut -------------------
        if p.crack.abs() > 0.001 || p.bright > 0.001 {
            self.split.process(body, weight);
            sig.copy_from_slice(body);
            for (s, w) in sig.iter_mut().zip(weight.iter()) {
                let shaped = if p.crack >= 0.0 {
                    1.0 + p.crack * 2.0 * *w
                } else {
                    1.0 + p.crack * *w
                };
                *s *= shaped;
            }
            self.brighten.process(sig);
            let mix = p.crack_mix;
            for (b, s) in body.iter_mut().zip(sig.iter()) {
                *b += (*s - *b) * mix;
            }
        }

        // --- BLOOM ---------------------------------------------------------
        if p.bloom_mix > 0.001 {
            self.bloom.process(body, wet, &mut self.bloom_buffers);
            for (b, w) in body.iter_mut().zip(wet.iter()) {
                *b += *w * p.bloom_mix;
            }
        }

        let level = p.level * gain;
        for (o, b) in out.iter_mut().zip(body.iter()) {
            *o += *b * level;
        }
    }
}

// ------------------------------------------------------------ pictures ---

/// The hero picture for one sub-page, the selected cell's series lit.
pub fn hero(params: &ClayParams, subpage: &str, selected: Option<u32>) -> Option<Hero> {
    match subpage {
        "Clay" => Some(clay_picture(params, selected)),
        "Amp" => Some(amp_picture(params, selected)),
        "Crack" => Some(crack_picture(params, selected)),
        "Bloom" => Some(bloom_picture(params, selected)),
        _ => None,
    }
}

const POINTS: usize = 96;

fn lit(selected: Option<u32>, ids: &[u32]) -> bool {
    selected.is_some_and(|id| ids.contains(&id))
}

/// The body's partials as bars on a log axis, the noise band as a
/// washed bell over them, the fundamental marked.
fn clay_picture(p: &ClayParams, selected: Option<u32>) -> Hero {
    const LO: f32 = 20.0;
    const HI: f32 = 20_000.0;
    let x_of = |hz: f32| (hz.clamp(LO, HI) / LO).ln() / (HI / LO).ln();
    let f0 = p.size_hz();
    let ratios = p.ratios();
    let weights = p.weights();
    let share = p.body_share();
    let longest = p.partial_decay_ms(0).max(1.0);
    let mut series = Vec::new();
    // Each partial: a bar whose height is its weight and whose width
    // is its decay against the fundamental's.
    for i in 0..PARTIALS {
        let x = x_of(f0 * ratios[i]);
        let h = (weights[i] * share).clamp(0.0, 1.0);
        let half = 0.004 + 0.02 * (p.partial_decay_ms(i) / longest);
        series.push(HeroSeries {
            name: "partial",
            points: vec![
                ((x - half).max(0.0), 0.0),
                ((x - half).max(0.0), h),
                ((x + half).min(1.0), h),
                ((x + half).min(1.0), 0.0),
            ],
            lit: lit(
                selected,
                &[cp::MATTER, cp::SIZE, cp::TONE, cp::DAMP, cp::DROP, cp::KEY],
            ),
        });
    }
    let (hz, q) = p.noise_band(p.strike);
    let level = p.noise_level(p.strike);
    let bell: Vec<(f32, f32)> = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let f = LO * (HI / LO).powf(x);
            let r = f / hz.max(1.0);
            let mag = (r / q) / ((1.0 - r * r).powi(2) + (r / q).powi(2)).sqrt();
            (x, (mag * level).clamp(0.0, 1.0))
        })
        .collect();
    series.push(HeroSeries {
        name: "noise",
        points: bell,
        lit: lit(selected, &[cp::STRIKE, cp::WIRES, cp::FEED]),
    });
    Hero {
        waveform: None,
        title: "BODY · level over Hz".to_owned(),
        series,
        marks: vec![HeroMark {
            x: x_of(f0),
            label: format!("{f0:.0} Hz"),
            lit: lit(selected, &[cp::SIZE, cp::KEY]),
        }],
        x_labels: ["20 Hz".to_owned(), "20 kHz".to_owned()],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// The level over time: the attack, the fundamental's decay, FEED's
/// floor while held, the release.
fn amp_picture(p: &ClayParams, selected: Option<u32>) -> Hero {
    let decay = p.partial_decay_ms(0).max(1.0);
    let hold = decay * 0.5;
    let total = p.attack_ms + decay + hold + p.release_ms;
    let at = |ms: f32| ms / total.max(1.0);
    let attack: Vec<(f32, f32)> = (0..12)
        .map(|i| {
            let t = i as f32 / 11.0;
            (at(p.attack_ms * t), t)
        })
        .collect();
    let body: Vec<(f32, f32)> = (0..POINTS)
        .map(|i| {
            let t = i as f32 / (POINTS - 1) as f32;
            let ms = t * (decay + hold);
            let e = (-DECADES * ms / decay).exp().max(p.feed);
            (at(p.attack_ms + ms), e)
        })
        .collect();
    let end = p.feed.max((-DECADES * (decay + hold) / decay).exp());
    let release: Vec<(f32, f32)> = (0..24)
        .map(|i| {
            let t = i as f32 / 23.0;
            (
                at(p.attack_ms + decay + hold + p.release_ms * t),
                end * (-DECADES * t).exp(),
            )
        })
        .collect();
    Hero {
        waveform: None,
        title: "AMP · level over ms".to_owned(),
        series: vec![
            HeroSeries {
                name: "attack",
                points: attack,
                lit: lit(selected, &[cp::ATTACK]) && p.attack_ms > 0.0,
            },
            HeroSeries {
                name: "body",
                points: body,
                lit: lit(selected, &[cp::DECAY, cp::FEED, cp::VEL, cp::LEVEL]),
            },
            HeroSeries {
                name: "release",
                points: release,
                lit: lit(selected, &[cp::RELEASE]),
            },
        ],
        marks: Vec::new(),
        x_labels: ["0".to_owned(), format!("{total:.0} ms")],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// The strike's weight over a hit and what CRACK makes of it.
fn crack_picture(p: &ClayParams, selected: Option<u32>) -> Hero {
    let gain: Vec<(f32, f32)> = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let w = (-x * 8.0).exp();
            let g = if p.crack >= 0.0 {
                1.0 + p.crack * 2.0 * w
            } else {
                1.0 + p.crack * w
            };
            (x, (g / 3.0).clamp(0.0, 1.0))
        })
        .collect();
    Hero {
        waveform: None,
        title: "CRACK · gain over the hit".to_owned(),
        series: vec![HeroSeries {
            name: "gain",
            points: gain,
            lit: lit(selected, &[cp::CRACK, cp::BRIGHT, cp::CRACK_MIX]),
        }],
        marks: vec![HeroMark {
            x: 0.0,
            label: format!("{CRACK_WINDOW_MS:.0} ms"),
            lit: false,
        }],
        x_labels: ["strike".to_owned(), "body".to_owned()],
        y_labels: ["0".to_owned(), "×3".to_owned()],
        diagonal: false,
    }
}

/// The bloom's decay over time.
fn bloom_picture(p: &ClayParams, selected: Option<u32>) -> Hero {
    let seconds = 0.2 + 3.8 * p.bloom_decay;
    let tail: Vec<(f32, f32)> = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            (x, (p.bloom_mix * (-DECADES * x).exp()).clamp(0.0, 1.0))
        })
        .collect();
    Hero {
        waveform: None,
        title: "BLOOM · level over s".to_owned(),
        series: vec![HeroSeries {
            name: "tail",
            points: tail,
            lit: lit(
                selected,
                &[
                    cp::BLOOM_SIZE,
                    cp::BLOOM_DECAY,
                    cp::BLOOM_DAMP,
                    cp::BLOOM_MIX,
                ],
            ),
        }],
        marks: Vec::new(),
        x_labels: ["0".to_owned(), format!("{seconds:.1} s")],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(params: ClayParams) -> ClayVoice {
        let mut v = ClayVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut ClayVoice, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    /// Zero crossings per second: the pitch, roughly, of a tone.
    fn rate_hz(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count();
        crossings as f32 / 2.0 * FS / samples.len() as f32
    }

    /// The share of the energy above `hz`: how NOISY, not tonal, a
    /// sound is.
    fn high_share(samples: &[f32], hz: f32) -> f32 {
        let mut hp = Svf::new();
        hp.prepare(FS, hz, 0.707);
        let mut high = samples.to_vec();
        hp.process(&mut high, Mode::Highpass);
        let total = rms(samples).max(1e-9);
        rms(&high) / total
    }

    fn kick() -> ClayParams {
        ClayParams {
            matter: 0.0,
            size: 0.15,
            strike: 0.3,
            drop: 24.0,
            wires: 0.0,
            ..ClayParams::default()
        }
    }

    fn snare() -> ClayParams {
        ClayParams {
            matter: 0.2,
            size: 0.45,
            strike: 0.7,
            drop: 6.0,
            wires: 0.9,
            ..ClayParams::default()
        }
    }

    fn hat() -> ClayParams {
        ClayParams {
            matter: 1.0,
            size: 0.85,
            strike: 0.8,
            drop: 0.0,
            wires: 0.0,
            ..ClayParams::default()
        }
    }

    fn struck(params: ClayParams, samples: usize) -> Vec<f32> {
        let mut v = voice(params);
        v.trigger(36, 110);
        render(&mut v, samples)
    }

    #[test]
    fn silent_until_struck_and_then_it_sounds() {
        let mut v = voice(ClayParams::default());
        assert_eq!(peak(&render(&mut v, 4_800)), 0.0);
        v.trigger(36, 100);
        assert!(peak(&render(&mut v, 4_800)) > 0.05);
    }

    /// The 808: a skin at the bottom is one long low sine with a drop
    /// and a fat, driven body — tonal, sustained, no hiss.
    #[test]
    fn the_kick_is_a_fat_808() {
        let out = struck(kick(), 48_000);
        let first = &out[..1_920];
        let late = &out[9_600..24_000];
        assert!(rate_hz(first) > rate_hz(late) * 1.5, "no drop");
        let tail_hz = rate_hz(&out[24_000..48_000]);
        assert!(
            (40.0..75.0).contains(&tail_hz),
            "the tail sits at {tail_hz} Hz, not the 808's"
        );
        assert!(
            high_share(late, 1_000.0) < 0.08,
            "the kick hisses: {}",
            high_share(late, 1_000.0)
        );
        // Fat: still there well past a quarter second.
        let at_300 = peak(&out[14_400..16_800]);
        assert!(at_300 > 0.2 * peak(&out), "the kick is thin: {at_300}");
        // Driven: the body is not a pure sine (the soft clip adds harmonics).
        assert!(high_share(late, 120.0) > 0.05, "no harmonics: not driven");
    }

    /// The snare is not tonal: past the strike, most of its energy is
    /// noise above a kilohertz, and the wires outlast the body.
    #[test]
    fn the_snare_is_mostly_noise() {
        let out = struck(snare(), 24_000);
        let after = &out[1_440..9_600];
        let share = high_share(after, 1_000.0);
        assert!(share > 0.5, "the snare is tonal: high share {share}");
        assert!(peak(&out[4_800..9_600]) > 0.02, "no tail");
    }

    /// The hat is noise with an inharmonic ring, short, and gone fast.
    #[test]
    fn the_hat_is_noise_and_short() {
        let out = struck(hat(), 24_000);
        let after = &out[480..4_800];
        let share = high_share(after, 2_000.0);
        assert!(share > 0.7, "the hat is tonal: high share {share}");
        assert!(
            peak(&out[19_200..]) < 0.01 * peak(&out).max(1e-6),
            "the hat rings on"
        );
    }

    /// Walking MATTER from skin to metal never goes silent and gets
    /// noisier the whole way: no type boundary hides in the middle.
    #[test]
    fn matter_is_continuous_and_noisier_toward_metal() {
        let mut last_share = 0.0;
        for step in 0..=10 {
            let params = ClayParams {
                matter: step as f32 / 10.0,
                size: 0.5,
                strike: 0.6,
                wires: 0.0,
                ..ClayParams::default()
            };
            let out = struck(params, 9_600);
            assert!(peak(&out) > 0.02, "silent at matter {}", step as f32 / 10.0);
            let share = high_share(&out[480..], 1_500.0);
            assert!(
                share + 0.08 >= last_share,
                "noise fell at matter {}: {share} after {last_share}",
                step as f32 / 10.0
            );
            last_share = last_share.max(share);
        }
    }

    /// FEED holds the body while the gate is held and lets it go on
    /// release: the bass.
    #[test]
    fn feed_holds_a_bass_until_the_gate_lets_go() {
        let params = ClayParams {
            matter: 0.0,
            size: 0.3,
            feed: 1.0,
            key: 1.0,
            release_ms: 60.0,
            ..ClayParams::default()
        };
        let mut v = voice(params);
        v.trigger(36, 100);
        let _ = render(&mut v, 48_000);
        let held = peak(&render(&mut v, 4_800));
        assert!(held > 0.1, "the feed did not hold: {held}");
        v.release();
        let _ = render(&mut v, 9_600);
        assert!(
            peak(&render(&mut v, 4_800)) < 0.01,
            "the release did not end"
        );
        // The partials' own tails reach the floor a little later.
        let _ = render(&mut v, 96_000);
        assert!(!v.active(), "{}", ClayParams::debug_active(&v));
        // Without FEED the gate going down changes nothing.
        let mut struck = voice(kick());
        struck.trigger(36, 100);
        let _ = render(&mut struck, 2_400);
        struck.release();
        assert!(peak(&render(&mut struck, 4_800)) > 0.1);
    }

    /// KEY 1 plays the note: an octave up doubles the rate.
    #[test]
    fn key_follows_the_note() {
        let params = ClayParams {
            matter: 0.0,
            size: 0.3,
            feed: 1.0,
            key: 1.0,
            drop: 0.0,
            strike: 0.0,
            ..ClayParams::default()
        };
        let mut low = voice(params);
        low.trigger(36, 100);
        let _ = render(&mut low, 9_600);
        let mut high = voice(params);
        high.trigger(48, 100);
        let _ = render(&mut high, 9_600);
        let a = rate_hz(&render(&mut low, 24_000));
        let b = rate_hz(&render(&mut high, 24_000));
        assert!((b / a - 2.0).abs() < 0.15, "{a} then {b}");
    }

    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let mut once = voice(kick());
        once.trigger(36, 127);
        let single = peak(&render(&mut once, 4_800));
        let mut twice = voice(kick());
        twice.trigger(36, 127);
        let _ = render(&mut twice, 240);
        twice.trigger(36, 127);
        let doubled = peak(&render(&mut twice, 4_800));
        assert!(doubled <= single * 1.05, "stacked: {doubled} vs {single}");
    }

    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        for params in [kick(), snare(), hat()] {
            let params = ClayParams {
                crack: 0.5,
                bright: 0.3,
                bloom_mix: 0.3,
                ..params
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
            assert!(first.is_none(), "first difference at {first:?}");
        }
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(ClayParams {
            wires: 1.0,
            crack: 1.0,
            bright: 1.0,
            bloom_mix: 1.0,
            feed: 0.5,
            ..snare()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(if i % 32 == 0 { 36 } else { 48 }, 100);
                }
                if i % 16 == 8 {
                    v.release();
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in cp::TABLE {
            for extreme in [def.min, def.max] {
                let mut params = ClayParams {
                    wires: 1.0,
                    crack: 1.0,
                    bright: 1.0,
                    bloom_mix: 1.0,
                    ..ClayParams::default()
                };
                params.set(def.id, extreme);
                let mut v = voice(params);
                v.trigger(36, 127);
                let out = render(&mut v, 4_096);
                assert!(
                    out.iter().all(|s| s.is_finite()),
                    "{} at {extreme} went non-finite",
                    def.name
                );
                v.trigger(108, 127);
                let out = render(&mut v, 4_096);
                assert!(out.iter().all(|s| s.is_finite()));
            }
        }
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = ClayParams::default();
        for def in cp::TABLE {
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
    }

    #[test]
    fn a_lock_is_heard_on_its_hit_and_the_knob_comes_back() {
        let mut v = voice(ClayParams::default());
        v.set_param(cp::MATTER, 0.2);
        v.plock(cp::MATTER, Some(0.9));
        assert_eq!(v.params().get(cp::MATTER), 0.9);
        v.plock(cp::MATTER, None);
        assert_eq!(v.params().get(cp::MATTER), 0.2);
    }

    #[test]
    fn every_subpage_has_a_picture_in_the_unit_square() {
        let p = ClayParams::default();
        for (page, id) in [
            ("Clay", cp::MATTER),
            ("Clay", cp::STRIKE),
            ("Amp", cp::FEED),
            ("Crack", cp::CRACK),
            ("Bloom", cp::BLOOM_MIX),
        ] {
            let hero = hero(&p, page, Some(id)).unwrap_or_else(|| panic!("{page}"));
            assert!(hero.series.iter().any(|s| s.lit), "{page}: nothing lit");
            for series in &hero.series {
                assert!(series.points.len() >= 2);
                assert!(
                    series
                        .points
                        .iter()
                        .all(|(x, y)| { (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y) })
                );
            }
        }
        assert!(hero(&p, "Nowhere", None).is_none());
        // Skin is nearly a sine; metal is a bank.
        let skin = ClayParams { matter: 0.0, ..p };
        let metal = ClayParams { matter: 1.0, ..p };
        assert!(skin.weights()[1] < 0.3 && metal.weights()[1] > 0.6);
    }
}
