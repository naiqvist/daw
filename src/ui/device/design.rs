//! The device design system: the ONLY place a device-layer frame or
//! margin is chosen. Mechanically enforced — `ui::tests` fails the build
//! if any other device file writes `Frame::new`, `inner_margin`, or
//! `Margin::` — so a card CANNOT ship with ad-hoc spacing: every surface
//! it is built from arrives here, pre-margined, on the 4px grid.
//!
//! The spacing rhythm, stated once:
//!
//! - **Card**: no inner margin of its own — its outline hugs the silhouette.
//! - **Title strip**: SM horizontal / XS vertical — wide enough to breathe,
//!   shallow enough to read as a label, not a toolbar.
//! - **Body**: no inset — the wells ARE the device face and tile every point
//!   below the title rule. An outer margin here only frames a frame.
//! - **Well**: XS all around — the one intentional breathing space between
//!   a section's edge and the control standing in it.
//!
//! Every value is a density-scaled token; nothing here invents a number.

use crate::ui::theme::Theme;
use crate::ui::tokens::{radius, space, stroke};
use eframe::egui;

/// The card silhouette: raised surface, hairline outline, SQUARE corners.
/// Marginless by design — compose [`title_strip`] and [`body`] inside it.
///
/// No radius, deliberately. A device chain is a strip of cards butted
/// together, and rounded corners put four little notches of background at
/// every join — the rack reads as a row of separate boxes instead of one
/// piece of equipment. Square corners also let the title rule run edge to
/// edge, which is what makes the strip read as a face rather than a label
/// floating on a panel.
pub fn card_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface_raised)
        .stroke(egui::Stroke::new(stroke::HAIR, theme.outline))
        .corner_radius(0)
}

/// The title strip's padding: SM across, XS down.
pub fn title_strip(theme: &Theme) -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(
        theme.sp(space::SM) as i8,
        theme.sp(space::XS) as i8,
    ))
}

/// The card body is deliberately marginless.
///
/// Wells already provide their own internal padding. Insetting the body as
/// well created an empty rail around every device and paid for the same
/// breathing room twice. The title is the card's only chrome; below its rule,
/// the content owns the whole face.
pub fn body(_theme: &Theme) -> egui::Frame {
    egui::Frame::new()
}

/// A section well: recessed, hairline-divided, control radius, XS padding.
///
/// Filled with `surface` rather than `surface_sunken` — one step below the
/// card instead of four. A well is a SHELF inside the card, not a hole
/// through it: at full sunken depth the wells were the loudest thing on
/// the card, and the eye read the shelving before it read the controls
/// standing on it. One step is enough to say "inside"; the controls' own
/// faces (a knob's dial, a fader's rail) keep `surface_sunken`, so they
/// still read as recessed against the well they sit in.
pub fn well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(stroke::HAIR, theme.divider))
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(theme.sp(space::XS) as i8))
}

/// The frame a DIVIDED well wears: the tray its divisions sit in.
///
/// Two differences from a plain [`well`], both for the same reason — at
/// well padding a divided well is only a hairline wider than the cells
/// inside it, so the enclosure reads as an uneven gap between siblings
/// rather than as a container, and the grouping it exists to show is
/// invisible.
///
/// - **SM padding, not XS.** A visible band of tray around the divisions
///   is what says "these belong together". Four pixels does not.
/// - **No stroke.** The band already separates the tray from the card;
///   an outline as well puts three borders within eight pixels, which
///   reads as a table rather than as a panel. The fill step carries it.
pub fn group_well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
}

/// The tray's inner padding, for the layout math that must subtract it.
/// Same token [`group_well`] bakes in.
pub fn group_pad(theme: &Theme) -> f32 {
    theme.sp(space::SM)
}

/// A SUB-well: a well inside a well, for splitting one group of related
/// controls into tighter subgroups.
///
/// One more step down the same ground ramp — card `surface_raised`, well
/// `surface`, sub-well `bg` — so nesting reads as depth rather than as a
/// new material. The ladder stops here on purpose: a third step would have
/// to be `surface_sunken`, which is where the CONTROLS live (a knob's dial
/// face, a fader's rail), and a container the same colour as the things
/// standing in it stops being a container.
///
/// Same hairline and radius as a well. What separates the two levels is
/// the fill step and nothing else — adding a heavier edge for depth is how
/// a card ends up looking like a spreadsheet.
pub fn sub_well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.bg)
        .stroke(egui::Stroke::new(stroke::HAIR, theme.divider))
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(theme.sp(space::XS) as i8))
}

/// The well's inner padding, for layout math that must subtract it
/// (sections' row-height split). Same token the [`well`] frame bakes in.
pub fn well_pad(theme: &Theme) -> f32 {
    theme.sp(space::XS)
}

/// The padding inside an editable value box.
///
/// The same figure the resting readout is drawn with, so the number does
/// not jump sideways the instant you click it — a value field that shifts
/// its own text on entering edit mode reads as a different control
/// appearing, rather than the same one opening.
pub fn field_pad(theme: &Theme) -> f32 {
    theme.sp(space::XS)
}

