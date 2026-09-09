//! THUMP — the kick machine, written against the pages.
//!
//! One sine and its two pitch envelopes: CLICK is fast and wide, the
//! transient's whip; BEND is slow and narrow, the body's fall. A filtered
//! noise burst with its own decay sits on the front. An ADSR shapes the
//! whole, then the saturator, the compressor and a harmonic-following
//! disperser — one whose allpass corner rides the sine's CURRENT
//! fundamental, times the harmonic FOCUS asks for, so its chirp belongs
//! to the note instead of sitting on top of it.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic is in
//! `src/dsp/`. The old KICK stays as reference and parts.
//!
//! # The path
//!
//! ```text
//! SINE ── pitch = TUNE × CLICK env × BEND env ─┐
//! NOISE ── SVF (COLOR, RESO, MODE, TRACK) ── burst env ─┼─▶ ADSR ─▶ SAT ─▶ COMP ─▶ DISPERSE ─▶ out
//! ```
//!
//! Per SAMPLE where modulation is a multiply, per CONTROL CHUNK where it
//! is a coefficient — the sine's frequency, the noise filter and the
//! disperser's corner are rebuilt once per [`CHUNK`].

use crate::dsp::adsr::{Adsr, ExpDecay};
use crate::dsp::arith::{db_to_gain, gain_to_db};
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode as CompMode, RmsDetector};
use crate::dsp::filters::{Disperser, Mode, Svf};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{DRIVE_MAX, DRIVE_MIN, Mode as ShapeMode, Waveshaper};
use crate::pages::{Hero, HeroMark, HeroSeries};
use crate::params::thump as tp;

/// How often the pitch, the noise filter and the disperser are rebuilt,
/// in samples.
pub const CHUNK: usize = 32;
/// The fundamental at TUNE zero, played at the root.
pub const BASE_HZ: f32 = 50.0;
/// The most allpass stages AMOUNT reaches.
const STAGES_MAX: f32 = 32.0;
/// The compressor's detector window.
const RMS_WINDOW_MS: f32 = 5.0;
/// ln(1000): the kernel's 60 dB decay, so the picture and the engine
/// draw the same curve.
const DECADES: f32 = 6.907_755_4;

/// The kick's settings, in engine units. Parallel to [`tp::TABLE`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThumpParams {
    pub tune: f32,
    pub click: f32,
    pub click_ms: f32,
    pub bend: f32,
    pub bend_ms: f32,
    pub curve: f32,
    pub phase: f32,
    pub noise: f32,
    pub noise_ms: f32,
    pub color_hz: f32,
    pub reso: f32,
    pub mode: f32,
    pub track: f32,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub vel: f32,
    pub vel_click: f32,
    pub level: f32,
    pub sat_drive: f32,
    pub sat_shape: f32,
    pub sat_bias: f32,
    pub sat_mix: f32,
    pub comp_thresh_db: f32,
    pub comp_ratio: f32,
    pub comp_attack_ms: f32,
    pub comp_release_ms: f32,
    pub comp_knee_db: f32,
    pub comp_makeup_db: f32,
    pub comp_mix: f32,
    pub disp_amount: f32,
    pub disp_focus: f32,
    pub disp_width: f32,
    pub disp_follow: f32,
    pub disp_mix: f32,
}

impl Default for ThumpParams {
    fn default() -> Self {
        let at = |id: u32| crate::params::def(tp::TABLE, id).default;
        Self {
            tune: at(tp::TUNE),
            click: at(tp::CLICK),
            click_ms: at(tp::CLICK_TIME),
            bend: at(tp::BEND),
            bend_ms: at(tp::BEND_TIME),
            curve: at(tp::CURVE),
            phase: at(tp::PHASE),
            noise: at(tp::NOISE),
            noise_ms: at(tp::NOISE_DECAY),
            color_hz: at(tp::COLOR),
            reso: at(tp::RESO),
            mode: at(tp::MODE),
            track: at(tp::TRACK),
            attack_ms: at(tp::ATTACK),
            decay_ms: at(tp::DECAY),
            sustain: at(tp::SUSTAIN),
            release_ms: at(tp::RELEASE),
            vel: at(tp::VEL),
            vel_click: at(tp::VEL_CLICK),
            level: at(tp::LEVEL),
            sat_drive: at(tp::SAT_DRIVE),
            sat_shape: at(tp::SAT_SHAPE),
            sat_bias: at(tp::SAT_BIAS),
            sat_mix: at(tp::SAT_MIX),
            comp_thresh_db: at(tp::COMP_THRESH),
            comp_ratio: at(tp::COMP_RATIO),
            comp_attack_ms: at(tp::COMP_ATTACK),
            comp_release_ms: at(tp::COMP_RELEASE),
            comp_knee_db: at(tp::COMP_KNEE),
            comp_makeup_db: at(tp::COMP_MAKEUP),
            comp_mix: at(tp::COMP_MIX),
            disp_amount: at(tp::DISP_AMOUNT),
            disp_focus: at(tp::DISP_FOCUS),
            disp_width: at(tp::DISP_WIDTH),
            disp_follow: at(tp::DISP_FOLLOW),
            disp_mix: at(tp::DISP_MIX),
        }
    }
}

