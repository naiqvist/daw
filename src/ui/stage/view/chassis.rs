//! The chassis a thing wears: a keyed chamfered outline, corner brackets
//! when it has the keys, a dashed edge when it does not.
//!
//! The cockpit's frame, exactly — `alloys/geometry/panel` does the
//! geometry, and what is left here is what is genuinely this app's: which
//! colours, how thick, and the half-pixel offset that lands a one-pixel
//! stroke on a pixel instead of straddling two and coming out grey.
//!
//! Focus is carried by the edge, not by the ink: solid and bracketed
//! against dashed is legible at a glance and survives any theme.

use super::palette;
use corpus_panel as panel;
use corpus_rect::Rect as KRect;
use eframe::egui;

/// How much is taken off the two keyed corners.
/// @tune 0..40 px
const CUT: f64 = 9.0;
/// Bracket arm length on the focused chassis.
/// @tune 4..60 px
const ARM: f64 = 12.0;
/// How far inside the outline the brackets sit.
/// @tune 0..24 px
const INSET: f64 = 4.0;
/// A dash on an unfocused outline.
/// @tune 2..30 px
const DASH: f64 = 6.0;
/// The gap after that dash.
/// @tune 2..30 px
const GAP: f64 = 4.0;

fn style() -> panel::Style {
    panel::Style {
        cut: crate::tune!(CUT),
        arm: crate::tune!(ARM),
        inset: crate::tune!(INSET),
        dash: dash::Pattern::new(crate::tune!(DASH), crate::tune!(GAP)),
    }
}

fn bounds_of(rect: egui::Rect) -> KRect {
    // Half a pixel in, so a one-pixel stroke lands on the pixel rather
    // than straddling two and coming out grey.
    KRect::new(
        rect.min.x as f64 + 0.5,
        rect.min.y as f64 + 0.5,
        rect.width() as f64 - 1.0,
        rect.height() as f64 - 1.0,
    )
}

fn to_pos(p: (f64, f64)) -> egui::Pos2 {
    egui::pos2(p.0 as f32, p.1 as f32)
}

/// Where a component stands in its row, which decides which corners it
/// gives up: the ends are keyed outward, the middle on the diagonal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    /// The leftmost: its two left corners cut.
    Left,
    /// In the run: top-left and bottom-right, the console's own key.
    Centre,
    /// The rightmost: its two right corners cut.
    Right,
    /// Every corner cut: a badge, not a component. Kept for the odd
    /// plaque; a lone head is still the leftmost.
    Both,
}

/// A chassis keyed by where it stands. Solid with brackets on the square
/// corners when focused, dashed when not — the same reading as `frame`,
/// with the corners chosen by position rather than fixed.
pub fn keyed(painter: &egui::Painter, rect: egui::Rect, focused: bool, key: Key) {
    let c = palette::colours();
    let cut = CUT
        .min(rect.width() as f64 / 3.0)
        .min(rect.height() as f64 / 3.0);
    let corners = match key {
        Key::Left => chamfer::Corners::left(cut),
        Key::Centre => chamfer::Corners::diagonal(cut),
        Key::Right => chamfer::Corners::right(cut),
        Key::Both => chamfer::Corners::all(cut),
    };
    let bounds = bounds_of(rect);
    let mut outline = Vec::new();
    chamfer::polygon(&bounds, corners, &mut outline);
    if outline.is_empty() {
        return;
    }
    if focused {
        painter.add(egui::Shape::closed_line(
            outline.iter().copied().map(to_pos).collect(),
            egui::Stroke::new(1.0, c.chassis),
        ));
        // Brackets on the corners that are still square; a bracket over
        // a cut corner is a muddle.
        let arm = |corner: f64| if corner == 0.0 { ARM } else { 0.0 };
        let arms = bracket::Arms {
            top_left: arm(corners.top_left),
            top_right: arm(corners.top_right),
            bottom_right: arm(corners.bottom_right),
            bottom_left: arm(corners.bottom_left),
        };
        let mut brackets = Vec::new();
        bracket::corners(&bounds.inset(INSET), arms, &mut brackets);
        for arm in &brackets {
            painter.add(egui::Shape::line(
                arm.iter().copied().map(to_pos).collect(),
                egui::Stroke::new(1.5, c.alert),
            ));
        }
    } else {
        let mut closed = outline.clone();
        closed.push(outline[0]);
        let mut dashes = Vec::new();
        dash::dashes(&closed, &dash::Pattern::new(DASH, GAP), &mut dashes);
        let hairline = egui::Stroke::new(1.0, c.edge);
        for (a, b) in &dashes {
            painter.line_segment([to_pos(*a), to_pos(*b)], hairline);
        }
    }
}

/// Four brackets and no outline: the cursor at cell scale.
pub fn brackets(painter: &egui::Painter, rect: egui::Rect, arm: f64) {
    let c = palette::colours();
    let mut arms = Vec::new();
    bracket::corners(&bounds_of(rect), bracket::Arms::all(arm), &mut arms);
    for arm in &arms {
        painter.add(egui::Shape::line(
            arm.iter().copied().map(to_pos).collect(),
            egui::Stroke::new(1.5, c.alert),
        ));
    }
}

/// Draw a chassis around `rect`.
pub fn frame(painter: &egui::Painter, rect: egui::Rect, focused: bool) {
    let c = palette::colours();
    let mut frame = panel::Frame::default();
    panel::frame(&bounds_of(rect), &style(), focused, &mut frame);

    if !frame.outline.is_empty() {
        painter.add(egui::Shape::closed_line(
            frame.outline.iter().copied().map(to_pos).collect(),
            egui::Stroke::new(1.0, c.chassis),
        ));
    }
    for arm in &frame.brackets {
        painter.add(egui::Shape::line(
            arm.iter().copied().map(to_pos).collect(),
            egui::Stroke::new(1.5, c.alert),
        ));
    }
    let hairline = egui::Stroke::new(1.0, c.edge);
    for (a, b) in &frame.dashes {
        painter.line_segment([to_pos(*a), to_pos(*b)], hairline);
    }
}
