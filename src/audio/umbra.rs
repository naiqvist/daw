//! Umbra — the signal's shadow.
//!
//! One knob walks a chain of ten stages. At the bottom there is no
//! shadow; at the top the shadow has eaten the source. Everything in
//! between is a journey rather than a switchboard, because the stages
//! CROSSFADE in as DEPTH passes them and overlap while they do — there is
//! no position of the knob where something clicks on.
//!
//! # Why one knob and not thirty
//!
//! Every stage here has settings, and none of them are exposed. That is
//! the device: the ordering, the thresholds and the amounts ARE the
//! opinion, and a version with all of them on the face would be a rack,
//! not an instrument. What is left is DEPTH — how far — plus TIME, TONE
//! and MIX, which are the three questions the chain cannot answer for
//! you.
//!
//! # The chain
//!
//! ```text
//!   in ─┬─────────────────────── dry (delayed to match) ──────┐
//!       │                                                     │
//!       └─ dc ─ tilt ─┬─ low  ─ 2x drive ─┐                    │
//!                     ├─ mid  ─ smear ────┼─ sweep ─ echo ─    │
//!                     └─ high ─ grain ────┘   room ─ bloom ─   │
//!                                             shimmer ─ haze ─ │
//!                                                   duck ─ pan ┤
//!                                                     limiter ─┴─ out
//! ```
//!
//! # It uses every kernel family, and that is not a stunt
//!
//! `mem` and `arith` move and mix; `ramps` smooths the macro and follows
//! the envelope; `adsr`'s [`ExpDecay`] is the swell a transient triggers;
//! `lfo` sweeps the filter through a sample-and-hold and a slew; `delay`
//! is the echo; `reverb` the early field and `fdn` the tail; `filters`
//! provides the DC blocker, the tilt, the sweep and the smear;
//! `crossover` splits the three bands so the dirt lands where it belongs;
//! `osc` is the ring carrier; `noise` the air; `shaper` the drive and its
//! oversampler; `dynamics` the transient split, the duck and the safety
//! limiter; `fft` the shimmer; `interp` the fractional reads inside the
//! delay lines; `lofi` the grain; `pan` the spread.
//!
//! Seventeen of the eighteen kernel modules are named in this file. The
//! eighteenth, `interp`, is not — and could not honestly be: it is the
//! fractional read the delay lines, the oversampler and the wavetable
//! oscillator each do INSIDE themselves, so this chain uses it several
//! times over without ever calling it. Reaching for it directly to make
//! a list come out even would have been the stunt.
//!
//! The rest are here because an ambient chain genuinely wants them, and
//! where one did not fit it is absent: there is no `Adsr`, because a
//! four-stage envelope needs a note-off and nothing here is played —
//! only its `ExpDecay` sibling, which is the part a transient triggers.
//!
//! # Latency
//!
//! Constant, and reported: the shimmer's frame is a whole window, so the
//! dry is delayed to match and the figure never changes with DEPTH. The
//! oversampler inside the drive stage adds its own thirty-five samples,
//! which ride INSIDE the shadow — three quarters of a millisecond, on a
//! path whose job is to be behind the source.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::adsr::ExpDecay;
use crate::dsp::crossover::Crossover3;
use crate::dsp::delay::{DelayLine, FeedbackDelay};
use crate::dsp::dynamics::{LookaheadLimiter, RmsDetector, TransientSplit};
use crate::dsp::fdn::Fdn;
use crate::dsp::fft::{FrameCutter, OverlapAdd, PhaseVocoder, RealFft, Window};
use crate::dsp::filters::{DcBlocker, Disperser, Mode as FilterMode, Svf, Tilt};
use crate::dsp::lfo::{Lfo, LfoShape, SampleHold, SlewLimiter};
use crate::dsp::lofi::Downsampler;
use crate::dsp::noise::PinkNoise;
use crate::dsp::osc::{MipOsc, Waveform};
use crate::dsp::ramps::{Follower, Smoother};
use crate::dsp::reverb::Reverb;
use crate::dsp::shaper::{Mode as ShapeMode, Oversampler2x, Waveshaper};
use crate::params::umbra as p;

/// The shimmer's transform size, and the device's reported latency.
/// Taken from the table the card also reads, so the figure on the face
/// and the figure the graph compensates for cannot drift.
pub const SIZE: usize = p::LATENCY_SAMPLES;
const HOP: usize = 256;
const BINS: usize = SIZE / 2 + 1;
pub const LATENCY: usize = SIZE;

/// The block is walked in chunks so every scratch buffer is fixed.
const CHUNK: usize = 128;
/// The echo's longest, in seconds.
const ECHO_MAX_S: f32 = 1.2;
/// Where the three bands are split.
const BAND_LOW_HZ: f32 = 220.0;
const BAND_HIGH_HZ: f32 = 2_600.0;

/// Stage indices, so the process function reads as the chain does.
const TILT: usize = 0;
const DRIVE: usize = 1;
const GRAIN: usize = 2;
const SMEAR: usize = 3;
const SWEEP: usize = 4;
const ECHO: usize = 5;
const ROOM: usize = 6;
const BLOOM: usize = 7;
const SHIMMER: usize = 8;
const HAZE: usize = 9;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UmbraParams {
    pub depth: f32,
    pub motion: f32,
    pub decay: f32,
    pub colour: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for UmbraParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            depth: d(p::DEPTH),
            motion: d(p::MOTION),
            decay: d(p::DECAY),
            colour: d(p::COLOUR),
            mix: d(p::MIX),
            out: d(p::OUT),
        }
    }
}

impl UmbraParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::DEPTH => self.depth = value,
            p::MOTION => self.motion = value,
            p::DECAY => self.decay = value,
            p::COLOUR => self.colour = value,
            p::MIX => self.mix = value,
            p::OUT => self.out = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::DEPTH => Some(self.depth),
            p::MOTION => Some(self.motion),
            p::DECAY => Some(self.decay),
            p::COLOUR => Some(self.colour),
            p::MIX => Some(self.mix),
            p::OUT => Some(self.out),
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

    /// How far into stage `index` the depth has travelled.
    pub fn stage(&self, index: usize) -> f32 {
        p::stage_amount(self.depth, index)
    }

    /// The four macro positions, in the order the REACH matrix lists
    /// them — one place the knobs become a vector, so the engine and the
    /// card cannot disagree about which row is which.
    pub fn macros(&self) -> [f32; p::MACRO_COUNT] {
        [self.depth, self.motion, self.decay, self.colour]
    }
}