impl ThumpParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(tp::TABLE, param, value) else {
            return;
        };
        match param {
            tp::TUNE => self.tune = value,
            tp::CLICK => self.click = value,
            tp::CLICK_TIME => self.click_ms = value,
            tp::BEND => self.bend = value,
            tp::BEND_TIME => self.bend_ms = value,
            tp::CURVE => self.curve = value,
            tp::PHASE => self.phase = value,
            tp::NOISE => self.noise = value,
            tp::NOISE_DECAY => self.noise_ms = value,
            tp::COLOR => self.color_hz = value,
            tp::RESO => self.reso = value,
            tp::MODE => self.mode = value,
            tp::TRACK => self.track = value,
            tp::ATTACK => self.attack_ms = value,
            tp::DECAY => self.decay_ms = value,
            tp::SUSTAIN => self.sustain = value,
            tp::RELEASE => self.release_ms = value,
            tp::VEL => self.vel = value,
            tp::VEL_CLICK => self.vel_click = value,
            tp::LEVEL => self.level = value,
            tp::SAT_DRIVE => self.sat_drive = value,
            tp::SAT_SHAPE => self.sat_shape = value,
            tp::SAT_BIAS => self.sat_bias = value,
            tp::SAT_MIX => self.sat_mix = value,
            tp::COMP_THRESH => self.comp_thresh_db = value,
            tp::COMP_RATIO => self.comp_ratio = value,
            tp::COMP_ATTACK => self.comp_attack_ms = value,
            tp::COMP_RELEASE => self.comp_release_ms = value,
            tp::COMP_KNEE => self.comp_knee_db = value,
            tp::COMP_MAKEUP => self.comp_makeup_db = value,
            tp::COMP_MIX => self.comp_mix = value,
            tp::DISP_AMOUNT => self.disp_amount = value,
            tp::DISP_FOCUS => self.disp_focus = value,
            tp::DISP_WIDTH => self.disp_width = value,
            tp::DISP_FOLLOW => self.disp_follow = value,
            tp::DISP_MIX => self.disp_mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            tp::TUNE => self.tune,
            tp::CLICK => self.click,
            tp::CLICK_TIME => self.click_ms,
            tp::BEND => self.bend,
            tp::BEND_TIME => self.bend_ms,
            tp::CURVE => self.curve,
            tp::PHASE => self.phase,
            tp::NOISE => self.noise,
            tp::NOISE_DECAY => self.noise_ms,
            tp::COLOR => self.color_hz,
            tp::RESO => self.reso,
            tp::MODE => self.mode,
            tp::TRACK => self.track,
            tp::ATTACK => self.attack_ms,
            tp::DECAY => self.decay_ms,
            tp::SUSTAIN => self.sustain,
            tp::RELEASE => self.release_ms,
            tp::VEL => self.vel,
            tp::VEL_CLICK => self.vel_click,
            tp::LEVEL => self.level,
            tp::SAT_DRIVE => self.sat_drive,
            tp::SAT_SHAPE => self.sat_shape,
            tp::SAT_BIAS => self.sat_bias,
            tp::SAT_MIX => self.sat_mix,
            tp::COMP_THRESH => self.comp_thresh_db,
            tp::COMP_RATIO => self.comp_ratio,
            tp::COMP_ATTACK => self.comp_attack_ms,
            tp::COMP_RELEASE => self.comp_release_ms,
            tp::COMP_KNEE => self.comp_knee_db,
            tp::COMP_MAKEUP => self.comp_makeup_db,
            tp::COMP_MIX => self.comp_mix,
            tp::DISP_AMOUNT => self.disp_amount,
            tp::DISP_FOCUS => self.disp_focus,
            tp::DISP_WIDTH => self.disp_width,
            tp::DISP_FOLLOW => self.disp_follow,
            tp::DISP_MIX => self.disp_mix,
            _ => 0.0,
        }
    }

    /// The noise filter's mode as the MODE cell names it.
    pub fn noise_mode(&self) -> Mode {
        match self.mode.round() as u32 {
            0 => Mode::Lowpass,
            2 => Mode::Highpass,
            _ => Mode::BandpassUnity,
        }
    }

    /// The saturator's curve as the SHAPE cell names it.
    pub fn shape_mode(&self) -> ShapeMode {
        match self.sat_shape.round() as u32 {
            1 => ShapeMode::HardClip,
            2 => ShapeMode::Cubic,
            3 => ShapeMode::Fold,
            4 => ShapeMode::Crush,
            _ => ShapeMode::SoftClip,
        }
    }

    /// The disperser's stage count from AMOUNT.
    pub fn disp_stages(&self) -> u32 {
        (self.disp_amount * STAGES_MAX)
            .round()
            .clamp(0.0, STAGES_MAX) as u32
    }

    /// The tuned fundamental for a note, before the envelopes.
    pub fn fundamental_hz(&self, pitch: u8) -> f32 {
        BASE_HZ * (self.tune / 12.0).exp2() * pitch_scale(pitch)
    }

    /// The pitch offset in semitones at `ms` after the strike, with the
    /// click at `click_weight` of its reach: the sum of both envelopes.
    /// What the engine plays and the picture draws, one arithmetic.
    pub fn pitch_semitones(&self, ms: f32, click_weight: f32) -> f32 {
        self.click * click_weight * self.envelope_shape(ms, self.click_ms)
            + self.bend * self.envelope_shape(ms, self.bend_ms)
    }

    /// One pitch envelope's value at `ms`: the 60 dB exponential the
    /// kernel runs, blended by CURVE towards a straight fall over the
    /// same time.
    pub fn envelope_shape(&self, ms: f32, time_ms: f32) -> f32 {
        let time = time_ms.max(1e-3);
        let exp = (-DECADES * ms / time).exp();
        let lin = (1.0 - ms / time).max(0.0);
        exp * (1.0 - self.curve) + lin * self.curve
    }
}

/// A note's own pitch as a multiplier on the tuned fundamental. MIDI 36
/// is unity, as on every drum machine.
fn pitch_scale(pitch: u8) -> f32 {
    const ROOT: f32 = 36.0;
    ((f32::from(pitch) - ROOT) / 12.0).exp2()
}

