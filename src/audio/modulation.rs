//! Modulation: the sources, the wires, and the compiled plan the audio
//! callback evaluates.
//!
//! The types in the first half are the ones the UI edits and the project
//! file stores. The second half is the RUNTIME side: a [`ModPlan`] compiled
//! green-side and carried inside a `Schedule`, evaluated once per transport
//! segment inside the callback.
//!
//! Why the engine and not the frame loop: a wire evaluated at repaint rate
//! moves in ~16ms steps, stalls whenever a frame is late, and — the part
//! that actually breaks a promise — does not exist at all in an offline
//! bounce, which drives the schedule and never runs UI code. Evaluated
//! here, a synced LFO is a pure function of `ctx.beat`, which is exactly
//! the sequencing contract's timeline-locked classification: a bounce
//! reproduces it sample for sample.
//!
//! Red zone: [`ModPlan::evaluate`], [`ModPlan::base_slot`] and
//! [`ModPlan::note_peaks`] run in the callback. They allocate nothing, take
//! no locks, and loop only over vectors sized at compile.

use crate::audio::graph::{MAX_METERS, NodeId};

// ------------------------------------------------------------- sources ---

/// A modulation source's waveform. Timeline-locked on purpose: an LFO is
/// a pure function of the BEAT, so it renders the same bytes every bounce
/// and freezes honestly when the transport stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModShape {
    Sine,
    Triangle,
    Saw,
    Square,
}

