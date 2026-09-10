//! The METER: what the machine was told, and what is written under it.
//!
//! Two readings side by side, and neither is a summary.
//!
//! On the left, every intent the stage actually ANSWERED — the verbs,
//! not the keys. A key means a different thing in every scope, so a
//! column of keystrokes would be a column that has to be decoded before
//! it can be read; the verb is what happened. Newest at the foot, so
//! the stream rises the same way the steps do.
//!
//! On the right, the song as a tracker reads it: one column group per
//! track, one row per step, rising through a NOW-LINE that does not
//! move. The rows are SONG steps rather than any one pattern's, so
//! tracks of different lengths stay in the time they are actually
//! played in and a polymetric session reads as the wheel it is.
//!
//! Cells and columns are frame-local projections of the document, never
//! a second musical authority. Editing goes through the same sequence
//! intents and undo history as the other editors. The core keeps only
//! navigation state and the bounded ring of answered intents.
//!
//! The painter is `view::meter`.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::devices::{DeviceKind, ParamLabel};
use crate::sequencing::{Clip, PATTERN_STEP_TICKS, ParamLock, Pattern, Trig};

#[cfg(test)]
thread_local! {
    static DERIVATIONS: std::cell::Cell<[usize; 3]> = const { std::cell::Cell::new([0; 3]) };
}

#[cfg(test)]
fn count_derivation(index: usize) {
    DERIVATIONS.with(|counts| {
        let mut next = counts.get();
        next[index] += 1;
        counts.set(next);
    });
}

#[cfg(test)]
pub(super) fn take_derivation_counts() -> [usize; 3] {
    DERIVATIONS.with(|counts| counts.replace([0; 3]))
}

/// How many answered intents the panel remembers. Deep enough that the
/// section opens onto a stream already running rather than a blank
/// that fills as you type; shallow enough to be a ring, not a log.
pub(super) const INPUTS: usize = 96;

/// One answered intent, as the panel reads it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct InputLine {
    /// The section the verb belongs to, as the codebook files it.
    pub(super) family: &'static str,
    /// The verb itself.
    pub(super) word: &'static str,
    /// The stage's uptime when this line last fired, in seconds.
    pub(super) at: f32,
    /// Repeats collapsed. Holding an arrow down is one line that
    /// counts, not forty lines that scroll the panel away.
    pub(super) count: u32,
    /// The stage refused it. Drawn as the refusal it was: a section
    /// that showed a refused verb as an answered one would be lying
    /// about the only thing it exists to report.
    pub(super) refused: bool,
}

/// The section's own state: the stream, and where the reader is.
#[derive(Debug, Default)]
pub(super) struct Meter {
    pub(super) open: bool,
    lines: VecDeque<InputLine>,
    /// The ABSOLUTE song step the now-line stands on while the reader
    /// has the wheel. `None` is RIDING: the now-line is the transport's
    /// own step and the rows come to it.
    ///
    /// Absolute, not a distance from the playhead, and that is the
    /// whole difference between held and displaced: a stored distance
    /// would keep the body moving under a reader who had just asked it
    /// to stop, and the title would say HELD over a surface that was
    /// still scrolling.
    pub(super) held: Option<isize>,
    /// Where the cursor stands ACROSS the body: an index into the flat
    /// plan of every field of every track, left to right. One number
    /// rather than a track and a field inside it, because the cursor
    /// walks the whole body as one line — which is what makes a block
    /// selection a rectangle in two numbers instead of four.
    pub(super) at: usize,
    /// The block's other corner: `(row, at)`, or nothing while the
    /// cursor is a single cell.
    pub(super) anchor: Option<(isize, usize)>,
    /// Which of the voice's parameters the ADD column is pointing at,
    /// as an index into that voice's own table. A parameter no step
    /// locks yet has no column to be edited in, so the ADD column is
    /// how one is started; `,` and `.` walk it, the way the forge walks
    /// its passes.
    pub(super) add: usize,
    /// The stage's uptime, fed by the frame. The core reads no clock;
    /// this is the same second-hand every stamp here is measured
    /// against, so the panel's ages are consistent by construction.
    pub(super) uptime: f32,
}

impl Meter {
    /// Let the frame's time pass.
    pub(super) fn tick(&mut self, dt: f32) {
        self.uptime += dt.max(0.0);
    }

    /// Record an intent the stage answered.
    ///
    /// Recorded whether or not the section is open: the stream is the
    /// machine's, not the window's, and a section that only remembers
    /// what happened while it was being watched would open onto
    /// nothing every time.
    pub(super) fn note(&mut self, family: &'static str, word: &'static str, refused: bool) {
        if let Some(last) = self.lines.back_mut()
            && last.word == word
            && last.refused == refused
        {
            last.count = last.count.saturating_add(1);
            last.at = self.uptime;
            return;
        }
        self.lines.push_back(InputLine {
            family,
            word,
            at: self.uptime,
            count: 1,
            refused,
        });
        while self.lines.len() > INPUTS {
            self.lines.pop_front();
        }
    }

    /// The stream, oldest first.
    pub(super) fn lines(&self) -> impl DoubleEndedIterator<Item = &InputLine> {
        self.lines.iter()
    }

    /// Walk the body by hand. The first walk takes the wheel from the
    /// playhead — anchoring on the step it was standing on — and from
    /// then on the rows stand still under the reader.
    pub(super) fn scroll(&mut self, by: isize, now: isize) {
        self.held = Some(self.held.unwrap_or(now).saturating_add(by));
    }

    /// Give the wheel back to the transport.
    pub(super) fn follow(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// Walk the cursor sideways across the flat plan.
    pub(super) fn walk_field(&mut self, right: bool, fields: usize) -> bool {
        let last = fields.saturating_sub(1);
        let to = if right {
            (self.at + 1).min(last)
        } else {
            self.at.saturating_sub(1)
        };
        let moved = to != self.at;
        self.at = to;
        moved
    }

    /// Everything a closed section should forget. The stream is NOT
    /// forgotten: it belongs to the machine, and the reader who opens
    /// the section again is asking what has been happening.
    pub(super) fn close(&mut self) {
        self.open = false;
        self.held = None;
        self.anchor = None;
    }
}

/// One editable field of one track: a column of the body.
///
/// A tracker's columns are not decoration — they are the addresses the
/// cursor walks and the edits land on. Note and velocity are always
/// there; a lock column exists for every parameter the pattern actually
/// locks somewhere, so the columns are the music's own shape rather
/// than a fixed table most of which would be empty. ADD is the one
/// column that is not data: it is how a parameter nothing locks yet
/// gets its first lock, and therefore its column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    Note,
    Vel,
    /// How long the note is held, in ticks.
    Len,
    /// Fire only on pass A of every B cycles.
    Cond,
    /// Fire this often, as hundredths.
    Prob,
    /// Repeats inside the step: how many, how fast.
    Rtg,
    /// Sub-step displacement: the pushed hat, the dragged snare.
    Micro,
    /// A whole other sound on this one step.
    Snd,
    Lock {
        device: Option<crate::sequencing::DeviceId>,
        param: u32,
    },
    Add,
}

impl Field {
    /// How many characters wide the column is drawn.
    pub(super) fn width(self) -> usize {
        match self {
            // Five, not four: four holds the widest spelling exactly
            // and leaves the rule mark that follows it touching the
            // note. A column whose glyphs collide is a column that has
            // to be read twice.
            Self::Note => 5,
            Self::Vel => 2,
            Self::Len | Self::Cond | Self::Prob | Self::Rtg | Self::Micro => 3,
            Self::Snd => 4,
            Self::Lock { .. } => 6,
            Self::Add => 6,
        }
    }
}

/// A track's heading: what the column is a column of.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Column {
    pub(super) name: String,
    /// The track's letter, as every clip on it is tagged.
    pub(super) letter: String,
    /// The machine the trigs address, by name.
    pub(super) machine: &'static str,
    /// The tag of the pattern sounding here, when one is.
    pub(super) clip: Option<String>,
    /// This is a drum lane: the one column colour that names something
    /// musical rather than something the machine says about itself.
    pub(super) drum: bool,
    pub(super) muted: bool,
    /// This track's columns, in order.
    pub(super) fields: Vec<Field>,
    /// What each field is called at the head, already cut to the
    /// column's width.
    pub(super) heads: Vec<String>,
}

/// Every field of every track, left to right, with the columns they
/// belong to. Derived fresh each time it is asked for: the cursor is an
/// index into this, so the plan and the cursor cannot disagree about
/// what the cursor is on.
#[derive(Clone, Debug, Default)]
pub(super) struct Plan {
    pub(super) columns: Vec<Column>,
    /// `(track, field)` for every column of the body.
    pub(super) flat: Vec<(usize, Field)>,
    starts: Vec<usize>,
}

impl Plan {
    pub(super) fn at(&self, at: usize) -> Option<(usize, Field)> {
        self.flat.get(at).copied()
    }

    /// Where a track's fields begin in the flat plan.
    pub(super) fn start_of(&self, track: usize) -> usize {
        self.starts.get(track).copied().unwrap_or(0)
    }
}

/// One lock, named and read.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Lock {
    /// The address the lock is on, so a cell and a COLUMN can be
    /// matched to each other. Without it the surface would have to
    /// match locks to columns by their names, and two devices are
    /// allowed to call a parameter the same thing.
    pub(super) device: Option<crate::sequencing::DeviceId>,
    pub(super) param: u32,
    pub(super) name: &'static str,
    pub(super) reading: String,
    /// The lock glides from here to the next lock on the same address.
    pub(super) slide: bool,
    /// The lock is on an effect rather than the voice.
    pub(super) effect: bool,
}

/// One track's step, as the tracker reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Cell {
    /// A trig stands here at all.
    pub(super) written: bool,
    /// It will fire: written, and not turned off.
    pub(super) sounding: bool,
    /// The first note's name and velocity.
    pub(super) note: Option<(String, u8)>,
    /// How many more notes the step holds beyond the first: a chord is
    /// one step, not parallel lanes, so the extra notes are a count.
    pub(super) chord: usize,
    pub(super) locks: Vec<Lock>,
    /// Fires on pass A of every B cycles.
    pub(super) cond: Option<(u8, u8)>,
    /// Repeats inside the step: rate and count.
    pub(super) retrig: Option<(u8, u8)>,
    /// A whole other sound on this one step, by name.
    pub(super) sound: Option<String>,
    /// Probability below certainty, as hundredths.
    pub(super) chance: Option<u8>,
    /// Sub-step displacement, in ticks: the pushed hat, the dragged
    /// snare. Data, and shown as data.
    pub(super) micro: i16,
    /// How long the first note is held, in ticks.
    pub(super) length: usize,
}

impl Cell {
    /// Whether anything at all is written here worth an ink.
    pub(super) fn marked(&self) -> bool {
        self.written
    }

    /// The marks a compressed column shows beside the note: one per
    /// lock, capped, with a count taking over past the cap.
    pub(super) fn lock_count(&self) -> usize {
        self.locks.len()
    }
}

/// One row of the body: one step of song time across every track.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    /// The step in song time. Negative rows are before the song began
    /// and hold nothing — they exist so the now-line can sit anywhere
    /// in the body without the rows above it having to be invented.
    pub(super) step: isize,
    /// The bar this step falls in, and its beat, both from one.
    pub(super) bar: usize,
    pub(super) beat: usize,
    /// The step's place in its bar, from zero: the accent the eye reads
    /// the grid by.
    pub(super) in_bar: usize,
    /// One per track, in the song's order. `None` is a track playing
    /// nothing here — which is a different fact from an empty step,
    /// and drawn as one.
    pub(super) cells: Vec<Option<Cell>>,
}

