//! Canonical musical sequence edited by every note view.
//!
//! The wrapped grid, chord editor and future piano roll are projections of
//! this model. They may keep cursor and zoom state, but never their own notes.
//! This is green-zone project data: compilation later turns it into the
//! immutable sample-stamped events consumed by the audio callback.
//!
//! Pitch is stored as [`Pitch`]: an anchor (absolute physics or a key
//! degree) plus an explicit cents deviation. Resolution happens at
//! projection, never here and never in the callback
//! (`notes/20260831-pitch-lens-spec.md`).

use crate::devices::DeviceKind;
use crate::pitch::{Key, Pitch, default_key};

pub const GRID_COLUMNS: usize = 16;
pub const GRID_ROWS: usize = 4;
pub const PATTERN_STEPS: usize = GRID_COLUMNS * GRID_ROWS;
pub const TICKS_PER_BEAT: usize = 48;
/// The canonical Song pattern's one-sixteenth trig stride.
pub const PATTERN_STEP_TICKS: usize = TICKS_PER_BEAT / 4;
pub const DEFAULT_PATTERN_TICKS: usize = PATTERN_STEPS * PATTERN_STEP_TICKS;

/// The canonical automation target ids the mixer speaks.
///
/// These strings are FILE FORMAT. They are shared with the legacy
/// `targets` table, and `redesign_bridge` asserts the two still agree —
/// a silent divergence here orphans every envelope already on disk.
pub const TRACK_VOLUME: &str = "track.volume";
pub const TRACK_PAN: &str = "track.pan";

/// How deeply the Song's positional group stack may nest.
///
/// This deliberately matches the legacy stack the C1 copyist writes. The
/// bound makes every group walk finite even for a hand-edited document.
pub const MAX_GROUP_DEPTH: u8 = 8;

/// Unity gain. A serde default, because a track absent from a pre-mixer
/// document must come back at unity: defaulting a fader to zero would
/// silently mute every project written before the mixer existed.
fn unity() -> f32 {
    1.0
}

/// Where a lane's LIVE signal comes from, beside its clips.
///
/// AUDIO ONLY, deliberately. The engine's input node reads device audio
/// channels and there is no MIDI input path at all, so a note lane must not
/// be offered a route whose every entry is silence.
///
/// Channels are stored as INDICES and not clamped on load, because the
/// number of them belongs to whatever interface is plugged in today. A
/// channel that is not there reads as silence at the node, so a project
/// written on an eight-in desk opens on a laptop quiet rather than wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TrackInput {
    /// Nothing. The lane is its clips and only its clips.
    #[default]
    None,
    /// One channel, centred.
    Mono(u32),
    /// A pair, hard left and hard right.
    Stereo(u32, u32),
}

impl TrackInput {
    /// The label a strip shows: `—`, `1`, `1/2`.
    ///
    /// One-based, because a musician counts inputs from one and the engine
    /// counts them from zero, and exactly one place should do the arithmetic.
    pub fn label(self) -> String {
        match self {
            Self::None => "—".to_owned(),
            Self::Mono(channel) => format!("{}", channel + 1),
            Self::Stereo(left, right) => format!("{}/{}", left + 1, right + 1),
        }
    }

    /// Every route an interface with `channels` inputs can offer, in the
    /// order a click cycles through them: nothing, each channel alone, then
    /// each adjacent pair.
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

    /// The next route after this one, wrapping. `back` walks the other way.
    pub fn cycled(self, channels: u32, back: bool) -> Self {
        let routes = Self::routes(channels);
        let at = routes.iter().position(|route| *route == self).unwrap_or(0);
        let step = if back { routes.len() - 1 } else { 1 };
        routes[(at + step) % routes.len()]
    }
}

/// Whether a routed input is HEARD.
///
/// `Off` is the safety default: an input wired to the speakers may be a
/// feedback loop, so choosing a source must not itself make noise. `Auto`
/// makes arming one gesture — armed lanes are heard, disarmed lanes are not.
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
    pub fn hears(self, armed: bool) -> bool {
        match self {
            Self::Off => false,
            Self::In => true,
            Self::Auto => armed,
        }
    }

    /// Off, in, auto — quietest first.
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::In,
            Self::In => Self::Auto,
            Self::Auto => Self::Off,
        }
    }

    /// THREE STATES, THREE SYMBOLS: paint never has to carry meaning that
    /// the text withholds.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "--",
            Self::In => "IN",
            Self::Auto => "AU",
        }
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    #[test]
    fn monitoring_truth_table_and_cycle_are_exact() {
        assert!(!Monitor::Off.hears(false));
        assert!(!Monitor::Off.hears(true), "off means off, armed or not");
        assert!(Monitor::In.hears(false), "in means in, armed or not");
        assert!(Monitor::In.hears(true));
        assert!(!Monitor::Auto.hears(false));
        assert!(Monitor::Auto.hears(true));

        let mut state = Monitor::default();
        let seen = (0..3)
            .map(|_| {
                state = state.cycled();
                state
            })
            .collect::<Vec<_>>();
        assert_eq!(seen, vec![Monitor::In, Monitor::Auto, Monitor::Off]);
        assert_eq!(Monitor::Off.label(), "--");
        assert_eq!(Monitor::In.label(), "IN");
        assert_eq!(Monitor::Auto.label(), "AU");
    }

    #[test]
    fn route_labels_are_one_based_and_cycles_are_bounded() {
        assert_eq!(TrackInput::None.label(), "—");
        assert_eq!(TrackInput::Mono(0).label(), "1");
        assert_eq!(TrackInput::Stereo(0, 1).label(), "1/2");
        assert_eq!(
            TrackInput::routes(2),
            vec![
                TrackInput::None,
                TrackInput::Mono(0),
                TrackInput::Mono(1),
                TrackInput::Stereo(0, 1),
            ]
        );
        assert_eq!(TrackInput::None.cycled(2, true), TrackInput::Stereo(0, 1));
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PatternId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TrackId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct BlockId(pub u64);

/// One device's identity, stable across a chain reorder.
///
/// An INSTANCE id and not a position, for the reason the legacy compiler
/// already gives: a knob sends a letter to a node, every graph swap mints
/// fresh node ids, and the mapping from device to node is captured with
/// the schedule. A device addressed by position would be a different
/// device the moment something above it moved.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DeviceId(pub u64);

/// One device on a track: what it is, whether it is passing sound
/// through, and the parameters that differ from its defaults.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Device {
    pub id: DeviceId,
    pub kind: DeviceKind,
    /// Passing sound through untouched. NOT the same as removing it: a
    /// bypassed device keeps its place, its id and its settings, so the
    /// comparison it exists to support is one keystroke each way.
    #[serde(default)]
    pub bypassed: bool,
    /// Parameters that differ from the kind's table defaults, as
    /// `(id, value)`.
    ///
    /// SPARSE, and by parameter id rather than by position. A positional
    /// vector would silently reassign every value the day a device gains
    /// a parameter in the middle of its table; an id survives that, and
    /// an id the table no longer knows is simply ignored — the same rule
    /// the red zone already applies to a stale letter.
    #[serde(default)]
    pub overrides: Vec<(u32, f32)>,
    /// The file a sampler plays. `None` on every other kind, and on a
    /// sampler nobody has given a sound yet — which compiles to a silent
    /// sampler rather than a refused graph, because a missing sample must
    /// not mute the project.
    ///
    /// On the DEVICE rather than in a parameter: a path is not a number
    /// in a range, and the parameter table is the engine's table, which
    /// the red zone reads and must never find a string in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<std::path::PathBuf>,
}

impl Device {
    pub fn new(id: DeviceId, kind: DeviceKind) -> Self {
        Self {
            id,
            kind,
            bypassed: false,
            overrides: Vec::new(),
            sample: None,
        }
    }

    /// The device's table, from the catalog.
    pub fn table(&self) -> &'static [crate::params::ParamDef] {
        self.kind.spec().params
    }

    /// What `param` is set to: the override if there is one, and the
    /// table's own default otherwise. An id the table does not know reads
    /// as zero, because there is nothing else it could honestly read as.
    pub fn value(&self, param: u32) -> f32 {
        if let Some((_, value)) = self.overrides.iter().find(|(id, _)| *id == param) {
            return *value;
        }
        self.table()
            .iter()
            .find(|def| def.id == param)
            .map_or(0.0, |def| def.default)
    }

    /// Set `param`, clamped to the range the table declares. `false` means
    /// the table has no such parameter and nothing was written — a device
    /// never grows a parameter by being sent one.
    pub fn set(&mut self, param: u32, value: f32) -> bool {
        let Some(def) = self.table().iter().find(|def| def.id == param) else {
            return false;
        };
        let value = def.clamp(value);
        match self.overrides.iter_mut().find(|(id, _)| *id == param) {
            Some(entry) => entry.1 = value,
            None => self.overrides.push((param, value)),
        }
        true
    }

    /// Whether this device makes sound rather than shaping it.
    pub fn is_instrument(&self) -> bool {
        self.kind.spec().instrument
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Note {
    pub pitch: Pitch,
    pub length_ticks: usize,
    pub velocity: u8,
    /// Per-note silence, independent of the time-aligned trig gate.
    #[serde(default)]
    pub muted: bool,
    /// Time deviation: signed sub-step displacement in ticks. The pushed
    /// hat, the dragged snare. Data, never quantized away.
    pub micro_ticks: i16,
}

impl Note {
    /// Bridge-era constructor: a legacy MIDI number becomes an absolute
    /// anchor, so every pre-pitch call site compiles unchanged and every
    /// imported note is physics until an explicit transform says
    /// otherwise.
    pub fn new(pitch: u8, length_ticks: usize, velocity: u8) -> Self {
        Self::with_pitch(Pitch::from_midi(pitch.min(127)), length_ticks, velocity)
    }

    pub fn with_pitch(pitch: Pitch, length_ticks: usize, velocity: u8) -> Self {
        Self {
            pitch,
            length_ticks: length_ticks.max(1),
            velocity: velocity.clamp(1, 127),
            muted: false,
            micro_ticks: 0,
        }
    }
}

/// Which anchor ENTRY produces on a track. A default for the hands, not
/// a constraint on the data: a track may hold mixed anchors — mixture is
/// the point.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PitchAuthority {
    Absolute,
    #[default]
    Degree,
}

/// One chronological step. Multiple notes are one chord, not parallel lanes.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Trig {
    pub enabled: bool,
    pub notes: Vec<Note>,
    pub probability: f32,
}

impl Default for Trig {
    fn default() -> Self {
        Self {
            enabled: false,
            notes: Vec::new(),
            probability: 1.0,
        }
    }
}

impl Trig {
    pub fn primary(&self) -> Option<&Note> {
        self.notes.first()
    }

    pub fn set_primary(&mut self, note: Note) {
        if let Some(primary) = self.notes.first_mut() {
            *primary = note;
        } else {
            self.notes.push(note);
        }
        self.enabled = true;
    }

    pub fn add_tone(&mut self, note: Note) {
        if let Some(existing) = self.notes.iter_mut().find(|tone| tone.pitch == note.pitch) {
            *existing = note;
        } else {
            self.notes.push(note);
            self.notes.sort_by(|a, b| a.pitch.stack_order(&b.pitch));
        }
        self.enabled = true;
    }

    pub fn clear(&mut self) {
        self.notes.clear();
        self.enabled = false;
    }

    /// Add a note at its own sub-step offset. Same pitch at the same
    /// offset replaces; anything else joins. Notes are kept in time order
    /// then stack order, so [`Trig::primary`] is the earliest and lowest.
    pub fn add_tone_at(&mut self, note: Note) {
        if let Some(existing) = self
            .notes
            .iter_mut()
            .find(|tone| tone.pitch == note.pitch && tone.micro_ticks == note.micro_ticks)
        {
            *existing = note;
        } else {
            self.notes.push(note);
            self.notes.sort_by(|a, b| {
                a.micro_ticks
                    .cmp(&b.micro_ticks)
                    .then_with(|| a.pitch.stack_order(&b.pitch))
            });
        }
        self.enabled = true;
    }

    /// The notes at exactly `micro` ticks into the step.
    pub fn notes_at(&self, micro: i16) -> impl Iterator<Item = &Note> {
        self.notes
            .iter()
            .filter(move |note| note.micro_ticks == micro)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    /// Natural session-clip boundary. Timeline placements may override it.
    #[serde(default = "default_pattern_ticks")]
    pub length_ticks: usize,
    trigs: Vec<Trig>,
}

const fn default_pattern_ticks() -> usize {
    DEFAULT_PATTERN_TICKS
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            id: PatternId(1),
            name: "P01".to_owned(),
            length_ticks: DEFAULT_PATTERN_TICKS,
            trigs: vec![Trig::default(); PATTERN_STEPS],
        }
    }
}

/// One placement of a pattern in song time. Musical ticks are project data;
/// sample stamps are produced only when the green-zone compiler runs.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PatternBlock {
    pub id: BlockId,
    pub pattern_id: PatternId,
    pub start_tick: usize,
    pub length_ticks: usize,
}

/// A clip-relative loop: a region of an audio block's own content that
/// repeats to fill it. Placement, not source — which is why it lives on
/// the block and not on the `AudioSource` inside it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LoopBrace {
    pub start_tick: usize,
    pub length_ticks: usize,
}

/// Why a sound could not be landed. Each is said OUT LOUD at the
/// surface; none of them is a silent no-op.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LandRefusal {
    NoTrack,
    NotAnAudioTrack,
    Occupied,
    Unreadable,
}

impl LandRefusal {
    /// The sign the performer actually reads.
    pub const fn sign(self) -> &'static str {
        match self {
            Self::NoTrack => "LAND: NO TRACK",
            Self::NotAnAudioTrack => "LAND: NOT AN AUDIO TRACK",
            Self::Occupied => "LAND: BLOCK IN THE WAY",
            Self::Unreadable => "LAND: CANNOT READ",
        }
    }
}

/// One placement of recorded sound in song time.
///
/// The `AudioSource` carries everything about the SOUND — path, trim,
/// gain, varispeed, reversal, and the fades. None of it is duplicated
/// here: a second home for a fade is a second authority for one fact.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AudioBlock {
    pub id: BlockId,
    /// What to CALL this sound.
    ///
    /// Carried rather than derived from the path, because the path is a
    /// cache artefact: an imported wav lives under a content hash, so a
    /// name taken from it reads `ed3ac3c1f22032ac-48000` instead of
    /// `cw_amen01_175`. A hash is not a name.
    #[serde(default)]
    pub name: String,
    pub start_tick: usize,
    pub length_ticks: usize,
    pub source: crate::audio_source::AudioSource,
    #[serde(default)]
    pub loop_brace: Option<LoopBrace>,
}

impl AudioBlock {
    /// One past the last tick this block occupies. Saturating, because an
    /// overlap test must never be the thing that panics.
    pub fn end_tick(&self) -> usize {
        self.start_tick.saturating_add(self.length_ticks)
    }

    pub fn intersects(&self, start: usize, end: usize) -> bool {
        self.start_tick < end && start < self.end_tick()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum TrackKind {
    Instrument,
    Audio,
}

/// One breakpoint of an automation envelope, in SONG TICKS.
///
/// Ticks, not beats: song time is ticks (the sequencing contract), and a
/// beat is a legacy render detail that dies with the projection.
///
/// `bend` bows the segment LEAVING this point: zero is a straight line and
/// the sign chooses which side it bows. It is the ONE curve control, and it
/// does not grow a second — no handles, no competing curve menu.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Point {
    pub tick: usize,
    pub value: f32,
    pub bend: f32,
}

impl Default for Point {
    fn default() -> Self {
        Self {
            tick: 0,
            value: 0.0,
            bend: 0.0,
        }
    }
}

/// Every breakpoint one track carries against one target.
///
/// `target` is a parameter id string (`track.volume`, `dev.7.reverb.mix`).
/// It is FILE FORMAT: renaming one orphans every envelope that points at
/// it, which is why the ids are minted in one place and never re-derived.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Envelope {
    pub target: String,
    pub points: Vec<Point>,
}