/// The kick's single voice: a retrigger cuts its own tail.
pub struct ThumpVoice {
    sample_rate: f32,
    params: ThumpParams,
    /// The knobs as letters last set them: what a lock's restore returns to.
    base: ThumpParams,

    osc: MipOsc,
    sine_tables: Vec<f32>,
    noise: WhiteNoise,

    click_env: ExpDecay,
    bend_env: ExpDecay,
    burst_env: ExpDecay,
    amp: Adsr,

    noise_filter: Svf,
    shaper: Waveshaper,
    detector: RmsDetector,
    computer: GainComputer,
    ballistics: Ballistics,
    disperser: Disperser,

    note_scale: f32,
    velocity: f32,
    /// Samples since the strike, for the straight-fall half of CURVE.
    age: u64,

    tuned_noise_hz: f32,
    tuned_noise_q: f32,
    tuned_disp_hz: f32,
    tuned_disp_q: f32,
    tuned_disp_stages: u32,
    tuned_shape: Option<(ShapeMode, f32, f32, f32)>,

    chunk_left: usize,
    sig: [f32; CHUNK],
    env: [f32; CHUNK],
    burst: [f32; CHUNK],
    dry: [f32; CHUNK],
}

impl Default for ThumpVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl ThumpVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: ThumpParams::default(),
            base: ThumpParams::default(),
            osc: MipOsc::new(),
            sine_tables: Vec::new(),
            noise: WhiteNoise::new(),
            click_env: ExpDecay::new(),
            bend_env: ExpDecay::new(),
            burst_env: ExpDecay::new(),
            amp: Adsr::new(),
            noise_filter: Svf::new(),
            shaper: Waveshaper::new(),
            detector: RmsDetector::new(),
            computer: GainComputer::new(),
            ballistics: Ballistics::new(),
            disperser: Disperser::new(),
            note_scale: 1.0,
            velocity: 1.0,
            age: 0,
            tuned_noise_hz: 0.0,
            tuned_noise_q: 0.0,
            tuned_disp_hz: 0.0,
            tuned_disp_q: 0.0,
            tuned_disp_stages: u32::MAX,
            tuned_shape: None,
            chunk_left: 0,
            sig: [0.0; CHUNK],
            env: [0.0; CHUNK],
            burst: [0.0; CHUNK],
            dry: [0.0; CHUNK],
        }
    }

    /// Green zone: build the table and settle every kernel. The one
    /// allocation in this file happens here.
    pub fn prepare(&mut self, sample_rate: f32, params: ThumpParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.sine_tables.resize(osc::table_len(Waveform::Sine), 0.0);
        osc::build_tables(Waveform::Sine, &mut self.sine_tables);
        self.osc.prepare(self.sample_rate, Waveform::Sine);
        self.noise.seed(0x7_4085_5eed_0001);
        self.detector.prepare(self.sample_rate, RMS_WINDOW_MS);
        self.params = params;
        self.base = params;
        self.apply_envelopes();
        self.apply_dynamics();
        self.reset();
    }

    /// Envelope timings, rebuilt from the params.
    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        self.click_env.prepare(fs, self.params.click_ms);
        self.bend_env.prepare(fs, self.params.bend_ms);
        self.burst_env.prepare(fs, self.params.noise_ms);
        self.amp.prepare(
            fs,
            self.params.attack_ms,
            self.params.decay_ms,
            self.params.sustain,
            self.params.release_ms,
        );
    }

    /// The compressor's rule and ballistics.
    fn apply_dynamics(&mut self) {
        self.computer.configure(
            CompMode::Compress,
            self.params.comp_thresh_db,
            self.params.comp_ratio,
            self.params.comp_knee_db,
        );
        self.ballistics.prepare(
            self.sample_rate,
            self.params.comp_attack_ms,
            self.params.comp_release_ms,
        );
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.chunk_left = 0;
        self.age = 0;
        self.osc.reset();
        self.noise.reset();
        self.click_env = ExpDecay::new();
        self.bend_env = ExpDecay::new();
        self.burst_env = ExpDecay::new();
        self.amp.reset();
        self.noise_filter.reset();
        self.detector.reset();
        self.ballistics.reset();
        self.disperser.reset();
        self.tuned_noise_hz = 0.0;
        self.tuned_disp_hz = 0.0;
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
        self.params.set(param, value);
        match param {
            tp::CLICK_TIME
            | tp::BEND_TIME
            | tp::NOISE_DECAY
            | tp::ATTACK
            | tp::DECAY
            | tp::SUSTAIN
            | tp::RELEASE => self.apply_envelopes(),
            tp::COMP_THRESH
            | tp::COMP_RATIO
            | tp::COMP_ATTACK
            | tp::COMP_RELEASE
            | tp::COMP_KNEE => self.apply_dynamics(),
            _ => {}
        }
    }

    pub fn params(&self) -> ThumpParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.amp.active()
    }

    /// Strike. Every envelope restarts together; the sine starts at
    /// PHASE, so a quarter turn is a click of its own.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.note_scale = pitch_scale(pitch);
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        self.osc.reset();
        self.osc.set_phase(self.params.phase);
        self.chunk_left = 0;
        self.age = 0;
        self.click_env.trigger(1.0);
        self.bend_env.trigger(1.0);
        self.burst_env.trigger(1.0);
        self.amp.gate_on();
    }

    /// The gate goes down: the ADSR releases. A sustain of zero has
    /// already decayed, so this is only heard when SUSTAIN holds.
    pub fn release(&mut self) {
        self.amp.gate_off();
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

    /// How much of CLICK this strike gets: V.CLICK is velocity's say.
    fn click_weight(&self) -> f32 {
        (1.0 - self.params.vel_click) + self.params.vel_click * self.velocity
    }

    /// The start of a control chunk: the pitch, the noise filter, the
    /// disperser's corner, the shaper's rule.
    fn start_chunk(&mut self) {
        let fs = self.sample_rate;
        let ceiling = fs * 0.45;
        let p = self.params;
        let ms = self.age as f32 * 1_000.0 / fs;
        // The two envelopes, each the kernel's exponential blended by
        // CURVE towards a straight fall — read from the running kernels
        // for the exponential half so a lock on the time mid-note is
        // heard where the kernel is, not where the picture would be.
        let click = self.click_env.current() * (1.0 - p.curve)
            + (1.0 - ms / p.click_ms.max(1e-3)).max(0.0) * p.curve;
        let bend = self.bend_env.current() * (1.0 - p.curve)
            + (1.0 - ms / p.bend_ms.max(1e-3)).max(0.0) * p.curve;
        let semis = p.click * self.click_weight() * click + p.bend * bend;
        let tuned = BASE_HZ * (p.tune / 12.0).exp2() * self.note_scale;
        let now = (tuned * (semis / 12.0).exp2()).clamp(1.0, ceiling);
        self.osc.set_freq(now);

        // The noise filter: COLOR, moved by the tuned note at TRACK.
        let track = (tuned / BASE_HZ).powf(p.track);
        let hz = (p.color_hz * track).clamp(20.0, ceiling);
        if (hz - self.tuned_noise_hz).abs() > self.tuned_noise_hz.max(1.0) * 1e-4
            || (p.reso - self.tuned_noise_q).abs() > 1e-4
        {
            self.tuned_noise_hz = hz;
            self.tuned_noise_q = p.reso;
            self.noise_filter.prepare(fs, hz, p.reso);
        }

        // The disperser follows the fundamental, at FOLLOW of the
        // envelopes' movement, sitting on the FOCUS harmonic.
        let followed = tuned * (semis * p.disp_follow / 12.0).exp2();
        let corner = (followed * p.disp_focus).clamp(20.0, ceiling);
        let stages = p.disp_stages();
        if (corner - self.tuned_disp_hz).abs() > self.tuned_disp_hz.max(1.0) * 1e-4
            || (p.disp_width - self.tuned_disp_q).abs() > 1e-4
            || stages != self.tuned_disp_stages
        {
            self.tuned_disp_hz = corner;
            self.tuned_disp_q = p.disp_width;
            self.tuned_disp_stages = stages;
            self.disperser.prepare(fs, corner, p.disp_width, stages);
        }

        // The shaper is stateless; its rule is rebuilt when a knob moved.
        let drive =
            (DRIVE_MIN + (DRIVE_MAX - DRIVE_MIN) * p.sat_drive * p.sat_drive).min(DRIVE_MAX);
        let shape = (p.shape_mode(), drive, p.sat_bias, p.sat_mix);
        if self.tuned_shape != Some(shape) {
            self.tuned_shape = Some(shape);
            self.shaper.configure(shape.0, shape.1, shape.2, shape.3);
        }
    }

    /// One run of samples inside a control chunk: per-sample work only.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(sig), Some(env), Some(burst), Some(dry)) = (
            self.sig.get_mut(..n),
            self.env.get_mut(..n),
            self.burst.get_mut(..n),
            self.dry.get_mut(..n),
        ) else {
            return;
        };
        let p = self.params;

        // --- the pitch envelopes run at sample rate; the chunk reads them
        self.click_env.process(env);
        self.bend_env.process(env);

        // --- the sine and the burst, into `sig` ------------------------
        self.osc.process(sig, &self.sine_tables);
        if p.noise > 0.0 {
            self.noise.process(burst);
            self.noise_filter.process(burst, p.noise_mode());
            self.burst_env.process(env);
            for (sample, (b, e)) in sig.iter_mut().zip(burst.iter().zip(env.iter())) {
                *sample += *b * *e * p.noise;
            }
        }

        // --- the ADSR, with the strike's weight -----------------------
        self.amp.process(env);
        let weight = (1.0 - p.vel) + p.vel * self.velocity;
        for (sample, e) in sig.iter_mut().zip(env.iter()) {
            *sample *= *e * weight;
        }

        // --- the saturator ---------------------------------------------
        self.shaper.process(sig);

        // --- the compressor, feedforward, per sample ------------------
        if p.comp_ratio > 1.0 || p.comp_makeup_db > 0.0 {
            let makeup = db_to_gain(p.comp_makeup_db);
            let mix = p.comp_mix;
            for sample in sig.iter_mut() {
                let x = *sample;
                let level = self.detector.tick(x);
                let target = self.computer.gain_db(gain_to_db(level.max(1e-6)));
                let smoothed = self.ballistics.tick(target);
                let wet = x * db_to_gain(smoothed) * makeup;
                *sample = x + (wet - x) * mix;
            }
        }

        // --- the disperser, on the note's harmonic ---------------------
        if p.disp_stages() > 0 {
            dry.copy_from_slice(sig);
            self.disperser.process(sig);
            let mix = p.disp_mix;
            for (sample, d) in sig.iter_mut().zip(dry.iter()) {
                *sample = *d + (*sample - *d) * mix;
            }
        }

        let level = p.level * gain;
        for (o, sample) in out.iter_mut().zip(sig.iter()) {
            *o += *sample * level;
        }
        self.age = self.age.saturating_add(n as u64);
    }
}

