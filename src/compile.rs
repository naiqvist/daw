//! Turning the arrangement into a graph the engine can run.
//!
//! One direction only: the document goes in, a `GraphSpec` comes out,
//! and nothing here reads back. That is what lets the whole compiler sit
//! apart from the app — and what makes `GraphNodes` worth handing back
//! rather than deriving later, because every swap mints fresh ids and a
//! knob has to know which node belongs to which lane if it is to send a
//! letter instead of forcing a recompile.
use super::*;

/// node that plays track 2. A track with no clips still gets a Seq: a Seq
/// with no events is silent, and keeping the shape stable means a track's
/// letters have somewhere to land the moment it gains a clip.
/// The addressable nodes a graph build hands back, each vec indexed BY
/// TRACK with `None` where that track has no such node.
///
/// This is what makes a knob turn a letter instead of a recompile: a param
/// change has to know which node belongs to which lane, and every swap
/// mints fresh ids, so the mapping is re-captured with the schedule rather
/// than derived later.
#[derive(Debug, Default, Clone)]
pub(crate) struct GraphNodes {
    /// Which readout slot each tapped device was given, by INSTANCE id.
    /// Only devices with something to say about themselves are in here —
    /// a compressor is, an equaliser is not.
    pub(crate) readouts: HashMap<u64, usize>,
    /// Every device instance that reached the schedule, by INSTANCE id —
    /// not by position, which is what lets a letter survive a chain
    /// reorder. A bypassed device, or one on a muted track, is absent.
    pub(crate) devices: HashMap<u64, NodeId>,
    /// Every AUDIO CLIP that reached the schedule, by clip id. What lets
    /// the clip editor's gain ride a letter instead of a recompile.
    pub(crate) audio_clips: HashMap<u64, NodeId>,
    pub(crate) pans: Vec<Option<NodeId>>,
    /// Every send's gain node, `[track][return]`. `None` where the track
    /// did not reach the schedule or the return is muted — the same
    /// meaning `pans` gives it, and the same reason: a letter addressed
    /// to a node that was never compiled has nowhere to go.
    ///
    /// A node PER PAIR, at gain zero as readily as at unity, and that is
    /// the deliberate cost: a send that only existed once it was open
    /// could not be opened by a drag, only by a recompile per frame of
    /// one.
    pub(crate) sends: Vec<Vec<Option<NodeId>>>,
    /// Each return's output stage, in return order.
    pub(crate) returns: Vec<Option<NodeId>>,
    /// The master's output stage — the fader every lane arrives at. One,
    /// not a vec, because there is one master.
    pub(crate) master_out: Option<NodeId>,
}

/// Reduce arbitrarily many sources through bounded-input mixers. Graph
/// compile caps fan-in at `MAX_NODE_INPUTS`; a library-sized arrangement
/// must not become un-compilable merely because one track has many clips.
pub(crate) fn mix_sources(spec: &mut GraphSpec, mut sources: Vec<NodeId>) -> Option<NodeId> {
    while sources.len() > 1 {
        let mut next =
            Vec::with_capacity(sources.len().div_ceil(daw::audio::graph::MAX_NODE_INPUTS));
        for group in sources.chunks(daw::audio::graph::MAX_NODE_INPUTS) {
            if let [only] = group {
                next.push(*only);
                continue;
            }
            let bus = spec.push(NodeSpec::Mixer { gain: 1.0 });
            for source in group {
                spec.connect(*source, bus);
            }
            next.push(bus);
        }
        sources = next;
    }
    sources.pop()
}

/// Wire one chain of effects onto `source`, in signal order, and return
/// what the last of them left behind.
///
/// Shared by every lane and by the master, which is the point: an effect
/// has to mean the same thing wherever it is dropped, and two copies of
/// this match would drift the first time one of them gained a device.
///
/// A bypassed effect leaves the schedule the way a muted track does, and
/// the chain closes over it. Instruments are skipped: at the head of a
/// lane one is already the source, and anywhere else it shapes nothing.
///
/// Delays on a SEND are not wired here. An aux is fed from the OUTPUT
/// stage, which does not exist until the chain has been walked, so they
/// are collected into `auxes` for the caller to hang off its own fader.
#[allow(clippy::too_many_lines)]
/// Which chain a rack edit belongs to.
///
/// The rack draws one chain at a time and does not care whose it is; the
/// app very much does, because a lane is addressed by index and the master
/// is not addressed at all. Naming the two makes every edit path say which
/// it meant instead of passing a `usize` that might be neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChainOwner {
    Track(usize),
    Return(usize),
    Master,
}

/// The master's meter slot, reserved at the top of the range. Track meters
/// are handed out by lane index from the bottom, so the two can only meet
/// in a project with thirty-two lanes — and there the master keeps its
/// reading and the last lane loses one, which is the right way round.
pub(crate) const MASTER_METER: usize = daw::audio::graph::MAX_METERS - 1;

/// How far one level of nesting shifts a lane's name.
///
/// The arrangement's own copy of the session view's constant, because
/// this file may not read that module's private constants — and the two
/// being equal is what makes a stack look the same in both views.
pub(crate) const NEST_INDENT: f32 = 7.0;

