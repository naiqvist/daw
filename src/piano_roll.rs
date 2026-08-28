//! The piano roll: the bottom region's second face.
//!
//! Shift+Tab swaps the device rack for this editor. It owns no notes: it
//! is a VIEW over the selected clip's `Vec<Note>`, editing that vec in
//! place. What lives here is view state only — where the cursor is, which
//! rung of the grid ladder is set, where the view is scrolled, which
//! notes are selected, which tool the pointer is holding, what was
//! copied. Select a different clip and the same editor shows different
//! notes; there is nothing to sync, because there is only one copy of a
//! note anywhere in the app.
//!
//! Note positions are CLIP-RELATIVE: beat 0 is the clip's start. Editing
//! a note never resizes the clip, so notes can be pushed past the clip's
//! end — they are drawn washed out beyond the end rule and do not sound
//! (see `seq_notes` in `main.rs`). Shortening a clip hides notes rather
//! than destroying them.
//!
//! Undo is free and must stay that way: `History::sync` watches the model
//! and banks whatever changed once every gesture has settled, so no verb
//! in this file has to remember to record anything, and no verb may write
//! to the clip by any route but the `&mut Clip` it is handed.
//!
//! See `notes/20260826-piano-roll-spec.md` for the design this
//! implements, and `notes/20260826-device-ui-contract.md` for the rules
//! every draggable target here obeys.
//!
//! # Geometry
//!
//! [`Geom`] is the ONLY way to turn a beat or a pitch into a pixel. That
//! is not a style preference: half this module used to reach for the
//! `ROW_H` and `PX_PER_BEAT` constants directly while the other half went
//! through [`Zoom`], so at any zoom but 1.0 the row stripes, the octave
//! rules, the gutter labels, the cursor cell and the drag arithmetic all
//! disagreed about where the notes were. The constants are now named
//! `*_DEFAULT` and used in exactly one place, which makes the mistake a
//! compile error rather than a bug report.
//!
//! [`Fold`] rides inside `Geom` as a 128-bit mask, one bit per pitch. A
//! pitch's row is the count of shown pitches above it — one shift and one
//! popcount — and the unfolded mask reduces to the `127 - pitch` it
//! replaced, so folded and unfolded share one code path rather than two
//! that drift.
//!
//! # Colour
//!
//! One channel, one meaning, enforced by [`note_paint`] and by
//! `unselected_notes_are_not_greyed` beside the theme:
//!
//! - **hue** is identity and never moves;
//! - **chroma** says whether the note can sound — grey means *cannot*,
//!   and nothing else;
//! - **value** carries velocity, inside a bounded band;
//! - **the outline**, and nothing else, says selected.
//!
//! The rule this replaced picked between two colours a hue and a third of
//! a lightness apart, so the notes you were not holding looked switched
//! off — which was most notes, most of the time, in an editor whose
//! entire content is notes.
//!
//! # Gestures
//!
//! One interaction over the grid, and the target captured AT THE PRESS
//! into `Drag`, from `press_origin` rather than from the pointer's
//! position a frame later. Nothing is re-decided after the button goes
//! down, so a drag that crosses a neighbour, pins at the end of a range,
//! or leaves the panel entirely still belongs to whatever it began on.
//! Every variant recomputes from the press rather than accumulating
//! per-frame deltas, and every one carries the whole SELECTION rather
//! than one index. The `pointer` test module drives all of it for real.
//!
//! The modifier grammar, once, for every drag:
//!
//! ```text
//! Shift       extend the selection / constrain the drag to one axis
//! Ctrl        toggle membership
//! Alt         copy instead of move
//! Ctrl+Alt    bypass snap (and scale lock) for this gesture
//! ```
//!
//! # Tools
//!
//! ```text
//! 1 pointer   2 draw   3 erase   4 split   5 mute   6 marquee
//! ```
//!
//! Holding a tool key BORROWS that tool until the finger comes up;
//! TAPPING it latches. Escape always comes home. The strip and the info
//! line both show whichever is in force, because a mode you cannot see is
//! a mode you will be surprised by.
//!
//! # The keyboard, in one place
//!
//! Everything the mouse can do the keyboard can do, routed the way the
//! arrangement's shortcuts are: `keys` runs BEFORE `Focus::begin` and
//! stands down unless the focus ring sat on the roll's cursor cell last
//! frame. Left at beat 0 is deliberately NOT claimed — there must always
//! be an arrow that walks back out to the rest of the app.
//!
//! ```text
//! arrows            move the cursor (grid step / semitone)
//! PageUp/PageDown   octave jump
//! Shift+arrows      extend a box selection from the anchor
//! Ctrl+Up/Down      transpose the selection a semitone — the CURSOR
//!                   rides along with every selection move, so the next
//!                   keystroke still holds the note it was holding
//! Ctrl+Left/Right   nudge the selection a grid step — or, with NOTHING
//!                   selected, widen/narrow the roll's own grid (the
//!                   arrangement keeps Ctrl+1/2 for its grid)
//! Ctrl+Shift+Up/Dn  transpose the selection an OCTAVE
//! Enter or A        add a note at the cursor
//! Delete/Backspace  delete the selection
//! Ctrl+A            select all;  Escape clears AND returns to Pointer
//! Ctrl+C/X/V        copy / cut / paste at the cursor beat
//! Ctrl+D            duplicate the selection directly after itself
//! [ / ]             shrink / grow selected LENGTHS by a grid step
//! Shift+[ / ]       halve / double the selection's SPAN in time
//! , / .             velocity -10 / +10;  Shift+, / . is -1 / +1
//! Ctrl+U            quantize starts;  Ctrl+Shift+U  lengths too
//! Ctrl+E            split selected notes at the cursor beat
//! Ctrl+J            join selected notes per pitch, first start to
//!                   last end — the inverse of the split
//! Ctrl+L            legato: stretch each note to the next start
//! 0                 deactivate (mute) the selection, Ableton's key;
//!                   muted notes draw hollow and never reach the wire
//! N / Shift+N       jump to the next / previous note and select it
//! Home / End        cursor to beat 0 / the end of the material
//! H                 humanize position and velocity
//! R                 retrograde the selection in its own span
//! I                 enter chord-writing mode; Enter commits, Escape
//!                   cancels. Shift+I inverts the selection about its
//!                   own middle
//! S / Shift+S       strum a stack up / down by a grid step
//! F                 fold the grid to the material
//! K / Shift+K       lock gestures to the key / force notes into it
//! L                 step the lane stack;  Ctrl+Shift+L locks the
//!                   drawn-note length to the selection's
//! + / - (Shift+)    zoom time (pitch)
//! 1..6              tools, held to borrow and tapped to latch
//! Ctrl+Shift+Enter  open/close the TRIG editor: probability (swept)
//!                   and the Elektron A:B condition ladder (stepped a
//!                   rung per keystroke); Delete resets a row, Escape
//!                   leaves. A conditional note wears a hollow ring
//! Shift+Enter       open/close the PARAMETER-LOCK editor on the held
//!                   note; inside it, arrows walk rows and set values
//!                   (Shift is fine), Enter locks a row at the knob,
//!                   Delete clears it, Escape leaves. The mouse clicks
//!                   a row to hold it and drags to set it; a locked
//!                   note wears a dot
//!
//! ```
//!
//! Enter is SMART: on an empty cell it adds a note; on a note it toggles
//! that note's selection — the keyboard's way to pick up what is already
//! there. `A` always adds. The cursor cannot walk off screen: any
//! keystroke that moves it scrolls the view the minimum to keep up.
//!
//! # Owed to this module
//!
//! Two things the spec asks for and this file cannot provide alone:
//!
//! - **Audition.** Clicking a note is silent, because there is no path
//!   from the UI to a voice. That needs a bounded `Copy`-only ring beside
//!   the existing param and transport ones, and it is red-zone work.
//! - **The transport.** [`body_at`] takes a [`TimeView`] and will draw a
//!   ruler, a playhead and a follow scroll the moment one is passed;
//!   [`body`] passes `playhead: None`, which draws no playhead rather
//!   than one parked at the origin. [`PianoRoll::take_locate`] is the
//!   other half of that handshake, waiting for a caller.

use daw::ui::theme::Theme;
use daw::ui::tokens::{font, radius, space, stroke};
use eframe::egui;
use std::collections::HashSet;

use crate::{Clip, Focus, GRID_BEATS, GRID_DEFAULT, GRID_NAMES, Key, Note, claim};
use daw::theory::{self, ChordSymbol, Voicing};

/// One lockable parameter of the track's instrument, as the plock editor
/// lists it: the table id the engine understands, the words the user
/// reads, the range the slider math needs, and the BASE — the knob's
/// current value, which is both what a fresh lock starts from and what
/// an unlocked note plays.
#[derive(Clone)]
pub struct PlockParam {
    pub id: u32,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub base: f32,
    /// Discrete choice count — 0 for a continuous parameter. A discrete
    /// row steps ONE choice per keystroke or detent; a continuous one
    /// sweeps fractions of its range.
    pub choices: u32,
    /// The instrument card's own formatter, so the editor prints "saw"
    /// and "1.05 s" — the words the knob would use — never a bare float.
    ///
    /// A CLOSURE rather than a bare `fn`, and that is the whole point: a
    /// function pointer cannot carry which device it belongs to, so the
    /// editor used to be handed one formatter for every instrument and
    /// only the poly synth's rows read correctly. Capturing the kind is
    /// what lets every device speak for itself.
    pub format: std::sync::Arc<dyn Fn(f32) -> String + Send + Sync>,
}

impl std::fmt::Debug for PlockParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlockParam")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("base", &self.base)
            .field("choices", &self.choices)
            .finish_non_exhaustive()
    }
}

impl PlockParam {
    /// A lock value in this parameter's own words.
    pub fn face(&self, value: f32) -> String {
        (self.format)(value)
    }

    /// Where `value` sits in this parameter's range, `0..=1`. What the
    /// row's position bar draws — and the thing that makes "this is
    /// already at its minimum" visible instead of mysterious.
    pub fn position(&self, value: f32) -> f32 {
        if self.max <= self.min {
            0.0
        } else {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }
}

// --- geometry, all in logical points -----------------------------------

/// One coarse nudge of a parameter lock, as a fraction of the row's
/// range; Shift gives the fine one.
const PLOCK_COARSE: f32 = 0.025;
const PLOCK_FINE: f32 = 0.0025;

/// Width of the piano keyboard gutter on the left.
const KEYS_W: f32 = 48.0;
/// Width of the tool strip on the far left. Narrow on purpose: it is six
/// one-glyph buttons, and every one of them is also a key.
const TOOLS_W: f32 = 18.0;
/// Height of the bar ruler along the top.
const RULER_H: f32 = 20.0;
/// Height of the info line along the bottom.
const INFO_H: f32 = 18.0;
/// Starting height of an expression lane, and the floor a drag may not
/// take it below before it collapses to its header.
const LANE_H: f32 = 56.0;
const LANE_H_MIN: f32 = 24.0;
const LANE_H_MAX: f32 = 240.0;
/// A collapsed lane: its header and nothing else.
const LANE_HEADER_H: f32 = 12.0;

/// The DEFAULT row height and beat width — the value `Zoom::default`
/// starts at, and nothing else.
///
/// These are named `_DEFAULT` deliberately. They used to be `ROW_H` and
/// `PX_PER_BEAT`, and half the module reached for them directly while the
/// other half went through `Zoom` — so at any zoom but 1.0 the row
/// stripes, the octave rules, the gutter labels, the cursor cell and the
/// drag arithmetic all disagreed with where the notes actually were. The
/// rename is the fix: every remaining use is now a compile error, and the
/// only legal path to a size on screen is [`Geom`].
const ROW_H_DEFAULT: f32 = 14.0;
const PX_PER_BEAT_DEFAULT: f32 = 24.0;

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
            px_per_beat: PX_PER_BEAT_DEFAULT,
            row_h: ROW_H_DEFAULT,
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
/// Which pitch rows the grid is showing, as a bit per MIDI pitch.
///
/// A `u128` rather than a lookup table because it makes the whole fold
/// feature nearly free: the row a pitch sits on is the number of shown
/// pitches above it, which is one shift and one `count_ones`. Unfolded is
/// `u128::MAX`, and for that mask the arithmetic reduces exactly to the
/// `127 - pitch` it replaced — so the unfolded path costs one popcount and
/// cannot drift from the folded one.
///
/// **Invariant:** a fold mask always contains every pitch the clip has a
/// note on. Folding is a view change and may never hide material; see
/// `fold_mask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fold {
    mask: u128,
}

impl Fold {
    /// Every pitch shown — the unfolded grid.
    pub const ALL: Self = Self { mask: u128::MAX };

    pub fn shows(self, pitch: u8) -> bool {
        self.mask >> pitch & 1 == 1
    }

    /// How many rows the grid has.
    pub fn rows(self) -> u32 {
        self.mask.count_ones()
    }

    /// The row a pitch sits on: the count of shown pitches above it.
    ///
    /// Defined for a hidden pitch too, where it gives the row of the next
    /// shown pitch below. That keeps it monotonic, which is all the
    /// callers that hand it a cursor position actually need.
    pub fn row_of(self, pitch: u8) -> u32 {
        self.mask
            .checked_shr(u32::from(pitch) + 1)
            .unwrap_or(0)
            .count_ones()
    }

    /// The shown pitches whose row index falls in `rows`, paired with that
    /// index, from the top of the grid down.
    ///
    /// One pass over 128 bits regardless of the window, which is cheaper
    /// than it looks and far cheaper than the alternative: the loops that
    /// call this used to walk all 128 pitches and paint every one.
    pub fn rows_in(self, rows: std::ops::Range<u32>) -> impl Iterator<Item = (u32, u8)> {
        let mask = self.mask;
        (0..=PITCH_MAX)
            .rev()
            .filter(move |&p| mask >> p & 1 == 1)
            .enumerate()
            .map(|(i, p)| (i as u32, p))
            .skip_while(move |&(i, _)| i < rows.start)
            .take_while(move |&(i, _)| i < rows.end)
    }

    /// The pitch on a given row, if the grid has that row.
    pub fn pitch_of_row(self, row: u32) -> Option<u8> {
        self.rows_in(row..row + 1).next().map(|(_, p)| p)
    }

    /// The mask over exactly the pitches these notes use. Empty input
    /// folds to nothing, which the caller turns back into `ALL` — an
    /// empty clip folded to zero rows is a blank panel, not a feature.
    pub fn used(notes: &[Note]) -> Self {
        let mut mask = 0u128;
        for n in notes {
            mask |= 1u128 << n.pitch;
        }
        Self { mask }
    }

    /// The mask over one scale, in every octave.
    pub fn scale(key: Key) -> Self {
        let mut mask = 0u128;
        for pitch in 0..=PITCH_MAX {
            if key.scale.contains(key.tonic, pitch) {
                mask |= 1u128 << pitch;
            }
        }
        Self { mask }
    }

    /// Everything either mask shows. Used to keep the invariant: whatever
    /// a fold is folding TO, the notes that exist come along.
    pub fn union(self, other: Self) -> Self {
        Self {
            mask: self.mask | other.mask,
        }
    }

    pub fn is_empty(self) -> bool {
        self.mask == 0
    }

    /// The shown pitch nearest to `pitch`, searching outward. Used to keep
    /// the cursor on a row that exists after a fold.
    pub fn nearest_shown(self, pitch: u8) -> u8 {
        if self.shows(pitch) || self.is_empty() {
            return pitch;
        }
        for d in 1..=i32::from(PITCH_MAX) {
            for cand in [i32::from(pitch) - d, i32::from(pitch) + d] {
                if (0..=i32::from(PITCH_MAX)).contains(&cand) && self.shows(cand as u8) {
                    return cand as u8;
                }
            }
        }
        pitch
    }
}

/// The map between musical coordinates and the screen, in one place.
///
/// Every conversion the roll performs lives on this struct. Nothing else
/// in the module may compute a row height or a beat width, which is why
/// [`ROW_H_DEFAULT`] and [`PX_PER_BEAT_DEFAULT`] are named the way they
/// are: the only way to get a size is to ask a `Geom` that already knows
/// the zoom — and, since folding, which rows exist.
///
/// `Copy` and small, so it is passed by value everywhere and there is
/// never a stale one to go hunting for.
#[derive(Debug, Clone, Copy)]
pub struct Geom {
    /// The note grid's rect. `x` maps beats, `y` maps pitch rows.
    pub grid: egui::Rect,
    /// First beat at the grid's left edge.
    pub sb: f32,
    /// Pixels of pitch content scrolled above the grid's top edge.
    pub sy: f32,
    pub z: Zoom,
    pub fold: Fold,
}

impl Geom {
    pub fn new(grid: egui::Rect, sb: f32, sy: f32, z: Zoom, fold: Fold) -> Self {
        Self {
            grid,
            sb,
            sy,
            z,
            fold,
        }
    }

    /// The same map over a different rect — what a lane below the grid
    /// uses, so a note's bar sits under the note itself by construction
    /// rather than by two call sites agreeing about scroll and zoom.
    pub fn over(self, rect: egui::Rect) -> Self {
        Self { grid: rect, ..self }
    }

    pub fn row_h(self) -> f32 {
        self.z.row_h
    }

    pub fn px_per_beat(self) -> f32 {
        self.z.px_per_beat
    }

    pub fn x_at(self, beat: f64) -> f32 {
        self.grid.left() + (beat as f32 - self.sb) * self.z.px_per_beat
    }

    pub fn beat_at(self, x: f32) -> f64 {
        f64::from(self.sb + (x - self.grid.left()) / self.z.px_per_beat).max(0.0)
    }

    /// The top of a row index, counting from the top of the pitch space.
    pub fn row_y(self, row: u32) -> f32 {
        self.grid.top() + row as f32 * self.z.row_h - self.sy
    }

    /// The top of a pitch's row. Unfolded, pitch 127 is row 0 — high notes
    /// on top, as they are on a keyboard stood on its end.
    pub fn row_top(self, pitch: u8) -> f32 {
        self.row_y(self.fold.row_of(pitch))
    }

    /// The pitch whose row contains `y`, if the grid has a row there.
    pub fn pitch_at(self, y: f32) -> Option<u8> {
        let row = ((y - self.grid.top() + self.sy) / self.z.row_h).floor();
        if row < 0.0 {
            return None;
        }
        self.fold.pitch_of_row(row as u32)
    }

    /// A note's rect in the grid.
    pub fn note_rect(self, n: &Note) -> egui::Rect {
        let y = self.row_top(n.pitch);
        egui::Rect::from_min_max(
            egui::pos2(self.x_at(n.start), y),
            egui::pos2(self.x_at(n.start + n.len), y + self.z.row_h),
        )
    }

    /// The row indices on screen. Never the whole 128: a paint loop over
    /// all of them at row height 5 in a 200pt panel draws 128 rects for 40
    /// visible rows, sixty times a second, forever.
    pub fn visible_rows(self) -> std::ops::Range<u32> {
        let first = ((self.sy / self.z.row_h).floor().max(0.0)) as u32;
        let count = (self.grid.height() / self.z.row_h).ceil() as u32 + 1;
        first..(first + count).min(self.fold.rows())
    }

    /// The visible rows paired with the pitch on each.
    pub fn visible(self) -> impl Iterator<Item = (u32, u8)> {
        self.fold.rows_in(self.visible_rows())
    }

    /// The total height of the pitch space at this zoom and this fold —
    /// what the vertical scroll is clamped against.
    pub fn content_h(self) -> f32 {
        self.fold.rows() as f32 * self.z.row_h
    }

    /// The row a pitch is on, straight from the fold. Test-facing: the
    /// paint path uses `row_y`/`row_top`, and this is how a test says
    /// "and those two agree with the mask" without reaching inside.
    #[cfg(test)]
    pub fn row_of_check(self, pitch: u8) -> u32 {
        self.fold.row_of(pitch)
    }

    /// Is any part of this rect on screen horizontally? Notes off to
    /// either side are skipped rather than drawn and clipped.
    pub fn spans_x(self, r: egui::Rect) -> bool {
        r.right() >= self.grid.left() && r.left() <= self.grid.right()
    }
}

// --- the note's appearance ----------------------------------------------

/// Everything the paint layer needs to know about a note, gathered once so
/// [`note_paint`] is a pure function of it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoteState {
    pub selected: bool,
    pub hovered: bool,
    /// Deactivated by the user — drawn hollow, never sounds.
    pub muted: bool,
    /// Past the clip's end: it CANNOT sound, whatever else is true.
    pub past_end: bool,
    /// Fires only sometimes — a probability under 1, or an A:B condition.
    pub conditional: bool,
    /// `1..=127`.
    pub vel: u8,
}

/// How one note is drawn.
#[derive(Debug, Clone, Copy)]
pub struct NotePaint {
    pub fill: egui::Color32,
    pub edge: egui::Stroke,
    /// A deactivated note is drawn as an outline: dimming alone reads as
    /// "quiet", and a deactivated note is not quiet, it is off.
    pub hollow: bool,
}

/// The velocity band's floor, as a fraction of the note's full value.
///
/// This used to be 0.55, which put a `vel 1` note within a hair of the
/// ground it sits on. Velocity is real information and deserves a visible
/// range, but a whisper is still a note and must still look like one — so
/// the range is a BAND, not a fade to nothing.
const VEL_VALUE_FLOOR: f32 = 0.72;
/// How much chroma a sometimes-firing note gives up. Chroma is the
/// will-it-sound channel; a conditional note is as loud as its neighbours
/// when it does fire, so it keeps its value and gives up saturation only.
const CONDITIONAL_CHROMA: f32 = 0.70;
/// What a note past the clip's end keeps. The one fully desaturated state
/// in the roll, because it is the one state that cannot make a sound.
const PAST_END_CHROMA: f32 = 0.0;
const PAST_END_VALUE: f32 = 0.55;

/// Pull a colour toward its own grey. Cheap and sufficient: the doctrine
/// asks for "less chroma", not for a perceptually uniform space.
fn desaturate(c: egui::Color32, keep: f32) -> egui::Color32 {
    let (r, g, b) = (f32::from(c.r()), f32::from(c.g()), f32::from(c.b()));
    let grey = 0.299 * r + 0.587 * g + 0.114 * b;
    let mix = |x: f32| (grey + (x - grey) * keep).clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(mix(r), mix(g), mix(b), c.a())
}

/// The colour doctrine, in one function, so it can only be got right or
/// wrong once. One channel, one meaning:
///
/// - **hue** is identity and never moves;
/// - **chroma** says whether the note can sound;
/// - **value** carries velocity, inside a bounded band;
/// - **the outline**, and nothing else, says selected.
///
/// The rule this replaces picked between `clip_body` and `clip_selected`,
/// two colours a hue and a third of a lightness apart. The notes you were
/// not holding therefore looked switched off — which was most notes, most
/// of the time, in an editor whose entire content is notes.
pub fn note_paint(theme: &Theme, st: NoteState) -> NotePaint {
    // Hue and base chroma: identity, then hover, then selection. All
    // three are one colour family by construction — see the `note_*`
    // roles in `ui::theme` and the doctrine test beside them.
    let base = if st.selected {
        theme.note_fill_selected
    } else if st.hovered {
        theme.note_hover
    } else {
        theme.note_fill
    };

    // Chroma: the only channel allowed to answer "will this sound?".
    let mut fill = if st.past_end {
        desaturate(base, PAST_END_CHROMA)
    } else if st.conditional {
        desaturate(base, CONDITIONAL_CHROMA)
    } else {
        base
    };

    // Value: velocity, inside the band. A past-end note also loses value,
    // because it genuinely sits behind the clip's wash.
    let vel = f32::from(st.vel.clamp(VELOCITY_MIN, PITCH_MAX)) / f32::from(PITCH_MAX);
    let mut value = VEL_VALUE_FLOOR + (1.0 - VEL_VALUE_FLOOR) * vel;
    if st.past_end {
        value *= PAST_END_VALUE;
    }
    fill = fill.gamma_multiply(value);

    // The outline, and only the outline, says selected.
    let edge = if st.selected {
        egui::Stroke::new(stroke::BOLD, theme.focus)
    } else {
        egui::Stroke::new(stroke::HAIR, theme.note_edge)
    };

    NotePaint {
        fill,
        edge,
        hollow: st.muted,
    }
}

// --- tools ---------------------------------------------------------------

/// What the pointer's verb is. Six, and the first is the pointer itself.
///
/// Cubase's rule, and the reason a tool palette does not need clicking:
/// every tool is also a MOMENTARY modifier. Holding its key borrows it
/// until release; tapping it latches. `Esc` always comes home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pointer,
    Draw,
    Erase,
    Split,
    Mute,
    Marquee,
}

impl Tool {
    pub const ALL: [Self; 6] = [
        Self::Pointer,
        Self::Draw,
        Self::Erase,
        Self::Split,
        Self::Mute,
        Self::Marquee,
    ];

    /// The one-glyph face in the tool column. Words do not fit in 48
    /// points, and an icon set is not worth a dependency.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pointer => "K",
            Self::Draw => "D",
            Self::Erase => "E",
            Self::Split => "S",
            Self::Mute => "M",
            Self::Marquee => "B",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Draw => "draw",
            Self::Erase => "erase",
            Self::Split => "split",
            Self::Mute => "mute",
            Self::Marquee => "marquee",
        }
    }

    /// The number key that picks it — and, held, borrows it.
    pub fn key(self) -> egui::Key {
        match self {
            Self::Pointer => egui::Key::Num1,
            Self::Draw => egui::Key::Num2,
            Self::Erase => egui::Key::Num3,
            Self::Split => egui::Key::Num4,
            Self::Mute => egui::Key::Num5,
            Self::Marquee => egui::Key::Num6,
        }
    }

    fn cursor(self) -> egui::CursorIcon {
        match self {
            Self::Pointer => egui::CursorIcon::Default,
            Self::Draw => egui::CursorIcon::Crosshair,
            Self::Erase => egui::CursorIcon::NoDrop,
            Self::Split => egui::CursorIcon::ResizeColumn,
            Self::Mute => egui::CursorIcon::Cell,
            Self::Marquee => egui::CursorIcon::Crosshair,
        }
    }
}

// --- expression lanes ----------------------------------------------------

/// A length lane's full-scale value, in beats: one bar of common time.
const LANE_LEN_FULL: f32 = 4.0;

/// One per-note property, shown as a lane of bars under the grid.
///
/// Only properties `Note` actually carries are here. Pressure, bend and
/// per-note pan are named in the spec and are deliberately NOT in this
/// list: the note has no field for them, and inventing one on the UI side
/// would be a promise the engine could not keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Velocity,
    Probability,
    Length,
}

impl Lane {
    pub const ALL: [Self; 3] = [Self::Velocity, Self::Probability, Self::Length];

    pub fn label(self) -> &'static str {
        match self {
            Self::Velocity => "vel",
            Self::Probability => "prob",
            Self::Length => "len",
        }
    }

    /// The note's value on this lane, normalized to `0..=1`.
    ///
    /// Length has no natural ceiling, so it is shown against a fixed four
    /// beats. A longer note pins at the top rather than rescaling the
    /// lane under all the others.
    pub fn norm(self, n: &Note) -> f32 {
        match self {
            Self::Velocity => f32::from(n.vel) / f32::from(PITCH_MAX),
            Self::Probability => n.prob.clamp(0.0, 1.0),
            Self::Length => (n.len as f32 / LANE_LEN_FULL).clamp(0.0, 1.0),
        }
    }

    /// Write a normalized value back onto the note, in the note's own
    /// units and with the note's own floors.
    pub fn set(self, n: &mut Note, norm: f32, grid: f64) {
        let norm = norm.clamp(0.0, 1.0);
        match self {
            Self::Velocity => {
                n.vel = (norm * f32::from(PITCH_MAX))
                    .round()
                    .clamp(f32::from(VELOCITY_MIN), f32::from(PITCH_MAX))
                    as u8;
            }
            Self::Probability => n.prob = norm,
            Self::Length => {
                // Snapped like every other length edit, and never below
                // one tick — a zero-length note is not a note.
                let want = f64::from(norm) * f64::from(LANE_LEN_FULL);
                n.len = snap(want, grid).max(grid);
            }
        }
    }

    /// The value in the note's own words, for the info line.
    pub fn face(self, n: &Note) -> String {
        match self {
            Self::Velocity => n.vel.to_string(),
            Self::Probability => format!("{:.0}%", n.prob * 100.0),
            Self::Length => format!("{:.3}", n.len),
        }
    }
}

/// One open lane, and how tall the user has made it.
#[derive(Debug, Clone, Copy)]
pub struct LaneView {
    pub lane: Lane,
    pub h: f32,
    pub collapsed: bool,
}

impl LaneView {
    pub fn height(self) -> f32 {
        if self.collapsed {
            LANE_HEADER_H
        } else {
            self.h.clamp(LANE_H_MIN, LANE_H_MAX)
        }
    }
}

// --- the transport, as the roll sees it ----------------------------------

/// What the roll needs to know about time. Mirrors `waveform::TimeView`
/// on purpose: two clip editors that disagree about what a playhead is
/// will eventually disagree on screen.
#[derive(Debug, Clone, Copy)]
pub struct TimeView {
    pub beats_per_bar: u32,
    /// The transport's position in ABSOLUTE beats, or `None` when the
    /// caller has not wired one. The roll subtracts the clip's own start,
    /// so a clip beginning at bar 5 still shows bar 1 at its own beat 0.
    ///
    /// `None` rather than `0.0` for the default, because a playhead
    /// parked at the origin and a playhead that does not exist look
    /// identical on screen and mean completely different things.
    pub playhead: Option<f64>,
    pub playing: bool,
    /// Playing AND following. The view chases the playhead only while
    /// both are true — and never while the user is scrolling by hand.
    pub follow: bool,
}

