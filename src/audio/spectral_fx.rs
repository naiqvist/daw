//! Instrument-owned, compiled serial/parallel audio patch. Not the DAW graph.
//!
//! Authoring has no node/chain-count ceiling. Preparation admits the requested
//! workspace budget, resolves names and sorts the DAG on the green side.
//! Playback follows that immutable schedule using preallocated buffers only.
//! These first adapters have mono audio ports and no compensation latency;
//! delay/reverb time is intentional sound, not plugin latency. Other signal
//! domains and graph feedback require explicit adapters, not unsafe coercion.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::{delay::FeedbackDelay, filters, mem, reverb::Reverb, shaper};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const INPUT: &str = "input";
pub const OUTPUT: &str = "output";
pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Peak,
    Allpass,
}
impl FilterMode {
    fn kernel(self) -> filters::Mode {
        match self {
            Self::Lowpass => filters::Mode::Lowpass,
            Self::Highpass => filters::Mode::Highpass,
            Self::Bandpass => filters::Mode::BandpassUnity,
            Self::Notch => filters::Mode::Notch,
            Self::Peak => filters::Mode::Peak,
            Self::Allpass => filters::Mode::Allpass,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ShapeMode {
    Clip,
    Soft,
    Cubic,
    Fold,
    Crush,
}
impl ShapeMode {
    fn kernel(self) -> shaper::Mode {
        match self {
            Self::Clip => shaper::Mode::HardClip,
            Self::Soft => shaper::Mode::SoftClip,
            Self::Cubic => shaper::Mode::Cubic,
            Self::Fold => shaper::Mode::Fold,
            Self::Crush => shaper::Mode::Crush,
        }
    }
}

/// Prepared audio adapters. This catalog is intentionally honest: not every
/// kernel has an adapter yet. All current settings are preparation-time edits.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Module {
    Gain {
        gain: f32,
    },
    Filter {
        mode: FilterMode,
        hz: f32,
        q: f32,
    },
    OnePole {
        highpass: bool,
        hz: f32,
    },
    Shape {
        mode: ShapeMode,
        drive: f32,
        bias: f32,
        mix: f32,
    },
    Delay {
        ms: f32,
        feedback: f32,
        damp_hz: f32,
    },
    Disperser {
        hz: f32,
        q: f32,
        stages: u32,
    },
    Tilt {
        hz: f32,
        db: f32,
    },
    DcBlock,
    Reverb {
        size: f32,
        decay: f32,
        damp: f32,
    },
}

/// First audio-rate control adapters. Structural settings (delay capacity,
/// topology, disperser stage count) deliberately cannot be modulation targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Control {
    Gain,
    Hz,
    Q,
    Drive,
    Bias,
    Mix,
    Feedback,
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    slot: usize,
    control: Control,
}

impl Module {
    pub fn control_value(&self, control: Control) -> Option<f32> {
        match (self, control) {
            (Self::Gain { gain }, Control::Gain) => Some(*gain),
            (Self::Filter { hz, .. } | Self::OnePole { hz, .. }, Control::Hz) => Some(*hz),
            (Self::Filter { q, .. }, Control::Q) => Some(*q),
            (Self::Shape { drive, .. }, Control::Drive) => Some(*drive),
            (Self::Shape { bias, .. }, Control::Bias) => Some(*bias),
            (Self::Shape { mix, .. }, Control::Mix) => Some(*mix),
            (Self::Delay { feedback, .. }, Control::Feedback) => Some(*feedback),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gain { .. } => "gain",
            Self::Filter { .. } => "svf",
            Self::OnePole { .. } => "one-pole",
            Self::Shape { .. } => "waveshaper",
            Self::Delay { .. } => "feedback-delay",
            Self::Disperser { .. } => "disperser",
            Self::Tilt { .. } => "tilt",
            Self::DcBlock => "dc-block",
            Self::Reverb { .. } => "reverb",
        }
    }

