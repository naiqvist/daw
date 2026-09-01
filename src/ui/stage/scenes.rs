//! Scenes — the session's rows, and the lattice they make with the tracks.
//!
//! A scene is one row across every track: the Ableton half of the
//! identity, where a row is launched as a unit and each cell of it is a
//! slot a clip can occupy. The strip already runs tracks ACROSS, one
//! column each, so scenes fall out as the other axis of the same surface:
//! a lattice of slots beneath the heads, one per (track, scene).
//!
//! **The Song owns the session** (`sequencing::Session`): how many scenes
//! there are and what each slot holds. This module reduces that to what
//! the lattice draws and answers geometry — it holds no session state of
//! its own, so a slot can never show something the document does not
//! have.
//!
//! **A slot is exactly as wide as its head.** The column IS the track's
//! address on this surface; a slot narrower or wider than the head above
//! it would be a second column the eye has to reconcile with the first.
//! The geometry below takes the head's rectangle and stacks beneath it —
//! it never computes a column width of its own.
//!
//! **A filled slot says two things and no more:** what KIND of thing is
//! here, as one glyph, and WHICH one, as a number. No names. A name is
//! prose, and prose in a lattice cell is read rather than seen; the
//! session is a surface for seeing, and the pattern's number is the
//! address a performer will learn the way they learn a track's column.

use super::browser::glyph;
use crate::sequencing::{Clip, Song};
use eframe::egui;

/// A slot's height. A LAYOUT dimension like the track column's: settled by
/// eye, fixed so the lattice never resizes with the scene count, and
/// shorter than a head because a slot carries one fact where a head
/// carries two.
pub const SLOT_H: f32 = 40.0;

/// What a cell of the session lattice stands for. The lattice's first
/// row is the heads — a track's identity, and the cell that opens it —
/// and every row after is a scene. The focus apparatus knows only
/// `(col, row)`; this is where those numbers get their meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Address {
    /// The head of track `track`.
    Head { track: usize },
    /// The slot where `track` meets `scene`.
    Slot { track: usize, scene: usize },
}

impl Address {
    pub fn of((col, row): (usize, usize)) -> Self {
        match row.checked_sub(1) {
            None => Self::Head { track: col },
            Some(scene) => Self::Slot { track: col, scene },
        }
    }

    pub fn track(self) -> usize {
        match self {
            Self::Head { track } | Self::Slot { track, .. } => track,
        }
    }
}

/// How many rows the focus lattice has for `song`: the head row, then
/// one per scene. The head row exists even for a session with no scenes,
/// because a track is addressable before it has anywhere to put a clip.
pub fn lattice_rows(song: &Song) -> usize {
    1 + song.session.scenes.len()
}

/// What a filled slot draws: one glyph for the kind of clip, one number
/// for which one. Reduced from the model every frame, never stored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mark {
    pub glyph: char,
    pub number: String,
}

/// The mark for the slot at `scene` on the track at `track_index`, or
/// `None` for an empty slot (and for a place that does not exist, which
/// draws the same way: as nothing).
pub fn mark(song: &Song, track_index: usize, scene: usize) -> Option<Mark> {
    let clip = song.slot_clip(track_index, scene)?;
    Some(match clip {
        // The filled square is the pattern's sign: a solid thing in a
        // cell, the way a trig is a solid thing in a step.
        Clip::Pattern(id) => Mark {
            glyph: glyph::DOT,
            number: format!("{:02}", id.0),
        },
    })
}

/// Every whole row of slots that fits in `room`, with `gap` between rows
/// and none after the last. A partial row is never drawn — a slot cut off
/// by the field's edge would read as a different, shorter kind of slot.
pub fn rows_that_fit(room: f32, gap: f32) -> usize {
    if room < SLOT_H {
        return 0;
    }
    ((room + gap) / (SLOT_H + gap)).floor() as usize
}