impl Default for TimeView {
    fn default() -> Self {
        Self {
            beats_per_bar: 4,
            playhead: None,
            playing: false,
            follow: false,
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

/// A deliberately small modal command line owned by the piano roll.
///
/// This is view state: until Enter successfully parses the whole phrase it
/// owns no notes and therefore creates no history entry. The parser and the
/// music-theory crate are green-zone work and never reach the audio callback.
#[derive(Debug, Default)]
struct ChordEntry {
    text: String,
    diagnostic: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum MusicScriptPage {
    #[default]
    Tonnetz,
    Help,
}

#[derive(Debug)]
struct ChordEntryPlan {
    notes: Vec<Note>,
    advance: f64,
    clip_length: Option<ClipLengthEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipLengthEdit {
    Set(f32),
    Extend(f32),
    Trim(f32),
    Fit,
}

const CHORD_ENTRY_MAX_CHARS: usize = 256;
const CHORD_ENTRY_MAX_EVENTS: usize = 64;

/// Append typed or pasted text to the entry line, bounded by
/// [`CHORD_ENTRY_MAX_CHARS`].
///
/// Control characters become spaces rather than being dropped: a pasted
/// phrase split over two lines is still one phrase, and the language
/// separates its events by whitespace, so a newline that silently vanished
/// would weld `!cM9` onto `!am9`.
fn append_entry(entry: &mut ChordEntry, text: &str) {
    let mut room = CHORD_ENTRY_MAX_CHARS.saturating_sub(entry.text.chars().count());
    for ch in text.chars() {
        if room == 0 {
            break;
        }
        entry.text.push(if ch.is_control() { ' ' } else { ch });
        room -= 1;
    }
}

/// A shipped harmonic idiom: one symbol standing for a whole gesture.
///
/// The catalogue is chromatic ninth-chord vamps, which is a specific thing
/// a specific music does — two chords a semitone apart, the fifth omitted,
/// voiced as a cluster. A chord symbol cannot say that. `!fm9` names five
/// intervals and is silent about the two things that make the sound: how
/// the voices are spaced, and what the next chord is.
///
/// Names are DESCRIPTIONS. `m9` says what it builds and cannot be wrong
/// about it. Genre names live in [`IDIOM_ALIASES`] instead, because
/// "house" is a citation rather than a definition — there are many houses,
/// and someone is entitled to disagree that this is the one.
struct Idiom {
    name: &'static str,
    /// Cycled over the generated chords, so a two-quality entry alternates
    /// and a one-quality entry repeats. `--q a,b` overrides it.
    qualities: &'static [&'static str],
    /// Semitones from each chord to the next. Chromatic means one.
    step: i16,
    gloss: &'static str,
}

/// Twelve, one per ninth-chord quality worth vamping on. Direction, length,
/// register, count and spacing are modifiers rather than more entries —
/// otherwise the catalogue is a hundred rows and none of them teach you the
/// axis they vary along.
const IDIOMS: &[Idiom] = &[
    Idiom {
        name: "m9",
        qualities: &["m9no5"],
        step: -1,
        gloss: "minor 9th — the deep-house vamp",
    },
    Idiom {
        name: "maj9",
        qualities: &["M9no5"],
        step: -1,
        gloss: "major 9th — lush, lifted",
    },
    Idiom {
        name: "dom9",
        qualities: &["9no5"],
        step: -1,
        gloss: "dominant 9th — the funk slide",
    },
    Idiom {
        name: "sus9",
        qualities: &["9sus4no5"],
        step: -1,
        gloss: "9sus4 — no third to commit you",
    },
    Idiom {
        name: "m11",
        qualities: &["m11no5"],
        step: -1,
        gloss: "minor 11th — wider, hazier",
    },
    Idiom {
        name: "m13",
        qualities: &["m13no5"],
        step: -1,
        gloss: "minor 13th — the whole stack",
    },
    Idiom {
        name: "add9",
        qualities: &["add9"],
        step: -1,
        gloss: "triad plus 9 — no seventh, bright",
    },
    Idiom {
        name: "six9",
        qualities: &["6add9"],
        step: -1,
        gloss: "6/9 — landed, going nowhere",
    },
    Idiom {
        name: "m69",
        qualities: &["m6add9no5"],
        step: -1,
        gloss: "minor 6/9 — dorian, not aeolian",
    },
    Idiom {
        name: "mmaj9",
        qualities: &["minMaj9no5"],
        step: -1,
        gloss: "minor-major 9th — the uneasy one",
    },
    Idiom {
        name: "alt9",
        qualities: &["7#9no5"],
        step: -1,
        gloss: "7#9 — grit inside the vamp",
    },
    Idiom {
        name: "half9",
        qualities: &["m9b5"],
        step: -1,
        gloss: "half-diminished 9th — unresolved",
    },
];

/// Genre names, kept apart from the catalogue on purpose. An alias is a
/// citation: it points at a practice rather than describing an interval, so
/// it is the part of this vocabulary that can be argued with.
const IDIOM_ALIASES: &[(&str, &str, i16)] = &[
    ("house", "m9", -1),
    ("lift", "m9", 1),
    ("deep", "maj9", -1),
    ("garage", "sus9", -1),
];

const IDIOM_MAX_CHORDS: usize = 8;

fn find_idiom(name: &str) -> Option<(&'static Idiom, i16)> {
    if let Some(idiom) = IDIOMS.iter().find(|idiom| idiom.name == name) {
        return Some((idiom, idiom.step));
    }
    let (_, target, step) = IDIOM_ALIASES.iter().find(|(alias, ..)| *alias == name)?;
    IDIOMS
        .iter()
        .find(|idiom| idiom.name == *target)
        .map(|idiom| (idiom, *step))
}

/// Expand every `@idiom` in the entry into the chord language proper.
///
/// The rule that makes this a shorthand rather than a black box: an idiom
/// emits tokens the user could have typed themselves. `@house f` becomes
/// `4 !fm9no5:1bar 3 !em9no5:1bar cluster`, the preview line shows exactly
/// that, and every part of it is then editable by hand. A symbol that
/// cannot be unfolded teaches nothing and varies only in the ways it was
/// told to vary.
fn expand_idioms(source: &str, cursor_pitch: u8) -> Result<String, String> {
    let tokens: Vec<&str> = source.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0;
    // Clustering is a property of the whole entry, not of one idiom, because
    // that is how `cluster` and `voicelead` already work. So two idioms that
    // disagree about it is a question with no answer, and the entry says so
    // rather than quietly clustering something that asked not to be.
    let mut wants_cluster: Option<bool> = None;

    while cursor < tokens.len() {
        let Some(name) = tokens[cursor].strip_prefix('@') else {
            out.push(tokens[cursor].to_owned());
            cursor += 1;
            continue;
        };
        let (idiom, default_step) = find_idiom(name).ok_or_else(|| {
            let known: Vec<&str> = IDIOMS.iter().map(|idiom| idiom.name).collect();
            format!("unknown idiom `@{name}`; try @{}", known.join(", @"))
        })?;
        cursor += 1;

        // Register and root both default to wherever the cursor is parked,
        // which is what every other head in this language does. A root that
        // spells its own octave (`@house f3`) outranks both.
        let mut octave = i16::from(cursor_pitch / 12) - 1;
        let mut root_pc = cursor_pitch % 12;
        if let Some(token) = tokens.get(cursor)
            && !token.starts_with(['-', '!', '@', '|', ':'])
        {
            let root = ChordSymbol::parse(token).map_err(|_| {
                format!("`@{name}` wants a note name such as f or c#, not `{token}`")
            })?;
            root_pc = root.root_pc;
            if let Some(spelled) = root.root_octave {
                octave = spelled;
            }
            cursor += 1;
        }

        let mut step = default_step;
        let mut chords = 2usize;
        let mut duration = "1bar".to_owned();
        let mut qualities: Vec<String> = idiom
            .qualities
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        let mut clustered = true;

        while let Some(&modifier) = tokens.get(cursor) {
            if modifier == "--nocluster" {
                clustered = false;
                cursor += 1;
                continue;
            }
            if !matches!(
                modifier,
                "--step" | "--n" | "--dur" | "--len" | "--oct" | "--q"
            ) {
                break;
            }
            let value = tokens
                .get(cursor + 1)
                .ok_or_else(|| format!("`{modifier}` needs a value"))?;
            match modifier {
                "--step" => {
                    // Named intervals and bare semitones both, because the
                    // musician thinks `-m2` and the arithmetic wants -1.
                    let interval = theory::parse_interval(value)?;
                    if !(-12..=12).contains(&interval) {
                        return Err(format!(
                            "`--step` reaches at most an octave, not {interval} semitones"
                        ));
                    }
                    step = interval;
                }
                "--n" => {
                    chords = value
                        .parse::<usize>()
                        .ok()
                        .filter(|count| (1..=IDIOM_MAX_CHORDS).contains(count))
                        .ok_or_else(|| {
                            format!("`--n` is 1 to {IDIOM_MAX_CHORDS} chords, not `{value}`")
                        })?;
                }
                "--dur" | "--len" => duration = (*value).to_owned(),
                "--oct" => {
                    octave = value
                        .parse::<i16>()
                        .map_err(|_| format!("invalid octave `{value}`"))?;
                }
                "--q" => {
                    qualities = value
                        .split(',')
                        .filter(|quality| !quality.is_empty())
                        .map(|quality| quality.to_owned())
                        .collect();
                    if qualities.is_empty() {
                        return Err("`--q` needs a chord quality such as m9no5".to_owned());
                    }
                }
                _ => unreachable!(),
            }
            cursor += 2;
        }

        let base = (octave + 1) * 12 + i16::from(root_pc);
        for index in 0..chords {
            let pitch = base + step * index as i16;
            if !(0..=i16::from(theory::MAX_PITCH)).contains(&pitch) {
                return Err(format!(
                    "`@{name}` walks off the keyboard after {index} chords; try a smaller --n or --step"
                ));
            }
            let quality = &qualities[index % qualities.len()];
            out.push(format!("{}", pitch / 12 - 1));
            out.push(format!(
                "!{}{quality}:{duration}",
                theory::pitch_class_name((pitch % 12) as u8).to_lowercase()
            ));
        }
        if wants_cluster.is_some_and(|wish| wish != clustered) {
            return Err(
                "`--nocluster` applies to the whole entry, so every idiom in it must agree"
                    .to_owned(),
            );
        }
        wants_cluster = Some(clustered);
    }

    match wants_cluster {
        Some(true) if !out.iter().any(|token| token == "cluster") => {
            out.push("cluster".to_owned());
        }
        Some(false) if out.iter().any(|token| token == "cluster") => {
            return Err("this entry both asks for `cluster` and passes `--nocluster`".to_owned());
        }
        _ => {}
    }

    Ok(out.join(" "))
}

/// Parse the terse chord-entry language into ordinary clip notes.
///
/// Supported first slice:
///
/// ```text
/// 2 !cM9 i 2
/// 3 !cm9
/// 2 !cM9:e. r:q 3 !am9:q
/// ```
///
/// Octaves use the DAW's documented convention (`C4 = MIDI 60`). Chord
/// durations and rests advance the insertion point; an omitted duration is
/// one piano-roll grid step. `|` is an optional visual bar separator.
fn parse_chord_entry(
    source: &str,
    cursor_pitch: u8,
    cursor_beat: f64,
    grid: f64,
    key: Key,
    beats_per_bar: u32,
) -> Result<ChordEntryPlan, String> {
    // Idioms expand FIRST, into tokens the rest of this function already
    // knows how to read. There is no second parser and no privileged path:
    // `@house f` is exactly as powerful as what it unfolds to, which is the
    // whole point of it unfolding.
    let source = expand_idioms(source, cursor_pitch)?;
    let source = source.as_str();

    let voice_lead = source
        .split_whitespace()
        .any(|token| matches!(token, "voicelead" | "--voicelead"));
    let clustered = source
        .split_whitespace()
        .any(|token| matches!(token, "cluster" | "--cluster"));
    let tokens: Vec<&str> = source
        .split_whitespace()
        .filter(|token| {
            !matches!(
                *token,
                "voicelead" | "--voicelead" | "cluster" | "--cluster"
            )
        })
        .collect();
    if tokens.is_empty() {
        return Err("type a chord, for example 2 !cM9 i 2".to_owned());
    }

    let mut notes = Vec::new();
    let mut clip_length = None;
    let mut at = cursor_beat;
    let mut token = 0;
    let mut events = 0;
    while token < tokens.len() {
        if tokens[token] == "|" {
            token += 1;
            continue;
        }
        if tokens[token] == "clip" {
            if clip_length.is_some() {
                return Err("use at most one clip-length command per entry".to_owned());
            }
            let action = tokens
                .get(token + 1)
                .ok_or_else(|| "`clip` needs len, extend, trim, or fit".to_owned())?;
            let (edit, used) = match *action {
                "fit" => (ClipLengthEdit::Fit, 2),
                "len" | "set" | "extend" | "trim" => {
                    let value = tokens
                        .get(token + 2)
                        .ok_or_else(|| format!("`clip {action}` needs a duration"))?;
                    let duration = parse_chord_duration(value, grid, beats_per_bar)? as f32;
                    let edit = match *action {
                        "len" | "set" => ClipLengthEdit::Set(duration),
                        "extend" => ClipLengthEdit::Extend(duration),
                        "trim" => ClipLengthEdit::Trim(duration),
                        _ => unreachable!(),
                    };
                    (edit, 3)
                }
                value => (
                    ClipLengthEdit::Set(parse_chord_duration(value, grid, beats_per_bar)? as f32),
                    2,
                ),
            };
            clip_length = Some(edit);
            token += used;
            events += 1;
            continue;
        }
        if events >= CHORD_ENTRY_MAX_EVENTS {
            return Err(format!(
                "at most {CHORD_ENTRY_MAX_EVENTS} chord/rest events per entry"
            ));
        }

        let mut octave = i16::from(cursor_pitch / 12) - 1;
        if !tokens[token].starts_with('!')
            && !tokens[token].starts_with('r')
            && let Ok(parsed) = tokens[token].parse::<i16>()
        {
            octave = parsed;
            token += 1;
            if token >= tokens.len() {
                return Err("an octave must be followed by a chord such as !cM9".to_owned());
            }
        }

        let head = tokens[token];
        if head == "r" || head.starts_with("r:") {
            let duration = if let Some((_, value)) = head.split_once(':') {
                parse_chord_duration(value, grid, beats_per_bar)?
            } else if tokens
                .get(token + 1)
                .is_some_and(|next| next.starts_with(':'))
            {
                token += 1;
                parse_chord_duration(tokens[token].trim_start_matches(':'), grid, beats_per_bar)?
            } else {
                grid
            };
            at += duration;
            token += 1;
            events += 1;
            continue;
        }

        let Some(chord_text) = head.strip_prefix('!') else {
            return Err(format!(
                "expected !chord or rest at `{head}`; try !cM9 or r:q"
            ));
        };
        let (symbol, inline_duration) = chord_text
            .rsplit_once(':')
            .map_or((chord_text, None), |(symbol, duration)| {
                (symbol, Some(duration))
            });
        if symbol.is_empty() {
            return Err("`!` needs a chord symbol, for example !cm9".to_owned());
        }

        let resolved_symbol = resolve_roman_chord(symbol, key)?;
        let chord = ChordSymbol::parse(&resolved_symbol)
            .map_err(|why| format!("{why}; try cM9, cm9, f#7, or bbM7"))?;
        let mut voicing = Voicing {
            octave,
            ..Voicing::default()
        };
        let mut duration = inline_duration
            .map(|value| parse_chord_duration(value, grid, beats_per_bar))
            .transpose()?
            .unwrap_or(grid);
        let mut velocity = VELOCITY_DEFAULT;
        let mut gate = 1.0f64;
        token += 1;

        // Modifiers may be spaced (`i 2`, `: e.`) or compact (`i2`, `:e.`),
        // and may appear in either order.
        while let Some(&modifier) = tokens.get(token) {
            if modifier == "i" {
                let value = tokens
                    .get(token + 1)
                    .ok_or_else(|| "`i` needs an inversion number".to_owned())?;
                voicing.inversion = parse_inversion(value)?;
                token += 2;
            } else if let Some(value) = modifier.strip_prefix('i')
                && !value.is_empty()
            {
                voicing.inversion = parse_inversion(value)?;
                token += 1;
            } else if modifier == ":" {
                let value = tokens
                    .get(token + 1)
                    .ok_or_else(|| "`:` needs a duration such as q or e.".to_owned())?;
                duration = parse_chord_duration(value, grid, beats_per_bar)?;
                token += 2;
            } else if let Some(value) = modifier.strip_prefix(':')
                && !value.is_empty()
            {
                duration = parse_chord_duration(value, grid, beats_per_bar)?;
                token += 1;
            } else if matches!(
                modifier,
                "--inv" | "--oct" | "--len" | "--dur" | "--vel" | "--gate"
            ) {
                let value = tokens
                    .get(token + 1)
                    .ok_or_else(|| format!("`{modifier}` needs a value"))?;
                match modifier {
                    "--inv" => voicing.inversion = parse_inversion(value)?,
                    "--oct" => {
                        voicing.octave = value
                            .parse::<i16>()
                            .map_err(|_| format!("invalid octave `{value}`"))?;
                    }
                    "--len" | "--dur" => {
                        duration = parse_chord_duration(value, grid, beats_per_bar)?;
                    }
                    "--vel" => {
                        velocity = value
                            .parse::<u8>()
                            .ok()
                            .filter(|value| *value > 0 && *value <= 127)
                            .ok_or_else(|| "velocity must be between 1 and 127".to_owned())?;
                    }
                    "--gate" => gate = parse_percent(value, "gate")?,
                    _ => unreachable!(),
                }
                token += 2;
            } else {
                break;
            }
        }

        let pitches = chord
            .pitches(voicing)
            .map_err(|why| format!("`{symbol}`: {why}"))?;
        for pitch in pitches {
            notes.push(Note {
                pitch,
                start: at,
                len: duration * gate,
                vel: velocity,
                muted: false,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            });
        }
        at += duration;
        events += 1;
    }

    if notes.is_empty() && clip_length.is_none() {
        return Err("the entry contains rests but no chord to write".to_owned());
    }
    if voice_lead {
        voice_lead_progression(&mut notes);
    }
    if clustered {
        cluster_progression(&mut notes);
    }
    Ok(ChordEntryPlan {
        notes,
        advance: at - cursor_beat,
        clip_length,
    })
}

/// Re-voice every simultaneity as a cluster.
///
/// Runs AFTER `voicelead` deliberately. Voice leading chooses a register by
/// rotating inversions, and a cluster then discards that spacing; doing it
/// in this order means the two compose — voicelead picks the octave,
/// cluster picks the spacing — instead of the second silently undoing the
/// first.
///
/// Rebuilt rather than mutated in place because a cluster collapses
/// doublings, so a chord can come out with FEWER notes than it went in
/// with. Every voice of one chord shares its length, velocity and
/// probability, so the group's first note serves as the template.
fn cluster_progression(notes: &mut Vec<Note>) {
    let mut starts: Vec<f64> = notes.iter().map(|note| note.start).collect();
    starts.sort_by(f64::total_cmp);
    starts.dedup_by(|a, b| a.total_cmp(b).is_eq());

    let mut voiced: Vec<Note> = Vec::with_capacity(notes.len());
    for start in starts {
        let group: Vec<&Note> = notes.iter().filter(|note| note.start == start).collect();
        let Some(template) = group.first().copied().cloned() else {
            continue;
        };
        let pitches: Vec<u8> = group.iter().map(|note| note.pitch).collect();
        for pitch in theory::cluster(&pitches) {
            voiced.push(Note {
                pitch,
                ..template.clone()
            });
        }
    }
    *notes = voiced;
}

fn parse_percent(value: &str, name: &str) -> Result<f64, String> {
    let number = value.trim_end_matches('%');
    let percent = number
        .parse::<f64>()
        .map_err(|_| format!("invalid {name} `{value}`"))?;
    if !(0.0..=100.0).contains(&percent) || percent == 0.0 {
        return Err(format!("{name} must be greater than 0% and at most 100%"));
    }
    Ok(percent / 100.0)
}

/// Revoice each chord after the first for the least pitch movement. Candidate
/// voicings are bounded (all inversions, plus/minus two octaves), so this is
/// deterministic and cannot turn preview into an unbounded UI-frame path.
fn voice_lead_progression(notes: &mut [Note]) {
    let mut starts: Vec<f64> = notes.iter().map(|note| note.start).collect();
    starts.sort_by(f64::total_cmp);
    starts.dedup_by(|a, b| a.total_cmp(b).is_eq());
    let mut previous: Option<Vec<u8>> = None;

    for start in starts {
        let mut indices: Vec<usize> = notes
            .iter()
            .enumerate()
            .filter_map(|(index, note)| (note.start == start).then_some(index))
            .collect();
        indices.sort_by_key(|index| notes[*index].pitch);
        let base: Vec<i16> = indices
            .iter()
            .map(|index| i16::from(notes[*index].pitch))
            .collect();
        if let Some(previous) = &previous {
            let mut best: Option<(i32, Vec<i16>)> = None;
            for inversion in 0..base.len() {
                let mut inverted = base[inversion..].to_vec();
                inverted.extend(base[..inversion].iter().map(|pitch| pitch + 12));
                for octaves in -2i16..=2 {
                    let candidate: Vec<i16> =
                        inverted.iter().map(|pitch| pitch + octaves * 12).collect();
                    if candidate.iter().any(|pitch| !(0..=127).contains(pitch)) {
                        continue;
                    }
                    let motion: i32 = candidate
                        .iter()
                        .map(|pitch| {
                            previous
                                .iter()
                                .map(|old| (i32::from(*pitch) - i32::from(*old)).abs())
                                .min()
                                .unwrap_or(0)
                        })
                        .sum();
                    let tie = candidate.first().copied().unwrap_or(0) as i32;
                    let score = motion * 128 + tie;
                    if best
                        .as_ref()
                        .is_none_or(|(best_score, _)| score < *best_score)
                    {
                        best = Some((score, candidate));
                    }
                }
            }
            if let Some((_, pitches)) = best {
                for (index, pitch) in indices.iter().zip(pitches) {
                    notes[*index].pitch = pitch as u8;
                }
            }
        }
        previous = Some(indices.iter().map(|index| notes[*index].pitch).collect());
    }
}

fn parse_inversion(value: &str) -> Result<u8, String> {
    value
        .parse::<u8>()
        .map_err(|_| format!("invalid inversion `{value}`; use i 0, i 1, i 2, ..."))
}

/// Resolve a Roman chord against the project key into an absolute symbol.
/// Absolute symbols pass through unchanged. Roman case is musical: uppercase
/// is major, lowercase is minor. A Roman denominator is tonicization
/// (`V7/ii`); any other denominator is a bass and is handed on untouched, so
/// `Imaj7/3` and `I/E3` still mean what the chord parser says they mean.
fn resolve_roman_chord(symbol: &str, key: Key) -> Result<String, String> {
    let Some((accidental, degree, upper, descriptor, consumed)) = parse_roman_head(symbol) else {
        return Ok(symbol.to_owned());
    };

    let slash = symbol[consumed..].strip_prefix('/');
    let mut bass = String::new();
    let root_pc = match slash {
        None => roman_degree_pc(key.tonic, key.scale, degree, accidental)?,
        Some(target) => match parse_roman_head(target) {
            Some((target_acc, target_degree, _, target_desc, target_used))
                if target_used == target.len() && target_desc.is_empty() =>
            {
                let target_pc = roman_degree_pc(key.tonic, key.scale, target_degree, target_acc)?;
                // The numerator is measured in a temporary major collection
                // rooted on the target. This makes V/ii the dominant of ii,
                // not scale degree five of the original key with a
                // decorative suffix.
                roman_degree_pc(target_pc, daw::theory::Scale::Major, degree, accidental)?
            }
            _ => {
                // Not a Roman target, so it is a bass: keep it verbatim for
                // the chord parser, which owns slash-bass meaning.
                bass = format!("/{target}");
                roman_degree_pc(key.tonic, key.scale, degree, accidental)?
            }
        },
    };

    let root = [
        "c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b",
    ][usize::from(root_pc)];
    let quality = if descriptor.is_empty() {
        if upper { "" } else { "m" }
    } else if descriptor == "°" {
        "dim"
    } else if descriptor == "+" {
        "aug"
    } else if !upper && matches!(descriptor, "7" | "9" | "11" | "13" | "maj7") {
        "m"
    } else {
        ""
    };
    Ok(format!("{root}{quality}{descriptor}{bass}"))
}

/// `(accidental, zero-based degree, uppercase, descriptor, bytes consumed)`.
fn parse_roman_head(symbol: &str) -> Option<(i16, usize, bool, &str, usize)> {
    let bytes = symbol.as_bytes();
    let mut at = 0usize;
    let mut accidental = 0i16;
    while let Some(byte) = bytes.get(at) {
        match byte {
            b'b' => accidental -= 1,
            b'#' => accidental += 1,
            _ => break,
        }
        at += 1;
    }
    let start = at;
    while matches!(bytes.get(at), Some(b'i' | b'v' | b'x' | b'I' | b'V' | b'X')) {
        at += 1;
    }
    if at == start {
        return None;
    }
    let roman = &symbol[start..at];
    let upper = roman.chars().all(|c| c.is_ascii_uppercase());
    let degree = match roman.to_ascii_uppercase().as_str() {
        "I" => 0,
        "II" => 1,
        "III" => 2,
        "IV" => 3,
        "V" => 4,
        "VI" => 5,
        "VII" => 6,
        _ => return None,
    };
    let end = symbol[at..]
        .find('/')
        .map_or(symbol.len(), |slash| at + slash);
    Some((accidental, degree, upper, &symbol[at..end], end))
}

fn roman_degree_pc(
    tonic: u8,
    scale: daw::theory::Scale,
    degree: usize,
    accidental: i16,
) -> Result<u8, String> {
    let Some(offset) = scale.degrees().get(degree) else {
        return Err(format!(
            "degree {} is outside the {} scale",
            degree + 1,
            scale.label()
        ));
    };
    Ok((i16::from(tonic) + offset + accidental).rem_euclid(12) as u8)
}

/// Convert a musical duration to quarter-note beats.
fn parse_chord_duration(value: &str, grid: f64, beats_per_bar: u32) -> Result<f64, String> {
    let mut value = value.trim();
    if value.is_empty() {
        return Err("missing duration; try q, e., 1/16, or grid".to_owned());
    }
    let triplet = value.ends_with('t') && !value.ends_with("beat");
    if triplet {
        value = &value[..value.len() - 1];
    }
    let dots = value.chars().rev().take_while(|&c| c == '.').count();
    if dots > 2 {
        return Err("durations support at most two dots".to_owned());
    }
    value = &value[..value.len() - dots];

    let base = match value {
        "grid" => grid,
        "w" => 4.0,
        "h" => 2.0,
        "q" => 1.0,
        "e" => 0.5,
        "s" => 0.25,
        "32" => 0.125,
        beats if beats.ends_with("beat") => beats
            .trim_end_matches("beat")
            .parse::<f64>()
            .map_err(|_| format!("invalid duration `{beats}`"))?,
        bars if bars.ends_with("bar") => {
            bars.trim_end_matches("bar")
                .parse::<f64>()
                .map_err(|_| format!("invalid duration `{bars}`"))?
                * f64::from(beats_per_bar.max(1))
        }
        fraction if fraction.contains('/') => {
            let (numerator, denominator) = fraction
                .split_once('/')
                .ok_or_else(|| format!("invalid duration `{fraction}`"))?;
            let numerator = numerator
                .parse::<f64>()
                .map_err(|_| format!("invalid duration `{fraction}`"))?;
            let denominator = denominator
                .parse::<f64>()
                .map_err(|_| format!("invalid duration `{fraction}`"))?;
            if numerator <= 0.0 || denominator <= 0.0 {
                return Err("duration fractions must be positive".to_owned());
            }
            4.0 * numerator / denominator
        }
        _ => {
            return Err(format!(
                "unknown duration `{value}`; try q, e., 1/16, or grid"
            ));
        }
    };
    let dotted = (0..dots)
        .fold((1.0, 0.5), |(sum, add), _| (sum + add, add * 0.5))
        .0;
    let duration = base * dotted * if triplet { 2.0 / 3.0 } else { 1.0 };
    if !duration.is_finite() || duration <= 0.0 {
        return Err("duration must be finite and greater than zero".to_owned());
    }
    Ok(duration)
}
/// One press of , or .
const VELOCITY_STEP: i32 = 10;
/// Velocity floor: 0 is a note-off in MIDI, so editing never produces it.
const VELOCITY_MIN: u8 = 1;

// --- accelerated scrolling ----------------------------------------------

/// Scroll events closer together than this feed the accelerator.
const ACCEL_WINDOW: f64 = 0.25;
/// Each event inside the window multiplies the factor by this...
const ACCEL_GROWTH: f32 = 1.25;
/// ...up to here. 128 rows is a long way; a pause resets to 1x.
const ACCEL_MAX: f32 = 4.0;

/// What the roll shows when no clip is selected.
const NO_CLIP: &str = "no clip selected — double-click a lane to make one";

/// A drag in flight.
///
/// Every variant snapshots what it needs AT THE PRESS and recomputes from
/// that origin each frame, so nothing drifts and nothing accumulates. What
/// changed from the first version of this enum is that the note-moving
/// variants carry the whole SELECTION rather than one index: selecting
/// eight notes and dragging one used to leave seven behind.
///
/// The device UI contract's rule 1 is honoured structurally — the grid
/// background and the note layer are separate `ui::interact` targets, so
/// a press that lands on a note can never become a marquee, whatever the
/// pointer does afterwards.
enum Drag {
    /// Moving notes. `from` is `(index, pitch, start)` for every note in
    /// the selection at the moment of the press; `anchor` is the one under
    /// the pointer, whose motion the rest follow.
    Move {
        anchor: usize,
        from: Vec<(usize, u8, f64)>,
        press: egui::Pos2,
        /// Which axis a Shift-constrained drag committed to, decided once
        /// at `AXIS_LOCK_PX` of travel and then held for the gesture.
        axis: Option<Axis>,
    },
    /// Pulling right edges. `from` is `(index, len)` per note.
    ResizeR {
        from: Vec<(usize, f64)>,
        press: egui::Pos2,
    },
    /// Pulling left edges: start and length move together, so the right
    /// edge stays where it is. `from` is `(index, start, len)`.
    ResizeL {
        from: Vec<(usize, f64, f64)>,
        press: egui::Pos2,
    },
    /// Rubber-band selection. `base` is the selection to add to, empty
    /// unless the gesture began with Shift.
    Marquee {
        press: egui::Pos2,
        base: HashSet<usize>,
    },
    /// Painting one expression lane: which lane, and the notes claimed at
    /// the press. `ramp` remembers the press point so a Shift-drag can
    /// draw a straight line rather than follow the hand.
    Lane {
        lane: Lane,
        targets: Vec<usize>,
        press: egui::Pos2,
        ramp: bool,
    },
    /// Dragging a lane's top edge to resize it.
    LaneResize {
        idx: usize,
        h0: f32,
        press: egui::Pos2,
    },
    /// Sweeping the erase or mute tool across notes.
    Sweep {
        verb: SweepVerb,
        /// What this pass has already touched, so a wobbling hand does not
        /// toggle the same note twice.
        done: HashSet<usize>,
    },
}

/// What a sweep does to each note it crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SweepVerb {
    Erase,
    Mute,
    Split,
}

/// Which way a constrained drag was committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Time,
    Pitch,
}

/// How far a Shift-constrained drag travels before it decides its axis.
/// Small enough to feel immediate, large enough that the first two pixels
/// of hand tremor do not choose for the user.
const AXIS_LOCK_PX: f32 = 4.0;

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
    /// The parameter-lock editor: which note it is open on, and which
    /// row of the parameter list the keyboard is holding. `None` closed.
    plock_view: Option<(usize, usize)>,
    /// The first row the lock panel is showing. Kept so the list scrolls
    /// only when the selection would leave the window, rather than every
    /// time it moves.
    plock_scroll: usize,
    /// The TRIG editor: which note, and which of its two rows
    /// (probability, condition) the keyboard is holding. `None` closed.
    /// The two editors are exclusive — opening one closes the other.
    trig_view: Option<(usize, usize)>,
    /// `i` opens this keyboard-owned chord command line. It remains purely
    /// a preview until Enter commits the complete plan in one mutation.
    chord_entry: Option<ChordEntry>,
    /// Which workspace occupies the large well above music-script input.
    /// It survives closing the palette so reopening returns to the page the
    /// musician was using.
    script_page: MusicScriptPage,
    /// A committed clip-length verb for the app to apply after the roll
    /// releases its mutable note borrow.
    pending_clip_length: Option<ClipLengthEdit>,
    /// Set by `keys` when a keystroke moved the cursor; `body` spends it
    /// by scrolling just enough to keep the cell in view. A keyboard
    /// cursor that can walk off screen is a cursor you then hunt for
    /// with the mouse, which defeats the whole point of having one.
    follow_cursor: bool,
    /// Accelerated scrolling: when the last event landed, and the factor it
    /// had earned.
    last_scroll: f64,
    accel: f32,
    drag: Option<Drag>,

    // --- the pointer's verb ------------------------------------------
    /// The LATCHED tool — what a tap on a tool key or a click in the tool
    /// column set. Held keys borrow a different one for their duration
    /// without disturbing this.
    pub tool: Tool,
    /// A tool borrowed by a held key. Not persisted: the whole point is
    /// that it lasts exactly as long as the finger does.
    held_tool: Option<Tool>,
    /// When that key went down. A SHORT press latches the tool on release;
    /// a long one was a borrow and gives it back. Cubase's rule, and the
    /// reason a tool palette never has to be clicked.
    held_since: f64,

    // --- what the pointer is on --------------------------------------
    /// The note under the pointer and which of its zones, recomputed every
    /// frame and never trusted across one. Purely visual — hover may not
    /// change what any key does.
    hover: Option<(usize, Zone)>,
    /// Where the pointer last was inside the grid, in musical coordinates.
    /// Feeds the info line when nothing is hovered and nothing selected.
    hover_cell: Option<(u8, f64)>,

    // --- the lanes ----------------------------------------------------
    /// Open expression lanes, top to bottom. Empty is legal and gives the
    /// grid the whole panel.
    pub lanes: Vec<LaneView>,

    // --- the writing surface ------------------------------------------
    /// Fold the grid to the pitches the clip actually uses.
    pub fold: bool,
    /// Constrain every pitch-changing gesture to the working key. Off by
    /// default, and always defeated by the snap-bypass modifier — a
    /// constraint with no escape is a cage.
    pub scale_lock: bool,
    /// The length a drawn note gets, in beats. `None` means "one grid
    /// step", which is the default and what most people want; a number is
    /// the fixed length the user locked in.
    pub draw_len: Option<f64>,

    // --- the playhead --------------------------------------------------
    /// When the user last scrolled by hand. Follow stands down for
    /// `FOLLOW_YIELD` seconds afterwards, because a view that fights the
    /// hand is worse than a view that does not follow at all.
    last_user_scroll: f64,
    /// The working key, as of the last frame the roll drew.
    ///
    /// `keys` runs BEFORE `body` in the frame — that ordering is what
    /// lets the roll stand down for the rest of the app — so a verb that
    /// needs the key reads the one the roll last painted against. It is
    /// the same one-frame handshake `owns_keys` already uses, and the key
    /// does not change between two frames without the user seeing it.
    last_key: Key,
    /// The seed the randomised transforms run from.
    ///
    /// It lives in view state and advances on each use, so two presses of
    /// humanize give two different passes while any ONE pass is exactly
    /// reproducible from the seed it used — which is what makes a
    /// randomised verb safe to put in a bounce.
    pub seed: u32,
    /// A locate the ruler asked for, in CLIP-RELATIVE beats.
    ///
    /// The roll does not own a transport and must not pretend to. It
    /// leaves the request here; the app takes it with [`take_locate`],
    /// adds the clip's start, and moves the insert marker. Until the app
    /// wires that up the ruler still moves the cursor, which is the half
    /// of the gesture the roll owns outright.
    ///
    /// [`take_locate`]: PianoRoll::take_locate
    locate: Option<f64>,
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
            plock_view: None,
            plock_scroll: 0,
            trig_view: None,
            chord_entry: None,
            script_page: MusicScriptPage::default(),
            pending_clip_length: None,
            follow_cursor: false,
            last_scroll: f64::NEG_INFINITY,
            accel: 1.0,
            drag: None,
            tool: Tool::Pointer,
            held_tool: None,
            held_since: 0.0,
            hover: None,
            hover_cell: None,
            // One lane open at the start, and it is velocity: it is the
            // property people reach for, and an editor that opens with no
            // lanes at all reads as an editor that does not have them.
            lanes: vec![LaneView {
                lane: Lane::Velocity,
                h: LANE_H,
                collapsed: false,
            }],
            fold: false,
            scale_lock: false,
            draw_len: None,
            last_user_scroll: f64::NEG_INFINITY,
            last_key: Key::default(),
            seed: 0x5eed_1234,
            locate: None,
        }
    }
}

/// Which part of a note the pointer is over. Three zones, and below
/// [`ZONE_MIN_W`] the body takes the whole note — a note too small to
/// have three zones must not pretend that it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Body,
    Right,
}

/// The narrowest note that still offers resize zones.
const ZONE_MIN_W: f32 = 18.0;

/// The longest a tool key press can last and still count as a TAP, in
/// seconds. Above it the press was a borrow and the tool is handed back.
const TOOL_TAP: f64 = 0.25;

/// How long follow stands down after a hand scroll, in seconds.
const FOLLOW_YIELD: f64 = 2.0;

/// Which zone of `r` the point `at` is in.
fn zone_of(r: egui::Rect, at: egui::Pos2) -> Zone {
    if r.width() < ZONE_MIN_W {
        return Zone::Body;
    }
    if at.x <= r.left() + EDGE_W {
        Zone::Left
    } else if at.x >= r.right() - EDGE_W {
        Zone::Right
    } else {
        Zone::Body
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

/// Sharps and flats — the rows drawn darker, like the keys they mirror.
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

fn script_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    page: &mut MusicScriptPage,
    value: MusicScriptPage,
    label: &str,
) {
    let selected = *page == value;
    let response = ui.selectable_label(
        selected,
        egui::RichText::new(label)
            .size(font::MINI_LABEL)
            .strong()
            .color(if selected {
                theme.accent
            } else {
                theme.text_muted
            }),
    );
    if response.clicked() {
        *page = value;
    }
}

/// An edge-to-edge Tonnetz: fifths run horizontally and major thirds run
/// diagonally. Pitch classes repeat modulo twelve, so the lattice wraps
/// naturally instead of ending at an arbitrary C or MIDI boundary.
fn paint_wrapping_tonnetz(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    tonic: u8,
    active: &HashSet<u8>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let x_step = 58.0;
    let y_step = 48.0;
    let cols = (rect.width() / x_step).ceil() as usize + 2;
    let rows = (rect.height() / y_step).ceil() as usize + 2;
    let center_col = cols as i16 / 2;
    let center_row = rows as i16 / 2;
    let mut nodes = vec![vec![(egui::Pos2::ZERO, 0u8); cols]; rows];

    for (row, line) in nodes.iter_mut().enumerate() {
        for (col, node) in line.iter_mut().enumerate() {
            let stagger = if row % 2 == 0 { 0.0 } else { x_step * 0.5 };
            let pos = egui::pos2(
                rect.left() - x_step * 0.5 + col as f32 * x_step + stagger,
                rect.top() + row as f32 * y_step,
            );
            let fifths = col as i16 - center_col;
            let thirds = center_row - row as i16;
            let pc = (i16::from(tonic) + fifths * 7 + thirds * 4).rem_euclid(12) as u8;
            *node = (pos, pc);
        }
    }

    for row in 0..rows {
        for col in 0..cols {
            let from = nodes[row][col].0;
            for (next_row, next_col) in [
                (row, col + 1),
                (row + 1, col),
                (row + 1, col + usize::from(row % 2 == 0)),
            ] {
                if next_row < rows && next_col < cols {
                    painter.line_segment(
                        [from, nodes[next_row][next_col].0],
                        egui::Stroke::new(stroke::HAIR, theme.divider),
                    );
                }
            }
        }
    }

    for line in &nodes {
        for &(pos, pc) in line {
            let sounding = active.contains(&pc);
            let home = pc == tonic % 12;
            painter.circle_filled(
                pos,
                if sounding { 15.0 } else { 12.0 },
                if sounding {
                    theme.accent_muted
                } else {
                    theme.surface_raised
                },
            );
            painter.circle_stroke(
                pos,
                if sounding { 15.0 } else { 12.0 },
                egui::Stroke::new(
                    if home { stroke::BOLD } else { stroke::HAIR },
                    if sounding || home {
                        theme.accent
                    } else {
                        theme.outline
                    },
                ),
            );
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                daw::theory::pitch_class_name(pc),
                egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
                if sounding {
                    theme.text_value
                } else {
                    theme.text_muted
                },
            );
        }
    }
}

fn script_help(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let mut help = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("music_script_help")
        .auto_shrink([false, false])
        .show(&mut help, |ui| {
            help_group(
                ui,
                theme,
                "CHORDS",
                "!c  !cm  !cmaj7  !cm7  !c7  !cM9  !cm9  !cdim  !caug  !csus2  !csus4  !cm7b5",
            );
            help_group(
                ui,
                theme,
                "STACKED",
                "!c13b9no5  !c7sus4  !cMaj7#11  !cmMaj7add9\nAdditions, alterations and omissions read left to right",
            );
            help_group(
                ui,
                theme,
                "BASS",
                "!cM7/e3 = absolute bass · !cM7/3 = chord member · !V7/ii = tonicization",
            );
            help_group(
                ui,
                theme,
                "ROMAN / KEY",
                "!I  !ii  !V7  !vi7  !bVIImaj7  !V7/ii\nUppercase = major · lowercase = minor · resolves against project key",
            );
            help_group(
                ui,
                theme,
                "REGISTER / INVERSION",
                "3 !am7   i 1   i2   --oct 3   --inv 1",
            );
            help_group(
                ui,
                theme,
                "TIME",
                ":w  :h  :q  :e  :s  :32  :1/16  :e.  :1/8t  :2beat  :1bar  :grid\nr:q = quarter-note rest",
            );
            help_group(
                ui,
                theme,
                "EXPRESSION",
                "--vel 110   --gate 75%   --dur 1bar   --len 1/8",
            );
            // Written from the catalogue rather than beside it: a row added
            // to IDIOMS shows up here, and a name on this page always names
            // something that parses.
            let idioms = IDIOMS
                .iter()
                .map(|idiom| format!("@{} — {}", idiom.name, idiom.gloss))
                .collect::<Vec<_>>()
                .join("\n");
            let aliases = IDIOM_ALIASES
                .iter()
                .map(|(alias, target, step)| {
                    format!("@{alias} = @{target} {step:+}")
                })
                .collect::<Vec<_>>()
                .join(" · ");
            help_group(
                ui,
                theme,
                "IDIOMS",
                &format!(
                    "@house f = two clustered minor 9ths a semitone apart\n{idioms}\n{aliases}"
                ),
            );
            help_group(
                ui,
                theme,
                "IDIOM OPTIONS",
                "--step -m2 -M2 m3 -P4 P5 -P8 · or semitones: -1 2 -7st\n--n 4   --q m9no5,M9no5   --oct 3   --dur 1bar   --nocluster",
            );
            help_group(
                ui,
                theme,
                "STRUCTURE",
                "Space separates events · | is a visual separator\nvoicelead revoices the progression · cluster packs it into seconds",
            );
            help_group(
                ui,
                theme,
                "CLIP LENGTH",
                "clip len 4bar   clip extend 1bar   clip trim 2beat   clip fit\nclip 8bar is shorthand for clip len 8bar",
            );
            help_group(
                ui,
                theme,
                "EXAMPLE",
                "3 !vi7:1bar 3 !Imaj7:1bar | voicelead\n@house f --dur 2beat --n 4",
            );
        });
}

fn help_group(ui: &mut egui::Ui, theme: &Theme, title: &str, vocabulary: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(font::MINI_LABEL)
            .strong()
            .color(theme.accent),
    );
    ui.label(
        egui::RichText::new(vocabulary)
            .size(font::LABEL)
            .monospace()
            .color(theme.text),
    );
    ui.add_space(theme.sp(space::SM));
}

// --- editing verbs, keyboard and mouse alike ----------------------------

impl PianoRoll {
    pub fn grid_beats(&self) -> f64 {
        f64::from(GRID_BEATS[self.grid.min(GRID_BEATS.len() - 1)])
    }

    /// Whether the roll currently owns typed characters as a modal command
    /// line. The app asks this before its GLOBAL shortcuts run: those are
    /// intentionally earlier than [`keys`], so consuming events inside
    /// `keys` alone cannot stop Space, `:`, or `a` leaking into transport,
    /// palette, or automation actions.
    pub fn owns_modal_input(&self) -> bool {
        self.chord_entry.is_some()
    }

    pub fn take_clip_length_edit(&mut self) -> Option<ClipLengthEdit> {
        self.pending_clip_length.take()
    }

