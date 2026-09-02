//! The circuit kit: the machine's own drawing.
//!
//! Traces that run orthogonally and bend at forty-five degrees, ending in
//! pads. Octagonal frames — a rectangle that gave up its corners. Vias and
//! junctions where lines meet. Rails with pads along them, tick bars,
//! barcodes, rows of bits. Everything a board is made of, and nothing a
//! hand would draw: the alien layer of the deck, before anyone read it as
//! script.
//!
//! Every painter here is `fn(out, …, ink)`: it pushes shapes and reads no
//! state. Whether a figure should be there at all is the caller's
//! decision, and the alphabet's. Hairlines are snapped to pixel centres so
//! a one-pixel line is one pixel wide.

use eframe::egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::kit::{Rng, Weight, snap, snap_pos};

/// A pad's side.
pub const PAD: f32 = 5.0;
/// A via's diameter.
pub const VIA: f32 = 3.0;
/// The corner a frame gives up. The same size everywhere — it reads as a
/// bevel on a made object, never as a shape of its own.
pub const CHAMFER: f32 = 6.0;
/// One dash, and the gap after it.
pub const DASH: f32 = 8.0;

// ------------------------------------------------------------------ lines

/// A polyline in one weight. The caller supplies the bends; `elbow` makes
/// the forty-five degree ones.
pub fn trace(out: &mut Vec<Shape>, path: &[Pos2], weight: Weight, ink: Color32) {
    if path.len() < 2 {
        return;
    }
    let pts: Vec<Pos2> = path.iter().map(|p| snap_pos(*p)).collect();
    out.push(Shape::line(pts, Stroke::new(weight.px(), ink)));
}

/// A path from `a` to `b`: straight along the longer axis, then a
/// forty-five degree run into `b`. The board's only bend.
pub fn elbow(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    if dx.abs() < 0.5 || dy.abs() < 0.5 {
        return vec![a, b];
    }
    let knee = if dx.abs() >= dy.abs() {
        pos2(b.x - dx.signum() * dy.abs(), a.y)
    } else {
        pos2(a.x, b.y - dy.signum() * dx.abs())
    };
    vec![a, knee, b]
}

/// Travelling dashes along a path. `phase` in 0..1 slides the pattern by
/// one dash-and-gap; the number of dashes never changes with it, only
/// where they sit. Drawn uncached, because it moves.
pub fn dashes(out: &mut Vec<Shape>, path: &[Pos2], phase: f32, weight: Weight, ink: Color32) {
    if path.len() < 2 {
        return;
    }
    let period = DASH * 2.0;
    let mut offset = -(phase.rem_euclid(1.0)) * period;
    let stroke = Stroke::new(weight.px(), ink);
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = b - a;
        let len = seg.length();
        if len < 1e-3 {
            continue;
        }
        let dir = seg / len;
        let mut t = offset;
        while t < len {
            let s = t.max(0.0);
            let e = (t + DASH).min(len);
            if e > s {
                out.push(Shape::line_segment(
                    [snap_pos(a + dir * s), snap_pos(a + dir * e)],
                    stroke,
                ));
            }
            t += period;
        }
        offset = t - len;
    }
}

// ------------------------------------------------------------------ marks

/// A square pad, filled or hollow, on the pixel grid.
pub fn pad(out: &mut Vec<Shape>, centre: Pos2, side: f32, ink: Color32, filled: bool) {
    let half = (side * 0.5).round();
    let c = pos2(centre.x.round(), centre.y.round());
    let rect = Rect::from_min_max(c - vec2(half, half), c + vec2(half, half));
    if filled {
        out.push(Shape::rect_filled(rect, 0.0, ink));
    } else {
        out.push(Shape::closed_line(
            vec![
                pos2(snap(rect.min.x), snap(rect.min.y)),
                pos2(snap(rect.max.x), snap(rect.min.y)),
                pos2(snap(rect.max.x), snap(rect.max.y)),
                pos2(snap(rect.min.x), snap(rect.max.y)),
            ],
            Stroke::new(Weight::Hair.px(), ink),
        ));
    }
}