impl ModShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "sin",
            Self::Triangle => "tri",
            Self::Saw => "saw",
            Self::Square => "sqr",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Sine => Self::Triangle,
            Self::Triangle => Self::Saw,
            Self::Saw => Self::Square,
            Self::Square => Self::Sine,
        }
    }

    /// One cycle, phase `0..1`, out `-1..=1`.
    pub fn wave(self, phase: f32) -> f32 {
        let phase = phase.rem_euclid(1.0);
        match self {
            Self::Sine => (phase * std::f32::consts::TAU).sin(),
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::Saw => phase * 2.0 - 1.0,
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// The musical rate ladder an LFO cycles through, in beats per cycle.
pub const MOD_RATES: [f32; 7] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

/// The free-mode rate ladder, in cycles per second.
pub const MOD_HZ: [f32; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

fn default_hz() -> f32 {
    1.0
}

/// What a modulator IS. An LFO is a function of the beat; a follower is a
/// function of another track's live level — which is what makes ducking
/// one track by another a single wire rather than a sidechain feature.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ModKind {
    Lfo {
        shape: ModShape,
        rate_beats: f32,
        /// FREE runs off the transport — it breathes while the transport
        /// stands still. It rides the ENGINE's sample clock, not a wall
        /// clock, so "free" means free of the timeline, not free of
        /// determinism: two bounces of one project still match.
        #[serde(default)]
        free: bool,
        /// The free-mode rate, in cycles per second.
        #[serde(default = "default_hz")]
        hz: f32,
    },
    Follower {
        track: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Modulator {
    pub id: u64,
    pub kind: ModKind,
}

/// One modulation relationship: a source, a (track, parameter) target, and
/// a bipolar depth as a FRACTION of the parameter's range — negative depth
/// is how a follower ducks. Modulation is RELATIVE: it rides on top of the
/// knob-or-automation value and is clamped into the parameter's range at
/// the end.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ModWire {
    /// Identity, for the view state that follows a wire around — scopes,
    /// smoothing memory, the expanded editor. Minted from the same counter
    /// as modulators; a file that lacks ids gets fresh ones on load.
    pub id: u64,
    pub source: u64,
    pub track: usize,
    pub target: String,
    pub depth: f32,
    /// Off is BYPASS, not deletion: the relationship stays on the page,
    /// silent — the A/B every mix decision deserves.
    pub enabled: bool,
    /// While ANY wire is soloed, only soloed wires sound: "what is this
    /// one actually doing" answered by ear.
    pub solo: bool,
    /// The response curve, `-1..=1` — the same bend the automation editor
    /// draws, applied to the source before depth.
    pub curve: f32,
    /// Quantize the response into this many steps. 0 or 1 is off; 2+ turns
    /// a sweep into a ladder — sample-and-hold by shaping.
    pub steps: u32,
    /// One-pole lag toward the target, in milliseconds. 0 is off. Applied
    /// LAST, so even a quantize ladder can glide between its rungs.
    pub smooth_ms: f32,
}

impl ModWire {
    /// The four numbers that shape this wire's response.
    pub fn chain(&self) -> Chain {
        Chain {
            depth: self.depth,
            curve: self.curve,
            steps: self.steps,
            smooth_ms: self.smooth_ms,
        }
    }
}

impl Default for ModWire {
    fn default() -> Self {
        Self {
            id: 0,
            source: 0,
            track: 0,
            target: String::new(),
            depth: 0.0,
            enabled: true,
            solo: false,
            curve: 0.0,
            steps: 0,
            smooth_ms: 0.0,
        }
    }
}

// --------------------------------------------------------------- chain ---

/// The automation editor's bend, shared: `t` in `0..=1`, `bend` in
/// `-1..=1`, out `0..=1`.
pub fn bend_curve(t: f32, bend: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if bend >= 0.0 {
        t.powf(1.0 + bend * 5.0)
    } else {
        1.0 - (1.0 - t).powf(1.0 + -bend * 5.0)
    }
}

/// A wire's transform chain, as the four numbers that define it. Travels
/// as one value everywhere — the engine's plan, the live edit letter, the
/// chain function — so a wire can never be applied half-updated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    pub depth: f32,
    pub curve: f32,
    /// Quantize into this many steps. 0 or 1 is off.
    pub steps: u32,
    /// One-pole lag in milliseconds. 0 is off.
    pub smooth_ms: f32,
}

/// One wire's transform chain — curve, quantize, depth, lag — in the
/// target's own units. Pure over its inputs, so every stage is testable
/// and the engine and the UI cannot disagree about what a wire does.
///
/// `source` is the modulator's raw value (LFOs `-1..=1`, followers
/// `0..=1`), `span` the target's range, `previous` the wire's last output
/// (the smoothing memory), `dt` the elapsed time. `muted` — bypass, or
/// standing down under someone else's solo — aims the chain at ZERO but
/// still runs the smoothing, so switching a wire off is a glide, not a
/// click.
pub fn wire_chain(
    chain: Chain,
    source: f32,
    span: f32,
    previous: Option<f32>,
    dt: f32,
    muted: bool,
) -> f32 {
    let Chain {
        depth,
        curve,
        steps,
        smooth_ms,
    } = chain;
    let target = if muted {
        0.0
    } else {
        // Curve in normalized space, sign restored by the mapping.
        let t = (source.clamp(-1.0, 1.0) + 1.0) * 0.5;
        let mut curved = bend_curve(t, curve);
        if steps > 1 {
            let steps = (steps - 1) as f32;
            curved = (curved * steps).round() / steps;
        }
        (curved * 2.0 - 1.0) * depth * span
    };
    // A wire that computes nonsense contributes NOTHING. `f32::clamp`
    // passes NaN straight through, so the range check downstream is no
    // guard at all — and a NaN reaching the lag below would lodge in its
    // memory and never leave, silencing the parameter for good. Junk gets
    // in from a hand-edited project file (an infinite depth times a zero
    // crossing is NaN), so this is a door, not a theoretical worry.
    let target = if target.is_finite() { target } else { 0.0 };
    if smooth_ms <= 0.0 {
        return target;
    }
    let previous = previous.unwrap_or(target);
    let alpha = crate::dsp::ramps::one_pole_coeff(dt * 1000.0 / smooth_ms.max(1.0));
    let out = previous + (target - previous) * alpha.clamp(0.0, 1.0);
    if out.is_finite() { out } else { target }
}

/// [`wire_chain`] addressed by a whole [`ModWire`] — the UI's spelling.
pub fn wire_contribution(
    wire: &ModWire,
    source: f32,
    span: f32,
    previous: Option<f32>,
    dt: f32,
    muted: bool,
) -> f32 {
    wire_chain(wire.chain(), source, span, previous, dt, muted)
}

/// A modulator's value, given the clocks and the track levels. Pure over
/// its inputs, which is what the determinism tests lean on. `beat` drives
/// synced LFOs, `seconds` drives free ones, and `levels` (one normalized
/// `0..=1` level per track) drives followers.
pub fn modulator_value(kind: &ModKind, beat: f32, seconds: f32, levels: &[f32]) -> f32 {
    match kind {
        ModKind::Lfo {
            shape,
            rate_beats,
            free,
            hz,
        } => {
            if *free {
                shape.wave(seconds * hz.max(1e-3))
            } else {
                shape.wave(beat / rate_beats.max(1e-3))
            }
        }
        ModKind::Follower { track } => levels.get(*track).copied().unwrap_or(0.0),
    }
}

// ------------------------------------------------------------- levels ---

/// The follower's detector floor, in dB. Matches the mixer meter's floor
/// so a follower and the meter the user is watching agree about silence;
/// `follower_level_matches_the_meter_scale` holds the two together.
pub const FOLLOWER_FLOOR_DB: f32 = -60.0;
/// The top of the detector scale, in dB — full scale.
pub const FOLLOWER_CEILING_DB: f32 = 0.0;

/// A block peak (linear amplitude) as a `0..=1` detector level. Silence,
/// −infinity and NaN all answer 0 rather than poisoning the chain.
///
/// This deliberately does NOT call the meter widget's mapping: `src/ui` is
/// green zone and the callback must not reach into it. The two are held
/// equal by test instead of by dependency.
pub fn peak_to_level(peak: f32) -> f32 {
    let a = peak.abs();
    if a.is_nan() || a <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * a.log10();
    if db.is_nan() {
        return 0.0;
    }
    ((db - FOLLOWER_FLOOR_DB) / (FOLLOWER_CEILING_DB - FOLLOWER_FLOOR_DB)).clamp(0.0, 1.0)
}

// ------------------------------------------------------- compiled plan ---

/// How many sources and wires the engine will carry, and therefore how
/// many the telemetry can show. Fixed so the snapshot stays `Copy`.
pub const MAX_MOD_SOURCES: usize = 16;
pub const MAX_MOD_WIRES: usize = 64;

/// One wire, green-side, with its target already resolved from a
/// `(track, parameter)` pair to a concrete node and param id. Built by the
/// graph builder, which is the only place that knows both.
#[derive(Clone, Debug, PartialEq)]
pub struct WireSpec {
    pub id: u64,
    /// The [`Modulator::id`] driving this wire.
    pub source: u64,
    pub node: NodeId,
    pub param: u32,
    /// The parameter's range, for the depth-as-a-fraction mapping and the
    /// final clamp.
    pub min: f32,
    pub max: f32,
    /// Whether the parameter lives on a LOG scale (cutoff, times). A log
    /// target takes its modulation in OCTAVES: depth 1.0 spans the whole
    /// range as a RATIO, symmetric around the base. Applied linearly, a
    /// bipolar source on a cutoff spends half its cycle pinned at the
    /// 20 Hz floor — which the ear reports as the track being GATED, not
    /// filtered. Equal travel must mean equal ratio on a ratio scale.
    pub log: bool,
    /// The knob-or-automation value this wire rides on top of, at compile
    /// time. Letters for this parameter replace it live.
    pub base: f32,
    pub chain: Chain,
    pub enabled: bool,
    pub solo: bool,
}

/// Everything the engine needs to run modulation, before compile resolves
/// node ids to dense indices. Rides along inside a `GraphSpec`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModSpec {
    /// Every modulator, in the arrangement's order — including ones no
    /// wire uses, so telemetry indices line up with the UI's list.
    pub sources: Vec<Modulator>,
    pub wires: Vec<WireSpec>,
}

/// A live edit to one wire's transform chain. The whole chain travels
/// together rather than a field at a time: it makes the message idempotent,
/// it is still only 24 bytes, and it removes any chance of a wire spending
/// a block half-updated.
///
/// Retargeting a wire, adding one, or deleting one is NOT here — those
/// change the plan's SHAPE, and shape changes ride a schedule swap like
/// every other structural edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireEdit {
    pub id: u64,
    pub chain: Chain,
    pub enabled: bool,
    pub solo: bool,
}