    /// The chord language gets a real modal surface in the ARRANGEMENT,
    /// where there is enough room for the visual composer that will grow
    /// above the command field. Its two pages share fixed geometry so future
    /// tools can grow without moving the musician's typing hand.
    pub fn chord_palette(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        arrangement: egui::Rect,
        key: Key,
        beats_per_bar: u32,
    ) {
        let Some(entry) = &self.chord_entry else {
            return;
        };
        let entry_text = entry.text.clone();
        let diagnostic = entry.diagnostic.clone();
        if arrangement.width() < 120.0 || arrangement.height() < 120.0 {
            return;
        }

        let width = (arrangement.width() * 0.78).clamp(420.0, 780.0);
        let height = (arrangement.height() * 0.78).clamp(300.0, 520.0);
        let size = egui::vec2(
            width.min(arrangement.width() - theme.sp(space::MD) * 2.0),
            height.min(arrangement.height() - theme.sp(space::MD) * 2.0),
        );
        let pos = arrangement.center() - size * 0.5;
        let pad = theme.sp(space::MD);
        let field_h = 48.0;
        let diagnostic_h = 24.0;

        egui::Area::new(egui::Id::new("piano_roll_chord_palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
                egui::Frame::new()
                    .fill(theme.surface_raised)
                    .stroke(egui::Stroke::new(stroke::BOLD, theme.outline))
                    .corner_radius(radius::PANEL as u8)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 2,
                        color: egui::Color32::from_black_alpha(96),
                    })
                    .inner_margin(egui::Margin::same(pad as i8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("MUSIC SCRIPT")
                                    .size(font::LABEL)
                                    .strong()
                                    .color(theme.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new("ENTER WRITE  ·  ESC CANCEL")
                                            .size(font::MINI_LABEL)
                                            .color(theme.text_muted),
                                    );
                                },
                            );
                        });

                        ui.add_space(theme.sp(space::SM));
                        let reserve_h = (ui.available_height()
                            - field_h
                            - diagnostic_h
                            - theme.sp(space::SM) * 2.0)
                            .max(80.0);
                        let (reserve, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), reserve_h),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            reserve,
                            radius::CTRL,
                            theme.surface_sunken.gamma_multiply(0.72),
                        );
                        ui.painter().rect_stroke(
                            reserve,
                            radius::CTRL,
                            egui::Stroke::new(stroke::HAIR, theme.divider),
                            egui::StrokeKind::Inside,
                        );
                        let active_pcs: HashSet<u8> = parse_chord_entry(
                            &entry_text,
                            self.cursor_pitch,
                            self.cursor_beat,
                            self.grid_beats(),
                            key,
                            beats_per_bar,
                        )
                        .map(|plan| plan.notes.into_iter().map(|note| note.pitch % 12).collect())
                        .unwrap_or_default();
                        let mut workspace = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(reserve.shrink(theme.sp(space::SM)))
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        workspace.horizontal(|ui| {
                            script_tab(
                                ui,
                                theme,
                                &mut self.script_page,
                                MusicScriptPage::Tonnetz,
                                "TONNETZ",
                            );
                            script_tab(
                                ui,
                                theme,
                                &mut self.script_page,
                                MusicScriptPage::Help,
                                "HELP",
                            );
                        });
                        workspace.add_space(theme.sp(space::XS));
                        let content = workspace.available_rect_before_wrap();
                        match self.script_page {
                            MusicScriptPage::Tonnetz => {
                                paint_wrapping_tonnetz(
                                    &workspace,
                                    theme,
                                    content,
                                    key.tonic,
                                    &active_pcs,
                                );
                            }
                            MusicScriptPage::Help => {
                                script_help(&mut workspace, theme, content);
                            }
                        }

                        ui.add_space(theme.sp(space::SM));
                        let (field, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), field_h),
                            egui::Sense::click(),
                        );
                        ui.painter()
                            .rect_filled(field, radius::CTRL, theme.surface_sunken);
                        ui.painter().rect_stroke(
                            field,
                            radius::CTRL,
                            egui::Stroke::new(stroke::FOCUS, theme.accent),
                            egui::StrokeKind::Inside,
                        );
                        let shown = if entry_text.is_empty() {
                            "2 !cM9 i 2"
                        } else {
                            entry_text.as_str()
                        };
                        let color = if entry_text.is_empty() {
                            theme.text_muted
                        } else {
                            theme.text_value
                        };
                        let text_pos = field.left_center() + egui::vec2(pad, 0.0);
                        let painter = ui.painter().with_clip_rect(field.shrink(pad));
                        painter.text(
                            text_pos,
                            egui::Align2::LEFT_CENTER,
                            shown,
                            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                            color,
                        );
                        // The modal input is keyboard-owned, so the caret is
                        // always at the end. A slow blink makes that ownership
                        // visible without turning the field into decoration.
                        if !entry_text.is_empty() && (ctx.input(|i| i.time) * 2.0) as u64 % 2 == 0 {
                            let galley = painter.layout_no_wrap(
                                entry_text.clone(),
                                egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                                color,
                            );
                            let x = (text_pos.x + galley.size().x).min(field.right() - pad * 0.5);
                            painter.line_segment(
                                [
                                    egui::pos2(x, field.center().y - 10.0),
                                    egui::pos2(x, field.center().y + 10.0),
                                ],
                                egui::Stroke::new(stroke::BOLD, theme.accent),
                            );
                            ctx.request_repaint_after(std::time::Duration::from_millis(400));
                        }

                        let diagnostic_text = diagnostic.as_deref().unwrap_or(
                            "Chords, Roman numerals, rests, durations, voicelead and clip length",
                        );
                        ui.label(
                            egui::RichText::new(diagnostic_text)
                                .size(font::MINI_LABEL)
                                .color(if diagnostic.is_some() {
                                    theme.danger
                                } else {
                                    theme.text_muted
                                }),
                        );
                    });
            });
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
        self.plock_view = None;
        self.trig_view = None;
        self.chord_entry = None;
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
            muted: false,
            plocks: Vec::new(),
            prob: 1.0,
            cond: None,
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

    /// Move the selection AND the cursor together — the keyboard's grab.
    ///
    /// The two travel as one on purpose: the cursor is how the keyboard
    /// holds a note, and an edit that moves the note out from under it
    /// makes the very next keystroke land on empty air. Both clamp the
    /// same way (pitch to the MIDI range, beats at zero), so a selection
    /// pressed against a wall does not leave the cursor drifting on
    /// alone. With nothing selected this is a no-op — plain arrows are
    /// how the CURSOR moves.
    fn move_selected(&mut self, notes: &mut [Note], d_pitch: i32, d_beat: f64) {
        if self.selected.is_empty() {
            return;
        }
        if d_pitch != 0 {
            self.transpose_selected(notes, d_pitch);
            self.cursor_pitch = transposed(self.cursor_pitch, d_pitch);
        }
        if d_beat != 0.0 {
            self.nudge_selected(notes, d_beat);
            self.cursor_beat = (self.cursor_beat + d_beat).max(0.0);
        }
        self.anchor = None;
    }

    // --- parameter locks -------------------------------------------

    /// Shift+Enter: open the lock editor on the note under the cursor
    /// (or the single selected note), or close it if it is open. With no
    /// note to hold, nothing opens — an editor over thin air edits
    /// nothing.
    fn plock_toggle_view(&mut self, notes: &[Note]) {
        if self.plock_view.is_some() {
            self.plock_view = None;
            return;
        }
        let target = self.note_at_cursor(notes).or_else(|| {
            (self.selected.len() == 1).then(|| *self.selected.iter().next().unwrap_or(&0))
        });
        if let Some(note) = target.filter(|&i| i < notes.len()) {
            self.plock_view = Some((note, 0));
        }
    }

    /// The open editor's (note, row), validated against the clip — a
    /// deleted note closes the view rather than editing its successor.
    fn plock_at(&mut self, notes: &[Note]) -> Option<(usize, usize)> {
        match self.plock_view {
            Some((note, row)) if note < notes.len() => Some((note, row)),
            _ => {
                self.plock_view = None;
                None
            }
        }
    }

    /// Move the editor's row cursor.
    fn plock_nav(&mut self, delta: i32, rows: usize) {
        if let Some((_, row)) = &mut self.plock_view
            && rows > 0
        {
            *row = (*row as i32 + delta).clamp(0, rows as i32 - 1) as usize;
        }
    }

    /// Enter on a row: lock it at the KNOB's value, or unlock it. A
    /// fresh lock starting anywhere but the base would jump the sound on
    /// a key whose meaning is "hold this parameter here".
    fn plock_toggle_row(&mut self, notes: &mut [Note], params: &[PlockParam]) {
        let Some((note, row)) = self.plock_at(notes) else {
            return;
        };
        let (Some(n), Some(p)) = (notes.get_mut(note), params.get(row)) else {
            return;
        };
        if let Some(at) = n.plocks.iter().position(|(id, _)| *id == p.id) {
            n.plocks.remove(at);
        } else {
            n.plocks.push((p.id, p.base));
        }
    }

    /// Adjust the row's lock by a fraction of its range, creating the
    /// lock from the base if the row was unlocked — turning an unlocked
    /// row IS how a lock begins, no ceremony first.
    ///
    /// A DISCRETE row does not do fractions: any nudge is ONE whole
    /// choice in the nudge's direction — one keystroke, the next wave.
    fn plock_adjust(&mut self, notes: &mut [Note], params: &[PlockParam], fraction: f32) {
        let Some((note, row)) = self.plock_at(notes) else {
            return;
        };
        let (Some(n), Some(p)) = (notes.get_mut(note), params.get(row)) else {
            return;
        };
        let entry = match n.plocks.iter_mut().find(|(id, _)| *id == p.id) {
            Some(e) => e,
            None => {
                n.plocks.push((p.id, p.base));
                match n.plocks.last_mut() {
                    Some(e) => e,
                    None => return,
                }
            }
        };
        if p.choices > 0 {
            // An infinite nudge is Home or End: go to the rail rather
            // than one choice towards it.
            let step = if fraction.is_infinite() {
                (p.max - p.min) * fraction.signum()
            } else if fraction > 0.0 {
                1.0
            } else {
                -1.0
            };
            entry.1 = (entry.1.round() + step).clamp(p.min, p.max).round();
        } else {
            // `f32::INFINITY * 0` is NaN, so a zero-width range would
            // poison the value rather than stay put. Clamp the product,
            // not just the sum.
            let moved = entry.1 + fraction * (p.max - p.min);
            entry.1 = if moved.is_nan() {
                entry.1
            } else {
                moved.clamp(p.min, p.max)
            };
        }
    }

    /// Delete on a row: the lock goes, the knob resumes.
    fn plock_remove(&mut self, notes: &mut [Note], params: &[PlockParam]) {
        let Some((note, row)) = self.plock_at(notes) else {
            return;
        };
        let (Some(n), Some(p)) = (notes.get_mut(note), params.get(row)) else {
            return;
        };
        n.plocks.retain(|(id, _)| *id != p.id);
    }

    // --- trig conditions -------------------------------------------

    /// Every A:B condition the editor steps through, Elektron's ladder:
    /// index 0 is "every cycle", then 1:2, 2:2, 1:3 … 8:8.
    pub fn trig_conditions() -> Vec<Option<(u8, u8)>> {
        let mut out = vec![None];
        for b in 2..=8u8 {
            for a in 1..=b {
                out.push(Some((a, b)));
            }
        }
        out
    }

    /// Ctrl+Shift+Enter: open the trig editor on the held note, or close
    /// it. Exclusive with the plock editor — two floating panels over one
    /// note is a fight, not a UI.
    fn trig_toggle_view(&mut self, notes: &[Note]) {
        if self.trig_view.is_some() {
            self.trig_view = None;
            return;
        }
        self.plock_view = None;
        let target = self.note_at_cursor(notes).or_else(|| {
            (self.selected.len() == 1).then(|| *self.selected.iter().next().unwrap_or(&0))
        });
        if let Some(note) = target.filter(|&i| i < notes.len()) {
            self.trig_view = Some((note, 0));
        }
    }

    /// The open trig editor's (note, row), validated like the plocks'.
    fn trig_at(&mut self, notes: &[Note]) -> Option<(usize, usize)> {
        match self.trig_view {
            Some((note, row)) if note < notes.len() => Some((note, row)),
            _ => {
                self.trig_view = None;
                None
            }
        }
    }

    /// Adjust the held trig row. Probability sweeps in fractions;
    /// the condition steps ONE rung of the ladder per nudge, whole,
    /// clamped at both ends.
    fn trig_adjust(&mut self, notes: &mut [Note], fraction: f32) {
        let Some((note, row)) = self.trig_at(notes) else {
            return;
        };
        let Some(n) = notes.get_mut(note) else {
            return;
        };
        if row == 0 {
            n.prob = (n.prob + fraction).clamp(0.0, 1.0);
        } else {
            let ladder = Self::trig_conditions();
            let at = ladder.iter().position(|c| *c == n.cond).unwrap_or(0);
            let step = if fraction > 0.0 { 1i32 } else { -1 };
            let next = (at as i32 + step).clamp(0, ladder.len() as i32 - 1) as usize;
            n.cond = ladder[next];
        }
    }

    /// Delete on a trig row: back to a plain note — probability 1, no
    /// condition.
    fn trig_clear(&mut self, notes: &mut [Note]) {
        let Some((note, row)) = self.trig_at(notes) else {
            return;
        };
        let Some(n) = notes.get_mut(note) else {
            return;
        };
        if row == 0 {
            n.prob = 1.0;
        } else {
            n.cond = None;
        }
    }

    /// The note whose row and span the cursor cell sits inside, if any.
    fn note_at_cursor(&self, notes: &[Note]) -> Option<usize> {
        notes.iter().position(|n| {
            n.pitch == self.cursor_pitch
                && n.start <= self.cursor_beat
                && self.cursor_beat < n.start + n.len
        })
    }

    /// Enter: SMART. On an empty cell it adds a note; on a note it
    /// toggles that note's selection. The distinction is what makes the
    /// keyboard complete — without it there is no way to pick up an
    /// existing note from the cursor, and pressing Enter on one stacked a
    /// silent duplicate on top of it.
    fn enter_at_cursor(&mut self, notes: &mut Vec<Note>) {
        match self.note_at_cursor(notes) {
            Some(i) => {
                if !self.selected.remove(&i) {
                    self.selected.insert(i);
                }
            }
            None => {
                let (pitch, beat) = (self.cursor_pitch, self.cursor_beat);
                self.add_note_at(notes, pitch, beat);
            }
        }
    }

    /// N / Shift+N: the cursor jumps to the next (or previous) note start
    /// and selects that note alone — walking the material note by note,
    /// each stop ready for [ ] , . or a nudge.
    fn jump_to_note(&mut self, notes: &[Note], forward: bool) {
        let best = notes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                if forward {
                    n.start > self.cursor_beat + 1e-9
                        || (n.start == self.cursor_beat && n.pitch != self.cursor_pitch)
                } else {
                    n.start + 1e-9 < self.cursor_beat
                }
            })
            .min_by(|(_, a), (_, b)| {
                let da = (a.start - self.cursor_beat).abs();
                let db = (b.start - self.cursor_beat).abs();
                da.total_cmp(&db).then(a.pitch.cmp(&b.pitch))
            });
        if let Some((i, n)) = best {
            self.cursor_beat = n.start;
            self.cursor_pitch = n.pitch;
            self.selected.clear();
            self.selected.insert(i);
            self.anchor = None;
        }
    }

    /// Ableton's `0`: deactivate. A mixed selection mutes rather than
    /// flips per note — one press, one audible outcome.
    fn toggle_mute_selected(&mut self, notes: &mut [Note]) {
        let any_live = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i))
            .any(|n| !n.muted);
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.muted = any_live;
            }
        }
    }

    /// Ctrl+U: starts to the grid. Ctrl+Shift+U: lengths too, minimum one
    /// step — Ableton's quantize, aimed at the roll's own grid rung.
    fn quantize_selected(&mut self, notes: &mut [Note], lengths_too: bool) {
        let grid = self.grid_beats();
        // Starts go through the general verb at full strength, so there
        // is ONE quantize in this module and `Ctrl+U` is a preset of it.
        self.quantize_at(notes, 1.0, 0.0);
        if lengths_too {
            let sel = self.acting_on(notes);
            for i in sel {
                if let Some(n) = notes.get_mut(i) {
                    n.len = snap(n.len, grid).max(grid);
                }
            }
        }
    }

    /// Ctrl+E: split every selected note the cursor beat passes through.
    /// Both halves stay selected, so a follow-up delete or nudge treats
    /// the cut as one gesture's result.
    fn split_selected_at_cursor(&mut self, notes: &mut Vec<Note>) {
        let at = self.cursor_beat;
        let cut: Vec<usize> = self
            .selected
            .iter()
            .copied()
            .filter(|&i| {
                notes
                    .get(i)
                    .is_some_and(|n| n.start + 1e-9 < at && at + 1e-9 < n.start + n.len)
            })
            .collect();
        for i in cut {
            let Some(head) = notes.get_mut(i) else {
                continue;
            };
            let tail = Note {
                start: at,
                len: head.start + head.len - at,
                ..head.clone()
            };
            head.len = at - head.start;
            notes.push(tail);
            self.selected.insert(notes.len() - 1);
        }
    }

    /// Ctrl+J: join. Per pitch, the selected notes collapse into ONE note
    /// spanning from the earliest start to the latest end — gaps
    /// included, which is what Ableton's join does and what makes it the
    /// inverse of a split.
    fn join_selected(&mut self, notes: &mut Vec<Note>) {
        let mut by_pitch: std::collections::HashMap<u8, Vec<usize>> =
            std::collections::HashMap::new();
        for &i in &self.selected {
            if let Some(n) = notes.get(i) {
                by_pitch.entry(n.pitch).or_default().push(i);
            }
        }
        let mut dead: Vec<usize> = Vec::new();
        for (_, group) in by_pitch {
            if group.len() < 2 {
                continue;
            }
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for &i in &group {
                if let Some(n) = notes.get(i) {
                    lo = lo.min(n.start);
                    hi = hi.max(n.start + n.len);
                }
            }
            // The earliest note becomes the joined one; the rest go.
            let keep = group
                .iter()
                .copied()
                .min_by(|&a, &b| {
                    let sa = notes.get(a).map_or(f64::INFINITY, |n| n.start);
                    let sb = notes.get(b).map_or(f64::INFINITY, |n| n.start);
                    sa.total_cmp(&sb)
                })
                .unwrap_or(group[0]);
            if let Some(n) = notes.get_mut(keep) {
                n.start = lo;
                n.len = hi - lo;
            }
            dead.extend(group.into_iter().filter(|&i| i != keep));
        }
        dead.sort_unstable_by(|a, b| b.cmp(a));
        for i in dead {
            self.remove_note(notes, i);
        }
    }

    /// End: the cursor to the end of the material, on the grid. Home is
    /// its inverse and needs no helper.
    fn cursor_to_end(&mut self, notes: &[Note]) {
        let end = notes.iter().map(|n| n.start + n.len).fold(0.0f64, f64::max);
        self.cursor_beat = snap(end, self.grid_beats());
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
pub fn keys(
    ctx: &egui::Context,
    pr: &mut PianoRoll,
    notes: Option<&mut Vec<Note>>,
    plocks: &[PlockParam],
    key: Key,
    beats_per_bar: u32,
) {
    // Chord entry is a real mode, not a text widget layered over the normal
    // keymap. It gets first refusal so `2`, `r`, and friends type music
    // rather than changing tools or transforming the clip underneath it.
    if pr.chord_entry.is_some() {
        let Some(notes) = notes else {
            pr.chord_entry = None;
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        ctx.input_mut(|input| {
            use egui::{Event, Key, Modifiers};

            if input.consume_key(Modifiers::NONE, Key::Escape) {
                cancel = true;
            }
            if input.consume_key(Modifiers::NONE, Key::Enter) {
                accept = true;
            }
            let backspace = input.consume_key(Modifiers::NONE, Key::Backspace);

            if let Some(entry) = &mut pr.chord_entry {
                if backspace {
                    entry.text.pop();
                }
                for event in &input.events {
                    // A paste arrives as ONE event rather than a stream of
                    // key presses, so it needs its own arm — without it the
                    // only way to get a phrase in is to retype it.
                    match event {
                        Event::Text(text) | Event::Paste(text) => append_entry(entry, text),
                        _ => {}
                    }
                }
            }

            // Nothing typed into this modal line may leak through to the
            // arrangement keymap or another widget later in the frame.
            input.events.retain(|event| {
                !matches!(event, Event::Text(_) | Event::Paste(_) | Event::Key { .. })
            });
        });

        if cancel {
            pr.chord_entry = None;
            return;
        }

        let source = pr
            .chord_entry
            .as_ref()
            .map(|entry| entry.text.clone())
            .unwrap_or_default();
        let parsed = parse_chord_entry(
            &source,
            pr.cursor_pitch,
            pr.cursor_beat,
            pr.grid_beats(),
            key,
            beats_per_bar,
        );
        if accept {
            match parsed {
                Ok(plan) => {
                    pr.pending_clip_length = plan.clip_length;
                    let first = notes.len();
                    let count = plan.notes.len();
                    notes.extend(plan.notes);
                    pr.selected = (first..first + count).collect();
                    pr.cursor_beat += plan.advance;
                    pr.anchor = None;
                    pr.follow_cursor = true;
                    pr.chord_entry = None;
                }
                Err(error) => {
                    if let Some(entry) = &mut pr.chord_entry {
                        entry.diagnostic = Some(error);
                    }
                }
            }
        } else if let Some(entry) = &mut pr.chord_entry {
            entry.diagnostic = parsed.err();
        }
        return;
    }

    if ctx.egui_wants_keyboard_input() || !pr.owns_keys {
        return;
    }
    let Some(notes) = notes else {
        return;
    };
    let grid = pr.grid_beats();
    let at_start = pr.cursor_beat <= 0.0;
    let has_selection = !pr.selected.is_empty();
    let cursor_before = (pr.cursor_pitch, pr.cursor_beat);
    ctx.input_mut(|i| {
        use egui::{Key, Modifiers};

        // Ctrl+Shift+Enter opens the TRIG editor (probability and A:B
        // condition); Shift+Enter the parameter-lock editor. Most
        // specific chord first, as everywhere. Each editor, while open,
        // owns the keyboard the same way.
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter) {
            pr.trig_toggle_view(notes);
        } else if i.consume_key(Modifiers::SHIFT, Key::Enter) {
            pr.trig_view = None;
            pr.plock_toggle_view(notes);
        }
        if pr.trig_view.is_some() {
            if i.consume_key(Modifiers::NONE, Key::ArrowUp)
                && let Some((_, row)) = &mut pr.trig_view
            {
                *row = row.saturating_sub(1);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown)
                && let Some((_, row)) = &mut pr.trig_view
            {
                *row = (*row + 1).min(1);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
                pr.trig_adjust(notes, 0.001);
            } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                pr.trig_adjust(notes, 0.01);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
                pr.trig_adjust(notes, -0.001);
            } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                pr.trig_adjust(notes, -0.01);
            }
            if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                pr.trig_clear(notes);
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                pr.trig_view = None;
            }
            return;
        }
        if pr.plock_view.is_some() {
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                pr.plock_nav(-1, plocks.len());
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                pr.plock_nav(1, plocks.len());
            }
            // Shift first, as everywhere: the fine step.
            //
            // A coarse press is a fortieth of the range, not a hundredth.
            // A hundredth means a hundred presses end to end, which reads
            // as "the key barely works"; a fortieth crosses a range in a
            // couple of seconds of held repeat and still lands anywhere
            // you want with Shift.
            if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
                pr.plock_adjust(notes, plocks, PLOCK_FINE);
            } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                pr.plock_adjust(notes, plocks, PLOCK_COARSE);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
                pr.plock_adjust(notes, plocks, -PLOCK_FINE);
            } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                pr.plock_adjust(notes, plocks, -PLOCK_COARSE);
            }
            // Home and End go straight to the rails — the two values that
            // are most often what you wanted and are furthest away by
            // nudging.
            if i.consume_key(Modifiers::NONE, Key::Home) {
                pr.plock_adjust(notes, plocks, -f32::INFINITY);
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                pr.plock_adjust(notes, plocks, f32::INFINITY);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                pr.plock_toggle_row(notes, plocks);
            }
            if i.consume_key(Modifiers::NONE, Key::Delete)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
            {
                pr.plock_remove(notes, plocks);
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                pr.plock_view = None;
            }
            return;
        }

        // Ctrl+Shift first, then Ctrl, then Shift, then plain —
        // `consume_key` matches modifiers logically, so the most specific
        // binding must go first or a plain-arrow check swallows
        // Ctrl+arrow. Same warning as in `arrangement_keys`.
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::ArrowUp) {
            // Ableton's octave transpose is Shift+arrow; Shift+arrow here
            // is the box selection, so the octave rides Ctrl+Shift.
            pr.move_selected(notes, 12, 0.0);
        } else if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::ArrowDown) {
            pr.move_selected(notes, -12, 0.0);
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowUp) {
            pr.move_selected(notes, 1, 0.0);
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowDown) {
            pr.move_selected(notes, -1, 0.0);
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowLeft) {
            // With notes selected this nudges them; with nothing selected
            // the same chord steps the roll's grid ALONE. Ctrl+1/2 moves
            // both grids together; this is the way to make the roll finer
            // than the arrangement without dragging the arrangement with
            // it.
            if has_selection {
                pr.move_selected(notes, 0, -grid);
            } else {
                pr.grid = pr.grid.saturating_sub(1);
            }
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowRight) {
            if has_selection {
                pr.move_selected(notes, 0, grid);
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
        if i.consume_key(Modifiers::NONE, Key::Home) {
            pr.cursor_beat = 0.0;
            pr.anchor = None;
        }
        if i.consume_key(Modifiers::NONE, Key::End) {
            pr.cursor_to_end(notes);
            pr.anchor = None;
        }
        // Enter is SMART — add on empty, toggle-select on a note; A
        // always adds, for deliberately stacking a cell.
        if i.consume_key(Modifiers::NONE, Key::Enter) {
            pr.enter_at_cursor(notes);
        }
        if i.consume_key(Modifiers::NONE, Key::A) {
            let (pitch, beat) = (pr.cursor_pitch, pr.cursor_beat);
            pr.add_note_at(notes, pitch, beat);
        }
        // Note-by-note navigation: N forward, Shift+N back. Each stop
        // lands the cursor ON the note and selects it alone.
        if i.consume_key(Modifiers::SHIFT, Key::N) {
            pr.jump_to_note(notes, false);
        } else if i.consume_key(Modifiers::NONE, Key::N) {
            pr.jump_to_note(notes, true);
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
            // Escape always comes home. A latched eraser you cannot get
            // out of is the worst kind of mode.
            pr.tool = Tool::Pointer;
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
        // Shift first: Shift+bracket scales the SPAN (below), so the
        // plain ones must not swallow it — `consume_key` matches
        // modifiers logically and a bare check would take both.
        if !i.modifiers.shift {
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) {
                pr.resize_selected(notes, -grid);
            }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) {
                pr.resize_selected(notes, grid);
            }
        }
        // Shift first: fine velocity is +-1 where the plain step is +-10.
        if i.consume_key(Modifiers::SHIFT, Key::Comma) {
            pr.velocity_selected(notes, -1);
        } else if i.consume_key(Modifiers::NONE, Key::Comma) {
            pr.velocity_selected(notes, -VELOCITY_STEP);
        }
        if i.consume_key(Modifiers::SHIFT, Key::Period) {
            pr.velocity_selected(notes, 1);
        } else if i.consume_key(Modifiers::NONE, Key::Period) {
            pr.velocity_selected(notes, VELOCITY_STEP);
        }
        // Ableton's own verbs, on Ableton's own keys.
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::U) {
            pr.quantize_selected(notes, true);
        } else if i.consume_key(Modifiers::COMMAND, Key::U) {
            pr.quantize_selected(notes, false);
        }
        if i.consume_key(Modifiers::COMMAND, Key::E) {
            pr.split_selected_at_cursor(notes);
        }
        if i.consume_key(Modifiers::COMMAND, Key::J) {
            pr.join_selected(notes);
        }
        if i.consume_key(Modifiers::NONE, Key::Num0) {
            pr.toggle_mute_selected(notes);
        }
        // --- the writing surface -------------------------------------
        // F folds to the material, K locks gestures to the key, L steps
        // the lane stack. All three are view state and all three say so
        // in the info line, because a mode you cannot see is a trap.
        if i.consume_key(Modifiers::NONE, Key::F) {
            pr.fold = !pr.fold;
        }
        // Shift first, as everywhere: `consume_key` matches modifiers
        // LOGICALLY, so a plain-K check would swallow Shift+K too.
        if i.consume_key(Modifiers::SHIFT, Key::K) {
            // Force what is there onto the key, rather than constraining
            // what comes next. The pair reads as one idea on one letter.
            let key = pr.last_key;
            pr.on_selection(notes, |ns, sel| force_to_scale(ns, sel, key));
        } else if i.consume_key(Modifiers::NONE, Key::K) {
            pr.scale_lock = !pr.scale_lock;
        }
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::L) {
            pr.toggle_fixed_length(notes);
        } else if i.consume_key(Modifiers::COMMAND, Key::L) {
            pr.on_selection(notes, legato);
        } else if i.consume_key(Modifiers::NONE, Key::L) {
            pr.cycle_lanes();
        }

        // --- the transform verbs -------------------------------------
        // A tenth of the grid and a dozen velocity steps: enough to
        // unstick a part from the grid, not enough to make it sloppy.
        if i.consume_key(Modifiers::NONE, Key::H) {
            pr.humanize_at(notes, grid * 0.1, 12);
        }
        if i.consume_key(Modifiers::NONE, Key::R) {
            pr.on_selection(notes, retrograde);
        }
        if i.consume_key(Modifiers::SHIFT, Key::I) {
            let pivot = pivot_of(notes, &pr.acting_on(notes));
            pr.on_selection(notes, |ns, sel| invert(ns, sel, pivot));
        } else if i.consume_key(Modifiers::NONE, Key::I) {
            pr.chord_entry = Some(ChordEntry::default());
        }
        // Strum: one grid step apart, Shift for the other direction.
        if i.consume_key(Modifiers::SHIFT, Key::S) {
            pr.on_selection(notes, |ns, sel| strum(ns, sel, -grid));
        } else if i.consume_key(Modifiers::NONE, Key::S) {
            pr.on_selection(notes, |ns, sel| strum(ns, sel, grid));
        }
        // Shift+brackets scale the selection's SPAN, where the plain
        // brackets scale each note's length. Half time and double time.
        if i.consume_key(Modifiers::SHIFT, Key::OpenBracket) {
            pr.on_selection(notes, |ns, sel| scale_time(ns, sel, 0.5));
        }
        if i.consume_key(Modifiers::SHIFT, Key::CloseBracket) {
            pr.on_selection(notes, |ns, sel| scale_time(ns, sel, 2.0));
        }
    });
    if (pr.cursor_pitch, pr.cursor_beat) != cursor_before {
        pr.follow_cursor = true;
    }
}

/// The parameter-lock editor: a floating list beside the note it holds.
/// Every row is one lockable parameter — its name, and either the locked
/// value or an em-dash for "following the knob". The keyboard walks it
/// with arrows (see `keys`); the mouse clicks a row to hold it and drags
/// to set it, right where the values are read.
#[allow(clippy::too_many_arguments)]
fn plock_overlay(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    notes: &mut [Note],
    plocks: &[PlockParam],
    g: Geom,
) {
    let grid = g.grid;
    let Some((note_idx, row_sel)) = pr.plock_at(notes) else {
        return;
    };
    if plocks.is_empty() {
        return;
    }
    let Some(anchor_note) = notes.get(note_idx) else {
        return;
    };

    const ROW_H_PX: f32 = 15.0;
    const PANEL_W: f32 = 190.0;
    let panel_h = (ROW_H_PX * plocks.len() as f32 + 8.0).min(grid.height() - 8.0);
    let nr = g.note_rect(anchor_note);
    let mut origin = egui::pos2(nr.right() + 8.0, nr.top());
    if origin.x + PANEL_W > grid.right() {
        origin.x = (nr.left() - PANEL_W - 8.0).max(grid.left());
    }
    origin.y = origin
        .y
        .clamp(grid.top(), (grid.bottom() - panel_h).max(grid.top()));
    let panel = egui::Rect::from_min_size(origin, egui::vec2(PANEL_W, panel_h));

    let painter = ui.painter();
    painter.rect_filled(panel, 3.0, theme.surface);
    painter.rect_stroke(
        panel,
        3.0,
        egui::Stroke::new(stroke::HAIR, theme.accent_muted),
        egui::StrokeKind::Inside,
    );

    let inner = panel.shrink(4.0);
    // One row of the panel is spent on the key legend, which is worth it
    // exactly once: nobody guesses "left and right change the value" from
    // a vertical list.
    let rows_h = (inner.height() - ROW_H_PX).max(ROW_H_PX);
    let visible = ((rows_h / ROW_H_PX) as usize).max(1);

    // Scroll only when the selection would LEAVE the window, and keep it
    // where it is otherwise.
    //
    // The first version pinned the selected row to the bottom edge, so
    // every step up scrolled the whole list by one and the row you were
    // reading slid away under you. A list that moves when the selection
    // does not is a list you cannot keep your place in.
    let first = pr
        .plock_scroll
        .min(row_sel)
        .max((row_sel + 1).saturating_sub(visible));
    let first = first.min(plocks.len().saturating_sub(visible));
    pr.plock_scroll = first;
    for (vis, row) in (first..plocks.len()).take(visible).enumerate() {
        let p = &plocks[row];
        let r = egui::Rect::from_min_size(
            egui::pos2(inner.left(), inner.top() + vis as f32 * ROW_H_PX),
            egui::vec2(inner.width(), ROW_H_PX),
        );
        let resp = ui.interact(
            r,
            ui.id().with(("plock-row", row)),
            egui::Sense::click_and_drag(),
        );
        if resp.clicked() {
            pr.plock_view = Some((note_idx, row));
        }
        if resp.double_clicked() {
            pr.plock_view = Some((note_idx, row));
            pr.plock_remove(notes, plocks);
        } else if resp.dragged() {
            pr.plock_view = Some((note_idx, row));
            let d = resp.drag_delta();
            if p.choices > 0 {
                // Discrete rows step in DETENTS: accumulate the pull in
                // widget memory and fire one whole choice per notch, the
                // same 14 px the synth's cells use — because the stored
                // value snaps and fractional progress would be eaten.
                let id = resp.id.with("plock-detent");
                let mut acc: f32 = if resp.drag_started() {
                    0.0
                } else {
                    ui.data(|d| d.get_temp(id)).unwrap_or(0.0)
                };
                acc += d.x - d.y;
                let detents = (acc / 14.0).trunc();
                acc -= detents * 14.0;
                ui.data_mut(|data| data.insert_temp(id, acc));
                for _ in 0..detents.abs() as i32 {
                    pr.plock_adjust(notes, plocks, if detents > 0.0 { 1.0 } else { -1.0 });
                }
            } else {
                pr.plock_adjust(notes, plocks, (d.x - d.y) / 300.0);
            }
        }
        let selected = row == row_sel;
        if selected {
            ui.painter()
                .rect_filled(r, 2.0, theme.surface_sunken.gamma_multiply(1.4));
        }
        let lock = notes
            .get(note_idx)
            .and_then(|n| n.plocks.iter().find(|(id, _)| *id == p.id))
            .map(|(_, v)| *v);
        let painter = ui.painter();

        // The POSITION BAR, under the text: where this value sits in its
        // own range, with the knob's value marked.
        //
        // It is here because "nothing happened" needed an explanation. A
        // row already at its minimum cannot go down, and without a bar
        // that is indistinguishable from a broken key — which is exactly
        // how it was reported. A full bar and an empty one look different
        // from across the room.
        let bar = egui::Rect::from_min_size(
            egui::pos2(r.left() + 2.0, r.bottom() - 3.0),
            egui::vec2(r.width() - 4.0, 1.5),
        );
        painter.rect_filled(bar, 0.0, theme.outline.gamma_multiply(0.5));
        let shown = lock.unwrap_or(p.base);
        let filled = egui::Rect::from_min_size(
            bar.min,
            egui::vec2(bar.width() * p.position(shown), bar.height()),
        );
        painter.rect_filled(
            filled,
            0.0,
            if lock.is_some() {
                theme.accent
            } else {
                theme.text_muted.gamma_multiply(0.5)
            },
        );
        // The KNOB's own value, so a lock can be read against what the
        // note would otherwise have played.
        if lock.is_some() {
            let x = bar.left() + bar.width() * p.position(p.base);
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, bar.top() - 1.5), egui::vec2(1.0, 4.5)),
                0.0,
                theme.text_muted,
            );
        }

        painter.text(
            egui::pos2(r.left() + 2.0, r.center().y - 1.0),
            egui::Align2::LEFT_CENTER,
            &p.name,
            egui::FontId::proportional(font::LABEL - 1.0),
            if lock.is_some() {
                theme.text
            } else {
                theme.text_muted
            },
        );
        // The parameter's OWN face: "saw", "1.05 s", "+3 st" — the same
        // words its knob prints, never a bare float.
        //
        // An unlocked row shows the KNOB's value in brackets rather than
        // a dash. A dash says "nothing here"; what is actually true is
        // "this note plays whatever the knob says", and that is the
        // number you are about to lock away from.
        let (value, colour) = match lock {
            Some(v) => (p.face(v), theme.accent),
            None => (format!("({})", p.face(p.base)), theme.outline),
        };
        painter.text(
            egui::pos2(r.right() - 2.0, r.center().y - 1.0),
            egui::Align2::RIGHT_CENTER,
            value,
            egui::FontId::monospace(font::LABEL - 1.0),
            colour,
        );
        resp.on_hover_text(if p.choices > 0 {
            "click to hold · drag to step · double-click clears"
        } else {
            "click to hold · drag to set (locks from the knob) · double-click clears"
        });
    }

    // The legend, once, along the bottom. Nobody guesses that a VERTICAL
    // list is adjusted with the HORIZONTAL arrows, and the alternative to
    // saying so is every user finding out the way this one did.
    let legend = egui::Rect::from_min_size(
        egui::pos2(inner.left(), inner.bottom() - ROW_H_PX),
        egui::vec2(inner.width(), ROW_H_PX),
    );
    ui.painter().text(
        legend.center(),
        egui::Align2::CENTER_CENTER,
        "↑↓ row  ←→ value  ⇧ fine  ⏎ lock  ⌫ clear",
        egui::FontId::proportional(font::MINI_LABEL),
        theme.text_muted,
    );
}

