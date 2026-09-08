//! QUAD in the Stage chain: the routing, on the shared instrument face.
//!
//! An FM synth's one fact that a parameter table cannot show is the
//! routing: which operator feeds which, and which are heard. The
//! picture lays the operators on a grid — one column each, in panel
//! order, modulators on the rows above the carriers they feed — and
//! draws each edge as a straight drop or an elbow, so a serial stack
//! reads as a stack and a fan-in as a fan. The same routine draws the
//! room's picture, larger.

use super::face::{self, Head, Layout, RowMark};
use super::palette;
use crate::audio::quad::QuadParams;
use crate::params::quad as qp;
use crate::ui::device::quad::{depth, op_word};
use crate::ui::stage::chain;
use eframe::egui;
use egui::Color32;

pub(super) use super::face::WIDTH;

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn algo_name(params: &QuadParams) -> &'static str {
    qp::ALGO_NAMES
        .get(params.algo.round().max(0.0) as usize)
        .copied()
        .unwrap_or("?")
}

/// Where each operator stands in the routing picture: a column and a
/// row. Carriers stand on the bottom row, left to right in panel order;
/// a modulator stands directly over the first operator it feeds, and
/// where several modulators feed one operator they fan out over it.
/// The picture is then as wide as it needs to be and no wider.
pub(super) fn routing_places(algo: qp::Algorithm) -> ([(f32, usize); qp::OPS], f32, usize) {
    // A subtree's width: the widest row of the modulators over it.
    fn width(algo: qp::Algorithm, op: usize) -> f32 {
        let feeders: Vec<usize> = algo
            .edges
            .iter()
            .filter(|(_, c)| *c == op)
            .map(|(m, _)| *m)
            // Each modulator is placed over the FIRST operator it feeds.
            .filter(|m| algo.edges.iter().find(|(mm, _)| mm == m).map(|(_, c)| *c) == Some(op))
            .collect();
        if feeders.is_empty() {
            1.0
        } else {
            feeders
                .iter()
                .map(|m| width(algo, *m))
                .sum::<f32>()
                .max(1.0)
        }
    }
    fn place(
        algo: qp::Algorithm,
        op: usize,
        left: f32,
        row: usize,
        out: &mut [(f32, usize); qp::OPS],
    ) {
        let w = width(algo, op);
        out[op] = (left + w * 0.5, row);
        let mut x = left;
        let mut feeders: Vec<usize> = algo
            .edges
            .iter()
            .filter(|(_, c)| *c == op)
            .map(|(m, _)| *m)
            .filter(|m| algo.edges.iter().find(|(mm, _)| mm == m).map(|(_, c)| *c) == Some(op))
            .collect();
        feeders.sort_unstable();
        for m in feeders {
            place(algo, m, x, row + 1, out);
            x += width(algo, m);
        }
    }
    let mut out = [(0.0, 0usize); qp::OPS];
    let mut x = 0.0;
    let mut carriers: Vec<usize> = algo.carriers.to_vec();
    carriers.sort_unstable();
    for c in carriers {
        place(algo, c, x, 0, &mut out);
        x += width(algo, c);
    }
    let rows = out.iter().map(|(_, row)| *row).max().unwrap_or(0) + 1;
    (out, x.max(1.0), rows)
}