/// A letter for the modulation plan, addressed by the id the UI knows.
/// Dragging a depth knob or an LFO's rate must be heard NOW, and a
/// schedule swap is debounced to once a second — far too slow for a knob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModEdit {
    Wire(WireEdit),
    /// A source's whole definition — shape, rate, free, hz — at once.
    Source {
        id: u64,
        kind: ModKind,
    },
}

/// A source as the callback carries it.
#[derive(Clone, Copy, Debug)]
struct PlanSource {
    id: u64,
    kind: ModKind,
    /// Free-mode phase in turns, advanced by elapsed samples. Never reset
    /// on discontinuity: free-running is a classification, not a bug.
    phase: f32,
    value: f32,
}

/// A wire as the callback carries it: indices, not ids.
#[derive(Clone, Copy, Debug)]
struct PlanWire {
    id: u64,
    source: usize,
    target: usize,
    chain: Chain,
    enabled: bool,
    solo: bool,
    /// The lag's memory. `None` on the first block so a wire starts AT its
    /// value rather than gliding up from zero.
    previous: Option<f32>,
    output: f32,
}

/// One modulated parameter: where it lives, what it may range over, the
/// base the user set, and the value the wires landed on this segment.
#[derive(Clone, Copy, Debug)]
struct PlanTarget {
    /// Dense index into the schedule's node vec.
    node: usize,
    param: u32,
    min: f32,
    max: f32,
    /// See [`WireSpec::log`]: sum in octaves, apply as a ratio.
    log: bool,
    /// Set by `ParamChange` letters — modulation rides on top of it.
    base: f32,
    value: f32,
    /// What was last handed to the node. The plan is the SOLE writer of a
    /// modulated parameter (letters divert to `base`), so this is an
    /// accurate mirror, and a value that has not moved need not be
    /// rewritten.
    ///
    /// Worth the four bytes: `Node::apply` is not always cheap — a reverb
    /// re-scales twelve delay lines on a size change, a synth recomputes
    /// envelope coefficients with `powf` — and a wire sitting still, or
    /// bypassed, or standing down under someone else's solo, otherwise
    /// pays that every segment forever.
    ///
    /// Starts NaN so the first segment after a compile always writes: NaN
    /// compares unequal to everything, including itself.
    written: f32,
    /// Whether `value` moved since `written` — set by `evaluate`, read by
    /// `writes`. A flag rather than a comparison in `writes` because
    /// `evaluate` is the one place that can update `written` at the same
    /// time, and doing both in one pass keeps them from disagreeing.
    dirty: bool,
}

/// The compiled modulation plan. Vectors are sized at compile and never
/// resized afterwards; the callback only reads and writes in place.
#[derive(Clone, Debug, Default)]
pub struct ModPlan {
    sources: Vec<PlanSource>,
    wires: Vec<PlanWire>,
    targets: Vec<PlanTarget>,
    /// The previous block's peak per meter slot, normalized. Followers read
    /// this: the current block's peaks do not exist until after the walk,
    /// so a follower is one block (~5ms at 256 frames) behind the signal it
    /// detects — the same latency any sidechain detector has.
    levels: [f32; MAX_METERS],
    sample_rate: f32,
    /// Precomputed: while any wire is soloed, the rest stand down.
    any_solo: bool,
}

