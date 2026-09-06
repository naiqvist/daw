#![allow(dead_code)]
//! The strip band: the console's sections as interlocking pieces.
//!
//! A section's card is a chamfered casing with a TONGUE on its right
//! edge and a NOTCH on its left, cut at the corner's angle, so a card
//! slots into the next and two cards' outlines are one line with a step
//! in it. The signal enters at the notch and leaves by the tongue — a
//! pair of traces when the path is stereo — and the route through the
//! card is the card's own: it is drawn by hand per section, and it is
//! the section's signature as much as its figure is.
//!
//! A section that is IN is a solid piece: surface fill, the figure in
//! the middle, the pair passing through. A section that is OUT is a
//! bypass piece: the same outline, but the ground shows through and the
//! pair crosses on a rail along the foot. Switching IN lifts the rail
//! into the figure; nothing moves.
//!
//! Every section's figure is a placeholder until its effect is written:
//! a recess with the section's word in it. The routes are real.

use super::palette;
use super::*;
use crate::console::{SectionKind, Width};
use crate::ui::chrome;

/// A narrow piece's width; a wide piece is the chain card's.
pub const NARROW_W: f32 = 184.0;
/// PREAMP gets one extra grid-and-a-half for its meter and transformer
/// bank. It is still compact, but no longer shares TONE's exact footprint.
pub const PREAMP_W: f32 = 440.0;
/// How far the tongue reaches into the next piece.
pub const TONGUE: f32 = 12.0;
/// The tongue's height, and the notch's.
pub const JOINT_H: f32 = 26.0;
/// The head band, where the name and the IN pad live and the joint sits.
pub const HEAD_H: f32 = 30.0;
/// The tallest figure any section draws at the top of its glass; the
/// rows begin under it. The band's row capacity is set by this.
pub const FIGURE_MAX_H: f32 = 64.0;
/// The foot under the glass, where an OUT piece's bypass rail runs.
pub const FOOT_H: f32 = 16.0;
/// A parameter row.
pub const ROW_H: f32 = 19.0;
/// The two traces' spacing about the joint's centre.
const PAIR: f32 = 4.0;

/// The joint's centre line, from the piece's top.
pub fn joint_y(rect: egui::Rect) -> f32 {
    rect.top() + HEAD_H * 0.5
}

/// The width of a piece for `kind`.
///
/// Every section has its own, because a desk is not a row of identical
/// modules: what a section needs to show decides how much face it gets,
/// and a run of pieces that are all one width reads as a table rather
/// than as a machine. The JOINT does not vary — that is what lets any
/// two of them mate — so the shapes can differ as much as they like.
///
/// No two neighbours on the strip share a width, which is what gives
/// the run its rhythm: wide, wide, narrow, narrow, wide.
pub fn width_of(kind: SectionKind) -> f32 {
    match kind {
        // The channel's own stages.
        SectionKind::Preamp => PREAMP_W,
        SectionKind::Tone => 424.0,
        SectionKind::Door => 496.0,
        SectionKind::Cut => 468.0,
        SectionKind::Hit => 412.0,
        SectionKind::Four => 516.0,
        SectionKind::Vca => 484.0,
        SectionKind::Split => 476.0,
        SectionKind::Pump => 396.0,
        SectionKind::Drive => 488.0,
        SectionKind::Grit => 462.0,
        SectionKind::Shine => 434.0,
        SectionKind::Drift => 472.0,
        SectionKind::Phase => 466.0,
        SectionKind::Smear => 452.0,
        SectionKind::Ring => 442.0,
        SectionKind::Spectra => 264.0,
        SectionKind::Echo => 252.0,
        SectionKind::Room => 228.0,
        SectionKind::Out => 176.0,
        // The desk's own.
        SectionKind::Glue => 182.0,
        SectionKind::Iron => 166.0,
        SectionKind::Ceiling => 190.0,
        SectionKind::Scope => 216.0,
        // The returns, which stand off the rail on their own.
        SectionKind::Tape => 252.0,
        SectionKind::Shadow => 232.0,
    }
}

/// The corner cuts a piece wears, as multiples of the house chamfer.
///
/// A family's leaning survives — dynamics heavy on top, tone heavy at
/// the foot, motion a step in the top edge, space a step in the foot —
/// but each card breaks it where its own silhouette wants to, so no two
/// neighbours are cut alike and the run's corners are a rhythm rather
/// than a rule.
pub fn cuts(kind: SectionKind) -> (f32, f32, f32, f32) {
    // The console's window: two opposite corners cut at the house
    // chamfer, the other two square — keyed, so it reads as a component
    // that fits one way round. Every piece the same; the section's
    // identity is its width, its face and its route, not a corner.
    let _ = kind;
    let c = chrome::CHAMFER;
    (c, 0.0, c, 0.0)
}

/// The lowest a crevice may be cut into a wall, as a share of the
/// piece's height.
///
/// The JOINT is sacred: a tongue lies in a notch at a fixed height, and
/// a bay that reached up into it would stop two pieces mating. Every bay
/// starts below this, however short the band's tray becomes.
const CREVICE_FLOOR: f32 = 0.24;

/// A step cut into a piece's TOP edge: where along the width it falls
/// and how far it drops. A piece with one is taller on the left than
/// on the right, which is what stops a run of them reading as a row of
/// boxes.
pub fn shoulder(kind: SectionKind) -> Option<(f32, f32)> {
    // No steps in a top edge: a piece is a plain component.
    let _ = kind;
    None
}

