//! QUAD's device card — the four-operator FM synth.
//!
//! Ids, ranges and the algorithm table come from [`crate::params::quad`].
//! The hero draws the routing shape: the operators as boxes, modulators
//! above the carriers they feed, from the same table the voices route
//! by. Seven pages of cells: one per operator, the routing and pitch
//! envelopes, the filter, and the output stage.

use crate::params::quad as qp;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const fn op_row(op: usize) -> [u32; 7] {
    [
        qp::op_param(op, qp::RATIO),
        qp::op_param(op, qp::FINE),
        qp::op_param(op, qp::LEVEL_OP),
        qp::op_param(op, qp::ATTACK),
        qp::op_param(op, qp::DECAY),
        qp::op_param(op, qp::SUSTAIN),
        qp::op_param(op, qp::RELEASE),
    ]
}
const OP1: [u32; 7] = op_row(0);
const OP2: [u32; 7] = op_row(1);
const OP3: [u32; 7] = op_row(2);
const OP4: [u32; 7] = op_row(3);

const PAGES: [(&str, &[u32]); 7] = [
    ("op 1", &OP1),
    ("op 2", &OP2),
    ("op 3", &OP3),
    ("op 4", &OP4),
    (
        "route",
        &[
            qp::ALGO,
            qp::FEEDBACK,
            qp::PITCH1,
            qp::PITCH1_RISE,
            qp::PITCH1_FALL,
            qp::PITCH2,
            qp::PITCH2_RISE,
            qp::PITCH2_FALL,
        ],
    ),
    (
        "filter",
        &[
            qp::FMODE,
            qp::CUTOFF,
            qp::RESO,
            qp::FENV,
            qp::FENV_ATT,
            qp::FENV_DEC,
            qp::KEYTRACK,
        ],
    ),
    ("out", &[qp::DIST, qp::DRIVE, qp::VELOCITY, qp::LEVEL]),
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
const HEADER_ROWS: usize = 1;
const COUNT: usize = 47;

pub fn pages() -> usize {
    PAGES.len()
}

/// Forty-seven cells: past serde's array limit, and never persisted —
/// the rack rebuilds it from the engine's knobs every frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadUi {
    pub norms: [f32; COUNT],
}

impl Default for QuadUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(qp::TABLE, id).default)
    }
}

impl QuadUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let mut norms = [0.0; COUNT];
        for (i, slot) in norms.iter_mut().enumerate() {
            let id = i as u32;
            *slot = quad_norm(id, get(id));
        }
        Self { norms }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        self.norms.get_mut(param as usize)
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    fn value(&self, id: u32) -> f32 {
        quad_value(id, self.norms.get(id as usize).copied().unwrap_or(0.0))
    }
}

fn percent_row(param: u32) -> bool {
    matches!(
        qp::op_of(param),
        Some((_, qp::LEVEL_OP)) | Some((_, qp::SUSTAIN))
    ) || matches!(param, qp::FEEDBACK | qp::KEYTRACK | qp::VELOCITY)
}

fn param_of(id: u32) -> Param {
    use crate::ui::device::{Mapping, Unit};
    let def = params::def(qp::TABLE, id);
    let linear = |name, unit| {
        Param::new(
            name,
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            unit,
        )
    };
    let log = |name, unit| {
        Param::new(
            name,
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            unit,
        )
    };
    let p = if let Some((_, field)) = qp::op_of(id) {
        match field {
            qp::RATIO => log("ratio", Unit::Ratio),
            qp::FINE => linear("fine", Unit::Cents).bipolar(),
            qp::LEVEL_OP => Param::percent("level"),
            qp::ATTACK => linear("attack", Unit::Ms),
            qp::DECAY => log("decay", Unit::Ms),
            qp::SUSTAIN => Param::percent("sustain"),
            _ => log("release", Unit::Ms),
        }
    } else {
        match id {
            qp::ALGO => Param::choice("algo", qp::ALGO_NAMES),
            qp::FEEDBACK => Param::percent("feedback"),
            qp::PITCH1 => linear("pitch 1", Unit::Semitones).bipolar(),
            qp::PITCH1_RISE => linear("rise 1", Unit::Ms),
            qp::PITCH1_FALL => log("fall 1", Unit::Ms),
            qp::PITCH2 => linear("pitch 2", Unit::Semitones).bipolar(),
            qp::PITCH2_RISE => linear("rise 2", Unit::Ms),
            qp::PITCH2_FALL => log("fall 2", Unit::Ms),
            qp::FMODE => Param::choice("filter", qp::FMODE_NAMES),
            qp::CUTOFF => log("cutoff", Unit::Hz),
            qp::RESO => log("reso", Unit::Plain),
            qp::FENV => linear("f env", Unit::Plain).bipolar(),
            qp::FENV_ATT => linear("f att", Unit::Ms),
            qp::FENV_DEC => log("f dec", Unit::Ms),
            qp::KEYTRACK => Param::percent("keytrack"),
            qp::DIST => Param::choice("dist", qp::DIST_NAMES),
            qp::DRIVE => log("drive", Unit::Ratio),
            qp::VELOCITY => Param::percent("velocity"),
            _ => Param::percent("level"),
        }
    };
    p.with_default(shown(id, def.default))
}

