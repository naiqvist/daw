//! What a track IS: its chain, its mixer values, its automation.
//!
//! `Track` is the arrangement's unit of everything-but-the-clips — one
//! lane's devices, level, pan, mute and solo — and `MasterTrack` is the
//! one every lane sums into. `TrackWire` is the on-disk shape, kept apart
//! so the hand-written `Deserialize` can migrate a v1 file's single
//! instrument and effect into a chain.
//!
//! Lifted out of `main.rs` unchanged.

use crate::TRACK_H;
use crate::automation::TrackAutomation;
use crate::device_state::{DeviceInstance, DeviceState, ReverbParams, unit_zoom};
use crate::devices::DeviceKind;
use crate::targets::{TRACK_PAN_TARGET, TRACK_VOLUME_TARGET};
use daw::audio::graph::SynthParams;
use daw::ui::device;
use daw::ui::vm::TrackKind;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Track {
    /// What the lane carries. Fixed at creation: changing a track's kind
    /// would change what every clip on it means, which is a conversion,
    /// not a toggle.
    pub kind: TrackKind,
    /// What the header shows and what a mixer strip will show later. Not
    /// unique — two tracks may share a name, exactly as two files in
    /// different folders may.
    pub name: String,
    pub height: f32,
    /// Silenced. A muted track leaves the SCHEDULE rather than being
    /// multiplied by zero: the graph should be as small as what is
    /// actually sounding.
    pub mute: bool,
    /// Soloed. While any track is soloed, only soloed tracks are wired —
    /// solo-in-place, the meaning every DAW agrees on.
    pub solo: bool,
    /// Constant-power pan, `-1..=1`. Center is 0.0, and it is exact: the
    /// knob snaps there so "back to the middle" is reachable by hand.
    pub pan: f32,
    /// Fader level as LINEAR amplitude, 1.0 = unity. Stored linear because
    /// that is what the engine multiplies by; the fader does the dB
    /// mapping, which is the only place the curve belongs.
    pub volume: f32,
    /// This lane is a GROUP: it carries no clips and no instrument, and
    /// its source is the sum of the lanes nested under it.
    #[serde(default)]
    pub is_group: bool,
    /// A group drawn CLOSED: its members are hidden from both views.
    ///
    /// Saved with the project rather than kept as machine-local view
    /// state, and that is the deliberate choice: which parts of a mix
    /// are put away is a fact about the arrangement — the drums are
    /// finished, so they are closed — and it should survive being
    /// emailed to someone else along with the song.
    ///
    /// A folded group still SOUNDS. Hiding is not muting, and a lane
    /// that fell silent because it was tidied away would be the worst
    /// bug this feature could have.
    #[serde(default)]
    pub folded: bool,
    /// How deeply nested in the stack. Zero is top level; a lane at
    /// depth `d` belongs to the nearest group above it at depth `d - 1`.
    ///
    /// POSITION, not a pointer. A group and its members are contiguous —
    /// the model every desk with groups uses — so membership is readable
    /// off the stack and cannot go stale when a lane moves. The cost is
    /// that a lane cannot belong to a group it is not next to, which is
    /// also true of the thing it models.
    #[serde(default)]
    pub depth: u8,
    /// Where this lane's LIVE signal comes from, beside its clips.
    #[serde(default)]
    pub input: TrackInput,
    /// Whether that live signal is HEARD.
    #[serde(default)]
    pub monitor: Monitor,
    /// Armed to RECORD. What decides whether a rolling transport writes
    /// this lane's input to a file — and, under `Monitor::Auto`, whether
    /// it is heard at all.
    ///
    /// Not saved with the project. An arm is a thing you are doing right
    /// now, and a song that opened with three lanes live and listening
    /// would be a song that could start recording over itself before
    /// anybody looked at it.
    #[serde(skip)]
    pub armed: bool,
    /// How much of this track each RETURN gets, as linear gain, indexed
    /// by return. Shorter than the return list is normal and means zero:
    /// adding a return must not have to walk every track to write a
    /// silence into it.
    ///
    /// POST-FADER, decided at the graph and not here — see the send
    /// nodes in the graph builder. What lives here is only how much.
    #[serde(default)]
    pub sends: Vec<f32>,
    /// Persistent track envelopes. Their values are applied live to the
    /// output node, so drawing a curve never requires a graph swap.
    pub automation: TrackAutomation,
    /// The devices on this lane, in SIGNAL order: at most one instrument,
    /// at the head, then its effects. Empty is a real state, not a
    /// placeholder — an empty track compiles to no nodes at all and is
    /// silent. Loading a device from the browser is what gives a track a
    /// voice.
    pub chain: Vec<DeviceInstance>,
    /// The FILE and slice table of every sampler on this lane, keyed by
    /// device instance id.
    ///
    /// Beside the devices rather than inside `DeviceState`, and that is a
    /// deliberate trade: `DeviceState` is `Copy` — one stored copy of one
    /// truth, cheap to pass by value, and half this file passes it by
    /// value — while a path and a slice list are neither. Putting them in
    /// the enum would cost `Copy` everywhere for the sake of one device.
    ///
    /// Keyed by instance id rather than by position, for the reason
    /// automation is: reordering a chain must not repoint a sample.
    #[serde(default)]
    pub sampler_sources: std::collections::BTreeMap<u64, SamplerSource>,
    /// Each rack's name and macros, keyed by the rack instance's id.
    ///
    /// Beside the chain rather than inside the instance, for the reason
    /// `sampler_sources` is: the half of a device that its own parameters
    /// cannot supply. Here that half is a name and eight assignments,
    /// neither of which is `Copy`, and `DeviceInstance` is.
    ///
    /// Keyed by id and not by position, also for `sampler_sources`'
    /// reason: reordering a chain must not repoint a macro.
    #[serde(default)]
    pub racks: std::collections::BTreeMap<u64, device::RackUi>,
}

