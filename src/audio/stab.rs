//! STAB — the house chord synth.
//!
//! One key is a chord. The chord, its inversion and its voicing are
//! knobs, so a line of single notes in the sequencer plays a
//! progression, and the same pattern re-voices when the knob turns —
//! which is how the records this is for were made, on a keyboard with
//! one finger and a sampler.
//!
//! The sound is a keyboard's: every note of the chord is a pair of
//! detuned oscillators reading one wavetable that morphs from a sine
//! through an electric piano's few partials to an organ's many (TONE),
//! spread across the field (WIDTH), through a lowpass with a plucked
//! envelope, under an amp envelope. After the sum, the grit: drive into
//! a soft clipper, and a bit crusher with its own clock. STRUM staggers
//! the notes by a few milliseconds, which is the difference between a
//! chord and a stab.

use crate::dsp::adsr::{Adsr, ExpDecay};
use crate::dsp::filters::{Mode as FilterMode, Svf};
use crate::dsp::interp::linear;
use crate::dsp::lofi::Downsampler;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::stab as p;

pub const VOICES: usize = 8;
const NOTES: usize = p::NOTES;
/// The wavetable's length, plus one guard sample for the read past the end.
const TABLE: usize = 2048;
/// A block is filtered in runs of this many, so the pluck's envelope
/// moves the cutoff smoothly rather than once a block.
const HOP: usize = 32;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StabParams {
    pub chord: f32,
    pub inversion: f32,
    pub open: f32,
    pub octave: f32,
    pub strum_ms: f32,
    pub tone: f32,
    pub detune_ct: f32,
    pub width: f32,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub cutoff_hz: f32,
    pub reso: f32,
    pub env_oct: f32,
    pub fenv_ms: f32,
    pub drive: f32,
    pub crush_bits: f32,
    pub rate_hz: f32,
    pub level: f32,
    pub omit: f32,
}

impl Default for StabParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            chord: d(p::CHORD),
            inversion: d(p::INVERSION),
            open: d(p::OPEN),
            octave: d(p::OCTAVE),
            strum_ms: d(p::STRUM),
            tone: d(p::TONE),
            detune_ct: d(p::DETUNE),
            width: d(p::WIDTH),
            attack_ms: d(p::ATTACK),
            decay_ms: d(p::DECAY),
            sustain: d(p::SUSTAIN),
            release_ms: d(p::RELEASE),
            cutoff_hz: d(p::CUTOFF),
            reso: d(p::RESO),
            env_oct: d(p::ENV),
            fenv_ms: d(p::FENV),
            drive: d(p::DRIVE),
            crush_bits: d(p::CRUSH),
            rate_hz: d(p::RATE),
            level: d(p::LEVEL),
            omit: d(p::OMIT),
        }
    }
}

impl StabParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::CHORD => self.chord = value,
            p::INVERSION => self.inversion = value,
            p::OPEN => self.open = value,
            p::OCTAVE => self.octave = value,
            p::STRUM => self.strum_ms = value,
            p::TONE => self.tone = value,
            p::DETUNE => self.detune_ct = value,
            p::WIDTH => self.width = value,
            p::ATTACK => self.attack_ms = value,
            p::DECAY => self.decay_ms = value,
            p::SUSTAIN => self.sustain = value,
            p::RELEASE => self.release_ms = value,
            p::CUTOFF => self.cutoff_hz = value,
            p::RESO => self.reso = value,
            p::ENV => self.env_oct = value,
            p::FENV => self.fenv_ms = value,
            p::DRIVE => self.drive = value,
            p::CRUSH => self.crush_bits = value,
            p::RATE => self.rate_hz = value,
            p::LEVEL => self.level = value,
            p::OMIT => self.omit = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::CHORD => self.chord,
            p::INVERSION => self.inversion,
            p::OPEN => self.open,
            p::OCTAVE => self.octave,
            p::STRUM => self.strum_ms,
            p::TONE => self.tone,
            p::DETUNE => self.detune_ct,
            p::WIDTH => self.width,
            p::ATTACK => self.attack_ms,
            p::DECAY => self.decay_ms,
            p::SUSTAIN => self.sustain,
            p::RELEASE => self.release_ms,
            p::CUTOFF => self.cutoff_hz,
            p::RESO => self.reso,
            p::ENV => self.env_oct,
            p::FENV => self.fenv_ms,
            p::DRIVE => self.drive,
            p::CRUSH => self.crush_bits,
            p::RATE => self.rate_hz,
            p::LEVEL => self.level,
            p::OMIT => self.omit,
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

    /// The chord's notes as semitones above the key, voiced by the knobs.
    pub fn voicing(&self) -> ([i32; NOTES], usize) {
        p::voicing(
            self.chord.round().max(0.0) as usize,
            self.inversion.round().max(0.0) as usize,
            self.open.round() >= 1.0,
            self.omit.round().max(0.0) as usize,
        )
    }
}