// NOTE: `Eq` is gone from `Track` deliberately. An envelope carries f32
// values, so total equality is not available; `PartialEq` is what the
// history and the tests actually use.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub kind: TrackKind,
    pub blocks: Vec<PatternBlock>,
    /// Silenced unless solo precedence makes this track the addressed
    /// sound. Absent from pre-mixer Song documents means sounding.
    #[serde(default)]
    pub muted: bool,
    /// While any Song track is soloed, only soloed Song tracks sound.
    #[serde(default)]
    pub solo: bool,
    /// Which anchor ENTRY produces here (the harmonic scope chain's
    /// per-track link: an Absolute track ignores the project key).
    pub pitch_authority: PitchAuthority,
    /// Track-scoped automation in SONG TIME: the BASE authority of the
    /// offset model.
    ///
    /// A trig's parameter lock is the OFFSET; `param_law::compose` is the
    /// only place the two are ever summed, and no device reinterprets the
    /// result.
    ///
    /// Track-scoped, NOT pattern-scoped, and the consequence is worth
    /// stating because it is not obvious: a pattern placed twice carries
    /// the same locks to both placements and reads DIFFERENT curve values
    /// at each. Locks travel with the content; curves belong to the
    /// timeline. Absent from pre-automation Song documents means "no
    /// curves", which reads exactly as it did before.
    /// Recorded sound placed on this track, in song time.
    ///
    /// A SEPARATE list rather than `blocks` becoming an enum, and the
    /// reason is the file format. RON is not self-describing enough for
    /// `#[serde(untagged)]` (it fails with "data did not match any
    /// variant"), and without the `implicit_some` extension an
    /// `Option`-based wire struct cannot read the old bare values either.
    /// An externally-tagged enum would rewrite every `blocks` entry from
    /// `(id:(1),…)` to `Pattern((id:(1),…))`, and since `blocks` is a
    /// required field, a present-but-wrong value is not rescued by any
    /// default: the Track fails, the Song fails, and the WHOLE PROJECT
    /// refuses to open rather than merely losing its song.
    ///
    /// Two typed lists cost a merge when time order across both is
    /// needed (see `blocks_in_time_order`). That is a small price for
    /// every project on disk continuing to load untouched — and
    /// `TrackKind` already says which list a track actually uses.
    #[serde(default)]
    pub audio_blocks: Vec<AudioBlock>,
    #[serde(default)]
    pub automation: Vec<Envelope>,
    /// Fader level as LINEAR amplitude, 1.0 = unity.
    ///
    /// Linear because that is what the engine multiplies by; the fader
    /// widget owns the dB mapping, which is the only place that curve
    /// belongs. Automatable through [`TRACK_VOLUME`].
    #[serde(default = "unity")]
    pub volume: f32,
    /// Constant-power pan, `-1..=1`. Centre is 0.0 and it is EXACT, so
    /// "back to the middle" stays reachable by hand rather than by luck.
    /// Automatable through [`TRACK_PAN`].
    #[serde(default)]
    pub pan: f32,
    /// How much of this track each return receives, as LINEAR gain.
    ///
    /// Positional and deliberately short: `sends[0]` feeds return A, and a
    /// missing entry is exact zero. The graph owns the post-fader tap; this
    /// green-zone model owns only the amount.
    #[serde(default)]
    pub sends: Vec<f32>,
    /// A group is a summing lane whose members are the contiguous run below
    /// it at greater depth. Membership is position, never a pointer.
    #[serde(default)]
    pub is_group: bool,
    /// Closed in the arrangement while still sounding.
    #[serde(default)]
    pub folded: bool,
    /// The devices on this track, in signal order.
    ///
    /// ONE list, with the instrument at the head where there is one —
    /// not an instrument field beside an effects vector. A chain is
    /// reordered as a chain, and two collections would make "move this
    /// device up" two different operations depending on where it started.
    /// [`Song::normalize_chains`] is what keeps the head a head.
    ///
    /// EMPTY is the meaningful default and not a gap: an instrument track
    /// with no chain sounds the default instrument, which is exactly what
    /// every project written before devices reached the Song does.
    #[serde(default)]
    pub chain: Vec<Device>,
    /// Zero is top level. A lane at depth `d` belongs to the nearest group
    /// above it at depth `d - 1`.
    #[serde(default)]
    pub depth: u8,
    /// Where this audio lane's live signal comes from, beside its clips.
    #[serde(default)]
    pub input: TrackInput,
    /// Whether that live signal is heard.
    #[serde(default)]
    pub monitor: Monitor,
    /// Armed to RECORD and, under [`Monitor::Auto`], to hear the input.
    ///
    /// An arm is a thing the performer is doing RIGHT NOW, not project
    /// state. Opening a song with live lanes armed could begin recording
    /// over them before anybody had looked at the screen.
    #[serde(skip)]
    pub armed: bool,
}

/// A placement on a track's timeline, whichever list it came from.
/// The borrow is what lets time-ordered work treat both kinds alike
/// without either list having to become the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockRef<'a> {
    Pattern(&'a PatternBlock),
    Audio(&'a AudioBlock),
}

impl BlockRef<'_> {
    pub fn id(&self) -> BlockId {
        match self {
            Self::Pattern(block) => block.id,
            Self::Audio(block) => block.id,
        }
    }

    pub fn start_tick(&self) -> usize {
        match self {
            Self::Pattern(block) => block.start_tick,
            Self::Audio(block) => block.start_tick,
        }
    }

    pub fn length_ticks(&self) -> usize {
        match self {
            Self::Pattern(block) => block.length_ticks,
            Self::Audio(block) => block.length_ticks,
        }
    }

    pub fn end_tick(&self) -> usize {
        self.start_tick().saturating_add(self.length_ticks())
    }

    pub fn intersects(&self, start: usize, end: usize) -> bool {
        self.start_tick() < end && start < self.end_tick()
    }
}

impl Track {
    /// Every placement on this track, pattern and audio alike, in time
    /// order. The one iterator that sees a whole lane — what the time
    /// verbs need, and what an overlap test across both kinds needs.
    pub fn blocks_in_time_order(&self) -> Vec<BlockRef<'_>> {
        let mut all: Vec<BlockRef<'_>> = self
            .blocks
            .iter()
            .map(BlockRef::Pattern)
            .chain(self.audio_blocks.iter().map(BlockRef::Audio))
            .collect();
        all.sort_by_key(BlockRef::start_tick);
        all
    }

    /// Whether anything at all on this lane occupies `[start, end)` —
    /// pattern or audio. An overlap test that only consulted `blocks`
    /// would happily drop a pattern on top of a recording.
    pub fn occupied(&self, start: usize, end: usize) -> bool {
        self.blocks.iter().any(|block| {
            block.start_tick < end && start < block.start_tick.saturating_add(block.length_ticks)
        }) || self
            .audio_blocks
            .iter()
            .any(|block| block.intersects(start, end))
    }

    /// This track's breakpoints against `target`, or an empty slice.
    pub fn points(&self, target: &str) -> &[Point] {
        self.automation
            .iter()
            .find(|envelope| envelope.target == target)
            .map_or(&[], |envelope| envelope.points.as_slice())
    }

    /// The breakpoints against `target`, minting an empty envelope when
    /// the track does not yet automate it.
    pub fn points_mut(&mut self, target: &str) -> &mut Vec<Point> {
        if let Some(index) = self
            .automation
            .iter()
            .position(|envelope| envelope.target == target)
        {
            return &mut self.automation[index].points;
        }
        self.automation.push(Envelope {
            target: target.to_owned(),
            points: Vec::new(),
        });
        let last = self.automation.len() - 1;
        &mut self.automation[last].points
    }

    /// Whether `target` carries a curve at all. A target with an envelope
    /// but no points is NOT automated — it reads as the bare knob.
    pub fn automated(&self, target: &str) -> bool {
        !self.points(target).is_empty()
    }

    /// The curve's value at `tick`: hold the knob until the first point,
    /// follow each point's outgoing bend, then hold the final value.
    ///
    /// `base` is the knob — layer one of the three-layer read. With no
    /// points this returns it untouched, which is what makes an
    /// un-automated parameter cost nothing.
    pub fn value_at(&self, target: &str, tick: usize, base: f32) -> f32 {
        let points = self.points(target);
        let Some(first) = points.first() else {
            return base;
        };
        if tick < first.tick {
            return base;
        }
        for pair in points.windows(2) {
            let [a, b] = pair else { continue };
            if tick <= b.tick {
                let span = b.tick.saturating_sub(a.tick);
                let t = if span == 0 {
                    1.0
                } else {
                    (tick.saturating_sub(a.tick) as f32 / span as f32).clamp(0.0, 1.0)
                };
                let bent = if a.bend >= 0.0 {
                    t.powf(1.0 + a.bend * 5.0)
                } else {
                    1.0 - (1.0 - t).powf(1.0 + -a.bend * 5.0)
                };
                return a.value + (b.value - a.value) * bent;
            }
        }
        points.last().map_or(base, |point| point.value)
    }

    /// Place or move a breakpoint. A point already AT `tick` takes the new
    /// value and keeps its bend — placing twice is an edit, not a stack.
    pub fn insert_point(&mut self, target: &str, tick: usize, value: f32) {
        let points = self.points_mut(target);
        let at = points.partition_point(|point| point.tick < tick);
        if points.get(at).is_some_and(|point| point.tick == tick) {
            points[at].value = value;
        } else {
            points.insert(
                at,
                Point {
                    tick,
                    value,
                    bend: 0.0,
                },
            );
        }
    }

    /// Remove the breakpoint exactly at `tick`, and say whether one went.
    pub fn remove_point(&mut self, target: &str, tick: usize) -> bool {
        let points = self.points_mut(target);
        let Some(at) = points.iter().position(|point| point.tick == tick) else {
            return false;
        };
        points.remove(at);
        true
    }

    /// Bow the segment leaving the point at `tick`.
    pub fn bend_point(&mut self, target: &str, tick: usize, bend: f32) -> bool {
        if !bend.is_finite() {
            return false;
        }
        let points = self.points_mut(target);
        let Some(at) = points.iter().position(|point| point.tick == tick) else {
            return false;
        };
        points[at].bend = bend.clamp(-1.0, 1.0);
        true
    }

    /// The fader at `tick` — the knob, or the curve that overrides it.
    ///
    /// Never negative: an envelope drawn below the floor reads as silence
    /// rather than as a phase inversion nobody asked for.
    pub fn volume_at(&self, tick: usize) -> f32 {
        self.value_at(TRACK_VOLUME, tick, self.volume).max(0.0)
    }

    /// The pan at `tick`, pinned to the legal span so a curve drawn past
    /// an edge reads as hard left or hard right instead of nonsense.
    pub fn pan_at(&self, tick: usize) -> f32 {
        self.value_at(TRACK_PAN, tick, self.pan).clamp(-1.0, 1.0)
    }

    /// One send's knob, with a short vector reading as exact silence.
    pub fn send(&self, index: usize) -> f32 {
        self.sends.get(index).copied().unwrap_or(0.0)
    }

    /// One send at `tick`: its static knob or the curve addressed by the
    /// return's stable letter target (`track.send.a` … `track.send.h`).
    pub fn send_at(&self, index: usize, tick: usize) -> f32 {
        let Some(target) = crate::targets::track_send_target(index) else {
            return 0.0;
        };
        self.value_at(target, tick, self.send(index))
            .clamp(0.0, 1.0)
    }

    /// THE THREE-LAYER READ, and the only sanctioned way to ask what a
    /// parameter is actually worth at a moment:
    ///
    /// ```text
    /// knob   the stored static value           -> `base` when no curve
    /// curve  this track's automation at `tick` -> the composed base
    /// lock   the trig's additive offset        -> the composed offset
    /// ```
    ///
    /// The sum is `param_law::compose`'s business, not a device's. The
    /// returned `Composition` keeps both authorities separately so the UI
    /// can show the sum AS a sum.
    pub fn effective(
        &self,
        def: &crate::params::ParamDef,
        target: &str,
        tick: usize,
        knob: f32,
        lock: Option<f32>,
    ) -> Result<crate::param_law::Composition, crate::param_law::ComposeError> {
        crate::param_law::compose(def, self.value_at(target, tick, knob), lock)
    }
}

/// Application-owned song context shared by the arrangement and note views.
/// A tempo change in song time. The tempo holds CONSTANT until the next
/// mark — see `notes/20260831-midi-and-tempo-decisions.md` for why a ramp
/// is deliberately not the first shape: a prefix sum over constant
/// segments is exact where an integrated ramp drifts along a long
/// timeline, and ramps stay strictly additive later.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TempoMark {
    pub tick: usize,
    pub bpm: f64,
}

/// A meter change in song time. Bars are counted from the mark onward.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MeterMark {
    pub tick: usize,
    pub numerator: u32,
    pub denominator: u32,
}

/// A return bus in the canonical Song.
///
/// Its effects chain remains projection-only for v1: the Song owns these
/// four mixer facts, and `project_song` writes them onto the legacy bus while
/// leaving that bus's effects intact.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct ReturnTrack {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mute: bool,
    #[serde(default = "unity")]
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
}

impl Default for ReturnTrack {
    fn default() -> Self {
        Self {
            name: String::new(),
            mute: false,
            volume: 1.0,
            pan: 0.0,
        }
    }
}

impl ReturnTrack {
    pub const MAX: usize = 8;

    pub fn letter(index: usize) -> char {
        if index < Self::MAX {
            (b'A' + index as u8) as char
        } else {
            '?'
        }
    }

    pub fn new(index: usize) -> Self {
        Self {
            name: format!("Return {}", Self::letter(index)),
            ..Self::default()
        }
    }
}

/// A tempo a musician could actually mean. Marks outside this are dropped
/// rather than trusted: a zero or NaN bpm would make a tick worth
/// infinity samples and hang the compile that tried to stamp it.
const MIN_BPM: f64 = 1.0;
const MAX_BPM: f64 = 1000.0;

fn usable_bpm(bpm: f64) -> bool {
    bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm)
}

/// How many scenes a new session offers: enough rows that a sketch has
/// room, few enough that they fit one screen.
pub const SESSION_SCENES: usize = 8;

/// What a session slot holds. One variant today; an audio clip joins it
/// when a slot can be given a sample, and the enum is what keeps that
/// from being a second slot type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Clip {
    Pattern(PatternId),
}

/// One clip in one scene, on one track. Keyed by [`TrackId`] rather than
/// track position so a reordered strip does not move clips between
/// tracks, and stored as a list rather than a map so the document stays
/// plain RON.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Slot {
    pub track: TrackId,
    pub clip: Clip,
}

/// One row of the session: a clip per track, at most, launched as a unit.
/// Launching is not modelled yet — a scene is a place to keep clips
/// before it is a thing to fire.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Scene {
    #[serde(default)]
    pub slots: Vec<Slot>,
}

impl Scene {
    pub fn clip(&self, track: TrackId) -> Option<Clip> {
        self.slots
            .iter()
            .find(|slot| slot.track == track)
            .map(|slot| slot.clip)
    }
}

/// The session: scenes down, tracks across, a clip where they meet.
/// Document data beside the arrangement, not a view of it — a clip in a
/// slot is not on the timeline until something places it there.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Session {
    pub scenes: Vec<Scene>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            scenes: vec![Scene::default(); SESSION_SCENES],
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Song {
    pub tracks: Vec<Track>,
    pub patterns: Vec<Pattern>,
    /// The master's fader, as LINEAR amplitude. 1.0 is unity.
    ///
    /// One number and no pan: a master pan is a thing to reach for when
    /// something upstream is wrong, and the mixer already gives every
    /// track one. Absent from a project written before the master had a
    /// fader means unity, which is what those projects sounded like.
    #[serde(default = "unity")]
    pub master: f32,
    /// Return buses in letter order. Their effects live on the legacy
    /// projection for v1; these mixer facts are canonical here.
    #[serde(default)]
    pub returns: Vec<ReturnTrack>,
    /// Tempo changes in song time, sorted by tick.
    ///
    /// EMPTY MEANS the single global tempo the transport has always
    /// carried, so every project written before the map keeps its
    /// meaning and `SetTempo` remains the no-marks case.
    ///
    /// Tempo is its OWN authority, never an automation target: an
    /// envelope is read *at* a tick, and tempo is what decides what a
    /// tick is worth. It is not a value in a unit — it is the unit's
    /// exchange rate.
    #[serde(default)]
    pub tempo: Vec<TempoMark>,
    /// Meter changes in song time, sorted by tick. Empty means the
    /// transport's single global signature.
    #[serde(default)]
    pub meter: Vec<MeterMark>,
    /// The session's scenes and their clips. Absent from documents
    /// written before the session existed means an empty default
    /// session, which reads exactly as it did before.
    #[serde(default)]
    pub session: Session,
    /// The project-level harmonic context. Degree anchors resolve against
    /// it; absolute anchors never read it.
    pub key: Key,
    /// The next [`TrackId`] to mint. MONOTONIC: a removed track's id is
    /// never handed out again, so anything that held it — a scene slot,
    /// an undo step, a frame's memory — can only ever point at the track
    /// it meant or at nothing. Zero, which is what every document written
    /// before removal existed carries, means "one past the highest in
    /// use", exactly as it always did.
    #[serde(default)]
    pub next_track_id: u64,
}