impl ModPlan {
    /// GREEN ZONE. Resolve a spec into a runnable plan. `dense` maps a node
    /// id to its index in the schedule's node vec, and answers `None` for a
    /// node that did not survive compilation — whose wires are dropped, the
    /// same way a meter tap on a vanished node reads silence.
    pub fn compile(
        spec: &ModSpec,
        sample_rate: f32,
        dense: impl Fn(NodeId) -> Option<usize>,
    ) -> Self {
        let sources: Vec<PlanSource> = spec
            .sources
            .iter()
            .take(MAX_MOD_SOURCES)
            .map(|m| PlanSource {
                id: m.id,
                kind: m.kind,
                phase: 0.0,
                value: 0.0,
            })
            .collect();

        let mut targets: Vec<PlanTarget> = Vec::new();
        let mut wires: Vec<PlanWire> = Vec::new();
        for wire in spec.wires.iter().take(MAX_MOD_WIRES) {
            let Some(node) = dense(wire.node) else {
                continue;
            };
            // A wire whose source was dropped (past the cap, or deleted
            // without its wires) drives nothing, so it is not carried.
            let Some(source) = spec
                .sources
                .iter()
                .take(MAX_MOD_SOURCES)
                .position(|m| m.id == wire.source)
            else {
                continue;
            };
            // Wires onto one parameter SHARE a target: they sum there, and
            // the sum is clamped once.
            let target = match targets
                .iter()
                .position(|t| t.node == node && t.param == wire.param)
            {
                Some(index) => index,
                None => {
                    // The seed gets the same finite check the letter door
                    // gives every later write. A base is the one value the
                    // clamp downstream cannot rescue, so nothing non-finite
                    // is allowed to become one.
                    let base = if wire.base.is_finite() {
                        wire.base.clamp(wire.min, wire.max)
                    } else {
                        wire.min
                    };
                    targets.push(PlanTarget {
                        node,
                        param: wire.param,
                        min: wire.min,
                        max: wire.max,
                        // Log needs a positive floor to be a ratio scale
                        // at all; a table row that says otherwise falls
                        // back to linear rather than to NaN.
                        log: wire.log && wire.min > 0.0,
                        base,
                        value: base,
                        written: f32::NAN,
                        dirty: true,
                    });
                    targets.len() - 1
                }
            };
            wires.push(PlanWire {
                id: wire.id,
                source,
                target,
                chain: wire.chain,
                enabled: wire.enabled,
                solo: wire.solo,
                previous: None,
                output: 0.0,
            });
        }

        let any_solo = wires.iter().any(|w| w.solo);
        Self {
            sources,
            wires,
            targets,
            levels: [0.0; MAX_METERS],
            sample_rate: if sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            any_solo,
        }
    }