/// A bay cut into a piece's RIGHT wall, under the joint: where it
/// starts as a share of the height, how tall it is, and how deep. What
/// a bay is FOR is the face beside it — a control that sits in the
/// crevice rather than in the middle of the glass.
///
/// Roughly two thirds of the desk wears one, and no two neighbours wear
/// the same combination, so the run's edges are a broken line rather
/// than a rule.
pub fn bay(kind: SectionKind) -> Option<(f32, f32, f32)> {
    let (at, tall, deep): (f32, f32, f32) = match kind {
        SectionKind::Door => (0.63, 34.0, 16.0),
        SectionKind::Four => (0.28, 34.0, 14.0),
        SectionKind::Vca => (0.28, 32.0, 18.0),
        SectionKind::Split => (0.40, 90.0, 20.0),
        SectionKind::Pump => (0.30, 66.0, 15.0),
        SectionKind::Drive => (0.26, 86.0, 18.0),
        SectionKind::Grit => (0.26, 36.0, 20.0),
        SectionKind::Drift => (0.42, 46.0, 18.0),
        SectionKind::Phase => (0.66, 36.0, 15.0),
        SectionKind::Smear => (0.26, 104.0, 15.0),
        SectionKind::Ring => (0.58, 34.0, 14.0),
        SectionKind::Spectra => (0.26, 52.0, 18.0),
        SectionKind::Echo => (0.44, 34.0, 16.0),
        SectionKind::Room => (0.33, 96.0, 22.0),
        SectionKind::Out => (0.52, 42.0, 16.0),
        SectionKind::Iron => (0.30, 58.0, 14.0),
        SectionKind::Ceiling => (0.29, 30.0, 14.0),
        SectionKind::Scope => (0.28, 108.0, 16.0),
        SectionKind::Tape => (0.30, 44.0, 18.0),
        SectionKind::Shadow => (0.26, 78.0, 18.0),
        _ => return None,
    };
    Some((at.max(CREVICE_FLOOR), tall, deep))
}

/// A step cut into a piece's BOTTOM edge, on the right: how far along
/// it starts and how far it lifts.
pub fn plinth(kind: SectionKind) -> Option<(f32, f32)> {
    match kind {
        SectionKind::Door => Some((0.58, 13.0)),
        SectionKind::Hit => Some((0.46, 11.0)),
        SectionKind::Four => Some((0.62, 12.0)),
        SectionKind::Vca => Some((0.72, 15.0)),
        SectionKind::Drive => Some((0.58, 12.0)),
        SectionKind::Grit => Some((0.72, 12.0)),
        SectionKind::Shine => Some((0.62, 14.0)),
        SectionKind::Drift => Some((0.62, 14.0)),
        SectionKind::Phase => Some((0.52, 14.0)),
        SectionKind::Ring => Some((0.54, 14.0)),
        SectionKind::Spectra => Some((0.42, 16.0)),
        SectionKind::Echo => Some((0.60, 12.0)),
        SectionKind::Room => Some((0.62, 18.0)),
        SectionKind::Out => Some((0.72, 16.0)),
        SectionKind::Ceiling => Some((0.72, 14.0)),
        SectionKind::Scope => Some((0.62, 14.0)),
        SectionKind::Tape => Some((0.62, 14.0)),
        _ => None,
    }
}

/// The piece's outline, clockwise from the top-left, with the tongue
/// and the notch where the piece has them. `(tl, tr, br, bl)` are the
/// corner cuts.
pub fn outline(rect: egui::Rect, kind: SectionKind, notch: bool, tongue: bool) -> Vec<egui::Pos2> {
    let (tl, tr, br, bl) = cuts(kind);
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let jy = joint_y(rect);
    let (jt, jb) = (jy - JOINT_H * 0.5, jy + JOINT_H * 0.5);
    let c = chrome::CHAMFER * 0.6;
    let mut points = vec![egui::pos2(l + tl, t)];
    // The top edge, with its step where the piece has one: along, down
    // the riser at the chamfer's angle, and on at the lower level.
    let top_right = match shoulder(kind) {
        Some((at, drop)) => {
            let x = l + rect.width() * at;
            points.push(egui::pos2(x - c, t));
            points.push(egui::pos2(x + c, t + drop));
            t + drop
        }
        None => t,
    };
    points.push(egui::pos2(r - tr, top_right));
    points.push(egui::pos2(r, top_right + tr));
    if tongue {
        points.extend([
            egui::pos2(r, jt),
            egui::pos2(r + TONGUE - c, jt),
            egui::pos2(r + TONGUE, jt + c),
            egui::pos2(r + TONGUE, jb - c),
            egui::pos2(r + TONGUE - c, jb),
            egui::pos2(r, jb),
        ]);
    }
    // The right wall's bay, under the joint.
    if let Some((at, tall, deep)) = bay(kind) {
        let y0 = t + rect.height() * at;
        let y1 = y0 + tall;
        points.extend([
            egui::pos2(r, y0),
            egui::pos2(r - deep, y0 + c),
            egui::pos2(r - deep, y1 - c),
            egui::pos2(r, y1),
        ]);
    }
    points.push(egui::pos2(r, b - br));
    // The bottom edge, with its plinth where the piece has one.
    match plinth(kind) {
        Some((at, lift)) => {
            let x = l + rect.width() * at;
            points.push(egui::pos2(r - br, b));
            points.push(egui::pos2(x + c, b));
            points.push(egui::pos2(x - c, b - lift));
            points.push(egui::pos2(l + bl, b - lift));
            points.push(egui::pos2(l, b - lift - bl));
        }
        None => {
            points.push(egui::pos2(r - br, b));
            points.push(egui::pos2(l + bl, b));
            points.push(egui::pos2(l, b - bl));
        }
    }
    if notch {
        points.extend([
            egui::pos2(l, jb),
            egui::pos2(l + TONGUE - c, jb),
            egui::pos2(l + TONGUE, jb - c),
            egui::pos2(l + TONGUE, jt + c),
            egui::pos2(l + TONGUE - c, jt),
            egui::pos2(l, jt),
        ]);
    }
    points.push(egui::pos2(l, t + tl));
    points
}