/// A via: a ring with the ground showing through it.
pub fn via(out: &mut Vec<Shape>, centre: Pos2, ink: Color32, ground: Color32) {
    let c = snap_pos(centre);
    out.push(Shape::circle_filled(c, VIA * 0.5 + 0.5, ground));
    out.push(Shape::circle_stroke(
        c,
        VIA * 0.5 + 0.5,
        Stroke::new(Weight::Hair.px(), ink),
    ));
}

/// A junction: a filled pad with a via cut through it. Where lines cross.
pub fn junction(out: &mut Vec<Shape>, centre: Pos2, ink: Color32, ground: Color32) {
    pad(out, centre, PAD + 2.0, ink, true);
    let c = pos2(centre.x.round(), centre.y.round());
    out.push(Shape::rect_filled(
        Rect::from_center_size(c, vec2(2.0, 2.0)),
        0.0,
        ground,
    ));
}

/// A tiny dot, the lattice's own texture.
pub fn dot(out: &mut Vec<Shape>, centre: Pos2, ink: Color32) {
    out.push(Shape::rect_filled(
        Rect::from_center_size(pos2(centre.x.round(), centre.y.round()), vec2(1.0, 1.0)),
        0.0,
        ink,
    ));
}

// ----------------------------------------------------------------- frames

/// The eight corners of an octagon: `rect` with every corner cut by
/// `chamfer`, clamped so a small rect stays a shape.
pub fn octagon_points(rect: Rect, chamfer: f32) -> Vec<Pos2> {
    let c = chamfer
        .min(rect.width() / 3.0)
        .min(rect.height() / 3.0)
        .max(0.0);
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    vec![
        pos2(l + c, t),
        pos2(r - c, t),
        pos2(r, t + c),
        pos2(r, b - c),
        pos2(r - c, b),
        pos2(l + c, b),
        pos2(l, b - c),
        pos2(l, t + c),
    ]
}

/// An octagon: filled, stroked, or both.
pub fn octagon(
    out: &mut Vec<Shape>,
    rect: Rect,
    chamfer: f32,
    fill: Option<Color32>,
    stroke: Option<(Weight, Color32)>,
) {
    if !rect.is_positive() {
        return;
    }
    if let Some(fill) = fill {
        out.push(Shape::convex_polygon(
            octagon_points(rect, chamfer),
            fill,
            Stroke::NONE,
        ));
    }
    if let Some((weight, ink)) = stroke {
        let inset = weight.px() * 0.5;
        let pts: Vec<Pos2> = octagon_points(rect.shrink(inset), chamfer)
            .into_iter()
            .map(|p| {
                if weight == Weight::Hair {
                    snap_pos(p)
                } else {
                    p
                }
            })
            .collect();
        out.push(Shape::closed_line(pts, Stroke::new(weight.px(), ink)));
    }
}

/// An octagonal outline at the house chamfer.
pub fn frame(out: &mut Vec<Shape>, rect: Rect, weight: Weight, ink: Color32) {
    octagon(out, rect, CHAMFER, None, Some((weight, ink)));
}

/// Two octagons, one inside the other: the master's frame, the archive's.
pub fn double_frame(out: &mut Vec<Shape>, rect: Rect, gap: f32, ink: Color32) {
    frame(out, rect, Weight::Heavy, ink);
    octagon(
        out,
        rect.shrink(gap),
        (CHAMFER - gap * 0.5).max(2.0),
        None,
        Some((Weight::Hair, ink)),
    );
}

/// A pad on each of the four cut corners.
pub fn corner_pads(out: &mut Vec<Shape>, rect: Rect, ink: Color32) {
    let pts = octagon_points(rect, CHAMFER);
    for i in [0usize, 2, 4, 6] {
        let a = pts[(i + 7) % 8];
        let b = pts[i];
        pad(
            out,
            pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5),
            PAD - 1.0,
            ink,
            true,
        );
    }
}

