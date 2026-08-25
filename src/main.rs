//! The `daw` application shell.
//!
//! Right now this is a SKELETON, not a UI: four regions boxed out in flat
//! color so the geometry can be judged before anything lives in it. No
//! labels, no widgets, no content — every region's body is an empty closure.
//!
//! These regions are deliberately NOT `ui::host::Panel`s. They are the frame
//! the app is built inside, not things that dock, hide, or tab. The panel
//! registry stays unwired until there is something to register.
//!
//! Show order is load-bearing: egui panels claim space in the order they are
//! shown, and whatever is claimed first spans the full extent.
//!
//! Top bar first, so it runs the full width. Then the device panel, so it
//! runs the full width beneath everything. The browser comes third and fills
//! the column between them — which is what makes its height MATCH the
//! arrangement's instead of hanging below it.
//!
//! Claiming the browser before the device is the other arrangement (Ableton's:
//! browser full height, device inset to its right). It is one line, and
//! `the_browser_and_arrangement_are_the_same_height` is the test that would
//! catch the swap.

use daw::audio::graph::{GraphSpec, NodeId, NodeSpec, Note as SeqNote, SynthParams};
use daw::audio::transport::TransportCmd;
use daw::audio::{Engine, EngineConfig, StreamHealth};
use daw::install_fonts;
use daw::ui::action::UiAction;
use daw::ui::device;
use daw::ui::kit;
use daw::ui::palette::{Command as PaletteCommand, Palette};
use daw::ui::prefs::{STORAGE_KEY, UiPrefs};
use daw::ui::skin::Skin;
use daw::ui::theme::Theme;
use daw::ui::tokens::{Density, radius, stroke};
use daw::ui::vm::{TrackKind, limits};
use eframe::egui;
use std::time::Instant;

mod piano_roll;

/// UI zoom.
///
/// This machine's panel is 1920x1200 across 34x22cm — 142 DPI — and the
/// compositor runs it at scale 1, so egui's 96 DPI default renders every
/// point about 1.5x too small physically. Every dimension below is in
/// logical points and scales with this, so tune sizes here first and
/// individual constants second.
///
/// Machine-local, not a design value: a different display wants a different
/// number, which is what `daw::ui::prefs` is for once it is wired up.
const ZOOM: f32 = 1.75;

// --- the skeleton's geometry: starting guesses, all three live here ---

/// Height of the top bar.
const TOP_BAR_H: f32 = 36.0;
/// Width of the left browser region.
const BROWSER_W: f32 = 240.0;
/// Height of the bottom device region.
const DEVICE_H: f32 = 180.0;

/// The drag seams. `#4797f5` is the exact inversion of the ground tint: the
/// warm ramp sits at hue 32.5deg, so its complement is 212.5deg — brought up to
/// 90% saturation and 62% lightness so it reads as a mark against near-black.
const SEAM: egui::Color32 = egui::Color32::from_rgb(0x47, 0x97, 0xf5);
/// Thickness of that mark. A narrow rectangle, not a hairline — thin enough
/// to read as an edge, thick enough to see.
const SEAM_PX: f32 = 3.0;

// --- the browser's content area ---

/// Dark brown left showing beside the content area, so it reads as sitting
/// IN the panel rather than being the panel.
///
/// Horizontal and vertical are separate on purpose. A vertical inset makes
/// the browser's content shorter than the arrangement and pushes it down by
/// the inset — the two stop lining up, which is exactly the misalignment
/// that a uniform 16 caused. Keep the vertical at 0 unless the arrangement
/// gains a matching inset.
const BROWSER_INSET_X: f32 = 16.0;
const BROWSER_INSET_Y: f32 = 0.0;
/// Starting height of the lower band, as a fraction of the browser's FULL
/// height — the panel's, not the inset area's, so the split tracks the
/// region rather than the margin. The user drags it from here.
const BROWSER_LOWER_FRAC: f32 = 0.2;
/// How far the divider can be pulled. Both ends leave a usable band; letting
/// either collapse to nothing would hide a thing the user cannot then grab.
const BROWSER_SPLIT_RANGE: std::ops::RangeInclusive<f32> = 0.08..=0.60;

/// Height of the search bar.
const SEARCH_H: f32 = 28.0;
/// Padding inside the well, and the gap between icon and field.
const SEARCH_PAD: f32 = 8.0;
/// Nerd Font magnifying glass (nf-fa-search), verified present in the exact
/// file `install_fonts` loads.
const SEARCH_ICON: &str = "\u{f002}";
const SEARCH_TYPE: f32 = 12.0;

// --- transport ---

/// Transport icons are DRAWN, not typed.
///
/// `Align2::CENTER_CENTER` centres a glyph's advance box, not its ink, and
/// the Nerd Font media glyphs carry asymmetric side bearings — so a
/// correctly centred cell still puts the triangle off centre. Shapes are
/// centred by construction, and this is where the UI is headed anyway.
///
/// Side of the square the icon is inscribed in.
const ICON: f32 = 11.0;
/// Pause bar width, and the gap between the two bars.
const PAUSE_BAR: f32 = 3.0;
const PAUSE_GAP: f32 = 3.0;
/// The return icon's bar, and the gap between it and the triangle.
const RETURN_BAR: f32 = 2.5;
const RETURN_GAP: f32 = 1.5;
/// The stop square, deliberately smaller than `ICON`.
///
/// A square filling the same box as the play triangle carries roughly twice
/// the ink and reads as much heavier beside it. Shrinking it is an optical
/// correction, not a measurement — set it to `ICON` if you want them
/// geometrically equal instead.
const STOP_SIDE: f32 = 9.0;

/// Square hit area for one transport button.
const TRANSPORT_BTN: f32 = 26.0;
/// Gap from the bar's left edge.
const TRANSPORT_PAD: f32 = 10.0;
/// Gap between adjacent buttons.
const TRANSPORT_GAP: f32 = 4.0;
/// Space between GROUPS of controls. Groups are separated by air, not by
/// rules — this window is fills only, and a divider on the bar would be the
/// first line anywhere in it.
const TRANSPORT_GROUP_GAP: f32 = 18.0;

/// Sixteenths per beat — the readout's finest division.
const DIVISIONS: u64 = 4;

/// Editable fields: a recessed well with a value in it.
const FIELD_H: f32 = 20.0;
const FIELD_TYPE: f32 = 12.0;
const FIELD_RADIUS: f32 = 2.0;
const TEMPO_W: f32 = 54.0;
const TIMESIG_W: f32 = 22.0;
/// BPM per pixel of horizontal drag.
const TEMPO_PER_PX: f64 = 0.2;
/// Pixels of vertical drag per step of the time-signature numerator.
const TS_STEP_PX: f32 = 10.0;
/// Denominators the beat-unit field cycles through.
const BEAT_UNITS: [u32; 4] = [2, 4, 8, 16];
/// Largest numerator the field will reach.
const TS_NUM_MAX: u32 = 16;

/// The readouts. Not in wells: a well means editable, and these are not.
const READOUT_TYPE: f32 = 14.0;
const READOUT_W: f32 = 74.0;
const TIMECODE_W: f32 = 84.0;
/// The engine slot beside the timecode: DSP load and xruns while the stream
/// runs, "engine off" while it does not, a notice when something broke.
const ENGINE_W: f32 = 140.0;
/// How many characters of a notice the slot shows; hover carries the rest.
const NOTICE_CHARS: usize = 16;

// --- the engine, from the UI thread ---

/// How often note edits may recompile the schedule, in seconds of wall time.
/// Edits inside the window coalesce: the schedule is swapped whole once the
/// window has passed and the note set genuinely differs — never streamed
/// mid-drag (sequencing contract rule 4).
const RECOMPILE_MIN_SECS: f64 = 1.0;

// --- the browser tree ---

/// Disclosure and branch glyphs. All verified present in the exact file
/// `install_fonts` loads. Drawn in the MONOSPACE family so the branch
/// characters line up into a continuous rule down the tree.
const TREE_OPEN: &str = "\u{25bc}"; // filled triangle, down
const TREE_SHUT: &str = "\u{25b6}"; // filled triangle, right
const TREE_TEE: &str = "\u{251c}\u{2500}"; // branch
const TREE_ELL: &str = "\u{2514}\u{2500}"; // last branch

const TREE_TYPE: f32 = 12.0;
const TREE_ROW_H: f32 = 20.0;
/// Gap between the search well and the first row.
const TREE_TOP_GAP: f32 = 6.0;
const TREE_PAD_X: f32 = 8.0;

// --- the arrangement ---

/// The grid ladder, in beats. `Ctrl+1` walks down it (finer), `Ctrl+2` walks
/// up (coarser) — Ableton's Narrow Grid / Widen Grid, same direction.
const GRID_BEATS: [f32; 6] = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125];
const GRID_NAMES: [&str; 6] = ["1/1", "1/2", "1/4", "1/8", "1/16", "1/32"];
/// Start on quarter notes.
const GRID_DEFAULT: usize = 2;
/// The grid's name, shown in the arrangement's corner.
///
/// Not decoration: past a certain fineness the subdivision lines are dropped
/// (see `GRID_MIN_PX`), so without this, Ctrl+1 at the fine end changes the
/// grid and nothing on screen moves — the shortcut would look broken.
const GRID_LABEL_TYPE: f32 = 11.0;
const GRID_LABEL_PAD: f32 = 8.0;
/// Horizontal zoom.
const PX_PER_BEAT: f32 = 24.0;
/// Below this spacing subdivision lines stop being a grid and start being a
/// smear, so they are dropped and only beats and bars are drawn.
const GRID_MIN_PX: f32 = 5.0;

/// The loop ruler: a strip above the lanes where the loop brace lives. The
/// handles need somewhere to be grabbed that is not inside a track.
const LOOP_RULER_H: f32 = 14.0;
/// Width of a loop handle's hit area.
const LOOP_HANDLE_W: f32 = 7.0;

/// Clip chrome.
///
/// Width of a clip's edge strips — the grab zones for resizing. They exist
/// only on the selected clip, so resizing cannot be triggered by accident.
const CLIP_EDGE_W: f32 = 6.0;
/// A clip narrower than this draws no name label — text spilling past the
/// block is worse than none.
const CLIP_LABEL_MIN_W: f32 = 34.0;
const CLIP_LABEL_PAD: f32 = 4.0;
/// The shortest note bar drawn, whatever the pitch span: a note must read
/// as present, not as a hairline.
const NOTE_MIN_H: f32 = 3.0;

/// Playhead chrome: the ruler triangle. The line itself is a stroke like
/// the loop brace's, not a measurement of its own.
const PLAYHEAD_TRI_HALF: f32 = 4.0;
const PLAYHEAD_TRI_H: f32 = 6.0;

/// Track lanes.
const TRACK_H: f32 = 64.0;
const TRACK_H_RANGE: std::ops::RangeInclusive<f32> = 28.0..=240.0;
/// How many lanes a fresh session opens with. Lanes are furniture — the
/// clips on them are the content, and a new session has none.
const TRACK_COUNT: usize = 4;

// --- the track header column ---
/// Width of the header strip down the arrangement's left edge. Wide enough
/// for a name, a mute and solo pair, and a pan knob without any of them
/// touching.
const HEADER_W: f32 = 150.0;
const HEADER_PAD: f32 = 6.0;
/// The name row's height, at the top of every header.
const HEADER_NAME_H: f32 = 15.0;
const HEADER_NAME_TYPE: f32 = 12.0;
/// The kind badge ("midi" / "audio"), right-aligned beside the name.
const HEADER_KIND_TYPE: f32 = 9.0;
/// Side of the square M and S buttons.
const HEADER_BTN: f32 = 15.0;
const HEADER_BTN_TYPE: f32 = 10.0;
/// Diameter of the header's pan knob — smaller than `control::KNOB`,
/// because this one sits inside a lane rather than on a device card.
const HEADER_KNOB: f32 = 20.0;
/// Below this lane height the controls row is DROPPED, name only: half a
/// button is worse than no button, and a squeezed lane is a lane the user
/// is not currently working on.
const HEADER_ROWS_MIN_H: f32 = 44.0;
/// How far one keyboard pan step moves, in `-1..=1` units.
const PAN_STEP: f32 = 0.1;
/// Half-width of the pan knob's center detent: inside this, pan snaps to
/// exactly 0.0, so "back to the middle" is reachable by hand.
const PAN_DETENT: f32 = 0.04;

// --- the keyboard cursor ---

/// The cursor is a RING around the focused element, not a box floating in a
/// region. Once the keyboard can reach individual buttons, a marker sitting
/// in the middle of a panel cannot say WHICH button it means.
const RING_STROKE: f32 = 2.0;
/// How far the ring stands off the element, so it surrounds rather than
/// covers it.
const RING_PAD: f32 = 3.0;
const RING_RADIUS: f32 = 3.0;
/// Stiffness of the ring's travel, radians per second. Critically damped, so
/// it accelerates in and settles without overshoot.
const RING_OMEGA: f32 = 34.0;
/// Below this the spring is done and we stop asking for frames.
const RING_SETTLED_PX: f32 = 0.25;
/// How much a candidate is penalised for being off the axis you pressed.
/// Above 1.0 this prefers the element you are lined up with over one merely
/// closer — which is what makes arrowing along a row of buttons work.
const CROSS_PENALTY: f32 = 2.5;

/// Drag limits for the two resizable regions, so proportions can be found by
/// feel rather than by guessing numbers.
const BROWSER_W_RANGE: std::ops::RangeInclusive<f32> = 180.0..=560.0;
/// The rack's ceiling is exactly one card plus its breathing room: cards
/// are height-locked, so any taller region is dead space by construction.
const DEVICE_H_RANGE: std::ops::RangeInclusive<f32> =
    80.0..=(daw::ui::tokens::control::DEVICE_H + 2.0 * daw::ui::tokens::space::SM);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("daw")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native("daw", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    fn key(self) -> egui::Key {
        match self {
            Self::Up => egui::Key::ArrowUp,
            Self::Down => egui::Key::ArrowDown,
            Self::Left => egui::Key::ArrowLeft,
            Self::Right => egui::Key::ArrowRight,
        }
    }
}

/// One step of a critically damped spring, solved implicitly.
///
/// Implicit rather than the obvious `vel += accel * dt`, because the explicit
/// form blows up when a frame runs long — exactly when a dropped frame would
/// otherwise fling the cursor off screen. This form is unconditionally
/// stable at any `dt`, and being critically damped it never overshoots, so
/// the box arrives without a wobble.
///
/// Pure, so `the_cursor_springs_without_overshooting` can check it with no
/// window in sight.
fn spring_step(
    pos: egui::Vec2,
    vel: egui::Vec2,
    target: egui::Vec2,
    dt: f32,
) -> (egui::Vec2, egui::Vec2) {
    let omega = RING_OMEGA;
    let f = 1.0 + 2.0 * dt * omega;
    let oo = omega * omega;
    let hoo = dt * oo;
    let hhoo = dt * hoo;
    let det_inv = 1.0 / (f + hhoo);
    let det_x = f * pos + dt * vel + hhoo * target;
    let det_v = vel + hoo * (target - pos);
    (det_x * det_inv, det_v * det_inv)
}

/// Split the browser's interior into the two bands, inset so the panel's own
/// dark brown shows around them.
///
/// Returns `None` when the panel is too small to hold both bands — dragged to
/// its minimum with a short window, the inset alone can exceed the height,
/// and half-drawn bands are worse than none.
///
/// Pure, so `the_browser_bands_split_and_degrade_cleanly` can check the
/// arithmetic without a window.
fn browser_bands(area: egui::Rect, split: f32) -> Option<(egui::Rect, egui::Rect)> {
    let inner = egui::Rect::from_min_max(
        egui::pos2(area.left() + BROWSER_INSET_X, area.top() + BROWSER_INSET_Y),
        egui::pos2(
            area.right() - BROWSER_INSET_X,
            area.bottom() - BROWSER_INSET_Y,
        ),
    );
    let lower_h = area.height() * split;
    if inner.width() <= 0.0 || inner.height() <= lower_h {
        return None;
    }
    let seam_y = inner.bottom() - lower_h;
    Some((
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), seam_y)),
        egui::Rect::from_min_max(egui::pos2(inner.left(), seam_y), inner.max),
    ))
}

/// The split a pointer at `y` is asking for, given the panel's `area`.
///
/// Inverse of the `seam_y` arithmetic in `browser_bands`, clamped. Pure, so
/// the drag maths is checkable without dragging anything.
fn split_from_pointer(area: egui::Rect, y: f32) -> f32 {
    let seam_top = area.bottom() - BROWSER_INSET_Y;
    let frac = (seam_top - y) / area.height().max(1.0);
    frac.clamp(*BROWSER_SPLIT_RANGE.start(), *BROWSER_SPLIT_RANGE.end())
}

/// Which element the keyboard is on, and where the ring is on its way there.
///
/// Elements register themselves as they are drawn, so the focusable set is
/// exactly the set of things that exist this frame. A control that is not
/// drawn cannot be focused, and one that is drawn cannot be missed.
#[derive(Default)]
struct Focus {
    at: Option<egui::Id>,
    items: Vec<(egui::Id, egui::Rect)>,
    /// The direction pressed this frame, resolved only once every element has
    /// registered — moving mid-frame would navigate a half-built list.
    pending: Option<Dir>,
    activate: bool,
    ring: Option<egui::Rect>,
    v_min: egui::Vec2,
    v_max: egui::Vec2,
}

impl Focus {
    /// Take this frame's keyboard input and start collecting elements.
    fn begin(&mut self, ctx: &egui::Context) {
        self.items.clear();
        self.pending = None;
        self.activate = false;

        // A focused text field owns the keyboard outright. Escape hands it
        // back — without that you would be stuck inside the search box.
        if ctx.egui_wants_keyboard_input() {
            let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if escape && let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            return;
        }
        ctx.input_mut(|i| {
            for dir in Dir::ALL {
                if i.consume_key(egui::Modifiers::NONE, dir.key()) {
                    self.pending = Some(dir);
                }
            }
            self.activate = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        });
    }

    /// Declare an element focusable. Returns true if the keyboard is on it.
    fn register(&mut self, id: egui::Id, rect: egui::Rect) -> bool {
        self.items.push((id, rect));
        self.at == Some(id)
    }

    /// Focused AND the user pressed Enter.
    fn activated(&self, id: egui::Id) -> bool {
        self.activate && self.at == Some(id)
    }

    /// Resolve movement, then ease the ring toward the focused element.
    fn end(&mut self, ui: &egui::Ui, theme: &Theme) {
        if self.items.is_empty() {
            return;
        }
        // Land somewhere on the first frame, and recover if whatever was
        // focused stopped being drawn — a folder collapsing under it, say.
        if !self.items.iter().any(|(id, _)| Some(*id) == self.at) {
            self.at = self.items.first().map(|(id, _)| *id);
            self.ring = None;
        }
        if let (Some(dir), Some(from)) = (self.pending, self.rect_of(self.at))
            && let Some(next) = nearest(from, dir, &self.items, self.at)
        {
            self.at = Some(next);
        }

        let Some(target) = self.rect_of(self.at) else {
            return;
        };
        let target = target.expand(RING_PAD);
        let ring = match self.ring {
            None => target,
            Some(ring) => {
                let dt = ui.ctx().input(|i| i.stable_dt);
                let (min, v_min) =
                    spring_step(ring.min.to_vec2(), self.v_min, target.min.to_vec2(), dt);
                let (max, v_max) =
                    spring_step(ring.max.to_vec2(), self.v_max, target.max.to_vec2(), dt);
                let next = egui::Rect::from_min_max(min.to_pos2(), max.to_pos2());
                let settled = (next.min - target.min).length() < RING_SETTLED_PX
                    && (next.max - target.max).length() < RING_SETTLED_PX;
                if settled {
                    self.v_min = egui::Vec2::ZERO;
                    self.v_max = egui::Vec2::ZERO;
                    target
                } else {
                    self.v_min = v_min;
                    self.v_max = v_max;
                    ui.ctx().request_repaint();
                    next
                }
            }
        };
        self.ring = Some(ring);

        ui.painter().rect_stroke(
            ring,
            RING_RADIUS,
            egui::Stroke::new(RING_STROKE, theme.focus),
            egui::StrokeKind::Middle,
        );
    }

    fn rect_of(&self, id: Option<egui::Id>) -> Option<egui::Rect> {
        self.items
            .iter()
            .find(|(i, _)| Some(*i) == id)
            .map(|(_, r)| *r)
    }
}

/// The arrangement's shortcuts. Ctrl+1 narrows the grid, Ctrl+2 widens it,
/// Ctrl+L loops the selection — Ableton's bindings and Ableton's directions,
/// so the muscle memory transfers.
///
/// Stands down while a text field has the keyboard, for the same reason the
/// arrows do: typing in the search box should not reshape the arrangement.
fn arrangement_keys(ctx: &egui::Context, arr: &Arrangement, out: &mut Vec<UiAction>) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // While the ring is on a cell, Left and Right walk the grid instead of
    // leaving the arrangement. At beat 0 Left is NOT claimed, so there is
    // always a way back out to the browser.
    if arr.owns_arrows {
        let at_start = arr.cursor.map(|c| c.1).unwrap_or(0.0) <= 0.0;
        ctx.input_mut(|i| {
            // SHIFT FIRST, and this order is not cosmetic. `consume_key`
            // matches modifiers LOGICALLY, which means it ignores an extra
            // Shift — so a plain-arrow check placed first happily swallows
            // Shift+Right and moves the cursor instead of extending. egui's
            // own docs say to match the most specific shortcut first; this
            // is what that warning is about.
            //
            // Shift is claimed even at beat 0, unlike plain Left: extending
            // is not a navigation gesture, so there is nothing to fall
            // through to.
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft) {
                out.push(UiAction::ExtendCell(-1));
            } else if i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight) {
                out.push(UiAction::ExtendCell(1));
            } else if !at_start && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                out.push(UiAction::MoveCell(-1));
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                out.push(UiAction::MoveCell(1));
            }
        });
    }
    ctx.input_mut(|i| {
        // Delete and Backspace both dispose the selected clip — whichever
        // key the user thinks of as "delete" is the one that works.
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        {
            out.push(UiAction::DeleteSelected);
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num1) {
            out.push(UiAction::NarrowGrid);
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Num2) {
            out.push(UiAction::WidenGrid);
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::L) {
            out.push(UiAction::LoopFromSelection);
        }
        // The clipboard verbs, everywhere a clip can be selected.
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::C) {
            out.push(UiAction::CopyClip);
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::V) {
            out.push(UiAction::PasteClip);
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::D) {
            out.push(UiAction::DuplicateClip);
        }
        // SHIFT FIRST, for the same reason as the arrows above:
        // `consume_key` ignores an extra Shift, so a plain Ctrl+T check
        // placed first would swallow Ctrl+Shift+T and make the wrong kind
        // of track. The keymap table carries the same pair in the same
        // order, and `shift_specific_track_gesture_wins` pins it there.
        if i.consume_key(
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            egui::Key::T,
        ) {
            out.push(UiAction::AddTrack(TrackKind::Midi));
        } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::T) {
            out.push(UiAction::AddTrack(TrackKind::Audio));
        }
    });
}

/// The element an arrow key should land on.
///
/// Candidates must lie genuinely in the pressed direction; among those the
/// winner is closest along that axis, penalised for being off it. That
/// penalty is what makes a row of buttons arrow left-to-right instead of
/// diving at whatever is nearest in a straight line.
///
/// Pure, so the whole navigation model is testable without a window.
fn nearest(
    from: egui::Rect,
    dir: Dir,
    items: &[(egui::Id, egui::Rect)],
    current: Option<egui::Id>,
) -> Option<egui::Id> {
    let a = from.center();
    let mut best: Option<(egui::Id, f32)> = None;
    for (id, rect) in items {
        if Some(*id) == current {
            continue;
        }
        let b = rect.center();
        let (along, across) = match dir {
            Dir::Left => (a.x - b.x, (b.y - a.y).abs()),
            Dir::Right => (b.x - a.x, (b.y - a.y).abs()),
            Dir::Up => (a.y - b.y, (b.x - a.x).abs()),
            Dir::Down => (b.y - a.y, (b.x - a.x).abs()),
        };
        if along <= 0.5 {
            continue; // not actually that way
        }
        let score = along + across * CROSS_PENALTY;
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((*id, score));
        }
    }
    best.map(|(id, _)| id)
}

/// Position as bar.beat.sixteenth, all 1-indexed the way a musician counts.
///
/// Pure, and the only place seconds become musical time. Takes `bpm` and the
/// bar length rather than reading the constants, so the tempo and time
/// signature controls feed it without touching the arithmetic.
fn bars_beats(seconds: f64, bpm: f64, beats_per_bar: u64) -> (u64, u64, u64) {
    let beats = (seconds.max(0.0) * bpm / 60.0).max(0.0);
    let per_bar = beats_per_bar.max(1) * DIVISIONS;
    let total = (beats * DIVISIONS as f64).floor() as u64;
    let (bar, rest) = (total / per_bar, total % per_bar);
    (bar + 1, rest / DIVISIONS + 1, rest % DIVISIONS + 1)
}

/// The readout's text. Fixed field widths so digits do not shuffle sideways
/// as the playhead rolls — the whole reason this is monospace.
fn format_position(seconds: f64, bpm: f64, beats_per_bar: u64) -> String {
    let (bar, beat, sixteenth) = bars_beats(seconds, bpm, beats_per_bar);
    format!("{bar:>4}.{beat:>2}.{sixteenth}")
}

/// Wall-clock position, `m:ss.mmm`. Fixed width for the same reason.
fn format_timecode(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let minutes = (s / 60.0).floor();
    format!("{:>3}:{:06.3}", minutes as u64, s - minutes * 60.0)
}

/// Move the whole loop by `delta` beats, never before beat 0, length
/// untouched. The handles change the length; the body only carries it.
///
/// Pure, so the brace-body drag is checkable without a mouse.
fn loop_move(range: (f32, f32), delta: f32) -> (f32, f32) {
    let len = range.1 - range.0;
    let start = (range.0 + delta).max(0.0);
    (start, start + len)
}

/// Where the view sits after follow has had its say. Follow off means hands
/// off. On: when the playhead crosses the right edge, the view jumps a page
/// so the playhead lands at the left; when the playhead falls behind the
/// view (Return, Stop), the view jumps back to it. Between edges the view
/// stays put — a continuously chasing view would be unreadable.
///
/// Pure, so the page logic is checkable without a window.
fn follow_view(offset: f32, playhead: f32, view_beats: f32, follow: bool) -> f32 {
    if !follow {
        return offset;
    }
    if playhead >= offset + view_beats || playhead < offset {
        playhead
    } else {
        offset
    }
}

/// Wrap the stand-in clock around the loop region. The loop's unit is the
/// beat, so the wrap is computed in beats and converted back to seconds.
///
/// This is a STAND-IN, like the clock it feeds: the engine's transport owns
/// sample-accurate loop points. When the engine is wired into the app, this
/// function and the clock both die.
fn wrap_loop(seconds: f64, bpm: f64, from_beats: f32, to_beats: f32) -> f64 {
    let beats = seconds * bpm / 60.0;
    let len = (to_beats - from_beats) as f64;
    if len <= 0.0 || beats < to_beats as f64 {
        return seconds;
    }
    let wrapped = from_beats as f64 + (beats - from_beats as f64) % len;
    wrapped * 60.0 / bpm
}

/// What a transport button draws. Adding a control is a variant here plus a
/// slot on the bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Icon {
    Return,
    Play,
    Pause,
    Stop,
    Record,
    Loop,
    Metronome,
    Follow,
    Power,
}

/// The power symbol: an arc-broken circle with a bar through the gap —
/// drawn as a full circle stroke plus the bar, which reads the same at 11px.
fn power_icon(rect: egui::Rect) -> (egui::Pos2, f32, [egui::Pos2; 2]) {
    let c = rect.center();
    let r = ICON * 0.5 * 0.9;
    (
        c,
        r,
        [egui::pos2(c.x, c.y - ICON * 0.5), egui::pos2(c.x, c.y)],
    )
}

/// The return icon: a bar against the left edge with a left-pointing
/// triangle beside it, together spanning `ICON` and centred on `rect`.
fn return_icon(rect: egui::Rect) -> (egui::Rect, [egui::Pos2; 3]) {
    let c = rect.center();
    let h = ICON * 0.5;
    let bar = egui::Rect::from_min_max(
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h + RETURN_BAR, c.y + h),
    );
    let apex = bar.right() + RETURN_GAP;
    (
        bar,
        [
            egui::pos2(c.x + h, c.y - h),
            egui::pos2(c.x + h, c.y + h),
            egui::pos2(apex, c.y),
        ],
    )
}

/// The record dot, centred on `rect`.
fn record_icon(rect: egui::Rect) -> (egui::Pos2, f32) {
    (rect.center(), ICON * 0.5 * 0.92)
}

/// The loop icon: a rounded track with an arrowhead riding its top edge.
fn loop_icon(rect: egui::Rect) -> (egui::Rect, [egui::Pos2; 3]) {
    let c = rect.center();
    let (w, h) = (ICON * 0.5, ICON * 0.36);
    let track = egui::Rect::from_center_size(c, egui::vec2(w * 2.0, h * 2.0));
    let tip = egui::pos2(track.right(), track.top());
    (
        track,
        [
            egui::pos2(tip.x - ICON * 0.26, tip.y - ICON * 0.20),
            egui::pos2(tip.x - ICON * 0.26, tip.y + ICON * 0.20),
            egui::pos2(tip.x + ICON * 0.12, tip.y),
        ],
    )
}

/// The metronome: a tapered body with the pendulum swung right.
fn metronome_icon(rect: egui::Rect) -> ([egui::Pos2; 3], [egui::Pos2; 2]) {
    let c = rect.center();
    let h = ICON * 0.5;
    (
        [
            egui::pos2(c.x - h * 0.78, c.y + h),
            egui::pos2(c.x + h * 0.78, c.y + h),
            egui::pos2(c.x, c.y - h),
        ],
        [
            egui::pos2(c.x, c.y + h * 0.55),
            egui::pos2(c.x + h * 0.62, c.y - h * 0.45),
        ],
    )
}

/// Follow: a playhead with the view chasing it rightwards.
fn follow_icon(rect: egui::Rect) -> (egui::Rect, [[egui::Pos2; 2]; 2]) {
    let c = rect.center();
    let h = ICON * 0.5;
    let bar = egui::Rect::from_min_max(
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h + RETURN_BAR, c.y + h),
    );
    let tip = egui::pos2(c.x + h, c.y);
    (
        bar,
        [
            [egui::pos2(tip.x - h * 0.7, c.y - h * 0.7), tip],
            [egui::pos2(tip.x - h * 0.7, c.y + h * 0.7), tip],
        ],
    )
}

/// The stop square, centred on `rect`.
fn stop_icon(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(STOP_SIDE))
}

/// The play triangle: right-pointing, inscribed in an `ICON`-square centred
/// on `rect`. Returned as points so the centring is checkable.
fn play_icon(rect: egui::Rect) -> [egui::Pos2; 3] {
    let c = rect.center();
    let h = ICON * 0.5;
    [
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h, c.y + h),
        egui::pos2(c.x + h, c.y),
    ]
}

/// The pause bars: two `PAUSE_BAR`-wide bars either side of `rect`'s centre.
fn pause_icon(rect: egui::Rect) -> [egui::Rect; 2] {
    let c = rect.center();
    let h = ICON * 0.5;
    let inner = PAUSE_GAP * 0.5;
    [
        egui::Rect::from_min_max(
            egui::pos2(c.x - inner - PAUSE_BAR, c.y - h),
            egui::pos2(c.x - inner, c.y + h),
        ),
        egui::Rect::from_min_max(
            egui::pos2(c.x + inner, c.y - h),
            egui::pos2(c.x + inner + PAUSE_BAR, c.y + h),
        ),
    ]
}

/// Lays the bar out left to right, everything vertically centred.
///
/// A cursor rather than indexed slots, because the bar mixes square buttons,
/// wider fields and readouts, and groups separated by air. Pure — it never
/// touches a `Ui` — so the whole rhythm is checkable.
struct Bar {
    x: f32,
    mid: f32,
}

impl Bar {
    /// A cursor starting at an arbitrary x, for anchoring a group to the
    /// centre or the right edge instead of running everything off the left.
    fn at(area: egui::Rect, x: f32) -> Self {
        Self {
            x,
            mid: area.center().y,
        }
    }

    fn button(&mut self) -> egui::Rect {
        let rect = egui::Rect::from_center_size(
            egui::pos2(self.x + TRANSPORT_BTN * 0.5, self.mid),
            egui::Vec2::splat(TRANSPORT_BTN),
        );
        self.x += TRANSPORT_BTN + TRANSPORT_GAP;
        rect
    }

    fn field(&mut self, width: f32) -> egui::Rect {
        let rect = egui::Rect::from_min_size(
            egui::pos2(self.x, self.mid - FIELD_H * 0.5),
            egui::vec2(width, FIELD_H),
        );
        self.x += width + TRANSPORT_GAP;
        rect
    }

    /// Air between groups, in place of a divider.
    fn group(&mut self) {
        self.x += TRANSPORT_GROUP_GAP - TRANSPORT_GAP;
    }
}

/// Width of `n` buttons laid in a row, gaps included.
fn buttons_width(n: usize) -> f32 {
    n as f32 * TRANSPORT_BTN + n.saturating_sub(1) as f32 * TRANSPORT_GAP
}

/// Width of a run of fields, gaps included.
fn fields_width(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * TRANSPORT_GAP
}