/// The matrix, drawn as the grid it is: a row per source, a column
/// per destination, each cell filled by how much, the diagonal being
/// feedback; and a column at the right for what each operator sends
/// to the output.
fn draw_matrix(
    painter: &egui::Painter,
    rect: egui::Rect,
    p: &QuadParams,
    shown: Option<usize>,
    px: f32,
) {
    let c = palette::colours();
    let font = face::font(px);
    let n = qp::OPS as f32;
    let label = px * 2.4;
    let gap = px * 0.8;
    let cell = ((rect.width() - label - gap) / (n + 1.0))
        .min((rect.height() - label) / n)
        .max(10.0);
    let total_w = label + cell * n + gap + cell;
    let total_h = label + cell * n;
    let left = rect.center().x - total_w * 0.5;
    let top = rect.center().y - total_h * 0.5;
    let at = |col: usize, row: usize| {
        egui::Rect::from_min_size(
            egui::pos2(
                left + label + cell * col as f32,
                top + label + cell * row as f32,
            ),
            egui::vec2(cell, cell),
        )
    };
    let out_at = |row: usize| {
        egui::Rect::from_min_size(
            egui::pos2(
                left + label + cell * n + gap,
                top + label + cell * row as f32,
            ),
            egui::vec2(cell, cell),
        )
    };
    painter.text(
        egui::pos2(left + label + cell * n * 0.5, top + label * 0.35),
        egui::Align2::CENTER_CENTER,
        "INTO",
        font.clone(),
        c.label,
    );
    painter.text(
        egui::pos2(
            left + label + cell * n + gap + cell * 0.5,
            top + label * 0.35,
        ),
        egui::Align2::CENTER_CENTER,
        "OUT",
        font.clone(),
        c.label,
    );
    let number = |r: egui::Rect, amount: f32| {
        if amount > 0.005 && cell >= px * 2.2 {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                format!("{:.0}", amount * 100.0),
                font.clone(),
                if amount > 0.5 { c.ground } else { c.fg },
            );
        }
    };
    for k in 0..qp::OPS {
        let live = shown == Some(k);
        painter.text(
            egui::pos2(left + label + cell * (k as f32 + 0.5), top + label * 0.8),
            egui::Align2::CENTER_CENTER,
            format!("{}", k + 1),
            font.clone(),
            if live { c.bright } else { c.dim },
        );
        painter.text(
            egui::pos2(left + label * 0.45, top + label + cell * (k as f32 + 0.5)),
            egui::Align2::CENTER_CENTER,
            format!("OP{}", k + 1),
            font.clone(),
            if live { c.bright } else { c.fg },
        );
        for to in 0..qp::OPS {
            let r = at(to, k).shrink(1.0);
            let amount = p.matrix[k][to].clamp(0.0, 1.0);
            let ink = if k == to { c.nominal } else { c.edge };
            painter.rect_filled(r, 0.0, alpha(ink, (18.0 + 200.0 * amount) as u8));
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, alpha(ink, 90)),
                egui::StrokeKind::Inside,
            );
            number(r, amount);
        }
        let r = out_at(k).shrink(1.0);
        let amount = p.out[k].clamp(0.0, 1.0);
        painter.rect_filled(r, 0.0, alpha(c.alert, (18.0 + 200.0 * amount) as u8));
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, alpha(c.alert, 110)),
            egui::StrokeKind::Inside,
        );
        number(r, amount);
    }
}