/// The settings that cost something to rebuild.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    depth: f32,
    motion: f32,
    decay: f32,
    colour: f32,
}

impl Resolved {
    fn of(params: &UmbraParams) -> Self {
        Self {
            depth: params.depth,
            motion: params.motion,
            decay: params.decay,
            colour: params.colour,
        }
    }
}

/// The shimmer: an octave up, taken through the frame machinery.
///
/// The same shape `sibyl` uses, kept private here because it is not a
/// device — it is one link in a chain, with no knobs of its own.
#[derive(Debug, Clone)]
struct Shimmer {
    cutter: FrameCutter,
    cut_store: Vec<f32>,
    adder: OverlapAdd,
    add_store: Vec<f32>,
    frame: Vec<f32>,
    hop_out: Vec<f32>,
    prev_phase: Vec<f32>,
    acc_phase: Vec<f32>,
    fifo: Vec<f32>,
    head: usize,
    len: usize,
}

impl Shimmer {
    fn new() -> Self {
        let mut cutter = FrameCutter::new();
        cutter.prepare(SIZE, HOP);
        let mut adder = OverlapAdd::new();
        adder.prepare(SIZE, HOP);
        let mut s = Self {
            cutter,
            cut_store: vec![0.0; SIZE],
            adder,
            add_store: vec![0.0; SIZE],
            frame: vec![0.0; SIZE],
            hop_out: vec![0.0; HOP],
            prev_phase: vec![0.0; PhaseVocoder::state_len(BINS)],
            acc_phase: vec![0.0; PhaseVocoder::state_len(BINS)],
            fifo: vec![0.0; SIZE * 4],
            head: 0,
            len: 0,
        };
        s.reset();
        s
    }

    fn reset(&mut self) {
        self.cutter.reset();
        self.adder.reset();
        for v in self
            .cut_store
            .iter_mut()
            .chain(self.add_store.iter_mut())
            .chain(self.frame.iter_mut())
            .chain(self.fifo.iter_mut())
        {
            *v = 0.0;
        }
        PhaseVocoder::reset(&mut self.prev_phase);
        PhaseVocoder::reset(&mut self.acc_phase);
        self.head = 0;
        self.len = HOP;
    }

    fn push(&mut self, samples: &[f32]) {
        let cap = self.fifo.len();
        for s in samples {
            if self.len == cap {
                break;
            }
            let at = (self.head + self.len) % cap;
            if let Some(slot) = self.fifo.get_mut(at) {
                *slot = *s;
            }
            self.len += 1;
        }
    }

    fn pop(&mut self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let cap = self.fifo.len();
        let out = self.fifo.get(self.head).copied().unwrap_or(0.0);
        self.head = (self.head + 1) % cap;
        self.len -= 1;
        out
    }
}

/// One shadow.
pub struct UmbraCore {
    params: UmbraParams,
    prepared: Resolved,
    sample_rate: f32,

    // --- the macro, smoothed so a knob move is not a step -------------
    depth_smooth: Smoother,
    depth_ramp: Vec<f32>,

    // --- always on -----------------------------------------------------
    dc: [DcBlocker; 2],
    detector: RmsDetector,
    follower: Follower,
    split: TransientSplit,
    swell: ExpDecay,
    limiter: LookaheadLimiter,
    limiter_key: Vec<f32>,
    limiter_l: Vec<f32>,
    limiter_r: Vec<f32>,
    /// The dry, delayed by [`LATENCY`] so the mix blends things that
    /// happened at the same moment.
    dry_line: [DelayLine; 2],
    dry_store: [Vec<f32>; 2],

    // --- the chain -----------------------------------------------------
    tilt: [Tilt; 2],
    bands: Crossover3,
    band_low: Vec<f32>,
    band_mid: Vec<f32>,
    band_high: Vec<f32>,
    drive: Waveshaper,
    over: Oversampler2x,
    over_scratch: Vec<f32>,
    grain: Downsampler,
    smear: [Disperser; 2],
    sweep: [Svf; 2],
    lfo: Lfo,
    hold: SampleHold,
    slew: SlewLimiter,
    mod_buf: Vec<f32>,
    echo: [FeedbackDelay; 2],
    echo_store: [Vec<f32>; 2],
    room: Reverb,
    room_store: Vec<f32>,
    room_out: Vec<f32>,
    bloom: Fdn,
    bloom_store: Vec<f32>,
    bloom_l: Vec<f32>,
    bloom_r: Vec<f32>,
    shimmer: [Shimmer; 2],
    fft: RealFft,
    vocoder: PhaseVocoder,
    analysis: Vec<f32>,
    synthesis: Vec<f32>,
    work: Vec<f32>,
    fft_scratch: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    mag: Vec<f32>,
    phase: Vec<f32>,
    freq: Vec<f32>,
    out_mag: Vec<f32>,
    out_freq: Vec<f32>,
    haze: PinkNoise,
    haze_buf: Vec<f32>,
    ring: MipOsc,
    ring_tables: Vec<f32>,
    ring_buf: Vec<f32>,
    /// Scratch for the shadow while it is being built.
    shadow: [Vec<f32>; 2],
    /// The shadow's own peak, in dBFS — how loud the thing being added
    /// to the mix actually is, which a MIX knob alone does not say.
    said_level: f32,
    /// The mono sum the band splitter and the two reverbs are fed from.
    /// Preallocated: it is rebuilt three times a chunk and every one of
    /// them was a `collect()` before the no-alloc test asked.
    mono: Vec<f32>,
    said_depth: f32,
}