/// The piece's fill, cut for its corners and its notch. The tongue is
/// drawn later, over the next piece's notch, by [`tongue_fill`].
pub fn body_fill(
    out: &mut Vec<egui::Shape>,
    rect: egui::Rect,
    kind: SectionKind,
    notch: bool,
    fill: egui::Color32,
    ground: egui::Color32,
) {
    // The casing is the rectangle, and every cut in the silhouette is
    // the ground painted back over it. Filling the outline instead
    // would ask the tessellator to fill a concave path, which it will
    // not do honestly — and a step, a bay and a notch are all concave.
    let (tl, tr, br, bl) = cuts(kind);
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let c = chrome::CHAMFER * 0.6;
    out.push(egui::Shape::rect_filled(rect, 0.0, fill));
    let tri = |out: &mut Vec<egui::Shape>, pts: Vec<egui::Pos2>| {
        out.push(egui::Shape::convex_polygon(pts, ground, egui::Stroke::NONE));
    };
    let block = |out: &mut Vec<egui::Shape>, a: egui::Pos2, z: egui::Pos2| {
        out.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(a, z),
            0.0,
            ground,
        ));
    };
    // The step in the top edge: the shoulder is gone on the right of
    // it, and the riser between is a chamfer.
    let top_right = match shoulder(kind) {
        Some((at, drop)) => {
            let x = l + rect.width() * at;
            block(out, egui::pos2(x + c, t), egui::pos2(r, t + drop));
            tri(
                out,
                vec![
                    egui::pos2(x - c, t),
                    egui::pos2(x + c, t),
                    egui::pos2(x + c, t + drop),
                ],
            );
            t + drop
        }
        None => t,
    };
    // The step in the bottom edge, on the left of it.
    let bottom_left = match plinth(kind) {
        Some((at, lift)) => {
            let x = l + rect.width() * at;
            block(out, egui::pos2(l, b - lift), egui::pos2(x - c, b));
            tri(
                out,
                vec![
                    egui::pos2(x - c, b - lift),
                    egui::pos2(x + c, b),
                    egui::pos2(x - c, b),
                ],
            );
            b - lift
        }
        None => b,
    };
    // The bay in the right wall.
    if let Some((at, tall, deep)) = bay(kind) {
        let y0 = t + rect.height() * at;
        block(
            out,
            egui::pos2(r - deep, y0 + c),
            egui::pos2(r, y0 + tall - c),
        );
        tri(
            out,
            vec![
                egui::pos2(r, y0),
                egui::pos2(r - deep, y0 + c),
                egui::pos2(r, y0 + c),
            ],
        );
        tri(
            out,
            vec![
                egui::pos2(r, y0 + tall),
                egui::pos2(r - deep, y0 + tall - c),
                egui::pos2(r, y0 + tall - c),
            ],
        );
    }
    // The corners, each against the edge it actually sits on.
    tri(
        out,
        vec![
            egui::pos2(l, t),
            egui::pos2(l + tl, t),
            egui::pos2(l, t + tl),
        ],
    );
    tri(
        out,
        vec![
            egui::pos2(r, top_right),
            egui::pos2(r - tr, top_right),
            egui::pos2(r, top_right + tr),
        ],
    );
    tri(
        out,
        vec![
            egui::pos2(r, b),
            egui::pos2(r - br, b),
            egui::pos2(r, b - br),
        ],
    );
    tri(
        out,
        vec![
            egui::pos2(l, bottom_left),
            egui::pos2(l + bl, bottom_left),
            egui::pos2(l, bottom_left - bl),
        ],
    );
    if notch {
        let jy = joint_y(rect);
        let (jt, jb) = (jy - JOINT_H * 0.5, jy + JOINT_H * 0.5);
        out.push(egui::Shape::convex_polygon(
            vec![
                egui::pos2(l, jt),
                egui::pos2(l + TONGUE - c, jt),
                egui::pos2(l + TONGUE, jt + c),
                egui::pos2(l + TONGUE, jb - c),
                egui::pos2(l + TONGUE - c, jb),
                egui::pos2(l, jb),
            ],
            ground,
            egui::Stroke::NONE,
        ));
    }
}

/// The tongue's fill, drawn after every body so it lies in the next
/// piece's notch.
pub fn tongue_fill(out: &mut Vec<egui::Shape>, rect: egui::Rect, fill: egui::Color32) {
    let r = rect.right();
    let jy = joint_y(rect);
    let (jt, jb) = (jy - JOINT_H * 0.5, jy + JOINT_H * 0.5);
    let c = chrome::CHAMFER * 0.6;
    out.push(egui::Shape::convex_polygon(
        vec![
            egui::pos2(r - 0.5, jt),
            egui::pos2(r + TONGUE - c, jt),
            egui::pos2(r + TONGUE, jt + c),
            egui::pos2(r + TONGUE, jb - c),
            egui::pos2(r + TONGUE - c, jb),
            egui::pos2(r - 0.5, jb),
        ],
        fill,
        egui::Stroke::NONE,
    ));
}

/// The glass: under the head, inset from the walls, down to the foot.
/// The screen is most of the piece — the figure at its top, every
/// parameter beneath it.
pub fn recess(rect: egui::Rect) -> egui::Rect {
    recess_of(rect, SectionKind::Out)
}

/// The glass, keeping out of whatever the piece's silhouette has cut
/// away: a step in the top edge takes the head down with it, a plinth
/// takes the foot up, and the bay in the right wall is not the glass's
/// at all — it belongs to whatever sits in it.
pub fn recess_of(rect: egui::Rect, kind: SectionKind) -> egui::Rect {
    let drop = shoulder(kind).map_or(0.0, |(_, drop)| drop);
    let lift = plinth(kind).map_or(0.0, |(_, lift)| lift);
    let deep = bay(kind).map_or(0.0, |(_, _, deep)| deep);
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.top() + drop + HEAD_H + 4.0),
        egui::pos2(rect.right() - 10.0 - deep, rect.bottom() - FOOT_H - lift),
    )
}