/// Where a lane's LIVE signal comes from, beside its clips.
///
/// AUDIO ONLY, deliberately. The engine's input node reads device audio
/// channels and there is no MIDI input path at all, so a note lane would
/// be offered a control that could not work. Saying nothing is kinder
/// than a routing menu whose every entry is silence.
///
/// Channels are stored as INDICES and not clamped on load, because the
/// number of them belongs to whatever interface is plugged in today. A
/// channel that is not there reads as silence at the node — see
/// `Node::Input` — so a project written on an eight-in desk opens on a
/// laptop quiet rather than wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TrackInput {
    /// Nothing. The lane is its clips and only its clips.
    #[default]
    None,
    /// One channel, centred. The engine's input node is one channel
    /// wide, so this is the shape everything else is built from.
    Mono(u32),
    /// A pair, hard left and hard right.
    Stereo(u32, u32),
}

impl TrackInput {
    /// The label a strip shows: `—`, `1`, `1/2`.
    ///
    /// One-based, because a musician counts inputs from one and the
    /// engine counts them from zero, and exactly one of those is the
    /// place to do the arithmetic.
    pub fn label(self) -> String {
        match self {
            Self::None => "—".to_owned(),
            Self::Mono(channel) => format!("{}", channel + 1),
            Self::Stereo(left, right) => format!("{}/{}", left + 1, right + 1),
        }
    }

    /// Every route an interface with `channels` inputs can offer, in the
    /// order a click cycles through them: nothing, each channel alone,
    /// then each adjacent pair.
    ///
    /// Mono before stereo because a single input is the common case —
    /// one microphone, one instrument — and the pairs are what a stereo
    /// synth or a pair of overheads wants.
    pub fn routes(channels: u32) -> Vec<Self> {
        let mut out = vec![Self::None];
        out.extend((0..channels).map(Self::Mono));
        out.extend(
            (0..channels.saturating_sub(1))
                .step_by(2)
                .map(|left| Self::Stereo(left, left + 1)),
        );
        out
    }

    /// The next route after this one, wrapping. `back` walks the other
    /// way, which is what makes a cycle usable rather than a hunt.
    pub fn cycled(self, channels: u32, back: bool) -> Self {
        let routes = Self::routes(channels);
        let at = routes.iter().position(|route| *route == self).unwrap_or(0);
        let step = if back { routes.len() - 1 } else { 1 };
        routes[(at + step) % routes.len()]
    }
}

/// Whether a routed input is HEARD.
///
/// `Off` is the default, and that is a SAFETY default rather than a
/// tidiness one: an input wired to the speakers is a feedback loop on a
/// laptop, so choosing a source must not by itself make a noise.
///
/// `Auto` is the one that makes arming a single gesture — arm the lane
/// and you hear what you are about to record, disarm it and the room
/// goes quiet again without a second switch to remember.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Monitor {
    #[default]
    Off,
    /// Heard whenever there is a route, armed or not.
    In,
    /// Heard while the lane is armed.
    Auto,
}

impl Monitor {
    /// Does this lane hear its input right now?
    ///
    /// Takes the arm rather than reading it from a track, so the rule
    /// stays a property of the three states and can be tested without
    /// building a lane around it.
    pub fn hears(self, armed: bool) -> bool {
        match self {
            Self::Off => false,
            Self::In => true,
            Self::Auto => armed,
        }
    }

