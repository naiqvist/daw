//! BRICK — the drum one-shot sampler with an opinion.
//!
//! One file, played from START at the note, forward or backward, and
//! done: no loop, no slices, no keyboard unless asked. What it adds is
//! the character a drum machine has and a clean sampler does not. A
//! PUNCH that leans on the first ten milliseconds. A BODY bell in the
//! lows and a SNAP shelf in the highs, so a thin kick gets a chest and
//! a dull snare gets its crack. A DROP that bends the pitch down from
//! the strike, the 808's whole trick. A decay whose CURVE falls the way
//! a drum's does. And the dirt: a bit depth and a converter clock that
//! default to a classic 12-bit machine's, and a GRIT drive after them.
//! CUT chokes the last hit with the next, as a closed hat should.
//!
//! The file is loaded when the node is built, as the sampler's is; the
//! voices only read it.

use crate::audio::material::Material;
use crate::dsp::filters::{BandShape, EqBand, Mode as FilterMode, Svf};
use crate::dsp::interp::hermite_at;
use crate::dsp::lofi::Downsampler;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::brick as p;

pub const VOICES: usize = 6;
const HOP: usize = 32;
/// How long the punch leans, in seconds, and how hard at most.
const PUNCH_TAU: f32 = 0.008;
const PUNCH_GAIN: f32 = 2.0;
/// A choked hit fades over this many samples rather than stopping dead.
const CUT_SAMPLES: u32 = 192;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BrickParams {
    pub tune: f32,
    pub fine: f32,
    pub key: f32,
    pub start: f32,
    pub reverse: f32,
    pub attack: f32,
    pub decay: f32,
    pub curve: f32,
    pub drop: f32,
    pub drop_ms: f32,
    pub punch: f32,
    pub body: f32,
    pub body_hz: f32,
    pub snap: f32,
    pub bits: f32,
    pub rate: f32,
    pub grit: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub choke: f32,
    pub velocity: f32,
    pub level: f32,
}

impl Default for BrickParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            tune: d(p::TUNE),
            fine: d(p::FINE),
            key: d(p::KEY),
            start: d(p::START),
            reverse: d(p::REVERSE),
            attack: d(p::ATTACK),
            decay: d(p::DECAY),
            curve: d(p::CURVE),
            drop: d(p::DROP),
            drop_ms: d(p::DROP_MS),
            punch: d(p::PUNCH),
            body: d(p::BODY),
            body_hz: d(p::BODY_HZ),
            snap: d(p::SNAP),
            bits: d(p::BITS),
            rate: d(p::RATE),
            grit: d(p::GRIT),
            cutoff: d(p::CUTOFF),
            reso: d(p::RESO),
            choke: d(p::CHOKE),
            velocity: d(p::VELOCITY),
            level: d(p::LEVEL),
        }
    }
}

impl BrickParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::TUNE => self.tune = value,
            p::FINE => self.fine = value,
            p::KEY => self.key = value,
            p::START => self.start = value,
            p::REVERSE => self.reverse = value,
            p::ATTACK => self.attack = value,
            p::DECAY => self.decay = value,
            p::CURVE => self.curve = value,
            p::DROP => self.drop = value,
            p::DROP_MS => self.drop_ms = value,
            p::PUNCH => self.punch = value,
            p::BODY => self.body = value,
            p::BODY_HZ => self.body_hz = value,
            p::SNAP => self.snap = value,
            p::BITS => self.bits = value,
            p::RATE => self.rate = value,
            p::GRIT => self.grit = value,
            p::CUTOFF => self.cutoff = value,
            p::RESO => self.reso = value,
            p::CHOKE => self.choke = value,
            p::VELOCITY => self.velocity = value,
            p::LEVEL => self.level = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::TUNE => self.tune,
            p::FINE => self.fine,
            p::KEY => self.key,
            p::START => self.start,
            p::REVERSE => self.reverse,
            p::ATTACK => self.attack,
            p::DECAY => self.decay,
            p::CURVE => self.curve,
            p::DROP => self.drop,
            p::DROP_MS => self.drop_ms,
            p::PUNCH => self.punch,
            p::BODY => self.body,
            p::BODY_HZ => self.body_hz,
            p::SNAP => self.snap,
            p::BITS => self.bits,
            p::RATE => self.rate,
            p::GRIT => self.grit,
            p::CUTOFF => self.cutoff,
            p::RESO => self.reso,
            p::CHOKE => self.choke,
            p::VELOCITY => self.velocity,
            p::LEVEL => self.level,
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

    pub fn tracks_key(&self) -> bool {
        self.key.round() >= 1.0
    }

    pub fn reversed(&self) -> bool {
        self.reverse.round() >= 1.0
    }

    pub fn cuts(&self) -> bool {
        self.choke.round() < 1.0
    }
}

