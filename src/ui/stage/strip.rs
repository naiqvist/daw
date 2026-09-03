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
/// PREAMP gets one extra grid-and-a-half for its meter and transformer
/// bank. It is still compact, but no longer shares TONE's exact footprint.
pub const PREAMP_W: f32 = 196.0;
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
pub fn width_of(kind: SectionKind) -> f32 {
    match kind {
        // The channel's own stages.
        SectionKind::Preamp => PREAMP_W,
        SectionKind::Tone => 212.0,
        SectionKind::Door => 176.0,
        SectionKind::Cut => 188.0,
        SectionKind::Hit => 152.0,
        SectionKind::Pump => 160.0,
        SectionKind::Grit => 172.0,
        SectionKind::Shine => 148.0,
        SectionKind::Phase => 180.0,
        SectionKind::Smear => 156.0,
        SectionKind::Ring => 164.0,
        SectionKind::Out => 192.0,
        // The ones with a picture to draw.
        SectionKind::Four => 288.0,
        SectionKind::Vca => 272.0,
        SectionKind::Split => 248.0,
        SectionKind::Drive => 232.0,
        SectionKind::Drift => 236.0,
        SectionKind::Spectra => 264.0,
        SectionKind::Echo => 244.0,
        SectionKind::Room => 228.0,
        // The desk's own.
        SectionKind::Glue => 160.0,
        SectionKind::Iron => 152.0,
        SectionKind::Ceiling => 168.0,
        SectionKind::Scope => 200.0,
        SectionKind::Tape => 236.0,
        SectionKind::Shadow => 220.0,
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
        SectionKind::Preamp => (c * 1.5, c * 0.75, c * 1.75, c),
        SectionKind::Out | SectionKind::Scope => (c, c, c, c),
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

/// The glass: under the head, inset from the walls, down to the foot.
/// The screen is most of the piece — the figure at its top, every
/// parameter beneath it.
pub fn recess(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.top() + HEAD_H + 4.0),
        egui::pos2(rect.right() - 10.0, rect.bottom() - FOOT_H),
    )
}

/// How tall a section's figure is at the top of its glass.
pub fn figure_h(kind: SectionKind) -> f32 {
    match kind {
        SectionKind::Preamp | SectionKind::Tone => FIGURE_MAX_H,
        _ => 30.0,
    }
}