    /// The next state, wrapping. Off, in, auto — quietest first, so a
    /// press from the default is always toward hearing something.
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::In,
            Self::In => Self::Auto,
            Self::Auto => Self::Off,
        }
    }

    /// The two characters the button shows.
    ///
    /// THREE STATES, THREE SYMBOLS. `Off` and `In` used to share `IN`
    /// and leave the button's fill to carry the difference — which made
    /// the fill load-bearing for two states while `Auto` carried its own
    /// glyph, so the alphabet said one thing and the paint said another.
    /// A reader glancing at a dim strip could not tell a lane that was
    /// off from one that was listening.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "--",
            Self::In => "IN",
            Self::Auto => "AU",
        }
    }
}

#[cfg(test)]
mod monitor_tests {
    use super::*;

    /// THE THREE STATES, AND THE ONE THAT READS THE ARM.
    ///
    /// `Auto` is what makes arming a single gesture — arm the lane and
    /// you hear what you are about to record, disarm it and the room
    /// goes quiet without a second switch to remember. `Off` staying
    /// silent under an arm is the safety half of the same rule.
    #[test]
    fn a_monitor_hears_by_state_and_by_arm() {
        assert!(!Monitor::Off.hears(false));
        assert!(!Monitor::Off.hears(true), "off means off, armed or not");
        assert!(Monitor::In.hears(false), "in means in, armed or not");
        assert!(Monitor::In.hears(true));
        assert!(!Monitor::Auto.hears(false));
        assert!(Monitor::Auto.hears(true));
    }

    /// The cycle starts quiet and comes back to quiet, so a press from
    /// the default is always toward hearing something.
    #[test]
    fn the_monitor_cycle_starts_and_ends_silent() {
        assert_eq!(Monitor::default(), Monitor::Off);
        let mut seen = Vec::new();
        let mut state = Monitor::default();
        for _ in 0..3 {
            state = state.cycled();
            seen.push(state);
        }
        assert_eq!(seen, vec![Monitor::In, Monitor::Auto, Monitor::Off]);
    }
}

/// A sampler's file and the slices cut from it.
///
/// Green-zone document data. The path persists; the decoded audio never
/// does — it is rebuilt at compile from the path, through the loader's
/// cache.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SamplerSource {
    pub path: std::path::PathBuf,
    /// Boundaries in SOURCE frames, sorted, first is always 0. Empty
    /// means "not authored yet", which compiles to the grid the `slices`
    /// knob asks for.
    pub slices: Vec<u64>,
}

impl Track {
    /// A fresh track of `kind`, named. The one door new tracks walk
    /// through, so the default session and Ctrl+T build the same thing.
    pub fn new(kind: TrackKind, name: String) -> Self {
        Self {
            kind,
            name,
            ..Self::default()
        }
    }

    /// A fresh GROUP lane. Audio-kinded because it holds no notes and
    /// takes no instrument, which is what that kind already means here.
    pub fn group(name: String) -> Self {
        Self {
            is_group: true,
            ..Self::new(TrackKind::Audio, name)
        }
    }

    /// The device with this instance id, if this track carries it.
    pub fn device(&self, id: u64) -> Option<&DeviceInstance> {
        self.chain.iter().find(|instance| instance.id == id)
    }

    pub fn device_mut(&mut self, id: u64) -> Option<&mut DeviceInstance> {
        self.chain.iter_mut().find(|instance| instance.id == id)
    }

    /// The instrument at the head of the chain, if there is one.
    pub fn instrument(&self) -> Option<&DeviceInstance> {
        self.chain
            .first()
            .filter(|head| head.kind().is_instrument())
    }

    /// Put a device on this lane, honouring the ordering rule: AT MOST ONE
    /// instrument and it heads the chain, effects following in the order
    /// they were added. Returns the id of the instrument it displaced, if
    /// any — its wires are dangling pointers and the caller must drop them.
    /// Put every top-level device on this track inside a new rack.
    ///
    /// Ableton's Ctrl+G, and the same shape: the devices keep their order
    /// and their settings, and a container appears around them. Because
    /// nesting is a pointer UPWARD, grouping is one field per device and
    /// one instance appended — nothing moves.
    ///
    /// Returns the new rack's id, or `None` when there was nothing to
    /// group. Racks do not nest yet, so a chain that is already one rack
    /// declines rather than wrapping itself again.
    /// Wrap every LOOSE device in a new rack — the whole chain, less
    /// anything already inside one.
    ///
    /// The convenience the chain-wide verb had before a selection
    /// existed, and still the right answer for a chain of three: picking
    /// all of them first would be ceremony.
    pub fn group_into_rack(&mut self, id: u64, name: &str) -> Option<u64> {
        let loose: Vec<u64> = self
            .chain
            .iter()
            .filter(|device| device.parent.is_none())
            .map(|device| device.id)
            .collect();
        self.group_devices_into_rack(id, name, &loose)
    }

