//! Table — measured, lockable synthesis for the pages.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::params::table as p;
pub(crate) mod bank;
mod picture;
pub(crate) mod spectral;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TableParams {
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
    pub morph: f32,
    pub scan: f32,
    pub scan_time: f32,
    pub sub: f32,
    pub detune: f32,
    pub phase: f32,
    pub vel_morph: f32,
    pub rough: f32,
    pub pw: f32,
    pub voicing: f32,
    pub inversion: f32,
    pub open: f32,
    pub drift: f32,
    pub drift_rate: f32,
    pub motion: f32,
    pub motion_rate: f32,
    pub ripple_focus: f32,
    pub ripple_feed: f32,
    pub ripple_damp: f32,
    pub ripple_mix: f32,
    pub fold: f32,
    pub fold_tilt: f32,
    pub fold_bias: f32,
    pub fold_mix: f32,
    pub ensemble_depth: f32,
    pub ensemble_rate: f32,
    pub ensemble_spread: f32,
    pub ensemble_mix: f32,
    pub slap_time: f32,
    pub slap_feed: f32,
    pub slap_damp: f32,
    pub slap_mix: f32,
}
impl Default for TableParams {
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
            morph: crate::params::def(p::TABLE, p::MORPH).default,
            scan: crate::params::def(p::TABLE, p::SCAN).default,
            scan_time: crate::params::def(p::TABLE, p::SCAN_TIME).default,
            sub: crate::params::def(p::TABLE, p::SUB).default,
            detune: crate::params::def(p::TABLE, p::DETUNE).default,
            phase: crate::params::def(p::TABLE, p::PHASE).default,
            vel_morph: crate::params::def(p::TABLE, p::VEL_MORPH).default,
            rough: crate::params::def(p::TABLE, p::ROUGH).default,
            pw: crate::params::def(p::TABLE, p::PW).default,
            voicing: crate::params::def(p::TABLE, p::VOICING).default,
            inversion: crate::params::def(p::TABLE, p::INVERSION).default,
            open: crate::params::def(p::TABLE, p::OPEN).default,
            drift: crate::params::def(p::TABLE, p::DRIFT).default,
            drift_rate: crate::params::def(p::TABLE, p::DRIFT_RATE).default,
            motion: crate::params::def(p::TABLE, p::MOTION).default,
            motion_rate: crate::params::def(p::TABLE, p::MOTION_RATE).default,
            ripple_focus: crate::params::def(p::TABLE, p::RIPPLE_FOCUS).default,
            ripple_feed: crate::params::def(p::TABLE, p::RIPPLE_FEED).default,
            ripple_damp: crate::params::def(p::TABLE, p::RIPPLE_DAMP).default,
            ripple_mix: crate::params::def(p::TABLE, p::RIPPLE_MIX).default,
            fold: crate::params::def(p::TABLE, p::FOLD).default,
            fold_tilt: crate::params::def(p::TABLE, p::FOLD_TILT).default,
            fold_bias: crate::params::def(p::TABLE, p::FOLD_BIAS).default,
            fold_mix: crate::params::def(p::TABLE, p::FOLD_MIX).default,
            ensemble_depth: crate::params::def(p::TABLE, p::ENSEMBLE_DEPTH).default,
            ensemble_rate: crate::params::def(p::TABLE, p::ENSEMBLE_RATE).default,
            ensemble_spread: crate::params::def(p::TABLE, p::ENSEMBLE_SPREAD).default,
            ensemble_mix: crate::params::def(p::TABLE, p::ENSEMBLE_MIX).default,
            slap_time: crate::params::def(p::TABLE, p::SLAP_TIME).default,
            slap_feed: crate::params::def(p::TABLE, p::SLAP_FEED).default,
            slap_damp: crate::params::def(p::TABLE, p::SLAP_DAMP).default,
            slap_mix: crate::params::def(p::TABLE, p::SLAP_MIX).default,
        }
    }
}
impl TableParams {
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
            p::MORPH => self.morph = value,
            p::SCAN => self.scan = value,
            p::SCAN_TIME => self.scan_time = value,
            p::SUB => self.sub = value,
            p::DETUNE => self.detune = value,
            p::PHASE => self.phase = value,
            p::VEL_MORPH => self.vel_morph = value,
            p::ROUGH => self.rough = value,
            p::PW => self.pw = value,
            p::VOICING => self.voicing = value,
            p::INVERSION => self.inversion = value,
            p::OPEN => self.open = value,
            p::DRIFT => self.drift = value,
            p::DRIFT_RATE => self.drift_rate = value,
            p::MOTION => self.motion = value,
            p::MOTION_RATE => self.motion_rate = value,
            p::RIPPLE_FOCUS => self.ripple_focus = value,
            p::RIPPLE_FEED => self.ripple_feed = value,
            p::RIPPLE_DAMP => self.ripple_damp = value,
            p::RIPPLE_MIX => self.ripple_mix = value,
            p::FOLD => self.fold = value,
            p::FOLD_TILT => self.fold_tilt = value,
            p::FOLD_BIAS => self.fold_bias = value,
            p::FOLD_MIX => self.fold_mix = value,
            p::ENSEMBLE_DEPTH => self.ensemble_depth = value,
            p::ENSEMBLE_RATE => self.ensemble_rate = value,
            p::ENSEMBLE_SPREAD => self.ensemble_spread = value,
            p::ENSEMBLE_MIX => self.ensemble_mix = value,
            p::SLAP_TIME => self.slap_time = value,
            p::SLAP_FEED => self.slap_feed = value,
            p::SLAP_DAMP => self.slap_damp = value,
            p::SLAP_MIX => self.slap_mix = value,
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
            p::MORPH => self.morph,
            p::SCAN => self.scan,
            p::SCAN_TIME => self.scan_time,
            p::SUB => self.sub,
            p::DETUNE => self.detune,
            p::PHASE => self.phase,
            p::VEL_MORPH => self.vel_morph,
            p::ROUGH => self.rough,
            p::PW => self.pw,
            p::VOICING => self.voicing,
            p::INVERSION => self.inversion,
            p::OPEN => self.open,
            p::DRIFT => self.drift,
            p::DRIFT_RATE => self.drift_rate,
            p::MOTION => self.motion,
            p::MOTION_RATE => self.motion_rate,
            p::RIPPLE_FOCUS => self.ripple_focus,
            p::RIPPLE_FEED => self.ripple_feed,
            p::RIPPLE_DAMP => self.ripple_damp,
            p::RIPPLE_MIX => self.ripple_mix,
            p::FOLD => self.fold,
            p::FOLD_TILT => self.fold_tilt,
            p::FOLD_BIAS => self.fold_bias,
            p::FOLD_MIX => self.fold_mix,
            p::ENSEMBLE_DEPTH => self.ensemble_depth,
            p::ENSEMBLE_RATE => self.ensemble_rate,
            p::ENSEMBLE_SPREAD => self.ensemble_spread,
            p::ENSEMBLE_MIX => self.ensemble_mix,
            p::SLAP_TIME => self.slap_time,
            p::SLAP_FEED => self.slap_feed,
            p::SLAP_DAMP => self.slap_damp,
            p::SLAP_MIX => self.slap_mix,
            _ => 0.0,
        }
    }
}
impl crate::audio::table::bank::Patch for TableParams {
    const KIND: u8 = 0;
    const TABLE: &'static [crate::params::ParamDef] = p::TABLE;
    fn get(self, id: u32) -> f32 {
        TableParams::get(&self, id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.set(id, v)
    }
}
pub struct TableVoices {
    core: crate::audio::table::bank::Bank<TableParams>,
}
impl Default for TableVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl TableVoices {
    pub fn new() -> Self {
        Self {
            core: crate::audio::table::bank::Bank::new(),
        }
    }
    pub fn prepare(&mut self, sr: f32, block: usize, params: TableParams) {
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
pub fn hero(params: &TableParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::table::bank::hero(*params, p::KEYS, page, selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mechanics() {
        crate::audio::table::bank::test_bank::<TableParams>();
    }
    #[test]
    fn pictures() {
        crate::audio::table::bank::test_pictures::<TableParams>(p::KEYS);
    }
}
