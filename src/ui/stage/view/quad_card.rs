//! QUAD in the Stage chain.
//!
//! An FM synth's one fact that a parameter table cannot show is the
//! routing: which operator feeds which, and which are heard. This wider
//! face keeps the scrolling parameter rail on its right and gives its
//! left to that picture — operators as boxes, modulators stacked over
//! the carriers they feed, each box carrying its ratio and its level —
//! with the two pitch envelopes and the filter's read out beneath.

use super::{chassis, palette};
use crate::PROFONT;
use crate::design::codex::Sign;
use crate::design::kit::Weight;
use crate::params::quad as qp;
use crate::ui::chrome;
use crate::ui::device::quad::depth;
use crate::ui::stage::chain::{self, QuadFace};
use eframe::egui;

pub(super) const WIDTH: f32 = 478.0;

const PAD: f32 = 9.0;
const GAP: f32 = 10.0;
const PARAM_W: f32 = 188.0;
const PARAM_HEAD_H: f32 = 19.0;
const FACT_H: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    head: egui::Rect,
    visual: egui::Rect,
    plot: egui::Rect,
    facts: egui::Rect,
    params: egui::Rect,
}

impl Layout {
    fn of(card: egui::Rect, head_h: f32) -> Option<Self> {
        if card.width() < PARAM_W + PAD * 2.0 + 80.0 || card.height() < head_h + FACT_H + 30.0 {
            return None;
        }
        let head = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), head_h));
        let body = egui::Rect::from_min_max(
            egui::pos2(card.left() + PAD, head.bottom() + PAD),
            egui::pos2(card.right() - PAD, card.bottom() - PAD),
        );
        let params =
            egui::Rect::from_min_max(egui::pos2(body.right() - PARAM_W, body.top()), body.max);
        let visual =
            egui::Rect::from_min_max(body.min, egui::pos2(params.left() - GAP, body.bottom()));
        let facts = egui::Rect::from_min_max(
            egui::pos2(visual.left(), visual.bottom() - FACT_H),
            visual.max,
        );
        let plot =
            egui::Rect::from_min_max(visual.min, egui::pos2(visual.right(), facts.top() - 6.0));
        Some(Self {
            head,
            visual,
            plot,
            facts,
            params,
        })
    }
}

fn algo_name(face: &QuadFace) -> &'static str {
    qp::ALGO_NAMES
        .get(face.params.algo.round().max(0.0) as usize)
        .copied()
        .unwrap_or("?")
}

fn draw_header(
    painter: &egui::Painter,
    layout: Layout,
    face: &QuadFace,
    column: &chain::Column,
    selected: bool,
    alpha: &crate::design::Alphabet,
) {
    let colours = palette::colours();
    let family_ink = if column.bypassed {
        alpha.edge.color
    } else {
        alpha.ink.color
    };
    let font = egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into()));
    let mut seal = Vec::new();
    Sign::Seal(crate::ui::stage::browser::family_mark(column.family)).paint(
        &mut seal,
        egui::Rect::from_center_size(
            egui::pos2(layout.head.left() + 16.0, layout.head.center().y),
            egui::Vec2::splat(19.0),
        ),
        Weight::Hair,
        family_ink,
    );
    for shape in seal {
        painter.add(shape);
    }
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.top() + 5.0),
        egui::Align2::LEFT_TOP,
        "QUAD // FM ENGINE",
        font.clone(),
        if selected {
            colours.bright
        } else {
            colours.dir
        },
    );
    let p = &face.params;
    let fmode = qp::FMODE_NAMES
        .get(p.fmode.round() as usize)
        .copied()
        .unwrap_or("lp");
    let dist = qp::DIST_NAMES
        .get(p.dist.round() as usize)
        .copied()
        .unwrap_or("off");
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.bottom() - 5.0),
        egui::Align2::LEFT_BOTTOM,
        super::fit_cells(
            &format!(
                "fb {:.0}% · {fmode} {:.1}k · {dist} x{:.1} · vel {:.0}%",
                p.feedback * 100.0,
                p.cutoff / 1000.0,
                p.drive,
                p.velocity * 100.0
            ),
            44,
        ),
        font,
        family_ink,
    );
    painter.text(
        egui::pos2(layout.head.right() - 10.0, layout.head.center().y),
        egui::Align2::RIGHT_CENTER,
        algo_name(face),
        egui::FontId::new(15.0, egui::FontFamily::Name(PROFONT.into())),
        colours.alert,
    );
}