    /// Wrap the devices in `chosen` in a new rack.
    ///
    /// The CHOSEN ones, which is what makes a rack a decision rather than
    /// a fact about the whole chain: two of five devices is the ordinary
    /// case — a filter and a delay that belong together while the
    /// compressor after them does not.
    ///
    /// Devices that are already inside a rack are refused rather than
    /// stolen: a device has one parent, and pulling one out of its rack
    /// by grouping it elsewhere would empty that rack from a gesture that
    /// never mentioned it. A rack itself is refused for the same reason
    /// nesting is not built yet.
    ///
    /// Order is the CHAIN's, never the order they were picked in — the
    /// chain is signal order, and a rack whose contents were arranged by
    /// the sequence of clicks would sound different depending on how it
    /// was selected.
    pub fn group_devices_into_rack(&mut self, id: u64, name: &str, chosen: &[u64]) -> Option<u64> {
        let loose: Vec<u64> = self
            .chain
            .iter()
            .filter(|d| {
                chosen.contains(&d.id)
                    && d.parent.is_none()
                    && !matches!(d.state, DeviceState::Rack)
            })
            .map(|d| d.id)
            .collect();
        if loose.is_empty() {
            return None;
        }
        for device in self.chain.iter_mut() {
            if loose.contains(&device.id) {
                device.parent = Some(id);
            }
        }
        self.chain.push(DeviceInstance {
            id,
            parent: None,
            state: DeviceState::Rack,
            bypass: false,
            page: 0,
            view_zoom: unit_zoom(),
            view_scroll: 0.0,
        });
        self.racks.insert(
            id,
            device::RackUi {
                name: name.to_owned(),
                ..device::RackUi::default()
            },
        );
        Some(id)
    }

    /// Take a rack apart, leaving its devices where they were.
    ///
    /// Ableton's Ctrl+Shift+G. The children's order is untouched — they
    /// were never moved to begin with — so ungrouping is the exact
    /// inverse of grouping and cannot reshuffle a chain.
    pub fn ungroup_rack(&mut self, id: u64) -> bool {
        if !self
            .chain
            .iter()
            .any(|d| d.id == id && matches!(d.state, DeviceState::Rack))
        {
            return false;
        }
        for device in self.chain.iter_mut() {
            if device.parent == Some(id) {
                device.parent = None;
            }
        }
        self.chain.retain(|d| d.id != id);
        self.racks.remove(&id);
        true
    }

    pub fn insert_device(&mut self, instance: DeviceInstance) -> Option<u64> {
        if !instance.kind().is_instrument() {
            self.chain.push(instance);
            return None;
        }
        let displaced = self.instrument().map(|old| old.id);
        // Anything else calling itself an instrument goes too: the rule is
        // one, not one at the head and others hiding behind it.
        self.chain.retain(|other| !other.kind().is_instrument());
        self.chain.insert(0, instance);
        displaced
    }
}

/// The bus every track lands on: one fader, one pan, one chain of effects,
/// and the last thing the speakers hear.
///
/// A separate type rather than another [`Track`] in the stack, and that is
/// the whole design. A master carries no clips, no kind, no mute and no
/// solo — a muted master is just a fader at the bottom — and above all it
/// has no INDEX. Half this file addresses a track by its position, and a
/// master that could be at position 3 would put a bus in the middle of the
/// song. Giving it its own field keeps every one of those loops honest.
///
/// An instrument dropped here is refused rather than silently placed: the
/// master has no notes to give it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MasterTrack {
    /// Fader level as LINEAR amplitude, exactly as a track's is.
    pub volume: f32,
    /// Constant-power pan over the whole mix. Rarely moved, and present
    /// for the same reason a console's master pan is: it exists.
    pub pan: f32,
    /// The master chain, in signal order. Effects only.
    pub chain: Vec<DeviceInstance>,
    /// Each rack's name and macros, keyed by instance id — the master
    /// carries a chain, so it can carry racks in it.
    #[serde(default)]
    pub racks: std::collections::BTreeMap<u64, device::RackUi>,
}

impl Default for MasterTrack {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            chain: Vec::new(),
            racks: std::collections::BTreeMap::new(),
        }
    }
}

impl MasterTrack {
    /// The name every surface shows. Not a field: renaming the master
    /// would only ever make a mix harder to read out loud.
    pub const NAME: &'static str = "MASTER";

    pub fn device_mut(&mut self, id: u64) -> Option<&mut DeviceInstance> {
        self.chain.iter_mut().find(|instance| instance.id == id)
    }