impl Default for Song {
    fn default() -> Self {
        let pattern = Pattern::default();
        let pattern_id = pattern.id;
        Self {
            master: 1.0,
            tracks: vec![Track {
                id: TrackId(1),
                name: "Instrument 01".to_owned(),
                kind: TrackKind::Instrument,
                blocks: vec![PatternBlock {
                    id: BlockId(1),
                    pattern_id,
                    start_tick: 0,
                    length_ticks: DEFAULT_PATTERN_TICKS,
                }],
                muted: false,
                solo: false,
                pitch_authority: PitchAuthority::default(),
                audio_blocks: Vec::new(),
                automation: Vec::new(),
                volume: 1.0,
                pan: 0.0,
                sends: Vec::new(),
                is_group: false,
                folded: false,
                chain: Vec::new(),
                depth: 0,
                input: TrackInput::default(),
                monitor: Monitor::default(),
                armed: false,
            }],
            patterns: vec![pattern],
            returns: Vec::new(),
            tempo: Vec::new(),
            meter: Vec::new(),
            session: Session::default(),
            key: default_key(),
            next_track_id: 2,
        }
    }
}

impl Song {
    /// Append a new, empty track of `kind` and return its id.
    ///
    /// Document editing, not scheduling: an empty track carries no events,
    /// no compiled chunks and no nodes, so none of the five rules in
    /// `notes/20260823-sequencing-contract.md` are in play. It becomes
    /// audible only once something is placed on it.
    ///
    /// The id comes off [`Self::next_track_id`], which only ever goes up:
    /// a removed track's id is never reused, so a scene slot or an undo
    /// step that still names it names nothing rather than a stranger. A
    /// document from before the counter existed carries zero, and reads
    /// as "one past the highest in use" — the same id it always minted.
    pub fn add_track(&mut self, kind: TrackKind) -> TrackId {
        let id = TrackId(self.mint_track_id());
        // Numbered within its own kind, so adding an audio track does not
        // depend on how many instrument tracks happen to exist.
        let ordinal = self
            .tracks
            .iter()
            .filter(|track| track.kind == kind)
            .count()
            .saturating_add(1);
        let name = match kind {
            TrackKind::Instrument => format!("Instrument {ordinal:02}"),
            TrackKind::Audio => format!("Audio {ordinal:02}"),
        };
        self.tracks.push(Track {
            id,
            name,
            kind,
            blocks: Vec::new(),
            muted: false,
            solo: false,
            pitch_authority: PitchAuthority::default(),
            audio_blocks: Vec::new(),
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            sends: Vec::new(),
            is_group: false,
            folded: false,
            chain: Vec::new(),
            depth: 0,
            input: TrackInput::default(),
            monitor: Monitor::default(),
            armed: false,
        });
        id
    }