/// Where each group starts, given the bar's width.
///
/// Three anchors: the verbs hold the left edge so muscle memory has somewhere
/// fixed to aim, the readouts sit dead centre because they are what you look
/// at, and the settings hold the right edge. Each anchor is stable under
/// resize — the middle stays middle, the ends stay at their ends.
///
/// If the three would collide, everything falls back to packed-left in the
/// same order. Pure, so both branches are checkable.
fn bar_layout(area: egui::Rect, verbs: f32, centre: f32, right: f32) -> (f32, f32, f32, bool) {
    let left_x = area.left() + TRANSPORT_PAD;
    let left_end = left_x + verbs;
    let right_x = area.right() - TRANSPORT_PAD - right;
    let centre_x = area.center().x - centre * 0.5;

    let spread = centre_x > left_end + TRANSPORT_GROUP_GAP
        && centre_x + centre < right_x - TRANSPORT_GROUP_GAP;

    if spread {
        (left_x, centre_x, right_x, true)
    } else {
        let centre_x = left_end + TRANSPORT_GROUP_GAP;
        (
            left_x,
            centre_x,
            centre_x + centre + TRANSPORT_GROUP_GAP,
            false,
        )
    }
}

/// One transport button: hover wash, icon, click. Returns true when pressed,
/// by mouse or by keyboard.
fn transport_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    rect: egui::Rect,
    id: &'static str,
    icon: Icon,
    colour: egui::Color32,
) -> bool {
    let wid = ui.id().with(id);
    focus.register(wid, rect);
    let response = ui.interact(rect, wid, egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme.accent_muted);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    match icon {
        Icon::Return => {
            let (bar, tri) = return_icon(rect);
            painter.rect_filled(bar, 0.0, colour);
            painter.add(egui::Shape::convex_polygon(
                tri.to_vec(),
                colour,
                egui::Stroke::NONE,
            ));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                play_icon(rect).to_vec(),
                colour,
                egui::Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for bar in pause_icon(rect) {
                painter.rect_filled(bar, 0.0, colour);
            }
        }
        Icon::Stop => {
            painter.rect_filled(stop_icon(rect), 0.0, colour);
        }
        Icon::Record => {
            let (centre, radius) = record_icon(rect);
            painter.circle_filled(centre, radius, colour);
        }
        Icon::Loop => {
            let (track, head) = loop_icon(rect);
            painter.rect_stroke(
                track,
                track.height() * 0.5,
                egui::Stroke::new(1.5, colour),
                egui::StrokeKind::Middle,
            );
            painter.add(egui::Shape::convex_polygon(
                head.to_vec(),
                colour,
                egui::Stroke::NONE,
            ));
        }
        Icon::Metronome => {
            let (body, pendulum) = metronome_icon(rect);
            painter.add(egui::Shape::convex_polygon(
                body.to_vec(),
                colour,
                egui::Stroke::NONE,
            ));
            // The swung arm sits one step back from the body, so a lit
            // metronome reads as a shape in front of its own shadow.
            painter.line_segment(pendulum, egui::Stroke::new(1.5, theme.text_muted));
        }
        Icon::Follow => {
            let (bar, chevron) = follow_icon(rect);
            painter.rect_filled(bar, 0.0, colour);
            for seg in chevron {
                painter.line_segment(seg, egui::Stroke::new(1.5, colour));
            }
        }
        Icon::Power => {
            let (centre, radius, bar) = power_icon(rect);
            painter.circle_stroke(centre, radius, egui::Stroke::new(1.5, colour));
            painter.line_segment(bar, egui::Stroke::new(1.5, colour));
        }
    }
    // Mouse and keyboard are the same press. The button does not care which.
    response.clicked() || focus.activated(wid)
}

/// The ink a plain toggle takes: lit when on, resting when off.
fn toggle_ink(theme: &Theme, on: bool) -> egui::Color32 {
    if on { theme.text } else { theme.text_muted }
}

/// A recessed, draggable value field. Returns its response so the caller
/// decides what a drag means — the field knows how to look, not what it is.
fn field(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    rect: egui::Rect,
    id: &'static str,
    text: &str,
) -> egui::Response {
    let wid = ui.id().with(id);
    focus.register(wid, rect);
    let response = ui.interact(rect, wid, egui::Sense::click_and_drag());
    let live = response.hovered() || response.dragged();
    ui.painter().rect_filled(
        rect,
        FIELD_RADIUS,
        if live {
            theme.accent_muted
        } else {
            theme.surface_sunken
        },
    );
    if live {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::new(FIELD_TYPE, egui::FontFamily::Monospace),
        theme.text,
    );
    response
}

/// A readout. Plain, never in a well: a well means editable, and these are
/// not.
fn readout(ui: &mut egui::Ui, rect: egui::Rect, text: &str, colour: egui::Color32) {
    ui.painter().text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::new(READOUT_TYPE, egui::FontFamily::Monospace),
        colour,
    );
}

/// Live engine numbers for the top bar's slot. Percent of the block budget
/// spent, and xruns since the stream started.
#[derive(Clone, Copy)]
struct EngineHud {
    load_pct: f32,
    xruns: u64,
}

/// What the top bar knows about the engine: whether it runs, its numbers,
/// and any notice (start failure, stream death). Read-only, like `Transport`.
#[derive(Clone, Copy)]
struct EngineView<'a> {
    on: bool,
    hud: Option<EngineHud>,
    notice: Option<&'a str>,
}

/// Truncate a notice to the bar's fixed slot. Chars, not bytes — an error
/// message carries arbitrary text.
fn ellipsize(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The top bar's contents. Emits wishes; it does not act on them.
fn top_bar_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    t: &Transport,
    ev: EngineView<'_>,
    out: &mut Vec<UiAction>,
) {
    let area = ui.max_rect();
    claim(ui);

    let verbs_w = buttons_width(5);
    let centre_w = fields_width(&[READOUT_W, TIMECODE_W, ENGINE_W]);
    let right_w =
        buttons_width(4) + TRANSPORT_GROUP_GAP + fields_width(&[TEMPO_W, TIMESIG_W, TIMESIG_W]);
    let (verbs_x, centre_x, right_x, _) = bar_layout(area, verbs_w, centre_w, right_w);

    // --- verbs, holding the left edge -------------------------------------
    let mut bar = Bar::at(area, verbs_x);
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "return",
        Icon::Return,
        theme.text_muted,
    ) {
        out.push(UiAction::Return);
    }
    // Play is the only one of these that is a state rather than a verb, so it
    // is the only one that lights.
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "play",
        Icon::Play,
        toggle_ink(theme, t.playing),
    ) {
        out.push(UiAction::TogglePlay);
    }
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "pause",
        Icon::Pause,
        theme.text_muted,
    ) {
        out.push(UiAction::Pause);
    }
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "stop",
        Icon::Stop,
        theme.text_muted,
    ) {
        out.push(UiAction::Stop);
    }
    // Record is the exception to "ink brightens when on": armed is a warning,
    // not an emphasis, so it takes the red outright.
    let record_ink = match (t.armed, t.recording()) {
        (_, true) => theme.red_zone,
        (true, _) => theme.danger,
        _ => theme.text_muted,
    };
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "record",
        Icon::Record,
        record_ink,
    ) {
        out.push(UiAction::ToggleRecord);
    }

    // --- where we are, dead centre ----------------------------------------
    let mut bar = Bar::at(area, centre_x);
    readout(
        ui,
        bar.field(READOUT_W),
        &format_position(t.position, t.bpm, u64::from(t.beats_per_bar)),
        if t.recording() {
            theme.red_zone
        } else {
            theme.text
        },
    );
    readout(
        ui,
        bar.field(TIMECODE_W),
        &format_timecode(t.position),
        theme.text_muted,
    );
    // The engine slot: a notice outranks the numbers — a dead stream makes
    // every other number stale. Hover carries the full text.
    let engine_rect = bar.field(ENGINE_W);
    match (ev.notice, ev.hud) {
        (Some(notice), _) => {
            readout(
                ui,
                engine_rect,
                &ellipsize(notice, NOTICE_CHARS),
                theme.red_zone,
            );
            ui.interact(
                engine_rect,
                ui.id().with("engine_notice"),
                egui::Sense::hover(),
            )
            .on_hover_text(notice);
        }
        (None, Some(hud)) => readout(
            ui,
            engine_rect,
            &format!("dsp {:4.1}% xr {}", hud.load_pct, hud.xruns),
            if hud.xruns == 0 {
                theme.text_muted
            } else {
                theme.danger
            },
        ),
        (None, None) => readout(ui, engine_rect, "engine off", theme.text_muted),
    }

    // --- settings, holding the right edge ---------------------------------
    let mut bar = Bar::at(area, right_x);
    // The power switch, apart from the transport verbs: this opens and
    // closes the audio device, it does not move the playhead.
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "power",
        Icon::Power,
        toggle_ink(theme, ev.on),
    ) {
        out.push(if ev.on {
            UiAction::StopEngine
        } else {
            UiAction::StartEngine
        });
    }
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "loop",
        Icon::Loop,
        toggle_ink(theme, t.loop_on),
    ) {
        out.push(UiAction::ToggleLoop);
    }
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "metro",
        Icon::Metronome,
        toggle_ink(theme, t.metronome),
    ) {
        out.push(UiAction::ToggleMetronome);
    }
    if transport_button(
        ui,
        theme,
        focus,
        bar.button(),
        "follow",
        Icon::Follow,
        toggle_ink(theme, t.follow),
    ) {
        out.push(UiAction::ToggleFollow);
    }

    bar.group();
    let tempo = field(
        ui,
        theme,
        focus,
        bar.field(TEMPO_W),
        "tempo",
        &format!("{:.2}", t.bpm),
    );
    if tempo.dragged() {
        let delta = f64::from(tempo.drag_delta().x) * TEMPO_PER_PX;
        out.push(UiAction::SetTempo(
            (t.bpm + delta).clamp(limits::BPM_MIN, limits::BPM_MAX),
        ));
    }

    // The numerator drags, the denominator cycles. Dragging through powers of
    // two would be a lie about what values exist.
    let num = field(
        ui,
        theme,
        focus,
        bar.field(TIMESIG_W),
        "ts_num",
        &t.beats_per_bar.to_string(),
    );
    // Integer fields need an accumulator. A 2-3px frame delta divided by the
    // step size truncates to zero every single frame, so without this the
    // field looks draggable and simply never moves.
    let acc_id = ui.id().with("ts_acc");
    let mut acc: f32 = ui.ctx().data(|d| d.get_temp(acc_id).unwrap_or(0.0));
    if num.dragged() {
        // Screen y grows downward; dragging UP should raise the count.
        acc -= num.drag_delta().y;
    }
    if num.drag_stopped() {
        acc = 0.0;
    }
    let mut steps = 0i32;
    while acc >= TS_STEP_PX {
        steps += 1;
        acc -= TS_STEP_PX;
    }
    while acc <= -TS_STEP_PX {
        steps -= 1;
        acc += TS_STEP_PX;
    }
    ui.ctx().data_mut(|d| d.insert_temp(acc_id, acc));

    ui.painter().text(
        egui::pos2(bar.x - TRANSPORT_GAP * 0.5, bar.mid),
        egui::Align2::CENTER_CENTER,
        "/",
        egui::FontId::new(FIELD_TYPE, egui::FontFamily::Monospace),
        theme.text_muted,
    );
    let den = field(
        ui,
        theme,
        focus,
        bar.field(TIMESIG_W),
        "ts_den",
        &t.beat_unit.to_string(),
    );
    if steps != 0 || den.clicked() {
        let unit = if den.clicked() {
            let i = BEAT_UNITS
                .iter()
                .position(|u| *u == t.beat_unit)
                .unwrap_or(1);
            BEAT_UNITS[(i + 1) % BEAT_UNITS.len()]
        } else {
            t.beat_unit
        };
        let top = (t.beats_per_bar as i32 + steps).clamp(1, TS_NUM_MAX as i32) as u32;
        out.push(UiAction::SetTimeSignature(top, unit));
    }
}

/// The app's transport, with no engine behind it.
///
/// `position` is in seconds and is advanced from frame time while rolling.
/// That is a stand-in, not a clock: it drifts with the repaint rate and knows
/// nothing about sample counts. It exists so Return has somewhere to return
/// TO, and so Pause and Stop are distinguishable. The engine's sample-count
/// master clock replaces it wholesale.
struct Transport {
    playing: bool,
    position: f64,
    bpm: f64,
    /// Time signature: beats per bar over the note value that gets the beat.
    beats_per_bar: u32,
    beat_unit: u32,
    /// Armed for recording. ROLLING is `armed && playing` — derived, never
    /// set, so the two can never disagree.
    armed: bool,
    loop_on: bool,
    metronome: bool,
    follow: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            position: 0.0,
            bpm: 120.0,
            beats_per_bar: 4,
            beat_unit: 4,
            armed: false,
            loop_on: false,
            metronome: false,
            follow: true,
        }
    }
}

impl Transport {
    /// Recording happens only while armed AND rolling.
    fn recording(&self) -> bool {
        self.armed && self.playing
    }
}

/// The translation step: where UI wishes become app state.
///
/// Pure over UI state on purpose: engine side effects live in
/// `App::route_transport` and friends, so this stays headless-testable and
/// is the whole transport when the engine is off.
fn perform(actions: &[UiAction], transport: &mut Transport, arrangement: &mut Arrangement) {
    for action in actions {
        match action {
            UiAction::TogglePlay => transport.playing = !transport.playing,
            UiAction::SetTempo(bpm) => {
                transport.bpm = bpm.clamp(limits::BPM_MIN, limits::BPM_MAX);
            }
            UiAction::SetTimeSignature(top, unit) => {
                transport.beats_per_bar = (*top).clamp(1, TS_NUM_MAX);
                transport.beat_unit = *unit;
            }
            UiAction::ToggleRecord => transport.armed = !transport.armed,
            UiAction::ToggleLoop => transport.loop_on = !transport.loop_on,
            UiAction::ToggleMetronome => transport.metronome = !transport.metronome,
            UiAction::ToggleFollow => transport.follow = !transport.follow,
            // Halt, hold where you are.
            UiAction::Pause => transport.playing = false,
            // Halt AND rewind — pause and return in one press.
            UiAction::Stop => {
                transport.playing = false;
                transport.position = 0.0;
            }
            // Rewind without touching whether we are rolling.
            UiAction::Return => transport.position = 0.0,
            UiAction::NarrowGrid => arrangement.grid = step_grid(arrangement.grid, true),
            UiAction::WidenGrid => arrangement.grid = step_grid(arrangement.grid, false),
            // Looping the selection also turns looping ON. Creating a loop
            // and having to reach for the toggle would be a step nobody
            // wants; the gesture says what was meant.
            UiAction::MoveCell(delta) => {
                let grid = arrangement.grid_beats();
                let (track, beat) = arrangement.cursor.unwrap_or((0, 0.0));
                let beat = (beat + *delta as f32 * grid).max(0.0);
                arrangement.cursor = Some((track, beat));
                // Plain movement drops the anchor here, collapsing the
                // selection to the one cell under the cursor.
                arrangement.anchor = beat;
                arrangement.selected = Some(track);
                arrangement.selection = Some(span(beat, beat, grid));
            }
            UiAction::ExtendCell(delta) => {
                let grid = arrangement.grid_beats();
                let (track, beat) = arrangement.cursor.unwrap_or((0, 0.0));
                let beat = (beat + *delta as f32 * grid).max(0.0);
                arrangement.cursor = Some((track, beat));
                arrangement.selected = Some(track);
                arrangement.selection = Some(extended(arrangement.anchor, beat, grid));
            }
            UiAction::LoopFromSelection => {
                if let Some(range) = arrangement.selection {
                    arrangement.loop_range = Some(range);
                    transport.loop_on = true;
                }
            }
            UiAction::DeleteSelected => {
                if let Some((t, i)) = arrangement.selected_clip {
                    arrangement.clips[t].remove(i);
                    arrangement.selected_clip = None;
                }
            }
            UiAction::CopyClip => arrangement.copy_selected(),
            UiAction::PasteClip => {
                let track = arrangement.paste_track();
                let playhead = (transport.position * transport.bpm / 60.0) as f32;
                arrangement.paste_clipboard(track, arrangement.paste_beat(playhead));
            }
            UiAction::DuplicateClip => {
                arrangement.duplicate_selected();
            }

            // --- tracks. Every one of these acts on the SELECTED track,
            // so the palette, the keyboard and the header buttons all
            // agree on which lane they mean.
            UiAction::AddTrack(kind) => {
                arrangement.add_track(*kind);
            }
            UiAction::RemoveTrack => {
                if let Some(t) = arrangement.active_track() {
                    arrangement.remove_track(t);
                }
            }
            UiAction::ToggleTrackMute => {
                if let Some(t) = arrangement.active_track()
                    && let Some(track) = arrangement.tracks.get_mut(t)
                {
                    track.mute = !track.mute;
                }
            }
            UiAction::ToggleTrackSolo => {
                if let Some(t) = arrangement.active_track()
                    && let Some(track) = arrangement.tracks.get_mut(t)
                {
                    track.solo = !track.solo;
                }
            }
            UiAction::NudgeTrackPan(delta) => {
                if let Some(t) = arrangement.active_track()
                    && let Some(track) = arrangement.tracks.get_mut(t)
                {
                    track.pan = (track.pan + delta).clamp(-1.0, 1.0);
                }
            }
            UiAction::CenterTrackPan => {
                if let Some(t) = arrangement.active_track()
                    && let Some(track) = arrangement.tracks.get_mut(t)
                {
                    track.pan = 0.0;
                }
            }
            // The rest of the vocabulary exists but nothing emits it yet.
            other => debug_assert!(false, "unhandled action: {other:?}"),
        }
    }
}

/// One track lane: its geometry, and the instrument that plays it.
///
/// Every track owns one sine synth — its knob positions for the device
/// panel, and the natural values those positions mean for the engine. The
/// two are kept side by side rather than derived, because the engine speaks
/// Hz/ms/gain and the knobs speak `0..=1`, and a recompile must be able to
/// bake in what the knobs last said without re-running the mapping.
/// The working key: a tonic pitch class and a scale. Musical content, so
/// it lives with the document rather than in preferences — a project
/// emailed to a stranger must open in the key it was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    /// Tonic as a pitch class, 0 = C.
    tonic: u8,
    scale: daw::theory::Scale,
}

impl Default for Key {
    fn default() -> Self {
        Self {
            tonic: 0,
            scale: daw::theory::Scale::Major,
        }
    }
}

impl Key {
    /// "C major" — shown in the piano roll beside the grid name.
    pub fn label(self) -> String {
        format!(
            "{} {}",
            daw::theory::pitch_class_name(self.tonic),
            self.scale.label()
        )
    }
}

/// An instrument a track can hold. One variant today; the enum exists so
/// adding the second is a match arm rather than a refactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    SineSynth,
    Reverb,
}

impl DeviceKind {
    /// Instruments MAKE sound, effects SHAPE it. A track holds one of each
    /// slot, and loading a device fills the slot it belongs to — so
    /// dropping a reverb on a track never displaces its instrument.
    fn is_instrument(self) -> bool {
        matches!(self, Self::SineSynth)
    }
}

struct Track {
    /// What the lane carries. Fixed at creation: changing a track's kind
    /// would change what every clip on it means, which is a conversion,
    /// not a toggle.
    kind: TrackKind,
    /// What the header shows and what a mixer strip will show later. Not
    /// unique — two tracks may share a name, exactly as two files in
    /// different folders may.
    name: String,
    height: f32,
    /// Silenced. A muted track leaves the SCHEDULE rather than being
    /// multiplied by zero: the graph should be as small as what is
    /// actually sounding.
    mute: bool,
    /// Soloed. While any track is soloed, only soloed tracks are wired —
    /// solo-in-place, the meaning every DAW agrees on.
    solo: bool,
    /// Constant-power pan, `-1..=1`. Center is 0.0, and it is exact: the
    /// knob snaps there so "back to the middle" is reachable by hand.
    pan: f32,
    /// The instrument loaded on this track, if any. `None` is a real
    /// state, not a placeholder: an empty track compiles to NO sequencer
    /// node and is silent. Loading a device from the browser is what
    /// gives a track a voice.
    device: Option<DeviceKind>,
    /// The effect after the instrument, if any. One slot for now; a chain
    /// is a Vec of these once a second effect exists.
    fx: Option<DeviceKind>,
    synth: device::SineSynthUi,
    params: SynthParams,
    reverb: device::ReverbUi,
}

impl Track {
    /// A fresh track of `kind`, named. The one door new tracks walk
    /// through, so the default session and Ctrl+T build the same thing.
    fn new(kind: TrackKind, name: String) -> Self {
        Self {
            kind,
            name,
            ..Self::default()
        }
    }
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
            // A fresh track has no instrument. Its knob state is still
            // here, ready, so loading a device shows sane values rather
            // than zeros.
            device: None,
            fx: None,
            synth: device::SineSynthUi::default(),
            params: SynthParams::default(),
            reverb: device::ReverbUi::default(),
        }
    }
}

/// A note inside a clip: MIDI pitch, velocity, start and length in beats
/// relative to the clip's start.
///
/// THE note type — the piano roll edits these in place and the engine
/// compiles them; there is no second representation to keep in sync. Beats
/// are f64 to match `graph::Note`, which is what these become at compile;
/// the clip's own placement stays f32 with the rest of the arrangement's
/// geometry.
#[derive(Debug, Clone, PartialEq)]
struct Note {
    pitch: u8,
    start: f64,
    len: f64,
    vel: u8,
}

/// One clip on a lane. Positions are absolute beats.
///
/// `id` is what selection tracks: indices shift whenever a clip moves past
/// a neighbour, ids never do. It is also what the piano roll watches to
/// notice it is looking at a different clip.
///
/// A clip's notes are NOT clamped to its length: editing a note never
/// resizes the clip, and a note starting at or past `len` simply does not
/// sound (see `seq_notes`). Shortening a clip therefore hides notes rather
/// than destroying them, and lengthening it brings them back.
#[derive(Debug, Clone, PartialEq)]
struct Clip {
    id: u64,
    name: String,
    start: f32,
    len: f32,
    notes: Vec<Note>,
}

/// A Ctrl+drag in flight: the copy rides the pointer as a translucent ghost
/// and lands where it is released. The original never moves.
struct Ghost {
    track: usize,
    clip: Clip,
    /// The original's start at press — the drag is measured from here, so
    /// the ghost's position is absolute, not incremental.
    start0: f32,
}

/// An inline rename in flight: which clip, and the text being edited.
/// `original` is what Escape restores.
struct Rename {
    track: usize,
    id: u64,
    text: String,
    original: String,
    /// Set once the edit box has been given the keyboard.
    focused: bool,
}

/// An inline track rename in flight: which lane, and the text being
/// edited. Separate from `Rename` (clips) because the two can never be
/// open at once but their identities differ — a lane is an index, a clip
/// is an id.
struct TrackRename {
    track: usize,
    text: String,
    original: String,
    focused: bool,
}

/// The arrangement's state.
struct Arrangement {
    tracks: Vec<Track>,
    /// Index into `GRID_BEATS`.
    grid: usize,
    /// The selected track. Time selection belongs to it and only it — one
    /// track at a time, so a selection can never span lanes.
    selected: Option<usize>,
    /// Selected time, in beats, always ordered and at least one grid unit
    /// wide. A zero-width selection would make Ctrl+L do nothing, which
    /// reads as broken rather than as empty.
    selection: Option<(f32, f32)>,
    /// The loop region, in beats.
    loop_range: Option<(f32, f32)>,
    /// The keyboard's cell cursor: (track, beat). The focus ring wraps this
    /// cell rather than the whole lane — a lane-sized ring could not say
    /// WHICH beat you were on, which is the thing you are choosing.
    cursor: Option<(usize, f32)>,
    /// The beat a shift-extended selection grows FROM. Plain movement drops
    /// it on the cursor; Shift+arrow leaves it put, which is what lets a
    /// selection grow in one direction and shrink back through itself.
    anchor: f32,
    /// Set while drawing when the ring is on one of our cells; read at the
    /// start of the next frame to decide who owns Left and Right.
    owns_arrows: bool,
    /// The first beat visible at the left edge of the view — the horizontal
    /// scroll position, in absolute beats. Only ever advanced by panning and
    /// by follow chasing the playhead; never negative.
    view_beats: f32,
    /// The working key: what the piano roll shades against and what the
    /// diatonic verbs build from.
    key: Key,
    /// Clips per track, each track's vec sorted by start. Sortedness is an
    /// invariant the overlap clamps rely on — `resort` restores it after
    /// every edit.
    clips: Vec<Vec<Clip>>,
    /// The selected clip, as (track, index into that track's clips).
    selected_clip: Option<(usize, usize)>,
    /// The clipboard: a whole clip, notes and all. `Ctrl+C` fills it,
    /// `Ctrl+V` reads it without emptying it.
    clipboard: Option<Clip>,
    /// The Ctrl+drag ghost, while one is in flight.
    ghost: Option<Ghost>,
    /// The inline rename, while one is open.
    rename: Option<Rename>,
    /// Monotonic id source — ids are also what the default clip names show.
    next_clip_id: u64,
    /// Monotonic per-KIND numbering for fresh track names, so deleting
    /// "Audio 2" never makes the next new track "Audio 2" again. Indexed
    /// by `TrackKind::ALL` order.
    next_track_no: [u32; TrackKind::ALL.len()],
    /// The header rename, while one is open.
    track_rename: Option<TrackRename>,
}

/// A note, spelled out. Only the tests build notes by hand — the app builds
/// them through the piano roll.
#[cfg(test)]
const fn note(pitch: u8, start: f64, len: f64, vel: u8) -> Note {
    Note {
        pitch,
        start,
        len,
        vel,
    }
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            tracks: (0..TRACK_COUNT)
                .map(|i| {
                    let kind = TrackKind::Midi;
                    Track::new(kind, format!("{} {}", kind.stem(), i + 1))
                })
                .collect(),
            grid: GRID_DEFAULT,
            selected: None,
            selection: None,
            loop_range: None,
            cursor: None,
            anchor: 0.0,
            owns_arrows: false,
            view_beats: 0.0,
            // A fresh session is empty: lanes are structural furniture, the
            // clips on them are the user's.
            clips: (0..TRACK_COUNT).map(|_| Vec::new()).collect(),
            selected_clip: None,
            clipboard: None,
            ghost: None,
            rename: None,
            key: Key::default(),
            next_clip_id: 1,
            // The default session's four lanes have already taken 1..=4.
            next_track_no: [TRACK_COUNT as u32 + 1, 1],
            track_rename: None,
        }
    }
}

impl Arrangement {
    fn grid_beats(&self) -> f32 {
        GRID_BEATS[self.grid.min(GRID_BEATS.len() - 1)]
    }

    /// Append a track of `kind`, named from that kind's own counter, and
    /// select it. The one door: Ctrl+T, the palette and any future menu
    /// all arrive here, so a new lane is always fully formed — a name, a
    /// clip vec, and the selection moved onto it.
    ///
    /// Returns its index.
    fn add_track(&mut self, kind: TrackKind) -> usize {
        let slot = Self::kind_slot(kind);
        let no = self.next_track_no[slot];
        self.next_track_no[slot] = no.saturating_add(1);
        self.tracks
            .push(Track::new(kind, format!("{} {no}", kind.stem())));
        self.clips.push(Vec::new());
        let i = self.tracks.len() - 1;
        // A new track is what you are about to work on. Selecting it also
        // means the device rack and the palette's track verbs point at it
        // without a second click.
        self.selected = Some(i);
        self.selected_clip = None;
        self.cursor = Some((i, self.cursor.map(|c| c.1).unwrap_or(0.0)));
        i
    }

    /// Drop a track and its clips. The LAST track is never removed — an
    /// arrangement with no lanes has nothing to click, and "undo" does not
    /// exist yet to get back out of it.
    fn remove_track(&mut self, track: usize) -> bool {
        if self.tracks.len() <= 1 || track >= self.tracks.len() {
            return false;
        }
        self.tracks.remove(track);
        self.clips.remove(track);
        // Every index pointing PAST the hole shifts down; every index AT
        // it is now pointing at a different track, so it is dropped.
        let fix = |i: usize| match i.cmp(&track) {
            std::cmp::Ordering::Less => Some(i),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(i - 1),
        };
        self.selected = self.selected.and_then(fix);
        self.selected_clip = self.selected_clip.and_then(|(t, c)| Some((fix(t)?, c)));
        self.cursor = self.cursor.and_then(|(t, b)| Some((fix(t)?, b)));
        self.track_rename = None;
        if self.selection.is_some() && self.selected.is_none() {
            self.selection = None;
        }
        true
    }

    /// Where a kind's name counter lives.
    fn kind_slot(kind: TrackKind) -> usize {
        match kind {
            TrackKind::Midi => 0,
            TrackKind::Audio => 1,
        }
    }

    /// A fresh id — also what the default clip name shows.
    fn next_id(&mut self) -> u64 {
        let id = self.next_clip_id;
        self.next_clip_id += 1;
        id
    }

    /// Insert a clip on `track` at the first gap at or after `at` that fits
    /// it, and select it. The one door every new clip walks through, so
    /// creation, paste and duplicate all share the same placement rules.
    fn insert_clip(&mut self, track: usize, clip: Clip, at: f32) -> Option<usize> {
        let (start, idx) = place_clip(&self.clips[track], at, clip.len);
        let mut clip = clip;
        clip.start = start;
        self.clips[track].insert(idx, clip);
        self.selected_clip = Some((track, idx));
        Some(idx)
    }

    /// Double-click on an empty lane: a one-bar clip at the click.
    fn create_clip(&mut self, track: usize, at: f32, len: f32) -> Option<usize> {
        let id = self.next_id();
        let name = format!("clip {id}");
        self.insert_clip(
            track,
            Clip {
                id,
                name,
                start: at,
                len,
                notes: vec![],
            },
            at,
        )
    }

    /// Ctrl+C. A missing selection copies nothing — silence, not an error.
    fn copy_selected(&mut self) {
        if let Some((t, i)) = self.selected_clip {
            self.clipboard = self.clips[t].get(i).cloned();
        }
    }

    /// Ctrl+V: the clipboard lands on `track` at `at`. The clipboard is not
    /// emptied — pasting twice makes two copies, like every other DAW.
    fn paste_clipboard(&mut self, track: usize, at: f32) -> Option<usize> {
        let mut clip = self.clipboard.clone()?;
        clip.id = self.next_id();
        self.insert_clip(track, clip, at)
    }

    /// Ctrl+D: a copy directly after the original, same name and notes.
    fn duplicate_selected(&mut self) -> Option<usize> {
        let (t, i) = self.selected_clip?;
        let mut copy = self.clips[t][i].clone();
        let at = copy.start + copy.len;
        copy.id = self.next_id();
        let clip = copy;
        self.insert_clip(t, clip, at)
    }

    /// Remove a clip by id — what the context menu's Delete uses. Clears
    /// the selection if the removed clip held it.
    fn remove_clip(&mut self, track: usize, id: u64) -> bool {
        let Some(i) = self.clips[track].iter().position(|c| c.id == id) else {
            return false;
        };
        self.clips[track].remove(i);
        if self.selected_clip == Some((track, i)) {
            self.selected_clip = None;
        }
        true
    }

    /// The track a paste lands on: the selected clip's, else the selected
    /// lane's, else the first.
    fn paste_track(&self) -> usize {
        self.selected_clip
            .map(|(t, _)| t)
            .or(self.selected)
            .unwrap_or(0)
    }

    /// The track the device panel belongs to: the selected clip's, else the
    /// selected lane's, else none. `paste_track`'s rule without the
    /// fallback-to-zero, because a rack showing track 0 when nothing is
    /// selected would claim a selection that does not exist.
    fn active_track(&self) -> Option<usize> {
        self.selected_clip.map(|(t, _)| t).or(self.selected)
    }

    /// The clip the piano roll edits, borrowed in place — the roll writes
    /// into the arrangement, never into a copy.
    fn active_clip(&mut self) -> Option<&mut Clip> {
        let (t, i) = self.selected_clip?;
        self.clips.get_mut(t)?.get_mut(i)
    }

    /// Its id, for the roll's clip-change reset.
    fn active_clip_id(&self) -> Option<u64> {
        let (t, i) = self.selected_clip?;
        self.clips.get(t)?.get(i).map(|c| c.id)
    }

    /// The beat a paste lands on: the cell cursor, else the selection's
    /// start, else the playhead — snapped to the grid.
    fn paste_beat(&self, playhead: f32) -> f32 {
        let at = self
            .cursor
            .map(|c| c.1)
            .or_else(|| self.selection.map(|s| s.0))
            .unwrap_or(playhead);
        snap(at, self.grid_beats())
    }
}

/// Beats <-> pixels, and snapping. All pure; the arrangement's whole
/// coordinate story lives in these four functions.
///
/// `offset` is the beat shown at the area's left edge — where the view has
/// been panned to. Beats are absolute musical time; the offset is only
/// where the window sits. Both functions are inverses at any offset.
fn beat_at(area: egui::Rect, offset: f32, x: f32) -> f32 {
    (offset + (x - area.left()) / PX_PER_BEAT).max(0.0)
}

fn x_at(area: egui::Rect, offset: f32, beat: f32) -> f32 {
    area.left() + (beat - offset) * PX_PER_BEAT
}

fn snap(beat: f32, grid: f32) -> f32 {
    if grid <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / grid).round() * grid).max(0.0)
}

/// Order a drag's two ends and guarantee it spans at least one grid unit.
///
/// A click with no drag would otherwise select zero time, and Ctrl+L on zero
/// time can only no-op — which looks like a broken shortcut rather than an
/// empty selection.
fn span(a: f32, b: f32, grid: f32) -> (f32, f32) {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if hi - lo < grid {
        (lo, lo + grid)
    } else {
        (lo, hi)
    }
}