    /// Put an effect on the master. Instruments are refused — there is
    /// nothing here for one to play — and the caller is told so it can say
    /// why rather than dropping the device into silence.
    pub fn insert_device(&mut self, instance: DeviceInstance) -> bool {
        if instance.kind().is_instrument() {
            return false;
        }
        self.chain.push(instance);
        true
    }
}

// ------------------------------------------------------------- groups ---

/// How deeply groups may nest. Eight, which is more than a mix has ever
/// needed and small enough that the recursive walks below cannot run
/// away on a hand-edited file.
pub const MAX_GROUP_DEPTH: u8 = 8;

/// The lanes nested under the group at `index`.
///
/// Empty for a lane that is not a group, and empty for a group nobody
/// has put anything in yet — which is a real state, not a broken one: a
/// group is made before it is filled.
pub fn group_members(tracks: &[Track], index: usize) -> std::ops::Range<usize> {
    let Some(group) = tracks.get(index).filter(|track| track.is_group) else {
        return index..index;
    };
    let mut end = index + 1;
    while tracks
        .get(end)
        .is_some_and(|track| track.depth > group.depth)
    {
        end += 1;
    }
    (index + 1)..end
}

/// The group `index` belongs to, if any.
///
/// The nearest lane ABOVE it that is a group one level shallower. The
/// nesting rule `sanitize_nesting` enforces guarantees there is exactly
/// one, or none at depth zero.
pub fn parent_group(tracks: &[Track], index: usize) -> Option<usize> {
    let depth = tracks.get(index)?.depth;
    if depth == 0 {
        return None;
    }
    tracks[..index]
        .iter()
        .rposition(|track| track.is_group && track.depth + 1 == depth)
}

/// Silenced by its own switch, or by a group above it.
///
/// A group's mute is the whole point of a group: one switch that takes
/// the drums out, however many lanes the drums are.
pub fn muted_in_place(tracks: &[Track], index: usize) -> bool {
    let mut at = index;
    loop {
        if tracks.get(at).is_some_and(|track| track.mute) {
            return true;
        }
        match parent_group(tracks, at) {
            Some(parent) => at = parent,
            None => return false,
        }
    }
}

/// Whether this lane belongs in a solo that is running somewhere.
///
/// Three ways in: it is soloed itself; a group ABOVE it is soloed, since
/// soloing the drums means hearing the drums; or it is a group holding
/// something soloed, since the soloed lane's signal has to get out
/// through the bus it lives on.
pub fn solo_in_scope(tracks: &[Track], index: usize) -> bool {
    let Some(track) = tracks.get(index) else {
        return false;
    };
    if track.solo {
        return true;
    }
    let mut at = index;
    while let Some(parent) = parent_group(tracks, at) {
        if tracks[parent].solo {
            return true;
        }
        at = parent;
    }
    group_members(tracks, index).any(|member| solo_in_scope(tracks, member))
}

/// Is this lane inside a folded group?
///
/// Walks the stack rather than the parent chain, because a fold hides a
/// contiguous RUN: everything under the folded lane until the stack
/// comes back up to its level. Reading it forwards means one pass says
/// the answer for every lane, which is what the layouts want.
///
/// Nested folds need no special case — once a run is hiding, a folded
/// group inside it is hidden along with everything it holds.
pub fn hidden_by_fold(tracks: &[Track], index: usize) -> bool {
    let mut hiding: Option<u8> = None;
    for (at, track) in tracks.iter().enumerate() {
        if hiding.is_some_and(|depth| track.depth <= depth) {
            hiding = None;
        }
        if at == index {
            return hiding.is_some();
        }
        if hiding.is_none() && track.is_group && track.folded {
            hiding = Some(track.depth);
        }
    }
    false
}

/// Force the nesting rule onto a stack that came from a FILE.
///
/// The rule is one line: a lane may sit one level deeper than what came
/// before it allows, and no deeper. A lane whose depth outruns that has
/// no group above it to belong to, and `parent_group` would answer
/// `None` for a lane the stack claims is nested — so the depth is
/// pulled back to something the stack can actually mean.
pub fn sanitize_nesting(tracks: &mut [Track]) {
    let mut allowed = 0;
    for track in tracks.iter_mut() {
        track.depth = track.depth.min(allowed).min(MAX_GROUP_DEPTH);
        // A lane that is not a group has nothing to fold, and a stray
        // flag on one would hide the lanes after it forever.
        track.folded &= track.is_group;
        allowed = if track.is_group {
            track.depth.saturating_add(1)
        } else {
            track.depth
        };
    }
}