/// The trig editor: two rows beside the note — probability, and the A:B
/// condition ladder. The same manners as the lock panel: keyboard walks
/// it, the mouse clicks a row to hold it and drags to set it.
#[allow(clippy::too_many_arguments)]
fn trig_overlay(ui: &mut egui::Ui, theme: &Theme, pr: &mut PianoRoll, notes: &mut [Note], g: Geom) {
    let grid = g.grid;
    let Some((note_idx, row_sel)) = pr.trig_at(notes) else {
        return;
    };
    let Some(anchor_note) = notes.get(note_idx) else {
        return;
    };

    const ROW_H_PX: f32 = 16.0;
    const PANEL_W: f32 = 150.0;
    let panel_h = ROW_H_PX * 2.0 + 8.0;
    let nr = g.note_rect(anchor_note);
    let mut origin = egui::pos2(nr.right() + 8.0, nr.top());
    if origin.x + PANEL_W > grid.right() {
        origin.x = (nr.left() - PANEL_W - 8.0).max(grid.left());
    }
    origin.y = origin
        .y
        .clamp(grid.top(), (grid.bottom() - panel_h).max(grid.top()));
    let panel = egui::Rect::from_min_size(origin, egui::vec2(PANEL_W, panel_h));

    let painter = ui.painter();
    painter.rect_filled(panel, 3.0, theme.surface);
    painter.rect_stroke(
        panel,
        3.0,
        egui::Stroke::new(stroke::HAIR, theme.accent_muted),
        egui::StrokeKind::Inside,
    );

    let inner = panel.shrink(4.0);
    let (prob, cond) = (anchor_note.prob, anchor_note.cond);
    for row in 0..2usize {
        let r = egui::Rect::from_min_size(
            egui::pos2(inner.left(), inner.top() + row as f32 * ROW_H_PX),
            egui::vec2(inner.width(), ROW_H_PX),
        );
        let resp = ui.interact(
            r,
            ui.id().with(("trig-row", row)),
            egui::Sense::click_and_drag(),
        );
        if resp.clicked() || resp.dragged() {
            pr.trig_view = Some((note_idx, row));
        }
        if resp.double_clicked() {
            pr.trig_clear(notes);
        } else if resp.dragged() {
            let d = resp.drag_delta();
            if row == 0 {
                pr.trig_adjust(notes, (d.x - d.y) / 300.0);
            } else {
                // The ladder steps in detents, like every choice here.
                let id = resp.id.with("trig-detent");
                let mut acc: f32 = if resp.drag_started() {
                    0.0
                } else {
                    ui.data(|d| d.get_temp(id)).unwrap_or(0.0)
                };
                acc += d.x - d.y;
                let detents = (acc / 14.0).trunc();
                acc -= detents * 14.0;
                ui.data_mut(|data| data.insert_temp(id, acc));
                for _ in 0..detents.abs() as i32 {
                    pr.trig_adjust(notes, if detents > 0.0 { 1.0 } else { -1.0 });
                }
            }
        }
        if row == row_sel {
            ui.painter()
                .rect_filled(r, 2.0, theme.surface_sunken.gamma_multiply(1.4));
        }
        let painter = ui.painter();
        let (name, value, live) = if row == 0 {
            ("probability", format!("{:.0} %", prob * 100.0), prob < 1.0)
        } else {
            (
                "condition",
                match cond {
                    Some((a, b)) => format!("{a}:{b}"),
                    None => "—".to_owned(),
                },
                cond.is_some(),
            )
        };
        painter.text(
            egui::pos2(r.left() + 2.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(font::LABEL - 1.0),
            if live { theme.text } else { theme.text_muted },
        );
        painter.text(
            egui::pos2(r.right() - 2.0, r.center().y),
            egui::Align2::RIGHT_CENTER,
            value,
            egui::FontId::monospace(font::LABEL - 1.0),
            if live { theme.accent } else { theme.outline },
        );
        resp.on_hover_text("click to hold · drag to set · double-click resets");
    }
}

// --- zoom -----------------------------------------------------------------

/// Ctrl+wheel and the +/- keys, on both axes.
///
/// The anchor is held still across the change: whatever sits under the
/// pointer before the zoom sits under it after. Without that, zooming is
/// a scroll you did not ask for.
fn zoom_input(ui: &egui::Ui, pr: &mut PianoRoll, grid: egui::Rect, fold: Fold) {
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

    // A trackpad pinch, which egui reports as a multiplicative factor
    // rather than as wheel ticks. Shift makes it the pitch axis, matching
    // the wheel and the keys — one grammar, three devices.
    let pinch = ui.input(|i| i.zoom_delta());
    if (pinch - 1.0).abs() > 1e-4 && ui.rect_contains_pointer(grid) {
        if mods.shift {
            pitch *= pinch;
        } else {
            time *= pinch;
        }
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

    let old = Geom::new(grid, pr.scroll_beats, pr.scroll_y, pr.zoom, fold);
    let new_zoom = old.z.scaled(time, pitch);
    if new_zoom == old.z {
        return;
    }

    // Anchor: whatever sits under the pointer, else under the cursor.
    let anchor = ui.ctx().pointer_latest_pos().filter(|p| grid.contains(*p));
    let (ax, ay) = match anchor {
        Some(p) => (p.x, p.y),
        None => (
            old.x_at(pr.cursor_beat).clamp(grid.left(), grid.right()),
            old.row_top(pr.cursor_pitch)
                .clamp(grid.top(), grid.bottom()),
        ),
    };
    let beat = old.beat_at(ax);
    // Rows measured from the top of the pitch space, in row units.
    let row_units = (ay - grid.top() + pr.scroll_y) / old.row_h();

    pr.zoom = new_zoom;
    pr.scroll_beats = (beat as f32 - (ax - grid.left()) / new_zoom.px_per_beat).max(0.0);
    pr.scroll_y = (row_units * new_zoom.row_h - (ay - grid.top())).max(0.0);
}

// --- layout ---------------------------------------------------------------

/// Where every region of the editor sits.
///
/// Pure, so the split — including how it degrades as the panel shrinks —
/// is checkable without a window.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The vertical tool strip on the far left, full height.
    pub tools: egui::Rect,
    /// The box above the keyboard gutter: grid name and working key.
    pub corner: egui::Rect,
    /// The bar ruler across the top of the grid.
    pub ruler: egui::Rect,
    /// The piano keyboard gutter.
    pub keys: egui::Rect,
    /// The note grid.
    pub grid: egui::Rect,
    /// One entry per OPEN lane: its index in `PianoRoll::lanes`, its
    /// gutter, and its body. Shorter than the lane list when the panel
    /// ran out of room, which is a real state and not an error.
    pub lanes: Vec<(usize, egui::Rect, egui::Rect)>,
    /// The info line. Empty when the panel is too short for one.
    pub info: egui::Rect,
}

/// The panel heights at which each region gives up its space. Ordered, and
/// ordered deliberately: the GRID is never the thing that shrinks first,
/// because the grid is what the panel is for.
const MIN_FOR_INFO: f32 = 140.0;
const MIN_FOR_RULER: f32 = 110.0;
const LANES_SQUEEZE_BELOW: f32 = 260.0;
/// The grid never goes below this, whatever else is asking for room.
const MIN_GRID_H: f32 = 60.0;

pub fn layout(area: egui::Rect, lanes: &[LaneView]) -> Layout {
    // The fixed slices are decided from the WHOLE panel, before anything
    // else is handed out. Deciding them from what is left over instead
    // lets a couple of lanes quietly push the ruler off the top, which is
    // the wrong answer: a lane is a detail and the ruler is navigation.
    let ruler_h = if area.height() >= MIN_FOR_RULER {
        RULER_H
    } else {
        0.0
    };
    let info_h = if area.height() >= MIN_FOR_INFO {
        INFO_H
    } else {
        0.0
    };
    let gutter_w = TOOLS_W + KEYS_W;
    let band_top = area.top() + ruler_h;
    let band_bottom = area.bottom() - info_h;

    // Lanes fill upward from just above the info line, in reverse so lane
    // zero ends up nearest the grid. They collapse to headers on a short
    // panel, and they stop asking for room the moment the grid would drop
    // below `MIN_GRID_H` — the grid is what the panel is for.
    let squeeze = area.height() < LANES_SQUEEZE_BELOW;
    let mut lane_rects = Vec::new();
    let mut bottom = band_bottom;
    for (i, lv) in lanes.iter().enumerate().rev() {
        let h = if squeeze { LANE_HEADER_H } else { lv.height() };
        if bottom - h - band_top < MIN_GRID_H {
            break;
        }
        let top = bottom - h;
        lane_rects.push((
            i,
            egui::Rect::from_min_max(
                egui::pos2(area.left(), top),
                egui::pos2(area.left() + gutter_w, bottom),
            ),
            egui::Rect::from_min_max(
                egui::pos2(area.left() + gutter_w, top),
                egui::pos2(area.right(), bottom),
            ),
        ));
        bottom = top;
    }
    lane_rects.reverse();

    Layout {
        tools: egui::Rect::from_min_max(area.min, egui::pos2(area.left() + TOOLS_W, bottom)),
        corner: egui::Rect::from_min_max(
            egui::pos2(area.left() + TOOLS_W, area.top()),
            egui::pos2(area.left() + gutter_w, band_top),
        ),
        ruler: egui::Rect::from_min_max(
            egui::pos2(area.left() + gutter_w, area.top()),
            egui::pos2(area.right(), band_top),
        ),
        keys: egui::Rect::from_min_max(
            egui::pos2(area.left() + TOOLS_W, band_top),
            egui::pos2(area.left() + gutter_w, bottom),
        ),
        grid: egui::Rect::from_min_max(
            egui::pos2(area.left() + gutter_w, band_top),
            egui::pos2(area.right(), bottom),
        ),
        lanes: lane_rects,
        info: egui::Rect::from_min_max(
            egui::pos2(area.left(), band_bottom),
            egui::pos2(area.right(), area.bottom()),
        ),
    }
}

// --- hit testing ----------------------------------------------------------

/// The note under a point, and which of its zones. Back to front, because
/// later notes draw on top and the one you can see is the one you meant.
fn hit_note(notes: &[Note], g: Geom, at: egui::Pos2) -> Option<(usize, Zone)> {
    notes.iter().enumerate().rev().find_map(|(i, n)| {
        let r = g.note_rect(n);
        r.contains(at).then(|| (i, zone_of(r, at)))
    })
}

/// Cut one note in two at `beat`, if the note actually spans it.
///
/// The tail is pushed on the end, which keeps every other index stable —
/// the selection is a set of indices, and a split that renumbered the
/// notes would silently reselect different ones.
fn split_one(notes: &mut Vec<Note>, i: usize, beat: f64) {
    let Some(n) = notes.get(i).cloned() else {
        return;
    };
    if n.start >= beat || n.start + n.len <= beat {
        return;
    }
    let mut tail = n.clone();
    tail.start = beat;
    tail.len = n.start + n.len - beat;
    if let Some(head) = notes.get_mut(i) {
        head.len = beat - n.start;
    }
    notes.push(tail);
}

/// The nearest scale degree to `pitch`, searching outward from it.
///
/// Outward rather than downward so a constrained gesture lands on the
/// closest legal note rather than always the one below — and it never
/// STOPS a gesture, because a constraint you cannot move through reads as
/// a broken drag rather than as a rule.
fn snap_to_scale(pitch: u8, key: Key) -> u8 {
    if key.scale.contains(key.tonic, pitch) {
        return pitch;
    }
    for d in 1..=6i32 {
        for cand in [i32::from(pitch) - d, i32::from(pitch) + d] {
            if (0..=i32::from(PITCH_MAX)).contains(&cand)
                && key.scale.contains(key.tonic, cand as u8)
            {
                return cand as u8;
            }
        }
    }
    pitch
}

/// Bar.beat.tick, one-based, over a CLIP-RELATIVE beat position. The roll
/// numbers a clip's own bars from 1 — Ableton's convention, and the only
/// one that survives moving the clip.
fn bbt(beat: f64, beats_per_bar: u32) -> String {
    let bpb = f64::from(beats_per_bar.max(1));
    let bar = (beat / bpb).floor();
    let within = beat - bar * bpb;
    let b = within.floor();
    let tick = ((within - b) * 960.0).round() as i64;
    format!("{}.{}.{:03}", bar as i64 + 1, b as i64 + 1, tick)
}

/// "C#4" — the full name, for labels on notes and in the info line. The
/// octave convention is the one the gutter already uses: middle C is C4.
fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i32::from(pitch) / 12 - 1
    )
}

// --- the roll's own view verbs -------------------------------------------

impl PianoRoll {
    /// The fold mask for this frame.
    ///
    /// The invariant lives here: whatever the user is folding TO, every
    /// pitch the clip actually uses comes along. Folding is a view change
    /// and may never hide material — a note you cannot see is a note you
    /// will delete by accident.
    fn fold_mask(&self, notes: Option<&[Note]>, key: Key) -> Fold {
        if !self.fold {
            return Fold::ALL;
        }
        let used = Fold::used(notes.unwrap_or_default());
        if used.is_empty() {
            // An empty clip folded to zero rows is a blank panel, not a
            // feature.
            return Fold::ALL;
        }
        // Folding to the scale as well keeps somewhere to WRITE: a fold
        // that shows only the notes already there has no empty row left
        // to put the next one on.
        used.union(Fold::scale(key))
    }

    /// The tool in force this frame.
    ///
    /// Holding a tool key BORROWS that tool for exactly as long as the
    /// finger is down; tapping it LATCHES. Both from the same keystroke,
    /// told apart by how long it lasted — which is Cubase's rule and the
    /// reason a tool palette never has to be clicked. The tool strip
    /// shows whichever is in force, so the two can never disagree about
    /// what the next click will do.
    fn tool_now(&mut self, ui: &egui::Ui) -> Tool {
        let now = ui.input(|i| i.time);
        // Only while the roll owns the keyboard — a number key typed at
        // the arrangement must not silently arm the eraser down here.
        if !self.owns_keys {
            self.held_tool = None;
            return self.tool;
        }
        let (pressed, released) = ui.ctx().input_mut(|i| {
            let mut pressed = None;
            let mut released = None;
            for t in Tool::ALL {
                if i.consume_key(egui::Modifiers::NONE, t.key()) {
                    pressed = Some(t);
                }
                if i.key_released(t.key()) && self.held_tool == Some(t) {
                    released = Some(t);
                }
            }
            (pressed, released)
        });
        if let Some(t) = pressed {
            self.held_tool = Some(t);
            self.held_since = now;
        }
        if let Some(t) = released {
            if now - self.held_since < TOOL_TAP {
                self.tool = t;
            }
            self.held_tool = None;
        }
        self.held_tool.unwrap_or(self.tool)
    }

    /// Step the lane stack: none, velocity, +probability, +length, none.
    ///
    /// One key rather than three, because three keys for three lanes is
    /// three things to remember and this is one thing to press until the
    /// lanes you want are showing.
    pub fn cycle_lanes(&mut self) {
        // The order is `Lane::ALL`, so the cycle and the lane list are
        // one list and cannot drift apart when a lane is added.
        let shown = (self.lanes.len() + 1) % (Lane::ALL.len() + 1);
        self.lanes = Lane::ALL[..shown]
            .iter()
            .map(|&lane| {
                // Keep a lane's height if it was already open, so cycling
                // past a lane and back does not forget how tall you made it.
                self.lanes
                    .iter()
                    .find(|l| l.lane == lane)
                    .copied()
                    .unwrap_or(LaneView {
                        lane,
                        h: LANE_H,
                        collapsed: false,
                    })
            })
            .collect();
    }

    /// Quantize what is being acted on, at a strength and a swing.
    ///
    /// The keyboard's `Ctrl+U` passes `(1.0, 0.0)` — the hard snap it has
    /// always done. Everything between is what makes quantize musical,
    /// and it is exactly what the old one-strength verb could not say.
    pub fn quantize_at(&mut self, notes: &mut [Note], strength: f32, swing: f32) {
        let sel = self.acting_on(notes);
        let grid = self.grid_beats();
        quantize(notes, &sel, grid, strength, swing);
    }

    /// Humanize position and velocity, and advance the seed so the next
    /// press is a different pass.
    pub fn humanize_at(&mut self, notes: &mut [Note], time: f64, vel: i32) {
        let sel = self.acting_on(notes);
        humanize(notes, &sel, time, vel, self.seed);
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
    }

    /// Run a transform that needs nothing but the selection.
    fn on_selection(&mut self, notes: &mut [Note], f: impl FnOnce(&mut [Note], &[usize])) {
        let sel = self.acting_on(notes);
        f(notes, &sel);
    }

    /// Lock the drawn-note length to the selection's, or release it.
    pub fn toggle_fixed_length(&mut self, notes: &[Note]) {
        if self.draw_len.is_some() {
            self.draw_len = None;
            return;
        }
        let picked = self.acting_on(notes);
        let len = picked
            .iter()
            .filter_map(|&i| notes.get(i))
            .map(|n| n.len)
            .fold(f64::NEG_INFINITY, f64::max);
        if len.is_finite() && len > 0.0 {
            self.draw_len = Some(len);
        }
    }

    /// Open a lane, or close it if it is already open. The palette's verb
    /// and the gutter's click both land here.
    pub fn toggle_lane(&mut self, lane: Lane) {
        if let Some(at) = self.lanes.iter().position(|l| l.lane == lane) {
            self.lanes.remove(at);
        } else {
            self.lanes.push(LaneView {
                lane,
                h: LANE_H,
                collapsed: false,
            });
        }
    }

    /// The cursor's full state, in words.
    ///
    /// One string, one source of truth: the info line prints it and the
    /// widget's accessibility label announces it. Two hand-written
    /// descriptions of the same state is how a screen reader ends up
    /// saying something the screen does not.
    pub fn cursor_readout(&self, notes: &[Note], beats_per_bar: u32) -> String {
        let mut out = format!(
            "{}, bar {}",
            note_name(self.cursor_pitch),
            bbt(self.cursor_beat, beats_per_bar)
        );
        if let Some(n) = self.note_at_cursor(notes).and_then(|i| notes.get(i)) {
            out.push_str(&format!(", velocity {}, length {:.3}", n.vel, n.len));
            if self
                .selected
                .contains(&self.note_at_cursor(notes).unwrap_or(usize::MAX))
            {
                out.push_str(", selected");
            }
            if n.muted {
                out.push_str(", deactivated");
            }
        } else {
            out.push_str(", empty");
        }
        if !self.selected.is_empty() {
            out.push_str(&format!(", {} notes held", self.selected.len()));
        }
        out
    }

    /// Take the ruler's pending locate, if there is one. Call it once a
    /// frame from the app; it clears as it is read, so a locate happens
    /// exactly once.
    ///
    /// Nothing inside this module calls it — the roll does not own a
    /// transport, so it asks. The app turns the answer into a seek, which
    /// moves the playhead even while it is rolling.
    pub fn take_locate(&mut self) -> Option<f64> {
        self.locate.take()
    }

    /// Place a note the way the DRAW gesture places one: at the fixed
    /// length if the user locked one, else one grid step. Returns the
    /// length it used, which the draw drag needs to size itself from.
    ///
    /// One function because three call sites place notes with the pointer
    /// — double-click, draw-click, draw-drag — and three copies of "which
    /// length is it again" is how they end up disagreeing.
    fn draw_note(&mut self, notes: &mut Vec<Note>, pitch: u8, start: f64, grid: f64) -> f64 {
        let len = self.draw_len.unwrap_or(grid);
        self.add_note_at(notes, pitch, start);
        if let Some(n) = notes.last_mut() {
            n.len = len;
        }
        self.cursor_pitch = pitch;
        self.cursor_beat = start;
        len
    }

    /// The selection, or the note under the cursor when nothing is
    /// selected. Every transform verb wants this and none of them should
    /// each decide it differently.
    pub fn acting_on(&self, notes: &[Note]) -> Vec<usize> {
        if !self.selected.is_empty() {
            let mut v: Vec<usize> = self.selected.iter().copied().collect();
            v.sort_unstable();
            return v;
        }
        self.note_at_cursor(notes).into_iter().collect()
    }
}

// --- gestures -------------------------------------------------------------

/// The modifier grammar, resolved once per frame so no two gestures can
/// disagree about what Alt means.
#[derive(Debug, Clone, Copy)]
struct Mods {
    shift: bool,
    command: bool,
    /// Alt alone: copy instead of move.
    copy: bool,
    /// Ctrl+Alt: bypass snap for this gesture. Alt alone is copy — which
    /// Live, Logic and Cubase all agree on — so the snap escape moves one
    /// key over rather than fighting it.
    free: bool,
}

impl Mods {
    fn read(ui: &egui::Ui) -> Self {
        let m = ui.input(|i| i.modifiers);
        Self {
            shift: m.shift,
            command: m.command,
            copy: m.alt && !m.command,
            free: m.alt && m.command,
        }
    }
}

/// Snap unless the gesture asked not to.
fn maybe_snap(beat: f64, grid: f64, free: bool) -> f64 {
    if free {
        beat.max(0.0)
    } else {
        snap(beat, grid)
    }
}

/// Apply a drag in flight. Returns the marquee rect to paint, if any.
///
/// Everything here recomputes from the press, never from the previous
/// frame: a drag that pins at the end of a range and comes back must land
/// where the pointer says, not where the accumulated deltas left it.
#[allow(clippy::too_many_arguments)]
fn apply_drag(
    d: &mut Drag,
    pr: &mut PianoRoll,
    notes: &mut Vec<Note>,
    g: Geom,
    pos: egui::Pos2,
    grid_beats: f64,
    mods: Mods,
    key: Key,
) -> Option<egui::Rect> {
    match d {
        Drag::Move {
            anchor,
            from,
            press,
            axis,
        } => {
            // Shift constrains, and commits to one axis ONCE — at
            // AXIS_LOCK_PX of travel — then holds it for the gesture. A
            // constraint that re-decides every frame flips under the hand.
            let delta = pos - *press;
            if mods.shift {
                if axis.is_none() && delta.length() >= AXIS_LOCK_PX {
                    *axis = Some(if delta.x.abs() >= delta.y.abs() {
                        Axis::Time
                    } else {
                        Axis::Pitch
                    });
                }
            } else {
                *axis = None;
            }
            let (dx, dy) = match axis {
                Some(Axis::Time) => (delta.x, 0.0),
                Some(Axis::Pitch) => (0.0, delta.y),
                None => (delta.x, delta.y),
            };

            // The anchor decides the move; everyone else follows it, so
            // relative pitch and time inside the block are preserved
            // exactly rather than each note rounding on its own.
            let &(_, a_pitch0, a_start0) = from.iter().find(|(i, _, _)| i == anchor)?;
            let rows = (dy / g.row_h()).round() as i32;
            let want_pitch = transposed(a_pitch0, -rows);
            let want_pitch = if pr.scale_lock && !mods.free {
                snap_to_scale(want_pitch, key)
            } else {
                want_pitch
            };
            let mut d_pitch = i32::from(want_pitch) - i32::from(a_pitch0);
            let want_start = a_start0 + f64::from(dx / g.px_per_beat());
            let mut d_beat = maybe_snap(want_start, grid_beats, mods.free) - a_start0;

            // Clamp as a BLOCK. Clamping note by note would shear the
            // chord: the low notes would stop while the high ones kept
            // going, and the shape you were dragging would not survive.
            let (mut lo_p, mut hi_p) = (PITCH_MAX, 0u8);
            let mut lo_s = f64::INFINITY;
            for &(_, p, s) in from.iter() {
                lo_p = lo_p.min(p);
                hi_p = hi_p.max(p);
                lo_s = lo_s.min(s);
            }
            d_pitch = d_pitch.clamp(-i32::from(lo_p), i32::from(PITCH_MAX) - i32::from(hi_p));
            if lo_s.is_finite() {
                d_beat = d_beat.max(-lo_s);
            }

            for &(i, p0, s0) in from.iter() {
                if let Some(n) = notes.get_mut(i) {
                    n.pitch = transposed(p0, d_pitch);
                    n.start = (s0 + d_beat).max(0.0);
                }
            }
            // The cursor rides the block, so the next keystroke still
            // holds the note the hand was holding.
            if let Some(n) = notes.get(*anchor) {
                pr.cursor_pitch = n.pitch;
                pr.cursor_beat = n.start;
            }
            None
        }

        Drag::ResizeR { from, press } => {
            let dx = f64::from((pos.x - press.x) / g.px_per_beat());
            for &(i, len0) in from.iter() {
                if let Some(n) = notes.get_mut(i) {
                    let want = n.start + len0 + dx;
                    n.len = (maybe_snap(want, grid_beats, mods.free) - n.start).max(grid_beats);
                }
            }
            None
        }

        Drag::ResizeL { from, press } => {
            let dx = f64::from((pos.x - press.x) / g.px_per_beat());
            for &(i, start0, len0) in from.iter() {
                if let Some(n) = notes.get_mut(i) {
                    // The RIGHT edge is what stays put: pulling a note's
                    // head must not also move its tail.
                    let right = start0 + len0;
                    let want = maybe_snap(start0 + dx, grid_beats, mods.free)
                        .clamp(0.0, (right - grid_beats).max(0.0));
                    n.start = want;
                    n.len = (right - want).max(grid_beats);
                }
            }
            None
        }

        Drag::Marquee { press, base } => {
            let band = egui::Rect::from_two_pos(*press, pos);
            let (b0, b1) = (g.beat_at(press.x), g.beat_at(pos.x));
            let clamp_y = |y: f32| y.clamp(g.grid.top(), g.grid.bottom() - 1.0);
            if let (Some(p0), Some(p1)) = (g.pitch_at(clamp_y(press.y)), g.pitch_at(clamp_y(pos.y)))
            {
                // In a FOLDED grid the box's pitch bounds are two rows,
                // not two pitch numbers — the rows between them are the
                // ones the user swept, whatever pitches they happen to be.
                let (r0, r1) = (g.fold.row_of(p0), g.fold.row_of(p1));
                let (r_lo, r_hi) = if r0 <= r1 { (r0, r1) } else { (r1, r0) };
                let swept: HashSet<u8> = g.fold.rows_in(r_lo..r_hi + 1).map(|(_, p)| p).collect();
                let (lo, hi) = if b0 <= b1 { (b0, b1) } else { (b1, b0) };
                pr.selected = base
                    .iter()
                    .copied()
                    .chain(notes.iter().enumerate().filter_map(|(i, n)| {
                        (swept.contains(&n.pitch) && n.start < hi && n.start + n.len > lo)
                            .then_some(i)
                    }))
                    .collect();
            }
            Some(band)
        }

        Drag::Lane {
            lane,
            targets,
            press,
            ramp,
        } => {
            // The lane's own rect is what `g` was rebased onto by the
            // caller, so bottom is zero and top is full scale.
            let usable = (g.grid.height() - VEL_LANE_PAD).max(1.0);
            let frac_at = |y: f32| ((g.grid.bottom() - y) / usable).clamp(0.0, 1.0);
            if *ramp && targets.len() > 1 {
                lane_ramp(
                    notes,
                    targets,
                    *lane,
                    g.beat_at(press.x),
                    g.beat_at(pos.x),
                    frac_at(press.y),
                    frac_at(pos.y),
                    grid_beats,
                );
            } else {
                let v = frac_at(pos.y);
                for &i in targets.iter() {
                    if let Some(n) = notes.get_mut(i) {
                        lane.set(n, v, grid_beats);
                    }
                }
            }
            None
        }

        Drag::LaneResize { idx, h0, press } => {
            if let Some(lv) = pr.lanes.get_mut(*idx) {
                // Dragging the top edge UP makes the lane taller.
                lv.h = (*h0 + (press.y - pos.y)).clamp(LANE_H_MIN, LANE_H_MAX);
                lv.collapsed = false;
            }
            None
        }

        Drag::Sweep { verb, done } => {
            // Erase and mute paint across notes.
            let (i, _) = hit_note(notes, g, pos)?;
            match verb {
                SweepVerb::Erase => pr.remove_note(notes, i),
                // `done` stops one pass toggling the same note twice as
                // the hand wobbles over it. Erase needs no such guard: a
                // deleted note cannot be swept a second time.
                SweepVerb::Mute => {
                    if done.insert(i)
                        && let Some(n) = notes.get_mut(i)
                    {
                        n.muted = !n.muted;
                    }
                }
                SweepVerb::Split => {
                    if done.insert(i) {
                        split_one(
                            notes,
                            i,
                            maybe_snap(g.beat_at(pos.x), grid_beats, mods.free),
                        );
                    }
                }
            }
            None
        }
    }
}

// --- the editor -----------------------------------------------------------

/// The whole editor, over `clip`'s notes, with no transport wired.
///
/// Kept as the plain-`body` entry point so a caller that has not yet
/// threaded a [`TimeView`] through still compiles and still works; it
/// simply gets no playhead, which `TimeView::playhead: None` says
/// honestly rather than drawing one parked at the origin.
#[allow(clippy::too_many_arguments)]
pub fn body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    pr: &mut PianoRoll,
    beats_per_bar: u32,
    clip: Option<&mut Clip>,
    key: Key,
    plocks: &[PlockParam],
) {
    let time = TimeView {
        beats_per_bar,
        ..TimeView::default()
    };
    body_at(ui, focus, theme, pr, time, clip, key, plocks, &[]);
}