    /// True when no wire reaches a parameter, so no node needs writing.
    /// Sources are still evaluated in this state — see [`Self::evaluate`].
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.sources.is_empty()
    }

    /// Red zone. Carry continuous state over from the plan being retired,
    /// matched BY ID so a reordered or partly-rebuilt plan still lands.
    ///
    /// This exists because all the modulation memory now lives inside the
    /// plan, and a plan is rebuilt on every recompile — which happens for a
    /// clip drag or a tempo nudge, once a second, while audio is playing.
    /// Without this a free LFO's phase snaps to zero and every wire's lag
    /// forgets where it was gliding from, so a slow fader sweep JUMPS to
    /// its target mid-glide. That is a click, and one that only appears
    /// when a recompile happens to coincide with a movement — the kind
    /// nobody can reproduce on purpose.
    ///
    /// Bounded: 16 sources and 64 wires against the same, both capped at
    /// compile. Runs once per swap, not per block.
    pub fn adopt_continuity(&mut self, old: &ModPlan) {
        for source in &mut self.sources {
            if let Some(previous) = old.sources.iter().find(|s| s.id == source.id) {
                source.phase = previous.phase;
                source.value = previous.value;
            }
        }
        for wire in &mut self.wires {
            if let Some(previous) = old.wires.iter().find(|w| w.id == wire.id) {
                wire.previous = previous.previous;
                wire.output = previous.output;
            }
        }
        // Follower detectors would otherwise read silence for one block
        // after every swap, which a ducking wire hears as a gap.
        self.levels = old.levels;
    }

    /// Red zone. The base slot for a parameter, if it is modulated. A
    /// `ParamChange` for a modulated parameter must land HERE rather than
    /// on the node: the node's value is owned by [`Self::evaluate`], and a
    /// letter writing it directly would be overwritten a moment later —
    /// the fader would go dead under a running LFO.
    pub fn base_slot(&mut self, node: usize, param: u32) -> Option<&mut f32> {
        self.targets
            .iter_mut()
            .find(|t| t.node == node && t.param == param)
            .map(|t| &mut t.base)
    }

    /// Red zone. Deliver one modulation letter, by the id the UI knows. An
    /// edit for a wire or source that is not in this plan — deleted, or
    /// past the cap — finds nothing and is binned, the same way a
    /// `ParamChange` for a departed node is.
    pub fn apply_edit(&mut self, edit: ModEdit) {
        match edit {
            ModEdit::Wire(e) => {
                if let Some(wire) = self.wires.iter_mut().find(|w| w.id == e.id) {
                    wire.chain = e.chain;
                    wire.enabled = e.enabled;
                    wire.solo = e.solo;
                    // Solo is global, so one wire's change is every wire's
                    // business — recomputed here rather than per segment.
                    self.any_solo = self.wires.iter().any(|w| w.solo);
                }
            }
            ModEdit::Source { id, kind } => {
                if let Some(source) = self.sources.iter_mut().find(|s| s.id == id) {
                    // The phase is deliberately NOT reset: changing an
                    // LFO's rate mid-flight should bend its motion, not
                    // restart it under the user's hand.
                    source.kind = kind;
                }
            }
        }
    }

    /// Red zone. Take this block's peaks as the follower detector's input,
    /// called before the peaks are cleared for the next block.
    pub fn note_peaks(&mut self, peaks: &[f32; MAX_METERS]) {
        for (level, peak) in self.levels.iter_mut().zip(peaks.iter()) {
            *level = peak_to_level(*peak);
        }
    }

    /// Red zone. Evaluate every source and wire for one transport segment
    /// of `frames` frames at `beat`, and land the results on the targets.
    ///
    /// Synced LFOs read `beat` and nothing else — that is the whole of the
    /// timeline-locked contract, and why a bounce reproduces. Free ones
    /// advance their own phase by elapsed samples, so they run while the
    /// transport is stopped and still land identically in an offline
    /// render.
    /// Sources are evaluated even when NO wire reaches a parameter. An
    /// unwired LFO is the first thing anyone makes, and the strip draws its
    /// card from this telemetry — gating source evaluation on having
    /// targets would freeze that card at zero and make a working LFO look
    /// broken.
    pub fn evaluate(&mut self, beat: f32, frames: usize) {
        if self.sources.is_empty() {
            return;
        }
        let dt = frames as f32 / self.sample_rate;
        let beat = if beat.is_finite() { beat } else { 0.0 };

        for source in &mut self.sources {
            source.value = match source.kind {
                ModKind::Lfo {
                    shape,
                    rate_beats,
                    free,
                    hz,
                } => {
                    if free {
                        let value = shape.wave(source.phase);
                        // Wrapped every step, so the accumulator cannot
                        // drift off into a range where f32 loses turns.
                        source.phase = (source.phase + hz.max(0.0) * dt).rem_euclid(1.0);
                        value
                    } else {
                        shape.wave(beat / rate_beats.max(1e-3))
                    }
                }
                ModKind::Follower { track } => self.levels.get(track).copied().unwrap_or(0.0),
            };
        }

        for target in &mut self.targets {
            target.value = 0.0;
        }
        for wire in &mut self.wires {
            let muted = !wire.enabled || (self.any_solo && !wire.solo);
            // Bounds-checked rather than indexed: compile built both, but
            // the callback is not the place to trust that.
            let Some(source) = self.sources.get(wire.source) else {
                continue;
            };
            let Some(target) = self.targets.get_mut(wire.target) else {
                continue;
            };
            // A log target's span is its range in OCTAVES, so depth is a
            // fraction of the range as a RATIO — the same meaning depth
            // has on a linear span, moved to the scale the ear uses for
            // this parameter.
            let span = if target.log {
                (target.max / target.min).log2()
            } else {
                target.max - target.min
            };
            let output = wire_chain(wire.chain, source.value, span, wire.previous, dt, muted);
            wire.previous = Some(output);
            wire.output = output;
            target.value += output;
        }
        for target in &mut self.targets {
            // Linear rides ON the base; log MULTIPLIES it — the sum of
            // wire outputs is octaves, and `base × 2^octaves` is what
            // "±2 octaves of wobble around the knob" means. Same clamp
            // either way.
            let value = if target.log {
                target.base * target.value.exp2()
            } else {
                target.base + target.value
            };
            // The last door before a node. A non-finite base (from a
            // letter carrying junk) would otherwise walk straight through
            // `clamp`, which returns NaN for NaN.
            let value = if value.is_finite() {
                value.clamp(target.min, target.max)
            } else {
                target.min
            };
            target.value = value;
            // Bookkeeping for `writes` in the same pass, so the mirror and
            // the flag cannot drift apart. `run` always consumes `writes`
            // right after calling this; nothing else may skip it.
            target.dirty = value != target.written;
            target.written = value;
        }
    }

    /// Red zone. What to write, after [`Self::evaluate`]: `(dense node,
    /// param, value)` for each modulated parameter whose value MOVED this
    /// segment. A still wire yields nothing and costs nothing.
    pub fn writes(&self) -> impl Iterator<Item = (usize, u32, f32)> + '_ {
        self.targets
            .iter()
            .filter(|t| t.dirty)
            .map(|t| (t.node, t.param, t.value))
    }

    /// Red zone. Every target's current value, moved or not — what a test
    /// asks when it wants the state rather than the delta.
    pub fn values(&self) -> impl Iterator<Item = (usize, u32, f32)> + '_ {
        self.targets.iter().map(|t| (t.node, t.param, t.value))
    }

    /// Red zone. Publish this segment's source values into a fixed array,
    /// in the arrangement's modulator order.
    pub fn source_values(&self, out: &mut [f32; MAX_MOD_SOURCES]) {
        *out = [0.0; MAX_MOD_SOURCES];
        for (slot, source) in out.iter_mut().zip(self.sources.iter()) {
            *slot = source.value;
        }
    }

    /// Red zone. Publish this segment's wire outputs and the ids they
    /// belong to. Wires whose target did not compile are absent, so the id
    /// is what matches a reading back to a wire — never the index.
    pub fn wire_outputs(&self, ids: &mut [u64; MAX_MOD_WIRES], out: &mut [f32; MAX_MOD_WIRES]) {
        *ids = [0; MAX_MOD_WIRES];
        *out = [0.0; MAX_MOD_WIRES];
        for ((id, value), wire) in ids.iter_mut().zip(out.iter_mut()).zip(self.wires.iter()) {
            *id = wire.id;
            *value = wire.output;
        }
    }

    /// The span of a modulated parameter, by dense node and param — what a
    /// scope normalizes against. Green zone; used by tests and telemetry.
    pub fn target_span(&self, node: usize, param: u32) -> Option<f32> {
        self.targets
            .iter()
            .find(|t| t.node == node && t.param == param)
            .map(|t| t.max - t.min)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    fn lfo(shape: ModShape, rate_beats: f32) -> ModKind {
        ModKind::Lfo {
            shape,
            rate_beats,
            free: false,
            hz: 1.0,
        }
    }

    fn spec_with(kind: ModKind, depth: f32) -> ModSpec {
        ModSpec {
            sources: vec![Modulator { id: 1, kind }],
            wires: vec![WireSpec {
                id: 10,
                source: 1,
                node: NodeId::from_bits(0x0000_0001_0000_0000).unwrap(),
                param: 0,
                min: 0.0,
                max: 2.0,
                log: false,
                base: 1.0,
                chain: Chain {
                    depth,
                    curve: 0.0,
                    steps: 0,
                    smooth_ms: 0.0,
                },
                enabled: true,
                solo: false,
            }],
        }
    }

    fn plan(spec: &ModSpec) -> ModPlan {
        ModPlan::compile(spec, 48_000.0, |_| Some(0))
    }

    /// The engine's chain and the UI's spelling of it are ONE function —
    /// if these ever diverge, a wire sounds different from how it draws.
    #[test]
    fn engine_chain_matches_the_ui_chain() {
        let wire = ModWire {
            depth: 0.5,
            curve: 0.3,
            steps: 5,
            smooth_ms: 20.0,
            ..Default::default()
        };
        for source in [-1.0, -0.4, 0.0, 0.25, 1.0] {
            let ui = wire_contribution(&wire, source, 2.0, Some(0.1), 0.005, false);
            let engine = wire_chain(wire.chain(), source, 2.0, Some(0.1), 0.005, false);
            assert_eq!(ui, engine, "source {source}");
        }
    }

    /// The timeline-locked contract, stated as a test: a synced LFO's value
    /// depends on the beat and NOTHING else — not on how many segments got
    /// there, not on block size. This is what makes a bounce reproduce.
    #[test]
    fn a_synced_lfo_depends_only_on_the_beat() {
        let spec = spec_with(lfo(ModShape::Sine, 2.0), 1.0);

        // One plan walks there in many small segments, another arrives in
        // a single jump.
        let mut walked = plan(&spec);
        for i in 0..64 {
            walked.evaluate(i as f32 * 0.05, 64);
        }
        let mut jumped = plan(&spec);
        jumped.evaluate(63.0 * 0.05, 64);

        let a: Vec<f32> = walked.values().map(|(_, _, v)| v).collect();
        let b: Vec<f32> = jumped.values().map(|(_, _, v)| v).collect();
        assert_eq!(a, b, "a synced LFO must not remember how it got there");
    }

    /// A free LFO is free of the TIMELINE, not of determinism: it advances
    /// with elapsed samples, so a stopped transport still breathes and two
    /// identical renders still match.
    #[test]
    fn a_free_lfo_runs_on_the_sample_clock_and_repeats() {
        let kind = ModKind::Lfo {
            shape: ModShape::Saw,
            rate_beats: 1.0,
            free: true,
            hz: 2.0,
        };
        let spec = spec_with(kind, 1.0);

        let run = || {
            let mut p = plan(&spec);
            let mut seen = Vec::new();
            for _ in 0..32 {
                // Beat stands still: the transport is stopped.
                p.evaluate(0.0, 256);
                seen.push(p.values().next().unwrap().2);
            }
            seen
        };
        let first = run();
        let second = run();
        assert_eq!(
            first, second,
            "a free LFO must still render deterministically"
        );
        assert!(
            first.iter().any(|v| *v != first[0]),
            "a free LFO must move while the transport is stopped"
        );
    }

    /// The pivot of the whole design: a letter sets the BASE, and the next
    /// block's modulation rides on top of it rather than erasing it.
    #[test]
    fn a_letter_moves_the_base_not_the_modulated_value() {
        let spec = spec_with(lfo(ModShape::Square, 1.0), 0.25);
        let mut p = plan(&spec);

        p.evaluate(0.0, 256);
        let before = p.values().next().unwrap().2;

        *p.base_slot(0, 0).unwrap() = 0.5;
        p.evaluate(0.0, 256);
        let after = p.values().next().unwrap().2;

        // Same beat, so the same modulation offset — moved by exactly the
        // distance the fader moved.
        assert!(
            (before - after - 0.5).abs() < 1e-6,
            "base moved 0.5, value moved {}",
            before - after
        );
    }

    #[test]
    fn the_final_value_is_clamped_into_the_parameter_range() {
        // Depth 1.0 on a 0..2 span swings ±2 around a base of 1.0.
        let spec = spec_with(lfo(ModShape::Square, 1.0), 1.0);
        let mut p = plan(&spec);
        for i in 0..16 {
            p.evaluate(i as f32 * 0.25, 256);
            let value = p.values().next().unwrap().2;
            assert!((0.0..=2.0).contains(&value), "escaped the range: {value}");
        }
    }

    /// A wire onto a node that did not survive compilation drives nothing —
    /// and must not land on some other node.
    #[test]
    fn a_wire_onto_a_vanished_node_is_dropped() {
        let spec = spec_with(lfo(ModShape::Sine, 1.0), 1.0);
        let p = ModPlan::compile(&spec, 48_000.0, |_| None);
        // No target, so nothing is written — but the SOURCE survives, so
        // its card still animates in the strip.
        assert_eq!(p.values().count(), 0);
        assert!(!p.is_empty(), "an unwired source is still worth running");
    }

    /// Two wires onto one parameter sum there, and the sum clamps ONCE —
    /// not once per wire, which would quietly swallow the second wire
    /// whenever the first had already reached the rail.
    #[test]
    fn wires_onto_one_parameter_sum_and_clamp_once() {
        // Square wave, so each wire's contribution is exactly ±depth*span
        // and the sum is arithmetic rather than a phase coincidence.
        let one = {
            let spec = spec_with(lfo(ModShape::Square, 1.0), 0.25);
            let mut p = plan(&spec);
            p.evaluate(0.0, 256);
            p.values().next().unwrap().2
        };

        let mut spec = spec_with(lfo(ModShape::Square, 1.0), 0.25);
        let mut second = spec.wires[0].clone();
        second.id = 11;
        spec.wires.push(second);
        let mut p = plan(&spec);
        assert_eq!(p.values().count(), 1, "one parameter, one write");
        p.evaluate(0.0, 256);
        let two = p.values().next().unwrap().2;

        // Base 1.0; one wire moves it by (one - 1.0), two move it twice as
        // far — well inside the 0..2 range, so nothing is clamped away.
        assert!(
            ((two - 1.0) - 2.0 * (one - 1.0)).abs() < 1e-6,
            "one wire gave {one}, two gave {two}"
        );
    }

    /// Solo is global: while any wire is soloed the rest aim at zero.
    #[test]
    fn solo_stands_the_other_wires_down() {
        let mut spec = spec_with(lfo(ModShape::Square, 1.0), 0.5);
        let mut soloed = spec.wires[0].clone();
        soloed.id = 11;
        soloed.solo = true;
        soloed.chain.depth = 0.0;
        spec.wires.push(soloed);

        let mut p = plan(&spec);
        p.evaluate(0.0, 256);
        // The un-soloed wire contributes nothing, so the value is the base.
        assert!((p.values().next().unwrap().2 - 1.0).abs() < 1e-6);
    }

    /// Followers read the previous block's peak, normalized on the same
    /// scale the mixer meter draws.
    #[test]
    fn a_follower_reads_the_metered_level() {
        let spec = spec_with(ModKind::Follower { track: 2 }, 1.0);
        let mut p = plan(&spec);
        let mut peaks = [0.0f32; MAX_METERS];
        peaks[2] = 1.0; // full scale
        p.note_peaks(&peaks);
        p.evaluate(0.0, 256);
        // Full scale is 1.0 on the detector, which the chain maps to the
        // top of the span: base 1.0 + depth 1.0 * span 2.0, clamped to 2.0.
        assert_eq!(p.values().next().unwrap().2, 2.0);
    }

    /// The detector scale is the meter's scale. These are separate code by
    /// design — `src/ui` is green zone and the callback may not reach into
    /// it — so a test is what keeps them from drifting apart.
    #[test]
    fn follower_level_matches_the_meter_scale() {
        for peak in [0.0, 1e-6, 0.001, 0.01, 0.1, 0.5, 1.0, 2.0] {
            let ours = peak_to_level(peak);
            let meter = crate::ui::device::meter::amp_to_norm(peak);
            assert!(
                (ours - meter).abs() < 1e-6,
                "peak {peak}: engine {ours} vs meter {meter}"
            );
        }
    }

    /// A recompile happens for a clip drag or a tempo nudge — once a
    /// second, while audio plays. The plan is rebuilt from scratch, so
    /// without carrying its memory a free LFO's phase snaps to zero and a
    /// wire mid-glide jumps to its target. That is an audible click that
    /// only appears when a swap coincides with a movement.
    #[test]
    fn a_recompile_does_not_restart_a_moving_wire() {
        let kind = ModKind::Lfo {
            shape: ModShape::Saw,
            rate_beats: 1.0,
            free: true,
            hz: 3.0,
        };
        let mut spec = spec_with(kind, 0.5);
        spec.wires[0].chain.smooth_ms = 500.0; // a long, obvious glide

        let mut old = plan(&spec);
        for _ in 0..20 {
            old.evaluate(0.0, 256);
        }
        let moving = old.values().next().unwrap().2;

        // The swap: a fresh plan from the same spec, as compile produces.
        let mut fresh = plan(&spec);
        let mut carried = plan(&spec);
        carried.adopt_continuity(&old);

        fresh.evaluate(0.0, 256);
        carried.evaluate(0.0, 256);
        let jumped = fresh.values().next().unwrap().2;
        let glided = carried.values().next().unwrap().2;

        assert!(
            (glided - moving).abs() < (jumped - moving).abs(),
            "carried state should continue the glide ({moving} -> {glided}), \
             not restart it ({moving} -> {jumped})"
        );
        // And the free LFO kept its phase rather than snapping to zero.
        assert_ne!(
            jumped, glided,
            "a rebuilt plan must not be indistinguishable from a carried one"
        );
    }

    /// Followers detect the PREVIOUS block's peak, so whoever drives the
    /// schedule must hand those over every block. The offline renderer did
    /// not for a while, and every follower in a bounce sat frozen at
    /// zero-level — a ducking wire rendered as no ducking at all.
    ///
    /// This pins the starved behaviour so the gap is visible: a follower
    /// with no peaks contributes NOTHING and the parameter rests at its
    /// base. Silent and plausible, which is exactly why it went unnoticed.
    #[test]
    fn a_starved_follower_contributes_nothing() {
        let spec = spec_with(ModKind::Follower { track: 0 }, 1.0);
        let mut p = plan(&spec);
        p.evaluate(0.0, 256);
        let base = 1.0;
        assert_eq!(p.values().next().unwrap().2, base);

        // Fed, it moves — so the assertion above is about starvation, not
        // about followers being inert.
        let mut peaks = [0.0f32; MAX_METERS];
        peaks[0] = 1.0;
        p.note_peaks(&peaks);
        p.evaluate(0.0, 256);
        assert!(p.values().next().unwrap().2 > base);
    }

    /// An unwired LFO is the first thing anyone makes. Its card animates
    /// from this telemetry, so sources must run even with no targets.
    #[test]
    fn an_unwired_source_still_moves() {
        let spec = ModSpec {
            sources: vec![Modulator {
                id: 1,
                kind: lfo(ModShape::Saw, 1.0),
            }],
            wires: Vec::new(),
        };
        let mut p = plan(&spec);
        let mut seen = Vec::new();
        for i in 0..8 {
            p.evaluate(i as f32 * 0.25, 256);
            let mut out = [0.0; MAX_MOD_SOURCES];
            p.source_values(&mut out);
            seen.push(out[0]);
        }
        assert!(
            seen.iter().any(|v| *v != seen[0]),
            "an unwired LFO must still animate: {seen:?}"
        );
    }

    /// A non-finite base cannot be clamped away downstream — it fails the
    /// finite check every segment and pins the parameter to the bottom of
    /// its range, silently and permanently. Compile must refuse to seed one.
    #[test]
    fn a_junk_base_never_becomes_a_base() {
        let mut spec = spec_with(lfo(ModShape::Sine, 1.0), 0.25);
        spec.wires[0].base = f32::NAN;
        let mut p = plan(&spec);
        p.evaluate(0.0, 256);
        let value = p.values().next().unwrap().2;
        assert!(value.is_finite(), "a junk base must not reach a node");
    }

    /// `Node::apply` is not free — a reverb re-scales twelve delay lines on
    /// a size change, a synth recomputes envelope coefficients with `powf`
    /// — so a parameter that has not moved must not be rewritten every
    /// segment. A bypassed wire, or one standing down under someone else's
    /// solo, is the common case and should cost nothing at all.
    #[test]
    fn a_still_wire_writes_nothing() {
        // A square LFO at a whole-beat rate: within a beat the value is
        // flat, so consecutive segments land on the same number.
        let spec = spec_with(lfo(ModShape::Square, 4.0), 0.25);
        let mut p = plan(&spec);

        p.evaluate(0.0, 256);
        assert_eq!(p.writes().count(), 1, "the first segment must write");
        p.evaluate(0.01, 256);
        assert_eq!(
            p.writes().count(),
            0,
            "an unmoved parameter must not be rewritten"
        );

        // And when it does move, it is written again.
        p.evaluate(2.5, 256); // past the square's edge
        assert_eq!(p.writes().count(), 1, "a moved parameter must be written");

        // A base letter counts as movement, even if the modulation did not.
        p.evaluate(2.51, 256);
        assert_eq!(p.writes().count(), 0);
        *p.base_slot(0, 0).unwrap() = 0.5;
        p.evaluate(2.52, 256);
        assert_eq!(p.writes().count(), 1, "a fader move must reach the node");
    }

    #[test]
    fn evaluate_survives_junk_input() {
        let spec = spec_with(lfo(ModShape::Sine, 0.0), f32::INFINITY);
        let mut p = plan(&spec);
        p.evaluate(f32::NAN, 0);
        let value = p.values().next().unwrap().2;
        assert!(value.is_finite(), "junk in must not put junk on a node");
    }
    /// THE gating fix, pinned. A bipolar LFO on a LOG target (a filter
    /// cutoff) modulates in OCTAVES around the base: symmetric as a
    /// RATIO, never slammed against the floor for half its cycle. The
    /// same wire applied linearly spent half its time hard at 20 Hz —
    /// which the ear reports as the track being gated, not filtered.
    #[test]
    fn a_log_target_wobbles_in_octaves_instead_of_gating() {
        let cutoff_spec = |log: bool| ModSpec {
            sources: vec![Modulator {
                id: 1,
                kind: lfo(ModShape::Sine, 4.0),
            }],
            wires: vec![WireSpec {
                id: 10,
                source: 1,
                node: NodeId::from_bits(0x0000_0001_0000_0000).unwrap(),
                param: 2,
                min: 20.0,
                max: 20_000.0,
                log,
                base: 1_000.0,
                chain: Chain {
                    depth: 0.3,
                    curve: 0.0,
                    steps: 0,
                    smooth_ms: 0.0,
                },
                enabled: true,
                solo: false,
            }],
        };

        let sweep = |log: bool| -> Vec<f32> {
            let mut plan = plan(&cutoff_spec(log));
            (0..64)
                .map(|i| {
                    plan.evaluate(i as f32 / 16.0, 256);
                    plan.values().next().map(|(_, _, v)| v).unwrap_or(0.0)
                })
                .collect()
        };

        // Linear at this depth digs 5,994 Hz below a 1 kHz base: pinned
        // at the floor for a large slice of the cycle. That behaviour is
        // WHY the flag exists — assert it so the contrast stays measured
        // rather than remembered.
        let linear = sweep(false);
        let floored = linear.iter().filter(|v| **v <= 20.0 + 1e-3).count();
        assert!(
            floored > linear.len() / 5,
            "linear no longer pins ({floored} floored) — update this story"
        );

        // Log never touches either wall at the same depth, and it is
        // ratio-symmetric: the peak and the trough multiply back to the
        // base squared.
        let log_sweep = sweep(true);
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in &log_sweep {
            lo = lo.min(*v);
            hi = hi.max(*v);
            assert!(*v > 20.0 && *v < 20_000.0, "log sweep hit a wall at {v}");
        }
        let sym = (lo * hi) / (1_000.0f32 * 1_000.0);
        assert!(
            (0.8..1.25).contains(&sym),
            "octave wobble is not ratio-symmetric: lo {lo} hi {hi}"
        );
        // And the swing is real, not a flatline: ±30 % of ~10 octaves is
        // about ±3 octaves.
        assert!(hi / lo > 30.0, "swing {} too small", hi / lo);
    }
}
