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
pub const DEFAULT_PATTERN_TICKS: usize = PATTERN_STEPS * 12;

/// The canonical automation target ids the mixer speaks.
///
/// These strings are FILE FORMAT. They are shared with the legacy
/// `targets` table, and `redesign_bridge` asserts the two still agree —
/// a silent divergence here orphans every envelope already on disk.
pub const TRACK_VOLUME: &str = "track.volume";
pub const TRACK_PAN: &str = "track.pan";

/// Unity gain. A serde default, because a track absent from a pre-mixer
/// document must come back at unity: defaulting a fader to zero would
/// silently mute every project written before the mixer existed.
fn unity() -> f32 {
    1.0
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
}

impl Track {
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
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Song {
    pub tracks: Vec<Track>,
    pub patterns: Vec<Pattern>,
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
                automation: Vec::new(),
                volume: 1.0,
                pan: 0.0,
            }],
            patterns: vec![pattern],
            key: default_key(),
        }
    }
}

impl Song {
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
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
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
