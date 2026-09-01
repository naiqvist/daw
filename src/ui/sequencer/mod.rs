//! The sequencer — the step grid, the piano roll, the trig inspector, and
//! the command grammar they are spoken through. Frame-independent.
//!
//! Born inside `ui::redesign` (the second frame) and lifted out whole once
//! a third frame wanted it, for the reason `crate::intent` was lifted: a
//! surface owned by one frame is one a second frame must either import
//! from the frame it is replacing, or mint again and let drift. Both
//! frames now draw THIS sequencer, and its grammar, verb table and
//! registers are the one keyboard vocabulary for editing a pattern.
//!
//! What it may depend on: the model (`sequencing`, `pitch`), the design
//! system (`design::signs`, `ui::tokens`, `ui::affordance`), and egui.
//! What it may not: any frame. Nothing here names `redesign` or `stage`.
//!
//! Contracts: `notes/20260831-command-grammar.md` (the sentence shape,
//! verbs, registers, refusals), `notes/20260831-pitch-lens-spec.md` (how
//! pitch is read and entered), and the sequencing contract for what the
//! edits compile to.

pub mod grammar;
pub mod grid_resolution;
pub mod layout_grid;
pub mod lens;
pub mod midi_typing;
pub mod registers;
pub mod roll;
pub mod sequence;
pub mod sequence_grid;
pub mod trig_info;
pub mod verbs;

use crate::pitch::Key;
use crate::sequencing::{
    DEFAULT_PATTERN_TICKS, Note, PATTERN_STEP_TICKS, PATTERN_STEPS, Pattern, PatternId, Song,
};
use eframe::egui;
use sequence::NoteView;

/// The sequencer's loudest value: the ink every certain thing is drawn
/// in, and the cursor. Hierarchy in this surface comes from value alone
/// (the second frame's charter, kept), and this is the top of it.
pub const INK: egui::Color32 = egui::Color32::WHITE;

/// Above this many cents from the nearest twelve-tone pitch, the bridge-
/// era playback path is approximating, and the view says so.
const APPROX_CENTS: f64 = 0.05;

/// How long `id` runs on the timeline, or a pattern's natural length when
/// nothing has placed it — a session clip's case.
pub fn pattern_length(song: &Song, id: PatternId) -> usize {
    song.tracks
        .iter()
        .flat_map(|track| &track.blocks)
        .find(|block| block.pattern_id == id)
        .map_or(DEFAULT_PATTERN_TICKS, |block| block.length_ticks)
}

/// Every sounding note of `pattern`, resolved against `key` for the
/// grid to draw. Green-zone resolution, once per frame: the view carries
/// the finished numbers, and the honest flag that the legacy path is
/// approximating.
pub fn note_views(pattern: &Pattern, key: &Key) -> Vec<NoteView> {
    // Sorted by start so a cell's first note is its earliest.
    let mut views: Vec<NoteView> = note_views_unsorted(pattern, key);
    views.sort_by_key(|view| view.start_ticks);
    views
}

fn note_views_unsorted(pattern: &Pattern, key: &Key) -> Vec<NoteView> {
    (0..PATTERN_STEPS)
        .flat_map(|step| {
            let trig = pattern.trig(step);
            trig.enabled
                .then_some(trig)
                .into_iter()
                .flat_map(move |trig| {
                    trig.notes.iter().map(move |note| {
                        // Where the note actually starts: its step and its
                        // own offset into it, the way the projection plays it.
                        let start = (step * PATTERN_STEP_TICKS)
                            .saturating_add_signed(isize::from(note.micro_ticks));
                        note_view(note, start, trig.probability, trig.enabled, key)
                    })
                })
        })
        .collect()
}

/// One note as the sequencer sees it.
pub fn note_view(
    note: &Note,
    start_ticks: usize,
    probability: f32,
    enabled: bool,
    key: &Key,
) -> NoteView {
    let hz = note.pitch.resolve(key);
    NoteView {
        pitch: note.pitch,
        hz,
        midi: crate::pitch::nearest_midi(hz),
        approx: crate::pitch::cents_from_midi_table(hz).abs() > APPROX_CENTS,
        start_ticks,
        length_ticks: note.length_ticks,
        micro_ticks: note.micro_ticks,
        velocity: note.velocity,
        probability,
        enabled,
    }
}

#[cfg(test)]
mod tests {
    /// The sequencer is frame-independent, which is the entire reason it
    /// was lifted. A frame name here would rebuild the coupling quietly.
    #[test]
    fn no_frame_reaches_into_the_sequencer() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/sequencer");
        for entry in std::fs::read_dir(root).expect("sequencer source dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("sequencer source file");
            // Judge the code, not the prose about the code: the module
            // doc says where this came from, and this test names what it
            // forbids.
            let body = source.split("#[cfg(test)]").next().unwrap_or(&source);
            let code: String = body
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for frame in ["ui::redesign", "ui::stage"] {
                assert!(
                    !code.contains(frame),
                    "{} names a frame ({frame})",
                    path.display()
                );
            }
        }
    }
}