/// What a track is playing, for the length of one derivation: the
/// pattern, the song tick it started at, how long it loops, and the
/// machine its trigs lock. Gathered once per frame rather than once per
/// row, because it is the same answer for every row in the body.
#[derive(Clone, Copy, Debug)]
struct Playing<'a> {
    pattern: &'a Pattern,
    origin: usize,
    steps: usize,
    voice: Option<DeviceKind>,
}

/// Frame-local lookups, never persistent copies of device or musical state.
/// Walk the desk once; resolve each distinct kind/parameter label once, even
/// when hundreds of visible trigs address the same effect.
struct LockLabels {
    devices: HashMap<crate::sequencing::DeviceId, DeviceKind>,
    labels: HashMap<(DeviceKind, u32), Option<&'static ParamLabel>>,
}

impl LockLabels {
    fn new(song: &crate::sequencing::Song) -> Self {
        Self {
            devices: song
                .all_devices()
                .map(|device| (device.id, device.kind))
                .collect(),
            labels: HashMap::new(),
        }
    }

    fn get(&mut self, voice: Option<DeviceKind>, lock: &ParamLock) -> Option<&'static ParamLabel> {
        let kind = match lock.device {
            None => voice,
            Some(id) => self.devices.get(&id).copied(),
        }?;
        *self
            .labels
            .entry((kind, lock.param))
            .or_insert_with(|| label_of(kind, lock.param))
    }
}

/// How a parameter's value reads, in the words its own table uses.
fn reading(label: &ParamLabel, value: f32) -> String {
    if !label.choices.is_empty() {
        let at = value.round().max(0.0) as usize;
        if let Some(choice) = label.choices.get(at) {
            return (*choice).to_owned();
        }
    }
    let magnitude = value.abs();
    let digits = if magnitude >= 100.0 {
        0
    } else if magnitude >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.digits$}")
}

/// A locked address, named by the device's own table — the same list
/// the band and the modulation matrix read, so a lock and a wire
/// pointed at one parameter call it the same thing.
fn label_of(kind: DeviceKind, param: u32) -> Option<&'static ParamLabel> {
    let spec = kind.spec();
    let at = spec.params.iter().position(|def| def.id == param)?;
    spec.labels.get(at)
}

impl super::Stage {
    /// The body's plan: every track's columns, and the flat line of
    /// fields the cursor walks.
    pub(super) fn meter_plan(&self) -> Plan {
        #[cfg(test)]
        count_derivation(0);
        let mut plan = Plan::default();
        for (at, track) in self.song.tracks.iter().enumerate() {
            plan.starts.push(plan.flat.len());
            let (spec, _) = super::trig_menu::voice_of(track);
            let playing = self.meter_pattern(at);
            let pattern = playing.and_then(|(id, _)| self.song.pattern(id));
            let fields = self.meter_track_fields(pattern, spec.kind);
            let heads = fields
                .iter()
                .map(|field| self.meter_head_of(*field, spec.kind))
                .collect();
            plan.columns.push(Column {
                name: track.name.clone(),
                letter: track.letter.clone(),
                machine: spec.name,
                clip: pattern
                    .map(|pattern| pattern.tag.clone())
                    .filter(|tag| !tag.is_empty()),
                drum: track.lane == crate::lane::Lane::Drum,
                muted: track.muted,
                fields: fields.clone(),
                heads,
            });
            plan.flat
                .extend(fields.into_iter().map(|field| (at, field)));
        }
        plan
    }

    /// One track's columns: note, velocity, a column for every lock the
    /// pattern actually holds, and ADD.
    ///
    /// The lock columns come from the PATTERN, not from the machine's
    /// table: a poly synth has fifty parameters and a clip locks three,
    /// and forty-seven empty columns would bury the three. A parameter
    /// gets its column the moment any step locks it, and ADD is how the
    /// first lock is laid.
    fn meter_track_fields(&self, pattern: Option<&Pattern>, kind: DeviceKind) -> Vec<Field> {
        // The fixed columns first and always, so a parameter's column
        // never moves when a lock is laid on another track; the locks
        // vary and are therefore grouped at the end, next to the door
        // that makes them.
        let mut fields = vec![
            Field::Note,
            Field::Vel,
            Field::Len,
            Field::Cond,
            Field::Prob,
            Field::Rtg,
            Field::Micro,
            Field::Snd,
        ];
        let Some(pattern) = pattern else {
            return fields;
        };
        let steps = pattern.length_ticks / PATTERN_STEP_TICKS;
        let mut seen: Vec<(Option<crate::sequencing::DeviceId>, u32)> = Vec::new();
        let mut unique = HashSet::new();
        // trig() wraps at the stored length; scanning repetitions cannot
        // discover another column. Preserve first-seen order for equal keys.
        for step in 0..steps.max(1).min(pattern.step_count()) {
            for lock in &pattern.trig(step).locks {
                if unique.insert((lock.device, lock.param)) {
                    seen.push((lock.device, lock.param));
                }
            }
        }
        // In the VOICE's own order, so two clips on one machine put the
        // same parameter in the same place and the eye can move between
        // them. Effect locks follow, grouped by the device they are on.
        let order = |device: Option<crate::sequencing::DeviceId>, param: u32| {
            let spec = match device {
                None => kind.spec(),
                Some(id) => match self.song.device(id) {
                    Some(device) => device.kind.spec(),
                    None => return usize::MAX,
                },
            };
            spec.params
                .iter()
                .position(|def| def.id == param)
                .unwrap_or(usize::MAX)
        };
        seen.sort_by_cached_key(|&(device, param)| {
            (device.map(|id| id.0).unwrap_or(0), order(device, param))
        });
        fields.extend(
            seen.into_iter()
                .map(|(device, param)| Field::Lock { device, param }),
        );
        fields.push(Field::Add);
        fields
    }

    /// What a column is called at the head, cut to its width.
    fn meter_head_of(&self, field: Field, kind: DeviceKind) -> String {
        match field {
            Field::Note => "NOTE".to_owned(),
            Field::Vel => "VL".to_owned(),
            Field::Len => "LEN".to_owned(),
            Field::Cond => "CND".to_owned(),
            Field::Prob => "PRB".to_owned(),
            Field::Rtg => "RTG".to_owned(),
            Field::Micro => "MIC".to_owned(),
            Field::Snd => "SND".to_owned(),
            Field::Add => "+LOCK".to_owned(),
            Field::Lock { device, param } => {
                let spec = match device {
                    None => kind.spec(),
                    Some(id) => match self.song.device(id) {
                        Some(device) => device.kind.spec(),
                        None => return format!("{param}"),
                    },
                };
                let name = spec
                    .params
                    .iter()
                    .position(|def| def.id == param)
                    .and_then(|at| spec.labels.get(at))
                    .map(|label| label.name)
                    .unwrap_or("?");
                name.chars()
                    .take(Field::Lock { device, param }.width())
                    .collect()
            }
        }
    }

    /// The parameter the ADD column on `track` is pointing at.
    ///
    /// The voice's table minus what the pattern already locks, because
    /// a column that offered a parameter which already HAS a column
    /// would be offering the reader a second way to reach one address.
    #[cfg(test)]
    pub(super) fn meter_add_param(&self, track: usize) -> Option<(u32, &'static str)> {
        self.meter_add_param_from_plan(track, &self.meter_plan())
    }

    pub(super) fn meter_add_param_from_plan(
        &self,
        track: usize,
        plan: &Plan,
    ) -> Option<(u32, &'static str)> {
        let held = self.song.tracks.get(track)?;
        let (spec, _) = super::trig_menu::voice_of(held);
        let fields = &plan.columns.get(track)?.fields;
        let mut free = spec
            .params
            .iter()
            .zip(spec.labels)
            .filter(|(def, _)| {
                !fields.contains(&Field::Lock {
                    device: None,
                    param: def.id,
                })
            })
            .map(|(def, label)| (def.id, label.name));
        let count = free.clone().count();
        if count == 0 {
            return None;
        }
        free.nth(self.meter.add % count)
    }

    /// The pattern `track` is playing, and the song tick that pattern
    /// started at — the origin every row on that column folds against.
    ///
    /// Two ways for a pattern to be playing, and the transport's mode
    /// says which one is the truth right now: in the session a scene is
    /// launched and its clip loops from tick zero; in the song a block
    /// is laid at a tick and the pattern loops inside it. Neither is
    /// consulted for the other, because a section that showed the
    /// session's clips while the song was playing would be showing a
    /// pattern that is not sounding.
    fn meter_pattern(&self, track: usize) -> Option<(crate::sequencing::PatternId, usize)> {
        if self.song_mode() {
            let laid = self.song.tracks.get(track)?;
            let tick = self.transport.tick();
            let block = laid.blocks.iter().find(|block| {
                block.start_tick <= tick
                    && tick < block.start_tick.saturating_add(block.length_ticks)
            })?;
            return Some((block.pattern_id, block.start_tick));
        }
        let scene = self.playing.get(track).copied().flatten()?;
        let id = self.song.tracks.get(track)?.id;
        let Clip::Pattern(pattern) = self.song.session.scenes.get(scene)?.clip(id)?;
        Some((pattern, 0))
    }

    /// How long a pattern loops, in steps. Never zero: a pattern with
    /// no length would fold every row onto itself and divide by it.
    fn meter_steps(&self, id: crate::sequencing::PatternId) -> usize {
        (crate::ui::sequencer::pattern_length(&self.song, id) / PATTERN_STEP_TICKS).max(1)
    }

    /// The body's rows, from `first` for `count` steps.
    ///
    /// Resolved per row rather than per pattern because the rows are
    /// song time and the patterns are not: two tracks whose clips are
    /// three and four steps long meet again every twelve, and the only
    /// place that wheel exists is here.
    #[cfg(test)]
    pub(super) fn meter_rows(&self, first: isize, count: usize) -> Vec<Row> {
        self.meter_rows_visible(first, count, &vec![true; self.song.tracks.len()])
    }

