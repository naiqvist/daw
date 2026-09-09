//! VOX: see notes/20260909-vox-brief.md.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::audio::mass::common::Instrument;
use crate::dsp::coverage_physical::Kind;
use crate::params::vox as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct VoxParams {
    pub vowel: f32,
    pub sex: f32,
    pub throat: f32,
    pub breath: f32,
    pub pw: f32,
    pub vibrato: f32,
    pub tune: f32,
    pub bite: f32,
    pub bend: f32,
    pub bend_time: f32,
    pub vib_rate: f32,
    pub vib_delay: f32,
    pub track: f32,
    pub tone: f32,
    pub formant: f32,
    pub air: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
    pub level: f32,
    pub spread: f32,
    pub voices: f32,
    pub sense: f32,
    pub range: f32,
    pub talk_attack: f32,
    pub talk_release: f32,
    pub talk_mix: f32,
    pub talk_bias: f32,
    pub choir_voices: f32,
    pub choir_detune: f32,
    pub choir_rate: f32,
    pub choir_mix: f32,
    pub choir_width: f32,
    pub choir_delay: f32,
}
impl Default for VoxParams {
    fn default() -> Self {
        Self {
            vowel: p::TABLE[0].default,
            sex: p::TABLE[1].default,
            throat: p::TABLE[2].default,
            breath: p::TABLE[3].default,
            pw: p::TABLE[4].default,
            vibrato: p::TABLE[5].default,
            tune: p::TABLE[6].default,
            bite: p::TABLE[7].default,
            bend: p::TABLE[8].default,
            bend_time: p::TABLE[9].default,
            vib_rate: p::TABLE[10].default,
            vib_delay: p::TABLE[11].default,
            track: p::TABLE[12].default,
            tone: p::TABLE[13].default,
            formant: p::TABLE[14].default,
            air: p::TABLE[15].default,
            attack: p::TABLE[16].default,
            decay: p::TABLE[17].default,
            sustain: p::TABLE[18].default,
            release: p::TABLE[19].default,
            velocity: p::TABLE[20].default,
            level: p::TABLE[21].default,
            spread: p::TABLE[22].default,
            voices: p::TABLE[23].default,
            sense: p::TABLE[24].default,
            range: p::TABLE[25].default,
            talk_attack: p::TABLE[26].default,
            talk_release: p::TABLE[27].default,
            talk_mix: p::TABLE[28].default,
            talk_bias: p::TABLE[29].default,
            choir_voices: p::TABLE[30].default,
            choir_detune: p::TABLE[31].default,
            choir_rate: p::TABLE[32].default,
            choir_mix: p::TABLE[33].default,
            choir_width: p::TABLE[34].default,
            choir_delay: p::TABLE[35].default,
        }
    }
}
impl VoxParams {
    pub fn get(&self, id: u32) -> f32 {
        match id {
            0 => self.vowel,
            1 => self.sex,
            2 => self.throat,
            3 => self.breath,
            4 => self.pw,
            5 => self.vibrato,
            6 => self.tune,
            7 => self.bite,
            8 => self.bend,
            9 => self.bend_time,
            10 => self.vib_rate,
            11 => self.vib_delay,
            12 => self.track,
            13 => self.tone,
            14 => self.formant,
            15 => self.air,
            16 => self.attack,
            17 => self.decay,
            18 => self.sustain,
            19 => self.release,
            20 => self.velocity,
            21 => self.level,
            22 => self.spread,
            23 => self.voices,
            24 => self.sense,
            25 => self.range,
            26 => self.talk_attack,
            27 => self.talk_release,
            28 => self.talk_mix,
            29 => self.talk_bias,
            30 => self.choir_voices,
            31 => self.choir_detune,
            32 => self.choir_rate,
            33 => self.choir_mix,
            34 => self.choir_width,
            35 => self.choir_delay,
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
            0 => self.vowel = v,
            1 => self.sex = v,
            2 => self.throat = v,
            3 => self.breath = v,
            4 => self.pw = v,
            5 => self.vibrato = v,
            6 => self.tune = v,
            7 => self.bite = v,
            8 => self.bend = v,
            9 => self.bend_time = v,
            10 => self.vib_rate = v,
            11 => self.vib_delay = v,
            12 => self.track = v,
            13 => self.tone = v,
            14 => self.formant = v,
            15 => self.air = v,
            16 => self.attack = v,
            17 => self.decay = v,
            18 => self.sustain = v,
            19 => self.release = v,
            20 => self.velocity = v,
            21 => self.level = v,
            22 => self.spread = v,
            23 => self.voices = v,
            24 => self.sense = v,
            25 => self.range = v,
            26 => self.talk_attack = v,
            27 => self.talk_release = v,
            28 => self.talk_mix = v,
            29 => self.talk_bias = v,
            30 => self.choir_voices = v,
            31 => self.choir_detune = v,
            32 => self.choir_rate = v,
            33 => self.choir_mix = v,
            34 => self.choir_width = v,
            35 => self.choir_delay = v,
            _ => {}
        }
    }
    pub fn values(&self) -> [f32; 36] {
        std::array::from_fn(|i| self.get(i as u32))
    }
}

pub struct VoxVoices {
    inner: Instrument,
}
impl Default for VoxVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl VoxVoices {
    pub fn new() -> Self {
        Self {
            inner: Instrument::new(Kind::Vox, p::TABLE),
        }
    }
    pub fn prepare(&mut self, sr: f32, max_block: usize, p: VoxParams) {
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
    Instrument::latency_for(Kind::Vox, sr)
}
pub fn hero(params: &VoxParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::mass::common::hero(Kind::Vox, params.values(), p::KEYS, page, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract() {
        crate::audio::mass::common::test_contract(Kind::Vox, p::TABLE, p::KEYS);
    }
    #[test]
    fn params_roundtrip() {
        let mut a = VoxParams::default();
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