/// Four corner marks and nothing joining them: the cursor's form.
pub fn brackets(out: &mut Vec<Shape>, rect: Rect, cap: f32, weight: Weight, ink: Color32) {
    let cap = cap.min(rect.width() * 0.4).min(rect.height() * 0.4);
    if cap <= 0.5 {
        return;
    }
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    let stroke = Stroke::new(weight.px(), ink);
    let fix = |p: Pos2| {
        if weight == Weight::Hair {
            snap_pos(p)
        } else {
            p
        }
    };
    for pts in [
        [pos2(l, t + cap), pos2(l, t), pos2(l + cap, t)],
        [pos2(r - cap, t), pos2(r, t), pos2(r, t + cap)],
        [pos2(r, b - cap), pos2(r, b), pos2(r - cap, b)],
        [pos2(l + cap, b), pos2(l, b), pos2(l, b - cap)],
    ] {
        out.push(Shape::line(pts.iter().map(|p| fix(*p)).collect(), stroke));
    }
}

// ------------------------------------------------------------------ rails

/// A hairline with filled pads at `pads`, each a position in 0..1 along it.
pub fn rail(out: &mut Vec<Shape>, from: Pos2, to: Pos2, pads: &[f32], ink: Color32) {
    trace(out, &[from, to], Weight::Hair, ink);
    for t in pads {
        let t = t.clamp(0.0, 1.0);
        pad(out, from + (to - from) * t, PAD - 1.0, ink, true);
    }
}

/// A gauge: `segments` ticks across `rect`, the first `lit` fraction in
/// `lit_ink` and the rest in `rest_ink`. Horizontal fills left to right,
/// vertical fills bottom to top.
pub fn tick_bar(
    out: &mut Vec<Shape>,
    rect: Rect,
    segments: usize,
    lit: f32,
    lit_ink: Color32,
    rest_ink: Color32,
    horizontal: bool,
) {
    if segments == 0 || !rect.is_positive() {
        return;
    }
    let n = segments as f32;
    let lit = lit.clamp(0.0, 1.0);
    for i in 0..segments {
        let t0 = i as f32 / n;
        let t1 = (i + 1) as f32 / n;
        let on = (i as f32 + 0.5) / n <= lit;
        let ink = if on { lit_ink } else { rest_ink };
        let cell = if horizontal {
            Rect::from_min_max(
                pos2(rect.min.x + rect.width() * t0, rect.min.y),
                pos2(rect.min.x + rect.width() * t1, rect.max.y),
            )
        } else {
            Rect::from_min_max(
                pos2(rect.min.x, rect.max.y - rect.height() * t1),
                pos2(rect.max.x, rect.max.y - rect.height() * t0),
            )
        };
        // a gap of one pixel between ticks, so a bar reads as a count
        let cell = if horizontal {
            Rect::from_min_max(
                pos2(cell.min.x.round(), cell.min.y),
                pos2(
                    (cell.max.x - 1.0).round().max(cell.min.x.round()),
                    cell.max.y,
                ),
            )
        } else {
            Rect::from_min_max(
                pos2(cell.min.x, (cell.min.y + 1.0).round()),
                pos2(
                    cell.max.x,
                    cell.max.y.round().max((cell.min.y + 1.0).round()),
                ),
            )
        };
        if cell.is_positive() {
            out.push(Shape::rect_filled(cell, 0.0, ink));
        }
    }
}

/// A list parameter's gauge: one segment per position, the chosen one lit.
pub fn choice_bar(
    out: &mut Vec<Shape>,
    rect: Rect,
    count: usize,
    chosen: usize,
    lit_ink: Color32,
    rest_ink: Color32,
) {
    if count == 0 || !rect.is_positive() {
        return;
    }
    let n = count as f32;
    for i in 0..count {
        let t0 = i as f32 / n;
        let t1 = (i + 1) as f32 / n;
        let cell = Rect::from_min_max(
            pos2((rect.min.x + rect.width() * t0).round(), rect.min.y),
            pos2((rect.min.x + rect.width() * t1 - 1.0).round(), rect.max.y),
        );
        if !cell.is_positive() {
            continue;
        }
        if i == chosen.min(count - 1) {
            out.push(Shape::rect_filled(cell, 0.0, lit_ink));
        } else {
            out.push(Shape::rect_stroke(
                cell,
                0.0,
                Stroke::new(Weight::Hair.px(), rest_ink),
                eframe::egui::StrokeKind::Inside,
            ));
        }
    }
}

// ----------------------------------------------------------------- ground

