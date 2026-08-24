//! The piano roll: the bottom region's second face.
//!
//! Shift+Tab swaps the device rack for this editor. It owns no notes: it is
//! a VIEW over the selected clip's `Vec<Note>`, editing that vec in place.
//! What lives here is view state only — where the cursor is, which rung of
//! the grid ladder is set, where the view is scrolled, which notes are
//! selected, what was copied. Select a different clip and the same editor
//! shows different notes; there is nothing to sync, because there is only
//! one copy of a note anywhere in the app.
//!
//! Note positions are CLIP-RELATIVE: beat 0 is the clip's start. Editing a
//! note never resizes the clip, so notes can be pushed past the clip's end —
//! they are drawn dimmed beyond the end rule and do not sound (see
//! `seq_notes` in `main.rs`). Shortening a clip hides notes rather than
//! destroying them.
//!
//! With no clip selected the grid still draws, but the editor is inert:
//! `keys` returns before consuming anything, so the arrows still navigate
//! the rest of the app.
//!
//! Everything the mouse can do the keyboard can do, routed the same way the
//! arrangement's shortcuts are: `keys` runs BEFORE `Focus::begin` and stands
//! down unless the focus ring sat on the roll's cursor cell last frame.
//!
//! The keyboard map, in one place:
//!
//!   arrows            move the cursor (grid step / semitone)
//!   PageUp/PageDown   octave jump
//!   Shift+arrows      extend a box selection from the anchor
//!   Ctrl+Up/Down      transpose the selection a semitone
//!   Ctrl+Left/Right   nudge the selection a grid step — or, with NOTHING
//!                     selected, widen/narrow the roll's own grid (the
//!                     arrangement keeps Ctrl+1/2 for its grid)
//!   Enter or A        add a note at the cursor (grid length, velocity 100)
//!   Delete/Backspace  delete the selection
//!   Ctrl+A            select all;  Escape clears
//!   Ctrl+C/X/V        copy / cut / paste at the cursor beat
//!   Ctrl+D            duplicate the selection directly after itself
//!   [ / ]             shrink / grow selected lengths by a grid step
//!   , / .             velocity -10 / +10
//!
//! Left at beat 0 is NOT claimed, for the same reason the arrangement leaves
//! it: there must always be an arrow that walks back out to the rest of the
//! app.

use daw::ui::theme::Theme;
use daw::ui::tokens::{font, stroke};
use eframe::egui;
use std::collections::HashSet;

use crate::{Clip, Focus, GRID_BEATS, GRID_DEFAULT, GRID_NAMES, Key, Note, claim};
use daw::theory;

// --- geometry, all in logical points -----------------------------------

/// Width of the piano keyboard gutter on the left.
const KEYS_W: f32 = 48.0;
/// Height of one pitch row.
const ROW_H: f32 = 14.0;
/// Height of the velocity lane along the bottom.
const VEL_H: f32 = 64.0;
/// Horizontal zoom — the arrangement's, so a beat is the same width in both.
const PX_PER_BEAT: f32 = 24.0;

/// Zoom bounds. The floor keeps a note clickable; the ceiling stops a
/// single beat from filling the window.
const PX_PER_BEAT_MIN: f32 = 4.0;
const PX_PER_BEAT_MAX: f32 = 400.0;
const ROW_H_MIN: f32 = 5.0;
const ROW_H_MAX: f32 = 40.0;
/// One notch of zoom, as a ratio. Multiplicative, not additive: zooming
/// out then back in must land where it started.
const ZOOM_STEP: f32 = 1.15;

/// How big a beat and a semitone are on screen. Carried together because
/// every piece of the roll's geometry needs both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    pub px_per_beat: f32,
    pub row_h: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            px_per_beat: PX_PER_BEAT,
            row_h: ROW_H,
        }
    }
}

impl Zoom {
    /// Step both axes by whole notches — what the palette verbs call.
    pub fn stepped(self, notches: i32) -> Self {
        let f = ZOOM_STEP.powi(notches);
        self.scaled(f, f)
    }

    /// Scale one or both axes by `factor`, clamped. Returns whether
    /// anything actually moved — at a limit, a zoom key should not also
    /// scroll the view to compensate for a change that did not happen.
    fn scaled(self, time: f32, pitch: f32) -> Self {
        Self {
            px_per_beat: (self.px_per_beat * time).clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX),
            row_h: (self.row_h * pitch).clamp(ROW_H_MIN, ROW_H_MAX),
        }
    }
}
/// Width of a note's right-edge grab zone, the resize handle.
const EDGE_W: f32 = 6.0;
/// Below this spacing subdivision lines are dropped, same as the arrangement.
const GRID_MIN_PX: f32 = 5.0;
/// Padding for the grid-name label and the C labels in the gutter.
const LABEL_PAD: f32 = 4.0;
/// Width of one velocity bar, and how close a pointer must land to claim it.
const VEL_BAR_W: f32 = 3.0;
const VEL_PICK_PX: f32 = 4.0;
/// Air above a full-velocity bar, so the tallest bar still reads as a bar.
const VEL_LANE_PAD: f32 = 4.0;

/// The panel's drag range. Far taller than the rack's: editing notes wants
/// vertical room the way a rack of knobs never does.
pub const H_RANGE: std::ops::RangeInclusive<f32> = 120.0..=900.0;
/// Starting height when the roll is first shown.
pub const DEFAULT_H: f32 = 360.0;

// --- the note domain ----------------------------------------------------

/// Top of the MIDI pitch range; there are `PITCH_MAX + 1` rows.
const PITCH_MAX: u8 = 127;
/// Middle C, the pitch the view centres on when first opened.
const C4: u8 = 60;
/// What a new note is struck at.
/// How strongly an out-of-key row is veiled. Enough to recede, not so much
/// that a chromatic note becomes invisible — accidentals are music, not
/// mistakes.
const OFF_SCALE_VEIL: f32 = 0.55;

const VELOCITY_DEFAULT: u8 = 100;
/// One press of , or .
const VELOCITY_STEP: i32 = 10;
/// Velocity floor: 0 is a note-off in MIDI, so editing never produces it.
const VELOCITY_MIN: u8 = 1;
/// How much velocity shows in a note's fill: full velocity is the theme's
/// own clip colour, silence-adjacent fades to this fraction of it.
const VEL_ALPHA_FLOOR: f32 = 0.55;

// --- accelerated scrolling ----------------------------------------------

/// Scroll events closer together than this feed the accelerator.
const ACCEL_WINDOW: f64 = 0.25;
/// Each event inside the window multiplies the factor by this...
const ACCEL_GROWTH: f32 = 1.25;
/// ...up to here. 128 rows is a long way; a pause resets to 1x.
const ACCEL_MAX: f32 = 4.0;

/// How far the wash past the clip's end knocks colour back. Notes out there
/// are still drawn and still editable — they simply do not sound, and a clip
/// lengthened over them brings them back.
const PAST_END_DIM: f32 = 0.5;

/// What the roll shows when no clip is selected.
const NO_CLIP: &str = "no clip selected — double-click a lane to make one";

/// A drag in flight. All positions are remembered from the press, so every
/// frame recomputes from the origin — no per-frame deltas to drift.
enum Drag {
    /// Moving a note's body: which note, and where it and the pointer were.
    Move {
        idx: usize,
        pitch0: u8,
        start0: f64,
        press: egui::Pos2,
    },
    /// Pulling a note's right edge.
    Resize {
        idx: usize,
        len0: f64,
        press: egui::Pos2,
    },
    /// Rubber-band selection from an empty press.
    Box { press: egui::Pos2 },
    /// Painting velocities: the notes claimed at the press.
    Velocity { targets: Vec<usize> },
}

/// The piano roll's VIEW state. The notes it edits belong to the selected
/// clip; nothing here outlives the clip except the clipboard and the view.
pub struct PianoRoll {
    /// Indices into the SHOWN clip's notes. Cleared whenever the shown clip
    /// changes — an index into one clip means nothing in another.
    pub selected: HashSet<usize>,
    /// Copied notes, starts relative to the earliest — paste re-anchors.
    /// Survives a clip change on purpose: copying between clips is the
    /// point of a clipboard.
    pub clipboard: Vec<Note>,
    /// The keyboard cursor: a pitch and a beat, one grid cell.
    pub cursor_pitch: u8,
    pub cursor_beat: f64,
    /// Index into the shared `GRID_BEATS` ladder — the roll's own rung,
    /// independent of the arrangement's.
    pub grid: usize,
    /// View offsets: first beat at the grid's left edge, and pixels of
    /// content scrolled above its top edge.
    pub scroll_beats: f32,
    pub scroll_y: f32,
    /// How big a beat and a semitone are drawn. View state, not content.
    pub zoom: Zoom,
    /// Set once the first frame has centred the view on C4 — centring needs
    /// the panel's height, which does not exist until the panel does.
    centered: bool,
    /// Set while drawing when the ring is on the cursor cell; read next
    /// frame to decide whether `keys` claims the keyboard.
    pub owns_keys: bool,
    /// The clip these indices and this cursor belong to. `follow_clip`
    /// compares it against the selection and resets when they disagree.
    shown_clip: Option<u64>,
    /// Where a Shift+arrow selection grows from. Plain movement clears it.
    anchor: Option<(u8, f64)>,
    /// Accelerated scrolling: when the last event landed, and the factor it
    /// had earned.
    last_scroll: f64,
    accel: f32,
    drag: Option<Drag>,
}

impl Default for PianoRoll {
    fn default() -> Self {
        Self {
            selected: HashSet::new(),
            clipboard: Vec::new(),
            cursor_pitch: C4,
            cursor_beat: 0.0,
            grid: GRID_DEFAULT,
            scroll_beats: 0.0,
            scroll_y: 0.0,
            zoom: Zoom::default(),
            centered: false,
            owns_keys: false,
            shown_clip: None,
            anchor: None,
            last_scroll: f64::NEG_INFINITY,
            accel: 1.0,
            drag: None,
        }
    }
}