    /// Invisible tracks retain their column indices but derive no cells.
    /// The readout shares these rows instead of performing another desk walk.
    pub(super) fn meter_rows_visible(
        &self,
        first: isize,
        count: usize,
        visible: &[bool],
    ) -> Vec<Row> {
        #[cfg(test)]
        count_derivation(1);
        // Match pattern_length's first-placement semantics without walking
        // every track's blocks again for every visible track.
        let mut lengths = HashMap::new();
        for block in self.song.tracks.iter().flat_map(|track| &track.blocks) {
            lengths
                .entry(block.pattern_id)
                .or_insert(block.length_ticks);
        }
        let playing: Vec<Option<Playing>> = self
            .song
            .tracks
            .iter()
            .enumerate()
            .map(|(at, track)| {
                if !visible.get(at).copied().unwrap_or(false) {
                    return None;
                }
                let (id, origin) = self.meter_pattern(at)?;
                let pattern = self.song.pattern(id)?;
                Some(Playing {
                    pattern,
                    origin,
                    steps: (lengths.get(&id).copied().unwrap_or(pattern.length_ticks)
                        / PATTERN_STEP_TICKS)
                        .max(1),
                    // `voice_of`, not `track.machine`: a track with no
                    // device placed still LOCKS something — the head of
                    // the chain, whatever it is — and the trig menu and
                    // the column head both name it through this
                    // fallback. Reading the field raw here left every
                    // lock on such a track nameless while the same lock
                    // on a track that happened to carry a device read
                    // fine, which is one fact told two ways.
                    voice: Some(super::trig_menu::voice_of(track).0.kind),
                })
            })
            .collect();
        let mut labels = LockLabels::new(&self.song);
        let per_bar = self.meter_steps_per_bar();
        (0..count)
            .map(|row| {
                let step = first + row as isize;
                let in_bar = step.rem_euclid(per_bar as isize) as usize;
                let bar = step.div_euclid(per_bar as isize);
                Row {
                    step,
                    bar: (bar + 1).max(0) as usize,
                    beat: in_bar / 4 + 1,
                    in_bar,
                    cells: playing
                        .iter()
                        .map(|held| {
                            let held = held.as_ref()?;
                            let origin = (held.origin / PATTERN_STEP_TICKS) as isize;
                            if step < origin {
                                return None;
                            }
                            let at = (step - origin).rem_euclid(held.steps as isize) as usize;
                            Some(self.meter_cell(held.pattern, at, held.voice, &mut labels))
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// Steps to a bar, from the song's own signature.
    pub(super) fn meter_steps_per_bar(&self) -> usize {
        let (beats, unit) = self
            .song
            .meter_at(self.transport.tick(), super::transport::DEFAULT_METER);
        let per_beat = (16 / unit.max(1)).max(1) as usize;
        (beats.max(1) as usize * per_beat).max(1)
    }

    /// One trig, read out.
    fn meter_cell(
        &self,
        pattern: &Pattern,
        step: usize,
        voice: Option<DeviceKind>,
        labels: &mut LockLabels,
    ) -> Cell {
        #[cfg(test)]
        count_derivation(2);
        let trig = pattern.trig(step);
        if !trig.enabled && trig.notes.is_empty() && trig.locks.is_empty() {
            return Cell::default();
        }
        let first = trig.notes.first();
        let note = first.map(|note| {
            let view = crate::ui::sequencer::note_view(
                note,
                0,
                trig.probability,
                trig.enabled,
                &self.song.key,
            );
            (note_name(view.midi), note.velocity)
        });
        Cell {
            written: true,
            sounding: trig.enabled,
            note,
            chord: trig.notes.len().saturating_sub(1),
            locks: self.meter_locks(trig, voice, labels),
            cond: trig.cond,
            retrig: trig.retrig.map(|retrig| (retrig.rate, retrig.count)),
            sound: trig.sound.as_ref().map(|sound| sound.name.clone()),
            chance: (trig.probability < 1.0)
                .then(|| (trig.probability.clamp(0.0, 1.0) * 100.0).round() as u8),
            micro: first.map_or(0, |note| note.micro_ticks),
            length: first.map_or(0, |note| note.length_ticks),
        }
    }

    /// A trig's locks, named against the devices they were laid on.
    ///
    /// A lock names its device by id, so a reordered chain keeps every
    /// lock on the device it was laid on — and an address whose device
    /// has since been removed is shown by its number rather than
    /// silently dropped, because a lock that is still in the document
    /// is still going to be played.
    fn meter_locks(
        &self,
        trig: &Trig,
        voice: Option<DeviceKind>,
        labels: &mut LockLabels,
    ) -> Vec<Lock> {
        trig.locks
            .iter()
            .map(|lock: &ParamLock| {
                let label = labels.get(voice, lock);
                Lock {
                    device: lock.device,
                    param: lock.param,
                    name: label.map_or("", |label| label.name),
                    reading: label.map_or_else(
                        || format!("{:.2}", lock.value),
                        |label| reading(label, lock.value),
                    ),
                    slide: lock.slide,
                    effect: lock.device.is_some(),
                }
            })
            .collect()
    }

    /// Where the now-line stands, and how far the body has glided
    /// between two steps.
    ///
    /// The glide is what makes the rows RISE rather than jump: a step
    /// at 120bpm is 125ms, and a body that moved once per step would
    /// stutter eight times a second. The fraction comes from the beat
    /// phase the transport already publishes, so there is no second
    /// clock to disagree with the first.
    pub(super) fn meter_now(&self) -> (isize, f32) {
        // A held body does not glide. The glide exists to carry the
        // rows toward a moment that is arriving; when the reader has
        // the wheel, no moment is arriving, and a surface that kept
        // sliding would be contradicting its own title.
        if let Some(at) = self.meter.held {
            return (at, 0.0);
        }
        let per_beat = (crate::sequencing::TICKS_PER_BEAT / PATTERN_STEP_TICKS) as f32;
        let step = (self.transport.tick() / PATTERN_STEP_TICKS) as isize;
        let glide = if self.transport.motion().is_rolling() {
            (self.transport.beat_phase() * per_beat).fract()
        } else {
            0.0
        };
        (step, glide)
    }
}

/// A midi number, spelled the way a tracker spells it: two characters
/// for the note, then the octave. The natural's dash is not decoration —
/// it is what keeps every note in the column exactly as wide as every
/// other, which is the whole reason a tracker is readable at a glance.
/// The one wide spelling is the sub-audio octave, `C--1`, and it is
/// better for that to look unusual than for it to look like `C-1`.
fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
    ];
    let octave = i32::from(midi) / 12 - 1;
    format!("{}{octave}", NAMES[usize::from(midi) % 12])
}

impl super::Stage {
    /// Write an answered intent into the stream.
    ///
    /// Called from `apply`, at both of its exits, which is the one place
    /// every verb the stage answers passes through — the chord, the
    /// palette's typed command and the test driver alike. A recorder
    /// hung on the keyboard instead would miss the palette, and a
    /// section whose whole claim is "everything you told it" cannot
    /// afford a door it does not watch.
    pub(super) fn meter_note(&mut self, intent: super::StageIntent, refused: bool) {
        self.meter
            .note(super::keymap::family(intent), intent.label(), refused);
    }

    /// The meter's own verbs.
    pub(super) fn apply_meter(
        &mut self,
        intent: super::MeterIntent,
    ) -> Result<(), super::RefusalReason> {
        use super::MeterIntent as I;
        use super::RefusalReason;
        if intent == I::Open {
            if self.meter.open {
                self.meter.close();
                return Ok(());
            }
            // The meter opens from the session and from inside a clip,
            // and from nowhere else.
            //
            // The chord already says so — it is bound in those scopes
            // and no others — but the palette's `:meter` is a second
            // door into the same room, and a door that skipped the rule
            // would let the section open UNDER a window that still owns
            // the keyboard: visible, and answering nothing. So the rule
            // lives here, where both doors pass through it, rather than
            // in the binding table where only one of them would.
            use super::keymap::ScopeContext as S;
            if !matches!(
                self.scope_context(),
                S::Root | S::Nested | S::Song | S::Mixer | S::Clip
            ) {
                return Err(RefusalReason::Unavailable);
            }
            self.meter.open = true;
            self.meter.held = None;
            self.meter.anchor = None;
            self.meter.at = self
                .meter
                .at
                .min(self.meter_plan().flat.len().saturating_sub(1));
            self.notice = Some("meter".to_owned());
            return Ok(());
        }
        if !self.meter.open {
            return Err(RefusalReason::Unavailable);
        }
        let bar = self.meter_steps_per_bar() as isize;
        let standing = self.meter_now().0;
        let plan = self.meter_plan();
        let fields = plan.flat.len();
        // A bare walk drops the block; a shifted one drags it. Nothing
        // else clears a selection by accident, which is what lets a
        // reader walk out to look at something and come back.
        let mut drop_block = true;
        match intent {
            I::Open => unreachable!("handled above"),
            I::Back => self.meter.scroll(-1, standing),
            I::Forward => self.meter.scroll(1, standing),
            I::PageBack => self.meter.scroll(-bar, standing),
            I::PageForward => self.meter.scroll(bar, standing),
            I::SelectBack | I::SelectForward | I::SelectLeft | I::SelectRight => {
                drop_block = false;
                // Whether the walk CAN happen is settled before the
                // block is started. A selection begun by a keystroke
                // that then refused would be state changed by a key
                // that reported changing nothing — and the reader would
                // be left holding a block they never asked for.
                let sideways = matches!(intent, I::SelectLeft | I::SelectRight);
                let right = intent == I::SelectRight;
                if sideways {
                    let can = if right {
                        self.meter.at + 1 < fields
                    } else {
                        self.meter.at > 0
                    };
                    if !can {
                        return Err(RefusalReason::Edge(if right {
                            super::Step::Right
                        } else {
                            super::Step::Left
                        }));
                    }
                }
                self.meter.anchor.get_or_insert((standing, self.meter.at));
                match intent {
                    I::SelectBack => self.meter.scroll(-1, standing),
                    I::SelectForward => self.meter.scroll(1, standing),
                    _ => {
                        self.meter.walk_field(right, fields);
                    }
                }
            }
            I::Left | I::Right => {
                if fields == 0 {
                    return Err(RefusalReason::Empty);
                }
                if !self.meter.walk_field(intent == I::Right, fields) {
                    return Err(RefusalReason::Edge(if intent == I::Right {
                        super::Step::Right
                    } else {
                        super::Step::Left
                    }));
                }
            }
            I::Follow => {
                // Already riding: there is no wheel to give back. A key
                // that reported a change here would be a key that lied,
                // and the title already says FOLLOWING, so the reader
                // has their answer without one.
                if !self.meter.follow() {
                    return Err(RefusalReason::Unavailable);
                }
                self.notice = Some("meter · following".to_owned());
            }
            // ------------------------------------------------- the edits
            I::Turn { up, coarse } => {
                return self.meter_edit(Edit::Turn { up, coarse });
            }
            I::Trig => {
                return self.meter_edit(Edit::Trig);
            }
            I::Chord(letter) => {
                return self.meter_edit(Edit::Chord(LETTER_CLASS[usize::from(letter) % 7]));
            }
            I::Slide => {
                return self.meter_edit(Edit::Slide);
            }
            I::Clear => {
                return self.meter_edit(Edit::Clear);
            }
            I::Letter(letter) => {
                drop_block = false;
                // One key, and the COLUMN says what it means. The
                // cursor's own column decides for the whole block, so a
                // block spanning both kinds does one thing rather than
                // two — a key that meant two things at once would be a
                // key nobody could aim.
                let edit = match plan.at(self.meter.at).map(|(_, field)| field) {
                    Some(Field::Vel) if letter < 6 => Edit::Nibble(10 + letter),
                    Some(Field::Note) => Edit::Pitch(LETTER_CLASS[usize::from(letter) % 7]),
                    _ => return Err(RefusalReason::Unavailable),
                };
                return self.meter_edit(edit);
            }
            I::Digit(digit) => {
                drop_block = false;
                if !matches!(plan.at(self.meter.at), Some((_, Field::Vel))) {
                    return Err(RefusalReason::Unavailable);
                }
                return self.meter_edit(Edit::Nibble(digit));
            }
            I::AddPrev | I::AddNext => {
                drop_block = false;
                let Some((track, _)) = plan.at(self.meter.at) else {
                    return Err(RefusalReason::Empty);
                };
                let next = if intent == I::AddNext {
                    self.meter.add.wrapping_add(1)
                } else {
                    self.meter.add.wrapping_sub(1)
                };
                self.meter.add = next;
                match self.meter_add_param_from_plan(track, &plan) {
                    Some((_, name)) => self.notice = Some(format!("+lock · {name}")),
                    None => return Err(RefusalReason::Unavailable),
                }
            }
        }
        if drop_block {
            self.meter.anchor = None;
        }
        Ok(())
    }

    /// Whether the meter has the field.
    pub(in crate::ui::stage) fn meter_open(&self) -> bool {
        self.meter.open
    }
}

/// What the painter reads. Every one of these is a plain read of state
/// the core already holds: the view has no memory of this section, so
/// there is nowhere for a second version of it to drift.
impl super::Stage {
    /// Where the cursor stands along the flat plan.
    pub(super) fn meter_at(&self) -> usize {
        self.meter.at
    }

    /// The block's other corner, if one is drawn.
    pub(super) fn meter_anchor(&self) -> Option<(isize, usize)> {
        self.meter.anchor
    }

    /// How far the held body stands from the playhead, if it is held.
    /// DERIVED from the two absolute positions rather than stored, so
    /// the number in the title cannot drift away from the rows.
    pub(super) fn meter_hold(&self) -> Option<isize> {
        let at = self.meter.held?;
        Some(at - (self.transport.tick() / PATTERN_STEP_TICKS) as isize)
    }

    /// The section's own second-hand, for the panel's ages.
    pub(super) fn meter_uptime(&self) -> f32 {
        self.meter.uptime
    }

    /// The stream, oldest first.
    pub(super) fn meter_lines(&self) -> impl DoubleEndedIterator<Item = &InputLine> {
        self.meter.lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::{Note, PATTERN_STEP_TICKS, PatternId, TrackKind};
    use crate::ui::stage::key::{Key, Mods};
    use crate::ui::stage::keymap::ScopeContext;
    use crate::ui::stage::transport::Motion;
    use crate::ui::stage::{ApplyOutcome, MeterIntent, Stage, StageIntent};

    fn open() -> Stage {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        assert_eq!(
            stage.handle_key(Mods::COMMAND.plus(Mods::SHIFT), Key::R),
            Some(ApplyOutcome::Changed),
            "the meter did not open"
        );
        stage
    }

    /// A track playing a clip of `steps` steps, with a trig on step 0.
    fn playing_track(stage: &mut Stage, steps: usize) -> PatternId {
        let at = stage.song.tracks.len();
        stage.song.add_track(TrackKind::Instrument);
        let pattern = stage
            .song
            .fill_slot(at, 0)
            .expect("the track took a clip in the first scene");
        let clip = stage
            .song
            .pattern_mut(pattern)
            .expect("the clip was just made");
        clip.length_ticks = steps * PATTERN_STEP_TICKS;
        let trig = clip.trig_mut(0);
        trig.enabled = true;
        trig.notes.push(Note::new(60, PATTERN_STEP_TICKS, 100));
        stage.playing.resize(stage.song.tracks.len(), None);
        stage.playing[at] = Some(0);
        pattern
    }

    #[test]
    fn the_meter_takes_the_field_and_escape_gives_it_back() {
        let mut stage = open();
        assert_eq!(stage.scope_context(), ScopeContext::Meter);
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Escape),
            Some(ApplyOutcome::Changed)
        );
        assert_ne!(
            stage.scope_context(),
            ScopeContext::Meter,
            "escape left the meter holding the keys"
        );
    }

    /// One room at a time. Two full-screen places sharing the field
    /// would leave the keyboard with two owners, and the reader with no
    /// way to tell which one answered.
    #[test]
    fn the_meter_will_not_open_over_another_room() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.lab.open = true;
        assert!(
            stage.apply_meter(MeterIntent::Open).is_err(),
            "the meter opened over the lab"
        );
        assert!(!stage.meter.open);
    }

    /// A held arrow is one fact repeated, not forty facts. A panel that
    /// scrolled its own history away every time a key was held would be
    /// unreadable exactly when the reader was busiest.
    #[test]
    fn a_held_key_is_one_line_that_counts() {
        let mut stage = open();
        for _ in 0..40 {
            let _ = stage.apply(StageIntent::Meter(MeterIntent::Forward));
        }
        let lines: Vec<_> = stage.meter_lines().collect();
        let last = lines.last().expect("the stream recorded the walk");
        assert_eq!(last.count, 40, "a held key was not collapsed");
        assert_eq!(last.word, MeterIntent::Forward.label());
        assert!(
            lines.len() < 40,
            "the panel kept a line per repeat: {} lines",
            lines.len()
        );
    }

    /// A refusal is the fact this section exists to report. Drawing one
    /// as though it had been answered would make the panel a liar
    /// exactly where it is most useful.
    #[test]
    fn a_refused_verb_is_recorded_as_the_refusal_it_was() {
        let mut stage = open();
        // Walking left off the first track is an edge, and edges refuse.
        while stage.apply(StageIntent::Meter(MeterIntent::Left)) == ApplyOutcome::Changed {}
        let refused = stage
            .meter_lines()
            .rev()
            .find(|line| line.word == MeterIntent::Left.label())
            .expect("the refused walk was recorded");
        assert!(refused.refused, "a refusal was recorded as an answer");
    }

    /// The stream belongs to the machine, not to the window. A section
    /// that only remembered what happened while it was open would open
    /// onto nothing every time, which is the one moment its reader
    /// wants the last minute back.
    #[test]
    fn the_stream_runs_while_the_section_is_shut() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        assert!(!stage.meter.open);
        let _ = stage.apply(StageIntent::ToggleTransport);
        assert!(
            stage
                .meter_lines()
                .any(|line| line.word == StageIntent::ToggleTransport.label()),
            "a verb answered with the meter shut was forgotten"
        );
    }

    /// The ring is a ring: it forgets the oldest rather than growing.
    #[test]
    fn the_stream_is_a_ring() {
        let mut stage = open();
        for _ in 0..(INPUTS * 2) {
            // Two verbs alternating, so nothing collapses into a count.
            let _ = stage.apply(StageIntent::Meter(MeterIntent::Forward));
            let _ = stage.apply(StageIntent::Meter(MeterIntent::Back));
        }
        assert_eq!(stage.meter_lines().count(), INPUTS);
    }

    /// Walking takes the wheel from the transport; Enter gives it back.
    /// The reader must always be able to tell whether what they are
    /// looking at is the present, and must always be one key from it.
    #[test]
    fn walking_takes_the_wheel_and_enter_gives_it_back() {
        let mut stage = open();
        assert_eq!(stage.meter_hold(), None, "the meter did not open riding");
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Back));
        assert_eq!(stage.meter_hold(), Some(-1));
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Follow));
        assert_eq!(stage.meter_hold(), None, "enter did not follow again");
        // And asking again changes nothing, so it refuses rather than
        // reporting a change it did not make.
        assert!(
            matches!(
                stage.apply(StageIntent::Meter(MeterIntent::Follow)),
                ApplyOutcome::Refused(_)
            ),
            "following while already following claimed to change something"
        );
    }

    /// The now-line stands on the transport's own step, and a held body
    /// stands where the reader put it.
    #[test]
    fn the_now_line_is_the_transports_step_until_it_is_held() {
        let mut stage = open();
        stage.transport.seek(8 * PATTERN_STEP_TICKS);
        let (now, _) = stage.meter_now();
        assert_eq!(now, 8);
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Back));
        let (held, _) = stage.meter_now();
        assert_eq!(held, 7, "the held body did not stand where it was put");
    }

    /// A lock is named by the machine its track actually locks, and a
    /// track with no device placed still locks the head of its chain.
    /// The column head has always said so through `voice_of`; the rows
    /// must say the same, or the same lock reads with a name on one
    /// track and without one on the next.
    #[test]
    fn a_lock_is_named_even_on_a_track_with_no_device_placed() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        let at = stage.song.tracks.len();
        let _ = stage.apply(StageIntent::NewInstrumentTrack);
        assert!(
            stage.song.tracks[at].machine.is_none(),
            "this test is only about a track with nothing placed on it"
        );
        let pattern = stage.song.fill_slot(at, 0).expect("a clip");
        let clip = stage.song.pattern_mut(pattern).expect("the clip");
        let trig = clip.trig_mut(0);
        trig.enabled = true;
        trig.notes.push(Note::new(60, PATTERN_STEP_TICKS, 100));
        trig.locks.push(crate::sequencing::ParamLock {
            param: crate::params::poly::F_CUTOFF,
            value: 1200.0,
            device: None,
            slide: false,
        });
        stage.playing.resize(stage.song.tracks.len(), None);
        stage.playing[at] = Some(0);

        let rows = stage.meter_rows(0, 1);
        let cell = rows[0].cells[at].as_ref().expect("the launched cell");
        let lock = cell.locks.first().expect("the lock");
        assert!(
            !lock.name.is_empty(),
            "a lock on a track with no device placed had no name"
        );
    }

    // ------------------------------------------------------ editing

    /// Where a track's `field` stands in the flat plan.
    ///
    /// Addressed by NAME rather than by number, so a test says what it
    /// is aiming at and adding a column between two others does not
    /// quietly re-aim every test in the file at its neighbour.
    fn field_at(stage: &Stage, track: usize, field: Field) -> usize {
        let plan = stage.meter_plan();
        let start = plan.start_of(track);
        let offset = plan.columns[track]
            .fields
            .iter()
            .position(|owned| *owned == field)
            .unwrap_or_else(|| panic!("track {track} has no {field:?} column"));
        start + offset
    }

    /// Put the cursor on one field of `track`, holding the body still
    /// on `row` so an edit has a fixed address.
    fn aim(stage: &mut Stage, track: usize, row: isize, field: Field) {
        stage.transport.stop();
        stage
            .transport
            .seek(row.max(0) as usize * PATTERN_STEP_TICKS);
        stage.meter.held = Some(row);
        stage.meter.at = field_at(stage, track, field);
        stage.meter.anchor = None;
    }

    fn trig_at(stage: &Stage, track: usize, step: usize) -> crate::sequencing::Trig {
        let (id, _) = stage.meter_pattern(track).expect("the track is playing");
        stage
            .song
            .pattern(id)
            .expect("the pattern")
            .trig(step)
            .clone()
    }

    /// The step's first note, for a before-and-after.
    fn note_at(stage: &Stage, track: usize, step: usize) -> crate::sequencing::Note {
        trig_at(stage, track, step)
            .notes
            .first()
            .cloned()
            .expect("the step holds a note")
    }

    /// The columns are the PATTERN's own locks, in the voice's order,
    /// with note and velocity in front and the door to a new lock at
    /// the end. A machine's whole table would be fifty columns of which
    /// a clip uses three.
    #[test]
    fn the_columns_are_the_locks_the_clip_actually_holds() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        let trig = stage.song.pattern_mut(clip).expect("clip").trig_mut(0);
        trig.locks.push(crate::sequencing::ParamLock {
            param: crate::params::poly::F_RES,
            value: 0.4,
            device: None,
            slide: false,
        });
        trig.locks.push(crate::sequencing::ParamLock {
            param: crate::params::poly::F_CUTOFF,
            value: 900.0,
            device: None,
            slide: false,
        });

        let plan = stage.meter_plan();
        let fields = &plan.columns[track].fields;
        // The fixed prefix, always and in this order, so a column never
        // moves under the reader when another track lays a lock.
        assert_eq!(
            &fields[..8],
            &[
                Field::Note,
                Field::Vel,
                Field::Len,
                Field::Cond,
                Field::Prob,
                Field::Rtg,
                Field::Micro,
                Field::Snd,
            ]
        );
        // Then the locks, and CUTOFF stands before RES in the voice's
        // own table — so it does here, whatever order they were laid in.
        assert_eq!(
            fields[8],
            Field::Lock {
                device: None,
                param: crate::params::poly::F_CUTOFF
            }
        );
        assert_eq!(
            fields[9],
            Field::Lock {
                device: None,
                param: crate::params::poly::F_RES
            }
        );
        assert_eq!(fields[10], Field::Add);
        assert_eq!(fields.len(), 11, "a column appeared for a lock nobody laid");
    }

    /// A typed letter writes a trig where there was none, in the octave
    /// the column is already speaking in — typing under a bass line
    /// gives a bass note, not whatever octave a constant chose.
    #[test]
    fn a_typed_letter_writes_a_note_in_the_columns_own_octave() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        // The clip's one note is C3 (midi 60); step 3 is empty.
        aim(&mut stage, track, 3, Field::Note);
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Letter(4))),
            ApplyOutcome::Changed,
            "typing E did not write a note"
        );
        let trig = trig_at(&stage, track, 3);
        assert!(trig.enabled, "the typed step did not become a trig");
        let note = trig.notes.first().expect("a note was written");
        let hz = note.pitch.resolve(&stage.song.key);
        assert_eq!(
            crate::pitch::nearest_midi(hz),
            64,
            "E landed outside the octave the column was speaking in"
        );
    }

    /// A pitch typed OVER a note changes the pitch and nothing else.
    /// A step carries a velocity and a length someone chose, and a key
    /// that quietly reset them would lose work that was never named.
    #[test]
    fn a_typed_pitch_keeps_the_velocity_and_length_it_lands_on() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let before = note_at(&stage, track, 0);
        aim(&mut stage, track, 0, Field::Note);
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Letter(3)));
        let after = note_at(&stage, track, 0);
        assert_eq!(after.velocity, before.velocity, "the velocity was reset");
        assert_eq!(
            after.length_ticks, before.length_ticks,
            "the length was reset"
        );
        let hz = after.pitch.resolve(&stage.song.key);
        assert_eq!(crate::pitch::nearest_midi(hz), 62, "D did not land");
    }

    /// Hex shifts in, the way a tracker shifts it: two keys set a byte,
    /// and there is no buffer to commit or abandon.
    #[test]
    fn typed_hex_shifts_into_the_velocity() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        aim(&mut stage, track, 0, Field::Vel);
        // 7 then F is 0x7F.
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Digit(7)));
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Letter(5)));
        assert_eq!(trig_at(&stage, track, 0).notes[0].velocity, 0x7f);
    }

    /// Turning a lock column on a step that holds no lock STARTS one,
    /// from the knob the trig would otherwise have played — which is
    /// the same rule the deck's own held-step turn follows.
    #[test]
    fn turning_an_empty_lock_column_starts_the_lock_at_the_knob() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        stage
            .song
            .pattern_mut(clip)
            .expect("clip")
            .trig_mut(0)
            .locks
            .push(crate::sequencing::ParamLock {
                param: crate::params::poly::F_CUTOFF,
                value: 900.0,
                device: None,
                slide: false,
            });
        // Step 4 has no trig and no lock; the CUTOFF column exists
        // because step 0 laid one.
        aim(
            &mut stage,
            track,
            4,
            Field::Lock {
                device: None,
                param: crate::params::poly::F_CUTOFF,
            },
        );
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Turn {
                up: true,
                coarse: false
            })),
            ApplyOutcome::Changed
        );
        let lock = trig_at(&stage, track, 4)
            .lock(crate::params::poly::F_CUTOFF)
            .expect("the turn started a lock");
        assert!(lock.is_finite());
    }

    /// The ADD column is how a parameter nothing locks yet gets its
    /// first lock — and therefore its column. Before the turn there is
    /// no column for it to be edited in; after it, there is.
    #[test]
    fn the_add_column_gives_a_parameter_its_first_column() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let (param, _) = stage.meter_add_param(track).expect("something to add");
        aim(&mut stage, track, 0, Field::Add);
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Turn {
                up: true,
                coarse: false
            })),
            ApplyOutcome::Changed
        );
        assert!(
            trig_at(&stage, track, 0).lock(param).is_some(),
            "the add column laid no lock"
        );
        assert!(
            stage.meter_plan().columns[track]
                .fields
                .contains(&Field::Lock {
                    device: None,
                    param
                }),
            "the lock was laid but no column appeared for it"
        );
    }

    /// A BLOCK edit lands on every cell it covers, across tracks and
    /// therefore across patterns — and lands as ONE step of undo,
    /// because the reader made one edit.
    #[test]
    fn a_block_turn_lands_on_every_cell_and_undoes_once() {
        let mut stage = open();
        let first = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let second = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let before = (note_at(&stage, first, 0), note_at(&stage, second, 0));

        aim(&mut stage, first, 0, Field::Vel);
        // Across, cell by cell, until the block reaches the OTHER
        // track's velocity: the point of the test is that a block
        // crossing tracks crosses patterns.
        let target = field_at(&stage, second, Field::Vel);
        while stage.meter_at() < target {
            assert_eq!(
                stage.apply(StageIntent::Meter(MeterIntent::SelectRight)),
                ApplyOutcome::Changed
            );
        }
        assert!(stage.meter_anchor().is_some(), "no block was drawn");
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Turn {
                up: true,
                coarse: false
            })),
            ApplyOutcome::Changed
        );
        let after = (note_at(&stage, first, 0), note_at(&stage, second, 0));
        assert!(
            after.0.velocity > before.0.velocity && after.1.velocity > before.1.velocity,
            "the block turn did not reach both tracks: {before:?} -> {after:?}"
        );

        assert_eq!(stage.apply(StageIntent::Undo), ApplyOutcome::Changed);
        let undone = (note_at(&stage, first, 0), note_at(&stage, second, 0));
        assert_eq!(
            (undone.0.velocity, undone.1.velocity),
            (before.0.velocity, before.1.velocity),
            "one undo did not step back over the whole block"
        );
    }

    /// Clear means the thing the cursor is ON. On a lock column it
    /// releases that lock and leaves the note; on the note column it
    /// clears the step. A single meaning for the key would make one of
    /// those two unreachable.
    #[test]
    fn clear_takes_the_lock_under_the_cursor_and_not_the_note() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        stage
            .song
            .pattern_mut(clip)
            .expect("clip")
            .trig_mut(0)
            .locks
            .push(crate::sequencing::ParamLock {
                param: crate::params::poly::F_CUTOFF,
                value: 900.0,
                device: None,
                slide: false,
            });
        aim(
            &mut stage,
            track,
            0,
            Field::Lock {
                device: None,
                param: crate::params::poly::F_CUTOFF,
            },
        );
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Clear));
        let trig = trig_at(&stage, track, 0);
        assert!(trig.locks.is_empty(), "the lock was not released");
        assert!(!trig.notes.is_empty(), "clearing a lock took the note too");

        aim(&mut stage, track, 0, Field::Note);
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Clear));
        assert!(
            trig_at(&stage, track, 0).notes.is_empty(),
            "clearing the note column left the note"
        );
    }

    /// An edit lands on the STEP, which is to say on every repetition
    /// of it the clip will ever play. That is what editing a loop
    /// means, and the rows show it: the same trig at every fold.
    #[test]
    fn an_edit_lands_on_the_step_and_so_on_every_fold_of_it() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 4);
        aim(&mut stage, track, 9, Field::Note);
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Letter(2)));
        // Row 9 of a four-step clip is step 1, and every row that folds
        // onto step 1 now reads the same.
        for row in [1isize, 5, 9, 13] {
            let rows = stage.meter_rows(row, 1);
            let cell = rows[0].cells[track]
                .as_ref()
                .expect("the launched cell")
                .note
                .as_ref()
                .expect("the note the edit wrote");
            assert_eq!(cell.0, "C-4", "the fold at row {row} disagreed");
        }
    }

    /// THE CLAIM OF THIS SECTION: nothing the surface shows is read
    /// only. Every column answers to a key and moves the document.
    ///
    /// Written as a SWEEP over the field kinds rather than as one test
    /// each, because the claim is about the set: a column added later
    /// and left inert fails here, which a test per column would not
    /// notice.
    #[test]
    fn every_column_answers_to_a_key() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        {
            let trig = stage.song.pattern_mut(clip).expect("clip").trig_mut(0);
            trig.locks.push(crate::sequencing::ParamLock {
                param: crate::params::poly::F_CUTOFF,
                value: 900.0,
                device: None,
                slide: false,
            });
            trig.sound = Some(crate::sequencing::SoundLock {
                name: "gong".to_owned(),
                sound: crate::sound::Sound::default(),
            });
        }
        let lock = Field::Lock {
            device: None,
            param: crate::params::poly::F_CUTOFF,
        };
        let up = StageIntent::Meter(MeterIntent::Turn {
            up: true,
            coarse: false,
        });
        // Each column, and the key that is its own way in.
        let cases: [(Field, StageIntent); 10] = [
            (Field::Note, StageIntent::Meter(MeterIntent::Letter(3))),
            (Field::Vel, StageIntent::Meter(MeterIntent::Digit(4))),
            (Field::Len, up),
            (Field::Cond, up),
            // Down, not up: a step starts CERTAIN, and the ladder has
            // nowhere likelier to go from there.
            (
                Field::Prob,
                StageIntent::Meter(MeterIntent::Turn {
                    up: false,
                    coarse: false,
                }),
            ),
            (Field::Rtg, up),
            (Field::Micro, up),
            (Field::Snd, StageIntent::Meter(MeterIntent::Clear)),
            (lock, up),
            (Field::Add, up),
        ];
        for (field, key) in cases {
            let before = trig_at(&stage, track, 0);
            aim(&mut stage, track, 0, field);
            assert_eq!(
                stage.apply(key),
                ApplyOutcome::Changed,
                "the {field:?} column refused the key that is its own"
            );
            assert_ne!(
                trig_at(&stage, track, 0),
                before,
                "the {field:?} column took a key and changed nothing"
            );
        }
    }

    /// A condition is a LADDER, not two numbers: 3:2 is not a
    /// condition, so the column walks the ones that exist and refuses
    /// at the ends rather than inventing one.
    #[test]
    fn the_condition_column_walks_a_ladder_and_stops_at_its_ends() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        aim(&mut stage, track, 0, Field::Cond);
        let up = StageIntent::Meter(MeterIntent::Turn {
            up: true,
            coarse: false,
        });
        let down = StageIntent::Meter(MeterIntent::Turn {
            up: false,
            coarse: false,
        });
        assert_eq!(stage.apply(up), ApplyOutcome::Changed);
        assert_eq!(trig_at(&stage, track, 0).cond, Some((1, 2)));
        assert_eq!(stage.apply(up), ApplyOutcome::Changed);
        assert_eq!(trig_at(&stage, track, 0).cond, Some((2, 2)));
        // Back down past the bottom of the ladder is no condition, and
        // one more is an edge.
        assert_eq!(stage.apply(down), ApplyOutcome::Changed);
        assert_eq!(stage.apply(down), ApplyOutcome::Changed);
        assert_eq!(trig_at(&stage, track, 0).cond, None);
        assert!(
            matches!(stage.apply(down), ApplyOutcome::Refused(_)),
            "the ladder ran off its own end"
        );
    }

    /// Two numbers on one column, and the modifier chooses which: a
    /// fine turn walks how many repeats, a coarse one how fast.
    #[test]
    fn a_retrig_walks_its_count_finely_and_its_rate_coarsely() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        aim(&mut stage, track, 0, Field::Rtg);
        let fine = StageIntent::Meter(MeterIntent::Turn {
            up: true,
            coarse: false,
        });
        let coarse = StageIntent::Meter(MeterIntent::Turn {
            up: true,
            coarse: true,
        });
        let _ = stage.apply(fine);
        let _ = stage.apply(fine);
        let retrig = trig_at(&stage, track, 0).retrig.expect("a retrig");
        assert_eq!(retrig.count, 2, "the fine turn did not walk the count");
        let rate = retrig.rate;
        let _ = stage.apply(coarse);
        let after = trig_at(&stage, track, 0).retrig.expect("still a retrig");
        assert!(after.rate > rate, "the coarse turn did not walk the rate");
        assert_eq!(after.count, 2, "the coarse turn moved the count too");

        // Down to nothing is NO retrig, not a retrig of nothing.
        let down = StageIntent::Meter(MeterIntent::Turn {
            up: false,
            coarse: false,
        });
        let _ = stage.apply(down);
        let _ = stage.apply(down);
        assert_eq!(trig_at(&stage, track, 0).retrig, None);
    }

    /// A chord is one step with more notes in it, so a shifted letter
    /// ADDS rather than replacing what is there.
    #[test]
    fn a_shifted_letter_builds_a_chord_rather_than_replacing_the_note() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        aim(&mut stage, track, 0, Field::Note);
        let before = note_at(&stage, track, 0);
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Chord(4))),
            ApplyOutcome::Changed
        );
        let trig = trig_at(&stage, track, 0);
        assert_eq!(trig.notes.len(), 2, "the shifted letter did not add");
        assert_eq!(
            trig.notes[0].pitch, before.pitch,
            "the note already there was replaced"
        );
    }

    /// The slide key marks the lock the cursor is IN, and marks it
    /// back. A slide is a property of one address, so it needs an
    /// address to be spoken about — which the column is.
    #[test]
    fn the_slide_key_marks_the_lock_under_the_cursor() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        stage
            .song
            .pattern_mut(clip)
            .expect("clip")
            .trig_mut(0)
            .locks
            .push(crate::sequencing::ParamLock {
                param: crate::params::poly::F_CUTOFF,
                value: 900.0,
                device: None,
                slide: false,
            });
        aim(
            &mut stage,
            track,
            0,
            Field::Lock {
                device: None,
                param: crate::params::poly::F_CUTOFF,
            },
        );
        let slide = StageIntent::Meter(MeterIntent::Slide);
        assert_eq!(stage.apply(slide), ApplyOutcome::Changed);
        assert!(trig_at(&stage, track, 0).locks[0].slide, "no slide marked");
        assert_eq!(stage.apply(slide), ApplyOutcome::Changed);
        assert!(
            !trig_at(&stage, track, 0).locks[0].slide,
            "the slide would not come off again"
        );
    }

    /// Clearing the sound column releases the sound and leaves the
    /// note. Laying one is the library's job — a sound is not a value
    /// a column can hold — and the section says so rather than
    /// pretending the column is half broken.
    #[test]
    fn clearing_the_sound_column_releases_the_sound_and_nothing_else() {
        let mut stage = open();
        let track = stage.song.tracks.len();
        playing_track(&mut stage, 8);
        let clip = stage.meter_pattern(track).expect("playing").0;
        stage
            .song
            .pattern_mut(clip)
            .expect("clip")
            .trig_mut(0)
            .sound = Some(crate::sequencing::SoundLock {
            name: "gong".to_owned(),
            sound: crate::sound::Sound::default(),
        });
        aim(&mut stage, track, 0, Field::Snd);
        assert_eq!(
            stage.apply(StageIntent::Meter(MeterIntent::Clear)),
            ApplyOutcome::Changed
        );
        let trig = trig_at(&stage, track, 0);
        assert!(trig.sound.is_none(), "the sound was not released");
        assert!(!trig.notes.is_empty(), "releasing the sound took the note");
    }

    /// HELD MEANS STOPPED. The hold is stored as the absolute step the
    /// now-line stands on, not as a distance from the playhead — so a
    /// transport that keeps rolling under a held body leaves it exactly
    /// where the reader put it. Stored as a distance, the body would go
    /// on scrolling under a title that said HELD.
    #[test]
    fn a_held_body_stands_still_while_the_song_runs_on() {
        let mut stage = open();
        stage.transport.set_motion(Motion::Rolling);
        stage.transport.seek(20 * PATTERN_STEP_TICKS);
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Back));
        let (held, glide) = stage.meter_now();
        assert_eq!(held, 19);
        assert_eq!(glide, 0.0, "a held body was still gliding");
        assert_eq!(stage.meter_hold(), Some(-1));

        // Four bars later, the reader has not moved and neither has
        // what they are reading.
        stage.transport.seek(84 * PATTERN_STEP_TICKS);
        assert_eq!(
            stage.meter_now().0,
            19,
            "the held body drifted with the transport"
        );
        assert_eq!(
            stage.meter_hold(),
            Some(-65),
            "the title did not follow the song away from the held step"
        );
        let _ = stage.apply(StageIntent::Meter(MeterIntent::Follow));
        assert_eq!(stage.meter_now().0, 84, "following did not catch up");
    }

    /// THE CLAIM THE ROWS EXIST TO MAKE. The rows are SONG steps, not
    /// any one pattern's, so two clips of different lengths keep the
    /// time they are actually played in: three against four comes back
    /// around at twelve, and the only place that wheel is visible is
    /// here.
    #[test]
    fn clips_of_different_lengths_meet_in_song_time() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        let three = stage.song.tracks.len();
        playing_track(&mut stage, 3);
        let four = stage.song.tracks.len();
        playing_track(&mut stage, 4);

        let rows = stage.meter_rows(0, 13);
        let fires =
            |row: &Row, track: usize| row.cells[track].as_ref().is_some_and(|cell| cell.sounding);
        for step in 0..13usize {
            assert_eq!(
                fires(&rows[step], three),
                step % 3 == 0,
                "the three-step clip did not fold at step {step}"
            );
            assert_eq!(
                fires(&rows[step], four),
                step % 4 == 0,
                "the four-step clip did not fold at step {step}"
            );
        }
        assert!(
            fires(&rows[12], three) && fires(&rows[12], four),
            "three against four did not come back around at twelve"
        );
    }

    /// A track playing nothing is not a track playing rests. One is
    /// silence someone wrote; the other is a channel that was never
    /// launched, and drawing them alike would invent a pattern.
    #[test]
    fn a_track_playing_nothing_is_not_an_empty_step() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        let launched = stage.song.tracks.len();
        playing_track(&mut stage, 4);
        let silent = stage.song.tracks.len();
        stage.song.add_track(TrackKind::Instrument);
        stage.playing.resize(stage.song.tracks.len(), None);

        let rows = stage.meter_rows(1, 1);
        assert!(
            rows[0].cells[launched].is_some(),
            "a launched track's empty step read as nothing at all"
        );
        assert!(
            !rows[0].cells[launched]
                .as_ref()
                .expect("the launched cell")
                .marked(),
            "an empty step read as a written one"
        );
        assert!(
            rows[0].cells[silent].is_none(),
            "an unlaunched track was given rests it never wrote"
        );
    }

    /// A rolling transport glides between rows rather than jumping: a
    /// step is 125ms at 120bpm, and a body that moved once a step would
    /// stutter eight times a second.
    #[test]
    fn the_body_glides_while_it_rolls_and_stands_still_when_parked() {
        let mut stage = open();
        stage.transport.stop();
        assert_eq!(stage.meter_now().1, 0.0, "a parked body was gliding");
        stage.transport.set_motion(Motion::Rolling);
        stage.transport.seek(PATTERN_STEP_TICKS / 2);
        let (step, glide) = stage.meter_now();
        assert_eq!(step, 0, "half a step in is still the first step");
        assert!(
            (glide - 0.5).abs() < 0.01,
            "half a step in did not read as half a row: {glide}"
        );
    }
}