    fn validate(&self) -> Result<(), String> {
        let range = |name: &str, value: f32, lo: f32, hi: f32| {
            if value.is_finite() && (lo..=hi).contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be finite and in {lo}..={hi}"))
            }
        };
        match *self {
            Self::Gain { gain } => range("gain", gain, -32.0, 32.0),
            Self::Filter { hz, q, .. } => {
                range("hz", hz, 20.0, 20_000.0)?;
                range("q", q, 0.1, 16.0)
            }
            Self::OnePole { hz, .. } => range("hz", hz, 20.0, 20_000.0),
            Self::Shape {
                drive, bias, mix, ..
            } => {
                range("drive", drive, 1.0, 32.0)?;
                range("bias", bias, -0.9, 0.9)?;
                range("mix", mix, 0.0, 1.0)
            }
            Self::Delay {
                ms,
                feedback,
                damp_hz,
            } => {
                range("ms", ms, 1.0, 4000.0)?;
                range("feedback", feedback, -0.95, 0.95)?;
                range("damp_hz", damp_hz, 20.0, 20_000.0)
            }
            Self::Disperser { hz, q, stages } => {
                range("hz", hz, 20.0, 20_000.0)?;
                range("q", q, 0.1, 16.0)?;
                if stages as usize <= filters::DISPERSER_MAX_STAGES {
                    Ok(())
                } else {
                    Err("disperser stages exceed the kernel's capacity".into())
                }
            }
            Self::Tilt { hz, db } => {
                range("hz", hz, 20.0, 20_000.0)?;
                range("db", db, -24.0, 24.0)
            }
            Self::DcBlock => Ok(()),
            Self::Reverb { size, decay, damp } => {
                range("size", size, 0.0, 1.0)?;
                range("decay", decay, 0.0, 1.0)?;
                range("damp", damp, 0.0, 1.0)
            }
        }
    }

    fn state_samples(&self, sr: f32, block: usize) -> usize {
        match *self {
            Self::Delay { ms, .. } => {
                FeedbackDelay::needed_len((sr * ms * 0.001).ceil() as usize + 2)
            }
            Self::Reverb { .. } => Reverb::buffer_len(sr).saturating_add(block),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub id: String,
    pub module: Module,
}

/// Parallel connections sum in document order. Gains are linear and signed;
/// there is no hidden normalization, dry path, or implicit output connection.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Route {
    pub from: String,
    pub to: String,
    pub gain: f32,
}
impl Route {
    pub fn new(from: impl Into<String>, to: impl Into<String>, gain: f32) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            gain,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Patch {
    pub version: u32,
    pub nodes: Vec<Node>,
    pub routes: Vec<Route>,
}
impl Default for Patch {
    fn default() -> Self {
        Self {
            version: VERSION,
            nodes: Vec::new(),
            routes: vec![Route::new(INPUT, OUTPUT, 1.0)],
        }
    }
}

struct Plan {
    order: Vec<usize>,
    incoming: Vec<Vec<(usize, f32)>>,
    output: Vec<(usize, f32)>,
}

impl Patch {
    pub fn resolve_control(&self, node: &str, control: Control) -> Result<(Target, f32), String> {
        let plan = self.plan()?;
        let Some((slot, &index)) = plan
            .order
            .iter()
            .enumerate()
            .find(|(_, i)| self.nodes[**i].id == node)
        else {
            return Err(format!("unknown FX modulation destination {node}"));
        };
        let value = self.nodes[index]
            .module
            .control_value(control)
            .ok_or_else(|| format!("{node} does not expose {control:?} for realtime modulation"))?;
        Ok((Target { slot, control }, value))
    }
    /// Validates the entire draft; the caller replaces its old patch only on
    /// success. Detached nodes are legal for incremental manual construction.
    pub fn validate(&self) -> Result<(), String> {
        self.plan().map(|_| ())
    }