// ------------------------------------------------------------ pictures ---

/// The hero picture for one of THUMP's sub-pages, with the selected
/// cell's contribution lit. Plain data: the view draws it, the core
/// never names egui, and the arithmetic is the engine's own.
pub fn hero(params: &ThumpParams, subpage: &str, selected: Option<u32>) -> Option<Hero> {
    match subpage {
        "Sine" => Some(sine_picture(params, selected)),
        "Noise" => Some(match selected {
            Some(tp::COLOR | tp::RESO | tp::MODE | tp::TRACK) => noise_response(params, selected),
            _ => noise_burst(params, selected),
        }),
        "Amp" => Some(amp_picture(params, selected)),
        "Sat" => Some(sat_picture(params, selected)),
        "Comp" => Some(comp_picture(params, selected)),
        "Disperse" => Some(disperse_picture(params, selected)),
        _ => None,
    }
}

/// How many points a curve is drawn with.
const POINTS: usize = 96;

fn lit(selected: Option<u32>, ids: &[u32]) -> bool {
    selected.is_some_and(|id| ids.contains(&id))
}

/// Pitch against time: the click and the bend each as its own strip,
/// the sum as the line the sine plays. Hz on a log axis.
fn sine_picture(p: &ThumpParams, selected: Option<u32>) -> Hero {
    let span_ms = (p.bend_ms * 1.25).max(p.click_ms * 4.0).max(60.0);
    let f0 = p.fundamental_hz(36);
    let top_st = (p.click + p.bend).max(1.0);
    let lo = f0 * 0.5;
    let hi = f0 * ((top_st + 3.0) / 12.0).exp2();
    let norm = |hz: f32| (hz.max(lo) / lo).ln() / (hi / lo).ln();
    let curve = |semis: fn(&ThumpParams, f32) -> f32| -> Vec<(f32, f32)> {
        (0..POINTS)
            .map(|i| {
                let x = i as f32 / (POINTS - 1) as f32;
                let ms = x * span_ms;
                let hz = f0 * (semis(p, ms) / 12.0).exp2();
                (x, norm(hz))
            })
            .collect()
    };
    let series = vec![
        HeroSeries {
            name: "click",
            points: curve(|p, ms| p.click * p.envelope_shape(ms, p.click_ms)),
            lit: lit(selected, &[tp::CLICK, tp::CLICK_TIME]),
        },
        HeroSeries {
            name: "bend",
            points: curve(|p, ms| p.bend * p.envelope_shape(ms, p.bend_ms)),
            lit: lit(selected, &[tp::BEND, tp::BEND_TIME]),
        },
        HeroSeries {
            name: "pitch",
            points: curve(|p, ms| p.pitch_semitones(ms, 1.0)),
            lit: lit(selected, &[tp::TUNE, tp::CURVE, tp::PHASE]) || selected.is_none(),
        },
    ];
    Hero {
        waveform: None,
        title: "PITCH · Hz over ms".to_owned(),
        series,
        marks: vec![
            HeroMark {
                x: (p.click_ms / span_ms).min(1.0),
                label: format!("{:.0} ms", p.click_ms),
                lit: lit(selected, &[tp::CLICK_TIME]),
            },
            HeroMark {
                x: (p.bend_ms / span_ms).min(1.0),
                label: format!("{:.0} ms", p.bend_ms),
                lit: lit(selected, &[tp::BEND_TIME]),
            },
        ],
        x_labels: ["0".to_owned(), format!("{span_ms:.0} ms")],
        y_labels: [format!("{lo:.0} Hz"), format!("{hi:.0} Hz")],
        diagonal: false,
    }
}

