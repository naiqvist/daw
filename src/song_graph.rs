//! The Song, made runnable: the document goes in, a `GraphSpec` comes out.
//!
//! One direction only, and nothing here reads back — the same rule the
//! `daw` binary's own compiler keeps. What is different is the INPUT: that
//! one compiles the legacy arrangement, this one compiles
//! [`crate::sequencing::Song`] itself, which is why it can live in the
//! library where any frame can reach it.
//!
//! # What sounds, and what decides it
//!
//! A scene is "a place to keep clips before it is a thing to fire" — the
//! document does not record which one is playing, because which one is
//! playing is a fact about a PERFORMANCE and not about a song. So the
//! launched scene arrives here as an argument. A frame owns that choice;
//! the document never learns it.
//!
//! # What it does not do yet, stated rather than implied
//!
//! Positional group nesting and trig conditions are not compiled here yet.
//! Parameter locks and automation are; fixed desk buses, returns, channel
//! strips, device chains, clip playback and monitored audio inputs are part
//! of both builders. The session model currently carries pattern clips only,
//! so recorded audio plays from the arrangement rather than a launcher slot.
//!
//! Every one of those is an addition to this file, not a rewrite of it.

use crate::audio::graph::{
    AutomationPoint as GraphAutomationPoint, GraphSpec, MAX_METERS, MAX_NODE_INPUTS, NodeId,
    NodeSpec, Note as GraphNote,
};
use crate::audio::modulation::{ModSpec, WireSpec};
use crate::devices::DeviceKind;
use crate::pitch::nearest_midi;
use crate::sequencing::{
    Clip, DeskPathIdentity, DeskPersonality, Device, DeviceId, PATTERN_STEP_TICKS, PATTERN_STEPS,
    Pattern, Song, TICKS_PER_BEAT, Track,
};

/// The master's meter slot: the LAST one, so the tracks can take theirs in
/// their own order from zero without either end having to know how many
/// the other used. The same convention the `daw` binary's compiler keeps.
pub const MASTER_METER: usize = MAX_METERS - 1;

/// The addressable nodes a build hands back, indexed BY TRACK.
///
/// This is what lets a fader send a letter instead of forcing a recompile:
/// every swap mints fresh ids, so the mapping is captured WITH the schedule
/// rather than derived from the song afterwards. A track that did not reach
/// the graph — silent, or with nothing to play — holds `None`, and a letter
/// addressed to it correctly has nowhere to go.
#[derive(Debug, Clone, PartialEq)]
pub struct SongNodes {
    /// Each track's output stage: the pan node that carries BOTH its fader
    /// and its pan, and the node its meter is tapped from.
    pub outputs: Vec<Option<NodeId>>,
    /// Each track's meter slot in [`crate::audio::BlockSnapshot::track_peaks`].
    pub meters: Vec<Option<usize>>,
    /// The master's output stage, always present — a song with nothing in
    /// it still has somewhere for the meter and the master gain to live.
    pub master: NodeId,
    /// Every REAL device that reached the graph, by id: chain effects,
    /// strip sections, and the desk's own. Exactly one row per id. A knob
    /// turn rides a letter to its node through this table rather than
    /// rebuilding the schedule.
    pub devices: Vec<(DeviceId, NodeId)>,
    /// Machine-generated nodes that answer to a device's parameter letters
    /// without being a second device. The channel OUT's two return taps are
    /// the current case. Kept apart from `devices` so resolving an automation
    /// target by instance id can never depend on whichever duplicate row a
    /// linear search happens to meet first.
    pub param_aliases: Vec<(DeviceId, NodeId)>,
    /// The two concrete post-fader return taps for each track. Stable target
    /// strings (`track.send.a`, `track.send.b`) resolve through this table,
    /// not through OUT's aliases, so the destination is unambiguous.
    pub sends: Vec<[Option<NodeId>; 2]>,
    /// Which telemetry slot each of those reports under, in the order
    /// they were built. The host reads the slot and hands the stage the
    /// device's id, so a card finds its own figures by the device it
    /// draws.
    pub telemetry: Vec<(DeviceId, usize)>,
}

impl SongNodes {
    /// Every node which listens to a device-instance parameter letter.
    ///
    /// Most instances yield one row. OUT additionally yields its generated
    /// send taps, explicitly and after the real device. Static knob sync uses
    /// this iterator; automation uses the typed output/send/device tables
    /// above so one target never relies on alias ordering.
    pub fn device_letter_nodes(&self) -> impl Iterator<Item = (DeviceId, NodeId)> + '_ {
        self.devices.iter().chain(&self.param_aliases).copied()
    }
}

/// Put `node` in the letter table WITHOUT a telemetry slot.
///
/// A voice is addressable — a knob on an instrument must ride a letter
/// like any other knob — but it reports no figures of its own, and the
/// telemetry slots are the console's. See [`register`].
fn address(nodes: &mut SongNodes, id: DeviceId, node: NodeId) {
    nodes.devices.push((id, node));
}

/// Put `node` in the tables: its letters, and its telemetry slot while
/// there is one to give.
fn register(spec: &mut GraphSpec, nodes: &mut SongNodes, id: DeviceId, node: NodeId) {
    nodes.devices.push((id, node));
    let slot = nodes.telemetry.len();
    if slot < crate::audio::graph::MAX_TELEMETRY {
        spec.telemetry(slot, node);
        nodes.telemetry.push((id, slot));
    }
}

/// The meter slots the desk takes: tracks below, then the four buses,
/// the two returns, the mix, and the master last.
pub const TRACK_METERS: usize = 24;
pub const BUS_METER_BASE: usize = 24;
pub const RETURN_METER_BASE: usize = 28;
pub const MIX_METER: usize = 30;

/// The desk as nodes: where a channel's output goes, by bus, and where
/// its sends go.
struct Desk {
    /// The first node of each group bus's rail.
    buses: Vec<NodeId>,
    /// The output and permanent identity of each group bus. Bus-to-bus
    /// coupling is tapped here and joins the master sum without feeding a
    /// neighbour backwards through its nonlinear rail.
    bus_outputs: Vec<(NodeId, DeskPathIdentity)>,
    /// The first node of each return's rail, in the console's order:
    /// TAPE, then SHADOW. A channel taps itself into these.
    aux: Vec<NodeId>,
    /// Every structural rail output that belongs in the master sum. The
    /// reduction tree is sealed only after track and crosstalk feeds exist.
    master_feeds: Vec<NodeId>,
    /// Final output of MIX, after its GLUE, IRON, CEILING, and SCOPE run.
    mix_out: NodeId,
}

#[derive(Clone, Copy)]
struct ChannelRoute {
    out: NodeId,
    /// `None` is the direct-master fallback for a document with no buses.
    bus: Option<usize>,
    identity: DeskPathIdentity,
}

/// One real stereo personality node from the persisted project draw and a
/// permanent model identity. Side keys never depend on graph insertion order.
fn personality_of(personality: DeskPersonality, identity: DeskPathIdentity) -> NodeSpec {
    let (left_identity, right_identity) = identity.stereo_keys();
    NodeSpec::DeskPath {
        project_seed: personality.seed,
        left_identity,
        right_identity,
        noise_enabled: personality.noise_enabled,
    }
}

fn bleed_of(
    personality: DeskPersonality,
    from: DeskPathIdentity,
    to: DeskPathIdentity,
) -> NodeSpec {
    let (from_left, from_right) = from.stereo_keys();
    let (to_left, to_right) = to.stereo_keys();
    NodeSpec::DeskBleed {
        project_seed: personality.seed,
        from_left,
        from_right,
        to_left,
        to_right,
    }
}

/// One rail — a bus, the mix, a return — as nodes: its sections in
/// order (all of them, a rail's sections are never OUT), then its own
/// pan and level. Returns the rail's first node and its output.
fn rail(
    spec: &mut GraphSpec,
    rail: &crate::sequencing::Rail,
    personality: DeskPersonality,
    nodes: &mut SongNodes,
) -> (NodeId, NodeId) {
    let out = spec.push(NodeSpec::Pan {
        pan: rail.pan,
        gain: rail.volume,
    });
    // The personality is the rail's input stage: even an otherwise-idle bus
    // has its own bounded noise floor, and downstream structural processors
    // hear that floor exactly as they hear summed program material.
    let personality = spec.push(personality_of(personality, DeskPathIdentity::Rail(rail.id)));
    let first = personality;
    let mut tail = personality;
    for device in &rail.sections {
        let Some(node) = effect_of(device) else {
            continue;
        };
        let node = spec.push(node);
        register(spec, nodes, device.id, node);
        spec.connect(tail, node);
        tail = node;
    }
    spec.connect(tail, out);
    (first, out)
}

/// The desk: buses and returns sum at the live master fader, then traverse
/// the MIX rail, with every rail metered. The fader must be BEFORE MIX's
/// CEILING: a post-ceiling boost would make the safety stage a decoration.
/// Built before any track, so the tracks have somewhere to go.
fn desk(spec: &mut GraphSpec, song: &Song, nodes: &mut SongNodes, master: NodeId) -> Desk {
    let (mix_in, mix_out) = rail(spec, &song.console.mix, song.desk_personality, nodes);
    spec.connect(master, mix_in);
    spec.meter(MIX_METER, mix_out);
    let mut buses = Vec::with_capacity(song.console.buses.len());
    let mut bus_outputs = Vec::with_capacity(song.console.buses.len());
    let mut master_feeds = Vec::with_capacity(song.console.buses.len() + song.console.aux.len());
    for (index, bus) in song.console.buses.iter().enumerate() {
        let (bus_in, bus_out) = rail(spec, bus, song.desk_personality, nodes);
        if BUS_METER_BASE + index < RETURN_METER_BASE {
            spec.meter(BUS_METER_BASE + index, bus_out);
        }
        buses.push(bus_in);
        bus_outputs.push((bus_out, DeskPathIdentity::Rail(bus.id)));
        master_feeds.push(bus_out);
    }
    let mut aux = Vec::with_capacity(song.console.aux.len());
    for (index, rail_of) in song.console.aux.iter().enumerate() {
        let (aux_in, aux_out) = rail(spec, rail_of, song.desk_personality, nodes);
        if RETURN_METER_BASE + index < MIX_METER {
            spec.meter(RETURN_METER_BASE + index, aux_out);
        }
        aux.push(aux_in);
        master_feeds.push(aux_out);
    }
    Desk {
        buses,
        bus_outputs,
        aux,
        master_feeds,
        mix_out,
    }
}

/// The two parameters of OUT that are the sends, in the console's return
/// order. A return past these has no send to it, which is the honest
/// answer for a desk that has exactly two.
const SEND_PARAMS: [u32; 2] = [
    crate::params::console::out::SEND_TAPE,
    crate::params::console::out::SEND_SHADOW,
];

/// Tap a channel into the returns.
///
/// A send is POST-FADER: `out` is the channel's pan-and-fader stage, so
/// pulling a channel down pulls what it is sending with it, which is
/// what a send on a desk does. The amount lives on the channel's own OUT
/// section, where the hand set it, and the tap node is registered under
/// that section's id — so the letter that carries a turn reaches the tap
/// as well as the section, and moving a send is a letter rather than a
/// recompile. A send at zero still builds its node: it ramps, so opening
/// one is a fade rather than a click.
fn sends(
    spec: &mut GraphSpec,
    track_index: usize,
    track: &Track,
    out: NodeId,
    desk: &Desk,
    aux_sources: &mut [Vec<NodeId>],
    nodes: &mut SongNodes,
) {
    let Some(section) = track
        .strip
        .iter()
        .find(|device| device.kind == DeviceKind::Console(crate::console::SectionKind::Out))
    else {
        return;
    };
    for (index, aux_in) in desk.aux.iter().enumerate() {
        let Some(param) = SEND_PARAMS.get(index).copied() else {
            break;
        };
        let node = spec.push(NodeSpec::Send {
            gain: (section.value(param) * 0.01).clamp(0.0, 1.0),
            param,
        });
        spec.connect(out, node);
        if let Some(sources) = aux_sources.get_mut(index) {
            sources.push(node);
        } else {
            // Defensive fallback for a malformed desk projection. The normal
            // vectors are constructed from this same `desk.aux` length.
            spec.connect(node, *aux_in);
        }
        // The tap answers to the section's letters, and takes no
        // telemetry slot: it has nothing of its own to report.
        nodes.param_aliases.push((section.id, node));
        if let Some(row) = nodes.sends.get_mut(track_index)
            && let Some(slot) = row.get_mut(index)
        {
            *slot = Some(node);
        }
    }
}

/// The bus a track's output lands on: its own, or the last one when the desk
/// has fewer than it names. `None` is a desk with no buses: direct master.
fn bus_of(desk: &Desk, track: &crate::sequencing::Track) -> Option<usize> {
    (!desk.buses.is_empty()).then(|| usize::from(track.bus).min(desk.buses.len() - 1))
}

fn feed_bus(
    bus_sources: &mut [Vec<NodeId>],
    master_sources: &mut Vec<NodeId>,
    bus: Option<usize>,
    source: NodeId,
) {
    if let Some(sources) = bus.and_then(|index| bus_sources.get_mut(index)) {
        sources.push(source);
    } else {
        master_sources.push(source);
    }
}

/// Add one directional, feed-forward coupling in each direction between
/// adjacent active physical channels. It lands in the NEIGHBOUR's bus, so a
/// pair assigned to different groups produces actual cross-bus leakage rather
/// than a decorative gain change on one sum.
fn add_adjacent_crosstalk(
    spec: &mut GraphSpec,
    personality: DeskPersonality,
    routes: &[Option<ChannelRoute>],
    bus_sources: &mut [Vec<NodeId>],
    master_sources: &mut Vec<NodeId>,
) {
    for pair in routes.windows(2) {
        let [Some(left), Some(right)] = pair else {
            continue;
        };
        for (from, to) in [(*left, *right), (*right, *left)] {
            let bleed = spec.push(bleed_of(personality, from.identity, to.identity));
            spec.connect(from.out, bleed);
            feed_bus(bus_sources, master_sources, to.bus, bleed);
        }
    }
}

/// Group rails are adjacent physical paths too. Each directional tap joins
/// the master sum after its source bus, which models output-stage coupling
/// without creating the two-way graph cycle that injecting it back into the
/// neighbour's input would imply.
fn add_adjacent_bus_crosstalk(
    spec: &mut GraphSpec,
    personality: DeskPersonality,
    desk: &Desk,
    master_sources: &mut Vec<NodeId>,
) {
    for pair in desk.bus_outputs.windows(2) {
        let [(left, left_identity), (right, right_identity)] = pair else {
            continue;
        };
        for (from, from_identity, to_identity) in [
            (*left, *left_identity, *right_identity),
            (*right, *right_identity, *left_identity),
        ] {
            let bleed = spec.push(bleed_of(personality, from_identity, to_identity));
            spec.connect(from, bleed);
            master_sources.push(bleed);
        }
    }
}

/// Seal an arbitrary number of channel/send feeds into fixed-fan-in graph
/// trees. A 24-channel song must not become uncompilable merely because one
/// group or return receives more than eight paths.
fn connect_reduced(spec: &mut GraphSpec, sources: &mut Vec<NodeId>, destination: NodeId) {
    if let Some(sum) = mix_track_sources(spec, std::mem::take(sources)) {
        spec.connect(sum, destination);
    }
}