    /// The next unused track id, and the counter moved past it.
    fn mint_track_id(&mut self) -> u64 {
        let past_highest = self
            .tracks
            .iter()
            .map(|track| track.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let id = self.next_track_id.max(past_highest);
        self.next_track_id = id.saturating_add(1);
        id
    }

    /// Take the track at `index` out of the song, and every scene slot
    /// that pointed at it with it. `None` means there is no such track.
    ///
    /// The slots go too because a slot is keyed by id and the id is never
    /// minted again: a slot left behind would be a clip filed under a
    /// track that no longer exists, invisible to every surface and still
    /// in the file. The patterns stay — a pattern is content, and content
    /// is not destroyed by removing a place it was kept.
    pub fn remove_track(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let track = self.tracks.remove(index);
        for scene in &mut self.session.scenes {
            scene.slots.retain(|slot| slot.track != track.id);
        }
        self.normalize_group_depths();
        Some(track)
    }

    /// Swap the track at `index` with its neighbour. `false` means it was
    /// already at that end of the strip, or there is no such track.
    ///
    /// Its clips travel with it for free: a slot names a track by id, so
    /// nothing in the session has to be told the strip was reordered.
    pub fn move_track(&mut self, index: usize, later: bool) -> bool {
        let to = if later {
            index + 1
        } else {
            index.wrapping_sub(1)
        };
        if index >= self.tracks.len() || to >= self.tracks.len() {
            return false;
        }
        self.tracks.swap(index, to);
        self.normalize_group_depths();
        true
    }

    /// Give the track at `index` a new name. `false` means there is no
    /// such track, or the name was blank — a track has to be called
    /// SOMETHING, or the strip has a column nobody can refer to.
    pub fn rename_track(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        match self.tracks.get_mut(index) {
            Some(track) => {
                track.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Normalize the positional group stack at a green-zone ownership
    /// boundary (load or edit), never in the C1 projection. Repairing only
    /// the twin would leave the Song and undo history holding illegal data.
    pub fn normalize_group_depths(&mut self) {
        let mut allowed = 0;
        for track in &mut self.tracks {
            track.depth = track.depth.min(allowed).min(MAX_GROUP_DEPTH);
            track.folded &= track.is_group;
            allowed = if track.is_group {
                track.depth.saturating_add(1)
            } else {
                track.depth
            };
        }
    }

    /// The group this lane belongs to: the nearest lane above it that is a
    /// group one level shallower.
    ///
    /// Membership is POSITION, never a pointer — which is why this is a
    /// search rather than a field, and why it cannot go stale.
    pub fn parent_group(&self, index: usize) -> Option<usize> {
        let depth = self.tracks.get(index)?.depth;
        if depth == 0 {
            return None;
        }
        self.tracks[..index]
            .iter()
            .rposition(|track| track.is_group && track.depth.saturating_add(1) == depth)
    }

    /// The contiguous run of lanes belonging to the group at `index`, or an
    /// empty range if that lane is not a group.
    pub fn group_members(&self, index: usize) -> std::ops::Range<usize> {
        let Some(group) = self.tracks.get(index).filter(|track| track.is_group) else {
            return index..index;
        };
        let mut end = index + 1;
        while self
            .tracks
            .get(end)
            .is_some_and(|track| track.depth > group.depth)
        {
            end += 1;
        }
        index + 1..end
    }

    /// Whether this lane is silenced where it stands: by its own mute, or by
    /// any group above it. A muted group takes its members with it.
    pub fn muted_in_place(&self, index: usize) -> bool {
        let mut at = index;
        loop {
            if self.tracks.get(at).is_some_and(|track| track.muted) {
                return true;
            }
            match self.parent_group(at) {
                Some(parent) => at = parent,
                None => return false,
            }
        }
    }

    /// Whether solo reaches this lane: its own, an ancestor group's (a
    /// soloed group sounds what is inside it), or a member's — a soloed lane
    /// keeps the bus above it, because otherwise nothing would carry it to
    /// the master.
    pub fn solo_in_scope(&self, index: usize) -> bool {
        let Some(track) = self.tracks.get(index) else {
            return false;
        };
        if track.solo {
            return true;
        }
        let mut at = index;
        while let Some(parent) = self.parent_group(at) {
            if self.tracks[parent].solo {
                return true;
            }
            at = parent;
        }
        self.group_members(index)
            .any(|member| self.solo_in_scope(member))
    }

    /// Whether any lane is soloed, which is what puts the song in the mode
    /// where silence is the default.
    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|track| track.solo)
    }

    /// **Whether this lane actually sounds** — the one answer to that
    /// question, and the reason it lives on the model rather than in a
    /// frame or a bridge.
    ///
    /// Mute and solo are not two independent switches: a lane can be
    /// unmuted and still silent because something else is soloed, and that
    /// third state is one a mixer has to be able to draw. Anything asking
    /// "is this sounding" — a compiler building a graph, a strip drawing a
    /// meter — asks HERE, so the surface and the sound cannot disagree.
    pub fn audible(&self, index: usize) -> bool {
        !self.muted_in_place(index) && (!self.any_solo() || self.solo_in_scope(index))
    }

    /// Put every chain back into legal shape at a green-zone ownership
    /// boundary (load or edit), the way [`Self::normalize_group_depths`]
    /// does for the group stack.
    ///
    /// Two rules, and both are about WHERE sound comes from:
    ///
    /// - An instrument heads its chain. A device that makes sound placed
    ///   after one that shapes it would have the shaping happen before
    ///   there was anything to shape.
    /// - One instrument per track, and none at all on an audio track,
    ///   whose sound comes from the clips on it. A second instrument is
    ///   dropped rather than silenced, because a device that is present
    ///   and inaudible with no way to say why is worse than one that is
    ///   gone.
    ///
    /// Effects keep their order exactly. Repairing the head is not a
    /// licence to rearrange what a musician put in a particular sequence.
    pub fn normalize_chains(&mut self) {
        for track in &mut self.tracks {
            let audio = track.kind == TrackKind::Audio;
            let mut instrument = None;
            let mut effects = Vec::with_capacity(track.chain.len());
            for device in track.chain.drain(..) {
                if !device.is_instrument() {
                    effects.push(device);
                } else if !audio && instrument.is_none() {
                    instrument = Some(device);
                }
            }
            track.chain = instrument.into_iter().chain(effects).collect();
        }
    }

    /// Put `kind` on `track`, at the head if it is an instrument and at
    /// the tail if it shapes sound. `None` means the track does not exist,
    /// or an instrument was offered to an audio track.
    ///
    /// The id is minted across the WHOLE SONG rather than per track, so a
    /// device carries its identity when it is moved to another track.
    pub fn add_device(&mut self, track: usize, kind: DeviceKind) -> Option<DeviceId> {
        let audio = self.tracks.get(track)?.kind == TrackKind::Audio;
        let instrument = kind.spec().instrument;
        if audio && instrument {
            return None;
        }
        let device = Device::new(DeviceId(self.mint_device_id()), kind);
        let at = self.tracks.get(track)?.chain.len();
        self.place_device(track, at, device)
    }

    /// Put a device that already has settings — one taken off another
    /// chain, or the same one — onto `track` at position `at`, and give
    /// it a fresh id. `None` means the track does not exist, or the
    /// device is an instrument offered to an audio track.
    ///
    /// A FRESH id, always: the device may be a copy of one still in the
    /// song, and two devices with one id would be one device to every
    /// letter addressed to it. What carries over is what the performer
    /// set — the kind, the bypass, the edits, the sample.
    ///
    /// `at` is clamped: an instrument goes to the head whatever was asked,
    /// replacing the one there, and an effect never lands in front of the
    /// head. Past the tail means the tail.
    pub fn insert_device(
        &mut self,
        track: usize,
        at: usize,
        mut device: Device,
    ) -> Option<DeviceId> {
        let audio = self.tracks.get(track)?.kind == TrackKind::Audio;
        if audio && device.is_instrument() {
            return None;
        }
        device.id = DeviceId(self.mint_device_id());
        self.place_device(track, at, device)
    }

    /// One past the highest device id anywhere in the song.
    fn mint_device_id(&self) -> u64 {
        self.tracks
            .iter()
            .flat_map(|track| track.chain.iter())
            .map(|device| device.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// The one place a device joins a chain, so the head rule has one
    /// home: an instrument REPLACES the head, and an effect lands at `at`
    /// clamped behind the head and inside the chain.
    fn place_device(&mut self, track: usize, at: usize, device: Device) -> Option<DeviceId> {
        let id = device.id;
        let chain = &mut self.tracks.get_mut(track)?.chain;
        if device.is_instrument() {
            // A second instrument REPLACES the first rather than joining
            // it: a track sounds one voice, and the head is that voice.
            chain.retain(|existing| !existing.is_instrument());
            chain.insert(0, device);
        } else {
            let first = usize::from(chain.first().is_some_and(Device::is_instrument));
            let at = at.clamp(first, chain.len());
            chain.insert(at, device);
        }
        Some(id)
    }

    /// Take a device off a track. `None` means there was no such device.
    pub fn remove_device(&mut self, track: usize, id: DeviceId) -> Option<Device> {
        let chain = &mut self.tracks.get_mut(track)?.chain;
        let at = chain.iter().position(|device| device.id == id)?;
        Some(chain.remove(at))
    }

    /// Move a device one place along its chain. `false` means it was
    /// already at that end, or the move would put an instrument behind an
    /// effect — the head is not a position a reorder may vacate.
    pub fn move_device(&mut self, track: usize, id: DeviceId, later: bool) -> bool {
        let Some(chain) = self.tracks.get_mut(track).map(|track| &mut track.chain) else {
            return false;
        };
        let Some(at) = chain.iter().position(|device| device.id == id) else {
            return false;
        };
        let to = if later { at + 1 } else { at.wrapping_sub(1) };
        if to >= chain.len() {
            return false;
        }
        if chain[at].is_instrument() || chain[to].is_instrument() {
            return false;
        }
        chain.swap(at, to);
        true
    }

    /// The device with this id, wherever it is.
    pub fn device(&self, id: DeviceId) -> Option<&Device> {
        self.tracks
            .iter()
            .flat_map(|track| track.chain.iter())
            .find(|device| device.id == id)
    }

    pub fn device_mut(&mut self, id: DeviceId) -> Option<&mut Device> {
        self.tracks
            .iter_mut()
            .flat_map(|track| track.chain.iter_mut())
            .find(|device| device.id == id)
    }

    /// Sanitize mixer data arriving from a file. Edits already enforce the
    /// same bounds; doing it once on load keeps the projection a pure copy.
    pub fn normalize_mixer(&mut self) {
        self.returns.truncate(ReturnTrack::MAX);
        for track in &mut self.tracks {
            track.sends.truncate(self.returns.len());
            for send in &mut track.sends {
                *send = if send.is_finite() {
                    send.clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
        }
    }

    /// Add the next lettered return. The caller turns `None` into a visible
    /// refusal; this model never silently exceeds A through H.
    pub fn add_return(&mut self) -> Option<usize> {
        if self.returns.len() >= ReturnTrack::MAX {
            return None;
        }
        let index = self.returns.len();
        self.returns.push(ReturnTrack::new(index));
        Some(index)
    }

    /// Take a return away, and every send that pointed at it.
    ///
    /// The sends shift DOWN rather than being cleared, because the list is
    /// positional: removing A must make what was B into A on every track at
    /// once, or every send past the gap would silently start feeding the
    /// wrong bus. Lettered envelopes are shifted with the same physical bus;
    /// the deleted bus's own envelope is the only one discarded.
    pub fn remove_return(&mut self, index: usize) -> bool {
        if index >= self.returns.len() {
            return false;
        }
        self.returns.remove(index);
        for track in &mut self.tracks {
            if index < track.sends.len() {
                track.sends.remove(index);
            }

            if let Some(deleted) = crate::targets::track_send_target(index) {
                track
                    .automation
                    .retain(|envelope| envelope.target != deleted);
            }
            for shifted in index + 1..=self.returns.len() {
                let Some(from) = crate::targets::track_send_target(shifted) else {
                    continue;
                };
                let Some(to) = crate::targets::track_send_target(shifted - 1) else {
                    continue;
                };
                for envelope in &mut track.automation {
                    if envelope.target == from {
                        envelope.target = to.to_owned();
                    }
                }
            }
        }
        true
    }

    /// The tempo in force at `tick`, or `fallback` before the first mark
    /// (and everywhere, when the map is empty).
    pub fn bpm_at(&self, tick: usize, fallback: f64) -> f64 {
        self.tempo
            .iter()
            .filter(|mark| mark.tick <= tick && usable_bpm(mark.bpm))
            .next_back()
            .map_or(fallback, |mark| mark.bpm)
    }

    /// The meter in force at `tick`, or `fallback` before the first mark.
    pub fn meter_at(&self, tick: usize, fallback: (u32, u32)) -> (u32, u32) {
        self.meter
            .iter()
            .filter(|mark| mark.tick <= tick && mark.numerator > 0 && mark.denominator > 0)
            .next_back()
            .map_or(fallback, |mark| (mark.numerator, mark.denominator))
    }

    /// Place or move a tempo mark. A mark already at `tick` takes the new
    /// tempo; an unusable tempo is refused rather than stored.
    pub fn set_tempo_mark(&mut self, tick: usize, bpm: f64) -> bool {
        if !usable_bpm(bpm) {
            return false;
        }
        let at = self.tempo.partition_point(|mark| mark.tick < tick);
        if self.tempo.get(at).is_some_and(|mark| mark.tick == tick) {
            self.tempo[at].bpm = bpm;
        } else {
            self.tempo.insert(at, TempoMark { tick, bpm });
        }
        true
    }

    pub fn remove_tempo_mark(&mut self, tick: usize) -> bool {
        let Some(at) = self.tempo.iter().position(|mark| mark.tick == tick) else {
            return false;
        };
        self.tempo.remove(at);
        true
    }

    /// Place or move a meter mark. A degenerate signature is refused.
    pub fn set_meter_mark(&mut self, tick: usize, numerator: u32, denominator: u32) -> bool {
        if numerator == 0 || denominator == 0 {
            return false;
        }
        let at = self.meter.partition_point(|mark| mark.tick < tick);
        let mark = MeterMark {
            tick,
            numerator,
            denominator,
        };
        if self
            .meter
            .get(at)
            .is_some_and(|existing| existing.tick == tick)
        {
            self.meter[at] = mark;
        } else {
            self.meter.insert(at, mark);
        }
        true
    }

    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|pattern| pattern.id == id)
    }

    pub fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|pattern| pattern.id == id)
    }

    /// Insert an empty pattern block when the requested region is free.
    /// Arrangement editing is green-zone work; the audio side will later
    /// receive a newly compiled immutable sequence rather than these vectors.
    /// A new, empty pattern with the next free id and the next serial
    /// name. The one place a pattern is minted, whether for the timeline
    /// or for a session slot.
    fn allocate_pattern(&mut self) -> Option<PatternId> {
        let pattern_number = self.patterns.len().checked_add(1)?;
        let pattern_id = PatternId(
            self.patterns
                .iter()
                .map(|pattern| pattern.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)?,
        );
        self.patterns
            .push(Pattern::empty(pattern_id, format!("P{pattern_number:02}")));
        Some(pattern_id)
    }

    /// The clip at `scene` on the track at `track_index`, if either exists
    /// and the slot holds one.
    pub fn slot_clip(&self, track_index: usize, scene: usize) -> Option<Clip> {
        let track = self.tracks.get(track_index)?;
        self.session.scenes.get(scene)?.clip(track.id)
    }

    /// Fill an empty session slot with a new, empty pattern and return
    /// its id. Refused — `None`, nothing changed — when there is no such
    /// track or scene, when the track is not an instrument (an audio slot
    /// needs a sample, which is the browser's verb), or when the slot is
    /// already taken: a clip is never silently replaced.
    ///
    /// Document editing, not scheduling: the new pattern has no trigs and
    /// no placement, so nothing in the sequencing contract is in play.
    pub fn fill_slot(&mut self, track_index: usize, scene: usize) -> Option<PatternId> {
        let track = self.tracks.get(track_index)?;
        if track.kind != TrackKind::Instrument {
            return None;
        }
        let track_id = track.id;
        if self.session.scenes.get(scene)?.clip(track_id).is_some() {
            return None;
        }
        let pattern_id = self.allocate_pattern()?;
        self.session.scenes.get_mut(scene)?.slots.push(Slot {
            track: track_id,
            clip: Clip::Pattern(pattern_id),
        });
        Some(pattern_id)
    }

    /// Empty a session slot and return what it held. The pattern itself
    /// stays in the song: a clip may be the only thing referring to it,
    /// but reclaiming patterns is a policy for the document as a whole,
    /// not a side effect of clearing one place they were kept.
    pub fn clear_slot(&mut self, track_index: usize, scene: usize) -> Option<Clip> {
        let track_id = self.tracks.get(track_index)?.id;
        let scene = self.session.scenes.get_mut(scene)?;
        let index = scene.slots.iter().position(|slot| slot.track == track_id)?;
        Some(scene.slots.remove(index).clip)
    }

    pub fn create_pattern_block(
        &mut self,
        track_index: usize,
        start_tick: usize,
        length_ticks: usize,
    ) -> Option<PatternId> {
        let track = self.tracks.get(track_index)?;
        if track.kind != TrackKind::Instrument || length_ticks == 0 {
            return None;
        }
        let end_tick = start_tick.checked_add(length_ticks)?;
        if track.blocks.iter().any(|block| {
            let block_end = block.start_tick.saturating_add(block.length_ticks);
            block.start_tick < end_tick && start_tick < block_end
        }) {
            return None;
        }

        let block_id = BlockId(
            self.tracks
                .iter()
                .flat_map(|track| track.blocks.iter())
                .map(|block| block.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)?,
        );
        let pattern_id = self.allocate_pattern()?;
        let track = self.tracks.get_mut(track_index)?;
        track.blocks.push(PatternBlock {
            id: block_id,
            pattern_id,
            start_tick,
            length_ticks,
        });
        track.blocks.sort_by_key(|block| block.start_tick);
        Some(pattern_id)
    }

    /// Delete every block intersecting a rectangular track/time selection.
    /// Patterns no longer referenced by any block are collected as project
    /// data; shared patterns survive until their final placement is removed.
    pub fn delete_blocks_in_region(
        &mut self,
        first_track: usize,
        last_track: usize,
        start_tick: usize,
        end_tick: usize,
    ) -> usize {
        if start_tick >= end_tick || self.tracks.is_empty() {
            return 0;
        }
        let first = first_track.min(self.tracks.len() - 1);
        let last = last_track.min(self.tracks.len() - 1);
        let mut deleted = 0;
        for track in &mut self.tracks[first.min(last)..=first.max(last)] {
            let before = track.blocks.len();
            track.blocks.retain(|block| {
                let block_end = block.start_tick.saturating_add(block.length_ticks);
                !(block.start_tick < end_tick && start_tick < block_end)
            });
            deleted += before - track.blocks.len();
        }
        if deleted > 0 {
            self.patterns.retain(|pattern| {
                self.tracks.iter().any(|track| {
                    track
                        .blocks
                        .iter()
                        .any(|block| block.pattern_id == pattern.id)
                })
            });
        }
        deleted
    }

    pub fn move_pattern_block(
        &mut self,
        id: BlockId,
        target_track: usize,
        start_tick: usize,
    ) -> bool {
        let Some((source_track, source_index)) = self.block_location(id) else {
            return false;
        };
        let block = self.tracks[source_track].blocks[source_index].clone();
        let Some(end_tick) = start_tick.checked_add(block.length_ticks) else {
            return false;
        };
        let Some(target) = self.tracks.get(target_track) else {
            return false;
        };
        if target.kind != TrackKind::Instrument
            || target.blocks.iter().any(|other| {
                other.id != id
                    && other.start_tick < end_tick
                    && start_tick < other.start_tick.saturating_add(other.length_ticks)
            })
        {
            return false;
        }
        let mut block = self.tracks[source_track].blocks.remove(source_index);
        block.start_tick = start_tick;
        self.tracks[target_track].blocks.push(block);
        self.tracks[target_track]
            .blocks
            .sort_by_key(|block| block.start_tick);
        true
    }

    pub fn duplicate_pattern_block(
        &mut self,
        id: BlockId,
        target_track: usize,
        start_tick: usize,
    ) -> Option<BlockId> {
        let (source_track, source_index) = self.block_location(id)?;
        let source = self.tracks[source_track].blocks[source_index].clone();
        let source_pattern = self.pattern(source.pattern_id)?.clone();
        self.adopt_pattern(
            source_pattern,
            target_track,
            start_tick,
            source.length_ticks,
        )
    }

    /// Place a pattern carried in from elsewhere — a register put, whose
    /// source of truth is the deep copy in the register (the original
    /// block may be long gone). The pattern is adopted with a fresh
    /// identity and a new block at the target; refuses on overlap, a
    /// non-instrument track, or a missing track.
    pub fn adopt_pattern(
        &mut self,
        mut pattern: Pattern,
        target_track: usize,
        start_tick: usize,
        length_ticks: usize,
    ) -> Option<BlockId> {
        let end_tick = start_tick.checked_add(length_ticks)?;
        let target = self.tracks.get(target_track)?;
        if target.kind != TrackKind::Instrument
            || target.blocks.iter().any(|block| {
                block.start_tick < end_tick
                    && start_tick < block.start_tick.saturating_add(block.length_ticks)
            })
        {
            return None;
        }
        let pattern_id = PatternId(
            self.patterns
                .iter()
                .map(|pattern| pattern.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)?,
        );
        let block_id = BlockId(
            self.tracks
                .iter()
                .flat_map(|track| track.blocks.iter())
                .map(|block| block.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)?,
        );
        pattern.id = pattern_id;
        pattern.name = format!("P{:02}", self.patterns.len() + 1);
        self.patterns.push(pattern);
        self.tracks[target_track].blocks.push(PatternBlock {
            id: block_id,
            pattern_id,
            start_tick,
            length_ticks,
        });
        self.tracks[target_track]
            .blocks
            .sort_by_key(|block| block.start_tick);
        Some(block_id)
    }

    pub fn resize_pattern_block(
        &mut self,
        id: BlockId,
        start_tick: usize,
        length_ticks: usize,
    ) -> bool {
        if length_ticks == 0 {
            return false;
        }
        let Some((track_index, block_index)) = self.block_location(id) else {
            return false;
        };
        let Some(end_tick) = start_tick.checked_add(length_ticks) else {
            return false;
        };
        if self.tracks[track_index].blocks.iter().any(|other| {
            other.id != id
                && other.start_tick < end_tick
                && start_tick < other.start_tick.saturating_add(other.length_ticks)
        }) {
            return false;
        }
        let block = &mut self.tracks[track_index].blocks[block_index];
        block.start_tick = start_tick;
        block.length_ticks = length_ticks;
        self.tracks[track_index]
            .blocks
            .sort_by_key(|block| block.start_tick);
        true
    }

    /// Land recorded sound on a track.
    ///
    /// The one place an audio block comes into existence. Refuses through
    /// the `Result` rather than doing nothing: a silent no-op is
    /// indistinguishable from a broken key.
    ///
    /// LENGTH IS NOT GUESSED. It comes from the file's real duration
    /// converted through the TEMPO TABLE — not a bar count, not a
    /// default. A sound landed at the wrong length sounds wrong the
    /// moment the tempo changes, and the bug then looks like a tempo bug
    /// rather than an import bug.
    pub fn place_audio(
        &mut self,
        track_index: usize,
        start_tick: usize,
        name: String,
        source: crate::audio_source::AudioSource,
        tempo: &crate::tempo::TempoTable,
    ) -> Result<BlockId, LandRefusal> {
        let Some(track) = self.tracks.get(track_index) else {
            return Err(LandRefusal::NoTrack);
        };
        if track.kind != TrackKind::Audio {
            return Err(LandRefusal::NotAnAudioTrack);
        }
        if source.sample_rate == 0 || source.source_frames == 0 {
            return Err(LandRefusal::Unreadable);
        }
        let seconds = source.source_frames as f64 / f64::from(source.sample_rate);
        let length_ticks = tempo.ticks_for_seconds(start_tick, seconds);
        let Some(end_tick) = start_tick.checked_add(length_ticks) else {
            return Err(LandRefusal::Unreadable);
        };
        // Both lists: a lane occupied by a pattern is occupied.
        if track.occupied(start_tick, end_tick) {
            return Err(LandRefusal::Occupied);
        }
        let id = BlockId(
            self.tracks
                .iter()
                .flat_map(|track| {
                    track
                        .blocks
                        .iter()
                        .map(|block| block.id.0)
                        .chain(track.audio_blocks.iter().map(|block| block.id.0))
                })
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        let track = &mut self.tracks[track_index];
        track.audio_blocks.push(AudioBlock {
            id,
            name,
            start_tick,
            length_ticks,
            source,
            loop_brace: None,
        });
        track.audio_blocks.sort_by_key(|block| block.start_tick);
        Ok(id)
    }

    pub fn pattern_block(&self, id: BlockId) -> Option<(usize, &PatternBlock)> {
        let (track, index) = self.block_location(id)?;
        Some((track, &self.tracks[track].blocks[index]))
    }

    fn block_location(&self, id: BlockId) -> Option<(usize, usize)> {
        self.tracks.iter().enumerate().find_map(|(track, lane)| {
            lane.blocks
                .iter()
                .position(|block| block.id == id)
                .map(|index| (track, index))
        })
    }
}

impl Pattern {
    pub fn empty(id: PatternId, name: String) -> Self {
        Self {
            id,
            name,
            length_ticks: DEFAULT_PATTERN_TICKS,
            trigs: vec![Trig::default(); PATTERN_STEPS],
        }
    }

    pub fn trig(&self, step: usize) -> &Trig {
        &self.trigs[step % PATTERN_STEPS]
    }

    pub fn trig_mut(&mut self, step: usize) -> &mut Trig {
        &mut self.trigs[step % PATTERN_STEPS]
    }

    pub fn set_primary(&mut self, step: usize, note: Note) {
        self.trig_mut(step).set_primary(note);
    }

    pub fn add_tone(&mut self, step: usize, note: Note) {
        self.trig_mut(step).add_tone(note);
    }

    pub fn toggle(&mut self, step: usize, default_note: Note) {
        let trig = self.trig_mut(step);
        if trig.notes.is_empty() {
            trig.set_primary(default_note);
        } else {
            trig.enabled = !trig.enabled;
        }
    }

    pub fn clear(&mut self, step: usize) {
        self.trig_mut(step).clear();
    }

    /// Apply one sequence intent, tick-accurately. A tick addresses a
    /// step and an offset into it: `tick / 12` is the step, `tick % 12`
    /// the note's `micro_ticks`. So a 1/32 grid, a triplet grid, or a
    /// sixty-fourth all land where they were spoken, and the projection
    /// plays them there — it already adds the micro offset to the step.
    /// What stays per STEP is the trig's own state, `enabled` and
    /// `probability`: notes finer than a sixteenth share the sixteenth's
    /// gate and condition.
    ///
    /// Returns a notice when the intent could not be honoured — the
    /// caller shows it, never swallows it.
    ///
    /// Document editing, not scheduling: what changes here is compiled
    /// into immutable chunks by the green-zone compiler afterwards, so
    /// none of the five contract rules are in play at this layer.
    pub fn apply(&mut self, intent: &crate::intent::sequence::Intent) -> Option<&'static str> {
        use crate::intent::sequence::Intent;
        let tick = match *intent {
            Intent::ResizeClip { delta_ticks } => {
                let next = self
                    .length_ticks
                    .saturating_add_signed(delta_ticks)
                    .clamp(PATTERN_STEP_TICKS, DEFAULT_PATTERN_TICKS);
                if next == self.length_ticks {
                    return Some("clip resize blocked at the pattern edge");
                }
                self.length_ticks = next;
                return None;
            }
            Intent::Toggle { tick, .. }
            | Intent::SetPrimary { tick, .. }
            | Intent::Clear { tick }
            | Intent::RemoveNote { tick, .. }
            | Intent::Nudge { tick, .. }
            | Intent::NudgeNote { tick, .. }
            | Intent::Transpose { tick, .. }
            | Intent::TransposeNote { tick, .. }
            | Intent::Resize { tick, .. }
            | Intent::ResizeNote { tick, .. }
            | Intent::AddNote { tick, .. }
            | Intent::SetProbability { tick, .. }
            | Intent::AdjustVelocity { tick, .. }
            | Intent::AdjustNoteVelocity { tick, .. }
            | Intent::SetNoteMuted { tick, .. } => tick,
        };
        let (step, micro) = Self::address(tick);
        if step >= PATTERN_STEPS {
            return Some("sequence step is outside the pattern");
        }
        let at = |note: &Note| note.micro_ticks == micro;
        match *intent {
            Intent::ResizeClip { .. } => {}
            // A note exactly here gates the step; nothing here means a
            // new note here, joining whatever else the step holds.
            Intent::Toggle {
                default_pitch,
                default_length_ticks,
                default_velocity,
                ..
            } => {
                let trig = self.trig_mut(step);
                if trig.notes.iter().any(at) {
                    trig.enabled = !trig.enabled;
                } else {
                    let mut note =
                        Note::with_pitch(default_pitch, default_length_ticks, default_velocity);
                    note.micro_ticks = micro;
                    trig.add_tone_at(note);
                }
            }
            // Entry replaces the note exactly here, or puts one here.
            Intent::SetPrimary {
                pitch,
                length_ticks,
                velocity,
                ..
            } => {
                let mut note = Note::with_pitch(pitch, length_ticks, velocity);
                note.micro_ticks = micro;
                let trig = self.trig_mut(step);
                if let Some(existing) = trig.notes.iter_mut().find(|note| at(note)) {
                    *existing = note;
                    trig.enabled = true;
                } else {
                    trig.add_tone_at(note);
                }
            }
            Intent::Clear { .. } => {
                let trig = self.trig_mut(step);
                trig.notes.retain(|note| !at(note));
                if trig.notes.is_empty() {
                    trig.clear();
                }
            }
            Intent::RemoveNote { pitch, .. } => {
                let trig = self.trig_mut(step);
                let before = trig.notes.len();
                trig.notes.retain(|note| !(at(note) && note.pitch == pitch));
                if trig.notes.len() == before {
                    return Some("delete: no note here");
                }
                if trig.notes.is_empty() {
                    trig.clear();
                }
            }
            Intent::AddNote {
                pitch,
                length_ticks,
                velocity,
                probability,
                ..
            } => {
                let mut note = Note::with_pitch(pitch, length_ticks, velocity);
                note.micro_ticks = micro;
                let trig = self.trig_mut(step);
                trig.add_tone_at(note);
                trig.probability = probability.clamp(0.01, 1.0);
            }
            Intent::SetProbability { probability, .. } => {
                self.trig_mut(step).probability = probability.clamp(0.01, 1.0);
            }
            Intent::AdjustVelocity { delta, .. } => {
                let trig = self.trig_mut(step);
                if !trig.notes.iter().any(at) {
                    return Some("velocity: no trig here");
                }
                for note in trig.notes.iter_mut().filter(|note| at(note)) {
                    note.velocity =
                        (isize::from(note.velocity).saturating_add(delta)).clamp(1, 127) as u8;
                }
            }
            Intent::AdjustNoteVelocity { pitch, delta, .. } => {
                let trig = self.trig_mut(step);
                let Some(note) = trig
                    .notes
                    .iter_mut()
                    .find(|note| at(note) && note.pitch == pitch)
                else {
                    return Some("velocity: no note here");
                };
                note.velocity =
                    (isize::from(note.velocity).saturating_add(delta)).clamp(1, 127) as u8;
            }
            Intent::SetNoteMuted { pitch, muted, .. } => {
                let trig = self.trig_mut(step);
                let Some(note) = trig
                    .notes
                    .iter_mut()
                    .find(|note| at(note) && note.pitch == pitch)
                else {
                    return Some("mute: no note here");
                };
                note.muted = muted;
            }
            Intent::Resize { delta_ticks, .. } => {
                let trig = self.trig_mut(step);
                if !trig.notes.iter().any(at) {
                    return Some("resize: no trig here");
                }
                for note in trig.notes.iter_mut().filter(|note| at(note)) {
                    note.length_ticks = note.length_ticks.saturating_add_signed(delta_ticks).max(1);
                }
            }
            Intent::ResizeNote {
                pitch, delta_ticks, ..
            } => {
                let trig = self.trig_mut(step);
                let Some(note) = trig
                    .notes
                    .iter_mut()
                    .find(|note| at(note) && note.pitch == pitch)
                else {
                    return Some("resize: no note here");
                };
                note.length_ticks = note.length_ticks.saturating_add_signed(delta_ticks).max(1);
            }
            // The notes exactly here move to exactly there, whatever grid
            // "there" is on. Refused, unchanged, past either end of the
            // pattern or onto a tick that already holds a note.
            Intent::Nudge { delta_ticks, .. } => {
                if !self.trig(step).notes.iter().any(at) {
                    return Some("nudge: no trig here");
                }
                let Some(target_tick) = isize::try_from(tick)
                    .ok()
                    .and_then(|tick| tick.checked_add(delta_ticks))
                    .filter(|target| *target >= 0)
                    .map(|target| target as usize)
                else {
                    return Some("nudge blocked at the pattern edge");
                };
                let (target_step, target_micro) = Self::address(target_tick);
                if target_step >= PATTERN_STEPS {
                    return Some("nudge blocked at the pattern edge");
                }
                if (target_step, target_micro) == (step, micro) {
                    return None;
                }
                if self
                    .trig(target_step)
                    .notes_at(target_micro)
                    .next()
                    .is_some()
                {
                    return Some("nudge blocked by an occupied step");
                }
                let source = self.trig_mut(step);
                let (moving, staying): (Vec<Note>, Vec<Note>) =
                    source.notes.drain(..).partition(at);
                let (was_enabled, probability) = (source.enabled, source.probability);
                source.notes = staying;
                if source.notes.is_empty() {
                    source.clear();
                }
                let target = self.trig_mut(target_step);
                let target_was_empty = target.notes.is_empty();
                for mut note in moving {
                    note.micro_ticks = target_micro;
                    target.add_tone_at(note);
                }
                // A moved trig keeps its own gate and condition when it
                // lands somewhere empty; joining an occupied step, it
                // takes that step's.
                if target_was_empty {
                    target.enabled = was_enabled;
                    target.probability = probability;
                }
            }
            // A roll cursor names one pitch within the tick. Move only
            // that note; unlike a stack nudge it may join a target tick
            // that holds other pitches, but never overwrite its own.
            Intent::NudgeNote {
                pitch, delta_ticks, ..
            } => {
                if !self
                    .trig(step)
                    .notes
                    .iter()
                    .any(|note| at(note) && note.pitch == pitch)
                {
                    return Some("nudge: no note here");
                }
                let Some(target_tick) = isize::try_from(tick)
                    .ok()
                    .and_then(|tick| tick.checked_add(delta_ticks))
                    .filter(|target| *target >= 0)
                    .map(|target| target as usize)
                else {
                    return Some("nudge blocked at the pattern edge");
                };
                let (target_step, target_micro) = Self::address(target_tick);
                if target_step >= PATTERN_STEPS {
                    return Some("nudge blocked at the pattern edge");
                }
                if (target_step, target_micro) == (step, micro) {
                    return None;
                }
                if self
                    .trig(target_step)
                    .notes_at(target_micro)
                    .any(|note| note.pitch == pitch)
                {
                    return Some("nudge blocked by an occupied note");
                }
                let (mut moving, was_enabled, probability) = {
                    let source = self.trig_mut(step);
                    let Some(index) = source
                        .notes
                        .iter()
                        .position(|note| at(note) && note.pitch == pitch)
                    else {
                        return Some("nudge: no note here");
                    };
                    let moving = source.notes.remove(index);
                    let state = (source.enabled, source.probability);
                    if source.notes.is_empty() {
                        source.clear();
                    }
                    (moving, state.0, state.1)
                };
                moving.micro_ticks = target_micro;
                let target = self.trig_mut(target_step);
                let target_was_empty = target.notes.is_empty();
                target.add_tone_at(moving);
                if target_was_empty {
                    target.enabled = was_enabled;
                    target.probability = probability;
                }
            }
            Intent::Transpose {
                delta_semitones, ..
            } => {
                let trig = self.trig_mut(step);
                if !trig.notes.iter().any(at) {
                    return Some("transpose: no trig here");
                }
                for note in trig.notes.iter_mut().filter(|note| at(note)) {
                    note.pitch = note.pitch.shifted_semitones(delta_semitones);
                }
            }
            Intent::TransposeNote {
                pitch,
                delta_semitones,
                ..
            } => {
                let target = pitch.shifted_semitones(delta_semitones);
                let trig = self.trig_mut(step);
                let Some(source) = trig
                    .notes
                    .iter()
                    .position(|note| at(note) && note.pitch == pitch)
                else {
                    return Some("transpose: no note here");
                };
                if trig
                    .notes
                    .iter()
                    .enumerate()
                    .any(|(index, note)| index != source && at(note) && note.pitch == target)
                {
                    return Some("transpose blocked by an occupied note");
                }
                trig.notes[source].pitch = target;
                trig.notes.sort_by(|a, b| a.pitch.stack_order(&b.pitch));
            }
        }
        None
    }

    /// A tick as the step it falls in and the offset into that step.
    pub fn address(tick: usize) -> (usize, i16) {
        (
            tick / PATTERN_STEP_TICKS,
            (tick % PATTERN_STEP_TICKS) as i16,
        )
    }

    /// Apply a frame's intents in order. The LAST notice wins, as the
    /// frame that first wrote this loop decided: one line of status per
    /// frame, and the most recent refusal is the one still true.
    pub fn apply_all(
        &mut self,
        intents: &[crate::intent::sequence::Intent],
    ) -> Option<&'static str> {
        let mut notice = None;
        for intent in intents {
            if let Some(refusal) = self.apply(intent) {
                notice = Some(refusal);
            }
        }
        notice
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::{Anchor, Interval, Key, Scale, Tuning};

    #[test]
    fn pattern_is_one_wrapped_sixty_four_step_sequence() {
        let pattern = Pattern::default();
        assert_eq!(PATTERN_STEPS, 64);
        assert_eq!(GRID_COLUMNS, 16);
        assert_eq!(GRID_ROWS, 4);
        assert!((0..PATTERN_STEPS).all(|step| pattern.trig(step).notes.is_empty()));
    }

    #[test]
    fn one_trig_can_hold_a_sorted_chord() {
        let mut pattern = Pattern::default();
        pattern.add_tone(15, Note::new(67, 12, 100));
        pattern.add_tone(15, Note::new(60, 12, 100));
        pattern.add_tone(15, Note::new(64, 12, 100));
        assert_eq!(
            pattern
                .trig(15)
                .notes
                .iter()
                .map(|note| note.pitch)
                .collect::<Vec<_>>(),
            [60, 64, 67].map(Pitch::from_midi)
        );
    }

    #[test]
    fn disabling_a_trig_preserves_its_notes_for_reactivation() {
        let mut pattern = Pattern::default();
        let note = Note::new(60, 12, 100);
        pattern.toggle(0, note.clone());
        pattern.toggle(0, note);
        assert!(!pattern.trig(0).enabled);
        assert_eq!(pattern.trig(0).notes.len(), 1);
    }

    #[test]
    fn default_song_places_the_workhorse_pattern_for_four_bars() {
        let song = Song::default();
        let block = &song.tracks[0].blocks[0];
        assert_eq!(block.length_ticks, DEFAULT_PATTERN_TICKS);
        assert_eq!(block.length_ticks / TICKS_PER_BEAT, 16);
        assert!(song.pattern(block.pattern_id).is_some());
    }

    #[test]
    fn song_round_trip_preserves_pitch_and_timing_bits() {
        let mut song = Song::default();
        song.key = Key::new(
            Tuning {
                reference_hz: 432.125,
                scale: Scale::new(
                    "seven-limit",
                    vec![
                        Interval::Ratio(9, 8),
                        Interval::Cents(386.313_713_864_834_8),
                        Interval::Ratio(3, 2),
                    ],
                    Interval::Ratio(2, 1),
                )
                .expect("test scale is ordered"),
            },
            2,
        )
        .expect("test key has a valid mode");

        let pattern_id = song.patterns[0].id;
        let pattern = song
            .pattern_mut(pattern_id)
            .expect("the default pattern exists");
        let mut degree = Note::with_pitch(Pitch::degree(2, -1), 17, 91);
        degree.pitch.offset_cents = -13.375;
        degree.micro_ticks = -5;
        let mut absolute = Note::with_pitch(Pitch::absolute(261.625_565_300_598_6), 29, 114);
        absolute.pitch.offset_cents = 7.25;
        absolute.micro_ticks = 3;
        pattern.set_primary(7, degree);
        pattern.add_tone(7, absolute);

        let encoded = ron::ser::to_string(&song).expect("song serializes");
        let decoded: Song = ron::from_str(&encoded).expect("song deserializes");
        assert_eq!(decoded, song);

        let notes = &decoded
            .pattern(pattern_id)
            .expect("pattern survived")
            .trig(7)
            .notes;
        let decoded_absolute = notes
            .iter()
            .find(|note| matches!(note.pitch.anchor, Anchor::Absolute(_)))
            .expect("absolute anchor survived");
        let decoded_degree = notes
            .iter()
            .find(|note| matches!(note.pitch.anchor, Anchor::Degree { .. }))
            .expect("degree anchor survived");
        assert_eq!(
            decoded_absolute.pitch.offset_cents.to_bits(),
            7.25_f32.to_bits()
        );
        assert_eq!(decoded_absolute.micro_ticks, 3);
        assert_eq!(
            decoded_degree.pitch.offset_cents.to_bits(),
            (-13.375_f32).to_bits()
        );
        assert_eq!(decoded_degree.micro_ticks, -5);
        assert_eq!(decoded.key.reference_hz().to_bits(), 432.125_f64.to_bits());
        assert_eq!(decoded.key.mode(), 2);
    }

    #[test]
    fn block_creation_rejects_overlap_and_keeps_timeline_ordered() {
        let mut song = Song::default();
        assert!(
            song.create_pattern_block(0, TICKS_PER_BEAT, TICKS_PER_BEAT)
                .is_none()
        );
        let id = song
            .create_pattern_block(0, DEFAULT_PATTERN_TICKS, TICKS_PER_BEAT * 2)
            .expect("free instrument region should accept a pattern block");
        assert_eq!(song.tracks[0].blocks.len(), 2);
        assert_eq!(song.tracks[0].blocks[1].pattern_id, id);
        assert_eq!(
            song.pattern(id).map(|pattern| pattern.name.as_str()),
            Some("P02")
        );
    }

    #[test]
    fn region_delete_collects_only_unreferenced_patterns() {
        let mut song = Song::default();
        let second = song
            .create_pattern_block(0, DEFAULT_PATTERN_TICKS, TICKS_PER_BEAT)
            .expect("free region");
        assert_eq!(
            song.delete_blocks_in_region(0, 0, DEFAULT_PATTERN_TICKS, DEFAULT_PATTERN_TICKS + 1),
            1
        );
        assert!(song.pattern(second).is_none());
        assert_eq!(song.patterns.len(), 1);
    }

    #[test]
    fn blocks_move_copy_and_resize_without_crossing_occupied_regions() {
        let mut song = Song::default();
        let original = song.tracks[0].blocks[0].id;
        assert!(song.move_pattern_block(original, 0, DEFAULT_PATTERN_TICKS));
        let copy = song
            .duplicate_pattern_block(original, 0, 0)
            .expect("the vacated region accepts a copy");
        assert_ne!(
            song.tracks[0].blocks[0].pattern_id,
            song.tracks[0].blocks[1].pattern_id
        );
        assert!(song.resize_pattern_block(copy, 0, TICKS_PER_BEAT * 4));
        assert!(!song.move_pattern_block(original, 0, TICKS_PER_BEAT * 2));
    }
}

/// The offset model's BASE authority: curves in song time, and the
/// three-layer read that finally gives `param_law` its callers.
#[cfg(test)]
mod automation_tests {
    use super::*;

    const TARGET: &str = "track.volume";

    fn track() -> Track {
        Track {
            id: TrackId(1),
            name: "T".to_owned(),
            kind: TrackKind::Instrument,
            blocks: Vec::new(),
            muted: false,
            solo: false,
            pitch_authority: PitchAuthority::default(),
            audio_blocks: Vec::new(),
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            sends: Vec::new(),
            is_group: false,
            folded: false,
            chain: Vec::new(),
            depth: 0,
            input: TrackInput::default(),
            monitor: Monitor::default(),
            armed: false,
        }
    }

    fn def() -> crate::params::ParamDef {
        crate::params::ParamDef {
            id: 0,
            name: "volume",
            min: 0.0,
            max: 1.0,
            default: 1.0,
        }
    }

    /// An un-automated parameter costs nothing: the knob comes back
    /// untouched, which is what lets every read go through this path.
    #[test]
    fn no_points_is_the_knob() {
        let track = track();
        assert!(!track.automated(TARGET));
        assert_eq!(track.value_at(TARGET, 0, 0.25), 0.25);
        assert_eq!(track.value_at(TARGET, 99_999, 0.25), 0.25);
    }

    /// Hold the knob before the first point, interpolate between points,
    /// hold the last value after the end. The three regions of a curve.
    #[test]
    fn the_curve_holds_then_travels_then_holds() {
        let mut track = track();
        track.insert_point(TARGET, 48, 0.0);
        track.insert_point(TARGET, 144, 1.0);
        assert!(track.automated(TARGET));
        // Before the first point: the knob, not the first value.
        assert_eq!(track.value_at(TARGET, 0, 0.7), 0.7);
        // Exactly on the points.
        assert_eq!(track.value_at(TARGET, 48, 0.7), 0.0);
        assert_eq!(track.value_at(TARGET, 144, 0.7), 1.0);
        // Halfway, straight segment.
        assert!((track.value_at(TARGET, 96, 0.7) - 0.5).abs() < 1e-6);
        // After the last point: the last value, not the knob.
        assert_eq!(track.value_at(TARGET, 10_000, 0.7), 1.0);
    }

    /// Bend bows the segment without moving its ends, and the sign picks
    /// the side. Zero must stay an exact straight line.
    #[test]
    fn bend_bows_the_segment_but_never_its_ends() {
        let mut track = track();
        track.insert_point(TARGET, 0, 0.0);
        track.insert_point(TARGET, 100, 1.0);
        let straight = track.value_at(TARGET, 50, 0.0);
        assert!((straight - 0.5).abs() < 1e-6);

        assert!(track.bend_point(TARGET, 0, 0.5));
        let up = track.value_at(TARGET, 50, 0.0);
        assert!(track.bend_point(TARGET, 0, -0.5));
        let down = track.value_at(TARGET, 50, 0.0);
        assert!(up < straight, "positive bend sags below the line");
        assert!(down > straight, "negative bend bows above the line");

        // The ends never move, whatever the bend.
        assert_eq!(track.value_at(TARGET, 0, 0.0), 0.0);
        assert_eq!(track.value_at(TARGET, 100, 0.0), 1.0);
    }

    /// Placing a point where one already sits is an EDIT, not a stack —
    /// and the list stays sorted however the points arrive.
    #[test]
    fn insert_replaces_in_place_and_keeps_order() {
        let mut track = track();
        track.insert_point(TARGET, 96, 0.5);
        track.insert_point(TARGET, 0, 0.1);
        track.insert_point(TARGET, 48, 0.9);
        track.insert_point(TARGET, 48, 0.3);
        let ticks: Vec<usize> = track.points(TARGET).iter().map(|p| p.tick).collect();
        assert_eq!(ticks, vec![0, 48, 96], "sorted, and 48 was not duplicated");
        assert_eq!(track.points(TARGET)[1].value, 0.3, "the later write won");
    }

    #[test]
    fn remove_reports_whether_a_point_went() {
        let mut track = track();
        track.insert_point(TARGET, 48, 1.0);
        assert!(track.remove_point(TARGET, 48));
        assert!(!track.remove_point(TARGET, 48));
        assert!(!track.automated(TARGET));
    }

    /// Two targets on one track never read each other's points.
    #[test]
    fn targets_are_independent() {
        let mut track = track();
        track.insert_point("track.volume", 0, 1.0);
        track.insert_point("track.pan", 0, -1.0);
        assert_eq!(track.value_at("track.volume", 0, 0.0), 1.0);
        assert_eq!(track.value_at("track.pan", 0, 0.0), -1.0);
        assert_eq!(track.value_at("track.unknown", 0, 0.42), 0.42);
    }

    /// The headline: knob -> curve -> lock, summed in exactly one place.
    /// The composition keeps both authorities so the UI can print the sum
    /// as a sum rather than a mystery number.
    #[test]
    fn the_three_layers_compose_through_param_law() {
        let mut track = track();
        let def = def();

        // Layer one only: no curve, no lock — the knob survives.
        let knob_only = track
            .effective(&def, TARGET, 0, 0.5, None)
            .expect("composes");
        assert_eq!(knob_only.effective(), 0.5);

        // Layer two: a curve overrides the knob at that tick.
        track.insert_point(TARGET, 0, 0.0);
        track.insert_point(TARGET, 100, 1.0);
        let curved = track
            .effective(&def, TARGET, 50, 0.5, None)
            .expect("composes");
        assert!((curved.effective() - 0.5).abs() < 1e-6);
        let curved = track
            .effective(&def, TARGET, 100, 0.5, None)
            .expect("composes");
        assert_eq!(curved.effective(), 1.0);

        // Layer three: the lock is an OFFSET on the curve, not a replacement.
        let locked = track
            .effective(&def, TARGET, 0, 0.5, Some(0.25))
            .expect("composes");
        assert_eq!(locked.effective(), 0.25, "curve 0.0 + lock 0.25");
    }

    /// A lock that pushes the sum past the range is PINNED, and the
    /// composition says so rather than pretending the lock was smaller.
    #[test]
    fn a_lock_past_the_range_pins_and_admits_it() {
        let mut track = track();
        let def = def();
        track.insert_point(TARGET, 0, 0.9);
        let composed = track
            .effective(&def, TARGET, 0, 0.0, Some(0.5))
            .expect("composes");
        assert_eq!(composed.effective(), 1.0);
        assert_eq!(
            composed.clamp_state(),
            crate::param_law::ClampState::Maximum
        );
    }

    /// The fader and pan are ordinary automation targets: the knob is
    /// the base, the curve overrides it, and neither is allowed to leave
    /// its legal span.
    #[test]
    fn the_mixer_reads_through_the_curve_and_stays_in_range() {
        let mut track = track();
        track.volume = 0.5;
        track.pan = 0.0;
        // No curve: the knob.
        assert_eq!(track.volume_at(0), 0.5);
        assert_eq!(track.pan_at(0), 0.0);

        // A curve overrides the knob from its first point onward.
        track.insert_point(TRACK_VOLUME, 48, 1.0);
        assert_eq!(track.volume_at(0), 0.5, "held knob before the curve");
        assert_eq!(track.volume_at(48), 1.0, "the curve took over");

        // Neither read escapes its span, however the curve is drawn.
        track.insert_point(TRACK_VOLUME, 96, -3.0);
        assert_eq!(track.volume_at(96), 0.0, "a fader never inverts phase");
        track.insert_point(TRACK_PAN, 0, -9.0);
        assert_eq!(track.pan_at(0), -1.0, "pinned hard left, not nonsense");
    }

    /// A document written before the mixer existed comes back at UNITY.
    /// Defaulting a fader to zero would mute every older project.
    #[test]
    fn a_pre_mixer_document_returns_at_unity_not_silence() {
        let song = Song::default();
        let text = ron::ser::to_string(&song).expect("serializes");
        let older = text.replace("volume:1.0,", "").replace("pan:0.0,", "");
        let back: Song = ron::from_str(&older).expect("older document loads");
        assert_eq!(back.tracks[0].volume, 1.0, "unity, never silence");
        assert_eq!(back.tracks[0].pan, 0.0, "centred, and exactly so");
    }

    /// A Song written before curves existed still loads, and reads as a
    /// song with no curves.
    #[test]
    fn pre_automation_documents_still_load() {
        let song = Song::default();
        let text = ron::ser::to_string(&song).expect("serializes");
        assert!(text.contains("automation"), "new documents carry the field");

        let older = text
            .replace("automation:[],", "")
            .replace("automation:[]", "");
        let back: Song = ron::from_str(&older).expect("older document still loads");
        assert!(back.tracks.iter().all(|track| track.automation.is_empty()));
    }
}

/// Mixer data is project file format first: every absent field defaults to
/// the sound an older project already made, and positional edits keep sends,
/// returns and their lettered curves in one meaning.
/// Making a track. Document editing rather than scheduling, so what is
/// on trial is identity — that a new track is empty, silent, uniquely
/// addressed, and named in a way that does not depend on tracks of some
/// other kind.
#[cfg(test)]
mod track_tests {
    use super::*;

    #[test]
    fn a_new_track_is_numbered_within_its_own_kind() {
        let mut song = Song::default();
        assert_eq!(song.tracks.len(), 1, "the default song changed shape");

        song.add_track(TrackKind::Audio);
        song.add_track(TrackKind::Audio);
        song.add_track(TrackKind::Instrument);

        let names: Vec<_> = song.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["Instrument 01", "Audio 01", "Audio 02", "Instrument 02"]
        );
    }

    #[test]
    fn ids_are_unique_among_the_tracks_the_song_holds() {
        let mut song = Song::default();
        song.add_track(TrackKind::Audio);
        song.add_track(TrackKind::Instrument);
        song.add_track(TrackKind::Audio);

        let mut ids: Vec<_> = song.tracks.iter().map(|track| track.id.0).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two tracks share an id");
    }

    #[test]
    fn an_id_is_never_minted_twice_even_after_its_track_is_gone() {
        // The counter the delete verb owed the document. Without it a
        // scene slot, an undo step or a frame's memory that held the
        // removed id would silently attach to the next track made.
        let mut song = Song::default();
        let first = song.add_track(TrackKind::Audio);
        assert_eq!(song.remove_track(1).map(|track| track.id), Some(first));
        let second = song.add_track(TrackKind::Audio);

        assert_ne!(second, first, "a removed track's id was handed out again");
        assert!(second.0 > first.0, "ids do not go up");
    }

    #[test]
    fn a_document_without_the_counter_mints_one_past_the_highest() {
        // Every file written before the counter existed reads as zero,
        // and must mint what it always minted.
        let mut song = Song {
            next_track_id: 0,
            ..Song::default()
        };
        song.tracks[0].id = TrackId(7);
        assert_eq!(song.add_track(TrackKind::Audio), TrackId(8));
        assert_eq!(song.next_track_id, 9);
    }

    #[test]
    fn removing_a_track_takes_its_slots_and_leaves_its_patterns() {
        let mut song = Song::default();
        let gone = song.add_track(TrackKind::Instrument);
        let kept = song.add_track(TrackKind::Instrument);
        let pattern = song.fill_slot(1, 2).expect("a slot on the doomed track");
        song.fill_slot(2, 2).expect("a slot on the survivor");
        let patterns = song.patterns.len();

        let removed = song.remove_track(1).expect("there");
        assert_eq!(removed.id, gone);
        assert_eq!(song.tracks.len(), 2);
        assert!(
            song.session
                .scenes
                .iter()
                .all(|scene| scene.clip(gone).is_none()),
            "a slot still named the removed track"
        );
        assert!(
            song.session.scenes[2].clip(kept).is_some(),
            "the survivor lost its clip"
        );
        assert_eq!(
            song.patterns.len(),
            patterns,
            "content was destroyed with a place"
        );
        assert!(song.pattern(pattern).is_some());
        assert!(
            song.remove_track(9).is_none(),
            "a track that is not there was removed"
        );
    }

    #[test]
    fn a_moved_track_keeps_its_clips_and_refuses_at_the_ends() {
        let mut song = Song::default();
        let first = song.tracks[0].id;
        let second = song.add_track(TrackKind::Audio);
        song.fill_slot(0, 0).expect("a pattern on the first track");

        assert!(song.move_track(0, true));
        let order: Vec<TrackId> = song.tracks.iter().map(|track| track.id).collect();
        assert_eq!(order, [second, first]);
        // The clip is on the track, wherever the track is.
        assert!(song.slot_clip(1, 0).is_some(), "the clip did not travel");
        assert!(song.slot_clip(0, 0).is_none());

        assert!(!song.move_track(1, true), "moved past the end");
        assert!(!song.move_track(0, false), "moved before the start");
        assert!(!song.move_track(5, true), "a track that is not there moved");
    }

    #[test]
    fn a_rename_is_trimmed_and_a_blank_one_is_refused() {
        let mut song = Song::default();
        assert!(song.rename_track(0, "  Bass  "));
        assert_eq!(song.tracks[0].name, "Bass");
        assert!(!song.rename_track(0, "   "), "a track was given no name");
        assert_eq!(song.tracks[0].name, "Bass");
        assert!(!song.rename_track(4, "Ghost"));
    }

    #[test]
    fn a_new_track_starts_empty_and_silent() {
        let mut song = Song::default();
        song.add_track(TrackKind::Instrument);
        let made = song.tracks.last().expect("the track was not added");

        assert!(made.blocks.is_empty());
        assert!(made.audio_blocks.is_empty());
        assert!(made.automation.is_empty());
        assert!(!made.muted && !made.solo && !made.armed);
        assert!(!made.is_group);
    }
}

#[cfg(test)]
mod mixer_tests {
    use super::*;