/// The whole editor, over `clip`'s notes. Mirrors `arrangement_body`'s
/// shape: interact first so edits land the frame they are made, paint
/// after, register the cursor cell with `focus` last.
///
/// `clip` is the arrangement's selected clip — the notes are edited IN it,
/// not copied out. With `None` the grid still draws and still scrolls, but
/// nothing is editable and the empty state says why.
///
/// `ghosts` are notes from somewhere else — another clip on this track,
/// or a chosen other track — drawn for reference only. They are never
/// hit-tested, never selectable, and never edited; writing a part against
/// another part should not require remembering it.
#[allow(clippy::too_many_arguments)]
pub fn body_at(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    pr: &mut PianoRoll,
    time: TimeView,
    clip: Option<&mut Clip>,
    key: Key,
    plocks: &[PlockParam],
    ghosts: &[Note],
) {
    let area = ui.max_rect();
    claim(ui);
    pr.last_key = key;

    // The clip, taken apart once: its notes are what everything below
    // edits and draws, its length is where the extent rule goes, and its
    // start is what turns an absolute playhead into a clip-relative one.
    // Split borrows, field by field: the notes are held for the whole
    // draw, and the loop bar edits the length and the brace at the same
    // time. Borrowing the clip twice would end the first one.
    let (mut notes, mut span, clip_len, clip_start) = match clip {
        Some(c) => {
            let Clip {
                notes,
                len,
                start,
                loop_on,
                loop_start,
                loop_len,
                ..
            } = c;
            let (was_len, was_start) = (f64::from(*len), f64::from(*start));
            (
                Some(notes),
                Some(ClipSpan {
                    len,
                    loop_on,
                    loop_start,
                    loop_len,
                }),
                Some(was_len),
                was_start,
            )
        }
        None => (None, None, None, 0.0),
    };

    // The fold, before the layout: it changes how tall the pitch space is,
    // which the scroll clamp below depends on.
    let fold = pr.fold_mask(notes.as_deref().map(Vec::as_slice), key);
    pr.cursor_pitch = fold.nearest_shown(pr.cursor_pitch);

    let lay = layout(area, &pr.lanes);
    if lay.grid.width() <= 0.0 || lay.grid.height() <= 0.0 {
        return;
    }

    // Zoom, applied before anything is measured: every rect below derives
    // from it, so a mid-frame change would draw one frame at two scales.
    zoom_input(ui, pr, lay.grid, fold);

    let now = ui.input(|i| i.time);
    let grid_beats = pr.grid_beats();

    // First show: centre the view on middle C. Needs the panel's height,
    // which is why it cannot happen in `Default`.
    if !pr.centered {
        pr.scroll_y = (fold.row_of(C4) as f32 * pr.zoom.row_h - lay.grid.height() * 0.5).max(0.0);
        pr.centered = true;
    }

    // --- scrolling -----------------------------------------------------
    // Wheel y scrolls pitch, wheel x (or Shift+wheel, which egui maps onto
    // x) scrolls time. Events in quick succession grow a multiplier, so a
    // flick crosses octaves while a lone tick still moves one row's worth.
    let scroll = ui.input(|i| i.smooth_scroll_delta);
    if scroll != egui::Vec2::ZERO && ui.rect_contains_pointer(area) {
        pr.accel = accel_step(pr.accel, now - pr.last_scroll);
        pr.last_scroll = now;
        pr.last_user_scroll = now;
        pr.scroll_y -= scroll.y * pr.accel;
        pr.scroll_beats -= scroll.x * pr.accel / pr.zoom.px_per_beat;
    }

    // The keyboard moved the cursor last frame: scroll the MINIMUM that
    // brings its cell back inside the grid, on both axes. Minimum, not
    // centred — a centring scroll on every arrow press turns navigation
    // into seasickness.
    if std::mem::take(&mut pr.follow_cursor) {
        let g = Geom::new(lay.grid, pr.scroll_beats, pr.scroll_y, pr.zoom, fold);
        let cell_w = (grid_beats as f32 * g.px_per_beat()).max(1.0);
        let x = g.x_at(pr.cursor_beat);
        if x < g.grid.left() {
            pr.scroll_beats = pr.cursor_beat as f32;
        } else if x + cell_w > g.grid.right() {
            pr.scroll_beats =
                (pr.cursor_beat as f32 - (g.grid.width() - cell_w) / g.px_per_beat()).max(0.0);
        }
        let y = g.row_top(pr.cursor_pitch);
        let row = fold.row_of(pr.cursor_pitch) as f32;
        if y < g.grid.top() {
            pr.scroll_y = row * g.row_h();
        } else if y + g.row_h() > g.grid.bottom() {
            pr.scroll_y = (row * g.row_h() - (g.grid.height() - g.row_h())).max(0.0);
        }
    }

    // Follow the playhead — but never while the hand is still on the
    // wheel. A view that fights a scroll is worse than one that does not
    // follow at all.
    // The playhead, in the clip's own time — and FOLDED into the clip's
    // loop where it has one.
    //
    // Without the fold the marker walks off the end of the brace and
    // keeps going, while the clip it is supposedly marking has already
    // jumped back to the loop's start. A launched clip runs for as long
    // as it is playing, so the marker ends up arbitrarily far from the
    // bar it is playing.
    //
    // The same arithmetic that PLACES the repeats, run backwards: if the
    // two disagreed the marker would sit where the sound is not.
    let head = time.playhead.map(|p| p - clip_start).map(|at| {
        match span.as_ref().filter(|s| *s.loop_on) {
            Some(s) => {
                let (loop_start, loop_len) = s.brace();
                crate::loop_position(at, loop_start, loop_len)
            }
            None => at,
        }
    });
    if time.follow
        && now - pr.last_user_scroll > FOLLOW_YIELD
        && let Some(h) = head
    {
        let g = Geom::new(lay.grid, pr.scroll_beats, pr.scroll_y, pr.zoom, fold);
        let x = g.x_at(h);
        if x < g.grid.left() || x > g.grid.right() - g.grid.width() * FOLLOW_MARGIN {
            pr.scroll_beats =
                (h as f32 - g.grid.width() * FOLLOW_MARGIN / g.px_per_beat()).max(0.0);
        }
    }

    {
        let probe = Geom::new(lay.grid, pr.scroll_beats, pr.scroll_y, pr.zoom, fold);
        pr.scroll_y = pr
            .scroll_y
            .clamp(0.0, (probe.content_h() - lay.grid.height()).max(0.0));
    }
    pr.scroll_beats = pr.scroll_beats.max(0.0);

    let g = Geom::new(lay.grid, pr.scroll_beats, pr.scroll_y, pr.zoom, fold);
    let mods = Mods::read(ui);
    let tool = pr.tool_now(ui);

    // --- what the pointer is on ----------------------------------------
    // Hover is recomputed from geometry every frame and trusted for
    // exactly one. It is PURELY visual: no key's meaning depends on it.
    pr.hover = None;
    pr.hover_cell = None;
    if let Some(at) = ui
        .ctx()
        .pointer_latest_pos()
        .filter(|p| lay.grid.contains(*p))
    {
        if let Some(ns) = notes.as_deref() {
            pr.hover = hit_note(ns, g, at);
        }
        if let Some(p) = g.pitch_at(at.y) {
            pr.hover_cell = Some((p, g.beat_at(at.x)));
        }
    }

    // --- gestures -------------------------------------------------------
    //
    // ONE interaction over the grid, and the target captured AT THE PRESS
    // into `pr.drag`. The device UI contract's rule 1 exists because of a
    // gesture with no owner — a widget that re-runs a nearest-handle
    // search every frame — and this is the other way to satisfy it:
    // nothing is re-decided after the button goes down, so a drag that
    // crosses a neighbour, pins at a limit, or leaves the panel entirely
    // still belongs to whatever it began on. The pointer tests prove it.
    let resp = ui.interact(
        lay.grid,
        ui.id().with("pr_grid"),
        egui::Sense::click_and_drag(),
    );
    let mut band: Option<egui::Rect> = None;

    if let Some(ns) = notes.as_deref_mut() {
        // --- clicks -----------------------------------------------------
        if resp.double_clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            // Double-click on EMPTY ground adds a note. On a note it does
            // nothing destructive — it used to DELETE, which is a
            // permanent verb on a gesture people make by accident, and
            // which no reference DAW puts there.
            if hit_note(ns, g, at).is_none()
                && let Some(pitch) = g.pitch_at(at.y)
            {
                let beat = snap_floor(g.beat_at(at.x), grid_beats);
                pr.draw_note(ns, pitch, beat, grid_beats);
            }
        } else if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            let hit = hit_note(ns, g, at);
            match (tool, hit) {
                (Tool::Erase, Some((i, _))) => pr.remove_note(ns, i),
                (Tool::Mute, Some((i, _))) => {
                    if let Some(n) = ns.get_mut(i) {
                        n.muted = !n.muted;
                    }
                }
                (Tool::Split, _) => {
                    let beat = maybe_snap(g.beat_at(at.x), grid_beats, mods.free);
                    match hit {
                        Some((i, _)) => split_one(ns, i, beat),
                        None => {
                            // Split with nothing under the pointer cuts
                            // the whole column, which is what a scissors
                            // dragged down a bar is for.
                            let crossing: Vec<usize> = ns
                                .iter()
                                .enumerate()
                                .filter(|(_, n)| n.start < beat && n.start + n.len > beat)
                                .map(|(i, _)| i)
                                .collect();
                            for i in crossing {
                                split_one(ns, i, beat);
                            }
                        }
                    }
                    pr.cursor_beat = beat;
                }
                (Tool::Draw, None) => {
                    if let Some(pitch) = g.pitch_at(at.y) {
                        let beat = snap_floor(g.beat_at(at.x), grid_beats);
                        pr.draw_note(ns, pitch, beat, grid_beats);
                    }
                }
                (_, Some((i, _))) => {
                    // Ctrl toggles membership; Shift adds; a plain click
                    // replaces. The arrangement's grammar, unchanged.
                    if mods.command {
                        if !pr.selected.remove(&i) {
                            pr.selected.insert(i);
                        }
                    } else if mods.shift {
                        pr.selected.insert(i);
                    } else {
                        pr.selected.clear();
                        pr.selected.insert(i);
                    }
                    if let Some(n) = ns.get(i) {
                        pr.cursor_pitch = n.pitch;
                        pr.cursor_beat = n.start;
                    }
                }
                (_, None) => {
                    if !mods.command && !mods.shift {
                        pr.selected.clear();
                    }
                    if let Some(pitch) = g.pitch_at(at.y) {
                        pr.cursor_pitch = pitch;
                        pr.cursor_beat = snap_floor(g.beat_at(at.x), grid_beats);
                    }
                }
            }
            pr.anchor = None;
        }

        // --- press: the target is chosen here and nowhere else -----------
        //
        // `press_origin` rather than `interact_pointer_pos`: egui reports
        // `drag_started` on the frame the pointer has ALREADY moved past
        // its drag threshold, so the "press" position is a step or two
        // downstream of where the button actually went down. On a wide
        // control nobody notices. On a 24-point note it picks the resize
        // handle when the user pressed the middle of the body, and every
        // delta measured from it is short by one step.
        if resp.drag_started()
            && let Some(press) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| resp.interact_pointer_pos())
        {
            let hit = hit_note(ns, g, press);
            let marquee = |base: HashSet<usize>| Drag::Marquee { press, base };
            pr.drag = Some(match tool {
                Tool::Erase => Drag::Sweep {
                    verb: SweepVerb::Erase,
                    done: HashSet::new(),
                },
                Tool::Mute => Drag::Sweep {
                    verb: SweepVerb::Mute,
                    done: HashSet::new(),
                },
                Tool::Split => Drag::Sweep {
                    verb: SweepVerb::Split,
                    done: HashSet::new(),
                },
                Tool::Marquee => marquee(if mods.shift {
                    pr.selected.clone()
                } else {
                    HashSet::new()
                }),
                Tool::Pointer | Tool::Draw => match (hit, tool) {
                    (None, Tool::Draw) => match g.pitch_at(press.y) {
                        None => marquee(HashSet::new()),
                        Some(pitch) => {
                            // Draw: the note appears on the press and its
                            // right edge follows the hand, so ONE gesture
                            // both places it and sizes it.
                            let beat = snap_floor(g.beat_at(press.x), grid_beats);
                            let len = pr.draw_note(ns, pitch, beat, grid_beats);
                            Drag::ResizeR {
                                from: vec![(ns.len() - 1, len)],
                                press: egui::pos2(g.x_at(beat + len), press.y),
                            }
                        }
                    },
                    (None, _) => marquee(if mods.shift {
                        pr.selected.clone()
                    } else {
                        HashSet::new()
                    }),
                    (Some((i, zone)), _) => {
                        // A press on a note the selection does not contain
                        // takes the selection over — otherwise dragging an
                        // unselected note would move eight other ones.
                        if !pr.selected.contains(&i) {
                            if !mods.command && !mods.shift {
                                pr.selected.clear();
                            }
                            pr.selected.insert(i);
                        }
                        // Alt copies: the originals stay, the copies move,
                        // and the copies are what ends up selected.
                        let mut anchor = i;
                        if mods.copy && zone == Zone::Body {
                            let picked = pr.acting_on(ns);
                            let mut fresh = HashSet::new();
                            for &src in &picked {
                                let Some(n) = ns.get(src).cloned() else {
                                    continue;
                                };
                                ns.push(n);
                                let new = ns.len() - 1;
                                fresh.insert(new);
                                if src == i {
                                    anchor = new;
                                }
                            }
                            pr.selected = fresh;
                        }
                        let picked = pr.acting_on(ns);
                        match zone {
                            Zone::Body => Drag::Move {
                                anchor,
                                from: picked
                                    .iter()
                                    .filter_map(|&j| ns.get(j).map(|n| (j, n.pitch, n.start)))
                                    .collect(),
                                press,
                                axis: None,
                            },
                            Zone::Right => Drag::ResizeR {
                                from: picked
                                    .iter()
                                    .filter_map(|&j| ns.get(j).map(|n| (j, n.len)))
                                    .collect(),
                                press,
                            },
                            Zone::Left => Drag::ResizeL {
                                from: picked
                                    .iter()
                                    .filter_map(|&j| ns.get(j).map(|n| (j, n.start, n.len)))
                                    .collect(),
                                press,
                            },
                        }
                    }
                },
            });
        }

        // --- the drag itself ---------------------------------------------
        if resp.dragged()
            && let Some(at) = resp.interact_pointer_pos()
        {
            // Taken out and put back, so `pr` is free to be written while
            // the drag's own state is being read.
            let mut d = pr.drag.take();
            if let Some(d) = d.as_mut() {
                band = apply_drag(d, pr, ns, g, at, grid_beats, mods, key);
            }
            pr.drag = d;
        }
    }
    if resp.drag_stopped() {
        pr.drag = None;
    }

    // The pointer says what the gesture WOULD be, before it happens.
    if ui
        .ctx()
        .pointer_latest_pos()
        .is_some_and(|p| lay.grid.contains(p))
    {
        let icon = match (&pr.drag, tool, pr.hover) {
            (Some(Drag::Move { .. }), _, _) => egui::CursorIcon::Grabbing,
            (Some(Drag::ResizeL { .. } | Drag::ResizeR { .. }), _, _) => {
                egui::CursorIcon::ResizeHorizontal
            }
            (_, Tool::Pointer, Some((_, Zone::Left | Zone::Right))) => {
                egui::CursorIcon::ResizeHorizontal
            }
            (_, Tool::Pointer, Some((_, Zone::Body))) => egui::CursorIcon::Grab,
            (_, t, _) => t.cursor(),
        };
        ui.ctx().set_cursor_icon(icon);
    }

    // --- paint: the grid ground ------------------------------------------
    let painter = ui.painter().with_clip_rect(lay.grid);
    painter.rect_filled(lay.grid, 0.0, theme.surface);

    // Pitch rows: black-key rows recessed, an octave rule under every C.
    // Only the rows on screen, and — when folded — only the rows that
    // exist at all.
    for (row, pitch) in g.visible() {
        let y = g.row_y(row);
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(lay.grid.left(), y),
            egui::vec2(lay.grid.width(), g.row_h()),
        );
        if is_black_key(pitch) {
            painter.rect_filled(row_rect, 0.0, theme.surface_sunken);
        }
        // Rows OUTSIDE the working key are veiled rather than the in-key
        // rows being lit: the scale should read as the ground you write
        // on, not as decoration laid over it. The tonic keeps a brighter
        // rule so the key has a visible home row.
        if !key.scale.contains(key.tonic, pitch) {
            painter.rect_filled(row_rect, 0.0, theme.bg.gamma_multiply(OFF_SCALE_VEIL));
        } else if i16::from(pitch).rem_euclid(12) == i16::from(key.tonic).rem_euclid(12) {
            painter.line_segment(
                [
                    egui::pos2(lay.grid.left(), y + g.row_h()),
                    egui::pos2(lay.grid.right(), y + g.row_h()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.accent_muted),
            );
        }
        if pitch % 12 == 0 {
            painter.line_segment(
                [
                    egui::pos2(lay.grid.left(), y + g.row_h()),
                    egui::pos2(lay.grid.right(), y + g.row_h()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
    }

    // Beat grid: bars, beats, subdivisions — the arrangement's three
    // weights, dropped to beats when the subdivision would smear.
    paint_beat_lines(&painter, theme, g, grid_beats, time.beats_per_bar);

    // The clip's extent: everything past its end is washed out and ruled
    // off, because notes out there do not sound.
    if let Some(len) = clip_len {
        let end_x = g.x_at(len);
        if end_x < lay.grid.right() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(end_x.max(lay.grid.left()), lay.grid.top()),
                    lay.grid.max,
                ),
                0.0,
                theme.surface_sunken.gamma_multiply(PAST_END_VALUE),
            );
        }
        painter.line_segment(
            [
                egui::pos2(end_x, lay.grid.top()),
                egui::pos2(end_x, lay.grid.bottom()),
            ],
            egui::Stroke::new(stroke::HAIR, theme.accent_muted),
        );
    }

    // --- paint: the ghosts, behind everything the clip owns ---------------
    // Flat, neutral, and unlabelled: a ghost is a reminder, and the moment
    // one competes with a real note for attention it has stopped helping.
    for n in ghosts {
        let r = g.note_rect(n);
        if !g.spans_x(r) || r.bottom() < lay.grid.top() || r.top() > lay.grid.bottom() {
            continue;
        }
        painter.rect_filled(
            r,
            radius::CTRL,
            theme.note_ghost.gamma_multiply(GHOST_ALPHA),
        );
    }

    // A valid chord-entry phrase is visible but untouchable until Enter.
    // It uses the accent rather than the neutral cross-clip ghost colour:
    // one is a proposed edit, the other is reference material.
    if let Some(entry) = &pr.chord_entry
        && let Ok(plan) = parse_chord_entry(
            &entry.text,
            pr.cursor_pitch,
            pr.cursor_beat,
            grid_beats,
            key,
            time.beats_per_bar,
        )
    {
        for n in &plan.notes {
            let r = g.note_rect(n);
            if !g.spans_x(r) || r.bottom() < lay.grid.top() || r.top() > lay.grid.bottom() {
                continue;
            }
            painter.rect_filled(r, radius::CTRL, theme.accent_muted.gamma_multiply(0.72));
            painter.rect_stroke(
                r,
                radius::CTRL,
                egui::Stroke::new(stroke::HAIR, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
    }

    // --- paint: the clip's own LOOP repeats --------------------------------
    //
    // What the brace will actually play, drawn where it will play it.
    // Only the head pass is editable — you edit the loop, not one of its
    // repetitions — so the repeats are ghosts: visible, and not in the
    // way. Without them a looping clip shows its pattern once and then
    // apparently nothing, which is a picture of a different clip from the
    // one you can hear.
    if let Some(span) = span.as_ref()
        && *span.loop_on
    {
        let len = f64::from(*span.len).max(0.0);
        let (loop_start, loop_len) = span.brace();
        let loop_end = loop_start + loop_len;
        if loop_len > 0.0 && loop_end < len {
            // The SAME arithmetic the compile uses, minus the head — the
            // head is the pass you can edit and is drawn as real notes
            // below.
            let repeats = crate::loop_repeats(
                notes.as_deref().map(Vec::as_slice).unwrap_or_default(),
                loop_start,
                loop_len,
                len,
                MAX_DRAWN_PASSES,
            );
            for ghost in &repeats {
                let r = g.note_rect(ghost);
                if !g.spans_x(r) || r.bottom() < lay.grid.top() || r.top() > lay.grid.bottom() {
                    continue;
                }
                painter.rect_filled(
                    r,
                    radius::CTRL,
                    theme.clip_note.gamma_multiply(LOOP_GHOST_ALPHA),
                );
            }
        }
    }

    // --- paint: the notes -------------------------------------------------
    let label_font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    let name_font = egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Proportional);
    for (i, n) in notes
        .as_deref()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let r = g.note_rect(n);
        if !g.spans_x(r) || r.bottom() < lay.grid.top() || r.top() > lay.grid.bottom() {
            continue;
        }
        let st = NoteState {
            selected: pr.selected.contains(&i),
            hovered: pr.hover.is_some_and(|(h, _)| h == i),
            muted: n.muted,
            past_end: clip_len.is_some_and(|len| n.start >= len),
            conditional: n.prob < 1.0 || n.cond.is_some(),
            vel: n.vel,
        };
        let paint = note_paint(theme, st);
        if paint.hollow {
            painter.rect_stroke(
                r.shrink(1.0),
                radius::CTRL,
                egui::Stroke::new(stroke::HAIR, paint.fill),
                egui::StrokeKind::Middle,
            );
        } else {
            painter.rect_filled(r, radius::CTRL, paint.fill);
        }
        painter.rect_stroke(r, radius::CTRL, paint.edge, egui::StrokeKind::Middle);

        // A conditional or probabilistic note wears a hollow ring at its
        // right shoulder — this note does not fire every cycle, said
        // before you open anything.
        if st.conditional {
            painter.circle_stroke(
                r.right_top() + egui::vec2(-3.0, 3.0),
                1.5,
                egui::Stroke::new(stroke::HAIR, theme.accent),
            );
        }
        // A parameter-locked note wears a dot — the promise that this note
        // sounds DIFFERENT from its neighbours, visible before you open
        // the editor to find out how.
        if !n.plocks.is_empty() {
            painter.circle_filled(r.left_top() + egui::vec2(3.0, 3.0), 1.5, theme.accent);
        }
        // The note's own name, once there is honestly room for it. A
        // clipped label is worse than no label.
        if r.width() >= NAME_MIN_W && r.height() >= NAME_MIN_H {
            painter.text(
                r.left_center() + egui::vec2(3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                note_name(n.pitch),
                name_font.clone(),
                theme.note_edge.gamma_multiply(NAME_ALPHA),
            );
        }
    }

    // Nothing to edit: say so rather than showing a grid that ignores
    // every click, which reads as broken.
    if notes.is_none() {
        painter.text(
            lay.grid.center(),
            egui::Align2::CENTER_CENTER,
            NO_CLIP,
            egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
            theme.text_muted,
        );
    }

    // The rubber band, over the notes it is sweeping.
    if let Some(b) = band {
        let b = b.intersect(lay.grid);
        painter.rect_filled(b, 0.0, theme.loop_region);
        painter.rect_stroke(
            b,
            0.0,
            egui::Stroke::new(stroke::HAIR, theme.accent_muted),
            egui::StrokeKind::Middle,
        );
    }

    // The playhead, over everything in the grid. Drawn only when the
    // caller actually gave us one.
    if let Some(h) = head {
        let x = g.x_at(h);
        if x >= lay.grid.left() && x <= lay.grid.right() {
            // Heavier while it is moving: a parked playhead is a
            // reference mark, a running one is the most important thing
            // on the panel.
            let weight = if time.playing {
                stroke::BOLD
            } else {
                stroke::HAIR
            };
            painter.line_segment(
                [
                    egui::pos2(x, lay.grid.top()),
                    egui::pos2(x, lay.grid.bottom()),
                ],
                egui::Stroke::new(weight, theme.playhead),
            );
        }
    }

    // --- the corner: grid rung and working key ---------------------------
    if lay.corner.height() > 0.0 {
        let painter = ui.painter().with_clip_rect(lay.corner);
        painter.rect_filled(lay.corner, 0.0, theme.surface_raised);
        painter.text(
            lay.corner.left_center() + egui::vec2(LABEL_PAD, 0.0),
            egui::Align2::LEFT_CENTER,
            GRID_NAMES[pr.grid.min(GRID_NAMES.len() - 1)],
            label_font.clone(),
            theme.text_muted,
        );
    }

    // --- the ruler --------------------------------------------------------
    if lay.ruler.height() > 0.0 {
        ruler(
            ui,
            theme,
            pr,
            lay.ruler,
            g,
            time.beats_per_bar,
            head,
            grid_beats,
        );
        // AFTER the ruler, so its handles win the pointer where the two
        // overlap — the ruler's own interaction covers the whole strip.
        if let Some(span) = span.as_mut() {
            loop_bar(ui, theme, lay.ruler, g, span, grid_beats);
        }
    }

    // --- the keyboard gutter ----------------------------------------------
    gutter(
        ui,
        theme,
        pr,
        lay.keys,
        g,
        notes.as_deref().map(Vec::as_slice),
    );

    // --- the tool strip ---------------------------------------------------
    tool_strip(ui, theme, pr, lay.tools, tool);

    // --- the parameter-lock and trig editors ------------------------------
    if let Some(ns) = notes.as_deref_mut() {
        plock_overlay(ui, theme, pr, ns, plocks, g);
        trig_overlay(ui, theme, pr, ns, g);
    }

    // --- the expression lanes ---------------------------------------------
    for (idx, gutter_rect, body_rect) in lay.lanes.clone() {
        expression_lane(
            ui,
            theme,
            pr,
            notes.as_deref_mut(),
            idx,
            gutter_rect,
            body_rect,
            g,
            grid_beats,
        );
    }

    // --- the info line ------------------------------------------------------
    if lay.info.height() > 0.0 {
        info_line(
            ui,
            theme,
            pr,
            notes.as_deref().map(Vec::as_slice),
            lay.info,
            tool,
            key,
            time.beats_per_bar,
        );
    }

    // --- the keyboard cursor -------------------------------------------------
    // Register the cursor CELL, like the arrangement's lanes do: the ring
    // wraps the cell, and its presence is what lets `keys` claim input.
    let cell = egui::Rect::from_min_size(
        egui::pos2(g.x_at(pr.cursor_beat), g.row_top(pr.cursor_pitch)),
        egui::vec2(grid_beats as f32 * g.px_per_beat(), g.row_h()),
    );
    let wid = ui.id().with("pr_cell");
    // Working in the roll with the POINTER takes the keyboard with it. The
    // ring is what `keys` claims its input from, so without this the roll
    // can plainly be the thing under the hand while `i` — and every other
    // verb — goes wherever the ring was last left. A press counts, not just
    // a completed click, so the keyboard arrives with the gesture; a ruler
    // scrub counts too, which is what `locate` being set means.
    if resp.is_pointer_button_down_on() || resp.clicked() || pr.locate.is_some() {
        focus.claim(wid);
    }
    pr.owns_keys = focus.register(wid, cell.intersect(lay.grid));

    // What the editor would say out loud. The roll is already
    // keyboard-complete; this is what makes it legible to something other
    // than eyes.
    let readout = pr.cursor_readout(
        notes.as_deref().map(Vec::as_slice).unwrap_or_default(),
        time.beats_per_bar,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, readout.clone()));
}

// --- the pieces the editor is made of ------------------------------------

/// How much of the grid's width follow keeps AHEAD of the playhead, so the
/// music you are about to hear is on screen rather than the music you just
/// heard.
const FOLLOW_MARGIN: f32 = 0.15;
/// How solid a ghost is. Present enough to read against, faint enough
/// that no one ever tries to click one.
const GHOST_ALPHA: f32 = 0.55;
/// A note has to be at least this big before its own name fits on it.
const NAME_MIN_W: f32 = 34.0;
const NAME_MIN_H: f32 = 11.0;
/// How loud a name on a note is. Quiet: the note is the mark, the name
/// only confirms it.
const NAME_ALPHA: f32 = 0.75;
/// The narrowest a bar label may be drawn at before the ruler thins out to
/// every fourth bar. Below that the numbers collide and none of them read.
const BAR_LABEL_MIN_PX: f32 = 44.0;

/// Bars, beats and subdivisions, in the arrangement's three weights.
fn paint_beat_lines(
    painter: &egui::Painter,
    theme: &Theme,
    g: Geom,
    grid_beats: f64,
    beats_per_bar: u32,
) {
    let sub = grid_beats as f32;
    let step = if sub * g.px_per_beat() < GRID_MIN_PX {
        1.0
    } else {
        sub
    };
    // And if even a whole beat would smear, thin out to bars. Without
    // this the grid becomes a solid wash the moment you zoom out.
    let per_bar = beats_per_bar.max(1) as f32;
    let step = if step * g.px_per_beat() < GRID_MIN_PX {
        per_bar
    } else {
        step
    };
    let mut beat = (g.sb / step).floor() * step;
    loop {
        let x = g.x_at(f64::from(beat));
        if x > g.grid.right() {
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
            [egui::pos2(x, g.grid.top()), egui::pos2(x, g.grid.bottom())],
            egui::Stroke::new(stroke::HAIR, colour),
        );
        beat += step;
    }
}

/// How solid a loop repeat is drawn against a real note. Present enough
/// to read the pattern, quiet enough that the pass you can actually edit
/// is obviously the bright one.
const LOOP_GHOST_ALPHA: f32 = 0.38;

/// The most repeats the roll will DRAW. A picture of four thousand
/// passes is a picture of a solid block, and the ones past the right edge
/// were never visible anyway.
const MAX_DRAWN_PASSES: usize = 512;

/// Height of the loop bar along the top of the ruler, in points.
const LOOP_BAR_H: f32 = 10.0;
/// Width of the loop toggle at the bar's left end, and of each brace
/// handle.
const LOOP_TOGGLE_W: f32 = 12.0;
const LOOP_GRIP_W: f32 = 7.0;

/// The parts of a clip the loop bar edits, borrowed field by field.
///
/// Split borrows rather than the whole clip, because the roll is holding
/// `&mut clip.notes` for the entire draw — everything below it edits
/// notes — and a second `&mut` to the clip would end that.
pub struct ClipSpan<'a> {
    pub len: &'a mut f32,
    pub loop_on: &'a mut bool,
    pub loop_start: &'a mut f32,
    pub loop_len: &'a mut f32,
}

impl ClipSpan<'_> {
    /// The brace, resolved and made sane. A clip whose brace was never
    /// set — every clip written before braces existed — reads as its own
    /// whole length, so switching the loop ON does something obvious
    /// rather than nothing.
    fn brace(&self) -> (f64, f64) {
        let len = f64::from(*self.len).max(0.0);
        let start = f64::from(*self.loop_start).clamp(0.0, len);
        let span = if *self.loop_len > 0.0 {
            f64::from(*self.loop_len)
        } else {
            len
        };
        (start, span.min((len - start).max(0.0)))
    }
}

/// The loop bar: the clip's extent, its loop brace, and the handles that
/// move them.
///
/// It lives in the TOP strip of the ruler, which is where Ableton puts it
/// and where it is out of the way of the bar numbers. Its handles are
/// allocated after the ruler's own interaction, so they win the pointer
/// where the two overlap — the device UI contract's rule about the last
/// widget at a position.
fn loop_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    g: Geom,
    span: &mut ClipSpan<'_>,
    grid_beats: f64,
) {
    let bar = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.top() + LOOP_BAR_H.min(rect.height())),
    );
    let painter = ui.painter().with_clip_rect(bar);
    painter.rect_filled(bar, 0.0, theme.surface_sunken.gamma_multiply(0.6));

    let over = g.over(rect);
    let x_at = |beat: f64| over.x_at(beat);
    let beat_at = |x: f32| snap(over.beat_at(x), grid_beats).max(0.0);

    // --- the toggle -------------------------------------------------
    let toggle = egui::Rect::from_min_size(bar.min, egui::vec2(LOOP_TOGGLE_W, bar.height()));
    let t = ui.interact(toggle, ui.id().with("pr_loop_toggle"), egui::Sense::click());
    if t.clicked() {
        *span.loop_on = !*span.loop_on;
        // Turning it on with no brace set adopts the whole clip, so the
        // first click loops what you are looking at instead of nothing.
        if *span.loop_on && *span.loop_len <= 0.0 {
            *span.loop_start = 0.0;
            *span.loop_len = *span.len;
        }
    }
    let lit = *span.loop_on;
    painter.rect_filled(
        toggle.shrink(1.0),
        1.0,
        if lit {
            theme.accent
        } else if t.hovered() {
            theme.outline
        } else {
            theme.surface_raised
        },
    );
    painter.text(
        toggle.center(),
        egui::Align2::CENTER_CENTER,
        "⟲",
        egui::FontId::proportional(font::MINI_LABEL),
        if lit { theme.surface } else { theme.text_muted },
    );
    t.on_hover_text("loop this clip (its brace repeats to fill it)");

    let clip_len = f64::from(*span.len).max(0.0);
    let (loop_start, loop_len) = span.brace();
    let loop_end = loop_start + loop_len;

    // --- the clip's own extent, and the end handle -------------------
    let end_x = x_at(clip_len);
    painter.line_segment(
        [
            egui::pos2(x_at(0.0), bar.bottom() - 0.5),
            egui::pos2(end_x, bar.bottom() - 0.5),
        ],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    // --- the brace ---------------------------------------------------
    if lit {
        let body = egui::Rect::from_x_y_ranges(x_at(loop_start)..=x_at(loop_end), bar.y_range());
        painter.rect_filled(body, 0.0, theme.accent.gamma_multiply(0.35));
        painter.rect_stroke(
            body,
            0.0,
            egui::Stroke::new(stroke::HAIR, theme.accent),
            egui::StrokeKind::Inside,
        );

        // Body drag: move the whole brace, keeping its length.
        let inner = body.shrink2(egui::vec2(LOOP_GRIP_W, 0.0));
        if inner.width() > 1.0 {
            let m = ui.interact(inner, ui.id().with("pr_brace_move"), egui::Sense::drag());
            if m.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if m.dragged()
                && let Some(at) = m.interact_pointer_pos()
            {
                let want = beat_at(at.x) - loop_len * 0.5;
                *span.loop_start = want.clamp(0.0, (clip_len - loop_len).max(0.0)) as f32;
                *span.loop_len = loop_len as f32;
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }

        // The two ends, each its own interaction so a drag that began on
        // one stays on it past the other.
        for (name, at_beat, is_start) in [
            ("pr_brace_start", loop_start, true),
            ("pr_brace_end", loop_end, false),
        ] {
            let x = x_at(at_beat);
            // Where the brace's end lands on the CLIP's end — which it
            // does for every clip that loops the whole of itself, so most
            // of them — only one handle is allocated, and it is the
            // clip's. Two draggable targets on one pixel is the
            // nearest-handle ambiguity the UI contract exists to forbid,
            // and leaving it to z-order would make which one you got a
            // matter of drawing order rather than of intent.
            if !is_start && (x - end_x).abs() < LOOP_GRIP_W {
                continue;
            }
            let grip = egui::Rect::from_min_size(
                egui::pos2(x - LOOP_GRIP_W * 0.5, bar.top()),
                egui::vec2(LOOP_GRIP_W, bar.height()),
            );
            let h = ui.interact(grip, ui.id().with(name), egui::Sense::drag());
            if h.hovered() || h.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                painter.rect_filled(grip, 0.0, theme.accent);
            }
            if h.dragged()
                && let Some(pos) = h.interact_pointer_pos()
            {
                let want = beat_at(pos.x);
                if is_start {
                    // Moving the start moves the start ONLY: the end stays
                    // where it was, which is what a brace edge means.
                    let start = want.clamp(0.0, (loop_end - crate::MIN_LOOP_BEATS as f64).max(0.0));
                    *span.loop_start = start as f32;
                    *span.loop_len = (loop_end - start) as f32;
                } else {
                    let end = want.clamp(loop_start + crate::MIN_LOOP_BEATS as f64, clip_len);
                    *span.loop_len = (end - loop_start) as f32;
                }
            }
        }
    }

    // The CLIP END, last, so it wins over a brace edge that lands on it —
    // and it is the one you reach for to make a launcher clip a different
    // number of bars from its neighbours.
    let grip = egui::Rect::from_min_size(
        egui::pos2(end_x - LOOP_GRIP_W * 0.5, bar.top()),
        egui::vec2(LOOP_GRIP_W, bar.height()),
    );
    let e = ui.interact(grip, ui.id().with("pr_clip_end"), egui::Sense::drag());
    if e.hovered() || e.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    painter.line_segment(
        [
            egui::pos2(end_x, bar.top()),
            egui::pos2(end_x, bar.bottom()),
        ],
        egui::Stroke::new(
            stroke::BOLD,
            if e.hovered() || e.dragged() {
                theme.accent
            } else {
                theme.text_muted
            },
        ),
    );
    if e.dragged()
        && let Some(pos) = e.interact_pointer_pos()
    {
        let want = beat_at(pos.x).max(f64::from(crate::MIN_LOOP_BEATS));
        *span.len = want as f32;
        // A brace cannot outlive its clip: shortening the clip over the
        // brace takes the brace with it rather than leaving a loop that
        // points past the end.
        let (start, len) = span.brace();
        *span.loop_start = start as f32;
        *span.loop_len = len.max(f64::from(crate::MIN_LOOP_BEATS)) as f32;
    }
    e.on_hover_text("drag to set the clip's length");
}

/// The bar ruler: numbers, the playhead, and a click that locates.
///
/// Bars are numbered from the CLIP's own start, which is Ableton's
/// convention and the only one that survives moving the clip along the
/// timeline. The absolute position stays available in the info line.
#[allow(clippy::too_many_arguments)]
fn ruler(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    rect: egui::Rect,
    g: Geom,
    beats_per_bar: u32,
    head: Option<f64>,
    grid_beats: f64,
) {
    let resp = ui.interact(
        rect,
        ui.id().with("pr_ruler"),
        egui::Sense::click_and_drag(),
    );
    // Click or scrub: the cursor moves, and a locate request is left for
    // the app to pick up. The roll cannot move the transport itself — it
    // does not own one — so it asks, and clears the ask when it is taken.
    if (resp.clicked() || resp.dragged())
        && let Some(at) = resp.interact_pointer_pos()
    {
        let beat = snap(g.over(rect).beat_at(at.x), grid_beats);
        pr.cursor_beat = beat;
        pr.locate = Some(beat);
        pr.anchor = None;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme.surface_raised);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    let per_bar = f64::from(beats_per_bar.max(1));
    let bar_px = per_bar as f32 * g.px_per_beat();
    // One label per bar while they fit, then every fourth. Sixteen bar
    // numbers overlapping each other is not sixteen bar numbers.
    let every = if bar_px >= BAR_LABEL_MIN_PX {
        1
    } else if bar_px * 4.0 >= BAR_LABEL_MIN_PX {
        4
    } else {
        16
    };
    let first = (f64::from(g.sb) / per_bar).floor().max(0.0) as i64;
    let mut bar = first - first.rem_euclid(every);
    loop {
        let beat = bar as f64 * per_bar;
        let x = g.x_at(beat);
        if x > rect.right() {
            break;
        }
        if x >= rect.left() - bar_px {
            painter.line_segment(
                [
                    egui::pos2(x, rect.bottom() - RULER_TICK),
                    egui::pos2(x, rect.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.grid_bar),
            );
            painter.text(
                egui::pos2(x + LABEL_PAD, rect.top() + 1.0),
                egui::Align2::LEFT_TOP,
                format!("{}", bar + 1),
                font.clone(),
                theme.text_muted,
            );
        }
        bar += every;
    }

    // The playhead's own head, so the line down the grid has something to
    // hang from and stays findable when the grid is busy.
    if let Some(h) = head {
        let x = g.x_at(h);
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(stroke::BOLD, theme.playhead),
            );
        }
    }
}

/// How tall a ruler tick is.
const RULER_TICK: f32 = 5.0;

/// The piano keyboard down the left.
///
/// Clicking a key moves the cursor to that pitch, which is the closest
/// thing to playing it the editor can honestly offer until the audition
/// path exists. It is also how you get the cursor onto a distant row
/// without arrowing there.
fn gutter(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    rect: egui::Rect,
    g: Geom,
    notes: Option<&[Note]>,
) {
    let resp = ui.interact(rect, ui.id().with("pr_keys"), egui::Sense::click_and_drag());
    if (resp.clicked() || resp.dragged())
        && let Some(at) = resp.interact_pointer_pos()
        && let Some(pitch) = g.over(rect).pitch_at(at.y)
    {
        pr.cursor_pitch = pitch;
        pr.follow_cursor = true;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Which pitches the clip is actually using — the gutter marks them, so
    // a folded-out view and an unfolded one tell the same story about
    // where the material is.
    let used = notes.map(Fold::used).unwrap_or(Fold { mask: 0 });

    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme.surface);
    let font = egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace);
    // Every note gets a name once the rows are tall enough to hold one;
    // below that, only the Cs, which is what you navigate by anyway.
    let name_all = g.row_h() >= 13.0;
    for (row, pitch) in g.visible() {
        let y = g.row_y(row);
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), y),
            egui::vec2(rect.width(), g.row_h()),
        );
        if is_black_key(pitch) {
            painter.rect_filled(row_rect, 0.0, theme.surface_sunken);
        }
        // The row the keyboard cursor is on, so the cursor is findable
        // even when it is off the right-hand side of the view.
        if pitch == pr.cursor_pitch {
            painter.rect_filled(row_rect, 0.0, theme.accent_muted.gamma_multiply(0.5));
        }
        if used.shows(pitch) {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.right() - USED_TAB_W, y),
                    egui::vec2(USED_TAB_W, g.row_h().max(1.0)),
                ),
                0.0,
                theme.note_fill.gamma_multiply(0.8),
            );
        }
        if (name_all || pitch % 12 == 0) && g.row_h() >= 7.0 {
            painter.text(
                egui::pos2(rect.right() - LABEL_PAD - USED_TAB_W, y + g.row_h() * 0.5),
                egui::Align2::RIGHT_CENTER,
                note_name(pitch),
                font.clone(),
                if pitch % 12 == 0 {
                    theme.text
                } else {
                    theme.text_muted
                },
            );
        }
    }
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
}

/// The width of the "this pitch is in use" tab on the gutter's inner edge.
const USED_TAB_W: f32 = 3.0;

/// The vertical tool strip.
///
/// `active` is the tool in force, which may be one borrowed by a held key
/// rather than the latched one — so the strip shows what will happen if
/// you press the button NOW, not what you last clicked.
fn tool_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    rect: egui::Rect,
    active: Tool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme.surface_raised);
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    let cell = TOOLS_W;
    for (n, tool) in Tool::ALL.into_iter().enumerate() {
        let top = rect.top() + n as f32 * cell;
        if top + cell > rect.bottom() {
            // Out of room: the strip degrades to the tools that fit, and
            // the keyboard still reaches all six.
            break;
        }
        let button =
            egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(rect.width(), cell));
        // One target, one interaction, its own id — the device UI
        // contract's rule 1, in the place it is cheapest to obey.
        let resp = ui.interact(button, ui.id().with(("pr_tool", n)), egui::Sense::click());
        if resp.clicked() {
            pr.tool = tool;
        }
        let on = tool == active;
        if on {
            painter.rect_filled(button, 0.0, theme.accent_muted);
        } else if resp.hovered() {
            painter.rect_filled(button, 0.0, theme.surface);
        }
        painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            tool.glyph(),
            font.clone(),
            if on { theme.text } else { theme.text_muted },
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.on_hover_text(format!("{} ({})", tool.label(), n + 1));
    }
}