/// The burst: the noise's envelope against time.
fn noise_burst(p: &ThumpParams, selected: Option<u32>) -> Hero {
    let span_ms = (p.noise_ms * 3.0).max(10.0);
    let points = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let ms = x * span_ms;
            let e = (-DECADES * ms / p.noise_ms.max(1e-3)).exp();
            (x, e * p.noise.max(0.02))
        })
        .collect();
    Hero {
        waveform: None,
        title: "BURST · level over ms".to_owned(),
        series: vec![HeroSeries {
            name: "burst",
            points,
            lit: true,
        }],
        marks: vec![HeroMark {
            x: (p.noise_ms / span_ms).min(1.0),
            label: format!("{:.0} ms", p.noise_ms),
            lit: lit(selected, &[tp::NOISE_DECAY]),
        }],
        x_labels: ["0".to_owned(), format!("{span_ms:.0} ms")],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// The noise filter's magnitude response on a log axis, COLOR marked.
fn noise_response(p: &ThumpParams, selected: Option<u32>) -> Hero {
    const LO: f32 = 20.0;
    const HI: f32 = 20_000.0;
    const DB_LO: f32 = -36.0;
    const DB_HI: f32 = 18.0;
    let q = p.reso.max(0.05);
    let mode = p.noise_mode();
    let points = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let hz = LO * (HI / LO).powf(x);
            let db = svf_db(hz, p.color_hz, q, mode);
            (x, ((db - DB_LO) / (DB_HI - DB_LO)).clamp(0.0, 1.0))
        })
        .collect();
    Hero {
        waveform: None,
        title: "COLOR · dB over Hz".to_owned(),
        series: vec![HeroSeries {
            name: "response",
            points,
            lit: true,
        }],
        marks: vec![HeroMark {
            x: (p.color_hz / LO).ln() / (HI / LO).ln(),
            label: format!("{:.0} Hz", p.color_hz),
            lit: lit(selected, &[tp::COLOR, tp::TRACK]),
        }],
        x_labels: ["20 Hz".to_owned(), "20 kHz".to_owned()],
        y_labels: [format!("{DB_LO:.0} dB"), format!("+{DB_HI:.0} dB")],
        diagonal: false,
    }
}

