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

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Note {
    pub pitch: Pitch,
    pub length_ticks: usize,
    pub velocity: u8,
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
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    trigs: Vec<Trig>,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            id: PatternId(1),
            name: "P01".to_owned(),
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

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Song {
    pub tracks: Vec<Track>,
    pub patterns: Vec<Pattern>,
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
    /// The project-level harmonic context. Degree anchors resolve against
    /// it; absolute anchors never read it.
    pub key: Key,
}

impl Default for Song {
    fn default() -> Self {
        let pattern = Pattern::default();
        let pattern_id = pattern.id;
        Self {
            tracks: vec![Track {
                id: TrackId(1),
                name: "INSTRUMENT 01".to_owned(),
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
                depth: 0,
                input: TrackInput::default(),
                monitor: Monitor::default(),
                armed: false,
            }],
            patterns: vec![pattern],
            returns: Vec::new(),
            tempo: Vec::new(),
            meter: Vec::new(),
            key: default_key(),
        }
    }
}

impl Song {
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

        let pattern_number = self.patterns.len().checked_add(1)?;
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
        self.patterns
            .push(Pattern::empty(pattern_id, format!("P{pattern_number:02}")));
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