/// One expression lane: its gutter, its bars, and the drags that paint it.
///
/// Every lane shares this code and differs only in which [`Lane`] it
/// carries, which is the point — a lane that edits pan and a lane that
/// edits velocity must not be two separate implementations that drift.
#[allow(clippy::too_many_arguments)]
fn expression_lane(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &mut PianoRoll,
    mut notes: Option<&mut Vec<Note>>,
    idx: usize,
    gutter_rect: egui::Rect,
    body_rect: egui::Rect,
    g: Geom,
    grid_beats: f64,
) {
    let Some(view) = pr.lanes.get(idx).copied() else {
        return;
    };
    let lane = view.lane;
    let lg = g.over(body_rect);

    // --- the gutter: name, and a click that collapses ------------------
    let head_resp = ui.interact(
        gutter_rect,
        ui.id().with(("pr_lane_head", idx)),
        egui::Sense::click(),
    );
    if head_resp.clicked() {
        // Plain click folds the lane to its header; Ctrl+click closes it
        // altogether. Same grammar as everywhere else: Ctrl removes.
        if ui.input(|i| i.modifiers.command) {
            pr.toggle_lane(lane);
            return;
        } else if let Some(lv) = pr.lanes.get_mut(idx) {
            lv.collapsed = !lv.collapsed;
        }
    }
    if head_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // --- the top edge: a resize target of its own ----------------------
    // Its own rect and its own id, so a drag that begins on the edge
    // stays a resize even when the pointer wanders into the bars.
    let edge = egui::Rect::from_min_max(
        egui::pos2(gutter_rect.left(), gutter_rect.top() - LANE_EDGE_W * 0.5),
        egui::pos2(body_rect.right(), gutter_rect.top() + LANE_EDGE_W * 0.5),
    );
    let edge_resp = ui.interact(
        edge,
        ui.id().with(("pr_lane_edge", idx)),
        egui::Sense::drag(),
    );
    if edge_resp.hovered() || matches!(pr.drag, Some(Drag::LaneResize { .. })) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if edge_resp.drag_started()
        && let Some(press) = edge_resp.interact_pointer_pos()
    {
        pr.drag = Some(Drag::LaneResize {
            idx,
            h0: view.height(),
            press,
        });
    }
    if edge_resp.dragged()
        && let Some(at) = edge_resp.interact_pointer_pos()
        && let Some(Drag::LaneResize { idx, h0, press }) = pr.drag
        && let Some(lv) = pr.lanes.get_mut(idx)
    {
        // Dragging the top edge UP makes the lane taller.
        lv.h = (h0 + (press.y - at.y)).clamp(LANE_H_MIN, LANE_H_MAX);
        lv.collapsed = false;
    }
    if edge_resp.drag_stopped() && matches!(pr.drag, Some(Drag::LaneResize { .. })) {
        pr.drag = None;
    }

    // --- the bars ------------------------------------------------------
    let mods = Mods::read(ui);
    if !view.collapsed && body_rect.height() > 2.0 {
        let resp = ui.interact(
            body_rect,
            ui.id().with(("pr_lane", idx)),
            egui::Sense::click_and_drag(),
        );
        if resp.drag_started()
            && let Some(press) = resp.interact_pointer_pos()
        {
            // Nearest bar within reach of the press. If it is already
            // selected, the whole selection is painted — which is how you
            // set eight velocities with one gesture.
            let mut best: Option<(usize, f32)> = None;
            for (i, n) in notes
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                let d = (lg.x_at(n.start) - press.x).abs();
                if d <= VEL_PICK_PX && best.is_none_or(|(_, b)| d < b) {
                    best = Some((i, d));
                }
            }
            let targets = match best {
                Some((i, _)) if pr.selected.contains(&i) => {
                    let mut v: Vec<usize> = pr.selected.iter().copied().collect();
                    v.sort_unstable();
                    v
                }
                Some((i, _)) => vec![i],
                // A press on empty lane ground with a selection ramps the
                // selection: Shift-drag across it and the values land on
                // a straight line.
                None if mods.shift && !pr.selected.is_empty() => {
                    let mut v: Vec<usize> = pr.selected.iter().copied().collect();
                    v.sort_unstable();
                    v
                }
                None => Vec::new(),
            };
            pr.drag = Some(Drag::Lane {
                lane,
                targets,
                press,
                ramp: mods.shift,
            });
        }
        if resp.dragged()
            && let Some(at) = resp.interact_pointer_pos()
            && matches!(pr.drag, Some(Drag::Lane { .. }))
        {
            let mut d = pr.drag.take();
            if let (Some(d), Some(ns)) = (d.as_mut(), notes.as_deref_mut()) {
                apply_drag(d, pr, ns, lg, at, grid_beats, mods, Key::default());
            }
            pr.drag = d;
        }
        if resp.drag_stopped() && matches!(pr.drag, Some(Drag::Lane { .. })) {
            pr.drag = None;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
    }

    // --- paint ----------------------------------------------------------
    let painter = ui.painter().with_clip_rect(gutter_rect.union(body_rect));
    painter.rect_filled(gutter_rect, 0.0, theme.surface_raised);
    painter.rect_filled(body_rect, 0.0, theme.surface_sunken);
    painter.line_segment(
        [gutter_rect.left_top(), body_rect.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    painter.text(
        gutter_rect.left_center() + egui::vec2(LABEL_PAD, 0.0),
        egui::Align2::LEFT_CENTER,
        if view.collapsed {
            format!("{} \u{25b8}", lane.label())
        } else {
            format!("{} \u{25be}", lane.label())
        },
        egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
        theme.text_muted,
    );
    if view.collapsed {
        return;
    }

    let usable = (body_rect.height() - VEL_LANE_PAD).max(1.0);
    for (i, n) in notes
        .as_deref()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let x = lg.x_at(n.start);
        if x < body_rect.left() || x > body_rect.right() {
            continue;
        }
        let h = usable * lane.norm(n);
        // The bar is the NOTE's colour, so a lane reads as the same
        // material as the grid above it rather than as a separate chart.
        let paint = note_paint(
            theme,
            NoteState {
                selected: pr.selected.contains(&i),
                hovered: pr.hover.is_some_and(|(hi, _)| hi == i),
                muted: n.muted,
                past_end: false,
                conditional: n.prob < 1.0 || n.cond.is_some(),
                vel: PITCH_MAX,
            },
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x - VEL_BAR_W * 0.5, body_rect.bottom() - h),
                egui::pos2(x + VEL_BAR_W * 0.5, body_rect.bottom()),
            ),
            0.0,
            paint.fill,
        );
    }
}

/// How wide the grab zone on a lane's top edge is.
const LANE_EDGE_W: f32 = 6.0;

/// The info line: what the pointer is on, else what is selected, else
/// where the cursor is — and, on the right, the modes in force.
///
/// This is the answer to "what am I looking at", and it is always present.
/// Logic and Cubase both have one and both are right to: an editor that
/// can only tell you a note's velocity by you dragging it is an editor
/// that makes you change things to read them.
#[allow(clippy::too_many_arguments)]
fn info_line(
    ui: &mut egui::Ui,
    theme: &Theme,
    pr: &PianoRoll,
    notes: Option<&[Note]>,
    rect: egui::Rect,
    tool: Tool,
    key: Key,
    beats_per_bar: u32,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme.surface_raised);
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);

    let ns = notes.unwrap_or_default();
    // Precedence: a lane drag in flight, then the pointer, then the
    // selection, then the cursor. Whatever is MOVING wins, because that
    // is the number the hand is currently choosing.
    let left = if pr.chord_entry.is_some() {
        "music script open in arrangement".to_owned()
    } else if let Some(Drag::Lane { lane, targets, .. }) = &pr.drag {
        let face = targets
            .first()
            .and_then(|&i| ns.get(i))
            .map_or_else(|| "\u{2014}".to_owned(), |n| lane.face(n));
        format!(
            "{} \u{2192} {}   ({} note{})",
            lane.label(),
            face,
            targets.len(),
            if targets.len() == 1 { "" } else { "s" }
        )
    } else if let Some(n) = pr.hover.and_then(|(i, _)| ns.get(i)) {
        let mut s = format!(
            "{:<4} {:>10}  len {:<7.3} vel {:>3}",
            note_name(n.pitch),
            bbt(n.start, beats_per_bar),
            n.len,
            n.vel
        );
        if n.prob < 1.0 {
            s.push_str(&format!("  prob {:.0}%", n.prob * 100.0));
        }
        if let Some((a, b)) = n.cond {
            s.push_str(&format!("  {a}:{b}"));
        }
        if !n.plocks.is_empty() {
            s.push_str(&format!("  \u{2022}{}", n.plocks.len()));
        }
        if n.muted {
            s.push_str("  off");
        }
        s
    } else if !pr.selected.is_empty() {
        let picked: Vec<&Note> = pr.selected.iter().filter_map(|&i| ns.get(i)).collect();
        let lo_p = picked.iter().map(|n| n.pitch).min().unwrap_or(0);
        let hi_p = picked.iter().map(|n| n.pitch).max().unwrap_or(0);
        let lo_v = picked.iter().map(|n| n.vel).min().unwrap_or(0);
        let hi_v = picked.iter().map(|n| n.vel).max().unwrap_or(0);
        let span = {
            let s = picked.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
            let e = picked
                .iter()
                .map(|n| n.start + n.len)
                .fold(f64::NEG_INFINITY, f64::max);
            if s.is_finite() { e - s } else { 0.0 }
        };
        format!(
            "{} notes  {}\u{2013}{}  vel {}\u{2013}{}  span {:.3}",
            picked.len(),
            note_name(lo_p),
            note_name(hi_p),
            lo_v,
            hi_v,
            span
        )
    } else {
        format!(
            "{:<4} {:>10}",
            note_name(pr.cursor_pitch),
            bbt(pr.cursor_beat, beats_per_bar)
        )
    };
    painter.text(
        rect.left_center() + egui::vec2(LABEL_PAD, 0.0),
        egui::Align2::LEFT_CENTER,
        left,
        font.clone(),
        theme.text_value,
    );

    // The modes, on the right. A mode you cannot see is a mode you will be
    // surprised by — which is the whole reason the tool strip and this
    // half of the line both exist.
    let mut right = if pr.chord_entry.is_some() {
        "CHORD ENTRY  ·  Enter write  ·  Esc cancel".to_owned()
    } else {
        format!(
            "{}  \u{00b7}  {}  \u{00b7}  {}",
            tool.label(),
            GRID_NAMES[pr.grid.min(GRID_NAMES.len() - 1)],
            key.label()
        )
    };
    if let Some(len) = pr.draw_len {
        right.push_str(&format!("  \u{00b7}  fixed {len:.3}"));
    }
    if pr.scale_lock {
        right.push_str("  \u{00b7}  scale");
    }
    if pr.fold {
        right.push_str("  \u{00b7}  fold");
    }
    painter.text(
        rect.right_center() - egui::vec2(LABEL_PAD, 0.0),
        egui::Align2::RIGHT_CENTER,
        right,
        font,
        theme.text_muted,
    );
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

    #[test]
    fn terse_chords_use_the_requested_octave_and_inversion() {
        let plan = parse_chord_entry("2 !cM9 i 2", C4, 3.0, 0.25, Key::default(), 4).unwrap();
        assert_eq!(plan.notes.len(), 5);
        assert_eq!(
            plan.notes[0].pitch, 43,
            "a second-inversion C chord has G2 in the bass"
        );
        assert!(plan.notes.iter().all(|note| note.start == 3.0));
        assert!(plan.notes.iter().all(|note| note.len == 0.25));
        assert_eq!(plan.advance, 0.25);

        let minor = parse_chord_entry("3 !cm9", C4, 0.0, 1.0, Key::default(), 4).unwrap();
        assert_eq!(
            minor
                .notes
                .iter()
                .map(|note| note.pitch)
                .collect::<Vec<_>>(),
            vec![48, 51, 55, 58, 62]
        );
    }

    #[test]
    fn chord_entry_places_dots_rests_and_the_next_chord_exactly() {
        let plan =
            parse_chord_entry("2 !cM9:e. r:q 3 !am9:q", C4, 4.0, 0.25, Key::default(), 4).unwrap();
        assert_eq!(plan.notes.len(), 10);
        assert!(
            plan.notes[..5]
                .iter()
                .all(|note| note.start == 4.0 && note.len == 0.75)
        );
        assert!(
            plan.notes[5..]
                .iter()
                .all(|note| note.start == 5.75 && note.len == 1.0)
        );
        assert_eq!(plan.advance, 2.75);
        assert_eq!(parse_chord_duration("1/8t", 1.0, 4).unwrap(), 1.0 / 3.0);
    }

    #[test]
    fn roman_progressions_follow_the_project_key_and_meter() {
        let key = Key {
            tonic: 2,
            scale: daw::theory::Scale::Major,
        };
        let plan = parse_chord_entry("3 !vi7:1bar !V7/ii:2beat", C4, 0.0, 0.25, key, 3).unwrap();
        assert_eq!(
            plan.notes[..4]
                .iter()
                .map(|note| note.pitch % 12)
                .collect::<Vec<_>>(),
            vec![11, 2, 6, 9],
            "vi7 in D major is B minor 7"
        );
        assert_eq!(plan.notes[0].len, 3.0);
        assert_eq!(plan.notes[4].start, 3.0);
        assert_eq!(plan.advance, 5.0);
    }

    // ------------------------------------------------------------ idioms ---

    fn idiom_pitches(source: &str) -> Vec<Vec<u8>> {
        let plan = parse_chord_entry(source, C4, 0.0, 0.25, Key::default(), 4)
            .unwrap_or_else(|why| panic!("`{source}`: {why}"));
        let mut starts: Vec<f64> = plan.notes.iter().map(|note| note.start).collect();
        starts.sort_by(f64::total_cmp);
        starts.dedup_by(|a, b| a.total_cmp(b).is_eq());
        starts
            .into_iter()
            .map(|start| {
                let mut chord: Vec<u8> = plan
                    .notes
                    .iter()
                    .filter(|note| note.start == start)
                    .map(|note| note.pitch)
                    .collect();
                chord.sort_unstable();
                chord
            })
            .collect()
    }

    /// The promise the whole design rests on: an idiom is a shorthand, not a
    /// second language. It expands into tokens the user could have typed.
    #[test]
    fn an_idiom_unfolds_into_the_chord_language() {
        assert_eq!(
            expand_idioms("@house f", C4).unwrap(),
            "4 !fm9no5:1bar 4 !em9no5:1bar cluster"
        );
        // No root: the cursor's pitch class, exactly like every other head.
        // Note the octave: a chromatic descent from C4 lands on B3, because
        // the step is arithmetic on pitch and not on a letter name.
        assert_eq!(
            expand_idioms("@m9", C4).unwrap(),
            "4 !cm9no5:1bar 3 !bm9no5:1bar cluster"
        );
        // A root may spell its own octave, and then it wins.
        assert_eq!(
            expand_idioms("@lift a2 --n 3", C4).unwrap(),
            "2 !am9no5:1bar 2 !a#m9no5:1bar 2 !bm9no5:1bar cluster"
        );
        // Idioms compose with hand-written events rather than replacing them.
        assert_eq!(
            expand_idioms("!cM9:q @house f", C4).unwrap(),
            "!cM9:q 4 !fm9no5:1bar 4 !em9no5:1bar cluster"
        );
    }

    /// The gesture itself: two minor ninths a semitone apart, fifth gone,
    /// voiced as stacks of seconds rather than stacks of thirds.
    #[test]
    fn the_house_vamp_is_two_clustered_ninths_a_semitone_apart() {
        let chords = idiom_pitches("@house f");
        assert_eq!(chords.len(), 2);
        // Eb F G Ab, then D E F# G — secundal, not tertian.
        assert_eq!(chords[0], vec![63, 65, 67, 68]);
        assert_eq!(chords[1], vec![62, 64, 66, 67]);
        for chord in &chords {
            let span = chord[chord.len() - 1] - chord[0];
            assert!(span <= 11, "{chord:?} is not a cluster");
            // The fifth is what the idiom omits; nothing sits 7 above the root.
            assert!(!chord.iter().any(|pitch| chord.contains(&(pitch + 7))));
        }
    }

    /// Every row of the catalogue and every alias builds something playable.
    /// A shipped name that does not parse is a broken promise on the help
    /// page, and the help page is where people learn this vocabulary.
    #[test]
    fn every_shipped_idiom_builds_a_playable_vamp() {
        let names = IDIOMS
            .iter()
            .map(|idiom| idiom.name)
            .chain(IDIOM_ALIASES.iter().map(|(alias, ..)| *alias));
        for name in names {
            let chords = idiom_pitches(&format!("@{name} f"));
            assert_eq!(chords.len(), 2, "@{name} should be a two-chord vamp");
            for chord in &chords {
                assert!(chord.len() >= 3, "@{name} built {chord:?}");
                let span = chord[chord.len() - 1] - chord[0];
                assert!(span <= 11, "@{name} built {chord:?}, which is no cluster");
            }
            // Chromatic: the two roots are a semitone apart as pitch-class sets.
            assert_ne!(chords[0], chords[1], "@{name} repeats one chord");
        }
    }

    #[test]
    fn idiom_modifiers_change_step_count_length_and_quality() {
        // Four chords descending by whole tones, a bar each.
        let wide = idiom_pitches("@m9 c --step -2 --n 4");
        assert_eq!(wide.len(), 4);

        // Alternating qualities, cycled over the chords.
        assert_eq!(
            expand_idioms("@m9 c --q m9no5,M9no5 --n 4", C4).unwrap(),
            "4 !cm9no5:1bar 3 !bM9no5:1bar 3 !a#m9no5:1bar 3 !aM9no5:1bar cluster"
        );

        // Register and length are the idiom's, not the cursor's, when asked.
        assert_eq!(
            expand_idioms("@house f --oct 2 --dur q", C4).unwrap(),
            "2 !fm9no5:q 2 !em9no5:q cluster"
        );

        // The help page's own example, kept honest: a page that teaches a
        // line this language cannot read is worse than no page.
        assert_eq!(idiom_pitches("@house f --dur 2beat --n 4").len(), 4);

        // Every interval, named the way a musician says it rather than
        // counted in semitones.
        assert_eq!(
            expand_idioms("@m9 c --step -m2", C4).unwrap(),
            expand_idioms("@m9 c --step -1", C4).unwrap()
        );
        assert_eq!(
            expand_idioms("@m9 c --step M2 --n 3", C4).unwrap(),
            "4 !cm9no5:1bar 4 !dm9no5:1bar 4 !em9no5:1bar cluster"
        );
        assert_eq!(
            expand_idioms("@m9 c --step -P4 --n 3", C4).unwrap(),
            "4 !cm9no5:1bar 3 !gm9no5:1bar 3 !dm9no5:1bar cluster"
        );
        // A step wider than an octave, and a name that is not an interval.
        assert!(
            expand_idioms("@house f --step 14", C4)
                .unwrap_err()
                .contains("octave")
        );
        assert!(
            expand_idioms("@house f --step M9", C4)
                .unwrap_err()
                .contains("not an interval")
        );

        // Two idioms in one entry must agree about it, since the transform
        // is entry-wide.
        assert!(
            expand_idioms("@house f @m9 c --nocluster", C4)
                .unwrap_err()
                .contains("whole entry")
        );
        assert!(
            expand_idioms("@house f --nocluster cluster", C4)
                .unwrap_err()
                .contains("nocluster")
        );

        // Opting out of the cluster leaves the chord's own spacing alone.
        let open = idiom_pitches("@house f --nocluster");
        assert_eq!(open[0], vec![65, 68, 75, 79]);
    }

    /// `cluster` is a voicing transform in its own right, not idiom-only
    /// machinery — it works on anything the chord language can spell.
    #[test]
    fn cluster_stands_alone_as_a_voicing_transform() {
        assert_eq!(idiom_pitches("4 !cm9no5 cluster")[0], vec![58, 60, 62, 63]);
        // A doubled root collapses: a cluster has no doublings.
        assert_eq!(idiom_pitches("4 !cmaj cluster")[0].len(), 3);
        // Composes with voicelead, and clustering wins the spacing argument.
        for chord in idiom_pitches("4 !am9no5:1bar 4 !dm9no5:1bar voicelead cluster") {
            assert!(chord[chord.len() - 1] - chord[0] <= 11);
        }
    }

    #[test]
    fn an_idiom_that_cannot_be_built_says_so_instead_of_guessing() {
        let why = expand_idioms("@techno f", C4).unwrap_err();
        assert!(
            why.contains("@m9"),
            "an unknown idiom should list the real ones: {why}"
        );

        assert!(
            expand_idioms("@house 9", C4)
                .unwrap_err()
                .contains("note name")
        );
        assert!(
            expand_idioms("@house f --n 99", C4)
                .unwrap_err()
                .contains("--n")
        );
        assert!(
            expand_idioms("@house f --step 40", C4)
                .unwrap_err()
                .contains("--step")
        );
        // Walking off the bottom of the keyboard refuses rather than wrapping
        // an octave up, which would be a different progression.
        assert!(
            expand_idioms("@house c-1 --n 8 --step -12", C4)
                .unwrap_err()
                .contains("keyboard")
        );
    }

    #[test]
    fn chord_options_control_octave_velocity_gate_and_voice_leading() {
        let plan = parse_chord_entry(
            "!am7 --oct 3 --dur 1bar --vel 111 --gate 75% !cmaj --oct 3 :q | voicelead",
            C4,
            0.0,
            0.25,
            Key::default(),
            4,
        )
        .unwrap();
        assert!(
            plan.notes[..4]
                .iter()
                .all(|note| note.vel == 111 && note.len == 3.0)
        );
        let first_top = plan.notes[..4].iter().map(|note| note.pitch).max().unwrap();
        let second_bass = plan.notes[4..].iter().map(|note| note.pitch).min().unwrap();
        assert!(
            i16::from(first_top).abs_diff(i16::from(second_bass)) <= 12,
            "voice leading should keep the next chord near the first"
        );
    }

    #[test]
    fn chord_symbols_stack_alterations_and_take_a_bass() {
        let pitches = |source: &str| {
            parse_chord_entry(source, C4, 0.0, 0.25, Key::default(), 4)
                .unwrap()
                .notes
                .iter()
                .map(|note| note.pitch)
                .collect::<Vec<_>>()
        };
        // A dominant seventh with a suspended fourth, not a chord in octave 7.
        assert_eq!(pitches("4 !c7sus4"), vec![60, 65, 67, 70]);
        // Alterations and omissions stack left to right.
        assert_eq!(pitches("4 !c13b9no5"), vec![60, 64, 70, 73, 77, 81]);
        // An absolute slash bass and the equivalent chord member agree.
        assert_eq!(pitches("4 !cM7/e3"), vec![52, 60, 64, 67, 71]);
        assert_eq!(pitches("4 !cM7/3"), vec![52, 60, 64, 67, 71]);
        // A Roman numeral keeps its slash bass through key resolution.
        assert_eq!(pitches("4 !IM7/3"), vec![52, 60, 64, 67, 71]);
    }

    #[test]
    fn music_script_parses_exact_relative_and_fit_clip_lengths() {
        let parse = |source| {
            parse_chord_entry(source, C4, 0.0, 0.25, Key::default(), 4)
                .unwrap()
                .clip_length
        };
        assert_eq!(parse("clip len 4bar"), Some(ClipLengthEdit::Set(16.0)));
        assert_eq!(parse("clip extend 1bar"), Some(ClipLengthEdit::Extend(4.0)));
        assert_eq!(parse("clip trim 2beat"), Some(ClipLengthEdit::Trim(2.0)));
        assert_eq!(parse("clip fit"), Some(ClipLengthEdit::Fit));
        assert_eq!(parse("clip 8bar"), Some(ClipLengthEdit::Set(32.0)));
    }

    #[test]
    fn impossible_chord_entry_refuses_without_panicking() {
        let error = parse_chord_entry("2 !cM9 i 9", C4, 0.0, 1.0, Key::default(), 4).unwrap_err();
        assert!(error.contains("outside this 5-note chord"), "{error}");
        assert!(parse_chord_entry("2 !wat", C4, 0.0, 1.0, Key::default(), 4).is_err());
        assert!(parse_chord_entry("r:q", C4, 0.0, 1.0, Key::default(), 4).is_err());
    }

    #[test]
    fn i_mode_commits_once_and_advances_the_cursor() {
        fn input(ctx: &egui::Context, events: Vec<egui::Event>) {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                    events,
                    ..Default::default()
                },
                |_ui| {},
            );
            out.textures_delta.clear();
        }

        let ctx = egui::Context::default();
        let mut pr = PianoRoll {
            owns_keys: true,
            ..PianoRoll::default()
        };
        let mut notes = Vec::new();
        input(
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::I,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        keys(&ctx, &mut pr, Some(&mut notes), &[], Key::default(), 4);
        assert!(pr.chord_entry.is_some());

        input(
            &ctx,
            vec![
                egui::Event::Text("3 !cm9".to_owned()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        keys(&ctx, &mut pr, Some(&mut notes), &[], Key::default(), 4);
        assert!(pr.chord_entry.is_none());
        assert_eq!(notes.len(), 5);
        assert_eq!(pr.selected, HashSet::from([0, 1, 2, 3, 4]));
        assert_eq!(pr.cursor_beat, pr.grid_beats());
    }

    #[test]
    fn a_pasted_phrase_lands_in_the_entry_line_whole() {
        let ctx = egui::Context::default();
        let mut pr = PianoRoll {
            owns_keys: true,
            chord_entry: Some(ChordEntry {
                text: "3 ".to_owned(),
                diagnostic: None,
            }),
            ..PianoRoll::default()
        };
        let mut notes = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                events: vec![egui::Event::Paste("!vi7:1bar\n!Imaj7:1bar".to_owned())],
                ..Default::default()
            },
            |_ui| {},
        );
        out.textures_delta.clear();

        keys(&ctx, &mut pr, Some(&mut notes), &[], Key::default(), 4);
        assert_eq!(
            pr.chord_entry.as_ref().map(|entry| entry.text.as_str()),
            Some("3 !vi7:1bar !Imaj7:1bar"),
            "the newline became a separator, not a weld"
        );
        assert!(
            pr.chord_entry
                .as_ref()
                .is_some_and(|entry| entry.diagnostic.is_none()),
            "and the pasted phrase parses"
        );
        assert!(notes.is_empty(), "a paste types, it does not commit");

        // The line is bounded: a paste cannot outgrow it.
        let long = "x".repeat(CHORD_ENTRY_MAX_CHARS * 2);
        if let Some(entry) = &mut pr.chord_entry {
            append_entry(entry, &long);
            assert_eq!(entry.text.chars().count(), CHORD_ENTRY_MAX_CHARS);
        }
    }

    #[test]
    fn chord_entry_consumes_printable_keys_instead_of_running_normal_bindings() {
        let ctx = egui::Context::default();
        let mut pr = PianoRoll {
            owns_keys: true,
            chord_entry: Some(ChordEntry::default()),
            ..PianoRoll::default()
        };
        let mut notes = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::Num2,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Key {
                        key: egui::Key::Space,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Key {
                        key: egui::Key::Colon,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::SHIFT,
                    },
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Text("2 !cM9:e. r:q".to_owned()),
                ],
                ..Default::default()
            },
            |_ui| {},
        );
        out.textures_delta.clear();

        assert!(pr.owns_modal_input());
        keys(&ctx, &mut pr, Some(&mut notes), &[], Key::default(), 4);
        assert_eq!(
            pr.chord_entry.as_ref().map(|entry| entry.text.as_str()),
            Some("2 !cM9:e. r:q")
        );
        assert!(notes.is_empty(), "a typed `a` must not add a normal note");
        assert_eq!(pr.tool, Tool::Pointer, "a typed `2` must not select Draw");
        assert!(
            !ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                    || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Colon)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::A)
            }),
            "modal characters leaked to a later shortcut reader"
        );
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
        let beat = Geom::new(grid, scroll, 0.0, old, Fold::ALL).beat_at(ax);

        for factor in [ZOOM_STEP, 1.0 / ZOOM_STEP, 2.5, 0.4] {
            let new = old.scaled(factor, 1.0);
            // The same arithmetic `zoom_input` uses.
            let new_scroll = (beat as f32 - (ax - grid.left()) / new.px_per_beat).max(0.0);
            let after = Geom::new(grid, new_scroll, 0.0, new, Fold::ALL).beat_at(ax);
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
        let beat = Geom::new(grid, 3.0, 0.0, old, Fold::ALL).beat_at(ax);
        let new = old.scaled(0.5, 1.0);
        let new_scroll = (beat as f32 - (ax - grid.left()) / new.px_per_beat).max(0.0);
        assert_eq!(new_scroll, 0.0, "never scroll before the first beat");
        assert!(Geom::new(grid, new_scroll, 0.0, new, Fold::ALL).beat_at(grid.left()) >= 0.0);
    }

    /// Pitch/pixel round trip, high notes on top.
    #[test]
    fn pitch_maps_upward_and_round_trips() {
        let z = Zoom::default();
        let grid = Rect::from_min_size(pos2(48.0, 100.0), vec2(800.0, 400.0));
        for scroll_y in [0.0, 123.0] {
            let g = Geom::new(grid, 0.0, scroll_y, z, Fold::ALL);
            for pitch in [0u8, 48, 60, 127] {
                let y = g.row_top(pitch);
                assert_eq!(g.pitch_at(y + z.row_h * 0.5), Some(pitch));
            }
            assert!(g.row_top(72) < g.row_top(60), "C5 must sit above C4");
        }
        // Off both ends of the range is nobody's row.
        let g = Geom::new(grid, 0.0, 0.0, z, Fold::ALL);
        assert_eq!(g.pitch_at(grid.top() - 10.0), None);
        assert_eq!(
            g.pitch_at(grid.top() + 129.0 * z.row_h),
            None,
            "below row 127 is off the keyboard"
        );

        // Beats round-trip too, from any pan.
        for sb in [0.0, 16.0] {
            let g = Geom::new(grid, sb, 0.0, z, Fold::ALL);
            for beat in [0.0, 1.0, 4.5] {
                let back = g.beat_at(g.x_at(beat));
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

    // ------------------------------------------- the clip's loop bar ---

    /// Drive the loop bar headlessly. Returns the clip after the gesture.
    fn loop_bar_gesture(clip: &mut Clip, path: &[daw::ui::device::probe::Step], px_per_beat: f32) {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, LOOP_BAR_H));
        let g = Geom::new(
            rect,
            0.0,
            0.0,
            Zoom {
                px_per_beat,
                row_h: 10.0,
            },
            Fold::ALL,
        );
        daw::ui::device::probe::run(&ctx, rect, path, |ui| {
            let Clip {
                len,
                loop_on,
                loop_start,
                loop_len,
                ..
            } = &mut *clip;
            let mut span = ClipSpan {
                len,
                loop_on,
                loop_start,
                loop_len,
            };
            loop_bar(ui, &Theme::dark(), rect, g, &mut span, 1.0);
        });
    }

    fn loopable(len: f32, loop_start: f32, loop_len: f32) -> Clip {
        Clip {
            id: 1,
            name: "c".into(),
            start: 0.0,
            len,
            notes: Vec::new(),
            audio: None,
            loop_on: true,
            loop_start,
            loop_len,
        }
    }

    /// The toggle switches the loop on, and switching it on with no brace
    /// set adopts the whole clip — so the first click loops what you are
    /// looking at rather than nothing.
    #[test]
    fn the_loop_toggle_adopts_the_whole_clip() {
        let mut clip = loopable(8.0, 0.0, 0.0);
        clip.loop_on = false;
        loop_bar_gesture(
            &mut clip,
            &daw::ui::device::probe::click_path(pos2(LOOP_TOGGLE_W * 0.5, LOOP_BAR_H * 0.5)),
            20.0,
        );
        assert!(clip.loop_on, "the toggle did not switch it on");
        assert_eq!(clip.loop_len, 8.0, "it did not adopt the clip");
    }

    /// The clip's END handle sets its length — which is the whole of how
    /// a launcher clip becomes a different number of bars from the one
    /// beside it.
    #[test]
    fn dragging_the_clip_end_sets_its_length() {
        let mut clip = loopable(8.0, 0.0, 8.0);
        // 20 px a beat: the end sits at x = 160, drag it to x = 80.
        loop_bar_gesture(
            &mut clip,
            &daw::ui::device::probe::drag_path(
                pos2(160.0, LOOP_BAR_H * 0.5),
                pos2(80.0, LOOP_BAR_H * 0.5),
                4,
            ),
            20.0,
        );
        assert_eq!(clip.len, 4.0, "the clip did not shorten");
        assert!(
            clip.loop_start + clip.loop_len <= clip.len + 1e-6,
            "the brace outlived its clip: {} + {} > {}",
            clip.loop_start,
            clip.loop_len,
            clip.len
        );
    }

    /// A brace END drag moves only the end. The start stays put, which is
    /// what an edge means — and a drag that began on the end must not be
    /// stolen by the start when the two cross.
    #[test]
    fn dragging_a_brace_edge_moves_only_that_edge() {
        let mut clip = loopable(8.0, 2.0, 4.0);
        // The end sits at beat 6 -> x = 120. Pull it back to beat 3.
        loop_bar_gesture(
            &mut clip,
            &daw::ui::device::probe::drag_path(
                pos2(120.0, LOOP_BAR_H * 0.5),
                pos2(60.0, LOOP_BAR_H * 0.5),
                4,
            ),
            20.0,
        );
        assert_eq!(clip.loop_start, 2.0, "the start moved");
        assert_eq!(clip.loop_len, 1.0, "the end did not land on beat 3");
    }

    /// And an edge dragged PAST its partner stops rather than inverting.
    /// A brace of negative length is not a loop, and a brace of zero
    /// length is a modulus by zero one layer down.
    #[test]
    fn a_brace_cannot_be_turned_inside_out() {
        let mut clip = loopable(8.0, 2.0, 4.0);
        loop_bar_gesture(
            &mut clip,
            &daw::ui::device::probe::drag_path(
                pos2(120.0, LOOP_BAR_H * 0.5),
                pos2(0.0, LOOP_BAR_H * 0.5),
                6,
            ),
            20.0,
        );
        assert!(clip.loop_len >= crate::MIN_LOOP_BEATS, "{}", clip.loop_len);
        assert_eq!(clip.loop_start, 2.0);
    }

    /// A press on empty bar moves nothing. The bar is mostly empty, and a
    /// stray click there must not resize the clip you were looking at.
    #[test]
    fn a_press_on_empty_bar_changes_nothing() {
        let mut clip = loopable(8.0, 2.0, 4.0);
        let before = clip.clone();
        loop_bar_gesture(
            &mut clip,
            &daw::ui::device::probe::drag_path(
                pos2(400.0, LOOP_BAR_H * 0.5),
                pos2(430.0, LOOP_BAR_H * 0.5),
                3,
            ),
            20.0,
        );
        assert_eq!(clip.len, before.len);
        assert_eq!(clip.loop_start, before.loop_start);
        assert_eq!(clip.loop_len, before.loop_len);
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

        keys(&ctx, &mut pr, None, &[], Key::default(), 4);
        assert_eq!(pr.cursor_beat, 0.0, "an inert editor must not move");
        assert!(
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)),
            "the key must still be there for whoever navigates next"
        );
    }

    /// Every region tiles the panel with no gap and no overlap, and the
    /// gutter, the grid and the lane all share one horizontal origin.
    #[test]
    fn the_layout_tiles_the_panel() {
        let area = Rect::from_min_size(pos2(0.0, 500.0), vec2(1200.0, 360.0));
        let lanes = [LaneView {
            lane: Lane::Velocity,
            h: LANE_H,
            collapsed: false,
        }];
        let l = layout(area, &lanes);

        assert_eq!(l.tools.width(), TOOLS_W);
        assert_eq!(l.keys.width(), KEYS_W);
        assert_eq!(l.tools.right(), l.keys.left());
        assert_eq!(l.keys.right(), l.grid.left());
        assert_eq!(l.grid.right(), area.right());

        // Vertically: ruler, then grid, then the lane, then the info line.
        assert_eq!(l.ruler.height(), RULER_H);
        assert_eq!(l.ruler.bottom(), l.grid.top());
        assert_eq!(l.corner.bottom(), l.keys.top());
        assert_eq!(l.lanes.len(), 1);
        let (_, lane_gutter, lane_body) = l.lanes[0];
        assert_eq!(l.grid.bottom(), lane_body.top());
        assert_eq!(lane_body.height(), LANE_H);
        assert_eq!(lane_gutter.right(), lane_body.left());
        assert_eq!(lane_gutter.left(), area.left());
        assert_eq!(lane_body.left(), l.grid.left());
        assert_eq!(lane_body.bottom(), l.info.top());
        assert_eq!(l.info.bottom(), area.bottom());
        assert_eq!(l.info.height(), INFO_H);
    }

    /// The panel shrinks in a stated order, and the GRID is never the
    /// thing that goes first — the grid is what the panel is for.
    #[test]
    fn the_layout_degrades_in_order() {
        let lanes = [
            LaneView {
                lane: Lane::Velocity,
                h: LANE_H,
                collapsed: false,
            },
            LaneView {
                lane: Lane::Probability,
                h: LANE_H,
                collapsed: false,
            },
        ];
        let at = |h: f32| layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, h)), &lanes);

        // Roomy: everything present, lanes at full height.
        let big = at(500.0);
        assert!(big.info.height() > 0.0);
        assert!(big.ruler.height() > 0.0);
        assert_eq!(big.lanes.len(), 2);
        assert_eq!(big.lanes[0].2.height(), LANE_H);

        // Squeezed: the lanes give up their height before anything else.
        let mid = at(230.0);
        assert!(mid.info.height() > 0.0);
        assert!(mid.ruler.height() > 0.0);
        assert_eq!(mid.lanes[0].2.height(), LANE_HEADER_H);

        // Short: the info line goes.
        let short = at(130.0);
        assert_eq!(short.info.height(), 0.0);
        assert!(short.ruler.height() > 0.0);

        // Shorter: the ruler goes too, and the grid still has a body.
        let tiny = at(100.0);
        assert_eq!(tiny.ruler.height(), 0.0);
        assert!(tiny.grid.height() > 0.0);

        // At every size the grid keeps the lion's share of the height.
        for h in [500.0, 300.0, 230.0, 160.0, 130.0, 100.0] {
            let l = at(h);
            let lanes_h: f32 = l.lanes.iter().map(|(_, _, b)| b.height()).sum();
            assert!(
                l.grid.height() >= lanes_h,
                "at {h}pt the lanes ({lanes_h}) outgrew the grid ({})",
                l.grid.height()
            );
        }
    }
    // ------------------------------------------ the keyboard verbs ---

    /// Quantize: starts snap to the roll's grid; lengths only when asked,
    /// and never below one step.
    #[test]
    fn quantize_snaps_starts_and_optionally_lengths() {
        let (mut pr, mut notes) =
            roll_with(vec![note(60, 1.13, 0.9, 100), note(64, 2.6, 0.2, 100)]);
        pr.grid = GRID_DEFAULT; // 1 beat
        let grid = pr.grid_beats();
        pr.selected = (0..notes.len()).collect();
        pr.quantize_selected(&mut notes, false);
        assert_eq!(notes[0].start, 1.0);
        assert_eq!(notes[1].start, 3.0);
        assert_eq!(notes[0].len, 0.9, "lengths untouched without the flag");
        pr.quantize_selected(&mut notes, true);
        assert_eq!(notes[0].len, grid);
        assert_eq!(notes[1].len, grid, "a sliver quantizes UP to one step");
    }

    /// Split at the cursor: one gesture, two notes, both selected — and a
    /// note the cursor does not pass through is left alone.
    #[test]
    fn split_cuts_spanning_notes_at_the_cursor() {
        let (mut pr, mut notes) = roll_with(vec![
            note(60, 0.0, 2.0, 90),
            note(64, 3.0, 1.0, 90), // beyond the cursor: untouched
        ]);
        pr.selected = (0..notes.len()).collect();
        pr.cursor_beat = 1.5;
        pr.split_selected_at_cursor(&mut notes);
        assert_eq!(notes.len(), 3);
        assert_eq!((notes[0].start, notes[0].len), (0.0, 1.5));
        assert_eq!((notes[2].start, notes[2].len), (1.5, 0.5));
        assert_eq!(notes[2].vel, 90, "the tail keeps the head's velocity");
        assert_eq!((notes[1].start, notes[1].len), (3.0, 1.0));
        assert!(pr.selected.contains(&2), "both halves stay selected");
    }

    /// Join: per pitch, first start to last end, gaps included — the
    /// inverse of the split, and Ableton's rule.
    #[test]
    fn join_merges_selected_notes_per_pitch() {
        let (mut pr, mut notes) = roll_with(vec![
            note(60, 0.0, 1.0, 100),
            note(60, 2.0, 1.0, 80),
            note(64, 0.5, 0.5, 100), // different pitch: its own island
        ]);
        pr.selected = (0..notes.len()).collect();
        pr.join_selected(&mut notes);
        assert_eq!(notes.len(), 2);
        let joined = notes.iter().find(|n| n.pitch == 60).unwrap();
        assert_eq!((joined.start, joined.len), (0.0, 3.0));
        assert!(notes.iter().any(|n| n.pitch == 64));
        // And a split undoes it: cursor at the seam, split, two notes.
        pr.selected = notes
            .iter()
            .position(|n| n.pitch == 60)
            .into_iter()
            .collect();
        pr.cursor_beat = 1.0;
        pr.split_selected_at_cursor(&mut notes);
        assert_eq!(notes.iter().filter(|n| n.pitch == 60).count(), 2);
    }

    /// Ableton's 0: a mixed selection mutes as one, a muted one unmutes.
    #[test]
    fn zero_toggles_mute_as_one_gesture() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100), note(64, 1.0, 1.0, 100)]);
        notes[1].muted = true;
        pr.selected = (0..notes.len()).collect();
        pr.toggle_mute_selected(&mut notes);
        assert!(notes.iter().all(|n| n.muted), "mixed -> all muted");
        pr.toggle_mute_selected(&mut notes);
        assert!(notes.iter().all(|n| !n.muted), "all muted -> all live");
    }

    /// Enter is smart: empty cell adds, occupied cell toggles selection —
    /// and never stacks a duplicate on an existing note.
    #[test]
    fn enter_adds_on_empty_and_selects_on_a_note() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        pr.cursor_pitch = 60;
        pr.cursor_beat = 0.0;
        pr.enter_at_cursor(&mut notes);
        assert_eq!(notes.len(), 1, "Enter on a note must not stack another");
        assert!(pr.selected.contains(&0), "it selects it instead");
        pr.enter_at_cursor(&mut notes);
        assert!(pr.selected.is_empty(), "and toggles it off again");
        pr.cursor_beat = 2.0;
        pr.enter_at_cursor(&mut notes);
        assert_eq!(notes.len(), 2, "an empty cell still adds");
    }

    /// N walks the material note by note, selecting each stop; Shift+N
    /// walks back.
    #[test]
    fn jumping_lands_on_notes_and_selects_them() {
        let (mut pr, notes) = roll_with(vec![
            note(60, 0.0, 1.0, 100),
            note(64, 2.0, 1.0, 100),
            note(62, 4.0, 1.0, 100),
        ]);
        pr.cursor_beat = 0.5;
        pr.jump_to_note(&notes, true);
        assert_eq!((pr.cursor_beat, pr.cursor_pitch), (2.0, 64));
        assert_eq!(pr.selected, [1].into_iter().collect());
        pr.jump_to_note(&notes, true);
        assert_eq!((pr.cursor_beat, pr.cursor_pitch), (4.0, 62));
        pr.jump_to_note(&notes, false);
        assert_eq!((pr.cursor_beat, pr.cursor_pitch), (2.0, 64));
        // At the far end, a further jump stays put rather than wrapping.
        pr.cursor_beat = 4.0;
        pr.cursor_pitch = 62;
        pr.jump_to_note(&notes, true);
        assert_eq!(pr.cursor_beat, 4.0);
    }

    /// End lands on the material's end, snapped to the grid; Home needs
    /// no test beyond its binding, it is an assignment.
    #[test]
    fn end_snaps_to_the_materials_end() {
        let (mut pr, notes) = roll_with(vec![note(60, 0.0, 2.3, 100)]);
        pr.grid = GRID_DEFAULT;
        pr.cursor_to_end(&notes);
        assert_eq!(pr.cursor_beat, 2.0, "2.3 snaps to the nearest rung");
        let (mut pr, empty) = roll_with(vec![]);
        pr.cursor_to_end(&empty);
        assert_eq!(pr.cursor_beat, 0.0);
    }

    /// The keyboard's grab: moving the selection carries the cursor with
    /// it, both clamped alike — so the cursor is still ON the note after
    /// the move, and the next [ ] , . or further move still holds it.
    #[test]
    fn moving_the_selection_carries_the_cursor() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 1.0, 1.0, 100)]);
        pr.grid = GRID_DEFAULT;
        pr.cursor_pitch = 60;
        pr.cursor_beat = 1.0;
        pr.selected.insert(0);

        pr.move_selected(&mut notes, 1, 0.0);
        assert_eq!((notes[0].pitch, pr.cursor_pitch), (61, 61));
        pr.move_selected(&mut notes, 0, 1.0);
        assert_eq!(notes[0].start, 2.0);
        assert_eq!(pr.cursor_beat, 2.0);
        assert!(
            pr.note_at_cursor(&notes).is_some(),
            "after the move the cursor no longer holds the note"
        );

        // Against the wall: both clamp, neither drifts past the other.
        pr.move_selected(&mut notes, 0, -100.0);
        assert_eq!(notes[0].start, 0.0);
        assert_eq!(pr.cursor_beat, 0.0);

        // With nothing selected, a move is a no-op — the cursor's own
        // travel is the plain arrows' job.
        pr.selected.clear();
        pr.cursor_beat = 5.0;
        pr.move_selected(&mut notes, 3, 2.0);
        assert_eq!(notes[0].pitch, 61, "no selection, no edit");
        assert_eq!(pr.cursor_beat, 5.0, "no selection, no cursor jump");
    }

    // ------------------------------------------ parameter locks ---

    fn plist() -> Vec<PlockParam> {
        let face = || -> std::sync::Arc<dyn Fn(f32) -> String + Send + Sync> {
            std::sync::Arc::new(|v: f32| format!("{v:.2}"))
        };
        vec![
            PlockParam {
                id: 17,
                name: "Filter Cutoff".into(),
                min: 20.0,
                max: 20_000.0,
                base: 1_000.0,
                choices: 0,
                format: face(),
            },
            PlockParam {
                id: 21,
                name: "Filter Drive".into(),
                min: 0.0,
                max: 100.0,
                base: 0.0,
                choices: 0,
                format: face(),
            },
            PlockParam {
                id: 0,
                name: "Osc A Wave".into(),
                min: 0.0,
                max: 7.0,
                base: 0.0,
                choices: 8,
                format: face(),
            },
        ]
    }

    /// Shift+Enter's whole lifecycle: opens on the held note, refuses
    /// thin air, closes on a second press — and a deleted note closes it
    /// rather than letting the editor point at a successor.
    #[test]
    fn the_plock_editor_opens_on_a_note_and_survives_deletion() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        pr.cursor_pitch = 72; // empty cell: nothing to hold
        pr.plock_toggle_view(&notes);
        assert_eq!(pr.plock_view, None, "opened over thin air");

        pr.cursor_pitch = 60;
        pr.cursor_beat = 0.0;
        pr.plock_toggle_view(&notes);
        assert_eq!(pr.plock_view, Some((0, 0)));
        pr.plock_toggle_view(&notes);
        assert_eq!(pr.plock_view, None, "second press must close");

        pr.plock_toggle_view(&notes);
        notes.clear();
        assert_eq!(pr.plock_at(&notes), None, "a deleted note kept the view");
        assert_eq!(pr.plock_view, None);
    }

    /// The editing verbs: toggle locks at the KNOB's value, adjust
    /// creates-then-moves, delete removes — and every value stays inside
    /// the parameter's range.
    #[test]
    fn plock_verbs_edit_the_note() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        let params = plist();
        pr.cursor_pitch = 60;
        pr.plock_toggle_view(&notes);

        // Enter: lock at base.
        pr.plock_toggle_row(&mut notes, &params);
        assert_eq!(notes[0].plocks, vec![(17, 1_000.0)]);
        // Enter again: unlock.
        pr.plock_toggle_row(&mut notes, &params);
        assert!(notes[0].plocks.is_empty());

        // Adjust on an UNLOCKED row: the lock begins at the base and
        // moves — turning the value is how a lock starts.
        pr.plock_adjust(&mut notes, &params, 0.01);
        let v = notes[0].plocks[0].1;
        assert!((v - (1_000.0 + 0.01 * 19_980.0)).abs() < 0.5);

        // Clamped at the rails.
        pr.plock_adjust(&mut notes, &params, 10.0);
        assert_eq!(notes[0].plocks[0].1, 20_000.0);

        // A second row locks independently.
        pr.plock_nav(1, params.len());
        pr.plock_adjust(&mut notes, &params, 0.5);
        assert_eq!(notes[0].plocks.len(), 2);
        assert_eq!(notes[0].plocks[1].0, 21);

        // Delete removes only the held row's lock.
        pr.plock_remove(&mut notes, &params);
        assert_eq!(notes[0].plocks, vec![(17, 20_000.0)]);
    }

    /// Drive the REAL key path for the lock editor, both directions.
    ///
    /// The verbs have their own test and pass it; this is the layer
    /// between them and the keyboard, which is where a binding that never
    /// fires would hide.
    #[test]
    fn the_lock_editors_arrows_move_the_value_both_ways() {
        fn press(ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |_ui| {},
            );
            out.textures_delta.clear();
        }

        let ctx = egui::Context::default();
        let params = plist();
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        pr.owns_keys = true;
        pr.cursor_pitch = 60;
        pr.cursor_beat = 0.0;
        pr.plock_toggle_view(&notes);
        assert!(pr.plock_view.is_some(), "the editor did not open");

        // RIGHT: up from the knob's value.
        press(&ctx, egui::Key::ArrowRight, egui::Modifiers::NONE);
        keys(&ctx, &mut pr, Some(&mut notes), &params, Key::default(), 4);
        let up = notes[0].plocks[0].1;
        assert!(up > 1_000.0, "right did not raise the value: {up}");

        // LEFT: back down again. The same distance, so it lands where it
        // started — which is the property that says the two directions
        // are the same size and not merely both non-zero.
        press(&ctx, egui::Key::ArrowLeft, egui::Modifiers::NONE);
        keys(&ctx, &mut pr, Some(&mut notes), &params, Key::default(), 4);
        let back = notes[0].plocks[0].1;
        assert!(
            (back - 1_000.0).abs() < 0.5,
            "left did not lower the value: {up} -> {back}"
        );

        // And below the knob's value, which is where "down does nothing"
        // would show up: the first press down from an unlocked row has to
        // both create the lock AND move it.
        press(&ctx, egui::Key::ArrowLeft, egui::Modifiers::NONE);
        keys(&ctx, &mut pr, Some(&mut notes), &params, Key::default(), 4);
        let below = notes[0].plocks[0].1;
        assert!(below < 1_000.0, "left could not go below the base: {below}");
    }

    /// A DISCRETE row steps whole choices: one nudge, the next wave —
    /// never a fraction of the range, never past either end.
    #[test]
    fn discrete_plock_rows_step_one_choice_per_nudge() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        let params = plist();
        pr.cursor_pitch = 60;
        pr.plock_toggle_view(&notes);
        pr.plock_nav(2, params.len()); // the wave row

        pr.plock_adjust(&mut notes, &params, 0.01);
        assert_eq!(notes[0].plocks, vec![(0, 1.0)], "one nudge, one wave");
        pr.plock_adjust(&mut notes, &params, 1.0);
        assert_eq!(notes[0].plocks[0].1, 2.0);
        pr.plock_adjust(&mut notes, &params, -0.5);
        assert_eq!(notes[0].plocks[0].1, 1.0);
        for _ in 0..20 {
            pr.plock_adjust(&mut notes, &params, 1.0);
        }
        assert_eq!(notes[0].plocks[0].1, 7.0, "clamped at the last choice");
    }

    /// A discrete row steps ONE WHOLE CHOICE down, including onto its own
    /// first choice — the case that was broken. With `choices` reported
    /// as 0 the row was swept as a continuous one, so a nudge from the
    /// bottom of a three-way switch moved it by a hundredth of a choice,
    /// rounded back to where it was, and looked like a dead key.
    #[test]
    fn a_switch_steps_down_onto_its_first_choice() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        let params = plist();
        pr.cursor_pitch = 60;
        pr.plock_toggle_view(&notes);
        pr.plock_nav(2, params.len()); // the wave row: discrete, 0..7

        // Start one choice up, then step down onto the floor.
        pr.plock_adjust(&mut notes, &params, 1.0);
        assert_eq!(notes[0].plocks[0].1, 1.0);
        pr.plock_adjust(&mut notes, &params, -1.0);
        assert_eq!(notes[0].plocks[0].1, 0.0, "one step down, one choice");

        // And at the floor it stays put rather than going negative.
        pr.plock_adjust(&mut notes, &params, -1.0);
        assert_eq!(notes[0].plocks[0].1, 0.0);
    }

    /// Home and End go straight to the rails, on both kinds of row.
    #[test]
    fn home_and_end_reach_the_ends() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        let params = plist();
        pr.cursor_pitch = 60;
        pr.plock_toggle_view(&notes);

        // Continuous.
        pr.plock_adjust(&mut notes, &params, f32::INFINITY);
        assert_eq!(notes[0].plocks[0].1, params[0].max);
        pr.plock_adjust(&mut notes, &params, -f32::INFINITY);
        assert_eq!(notes[0].plocks[0].1, params[0].min);

        // Discrete: the whole ladder in one press, not one rung.
        pr.plock_nav(2, params.len());
        pr.plock_adjust(&mut notes, &params, f32::INFINITY);
        let wave = notes[0]
            .plocks
            .iter()
            .find(|(id, _)| *id == 0)
            .map(|(_, v)| *v);
        assert_eq!(wave, Some(params[2].max));
        pr.plock_adjust(&mut notes, &params, -f32::INFINITY);
        let wave = notes[0]
            .plocks
            .iter()
            .find(|(id, _)| *id == 0)
            .map(|(_, v)| *v);
        assert_eq!(wave, Some(params[2].min));
    }

    /// A zero-width row cannot be nudged into a NaN. `INFINITY * 0` is
    /// NaN, and a NaN reaching a lock would be a value the engine has to
    /// bin on every note that carries it.
    #[test]
    fn a_zero_width_row_stays_a_number() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        let params = vec![PlockParam {
            id: 3,
            name: "pinned".to_owned(),
            min: 0.5,
            max: 0.5,
            base: 0.5,
            choices: 0,
            format: std::sync::Arc::new(|v| format!("{v}")),
        }];
        pr.cursor_pitch = 60;
        pr.plock_toggle_view(&notes);
        for nudge in [f32::INFINITY, -f32::INFINITY, 1.0, -1.0] {
            pr.plock_adjust(&mut notes, &params, nudge);
            assert!(
                notes[0].plocks[0].1.is_finite(),
                "a {nudge} nudge produced {}",
                notes[0].plocks[0].1
            );
        }
        assert_eq!(notes[0].plocks[0].1, 0.5);
    }

    /// The trig editor's verbs: the ladder steps one rung per nudge in
    /// either direction, probability sweeps and clamps, Delete resets,
    /// and the two floating editors are exclusive.
    #[test]
    fn trig_verbs_edit_the_note() {
        let (mut pr, mut notes) = roll_with(vec![note(60, 0.0, 1.0, 100)]);
        pr.cursor_pitch = 60;

        // Exclusive with the lock editor.
        pr.plock_toggle_view(&notes);
        assert!(pr.plock_view.is_some());
        pr.trig_toggle_view(&notes);
        assert!(pr.trig_view.is_some() && pr.plock_view.is_none());

        // Probability row sweeps and clamps.
        pr.trig_adjust(&mut notes, -0.25);
        assert!((notes[0].prob - 0.75).abs() < 1e-6);
        pr.trig_adjust(&mut notes, -10.0);
        assert_eq!(notes[0].prob, 0.0);
        pr.trig_clear(&mut notes);
        assert_eq!(notes[0].prob, 1.0);

        // Condition row: one rung per nudge, whole, both directions.
        pr.trig_view = Some((0, 1));
        pr.trig_adjust(&mut notes, 0.01);
        assert_eq!(notes[0].cond, Some((1, 2)), "first rung of the ladder");
        pr.trig_adjust(&mut notes, 1.0);
        assert_eq!(notes[0].cond, Some((2, 2)));
        pr.trig_adjust(&mut notes, -0.5);
        assert_eq!(notes[0].cond, Some((1, 2)));
        pr.trig_adjust(&mut notes, -1.0);
        assert_eq!(notes[0].cond, None, "below the ladder is a plain note");
        for _ in 0..99 {
            pr.trig_adjust(&mut notes, 1.0);
        }
        assert_eq!(notes[0].cond, Some((8, 8)), "clamped at the top rung");
        pr.trig_clear(&mut notes);
        assert_eq!(notes[0].cond, None);
    }
}