/// The routing, drawn as the tree it is. `shown` is the operator to
/// light, if any; `px` the type size, which sets the box size with it.
pub(super) fn draw_routing(
    painter: &egui::Painter,
    rect: egui::Rect,
    p: &QuadParams,
    shown: Option<usize>,
    px: f32,
) {
    let c = palette::colours();
    let font = face::font(px);
    if p.is_matrix() {
        draw_matrix(painter, rect, p, shown, px);
        return;
    }
    let algo = p.algorithm();
    let (places, columns, rows) = routing_places(algo);
    // Boxes of one size, the picture centred in the frame.
    let box_w = (px * 9.0).min(rect.width() / columns - 8.0).max(24.0);
    let box_h = (px * 3.4).min(rect.height() / rows as f32 - 8.0).max(14.0);
    let col_w = box_w + 12.0;
    let row_h = box_h + px * 1.4;
    let total_w = col_w * columns;
    let total_h = row_h * rows as f32;
    let left = rect.center().x - total_w * 0.5;
    let bottom = rect.center().y + total_h * 0.5;
    let boxed = |op: usize| {
        let (col, row) = places[op];
        egui::Rect::from_center_size(
            egui::pos2(left + col * col_w, bottom - (row as f32 + 0.5) * row_h),
            egui::vec2(box_w, box_h),
        )
    };
    // Edges first, under the boxes: a straight drop when the two share
    // a column, an elbow halfway between when they do not.
    for (m, carrier) in algo.edges {
        let from = egui::pos2(boxed(*m).center().x, boxed(*m).bottom());
        let to = egui::pos2(boxed(*carrier).center().x, boxed(*carrier).top());
        let stroke = egui::Stroke::new(1.0, c.chassis);
        if (from.x - to.x).abs() < 0.5 {
            painter.line_segment([from, to], stroke);
        } else {
            let mid = (from.y + to.y) * 0.5;
            painter.line_segment([from, egui::pos2(from.x, mid)], stroke);
            painter.line_segment([egui::pos2(from.x, mid), egui::pos2(to.x, mid)], stroke);
            painter.line_segment([egui::pos2(to.x, mid), to], stroke);
        }
        let a = 3.0;
        painter.line_segment([to, egui::pos2(to.x - a, to.y - a)], stroke);
        painter.line_segment([to, egui::pos2(to.x + a, to.y - a)], stroke);
    }
    // Feedback: a loop off the right side of its operator.
    if p.feedback.abs() > 0.005 {
        let r = boxed(p.feedback_op());
        let loop_rect = egui::Rect::from_min_max(
            egui::pos2(r.right() + 2.0, r.top() + 3.0),
            egui::pos2(r.right() + 8.0, r.bottom() - 3.0),
        );
        painter.rect_stroke(
            loop_rect,
            0.0,
            egui::Stroke::new(1.0, c.nominal),
            egui::StrokeKind::Inside,
        );
    }
    for op in 0..qp::OPS {
        let knobs = &p.ops[op];
        let r = boxed(op);
        let carrier = algo.carriers.contains(&op);
        let live = shown == Some(op);
        let ink = if carrier { c.alert } else { c.edge };
        painter.rect_filled(r, 0.0, alpha(ink, (28.0 + 110.0 * knobs.level) as u8));
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(
                if live { 2.0 } else { 1.0 },
                if live { c.bright } else { ink },
            ),
            egui::StrokeKind::Inside,
        );
        let name_ink = if live || knobs.level > 0.02 {
            c.bright
        } else {
            c.dim
        };
        let cells = ((box_w - 2.0) / (px * 0.56)) as usize;
        if box_h >= px * 2.6 {
            painter.text(
                egui::pos2(r.center().x, r.top() + px * 0.75 + 1.0),
                egui::Align2::CENTER_CENTER,
                format!("OP {}", op + 1),
                font.clone(),
                name_ink,
            );
            painter.text(
                egui::pos2(r.center().x, r.top() + px * 1.9 + 1.0),
                egui::Align2::CENTER_CENTER,
                super::fit_cells(
                    &op_word(knobs.ratio, knobs.fixed, knobs.hz, knobs.wave),
                    cells,
                )
                .trim_end()
                .to_owned(),
                font.clone(),
                if knobs.level > 0.02 { c.fg } else { c.dim },
            );
        } else {
            painter.text(
                egui::pos2(r.center().x, r.center().y - 1.5),
                egui::Align2::CENTER_CENTER,
                super::fit_cells(
                    &format!(
                        "{} {}",
                        op + 1,
                        op_word(knobs.ratio, knobs.fixed, knobs.hz, knobs.wave)
                    ),
                    cells,
                )
                .trim_end()
                .to_owned(),
                font.clone(),
                name_ink,
            );
        }
        let foot = egui::Rect::from_min_max(
            egui::pos2(r.left() + 2.0, r.bottom() - 3.0),
            egui::pos2(
                r.left() + 2.0 + (r.width() - 4.0) * knobs.level.clamp(0.0, 1.0),
                r.bottom() - 1.0,
            ),
        );
        painter.rect_filled(foot, 0.0, if live { c.bright } else { ink });
    }
}