/// A RETURN: a bus every track can feed, which lands on the master.
///
/// Shaped like [`MasterTrack`] rather than like [`Track`], because that
/// is what it is — a chain, a fader, a pan, and no clips of its own.
/// What it adds over the master is a NAME, since a project has one
/// master and may have several returns, and a MUTE, since a reverb is a
/// thing you switch off to hear what is underneath it.
///
/// # A return does not send
///
/// Deliberately, and it is the one place this differs from a console.
/// Returns feeding returns is how a desk makes a feedback loop, and this
/// graph has no cycle detector — the schedule is a flat topological
/// order compiled once, so a cycle is not a howl, it is a graph that
/// will not compile. The vocabulary simply does not contain the move.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ReturnTrack {
    /// What the strip shows. Beside the letter, not instead of it: a
    /// send row says `A`, and `A` is a position, so the name is free to
    /// say what the return actually IS.
    pub name: String,
    pub mute: bool,
    /// Fader level as LINEAR amplitude, exactly as a track's is.
    pub volume: f32,
    pub pan: f32,
    /// Effects only, in signal order — a return has nothing for an
    /// instrument to play.
    pub chain: Vec<DeviceInstance>,
    #[serde(default)]
    pub sampler_sources: std::collections::BTreeMap<u64, SamplerSource>,
    #[serde(default)]
    pub racks: std::collections::BTreeMap<u64, device::RackUi>,
}

impl Default for ReturnTrack {
    fn default() -> Self {
        Self {
            name: String::new(),
            mute: false,
            volume: 1.0,
            pan: 0.0,
            chain: Vec::new(),
            sampler_sources: std::collections::BTreeMap::new(),
            racks: std::collections::BTreeMap::new(),
        }
    }
}

impl ReturnTrack {
    /// How many returns a project may hold: A through H, which is the
    /// count every desk and every DAW settled on, and the count a send
    /// column can label without a second character.
    pub const MAX: usize = 8;

    /// The letter at position `index`: `A`, `B`, … Past `MAX` it is `?`,
    /// which cannot happen and is still not a panic.
    pub fn letter(index: usize) -> char {
        if index < Self::MAX {
            (b'A' + index as u8) as char
        } else {
            '?'
        }
    }

    /// A fresh return, named for where it sits.
    pub fn new(index: usize) -> Self {
        Self {
            name: format!("Return {}", Self::letter(index)),
            ..Self::default()
        }
    }

    pub fn device_mut(&mut self, id: u64) -> Option<&mut DeviceInstance> {
        self.chain.iter_mut().find(|instance| instance.id == id)
    }

    /// Put an effect on this return. Instruments are refused for the
    /// reason the master refuses them, and the caller is told so it can
    /// say why rather than dropping a synth into a bus.
    pub fn insert_device(&mut self, instance: DeviceInstance) -> bool {
        if instance.kind().is_instrument() {
            return false;
        }
        self.chain.push(instance);
        true
    }
}

/// Move `moved` so it sits immediately before `before` in the chain.
///
/// Positional, because a chain IS its order — the signal runs left to
/// right and there is nothing else to say about where a device is.
///
/// Two things it refuses. An INSTRUMENT stays at the head: the head is
/// what makes sound and everything after it shapes that, so a synth
/// dragged into the middle would compile to a source the effects before
/// it never see. And a device only moves among its OWN siblings — a card
/// dragged out of a rack would be leaving the rack, which is a different
/// gesture from reordering and is not this one.
///
/// Returns whether anything moved, so a caller can tell a no-op from a
/// refusal without asking twice.
pub fn move_device(chain: &mut Vec<DeviceInstance>, moved: u64, before: u64) -> bool {
    if moved == before {
        return false;
    }
    let Some(from) = chain.iter().position(|device| device.id == moved) else {
        return false;
    };
    let Some(to) = chain.iter().position(|device| device.id == before) else {
        return false;
    };
    if chain[from].kind().is_instrument() || chain[to].kind().is_instrument() {
        return false;
    }
    if chain[from].parent != chain[to].parent {
        return false;
    }
    let device = chain.remove(from);
    // Removing shifted everything after the hole down by one, so a target
    // that was past it is now one place earlier.
    let to = if to > from { to - 1 } else { to };
    chain.insert(to, device);
    true
}

/// Force the ordering rule onto a chain that came from a FILE: keep the
/// first instrument, move it to the head, drop any others. A hand-edited
/// project is input like any other, and an instrument in the middle of a
/// chain would compile to a source the effects before it never see.
pub fn sanitize_chain(chain: &mut Vec<DeviceInstance>) {
    let Some(at) = chain.iter().position(|d| d.kind().is_instrument()) else {
        return;
    };
    let instrument = chain.remove(at);
    chain.retain(|other| !other.kind().is_instrument());
    chain.insert(0, instrument);
}