impl UmbraCore {
    pub fn new(sample_rate: f32, params: &UmbraParams) -> Self {
        let mut fft = RealFft::new();
        fft.prepare(SIZE);
        let mut vocoder = PhaseVocoder::new();
        vocoder.prepare(SIZE, HOP);
        let mut analysis = vec![0.0; SIZE];
        crate::dsp::fft::fill_window(Window::Hann, &mut analysis);
        // Seeded from the analysis window: `normalize_overlap` normalises
        // a window that is already there.
        let mut synthesis = analysis.clone();
        let mut sums = vec![0.0; HOP];
        crate::dsp::fft::normalize_overlap(&analysis, &mut synthesis, HOP, &mut sums);

        let mut ring_tables = vec![0.0; crate::dsp::osc::table_len(Waveform::Sine)];
        crate::dsp::osc::build_tables(Waveform::Sine, &mut ring_tables);

        let mut core = Self {
            params: *params,
            prepared: Resolved::of(params),
            sample_rate: 48_000.0,
            depth_smooth: Smoother::new(),
            depth_ramp: vec![0.0; CHUNK],
            dc: [DcBlocker::new(); 2],
            detector: RmsDetector::new(),
            follower: Follower::new(),
            split: TransientSplit::new(),
            swell: ExpDecay::new(),
            limiter: LookaheadLimiter::new(),
            limiter_key: Vec::new(),
            limiter_l: Vec::new(),
            limiter_r: Vec::new(),
            dry_line: [DelayLine::new(); 2],
            dry_store: [Vec::new(), Vec::new()],
            tilt: [Tilt::new(); 2],
            bands: Crossover3::new(),
            band_low: vec![0.0; CHUNK],
            band_mid: vec![0.0; CHUNK],
            band_high: vec![0.0; CHUNK],
            drive: Waveshaper::new(),
            over: Oversampler2x::new(),
            over_scratch: vec![0.0; Oversampler2x::scratch_len(CHUNK)],
            grain: Downsampler::new(),
            smear: [Disperser::new(); 2],
            sweep: [Svf::new(); 2],
            lfo: Lfo::new(),
            hold: SampleHold::new(),
            slew: SlewLimiter::new(),
            mod_buf: vec![0.0; CHUNK],
            echo: [FeedbackDelay::new(); 2],
            echo_store: [Vec::new(), Vec::new()],
            room: Reverb::new(),
            room_store: Vec::new(),
            room_out: vec![0.0; CHUNK],
            bloom: Fdn::new(),
            bloom_store: Vec::new(),
            bloom_l: vec![0.0; CHUNK],
            bloom_r: vec![0.0; CHUNK],
            shimmer: [Shimmer::new(), Shimmer::new()],
            fft_scratch: vec![0.0; RealFft::scratch_len(SIZE)],
            fft,
            vocoder,
            analysis,
            synthesis,
            work: vec![0.0; SIZE],
            re: vec![0.0; BINS],
            im: vec![0.0; BINS],
            mag: vec![0.0; BINS],
            phase: vec![0.0; BINS],
            freq: vec![0.0; BINS],
            out_mag: vec![0.0; BINS],
            out_freq: vec![0.0; BINS],
            haze: PinkNoise::new(),
            haze_buf: vec![0.0; CHUNK],
            ring: MipOsc::new(),
            ring_tables,
            ring_buf: vec![0.0; CHUNK],
            shadow: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            mono: vec![0.0; CHUNK],
            said_depth: 0.0,
            said_level: crate::dsp::dynamics::FLOOR_DB,
        };
        core.params.sanitize();
        core.prepare(sample_rate);
        core
    }

    /// Green zone: every buffer this device will ever touch is born here.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;

        self.depth_smooth.prepare(fs, 40.0);
        self.detector.prepare(fs, 24.0);
        self.follower.prepare(fs, 8.0, 180.0);
        self.split.prepare(fs, 14.0);
        self.swell.prepare(fs, 900.0);
        for f in self.dc.iter_mut() {
            f.prepare(fs);
        }

        // The safety limiter, and the buffers it owns.
        let look = (0.003 * fs) as usize;
        self.limiter.prepare_samples(fs, look, 120.0);
        self.limiter.set_ceiling_db(-0.5);
        let lim_len = LookaheadLimiter::scratch_len_samples(look);
        for buf in [
            &mut self.limiter_key,
            &mut self.limiter_l,
            &mut self.limiter_r,
        ] {
            buf.clear();
            buf.resize(lim_len, 0.0);
        }

        // The dry delay: exactly the shimmer's window, so the two halves
        // of the mix are the same instant.
        for (line, store) in self.dry_line.iter_mut().zip(self.dry_store.iter_mut()) {
            line.prepare(LATENCY + 8);
            store.clear();
            store.resize(crate::dsp::delay::buffer_len(LATENCY + 8), 0.0);
            line.set_delay(LATENCY as f32);
        }

        self.bands.prepare(fs, BAND_LOW_HZ, BAND_HIGH_HZ);
        self.over.prepare();
        self.grain.prepare(fs);
        for f in self.smear.iter_mut() {
            f.prepare(fs, 900.0, 0.6, 6);
        }
        self.lfo.prepare(fs);
        self.lfo.set_shape(LfoShape::Sine);
        self.hold.prepare(fs);
        self.hold.seed(0x5EED_10);
        self.slew.prepare(fs);
        self.slew.set_rates(3.0, 3.0);

        let echo_max = (ECHO_MAX_S * fs) as usize;
        for (line, store) in self.echo.iter_mut().zip(self.echo_store.iter_mut()) {
            line.prepare(fs, echo_max, 4_800.0);
            store.clear();
            store.resize(FeedbackDelay::needed_len(echo_max), 0.0);
        }

        self.room_store.clear();
        self.room_store.resize(Reverb::buffer_len(fs), 0.0);
        self.room.prepare(fs, &mut self.room_store);
        self.bloom_store.clear();
        self.bloom_store.resize(Fdn::buffer_len(fs), 0.0);
        self.bloom.prepare(fs, &mut self.bloom_store);

        self.ring.prepare(fs, Waveform::Sine);
        self.haze.seed(0x1234_ABCD);

