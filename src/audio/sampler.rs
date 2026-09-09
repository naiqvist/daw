//! The sampler's voice bank — the instrument half of `Node::Sampler`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when. Design, reasoning and the parameter table:
//! `notes/20260827-sampler-brief.md`.
//!
//! # The path, per voice, in fixed order
//!
//! ```text
//! material ─▶ READ HEAD ─▶ fades ─▶ AMP ENV ─▶ FILTER ─▶ DRIVE ─▶ DOWNSAMPLER ─┐
//!            (hermite4,   (declick   (adsr)    (svf,     (soft    (rate hold + │
//!             loop,        + loop               keytrack) clip,     quantise)   │
//!             slice,       xfade)               + mod)    2x)                   │
//!             reverse)                                                          ▼
//!                                                     8 voices ──sum──▶ pan ──▶ out
//! ```
//!
//! The pre-amp is NOT here. It is the machine's output stage, it is one
//! per device rather than one per voice, and it lives in
//! [`crate::audio::preamp`] where the node applies it after the sum.
//!
//! # Why the downsampler is per voice
//!
//! Its hold clock runs at a fixed rate in HERTZ, not relative to the
//! note. A sample played an octave up runs twice as fast past a clock
//! that did not move, so its aliasing products land somewhere new. That
//! relationship — pitch against a stationary converter — is the whole of
//! the sound this borrows from. Post-sum, every note would alias
//! identically and it would be a lowpass with hiss.
//!
//! # Scalar voices, and why this file breaks the lane-major rule
//!
//! `AGENTS.md` says voices are lane-major SoA. The sampler is the
//! deliberate exception, argued in the brief: a sampler voice's inner
//! loop is a GATHER — eight voices reading eight unrelated addresses at
//! eight different fractional positions — and that does not vectorise on
//! any axis the SoA layout offers. You would write the gather by hand
//! and then interpolate scalar anyway, having paid for the shuffle first.
//!
//! # Two rates, on purpose
//!
//! Per SAMPLE, where modulation is a multiply: the amp envelope, the
//! fades, the pan gains. Per control CHUNK, where it is a coefficient
//! that costs a transcendental to rebuild: the read increment, the filter
//! cutoff, the converter clock. The same split `kick.rs` and `poly.rs`
//! make — and, as there, [`Voice::chunk_left`] is anchored to the VOICE's
//! own timeline rather than to the render call, so a segment boundary
//! cannot move where the chunks fall.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::graph::Ramp;
use crate::audio::material::Material;
use crate::dsp::adsr::Adsr;
use crate::dsp::filters::{Mode as FilterMode, Svf};
use crate::dsp::interp;
use crate::dsp::lofi::Downsampler;
use crate::dsp::ramps::Fade;
use crate::dsp::shaper::{Mode as ShapeMode, Oversampler2x, Waveshaper};
use crate::params::sampler as sp;
mod advanced;
mod picture;
use crate::dsp::sample_read::SampleReader;
pub use picture::hero;

/// Voices. Eight is what a chopped break needs and twice what a bassline
/// does; the cost of one is a gather and two filters.
pub const VOICES: usize = 16;

/// Control-chunk length, in samples. 32 at 48 kHz is 0.67 ms — the same
/// number `kick.rs` and `poly.rs` use, for the same reason.
pub const CHUNK: usize = 32;

/// The most slices a file can be cut into. Sixty-four notes from C1 is
/// five octaves, which is where a keyboard runs out.
pub const MAX_SLICES: usize = 64;

/// How much of the drive knob reaches the shaper's own range. The
/// waveshaper's drive is `1..32`; a sampler wants the bottom of that,
/// because past a handful the soft clip stops being a colour.
const DRIVE_SPAN: f32 = 7.0;

/// The bias the drive stage adds at full drive. Breaking the odd symmetry
/// is where even harmonics come from, and a sampler's distortion should
/// be a little bit dirty rather than perfectly symmetric.
const DRIVE_BIAS: f32 = 0.08;

// -------------------------------------------------------------- params ---

/// A sampler's settings, in engine units. Parallel to
/// [`params::sampler::TABLE`](crate::params::sampler::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SamplerParams {
    pub mode: f32,
    pub start: f32,
    pub end: f32,
    pub reverse: f32,
    pub fade_in_ms: f32,
    pub fade_out_ms: f32,
    pub root: f32,
    pub tune: f32,
    pub fine: f32,
    pub loop_mode: f32,
    pub loop_start: f32,
    pub loop_xfade_ms: f32,
    pub slices: f32,
    pub slice_source: f32,
    pub choke: f32,
    pub amp_attack_ms: f32,
    pub amp_decay_ms: f32,
    pub amp_sustain: f32,
    pub amp_release_ms: f32,
    pub filter_mode: f32,
    pub cutoff_hz: f32,
    pub res: f32,
    pub keytrack: f32,
    pub mod_attack_ms: f32,
    pub mod_decay_ms: f32,
    pub mod_sustain: f32,
    pub mod_release_ms: f32,
    pub mod_dest: f32,
    pub mod_depth: f32,
    pub velocity: f32,
    pub drive: f32,
    pub rate_hz: f32,
    pub bits: f32,
    pub preamp: f32,
    pub gain_db: f32,
    pub pan: f32,
    /// The slice a note plays in slice mode, from one. Locked per trig
    /// like any other row; the note itself is a pitch against the root.
    pub slice: f32,
    pub playback: f32,
    pub time: f32,
    pub speed: f32,
    pub window_ms: f32,
    pub transient: f32,
    pub loop_size: f32,
    pub loop_fade: f32,
    pub scan: f32,
    pub travel: f32,
    pub motion_mode: f32,
    pub loop_units: f32,
    pub loop_exit: f32,
    pub play_mode: f32,
    pub voice_count: f32,
    pub glide_ms: f32,
    pub spread: f32,
    pub env_pitch: f32,
    pub env_position: f32,
    pub env_size: f32,
    pub env_filter: f32,
    pub start_jitter: f32,
    pub pitch_jitter: f32,
    pub seed: f32,
    pub slip: f32,
    pub attack_shape: f32,
    pub filter_slope: f32,
    pub comb_focus: f32,
    pub comb_feed: f32,
    pub comb_damp: f32,
    pub comb_mix: f32,
    pub source_beats: f32,
    pub fit_beats: f32,
    pub slice_thru: f32,
    pub hard: f32,
    pub sense: f32,
    pub min_gap_ms: f32,
}

impl Default for SamplerParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new sampler sounds
    /// like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(sp::TABLE, id).default;
        Self {
            mode: at(sp::MODE),
            start: at(sp::START),
            end: at(sp::END),
            reverse: at(sp::REVERSE),
            fade_in_ms: at(sp::FADE_IN),
            fade_out_ms: at(sp::FADE_OUT),
            root: at(sp::ROOT),
            tune: at(sp::TUNE),
            fine: at(sp::FINE),
            loop_mode: at(sp::LOOP_MODE),
            loop_start: at(sp::LOOP_START),
            loop_xfade_ms: at(sp::LOOP_XFADE),
            slices: at(sp::SLICES),
            slice_source: at(sp::SLICE_SOURCE),
            choke: at(sp::CHOKE),
            amp_attack_ms: at(sp::AMP_A),
            amp_decay_ms: at(sp::AMP_D),
            amp_sustain: at(sp::AMP_S),
            amp_release_ms: at(sp::AMP_R),
            filter_mode: at(sp::FILT_MODE),
            cutoff_hz: at(sp::CUTOFF),
            res: at(sp::RES),
            keytrack: at(sp::KEYTRACK),
            mod_attack_ms: at(sp::MOD_A),
            mod_decay_ms: at(sp::MOD_D),
            mod_sustain: at(sp::MOD_S),
            mod_release_ms: at(sp::MOD_R),
            mod_dest: at(sp::MOD_DEST),
            mod_depth: at(sp::MOD_DEPTH),
            velocity: at(sp::VELOCITY),
            drive: at(sp::DRIVE),
            rate_hz: at(sp::RATE),
            bits: at(sp::BITS),
            preamp: at(sp::PREAMP),
            gain_db: at(sp::GAIN),
            pan: at(sp::PAN),
            slice: at(sp::SLICE),
            playback: at(sp::PLAYBACK),
            time: at(sp::TIME),
            speed: at(sp::SPEED),
            window_ms: at(sp::WINDOW),
            transient: at(sp::TRANSIENT),
            loop_size: at(sp::LOOP_SIZE),
            loop_fade: at(sp::LOOP_FADE),
            scan: at(sp::SCAN),
            travel: at(sp::TRAVEL),
            motion_mode: at(sp::MOTION_MODE),
            loop_units: at(sp::LOOP_UNITS),
            loop_exit: at(sp::LOOP_EXIT),
            play_mode: at(sp::PLAY_MODE),
            voice_count: at(sp::VOICE_COUNT),
            glide_ms: at(sp::GLIDE),
            spread: at(sp::SPREAD),
            env_pitch: at(sp::ENV_PITCH),
            env_position: at(sp::ENV_POSITION),
            env_size: at(sp::ENV_SIZE),
            env_filter: at(sp::ENV_FILTER),
            start_jitter: at(sp::START_JITTER),
            pitch_jitter: at(sp::PITCH_JITTER),
            seed: at(sp::SEED),
            slip: at(sp::SLIP),
            attack_shape: at(sp::ATTACK_SHAPE),
            filter_slope: at(sp::FILTER_SLOPE),
            comb_focus: at(sp::COMB_FOCUS),
            comb_feed: at(sp::COMB_FEED),
            comb_damp: at(sp::COMB_DAMP),
            comb_mix: at(sp::COMB_MIX),
            source_beats: at(sp::SOURCE_BEATS),
            fit_beats: at(sp::FIT_BEATS),
            slice_thru: at(sp::SLICE_THRU),
            hard: at(sp::HARD),
            sense: at(sp::SENSE),
            min_gap_ms: at(sp::MIN_GAP),
        }
    }
}