/// Pointer tests.
///
/// The device UI contract is blunt about why these exist: the five
/// standing card tests are all about parameters, and every interaction
/// bug this codebase has shipped lived one layer below them, where only a
/// pointer finds it. The roll's gestures are rebuilt around a selection
/// rather than an index, so every one of them gets driven for real.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod pointer {
    use super::*;
    use crate::note;
    use daw::ui::device::probe;
    use egui::{Modifiers, Rect, pos2, vec2};

    /// The panel under test. Wide and tall enough that the layout is the
    /// roomy one: ruler, grid, one lane, info line.
    const PANEL: Rect = Rect {
        min: egui::Pos2 { x: 0.0, y: 0.0 },
        max: egui::Pos2 {
            x: 1200.0,
            y: 400.0,
        },
    };

    /// A roll with its view pinned, so a test can compute where a note is
    /// drawn instead of guessing. `centered` is spent up front for the
    /// same reason: the first-frame centring must not happen halfway
    /// through a gesture.
    fn scene(notes: Vec<Note>) -> (PianoRoll, Clip) {
        let mut pr = PianoRoll {
            centered: true,
            scroll_y: 0.0,
            scroll_beats: 0.0,
            ..PianoRoll::default()
        };
        // Show the octave around middle C: row 0 is pitch 127, so scroll
        // to put C5 (72) at the top of the grid.
        pr.scroll_y = f32::from(PITCH_MAX - 72) * pr.zoom.row_h;
        let clip = Clip {
            id: 1,
            name: "scene".to_owned(),
            start: 0.0,
            len: 16.0,
            notes,
            audio: None,
            ..Clip::default()
        };
        (pr, clip)
    }

    /// Where the grid sits inside `PANEL`, and the map onto it. Derived
    /// from `layout`, not written down twice — a test that hardcodes a
    /// rect stops testing the layout the moment the layout changes.
    fn geom(pr: &PianoRoll) -> Geom {
        let lay = layout(PANEL, &pr.lanes);
        Geom::new(lay.grid, pr.scroll_beats, pr.scroll_y, pr.zoom, Fold::ALL)
    }

    /// Drive a real pointer over a real editor.
    fn drive(pr: &mut PianoRoll, clip: &mut Clip, path: &[probe::Step]) {
        drive_with_ghosts(pr, clip, &[], path);
    }

    /// The same, with reference material behind the clip's own notes.
    fn drive_with_ghosts(
        pr: &mut PianoRoll,
        clip: &mut Clip,
        ghosts: &[Note],
        path: &[probe::Step],
    ) {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut focus = Focus::default();
        probe::run(&ctx, PANEL, path, |ui| {
            body_at(
                ui,
                &mut focus,
                &theme,
                pr,
                TimeView::default(),
                Some(clip),
                Key::default(),
                &[],
                ghosts,
            );
        });
    }

    /// The centre of a note's body, and a point inside each edge zone.
    fn body_of(g: Geom, n: &Note) -> egui::Pos2 {
        g.note_rect(n).center()
    }
    fn right_edge_of(g: Geom, n: &Note) -> egui::Pos2 {
        let r = g.note_rect(n);
        pos2(r.right() - 2.0, r.center().y)
    }
    fn left_edge_of(g: Geom, n: &Note) -> egui::Pos2 {
        let r = g.note_rect(n);
        pos2(r.left() + 2.0, r.center().y)
    }

    /// The keyboard has to follow the hand. `keys` claims its input from
    /// the focus ring, so a roll being worked in with the mouse while the
    /// ring sat elsewhere answered no keystroke at all — press `i`, nothing
    /// happens.
    #[test]
    fn clicking_the_grid_brings_the_keyboard_cursor_with_it() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 1.0, 100)]);
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut focus = Focus::default();
        let mut frame = |pr: &mut PianoRoll, clip: &mut Clip, path: &[probe::Step]| {
            probe::run(&ctx, PANEL, path, |ui| {
                body_at(
                    ui,
                    &mut focus,
                    &theme,
                    pr,
                    TimeView::default(),
                    Some(clip),
                    Key::default(),
                    &[],
                    &[],
                );
            });
        };

        let at = layout(PANEL, &pr.lanes).grid.center();
        frame(&mut pr, &mut clip, &[probe::Step::moved(at)]);
        assert!(
            !pr.owns_keys,
            "hovering is not working in it — the ring stays where it was"
        );

        frame(&mut pr, &mut clip, &probe::click_path(at));
        assert!(
            pr.owns_keys,
            "a click in the grid is the roll becoming the thing you are typing at"
        );
    }

    // --- the bug this whole rewrite is about ---------------------------

    /// A DRAG MOVES THE WHOLE SELECTION. The old `Drag::Move` carried one
    /// index, so selecting eight notes and dragging one left seven
    /// behind — which is not what any DAW does and not what anyone means.
    #[test]
    fn a_drag_moves_the_whole_selection() {
        let (mut pr, mut clip) = scene(vec![
            note(60, 0.0, 1.0, 100),
            note(64, 0.0, 1.0, 100),
            note(67, 0.0, 1.0, 100),
        ]);
        pr.selected = (0..3).collect();
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // Two beats right and two rows up.
        let to = from + vec2(2.0 * g.px_per_beat(), -2.0 * g.row_h());
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 6));

        for (n, was) in clip.notes.iter().zip([60u8, 64, 67]) {
            assert_eq!(n.pitch, was + 2, "every note transposes together");
            assert!((n.start - 2.0).abs() < 1e-6, "every note moves in time");
        }
        assert_eq!(pr.selected.len(), 3, "the selection survives the drag");
    }

    /// A BLOCK CLAMPS AS A BLOCK. Clamping note by note shears the chord:
    /// the low notes stop at the floor while the high ones keep going, and
    /// the shape being dragged does not survive.
    #[test]
    fn a_block_drag_clamps_as_a_block() {
        let (mut pr, mut clip) = scene(vec![note(60, 4.0, 1.0, 100), note(72, 4.0, 1.0, 100)]);
        pr.selected = (0..2).collect();
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // Far enough left to take the first note well before beat 0.
        let to = from - vec2(20.0 * g.px_per_beat(), 0.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 8));

        assert!((clip.notes[0].start - 0.0).abs() < 1e-6, "pinned at zero");
        assert!(
            (clip.notes[1].start - 0.0).abs() < 1e-6,
            "and its partner pinned with it, not four beats later"
        );
        assert_eq!(
            clip.notes[1].start - clip.notes[0].start,
            0.0,
            "the interval inside the block is unchanged"
        );
    }

    /// A DRAG THAT CROSSES A NEIGHBOUR KEEPS ITS OWN TARGET. This is the
    /// device UI contract's first failure mode, and the reason the target
    /// is captured at the press and never looked up again.
    #[test]
    fn crossing_a_neighbour_keeps_the_target() {
        let (mut pr, mut clip) = scene(vec![
            note(60, 0.0, 1.0, 100),
            note(60, 4.0, 1.0, 100),
            note(60, 8.0, 1.0, 100),
        ]);
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // Straight through both neighbours and out the other side.
        let to = from + vec2(9.0 * g.px_per_beat(), 0.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 20));

        assert!(
            (clip.notes[0].start - 9.0).abs() < 1e-6,
            "the note that was pressed is the note that moved, to {}",
            clip.notes[0].start
        );
        assert_eq!(clip.notes[1].start, 4.0, "the neighbour did not move");
        assert_eq!(clip.notes[2].start, 8.0, "nor the one after it");
    }

    /// A DRAG THAT PINS AT A LIMIT SURVIVES THE RETURN. The old failure
    /// was a handle that stopped following, the pointer running away from
    /// it, and the drag dying halfway. Recomputing from the press rather
    /// than accumulating deltas is what fixes it, and this proves it.
    #[test]
    fn pinning_at_a_limit_survives_the_return() {
        let (mut pr, mut clip) = scene(vec![note(60, 2.0, 1.0, 100)]);
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        let far_left = from - vec2(30.0 * g.px_per_beat(), 0.0);
        let back = from + vec2(3.0 * g.px_per_beat(), 0.0);

        // Out past the floor, hold there, then all the way back.
        let mut path = probe::drag_path(from, far_left, 6);
        path.pop(); // drop the release; keep dragging
        path.extend(
            probe::drag_path(far_left, back, 6).into_iter().skip(2), // its own hover+press frames are not wanted
        );
        drive(&mut pr, &mut clip, &path);

        assert!(
            (clip.notes[0].start - 5.0).abs() < 1e-6,
            "came back to where the pointer is, not to where the deltas \
             left it: {}",
            clip.notes[0].start
        );
    }

    /// A PRESS ON A NOTE NEVER STARTS A MARQUEE. Two gestures over one
    /// rectangle is the contract's second failure mode.
    #[test]
    fn a_press_on_a_note_never_starts_a_marquee() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 4.0, 100), note(67, 0.0, 4.0, 100)]);
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // A gesture that, as a marquee, would sweep up both notes.
        let to = pos2(from.x + 10.0, g.note_rect(&clip.notes[1]).center().y);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 6));

        assert_eq!(
            pr.selected.len(),
            1,
            "a press on a note moves that note; it does not select a region"
        );
        assert!(pr.selected.contains(&0));
    }

    /// A PRESS ON EMPTY GROUND MOVES NOTHING. The other half of the same
    /// rule.
    #[test]
    fn a_press_on_empty_ground_moves_nothing() {
        let (mut pr, mut clip) = scene(vec![note(60, 8.0, 1.0, 100)]);
        let before = clip.notes.clone();
        let g = geom(&pr);
        let from = pos2(g.x_at(0.5), g.row_top(62) + g.row_h() * 0.5);
        drive(
            &mut pr,
            &mut clip,
            &probe::drag_path(from, from + vec2(40.0, 20.0), 5),
        );
        assert_eq!(clip.notes, before, "a marquee edits nothing");
    }

    // --- the modifier grammar -------------------------------------------

    /// ALT COPIES, and the COPIES are what ends up selected — so the next
    /// gesture acts on what you just made, not on what you left behind.
    #[test]
    fn alt_drag_copies_and_selects_the_copies() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 1.0, 100)]);
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        let to = from + vec2(4.0 * g.px_per_beat(), 0.0);
        drive(
            &mut pr,
            &mut clip,
            &probe::drag_path_holding(from, to, 6, Modifiers::ALT),
        );

        assert_eq!(clip.notes.len(), 2, "the original stayed behind");
        assert_eq!(clip.notes[0].start, 0.0, "and it did not move");
        assert!((clip.notes[1].start - 4.0).abs() < 1e-6, "the copy moved");
        assert_eq!(pr.selected, HashSet::from([1]), "the COPY is selected");
    }

    /// SHIFT LOCKS THE AXIS, and locks it ONCE. A constraint that
    /// re-decides every frame flips under the hand.
    #[test]
    fn shift_locks_the_axis_and_holds_it() {
        let (mut pr, mut clip) = scene(vec![note(60, 4.0, 1.0, 100)]);
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // Mostly horizontal at first — which commits to time — then a
        // long vertical excursion that must be ignored.
        let mid = from + vec2(3.0 * g.px_per_beat(), 0.0);
        let end = mid + vec2(0.0, -6.0 * g.row_h());
        let mut path = probe::drag_path_holding(from, mid, 6, Modifiers::SHIFT);
        path.pop();
        path.extend(
            probe::drag_path_holding(mid, end, 6, Modifiers::SHIFT)
                .into_iter()
                .skip(2),
        );
        drive(&mut pr, &mut clip, &path);

        assert_eq!(clip.notes[0].pitch, 60, "the axis lock held: no transpose");
        assert!(clip.notes[0].start > 4.0, "and time still moved");
    }

    /// CTRL+ALT BYPASSES SNAP. Alt alone is copy — Live, Logic and Cubase
    /// all agree — so the snap escape moves one key over rather than
    /// fighting it.
    #[test]
    fn ctrl_alt_bypasses_the_grid() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 1.0, 100)]);
        pr.grid = 2; // a coarse rung, so an unsnapped landing is obvious
        let g = geom(&pr);
        let from = body_of(g, &clip.notes[0]);
        // A deliberately awkward distance: a third of a beat.
        let to = from + vec2(g.px_per_beat() / 3.0, 0.0);
        let free = Modifiers {
            alt: true,
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        drive(
            &mut pr,
            &mut clip,
            &probe::drag_path_holding(from, to, 4, free),
        );

        let landed = clip.notes[0].start;
        assert!(landed > 0.0, "it moved");
        assert!(
            (landed / pr.grid_beats()).fract().abs() > 1e-6,
            "and it landed off the grid, at {landed}"
        );
    }

    // --- resizing --------------------------------------------------------

    /// THE LEFT EDGE HOLDS THE RIGHT ONE. Pulling a note's head must not
    /// also move its tail — which is the whole reason the left edge is a
    /// separate variant rather than a negative resize.
    #[test]
    fn left_edge_resize_holds_the_right_edge() {
        let (mut pr, mut clip) = scene(vec![note(60, 4.0, 4.0, 100)]);
        let g = geom(&pr);
        let from = left_edge_of(g, &clip.notes[0]);
        let to = from + vec2(2.0 * g.px_per_beat(), 0.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 6));

        let n = &clip.notes[0];
        assert!(
            (n.start - 6.0).abs() < 1e-6,
            "the head moved to {}",
            n.start
        );
        assert!(
            (n.start + n.len - 8.0).abs() < 1e-6,
            "the tail stayed at 8, not {}",
            n.start + n.len
        );
    }

    /// The right edge resizes the WHOLE selection, like every other verb.
    #[test]
    fn the_right_edge_resizes_the_selection() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 1.0, 100), note(64, 0.0, 1.0, 100)]);
        pr.selected = (0..2).collect();
        let g = geom(&pr);
        let from = right_edge_of(g, &clip.notes[0]);
        let to = from + vec2(2.0 * g.px_per_beat(), 0.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 6));
        for n in &clip.notes {
            assert!((n.len - 3.0).abs() < 1e-6, "len is {}", n.len);
        }
    }

    /// A resize can never produce a note shorter than a grid step. Zero
    /// length is not a note.
    #[test]
    fn a_resize_never_reaches_zero() {
        let (mut pr, mut clip) = scene(vec![note(60, 4.0, 2.0, 100)]);
        let g = geom(&pr);
        let from = right_edge_of(g, &clip.notes[0]);
        let to = from - vec2(20.0 * g.px_per_beat(), 0.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 8));
        assert!(
            clip.notes[0].len >= pr.grid_beats(),
            "len {}",
            clip.notes[0].len
        );
    }

    // --- the verbs that used to be dangerous ------------------------------

    /// DOUBLE-CLICK NO LONGER DELETES. It was a permanent verb on a
    /// gesture people make by accident, and no reference DAW puts it
    /// there. On empty ground it still adds, which is what it is for.
    #[test]
    fn double_click_adds_and_never_deletes() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 1.0, 100)]);
        let g = geom(&pr);
        let at = body_of(g, &clip.notes[0]);
        // Two clicks in a row on the note: egui reads that as a
        // double-click.
        let mut path = probe::click_path(at);
        path.extend(probe::click_path(at).into_iter().skip(1));
        drive(&mut pr, &mut clip, &path);
        assert_eq!(clip.notes.len(), 1, "the note is still there");

        // On empty ground it adds one.
        let empty = pos2(g.x_at(6.0) + 4.0, g.row_top(64) + g.row_h() * 0.5);
        let mut path = probe::click_path(empty);
        path.extend(probe::click_path(empty).into_iter().skip(1));
        drive(&mut pr, &mut clip, &path);
        assert_eq!(clip.notes.len(), 2, "double-click on empty ground adds");
        assert_eq!(clip.notes[1].pitch, 64);
    }

    // --- hover ------------------------------------------------------------

    /// HOVER TARGETS EXACTLY ONE NOTE, and it is the one on top — later
    /// notes draw over earlier ones, and the one you can see is the one
    /// you mean.
    #[test]
    fn hover_targets_exactly_one_note_the_one_on_top() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 4.0, 100), note(60, 1.0, 1.0, 100)]);
        let g = geom(&pr);
        let over_both = body_of(g, &clip.notes[1]);
        drive(&mut pr, &mut clip, &[probe::Step::moved(over_both); 2]);
        assert_eq!(pr.hover.map(|(i, _)| i), Some(1), "the topmost note");

        // And off the notes entirely, nothing is hovered but the cell
        // still reports where the pointer is.
        let empty = pos2(g.x_at(10.0), g.row_top(62) + g.row_h() * 0.5);
        drive(&mut pr, &mut clip, &[probe::Step::moved(empty); 2]);
        assert!(pr.hover.is_none());
        assert_eq!(pr.hover_cell.map(|(p, _)| p), Some(62));
    }

    /// The three zones of a note are three different gestures, and the
    /// narrow ones do not exist on a note too small to hold them.
    #[test]
    fn a_note_has_three_zones_until_it_is_too_small_for_them() {
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 14.0));
        assert_eq!(zone_of(wide, pos2(2.0, 7.0)), Zone::Left);
        assert_eq!(zone_of(wide, pos2(40.0, 7.0)), Zone::Body);
        assert_eq!(zone_of(wide, pos2(78.0, 7.0)), Zone::Right);

        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(ZONE_MIN_W - 1.0, 14.0));
        for x in [0.5, 8.0, ZONE_MIN_W - 1.5] {
            assert_eq!(
                zone_of(narrow, pos2(x, 7.0)),
                Zone::Body,
                "a note too small for three zones must not pretend"
            );
        }
    }

    // --- the tools ---------------------------------------------------------

    /// The eraser sweeps: one drag across four notes removes four notes.
    #[test]
    fn the_eraser_sweeps() {
        let (mut pr, mut clip) = scene(vec![
            note(60, 0.0, 1.0, 100),
            note(60, 1.0, 1.0, 100),
            note(60, 2.0, 1.0, 100),
            note(60, 3.0, 1.0, 100),
        ]);
        pr.tool = Tool::Erase;
        let g = geom(&pr);
        let y = g.row_top(60) + g.row_h() * 0.5;
        drive(
            &mut pr,
            &mut clip,
            &probe::drag_path(pos2(g.x_at(0.5), y), pos2(g.x_at(3.5), y), 24),
        );
        assert!(clip.notes.is_empty(), "{} survived", clip.notes.len());
    }

    /// The mute sweep toggles each note ONCE, however much the hand
    /// wobbles over it.
    #[test]
    fn the_mute_sweep_toggles_each_note_once() {
        let (mut pr, mut clip) = scene(vec![note(60, 0.0, 4.0, 100)]);
        pr.tool = Tool::Mute;
        let g = geom(&pr);
        let y = g.row_top(60) + g.row_h() * 0.5;
        // Back and forth across the same single note, many times.
        let mut path = probe::drag_path(pos2(g.x_at(0.2), y), pos2(g.x_at(3.8), y), 30);
        path.pop();
        path.extend(
            probe::drag_path(pos2(g.x_at(3.8), y), pos2(g.x_at(0.2), y), 30)
                .into_iter()
                .skip(2),
        );
        drive(&mut pr, &mut clip, &path);
        assert!(clip.notes[0].muted, "one sweep, one toggle");
    }

    /// Draw places a note on the press and sizes it with the same drag.
    #[test]
    fn draw_places_and_sizes_in_one_gesture() {
        let (mut pr, mut clip) = scene(Vec::new());
        pr.tool = Tool::Draw;
        let g = geom(&pr);
        let y = g.row_top(65) + g.row_h() * 0.5;
        drive(
            &mut pr,
            &mut clip,
            &probe::drag_path(pos2(g.x_at(2.0) + 2.0, y), pos2(g.x_at(5.0), y), 8),
        );
        assert_eq!(clip.notes.len(), 1);
        let n = &clip.notes[0];
        assert_eq!(n.pitch, 65);
        assert!((n.start - 2.0).abs() < 1e-6, "start {}", n.start);
        assert!((n.len - 3.0).abs() < 1e-6, "len {}", n.len);
    }

    /// The split tool cuts where it is clicked, and the tail keeps every
    /// property the head had.
    #[test]
    fn split_cuts_at_the_pointer_and_keeps_the_properties() {
        let (mut pr, mut clip) = scene(vec![Note {
            vel: 77,
            prob: 0.5,
            ..note(60, 0.0, 4.0, 100)
        }]);
        pr.tool = Tool::Split;
        let g = geom(&pr);
        let at = pos2(g.x_at(2.0), g.row_top(60) + g.row_h() * 0.5);
        drive(&mut pr, &mut clip, &probe::click_path(at));

        assert_eq!(clip.notes.len(), 2);
        assert!((clip.notes[0].len - 2.0).abs() < 1e-6);
        assert!((clip.notes[1].start - 2.0).abs() < 1e-6);
        assert!((clip.notes[1].len - 2.0).abs() < 1e-6);
        assert_eq!(clip.notes[1].vel, 77, "the tail is the same note");
        assert_eq!(clip.notes[1].prob, 0.5);
    }

    /// Clicking a tool in the strip latches it, and Escape comes home.
    #[test]
    fn the_tool_strip_latches_and_escape_comes_home() {
        let (mut pr, mut clip) = scene(Vec::new());
        let lay = layout(PANEL, &pr.lanes);
        // The third button down is Erase.
        let at = pos2(
            lay.tools.center().x,
            lay.tools.top() + TOOLS_W * 2.0 + TOOLS_W * 0.5,
        );
        drive(&mut pr, &mut clip, &probe::click_path(at));
        assert_eq!(pr.tool, Tool::Erase);
    }

    // --- the lane ----------------------------------------------------------

    /// A lane drag paints the SELECTION when the press lands on a
    /// selected note's bar — one gesture, eight velocities.
    #[test]
    fn a_lane_paints_the_whole_selection() {
        let (mut pr, mut clip) = scene(vec![
            note(60, 0.0, 1.0, 20),
            note(64, 1.0, 1.0, 20),
            note(67, 2.0, 1.0, 20),
        ]);
        pr.selected = (0..3).collect();
        let lay = layout(PANEL, &pr.lanes);
        let (_, _, lane_body) = lay.lanes[0];
        let g = geom(&pr).over(lane_body);
        let from = pos2(g.x_at(0.0), lane_body.bottom() - 4.0);
        let to = pos2(from.x, lane_body.top() + 2.0);
        drive(&mut pr, &mut clip, &probe::drag_path(from, to, 6));

        for n in &clip.notes {
            assert!(n.vel > 100, "every selected velocity rose, got {}", n.vel);
        }
    }

    /// GHOSTS ARE NEVER HIT-TESTED and never enter the selection. They
    /// are reference material; a click that lands on one must behave
    /// exactly as if the ground there were empty.
    #[test]
    fn ghosts_are_reference_material_and_nothing_else() {
        let (mut pr, mut clip) = scene(Vec::new());
        let ghosts = vec![note(60, 0.0, 4.0, 100)];
        let g = geom(&pr);
        let over = g.note_rect(&ghosts[0]).center();

        // Hovering one finds nothing.
        drive_with_ghosts(&mut pr, &mut clip, &ghosts, &[probe::Step::moved(over); 2]);
        assert!(pr.hover.is_none(), "a ghost was hovered");

        // Dragging across one selects a region and edits nothing.
        drive_with_ghosts(
            &mut pr,
            &mut clip,
            &ghosts,
            &probe::drag_path(over, over + vec2(60.0, 20.0), 6),
        );
        assert!(pr.selected.is_empty(), "a ghost entered the selection");
        assert!(clip.notes.is_empty(), "a ghost was edited into the clip");
        assert_eq!(ghosts.len(), 1, "and the ghost itself is untouched");
    }

    /// Each lane writes ONLY the property it names.
    #[test]
    fn each_lane_writes_only_its_own_property() {
        for lane in Lane::ALL {
            let mut n = note(60, 1.0, 2.0, 64);
            n.prob = 0.5;
            let before = n.clone();
            lane.set(&mut n, 0.25, 0.25);
            match lane {
                Lane::Velocity => {
                    assert_ne!(n.vel, before.vel);
                    assert_eq!(n.len, before.len);
                    assert_eq!(n.prob, before.prob);
                }
                Lane::Probability => {
                    assert_ne!(n.prob, before.prob);
                    assert_eq!(n.vel, before.vel);
                    assert_eq!(n.len, before.len);
                }
                Lane::Length => {
                    assert_ne!(n.len, before.len);
                    assert_eq!(n.vel, before.vel);
                    assert_eq!(n.prob, before.prob);
                }
            }
            assert_eq!(n.pitch, before.pitch, "no lane ever moves a note");
            assert_eq!(n.start, before.start);
        }
    }
}