impl Default for Track {
    fn default() -> Self {
        Self {
            kind: TrackKind::default(),
            name: String::new(),
            height: TRACK_H,
            mute: false,
            solo: false,
            pan: 0.0,
            volume: 1.0,
            is_group: false,
            folded: false,
            depth: 0,
            input: TrackInput::default(),
            monitor: Monitor::default(),
            armed: false,
            sends: Vec::new(),
            automation: TrackAutomation::default(),
            // A fresh track has no devices at all.
            chain: Vec::new(),
            sampler_sources: std::collections::BTreeMap::new(),
            racks: std::collections::BTreeMap::new(),
        }
    }
}

/// Compatibility shape for projects written before a track's devices became
/// a chain of identified instances.
///
/// A v1 track carried ONE instrument and ONE effect as named fields, plus
/// two copies of the synth's values: `params` in engine units and `synth` as
/// normalized knob positions. `params` is the one the engine ever read, so
/// it is the one that survives; `synth` is simply not named here and serde
/// skips it. The reverb's stored values were already engine units (its
/// ranges are `0..=1` and the percent was a rendering), so they carry over
/// as they are.
///
/// The instances land with id 0 — the document's id mint lives in
/// `apply_project_doc`, which is also where the old targets are rewritten.
#[derive(serde::Deserialize)]
#[serde(default)]
pub struct TrackWire {
    pub kind: TrackKind,
    pub name: String,
    pub height: f32,
    pub mute: bool,
    pub solo: bool,
    pub pan: f32,
    pub volume: f32,
    /// Absent from a project written before groups existed, which loads
    /// as a flat stack — which is what it was.
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub folded: bool,
    #[serde(default)]
    pub depth: u8,
    /// Absent from a project written before routing existed, which loads
    /// as a lane that is its clips and nothing else — exactly how it
    /// sounded.
    #[serde(default)]
    pub input: TrackInput,
    #[serde(default)]
    pub monitor: Monitor,
    /// `#[serde(default)]` so a project written before returns existed
    /// loads with a track that sends nowhere — which is exactly how it
    /// sounded.
    #[serde(default)]
    pub sends: Vec<f32>,
    pub automation: TrackAutomation,
    pub chain: Vec<DeviceInstance>,
    pub sampler_sources: std::collections::BTreeMap<u64, SamplerSource>,
    pub device: Option<DeviceKind>,
    pub fx: Option<DeviceKind>,
    pub params: SynthParams,
    pub reverb: device::ReverbUi,
    /// Each rack's name and macros. `#[serde(default)]` so a project
    /// written before racks existed loads with none.
    #[serde(default)]
    pub racks: std::collections::BTreeMap<u64, device::RackUi>,
}

impl Default for TrackWire {
    fn default() -> Self {
        let track = Track::default();
        Self {
            kind: track.kind,
            name: track.name,
            height: track.height,
            mute: track.mute,
            solo: track.solo,
            pan: track.pan,
            volume: track.volume,
            is_group: track.is_group,
            folded: track.folded,
            depth: track.depth,
            input: track.input,
            monitor: track.monitor,
            sends: track.sends,
            automation: track.automation,
            chain: track.chain,
            sampler_sources: track.sampler_sources,
            device: None,
            fx: None,
            params: SynthParams::default(),
            reverb: device::ReverbUi::default(),
            racks: track.racks,
        }
    }
}

impl<'de> serde::Deserialize<'de> for Track {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TrackWire::deserialize(deserializer)?;
        let mut chain = wire.chain;
        if chain.is_empty() {
            if let Some(kind) = wire.device.filter(|kind| kind.is_instrument()) {
                chain.push(DeviceInstance {
                    id: 0,
                    state: match kind {
                        DeviceKind::SineSynth => DeviceState::SineSynth(wire.params),
                        other => DeviceState::new(other),
                    },
                    // A v1 project predates racks entirely, so nothing
                    // it carries lives inside one.
                    parent: None,
                    bypass: false,
                    page: 0,
                    view_zoom: unit_zoom(),
                    view_scroll: 0.0,
                });
            }
            if let Some(kind) = wire.fx.filter(|kind| !kind.is_instrument()) {
                chain.push(DeviceInstance {
                    id: 0,
                    state: match kind {
                        // A project written before the network replaced
                        // the Freeverb carries three numbers. The rest
                        // come from the table's defaults, which is what
                        // `..Default::default()` is for — the old file
                        // never knew about pre-delay or width and must
                        // not be made to guess.
                        DeviceKind::Reverb => DeviceState::Reverb(ReverbParams {
                            mix: wire.reverb.mix,
                            size: wire.reverb.size,
                            damp: wire.reverb.damp,
                            ..ReverbParams::default()
                        }),
                        other => DeviceState::new(other),
                    },
                    // A v1 project predates racks entirely, so nothing
                    // it carries lives inside one.
                    parent: None,
                    bypass: false,
                    page: 0,
                    view_zoom: unit_zoom(),
                    view_scroll: 0.0,
                });
            }
        }
        Ok(Self {
            kind: wire.kind,
            name: wire.name,
            height: wire.height,
            mute: wire.mute,
            solo: wire.solo,
            pan: wire.pan,
            volume: wire.volume,
            is_group: wire.is_group,
            folded: wire.folded,
            depth: wire.depth,
            input: wire.input,
            monitor: wire.monitor,
            // Deliberately not from the file — see `Track::armed`.
            armed: false,
            sends: wire.sends,
            automation: wire.automation,
            chain,
            sampler_sources: wire.sampler_sources,
            racks: wire.racks,
        })
    }
}