impl SamplerParams {
    /// Write one row by id. Unknown ids are dropped, which is what a
    /// stale or misrouted letter deserves.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(sp::TABLE, param, value) else {
            return;
        };
        match param {
            sp::MODE => self.mode = value,
            sp::START => self.start = value,
            sp::END => self.end = value,
            sp::REVERSE => self.reverse = value,
            sp::FADE_IN => self.fade_in_ms = value,
            sp::FADE_OUT => self.fade_out_ms = value,
            sp::ROOT => self.root = value,
            sp::TUNE => self.tune = value,
            sp::FINE => self.fine = value,
            sp::LOOP_MODE => self.loop_mode = value,
            sp::LOOP_START => self.loop_start = value,
            sp::LOOP_XFADE => self.loop_xfade_ms = value,
            sp::SLICES => self.slices = value,
            sp::SLICE_SOURCE => self.slice_source = value,
            sp::CHOKE => self.choke = value,
            sp::AMP_A => self.amp_attack_ms = value,
            sp::AMP_D => self.amp_decay_ms = value,
            sp::AMP_S => self.amp_sustain = value,
            sp::AMP_R => self.amp_release_ms = value,
            sp::FILT_MODE => self.filter_mode = value,
            sp::CUTOFF => self.cutoff_hz = value,
            sp::RES => self.res = value,
            sp::KEYTRACK => self.keytrack = value,
            sp::MOD_A => self.mod_attack_ms = value,
            sp::MOD_D => self.mod_decay_ms = value,
            sp::MOD_S => self.mod_sustain = value,
            sp::MOD_R => self.mod_release_ms = value,
            sp::MOD_DEST => self.mod_dest = value,
            sp::MOD_DEPTH => self.mod_depth = value,
            sp::VELOCITY => self.velocity = value,
            sp::DRIVE => self.drive = value,
            sp::RATE => self.rate_hz = value,
            sp::BITS => self.bits = value,
            sp::PREAMP => self.preamp = value,
            sp::GAIN => self.gain_db = value,
            sp::PAN => self.pan = value,
            sp::SLICE => self.slice = value,
            sp::PLAYBACK => self.playback = value,
            sp::TIME => self.time = value,
            sp::SPEED => self.speed = value,
            sp::WINDOW => self.window_ms = value,
            sp::TRANSIENT => self.transient = value,
            sp::LOOP_SIZE => self.loop_size = value,
            sp::LOOP_FADE => self.loop_fade = value,
            sp::SCAN => self.scan = value,
            sp::TRAVEL => self.travel = value,
            sp::MOTION_MODE => self.motion_mode = value,
            sp::LOOP_UNITS => self.loop_units = value,
            sp::LOOP_EXIT => self.loop_exit = value,
            sp::PLAY_MODE => self.play_mode = value,
            sp::VOICE_COUNT => self.voice_count = value,
            sp::GLIDE => self.glide_ms = value,
            sp::SPREAD => self.spread = value,
            sp::ENV_PITCH => self.env_pitch = value,
            sp::ENV_POSITION => self.env_position = value,
            sp::ENV_SIZE => self.env_size = value,
            sp::ENV_FILTER => self.env_filter = value,
            sp::START_JITTER => self.start_jitter = value,
            sp::PITCH_JITTER => self.pitch_jitter = value,
            sp::SEED => self.seed = value,
            sp::SLIP => self.slip = value,
            sp::ATTACK_SHAPE => self.attack_shape = value,
            sp::FILTER_SLOPE => self.filter_slope = value,
            sp::COMB_FOCUS => self.comb_focus = value,
            sp::COMB_FEED => self.comb_feed = value,
            sp::COMB_DAMP => self.comb_damp = value,
            sp::COMB_MIX => self.comb_mix = value,
            sp::SOURCE_BEATS => self.source_beats = value,
            sp::FIT_BEATS => self.fit_beats = value,
            sp::SLICE_THRU => self.slice_thru = value,
            sp::HARD => self.hard = value,
            sp::SENSE => self.sense = value,
            sp::MIN_GAP => self.min_gap_ms = value,

            _ => {}
        }
    }

    /// Read one row by id. `None` for an id the table does not know.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            sp::MODE => self.mode,
            sp::START => self.start,
            sp::END => self.end,
            sp::REVERSE => self.reverse,
            sp::FADE_IN => self.fade_in_ms,
            sp::FADE_OUT => self.fade_out_ms,
            sp::ROOT => self.root,
            sp::TUNE => self.tune,
            sp::FINE => self.fine,
            sp::LOOP_MODE => self.loop_mode,
            sp::LOOP_START => self.loop_start,
            sp::LOOP_XFADE => self.loop_xfade_ms,
            sp::SLICES => self.slices,
            sp::SLICE_SOURCE => self.slice_source,
            sp::CHOKE => self.choke,
            sp::AMP_A => self.amp_attack_ms,
            sp::AMP_D => self.amp_decay_ms,
            sp::AMP_S => self.amp_sustain,
            sp::AMP_R => self.amp_release_ms,
            sp::FILT_MODE => self.filter_mode,
            sp::CUTOFF => self.cutoff_hz,
            sp::RES => self.res,
            sp::KEYTRACK => self.keytrack,
            sp::MOD_A => self.mod_attack_ms,
            sp::MOD_D => self.mod_decay_ms,
            sp::MOD_S => self.mod_sustain,
            sp::MOD_R => self.mod_release_ms,
            sp::MOD_DEST => self.mod_dest,
            sp::MOD_DEPTH => self.mod_depth,
            sp::VELOCITY => self.velocity,
            sp::DRIVE => self.drive,
            sp::RATE => self.rate_hz,
            sp::BITS => self.bits,
            sp::PREAMP => self.preamp,
            sp::GAIN => self.gain_db,
            sp::PAN => self.pan,
            sp::SLICE => self.slice,
            sp::PLAYBACK => self.playback,
            sp::TIME => self.time,
            sp::SPEED => self.speed,
            sp::WINDOW => self.window_ms,
            sp::TRANSIENT => self.transient,
            sp::LOOP_SIZE => self.loop_size,
            sp::LOOP_FADE => self.loop_fade,
            sp::SCAN => self.scan,
            sp::TRAVEL => self.travel,
            sp::MOTION_MODE => self.motion_mode,
            sp::LOOP_UNITS => self.loop_units,
            sp::LOOP_EXIT => self.loop_exit,
            sp::PLAY_MODE => self.play_mode,
            sp::VOICE_COUNT => self.voice_count,
            sp::GLIDE => self.glide_ms,
            sp::SPREAD => self.spread,
            sp::ENV_PITCH => self.env_pitch,
            sp::ENV_POSITION => self.env_position,
            sp::ENV_SIZE => self.env_size,
            sp::ENV_FILTER => self.env_filter,
            sp::START_JITTER => self.start_jitter,
            sp::PITCH_JITTER => self.pitch_jitter,
            sp::SEED => self.seed,
            sp::SLIP => self.slip,
            sp::ATTACK_SHAPE => self.attack_shape,
            sp::FILTER_SLOPE => self.filter_slope,
            sp::COMB_FOCUS => self.comb_focus,
            sp::COMB_FEED => self.comb_feed,
            sp::COMB_DAMP => self.comb_damp,
            sp::COMB_MIX => self.comb_mix,
            sp::SOURCE_BEATS => self.source_beats,
            sp::FIT_BEATS => self.fit_beats,
            sp::SLICE_THRU => self.slice_thru,
            sp::HARD => self.hard,
            sp::SENSE => self.sense,
            sp::MIN_GAP => self.min_gap_ms,

            _ => return None,
        })
    }

    /// The filter shape, as the kernel names it.
    pub fn filter(&self) -> FilterMode {
        match self.filter_mode.round() {
            m if m == sp::FILT_HIGHPASS => FilterMode::Highpass,
            m if m == sp::FILT_BANDPASS => FilterMode::BandpassUnity,
            m if m == sp::FILT_NOTCH => FilterMode::Notch,
            _ => FilterMode::Lowpass,
        }
    }

    /// Whether a note holds (classic) or plays through (one-shot, slice
    /// with choke off).
    fn one_shot(&self) -> bool {
        self.mode.round() == sp::MODE_ONE_SHOT
    }

    fn slicing(&self) -> bool {
        self.mode.round() == sp::MODE_SLICE
    }

    fn reversed(&self) -> bool {
        self.reverse.round() >= 1.0
    }
}

// --------------------------------------------------------------- voice ---

/// Which way a loop turns a voice around, once resolved for the note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Looping {
    Off,
    /// Wraps back to the loop start, with a crossfade.
    Forward,
    /// Turns around at each end. No crossfade: a ping-pong turnaround
    /// re-reads the same samples backwards, so the signal is continuous
    /// in value and only its derivative kinks. A crossfade there would be
    /// blending a signal with itself.
    PingPong,
}

/// One sounding note.
///
/// Positions are `f64` FRAMES into the material. At the five-minute cap
/// an `f64` ulp is about two nanoframes, so accumulation drift is
/// inaudible by a very wide margin; 32.32 fixed point is the defensible
/// alternative and this is simply the simpler one.
#[derive(Debug, Clone, Copy)]
struct Voice {
    sample_rate: f32,
    source_frames: f64,
    patch: SamplerParams,
    locks: [u64; 2],
    reader: SampleReader,
    use_reader: bool,
    released_tail: bool,
    motion_phase: f64,
    pitch_drift: f32,
    filter2_l: Svf,
    filter2_r: Svf,
    attack_split: crate::dsp::dynamics::TransientSplit,
    comb_l: crate::dsp::delay::FeedbackDelay,
    comb_r: crate::dsp::delay::FeedbackDelay,
    steal_l: f32,
    steal_r: f32,
    steal_left: usize,
    last_l: f32,
    last_r: f32,
    active: bool,
    /// True from note-on until the gate drops. Separate from `active`,
    /// which stays true through the release.
    gated: bool,
    pitch: u8,
    vel: f32,
    age: u64,