/// The analogue prototype's magnitude for the SVF's three modes.
fn svf_db(hz: f32, cutoff: f32, q: f32, mode: Mode) -> f32 {
    let x = hz / cutoff.max(1.0);
    let re = 1.0 - x * x;
    let im = x / q;
    let denom = (re * re + im * im).sqrt().max(1e-9);
    let mag = match mode {
        Mode::Lowpass => 1.0 / denom,
        Mode::Highpass => x * x / denom,
        _ => im / denom,
    };
    20.0 * mag.max(1e-9).log10()
}

/// The ADSR: rise, fall to the plateau, the plateau, the release.
fn amp_picture(p: &ThumpParams, selected: Option<u32>) -> Hero {
    let hold_ms = (p.attack_ms + p.decay_ms + p.release_ms).max(10.0) * 0.25;
    let total = p.attack_ms + p.decay_ms + hold_ms + p.release_ms;
    let at = |ms: f32| ms / total;
    let a_end = p.attack_ms;
    let d_end = a_end + p.decay_ms;
    let s_end = d_end + hold_ms;
    let segment =
        |name: &'static str, from: f32, to: f32, y0: f32, y1: f32, ids: &[u32]| HeroSeries {
            name,
            points: (0..24)
                .map(|i| {
                    let t = i as f32 / 23.0;
                    (at(from + (to - from) * t), y0 + (y1 - y0) * t)
                })
                .collect(),
            // A segment with no width cannot be lit; the view then
            // lights the whole envelope.
            lit: lit(selected, ids) && to > from,
        };
    Hero {
        waveform: None,
        title: "AMP · level over ms".to_owned(),
        series: vec![
            segment("attack", 0.0, a_end, 0.0, 1.0, &[tp::ATTACK]),
            segment("decay", a_end, d_end, 1.0, p.sustain, &[tp::DECAY]),
            segment(
                "sustain",
                d_end,
                s_end,
                p.sustain,
                p.sustain,
                &[tp::SUSTAIN],
            ),
            segment("release", s_end, total, p.sustain, 0.0, &[tp::RELEASE]),
        ],
        marks: Vec::new(),
        x_labels: ["0".to_owned(), format!("{total:.0} ms")],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// The saturator's transfer curve, in against out, unity as a diagonal.
fn sat_picture(p: &ThumpParams, selected: Option<u32>) -> Hero {
    let mut shaper = Waveshaper::new();
    let drive = (DRIVE_MIN + (DRIVE_MAX - DRIVE_MIN) * p.sat_drive * p.sat_drive).min(DRIVE_MAX);
    shaper.configure(p.shape_mode(), drive, p.sat_bias, p.sat_mix);
    let points = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let input = x * 2.0 - 1.0;
            let output = shaper.shape(input).clamp(-1.0, 1.0);
            (x, output * 0.5 + 0.5)
        })
        .collect();
    Hero {
        waveform: None,
        title: "SAT · out over in".to_owned(),
        series: vec![HeroSeries {
            name: "transfer",
            points,
            lit: lit(
                selected,
                &[tp::SAT_DRIVE, tp::SAT_SHAPE, tp::SAT_BIAS, tp::SAT_MIX],
            ) || selected.is_none(),
        }],
        marks: Vec::new(),
        x_labels: ["-1".to_owned(), "+1".to_owned()],
        y_labels: ["-1".to_owned(), "+1".to_owned()],
        diagonal: true,
    }
}

/// The compressor's gain curve in dB, threshold marked.
fn comp_picture(p: &ThumpParams, selected: Option<u32>) -> Hero {
    const LO: f32 = -60.0;
    let mut computer = GainComputer::new();
    computer.configure(
        CompMode::Compress,
        p.comp_thresh_db,
        p.comp_ratio,
        p.comp_knee_db,
    );
    let points = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let input = LO + x * -LO;
            let output = input + computer.gain_db(input) + p.comp_makeup_db;
            (x, ((output - LO) / -LO).clamp(0.0, 1.0))
        })
        .collect();
    Hero {
        waveform: None,
        title: "COMP · out dB over in dB".to_owned(),
        series: vec![HeroSeries {
            name: "gain",
            points,
            lit: lit(
                selected,
                &[
                    tp::COMP_THRESH,
                    tp::COMP_RATIO,
                    tp::COMP_KNEE,
                    tp::COMP_MAKEUP,
                    tp::COMP_MIX,
                    tp::COMP_ATTACK,
                    tp::COMP_RELEASE,
                ],
            ) || selected.is_none(),
        }],
        marks: vec![HeroMark {
            x: (p.comp_thresh_db - LO) / -LO,
            label: format!("{:.0} dB", p.comp_thresh_db),
            lit: lit(selected, &[tp::COMP_THRESH, tp::COMP_KNEE]),
        }],
        x_labels: [format!("{LO:.0} dB"), "0 dB".to_owned()],
        y_labels: [format!("{LO:.0} dB"), "0 dB".to_owned()],
        diagonal: true,
    }
}