pub fn split_track_automation_at(track: &mut Track, beat: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.split_at(&target, beat, base);
    }
}

pub fn insert_track_automation_time(track: &mut Track, at: f32, amount: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.insert_time(&target, at, amount, base);
    }
}

pub fn delete_track_automation_time(track: &mut Track, from: f32, to: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.delete_time(&target, from, to, base);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod rack_tests {
    use super::*;

    fn device(id: u64, kind: DeviceKind) -> DeviceInstance {
        DeviceInstance {
            id,
            parent: None,
            state: DeviceState::new(kind),
            bypass: false,
            page: 0,
            view_zoom: unit_zoom(),
            view_scroll: 0.0,
        }
    }

    fn track_with(kinds: &[DeviceKind]) -> Track {
        let mut track = Track::default();
        for (at, kind) in kinds.iter().enumerate() {
            track.insert_device(device(at as u64 + 1, *kind));
        }
        track
    }

    /// Grouping points every loose device at the rack and moves NOTHING.
    /// The order a chain runs in is the order it already sat in.
    #[test]
    fn grouping_reparents_without_reordering() {
        let mut track = track_with(&[DeviceKind::Lofi, DeviceKind::Sheen, DeviceKind::Tilt]);
        let before: Vec<u64> = track.chain.iter().map(|d| d.id).collect();
        let rack = track.group_into_rack(99, "bass rack").unwrap();

        let after: Vec<u64> = track
            .chain
            .iter()
            .filter(|d| d.id != rack)
            .map(|d| d.id)
            .collect();
        assert_eq!(before, after, "grouping reordered the chain");
        assert!(
            track
                .chain
                .iter()
                .filter(|d| d.id != rack)
                .all(|d| d.parent == Some(rack)),
            "something was left outside the rack"
        );
        assert_eq!(
            track.racks.get(&rack).map(|r| r.name.as_str()),
            Some("bass rack")
        );
    }

    /// And ungrouping is its exact inverse.
    #[test]
    fn ungrouping_is_the_inverse_of_grouping() {
        let mut track = track_with(&[DeviceKind::Lofi, DeviceKind::Sheen]);
        let before = track.chain.clone();
        let rack = track.group_into_rack(99, "r").unwrap();
        assert!(track.ungroup_rack(rack));
        assert_eq!(track.chain, before, "the round trip changed the chain");
        assert!(track.racks.is_empty(), "the rack's macros outlived it");
    }

    /// An empty chain has nothing to group, and says so rather than
    /// making an empty rack nobody asked for.
    #[test]
    fn an_empty_chain_declines_to_group() {
        let mut track = Track::default();
        assert!(track.group_into_rack(99, "r").is_none());
        assert!(track.chain.is_empty());
        assert!(track.racks.is_empty());
    }

    /// A chain that is ALREADY one rack declines too — racks do not nest
    /// yet, and wrapping a rack in a rack would make a container whose
    /// child the UI cannot draw.
    #[test]
    fn a_chain_that_is_already_a_rack_declines() {
        let mut track = track_with(&[DeviceKind::Lofi]);
        let rack = track.group_into_rack(99, "r").unwrap();
        assert!(
            track.group_into_rack(100, "again").is_none(),
            "a rack was wrapped in a rack"
        );
        assert_eq!(
            track
                .chain
                .iter()
                .filter(|d| matches!(d.state, DeviceState::Rack))
                .count(),
            1
        );
        assert!(track.ungroup_rack(rack));
    }

    /// Ungrouping something that is not a rack does nothing at all.
    #[test]
    fn ungrouping_a_non_rack_is_refused() {
        let mut track = track_with(&[DeviceKind::Lofi]);
        assert!(!track.ungroup_rack(1), "a plain device was ungrouped");
        assert_eq!(track.chain.len(), 1);
    }
}