// --- pure arithmetic: snapping, hit tests, edits ------------------------

/// Snap to the nearest grid line, never before beat 0.
fn snap(beat: f64, grid: f64) -> f64 {
    if grid <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / grid).round() * grid).max(0.0)
}

/// Snap DOWN to the cell a point is inside — what a double-click means: the
/// note goes in the cell you clicked, not the nearer boundary.
fn snap_floor(beat: f64, grid: f64) -> f64 {
    if grid <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / grid).floor() * grid).max(0.0)
}

/// Notes intersecting a box: pitches inclusive, beats half-open, so a note
/// merely touching the box's left edge with its end is not swept up.
fn notes_in_box(notes: &[Note], b0: f64, b1: f64, p0: u8, p1: u8) -> Vec<usize> {
    let (b_lo, b_hi) = if b0 <= b1 { (b0, b1) } else { (b1, b0) };
    let (p_lo, p_hi) = if p0 <= p1 { (p0, p1) } else { (p1, p0) };
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            (p_lo..=p_hi).contains(&n.pitch) && n.start < b_hi && n.start + n.len > b_lo
        })
        .map(|(i, _)| i)
        .collect()
}

/// The selected notes, re-anchored so the earliest starts at 0 — what the
/// clipboard holds, so paste places the block wherever the cursor is.
fn clipboard_of(notes: &[Note], selected: &HashSet<usize>) -> Vec<Note> {
    let base = selected
        .iter()
        .filter_map(|&i| notes.get(i))
        .map(|n| n.start)
        .fold(f64::INFINITY, f64::min);
    if !base.is_finite() {
        return Vec::new();
    }
    let mut out: Vec<Note> = selected
        .iter()
        .filter_map(|&i| notes.get(i))
        .map(|n| Note {
            start: n.start - base,
            ..n.clone()
        })
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    out
}

/// How far Ctrl+D shifts the copies: the selection's full span, so the
/// duplicate starts exactly where the original block ends — Ableton's rule.
fn duplicate_offset(notes: &[Note], selected: &HashSet<usize>) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &i in selected {
        if let Some(n) = notes.get(i) {
            lo = lo.min(n.start);
            hi = hi.max(n.start + n.len);
        }
    }
    if lo.is_finite() && hi > lo {
        hi - lo
    } else {
        0.0
    }
}

/// A pitch moved by `delta` semitones, pinned to the MIDI range.
fn transposed(pitch: u8, delta: i32) -> u8 {
    (i32::from(pitch) + delta).clamp(0, i32::from(PITCH_MAX)) as u8
}

/// A velocity nudged by `delta`, pinned to 1..=127 — 0 is a note-off in
/// MIDI, so editing can never produce it.
fn nudged_velocity(velocity: u8, delta: i32) -> u8 {
    (i32::from(velocity) + delta).clamp(i32::from(VELOCITY_MIN), i32::from(PITCH_MAX)) as u8
}

/// The scroll accelerator: events inside the window grow the factor, a
/// pause resets it. Pure, so the ramp is checkable without a wheel.
fn accel_step(factor: f32, since_last: f64) -> f32 {
    if since_last < ACCEL_WINDOW {
        (factor * ACCEL_GROWTH).min(ACCEL_MAX)
    } else {
        1.0
    }
}

// --- coordinates: beats/pitches <-> pixels ------------------------------

fn x_at(grid: egui::Rect, scroll_beats: f32, beat: f64, z: Zoom) -> f32 {
    grid.left() + (beat as f32 - scroll_beats) * z.px_per_beat
}

fn beat_at(grid: egui::Rect, scroll_beats: f32, x: f32, z: Zoom) -> f64 {
    f64::from(scroll_beats + (x - grid.left()) / z.px_per_beat).max(0.0)
}

/// The top of a pitch's row. Pitch 127 is row 0 — high notes on top.
fn row_top(grid: egui::Rect, scroll_y: f32, pitch: u8, z: Zoom) -> f32 {
    grid.top() + f32::from(PITCH_MAX - pitch) * z.row_h - scroll_y
}

/// The pitch whose row contains `y`, if any is there.
fn pitch_at(grid: egui::Rect, scroll_y: f32, y: f32, z: Zoom) -> Option<u8> {
    let row = ((y - grid.top() + scroll_y) / z.row_h).floor() as i32;
    if (0..=i32::from(PITCH_MAX)).contains(&row) {
        Some(PITCH_MAX - row as u8)
    } else {
        None
    }
}

/// A note's rect in the grid.
fn note_rect(grid: egui::Rect, scroll_beats: f32, scroll_y: f32, n: &Note, z: Zoom) -> egui::Rect {
    let y = row_top(grid, scroll_y, n.pitch, z);
    egui::Rect::from_min_max(
        egui::pos2(x_at(grid, scroll_beats, n.start, z), y),
        egui::pos2(x_at(grid, scroll_beats, n.start + n.len, z), y + z.row_h),
    )
}

/// Sharps and flats — the rows drawn darker, like the keys they mirror.
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// "C4" for 60 — the octave convention where middle C is C4.
fn pitch_name(pitch: u8) -> String {
    format!("C{}", i32::from(pitch) / 12 - 1)
}

// --- editing verbs, keyboard and mouse alike ----------------------------

impl PianoRoll {
    pub fn grid_beats(&self) -> f64 {
        f64::from(GRID_BEATS[self.grid.min(GRID_BEATS.len() - 1)])
    }

    /// Which clip's notes the indices and cursor refer to. Called once a
    /// frame BEFORE `keys`, so a stale index from the previous clip can
    /// never reach an edit: a Delete right after switching clips must not
    /// delete note 3 of whatever is showing now.
    ///
    /// The clipboard and the view (scroll, grid rung) survive on purpose —
    /// they are the user's working context, not the clip's.
    pub fn follow_clip(&mut self, id: Option<u64>) {
        if self.shown_clip == id {
            return;
        }
        self.shown_clip = id;
        self.selected.clear();
        self.anchor = None;
        self.drag = None;
        self.cursor_beat = 0.0;
    }

    /// Add a note at the cursor cell and select it — Enter, 'a', and the
    /// double-click all land here so the three cannot disagree.
    fn add_note_at(&mut self, notes: &mut Vec<Note>, pitch: u8, start: f64) {
        notes.push(Note {
            pitch,
            start,
            len: self.grid_beats(),
            vel: VELOCITY_DEFAULT,
        });
        self.selected.clear();
        self.selected.insert(notes.len() - 1);
    }

    /// Remove one note. Selection indices past it shift down by one.
    fn remove_note(&mut self, notes: &mut Vec<Note>, idx: usize) {
        notes.remove(idx);
        self.selected = self
            .selected
            .iter()
            .filter(|&&i| i != idx)
            .map(|&i| if i > idx { i - 1 } else { i })
            .collect();
    }

    fn delete_selected(&mut self, notes: &mut Vec<Note>) {
        let selected = std::mem::take(&mut self.selected);
        let mut i = 0usize;
        notes.retain(|_| {
            let dead = selected.contains(&i);
            i += 1;
            !dead
        });
    }

    fn transpose_selected(&mut self, notes: &mut [Note], delta: i32) {
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.pitch = transposed(n.pitch, delta);
            }
        }
    }

    fn nudge_selected(&mut self, notes: &mut [Note], delta: f64) {
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.start = (n.start + delta).max(0.0);
            }
        }
    }

    /// Grow or shrink selected lengths by a grid step, one step minimum.
    fn resize_selected(&mut self, notes: &mut [Note], delta: f64) {
        let min = self.grid_beats();
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.len = (n.len + delta).max(min);
            }
        }
    }

    fn velocity_selected(&mut self, notes: &mut [Note], delta: i32) {
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.vel = nudged_velocity(n.vel, delta);
            }
        }
    }

    fn copy_selected(&mut self, notes: &[Note]) {
        if !self.selected.is_empty() {
            self.clipboard = clipboard_of(notes, &self.selected);
        }
    }

    /// Paste the clipboard's block at the cursor beat, pitches unchanged —
    /// the cursor's pitch names a row, not a transposition. The pasted
    /// notes become the selection, ready for a nudge or transpose.
    fn paste_at_cursor(&mut self, notes: &mut Vec<Note>) {
        if self.clipboard.is_empty() {
            return;
        }
        let at = self.cursor_beat;
        self.selected.clear();
        for n in self.clipboard.clone() {
            notes.push(Note {
                start: at + n.start,
                ..n
            });
            self.selected.insert(notes.len() - 1);
        }
    }

    /// Ctrl+D: copies of the selection, shifted by its own span, selected.
    fn duplicate_selected(&mut self, notes: &mut Vec<Note>) {
        let offset = duplicate_offset(notes, &self.selected);
        if offset <= 0.0 {
            return;
        }
        let copies: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i))
            .map(|n| Note {
                start: n.start + offset,
                ..n.clone()
            })
            .collect();
        self.selected.clear();
        for c in copies {
            notes.push(c);
            self.selected.insert(notes.len() - 1);
        }
    }

    /// Shift+arrow: drop the anchor where the cursor IS (unless one is
    /// already down), move the cursor, then select everything in the box
    /// between the anchor cell and the cursor cell, both inclusive.
    fn extend_to(&mut self, notes: &[Note], d_pitch: i32, d_beat: f64) {
        let (ap, ab) = *self
            .anchor
            .get_or_insert((self.cursor_pitch, self.cursor_beat));
        self.cursor_pitch = transposed(self.cursor_pitch, d_pitch);
        self.cursor_beat = (self.cursor_beat + d_beat).max(0.0);
        let grid = self.grid_beats();
        let (b_lo, b_hi) = if ab <= self.cursor_beat {
            (ab, self.cursor_beat + grid)
        } else {
            (self.cursor_beat, ab + grid)
        };
        self.selected = notes_in_box(notes, b_lo, b_hi, ap, self.cursor_pitch)
            .into_iter()
            .collect();
    }
}