pub(crate) fn compile_chain(
    spec: &mut GraphSpec,
    source: NodeId,
    chain: &[DeviceInstance],
    devices: &mut HashMap<u64, NodeId>,
    readouts: &mut HashMap<u64, usize>,
    auxes: &mut Vec<(u64, EchoParams)>,
) -> NodeId {
    let mut tail = source;
    for instance in chain {
        if instance.bypass {
            continue;
        }
        match instance.state {
            // The instrument is already the source, and an instrument
            // anywhere else in a chain shapes nothing.
            DeviceState::SineSynth(_)
            | DeviceState::Poly(_)
            | DeviceState::Loom(_)
            | DeviceState::Tine(_)
            | DeviceState::Scomp(_)
            | DeviceState::Stab(_)
            | DeviceState::Quad(_)
            | DeviceState::Haze(_)
            | DeviceState::Sampler(_)
            | DeviceState::Kick(_)
            | DeviceState::Snare(_)
            | DeviceState::Tom(_)
            | DeviceState::Hat(_)
            | DeviceState::Handclap(_)
            | DeviceState::Acid(_) => {}
            DeviceState::Utility(params) => {
                let node = spec.push(NodeSpec::Utility { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Tone(params) => {
                let node = spec.push(NodeSpec::Tone { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Sigil(params) => {
                let node = spec.push(NodeSpec::Sigil { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Gauge(params) => {
                let node = spec.push(NodeSpec::Gauge { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Umbra(params) => {
                let node = spec.push(NodeSpec::Umbra { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Ferric(params) => {
                let node = spec.push(NodeSpec::Ferric { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Sibyl(params) => {
                let node = spec.push(NodeSpec::Sibyl { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Flint(params) => {
                let node = spec.push(NodeSpec::Flint { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Modulato(params) => {
                let node = spec.push(NodeSpec::Modulato { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Filter(params) => {
                let node = spec.push(NodeSpec::Filter { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Limiter(params) => {
                let node = spec.push(NodeSpec::Limiter { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Reverb(params) => {
                let rev = spec.push(NodeSpec::Reverb {
                    predelay_ms: params.predelay_ms,
                    size: params.size,
                    decay: params.decay,
                    damp: params.damp,
                    low_cut: params.low_cut,
                    diffusion: params.diffusion,
                    modulation: params.modulation,
                    width: params.width,
                    mix: params.mix,
                });
                spec.connect(tail, rev);
                devices.insert(instance.id, rev);
                tail = rev;
            }
            DeviceState::Echo(params) => {
                // Above zero the delay is not in the signal path at
                // all: it hangs off the track's output instead, and
                // the chain closes over it exactly as a bypass does.
                if params.send > 0.0 {
                    auxes.push((instance.id, params));
                    continue;
                }
                let echo = spec.push(NodeSpec::Echo {
                    sync: params.sync.round().max(0.0) as u32,
                    time_ms: params.time_ms,
                    feedback: params.feedback,
                    tone_hz: params.tone_hz,
                    drive: params.drive,
                    wow: params.wow,
                    spread: params.spread,
                    mix: params.mix,
                    send: params.send,
                });
                spec.connect(tail, echo);
                devices.insert(instance.id, echo);
                tail = echo;
            }
            DeviceState::Eq(params) => {
                let eq = spec.push(NodeSpec::Eq { params });
                spec.connect(tail, eq);
                devices.insert(instance.id, eq);
                tail = eq;
            }
            // A RACK IS NOT A NODE. Its children sit beside it in this
            // same flat chain, in the order they run, so building it is
            // building nothing — the container is an idea the UI has
            // about the chain, and the audio path never learns of it.
            DeviceState::Rack => {}
            DeviceState::Resyn(params) => {
                let node = spec.push(NodeSpec::Resyn { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Strip(params) => {
                let node = spec.push(NodeSpec::Strip { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Gate(params) => {
                let node = spec.push(NodeSpec::Gate { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Prism(params) => {
                let node = spec.push(NodeSpec::Prism { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                // Three bands of gain movement ride the same readout
                // the compressors use — see `Readout::bands`.
                if readouts.len() < daw::audio::graph::MAX_METERS {
                    let slot = readouts.len();
                    spec.tap(slot, node);
                    readouts.insert(instance.id, slot);
                }
                tail = node;
            }
            DeviceState::Clamp(params) => {
                let node = spec.push(NodeSpec::Clamp { params });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                // A compressor has something to SAY, and half this card
                // is a display for it. The slot is handed out here, in
                // device order, so a chain that gains a device does not
                // shuffle everyone else's readings.
                if readouts.len() < daw::audio::graph::MAX_METERS {
                    let slot = readouts.len();
                    spec.tap(slot, node);
                    readouts.insert(instance.id, slot);
                }
                tail = node;
            }
            DeviceState::Glue(params) => {
                let glue = spec.push(NodeSpec::Glue { params });
                spec.connect(tail, glue);
                devices.insert(instance.id, glue);
                // A compressor has something to SAY, and its card is
                // most of a display for it. The slot is handed out
                // here, in device order, so a chain that gains a
                // device does not shuffle everyone else's readings.
                if readouts.len() < daw::audio::graph::MAX_METERS {
                    let slot = readouts.len();
                    spec.tap(slot, glue);
                    readouts.insert(instance.id, slot);
                }
                tail = glue;
            }
            DeviceState::Phaser(params) => {
                let node = spec.push(NodeSpec::Phaser {
                    amount: params.amount,
                    centre_hz: params.centre,
                    depth_oct: params.depth,
                    rate_hz: params.rate,
                    mix: params.mix,
                });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Tilt(params) => {
                let node = spec.push(NodeSpec::Tilt {
                    tilt_db: params.tilt,
                    pivot_hz: params.pivot,
                });
                spec.connect(tail, node);
                devices.insert(instance.id, node);
                tail = node;
            }
            DeviceState::Disperser(params) => {
                let disp = spec.push(NodeSpec::Disperser {
                    amount: params.amount,
                    freq_hz: params.freq,
                    pinch: params.pinch,
                });
                spec.connect(tail, disp);
                devices.insert(instance.id, disp);
                tail = disp;
            }
            DeviceState::Sheen(params) => {
                let sheen = spec.push(NodeSpec::Sheen {
                    amount: params.amount,
                    edge_hz: params.edge,
                    mix: params.mix,
                    out: params.out,
                });
                spec.connect(tail, sheen);
                devices.insert(instance.id, sheen);
                tail = sheen;
            }
            DeviceState::Lofi(params) => {
                let lofi = spec.push(NodeSpec::Lofi {
                    rate: params.rate,
                    bits: params.bits,
                    mix: params.mix,
                    out: params.out,
                });
                spec.connect(tail, lofi);
                devices.insert(instance.id, lofi);
                tail = lofi;
            }
            DeviceState::Sat(params) => {
                let sat = spec.push(NodeSpec::Sat {
                    // The one place the index stops being a float:
                    // the spec wants a choice, and rounding is that
                    // single arithmetic step.
                    mode: params.mode.round().max(0.0) as u32,
                    drive: params.drive,
                    bias: params.bias,
                    mix: params.mix,
                    out: params.out,
                });
                spec.connect(tail, sat);
                devices.insert(instance.id, sat);
                tail = sat;
            }
        }
    }
    tail
}

/// Compile a lane's live input into source nodes.
///
/// A mono route is one `Input` node, which the mixers below it will
/// centre. A stereo pair is two, each placed hard to its own side by a
/// `Pan` — mono placement at the extremes is unity on one channel and
/// silence on the other, which is exactly what a pair means.
pub(crate) fn push_input_sources(
    spec: &mut GraphSpec,
    input: TrackInput,
    sources: &mut Vec<NodeId>,
) {
    match input {
        TrackInput::None => {}
        TrackInput::Mono(channel) => {
            sources.push(spec.push(NodeSpec::Input { channel }));
        }
        TrackInput::Stereo(left, right) => {
            for (channel, side) in [(left, -1.0), (right, 1.0)] {
                let node = spec.push(NodeSpec::Input { channel });
                let placed = spec.push(NodeSpec::Pan {
                    pan: side,
                    gain: 1.0,
                });
                spec.connect(node, placed);
                sources.push(placed);
            }
        }
    }
}

/// Send `node` where this lane's output belongs: into a group's bus, or
/// onto the master's own input list.
///
/// One function because a lane's dry output, its aux returns and — once
/// groups nest — a group's own output all have to arrive at the SAME
/// place. Three copies of that decision is three chances for one of them
/// to keep going to the master.
pub(crate) fn land(
    master: &mut Vec<NodeId>,
    group_inputs: &mut [Vec<NodeId>],
    destination: Option<usize>,
    node: NodeId,
) {
    match destination {
        // Collected against the GROUP, not wired straight into its bus,
        // so the fan-in can be capped once the list is complete. A group
        // whose bus was never built — because the group is muted — keeps
        // its list and it is thrown away, which is what a muted group
        // means: the signal goes nowhere, rather than to the master.
        Some(parent) => group_inputs[parent].push(node),
        None => master.push(node),
    }
}

/// One aux echo, from a device's parameters.
///
/// A function because three places build the same node — a lane, a
/// group, the master — and a fourth would have been a copy of the
/// eight-field literal with one field quietly different.
pub(crate) fn echo_node(spec: &mut GraphSpec, params: EchoParams) -> NodeId {
    spec.push(NodeSpec::Echo {
        sync: params.sync.round().max(0.0) as u32,
        time_ms: params.time_ms,
        feedback: params.feedback,
        tone_hz: params.tone_hz,
        drive: params.drive,
        wow: params.wow,
        spread: params.spread,
        mix: params.mix,
        send: params.send,
    })
}

pub(crate) fn build_graph_spec(
    tracks: &[Track],
    bus: &MasterTrack,
    returns: &[ReturnTrack],
    clips: &[Vec<Clip>],
    loop_len_beats: Option<f64>,
    metronome: bool,
) -> (GraphSpec, GraphNodes) {
    let mut spec = GraphSpec::default();
    let mixer = spec.push(NodeSpec::Mixer { gain: 1.0 });
    // Everything that reaches the master: one output stage per audible
    // track, every send return, and the click.
    let mut master: Vec<NodeId> = Vec::new();
    let mut pan_ids: Vec<Option<NodeId>> = vec![None; tracks.len()];
    // The return buses come FIRST, because a send needs somewhere to
    // land before the lane that feeds it is built. A muted return is not
    // built at all — the same rule a muted track gets, and for the same
    // reason: the graph should be as small as what is actually sounding.
    let return_buses: Vec<Option<NodeId>> = returns
        .iter()
        .map(|bus| (!bus.mute).then(|| spec.push(NodeSpec::Mixer { gain: 1.0 })))
        .collect();
    let mut send_ids: Vec<Vec<Option<NodeId>>> = vec![vec![None; returns.len()]; tracks.len()];
    let mut return_ids: Vec<Option<NodeId>> = vec![None; returns.len()];
    // A group's summing bus, and what feeds it. The bus exists as soon
    // as the group lane is reached, which is BEFORE its members — the
    // stack puts a group above what it holds, so by the time a member
    // needs somewhere to land, the somewhere is there.
    let mut group_bus: Vec<Option<NodeId>> = vec![None; tracks.len()];
    // Collected rather than wired as we go, so a group's fan-in can be
    // reduced through the same tree of buses the master's is. A group
    // with more than `MAX_NODE_INPUTS` members is an ordinary drum bus,
    // not an exotic case.
    let mut group_inputs: Vec<Vec<NodeId>> = vec![Vec::new(); tracks.len()];
    let mut devices: HashMap<u64, NodeId> = HashMap::new();
    let mut readouts: HashMap<u64, usize> = HashMap::new();
    let mut audio_clips: HashMap<u64, NodeId> = HashMap::new();
    for (i, track) in tracks.iter().enumerate() {
        if !track_audible(tracks, i) {
            continue;
        }
        // Where this lane's output lands: the bus of the group holding
        // it, or the master. Resolved BEFORE the lane is built, so the
        // aux returns below land in the same place the dry signal does.
        let destination = track::parent_group(tracks, i);
        // A GROUP has no clips and no instrument. Its source is the sum
        // of what is nested under it, which is a node its members wire
        // themselves into after this.
        if track.is_group {
            let bus = spec.push(NodeSpec::Mixer { gain: 1.0 });
            group_bus[i] = Some(bus);
            let mut auxes: Vec<(u64, EchoParams)> = Vec::new();
            let tail = compile_chain(
                &mut spec,
                bus,
                &track.chain,
                &mut devices,
                &mut readouts,
                &mut auxes,
            );
            let pan = spec.push(NodeSpec::Pan {
                pan: track.pan,
                gain: track.volume,
            });
            spec.connect(tail, pan);
            pan_ids[i] = Some(pan);
            land(&mut master, &mut group_inputs, destination, pan);
            for (id, params) in auxes {
                let echo = echo_node(&mut spec, params);
                spec.connect(pan, echo);
                devices.insert(id, echo);
                land(&mut master, &mut group_inputs, destination, echo);
            }
            for (index, bus) in return_buses.iter().enumerate() {
                let Some(bus) = *bus else { continue };
                let level = track
                    .sends
                    .get(index)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let send = spec.push(NodeSpec::Mixer { gain: level });
                spec.connect(pan, send);
                spec.connect(send, bus);
                send_ids[i][index] = Some(send);
            }
            if i < MASTER_METER {
                spec.meter(i, pan);
            }
            continue;
        }
        let mut sources = Vec::new();
        match track.kind {
            TrackKind::Midi => {
                // The instrument heads the chain, so it is the only device
                // that can be the lane's source. Absent or bypassed, the
                // lane makes no sound and compiles to nothing at all.
                let Some(head) = track.instrument().filter(|head| !head.bypass) else {
                    continue;
                };
                let notes = clips
                    .get(i)
                    .map(|clips| seq_notes(clips))
                    .unwrap_or_default();
                // Which instrument heads the chain decides which node the
                // lane's pattern compiles into. The pattern itself is the
                // same either way — same notes, same clip length — which
                // is exactly what `PatternClock` being instrument-agnostic
                // bought.
                let node = match head.state {
                    // An effect at the head is not an instrument; the
                    // lane has nothing to make sound with.
                    DeviceState::Modulato(_)
                    | DeviceState::Flint(_)
                    | DeviceState::Sibyl(_)
                    | DeviceState::Ferric(_)
                    | DeviceState::Umbra(_)
                    | DeviceState::Tone(_)
                    | DeviceState::Sigil(_)
                    | DeviceState::Gauge(_)
                    | DeviceState::Utility(_)
                    | DeviceState::Filter(_)
                    | DeviceState::Lofi(_)
                    | DeviceState::Sheen(_)
                    | DeviceState::Disperser(_)
                    | DeviceState::Tilt(_)
                    | DeviceState::Phaser(_)
                    // A rack at the head is not an instrument either: it
                    // is a container, and what it contains is beside it.
                    | DeviceState::Rack
                    | DeviceState::Gate(_)
                    | DeviceState::Strip(_)
                    | DeviceState::Resyn(_)
                    | DeviceState::Limiter(_) => continue,
                    DeviceState::SineSynth(params) => NodeSpec::Seq {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Tine(params) => NodeSpec::Tine {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Scomp(params) => NodeSpec::Scomp {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Stab(params) => NodeSpec::Stab {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Quad(params) => NodeSpec::Quad {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Poly(params) => NodeSpec::Poly {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Loom(params) => NodeSpec::Loom {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Haze(params) => NodeSpec::Haze {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    // The sampler's FILE and slice table live beside the
                    // chain rather than inside the device's params — see
                    // `Track::sampler_sources` for why — so they are
                    // fetched by instance id here and travel into the
                    // spec together with the knobs.
                    DeviceState::Sampler(params) => {
                        let source = track.sampler_sources.get(&head.id);
                        NodeSpec::Sampler {
                            notes,
                            subloops: Vec::new(),
                            loop_len_beats,
                            path: source.map(|s| s.path.clone()).unwrap_or_default(),
                            params,
                            // The node spec takes slices as FRACTIONS of
                            // the file; this frame's source table is in
                            // frames, and nothing in this frame authors
                            // it, so the compiler leaves the sampler to
                            // the grid its SLICES knob asks for. The
                            // authored table lives on the Song's device.
                            slices: Vec::new(),
                        }
                    }
                    DeviceState::Acid(params) => NodeSpec::Acid {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Kick(params) => NodeSpec::Kick {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Snare(params) => NodeSpec::Snare {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Tom(params) => NodeSpec::Tom {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Hat(params) => NodeSpec::Hat {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    DeviceState::Handclap(params) => NodeSpec::Handclap {
                        notes,
                        subloops: Vec::new(),
                        loop_len_beats,
                        params,
                    },
                    // An effect at the head is not an instrument; the lane
                    // has nothing to make sound with.
                    DeviceState::Reverb(_)
                    | DeviceState::Sat(_)
                    | DeviceState::Echo(_)
                    | DeviceState::Eq(_)
                    | DeviceState::Clamp(_)
                    | DeviceState::Prism(_)
                    | DeviceState::Glue(_) => {
                        continue;
                    }
                };
                let source = spec.push(node);
                devices.insert(head.id, source);
                sources.push(source);
            }
            TrackKind::Audio => {
                // The live input, beside the clips rather than instead of
                // them: an audio lane monitoring a microphone is still an
                // audio lane, and a clip on it still plays.
                //
                // Only when MONITORING. A route with the monitor off is a
                // choice remembered, not a signal — and the default being
                // off is what stops picking an input from putting the
                // speakers into the microphone.
                if track.monitor.hears(track.armed) {
                    push_input_sources(&mut spec, track.input, &mut sources);
                }
                for clip in clips.get(i).into_iter().flatten() {
                    let Some(audio) = &clip.audio else { continue };
                    // A clip whose reversal is still being written has
                    // no file to open yet. It compiles to nothing rather
                    // than to the forward file, because playing it the
                    // right way round for half a second and then
                    // switching is worse than a moment of silence.
                    let Some(path) = audio.playing_path() else {
                        continue;
                    };
                    // The clip's BRACE, in source frames.
                    //
                    // No tempo needed: the clip spans `len` beats and
                    // `source_frames` frames, so a fraction of it in beats
                    // is the same fraction of its region. That also means
                    // the brace survives a tempo change exactly as the
                    // clip does, rather than drifting against it.
                    let (played, loop_from) = match clip_loop(clip) {
                        Some((loop_start, loop_len)) if clip.len > 0.0 => {
                            let per_beat = audio.source_frames as f64 / f64::from(clip.len);
                            let end = ((f64::from(loop_start) + f64::from(loop_len)) * per_beat)
                                .round()
                                .clamp(1.0, audio.source_frames as f64)
                                as u64;
                            let from = (f64::from(loop_start) * per_beat)
                                .round()
                                .clamp(0.0, end.saturating_sub(1) as f64)
                                as u64;
                            (end, from)
                        }
                        // No brace: the whole region, wrapping to its own
                        // start — which is what `looped` always meant.
                        _ => (audio.source_frames, 0),
                    };
                    let node = spec.push(NodeSpec::AudioClip {
                        path,
                        start_beats: f64::from(clip.start),
                        length_beats: Some(f64::from(clip.len)),
                        source_offset_frames: audio.playing_offset(),
                        source_frames: Some(played),
                        loop_clip: audio.looped || clip.loop_on,
                        loop_start_frames: loop_from,
                        gain: audio.gain,
                        fade_in_frames: audio.fade_in,
                        fade_out_frames: audio.fade_out,
                        fade_in_shape: audio.fade_in_curve,
                        fade_out_shape: audio.fade_out_curve,
                        envelope: audio
                            .envelope
                            .iter()
                            .map(|(at, db)| (*at, envelope_gain(*db)))
                            .collect(),
                    });
                    // By CLIP ID, so the editor's gain can reach the node
                    // that is already playing instead of waiting for the
                    // debounced recompile every other clip edit rides.
                    // `Node::AudioClip` ramps its gain, so the letter is
                    // click-free and a drag is heard as it happens.
                    audio_clips.insert(clip.id, node);
                    sources.push(node);
                }
            }
        }
        let Some(source) = mix_sources(&mut spec, sources) else {
            continue;
        };
        // The effects sit between this track's private source bus and pan,
        // each chaining onto the one before; the master never leaks into a
        // track insert.
        //
        // The aux delays come back out rather than being wired here: they
        // hang off the output stage, which does not exist yet.
        let mut auxes: Vec<(u64, EchoParams)> = Vec::new();
        let tail = compile_chain(
            &mut spec,
            source,
            &track.chain,
            &mut devices,
            &mut readouts,
            &mut auxes,
        );
        // Pan is ALWAYS a node, even at dead center, and that is
        // deliberate: it gives every track's pan a permanent address,
        // so turning the header knob is a param letter rather than a
        // schedule swap. A centered constant-power pan is two
        // multiplies; a swap per mouse-move is a recompile per frame.
        let pan = spec.push(NodeSpec::Pan {
            pan: track.pan,
            gain: track.volume,
        });
        spec.connect(tail, pan);
        land(&mut master, &mut group_inputs, destination, pan);
        pan_ids[i] = Some(pan);
        // The sends, tapped POST-FADER off the output stage — the tap
        // every console defaults to and the only one that behaves the
        // way a mixed track should: pull the fader down and the reverb
        // goes with it, instead of a tail hanging on over a track that
        // has left.
        //
        // A gain node per pair, always, even at zero. That is what lets
        // a send be OPENED by a drag: the node has an address, so moving
        // the control is a param letter, and the alternative is a graph
        // swap per frame of the same gesture.
        for (index, bus) in return_buses.iter().enumerate() {
            let Some(bus) = *bus else { continue };
            let level = track
                .sends
                .get(index)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let send = spec.push(NodeSpec::Mixer { gain: level });
            spec.connect(pan, send);
            spec.connect(send, bus);
            send_ids[i][index] = Some(send);
        }
        // The sends, now that there is an output stage to tap. POST-FADER,
        // which is the tap every console defaults to and the only one that
        // behaves the way a mixed track should: pull the fader down and its
        // delay goes with it, instead of the repeats hanging on over a
        // track that has left.
        //
        // The return lands at the MASTER, beside the dry — not back through
        // the pan, which would send it round the fader a second time and,
        // worse, would be silently dropped: `Node::Pan` reads its FIRST
        // input and no other.
        for (id, params) in auxes {
            // The node scales its own tap, so the send costs no node of
            // its own and the parameter keeps ONE address: a knob, an
            // automation lane and a modulation wire all letter the echo
            // at `echo::SEND`, in percent, like every other row.
            let echo = echo_node(&mut spec, params);
            spec.connect(pan, echo);
            devices.insert(id, echo);
            land(&mut master, &mut group_inputs, destination, echo);
        }
        // The track's own output stage is where its meter is read: after
        // the fader and the pan, which is what a mixer meter shows. The
        // slot is the TRACK index, so a muted track — which compiles to
        // nothing at all — leaves its meter reading silence rather than
        // shifting every meter below it up one. The master's slot is
        // reserved above them all, so lanes stop one short of the ceiling.
        if i < MASTER_METER {
            spec.meter(i, pan);
        }
    }
    // --- the group buses -------------------------------------------------
    // Wired here rather than as each member was built, so a group's
    // fan-in is capped exactly as the master's is: past the ceiling the
    // members are reduced through a tree, at or under it they go
    // straight in, and a project with no groups compiles to the graph it
    // did before groups existed.
    for (index, inputs) in std::mem::take(&mut group_inputs).into_iter().enumerate() {
        let Some(bus) = group_bus[index] else {
            continue;
        };
        if inputs.len() > daw::audio::graph::MAX_NODE_INPUTS {
            if let Some(reduced) = mix_sources(&mut spec, inputs) {
                spec.connect(reduced, bus);
            }
        } else {
            for node in inputs {
                spec.connect(node, bus);
            }
        }
    }

    // --- the returns ---------------------------------------------------
    // Each return's own chain, then its fader, then onto the master
    // beside the dry tracks. Compiled AFTER the lanes because that is
    // when every send that feeds it exists; wired to buses that were
    // made before them, because a wire does not care which end was
    // created first.
    //
    // A return is built whether or not anything sends to it. That is the
    // opposite of the rule a lane gets, and it is the right way round: a
    // send is a live gain, so a return that only existed once a send was
    // open could not be opened without a recompile.
    for (index, ret) in returns.iter().enumerate() {
        let Some(bus_node) = return_buses[index] else {
            continue;
        };
        let mut auxes: Vec<(u64, EchoParams)> = Vec::new();
        let mut tail = compile_chain(
            &mut spec,
            bus_node,
            &ret.chain,
            &mut devices,
            &mut readouts,
            &mut auxes,
        );
        // An echo's own aux on a return has nowhere further to go, so it
        // lands beside the dry BEFORE this return's fader — exactly the
        // shape the master gives one.
        if !auxes.is_empty() {
            let mut summed = vec![tail];
            for (id, params) in auxes {
                let echo = spec.push(NodeSpec::Echo {
                    sync: params.sync.round().max(0.0) as u32,
                    time_ms: params.time_ms,
                    feedback: params.feedback,
                    tone_hz: params.tone_hz,
                    drive: params.drive,
                    wow: params.wow,
                    spread: params.spread,
                    mix: params.mix,
                    send: params.send,
                });
                spec.connect(tail, echo);
                devices.insert(id, echo);
                summed.push(echo);
            }
            if let Some(node) = mix_sources(&mut spec, summed) {
                tail = node;
            }
        }
        let out = spec.push(NodeSpec::Pan {
            pan: ret.pan,
            gain: ret.volume,
        });
        spec.connect(tail, out);
        master.push(out);
        return_ids[index] = Some(out);
        // Return meters are handed out DOWNWARDS from just under the
        // master's reserved slot, while lanes are handed out upwards
        // from zero. The two can only meet in a project with thirty-two
        // lanes AND eight returns, and there the return loses its
        // reading rather than stealing a lane's — a lane that stopped
        // metering would look broken, a return that does not is merely
        // quiet.
        let slot = MASTER_METER.saturating_sub(1 + index);
        if slot > tracks.len() {
            spec.meter(slot, out);
        }
    }
    if metronome {
        let click = spec.push(NodeSpec::Click);
        master.push(click);
    }
    // Fan-in at the master is capped like every other node's, and returns
    // push against that ceiling from a second direction. Past the cap the
    // inputs are reduced through a tree of buses; at or under it they are
    // wired straight in, so an ordinary project compiles to exactly the
    // graph it did before sends existed.
    if master.len() > daw::audio::graph::MAX_NODE_INPUTS {
        if let Some(bus) = mix_sources(&mut spec, master) {
            spec.connect(bus, mixer);
        }
    } else {
        for node in master {
            spec.connect(node, mixer);
        }
    }

    // --- the master ------------------------------------------------------
    // The sum feeds the master's own chain, and the master fader is the
    // LAST thing before the speakers. Its effects are compiled by the same
    // function every lane's are, so a compressor means the same thing here
    // as it does on a track.
    let mut master_auxes: Vec<(u64, EchoParams)> = Vec::new();
    let mut tail = compile_chain(
        &mut spec,
        mixer,
        &bus.chain,
        &mut devices,
        &mut readouts,
        &mut master_auxes,
    );
    // A send on the master has nowhere further to go, so its return lands
    // beside the dry on a bus of its own, BEFORE the fader — pulling the
    // master down has to take the repeats with it.
    if !master_auxes.is_empty() {
        let mut returns = vec![tail];
        for (id, params) in master_auxes {
            let echo = spec.push(NodeSpec::Echo {
                sync: params.sync.round().max(0.0) as u32,
                time_ms: params.time_ms,
                feedback: params.feedback,
                tone_hz: params.tone_hz,
                drive: params.drive,
                wow: params.wow,
                spread: params.spread,
                mix: params.mix,
                send: params.send,
            });
            spec.connect(tail, echo);
            devices.insert(id, echo);
            returns.push(echo);
        }
        if let Some(summed) = mix_sources(&mut spec, returns) {
            tail = summed;
        }
    }
    // Always a node, at unity and centre exactly as a track's pan is: it
    // gives the master fader a permanent address, so moving it is a param
    // letter rather than a recompile per mouse-move.
    let master_out = spec.push(NodeSpec::Pan {
        pan: bus.pan,
        gain: bus.volume,
    });
    spec.connect(tail, master_out);
    // The master's meter slot is RESERVED at the top of the range rather
    // than handed out after the tracks: a project with thirty-two lanes
    // must not be the one where the master stops reading.
    spec.meter(MASTER_METER, master_out);
    spec.set_output(master_out);
    (
        spec,
        GraphNodes {
            devices,
            readouts,
            audio_clips,
            pans: pan_ids,
            sends: send_ids,
            returns: return_ids,
            master_out: Some(master_out),
        },
    )
}

/// Every automated parameter's value at `beat`, as engine letters.
///
/// The offline half of what `sync_engine` does live. Live, an envelope is a
/// stream of letters from the UI thread; a render has no UI thread, so the
/// same values have to be handed to `bounce_automated` block by block or
/// the export hears every fader parked at its written-down value.
///
/// Only tracks that ACTUALLY carry an envelope for a target contribute: a
/// letter per block per unautomated fader would be work with no effect,
/// and the compiled-in value is already right.
pub(crate) fn automation_letters(
    tracks: &[Track],
    nodes: &GraphNodes,
    registry: &ParameterRegistry,
    beat: f64,
    out: &mut Vec<daw::audio::graph::ParamChange>,
) {
    let beat = beat as f32;
    for (i, track) in tracks.iter().enumerate() {
        for envelope in &track.automation.envelopes {
            let target = envelope.target.as_str();
            if envelope.points.is_empty() || !target_applies(track, target) {
                continue;
            }
            // Volume and pan live on the track's output stage; sends live
            // on their per-return gain nodes; everything else resolves
            // through the registry to a device node.
            let (node, param, value) = match target {
                TRACK_VOLUME_TARGET | TRACK_PAN_TARGET => {
                    let Some(Some(node)) = nodes.pans.get(i).copied() else {
                        continue;
                    };
                    let volume = target == TRACK_VOLUME_TARGET;
                    let base = if volume { track.volume } else { track.pan };
                    let value = track.automation.value_at(target, beat, base);
                    let (param, value) = if volume {
                        (daw::params::pan::GAIN, value.max(0.0))
                    } else {
                        (daw::params::pan::PAN, value.clamp(-1.0, 1.0))
                    };
                    (node, param, value)
                }
                _ => match target_ref(target) {
                    Some(TargetRef::TrackSend(index)) => {
                        let Some(Some(node)) =
                            nodes.sends.get(i).and_then(|row| row.get(index)).copied()
                        else {
                            continue;
                        };
                        let base = track.sends.get(index).copied().unwrap_or(0.0);
                        let value = track
                            .automation
                            .value_at(target, beat, base)
                            .clamp(0.0, 1.0);
                        (node, daw::params::mixer::GAIN, value)
                    }
                    Some(TargetRef::Device { id, param }) => {
                        let Some(spec) = registry.spec(target) else {
                            continue;
                        };
                        let Some(node) = nodes.devices.get(&id).copied() else {
                            continue;
                        };
                        let base = parameter_base(track, target, spec);
                        let value = track
                            .automation
                            .value_at(target, beat, base)
                            .clamp(spec.min, spec.max);
                        (node, param, value)
                    }
                    Some(TargetRef::TrackOutput(_)) | None => continue,
                },
            };
            out.push(daw::audio::graph::ParamChange {
                node: node.to_bits(),
                param,
                value,
            });
        }
    }
}

/// Resolve the arrangement's modulation into the form the ENGINE runs:
/// every wire's `(track, parameter)` target turned into a concrete node and
/// `ParamChange` id, with the parameter's range and its base value along
/// for the ride.
///
/// This is the only place that knows both halves — the wire's target string
/// and the node ids `build_graph_spec` just minted — so it is where the two
/// meet. The same applicability rules the picker enforces apply here: a
/// wire aimed at a device its track does not carry compiles to nothing
/// rather than lettering some other node.
///
/// Every modulator is carried, wired or not, so telemetry indices line up
/// with the strip's list.
pub(crate) fn build_mod_spec(
    tracks: &[Track],
    modulators: &[Modulator],
    wires: &[ModWire],
    registry: &ParameterRegistry,
    nodes: &GraphNodes,
) -> ModSpec {
    let mut out = Vec::new();
    for wire in wires {
        let Some(track) = tracks.get(wire.track) else {
            continue;
        };
        let target = wire.target.as_str();
        let Some(binding) = target_ref(target) else {
            continue;
        };
        if !target_applies(track, target) {
            continue;
        }
        let Some(spec) = registry.spec(target) else {
            continue;
        };
        // A muted track compiles to no nodes at all, and so does a
        // bypassed device; their wires go with them.
        let (node, param) = match binding {
            TargetRef::TrackOutput(param) => {
                let Some(Some(node)) = nodes.pans.get(wire.track).copied() else {
                    continue;
                };
                (node, param)
            }
            TargetRef::TrackSend(index) => {
                let Some(Some(node)) = nodes
                    .sends
                    .get(wire.track)
                    .and_then(|row| row.get(index))
                    .copied()
                else {
                    continue;
                };
                (node, daw::params::mixer::GAIN)
            }
            TargetRef::Device { id, param } => {
                let Some(node) = nodes.devices.get(&id).copied() else {
                    continue;
                };
                (node, param)
            }
        };
        // A log-scaled target takes its modulation in octaves; the card's
        // mapping is the authority on which parameters those are. Track
        // outputs (pan, volume) stay linear.
        let log = match binding {
            TargetRef::Device { id, param } => track
                .chain
                .iter()
                .find(|d| d.id == id)
                .is_some_and(|d| device_is_log(d.kind(), param)),
            TargetRef::TrackOutput(_) | TargetRef::TrackSend(_) => false,
        };
        out.push(WireSpec {
            id: wire.id,
            source: wire.source,
            node,
            param,
            min: spec.min,
            max: spec.max,
            log,
            // The knob-or-automation value as of this compile. Letters
            // replace it live, so this only has to be right for the first
            // block after a swap — but being wrong for one block is an
            // audible jump, so it is seeded properly.
            base: parameter_base(track, target, spec),
            chain: wire.chain(),
            enabled: wire.enabled,
            solo: wire.solo,
        });
    }
    ModSpec {
        sources: modulators.to_vec(),
        wires: out,
    }
}

/// Should the schedule be rebuilt this frame? Only when what the graph is
/// built FROM genuinely differs from what it was built from, and at most
/// once per `RECOMPILE_MIN_SECS` — so a drag coalesces into one
/// whole-schedule swap.
///
/// Pure, so the debounce is checkable without a clock.
pub(crate) fn recompile_due(dirty: bool, since_last_compile: f64) -> bool {
    dirty && since_last_compile >= RECOMPILE_MIN_SECS
}