/// The grid cell at `beat` in `lane`: one grid division wide, the lane's
/// full height.
fn cell_rect(
    content: egui::Rect,
    offset: f32,
    lane: egui::Rect,
    beat: f32,
    grid: f32,
) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(x_at(content, offset, beat), lane.top()),
        egui::pos2(x_at(content, offset, beat + grid), lane.bottom()),
    )
}

/// The selection covering every cell between `anchor` and `cursor`.
///
/// Inclusive at both ends: anchoring on beat 4 and extending to beat 6
/// selects cells 4, 5 and 6, so the range runs to `6 + grid`. With the two
/// equal it is one cell, which is exactly what plain movement produces.
///
/// Pure, so the extend arithmetic is checkable in both directions.
fn extended(anchor: f32, cursor: f32, grid: f32) -> (f32, f32) {
    (anchor.min(cursor), anchor.max(cursor) + grid)
}

/// Step the grid one rung, clamped at both ends.
///
/// Clamped rather than wrapped: walking off the fine end and reappearing at
/// 1/1 would be a nasty surprise mid-edit. Pure, so the ladder is testable.
fn step_grid(grid: usize, finer: bool) -> usize {
    if finer {
        (grid + 1).min(GRID_BEATS.len() - 1)
    } else {
        grid.saturating_sub(1)
    }
}

// --- clips: geometry and overlap clamps, all pure -------------------------

/// A clip's rect in its lane. The caller clips it to the visible area.
fn clip_rect(content: egui::Rect, offset: f32, lane: egui::Rect, clip: &Clip) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(x_at(content, offset, clip.start), lane.top()),
        egui::pos2(x_at(content, offset, clip.start + clip.len), lane.bottom()),
    )
}

/// Where a note's bar goes inside a clip's rect, given the clip's pitch
/// range. Higher pitches sit higher in the block, the way a piano roll
/// reads, and every bar is at least `NOTE_MIN_H` tall so quiet notes do not
/// vanish into the fill.
fn note_rect(
    area: egui::Rect,
    note: &Note,
    pitch_lo: u8,
    pitch_hi: u8,
    clip_len: f32,
) -> egui::Rect {
    let span = (pitch_hi - pitch_lo + 1) as f32;
    let frac = (note.pitch - pitch_lo) as f32 / span;
    let band = area.height() / span;
    let y = area.top() + (1.0 - frac) * area.height() - band * 0.5;
    // Notes carry musical time in f64; the lane's geometry is f32.
    let x = area.left() + (note.start as f32 / clip_len) * area.width();
    let w = ((note.len as f32 / clip_len) * area.width()).max(1.0);
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, band.max(NOTE_MIN_H)))
}

/// The bounds a clip's start must respect: at or after the previous clip's
/// end, at or before the next clip's start minus its own length, never
/// negative. `clips` is sorted by start — it always is, by construction.
///
/// When neighbours leave no room at all, the answer is "stay put": a
/// pinned clip is better than one that jumped through a neighbour.
fn clamp_clip_start(clips: &[Clip], idx: usize, want: f32) -> f32 {
    let len = clips[idx].len;
    let lo = idx
        .checked_sub(1)
        .map(|p| clips[p].start + clips[p].len)
        .unwrap_or(0.0);
    let hi = clips
        .get(idx + 1)
        .map(|n| n.start - len)
        .unwrap_or(f32::INFINITY);
    if hi < lo {
        return clips[idx].start;
    }
    want.clamp(lo, hi).max(0.0)
}

/// The bounds a clip's length must respect while its start stays put: one
/// grid unit minimum, the gap to the next clip maximum.
fn clamp_clip_len(clips: &[Clip], idx: usize, want: f32, grid: f32) -> f32 {
    let hi = clips
        .get(idx + 1)
        .map(|n| n.start - clips[idx].start)
        .unwrap_or(f32::INFINITY);
    if hi < grid {
        return clips[idx].len;
    }
    want.clamp(grid, hi)
}

/// Keep a track's clips sorted by start after an edit. Order is what the
/// clamp functions assume — this is the function that restores it.
fn resort(track: &mut [Clip]) {
    track.sort_by(|a, b| a.start.total_cmp(&b.start));
}

/// Where a NEW clip of `len` beats can live: the first gap at or after `at`
/// that fits it, else the end of the track. Returns (start, insertion
/// index). The final gap is infinite, so there is always an answer — new
/// clips never fail to place, they just land further right than asked.
///
/// Pure; the placement policy behind create, paste and duplicate alike.
fn place_clip(track: &[Clip], at: f32, len: f32) -> (f32, usize) {
    let idx = track.partition_point(|c| c.start < at);
    for i in idx..=track.len() {
        let lo = i
            .checked_sub(1)
            .map(|p| track[p].start + track[p].len)
            .unwrap_or(0.0);
        let hi = track.get(i).map(|n| n.start).unwrap_or(f32::INFINITY);
        if hi - lo >= len {
            return (at.clamp(lo, hi - len).max(0.0), i);
        }
    }
    let end = track.last().map(|c| c.start + c.len).unwrap_or(0.0);
    (end, track.len())
}

/// Where each lane sits, top to bottom, and the boundary below it.
///
/// Pure, so lane stacking and the resize maths are checkable without a
/// window. Lanes past the bottom of the view are still returned — the caller
/// decides what to draw.
/// Does this track reach the mixer?
///
/// Solo-in-place: while ANYTHING is soloed, only soloed tracks sound. Mute
/// wins over solo, so muting a soloed track still silences it — which is
/// what both buttons being lit has to mean.
///
/// The one answer, used twice: `build_graph_spec` decides what to wire from
/// it, and the header dims the tracks it says are silent. A header that
/// disagreed with the schedule would be the worst possible bug here.
/// Everything about the tracks that changes the graph's SHAPE, hashed.
///
/// Loading or removing a device adds or drops a node; so does muting a
/// track, soloing one (which drops every other), or a track's KIND, since
/// an audio track compiles to no sequencer at all. None of that can be
/// expressed as a parameter letter, so a change here must swap the schedule
/// immediately rather than wait for the clip debounce.
///
/// A hash rather than the bitmask this used to be: there is no longer a
/// fixed number of bits per track, and the mask silently stopped covering
/// tracks past the 32nd.
///
/// PAN IS NOT IN HERE, deliberately: every instrument track always carries
/// a Pan node, so pan rides a letter and a knob drag costs nothing.
fn shape_hash(tracks: &[Track]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for t in tracks {
        t.kind.hash(&mut hasher);
        t.device.is_some().hash(&mut hasher);
        t.fx.is_some().hash(&mut hasher);
        t.mute.hash(&mut hasher);
        t.solo.hash(&mut hasher);
    }
    hasher.finish()
}

fn track_audible(tracks: &[Track], i: usize) -> bool {
    let any_solo = tracks.iter().any(|t| t.solo);
    tracks
        .get(i)
        .is_some_and(|t| !t.mute && (!any_solo || t.solo))
}

fn lane_rects(area: egui::Rect, tracks: &[Track]) -> Vec<egui::Rect> {
    let mut y = area.top();
    tracks
        .iter()
        .map(|t| {
            let rect = egui::Rect::from_min_max(
                egui::pos2(area.left(), y),
                egui::pos2(area.right(), y + t.height),
            );
            y += t.height;
            rect
        })
        .collect()
}

/// Paint the beat grid across `area`.
///
/// Three weights: bars, beats, and whatever subdivision the grid is set to.
/// Subdivisions are dropped entirely when they would land closer together
/// than `GRID_MIN_PX` — at 1/32 and this zoom that is 3px apart, which reads
/// as a wash rather than a grid.
fn beat_grid(
    ui: &egui::Ui,
    area: egui::Rect,
    theme: &Theme,
    arr: &Arrangement,
    beats_per_bar: u32,
) {
    let painter = ui.painter();
    let sub = arr.grid_beats();
    let sub_px = sub * PX_PER_BEAT;
    let step = if sub_px < GRID_MIN_PX { 1.0 } else { sub };
    let per_bar = beats_per_bar.max(1) as f32;

    // Start at the first line at or before the left edge, so lines land on
    // absolute beat/bar boundaries no matter where the view is panned.
    let mut beat = (arr.view_beats / step).floor() * step;
    loop {
        let x = x_at(area, arr.view_beats, beat);
        if x > area.right() {
            break;
        }
        let on_bar = (beat % per_bar).abs() < 1e-3;
        let on_beat = (beat.fract()).abs() < 1e-3;
        let colour = if on_bar {
            theme.grid_bar
        } else if on_beat {
            theme.grid_beat
        } else {
            theme.grid_sub
        };
        painter.line_segment(
            [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())],
            egui::Stroke::new(1.0, colour),
        );
        beat += step;
    }
}

/// A small bipolar pan knob, drawn into `rect`.
///
/// Deliberately not `kit::knob`: this one lives inside a lane rather than
/// on a device card, so it is sized here rather than by the theme's control
/// scale, and it has a center DETENT — pan's most-wanted value is exactly
/// 0.0, and a knob you cannot return to center by hand is a knob you end up
/// fighting. Double-click centers it outright.
///
/// `pan` is `-1..=1`. Returns true when the user moved it.
fn pan_knob(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, pan: &mut f32) -> bool {
    let id = ui
        .id()
        .with(("pan", rect.left_top().x as i32, rect.top() as i32));
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let mut changed = false;

    if response.double_clicked() {
        if *pan != 0.0 {
            *pan = 0.0;
            changed = true;
        }
    } else if response.dragged() {
        // Full travel over four knob-heights, tenth-speed with Shift —
        // the same feel as every other knob in the app.
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = rect.height() * 4.0 * if fine { 10.0 } else { 1.0 };
        let delta = -response.drag_delta().y / travel * 2.0;
        if delta != 0.0 {
            let next = (*pan + delta).clamp(-1.0, 1.0);
            // The detent: crossing the middle STICKS there for a moment
            // instead of sliding through it.
            let next = if next.abs() < PAN_DETENT { 0.0 } else { next };
            if next != *pan {
                *pan = next;
                changed = true;
            }
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    // --- paint ------------------------------------------------------------
    const START: f32 = std::f32::consts::PI * 0.75;
    const SWEEP: f32 = std::f32::consts::PI * 1.5;
    let center = rect.center();
    let radius = rect.width() * 0.5 - stroke::BOLD;
    let painter = ui.painter();
    painter.circle_filled(center, radius, theme.surface_sunken);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    let arc = |from: f32, to: f32, s: egui::Stroke| {
        const STEPS: usize = 16;
        let points: Vec<egui::Pos2> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                center + kit::knob_dir(a) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };

    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let mid = START + SWEEP * 0.5;
    let at = START + SWEEP * ((*pan + 1.0) * 0.5).clamp(0.0, 1.0);
    if (at - mid).abs() > f32::EPSILON {
        arc(mid, at, egui::Stroke::new(stroke::BOLD, theme.accent));
    }
    let dir = kit::knob_dir(at);
    painter.line_segment(
        [center + dir * (radius * 0.35), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );
    // The 12-o'clock tick: where center IS, so the detent is visible.
    painter.line_segment(
        [
            egui::pos2(center.x, rect.top() - 1.0),
            egui::pos2(center.x, rect.top() + 1.0),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    changed
}

/// How pan reads out: "C" at center, "L42" / "R42" either side. Percent,
/// because degrees would imply a precision constant-power panning does not
/// have.
fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        "C".to_owned()
    } else if pan < 0.0 {
        format!("L{amount}")
    } else {
        format!("R{amount}")
    }
}

/// One small square toggle — the M and S of a track header.
///
/// Returns true when clicked. `on_fill` is the colour it takes while
/// engaged; off, it is a hairline outline and nothing else, so a header
/// with nothing engaged is quiet.
fn header_toggle(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    id: egui::Id,
    letter: &str,
    on: bool,
    on_fill: egui::Color32,
) -> bool {
    let response = ui.interact(rect, id, egui::Sense::click());
    let painter = ui.painter();
    let fill = if on {
        on_fill
    } else if response.hovered() {
        theme.surface_raised
    } else {
        theme.surface_sunken
    };
    painter.rect_filled(rect, radius::CTRL, fill);
    painter.rect_stroke(
        rect,
        radius::CTRL,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(HEADER_BTN_TYPE),
        // Engaged, the fill carries the meaning and the letter sits on it
        // in the page colour; idle, the letter is the only thing there.
        if on { theme.bg } else { theme.text_muted },
    );
    response.clicked()
}

/// The header column down the arrangement's left edge: one header per lane,
/// each with a name, a kind badge, mute, solo, and pan.
///
/// Headers are MOUSE-driven by design. Registering them with `Focus` would
/// put them in the arrow-key graph immediately left of the lane cells, which
/// already claim Left and Right for the grid — the ring could get in but
/// never out. The keyboard reaches all of this through the palette's track
/// verbs instead, which is why those exist.
///
/// `lanes` carries the y geometry (and only that); the x range is the
/// column's.
fn track_headers(
    ui: &mut egui::Ui,
    theme: &Theme,
    arr: &mut Arrangement,
    column: egui::Rect,
    lanes: &[egui::Rect],
) {
    ui.painter().rect_filled(column, 0.0, theme.surface_sunken);

    // Intents, applied after the loop: the closure body borrows `arr`
    // immutably to read each track, so it cannot also write to it.
    let mut select: Option<usize> = None;
    let mut mute: Option<usize> = None;
    let mut solo: Option<usize> = None;
    let mut rename_open: Option<usize> = None;
    let mut pan_edit: Option<(usize, f32)> = None;
    let renaming = arr.track_rename.as_ref().map(|r| r.track);

    for (i, lane) in lanes.iter().enumerate() {
        if lane.top() > column.bottom() {
            break;
        }
        let Some(track) = arr.tracks.get(i) else {
            break;
        };
        let head = egui::Rect::from_min_max(
            egui::pos2(column.left(), lane.top()),
            egui::pos2(column.right(), lane.bottom()),
        )
        .intersect(column);
        if head.height() <= 0.0 {
            continue;
        }
        let wid = ui.id().with(("header", i));
        // What the schedule will actually wire. A track the solo rule has
        // excluded reads as silent here rather than looking live.
        let audible = track_audible(&arr.tracks, i);

        if arr.selected == Some(i) {
            ui.painter().rect_filled(head, 0.0, theme.surface);
            // A selected lane gets a spine in the accent, so which track
            // the palette's verbs will hit is readable at a glance.
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    head.left_top(),
                    egui::pos2(head.left() + stroke::FOCUS, head.bottom()),
                ),
                0.0,
                theme.accent,
            );
        }

        // --- the name row -------------------------------------------------
        let name_row = egui::Rect::from_min_max(
            egui::pos2(head.left() + HEADER_PAD, head.top() + HEADER_PAD * 0.5),
            egui::pos2(
                head.right() - HEADER_PAD,
                (head.top() + HEADER_PAD * 0.5 + HEADER_NAME_H).min(head.bottom()),
            ),
        );
        if name_row.height() > 1.0 {
            let badge = track.kind.label();
            let badge_w = badge.len() as f32 * HEADER_KIND_TYPE * 0.62;
            let text_row = egui::Rect::from_min_max(
                name_row.min,
                egui::pos2(name_row.right() - badge_w - HEADER_PAD, name_row.bottom()),
            );

            if renaming == Some(i) {
                // The edit box replaces the label in place. Escape and
                // Enter are handled by the caller of this frame's rename
                // state, below.
                if let Some(rename) = arr.track_rename.as_mut() {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(text_row)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let field = child.add(
                        egui::TextEdit::singleline(&mut rename.text)
                            .desired_width(text_row.width())
                            .font(egui::FontId::proportional(HEADER_NAME_TYPE)),
                    );
                    if !rename.focused {
                        field.request_focus();
                        rename.focused = true;
                    }
                }
            } else {
                let response = ui.interact(text_row, wid.with("name"), egui::Sense::click());
                if response.double_clicked() {
                    rename_open = Some(i);
                } else if response.clicked() {
                    select = Some(i);
                }
                ui.painter().text(
                    text_row.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &track.name,
                    egui::FontId::proportional(HEADER_NAME_TYPE),
                    match (audible, arr.selected == Some(i)) {
                        (false, _) => theme.divider,
                        (true, true) => theme.text,
                        (true, false) => theme.text_muted,
                    },
                );
            }

            ui.painter().text(
                name_row.right_center(),
                egui::Align2::RIGHT_CENTER,
                badge,
                egui::FontId::proportional(HEADER_KIND_TYPE),
                theme.text_muted,
            );
        }

        // --- mute, solo, pan ------------------------------------------------
        // Dropped entirely on a squeezed lane: half a button is worse than
        // no button, and a lane pulled down to a sliver is not the one
        // being worked on.
        if head.height() < HEADER_ROWS_MIN_H {
            continue;
        }
        let row_y = name_row.bottom() + HEADER_PAD * 0.5;
        let btn = |n: f32| {
            egui::Rect::from_min_size(
                egui::pos2(head.left() + HEADER_PAD + n * (HEADER_BTN + 3.0), row_y),
                egui::vec2(HEADER_BTN, HEADER_BTN),
            )
        };
        if header_toggle(
            ui,
            theme,
            btn(0.0),
            wid.with("mute"),
            "M",
            track.mute,
            theme.warn,
        ) {
            mute = Some(i);
        }
        if header_toggle(
            ui,
            theme,
            btn(1.0),
            wid.with("solo"),
            "S",
            track.solo,
            theme.accent,
        ) {
            solo = Some(i);
        }

        let knob_rect = egui::Rect::from_min_size(
            egui::pos2(
                head.right() - HEADER_PAD - HEADER_KNOB,
                row_y + (HEADER_BTN - HEADER_KNOB) * 0.5,
            ),
            egui::vec2(HEADER_KNOB, HEADER_KNOB),
        );
        let mut pan = track.pan;
        if pan_knob(ui, theme, knob_rect, &mut pan) {
            pan_edit = Some((i, pan));
        }
        ui.painter().text(
            egui::pos2(knob_rect.left() - HEADER_PAD * 0.5, knob_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            pan_label(pan),
            egui::FontId::monospace(HEADER_KIND_TYPE),
            theme.text_value,
        );
    }

    // The column's right edge, so the headers read as their own strip
    // rather than as the first bar of the grid.
    ui.painter().line_segment(
        [column.right_top(), column.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    if let Some(i) = select {
        arr.selected = Some(i);
        arr.selected_clip = None;
    }
    if let Some(i) = mute
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.mute = !t.mute;
    }
    if let Some(i) = solo
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.solo = !t.solo;
    }
    if let Some((i, pan)) = pan_edit
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.pan = pan;
    }
    if let Some(i) = rename_open
        && let Some(t) = arr.tracks.get(i)
    {
        arr.selected = Some(i);
        arr.track_rename = Some(TrackRename {
            track: i,
            text: t.name.clone(),
            original: t.name.clone(),
            focused: false,
        });
    }
}

/// Resolve an open track rename: Enter commits, Escape restores what was
/// there, and losing the keyboard commits too (clicking away is not a
/// cancel anywhere else in the app either).
///
/// Pure over UI state and called before anything else reads the keyboard,
/// so a name being typed can never also be a shortcut.
fn track_rename_keys(ctx: &egui::Context, arr: &mut Arrangement) {
    let Some(rename) = arr.track_rename.as_ref() else {
        return;
    };
    if !rename.focused {
        return;
    }
    let (commit, cancel) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    let still_editing = ctx.memory(|m| m.focused()).is_some();
    if !(commit || cancel || !still_editing) {
        return;
    }
    // Take it out first: every path below ends with the rename closed.
    let Some(rename) = arr.track_rename.take() else {
        return;
    };
    let Some(track) = arr.tracks.get_mut(rename.track) else {
        return;
    };
    let text = rename.text.trim();
    // An empty name is not a name. Cancelling and emptying both restore
    // what was there, so a track always has something to call itself.
    track.name = if cancel || text.is_empty() {
        rename.original
    } else {
        text.to_owned()
    };
}

/// The arrangement: a loop ruler, then lanes stacked in a beat grid.
///
/// Selection is click-and-drag inside a lane; it snaps to the grid and stays
/// within the one track, because a selection spanning lanes would have no
/// meaning for the loop it becomes.
///
/// The view pans horizontally with the wheel (Shift+wheel or plain wheel —
/// there is no vertical overflow to spend it on). Returns true when the user
/// panned this frame, so the caller can hand the view back: manual panning
/// turns follow off.
///
/// `playhead` is the transport position in beats; it is drawn above the
/// clips, and `follow` pages the view to keep it on screen.
fn arrangement_body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    arr: &mut Arrangement,
    beats_per_bar: u32,
    playhead: f32,
    follow: bool,
) -> bool {
    let area = ui.max_rect();
    claim(ui);

    // Wheel panning. Follow is turned off by the caller on the return value,
    // not here — the arrangement does not own transport state.
    // The header column owns the left edge; everything time-shaped lives to
    // the right of it. Splitting HERE, before anything reads a rect, is what
    // keeps `beat_at` / `x_at` honest: they measure from `content.left()`,
    // so the timeline's beat 0 is the column's right edge and not the
    // window's.
    let column_w = HEADER_W.min(area.width() * 0.5);
    let timeline_left = area.left() + column_w;

    let scroll = ui.input(|i| i.smooth_scroll_delta);
    let pan = scroll.x + scroll.y;
    let mut panned = false;
    let pointer_on_timeline = ui
        .ctx()
        .pointer_latest_pos()
        .is_some_and(|p| p.x >= timeline_left);
    if pan != 0.0 && ui.ui_contains_pointer() && pointer_on_timeline {
        arr.view_beats = (arr.view_beats + pan / PX_PER_BEAT).max(0.0);
        panned = true;
    }

    let ruler = egui::Rect::from_min_max(
        egui::pos2(timeline_left, area.top()),
        egui::pos2(area.right(), area.top() + LOOP_RULER_H),
    );
    let content = egui::Rect::from_min_max(egui::pos2(timeline_left, ruler.bottom()), area.max);
    let column = egui::Rect::from_min_max(
        egui::pos2(area.left(), ruler.bottom()),
        egui::pos2(timeline_left, area.bottom()),
    );
    let grid = arr.grid_beats();

    // Follow pages the view BEFORE anything reads the offset, so the
    // playhead is on screen the same frame it outran the window.
    arr.view_beats = follow_view(
        arr.view_beats,
        playhead,
        content.width() / PX_PER_BEAT,
        follow,
    );
    let offset = arr.view_beats;

    beat_grid(ui, content, theme, arr, beats_per_bar);

    ui.painter().text(
        egui::pos2(area.right() - GRID_LABEL_PAD, ruler.center().y),
        egui::Align2::RIGHT_CENTER,
        GRID_NAMES[arr.grid.min(GRID_NAMES.len() - 1)],
        egui::FontId::new(GRID_LABEL_TYPE, egui::FontFamily::Monospace),
        theme.text_muted,
    );

    // --- the loop region, under everything it covers ----------------------
    if let Some((from, to)) = arr.loop_range {
        let band = egui::Rect::from_min_max(
            egui::pos2(x_at(content, offset, from), content.top()),
            egui::pos2(x_at(content, offset, to), content.bottom()),
        );
        ui.painter()
            .rect_filled(band.intersect(content), 0.0, theme.loop_region);
    }

    let grab = ui.style().interaction.resize_grab_radius_side;
    let lanes = lane_rects(content, &arr.tracks);
    let mut resize: Option<(usize, f32)> = None;
    let mut select: Option<(usize, f32, f32)> = None;
    let mut create_req: Option<(usize, f32)> = None;
    // A Cell for the same reason as clips_pass's menu: one closure per lane
    // per frame, one shared slot.
    let menu_create: std::cell::Cell<Option<(usize, f32)>> = std::cell::Cell::new(None);
    // Where the cell cursor sits. Beat is shared across lanes so moving up
    // or down keeps your place in time.
    let (_, cursor_beat) = arr.cursor.unwrap_or((0, 0.0));
    let mut focused_lane: Option<usize> = None;

    for (i, lane) in lanes.iter().enumerate() {
        if lane.top() > content.bottom() {
            break;
        }
        let visible = lane.intersect(content);
        // The focusable rect is the CELL, not the lane. Cells in different
        // lanes share an x, so Up and Down keep your place in time while the
        // generic spatial navigation does the work.
        let cell = cell_rect(content, offset, visible, cursor_beat, grid);
        let wid = ui.id().with(("lane", i));
        if focus.register(wid, cell.intersect(visible)) {
            focused_lane = Some(i);
        }

        if arr.selected == Some(i) {
            // The selected lane lifts one step off the canvas.
            ui.painter().rect_filled(visible, 0.0, theme.surface);
        }

        // The lane body, minus the strip the resize handle owns — otherwise
        // reaching for the boundary would also start a selection.
        let body = egui::Rect::from_min_max(
            visible.min,
            egui::pos2(
                visible.right(),
                (visible.bottom() - grab).max(visible.top()),
            ),
        );
        let anchor_id = wid.with("anchor");
        let picked = ui.interact(body, wid.with("body"), egui::Sense::click_and_drag());
        if let Some(pos) = picked.interact_pointer_pos() {
            let here = snap(beat_at(content, offset, pos.x), grid);
            if picked.drag_started() || picked.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(anchor_id, here));
                select = Some((i, here, here));
            } else if picked.dragged() {
                let from: f32 = ui.ctx().data(|d| d.get_temp(anchor_id).unwrap_or(here));
                select = Some((i, from, here));
            }
        }

        // Double-click creates a one-bar clip at the click; the right-click
        // menu offers the same through words.
        if picked.double_clicked()
            && let Some(pos) = picked.interact_pointer_pos()
        {
            create_req = Some((i, snap(beat_at(content, offset, pos.x), grid)));
        }
        let menu_beat = picked
            .interact_pointer_pos()
            .map(|p| snap(beat_at(content, offset, p.x), grid))
            .unwrap_or(cursor_beat);
        picked.context_menu(|ui| {
            if ui.button("New clip").clicked() {
                menu_create.set(Some((i, menu_beat)));
                ui.close();
            }
        });

        // The selection wash, inside its lane only.
        if arr.selected == Some(i)
            && let Some((from, to)) = arr.selection
        {
            let band = egui::Rect::from_min_max(
                egui::pos2(x_at(content, offset, from), visible.top()),
                egui::pos2(x_at(content, offset, to), visible.bottom()),
            );
            ui.painter()
                .rect_filled(band.intersect(visible), 0.0, theme.selection);
        }

        // The boundary below this lane resizes it.
        let seam = egui::Rect::from_min_max(
            egui::pos2(lane.left(), lane.bottom() - grab),
            egui::pos2(lane.right(), lane.bottom() + grab),
        );
        let response = ui.interact(seam, wid.with("seam"), egui::Sense::drag());
        if response.dragged() {
            resize = Some((i, arr.tracks[i].height + response.drag_delta().y));
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(lane.left(), lane.bottom() - SEAM_PX),
                    egui::pos2(lane.right(), lane.bottom()),
                ),
                0.0,
                SEAM,
            );
        } else {
            ui.painter().line_segment(
                [
                    egui::pos2(lane.left(), lane.bottom()),
                    egui::pos2(lane.right(), lane.bottom()),
                ],
                egui::Stroke::new(1.0, theme.divider),
            );
        }
    }

    if let Some((i, from, to)) = select {
        arr.selected = Some(i);
        arr.selection = Some(span(from, to, grid));
        // Keep the keyboard where the mouse just went, so arrowing carries
        // on from where you clicked instead of jumping back.
        arr.cursor = Some((i, span(from, to, grid).0));
        arr.anchor = span(from, to, grid).0;
        // A drag on empty lane is a new intention; a clip left selected
        // from before is not part of it.
        arr.selected_clip = None;
    }

    // Arrowing between lanes moves the cursor and takes the selection with
    // it — the cell you are on IS the selection.
    arr.owns_arrows = focused_lane.is_some();
    if let Some(i) = focused_lane
        && arr.cursor.map(|c| c.0) != Some(i)
    {
        arr.cursor = Some((i, cursor_beat));
        arr.anchor = cursor_beat;
        arr.selected = Some(i);
        arr.selection = Some(span(cursor_beat, cursor_beat, grid));
    }
    if let Some((i, height)) = resize {
        arr.tracks[i].height = height.clamp(*TRACK_H_RANGE.start(), *TRACK_H_RANGE.end());
    }

    // Creation: double-click and the menu both land here, one bar long, at
    // the snapped click beat. `create_clip` picks the first gap that fits
    // and selects the result.
    if let Some((t, at)) = create_req.or(menu_create.get())
        && arr.create_clip(t, at, beats_per_bar as f32).is_some()
    {
        arr.selected = Some(t);
    }

    loop_brace(ui, theme, focus, ruler, content, arr, grid);
    clips_pass(ui, theme, content, arr, grid);

    // The headers last of the lane furniture, so their fills and controls
    // sit above the grid lines that run under the column's edge.
    track_headers(ui, theme, arr, column, &lanes);

    // --- the playhead, above everything it passes over ---------------------
    let x = x_at(content, offset, playhead);
    if x >= content.left() && x <= content.right() {
        let painter = ui.painter();
        painter.line_segment(
            [
                egui::pos2(x, content.top()),
                egui::pos2(x, content.bottom()),
            ],
            egui::Stroke::new(1.5, theme.playhead),
        );
        // The ruler triangle, pointing down at the line it belongs to.
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, ruler.top() + 2.0),
                egui::pos2(x - PLAYHEAD_TRI_HALF, ruler.top() + 2.0 + PLAYHEAD_TRI_H),
                egui::pos2(x + PLAYHEAD_TRI_HALF, ruler.top() + 2.0 + PLAYHEAD_TRI_H),
            ],
            theme.playhead,
            egui::Stroke::NONE,
        ));
    }

    panned
}

/// What the clip context menu asked for.
#[derive(Clone, Copy)]
enum ClipMenu {
    Copy,
    Duplicate,
    Rename,
    Delete,
}