    #[test]
    fn a_pre_mixer_song_still_loads_untouched() {
        let text = ron::ser::to_string(&Song::default()).expect("serializes");
        let older = text
            .replace("sends:[],", "")
            .replace("is_group:false,", "")
            .replace("folded:false,", "")
            .replace("depth:0,", "")
            .replace("input:None,", "")
            .replace("monitor:Off,", "")
            .replace("returns:[],", "");
        assert!(!older.contains("sends"));
        assert!(!older.contains("is_group"));
        assert!(!older.contains("returns"));

        let loaded: Song = ron::from_str(&older).expect("the whole older Song still opens");
        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].volume, 1.0);
        assert_eq!(loaded.tracks[0].pan, 0.0);
        assert!(loaded.tracks[0].sends.is_empty());
        assert!(!loaded.tracks[0].is_group);
        assert!(!loaded.tracks[0].folded);
        assert_eq!(loaded.tracks[0].depth, 0);
        assert_eq!(loaded.tracks[0].input, TrackInput::None);
        assert_eq!(loaded.tracks[0].monitor, Monitor::Off);
        assert!(!loaded.tracks[0].armed);
        assert!(loaded.returns.is_empty());
    }

    #[test]
    fn returns_and_group_fields_round_trip() {
        let mut song = Song::default();
        song.returns.push(ReturnTrack {
            name: "Plate".to_owned(),
            mute: true,
            volume: 0.7,
            pan: -0.25,
        });
        let track = &mut song.tracks[0];
        track.sends = vec![0.375];
        track.is_group = true;
        track.folded = true;
        track.depth = 3;
        track.input = TrackInput::Stereo(2, 3);
        track.monitor = Monitor::Auto;

        let text = ron::ser::to_string(&song).expect("serializes");
        let loaded: Song = ron::from_str(&text).expect("deserializes");
        assert_eq!(loaded, song);
    }

    #[test]
    fn arm_is_live_state_and_never_opens_from_disk() {
        let mut song = Song::default();
        song.tracks[0].armed = true;
        let text = ron::ser::to_string(&song).expect("serializes");
        assert!(
            !text.contains("armed"),
            "an arm must never become project state"
        );

        let loaded: Song = ron::from_str(&text).expect("deserializes");
        assert!(!loaded.tracks[0].armed, "the song opened armed");
    }

    #[test]
    fn deleting_a_return_shifts_static_sends_and_lettered_curves_together() {
        let mut song = Song::default();
        for _ in 0..3 {
            song.add_return().expect("A through C fit");
        }
        let track = &mut song.tracks[0];
        track.sends = vec![0.1, 0.2, 0.3];
        track.automation = vec![
            Envelope {
                target: "track.send.b".to_owned(),
                points: vec![Point {
                    tick: 0,
                    value: 0.8,
                    bend: 0.0,
                }],
            },
            Envelope {
                target: "track.send.c".to_owned(),
                points: vec![Point {
                    tick: 0,
                    value: 0.6,
                    bend: 0.0,
                }],
            },
        ];

        assert!(song.remove_return(1));
        assert_eq!(song.tracks[0].sends, vec![0.1, 0.3]);
        assert_eq!(song.tracks[0].automation.len(), 1);
        assert_eq!(song.tracks[0].automation[0].target, "track.send.b");
        assert_eq!(song.tracks[0].automation[0].points[0].value, 0.6);
    }

    #[test]
    fn group_depth_is_normalized_on_the_song() {
        let mut song = Song::default();
        song.tracks[0].depth = u8::MAX;
        song.tracks[0].folded = true;
        let mut child = song.tracks[0].clone();
        child.id = TrackId(2);
        child.is_group = true;
        child.folded = true;
        child.depth = u8::MAX;
        song.tracks.push(child);

        song.normalize_group_depths();

        assert_eq!(song.tracks[0].depth, 0);
        assert!(!song.tracks[0].folded, "plain lanes cannot fold");
        assert_eq!(song.tracks[1].depth, 0, "no group above permits depth one");
        assert!(song.tracks[1].folded, "a real group keeps its fold");
    }

    /// Lanes built from the default song's own track, so this helper cannot
    /// rot as the model grows fields: only what audibility reads is set.
    fn lanes(shape: &[(bool, u8, bool, bool)]) -> Song {
        let base = Song::default()
            .tracks
            .into_iter()
            .next()
            .expect("the default song has a track");
        Song {
            tracks: shape
                .iter()
                .map(|&(is_group, depth, muted, solo)| Track {
                    is_group,
                    depth,
                    muted,
                    solo,
                    ..base.clone()
                })
                .collect(),
            ..Song::default()
        }
    }

    #[test]
    fn a_muted_group_takes_its_members_with_it() {
        // A group, one lane inside it, one lane outside.
        let song = lanes(&[
            (true, 0, true, false),
            (false, 1, false, false),
            (false, 0, false, false),
        ]);
        assert!(!song.audible(0), "the muted group itself sounded");
        assert!(
            !song.audible(1),
            "a lane inside a muted group kept sounding"
        );
        assert!(song.audible(2), "a lane outside the group was silenced");
    }

    #[test]
    fn nothing_soloed_leaves_every_unmuted_lane_sounding() {
        let song = lanes(&[(false, 0, false, false), (false, 0, true, false)]);
        assert!(song.audible(0));
        assert!(!song.audible(1), "an explicitly muted lane sounded");
    }

    #[test]
    fn solo_anywhere_silences_the_lanes_it_does_not_reach() {
        let song = lanes(&[(false, 0, false, true), (false, 0, false, false)]);
        assert!(song.any_solo());
        assert!(song.audible(0));
        assert!(
            !song.audible(1),
            "solo elsewhere left an unsoloed lane sounding"
        );
    }

    #[test]
    fn a_soloed_group_sounds_the_lanes_inside_it() {
        let song = lanes(&[
            (true, 0, false, true),
            (false, 1, false, false),
            (false, 0, false, false),
        ]);
        assert!(song.audible(0));
        assert!(song.audible(1), "a member of the soloed group went silent");
        assert!(!song.audible(2), "a lane outside the soloed group sounded");
    }

    #[test]
    fn a_soloed_member_keeps_the_group_that_carries_it() {
        // Nothing would reach the master if the bus above a soloed lane
        // were silenced by that same solo.
        let song = lanes(&[
            (true, 0, false, false),
            (false, 1, false, true),
            (false, 0, false, false),
        ]);
        assert!(song.audible(1), "the soloed lane itself went silent");
        assert!(
            song.audible(0),
            "the group carrying the soloed lane was silenced by it"
        );
        assert!(!song.audible(2));
    }

    #[test]
    fn muting_outranks_solo_on_the_same_lane() {
        let song = lanes(&[(false, 0, true, true), (false, 0, false, false)]);
        assert!(
            !song.audible(0),
            "a muted lane sounded because it was soloed"
        );
        assert!(!song.audible(1), "solo elsewhere left this lane sounding");
    }

    #[test]
    fn an_index_past_the_song_is_not_audible() {
        let song = lanes(&[(false, 0, false, false)]);
        assert!(!song.solo_in_scope(9));
        assert!(song.group_members(9).is_empty());
        assert_eq!(song.parent_group(9), None);
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;
    use crate::devices::DeviceKind;

    fn song() -> Song {
        let mut song = Song::default();
        song.add_track(TrackKind::Audio);
        song
    }

    #[test]
    fn a_song_written_before_devices_still_opens_and_sounds_the_same() {
        // The empty chain is the meaningful default: an instrument track
        // with nothing on it sounds what it always sounded.
        let text = ron::ser::to_string(&Song::default()).expect("serializes");
        let older = text.replace("chain:[],", "");
        assert!(
            !older.contains("chain"),
            "the field was not actually removed"
        );
        let loaded: Song = ron::from_str(&older).expect("a pre-chain project must still open");
        assert!(loaded.tracks[0].chain.is_empty());
    }

    #[test]
    fn an_instrument_heads_the_chain_and_an_effect_joins_the_tail() {
        let mut song = song();
        let reverb = song
            .add_device(0, DeviceKind::Reverb)
            .expect("an effect goes on an instrument track");
        let poly = song
            .add_device(0, DeviceKind::Poly)
            .expect("so does an instrument");
        let kinds: Vec<DeviceId> = song.tracks[0].chain.iter().map(|d| d.id).collect();
        assert_eq!(
            kinds,
            [poly, reverb],
            "the instrument did not take the head the effect was already occupying"
        );
    }

    #[test]
    fn a_track_sounds_one_voice() {
        let mut song = song();
        let first = song.add_device(0, DeviceKind::Poly).expect("an instrument");
        let second = song.add_device(0, DeviceKind::Haze).expect("another");
        assert_eq!(
            song.tracks[0].chain.len(),
            1,
            "two instruments on one track"
        );
        assert_eq!(song.tracks[0].chain[0].id, second);
        assert!(song.device(first).is_none(), "the replaced voice lingered");
    }

    #[test]
    fn an_audio_track_refuses_an_instrument_and_takes_effects() {
        let mut song = song();
        assert_eq!(
            song.add_device(1, DeviceKind::Poly),
            None,
            "an audio track grew a voice it cannot use"
        );
        assert!(song.add_device(1, DeviceKind::Reverb).is_some());
    }

    #[test]
    fn effects_reorder_but_never_around_the_head() {
        let mut song = song();
        let poly = song.add_device(0, DeviceKind::Poly).expect("instrument");
        let a = song.add_device(0, DeviceKind::Reverb).expect("effect");
        let b = song.add_device(0, DeviceKind::Echo).expect("effect");

        assert!(
            song.move_device(0, a, true),
            "an effect would not move later"
        );
        let order: Vec<DeviceId> = song.tracks[0].chain.iter().map(|d| d.id).collect();
        assert_eq!(order, [poly, b, a]);

        assert!(
            !song.move_device(0, b, false),
            "an effect moved into the instrument's place"
        );
        assert!(
            !song.move_device(0, poly, true),
            "the instrument left the head"
        );
        assert!(
            !song.move_device(0, a, true),
            "an effect moved past the tail"
        );
    }

    #[test]
    fn a_put_device_keeps_its_settings_and_gets_a_fresh_id() {
        let mut song = song();
        let poly = song.add_device(0, DeviceKind::Poly).expect("instrument");
        let a = song.add_device(0, DeviceKind::Reverb).expect("effect");
        let b = song.add_device(0, DeviceKind::Echo).expect("effect");
        // A COPY of a device still on the chain: the case where keeping
        // the id would make two devices one.
        let mut taken = song.device(b).expect("there").clone();
        taken.bypassed = true;
        taken.set(crate::params::echo::MIX, 0.9);

        // Asked for the head, an effect lands right behind it.
        let put = song.insert_device(0, 0, taken.clone()).expect("put");
        let order: Vec<DeviceId> = song.tracks[0].chain.iter().map(|d| d.id).collect();
        assert_eq!(order, [poly, put, a, b]);
        assert_ne!(put, b, "the put device kept an id that is in use");
        let device = song.device(put).expect("there");
        assert!(device.bypassed, "the bypass did not travel");
        assert_eq!(
            device.value(crate::params::echo::MIX),
            0.9,
            "the edit did not travel"
        );
        assert!(
            !song.device(b).expect("there").bypassed,
            "the original was edited"
        );

        // Putting the same thing again is another copy, not a move.
        let again = song
            .insert_device(0, 99, taken.clone())
            .expect("put past the tail");
        assert_ne!(again, put);
        assert_eq!(song.tracks[0].chain.len(), 5);
        assert_eq!(song.tracks[0].chain.last().map(|d| d.id), Some(again));

        // An instrument put anywhere replaces the head.
        let head = song.remove_device(0, poly).expect("there");
        let replaced = song.insert_device(0, 3, head).expect("put");
        assert_eq!(song.tracks[0].chain[0].id, replaced);
        assert_eq!(song.tracks[0].chain.len(), 5);

        // And an audio track refuses it, as it refuses every instrument.
        let head = song.remove_device(0, replaced).expect("there");
        assert!(song.insert_device(1, 0, head).is_none());
    }

    #[test]
    fn a_sampler_carries_its_sample_through_the_file() {
        let mut song = song();
        let id = song.add_device(0, DeviceKind::Sampler).expect("instrument");
        song.device_mut(id).expect("there").sample = Some("/kits/kick.wav".into());
        let text = ron::ser::to_string(&song).expect("encodes");
        let back: Song = ron::from_str(&text).expect("decodes");
        assert_eq!(
            back.device(id).and_then(|device| device.sample.clone()),
            Some(std::path::PathBuf::from("/kits/kick.wav"))
        );
        // A device with no sample writes no field, so every file written
        // before the field existed reads the same as one written after.
        let reverb = song.add_device(0, DeviceKind::Reverb).expect("effect");
        assert!(song.device(reverb).expect("there").sample.is_none());
        let text = ron::ser::to_string(song.device(reverb).expect("there")).expect("encodes");
        assert!(
            !text.contains("sample"),
            "an absent sample was written: {text}"
        );
    }

    #[test]
    fn normalising_repairs_a_head_without_rearranging_the_rest() {
        let mut song = song();
        // A chain assembled by hand, in an illegal order.
        for kind in [DeviceKind::Reverb, DeviceKind::Echo, DeviceKind::Poly] {
            let id = DeviceId(song.tracks[0].chain.len() as u64 + 1);
            song.tracks[0].chain.push(Device::new(id, kind));
        }
        song.normalize_chains();
        let kinds: Vec<DeviceKind> = song.tracks[0].chain.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            [DeviceKind::Poly, DeviceKind::Reverb, DeviceKind::Echo],
            "the head was not repaired, or the effects were rearranged with it"
        );
    }

    #[test]
    fn normalising_takes_an_instrument_off_an_audio_track() {
        let mut song = song();
        song.tracks[1]
            .chain
            .push(Device::new(DeviceId(1), DeviceKind::Poly));
        song.tracks[1]
            .chain
            .push(Device::new(DeviceId(2), DeviceKind::Reverb));
        song.normalize_chains();
        let kinds: Vec<DeviceKind> = song.tracks[1].chain.iter().map(|d| d.kind).collect();
        assert_eq!(kinds, [DeviceKind::Reverb]);
    }

    #[test]
    fn a_parameter_reads_its_default_until_it_is_set() {
        let mut song = song();
        let id = song.add_device(0, DeviceKind::Reverb).expect("an effect");
        let device = song.device(id).expect("it is there");
        let def = device.table().first().expect("reverb has parameters");
        assert_eq!(device.value(def.id), def.default);
        assert!(
            device.overrides.is_empty(),
            "a default was stored as an edit"
        );

        let (id_, min, max) = (def.id, def.min, def.max);
        let device = song.device_mut(id).expect("it is there");
        assert!(device.set(id_, max + 100.0));
        assert_eq!(
            device.value(id_),
            max,
            "a value was not clamped to its range"
        );
        assert!(device.set(id_, min - 100.0));
        assert_eq!(device.value(id_), min);
        assert_eq!(device.overrides.len(), 1, "setting twice stored two values");
    }

    #[test]
    fn a_device_never_grows_a_parameter_by_being_sent_one() {
        let mut song = song();
        let id = song.add_device(0, DeviceKind::Reverb).expect("an effect");
        let device = song.device_mut(id).expect("it is there");
        assert!(!device.set(9_999, 1.0), "an unknown id was accepted");
        assert!(device.overrides.is_empty());
        assert_eq!(device.value(9_999), 0.0);
    }

    #[test]
    fn an_id_survives_a_reorder_and_a_bypass() {
        let mut song = song();
        let _ = song.add_device(0, DeviceKind::Poly);
        let a = song.add_device(0, DeviceKind::Reverb).expect("effect");
        let b = song.add_device(0, DeviceKind::Echo).expect("effect");
        song.device_mut(a).expect("there").bypassed = true;
        assert!(song.move_device(0, a, true));
        // The whole reason ids exist: the letter still finds its device.
        let moved = song.device(a).expect("the id outlived the move");
        assert!(moved.bypassed, "bypass travelled to another device");
        assert_ne!(a, b);
    }

    #[test]
    fn removing_hands_the_device_back() {
        let mut song = song();
        let id = song.add_device(0, DeviceKind::Reverb).expect("effect");
        let taken = song.remove_device(0, id).expect("it was there");
        assert_eq!(taken.id, id);
        assert!(song.device(id).is_none());
        assert_eq!(song.remove_device(0, id), None, "it came off twice");
    }
}