/// That padding as a margin, for the text editor itself. Here rather than
/// in the widget because a margin is spacing, and spacing is decided in
/// exactly one file.
pub fn field_margin(theme: &Theme) -> egui::Margin {
    egui::Margin::symmetric(field_pad(theme) as i8, 0)
}

/// The radius of a control or display box.
///
/// One value, routed through here rather than each widget reaching for a
/// token, because the drift was real: the filter display had square
/// corners while its OWN thumbnail had round ones, and the spectrum and
/// envelope were square while everything else was not. Nothing said which
/// was right, because nothing had said anything at all.
///
/// The ladder it belongs to: the card is square (a rack of cards butts
/// together and notches at every join read as separate boxes), and
/// everything inside it — wells, trays, controls, displays — is rounded
/// at this one radius.
pub fn box_radius() -> f32 {
    radius::CTRL
}

/// The radius of a HARDWARE-STYLE DISPLAY: none.
///
/// The ladder above is right for a rack of software cards, and wrong for
/// the one card that is pretending to be a screen. Elektron's panel is a
/// 128×64 bitmap — it has no radii because it has no pixels to spare for
/// them — and every box inside a TE engine screen is square too. A
/// rounded corner is the single loudest tell that a display is drawn by
/// a compositor rather than lit by an LCD.
///
/// A separate function rather than a parameter on `box_radius`, so the
/// departure has a NAME and a reason attached to it, and so grepping for
/// "who is pretending to be hardware" returns an answer.
pub fn screen_radius() -> f32 {
    0.0
}

/// Draw the focus ring for a rectangular control.
///
/// One ring, drawn one way. Six widgets had grown their own copy of these
/// four lines, agreeing on everything except the radius — and disagreeing
/// about WHEN to draw it, which was worse: some showed it while merely
/// being dragged with the mouse, so the same visual meant "the keyboard
/// is here" on one control and "the pointer is here" on the next.
///
/// The rule is the first one. A mouse drag has the pointer for feedback;
/// the ring is how a control says the arrow keys will now reach it.
///
/// # It is CORNERS, not a box
///
/// A closed rectangle drawn around a control covers the control's own
/// outline, so on a dense surface "focused" and "selected" arrive as the
/// same closed box in two colours — and in greyscale as one box. Corner
/// brackets fix the same rectangle with a fifth of the ink at the four
/// points that define it, which makes focus a different SHAPE rather
/// than a different colour. See `ui::hud`.
///
/// Below `hud::BRACKET_FLOOR` the brackets would be four dots, and the
/// closed ring says it better; that fallback is the one place the two
/// marks are allowed to be the same.
pub fn focus_ring(painter: &egui::Painter, theme: &Theme, rect: egui::Rect) {
    let marked = rect.expand(stroke::FOCUS);
    let ink = egui::Stroke::new(stroke::FOCUS, theme.focus);
    if crate::ui::hud::is_bracketed(marked) {
        crate::ui::hud::brackets(painter, marked, ink);
    } else {
        painter.rect_stroke(marked, box_radius(), ink, egui::StrokeKind::Outside);
    }
}

// ------------------------------------------------------------- compact ---
//
// The mini surfaces. A card holding mini widgets is a card where the
// chrome is competing with its own content: four points of padding and a
// hairline around a twenty-point dial is a frame you notice before the
// knob. These are the same surfaces with the weight taken out.
//
// Two differences, both deliberate:
//
// - **Half-step padding** ([`space::XXS`]), the one place that token is
//   used. A grid exists so spacing is decided once; a mini well is the
//   case that argued for a half-step, and the argument lives in `tokens`.
// - **No stroke.** At this density hairlines everywhere read as a table.
//   The fill step already separates the levels — it is doing the same job
//   the tray does, one size down.

/// A mini well: the compact form of [`well`].
pub fn mini_well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(mini_pad(theme) as i8))
}

/// A mini tray: the compact form of [`group_well`].
pub fn mini_group_well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(mini_group_pad(theme) as i8))
}

/// A mini sub-well: the compact form of [`sub_well`].
pub fn mini_sub_well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.bg)
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(mini_pad(theme) as i8))
}

/// A mini well's inner padding.
pub fn mini_pad(theme: &Theme) -> f32 {
    theme.sp(space::XXS)
}

/// A mini tray's inner padding. Still a step roomier than the wells it
/// holds — a tray whose padding matched its cells would stop reading as a
/// container at all.
pub fn mini_group_pad(theme: &Theme) -> f32 {
    theme.sp(space::XS)
}

/// The gap between mini wells.
pub fn mini_gap(theme: &Theme) -> f32 {
    theme.sp(space::XXS)
}

/// The gap BETWEEN things: between two wells, and between a control and
/// its label or readout.
///
/// One token for both on purpose. A card whose wells are separated by one
/// distance and whose label sits at another reads as two rhythms fighting;
/// the same gap everywhere is what makes a rack look machined. Widgets
/// measure with this (`metrics::labelled_control`) and lay out with it, so
/// a footprint and the thing it describes cannot disagree.
pub fn gap(theme: &Theme) -> f32 {
    theme.sp(space::XS)
}
