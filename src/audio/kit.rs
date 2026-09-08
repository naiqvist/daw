//! KIT: sixteen bricks split across the keyboard.
//!
//! One device, sixteen one-shots, each pad on its own key from BASE up.
//! Every pad is a whole `BrickVoices` with the brick's own knobs, plus
//! a pan, a choke group and an on switch; the kit adds a transposition,
//! a tightness that scales every decay, a level, and a solo. Choke
//! groups are the hats' rule made general: a hit in group A fades every
//! other pad sounding in group A.
//!
//! The kit-wide TUNE and TIGHT are folded into each pad's brick when
//! anything settles, so the bricks never know they are in a kit.

use crate::audio::brick::{BrickParams, BrickVoices};
use crate::audio::material::Material;
use crate::params::{self, brick as bp, kit as p};

/// One pad's knobs: the brick's, then the kit's three.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PadParams {
    pub brick: BrickParams,
    pub pan: f32,
    pub group: f32,
    pub on: f32,
}

impl Default for PadParams {
    fn default() -> Self {
        Self {
            brick: BrickParams::default(),
            pan: 0.0,
            group: 0.0,
            on: 1.0,
        }
    }
}

impl PadParams {
    fn set(&mut self, sub: u32, value: f32) {
        match sub {
            p::PAN => self.pan = value,
            p::GROUP => self.group = value,
            p::ON => self.on = value,
            other => self.brick.set(other, value),
        }
    }

    fn get(&self, sub: u32) -> Option<f32> {
        match sub {
            p::PAN => Some(self.pan),
            p::GROUP => Some(self.group),
            p::ON => Some(self.on),
            other => self.brick.get(other),
        }
    }

    pub fn is_on(&self) -> bool {
        self.on >= 0.5
    }

    /// The choke group, 1..=4; zero is none.
    pub fn choke_group(&self) -> u32 {
        self.group.round().clamp(0.0, p::GROUPS as f32) as u32
    }
}

/// The kit's knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KitParams {
    pub base: f32,
    pub pad: f32,
    pub tune: f32,
    pub tight: f32,
    pub level: f32,
    pub solo: f32,
    pub pads: [PadParams; p::PADS],
}

impl Default for KitParams {
    fn default() -> Self {
        let d = |id: u32| params::def(p::TABLE, id).default;
        Self {
            base: d(p::BASE),
            pad: d(p::PAD),
            tune: d(p::TUNE),
            tight: d(p::TIGHT),
            level: d(p::LEVEL),
            solo: d(p::SOLO),
            pads: [PadParams::default(); p::PADS],
        }
    }
}

impl KitParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.get(param as usize) else {
            return;
        };
        let value = def.clamp(value);
        match p::pad_of(param) {
            Some((pad, sub)) => {
                if let Some(slot) = self.pads.get_mut(pad) {
                    slot.set(sub, value);
                }
            }
            None => match param {
                p::BASE => self.base = value,
                p::PAD => self.pad = value,
                p::TUNE => self.tune = value,
                p::TIGHT => self.tight = value,
                p::LEVEL => self.level = value,
                p::SOLO => self.solo = value,
                _ => {}
            },
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match p::pad_of(param) {
            Some((pad, sub)) => self.pads.get(pad)?.get(sub),
            None => match param {
                p::BASE => Some(self.base),
                p::PAD => Some(self.pad),
                p::TUNE => Some(self.tune),
                p::TIGHT => Some(self.tight),
                p::LEVEL => Some(self.level),
                p::SOLO => Some(self.solo),
                _ => None,
            },
        }
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE {
            if let Some(value) = self.get(def.id) {
                let clean = if value.is_finite() {
                    def.clamp(value)
                } else {
                    def.default
                };
                self.set(def.id, clean);
            }
        }
    }

    /// The key the kit starts on.
    pub fn base_note(&self) -> u8 {
        self.base.round().clamp(0.0, 127.0) as u8
    }

    /// The pad in hand — the one a browsed file lands on.
    pub fn pad_in_hand(&self) -> usize {
        (self.pad.round().max(0.0) as usize).min(p::PADS - 1)
    }

    /// The soloed pad, if one is.
    pub fn soloed(&self) -> Option<usize> {
        let solo = self.solo.round() as i32;
        (solo >= 1 && solo <= p::PADS as i32).then(|| (solo - 1) as usize)
    }

    /// Which pad `pitch` plays, if any.
    pub fn pad_for(&self, pitch: u8) -> Option<usize> {
        let offset = i32::from(pitch) - i32::from(self.base_note());
        (0..p::PADS as i32)
            .contains(&offset)
            .then_some(offset as usize)
    }

    /// The key pad `pad` sits on.
    pub fn key_of(&self, pad: usize) -> u8 {
        (u32::from(self.base_note()) + pad as u32).min(127) as u8
    }

    /// Pad `pad`'s brick as it should sound: its own knobs with the
    /// kit's tune and tightness folded in.
    pub fn effective(&self, pad: usize) -> BrickParams {
        let mut brick = self.pads.get(pad).map(|p| p.brick).unwrap_or_default();
        brick.set(bp::TUNE, brick.tune + self.tune);
        brick.set(bp::DECAY, brick.decay * self.tight);
        brick
    }
}