impl super::Stage {
    /// The section, posed: five tracks of real material under a rolling
    /// transport, with a stream of answered verbs already behind it.
    ///
    /// A pose of a section whose whole subject is MOTION has to be
    /// staged rather than defaulted — an empty song under a parked
    /// transport would draw the one picture this section never shows.
    /// The clip lengths deliberately disagree (sixteen against twelve
    /// against six), because the wheel they turn is the thing the rows
    /// exist to make visible.
    pub fn pose_meter(&mut self, pose: &str) {
        use crate::params::poly as pp;
        use crate::sequencing::{Note, PATTERN_STEP_TICKS, ParamLock};

        // The pose reads its own name, the way the midi lab's does. A
        // section whose subject is motion cannot be shown by one
        // picture, so the frame number is part of the pose: `-f12` is
        // twelve sixths of a step past the base moment, which is what
        // lets a strip of renders be a strip of one continuous thing.
        // `-held3` walks the body back three steps by the real intent,
        // and `-trk2` moves the focus, so the drive is the app's own
        // path and not a picture of it.
        let mut frame = 0usize;
        let mut held = 0usize;
        let mut focus = 0usize;
        let mut follow = false;
        for word in pose.split('-') {
            if word == "follow" {
                follow = true;
            }
            if let Some(n) = word.strip_prefix("held").and_then(|n| n.parse().ok()) {
                held = n;
            } else if let Some(n) = word.strip_prefix("trk").and_then(|n| n.parse().ok()) {
                focus = n;
            } else if let Some(n) = word.strip_prefix('f').and_then(|n| n.parse().ok()) {
                frame = n;
            }
        }
        /// A frame is a sixth of a step, which divides the twelve-tick
        /// step exactly — so a strip of frames lands on real transport
        /// positions rather than on rounded ones.
        const FRAME_TICKS: usize = PATTERN_STEP_TICKS / 6;

        // Five voices, each with a clip in the first scene.
        let staged: [(usize, &[usize]); 5] = [
            (16, &[0, 4, 8, 10, 12]),
            (12, &[0, 3, 6, 9]),
            (16, &[2, 6, 11, 14]),
            (6, &[0, 2, 4]),
            (16, &[0, 8]),
        ];
        for (voice, (steps, hits)) in staged.iter().enumerate() {
            // The song opens with one track already on it, so the first
            // voice moves into that one rather than leaving it blank
            // beside five that are playing.
            let at = if voice == 0 {
                0
            } else {
                let at = self.song.tracks.len();
                let _ = self.apply(super::StageIntent::NewInstrumentTrack);
                if self.song.tracks.len() <= at {
                    break;
                }
                at
            };
            let Some(pattern) = self.song.fill_slot(at, 0) else {
                continue;
            };
            let Some(clip) = self.song.pattern_mut(pattern) else {
                continue;
            };
            clip.length_ticks = steps * PATTERN_STEP_TICKS;
            for (nth, &step) in hits.iter().enumerate() {
                let trig = clip.trig_mut(step);
                trig.enabled = true;
                let pitch = 36 + (voice as u8 * 7) + (nth as u8 % 3) * 5;
                trig.notes.push(Note::new(
                    pitch,
                    PATTERN_STEP_TICKS,
                    90 + (nth as u8 * 11) % 37,
                ));
                // A chord on one step, so the count beside a note has
                // something to count.
                if nth == 1 && voice % 2 == 0 {
                    trig.notes
                        .push(Note::new(pitch + 7, PATTERN_STEP_TICKS, 80));
                }
                // Locks, and one pair of them sliding into each other,
                // so the marks and the readout both have real material.
                if nth % 2 == 0 {
                    trig.locks.push(ParamLock {
                        param: pp::F_CUTOFF,
                        value: 400.0 + nth as f32 * 900.0,
                        device: None,
                        slide: nth == 0 && voice == 1,
                    });
                }
                if nth % 3 == 1 {
                    trig.locks.push(ParamLock {
                        param: pp::F_RES,
                        value: 0.2 + nth as f32 * 0.15,
                        device: None,
                        slide: false,
                    });
                }
                if nth == 2 {
                    trig.locks.push(ParamLock {
                        param: pp::AMP_D,
                        value: 120.0,
                        device: None,
                        slide: false,
                    });
                }
                // The rules a step can play by, one of each across the
                // song rather than all of them on one trig.
                match (voice, nth) {
                    (0, 3) => trig.cond = Some((1, 2)),
                    (2, 1) => {
                        trig.retrig = Some(crate::sequencing::Retrig {
                            rate: 4,
                            count: 3,
                            decay: -8,
                        })
                    }
                    (3, 2) => trig.probability = 0.6,
                    (1, 2) => {
                        if let Some(note) = trig.notes.first_mut() {
                            note.micro_ticks = 3;
                        }
                    }
                    _ => {}
                }
            }
            self.playing.resize(self.song.tracks.len(), None);
            self.playing[at] = Some(0);
        }

        // A stream with something in it. These are answered for real,
        // through the same funnel every other verb goes through, so the
        // panel in the picture is showing what it would show.
        use super::Step::{Down, Left, Right, Up};
        for intent in [
            super::StageIntent::Step(Down),
            super::StageIntent::Step(Right),
            super::StageIntent::Step(Right),
            super::StageIntent::Deck,
            super::StageIntent::Turn {
                up: true,
                coarse: false,
            },
            super::StageIntent::Turn {
                up: true,
                coarse: false,
            },
            super::StageIntent::Turn {
                up: false,
                coarse: true,
            },
            super::StageIntent::Escape,
            super::StageIntent::Step(Left),
            super::StageIntent::Step(Down),
            super::StageIntent::Step(Down),
            super::StageIntent::Mix,
            super::StageIntent::Escape,
            super::StageIntent::SongView,
            super::StageIntent::SongView,
            super::StageIntent::Step(Up),
            super::StageIntent::Step(Up),
            super::StageIntent::Ground,
            super::StageIntent::Ground,
            super::StageIntent::Step(Right),
        ] {
            let _ = self.apply(intent);
            self.meter.tick(0.4);
        }

        // Four four, so the grid the eye counts by is the one it
        // expects; the default song's signature is not this pose's
        // subject and an odd bar would read as a bug in the rows.
        self.song.set_meter_mark(0, 4, 4);
        self.transport.set_motion(super::transport::Motion::Rolling);
        // A moment where the focused track is actually holding
        // something, so the readout band shows what it is for rather
        // than the true but useless fact that this step is empty.
        // The base moment first, so a `-held` walk anchors where the
        // reader would have been standing when they took the wheel;
        // only then does the frame carry the transport forward. Held
        // the other way round, every frame of a held strip would anchor
        // somewhere new and the body would appear to follow after all.
        let base = 20 * PATTERN_STEP_TICKS + PATTERN_STEP_TICKS / 3;
        self.transport.seek(base);
        let _ = self.apply_meter(super::MeterIntent::Open);
        for _ in 0..held {
            let _ = self.apply(super::StageIntent::Meter(super::MeterIntent::Back));
        }
        for _ in 0..focus {
            let _ = self.apply(super::StageIntent::Meter(super::MeterIntent::Right));
        }
        if follow {
            let _ = self.apply(super::StageIntent::Meter(super::MeterIntent::Follow));
        }
        // `-do<script>` presses keys, one character each, through the
        // ordinary intent path — so a posed edit is the edit the keys
        // make and a strip of frames is a real session rather than a
        // series of states someone assembled to look like one.
        // Uppercase is a verb, lowercase a note letter, a digit a digit.
        if let Some(script) = pose.split('-').find_map(|word| word.strip_prefix("do")) {
            use super::MeterIntent as I;
            for key in script.chars() {
                let intent = match key {
                    'N' => I::Right,
                    'P' => I::Left,
                    'S' => I::SelectRight,
                    'W' => I::SelectForward,
                    'U' => I::Turn {
                        up: true,
                        coarse: false,
                    },
                    'D' => I::Turn {
                        up: false,
                        coarse: false,
                    },
                    'C' => I::Turn {
                        up: true,
                        coarse: true,
                    },
                    'T' => I::Trig,
                    'X' => I::Clear,
                    'L' => I::AddNext,
                    'a'..='g' => I::Letter(key as u8 - b'a'),
                    '0'..='9' => I::Digit(key as u8 - b'0'),
                    _ => continue,
                };
                let _ = self.apply(super::StageIntent::Meter(intent));
                self.meter.tick(0.3);
            }
        }
        self.transport.seek(base + frame * FRAME_TICKS);
    }
}

