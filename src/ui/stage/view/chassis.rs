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

/// A recessed instrument rail. Title and status use the same material as
/// the rest of the console, but their two hairlines make the boundary
/// explicit instead of relying on a decorative change of colour.
pub fn instrument_rail(painter: &egui::Painter, rect: egui::Rect) {
    if rect.is_negative() || rect.height() < 1.0 {
        return;
    }
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.panel);
    let stroke = egui::Stroke::new(1.0, c.rule);
    let top = rect.min.y.round() - 0.5;
    let bottom = rect.max.y.round() - 0.5;
    painter.line_segment(
        [egui::pos2(rect.min.x, top), egui::pos2(rect.max.x, top)],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(rect.min.x, bottom),
            egui::pos2(rect.max.x, bottom),
        ],
        stroke,
    );
}

/// A splice between two real groups of readings on an instrument rail.
/// The centre terminal is structural: one per semantic boundary supplied by
/// the caller, never repeated as an ornamental grid.
pub fn rail_splice(painter: &egui::Painter, rect: egui::Rect, x: f32) {
    if x <= rect.min.x || x >= rect.max.x || rect.height() < 8.0 {
        return;
    }
    let c = palette::colours();
    let x = x.round() - 0.5;
    painter.line_segment(
        [
            egui::pos2(x, rect.min.y + 4.0),
            egui::pos2(x, rect.max.y - 4.0),
        ],
        egui::Stroke::new(1.0, c.edge),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(2.0, 2.0)),
        0.0,
        c.chassis,
    );
}

/// Where a component stands in its row, which decides which corners it
/// gives up: the ends are keyed outward, the middle on the diagonal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    /// The leftmost: its two left corners cut.
    Left,
    /// In the run: square. Only the ends of a row are keyed.
    Centre,
    /// The rightmost: its two right corners cut.
    Right,
    /// Every corner cut: a badge, not a component. Kept for the odd
    /// plaque; a lone head is still the leftmost.
    Both,
}

/// A chassis keyed by where it stands: a FILLED plate, outlined only
/// when it has the keys. The cursor itself is the overlay's
/// (`ui::nav_cursor`), so no brackets are drawn here.
///
/// S07 of the soft PC-98 earth spec. This used to be an outline and
/// nothing else — dashed at rest, solid when focused — so a plate was a
/// shape you inferred from four hairlines over the field's own ground.
/// The mockups separate a plate from the field by MATERIAL instead: the
/// fill is the edge, and a rule survives only where it carries meaning.
/// A dashed rectangle around every head was decoration standing in for a
/// surface.
pub fn keyed(painter: &egui::Painter, rect: egui::Rect, focused: bool, key: Key) {
    let c = palette::colours();
    let cut = CUT
        .min(rect.width() as f64 / 3.0)
        .min(rect.height() as f64 / 3.0);
    let corners = match key {
        Key::Left => chamfer::Corners::left(cut),
        Key::Centre => chamfer::Corners::default(),
        Key::Right => chamfer::Corners::right(cut),
        Key::Both => chamfer::Corners::all(cut),
    };
    let bounds = bounds_of(rect);
    let mut outline = Vec::new();
    chamfer::polygon(&bounds, corners, &mut outline);
    if outline.is_empty() {
        return;
    }
    let points: Vec<egui::Pos2> = outline.iter().copied().map(to_pos).collect();
    // The plate itself. A keyed chamfer is convex, so one polygon does it.
    painter.add(egui::Shape::convex_polygon(
        points.clone(),
        c.panel,
        egui::Stroke::NONE,
    ));
    // Focus is still carried by the edge, because that survives any
    // theme; it is now an edge on a surface rather than an edge instead
    // of one. At rest the plate wears no outline at all.
    if focused {
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.0, c.chassis),
        ));
    }
}

/// Registration marks: four short brackets in rule at a region's corners,
/// the way an overlay is registered to the screen it is laid on.
pub fn marks(painter: &egui::Painter, rect: egui::Rect, arm: f64) {
    let c = palette::colours();
    let mut arms = Vec::new();
    bracket::corners(&bounds_of(rect), bracket::Arms::all(arm), &mut arms);
    for arm in &arms {
        painter.add(egui::Shape::line(
            arm.iter().copied().map(to_pos).collect(),
            egui::Stroke::new(1.0, c.rule),
        ));
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
