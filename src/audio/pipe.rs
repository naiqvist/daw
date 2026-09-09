//! PIPE: see notes/20260909-pipe-brief.md.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::audio::mass::common::Instrument;
use crate::dsp::coverage_physical::Kind;
use crate::params::pipe as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PipeParams {
    pub bar_16: f32,
    pub bar_5_3: f32,
    pub bar_8: f32,
    pub bar_4: f32,
    pub register: f32,
    pub leak: f32,
    pub tune: f32,
    pub click: f32,
    pub bar_2_3: f32,
    pub bar_2: f32,
    pub bar_1_3: f32,
    pub bar_1_1: f32,
    pub bar_1: f32,
    pub perc: f32,
    pub p_decay: f32,
    pub scanner: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
    pub level: f32,
    pub spread: f32,
    pub voices: f32,
    pub drive: f32,
    pub tube_bias: f32,
    pub tube_tone: f32,
    pub tube_mix: f32,
    pub tube_corner: f32,
    pub sag: f32,
    pub speed: f32,
    pub accel: f32,
    pub horn: f32,
    pub drum: f32,
    pub rotary_mix: f32,
    pub distance: f32,
}
impl Default for PipeParams {
    fn default() -> Self {
        Self {
            bar_16: p::TABLE[0].default,
            bar_5_3: p::TABLE[1].default,
            bar_8: p::TABLE[2].default,
            bar_4: p::TABLE[3].default,
            register: p::TABLE[4].default,
            leak: p::TABLE[5].default,
            tune: p::TABLE[6].default,
            click: p::TABLE[7].default,
            bar_2_3: p::TABLE[8].default,
            bar_2: p::TABLE[9].default,
            bar_1_3: p::TABLE[10].default,
            bar_1_1: p::TABLE[11].default,
            bar_1: p::TABLE[12].default,
            perc: p::TABLE[13].default,
            p_decay: p::TABLE[14].default,
            scanner: p::TABLE[15].default,
            attack: p::TABLE[16].default,
            decay: p::TABLE[17].default,
            sustain: p::TABLE[18].default,
            release: p::TABLE[19].default,
            velocity: p::TABLE[20].default,
            level: p::TABLE[21].default,
            spread: p::TABLE[22].default,
            voices: p::TABLE[23].default,
            drive: p::TABLE[24].default,
            tube_bias: p::TABLE[25].default,
            tube_tone: p::TABLE[26].default,
            tube_mix: p::TABLE[27].default,
            tube_corner: p::TABLE[28].default,
            sag: p::TABLE[29].default,
            speed: p::TABLE[30].default,
            accel: p::TABLE[31].default,
            horn: p::TABLE[32].default,
            drum: p::TABLE[33].default,
            rotary_mix: p::TABLE[34].default,
            distance: p::TABLE[35].default,
        }
    }
}
impl PipeParams {
    pub fn get(&self, id: u32) -> f32 {
        match id {
            0 => self.bar_16,
            1 => self.bar_5_3,
            2 => self.bar_8,
            3 => self.bar_4,
            4 => self.register,
            5 => self.leak,
            6 => self.tune,
            7 => self.click,
            8 => self.bar_2_3,
            9 => self.bar_2,
            10 => self.bar_1_3,
            11 => self.bar_1_1,
            12 => self.bar_1,
            13 => self.perc,
            14 => self.p_decay,
            15 => self.scanner,
            16 => self.attack,
            17 => self.decay,
            18 => self.sustain,
            19 => self.release,
            20 => self.velocity,
            21 => self.level,
            22 => self.spread,
            23 => self.voices,
            24 => self.drive,
            25 => self.tube_bias,
            26 => self.tube_tone,
            27 => self.tube_mix,
            28 => self.tube_corner,
            29 => self.sag,
            30 => self.speed,
            31 => self.accel,
            32 => self.horn,
            33 => self.drum,
            34 => self.rotary_mix,
            35 => self.distance,
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
            0 => self.bar_16 = v,
            1 => self.bar_5_3 = v,
            2 => self.bar_8 = v,
            3 => self.bar_4 = v,
            4 => self.register = v,
            5 => self.leak = v,
            6 => self.tune = v,
            7 => self.click = v,
            8 => self.bar_2_3 = v,
            9 => self.bar_2 = v,
            10 => self.bar_1_3 = v,
            11 => self.bar_1_1 = v,
            12 => self.bar_1 = v,
            13 => self.perc = v,
            14 => self.p_decay = v,
            15 => self.scanner = v,
            16 => self.attack = v,
            17 => self.decay = v,
            18 => self.sustain = v,
            19 => self.release = v,
            20 => self.velocity = v,
            21 => self.level = v,
            22 => self.spread = v,
            23 => self.voices = v,
            24 => self.drive = v,
            25 => self.tube_bias = v,
            26 => self.tube_tone = v,
            27 => self.tube_mix = v,
            28 => self.tube_corner = v,
            29 => self.sag = v,
            30 => self.speed = v,
            31 => self.accel = v,
            32 => self.horn = v,
            33 => self.drum = v,
            34 => self.rotary_mix = v,
            35 => self.distance = v,
            _ => {}
        }
    }
    pub fn values(&self) -> [f32; 36] {
        std::array::from_fn(|i| self.get(i as u32))
    }
}

pub struct PipeVoices {
    inner: Instrument,
}
impl Default for PipeVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl PipeVoices {
    pub fn new() -> Self {
        Self {
            inner: Instrument::new(Kind::Pipe, p::TABLE),
        }
    }
    pub fn prepare(&mut self, sr: f32, max_block: usize, p: PipeParams) {
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
    Instrument::latency_for(Kind::Pipe, sr)
}
pub fn hero(params: &PipeParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::mass::common::hero(Kind::Pipe, params.values(), p::KEYS, page, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract() {
        crate::audio::mass::common::test_contract(Kind::Pipe, p::TABLE, p::KEYS);
    }
    #[test]
    fn params_roundtrip() {
        let mut a = PipeParams::default();
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