fn seal_desk_feeds(
    spec: &mut GraphSpec,
    desk: &Desk,
    master: NodeId,
    bus_sources: &mut [Vec<NodeId>],
    aux_sources: &mut [Vec<NodeId>],
    master_sources: &mut Vec<NodeId>,
) {
    for (sources, destination) in bus_sources.iter_mut().zip(&desk.buses) {
        connect_reduced(spec, sources, *destination);
    }
    for (sources, destination) in aux_sources.iter_mut().zip(&desk.aux) {
        connect_reduced(spec, sources, *destination);
    }
    connect_reduced(spec, master_sources, master);
}

/// Add the live device-input sources an audio channel is actually monitoring.
///
/// A selected route is only a remembered choice until monitor IN is active,
/// or monitor AUTO meets an armed channel. Stereo routes are placed left and
/// right before they join the channel source bus, exactly like the engine's
/// other compiler; an unavailable hardware channel is silence in `Node::Input`.
fn push_monitored_input(spec: &mut GraphSpec, track: &Track, sources: &mut Vec<NodeId>) {
    if track.is_group
        || track.kind != crate::sequencing::TrackKind::Audio
        || !track.monitor.hears(track.armed)
    {
        return;
    }
    match track.input {
        crate::sequencing::TrackInput::None => {}
        crate::sequencing::TrackInput::Mono(channel) => {
            sources.push(spec.push(NodeSpec::Input { channel }));
        }
        crate::sequencing::TrackInput::Stereo(left, right) => {
            for (channel, pan) in [(left, -1.0), (right, 1.0)] {
                let input = spec.push(NodeSpec::Input { channel });
                let placed = spec.push(NodeSpec::Pan { pan, gain: 1.0 });
                spec.connect(input, placed);
                sources.push(placed);
            }
        }
    }
}

/// Compile `song` into a graph, playing whatever `playing` says.
///
/// `playing[track]` is the SCENE whose clip that track is sounding, and
/// `None` means the track is playing nothing. Per track rather than one
/// scene for the whole song, because that is what a session is: firing a
/// row is firing every clip in it, and a performer who could only ever
/// fire whole rows would be using less than the model already holds.
///
/// Nothing launched anywhere is a legitimate state and not an empty case:
/// it is the transport rolling with nothing fired. The graph still runs;
/// only an explicitly monitored hardware input may sound in that state.
pub fn build(song: &Song, playing: &[Option<usize>]) -> (GraphSpec, SongNodes) {
    let mut spec = GraphSpec::default();
    // The master exists before anything can feed it: a song with nothing
    // in it still needs somewhere for the master gain and its meter to
    // live, and a graph with no output is not a runnable graph.
    let master = spec.push(NodeSpec::Mixer { gain: song.master });
    let mut nodes = SongNodes {
        outputs: vec![None; song.tracks.len()],
        meters: vec![None; song.tracks.len()],
        master,
        devices: Vec::new(),
        param_aliases: Vec::new(),
        sends: vec![[None; 2]; song.tracks.len()],
        telemetry: Vec::new(),
    };
    let desk = desk(&mut spec, song, &mut nodes, master);
    let mut bus_sources: Vec<Vec<NodeId>> = (0..desk.buses.len()).map(|_| Vec::new()).collect();
    let mut aux_sources: Vec<Vec<NodeId>> = (0..desk.aux.len()).map(|_| Vec::new()).collect();
    let mut master_sources = desk.master_feeds.clone();
    let mut channel_routes = vec![None; song.tracks.len()];

    for (index, track) in song.tracks.iter().enumerate() {
        if !song.audible(index) {
            continue;
        }
        let pattern = playing
            .get(index)
            .copied()
            .flatten()
            .and_then(|scene| song.session.scenes.get(scene))
            .and_then(|scene| scene.clip(track.id))
            .and_then(|clip| match clip {
                Clip::Pattern(id) => song.patterns.iter().find(|pattern| pattern.id == id),
            });

        let mut sources = Vec::new();
        let mut voice = None;
        if let Some(pattern) = pattern {
            let notes = notes_of(song, pattern, &[]);
            if !notes.is_empty()
                && let Some(instrument) = voice_of(track, notes, Some(beats(pattern.length_ticks)))
            {
                let node = spec.push(instrument);
                sources.push(node);
                voice = Some(node);
                // The instrument is a device with knobs like any other, so it
                // goes in the letter table. Leaving it out meant a turn on a
                // kick was heard only when something ELSE rebuilt the graph.
                if let Some(head) = track.chain.first().filter(|device| device.is_instrument()) {
                    address(&mut nodes, head.id, node);
                }
            }
        }
        push_monitored_input(&mut spec, track, &mut sources);
        let Some(source) = mix_track_sources(&mut spec, sources) else {
            // No launched notes and no live input being monitored.
            continue;
        };

        // The effects, in signal order, each fed by the one before it. A
        // bypassed effect becomes a dry latency shadow: none of its DSP is
        // instantiated, but bypassing it cannot pull this track earlier than
        // its siblings and turn an A/B comparison into a timing comparison.
        let mut effects: Vec<(DeviceId, NodeId)> = Vec::new();
        let mut tail = source;
        // The chain's effects, then the strip's sections that are IN, in
        // the desk's order: one run of nodes, each fed by the one before.
        for device in track.chain.iter().chain(track.strip.iter()) {
            if device.is_instrument() {
                continue;
            }
            let Some(node) = effect_of(device) else {
                continue;
            };
            if device.bypassed {
                let node = spec.push(NodeSpec::LatencyBypass {
                    effect: Box::new(node),
                });
                spec.connect(tail, node);
                tail = node;
                continue;
            }
            let node = spec.push(node);
            spec.connect(tail, node);
            effects.push((device.id, node));
            register(&mut spec, &mut nodes, device.id, node);
            tail = node;
        }
        // Now the effects have ids, the notes can name them: the voice's
        // notes are cut again with every effect lock addressed, and every
        // locked effect parameter registers the knob it returns to.
        if let (Some(voice), Some(pattern)) = (voice, pattern)
            && !effects.is_empty()
        {
            if let Some(notes) = spec.node_mut(voice).and_then(NodeSpec::notes_mut) {
                *notes = notes_of(song, pattern, &effects);
            }
            for step in 0..PATTERN_STEPS {
                for lock in &pattern.trig(step).locks {
                    let Some(id) = lock.device else {
                        continue;
                    };
                    if let Some((_, node)) = effects.iter().find(|(device, _)| *device == id)
                        && let Some(device) = track
                            .chain
                            .iter()
                            .chain(track.strip.iter())
                            .find(|device| device.id == id)
                    {
                        spec.lock_base(*node, lock.param, device.value(lock.param));
                    }
                }
            }
        }
        // The permanent path personality belongs to the channel strip, after
        // every insert and before the one post-fader/pan output stage. This
        // keeps established source -> insert routing intact while making the
        // path audible to the bus, sends, and meter alike.
        let personality = spec.push(personality_of(
            song.desk_personality,
            DeskPathIdentity::Track(track.id),
        ));
        spec.connect(tail, personality);
        tail = personality;
        // The fader and the pan are ONE node, which is why the meter tapped
        // from it reads post-fader and post-pan — what a mixer meter is
        // expected to show.
        let out = spec.push(NodeSpec::Pan {
            pan: track.pan,
            gain: track.volume,
        });
        spec.connect(tail, out);
        // Collect desk feeds first; a reduction tree seals them after every
        // channel exists, keeping the graph's fixed fan-in invariant.
        let bus = bus_of(&desk, track);
        feed_bus(&mut bus_sources, &mut master_sources, bus, out);
        channel_routes[index] = Some(ChannelRoute {
            out,
            bus,
            identity: DeskPathIdentity::Track(track.id),
        });
        sends(
            &mut spec,
            index,
            track,
            out,
            &desk,
            &mut aux_sources,
            &mut nodes,
        );
        nodes.outputs[index] = Some(out);

        if index < TRACK_METERS {
            spec.meter(index, out);
            nodes.meters[index] = Some(index);
        }
    }

    add_adjacent_crosstalk(
        &mut spec,
        song.desk_personality,
        &channel_routes,
        &mut bus_sources,
        &mut master_sources,
    );
    add_adjacent_bus_crosstalk(&mut spec, song.desk_personality, &desk, &mut master_sources);
    seal_desk_feeds(
        &mut spec,
        &desk,
        master,
        &mut bus_sources,
        &mut aux_sources,
        &mut master_sources,
    );
    // The master fader remains the live-addressable `SongNodes::master`, but
    // the heard and metered master is AFTER MIX's safety processing.
    install_automation(&mut spec, song, &nodes);
    install_modulation(&mut spec, song, &nodes);
    spec.meter(MASTER_METER, desk.mix_out);
    spec.set_output(desk.mix_out);
    (spec, nodes)
}

/// The instrument a track sounds, ready to push.
///
/// The head of its chain when there is one, and the default voice when
/// there is not — which is what every project written before devices
/// reached the Song carries, and what it has always sounded.
///
/// `None` means this track produces nothing: its instrument is bypassed,
/// or it is a kind this compiler cannot build yet. Silence is the honest
/// answer to both. Quietly substituting a different instrument would be a
/// track sounding something nobody chose.
///
/// The sampler is the one instrument whose patch is not only numbers: it
/// plays the file the device carries in `Device::sample`, and a sampler
/// with no file yet compiles to a SILENT sampler rather than a refused
/// graph — the engine's own rule, because a missing sample must not mute
/// a project.
///
/// **Not built yet:** `SineSynth`. An addition here.
fn voice_of(
    track: &crate::sequencing::Track,
    notes: Vec<GraphNote>,
    loop_len_beats: Option<f64>,
) -> Option<NodeSpec> {
    // Every instrument node has the same shape — the pattern's notes and
    // its own typed patch — so the arms differ only in which two names
    // they name. Written as one shape so a new instrument is one line and
    // cannot quietly acquire a different contract.
    macro_rules! voice {
        ($variant:ident, $params:path, $device:expr) => {{
            let mut params = <$params>::default();
            for (id, value) in &$device.overrides {
                params.set(*id, *value);
            }
            NodeSpec::$variant {
                notes,
                subloops: Vec::new(),
                loop_len_beats,
                params,
            }
        }};
    }

    let Some(head) = track.chain.first().filter(|device| device.is_instrument()) else {
        // No instrument on the chain: the default voice, at its defaults.
        return Some(NodeSpec::Poly {
            notes,
            subloops: Vec::new(),
            loop_len_beats,
            params: Default::default(),
        });
    };
    if head.bypassed {
        return None;
    }
    Some(match head.kind {
        DeviceKind::Poly => voice!(Poly, crate::audio::poly::PolyParams, head),
        DeviceKind::Haze => voice!(Haze, crate::audio::haze::HazeParams, head),
        DeviceKind::Loom => voice!(Loom, crate::audio::loom::LoomParams, head),
        DeviceKind::Tine => voice!(Tine, crate::audio::tine::TineParams, head),
        DeviceKind::Scomp => voice!(Scomp, crate::scomp::ScompParams, head),
        DeviceKind::Stab => voice!(Stab, crate::audio::stab::StabParams, head),
        DeviceKind::Acid => voice!(Acid, crate::audio::acid::AcidParams, head),
        DeviceKind::Kick => voice!(Kick, crate::audio::kick::KickParams, head),
        DeviceKind::Snare => voice!(Snare, crate::audio::snare::SnareParams, head),
        DeviceKind::Hat => voice!(Hat, crate::audio::hat::HatParams, head),
        DeviceKind::Tom => voice!(Tom, crate::audio::tom::TomParams, head),
        DeviceKind::Handclap => voice!(Handclap, crate::audio::handclap::HandclapParams, head),
        DeviceKind::Sampler => {
            let mut params = crate::audio::sampler::SamplerParams::default();
            for (id, value) in &head.overrides {
                params.set(*id, *value);
            }
            NodeSpec::Sampler {
                notes,
                subloops: Vec::new(),
                loop_len_beats,
                path: head.sample.clone().unwrap_or_default(),
                params,
                // The device's authored slices, as fractions; an empty
                // table leaves the compiler to lay the grid the SLICES
                // knob asks for.
                slices: head.slices.clone(),
            }
        }
        _ => return None,
    })
}

/// One effect, ready to push, or `None` for a kind this compiler cannot
/// build.
///
/// Two shapes, and they are the engine's shapes rather than a choice made
/// here: most effects take their whole patch as one typed struct with an
/// id-addressed setter — the same shape every instrument has — and eight
/// take their settings as named fields. The field order of those eight is
/// their parameter table's order, which is checked by the fact that this
/// file names each id explicitly rather than counting positions.
///
/// `Rack` is the one absent kind: it is a container for other devices,
/// not a node, and nesting is not a thing the Song's chain models yet.
fn effect_of(device: &Device) -> Option<NodeSpec> {
    // Every struct-shaped effect is built the same way: the kind's own
    // defaults, then the device's edits applied by id. An id the table
    // does not know is dropped by the setter, exactly as a stale letter is.
    macro_rules! patch {
        ($variant:ident, $params:path) => {{
            let mut params = <$params>::default();
            for (id, edit) in &device.overrides {
                params.set(*id, *edit);
            }
            NodeSpec::$variant { params }
        }};
    }
    let value = |id: u32| device.value(id);
    // The one place an index stops being a float: the spec wants a
    // choice, and rounding is that single arithmetic step.
    let choice = |id: u32| device.value(id).round().max(0.0) as u32;

    Some(match device.kind {
        // A section of the console: the kind and the device's edits, as
        // they are — the core clamps them again on the way in.
        DeviceKind::Console(kind) => NodeSpec::Section {
            params: crate::console::SectionParams {
                kind,
                values: device.overrides.clone(),
            },
        },
        DeviceKind::Utility => patch!(Utility, crate::audio::utility::UtilityParams),
        DeviceKind::Modulato => patch!(Modulato, crate::audio::modulato::ModulatoParams),
        DeviceKind::Filter => patch!(Filter, crate::audio::filter::FilterParams),
        DeviceKind::Limiter => patch!(Limiter, crate::audio::limiter::LimiterParams),
        DeviceKind::Clamp => patch!(Clamp, crate::audio::clamp::ClampParams),
        DeviceKind::Prism => patch!(Prism, crate::audio::prism::PrismParams),
        DeviceKind::Glue => patch!(Glue, crate::audio::glue::GlueParams),
        DeviceKind::Eq => patch!(Eq, crate::audio::eq::EqParams),
        DeviceKind::Flint => patch!(Flint, crate::audio::flint::FlintParams),
        DeviceKind::Sibyl => patch!(Sibyl, crate::audio::sibyl::SibylParams),
        DeviceKind::Ferric => patch!(Ferric, crate::audio::ferric::FerricParams),
        DeviceKind::Umbra => patch!(Umbra, crate::audio::umbra::UmbraParams),
        DeviceKind::Tone => patch!(Tone, crate::audio::tone::ToneParams),
        DeviceKind::Sigil => patch!(Sigil, crate::audio::sigil::SigilParams),
        DeviceKind::Gauge => patch!(Gauge, crate::audio::gauge::GaugeParams),
        DeviceKind::Gate => patch!(Gate, crate::audio::gate::GateParams),
        DeviceKind::Strip => patch!(Strip, crate::audio::strip::StripParams),
        DeviceKind::Resyn => patch!(Resyn, crate::audio::resyn::ResynParams),
        DeviceKind::Reverb => NodeSpec::Reverb {
            predelay_ms: value(crate::params::reverb::PREDELAY),
            size: value(crate::params::reverb::SIZE),
            decay: value(crate::params::reverb::DECAY),
            damp: value(crate::params::reverb::DAMP),
            low_cut: value(crate::params::reverb::LOWCUT),
            diffusion: value(crate::params::reverb::DIFFUSION),
            modulation: value(crate::params::reverb::MODULATION),
            width: value(crate::params::reverb::WIDTH),
            mix: value(crate::params::reverb::MIX),
        },
        DeviceKind::Echo => NodeSpec::Echo {
            sync: choice(crate::params::echo::SYNC),
            time_ms: value(crate::params::echo::TIME),
            feedback: value(crate::params::echo::FEEDBACK),
            tone_hz: value(crate::params::echo::TONE),
            drive: value(crate::params::echo::DRIVE),
            wow: value(crate::params::echo::WOW),
            spread: value(crate::params::echo::SPREAD),
            mix: value(crate::params::echo::MIX),
            send: value(crate::params::echo::SEND),
        },
        DeviceKind::Sat => NodeSpec::Sat {
            mode: choice(crate::params::sat::MODE),
            drive: value(crate::params::sat::DRIVE),
            bias: value(crate::params::sat::BIAS),
            mix: value(crate::params::sat::MIX),
            out: value(crate::params::sat::OUT),
        },
        DeviceKind::Lofi => NodeSpec::Lofi {
            rate: value(crate::params::lofi::RATE),
            bits: value(crate::params::lofi::BITS),
            mix: value(crate::params::lofi::MIX),
            out: value(crate::params::lofi::OUT),
        },
        DeviceKind::Sheen => NodeSpec::Sheen {
            amount: value(crate::params::sheen::AMOUNT),
            edge_hz: value(crate::params::sheen::EDGE),
            mix: value(crate::params::sheen::MIX),
            out: value(crate::params::sheen::OUT),
        },
        DeviceKind::Disperser => NodeSpec::Disperser {
            amount: value(crate::params::disperser::AMOUNT),
            freq_hz: value(crate::params::disperser::FREQ),
            pinch: value(crate::params::disperser::PINCH),
        },
        DeviceKind::Tilt => NodeSpec::Tilt {
            tilt_db: value(crate::params::tilt::TILT),
            pivot_hz: value(crate::params::tilt::PIVOT),
        },
        DeviceKind::Phaser => NodeSpec::Phaser {
            amount: value(crate::params::phaser::AMOUNT),
            centre_hz: value(crate::params::phaser::CENTRE),
            depth_oct: value(crate::params::phaser::DEPTH),
            rate_hz: value(crate::params::phaser::RATE),
            mix: value(crate::params::phaser::MIX),
        },
        _ => return None,
    })
}

