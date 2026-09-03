//! SPECTRA: the spectral section.
//!
//! The sound is cut into overlapping frames, transformed, changed in
//! the frequency domain and put back — which is the only place some of
//! these things can be done at all. Five modes, all on one machine:
//!
//! - FREEZE holds the spectrum where it was: the sound stops moving
//!   and hangs, a chord out of a passing note. It hangs QUIETER than
//!   the sound it came from — a held spectrum's partials no longer
//!   beat against each other the way they did, and what that beating
//!   contributed is gone — so a freeze is turned up, not merely
//!   switched on.
//! - BLUR smears each bin's magnitude toward the last frame's, so
//!   sounds arrive late and leave later — a reverb made of time rather
//!   than of space.
//! - PITCH moves every partial by the same RATIO, in semitones,
//!   without moving the sound in time.
//! - CHOIR is PITCH twice over, at two intervals, mixed under the dry:
//!   two more voices singing the same line.
//! - ROBOT throws the phases away and rebuilds them coherently, which
//!   flattens the sound onto one pitch: a vocoder with no carrier.
//!
//! Latency is the transform's, reported so the graph pays it back. MIX
//! at zero is a wire behind that latency.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::fft::{
    FrameCutter, OverlapAdd, PhaseVocoder, RealFft, Window, apply_window_in_place, fill_window,
    magnitude_phase, normalize_overlap, polar_to_cartesian,
};
use crate::params::console::spectra as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: u32,
    pub freeze: bool,
    /// 0..1.
    pub blur: f32,
    pub pitch: f32,
    pub voice_a: f32,
    pub voice_b: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Spectra.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            mode: (clamp(p::MODE).round().max(0.0) as u32).min(p::MODE_ROBOT),
            freeze: clamp(p::FREEZE) >= 0.5,
            blur: clamp(p::BLUR) / 100.0,
            pitch: clamp(p::PITCH),
            voice_a: clamp(p::VOICE_A),
            voice_b: clamp(p::VOICE_B),
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }

    /// Whether the mode is holding the spectrum still.
    pub fn holding(&self) -> bool {
        self.mode == p::MODE_FREEZE && self.freeze
    }
}

/// One side's machine: the frames, the transform, the state the modes
/// keep between frames, and the two lines that keep wet and dry
/// telling the same moment.
struct Side {
    cutter: FrameCutter,
    adder: OverlapAdd,
    cut_store: Vec<f32>,
    add_store: Vec<f32>,
    /// A few frames at a time, so the block is fed in pieces rather
    /// than needing room for the longest block's worth at once.
    frames: Vec<f32>,
    hop_out: Vec<f32>,
    real: Vec<f32>,
    imag: Vec<f32>,
    scratch: Vec<f32>,
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    freq: Vec<f32>,
    prev_phase: Vec<f32>,
    out_phase: Vec<f32>,
    /// What the modes hold between frames, and the shifted frame the
    /// pitch modes build. A freeze holds the FREQUENCIES and the
    /// analysed phases as well as the magnitudes: a held sound whose
    /// bins are rebuilt at their centres beats against itself and
    /// sinks.
    held: Vec<f32>,
    held_freq: Vec<f32>,
    held_phase: Vec<f32>,
    /// The phases as they were ANALYSED: the locking below needs the
    /// relationships the sound arrived with.
    analysed: Vec<f32>,
    /// Which peak each bin belongs to.
    peak_of: Vec<u32>,
    shifted_mag: Vec<f32>,
    shifted_freq: Vec<f32>,
    /// The choir's accumulator: the loudest contributor per bin, and
    /// the frequency that came with it.
    acc_mag: Vec<f32>,
    acc_freq: Vec<f32>,
    warm: bool,
    /// What the machine has finished, waiting to be handed out, and
    /// the dry delayed by the same latency so the mix is between two
    /// things that happened at once.
    fifo: Vec<f32>,
    fifo_read: usize,
    fifo_write: usize,
    dry: Vec<f32>,
    dry_at: usize,
    /// This block's wet, taken out before this block's input goes in.
    wet: Vec<f32>,
    /// The last frame's POST-mode magnitudes, which is what the card's
    /// flux is measured against.
    prev_mag: Vec<f32>,
}

/// How many frames are cut at a time.
const FRAMES_AT_ONCE: usize = 4;

/// The longest block the section answers. The graph's arena never
/// hands a node more.
const MAX_BLOCK: usize = 4096;