    /// Where the head is, and which way it is going.
    pos: f64,
    dir: f64,
    /// Base increment magnitude, before per-chunk pitch modulation.
    base_inc: f64,
    /// The increment actually in use, rebuilt once per chunk.
    inc: f64,

    /// The span this note plays, LOW then HIGH regardless of direction.
    span_lo: f64,
    span_hi: f64,
    /// The loop's low end. The span's high end is always the loop's high
    /// end — see the brief: there is no separate loop end to line up.
    loop_lo: f64,
    looping: Looping,
    /// Crossfade length in frames, already clamped to fit both the loop
    /// and the material either side of it.
    xfade: f64,

    /// Samples since the gate. Drives the fade-in, which is a declick and
    /// therefore about time rather than about position.
    since_gate: u64,
    fade_in_frames: f64,
    fade_out_frames: f64,

    amp: Adsr,
    moden: Adsr,
    filter_l: Svf,
    filter_r: Svf,
    /// What the filter was last prepared with, so a static cutoff costs no
    /// transcendental at all.
    filter_hz: f32,
    filter_q: f32,
    over_l: Oversampler2x,
    over_r: Oversampler2x,
    shaper: Waveshaper,
    crush_l: Downsampler,
    crush_r: Downsampler,

    /// The choke / steal / all-sound-off ramp. Distinct from the
    /// positional fades: this one is about a note being taken away.
    cut: Fade,
    cutting: bool,

    pan_l: f32,
    pan_r: f32,

    chunk_left: usize,
}

impl Voice {
    fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            source_frames: 0.0,
            patch: SamplerParams::default(),
            locks: [0; 2],
            reader: SampleReader::new(),
            use_reader: false,
            released_tail: false,
            motion_phase: 0.0,
            pitch_drift: 0.0,
            filter2_l: Svf::new(),
            filter2_r: Svf::new(),
            attack_split: crate::dsp::dynamics::TransientSplit::new(),
            comb_l: crate::dsp::delay::FeedbackDelay::new(),
            comb_r: crate::dsp::delay::FeedbackDelay::new(),
            steal_l: 0.0,
            steal_r: 0.0,
            steal_left: 0,
            last_l: 0.0,
            last_r: 0.0,

            active: false,
            gated: false,
            pitch: 60,
            vel: 1.0,
            age: 0,
            pos: 0.0,
            dir: 1.0,
            base_inc: 1.0,
            inc: 1.0,
            span_lo: 0.0,
            span_hi: 0.0,
            loop_lo: 0.0,
            looping: Looping::Off,
            xfade: 0.0,
            since_gate: 0,
            fade_in_frames: 0.0,
            fade_out_frames: 0.0,
            amp: Adsr::new(),
            moden: Adsr::new(),
            filter_l: Svf::new(),
            filter_r: Svf::new(),
            filter_hz: 0.0,
            filter_q: 0.0,
            over_l: Oversampler2x::new(),
            over_r: Oversampler2x::new(),
            shaper: Waveshaper::new(),
            crush_l: Downsampler::new(),
            crush_r: Downsampler::new(),
            cut: Fade::new(),
            cutting: false,
            pan_l: 1.0,
            pan_r: 1.0,
            chunk_left: 0,
        }
    }

    fn silence(&mut self) {
        self.active = false;
        self.gated = false;
        self.amp.reset();
        self.moden.reset();
        self.filter_l.reset();
        self.filter_r.reset();
        self.over_l.reset();
        self.over_r.reset();
        self.crush_l.reset();
        self.crush_r.reset();
        self.cut.reset();
        self.cutting = false;
        self.chunk_left = 0;
    }

    /// Take this note away over its own fade-out. Used for choke, for
    /// stealing, and for a slice cutting the one before it.
    fn cut_now(&mut self) {
        if !self.active {
            return;
        }
        let samples = self.fade_out_frames.max(1.0) as u32;
        self.cut.start_out(samples);
        self.cutting = true;
        self.gated = false;
    }

    fn sounding(&self) -> bool {
        self.active
    }
}

// ---------------------------------------------------------------- bank ---

/// Scratch the render walk needs. The per-chunk buffers are fixed arrays
/// — a chunk is a compile-time length, so there is nothing to size and
/// nothing to allocate. Only the right-channel readback depends on the
/// block, and it is sized once at prepare.
struct Scratch {
    l: [f32; CHUNK],
    r: [f32; CHUNK],
    /// The amp envelope, the mod envelope, and the two positional fades
    /// multiplied together — all per sample.
    env: [f32; CHUNK],
    menv: [f32; CHUNK],
    edge: [f32; CHUNK],
    over: [f32; CHUNK * 2],
    /// Filled during the walk and read back by the node, because the
    /// clock's own buffer is mono.
    right: Vec<f32>,
}

impl Scratch {
    fn new(block: usize) -> Self {
        Self {
            l: [0.0; CHUNK],
            r: [0.0; CHUNK],
            env: [0.0; CHUNK],
            menv: [0.0; CHUNK],
            edge: [0.0; CHUNK],
            over: [0.0; CHUNK * 2],
            right: vec![0.0; block.max(1)],
        }
    }
}

/// The sampler: material, settings, and eight voices that read it.
pub struct SamplerVoices {
    locks: [u64; 2],
    beats_per_sample: f64,
    comb_buffers: Vec<(Vec<f32>, Vec<f32>)>,
    material: Material,
    /// What letters set — the base a plock restore returns to.
    base: SamplerParams,
    /// What is actually in force, which a plock may have moved.
    live: SamplerParams,
    sample_rate: f32,
    voices: [Voice; VOICES],
    /// Slice boundaries in material frames, sorted and in range, baked at
    /// compile. A fixed array rather than a `Vec`: sixty-four `u64` is
    /// half a kilobyte and it makes the whole bank movable without an
    /// allocation anywhere near the callback.
    slices: [u64; MAX_SLICES],
    slice_count: usize,
    onsets: [u64; MAX_SLICES],
    onset_count: usize,
    scratch: Scratch,
}

impl SamplerVoices {
    pub fn new(sample_rate: f32, block: usize, params: SamplerParams, material: Material) -> Self {
        let mut bank = Self {
            locks: [0; 2],
            beats_per_sample: 120.0 / 60.0 / f64::from(sample_rate.max(1.0)),
            comb_buffers: (0..VOICES)
                .map(|_| {
                    let n = crate::dsp::delay::FeedbackDelay::needed_len(
                        (sample_rate.max(1.0) * 0.5) as usize,
                    );
                    (vec![0.0; n], vec![0.0; n])
                })
                .collect(),

            material,
            base: params,
            live: params,
            sample_rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            voices: core::array::from_fn(|_| Voice::new()),
            slices: [0; MAX_SLICES],
            slice_count: 0,
            onsets: [0; MAX_SLICES],
            onset_count: 0,
            scratch: Scratch::new(block),
        };
        bank.prepare();
        let onsets = crate::slice::transients_of_spaced(
            &crate::slice::Planar::from(&bank.material),
            params.sense,
            params.min_gap_ms,
        );
        bank.onset_count = onsets.len().min(MAX_SLICES);
        for (dst, src) in bank.onsets.iter_mut().zip(onsets) {
            *dst = src;
        }
        bank
    }

    /// Green zone: hand the bank its slice table, already sorted and in
    /// range. Compile does the sorting for the same reason it sorts a
    /// clip's envelope — the callback walks this assuming it rises, and
    /// sorting there would be both an allocation and an unbounded path.
    pub fn set_slices(&mut self, slices: &[u64]) {
        self.slice_count = 0;
        for (dst, src) in self.slices.iter_mut().zip(slices.iter()) {
            *dst = (*src).min(self.material.frames);
            self.slice_count += 1;
        }
    }

    pub fn material(&self) -> &Material {
        &self.material
    }

    pub fn params(&self) -> SamplerParams {
        self.base
    }

    /// Green zone: rebuild everything a parameter change cannot be
    /// expressed as a per-chunk value.
    fn prepare(&mut self) {
        let sr = self.sample_rate;
        let p = self.live;
        for v in self.voices.iter_mut() {
            v.amp.prepare(
                sr,
                p.amp_attack_ms,
                p.amp_decay_ms,
                p.amp_sustain,
                p.amp_release_ms,
            );
            v.moden.prepare(
                sr,
                p.mod_attack_ms,
                p.mod_decay_ms,
                p.mod_sustain,
                p.mod_release_ms,
            );
            v.over_l.prepare();
            v.over_r.prepare();
            v.crush_l.prepare(sr);
            v.crush_r.prepare(sr);
            v.crush_l.set_bits(p.bits);
            v.crush_r.set_bits(p.bits);
            v.crush_l.set_rate(p.rate_hz);
            v.crush_r.set_rate(p.rate_hz);
        }
    }