/// The disperser's group delay against frequency on a log axis, with
/// the harmonic ladder of the tuned note marked and FOCUS lit.
fn disperse_picture(p: &ThumpParams, selected: Option<u32>) -> Hero {
    const LO: f32 = 20.0;
    const HI: f32 = 20_000.0;
    let f0 = p.fundamental_hz(36);
    let corner = (f0 * p.disp_focus).clamp(LO, HI);
    let q = p.disp_width.max(0.05);
    let stages = p.disp_stages() as f32;
    let delay = |hz: f32| allpass_group_delay(hz, corner, q) * stages;
    let peak = delay(corner).max(1e-9);
    let points = (0..POINTS)
        .map(|i| {
            let x = i as f32 / (POINTS - 1) as f32;
            let hz = LO * (HI / LO).powf(x);
            (x, (delay(hz) / peak).clamp(0.0, 1.0))
        })
        .collect();
    let marks = (1..=8)
        .map(|h| {
            let hz = (f0 * h as f32).clamp(LO, HI);
            HeroMark {
                x: (hz / LO).ln() / (HI / LO).ln(),
                label: if h == 1 {
                    format!("{hz:.0} Hz")
                } else {
                    format!("×{h}")
                },
                lit: (h as f32 - p.disp_focus).abs() < 0.5
                    && lit(selected, &[tp::DISP_FOCUS, tp::DISP_FOLLOW]),
            }
        })
        .collect();
    Hero {
        waveform: None,
        title: "DISPERSE · delay over Hz".to_owned(),
        series: vec![HeroSeries {
            name: "delay",
            points,
            lit: lit(
                selected,
                &[
                    tp::DISP_AMOUNT,
                    tp::DISP_WIDTH,
                    tp::DISP_MIX,
                    tp::DISP_FOCUS,
                    tp::DISP_FOLLOW,
                ],
            ) || selected.is_none(),
        }],
        marks,
        x_labels: ["20 Hz".to_owned(), "20 kHz".to_owned()],
        y_labels: ["0".to_owned(), format!("{} st", p.disp_stages())],
        diagonal: false,
    }
}