impl Side {
    /// `prime` is the queue's head start; `delay` is what the section
    /// tells the graph it costs, which is where the round-trip test
    /// finds an impulse.
    fn new(bins: usize, prime: usize, delay: usize) -> Self {
        let mut side = Self {
            cutter: FrameCutter::new(),
            adder: OverlapAdd::new(),
            cut_store: vec![0.0; p::SIZE],
            add_store: vec![0.0; p::SIZE],
            frames: vec![0.0; p::SIZE * FRAMES_AT_ONCE],
            hop_out: vec![0.0; p::HOP * FRAMES_AT_ONCE],
            real: vec![0.0; bins],
            imag: vec![0.0; bins],
            scratch: vec![0.0; RealFft::scratch_len(p::SIZE)],
            magnitude: vec![0.0; bins],
            phase: vec![0.0; bins],
            freq: vec![0.0; bins],
            prev_phase: vec![0.0; PhaseVocoder::state_len(bins)],
            out_phase: vec![0.0; PhaseVocoder::state_len(bins)],
            held: vec![0.0; bins],
            held_freq: vec![0.0; bins],
            held_phase: vec![0.0; bins],
            analysed: vec![0.0; bins],
            peak_of: vec![0; bins],
            shifted_mag: vec![0.0; bins],
            shifted_freq: vec![0.0; bins],
            acc_mag: vec![0.0; bins],
            acc_freq: vec![0.0; bins],
            warm: false,
            // Room for the latency's head start and a long block on
            // top of it.
            fifo: vec![0.0; prime + delay + p::SIZE * 8],
            fifo_read: 0,
            fifo_write: 0,
            dry: vec![0.0; delay.max(1)],
            dry_at: 0,
            wet: vec![0.0; p::HOP],
            prev_mag: vec![0.0; bins],
        };
        // A HEAD START of silence, so the queue between the machine's
        // two halves never runs dry. A queue that runs dry does not
        // merely gap: it loses its place for good, and where it ran dry
        // would depend on how the sound was cut into blocks. The start
        // is a window — what the machine owes before it can speak — and
        // a hop on top, which is the most a chunk can ask for before
        // the next frame lands.
        side.fifo_write = prime % side.fifo.len();
        side
    }

    fn reset(&mut self, prime: usize) {
        self.cutter.reset();
        self.adder.reset();
        self.cut_store.fill(0.0);
        self.add_store.fill(0.0);
        PhaseVocoder::reset(&mut self.prev_phase);
        PhaseVocoder::reset(&mut self.out_phase);
        self.held.fill(0.0);
        self.held_freq.fill(0.0);
        self.held_phase.fill(0.0);
        self.warm = false;
        self.fifo.fill(0.0);
        self.fifo_read = 0;
        self.fifo_write = prime % self.fifo.len();
        self.dry.fill(0.0);
        self.dry_at = 0;
        self.prev_mag.fill(0.0);
    }

    fn push(&mut self, samples: &[f32]) {
        let len = self.fifo.len();
        for s in samples {
            self.fifo[self.fifo_write] = *s;
            self.fifo_write = (self.fifo_write + 1) % len;
        }
    }

    fn pop(&mut self) -> f32 {
        if self.fifo_read == self.fifo_write {
            return 0.0;
        }
        let out = self.fifo[self.fifo_read];
        self.fifo_read = (self.fifo_read + 1) % self.fifo.len();
        out
    }

    /// One sample into the dry line, and the one that comes out.
    #[inline(always)]
    fn delay(&mut self, x: f32) -> f32 {
        let out = self.dry[self.dry_at];
        self.dry[self.dry_at] = x;
        self.dry_at = (self.dry_at + 1) % self.dry.len();
        out
    }
}

/// How loud a bin must be, against the frame's loudest, to count as a
/// peak of its own.
const PEAK_FLOOR: f32 = 0.01;

/// Phase locking, which is the difference between a phase vocoder
/// that sounds like the sound and one that sounds like a phase
/// vocoder.
///
/// A note is not one bin: a windowed partial spreads over three or
/// four, and what makes them add back up to a note is the
/// RELATIONSHIP between their phases. Advance each bin on its own —
/// which is what coherent synthesis does — and the relationship
/// drifts, the lobe smears, and the sound sinks and wanders. So only
/// the PEAKS advance freely; every other bin is put back where it
/// stood relative to its peak when the sound arrived.
fn lock_to_peaks(magnitude: &[f32], analysed: &[f32], phase: &mut [f32], peak_of: &mut [u32]) {
    let bins = magnitude.len();
    if bins < 3 {
        return;
    }
    // Only a bin that is loud enough to be part of the sound counts
    // as a peak. Without the floor every speck of noise is a local
    // maximum, every speck advances on its own, and the locking has
    // nothing left to lock to.
    let loudest = magnitude.iter().fold(0.0f32, |m, v| m.max(*v));
    let floor = loudest * PEAK_FLOOR;
    let is_peak = |k: usize| {
        magnitude[k] > floor
            && (k == 0 || magnitude[k] >= magnitude[k - 1])
            && (k + 1 >= bins || magnitude[k] >= magnitude[k + 1])
    };
    // Whose peak each bin belongs to: the last one at or below it,
    // then the next one above where that is nearer.
    let mut last = 0u32;
    for k in 0..bins {
        if is_peak(k) {
            last = k as u32;
        }
        peak_of[k] = last;
    }
    let mut next = (bins - 1) as u32;
    for k in (0..bins).rev() {
        if is_peak(k) {
            next = k as u32;
        }
        let mine = peak_of[k] as i64;
        if (next as i64 - k as i64).abs() < (mine - k as i64).abs() {
            peak_of[k] = next;
        }
    }
    for k in 0..bins {
        let peak = peak_of[k] as usize;
        if peak != k && peak < bins {
            phase[k] = phase[peak] + (analysed[k] - analysed[peak]);
        }
    }
}

/// The frame's first two moments and its loudest bin: where the mass
/// of the spectrum sits, how far it is spread either side of that, and
/// how tall the tallest partial stands. Both moments come back in BINS
/// — the caller divides by the last bin to put them on 0..1 — and the
/// peak comes back as a raw magnitude.
///
/// A frame carrying almost nothing has no centre and no width, only the
/// ratio of two roundings, so under [`p::MOMENT_FLOOR`] both are 0.
fn moments(magnitude: &[f32]) -> (f32, f32, f32) {
    let mut sum = 0.0f32;
    let mut weighted = 0.0f32;
    let mut peak = 0.0f32;
    for (k, m) in magnitude.iter().enumerate() {
        sum += *m;
        weighted += k as f32 * *m;
        peak = peak.max(*m);
    }
    if sum < p::MOMENT_FLOOR {
        return (0.0, 0.0, peak);
    }
    let centroid = weighted / sum;
    let mut variance = 0.0f32;
    for (k, m) in magnitude.iter().enumerate() {
        let away = k as f32 - centroid;
        variance += *m * away * away;
    }
    (centroid, (variance / sum).max(0.0).sqrt(), peak)
}

