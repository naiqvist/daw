//! PrismVoice — measured, lockable synthesis for the pages.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::params::prism_voice as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PrismVoiceParams {
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
    pub source: f32,
    pub sieve: f32,
    pub sieve_width: f32,
    pub shift: f32,
    pub tilt: f32,
    pub blur: f32,
    pub freeze: f32,
    pub band: f32,
    pub s_env: f32,
    pub s_time: f32,
    pub shift_lfo: f32,
    pub shift_rate: f32,
    pub f_env: f32,
    pub f_time: f32,
    pub rough: f32,
    pub seed: f32,
    pub smear_low: f32,
    pub smear_high: f32,
    pub smear_feed: f32,
    pub smear_mix: f32,
    pub halo_decay: f32,
    pub halo_tilt: f32,
    pub halo_damp: f32,
    pub halo_mix: f32,
}
impl Default for PrismVoiceParams {
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
            source: crate::params::def(p::TABLE, p::SOURCE).default,
            sieve: crate::params::def(p::TABLE, p::SIEVE).default,
            sieve_width: crate::params::def(p::TABLE, p::SIEVE_WIDTH).default,
            shift: crate::params::def(p::TABLE, p::SHIFT).default,
            tilt: crate::params::def(p::TABLE, p::TILT).default,
            blur: crate::params::def(p::TABLE, p::BLUR).default,
            freeze: crate::params::def(p::TABLE, p::FREEZE).default,
            band: crate::params::def(p::TABLE, p::BAND).default,
            s_env: crate::params::def(p::TABLE, p::S_ENV).default,
            s_time: crate::params::def(p::TABLE, p::S_TIME).default,
            shift_lfo: crate::params::def(p::TABLE, p::SHIFT_LFO).default,
            shift_rate: crate::params::def(p::TABLE, p::SHIFT_RATE).default,
            f_env: crate::params::def(p::TABLE, p::F_ENV).default,
            f_time: crate::params::def(p::TABLE, p::F_TIME).default,
            rough: crate::params::def(p::TABLE, p::ROUGH).default,
            seed: crate::params::def(p::TABLE, p::SEED).default,
            smear_low: crate::params::def(p::TABLE, p::SMEAR_LOW).default,
            smear_high: crate::params::def(p::TABLE, p::SMEAR_HIGH).default,
            smear_feed: crate::params::def(p::TABLE, p::SMEAR_FEED).default,
            smear_mix: crate::params::def(p::TABLE, p::SMEAR_MIX).default,
            halo_decay: crate::params::def(p::TABLE, p::HALO_DECAY).default,
            halo_tilt: crate::params::def(p::TABLE, p::HALO_TILT).default,
            halo_damp: crate::params::def(p::TABLE, p::HALO_DAMP).default,
            halo_mix: crate::params::def(p::TABLE, p::HALO_MIX).default,
        }
    }
}
impl PrismVoiceParams {
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
            p::SOURCE => self.source = value,
            p::SIEVE => self.sieve = value,
            p::SIEVE_WIDTH => self.sieve_width = value,
            p::SHIFT => self.shift = value,
            p::TILT => self.tilt = value,
            p::BLUR => self.blur = value,
            p::FREEZE => self.freeze = value,
            p::BAND => self.band = value,
            p::S_ENV => self.s_env = value,
            p::S_TIME => self.s_time = value,
            p::SHIFT_LFO => self.shift_lfo = value,
            p::SHIFT_RATE => self.shift_rate = value,
            p::F_ENV => self.f_env = value,
            p::F_TIME => self.f_time = value,
            p::ROUGH => self.rough = value,
            p::SEED => self.seed = value,
            p::SMEAR_LOW => self.smear_low = value,
            p::SMEAR_HIGH => self.smear_high = value,
            p::SMEAR_FEED => self.smear_feed = value,
            p::SMEAR_MIX => self.smear_mix = value,
            p::HALO_DECAY => self.halo_decay = value,
            p::HALO_TILT => self.halo_tilt = value,
            p::HALO_DAMP => self.halo_damp = value,
            p::HALO_MIX => self.halo_mix = value,
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
            p::SOURCE => self.source,
            p::SIEVE => self.sieve,
            p::SIEVE_WIDTH => self.sieve_width,
            p::SHIFT => self.shift,
            p::TILT => self.tilt,
            p::BLUR => self.blur,
            p::FREEZE => self.freeze,
            p::BAND => self.band,
            p::S_ENV => self.s_env,
            p::S_TIME => self.s_time,
            p::SHIFT_LFO => self.shift_lfo,
            p::SHIFT_RATE => self.shift_rate,
            p::F_ENV => self.f_env,
            p::F_TIME => self.f_time,
            p::ROUGH => self.rough,
            p::SEED => self.seed,
            p::SMEAR_LOW => self.smear_low,
            p::SMEAR_HIGH => self.smear_high,
            p::SMEAR_FEED => self.smear_feed,
            p::SMEAR_MIX => self.smear_mix,
            p::HALO_DECAY => self.halo_decay,
            p::HALO_TILT => self.halo_tilt,
            p::HALO_DAMP => self.halo_damp,
            p::HALO_MIX => self.halo_mix,
            _ => 0.0,
        }
    }
}
impl crate::audio::table::bank::Patch for PrismVoiceParams {
    const KIND: u8 = 2;
    const TABLE: &'static [crate::params::ParamDef] = p::TABLE;
    fn get(self, id: u32) -> f32 {
        PrismVoiceParams::get(&self, id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.set(id, v)
    }
}
pub struct PrismVoiceVoices {
    core: crate::audio::table::bank::Bank<PrismVoiceParams>,
}
impl Default for PrismVoiceVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl PrismVoiceVoices {
    pub fn new() -> Self {
        Self {
            core: crate::audio::table::bank::Bank::new(),
        }
    }
    pub fn prepare(&mut self, sr: f32, block: usize, params: PrismVoiceParams) {
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
    crate::audio::table::spectral::SIZE
}
pub fn hero(
    params: &PrismVoiceParams,
    page: &str,
    selected: Option<u32>,
) -> Option<crate::pages::Hero> {
    crate::audio::table::bank::hero(*params, p::KEYS, page, selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mechanics() {
        crate::audio::table::bank::test_bank::<PrismVoiceParams>();
    }
    #[test]
    fn pictures() {
        crate::audio::table::bank::test_pictures::<PrismVoiceParams>(p::KEYS);
    }
    #[test]
    fn pictures_use_effective_band_and_halo_decay() {
        let p = PrismVoiceParams::default();
        let Some(band) = hero(&p, "Band", None) else {
            panic!("missing Band hero");
        };
        assert!(band.title.contains("8000 Hz"));
        let Some(halo) = hero(&p, "Halo", None) else {
            panic!("missing Halo hero");
        };
        assert!(halo.title.contains("2.0 s"));
        let mut narrow = p;
        narrow.cutoff = 400.;
        let Some(band) = hero(&narrow, "Band", None) else {
            panic!("missing Band hero");
        };
        assert!(band.title.contains("400 Hz"));
    }
}