/// One held key: a chord of notes, each a detuned pair.
struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    age: u64,
    count: usize,
    /// Phase per note, per side of the pair, in table turns.
    phase: [[f32; 2]; NOTES],
    inc: [[f32; 2]; NOTES],
    /// Samples since the gate, and when each note is allowed to start.
    elapsed: u64,
    start: [u64; NOTES],
    amp: Adsr,
    pluck: ExpDecay,
    filter_l: Svf,
    filter_r: Svf,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            held: false,
            pitch: 0,
            age: 0,
            count: 0,
            phase: [[0.0; 2]; NOTES],
            inc: [[0.0; 2]; NOTES],
            elapsed: 0,
            start: [0; NOTES],
            amp: Adsr::new(),
            pluck: ExpDecay::new(),
            filter_l: Svf::new(),
            filter_r: Svf::new(),
        }
    }
}

/// The instrument.
pub struct StabVoices {
    params: StabParams,
    base: StabParams,
    sample_rate: f32,
    /// The three tables TONE morphs across: a sine, a keyboard, an organ.
    sine: Vec<f32>,
    keys: Vec<f32>,
    organ: Vec<f32>,
    voices: Vec<Voice>,
    /// Scratch: one chord's two sides, the envelope, and the sum.
    vl: Vec<f32>,
    vr: Vec<f32>,
    env: Vec<f32>,
    pluck: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    stash: Vec<f32>,
    shaper: Waveshaper,
    crush_l: Downsampler,
    crush_r: Downsampler,
    said_voices: f32,
}

/// The table read for TONE: sine to keys to organ.
#[inline(always)]
fn read(sine: &[f32], keys: &[f32], organ: &[f32], phase: f32, tone: f32) -> f32 {
    let pos = phase.fract().max(0.0) * TABLE as f32;
    let i = (pos as usize).min(TABLE - 1);
    let frac = pos - i as f32;
    let at = |table: &[f32]| {
        linear(
            table.get(i).copied().unwrap_or(0.0),
            table.get(i + 1).copied().unwrap_or(0.0),
            frac,
        )
    };
    if tone < 0.5 {
        let t = tone * 2.0;
        at(sine) * (1.0 - t) + at(keys) * t
    } else {
        let t = (tone - 0.5) * 2.0;
        at(keys) * (1.0 - t) + at(organ) * t
    }
}

/// Fill a wavetable with partials, normalised to a peak of one.
fn build(table: &mut [f32], partials: &[f32]) {
    let n = table.len().saturating_sub(1).max(1);
    let mut peak = 0.0f32;
    for (i, slot) in table.iter_mut().take(n).enumerate() {
        let t = i as f32 / n as f32 * core::f32::consts::TAU;
        let mut v = 0.0;
        for (k, amp) in partials.iter().enumerate() {
            v += amp * (t * (k + 1) as f32).sin();
        }
        *slot = v;
        peak = peak.max(v.abs());
    }
    if peak > 0.0 {
        for slot in table.iter_mut().take(n) {
            *slot /= peak;
        }
    }
    let first = table.first().copied().unwrap_or(0.0);
    if let Some(last) = table.last_mut() {
        *last = first;
    }
}