/// Audio blocks are the new half of a track's timeline. The tests that
/// matter most here are not the round trips — they are the ones proving
/// a project written before audio blocks existed still opens.
#[cfg(test)]
mod audio_block_tests {
    use super::*;

    fn source() -> crate::audio_source::AudioSource {
        ron::from_str(
            r#"(path:"/tmp/kick.wav",sample_rate:48000,source_offset:0,source_frames:24000,gain:1.0,looped:false)"#,
        )
        .expect("a minimal source deserializes from its required fields")
    }

    fn audio(id: u64, start_tick: usize, length_ticks: usize) -> AudioBlock {
        AudioBlock {
            id: BlockId(id),
            name: "kick".to_owned(),
            start_tick,
            length_ticks,
            source: source(),
            loop_brace: None,
        }
    }

    /// THE test this design exists for.
    ///
    /// A document that predates audio blocks — built by serializing a
    /// real Song and stripping every field added since, which is exactly
    /// the shape sitting in users' .daw.ron files today. Not a round
    /// trip: a round trip only proves the new code agrees with itself.
    /// This proves it agrees with what is already on disk.
    ///
    /// Getting this wrong means every project a user ever saved stops
    /// opening, and they find out by losing their work.
    #[test]
    fn a_pre_audio_project_still_loads_untouched() {
        let song = Song::default();
        let text = ron::ser::to_string(&song).expect("serializes");
        assert!(
            text.contains("audio_blocks:[]"),
            "the new field is written today"
        );

        // Strip every field added after the original on-disk shape.
        let older = text
            .replace("audio_blocks:[],", "")
            .replace("automation:[],", "")
            .replace("volume:1.0,", "")
            .replace("pan:0.0,", "")
            .replace("sends:[],", "")
            .replace("is_group:false,", "")
            .replace("folded:false,", "")
            .replace("depth:0,", "")
            .replace("returns:[],", "")
            .replace("tempo:[],", "")
            .replace("meter:[],", "");
        assert!(
            !older.contains("audio_blocks"),
            "the stripped document really lacks the field"
        );

        let back: Song = match ron::from_str(&older) {
            Ok(back) => back,
            Err(error) => panic!("a pre-audio document must still load: {error}"),
        };
        let track = &back.tracks[0];
        assert_eq!(track.blocks.len(), 1, "its pattern block survived");
        assert_eq!(track.blocks[0].length_ticks, DEFAULT_PATTERN_TICKS);
        assert!(track.audio_blocks.is_empty(), "and it simply has no audio");
        // Everything added since defaults rather than failing the load.
        assert_eq!(track.volume, 1.0, "unity, never silence");
        assert!(track.automation.is_empty());
        assert!(back.tempo.is_empty());
        assert!(back.returns.is_empty());
        assert!(track.sends.is_empty());
        assert!(!track.is_group);
        assert!(!track.folded);
        assert_eq!(track.depth, 0);
    }