/// How much the frame MOVED since the last one, as a share of its own
/// size — and the last one replaced by this one on the way through.
///
/// Nothing moving reads exactly 0, which is what a held freeze is; a
/// frame that arrived out of silence reads 1.
fn flux(magnitude: &[f32], prev: &mut [f32]) -> f32 {
    let mut change = 0.0f32;
    let mut sum = 0.0f32;
    for (m, was) in magnitude.iter().zip(prev.iter_mut()) {
        change += (*m - *was).abs();
        sum += *m;
        *was = *m;
    }
    (change / (sum + p::FLUX_EPS)).clamp(0.0, 1.0)
}

/// One step of the readout's one-pole, taken once per analysed frame.
/// `keep` is the coefficient: 1 would never move, 0 would not smooth.
#[inline(always)]
fn glide(state: &mut f32, target: f32, keep: f32) {
    *state += (target - *state) * (1.0 - keep);
}

pub struct SpectraCore {
    params: SectionParams,
    settings: Settings,
    fft: RealFft,
    vocoder: PhaseVocoder,
    /// The window on the way in, and the one on the way out —
    /// normalised for the hop, so two of them overlap to unity.
    window: Vec<f32>,
    synthesis: Vec<f32>,
    sides: [Side; 2],
    bins: usize,
    latency: usize,
    level_db: f32,
    /// What the card is told about the sound, all of it measured in
    /// `process` on side 0 and only copied out in `readout`.
    centroid: f32,
    spread: f32,
    flux: f32,
    peak_db: f32,
    /// The readout one-pole's per-frame coefficient, and what turns a
    /// raw bin magnitude into a dBFS figure.
    smooth: f32,
    mag_scale: f32,
}