/// The bay's own rectangle, for the instrument that lives in it.
pub fn bay_rect(rect: egui::Rect, kind: SectionKind) -> Option<egui::Rect> {
    let (at, tall, deep) = bay(kind)?;
    let y0 = rect.top() + rect.height() * at;
    Some(egui::Rect::from_min_max(
        egui::pos2(rect.right() - deep - 2.0, y0 + 2.0),
        egui::pos2(rect.right() - 3.0, y0 + tall - 2.0),
    ))
}

/// The plinth's own rectangle: the low shelf along the piece's foot,
/// left of the step.
pub fn plinth_rect(rect: egui::Rect, kind: SectionKind) -> Option<egui::Rect> {
    let (at, lift) = plinth(kind)?;
    Some(egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.bottom() - lift - FOOT_H + 2.0),
        egui::pos2(
            rect.left() + rect.width() * at - 6.0,
            rect.bottom() - lift + 2.0,
        ),
    ))
}

/// How tall a section's figure is at the top of its glass.
pub fn figure_h(kind: SectionKind) -> f32 {
    match kind {
        // A section whose face IS its parameters is handed the whole
        // glass; the rest get the figure band at the top of theirs.
        kind if kind.owns_its_glass() => FIGURE_MAX_H,
        _ => 30.0,
    }
}

/// The figure's part of the glass.
pub fn figure_rect(rect: egui::Rect, kind: SectionKind) -> egui::Rect {
    let glass = recess_of(rect, kind).shrink(3.0);
    egui::Rect::from_min_max(
        glass.min,
        egui::pos2(glass.max.x, (glass.min.y + figure_h(kind)).min(glass.max.y)),
    )
}

/// Where the rows begin, inside the glass.
pub fn rows_top(rect: egui::Rect, kind: SectionKind) -> f32 {
    figure_rect(rect, kind).bottom() + 4.0
}

/// The pair's two routes through a piece, from the notch's inner wall
/// to the tongue's tip. Each section's own; the placeholder routes are
/// four ways round the recess, until each section draws its own
/// through its figure.
pub fn route(
    rect: egui::Rect,
    kind: SectionKind,
    notch: bool,
    tongue: bool,
) -> [Vec<egui::Pos2>; 2] {
    let jy = joint_y(rect);
    let x0 = if notch {
        rect.left() + TONGUE
    } else {
        rect.left()
    };
    let x1 = if tongue {
        rect.right() + TONGUE
    } else {
        rect.right()
    };
    let hole = figure_rect(rect, kind);
    let a = egui::pos2(x0, jy - PAIR);
    let b = egui::pos2(x0, jy + PAIR);
    let a_end = egui::pos2(x1, jy - PAIR);
    let b_end = egui::pos2(x1, jy + PAIR);
    let over = |y: f32, from: egui::Pos2, to: egui::Pos2| -> Vec<egui::Pos2> {
        let mut path = vec![from];
        path.extend(
            chrome::elbow(from, egui::pos2(hole.left() - 6.0, y))
                .into_iter()
                .skip(1),
        );
        path.push(egui::pos2(hole.right() + 6.0, y));
        path.extend(
            chrome::elbow(egui::pos2(hole.right() + 6.0, y), to)
                .into_iter()
                .skip(1),
        );
        path
    };
    if kind == SectionKind::Preamp {
        // The transformer: the pair comes down the screen's left wall,
        // meets at the needle's pivot — primary and secondary — and
        // leaves up the right wall.
        let pivot = preamp_pivot(hole);
        let left_wall = hole.left() + 8.0;
        let right_wall = hole.right() - 8.0;
        let mut top = vec![a];
        top.extend(
            chrome::elbow(a, egui::pos2(left_wall, pivot.y - 3.0))
                .into_iter()
                .skip(1),
        );
        top.push(pivot);
        top.push(egui::pos2(right_wall, pivot.y - 3.0));
        top.extend(
            chrome::elbow(egui::pos2(right_wall, pivot.y - 3.0), a_end)
                .into_iter()
                .skip(1),
        );
        let mut bottom = vec![b];
        bottom.extend(
            chrome::elbow(b, egui::pos2(left_wall - 4.0, pivot.y + 3.0))
                .into_iter()
                .skip(1),
        );
        bottom.push(pivot);
        bottom.push(egui::pos2(right_wall + 4.0, pivot.y + 3.0));
        bottom.extend(
            chrome::elbow(egui::pos2(right_wall + 4.0, pivot.y + 3.0), b_end)
                .into_iter()
                .skip(1),
        );
        return [top, bottom];
    }
    match kind.strip_index().unwrap_or(0) % 4 {
        // Straight through the head, both.
        0 => [vec![a, a_end], vec![b, b_end]],
        // Both dip to the recess's top edge and back.
        1 => [
            over(hole.top() + 6.0, a, a_end),
            over(hole.top() + 12.0, b, b_end),
        ],
        // Split: one over the head, one down the recess's foot.
        2 => [vec![a, a_end], over(hole.bottom() - 6.0, b, b_end)],
        // Crossed: they swap through the recess's middle.
        _ => [
            over(hole.center().y + 4.0, a, b_end),
            over(hole.center().y - 4.0, b, a_end),
        ],
    }
}