    /// A LETTER: moves the base, and the live value with it, so a knob
    /// turned mid-playback is heard on every unlocked note.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.live.set(param, value);
        for v in &mut self.voices {
            let locked = v
                .locks
                .get(param as usize / 64)
                .is_some_and(|bits| bits & (1u64 << (param % 64)) != 0);
            if !locked {
                v.patch.set(param, value);
            }
        }
        self.after_param(param);
    }

    /// A parameter LOCK at a note boundary. `None` restores the base.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        match value {
            Some(v) => self.live.set(param, v),
            None => {
                if let Some(v) = self.base.get(param) {
                    self.live.set(param, v);
                }
            }
        }
        if let Some(bits) = self.locks.get_mut(param as usize / 64) {
            if value.is_some() {
                *bits |= 1u64 << (param % 64);
            } else {
                *bits &= !(1u64 << (param % 64));
            }
        }
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        let age = self.voices.iter().filter(|v| v.active).map(|v| v.age).max();
        if let (Some(live), Some(base)) = (self.live.get(param), self.base.get(param)) {
            self.live
                .set(param, live + (base - live) * alpha.clamp(0.0, 1.0));
            for v in &mut self.voices {
                if v.active
                    && Some(v.age) == age
                    && v.locks
                        .get(param as usize / 64)
                        .is_some_and(|bits| bits & (1u64 << (param % 64)) != 0)
                    && let Some(value) = v.patch.get(param)
                {
                    v.patch
                        .set(param, value + (base - value) * alpha.clamp(0.0, 1.0));
                }
            }
            self.after_param(param);
        }
    }

    /// The rows that are not simply read where they are used.
    fn after_param(&mut self, param: u32) {
        match param {
            sp::AMP_A | sp::AMP_D | sp::AMP_S | sp::AMP_R => {
                let sr = self.sample_rate;
                for v in self.voices.iter_mut() {
                    let p = v.patch;
                    v.amp.prepare(
                        sr,
                        p.amp_attack_ms,
                        p.amp_decay_ms,
                        p.amp_sustain,
                        p.amp_release_ms,
                    );
                }
            }
            sp::MOD_A | sp::MOD_D | sp::MOD_S | sp::MOD_R => {
                let sr = self.sample_rate;
                for v in self.voices.iter_mut() {
                    let p = v.patch;
                    v.moden.prepare(
                        sr,
                        p.mod_attack_ms,
                        p.mod_decay_ms,
                        p.mod_sustain,
                        p.mod_release_ms,
                    );
                }
            }
            sp::BITS => {
                for v in self.voices.iter_mut() {
                    let p = v.patch;
                    v.crush_l.set_bits(p.bits);
                    v.crush_r.set_bits(p.bits);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ notes ---

    pub fn all_sound_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.silence();
        }
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            if v.active && v.gated {
                v.gated = false;
                v.amp.gate_off();
                v.moden.gate_off();
            }
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        // A one-shot ignores the gate going down: the sample IS the
        // gesture, the same contract `KickVoice` keeps.
        for v in self.voices.iter_mut() {
            if v.active && v.gated && v.pitch == pitch && !v.patch.one_shot() {
                if v.patch.loop_exit >= 0.5 && v.use_reader {
                    v.reader.exit_loop();
                    v.released_tail = true;
                    v.looping = Looping::Off;
                    v.gated = false;
                    continue;
                }
                v.gated = false;
                v.amp.gate_off();
                v.moden.gate_off();
            }
        }
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        if self.material.is_empty() {
            return;
        }
        let p = self.live;
        let sr = self.sample_rate;
        let frames = self.material.frames as f64;
        if p.play_mode.round() == 2.0 {
            if let Some(v) = self.voices.iter_mut().find(|v| v.active && v.gated) {
                v.pitch = pitch;
                v.patch = p;
                v.locks = self.locks;
                return;
            }
        }
        if p.play_mode.round() == 1.0 {
            for v in &mut self.voices {
                v.cut_now();
            }
        }

        // Where in the material this note reads, and at what ratio.
        let Some((lo, hi, ratio_semitones)) = self.region_for(pitch) else {
            // A note past the last slice sounds NOTHING. Deliberately not
            // clamped onto the last slice: a clamp turns a wrong note
            // into a right-sounding one and hides the mistake.
            return;
        };

        // Choke: in slice mode with choke on, a new trigger takes the
        // previous one away rather than stacking on it.
        if p.slicing() && p.choke.round() >= 1.0 {
            for v in self.voices.iter_mut() {
                v.cut_now();
            }
        }

        let slot = self.steal();
        let Some(v) = self.voices.get_mut(slot) else {
            return;
        };

        let vel = f32::from(vel) / 127.0;
        v.patch = p;
        v.locks = self.locks;
        v.released_tail = false;
        v.active = true;
        v.gated = true;
        v.pitch = pitch;
        v.vel = vel;
        v.age = age;
        v.since_gate = 0;
        v.cut.reset();
        v.cutting = false;
        v.chunk_left = 0;

        v.span_lo = lo;
        v.span_hi = hi;
        v.dir = if p.reversed() { -1.0 } else { 1.0 };

        // The `start` destination is sampled ONCE, here. An envelope is
        // zero at its own gate, so applying it continuously to a read
        // position would be a modulated delay line rather than a sample
        // start; what people actually want from this destination is
        // velocity, and that is what it takes.
        let start_shift = if p.mod_dest.round() == sp::DEST_START {
            (hi - lo) * f64::from(p.mod_depth * sp::DEST_START_FRACTION * vel)
        } else {
            0.0
        };
        v.pos = if p.reversed() { hi } else { lo } + start_shift;
        v.pos = v.pos.clamp(lo, hi);

        // Legacy geometry; the advanced reader resolves an independent
        // sustain span, including loops inside a slice.
        let loop_mode = p.loop_mode.round();
        let loop_lo = (lo + (hi - lo) * f64::from(p.loop_start.clamp(0.0, 1.0))).clamp(lo, hi);
        let loop_len = hi - loop_lo;
        v.loop_lo = loop_lo;
        v.looping = if loop_len < 2.0 {
            // A loop of no length is silently loop-off rather than a
            // division by zero in the callback.
            Looping::Off
        } else if loop_mode == sp::LOOP_FORWARD {
            Looping::Forward
        } else if loop_mode == sp::LOOP_PINGPONG {
            Looping::PingPong
        } else {
            Looping::Off
        };

        // The crossfade needs material on BOTH sides of the wrap: one
        // loop length back from the wrap point, which is before the loop
        // start going forward and past the span end going backwards.
        let want = f64::from(p.loop_xfade_ms.max(0.0)) * f64::from(sr) / 1_000.0;
        let headroom = if p.reversed() { frames - hi } else { loop_lo };
        v.xfade = if v.looping == Looping::Forward {
            want.min(loop_len * 0.5).min(headroom).max(0.0)
        } else {
            0.0
        };

        let ms_to_frames = f64::from(sr) / 1_000.0;
        v.fade_in_frames = f64::from(p.fade_in_ms.max(0.0)) * ms_to_frames;
        v.fade_out_frames = f64::from(p.fade_out_ms.max(0.0)) * ms_to_frames;

        v.base_inc = 2f64.powf(f64::from(ratio_semitones) / 12.0);
        v.inc = v.base_inc;

        v.filter_hz = 0.0;
        v.filter_q = 0.0;
        v.filter_l.reset();
        v.filter_r.reset();
        v.over_l.reset();
        v.over_r.reset();
        v.crush_l.reset();
        v.crush_r.reset();
        Self::prepare_note(v, p, sr);
        Self::start_reader(v, p, sr, self.beats_per_sample, frames);
        v.reader.set_onsets(&self.onsets[..self.onset_count]);
        if let Some((left, right)) = self.comb_buffers.get_mut(slot) {
            left.fill(0.0);
            right.fill(0.0);
        }
        v.amp.gate_on();
        v.moden.gate_on();
    }

    /// The span a note reads and how far it is transposed, or `None` if
    /// the note maps to nothing.
    fn region_for(&self, pitch: u8) -> Option<(f64, f64, f32)> {
        let p = self.live;
        let frames = self.material.frames as f64;
        let detune = p.tune + p.fine / 100.0;

        if p.slicing() {
            // The SLICE row says which cut; the note says how fast. A
            // slice past the table is silence, not the last slice: a
            // lock that points at nothing should be heard as nothing.
            let index = (p.slice.round().max(1.0) as usize) - 1;
            let count = self.slice_count;
            if count == 0 || index >= count {
                return None;
            }
            let lo = *self.slices.get(index)? as f64;
            // The array is a fixed sixty-four entries and only the first
            // `slice_count` of them mean anything. Reading past the count
            // would find a zero and hand this slice an end BEFORE its
            // start, which reads as an empty span and sounds as silence —
            // a bug that only ever shows on the LAST slice, which is not
            // where anybody looks.
            let hi = if index + 1 < count {
                *self.slices.get(index + 1)? as f64
            } else {
                frames
            };
            let hi = if p.slice_thru >= 0.5 { frames } else { hi };
            let span = hi - lo;
            let start = lo + span * f64::from(p.start.clamp(0.0, 1.0));
            let end = lo + span * f64::from(p.end.clamp(0.0, 1.0));
            let (lo, hi) = (start, end.max(start));
            if hi - lo < 2.0 {
                return None;
            }
            // Slice mode: the row selects, and the note transposes,
            // as it does in classic mode.
            return Some((lo, hi, f32::from(pitch) - p.root + detune));
        }

        let a = frames * f64::from(p.start.clamp(0.0, 1.0));
        let b = frames * f64::from(p.end.clamp(0.0, 1.0));
        let (lo, hi) = if b > a { (a, b) } else { (a, frames) };
        if hi - lo < 2.0 {
            return None;
        }
        Some((lo, hi, f32::from(pitch) - p.root + detune))
    }

    /// A free voice, or the best one to steal: a releasing voice before a
    /// held one, and the oldest before the newest.
    fn steal(&mut self) -> usize {
        let count = (self.live.voice_count.round() as usize).clamp(1, VOICES);
        if let Some(v) = (0..count).find(|i| self.voices.get(*i).is_some_and(|v| !v.active)) {
            return v;
        }
        let mut best = 0usize;
        let mut key = (true, u64::MAX);
        for (i, v) in self.voices.iter().take(count).enumerate() {
            if (v.gated, v.age) < key {
                key = (v.gated, v.age);
                best = i;
            }
        }
        // A stolen voice is silenced rather than ramped: the note that
        // takes its slot starts on the very next sample, so there is no
        // room to ramp in, and the new note's own fade-in covers the
        // seam.
        if let Some(v) = self.voices.get_mut(best) {
            v.steal_l = v.last_l;
            v.steal_r = v.last_r;
            v.steal_left = 64;
            v.silence();
        }
        best
    }

    /// The right channel of the last render, for the node to read back.
    pub fn right(&self, len: usize) -> &[f32] {
        self.scratch.right.get(..len).unwrap_or(&[])
    }

    pub fn any_active(&self) -> bool {
        self.voices.iter().any(Voice::sounding)
    }

    // ----------------------------------------------------------- render ---

    /// Red zone. Write `out.len()` samples of the left channel into
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
        for v in 0..VOICES {
            self.render_voice(v, out, at);
        }
        // The ramp advances exactly once per sample on every path —
        // silent voices included — which is what lets the caller land it
        // on its target exactly.
        for (i, s) in out.iter_mut().enumerate() {
            let g = gain.next();
            *s *= g;
            if let Some(r) = self.scratch.right.get_mut(at + i) {
                *r *= g;
            }
        }
    }

    fn render_voice(&mut self, index: usize, out: &mut [f32], at: usize) {
        if !self.voices.get(index).is_some_and(|v| v.active) {
            return;
        }
        let mut done = 0usize;
        while done < out.len() {
            let Some(voice) = self.voices.get_mut(index) else {
                return;
            };
            if !voice.active {
                return;
            }
            if voice.chunk_left == 0 {
                let patch = voice.patch;
                Self::retune(voice, &patch, self.sample_rate);
                Self::update_reader(voice, &patch, self.sample_rate, self.beats_per_sample);
                voice.chunk_left = CHUNK;
            }
            let n = (out.len() - done).min(voice.chunk_left).min(CHUNK);
            if n == 0 {
                return;
            }
            let patch = voice.patch;
            Self::render_chunk(voice, &patch, &self.material, &mut self.scratch, n);
            if let Some(buffers) = self.comb_buffers.get_mut(index) {
                Self::finish_voice(voice, &patch, &mut self.scratch, n, buffers);
            }

            // Saturating, because `render_chunk` can end the voice
            // mid-chunk — the span ran out, the envelope finished, the
            // choke ramp landed — and `silence` zeroes this on its way
            // past. The chunk's audio is still mixed in below; the loop's
            // own top-of-iteration check is what stops the next one.
            voice.chunk_left = voice.chunk_left.saturating_sub(n);

            let (Some(l), Some(r)) = (self.scratch.l.get(..n), self.scratch.r.get(..n)) else {
                return;
            };
            let note_gain = 10f32.powf((voice.patch.gain_db - self.base.gain_db) / 20.0);
            let (pan_l, pan_r) = (voice.pan_l * note_gain, voice.pan_r * note_gain);
            for (i, (a, b)) in l.iter().zip(r.iter()).enumerate() {
                if let Some(d) = out.get_mut(done + i) {
                    *d += *a * pan_l;
                }
                if let Some(d) = self.scratch.right.get_mut(at + done + i) {
                    *d += *b * pan_r;
                }
            }
            done += n;
        }
    }

    /// Everything that costs a transcendental, once per control chunk.
    fn retune(v: &mut Voice, p: &SamplerParams, sr: f32) {
        // The modulation envelope's value at the chunk boundary drives
        // every coefficient destination. Sampled rather than averaged:
        // a chunk is 0.67 ms and the envelope is a straight line across
        // it.
        let env = v.moden.current();
        let depth = p.mod_depth * env;
        let dest = p.mod_dest.round();

        // Pitch.
        let semitones = if dest == sp::DEST_PITCH {
            depth * sp::DEST_PITCH_SEMITONES
        } else {
            0.0
        };
        let target = 2f64.powf(
            f64::from(
                f32::from(v.pitch) - p.root
                    + p.tune
                    + p.fine / 100.0
                    + v.pitch_drift
                    + semitones
                    + p.env_pitch * env,
            ) / 12.0,
        );
        let glide = if p.glide_ms > 0.0 {
            1.0 - (-(CHUNK as f64) / (f64::from(sr) * f64::from(p.glide_ms) * 0.001)).exp()
        } else {
            1.0
        };
        v.inc += (target - v.inc) * glide;

        // Filter: cutoff from the knob, keytrack and the envelope.
        let keytrack = p.keytrack * (f32::from(v.pitch) - 60.0) / 12.0;
        let octaves = keytrack
            + p.env_filter * env / 12.0
            + if dest == sp::DEST_CUTOFF {
                depth * sp::DEST_CUTOFF_OCTAVES
            } else {
                0.0
            };
        let hz = (p.cutoff_hz * octaves.exp2()).clamp(20.0, 20_000.0);
        let q = p.res.clamp(0.5, 20.0);
        // A static cutoff costs no `tan` at all — the common case for a
        // sampler, whose filter is usually parked.
        if (hz - v.filter_hz).abs() > 0.01 || (q - v.filter_q).abs() > 0.001 {
            v.filter_l.prepare(sr, hz, q);
            v.filter_r.prepare(sr, hz, q);
            v.filter2_l.prepare(sr, hz, 0.707);
            v.filter2_r.prepare(sr, hz, 0.707);
            v.filter_hz = hz;
            v.filter_q = q;
        }

        // Drive.
        let drive_amount =
            (p.drive + if dest == sp::DEST_DRIVE { depth } else { 0.0 }).clamp(0.0, 1.0);
        v.shaper.configure(
            ShapeMode::SoftClip,
            1.0 + drive_amount * DRIVE_SPAN,
            drive_amount * DRIVE_BIAS,
            1.0,
        );

        // The converter clock.
        let rate = if dest == sp::DEST_RATE {
            p.rate_hz * (depth * sp::DEST_RATE_OCTAVES).exp2()
        } else {
            p.rate_hz
        };
        v.crush_l.set_rate(rate);
        v.crush_r.set_rate(rate);

        // Pan is a BALANCE, not a pan pot: centre is unity on both sides
        // rather than −3 dB on both. A sampler is usually playing stereo
        // material, and a centred stereo file must come out as the file —
        // the device's transparency claim would not survive an equal-power
        // law quietly taking 3 dB off it.
        let pan = (p.pan + if dest == sp::DEST_PAN { depth } else { 0.0 }).clamp(-1.0, 1.0);
        v.pan_l = (1.0 - pan).min(1.0);
        v.pan_r = (1.0 + pan).min(1.0);
    }

    /// One control chunk of one voice into the scratch pair.
    fn render_chunk(
        v: &mut Voice,
        p: &SamplerParams,
        material: &Material,
        scratch: &mut Scratch,
        n: usize,
    ) {
        let left = material.channel(0);
        // A mono file feeds both sides; a file with more than two
        // channels is read as its first two, which is what every sampler
        // that ever met a surround file does.
        let right = if material.channels > 1 {
            material.channel(1)
        } else {
            left
        };

        // --- the read head, and the two positional fades ------------
        //
        // The fades are computed HERE, inside the read loop, because both
        // of them depend on where the head is at THAT sample. Computed
        // after the loop they would use the position the chunk ended at,
        // and a fade-out would arrive thirty-two samples late.
        for i in 0..n {
            let g = Self::edge_gain(v);
            let (a, b) = if v.use_reader {
                let pair = v.reader.tick(left, right);
                v.pos = v.reader.position();
                (pair[0], pair[1])
            } else {
                Self::read(v, left, right)
            };
            if let Some(s) = scratch.l.get_mut(i) {
                *s = a;
            }
            if let Some(s) = scratch.r.get_mut(i) {
                *s = b;
            }
            if let Some(s) = scratch.edge.get_mut(i) {
                *s = g;
            }
            if v.use_reader {
                v.since_gate = v.since_gate.saturating_add(1);
                if !v.reader.active() {
                    v.active = false;
                }
            } else {
                Self::advance(v);
            }
            if !v.active {
                // The span finished mid-chunk. Everything after it is
                // silence, and the fade-out has already brought the last
                // samples down to meet it.
                for tail in i + 1..n {
                    if let Some(s) = scratch.l.get_mut(tail) {
                        *s = 0.0;
                    }
                    if let Some(s) = scratch.r.get_mut(tail) {
                        *s = 0.0;
                    }
                    if let Some(s) = scratch.edge.get_mut(tail) {
                        *s = 0.0;
                    }
                }
                break;
            }
        }

        // --- envelopes ------------------------------------------------
        //
        // Both envelopes advance on every chunk whether or not anything
        // reads them. The mod envelope's VALUE at the next chunk boundary
        // is what `retune` samples, and an envelope that only ran when it
        // was wired would arrive there late.
        if let Some(env) = scratch.env.get_mut(..n) {
            v.amp.process(env);
        }
        if let Some(menv) = scratch.menv.get_mut(..n) {
            v.moden.process(menv);
        }

        // The choke / steal ramp, advanced ONCE and applied to both
        // channels: a copy for the left, the real one for the right, so
        // the two sides see the same gain rather than the right seeing
        // the ramp twice as fast.
        if v.cutting {
            let mut mirror = v.cut;
            if let Some(l) = scratch.l.get_mut(..n) {
                mirror.process(l);
            }
            if let Some(r) = scratch.r.get_mut(..n) {
                v.cut.process(r);
            }
            if !v.cut.active() {
                v.silence();
                return;
            }
        }

        // --- filter ---------------------------------------------------
        //
        // Skipped entirely when it would be a wire: a lowpass parked at
        // 20 kHz with no keytrack and no envelope on it is the sampler's
        // default, and paying for it there would be paying for it always.
        let mode = p.filter();
        let filter_open = mode == FilterMode::Lowpass
            && v.filter_hz >= 19_999.0
            && p.keytrack <= 0.0
            && !(p.mod_dest.round() == sp::DEST_CUTOFF && p.mod_depth != 0.0);
        if !filter_open {
            if let Some(l) = scratch.l.get_mut(..n) {
                v.filter_l.process(l, mode);
            }
            if let Some(r) = scratch.r.get_mut(..n) {
                v.filter_r.process(r, mode);
            }
            if p.filter_slope >= 0.5 {
                if let Some(l) = scratch.l.get_mut(..n) {
                    v.filter2_l.process(l, mode);
                }
                if let Some(r) = scratch.r.get_mut(..n) {
                    v.filter2_r.process(r, mode);
                }
            }
        }

        // --- drive, 2x oversampled ------------------------------------
        if p.drive > 0.0 || (p.mod_dest.round() == sp::DEST_DRIVE && p.mod_depth != 0.0) {
            Self::drive_channel(
                &mut v.over_l,
                &v.shaper,
                &mut scratch.l,
                &mut scratch.over,
                n,
            );
            Self::drive_channel(
                &mut v.over_r,
                &v.shaper,
                &mut scratch.r,
                &mut scratch.over,
                n,
            );
        }

        // --- the converter --------------------------------------------
        if let Some(l) = scratch.l.get_mut(..n) {
            v.crush_l.process(l);
        }
        if let Some(r) = scratch.r.get_mut(..n) {
            v.crush_r.process(r);
        }

        // A voice whose envelope has finished costs nothing further.
        if !v.amp.active() {
            v.silence();
        }
    }

    /// The per-voice drive stage: 2x oversampled, so the harmonics it
    /// makes above Nyquist fold back OUT of the band rather than into it.
    ///
    /// Which matters more here than it looks: the converter stage right
    /// after this one is where folding is supposed to happen, and it
    /// folds whatever it is given. Aliasing the drive first would mean
    /// crushing a mess rather than crushing a signal.
    fn drive_channel(
        over: &mut Oversampler2x,
        shaper: &Waveshaper,
        io: &mut [f32; CHUNK],
        scratch: &mut [f32; CHUNK * 2],
        n: usize,
    ) {
        let Some(up) = scratch.get_mut(..n * 2) else {
            return;
        };
        let Some(block) = io.get(..n) else { return };
        over.up(block, up);
        shaper.process(up);
        let Some(out) = io.get_mut(..n) else { return };
        over.down(up, out);
    }

    /// One frame from the material, with the loop crossfade applied.
    fn read(v: &Voice, left: &[f32], right: &[f32]) -> (f32, f32) {
        let a = interp::hermite_at(left, v.pos);
        let b = interp::hermite_at(right, v.pos);
        if v.looping != Looping::Forward || v.xfade <= 0.0 {
            return (a, b);
        }
        // Distance to the point where the loop wraps, in the direction of
        // travel.
        let wrap = if v.dir > 0.0 { v.span_hi } else { v.loop_lo };
        let to_wrap = (wrap - v.pos).abs();
        if to_wrap >= v.xfade {
            return (a, b);
        }
        // The other side of the seam is exactly one loop length back
        // along the direction of travel: at the wrap point it IS the
        // sample the head is about to jump to, which is what makes the
        // join continuous.
        let length = v.span_hi - v.loop_lo;
        let other = v.pos - v.dir * length;
        let w = (to_wrap / v.xfade).clamp(0.0, 1.0) as f32;
        let angle = w * core::f32::consts::FRAC_PI_2;
        let (near, far) = (angle.sin(), angle.cos());
        (
            a * near + interp::hermite_at(left, other) * far,
            b * near + interp::hermite_at(right, other) * far,
        )
    }

    /// Move the head one frame, and resolve what it ran into.
    fn advance(v: &mut Voice) {
        v.pos += v.dir * v.inc;
        v.since_gate += 1;
        match v.looping {
            Looping::Off => {
                if v.pos >= v.span_hi || v.pos <= v.span_lo {
                    // The span is finished. The positional fade-out has
                    // already brought this to silence, so stopping here
                    // is a stop and not a cut.
                    v.pos = v.pos.clamp(v.span_lo, v.span_hi);
                    v.active = false;
                }
            }
            Looping::Forward => {
                let length = v.span_hi - v.loop_lo;
                if length <= 0.0 {
                    v.active = false;
                } else if v.dir > 0.0 && v.pos >= v.span_hi {
                    v.pos -= length;
                } else if v.dir < 0.0 && v.pos <= v.loop_lo {
                    v.pos += length;
                }
            }
            Looping::PingPong => {
                if v.pos >= v.span_hi {
                    v.pos = v.span_hi - (v.pos - v.span_hi);
                    v.dir = -1.0;
                } else if v.pos <= v.loop_lo {
                    v.pos = v.loop_lo + (v.loop_lo - v.pos);
                    v.dir = 1.0;
                }
            }
        }
    }

    /// The two positional fades, for the sample the head is on RIGHT NOW.
    ///
    /// Fade IN is measured in time since the gate, because a declick is
    /// about the seam at the start of a note and has nothing to do with
    /// where in the file that note reads. Fade OUT is measured in
    /// POSITION, because what it is getting out of the way of is the end
    /// of the span — which arrives sooner when the note is pitched up.
    fn edge_gain(v: &Voice) -> f32 {
        let mut g = 1.0f32;
        if v.fade_in_frames > 0.0 {
            g *= (v.since_gate as f64 / v.fade_in_frames).clamp(0.0, 1.0) as f32;
        }
        // Only where there is an end to arrive at. A loop has none.
        if v.looping == Looping::Off && v.fade_out_frames > 0.0 && v.inc > 0.0 {
            let target = if v.dir > 0.0 { v.span_hi } else { v.span_lo };
            let remaining = (target - v.pos).abs() / v.inc;
            g *= (remaining / v.fade_out_frames).clamp(0.0, 1.0) as f32;
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    pub(super) const SR: f32 = 48_000.0;
    pub(super) const BLOCK: usize = 256;

    /// Material whose left channel is `f(frame)` and whose right is
    /// `f(frame) + 1`, so a channel swap is visible.
    pub(super) fn material(frames: u64, f: impl Fn(u64) -> f32) -> Material {
        let mut samples = Vec::with_capacity(frames as usize * 2);
        for i in 0..frames {
            samples.push(f(i));
        }
        for i in 0..frames {
            samples.push(f(i) + 1.0);
        }
        Material {
            samples: Arc::new(samples),
            channels: 2,
            frames,
            source: std::path::PathBuf::from("test"),
            sample_rate: 48_000,
            original_rate: 48_000,
            truncated: false,
        }
    }

    /// A tone, which is what most of these want to look at.
    pub(super) fn tone(frames: u64) -> Material {
        material(frames, |i| {
            (std::f32::consts::TAU * 400.0 * i as f32 / SR).sin() * 0.5
        })
    }

    /// Every colour stage at the bottom of its range, every envelope held
    /// open, no fades: the configuration the transparency claim is about.
    pub(super) fn transparent() -> SamplerParams {
        let mut p = SamplerParams::default();
        p.drive = 0.0;
        p.rate_hz = 48_000.0;
        p.bits = 16.0;
        p.preamp = 0.0;
        p.cutoff_hz = 20_000.0;
        p.res = std::f32::consts::FRAC_1_SQRT_2;
        p.keytrack = 0.0;
        p.mod_depth = 0.0;
        p.fade_in_ms = 0.0;
        p.fade_out_ms = 0.0;
        p.loop_fade = 0.0;
        p.loop_xfade_ms = 0.0;
        p.amp_attack_ms = 0.1;
        p.amp_sustain = 1.0;
        p.velocity = 0.0;
        p
    }

    pub(super) fn bank(p: SamplerParams, m: Material) -> SamplerVoices {
        SamplerVoices::new(SR, BLOCK, p, m)
    }

    /// Render `frames` samples in blocks of `block`, returning both
    /// channels.
    pub(super) fn run(
        voices: &mut SamplerVoices,
        frames: usize,
        block: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        let mut done = 0;
        let mut buf = vec![0.0f32; block];
        while done < frames {
            let n = block.min(frames - done);
            let out = &mut buf[..n];
            let mut ramp = Ramp::across(1.0, 1.0, n);
            voices.render(out, 0, &mut ramp);
            left.extend_from_slice(out);
            right.extend_from_slice(voices.right(n));
            done += n;
        }
        (left, right)
    }

    // ------------------------------------------------------ the thesis ---

    /// The identity the whole device's honesty rests on: with every
    /// colour stage off and a file already at the device rate, what comes
    /// out is what went in, to the bit.
    ///
    /// Compared from sample 16 so the attack — which is 0.1 ms and cannot
    /// be zero, because the table has no zero — has landed on exactly
    /// 1.0. It stays there: sustain is 1.0, so the decay's step is zero.
    #[test]
    fn every_stage_off_is_a_wire() {
        let m = tone(2_000);
        let mut v = bank(transparent(), m.clone());
        v.note_on(60, 127, 0);
        let (l, r) = run(&mut v, 1_000, BLOCK);
        for i in 16..1_000 {
            assert_eq!(l[i], m.channel(0)[i], "left drifted at {i}");
            assert_eq!(r[i], m.channel(1)[i], "right drifted at {i}");
        }
    }

    /// Pitch is varispeed, and the arithmetic says so: an octave up is
    /// half the length.
    #[test]
    fn an_octave_up_is_half_as_long() {
        let m = tone(4_800);
        let mut p = transparent();
        p.amp_decay_ms = 16_000.0;
        let mut v = bank(p, m);
        v.note_on(72, 127, 0);
        let (l, _) = run(&mut v, 4_800, BLOCK);
        let last = l.iter().rposition(|s| s.abs() > 1e-6).unwrap_or(0);
        // 4800 frames at twice the rate is 2400, give or take the final
        // partial chunk.
        assert!(
            (2_300..=2_500).contains(&last),
            "an octave up ran for {last} frames"
        );
    }

    /// Split-block equivalence, over the state a lazy implementation gets
    /// wrong: the control chunk. If chunk boundaries were anchored to the
    /// render call rather than to the voice, a segment boundary would
    /// move where the coefficients are rebuilt and the two runs would
    /// diverge.
    #[test]
    fn a_block_boundary_does_not_change_the_answer() {
        let m = tone(4_000);
        let mut p = transparent();
        // Give the chunk rate something to do: a moving cutoff is what
        // makes the boundary observable at all.
        p.cutoff_hz = 800.0;
        p.mod_depth = 0.9;
        p.mod_dest = sp::DEST_CUTOFF;
        p.mod_decay_ms = 40.0;
        p.drive = 0.4;
        p.bits = 10.0;
        p.rate_hz = 22_050.0;

        let mut whole = bank(p, m.clone());
        whole.note_on(64, 100, 0);
        let (a, _) = run(&mut whole, 2_048, 2_048);

        let mut split = bank(p, m);
        split.note_on(64, 100, 0);
        let (b, _) = run(&mut split, 2_048, 37);

        assert_eq!(a, b, "the same note rendered differently in two shapes");
    }

    #[test]
    fn rendering_does_not_allocate() {
        let m = tone(8_000);
        let mut p = SamplerParams::default();
        p.loop_mode = sp::LOOP_FORWARD;
        p.loop_xfade_ms = 20.0;
        let mut v = bank(p, m);
        let mut buf = vec![0.0f32; BLOCK];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..200usize {
                if i % 5 == 0 {
                    v.note_on(60 + (i % 12) as u8, 100, i as u64);
                }
                if i % 7 == 0 {
                    v.note_off(60 + (i.saturating_sub(1) % 12) as u8);
                    v.set_param(sp::CUTOFF, 200.0 + (i % 40) as f32 * 300.0);
                    v.set_param(sp::RATE, 4_000.0 + (i % 9) as f32 * 4_000.0);
                    v.plock(sp::DRIVE, Some((i % 5) as f32 * 0.2));
                }
                if i % 11 == 0 {
                    v.plock(sp::DRIVE, None);
                    v.all_sound_off();
                }
                let mut ramp = Ramp::across(0.9, 1.0, BLOCK);
                v.render(&mut buf, 0, &mut ramp);
            }
        });
    }

    // -------------------------------------------------------- the edges ---

    /// Degenerate regions produce silence, not a panic and not a runaway.
    #[test]
    fn a_degenerate_region_is_silent() {
        for (start, end) in [(0.5f32, 0.5f32), (0.9, 0.1), (1.0, 1.0), (0.0, 0.0)] {
            let mut p = transparent();
            p.start = start;
            p.end = end;
            // `end` below `start` is read as "to the end of the file", so
            // only the truly empty spans are silent; either way nothing
            // here may panic.
            let mut v = bank(p, tone(1_000));
            v.note_on(60, 127, 0);
            let (l, _) = run(&mut v, 512, BLOCK);
            assert!(
                l.iter().all(|s| s.is_finite()),
                "start {start} end {end} went non-finite"
            );
        }
    }

    /// A one-frame file, an empty one, and a block longer than the
    /// material.
    #[test]
    fn tiny_material_is_survivable() {
        for frames in [0u64, 1, 2, 3] {
            let m = if frames == 0 {
                Material::empty()
            } else {
                tone(frames)
            };
            let mut v = bank(transparent(), m);
            v.note_on(60, 127, 0);
            let (l, r) = run(&mut v, 512, BLOCK);
            assert!(l.iter().all(|s| s.is_finite()), "{frames} frames: left");
            assert!(r.iter().all(|s| s.is_finite()), "{frames} frames: right");
        }
    }

    /// Nothing playing is exactly silent — not nearly, and not a
    /// denormal floor.
    #[test]
    fn an_idle_sampler_is_exactly_silent() {
        let mut v = bank(SamplerParams::default(), tone(1_000));
        let (l, r) = run(&mut v, 1_024, BLOCK);
        assert!(l.iter().all(|s| *s == 0.0), "left was not silent");
        assert!(r.iter().all(|s| *s == 0.0), "right was not silent");
    }

    // ---------------------------------------------------------- looping ---

    /// A loop with no crossfade repeats bit-exactly, forever. This is the
    /// property that says the wrap arithmetic does not drift: a loop that
    /// accumulated a fraction of a frame per cycle would pass a "sounds
    /// looped" test and fail this one on the second cycle.
    #[test]
    fn a_loop_without_a_crossfade_repeats_exactly() {
        let m = tone(1_000);
        let mut p = transparent();
        p.loop_mode = sp::LOOP_FORWARD;
        p.loop_xfade_ms = 0.0;
        p.loop_start = 0.0;
        p.amp_decay_ms = 16_000.0;
        let mut v = bank(p, m);
        v.note_on(60, 127, 0);
        let (l, _) = run(&mut v, 4_000, BLOCK);
        // Three full cycles after the attack has landed.
        for i in 16..1_000 {
            assert_eq!(l[i], l[i + 1_000], "cycle 2 differed at {i}");
            assert_eq!(l[i], l[i + 2_000], "cycle 3 differed at {i}");
        }
    }

    /// A crossfaded loop is continuous across the seam. Deliberately over
    /// material with a step at the loop point, which is exactly what a
    /// crossfade exists for.
    #[test]
    fn a_crossfaded_loop_has_no_step_at_the_seam() {
        // Ramps up to 0.9 and then drops to 0: without a crossfade the
        // wrap is a cliff.
        let m = material(2_000, |i| if i < 1_000 { i as f32 / 1_000.0 } else { 0.0 });
        let mut p = transparent();
        p.loop_mode = sp::LOOP_FORWARD;
        p.loop_start = 0.5;
        p.loop_xfade_ms = 5.0;
        p.loop_fade = 0.4;
        p.end = 0.5;
        p.amp_decay_ms = 16_000.0;
        let mut v = bank(p, m);
        v.note_on(60, 127, 0);
        let (l, _) = run(&mut v, 4_000, BLOCK);

        let biggest = l
            .windows(2)
            .skip(64)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // The material's own biggest step is one thousandth per sample.
        assert!(
            biggest < 0.01,
            "the seam stepped by {biggest}, which is a click"
        );
    }

    /// Ping-pong turns around at both ends, so the head visits the middle
    /// of the span more often than the ends and NEVER leaves it.
    #[test]
    fn a_pingpong_loop_stays_inside_its_span() {
        // A ramp, so the output value IS the read position.
        let m = material(1_000, |i| i as f32 / 1_000.0);
        let mut p = transparent();
        p.loop_mode = sp::LOOP_PINGPONG;
        p.loop_start = 0.25;
        p.end = 0.75;
        p.amp_decay_ms = 16_000.0;
        let mut v = bank(p, m);
        v.note_on(60, 127, 0);
        let (l, _) = run(&mut v, 4_000, BLOCK);
        // The recorded attack precedes entry into the sustain loop.
        for (i, s) in l.iter().enumerate().skip(188) {
            assert!(
                (0.18..=0.77).contains(s),
                "sample {i} read {s}, which is outside the loop"
            );
        }
        // And it really did turn around: a forward-only loop over a ramp
        // never decreases except at the wrap.
        let falls = l.windows(2).skip(64).filter(|w| w[1] < w[0]).count();
        assert!(falls > 500, "only {falls} falling samples — did it turn?");
    }

    /// Reverse plays the span backwards, and it is the SPAN that is
    /// reversed rather than the file.
    #[test]
    fn reverse_walks_the_span_backwards() {
        let m = material(1_000, |i| i as f32 / 1_000.0);
        let mut p = transparent();
        p.reverse = 1.0;
        p.start = 0.25;
        p.end = 0.75;
        p.amp_decay_ms = 16_000.0;
        let mut v = bank(p, m);
        v.note_on(60, 127, 0);
        let (l, _) = run(&mut v, 600, BLOCK);
        assert!(l[20] > 0.7, "reverse started at {} not near 0.75", l[20]);
        assert!(l[400] < l[100], "it did not run downhill");
    }

    // ---------------------------------------------------------- slicing ---

    fn grid(frames: u64, count: usize) -> Vec<u64> {
        (0..count)
            .map(|i| frames * i as u64 / count as u64)
            .collect()
    }

    /// A note selects a slice, and each slice reads its own span.
    #[test]
    fn each_note_reads_its_own_slice() {
        let m = material(1_600, |i| i as f32 / 1_600.0);
        let mut p = transparent();
        p.mode = sp::MODE_SLICE;
        p.choke = 0.0;
        let mut v = bank(p, m);
        v.set_slices(&grid(1_600, 4));

        for slice in 0..4u8 {
            let mut v = {
                let mut fresh = bank(p, tone(1_600));
                fresh.material = material(1_600, |i| i as f32 / 1_600.0);
                fresh.set_slices(&grid(1_600, 4));
                fresh
            };
            v.plock(sp::SLICE, Some(f32::from(slice) + 1.0));
            v.note_on(p.root.round() as u8, 127, 0);
            let (l, _) = run(&mut v, 64, BLOCK);
            let want = f32::from(slice) * 0.25;
            assert!(
                (l[20] - want).abs() < 0.02,
                "slice {slice} started at {} not {want}",
                l[20]
            );
        }
    }

    /// A note past the last slice sounds NOTHING. Not the last slice
    /// again: a clamp turns a wrong note into a right-sounding one and
    /// hides the mistake.
    #[test]
    fn a_note_past_the_last_slice_is_silent() {
        let mut p = transparent();
        p.mode = sp::MODE_SLICE;
        let mut v = bank(p, tone(1_600));
        v.set_slices(&grid(1_600, 4));
        v.plock(sp::SLICE, Some(10.0));
        v.note_on(p.root.round() as u8, 127, 0);
        let (l, _) = run(&mut v, 256, BLOCK);
        assert!(l.iter().all(|s| *s == 0.0), "a slice off the end sounded");
        assert!(!v.any_active());
    }

    /// Choke: a new slice takes the last one away rather than stacking on
    /// it.
    #[test]
    fn choke_takes_the_previous_slice_away() {
        let mut p = transparent();
        p.mode = sp::MODE_SLICE;
        p.choke = 1.0;
        p.fade_out_ms = 1.0;
        let mut v = bank(p, tone(1_600));
        v.set_slices(&grid(1_600, 4));
        v.plock(sp::SLICE, Some(1.0));
        v.note_on(p.root.round() as u8, 127, 0);
        let _ = run(&mut v, 64, BLOCK);
        v.plock(sp::SLICE, Some(2.0));
        v.note_on(p.root.round() as u8, 127, 1);
        let _ = run(&mut v, 256, BLOCK);
        let sounding = (0..VOICES).filter(|i| v.voices[*i].active).count();
        assert_eq!(sounding, 1, "choke left {sounding} voices sounding");
    }

    /// With choke off, slices stack — which is the other half of the
    /// switch actually doing something.
    #[test]
    fn without_choke_slices_stack() {
        let mut p = transparent();
        p.mode = sp::MODE_SLICE;
        p.choke = 0.0;
        let mut v = bank(p, tone(1_600));
        v.set_slices(&grid(1_600, 4));
        v.plock(sp::SLICE, Some(1.0));
        v.note_on(p.root.round() as u8, 127, 0);
        v.plock(sp::SLICE, Some(2.0));
        v.note_on(p.root.round() as u8, 127, 1);
        let _ = run(&mut v, 64, BLOCK);
        let sounding = (0..VOICES).filter(|i| v.voices[*i].active).count();
        assert_eq!(sounding, 2);
    }

    // ------------------------------------------------------------- gates ---

    /// A one-shot ignores note-off; a classic note does not.
    #[test]
    fn one_shot_ignores_the_gate_going_down() {
        for (mode, still_gated) in [(sp::MODE_CLASSIC, false), (sp::MODE_ONE_SHOT, true)] {
            let mut p = transparent();
            p.mode = mode;
            p.amp_decay_ms = 16_000.0;
            let mut v = bank(p, tone(16_000));
            v.note_on(60, 127, 0);
            let _ = run(&mut v, 128, BLOCK);
            v.note_off(60);
            assert_eq!(v.voices[0].gated, still_gated, "mode {mode}");
        }
    }

    /// The transport moving is the one thing that silences a one-shot.
    #[test]
    fn a_discontinuity_silences_even_a_one_shot() {
        let mut p = transparent();
        p.mode = sp::MODE_ONE_SHOT;
        let mut v = bank(p, tone(16_000));
        v.note_on(60, 127, 0);
        let _ = run(&mut v, 128, BLOCK);
        assert!(v.any_active());
        v.all_sound_off();
        assert!(!v.any_active());
        let (l, _) = run(&mut v, 256, BLOCK);
        assert!(l.iter().all(|s| *s == 0.0));
    }

    /// A ninth note steals the oldest, and the seam does not step.
    #[test]
    fn a_ninth_note_steals_without_a_step() {
        let mut p = transparent();
        p.amp_decay_ms = 16_000.0;
        p.fade_in_ms = 2.0;
        let mut v = bank(p, tone(16_000));
        for i in 0..VOICES as u8 {
            v.note_on(50 + i, 100, u64::from(i));
        }
        let _ = run(&mut v, 512, BLOCK);
        v.note_on(70, 100, 99);
        let (l, _) = run(&mut v, 512, BLOCK);
        let biggest = l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(biggest < 0.5, "the steal stepped by {biggest}");
        assert_eq!((0..VOICES).filter(|i| v.voices[*i].active).count(), VOICES);
    }

    // ------------------------------------------------------- modulation ---

    /// Zero depth is bit-identical to the envelope not existing.
    #[test]
    fn a_mod_depth_of_zero_changes_nothing() {
        let m = tone(4_000);
        let mut a = transparent();
        a.cutoff_hz = 2_000.0;
        a.mod_depth = 0.0;
        a.mod_dest = sp::DEST_CUTOFF;

        let mut with = bank(a, m.clone());
        with.note_on(60, 127, 0);
        let (x, _) = run(&mut with, 2_048, BLOCK);

        let mut b = a;
        b.mod_dest = sp::DEST_PAN;
        let mut without = bank(b, m);
        without.note_on(60, 127, 0);
        let (y, _) = run(&mut without, 2_048, BLOCK);

        assert_eq!(x, y, "a dead wire was audible");
    }

    /// Every destination does SOMETHING, and none of them produces a
    /// number that is not a number. The cheapest possible guard against a
    /// destination that was added to the list and never wired.
    #[test]
    fn every_destination_is_wired() {
        let m = tone(8_000);
        let mut base = transparent();
        base.cutoff_hz = 1_000.0;
        base.rate_hz = 8_000.0;
        base.drive = 0.3;
        base.amp_decay_ms = 16_000.0;

        let mut flat = base;
        flat.mod_depth = 0.0;
        let mut quiet = bank(flat, m.clone());
        quiet.note_on(60, 100, 0);
        let (reference, _) = run(&mut quiet, 2_048, BLOCK);

        for dest in [
            sp::DEST_CUTOFF,
            sp::DEST_PITCH,
            sp::DEST_START,
            sp::DEST_RATE,
            sp::DEST_DRIVE,
            sp::DEST_PAN,
        ] {
            let mut p = base;
            p.mod_dest = dest;
            p.mod_depth = 0.9;
            p.mod_decay_ms = 200.0;
            let mut v = bank(p, m.clone());
            v.note_on(60, 100, 0);
            let (l, r) = run(&mut v, 2_048, BLOCK);
            assert!(l.iter().all(|s| s.is_finite()), "dest {dest}: left");
            assert!(r.iter().all(|s| s.is_finite()), "dest {dest}: right");
            let moved = l
                .iter()
                .zip(reference.iter())
                .any(|(a, b)| (a - b).abs() > 1e-6);
            assert!(moved, "destination {dest} did nothing");
        }
    }

    /// Velocity at zero means every note is the same level; at one it
    /// tracks.
    #[test]
    fn velocity_tracking_is_a_knob() {
        let m = tone(4_000);
        let peak = |p: SamplerParams, vel: u8| {
            let mut v = bank(p, m.clone());
            v.note_on(60, vel, 0);
            let (l, _) = run(&mut v, 1_024, BLOCK);
            l.iter().fold(0.0f32, |a, s| a.max(s.abs()))
        };
        let mut off = transparent();
        off.velocity = 0.0;
        assert!((peak(off, 127) - peak(off, 32)).abs() < 1e-6);

        let mut on = transparent();
        on.velocity = 1.0;
        assert!(peak(on, 32) < peak(on, 127) * 0.5);
    }

    // ------------------------------------------------------------- dirt ---

    /// The bottom of the drive and converter ranges is a wire, so the
    /// colour can be measured against something.
    #[test]
    fn the_dirt_stages_have_an_exact_bottom() {
        let m = tone(4_000);
        let mut clean = transparent();
        clean.cutoff_hz = 20_000.0;
        let mut a = bank(clean, m.clone());
        a.note_on(60, 127, 0);
        let (x, _) = run(&mut a, 1_024, BLOCK);
        for i in 16..1_024 {
            assert_eq!(x[i], m.channel(0)[i], "not a wire at {i}");
        }
    }

    /// And the defaults are NOT a wire. This is the thesis, as a test: a
    /// sampler out of the box has a sound.
    #[test]
    fn the_defaults_are_not_a_wire() {
        let m = tone(4_000);
        let mut p = SamplerParams::default();
        p.fade_in_ms = 0.0;
        let mut v = bank(p, m.clone());
        v.note_on(60, 127, 0);
        let (l, _) = run(&mut v, 1_024, BLOCK);
        let difference = l
            .iter()
            .skip(64)
            .zip(m.channel(0).iter().skip(64))
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            difference > 1e-4,
            "the defaults reproduced the file exactly, which is not the device we said we were building"
        );
    }

    /// A knob turned while a note sounds is heard on the NEXT note, and a
    /// plock restore comes back to it — the trait's contract for `None`.
    #[test]
    fn a_plock_restores_to_the_live_knob_and_not_a_snapshot() {
        let mut v = bank(transparent(), tone(1_000));
        v.set_param(sp::CUTOFF, 5_000.0);
        v.plock(sp::CUTOFF, Some(300.0));
        assert_eq!(v.live.cutoff_hz, 300.0);
        // The knob moves while the lock is in force.
        v.set_param(sp::CUTOFF, 9_000.0);
        v.plock(sp::CUTOFF, None);
        assert_eq!(v.live.cutoff_hz, 9_000.0, "restore went to a snapshot");
        assert_eq!(v.base.cutoff_hz, 9_000.0);
    }

    /// Every row in the table round-trips through the params struct.
    /// The cheapest possible guard against a row that was added to the
    /// table and never given a field.
    #[test]
    fn every_table_row_is_readable_and_writable() {
        let mut p = SamplerParams::default();
        for def in sp::TABLE {
            let mid = (def.min + def.max) * 0.5;
            p.set(def.id, mid);
            let got = p.get(def.id);
            assert_eq!(
                got,
                Some(mid),
                "row {} ({}) did not round-trip",
                def.id,
                def.name
            );
        }
        assert_eq!(
            sp::TABLE.len(),
            sp::MIN_GAP as usize + 1,
            "the parameter ids remain contiguous and old ids are stable"
        );
    }

    /// Defaults come from the table and from nowhere else.
    #[test]
    fn defaults_come_from_the_table() {
        let p = SamplerParams::default();
        for def in sp::TABLE {
            assert_eq!(
                p.get(def.id),
                Some(def.default),
                "row {} ({}) disagreed with the table",
                def.id,
                def.name
            );
        }
    }
}

#[cfg(test)]
mod advanced_tests;