/// Ticks in the Song's own resolution, as beats — the unit the graph
/// speaks. Tempo is applied when the spec is compiled, never here.
fn beats(ticks: usize) -> f64 {
    ticks as f64 / TICKS_PER_BEAT as f64
}

/// Reduce a track's voices and audio clips to one bounded-input source.
///
/// `Node::Pan` consumes one input, so wiring several clips straight into it
/// does not mix them. A reduction tree also keeps a track with a library-sized
/// number of placements below the graph's fixed fan-in ceiling. This is all
/// green-side graph construction; the callback sees only the compiled mixers.
fn mix_track_sources(spec: &mut GraphSpec, mut sources: Vec<NodeId>) -> Option<NodeId> {
    while sources.len() > 1 {
        let mut next = Vec::with_capacity(sources.len().div_ceil(MAX_NODE_INPUTS));
        for group in sources.chunks(MAX_NODE_INPUTS) {
            if let [only] = group {
                next.push(*only);
                continue;
            }
            let sum = spec.push(NodeSpec::Mixer { gain: 1.0 });
            for source in group {
                spec.connect(*source, sum);
            }
            next.push(sum);
        }
        sources = next;
    }
    sources.pop()
}

/// An authored clip-envelope point, from the model's dB into the node's
/// linear gain. These are the same bounds the clip editor enforces. Keeping
/// the conversion here means the callback only multiplies already-compiled
/// values, as required by the sequencing contract.
fn audio_envelope_gain(db: f32) -> f32 {
    const FLOOR_DB: f32 = -60.0;
    const CEIL_DB: f32 = 6.0;
    if !db.is_finite() || db <= FLOOR_DB {
        0.0
    } else {
        crate::dsp::arith::db_to_gain(db.min(CEIL_DB))
    }
}

/// The SONG: every track's blocks laid on the timeline, as the graph
/// plays them. The session's `build` plays one scene and loops it; this
/// plays the arrangement once from the top, so no node loops and every
/// note is stamped in song time.
///
/// A pattern block plays its pattern from the block's start, repeated
/// to fill the block and cut at its end. An audio block streams its
/// file from the block's start. Effects, pan, and the master are the
/// track's as in the session; a track with neither placed content nor a
/// monitored live input is not built at all.
pub fn build_song(song: &Song) -> (GraphSpec, SongNodes) {
    let mut spec = GraphSpec::default();
    let master = spec.push(NodeSpec::Mixer { gain: song.master });
    let mut nodes = SongNodes {
        outputs: vec![None; song.tracks.len()],
        meters: vec![None; song.tracks.len()],
        master,
        devices: Vec::new(),
        param_aliases: Vec::new(),
        sends: vec![[None; 2]; song.tracks.len()],
        telemetry: Vec::new(),
    };
    let desk = desk(&mut spec, song, &mut nodes, master);
    let mut bus_sources: Vec<Vec<NodeId>> = (0..desk.buses.len()).map(|_| Vec::new()).collect();
    let mut aux_sources: Vec<Vec<NodeId>> = (0..desk.aux.len()).map(|_| Vec::new()).collect();
    let mut master_sources = desk.master_feeds.clone();
    let mut channel_routes = vec![None; song.tracks.len()];

    for (index, track) in song.tracks.iter().enumerate() {
        if !song.audible(index) {
            continue;
        }
        let notes = notes_of_blocks(song, track, &[]);
        let mut sources = Vec::new();
        let voice = if notes.is_empty() {
            None
        } else {
            voice_of(track, notes, None).map(|instrument| {
                let voice = spec.push(instrument);
                sources.push(voice);
                voice
            })
        };
        if let Some(voice) = voice {
            // The instrument is a device with knobs like any other, so it
            // goes in the letter table. Leaving it out meant a turn on a
            // kick was heard only when something ELSE rebuilt the graph.
            if let Some(head) = track.chain.first().filter(|device| device.is_instrument()) {
                address(&mut nodes, head.id, voice);
            }
        }
        push_monitored_input(&mut spec, track, &mut sources);
        for clip in audio_blocks_of(song, track) {
            sources.push(spec.push(clip));
        }
        let Some(source) = mix_track_sources(&mut spec, sources) else {
            // This includes a reversed clip whose cache render is not ready:
            // silence is honest; opening its forward file would play it wrong.
            continue;
        };

        // Pattern voices and recorded clips share the track's insert chain
        // and complete channel strip. They are sources of one channel, not a
        // dry side-door around its processing.
        let mut effects: Vec<(DeviceId, NodeId)> = Vec::new();
        let mut tail = source;
        for device in track.chain.iter().chain(track.strip.iter()) {
            if device.is_instrument() {
                continue;
            }
            let Some(node) = effect_of(device) else {
                continue;
            };
            if device.bypassed {
                let node = spec.push(NodeSpec::LatencyBypass {
                    effect: Box::new(node),
                });
                spec.connect(tail, node);
                tail = node;
                continue;
            }
            let node = spec.push(node);
            spec.connect(tail, node);
            effects.push((device.id, node));
            register(&mut spec, &mut nodes, device.id, node);
            tail = node;
        }

        // Once the effects have ids, pattern locks can name exactly the node
        // they ride. Audio-only tracks still build the same effects above,
        // but have no note events and therefore no locks to compile.
        if let Some(voice) = voice
            && !effects.is_empty()
        {
            if let Some(notes) = spec.node_mut(voice).and_then(NodeSpec::notes_mut) {
                *notes = notes_of_blocks(song, track, &effects);
            }
            for block in &track.blocks {
                let Some(pattern) = song.pattern(block.pattern_id) else {
                    continue;
                };
                for step in 0..PATTERN_STEPS {
                    for lock in &pattern.trig(step).locks {
                        let Some(id) = lock.device else {
                            continue;
                        };
                        if let Some((_, node)) = effects.iter().find(|(device, _)| *device == id)
                            && let Some(device) = track
                                .chain
                                .iter()
                                .chain(track.strip.iter())
                                .find(|device| device.id == id)
                        {
                            spec.lock_base(*node, lock.param, device.value(lock.param));
                        }
                    }
                }
            }
        }

        // One stable physical path for the complete channel. Its identity is
        // the persisted TrackId, so moving the track cannot redraw it.
        let personality = spec.push(personality_of(
            song.desk_personality,
            DeskPathIdentity::Track(track.id),
        ));
        spec.connect(tail, personality);
        tail = personality;

        // One permanent output stage after the whole channel path: meter,
        // post-fader sends, pan and bus routing all hear the same result.
        let out = spec.push(NodeSpec::Pan {
            pan: track.pan,
            gain: track.volume,
        });
        spec.connect(tail, out);
        let bus = bus_of(&desk, track);
        feed_bus(&mut bus_sources, &mut master_sources, bus, out);
        channel_routes[index] = Some(ChannelRoute {
            out,
            bus,
            identity: DeskPathIdentity::Track(track.id),
        });
        sends(
            &mut spec,
            index,
            track,
            out,
            &desk,
            &mut aux_sources,
            &mut nodes,
        );
        nodes.outputs[index] = Some(out);
        if index < TRACK_METERS {
            spec.meter(index, out);
            nodes.meters[index] = Some(index);
        }
    }

    add_adjacent_crosstalk(
        &mut spec,
        song.desk_personality,
        &channel_routes,
        &mut bus_sources,
        &mut master_sources,
    );
    add_adjacent_bus_crosstalk(&mut spec, song.desk_personality, &desk, &mut master_sources);
    seal_desk_feeds(
        &mut spec,
        &desk,
        master,
        &mut bus_sources,
        &mut aux_sources,
        &mut master_sources,
    );
    install_automation(&mut spec, song, &nodes);
    install_modulation(&mut spec, song, &nodes);
    spec.meter(MASTER_METER, desk.mix_out);
    spec.set_output(desk.mix_out);
    (spec, nodes)
}

/// Turn the graph's beat clock into the Song's file-format time.
///
/// This conversion is deliberately shared by the live host and offline
/// bounce. Both sample an envelope at the start of the audio they are about
/// to run, and both floor to the whole tick the Song editor addresses. A
/// corrupt clock is the top of the song rather than a fabricated far-future
/// point.
pub fn automation_tick(beat: f64) -> usize {
    if !beat.is_finite() || beat <= 0.0 {
        0
    } else {
        (beat * TICKS_PER_BEAT as f64) as usize
    }
}

#[derive(Clone, Copy)]
struct AutomationBinding {
    node: NodeId,
    param: u32,
    base: f32,
    min: f32,
    max: f32,
    scale: f32,
}

/// Put Song-owned curves inside the immutable audio schedule. This is the
/// timing path; [`automation_letters`] remains the public one-shot evaluator
/// for previews/tests, but live and bounce no longer depend on how often a UI
/// frame or render block happens to call it.
fn install_automation(spec: &mut GraphSpec, song: &Song, nodes: &SongNodes) {
    for (track_index, track) in song.tracks.iter().enumerate() {
        for (envelope_index, envelope) in track.automation.iter().enumerate() {
            if envelope.points.is_empty()
                || track.automation[..envelope_index]
                    .iter()
                    .any(|earlier| earlier.target == envelope.target)
            {
                continue;
            }
            let Some(binding) =
                automation_binding(track_index, track, envelope.target.as_str(), nodes)
            else {
                continue;
            };
            let points = envelope
                .points
                .iter()
                .filter(|point| point.value.is_finite())
                .map(|point| GraphAutomationPoint {
                    beat: point.tick as f64 / TICKS_PER_BEAT as f64,
                    value: point.value.clamp(binding.min, binding.max) * binding.scale,
                    bend: if point.bend.is_finite() {
                        point.bend.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    },
                })
                .collect();
            spec.automate(
                binding.node,
                binding.param,
                binding.base.clamp(binding.min, binding.max) * binding.scale,
                points,
            );
        }
    }
}

/// Put the Song's authored modulation inside the same immutable graph used
/// by live playback and offline bounce.
///
/// Target resolution deliberately goes through [`automation_binding`]: it is
/// already the one ownership-checked translation from a stable target string
/// to the concrete node and parameter minted by this build. The send scale is
/// applied to the base AND bounds here because modulation runs in the node's
/// units, while the Song stores the two channel sends normalized.
fn install_modulation(spec: &mut GraphSpec, song: &Song, nodes: &SongNodes) {
    let wires = song
        .mod_wires
        .iter()
        .filter_map(|wire| {
            let track = song.tracks.get(wire.track)?;
            let binding = automation_binding(wire.track, track, &wire.target, nodes)?;
            let scale = binding.scale;
            Some(WireSpec {
                id: wire.id,
                source: wire.source,
                node: binding.node,
                param: binding.param,
                min: binding.min * scale,
                max: binding.max * scale,
                log: modulation_target_is_log(track, &wire.target),
                base: binding.base * scale,
                chain: wire.chain(),
                enabled: wire.enabled,
                solo: wire.solo,
            })
        })
        .collect();
    spec.set_modulation(ModSpec {
        // Unwired sources stay in the plan: their engine telemetry is what
        // lets a newly-created LFO visibly move before it has a destination.
        sources: song.modulators.clone(),
        wires,
    });
}