/// The roll's shortcuts, mirroring `arrangement_keys`: runs before
/// `Focus::begin`, stands down while a text field types or while the ring is
/// elsewhere. The full map is in the module docs.
///
/// With no clip selected this returns having consumed NOTHING — an editor
/// with nothing to edit must not swallow the arrows the rest of the app
/// navigates by.
pub fn keys(ctx: &egui::Context, pr: &mut PianoRoll, notes: Option<&mut Vec<Note>>) {
    if ctx.egui_wants_keyboard_input() || !pr.owns_keys {
        return;
    }
    let Some(notes) = notes else {
        return;
    };
    let grid = pr.grid_beats();
    let at_start = pr.cursor_beat <= 0.0;
    let has_selection = !pr.selected.is_empty();
    ctx.input_mut(|i| {
        use egui::{Key, Modifiers};

        // Ctrl first, then Shift, then plain — `consume_key` matches
        // modifiers logically, so the most specific binding must go first
        // or a plain-arrow check swallows Ctrl+arrow. Same warning as in
        // `arrangement_keys`.
        if i.consume_key(Modifiers::COMMAND, Key::ArrowUp) {
            pr.transpose_selected(notes, 1);
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowDown) {
            pr.transpose_selected(notes, -1);
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowLeft) {
            // With notes selected this nudges them; with nothing selected
            // the same chord steps the roll's grid ALONE. Ctrl+1/2 moves
            // both grids together; this is the way to make the roll finer
            // than the arrangement without dragging the arrangement with
            // it.
            if has_selection {
                pr.nudge_selected(notes, -grid);
            } else {
                pr.grid = pr.grid.saturating_sub(1);
            }
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowRight) {
            if has_selection {
                pr.nudge_selected(notes, grid);
            } else {
                pr.grid = (pr.grid + 1).min(GRID_BEATS.len() - 1);
            }
        } else if i.consume_key(Modifiers::SHIFT, Key::ArrowUp) {
            pr.extend_to(notes, 1, 0.0);
        } else if i.consume_key(Modifiers::SHIFT, Key::ArrowDown) {
            pr.extend_to(notes, -1, 0.0);
        } else if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
            pr.extend_to(notes, 0, -grid);
        } else if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
            pr.extend_to(notes, 0, grid);
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            pr.cursor_pitch = transposed(pr.cursor_pitch, 1);
            pr.anchor = None;
        } else if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            pr.cursor_pitch = transposed(pr.cursor_pitch, -1);
            pr.anchor = None;
        } else if !at_start && i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            // At beat 0 Left is NOT claimed — the way back out.
            pr.cursor_beat = (pr.cursor_beat - grid).max(0.0);
            pr.anchor = None;
        } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            pr.cursor_beat += grid;
            pr.anchor = None;
        }

        if i.consume_key(Modifiers::NONE, Key::PageUp) {
            pr.cursor_pitch = transposed(pr.cursor_pitch, 12);
        }
        if i.consume_key(Modifiers::NONE, Key::PageDown) {
            pr.cursor_pitch = transposed(pr.cursor_pitch, -12);
        }
        if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::A) {
            let (pitch, beat) = (pr.cursor_pitch, pr.cursor_beat);
            pr.add_note_at(notes, pitch, beat);
        }
        if i.consume_key(Modifiers::NONE, Key::Delete)
            || i.consume_key(Modifiers::NONE, Key::Backspace)
        {
            pr.delete_selected(notes);
        }
        if i.consume_key(Modifiers::COMMAND, Key::A) {
            pr.selected = (0..notes.len()).collect();
        }
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            pr.selected.clear();
            pr.anchor = None;
        }
        if i.consume_key(Modifiers::COMMAND, Key::C) {
            pr.copy_selected(notes);
        }
        if i.consume_key(Modifiers::COMMAND, Key::X) {
            pr.copy_selected(notes);
            pr.delete_selected(notes);
        }
        if i.consume_key(Modifiers::COMMAND, Key::V) {
            pr.paste_at_cursor(notes);
        }
        if i.consume_key(Modifiers::COMMAND, Key::D) {
            pr.duplicate_selected(notes);
        }
        if i.consume_key(Modifiers::NONE, Key::OpenBracket) {
            pr.resize_selected(notes, -grid);
        }
        if i.consume_key(Modifiers::NONE, Key::CloseBracket) {
            pr.resize_selected(notes, grid);
        }
        if i.consume_key(Modifiers::NONE, Key::Comma) {
            pr.velocity_selected(notes, -VELOCITY_STEP);
        }
        if i.consume_key(Modifiers::NONE, Key::Period) {
            pr.velocity_selected(notes, VELOCITY_STEP);
        }
    });
}

/// Zoom gestures, read once per frame before anything is measured.
///
/// Ctrl+wheel zooms TIME, Ctrl+Shift+wheel zooms PITCH, and `+`/`-` do the
/// same from the keyboard. Both are anchored: the beat and the pitch under
/// the pointer (or under the cursor, for the keys) stay put while the scale
/// changes around them. An unanchored zoom throws away your place, which is
/// the difference between zooming and being teleported.
fn zoom_input(ui: &egui::Ui, pr: &mut PianoRoll, grid: egui::Rect) {
    let (mods, wheel) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y));
    let mut time = 1.0f32;
    let mut pitch = 1.0f32;

    if mods.ctrl && wheel != 0.0 && ui.rect_contains_pointer(grid) {
        // One notch per ~50px of wheel, in the same ratio steps as the keys.
        let notches = wheel / 50.0;
        if mods.shift {
            pitch = ZOOM_STEP.powf(notches);
        } else {
            time = ZOOM_STEP.powf(notches);
        }
        // Spend it here: the roll's own scroll must not also consume it.
        ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
    }

    if pr.owns_keys {
        ui.ctx().input_mut(|i| {
            for (key, factor) in [
                (egui::Key::Plus, ZOOM_STEP),
                (egui::Key::Equals, ZOOM_STEP),
                (egui::Key::Minus, 1.0 / ZOOM_STEP),
            ] {
                // Shift+key zooms the pitch axis, plain zooms time.
                if i.consume_key(egui::Modifiers::SHIFT, key) {
                    pitch *= factor;
                } else if i.consume_key(egui::Modifiers::NONE, key) {
                    time *= factor;
                }
            }
        });
    }

    if time == 1.0 && pitch == 1.0 {
        return;
    }

    // Anchor: whatever sits under the pointer, else under the cursor.
    let anchor = ui.ctx().pointer_latest_pos().filter(|p| grid.contains(*p));
    let old = pr.zoom;
    let new = old.scaled(time, pitch);
    if new == old {
        return;
    }

    let (ax, ay) = match anchor {
        Some(p) => (p.x, p.y),
        None => (
            x_at(grid, pr.scroll_beats, pr.cursor_beat, old).clamp(grid.left(), grid.right()),
            row_top(grid, pr.scroll_y, pr.cursor_pitch, old).clamp(grid.top(), grid.bottom()),
        ),
    };
    let beat = beat_at(grid, pr.scroll_beats, ax, old);
    // Rows measured from the top of the pitch space, in row units.
    let row_units = (ay - grid.top() + pr.scroll_y) / old.row_h;

    pr.zoom = new;
    pr.scroll_beats = (beat as f32 - (ax - grid.left()) / new.px_per_beat).max(0.0);
    pr.scroll_y = (row_units * new.row_h - (ay - grid.top())).max(0.0);
}

/// Where the three areas sit: keyboard gutter, note grid, velocity lane.
/// Pure, so the split is checkable without a window.
fn layout(area: egui::Rect) -> (egui::Rect, egui::Rect, egui::Rect) {
    let lane_top = (area.bottom() - VEL_H).max(area.top());
    let keys = egui::Rect::from_min_max(area.min, egui::pos2(area.left() + KEYS_W, lane_top));
    let grid = egui::Rect::from_min_max(
        egui::pos2(keys.right(), area.top()),
        egui::pos2(area.right(), lane_top),
    );
    let lane = egui::Rect::from_min_max(egui::pos2(keys.right(), lane_top), area.max);
    (keys, grid, lane)
}

