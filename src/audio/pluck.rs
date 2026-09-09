//! PLUCK: see notes/20260909-pluck-brief.md.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::audio::mass::common::Instrument;
use crate::dsp::coverage_physical::Kind;
use crate::params::pluck as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PluckParams {
    pub tune: f32,
    pub detune: f32,
    pub ring: f32,
    pub bright: f32,
    pub pick: f32,
    pub strike: f32,
    pub stiff: f32,
    pub stretch: f32,
    pub hammer: f32,
    pub pickup: f32,
    pub bark: f32,
    pub overtone: f32,
    pub tonebar: f32,
    pub tone_decay: f32,
    pub damp: f32,
    pub choke: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
    pub level: f32,
    pub spread: f32,
    pub voices: f32,
    pub body_size: f32,
    pub body_decay: f32,
    pub body_damp: f32,
    pub body_mix: f32,
    pub diffuse: f32,
    pub motion: f32,
    pub shine: f32,
    pub shine_corner: f32,
    pub shine_mix: f32,
    pub shine_attack: f32,
    pub shine_release: f32,
    pub shine_tilt: f32,
}
impl Default for PluckParams {
    fn default() -> Self {
        Self {
            tune: p::TABLE[0].default,
            detune: p::TABLE[1].default,
            ring: p::TABLE[2].default,
            bright: p::TABLE[3].default,
            pick: p::TABLE[4].default,
            strike: p::TABLE[5].default,
            stiff: p::TABLE[6].default,
            stretch: p::TABLE[7].default,
            hammer: p::TABLE[8].default,
            pickup: p::TABLE[9].default,
            bark: p::TABLE[10].default,
            overtone: p::TABLE[11].default,
            tonebar: p::TABLE[12].default,
            tone_decay: p::TABLE[13].default,
            damp: p::TABLE[14].default,
            choke: p::TABLE[15].default,
            attack: p::TABLE[16].default,
            decay: p::TABLE[17].default,
            sustain: p::TABLE[18].default,
            release: p::TABLE[19].default,
            velocity: p::TABLE[20].default,
            level: p::TABLE[21].default,
            spread: p::TABLE[22].default,
            voices: p::TABLE[23].default,
            body_size: p::TABLE[24].default,
            body_decay: p::TABLE[25].default,
            body_damp: p::TABLE[26].default,
            body_mix: p::TABLE[27].default,
            diffuse: p::TABLE[28].default,
            motion: p::TABLE[29].default,
            shine: p::TABLE[30].default,
            shine_corner: p::TABLE[31].default,
            shine_mix: p::TABLE[32].default,
            shine_attack: p::TABLE[33].default,
            shine_release: p::TABLE[34].default,
            shine_tilt: p::TABLE[35].default,
        }
    }
}
impl PluckParams {
    pub fn get(&self, id: u32) -> f32 {
        match id {
            0 => self.tune,
            1 => self.detune,
            2 => self.ring,
            3 => self.bright,
            4 => self.pick,
            5 => self.strike,
            6 => self.stiff,
            7 => self.stretch,
            8 => self.hammer,
            9 => self.pickup,
            10 => self.bark,
            11 => self.overtone,
            12 => self.tonebar,
            13 => self.tone_decay,
            14 => self.damp,
            15 => self.choke,
            16 => self.attack,
            17 => self.decay,
            18 => self.sustain,
            19 => self.release,
            20 => self.velocity,
            21 => self.level,
            22 => self.spread,
            23 => self.voices,
            24 => self.body_size,
            25 => self.body_decay,
            26 => self.body_damp,
            27 => self.body_mix,
            28 => self.diffuse,
            29 => self.motion,
            30 => self.shine,
            31 => self.shine_corner,
            32 => self.shine_mix,
            33 => self.shine_attack,
            34 => self.shine_release,
            35 => self.shine_tilt,
            _ => 0.0,
        }
    }
    pub fn set(&mut self, id: u32, value: f32) {
        let Some(d) = p::TABLE.get(id as usize) else {
            return;
        };
        let v = if value.is_finite() {
            value.clamp(d.min, d.max)
        } else {
            d.default
        };
        match id {
            0 => self.tune = v,
            1 => self.detune = v,
            2 => self.ring = v,
            3 => self.bright = v,
            4 => self.pick = v,
            5 => self.strike = v,
            6 => self.stiff = v,
            7 => self.stretch = v,
            8 => self.hammer = v,
            9 => self.pickup = v,
            10 => self.bark = v,
            11 => self.overtone = v,
            12 => self.tonebar = v,
            13 => self.tone_decay = v,
            14 => self.damp = v,
            15 => self.choke = v,
            16 => self.attack = v,
            17 => self.decay = v,
            18 => self.sustain = v,
            19 => self.release = v,
            20 => self.velocity = v,
            21 => self.level = v,
            22 => self.spread = v,
            23 => self.voices = v,
            24 => self.body_size = v,
            25 => self.body_decay = v,
            26 => self.body_damp = v,
            27 => self.body_mix = v,
            28 => self.diffuse = v,
            29 => self.motion = v,
            30 => self.shine = v,
            31 => self.shine_corner = v,
            32 => self.shine_mix = v,
            33 => self.shine_attack = v,
            34 => self.shine_release = v,
            35 => self.shine_tilt = v,
            _ => {}
        }
    }
    pub fn values(&self) -> [f32; 36] {
        std::array::from_fn(|i| self.get(i as u32))
    }
}

pub struct PluckVoices {
    inner: Instrument,
}
impl Default for PluckVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl PluckVoices {
    pub fn new() -> Self {
        Self {
            inner: Instrument::new(Kind::Pluck, p::TABLE),
        }
    }
    pub fn prepare(&mut self, sr: f32, max_block: usize, p: PluckParams) {
        self.inner.prepare(sr, max_block, p.values());
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
    pub fn set_param(&mut self, id: u32, v: f32) {
        self.inner.set_param(id, v);
    }
    pub fn plock(&mut self, id: u32, v: Option<f32>) {
        self.inner.plock(id, v);
    }
    pub fn plock_glide(&mut self, id: u32, a: f32) {
        self.inner.plock_glide(id, a);
    }
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        self.inner.note_on(pitch, vel as f32 / 127.0, age);
    }
    pub fn note_off(&mut self, pitch: u8) {
        self.inner.note_off(pitch);
    }
    pub fn release_all(&mut self) {
        self.inner.release_all();
    }
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        self.inner.render(out, at, gain);
    }
    pub fn right(&self, len: usize) -> &[f32] {
        self.inner.right(len)
    }
    pub fn latency(&self) -> usize {
        self.inner.latency()
    }
    pub fn active(&self) -> bool {
        self.inner.active()
    }
}
pub fn latency(sr: f32) -> usize {
    Instrument::latency_for(Kind::Pluck, sr)
}
pub fn hero(params: &PluckParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::mass::common::hero(Kind::Pluck, params.values(), p::KEYS, page, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract() {
        crate::audio::mass::common::test_contract(Kind::Pluck, p::TABLE, p::KEYS);
    }
    #[test]
    fn params_roundtrip() {
        let mut a = PluckParams::default();
        for d in p::TABLE {
            a.set(d.id, d.min);
            assert_eq!(a.get(d.id), d.min);
            a.set(d.id, d.max);
            assert_eq!(a.get(d.id), d.max);
            a.set(d.id, d.max + 100.0);
            assert_eq!(a.get(d.id), d.max);
        }
        let old = a;
        a.set(999, 1.0);
        assert_eq!(a, old);
    }
}