/// The ground's own texture: points on a fixed pitch, anchored to the
/// area so the field does not crawl under a resize.
pub fn lattice(out: &mut Vec<Shape>, area: Rect, pitch: f32, ink: Color32) {
    if pitch <= 1.0 || !area.is_positive() {
        return;
    }
    let mut y = area.min.y + pitch;
    while y < area.max.y {
        let mut x = area.min.x + pitch;
        while x < area.max.x {
            dot(out, pos2(x, y), ink);
            x += pitch;
        }
        y += pitch;
    }
}

/// The board: a hairline down every column and across every row, with a
/// junction at each crossing. Columns and rows are given as coordinates.
pub fn board(
    out: &mut Vec<Shape>,
    area: Rect,
    cols: &[f32],
    rows: &[f32],
    ink: Color32,
    ground: Color32,
) {
    if !area.is_positive() {
        return;
    }
    for x in cols {
        trace(
            out,
            &[pos2(*x, area.min.y), pos2(*x, area.max.y)],
            Weight::Hair,
            ink,
        );
    }
    for y in rows {
        trace(
            out,
            &[pos2(area.min.x, *y), pos2(area.max.x, *y)],
            Weight::Hair,
            ink,
        );
    }
    for x in cols {
        for y in rows {
            junction(out, pos2(*x, *y), ink, ground);
        }
    }
}

// ------------------------------------------------------------------- tags

/// A barcode: a deterministic run of bars across `rect`.
pub fn barcode(out: &mut Vec<Shape>, rect: Rect, rng: &mut Rng, ink: Color32) {
    if !rect.is_positive() {
        return;
    }
    let mut x = rect.min.x.round();
    while x < rect.max.x {
        let bw = rng.int(1, 4) as f32;
        if rng.chance(0.55) {
            let bw = bw.min(rect.max.x - x);
            out.push(Shape::rect_filled(
                Rect::from_min_size(pos2(x, rect.min.y), vec2(bw, rect.height())),
                0.0,
                ink,
            ));
        }
        x += bw + rng.int(1, 3) as f32;
    }
}

/// A row of `count` bits from `bits`, low bit first: a filled pad for one,
/// a hollow pad for zero.
pub fn binary(
    out: &mut Vec<Shape>,
    origin: Pos2,
    unit: f32,
    bits: u32,
    count: usize,
    ink: Color32,
) {
    for i in 0..count.min(32) {
        let on = (bits >> i) & 1 == 1;
        pad(
            out,
            pos2(origin.x + unit * (i as f32 + 0.5), origin.y + unit * 0.5),
            (unit - 2.0).max(1.0),
            ink,
            on,
        );
    }
}