/// One edit, before it knows which cells it lands on.
///
/// The block decides WHERE, this decides WHAT, and the two meet in
/// `meter_edit` — which is why a selection needed no second set of
/// verbs. Every one of these becomes an ordinary `sequence::Intent`, so
/// an edit made here is the same edit the grid and the deck make, and
/// undo steps back through it the same way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Edit {
    Turn {
        up: bool,
        coarse: bool,
    },
    /// A typed pitch class, 0..12, keeping the octave that was there.
    Pitch(u8),
    /// A typed hex nibble, shifted into the velocity the way a tracker
    /// shifts one: two keys set a byte and there is no buffer to commit
    /// or abandon.
    Nibble(u8),
    /// A typed pitch class ADDED to the step rather than replacing it:
    /// a chord is one step, so building one is adding to it.
    Chord(u8),
    /// The lock under the cursor glides to the next lock on the same
    /// address, or goes back to being a plain one.
    Slide,
    Trig,
    Clear,
}

/// The pitch class each of the seven typed letters names.
///
/// A..G rather than the chromatic row a piano keyboard would give,
/// because these are the NAMES of notes and the column shows names. A
/// sharp is the letter and then one turn up: one more key than a
/// dedicated row would need, and one fewer thing to learn.
const LETTER_CLASS: [u8; 7] = [9, 11, 0, 2, 4, 5, 7];