/// Draw and interact with the clips.
///
/// Runs AFTER the lane pass, so clips sit above the selection wash and their
/// interact rects are created after the lane bodies' — a clip wins the
/// pointer wherever they overlap, and a click on empty lane still starts a
/// time selection. Edits are collected while drawing and applied at the end:
/// the draw borrow stays read-only, and a drag reads last frame's rects
/// against this frame's delta — one frame of lag, invisible at 60fps.
fn clips_pass(
    ui: &mut egui::Ui,
    theme: &Theme,
    content: egui::Rect,
    arr: &mut Arrangement,
    grid: f32,
) {
    let offset = arr.view_beats;
    let lanes = lane_rects(content, &arr.tracks);
    // The rename and the ghost ride out of `arr` for the draw — both are
    // frame-to-frame state the pass either finishes or hands back.
    let mut rename = arr.rename.take();
    let mut ghost = arr.ghost.take();

    let mut select: Option<(usize, u64)> = None;
    let mut move_to: Option<(usize, u64, f32)> = None;
    let mut left_to: Option<(usize, u64, f32)> = None;
    let mut right_to: Option<(usize, u64, f32)> = None;
    let mut ghost_finalize: Option<Ghost> = None;
    // A Cell, because one context-menu closure is created per clip per
    // frame and they all need to reach the same slot.
    let menu: std::cell::Cell<Option<(usize, u64, ClipMenu)>> = std::cell::Cell::new(None);
    let mut rename_commit = false;
    let mut rename_cancel = false;

    for (t, track) in arr.clips.iter().enumerate() {
        let Some(lane) = lanes.get(t) else { break };
        for (i, clip) in track.iter().enumerate() {
            let rect = clip_rect(content, offset, *lane, clip).intersect(content);
            if rect.width() <= 0.0 {
                continue;
            }
            let selected = arr.selected_clip == Some((t, i));
            let body_id = ui.id().with(("clip", clip.id));

            // Interact BEFORE painting: the strips, created after the body,
            // sit above it in hit-test order, and the paint lands on top of
            // both in the same order it is issued.
            let body = ui.interact(rect, body_id, egui::Sense::click_and_drag());
            let command = ui.input(|i| i.modifiers.command);
            if body.drag_started() {
                if command {
                    // Ctrl+drag: the original stays, a ghost copy rides the
                    // pointer and lands on release.
                    ghost = Some(Ghost {
                        track: t,
                        clip: clip.clone(),
                        start0: clip.start,
                    });
                } else {
                    select = Some((t, clip.id));
                }
            }
            if body.clicked() || body.secondary_clicked() {
                select = Some((t, clip.id));
            }
            if body.double_clicked() {
                select = Some((t, clip.id));
                rename = Some(Rename {
                    track: t,
                    id: clip.id,
                    text: clip.name.clone(),
                    original: clip.name.clone(),
                    focused: false,
                });
            }
            if body.dragged() {
                if let Some(g) = &mut ghost
                    && g.track == t
                    && g.clip.id == clip.id
                {
                    let want = g.start0 + body.drag_delta().x / PX_PER_BEAT;
                    g.clip.start = snap(want, grid);
                } else {
                    // drag_delta is measured from the press origin, so this
                    // is absolute, not incremental — no drift accumulates.
                    move_to = Some((t, clip.id, clip.start + body.drag_delta().x / PX_PER_BEAT));
                }
            }
            if body.drag_stopped() {
                // The ghost lands: place it wherever the first fitting gap
                // is, like any other new clip. Deferred to the apply
                // section — the draw loop still holds `arr.clips` borrowed.
                let active = ghost
                    .as_ref()
                    .is_some_and(|g| g.track == t && g.clip.id == clip.id);
                if active {
                    ghost_finalize = ghost.take();
                }
            }
            if body.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if body.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }

            body.context_menu(|ui| {
                if ui.button("Copy").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Copy)));
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Duplicate)));
                    ui.close();
                }
                if ui.button("Rename").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Rename)));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Delete)));
                    ui.close();
                }
            });

            if selected {
                let left = egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2((rect.left() + CLIP_EDGE_W).min(rect.right()), rect.bottom()),
                );
                let right = egui::Rect::from_min_max(
                    egui::pos2((rect.right() - CLIP_EDGE_W).max(rect.left()), rect.top()),
                    rect.max,
                );
                for (side, strip) in [(0usize, left), (1usize, right)] {
                    let wid = ui.id().with(("clip_edge", clip.id, side));
                    let resp = ui.interact(strip, wid, egui::Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if resp.dragged()
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let beat = beat_at(content, offset, pos.x);
                        if side == 0 {
                            left_to = Some((t, clip.id, beat));
                        } else {
                            right_to = Some((t, clip.id, beat));
                        }
                    }
                }
            }

            // --- paint ---------------------------------------------------
            let painter = ui.painter();
            painter.rect_filled(rect, 3.0, theme.clip_body);
            if selected {
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.5, theme.clip_selected),
                    egui::StrokeKind::Middle,
                );
                // The edge strips, made visible only now they mean something.
                painter.line_segment(
                    [rect.left_top(), rect.left_bottom()],
                    egui::Stroke::new(1.5, theme.clip_selected),
                );
                painter.line_segment(
                    [rect.right_top(), rect.right_bottom()],
                    egui::Stroke::new(1.5, theme.clip_selected),
                );
            }

            if !clip.notes.is_empty() {
                let pitch_lo = clip.notes.iter().map(|n| n.pitch).min().unwrap_or(0);
                let pitch_hi = clip.notes.iter().map(|n| n.pitch).max().unwrap_or(0);
                for note in &clip.notes {
                    let r = note_rect(rect, note, pitch_lo, pitch_hi, clip.len).intersect(rect);
                    painter.rect_filled(r, 1.0, theme.clip_note);
                }
            }

            if let Some(r) = rename.as_mut().filter(|r| r.track == t && r.id == clip.id) {
                // The name label becomes the edit box, in place.
                let edit = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + CLIP_LABEL_PAD, rect.top() + CLIP_LABEL_PAD),
                    egui::vec2((rect.width() - 2.0 * CLIP_LABEL_PAD).max(80.0), 20.0),
                );
                let resp = ui.put(edit, egui::TextEdit::singleline(&mut r.text));
                if !r.focused {
                    resp.request_focus();
                    r.focused = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    rename_cancel = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) || resp.lost_focus() {
                    rename_commit = true;
                }
            } else if rect.width() >= CLIP_LABEL_MIN_W {
                painter.text(
                    egui::pos2(rect.left() + CLIP_LABEL_PAD, rect.top() + CLIP_LABEL_PAD),
                    egui::Align2::LEFT_TOP,
                    &clip.name,
                    egui::FontId::new(11.0, egui::FontFamily::Proportional),
                    theme.text,
                );
            }
        }
    }

    // --- the ghost, drawn above everything it might land on ---------------
    if let Some(g) = &ghost
        && let Some(lane) = lanes.get(g.track)
    {
        let r = clip_rect(content, offset, *lane, &g.clip).intersect(content);
        let painter = ui.painter();
        painter.rect_filled(r, 3.0, theme.clip_body.gamma_multiply(0.55));
        painter.rect_stroke(
            r,
            3.0,
            egui::Stroke::new(1.0, theme.clip_selected),
            egui::StrokeKind::Middle,
        );
        if r.width() >= CLIP_LABEL_MIN_W {
            painter.text(
                egui::pos2(r.left() + CLIP_LABEL_PAD, r.top() + CLIP_LABEL_PAD),
                egui::Align2::LEFT_TOP,
                &g.clip.name,
                egui::FontId::new(11.0, egui::FontFamily::Proportional),
                theme.text_muted,
            );
        }
    }

    // --- apply, after the draw borrow is done -----------------------------
    let index_of = |track: &[Clip], id: u64| track.iter().position(|c| c.id == id);

    if let Some((t, id, act)) = menu.get() {
        match act {
            ClipMenu::Copy => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    arr.clipboard = Some(arr.clips[t][i].clone());
                    arr.selected_clip = Some((t, i));
                }
            }
            ClipMenu::Duplicate => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    arr.selected_clip = Some((t, i));
                    arr.duplicate_selected();
                }
            }
            ClipMenu::Rename => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    arr.selected_clip = Some((t, i));
                    rename = Some(Rename {
                        track: t,
                        id,
                        text: arr.clips[t][i].name.clone(),
                        original: arr.clips[t][i].name.clone(),
                        focused: false,
                    });
                }
            }
            ClipMenu::Delete => {
                arr.remove_clip(t, id);
            }
        }
    }

    // Rename settlement: Escape restores the original, Enter or clicking
    // away keeps the edit — but never an empty name, whatever the key was.
    if rename_cancel {
        if let Some(r) = &rename
            && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
        {
            c.name = r.original.clone();
        }
        rename = None;
    } else if rename_commit {
        if rename.as_ref().is_some_and(|r| r.text.trim().is_empty())
            && let Some(r) = &rename
            && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
        {
            c.name = r.original.clone();
        }
        rename = None;
    } else if let Some(r) = &rename
        && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
    {
        // Live: the clip reads its new name while it is being typed.
        c.name = r.text.clone();
    }
    arr.rename = rename;

    if let Some((t, id)) = select
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        arr.selected_clip = Some((t, i));
    }
    if let Some((t, id, want)) = move_to
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        arr.clips[t][i].start = clamp_clip_start(&arr.clips[t], i, snap(want, grid));
        resort(&mut arr.clips[t]);
        if let Some(i) = index_of(&arr.clips[t], id) {
            arr.selected_clip = Some((t, i));
        }
    }
    if let Some((t, id, want)) = left_to
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        let start = clamp_clip_start(&arr.clips[t], i, snap(want, grid));
        let end = arr.clips[t][i].start + arr.clips[t][i].len;
        arr.clips[t][i].len = end - start;
        arr.clips[t][i].start = start;
    }
    if let Some((t, id, want)) = right_to
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        arr.clips[t][i].len = clamp_clip_len(&arr.clips[t], i, snap(want, grid), grid);
    }

    // The released ghost becomes a real clip: first fitting gap, selected.
    if let Some(g) = ghost_finalize {
        let (start, idx) = place_clip(&arr.clips[g.track], g.clip.start, g.clip.len);
        let mut placed = g.clip;
        placed.start = start;
        arr.clips[g.track].insert(idx, placed);
        arr.selected_clip = Some((g.track, idx));
    }

    arr.ghost = ghost;
}

/// The loop brace and its two handles, in the ruler strip.
///
/// Each end drags independently and snaps to the grid. They cannot be pulled
/// through each other: crossing would silently invert the loop, so the
/// dragged end stops one grid unit short of the other. The brace BODY drags
/// the whole loop — both ends at once, length untouched, snapped, and never
/// before beat 0.
fn loop_brace(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    ruler: egui::Rect,
    content: egui::Rect,
    arr: &mut Arrangement,
    grid: f32,
) {
    ui.painter().line_segment(
        [
            egui::pos2(ruler.left(), ruler.bottom()),
            egui::pos2(ruler.right(), ruler.bottom()),
        ],
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );

    let Some((from, to)) = arr.loop_range else {
        return;
    };
    let offset = arr.view_beats;
    let (x0, x1) = (x_at(content, offset, from), x_at(content, offset, to));
    let brace = egui::Rect::from_min_max(
        egui::pos2(x0, ruler.top() + 2.0),
        egui::pos2(x1, ruler.bottom() - 2.0),
    );

    // The brace body carries the WHOLE loop. Created before the handles, so
    // the handles win the pointer wherever they overlap the body.
    let body = ui.interact(brace, ui.id().with("loop_body"), egui::Sense::drag());
    if body.hovered() || body.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let mut next = (from, to);
    if body.dragged() {
        next = loop_move(next, snap(body.drag_delta().x / PX_PER_BEAT, grid));
    }
    let (hx0, hx1) = (x_at(content, offset, next.0), x_at(content, offset, next.1));
    for (which, x) in [(0usize, hx0), (1usize, hx1)] {
        let handle = egui::Rect::from_min_max(
            egui::pos2(x - LOOP_HANDLE_W * 0.5, ruler.top()),
            egui::pos2(x + LOOP_HANDLE_W * 0.5, ruler.bottom()),
        );
        let wid = ui.id().with(("loop_handle", which));
        focus.register(wid, handle);
        let response = ui.interact(handle, wid, egui::Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && response.dragged()
        {
            let at = snap(beat_at(content, offset, pos.x), grid);
            if which == 0 {
                next.0 = at.min(next.1 - grid).max(0.0);
            } else {
                next.1 = at.max(next.0 + grid);
            }
        }
    }
    arr.loop_range = Some(next);

    // Paint the brace LAST, at its final position for this frame — so both
    // handle drags and body drags feed back live instead of one frame late.
    let (nx0, nx1) = (x_at(content, offset, next.0), x_at(content, offset, next.1));
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(nx0, ruler.top() + 2.0),
            egui::pos2(nx1, ruler.bottom() - 2.0),
        )
        .intersect(ruler),
        2.0,
        theme.loop_brace,
    );
}

/// A top-level folder and its contents. Dummy data for now — the point is
/// the layout, not the library.
/// A browser entry that does something when activated. Only devices so
/// far — samples and presets arrive as more variants of what `load`
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrowserItem {
    name: &'static str,
    load: DeviceKind,
}

struct Folder {
    name: &'static str,
    items: &'static [BrowserItem],
    open: bool,
}

/// The instruments the app can actually build. Not a catalogue of
/// intentions — every row here loads.
const INSTRUMENTS: &[BrowserItem] = &[BrowserItem {
    name: "Sine Synth",
    load: DeviceKind::SineSynth,
}];

/// Effects: they shape whatever the track's instrument makes.
const FX: &[BrowserItem] = &[BrowserItem {
    name: "Reverb",
    load: DeviceKind::Reverb,
}];

/// Everything the browser owns that outlives a frame.
struct Browser {
    /// Where the two bands meet, as a fraction of panel height.
    split: f32,
    query: String,
    folders: Vec<Folder>,
}

impl Default for Browser {
    fn default() -> Self {
        // One folder, holding the instruments that really exist. Open,
        // because a browser whose only content is folded shut reads as
        // empty.
        Self {
            split: BROWSER_LOWER_FRAC,
            query: String::new(),
            folders: vec![
                Folder {
                    name: "Instruments",
                    items: INSTRUMENTS,
                    open: true,
                },
                Folder {
                    name: "Fx",
                    items: FX,
                    open: true,
                },
            ],
        }
    }
}

/// What the browser shows with nothing in it. The app's muted empty-state
/// line (`kit::empty_state`), painted rather than laid out because this
/// region draws with a painter throughout.
const BROWSER_EMPTY: &str = "no library yet";

/// The rows the tree currently shows, as (indent depth, text, is_folder).
///
/// Pure: the whole tree is derived from the folder list, so what is on
/// screen and what the click handler thinks is on screen cannot drift apart.
/// The last child of a folder gets the elbow, the rest get tees.
fn tree_rows(folders: &[Folder]) -> Vec<(String, bool)> {
    let mut rows = Vec::new();
    for folder in folders {
        let arrow = if folder.open { TREE_OPEN } else { TREE_SHUT };
        rows.push((format!("{arrow} {}", folder.name), true));
        if !folder.open {
            continue;
        }
        for (i, item) in folder.items.iter().enumerate() {
            let last = i + 1 == folder.items.len();
            let branch = if last { TREE_ELL } else { TREE_TEE };
            rows.push((format!("  {branch} {}", item.name), false));
        }
    }
    rows
}

/// What each visible row IS: a folder to toggle, or an item to load.
/// Built alongside `tree_rows` so the two can never drift out of step.
fn tree_targets(folders: &[Folder]) -> Vec<Option<BrowserItem>> {
    let mut out = Vec::new();
    for folder in folders {
        out.push(None); // the folder's own row
        if folder.open {
            out.extend(folder.items.iter().map(|i| Some(*i)));
        }
    }
    out
}

/// Draw the tree under the search well, and toggle a folder when its row is
/// clicked. Rows past the bottom of the band are simply not drawn.
fn tree(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    upper: egui::Rect,
    folders: &mut [Folder],
) -> Option<BrowserItem> {
    let font = egui::FontId::new(TREE_TYPE, egui::FontFamily::Monospace);
    let top = upper.top() + SEARCH_H + TREE_TOP_GAP;
    let rows = tree_rows(folders);

    // An empty browser must not look like a broken one.
    if rows.is_empty() {
        ui.painter().text(
            egui::pos2(upper.center().x, top + TREE_ROW_H),
            egui::Align2::CENTER_TOP,
            BROWSER_EMPTY,
            font,
            theme.text_muted,
        );
        return None;
    }
    let targets = tree_targets(folders);
    let mut load: Option<BrowserItem> = None;

    // Which folder each row belongs to, so a click knows what to toggle.
    let mut owner = Vec::new();
    for (i, folder) in folders.iter().enumerate() {
        owner.push(Some(i));
        if folder.open {
            owner.extend(std::iter::repeat_n(None, folder.items.len()));
        }
    }

    let mut toggle = None;
    for (index, (text, is_folder)) in rows.iter().enumerate() {
        let y = top + index as f32 * TREE_ROW_H;
        if y + TREE_ROW_H > upper.bottom() {
            break;
        }
        let row = egui::Rect::from_min_size(
            egui::pos2(upper.left(), y),
            egui::vec2(upper.width(), TREE_ROW_H),
        );

        // Every row is reachable by keyboard; only folders do anything when
        // pressed.
        let wid = ui.id().with(("tree_row", index));
        focus.register(wid, row);
        if *is_folder {
            let response = ui.interact(row, wid, egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 0.0, theme.accent_muted);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() || focus.activated(wid) {
                toggle = owner.get(index).copied().flatten();
            }
        } else if let Some(item) = targets.get(index).copied().flatten() {
            // An item loads: double-click with the mouse, Enter with the
            // keyboard ring. Single click only points at it — loading a
            // device is a commitment, and a stray click on a list should
            // not rewire a track.
            let response = ui.interact(row, wid, egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 0.0, theme.accent_muted);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.double_clicked() || focus.activated(wid) {
                load = Some(item);
            }
        }

        ui.painter().text(
            egui::pos2(row.left() + TREE_PAD_X, row.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            if *is_folder {
                theme.text
            } else {
                theme.text_muted
            },
        );
    }

    if let Some(i) = toggle {
        folders[i].open = !folders[i].open;
    }
    load
}

/// The search well at the top of the upper band: a darker rectangle holding
/// the magnifying glass and the field.
fn search_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    upper: egui::Rect,
    query: &mut String,
) {
    let well = egui::Rect::from_min_size(upper.min, egui::vec2(upper.width(), SEARCH_H));
    if well.width() <= SEARCH_PAD * 3.0 || upper.height() < SEARCH_H {
        // Too narrow to hold an icon and a field, or shorter than the well
        // itself — better absent than spilling out of its band.
        return;
    }
    // Enter on the well hands the keyboard to the field; Escape gives it back.
    let wid = ui.id().with("search");
    if focus.register(wid, well) && focus.activated(wid) {
        ui.ctx().memory_mut(|m| m.request_focus(wid.with("edit")));
    }
    ui.painter().rect_filled(well, 0.0, theme.surface_sunken);

    let icon_font = egui::FontId::new(SEARCH_TYPE, egui::FontFamily::Monospace);
    let icon_x = ui
        .painter()
        .text(
            egui::pos2(well.left() + SEARCH_PAD, well.center().y),
            egui::Align2::LEFT_CENTER,
            SEARCH_ICON,
            icon_font,
            theme.text_muted,
        )
        .right();

    // `ui.put` lays out centered_and_justified, so a field spanning the full
    // well stretches to 28px and draws its text at the TOP of that box. Give
    // it exactly one text row instead, centred on the well, and the justify
    // has nothing left to stretch.
    let text_font = egui::FontId::new(SEARCH_TYPE, egui::FontFamily::Proportional);
    let row = ui.ctx().fonts_mut(|f| f.row_height(&text_font));
    let field = centred_band(well, icon_x + SEARCH_PAD, well.right() - SEARCH_PAD, row);
    ui.put(
        field,
        egui::TextEdit::singleline(query)
            .id(wid.with("edit"))
            // The well is the background; a second frame on top of it would
            // read as a box inside a box, so hand it an empty one.
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .text_color(theme.text)
            .font(text_font),
    );
}

/// A `height`-tall strip spanning `left..right`, centred on `outer`'s middle.
///
/// Pure, so `the_search_field_is_centred_in_its_well` can check the centring
/// arithmetic without laying out any text.
fn centred_band(outer: egui::Rect, left: f32, right: f32, height: f32) -> egui::Rect {
    let mid = outer.center().y;
    egui::Rect::from_min_max(
        egui::pos2(left, mid - height * 0.5),
        egui::pos2(right, mid + height * 0.5),
    )
}

/// The browser's body: claim the space like every other region, then paint
/// its content area and let the user drag the divider between the bands.
///
/// The interaction happens BEFORE the paint so a drag lands on the same
/// frame it was made — reading it back afterwards would put the bands one
/// frame behind the pointer.
fn browser_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    browser: &mut Browser,
) -> Option<BrowserItem> {
    let area = ui.max_rect();
    claim(ui);

    let (upper, _) = browser_bands(area, browser.split)?;

    let grab = ui.style().interaction.resize_grab_radius_side;
    let hit = egui::Rect::from_min_max(
        egui::pos2(upper.left(), upper.bottom() - grab),
        egui::pos2(upper.right(), upper.bottom() + grab),
    );
    let response = ui.interact(hit, ui.id().with("browser_split"), egui::Sense::drag());
    if let Some(pointer) = response.interact_pointer_pos() {
        browser.split = split_from_pointer(area, pointer.y);
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    // Recomputed, because the drag above may have just moved the divider.
    let (upper, lower) = browser_bands(area, browser.split)?;
    // The two bands sit on the theme's own ramp: the upper one a step above
    // the panel behind, the lower one a step above that — which keeps the
    // original "felt, not read" split whatever the scheme's ground is.
    let painter = ui.painter();
    painter.rect_filled(upper, 0.0, theme.surface);
    painter.rect_filled(lower, 0.0, theme.surface_raised);

    search_bar(ui, theme, focus, upper, &mut browser.query);
    let load = tree(ui, theme, focus, upper, &mut browser.folders);

    let painter = ui.painter();
    // Same affordance as the panel edges: the divider is invisible until you
    // reach for it, then it is unmistakable.
    if response.hovered() || response.dragged() {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(upper.left(), upper.bottom() - SEAM_PX),
                egui::pos2(upper.right(), upper.bottom()),
            ),
            0.0,
            SEAM,
        );
    }
    load
}

/// A region's body: claim the whole area, draw nothing.
///
/// This is not cosmetic and it is not optional. An egui panel sizes itself to
/// its CONTENT, so a region with an empty body collapses to its minimum size
/// — and that collapsed rect is what gets persisted as the panel's state.
/// The next frame reads it back, so a drag on the resize handle is computed
/// correctly and then immediately discarded: the panel looks unresizable
/// while the handle is in fact working fine.
///
/// Claiming the space is what makes a region both the size it asked for and
/// draggable at all. Verified by `regions_honour_their_sizes` below.
fn claim(ui: &mut egui::Ui) {
    ui.allocate_space(ui.available_size());
}

/// The device region's body: the SELECTED track's rack. One track, one
/// instrument — its sine synth card — and an empty state when no track is
/// selected, because a rack belonging to nothing is not a rack.
///
/// Ends with `claim` for the same reason the other regions do: the leftover
/// space must be allocated or the region collapses to its content and the
/// resize handle stops holding.
/// What the device region produced this frame, kept apart by which device
/// made it: the two cards share a parameter NUMBERING but not a node, and
/// mixing them up would send a reverb's mix to a synth's gain.
#[derive(Default)]
struct DeviceEdits {
    synth: Vec<device::ParamEdit>,
    fx: Vec<device::ParamEdit>,
}

fn device_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    synth: Option<&mut device::SineSynthUi>,
    fx: Option<&mut device::ReverbUi>,
) -> DeviceEdits {
    let mut edits = DeviceEdits::default();
    // A rack grows rightward, so the region scrolls horizontally — and a
    // touchpad's two-finger VERTICAL swipe is translated onto that axis
    // too, because there is no vertical content to spend it on and "hover
    // the rack, swipe, it moves" is what the gesture means here. Wheel
    // users get the same courtesy for free.
    // auto_shrink off does the claiming here: the scroll area fills the
    // whole region, so the region keeps its size and its resize handle —
    // a `claim` INSIDE the scroll content would instead ask for the
    // content's available width, which is infinite on the scroll axis.
    egui::ScrollArea::horizontal()
        .id_salt("device_rack")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(
                    theme.sp(daw::ui::tokens::space::SM) as i8
                ))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        // The selected track's chain, left to right in
                        // signal order: instrument first, then its effect.
                        // Edits leave as (param id, natural value) data;
                        // the app layer turns them into engine letters
                        // addressed to THIS track's nodes.
                        if synth.is_none() && fx.is_none() {
                            daw::ui::kit::empty_state(ui, theme, DEVICE_EMPTY);
                            return;
                        }
                        if let Some(synth) = synth {
                            edits.synth = device::sine_synth_card(ui, theme, synth);
                        }
                        if let Some(fx) = fx {
                            edits.fx = device::reverb_card(ui, theme, fx);
                        }
                    });
                });
            // Vertical wheel becomes horizontal rack scroll — read AFTER
            // the cards, so a wheel a hovered control already consumed
            // (device::adjust zeroes it) no longer moves the rack too.
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 && ui.rect_contains_pointer(ui.max_rect()) {
                ui.scroll_with_delta(egui::vec2(dy, 0.0));
            }
        });
    edits
}

/// The browser's right edge: a hairline drawn just inside the region, so the
/// mark belongs to the chrome rather than straddling the boundary.
fn vertical_seam(browser: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(browser.right() - SEAM_PX, browser.top()),
        egui::pos2(browser.right(), browser.bottom()),
    )
}

/// The device panel's top edge, same rule.
fn horizontal_seam(device: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(device.left(), device.top()),
        egui::pos2(device.right(), device.top() + SEAM_PX),
    )
}

/// The id egui files a panel's resize handle under.
///
/// This mirrors egui's private `resize_widget_id`, which is
/// `id.with("__resize")`. Reproducing a private detail is a real risk, so
/// `the_resize_handle_id_is_the_one_we_paint_against` asserts it still
/// resolves — without that test, an upstream rename would make the seams
/// silently stop appearing and nothing would fail.
fn handle_id(panel_id: &str) -> egui::Id {
    egui::Id::new(panel_id).with("__resize")
}

/// Is this panel's edge being pointed at or pulled?
///
/// Asking egui rather than testing the pointer against the seam ourselves is
/// what keeps the mark lit through a drag that runs past the size clamp — at
/// that point the pointer has left the edge, but the drag is still live.
fn seam_active(ctx: &egui::Context, panel_id: &str) -> bool {
    ctx.read_response(handle_id(panel_id))
        .is_some_and(|r| r.hovered() || r.dragged())
}

fn seam(ui: &egui::Ui, panel_id: &str, rect: egui::Rect) {
    if seam_active(ui.ctx(), panel_id) {
        ui.painter().rect_filled(rect, 0.0, SEAM);
    }
}

/// What the bottom region shows. Shift+Tab flips it. Two panel IDS, not
/// one: egui persists a panel's size under its id, and the piano roll wants
/// far more height than the rack — sharing an id would make each mode
/// inherit the other's size every switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BottomView {
    Rack,
    PianoRoll,
}

impl BottomView {
    fn toggled(self) -> Self {
        match self {
            Self::Rack => Self::PianoRoll,
            Self::PianoRoll => Self::Rack,
        }
    }

    /// The panel id the current mode draws (and persists its size) under.
    fn panel_id(self) -> &'static str {
        match self {
            Self::Rack => "device",
            Self::PianoRoll => "piano_roll",
        }
    }
}

/// One track's clips -> one Seq's note list, in ABSOLUTE timeline beats.
///
/// Clip-relative becomes absolute here and nowhere else: a note sounds at
/// `clip.start + note.start`. A note starting at or past the clip's end does
/// not sound at all — editing a note never resizes its clip, so a clip
/// shortened over its notes hides them rather than destroying them. A note
/// that STARTS inside the clip keeps its full length even if it rings past
/// the end: once struck, it belongs to the timeline.
fn seq_notes(clips: &[Clip]) -> Vec<SeqNote> {
    let mut out = Vec::new();
    for clip in clips {
        let start = f64::from(clip.start);
        let len = f64::from(clip.len);
        out.extend(
            clip.notes
                .iter()
                .filter(|n| n.start < len)
                .map(|n| SeqNote {
                    start_beats: start + n.start,
                    len_beats: n.len,
                    pitch: n.pitch,
                    vel: n.vel,
                }),
        );
    }
    out
}

/// The app's graph: one Seq per track carrying that track's clips and its
/// own synth params, a Click when the metronome is on, everything summed by
/// a Mixer that feeds the speakers.
///
/// Pure construction — compiling and swapping stay with the caller. Returns
/// the Seq ids BY TRACK INDEX, which is how a knob turn on track 2 finds the
/// node that plays track 2. A track with no clips still gets a Seq: a Seq
/// with no events is silent, and keeping the shape stable means a track's
/// letters have somewhere to land the moment it gains a clip.
/// The addressable nodes a graph build hands back, each vec indexed BY
/// TRACK with `None` where that track has no such node.
///
/// This is what makes a knob turn a letter instead of a recompile: a param
/// change has to know which node belongs to which lane, and every swap
/// mints fresh ids, so the mapping is re-captured with the schedule rather
/// than derived later.
#[derive(Debug, Default)]
struct GraphNodes {
    seqs: Vec<Option<NodeId>>,
    fx: Vec<Option<NodeId>>,
    pans: Vec<Option<NodeId>>,
}

fn build_graph_spec(
    tracks: &[Track],
    clips: &[Vec<Clip>],
    loop_len_beats: Option<f64>,
    metronome: bool,
) -> (GraphSpec, GraphNodes) {
    let mut spec = GraphSpec::default();
    let mixer = spec.push(NodeSpec::Mixer { gain: 1.0 });
    // Only tracks holding an instrument become sequencer nodes. An empty
    // track is silent by ABSENCE rather than by a muted node — the graph
    // stays as small as the session really is.
    let mut fx_ids: Vec<Option<NodeId>> = vec![None; tracks.len()];
    let mut pan_ids: Vec<Option<NodeId>> = vec![None; tracks.len()];
    let mut seqs: Vec<Option<NodeId>> = Vec::with_capacity(tracks.len());
    for (i, track) in tracks.iter().enumerate() {
        seqs.push((|| {
            track.device?;
            // An AUDIO track has no instrument slot to fill, so it makes no
            // sequencer either: its material is the sound, and streaming it
            // is the AudioClip work that has not landed yet. Wiring one now
            // would be a lane that looks live and is not.
            if !track.kind.takes_instrument() {
                return None;
            }
            // Silence is ABSENCE here too — a muted or solo-excluded track
            // is left out of the schedule rather than multiplied by zero.
            if !track_audible(tracks, i) {
                return None;
            }
            let seq = spec.push(NodeSpec::Seq {
                notes: clips.get(i).map(|c| seq_notes(c)).unwrap_or_default(),
                subloops: Vec::new(),
                loop_len_beats,
                params: track.params,
            });
            // The effect sits BETWEEN the instrument and the mixer, so the
            // track's own signal is what gets processed — not the sum of
            // every track, which is what a reverb on the master would be.
            let tail = match track.fx {
                Some(DeviceKind::Reverb) => {
                    let rev = spec.push(NodeSpec::Reverb {
                        size: track.reverb.size,
                        damp: track.reverb.damp,
                        mix: track.reverb.mix,
                    });
                    spec.connect(seq, rev);
                    fx_ids[i] = Some(rev);
                    rev
                }
                _ => seq,
            };
            // Pan is ALWAYS a node, even at dead center, and that is
            // deliberate: it gives every track's pan a permanent address,
            // so turning the header knob is a param letter rather than a
            // schedule swap. A centered constant-power pan is two
            // multiplies; a swap per mouse-move is a recompile per frame.
            let pan = spec.push(NodeSpec::Pan { pan: track.pan });
            spec.connect(tail, pan);
            spec.connect(pan, mixer);
            pan_ids[i] = Some(pan);
            Some(seq)
        })());
    }
    if metronome {
        let click = spec.push(NodeSpec::Click);
        spec.connect(click, mixer);
    }
    spec.set_output(mixer);
    (
        spec,
        GraphNodes {
            seqs,
            fx: fx_ids,
            pans: pan_ids,
        },
    )
}

/// Should the schedule be rebuilt this frame? Only when what the graph is
/// built FROM genuinely differs from what it was built from, and at most
/// once per `RECOMPILE_MIN_SECS` — so a drag coalesces into one
/// whole-schedule swap.
///
/// Pure, so the debounce is checkable without a clock.
fn recompile_due(dirty: bool, since_last_compile: f64) -> bool {
    dirty && since_last_compile >= RECOMPILE_MIN_SECS
}

struct App {
    theme: Theme,
    /// Machine-local preferences, loaded at startup and handed back to
    /// eframe on `save` — density lives here, so a packing choice survives
    /// the session.
    prefs: UiPrefs,
    /// Transport state. The engine's transport is the master while a stream
    /// runs (the mirror is refreshed from telemetry each frame); with the
    /// engine off this is the stand-in the app falls back to.
    transport: Transport,
    browser: Browser,
    arrangement: Arrangement,
    /// The piano roll's VIEW state — cursor, grid rung, scroll, selection.
    /// The notes it edits live in the selected clip, not here.
    piano_roll: piano_roll::PianoRoll,
    /// Which face the bottom region shows.
    bottom_view: BottomView,
    /// Which region the keyboard is pointing at.
    focus: Focus,
    /// The command palette. `:` opens it; while open it owns the keyboard.
    palette: Palette,
    /// The theme window: every Gogh scheme, picked live. Opened from the
    /// palette's "change theme"; while open it owns the keyboard the same
    /// way the palette does.
    skin: Skin,

    // --- the engine, owned here and nowhere else ---------------------------
    /// The running stream, if any. Dropping it stops and closes the stream.
    engine: Option<Engine>,
    /// What the top bar's engine slot shows when something went wrong:
    /// a failed start, a dead stream. Cleared by a successful start.
    notice: Option<String>,
    /// Live numbers for the bar, refreshed from telemetry each frame.
    hud: Option<EngineHud>,
    /// The Seq node of each track in the CURRENT schedule, by track index.
    /// Every recompile yields fresh ids, so this is re-captured on every
    /// swap — a letter must never be addressed to a retired node.
    seq_ids: Vec<Option<NodeId>>,
    /// Each track's EFFECT node in the current schedule, by track index.
    /// Re-captured on every swap, exactly like `seq_ids`.
    fx_ids: Vec<Option<NodeId>>,
    /// Each track's PAN node, by track index. Every instrument track has
    /// one, so a header knob is always addressable without a recompile.
    pan_ids: Vec<Option<NodeId>>,
    /// The pan value last SENT to each track's node. Comparing against it
    /// is what makes the knob, the palette's nudge verbs and a loaded
    /// project all reconcile through one door — whoever moved pan, the
    /// letter goes out once and only on a real change.
    sent_pan: Vec<f32>,
    /// The clips the current schedule was compiled from — the dirty check —
    /// and when it was compiled — the debounce clock.
    compiled_clips: Vec<Vec<Clip>>,
    last_compile: Option<Instant>,
    /// The graph's SHAPE as compiled: (metronome, loop_len_beats, tracks).
    /// A change here swaps the schedule immediately, no debounce — the graph
    /// gained or lost a node, which no param letter can express.
    graph_key: (bool, Option<f64>, usize, u64),
    /// The transport loop last sent, in samples — resent only on change, so
    /// brace drags, Ctrl+L and tempo changes all reconcile through one door.
    sent_loop: Option<(u64, u64)>,
}