/// The routing: operators as boxes, modulators over their carriers.
fn draw_plot(painter: &egui::Painter, rect: egui::Rect, face: &QuadFace) {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.panel);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c.rule),
        egui::StrokeKind::Inside,
    );
    let body = rect.shrink(8.0);
    if body.width() < 60.0 || body.height() < 24.0 {
        return;
    }
    let font = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
    let p = &face.params;
    let algo = p.algorithm();
    let rows = (0..qp::OPS).map(|op| depth(algo, op)).max().unwrap_or(0) + 1;
    let box_w = (body.width() / qp::OPS as f32 - 8.0).max(16.0);
    let box_h = ((body.height() - 6.0 * (rows as f32 - 1.0)) / rows as f32)
        .min(30.0)
        .max(12.0);
    let centre = |op: usize| {
        let x = body.left() + (op as f32 + 0.5) * body.width() / qp::OPS as f32;
        let y = body.bottom() - box_h * 0.5 - depth(algo, op) as f32 * (box_h + 6.0);
        egui::pos2(x, y)
    };
    for (m, carrier) in algo.edges {
        painter.line_segment(
            [centre(*m), centre(*carrier)],
            egui::Stroke::new(1.0, c.chassis),
        );
    }
    // Feedback: a loop drawn beside the top operator.
    if p.feedback > 0.005 {
        let at = centre(qp::OPS - 1);
        let r = egui::Rect::from_center_size(
            egui::pos2(at.x + box_w * 0.5 + 6.0, at.y),
            egui::vec2(8.0, box_h * 0.8),
        );
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, c.nominal),
            egui::StrokeKind::Inside,
        );
    }
    for op in 0..qp::OPS {
        let knobs = &p.ops[op];
        let r = egui::Rect::from_center_size(centre(op), egui::vec2(box_w, box_h));
        let carrier = algo.carriers.contains(&op);
        let ink = if carrier { c.alert } else { c.edge };
        painter.rect_filled(
            r,
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                ink.r(),
                ink.g(),
                ink.b(),
                (30.0 + 120.0 * knobs.level) as u8,
            ),
        );
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, ink),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(r.center().x, r.center().y - 2.0),
            egui::Align2::CENTER_CENTER,
            format!("{}:{:.2}", op + 1, knobs.ratio),
            font.clone(),
            if knobs.level > 0.02 { c.bright } else { c.dim },
        );
        // The level, as a bar along the foot of the box.
        let foot = egui::Rect::from_min_max(
            egui::pos2(r.left() + 2.0, r.bottom() - 3.0),
            egui::pos2(
                r.left() + 2.0 + (r.width() - 4.0) * knobs.level.clamp(0.0, 1.0),
                r.bottom() - 1.0,
            ),
        );
        painter.rect_filled(foot, 0.0, c.bright);
    }
}

fn draw_facts(painter: &egui::Painter, rect: egui::Rect, face: &QuadFace) {
    let colours = palette::colours();
    let font = egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into()));
    let p = &face.params;
    let facts = [
        (
            "PITCH 1",
            format!("{:+.0}st {:.0}/{:.0}ms", p.pitch1, p.p1_rise, p.p1_fall),
        ),
        (
            "PITCH 2",
            format!("{:+.0}st {:.0}/{:.0}ms", p.pitch2, p.p2_rise, p.p2_fall),
        ),
        ("FILTER", format!("{:+.1}oct {:.0}ms", p.fenv, p.fenv_dec)),
    ];
    let w = rect.width() / facts.len() as f32;
    for (i, (label, value)) in facts.iter().enumerate() {
        let x = rect.left() + w * i as f32;
        painter.text(
            egui::pos2(x, rect.top() + 4.0),
            egui::Align2::LEFT_TOP,
            *label,
            font.clone(),
            colours.label,
        );
        painter.text(
            egui::pos2(x, rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            super::fit_cells(value, ((w / 5.5) as usize).max(4)),
            font.clone(),
            colours.fg,
        );
    }
}

