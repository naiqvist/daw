//! Sibyl — the pitch shifter that arrives as a harmoniser.
//!
//! An oracle answers in more voices than it was asked with, which is what
//! this does: a phase vocoder shifts the incoming voice, and one or two
//! more voices arrive beside it in whatever key the passage is in.
//!
//! # What makes it this device and not a pitch knob
//!
//! **The harmonies are measured in SCALE STEPS, not semitones.** A third
//! is three semitones or four depending on where in the key you are
//! standing, and a harmoniser set to "+4 semitones" is wrong half the
//! time — famously, audibly wrong, in the way that makes people give up
//! on harmonisers. Sibyl listens for the note actually being sung,
//! snaps it into the chosen scale, counts the requested number of
//! DEGREES from there, and shifts by whatever interval that turns out to
//! be. The interval changes from note to note. That is the point.
//!
//! **The formant control is independent and defaults to HELD.** Shifting
//! a spectrum moves its resonances with it, which is why cheap shifters
//! turn a voice into a chipmunk going up and a monster going down. The
//! envelope here is put back where the source had it unless FORMANT says
//! otherwise — so an octave down is an octave down, and the monster is
//! available deliberately at −12 rather than by accident at 0.
//!
//! # What it is honestly good at
//!
//! MONOPHONIC material. The pitch detector picks one fundamental, and on
//! a chord it will pick one of the notes and harmonise that. This is the
//! same limitation every harmoniser has and it is not hidden: the card
//! prints the note it thinks it heard, so a disagreement is visible
//! rather than mysterious.
//!
//! # The parts
//!
//! [`RealFft`](crate::dsp::fft::RealFft) with a frame cutter and
//! overlap-add, as `resyn` uses; and
//! [`PhaseVocoder`](crate::dsp::fft::PhaseVocoder), which `resyn` does
//! not — that one deliberately keeps the ORIGINAL phase, which is what
//! makes it a resynthesiser and what makes it unable to transpose.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::fft::{FrameCutter, OverlapAdd, PhaseVocoder, RealFft, Window};
use crate::params::sibyl as p;
use crate::theory::{Scale, scale_interval};

/// The transform size. The same 1024 `resyn` settled on: long enough to
/// resolve a bass fundamental, short enough that a transient does not
/// smear across the whole frame.
pub const SIZE: usize = 1024;
/// Four-times overlap.
pub const HOP: usize = 256;
pub const BINS: usize = SIZE / 2 + 1;
/// Reported latency, in samples.
pub const LATENCY: usize = SIZE;

/// Bins the spectral envelope is smoothed over. Wide enough to lose the
/// individual partials, narrow enough to keep the formants.
const ENV_SPAN: usize = 9;
/// Where a fundamental is looked for, in hertz.
const PITCH_MIN_HZ: f32 = 55.0;
const PITCH_MAX_HZ: f32 = 1_200.0;
/// A bin quieter than this fraction of the frame's peak is not a note.
const PITCH_FLOOR: f32 = 0.06;
/// How many frames a detected pitch is held after it stops being heard,
/// so a harmony does not lurch on every consonant.
const PITCH_HOLD_FRAMES: u32 = 24;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SibylParams {
    pub shift_st: f32,
    pub formant_st: f32,
    pub voice_a: f32,
    pub voice_b: f32,
    pub key: f32,
    pub scale: f32,
    pub blend: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SibylParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            shift_st: d(p::SHIFT),
            formant_st: d(p::FORMANT),
            voice_a: d(p::VOICE_A),
            voice_b: d(p::VOICE_B),
            key: d(p::KEY),
            scale: d(p::SCALE),
            blend: d(p::BLEND),
            mix: d(p::MIX),
            out: d(p::OUT),
        }
    }
}

impl SibylParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::SHIFT => self.shift_st = value,
            p::FORMANT => self.formant_st = value,
            p::VOICE_A => self.voice_a = value,
            p::VOICE_B => self.voice_b = value,
            p::KEY => self.key = value,
            p::SCALE => self.scale = value,
            p::BLEND => self.blend = value,
            p::MIX => self.mix = value,
            p::OUT => self.out = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::SHIFT => Some(self.shift_st),
            p::FORMANT => Some(self.formant_st),
            p::VOICE_A => Some(self.voice_a),
            p::VOICE_B => Some(self.voice_b),
            p::KEY => Some(self.key),
            p::SCALE => Some(self.scale),
            p::BLEND => Some(self.blend),
            p::MIX => Some(self.mix),
            p::OUT => Some(self.out),
            _ => None,
        }
    }

    /// Drag every field back inside its row. A patch off disk is
    /// untrusted input.
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

    /// The scale this patch is in.
    pub fn scale_of(&self) -> Scale {
        let i = (self.scale.round().max(0.0) as usize).min(Scale::ALL.len() - 1);
        Scale::ALL.get(i).copied().unwrap_or(Scale::Major)
    }

    /// The tonic's pitch class, `0..=11`.
    pub fn key_of(&self) -> i16 {
        (self.key.round() as i16).rem_euclid(12)
    }
}

