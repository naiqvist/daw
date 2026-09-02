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

use super::*;
use crate::console::{SectionKind, Width};

/// A narrow piece's width; a wide piece is the chain card's.
pub const NARROW_W: f32 = 184.0;
/// How far the tongue reaches into the next piece.
pub const TONGUE: f32 = 12.0;
/// The tongue's height, and the notch's.
pub const JOINT_H: f32 = 26.0;
/// The head band, where the name and the IN pad live and the joint sits.
pub const HEAD_H: f32 = 30.0;
/// The figure's recess under the head.
pub const FIGURE_H: f32 = 58.0;
/// A parameter row.
pub const ROW_H: f32 = 19.0;
/// The two traces' spacing about the joint's centre.
const PAIR: f32 = 4.0;

/// The joint's centre line, from the piece's top.
pub fn joint_y(rect: egui::Rect) -> f32 {
    rect.top() + HEAD_H * 0.5
}

/// The width of a piece for `kind`.
pub fn width_of(kind: SectionKind) -> f32 {
    match kind.width() {
        Width::Narrow => NARROW_W,
        Width::Wide => CHAIN_W,
    }
}

/// The corner cuts a family wears, so the run reads as related but
/// distinct pieces: dynamics heavy on top, tone heavy at the foot,
/// motion a step in the top edge, space a step in the foot.
fn cuts(kind: SectionKind) -> (f32, f32, f32, f32) {
    let c = circuit::CHAMFER;
    match kind {
        SectionKind::Door
        | SectionKind::Hit
        | SectionKind::Vca
        | SectionKind::Split
        | SectionKind::Pump
        | SectionKind::Glue
        | SectionKind::Ceiling => (c * 2.0, c * 2.0, c, c),
        SectionKind::Tone
        | SectionKind::Cut
        | SectionKind::Four
        | SectionKind::Drive
        | SectionKind::Grit
        | SectionKind::Shine
        | SectionKind::Iron => (c, c, c * 2.0, c * 2.0),
        SectionKind::Drift
        | SectionKind::Phase
        | SectionKind::Smear
        | SectionKind::Ring
        | SectionKind::Spectra => (c * 2.0, c, c, c * 2.0),
        SectionKind::Echo | SectionKind::Room | SectionKind::Tape | SectionKind::Shadow => {
            (c, c * 2.0, c * 2.0, c)
        }
        SectionKind::Preamp | SectionKind::Out | SectionKind::Scope => (c, c, c, c),
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
    let c = circuit::CHAMFER * 0.6;
    let mut points = vec![
        egui::pos2(l + tl, t),
        egui::pos2(r - tr, t),
        egui::pos2(r, t + tr),
    ];
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
    points.extend([
        egui::pos2(r, b - br),
        egui::pos2(r - br, b),
        egui::pos2(l + bl, b),
        egui::pos2(l, b - bl),
    ]);
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
    let (tl, tr, br, bl) = cuts(kind);
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    out.push(egui::Shape::rect_filled(rect, 0.0, fill));
    let tri = |out: &mut Vec<egui::Shape>, pts: Vec<egui::Pos2>| {
        out.push(egui::Shape::convex_polygon(pts, ground, egui::Stroke::NONE));
    };
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
            egui::pos2(r, t),
            egui::pos2(r - tr, t),
            egui::pos2(r, t + tr),
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
            egui::pos2(l, b),
            egui::pos2(l + bl, b),
            egui::pos2(l, b - bl),
        ],
    );
    if notch {
        let jy = joint_y(rect);
        let (jt, jb) = (jy - JOINT_H * 0.5, jy + JOINT_H * 0.5);
        let c = circuit::CHAMFER * 0.6;
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
    let c = circuit::CHAMFER * 0.6;
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

/// The figure's recess: under the head, inset from the walls.
pub fn recess(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + 14.0, rect.top() + HEAD_H + 4.0),
        egui::pos2(rect.right() - 14.0, rect.top() + HEAD_H + 4.0 + FIGURE_H),
    )
}

