//! Ring — measured, lockable synthesis for the pages.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::params::ring as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RingParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub velocity: f32,
    pub level: f32,
    pub tune: f32,
    pub width: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub filter_env: f32,
    pub filter_decay: f32,
    pub keytrack: f32,
    pub material: f32,
    pub inharm: f32,
    pub damp: f32,
    pub strike: f32,
    pub hard: f32,
    pub position: f32,
    pub spread: f32,
    pub feed: f32,
    pub partial1: f32,
    pub partial2: f32,
    pub partial3: f32,
    pub partial4: f32,
    pub partial5: f32,
    pub partial6: f32,
    pub choke: f32,
    pub contact: f32,
    pub disperse: f32,
    pub disperse_focus: f32,
    pub disperse_width: f32,
    pub disperse_follow: f32,
    pub bloom_size: f32,
    pub bloom_decay: f32,
    pub bloom_damp: f32,
    pub bloom_mix: f32,
}
impl Default for RingParams {
    fn default() -> Self {
        Self {
            attack: crate::params::def(p::TABLE, p::ATTACK).default,
            decay: crate::params::def(p::TABLE, p::DECAY).default,
            sustain: crate::params::def(p::TABLE, p::SUSTAIN).default,
            release: crate::params::def(p::TABLE, p::RELEASE).default,
            velocity: crate::params::def(p::TABLE, p::VELOCITY).default,
            level: crate::params::def(p::TABLE, p::LEVEL).default,
            tune: crate::params::def(p::TABLE, p::TUNE).default,
            width: crate::params::def(p::TABLE, p::WIDTH).default,
            cutoff: crate::params::def(p::TABLE, p::CUTOFF).default,
            resonance: crate::params::def(p::TABLE, p::RESONANCE).default,
            filter_env: crate::params::def(p::TABLE, p::FILTER_ENV).default,
            filter_decay: crate::params::def(p::TABLE, p::FILTER_DECAY).default,
            keytrack: crate::params::def(p::TABLE, p::KEYTRACK).default,
            material: crate::params::def(p::TABLE, p::MATERIAL).default,
            inharm: crate::params::def(p::TABLE, p::INHARM).default,
            damp: crate::params::def(p::TABLE, p::DAMP).default,
            strike: crate::params::def(p::TABLE, p::STRIKE).default,
            hard: crate::params::def(p::TABLE, p::HARD).default,
            position: crate::params::def(p::TABLE, p::POSITION).default,
            spread: crate::params::def(p::TABLE, p::SPREAD).default,
            feed: crate::params::def(p::TABLE, p::FEED).default,
            partial1: crate::params::def(p::TABLE, p::PARTIAL1).default,
            partial2: crate::params::def(p::TABLE, p::PARTIAL2).default,
            partial3: crate::params::def(p::TABLE, p::PARTIAL3).default,
            partial4: crate::params::def(p::TABLE, p::PARTIAL4).default,
            partial5: crate::params::def(p::TABLE, p::PARTIAL5).default,
            partial6: crate::params::def(p::TABLE, p::PARTIAL6).default,
            choke: crate::params::def(p::TABLE, p::CHOKE).default,
            contact: crate::params::def(p::TABLE, p::CONTACT).default,
            disperse: crate::params::def(p::TABLE, p::DISPERSE).default,
            disperse_focus: crate::params::def(p::TABLE, p::DISPERSE_FOCUS).default,
            disperse_width: crate::params::def(p::TABLE, p::DISPERSE_WIDTH).default,
            disperse_follow: crate::params::def(p::TABLE, p::DISPERSE_FOLLOW).default,
            bloom_size: crate::params::def(p::TABLE, p::BLOOM_SIZE).default,
            bloom_decay: crate::params::def(p::TABLE, p::BLOOM_DECAY).default,
            bloom_damp: crate::params::def(p::TABLE, p::BLOOM_DAMP).default,
            bloom_mix: crate::params::def(p::TABLE, p::BLOOM_MIX).default,
        }
    }
}
impl RingParams {
    pub fn set(&mut self, id: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        let Some(value) = crate::params::clamp(p::TABLE, id, value) else {
            return;
        };
        match id {
            p::ATTACK => self.attack = value,
            p::DECAY => self.decay = value,
            p::SUSTAIN => self.sustain = value,
            p::RELEASE => self.release = value,
            p::VELOCITY => self.velocity = value,
            p::LEVEL => self.level = value,
            p::TUNE => self.tune = value,
            p::WIDTH => self.width = value,
            p::CUTOFF => self.cutoff = value,
            p::RESONANCE => self.resonance = value,
            p::FILTER_ENV => self.filter_env = value,
            p::FILTER_DECAY => self.filter_decay = value,
            p::KEYTRACK => self.keytrack = value,
            p::MATERIAL => self.material = value,
            p::INHARM => self.inharm = value,
            p::DAMP => self.damp = value,
            p::STRIKE => self.strike = value,
            p::HARD => self.hard = value,
            p::POSITION => self.position = value,
            p::SPREAD => self.spread = value,
            p::FEED => self.feed = value,
            p::PARTIAL1 => self.partial1 = value,
            p::PARTIAL2 => self.partial2 = value,
            p::PARTIAL3 => self.partial3 = value,
            p::PARTIAL4 => self.partial4 = value,
            p::PARTIAL5 => self.partial5 = value,
            p::PARTIAL6 => self.partial6 = value,
            p::CHOKE => self.choke = value,
            p::CONTACT => self.contact = value,
            p::DISPERSE => self.disperse = value,
            p::DISPERSE_FOCUS => self.disperse_focus = value,
            p::DISPERSE_WIDTH => self.disperse_width = value,
            p::DISPERSE_FOLLOW => self.disperse_follow = value,
            p::BLOOM_SIZE => self.bloom_size = value,
            p::BLOOM_DECAY => self.bloom_decay = value,
            p::BLOOM_DAMP => self.bloom_damp = value,
            p::BLOOM_MIX => self.bloom_mix = value,
            _ => {}
        }
    }
    pub fn get(&self, id: u32) -> f32 {
        match id {
            p::ATTACK => self.attack,
            p::DECAY => self.decay,
            p::SUSTAIN => self.sustain,
            p::RELEASE => self.release,
            p::VELOCITY => self.velocity,
            p::LEVEL => self.level,
            p::TUNE => self.tune,
            p::WIDTH => self.width,
            p::CUTOFF => self.cutoff,
            p::RESONANCE => self.resonance,
            p::FILTER_ENV => self.filter_env,
            p::FILTER_DECAY => self.filter_decay,
            p::KEYTRACK => self.keytrack,
            p::MATERIAL => self.material,
            p::INHARM => self.inharm,
            p::DAMP => self.damp,
            p::STRIKE => self.strike,
            p::HARD => self.hard,
            p::POSITION => self.position,
            p::SPREAD => self.spread,
            p::FEED => self.feed,
            p::PARTIAL1 => self.partial1,
            p::PARTIAL2 => self.partial2,
            p::PARTIAL3 => self.partial3,
            p::PARTIAL4 => self.partial4,
            p::PARTIAL5 => self.partial5,
            p::PARTIAL6 => self.partial6,
            p::CHOKE => self.choke,
            p::CONTACT => self.contact,
            p::DISPERSE => self.disperse,
            p::DISPERSE_FOCUS => self.disperse_focus,
            p::DISPERSE_WIDTH => self.disperse_width,
            p::DISPERSE_FOLLOW => self.disperse_follow,
            p::BLOOM_SIZE => self.bloom_size,
            p::BLOOM_DECAY => self.bloom_decay,
            p::BLOOM_DAMP => self.bloom_damp,
            p::BLOOM_MIX => self.bloom_mix,
            _ => 0.0,
        }
    }
}
impl crate::audio::table::bank::Patch for RingParams {
    const KIND: u8 = 1;
    const TABLE: &'static [crate::params::ParamDef] = p::TABLE;
    fn get(self, id: u32) -> f32 {
        RingParams::get(&self, id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.set(id, v)
    }
}
pub struct RingVoices {
    core: crate::audio::table::bank::Bank<RingParams>,
}
impl Default for RingVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl RingVoices {
    pub fn new() -> Self {
        Self {
            core: crate::audio::table::bank::Bank::new(),
        }
    }
    pub fn prepare(&mut self, sr: f32, block: usize, params: RingParams) {
        self.core.prepare(sr, block, params)
    }
    pub fn reset(&mut self) {
        self.core.reset()
    }
    pub fn set_param(&mut self, id: u32, v: f32) {
        self.core.set_param(id, v)
    }
    pub fn plock(&mut self, id: u32, v: Option<f32>) {
        self.core.plock(id, v)
    }
    pub fn plock_glide(&mut self, id: u32, a: f32) {
        self.core.plock_glide(id, a)
    }
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        self.core.note_on(pitch, vel, age)
    }
    pub fn note_off(&mut self, pitch: u8) {
        self.core.note_off(pitch)
    }
    pub fn release_all(&mut self) {
        self.core.release_all()
    }
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        self.core.render(out, at, gain)
    }
    pub fn right(&self, len: usize) -> &[f32] {
        self.core.right(len)
    }
    pub fn latency(&self) -> usize {
        latency(self.core.sample_rate())
    }
}
pub fn latency(_sample_rate: f32) -> usize {
    0
}
pub fn hero(params: &RingParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::table::bank::hero(*params, p::KEYS, page, selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mechanics() {
        crate::audio::table::bank::test_bank::<RingParams>();
    }
    #[test]
    fn pictures() {
        crate::audio::table::bank::test_pictures::<RingParams>(p::KEYS);
    }
}