/// The whole editor, over `clip`'s notes. Mirrors `arrangement_body`'s
/// shape: interact first so edits land the frame they are made, paint after,
/// register the cursor cell with `focus` last.
///
/// `clip` is the arrangement's selected clip — the notes are edited IN it,
/// not copied out. With `None` the grid still draws and still scrolls, but
/// nothing is editable and the empty state says why.
pub fn body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    pr: &mut PianoRoll,
    beats_per_bar: u32,
    clip: Option<&mut Clip>,
    key: Key,
) {
    let area = ui.max_rect();
    claim(ui);
    let (keys_rect, grid, lane) = layout(area);
    if grid.width() <= 0.0 || grid.height() <= 0.0 {
        return;
    }

    // Zoom, applied before anything is measured: every rect below derives
    // from it, so a mid-frame change would draw one frame at two scales.
    zoom_input(ui, pr, grid);
    let z = pr.zoom;

    // First show: centre the view on middle C. Needs the panel's height,
    // which is why it cannot happen in `Default`.
    let content_h = f32::from(PITCH_MAX) * ROW_H + ROW_H;
    if !pr.centered {
        pr.scroll_y = (f32::from(PITCH_MAX - C4) * ROW_H - grid.height() * 0.5).max(0.0);
        pr.centered = true;
    }

    // --- accelerated scrolling ------------------------------------------
    // Wheel y scrolls pitch, wheel x (or Shift+wheel, which egui maps onto
    // x) scrolls time. Events in quick succession grow a multiplier, so a
    // flick crosses octaves while a lone tick still moves one row's worth.
    let scroll = ui.input(|i| i.smooth_scroll_delta);
    if scroll != egui::Vec2::ZERO && ui.rect_contains_pointer(area) {
        let now = ui.input(|i| i.time);
        pr.accel = accel_step(pr.accel, now - pr.last_scroll);
        pr.last_scroll = now;
        pr.scroll_y -= scroll.y * pr.accel;
        pr.scroll_beats -= scroll.x * pr.accel / PX_PER_BEAT;
    }
    pr.scroll_y = pr.scroll_y.clamp(0.0, (content_h - grid.height()).max(0.0));
    pr.scroll_beats = pr.scroll_beats.max(0.0);

    let grid_beats = pr.grid_beats();
    let (sb, sy) = (pr.scroll_beats, pr.scroll_y);

    // The clip, taken apart once: its notes are what everything below edits
    // and draws, its length is where the extent rule goes.
    let (mut notes, clip_len) = match clip {
        Some(c) => (Some(&mut c.notes), Some(f64::from(c.len))),
        None => (None, None),
    };

    // --- interact, before any painting ------------------------------------
    let grid_id = ui.id().with("pr_grid");
    let resp = ui.interact(grid, grid_id, egui::Sense::click_and_drag());
    let command = ui.input(|i| i.modifiers.command);

    // What the pointer is on: Some((idx, on_right_edge)). A fn, not a
    // closure, so its borrow of the notes ends at each call site and the
    // edits below stay free to mutate them.
    fn hit_note(
        notes: &[Note],
        grid: egui::Rect,
        sb: f32,
        sy: f32,
        pos: egui::Pos2,
        z: Zoom,
    ) -> Option<(usize, bool)> {
        // Later notes draw on top, so hit-test back to front.
        for (i, n) in notes.iter().enumerate().rev() {
            let r = note_rect(grid, sb, sy, n, z);
            if r.contains(pos) {
                return Some((i, pos.x >= r.right() - EDGE_W));
            }
        }
        None
    }

    // The box drawn this frame, resolved during the drag match, painted
    // after the notes so it washes over them.
    let mut band: Option<egui::Rect> = None;
    // Every edit below needs notes to edit: with no clip selected the
    // grid is a picture, not an editor.
    if let Some(notes) = notes.as_deref_mut() {
        if resp.double_clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            match hit_note(notes, grid, sb, sy, pos, z) {
                // Double-click a note: delete it.
                Some((idx, _)) => pr.remove_note(notes, idx),
                // Double-click empty: a note in the clicked cell.
                None => {
                    if let Some(pitch) = pitch_at(grid, sy, pos.y, z) {
                        let beat = snap_floor(beat_at(grid, sb, pos.x, z), grid_beats);
                        pr.add_note_at(notes, pitch, beat);
                        pr.cursor_pitch = pitch;
                        pr.cursor_beat = beat;
                    }
                }
            }
        } else if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            match hit_note(notes, grid, sb, sy, pos, z) {
                Some((idx, _)) => {
                    // Ctrl+click toggles membership; a plain click replaces.
                    if command {
                        if !pr.selected.remove(&idx) {
                            pr.selected.insert(idx);
                        }
                    } else {
                        pr.selected.clear();
                        pr.selected.insert(idx);
                    }
                    pr.cursor_pitch = notes[idx].pitch;
                    pr.cursor_beat = notes[idx].start;
                }
                None => {
                    if !command {
                        pr.selected.clear();
                    }
                    if let Some(pitch) = pitch_at(grid, sy, pos.y, z) {
                        pr.cursor_pitch = pitch;
                        pr.cursor_beat = snap_floor(beat_at(grid, sb, pos.x, z), grid_beats);
                    }
                }
            }
            pr.anchor = None;
        }

        if resp.drag_started()
            && let Some(press) = resp.interact_pointer_pos()
        {
            pr.drag = Some(match hit_note(notes, grid, sb, sy, press, z) {
                Some((idx, true)) => Drag::Resize {
                    idx,
                    len0: notes[idx].len,
                    press,
                },
                Some((idx, false)) => {
                    if !pr.selected.contains(&idx) {
                        pr.selected.clear();
                        pr.selected.insert(idx);
                    }
                    Drag::Move {
                        idx,
                        pitch0: notes[idx].pitch,
                        start0: notes[idx].start,
                        press,
                    }
                }
                None => Drag::Box { press },
            });
        }
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            // Matching the place directly is fine: every bound field is Copy,
            // so nothing moves out of `pr.drag`.
            match pr.drag {
                Some(Drag::Move {
                    idx,
                    pitch0,
                    start0,
                    press,
                }) => {
                    let rows = ((pos.y - press.y) / ROW_H).round() as i32;
                    let want = start0 + f64::from((pos.x - press.x) / PX_PER_BEAT);
                    if let Some(n) = notes.get_mut(idx) {
                        n.pitch = transposed(pitch0, -rows);
                        n.start = snap(want, grid_beats);
                    }
                }
                Some(Drag::Resize { idx, len0, press }) => {
                    let want = len0 + f64::from((pos.x - press.x) / PX_PER_BEAT);
                    if let Some(n) = notes.get_mut(idx) {
                        n.len = snap(want, grid_beats).max(grid_beats);
                    }
                }
                Some(Drag::Box { press }) => {
                    band = Some(egui::Rect::from_two_pos(press, pos));
                    let (b0, b1) = (beat_at(grid, sb, press.x, z), beat_at(grid, sb, pos.x, z));
                    let p0 = pitch_at(grid, sy, press.y.clamp(grid.top(), grid.bottom() - 1.0), z);
                    let p1 = pitch_at(grid, sy, pos.y.clamp(grid.top(), grid.bottom() - 1.0), z);
                    if let (Some(p0), Some(p1)) = (p0, p1) {
                        pr.selected = notes_in_box(notes, b0, b1, p0, p1).into_iter().collect();
                    }
                }
                _ => {}
            }
        }
        if resp.drag_stopped() {
            pr.drag = None;
        }
        if resp.dragged() && matches!(pr.drag, Some(Drag::Move { .. })) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if resp
            .hover_pos()
            .and_then(|p| hit_note(notes, grid, sb, sy, p, z))
            .is_some_and(|(_, edge)| edge)
            || matches!(pr.drag, Some(Drag::Resize { .. }))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    // --- paint -------------------------------------------------------------
    let painter = ui.painter().with_clip_rect(grid);
    painter.rect_filled(grid, 0.0, theme.surface);

    // Pitch rows: black-key rows recessed, an octave rule under every C.
    let lo = pitch_at(grid, sy, grid.bottom() - 1.0, z).unwrap_or(0);
    let hi = pitch_at(grid, sy, grid.top(), z).unwrap_or(PITCH_MAX);
    for pitch in lo..=hi {
        let y = row_top(grid, sy, pitch, z);
        if is_black_key(pitch) {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(grid.left(), y),
                    egui::vec2(grid.width(), ROW_H),
                ),
                0.0,
                theme.surface_sunken,
            );
        }
        // Rows OUTSIDE the working key are veiled rather than the in-key
        // rows being lit: the scale should read as the ground you write on,
        // not as decoration laid over it. The tonic keeps a brighter rule
        // so the key has a visible home row.
        if !key.scale.contains(key.tonic, pitch) {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(grid.left(), y),
                    egui::vec2(grid.width(), ROW_H),
                ),
                0.0,
                theme.bg.gamma_multiply(OFF_SCALE_VEIL),
            );
        } else if i16::from(pitch).rem_euclid(12) == i16::from(key.tonic).rem_euclid(12) {
            painter.line_segment(
                [
                    egui::pos2(grid.left(), y + ROW_H),
                    egui::pos2(grid.right(), y + ROW_H),
                ],
                egui::Stroke::new(stroke::HAIR, theme.accent_muted),
            );
        }
        if pitch % 12 == 0 {
            painter.line_segment(
                [
                    egui::pos2(grid.left(), y + ROW_H),
                    egui::pos2(grid.right(), y + ROW_H),
                ],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
    }

    // Beat grid: bars, beats, subdivisions — the arrangement's three
    // weights, dropped to beats when the subdivision would smear.
    let sub = grid_beats as f32;
    let step = if sub * PX_PER_BEAT < GRID_MIN_PX {
        1.0
    } else {
        sub
    };
    let per_bar = beats_per_bar.max(1) as f32;
    let mut beat = (sb / step).floor() * step;
    loop {
        let x = x_at(grid, sb, f64::from(beat), z);
        if x > grid.right() {
            break;
        }
        let on_bar = (beat % per_bar).abs() < 1e-3;
        let on_beat = beat.fract().abs() < 1e-3;
        let colour = if on_bar {
            theme.grid_bar
        } else if on_beat {
            theme.grid_beat
        } else {
            theme.grid_sub
        };
        painter.line_segment(
            [egui::pos2(x, grid.top()), egui::pos2(x, grid.bottom())],
            egui::Stroke::new(stroke::HAIR, colour),
        );
        beat += step;
    }

    // The clip's extent: everything past its end is washed out and ruled
    // off, because notes out there do not sound.
    if let Some(len) = clip_len {
        let end_x = x_at(grid, sb, len, z);
        if end_x < grid.right() {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(end_x.max(grid.left()), grid.top()), grid.max),
                0.0,
                theme.surface_sunken.gamma_multiply(PAST_END_DIM),
            );
        }
        painter.line_segment(
            [
                egui::pos2(end_x, grid.top()),
                egui::pos2(end_x, grid.bottom()),
            ],
            egui::Stroke::new(stroke::HAIR, theme.accent_muted),
        );
    }

    // Notes: theme clip colours, velocity showing in the fill. A note past
    // the clip's end is drawn faded — present, editable, and silent.
    for (i, n) in notes
        .as_deref()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let r = note_rect(grid, sb, sy, n, z);
        if r.right() < grid.left() || r.left() > grid.right() {
            continue;
        }
        let selected = pr.selected.contains(&i);
        let alpha =
            VEL_ALPHA_FLOOR + (1.0 - VEL_ALPHA_FLOOR) * f32::from(n.vel) / f32::from(PITCH_MAX);
        let silent = clip_len.is_some_and(|len| n.start >= len);
        let fill = if selected {
            theme.clip_selected
        } else {
            theme.clip_body
        }
        .gamma_multiply(if silent { alpha * PAST_END_DIM } else { alpha });
        painter.rect_filled(r, daw::ui::tokens::radius::CTRL, fill);
        painter.rect_stroke(
            r,
            daw::ui::tokens::radius::CTRL,
            egui::Stroke::new(stroke::HAIR, theme.outline),
            egui::StrokeKind::Middle,
        );
    }

    // Nothing to edit: say so rather than showing a grid that ignores every
    // click, which reads as broken.
    if notes.is_none() {
        painter.text(
            grid.center(),
            egui::Align2::CENTER_CENTER,
            NO_CLIP,
            egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
            theme.text_muted,
        );
    }

    // The rubber band, over the notes it is sweeping.
    if let Some(b) = band {
        painter.rect_filled(b.intersect(grid), 0.0, theme.loop_region);
        painter.rect_stroke(
            b.intersect(grid),
            0.0,
            egui::Stroke::new(stroke::HAIR, theme.accent_muted),
            egui::StrokeKind::Middle,
        );
    }

    // The grid's name, for the same reason the arrangement shows its own:
    // past GRID_MIN_PX the ladder changes and nothing on screen would move.
    let label_font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    let grid_label = painter.text(
        egui::pos2(grid.right() - LABEL_PAD, grid.top() + LABEL_PAD),
        egui::Align2::RIGHT_TOP,
        GRID_NAMES[pr.grid.min(GRID_NAMES.len() - 1)],
        label_font.clone(),
        theme.text_muted,
    );
    // The working key, beside the grid: the roll shades every row against
    // it, so the reason those rows look that way should be readable here
    // rather than remembered.
    painter.text(
        egui::pos2(grid_label.left() - LABEL_PAD, grid.top() + LABEL_PAD),
        egui::Align2::RIGHT_TOP,
        key.label(),
        label_font,
        theme.text_muted,
    );

    // --- the keyboard gutter ----------------------------------------------
    let painter = ui.painter().with_clip_rect(keys_rect);
    painter.rect_filled(keys_rect, 0.0, theme.surface);
    for pitch in lo..=hi {
        let y = row_top(grid, sy, pitch, z);
        if is_black_key(pitch) {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(keys_rect.left(), y),
                    egui::vec2(keys_rect.width(), ROW_H),
                ),
                0.0,
                theme.surface_sunken,
            );
        }
        if pitch % 12 == 0 {
            painter.text(
                egui::pos2(keys_rect.right() - LABEL_PAD, y + ROW_H * 0.5),
                egui::Align2::RIGHT_CENTER,
                pitch_name(pitch),
                egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                theme.text_muted,
            );
        }
    }
    // The gutter's edge against the grid.
    painter.line_segment(
        [keys_rect.right_top(), keys_rect.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    // --- the velocity lane --------------------------------------------------
    velocity_lane(ui, theme, pr, notes, grid, lane);

    // --- the keyboard cursor -------------------------------------------------
    // Register the cursor CELL, like the arrangement's lanes do: the ring
    // wraps the cell, and its presence is what lets `keys` claim input.
    let cell = egui::Rect::from_min_size(
        egui::pos2(
            x_at(grid, sb, pr.cursor_beat, z),
            row_top(grid, sy, pr.cursor_pitch, z),
        ),
        egui::vec2(grid_beats as f32 * PX_PER_BEAT, ROW_H),
    );
    let wid = ui.id().with("pr_cell");
    pr.owns_keys = focus.register(wid, cell.intersect(grid));
}

/// The velocity lane: one bar per note, draggable. A drag claims its notes
/// at the press — the note under the pointer, or the whole selection if the
/// pointer landed on a selected note's bar — and paints them until release.
fn velocity_lane(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    mut notes: Option<&mut Vec<Note>>,
    grid: egui::Rect,
    lane: egui::Rect,
) {
    let z = pr.zoom;
    let sb = pr.scroll_beats;
    let lane_id = ui.id().with("pr_vel");
    let resp = ui.interact(lane, lane_id, egui::Sense::click_and_drag());

    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        // Nearest bar within reach of the press.
        let mut best: Option<(usize, f32)> = None;
        for (i, n) in notes
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let d = (x_at(grid, sb, n.start, z) - pos.x).abs();
            if d <= VEL_PICK_PX && best.is_none_or(|(_, b)| d < b) {
                best = Some((i, d));
            }
        }
        let targets = match best {
            Some((i, _)) if pr.selected.contains(&i) => pr.selected.iter().copied().collect(),
            Some((i, _)) => vec![i],
            None => Vec::new(),
        };
        pr.drag = Some(Drag::Velocity { targets });
    }
    // Cloned out first, so the borrow of `pr.drag` is over before the
    // notes are written.
    let targets: Vec<usize> = match &pr.drag {
        Some(Drag::Velocity { targets }) => targets.clone(),
        _ => Vec::new(),
    };
    if resp.dragged()
        && !targets.is_empty()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let usable = (lane.height() - VEL_LANE_PAD).max(1.0);
        let frac = ((lane.bottom() - pos.y) / usable).clamp(0.0, 1.0);
        let velocity = nudged_velocity(
            VELOCITY_MIN,
            (frac * f32::from(PITCH_MAX)).round() as i32 - i32::from(VELOCITY_MIN),
        );
        for &i in &targets {
            if let Some(n) = notes.as_deref_mut().and_then(|ns| ns.get_mut(i)) {
                n.vel = velocity;
            }
        }
    }
    if resp.drag_stopped() && matches!(pr.drag, Some(Drag::Velocity { .. })) {
        pr.drag = None;
    }

    let painter = ui.painter().with_clip_rect(lane);
    painter.rect_filled(lane, 0.0, theme.surface_sunken);
    // The rule separating grid from lane.
    painter.line_segment(
        [lane.left_top(), lane.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let usable = (lane.height() - VEL_LANE_PAD).max(1.0);
    for (i, n) in notes
        .as_deref()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let x = x_at(grid, sb, n.start, z);
        if x < lane.left() || x > lane.right() {
            continue;
        }
        let h = usable * f32::from(n.vel) / f32::from(PITCH_MAX);
        let colour = if pr.selected.contains(&i) {
            theme.accent
        } else {
            theme.accent_muted
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x - VEL_BAR_W * 0.5, lane.bottom() - h),
                egui::pos2(x + VEL_BAR_W * 0.5, lane.bottom()),
            ),
            0.0,
            colour,
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::note;
    use egui::{Rect, pos2, vec2};

    /// A roll and the clip's notes it is editing — the pair every verb now
    /// takes, since the roll owns no notes of its own.
    fn roll_with(notes: Vec<Note>) -> (PianoRoll, Vec<Note>) {
        (PianoRoll::default(), notes)
    }

    /// Snapping: nearest for edits, floor for "the cell you clicked", both
    /// pinned at beat 0 and safe against a zero grid.
    #[test]
    fn snapping_lands_on_the_grid() {
        assert_eq!(snap(1.2, 1.0), 1.0);
        assert_eq!(snap(1.6, 1.0), 2.0);
        assert_eq!(snap(1.6, 0.5), 1.5);
        assert_eq!(snap(-3.0, 1.0), 0.0, "snapping cannot go negative");
        assert_eq!(snap(2.7, 0.0), 2.7, "a zero grid must not divide");

        assert_eq!(snap_floor(1.9, 1.0), 1.0, "floor stays in the cell");
        assert_eq!(snap_floor(1.9, 0.5), 1.5);
        assert_eq!(snap_floor(-0.2, 1.0), 0.0);
    }

    /// The box selection hit test: pitches inclusive, beats half-open, and
    /// corner order must not matter — a drag can go any direction.
    #[test]
    fn the_box_selects_what_it_covers() {
        let notes = vec![
            note(60, 0.0, 1.0, 100), // inside
            note(64, 2.0, 1.0, 100), // right of the box
            note(48, 0.5, 1.0, 100), // below the pitch range
            note(62, 1.5, 1.0, 100), // straddles the right edge
        ];
        let hit = notes_in_box(&notes, 0.0, 2.0, 55, 70);
        assert_eq!(hit, vec![0, 3], "inside and straddling are selected");

        // Swapped corners select the same notes.
        assert_eq!(notes_in_box(&notes, 2.0, 0.0, 70, 55), vec![0, 3]);

        // A note ENDING exactly at the box's start is not swept up, and one
        // starting exactly at its end is not either.
        let edge = vec![note(60, 0.0, 1.0, 100), note(60, 3.0, 1.0, 100)];
        assert!(notes_in_box(&edge, 1.0, 3.0, 0, 127).is_empty());
    }

    /// Copy re-anchors to the earliest start; paste places the block at the
    /// cursor beat with its internal offsets intact.
    #[test]
    fn copy_paste_keeps_relative_placement() {
        let (mut pr, mut notes) = roll_with(vec![
            note(60, 4.0, 1.0, 100),
            note(64, 5.5, 0.5, 90),
            note(67, 4.0, 2.0, 80),
        ]);
        pr.selected = [0, 1, 2].into_iter().collect();
        pr.copy_selected(&notes);
        assert_eq!(pr.clipboard.len(), 3);
        assert_eq!(
            pr.clipboard[0].start, 0.0,
            "the earliest note re-anchors to zero"
        );

        pr.cursor_beat = 10.0;
        pr.paste_at_cursor(&mut notes);
        assert_eq!(notes.len(), 6);
        let pasted: Vec<(u8, f64)> = notes[3..].iter().map(|n| (n.pitch, n.start)).collect();
        assert!(pasted.contains(&(60, 10.0)), "block lands at the cursor");
        assert!(pasted.contains(&(64, 11.5)), "offsets survive the trip");
        assert!(pasted.contains(&(67, 10.0)), "pitches are untouched");
        assert_eq!(pr.selected.len(), 3, "the paste becomes the selection");
        assert!(pr.selected.contains(&3) && pr.selected.contains(&5));
        assert!(
            !pr.clipboard.is_empty(),
            "paste must not empty the clipboard"
        );
    }

    /// Ctrl+D: the copies start exactly where the original block ends.
    #[test]
    fn duplicate_lands_directly_after_the_selection() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 2.0, 1.0, 100), note(64, 3.0, 2.0, 90)]);
        pr.selected = [0, 1].into_iter().collect();
        // Span is 2.0..5.0, so the copies shift by 3.
        pr.duplicate_selected(&mut notes);
        assert_eq!(notes.len(), 4);
        let starts: Vec<f64> = notes[2..].iter().map(|n| n.start).collect();
        assert!(starts.contains(&5.0) && starts.contains(&6.0));
        assert_eq!(
            pr.selected,
            [2, 3].into_iter().collect(),
            "the copies are selected, ready for another Ctrl+D"
        );

        // Nothing selected duplicates nothing.
        let (mut empty, mut one) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        empty.duplicate_selected(&mut one);
        assert_eq!(one.len(), 1);
    }

    /// Transposition pins to the MIDI range at both ends.
    #[test]
    fn transposition_clamps_to_midi() {
        assert_eq!(transposed(60, 12), 72);
        assert_eq!(transposed(120, 12), 127, "no pitch above 127");
        assert_eq!(transposed(5, -12), 0, "no pitch below 0");
        assert_eq!(transposed(127, 1), 127);
        assert_eq!(transposed(0, -1), 0);

        let (mut pr, mut notes) = roll_with(vec![note(127, 0.0, 1.0, 100), note(0, 1.0, 1.0, 100)]);
        pr.selected = [0, 1].into_iter().collect();
        pr.transpose_selected(&mut notes, 5);
        assert_eq!(notes[0].pitch, 127);
        assert_eq!(notes[1].pitch, 5);
    }

    /// Velocity clamps to 1..=127 — 0 is a note-off, so editing never
    /// produces it.
    #[test]
    fn velocity_clamps_and_never_reaches_zero() {
        assert_eq!(nudged_velocity(100, 10), 110);
        assert_eq!(nudged_velocity(120, 10), 127);
        assert_eq!(nudged_velocity(5, -10), 1, "1 is the floor, not 0");
        assert_eq!(nudged_velocity(127, 100), 127);

        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 8)]);
        pr.selected.insert(0);
        pr.velocity_selected(&mut notes, -VELOCITY_STEP);
        assert_eq!(notes[0].vel, 1);
        pr.velocity_selected(&mut notes, VELOCITY_STEP * 100);
        assert_eq!(notes[0].vel, 127);
    }

    /// The scroll accelerator grows inside the window, clamps at the max,
    /// and a pause resets it to 1x.
    #[test]
    fn scroll_acceleration_grows_and_decays() {
        let mut f = 1.0;
        // Rapid events ramp the factor...
        for _ in 0..3 {
            let next = accel_step(f, ACCEL_WINDOW * 0.5);
            assert!(next > f, "consecutive scrolls should accelerate");
            f = next;
        }
        // ...but never past the clamp.
        for _ in 0..50 {
            f = accel_step(f, ACCEL_WINDOW * 0.5);
        }
        assert_eq!(f, ACCEL_MAX);
        // A pause longer than the window resets outright.
        assert_eq!(accel_step(f, ACCEL_WINDOW * 2.0), 1.0);
        assert_eq!(accel_step(ACCEL_MAX, f64::INFINITY), 1.0);
    }

    /// Zoom is multiplicative and clamped: out-then-in returns to where it
    /// started, and neither axis can run away.
    #[test]
    fn zoom_steps_are_reversible_and_bounded() {
        let z = Zoom::default();
        let there_and_back = z.scaled(ZOOM_STEP, 1.0).scaled(1.0 / ZOOM_STEP, 1.0);
        assert!(
            (there_and_back.px_per_beat - z.px_per_beat).abs() < 0.01,
            "a zoom in and back out must land where it started"
        );

        // Hammering either direction stops at the limit rather than
        // collapsing the grid or exploding it.
        let mut wide = z;
        let mut tight = z;
        for _ in 0..200 {
            wide = wide.scaled(1.0 / ZOOM_STEP, 1.0 / ZOOM_STEP);
            tight = tight.scaled(ZOOM_STEP, ZOOM_STEP);
        }
        assert_eq!(wide.px_per_beat, PX_PER_BEAT_MIN);
        assert_eq!(wide.row_h, ROW_H_MIN);
        assert_eq!(tight.px_per_beat, PX_PER_BEAT_MAX);
        assert_eq!(tight.row_h, ROW_H_MAX);
    }

    /// Zooming keeps the anchored beat under the same pixel. This is the
    /// property that separates zooming from being teleported, and it is
    /// invisible in a screenshot — so it gets a test.
    #[test]
    fn zooming_holds_the_anchored_beat_still() {
        let grid = Rect::from_min_size(pos2(48.0, 0.0), vec2(800.0, 400.0));
        let old = Zoom::default();
        // Well away from the start, so zooming out still has room to
        // scroll — see the clamp case below.
        let scroll = 200.0f32;
        let ax = grid.left() + grid.width() * 0.66;
        let beat = beat_at(grid, scroll, ax, old);

        for factor in [ZOOM_STEP, 1.0 / ZOOM_STEP, 2.5, 0.4] {
            let new = old.scaled(factor, 1.0);
            // The same arithmetic `zoom_input` uses.
            let new_scroll = (beat as f32 - (ax - grid.left()) / new.px_per_beat).max(0.0);
            let after = beat_at(grid, new_scroll, ax, new);
            assert!(
                (after - beat).abs() < 0.01,
                "beat {beat} drifted to {after} at factor {factor}"
            );
        }
    }

    /// The one case where the anchor CANNOT hold: zooming out near the
    /// start would need to scroll before beat 0. The view pins to 0 instead
    /// of scrolling negative — a timeline has a beginning.
    #[test]
    fn zooming_out_at_the_start_pins_to_zero() {
        let grid = Rect::from_min_size(pos2(48.0, 0.0), vec2(800.0, 400.0));
        let old = Zoom::default();
        let ax = grid.left() + grid.width() * 0.66;
        let beat = beat_at(grid, 3.0, ax, old);
        let new = old.scaled(0.5, 1.0);
        let new_scroll = (beat as f32 - (ax - grid.left()) / new.px_per_beat).max(0.0);
        assert_eq!(new_scroll, 0.0, "never scroll before the first beat");
        assert!(beat_at(grid, new_scroll, grid.left(), new) >= 0.0);
    }

    /// Pitch/pixel round trip, high notes on top.
    #[test]
    fn pitch_maps_upward_and_round_trips() {
        let z = Zoom::default();
        let grid = Rect::from_min_size(pos2(48.0, 100.0), vec2(800.0, 400.0));
        for scroll_y in [0.0, 123.0] {
            for pitch in [0u8, 48, 60, 127] {
                let y = row_top(grid, scroll_y, pitch, z);
                assert_eq!(pitch_at(grid, scroll_y, y + z.row_h * 0.5, z), Some(pitch));
            }
            assert!(
                row_top(grid, scroll_y, 72, z) < row_top(grid, scroll_y, 60, z),
                "C5 must sit above C4"
            );
        }
        // Off both ends of the range is nobody's row.
        assert_eq!(pitch_at(grid, 0.0, grid.top() - 10.0, z), None);
        assert_eq!(
            pitch_at(grid, 0.0, grid.top() + 129.0 * z.row_h, z),
            None,
            "below row 127 is off the keyboard"
        );

        // Beats round-trip too, from any pan.
        for sb in [0.0, 16.0] {
            for beat in [0.0, 1.0, 4.5] {
                let back = beat_at(grid, sb, x_at(grid, sb, beat, z), z);
                assert!((back - beat).abs() < 1e-3);
            }
        }
    }

    /// Deleting the selection reindexes what survives, so a stale index can
    /// never point at the wrong note afterwards.
    #[test]
    fn deletion_reindexes_the_selection() {
        let (mut pr, mut notes) = roll_with(vec![
            note(60, 0.0, 1.0, 100),
            note(62, 1.0, 1.0, 100),
            note(64, 2.0, 1.0, 100),
        ]);
        pr.selected = [1].into_iter().collect();
        pr.delete_selected(&mut notes);
        assert_eq!(notes.len(), 2);
        assert!(pr.selected.is_empty());
        assert_eq!(notes[1].pitch, 64, "the survivor shifted down one slot");

        // remove_note keeps OTHER selections valid across the shift.
        let (mut pr, mut notes) = roll_with(vec![
            note(60, 0.0, 1.0, 100),
            note(62, 1.0, 1.0, 100),
            note(64, 2.0, 1.0, 100),
        ]);
        pr.selected = [0, 2].into_iter().collect();
        pr.remove_note(&mut notes, 0);
        assert_eq!(pr.selected, [1].into_iter().collect());
        assert_eq!(notes[1].pitch, 64);
    }

    /// The keyboard's box selection: anchor holds while the cursor sweeps,
    /// exactly like the mouse rubber band.
    #[test]
    fn shift_arrows_extend_a_box_from_the_anchor() {
        let (mut pr, notes) = roll_with(vec![
            note(60, 0.0, 1.0, 100),
            note(62, 1.0, 1.0, 100),
            note(70, 0.0, 1.0, 100), // out of the pitch sweep
        ]);
        pr.cursor_pitch = 60;
        pr.cursor_beat = 0.0;

        // Sweep right one cell: both cells on the anchor's own pitch.
        pr.extend_to(&notes, 0, pr.grid_beats());
        assert_eq!(pr.selected, [0].into_iter().collect());
        // Then up two semitones: the box now covers both, but never the
        // note at pitch 70 above the sweep.
        pr.extend_to(&notes, 2, 0.0);
        assert_eq!(pr.selected, [0, 1].into_iter().collect());
        assert_eq!(pr.anchor, Some((60, 0.0)), "the anchor must not drift");
    }

    /// Switching clips drops the note selection and the cursor beat, because
    /// index 3 of one clip is a different note in another — and a Delete
    /// straight after a switch must not hit the wrong one. The clipboard and
    /// the view survive: they are the user's context, not the clip's.
    #[test]
    fn a_clip_change_resets_what_belongs_to_the_clip() {
        let (mut pr, _) = roll_with(vec![]);
        pr.clipboard = vec![note(60, 0.0, 1.0, 100)];
        pr.grid = 4;
        pr.scroll_beats = 12.0;

        pr.follow_clip(Some(1));
        pr.selected = [0, 2].into_iter().collect();
        pr.cursor_beat = 7.0;
        pr.anchor = Some((60, 7.0));

        // Same clip: nothing moves.
        pr.follow_clip(Some(1));
        assert_eq!(pr.selected, [0, 2].into_iter().collect());
        assert_eq!(pr.cursor_beat, 7.0);

        // A different clip: indices and cursor go, view and clipboard stay.
        pr.follow_clip(Some(2));
        assert!(pr.selected.is_empty(), "stale indices must not survive");
        assert_eq!(pr.anchor, None);
        assert_eq!(pr.cursor_beat, 0.0);
        assert_eq!(pr.clipboard.len(), 1, "the clipboard crosses clips");
        assert_eq!(pr.grid, 4, "the grid rung is view state");
        assert_eq!(pr.scroll_beats, 12.0, "so is the scroll");

        // Deselecting entirely is a change too.
        pr.selected = [0].into_iter().collect();
        pr.follow_clip(None);
        assert!(pr.selected.is_empty());
    }

    /// Notes belong to the clip, so they survive being looked away from and
    /// come back untouched — there is no second copy to fall out of sync.
    #[test]
    fn notes_survive_a_clip_switch() {
        let mut a = crate::Arrangement::default();
        a.create_clip(0, 0.0, 4.0).unwrap();
        a.create_clip(1, 0.0, 4.0).unwrap();
        let mut pr = PianoRoll::default();

        // Edit clip A through the roll's own verbs.
        a.selected_clip = Some((0, 0));
        pr.follow_clip(a.active_clip_id());
        let first = a.active_clip_id();
        pr.add_note_at(&mut a.active_clip().unwrap().notes, 60, 0.0);
        assert_eq!(a.clips[0][0].notes.len(), 1);

        // Look at clip B, edit it, and A must be untouched.
        a.selected_clip = Some((1, 0));
        pr.follow_clip(a.active_clip_id());
        assert_ne!(a.active_clip_id(), first, "a different clip");
        pr.add_note_at(&mut a.active_clip().unwrap().notes, 72, 1.0);
        pr.add_note_at(&mut a.active_clip().unwrap().notes, 74, 2.0);
        assert_eq!(a.clips[1][0].notes.len(), 2);
        assert_eq!(a.clips[0][0].notes.len(), 1, "clip A kept its note");
        assert_eq!(a.clips[0][0].notes[0].pitch, 60);

        // Back to A: the note is still there, and the selection did not
        // follow us across.
        a.selected_clip = Some((0, 0));
        pr.follow_clip(a.active_clip_id());
        assert!(pr.selected.is_empty());
        assert_eq!(a.active_clip().unwrap().notes[0].pitch, 60);
    }

    /// With no clip selected the roll must not swallow a single key — the
    /// arrows still have to navigate the rest of the app.
    #[test]
    fn no_clip_means_the_keys_pass_through() {
        let ctx = egui::Context::default();
        let mut pr = PianoRoll {
            owns_keys: true,
            ..Default::default()
        };

        let press = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(press, |_ui| {});
        out.textures_delta.clear();

        keys(&ctx, &mut pr, None);
        assert_eq!(pr.cursor_beat, 0.0, "an inert editor must not move");
        assert!(
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)),
            "the key must still be there for whoever navigates next"
        );
    }

    /// The three areas split cleanly: gutter left, lane below, grid the rest.
    #[test]
    fn the_layout_splits_gutter_grid_and_lane() {
        let area = Rect::from_min_size(pos2(0.0, 500.0), vec2(1200.0, 360.0));
        let (keys, grid, lane) = layout(area);
        assert_eq!(keys.width(), KEYS_W);
        assert_eq!(keys.right(), grid.left());
        assert_eq!(grid.bottom(), lane.top());
        assert_eq!(lane.height(), VEL_H);
        assert_eq!(lane.left(), grid.left(), "the lane starts under the grid");
        assert_eq!(grid.right(), area.right());
        assert_eq!(lane.bottom(), area.bottom());
    }
}