fn shown(param: u32, value: f32) -> f32 {
    if param == qp::LEVEL {
        value / qp::LEVEL_MAX * 100.0
    } else if percent_row(param) {
        value * 100.0
    } else {
        value
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = if param == qp::LEVEL {
        value / 100.0 * qp::LEVEL_MAX
    } else if percent_row(param) {
        value / 100.0
    } else {
        value
    };
    params::def(qp::TABLE, param).clamp(raw)
}

pub fn quad_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn quad_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn quad_is_discrete(param: u32) -> bool {
    matches!(param, qp::ALGO | qp::FMODE | qp::DIST)
}

pub fn quad_is_log(param: u32) -> bool {
    matches!(
        qp::op_of(param),
        Some((_, qp::RATIO)) | Some((_, qp::DECAY)) | Some((_, qp::RELEASE))
    ) || matches!(
        param,
        qp::PITCH1_FALL | qp::PITCH2_FALL | qp::CUTOFF | qp::RESO | qp::FENV_DEC | qp::DRIVE
    )
}

pub fn quad_edits(state: &QuadUi) -> Vec<ParamEdit> {
    let mut state = *state;
    qp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: quad_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    PAGES
        .iter()
        .map(|(_, row)| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// Where each operator stands in the routing picture: its row is how
/// many modulators feed down from it to a carrier, its column keeps
/// the operators in panel order. Shared with the Stage's card.
pub fn depth(algo: qp::Algorithm, op: usize) -> usize {
    if algo.carriers.contains(&op) {
        return 0;
    }
    algo.edges
        .iter()
        .filter(|(m, _)| *m == op)
        .map(|(_, c)| depth(algo, *c) + 1)
        .max()
        .unwrap_or(0)
}

pub fn quad_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut QuadUi,
    page: &mut usize,
    voices: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);
    card::card_sized(ui, theme, "quad", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(
                ui,
                theme,
                None,
                0.0,
                HEADER_ROWS,
                FOOTER_ROWS,
                |ui, region| match region {
                    poly_widgets::CurveRegion::Header => rail(ui, theme, page),
                    poly_widgets::CurveRegion::Plot => routing(ui, theme, state, voices),
                    poly_widgets::CurveRegion::Footer => {
                        footer(ui, theme, state, *page, &mut edits);
                    }
                },
            );
        });
    });
    edits
}

fn rail(ui: &mut egui::Ui, theme: &Theme, page: &mut usize) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    *page = (*page).min(PAGES.len() - 1);
    let width = rect.width() / PAGES.len() as f32;
    for (index, (label, _)) in PAGES.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            egui::pos2(rect.left() + width * index as f32, rect.top()),
            egui::vec2(width, rect.height()),
        );
        let response = ui
            .interact(
                tab.shrink2(egui::vec2(1.0, 0.0)),
                ui.id().with(("quad_page", index)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if response.clicked() {
            *page = index;
        }
        let live = index == *page;
        if live {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.role_shape_dim);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.surface_raised);
        }
        ui.painter().text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            label.to_uppercase(),
            egui::FontId::proportional(font::MINI_LABEL),
            if live { theme.text } else { theme.text_muted },
        );
    }
}

/// The hero: the routing shape, operators as boxes.
fn routing(ui: &mut egui::Ui, theme: &Theme, state: &QuadUi, voices: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let algo_index = state.value(qp::ALGO).round().max(0.0) as usize;
    let algo = qp::algorithm(algo_index as f32);
    painter.text(
        rect.left_top() + egui::vec2(pad, pad),
        egui::Align2::LEFT_TOP,
        format!(
            "{}   FB {:.0}%   {} {:.1}k   {}   {:.0} VOICES",
            qp::ALGO_NAMES.get(algo_index).copied().unwrap_or("?"),
            state.value(qp::FEEDBACK) * 100.0,
            qp::FMODE_NAMES
                .get(state.value(qp::FMODE).round() as usize)
                .copied()
                .unwrap_or("lp")
                .to_uppercase(),
            state.value(qp::CUTOFF) / 1000.0,
            qp::DIST_NAMES
                .get(state.value(qp::DIST).round() as usize)
                .copied()
                .unwrap_or("off")
                .to_uppercase(),
            voices
        ),
        mini.clone(),
        theme.text_muted,
    );
    let top = rect.top() + pad * 2.0 + font::MICRO_LABEL;
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad, top),
        rect.max - egui::vec2(pad, pad),
    );
    if body.height() < 20.0 {
        return;
    }
    let rows = (0..qp::OPS).map(|op| depth(algo, op)).max().unwrap_or(0) + 1;
    let box_w = (body.width() / qp::OPS as f32 - 6.0).max(12.0);
    let box_h = ((body.height() - 4.0 * (rows as f32 - 1.0)) / rows as f32)
        .min(28.0)
        .max(10.0);
    let centre = |op: usize| {
        let x = body.left() + (op as f32 + 0.5) * body.width() / qp::OPS as f32;
        let y = body.bottom() - box_h * 0.5 - depth(algo, op) as f32 * (box_h + 4.0);
        egui::pos2(x, y)
    };
    for (m, c) in algo.edges {
        painter.line_segment(
            [centre(*m), centre(*c)],
            egui::Stroke::new(1.0, theme.role_time),
        );
    }
    for op in 0..qp::OPS {
        let level = state.value(qp::op_param(op, qp::LEVEL_OP));
        let r = egui::Rect::from_center_size(centre(op), egui::vec2(box_w, box_h));
        let carrier = algo.carriers.contains(&op);
        painter.rect_filled(
            r,
            0.0,
            if carrier {
                theme.role_mod.gamma_multiply(0.25 + 0.5 * level)
            } else {
                theme.role_time.gamma_multiply(0.2 + 0.5 * level)
            },
        );
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(
                1.0,
                if carrier {
                    theme.role_mod
                } else {
                    theme.role_time
                },
            ),
            egui::StrokeKind::Inside,
        );
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "{} x{:.1}",
                op + 1,
                state.value(qp::op_param(op, qp::RATIO))
            ),
            mini.clone(),
            theme.text,
        );
    }
    crate::ui::hud::brackets(&painter, body, egui::Stroke::new(1.0, theme.outline));
}

fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut QuadUi,
    page: usize,
    edits: &mut Vec<ParamEdit>,
) {
    let Some((_, row)) = PAGES.get(page.min(PAGES.len() - 1)) else {
        return;
    };
    let height = ui.available_height().max(1.0);
    ui.horizontal(|ui| {
        let gap_x = ui.spacing().item_spacing.x;
        for (drawn, param) in row.iter().enumerate() {
            let spec = param_of(*param);
            let left = (row.len() - drawn) as f32;
            let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let Some(norm) = state.slot(*param) else {
                continue;
            };
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    if poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None) {
                        edits.push(ParamEdit {
                            param: *param,
                            value: quad_value(*param, *norm),
                        });
                    }
                },
            );
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 620.0;
    const MIN_W: f32 = 260.0;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            },
        );
        run.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn every_table_row_leaves_as_an_edit_and_is_on_exactly_one_page() {
        let edits = quad_edits(&QuadUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = qp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
        let mut shown: Vec<u32> = PAGES
            .iter()
            .flat_map(|(_, row)| row.iter().copied())
            .collect();
        shown.sort_unstable();
        assert_eq!(shown, expect);
        assert_eq!(COUNT, qp::TABLE.len());
    }

    #[test]
    fn every_position_is_a_legal_engine_value_and_round_trips() {
        for def in qp::TABLE {
            for i in 0..=40 {
                let value = quad_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
                let again = quad_value(def.id, quad_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (quad_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (quad_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table_and_depth_follows_the_routing() {
        let state = QuadUi::default();
        for def in qp::TABLE {
            let value = quad_value(def.id, state.norms[def.id as usize]);
            assert!(
                (value - def.default).abs() <= def.default.abs() * 1e-3 + 1e-3,
                "{}: default {} drew as {value}",
                def.name,
                def.default
            );
        }
        let serial = qp::ALGORITHMS[0];
        assert_eq!((depth(serial, 0), depth(serial, 3)), (0, 3));
        let flat = qp::ALGORITHMS[7];
        assert!((0..qp::OPS).all(|op| depth(flat, op) == 0));
        assert!(quad_is_discrete(qp::ALGO) && !quad_is_discrete(qp::CUTOFF));
        assert!(quad_is_log(qp::op_param(2, qp::RATIO)) && !quad_is_log(qp::FEEDBACK));
    }

    #[test]
    fn the_card_stays_inside_its_budget_and_fits_its_panel() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = QuadUi::default();
        let mut page = 0;
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| quad_card(ui, &theme, &mut state, &mut page, 0.0))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(h <= budget, "the card is {h:.0} pt tall, over {budget:.0}");
        let floor = frame(&ctx, |ui| face_width(ui, &theme)) + theme.sp(space::SM) * 2.0;
        for panel_w in [floor.ceil(), floor * 1.3, floor * 1.8] {
            let host = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(panel_w, theme.sp(control::DEVICE_TALL_H) + 8.0),
            );
            let used = frame(&ctx, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(host)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(host.width());
                quad_card(&mut child, &theme, &mut state, &mut page, 0.0);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at {panel_w:.0} the card drew {:.0}",
                used.width()
            );
            assert!(used.width() > panel_w * 0.7);
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing_on_any_page() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = QuadUi::default();
        let before = state;
        for page_index in 0..PAGES.len() {
            let mut page = page_index;
            for _ in 0..2 {
                let edits = frame(&ctx, |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| quad_card(ui, &theme, &mut state, &mut page, 1.0))
                        .inner
                });
                assert!(
                    edits.is_empty(),
                    "page {page_index} emitted {edits:?} at rest"
                );
            }
            assert_eq!(page, page_index);
        }
        assert_eq!(state, before);
    }
}