impl StabVoices {
    pub fn new(sample_rate: f32, block: usize, params: StabParams) -> Self {
        let mut sine = vec![0.0; TABLE + 1];
        let mut keys = vec![0.0; TABLE + 1];
        let mut organ = vec![0.0; TABLE + 1];
        build(&mut sine, &[1.0]);
        // An electric piano's tine: the fundamental and a quick fall.
        build(&mut keys, &[1.0, 0.55, 0.28, 0.12, 0.06]);
        // Drawbars pulled: the harmonics stay strong.
        build(&mut organ, &[1.0, 0.8, 0.6, 0.5, 0.35, 0.25, 0.2, 0.15]);
        let n = block.max(HOP);
        let mut me = Self {
            params,
            base: params,
            sample_rate: 48_000.0,
            sine,
            keys,
            organ,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            vl: vec![0.0; n],
            vr: vec![0.0; n],
            env: vec![0.0; n],
            pluck: vec![0.0; n],
            left: vec![0.0; n],
            right: vec![0.0; n],
            stash: vec![0.0; n],
            shaper: Waveshaper::new(),
            crush_l: Downsampler::new(),
            crush_r: Downsampler::new(),
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.prepare_at(sample_rate);
        me
    }

    /// Green zone.
    pub fn prepare_at(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.crush_l.prepare(self.sample_rate);
        self.crush_r.prepare(self.sample_rate);
        self.settle();
    }

    /// Everything the knobs decide that is not per sample.
    fn settle(&mut self) {
        let sr = self.sample_rate;
        let p = self.params;
        for v in self.voices.iter_mut() {
            v.amp
                .prepare(sr, p.attack_ms, p.decay_ms, p.sustain, p.release_ms);
            v.pluck.prepare(sr, p.fenv_ms);
        }
        self.shaper
            .configure(ShapeMode::SoftClip, p.drive, 0.0, 1.0);
        self.crush_l.set_bits(p.crush_bits);
        self.crush_r.set_bits(p.crush_bits);
        self.crush_l.set_rate(p.rate_hz);
        self.crush_r.set_rate(p.rate_hz);
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

    pub fn params(&self) -> &StabParams {
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
            v.amp.reset();
            v.pluck.reset();
            v.filter_l.reset();
            v.filter_r.reset();
        }
        self.crush_l.reset();
        self.crush_r.reset();
        self.said_voices = 0.0;
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            if v.held {
                v.held = false;
                v.amp.gate_off();
            }
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut() {
            if v.held && v.pitch == pitch {
                v.held = false;
                v.amp.gate_off();
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
        let (notes, count) = self.params.voicing();
        let octave = self.params.octave.round() * 12.0;
        let detune = self.params.detune_ct / 100.0;
        let strum = (self.params.strum_ms.max(0.0) / 1000.0 * self.sample_rate) as u64;
        let sr = self.sample_rate;
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        v.active = true;
        v.held = true;
        v.pitch = pitch;
        v.age = age;
        v.count = count;
        v.elapsed = 0;
        for i in 0..NOTES {
            let semis = f32::from(pitch) + octave + notes.get(i).copied().unwrap_or(0) as f32;
            for (side, sign) in [(0usize, -0.5f32), (1, 0.5)] {
                let hz = 440.0 * ((semis - 69.0 + sign * detune) / 12.0).exp2();
                let inc = (hz / sr).clamp(0.0, 0.45);
                v.inc[i][side] = inc;
                // Every note starts from its own place on the cycle so a
                // chord does not begin as one thick click.
                v.phase[i][side] = (i as f32 * 0.17 + side as f32 * 0.31).fract();
            }
            v.start[i] = strum * i as u64;
        }
        let _ = vel;
        v.filter_l.reset();
        v.filter_r.reset();
        v.amp.gate_on();
        v.pluck.trigger(1.0);
    }

    /// Red zone: render the LEFT channel, stash the right. Any length:
    /// the scratch is a block, and a longer run is walked in blocks.
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
        for slot in self
            .left
            .iter_mut()
            .take(n)
            .chain(self.right.iter_mut().take(n))
        {
            *slot = 0.0;
        }
        let tone = self.params.tone.clamp(0.0, 1.0);
        let (gl, gr) = crate::dsp::pan::spread(-self.params.width.clamp(0.0, 1.0));
        let cutoff = self.params.cutoff_hz;
        let reso = self.params.reso;
        let env_oct = self.params.env_oct;
        let nyquist = self.sample_rate * 0.45;
        let mut sounding = 0usize;
        let count_voices = self.voices.len();
        let (sine, keys, organ) = (&self.sine, &self.keys, &self.organ);
        for vi in 0..count_voices {
            if !self.voices[vi].active {
                continue;
            }
            let mut done = 0usize;
            while done < n {
                let take = (n - done).min(HOP).min(self.vl.len());
                if take == 0 {
                    break;
                }
                // Oscillators, each note a detuned pair spread across the field.
                for s in 0..take {
                    let v = &mut self.voices[vi];
                    let mut l = 0.0f32;
                    let mut r = 0.0f32;
                    for i in 0..v.count {
                        if v.elapsed < v.start[i] {
                            continue;
                        }
                        let a = v.phase[i][0];
                        let b = v.phase[i][1];
                        v.phase[i][0] = (a + v.inc[i][0]).fract();
                        v.phase[i][1] = (b + v.inc[i][1]).fract();
                        let (sa, sb) = (
                            read(sine, keys, organ, a, tone),
                            read(sine, keys, organ, b, tone),
                        );
                        l += sa * gl + sb * gr;
                        r += sa * gr + sb * gl;
                    }
                    self.voices[vi].elapsed += 1;
                    let scale = 0.35 / (self.voices[vi].count.max(1) as f32).sqrt();
                    if let Some(slot) = self.vl.get_mut(s) {
                        *slot = l * scale;
                    }
                    if let Some(slot) = self.vr.get_mut(s) {
                        *slot = r * scale;
                    }
                }
                // The pluck: the cutoff rides the decay, in octaves.
                let v = &mut self.voices[vi];
                if let Some(pluck) = self.pluck.get_mut(..take) {
                    v.pluck.process(pluck);
                    let lift = pluck.first().copied().unwrap_or(0.0) * env_oct;
                    let hz = (cutoff * lift.exp2()).clamp(20.0, nyquist);
                    v.filter_l.prepare(self.sample_rate, hz, reso);
                    v.filter_r.prepare(self.sample_rate, hz, reso);
                }
                if let (Some(l), Some(r)) = (self.vl.get_mut(..take), self.vr.get_mut(..take)) {
                    v.filter_l.process(l, FilterMode::Lowpass);
                    v.filter_r.process(r, FilterMode::Lowpass);
                }
                // The amp envelope, into the sum.
                if let Some(env) = self.env.get_mut(..take) {
                    v.amp.process(env);
                    for s in 0..take {
                        let e = env.get(s).copied().unwrap_or(0.0);
                        if let (Some(dl), Some(sl)) = (self.left.get_mut(done + s), self.vl.get(s))
                        {
                            *dl += sl * e;
                        }
                        if let (Some(dr), Some(sr)) = (self.right.get_mut(done + s), self.vr.get(s))
                        {
                            *dr += sr * e;
                        }
                    }
                }
                done += take;
            }
            let v = &mut self.voices[vi];
            if !v.amp.active() {
                v.active = false;
                v.held = false;
            } else {
                sounding += 1;
            }
        }
        self.said_voices = sounding as f32;

        // The grit, on the sum: drive, then the crusher.
        if let (Some(l), Some(r)) = (self.left.get_mut(..n), self.right.get_mut(..n)) {
            self.shaper.process(l);
            self.shaper.process(r);
            self.crush_l.process(l);
            self.crush_r.process(r);
        }
        let level = self.params.level;
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

    fn run(voices: &mut StabVoices, n: usize) -> Vec<f32> {
        let mut out = vec![0.0; n];
        let mut ramp = Ramp::across(1.0, 1.0, n);
        voices.render(&mut out, 0, &mut ramp);
        out
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut voices = StabVoices::new(FS, 256, StabParams::default());
        for def in p::TABLE {
            voices.set_param(def.id, def.max);
            assert_eq!(voices.params().get(def.id), Some(def.max), "{}", def.name);
        }
    }

    #[test]
    fn one_key_is_a_chord_and_the_chord_follows_the_knobs() {
        let mut params = StabParams::default();
        params.chord = 3.0; // m7
        params.inversion = 1.0;
        let voices = StabVoices::new(FS, 256, params);
        let (notes, count) = voices.params().voicing();
        assert_eq!(&notes[..count], &[3, 7, 10, 12]);
        let mut voices = StabVoices::new(FS, 256, params);
        assert!(run(&mut voices, 256).iter().all(|x| *x == 0.0));
        voices.note_on(60, 100, 1);
        let block = run(&mut voices, 4_800);
        assert!(
            rms(&block) > 0.02,
            "the chord made no sound: {}",
            rms(&block)
        );
        assert!(block.iter().all(|x| x.is_finite() && x.abs() <= 1.5));
        assert_eq!(voices.readout().bands[0], 1.0);
        // The right side is not the left: the pair is spread.
        let right = voices.right(4_800).to_vec();
        assert!(
            block
                .iter()
                .zip(right.iter())
                .any(|(l, r)| (l - r).abs() > 1e-4)
        );
        voices.note_off(60);
        let _ = run(&mut voices, 48_000);
        assert_eq!(
            voices.readout().bands[0],
            0.0,
            "released, yet still sounding after a second"
        );
    }

    #[test]
    fn tone_drive_and_crush_change_the_sound_and_all_sound_off_silences() {
        let mut params = StabParams::default();
        params.sustain = 1.0;
        params.decay_ms = 5.0;
        let mut clean = StabVoices::new(FS, 256, params);
        clean.note_on(48, 100, 1);
        let _ = run(&mut clean, 2_000);
        let a = run(&mut clean, 2_000);
        params.tone = 1.0;
        params.drive = 20.0;
        params.crush_bits = 4.0;
        params.rate_hz = 6_000.0;
        let mut dirty = StabVoices::new(FS, 256, params);
        dirty.note_on(48, 100, 1);
        let _ = run(&mut dirty, 2_000);
        let b = run(&mut dirty, 2_000);
        assert!(a.iter().zip(b.iter()).any(|(x, y)| (x - y).abs() > 0.05));
        assert!(b.iter().all(|x| x.is_finite() && x.abs() <= 1.5));
        dirty.all_sound_off();
        assert!(run(&mut dirty, 256).iter().all(|x| *x == 0.0));
    }

    #[test]
    fn a_strum_delays_the_upper_notes() {
        let mut params = StabParams::default();
        params.strum_ms = 40.0;
        params.attack_ms = 0.0;
        let mut voices = StabVoices::new(FS, 256, params);
        voices.note_on(60, 100, 1);
        let early = run(&mut voices, 480);
        let _ = run(&mut voices, 48_000 / 4);
        let late = run(&mut voices, 480);
        assert!(rms(&early) > 0.0, "the first note did not start at once");
        assert!(
            voices.voices[0].elapsed > voices.voices[0].start[voices.voices[0].count - 1],
            "the last note never started"
        );
        assert!(rms(&late) > rms(&early) * 0.5);
    }
}