/// Whether equal travel on this target means equal ratio. This is the same
/// mapping used by the device cards: frequency and time controls sweep in
/// ratios, while dB/semitone values are already logarithmic units and remain
/// linear here.
pub(crate) fn modulation_target_is_log(track: &Track, target: &str) -> bool {
    let Some((id, rest)) = target
        .strip_prefix(crate::targets::DEVICE_TARGET_PREFIX)
        .and_then(|rest| rest.split_once('.'))
    else {
        return false;
    };
    let Ok(id) = id.parse::<u64>() else {
        return false;
    };
    let Some((prefix, parameter)) = rest.split_once('.') else {
        return false;
    };
    let Some(device) = track
        .chain
        .iter()
        .chain(track.strip.iter())
        .find(|device| device.id == DeviceId(id) && device.kind.spec().prefix == prefix)
    else {
        return false;
    };
    let Some(param) = device
        .kind
        .spec()
        .params
        .iter()
        .find(|def| def.name == parameter)
        .map(|def| def.id)
    else {
        return false;
    };
    use crate::ui::device;
    match device.kind {
        DeviceKind::Flint
        | DeviceKind::Sibyl
        | DeviceKind::Ferric
        | DeviceKind::Umbra
        | DeviceKind::Gauge
        | DeviceKind::Tine
        | DeviceKind::Rack
        | DeviceKind::Console(_) => false,
        DeviceKind::Tone => param == crate::params::tone::FREQ,
        DeviceKind::Sigil => param == crate::params::sigil::FREQ,
        DeviceKind::Poly => device::poly_is_log(param),
        DeviceKind::Loom => device::loom::loom_is_log(param),
        DeviceKind::Haze => device::haze::haze_is_log(param),
        DeviceKind::Sampler => device::sampler_is_log(param),
        DeviceKind::Scomp => device::scomp::scomp_is_log(param),
        DeviceKind::Stab => device::stab::stab_is_log(param),
        DeviceKind::Kick => device::kick::kick_is_log(param),
        DeviceKind::Snare => device::snare_is_log(param),
        DeviceKind::Tom => device::tom_is_log(param),
        DeviceKind::Hat => device::hat_is_log(param),
        DeviceKind::Handclap => device::handclap_is_log(param),
        DeviceKind::Limiter => device::limiter_is_log(param),
        DeviceKind::SineSynth => device::sine_synth_is_log(param),
        DeviceKind::Reverb => device::reverb::reverb_is_log(param),
        DeviceKind::Sat => device::sat_is_log(param),
        DeviceKind::Lofi => device::lofi_is_log(param),
        DeviceKind::Sheen => device::sheen_is_log(param),
        DeviceKind::Disperser => device::disperser_is_log(param),
        DeviceKind::Tilt => device::tilt_is_log(param),
        DeviceKind::Phaser => device::phaser_is_log(param),
        DeviceKind::Echo => device::echo_is_log(param),
        DeviceKind::Eq => device::eq_is_log(param),
        DeviceKind::Filter => device::filter_is_log(param),
        DeviceKind::Glue => device::glue_is_log(param),
        DeviceKind::Clamp => device::clamp::clamp_is_log(param),
        DeviceKind::Prism => device::prism::prism_is_log(param),
        DeviceKind::Gate => device::gate_is_log(param),
        DeviceKind::Strip => device::strip_is_log(param),
        DeviceKind::Resyn => device::resyn_is_log(param),
        DeviceKind::Acid => device::acid_is_log(param),
        DeviceKind::Modulato => device::modulato::modulato_is_log(param),
        DeviceKind::Utility => device::utility_is_log(param),
    }
}

/// The exact units returned for one wire in engine telemetry. Kept beside
/// target resolution so the UI does not rebuild its allocated destination
/// catalog per wire (and so sends retain their graph-side percent scale).
pub(crate) fn modulation_target_span(track: &Track, target: &str) -> Option<f32> {
    if crate::targets::track_send_index(target).is_some() {
        // The Song stores sends as 0..1; their graph nodes speak 0..100.
        return Some(100.0);
    }
    let (min, max) = crate::targets::span_of(target)?;
    Some(crate::audio::modulation::wire_span(
        min,
        max,
        modulation_target_is_log(track, target),
    ))
}

fn automation_binding(
    track_index: usize,
    track: &Track,
    target: &str,
    nodes: &SongNodes,
) -> Option<AutomationBinding> {
    match target {
        crate::sequencing::TRACK_VOLUME => {
            let def = crate::params::pan::TABLE
                .iter()
                .find(|def| def.id == crate::params::pan::GAIN)?;
            Some(AutomationBinding {
                node: nodes.outputs.get(track_index).copied().flatten()?,
                param: crate::params::pan::GAIN,
                base: track.volume,
                min: def.min,
                max: def.max,
                scale: 1.0,
            })
        }
        crate::sequencing::TRACK_PAN => {
            let def = crate::params::pan::TABLE
                .iter()
                .find(|def| def.id == crate::params::pan::PAN)?;
            Some(AutomationBinding {
                node: nodes.outputs.get(track_index).copied().flatten()?,
                param: crate::params::pan::PAN,
                base: track.pan,
                min: def.min,
                max: def.max,
                scale: 1.0,
            })
        }
        _ => automation_send_binding(track_index, track, target, nodes)
            .or_else(|| automation_device_binding(track, target, nodes)),
    }
}

fn automation_send_binding(
    track_index: usize,
    track: &Track,
    target: &str,
    nodes: &SongNodes,
) -> Option<AutomationBinding> {
    let index = crate::targets::track_send_index(target)?;
    let param = *SEND_PARAMS.get(index)?;
    let node = nodes
        .sends
        .get(track_index)?
        .get(index)
        .copied()
        .flatten()?;
    let section = track
        .strip
        .iter()
        .find(|device| device.kind == DeviceKind::Console(crate::console::SectionKind::Out))?;
    Some(AutomationBinding {
        node,
        param,
        base: section.value(param) * 0.01,
        min: 0.0,
        max: 1.0,
        scale: 100.0,
    })
}

fn automation_device_binding(
    track: &Track,
    target: &str,
    nodes: &SongNodes,
) -> Option<AutomationBinding> {
    let (id, rest) = target
        .strip_prefix(crate::targets::DEVICE_TARGET_PREFIX)?
        .split_once('.')?;
    let id = DeviceId(id.parse().ok()?);
    let (prefix, param_name) = rest.split_once('.')?;
    let device = track
        .chain
        .iter()
        .chain(track.strip.iter())
        .find(|device| device.id == id)?;
    let device_spec = device.kind.spec();
    if device_spec.prefix != prefix {
        return None;
    }
    let def = device_spec
        .params
        .iter()
        .find(|def| def.name == param_name)?;
    Some(AutomationBinding {
        node: nodes
            .devices
            .iter()
            .find_map(|(device, node)| (*device == id).then_some(*node))?,
        param: def.id,
        base: device.value(def.id),
        min: def.min,
        max: def.max,
        scale: 1.0,
    })
}

/// Resolve every active stored envelope at one Song tick into concrete engine
/// letters.
///
/// This is the ONE evaluator used by Stage's live host and its offline
/// renderer. It emits only targets which actually carry points; static knobs
/// are already in the compiled graph. Track level and pan land on the track's
/// output node, the two canonical send targets land on their explicit return
/// taps, and a device target lands on the one real node registered for that
/// instance. Device ownership is checked against the track carrying the
/// envelope, so a stale or hand-edited target can never automate a different
/// track's identically-shaped device.
///
/// Unsupported targets are intentionally silent: master, bus and return rails
/// do not yet own automation envelopes in [`Song`], and a bypassed or otherwise
/// uncompiled device has no node to receive a letter. The fixed analog desk has
/// exactly two channel sends; targets C through H remain valid file-format
/// names for the legacy configurable-return model but do not invent routes in
/// this graph.
pub fn automation_letters(
    song: &Song,
    nodes: &SongNodes,
    tick: usize,
    out: &mut Vec<crate::audio::graph::ParamChange>,
) {
    for (track_index, track) in song.tracks.iter().enumerate() {
        for (envelope_index, envelope) in track.automation.iter().enumerate() {
            if envelope.points.is_empty()
                || track.automation[..envelope_index]
                    .iter()
                    .any(|earlier| earlier.target == envelope.target)
            {
                continue;
            }
            let target = envelope.target.as_str();
            let binding = match target {
                crate::sequencing::TRACK_VOLUME => {
                    let Some(node) = nodes.outputs.get(track_index).copied().flatten() else {
                        continue;
                    };
                    Some((node, crate::params::pan::GAIN, track.volume_at(tick)))
                }
                crate::sequencing::TRACK_PAN => {
                    let Some(node) = nodes.outputs.get(track_index).copied().flatten() else {
                        continue;
                    };
                    Some((node, crate::params::pan::PAN, track.pan_at(tick)))
                }
                _ => send_binding(track_index, track, target, nodes, tick)
                    .or_else(|| device_binding(track, target, nodes, tick)),
            };
            let Some((node, param, value)) = binding else {
                continue;
            };
            if value.is_finite() {
                out.push(crate::audio::graph::ParamChange {
                    node: node.to_bits(),
                    param,
                    value,
                });
            }
        }
    }
}

/// Resolve one of the analog desk's two canonical post-fader sends.
///
/// The OUT section stores its knobs as percentages while automation's stable
/// `track.send.*` targets use normalized gain. The letter goes back to the
/// Send node as a percentage because that node deliberately speaks OUT's
/// parameter protocol; the unit conversion happens here, once.
fn send_binding(
    track_index: usize,
    track: &Track,
    target: &str,
    nodes: &SongNodes,
    tick: usize,
) -> Option<(NodeId, u32, f32)> {
    let index = crate::targets::track_send_index(target)?;
    let param = *SEND_PARAMS.get(index)?;
    let node = nodes
        .sends
        .get(track_index)?
        .get(index)
        .copied()
        .flatten()?;
    let section = track
        .strip
        .iter()
        .find(|device| device.kind == DeviceKind::Console(crate::console::SectionKind::Out))?;
    let base = (section.value(param) * 0.01).clamp(0.0, 1.0);
    let value = track.value_at(target, tick, base).clamp(0.0, 1.0);
    Some((node, param, value * 100.0))
}

/// Resolve `dev.<instance>.<kind>.<parameter>` against the device on THIS
/// track, then against the graph's one-device/one-node table.
fn device_binding(
    track: &Track,
    target: &str,
    nodes: &SongNodes,
    tick: usize,
) -> Option<(NodeId, u32, f32)> {
    let (id, rest) = target
        .strip_prefix(crate::targets::DEVICE_TARGET_PREFIX)?
        .split_once('.')?;
    let id = DeviceId(id.parse().ok()?);
    let (prefix, param_name) = rest.split_once('.')?;
    let device = track
        .chain
        .iter()
        .chain(track.strip.iter())
        .find(|device| device.id == id)?;
    let spec = device.kind.spec();
    if spec.prefix != prefix {
        return None;
    }
    let def = spec.params.iter().find(|def| def.name == param_name)?;
    let node = nodes
        .devices
        .iter()
        .find_map(|(device, node)| (*device == id).then_some(*node))?;
    let value = track
        .value_at(target, tick, device.value(def.id))
        .clamp(def.min, def.max);
    Some((node, def.id, value))
}

/// A track's pattern blocks as notes in song time: each block plays its
/// pattern from the block's start, repeated to fill the block, and any
/// note that would outrun the block is cut at the block's end.
fn notes_of_blocks(song: &Song, track: &Track, effects: &[(DeviceId, NodeId)]) -> Vec<GraphNote> {
    let mut out = Vec::new();
    for block in &track.blocks {
        let Some(pattern) = song.pattern(block.pattern_id) else {
            continue;
        };
        let pattern_len = beats(pattern.length_ticks.max(1));
        let block_len = beats(block.length_ticks);
        if block_len <= 0.0 {
            continue;
        }
        let base = notes_of(song, pattern, effects);
        let start = beats(block.start_tick);
        let mut offset = 0.0;
        while offset < block_len {
            for note in &base {
                let at = offset + note.start_beats;
                if at >= block_len {
                    continue;
                }
                let mut placed = note.clone();
                placed.start_beats = start + at;
                placed.len_beats = note.len_beats.min(block_len - at);
                out.push(placed);
            }
            offset += pattern_len;
        }
    }
    out.sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
    out
}

/// A track's audio blocks as clip nodes in song time.
fn audio_blocks_of(song: &Song, track: &Track) -> Vec<NodeSpec> {
    track
        .audio_blocks
        .iter()
        .filter_map(|block| {
            let source = &block.source;
            // Transpose/detune and reversal are rendered green-side. The
            // current `path` is the transposed render when one is applied;
            // `playing_path` adds the reverse cache when requested and
            // deliberately returns None while that cache is not ready.
            let path = source.playing_path()?;
            // A clip-relative loop brace is in ticks; the node wants the
            // loop's start in the file's own frames. Resolve the complete
            // song map at the file rate so a brace spanning a tempo mark
            // keeps its authored musical endpoints instead of stretching
            // the whole region at only the block's opening tempo.
            let timeline = crate::tempo::TempoTable::build(
                song,
                f64::from(source.sample_rate.max(1)),
                song.bpm,
            );
            let block_sample = timeline.sample_at(block.start_tick);
            let (played_frames, loop_start_frames, has_brace) = block
                .loop_brace
                .filter(|brace| brace.length_ticks > 0 && source.source_frames > 0)
                .map_or((source.source_frames, 0, false), |brace| {
                    let start_tick = block.start_tick.saturating_add(brace.start_tick);
                    let end_tick = start_tick.saturating_add(brace.length_ticks);
                    let end = timeline
                        .sample_at(end_tick)
                        .saturating_sub(block_sample)
                        .clamp(1, source.source_frames);
                    let start = timeline
                        .sample_at(start_tick)
                        .saturating_sub(block_sample)
                        .min(end.saturating_sub(1));
                    (end, start, true)
                });
            Some(NodeSpec::AudioClip {
                path,
                start_beats: beats(block.start_tick),
                length_beats: Some(beats(block.length_ticks)),
                source_offset_frames: source.playing_offset(),
                source_frames: Some(played_frames),
                loop_clip: source.looped || has_brace,
                loop_start_frames,
                gain: source.gain,
                fade_in_frames: source.fade_in,
                fade_out_frames: source.fade_out,
                fade_in_shape: source.fade_in_curve,
                fade_out_shape: source.fade_out_curve,
                envelope: source
                    .envelope
                    .iter()
                    .map(|(at, db)| (*at, audio_envelope_gain(*db)))
                    .collect(),
            })
        })
        .collect()
}