    /// A song carrying audio survives a round trip whole, source and all.
    #[test]
    fn an_audio_block_round_trips() {
        let mut song = Song::default();
        song.tracks[0].audio_blocks.push(audio(7, 96, 480));
        song.tracks[0].audio_blocks[0].loop_brace = Some(LoopBrace {
            start_tick: 0,
            length_ticks: 240,
        });

        let text = ron::ser::to_string(&song).expect("serializes");
        let back: Song = ron::from_str(&text).expect("deserializes");
        assert_eq!(back, song, "every field of the source came back");
        assert_eq!(back.tracks[0].audio_blocks[0].source.source_frames, 24_000);
    }

    /// An overlap test that only consulted `blocks` would happily drop a
    /// pattern on top of a recording. `occupied` sees both lists.
    #[test]
    fn occupancy_sees_audio_as_well_as_patterns() {
        let mut song = Song::default();
        let track = &mut song.tracks[0];
        // The default song already holds one pattern block at 0..768.
        assert!(track.occupied(0, 10), "the pattern block is seen");
        assert!(!track.occupied(1_000, 1_100), "empty space is empty");

        track.audio_blocks.push(audio(9, 1_000, 100));
        assert!(track.occupied(1_050, 1_100), "and now the audio is seen");
        assert!(!track.occupied(1_100, 1_200), "but only where it sits");
    }

    fn audio_track(song: &mut Song) -> usize {
        song.tracks.push(Track {
            id: TrackId(99),
            name: "AUDIO 01".to_owned(),
            kind: TrackKind::Audio,
            blocks: Vec::new(),
            audio_blocks: Vec::new(),
            muted: false,
            solo: false,
            pitch_authority: PitchAuthority::default(),
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            sends: Vec::new(),
            is_group: false,
            folded: false,
            chain: Vec::new(),
            depth: 0,
            input: TrackInput::default(),
            monitor: Monitor::default(),
            armed: false,
        });
        song.tracks.len() - 1
    }

    /// One second of audio at 120bpm is two beats — 96 ticks.
    #[test]
    fn landing_takes_its_length_from_the_files_real_duration() {
        let mut song = Song::default();
        let track = audio_track(&mut song);
        let tempo = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);

