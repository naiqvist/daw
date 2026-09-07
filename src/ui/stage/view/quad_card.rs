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

/// The routing on a grid. `shown` is the operator to light, if any;
/// `px` the type size, which sets the box size with it.
pub(super) fn draw_routing(
    painter: &egui::Painter,
    rect: egui::Rect,
    p: &QuadParams,
    shown: Option<usize>,
    px: f32,
) {
    let c = palette::colours();
    let font = face::font(px);
    let algo = p.algorithm();
    let rows = (0..qp::OPS).map(|op| depth(algo, op)).max().unwrap_or(0) + 1;
    let cell_w = rect.width() / qp::OPS as f32;
    let cell_h = rect.height() / rows as f32;
    let box_w = (cell_w - 10.0).clamp(24.0, px * 10.0);
    let box_h = (cell_h - 8.0).clamp(14.0, px * 3.6);
    let centre = |op: usize| {
        egui::pos2(
            rect.left() + (op as f32 + 0.5) * cell_w,
            rect.bottom() - (depth(algo, op) as f32 + 0.5) * cell_h,
        )
    };
    let boxed = |op: usize| egui::Rect::from_center_size(centre(op), egui::vec2(box_w, box_h));
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
        // An arrowhead at the carrier.
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
                    ((box_w - 2.0) / (px * 0.56)) as usize,
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
                    ((box_w - 2.0) / (px * 0.56)) as usize,
                )
                .trim_end()
                .to_owned(),
                font.clone(),
                name_ink,
            );
        }
        // The level, as a bar along the foot of the box.
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