impl SpectraCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let bins = RealFft::bins(p::SIZE);
        let mut fft = RealFft::new();
        fft.prepare(p::SIZE);
        let mut vocoder = PhaseVocoder::new();
        vocoder.prepare(p::SIZE, p::HOP);
        let mut window = vec![0.0; p::SIZE];
        fill_window(Window::SqrtHann, &mut window);
        // What the queue is given to start with, and what the whole
        // machine costs. The first is a window and a hop, which is the
        // most a chunk can ask for before the next frame lands; the
        // second is two windows, which is where the round-trip test
        // finds an impulse and what the graph pays back.
        let prime = p::SIZE + p::HOP;
        let latency = p::SIZE * 2;
        // A bin's magnitude out of an unnormalised transform is the
        // ANALYSIS WINDOW'S coherent gain times the partial's own
        // amplitude. Dividing that out is what makes the frame's peak
        // a dBFS figure rather than a number about the window: a
        // full-scale sine then reads 0 dB, whatever SIZE is.
        let coherent = window.iter().sum::<f32>() * 0.5;
        let mag_scale = if coherent > 0.0 { 1.0 / coherent } else { 1.0 };
        // The readout's one-pole steps once per analysed FRAME, not
        // per sample and not per block, so its coefficient is one
        // hop's worth of the time constant — which keeps how the card
        // moves independent of how the sound was cut into blocks.
        let frame_dt = p::HOP as f32 / sample_rate.max(1.0);
        let smooth = (-frame_dt / p::READOUT_TAU_S).exp();
        let mut core = Self {
            params: params.clone(),
            settings: Settings::of(params),
            fft,
            vocoder,
            synthesis: window.clone(),
            window,
            sides: [
                Side::new(bins, prime, latency),
                Side::new(bins, prime, latency),
            ],
            bins,
            latency,
            level_db: -120.0,
            centroid: 0.0,
            spread: 0.0,
            flux: 0.0,
            peak_db: p::READOUT_FLOOR_DB,
            smooth,
            mag_scale,
        };
        for side in &mut core.sides {
            side.cutter.prepare(p::SIZE, p::HOP);
            side.adder.prepare(p::SIZE, p::HOP);
        }
        // Two square-root Hann windows, one on the way in and one on
        // the way out, overlap to a constant only once they are
        // normalised for the hop.
        let mut sums = vec![0.0; p::HOP];
        let analysis = core.window.clone();
        let _ = normalize_overlap(&analysis, &mut core.synthesis, p::HOP, &mut sums);
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Move a spectrum by `semitones`, into `mag` and `freq`.
    fn shift(
        semitones: f32,
        magnitude: &[f32],
        freq: &[f32],
        mag_out: &mut [f32],
        freq_out: &mut [f32],
    ) {
        let ratio = 2f32.powf(semitones / 12.0);
        mag_out.fill(0.0);
        freq_out.fill(0.0);
        for (k, (m, f)) in magnitude.iter().zip(freq).enumerate() {
            let to = ((k as f32) * ratio).round() as usize;
            if to < mag_out.len() {
                // Where two bins land on one, the louder wins: a sum
                // would build a peak that was never in the sound.
                if *m > mag_out[to] {
                    mag_out[to] = *m;
                    freq_out[to] = *f * ratio;
                }
            }
        }
    }

    /// Frame `index` of the batch, changed by the mode in hand.
    fn frame(&mut self, side: usize, index: usize) {
        let s = self.settings;
        let bins = self.bins;
        let at = index * p::SIZE..(index + 1) * p::SIZE;
        let it = &mut self.sides[side];
        apply_window_in_place(&mut it.frames[at.clone()], &self.window);
        self.fft.forward(
            &it.frames[at.clone()],
            &mut it.real,
            &mut it.imag,
            &mut it.scratch,
        );
        magnitude_phase(&it.real, &it.imag, &mut it.magnitude, &mut it.phase);
        // The card's two moments are taken PRE-mode, on side 0 only:
        // the face draws the mode's own law bending this spectrum, so
        // measuring the shifted frame here would draw the shift twice.
        // The two sides carry the same music; one pass is enough.
        if side == 0 {
            let (centroid, spread, peak) = moments(&it.magnitude[..bins]);
            let last = (bins.max(2) - 1) as f32;
            glide(
                &mut self.centroid,
                (centroid / last).clamp(0.0, 1.0),
                self.smooth,
            );
            glide(
                &mut self.spread,
                (spread / last).clamp(0.0, 1.0),
                self.smooth,
            );
            // The rake's height, so it stands even at MIX 0 where
            // level_db is reporting the dry going past.
            let scaled = peak * self.mag_scale;
            self.peak_db = if scaled <= 0.0 {
                p::READOUT_FLOOR_DB
            } else {
                (20.0 * scaled.log10()).max(p::READOUT_FLOOR_DB)
            };
        }
        it.analysed[..bins].copy_from_slice(&it.phase[..bins]);
        self.vocoder
            .analyse(&it.phase, &mut it.freq, &mut it.prev_phase);

        match s.mode {
            p::MODE_FREEZE => {
                if s.freeze {
                    if !it.warm {
                        it.held[..bins].copy_from_slice(&it.magnitude[..bins]);
                        it.held_freq[..bins].copy_from_slice(&it.freq[..bins]);
                        it.held_phase[..bins].copy_from_slice(&it.analysed[..bins]);
                        it.warm = true;
                    }
                    it.magnitude[..bins].copy_from_slice(&it.held[..bins]);
                    it.freq[..bins].copy_from_slice(&it.held_freq[..bins]);
                    it.analysed[..bins].copy_from_slice(&it.held_phase[..bins]);
                    // The held phases ROTATE. What holds a note
                    // together is the relationship between its bins,
                    // and that relationship turns by each bin's own
                    // hop advance every frame — hold it still and
                    // neighbours fall out of step and cancel, which is
                    // a freeze that sinks eighteen dB the moment it
                    // takes hold.
                    let per_hop = core::f32::consts::TAU * p::HOP as f32 / p::SIZE as f32;
                    for (k, phase) in it.held_phase[..bins].iter_mut().enumerate() {
                        *phase += per_hop * k as f32;
                    }
                } else {
                    it.warm = false;
                }
            }
            p::MODE_BLUR => {
                let hold = p::BLUR_HOLD * s.blur;
                for (held, now) in it.held.iter_mut().zip(&it.magnitude[..bins]) {
                    *held = held.max(*now) * hold + *now * (1.0 - hold);
                }
                it.magnitude[..bins].copy_from_slice(&it.held[..bins]);
            }
            p::MODE_PITCH => {
                Self::shift(
                    s.pitch,
                    &it.magnitude[..bins],
                    &it.freq[..bins],
                    &mut it.shifted_mag,
                    &mut it.shifted_freq,
                );
                it.magnitude[..bins].copy_from_slice(&it.shifted_mag[..bins]);
                it.freq[..bins].copy_from_slice(&it.shifted_freq[..bins]);
            }
            p::MODE_CHOIR => {
                // Two more voices, each half the height of the line
                // they sing under. A bin takes its LOUDEST claimant,
                // magnitude and frequency together: a bin that took one
                // voice's height and another's pitch would sing neither.
                it.acc_mag[..bins].copy_from_slice(&it.magnitude[..bins]);
                it.acc_freq[..bins].copy_from_slice(&it.freq[..bins]);
                for semitones in [s.voice_a, s.voice_b] {
                    Self::shift(
                        semitones,
                        &it.magnitude[..bins],
                        &it.freq[..bins],
                        &mut it.shifted_mag,
                        &mut it.shifted_freq,
                    );
                    for k in 0..bins {
                        let voice = it.shifted_mag[k] * 0.5;
                        if voice > it.acc_mag[k] {
                            it.acc_mag[k] = voice;
                            it.acc_freq[k] = it.shifted_freq[k];
                        }
                    }
                }
                it.magnitude[..bins].copy_from_slice(&it.acc_mag[..bins]);
                it.freq[..bins].copy_from_slice(&it.acc_freq[..bins]);
            }
            _ => {
                // ROBOT: every partial rebuilt on the bin it sits in,
                // so what was speech becomes a monotone.
                for (k, f) in it.freq[..bins].iter_mut().enumerate() {
                    *f = k as f32;
                }
            }
        }

        // Flux is taken POST-mode, so it reports what the machine
        // actually did rather than what arrived: exactly nothing moving
        // under a held freeze, a little under a blur, everything moving
        // on a transient. It is the number that proves the mode took.
        if side == 0 {
            let moved = flux(&it.magnitude[..bins], &mut it.prev_mag[..bins]);
            glide(&mut self.flux, moved, self.smooth);
        }
        self.vocoder
            .synthesise(&it.freq, &mut it.phase, &mut it.out_phase);
        // Locking is for the modes that leave the partials where they
        // are. A held or a moved lobe carries its own true frequency in
        // every one of its bins, so advancing each by that keeps them
        // in step; locking those to a snapshot of the ORIGINAL sound's
        // relationships is what pulls them apart.
        let moved = matches!(s.mode, p::MODE_PITCH | p::MODE_CHOIR) || s.holding();
        if !moved {
            lock_to_peaks(
                &it.magnitude[..bins],
                &it.analysed[..bins],
                &mut it.phase[..bins],
                &mut it.peak_of[..bins],
            );
        }
        polar_to_cartesian(&it.magnitude, &it.phase, &mut it.real, &mut it.imag);
        self.fft.inverse(
            &it.real,
            &it.imag,
            &mut it.frames[at.clone()],
            &mut it.scratch,
        );
        apply_window_in_place(&mut it.frames[at], &self.synthesis);
    }

    fn run(&mut self, side: usize, io: &mut [f32]) {
        // A HOP at a time, whatever the block is. The queue between
        // the two halves of the machine is exactly one hop deep in the
        // steady state; hand it a longer stretch and it runs dry, and
        // a queue that has run dry has lost its place for good. Working
        // in hops instead makes what comes out independent of how the
        // sound was cut up, which is the contract.
        let mut at = 0;
        while at < io.len() {
            let end = (at + p::HOP).min(io.len());
            self.chunk(side, &mut io[at..end]);
            at = end;
        }
    }

    /// One hop or less: take out what is ready, feed in what arrived,
    /// and mix the two.
    fn chunk(&mut self, side: usize, io: &mut [f32]) {
        let mix = self.settings.mix;
        let n = io.len();
        // What the machine finished BEFORE this chunk, taken out first:
        // afterwards, a chunk's own late frames would overtake its
        // early samples.
        for i in 0..n {
            self.sides[side].wet[i] = self.sides[side].pop();
        }
        let mut fed = 0;
        while fed < n {
            let (used, made) = {
                let it = &mut self.sides[side];
                let mut frames = std::mem::take(&mut it.frames);
                let got = it
                    .cutter
                    .process(&io[fed..], &mut frames, &mut it.cut_store);
                it.frames = frames;
                got
            };
            if used == 0 && made == 0 {
                break;
            }
            fed += used;
            for index in 0..made {
                self.frame(side, index);
            }
            if made > 0 {
                let it = &mut self.sides[side];
                let mut hop_out = std::mem::take(&mut it.hop_out);
                let (_, wrote) = it.adder.process(
                    &it.frames[..made * p::SIZE],
                    &mut hop_out[..made * p::HOP],
                    &mut it.add_store,
                );
                self.sides[side].push(&hop_out[..wrote]);
                self.sides[side].hop_out = hop_out;
            }
        }
        // Then the dry, delayed to stand beside the wet.
        for (i, x) in io.iter_mut().enumerate().take(n) {
            let dry = self.sides[side].delay(*x);
            let wet = self.sides[side].wet[i];
            *x = dry + (wet - dry) * mix;
        }
    }
}