/// The kit as it plays.
pub struct KitVoices {
    params: KitParams,
    base: KitParams,
    pads: Vec<BrickVoices>,
    /// Scratch for one pad's left channel.
    scratch: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    stash: Vec<f32>,
    said_voices: f32,
    said_mask: u32,
    last_pad: Option<usize>,
}

impl KitVoices {
    /// `materials` are the pads' files in pad order; a short list leaves
    /// the rest empty.
    pub fn new(
        sample_rate: f32,
        block: usize,
        params: KitParams,
        mut materials: Vec<Material>,
    ) -> Self {
        let n = block.max(1);
        materials.resize_with(p::PADS, Material::empty);
        let mut params = params;
        params.sanitize();
        let pads = materials
            .into_iter()
            .enumerate()
            .map(|(i, material)| BrickVoices::new(sample_rate, n, params.effective(i), material))
            .collect();
        Self {
            params,
            base: params,
            pads,
            scratch: vec![0.0; n],
            left: vec![0.0; n],
            right: vec![0.0; n],
            stash: vec![0.0; n],
            said_voices: 0.0,
            said_mask: 0,
            last_pad: None,
        }
    }

    pub fn params(&self) -> &KitParams {
        &self.params
    }

    /// Push the live knobs down into the bricks: one pad, or all of
    /// them when a kit-wide knob moved.
    fn settle(&mut self, only: Option<usize>) {
        let pads = match only {
            Some(pad) => pad..pad + 1,
            None => 0..p::PADS,
        };
        for pad in pads {
            let wanted = self.params.effective(pad);
            let Some(voices) = self.pads.get_mut(pad) else {
                continue;
            };
            let have = *voices.params();
            for def in bp::TABLE {
                let (a, b) = (wanted.get(def.id), have.get(def.id));
                if let (true, Some(value)) = (a != b, a) {
                    voices.set_param(def.id, value);
                }
            }
        }
    }

