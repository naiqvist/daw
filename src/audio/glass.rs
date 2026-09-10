//! Glass — measured, lockable synthesis for the pages.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::params::glass as p;
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlassParams {
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
    pub algorithm: f32,
    pub ratio: f32,
    pub coarse: f32,
    pub fine: f32,
    pub index: f32,
    pub i_decay: f32,
    pub i_sustain: f32,
    pub feedback: f32,
    pub vel_index: f32,
    pub disorder: f32,
    pub pitch_env: f32,
    pub pitch_time: f32,
    pub key_index: f32,
    pub pan_motion: f32,
    pub pan_rate: f32,
    pub phase: f32,
    pub shimmer_pitch: f32,
    pub shimmer_time: f32,
    pub shimmer_feed: f32,
    pub shimmer_mix: f32,
    pub tilt: f32,
    pub tilt_pivot: f32,
    pub tilt_drive: f32,
    pub tilt_mix: f32,
    pub ratio_b: f32,
    pub index_b: f32,
    pub cascade: f32,
    pub sub: f32,
    pub sub_tune: f32,
    pub body: f32,
}
impl Default for GlassParams {
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
            algorithm: crate::params::def(p::TABLE, p::ALGORITHM).default,
            ratio: crate::params::def(p::TABLE, p::RATIO).default,
            coarse: crate::params::def(p::TABLE, p::COARSE).default,
            fine: crate::params::def(p::TABLE, p::FINE).default,
            index: crate::params::def(p::TABLE, p::INDEX).default,
            i_decay: crate::params::def(p::TABLE, p::I_DECAY).default,
            i_sustain: crate::params::def(p::TABLE, p::I_SUSTAIN).default,
            feedback: crate::params::def(p::TABLE, p::FEEDBACK).default,
            vel_index: crate::params::def(p::TABLE, p::VEL_INDEX).default,
            disorder: crate::params::def(p::TABLE, p::DISORDER).default,
            pitch_env: crate::params::def(p::TABLE, p::PITCH_ENV).default,
            pitch_time: crate::params::def(p::TABLE, p::PITCH_TIME).default,
            key_index: crate::params::def(p::TABLE, p::KEY_INDEX).default,
            pan_motion: crate::params::def(p::TABLE, p::PAN_MOTION).default,
            pan_rate: crate::params::def(p::TABLE, p::PAN_RATE).default,
            phase: crate::params::def(p::TABLE, p::PHASE).default,
            shimmer_pitch: crate::params::def(p::TABLE, p::SHIMMER_PITCH).default,
            shimmer_time: crate::params::def(p::TABLE, p::SHIMMER_TIME).default,
            shimmer_feed: crate::params::def(p::TABLE, p::SHIMMER_FEED).default,
            shimmer_mix: crate::params::def(p::TABLE, p::SHIMMER_MIX).default,
            tilt: crate::params::def(p::TABLE, p::TILT).default,
            tilt_pivot: crate::params::def(p::TABLE, p::TILT_PIVOT).default,
            tilt_drive: crate::params::def(p::TABLE, p::TILT_DRIVE).default,
            tilt_mix: crate::params::def(p::TABLE, p::TILT_MIX).default,
            ratio_b: crate::params::def(p::TABLE, p::RATIO_B).default,
            index_b: crate::params::def(p::TABLE, p::INDEX_B).default,
            cascade: crate::params::def(p::TABLE, p::CASCADE).default,
            sub: crate::params::def(p::TABLE, p::SUB).default,
            sub_tune: crate::params::def(p::TABLE, p::SUB_TUNE).default,
            body: crate::params::def(p::TABLE, p::BODY).default,
        }
    }
}
impl GlassParams {
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
            p::ALGORITHM => self.algorithm = value,
            p::RATIO => self.ratio = value,
            p::COARSE => self.coarse = value,
            p::FINE => self.fine = value,
            p::INDEX => self.index = value,
            p::I_DECAY => self.i_decay = value,
            p::I_SUSTAIN => self.i_sustain = value,
            p::FEEDBACK => self.feedback = value,
            p::VEL_INDEX => self.vel_index = value,
            p::DISORDER => self.disorder = value,
            p::PITCH_ENV => self.pitch_env = value,
            p::PITCH_TIME => self.pitch_time = value,
            p::KEY_INDEX => self.key_index = value,
            p::PAN_MOTION => self.pan_motion = value,
            p::PAN_RATE => self.pan_rate = value,
            p::PHASE => self.phase = value,
            p::SHIMMER_PITCH => self.shimmer_pitch = value,
            p::SHIMMER_TIME => self.shimmer_time = value,
            p::SHIMMER_FEED => self.shimmer_feed = value,
            p::SHIMMER_MIX => self.shimmer_mix = value,
            p::TILT => self.tilt = value,
            p::TILT_PIVOT => self.tilt_pivot = value,
            p::TILT_DRIVE => self.tilt_drive = value,
            p::TILT_MIX => self.tilt_mix = value,
            p::RATIO_B => self.ratio_b = value,
            p::INDEX_B => self.index_b = value,
            p::CASCADE => self.cascade = value,
            p::SUB => self.sub = value,
            p::SUB_TUNE => self.sub_tune = value,
            p::BODY => self.body = value,
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
            p::ALGORITHM => self.algorithm,
            p::RATIO => self.ratio,
            p::COARSE => self.coarse,
            p::FINE => self.fine,
            p::INDEX => self.index,
            p::I_DECAY => self.i_decay,
            p::I_SUSTAIN => self.i_sustain,
            p::FEEDBACK => self.feedback,
            p::VEL_INDEX => self.vel_index,
            p::DISORDER => self.disorder,
            p::PITCH_ENV => self.pitch_env,
            p::PITCH_TIME => self.pitch_time,
            p::KEY_INDEX => self.key_index,
            p::PAN_MOTION => self.pan_motion,
            p::PAN_RATE => self.pan_rate,
            p::PHASE => self.phase,
            p::SHIMMER_PITCH => self.shimmer_pitch,
            p::SHIMMER_TIME => self.shimmer_time,
            p::SHIMMER_FEED => self.shimmer_feed,
            p::SHIMMER_MIX => self.shimmer_mix,
            p::TILT => self.tilt,
            p::TILT_PIVOT => self.tilt_pivot,
            p::TILT_DRIVE => self.tilt_drive,
            p::TILT_MIX => self.tilt_mix,
            p::RATIO_B => self.ratio_b,
            p::INDEX_B => self.index_b,
            p::CASCADE => self.cascade,
            p::SUB => self.sub,
            p::SUB_TUNE => self.sub_tune,
            p::BODY => self.body,
            _ => 0.0,
        }
    }
}
impl crate::audio::table::bank::Patch for GlassParams {
    const KIND: u8 = 3;
    const TABLE: &'static [crate::params::ParamDef] = p::TABLE;
    fn get(self, id: u32) -> f32 {
        GlassParams::get(&self, id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.set(id, v)
    }
}
pub struct GlassVoices {
    core: crate::audio::table::bank::Bank<GlassParams>,
}
impl Default for GlassVoices {
    fn default() -> Self {
        Self::new()
    }
}
impl GlassVoices {
    pub fn new() -> Self {
        Self {
            core: crate::audio::table::bank::Bank::new(),
        }
    }
    pub fn prepare(&mut self, sr: f32, block: usize, params: GlassParams) {
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
pub fn hero(params: &GlassParams, page: &str, selected: Option<u32>) -> Option<crate::pages::Hero> {
    crate::audio::table::bank::hero(*params, p::KEYS, page, selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn neuro() -> GlassParams {
        GlassParams {
            ratio: 1.,
            index: 5.,
            ratio_b: 2.01,
            index_b: 4.,
            cascade: 0.7,
            sub: 0.8,
            body: 0.8,
            width: 0.9,
            attack: 2.,
            sustain: 1.,
            release: 40.,
            i_sustain: 0.6,
            ..Default::default()
        }
    }
    fn sound(p: GlassParams) -> (Vec<f32>, Vec<f32>) {
        let mut v = GlassVoices::new();
        v.prepare(48000., 4096, p);
        v.note_on(41, 127, 1);
        let mut x = vec![0.; 4096];
        v.render(&mut x, 0, &mut Ramp::across(1., 1., 1));
        (x, v.right(4096).to_vec())
    }
    #[test]
    fn neuro_controls_serialize_and_have_independent_agency() {
        let p = neuro();
        let encoded = ron::to_string(&p).unwrap();
        assert_eq!(p, ron::from_str::<GlassParams>(&encoded).unwrap());
        let old: GlassParams = ron::from_str("(ratio:1.0,index:3.0)").unwrap();
        assert_eq!((old.index_b, old.sub, old.body), (0., 0., 1.));
        let baseline = sound(p).0;
        for (id, value) in [
            (p::RATIO_B, 3.1),
            (p::INDEX_B, 0.),
            (p::CASCADE, 0.),
            (p::SUB, 0.),
            (p::SUB_TUNE, 0.),
            (p::BODY, 0.),
        ] {
            let mut changed = p;
            changed.set(id, value);
            assert_ne!(baseline, sound(changed).0, "parameter {id}");
        }
    }
    #[test]
    fn neuro_is_exactly_segmentable_and_realtime_safe() {
        let p = neuro();
        let whole = sound(p);
        let mut v = GlassVoices::new();
        v.prepare(48000., 4096, p);
        v.note_on(41, 127, 1);
        let mut x = vec![0.; 4096];
        assert_no_alloc::assert_no_alloc(|| {
            v.render(&mut [], 0, &mut Ramp::across(1., 1., 1));
            let mut at = 0;
            for n in [1, 31, 257, 100, 3707] {
                v.render(&mut x[at..at + n], at, &mut Ramp::across(1., 1., 1));
                at += n;
            }
        });
        assert_eq!(whole.0, x);
        assert_eq!(whole.1, v.right(4096));
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..32 {
                v.note_on(36 + (i % 12) as u8, 127, i + 2);
            }
            v.render(&mut x[..257], 0, &mut Ramp::across(1., 1., 1));
            v.release_all();
            v.reset();
            v.render(&mut x, 0, &mut Ramp::across(1., 1., 1));
        });
        assert!(x.iter().all(|x| *x == 0.));
    }
    #[test]
    fn foundation_is_mono_and_bypasses_internal_colour() {
        let p = GlassParams {
            body: 0.,
            sub: 0.8,
            width: 1.,
            pan_motion: 1.,
            tilt_drive: 12.,
            tilt: 18.,
            shimmer_mix: 1.,
            cutoff: 30.,
            ..neuro()
        };
        let coloured = sound(p);
        assert_eq!(coloured.0, coloured.1);
        assert!(coloured.0.iter().any(|x| x.abs() > 0.01));
        let dry = sound(GlassParams {
            tilt_drive: 0.,
            tilt: 0.,
            shimmer_mix: 0.,
            cutoff: 20000.,
            ..p
        });
        assert_eq!(coloured, dry);
    }
    #[test]
    fn mechanics() {
        crate::audio::table::bank::test_bank::<GlassParams>();
    }
    #[test]
    fn pictures() {
        crate::audio::table::bank::test_pictures::<GlassParams>(p::KEYS);
    }
    #[test]
    fn pitch_picture_changes_with_tune_on_a_fixed_time_axis() {
        let mut p = GlassParams::default();
        let Some(a) = hero(&p, "Pitch", None) else {
            panic!("missing Pitch hero");
        };
        p.tune = 12.;
        let Some(b) = hero(&p, "Pitch", None) else {
            panic!("missing Pitch hero");
        };
        assert_eq!(a.x_labels, b.x_labels);
        assert_ne!(a.title, b.title);
        assert_ne!(a.series[0].points, b.series[0].points);
    }
}
