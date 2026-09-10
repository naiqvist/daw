//! The MIDI Lab's LADDER: five rungs from an empty clip to a written one.
//!
//! The composer underneath can do almost anything — a hundred controls
//! over harmony, voicing, rhythm, motif, form and species counterpoint —
//! and that is exactly why it is hard to start. The ladder is a front
//! door, not a second engine: every rung drives the same
//! [`super::composer`] controls through the same `turn`, `text_edit` and
//! actions the inspector uses, so anything done here can be taken
//! further there, and nothing here can produce a composition the
//! inspector could not.
//!
//! The order is the point. A clip to write into, then chords, then who
//! plays, then how it feels, then hearing it — each rung asks one
//! question, with a list of answers and one of them already chosen.
//! `A` steps out to the full inspector at any time.

use super::composer::{self, Control};
use super::{RefusalReason, Stage, Step};
use crate::midi_lab::Destination;
use crate::midi_lab::Voice;

/// Where on the ladder the hands are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Rung {
    /// Where does it go: an existing clip, or a new one in an empty slot.
    #[default]
    Clip,
    /// What the harmony is.
    Chords,
    /// Who plays.
    Parts,
    /// How it moves.
    Feel,
    /// Hear it, then write it.
    Listen,
}

impl Rung {
    pub(super) const ALL: [Self; 5] = [
        Self::Clip,
        Self::Chords,
        Self::Parts,
        Self::Feel,
        Self::Listen,
    ];

    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Clip => "CLIP",
            Self::Chords => "CHORDS",
            Self::Parts => "PARTS",
            Self::Feel => "FEEL",
            Self::Listen => "LISTEN",
        }
    }

    /// The question this rung asks, in a line.
    pub(super) fn question(self) -> &'static str {
        match self {
            Self::Clip => "WHERE DOES IT GO",
            Self::Chords => "WHAT ARE THE CHORDS",
            Self::Parts => "WHO PLAYS",
            Self::Feel => "HOW DOES IT MOVE",
            Self::Listen => "HEAR IT, THEN WRITE IT",
        }
    }

    fn at(self) -> usize {
        Self::ALL.iter().position(|rung| *rung == self).unwrap_or(0)
    }

    fn step(self, forward: bool) -> Self {
        let at = self.at();
        let next = if forward {
            (at + 1).min(Self::ALL.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Ladder {
    pub rung: Rung,
    /// The row the cursor is on, within the rung.
    pub at: usize,
    /// The tonic the chord shapes are written from.
    pub tonic: u8,
}

/// What Enter does on a row, and what the arrows change.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum RowAct {
    /// An existing clip: take it as the destination.
    Take(Destination),
    /// An empty session slot: make a clip there, then take it.
    Make {
        track: usize,
        scene: usize,
    },
    /// A named chord shape, as the progression text it writes.
    Shape {
        text: String,
    },
    /// The tonic those shapes are written from.
    Tonic,
    /// One of the five parts: Enter turns it on or off, the arrows walk
    /// how it plays.
    Part {
        voice: usize,
        style: Control,
    },
    /// A control the arrows turn.
    Turn(Control),
    /// Hear the harmony, play the phrase, write it to the clips.
    Hear,
    Play,
    Send,
}

/// One row of a rung: what it is, what it says, and what it does.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub label: String,
    pub value: String,
    /// The line under the label, when a row needs one.
    pub note: String,
    /// Whether the row is the standing answer to the rung's question.
    pub chosen: bool,
    pub act: RowAct,
}

/// The chord shapes the CHORDS rung offers: SEMITONES above the tonic,
/// a quality and a length in beats.
///
/// Semitones rather than scale degrees, because half of these are minor
/// and spelling a minor loop through the major scale gives you the wrong
/// three chords — which is exactly what the first version did.
const SHAPES: &[(&str, &str, &[(u8, &str, u8)])] = &[
    (
        "ii-V-I",
        "the cadence most of this music is made of",
        &[(2, "m7", 4), (7, "7", 4), (0, "maj7", 8)],
    ),
    (
        "I-vi-ii-V",
        "the turnaround: four bars that come back round",
        &[(0, "maj7", 4), (9, "m7", 4), (2, "m7", 4), (7, "7", 4)],
    ),
    (
        "i-VI-III-VII",
        "minor loop, the one that sounds like rain",
        &[(0, "m9", 4), (8, "maj7", 4), (3, "maj7", 4), (10, "7", 4)],
    ),
    (
        "i-iv vamp",
        "two chords, breathing: room for everything else",
        &[(0, "m9", 8), (5, "m9", 8)],
    ),
    (
        "modal I-II",
        "dorian pair, no cadence, no gravity",
        &[(0, "m9", 8), (2, "9", 8)],
    ),
    (
        "one chord",
        "a single span to improvise over",
        &[(0, "maj9", 16)],
    ),
];

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A shape written from a tonic: `Dm7:4 G7:4 Cmaj7:8`.
fn shape_text(tonic: u8, steps: &[(u8, &str, u8)]) -> String {
    steps
        .iter()
        .map(|(semitones, quality, beats)| {
            let root = (tonic + semitones) % 12;
            format!("{}{quality}:{beats}", NAMES[usize::from(root)])
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The subject that makes the composer read THIS voice.
///
/// `composer::voice` takes the voice from the subject for three of the
/// five, and only falls back to `state.voice` for the rest — so a row
/// that set the voice alone would read the chords' rhythm five times
/// over, which is exactly what it did until this existed.
fn subject_of(voice: Voice) -> composer::Subject {
    match voice {
        Voice::Chords => composer::Subject::Harmony,
        Voice::Bass => composer::Subject::Bass,
        Voice::Counterpoint => composer::Subject::Counterpoint,
        Voice::Arp | Voice::Melody => composer::Subject::Melody,
    }
}

impl Stage {
    /// The ladder of the focused MIDI Lab window, if it is on one.
    pub(super) fn midi_ladder(&self) -> Option<Ladder> {
        self.midi_ladder_window()
            .and_then(|window| self.midi_window(window).and_then(|state| state.ladder))
    }

    fn midi_ladder_window(&self) -> Option<usize> {
        let (window, _) = self.midi_focus()?;
        Some(window)
    }

    /// Every row of the rung the ladder is on.
    pub(super) fn midi_ladder_rows(&self) -> Vec<Row> {
        let Some(ladder) = self.midi_ladder() else {
            return Vec::new();
        };
        let Some((window, draft)) = self.midi_focus() else {
            return Vec::new();
        };
        let Some(composer_state) = self.midi_window(window).map(|state| &state.composer) else {
            return Vec::new();
        };
        match ladder.rung {
            Rung::Clip => self.ladder_clip_rows(draft),
            Rung::Chords => self.ladder_chord_rows(draft, ladder.tonic),
            Rung::Parts => self.ladder_part_rows(draft, composer_state),
            Rung::Feel => self.ladder_feel_rows(draft, composer_state),
            Rung::Listen => self.ladder_listen_rows(draft),
        }
    }

    /// The clips this lab could write into, and the empty slots it could
    /// make one in. The list IS the question: no tag has to be typed and
    /// no slot has to be found first.
    fn ladder_clip_rows(&self, draft: u64) -> Vec<Row> {
        let standing = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.id == draft)
            .and_then(|d| d.destination);
        let mut rows = Vec::new();
        for (index, track) in self.song.tracks.iter().enumerate() {
            if track.machine.is_none() || track.letter.is_empty() {
                continue;
            }
            for pattern in &self.song.patterns {
                let Some(rest) = pattern.tag.strip_prefix(&track.letter) else {
                    continue;
                };
                if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let notes = (0..pattern.step_count())
                    .map(|i| pattern.trig(i).notes.len())
                    .sum::<usize>();
                let bars = pattern.length_ticks as f64 / (4.0 * 48.0);
                let target = Destination {
                    track: track.id,
                    pattern: pattern.id,
                };
                rows.push(Row {
                    label: pattern.tag.clone(),
                    value: if notes == 0 {
                        "empty".to_owned()
                    } else {
                        format!("{notes} notes")
                    },
                    note: format!("{} · {bars:.0} bars", track.name),
                    chosen: standing == Some(target),
                    act: RowAct::Take(target),
                });
            }
            // And the first empty slot on this track, so a new clip is a
            // row rather than a trip to the matrix.
            if let Some(scene) = self.ladder_free_slot(index) {
                rows.push(Row {
                    label: format!("+ new on {}", track.name),
                    value: format!("scene {}", scene + 1),
                    note: "an empty slot: a clip is made here".to_owned(),
                    chosen: false,
                    act: RowAct::Make {
                        track: index,
                        scene,
                    },
                });
            }
        }
        rows
    }

    /// The first scene with no clip on this track.
    fn ladder_free_slot(&self, track: usize) -> Option<usize> {
        let id = self.song.tracks.get(track)?.id;
        (0..self.song.session.scenes.len())
            .find(|scene| self.song.session.scenes[*scene].clip(id).is_none())
    }

    fn ladder_chord_rows(&self, draft: u64, tonic: u8) -> Vec<Row> {
        let standing = self
            .midi_recipe(draft)
            .map(super::midi_lab::progression_text)
            .unwrap_or_default();
        let mut rows = vec![Row {
            label: "key".to_owned(),
            value: NAMES[usize::from(tonic % 12)].to_owned(),
            note: "the shapes below are written from here".to_owned(),
            chosen: false,
            act: RowAct::Tonic,
        }];
        for (name, note, degrees) in SHAPES {
            let text = shape_text(tonic, degrees);
            rows.push(Row {
                label: (*name).to_owned(),
                value: text.clone(),
                note: (*note).to_owned(),
                chosen: standing == text,
                act: RowAct::Shape { text },
            });
        }
        rows
    }

    /// The five parts, each with how it plays. Enter turns one on; the
    /// arrows walk its manner.
    fn ladder_part_rows(&self, draft: u64, composer_state: &composer::State) -> Vec<Row> {
        let Some(recipe) = self.midi_recipe(draft) else {
            return Vec::new();
        };
        let Some(composition) = recipe.composition.as_ref() else {
            return Vec::new();
        };
        [
            (Voice::Chords, Control::Rhythm, "the harmony itself"),
            (Voice::Arp, Control::Rhythm, "the chord, one note at a time"),
            (Voice::Melody, Control::Contour, "the tune over the top"),
            (Voice::Bass, Control::BassRole, "the bottom of the music"),
            (
                Voice::Counterpoint,
                Control::Contour,
                "a second line against the first",
            ),
        ]
        .into_iter()
        .map(|(voice, style, note)| {
            let index = voice.index();
            let on = composition
                .voices
                .get(index)
                .is_some_and(|part| part.enabled);
            let mut state = composer_state.clone();
            state.voice = voice;
            state.subject = subject_of(voice);
            Row {
                label: voice.label().to_owned(),
                value: if on {
                    composer::reading(style, composition, &state)
                } else {
                    "off".to_owned()
                },
                note: note.to_owned(),
                chosen: on,
                act: RowAct::Part {
                    voice: index,
                    style,
                },
            }
        })
        .collect()
    }

    fn ladder_feel_rows(&self, draft: u64, composer_state: &composer::State) -> Vec<Row> {
        let Some(recipe) = self.midi_recipe(draft) else {
            return Vec::new();
        };
        let Some(composition) = recipe.composition.as_ref() else {
            return Vec::new();
        };
        let state = composer_state;
        [
            (Control::Swing, "swing", "how far the offbeats lean"),
            (
                Control::TensionDensity,
                "density",
                "how much happens in a bar",
            ),
            (
                Control::TensionRegister,
                "register",
                "how high the parts sit",
            ),
            (
                Control::TensionVelocity,
                "dynamics",
                "how hard the notes are struck",
            ),
        ]
        .into_iter()
        .map(|(control, label, note)| Row {
            label: label.to_owned(),
            value: composer::reading(control, composition, &state),
            note: note.to_owned(),
            chosen: false,
            act: RowAct::Turn(control),
        })
        .collect()
    }

    fn ladder_listen_rows(&self, draft: u64) -> Vec<Row> {
        let where_to = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.id == draft)
            .and_then(|d| d.destination)
            .map(|d| self.song.tag_of(d.pattern))
            .unwrap_or_else(|| "no clip".to_owned());
        let (spans, parts) = self
            .midi_recipe(draft)
            .and_then(|recipe| recipe.composition.as_ref())
            .map(|c| {
                (
                    c.harmony.len(),
                    c.voices.iter().filter(|part| part.enabled).count(),
                )
            })
            .unwrap_or((0, 0));
        vec![
            Row {
                label: "hear the chords".to_owned(),
                value: format!("{spans} spans"),
                note: "the harmony alone, without the parts".to_owned(),
                chosen: false,
                act: RowAct::Hear,
            },
            Row {
                label: "play it".to_owned(),
                value: format!("{parts} parts"),
                note: "the whole phrase, on the destination instrument".to_owned(),
                chosen: false,
                act: RowAct::Play,
            },
            Row {
                label: "write it to the clip".to_owned(),
                value: where_to,
                note: "one undoable edit; the clip keeps its place".to_owned(),
                chosen: false,
                act: RowAct::Send,
            },
        ]
    }
}

// ----------------------------------------------------------- the verbs --

impl Stage {
    fn ladder_focus(&self) -> Option<(usize, u64, Ladder)> {
        let (window, draft) = self.midi_focus()?;
        let ladder = self.midi_window(window)?.ladder?;
        Some((window, draft, ladder))
    }

    fn set_ladder(&mut self, window: usize, ladder: Ladder) {
        if let Some(state) = self.midi_window_mut(window) {
            state.ladder = Some(ladder);
        }
    }

    /// Up and down the rung's rows. The ends are edges, not wraps: a
    /// ladder should feel like it has a top and a bottom.
    pub(super) fn ladder_move(&mut self, step: Step) -> Result<(), RefusalReason> {
        let (window, _, mut ladder) = self.ladder_focus().ok_or(RefusalReason::Empty)?;
        let rows = self.midi_ladder_rows().len();
        if rows == 0 {
            return Err(RefusalReason::Empty);
        }
        let next = match step {
            Step::Up => ladder.at.checked_sub(1),
            Step::Down => (ladder.at + 1 < rows).then_some(ladder.at + 1),
            Step::Left | Step::Right => None,
        };
        let at = next.ok_or(RefusalReason::Edge(step))?;
        ladder.at = at;
        self.set_ladder(window, ladder);
        Ok(())
    }

    /// Left and right: the value on the row under the cursor. A row that
    /// has no value to turn says so rather than moving something else.
    pub(super) fn ladder_turn(&mut self, by: i32) -> Result<(), RefusalReason> {
        let (window, draft, mut ladder) = self.ladder_focus().ok_or(RefusalReason::Empty)?;
        let rows = self.midi_ladder_rows();
        let row = rows.get(ladder.at).ok_or(RefusalReason::Empty)?;
        let (control, voice) = match &row.act {
            RowAct::Tonic => {
                ladder.tonic = (i32::from(ladder.tonic) + by).rem_euclid(12) as u8;
                self.set_ladder(window, ladder);
                return Ok(());
            }
            RowAct::Part { voice, style } => (*style, Some(*voice)),
            RowAct::Turn(control) => (*control, None),
            _ => return Err(RefusalReason::Unavailable),
        };
        // The composer's own arithmetic, on the composer's own state:
        // the ladder turns the knob the inspector turns.
        let mut state = self
            .midi_window(window)
            .ok_or(RefusalReason::Empty)?
            .composer
            .clone();
        if let Some(index) = voice {
            let voice = Voice::ALL[index.min(Voice::ALL.len() - 1)];
            state.voice = voice;
            state.subject = subject_of(voice);
        }
        let said = {
            let composition = self
                .song
                .midi_labs
                .iter_mut()
                .find(|d| d.id == draft)
                .and_then(|d| d.recipe.composition.as_mut())
                .ok_or(RefusalReason::Empty)?;
            composer::turn(composition, &mut state, control, by)
                .map_err(|_| RefusalReason::Unavailable)?;
            composer::reading(control, composition, &state)
        };
        if let Some(window) = self.midi_window_mut(window) {
            window.composer = state;
            window.status = said;
        }
        Ok(())
    }

    /// Enter: take the row. On the rungs that ask ONE question — where it
    /// goes, what the chords are — taking the answer climbs to the next
    /// rung, because that is what makes this a ladder rather than a page.
    pub(super) fn ladder_enter(&mut self) -> Result<(), RefusalReason> {
        let (window, draft, mut ladder) = self.ladder_focus().ok_or(RefusalReason::Empty)?;
        let rows = self.midi_ladder_rows();
        let row = rows.get(ladder.at).cloned().ok_or(RefusalReason::Empty)?;
        match row.act {
            RowAct::Take(destination) => {
                self.ladder_target(window, draft, destination);
                ladder.rung = Rung::Chords;
                ladder.at = 0;
                self.set_ladder(window, ladder);
                Ok(())
            }
            RowAct::Make { track, scene } => {
                // The empty slot becomes a clip, and the clip becomes the
                // destination: one keystroke instead of a trip to the
                // matrix and back with a tag in your head.
                let pattern = self
                    .song
                    .fill_slot(track, scene)
                    .ok_or(RefusalReason::Unavailable)?;
                let id = self
                    .song
                    .tracks
                    .get(track)
                    .map(|track| track.id)
                    .ok_or(RefusalReason::Unavailable)?;
                self.ladder_target(window, draft, Destination { track: id, pattern });
                self.remixed();
                ladder.rung = Rung::Chords;
                ladder.at = 0;
                self.set_ladder(window, ladder);
                Ok(())
            }
            RowAct::Shape { text } => {
                let mut state = self
                    .midi_window(window)
                    .ok_or(RefusalReason::Empty)?
                    .composer
                    .clone();
                {
                    let composition = self
                        .song
                        .midi_labs
                        .iter_mut()
                        .find(|d| d.id == draft)
                        .and_then(|d| d.recipe.composition.as_mut())
                        .ok_or(RefusalReason::Empty)?;
                    composer::text_edit(composition, &mut state, Control::Progression, &text)
                        .map_err(|_| RefusalReason::Unavailable)?;
                }
                if let Some(window) = self.midi_window_mut(window) {
                    window.composer = state;
                    window.progression = text.clone();
                    window.status = format!("chords · {text}");
                }
                ladder.rung = Rung::Parts;
                ladder.at = 0;
                self.set_ladder(window, ladder);
                Ok(())
            }
            RowAct::Part { voice, .. } => {
                let mut state = self
                    .midi_window(window)
                    .ok_or(RefusalReason::Empty)?
                    .composer
                    .clone();
                let which = Voice::ALL[voice.min(Voice::ALL.len() - 1)];
                state.voice = which;
                state.subject = subject_of(which);
                let said = {
                    let composition = self
                        .song
                        .midi_labs
                        .iter_mut()
                        .find(|d| d.id == draft)
                        .and_then(|d| d.recipe.composition.as_mut())
                        .ok_or(RefusalReason::Empty)?;
                    let part = composition
                        .voices
                        .get_mut(voice)
                        .ok_or(RefusalReason::Empty)?;
                    part.enabled = !part.enabled;
                    format!(
                        "{} {}",
                        state.voice.label(),
                        if part.enabled { "on" } else { "off" }
                    )
                };
                if let Some(window) = self.midi_window_mut(window) {
                    window.composer = state;
                    window.status = said;
                }
                Ok(())
            }
            RowAct::Tonic | RowAct::Turn(_) => Err(RefusalReason::Unavailable),
            RowAct::Hear => self.midi_intent(super::StageIntent::MidiLabHear),
            RowAct::Play => self.midi_intent(super::StageIntent::MidiLabPlay),
            RowAct::Send => self.midi_intent(super::StageIntent::MidiLabSend),
        }
    }

    /// The clip this lab writes into, and the address the rest of the
    /// lab reads it by.
    fn ladder_target(&mut self, window: usize, draft: u64, destination: Destination) {
        if let Some(entry) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
            entry.destination = Some(destination);
        }
        let tag = self.song.tag_of(destination.pattern);
        if let Some(state) = self.midi_window_mut(window) {
            state.address = tag.clone();
            state.status = format!("writing into {tag}");
        }
    }

    /// Tab and Shift+Tab: the next rung and the one before. The ends
    /// hold rather than wrap — a ladder has a top.
    pub(super) fn ladder_rung(&mut self, forward: bool) -> Result<(), RefusalReason> {
        let (window, _, mut ladder) = self.ladder_focus().ok_or(RefusalReason::Empty)?;
        let next = ladder.rung.step(forward);
        if next == ladder.rung {
            return Err(RefusalReason::Edge(if forward {
                Step::Right
            } else {
                Step::Left
            }));
        }
        ladder.rung = next;
        ladder.at = 0;
        self.set_ladder(window, ladder);
        if let Some(state) = self.midi_window_mut(window) {
            state.status = next.question().to_owned();
        }
        Ok(())
    }

    /// `A`: out to the full inspector, and back again. Everything the
    /// ladder set is already in the composition, so nothing is lost in
    /// either direction.
    pub(super) fn ladder_toggle(&mut self) -> Result<(), RefusalReason> {
        let (window, _) = self.midi_focus().ok_or(RefusalReason::Empty)?;
        let now = self.midi_window(window).and_then(|state| state.ladder);
        if let Some(state) = self.midi_window_mut(window) {
            state.ladder = match now {
                Some(_) => None,
                None => Some(Ladder::default()),
            };
            state.status = if state.ladder.is_some() {
                "the ladder · Tab walks the rungs".to_owned()
            } else {
                "the full inspector · A returns to the ladder".to_owned()
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::key::{Key, Mods};

    /// A lab with one instrument track and a clip on it.
    fn opened() -> Stage {
        let mut stage = Stage::new();
        stage.open_midi_lab("");
        stage
    }

    fn ladder_of(stage: &Stage) -> Ladder {
        stage.midi_ladder().expect("the lab opens on the ladder")
    }

    fn press(stage: &mut Stage, key: Key) -> Option<super::super::ApplyOutcome> {
        stage.handle_key(Mods::NONE, key)
    }

    #[test]
    fn a_new_lab_opens_on_the_first_rung() {
        let stage = opened();
        let ladder = ladder_of(&stage);
        assert_eq!(ladder.rung, Rung::Clip);
        assert_eq!(ladder.at, 0);
        assert_eq!(Rung::Clip.question(), "WHERE DOES IT GO");
    }

    /// The first question is answered from a LIST: every clip that can be
    /// written into, and an empty slot on each track that has one. No tag
    /// is typed and no slot has to be found first.
    #[test]
    fn the_clip_rung_offers_every_clip_and_an_empty_slot() {
        let stage = opened();
        let rows = stage.midi_ladder_rows();
        assert!(!rows.is_empty(), "the clip rung offered nothing");
        let existing = rows
            .iter()
            .filter(|row| matches!(row.act, RowAct::Take(_)))
            .count();
        let new = rows
            .iter()
            .filter(|row| matches!(row.act, RowAct::Make { .. }))
            .count();
        assert!(existing >= 1, "no existing clip was offered");
        assert!(new >= 1, "no empty slot was offered");
        // Every row says what it is: a tag, a reading and a line.
        for row in &rows {
            assert!(!row.label.is_empty());
            assert!(!row.note.is_empty());
        }
    }

    /// Taking a clip sets the destination and climbs — that is what makes
    /// this a ladder rather than a page of controls.
    #[test]
    fn taking_a_clip_targets_it_and_climbs() {
        let mut stage = opened();
        let rows = stage.midi_ladder_rows();
        let at = rows
            .iter()
            .position(|row| matches!(row.act, RowAct::Take(_)))
            .expect("an existing clip");
        let RowAct::Take(wanted) = rows[at].act else {
            panic!("not a clip row")
        };
        if let Some(window) = stage.midi_window_mut(0)
            && let Some(ladder) = window.ladder.as_mut()
        {
            ladder.at = at;
        }
        assert_eq!(
            press(&mut stage, Key::Enter),
            Some(super::super::ApplyOutcome::Changed)
        );
        assert_eq!(stage.song.midi_labs[0].destination, Some(wanted));
        assert_eq!(ladder_of(&stage).rung, Rung::Chords);
    }

    /// AND IT CAN MAKE ONE: an empty slot becomes a clip, and that clip
    /// becomes the destination, without leaving the lab.
    #[test]
    fn an_empty_slot_becomes_a_clip_and_the_destination() {
        let mut stage = opened();
        let before = stage.song.patterns.len();
        let rows = stage.midi_ladder_rows();
        let at = rows
            .iter()
            .position(|row| matches!(row.act, RowAct::Make { .. }))
            .expect("an empty slot");
        if let Some(window) = stage.midi_window_mut(0)
            && let Some(ladder) = window.ladder.as_mut()
        {
            ladder.at = at;
        }
        let _ = press(&mut stage, Key::Enter);
        assert_eq!(
            stage.song.patterns.len(),
            before + 1,
            "no clip was made in the empty slot"
        );
        let destination = stage.song.midi_labs[0]
            .destination
            .expect("the new clip is the destination");
        assert!(
            stage.song.pattern(destination.pattern).is_some(),
            "the destination is not a pattern of this song"
        );
        assert_eq!(ladder_of(&stage).rung, Rung::Chords);
    }

    /// The chords rung writes a real progression, and the key it is
    /// written from is a row of its own.
    #[test]
    fn a_shape_writes_the_progression_in_the_chosen_key() {
        let mut stage = opened();
        if let Some(window) = stage.midi_window_mut(0) {
            window.ladder = Some(Ladder {
                rung: Rung::Chords,
                at: 0,
                tonic: 0,
            });
        }
        let rows = stage.midi_ladder_rows();
        assert!(matches!(rows[0].act, RowAct::Tonic), "the key leads");
        assert_eq!(rows[0].value, "C");
        // The shapes are written from the tonic.
        let RowAct::Shape { text } = &rows[1].act else {
            panic!("the first shape")
        };
        assert!(text.starts_with("Dm7:4 G7:4 Cmaj7"), "in C: {text}");

        // Turn the key and they follow.
        let _ = press(&mut stage, Key::ArrowRight);
        let _ = press(&mut stage, Key::ArrowRight);
        let rows = stage.midi_ladder_rows();
        assert_eq!(rows[0].value, "D");
        let RowAct::Shape { text } = &rows[1].act else {
            panic!("the first shape")
        };
        assert!(text.starts_with("Em7:4 A7:4 Dmaj7"), "in D: {text}");

        // Taking one writes it into the composition and climbs.
        let wanted = text.clone();
        let _ = press(&mut stage, Key::ArrowDown);
        let _ = press(&mut stage, Key::Enter);
        assert_eq!(ladder_of(&stage).rung, Rung::Parts);
        let composition = stage.song.midi_labs[0]
            .recipe
            .composition
            .as_ref()
            .expect("a composition");
        assert_eq!(composition.harmony.len(), 3, "three spans were written");
        // The COMPOSITION is what the composer writes; the recipe's own
        // harmony is the legacy generator's and stays where it was.
        let written = composition
            .harmony
            .iter()
            .map(|span| span.material.label())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            written.contains("Em7") && written.contains("A7") && written.contains("Dmaj7"),
            "the chords written were {written}, not {wanted}"
        );
    }

    /// The parts rung is five switches, and the arrows walk how each one
    /// plays — the same controls the inspector turns.
    #[test]
    fn the_parts_rung_turns_voices_on_and_walks_their_manner() {
        let mut stage = opened();
        if let Some(window) = stage.midi_window_mut(0) {
            window.ladder = Some(Ladder {
                rung: Rung::Parts,
                at: 0,
                tonic: 0,
            });
        }
        let rows = stage.midi_ladder_rows();
        assert_eq!(rows.len(), 5, "five parts");
        assert_eq!(rows[0].label, "Chords");
        assert_eq!(rows[3].label, "Bass");

        // A part that is off turns on, and the row stops saying "off".
        let at = rows
            .iter()
            .position(|row| !row.chosen)
            .expect("some part starts off");
        for _ in 0..at {
            let _ = press(&mut stage, Key::ArrowDown);
        }
        let _ = press(&mut stage, Key::Enter);
        let rows = stage.midi_ladder_rows();
        assert!(rows[at].chosen, "Enter did not switch the part on");
        assert_ne!(rows[at].value, "off", "an on part still reads off");

        // And the arrows walk its manner without switching it off.
        let before = rows[at].value.clone();
        let _ = press(&mut stage, Key::ArrowRight);
        let rows = stage.midi_ladder_rows();
        assert!(rows[at].chosen, "turning the manner switched the part off");
        assert_ne!(rows[at].value, before, "the manner did not move");

        // Each row reads ITS OWN voice, not the first one's.
        let values: Vec<&str> = rows.iter().map(|row| row.value.as_str()).collect();
        assert!(
            values
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "every part read the same control: {values:?}"
        );
    }

    /// Tab climbs, Escape goes back down, and the ends hold.
    #[test]
    fn the_rungs_climb_and_descend_with_ends_that_hold() {
        let mut stage = opened();
        for wanted in [Rung::Chords, Rung::Parts, Rung::Feel, Rung::Listen] {
            let _ = press(&mut stage, Key::Tab);
            assert_eq!(ladder_of(&stage).rung, wanted);
        }
        // The top holds.
        assert!(matches!(
            press(&mut stage, Key::Tab),
            Some(super::super::ApplyOutcome::Refused(_))
        ));
        assert_eq!(ladder_of(&stage).rung, Rung::Listen);
        for wanted in [Rung::Feel, Rung::Parts, Rung::Chords, Rung::Clip] {
            let _ = press(&mut stage, Key::Escape);
            assert_eq!(ladder_of(&stage).rung, wanted);
        }
    }

    /// `A` is a door, not a mode: what the ladder set is still there in
    /// the inspector, and the ladder comes back where it was left.
    #[test]
    fn the_inspector_is_one_key_away_and_keeps_the_work() {
        let mut stage = opened();
        if let Some(window) = stage.midi_window_mut(0) {
            window.ladder = Some(Ladder {
                rung: Rung::Chords,
                at: 1,
                tonic: 0,
            });
        }
        let _ = press(&mut stage, Key::Enter);
        let written = |stage: &Stage| {
            stage.song.midi_labs[0]
                .recipe
                .composition
                .as_ref()
                .map(|c| {
                    c.harmony
                        .iter()
                        .map(|span| span.material.label())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        };
        let chords = written(&stage);
        assert!(!chords.is_empty());

        let _ = press(&mut stage, Key::A);
        assert!(
            stage.midi_ladder().is_none(),
            "A did not open the inspector"
        );
        assert_eq!(
            written(&stage),
            chords,
            "the inspector lost what the ladder wrote"
        );
        let _ = press(&mut stage, Key::A);
        assert!(stage.midi_ladder().is_some(), "A did not come back");
    }

    /// The last rung says what it will write, and where.
    #[test]
    fn the_listen_rung_says_what_it_will_do() {
        let mut stage = opened();
        if let Some(window) = stage.midi_window_mut(0) {
            window.ladder = Some(Ladder {
                rung: Rung::Listen,
                at: 0,
                tonic: 0,
            });
        }
        let rows = stage.midi_ladder_rows();
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0].act, RowAct::Hear));
        assert!(matches!(rows[1].act, RowAct::Play));
        assert!(matches!(rows[2].act, RowAct::Send));
        // The send row names the clip it will write into.
        assert!(
            !rows[2].value.is_empty(),
            "the send row does not say where it goes"
        );
    }
}