// ----------------------------------------------------------- transforms ---
//
// The verbs a writer actually reaches for, all acting on a set of note
// indices, all pure, all one undo entry by construction — `History::sync`
// banks whatever changed once the hand comes off, so nothing here has to
// remember to say so.
//
// Free functions rather than methods, because every one of them is worth
// testing against a fixed input and an exact expected output, and because
// the palette will want to call them with arguments the keyboard does not
// offer.

/// A tiny deterministic generator.
///
/// Randomised verbs must be REPRODUCIBLE: a humanize you cannot repeat is
/// a humanize you cannot undo-and-retry, and a bounce that comes out
/// different every time is not a bounce. The seed rides view state, so the
/// same selection humanized twice with the same seed lands identically.
///
/// xorshift32 — three shifts, no dependency, and far better distributed
/// than the modulo-a-counter trick it replaces.
#[derive(Debug, Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // Two things to get right, both of which bit the first version.
        //
        // Zero is xorshift's fixed point and emits nothing but zero. And
        // ADJACENT seeds must not produce adjacent streams: seeds 42 and
        // 43 differ in one low bit, which xorshift barely propagates for
        // several rounds, so `humanize` with two nearby seeds came out
        // identical. The seed therefore goes through a mixer first.
        let mut x = if seed == 0 { 0x9e37_79b9 } else { seed };
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb_352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846c_a68b);
        x ^= x >> 16;
        Self(x.max(1))
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A value in `-1.0..=1.0`.
    ///
    /// From the HIGH sixteen bits: xorshift32's low bits are its weakest,
    /// and this is the only place the numbers are actually looked at.
    fn bipolar(&mut self) -> f32 {
        f32::from((self.next_u32() >> 16) as u16) / f32::from(u16::MAX) * 2.0 - 1.0
    }
}

/// Quantize starts toward the grid at `strength`, with `swing` pushing
/// every odd grid position late.
///
/// `strength` is the fraction of the way to the grid line, so 1.0 is the
/// hard snap `Ctrl+U` has always done and 0.6 is "tighten it up but keep
/// the feel". Anything else is what makes quantize musical rather than
/// mechanical, and it is the single most-requested thing missing from the
/// old one-strength verb.
///
/// `swing` is `0.0..=1.0`, offsetting odd grid slots by that fraction of
/// half a grid step — the usual definition, and the one that makes 0.0
/// straight and ~0.66 a hard shuffle.
pub fn quantize(notes: &mut [Note], sel: &[usize], grid: f64, strength: f32, swing: f32) {
    if grid <= 0.0 {
        return;
    }
    let strength = f64::from(strength.clamp(0.0, 1.0));
    let swing = f64::from(swing.clamp(0.0, 1.0));
    for &i in sel {
        let Some(n) = notes.get_mut(i) else { continue };
        let slot = (n.start / grid).round();
        // Odd slots are the off-beats; swing pushes them late by a
        // fraction of half a step, so a straight grid and a shuffled one
        // agree on the down-beats.
        let late = if (slot as i64).rem_euclid(2) == 1 {
            swing * grid * 0.5
        } else {
            0.0
        };
        let target = (slot * grid + late).max(0.0);
        n.start = (n.start + (target - n.start) * strength).max(0.0);
    }
}

/// Nudge starts and velocities by a bounded random amount.
///
/// `time` is in beats and `vel` in MIDI steps; both are the FULL width of
/// the excursion either side of where the note already is. Deterministic
/// from `seed`.
pub fn humanize(notes: &mut [Note], sel: &[usize], time: f64, vel: i32, seed: u32) {
    let mut rng = Rng::new(seed);
    for &i in sel {
        let Some(n) = notes.get_mut(i) else { continue };
        n.start = (n.start + f64::from(rng.bipolar()) * time).max(0.0);
        n.vel = nudged_velocity(n.vel, (rng.bipolar() * vel as f32).round() as i32);
    }
}

/// Stretch every selected note to meet the next note's start.
///
/// Ableton's Legato: the next START in the whole clip, not the next note
/// at the same pitch — a chord that changes underneath a held melody note
/// is what actually ends the melody note.
pub fn legato(notes: &mut [Note], sel: &[usize]) {
    let mut starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
    starts.sort_by(f64::total_cmp);
    for &i in sel {
        let Some(n) = notes.get(i) else { continue };
        let here = n.start;
        let next = starts.iter().copied().find(|&s| s > here + 1e-9);
        if let (Some(next), Some(n)) = (next, notes.get_mut(i)) {
            n.len = next - here;
        }
    }
}

/// Spread notes that begin together, earliest pitch first.
///
/// `spread` is the gap between neighbours, in beats; negative strums from
/// the top down. Only notes that share a start are touched, because a
/// strum across notes that were never simultaneous is just a delay.
pub fn strum(notes: &mut [Note], sel: &[usize], spread: f64) {
    // Group the selection by start, to the nearest tick.
    let mut groups: Vec<(f64, Vec<usize>)> = Vec::new();
    for &i in sel {
        let Some(n) = notes.get(i) else { continue };
        match groups.iter_mut().find(|(s, _)| (*s - n.start).abs() < 1e-6) {
            Some((_, members)) => members.push(i),
            None => groups.push((n.start, vec![i])),
        }
    }
    for (start, mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort_by_key(|&i| notes.get(i).map_or(0, |n| n.pitch));
        if spread < 0.0 {
            members.reverse();
        }
        let step = spread.abs();
        for (rank, &i) in members.iter().enumerate() {
            if let Some(n) = notes.get_mut(i) {
                n.start = (start + rank as f64 * step).max(0.0);
            }
        }
    }
}

/// Draw one lane's values along a straight line in TIME.
///
/// `t` runs from `from_beat` to `to_beat`, and each note's value is read
/// off the line at its own start — a crescendo across the bar, not a list
/// walked in whatever order the notes happen to be stored in, which is
/// the difference between a ramp and a shuffle.
///
/// One function for both callers: the lane's Shift-drag and the palette's
/// `vel 40..100 ramp`. Two implementations of "draw a straight line"
/// would eventually draw two different lines.
#[allow(clippy::too_many_arguments)]
pub fn lane_ramp(
    notes: &mut [Note],
    sel: &[usize],
    lane: Lane,
    from_beat: f64,
    to_beat: f64,
    v0: f32,
    v1: f32,
    grid: f64,
) {
    let width = to_beat - from_beat;
    for &i in sel {
        let Some(n) = notes.get(i) else { continue };
        let t = if width.abs() < 1e-9 {
            0.0
        } else {
            (((n.start - from_beat) / width) as f32).clamp(0.0, 1.0)
        };
        let v = v0 + (v1 - v0) * t;
        if let Some(n) = notes.get_mut(i) {
            lane.set(n, v, grid);
        }
    }
}

/// Reverse the selection in time, inside its own span. Rhythm mirrored,
/// pitches untouched.
pub fn retrograde(notes: &mut [Note], sel: &[usize]) {
    let (lo, hi) = span_of(notes, sel);
    if !lo.is_finite() {
        return;
    }
    for &i in sel {
        let Some(n) = notes.get_mut(i) else { continue };
        // The note's END becomes its distance from the start, so a note
        // that ended on the last beat now begins on the first.
        n.start = (lo + hi - (n.start + n.len)).max(0.0);
    }
}

/// Mirror pitches about `pivot`. The selection's own middle is the usual
/// pivot and the one the keyboard verb passes.
pub fn invert(notes: &mut [Note], sel: &[usize], pivot: u8) {
    for &i in sel {
        let Some(n) = notes.get_mut(i) else { continue };
        n.pitch = transposed(pivot, i32::from(pivot) - i32::from(n.pitch));
    }
}

/// The pitch every selected note would be inverted about: the middle of
/// the selection's own range.
pub fn pivot_of(notes: &[Note], sel: &[usize]) -> u8 {
    let mut lo = PITCH_MAX;
    let mut hi = 0u8;
    let mut any = false;
    for &i in sel {
        if let Some(n) = notes.get(i) {
            lo = lo.min(n.pitch);
            hi = hi.max(n.pitch);
            any = true;
        }
    }
    if any {
        ((u16::from(lo) + u16::from(hi)) / 2) as u8
    } else {
        C4
    }
}

/// Move every selected note onto the nearest degree of `key`.
///
/// Note by note rather than by a shared interval, because this is a
/// correction and not a transposition: the point is that each wrong note
/// becomes right, even if that changes the interval between two of them.
pub fn force_to_scale(notes: &mut [Note], sel: &[usize], key: Key) {
    for &i in sel {
        if let Some(n) = notes.get_mut(i) {
            n.pitch = snap_to_scale(n.pitch, key);
        }
    }
}

/// Scale the selection's placement and length about its own start.
/// `2.0` doubles the span — half time; `0.5` halves it — double time.
pub fn scale_time(notes: &mut [Note], sel: &[usize], factor: f64) {
    if factor <= 0.0 {
        return;
    }
    let (lo, _) = span_of(notes, sel);
    if !lo.is_finite() {
        return;
    }
    for &i in sel {
        let Some(n) = notes.get_mut(i) else { continue };
        n.start = (lo + (n.start - lo) * factor).max(0.0);
        n.len = (n.len * factor).max(f64::MIN_POSITIVE);
    }
}

/// The first start and last end of a set of notes. `(inf, -inf)` if the
/// set is empty, which every caller checks for.
fn span_of(notes: &[Note], sel: &[usize]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &i in sel {
        if let Some(n) = notes.get(i) {
            lo = lo.min(n.start);
            hi = hi.max(n.start + n.len);
        }
    }
    (lo, hi)
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
            muted: false,
            plocks: Vec::new(),
            prob: 1.0,
            cond: None,
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
            muted: false,
            plocks: Vec::new(),
            prob: 1.0,
            cond: None,
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
            muted: false,
            plocks: Vec::new(),
            prob: 1.0,
            cond: None,
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
            muted: false,
            plocks: Vec::new(),
            prob: 1.0,
            cond: None,
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod transforms {
    use super::*;
    use crate::note;

    fn all(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    // --- quantize ---------------------------------------------------------

    /// Strength is the fraction of the way to the grid, and 1.0 is the
    /// hard snap `Ctrl+U` has always been.
    #[test]
    fn quantize_strength_moves_part_of_the_way() {
        let late = || vec![note(60, 1.10, 1.0, 100)];

        let mut hard = late();
        quantize(&mut hard, &all(1), 1.0, 1.0, 0.0);
        assert!((hard[0].start - 1.0).abs() < 1e-9, "{}", hard[0].start);

        let mut half = late();
        quantize(&mut half, &all(1), 1.0, 0.5, 0.0);
        assert!(
            (half[0].start - 1.05).abs() < 1e-9,
            "halfway, not all the way: {}",
            half[0].start
        );

        let mut none = late();
        quantize(&mut none, &all(1), 1.0, 0.0, 0.0);
        assert!(
            (none[0].start - 1.10).abs() < 1e-9,
            "zero strength is a no-op"
        );
    }

    /// Swing pushes the OFF-beats late and leaves the down-beats where
    /// they are — which is what makes a straight grid and a shuffled one
    /// agree about where beat one is.
    #[test]
    fn swing_moves_the_offbeats_only() {
        let mut notes = vec![
            note(60, 0.0, 0.5, 100),
            note(60, 0.5, 0.5, 100),
            note(60, 1.0, 0.5, 100),
            note(60, 1.5, 0.5, 100),
        ];
        quantize(&mut notes, &all(4), 0.5, 1.0, 0.5);
        assert!((notes[0].start - 0.0).abs() < 1e-9, "slot 0 is a down-beat");
        assert!(
            (notes[1].start - 0.625).abs() < 1e-9,
            "slot 1 is late by half of half a step: {}",
            notes[1].start
        );
        assert!((notes[2].start - 1.0).abs() < 1e-9, "slot 2 is a down-beat");
        assert!((notes[3].start - 1.625).abs() < 1e-9, "{}", notes[3].start);
    }

    /// Quantize never produces a negative start.
    #[test]
    fn quantize_never_goes_before_the_beginning() {
        let mut notes = vec![note(60, 0.05, 1.0, 100)];
        quantize(&mut notes, &all(1), 1.0, 1.0, 0.0);
        assert!(notes[0].start >= 0.0);
    }

    // --- humanize ---------------------------------------------------------

    /// A RANDOMISED VERB REPRODUCES FROM ITS SEED. Anything else cannot be
    /// bounced twice and cannot be undone and retried.
    #[test]
    fn humanize_reproduces_from_its_seed() {
        let base = || {
            vec![
                note(60, 0.0, 1.0, 64),
                note(62, 1.0, 1.0, 64),
                note(64, 2.0, 1.0, 64),
            ]
        };
        let mut a = base();
        let mut b = base();
        let mut c = base();
        humanize(&mut a, &all(3), 0.05, 12, 42);
        humanize(&mut b, &all(3), 0.05, 12, 42);
        humanize(&mut c, &all(3), 0.05, 12, 43);
        assert_eq!(a, b, "the same seed is the same pass");
        assert_ne!(a, c, "a different seed is a different pass");
    }

    /// And it stays inside the bounds it was given, on both channels.
    #[test]
    fn humanize_stays_inside_its_amount() {
        let mut notes: Vec<Note> = (0..64).map(|i| note(60, f64::from(i), 1.0, 64)).collect();
        let before = notes.clone();
        humanize(&mut notes, &all(64), 0.05, 12, 7);
        for (n, was) in notes.iter().zip(&before) {
            assert!(
                (n.start - was.start).abs() <= 0.05 + 1e-9,
                "moved {} beats",
                n.start - was.start
            );
            assert!((i32::from(n.vel) - i32::from(was.vel)).abs() <= 12);
            assert!(n.vel >= VELOCITY_MIN, "never a note-off");
            assert!(n.start >= 0.0);
        }
    }

    // --- legato -----------------------------------------------------------

    /// Each note stretches to the NEXT start in the clip — including one
    /// at a different pitch, because a chord change is what ends a held
    /// melody note.
    #[test]
    fn legato_reaches_the_next_start_whatever_its_pitch() {
        let mut notes = vec![
            note(60, 0.0, 0.25, 100),
            note(67, 1.0, 0.25, 100),
            note(60, 3.0, 0.25, 100),
        ];
        legato(&mut notes, &all(3));
        assert!((notes[0].len - 1.0).abs() < 1e-9, "{}", notes[0].len);
        assert!((notes[1].len - 2.0).abs() < 1e-9, "{}", notes[1].len);
        assert!(
            (notes[2].len - 0.25).abs() < 1e-9,
            "the last note has nothing to reach and keeps its length"
        );
    }

    // --- strum ------------------------------------------------------------

    /// Only notes that actually began together are spread, lowest first —
    /// and a negative spread strums from the top.
    #[test]
    fn strum_spreads_a_stack_and_leaves_a_line_alone() {
        let mut notes = vec![
            note(60, 0.0, 1.0, 100),
            note(64, 0.0, 1.0, 100),
            note(67, 0.0, 1.0, 100),
            note(72, 2.0, 1.0, 100),
        ];
        strum(&mut notes, &all(4), 0.1);
        assert!((notes[0].start - 0.0).abs() < 1e-9);
        assert!((notes[1].start - 0.1).abs() < 1e-9);
        assert!((notes[2].start - 0.2).abs() < 1e-9);
        assert!(
            (notes[3].start - 2.0).abs() < 1e-9,
            "a lone note is not a chord"
        );

        let mut down = vec![
            note(60, 0.0, 1.0, 100),
            note(64, 0.0, 1.0, 100),
            note(67, 0.0, 1.0, 100),
        ];
        strum(&mut down, &all(3), -0.1);
        assert!((down[2].start - 0.0).abs() < 1e-9, "the top note leads");
        assert!((down[0].start - 0.2).abs() < 1e-9, "the bottom note trails");
    }

    // --- ramps ------------------------------------------------------------

    /// THE RAMP LANDS ON A STRAIGHT LINE, read off each note's own
    /// position in time rather than its index.
    #[test]
    fn the_ramp_lands_on_a_straight_line() {
        // Deliberately out of order in storage, and unevenly spaced.
        let mut notes = vec![
            note(60, 4.0, 1.0, 1),
            note(60, 0.0, 1.0, 1),
            note(60, 3.0, 1.0, 1),
            note(60, 1.0, 1.0, 1),
        ];
        lane_ramp(
            &mut notes,
            &all(4),
            Lane::Velocity,
            0.0,
            4.0,
            0.0,
            1.0,
            0.25,
        );
        let mut got: Vec<(f64, u8)> = notes.iter().map(|n| (n.start, n.vel)).collect();
        got.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (start, vel) in got {
            let want = (start / 4.0 * 127.0).round().max(1.0) as u8;
            assert_eq!(vel, want, "at beat {start}");
        }
    }

    /// A zero-width ramp is not a division by zero.
    #[test]
    fn a_ramp_with_no_width_is_flat() {
        let mut notes = vec![note(60, 2.0, 1.0, 10), note(64, 2.0, 1.0, 10)];
        lane_ramp(
            &mut notes,
            &all(2),
            Lane::Velocity,
            2.0,
            2.0,
            0.2,
            0.9,
            0.25,
        );
        assert_eq!(notes[0].vel, notes[1].vel);
        assert!(notes[0].vel > 0);
    }

    // --- retrograde and inversion ------------------------------------------

    /// Retrograde mirrors rhythm inside the selection's own span, and
    /// leaves pitch alone.
    #[test]
    fn retrograde_mirrors_the_rhythm_in_place() {
        let mut notes = vec![
            note(60, 0.0, 1.0, 100),
            note(62, 1.0, 3.0, 100),
            note(64, 4.0, 2.0, 100),
        ];
        let (lo, hi) = span_of(&notes, &all(3));
        assert_eq!((lo, hi), (0.0, 6.0));
        retrograde(&mut notes, &all(3));

        assert!((notes[0].start - 5.0).abs() < 1e-9, "{}", notes[0].start);
        assert!((notes[1].start - 2.0).abs() < 1e-9, "{}", notes[1].start);
        assert!((notes[2].start - 0.0).abs() < 1e-9, "{}", notes[2].start);
        // The span is unchanged and the pitches never moved.
        let (lo2, hi2) = span_of(&notes, &all(3));
        assert!((lo2 - lo).abs() < 1e-9 && (hi2 - hi).abs() < 1e-9);
        assert_eq!(
            notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            vec![60, 62, 64]
        );

        // And it is its own inverse.
        retrograde(&mut notes, &all(3));
        assert!((notes[0].start - 0.0).abs() < 1e-9);
        assert!((notes[1].start - 1.0).abs() < 1e-9);
        assert!((notes[2].start - 4.0).abs() < 1e-9);
    }

    /// Inversion mirrors pitch about the selection's own middle, and is
    /// its own inverse too.
    #[test]
    fn inversion_mirrors_about_the_pivot() {
        let mut notes = vec![
            note(60, 0.0, 1.0, 100),
            note(64, 1.0, 1.0, 100),
            note(72, 2.0, 1.0, 100),
        ];
        let pivot = pivot_of(&notes, &all(3));
        assert_eq!(pivot, 66);
        invert(&mut notes, &all(3), pivot);
        assert_eq!(
            notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            vec![72, 68, 60]
        );
        invert(&mut notes, &all(3), pivot);
        assert_eq!(
            notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            vec![60, 64, 72]
        );
    }

    /// Inversion clamps rather than wrapping: a pitch pushed off the end
    /// of the keyboard stops at the end of the keyboard.
    #[test]
    fn inversion_clamps_to_the_keyboard() {
        let mut notes = vec![note(2, 0.0, 1.0, 100), note(125, 0.0, 1.0, 100)];
        invert(&mut notes, &all(2), 120);
        assert!(notes.iter().all(|n| n.pitch <= PITCH_MAX));
        invert(&mut notes, &all(2), 4);
        assert!(notes.iter().all(|n| n.pitch <= PITCH_MAX));
    }

    // --- scale --------------------------------------------------------------

    /// Every note lands on a degree of the key, and a note already on one
    /// does not move.
    #[test]
    fn force_to_scale_lands_on_the_nearest_degree() {
        let key = Key {
            tonic: 0,
            scale: theory::Scale::Major,
        };
        let mut notes: Vec<Note> = (60..=72).map(|p| note(p, 0.0, 1.0, 100)).collect();
        let before = notes.clone();
        let sel = all(notes.len());
        force_to_scale(&mut notes, &sel, key);
        for (n, was) in notes.iter().zip(&before) {
            assert!(
                key.scale.contains(key.tonic, n.pitch),
                "{} did not land in the key",
                n.pitch
            );
            assert!(
                (i32::from(n.pitch) - i32::from(was.pitch)).abs() <= 1,
                "{} travelled too far to reach {}",
                was.pitch,
                n.pitch
            );
            if key.scale.contains(key.tonic, was.pitch) {
                assert_eq!(n.pitch, was.pitch, "an in-key note must not move");
            }
        }
    }

    /// The constraint the DRAG uses is the same one, and it never refuses
    /// a gesture — it lands on the nearest legal pitch instead.
    #[test]
    fn scale_lock_snaps_rather_than_stopping() {
        let key = Key {
            tonic: 0,
            scale: theory::Scale::Major,
        };
        for pitch in 0..=PITCH_MAX {
            let got = snap_to_scale(pitch, key);
            assert!(key.scale.contains(key.tonic, got), "{pitch} -> {got}");
            assert!((i32::from(got) - i32::from(pitch)).abs() <= 1);
        }
    }

    // --- time scaling ---------------------------------------------------------

    /// Double time and half time are exact inverses, anchored on the
    /// selection's own first note rather than on beat zero.
    #[test]
    fn scaling_time_is_reversible_and_anchored() {
        let start = vec![
            note(60, 4.0, 1.0, 100),
            note(62, 5.0, 1.0, 100),
            note(64, 7.0, 2.0, 100),
        ];
        let mut notes = start.clone();
        scale_time(&mut notes, &all(3), 2.0);
        assert!((notes[0].start - 4.0).abs() < 1e-9, "the anchor holds");
        assert!((notes[1].start - 6.0).abs() < 1e-9);
        assert!((notes[2].start - 10.0).abs() < 1e-9);
        assert!((notes[2].len - 4.0).abs() < 1e-9);

        scale_time(&mut notes, &all(3), 0.5);
        for (n, was) in notes.iter().zip(&start) {
            assert!((n.start - was.start).abs() < 1e-9);
            assert!((n.len - was.len).abs() < 1e-9);
        }
    }

    // --- the rules every transform shares ---------------------------------------

    /// A TRANSFORM WITH NOTHING SELECTED CHANGES NOTHING. Every one of
    /// them, checked in one place so a new verb cannot quietly skip it.
    #[test]
    fn transforms_with_no_selection_change_nothing() {
        let key = Key::default();
        /// A verb under test: its name, and the call that runs it.
        type Verb = (&'static str, Box<dyn Fn(&mut Vec<Note>)>);
        let verbs: Vec<Verb> = vec![
            (
                "quantize",
                Box::new(|ns: &mut Vec<Note>| quantize(ns, &[], 0.25, 1.0, 0.0)),
            ),
            (
                "humanize",
                Box::new(|ns: &mut Vec<Note>| humanize(ns, &[], 0.1, 10, 1)),
            ),
            ("legato", Box::new(|ns: &mut Vec<Note>| legato(ns, &[]))),
            ("strum", Box::new(|ns: &mut Vec<Note>| strum(ns, &[], 0.1))),
            (
                "ramp",
                Box::new(|ns: &mut Vec<Note>| {
                    lane_ramp(ns, &[], Lane::Velocity, 0.0, 4.0, 0.0, 1.0, 0.25)
                }),
            ),
            (
                "retrograde",
                Box::new(|ns: &mut Vec<Note>| retrograde(ns, &[])),
            ),
            ("invert", Box::new(|ns: &mut Vec<Note>| invert(ns, &[], 60))),
            (
                "scale",
                Box::new(move |ns: &mut Vec<Note>| force_to_scale(ns, &[], key)),
            ),
            (
                "time",
                Box::new(|ns: &mut Vec<Note>| scale_time(ns, &[], 2.0)),
            ),
        ];
        for (name, verb) in verbs {
            let before = vec![note(61, 1.3, 0.7, 55), note(66, 2.9, 1.1, 90)];
            let mut notes = before.clone();
            verb(&mut notes);
            assert_eq!(notes, before, "{name} touched an empty selection");
        }
    }

    /// And no transform ever produces a note the model considers illegal:
    /// no zero velocity, no zero length, no pitch off the keyboard, no
    /// start before the beginning.
    #[test]
    fn no_transform_produces_an_illegal_note() {
        let key = Key::default();
        let base: Vec<Note> = (0..12)
            .map(|i| note(48 + i * 6, f64::from(i) * 0.37, 0.9, 3 + i * 9))
            .collect();
        let sel = all(base.len());
        let mut cases: Vec<(&str, Vec<Note>)> = Vec::new();

        let mut n = base.clone();
        quantize(&mut n, &sel, 0.25, 1.0, 0.9);
        cases.push(("quantize", n));
        let mut n = base.clone();
        humanize(&mut n, &sel, 2.0, 120, 99);
        cases.push(("humanize", n));
        let mut n = base.clone();
        legato(&mut n, &sel);
        cases.push(("legato", n));
        let mut n = base.clone();
        strum(&mut n, &sel, -0.5);
        cases.push(("strum", n));
        let mut n = base.clone();
        lane_ramp(&mut n, &sel, Lane::Velocity, 0.0, 4.0, 0.0, 1.0, 0.25);
        cases.push(("ramp", n));
        let mut n = base.clone();
        lane_ramp(&mut n, &sel, Lane::Length, 0.0, 4.0, 0.0, 0.0, 0.25);
        cases.push(("len ramp", n));
        let mut n = base.clone();
        retrograde(&mut n, &sel);
        cases.push(("retrograde", n));
        let mut n = base.clone();
        invert(&mut n, &sel, 3);
        cases.push(("invert low", n));
        let mut n = base.clone();
        invert(&mut n, &sel, 124);
        cases.push(("invert high", n));
        let mut n = base.clone();
        force_to_scale(&mut n, &sel, key);
        cases.push(("scale", n));
        let mut n = base.clone();
        scale_time(&mut n, &sel, 0.01);
        cases.push(("time", n));

        for (name, notes) in cases {
            for note in &notes {
                assert!(note.pitch <= PITCH_MAX, "{name}: pitch {}", note.pitch);
                assert!(note.vel >= VELOCITY_MIN, "{name}: vel {}", note.vel);
                assert!(note.start >= 0.0, "{name}: start {}", note.start);
                assert!(note.len > 0.0, "{name}: len {}", note.len);
                assert!(note.prob >= 0.0 && note.prob <= 1.0, "{name}: prob");
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod folding {
    use super::*;
    use crate::note;

    /// UNFOLDED, the mask arithmetic reduces exactly to the `127 - pitch`
    /// it replaced. This is the property that lets one code path serve
    /// both states.
    #[test]
    fn the_unfolded_mask_is_the_old_arithmetic() {
        for pitch in 0..=PITCH_MAX {
            assert_eq!(Fold::ALL.row_of(pitch), u32::from(PITCH_MAX - pitch));
            assert_eq!(
                Fold::ALL.pitch_of_row(u32::from(PITCH_MAX - pitch)),
                Some(pitch)
            );
        }
        assert_eq!(Fold::ALL.rows(), 128);
        assert_eq!(Fold::ALL.pitch_of_row(128), None);
    }

    /// FOLDING IS A VIEW CHANGE ONLY. It may never touch a note, and it
    /// may never hide one either — a note you cannot see is a note you
    /// will delete by accident.
    #[test]
    fn folding_hides_nothing_that_has_a_note_on_it() {
        let notes = vec![
            note(37, 0.0, 1.0, 100),
            note(60, 1.0, 1.0, 100),
            note(61, 2.0, 1.0, 100),
            note(103, 3.0, 1.0, 100),
        ];
        let mut pr = PianoRoll {
            fold: true,
            ..PianoRoll::default()
        };
        let key = Key::default();
        let before = notes.clone();
        let fold = pr.fold_mask(Some(&notes), key);
        for n in &notes {
            assert!(fold.shows(n.pitch), "pitch {} was folded away", n.pitch);
        }
        assert_eq!(notes, before, "folding edited the clip");

        // And it really did fold: far fewer rows than 128, but still
        // enough empty ones to write the next note on.
        assert!(fold.rows() < 128, "nothing was folded");
        assert!(
            fold.rows() > notes.len() as u32,
            "a fold with no empty row has nowhere to write"
        );
        pr.fold = false;
        assert_eq!(pr.fold_mask(Some(&notes), key), Fold::ALL);
    }

    /// An empty clip does not fold to a blank panel.
    #[test]
    fn an_empty_clip_never_folds_to_nothing() {
        let pr = PianoRoll {
            fold: true,
            ..PianoRoll::default()
        };
        assert_eq!(pr.fold_mask(Some(&[]), Key::default()), Fold::ALL);
        assert_eq!(pr.fold_mask(None, Key::default()), Fold::ALL);
    }

    /// Rows stay in pitch order and stay contiguous, folded or not, so
    /// the grid never has a hole in it.
    #[test]
    fn folded_rows_are_contiguous_and_ordered() {
        let notes = vec![note(40, 0.0, 1.0, 100), note(90, 0.0, 1.0, 100)];
        let pr = PianoRoll {
            fold: true,
            ..PianoRoll::default()
        };
        let fold = pr.fold_mask(Some(&notes), Key::default());
        let rows: Vec<(u32, u8)> = fold.rows_in(0..fold.rows()).collect();
        assert_eq!(rows.len(), fold.rows() as usize);
        for (n, (row, pitch)) in rows.iter().enumerate() {
            assert_eq!(*row, n as u32, "row indices must be dense");
            assert_eq!(fold.row_of(*pitch), *row, "and must round-trip");
        }
        for pair in rows.windows(2) {
            assert!(pair[0].1 > pair[1].1, "high notes stay on top");
        }
    }

    /// The geometry agrees with itself at every zoom AND every fold. This
    /// is the property the old code broke: half the module went through
    /// `Zoom` and half reached for the default constants, so at any zoom
    /// but 1.0 the rows and the notes were drawn in different places.
    #[test]
    fn geometry_agrees_at_every_zoom_and_fold() {
        let grid = egui::Rect::from_min_size(egui::pos2(66.0, 20.0), egui::vec2(1000.0, 300.0));
        let notes = vec![note(48, 0.0, 1.0, 100), note(60, 1.0, 1.0, 100)];
        let folds = [
            Fold::ALL,
            Fold::used(&notes).union(Fold::scale(Key::default())),
        ];
        for fold in folds {
            for row_h in [ROW_H_MIN, 7.3, ROW_H_DEFAULT, 31.0, ROW_H_MAX] {
                for px in [PX_PER_BEAT_MIN, 11.0, PX_PER_BEAT_DEFAULT, 137.0] {
                    let z = Zoom {
                        px_per_beat: px,
                        row_h,
                    };
                    for sy in [0.0, 40.0, 500.0] {
                        let g = Geom::new(grid, 3.5, sy, z, fold);
                        // Every shown pitch round-trips through its row.
                        for (row, pitch) in fold.rows_in(0..fold.rows()) {
                            assert_eq!(g.row_of_check(pitch), row);
                            let y = g.row_y(row);
                            assert!(
                                (g.row_top(pitch) - y).abs() < 1e-3,
                                "row {row} pitch {pitch} disagrees"
                            );
                        }
                        // And every beat round-trips through its pixel.
                        for beat in [0.0, 3.5, 4.25, 19.0] {
                            let back = g.beat_at(g.x_at(beat));
                            assert!((back - beat).abs() < 1e-3, "beat {beat} -> {back}");
                        }
                        // The visible window is a window, never the lot.
                        let visible = g.visible().count();
                        assert!(
                            visible as f32 <= grid.height() / row_h + 2.0,
                            "{visible} rows painted for a {}pt grid at {row_h}pt rows",
                            grid.height()
                        );
                    }
                }
            }
        }
    }
}