struct Voice {
    active: bool,
    age: u64,
    vel: f32,
    /// Where the read head stands, in frames, and how far it moves a
    /// sample: negative for a reversed hit.
    pos: f64,
    inc: f64,
    elapsed: u64,
    /// A choked hit: samples of fade left.
    cut: u32,
    filter_l: Svf,
    filter_r: Svf,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            age: 0,
            vel: 1.0,
            pos: 0.0,
            inc: 1.0,
            elapsed: 0,
            cut: 0,
            filter_l: Svf::new(),
            filter_r: Svf::new(),
        }
    }
}

/// The instrument.
pub struct BrickVoices {
    params: BrickParams,
    base: BrickParams,
    sample_rate: f32,
    material: Material,
    voices: Vec<Voice>,
    block_l: Vec<f32>,
    block_r: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    stash: Vec<f32>,
    body_l: EqBand,
    body_r: EqBand,
    snap_l: EqBand,
    snap_r: EqBand,
    crush_l: Downsampler,
    crush_r: Downsampler,
    shaper: Waveshaper,
    said_voices: f32,
}

impl BrickVoices {
    /// Green zone: the file is here already, loaded at compile.
    pub fn new(sample_rate: f32, block: usize, params: BrickParams, material: Material) -> Self {
        let n = block.max(HOP);
        let mut me = Self {
            params,
            base: params,
            sample_rate: 48_000.0,
            material,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            block_l: vec![0.0; n],
            block_r: vec![0.0; n],
            left: vec![0.0; n],
            right: vec![0.0; n],
            stash: vec![0.0; n],
            body_l: EqBand::new(),
            body_r: EqBand::new(),
            snap_l: EqBand::new(),
            snap_r: EqBand::new(),
            crush_l: Downsampler::new(),
            crush_r: Downsampler::new(),
            shaper: Waveshaper::new(),
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.prepare_at(sample_rate);
        me
    }

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
        // BODY: a bell up to twelve decibels; SNAP: a shelf up to eight.
        for band in [&mut self.body_l, &mut self.body_r] {
            band.prepare(sr, p.body_hz, 1.2, p.body * 12.0, BandShape::Bell);
        }
        for band in [&mut self.snap_l, &mut self.snap_r] {
            band.prepare(sr, 4_000.0, 0.7, p.snap * 8.0, BandShape::HighShelf);
        }
        self.crush_l.set_bits(p.bits);
        self.crush_r.set_bits(p.bits);
        self.crush_l.set_rate(p.rate);
        self.crush_r.set_rate(p.rate);
        self.shaper.configure(ShapeMode::SoftClip, p.grit, 0.0, 1.0);
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

    pub fn params(&self) -> &BrickParams {
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
            v.cut = 0;
            v.filter_l.reset();
            v.filter_r.reset();
        }
        self.crush_l.reset();
        self.crush_r.reset();
        self.said_voices = 0.0;
    }

    /// A one-shot has nothing to release: the hit runs its course.
    pub fn release_all(&mut self) {}

