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
const CUT: f64 = 9.0;
/// Bracket arm length on the focused chassis.
const ARM: f64 = 12.0;
/// How far inside the outline the brackets sit.
const INSET: f64 = 4.0;
/// A dash on an unfocused outline, and the gap after it.
const DASH: f64 = 6.0;
const GAP: f64 = 4.0;

fn style() -> panel::Style {
    panel::Style {
        cut: CUT,
        arm: ARM,
        inset: INSET,
        dash: dash::Pattern::new(DASH, GAP),
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