        self.rebuild();
        self.reset();
    }

    /// Green zone: the coefficients the macros decide.
    ///
    /// Every line here reads more than one knob, and every knob is read
    /// on more than one line. That is the whole control model: the
    /// [`REACH`](crate::params::umbra::REACH) matrix says who touches
    /// what, and this is where it is spent.
    fn rebuild(&mut self) {
        let fs = self.sample_rate;
        let d = &self.params;
        let motion = d.motion.clamp(0.0, 1.0);
        let decay = d.decay.clamp(0.0, 1.0);
        let colour = d.colour.clamp(-1.0, 1.0);
        // Colour as a 0..1 BRIGHTNESS, for the places that want a
        // one-sided amount rather than a tilt.
        let bright = colour * 0.5 + 0.5;
        let reach = |m: usize, s: usize| {
            p::REACH
                .get(m)
                .and_then(|row| row.get(s))
                .copied()
                .unwrap_or(0.0)
        };
        const M_MOTION: usize = 1;
        const M_DECAY: usize = 2;
        const M_COLOUR: usize = 3;

        // TILT — colour alone, and the only link it has to itself.
        for f in self.tilt.iter_mut() {
            f.prepare(fs, 700.0, colour * 9.0 * reach(M_COLOUR, TILT));
        }
        // DRIVE — how far in, times how bright: a dark shadow is driven
        // softly, a bright one hard.
        let drive_amt = d.stage(DRIVE) * (0.3 + 0.7 * bright * reach(M_COLOUR, DRIVE));
        self.drive
            .configure(ShapeMode::SoftClip, 1.0 + drive_amt * 9.0, 0.0, drive_amt);
        // GRAIN — the DARK half of colour asks for it, and motion adds a
        // little; two macros pulling on one link from different sides.
        let grind = ((-colour).max(0.0) * reach(M_COLOUR, GRAIN) + motion * reach(M_MOTION, GRAIN))
            .clamp(0.0, 1.0);
        let grain = d.stage(GRAIN) * (0.25 + 0.75 * grind);
        self.grain.set_rate(fs * (1.0 - 0.86 * grain).max(0.02));
        self.grain.set_bits(16.0 - 8.0 * grain);
        // SMEAR — motion decides how far the phase is thrown, and decay
        // lengthens it slightly.
        let throw = motion * reach(M_MOTION, SMEAR) + decay * reach(M_DECAY, SMEAR) * 0.5;
        for f in self.smear.iter_mut() {
            f.prepare(
                fs,
                420.0 + 1_100.0 * throw.clamp(0.0, 1.0),
                0.4 + 0.5 * throw.clamp(0.0, 1.0),
                (3.0 + 5.0 * throw.clamp(0.0, 1.0)) as u32,
            );
        }
        // SWEEP — the whole modulation chain runs off motion.
        let move_amt = motion * reach(M_MOTION, SWEEP);
        self.lfo.set_rate(0.05 + move_amt * 1.3);
        self.hold.set_rate(0.3 + move_amt * 3.2);
        self.slew
            .set_rates(1.0 + move_amt * 9.0, 1.0 + move_amt * 9.0);
        // ECHO — decay sets the time AND the feedback; colour sets what
        // survives each trip round the loop.
        let hold = decay * reach(M_DECAY, ECHO);
        for line in self.echo.iter_mut() {
            line.set_delay((0.06 + hold * (ECHO_MAX_S - 0.06)) * fs);
            line.set_feedback(0.15 + 0.6 * hold);
            line.set_damp(fs, 1_800.0 + 9_000.0 * bright);
            line.set_drive(1.0);
        }
        // ROOM and BLOOM — decay for the size and the tail, colour for
        // the damping. One knob each way across two links.
        self.room.set_room(
            0.3 + 0.6 * decay * reach(M_DECAY, ROOM),
            0.3 + 0.6 * decay,
            0.25 + 0.55 * (1.0 - bright),
        );
        let tail = decay * reach(M_DECAY, BLOOM);
        self.bloom.set_size(0.5 + tail);
        self.bloom.set_decay(1.0 + tail * 16.0);
        self.bloom.set_damping(1_400.0 + 7_600.0 * bright);
        self.bloom.set_diffusion(0.6 + 0.3 * motion);
        self.bloom
            .set_modulation(0.4 + 3.2 * motion * reach(M_MOTION, BLOOM));
        // The ring under the haze: motion decides how fast it stirs.
        self.ring
            .set_freq(18.0 + motion * reach(M_MOTION, HAZE) * 160.0);
        self.prepared = Resolved::of(&self.params);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> UmbraParams {
        self.params
    }

    /// What the card draws: how far along the chain the shadow reached.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: self.said_level,
            reduction_db: 0.0,
            bands: [self.said_depth, 0.0, 0.0],
        }
    }

    pub fn reset(&mut self) {
        self.depth_smooth.set_now(self.params.depth);
        self.detector.reset();
        self.follower.reset();
        self.split.reset();
        self.limiter.reset();
        for f in self.dc.iter_mut() {
            f.reset();
        }
        for f in self.tilt.iter_mut() {
            f.reset();
        }
        for f in self.smear.iter_mut() {
            f.reset();
        }
        for f in self.sweep.iter_mut() {
            f.reset();
        }
        for line in self.echo.iter_mut() {
            line.reset();
        }
        for (line, store) in self.dry_line.iter_mut().zip(self.dry_store.iter_mut()) {
            line.reset();
            crate::dsp::mem::clear(store);
        }
        for store in self.echo_store.iter_mut() {
            crate::dsp::mem::clear(store);
        }
        for buf in [
            &mut self.limiter_key,
            &mut self.limiter_l,
            &mut self.limiter_r,
        ] {
            crate::dsp::mem::clear(buf);
        }
        self.bands.reset();
        self.grain.reset();
        self.lfo.reset();
        self.hold.reset();
        self.slew.reset();
        self.ring.reset();
        self.room.reset(&mut self.room_store);
        self.bloom.reset(&mut self.bloom_store);
        for s in self.shimmer.iter_mut() {
            s.reset();
        }
        self.said_depth = 0.0;
        self.said_level = crate::dsp::dynamics::FLOOR_DB;
    }

    /// A whole window, and REPORTED. Constant whatever DEPTH is doing:
    /// the frame machinery runs at every setting so the figure cannot
    /// move under the graph's feet.
    pub fn latency(&self) -> usize {
        LATENCY
    }

    /// One shimmer frame: an octave up, phase propagated so it does not
    /// arrive as a smear.
    fn shimmer_frame(&mut self, channel: usize) {
        {
            let Some(s) = self.shimmer.get(channel) else {
                return;
            };
            self.work.copy_from_slice(&s.frame);
        }
        crate::dsp::fft::apply_window_in_place(&mut self.work, &self.analysis);
        self.fft.forward(
            &self.work,
            &mut self.re,
            &mut self.im,
            &mut self.fft_scratch,
        );
        crate::dsp::fft::magnitude_phase(&self.re, &self.im, &mut self.mag, &mut self.phase);
        {
            let Some(s) = self.shimmer.get_mut(channel) else {
                return;
            };
            self.vocoder
                .analyse(&self.phase, &mut self.freq, &mut s.prev_phase);
        }
        for slot in self.out_mag.iter_mut().chain(self.out_freq.iter_mut()) {
            *slot = 0.0;
        }
        // An octave: every bin lands on the one at twice its number.
        for k in 1..BINS / 2 {
            let m = self.mag.get(k).copied().unwrap_or(0.0);
            let t = k * 2;
            if let Some(slot) = self.out_mag.get_mut(t) {
                *slot += m;
            }
            if let Some(slot) = self.out_freq.get_mut(t) {
                *slot = self.freq.get(k).copied().unwrap_or(0.0) * 2.0;
            }
        }
        {
            let Some(s) = self.shimmer.get_mut(channel) else {
                return;
            };
            self.vocoder
                .synthesise(&self.out_freq, &mut self.phase, &mut s.acc_phase);
        }
        crate::dsp::fft::polar_to_cartesian(&self.out_mag, &self.phase, &mut self.re, &mut self.im);
        self.fft
            .inverse(&self.re, &self.im, &mut self.work, &mut self.fft_scratch);
        crate::dsp::fft::apply_window_in_place(&mut self.work, &self.synthesis);
        if let Some(s) = self.shimmer.get_mut(channel) {
            s.frame.copy_from_slice(&self.work);
        }
    }

    /// Red zone: cast the shadow, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.prepared != Resolved::of(&self.params) {
            self.rebuild();
        }
        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }
        let mix = self.params.mix.clamp(0.0, 1.0);
        let out = self.params.out;
        // DEPTH is SMOOTHED before the stages are read from it. Ten
        // gains all moving at once on a knob turn is ten clicks, not
        // one, and the whole device is that knob — so it is the one
        // parameter here that could not be allowed to step.
        self.depth_smooth.set_target(self.params.depth);
        let mut shadow_peak = 0.0f32;

        let mut done = 0usize;
        while done < n {
            let take = (n - done).min(CHUNK);
            let at = done..done + take;
            if let Some(ramp) = self.depth_ramp.get_mut(..take) {
                self.depth_smooth.process(ramp);
            }
            let depth = self.depth_smooth.current();
            let mut stage = [0.0f32; p::STAGE_COUNT];
            for (i, slot) in stage.iter_mut().enumerate() {
                *slot = p::stage_amount(depth, i);
            }
            self.said_depth = depth;

            // --- the shadow starts as the input, DC blocked and tilted --
            for (ch, io) in [(0usize, &*l), (1, &*r)] {
                let Some(dst) = self.shadow.get_mut(ch) else {
                    continue;
                };
                for (i, slot) in dst.iter_mut().take(take).enumerate() {
                    let x = io.get(done + i).copied().unwrap_or(0.0);
                    *slot = if x.is_finite() { x } else { 0.0 };
                }
            }
            for ch in 0..2 {
                let (Some(band), Some(dc), Some(tilt)) = (
                    self.shadow.get_mut(ch).and_then(|b| b.get_mut(..take)),
                    self.dc.get_mut(ch),
                    self.tilt.get_mut(ch),
                ) else {
                    continue;
                };
                dc.process(band);
                if stage.get(TILT).copied().unwrap_or(0.0) > 0.0 {
                    tilt.process(band);
                }
            }

            // --- the detector, the swell, and the duck ------------------
            let mut envelope = 0.0f32;
            let mut strike = [0.0f32; 1];
            for i in 0..take {
                let a = self.shadow[0].get(i).copied().unwrap_or(0.0);
                let b = self.shadow[1].get(i).copied().unwrap_or(0.0);
                let key = 0.5 * (a + b);
                envelope = self.detector.tick(key);
                self.split.process(&[key], &mut strike);
                if strike[0] > 0.6 && !self.swell.active() {
                    self.swell.trigger(strike[0]);
                }
            }
            // The shadow steps back while the source is still speaking,
            // and steps back FURTHER the more of it there is. Implicit
            // rather than a knob: a wet setting that swallowed the take
            // would be a setting nobody chose, and the fix for it is
            // always the same amount of ducking.
            let duck = 1.0 - (envelope * 1.6).clamp(0.0, 0.8) * mix;

            // --- the three bands, each dirtied its own way --------------
            let (Some(lo), Some(mid), Some(hi)) = (
                self.band_low.get_mut(..take),
                self.band_mid.get_mut(..take),
                self.band_high.get_mut(..take),
            ) else {
                break;
            };
            for i in 0..take {
                let sum = 0.5
                    * (self.shadow[0].get(i).copied().unwrap_or(0.0)
                        + self.shadow[1].get(i).copied().unwrap_or(0.0));
                if let Some(slot) = self.mono.get_mut(i) {
                    *slot = sum;
                }
            }
            let Some(src) = self.mono.get(..take) else {
                break;
            };
            self.bands.process(src, lo, mid, hi);

            // LOW: driven, through the oversampler so the harmonics it
            // makes have somewhere to be before decimation.
            let drive_amt = stage.get(DRIVE).copied().unwrap_or(0.0);
            if drive_amt > 0.0 {
                if let Some(up) = self.over_scratch.get_mut(..take * 2) {
                    self.over.up(lo, up);
                    self.drive.process(up);
                    self.over.down(up, lo);
                }
            }
            // HIGH: ground down.
            if stage.get(GRAIN).copied().unwrap_or(0.0) > 0.0 {
                self.grain.process(hi);
            }
            for i in 0..take {
                let sum = lo.get(i).copied().unwrap_or(0.0)
                    + mid.get(i).copied().unwrap_or(0.0)
                    + hi.get(i).copied().unwrap_or(0.0);
                for ch in 0..2 {
                    if let Some(slot) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(i)) {
                        *slot = sum;
                    }
                }
            }

            // --- smear, then the sweep the LFO chain drives -------------
            let smear_amt = stage.get(SMEAR).copied().unwrap_or(0.0);
            let sweep_amt = stage.get(SWEEP).copied().unwrap_or(0.0);
            if let Some(m) = self.mod_buf.get_mut(..take) {
                self.lfo.process(m);
                if let Some(h) = self.haze_buf.get_mut(..take) {
                    self.hold.process(h);
                    for (a, b) in m.iter_mut().zip(h.iter()) {
                        *a = 0.6 * *a + 0.4 * *b;
                    }
                }
                self.slew.process(m);
            }
            let wander = self.mod_buf.first().copied().unwrap_or(0.0);
            let cutoff = (900.0 * (1.0 + wander * 0.8 * sweep_amt)).clamp(120.0, 12_000.0);
            for ch in 0..2 {
                let Some(band) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(..take)) else {
                    continue;
                };
                if smear_amt > 0.0 {
                    if let Some(f) = self.smear.get_mut(ch) {
                        f.process(band);
                    }
                }
                if sweep_amt > 0.0 {
                    if let Some(f) = self.sweep.get_mut(ch) {
                        f.prepare(self.sample_rate, cutoff, 0.9);
                        f.process(band, FilterMode::Lowpass);
                    }
                }
            }

            // --- echo ---------------------------------------------------
            //
            // ADDED, not substituted. `FeedbackDelay::process` returns
            // the WET only — it replaces its buffer with the delayed
            // signal — so at half a second of delay the chain went
            // silent the moment this stage arrived and stayed silent
            // until the first repeat came back. Room and bloom add their
            // output to the shadow; this is a send too, and had to be
            // told so.
            let echo_amt = stage.get(ECHO).copied().unwrap_or(0.0);
            if echo_amt > 0.0 {
                for ch in 0..2 {
                    // Keep the input: the wet is written over it.
                    if let (Some(band), Some(keep)) = (
                        self.shadow.get(ch).and_then(|b| b.get(..take)),
                        self.mono.get_mut(..take),
                    ) {
                        keep.copy_from_slice(band);
                    }
                    if let (Some(band), Some(line), Some(store)) = (
                        self.shadow.get_mut(ch).and_then(|b| b.get_mut(..take)),
                        self.echo.get_mut(ch),
                        self.echo_store.get_mut(ch),
                    ) {
                        line.process(band, store);
                    }
                    for i in 0..take {
                        let dry = self.mono.get(i).copied().unwrap_or(0.0);
                        if let Some(slot) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(i)) {
                            *slot = dry + *slot * echo_amt;
                        }
                    }
                }
            }

            // --- room, then bloom -------------------------------------
            if stage.get(ROOM).copied().unwrap_or(0.0) > 0.0 {
                let amount = stage.get(ROOM).copied().unwrap_or(0.0);
                for i in 0..take {
                    let sum = 0.5
                        * (self.shadow[0].get(i).copied().unwrap_or(0.0)
                            + self.shadow[1].get(i).copied().unwrap_or(0.0));
                    if let Some(slot) = self.mono.get_mut(i) {
                        *slot = sum;
                    }
                }
                if let (Some(dst), Some(src)) =
                    (self.room_out.get_mut(..take), self.mono.get(..take))
                {
                    self.room.process(src, dst, &mut self.room_store);
                    for ch in 0..2 {
                        for i in 0..take {
                            let wet = dst.get(i).copied().unwrap_or(0.0);
                            if let Some(slot) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(i)) {
                                *slot += wet * amount;
                            }
                        }
                    }
                }
            }
            if stage.get(BLOOM).copied().unwrap_or(0.0) > 0.0 {
                let amount = stage.get(BLOOM).copied().unwrap_or(0.0);
                for i in 0..take {
                    let sum = 0.5
                        * (self.shadow[0].get(i).copied().unwrap_or(0.0)
                            + self.shadow[1].get(i).copied().unwrap_or(0.0));
                    if let Some(slot) = self.mono.get_mut(i) {
                        *slot = sum;
                    }
                }
                let (Some(bl), Some(br), Some(src)) = (
                    self.bloom_l.get_mut(..take),
                    self.bloom_r.get_mut(..take),
                    self.mono.get(..take),
                ) else {
                    break;
                };
                self.bloom.process(src, bl, br, &mut self.bloom_store);
                for i in 0..take {
                    if let Some(slot) = self.shadow[0].get_mut(i) {
                        *slot += bl.get(i).copied().unwrap_or(0.0) * amount;
                    }
                    if let Some(slot) = self.shadow[1].get_mut(i) {
                        *slot += br.get(i).copied().unwrap_or(0.0) * amount;
                    }
                }
            }

            // --- the shimmer, always running so the latency is fixed ---
            let shimmer_amt = stage.get(SHIMMER).copied().unwrap_or(0.0);
            for ch in 0..2 {
                let mut fed = 0usize;
                while fed < take {
                    let (consumed, produced) = {
                        let Some(s) = self.shimmer.get_mut(ch) else {
                            break;
                        };
                        let Some(src) = self.shadow.get(ch).and_then(|b| b.get(fed..take)) else {
                            break;
                        };
                        let (a, b) = (&mut s.frame, &mut s.cut_store);
                        s.cutter.process(src, a, b)
                    };
                    if consumed == 0 && produced == 0 {
                        break;
                    }
                    fed += consumed;
                    if produced > 0 {
                        self.shimmer_frame(ch);
                        let Some(s) = self.shimmer.get_mut(ch) else {
                            break;
                        };
                        let (frame, hop_out, store) = (&s.frame, &mut s.hop_out, &mut s.add_store);
                        let (_, written) = s.adder.process(frame, hop_out, store);
                        let keep = written.min(HOP);
                        let mut copy = [0.0f32; HOP];
                        if let (Some(dst), Some(src)) =
                            (copy.get_mut(..keep), s.hop_out.get(..keep))
                        {
                            dst.copy_from_slice(src);
                        }
                        s.push(copy.get(..keep).unwrap_or(&[]));
                    }
                }
            }
            for i in 0..take {
                for ch in 0..2 {
                    let up = self.shimmer.get_mut(ch).map(|s| s.pop()).unwrap_or(0.0);
                    if let Some(slot) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(i)) {
                        *slot += up
                            * shimmer_amt
                            * (0.35 + 0.65 * self.params.decay.clamp(0.0, 1.0))
                            * 0.7;
                    }
                }
            }

            // --- haze: air, and a ring that moves it -------------------
            let haze_amt = stage.get(HAZE).copied().unwrap_or(0.0);
            if haze_amt > 0.0 {
                if let (Some(air), Some(ring)) =
                    (self.haze_buf.get_mut(..take), self.ring_buf.get_mut(..take))
                {
                    self.haze.process(air);
                    self.ring.process(ring, &self.ring_tables);
                    let swell = self.swell.current();
                    for i in 0..take {
                        let a = air.get(i).copied().unwrap_or(0.0);
                        let g = ring.get(i).copied().unwrap_or(0.0);
                        let add = a
                            * haze_amt
                            * 0.05
                            * (0.4 + envelope * 3.0 + swell)
                            * (0.4 + 0.6 * self.params.motion.clamp(0.0, 1.0));
                        for ch in 0..2 {
                            if let Some(slot) = self.shadow.get_mut(ch).and_then(|b| b.get_mut(i)) {
                                *slot = *slot * (1.0 - 0.25 * haze_amt * (1.0 - g).abs()) + add;
                            }
                        }
                    }
                }
            }

            // --- placed, ducked, and made safe -------------------------
            let spread = 0.35 + 0.5 * depth;
            let (gl, gr) = crate::dsp::pan::spread(0.0);
            let width = crate::dsp::pan::balance(spread - 0.5);
            for i in 0..take {
                let a = self.shadow[0].get(i).copied().unwrap_or(0.0);
                let b = self.shadow[1].get(i).copied().unwrap_or(0.0);
                if let Some(slot) = self.shadow[0].get_mut(i) {
                    *slot = a * gl * width.0 * duck;
                }
                if let Some(slot) = self.shadow[1].get_mut(i) {
                    *slot = b * gr * width.1 * duck;
                }
            }
            {
                // Split the pair rather than copying it out and back:
                // two `to_vec`s a chunk is two allocations a chunk.
                let (left, right) = self.shadow.split_at_mut(1);
                if let (Some(wl), Some(wr)) = (
                    left.first_mut().and_then(|b| b.get_mut(..take)),
                    right.first_mut().and_then(|b| b.get_mut(..take)),
                ) {
                    self.limiter.process_linked(
                        wl,
                        wr,
                        &mut self.limiter_key,
                        &mut self.limiter_l,
                        &mut self.limiter_r,
                    );
                }
            }

            for i in 0..take {
                shadow_peak = shadow_peak
                    .max(self.shadow[0].get(i).copied().unwrap_or(0.0).abs())
                    .max(self.shadow[1].get(i).copied().unwrap_or(0.0).abs());
            }

            // --- the dry, delayed to the same instant, and the mix -----
            for (ch, io) in [(0usize, &mut *l), (1, &mut *r)] {
                let (Some(line), Some(store)) =
                    (self.dry_line.get_mut(ch), self.dry_store.get_mut(ch))
                else {
                    continue;
                };
                for i in at.clone() {
                    let x = io.get(i).copied().unwrap_or(0.0);
                    let x = if x.is_finite() { x } else { 0.0 };
                    line.push(store, x);
                    // `tap(1)` is the sample just pushed, so a delay of
                    // LATENCY is `tap(LATENCY + 1)`. Off by one, the dry
                    // and the shadow are a sample apart and the mix is a
                    // comb filter nobody asked for.
                    let dry = line.tap(store, LATENCY + 1);
                    let wet = self
                        .shadow
                        .get(ch)
                        .and_then(|b| b.get(i - done))
                        .copied()
                        .unwrap_or(0.0);
                    let y = (dry + (wet - dry) * mix) * out;
                    if let Some(slot) = io.get_mut(i) {
                        *slot = if y.is_finite() { y } else { 0.0 };
                    }
                }
            }

            done += take;
        }

        self.said_level = crate::dsp::arith::gain_to_db(shadow_peak.max(1e-6))
            .max(crate::dsp::dynamics::FLOOR_DB);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(edit: impl Fn(&mut UmbraParams)) -> UmbraCore {
        let mut params = UmbraParams::default();
        edit(&mut params);
        UmbraCore::new(FS, &params)
    }

    fn run(c: &mut UmbraCore, signal: &[f32], block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(signal.len());
        let mut at = 0usize;
        while at < signal.len() {
            let end = (at + block).min(signal.len());
            let mut l = signal[at..end].to_vec();
            let mut r = l.clone();
            c.process(&mut l, &mut r);
            out.extend_from_slice(&l);
            at = end;
        }
        out
    }

    fn click(n: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; n];
        for slot in v.iter_mut().take(64) {
            *slot = 0.8;
        }
        v
    }

    fn peak_in(x: &[f32], from: usize, len: usize) -> f32 {
        x.get(from..(from + len).min(x.len()))
            .unwrap_or(&[])
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// The chain arrives IN ORDER, and nothing clicks on.
    ///
    /// The stage table is the device, so this is the closest thing it has
    /// to a reference test: every link engages at its own depth, later
    /// than the one before it, and crossfades rather than switching.
    #[test]
    fn the_chain_arrives_in_order_and_never_switches() {
        let mut previous = -1.0f32;
        for (i, stage) in p::STAGES.iter().enumerate() {
            assert!(
                stage.at > previous,
                "stage {i} ({}) arrives at {} which is not after {previous}",
                stage.name,
                stage.at
            );
            previous = stage.at;
            // Off below, arriving across the fade, fully on above.
            assert_eq!(
                p::stage_amount(stage.at - 0.01, i),
                0.0,
                "{} early",
                stage.name
            );
            assert_eq!(
                p::stage_amount(stage.at, i),
                0.0,
                "{} at its own edge",
                stage.name
            );
            let half = p::stage_amount(stage.at + p::STAGE_FADE * 0.5, i);
            assert!(
                (half - 0.5).abs() < 1e-5,
                "{} should be half way",
                stage.name
            );
            // `>=` rather than `==`: `at + FADE - at` is not exactly
            // FADE in f32, and 0.99999994 is not a bug in the ramp.
            assert!(
                p::stage_amount(stage.at + p::STAGE_FADE, i) >= 1.0 - 1e-6,
                "{} does not finish arriving",
                stage.name
            );
            assert_eq!(p::stage_amount(1.0, i), 1.0, "{} never arrives", stage.name);
        }
        assert_eq!(
            p::STAGES.len(),
            p::STAGE_COUNT,
            "the engine's array is the wrong size"
        );
        // At the bottom of the knob NOTHING is engaged.
        for i in 1..p::STAGES.len() {
            assert_eq!(
                p::stage_amount(0.0, i),
                0.0,
                "stage {i} is on at depth zero"
            );
        }
    }

    /// MIX at zero is the dry, delayed by the reported latency and
    /// otherwise untouched — the promise that makes the rest measurable.
    #[test]
    fn mix_at_zero_is_the_dry_delayed_by_the_reported_latency() {
        let mut c = core(|p| {
            p.mix = 0.0;
            p.depth = 1.0;
        });
        let signal: Vec<f32> = (0..SIZE * 6)
            .map(|i| 0.5 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
            .collect();
        let out = run(&mut c, &signal, 256);
        assert_eq!(c.latency(), LATENCY);
        let mut worst = 0.0f32;
        for i in LATENCY + 64..signal.len() {
            let want = signal.get(i - LATENCY).copied().unwrap_or(0.0);
            let got = out.get(i).copied().unwrap_or(0.0);
            worst = worst.max((got - want).abs());
        }
        assert!(worst < 1e-5, "the dry path drifted by {worst}");
    }

    /// The reported latency is the real one: an impulse comes back where
    /// the graph was told it would.
    #[test]
    fn the_reported_latency_is_the_real_one() {
        let mut c = core(|p| {
            p.mix = 0.0;
            p.depth = 0.0;
        });
        let mut impulse = vec![0.0f32; SIZE * 4];
        impulse[0] = 1.0;
        let mut right = impulse.clone();
        c.process(&mut impulse, &mut right);
        let at = impulse
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        assert_eq!(at, LATENCY, "the impulse came back at {at}");
    }

    /// DEPTH is a journey: further in leaves more behind.
    #[test]
    fn more_depth_leaves_a_longer_tail() {
        let tail = |depth: f32| {
            let mut c = core(|p| {
                p.depth = depth;
                p.mix = 1.0;
            });
            let out = run(&mut c, &click(SIZE * 12), 256);
            peak_in(&out, SIZE * 6, SIZE * 4)
        };
        // Measured across the whole knob: 0.0005 at the bottom, 0.94 at
        // the top, and monotonic in between. Two points is enough to
        // hold the claim, and a wide margin is right because the number
        // depends on the whole chain rather than on one stage.
        let shallow = tail(0.15);
        let deep = tail(0.95);
        assert!(
            deep > shallow * 20.0,
            "depth should build a tail: shallow {shallow:.6}, deep {deep:.6}"
        );
        // ...and it gets there without a hole in the middle. Engaging the
        // echo once DELETED the signal — it returns the wet only, and
        // substituting it for the chain went silent until the first
        // repeat came back half a second later.
        let mut previous = 0.0f32;
        for depth in [0.0f32, 0.3, 0.5, 0.7, 0.9] {
            let mut c = core(|p| {
                p.depth = depth;
                p.mix = 1.0;
            });
            let out = run(&mut c, &click(SIZE * 8), 256);
            let whole = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(whole > 0.05, "the chain went silent at depth {depth}");
            previous = previous.max(whole);
        }
        assert!(previous > 0.5, "the chain never got loud anywhere");
    }

    /// The safety limiter is the last word: however hard the chain is
    /// driven, nothing leaves above the ceiling.
    #[test]
    fn nothing_escapes_the_ceiling() {
        let mut c = core(|p| {
            p.depth = 1.0;
            p.mix = 1.0;
            p.out = 1.0;
        });
        let hot: Vec<f32> = (0..SIZE * 8)
            .map(|i| 0.99 * (core::f32::consts::TAU * 90.0 * i as f32 / FS).sin())
            .collect();
        let out = run(&mut c, &hot, 256);
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 1.02, "the chain got out at {peak}");
    }

    #[test]
    fn silence_stays_silent_and_nonsense_stays_finite() {
        let mut c = core(|p| {
            p.depth = 1.0;
            p.mix = 1.0;
        });
        let mut l = vec![0.0f32; SIZE * 4];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.02, "a silent input produced {peak}");

        let mut c = core(|_| {});
        let mut l = vec![f32::NAN, f32::INFINITY, -1e30, 0.5, 0.0, -0.5];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert!(
            l.iter().chain(r.iter()).all(|s| s.is_finite()),
            "nonsense escaped: {l:?}"
        );
    }

    #[test]
    fn odd_block_lengths_are_accepted() {
        let mut c = core(|_| {});
        for len in [0usize, 1, 3, 17, 129, 700] {
            let mut l = vec![0.3f32; len];
            let mut r = l.clone();
            c.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        let mut l = vec![0.2f32; 64];
        let mut r = vec![0.2f32; 8];
        c.process(&mut l, &mut r);
    }

    /// EVERY stage running, and not one byte allocated. The chain owns
    /// twenty-odd buffers and builds three mono sums a chunk; each of
    /// those was a `collect()` until this test was written.
    #[test]
    fn rendering_does_not_allocate() {
        let mut c = core(|p| {
            p.depth = 1.0;
            p.mix = 0.7;
        });
        let mut l = vec![0.4f32; 256];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..30 {
                c.process(&mut l, &mut r);
            }
        });
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = UmbraParams::default();
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

        let mut junk = UmbraParams {
            depth: f32::NAN,
            motion: 40.0,
            decay: f32::INFINITY,
            colour: -9.0,
            mix: 12.0,
            out: f32::NAN,
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