    pub fn note_off(&mut self, _pitch: u8) {}

    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        if self.material.is_empty() {
            return;
        }
        // CUT: the hit sounding is faded out for the new one.
        if self.params.cuts() {
            for v in self.voices.iter_mut() {
                if v.active && v.cut == 0 {
                    v.cut = CUT_SAMPLES;
                }
            }
        }
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
        let p = self.params;
        let frames = self.material.frames as f64;
        let start = f64::from(p.start.clamp(0.0, 1.0)) * (frames - 1.0).max(0.0);
        let key = if p.tracks_key() {
            (f32::from(pitch) - 60.0) / 12.0
        } else {
            0.0
        };
        let ratio = (key + p.tune / 12.0 + p.fine / 1200.0).exp2()
            * (self.material.sample_rate.max(1) as f32 / self.sample_rate);
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        v.active = true;
        v.age = age;
        v.vel = (f32::from(vel) / 127.0).clamp(0.0, 1.0);
        v.elapsed = 0;
        v.cut = 0;
        v.inc = if ratio.is_finite() {
            f64::from(ratio.clamp(0.05, 16.0))
        } else {
            1.0
        };
        v.pos = if p.reversed() {
            v.inc = -v.inc;
            (frames - 1.0).max(0.0) - start
        } else {
            start
        };
        v.filter_l.reset();
        v.filter_r.reset();
    }

    /// Red zone: render the LEFT channel, stash the right. Any length.
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
        let p = self.params;
        let sr = self.sample_rate;
        let nyquist = sr * 0.45;
        let frames = self.material.frames as f64;
        let last = (frames - 1.0).max(0.0);
        let left_ch = self.material.channel(0);
        let right_ch = if self.material.channels > 1 {
            self.material.channel(1)
        } else {
            left_ch
        };
        let attack = (p.attack.max(0.0) / 1000.0 * sr).max(1.0);
        let decay = (p.decay.max(1.0) / 1000.0 * sr).max(1.0);
        let drop_tau = p.drop_ms.max(1.0) / 1000.0;
        let mut sounding = 0usize;
        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
            let mut done = 0usize;
            let mut finished = false;
            while done < n && !finished {
                let take = (n - done).min(HOP).min(self.block_l.len());
                if take == 0 {
                    break;
                }
                let v = &mut self.voices[vi];
                let t = v.elapsed as f32 / sr;
                // DROP: the pitch falls from `drop` semitones above.
                let bend = if p.drop > 0.0 {
                    (p.drop * (-t / drop_tau).exp() / 12.0).exp2()
                } else {
                    1.0
                };
                let touch = 1.0 - p.velocity + p.velocity * v.vel;
                for s in 0..take {
                    let e = v.elapsed as f32;
                    // The envelope: a short attack, then the fall.
                    let rising = e < attack;
                    let env = if rising {
                        e / attack
                    } else {
                        p::fall(p.curve, (e - attack) / decay)
                    };
                    // PUNCH: a lean on the first milliseconds.
                    let punch = 1.0 + p.punch * PUNCH_GAIN * (-(e / sr) / PUNCH_TAU).exp();
                    // A choked hit fades.
                    let cut = if v.cut > 0 {
                        v.cut -= 1;
                        v.cut as f32 / CUT_SAMPLES as f32
                    } else {
                        1.0
                    };
                    let g = env * punch * touch * cut;
                    let l = hermite_at(left_ch, v.pos) * g;
                    let r = hermite_at(right_ch, v.pos) * g;
                    if let Some(slot) = self.block_l.get_mut(s) {
                        *slot = l;
                    }
                    if let Some(slot) = self.block_r.get_mut(s) {
                        *slot = r;
                    }
                    v.pos += v.inc * f64::from(bend);
                    v.elapsed += 1;
                    let out_of_file = v.pos < 0.0 || v.pos > last;
                    if out_of_file || (!rising && env <= 0.0) || (v.cut == 0 && cut < 1.0) {
                        finished = true;
                        // The rest of the hop is silence.
                        for rest in s + 1..take {
                            if let Some(slot) = self.block_l.get_mut(rest) {
                                *slot = 0.0;
                            }
                            if let Some(slot) = self.block_r.get_mut(rest) {
                                *slot = 0.0;
                            }
                        }
                        break;
                    }
                }
                let hz = p.cutoff.clamp(20.0, nyquist);
                v.filter_l.prepare(sr, hz, p.reso);
                v.filter_r.prepare(sr, hz, p.reso);
                if let (Some(bl), Some(br)) =
                    (self.block_l.get_mut(..take), self.block_r.get_mut(..take))
                {
                    if p.cutoff < 19_000.0 {
                        v.filter_l.process(bl, FilterMode::Lowpass);
                        v.filter_r.process(br, FilterMode::Lowpass);
                    }
                    for i in 0..take {
                        if let Some(slot) = self.left.get_mut(done + i) {
                            *slot += bl[i];
                        }
                        if let Some(slot) = self.right.get_mut(done + i) {
                            *slot += br[i];
                        }
                    }
                }
                done += take;
            }
            let v = &mut self.voices[vi];
            if finished {
                v.active = false;
            } else {
                sounding += 1;
            }
        }
        self.said_voices = sounding as f32;
        // The character, on the sum: the body and the snap, the dirt,
        // the grit.
        if let (Some(l), Some(r)) = (self.left.get_mut(..n), self.right.get_mut(..n)) {
            if p.body > 0.001 {
                self.body_l.process(l);
                self.body_r.process(r);
            }
            if p.snap > 0.001 {
                self.snap_l.process(l);
                self.snap_r.process(r);
            }
            self.crush_l.process(l);
            self.crush_r.process(r);
            if p.grit > 1.001 {
                self.shaper.process(l);
                self.shaper.process(r);
            }
        }
        let level = p.level;
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
    use std::sync::Arc;

    const FS: f32 = 48_000.0;

    /// A tenth of a second of a decaying 200 Hz tone with a click on it.
    fn hit() -> Material {
        let frames = 4_800usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / FS;
                let env = (-t * 20.0).exp();
                0.8 * env * (t * 200.0 * core::f32::consts::TAU).sin()
                    + if i < 48 { 0.3 } else { 0.0 }
            })
            .collect();
        Material {
            samples: Arc::new(samples),
            channels: 1,
            frames: frames as u64,
            source: "/kits/kick.wav".into(),
            sample_rate: 48_000,
            original_rate: 48_000,
            truncated: false,
        }
    }

    fn voices(edit: impl Fn(&mut BrickParams)) -> BrickVoices {
        let mut params = BrickParams::default();
        params.bits = 16.0;
        params.rate = 48_000.0;
        params.body = 0.0;
        params.snap = 0.0;
        params.punch = 0.0;
        params.grit = 1.0;
        params.decay = 4_000.0;
        params.curve = 0.0;
        params.velocity = 0.0;
        edit(&mut params);
        BrickVoices::new(FS, 256, params, hit())
    }

    fn run(voices: &mut BrickVoices, n: usize) -> Vec<f32> {
        let mut out = vec![0.0; n];
        let mut ramp = Ramp::across(1.0, 1.0, n);
        voices.render(&mut out, 0, &mut ramp);
        out
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len().max(1) as f32).sqrt()
    }

    fn crossings(xs: &[f32]) -> usize {
        xs.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn every_parameter_reaches_its_field_and_an_empty_file_is_silent() {
        let mut v = voices(|_| {});
        for def in p::TABLE {
            v.set_param(def.id, def.max);
            assert_eq!(v.params().get(def.id), Some(def.max), "{}", def.name);
        }
        let mut empty = BrickVoices::new(FS, 256, BrickParams::default(), Material::empty());
        empty.note_on(60, 100, 1);
        assert!(run(&mut empty, 512).iter().all(|x| *x == 0.0));
        assert_eq!(empty.readout().bands[0], 0.0);
    }

    #[test]
    fn a_hit_plays_once_forward_or_backward_from_its_start_and_stays_put_across_keys() {
        let mut v = voices(|_| {});
        assert!(run(&mut v, 256).iter().all(|x| *x == 0.0));
        v.note_on(36, 100, 1);
        let a = run(&mut v, 4_800);
        assert!(rms(&a) > 0.05 && a.iter().all(|x| x.is_finite()));
        let _ = run(&mut v, 4_800);
        assert_eq!(v.readout().bands[0], 0.0, "a one-shot outlived its file");
        // Fixed key: the same hit at any note.
        let mut lo = voices(|_| {});
        let mut hi = voices(|_| {});
        lo.note_on(36, 100, 1);
        hi.note_on(72, 100, 1);
        let (a, b) = (run(&mut lo, 2_400), run(&mut hi, 2_400));
        assert!(a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6));
        // Tracking: an octave up reads twice as fast.
        let mut hi = voices(|p| p.key = 1.0);
        hi.note_on(72, 100, 1);
        let b = run(&mut hi, 2_400);
        assert!(
            crossings(&b) > crossings(&a) * 3 / 2,
            "{} vs {}",
            crossings(&b),
            crossings(&a)
        );
        // Reverse: the click at the head of the file arrives last.
        let mut back = voices(|p| p.reverse = 1.0);
        back.note_on(36, 100, 1);
        let r = run(&mut back, 4_800);
        let head = rms(&r[..480]);
        let tail = rms(&r[4_200..4_700]);
        assert!(
            tail > head,
            "reversed, the loud head should come last: {head} then {tail}"
        );
        // Start halfway: the file ends sooner.
        let mut late = voices(|p| p.start = 0.5);
        late.note_on(36, 100, 1);
        let _ = run(&mut late, 2_500);
        assert_eq!(late.readout().bands[0], 0.0);
    }

    #[test]
    fn the_character_knobs_do_what_they_say() {
        let render = |edit: &dyn Fn(&mut BrickParams)| {
            let mut v = voices(edit);
            v.note_on(36, 100, 1);
            run(&mut v, 4_800)
        };
        let plain = render(&|_| {});
        // Punch: louder in the first ten milliseconds, not after.
        let punched = render(&|p| p.punch = 1.0);
        assert!(rms(&punched[..240]) > rms(&plain[..240]) * 1.5);
        assert!((rms(&punched[2_400..]) - rms(&plain[2_400..])).abs() < rms(&plain[2_400..]) * 0.2);
        // Drop: the pitch starts higher.
        let dropped = render(&|p| {
            p.drop = 24.0;
            p.drop_ms = 200.0;
        });
        assert!(crossings(&dropped[..960]) > crossings(&plain[..960]) + 4);
        // Decay: a short one is gone by the end.
        let short = render(&|p| {
            p.decay = 30.0;
            p.curve = 1.0;
        });
        assert!(rms(&short[2_400..]) < 1e-3 && rms(&short[..480]) > 0.05);
        // Body: more energy at the bell; snap: more at the top.
        let bodied = render(&|p| {
            p.body = 1.0;
            p.body_hz = 200.0;
        });
        assert!(rms(&bodied) > rms(&plain) * 1.3);
        let brightness = |xs: &[f32]| {
            let mut hp = 0.0f32;
            let mut all = 0.0f32;
            for w in xs.windows(2) {
                hp += (w[1] - w[0]).powi(2);
                all += w[1] * w[1];
            }
            if all > 0.0 { hp / all } else { 0.0 }
        };
        let snapped = render(&|p| p.snap = 1.0);
        assert!(brightness(&snapped) > brightness(&plain) * 1.2);
        // Dirt: bits and a slow clock change the sound; grit too.
        let dirty = render(&|p| {
            p.bits = 6.0;
            p.rate = 12_000.0;
            p.grit = 12.0;
        });
        assert!(
            dirty
                .iter()
                .zip(plain.iter())
                .any(|(a, b)| (a - b).abs() > 0.02)
        );
        assert!(dirty.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3));
        // The filter closes the top: under the tone, the file's own
        // motion is mostly gone.
        let motion = |xs: &[f32]| xs.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>();
        let dark = render(&|p| p.cutoff = 100.0);
        assert!(motion(&dark) < motion(&plain) * 0.5);
        // Velocity: full sensitivity halves a half-velocity hit's level.
        let mut soft = voices(|p| p.velocity = 1.0);
        let mut hard = voices(|p| p.velocity = 1.0);
        soft.note_on(36, 40, 1);
        hard.note_on(36, 127, 1);
        assert!(rms(&run(&mut hard, 2_400)) > rms(&run(&mut soft, 2_400)) * 2.0);
    }

    #[test]
    fn cut_chokes_the_last_hit_and_ring_lets_it_ring() {
        let mut cut = voices(|_| {});
        cut.note_on(36, 100, 1);
        let _ = run(&mut cut, 480);
        cut.note_on(36, 100, 2);
        let _ = run(&mut cut, 480);
        assert_eq!(cut.readout().bands[0], 1.0, "cut left two hits sounding");
        let mut ring = voices(|p| p.choke = 1.0);
        ring.note_on(36, 100, 1);
        let _ = run(&mut ring, 480);
        ring.note_on(36, 100, 2);
        let _ = run(&mut ring, 480);
        assert_eq!(ring.readout().bands[0], 2.0, "ring choked the first hit");
        ring.all_sound_off();
        assert!(run(&mut ring, 256).iter().all(|x| *x == 0.0));
    }
}