    fn plan(&self) -> Result<Plan, String> {
        if self.version != VERSION {
            return Err(format!("unsupported spectral FX version {}", self.version));
        }
        let mut names = BTreeMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.id.is_empty()
                || node.id == INPUT
                || node.id == OUTPUT
                || !node
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(format!("invalid or reserved module id {:?}", node.id));
            }
            if names.insert(node.id.as_str(), i).is_some() {
                return Err(format!("duplicate module {}", node.id));
            }
            node.module
                .validate()
                .map_err(|e| format!("{}: {e}", node.id))?;
        }
        let mut degree = vec![0usize; self.nodes.len()];
        let mut children = vec![Vec::new(); self.nodes.len()];
        let mut seen = BTreeSet::new();
        for route in &self.routes {
            if !route.gain.is_finite() || !(-32.0..=32.0).contains(&route.gain) {
                return Err("route gain must be finite and in -32..=32".into());
            }
            if route.from == OUTPUT || route.to == INPUT {
                return Err("output is a sink; input is a source".into());
            }
            if route.from != INPUT && !names.contains_key(route.from.as_str()) {
                return Err(format!("unknown source {}", route.from));
            }
            if route.to != OUTPUT && !names.contains_key(route.to.as_str()) {
                return Err(format!("unknown destination {}", route.to));
            }
            if !seen.insert((&route.from, &route.to)) {
                return Err("duplicate route; edit its gain instead".into());
            }
            if let (Some(&from), Some(&to)) =
                (names.get(route.from.as_str()), names.get(route.to.as_str()))
            {
                degree[to] += 1;
                children[from].push(to);
            }
        }
        let mut ready: VecDeque<_> = degree
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (*n == 0).then_some(i))
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(i) = ready.pop_front() {
            order.push(i);
            for &to in &children[i] {
                degree[to] -= 1;
                if degree[to] == 0 {
                    ready.push_back(to);
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err("cyclic routing: graph feedback is not supported yet, even through a delay module; use the delay's internal feedback".into());
        }
        let mut slot = vec![0; self.nodes.len()];
        for (i, &original) in order.iter().enumerate() {
            slot[original] = i + 1;
        }
        let mut incoming = vec![Vec::new(); order.len()];
        let mut output = Vec::new();
        for route in &self.routes {
            let from = names
                .get(route.from.as_str())
                .map(|&i| slot[i])
                .unwrap_or(0);
            if let Some(&to) = names.get(route.to.as_str()) {
                incoming[slot[to] - 1].push((from, route.gain));
            } else {
                output.push((from, route.gain));
            }
        }
        Ok(Plan {
            order,
            incoming,
            output,
        })
    }

    /// Green-only preparation. The budget covers float workspaces and delay/
    /// reverb memory, not allocator overhead or authoring metadata. No fixed
    /// node-count cap. CPU admission/host graph publication are separate work.
    pub fn prepare(
        &self,
        sr: f32,
        block: usize,
        workspace_bytes: usize,
    ) -> Result<Prepared, String> {
        if !sr.is_finite() || !(8000.0..=192_000.0).contains(&sr) || block == 0 {
            return Err("prepare needs 8000..192000 Hz and a nonzero block size".into());
        }
        let plan = self.plan()?;
        let floats = self
            .nodes
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_mul(block))
            .ok_or("workspace size overflow")?;
        let total = self
            .nodes
            .iter()
            .try_fold(floats, |n, node| {
                n.checked_add(node.module.state_samples(sr, block))
            })
            .and_then(|n| n.checked_mul(core::mem::size_of::<f32>()))
            .ok_or("workspace size overflow")?;
        if total > workspace_bytes {
            return Err(format!(
                "patch needs {total} workspace bytes; budget is {workspace_bytes}"
            ));
        }
        let mut kernels = Vec::with_capacity(plan.order.len());
        for &i in &plan.order {
            kernels.push(Kernel::prepare(&self.nodes[i].module, sr, block)?);
        }
        let settings = plan
            .order
            .iter()
            .map(|&i| self.nodes[i].module.clone())
            .collect();
        Ok(Prepared {
            kernels,
            settings,
            sr,
            incoming: plan.incoming,
            output: plan.output,
            buffers: zeros(floats)?,
            block,
            workspace_bytes: total,
            faulted: false,
        })
    }
}

