//! sCOMP's voices: the take's last pass, played like a sample.
//!
//! The instrument's whole character is rendered in the green zone by
//! [`crate::scomp::render`] — the sine, the passes, the bounce. What
//! is left for the audio thread is a sampler with one file and no
//! knobs on the file: read the take at the note's speed, shape it with
//! an amp envelope, sum the voices. Nothing here allocates; the take is
//! an `Arc` the node was built with and is never replaced in place — a
//! new take is a new node, exactly as a new slice table is.

use std::sync::Arc;

use crate::dsp::adsr::Adsr;
use crate::dsp::interp::hermite_at;
use crate::params::scomp as p;
pub use crate::scomp::{ScompParams, Take};

pub const VOICES: usize = 8;

struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    age: u64,
    pos: f64,
    inc: f64,
    amp: Adsr,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            held: false,
            pitch: 0,
            age: 0,
            pos: 0.0,
            inc: 1.0,
            amp: Adsr::new(),
        }
    }
}

/// The instrument.
pub struct ScompVoices {
    params: ScompParams,
    /// The base a p-lock is released back to — the LIVE knob.
    base: ScompParams,
    sample_rate: f32,
    take: Arc<Vec<f32>>,
    root_hz: f32,
    /// The take's own rate: it may have been rendered at another.
    take_rate: f32,
    voices: Vec<Voice>,
    env: Vec<f32>,
    stash: Vec<f32>,
    said_voices: f32,
}