/// A second-order allpass's group delay in seconds at `hz`, for a
/// corner `wc` and quality `q`: `2·(wc/Q)·(w² + wc²) / ((wc² − w²)² + (w·wc/Q)²)`.
fn allpass_group_delay(hz: f32, corner: f32, q: f32) -> f32 {
    let w = hz * core::f32::consts::TAU;
    let wc = corner * core::f32::consts::TAU;
    let bw = wc / q;
    let num = 2.0 * bw * (w * w + wc * wc);
    let den = (wc * wc - w * w).powi(2) + (w * bw).powi(2);
    num / den.max(1e-12)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn voice(params: ThumpParams) -> ThumpVoice {
        let mut v = ThumpVoice::new();
        v.prepare(48_000.0, params);
        v
    }

    fn render(v: &mut ThumpVoice, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Zero crossings per second over a window: the pitch, roughly.
    fn rate_hz(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count();
        crossings as f32 / 2.0 * 48_000.0 / samples.len() as f32
    }

    #[test]
    fn a_fresh_kick_is_silent_until_it_is_struck() {
        let mut v = voice(ThumpParams::default());
        assert_eq!(peak(&render(&mut v, 4_800)), 0.0);
        v.trigger(36, 100);
        assert!(peak(&render(&mut v, 4_800)) > 0.05);
    }

    /// CLICK lifts the first milliseconds' pitch; BEND lifts the pitch
    /// later on; neither moves the tail.
    #[test]
    fn the_click_is_early_and_the_bend_is_later() {
        let quiet = ThumpParams {
            click: 0.0,
            bend: 0.0,
            noise: 0.0,
            disp_amount: 0.0,
            decay_ms: 3_000.0,
            sustain: 1.0,
            ..ThumpParams::default()
        };
        let clicked = ThumpParams {
            click: 48.0,
            click_ms: 10.0,
            ..quiet
        };
        let bent = ThumpParams {
            bend: 24.0,
            bend_ms: 400.0,
            ..quiet
        };
        let early = |p: ThumpParams| {
            let mut v = voice(p);
            v.trigger(36, 100);
            let out = render(&mut v, 480);
            rate_hz(&out)
        };
        let late = |p: ThumpParams| {
            let mut v = voice(p);
            v.trigger(36, 100);
            let _ = render(&mut v, 4_800);
            rate_hz(&render(&mut v, 4_800))
        };
        assert!(
            early(clicked) > early(quiet) * 2.0,
            "the click did not whip"
        );
        assert!(
            (late(clicked) - late(quiet)).abs() < 5.0,
            "the click lasted: {} vs {}",
            late(clicked),
            late(quiet)
        );
        assert!(
            early(bent) > early(quiet) * 1.3,
            "the bend did not lift the body"
        );
        // The tail returns to the tuned note.
        let mut v = voice(bent);
        v.trigger(36, 100);
        let _ = render(&mut v, 96_000);
        let tail = rate_hz(&render(&mut v, 9_600));
        assert!((tail - BASE_HZ).abs() < 4.0, "the tail sits at {tail} Hz");
    }

    #[test]
    fn the_burst_is_gone_by_four_times_its_decay() {
        let params = ThumpParams {
            noise: 1.0,
            noise_ms: 20.0,
            click: 0.0,
            bend: 0.0,
            sustain: 1.0,
            decay_ms: 3_000.0,
            ..ThumpParams::default()
        };
        let mut with = voice(params);
        with.trigger(36, 100);
        let mut without = voice(ThumpParams {
            noise: 0.0,
            ..params
        });
        without.trigger(36, 100);
        let a = render(&mut with, 480);
        let b = render(&mut without, 480);
        assert!(a != b, "the burst was not heard");
        let _ = render(&mut with, 4_800);
        let _ = render(&mut without, 4_800);
        let a = render(&mut with, 480);
        let b = render(&mut without, 480);
        let diff = a
            .iter()
            .zip(&b)
            .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(diff < 0.02, "the burst is still going: {diff}");
    }

    #[test]
    fn a_retrigger_replaces_the_hit_rather_than_layering_on_it() {
        let params = ThumpParams {
            decay_ms: 2_000.0,
            ..ThumpParams::default()
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
    fn the_gate_releases_a_held_sustain() {
        let params = ThumpParams {
            sustain: 1.0,
            release_ms: 50.0,
            ..ThumpParams::default()
        };
        let mut v = voice(params);
        v.trigger(36, 100);
        let _ = render(&mut v, 24_000);
        assert!(
            peak(&render(&mut v, 480)) > 0.05,
            "the sustain did not hold"
        );
        v.release();
        let _ = render(&mut v, 9_600);
        assert_eq!(peak(&render(&mut v, 480)), 0.0, "the release did not end");
        assert!(!v.active());
    }

    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = ThumpParams {
            noise: 0.6,
            disp_amount: 0.5,
            comp_ratio: 8.0,
            sat_drive: 0.7,
            ..ThumpParams::default()
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
            "512 must equal 100 + 257 + 155; first difference at {first:?}"
        );
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(ThumpParams {
            noise: 1.0,
            disp_amount: 1.0,
            comp_ratio: 20.0,
            sat_drive: 1.0,
            ..ThumpParams::default()
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
        for def in tp::TABLE {
            for extreme in [def.min, def.max] {
                let mut params = ThumpParams {
                    noise: 1.0,
                    disp_amount: 1.0,
                    comp_ratio: 20.0,
                    sat_drive: 1.0,
                    ..ThumpParams::default()
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
                v.trigger(96, 127);
                let out = render(&mut v, 4_096);
                assert!(out.iter().all(|s| s.is_finite()));
            }
        }
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = ThumpParams::default();
        for def in tp::TABLE {
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
        params.set(tp::TUNE, 1e9);
        assert!((params.tune - tp::TUNE_MAX).abs() < 1e-3);
    }

    /// A hot hit through a hard ratio comes out smaller than the same
    /// hit with the compressor idle.
    #[test]
    fn the_compressor_holds_a_hot_hit_down() {
        let loud = ThumpParams {
            sat_drive: 0.0,
            disp_amount: 0.0,
            comp_ratio: 1.0,
            ..ThumpParams::default()
        };
        let mut idle = voice(loud);
        idle.trigger(36, 127);
        let _ = render(&mut idle, 240);
        let open = peak(&render(&mut idle, 2_400));
        let mut held = voice(ThumpParams {
            comp_thresh_db: -30.0,
            comp_ratio: 20.0,
            comp_attack_ms: 0.1,
            ..loud
        });
        held.trigger(36, 127);
        let _ = render(&mut held, 240);
        let squashed = peak(&render(&mut held, 2_400));
        assert!(squashed < open * 0.7, "open {open} squashed {squashed}");
    }

    /// FOCUS moves where the smear sits: the disperser's output differs
    /// between two harmonics, and with no stages it is a wire.
    #[test]
    fn focus_moves_the_disperser_and_no_stages_is_a_wire() {
        let base = ThumpParams {
            disp_amount: 0.0,
            ..ThumpParams::default()
        };
        let mut dry = voice(base);
        dry.trigger(36, 100);
        let dry_out = render(&mut dry, 4_800);
        let mut also_dry = voice(ThumpParams {
            disp_focus: 8.0,
            ..base
        });
        also_dry.trigger(36, 100);
        assert_eq!(
            dry_out,
            render(&mut also_dry, 4_800),
            "zero stages is not a wire"
        );
        let mut low = voice(ThumpParams {
            disp_amount: 0.5,
            disp_focus: 1.0,
            ..base
        });
        low.trigger(36, 100);
        let mut high = voice(ThumpParams {
            disp_amount: 0.5,
            disp_focus: 8.0,
            ..base
        });
        high.trigger(36, 100);
        let low_out = render(&mut low, 4_800);
        let high_out = render(&mut high, 4_800);
        assert_ne!(low_out, dry_out, "the disperser did nothing");
        assert_ne!(low_out, high_out, "FOCUS did not move the corner");
    }

    #[test]
    fn a_lock_is_heard_on_its_hit_and_the_knob_comes_back() {
        let mut v = voice(ThumpParams::default());
        v.set_param(tp::BEND, 20.0);
        v.plock(tp::BEND, Some(5.0));
        assert_eq!(v.params().get(tp::BEND), 5.0);
        v.plock(tp::BEND, None);
        assert_eq!(v.params().get(tp::BEND), 20.0);
    }

    #[test]
    fn every_subpage_has_a_picture_and_the_selected_cell_lights_it() {
        let p = ThumpParams::default();
        for (page, id) in [
            ("Sine", tp::CLICK),
            ("Noise", tp::NOISE_DECAY),
            ("Noise", tp::COLOR),
            ("Amp", tp::DECAY),
            ("Sat", tp::SAT_DRIVE),
            ("Comp", tp::COMP_THRESH),
            ("Disperse", tp::DISP_WIDTH),
        ] {
            let hero = hero(&p, page, Some(id)).unwrap_or_else(|| panic!("{page} has no picture"));
            assert!(!hero.series.is_empty());
            assert!(
                hero.series.iter().any(|s| s.lit),
                "{page}: nothing lit for {id}"
            );
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
        // The pitch picture and the engine agree: the same envelope shape.
        assert!((p.envelope_shape(0.0, 10.0) - 1.0).abs() < 1e-6);
        assert!(p.envelope_shape(10.0, 10.0) < 0.002);
    }

    #[test]
    fn the_drum_machine_c_is_the_tuned_note() {
        assert!((pitch_scale(36) - 1.0).abs() < 1e-6);
        assert!((pitch_scale(48) - 2.0).abs() < 1e-5);
    }
}