fn zeros(len: usize) -> Result<Vec<f32>, String> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| "could not allocate spectral FX workspace")?;
    values.resize(len, 0.0);
    Ok(values)
}

#[derive(Clone, Debug)]
enum Kernel {
    Gain(f32),
    Filter(filters::Svf, filters::Mode),
    OnePole(filters::OnePole, bool),
    Shape(shaper::Waveshaper),
    Delay(FeedbackDelay, Vec<f32>),
    Disperser(Box<filters::Disperser>),
    Tilt(filters::Tilt),
    DcBlock(filters::DcBlocker),
    Reverb(Box<Reverb>, Vec<f32>, Vec<f32>),
}
impl Kernel {
    fn prepare(module: &Module, sr: f32, block: usize) -> Result<Self, String> {
        Ok(match *module {
            Module::Gain { gain } => Self::Gain(gain),
            Module::Filter { mode, hz, q } => {
                let mut k = filters::Svf::new();
                k.prepare(sr, hz, q);
                Self::Filter(k, mode.kernel())
            }
            Module::OnePole { highpass, hz } => {
                let mut k = filters::OnePole::new();
                k.prepare(sr, hz);
                Self::OnePole(k, highpass)
            }
            Module::Shape {
                mode,
                drive,
                bias,
                mix,
            } => {
                let mut k = shaper::Waveshaper::new();
                k.configure(mode.kernel(), drive, bias, mix);
                Self::Shape(k)
            }
            Module::Delay {
                ms,
                feedback,
                damp_hz,
            } => {
                let max = (sr * ms * 0.001).ceil() as usize + 2;
                let mut k = FeedbackDelay::new();
                k.prepare(sr, max, damp_hz);
                k.set_delay(sr * ms * 0.001);
                k.set_feedback(feedback);
                Self::Delay(k, zeros(FeedbackDelay::needed_len(max))?)
            }
            Module::Disperser { hz, q, stages } => {
                let mut k = Box::new(filters::Disperser::new());
                k.prepare(sr, hz, q, stages);
                Self::Disperser(k)
            }
            Module::Tilt { hz, db } => {
                let mut k = filters::Tilt::new();
                k.prepare(sr, hz, db);
                Self::Tilt(k)
            }
            Module::DcBlock => {
                let mut k = filters::DcBlocker::new();
                k.prepare(sr);
                Self::DcBlock(k)
            }
            Module::Reverb { size, decay, damp } => {
                let mut k = Box::new(Reverb::new());
                let mut state = zeros(Reverb::buffer_len(sr))?;
                k.prepare(sr, &mut state);
                k.set_room(size, decay, damp);
                Self::Reverb(k, state, zeros(block)?)
            }
        })
    }
    fn process(&mut self, io: &mut [f32]) {
        match self {
            Self::Gain(gain) => {
                for sample in io {
                    *sample *= *gain;
                }
            }
            Self::Filter(k, mode) => k.process(io, *mode),
            Self::OnePole(k, true) => k.process_highpass(io),
            Self::OnePole(k, false) => k.process_lowpass(io),
            Self::Shape(k) => k.process(io),
            Self::Delay(k, state) => k.process(io, state),
            Self::Disperser(k) => k.process(io),
            Self::Tilt(k) => k.process(io),
            Self::DcBlock(k) => k.process(io),
            Self::Reverb(k, state, dry) => {
                dry[..io.len()].copy_from_slice(io);
                k.process(&dry[..io.len()], io, state);
            }
        }
    }
    fn reset(&mut self) {
        match self {
            Self::Gain(_) | Self::Shape(_) => {}
            Self::Filter(k, _) => k.reset(),
            Self::OnePole(k, _) => k.reset(),
            Self::Delay(k, state) => {
                k.reset();
                state.fill(0.0);
            }
            Self::Disperser(k) => k.reset(),
            Self::Tilt(k) => k.reset(),
            Self::DcBlock(k) => k.reset(),
            Self::Reverb(k, state, dry) => {
                k.reset(state);
                dry.fill(0.0);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Prepared {
    settings: Vec<Module>,
    sr: f32,
    kernels: Vec<Kernel>,
    incoming: Vec<Vec<(usize, f32)>>,
    output: Vec<(usize, f32)>,
    buffers: Vec<f32>,
    block: usize,
    workspace_bytes: usize,
    faulted: bool,
}
impl Prepared {
    /// Red-zone address resolved at preparation, with no string lookup.
    /// Targets belong to this prepared patch and must be re-resolved on rebuild.
    pub fn set_control(&mut self, target: Target, value: f32) {
        if !value.is_finite() {
            return;
        }
        let (Some(module), Some(kernel)) = (
            self.settings.get_mut(target.slot),
            self.kernels.get_mut(target.slot),
        ) else {
            return;
        };
        match (module, kernel, target.control) {
            (Module::Gain { gain }, Kernel::Gain(k), Control::Gain) => {
                *gain = value.clamp(-32.0, 32.0);
                *k = *gain;
            }
            (
                Module::Filter { hz, q, .. },
                Kernel::Filter(k, _),
                control @ (Control::Hz | Control::Q),
            ) => {
                if control == Control::Hz {
                    *hz = value.clamp(20.0, 20_000.0);
                } else {
                    *q = value.clamp(0.1, 16.0);
                }
                k.prepare(self.sr, *hz, *q);
            }
            (Module::OnePole { hz, .. }, Kernel::OnePole(k, _), Control::Hz) => {
                *hz = value.clamp(20.0, 20_000.0);
                k.prepare(self.sr, *hz);
            }
            (
                Module::Shape {
                    mode,
                    drive,
                    bias,
                    mix,
                },
                Kernel::Shape(k),
                control,
            ) => {
                match control {
                    Control::Drive => *drive = value.clamp(1.0, 32.0),
                    Control::Bias => *bias = value.clamp(-0.9, 0.9),
                    Control::Mix => *mix = value.clamp(0.0, 1.0),
                    _ => return,
                }
                k.configure(mode.kernel(), *drive, *bias, *mix);
            }
            (Module::Delay { feedback, .. }, Kernel::Delay(k, _), Control::Feedback) => {
                *feedback = value.clamp(-0.95, 0.95);
                k.set_feedback(*feedback);
            }
            _ => {}
        }
    }
    pub fn workspace_bytes(&self) -> usize {
        self.workspace_bytes
    }
    pub fn modules(&self) -> usize {
        self.kernels.len()
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn latency(&self) -> usize {
        0
    }
    pub fn reset(&mut self) {
        for k in &mut self.kernels {
            k.reset();
        }
        self.buffers.fill(0.0);
        self.faulted = false;
    }

    /// Red zone: arbitrary host blocks, no allocation or dynamic name lookup.
    /// Invalid/nonfinite signals latch a silent fault until reset. This is NOT
    /// a limiter: ordinary finite gain and parallel summing remain unaltered.
    pub fn process(&mut self, io: &mut [f32]) {
        for chunk in io.chunks_mut(self.block) {
            if self.faulted || chunk.iter().any(|x| !x.is_finite()) {
                self.faulted = true;
                chunk.fill(0.0);
                continue;
            }
            let n = chunk.len();
            self.buffers[..n].copy_from_slice(chunk);
            for (i, kernel) in self.kernels.iter_mut().enumerate() {
                let (previous, rest) = self.buffers.split_at_mut((i + 1) * self.block);
                let current = &mut rest[..n];
                current.fill(0.0);
                for &(from, gain) in &self.incoming[i] {
                    mem::gain_add(
                        &previous[from * self.block..from * self.block + n],
                        current,
                        gain,
                    );
                }
                if current.iter().any(|x| !x.is_finite()) {
                    self.faulted = true;
                    break;
                }
                kernel.process(current);
                if current.iter().any(|x| !x.is_finite()) {
                    self.faulted = true;
                    break;
                }
            }
            chunk.fill(0.0);
            if !self.faulted {
                for &(from, gain) in &self.output {
                    mem::gain_add(
                        &self.buffers[from * self.block..from * self.block + n],
                        chunk,
                        gain,
                    );
                }
                if chunk.iter().any(|x| !x.is_finite()) {
                    self.faulted = true;
                    chunk.fill(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn node(id: &str, module: Module) -> Node {
        Node {
            id: id.into(),
            module,
        }
    }
    #[test]
    fn serial_parallel_fanout_recombination_signed_gains_and_document_order() {
        // Deliberately reverse topology in the document. out = .25 + .5*(2 + 3) = 2.75.
        let patch = Patch {
            version: 1,
            nodes: vec![
                node("mix", Module::Gain { gain: 0.5 }),
                node("b", Module::Gain { gain: 3.0 }),
                node("a", Module::Gain { gain: 2.0 }),
            ],
            routes: vec![
                Route::new(INPUT, "a", 1.0),
                Route::new(INPUT, "b", 1.0),
                Route::new("a", "mix", 1.0),
                Route::new("b", "mix", 1.0),
                Route::new(INPUT, OUTPUT, 0.25),
                Route::new("mix", OUTPUT, 1.0),
            ],
        };
        let mut k = patch.prepare(48_000.0, 17, 1_000_000).unwrap();
        let mut out = [1.0; 129];
        k.process(&mut out);
        assert_eq!(out, [2.75; 129]);
        let mut inverted = patch.clone();
        inverted.routes[3].gain = -1.0;
        let mut k = inverted.prepare(48_000.0, 17, 1_000_000).unwrap();
        out.fill(1.0);
        k.process(&mut out);
        assert_eq!(out, [-0.25; 129]);
    }
    #[test]
    fn rejects_cycles_bad_ports_ids_versions_and_nonfinite_values() {
        let mut patch = Patch::default();
        patch.version = 2;
        assert!(patch.validate().is_err());
        patch.version = 1;
        patch.routes.push(Route::new("missing", OUTPUT, 1.0));
        assert!(patch.validate().is_err());
        patch.routes.pop();
        patch.routes[0].gain = f32::NAN;
        assert!(patch.validate().is_err());
        patch.routes[0].gain = 1.0;
        patch.nodes = vec![
            node("a", Module::Gain { gain: 1.0 }),
            node("b", Module::Gain { gain: 1.0 }),
        ];
        patch.routes = vec![Route::new("a", "b", 1.0), Route::new("b", "a", 1.0)];
        assert!(patch.validate().is_err());
        patch.routes = vec![Route::new(OUTPUT, "a", 1.0)];
        assert!(patch.validate().is_err());
        patch.routes = vec![Route::new("a", INPUT, 1.0)];
        assert!(patch.validate().is_err());
        patch.routes = vec![Route::new("a", "a", 1.0)];
        assert!(patch.validate().is_err());
        patch.routes.clear();
        patch.nodes[1].id = "a".into();
        assert!(patch.validate().is_err());
        patch.nodes[1].id = INPUT.into();
        assert!(patch.validate().is_err());
        patch.nodes[1].id = "b".into();
        patch.nodes[1].module = Module::Gain {
            gain: f32::INFINITY,
        };
        assert!(patch.validate().is_err());
    }
    fn all_modules() -> Vec<Module> {
        vec![
            Module::Gain { gain: 0.7 },
            Module::Filter {
                mode: FilterMode::Lowpass,
                hz: 6000.0,
                q: 0.7,
            },
            Module::OnePole {
                highpass: true,
                hz: 80.0,
            },
            Module::Shape {
                mode: ShapeMode::Fold,
                drive: 2.0,
                bias: 0.0,
                mix: 0.8,
            },
            Module::Delay {
                ms: 5.0,
                feedback: 0.4,
                damp_hz: 7000.0,
            },
            Module::Disperser {
                hz: 900.0,
                q: 0.7,
                stages: 4,
            },
            Module::Tilt {
                hz: 1000.0,
                db: 3.0,
            },
            Module::DcBlock,
            Module::Reverb {
                size: 0.5,
                decay: 0.4,
                damp: 0.6,
            },
        ]
    }
    #[test]
    fn every_adapter_is_split_exact_no_alloc_and_resets_silent() {
        for module in all_modules() {
            let patch = Patch {
                version: 1,
                nodes: vec![node("fx", module.clone())],
                routes: vec![Route::new(INPUT, "fx", 1.0), Route::new("fx", OUTPUT, 1.0)],
            };
            let mut full = patch.prepare(48_000.0, 64, 4_000_000).unwrap();
            let mut split = patch.prepare(48_000.0, 37, 4_000_000).unwrap();
            let mut a: Vec<f32> = (0..8193).map(|i| (i as f32 * 0.17).sin() * 0.1).collect();
            let mut b = a.clone();
            assert_no_alloc::assert_no_alloc(|| {
                full.process(&mut a);
                split.process(&mut []);
                split.process(&mut b[..1]);
                split.process(&mut b[1..129]);
                split.process(&mut b[129..]);
            });
            assert_eq!(a, b, "{} split", module.name());
            assert!(a.iter().all(|x| x.is_finite()));
            full.reset();
            a.fill(0.0);
            full.process(&mut a);
            assert!(a.iter().all(|x| *x == 0.0), "{} reset", module.name());
        }
    }
    #[test]
    fn no_chain_count_cap_budget_is_explicit_and_patch_roundtrips() {
        let mut patch = Patch {
            version: 1,
            nodes: Vec::new(),
            routes: Vec::new(),
        };
        let mut previous = INPUT.to_string();
        for i in 0..1024 {
            let id = format!("g{i}");
            patch.nodes.push(node(&id, Module::Gain { gain: 1.0 }));
            patch.routes.push(Route::new(previous, &id, 1.0));
            previous = id;
        }
        patch.routes.push(Route::new(previous, OUTPUT, 1.0));
        let encoded = ron::to_string(&patch).unwrap();
        assert_eq!(patch, ron::from_str::<Patch>(&encoded).unwrap());
        assert!(patch.prepare(48_000.0, 17, 100).is_err());
        let mut k = patch.prepare(48_000.0, 17, 1_000_000).unwrap();
        assert_eq!(k.modules(), 1024);
        let mut out = [0.125; 257];
        assert_no_alloc::assert_no_alloc(|| k.process(&mut out));
        assert_eq!(out, [0.125; 257]);
    }
    #[test]
    fn identity_silence_and_nonfinite_fault_latch() {
        let mut k = Patch::default().prepare(48_000.0, 16, 1024).unwrap();
        let mut audio = [0.125; 33];
        k.process(&mut audio);
        assert_eq!(audio, [0.125; 33]);
        audio[0] = f32::NAN;
        k.process(&mut audio);
        assert!(k.faulted());
        assert_eq!(audio, [0.0; 33]);
        audio.fill(0.1);
        k.process(&mut audio);
        assert_eq!(audio, [0.0; 33]);
        k.reset();
        audio.fill(0.1);
        k.process(&mut audio);
        assert_eq!(audio, [0.1; 33]);
    }
}