impl super::super::Stage {
    /// Draw the QUAD face. Returns true when the pointer asked to enter
    /// the forge; keyboard Enter continues through the normal map.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_quad_chain_card(
        &self,
        ui: &mut egui::Ui,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
    ) -> bool {
        let Some(face) = column.quad.as_ref() else {
            return false;
        };
        let Some(layout) = Layout::of(card, head_h, true) else {
            return false;
        };
        let painter = ui.painter().clone();
        let alpha_ = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        face::draw_shell(&painter, card, layout, index, selected, &alpha_);
        let p = &face.params;
        let fmode = qp::FMODE_NAMES
            .get(p.fmode.round() as usize)
            .copied()
            .unwrap_or("lp");
        let dist = qp::DIST_NAMES
            .get(p.dist.round() as usize)
            .copied()
            .unwrap_or("off");
        let mode = qp::MONO_NAMES
            .get(p.mono.round() as usize)
            .copied()
            .unwrap_or("poly");
        let head = Head {
            title: "QUAD // FM ENGINE",
            subtitle: format!(
                "{fmode} {:.1}k · {dist} x{:.1} · {mode} · uni {}",
                p.cutoff / 1000.0,
                p.drive,
                p.unison_count()
            ),
            word: algo_name(p).to_owned(),
            plate: Some("ENTER  FORGE >"),
        };
        let opened = face::draw_head(
            ui,
            layout,
            column,
            index,
            selected,
            &alpha_,
            &head,
            "stage-quad-forge",
        );
        let inner = face::frame_plot(&painter, layout.plot);
        draw_routing(&painter, inner, p, None, 9.0);
        face::draw_facts(
            &painter,
            layout.facts,
            &[
                (
                    "PITCH 1",
                    format!("{:+.0}st {:.0}/{:.0}ms", p.pitch1, p.p1_rise, p.p1_fall),
                ),
                (
                    "PITCH 2",
                    format!("{:+.0}st {:.0}/{:.0}ms", p.pitch2, p.p2_rise, p.p2_fall),
                ),
                ("FILTER", format!("{:+.1}oct {:.0}ms", p.fenv, p.fenv_dec)),
            ],
        );
        face::draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
            "stage-quad-param-cursor",
            "PARAM BANK // UP/DN  LEFT/RIGHT",
            &|_| RowMark::default(),
        );
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routing_stands_carriers_on_the_floor_and_modulators_over_what_they_feed() {
        // The serial stack: one column, four rows.
        let (places, columns, rows) = routing_places(qp::ALGORITHMS[0]);
        assert_eq!((columns, rows), (1.0, 4));
        assert!((0..4).all(|op| (places[op].0 - 0.5).abs() < 1e-6 && places[op].1 == op));
        // Everything into op 1: three columns over one carrier, centred.
        let (places, columns, rows) = routing_places(qp::ALGORITHMS[5]);
        assert_eq!((columns, rows), (3.0, 2));
        assert!((places[0].0 - 1.5).abs() < 1e-6);
        let mut tops: Vec<f32> = (1..4).map(|op| places[op].0).collect();
        tops.sort_by(f32::total_cmp);
        assert_eq!(tops, vec![0.5, 1.5, 2.5]);
        // Four carriers: one row, in panel order.
        let (places, columns, rows) = routing_places(qp::ALGORITHMS[7]);
        assert_eq!((columns, rows), (4.0, 1));
        assert!((0..4).all(|op| (places[op].0 - (op as f32 + 0.5)).abs() < 1e-6));
    }

    #[test]
    fn the_face_names_the_routing() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Quad);
        assert_eq!(
            algo_name(&chain::QuadFace::from_device(&device).params),
            "4>3>2>1"
        );
        device.set(qp::ALGO, 7.0);
        assert_eq!(
            algo_name(&chain::QuadFace::from_device(&device).params),
            "1+2+3+4"
        );
    }
}