// ------------------------------------------------------- generative edits ---
//
// Pure functions over notes: notes in, notes out, no UI and no state. That
// is what makes them testable, and it is what will make them PREVIEWABLE
// (run into a scratch buffer, draw the result) once the palette grows a
// preview pass. Every one is a single undo step by construction, because
// each rewrites the note list once.

/// Write `quality` rooted at `root`, starting at `beat` and lasting `len`.
/// Returns the notes added, so a caller can select them.
pub fn chord_at(
    notes: &mut Vec<Note>,
    root: u8,
    beat: f64,
    len: f64,
    quality: theory::Quality,
) -> Vec<usize> {
    let mut added = Vec::new();
    for pitch in theory::chord_pitches(root, quality) {
        added.push(notes.len());
        notes.push(Note {
            pitch,
            start: beat,
            len,
            vel: VELOCITY_DEFAULT,
        });
    }
    added
}

/// Which way an arpeggio walks its chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpDirection {
    Up,
    Down,
    UpDown,
}

/// Turn stacked notes into an arpeggio: the selected notes are re-dealt
/// one per `step` beats, in pitch order, each `step` long.
///
/// Operates on the notes that START TOGETHER — the chord under the
/// selection — and leaves everything else alone. The arpeggio begins where
/// the chord began, so a chord becomes a figure without moving in time.
/// `UpDown` walks up and back without repeating either endpoint, which is
/// what stops the turn sounding like a stutter.
pub fn arpeggiate(
    notes: &mut Vec<Note>,
    selected: &HashSet<usize>,
    step: f64,
    direction: ArpDirection,
) -> bool {
    if selected.len() < 2 || step <= 0.0 {
        return false;
    }
    let mut chord: Vec<Note> = selected
        .iter()
        .filter_map(|i| notes.get(*i).cloned())
        .collect();
    if chord.len() < 2 {
        return false;
    }
    chord.sort_by_key(|n| n.pitch);
    let start = chord.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);

    let order: Vec<usize> = match direction {
        ArpDirection::Up => (0..chord.len()).collect(),
        ArpDirection::Down => (0..chord.len()).rev().collect(),
        ArpDirection::UpDown => {
            let up = 0..chord.len();
            // Skip both endpoints coming back down: ascending 1-2-3 turns
            // into 1-2-3-2, not 1-2-3-3-2-1.
            let down = (1..chord.len().saturating_sub(1)).rev();
            up.chain(down).collect()
        }
    };

    // Drop the originals (high indices first, so the rest stay valid).
    let mut doomed: Vec<usize> = selected.iter().copied().collect();
    doomed.sort_unstable_by(|a, b| b.cmp(a));
    for i in doomed {
        if i < notes.len() {
            notes.remove(i);
        }
    }
    for (slot, ci) in order.into_iter().enumerate() {
        notes.push(Note {
            pitch: chord[ci].pitch,
            start: start + slot as f64 * step,
            len: step,
            vel: chord[ci].vel,
        });
    }
    true
}