/// The conditions a turn walks, in the order a player reaches for
/// them: off, then the halves, the thirds and the quarters. A ladder
/// rather than two numbers on one column, because A and B are not
/// independent — 3:2 is not a condition, and a column that let you
/// type it would have to refuse it afterwards.
const CONDITIONS: [Option<(u8, u8)>; 10] = [
    None,
    Some((1, 2)),
    Some((2, 2)),
    Some((1, 3)),
    Some((2, 3)),
    Some((3, 3)),
    Some((1, 4)),
    Some((2, 4)),
    Some((3, 4)),
    Some((4, 4)),
];

/// The chances a turn walks. The same ladder the grid's own condition
/// key steps through, so one fact has one set of values.
const CHANCES: [f32; 5] = [1.0, 0.75, 0.5, 0.25, 0.1];

/// The rates a retrig can run at, as the compiler sanitizes them.
const RATES: [u8; 6] = [1, 2, 3, 4, 6, 8];

/// Walk a ladder by one, in either direction, without wrapping past
/// its ends — an edge is a refusal here as everywhere.
fn ladder_step<T: PartialEq + Copy>(ladder: &[T], at: T, up: bool) -> Option<T> {
    let index = ladder.iter().position(|entry| *entry == at)?;
    let next = if up {
        index.checked_add(1).filter(|next| *next < ladder.len())?
    } else {
        index.checked_sub(1)?
    };
    ladder.get(next).copied()
}