/// The bypass rail an OUT piece carries: the pair drops from the notch
/// to the foot, runs across, and climbs to the tongue.
pub fn bypass(rect: egui::Rect, notch: bool, tongue: bool) -> [Vec<egui::Pos2>; 2] {
    let jy = joint_y(rect);
    let x0 = if notch {
        rect.left() + TONGUE
    } else {
        rect.left()
    };
    let x1 = if tongue {
        rect.right() + TONGUE
    } else {
        rect.right()
    };
    let foot = rect.bottom() - 9.0;
    let run = |dy: f32, y: f32| -> Vec<egui::Pos2> {
        let from = egui::pos2(x0, jy + dy);
        let to = egui::pos2(x1, jy + dy);
        let mut path = vec![from];
        path.extend(
            chrome::elbow(from, egui::pos2(x0 + 18.0, y))
                .into_iter()
                .skip(1),
        );
        path.push(egui::pos2(x1 - 18.0, y));
        path.extend(
            chrome::elbow(egui::pos2(x1 - 18.0, y), to)
                .into_iter()
                .skip(1),
        );
        path
    };
    [run(-PAIR, foot - PAIR), run(PAIR, foot + PAIR)]
}

/// One piece as the band lays it: which column, where, and its joints.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub index: usize,
    pub rect: egui::Rect,
    pub kind: SectionKind,
    pub notch: bool,
    pub tongue: bool,
}

impl Stage {
    /// The piece's body: the casing, cut for its corners and notch.
    /// Bodies are laid before any tongue so a tongue lies in a notch.
    pub(super) fn draw_piece_body(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
    ) {
        let alpha = self.glass();
        // A casing is outlined on the ground, like every chassis on the
        // console; it is not a plate, and it casts no shadow.
        let fill = alpha.ground.color;
        let _ = column;
        let mut shapes = Vec::new();
        // A piece stands one hard step above the field. The shadow is
        // the same cut body translated once: no blur and no gradient.
        body_fill(
            &mut shapes,
            piece.rect,
            piece.kind,
            piece.notch,
            fill,
            alpha.ground.color,
        );
        painter.extend(shapes);
    }

