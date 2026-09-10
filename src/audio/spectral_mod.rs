//! Compiled modulation routes for the spectral prototype. General envelopes
//! become pitch/amp envelopes by destination, not separate hardwired features.
//! The first scope boundary is explicit: voice pitch/amp vs shared spectrum/FX.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use super::spectral_fx;
use crate::dsp::{
    LANES, LaneFrame,
    adsr::LaneAdsr,
    lfo::{LaneLfo, LfoShape},
    noise::WhiteNoise,
    ramps::LinearRamp,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    Voice,
    Shared,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Shape {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
}
impl Shape {
    fn kernel(self) -> LfoShape {
        match self {
            Self::Sine => LfoShape::Sine,
            Self::Triangle => LfoShape::Triangle,
            Self::SawUp => LfoShape::SawUp,
            Self::SawDown => LfoShape::SawDown,
            Self::Square => LfoShape::Square,
        }
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Generator {
    Lfo {
        shape: Shape,
        hz: f32,
        phase: f32,
        retrigger: bool,
    },
    Envelope {
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    },
    NoteRandom {
        seed: u64,
    },
    Velocity,
    /// One-based canonical performance macro; shared, smoothed and p-lockable.
    Macro {
        index: u8,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn route(source: &str, target: Target, depth: f32) -> Route {
        Route {
            source: source.into(),
            target,
            depth,
            polarity: Polarity::Native,
            offset: 0.0,
        }
    }
    #[test]
    fn spectral_macro_is_smoothed_lockable_and_fans_out_without_allocation() {
        let fxpatch = spectral_fx::Patch::default();
        let mut patch = Patch {
            sources: vec![Source {
                id: "m".into(),
                scope: Scope::Shared,
                generator: Generator::Macro { index: 8 },
            }],
            routes: vec![
                route("m", Target::PitchSemitones, 12.0),
                route(
                    "m",
                    Target::HarmonicAmp {
                        first: 1,
                        last: 128,
                    },
                    0.1,
                ),
            ],
        };
        let mut p = patch.prepare(48000.0, &fxpatch).unwrap();
        let mut fx = fxpatch.prepare(48000.0, 16, 4096).unwrap();
        p.set_macro(7, 1.0, 4);
        assert_no_alloc::assert_no_alloc(|| {
            for expected in [3.0, 6.0, 9.0, 12.0] {
                let mut amps = [0.0; 128];
                let controls = p.tick(&mut amps, &mut [0.0; 128], &mut fx);
                assert_eq!(controls.pitch, [expected; LANES]);
                assert!((amps[127] - expected / 120.0).abs() < 1e-6);
            }
        });
        p.reset();
        assert_eq!(tick(&mut p, &mut fx).pitch, [0.0; LANES]);
        patch.sources[0].generator = Generator::Macro { index: 0 };
        assert!(patch.prepare(48000.0, &fxpatch).is_err());
        patch.sources[0].generator = Generator::Macro { index: 8 };
        patch.sources[0].scope = Scope::Voice;
        assert!(patch.prepare(48000.0, &fxpatch).is_err());
    }
    fn tick(p: &mut Prepared, fx: &mut spectral_fx::Prepared) -> Controls {
        p.tick(&mut [0.0; 128], &mut [0.0; 128], fx)
    }
    #[test]
    fn per_note_random_is_held_reproducible_and_other_voices_are_untouched() {
        let patch = Patch {
            sources: vec![Source {
                id: "rnd".into(),
                scope: Scope::Voice,
                generator: Generator::NoteRandom { seed: 71 },
            }],
            routes: vec![route("rnd", Target::PitchSemitones, 0.2)],
        };
        let fxpatch = spectral_fx::Patch::default();
        let mut fx = fxpatch.prepare(48_000.0, 16, 4096).unwrap();
        let mut p = patch.prepare(48_000.0, &fxpatch).unwrap();
        p.note_on(0, 0.7);
        let first = tick(&mut p, &mut fx).pitch;
        assert_ne!(first[0], 0.0);
        assert_eq!(first[1..], [0.0; LANES - 1]);
        for _ in 0..100 {
            assert_eq!(tick(&mut p, &mut fx).pitch, first);
        }
        p.note_on(1, 0.8);
        let second = tick(&mut p, &mut fx).pitch;
        assert_eq!(first[0], second[0]);
        assert_ne!(second[1], 0.0);
        p.reset();
        p.note_on(0, 0.7);
        assert_eq!(tick(&mut p, &mut fx).pitch, first);
    }
    #[test]
    fn general_envelope_drives_pitch_and_amplitude_independently_per_voice() {
        let mut amplitude = route("env", Target::Amplitude, 1.0);
        amplitude.offset = -1.0;
        let patch = Patch {
            sources: vec![Source {
                id: "env".into(),
                scope: Scope::Voice,
                generator: Generator::Envelope {
                    attack_ms: 10.0,
                    decay_ms: 20.0,
                    sustain: 0.5,
                    release_ms: 10.0,
                },
            }],
            routes: vec![route("env", Target::PitchSemitones, 12.0), amplitude],
        };
        let fxpatch = spectral_fx::Patch::default();
        let mut fx = fxpatch.prepare(48_000.0, 16, 4096).unwrap();
        let mut p = patch.prepare(48_000.0, &fxpatch).unwrap();
        p.note_on(0, 1.0);
        let mut before = Controls {
            pitch: [0.0; LANES],
            amplitude: [0.0; LANES],
            shift: 0.0,
        };
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..240 {
                before = tick(&mut p, &mut fx);
            }
        });
        assert!(before.pitch[0] > 0.0);
        assert_eq!(before.pitch[1], 0.0);
        assert_eq!(before.amplitude[1], 0.0);
        assert!((before.pitch[0] - before.amplitude[0] * 12.0).abs() < 1e-5);
        p.note_on(1, 1.0);
        let now = tick(&mut p, &mut fx);
        assert!(now.pitch[0] > before.pitch[0]);
        assert!(now.pitch[1] < now.pitch[0]);
        p.note_off(0);
        for _ in 0..1000 {
            tick(&mut p, &mut fx);
        }
        let now = tick(&mut p, &mut fx);
        assert_eq!(now.amplitude[0], 0.0);
        assert!(now.amplitude[1] > 0.0);
    }
    #[test]
    fn shared_lfo_fans_out_to_bins_phase_and_fx_routes_sum() {
        let fxpatch = spectral_fx::Patch {
            version: 1,
            nodes: vec![spectral_fx::Node {
                id: "vca".into(),
                module: spectral_fx::Module::Gain { gain: 1.0 },
            }],
            routes: vec![
                spectral_fx::Route::new("input", "vca", 1.0),
                spectral_fx::Route::new("vca", "output", 1.0),
            ],
        };
        let target = Target::Fx {
            node: "vca".into(),
            control: spectral_fx::Control::Gain,
        };
        let patch = Patch {
            sources: vec![Source {
                id: "lfo".into(),
                scope: Scope::Shared,
                generator: Generator::Lfo {
                    shape: Shape::Sine,
                    hz: 1.0,
                    phase: 0.25,
                    retrigger: false,
                },
            }],
            routes: vec![
                route("lfo", Target::HarmonicAmp { first: 3, last: 5 }, 0.1),
                route("lfo", Target::HarmonicPhase { first: 1, last: 2 }, 90.0),
                route("lfo", target.clone(), 0.25),
                route("lfo", target, 0.25),
            ],
        };
        let mut p = patch.prepare(48_000.0, &fxpatch).unwrap();
        let mut fx = fxpatch.prepare(48_000.0, 16, 4096).unwrap();
        let mut amps = [0.0; 128];
        let mut phase = amps;
        assert_no_alloc::assert_no_alloc(|| {
            p.tick(&mut amps, &mut phase, &mut fx);
        });
        assert_eq!(amps[2..5], [0.1; 3]);
        assert_eq!(amps[0], 0.0);
        assert_eq!(phase[..2], [90.0; 2]);
        let mut audio = [1.0];
        fx.process(&mut audio);
        assert_eq!(audio, [1.5]);
    }
    #[test]
    fn refuses_ambiguous_voice_scope_structural_fx_targets_and_bad_names() {
        let fx = spectral_fx::Patch::default();
        let mut patch = Patch {
            sources: vec![Source {
                id: "voice".into(),
                scope: Scope::Voice,
                generator: Generator::Velocity,
            }],
            routes: vec![route("voice", Target::ShiftBins, 1.0)],
        };
        assert!(patch.prepare(48_000.0, &fx).is_err());
        patch.sources[0].scope = Scope::Shared;
        assert!(patch.prepare(48_000.0, &fx).is_ok());
        patch.routes[0].source = "missing".into();
        assert!(patch.prepare(48_000.0, &fx).is_err());
        patch.routes[0].source = "voice".into();
        patch.routes[0].depth = f32::NAN;
        assert!(patch.prepare(48_000.0, &fx).is_err());
        patch.routes[0].depth = 1.0;
        patch.routes[0].target = Target::Fx {
            node: "missing".into(),
            control: spectral_fx::Control::Hz,
        };
        assert!(patch.prepare(48_000.0, &fx).is_err());
        patch.routes[0].target = Target::HarmonicAmp { first: 0, last: 5 };
        assert!(patch.prepare(48_000.0, &fx).is_err());
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub id: String,
    pub scope: Scope,
    pub generator: Generator,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Target {
    PitchSemitones,
    /// Additive gain offset around one; sums clamp to 0..2 at destination.
    Amplitude,
    /// One-based, inclusive harmonic selection, applied to shared spectrum.
    HarmonicAmp {
        first: usize,
        last: usize,
    },
    HarmonicPhase {
        first: usize,
        last: usize,
    },
    ShiftBins,
    Fx {
        node: String,
        control: spectral_fx::Control,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Polarity {
    Native,
    Unipolar,
    Bipolar,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Route {
    pub source: String,
    pub target: Target,
    pub depth: f32,
    pub polarity: Polarity,
    /// Applied after polarity and depth. E.g. an envelope -> amplitude with
    /// depth 1, offset -1 multiplies the note's existing amplitude by the envelope.
    #[serde(default)]
    pub offset: f32,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Patch {
    pub sources: Vec<Source>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug)]
enum Runtime {
    Lfo {
        osc: LaneLfo,
        phase: f32,
        retrigger: bool,
    },
    Envelope(LaneAdsr),
    Random {
        rng: WhiteNoise,
        held: LaneFrame,
    },
    Velocity(LaneFrame),
    Macro(usize),
}
#[derive(Clone, Debug)]
struct PreparedSource {
    runtime: Runtime,
    scope: Scope,
    values: LaneFrame,
    bipolar: bool,
}
#[derive(Clone, Debug)]
struct PreparedRoute {
    source: usize,
    target: usize,
    depth: f32,
    polarity: Polarity,
    offset: f32,
}
#[derive(Clone, Debug)]
struct Destination {
    target: Target,
    fx: Option<(spectral_fx::Target, f32)>,
    value: LaneFrame,
}
#[derive(Clone, Debug)]
pub struct Prepared {
    sources: Vec<PreparedSource>,
    routes: Vec<PreparedRoute>,
    destinations: Vec<Destination>,
    held: [bool; LANES],
    macros: [LinearRamp; 8],
}
#[derive(Clone, Copy, Debug)]
pub struct Controls {
    pub pitch: LaneFrame,
    pub amplitude: LaneFrame,
    pub shift: f32,
}

impl Patch {
    pub fn prepare(&self, sr: f32, fx: &spectral_fx::Patch) -> Result<Prepared, String> {
        if !sr.is_finite() || !(8000.0..=192_000.0).contains(&sr) {
            return Err("invalid modulation sample rate".into());
        }
        let finite_range = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        let mut ids = BTreeMap::new();
        let mut sources = Vec::new();
        for (i, source) in self.sources.iter().enumerate() {
            if source.id.is_empty() || ids.insert(source.id.as_str(), i).is_some() {
                return Err("modulator names must be nonempty and unique".into());
            }
            let (runtime, bipolar) = match source.generator {
                Generator::Lfo {
                    shape,
                    hz,
                    phase,
                    retrigger,
                } => {
                    if !finite_range(hz, 0.0, 100.0) || !finite_range(phase, 0.0, 1.0) {
                        return Err(format!("{}: LFO needs 0..100 Hz, phase 0..1", source.id));
                    }
                    let mut osc = LaneLfo::new();
                    osc.prepare(sr, hz, shape.kernel());
                    for lane in 0..LANES {
                        osc.set_phase(lane, phase);
                    }
                    (
                        Runtime::Lfo {
                            osc,
                            phase,
                            retrigger,
                        },
                        true,
                    )
                }
                Generator::Envelope {
                    attack_ms,
                    decay_ms,
                    sustain,
                    release_ms,
                } => {
                    if ![attack_ms, decay_ms, release_ms]
                        .iter()
                        .all(|v| finite_range(*v, 1.0, 120_000.0))
                        || !finite_range(sustain, 0.0, 1.0)
                    {
                        return Err(format!(
                            "{}: envelope needs finite 1..120000 ms and sustain 0..1",
                            source.id
                        ));
                    }
                    let mut env = LaneAdsr::new();
                    env.prepare(sr, attack_ms, decay_ms, sustain, release_ms);
                    (Runtime::Envelope(env), false)
                }
                Generator::NoteRandom { seed } => {
                    let mut rng = WhiteNoise::new();
                    rng.seed(seed);
                    (
                        Runtime::Random {
                            rng,
                            held: [0.0; LANES],
                        },
                        true,
                    )
                }
                Generator::Velocity => (Runtime::Velocity([0.0; LANES]), false),
                Generator::Macro { index } => {
                    if !(1..=8).contains(&index) || source.scope != Scope::Shared {
                        return Err("macros need shared scope and index 1..8".into());
                    }
                    (Runtime::Macro(index as usize - 1), false)
                }
            };
            sources.push(PreparedSource {
                runtime,
                scope: source.scope,
                values: [0.0; LANES],
                bipolar,
            });
        }
        let mut destinations: Vec<Destination> = Vec::new();
        let mut routes = Vec::new();
        for route in &self.routes {
            let &source = ids
                .get(route.source.as_str())
                .ok_or_else(|| format!("unknown modulator {}", route.source))?;
            if !finite_range(route.depth, -100_000.0, 100_000.0)
                || !finite_range(route.offset, -100_000.0, 100_000.0)
            {
                return Err(
                    "modulation depth/offset must be finite, -100000..100000 in destination units"
                        .into(),
                );
            }
            let shared = !matches!(route.target, Target::PitchSemitones | Target::Amplitude);
            if shared && sources[source].scope != Scope::Shared {
                return Err("voice modulator cannot drive a shared spectrum/FX target without an explicit reducer; use a shared source in this prototype".into());
            }
            if let Target::HarmonicAmp { first, last } | Target::HarmonicPhase { first, last } =
                route.target
            {
                if first == 0 || last < first || last > 128 {
                    return Err("harmonic range must be inclusive 1..128".into());
                }
            }
            let fx_target = if let Target::Fx { node, control } = &route.target {
                Some(fx.resolve_control(node, *control)?)
            } else {
                None
            };
            let target = if let Some(i) = destinations.iter().position(|d| d.target == route.target)
            {
                i
            } else {
                destinations.push(Destination {
                    target: route.target.clone(),
                    fx: fx_target,
                    value: [0.0; LANES],
                });
                destinations.len() - 1
            };
            routes.push(PreparedRoute {
                source,
                target,
                depth: route.depth,
                polarity: route.polarity,
                offset: route.offset,
            });
        }
        Ok(Prepared {
            sources,
            routes,
            destinations,
            held: [false; LANES],
            macros: [LinearRamp::new(); 8],
        })
    }
}

impl Prepared {
    pub fn set_macro(&mut self, index: usize, value: f32, samples: u32) {
        if value.is_finite() {
            if let Some(ramp) = self.macros.get_mut(index) {
                ramp.glide(value.clamp(0.0, 1.0), samples);
            }
        }
    }
    pub fn note_on(&mut self, lane: usize, velocity: f32) {
        if lane >= LANES {
            return;
        }
        self.held[lane] = true;
        for source in &mut self.sources {
            let i = if source.scope == Scope::Shared {
                0
            } else {
                lane
            };
            match &mut source.runtime {
                Runtime::Lfo {
                    osc,
                    phase,
                    retrigger,
                } => {
                    if *retrigger {
                        osc.set_phase(i, *phase);
                    }
                }
                Runtime::Envelope(env) => {
                    env.reset_lane(i);
                    env.gate_on(i);
                }
                Runtime::Random { rng, held } => rng.process(&mut held[i..i + 1]),
                Runtime::Velocity(values) => {
                    values[i] = if velocity.is_finite() {
                        velocity.clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }
                Runtime::Macro(_) => {}
            }
        }
    }
    pub fn note_off(&mut self, lane: usize) {
        if lane >= LANES {
            return;
        }
        self.held[lane] = false;
        for source in &mut self.sources {
            let i = if source.scope == Scope::Shared {
                if self.held.iter().any(|v| *v) {
                    continue;
                }
                0
            } else {
                lane
            };
            if let Runtime::Envelope(env) = &mut source.runtime {
                env.gate_off(i);
            }
        }
    }
    pub fn reset(&mut self) {
        self.held.fill(false);
        for ramp in &mut self.macros {
            ramp.reset();
        }
        for source in &mut self.sources {
            source.values.fill(0.0);
            match &mut source.runtime {
                Runtime::Lfo { osc, phase, .. } => {
                    osc.reset();
                    for lane in 0..LANES {
                        osc.set_phase(lane, *phase);
                    }
                }
                Runtime::Envelope(env) => env.reset(),
                Runtime::Random { rng, held } => {
                    rng.reset();
                    held.fill(0.0);
                }
                Runtime::Velocity(values) => values.fill(0.0),
                Runtime::Macro(_) => {}
            }
        }
    }
    /// One audio-sample step. Connections combine additively, in document order.
    /// Source polarity conversions happen BEFORE signed depth multiplication.
    pub fn tick(
        &mut self,
        amps: &mut [f32; 128],
        phases: &mut [f32; 128],
        fx: &mut spectral_fx::Prepared,
    ) -> Controls {
        let mut macros = [0.0; 8];
        for (ramp, value) in self.macros.iter_mut().zip(&mut macros) {
            ramp.process(core::slice::from_mut(value));
        }
        for source in &mut self.sources {
            match &mut source.runtime {
                Runtime::Lfo { osc, .. } => osc.process(core::slice::from_mut(&mut source.values)),
                Runtime::Envelope(env) => env.process(core::slice::from_mut(&mut source.values)),
                Runtime::Random { held, .. } | Runtime::Velocity(held) => source.values = *held,
                Runtime::Macro(index) => source.values.fill(macros[*index]),
            }
            if source.scope == Scope::Shared {
                let value = source.values[0];
                source.values.fill(value);
            }
        }
        for target in &mut self.destinations {
            target.value.fill(0.0);
        }
        for route in &self.routes {
            let source = &self.sources[route.source];
            let target = &mut self.destinations[route.target];
            for lane in 0..LANES {
                let raw = source.values[lane];
                let value = match (source.bipolar, route.polarity) {
                    (true, Polarity::Unipolar) => (raw + 1.0) * 0.5,
                    (false, Polarity::Bipolar) => raw * 2.0 - 1.0,
                    _ => raw,
                };
                target.value[lane] += value * route.depth + route.offset;
            }
        }
        let mut controls = Controls {
            pitch: [0.0; LANES],
            amplitude: [1.0; LANES],
            shift: 0.0,
        };
        for destination in &self.destinations {
            match destination.target {
                Target::PitchSemitones => {
                    for lane in 0..LANES {
                        controls.pitch[lane] = destination.value[lane].clamp(-96.0, 96.0);
                    }
                }
                Target::Amplitude => {
                    for lane in 0..LANES {
                        controls.amplitude[lane] = (1.0 + destination.value[lane]).clamp(0.0, 2.0);
                    }
                }
                Target::HarmonicAmp { first, last } => {
                    for amp in &mut amps[first - 1..last] {
                        *amp += destination.value[0];
                    }
                }
                Target::HarmonicPhase { first, last } => {
                    for phase in &mut phases[first - 1..last] {
                        *phase += destination.value[0];
                    }
                }
                Target::ShiftBins => controls.shift = destination.value[0].clamp(-32.0, 32.0),
                Target::Fx { .. } => {
                    if let Some((target, base)) = destination.fx {
                        fx.set_control(target, base + destination.value[0]);
                    }
                }
            }
        }
        // Overlapping harmonic selections combine before destination clamp.
        for amp in amps {
            *amp = amp.clamp(0.0, 1.0);
        }
        controls
    }
}