/// What the device region shows when there is no device to show: no track
/// selected, or a selected track with nothing loaded on it. One line for
/// both, because the fix is the same — pick a track, load an instrument.
const DEVICE_EMPTY: &str = "no device — load one from the browser";

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(ZOOM);
        // Machine-local preferences. A corrupt or missing blob falls back to
        // defaults — preferences are never worth failing a launch over.
        let prefs: UiPrefs = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, STORAGE_KEY))
            .unwrap_or_default();
        let mut theme = Theme::dark();
        let mut skin = Skin::default();
        // The kept theme, if any, is restored before anything draws:
        // startup wears the last choice, not the default.
        skin.restore(&mut theme);
        // Density arrives from prefs, not from a project — a scheme is
        // colours only, and a swap must not re-pack the interface.
        theme.set_density(prefs.density);
        theme.apply(&cc.egui_ctx);
        Self {
            theme,
            prefs,
            transport: Transport::default(),
            browser: Browser::default(),
            arrangement: Arrangement::default(),
            piano_roll: piano_roll::PianoRoll::default(),
            bottom_view: BottomView::Rack,
            focus: Focus::default(),
            palette: Palette::default(),
            skin,
            engine: None,
            notice: None,
            hud: None,
            seq_ids: Vec::new(),
            fx_ids: Vec::new(),
            pan_ids: Vec::new(),
            sent_pan: Vec::new(),
            compiled_clips: Vec::new(),
            last_compile: None,
            graph_key: (false, None, 0, 0),
            sent_loop: None,
        }
    }

    /// A region is a flat fill and nothing else. One fill per region, drawn
    /// edge to edge — no inner margin, so the boxes read as boxes.
    fn fill(&self, color: egui::Color32) -> egui::Frame {
        egui::Frame::new().fill(color)
    }

    /// The Seq's clip length: the loop region's END beat while looping is on,
    /// so the pattern's cycle and the transport's wrap agree; None otherwise
    /// (one-shot against the rolling timeline).
    fn loop_len_beats(&self) -> Option<f64> {
        if !self.transport.loop_on {
            return None;
        }
        self.arrangement.loop_range.map(|(_, to)| f64::from(to))
    }

    /// Open the stream and put the current graph on it. Failure lands in the
    /// bar's notice slot; the app stays usable on the stand-in clock.
    /// Everything the palette can run, in authored order — which is also
    /// the order an empty query shows. Grouped by what the verb acts on,
    /// most-reached-for group first.
    ///
    /// `enabled` reflects the CURRENT state, so a verb that needs a
    /// selection or an engine reads as unavailable instead of failing
    /// silently when it is run. Nothing is ever hidden: the palette is a
    /// map of what the app can do, not a shifting subset of it.
    fn commands(&self) -> Vec<PaletteCommand> {
        let on = self.engine.is_some();
        let has_track = self.arrangement.active_track().is_some();
        let has_clip = self.arrangement.selected_clip.is_some();
        let roll = self.bottom_view == BottomView::PianoRoll;
        vec![
            // --- transport ---------------------------------------------
            PaletteCommand::new("transport.play", "transport", "play / stop").hint("space"),
            PaletteCommand::new("transport.return", "transport", "return to zero"),
            PaletteCommand::new("transport.metronome", "transport", "toggle metronome"),
            PaletteCommand::new("transport.loop", "transport", "toggle loop"),
            PaletteCommand::new("transport.follow", "transport", "toggle follow playhead"),
            // --- clip --------------------------------------------------
            PaletteCommand::new("clip.new", "clip", "new clip at cursor")
                .enabled(self.arrangement.cursor.is_some()),
            PaletteCommand::new("clip.duplicate", "clip", "duplicate clip").enabled(has_clip),
            PaletteCommand::new("clip.delete", "clip", "delete clip").enabled(has_clip),
            PaletteCommand::new("clip.loop", "clip", "loop the selection"),
            // --- tracks ------------------------------------------------
            PaletteCommand::new("track.new.audio", "track", "new audio track").hint("ctrl+T"),
            PaletteCommand::new("track.new.midi", "track", "new MIDI track").hint("ctrl+shift+T"),
            PaletteCommand::new("track.rename", "track", "rename track").enabled(has_track),
            PaletteCommand::new("track.mute", "track", "mute / unmute track").enabled(has_track),
            PaletteCommand::new("track.solo", "track", "solo / unsolo track").enabled(has_track),
            // Pan has no gesture of its own: the header knob is the mouse
            // route, and these three are the keyboard's.
            PaletteCommand::new("track.pan.left", "track", "pan left").enabled(has_track),
            PaletteCommand::new("track.pan.right", "track", "pan right").enabled(has_track),
            PaletteCommand::new("track.pan.center", "track", "center pan").enabled(has_track),
            PaletteCommand::new("track.delete", "track", "delete track")
                .enabled(has_track && self.arrangement.tracks.len() > 1),
            // --- notes: write at the piano roll's cursor ---------------
            PaletteCommand::new("note.chord.major", "chord", "major triad at cursor").enabled(roll),
            PaletteCommand::new("note.chord.minor", "chord", "minor triad at cursor").enabled(roll),
            PaletteCommand::new("note.chord.maj7", "chord", "major 7th at cursor").enabled(roll),
            PaletteCommand::new("note.chord.min7", "chord", "minor 7th at cursor").enabled(roll),
            PaletteCommand::new("note.chord.dom7", "chord", "dominant 7th at cursor").enabled(roll),
            PaletteCommand::new("note.chord.dim", "chord", "diminished triad at cursor")
                .enabled(roll),
            PaletteCommand::new("note.chord.sus4", "chord", "sus4 at cursor").enabled(roll),
            PaletteCommand::new("note.chord.diatonic", "chord", "diatonic triad in key")
                .enabled(roll),
            // --- view: the roll's own scale ----------------------------
            PaletteCommand::new("roll.zoom.in", "zoom", "zoom in").enabled(roll),
            PaletteCommand::new("roll.zoom.out", "zoom", "zoom out").enabled(roll),
            PaletteCommand::new("roll.zoom.reset", "zoom", "reset zoom").enabled(roll),
            // --- key ---------------------------------------------------
            PaletteCommand::new("key.tonic", "key", "set tonic from cursor").enabled(roll),
            PaletteCommand::new("key.major", "key", "scale: major"),
            PaletteCommand::new("key.minor", "key", "scale: minor"),
            PaletteCommand::new("key.dorian", "key", "scale: dorian"),
            PaletteCommand::new("key.mixolydian", "key", "scale: mixolydian"),
            PaletteCommand::new("key.pentatonic", "key", "scale: pentatonic minor"),
            PaletteCommand::new("key.snap", "key", "snap selection into key").enabled(roll),
            // --- notes: transform the selection ------------------------
            PaletteCommand::new("note.arp.up", "arp", "arpeggiate selection up").enabled(roll),
            PaletteCommand::new("note.arp.down", "arp", "arpeggiate selection down").enabled(roll),
            PaletteCommand::new("note.arp.updown", "arp", "arpeggiate selection up-down")
                .enabled(roll),
            PaletteCommand::new("note.counter.above", "counterpoint", "counter-melody above")
                .enabled(roll),
            PaletteCommand::new("note.counter.below", "counterpoint", "counter-melody below")
                .enabled(roll),
            // --- grid --------------------------------------------------
            PaletteCommand::new("grid.narrow", "grid", "narrow grid"),
            PaletteCommand::new("grid.widen", "grid", "widen grid"),
            // --- view --------------------------------------------------
            PaletteCommand::new("view.roll", "view", "show piano roll").enabled(!roll),
            PaletteCommand::new("view.rack", "view", "show device rack").enabled(roll),
            PaletteCommand::new("view.compact", "view", "density: compact"),
            PaletteCommand::new("view.comfortable", "view", "density: comfortable"),
            PaletteCommand::new("view.theme", "view", "change theme"),
            // --- device ------------------------------------------------
            PaletteCommand::new("device.reset", "device", "reset device knobs").enabled(has_track),
            // --- engine ------------------------------------------------
            PaletteCommand::new("engine.start", "engine", "start audio engine").enabled(!on),
            PaletteCommand::new("engine.stop", "engine", "stop audio engine").enabled(on),
        ]
    }

    /// Perform one palette verb. Actions that already exist in the
    /// `UiAction` vocabulary are pushed onto the frame's action list so
    /// they travel the same path a button press does — the palette is
    /// another way to SAY things, not a second way to do them. Only
    /// app-local verbs with no action are handled inline.
    fn run_command(&mut self, id: &'static str, actions: &mut Vec<UiAction>) {
        match id {
            "transport.play" => actions.push(UiAction::TogglePlay),
            "transport.return" => actions.push(UiAction::Return),
            "transport.metronome" => actions.push(UiAction::ToggleMetronome),
            "transport.loop" => actions.push(UiAction::ToggleLoop),
            "transport.follow" => actions.push(UiAction::ToggleFollow),

            "clip.new" => {
                // At the arrangement's keyboard cursor, one bar long — the
                // create path the palette exists to give an empty session.
                if let Some((track, beat)) = self.arrangement.cursor {
                    let len = self.transport.beats_per_bar as f32;
                    self.arrangement.create_clip(track, beat, len);
                }
            }
            "clip.duplicate" => actions.push(UiAction::DuplicateClip),
            "clip.delete" => actions.push(UiAction::DeleteSelected),
            "clip.loop" => actions.push(UiAction::LoopFromSelection),

            "track.new.audio" => actions.push(UiAction::AddTrack(TrackKind::Audio)),
            "track.new.midi" => actions.push(UiAction::AddTrack(TrackKind::Midi)),
            "track.mute" => actions.push(UiAction::ToggleTrackMute),
            "track.solo" => actions.push(UiAction::ToggleTrackSolo),
            "track.pan.left" => actions.push(UiAction::NudgeTrackPan(-PAN_STEP)),
            "track.pan.right" => actions.push(UiAction::NudgeTrackPan(PAN_STEP)),
            "track.pan.center" => actions.push(UiAction::CenterTrackPan),
            "track.delete" => actions.push(UiAction::RemoveTrack),
            "track.rename" => {
                // The palette opens the SAME inline editor a double-click
                // on the header does — one rename path, not two.
                if let Some(i) = self.arrangement.active_track()
                    && let Some(t) = self.arrangement.tracks.get(i)
                {
                    let name = t.name.clone();
                    self.arrangement.selected = Some(i);
                    self.arrangement.track_rename = Some(TrackRename {
                        track: i,
                        text: name.clone(),
                        original: name,
                        focused: false,
                    });
                }
            }

            // Note verbs all act on the selected clip through the roll's
            // cursor and selection. Each rewrites the note list once, so
            // each is one undo step when undo arrives.
            id if id.starts_with("note.") => self.run_note_command(id),

            "roll.zoom.in" => self.piano_roll.zoom = self.piano_roll.zoom.stepped(2),
            "roll.zoom.out" => self.piano_roll.zoom = self.piano_roll.zoom.stepped(-2),
            "roll.zoom.reset" => self.piano_roll.zoom = piano_roll::Zoom::default(),

            "key.major" => self.arrangement.key.scale = daw::theory::Scale::Major,
            "key.minor" => self.arrangement.key.scale = daw::theory::Scale::NaturalMinor,
            "key.dorian" => self.arrangement.key.scale = daw::theory::Scale::Dorian,
            "key.mixolydian" => self.arrangement.key.scale = daw::theory::Scale::Mixolydian,
            "key.pentatonic" => self.arrangement.key.scale = daw::theory::Scale::PentatonicMinor,
            "key.tonic" => {
                // The cursor names the key: park on the note that feels
                // like home and say so, instead of picking from twelve.
                self.arrangement.key.tonic = self.piano_roll.cursor_pitch % 12;
            }
            "key.snap" => {
                let key = self.arrangement.key;
                let selected = self.piano_roll.selected.clone();
                if let Some(clip) = self.arrangement.active_clip() {
                    for i in selected {
                        if let Some(n) = clip.notes.get_mut(i) {
                            n.pitch = key.scale.snap(key.tonic, n.pitch);
                        }
                    }
                }
            }

            "grid.narrow" => actions.push(UiAction::NarrowGrid),
            "grid.widen" => actions.push(UiAction::WidenGrid),

            "view.roll" => self.bottom_view = BottomView::PianoRoll,
            "view.rack" => self.bottom_view = BottomView::Rack,
            "view.compact" => actions.push(UiAction::SetDensity(Density::Compact)),
            "view.comfortable" => actions.push(UiAction::SetDensity(Density::Comfortable)),
            "view.theme" => self.skin.open(),

            "device.reset" => {
                if let Some(t) = self.arrangement.active_track() {
                    let fresh = device::SineSynthUi::default();
                    self.arrangement.tracks[t].synth = fresh;
                    let edits = device::sine_synth_edits(&fresh);
                    self.apply_param_edits(t, &edits);
                }
            }

            "engine.start" => self.start_engine(),
            "engine.stop" => self.stop_engine(),

            _ => {}
        }
    }

    /// The generative note verbs. They need the roll's cursor and selection
    /// AND the selected clip's notes, which is why they are not plain
    /// `UiAction`s: no other input method can express "at the cursor, in
    /// this clip".
    fn run_note_command(&mut self, id: &'static str) {
        use daw::theory::Quality;
        let grid = self.piano_roll.grid_beats();
        let (pitch, beat) = (self.piano_roll.cursor_pitch, self.piano_roll.cursor_beat);
        let selected = self.piano_roll.selected.clone();
        // Read before the clip borrow: the key belongs to the arrangement.
        let key = self.arrangement.key;
        let Some(clip) = self.arrangement.active_clip() else {
            return;
        };
        let notes = &mut clip.notes;

        let quality = match id {
            "note.chord.major" => Some(Quality::Major),
            "note.chord.minor" => Some(Quality::Minor),
            "note.chord.maj7" => Some(Quality::Major7),
            "note.chord.min7" => Some(Quality::Minor7),
            "note.chord.dom7" => Some(Quality::Dominant7),
            "note.chord.dim" => Some(Quality::Diminished),
            "note.chord.sus4" => Some(Quality::Sus4),
            _ => None,
        };
        if id == "note.chord.diatonic" {
            // Quality comes from the DEGREE, not from the caller: a triad
            // on the second degree of a major key is minor, and the user
            // never has to know that to get it right.
            let added: Vec<usize> = key
                .scale
                .diatonic_triad(key.tonic, pitch)
                .into_iter()
                .map(|p| {
                    notes.push(Note {
                        pitch: p,
                        start: beat,
                        len: grid,
                        vel: 100,
                    });
                    notes.len() - 1
                })
                .collect();
            self.piano_roll.selected = added.into_iter().collect();
            return;
        }

        if let Some(q) = quality {
            // A written chord becomes the selection, so the very next verb
            // (arpeggiate, counterpoint) acts on what was just made.
            let added = piano_roll::chord_at(notes, pitch, beat, grid, q);
            self.piano_roll.selected = added.into_iter().collect();
            return;
        }

        match id {
            "note.arp.up" | "note.arp.down" | "note.arp.updown" => {
                let dir = match id {
                    "note.arp.up" => piano_roll::ArpDirection::Up,
                    "note.arp.down" => piano_roll::ArpDirection::Down,
                    _ => piano_roll::ArpDirection::UpDown,
                };
                if piano_roll::arpeggiate(notes, &selected, grid, dir) {
                    // The old indices are gone; selecting the wrong notes is
                    // worse than selecting none.
                    self.piano_roll.selected.clear();
                }
            }
            "note.counter.above" | "note.counter.below" => {
                let above = id.ends_with("above");
                let added = piano_roll::counterpoint(notes, &selected, above, key);
                self.piano_roll.selected = added.into_iter().collect();
            }
            _ => {}
        }
    }

    /// Put a browser item's device on a track, and select that track so the
    /// device region immediately shows what just landed.
    ///
    /// Target is the active track, or the first one when nothing is
    /// selected — activating a device and watching nothing happen reads as
    /// broken, and "it went to track 1, which is now selected" is both
    /// visible and undoable by loading it somewhere else.
    fn load_device(&mut self, item: BrowserItem) {
        let track = self.arrangement.active_track().unwrap_or(0);
        let Some(t) = self.arrangement.tracks.get_mut(track) else {
            return;
        };
        // Each device fills its own slot: an effect never displaces the
        // instrument that feeds it.
        if item.load.is_instrument() {
            // An audio track's sound IS its material — an instrument on
            // one would fill a slot the graph never reads. Refuse in
            // words rather than silently.
            if !t.kind.takes_instrument() {
                let name = t.name.clone();
                self.notice = Some(format!("{name} is an audio track — no instrument slot"));
                return;
            }
            t.device = Some(item.load);
            // Fresh knobs for a fresh instrument, and the engine hears them
            // on the next swap — which the shape change forces immediately.
            t.synth = device::SineSynthUi::default();
            t.params = SynthParams::default();
        } else {
            t.fx = Some(item.load);
            t.reverb = device::ReverbUi::default();
        }
        self.arrangement.selected = Some(track);
    }

    fn shape_hash(&self) -> u64 {
        shape_hash(&self.arrangement.tracks)
    }

    fn start_engine(&mut self) {
        if self.engine.is_some() {
            return;
        }
        match Engine::start(EngineConfig::default()) {
            Ok(mut engine) => {
                engine.transport(TransportCmd::SetTempo(self.transport.bpm));
                self.engine = Some(engine);
                self.notice = None;
                self.sent_loop = None;
                self.push_graph();
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Close the stream. Dropping the Engine stops it; the transport mirror
    /// halts so the stand-in clock does not sprint off from where audio died.
    fn stop_engine(&mut self) {
        self.engine = None;
        self.hud = None;
        self.seq_ids.clear();
        self.fx_ids.clear();
        self.sent_loop = None;
        self.last_compile = None;
        self.transport.playing = false;
    }

    /// Compile the arrangement into a schedule and swap it in WHOLE —
    /// sequencing contract rule 4: compiled immutable chunks, never streamed.
    fn push_graph(&mut self) {
        let Some(info) = self.engine.as_ref().map(|e| e.info()) else {
            return;
        };
        let loop_len = self.loop_len_beats();
        let (spec, nodes) = build_graph_spec(
            &self.arrangement.tracks,
            &self.arrangement.clips,
            loop_len,
            self.transport.metronome,
        );
        match spec.compile(info.sample_rate, info.max_frames) {
            Ok(sched) => {
                let Some(engine) = &mut self.engine else {
                    return;
                };
                match engine.set_schedule(Box::new(sched)) {
                    Ok(()) => {
                        self.seq_ids = nodes.seqs;
                        self.fx_ids = nodes.fx;
                        self.pan_ids = nodes.pans;
                        // Fresh ids: every track's pan must be re-sent, so
                        // nothing survives a swap sitting at the node's
                        // compiled-in default while the knob says otherwise.
                        self.sent_pan.clear();
                        self.compiled_clips = self.arrangement.clips.clone();
                        self.graph_key = (
                            self.transport.metronome,
                            loop_len,
                            self.arrangement.tracks.len(),
                            self.shape_hash(),
                        );
                        self.last_compile = Some(Instant::now());
                    }
                    Err(e) => self.notice = Some(e.to_string()),
                }
            }
            Err(e) => self.notice = Some(format!("graph refused: {e}")),
        }
    }

    /// Once per frame while a stream runs: retire trashed schedules, mirror
    /// the transport from telemetry, refresh the bar's numbers, judge health.
    /// Runs before anything draws, so the frame shows current numbers.
    fn pump_engine(&mut self, ctx: &egui::Context) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.collect_trash();
        let info = engine.info();
        let snap = engine.latest_block();
        self.transport.playing = snap.playing;
        self.transport.position = snap.position as f64 / f64::from(info.sample_rate.max(1));
        self.hud = Some(EngineHud {
            load_pct: (snap.load(info.sample_rate, info.max_frames as u32) * 100.0) as f32,
            xruns: snap.underflows + snap.overflows,
        });
        match engine.health() {
            StreamHealth::Running => {}
            StreamHealth::Stalled { seconds } => {
                self.notice = Some(format!(
                    "stream dead — no blocks for {seconds:.1}s; power off and on to reconnect"
                ));
            }
            StreamHealth::Errored(err) => {
                self.notice = Some(format!(
                    "stream error — {err}; power off and on to reconnect"
                ));
            }
        }
        // Telemetry only moves if frames keep coming.
        ctx.request_repaint();
    }

    /// Translate this frame's transport wishes into engine commands. Runs
    /// BEFORE `perform` flips the mirror, so TogglePlay reads the state the
    /// user saw. The mirror is still updated by `perform` — it is what the
    /// buttons light from, and the whole transport when the engine is off.
    fn route_transport(&mut self, actions: &[UiAction]) {
        let playing = self.transport.playing;
        let Some(engine) = &mut self.engine else {
            return;
        };
        for action in actions {
            match action {
                UiAction::TogglePlay => engine.transport(if playing {
                    TransportCmd::Stop
                } else {
                    TransportCmd::Play
                }),
                // Halt, hold — the engine's Stop.
                UiAction::Pause => engine.transport(TransportCmd::Stop),
                // Halt AND rewind — the engine's Return.
                UiAction::Stop => engine.transport(TransportCmd::Return),
                // Rewind without halting: a seek leaves `playing` alone.
                UiAction::Return => engine.transport(TransportCmd::Seek(0)),
                UiAction::SetTempo(bpm) => engine.transport(TransportCmd::SetTempo(
                    bpm.clamp(limits::BPM_MIN, limits::BPM_MAX),
                )),
                _ => {}
            }
        }
    }

    /// End of frame: reconcile the loop region and the schedule with what
    /// the UI now says. One door for every way the loop or graph can change
    /// (buttons, Ctrl+L, brace drags, tempo, note edits).
    fn sync_engine(&mut self) {
        if self.engine.is_none() {
            return;
        }
        // Loop points ride beats -> samples through the engine's own rule:
        // 60/bpm * rate, rounded — same formula, same rounding (TimeMap).
        let want = if self.transport.loop_on {
            self.arrangement.loop_range.map(|(from, to)| {
                let spb = 60.0 / self.transport.bpm
                    * f64::from(self.engine.as_ref().map_or(0, |e| e.info().sample_rate));
                (
                    (f64::from(from) * spb).round() as u64,
                    (f64::from(to) * spb).round() as u64,
                )
            })
        } else {
            None
        };
        if want != self.sent_loop {
            if let Some(engine) = &mut self.engine {
                match want {
                    Some((start, end)) => engine.transport(TransportCmd::SetLoop { start, end }),
                    None => engine.transport(TransportCmd::ClearLoop),
                }
            }
            self.sent_loop = want;
        }

        self.sync_pans();

        // Shape changes (metronome, loop length, track count) swap now: they
        // add or remove nodes, which no letter can express. Clip edits are
        // debounced so a drag lands as one swap, not sixty.
        let shape = (
            self.transport.metronome,
            self.loop_len_beats(),
            self.arrangement.tracks.len(),
            self.shape_hash(),
        );
        if shape != self.graph_key {
            self.push_graph();
            return;
        }
        let since = self
            .last_compile
            .map_or(f64::INFINITY, |t| t.elapsed().as_secs_f64());
        // The dirty check covers every way the sounding material can move:
        // notes edited, clips created, deleted, dragged, resized, renamed.
        // Synth params are deliberately NOT in it — they ride param letters
        // that have already landed, and a swap purely for a knob turn would
        // cut sounding voices. `push_graph` reads the tracks' current values,
        // so the next swap for any other reason bakes them in.
        if recompile_due(self.arrangement.clips != self.compiled_clips, since) {
            self.push_graph();
        }
    }

    /// Knob edits from a track's synth card: remembered on that track so the
    /// next recompile preserves them, and sent live to that track's Seq node.
    /// The param ids are the wire contract with `Node::Seq::apply`
    /// (0 gain, 1 attack ms, 2 release ms).
    /// Reverb knob edits: stored on the track and sent to that track's
    /// EFFECT node. The two cards number their parameters the same way, so
    /// this deliberately does not share a path with the synth's letters.
    /// Send a pan letter for every track whose pan has moved since the
    /// last one. `NodeSpec::Pan` param 0 is pan, `-1..=1`.
    ///
    /// A LETTER, not a recompile: the node already exists on every
    /// instrument track, so dragging the header knob costs one 16-byte
    /// message per changed frame instead of a schedule swap per frame.
    fn sync_pans(&mut self) {
        let n = self.arrangement.tracks.len();
        // `f32::NAN != NAN`, so a freshly grown slot always sends once.
        self.sent_pan.resize(n, f32::NAN);
        for i in 0..n {
            let pan = self.arrangement.tracks[i].pan;
            if pan == self.sent_pan[i] {
                continue;
            }
            let Some(Some(node)) = self.pan_ids.get(i).copied() else {
                continue;
            };
            let Some(engine) = &mut self.engine else {
                return;
            };
            engine.set_param(node, 0, pan);
            self.sent_pan[i] = pan;
        }
    }

    fn apply_fx_edits(&mut self, track: usize, edits: &[device::ParamEdit]) {
        let Some(t) = self.arrangement.tracks.get_mut(track) else {
            return;
        };
        for edit in edits {
            match edit.param {
                0 => t.reverb.mix = edit.value,
                1 => t.reverb.size = edit.value,
                2 => t.reverb.damp = edit.value,
                _ => {}
            }
        }
        let Some(Some(node)) = self.fx_ids.get(track).copied() else {
            return;
        };
        if let Some(engine) = &mut self.engine {
            for edit in edits {
                engine.set_param(node, edit.param, edit.value);
            }
        }
    }

    fn apply_param_edits(&mut self, track: usize, edits: &[device::ParamEdit]) {
        let Some(params) = self
            .arrangement
            .tracks
            .get_mut(track)
            .map(|t| &mut t.params)
        else {
            return;
        };
        for edit in edits {
            match edit.param {
                0 => params.gain = edit.value,
                1 => params.attack_ms = edit.value,
                2 => params.release_ms = edit.value,
                _ => {}
            }
            // Only the track's OWN node — a letter to the wrong Seq would
            // retune a different instrument.
            if let (Some(engine), Some(Some(seq))) =
                (&mut self.engine, self.seq_ids.get(track).copied())
            {
                engine.set_param(seq, edit.param, edit.value);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // The engine first, before anything draws from its numbers.
        self.pump_engine(ui.ctx());

        // Shift+Tab flips the bottom region between rack and piano roll.
        // Consumed first, so neither editor mistakes it for its own input.
        if !ui.ctx().egui_wants_keyboard_input()
            && !self.skin.is_open()
            && ui
                .ctx()
                .input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab))
        {
            self.bottom_view = self.bottom_view.toggled();
            // Stale focus must not keep eating arrows once the roll is gone.
            self.piano_roll.owns_keys = false;
        }

        // ORDER MATTERS. `piano_roll::keys` and `arrangement_keys` must get
        // first refusal on the arrows, because `Focus::begin` consumes all
        // four unconditionally. Run them second and Right never reaches
        // either grid — generic navigation eats it and the ring leaves for
        // the tempo field. The roll goes before the arrangement so its
        // Delete/Ctrl+C claims beat the arrangement's while the ring is on
        // a note cell.
        let mut actions: Vec<UiAction> = Vec::new();

        // The palette gets the keyboard before every grid. While it is open
        // it owns the arrows and Enter (it consumes what it uses), so a
        // command list can never be navigated and an arrangement cell moved
        // by the same keystroke.
        let cmds = self.commands();
        if let Some(id) = self.palette.show(ui.ctx(), &self.theme, &cmds) {
            self.run_command(id, &mut actions);
        } else if !self.palette.is_open()
            && !self.skin.is_open()
            && ui
                .ctx()
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Colon))
        {
            self.palette.open();
        }
        let palette_open = self.palette.is_open();

        // The theme window, after the palette and before every other
        // keyboard consumer: while open it owns the keys the same way the
        // palette does, and a preview applies before the panels draw, so
        // the whole frame is painted in the scheme under the cursor.
        self.skin
            .draw(ui.ctx(), ui.ctx().content_rect(), &mut self.theme);
        let skin_open = self.skin.is_open();
        if skin_open {
            // egui's stock widgets follow the previewed theme too.
            self.theme.apply(ui.ctx());
        }

        // Transport keys, global to the app: space plays, Home returns to
        // zero. Skipped while a text field, the palette or the theme window
        // owns the keyboard — a space typed into a search box is a space,
        // not a play command, and getting that wrong is the classic DAW bug.
        if !palette_open && !skin_open && !ui.ctx().egui_wants_keyboard_input() {
            ui.ctx().input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Space) {
                    actions.push(UiAction::TogglePlay);
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                    actions.push(UiAction::Return);
                }
            });
        }

        if !palette_open && !skin_open && self.bottom_view == BottomView::PianoRoll {
            // Point the roll at the selected clip BEFORE any key reaches it:
            // an index from the previously shown clip must never survive
            // into an edit on this one.
            self.piano_roll
                .follow_clip(self.arrangement.active_clip_id());
            let notes = self.arrangement.active_clip().map(|c| &mut c.notes);
            piano_roll::keys(ui.ctx(), &mut self.piano_roll, notes);
        }
        if !palette_open && !skin_open {
            arrangement_keys(ui.ctx(), &self.arrangement, &mut actions);
        }
        // An open header rename owns the keyboard outright, and this must
        // run BEFORE `Focus::begin` — that consumes Escape to hand focus
        // back from any text field, which would turn a cancel into a
        // commit.
        track_rename_keys(ui.ctx(), &mut self.arrangement);
        self.focus.begin(ui.ctx());
        let t = &self.theme;

        let engine_view = EngineView {
            on: self.engine.is_some(),
            hud: self.hud,
            notice: self.notice.as_deref(),
        };
        egui::Panel::top("top_bar")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(TOP_BAR_H)
            .frame(self.fill(t.surface_sunken))
            .show(ui, |ui| {
                top_bar_body(
                    ui,
                    t,
                    &mut self.focus,
                    &self.transport,
                    engine_view,
                    &mut actions,
                )
            });

        // Two panels, one slot: only the current mode's panel is shown, and
        // each keeps its own persisted size — the rack stays rack-sized, the
        // roll can be pulled most of the way up the window.
        // The device rack belongs to the selected track, and its knob edits
        // are addressed to that track — captured here, not re-derived later,
        // so a selection change mid-frame cannot misdeliver them.
        let device_track = self.arrangement.active_track();
        // The frame is built BEFORE the field borrows below: `fill` takes
        // `&self`, and a whole-self borrow cannot coexist with them.
        let sunken = self.fill(t.surface_sunken);
        let (device, edits) = match self.bottom_view {
            BottomView::Rack => {
                let arrangement = &mut self.arrangement;
                let out = egui::Panel::bottom("device")
                    .resizable(true)
                    .show_separator_line(false)
                    .default_size(DEVICE_H)
                    .size_range(DEVICE_H_RANGE)
                    .frame(sunken)
                    .show(ui, |ui| {
                        // Only a track that HOLDS a device shows one. An
                        // empty track's knob state exists but is not its
                        // instrument, so it must not be drawn as one.
                        let track = device_track.and_then(|i| arrangement.tracks.get_mut(i));
                        // Split the borrow: each card needs its own slot,
                        // and only the slots that are actually filled.
                        let (synth, fx) = match track {
                            Some(t) => {
                                let has_dev = t.device.is_some();
                                let has_fx = t.fx.is_some();
                                (
                                    has_dev.then_some(&mut t.synth),
                                    has_fx.then_some(&mut t.reverb),
                                )
                            }
                            None => (None, None),
                        };
                        device_body(ui, t, synth, fx)
                    });
                (out.response.rect, out.inner)
            }
            BottomView::PianoRoll => {
                let arrangement = &mut self.arrangement;
                let piano_roll = &mut self.piano_roll;
                let focus = &mut self.focus;
                let beats_per_bar = self.transport.beats_per_bar;
                let rect = egui::Panel::bottom("piano_roll")
                    .resizable(true)
                    .show_separator_line(false)
                    .default_size(piano_roll::DEFAULT_H)
                    .size_range(piano_roll::H_RANGE)
                    .frame(sunken)
                    .show(ui, |ui| {
                        let key = arrangement.key;
                        piano_roll::body(
                            ui,
                            focus,
                            t,
                            piano_roll,
                            beats_per_bar,
                            arrangement.active_clip(),
                            key,
                        )
                    })
                    .response
                    .rect;
                (rect, DeviceEdits::default())
            }
        };
        // `edits` is applied at the end of the frame, once the theme borrow
        // the panels hold has ended.

        let browser_panel = egui::Panel::left("browser")
            .resizable(true)
            .show_separator_line(false)
            .default_size(BROWSER_W)
            .size_range(BROWSER_W_RANGE)
            .frame(self.fill(t.surface_sunken))
            .show(ui, |ui| {
                browser_body(ui, t, &mut self.focus, &mut self.browser)
            });
        // Applied at the end of the frame with `edits`, for the same
        // reason: the panels still hold the theme borrow here.
        let load_device = browser_panel.inner;
        let browser = browser_panel.response.rect;

        // The transport position in beats: what the arrangement draws its
        // playhead from. The stand-in clock ticks in seconds; the engine's
        // playhead will arrive in beats already.
        let playhead = (self.transport.position * self.transport.bpm / 60.0) as f32;

        let panned = egui::CentralPanel::default()
            .frame(self.fill(t.bg))
            .show(ui, |ui| {
                arrangement_body(
                    ui,
                    &mut self.focus,
                    &self.theme,
                    &mut self.arrangement,
                    self.transport.beats_per_bar,
                    playhead,
                    self.transport.follow,
                )
            })
            .inner;

        // Panning by hand is the user taking the wheel: follow stays off
        // until they ask for it again.
        if panned {
            self.transport.follow = false;
        }

        // Painted last, from the root Ui, so the marks land on top of every
        // region's fill. Only the two DRAGGABLE seams are marked — the top
        // bar's lower edge is fixed, so marking it would advertise a handle
        // that is not there — and only while that seam is pointed at or
        // pulled, so an untouched window is fills and nothing else.
        seam(ui, "browser", vertical_seam(browser));
        seam(ui, self.bottom_view.panel_id(), horizontal_seam(device));

        self.focus.end(ui, t);

        // Knob edits leave the card as (param id, natural value); they land
        // on the track that drew the card and, live, on that track's Seq.
        if let Some(track) = device_track {
            self.apply_param_edits(track, &edits.synth);
            self.apply_fx_edits(track, &edits.fx);
        }
        if let Some(item) = load_device {
            self.load_device(item);
        }

        // The power actions are the app's own — they own the Engine, which
        // `perform` (pure over UI state) never touches. Everything else goes
        // through the vocabulary as before.
        let mut wishes = Vec::with_capacity(actions.len());
        for action in actions {
            match action {
                UiAction::StartEngine => self.start_engine(),
                UiAction::StopEngine => self.stop_engine(),
                // Density is the theme's packing AND a machine-local
                // preference: apply both, and the next `save` carries it.
                UiAction::SetDensity(density) => {
                    self.theme.set_density(density);
                    self.prefs.density = density;
                    self.theme.apply(ui.ctx());
                }
                other => wishes.push(other),
            }
        }
        // Engine commands first, so TogglePlay reads the mirror the user saw;
        // then the mirror itself; then the loop/schedule reconciliation.
        self.route_transport(&wishes);
        // The grid keys move BOTH grids. `perform` is pure over transport +
        // arrangement — a shape forty tests rely on — so the roll's half
        // lives here rather than widening that signature. Each grid steps
        // independently in the same direction, so a roll deliberately set
        // finer than the arrangement stays finer.
        for wish in &wishes {
            match wish {
                UiAction::NarrowGrid => {
                    self.piano_roll.grid = step_grid(self.piano_roll.grid, true)
                }
                UiAction::WidenGrid => {
                    self.piano_roll.grid = step_grid(self.piano_roll.grid, false)
                }
                _ => {}
            }
        }
        perform(&wishes, &mut self.transport, &mut self.arrangement);
        self.sync_engine();

        // Roll the stand-in clock, and keep frames coming while it runs.
        // Only while no stream runs: with an engine up, the sample counter is
        // the master clock and the mirror is refreshed from telemetry.
        if self.engine.is_none() && self.transport.playing {
            self.transport.position += f64::from(ui.ctx().input(|i| i.stable_dt));
            // Loop wrap, stand-in edition: the engine's transport owns the
            // sample-accurate version; until it is wired in, the clock wraps
            // at the loop region so the loop on screen means what it draws.
            if self.transport.loop_on
                && let Some((from, to)) = self.arrangement.loop_range
            {
                self.transport.position =
                    wrap_loop(self.transport.position, self.transport.bpm, from, to);
            }
            ui.ctx().request_repaint();
        }
    }

    /// Machine-local preferences ride eframe's storage: saved at exit and on
    /// the 30s auto-save, so a density choice made mid-session survives it.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.prefs);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput, Rect, Vec2, pos2, vec2};

    const SCREEN: Vec2 = vec2(1600.0, 900.0);

    fn screen_rect() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), SCREEN)
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(screen_rect()),
            events,
            ..Default::default()
        }
    }

    /// Run one headless pass, returning the browser region's rendered width.
    fn pass(ctx: &egui::Context, events: Vec<Event>) -> f32 {
        let mut width = 0.0;
        let mut out = ctx.run_ui(input(events), |ui| {
            let theme = Theme::dark();
            egui::Panel::top("top_bar")
                .resizable(false)
                .exact_size(TOP_BAR_H)
                .frame(egui::Frame::new().fill(theme.surface_raised))
                .show(ui, claim);

            egui::Panel::bottom("device")
                .resizable(true)
                .default_size(DEVICE_H)
                .size_range(DEVICE_H_RANGE)
                .frame(egui::Frame::new().fill(theme.surface_sunken))
                .show(ui, claim);

            width = egui::Panel::left("browser")
                .resizable(true)
                .default_size(BROWSER_W)
                .size_range(BROWSER_W_RANGE)
                .frame(egui::Frame::new().fill(theme.surface))
                .show(ui, |ui| {
                    browser_body(ui, &theme, &mut Focus::default(), &mut Browser::default())
                })
                .response
                .rect
                .width();

            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme.bg))
                .show(ui, claim);
        });
        out.textures_delta.clear();
        width
    }

    /// A region whose body claims no space collapses to its MINIMUM, not its
    /// default — and the collapsed size is what gets persisted. This is the
    /// bug that made the browser look unresizable.
    #[test]
    fn regions_honour_their_sizes() {
        let ctx = egui::Context::default();
        for _ in 0..3 {
            assert_eq!(
                pass(&ctx, vec![]),
                BROWSER_W,
                "browser rendered at its minimum instead of its default — its \
                 body is not claiming the available space"
            );
        }
    }

    /// The thing that was actually broken: drag the edge, and the new width
    /// must survive into the following frame instead of snapping back.
    #[test]
    fn dragging_the_browser_edge_resizes_it() {
        let ctx = egui::Context::default();
        pass(&ctx, vec![]);

        // Grab the edge. The handle sits on the browser's right boundary, and
        // the grab zone is RESIZE_GRAB_PX wide, so landing on the seam works.
        let edge = pos2(BROWSER_W, SCREEN.y * 0.5);
        pass(
            &ctx,
            vec![
                Event::PointerMoved(edge),
                Event::PointerButton {
                    pos: edge,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );

        // Pull it right. Two passes: one registers the drag, the next reads
        // that response back — which is exactly the round trip the collapse
        // bug was breaking.
        let target = 380.0;
        let moved = pos2(target, SCREEN.y * 0.5);
        pass(&ctx, vec![Event::PointerMoved(moved)]);
        let width = pass(&ctx, vec![Event::PointerMoved(moved)]);

        assert!(
            (width - target).abs() < 2.0,
            "dragged to {target}px but the region rendered at {width}px"
        );
    }

    /// The seams are painted against an id egui does not publish. If that
    /// detail ever changes, the marks vanish silently — so assert it here.
    #[test]
    fn the_resize_handle_id_is_the_one_we_paint_against() {
        let ctx = egui::Context::default();
        pass(&ctx, vec![]);
        let edge = pos2(BROWSER_W, SCREEN.y * 0.5);
        pass(&ctx, vec![Event::PointerMoved(edge)]);
        pass(&ctx, vec![Event::PointerMoved(edge)]);

        let response = ctx.read_response(handle_id("browser"));
        assert!(
            response.is_some(),
            "egui no longer registers a panel's resize handle under \
             `Id::new(panel_id).with(\"__resize\")` — `handle_id` is stale and \
             the seam marks will never appear"
        );
        assert!(
            seam_active(&ctx, "browser"),
            "the pointer is sitting on the browser seam, but the handle \
             reports neither hover nor drag"
        );
    }

    /// An untouched window has no marks at all.
    #[test]
    fn seams_are_invisible_until_pointed_at() {
        let ctx = egui::Context::default();
        pass(&ctx, vec![]);
        pass(
            &ctx,
            vec![Event::PointerMoved(pos2(SCREEN.x * 0.7, SCREEN.y * 0.3))],
        );
        assert!(!seam_active(&ctx, "browser"));
        assert!(!seam_active(&ctx, "device"));
    }

    /// The box travels: it must not arrive in one step, must not overshoot,
    /// and must actually get there.
    #[test]
    fn the_cursor_springs_without_overshooting() {
        let start = egui::Vec2::ZERO;
        let target = egui::vec2(600.0, 0.0);
        let dt = 1.0 / 60.0;

        let (first, _) = spring_step(start, egui::Vec2::ZERO, target, dt);
        assert!(
            first.x > 0.0 && first.x < target.x,
            "one frame should move the box partway, not teleport it — got {}",
            first.x
        );

        let mut pos = start;
        let mut vel = egui::Vec2::ZERO;
        let mut frames = 0;
        while (pos - target).length() > RING_SETTLED_PX && frames < 600 {
            (pos, vel) = spring_step(pos, vel, target, dt);
            assert!(
                pos.x <= target.x + f32::EPSILON,
                "critically damped means no overshoot, but reached {} past {}",
                pos.x,
                target.x
            );
            frames += 1;
        }
        assert!(frames < 600, "the spring never settled");
        // ~5/omega seconds to settle, so well under half a second at 60fps.
        assert!(
            frames < 30,
            "settling took {frames} frames at 60fps — that reads as sluggish"
        );
    }

    /// A long frame must not fling the box. This is why the spring is solved
    /// implicitly: the explicit form is unstable exactly when frames drop.
    #[test]
    fn a_dropped_frame_does_not_fling_the_cursor() {
        let target = egui::vec2(600.0, 0.0);
        for dt in [1.0 / 60.0, 0.1, 0.5, 2.0] {
            let (pos, _) = spring_step(egui::Vec2::ZERO, egui::Vec2::ZERO, target, dt);
            assert!(
                pos.x >= 0.0 && pos.x <= target.x + f32::EPSILON,
                "dt={dt}s put the box at {} — outside the travel",
                pos.x
            );
        }
    }

    /// The bands are inset on every side, meet exactly, and give up rather
    /// than draw a sliver when the panel gets small.
    #[test]
    fn the_browser_bands_split_and_degrade_cleanly() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 500.0));
        let (upper, lower) = browser_bands(area, BROWSER_LOWER_FRAC).unwrap();

        // Dark brown shows down both sides...
        assert_eq!(upper.left(), area.left() + BROWSER_INSET_X);
        assert_eq!(lower.right(), area.right() - BROWSER_INSET_X);
        // ...and the content spans the panel's full height, so it lines up
        // with the arrangement beside it.
        assert_eq!(upper.top(), area.top() + BROWSER_INSET_Y);
        assert_eq!(lower.bottom(), area.bottom() - BROWSER_INSET_Y);

        // No gap and no overlap between them.
        assert_eq!(upper.bottom(), lower.top());
        assert_eq!(
            lower.height(),
            area.height() * BROWSER_LOWER_FRAC,
            "the lower band is a fifth of the PANEL height, not of the inset area"
        );
        assert!(upper.height() > lower.height());

        // The split tracks the region: a taller panel gets a taller band.
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 1000.0));
        let (_, tall_lower) = browser_bands(tall, BROWSER_LOWER_FRAC).unwrap();
        assert_eq!(tall_lower.height(), 200.0);

        // A squat panel no longer degenerates: with no vertical inset the
        // bands simply scale down, both keeping positive height.
        let squat = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 20.0));
        let (u, l) = browser_bands(squat, BROWSER_LOWER_FRAC).unwrap();
        assert!(u.height() > 0.0 && l.height() > 0.0);

        // What does still degenerate: no height at all, and a panel narrower
        // than its own side margins.
        let flat = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 0.0));
        assert!(browser_bands(flat, BROWSER_LOWER_FRAC).is_none());
        let sliver = Rect::from_min_size(pos2(0.0, 0.0), vec2(BROWSER_INSET_X * 2.0, 500.0));
        assert!(browser_bands(sliver, BROWSER_LOWER_FRAC).is_none());
    }

    /// Dragging the inner divider maps pointer position to split, and cannot
    /// be pulled far enough to erase either band.
    #[test]
    fn the_inner_divider_drags_within_bounds() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 500.0));
        let lo = *BROWSER_SPLIT_RANGE.start();
        let hi = *BROWSER_SPLIT_RANGE.end();

        // Pointer at the divider's resting place reproduces the resting split.
        let (upper, _) = browser_bands(area, BROWSER_LOWER_FRAC).unwrap();
        let round_trip = split_from_pointer(area, upper.bottom());
        assert!(
            (round_trip - BROWSER_LOWER_FRAC).abs() < 1e-4,
            "pointer on the divider asked for {round_trip}, not {BROWSER_LOWER_FRAC}"
        );

        // Dragging up grows the lower band, down shrinks it.
        assert!(split_from_pointer(area, upper.bottom() - 50.0) > BROWSER_LOWER_FRAC);
        assert!(split_from_pointer(area, upper.bottom() + 50.0) < BROWSER_LOWER_FRAC);

        // Yanked off either end, both bands survive.
        for y in [-10_000.0, -1.0, 0.0, 250.0, 499.0, 10_000.0] {
            let split = split_from_pointer(area, y);
            assert!((lo..=hi).contains(&split), "y={y} gave split {split}");
            let (u, l) = browser_bands(area, split).unwrap();
            assert!(u.height() > 0.0 && l.height() > 0.0, "y={y} erased a band");
        }
    }

    /// The browser must be exactly as tall as the arrangement. It hung 180px
    /// below it — the device panel's height — because `left` was claimed
    /// before `bottom`, so the browser took the full column and the device
    /// only spanned from the browser's right edge.
    #[test]
    fn the_browser_and_arrangement_are_the_same_height() {
        let ctx = egui::Context::default();
        let mut rects = (Rect::ZERO, Rect::ZERO, Rect::ZERO);
        for _ in 0..3 {
            let mut out = ctx.run_ui(input(vec![]), |ui| {
                let theme = Theme::dark();
                egui::Panel::top("top_bar")
                    .resizable(false)
                    .show_separator_line(false)
                    .exact_size(TOP_BAR_H)
                    .frame(egui::Frame::new().fill(theme.surface_sunken))
                    .show(ui, claim);
                let device = egui::Panel::bottom("device")
                    .resizable(true)
                    .show_separator_line(false)
                    .default_size(DEVICE_H)
                    .size_range(DEVICE_H_RANGE)
                    .frame(egui::Frame::new().fill(theme.surface_sunken))
                    .show(ui, claim)
                    .response
                    .rect;
                let browser = egui::Panel::left("browser")
                    .resizable(true)
                    .show_separator_line(false)
                    .default_size(BROWSER_W)
                    .size_range(BROWSER_W_RANGE)
                    .frame(egui::Frame::new().fill(theme.surface_sunken))
                    .show(ui, |ui| {
                        browser_body(ui, &theme, &mut Focus::default(), &mut Browser::default())
                    })
                    .response
                    .rect;
                let center = egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme.bg))
                    .show(ui, claim)
                    .response
                    .rect;
                rects = (browser, center, device);
            });
            out.textures_delta.clear();
        }
        let (browser, center, device) = rects;
        assert_eq!(
            browser.top(),
            center.top(),
            "browser and arrangement should start at the same y"
        );

        // The visible content, not just the panel: a vertical inset would
        // make the browser's box shorter than the arrangement and push it
        // down, which is what a uniform inset did.
        let (upper, lower) = browser_bands(browser, BROWSER_LOWER_FRAC).unwrap();
        assert_eq!(
            upper.top(),
            center.top(),
            "the browser's content sits {}px below the arrangement",
            upper.top() - center.top()
        );
        assert_eq!(
            lower.bottom(),
            center.bottom(),
            "the browser's content stops {}px short of the arrangement",
            center.bottom() - lower.bottom()
        );
        assert_eq!(
            browser.bottom(),
            center.bottom(),
            "browser hangs {}px past the arrangement",
            browser.bottom() - center.bottom()
        );
        assert_eq!(
            device.left(),
            browser.left(),
            "the device panel should span the full width, under the browser too"
        );
    }

    /// The field is one text row tall and sits on the well's centre line —
    /// not stretched to the well's full height, which is what `ui.put`'s
    /// centered_and_justified layout does and what left the text riding high.
    #[test]
    fn the_search_field_is_centred_in_its_well() {
        let well = Rect::from_min_size(pos2(0.0, 100.0), vec2(200.0, SEARCH_H));
        let row = 14.0;
        let field = centred_band(well, 30.0, 190.0, row);

        assert_eq!(field.height(), row, "the field should be one row tall");
        assert!(field.height() < well.height(), "it must not fill the well");
        assert_eq!(
            field.center().y,
            well.center().y,
            "field centre {} is off the well's centre {}",
            field.center().y,
            well.center().y
        );
        assert_eq!(well.top() - field.top(), field.bottom() - well.bottom());
        assert_eq!((field.left(), field.right()), (30.0, 190.0));
    }

    /// No invented library: every row the browser offers is something
    /// `load_device` can really build, and both slots are represented.
    #[test]
    fn the_browser_lists_only_devices_that_load() {
        let browser = Browser::default();
        let items: Vec<BrowserItem> = browser
            .folders
            .iter()
            .flat_map(|f| f.items.iter().copied())
            .collect();
        assert!(!items.is_empty(), "loadable devices exist, so list them");
        assert!(
            items.iter().any(|i| i.load.is_instrument()),
            "an instrument must be listed"
        );
        assert!(
            items.iter().any(|i| !i.load.is_instrument()),
            "an effect must be listed"
        );
        // And every item is reachable as a row, so it can be activated.
        assert_eq!(
            tree_targets(&browser.folders)
                .iter()
                .filter(|t| t.is_some())
                .count(),
            items.len()
        );
    }

    /// The tree still renders whatever it is given: an elbow only on the
    /// last child, and it grows and shrinks as folders open and close.
    #[test]
    fn the_tree_renders_the_folders_it_is_given() {
        let mut folders = vec![
            Folder {
                name: "One",
                items: &[
                    BrowserItem {
                        name: "a",
                        load: DeviceKind::SineSynth,
                    },
                    BrowserItem {
                        name: "b",
                        load: DeviceKind::SineSynth,
                    },
                    BrowserItem {
                        name: "c",
                        load: DeviceKind::SineSynth,
                    },
                ],
                open: true,
            },
            Folder {
                name: "Two",
                items: &[BrowserItem {
                    name: "d",
                    load: DeviceKind::SineSynth,
                }],
                open: false,
            },
        ];

        let rows = tree_rows(&folders);
        // Only One is open: 2 folder rows + its 3 children.
        assert_eq!(rows.len(), 5);
        assert!(
            rows[0].0.starts_with(TREE_OPEN),
            "open folder wants {TREE_OPEN}"
        );
        assert!(rows[0].1, "row 0 should be a folder");
        assert!(rows[1].0.contains(TREE_TEE) && rows[1].0.contains("a"));
        assert!(!rows[1].1, "row 1 should be a child");

        // Last child gets the elbow, and only the last.
        assert!(rows[3].0.contains(TREE_ELL), "last child wants {TREE_ELL}");
        assert_eq!(rows.iter().filter(|(t, _)| t.contains(TREE_ELL)).count(), 1);

        // A closed folder shows the shut arrow and contributes one row.
        assert!(rows[4].0.starts_with(TREE_SHUT));

        // Opening every folder shows every item; closing all shows the
        // folder rows alone.
        for folder in &mut folders {
            folder.open = true;
        }
        assert_eq!(tree_rows(&folders).len(), 2 + 3 + 1);
        for folder in &mut folders {
            folder.open = false;
        }
        assert_eq!(tree_rows(&folders).len(), 2);
    }

    /// The bar lays controls out left to right, vertically centred, never
    /// overlapping, with air between groups instead of rules.
    #[test]
    fn the_bar_lays_out_left_to_right() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, TOP_BAR_H));
        let mut bar = Bar::at(area, area.left() + TRANSPORT_PAD);

        let first = bar.button();
        assert_eq!(
            first.left(),
            TRANSPORT_PAD,
            "first control hugs the padding"
        );
        assert_eq!(
            first.center().y,
            area.center().y,
            "controls centre in the bar"
        );
        assert!(
            first.height() <= area.height(),
            "a button does not fit the bar"
        );

        let second = bar.button();
        assert_eq!(second.left() - first.left(), TRANSPORT_BTN + TRANSPORT_GAP);
        assert!(second.left() >= first.right(), "adjacent buttons overlap");

        // A group break leaves strictly more air than the ordinary gap.
        bar.group();
        let after = bar.button();
        assert!(
            after.left() - second.right() > second.left() - first.right(),
            "the group break is no wider than the gap inside a group"
        );

        // Fields share the centre line and their own width.
        let f = bar.field(TEMPO_W);
        assert_eq!(f.center().y, area.center().y);
        assert_eq!(f.width(), TEMPO_W);
        assert_eq!(f.height(), FIELD_H);
        assert!(
            f.left() >= after.right(),
            "field overlaps the button before it"
        );
    }

    fn group_widths() -> (f32, f32, f32) {
        (
            buttons_width(5),
            fields_width(&[READOUT_W, TIMECODE_W, ENGINE_W]),
            buttons_width(4) + TRANSPORT_GROUP_GAP + fields_width(&[TEMPO_W, TIMESIG_W, TIMESIG_W]),
        )
    }

    /// The three groups anchor left, centre and right, and none of them
    /// overlaps its neighbour at a realistic window width.
    #[test]
    fn the_bar_spreads_across_three_anchors() {
        let (verbs, centre, right) = group_widths();

        for width in [1600.0, 1097.0, 900.0] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, TOP_BAR_H));
            let (lx, cx, rx, spread) = bar_layout(area, verbs, centre, right);
            assert!(spread, "{width}pt should be wide enough to spread");

            assert_eq!(lx, TRANSPORT_PAD, "verbs should hold the left edge");
            assert!(
                (cx + centre * 0.5 - area.center().x).abs() < 1e-3,
                "the readouts are not centred at {width}pt"
            );
            assert!(
                (rx + right - (area.right() - TRANSPORT_PAD)).abs() < 1e-3,
                "the settings do not reach the right edge at {width}pt"
            );

            assert!(
                cx > lx + verbs,
                "readouts collide with the verbs at {width}pt"
            );
            assert!(
                rx > cx + centre,
                "settings collide with the readouts at {width}pt"
            );
        }
    }

    /// Squeezed narrow, the anchors would cross — so it packs left instead,
    /// in the same order, rather than overlapping.
    #[test]
    fn a_narrow_bar_packs_left_instead_of_overlapping() {
        let (verbs, centre, right) = group_widths();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, TOP_BAR_H));
        let (lx, cx, rx, spread) = bar_layout(area, verbs, centre, right);

        assert!(!spread, "420pt cannot hold three anchored groups");
        assert!(cx >= lx + verbs, "packed readouts overlap the verbs");
        assert!(rx >= cx + centre, "packed settings overlap the readouts");
    }

    /// Play emits TogglePlay, and TogglePlay is what flips the state. The
    /// button never touches `playing` itself.
    #[test]
    fn play_toggles_through_the_action_vocabulary() {
        let mut t = Transport::default();

        perform(&[UiAction::TogglePlay], &mut t, &mut Arrangement::default());
        assert!(t.playing, "first press should start playing");
        perform(&[UiAction::TogglePlay], &mut t, &mut Arrangement::default());
        assert!(!t.playing, "second press should stop");

        // Two presses in one frame land in order, not as one.
        perform(
            &[UiAction::TogglePlay, UiAction::TogglePlay],
            &mut t,
            &mut Arrangement::default(),
        );
        assert!(!t.playing);
    }

    /// The four transport verbs are genuinely distinct. Pause and Stop differ
    /// only in what they do to position; Return differs only in what it does
    /// to playing. Collapse any pair and one button becomes decoration.
    #[test]
    fn the_four_transport_verbs_are_distinct() {
        // Pause: halt, HOLD position.
        let mut t = Transport {
            playing: true,
            position: 12.5,
            ..Default::default()
        };
        perform(&[UiAction::Pause], &mut t, &mut Arrangement::default());
        assert!(!t.playing);
        assert_eq!(t.position, 12.5, "pause must not rewind");

        // Stop: halt AND rewind.
        let mut t = Transport {
            playing: true,
            position: 12.5,
            ..Default::default()
        };
        perform(&[UiAction::Stop], &mut t, &mut Arrangement::default());
        assert!(!t.playing);
        assert_eq!(t.position, 0.0, "stop should rewind");

        // Return: rewind, WITHOUT halting.
        let mut t = Transport {
            playing: true,
            position: 12.5,
            ..Default::default()
        };
        perform(&[UiAction::Return], &mut t, &mut Arrangement::default());
        assert!(t.playing, "return must not stop playback");
        assert_eq!(t.position, 0.0);

        // Stop is exactly Pause + Return.
        let mut via_stop = Transport {
            playing: true,
            position: 9.0,
            ..Default::default()
        };
        let mut via_both = Transport {
            playing: true,
            position: 9.0,
            ..Default::default()
        };
        perform(
            &[UiAction::Stop],
            &mut via_stop,
            &mut Arrangement::default(),
        );
        perform(
            &[UiAction::Pause, UiAction::Return],
            &mut via_both,
            &mut Arrangement::default(),
        );
        assert_eq!(
            (via_stop.playing, via_stop.position),
            (via_both.playing, via_both.position)
        );

        // All of them are idempotent.
        for action in [UiAction::Pause, UiAction::Stop, UiAction::Return] {
            let mut once = Transport {
                playing: true,
                position: 4.0,
                ..Default::default()
            };
            let mut twice = Transport {
                playing: true,
                position: 4.0,
                ..Default::default()
            };
            perform(&[action], &mut once, &mut Arrangement::default());
            perform(&[action, action], &mut twice, &mut Arrangement::default());
            assert_eq!(
                (once.playing, once.position),
                (twice.playing, twice.position),
                "{action:?} is not idempotent"
            );
        }
    }

    /// Both icons are centred on the button by construction — the thing a
    /// glyph could not promise, because Align2 centres the advance box and
    /// the ink sits off centre inside it.
    #[test]
    fn transport_icons_are_centred_on_their_button() {
        let button = Bar::at(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, TOP_BAR_H)),
            TRANSPORT_PAD,
        )
        .button();
        let c = button.center();

        let tri = play_icon(button);
        let xs: Vec<f32> = tri.iter().map(|p| p.x).collect();
        let ys: Vec<f32> = tri.iter().map(|p| p.y).collect();
        let (x0, x1) = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MIN, f32::max),
        );
        assert!(
            ((x0 + x1) * 0.5 - c.x).abs() < 1e-4,
            "play is off centre in x"
        );
        assert!(
            ((y0 + y1) * 0.5 - c.y).abs() < 1e-4,
            "play is off centre in y"
        );
        assert!((x1 - x0 - ICON).abs() < 1e-4 && (y1 - y0 - ICON).abs() < 1e-4);
        assert!(button.contains(pos2(x0, y0)) && button.contains(pos2(x1, y1)));

        let [left, right] = pause_icon(button);
        assert!(
            ((left.left() + right.right()) * 0.5 - c.x).abs() < 1e-4,
            "pause is off centre in x"
        );
        assert_eq!(left.center().y, c.y);
        assert_eq!(right.center().y, c.y);
        assert_eq!(left.width(), PAUSE_BAR);
        assert_eq!(right.width(), PAUSE_BAR);
        assert!(
            (right.left() - left.right() - PAUSE_GAP).abs() < 1e-4,
            "wrong gap"
        );
        assert!(button.contains(left.min) && button.contains(right.max));

        let stop = stop_icon(button);
        assert_eq!(stop.center(), c, "stop is off centre");
        assert_eq!(stop.width(), stop.height(), "stop should be square");
        assert!(button.contains(stop.min) && button.contains(stop.max));
        assert!(
            stop.width() < ICON,
            "a stop square the same size as the play triangle reads far \
             heavier beside it — STOP_SIDE is meant to be the smaller"
        );

        let (bar, tri) = return_icon(button);
        let apex = tri[2];
        let back = tri[0].x;
        assert!(apex.x < back, "the return triangle should point LEFT");
        assert_eq!(apex.y, c.y, "return apex is off the centre line");
        assert!(
            (bar.left() + back) * 0.5 - c.x < 1e-4,
            "return is off centre in x"
        );
        assert!(
            (back - bar.left() - ICON).abs() < 1e-4,
            "return should span ICON"
        );
        assert!(bar.right() < apex.x, "the bar and triangle overlap");
        assert!(button.contains(bar.min) && button.contains(pos2(back, tri[1].y)));
    }

    /// The readout counts the way a musician does: 1-indexed, four beats to
    /// the bar, four sixteenths to the beat.
    #[test]
    fn the_readout_counts_bars_and_beats() {
        let bpm = 120.0; // two beats per second, so seconds map cleanly

        assert_eq!(
            bars_beats(0.0, bpm, 4),
            (1, 1, 1),
            "the song starts at bar 1"
        );
        assert_eq!(
            bars_beats(0.125, bpm, 4),
            (1, 1, 2),
            "a quarter beat is one sixteenth"
        );
        assert_eq!(bars_beats(0.5, bpm, 4), (1, 2, 1), "one beat");
        assert_eq!(bars_beats(1.5, bpm, 4), (1, 4, 1), "three beats");
        assert_eq!(
            bars_beats(2.0, bpm, 4),
            (2, 1, 1),
            "four beats is the next bar"
        );
        assert_eq!(bars_beats(8.0, bpm, 4), (5, 1, 1));

        // Just shy of a boundary must not round up into it.
        assert_eq!(bars_beats(2.0 - 1e-9, bpm, 4), (1, 4, 4));

        // Tempo scales it: twice the bpm reaches bar 2 in half the time.
        assert_eq!(bars_beats(1.0, 240.0, 4), (2, 1, 1));

        // Nothing before zero.
        assert_eq!(bars_beats(-5.0, bpm, 4), (1, 1, 1));
    }

    /// The field widths are fixed, so the digits do not shuffle sideways as
    /// the playhead rolls. That is the entire reason it is monospace.
    #[test]
    fn the_readout_does_not_jitter() {
        let bpm = 120.0;
        let at_start = format_position(0.0, bpm, 4);
        assert_eq!(at_start, "   1. 1.1");

        // Every position from bar 1 to bar 999 renders the same width.
        for secs in [0.0, 0.9, 7.3, 61.0, 600.0, 1997.9] {
            assert_eq!(
                format_position(secs, bpm, 4).len(),
                at_start.len(),
                "`{}` is a different width to `{at_start}`",
                format_position(secs, bpm, 4)
            );
        }
    }

    /// Recording is DERIVED from armed + playing, never set. Arming while
    /// stopped must not record; stopping must not disarm.
    #[test]
    fn recording_is_armed_and_rolling() {
        let mut t = Transport::default();
        assert!(!t.recording());

        perform(
            &[UiAction::ToggleRecord],
            &mut t,
            &mut Arrangement::default(),
        );
        assert!(t.armed, "record should arm");
        assert!(!t.recording(), "armed but stopped is not recording");

        perform(&[UiAction::TogglePlay], &mut t, &mut Arrangement::default());
        assert!(t.recording(), "armed and rolling IS recording");

        perform(&[UiAction::Stop], &mut t, &mut Arrangement::default());
        assert!(!t.recording(), "stopping should end the take");
        assert!(t.armed, "stopping must not silently disarm");

        perform(
            &[UiAction::ToggleRecord],
            &mut t,
            &mut Arrangement::default(),
        );
        assert!(!t.armed, "record should disarm on a second press");
    }

    /// Tempo is clamped to the domain limits the vm already owns, and the
    /// readout follows it rather than a constant.
    #[test]
    fn tempo_is_clamped_and_drives_the_readout() {
        let mut t = Transport::default();

        perform(
            &[UiAction::SetTempo(1000.0)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(t.bpm, limits::BPM_MAX, "tempo should clamp at the top");
        perform(
            &[UiAction::SetTempo(-40.0)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(t.bpm, limits::BPM_MIN, "tempo should clamp at the bottom");

        // One second at 60bpm is one beat; at 120 it is two.
        perform(
            &[UiAction::SetTempo(60.0)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(bars_beats(1.0, t.bpm, 4), (1, 2, 1));
        perform(
            &[UiAction::SetTempo(120.0)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(bars_beats(1.0, t.bpm, 4), (1, 3, 1));
    }

    /// The time signature changes how many beats make a bar, and the readout
    /// must follow — a 3/4 bar wraps a beat sooner than a 4/4 one.
    #[test]
    fn the_time_signature_reshapes_the_bar() {
        let mut t = Transport::default();
        perform(
            &[UiAction::SetTimeSignature(3, 4)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!((t.beats_per_bar, t.beat_unit), (3, 4));

        // At 120bpm, 1.5s is three beats: a full 3/4 bar, but only 3 of 4.
        assert_eq!(bars_beats(1.5, 120.0, 3), (2, 1, 1), "3/4 should wrap here");
        assert_eq!(bars_beats(1.5, 120.0, 4), (1, 4, 1), "4/4 should not");

        // Numerator clamps; a zero-beat bar would divide by nothing.
        perform(
            &[UiAction::SetTimeSignature(0, 4)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(t.beats_per_bar, 1);
        perform(
            &[UiAction::SetTimeSignature(999, 4)],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!(t.beats_per_bar, TS_NUM_MAX);
        assert_eq!(bars_beats(1.0, 120.0, 0), bars_beats(1.0, 120.0, 1));
    }

    /// The three modifier toggles are independent of each other and of the
    /// transport verbs.
    #[test]
    fn the_modifier_toggles_are_independent() {
        let mut t = Transport::default();
        let before = (t.loop_on, t.metronome, t.follow);
        assert_eq!(before, (false, false, true), "follow defaults on");

        perform(&[UiAction::ToggleLoop], &mut t, &mut Arrangement::default());
        assert_eq!((t.loop_on, t.metronome, t.follow), (true, false, true));
        perform(
            &[UiAction::ToggleMetronome],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!((t.loop_on, t.metronome, t.follow), (true, true, true));
        perform(
            &[UiAction::ToggleFollow],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!((t.loop_on, t.metronome, t.follow), (true, true, false));

        // Transport verbs leave all three alone.
        perform(
            &[UiAction::TogglePlay, UiAction::Stop, UiAction::Return],
            &mut t,
            &mut Arrangement::default(),
        );
        assert_eq!((t.loop_on, t.metronome, t.follow), (true, true, false));
    }

    /// Timecode is wall clock, independent of tempo, and fixed width.
    #[test]
    fn timecode_is_wall_clock() {
        assert_eq!(format_timecode(0.0), "  0:00.000");
        assert_eq!(format_timecode(1.5), "  0:01.500");
        assert_eq!(format_timecode(61.25), "  1:01.250");
        assert_eq!(format_timecode(-3.0), "  0:00.000", "nothing before zero");

        let width = format_timecode(0.0).len();
        for secs in [0.0, 9.999, 59.9, 60.0, 599.0, 3599.0] {
            assert_eq!(format_timecode(secs).len(), width, "{secs}s changed width");
        }
    }

    fn grid() -> Vec<(egui::Id, Rect)> {
        // A row of three buttons with a field below the middle one.
        vec![
            (
                egui::Id::new("a"),
                Rect::from_min_size(pos2(0.0, 0.0), vec2(26.0, 26.0)),
            ),
            (
                egui::Id::new("b"),
                Rect::from_min_size(pos2(30.0, 0.0), vec2(26.0, 26.0)),
            ),
            (
                egui::Id::new("c"),
                Rect::from_min_size(pos2(60.0, 0.0), vec2(26.0, 26.0)),
            ),
            (
                egui::Id::new("below"),
                Rect::from_min_size(pos2(30.0, 60.0), vec2(26.0, 26.0)),
            ),
        ]
    }

    /// Arrowing along a row lands on the NEXT element, not the far one, and
    /// stops at the ends rather than wrapping.
    #[test]
    fn navigation_steps_one_element_at_a_time() {
        let items = grid();
        let (a, b, c) = (items[0].0, items[1].0, items[2].0);

        assert_eq!(nearest(items[0].1, Dir::Right, &items, Some(a)), Some(b));
        assert_eq!(nearest(items[1].1, Dir::Right, &items, Some(b)), Some(c));
        assert_eq!(nearest(items[2].1, Dir::Left, &items, Some(c)), Some(b));

        // At an edge, nothing lies that way — focus holds rather than wraps.
        assert_eq!(nearest(items[0].1, Dir::Left, &items, Some(a)), None);
        assert_eq!(nearest(items[2].1, Dir::Right, &items, Some(c)), None);
    }

    /// The cross-axis penalty is what makes a row behave. Without it, an
    /// element slightly closer in a straight line steals a sideways press.
    #[test]
    fn navigation_prefers_the_element_you_are_lined_up_with() {
        let items = grid();
        let (b, below) = (items[1].0, items[3].0);

        // Straight down from `b` reaches `below`...
        assert_eq!(nearest(items[1].1, Dir::Down, &items, Some(b)), Some(below));
        // ...but pressing Right from `b` must NOT dive to it, even though it
        // is only twice as far as `c` is.
        assert_eq!(
            nearest(items[1].1, Dir::Right, &items, Some(b)),
            Some(items[2].0)
        );
        // And Up from `below` comes back to the one it is aligned with.
        assert_eq!(nearest(items[3].1, Dir::Up, &items, Some(below)), Some(b));
    }

    /// Focus never lands on itself, and an empty set is not a panic.
    #[test]
    fn navigation_has_no_degenerate_cases() {
        let items = grid();
        for dir in Dir::ALL {
            assert_ne!(
                nearest(items[1].1, dir, &items, Some(items[1].0)),
                Some(items[1].0),
                "{dir:?} landed back on the current element"
            );
        }
        assert_eq!(nearest(items[0].1, Dir::Right, &[], Some(items[0].0)), None);

        // An element exactly on top of the current one is not "in" any
        // direction, so it cannot be reached by an arrow.
        let stacked = vec![
            (
                egui::Id::new("x"),
                Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)),
            ),
            (
                egui::Id::new("y"),
                Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)),
            ),
        ];
        for dir in Dir::ALL {
            assert_eq!(
                nearest(stacked[0].1, dir, &stacked, Some(stacked[0].0)),
                None
            );
        }
    }

    /// Every control the bar draws registers itself, so the keyboard can
    /// reach all of them. A control that is drawn but not registered is
    /// invisible to the keyboard and impossible to notice by eye.
    #[test]
    fn every_transport_control_is_reachable() {
        let ctx = egui::Context::default();
        let mut focus = Focus::default();
        let mut actions = Vec::new();
        let t = Transport::default();

        let off = EngineView {
            on: false,
            hud: None,
            notice: None,
        };
        let mut out = ctx.run_ui(input(vec![]), |ui| {
            let theme = Theme::dark();
            egui::Panel::top("top_bar")
                .exact_size(TOP_BAR_H)
                .show(ui, |ui| {
                    top_bar_body(ui, &theme, &mut focus, &t, off, &mut actions)
                });
        });
        out.textures_delta.clear();

        // 5 verbs + power + 3 toggles + tempo + numerator + denominator.
        assert_eq!(
            focus.items.len(),
            12,
            "expected every control to register; got {:?}",
            focus.items.len()
        );

        // Arrowing right from the leftmost reaches every one of them.
        let mut seen = 1;
        let mut at = focus.items[0].0;
        while let Some(next) = nearest(
            focus.rect_of(Some(at)).unwrap(),
            Dir::Right,
            &focus.items,
            Some(at),
        ) {
            at = next;
            seen += 1;
            assert!(seen <= 12, "navigation looped instead of terminating");
        }
        assert_eq!(
            seen, 12,
            "only {seen} of 12 controls are reachable rightwards"
        );
    }

    /// Ctrl+1 walks the grid finer, Ctrl+2 coarser, and both stop at the
    /// ends. Wrapping from 1/32 back to 1/1 mid-edit would be a nasty
    /// surprise, so it clamps.
    #[test]
    fn the_grid_ladder_steps_and_clamps() {
        // Narrow: 1/4 -> 1/8 -> 1/16 -> 1/32, then hold.
        assert_eq!(GRID_NAMES[GRID_DEFAULT], "1/4");
        let mut g = GRID_DEFAULT;
        for expected in ["1/8", "1/16", "1/32"] {
            g = step_grid(g, true);
            assert_eq!(GRID_NAMES[g], expected);
        }
        assert_eq!(step_grid(g, true), g, "narrowing past 1/32 should hold");

        // Widen back down, then hold at 1/1.
        for expected in ["1/16", "1/8", "1/4", "1/2", "1/1"] {
            g = step_grid(g, false);
            assert_eq!(GRID_NAMES[g], expected);
        }
        assert_eq!(step_grid(g, false), g, "widening past 1/1 should hold");

        // Every rung is half the one above it.
        for pair in GRID_BEATS.windows(2) {
            assert!(
                (pair[1] * 2.0 - pair[0]).abs() < 1e-6,
                "{pair:?} is not a halving"
            );
        }
    }

    /// The grid actions reach the arrangement, and leave the transport alone.
    #[test]
    fn grid_actions_move_the_grid_only() {
        let mut t = Transport::default();
        let mut a = Arrangement::default();

        perform(&[UiAction::NarrowGrid], &mut t, &mut a);
        assert_eq!(GRID_NAMES[a.grid], "1/8");
        assert_eq!(a.grid_beats(), 0.5, "1/8 is half a beat in 4/4");

        perform(&[UiAction::WidenGrid, UiAction::WidenGrid], &mut t, &mut a);
        assert_eq!(GRID_NAMES[a.grid], "1/2");

        // Nothing about the transport moved.
        let fresh = Transport::default();
        assert_eq!(
            (t.playing, t.position, t.bpm),
            (fresh.playing, fresh.position, fresh.bpm)
        );
    }

    /// Lanes stack from the top with no gaps or overlaps, and each keeps its
    /// own height.
    #[test]
    fn lanes_stack_without_gaps() {
        let area = Rect::from_min_size(pos2(0.0, 100.0), vec2(800.0, 400.0));
        let mut arr = Arrangement::default();
        arr.tracks[1].height = 120.0;

        let lanes = lane_rects(area, &arr.tracks);
        assert_eq!(lanes.len(), TRACK_COUNT);
        assert_eq!(
            lanes[0].top(),
            area.top(),
            "the first lane starts at the top"
        );
        assert_eq!(lanes[1].height(), 120.0, "a lane keeps its own height");

        for pair in lanes.windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].top(), "lanes must meet exactly");
        }
        for lane in &lanes {
            assert_eq!(lane.left(), area.left());
            assert_eq!(lane.right(), area.right());
        }
    }

    /// A lane cannot be dragged to nothing, or to swallow the view.
    #[test]
    fn lane_heights_are_clamped() {
        let lo = *TRACK_H_RANGE.start();
        let hi = *TRACK_H_RANGE.end();
        for raw in [-500.0, 0.0, 5.0, 64.0, 1000.0] {
            let clamped = f32::clamp(raw, lo, hi);
            assert!((lo..=hi).contains(&clamped), "{raw} escaped the range");
            assert!(
                clamped > 0.0,
                "a lane of zero height cannot be grabbed back"
            );
        }
    }

    /// Beats and pixels round-trip, and snapping lands on grid lines.
    #[test]
    fn the_arrangement_maps_beats_to_pixels() {
        let area = Rect::from_min_size(pos2(40.0, 0.0), vec2(800.0, 400.0));
        for beat in [0.0, 1.0, 4.5, 37.25] {
            let back = beat_at(area, 0.0, x_at(area, 0.0, beat));
            assert!((back - beat).abs() < 1e-3, "{beat} round-tripped to {back}");
        }
        // The offset is a view window, not a time change: the round trip
        // holds from anywhere in the song.
        for beat in [0.0, 1.0, 4.5, 37.25] {
            let back = beat_at(area, 24.0, x_at(area, 24.0, beat));
            assert!((back - beat).abs() < 1e-3, "{beat} round-tripped to {back}");
        }
        // Nothing before the start of the timeline.
        assert_eq!(beat_at(area, 0.0, area.left() - 500.0), 0.0);
        // A panned view puts absolute beats at the left edge and earlier
        // beats off-screen to the left.
        assert_eq!(x_at(area, 24.0, 24.0), area.left());
        assert!(x_at(area, 24.0, 0.0) < area.left());

        // Snapping lands on multiples of the grid, nearest wins.
        assert_eq!(snap(1.2, 1.0), 1.0);
        assert_eq!(snap(1.6, 1.0), 2.0);
        assert_eq!(snap(1.6, 0.5), 1.5);
        assert_eq!(snap(0.4, 4.0), 0.0);
        assert_eq!(snap(-3.0, 1.0), 0.0, "snapping cannot go negative");
        assert_eq!(snap(2.7, 0.0), 2.7, "a zero grid must not divide by zero");
    }

    /// Clips can be moved and resized, but never through a neighbour and
    /// never before beat 0. The clamps are the whole overlap story.
    #[test]
    fn clips_clamp_against_neighbours() {
        // a: 0..4, b: 4..8, c: 12..20 — four beats of free space between b
        // and c is the gap b is allowed to roam in.
        let track = vec![
            Clip {
                id: 1,
                name: "a".into(),
                start: 0.0,
                len: 4.0,
                notes: vec![],
            },
            Clip {
                id: 2,
                name: "b".into(),
                start: 4.0,
                len: 4.0,
                notes: vec![],
            },
            Clip {
                id: 3,
                name: "c".into(),
                start: 12.0,
                len: 8.0,
                notes: vec![],
            },
        ];

        // b moves freely inside the gap its neighbours leave...
        assert_eq!(clamp_clip_start(&track, 1, 6.0), 6.0);
        // ...but not past a's end...
        assert_eq!(clamp_clip_start(&track, 1, 1.0), 4.0);
        // ...nor over c's start (b is 4 long, so it must end by 12).
        assert_eq!(clamp_clip_start(&track, 1, 10.0), 8.0);
        // The first clip can never go negative.
        assert_eq!(clamp_clip_start(&track, 0, -3.0), 0.0);
        // The last clip has no right neighbour, but still respects a's... b's end.
        assert_eq!(clamp_clip_start(&track, 2, 500.0), 500.0);
        assert_eq!(clamp_clip_start(&track, 2, 0.0), 8.0);

        // Length: at least one grid unit, at most the gap to the next clip.
        assert_eq!(clamp_clip_len(&track, 0, 3.0, 1.0), 3.0);
        assert_eq!(
            clamp_clip_len(&track, 0, 5.0, 1.0),
            4.0,
            "no overlap with b"
        );
        assert_eq!(
            clamp_clip_len(&track, 0, 0.25, 1.0),
            1.0,
            "one grid unit minimum"
        );
        // A clip whose gap is smaller than the grid keeps its length
        // instead of violating either bound.
        let squeezed = vec![
            Clip {
                id: 1,
                name: "a".into(),
                start: 0.0,
                len: 0.75,
                notes: vec![],
            },
            Clip {
                id: 2,
                name: "b".into(),
                start: 0.75,
                len: 1.0,
                notes: vec![],
            },
        ];
        assert_eq!(clamp_clip_len(&squeezed, 0, 4.0, 1.0), 0.75);

        // A moved clip re-sorts to keep the sortedness invariant.
        let mut moved = track.clone();
        moved[2].start = 2.0;
        resort(&mut moved);
        assert_eq!(
            moved.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![1, 3, 2],
            "c moved to 2 must sort between a (0) and b (4)"
        );
        let mut jumped = track.clone();
        jumped[0].start = 6.0;
        resort(&mut jumped);
        assert_eq!(
            jumped.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![2, 1, 3],
            "a jumped to 6 must land between b (4) and c (12)"
        );
    }

    /// Notes map pitch to y with the high notes on top, and a clip's time
    /// range fills its rect edge to edge.
    #[test]
    fn notes_map_pitch_upward() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        let lo = note(48, 0.0, 1.0, 100);
        let hi = note(60, 0.0, 1.0, 100);
        let lo_rect = note_rect(area, &lo, 48, 60, 1.0);
        let hi_rect = note_rect(area, &hi, 48, 60, 1.0);
        assert!(
            hi_rect.top() < lo_rect.top(),
            "C4 must draw above C3, like a piano roll"
        );
        // A note spanning the whole clip spans the whole width.
        assert_eq!(lo_rect.left(), area.left());
        assert_eq!(lo_rect.right(), area.right());

        // A clip's rect: start at the left edge, length in beats to pixels.
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 400.0));
        let lane = Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 64.0));
        let clip = Clip {
            id: 9,
            name: "x".into(),
            start: 2.0,
            len: 4.0,
            notes: vec![],
        };
        let r = clip_rect(content, 0.0, lane, &clip);
        assert_eq!(r.left(), 2.0 * PX_PER_BEAT);
        assert_eq!(r.width(), 4.0 * PX_PER_BEAT);
        assert_eq!(r.height(), lane.height());
    }

    /// Follow pages the view: it only moves when the playhead leaves the
    /// window, and hands off means hands off.
    #[test]
    fn follow_pages_the_view() {
        // Follow off: nothing ever moves.
        assert_eq!(follow_view(10.0, 100.0, 30.0, false), 10.0);
        // Inside the window: stay put.
        assert_eq!(follow_view(10.0, 20.0, 30.0, true), 10.0);
        // Crossing the right edge jumps a page — the playhead lands at the
        // left edge.
        assert_eq!(follow_view(10.0, 40.0, 30.0, true), 40.0);
        // Rewinding behind the view jumps back to it.
        assert_eq!(follow_view(10.0, 5.0, 30.0, true), 5.0);
        // Exactly on the left edge is inside the window.
        assert_eq!(follow_view(10.0, 10.0, 30.0, true), 10.0);
    }

    /// The stand-in clock wraps around the loop region in beats. Dead or
    /// unreached loops are a no-op; overshoot wraps by the remainder.
    #[test]
    fn the_clock_wraps_at_the_loop() {
        // 120 bpm: 1 beat = 0.5s. Loop 4..8 beats = 2..4 seconds.
        let bpm = 120.0;
        // Before the loop: untouched.
        assert!((wrap_loop(1.0, bpm, 4.0, 8.0) - 1.0).abs() < 1e-9);
        // At the end: wraps to the start (4 beats = 2s).
        assert!((wrap_loop(4.0, bpm, 4.0, 8.0) - 2.0).abs() < 1e-9);
        // Past the end by half a loop: 2 + (9-8)*0.5 = 2.5s.
        assert!((wrap_loop(4.5, bpm, 4.0, 8.0) - 2.5).abs() < 1e-9);
        // Two loops past: 7s is 14 beats; (14 - 4) % 4 = 2, so it wraps to
        // 4 + 2 = 6 beats = 3s.
        assert!((wrap_loop(7.0, bpm, 4.0, 8.0) - 3.0).abs() < 1e-9);
        // A degenerate loop is a no-op, never a division by zero.
        assert_eq!(wrap_loop(3.0, bpm, 4.0, 4.0), 3.0);
    }

    /// The brace body carries the loop; the length is the handles' business.
    #[test]
    fn the_loop_body_moves_the_whole_loop() {
        assert_eq!(loop_move((4.0, 8.0), 2.0), (6.0, 10.0));
        assert_eq!(loop_move((4.0, 8.0), -2.0), (2.0, 6.0));
        assert_eq!(
            loop_move((2.0, 6.0), -3.0),
            (0.0, 4.0),
            "the loop stops at beat 0, never before"
        );
    }

    /// Dragging a clip's body moves it one grid step per step of the drag,
    /// snapped and clamped. The whole interact path — rect, id, delta —
    /// runs headless.
    #[test]
    fn dragging_a_clip_moves_it_on_the_grid() {
        let ctx = egui::Context::default();
        let mut arr = Arrangement::default();
        // A four-beat clip at the start of lane 0 — the test builds its own
        // content; the app ships with none.
        arr.create_clip(0, 0.0, 4.0).unwrap();
        arrangement_pass(&ctx, &mut arr, vec![]);
        assert_eq!(arr.clips[0][0].start, 0.0, "the clip starts at beat 0");

        // Lane 0 spans y 14..78 (ruler above). The clip spans x 0..96.
        // Like the browser-drag test: press in one pass, then move — one
        // pass registers the drag, the next reads it back.
        let press = pos2(TL + 48.0, 40.0);
        let release = pos2(TL + 48.0 + PX_PER_BEAT, 40.0);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(press),
                Event::PointerButton {
                    pos: press,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        arrangement_pass(&ctx, &mut arr, vec![Event::PointerMoved(release)]);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(release),
                Event::PointerButton {
                    pos: release,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(
            arr.clips[0][0].start, 1.0,
            "one grid step of drag moves the clip one beat"
        );
        assert_eq!(arr.selected_clip, Some((0, 0)), "the drag also selects");
    }

    /// Dragging the loop brace's body moves both ends, snapped to the grid.
    #[test]
    fn dragging_the_loop_brace_moves_the_loop() {
        let ctx = egui::Context::default();
        let mut arr = Arrangement {
            loop_range: Some((4.0, 8.0)),
            ..Default::default()
        };
        arrangement_pass(&ctx, &mut arr, vec![]);

        // The brace spans x 96..192 at y 2..12 (the ruler strip).
        let press = pos2(TL + 120.0, 7.0);
        let release = pos2(TL + 120.0 + PX_PER_BEAT, 7.0);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(press),
                Event::PointerButton {
                    pos: press,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        arrangement_pass(&ctx, &mut arr, vec![Event::PointerMoved(release)]);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(release),
                Event::PointerButton {
                    pos: release,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(
            arr.loop_range,
            Some((5.0, 9.0)),
            "the whole loop rides one grid step right"
        );
    }

    /// A harness that renders ONLY the arrangement into a headless context —
    /// no top bar, no browser, so the central panel starts at the window's
    /// origin and the lane coordinates above stay true.
    /// The timeline's left edge inside the test window. The track header
    /// column owns everything to the left of it, so every simulated
    /// pointer x below is measured FROM here — a bare `x` would land on a
    /// mute button instead of a clip.
    const TL: f32 = HEADER_W;

    fn arrangement_pass(ctx: &egui::Context, arr: &mut Arrangement, events: Vec<Event>) {
        let mut out = ctx.run_ui(input(events), |ui| {
            let theme = Theme::dark();
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme.bg))
                .show(ui, |ui| {
                    arrangement_body(ui, &mut Focus::default(), &theme, arr, 4, 0.0, false);
                });
        });
        out.textures_delta.clear();
    }

    /// `place_clip` is the placement policy behind create, paste, duplicate
    /// and the ghost: first gap at or after the asked-for beat that fits,
    /// else the end of the track.
    #[test]
    fn place_clip_finds_the_first_fitting_gap() {
        let track = vec![
            Clip {
                id: 1,
                name: "a".into(),
                start: 0.0,
                len: 4.0,
                notes: vec![],
            },
            Clip {
                id: 2,
                name: "b".into(),
                start: 8.0,
                len: 2.0,
                notes: vec![],
            },
        ];
        // A small clip fits in the 4..8 gap, exactly where asked.
        assert_eq!(place_clip(&track, 6.0, 1.0), (6.0, 1));
        // A bigger clip is clamped to the gap's start.
        assert_eq!(place_clip(&track, 6.0, 4.0), (4.0, 1));
        // Too big for the gap: the next gap is the end of the track.
        assert_eq!(place_clip(&track, 5.0, 6.0), (10.0, 2));
        // Asking inside a clip pushes to the gap after it.
        assert_eq!(place_clip(&track, 0.5, 1.0), (4.0, 1));
        // A negative ask is clamped: the first gap at or after -3 is the
        // 4..8 one, since a occupies 0..4 — and on an empty track the clamp
        // lands at beat 0 itself.
        assert_eq!(place_clip(&track, -3.0, 1.0), (4.0, 1));
        assert_eq!(place_clip(&[], -3.0, 1.0), (0.0, 0));
        assert_eq!(place_clip(&[], 3.0, 4.0), (3.0, 0));
    }

    /// The whole lifecycle, in order: create, copy, paste, duplicate,
    /// remove — ids, names, placement and selection each time.
    #[test]
    fn clips_go_through_the_full_lifecycle() {
        let mut a = Arrangement::default();

        // Create on lane 3: one bar long, auto-named, selected. Ids start
        // at 1, because nothing was there before the user.
        let idx = a.create_clip(3, 2.0, 4.0).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(a.clips[3].len(), 1);
        assert_eq!(a.clips[3][0].start, 2.0);
        assert_eq!(a.clips[3][0].name, "clip 1", "the first clip is clip 1");
        assert_eq!(a.selected_clip, Some((3, 0)));

        // Copy with no selection copies nothing.
        a.selected_clip = None;
        a.copy_selected();
        assert!(a.clipboard.is_none());
        a.selected_clip = Some((3, 0));
        a.copy_selected();
        assert_eq!(a.clipboard.as_ref().unwrap().name, "clip 1");

        // Paste: lands at the beat asked for, new id, clipboard survives.
        let pasted = a.paste_clipboard(3, 6.0).unwrap();
        assert_eq!(a.clips[3].len(), 2);
        assert_eq!(a.clips[3][pasted].start, 6.0);
        assert!(a.clipboard.is_some(), "paste must not empty the clipboard");
        assert_ne!(a.clips[3][pasted].id, a.clips[3][0].id);
        assert_eq!(a.selected_clip, Some((3, pasted)), "the paste is selected");

        // Duplicate of the first clip: the 2..6 clip's copy can't land at 6
        // (occupied), so it rides to the next gap at 10.
        a.selected_clip = Some((3, 0));
        a.duplicate_selected().unwrap();
        assert_eq!(a.clips[3].len(), 3);
        let starts: Vec<f32> = a.clips[3].iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![2.0, 6.0, 10.0]);

        // The fallbacks: paste goes to the selected clip's track, else the
        // selected lane; the beat comes from the cursor, else the selection,
        // else the playhead.
        a.selected_clip = None;
        a.selected = Some(2);
        a.cursor = Some((2, 3.0));
        assert_eq!(a.paste_track(), 2);
        assert_eq!(a.paste_beat(99.0), 3.0);
        a.cursor = None;
        a.selection = Some((5.0, 6.0));
        assert_eq!(a.paste_beat(99.0), 5.0);
        a.selection = None;
        assert_eq!(a.paste_beat(8.4), 8.0, "playhead fallback, snapped");

        // Remove by id (the menu's Delete): gone, and selection clears when
        // it held the removed clip.
        let victim = a.clips[3][0].id;
        a.selected_clip = Some((3, 0));
        assert!(a.remove_clip(3, victim));
        assert_eq!(a.clips[3].len(), 2);
        assert_eq!(a.selected_clip, None);
        assert!(
            !a.remove_clip(3, victim),
            "a second remove is a clean no-op"
        );
    }

    /// Double-clicking an empty lane creates a one-bar clip at the click.
    /// The whole path — hit test, double-click detection, snap, placement —
    /// runs headless.
    #[test]
    fn double_click_creates_a_clip() {
        let ctx = egui::Context::default();
        let mut arr = Arrangement::default();
        arrangement_pass(&ctx, &mut arr, vec![]);
        assert!(
            arr.clips.iter().all(|t| t.is_empty()),
            "a fresh session has no clips at all"
        );

        // Lane 2 spans y 142..206. Click at beat 2, twice — one pass per
        // click, so egui sees two clicks in a row at one position.
        let pos = pos2(TL + 2.0 * PX_PER_BEAT, 170.0);
        for _ in 0..2 {
            arrangement_pass(
                &ctx,
                &mut arr,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert_eq!(arr.clips[2].len(), 1, "double-click created one clip");
        assert_eq!(arr.clips[2][0].start, 2.0, "snapped to the grid");
        assert_eq!(arr.clips[2][0].len, 4.0, "one bar long at 4/4");
        assert_eq!(arr.selected_clip, Some((2, 0)), "the new clip is selected");
    }

    /// Ctrl+drag copies: the original stays put and the copy lands where the
    /// pointer is released, at the first gap that fits it.
    #[test]
    fn ctrl_drag_duplicates_into_the_gap() {
        let ctx = egui::Context::default();
        let mut arr = Arrangement::default();
        // 0..4 and 8..10, so there is a gap to land in and a clip past it.
        arr.create_clip(0, 0.0, 4.0).unwrap();
        arr.create_clip(0, 8.0, 2.0).unwrap();
        arrangement_pass(&ctx, &mut arr, vec![]);
        assert_eq!(arr.clips[0].len(), 2, "two clips to drag between");

        // Press on the first clip (x 0..96, lane 0 y 14..78) with the
        // command modifier held (egui learns modifiers from
        // ModifiersChanged events), then drag 10 beats right.
        let press = pos2(TL + 48.0, 40.0);
        let release = pos2(TL + 48.0 + 10.0 * PX_PER_BEAT, 40.0);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(press),
                Event::ModifiersChanged(egui::Modifiers::COMMAND),
                Event::PointerButton {
                    pos: press,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::COMMAND,
                },
            ],
        );

        arrangement_pass(&ctx, &mut arr, vec![Event::PointerMoved(release)]);
        arrangement_pass(
            &ctx,
            &mut arr,
            vec![
                Event::PointerMoved(release),
                Event::PointerButton {
                    pos: release,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
            ],
        );

        assert_eq!(arr.clips[0].len(), 3, "the ghost became a real clip");
        assert_eq!(arr.clips[0][0].start, 0.0, "the original never moved");
        assert_eq!(
            arr.clips[0][2].start, 10.0,
            "the copy landed at the release beat, past the second clip"
        );
        assert_eq!(arr.selected_clip, Some((0, 2)), "the copy is selected");
    }

    /// A selection is ordered and never empty. A click with no drag still
    /// selects a grid unit, so Ctrl+L always has something to work with.
    #[test]
    fn a_selection_is_ordered_and_never_empty() {
        // Dragged backwards, it comes out ordered.
        assert_eq!(span(8.0, 4.0, 1.0), (4.0, 8.0));
        assert_eq!(span(4.0, 8.0, 1.0), (4.0, 8.0));

        // A click (both ends equal) still spans one grid unit.
        assert_eq!(span(4.0, 4.0, 1.0), (4.0, 5.0));
        assert_eq!(span(4.0, 4.0, 0.25), (4.0, 4.25));

        // A drag shorter than the grid is widened to it, not collapsed.
        let (lo, hi) = span(4.0, 4.1, 1.0);
        assert!(hi - lo >= 1.0, "a sub-grid drag should still be selectable");
    }

    /// Ctrl+L turns the selection into the loop AND enables looping — the
    /// gesture says what was meant.
    #[test]
    fn ctrl_l_loops_the_selection() {
        let mut t = Transport::default();
        let mut a = Arrangement::default();

        // With nothing selected there is nothing to loop.
        perform(&[UiAction::LoopFromSelection], &mut t, &mut a);
        assert_eq!(a.loop_range, None);
        assert!(!t.loop_on, "an empty selection must not arm looping");

        a.selected = Some(2);
        a.selection = Some((4.0, 8.0));
        perform(&[UiAction::LoopFromSelection], &mut t, &mut a);
        assert_eq!(a.loop_range, Some((4.0, 8.0)));
        assert!(t.loop_on, "looping the selection should switch looping on");

        // Selecting elsewhere does not disturb the loop already made.
        a.selection = Some((16.0, 20.0));
        assert_eq!(a.loop_range, Some((4.0, 8.0)));
    }

    /// Selection belongs to one track. Picking another lane moves it whole
    /// rather than spanning both.
    #[test]
    fn selection_lives_in_exactly_one_track() {
        let mut a = Arrangement {
            selected: Some(1),
            selection: Some(span(2.0, 6.0, 1.0)),
            ..Default::default()
        };
        assert_eq!((a.selected, a.selection), (Some(1), Some((2.0, 6.0))));

        // Picking lane 4 replaces both halves; there is no second selection.
        a.selected = Some(4);
        a.selection = Some(span(9.0, 9.0, 1.0));
        assert_eq!(a.selected, Some(4));
        assert_eq!(a.selection, Some((9.0, 10.0)));
    }

    /// The cell cursor walks the grid, one division per press, and the cell
    /// it lands on IS the selection — so Ctrl+L works straight from the
    /// keyboard.
    #[test]
    fn the_cursor_selects_grid_cells() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            cursor: Some((2, 0.0)),
            ..Default::default()
        };
        assert_eq!(a.grid_beats(), 1.0, "this test assumes the 1/4 default");

        perform(&[UiAction::MoveCell(1)], &mut t, &mut a);
        assert_eq!(a.cursor, Some((2, 1.0)), "one press moves one division");
        assert_eq!(a.selected, Some(2), "the cursor selects its own track");
        assert_eq!(a.selection, Some((1.0, 2.0)), "the cell IS the selection");

        // Ctrl+L straight after, with no mouse involved.
        perform(&[UiAction::LoopFromSelection], &mut t, &mut a);
        assert_eq!(a.loop_range, Some((1.0, 2.0)));
        assert!(t.loop_on);

        // The step follows the grid: narrow it and the cursor moves less.
        perform(
            &[UiAction::NarrowGrid, UiAction::MoveCell(1)],
            &mut t,
            &mut a,
        );
        assert_eq!(a.grid_beats(), 0.5);
        assert_eq!(a.cursor, Some((2, 1.5)), "a 1/8 grid steps half a beat");
    }

    /// The cursor cannot walk off the front of the timeline.
    #[test]
    fn the_cursor_stops_at_beat_zero() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            cursor: Some((0, 1.0)),
            ..Default::default()
        };
        perform(&[UiAction::MoveCell(-1)], &mut t, &mut a);
        assert_eq!(a.cursor, Some((0, 0.0)));
        perform(&[UiAction::MoveCell(-1)], &mut t, &mut a);
        assert_eq!(a.cursor, Some((0, 0.0)), "there is nothing before bar 1");
        assert_eq!(a.selection, Some((0.0, 1.0)), "the selection stays valid");
    }

    /// Cells in different lanes share an x, so Up and Down keep your place
    /// in time. That is what makes the generic spatial navigation work here.
    #[test]
    fn cells_line_up_across_lanes() {
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0));
        let arr = Arrangement::default();
        let lanes = lane_rects(content, &arr.tracks);
        let grid = arr.grid_beats();

        let a = cell_rect(content, 0.0, lanes[0], 4.0, grid);
        let b = cell_rect(content, 0.0, lanes[3], 4.0, grid);
        assert_eq!(a.left(), b.left(), "cells at the same beat must share an x");
        assert_eq!(a.width(), b.width());
        assert_eq!(
            a.width(),
            grid * PX_PER_BEAT,
            "a cell is one grid division wide"
        );
        assert_eq!(a.height(), lanes[0].height(), "a cell is its lane's height");

        // And Down from lane 0 finds lane 1, not something on the bar.
        let items: Vec<(egui::Id, Rect)> = lanes
            .iter()
            .enumerate()
            .map(|(i, l)| {
                (
                    egui::Id::new(("lane", i)),
                    cell_rect(content, 0.0, *l, 4.0, grid),
                )
            })
            .collect();
        assert_eq!(
            nearest(items[0].1, Dir::Down, &items, Some(items[0].0)),
            Some(items[1].0)
        );
    }

    /// `arrangement_keys` must get first refusal on the arrows. `Focus`
    /// consumes all four unconditionally, so running it first swallows Right
    /// and the ring walks off to the tempo field instead of along the grid.
    #[test]
    fn the_grid_gets_first_refusal_on_the_arrows() {
        let right = || {
            input(vec![Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }])
        };
        let on_a_cell = Arrangement {
            owns_arrows: true,
            cursor: Some((0, 4.0)),
            ..Default::default()
        };

        // Correct order: the grid claims it, and Focus sees nothing.
        let ctx = egui::Context::default();
        let mut focus = Focus::default();
        let mut actions = Vec::new();
        let mut out = ctx.run_ui(right(), |_ui| {});
        out.textures_delta.clear();
        arrangement_keys(&ctx, &on_a_cell, &mut actions);
        focus.begin(&ctx);
        assert_eq!(
            actions,
            vec![UiAction::MoveCell(1)],
            "the grid should claim Right"
        );
        assert_eq!(focus.pending, None, "Focus must not also get the key");

        // Wrong order, for contrast: Focus eats it and the grid gets nothing.
        let ctx = egui::Context::default();
        let mut focus = Focus::default();
        let mut actions = Vec::new();
        let mut out = ctx.run_ui(right(), |_ui| {});
        out.textures_delta.clear();
        focus.begin(&ctx);
        arrangement_keys(&ctx, &on_a_cell, &mut actions);
        assert_eq!(focus.pending, Some(Dir::Right));
        assert!(actions.is_empty(), "this is the bug the ordering prevents");
    }

    /// At beat 0 the arrangement deliberately does NOT claim Left, so there
    /// is always a way back out to the browser.
    #[test]
    fn left_at_beat_zero_escapes_the_arrangement() {
        let left = || {
            input(vec![Event::Key {
                key: egui::Key::ArrowLeft,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }])
        };

        for (beat, claimed) in [(0.0, false), (4.0, true)] {
            let arr = Arrangement {
                owns_arrows: true,
                cursor: Some((0, beat)),
                ..Default::default()
            };
            let ctx = egui::Context::default();
            let mut focus = Focus::default();
            let mut actions = Vec::new();
            let mut out = ctx.run_ui(left(), |_ui| {});
            out.textures_delta.clear();
            arrangement_keys(&ctx, &arr, &mut actions);
            focus.begin(&ctx);

            if claimed {
                assert_eq!(
                    actions,
                    vec![UiAction::MoveCell(-1)],
                    "beat {beat} should step"
                );
                assert_eq!(focus.pending, None);
            } else {
                assert!(actions.is_empty(), "beat 0 must not claim Left");
                assert_eq!(
                    focus.pending,
                    Some(Dir::Left),
                    "it should fall through to navigation"
                );
            }
        }
    }

    /// Shift+arrow grows the selection from an anchor, in either direction,
    /// and can shrink back through itself without the range inverting.
    #[test]
    fn shift_arrow_extends_the_selection() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            cursor: Some((0, 4.0)),
            anchor: 4.0,
            ..Default::default()
        };
        let grid = a.grid_beats();
        assert_eq!(grid, 1.0, "this test assumes the 1/4 default");

        // One cell to start with.
        perform(&[UiAction::MoveCell(0)], &mut t, &mut a);
        assert_eq!(a.selection, Some((4.0, 5.0)));

        // Rightwards: each press adds the cell after.
        perform(&[UiAction::ExtendCell(1)], &mut t, &mut a);
        assert_eq!(a.selection, Some((4.0, 6.0)), "cells 4 and 5");
        perform(&[UiAction::ExtendCell(1)], &mut t, &mut a);
        assert_eq!(a.selection, Some((4.0, 7.0)), "cells 4, 5 and 6");
        assert_eq!(a.anchor, 4.0, "the anchor must not drift");

        // Back through the anchor: the range shrinks, then grows the other
        // way, and never inverts.
        for expected in [(4.0, 6.0), (4.0, 5.0), (3.0, 5.0), (2.0, 5.0)] {
            perform(&[UiAction::ExtendCell(-1)], &mut t, &mut a);
            let (lo, hi) = a.selection.unwrap();
            assert!(lo < hi, "selection inverted to ({lo}, {hi})");
            assert_eq!(a.selection, Some(expected));
        }
    }

    /// A plain arrow collapses the selection again — it moves, it does not
    /// extend.
    #[test]
    fn a_plain_arrow_collapses_the_selection() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            cursor: Some((0, 4.0)),
            anchor: 4.0,
            ..Default::default()
        };
        perform(
            &[UiAction::ExtendCell(1), UiAction::ExtendCell(1)],
            &mut t,
            &mut a,
        );
        assert_eq!(a.selection, Some((4.0, 7.0)));

        perform(&[UiAction::MoveCell(1)], &mut t, &mut a);
        assert_eq!(a.selection, Some((7.0, 8.0)), "one cell again");
        assert_eq!(a.anchor, 7.0, "and the anchor comes with it");
    }

    /// Extending cannot walk off the front of the timeline.
    #[test]
    fn extending_clamps_at_beat_zero() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            cursor: Some((0, 1.0)),
            anchor: 1.0,
            ..Default::default()
        };
        for _ in 0..5 {
            perform(&[UiAction::ExtendCell(-1)], &mut t, &mut a);
        }
        assert_eq!(a.cursor, Some((0, 0.0)));
        assert_eq!(a.selection, Some((0.0, 2.0)), "beat 0 through the anchor");

        // Which is still a valid loop.
        perform(&[UiAction::LoopFromSelection], &mut t, &mut a);
        assert_eq!(a.loop_range, Some((0.0, 2.0)));
    }

    /// `extended` is symmetric: which side the cursor is on does not change
    /// the cells covered.
    #[test]
    fn extension_is_symmetric_about_the_anchor() {
        assert_eq!(
            extended(4.0, 4.0, 1.0),
            (4.0, 5.0),
            "anchor alone is one cell"
        );
        assert_eq!(extended(4.0, 6.0, 1.0), (4.0, 7.0));
        assert_eq!(
            extended(6.0, 4.0, 1.0),
            (4.0, 7.0),
            "same cells the other way round"
        );
        assert_eq!(
            extended(4.0, 4.5, 0.5),
            (4.0, 5.0),
            "the grid sets the step"
        );
    }

    /// The KEY MAPPING, not just the action. `consume_key` matches modifiers
    /// logically and so ignores an extra Shift — meaning a plain-arrow check
    /// placed first silently swallows Shift+arrow and moves the cursor
    /// instead of extending the selection. The earlier tests drove `perform`
    /// directly and could not see this at all.
    #[test]
    fn shift_arrow_reaches_the_extend_action() {
        let arrow = |key: egui::Key, shift: bool| {
            let modifiers = egui::Modifiers {
                shift,
                ..Default::default()
            };
            input(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        };
        let on_a_cell = Arrangement {
            owns_arrows: true,
            cursor: Some((0, 4.0)),
            anchor: 4.0,
            ..Default::default()
        };

        for (key, shift, expected) in [
            (egui::Key::ArrowRight, true, UiAction::ExtendCell(1)),
            (egui::Key::ArrowLeft, true, UiAction::ExtendCell(-1)),
            (egui::Key::ArrowRight, false, UiAction::MoveCell(1)),
            (egui::Key::ArrowLeft, false, UiAction::MoveCell(-1)),
        ] {
            let ctx = egui::Context::default();
            let mut focus = Focus::default();
            let mut actions = Vec::new();
            let mut out = ctx.run_ui(arrow(key, shift), |_ui| {});
            out.textures_delta.clear();
            arrangement_keys(&ctx, &on_a_cell, &mut actions);
            focus.begin(&ctx);

            assert_eq!(
                actions,
                vec![expected],
                "{key:?} with shift={shift} produced {actions:?}"
            );
            assert_eq!(
                focus.pending, None,
                "the key should not also reach navigation"
            );
        }
    }

    /// Painting inside the browser must not change the size it reports, or
    /// the seam mark and the drag would drift apart.
    #[test]
    fn painting_the_bands_does_not_resize_the_browser() {
        let ctx = egui::Context::default();
        for _ in 0..3 {
            assert_eq!(pass(&ctx, vec![]), BROWSER_W);
        }
    }

    /// The drag is clamped at both ends of BROWSER_W_RANGE.
    #[test]
    fn the_browser_cannot_be_dragged_outside_its_range() {
        for (aim, expect) in [
            (20.0_f32, *BROWSER_W_RANGE.start()),
            (1400.0, *BROWSER_W_RANGE.end()),
        ] {
            let ctx = egui::Context::default();
            pass(&ctx, vec![]);
            let edge = pos2(BROWSER_W, SCREEN.y * 0.5);
            pass(
                &ctx,
                vec![
                    Event::PointerMoved(edge),
                    Event::PointerButton {
                        pos: edge,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
            );
            let moved = pos2(aim, SCREEN.y * 0.5);
            pass(&ctx, vec![Event::PointerMoved(moved)]);
            let width = pass(&ctx, vec![Event::PointerMoved(moved)]);
            assert!(
                (width - expect).abs() < 2.0,
                "dragging toward {aim}px should clamp to {expect}px, got {width}px"
            );
        }
    }

    /// Clip-relative becomes absolute at the engine boundary, and notes
    /// past the clip's end do not sound.
    #[test]
    fn clip_notes_become_absolute_engine_notes() {
        let clips = vec![
            Clip {
                id: 1,
                name: "a".into(),
                start: 4.0,
                len: 2.0,
                notes: vec![note(60, 0.0, 1.0, 100), note(62, 1.5, 0.5, 90)],
            },
            Clip {
                id: 2,
                name: "b".into(),
                start: 16.0,
                len: 1.0,
                notes: vec![note(48, 0.25, 0.25, 1)],
            },
        ];
        let out = seq_notes(&clips);
        assert_eq!(out.len(), 3);
        // Beat 0 of a clip at beat 4 sounds at beat 4.
        assert_eq!(
            (
                out[0].start_beats,
                out[0].len_beats,
                out[0].pitch,
                out[0].vel
            ),
            (4.0, 1.0, 60, 100)
        );
        assert_eq!(out[1].start_beats, 5.5, "1.5 into a clip at 4");
        assert_eq!(
            (out[2].start_beats, out[2].pitch, out[2].vel),
            (16.25, 48, 1)
        );

        // A note starting at or past the clip's end is silent — the clip
        // was shortened over it, not edited.
        let hidden = vec![Clip {
            id: 3,
            name: "c".into(),
            start: 0.0,
            len: 1.0,
            notes: vec![
                note(60, 0.0, 1.0, 100),
                note(64, 1.0, 1.0, 100),
                note(67, 8.0, 1.0, 100),
            ],
        }];
        let out = seq_notes(&hidden);
        assert_eq!(out.len(), 1, "only the note inside the clip sounds");
        assert_eq!(out[0].pitch, 60);

        // A note that STARTS inside keeps its full length, even ringing past.
        let ringing = vec![Clip {
            id: 4,
            name: "d".into(),
            start: 0.0,
            len: 1.0,
            notes: vec![note(60, 0.5, 4.0, 100)],
        }];
        assert_eq!(seq_notes(&ringing)[0].len_beats, 4.0);

        assert!(seq_notes(&[]).is_empty());
    }

    /// The debounce: changed material recompiles only once the window has
    /// passed, and unchanged material never recompiles at all.
    #[test]
    fn note_edits_recompile_at_most_once_per_window() {
        // Mid-drag: dirty, but inside the window — wait.
        assert!(!recompile_due(true, 0.0));
        assert!(!recompile_due(true, RECOMPILE_MIN_SECS * 0.5));
        // Window passed and still dirty: one swap.
        assert!(recompile_due(true, RECOMPILE_MIN_SECS));
        assert!(recompile_due(true, RECOMPILE_MIN_SECS * 10.0));
        // Clean means clean, however long it has been.
        assert!(!recompile_due(false, 0.0));
        assert!(!recompile_due(false, f64::INFINITY));
        // No schedule yet (last_compile None maps to INFINITY): dirty
        // material compiles immediately.
        assert!(recompile_due(true, f64::INFINITY));
    }

    /// The dirty check is the clips themselves, so every edit the user can
    /// make to sounding material trips it — and nothing else does.
    #[test]
    fn every_clip_edit_is_a_recompile() {
        let mut a = Arrangement::default();
        a.create_clip(0, 0.0, 4.0).unwrap();
        let compiled = a.clips.clone();
        assert_eq!(a.clips, compiled, "an untouched arrangement is clean");

        // A note edited inside a clip.
        a.clips[0][0].notes.push(note(60, 0.0, 1.0, 100));
        assert_ne!(a.clips, compiled);

        // A clip moved, renamed, resized, added, removed.
        for edit in [0, 1, 2, 3, 4] {
            let mut b = compiled.clone();
            match edit {
                0 => b[0][0].start += 1.0,
                1 => b[0][0].name = "renamed".into(),
                2 => b[0][0].len = 8.0,
                3 => {
                    let moved = b[0][0].clone();
                    b[1].push(moved);
                }
                _ => b[0].clear(),
            }
            assert_ne!(b, compiled, "edit {edit} should be a recompile");
        }
    }

    /// The graph the app builds actually compiles: one Seq per track, each
    /// through its own Pan into the mixer, plus a Click when the metronome
    /// is on.
    #[test]
    fn the_app_graph_compiles_with_and_without_the_click() {
        let mut a = Arrangement::default();
        a.create_clip(0, 0.0, 4.0).unwrap();
        a.clips[0][0].notes.push(note(60, 0.0, 1.0, 100));
        // Only tracks holding an instrument become nodes.
        for t in a.tracks.iter_mut() {
            t.device = Some(DeviceKind::SineSynth);
        }

        for (metronome, extra) in [(false, 0), (true, 1)] {
            let (spec, nodes) = build_graph_spec(&a.tracks, &a.clips, Some(8.0), metronome);
            assert_eq!(
                nodes.seqs.iter().filter(|s| s.is_some()).count(),
                TRACK_COUNT,
                "one Seq per track holding a device, empty of clips or not"
            );
            assert_eq!(
                nodes.pans.iter().filter(|p| p.is_some()).count(),
                TRACK_COUNT,
                "every instrument track carries a Pan, so pan is a letter"
            );
            // Two wires per track — seq -> pan, pan -> mixer — plus the
            // click's one.
            assert_eq!(
                spec.wires().len(),
                TRACK_COUNT * 2 + extra,
                "every track reaches the mixer through its own pan"
            );
            let output = spec.output().unwrap();
            assert!(
                !nodes.seqs.contains(&Some(output)),
                "the mixer, not a seq, feeds the speakers"
            );
            for (seq, pan) in nodes.seqs.iter().zip(&nodes.pans) {
                assert!(
                    spec.wires()
                        .iter()
                        .any(|(f, t)| Some(*f) == *seq && Some(*t) == *pan),
                    "every track's seq feeds its own pan"
                );
                assert!(
                    spec.wires()
                        .iter()
                        .any(|(f, t)| Some(*f) == *pan && *t == output),
                    "every track's pan feeds the mixer"
                );
            }
            assert!(
                spec.compile(48_000, 256).is_ok(),
                "the app's graph shape must compile"
            );
        }
    }

    /// An empty arrangement — a fresh session — is still a valid graph.
    /// Silent, but a graph, so pressing play before making a clip is not a
    /// failure path.
    #[test]
    fn an_empty_arrangement_still_compiles() {
        let mut a = Arrangement::default();
        assert!(
            a.clips.iter().all(|t| t.is_empty()),
            "a fresh session has no clips"
        );

        // A fresh session has no INSTRUMENTS either, so no track becomes a
        // node — the graph is empty and still compiles.
        let (spec, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert!(nodes.seqs.iter().all(|s| s.is_none()), "no device, no node");
        assert!(spec.compile(48_000, 256).is_ok());

        // Load an instrument on every track and each becomes one.
        for t in a.tracks.iter_mut() {
            t.device = Some(DeviceKind::SineSynth);
        }
        let (spec, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert_eq!(
            nodes.seqs.iter().filter(|s| s.is_some()).count(),
            TRACK_COUNT
        );
        assert!(spec.compile(48_000, 256).is_ok());

        // And so is one with no tracks at all.
        let (spec, nodes) = build_graph_spec(&[], &[], None, true);
        assert!(nodes.seqs.is_empty());
        assert!(spec.compile(48_000, 256).is_ok());
    }

    /// Loading a device is what gives a track a voice, and the graph must
    /// notice: a track that gains an instrument gains a NODE, which no
    /// parameter letter can express.
    #[test]
    fn loading_a_device_changes_the_graph_shape() {
        let mut a = Arrangement::default();
        let mask = |a: &Arrangement| {
            a.tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.device.is_some())
                .fold(0u64, |m, (i, _)| m | (1 << i))
        };
        assert_eq!(mask(&a), 0, "a fresh session holds no instruments");

        a.tracks[2].device = Some(DeviceKind::SineSynth);
        assert_eq!(mask(&a), 1 << 2, "the mask names WHICH track, not how many");

        // Only that track compiles to a node, and it lands at its own index
        // so `seq_ids[track]` stays the right address.
        let (_, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert!(nodes.seqs[2].is_some());
        assert!(nodes.seqs[0].is_none() && nodes.seqs[1].is_none() && nodes.seqs[3].is_none());
    }

    // --- tracks -----------------------------------------------------------

    /// A fresh track is fully formed: kind, a name from that kind's own
    /// counter, its own clip vec, and the selection moved onto it. Every
    /// route in (Ctrl+T, the palette) goes through `add_track`, so getting
    /// this right once is getting it right everywhere.
    #[test]
    fn adding_a_track_names_it_and_selects_it() {
        let mut a = Arrangement::default();
        let before = a.tracks.len();

        let i = a.add_track(TrackKind::Audio);
        assert_eq!(i, before, "the new lane is appended, not inserted");
        assert_eq!(a.tracks.len(), before + 1);
        assert_eq!(a.clips.len(), a.tracks.len(), "clips stay parallel");
        assert_eq!(a.tracks[i].kind, TrackKind::Audio);
        assert_eq!(a.tracks[i].name, "Audio 1", "audio numbering starts fresh");
        assert_eq!(a.selected, Some(i), "a new track is what you work on next");
        assert_eq!(a.selected_clip, None);

        // The two kinds count separately: the default session's four MIDI
        // lanes have taken 1..=4, so the next MIDI track is 5.
        let m = a.add_track(TrackKind::Midi);
        assert_eq!(a.tracks[m].name, "MIDI 5");
        assert_eq!(a.add_track(TrackKind::Audio), m + 1);
        assert_eq!(a.tracks[m + 1].name, "Audio 2");
    }

    /// Names come from a MONOTONIC counter, not from the track count.
    /// Deleting "Audio 1" and adding another must not produce a second
    /// "Audio 1" — two lanes with one name is a UI that lies.
    #[test]
    fn track_names_never_repeat_after_a_delete() {
        let mut a = Arrangement::default();
        let first = a.add_track(TrackKind::Audio);
        assert_eq!(a.tracks[first].name, "Audio 1");
        assert!(a.remove_track(first));
        let second = a.add_track(TrackKind::Audio);
        assert_eq!(a.tracks[second].name, "Audio 2", "the counter moved on");
    }

    /// Removing a track takes its clips with it and REPAIRS every index
    /// pointing into the list. An index left pointing past the hole is the
    /// bug that silently edits the wrong lane.
    #[test]
    fn removing_a_track_repairs_the_indices() {
        let mut a = Arrangement::default();
        a.create_clip(2, 0.0, 4.0).unwrap();
        a.create_clip(3, 0.0, 4.0).unwrap();
        a.selected_clip = Some((3, 0));
        a.selected = Some(3);
        a.cursor = Some((3, 8.0));

        assert!(a.remove_track(1), "a middle track goes");
        assert_eq!(a.tracks.len(), TRACK_COUNT - 1);
        assert_eq!(a.clips.len(), a.tracks.len());
        // Everything that pointed at 3 now points at 2, and the clip it
        // named came with it.
        assert_eq!(a.selected, Some(2));
        assert_eq!(a.selected_clip, Some((2, 0)));
        assert_eq!(a.cursor, Some((2, 8.0)));
        assert_eq!(a.clips[2].len(), 1, "the clip followed its lane");

        // Removing the SELECTED track drops the selection rather than
        // silently moving it to whatever slid into the slot.
        a.selected = Some(2);
        a.selected_clip = Some((2, 0));
        assert!(a.remove_track(2));
        assert_eq!(a.selected, None);
        assert_eq!(a.selected_clip, None);
    }

    /// The last lane is never removed: an arrangement with no tracks has
    /// nothing to click, and there is no undo to get back out of it.
    #[test]
    fn the_last_track_stays() {
        let mut a = Arrangement::default();
        while a.tracks.len() > 1 {
            assert!(a.remove_track(0));
        }
        assert!(!a.remove_track(0), "the last one is refused");
        assert_eq!(a.tracks.len(), 1);
    }

    /// Solo-in-place, and mute beating solo. One rule, `track_audible`,
    /// which both the schedule and the header dimming read — a header that
    /// disagreed with what is wired would be the worst bug available here.
    #[test]
    fn mute_and_solo_decide_what_reaches_the_mixer() {
        let mut a = Arrangement::default();
        // Nothing engaged: everything sounds.
        assert!((0..a.tracks.len()).all(|i| track_audible(&a.tracks, i)));

        a.tracks[1].mute = true;
        assert!(!track_audible(&a.tracks, 1));
        assert!(track_audible(&a.tracks, 0), "muting one is not muting all");

        // Solo anywhere silences every track that is not soloed.
        a.tracks[2].solo = true;
        assert!(track_audible(&a.tracks, 2));
        assert!(!track_audible(&a.tracks, 0), "solo-in-place");
        assert!(!track_audible(&a.tracks, 3));

        // Mute wins over solo — both lit means silent, which is what the
        // two buttons showing at once has to mean.
        a.tracks[2].mute = true;
        assert!(!track_audible(&a.tracks, 2));
    }

    /// A muted track leaves the SCHEDULE. Silence by absence, not by a
    /// node multiplying by zero — the graph should be as small as what is
    /// actually sounding.
    #[test]
    fn a_muted_track_is_not_compiled() {
        let mut a = Arrangement::default();
        for t in a.tracks.iter_mut() {
            t.device = Some(DeviceKind::SineSynth);
        }
        let (_, all) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert_eq!(all.seqs.iter().filter(|s| s.is_some()).count(), TRACK_COUNT);

        a.tracks[1].mute = true;
        let (_, muted) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert!(muted.seqs[1].is_none(), "a muted track makes no node");
        assert!(muted.pans[1].is_none(), "and no pan either");
        assert_eq!(muted.seqs.iter().filter(|s| s.is_some()).count(), 3);

        // Solo drops everything else the same way.
        a.tracks[1].mute = false;
        a.tracks[0].solo = true;
        let (_, soloed) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert!(soloed.seqs[0].is_some());
        assert_eq!(soloed.seqs.iter().filter(|s| s.is_some()).count(), 1);
    }

    /// An audio track compiles to nothing: it has no instrument slot, and
    /// its material is the sound. A lane that looked live and was silent
    /// would be worse than one that plainly holds nothing yet.
    #[test]
    fn an_audio_track_makes_no_sequencer() {
        let mut a = Arrangement::default();
        let i = a.add_track(TrackKind::Audio);
        // Even with a device somehow set on it, the kind decides.
        a.tracks[i].device = Some(DeviceKind::SineSynth);
        let (_, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);
        assert!(nodes.seqs[i].is_none(), "no instrument on an audio track");
        assert!(!TrackKind::Audio.takes_instrument());
        assert!(TrackKind::Midi.takes_instrument());
    }

    /// Mute, solo and kind are graph SHAPE — they add or drop nodes, so
    /// they must force an immediate swap. Pan is NOT: every instrument
    /// track always carries a Pan node, so pan rides a letter and a knob
    /// drag costs nothing.
    #[test]
    fn shape_hash_covers_mute_and_solo_but_not_pan() {
        let mut a = Arrangement::default();
        let base = shape_hash(&a.tracks);

        a.tracks[0].pan = -0.8;
        assert_eq!(
            shape_hash(&a.tracks),
            base,
            "pan is a letter, never a recompile"
        );

        a.tracks[0].mute = true;
        let muted = shape_hash(&a.tracks);
        assert_ne!(muted, base, "mute drops a node");

        a.tracks[0].mute = false;
        a.tracks[0].solo = true;
        assert_ne!(shape_hash(&a.tracks), base, "solo drops every other node");

        a.tracks[0].solo = false;
        assert_eq!(shape_hash(&a.tracks), base, "and back again");

        let mut b = Arrangement::default();
        b.add_track(TrackKind::Audio);
        let mut c = Arrangement::default();
        c.add_track(TrackKind::Midi);
        assert_ne!(
            shape_hash(&b.tracks),
            shape_hash(&c.tracks),
            "kind decides whether a sequencer exists, so it is shape"
        );
    }

    /// The track verbs all act on the SELECTED track, so the palette, the
    /// keyboard and the header buttons can never disagree about which lane
    /// they mean.
    #[test]
    fn the_track_verbs_act_on_the_selected_track() {
        let mut t = Transport::default();
        let mut a = Arrangement {
            selected: Some(2),
            ..Default::default()
        };

        perform(&[UiAction::ToggleTrackMute], &mut t, &mut a);
        assert!(a.tracks[2].mute);
        assert!(!a.tracks[0].mute, "only the selected lane");
        perform(&[UiAction::ToggleTrackMute], &mut t, &mut a);
        assert!(!a.tracks[2].mute, "the same verb turns it back off");

        perform(&[UiAction::ToggleTrackSolo], &mut t, &mut a);
        assert!(a.tracks[2].solo);

        perform(&[UiAction::NudgeTrackPan(-PAN_STEP)], &mut t, &mut a);
        assert!(
            (a.tracks[2].pan + PAN_STEP).abs() < 1e-6,
            "left is negative"
        );
        perform(&[UiAction::CenterTrackPan], &mut t, &mut a);
        assert_eq!(a.tracks[2].pan, 0.0, "center is EXACTLY zero");

        // Pan clamps rather than running off the end.
        for _ in 0..40 {
            perform(&[UiAction::NudgeTrackPan(PAN_STEP)], &mut t, &mut a);
        }
        assert_eq!(a.tracks[2].pan, 1.0);
    }

    /// `AddTrack` through the action vocabulary builds the same thing
    /// `add_track` does, and the two gestures pick different kinds.
    #[test]
    fn the_add_track_action_picks_the_kind() {
        let mut t = Transport::default();
        let mut a = Arrangement::default();
        perform(&[UiAction::AddTrack(TrackKind::Audio)], &mut t, &mut a);
        assert_eq!(a.tracks.last().map(|t| t.kind), Some(TrackKind::Audio));
        perform(&[UiAction::AddTrack(TrackKind::Midi)], &mut t, &mut a);
        assert_eq!(a.tracks.last().map(|t| t.kind), Some(TrackKind::Midi));
        assert_eq!(a.tracks.len(), TRACK_COUNT + 2);
        assert_eq!(a.selected, Some(TRACK_COUNT + 1), "the newest is selected");
    }

    /// Pan reads out in words the ear agrees with: C in the middle, L and
    /// R either side, as a percentage.
    #[test]
    fn pan_reads_out_as_left_center_right() {
        assert_eq!(pan_label(0.0), "C");
        assert_eq!(pan_label(-1.0), "L100");
        assert_eq!(pan_label(1.0), "R100");
        assert_eq!(pan_label(-0.42), "L42");
        assert_eq!(pan_label(0.42), "R42");
        // Rounding must not produce "L0" — that is C, said badly.
        assert_eq!(pan_label(-0.001), "C");
    }

    /// A header rename commits on Enter, restores on Escape, and refuses
    /// to leave a track nameless.
    #[test]
    fn renaming_a_track_commits_or_restores() {
        let commit = |text: &str, cancel: bool| {
            let mut a = Arrangement::default();
            let original = a.tracks[0].name.clone();
            a.track_rename = Some(TrackRename {
                track: 0,
                text: text.to_owned(),
                original: original.clone(),
                focused: true,
            });
            let ctx = egui::Context::default();
            let key = if cancel {
                egui::Key::Escape
            } else {
                egui::Key::Enter
            };
            let mut out = ctx.run_ui(
                input(vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }]),
                |ui| {
                    track_rename_keys(ui.ctx(), &mut a);
                },
            );
            out.textures_delta.clear();
            (a.tracks[0].name.clone(), a.track_rename.is_none(), original)
        };

        let (name, closed, _) = commit("Drums", false);
        assert_eq!(name, "Drums", "Enter commits");
        assert!(closed, "and closes the editor");

        let (name, closed, original) = commit("Drums", true);
        assert_eq!(name, original, "Escape restores");
        assert!(closed);

        // Whitespace is trimmed, and an empty name is not a name.
        assert_eq!(commit("  Bass  ", false).0, "Bass");
        let (name, _, original) = commit("   ", false);
        assert_eq!(
            name, original,
            "a track always has something to call itself"
        );
    }

    /// An effect sits between its track's instrument and the mixer — not
    /// on the master, which is what a reverb wired to the sum would be.
    #[test]
    fn an_effect_is_wired_into_its_own_track() {
        let mut a = Arrangement::default();
        a.tracks[0].device = Some(DeviceKind::SineSynth);
        a.tracks[1].device = Some(DeviceKind::SineSynth);
        a.tracks[1].fx = Some(DeviceKind::Reverb);

        let (spec, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);
        let out = spec.output().unwrap();

        // Only the track that loaded one has an effect node.
        assert!(nodes.fx[0].is_none(), "track 0 loaded no effect");
        let rev = nodes.fx[1].expect("track 1 loaded a reverb");
        assert!(nodes.fx[2].is_none() && nodes.fx[3].is_none());

        // Track 0: instrument -> pan -> mixer, no effect in between.
        let seq0 = nodes.seqs[0].unwrap();
        let pan0 = nodes.pans[0].unwrap();
        assert!(
            spec.wires().iter().any(|(f, t)| *f == seq0 && *t == pan0),
            "a track with no effect feeds its pan directly"
        );
        assert!(spec.wires().iter().any(|(f, t)| *f == pan0 && *t == out));
        // Track 1: instrument -> reverb -> pan -> mixer, and NOT
        // instrument -> pan, or the dry signal would bypass the effect.
        let seq1 = nodes.seqs[1].unwrap();
        let pan1 = nodes.pans[1].unwrap();
        assert!(spec.wires().iter().any(|(f, t)| *f == seq1 && *t == rev));
        assert!(spec.wires().iter().any(|(f, t)| *f == rev && *t == pan1));
        assert!(spec.wires().iter().any(|(f, t)| *f == pan1 && *t == out));
        assert!(
            !spec.wires().iter().any(|(f, t)| *f == seq1 && *t == out),
            "the instrument must not also bypass its own effect"
        );
        assert!(spec.compile(48_000, 256).is_ok());
    }

    /// Loading a device fills its own SLOT: an effect never displaces the
    /// instrument that feeds it, and the graph key notices both.
    #[test]
    fn instrument_and_effect_slots_are_independent() {
        let mut a = Arrangement::default();
        a.tracks[0].device = Some(DeviceKind::SineSynth);
        a.tracks[0].fx = Some(DeviceKind::Reverb);
        assert!(DeviceKind::SineSynth.is_instrument());
        assert!(!DeviceKind::Reverb.is_instrument());

        // Both slots survive together.
        assert_eq!(a.tracks[0].device, Some(DeviceKind::SineSynth));
        assert_eq!(a.tracks[0].fx, Some(DeviceKind::Reverb));

        // The shape hash distinguishes "instrument only" from "both", so
        // loading an effect forces a schedule swap rather than being
        // mistaken for no change at all.
        let both = shape_hash(&a.tracks);
        a.tracks[0].fx = None;
        let instrument_only = shape_hash(&a.tracks);
        assert_ne!(both, instrument_only, "an effect changes the graph's shape");
    }

    /// Ctrl+1 and Ctrl+2 move BOTH grids. They are the shared grid keys —
    /// a session where the arrangement and the roll disagree about what a
    /// grid step means is a session where snapping surprises you.
    #[test]
    fn the_grid_keys_move_both_grids_together() {
        // Both start on the default rung.
        let mut arrangement = GRID_DEFAULT;
        let mut roll = GRID_DEFAULT;
        for _ in 0..2 {
            arrangement = step_grid(arrangement, true);
            roll = step_grid(roll, true);
        }
        assert_eq!(arrangement, roll, "two narrows, still in step");

        // And a roll deliberately set finer STAYS finer: each grid steps
        // independently in the same direction rather than being flattened
        // onto one value.
        let mut arrangement = GRID_DEFAULT;
        let mut roll = GRID_DEFAULT + 2;
        let before = roll - arrangement;
        arrangement = step_grid(arrangement, false);
        roll = step_grid(roll, false);
        assert_eq!(
            roll - arrangement,
            before,
            "the offset the user chose survives"
        );

        // The ladder has ends, and stepping past them is a no-op, not a
        // panic or a wrap into the other extreme.
        let mut g = 0;
        for _ in 0..10 {
            g = step_grid(g, false);
        }
        assert_eq!(g, 0);
        let mut g = GRID_BEATS.len() - 1;
        for _ in 0..10 {
            g = step_grid(g, true);
        }
        assert_eq!(g, GRID_BEATS.len() - 1);
    }

    /// A knob turn must reach the Seq of the track whose card drew it — the
    /// ids are position-dependent, and getting this wrong retunes somebody
    /// else's instrument.
    #[test]
    fn params_route_to_their_own_track() {
        let mut a = Arrangement::default();
        // Give each track a different gain, then check the compiled specs
        // carry them per track rather than sharing one.
        for (i, track) in a.tracks.iter_mut().enumerate() {
            track.params.gain = 0.1 * (i + 1) as f32;
            track.device = Some(DeviceKind::SineSynth);
        }
        let (spec, nodes) = build_graph_spec(&a.tracks, &a.clips, None, false);

        // The ids are distinct and ordered by track, which is what makes
        // `seq_ids[track]` the right address.
        assert_eq!(nodes.seqs.len(), TRACK_COUNT);
        for (i, id) in nodes.seqs.iter().enumerate() {
            for (j, other) in nodes.seqs.iter().enumerate() {
                assert!(i == j || id != other, "two tracks share a Seq id");
            }
        }

        // Each Seq carries its own track's params.
        let gains: Vec<f32> = nodes
            .seqs
            .iter()
            .map(|id| {
                spec.iter_ordered()
                    .find(|(nid, _)| Some(*nid) == *id)
                    .map(|(_, node)| match node {
                        NodeSpec::Seq { params, .. } => params.gain,
                        _ => f32::NAN,
                    })
                    .unwrap_or(f32::NAN)
            })
            .collect();
        assert_eq!(gains, vec![0.1, 0.2, 0.3, 0.4]);
    }

    /// The bar's notice slot truncates by characters, not bytes, and leaves
    /// short strings alone.
    #[test]
    fn notices_ellipsize_cleanly() {
        assert_eq!(ellipsize("engine off", 16), "engine off");
        assert_eq!(ellipsize("stream dead — no blocks", 12), "stream dead…");
        assert_eq!(ellipsize("", 4), "");
        // Exactly at the limit is untouched.
        assert_eq!(ellipsize("abcd", 4), "abcd");
    }
}
