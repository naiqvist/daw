//! BRICK's device card — the drum one-shot in the old rack.
//!
//! Four pages: the hit, its shape, its tone, its dirt. The hero draws
//! the shape the knobs give a hit — the attack, the punch's lean, the
//! curved fall — from the same rule the voices shape it with.

use crate::params::brick as bp;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const PAGES: [(&str, &[u32]); 4] = [
    (
        "hit",
        &[bp::TUNE, bp::FINE, bp::KEY, bp::START, bp::REVERSE],
    ),
    (
        "shape",
        &[
            bp::ATTACK,
            bp::DECAY,
            bp::CURVE,
            bp::DROP,
            bp::DROP_MS,
            bp::PUNCH,
        ],
    ),
    (
        "tone",
        &[bp::BODY, bp::BODY_HZ, bp::SNAP, bp::CUTOFF, bp::RESO],
    ),
    (
        "dirt",
        &[
            bp::BITS,
            bp::RATE,
            bp::GRIT,
            bp::CHOKE,
            bp::VELOCITY,
            bp::LEVEL,
        ],
    ),
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
const HEADER_ROWS: usize = 1;
const COUNT: usize = 22;

pub fn pages() -> usize {
    PAGES.len()
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BrickUi {
    pub norms: [f32; COUNT],
}

impl Default for BrickUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(bp::TABLE, id).default)
    }
}

impl BrickUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let mut norms = [0.0; COUNT];
        for (i, slot) in norms.iter_mut().enumerate() {
            let id = i as u32;
            *slot = brick_norm(id, get(id));
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
        brick_value(id, self.norms.get(id as usize).copied().unwrap_or(0.0))
    }
}

fn percent_row(param: u32) -> bool {
    matches!(
        param,
        bp::START | bp::PUNCH | bp::BODY | bp::SNAP | bp::VELOCITY
    )
}

fn param_of(id: u32) -> Param {
    use crate::ui::device::{Mapping, Unit};
    let def = params::def(bp::TABLE, id);
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
    let p = match id {
        bp::TUNE => linear("tune", Unit::Semitones).bipolar(),
        bp::FINE => linear("fine", Unit::Cents).bipolar(),
        bp::KEY => Param::choice("key", bp::KEY_NAMES),
        bp::START => Param::percent("start"),
        bp::REVERSE => Param::choice("reverse", bp::REVERSE_NAMES),
        bp::ATTACK => linear("attack", Unit::Ms),
        bp::DECAY => log("decay", Unit::Ms),
        bp::CURVE => linear("curve", Unit::Plain).bipolar(),
        bp::DROP => linear("drop", Unit::Semitones),
        bp::DROP_MS => log("drop t", Unit::Ms),
        bp::PUNCH => Param::percent("punch"),
        bp::BODY => Param::percent("body"),
        bp::BODY_HZ => log("body hz", Unit::Hz),
        bp::SNAP => Param::percent("snap"),
        bp::BITS => linear("bits", Unit::Plain),
        bp::RATE => log("rate", Unit::Hz),
        bp::GRIT => log("grit", Unit::Ratio),
        bp::CUTOFF => log("cutoff", Unit::Hz),
        bp::RESO => log("reso", Unit::Plain),
        bp::CHOKE => Param::choice("choke", bp::CHOKE_NAMES),
        bp::VELOCITY => Param::percent("velocity"),
        _ => Param::percent("level"),
    };
    p.with_default(shown(id, def.default))
}