/// One pattern's trigs as graph notes, in the key the song is in.
///
/// A pitch in this model is an ANCHOR — a degree of the song's key, or an
/// absolute frequency — so resolving it needs the key and cannot be done
/// by the caller. The engine speaks MIDI numbers, so the resolution ends
/// at the nearest one.
fn notes_of(song: &Song, pattern: &Pattern, effects: &[(DeviceId, NodeId)]) -> Vec<GraphNote> {
    let mut notes = Vec::new();
    for step in 0..PATTERN_STEPS {
        let trig = pattern.trig(step);
        let trigless = trig.notes.is_empty() && !trig.locks.is_empty();
        if !trig.enabled && !trigless {
            continue;
        }
        let at = step * PATTERN_STEP_TICKS;
        for note in &trig.notes {
            if note.muted {
                continue;
            }
            // Micro-timing is a displacement in ticks and may point either
            // way; the first step's early note lands at the top of the
            // pattern rather than before it.
            let start = at.saturating_add_signed(note.micro_ticks as isize);
            notes.push(GraphNote {
                start_beats: beats(start),
                len_beats: beats(note.length_ticks),
                pitch: nearest_midi(note.pitch.resolve(&song.key)),
                vel: note.velocity,
                // The trig's locks ride every note of the chord: the
                // graph locks per note, the trig locks per firing, and a
                // chord is one firing.
                // The voice's locks, and the effects' locks each addressed to
                // the node its device became. A lock on a device that is not
                // on the chain now — bypassed, or gone — is left out, not
                // misdelivered.
                plocks: trig
                    .locks
                    .iter()
                    .filter(|lock| lock.device.is_none())
                    .map(|lock| (lock.param, lock.value))
                    .collect(),
                fx_locks: trig
                    .locks
                    .iter()
                    .filter_map(|lock| {
                        let device = lock.device?;
                        let (_, node) = effects.iter().find(|(id, _)| *id == device)?;
                        Some((node.to_bits(), lock.param, lock.value))
                    })
                    .collect(),
                prob: trig.probability,
                cond: None,
            });
        }
        if trigless {
            // Velocity zero is the compiled graph's explicit lock-only
            // event. Real notes are clamped to 1..=127; compile_events can
            // therefore schedule a lock and its cell-boundary restore
            // without a second public event type or a phantom voice.
            notes.push(GraphNote {
                start_beats: beats(at),
                len_beats: beats(PATTERN_STEP_TICKS),
                pitch: 0,
                vel: 0,
                plocks: trig
                    .locks
                    .iter()
                    .filter(|lock| lock.device.is_none())
                    .map(|lock| (lock.param, lock.value))
                    .collect(),
                fx_locks: trig
                    .locks
                    .iter()
                    .filter_map(|lock| {
                        let device = lock.device?;
                        let (_, node) = effects.iter().find(|(id, _)| *id == device)?;
                        Some((node.to_bits(), lock.param, lock.value))
                    })
                    .collect(),
                prob: trig.probability,
                cond: None,
            });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHICH DEVICES A LETTER CAN REACH.
    ///
    /// The host sends a knob turn to every device in `nodes.devices` and
    /// to nothing else, so this table is the exact set of things a turn
    /// can be heard on without a rebuild. An instrument that is playing
    /// is in it. A section that is switched OUT is NOT — it was never
    /// built, because a section that is out is not in the signal path —
    /// so its knobs move the document and nothing else until it is
    /// switched in, which rebuilds and compiles the value in.
    ///
    /// That is correct, and it is also the thing most likely to be read
    /// as a knob that does not work.
    #[test]
    fn a_letter_reaches_what_is_built_and_nothing_else() {
        use crate::console::SectionKind;
        let mut song = song_with_a_clip();
        let kick = song
            .add_device(0, crate::devices::DeviceKind::Kick)
            .expect("a kick on the track");
        let tone = song
            .section(0, SectionKind::Tone)
            .expect("every channel has a tone")
            .id;
        // TONE is out at its defaults, as every switchable section is.
        assert!(song.device(tone).is_some_and(|device| device.bypassed));

        let (_, nodes) = build(&song, &playing(&song));
        let reaches =
            |id: crate::sequencing::DeviceId| nodes.devices.iter().any(|(other, _)| *other == id);
        assert!(
            reaches(kick),
            "the instrument is not in the letter table: a knob on it could not be \
             heard without rebuilding the graph"
        );
        assert!(!reaches(tone), "a switched-out section was built anyway");

        // Switched IN, it is built and a letter reaches it.
        if let Some(device) = song.device_mut(tone) {
            device.bypassed = false;
        }
        let (_, nodes) = build(&song, &playing(&song));
        assert!(
            nodes.devices.iter().any(|(other, _)| *other == tone),
            "a switched-in section is still not in the letter table"
        );
    }

    /// A send is a real path, not a number on a card: opening one on a
    /// channel's OUT puts that channel into the return's rail, and the
    /// return lands in the mix. Closed, the tap is still built — it
    /// ramps, so opening one is a fade rather than a click. Generated taps
    /// live in the alias table, never as duplicate device identities.
    #[test]
    fn a_send_taps_the_channel_into_its_return() {
        use crate::console::SectionKind;
        use crate::params::console::out as p;
        let mut song = song_with_a_clip();
        let track = 0;
        let out = song
            .section(track, SectionKind::Out)
            .expect("every channel has an out")
            .id;
        // Closed: the taps exist, and they are the OUT section's.
        let (_, nodes) = build(&song, &playing(&song));
        assert_eq!(
            nodes.devices.iter().filter(|(id, _)| *id == out).count(),
            1,
            "OUT is one device, however many taps it owns"
        );
        let taps = nodes
            .param_aliases
            .iter()
            .filter(|(id, _)| *id == out)
            .count();
        assert_eq!(taps, song.console.aux.len(), "one generated tap per return");
        // Every tap has exactly one telemetry slot between them all:
        // the section reports, the taps have nothing of their own.
        assert_eq!(
            nodes.telemetry.iter().filter(|(id, _)| *id == out).count(),
            1
        );
        // Opened: the tap's gain is the amount the hand set.
        if let Some(device) = song.device_mut(out) {
            device.set(p::SEND_TAPE, 50.0);
        }
        let (spec, nodes) = build(&song, &playing(&song));
        let opened: Vec<f32> = nodes
            .param_aliases
            .iter()
            .filter(|(id, _)| *id == out)
            .filter_map(|(_, node)| match spec.node(*node) {
                Some(NodeSpec::Send { gain, param }) if *param == p::SEND_TAPE => Some(*gain),
                _ => None,
            })
            .collect();
        assert_eq!(opened.len(), 1, "one tape tap on this channel");
        assert!(
            (opened[0] - 0.5).abs() < 0.001,
            "the send did not reach its tap: {opened:?}"
        );
    }

    /// Every supported Song target resolves to its own concrete node and
    /// parameter. In particular, OUT remains one device while send A takes
    /// the explicit tap address beside it — no duplicate-id search decides
    /// which one automation happened to reach.
    #[test]
    fn automation_targets_resolve_without_device_alias_ambiguity() {
        use crate::console::SectionKind;
        use crate::params::{console::tone, sat};

        let mut song = song_with_a_clip();
        let sat_id = song.add_device(0, DeviceKind::Sat).expect("a sat insert");
        let tone_id = song
            .section(0, SectionKind::Tone)
            .expect("the fixed strip has TONE")
            .id;
        song.device_mut(tone_id).expect("tone section").bypassed = false;

        let sat_target = crate::targets::device_target(
            sat_id.0,
            DeviceKind::Sat.spec(),
            sat::TABLE
                .iter()
                .find(|def| def.id == sat::DRIVE)
                .expect("drive definition")
                .name,
        );
        let tone_target = crate::targets::device_target(
            tone_id.0,
            DeviceKind::Console(SectionKind::Tone).spec(),
            tone::TABLE
                .iter()
                .find(|def| def.id == tone::MID)
                .expect("mid definition")
                .name,
        );
        let track = &mut song.tracks[0];
        for (target, value) in [
            (crate::sequencing::TRACK_VOLUME.to_owned(), 0.25),
            (crate::sequencing::TRACK_PAN.to_owned(), -0.5),
            ("track.send.a".to_owned(), 0.75),
            (sat_target, 2.0),
            (tone_target, 3.0),
        ] {
            track.insert_point(&target, 0, value);
        }

        let (_, nodes) = build_song(&song);
        let mut letters = Vec::new();
        automation_letters(&song, &nodes, 0, &mut letters);

        let out = nodes.outputs[0].expect("track output");
        let send = nodes.sends[0][0].expect("send A tap");
        let sat_node = nodes
            .devices
            .iter()
            .find_map(|(id, node)| (*id == sat_id).then_some(*node))
            .expect("sat node");
        let tone_node = nodes
            .devices
            .iter()
            .find_map(|(id, node)| (*id == tone_id).then_some(*node))
            .expect("tone node");
        let says = |node: NodeId, param: u32, value: f32| {
            letters.iter().any(|letter| {
                letter.node == node.to_bits()
                    && letter.param == param
                    && (letter.value - value).abs() < 1e-6
            })
        };
        assert!(says(out, crate::params::pan::GAIN, 0.25));
        assert!(says(out, crate::params::pan::PAN, -0.5));
        assert!(says(send, crate::params::console::out::SEND_TAPE, 75.0));
        assert!(
            says(sat_node, sat::DRIVE, 2.0),
            "sat target did not resolve: {letters:?}"
        );
        assert!(
            says(tone_node, tone::MID, 3.0),
            "strip target did not resolve: {letters:?}"
        );

        let mut ids: Vec<_> = nodes.devices.iter().map(|(id, _)| *id).collect();
        let count = ids.len();
        ids.sort_by_key(|id| id.0);
        ids.dedup();
        assert_eq!(ids.len(), count, "the real-device table contains an alias");
        assert!(
            nodes.param_aliases.iter().any(|(_, node)| *node == send),
            "the send tap was not named as an alias"
        );
    }

    /// An instance id is not enough: the envelope must live on the track
    /// which owns that instance. A stale cross-track target is orphaned,
    /// never delivered to the other lane's effect.
    #[test]
    fn device_automation_cannot_cross_a_track_boundary() {
        use crate::params::sat;

        let mut song = song_with_a_clip();
        song.add_track(TrackKind::Instrument);
        song.tracks[1].blocks = song.tracks[0].blocks.clone();
        let second = song
            .add_device(1, DeviceKind::Sat)
            .expect("an effect on track two");
        let target = crate::targets::device_target(
            second.0,
            DeviceKind::Sat.spec(),
            sat::TABLE
                .iter()
                .find(|def| def.id == sat::DRIVE)
                .expect("drive definition")
                .name,
        );
        song.tracks[0].insert_point(&target, 0, 0.9);

        let (_, nodes) = build_song(&song);
        let mut letters = Vec::new();
        automation_letters(&song, &nodes, 0, &mut letters);
        assert!(
            !letters.iter().any(|letter| letter.param == sat::DRIVE),
            "track one's envelope reached track two's device"
        );
    }
    use crate::sequencing::{Note, Scene, Slot, TrackKind};

    /// A song with one instrument track holding one pattern, and a scene
    /// that fires it.
    /// Every track playing scene zero — the table the harness wants
    /// whenever it just needs the clips to sound.
    fn playing(song: &Song) -> Vec<Option<usize>> {
        vec![Some(0); song.tracks.len()]
    }

    /// A song whose first scene holds a clip with one note in it. The
    /// clip lives IN the session, because that is where the compiler
    /// looks — a scene handed in from the side would be a scene no
    /// performer could have fired.
    fn song_with_a_clip() -> Song {
        let mut song = Song::default();
        let pattern = song.patterns[0].id;
        song.patterns[0].toggle(0, Note::new(60, PATTERN_STEP_TICKS, 100));
        song.session.scenes[0] = Scene {
            slots: vec![Slot {
                track: song.tracks[0].id,
                clip: Clip::Pattern(pattern),
            }],
        };
        song
    }

    #[test]
    fn both_song_compilers_install_the_same_modulation_plan() {
        let mut song = song_with_a_clip();
        let source = song.add_lfo().expect("an LFO fits");
        let wire = song
            .add_mod_wire(source, 0, crate::sequencing::TRACK_PAN)
            .expect("a pan wire fits");

        for (spec, nodes) in [build(&song, &playing(&song)), build_song(&song)] {
            let modulation = spec.modulation();
            assert_eq!(modulation.sources, song.modulators);
            assert_eq!(modulation.wires.len(), 1);
            let compiled = &modulation.wires[0];
            assert_eq!(compiled.id, wire);
            assert_eq!(compiled.source, source);
            assert_eq!(compiled.node, nodes.outputs[0].expect("a channel output"));
            assert_eq!(compiled.param, crate::params::pan::PAN);
            assert_eq!(
                (compiled.min, compiled.max, compiled.base),
                (-1.0, 1.0, 0.0)
            );
            assert!(!compiled.log);
        }
    }

    #[test]
    fn modulation_resolves_instance_log_law_and_send_units() {
        let mut song = song_with_a_clip();
        let source = song.add_lfo().expect("an LFO fits");
        let filter = song
            .add_device(0, DeviceKind::Filter)
            .expect("a filter fits");
        let target = crate::targets::device_target(filter.0, DeviceKind::Filter.spec(), "cutoff");
        song.add_mod_wire(source, 0, target)
            .expect("the cutoff resolves");
        song.add_mod_wire(source, 0, crate::targets::TRACK_SEND_TARGETS[0])
            .expect("the analog send resolves");

        let (spec, nodes) = build_song(&song);
        let filter_node = nodes
            .devices
            .iter()
            .find_map(|(id, node)| (*id == filter).then_some(*node))
            .expect("the filter compiled");
        let cutoff = spec
            .modulation()
            .wires
            .iter()
            .find(|wire| wire.node == filter_node && wire.param == crate::params::filter::CUTOFF)
            .expect("the cutoff wire compiled");
        assert!(cutoff.log, "a frequency wire was mapped linearly");

        let send_node = nodes.sends[0][0].expect("send A compiled");
        let send = spec
            .modulation()
            .wires
            .iter()
            .find(|wire| wire.node == send_node)
            .expect("the send wire compiled");
        assert_eq!((send.min, send.max), (0.0, 100.0));
        assert!((0.0..=100.0).contains(&send.base));
    }

    fn audio_block(
        id: u64,
        start_tick: usize,
        length_ticks: usize,
    ) -> crate::sequencing::AudioBlock {
        crate::sequencing::AudioBlock {
            id: crate::sequencing::BlockId(id),
            name: format!("take {id}"),
            start_tick,
            length_ticks,
            source: ron::from_str(
                r#"(path:"/x/take.wav",sample_rate:48000,source_offset:0,source_frames:96000,gain:1.0,looped:false)"#,
            )
            .expect("a minimal source deserializes from its required fields"),
            loop_brace: None,
        }
    }

    /// The song compiler lays a block's pattern at the block's start,
    /// repeats it to fill the block, and cuts the last repeat at the
    /// block's end; the voice's node does not loop.
    #[test]
    fn a_block_plays_its_pattern_from_its_start_repeated_to_its_end() {
        let mut song = song_with_a_clip();
        // A second note halfway through the bar, so repeats are visible.
        song.patterns[0].toggle(8, Note::new(62, PATTERN_STEP_TICKS, 90));
        let pattern = song.patterns[0].id;
        let bar = crate::sequencing::TICKS_PER_BEAT * 4;
        // The pattern is four bars; place it at bar two, six bars long,
        // so the second repeat is cut after two bars.
        song.tracks[0].blocks.clear();
        song.tracks[0].blocks.push(crate::sequencing::PatternBlock {
            id: crate::sequencing::BlockId(1),
            pattern_id: pattern,
            start_tick: 2 * bar,
            length_ticks: 6 * bar,
        });
        let (spec, _) = build_song(&song);
        let (notes, loops) = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Poly {
                    notes,
                    loop_len_beats,
                    ..
                } => Some((notes.clone(), *loop_len_beats)),
                _ => None,
            })
            .expect("the voice is in the graph");
        assert_eq!(loops, None, "the song's voice loops");
        let starts: Vec<f64> = notes.iter().map(|n| n.start_beats).collect();
        // Beats: bar two starts at 8; the pattern's notes at 0 and 2
        // beats in; the repeat starts four bars (16 beats) later.
        assert_eq!(starts, vec![8.0, 10.0, 24.0, 26.0]);
    }

    /// An audio block becomes a clip node at its start, and a track with
    /// nothing placed on it is not in the graph.
    #[test]
    fn audio_blocks_stream_from_their_start_and_empty_tracks_are_absent() {
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        song.tracks[0].audio_blocks.push(crate::sequencing::AudioBlock {
            id: crate::sequencing::BlockId(7),
            name: "take".to_owned(),
            start_tick: crate::sequencing::TICKS_PER_BEAT * 4,
            length_ticks: crate::sequencing::TICKS_PER_BEAT * 8,
            source: ron::from_str(
                r#"(path:"/x/take.wav",sample_rate:48000,source_offset:0,source_frames:96000,gain:1.0,looped:false)"#,
            )
            .expect("a minimal source deserializes from its required fields"),
            loop_brace: None,
        });
        let (spec, nodes) = build_song(&song);
        let clip = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::AudioClip {
                    start_beats,
                    length_beats,
                    ..
                } => Some((*start_beats, *length_beats)),
                _ => None,
            })
            .expect("the clip is in the graph");
        assert_eq!(clip, (4.0, Some(8.0)));
        assert!(nodes.outputs[0].is_some());
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "a voice was built for a track with no notes"
        );
    }

    /// The Song owns rich audio-source metadata; compiling its timeline must
    /// not quietly replace those edits with the old zero-value defaults.
    #[test]
    fn audio_blocks_keep_their_render_path_region_fades_and_envelope() {
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        let mut block = audio_block(
            8,
            crate::sequencing::TICKS_PER_BEAT * 2,
            crate::sequencing::TICKS_PER_BEAT * 4,
        );
        block.source.path = std::path::PathBuf::from("/x/pitched-take.wav");
        block.source.transpose = 7.0;
        block.source.detune = -12.0;
        block.source.transposed_from = Some(std::path::PathBuf::from("/x/original-take.wav"));
        block.source.applied_ratio = 1.49;
        block.source.source_offset = 321;
        block.source.source_frames = 48_000;
        block.source.gain = 0.625;
        block.source.fade_in = 480;
        block.source.fade_out = 960;
        block.source.fade_in_curve = -0.25;
        block.source.fade_out_curve = 0.75;
        block.source.envelope = vec![
            (0, -60.0),
            (12_000, -6.0),
            (24_000, 9.0),
            (36_000, f32::NAN),
        ];
        block.loop_brace = Some(crate::sequencing::LoopBrace {
            start_tick: crate::sequencing::TICKS_PER_BEAT / 2,
            length_ticks: crate::sequencing::TICKS_PER_BEAT,
        });
        let authored = block.source.clone();
        song.tracks[0].audio_blocks.push(block);

        let (spec, _) = build_song(&song);
        let clip = spec
            .iter_ordered()
            .find_map(|(_, node)| matches!(node, NodeSpec::AudioClip { .. }).then_some(node))
            .expect("the audio block compiled");
        let NodeSpec::AudioClip {
            path,
            source_offset_frames,
            source_frames,
            loop_clip,
            loop_start_frames,
            gain,
            fade_in_frames,
            fade_out_frames,
            fade_in_shape,
            fade_out_shape,
            envelope,
            ..
        } = clip
        else {
            unreachable!("selected only AudioClip above")
        };
        assert_eq!(
            path, &authored.path,
            "the current transpose render is played"
        );
        assert_eq!(*source_offset_frames, authored.playing_offset());
        assert_eq!(*source_frames, Some(36_000), "the brace end caps the pass");
        assert!(*loop_clip);
        assert_eq!(*loop_start_frames, 12_000, "the brace start is the wrap");
        assert_eq!(*gain, authored.gain);
        assert_eq!(*fade_in_frames, authored.fade_in);
        assert_eq!(*fade_out_frames, authored.fade_out);
        assert_eq!(*fade_in_shape, authored.fade_in_curve);
        assert_eq!(*fade_out_shape, authored.fade_out_curve);
        assert_eq!(envelope[0], (0, 0.0), "the envelope floor is silence");
        assert!(
            (envelope[1].1 - crate::dsp::arith::db_to_gain(-6.0)).abs() < 1e-6,
            "dB is compiled to linear gain"
        );
        assert!(
            (envelope[2].1 - crate::dsp::arith::db_to_gain(6.0)).abs() < 1e-6,
            "the authored ceiling is enforced"
        );
        assert_eq!(envelope[3], (36_000, 0.0), "a corrupt value is safe");
    }

    #[test]
    fn an_audio_loop_brace_integrates_tempo_marks_inside_the_clip() {
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        let block_start = TICKS_PER_BEAT * 2;
        let mut block = audio_block(81, block_start, TICKS_PER_BEAT * 4);
        block.loop_brace = Some(crate::sequencing::LoopBrace {
            start_tick: 0,
            length_ticks: TICKS_PER_BEAT * 2,
        });
        song.tempo.push(crate::sequencing::TempoMark {
            tick: block_start + TICKS_PER_BEAT,
            bpm: 60.0,
        });
        song.tracks[0].audio_blocks.push(block);

        let (spec, _) = build_song(&song);
        let (frames, loop_start) = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::AudioClip {
                    source_frames,
                    loop_start_frames,
                    ..
                } => Some((*source_frames, *loop_start_frames)),
                _ => None,
            })
            .expect("the audio block compiled");
        assert_eq!(loop_start, 0);
        assert_eq!(
            frames,
            Some(72_000),
            "one beat at 120 and one at 60 must occupy 24k + 48k source frames"
        );
    }

    /// Reverse is a green-side rendered cache. Until it exists, playing the
    /// forward file would be an audible lie, so the block must compile quiet.
    #[test]
    fn a_reverse_waiting_for_its_cache_does_not_play_forward() {
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        let mut block = audio_block(9, 0, crate::sequencing::TICKS_PER_BEAT * 4);
        block.source.path = std::env::temp_dir().join(format!(
            "daw-song-graph-no-reverse-cache-{}.wav",
            std::process::id()
        ));
        block.source.file_frames = block.source.source_frames;
        block.source.reversed = true;
        assert_eq!(
            block.source.playing_path(),
            None,
            "the fixture unexpectedly has a reverse render"
        );
        song.tracks[0].audio_blocks.push(block);

        let (spec, nodes) = build_song(&song);
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::AudioClip { .. })),
            "the forward file leaked in while reversal was pending"
        );
        assert_eq!(nodes.outputs[0], None, "a silent channel costs no path");
    }

    /// Recorded sound is a channel source just like its instrument: every
    /// active insert and strip section must precede the fader, meter and bus.
    #[test]
    fn an_audio_block_traverses_the_insert_chain_and_channel_strip() {
        use crate::console::SectionKind;
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        let reverb = song
            .add_device(0, DeviceKind::Reverb)
            .expect("an insert on the audio channel");
        let tone = song
            .section(0, SectionKind::Tone)
            .expect("the fixed strip has TONE")
            .id;
        song.device_mut(tone).expect("the section exists").bypassed = false;
        song.tracks[0]
            .audio_blocks
            .push(audio_block(10, 0, crate::sequencing::TICKS_PER_BEAT * 4));

        let (spec, nodes) = build_song(&song);
        let clip = spec
            .iter_ordered()
            .find_map(|(id, node)| matches!(node, NodeSpec::AudioClip { .. }).then_some(id))
            .expect("the clip node");
        let out = nodes.outputs[0].expect("the channel output");
        let active: Vec<DeviceId> = song.tracks[0]
            .chain
            .iter()
            .chain(song.tracks[0].strip.iter())
            .filter(|device| {
                !device.is_instrument() && !device.bypassed && effect_of(device).is_some()
            })
            .map(|device| device.id)
            .collect();
        assert!(active.contains(&reverb));
        assert!(active.contains(&tone));

        let path: Vec<(DeviceId, NodeId)> = active
            .iter()
            .map(|device| {
                (
                    *device,
                    nodes
                        .devices
                        .iter()
                        .find_map(|(id, node)| (*id == *device).then_some(*node))
                        .expect("every active channel device is registered"),
                )
            })
            .collect();
        let reaches =
            |from: NodeId, to: NodeId| {
                let mut frontier = vec![from];
                let mut seen = Vec::new();
                while let Some(node) = frontier.pop() {
                    if node == to {
                        return true;
                    }
                    if seen.contains(&node) {
                        continue;
                    }
                    seen.push(node);
                    frontier.extend(spec.wires().iter().filter_map(|(wire_from, wire_to)| {
                        (*wire_from == node).then_some(*wire_to)
                    }));
                }
                false
            };
        let mut previous = clip;
        for (device, node) in path.iter().copied() {
            // Generated send aliases live in a separate table, so this is
            // always the actual section node in the channel path.
            assert!(
                reaches(previous, node),
                "device {device:?} at {node:?} was bypassed after {previous:?}; outgoing: {:?}; path: {:?}",
                spec.wires()
                    .iter()
                    .filter(|(from, _)| *from == previous)
                    .collect::<Vec<_>>(),
                path
            );
            previous = node;
        }
        let channel_keys = DeskPathIdentity::Track(song.tracks[0].id).stereo_keys();
        let personality = spec
            .iter_ordered()
            .find_map(|(id, node)| match node {
                NodeSpec::DeskPath {
                    left_identity,
                    right_identity,
                    ..
                } if (*left_identity, *right_identity) == channel_keys => Some(id),
                _ => None,
            })
            .expect("the physical channel path");
        assert!(spec.wires().contains(&(previous, personality)));
        assert!(spec.wires().contains(&(personality, out)));
        assert!(
            !spec.wires().contains(&(clip, out)),
            "the old dry side-door still reaches the fader"
        );
    }

    /// Pan has one input, so overlapping placements need an explicit source
    /// bus. Both clips must reach that sum before any channel processing.
    #[test]
    fn overlapping_audio_blocks_are_summed_before_the_channel_path() {
        let mut song = song_with_a_clip();
        song.tracks[0].blocks.clear();
        song.tracks[0]
            .audio_blocks
            .push(audio_block(11, 0, crate::sequencing::TICKS_PER_BEAT * 4));
        song.tracks[0]
            .audio_blocks
            .push(audio_block(12, 0, crate::sequencing::TICKS_PER_BEAT * 4));

        let (spec, nodes) = build_song(&song);
        let clips: Vec<NodeId> = spec
            .iter_ordered()
            .filter_map(|(id, node)| matches!(node, NodeSpec::AudioClip { .. }).then_some(id))
            .collect();
        assert_eq!(clips.len(), 2);
        let sum = spec
            .wires()
            .iter()
            .find_map(|(from, to)| {
                (*from == clips[0]
                    && matches!(spec.node(*to), Some(NodeSpec::Mixer { gain }) if *gain == 1.0))
                .then_some(*to)
            })
            .expect("the first clip reaches a source sum");
        assert!(spec.wires().contains(&(clips[1], sum)));
        let out = nodes.outputs[0].expect("the channel output");
        assert!(!spec.wires().contains(&(clips[0], out)));
        assert!(!spec.wires().contains(&(clips[1], out)));
    }

    /// Route selection alone is quiet; IN and armed AUTO are the two states
    /// that put hardware input into both session and arrangement compilers.
    #[test]
    fn monitored_audio_input_uses_the_channel_path_in_both_builders() {
        use crate::sequencing::{Monitor, TrackInput};

        fn assert_input_count(song: &Song, expected: usize) {
            let playing = vec![None; song.tracks.len()];
            for (spec, nodes) in [build(song, &playing), build_song(song)] {
                assert_eq!(
                    spec.iter_ordered()
                        .filter(|(_, node)| matches!(node, NodeSpec::Input { .. }))
                        .count(),
                    expected
                );
                assert_eq!(nodes.outputs[0].is_some(), expected > 0);
            }
        }

        let mut song = song_with_a_clip();
        song.tracks[0].kind = TrackKind::Audio;
        song.tracks[0].blocks.clear();
        song.tracks[0].input = TrackInput::Mono(3);
        song.tracks[0].armed = true;
        song.tracks[0].monitor = Monitor::Off;
        assert_input_count(&song, 0);
        song.tracks[0].armed = false;
        song.tracks[0].monitor = Monitor::Auto;
        assert_input_count(&song, 0);
        song.tracks[0].armed = true;
        assert_input_count(&song, 1);
        song.tracks[0].armed = false;
        song.tracks[0].monitor = Monitor::In;

        let insert = song
            .add_device(0, DeviceKind::Reverb)
            .expect("a monitored insert");
        let playing = vec![None; song.tracks.len()];
        for (spec, nodes) in [build(&song, &playing), build_song(&song)] {
            let input = spec
                .iter_ordered()
                .find_map(|(id, node)| matches!(node, NodeSpec::Input { channel: 3 }).then_some(id))
                .expect("monitor IN builds the selected route");
            let effect = nodes
                .devices
                .iter()
                .find_map(|(id, node)| (*id == insert).then_some(*node))
                .expect("the monitored insert is registered");
            let input_gain = song.tracks[0]
                .chain
                .iter()
                .find(|device| device.role == crate::sequencing::DeviceRole::InputGain)
                .and_then(|device| {
                    nodes
                        .devices
                        .iter()
                        .find_map(|(id, node)| (*id == device.id).then_some(*node))
                })
                .expect("the input gain is registered");
            assert!(
                spec.wires().contains(&(input, input_gain)),
                "live input bypassed the insert/strip path"
            );
            assert!(
                spec.iter_ordered().any(|(id, _)| id == effect),
                "the monitored insert left the graph"
            );
            assert!(nodes.outputs[0].is_some());
        }

        song.tracks[0].input = TrackInput::Stereo(4, 5);
        assert_input_count(&song, 2);
        song.tracks[0].kind = TrackKind::Instrument;
        assert_input_count(&song, 0);
    }

    /// Every device that reaches the graph gets a telemetry slot of its
    /// own, in both builders: the strip's IN sections and the desk's
    /// rails alike, no two on one slot.
    #[test]
    fn every_built_device_reports_under_its_own_slot() {
        let mut song = song_with_a_clip();
        // With an instrument on the track, so the case that the letter
        // table is WIDER than the telemetry table is actually covered.
        let voice = song
            .add_device(0, crate::devices::DeviceKind::Kick)
            .expect("a kick");
        for (spec, nodes) in [build(&song, &playing(&song)), build_song(&song)] {
            let _ = spec;
            // Telemetry is the CONSOLE's: every section that reached
            // the graph reports, and nothing else does. The letter
            // table is wider than that — an instrument is addressable
            // so a knob on it can be heard, but has no telemetry — so the
            // two tables are not the same length and never were meant to
            // be. Generated OUT taps live in `param_aliases`; they are not
            // duplicate devices. What must hold is that everything
            // telemetered is addressable, and that no two share a slot.
            let mut addressed: Vec<u64> = nodes.devices.iter().map(|(id, _)| id.0).collect();
            addressed.sort_unstable();
            addressed.dedup();
            for (id, _) in &nodes.telemetry {
                assert!(
                    addressed.contains(&id.0),
                    "a device reports figures no letter can reach"
                );
            }
            assert!(
                nodes.telemetry.len() <= addressed.len(),
                "more reporters than devices"
            );
            // The instrument is the case that makes them differ: a
            // knob on it rides a letter, and it has no figures of its
            // own to report.
            assert!(
                addressed.contains(&voice.0),
                "the instrument is unaddressable"
            );
            assert!(
                !nodes.telemetry.iter().any(|(id, _)| *id == voice),
                "the instrument took a telemetry slot it has nothing to put in"
            );
            let mut slots: Vec<usize> = nodes.telemetry.iter().map(|(_, slot)| *slot).collect();
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(
                slots.len(),
                nodes.telemetry.len(),
                "two devices share a slot"
            );
            let preamp = song
                .section(0, crate::console::SectionKind::Preamp)
                .expect("preamp")
                .id;
            assert!(
                nodes.telemetry.iter().any(|(id, _)| *id == preamp),
                "the preamp is not telemetered"
            );
            let glue = song.console.buses[0]
                .section(crate::console::SectionKind::Glue)
                .expect("glue")
                .id;
            assert!(
                nodes.telemetry.iter().any(|(id, _)| *id == glue),
                "the bus's glue is not telemetered"
            );
        }
    }

    /// The arrangement's stored curve lives in the compiled schedule. The
    /// sustained note is silent for the first half and audible in the second,
    /// and two unrelated render quanta produce the same samples.
    #[test]
    fn stored_automation_shapes_the_offline_arrangement() {
        let mut song = song_with_a_clip();
        song.patterns[0]
            .trig_mut(0)
            .set_primary(Note::new(60, TICKS_PER_BEAT * 4, 100));
        song.tracks[0].insert_point(crate::sequencing::TRACK_VOLUME, 0, 0.0);
        song.tracks[0].insert_point(crate::sequencing::TRACK_VOLUME, TICKS_PER_BEAT * 2, 0.0);
        song.tracks[0].insert_point(crate::sequencing::TRACK_VOLUME, TICKS_PER_BEAT * 9 / 4, 1.0);
        let (spec, _) = build_song(&song);
        let render = |block_frames: usize| {
            let path = std::env::temp_dir().join(format!(
                "daw-song-graph-automated-export-{}-{block_frames}.wav",
                std::process::id()
            ));
            let opts = crate::audio::bounce::BounceOptions {
                sample_rate: 48_000,
                block_frames,
                bpm: 120.0,
                length_beats: 4.0,
                start_beats: 0.0,
                format: crate::audio::bounce::BounceFormat::Float32,
            };
            crate::audio::bounce::bounce_automated(&spec, &opts, &path, |_, _| {}, |_| true)
                .expect("the internally automated arrangement renders");
            let samples: Vec<f32> = hound::WavReader::open(&path)
                .expect("the export exists")
                .samples::<f32>()
                .map(Result::unwrap)
                .collect();
            let _ = std::fs::remove_file(&path);
            samples
        };
        let samples = render(256);
        let odd_quantum = render(113);
        let (first_mismatch, max_delta) = samples.iter().zip(&odd_quantum).enumerate().fold(
            (None, 0.0f32),
            |(first, peak), (index, (a, b))| {
                let delta = (*a - *b).abs();
                (first.or((delta != 0.0).then_some(index)), peak.max(delta))
            },
        );
        assert_eq!(
            samples.len(),
            odd_quantum.len(),
            "the musical range changed with the offline render block size"
        );
        assert!(
            max_delta <= f32::EPSILON,
            "automation changed with the offline render block size: first mismatch {first_mismatch:?}, max delta {max_delta}"
        );
        let rms = |from_seconds: f32, to_seconds: f32| {
            let from = (from_seconds * 48_000.0) as usize * 2;
            let to = ((to_seconds * 48_000.0) as usize * 2).min(samples.len());
            let window = &samples[from.min(to)..to];
            (window.iter().map(|sample| sample * sample).sum::<f32>() / window.len().max(1) as f32)
                .sqrt()
        };
        let held_silent = rms(0.20, 0.80);
        let opened = rms(1.30, 1.80);
        assert!(
            opened > held_silent * 8.0 + 1e-4,
            "the render stayed flat: silent rms {held_silent}, open rms {opened}"
        );
    }

    /// A sampler's authored slices ride into its node spec as the
    /// fractions the device holds.
    #[test]
    fn a_samplers_slices_ride_into_its_spec() {
        let mut song = song_with_a_clip();
        let id = song
            .add_device(0, DeviceKind::Sampler)
            .expect("a sampler on the track");
        song.device_mut(id)
            .expect("the device")
            .set_slices([0.5, 0.25, 0.0]);
        let (spec, _) = build(&song, &playing(&song));
        let slices = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { slices, .. } => Some(slices.clone()),
                _ => None,
            })
            .expect("a sampler node");
        assert_eq!(slices, vec![0.0, 0.25, 0.5]);
    }

    /// A lock on an effect rides into the voice's notes addressed to the
    /// effect's node, and the effect's knob is registered so the lock
    /// can be restored.
    #[test]
    fn an_effect_lock_names_its_node_and_registers_its_knob() {
        use crate::params::sat;
        let mut song = song_with_a_clip();
        let sat_id = song
            .add_device(0, DeviceKind::Sat)
            .expect("an effect on the track");
        song.device_mut(sat_id)
            .expect("device")
            .set(sat::DRIVE, 0.3);
        let knob = song.device(sat_id).expect("device").value(sat::DRIVE);
        song.patterns[0]
            .trig_mut(0)
            .set_lock_on(Some(sat_id), sat::DRIVE, 0.9);
        let (spec, _) = build(&song, &playing(&song));
        let sat_node = spec
            .iter_ordered()
            .find_map(|(id, node)| matches!(node, NodeSpec::Sat { .. }).then_some(id))
            .expect("the effect is in the graph");
        let notes = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Poly { notes, .. } => Some(notes.clone()),
                _ => None,
            })
            .expect("the voice is in the graph");
        assert_eq!(
            notes[0].fx_locks,
            vec![(sat_node.to_bits(), sat::DRIVE, 0.9)]
        );
        assert!(
            notes[0].plocks.is_empty(),
            "an effect lock leaked onto the voice"
        );
        let bases = spec.lock_bases();
        assert!(
            bases.iter().any(|(node, param, base)| *node == sat_node
                && *param == sat::DRIVE
                && (*base - knob).abs() < 1e-6),
            "the knob was not registered: {bases:?}"
        );
    }

    /// A trig's locks ride into the graph verbatim, on every note of
    /// the chord, as the (param, value) pairs the engine applies.
    #[test]
    fn a_trigs_locks_ride_its_notes_into_the_graph() {
        let mut song = song_with_a_clip();
        song.patterns[0].trig_mut(0).set_lock(3, 0.25);
        song.patterns[0].trig_mut(0).set_lock(7, 0.9);
        let pattern = song.patterns[0].clone();
        let notes = notes_of(&song, &pattern, &[]);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].plocks, vec![(3, 0.25), (7, 0.9)]);
    }

    #[test]
    fn a_trigless_lock_reaches_the_graph_without_inventing_a_note() {
        let mut song = song_with_a_clip();
        let trig = song.patterns[0].trig_mut(3);
        trig.clear();
        trig.set_lock(11, 0.42);
        let pattern = song.patterns[0].clone();
        let notes = notes_of(&song, &pattern, &[]);
        let lock_only = notes
            .iter()
            .find(|note| note.vel == 0)
            .expect("lock-only graph event");

        assert_eq!(lock_only.start_beats, beats(3 * PATTERN_STEP_TICKS));
        assert_eq!(lock_only.len_beats, beats(PATTERN_STEP_TICKS));
        assert_eq!(lock_only.plocks, vec![(11, 0.42)]);
        assert_eq!(
            notes.iter().filter(|note| note.vel > 0).count(),
            1,
            "the trigless lock created an audible note"
        );
    }

    /// Put `track`'s clip for scene zero into the session.
    fn also_playing(song: &mut Song, track: usize) {
        let (id, pattern) = (song.tracks[track].id, song.patterns[0].id);
        song.session.scenes[0].slots.push(Slot {
            track: id,
            clip: Clip::Pattern(pattern),
        });
    }

    fn voices(spec: &GraphSpec) -> usize {
        spec.iter_ordered()
            .filter(|(_, node)| matches!(node, NodeSpec::Poly { .. }))
            .count()
    }

    #[test]
    fn a_song_with_nothing_launched_still_runs_and_is_silent() {
        let mut song = song_with_a_clip();
        song.desk_personality.noise_enabled = false;
        let (spec, nodes) = build(&song, &[]);
        assert_eq!(voices(&spec), 0, "something sounded with nothing launched");
        assert!(nodes.outputs.iter().all(Option::is_none));
        spec.compile(48_000, 128)
            .expect("a silent graph must still be runnable");
    }

    #[test]
    fn a_launched_scene_gives_its_track_a_voice_that_compiles() {
        let song = song_with_a_clip();
        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 1);
        assert!(nodes.outputs[0].is_some(), "the track has no output stage");
        assert_eq!(nodes.meters[0], Some(0));
        spec.compile(48_000, 128).expect("the graph must run");
    }

    #[test]
    fn a_muted_track_never_reaches_the_graph() {
        let mut song = song_with_a_clip();
        song.tracks[0].muted = true;
        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 0);
        assert!(nodes.outputs[0].is_none());
        assert!(nodes.meters[0].is_none(), "a silent track kept a meter");
    }

    #[test]
    fn solo_elsewhere_keeps_a_track_out_of_the_graph() {
        let mut song = song_with_a_clip();
        song.add_track(TrackKind::Instrument);
        also_playing(&mut song, 1);
        song.tracks[1].solo = true;

        let (spec, nodes) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 1, "the unsoloed track was still heard");
        assert!(nodes.outputs[0].is_none());
        assert!(nodes.outputs[1].is_some());
    }

    #[test]
    fn a_track_playing_a_scene_it_has_no_clip_in_is_silent() {
        let mut song = song_with_a_clip();
        song.desk_personality.noise_enabled = false;
        // Point the track at a scene that holds nothing for it.
        song.session.scenes[1] = Scene { slots: Vec::new() };
        let (spec, nodes) = build(&song, &[Some(1)]);
        assert_eq!(voices(&spec), 0);
        assert!(nodes.outputs[0].is_none());
    }

    #[test]
    fn the_fader_and_the_pan_ride_one_output_stage() {
        let mut song = song_with_a_clip();
        song.tracks[0].volume = 0.25;
        song.tracks[0].pan = -0.5;
        let (spec, nodes) = build(&song, &playing(&song));
        let out = nodes.outputs[0].expect("a sounding track has an output stage");
        let (_, node) = spec
            .iter_ordered()
            .find(|(id, _)| *id == out)
            .expect("the output stage is in the graph");
        match node {
            NodeSpec::Pan { pan, gain } => {
                assert_eq!(*gain, 0.25, "the fader did not reach the graph");
                assert_eq!(*pan, -0.5, "the pan did not reach the graph");
            }
            other => panic!("the output stage is a {other:?}, not a pan"),
        }
    }

    #[test]
    fn the_master_keeps_the_last_meter_slot_for_itself() {
        let song = song_with_a_clip();
        let (_, nodes) = build(&song, &playing(&song));
        assert_eq!(nodes.meters[0], Some(0), "the track took the master's slot");
        assert_ne!(nodes.meters[0], Some(MASTER_METER));
    }

    #[test]
    fn master_gain_precedes_mix_ceiling_and_final_output_in_both_builders() {
        let mut song = song_with_a_clip();
        song.master = 1.5;
        let first_id = song
            .console
            .mix
            .sections
            .first()
            .expect("MIX has a head")
            .id;
        let ceiling_id = song
            .console
            .mix
            .section(crate::console::SectionKind::Ceiling)
            .expect("MIX has its safety stage")
            .id;
        let last_id = song.console.mix.sections.last().expect("MIX has a tail").id;
        let mix_keys = DeskPathIdentity::Rail(song.console.mix.id).stereo_keys();

        for (spec, nodes) in [build(&song, &playing(&song)), build_song(&song)] {
            let device_node = |id| {
                nodes
                    .devices
                    .iter()
                    .find_map(|(device, node)| (*device == id).then_some(*node))
                    .expect("MIX section reached the graph")
            };
            let first = device_node(first_id);
            let ceiling = device_node(ceiling_id);
            let last = device_node(last_id);
            let mix_personality = spec
                .iter_ordered()
                .find_map(|(id, node)| match node {
                    NodeSpec::DeskPath {
                        left_identity,
                        right_identity,
                        ..
                    } if (*left_identity, *right_identity) == mix_keys => Some(id),
                    _ => None,
                })
                .expect("MIX personality reached the graph");
            let output = spec.output().expect("the graph has a final output");

            assert!(
                spec.wires().contains(&(nodes.master, mix_personality)),
                "master gain was not before MIX"
            );
            assert!(
                spec.wires().contains(&(mix_personality, first)),
                "MIX personality did not precede its sections"
            );
            assert_ne!(output, nodes.master, "master gain remained post-ceiling");
            assert!(
                spec.wires().contains(&(ceiling, last)),
                "MIX safety stage was not before its final scope"
            );
            assert!(
                spec.wires().contains(&(last, output)),
                "final output bypassed the complete MIX run"
            );
            assert!(
                matches!(spec.node(nodes.master), Some(NodeSpec::Mixer { gain }) if *gain == 1.5),
                "SongNodes::master stopped naming the live fader"
            );
        }
    }

    #[test]
    fn adjacent_channels_have_stable_directional_cross_bus_crosstalk() {
        let mut song = song_with_a_clip();
        let blocks = song.tracks[0].blocks.clone();
        song.add_track(TrackKind::Instrument);
        song.tracks[1].blocks = blocks;
        song.tracks[0].bus = 0;
        song.tracks[0].bus_by_hand = true;
        song.tracks[1].bus = 1;
        song.tracks[1].bus_by_hand = true;
        also_playing(&mut song, 1);

        let first = DeskPathIdentity::Track(song.tracks[0].id).stereo_keys();
        let second = DeskPathIdentity::Track(song.tracks[1].id).stereo_keys();
        for (spec, _) in [build(&song, &playing(&song)), build_song(&song)] {
            let paths: Vec<_> = spec
                .iter_ordered()
                .filter_map(|(_, node)| match node {
                    NodeSpec::DeskBleed {
                        from_left,
                        from_right,
                        to_left,
                        to_right,
                        ..
                    } => Some(((*from_left, *from_right), (*to_left, *to_right))),
                    _ => None,
                })
                .collect();
            assert_eq!(
                paths.len(),
                8,
                "one channel pair plus three adjacent bus pairs need both directions"
            );
            assert!(paths.contains(&(first, second)));
            assert!(paths.contains(&(second, first)));
            for pair in song.console.buses.windows(2) {
                let left = DeskPathIdentity::Rail(pair[0].id).stereo_keys();
                let right = DeskPathIdentity::Rail(pair[1].id).stereo_keys();
                assert!(paths.contains(&(left, right)));
                assert!(paths.contains(&(right, left)));
            }
            spec.compile(48_000, 128)
                .expect("the coupled cross-bus graph must run");
        }
    }

    #[test]
    fn dense_bus_and_return_fan_in_is_reduced_before_compile() {
        let mut song = song_with_a_clip();
        let blocks = song.tracks[0].blocks.clone();
        song.tracks[0].bus = 0;
        song.tracks[0].bus_by_hand = true;
        for _ in 1..10 {
            let index = song.tracks.len();
            song.add_track(TrackKind::Instrument);
            song.tracks[index].blocks = blocks.clone();
            song.tracks[index].bus = 0;
            song.tracks[index].bus_by_hand = true;
            also_playing(&mut song, index);
        }

        for (spec, _) in [build(&song, &playing(&song)), build_song(&song)] {
            assert_eq!(
                spec.iter_ordered()
                    .filter(|(_, node)| matches!(node, NodeSpec::DeskBleed { .. }))
                    .count(),
                24,
                "ten channels and four buses need every adjacent pair in both directions"
            );
            spec.compile(48_000, 128)
                .expect("more than eight bus and return feeds need reduction trees");
        }
    }

    fn render_empty_desk(noise_enabled: bool) -> (Vec<f32>, usize) {
        use crate::audio::graph::ProcessCtx;

        let mut song = Song::default();
        song.tracks.clear();
        song.desk_personality.noise_enabled = noise_enabled;
        let (spec, _) = build_song(&song);
        let personalities = spec
            .iter_ordered()
            .filter(|(_, node)| matches!(node, NodeSpec::DeskPath { .. }))
            .count();
        let mut schedule = spec.compile(48_000, 256).expect("desk compiles");
        let input = [0.0f32; 512];
        let mut output = vec![0.0f32; 512];
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        for block in 0..40u64 {
            let ctx = ProcessCtx {
                device_input: &input,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: block * 256,
                beat: block as f64 * 256.0 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            if block == 39 {
                assert_no_alloc::assert_no_alloc(|| schedule.run(&mut output, &ctx));
            } else {
                schedule.run(&mut output, &ctx);
            }
        }
        (output, personalities)
    }

    #[test]
    fn compiled_structural_rails_make_noise_and_global_defeat_is_exact() {
        let (enabled, personalities) = render_empty_desk(true);
        assert_eq!(
            personalities,
            crate::sequencing::BUS_COUNT + crate::sequencing::RETURN_NAMES.len() + 1,
            "every bus, return, and MIX needs one personality node"
        );
        let peak = enabled
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak > 1.0e-8, "enabled desk rendered exact silence");

        let (defeated, defeated_personalities) = render_empty_desk(false);
        assert_eq!(defeated_personalities, personalities);
        assert!(
            defeated.iter().all(|sample| sample.to_bits() == 0),
            "measurement defeat left an additive signal"
        );
    }

    #[test]
    fn compiled_song_personality_is_split_block_bit_exact() {
        use crate::audio::graph::ProcessCtx;

        let mut song = Song::default();
        song.tracks.clear();
        let (whole_spec, _) = build_song(&song);
        let (split_spec, _) = build_song(&song);
        let mut whole = whole_spec.compile(48_000, 256).expect("whole compiles");
        let mut split = split_spec.compile(48_000, 256).expect("split compiles");
        let input = [0.0f32; 512];
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let mut scratch_a = vec![0.0f32; 512];
        let mut scratch_b = vec![0.0f32; 512];

        // Settle graph-level ramps and MIX's latency with identical history.
        for block in 0..40u64 {
            let ctx = ProcessCtx {
                device_input: &input,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: block * 256,
                beat: block as f64 * 256.0 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            whole.run(&mut scratch_a, &ctx);
            split.run(&mut scratch_b, &ctx);
        }
        assert_eq!(scratch_a, scratch_b, "identical schedules already diverged");

        let position = 40 * 256;
        let mut whole_out = vec![0.0f32; 512];
        let mut split_out = vec![0.0f32; 512];
        let whole_ctx = ProcessCtx {
            device_input: &input,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position,
            beat: position as f64 * beats_per_sample,
            beats_per_sample,
            discontinuity: false,
        };
        whole.run(&mut whole_out, &whole_ctx);

        let first = ProcessCtx {
            device_input: &input,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 97,
            playing: true,
            position,
            beat: position as f64 * beats_per_sample,
            beats_per_sample,
            discontinuity: false,
        };
        split.run(&mut split_out, &first);
        let rest_position = position + 97;
        let rest = ProcessCtx {
            device_input: &input,
            in_channels: 2,
            block_frames: 256,
            offset: 97,
            len: 159,
            playing: true,
            position: rest_position,
            beat: rest_position as f64 * beats_per_sample,
            beats_per_sample,
            discontinuity: false,
        };
        split.run(&mut split_out, &rest);
        assert_eq!(whole_out, split_out);
    }

    #[test]
    fn every_compiled_channel_and_rail_gets_stable_stereo_identity() {
        let song = song_with_a_clip();
        let (spec, _) = build_song(&song);
        let paths: Vec<(u64, u64)> = spec
            .iter_ordered()
            .filter_map(|(_, node)| match node {
                NodeSpec::DeskPath {
                    left_identity,
                    right_identity,
                    ..
                } => Some((*left_identity, *right_identity)),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths.len(),
            song.tracks.len()
                + crate::sequencing::BUS_COUNT
                + crate::sequencing::RETURN_NAMES.len()
                + 1
        );
        let expected_track = DeskPathIdentity::Track(song.tracks[0].id).stereo_keys();
        assert!(paths.contains(&expected_track));
        assert!(paths.iter().all(|(left, right)| left != right));
    }

    /// The one that matters: a song, a launched scene, and actual sound
    /// out of the schedule the engine would be handed.
    ///
    /// Every other test here checks a claim about the graph's SHAPE. This
    /// one runs it, because a graph that compiles and is silent would
    /// pass all of them — and silence with no explanation is the failure
    /// this project spends the most effort avoiding.
    #[test]
    fn a_launched_scene_actually_makes_sound() {
        use crate::audio::graph::ProcessCtx;

        let song = song_with_a_clip();
        let (spec, _) = build(&song, &playing(&song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");

        // Two seconds of blocks at 120bpm, rolling from the top. The
        // first block carries the discontinuity, as a first play does.
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut loudest = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            let ctx = ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (block * 256) as u64,
                beat: (block * 256) as f64 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            schedule.run(&mut out, &ctx);
            for sample in &out {
                loudest = loudest.max(sample.abs());
            }
        }
        assert!(
            loudest > 0.01,
            "the graph compiled and stayed silent (loudest {loudest})"
        );
        assert!(
            loudest <= 1.5,
            "the graph is not an amplitude any more ({loudest})"
        );
    }

    /// And the other half of the same claim: a track the mixer says is
    /// silent IS silent, all the way through the engine rather than only
    /// in the shape of the graph.
    #[test]
    fn a_muted_track_makes_no_sound_at_all() {
        use crate::audio::graph::ProcessCtx;

        let mut song = song_with_a_clip();
        song.tracks[0].muted = true;
        // Exact-silence assertions are measurements. Structural buses remain
        // physical paths even when a channel is muted, so use the persisted
        // global measurement defeat rather than confusing a floor with leak.
        song.desk_personality.noise_enabled = false;
        let (spec, _) = build(&song, &playing(&song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");

        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut loudest = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            let ctx = ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (block * 256) as u64,
                beat: (block * 256) as f64 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            schedule.run(&mut out, &ctx);
            for sample in &out {
                loudest = loudest.max(sample.abs());
            }
        }
        assert_eq!(loudest, 0.0, "a muted track was heard");
    }

    #[test]
    fn the_chains_head_is_the_voice_the_track_sounds() {
        let mut song = song_with_a_clip();
        // The default: no chain, the default voice.
        let (spec, _) = build(&song, &playing(&song));
        assert!(
            spec.iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "an empty chain did not sound the default voice"
        );

        // Choose another instrument and the track sounds THAT.
        song.add_device(0, DeviceKind::Haze).expect("an instrument");
        let (spec, _) = build(&song, &playing(&song));
        assert!(
            spec.iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Haze { .. })),
            "the chosen instrument did not reach the graph"
        );
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "the default voice sounded alongside the chosen one"
        );
    }

    #[test]
    fn a_devices_settings_reach_the_voice() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Poly).expect("an instrument");
        let gain = crate::params::poly::GAIN;
        let quiet = {
            let device = song.device_mut(id).expect("there");
            assert!(device.set(gain, 0.2));
            device.value(gain)
        };

        let (spec, _) = build(&song, &playing(&song));
        let params = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Poly { params, .. } => Some(*params),
                _ => None,
            })
            .expect("the voice is in the graph");
        assert_eq!(
            params.gain, quiet,
            "an edited parameter did not reach the engine's patch"
        );
    }

    #[test]
    fn a_sampler_plays_the_file_its_device_carries() {
        let mut song = song_with_a_clip();
        let id = song
            .add_device(0, DeviceKind::Sampler)
            .expect("an instrument");

        // No file yet: a silent sampler, not a refused graph and not the
        // default voice standing in.
        let (spec, nodes) = build(&song, &playing(&song));
        let path = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { path, .. } => Some(path.clone()),
                _ => None,
            })
            .expect("the sampler is in the graph");
        assert!(path.as_os_str().is_empty());
        assert!(
            nodes.outputs[0].is_some(),
            "a sampler with no file lost its output"
        );

        let device = song.device_mut(id).expect("there");
        device.sample = Some("/kits/break.wav".into());
        assert!(device.set(crate::params::sampler::START, 0.25));
        let (spec, _) = build(&song, &playing(&song));
        let (path, params) = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sampler { path, params, .. } => Some((path.clone(), *params)),
                _ => None,
            })
            .expect("the sampler is in the graph");
        assert_eq!(path, std::path::PathBuf::from("/kits/break.wav"));
        assert_eq!(
            params.start, 0.25,
            "an edit did not reach the sampler's patch"
        );
    }

    #[test]
    fn a_bypassed_instrument_is_silence_and_not_a_substitute() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Haze).expect("an instrument");
        song.device_mut(id).expect("there").bypassed = true;

        let (spec, nodes) = build(&song, &playing(&song));
        assert!(
            nodes.outputs[0].is_none(),
            "a bypassed voice still had an output"
        );
        assert_eq!(voices(&spec), 0);
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Poly { .. })),
            "bypassing an instrument quietly substituted the default one"
        );
        spec.compile(48_000, 128)
            .expect("a silent graph must still run");
    }

    /// Render one graph and report the loudest sample it produced.
    fn loudest(song: &Song) -> f32 {
        use crate::audio::graph::ProcessCtx;
        let (spec, _) = build(song, &playing(song));
        let mut schedule = spec.compile(48_000, 256).expect("the graph must run");
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let silence = [0.0f32; 512];
        let mut peak = 0.0f32;
        for block in 0..64 {
            let mut out = vec![0.0f32; 512];
            schedule.run(
                &mut out,
                &ProcessCtx {
                    device_input: &silence,
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (block * 256) as u64,
                    beat: (block * 256) as f64 * beats_per_sample,
                    beats_per_sample,
                    discontinuity: block == 0,
                },
            );
            for sample in &out {
                peak = peak.max(sample.abs());
            }
        }
        peak
    }

    #[test]
    fn effects_reach_the_graph_in_signal_order_and_it_still_runs() {
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        song.add_device(0, DeviceKind::Reverb).expect("effect");
        song.add_device(0, DeviceKind::Sat).expect("effect");

        let (spec, _) = build(&song, &playing(&song));
        let order: Vec<&str> = spec
            .iter_ordered()
            .filter_map(|(_, node)| match node {
                NodeSpec::Poly { .. } => Some("poly"),
                NodeSpec::Reverb { .. } => Some("reverb"),
                NodeSpec::Sat { .. } => Some("sat"),
                _ => None,
            })
            .collect();
        assert_eq!(
            order,
            ["poly", "reverb", "sat"],
            "the chain did not reach the graph in the order it was built"
        );
        assert!(loudest(&song) > 0.01, "a chained track went silent");
    }

    #[test]
    fn a_bypassed_effect_is_dry_but_keeps_its_latency() {
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        let sat = song.add_device(0, DeviceKind::Sat).expect("effect");

        let active_latency = build(&song, &playing(&song))
            .0
            .compile(48_000, 256)
            .expect("active graph")
            .latency();
        song.device_mut(sat).expect("there").bypassed = true;

        let (spec, _) = build(&song, &playing(&song));
        assert!(
            !spec
                .iter_ordered()
                .any(|(_, node)| matches!(node, NodeSpec::Sat { .. })),
            "a bypassed effect was still built"
        );
        assert!(
            spec.iter_ordered().any(|(_, node)| matches!(
                node,
                NodeSpec::LatencyBypass { effect }
                    if matches!(effect.as_ref(), NodeSpec::Sat { .. })
            )),
            "the bypass lost the effect's latency declaration"
        );
        let bypassed_latency = spec.compile(48_000, 256).expect("bypassed graph").latency();
        assert_eq!(
            bypassed_latency, active_latency,
            "bypassing the saturator moved the channel"
        );
        // And the signal still arrives: bypass is a pass, not a cut.
        assert!(
            loudest(&song) > 0.01,
            "bypassing an effect silenced the track"
        );
    }

    #[test]
    fn an_effects_settings_reach_its_node() {
        let mut song = song_with_a_clip();
        let id = song.add_device(0, DeviceKind::Sat).expect("effect");
        let device = song.device_mut(id).expect("there");
        assert!(device.set(crate::params::sat::DRIVE, 9.0));
        assert!(device.set(crate::params::sat::MODE, 3.0));
        let (drive, mode) = (
            device.value(crate::params::sat::DRIVE),
            device.value(crate::params::sat::MODE),
        );

        let (spec, _) = build(&song, &playing(&song));
        let built = spec
            .iter_ordered()
            .find_map(|(_, node)| match node {
                NodeSpec::Sat { drive, mode, .. } => Some((*drive, *mode)),
                _ => None,
            })
            .expect("the effect is in the graph");
        assert_eq!(built.0, drive, "an edited value did not reach the node");
        assert_eq!(
            built.1,
            mode.round() as u32,
            "a choice did not survive becoming an index"
        );
    }

    #[test]
    fn an_effect_shapes_what_it_is_given() {
        // The claim a chain exists to make: the same notes through a
        // different chain are a different sound. A closed output trim is
        // the least ambiguous shape an effect can impose.
        let mut song = song_with_a_clip();
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        let bare = loudest(&song);
        assert!(bare > 0.01, "the bare track was silent to begin with");

        let sat = song.add_device(0, DeviceKind::Sat).expect("effect");
        let device = song.device_mut(sat).expect("there");
        assert!(device.set(crate::params::sat::OUT, 0.0));
        assert!(device.set(crate::params::sat::MIX, 1.0));
        assert!(
            loudest(&song) < bare * 0.5,
            "the effect made no difference to the sound"
        );
    }

    #[test]
    fn every_effect_kind_this_song_can_hold_actually_builds() {
        // A device the model accepts and the compiler silently drops is
        // a chain that lies. Rack is the one absent kind, and it says so.
        use crate::devices::DEVICES;
        let mut missing = Vec::new();
        for spec in DEVICES.iter().filter(|spec| !spec.instrument) {
            let mut song = Song::default();
            let Some(id) = song.add_device(0, spec.kind) else {
                continue;
            };
            let device = song.device(id).expect("there");
            if effect_of(device).is_none() {
                missing.push(spec.name);
            }
        }
        assert_eq!(
            missing,
            ["rack"],
            "effects the chain accepts but cannot build"
        );
    }

    #[test]
    fn an_empty_trig_contributes_no_note() {
        let mut song = song_with_a_clip();
        song.patterns[0].clear(0);
        let (spec, _) = build(&song, &playing(&song));
        assert_eq!(voices(&spec), 0, "a pattern of rests still made a voice");
    }

    #[test]
    fn a_muted_note_is_not_played_while_its_trig_still_is() {
        let mut song = song_with_a_clip();
        song.patterns[0].toggle(4, Note::new(64, PATTERN_STEP_TICKS, 100));
        let heard = |song: &Song| {
            let (spec, _) = build(song, &playing(song));
            spec.iter_ordered()
                .find_map(|(_, node)| match node {
                    NodeSpec::Poly { notes, .. } => Some(notes.len()),
                    _ => None,
                })
                .unwrap_or(0)
        };
        assert_eq!(heard(&song), 2);
        song.patterns[0].trig_mut(4).notes[0].muted = true;
        assert_eq!(heard(&song), 1, "a muted note was played anyway");
    }
}