/// The slot `row` rows beneath `head`, sharing its left edge and width
/// exactly. The first row sits one `gap` under the head, and every row
/// after sits one `gap` under the one before.
pub fn slot_beneath(head: egui::Rect, row: usize, gap: f32) -> egui::Rect {
    let top = head.max.y + gap + row as f32 * (SLOT_H + gap);
    egui::Rect::from_min_size(
        egui::pos2(head.min.x, top),
        egui::vec2(head.width(), SLOT_H),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::SESSION_SCENES;

    const GAP: f32 = 8.0;

    fn head() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(50.0, 20.0), egui::vec2(132.0, 64.0))
    }

    #[test]
    fn the_head_row_comes_before_the_scenes() {
        assert_eq!(Address::of((3, 0)), Address::Head { track: 3 });
        assert_eq!(Address::of((3, 1)), Address::Slot { track: 3, scene: 0 });
        assert_eq!(Address::of((0, 8)), Address::Slot { track: 0, scene: 7 });
        assert_eq!(Address::of((3, 5)).track(), 3);
        assert_eq!(lattice_rows(&Song::default()), 1 + SESSION_SCENES);
    }

    #[test]
    fn an_empty_slot_draws_nothing_and_a_filled_one_draws_its_number() {
        let mut song = Song::default();
        assert_eq!(mark(&song, 0, 0), None);
        let id = song.fill_slot(0, 0).expect("fill");
        assert_eq!(
            mark(&song, 0, 0),
            Some(Mark {
                glyph: glyph::DOT,
                number: format!("{:02}", id.0),
            })
        );
        assert_eq!(
            mark(&song, 0, 1),
            None,
            "the mark leaked into another scene"
        );
        assert_eq!(
            mark(&song, 7, 0),
            None,
            "a track that does not exist drew a mark"
        );
    }

    #[test]
    fn a_mark_never_carries_a_name() {
        let mut song = Song::default();
        song.fill_slot(0, 0).expect("fill");
        let mark = mark(&song, 0, 0).expect("mark");
        assert!(
            mark.number.chars().all(|ch| ch.is_ascii_digit()),
            "the number was not a number: {:?}",
            mark.number
        );
        for pattern in &song.patterns {
            assert!(
                !mark.number.contains(&pattern.name),
                "the pattern's name reached the lattice"
            );
        }
    }

    #[test]
    fn every_slot_is_exactly_as_wide_as_its_head() {
        for row in 0..SESSION_SCENES {
            let slot = slot_beneath(head(), row, GAP);
            assert_eq!(slot.min.x, head().min.x, "row {row} drifted sideways");
            assert_eq!(slot.width(), head().width(), "row {row} changed width");
            assert_eq!(slot.height(), SLOT_H);
        }
    }

    #[test]
    fn rows_stack_downward_by_one_gap_and_never_overlap() {
        let first = slot_beneath(head(), 0, GAP);
        assert_eq!(
            first.min.y,
            head().max.y + GAP,
            "the first row touched the head"
        );
        for row in 1..SESSION_SCENES {
            let above = slot_beneath(head(), row - 1, GAP);
            let below = slot_beneath(head(), row, GAP);
            assert_eq!(below.min.y, above.max.y + GAP, "row {row} lost its gap");
        }
    }

    #[test]
    fn only_whole_rows_fit() {
        assert_eq!(rows_that_fit(0.0, GAP), 0);
        assert_eq!(
            rows_that_fit(SLOT_H - 1.0, GAP),
            0,
            "a partial row was counted"
        );
        assert_eq!(rows_that_fit(SLOT_H, GAP), 1);
        // Two rows need two slots and ONE gap: no gap after the last.
        assert_eq!(rows_that_fit(SLOT_H * 2.0 + GAP, GAP), 2);
        assert_eq!(rows_that_fit(SLOT_H * 2.0 + GAP - 1.0, GAP), 1);
    }
}