impl ScompVoices {
    /// Green zone: the take is rendered here, from the knobs.
    pub fn new(sample_rate: f32, block: usize, params: ScompParams) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let take = crate::scomp::render(&params, sr as u32);
        Self::with_take(sr, block, params, &take)
    }

    /// Green zone: the voices over a take already rendered.
    pub fn with_take(sample_rate: f32, block: usize, params: ScompParams, take: &Take) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut me = Self {
            params,
            base: params,
            sample_rate: sr,
            take: take.last(),
            root_hz: take.root_hz,
            take_rate: take.sample_rate.max(1) as f32,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            env: vec![0.0; block.max(64)],
            stash: vec![0.0; block.max(64)],
            said_voices: 0.0,
        };
        me.params.sanitize();
        me.base = me.params;
        me.prepare_envelopes();
        me
    }

    fn prepare_envelopes(&mut self) {
        let (a, r) = (self.params.amp_a_ms, self.params.amp_r_ms);
        for v in self.voices.iter_mut() {
            v.amp.prepare(self.sample_rate, a, 0.0, 1.0, r);
        }
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.base.set(param, value);
        if matches!(param, p::AMP_A | p::AMP_R) {
            self.prepare_envelopes();
        }
    }

    /// A note's own override, or `None` to fall back to the live knob.
    /// A baked row cannot be locked: the take is what it is.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        if p::baked(param) {
            return;
        }
        match value {
            Some(v) => self.params.set(param, v),
            None => {
                if let Some(v) = self.base.get(param) {
                    self.params.set(param, v);
                }
            }
        }
        if matches!(param, p::AMP_A | p::AMP_R) {
            self.prepare_envelopes();
        }
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        if p::baked(param) {
            return;
        }
        if let (Some(live), Some(base)) = (self.params.get(param), self.base.get(param)) {
            self.params
                .set(param, live + (base - live) * alpha.clamp(0.0, 1.0));
        }
    }

    pub fn params(&self) -> &ScompParams {
        &self.params
    }

    /// What the card draws: how many voices are sounding.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [self.said_voices, 0.0, 0.0],
        }
    }

    /// The right channel of the last render: the same as the left.
    pub fn right(&self, len: usize) -> &[f32] {
        self.stash.get(..len.min(self.stash.len())).unwrap_or(&[])
    }

    pub fn all_sound_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
            v.held = false;
            v.amp.reset();
        }
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
        // The take was rendered at the root; a note is the ratio to it,
        // and a take rendered at another rate is corrected as it plays.
        let semis = f32::from(pitch) - self.params.root.clamp(0.0, 127.0) + self.params.tune_st;
        let ratio = (semis / 12.0).exp2() * (self.take_rate / self.sample_rate);
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        v.active = true;
        v.held = true;
        v.pitch = pitch;
        v.age = age;
        v.pos = 0.0;
        v.inc = if ratio.is_finite() {
            f64::from(ratio.clamp(1.0e-3, 64.0))
        } else {
            1.0
        };
        let _ = vel;
        v.amp.gate_on();
    }

    /// Red zone: render the LEFT channel, stash the right. Any length:
    /// the envelope scratch is walked in chunks.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut crate::audio::graph::Ramp) {
        for slot in out.iter_mut() {
            *slot = 0.0;
        }
        let n = out.len();
        let chunk = self.env.len().max(1);
        let end = self.take.len().saturating_sub(1) as f64;
        let mut sounding = 0usize;
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let mut done = false;
            let mut from = 0usize;
            while from < n && !done {
                let take = (n - from).min(chunk);
                let (Some(env), Some(block)) =
                    (self.env.get_mut(..take), out.get_mut(from..from + take))
                else {
                    break;
                };
                v.amp.process(env);
                for (slot, e) in block.iter_mut().zip(env.iter()) {
                    if done {
                        break;
                    }
                    let s = hermite_at(&self.take, v.pos) * *e;
                    *slot += if s.is_finite() { s } else { 0.0 };
                    v.pos += v.inc;
                    if v.pos >= end {
                        done = true;
                    }
                }
                from += take;
            }
            if done || !v.amp.active() {
                v.active = false;
                v.held = false;
            } else {
                sounding += 1;
            }
        }
        self.said_voices = sounding as f32;
        let level = self.params.level;
        for (i, slot) in out.iter_mut().enumerate() {
            let g = gain.next() * level;
            let s = *slot * g;
            *slot = if s.is_finite() { s } else { 0.0 };
            if let Some(r) = self.stash.get_mut(at + i) {
                *r = *slot;
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

    fn run(voices: &mut ScompVoices, n: usize) -> Vec<f32> {
        let mut out = vec![0.0; n];
        let mut ramp = Ramp::across(1.0, 1.0, n);
        voices.render(&mut out, 0, &mut ramp);
        out
    }

    #[test]
    fn every_parameter_reaches_its_field() {
        let mut voices = ScompVoices::new(FS, 256, ScompParams::default());
        for def in p::TABLE {
            voices.set_param(def.id, def.max);
            assert_eq!(voices.params().get(def.id), Some(def.max), "{}", def.name);
        }
    }

    #[test]
    fn a_note_plays_the_take_at_its_pitch_and_a_lock_on_a_baked_row_is_ignored() {
        let mut params = ScompParams::default();
        params.take_s = 0.25;
        params.shift_st = 0.0;
        let mut voices = ScompVoices::new(FS, 256, params);
        assert!(
            run(&mut voices, 256).iter().all(|x| *x == 0.0),
            "silent before a note"
        );
        voices.note_on(36, 100, 1);
        let block = run(&mut voices, 4_800);
        assert!(
            block.iter().any(|x| x.abs() > 0.05),
            "the note made no sound"
        );
        assert!(block.iter().all(|x| x.is_finite() && x.abs() <= 2.0));
        assert_eq!(voices.readout().bands[0], 1.0);
        // An octave up reads the take twice as fast: it is over sooner.
        let mut fast = ScompVoices::new(FS, 256, params);
        fast.note_on(48, 100, 1);
        let _ = run(&mut fast, 6_000);
        let _ = run(&mut fast, 6_000);
        assert_eq!(
            fast.readout().bands[0],
            0.0,
            "an octave up, a quarter-second take outlasted half a second"
        );
        // A lock on DRIVE cannot re-render; a lock on LEVEL can.
        let drive = voices.params().drive;
        voices.plock(p::DRIVE, Some(30.0));
        assert_eq!(voices.params().drive, drive);
        voices.plock(p::LEVEL, Some(0.1));
        assert_eq!(voices.params().level, 0.1);
        voices.plock(p::LEVEL, None);
        assert_eq!(voices.params().level, ScompParams::default().level);
    }

    #[test]
    fn note_off_releases_and_all_sound_off_silences() {
        let mut params = ScompParams::default();
        params.take_s = 4.0;
        params.amp_r_ms = 10.0;
        let mut voices = ScompVoices::new(FS, 256, params);
        voices.note_on(36, 100, 1);
        let _ = run(&mut voices, 512);
        voices.note_off(36);
        let _ = run(&mut voices, 4_800);
        assert_eq!(
            voices.readout().bands[0],
            0.0,
            "released, yet still sounding after 100 ms"
        );
        voices.note_on(36, 100, 2);
        voices.note_on(40, 100, 3);
        let _ = run(&mut voices, 256);
        assert_eq!(voices.readout().bands[0], 2.0);
        voices.all_sound_off();
        assert!(run(&mut voices, 256).iter().all(|x| *x == 0.0));
    }
}