/// An annotation's arrow: a pad at the note, a trace with one bend, and an
/// open head at the thing being pointed at.
pub fn annotation_arrow(out: &mut Vec<Shape>, from_pad: Pos2, to: Pos2, ink: Color32) {
    pad(out, from_pad, PAD - 1.0, ink, true);
    let path = elbow(from_pad, to);
    trace(out, &path, Weight::Hair, ink);
    if path.len() < 2 {
        return;
    }
    let tail = path[path.len() - 2];
    let d = (to - tail).normalized();
    if d.length() < 0.5 {
        return;
    }
    let n = vec2(-d.y, d.x);
    let h = 4.0;
    trace(
        out,
        &[to - d * h + n * h * 0.6, to, to - d * h - n * h * 0.6],
        Weight::Hair,
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::kit::points_of;

    fn r(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(20.0, 30.0), vec2(w, h))
    }

    const INK: Color32 = Color32::WHITE;
    const GROUND: Color32 = Color32::BLACK;

    #[test]
    fn an_elbow_bends_once_at_forty_five_degrees() {
        let path = elbow(pos2(0.0, 0.0), pos2(100.0, 30.0));
        assert_eq!(path.len(), 3);
        let knee = path[1];
        assert_eq!(knee.y, 0.0, "the first leg runs along the longer axis");
        let leg = path[2] - knee;
        assert!(
            (leg.x.abs() - leg.y.abs()).abs() < 1e-3,
            "the second leg is not diagonal"
        );
        assert_eq!(
            elbow(pos2(0.0, 0.0), pos2(50.0, 0.0)).len(),
            2,
            "a straight run has no knee"
        );
    }

    #[test]
    fn dashes_shift_with_phase_but_never_change_count() {
        let path = [pos2(0.0, 10.0), pos2(200.0, 10.0)];
        let count = |phase: f32| {
            let mut out = Vec::new();
            dashes(&mut out, &path, phase, Weight::Hair, INK);
            out.len()
        };
        let base = count(0.0);
        assert!(base >= 10);
        for phase in [0.1, 0.25, 0.5, 0.9, 1.3] {
            let n = count(phase);
            assert!(
                (n as i32 - base as i32).abs() <= 1,
                "phase {phase} drew {n} dashes against {base}"
            );
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        dashes(&mut a, &path, 0.0, Weight::Hair, INK);
        dashes(&mut b, &path, 0.5, Weight::Hair, INK);
        assert_ne!(
            format!("{a:?}"),
            format!("{b:?}"),
            "the phase moved nothing"
        );
    }

    #[test]
    fn every_figure_stays_inside_its_rect() {
        for side in [4.0f32, 8.0, 14.0, 22.0, 60.0, 120.0] {
            let rect = r(side, side * 0.7 + 3.0);
            let within = rect.expand(2.0);
            let mut cases: Vec<(&str, Vec<Shape>)> = Vec::new();
            let mut out = Vec::new();
            octagon(
                &mut out,
                rect,
                CHAMFER,
                Some(INK),
                Some((Weight::Heavy, INK)),
            );
            cases.push(("octagon", out));
            let mut out = Vec::new();
            double_frame(&mut out, rect, 3.0, INK);
            cases.push(("double_frame", out));
            let mut out = Vec::new();
            corner_pads(&mut out, rect, INK);
            cases.push(("corner_pads", out));
            let mut out = Vec::new();
            brackets(&mut out, rect, 8.0, Weight::Bold, INK);
            cases.push(("brackets", out));
            let mut out = Vec::new();
            tick_bar(&mut out, rect, 12, 0.6, INK, GROUND, true);
            tick_bar(&mut out, rect, 12, 0.6, INK, GROUND, false);
            cases.push(("tick_bar", out));
            let mut out = Vec::new();
            choice_bar(&mut out, rect, 5, 2, INK, GROUND);
            cases.push(("choice_bar", out));
            let mut out = Vec::new();
            lattice(&mut out, rect, 8.0, INK);
            cases.push(("lattice", out));
            let mut out = Vec::new();
            barcode(&mut out, rect, &mut Rng::new(1), INK);
            cases.push(("barcode", out));
            for (name, shapes) in cases {
                for s in &shapes {
                    for p in points_of(s) {
                        assert!(
                            within.contains(p),
                            "{name} at {side}: {p:?} outside {rect:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_tick_bar_lights_from_the_bottom_and_the_left() {
        let rect = r(120.0, 10.0);
        let mut out = Vec::new();
        tick_bar(&mut out, rect, 10, 0.5, INK, GROUND, true);
        let lit: Vec<f32> = out
            .iter()
            .filter_map(|s| match s {
                Shape::Rect(rs) if rs.fill == INK => Some(rs.rect.min.x),
                _ => None,
            })
            .collect();
        assert_eq!(lit.len(), 5);
        assert!(lit.iter().all(|x| *x < rect.center().x));
    }

    #[test]
    fn a_board_puts_a_junction_at_every_crossing() {
        let mut out = Vec::new();
        board(
            &mut out,
            r(200.0, 100.0),
            &[40.0, 80.0, 120.0],
            &[50.0, 90.0],
            INK,
            GROUND,
        );
        let pads = out
            .iter()
            .filter(|s| matches!(s, Shape::Rect(rs) if rs.fill == INK))
            .count();
        assert_eq!(pads, 6);
    }

    #[test]
    fn the_same_input_draws_the_same_shapes() {
        let draw = || {
            let mut out = Vec::new();
            barcode(&mut out, r(100.0, 12.0), &mut Rng::seeded("code"), INK);
            annotation_arrow(&mut out, pos2(10.0, 10.0), pos2(80.0, 40.0), INK);
            format!("{out:?}")
        };
        assert_eq!(draw(), draw());
    }
}