/// Where a typed pitch lands when the step it is typed on is empty.
const TYPED_OCTAVE: i32 = 3;
/// The velocity a trig gets when it is made rather than edited.
const NEW_VELOCITY: u8 = 100;

impl super::Stage {
    /// Every cell the next edit lands on: the cursor's, or every cell of
    /// the block if one is drawn.
    ///
    /// A rectangle in two numbers — row and position along the flat
    /// plan — which is the whole reason the cursor walks one line
    /// instead of a track and a field inside it.
    pub(super) fn meter_cells(&self) -> Vec<(isize, usize)> {
        let (row, _) = self.meter_now();
        let at = self.meter.at;
        let Some((from_row, from_at)) = self.meter.anchor else {
            return vec![(row, at)];
        };
        let rows = from_row.min(row)..=from_row.max(row);
        let ats = from_at.min(at)..=from_at.max(at);
        rows.flat_map(|r| ats.clone().map(move |a| (r, a)))
            .collect()
    }

    /// The block's shape, for the surface to draw and for a notice to
    /// say: rows by columns.
    pub(super) fn meter_block(&self) -> Option<(isize, usize)> {
        let (row, _) = self.meter_now();
        let (from_row, from_at) = self.meter.anchor?;
        Some((
            (from_row - row).abs() + 1,
            from_at.abs_diff(self.meter.at) + 1,
        ))
    }

    /// The pattern a row of a track writes to, and the step inside it.
    ///
    /// The rows are song steps and the patterns are not, so every edit
    /// passes through this fold — and an edit therefore lands on the
    /// STEP, which is to say on every repetition of it the clip will
    /// ever play. That is what editing a loop means, and the surface
    /// shows it: the same trig changes at every fold on screen.
    fn meter_address(
        &self,
        row: isize,
        track: usize,
    ) -> Option<(crate::sequencing::PatternId, usize)> {
        let (id, origin) = self.meter_pattern(track)?;
        let steps = self.meter_steps(id);
        let origin = (origin / PATTERN_STEP_TICKS) as isize;
        if row < origin {
            return None;
        }
        Some((id, (row - origin).rem_euclid(steps as isize) as usize))
    }

    /// The octave a typed pitch lands in on `track` at `step`: the one
    /// already standing on that step, else the last one written above
    /// it, else a middling default. Never a fixed number alone — typing
    /// a letter under a bass line should give a bass note.
    fn meter_octave(&self, pattern: &Pattern, step: usize) -> i32 {
        let steps = (pattern.length_ticks / PATTERN_STEP_TICKS).max(1);
        for back in 0..steps {
            let at = (step + steps - back % steps) % steps;
            if let Some(note) = pattern.trig(at).notes.first() {
                let hz = note.pitch.resolve(&self.song.key);
                return i32::from(crate::pitch::nearest_midi(hz)) / 12 - 1;
            }
        }
        TYPED_OCTAVE
    }