    /// The piece's face: the tongue, the outline, the head, the figure's
    /// recess, the signal through it, and its rows.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_piece_face(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        sounding: bool,
        phase: Phase,
        level: Option<f32>,
    ) {
        let alpha = self.glass();
        let rect = piece.rect;
        let is_in = !column.bypassed;
        let fill = alpha.ground.color;
        let ink = if is_in {
            alpha.ink.color
        } else {
            alpha.edge.color
        };
        let edge = alpha.edge.color;
        let focused = cursor.is_some_and(|(col, _)| col == piece.index);
        let selected_row = cursor.and_then(|(col, row)| (col == piece.index).then_some(row));
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));

        let mut shapes = Vec::new();
        // A basic chamfered card: the console's keyed outline, solid while
        // the section is IN — the live chassis under the cursor — and
        // dashed while it is OUT.
        let path = chrome::keyed_outline(rect);
        if is_in {
            chrome::trace(
                &mut shapes,
                &path,
                Weight::Hair,
                if focused {
                    palette::colours().chassis
                } else {
                    edge
                },
            );
        } else {
            chrome::dashes(&mut shapes, &path, 0.0, Weight::Hair, edge);
        }
        // The head's foot: a rule under the name band, short of the walls.
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(rect.left() + 8.0, rect.top() + HEAD_H),
                egui::pos2(rect.right() - 8.0, rect.top() + HEAD_H),
            ],
            Weight::Hair,
            edge.gamma_multiply(if is_in { 1.0 } else { 0.6 }),
        );

        // The signal: through the figure when IN, along the foot when OUT.

        // The screen: the piece's casing is the shell, and this is the
        // glass set into it — the ground showing through a double
        // frame with a rune tick at each corner. An OUT piece's glass
        // is dark: the frame dim, no figure, its rows in the edge ink.
        let hole = recess_of(rect, piece.kind);
        if hole.is_positive() {
            let screen_edge = if is_in {
                edge
            } else {
                edge.gamma_multiply(0.55)
            };
            screen_frame(&mut shapes, hole, screen_edge, alpha.ground.color);
        }
        // The IN pad on the head's right: lit while IN, fixed for a
        // section the desk never lets out.
        painter.extend(shapes);

        // The name, in the block face, over the joint's rail.
        painter.text(
            egui::pos2(rect.left() + 10.0, rect.top() + 6.0),
            egui::Align2::LEFT_TOP,
            piece.kind.name(),
            egui::FontId::monospace(11.0),
            ink,
        );

        // The reveal: a screen that comes under the hand redraws itself
        // top to bottom with a scan line at the edge, the way a terminal
        // painted a page. Once painted it stays.
        let glass = hole.shrink(3.0);
        let reveal = 1.0;
        let shown = egui::Rect::from_min_max(
            glass.min,
            egui::pos2(glass.max.x, glass.min.y + glass.height() * reveal),
        );
        // The reveal's clip takes in the piece's crevices as well as
        // its glass: a control that lives in a bay is still the
        // section's face, and would otherwise be wiped away with the
        // wall it sits in.
        let mut clip = shown;
        for crevice in [bay_rect(rect, piece.kind), plinth_rect(rect, piece.kind)]
            .into_iter()
            .flatten()
        {
            clip = clip.union(crevice);
        }
        let screen = painter.with_clip_rect(clip);
        let figure = figure_rect(rect, piece.kind);
        let face = if piece.kind.owns_its_glass() {
            glass
        } else {
            figure
        };
        if is_in {
            self.draw_figure(&screen, piece, column, face, selected_row, level, phase);
        } else if piece.kind.owns_its_glass() {
            // A section whose parameters ARE its picture keeps the
            // picture while it is out — behind a veil, with the word
            // over it. A blank glass would say nothing about what the
            // piece is, and the settings are still there waiting: this
            // is a section switched out, not a section emptied.
            self.draw_figure(&screen, piece, column, face, None, level, Phase::STILL);
            screen.rect_filled(glass, 0.0, alpha.ground.color.gamma_multiply(0.72));
            screen.text(
                glass.center(),
                egui::Align2::CENTER_CENTER,
                "OUT",
                row_font.clone(),
                ink.gamma_multiply(0.8),
            );
        } else {
            screen.text(
                figure.center(),
                egui::Align2::CENTER_CENTER,
                "OUT",
                row_font.clone(),
                edge.gamma_multiply(0.9),
            );
        }

        // The rows, on the glass under the figure: a terminal's table —
        // the name, a leader of dots, the value; the cursor's row an
        // inverse block; a wide piece's gauges as runs of cells.
        // PREAMP's five parameters are already the five instruments on
        // its face, so it has no second, generic table under them.
        // A section that owns its glass has drawn every parameter as an
        // instrument already, so it gets no second, generic table.
        let rows_shown = if piece.kind.owns_its_glass() {
            0
        } else {
            rows_shown
        };
        let wide = piece.kind.width() == Width::Wide;
        let top = rows_top(rect, piece.kind);
        let text_ink = if is_in { ink } else { edge };
        let cell_w = painter
            .layout_no_wrap("M".to_owned(), row_font.clone(), ink)
            .rect
            .width()
            .max(1.0);
        for line in 0..rows_shown {
            let Some(row) = column.rows.get(row_offset + line) else {
                break;
            };
            let y = top + line as f32 * ROW_H;
            if y + ROW_H > glass.bottom() - 2.0 {
                break;
            }
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(glass.left() + 4.0, y),
                egui::vec2(glass.width() - 8.0, ROW_H),
            );
            let on_row = focused && cursor == Some((piece.index, row_offset + line));
            if on_row {
                screen.rect_filled(row_rect, 0.0, alpha.live_dim.color);
                crate::ui::nav_cursor::claim(
                    &screen,
                    ("stage-strip-row-cursor", piece.index, row_offset + line),
                    row_rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Surface,
                    palette::colours().alert,
                );
            }
            let value_ink = if on_row {
                palette::colours().bright
            } else if row.edited {
                alpha.focus.color
            } else {
                text_ink
            };
            let name_ink = if on_row {
                palette::colours().bright
            } else {
                text_ink
            };
            let value_w = painter
                .layout_no_wrap(row.value.clone(), row_font.clone(), value_ink)
                .rect
                .width();
            let gauge_w = if wide { 44.0 } else { 0.0 };
            let value_col = if wide {
                (cell_w * 8.0).max(value_w)
            } else {
                value_w
            };
            let gauge = egui::Rect::from_center_size(
                egui::pos2(
                    row_rect.right() - value_col - gauge_w * 0.5 - 8.0,
                    row_rect.center().y,
                ),
                egui::vec2(gauge_w, 7.0),
            );
            let name_room = if wide {
                gauge.left() - row_rect.left() - 8.0
            } else {
                row_rect.width() - value_col - 10.0
            };
            let cells = (name_room / cell_w).floor().max(1.0) as usize;
            let name = fit_cells(&row.name, cells);
            let name_w = painter
                .layout_no_wrap(
                    row.name.chars().take(cells).collect(),
                    row_font.clone(),
                    name_ink,
                )
                .rect
                .width();
            screen.text(
                egui::pos2(row_rect.left() + 2.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                row_font.clone(),
                name_ink,
            );
            // The leader: dots from the name to the value or the gauge.
            let leader_end = if wide {
                gauge.left() - 6.0
            } else {
                row_rect.right() - value_col - 6.0
            };
            let mut x = row_rect.left() + 2.0 + name_w + cell_w;
            let mut dots = Vec::new();
            while x < leader_end {
                chrome::dot(
                    &mut dots,
                    egui::pos2(x, row_rect.center().y + 3.0),
                    if on_row { alpha.ground.color } else { edge },
                );
                x += cell_w;
            }
            screen.extend(dots);
            if wide {
                let mut marks = Vec::new();
                let rest = if on_row { alpha.surface.color } else { edge };
                if row.choices > 0 {
                    chrome::choice_bar(&mut marks, gauge, row.choices, row.choice, value_ink, rest);
                } else {
                    chrome::tick_bar(&mut marks, gauge, 12, row.place, value_ink, rest, true);
                }
                screen.extend(marks);
            }
            screen.text(
                egui::pos2(row_rect.right() - 2.0, row_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                row_font.clone(),
                value_ink,
            );
        }
        // The "more below" arrow belongs to the generic table. A section
        // whose face IS its parameters has no table and no more below.
        if !piece.kind.owns_its_glass() && column.rows.len() > row_offset + rows_shown {
            let mut marks = Vec::new();
            chrome::annotation_arrow(
                &mut marks,
                egui::pos2(glass.center().x, glass.bottom() - 10.0),
                egui::pos2(glass.center().x, glass.bottom() - 1.0),
                text_ink,
            );
            screen.extend(marks);
        }
        if reveal < 1.0 {
            let y = shown.max.y;
            let mut scan = Vec::new();
            chrome::trace(
                &mut scan,
                &[egui::pos2(glass.left(), y), egui::pos2(glass.right(), y)],
                Weight::Heavy,
                alpha.live.color,
            );
            painter.extend(scan);
            painter.ctx().request_repaint();
        }
        if is_in && piece.kind.full_screen() {
            // At the FOOT of the glass, not the top: the top of a
            // section's glass is its figure, and a hint that sits over a
            // drawing is a hint that has taken the drawing's place.
            painter.text(
                egui::pos2(hole.right() - 6.0, hole.bottom() - 3.0),
                egui::Align2::RIGHT_BOTTOM,
                "⏎ FULL",
                row_font.clone(),
                edge,
            );
        }

        // Register the glass itself, after its contents exist. The local
        // phosphor pass reads those real pixels, so glyphs, curves, needles
        // and the cursor emit while the vector casing stays untouched.
        if glass.is_positive() && self.polarity == design::Polarity::Dark {
            let meter = level.unwrap_or(0.0).clamp(0.0, 1.0);
            let addressed = if selected_row.is_some() { 0.20 } else { 0.0 };
            let activity = if sounding && phase.rolling {
                meter.max(0.22 + phase.pulse() * 0.58)
            } else {
                (meter * 0.32_f32).max(addressed)
            };
            crate::shell::screen::register(
                painter,
                glass.shrink(1.0),
                crate::shell::screen::State::new(
                    if phase.rolling { phase.beat } else { 0.0 },
                    activity,
                ),
            );
        }
    }
}

