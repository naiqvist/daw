//! The sequencer's chrome, in the console's terms.
//!
//! The grid, the roll and the trig inspector were drawn with the circuit
//! kit — traces that bend at forty-five degrees, octagons, pads. On the
//! console that reads as a different machine. These are the same
//! painters, signature for signature, saying the same things in the
//! console's language: a trace is a straight hairline, a pad is a small
//! square, an octagon is a keyed chamfer from the corpus, corner pads are
//! brackets. Every painter is `fn(out, …, ink)`: it pushes shapes and
//! reads no state, so nothing about WHEN a figure is drawn changed.

use crate::design::kit::Weight;
use corpus_rect::Rect as KRect;
use eframe::egui::{Color32, Pos2, Rect, Shape, Stroke, pos2};

/// A pad's side.
pub const PAD: f32 = 5.0;
/// The corner a panel gives up.
const CUT: f64 = 9.0;

fn snap(p: Pos2) -> Pos2 {
    pos2(p.x.round() + 0.5, p.y.round() + 0.5)
}

fn bounds(rect: Rect) -> KRect {
    KRect::new(
        rect.min.x as f64 + 0.5,
        rect.min.y as f64 + 0.5,
        rect.width() as f64 - 1.0,
        rect.height() as f64 - 1.0,
    )
}

fn keyed(rect: Rect) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let cut = CUT
        .min(rect.width() as f64 / 3.0)
        .min(rect.height() as f64 / 3.0);
    chamfer::polygon(&bounds(rect), chamfer::Corners::diagonal(cut), &mut pts);
    pts.into_iter()
        .map(|(x, y)| pos2(x as f32, y as f32))
        .collect()
}

/// A straight hairline along `path`.
pub fn trace(out: &mut Vec<Shape>, path: &[Pos2], weight: Weight, ink: Color32) {
    if path.len() < 2 {
        return;
    }
    out.push(Shape::line(
        path.iter().copied().map(snap).collect(),
        Stroke::new(weight.px(), ink),
    ));
}

/// A small square.
pub fn pad(out: &mut Vec<Shape>, centre: Pos2, side: f32, ink: Color32, filled: bool) {
    let rect = Rect::from_center_size(centre, eframe::egui::Vec2::splat(side));
    if filled {
        out.push(Shape::rect_filled(rect, 0.0, ink));
    } else {
        out.push(Shape::rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, ink),
            eframe::egui::StrokeKind::Inside,
        ));
    }
}

/// A dot where lines meet.
pub fn via(out: &mut Vec<Shape>, centre: Pos2, ink: Color32, _ground: Color32) {
    pad(out, centre, 3.0, ink, true);
}

/// A keyed chamfered panel: filled, stroked, or both.
pub fn octagon(
    out: &mut Vec<Shape>,
    rect: Rect,
    _chamfer: f32,
    fill: Option<Color32>,
    stroke: Option<(Weight, Color32)>,
) {
    let pts = keyed(rect);
    if let Some(fill) = fill {
        out.push(Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
    }
    if let Some((weight, ink)) = stroke {
        out.push(Shape::closed_line(pts, Stroke::new(weight.px(), ink)));
    }
}

/// A panel; the variant is the circuit kit's notion and is ignored here.
pub fn panel_variant(
    out: &mut Vec<Shape>,
    rect: Rect,
    fill: Option<Color32>,
    _ground: Color32,
    stroke: Option<(Weight, Color32)>,
    _variant: u8,
) {
    octagon(out, rect, 0.0, fill, stroke);
}

/// A panel's outline alone.
pub fn panel_frame(out: &mut Vec<Shape>, rect: Rect, weight: Weight, ink: Color32) {
    octagon(out, rect, 0.0, None, Some((weight, ink)));
}

/// A panel's outline alone; the variant is ignored here.
pub fn panel_frame_variant(
    out: &mut Vec<Shape>,
    rect: Rect,
    weight: Weight,
    ink: Color32,
    _variant: u8,
) {
    octagon(out, rect, 0.0, None, Some((weight, ink)));
}

/// Corner marks: four brackets, the console's way of saying "this area".
pub fn corner_pads(out: &mut Vec<Shape>, rect: Rect, ink: Color32) {
    let mut arms = Vec::new();
    bracket::corners(&bounds(rect), bracket::Arms::all(6.0), &mut arms);
    for arm in arms {
        out.push(Shape::line(
            arm.iter()
                .map(|(x, y)| pos2(*x as f32, *y as f32))
                .collect(),
            Stroke::new(1.0, ink),
        ));
    }
}

/// A leader from a pad to the thing it annotates.
pub fn annotation_arrow(out: &mut Vec<Shape>, from_pad: Pos2, to: Pos2, ink: Color32) {
    trace(out, &[from_pad, to], Weight::Hair, ink);
    pad(out, to, 3.0, ink, true);
}