        let id = song
            .place_audio(track, 0, "kick".to_owned(), source(), &tempo)
            .expect("lands on an audio track");
        let block = &song.tracks[track].audio_blocks[0];
        assert_eq!(block.id, id);
        // 24000 frames at 48kHz is half a second; at 120bpm that is one
        // beat, which is TICKS_PER_BEAT.
        assert_eq!(block.length_ticks, TICKS_PER_BEAT);
    }

    /// THE test the landing brief insists on.
    ///
    /// The length must come through the TEMPO MAP. A uniform map would
    /// pass even if the map were ignored entirely, so this one changes
    /// tempo underneath the sound: at half speed the same half-second of
    /// audio spans half as many ticks, because a tick lasts twice as
    /// long.
    #[test]
    fn a_landed_length_follows_a_non_uniform_tempo_map() {
        let mut song = Song::default();
        let track = audio_track(&mut song);

        let fast = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);
        song.place_audio(track, 0, "kick".to_owned(), source(), &fast)
            .expect("lands");
        let at_120 = song.tracks[track].audio_blocks[0].length_ticks;

        // Now the same sound on a song that runs at half the tempo.
        let mut slow_song = Song::default();
        let slow_track = audio_track(&mut slow_song);
        assert!(slow_song.set_tempo_mark(0, 60.0));
        let slow = crate::tempo::TempoTable::build(&slow_song, 48_000.0, 120.0);
        slow_song
            .place_audio(slow_track, 0, "kick".to_owned(), source(), &slow)
            .expect("lands");
        let at_60 = slow_song.tracks[slow_track].audio_blocks[0].length_ticks;

        assert_eq!(at_120, TICKS_PER_BEAT);
        assert_eq!(
            at_60,
            TICKS_PER_BEAT / 2,
            "half the tempo, half the ticks for the same half-second"
        );
    }

    /// Every refusal fires, by name, and none of them is silent.
    #[test]
    fn landing_refuses_out_loud_for_every_reason() {
        let mut song = Song::default();
        let tempo = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);

        assert_eq!(
            song.place_audio(99, 0, "kick".to_owned(), source(), &tempo),
            Err(LandRefusal::NoTrack)
        );
        // Track 0 of a default song is an INSTRUMENT track.
        assert_eq!(
            song.place_audio(0, 0, "kick".to_owned(), source(), &tempo),
            Err(LandRefusal::NotAnAudioTrack)
        );

        let track = audio_track(&mut song);
        let mut broken = source();
        broken.source_frames = 0;
        assert_eq!(
            song.place_audio(track, 0, "kick".to_owned(), broken, &tempo),
            Err(LandRefusal::Unreadable)
        );

        song.place_audio(track, 0, "kick".to_owned(), source(), &tempo)
            .expect("lands");
        assert_eq!(
            song.place_audio(track, 0, "kick".to_owned(), source(), &tempo),
            Err(LandRefusal::Occupied),
            "a second sound cannot sit on the first"
        );

        // And every sign is distinct, so a refusal names its own reason.
        let signs = [
            LandRefusal::NoTrack.sign(),
            LandRefusal::NotAnAudioTrack.sign(),
            LandRefusal::Occupied.sign(),
            LandRefusal::Unreadable.sign(),
        ];
        for (index, sign) in signs.iter().enumerate() {
            assert!(sign.starts_with("LAND: "));
            assert!(!signs[..index].contains(sign), "signs are distinct");
        }
    }

    /// A pattern block occupies the lane too. An overlap test that read
    /// only `audio_blocks` would drop a recording on top of a pattern.
    #[test]
    fn landing_refuses_where_a_pattern_block_already_sits() {
        let mut song = Song::default();
        // Make track 0 an audio track while keeping its pattern block.
        song.tracks[0].kind = TrackKind::Audio;
        let tempo = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);
        assert_eq!(
            song.place_audio(0, 0, "kick".to_owned(), source(), &tempo),
            Err(LandRefusal::Occupied)
        );
        // Past the pattern block there is room.
        assert!(
            song.place_audio(
                0,
                DEFAULT_PATTERN_TICKS,
                "kick".to_owned(),
                source(),
                &tempo
            )
            .is_ok()
        );
    }

    /// Block ids are unique across BOTH lists, or a lookup by id becomes
    /// ambiguous the moment a track holds one of each.
    #[test]
    fn ids_are_unique_across_both_lists() {
        let mut song = Song::default();
        let track = audio_track(&mut song);
        let tempo = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);
        let first = song
            .place_audio(track, 0, "kick".to_owned(), source(), &tempo)
            .expect("lands");
        let second = song
            .place_audio(track, 480, "kick".to_owned(), source(), &tempo)
            .expect("lands");
        let pattern_id = song.tracks[0].blocks[0].id;
        assert_ne!(first, second);
        assert_ne!(first, pattern_id);
        assert_ne!(second, pattern_id);
    }

    /// The merged view is what the time verbs will walk, so it must be in
    /// time order regardless of which list a block came from.
    #[test]
    fn the_merged_view_is_in_time_order_across_both_lists() {
        let mut song = Song::default();
        let track = &mut song.tracks[0];
        track.audio_blocks.push(audio(9, 2_000, 100));
        track.audio_blocks.push(audio(8, 100, 100));

        let order: Vec<usize> = track
            .blocks_in_time_order()
            .iter()
            .map(BlockRef::start_tick)
            .collect();
        assert_eq!(order, vec![0, 100, 2_000], "interleaved, not concatenated");

        let ids: Vec<u64> = track
            .blocks_in_time_order()
            .iter()
            .map(|block| block.id().0)
            .collect();
        assert_eq!(ids, vec![1, 8, 9]);
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn a_new_song_has_an_empty_session_of_the_default_size() {
        let song = Song::default();
        assert_eq!(song.session.scenes.len(), SESSION_SCENES);
        assert!(
            song.session
                .scenes
                .iter()
                .all(|scene| scene.slots.is_empty())
        );
        assert_eq!(song.slot_clip(0, 0), None);
    }

    #[test]
    fn filling_a_slot_mints_a_pattern_and_keeps_it_there() {
        let mut song = Song::default();
        let before = song.patterns.len();
        let id = song
            .fill_slot(0, 2)
            .expect("an instrument slot refused a clip");
        assert_eq!(song.patterns.len(), before + 1, "no pattern was minted");
        assert!(song.patterns.iter().any(|pattern| pattern.id == id));
        assert_eq!(song.slot_clip(0, 2), Some(Clip::Pattern(id)));
        assert_eq!(
            song.slot_clip(0, 1),
            None,
            "the clip leaked into another scene"
        );
        assert!(
            song.tracks[0]
                .blocks
                .iter()
                .all(|block| block.pattern_id != id),
            "a session clip landed on the timeline"
        );
    }

    #[test]
    fn a_filled_slot_is_never_silently_replaced() {
        let mut song = Song::default();
        let first = song.fill_slot(0, 0).expect("first fill");
        let patterns = song.patterns.len();
        assert_eq!(song.fill_slot(0, 0), None, "a clip was overwritten");
        assert_eq!(
            song.patterns.len(),
            patterns,
            "a refused fill still minted a pattern"
        );
        assert_eq!(song.slot_clip(0, 0), Some(Clip::Pattern(first)));
    }

    #[test]
    fn an_audio_track_and_a_missing_place_refuse_a_pattern() {
        let mut song = Song::default();
        song.add_track(TrackKind::Audio);
        assert_eq!(song.fill_slot(1, 0), None, "an audio slot took a pattern");
        assert_eq!(
            song.fill_slot(0, SESSION_SCENES),
            None,
            "a scene past the end took a clip"
        );
        assert_eq!(
            song.fill_slot(9, 0),
            None,
            "a track that does not exist took a clip"
        );
        assert_eq!(song.patterns.len(), 1, "a refused fill minted a pattern");
    }

    #[test]
    fn clearing_returns_the_clip_and_leaves_the_pattern_in_the_song() {
        let mut song = Song::default();
        let id = song.fill_slot(0, 3).expect("fill");
        assert_eq!(song.clear_slot(0, 3), Some(Clip::Pattern(id)));
        assert_eq!(song.slot_clip(0, 3), None);
        assert!(
            song.patterns.iter().any(|pattern| pattern.id == id),
            "clearing a slot deleted its pattern"
        );
        assert_eq!(
            song.clear_slot(0, 3),
            None,
            "an empty slot claimed to hold something"
        );
        assert!(
            song.fill_slot(0, 3).is_some(),
            "a cleared slot could not be refilled"
        );
    }

    #[test]
    fn clips_follow_their_track_by_id_not_by_position() {
        let mut song = Song::default();
        song.add_track(TrackKind::Instrument);
        let id = song.fill_slot(1, 0).expect("fill on the second track");
        song.tracks.swap(0, 1);
        assert_eq!(
            song.slot_clip(0, 0),
            Some(Clip::Pattern(id)),
            "the clip stayed at the old position"
        );
        assert_eq!(song.slot_clip(1, 0), None);
    }

    #[test]
    fn the_session_survives_the_document_and_is_defaulted_when_absent() {
        let mut song = Song::default();
        song.fill_slot(0, 5).expect("fill");
        let text = ron::to_string(&song).expect("serialise");
        let back: Song = ron::from_str(&text).expect("deserialise");
        assert_eq!(back.session, song.session);

        // A document from before the session existed names no `session`
        // field at all, and must open with the empty default rather than
        // refuse.
        let without: String = text
            .split_once("session:")
            .map(|(head, tail)| {
                let rest = tail
                    .split_once("key:")
                    .map(|(_, rest)| rest)
                    .expect("key field");
                format!("{head}key:{rest}")
            })
            .expect("the session was not written");
        let old: Song = ron::from_str(&without).expect("an old document refused to open");
        assert_eq!(old.session, Session::default());
    }
}

#[cfg(test)]
mod tick_tests {
    use super::*;
    use crate::intent::sequence::Intent;
    use crate::pitch::Pitch;

    fn toggle(tick: usize) -> Intent {
        Intent::Toggle {
            tick,
            default_pitch: Pitch::from_midi(60),
            default_length_ticks: 6,
            default_velocity: 100,
        }
    }

    #[test]
    fn a_thirty_second_lands_in_its_step_at_its_offset() {
        let mut pattern = Pattern::default();
        assert_eq!(pattern.apply(&toggle(6)), None);
        let trig = pattern.trig(0);
        assert!(trig.enabled);
        assert_eq!(trig.notes.len(), 1);
        assert_eq!(trig.notes[0].micro_ticks, 6, "the offset was rounded away");
        // A sixty-fourth and a triplet land exactly too.
        assert_eq!(pattern.apply(&toggle(3)), None);
        assert_eq!(pattern.apply(&toggle(8)), None);
        let micros: Vec<i16> = pattern
            .trig(0)
            .notes
            .iter()
            .map(|n| n.micro_ticks)
            .collect();
        assert_eq!(micros, vec![3, 6, 8], "notes are not kept in time order");
        assert_eq!(Pattern::address(6), (0, 6));
        assert_eq!(Pattern::address(20), (1, 8));
    }

    #[test]
    fn toggling_exactly_a_note_gates_the_step_and_elsewhere_adds_a_note() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&toggle(0));
        assert!(
            !pattern.trig(0).enabled,
            "a second toggle on the note did not gate it"
        );
        assert_eq!(pattern.trig(0).notes.len(), 1);
        pattern.apply(&toggle(6));
        assert!(
            pattern.trig(0).enabled,
            "adding a note did not reopen the gate"
        );
        assert_eq!(pattern.trig(0).notes.len(), 2);
    }

    #[test]
    fn clearing_a_tick_leaves_the_steps_other_notes_alone() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&toggle(6));
        pattern.apply(&Intent::Clear { tick: 6 });
        assert_eq!(pattern.trig(0).notes.len(), 1);
        assert_eq!(pattern.trig(0).notes[0].micro_ticks, 0);
        assert!(pattern.trig(0).enabled);
        pattern.apply(&Intent::Clear { tick: 0 });
        assert!(pattern.trig(0).notes.is_empty());
        assert!(
            !pattern.trig(0).enabled,
            "an emptied step stayed gated open"
        );
    }

    #[test]
    fn a_nudge_moves_by_ticks_across_steps_and_refuses_an_occupied_tick() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(6));
        assert_eq!(
            pattern.apply(&Intent::Nudge {
                tick: 6,
                delta_ticks: 9,
            }),
            None
        );
        assert!(pattern.trig(0).notes.is_empty());
        assert_eq!(
            pattern.trig(1).notes[0].micro_ticks,
            3,
            "6 + 9 is step 1, offset 3"
        );
        pattern.apply(&toggle(12));
        assert_eq!(
            pattern.apply(&Intent::Nudge {
                tick: 15,
                delta_ticks: -3,
            }),
            Some("nudge blocked by an occupied step")
        );
        assert_eq!(
            pattern.apply(&Intent::Nudge {
                tick: 12,
                delta_ticks: -13,
            }),
            Some("nudge blocked at the pattern edge")
        );
    }

    #[test]
    fn a_note_nudge_moves_one_tone_and_can_join_another_stack() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&Intent::AddNote {
            tick: 0,
            pitch: Pitch::from_midi(64),
            length_ticks: 12,
            velocity: 90,
            probability: 1.0,
        });
        pattern.apply(&Intent::AddNote {
            tick: 12,
            pitch: Pitch::from_midi(67),
            length_ticks: 12,
            velocity: 80,
            probability: 1.0,
        });

        assert_eq!(
            pattern.apply(&Intent::NudgeNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta_ticks: 12,
            }),
            None
        );
        assert_eq!(pattern.trig(0).notes.len(), 1, "the C sibling moved too");
        assert!(
            pattern
                .trig(1)
                .notes
                .iter()
                .any(|note| note.pitch == Pitch::from_midi(64))
        );
        assert!(
            pattern
                .trig(1)
                .notes
                .iter()
                .any(|note| note.pitch == Pitch::from_midi(67)),
            "joining the G stack replaced it"
        );
    }

    #[test]
    fn transpose_note_is_individual_and_transpose_moves_the_whole_stack() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&Intent::AddNote {
            tick: 0,
            pitch: Pitch::from_midi(64),
            length_ticks: 12,
            velocity: 90,
            probability: 1.0,
        });

        assert_eq!(
            pattern.apply(&Intent::TransposeNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta_semitones: 12,
            }),
            None
        );
        let key = crate::pitch::default_key();
        let pitches = |pattern: &Pattern| {
            pattern
                .trig(0)
                .notes
                .iter()
                .map(|note| crate::pitch::nearest_midi(note.pitch.resolve(&key)))
                .collect::<Vec<_>>()
        };
        assert_eq!(pitches(&pattern), vec![60, 76]);

        assert_eq!(
            pattern.apply(&Intent::Transpose {
                tick: 0,
                delta_semitones: -12,
            }),
            None
        );
        assert_eq!(pitches(&pattern), vec![48, 64]);
    }

    #[test]
    fn velocity_and_length_address_only_the_note_at_the_tick() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&toggle(6));
        pattern.apply(&Intent::AdjustVelocity {
            tick: 6,
            delta: -50,
        });
        pattern.apply(&Intent::Resize {
            tick: 6,
            delta_ticks: 6,
        });
        let notes = &pattern.trig(0).notes;
        assert_eq!((notes[0].velocity, notes[0].length_ticks), (100, 6));
        assert_eq!((notes[1].velocity, notes[1].length_ticks), (50, 12));
        assert_eq!(
            pattern.apply(&Intent::AdjustVelocity { tick: 3, delta: 1 }),
            Some("velocity: no trig here")
        );
    }

    #[test]
    fn per_note_resize_velocity_mute_and_delete_leave_stack_siblings_alone() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(0));
        pattern.apply(&Intent::AddNote {
            tick: 0,
            pitch: Pitch::from_midi(64),
            length_ticks: 6,
            velocity: 90,
            probability: 1.0,
        });
        pattern.apply(&Intent::ResizeNote {
            tick: 0,
            pitch: Pitch::from_midi(64),
            delta_ticks: 6,
        });
        pattern.apply(&Intent::AdjustNoteVelocity {
            tick: 0,
            pitch: Pitch::from_midi(64),
            delta: -10,
        });
        pattern.apply(&Intent::SetNoteMuted {
            tick: 0,
            pitch: Pitch::from_midi(64),
            muted: true,
        });
        let e = pattern
            .trig(0)
            .notes
            .iter()
            .find(|note| note.pitch == Pitch::from_midi(64))
            .expect("E remains in the stack");
        assert_eq!((e.length_ticks, e.velocity, e.muted), (12, 80, true));

        pattern.apply(&Intent::RemoveNote {
            tick: 0,
            pitch: Pitch::from_midi(64),
        });
        assert_eq!(pattern.trig(0).notes.len(), 1);
        assert_eq!(pattern.trig(0).notes[0].pitch, Pitch::from_midi(60));
    }

    #[test]
    fn clip_resize_changes_the_boundary_without_destroying_hidden_notes() {
        let mut pattern = Pattern::default();
        pattern.apply(&toggle(DEFAULT_PATTERN_TICKS - PATTERN_STEP_TICKS));
        assert_eq!(
            pattern.apply(&Intent::ResizeClip {
                delta_ticks: -(TICKS_PER_BEAT as isize),
            }),
            None
        );
        assert_eq!(pattern.length_ticks, DEFAULT_PATTERN_TICKS - TICKS_PER_BEAT);
        assert!(
            !pattern.trig(PATTERN_STEPS - 1).notes.is_empty(),
            "shrinking hid the tail note instead of deleting it"
        );
        assert_eq!(
            pattern.apply(&Intent::ResizeClip {
                delta_ticks: TICKS_PER_BEAT as isize,
            }),
            None
        );
        assert_eq!(pattern.length_ticks, DEFAULT_PATTERN_TICKS);
        assert_eq!(
            pattern.apply(&Intent::ResizeClip { delta_ticks: 1 }),
            Some("clip resize blocked at the pattern edge")
        );
    }
}