fn shown(param: u32, value: f32) -> f32 {
    if param == bp::LEVEL {
        value / bp::LEVEL_MAX * 100.0
    } else if percent_row(param) {
        value * 100.0
    } else {
        value
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = if param == bp::LEVEL {
        value / 100.0 * bp::LEVEL_MAX
    } else if percent_row(param) {
        value / 100.0
    } else {
        value
    };
    params::def(bp::TABLE, param).clamp(raw)
}

pub fn brick_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn brick_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn brick_is_discrete(param: u32) -> bool {
    matches!(param, bp::KEY | bp::REVERSE | bp::CHOKE)
}

pub fn brick_is_log(param: u32) -> bool {
    matches!(
        param,
        bp::DECAY | bp::DROP_MS | bp::BODY_HZ | bp::RATE | bp::GRIT | bp::CUTOFF | bp::RESO
    )
}

pub fn brick_edits(state: &BrickUi) -> Vec<ParamEdit> {
    let mut state = *state;
    bp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: brick_value(def.id, *norm),
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

/// The shape a hit takes: level against time, the attack then the fall
/// with the punch's lean on it, over `seconds`. Shared with the Stage's
/// card, so both draw what the voices do.
pub fn hit_shape(
    attack_ms: f32,
    decay_ms: f32,
    curve: f32,
    punch: f32,
    seconds: f32,
    n: usize,
) -> Vec<f32> {
    let attack = (attack_ms.max(0.0) / 1000.0).max(1.0e-4);
    let decay = (decay_ms.max(1.0) / 1000.0).max(1.0e-3);
    (0..=n)
        .map(|i| {
            let t = seconds * i as f32 / n.max(1) as f32;
            let env = if t < attack {
                t / attack
            } else {
                bp::fall(curve, (t - attack) / decay)
            };
            let lean = 1.0 + punch * 2.0 * (-t / 0.008).exp();
            (env * lean) / 3.0
        })
        .collect()
}

pub fn brick_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut BrickUi,
    page: &mut usize,
    voices: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);
    card::card_sized(ui, theme, "brick", control::DEVICE_TALL_H, |ui| {
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
                    poly_widgets::CurveRegion::Plot => shape(ui, theme, state, voices),
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
                ui.id().with(("brick_page", index)),
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

/// The hero: the hit's shape.
fn shape(ui: &mut egui::Ui, theme: &Theme, state: &BrickUi, voices: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    painter.text(
        rect.left_top() + egui::vec2(pad, pad),
        egui::Align2::LEFT_TOP,
        format!(
            "{:.0}bit {:.1}k   drop {:+.0}st   punch {:.0}%   {}   {:.0} HITS",
            state.value(bp::BITS),
            state.value(bp::RATE) / 1000.0,
            state.value(bp::DROP),
            state.value(bp::PUNCH) * 100.0,
            bp::CHOKE_NAMES
                .get(state.value(bp::CHOKE).round() as usize)
                .copied()
                .unwrap_or("cut")
                .to_uppercase(),
            voices
        ),
        mini,
        theme.text_muted,
    );
    let top = rect.top() + pad * 2.0 + font::MICRO_LABEL;
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad, top),
        rect.max - egui::vec2(pad, pad),
    );
    if plot.height() < 12.0 {
        return;
    }
    let seconds = (state.value(bp::DECAY) / 1000.0 + 0.05).clamp(0.1, 2.0);
    let levels = hit_shape(
        state.value(bp::ATTACK),
        state.value(bp::DECAY),
        state.value(bp::CURVE),
        state.value(bp::PUNCH),
        seconds,
        64,
    );
    let peak = levels.iter().cloned().fold(0.0f32, f32::max).max(1.0e-3);
    let points: Vec<egui::Pos2> = levels
        .iter()
        .enumerate()
        .map(|(i, level)| {
            egui::pos2(
                plot.left() + plot.width() * i as f32 / 64.0,
                plot.bottom() - plot.height() * (level / peak).clamp(0.0, 1.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.8, theme.role_mod),
    ));
    crate::ui::hud::brackets(&painter, plot, egui::Stroke::new(1.0, theme.outline));
}

fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut BrickUi,
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
                            value: brick_value(*param, *norm),
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
        let edits = brick_edits(&BrickUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = bp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
        let mut shown: Vec<u32> = PAGES
            .iter()
            .flat_map(|(_, row)| row.iter().copied())
            .collect();
        shown.sort_unstable();
        assert_eq!(shown, expect);
        assert_eq!(COUNT, bp::TABLE.len());
    }

    #[test]
    fn every_position_is_a_legal_engine_value_and_round_trips() {
        for def in bp::TABLE {
            for i in 0..=40 {
                let value = brick_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
                let again = brick_value(def.id, brick_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (brick_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (brick_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table_and_the_shape_falls() {
        let state = BrickUi::default();
        for def in bp::TABLE {
            let value = brick_value(def.id, state.norms[def.id as usize]);
            assert!(
                (value - def.default).abs() <= def.default.abs() * 1e-3 + 1e-3,
                "{}: default {} drew as {value}",
                def.name,
                def.default
            );
        }
        let levels = hit_shape(1.0, 300.0, 0.5, 1.0, 0.5, 32);
        assert!(levels[1] > levels[16] && levels[16] > levels[32]);
        assert!(brick_is_discrete(bp::CHOKE) && !brick_is_discrete(bp::DROP));
        assert!(brick_is_log(bp::DECAY) && !brick_is_log(bp::BITS));
    }

    #[test]
    fn the_card_stays_inside_its_budget_and_fits_its_panel() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = BrickUi::default();
        let mut page = 0;
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| brick_card(ui, &theme, &mut state, &mut page, 0.0))
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
                brick_card(&mut child, &theme, &mut state, &mut page, 0.0);
                child.min_rect()
            });
            assert!(used.width() <= panel_w + 1.0);
            assert!(used.width() > panel_w * 0.7);
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing_on_any_page() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = BrickUi::default();
        let before = state;
        for page_index in 0..PAGES.len() {
            let mut page = page_index;
            for _ in 0..2 {
                let edits = frame(&ctx, |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| brick_card(ui, &theme, &mut state, &mut page, 1.0))
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