/// How far a counter-line may sit from the cantus before it stops sounding
/// like a companion: an octave and a fifth.
const COUNTERPOINT_SPAN: i16 = 19;

/// Write a first-species counter-melody against the selected notes.
///
/// Note against note: for every note in the cantus, one counter-note of the
/// same rhythm. The rules it actually obeys, which is what separates this
/// from "add a third to everything":
///
/// - every interval is consonant (and the fourth counts as a dissonance —
///   see `theory::CONSONANT`),
/// - no parallel fifths or octaves,
/// - contrary motion is preferred, similar motion is a last resort,
/// - the line stays within [`COUNTERPOINT_SPAN`] of the cantus and prefers
///   small steps to leaps.
///
/// `above` puts the counter-line over the cantus. The counter-line stays in
/// `key`: a second voice that wanders out of the mode stops sounding like
/// an answer and starts sounding like a mistake.
///
/// Consonance outranks the key, though. If a cantus note admits no in-key
/// consonance — a chromatic note, or a five-note scale with nowhere to go —
/// the line takes a chromatic consonance rather than writing a dissonance
/// to stay diatonic. Species counterpoint treats the interval as the hard
/// rule and the mode as the strong preference, and so does this.
///
/// Returns the added notes' indices, or an empty vec if there was nothing
/// to work against.
pub fn counterpoint(
    notes: &mut Vec<Note>,
    selected: &HashSet<usize>,
    above: bool,
    key: Key,
) -> Vec<usize> {
    let mut cantus: Vec<Note> = selected
        .iter()
        .filter_map(|i| notes.get(*i).cloned())
        .collect();
    if cantus.is_empty() {
        return Vec::new();
    }
    cantus.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut added = Vec::new();
    let mut prev: Option<(u8, u8)> = None;
    for n in &cantus {
        // Two passes: stay in the key if the key allows a consonance here,
        // otherwise take a chromatic one. Never a dissonance.
        let mut best: Option<(i32, u8)> = None;
        for in_key_only in [true, false] {
            if best.is_some() {
                break;
            }
            best = pick_counter_note(n.pitch, above, prev, key, in_key_only);
        }
        let Some((_, pitch)) = best else { continue };
        prev = Some((n.pitch, pitch));
        added.push(notes.len());
        notes.push(Note {
            pitch,
            start: n.start,
            len: n.len,
            vel: n.vel,
        });
    }
    added
}