/// A screen paints itself in about this long when it comes under the
/// hand.
const REVEAL_S: f32 = 0.22;

/// The glass: a double hairline frame and a rune tick in each corner,
/// the ground showing through.
pub fn screen_frame(
    out: &mut Vec<egui::Shape>,
    hole: egui::Rect,
    edge: egui::Color32,
    ground: egui::Color32,
) {
    // A chamfered window: the ground, one keyed outline, and a header
    // rule a title's height down — nothing inside the frame but the
    // face itself.
    out.push(egui::Shape::rect_filled(hole, 0.0, ground));
    chrome::panel_frame_variant(out, hole, Weight::Hair, edge, 0);
    let y = (hole.min.y + 12.0).round() - 0.5;
    chrome::trace(
        out,
        &[
            egui::pos2(hole.min.x + chrome::CHAMFER, y),
            egui::pos2(hole.max.x - 1.0, y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.6),
    );
}

/// PREAMP alone carries a stepped crown above its glass. The darker solid
/// plate and the short calibration cuts read as a replaceable input module,
/// not a texture painted over every device in the chain.
fn preamp_crown(
    out: &mut Vec<egui::Shape>,
    rect: egui::Rect,
    fill: egui::Color32,
    edge: egui::Color32,
) {
    let plate = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.top() + 3.0),
        egui::pos2(rect.right() - 4.0, rect.top() + HEAD_H - 3.0),
    );
    let (l, t, r, b) = (plate.left(), plate.top(), plate.right(), plate.bottom());
    let points = vec![
        egui::pos2(l + 5.0, t),
        egui::pos2(r - 20.0, t),
        egui::pos2(r - 14.0, t + 6.0),
        egui::pos2(r, t + 6.0),
        egui::pos2(r, b - 5.0),
        egui::pos2(r - 5.0, b),
        egui::pos2(l + 16.0, b),
        egui::pos2(l + 11.0, b - 5.0),
        egui::pos2(l, b - 5.0),
        egui::pos2(l, t + 5.0),
    ];
    out.push(egui::Shape::convex_polygon(
        points.clone(),
        fill,
        egui::Stroke::NONE,
    ));
    let mut outline = points;
    outline.push(outline[0]);
    chrome::trace(out, &outline, Weight::Hair, edge.gamma_multiply(0.72));
    for i in 0..3 {
        let x = r - 43.0 + i as f32 * 6.0;
        chrome::trace(
            out,
            &[egui::pos2(x, t + 5.0), egui::pos2(x + 4.0, t + 9.0)],
            Weight::Hair,
            edge,
        );
    }
}

/// The PREAMP glass is a real two-layer aperture: a hard-backed outer
/// bezel, then an alternate-cut inner keyline. Its two square mounts are
/// deliberately sparse so the screen still reads before its decoration.
fn preamp_screen_frame(
    out: &mut Vec<egui::Shape>,
    hole: egui::Rect,
    edge: egui::Color32,
    ground: egui::Color32,
    casing: egui::Color32,
) {
    chrome::panel_variant(
        out,
        hole,
        Some(ground),
        casing,
        Some((Weight::Heavy, edge)),
        0,
    );
    chrome::panel_frame_variant(
        out,
        hole.shrink(3.0),
        Weight::Hair,
        edge.gamma_multiply(0.58),
        3,
    );
    for at in [
        egui::pos2(hole.left() - 5.0, hole.top() + 11.0),
        egui::pos2(hole.right() + 5.0, hole.bottom() - 11.0),
    ] {
        chrome::pad(out, at, chrome::PAD - 1.0, edge, false);
    }
}

/// Where the preamp's needle turns: low on the glass, right of centre,
/// leaving the trim ring its third on the left.
pub fn preamp_pivot(figure: egui::Rect) -> egui::Pos2 {
    egui::pos2(figure.left() + figure.width() * 0.62, figure.bottom() - 7.0)
}

/// A point on an arc about `c`, at `deg` (0 is right, counter-clockwise
/// positive, screen y down).
pub fn on_arc(c: egui::Pos2, r: f32, deg: f32) -> egui::Pos2 {
    let a = deg.to_radians();
    egui::pos2(c.x + r * a.cos(), c.y - r * a.sin())
}

