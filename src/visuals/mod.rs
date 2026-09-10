//! Optional, timeline-locked visual instrument. NO dependency from audio code.
//! Compiled score + absolute time -> a frame. No simulation/wall-clock state,
//! so seeking and dropped preview frames cannot change an eventual render.
pub mod command;
pub mod export;
pub mod gpu;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_LAYERS: usize = 16;
pub const MAX_KEYS: usize = 32768;
pub const MAX_TICKS: u64 = 48 * 4 * 4096;
pub const PARAMS: &[(&str, f32, f32, f32)] = &[
    ("x", -4.0, 4.0, 0.0),
    ("y", -4.0, 4.0, 0.0),
    ("scale", 0.01, 8.0, 1.0),
    ("rotation", -36000.0, 36000.0, 0.0),
    ("hue", -16.0, 16.0, 0.55),
    ("saturation", 0.0, 1.0, 0.8),
    ("brightness", 0.0, 4.0, 1.0),
    ("opacity", 0.0, 1.0, 1.0),
    ("frequency", 0.1, 64.0, 6.0),
    ("warp", 0.0, 4.0, 0.15),
    ("softness", 0.001, 1.0, 0.1),
    ("phase", -10000.0, 10000.0, 0.0),
];
pub const X: usize = 0;
pub const Y: usize = 1;
pub const SCALE: usize = 2;
pub const ROTATION: usize = 3;
pub const HUE: usize = 4;
pub const SATURATION: usize = 5;
pub const BRIGHTNESS: usize = 6;
pub const OPACITY: usize = 7;
pub const FREQUENCY: usize = 8;
pub const WARP: usize = 9;
pub const SOFTNESS: usize = 10;
pub const PHASE: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Primitive {
    Disc,
    Rings,
    Ribbon,
    Field,
    Noise,
}
impl Primitive {
    pub fn index(self) -> u32 {
        self as u32
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    Over,
    Add,
    Multiply,
    Screen,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub tick: u64,
    pub value: f32,
    pub slide: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub param: String,
    pub keys: Vec<Key>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Modulator {
    Lfo {
        cycles_per_beat: f64,
        phase: f64,
    },
    Random {
        step_ticks: u64,
        seed: u32,
    },
    /// Beat-synchronous repeating exponential pulse, no audio analysis needed.
    Pulse {
        period_ticks: u64,
        decay_ticks: f64,
        phase_ticks: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mod {
    pub param: String,
    pub depth: f32,
    pub source: Modulator,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub primitive: Primitive,
    pub blend: Blend,
    pub params: [f32; 12],
    pub automation: Vec<Lane>,
    pub modulation: Vec<Mod>,
}
impl Layer {
    pub fn new(id: String, primitive: Primitive) -> Self {
        Self {
            id,
            primitive,
            blend: Blend::Over,
            params: std::array::from_fn(|i| PARAMS[i].3),
            automation: vec![],
            modulation: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub length_ticks: u64,
    pub layers: Vec<Layer>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub clip: String,
    pub at: u64,
    pub length_ticks: u64,
    pub repeat: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub version: u32,
    pub locked: bool,
    pub seed: u32,
    pub background: [f32; 3],
    pub clips: Vec<Clip>,
    pub arrangement: Vec<Placement>,
}
impl Default for Score {
    fn default() -> Self {
        Self {
            version: 1,
            locked: false,
            seed: 1,
            background: [0.006, 0.009, 0.02],
            clips: vec![],
            arrangement: vec![],
        }
    }
}
pub fn param(name: &str) -> Result<usize, String> {
    PARAMS
        .iter()
        .position(|p| p.0 == name)
        .ok_or_else(|| format!("unknown visual parameter {name}"))
}
pub fn valid_value(id: usize, value: f32) -> bool {
    PARAMS
        .get(id)
        .is_some_and(|p| value.is_finite() && (p.1..=p.2).contains(&value))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLayer {
    pub transform: [f32; 4],
    pub colour: [f32; 4],
    pub shape: [f32; 4],
    pub kind: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Frame {
    pub background: [f32; 4],
    pub info: [f32; 4],
    pub layers: [GpuLayer; MAX_LAYERS],
}

#[derive(Clone, Debug)]
struct CompiledLayer {
    layer: Layer,
    lanes: Vec<(usize, Vec<Key>)>,
    mods: Vec<(usize, Mod)>,
}
#[derive(Clone, Debug)]
struct CompiledClip {
    length: u64,
    layers: Vec<CompiledLayer>,
}
#[derive(Clone, Debug)]
pub struct Compiled {
    score: Score,
    clips: BTreeMap<String, CompiledClip>,
}
impl Compiled {
    pub fn new(score: &Score) -> Result<Self, String> {
        if score.version != 1 {
            return Err("unsupported visual score version".into());
        }
        if !score
            .background
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err("background needs linear RGB 0..1".into());
        }
        if score.clips.len() > 256 || score.arrangement.len() > 4096 {
            return Err("visual score budget: 256 clips / 4096 placements".into());
        }
        let mut clips = BTreeMap::new();
        let mut keys = 0;
        for clip in &score.clips {
            if !valid_id(&clip.id)
                || clips.contains_key(&clip.id)
                || !(1..=MAX_TICKS).contains(&clip.length_ticks)
            {
                return Err("unique clip ids and positive bounded clip lengths required".into());
            }
            if clip.layers.len() > MAX_LAYERS {
                return Err("GPU budget: 16 simultaneously visible layers".into());
            }
            let mut names = BTreeSet::new();
            let mut layers = Vec::new();
            for layer in &clip.layers {
                if !valid_id(&layer.id) || !names.insert(&layer.id) {
                    return Err("layer ids must be unique within a clip".into());
                }
                if !layer
                    .params
                    .iter()
                    .enumerate()
                    .all(|(i, v)| valid_value(i, *v))
                {
                    return Err("nonfinite or out-of-range visual parameter".into());
                }
                if layer.modulation.len() > 64 {
                    return Err("visual budget: 64 routes per layer".into());
                }
                let mut lanes = vec![];
                let mut ids = BTreeSet::new();
                for lane in &layer.automation {
                    let id = param(&lane.param)?;
                    if !ids.insert(id) {
                        return Err("duplicate automation lane".into());
                    }
                    let mut points = lane.keys.clone();
                    points.sort_by_key(|k| k.tick);
                    if points.windows(2).any(|w| w[0].tick == w[1].tick)
                        || points
                            .iter()
                            .any(|k| k.tick >= clip.length_ticks || !valid_value(id, k.value))
                    {
                        return Err("invalid/duplicate key timestamp or parameter value".into());
                    }
                    keys += points.len();
                    if keys > MAX_KEYS {
                        return Err("visual budget: 32768 keys".into());
                    }
                    lanes.push((id, points));
                }
                let mut mods = vec![];
                for m in &layer.modulation {
                    let id = param(&m.param)?;
                    if !m.depth.is_finite() || m.depth.abs() > 100000.0 {
                        return Err("invalid modulation depth".into());
                    }
                    let valid = match m.source {
                        Modulator::Lfo {
                            cycles_per_beat,
                            phase,
                        } => {
                            cycles_per_beat.is_finite()
                                && (0.0..=64.0).contains(&cycles_per_beat)
                                && phase.is_finite()
                                && phase.abs() <= 10000.0
                        }
                        Modulator::Random { step_ticks, .. } => {
                            (1..=MAX_TICKS).contains(&step_ticks)
                        }
                        Modulator::Pulse {
                            period_ticks,
                            decay_ticks,
                            phase_ticks,
                        } => {
                            (1..=MAX_TICKS).contains(&period_ticks)
                                && decay_ticks.is_finite()
                                && (0.01..=MAX_TICKS as f64).contains(&decay_ticks)
                                && phase_ticks < period_ticks
                        }
                    };
                    if !valid {
                        return Err("invalid visual modulator".into());
                    }
                    mods.push((id, m.clone()));
                }
                layers.push(CompiledLayer {
                    layer: layer.clone(),
                    lanes,
                    mods,
                });
            }
            clips.insert(
                clip.id.clone(),
                CompiledClip {
                    length: clip.length_ticks,
                    layers,
                },
            );
        }
        let mut edges = vec![];
        for p in &score.arrangement {
            let clip = clips
                .get(&p.clip)
                .ok_or("placement references unknown clip")?;
            if p.length_ticks == 0
                || p.at
                    .checked_add(p.length_ticks)
                    .is_none_or(|end| end > MAX_TICKS)
            {
                return Err("invalid placement extent".into());
            }
            if !p.repeat && p.length_ticks > clip.length {
                return Err("nonrepeating placement exceeds clip; use repeat".into());
            }
            edges.push((p.at, clip.layers.len() as i32));
            edges.push((p.at + p.length_ticks, -(clip.layers.len() as i32)));
        }
        edges.sort();
        let mut active = 0;
        for (_, delta) in edges {
            active += delta;
            if active > MAX_LAYERS as i32 {
                return Err("overlapping clips exceed 16-layer GPU budget".into());
            }
        }
        Ok(Self {
            score: score.clone(),
            clips,
        })
    }
    pub fn end_tick(&self) -> u64 {
        self.score
            .arrangement
            .iter()
            .map(|p| p.at + p.length_ticks)
            .max()
            .unwrap_or(0)
    }
    pub fn score(&self) -> &Score {
        &self.score
    }
    /// Pure evaluation at absolute fractional musical tick. Edges are [start,end).
    /// All previous keys remain observable even after skipped frames/seeks.
    pub fn frame(&self, tick: f64, aspect: f32) -> Frame {
        let mut frame: Frame = bytemuck::Zeroable::zeroed();
        frame.background = [
            self.score.background[0],
            self.score.background[1],
            self.score.background[2],
            1.0,
        ];
        frame.info = [
            if aspect.is_finite() {
                aspect.clamp(0.1, 10.0)
            } else {
                1.0
            },
            0.0,
            0.0,
            0.0,
        ];
        if !tick.is_finite() || tick < 0.0 {
            return frame;
        }
        let mut count = 0;
        for place in &self.score.arrangement {
            if tick < place.at as f64 || tick >= (place.at + place.length_ticks) as f64 {
                continue;
            }
            let clip = &self.clips[&place.clip];
            let local = (tick - place.at as f64) % clip.length as f64;
            for layer in &clip.layers {
                let mut p = layer.layer.params;
                for (id, keys) in &layer.lanes {
                    let after = keys.partition_point(|k| k.tick as f64 <= local);
                    if after > 0 {
                        let a = keys[after - 1];
                        p[*id] = a.value;
                        if a.slide
                            && let Some(b) = keys.get(after)
                        {
                            let t = ((local - a.tick as f64) / (b.tick - a.tick) as f64) as f32;
                            p[*id] += (b.value - a.value) * t;
                        }
                    }
                }
                for (id, m) in &layer.mods {
                    let v = match m.source {
                        Modulator::Lfo {
                            cycles_per_beat,
                            phase,
                        } => {
                            ((local / 48.0 * cycles_per_beat + phase) * std::f64::consts::TAU).sin()
                        }
                        Modulator::Random { step_ticks, seed } => {
                            let mut x = (local as u64 / step_ticks) as u32 ^ seed ^ self.score.seed;
                            x = (x ^ (x >> 16)).wrapping_mul(0x7feb352d);
                            x = (x ^ (x >> 15)).wrapping_mul(0x846ca68b);
                            ((x ^ (x >> 16)) >> 8) as f64 / 8388607.5 - 1.0
                        }
                        Modulator::Pulse {
                            period_ticks,
                            decay_ticks,
                            phase_ticks,
                        } => (-((local - phase_ticks as f64).rem_euclid(period_ticks as f64))
                            / decay_ticks)
                            .exp(),
                    };
                    p[*id] += m.depth * v as f32;
                }
                for (i, v) in p.iter_mut().enumerate() {
                    *v = v.clamp(PARAMS[i].1, PARAMS[i].2);
                }
                frame.layers[count] = GpuLayer {
                    transform: [p[X], p[Y], p[SCALE], p[ROTATION].to_radians()],
                    colour: [
                        p[HUE].rem_euclid(1.0),
                        p[SATURATION],
                        p[BRIGHTNESS],
                        p[OPACITY],
                    ],
                    shape: [p[FREQUENCY], p[WARP], p[SOFTNESS], p[PHASE]],
                    kind: [
                        layer.layer.primitive.index(),
                        layer.layer.blend as u32,
                        self.score.seed,
                        0,
                    ],
                };
                count += 1;
            }
        }
        frame.info[1] = count as f32;
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn score() -> Score {
        let mut s = Score::default();
        let mut l = Layer::new("a".into(), Primitive::Rings);
        l.automation.push(Lane {
            param: "opacity".into(),
            keys: vec![
                Key {
                    tick: 0,
                    value: 0.0,
                    slide: true,
                },
                Key {
                    tick: 48,
                    value: 1.0,
                    slide: false,
                },
            ],
        });
        s.clips.push(Clip {
            id: "c".into(),
            length_ticks: 96,
            layers: vec![l],
        });
        s.arrangement.push(Placement {
            clip: "c".into(),
            at: 48,
            length_ticks: 192,
            repeat: true,
        });
        s
    }
    #[test]
    fn visual_locks_slides_repeat_and_random_access_are_exact() {
        let c = Compiled::new(&score()).unwrap();
        assert_eq!(c.frame(47.0, 1.0).info[1], 0.0);
        assert_eq!(c.frame(72.0, 1.0).layers[0].colour[3], 0.5);
        assert_eq!(c.frame(72.0, 1.0), c.frame(168.0, 1.0));
        let wanted = c.frame(100.0, 1.0);
        for t in [200.0, 0.0, 239.0, 48.0] {
            c.frame(t, 1.0);
        }
        assert_eq!(wanted, c.frame(100.0, 1.0));
        assert_eq!(c.frame(240.0, 1.0).info[1], 0.0);
        assert_no_alloc::assert_no_alloc(|| {
            c.frame(100.0, 1.0);
        });
    }
    #[test]
    fn visual_validation_refuses_bad_graphs_and_nonfinite_values() {
        let s = score();
        let mut bad = s.clone();
        bad.arrangement[0].clip = "unknown".into();
        assert!(Compiled::new(&bad).is_err());
        let mut bad = s.clone();
        bad.clips[0].layers[0].params[0] = f32::NAN;
        assert!(Compiled::new(&bad).is_err());
        let mut bad = s.clone();
        bad.clips[0].layers[0].automation[0].keys[1].tick = 0;
        assert!(Compiled::new(&bad).is_err());
        let mut bad = s.clone();
        bad.arrangement = vec![s.arrangement[0].clone(); 17];
        assert!(Compiled::new(&bad).is_err());
        assert_eq!(
            ron::from_str::<Score>(&ron::to_string(&s).unwrap()).unwrap(),
            s
        );
    }
}