/// The figure's part of the glass.
pub fn figure_rect(rect: egui::Rect, kind: SectionKind) -> egui::Rect {
    let glass = recess(rect).shrink(3.0);
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
        // A piece stands one hard step above the field. The shadow is
        // the same cut body translated once: no blur and no gradient.
        body_fill(
            &mut shapes,
            piece
                .rect
                .translate(egui::vec2(circuit::SHADOW_X, circuit::SHADOW_Y)),
            piece.kind,
            piece.notch,
            circuit::shadow_ink(alpha.ground.color),
            alpha.ground.color,
        );
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
        let selected_row = cursor.and_then(|(col, row)| (col == piece.index).then_some(row));
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));

        let mut shapes = Vec::new();
        if piece.tongue {
            tongue_fill(&mut shapes, rect, fill);
        }
        if piece.kind == SectionKind::Preamp {
            preamp_crown(
                &mut shapes,
                rect,
                if is_in {
                    alpha.well.color
                } else {
                    alpha.ground.color
                },
                edge,
            );
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
        // frame with a rune tick at each corner. An OUT piece's glass
        // is dark: the frame dim, no figure, its rows in the edge ink.
        let hole = recess(rect);
        if hole.is_positive() {
            let screen_edge = if is_in {
                edge
            } else {
                edge.gamma_multiply(0.55)
            };
            if piece.kind == SectionKind::Preamp {
                preamp_screen_frame(&mut shapes, hole, screen_edge, alpha.ground.color, fill);
            } else {
                screen_frame(&mut shapes, hole, screen_edge, alpha.ground.color);
            }
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

        // The reveal: a screen that comes under the hand redraws itself
        // top to bottom with a scan line at the edge, the way a terminal
        // painted a page. Once painted it stays.
        let glass = hole.shrink(3.0);
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
        let shown = egui::Rect::from_min_max(
            glass.min,
            egui::pos2(glass.max.x, glass.min.y + glass.height() * reveal),
        );
        let screen = painter.with_clip_rect(shown);
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
                screen.rect_filled(row_rect, 0.0, alpha.focus.color);
            }
            let value_ink = if on_row {
                alpha.ground.color
            } else if row.edited {
                alpha.focus.color
            } else {
                text_ink
            };
            let name_ink = if on_row { alpha.ground.color } else { text_ink };
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
                circuit::dot(
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
        if piece.kind != SectionKind::Preamp && column.rows.len() > row_offset + rows_shown {
            let mut marks = Vec::new();
            circuit::annotation_arrow(
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
            circuit::trace(
                &mut scan,
                &[egui::pos2(glass.left(), y), egui::pos2(glass.right(), y)],
                Weight::Heavy,
                alpha.live.color,
            );
            painter.extend(scan);
            painter.ctx().request_repaint();
        }
        if is_in && piece.kind.full_screen() {
            painter.text(
                egui::pos2(hole.right() - 6.0, hole.top() + 4.0),
                egui::Align2::RIGHT_TOP,
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
    circuit::trace(out, &outline, Weight::Hair, edge.gamma_multiply(0.72));
    for i in 0..3 {
        let x = r - 43.0 + i as f32 * 6.0;
        circuit::trace(
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
    circuit::panel_variant(
        out,
        hole,
        Some(ground),
        casing,
        Some((Weight::Heavy, edge)),
        0,
    );
    circuit::panel_frame_variant(
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
        circuit::pad(out, at, circuit::PAD - 1.0, edge, false);
    }
}

/// Where the preamp's needle turns: low on the glass, right of centre,
/// leaving the trim ring its third on the left.
fn preamp_pivot(figure: egui::Rect) -> egui::Pos2 {
    egui::pos2(figure.left() + figure.width() * 0.62, figure.bottom() - 7.0)
}

/// A point on an arc about `c`, at `deg` (0 is right, counter-clockwise
/// positive, screen y down).
fn on_arc(c: egui::Pos2, r: f32, deg: f32) -> egui::Pos2 {
    let a = deg.to_radians();
    egui::pos2(c.x + r * a.cos(), c.y - r * a.sin())
}

/// The five PREAMP parameters have five different instruments. Their
/// rectangles are authored together so the keyboard address and the
/// thing it illuminates cannot drift apart.
#[derive(Clone, Copy, Debug)]
struct PreampFace {
    meter: egui::Rect,
    trim: egui::Rect,
    iron: egui::Rect,
    character: egui::Rect,
    phase: egui::Rect,
    colour: egui::Rect,
}

impl PreampFace {
    fn controls(self) -> [(u32, egui::Rect); 5] {
        use crate::params::console::preamp as p;
        [
            (p::TRIM, self.trim),
            (p::IRON, self.iron),
            (p::CHARACTER, self.character),
            (p::PHASE, self.phase),
            (p::COLOUR, self.colour),
        ]
    }

    fn control(self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }
}

fn preamp_face(glass: egui::Rect) -> PreampFace {
    let x = glass.shrink2(egui::vec2(5.0, 0.0));
    let meter = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right(), (x.top() + FIGURE_MAX_H).min(x.bottom())),
    );
    let controls = egui::Rect::from_min_max(
        egui::pos2(x.left(), (meter.bottom() + 4.0).min(x.bottom())),
        egui::pos2(x.right(), (x.bottom() - 4.0).max(meter.bottom() + 4.0)),
    );
    let gap = 3.0;
    let usable = (controls.height() - gap * 3.0).max(4.0);
    let trim_h = usable * 0.20;
    let iron_h = usable * 0.30;
    let character_h = usable * 0.20;
    let buttons_h = (usable - trim_h - iron_h - character_h).max(1.0);
    let trim = egui::Rect::from_min_size(controls.min, egui::vec2(controls.width(), trim_h));
    let iron = egui::Rect::from_min_size(
        egui::pos2(controls.left(), trim.bottom() + gap),
        egui::vec2(controls.width(), iron_h),
    );
    let character = egui::Rect::from_min_size(
        egui::pos2(controls.left(), iron.bottom() + gap),
        egui::vec2(controls.width(), character_h),
    );
    let button_row = egui::Rect::from_min_size(
        egui::pos2(controls.left(), character.bottom() + gap),
        egui::vec2(controls.width(), buttons_h),
    );
    let button_gap = 4.0;
    let button_w = ((button_row.width() - button_gap) * 0.5).max(1.0);
    let phase = egui::Rect::from_min_size(button_row.min, egui::vec2(button_w, buttons_h));
    let colour = egui::Rect::from_min_size(
        egui::pos2(phase.right() + button_gap, button_row.top()),
        egui::vec2(button_w, buttons_h),
    );
    PreampFace {
        meter,
        trim,
        iron,
        character,
        phase,
        colour,
    }
}

/// TONE's three bands, each its own hue, so a glance says which lever
/// is which without a word being written. Warm at the bottom, violet
/// in the middle, the deck's own live cyan at the top: the order a
/// spectrum is drawn in everywhere.
const TONE_LO_INK: egui::Color32 = egui::Color32::from_rgb(255, 138, 62);
const TONE_MID_INK: egui::Color32 = egui::Color32::from_rgb(186, 122, 255);
const TONE_HI_INK: egui::Color32 = egui::Color32::from_rgb(139, 245, 255);

/// How many squares a band lights at full boost or full cut, and so
/// how much each one is worth.
const TONE_CELLS: usize = 6;
/// The face's frequency span, in Hz: what the horizontal axis means.
const TONE_LOW_HZ: f32 = 30.0;
const TONE_HIGH_HZ: f32 = 18_000.0;

/// Where `hz` falls across `field`, by octaves rather than by hertz —
/// which is how the ear reads a spectrum and how the band that moves
/// should move.
fn tone_x(field: egui::Rect, hz: f32) -> f32 {
    let span = (TONE_HIGH_HZ / TONE_LOW_HZ).log2();
    let at = (hz.max(1.0) / TONE_LOW_HZ).log2() / span;
    egui::lerp(field.x_range(), at.clamp(0.0, 1.0))
}

/// TONE's seven parameters as seven places on the glass. Authored
/// together, so the key that addresses one and the art that answers
/// cannot drift apart.
#[derive(Clone, Copy, Debug)]
struct ToneFace {
    /// The whole picture: the curve's field and the bands' ground.
    field: egui::Rect,
    /// One column per band, the middle one wherever its frequency
    /// puts it.
    columns: [egui::Rect; 3],
    /// The rail the middle band slides along, and the kill pads.
    sweep: egui::Rect,
    kills: [egui::Rect; 3],
}

impl ToneFace {
    fn controls(self) -> [(u32, egui::Rect); 7] {
        use crate::params::console::tone as p;
        [
            (p::LO, self.columns[0]),
            (p::MID, self.columns[1]),
            (p::HI, self.columns[2]),
            (p::MID_HZ, self.sweep),
            (p::KILL_LO, self.kills[0]),
            (p::KILL_MID, self.kills[1]),
            (p::KILL_HI, self.kills[2]),
        ]
    }

    fn control(self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }
}

/// Where everything stands, given the glass and where the middle band
/// has been swept to.
fn tone_face(glass: egui::Rect, mid_hz: f32) -> ToneFace {
    use crate::params::console::tone as p;
    let inner = glass.shrink2(egui::vec2(6.0, 4.0));
    let kill_h = 12.0;
    let sweep_h = 9.0;
    let field = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(
            inner.right(),
            (inner.bottom() - kill_h - sweep_h - 6.0).max(inner.top() + 20.0),
        ),
    );
    let sweep = egui::Rect::from_min_max(
        egui::pos2(inner.left(), field.bottom() + 3.0),
        egui::pos2(inner.right(), field.bottom() + 3.0 + sweep_h),
    );
    let column_w = (field.width() * 0.18).clamp(14.0, 34.0);
    let column = |hz: f32| {
        let x = tone_x(field, hz).clamp(
            field.left() + column_w * 0.5,
            field.right() - column_w * 0.5,
        );
        egui::Rect::from_min_max(
            egui::pos2(x - column_w * 0.5, field.top()),
            egui::pos2(x + column_w * 0.5, field.bottom()),
        )
    };
    let columns = [column(p::LO_HZ), column(mid_hz), column(p::HI_HZ)];
    let kills = columns.map(|c| {
        egui::Rect::from_min_max(
            egui::pos2(c.center().x - kill_h * 0.5, sweep.bottom() + 3.0),
            egui::pos2(c.center().x + kill_h * 0.5, sweep.bottom() + 3.0 + kill_h),
        )
    });
    ToneFace {
        field,
        columns,
        sweep,
        kills,
    }
}

fn mix_ink(a: egui::Color32, b: egui::Color32, amount: f32) -> egui::Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    egui::Color32::from_rgba_unmultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
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
        selected: Option<usize>,
        level: Option<f32>,
        phase: Phase,
    ) {
        let alpha = self.alphabet();
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        match piece.kind {
            SectionKind::Preamp => {
                self.draw_preamp_figure(painter, piece, column, glass, selected, level, phase)
            }
            SectionKind::Tone => self.draw_tone_figure(painter, piece, glass, selected, phase),
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

    /// TONE: three bands, three hues, and no words.
    ///
    /// Each band is a stack of SQUARES standing on the zero line —
    /// lit upward for a boost, downward for a cut, one square for
    /// every two and a half dB. The middle band's stack SLIDES along
    /// the face to wherever its frequency is set, so sweeping it is
    /// watching the band walk up the spectrum rather than watching a
    /// number climb. Under each stack is its kill pad, which fills
    /// with the band's own hue when the band is gone; and behind all
    /// three, faintly, is the response the engine is actually running,
    /// computed from the same coefficients, so what is drawn is what
    /// is heard.
    ///
    /// Nothing here is labelled. A square that is lit is a decibel
    /// that is happening.
    fn draw_tone_figure(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        glass: egui::Rect,
        selected: Option<usize>,
        phase: Phase,
    ) {
        use crate::params::console::tone as p;
        let alpha = self.alphabet();
        let edge = alpha.edge.color;
        let (_, id) = self.band_device(piece.index).unzip();
        let device = id.and_then(|id| self.song.device(id));
        let value = |param: u32| device.map_or(0.0, |device| device.value(param));
        let gains = [value(p::LO), value(p::MID), value(p::HI)];
        let kills = [
            value(p::KILL_LO) >= 0.5,
            value(p::KILL_MID) >= 0.5,
            value(p::KILL_HI) >= 0.5,
        ];
        let inks = [TONE_LO_INK, TONE_MID_INK, TONE_HI_INK];

        // The swept band walks rather than jumps.
        let mid_hz = painter.ctx().animate_value_with_time(
            egui::Id::new(("stage-tone-mid", piece.index)),
            value(p::MID_HZ),
            0.16,
        );
        let face = tone_face(glass, mid_hz.max(1.0));
        let mut shapes = Vec::new();

        // The ground: a hairline lattice, the zero line bright across
        // the middle, and a tick at each decade so the axis is a
        // spectrum and not a strip.
        circuit::lattice(
            &mut shapes,
            face.field,
            design::px(design::space::ROOM),
            edge.gamma_multiply(0.4),
        );
        let zero_y = face.field.center().y;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(face.field.left(), zero_y),
                egui::pos2(face.field.right(), zero_y),
            ],
            Weight::Hair,
            edge.gamma_multiply(1.2),
        );
        for hz in [100.0, 1_000.0, 10_000.0] {
            let x = tone_x(face.field, hz);
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(x, face.field.bottom() - 3.0),
                    egui::pos2(x, face.field.bottom()),
                ],
                Weight::Hair,
                edge.gamma_multiply(0.8),
            );
        }

        // The response the engine is running, drawn from its own
        // coefficients: the one line on the face that is measured
        // rather than set.
        let shape = device.map(|device| {
            let mut params = crate::console::SectionParams::of(crate::console::SectionKind::Tone);
            for (id, value) in &device.overrides {
                params.set(*id, *value);
            }
            crate::console::tone_curve::Shape::of(&params)
        });
        if let Some(shape) = shape {
            let sample_rate = 48_000.0;
            let curve: Vec<egui::Pos2> = (0..=48)
                .map(|i| {
                    let at = i as f32 / 48.0;
                    let hz = TONE_LOW_HZ * (TONE_HIGH_HZ / TONE_LOW_HZ).powf(at);
                    let db = crate::console::tone_curve::response_db(&shape, sample_rate, hz);
                    egui::pos2(
                        egui::lerp(face.field.x_range(), at),
                        zero_y - (db / 18.0).clamp(-1.0, 1.0) * face.field.height() * 0.46,
                    )
                })
                .collect();
            circuit::trace(&mut shapes, &curve, Weight::Hair, alpha.ink.color);
        }

        // The three stacks.
        let cell_pitch = (face.field.height() * 0.46 / TONE_CELLS as f32).min(17.0);
        let cell = (cell_pitch - 3.0)
            .max(5.0)
            .min(face.columns[0].width() - 2.0);
        for band in 0..3 {
            let column = face.columns[band];
            let ink = inks[band];
            let x = column.center().x;
            let lit = painter.ctx().animate_value_with_time(
                egui::Id::new(("stage-tone-band", piece.index, band)),
                (gains[band] / 15.0).clamp(-1.0, 1.0),
                0.14,
            );
            let killed = kills[band];
            // The post the squares stand on.
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(x, zero_y - cell_pitch * TONE_CELLS as f32),
                    egui::pos2(x, zero_y + cell_pitch * TONE_CELLS as f32),
                ],
                Weight::Hair,
                edge.gamma_multiply(if killed { 0.4 } else { 0.7 }),
            );
            for step in 1..=TONE_CELLS {
                let reach = step as f32 / TONE_CELLS as f32;
                for up in [true, false] {
                    let awake = !killed
                        && if up {
                            lit > 0.0 && reach <= lit + 0.001
                        } else {
                            lit < 0.0 && reach <= -lit + 0.001
                        };
                    let y = if up {
                        zero_y - cell_pitch * step as f32
                    } else {
                        zero_y + cell_pitch * step as f32
                    };
                    let square =
                        egui::Rect::from_center_size(egui::pos2(x, y), egui::Vec2::splat(cell));
                    if awake {
                        // A lit square is solid in the band's hue and
                        // brighter the further out it stands, with a
                        // wash of the same hue around it — the light a
                        // lit thing throws on the glass beside it.
                        shapes.push(egui::Shape::rect_filled(
                            square.expand(2.0),
                            0.0,
                            ink.gamma_multiply(0.22),
                        ));
                        shapes.push(egui::Shape::rect_filled(
                            square,
                            0.0,
                            mix_ink(ink.gamma_multiply(0.7), ink, reach),
                        ));
                    } else {
                        shapes.push(egui::Shape::rect_stroke(
                            square,
                            0.0,
                            egui::Stroke::new(
                                Weight::Hair.px(),
                                edge.gamma_multiply(if killed { 0.35 } else { 0.6 }),
                            ),
                            egui::StrokeKind::Inside,
                        ));
                    }
                }
            }
            // A killed band: the stack is struck through in its own
            // hue, which is the one mark on this face that means STOP.
            if killed {
                let top = zero_y - cell_pitch * TONE_CELLS as f32;
                let bottom = zero_y + cell_pitch * TONE_CELLS as f32;
                let arm = column.width() * 0.36;
                for (a, b) in [
                    (egui::pos2(x - arm, top), egui::pos2(x + arm, bottom)),
                    (egui::pos2(x + arm, top), egui::pos2(x - arm, bottom)),
                ] {
                    circuit::trace(&mut shapes, &[a, b], Weight::Heavy, ink);
                }
            }
        }

        // The sweep rail: the middle band's own axis, with its stack's
        // foot riding it and the two fixed bands marked as posts.
        circuit::rail(
            &mut shapes,
            egui::pos2(face.sweep.left(), face.sweep.center().y),
            egui::pos2(face.sweep.right(), face.sweep.center().y),
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            edge.gamma_multiply(0.8),
        );
        for (band, ink) in [(0usize, TONE_LO_INK), (2, TONE_HI_INK)] {
            circuit::pad(
                &mut shapes,
                egui::pos2(face.columns[band].center().x, face.sweep.center().y),
                circuit::PAD - 2.0,
                ink.gamma_multiply(0.7),
                false,
            );
        }
        let rider = egui::Rect::from_center_size(
            egui::pos2(face.columns[1].center().x, face.sweep.center().y),
            egui::vec2(9.0, face.sweep.height()),
        );
        shapes.push(egui::Shape::rect_filled(rider, 0.0, TONE_MID_INK));

        // The kill pads: hollow while the band sounds, filled in its
        // own hue the moment it does not.
        for band in 0..3 {
            let pad = face.kills[band];
            if kills[band] {
                shapes.push(egui::Shape::rect_filled(pad, 0.0, inks[band]));
            } else {
                shapes.push(egui::Shape::rect_stroke(
                    pad,
                    0.0,
                    egui::Stroke::new(Weight::Hair.px(), inks[band].gamma_multiply(0.55)),
                    egui::StrokeKind::Inside,
                ));
            }
        }
        painter.extend(shapes);

        // The cursor: the house brackets around whichever instrument
        // the keyboard is holding, and nothing else on the face bright.
        if let Some(rect) = selected.and_then(|param| face.control(param)) {
            let mut cursor = Vec::new();
            circuit::brackets(
                &mut cursor,
                rect.expand(2.0),
                6.0,
                Weight::Bold,
                motion::pulse_ink(alpha.live.color, alpha.ink.color, phase),
            );
            painter.extend(cursor);
        }
    }

    /// PREAMP: the meter remains the shared signal picture, then every
    /// parameter gets its own instrument. TRM is a small bipolar bar;
    /// IRON is the large heat bar; CHARACTER is a pair of transfer
    /// curves; PHASE is the phase glyph; COLOUR is the CLR pad. The
    /// keyboard cursor lives inside whichever instrument it addresses.
    fn draw_preamp_figure(
        &self,
        painter: &egui::Painter,
        piece: Piece,
        column: &chain::Column,
        glass: egui::Rect,
        selected: Option<usize>,
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
        let face = preamp_face(glass);
        let trim_place = painter.ctx().animate_value_with_time(
            egui::Id::new(("stage-preamp-trim", piece.index)),
            ((trim + 24.0) / 48.0).clamp(0.0, 1.0),
            0.14,
        );
        let iron_place = painter.ctx().animate_value_with_time(
            egui::Id::new(("stage-preamp-iron", piece.index)),
            iron,
            0.22,
        );
        let mut shapes = Vec::new();

        // A nested, solid meter subassembly sits inside the screen. The
        // well-coloured bezel and black inner glass make depth using only
        // plane changes and hard shadows.
        circuit::panel_variant(
            &mut shapes,
            face.meter,
            Some(alpha.well.color),
            alpha.ground.color,
            Some((Weight::Hair, edge)),
            2,
        );
        let meter_glass = face.meter.shrink(3.0);
        circuit::panel_variant(
            &mut shapes,
            meter_glass,
            Some(alpha.ground.color),
            alpha.well.color,
            Some((Weight::Hair, edge.gamma_multiply(0.62))),
            0,
        );

        // The VU: an arc from −20 to +3, the top three dB in the live
        // ink, the needle from the pivot at the channel's level. It is
        // not a sixth control: it is what comes out of the five below.
        let meter = meter_glass.shrink2(egui::vec2(4.0, 2.0));
        let pivot = preamp_pivot(meter);
        let radius = (meter.height() - 10.0).min(meter.width() * 0.31);
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
        // The transfer has its own little scope to the LEFT of the VU.
        // Keeping its right edge clear of the arc means the curve can
        // bend hard without ever becoming a second meter needle.
        let chord_right = (pivot.x - radius * 0.90 - 3.0)
            .max(meter.left() + 18.0)
            .min(meter.right());
        let chord = egui::Rect::from_min_max(
            egui::pos2(meter.left() + 2.0, meter.top() + 7.0),
            egui::pos2(chord_right, meter.bottom() - 7.0),
        );
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(chord.left(), chord.center().y),
                egui::pos2(chord.right(), chord.center().y),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.45),
        );
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(chord.center().x, chord.top()),
                egui::pos2(chord.center().x, chord.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.45),
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
                mix_ink(
                    alpha.live_dim.color,
                    alpha.jeopardy_latent.color,
                    iron * 0.65,
                )
            } else {
                edge.gamma_multiply(0.5)
            },
        );
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
            if db >= 0.0 { alpha.live.color } else { live },
        );
        circuit::pad(&mut shapes, pivot, circuit::PAD, ink, true);
        circuit::pad(
            &mut shapes,
            egui::pos2(meter_glass.right() - 7.0, meter_glass.top() + 7.0),
            circuit::PAD - 1.0,
            if db >= 0.0 {
                alpha.jeopardy_active.color
            } else {
                edge
            },
            db >= 0.0,
        );

        // TRM: deliberately little. It is bipolar about the bright
        // centre post, with only the run between unity and the smooth
        // moving cursor awake.
        circuit::panel_frame_variant(&mut shapes, face.trim, Weight::Hair, edge, 3);
        let trim_bar = egui::Rect::from_min_max(
            egui::pos2(face.trim.left() + 30.0, face.trim.center().y - 5.0),
            egui::pos2(face.trim.right() - 6.0, face.trim.center().y + 5.0),
        );
        let trim_segments = 17usize;
        for i in 0..trim_segments {
            let t = i as f32 / (trim_segments - 1) as f32;
            let x = egui::lerp(trim_bar.x_range(), t);
            let from = trim_place.min(0.5);
            let to = trim_place.max(0.5);
            let awake = t >= from - 0.001 && t <= to + 0.001;
            let cursor = (t - trim_place).abs() < 0.5 / (trim_segments - 1) as f32;
            let h = if cursor {
                trim_bar.height()
            } else if i % 4 == 0 {
                7.0
            } else {
                4.0
            };
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(x, trim_bar.center().y - h * 0.5),
                    egui::pos2(x, trim_bar.center().y + h * 0.5),
                ],
                if cursor { Weight::Heavy } else { Weight::Hair },
                if awake {
                    ink
                } else {
                    edge.gamma_multiply(0.65)
                },
            );
        }
        let unity_x = egui::lerp(trim_bar.x_range(), 0.5);
        circuit::pad(
            &mut shapes,
            egui::pos2(unity_x, trim_bar.center().y),
            circuit::PAD - 2.0,
            alpha.focus.color,
            true,
        );

        // IRON: a larger bank. More of it wakes with drive and every
        // live segment moves from the bone ink toward the alphabet's
        // hot hue as the stage is leaned on.
        circuit::panel_variant(
            &mut shapes,
            face.iron,
            Some(alpha.well.color),
            alpha.ground.color,
            Some((Weight::Hair, edge)),
            1,
        );
        let iron_bar = egui::Rect::from_min_max(
            egui::pos2(face.iron.left() + 38.0, face.iron.top() + 6.0),
            egui::pos2(face.iron.right() - 6.0, face.iron.bottom() - 6.0),
        );
        let iron_segments = 14usize;
        for i in 0..iron_segments {
            let n = iron_segments as f32;
            let t0 = i as f32 / n;
            let t1 = (i + 1) as f32 / n;
            let at = (i as f32 + 0.5) / n;
            let cell = egui::Rect::from_min_max(
                egui::pos2(
                    egui::lerp(iron_bar.x_range(), t0),
                    iron_bar.bottom() - iron_bar.height() * (0.45 + at * 0.55),
                ),
                egui::pos2(egui::lerp(iron_bar.x_range(), t1) - 1.0, iron_bar.bottom()),
            );
            let awake = at <= iron_place;
            let warmth = (iron_place * 0.78 + at * 0.22).clamp(0.0, 1.0);
            shapes.push(egui::Shape::rect_filled(
                cell,
                0.0,
                if awake {
                    mix_ink(ink, alpha.jeopardy_active.color, warmth)
                } else {
                    edge.gamma_multiply(0.55)
                },
            ));
        }

        // CHARACTER: not another bar. The two actual transfer families
        // face each other; the selected path is the readable one.
        circuit::panel_frame_variant(&mut shapes, face.character, Weight::Hair, edge, 2);
        let split_x = face.character.center().x;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(split_x, face.character.top() + 3.0),
                egui::pos2(split_x, face.character.bottom() - 3.0),
            ],
            Weight::Hair,
            edge,
        );
        for (right, is_steel) in [(false, false), (true, true)] {
            let half = if right {
                egui::Rect::from_min_max(
                    egui::pos2(split_x, face.character.top()),
                    face.character.max,
                )
            } else {
                egui::Rect::from_min_max(
                    face.character.min,
                    egui::pos2(split_x, face.character.bottom()),
                )
            };
            let chosen = steel == is_steel;
            let plot = egui::Rect::from_min_max(
                egui::pos2(half.left() + 20.0, half.top() + 4.0),
                egui::pos2(half.right() - 5.0, half.bottom() - 4.0),
            );
            let curve: Vec<egui::Pos2> = (0..=12)
                .map(|i| {
                    let x = -1.0 + 2.0 * i as f32 / 12.0;
                    let y = crate::console::preamp_curve::transfer(0.72, is_steel, x);
                    egui::pos2(
                        plot.left() + (x + 1.0) * 0.5 * plot.width(),
                        plot.bottom() - (y + 1.0) * 0.5 * plot.height(),
                    )
                })
                .collect();
            circuit::trace(
                &mut shapes,
                &curve,
                if chosen { Weight::Heavy } else { Weight::Hair },
                if chosen {
                    ink
                } else {
                    edge.gamma_multiply(0.7)
                },
            );
            circuit::pad(
                &mut shapes,
                egui::pos2(half.left() + 12.0, half.center().y),
                circuit::PAD - 1.0,
                if chosen { ink } else { edge },
                chosen,
            );
        }

        // PHASE and COLOUR are switches, so they are buttons and
        // nothing else: the existing phase glyph, and CLR.
        let button = |shapes: &mut Vec<egui::Shape>,
                      rect: egui::Rect,
                      on: bool,
                      lit: egui::Color32,
                      variant: u8| {
            circuit::panel_variant(
                shapes,
                rect,
                Some(if on { lit } else { alpha.ground.color }),
                alpha.ground.color,
                Some((Weight::Hair, if on { lit } else { edge })),
                variant,
            );
            circuit::pad(
                shapes,
                egui::pos2(rect.right() - 7.0, rect.top() + 7.0),
                circuit::PAD - 1.0,
                if on { alpha.ground.color } else { edge },
                on,
            );
        };
        button(
            &mut shapes,
            face.phase,
            flip,
            alpha.jeopardy_active.color,
            3,
        );
        button(&mut shapes, face.colour, colour, alpha.live.color, 0);
        painter.extend(shapes);

        let button_ink = |on: bool| if on { alpha.ground.color } else { ink };
        let meter_font = egui::FontId::monospace((font.size * 0.72).max(7.0));
        painter.text(
            egui::pos2(meter_glass.left() + 8.0, meter_glass.top() + 4.0),
            egui::Align2::LEFT_TOP,
            "XFR",
            meter_font.clone(),
            edge,
        );
        painter.text(
            egui::pos2(meter_glass.right() - 13.0, meter_glass.top() + 3.0),
            egui::Align2::RIGHT_TOP,
            "PK",
            meter_font.clone(),
            if db >= 0.0 {
                alpha.jeopardy_active.color
            } else {
                edge
            },
        );
        painter.text(
            egui::pos2(pivot.x, meter_glass.top() + 3.0),
            egui::Align2::CENTER_TOP,
            "VU / dB",
            meter_font,
            edge,
        );
        painter.text(
            egui::pos2(face.trim.left() + 6.0, face.trim.center().y),
            egui::Align2::LEFT_CENTER,
            "TRM",
            font.clone(),
            ink,
        );
        painter.text(
            egui::pos2(face.iron.left() + 6.0, face.iron.center().y),
            egui::Align2::LEFT_CENTER,
            "IRON",
            font.clone(),
            mix_ink(ink, alpha.jeopardy_active.color, iron_place),
        );
        painter.text(
            egui::pos2(face.character.left() + 7.0, face.character.center().y),
            egui::Align2::LEFT_CENTER,
            "FE",
            font.clone(),
            if steel { edge } else { ink },
        );
        painter.text(
            egui::pos2(face.character.center().x + 7.0, face.character.center().y),
            egui::Align2::LEFT_CENTER,
            "ST",
            font.clone(),
            if steel { ink } else { edge },
        );
        painter.text(
            face.phase.center(),
            egui::Align2::CENTER_CENTER,
            "Ø",
            font.clone(),
            button_ink(flip),
        );
        painter.text(
            face.colour.center(),
            egui::Align2::CENTER_CENTER,
            "CLR",
            font.clone(),
            button_ink(colour),
        );

        // The cursor is not a sixth row below the drawing. Four bright
        // corners sit just inside the addressed instrument itself.
        if let Some(rect) = selected.and_then(|param| face.control(param)) {
            let mut cursor = Vec::new();
            circuit::brackets(
                &mut cursor,
                rect.shrink(2.0),
                5.0,
                Weight::Bold,
                alpha.focus.color,
            );
            painter.extend(cursor);
        }
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
            assert!(w >= 140.0 && w <= 300.0, "{kind:?} is {w} wide");
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

    /// Every TONE parameter has a place on the glass, they do not
    /// overlap, and the middle band's stack stands where its frequency
    /// says it does.
    #[test]
    fn every_tone_parameter_has_one_instrument() {
        use crate::params::console::tone as p;
        let glass = egui::Rect::from_min_size(egui::pos2(20.0, 60.0), egui::vec2(200.0, 190.0));
        let face = tone_face(glass, 1_000.0);
        let controls = face.controls();
        for (_, rect) in controls {
            assert!(rect.is_positive(), "an instrument has no room");
            assert!(glass.contains_rect(rect), "an instrument left the glass");
        }
        for table in SectionKind::Tone.table() {
            assert!(
                face.control(table.id as usize).is_some(),
                "{} has no instrument",
                table.name
            );
        }
        // The three stacks stand apart, low to high, left to right.
        assert!(face.columns[0].right() <= face.columns[1].left());
        assert!(face.columns[1].right() <= face.columns[2].left());
        // And the middle one walks when it is swept.
        let low = tone_face(glass, 250.0).columns[1].center().x;
        let high = tone_face(glass, 5_000.0).columns[1].center().x;
        assert!(
            high > low + 20.0,
            "the swept band did not move: {low} to {high}"
        );
        // Each kill pad stands under its own band.
        for band in 0..3 {
            assert!(
                (face.kills[band].center().x - face.columns[band].center().x).abs() < 1.0,
                "kill {band} is not under its band"
            );
        }
        let _ = p::MID_HZ;
    }

    /// The face's axis is read in octaves, so a decade takes the same
    /// room wherever it sits.
    #[test]
    fn the_tone_axis_is_octaves_not_hertz() {
        let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 100.0));
        let a = tone_x(field, 100.0) - tone_x(field, 50.0);
        let b = tone_x(field, 8_000.0) - tone_x(field, 4_000.0);
        assert!((a - b).abs() < 0.5, "an octave is {a} here and {b} there");
        assert!(tone_x(field, 20.0) >= field.left());
        assert!(tone_x(field, 30_000.0) <= field.right());
    }

    /// The preamp's chassis is its own — a width and a set of corner
    /// cuts that no neighbour shares. It is no longer the WIDEST of
    /// them: every section is sized by what it has to show, and TONE
    /// has a spectrum to lay three bands across.
    #[test]
    fn preamp_owns_a_distinct_chassis_footprint() {
        assert_ne!(width_of(SectionKind::Preamp), width_of(SectionKind::Tone));
        assert_ne!(width_of(SectionKind::Preamp), width_of(SectionKind::Door));
        assert_ne!(cuts(SectionKind::Preamp), cuts(SectionKind::Tone));
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

    /// PREAMP has no fallback table: every table position maps to one
    /// bespoke instrument, once, and the meter maps to none.
    #[test]
    fn every_preamp_parameter_has_one_instrument() {
        use crate::params::console::preamp as p;
        let glass = egui::Rect::from_min_size(egui::pos2(40.0, 500.0), egui::vec2(158.0, 200.0));
        let face = preamp_face(glass);
        let controls = face.controls();
        assert_eq!(
            controls.map(|(id, _)| id),
            [p::TRIM, p::IRON, p::CHARACTER, p::PHASE, p::COLOUR]
        );
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert_eq!(face.control(*id as usize), Some(*rect));
            assert!(glass.contains_rect(*rect), "parameter {id} left the glass");
            assert!(rect.is_positive(), "parameter {id} lost its instrument");
            assert!(
                !face.meter.intersects(*rect),
                "parameter {id} invaded the meter"
            );
            for (other_id, other) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other),
                    "parameters {id} and {other_id} overlap"
                );
            }
        }
    }

    /// The IRON bank is deliberately the largest single-parameter
    /// instrument, while the two binary parameters share the last row.
    #[test]
    fn iron_owns_the_big_bar_and_the_switches_share_the_foot() {
        let glass = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(158.0, 200.0));
        let face = preamp_face(glass);
        assert!(face.iron.height() > face.trim.height());
        assert!(face.iron.height() > face.character.height());
        assert_eq!(face.phase.y_range(), face.colour.y_range());
        assert!(face.phase.right() < face.colour.left());
    }
}