/// The scoring pass: the best counter-note against one cantus note, or
/// `None` when nothing on this side satisfies the rules.
fn pick_counter_note(
    cantus: u8,
    above: bool,
    prev: Option<(u8, u8)>,
    key: Key,
    in_key_only: bool,
) -> Option<(i32, u8)> {
    let mut best: Option<(i32, u8)> = None;
    {
        for offset in 1..=COUNTERPOINT_SPAN {
            let p = if above {
                i16::from(cantus) + offset
            } else {
                i16::from(cantus) - offset
            };
            if !(0..=i16::from(theory::MAX_PITCH)).contains(&p) {
                continue;
            }
            let cand = p as u8;
            if !theory::is_consonant(cantus, cand) {
                continue;
            }
            if in_key_only && !key.scale.contains(key.tonic, cand) {
                continue;
            }
            // Score: lower is better.
            let mut score = 0i32;
            if let Some(prev_pair) = prev {
                let now = (cantus, cand);
                if theory::is_parallel_perfect(prev_pair, now) {
                    continue; // a hard rule, not a preference
                }
                score += match theory::motion(prev_pair, now) {
                    theory::Motion::Contrary => 0,
                    theory::Motion::Oblique => 2,
                    theory::Motion::Similar => 5,
                };
                // Prefer stepwise movement in the counter-line.
                score += (i32::from(cand) - i32::from(prev_pair.1)).abs();
                // Perfect intervals are fine but plain; imperfect ones
                // (thirds and sixths) are the substance of a duet.
                if theory::is_perfect(cantus, cand) {
                    score += 3;
                }
            } else {
                // The opening: a perfect interval is the traditional start.
                score += if theory::is_perfect(cantus, cand) {
                    0
                } else {
                    2
                };
                score += i32::from(offset);
            }
            if best.is_none_or(|(bs, _)| score < bs) {
                best = Some((score, cand));
            }
        }
    }
    best
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod generative_tests {
    use super::*;

    fn n(pitch: u8, start: f64, len: f64) -> Note {
        Note {
            pitch,
            start,
            len,
            vel: 100,
        }
    }

    fn sel(idx: &[usize]) -> HashSet<usize> {
        idx.iter().copied().collect()
    }

    #[test]
    fn a_chord_lands_stacked_at_the_cursor() {
        let mut notes = Vec::new();
        let added = chord_at(&mut notes, 60, 2.0, 0.5, theory::Quality::Minor7);
        assert_eq!(added.len(), 4);
        let pitches: Vec<u8> = notes.iter().map(|x| x.pitch).collect();
        assert_eq!(pitches, vec![60, 63, 67, 70]);
        // Stacked: one onset, one length, all together.
        assert!(notes.iter().all(|x| x.start == 2.0 && x.len == 0.5));
    }

    #[test]
    fn arpeggio_spreads_a_chord_without_moving_it() {
        let mut notes = vec![n(60, 4.0, 2.0), n(64, 4.0, 2.0), n(67, 4.0, 2.0)];
        assert!(arpeggiate(
            &mut notes,
            &sel(&[0, 1, 2]),
            0.25,
            ArpDirection::Up
        ));
        assert_eq!(notes.len(), 3, "three notes in, three out");
        let mut got: Vec<(u8, f64)> = notes.iter().map(|x| (x.pitch, x.start)).collect();
        got.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        assert_eq!(got, vec![(60, 4.0), (64, 4.25), (67, 4.5)]);
        // It begins where the chord began — an arpeggio is a re-voicing in
        // time, not a move.
        assert!(notes.iter().any(|x| x.start == 4.0));
    }

    #[test]
    fn arpeggio_directions_differ_and_updown_does_not_stutter() {
        let build = |dir| {
            let mut notes = vec![n(60, 0.0, 1.0), n(64, 0.0, 1.0), n(67, 0.0, 1.0)];
            arpeggiate(&mut notes, &sel(&[0, 1, 2]), 0.25, dir);
            notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            notes.iter().map(|x| x.pitch).collect::<Vec<u8>>()
        };
        assert_eq!(build(ArpDirection::Up), vec![60, 64, 67]);
        assert_eq!(build(ArpDirection::Down), vec![67, 64, 60]);
        // Neither endpoint repeats on the way back.
        assert_eq!(build(ArpDirection::UpDown), vec![60, 64, 67, 64]);
    }

    #[test]
    fn arpeggio_needs_a_chord() {
        let mut notes = vec![n(60, 0.0, 1.0)];
        assert!(!arpeggiate(&mut notes, &sel(&[0]), 0.25, ArpDirection::Up));
        assert_eq!(notes.len(), 1, "a single note is not a chord; leave it be");
        assert!(!arpeggiate(&mut notes, &sel(&[0]), 0.0, ArpDirection::Up));
    }

    /// The rules, asserted. This is what makes it counterpoint rather than
    /// "a third above everything".
    #[test]
    fn counterpoint_obeys_its_rules() {
        // A cantus that moves around enough to tempt every fault.
        let cantus: Vec<Note> = [60u8, 62, 64, 65, 67, 65, 64, 62, 60]
            .iter()
            .enumerate()
            .map(|(i, p)| n(*p, i as f64, 1.0))
            .collect();
        let mut notes = cantus.clone();
        let key = Key::default(); // C major
        let added = counterpoint(
            &mut notes,
            &sel(&(0..cantus.len()).collect::<Vec<_>>()),
            true,
            key,
        );
        assert_eq!(
            added.len(),
            cantus.len(),
            "one counter-note per cantus note"
        );

        let counter: Vec<Note> = added.iter().map(|i| notes[*i].clone()).collect();
        for (c, x) in cantus.iter().zip(&counter) {
            assert_eq!(x.start, c.start, "note against note: same rhythm");
            assert_eq!(x.len, c.len);
            assert!(x.pitch > c.pitch, "asked for above, stay above");
            assert!(
                theory::is_consonant(c.pitch, x.pitch),
                "dissonance at beat {}: {} against {}",
                c.start,
                x.pitch,
                c.pitch
            );
            assert!(
                i16::from(x.pitch) - i16::from(c.pitch) <= COUNTERPOINT_SPAN,
                "the counter-line drifted out of earshot"
            );
        }
        // No parallel fifths or octaves anywhere in the pair sequence.
        for w in cantus.iter().zip(&counter).collect::<Vec<_>>().windows(2) {
            let prev = (w[0].0.pitch, w[0].1.pitch);
            let now = (w[1].0.pitch, w[1].1.pitch);
            assert!(
                !theory::is_parallel_perfect(prev, now),
                "parallel perfect between {prev:?} and {now:?}"
            );
        }
        // Every counter-note is IN THE KEY: a second voice that wanders out
        // of the mode stops sounding like an answer.
        for x in &counter {
            assert!(
                key.scale.contains(key.tonic, x.pitch),
                "{} is not in {}",
                x.pitch,
                key.label()
            );
        }
        // And it is a real second voice: not a fixed interval throughout.
        let intervals: HashSet<i16> = cantus
            .iter()
            .zip(&counter)
            .map(|(c, x)| i16::from(x.pitch) - i16::from(c.pitch))
            .collect();
        assert!(
            intervals.len() > 1,
            "a constant offset is parallel harmony, not counterpoint"
        );
    }

    /// The key is not decoration: the same cantus must yield a different
    /// counter-line in a different mode, and always stay inside it.
    #[test]
    fn counterpoint_follows_the_chosen_scale() {
        let cantus: Vec<Note> = [60u8, 62, 64, 65, 67]
            .iter()
            .enumerate()
            .map(|(i, p)| n(*p, i as f64, 1.0))
            .collect();
        let line = |key: Key| -> Vec<u8> {
            let mut notes = cantus.clone();
            let added = counterpoint(
                &mut notes,
                &sel(&(0..cantus.len()).collect::<Vec<_>>()),
                true,
                key,
            );
            let out: Vec<u8> = added.iter().map(|i| notes[*i].pitch).collect();
            for p in &out {
                assert!(
                    key.scale.contains(key.tonic, *p),
                    "{p} is outside {}",
                    key.label()
                );
            }
            out
        };

        let major = line(Key {
            tonic: 0,
            scale: theory::Scale::Major,
        });
        let minor = line(Key {
            tonic: 0,
            scale: theory::Scale::NaturalMinor,
        });
        assert_ne!(
            major, minor,
            "C major and C minor must not produce the same counter-line"
        );
    }

    /// Consonance is the hard rule, the mode is the strong preference. A
    /// cantus with no in-key consonance available must still get a
    /// consonant answer rather than a diatonic dissonance.
    #[test]
    fn consonance_outranks_the_key() {
        // Pentatonic minor on C has only five pitches; a chromatic cantus
        // note (C#) has no in-key consonance within reach in some spots.
        let key = Key {
            tonic: 0,
            scale: theory::Scale::PentatonicMinor,
        };
        let cantus: Vec<Note> = (61..67u8)
            .enumerate()
            .map(|(i, p)| n(p, i as f64, 1.0))
            .collect();
        let mut notes = cantus.clone();
        let added = counterpoint(
            &mut notes,
            &sel(&(0..cantus.len()).collect::<Vec<_>>()),
            true,
            key,
        );
        assert_eq!(added.len(), cantus.len(), "every note gets an answer");
        for (c, i) in cantus.iter().zip(&added) {
            assert!(
                theory::is_consonant(c.pitch, notes[*i].pitch),
                "a dissonance was written to stay in key"
            );
        }
    }

    #[test]
    fn counterpoint_below_stays_below_and_handles_nothing() {
        let mut notes = vec![n(72, 0.0, 1.0), n(74, 1.0, 1.0)];
        let added = counterpoint(&mut notes, &sel(&[0, 1]), false, Key::default());
        assert_eq!(added.len(), 2);
        for i in added {
            assert!(notes[i].pitch < 72, "asked for below, stay below");
        }
        // Nothing selected: nothing written, no panic.
        let mut empty = vec![n(60, 0.0, 1.0)];
        assert!(counterpoint(&mut empty, &HashSet::new(), true, Key::default()).is_empty());
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn counterpoint_survives_the_pitch_ceiling() {
        // A cantus at the very top: there is no room above, so the verb
        // writes nothing rather than wrapping into the bass.
        let mut notes = vec![n(127, 0.0, 1.0)];
        let added = counterpoint(&mut notes, &sel(&[0]), true, Key::default());
        assert!(added.is_empty(), "no room above 127");
        assert_eq!(notes.len(), 1);
    }
}