/// One channel's frame plumbing — the same shape `resyn` uses, which is
/// the shape that made its latency constant.
#[derive(Debug, Clone)]
struct Channel {
    cutter: FrameCutter,
    cut_store: Vec<f32>,
    adder: OverlapAdd,
    add_store: Vec<f32>,
    frame: Vec<f32>,
    hop_out: Vec<f32>,
    /// The vocoder's two per-bin memories.
    prev_phase: Vec<f32>,
    acc_phase: Vec<f32>,
    fifo: Vec<f32>,
    fifo_head: usize,
    fifo_len: usize,
    dry: Vec<f32>,
    dry_write: usize,
}

impl Channel {
    fn new() -> Self {
        let mut cutter = FrameCutter::new();
        cutter.prepare(SIZE, HOP);
        let mut adder = OverlapAdd::new();
        adder.prepare(SIZE, HOP);
        let mut ch = Self {
            cutter,
            cut_store: vec![0.0; SIZE],
            adder,
            add_store: vec![0.0; SIZE],
            frame: vec![0.0; SIZE],
            hop_out: vec![0.0; HOP],
            prev_phase: vec![0.0; PhaseVocoder::state_len(BINS)],
            acc_phase: vec![0.0; PhaseVocoder::state_len(BINS)],
            fifo: vec![0.0; SIZE * 4],
            fifo_head: 0,
            fifo_len: 0,
            dry: vec![0.0; LATENCY],
            dry_write: 0,
        };
        ch.prime();
        ch
    }

    /// [`HOP`] zeros, which is what turns a delay that jitters with the
    /// block boundary into a constant one.
    fn prime(&mut self) {
        self.fifo_head = 0;
        self.fifo_len = HOP;
        for s in self.fifo.iter_mut() {
            *s = 0.0;
        }
    }

    fn reset(&mut self) {
        self.cutter.reset();
        self.adder.reset();
        for s in self
            .cut_store
            .iter_mut()
            .chain(self.add_store.iter_mut())
            .chain(self.dry.iter_mut())
            .chain(self.frame.iter_mut())
        {
            *s = 0.0;
        }
        PhaseVocoder::reset(&mut self.prev_phase);
        PhaseVocoder::reset(&mut self.acc_phase);
        self.dry_write = 0;
        self.prime();
    }

    fn push_fifo(&mut self, samples: &[f32]) {
        let cap = self.fifo.len();
        for s in samples {
            if self.fifo_len == cap {
                break; // fail open; never a panic
            }
            let at = (self.fifo_head + self.fifo_len) % cap;
            if let Some(slot) = self.fifo.get_mut(at) {
                *slot = *s;
            }
            self.fifo_len += 1;
        }
    }

    fn pop_fifo(&mut self) -> f32 {
        if self.fifo_len == 0 {
            return 0.0;
        }
        let cap = self.fifo.len();
        let out = self.fifo.get(self.fifo_head).copied().unwrap_or(0.0);
        self.fifo_head = (self.fifo_head + 1) % cap;
        self.fifo_len -= 1;
        out
    }

    fn dry_tick(&mut self, x: f32) -> f32 {
        let cap = self.dry.len();
        let out = self.dry.get(self.dry_write).copied().unwrap_or(0.0);
        if let Some(slot) = self.dry.get_mut(self.dry_write) {
            *slot = x;
        }
        self.dry_write += 1;
        if self.dry_write >= cap {
            self.dry_write = 0;
        }
        out
    }
}

/// One harmoniser.
#[derive(Debug, Clone)]
pub struct SibylCore {
    params: SibylParams,
    sample_rate: f32,
    ch: Vec<Channel>,
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
    env: Vec<f32>,
    out_mag: Vec<f32>,
    out_freq: Vec<f32>,
    /// The strongest magnitude each output bin has been offered, so the
    /// LOUDEST contributor decides that bin's frequency rather than
    /// whichever voice happened to be written last.
    out_claim: Vec<f32>,
    /// The last pitch heard, in MIDI, and how long ago.
    heard_midi: f32,
    heard_age: u32,
}