    /// What a lock column's turn starts from when the step holds no
    /// lock yet: the knob the trig would otherwise have played.
    fn meter_standing(
        &self,
        track: usize,
        device: Option<crate::sequencing::DeviceId>,
        param: u32,
    ) -> Option<(
        f32,
        crate::params::ParamDef,
        &'static crate::devices::ParamLabel,
    )> {
        let held = self.song.tracks.get(track)?;
        let (spec, voice) = super::trig_menu::voice_of(held);
        let (spec, machine) = match device {
            None => (spec, voice),
            Some(id) => {
                let device = self.song.device(id)?;
                (device.kind.spec(), Some(device))
            }
        };
        let at = spec.params.iter().position(|def| def.id == param)?;
        let def = *spec.params.get(at)?;
        let label = spec.labels.get(at)?;
        let knob = machine.map_or(def.default, |device| device.value(param));
        Some((knob, def, label))
    }

    /// Apply `edit` to every cell of the block.
    ///
    /// Grouped by PATTERN before it lands, because a block can cross
    /// tracks and `apply_sequence` speaks to one pattern at a time —
    /// and because one call per pattern rather than one per cell keeps
    /// the whole block inside a single settle, so undo steps back over
    /// the edit the reader made rather than over its last cell.
    pub(super) fn meter_edit(&mut self, edit: Edit) -> Result<(), super::RefusalReason> {
        use super::RefusalReason;
        use crate::intent::sequence::Intent;
        if !self.meter.open {
            return Err(RefusalReason::Unavailable);
        }
        let plan = self.meter_plan();
        let mut work: Vec<(crate::sequencing::PatternId, Intent)> = Vec::new();
        // Sound locks are not values an intent can hold, which is why
        // there is no intent that lays one. Releasing one IS a plain
        // fact, and it is done the same way `apply_sequence` itself
        // resolves a sound: on the pattern, by hand, inside the same
        // settle as everything else in the block.
        let mut released: Vec<(crate::sequencing::PatternId, usize)> = Vec::new();
        for (row, at) in self.meter_cells() {
            let Some((track, field)) = plan.at(at) else {
                continue;
            };
            let Some((id, step)) = self.meter_address(row, track) else {
                continue;
            };
            let Some(pattern) = self.song.pattern(id) else {
                continue;
            };
            let tick = step * PATTERN_STEP_TICKS;
            let trig = pattern.trig(step);
            let written = trig.enabled || !trig.notes.is_empty() || !trig.locks.is_empty();
            match (field, edit) {
                // ---------------------------------------------- the note
                (Field::Note, Edit::Turn { up, coarse }) if written => {
                    work.push((
                        id,
                        Intent::Transpose {
                            tick,
                            delta_semitones: if coarse { 12 } else { 1 } * if up { 1 } else { -1 },
                        },
                    ));
                }
                (Field::Note, Edit::Pitch(class)) => {
                    let octave = self.meter_octave(pattern, step);
                    let midi = ((octave + 1) * 12 + i32::from(class)).clamp(0, 127) as u8;
                    let pitch = crate::pitch::Pitch::from_midi(midi);
                    work.push((
                        id,
                        match trig.notes.first() {
                            // Keep everything the step already said and
                            // change only what was typed: a pitch typed
                            // over a note must not silently reset its
                            // velocity or its length.
                            Some(note) => Intent::SetPrimary {
                                tick,
                                pitch,
                                length_ticks: note.length_ticks,
                                velocity: note.velocity,
                            },
                            None => Intent::Toggle {
                                tick,
                                default_pitch: pitch,
                                default_length_ticks: PATTERN_STEP_TICKS,
                                default_velocity: NEW_VELOCITY,
                            },
                        },
                    ));
                }
                // ------------------------------------------ the velocity
                (Field::Vel, Edit::Turn { up, coarse }) if written => {
                    work.push((
                        id,
                        Intent::AdjustVelocity {
                            tick,
                            delta: if coarse { 16 } else { 1 } * if up { 1 } else { -1 },
                        },
                    ));
                }
                (Field::Vel, Edit::Nibble(nibble)) => {
                    let Some(note) = trig.notes.first() else {
                        continue;
                    };
                    let was = i32::from(note.velocity);
                    let now = ((was << 4) | i32::from(nibble)) & 0x7f;
                    work.push((
                        id,
                        Intent::AdjustVelocity {
                            tick,
                            delta: (now - was) as isize,
                        },
                    ));
                }
                // ---------------------------------------------- the locks
                (Field::Lock { device, param }, Edit::Turn { up, coarse }) => {
                    let Some((knob, def, label)) = self.meter_standing(track, device, param) else {
                        continue;
                    };
                    let standing = trig.lock_on(device, param).unwrap_or(knob);
                    let amount =
                        super::chain::step_of(&def, label, coarse) * if up { 1.0 } else { -1.0 };
                    work.push((
                        id,
                        Intent::SetLock {
                            tick,
                            device: device.map(|id| id.0),
                            param,
                            value: def.clamp(standing + amount),
                        },
                    ));
                }
                // ------------------------------------------ the chord
                (Field::Note, Edit::Chord(class)) => {
                    let octave = self.meter_octave(pattern, step);
                    let midi = ((octave + 1) * 12 + i32::from(class)).clamp(0, 127) as u8;
                    work.push((
                        id,
                        Intent::AddNote {
                            tick,
                            pitch: crate::pitch::Pitch::from_midi(midi),
                            length_ticks: trig
                                .notes
                                .first()
                                .map_or(PATTERN_STEP_TICKS, |note| note.length_ticks),
                            velocity: trig
                                .notes
                                .first()
                                .map_or(NEW_VELOCITY, |note| note.velocity),
                            probability: trig.probability,
                        },
                    ));
                }
                // ----------------------------------------- how long it is
                (Field::Len, Edit::Turn { up, coarse }) if written => {
                    work.push((
                        id,
                        Intent::Resize {
                            tick,
                            delta_ticks: if coarse {
                                PATTERN_STEP_TICKS as isize
                            } else {
                                1
                            } * if up { 1 } else { -1 },
                        },
                    ));
                }
                // ------------------------------------- when it fires at all
                (Field::Cond, Edit::Turn { up, .. }) if written => {
                    let Some(cond) = ladder_step(&CONDITIONS, trig.cond, up) else {
                        continue;
                    };
                    work.push((id, Intent::SetCondition { tick, cond }));
                }
                (Field::Cond, Edit::Clear) => {
                    work.push((id, Intent::SetCondition { tick, cond: None }));
                }
                (Field::Prob, Edit::Turn { up, .. }) if written => {
                    // The ladder runs from certain to rare, so turning
                    // UP makes the step likelier: the direction the
                    // number on screen moves.
                    let at = CHANCES
                        .iter()
                        .copied()
                        .min_by(|a, b| {
                            (a - trig.probability)
                                .abs()
                                .total_cmp(&(b - trig.probability).abs())
                        })
                        .unwrap_or(1.0);
                    let Some(probability) = ladder_step(&CHANCES, at, !up) else {
                        continue;
                    };
                    work.push((id, Intent::SetProbability { tick, probability }));
                }
                (Field::Prob, Edit::Clear) => {
                    work.push((
                        id,
                        Intent::SetProbability {
                            tick,
                            probability: 1.0,
                        },
                    ));
                }
                // --------------------------------- how many times it fires
                //
                // Two numbers on one column, and the modifier chooses
                // which: a fine turn walks the count, a coarse one the
                // rate. They are not independent enough to deserve two
                // columns — a rate with no count is not a retrig.
                (Field::Rtg, Edit::Turn { up, coarse }) if written => {
                    let held = trig.retrig.unwrap_or_default();
                    let retrig = if coarse {
                        let Some(rate) = ladder_step(&RATES, held.rate, up) else {
                            continue;
                        };
                        Some(crate::sequencing::Retrig { rate, ..held })
                    } else {
                        let count = i32::from(trig.retrig.map_or(0, |retrig| retrig.count))
                            + if up { 1 } else { -1 };
                        if count < 0 || count > 8 {
                            continue;
                        }
                        // A count of nothing is not a retrig of nothing:
                        // it is no retrig, and the document says so.
                        (count > 0).then(|| crate::sequencing::Retrig {
                            count: count as u8,
                            ..held
                        })
                    };
                    work.push((id, Intent::SetRetrig { tick, retrig }));
                }
                (Field::Rtg, Edit::Clear) => {
                    work.push((id, Intent::SetRetrig { tick, retrig: None }));
                }
                // ------------------------------------------ when it fires
                (Field::Micro, Edit::Turn { up, coarse }) if written => {
                    work.push((
                        id,
                        Intent::Nudge {
                            tick,
                            delta_ticks: if coarse { 3 } else { 1 } * if up { 1 } else { -1 },
                        },
                    ));
                }
                // ------------------------------------------- the slide
                (Field::Lock { device, param }, Edit::Slide) => {
                    let Some(lock) = trig
                        .locks
                        .iter()
                        .find(|lock| lock.device == device && lock.param == param)
                    else {
                        continue;
                    };
                    work.push((
                        id,
                        Intent::SetSlide {
                            tick,
                            device: device.map(|id| id.0),
                            param,
                            slide: !lock.slide,
                        },
                    ));
                }
                (Field::Lock { device, param }, Edit::Clear) => {
                    work.push((
                        id,
                        Intent::ClearLock {
                            tick,
                            device: device.map(|id| id.0),
                            param,
                        },
                    ));
                }
                // ------------------------------------------------ the add
                //
                // Up lays the first lock and the column appears; there is
                // nothing for down to do, because a parameter with no
                // lock anywhere has nothing to turn.
                (Field::Add, Edit::Turn { up: true, .. }) => {
                    let Some((param, _)) = self.meter_add_param_from_plan(track, &plan) else {
                        continue;
                    };
                    let Some((knob, _, _)) = self.meter_standing(track, None, param) else {
                        continue;
                    };
                    work.push((
                        id,
                        Intent::SetLock {
                            tick,
                            device: None,
                            param,
                            value: knob,
                        },
                    ));
                }
                // ------------------------------------------- the whole trig
                (_, Edit::Trig) => {
                    let pitch = trig
                        .notes
                        .first()
                        .map(|note| note.pitch)
                        .unwrap_or_else(|| {
                            let octave = self.meter_octave(pattern, step);
                            crate::pitch::Pitch::from_midi(((octave + 1) * 12).clamp(0, 127) as u8)
                        });
                    work.push((
                        id,
                        Intent::Toggle {
                            tick,
                            default_pitch: pitch,
                            default_length_ticks: PATTERN_STEP_TICKS,
                            default_velocity: NEW_VELOCITY,
                        },
                    ));
                }
                (Field::Snd, Edit::Clear) if trig.sound.is_some() => {
                    released.push((id, step));
                }
                (Field::Note | Field::Vel | Field::Len, Edit::Clear) => {
                    work.push((id, Intent::Clear { tick }));
                }
                _ => {}
            }
        }
        if work.is_empty() && released.is_empty() {
            return Err(RefusalReason::Empty);
        }
        let laid = work.len() + released.len();
        for (id, step) in released {
            if let Some(pattern) = self.song.pattern_mut(id) {
                pattern.set_sound_lock(step, None);
            }
            self.touched();
        }
        let mut patterns: Vec<crate::sequencing::PatternId> = Vec::new();
        for (id, _) in &work {
            if !patterns.contains(id) {
                patterns.push(*id);
            }
        }
        for pattern in patterns {
            let intents: Vec<Intent> = work
                .iter()
                .filter(|(id, _)| *id == pattern)
                .map(|(_, intent)| *intent)
                .collect();
            self.apply_sequence(pattern, &intents);
        }
        if laid > 1 {
            self.notice = Some(format!("{laid} cells"));
        }
        Ok(())
    }
}