fn draw_params(
    painter: &egui::Painter,
    rect: egui::Rect,
    column: &chain::Column,
    index: usize,
    cursor: Option<(usize, usize)>,
    row_offset: usize,
    rows_shown: usize,
) {
    let c = palette::colours();
    chassis::instrument_rail(painter, rect);
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PARAM_HEAD_H));
    painter.text(
        egui::pos2(head.left() + 6.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        "PARAM BANK // UP/DN  LEFT/RIGHT",
        egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into())),
        c.label,
    );
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    if rows_shown == 0 || body.height() <= 1.0 {
        return;
    }
    let pitch = (body.height() / rows_shown as f32).min(24.0).max(13.0);
    let font = egui::FontId::new(9.5, egui::FontFamily::Name(PROFONT.into()));
    let cell_w = painter
        .layout_no_wrap("M".to_owned(), font.clone(), c.fg)
        .rect
        .width()
        .max(1.0);
    for line in 0..rows_shown {
        let Some(row) = column.rows.get(row_offset + line) else {
            break;
        };
        let row_index = row_offset + line;
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + 4.0, body.top() + line as f32 * pitch),
            egui::vec2(body.width() - 8.0, pitch),
        );
        let selected = cursor == Some((index, row_index));
        if selected {
            painter.rect_filled(row_rect, 0.0, c.select);
            crate::ui::nav_cursor::claim(
                painter,
                ("stage-quad-param-cursor", index, row_index),
                row_rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
        }
        let ink = if selected {
            c.bright
        } else if row.edited {
            c.alert
        } else {
            c.fg
        };
        let value_cells = 9usize;
        let name_cells = ((row_rect.width() / cell_w).floor() as usize)
            .saturating_sub(value_cells + 2)
            .max(3);
        painter.text(
            egui::pos2(row_rect.left() + 4.0, row_rect.center().y - 1.0),
            egui::Align2::LEFT_CENTER,
            super::fit_cells(&row.name, name_cells),
            font.clone(),
            ink,
        );
        painter.text(
            egui::pos2(row_rect.right() - 4.0, row_rect.center().y - 1.0),
            egui::Align2::RIGHT_CENTER,
            super::fit_cells(&row.value, value_cells),
            font.clone(),
            ink,
        );
        let rail = egui::Rect::from_min_max(
            egui::pos2(row_rect.left() + 4.0, row_rect.bottom() - 3.0),
            egui::pos2(row_rect.right() - 4.0, row_rect.bottom() - 2.0),
        );
        painter.rect_filled(rail, 0.0, c.rule);
        painter.rect_filled(
            egui::Rect::from_min_max(
                rail.min,
                egui::pos2(
                    rail.left() + rail.width() * row.place.clamp(0.0, 1.0),
                    rail.bottom(),
                ),
            ),
            0.0,
            if selected { c.bright } else { c.chassis },
        );
    }
}

impl super::super::Stage {
    /// Draw the QUAD face.
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
    ) {
        let Some(face) = column.quad.as_ref() else {
            return;
        };
        let Some(layout) = Layout::of(card, head_h) else {
            return;
        };
        let painter = ui.painter().clone();
        let alpha = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        let mut shell = Vec::new();
        chrome::panel_variant(
            &mut shell,
            card,
            Some(alpha.surface.color),
            alpha.ground.color,
            Some((
                if selected {
                    Weight::Heavy
                } else {
                    Weight::Hair
                },
                if selected {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            )),
            index as u8,
        );
        chrome::trace(
            &mut shell,
            &[
                egui::pos2(card.left() + chrome::CHAMFER, layout.head.bottom()),
                egui::pos2(card.right() - chrome::CHAMFER, layout.head.bottom()),
            ],
            Weight::Hair,
            alpha.edge.color,
        );
        for point in [
            egui::pos2(card.center().x, card.top()),
            egui::pos2(card.center().x, card.bottom()),
            egui::pos2(card.left(), layout.head.bottom() - 5.0),
            egui::pos2(card.right(), layout.head.bottom() - 5.0),
        ] {
            chrome::pad(&mut shell, point, chrome::PAD, alpha.ink.color, true);
        }
        for shape in shell {
            painter.add(shape);
        }
        draw_header(&painter, layout, face, column, selected, &alpha);
        draw_plot(&painter, layout.plot, face);
        draw_facts(&painter, layout.facts, face);
        draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quad_face_reserves_a_routing_picture_and_a_parameter_rail() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH, 236.0));
        let layout = Layout::of(card, 40.0).expect("a full QUAD card");
        assert!(layout.plot.width() > layout.params.width());
        assert!(layout.plot.height() > FACT_H);
        assert_eq!(layout.params.width(), PARAM_W);
        assert!(!layout.plot.intersects(layout.params));
    }

    #[test]
    fn the_face_names_the_routing() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Quad);
        assert_eq!(algo_name(&QuadFace::from_device(&device)), "4>3>2>1");
        device.set(qp::ALGO, 7.0);
        assert_eq!(algo_name(&QuadFace::from_device(&device)), "1+2+3+4");
    }
}