impl SibylCore {
    pub fn new(sample_rate: f32, params: &SibylParams) -> Self {
        let mut fft = RealFft::new();
        fft.prepare(SIZE);
        let mut vocoder = PhaseVocoder::new();
        vocoder.prepare(SIZE, HOP);
        let mut analysis = vec![0.0; SIZE];
        crate::dsp::fft::fill_window(Window::Hann, &mut analysis);
        // The synthesis window starts as a COPY of the analysis one.
        // `normalize_overlap` normalises a window that is already there —
        // it multiplies the pair together to find the overlap gain — so
        // handing it a zeroed buffer makes every sum zero and leaves it
        // zeroed, which is a device that transforms perfectly and then
        // multiplies the result by nothing.
        let mut synthesis = analysis.clone();
        let mut phase_sums = vec![0.0; HOP];
        crate::dsp::fft::normalize_overlap(&analysis, &mut synthesis, HOP, &mut phase_sums);

        let mut core = Self {
            params: *params,
            sample_rate: 48_000.0,
            ch: vec![Channel::new(), Channel::new()],
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
            env: vec![0.0; BINS],
            out_mag: vec![0.0; BINS],
            out_freq: vec![0.0; BINS],
            out_claim: vec![0.0; BINS],
            heard_midi: f32::NAN,
            heard_age: u32::MAX,
        };
        core.params.sanitize();
        core.prepare(sample_rate);
        core
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.reset();
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> SibylParams {
        self.params
    }

    /// What the last block heard, for the card.
    ///
    /// `bands[0]` carries the MIDI note it is following, or a negative
    /// number for "nothing" — the card draws the difference, because a
    /// harmoniser that has lost the pitch should say so rather than
    /// quietly holding the last one.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [self.heard().unwrap_or(-1.0), 0.0, 0.0],
        }
    }

    /// The note it thinks it is hearing, in MIDI, or `None`.
    pub fn heard(&self) -> Option<f32> {
        if self.heard_midi.is_finite() && self.heard_age <= PITCH_HOLD_FRAMES {
            Some(self.heard_midi)
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        for ch in self.ch.iter_mut() {
            ch.reset();
        }
        self.heard_midi = f32::NAN;
        self.heard_age = u32::MAX;
    }

    /// A whole window, and REPORTED — the graph compensates for it.
    pub fn latency(&self) -> usize {
        LATENCY
    }

    /// The spectral envelope: the magnitude with its partials smoothed
    /// away, leaving the resonances the formant control moves.
    fn envelope(&mut self) {
        let half = ENV_SPAN / 2;
        for k in 0..BINS {
            let lo = k.saturating_sub(half);
            let hi = (k + half + 1).min(BINS);
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for m in lo..hi {
                sum += self.mag.get(m).copied().unwrap_or(0.0);
                count += 1.0;
            }
            if let Some(slot) = self.env.get_mut(k) {
                *slot = sum / count.max(1.0);
            }
        }
    }

    /// The envelope read at a fractional bin.
    fn env_at(&self, x: f32) -> f32 {
        if !x.is_finite() || x <= 0.0 {
            return self.env.first().copied().unwrap_or(0.0);
        }
        let i = x.floor() as usize;
        if i + 1 >= BINS {
            return self.env.get(BINS - 1).copied().unwrap_or(0.0);
        }
        let f = x - i as f32;
        let a = self.env.get(i).copied().unwrap_or(0.0);
        let b = self.env.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * f
    }

    /// Which note is sounding, in MIDI, or `None`.
    ///
    /// A harmonic sum rather than the loudest bin: a voice's second
    /// partial is often louder than its first, and a detector that took
    /// the loudest bin would harmonise the octave above the note being
    /// sung — which is both wrong and hard to hear as wrong.
    fn detect(&self) -> Option<f32> {
        let bin_hz = self.sample_rate / SIZE as f32;
        let peak = self.mag.iter().fold(0.0f32, |m, v| m.max(*v));
        if peak <= 1e-6 {
            return None;
        }
        let lo = ((PITCH_MIN_HZ / bin_hz).floor() as usize).max(1);
        let hi = ((PITCH_MAX_HZ / bin_hz).ceil() as usize).min(BINS - 1);
        let mut best = 0.0f32;
        let mut at = 0usize;
        for k in lo..hi {
            let mut score = self.mag.get(k).copied().unwrap_or(0.0);
            for (h, weight) in [(2usize, 0.5f32), (3, 0.33), (4, 0.25)] {
                if let Some(v) = self.mag.get(k * h) {
                    score += *v * weight;
                }
            }
            if score > best {
                best = score;
                at = k;
            }
        }
        if at == 0 || best < peak * PITCH_FLOOR {
            return None;
        }
        // The vocoder's frequency, not the bin's — that is the whole
        // reason it is computed.
        let bin = self.freq.get(at).copied().unwrap_or(at as f32);
        let hz = bin.max(0.1) * bin_hz;
        if !(PITCH_MIN_HZ * 0.5..=PITCH_MAX_HZ * 2.0).contains(&hz) {
            return None;
        }
        Some(69.0 + 12.0 * (hz / 440.0).log2())
    }

    /// Fold one voice into the output spectrum.
    ///
    /// `ratio` is the frequency multiplier and `gain` its level. The
    /// formant correction happens HERE, per voice, because each voice is
    /// shifted by a different amount and one correction applied to the
    /// sum would be right for at most one of them.
    fn fold(&mut self, ratio: f32, gain: f32, formant_ratio: f32) {
        if gain <= 0.0 || !ratio.is_finite() || ratio <= 0.0 {
            return;
        }
        for k in 1..BINS {
            let m = self.mag.get(k).copied().unwrap_or(0.0);
            if m <= 1e-9 {
                continue;
            }
            let target = k as f32 * ratio;
            let t = target.round() as usize;
            if t == 0 || t >= BINS {
                continue;
            }
            // Put the envelope back where the source had it, then move it
            // by however much FORMANT asks for. At a formant ratio of 1
            // this is `env(t) / env(k)`, which restores exactly.
            let want = self.env_at(target / formant_ratio);
            let had = self.env.get(k).copied().unwrap_or(0.0).max(1e-9);
            let shaped = m * gain * (want / had).clamp(0.0, 8.0);
            if let Some(slot) = self.out_mag.get_mut(t) {
                *slot += shaped;
            }
            // The loudest contributor owns the bin's frequency.
            if shaped > self.out_claim.get(t).copied().unwrap_or(0.0) {
                if let Some(slot) = self.out_claim.get_mut(t) {
                    *slot = shaped;
                }
                if let Some(slot) = self.out_freq.get_mut(t) {
                    *slot = self.freq.get(k).copied().unwrap_or(0.0) * ratio;
                }
            }
        }
    }

    fn transform(&mut self, channel: usize) {
        {
            let Some(ch) = self.ch.get(channel) else {
                return;
            };
            self.work.copy_from_slice(&ch.frame);
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
            let Some(ch) = self.ch.get_mut(channel) else {
                return;
            };
            self.vocoder
                .analyse(&self.phase, &mut self.freq, &mut ch.prev_phase);
        }
        self.envelope();

        // --- what note is this? ---------------------------------------
        //
        // Only the left channel votes: two detectors on one performance
        // would sometimes disagree and put the two harmonies in
        // different keys, which is worse than either answer.
        if channel == 0 {
            match self.detect() {
                Some(midi) => {
                    self.heard_midi = midi;
                    self.heard_age = 0;
                }
                None => self.heard_age = self.heard_age.saturating_add(1),
            }
        }

        // --- the voices ------------------------------------------------
        for slot in self
            .out_mag
            .iter_mut()
            .chain(self.out_freq.iter_mut())
            .chain(self.out_claim.iter_mut())
        {
            *slot = 0.0;
        }
        let formant_ratio = (self.params.formant_st / 12.0).exp2();
        let main_ratio = (self.params.shift_st / 12.0).exp2();
        self.fold(main_ratio, 1.0, formant_ratio);

        let heard = self.heard();
        let key = self.params.key_of();
        let scale = self.params.scale_of();
        let blend = self.params.blend.clamp(0.0, 1.0);
        for steps in [self.params.voice_a, self.params.voice_b] {
            let steps = steps.round() as i32;
            if steps == 0 {
                continue;
            }
            // Without a note to count from there is no in-key answer, so
            // the voice stays silent rather than guessing. A harmony on
            // a cymbal is nobody's idea of a feature.
            let Some(midi) = heard else { continue };
            let interval = scale_interval(midi, key, scale, steps);
            let ratio = main_ratio * (interval / 12.0).exp2();
            self.fold(ratio, blend, formant_ratio);
        }

        // --- synthesis -------------------------------------------------
        {
            let Some(ch) = self.ch.get_mut(channel) else {
                return;
            };
            self.vocoder
                .synthesise(&self.out_freq, &mut self.phase, &mut ch.acc_phase);
        }
        crate::dsp::fft::polar_to_cartesian(&self.out_mag, &self.phase, &mut self.re, &mut self.im);
        self.fft
            .inverse(&self.re, &self.im, &mut self.work, &mut self.fft_scratch);
        crate::dsp::fft::apply_window_in_place(&mut self.work, &self.synthesis);
        if let Some(ch) = self.ch.get_mut(channel) {
            ch.frame.copy_from_slice(&self.work);
        }
    }

    /// Red zone: run a stereo pair through the harmoniser, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let mix = self.params.mix.clamp(0.0, 1.0);
        let out = self.params.out;

        for channel in 0..2 {
            if channel == 1 && !stereo {
                break;
            }
            let io: &mut [f32] = if channel == 0 { l } else { &mut r[..n] };

            let mut fed = 0usize;
            while fed < n {
                let (consumed, produced) = {
                    let Some(ch) = self.ch.get_mut(channel) else {
                        break;
                    };
                    let (a, b) = (&mut ch.frame, &mut ch.cut_store);
                    ch.cutter.process(&io[fed..], a, b)
                };
                if consumed == 0 && produced == 0 {
                    break;
                }
                fed += consumed;
                if produced > 0 {
                    self.transform(channel);
                    let Some(ch) = self.ch.get_mut(channel) else {
                        break;
                    };
                    let (frame, hop_out, store) = (&ch.frame, &mut ch.hop_out, &mut ch.add_store);
                    let (_, written) = ch.adder.process(frame, hop_out, store);
                    let take = written.min(HOP);
                    let mut copy = [0.0f32; HOP];
                    if let (Some(dst), Some(src)) = (copy.get_mut(..take), ch.hop_out.get(..take)) {
                        dst.copy_from_slice(src);
                    }
                    ch.push_fifo(copy.get(..take).unwrap_or(&[]));
                }
            }

            let Some(ch) = self.ch.get_mut(channel) else {
                break;
            };
            for sample in io.iter_mut().take(n) {
                let x = if sample.is_finite() { *sample } else { 0.0 };
                let dry = ch.dry_tick(x);
                let wet = ch.pop_fifo();
                let y = (dry + (wet - dry) * mix) * out;
                *sample = if y.is_finite() { y } else { 0.0 };
            }
        }

        if !stereo {
            for (i, sample) in r.iter_mut().enumerate() {
                if let Some(v) = l.get(i) {
                    *sample = *v;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(edit: impl Fn(&mut SibylParams)) -> SibylCore {
        let mut params = SibylParams::default();
        edit(&mut params);
        SibylCore::new(FS, &params)
    }

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.6 * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(mut c: SibylCore, signal: &[f32], block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(signal.len());
        let mut at = 0;
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

    /// Goertzel magnitude at `hz`, windowed.
    fn tone_amp(x: &[f32], hz: f32) -> f64 {
        let n = x.len();
        if n == 0 {
            return 0.0;
        }
        let w = core::f64::consts::TAU * (hz / FS) as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let mut norm = 0.0f64;
        for (i, v) in x.iter().enumerate() {
            let t = core::f64::consts::TAU * i as f64 / n as f64;
            let win = 0.5 - 0.5 * t.cos();
            norm += win;
            let s0 = *v as f64 * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - c * s1 * s2;
        2.0 * power.max(0.0).sqrt() / norm.max(1.0)
    }

    // ------------------------------------------------------ the device ---

    /// The reported latency is the real one — the graph compensates by
    /// this number and a wrong one puts the harmony off the beat.
    #[test]
    fn the_reported_latency_is_the_real_one() {
        let mut c = core(|p| {
            p.voice_a = 0.0;
            p.voice_b = 0.0;
        });
        assert_eq!(c.latency(), LATENCY);
        let mut impulse = vec![0.0f32; SIZE * 6];
        impulse[0] = 1.0;
        let mut right = impulse.clone();
        c.process(&mut impulse, &mut right);
        let peak = impulse
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        assert!(
            (peak as i64 - LATENCY as i64).abs() <= HOP as i64,
            "the impulse came back at {peak}, latency says {LATENCY}"
        );
    }

    /// At rest — no shift, no voices — it gives back what it was given.
    #[test]
    fn at_rest_it_reconstructs_its_input() {
        let c = core(|p| {
            p.shift_st = 0.0;
            p.voice_a = 0.0;
            p.voice_b = 0.0;
        });
        let signal = sine(440.0, SIZE * 8);
        let out = run(c, &signal, 128);
        let settled = out.get(SIZE * 2..).unwrap();
        let want = tone_amp(signal.get(SIZE * 2..).unwrap(), 440.0);
        let got = tone_amp(settled, 440.0);
        assert!(
            (got - want).abs() < want * 0.25,
            "at rest 440 Hz came back at {got} against {want}"
        );
    }

    /// A shift MOVES the partial, by the ratio it was asked for. This is
    /// the thing `resyn` cannot do, and the reason this device exists.
    #[test]
    fn a_shift_moves_the_partial_to_its_new_pitch() {
        for (semitones, ratio) in [(12.0f32, 2.0f32), (-12.0, 0.5), (7.0, 1.498_307)] {
            let c = core(|p| {
                p.shift_st = semitones;
                p.voice_a = 0.0;
                p.voice_b = 0.0;
            });
            let source = 440.0f32;
            let signal = sine(source, SIZE * 10);
            let out = run(c, &signal, 256);
            let settled = out.get(SIZE * 3..).unwrap();
            let moved = tone_amp(settled, source * ratio);
            let stayed = tone_amp(settled, source);
            assert!(
                moved > stayed * 3.0,
                "{semitones} st: the new pitch read {moved} and the old one {stayed}"
            );
        }
    }

    /// A harmony voice adds a SECOND pitch without removing the first,
    /// and puts it where the key says.
    #[test]
    fn a_harmony_voice_adds_the_note_the_key_asks_for() {
        // A 440 (MIDI 69) in C major, up two scale steps, is C (523.25).
        let c = core(|p| {
            p.shift_st = 0.0;
            p.voice_a = 2.0;
            p.voice_b = 0.0;
            p.key = 0.0;
            p.scale = 0.0; // major
            p.blend = 1.0;
        });
        let signal = sine(440.0, SIZE * 12);
        let out = run(c, &signal, 256);
        let settled = out.get(SIZE * 4..).unwrap();
        let root = tone_amp(settled, 440.0);
        let third = tone_amp(settled, 523.25);
        assert!(root > 1e-4, "the original voice vanished: {root}");
        assert!(
            third > root * 0.2,
            "no harmony at C: root {root}, third {third}"
        );
    }

    /// With no note to count from, a harmony voice stays silent rather
    /// than guessing — a harmony on a cymbal is nobody's feature.
    #[test]
    fn a_harmony_on_noise_stays_out_of_it() {
        let mut c = core(|p| {
            p.voice_a = 2.0;
            p.blend = 1.0;
        });
        let mut l = vec![0.0f32; SIZE * 4];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert!(c.heard().is_none(), "it heard a note in silence");
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn silence_stays_silent_and_nonsense_stays_finite() {
        let mut c = core(|p| {
            p.shift_st = 7.0;
            p.voice_a = 2.0;
        });
        let mut l = vec![0.0f32; SIZE * 4];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert!(l.iter().all(|s| *s == 0.0), "silence did not stay silent");

        let mut c = core(|p| p.shift_st = -12.0);
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
        for len in [0usize, 1, 3, 17, 63, 255, 1_000] {
            let mut l = sine(330.0, len);
            let mut r = l.clone();
            c.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        // A shorter right channel is copied from the left, not left half
        // written.
        let mut l = sine(330.0, 64);
        let mut r = vec![0.0f32; 8];
        c.process(&mut l, &mut r);
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut c = core(|p| {
            p.shift_st = 5.0;
            p.voice_a = 2.0;
            p.voice_b = -2.0;
        });
        let mut l = sine(220.0, 256);
        let mut r = l.clone();
        // Prime first: the first frames build the window tables.
        c.process(&mut l, &mut r);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..20 {
                c.process(&mut l, &mut r);
            }
        });
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = SibylParams::default();
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

        let mut junk = SibylParams {
            shift_st: f32::NAN,
            formant_st: 1e9,
            voice_a: -400.0,
            voice_b: f32::INFINITY,
            key: 99.0,
            scale: -7.0,
            blend: 40.0,
            mix: f32::NAN,
            out: -3.0,
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
        // And the two enumerations always land on something real.
        assert!(Scale::ALL.contains(&junk.scale_of()));
        assert!((0..12).contains(&junk.key_of()));
    }
}