impl Stage {
    /// The section's figure on its glass.
    ///
    /// Everything a face may see is gathered here and handed over as
    /// one [`faces::Face`]: the section's settings with the table's
    /// defaults filled in, what the engine measured of itself last
    /// block, the transport, and the parameter the keyboard stands on.
    /// A face reaches for nothing else — not the stage, not the song,
    /// not the engine — which is what keeps a card honest: it can only
    /// draw what has already been measured or already been set.
    #[allow(clippy::too_many_arguments)]
    fn draw_figure(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
        glass: egui::Rect,
        selected: Option<usize>,
        level: Option<f32>,
        phase: Phase,
    ) {
        let (_, id) = self.band_device(piece.index).unzip();
        let mut params = crate::console::SectionParams::of(piece.kind);
        if let Some(device) = id.and_then(|id| self.song.device(id)) {
            for (param, value) in &device.overrides {
                params.set(*param, *value);
            }
        }
        let lifted = self.glass();
        faces::draw(&faces::Face {
            painter,
            piece,
            glass,
            alpha: &lifted,
            phase,
            said: id.map(|id| self.telemetry(id)).unwrap_or_default(),
            level,
            selected,
            out: column.bypassed,
            params,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every piece has its own footprint, and the joint they mate on
    /// does not vary with it — which is what lets a run of different
    /// shapes still snap together.
    #[test]
    fn every_section_has_its_own_width_and_the_same_joint() {
        let mut seen: Vec<(SectionKind, f32)> = Vec::new();
        for kind in SectionKind::ALL {
            let w = width_of(kind);
            // The ceiling is generous on purpose: a section with a real
            // instrument on it — a transfer curve, a harmonic ladder —
            // needs the room, and a section that does not, does not get
            // it. The floor is what keeps a card from becoming a label.
            assert!(w >= 140.0 && w <= 520.0, "{kind:?} is {w} wide");
            seen.push((kind, w));
        }
        // Neighbours on the strip do not share a width: the run reads
        // as a machine rather than as a table.
        for pair in SectionKind::STRIP.windows(2) {
            assert!(
                (width_of(pair[0]) - width_of(pair[1])).abs() > 0.5,
                "{:?} and {:?} are the same width",
                pair[0],
                pair[1]
            );
        }
        // However wide they are, the joint is in the same place.
        let a = egui::Rect::from_min_size(
            egui::pos2(0.0, 40.0),
            egui::vec2(width_of(SectionKind::Hit), 200.0),
        );
        let b = egui::Rect::from_min_size(
            egui::pos2(0.0, 40.0),
            egui::vec2(width_of(SectionKind::Four), 200.0),
        );
        assert_eq!(joint_y(a), joint_y(b));
    }

    /// DOOR keeps the bay and plinth its face is laid out around, but its
    /// chassis is the same keyed card as every other section.
    #[test]
    fn the_door_keeps_its_face_geometry_inside_the_standard_chassis() {
        let piece = egui::Rect::from_min_size(
            egui::pos2(0.0, 40.0),
            egui::vec2(width_of(SectionKind::Door), 240.0),
        );
        assert_eq!(cuts(SectionKind::Door), cuts(SectionKind::Tone));
        assert!(shoulder(SectionKind::Door).is_none());
        assert!(bay(SectionKind::Door).is_some());
        assert!(plinth(SectionKind::Door).is_some());

        // The screen still keeps clear of the face's bay and plinth. With
        // shoulders removed, it begins on the same line as a plain face.
        let glass = recess_of(piece, SectionKind::Door);
        let plain = recess_of(piece, SectionKind::Tone);
        assert_eq!(glass.top(), plain.top());
        assert!(glass.right() < plain.right());
        assert!(glass.bottom() < plain.bottom());
    }

    /// PREAMP keeps the width its face needs while wearing the same keyed
    /// chassis as every other section.
    #[test]
    fn preamp_owns_a_distinct_width_inside_the_standard_chassis() {
        assert_ne!(width_of(SectionKind::Preamp), width_of(SectionKind::Tone));
        assert_ne!(width_of(SectionKind::Preamp), width_of(SectionKind::Door));
        assert_eq!(cuts(SectionKind::Preamp), cuts(SectionKind::Tone));
    }

    /// A piece with both joints has an outline with a step in on the
    /// left and out on the right, at the same height, so two mate.
    #[test]
    fn the_tongue_and_the_notch_mate() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(NARROW_W, 200.0));
        let path = outline(rect, SectionKind::Tone, true, true);
        let jy = joint_y(rect);
        let deepest_left = path.iter().map(|p| p.x).fold(f32::MAX, f32::min);
        let furthest_right = path.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        assert_eq!(deepest_left, rect.left());
        assert_eq!(furthest_right, rect.right() + TONGUE);
        let notch_wall = path
            .iter()
            .filter(|p| (p.x - (rect.left() + TONGUE)).abs() < 0.01 && (p.y - jy).abs() < JOINT_H)
            .count();
        assert_eq!(notch_wall, 2, "the notch's inner wall has two corners");
        let tongue_tip = path
            .iter()
            .filter(|p| (p.x - (rect.right() + TONGUE)).abs() < 0.01)
            .count();
        assert_eq!(tongue_tip, 2);
        assert!(
            path.iter()
                .any(|p| (p.y - (jy - JOINT_H * 0.5)).abs() < 0.01)
        );
        let next = rect.translate(egui::vec2(rect.width(), 0.0));
        assert_eq!(
            joint_y(next),
            jy,
            "the next piece's notch is at the same height"
        );
    }

    /// Every route starts at the notch's wall and ends at the tongue's
    /// tip, on the pair's two lines, IN or OUT.
    #[test]
    fn every_route_lands_on_the_joints() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(CHAIN_W, 220.0));
        let jy = joint_y(rect);
        for kind in SectionKind::STRIP {
            for paths in [route(rect, kind, true, true), bypass(rect, true, true)] {
                for path in paths {
                    let first = path[0];
                    let last = *path.last().expect("a path");
                    assert_eq!(first.x, rect.left() + TONGUE, "{kind:?}");
                    assert_eq!(last.x, rect.right() + TONGUE, "{kind:?}");
                    assert!((first.y - jy).abs() <= PAIR + 0.01, "{kind:?}");
                    assert!((last.y - jy).abs() <= PAIR + 0.01, "{kind:?}");
                }
            }
        }
    }
}