impl SectionCore for SpectraCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            // A mode that holds state starts fresh when it is chosen.
            if next.mode != self.settings.mode || next.freeze != self.settings.freeze {
                for side in &mut self.sides {
                    side.warm = false;
                    side.held.fill(0.0);
                }
            }
            self.settings = next;
        }
    }

    fn reset(&mut self) {
        let prime = p::SIZE + p::HOP;
        for side in &mut self.sides {
            side.reset(prime);
        }
        self.level_db = -120.0;
        self.centroid = 0.0;
        self.spread = 0.0;
        self.flux = 0.0;
        self.peak_db = p::READOUT_FLOOR_DB;
    }

    fn latency(&self) -> usize {
        self.latency
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > MAX_BLOCK {
            return;
        }
        let stereo = r.len() >= n;
        // Even at no mix the machine runs: the latency it costs must
        // not change with the knob, and the graph compensates for it
        // once at compile.
        self.run(0, l);
        if stereo {
            self.run(1, &mut r[..n]);
        }
        let peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
    }

    /// What the card is drawn from. A plain copy of fields measured in
    /// `process`; nothing is computed here. Everything but `level_db`
    /// is refreshed once per ANALYSED FRAME — one hop, 256 samples,
    /// about 5.3 ms at 48 kHz — and held between frames.
    ///
    /// - `level_db`: the section's OUTPUT peak over the last block, in
    ///   dBFS, floored at -120. At MIX 0 that is the dry going past
    ///   untouched, which is why it can be full while the spectrum
    ///   above it is doing something else entirely. Per block, no
    ///   smoothing.
    /// - `reduction_db`: repurposed — this section reduces no gain.
    ///   The PRE-mode analysed frame's LOUDEST BIN in dBFS, the
    ///   analysis window's coherent gain divided out so a full-scale
    ///   sine reads 0. Range -120..0-ish, floored hard at -120. Per
    ///   frame, no smoothing: it is that frame's own peak.
    /// - `bands[0]`: SPECTRAL CENTROID of the PRE-mode frame — where
    ///   the spectrum's mass sits — as a share of the LINEAR bin axis:
    ///   0.0 is DC, 1.0 is Nyquist. Exactly 0.0 on a frame whose
    ///   magnitudes sum under 1e-6. One-pole, 60 ms.
    /// - `bands[1]`: SPECTRAL SPREAD of the same PRE-mode frame — the
    ///   magnitude-weighted standard deviation about that centroid —
    ///   on the same 0..1 bin axis. A lone partial sits near 0, a
    ///   broadband noise near 0.3, silence at exactly 0.0. One-pole,
    ///   60 ms.
    /// - `bands[2]`: SPECTRAL FLUX of the POST-mode frame — how much
    ///   the frame moved since the last one, sum|m - m_prev| over
    ///   sum m — clamped to 0..1. 0.0 while a freeze holds, small
    ///   under a blur, near 1.0 on a transient out of silence.
    ///   One-pole, 60 ms.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.peak_db,
            bands: [self.centroid, self.spread, self.flux],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> SpectraCore {
        let mut params = SectionParams::of(SectionKind::Spectra);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        SpectraCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut SpectraCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    /// A deterministic hiss: broadband, the same sound every run, and
    /// no dependency on anything the red zone cannot have.
    fn noise(amp: f32, n: usize) -> Vec<f32> {
        let mut state = 0x1234_5678u32;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((state >> 8) as f32 / 8_388_608.0 - 1.0)
            })
            .collect()
    }

    /// Run a sound through and ask the section what it saw.
    fn readout_after(core: &mut SpectraCore, l: &[f32]) -> Readout {
        let _ = run(core, l);
        core.readout()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// The level at `hz`, in dBFS.
    fn level_at(signal: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in signal.iter().enumerate() {
            let w = 2.0 * core::f32::consts::PI * hz * i as f32 / FS;
            re += s * w.cos();
            im -= s * w.sin();
        }
        let mag = (re * re + im * im).sqrt() * 2.0 / signal.len() as f32;
        20.0 * mag.max(1e-9).log10()
    }

    /// The round trip lands an impulse exactly where the section says
    /// it will, at the level it went in: the window pair overlaps to
    /// unity and the latency reported is the latency taken.
    #[test]
    fn the_round_trip_is_unity_at_the_latency_it_reports() {
        let n = 8192;
        let mut l = vec![0.0f32; n];
        l[0] = 1.0;
        let mut core = core_with(&[(p::MIX, 100.0)]);
        let ahead = core.latency();
        let out = run(&mut core, &l);
        let (at, peak) = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, v)| (i, *v))
            .expect("something came out");
        assert_eq!(at, ahead, "the impulse landed at {at}, not {ahead}");
        assert!(
            (peak - 1.0).abs() < 0.01,
            "the round trip is not unity: {peak}"
        );
        // A window's ring before the peak is the transform's own
        // doing; what matters is that the peak is where the section
        // says it is, and that nothing loud arrives before it.
        assert!(
            out[..ahead].iter().all(|s| s.abs() < 0.05),
            "something loud arrived before the latency"
        );
    }

    /// At no mix the section is a wire behind its latency, which the
    /// graph pays back — and the latency does not move with the knobs.
    #[test]
    fn no_mix_is_a_wire_behind_the_latency() {
        let mut core = core_with(&[]);
        let ahead = core.latency();
        assert!(ahead > 0);
        let n = 8192;
        let l = sine(440.0, 0.5, n);
        let out = run(&mut core, &l);
        for i in ahead..n {
            assert!(
                (out[i] - l[i - ahead]).abs() < 1e-6,
                "sample {i}: {} against {}",
                out[i],
                l[i - ahead]
            );
        }
        let mut busy = core_with(&[(p::MODE, 2.0), (p::PITCH, 7.0), (p::MIX, 100.0)]);
        assert_eq!(busy.latency(), ahead, "the latency moved with the mode");
        let _ = run(&mut busy, &l);
    }

    /// The round trip is near transparent: a tone through the machine
    /// with nothing asked of it comes back at its own level.
    #[test]
    fn the_round_trip_keeps_the_sound() {
        let n = FS as usize / 2;
        let l = sine(440.0, 0.5, n);
        let mut core = core_with(&[(p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let settled = n / 2..n - 2048;
        let change = 20.0 * (rms(&out[settled.clone()]) / rms(&l[settled])).log10();
        assert!(
            change.abs() < 1.5,
            "the round trip moved the level {change} dB"
        );
    }

    /// FREEZE holds the sound after it has gone: what hangs is the
    /// note that was there, at a steady level, and thawed the section
    /// lets go with it.
    #[test]
    fn freeze_holds_the_note_after_it_has_gone() {
        let n = FS as usize;
        let mut l = sine(440.0, 0.5, n);
        // The note stops halfway.
        for s in l.iter_mut().skip(n / 2) {
            *s = 0.0;
        }
        let mut core = core_with(&[(p::MODE, 0.0), (p::FREEZE, 1.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let held = rms(&out[n * 3 / 4..]);
        assert!(held > 0.02, "the freeze let go: {held}");
        // Steady, not decaying: two windows a quarter of a second
        // apart are within a dB of each other.
        let early = rms(&out[n * 5 / 8..n * 5 / 8 + 4_800]);
        let late = rms(&out[n - 4_800..]);
        let drift = 20.0 * (late / early).log10();
        assert!(drift.abs() < 1.0, "the held sound drifted {drift} dB");
        // And it is the note, not a smear: the fundamental leads.
        let window = n * 3 / 4..n * 3 / 4 + 9_600;
        let note = level_at(&out[window.clone()], 440.0);
        let octave = level_at(&out[window], 880.0);
        assert!(
            note > octave + 6.0,
            "the held sound is a smear: {note} against {octave}"
        );
        // CHANGED with the readout: bands[2] used to be a copy of
        // holding(), which the face already has from its own value —
        // a band spent on a setting. It is now the POST-mode spectral
        // flux, so the same fact arrives MEASURED instead: a held
        // freeze puts the identical frame out every hop, and nothing
        // moving is what the band reads.
        let churn = core.readout().bands[2];
        assert!(churn < 0.01, "the held spectrum is still moving: {churn}");

        let mut thawed = core_with(&[(p::MODE, 0.0), (p::FREEZE, 0.0), (p::MIX, 100.0)]);
        let out = run(&mut thawed, &l);
        assert!(
            rms(&out[n * 3 / 4..]) < 0.02,
            "it froze without being asked"
        );
    }

    /// PITCH moves the partials by a ratio: an octave up puts the tone
    /// an octave up.
    #[test]
    fn pitch_moves_the_note() {
        let n = FS as usize / 2;
        let l = sine(440.0, 0.5, n);
        let mut core = core_with(&[(p::MODE, 2.0), (p::PITCH, 12.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let window = n / 2..n / 2 + 9600;
        let up = level_at(&out[window.clone()], 880.0);
        let old = level_at(&out[window], 440.0);
        assert!(up > -24.0, "no octave: {up} dBFS");
        assert!(
            up > old + 6.0,
            "the old note is still there: {old} against {up}"
        );
    }

    /// CHOIR adds voices without taking the line away.
    #[test]
    fn choir_sings_under_the_line() {
        let n = FS as usize / 2;
        let l = sine(440.0, 0.5, n);
        let mut core = core_with(&[
            (p::MODE, 3.0),
            (p::VOICE_A, 4.0),
            (p::VOICE_B, 7.0),
            (p::MIX, 100.0),
        ]);
        let out = run(&mut core, &l);
        let window = n / 2..n / 2 + 9600;
        let line = level_at(&out[window.clone()], 440.0);
        let third = level_at(&out[window.clone()], 440.0 * 2f32.powf(4.0 / 12.0));
        let fifth = level_at(&out[window], 440.0 * 2f32.powf(7.0 / 12.0));
        assert!(line > -20.0, "the line went: {line} dBFS");
        assert!(third > line - 20.0, "no third: {third} against {line}");
        assert!(fifth > line - 20.0, "no fifth: {fifth} against {line}");
    }

    /// BLUR and ROBOT both change the sound and neither runs away.
    #[test]
    fn blur_and_robot_do_something_and_stay_bounded() {
        let n = FS as usize / 2;
        let l = sine(440.0, 0.5, n);
        for (mode, extra) in [(1.0, (p::BLUR, 100.0)), (4.0, (p::BLUR, 0.0))] {
            let mut core = core_with(&[(p::MODE, mode), extra, (p::MIX, 100.0)]);
            let out = run(&mut core, &l);
            let apart = rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>());
            assert!(apart > 0.01, "mode {mode} did nothing");
            assert!(out.iter().all(|s| s.abs() < 2.0), "mode {mode} ran away");
        }
    }

    /// However the sound is cut into blocks, the same sound comes
    /// out. Not to the bit: an overlap-add sums its frames in whatever
    /// order the blocks hand them over, and a phase vocoder integrates
    /// those roundings from frame to frame — so the contract here is
    /// that the difference is buried, forty dB under the sound.
    #[test]
    fn split_blocks_give_the_same_sound() {
        let l = sine(330.0, 0.4, 6000);
        let edits = [(p::MODE, 2.0), (p::PITCH, 5.0), (p::MIX, 70.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        while at < b.len() {
            for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
                let end = (at + len).min(b.len());
                if at >= end {
                    break;
                }
                pieces.process(&mut b[at..end], &mut [], &clock());
                at = end;
            }
        }
        let apart = rms(&a.iter().zip(&b).map(|(x, y)| x - y).collect::<Vec<_>>());
        let sound = rms(&a);
        let under = 20.0 * (apart / sound.max(1e-9)).log10();
        assert!(under < -40.0, "the two runs differ by {under} dB");
    }

    /// At its defaults, on silence, the section reports rest: no
    /// centre, no width, nothing moving, and both dB figures on the
    /// floor. A card drawn from this is a card at rest.
    #[test]
    fn the_defaults_report_rest() {
        let mut core = core_with(&[]);
        let quiet = vec![0.0f32; FS as usize / 4];
        let said = readout_after(&mut core, &quiet);
        assert_eq!(said.bands, [0.0, 0.0, 0.0], "silence is not rest: {said:?}");
        assert_eq!(said.reduction_db, -120.0, "the rake stands on silence");
        assert_eq!(said.level_db, -120.0, "the floor bar stands on silence");
    }

    /// bands[0] IS the sound's centre of mass on the linear bin axis:
    /// a low tone puts it near DC, a high one carries it up, and each
    /// lands on the bin the tone is actually in — which is where the
    /// card draws the envelope's centre.
    #[test]
    fn the_centroid_band_follows_the_sound_up_the_bin_axis() {
        let n = FS as usize / 2;
        let last = (RealFft::bins(p::SIZE) - 1) as f32;
        let want = |hz: f32| hz * p::SIZE as f32 / FS / last;
        let mut low = core_with(&[]);
        let low_c = readout_after(&mut low, &sine(220.0, 0.5, n)).bands[0];
        let mut high = core_with(&[]);
        let high_c = readout_after(&mut high, &sine(5_000.0, 0.5, n)).bands[0];
        assert!(
            (low_c - want(220.0)).abs() < 0.02,
            "220 Hz landed at {low_c}, not {}",
            want(220.0)
        );
        assert!(
            (high_c - want(5_000.0)).abs() < 0.02,
            "5 kHz landed at {high_c}, not {}",
            want(5_000.0)
        );
        assert!(
            high_c > low_c + 0.1,
            "the band did not move: {high_c} against {low_c}"
        );
    }

    /// bands[1] is the WIDTH of that mass: one partial is narrow, hiss
    /// across the whole band is wide, and the number sits where a
    /// uniform spectrum's standard deviation should — 1/sqrt(12) of
    /// the axis.
    #[test]
    fn the_spread_band_tells_a_tone_from_a_hiss() {
        let n = FS as usize / 2;
        let mut tone = core_with(&[]);
        let narrow = readout_after(&mut tone, &sine(1_000.0, 0.5, n)).bands[1];
        let mut hiss = core_with(&[]);
        let wide = readout_after(&mut hiss, &noise(0.5, n)).bands[1];
        assert!(narrow < 0.05, "a lone partial reads {narrow} wide");
        assert!(
            (wide - 1.0 / 12f32.sqrt()).abs() < 0.08,
            "flat hiss reads {wide}, not about 0.29"
        );
        assert!(wide <= 1.0, "the spread ran off the axis: {wide}");
    }

    /// bands[2] is measured POST-mode, which is what makes it the
    /// number that proves the mode took: a held freeze puts the same
    /// frame out over and over and the band falls to nothing, while
    /// the same hiss running free keeps it up.
    #[test]
    fn the_flux_band_falls_to_nothing_under_a_held_freeze() {
        let n = FS as usize / 2;
        let hiss = noise(0.5, n);
        let mut running = core_with(&[(p::MODE, 0.0), (p::FREEZE, 0.0), (p::MIX, 100.0)]);
        let moving = readout_after(&mut running, &hiss).bands[2];
        let mut held = core_with(&[(p::MODE, 0.0), (p::FREEZE, 1.0), (p::MIX, 100.0)]);
        let still = readout_after(&mut held, &hiss).bands[2];
        assert!(moving > 0.1, "churning hiss reads only {moving}");
        assert!(still < 0.01, "the held spectrum still moves: {still}");
    }

    /// And a blur smears one frame into the next, so the band DROPS
    /// without reaching nothing — which is what the stair's lit share
    /// on the card is drawn from.
    #[test]
    fn the_flux_band_drops_under_a_blur() {
        let n = FS as usize / 2;
        let hiss = noise(0.5, n);
        let mut open = core_with(&[(p::MODE, 1.0), (p::BLUR, 0.0), (p::MIX, 100.0)]);
        let raw = readout_after(&mut open, &hiss).bands[2];
        let mut smeared = core_with(&[(p::MODE, 1.0), (p::BLUR, 100.0), (p::MIX, 100.0)]);
        let smooth = readout_after(&mut smeared, &hiss).bands[2];
        assert!(raw > 0.1, "the hiss did not churn: {raw}");
        assert!(
            smooth < raw * 0.5,
            "the blur left the flux at {smooth} of {raw}"
        );
        assert!(smooth > 0.0, "the blur stopped the spectrum dead");
    }

    /// The two moments are taken BEFORE the mode: an octave shift
    /// moves the sound and not the numbers, because the face draws the
    /// mode's own law over the incoming spectrum and must not be
    /// handed the shift a second time.
    #[test]
    fn the_moments_are_measured_before_the_mode() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, 0.5, n);
        let mut flat = core_with(&[(p::MODE, 2.0), (p::PITCH, 0.0), (p::MIX, 100.0)]);
        let a = readout_after(&mut flat, &l);
        let mut up = core_with(&[(p::MODE, 2.0), (p::PITCH, 12.0), (p::MIX, 100.0)]);
        let b = readout_after(&mut up, &l);
        assert!(
            (a.bands[0] - b.bands[0]).abs() < 0.005,
            "the shift reached the centroid: {} against {}",
            a.bands[0],
            b.bands[0]
        );
        assert!(
            (a.reduction_db - b.reduction_db).abs() < 0.5,
            "the shift reached the height: {} against {}",
            a.reduction_db,
            b.reduction_db
        );
    }

    /// reduction_db is the analysed INPUT frame's tallest bin, so the
    /// rake has a height at MIX 0 — where level_db is busy reporting
    /// the dry going past — and a half-scale tone reads about -6 dBFS,
    /// which is the window's gain divided back out.
    #[test]
    fn the_rake_has_a_height_at_no_mix() {
        let n = FS as usize / 2;
        let mut loud = core_with(&[]);
        let a = readout_after(&mut loud, &sine(1_000.0, 0.5, n));
        assert!(loud.settings().is_wire(), "the test is not at MIX 0");
        let mut quiet = core_with(&[]);
        let b = readout_after(&mut quiet, &sine(1_000.0, 0.005, n));
        assert!(
            (a.reduction_db + 6.0).abs() < 3.0,
            "half scale read {} dBFS",
            a.reduction_db
        );
        assert!(
            a.reduction_db > b.reduction_db + 30.0,
            "the height did not follow the level: {} against {}",
            a.reduction_db,
            b.reduction_db
        );
        assert!(a.reduction_db >= -120.0 && b.reduction_db >= -120.0);
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::MODE, 99.0);
        assert_eq!(core.settings().mode, p::MODE_ROBOT);
        core.set_param(p::PITCH, 99.0);
        assert_eq!(core.settings().pitch, 24.0);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }
}
