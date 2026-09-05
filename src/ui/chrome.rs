//! The console's chrome: the circuit kit's painters, in the console's terms.
//!
//! The grid, the roll, the trig inspector, the band and its faces were
//! drawn with the circuit kit — traces that bend at forty-five degrees, octagons, pads. On the
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
/// A via's diameter.
pub const VIA: f32 = 3.0;
/// The corner a panel gives up.
pub const CHAMFER: f32 = 9.0;
const CUT: f64 = 9.0;
/// The console casts no offset shadow: depth is a step in value.
pub const SHADOW_X: f32 = 0.0;
pub const SHADOW_Y: f32 = 0.0;
/// One dash, and the gap after it.
pub const DASH: f32 = 6.0;

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

/// The keyed chamfer's outline, closed, for a caller that wants to dash
/// or fill it its own way.
pub fn keyed_outline(rect: Rect) -> Vec<Pos2> {
    let mut pts = keyed(rect);
    if let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    pts
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

/// A straight run: the console bends nothing at forty-five degrees.
pub fn elbow(a: Pos2, b: Pos2) -> Vec<Pos2> {
    vec![a, b]
}

/// Dashes along `path`. The phase is ignored: nothing here marches.
pub fn dashes(out: &mut Vec<Shape>, path: &[Pos2], _phase: f32, weight: Weight, ink: Color32) {
    if path.len() < 2 {
        return;
    }
    let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    let mut segs = Vec::new();
    dash::dashes(
        &pts,
        &dash::Pattern::new(DASH as f64, (DASH * 0.6) as f64),
        &mut segs,
    );
    for (a, b) in segs {
        out.push(Shape::line_segment(
            [
                snap(pos2(a.0 as f32, a.1 as f32)),
                snap(pos2(b.0 as f32, b.1 as f32)),
            ],
            Stroke::new(weight.px(), ink),
        ));
    }
}

/// A faint dot lattice over an area: the ground's own material.
pub fn lattice(out: &mut Vec<Shape>, area: Rect, pitch: f32, ink: Color32) {
    if pitch < 2.0 {
        return;
    }
    let mut y = area.min.y + pitch * 0.5;
    while y < area.max.y {
        let mut x = area.min.x + pitch * 0.5;
        while x < area.max.x {
            out.push(Shape::rect_filled(
                Rect::from_center_size(pos2(x.round(), y.round()), eframe::egui::Vec2::splat(1.0)),
                0.0,
                ink,
            ));
            x += pitch;
        }
        y += pitch;
    }
}

/// One dot.
pub fn dot(out: &mut Vec<Shape>, centre: Pos2, ink: Color32) {
    pad(out, centre, 2.0, ink, true);
}

/// A line with a pad at each fraction along it.
pub fn rail(out: &mut Vec<Shape>, from: Pos2, to: Pos2, pads: &[f32], ink: Color32) {
    trace(out, &[from, to], Weight::Hair, ink);
    for t in pads {
        let p = from + (to - from) * t.clamp(0.0, 1.0);
        pad(out, p, PAD - 1.0, ink, true);
    }
}

/// Where lines meet: a dot, the ground cut around it.
pub fn junction(out: &mut Vec<Shape>, centre: Pos2, ink: Color32, ground: Color32) {
    pad(out, centre, VIA + 2.0, ground, true);
    pad(out, centre, VIA, ink, true);
}

/// A bar of segments, the first `lit` share of them lit.
#[allow(clippy::too_many_arguments)]
pub fn tick_bar(
    out: &mut Vec<Shape>,
    rect: Rect,
    segments: usize,
    lit: f32,
    lit_ink: Color32,
    rest_ink: Color32,
    horizontal: bool,
) {
    let n = segments.max(1);
    let lit = (lit.clamp(0.0, 1.0) * n as f32).round() as usize;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let seg = if horizontal {
            Rect::from_min_max(
                pos2(rect.min.x + rect.width() * a, rect.min.y),
                pos2(rect.min.x + rect.width() * b - 1.0, rect.max.y),
            )
        } else {
            Rect::from_min_max(
                pos2(rect.min.x, rect.max.y - rect.height() * b + 1.0),
                pos2(rect.max.x, rect.max.y - rect.height() * a),
            )
        };
        out.push(Shape::rect_filled(
            seg,
            0.0,
            if i < lit { lit_ink } else { rest_ink },
        ));
    }
}

/// A row of choices, the chosen one lit.
pub fn choice_bar(
    out: &mut Vec<Shape>,
    rect: Rect,
    count: usize,
    chosen: usize,
    lit_ink: Color32,
    rest_ink: Color32,
) {
    let n = count.max(1);
    for i in 0..n {
        let seg = Rect::from_min_max(
            pos2(rect.min.x + rect.width() * i as f32 / n as f32, rect.min.y),
            pos2(
                rect.min.x + rect.width() * (i + 1) as f32 / n as f32 - 1.0,
                rect.max.y,
            ),
        );
        out.push(Shape::rect_filled(
            seg,
            0.0,
            if i == chosen { lit_ink } else { rest_ink },
        ));
    }
}

/// Decorative bits. The console prints no number it did not measure,
/// so this draws nothing.
pub fn binary(
    _out: &mut Vec<Shape>,
    _origin: Pos2,
    _unit: f32,
    _bits: u32,
    _count: usize,
    _ink: Color32,
) {
}

/// The console casts no shadow: what lies under a plane is the ground.
pub fn shadow_ink(ground: Color32) -> Color32 {
    ground
}

/// Corner brackets with a cap length: the bracket kernel, in ink.
pub fn brackets(out: &mut Vec<Shape>, rect: Rect, cap: f32, weight: Weight, ink: Color32) {
    let mut arms = Vec::new();
    bracket::corners(&bounds(rect), bracket::Arms::all(cap as f64), &mut arms);
    for arm in arms {
        out.push(Shape::line(
            arm.iter()
                .map(|(x, y)| pos2(*x as f32, *y as f32))
                .collect(),
            Stroke::new(weight.px(), ink),
        ));
    }
}