/// Where the rows begin.
pub fn rows_top(rect: egui::Rect) -> f32 {
    recess(rect).bottom() + 6.0
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
    let hole = recess(rect);
    let a = egui::pos2(x0, jy - PAIR);
    let b = egui::pos2(x0, jy + PAIR);
    let a_end = egui::pos2(x1, jy - PAIR);
    let b_end = egui::pos2(x1, jy + PAIR);
    let over = |y: f32, from: egui::Pos2, to: egui::Pos2| -> Vec<egui::Pos2> {
        let mut path = vec![from];
        path.extend(
            circuit::elbow(from, egui::pos2(hole.left() - 6.0, y))
                .into_iter()
                .skip(1),
        );
        path.push(egui::pos2(hole.right() + 6.0, y));
        path.extend(
            circuit::elbow(egui::pos2(hole.right() + 6.0, y), to)
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
            circuit::elbow(a, egui::pos2(left_wall, pivot.y - 3.0))
                .into_iter()
                .skip(1),
        );
        top.push(pivot);
        top.push(egui::pos2(right_wall, pivot.y - 3.0));
        top.extend(
            circuit::elbow(egui::pos2(right_wall, pivot.y - 3.0), a_end)
                .into_iter()
                .skip(1),
        );
        let mut bottom = vec![b];
        bottom.extend(
            circuit::elbow(b, egui::pos2(left_wall - 4.0, pivot.y + 3.0))
                .into_iter()
                .skip(1),
        );
        bottom.push(pivot);
        bottom.push(egui::pos2(right_wall + 4.0, pivot.y + 3.0));
        bottom.extend(
            circuit::elbow(egui::pos2(right_wall + 4.0, pivot.y + 3.0), b_end)
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
            circuit::elbow(from, egui::pos2(x0 + 18.0, y))
                .into_iter()
                .skip(1),
        );
        path.push(egui::pos2(x1 - 18.0, y));
        path.extend(
            circuit::elbow(egui::pos2(x1 - 18.0, y), to)
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
        let alpha = self.alphabet();
        let fill = if column.bypassed {
            alpha.ground.color
        } else {
            alpha.surface.color
        };
        let mut shapes = Vec::new();
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
        let alpha = self.alphabet();
        let rect = piece.rect;
        let is_in = !column.bypassed;
        let fill = if is_in {
            alpha.surface.color
        } else {
            alpha.ground.color
        };
        let ink = if is_in {
            alpha.ink.color
        } else {
            alpha.edge.color
        };
        let edge = alpha.edge.color;
        let focused = cursor.is_some_and(|(col, _)| col == piece.index);
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));

        let mut shapes = Vec::new();
        if piece.tongue {
            tongue_fill(&mut shapes, rect, fill);
        }
        let mut path = outline(rect, piece.kind, piece.notch, piece.tongue);
        path.push(path[0]);
        circuit::trace(
            &mut shapes,
            &path,
            if focused { Weight::Heavy } else { Weight::Hair },
            if focused { alpha.ink.color } else { edge },
        );
        // The head's foot: a rule under the name band, short of the walls.
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(rect.left() + 8.0, rect.top() + HEAD_H),
                egui::pos2(rect.right() - 8.0, rect.top() + HEAD_H),
            ],
            Weight::Hair,
            edge.gamma_multiply(if is_in { 1.0 } else { 0.6 }),
        );

        // The signal: through the figure when IN, along the foot when OUT.
        let routes = if is_in {
            route(rect, piece.kind, piece.notch, piece.tongue)
        } else {
            bypass(rect, piece.notch, piece.tongue)
        };
        for path in &routes {
            circuit::trace(
                &mut shapes,
                path,
                Weight::Hair,
                if is_in { ink } else { edge.gamma_multiply(1.3) },
            );
            if sounding && phase.rolling {
                circuit::dashes(
                    &mut shapes,
                    path,
                    phase.dash(),
                    Weight::Heavy,
                    alpha.live_dim.color,
                );
            }
            // Contacts: a pad where the pair lands and where it leaves.
            if let (Some(first), Some(last)) = (path.first(), path.last()) {
                circuit::pad(&mut shapes, *first, circuit::PAD - 1.0, ink, is_in);
                circuit::pad(&mut shapes, *last, circuit::PAD - 1.0, ink, is_in);
            }
        }

        // The screen: the piece's casing is the shell, and this is the
        // glass set into it — the ground showing through a double
        // frame with a rune tick at each corner.
        let hole = recess(rect);
        if is_in && hole.is_positive() {
            screen_frame(&mut shapes, hole, edge, alpha.ground.color);
        }
        // The IN pad on the head's right: lit while IN, fixed for a
        // section the desk never lets out.
        circuit::pad(
            &mut shapes,
            egui::pos2(rect.right() - 12.0, rect.top() + HEAD_H * 0.5),
            circuit::PAD + 1.0,
            if is_in { alpha.live.color } else { edge },
            is_in,
        );
        if piece.kind.always_in() {
            circuit::via(
                &mut shapes,
                egui::pos2(rect.right() - 12.0, rect.top() + HEAD_H * 0.5),
                edge,
                fill,
            );
        }
        painter.extend(shapes);

        // The name, in the block face, over the joint's rail.
        block::paint(
            painter,
            egui::Id::new(("stage-piece-name", piece.index)),
            egui::pos2(
                rect.left() + if piece.notch { TONGUE + 8.0 } else { 10.0 },
                rect.top() + 6.0,
            ),
            egui::Align2::LEFT_TOP,
            block::unit::MICRO,
            piece.kind.name(),
            ink,
        );
        if is_in {
            // The reveal: a screen that comes under the hand redraws
            // itself top to bottom with a scan line at the edge, the
            // way a terminal painted a page. Once painted it stays.
            let reveal = if focused {
                painter.ctx().animate_bool_with_time(
                    egui::Id::new(("stage-piece-reveal", piece.index)),
                    true,
                    REVEAL_S,
                )
            } else {
                painter.ctx().animate_bool_with_time(
                    egui::Id::new(("stage-piece-reveal", piece.index)),
                    false,
                    0.0,
                );
                1.0
            };
            let glass = hole.shrink(3.0);
            let shown = egui::Rect::from_min_max(
                glass.min,
                egui::pos2(glass.max.x, glass.min.y + glass.height() * reveal),
            );
            let screen = painter.with_clip_rect(shown);
            self.draw_figure(&screen, piece, column, glass, level, phase);
            if reveal < 1.0 {
                let y = shown.max.y;
                let mut scan = Vec::new();
                circuit::trace(
                    &mut scan,
                    &[egui::pos2(glass.left(), y), egui::pos2(glass.right(), y)],
                    Weight::Heavy,
                    alpha.live.color,
                );
                painter.extend(scan);
                painter.ctx().request_repaint();
            }
            if piece.kind.full_screen() {
                painter.text(
                    egui::pos2(hole.right() - 6.0, hole.bottom() - 4.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "⏎ FULL",
                    row_font.clone(),
                    edge,
                );
            }
        } else {
            painter.text(
                egui::pos2(rect.center().x, rect.top() + HEAD_H + 14.0),
                egui::Align2::CENTER_CENTER,
                "OUT",
                row_font.clone(),
                edge.gamma_multiply(0.9),
            );
        }

        // The rows, narrow or wide.
        let wide = piece.kind.width() == Width::Wide;
        let top = rows_top(rect);
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
            if y + ROW_H > rect.bottom() - 4.0 {
                break;
            }
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 12.0, y),
                egui::vec2(rect.width() - 24.0, ROW_H),
            );
            let on_row = focused && cursor == Some((piece.index, row_offset + line));
            if on_row {
                painter.rect_filled(row_rect, 0.0, alpha.focus.color);
                let mut marks = Vec::new();
                circuit::brackets(
                    &mut marks,
                    row_rect.expand(2.0),
                    5.0,
                    Weight::Bold,
                    alpha.ink.color,
                );
                painter.extend(marks);
            }
            let value_ink = if on_row {
                alpha.ground.color
            } else if row.edited {
                alpha.focus.color
            } else {
                ink
            };
            let name_ink = if on_row { alpha.ground.color } else { ink };
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
            painter.text(
                egui::pos2(row_rect.left(), row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                fit_cells(&row.name, cells),
                row_font.clone(),
                name_ink,
            );
            if wide {
                let mut marks = Vec::new();
                let rest = if on_row { alpha.surface.color } else { edge };
                if row.choices > 0 {
                    circuit::choice_bar(
                        &mut marks,
                        gauge,
                        row.choices,
                        row.choice,
                        value_ink,
                        rest,
                    );
                } else {
                    circuit::tick_bar(&mut marks, gauge, 12, row.place, value_ink, rest, true);
                }
                painter.extend(marks);
            }
            painter.text(
                egui::pos2(row_rect.right(), row_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                row_font.clone(),
                value_ink,
            );
        }
        if column.rows.len() > row_offset + rows_shown {
            let mut marks = Vec::new();
            circuit::annotation_arrow(
                &mut marks,
                egui::pos2(rect.center().x, rect.bottom() - 2.0),
                egui::pos2(rect.center().x, rect.bottom() + 8.0),
                ink,
            );
            painter.extend(marks);
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
    out.push(egui::Shape::rect_filled(hole, 0.0, ground));
    circuit::panel_frame_variant(out, hole, Weight::Hair, edge, 0);
    let inner = hole.shrink(3.0);
    circuit::trace(
        out,
        &[
            inner.left_top(),
            inner.right_top(),
            inner.right_bottom(),
            inner.left_bottom(),
            inner.left_top(),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.5),
    );
    let tick = 5.0;
    for (corner, dx, dy) in [
        (inner.left_top(), 1.0, 1.0),
        (inner.right_top(), -1.0, 1.0),
        (inner.right_bottom(), -1.0, -1.0),
        (inner.left_bottom(), 1.0, -1.0),
    ] {
        let c = corner + egui::vec2(dx * 2.0, dy * 2.0);
        circuit::trace(
            out,
            &[
                c + egui::vec2(0.0, dy * tick),
                c,
                c + egui::vec2(dx * tick, 0.0),
            ],
            Weight::Hair,
            edge,
        );
    }
}

/// Where the preamp's needle turns: low on the glass, right of centre,
/// leaving the trim ring its third on the left.
fn preamp_pivot(hole: egui::Rect) -> egui::Pos2 {
    let glass = hole.shrink(3.0);
    egui::pos2(glass.left() + glass.width() * 0.62, glass.bottom() - 7.0)
}

/// A point on an arc about `c`, at `deg` (0 is right, counter-clockwise
/// positive, screen y down).
fn on_arc(c: egui::Pos2, r: f32, deg: f32) -> egui::Pos2 {
    let a = deg.to_radians();
    egui::pos2(c.x + r * a.cos(), c.y - r * a.sin())
}

impl Stage {
    /// The section's figure on its glass. Every section draws its own;
    /// one not yet written shows its word.
    fn draw_figure(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
        glass: egui::Rect,
        level: Option<f32>,
        phase: Phase,
    ) {
        let alpha = self.alphabet();
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        match piece.kind {
            SectionKind::Preamp => {
                self.draw_preamp_figure(painter, piece, column, glass, level, phase)
            }
            _ => {
                painter.text(
                    glass.center(),
                    egui::Align2::CENTER_CENTER,
                    piece.kind.blurb(),
                    row_font,
                    alpha.edge.color,
                );
            }
        }
    }

    /// PREAMP: a trim ring with a pointer on the left; the VU arc with
    /// its needle on the right, the transfer curve faint behind it; the
    /// character's word over the arc; PHASE and COLOUR as lit buttons at
    /// the foot.
    fn draw_preamp_figure(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
        glass: egui::Rect,
        level: Option<f32>,
        phase: Phase,
    ) {
        use crate::params::console::preamp as p;
        let alpha = self.alphabet();
        let edge = alpha.edge.color;
        let ink = alpha.ink.color;
        let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
        let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let (_, id) = self.band_device(piece.index).unzip();
        let device = id.and_then(|id| self.song.device(id));
        let value = |param: u32| device.map_or(0.0, |device| device.value(param));
        let trim = value(p::TRIM);
        let iron = value(p::IRON) / 100.0;
        let steel = value(p::CHARACTER) >= 0.5;
        let flip = value(p::PHASE) >= 0.5;
        let colour = value(p::COLOUR) >= 0.5;
        let _ = column;
        let mut shapes = Vec::new();

        // The trim ring: a stepped rotary, one tick per 4 dB, the
        // pointer at the value, unity marked.
        let ring = egui::pos2(glass.left() + glass.width() * 0.18, glass.center().y + 2.0);
        let r = (glass.height() * 0.36).min(glass.width() * 0.14);
        for step in 0..=12 {
            let deg = 225.0 - step as f32 * 22.5;
            let long = step % 3 == 0;
            let a = on_arc(ring, r + 2.0, deg);
            let b = on_arc(ring, r + if long { 6.0 } else { 4.0 }, deg);
            circuit::trace(
                &mut shapes,
                &[a, b],
                Weight::Hair,
                if step == 6 { ink } else { edge },
            );
        }
        shapes.push(egui::Shape::circle_stroke(
            ring,
            r,
            egui::Stroke::new(Weight::Hair.px(), edge),
        ));
        let deg = 225.0 - (trim + 24.0) / 48.0 * 270.0;
        circuit::trace(
            &mut shapes,
            &[ring, on_arc(ring, r - 1.0, deg)],
            Weight::Heavy,
            ink,
        );
        circuit::pad(&mut shapes, ring, circuit::PAD - 1.0, ink, true);

        // The VU: an arc from −20 to +3, the top three dB in the live
        // ink, the needle from the pivot at the channel's level.
        let pivot = preamp_pivot(glass.expand(3.0));
        let radius = (glass.height() - 12.0).min(glass.width() * 0.3);
        let (start, end) = (150.0, 30.0);
        let arc: Vec<egui::Pos2> = (0..=24)
            .map(|i| on_arc(pivot, radius, start + (end - start) * i as f32 / 24.0))
            .collect();
        circuit::trace(&mut shapes, &arc, Weight::Heavy, edge.gamma_multiply(1.25));
        let deg_of = |db: f32| start + (end - start) * ((db + 20.0) / 23.0).clamp(0.0, 1.0);
        let hot: Vec<egui::Pos2> = (0..=6)
            .map(|i| {
                on_arc(
                    pivot,
                    radius,
                    deg_of(0.0) + (end - deg_of(0.0)) * i as f32 / 6.0,
                )
            })
            .collect();
        circuit::trace(&mut shapes, &hot, Weight::Heavy, alpha.live_dim.color);
        for db in [-20.0, -10.0, -5.0, 0.0, 3.0] {
            let d = deg_of(db);
            circuit::trace(
                &mut shapes,
                &[
                    on_arc(pivot, radius - 4.0, d),
                    on_arc(pivot, radius + 1.0, d),
                ],
                Weight::Hair,
                if db >= 0.0 {
                    alpha.live_dim.color
                } else {
                    edge
                },
            );
        }
        // The transfer, faint, behind the needle: the stage's curve at
        // the current drive, drawn across the arc's chord.
        let chord = egui::Rect::from_min_max(
            egui::pos2(pivot.x - radius * 0.8, pivot.y - radius * 0.95),
            egui::pos2(pivot.x + radius * 0.8, pivot.y - radius * 0.15),
        );
        let curve: Vec<egui::Pos2> = (0..=20)
            .map(|i| {
                let x = -1.0 + 2.0 * i as f32 / 20.0;
                let y = crate::console::preamp_curve::transfer(iron, steel, x);
                egui::pos2(
                    chord.left() + (x + 1.0) * 0.5 * chord.width(),
                    chord.bottom() - (y + 1.0) * 0.5 * chord.height(),
                )
            })
            .collect();
        circuit::trace(
            &mut shapes,
            &curve,
            Weight::Hair,
            if iron > 0.0 {
                alpha.live_dim.color
            } else {
                edge.gamma_multiply(0.5)
            },
        );
        let _ = live;
        // The needle.
        let db = level.map_or(-60.0, |peak| {
            if peak <= 1e-6 {
                -60.0
            } else {
                20.0 * peak.log10()
            }
        });
        let needle = deg_of(db);
        circuit::trace(
            &mut shapes,
            &[pivot, on_arc(pivot, radius - 2.0, needle)],
            Weight::Heavy,
            if db >= 0.0 { alpha.live.color } else { ink },
        );
        circuit::pad(&mut shapes, pivot, circuit::PAD, ink, true);

        // The buttons: PHASE lit red as the hardware does, COLOUR lit in
        // the live ink.
        let button =
            |shapes: &mut Vec<egui::Shape>, at: egui::Pos2, on: bool, lit: egui::Color32| {
                let square = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
                shapes.push(egui::Shape::rect_filled(
                    square,
                    0.0,
                    if on { lit } else { alpha.ground.color },
                ));
                shapes.push(egui::Shape::rect_stroke(
                    square,
                    0.0,
                    egui::Stroke::new(Weight::Hair.px(), if on { lit } else { edge }),
                    egui::StrokeKind::Inside,
                ));
            };
        button(
            &mut shapes,
            egui::pos2(glass.right() - 9.0, glass.bottom() - 8.0),
            flip,
            alpha.jeopardy_active.color,
        );
        button(
            &mut shapes,
            egui::pos2(glass.right() - 9.0, glass.bottom() - 20.0),
            colour,
            alpha.live.color,
        );
        painter.extend(shapes);

        painter.text(
            egui::pos2(glass.right() - 16.0, glass.bottom() - 8.0),
            egui::Align2::RIGHT_CENTER,
            "Ø",
            font.clone(),
            edge,
        );
        painter.text(
            egui::pos2(glass.right() - 16.0, glass.bottom() - 20.0),
            egui::Align2::RIGHT_CENTER,
            "CLR",
            font.clone(),
            edge,
        );
        block::paint(
            painter,
            egui::Id::new(("stage-preamp-character", piece.index)),
            egui::pos2(glass.right() - 5.0, glass.top() + 2.0),
            egui::Align2::RIGHT_TOP,
            block::unit::MICRO,
            if steel { "STEEL" } else { "IRON" },
            if iron > 0.0 { ink } else { edge },
        );
        painter.text(
            egui::pos2(ring.x, glass.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{trim:+.0}"),
            font,
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