    fn touched(&mut self, param: u32) {
        let only = match p::pad_of(param) {
            Some((pad, _)) => Some(pad),
            None if matches!(param, p::TUNE | p::TIGHT) => None,
            // The other kit-wide rows are the kit's own business.
            None => return,
        };
        self.settle(only);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.base.set(param, value);
        self.touched(param);
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
        self.touched(param);
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        if let (Some(live), Some(base)) = (self.params.get(param), self.base.get(param)) {
            self.params
                .set(param, live + (base - live) * alpha.clamp(0.0, 1.0));
            self.touched(param);
        }
    }

    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [
                self.said_voices,
                self.said_mask as f32,
                self.last_pad.map_or(0.0, |pad| pad as f32 + 1.0),
            ],
        }
    }

    pub fn right(&self, len: usize) -> &[f32] {
        self.stash.get(..len.min(self.stash.len())).unwrap_or(&[])
    }

    pub fn all_sound_off(&mut self) {
        for pad in self.pads.iter_mut() {
            pad.all_sound_off();
        }
        self.said_voices = 0.0;
        self.said_mask = 0;
    }

    pub fn release_all(&mut self) {}

    pub fn note_off(&mut self, _pitch: u8) {}

    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        let Some(pad) = self.params.pad_for(pitch) else {
            return;
        };
        let knobs = self.params.pads[pad];
        if !knobs.is_on() {
            return;
        }
        if self.params.soloed().is_some_and(|solo| solo != pad) {
            return;
        }
        let group = knobs.choke_group();
        if group > 0 {
            for (other, voices) in self.pads.iter_mut().enumerate() {
                if other != pad && self.params.pads[other].choke_group() == group {
                    voices.choke();
                }
            }
        }
        if let Some(voices) = self.pads.get_mut(pad) {
            // The brick hears middle C: a pad has one key, and the pad's
            // own TUNE says where it sits.
            voices.note_on(60, vel, age);
            self.last_pad = Some(pad);
        }
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
        let mut sounding = 0usize;
        let mut mask = 0u32;
        for (i, pad) in self.pads.iter_mut().enumerate() {
            if pad.sounding() == 0 {
                continue;
            }
            let mut flat = crate::audio::graph::Ramp::across(1.0, 1.0, n);
            let Some(scratch) = self.scratch.get_mut(..n) else {
                break;
            };
            pad.render(scratch, 0, &mut flat);
            let right = pad.right(n);
            // Constant-power pan on the pad's own stereo pair.
            let pan = self.params.pads[i].pan.clamp(-1.0, 1.0);
            let angle = (pan + 1.0) * core::f32::consts::FRAC_PI_4;
            let (gl, gr) = (angle.cos(), angle.sin());
            for s in 0..n {
                if let Some(slot) = self.left.get_mut(s) {
                    *slot += scratch.get(s).copied().unwrap_or(0.0) * gl;
                }
                if let Some(slot) = self.right.get_mut(s) {
                    *slot += right.get(s).copied().unwrap_or(0.0) * gr;
                }
            }
            let still = pad.sounding();
            if still > 0 {
                sounding += still;
                mask |= 1 << i;
            }
        }
        self.said_voices = sounding as f32;
        self.said_mask = mask;
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
    use std::sync::Arc;

    const FS: f32 = 48_000.0;

    /// A tenth of a second of a tone at `hz`, decaying.
    fn tone(hz: f32) -> Material {
        let frames = 4_800usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / FS;
                0.8 * (-t * 20.0).exp() * (t * hz * core::f32::consts::TAU).sin()
            })
            .collect();
        Material {
            samples: Arc::new(samples),
            channels: 1,
            frames: frames as u64,
            source: format!("/kits/{hz}.wav").into(),
            sample_rate: 48_000,
            original_rate: 48_000,
            truncated: false,
        }
    }

    fn kit(edit: impl Fn(&mut KitParams)) -> KitVoices {
        let mut params = KitParams::default();
        for pad in params.pads.iter_mut() {
            pad.brick.bits = 16.0;
            pad.brick.rate = 48_000.0;
            pad.brick.punch = 0.0;
            pad.brick.body = 0.0;
            pad.brick.snap = 0.0;
            pad.brick.grit = 1.0;
            pad.brick.decay = 4_000.0;
            pad.brick.velocity = 0.0;
        }
        edit(&mut params);
        let materials = (0..4).map(|i| tone(200.0 * (i + 1) as f32)).collect();
        KitVoices::new(FS, 256, params, materials)
    }

    fn run(voices: &mut KitVoices, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut out = vec![0.0; n];
        let mut ramp = Ramp::across(1.0, 1.0, n);
        voices.render(&mut out, 0, &mut ramp);
        let right = voices.right(n).to_vec();
        (out, right)
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
    fn every_row_reaches_a_field_and_the_table_is_shaped_as_promised() {
        assert_eq!(
            p::TABLE.len(),
            p::GLOBALS as usize + p::PADS * p::PER_PAD as usize
        );
        let mut k = kit(|_| {});
        for def in p::TABLE {
            k.set_param(def.id, def.max);
            assert_eq!(k.params().get(def.id), Some(def.max), "{}", def.name);
        }
        assert_eq!(p::pad_of(p::pad_param(3, p::GROUP)), Some((3, p::GROUP)));
        assert_eq!(p::pad_of(p::SOLO), None);
        // The pad rows mirror the brick's own order.
        for def in bp::TABLE {
            let row = p::TABLE[p::pad_param(15, def.id) as usize];
            assert_eq!(row.name, format!("p16 {}", def.name));
            assert_eq!(
                (row.min, row.max, row.default),
                (def.min, def.max, def.default)
            );
        }
    }

    #[test]
    fn each_key_from_base_plays_its_own_pad_and_nothing_else_answers() {
        let mut k = kit(|_| {});
        let base = k.params().base_note();
        k.note_on(base, 100, 1);
        let (a, _) = run(&mut k, 2_400);
        assert!(rms(&a) > 0.05);
        let mut k2 = kit(|_| {});
        k2.note_on(base + 1, 100, 1);
        let (b, _) = run(&mut k2, 2_400);
        assert!(
            crossings(&b) > crossings(&a) * 3 / 2,
            "pad 2 should be the 400 Hz file: {} vs {}",
            crossings(&b),
            crossings(&a)
        );
        // Off the kit, or on an empty pad: silence, and a clean readout.
        let mut k3 = kit(|_| {});
        k3.note_on(base - 1, 100, 1);
        k3.note_on(base + 15, 100, 2);
        k3.note_on(base + 16, 100, 3);
        let (c, _) = run(&mut k3, 512);
        assert!(c.iter().all(|x| *x == 0.0));
        assert_eq!(k3.readout().bands[0], 0.0);
        // BASE moves the whole kit.
        let mut k4 = kit(|p| p.base = 48.0);
        k4.note_on(36, 100, 1);
        assert!(run(&mut k4, 512).0.iter().all(|x| *x == 0.0));
        k4.note_on(48, 100, 2);
        assert!(rms(&run(&mut k4, 2_400).0) > 0.05);
    }

    #[test]
    fn mute_solo_and_choke_groups_do_what_they_say() {
        let base = KitParams::default().base_note();
        // Mute: pad 1 off says nothing.
        let mut k = kit(|p| p.pads[0].on = 0.0);
        k.note_on(base, 100, 1);
        assert!(run(&mut k, 512).0.iter().all(|x| *x == 0.0));
        // Solo pad 2: pad 1 says nothing, pad 2 plays.
        let mut k = kit(|p| p.solo = 2.0);
        k.note_on(base, 100, 1);
        assert!(run(&mut k, 512).0.iter().all(|x| *x == 0.0));
        k.note_on(base + 1, 100, 2);
        assert!(rms(&run(&mut k, 512).0) > 0.05);
        // Choke: pads 1 and 2 in group A — the second hit fades the first.
        let mut k = kit(|p| {
            p.pads[0].group = 1.0;
            p.pads[1].group = 1.0;
        });
        k.note_on(base, 100, 1);
        let _ = run(&mut k, 480);
        k.note_on(base + 1, 100, 2);
        let _ = run(&mut k, 480);
        assert_eq!(k.readout().bands[0], 1.0, "group A left both sounding");
        assert_eq!(k.readout().bands[1], 2.0, "only pad 2 should be lit");
        assert_eq!(k.readout().bands[2], 2.0);
        // No group: both ring.
        let mut k = kit(|_| {});
        k.note_on(base, 100, 1);
        let _ = run(&mut k, 480);
        k.note_on(base + 1, 100, 2);
        let _ = run(&mut k, 480);
        assert_eq!(k.readout().bands[0], 2.0);
        assert_eq!(k.readout().bands[1], 3.0);
    }

    #[test]
    fn kit_tune_tight_pan_and_level_fold_into_the_pads() {
        let base = KitParams::default().base_note();
        let mut plain = kit(|_| {});
        plain.note_on(base, 100, 1);
        let (a, ar) = run(&mut plain, 2_400);
        // TUNE up an octave: twice the crossings.
        let mut up = kit(|p| p.tune = 12.0);
        up.note_on(base, 100, 1);
        let (b, _) = run(&mut up, 2_400);
        assert!(crossings(&b) > crossings(&a) * 3 / 2);
        // TIGHT short: gone by the end of the file.
        let mut tight = kit(|p| {
            p.tight = 0.25;
            for pad in p.pads.iter_mut() {
                pad.brick.decay = 100.0;
                pad.brick.curve = 1.0;
            }
        });
        tight.note_on(base, 100, 1);
        let (t, _) = run(&mut tight, 4_800);
        assert!(rms(&t[2_400..]) < 1e-3 && rms(&t[..480]) > 0.05);
        // PAN hard left: the right side is silent.
        let mut left = kit(|p| p.pads[0].pan = -1.0);
        left.note_on(base, 100, 1);
        let (l, lr) = run(&mut left, 2_400);
        // The right stash is one block deep: compare inside it.
        assert!(rms(&l) > rms(&a) * 1.3 && rms(&lr[..256]) < 1e-4);
        assert!(
            (rms(&a[..256]) - rms(&ar[..256])).abs() < 1e-3,
            "centre is equal both sides"
        );
        // LEVEL: half the level, half the sound; and a live change lands.
        let mut quiet = kit(|p| p.level = 0.45);
        quiet.note_on(base, 100, 1);
        let (q, _) = run(&mut quiet, 2_400);
        assert!((rms(&q) - rms(&a) * 0.5).abs() < rms(&a) * 0.05);
        plain.set_param(p::pad_param(0, bp::LEVEL), 0.0);
        plain.note_on(base, 100, 2);
        let (_, _) = run(&mut plain, 4_800);
        assert!(run(&mut plain, 2_400).0.iter().all(|x| x.abs() < 1e-6));
    }
}
