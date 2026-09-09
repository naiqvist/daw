//! MASS: see notes/20260909-mass-brief.md.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::audio::mass::common::Instrument;
use crate::dsp::coverage_physical::Kind;
use crate::params::mass as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MassParams {
    pub tune: f32,
    pub layer: f32,
    pub octave: f32,
    pub blend: f32,
    pub phase: f32,
    pub drop: f32,
    pub drop_time: f32,
    pub glide: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub env: f32,
    pub f_decay: f32,
    pub track: f32,
    pub key_low: f32,
    pub sub_floor: f32,
    pub legato: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
    pub level: f32,
    pub spread: f32,
    pub voices: f32,
    pub harmonic: f32,
    pub tone: f32,
    pub corner: f32,
    pub bias: f32,
    pub weight_mix: f32,
    pub skew: f32,
    pub ceiling: f32,
    pub clamp_release: f32,
    pub glue: f32,
    pub threshold: f32,
    pub clamp_attack: f32,
    pub clamp_mix: f32,
}
impl Default for MassParams {
    fn default() -> Self {
        Self {
            tune: p::TABLE[0].default,
            layer: p::TABLE[1].default,
            octave: p::TABLE[2].default,
            blend: p::TABLE[3].default,
            phase: p::TABLE[4].default,
            drop: p::TABLE[5].default,
            drop_time: p::TABLE[6].default,
            glide: p::TABLE[7].default,
            cutoff: p::TABLE[8].default,
            reso: p::TABLE[9].default,
            env: p::TABLE[10].default,
            f_decay: p::TABLE[11].default,
            track: p::TABLE[12].default,
            key_low: p::TABLE[13].default,
            sub_floor: p::TABLE[14].default,
            legato: p::TABLE[15].default,
            attack: p::TABLE[16].default,
            decay: p::TABLE[17].default,
            sustain: p::TABLE[18].default,
            release: p::TABLE[19].default,
            velocity: p::TABLE[20].default,
            level: p::TABLE[21].default,
            spread: p::TABLE[22].default,
            voices: p::TABLE[23].default,
            harmonic: p::TABLE[24].default,
            tone: p::TABLE[25].default,
            corner: p::TABLE[26].default,
            bias: p::TABLE[27].default,
            weight_mix: p::TABLE[28].default,
            skew: p::TABLE[29].default,
            ceiling: p::TABLE[30].default,
            clamp_release: p::TABLE[31].default,
            glue: p::TABLE[32].default,
            threshold: p::TABLE[33].default,
            clamp_attack: p::TABLE[34].default,
            clamp_mix: p::TABLE[35].default,
        }
    }
}
impl MassParams {
    pub fn get(&self, id: u32) -> f32 {
        match id {
            0 => self.tune,
            1 => self.layer,
            2 => self.octave,
            3 => self.blend,
            4 => self.phase,
            5 => self.drop,
            6 => self.drop_time,
            7 => self.glide,
            8 => self.cutoff,
            9 => self.reso,
            10 => self.env,
            11 => self.f_decay,
            12 => self.track,
            13 => self.key_low,
            14 => self.sub_floor,
            15 => self.legato,
            16 => self.attack,
            17 => self.decay,
            18 => self.sustain,
            19 => self.release,
            20 => self.velocity,
            21 => self.level,
            22 => self.spread,
            23 => self.voices,
            24 => self.harmonic,
            25 => self.tone,
            26 => self.corner,
            27 => self.bias,
            28 => self.weight_mix,
            29 => self.skew,
            30 => self.ceiling,
            31 => self.clamp_release,
            32 => self.glue,
            33 => self.threshold,
            34 => self.clamp_attack,
            35 => self.clamp_mix,
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
            1 => self.layer = v,
            2 => self.octave = v,
            3 => self.blend = v,
            4 => self.phase = v,
            5 => self.drop = v,
            6 => self.drop_time = v,
            7 => self.glide = v,
            8 => self.cutoff = v,
            9 => self.reso = v,
            10 => self.env = v,
            11 => self.f_decay = v,
            12 => self.track = v,
            13 => self.key_low = v,
            14 => self.sub_floor = v,
            15 => self.legato = v,
            16 => self.attack = v,
            17 => self.decay = v,
            18 => self.sustain = v,
            19 => self.release = v,
            20 => self.velocity = v,
            21 => self.level = v,
            22 => self.spread = v,
            23 => self.voices = v,
            24 => self.harmonic = v,
            25 => self.tone = v,
            26 => self.corner = v,
            27 => self.bias = v,
            28 => self.weight_mix = v,
            29 => self.skew = v,
            30 => self.ceiling = v,
            31 => self.clamp_release = v,
            32 => self.glue = v,
            33 => self.threshold = v,
            34 => self.clamp_attack = v,
            35 => self.clamp_mix = v,
            _ => {}
        }
    }
    pub fn values(&self) -> [f32; 36] {
        std::array::from_fn(|i| self.get(i as u32))
    }
}

pub struct MassVoices {
    inner: Instrument,
}
impl Default for MassVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl MassVoices {
    pub fn new() -> Self {
        Self {
            inner: Instrument::new(Kind::Mass, p::TABLE),
        }
    }
    pub fn prepare(&mut self, sr: f32, max_block: usize, p: MassParams) {
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
    Instrument::latency_for(Kind::Mass, sr)
}
pub fn hero(params: &MassParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::mass::common::hero(Kind::Mass, params.values(), p::KEYS, page, selected)
}

pub(crate) mod common;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract() {
        crate::audio::mass::common::test_contract(Kind::Mass, p::TABLE, p::KEYS);
    }
    #[test]
    fn params_roundtrip() {
        let mut a = MassParams::default();
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
