//! sCOMP's device card — the bounced sine, in the old rack.
//!
//! Ids and ranges come from [`crate::params::scomp`]. The card is four
//! pages of cells under one hero: a schematic of the chain the take is
//! bounced through, as many times as PASSES says. The forge on the
//! stage draws the take itself; this card only has to say what the
//! knobs are and let them be turned.

use crate::params::scomp as sp;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The pages, in signal order: the sine, the filter in front of the
/// dynamics, the squash, and how the take is played.
const PAGES: [(&str, &[u32]); 4] = [
    (
        "source",
        &[sp::PASSES, sp::TAKE, sp::DROP, sp::DROP_MS, sp::DECAY],
    ),
    (
        "filter",
        &[sp::FILTER, sp::HARMONIC, sp::RESO, sp::SWEEP, sp::DRIFT],
    ),
    (
        "squash",
        &[
            sp::THRESH,
            sp::RATIO,
            sp::ATTACK,
            sp::RELEASE,
            sp::MAKEUP,
            sp::DRIVE,
        ],
    ),
    (
        "play",
        &[
            sp::SHIFT,
            sp::AMP_A,
            sp::AMP_R,
            sp::TUNE,
            sp::ROOT,
            sp::LEVEL,
            sp::OPEN,
        ],
    ),
];

/// A labelled cell is TWO lines: its value, then its name.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
const HEADER_ROWS: usize = 1;

pub fn pages() -> usize {
    PAGES.len()
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ScompUi {
    pub norms: [f32; 23],
}

impl Default for ScompUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(sp::TABLE, id).default)
    }
}

impl ScompUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let mut norms = [0.0; 23];
        for (i, slot) in norms.iter_mut().enumerate() {
            let id = i as u32;
            *slot = scomp_norm(id, get(id));
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
}

fn param_of(id: u32) -> Param {
    use crate::ui::device::{Mapping, Unit};
    let def = params::def(sp::TABLE, id);
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
        sp::PASSES => Param::choice("passes", sp::PASS_NAMES),
        sp::FILTER => Param::choice("filter", sp::FILTER_NAMES),
        sp::TAKE => log("take", Unit::Seconds),
        sp::DROP => linear("drop", Unit::Semitones),
        sp::DROP_MS => log("drop t", Unit::Ms),
        sp::DECAY => log("decay", Unit::Seconds),
        sp::HARMONIC => log("harm", Unit::Ratio),
        sp::RESO => log("reso", Unit::Plain),
        sp::SWEEP => linear("sweep", Unit::Plain).bipolar(),
        sp::DRIFT => linear("drift", Unit::Semitones).bipolar(),
        sp::THRESH => Param::db("thresh", def.min, def.max),
        sp::RATIO => log("ratio", Unit::Ratio),
        sp::ATTACK => log("attack", Unit::Ms),
        sp::RELEASE => log("release", Unit::Ms),
        sp::MAKEUP => Param::db("makeup", def.min, def.max),
        sp::DRIVE => log("drive", Unit::Ratio),
        sp::SHIFT => linear("shift", Unit::Semitones).bipolar(),
        sp::AMP_A => linear("amp a", Unit::Ms),
        sp::AMP_R => log("amp r", Unit::Ms),
        sp::TUNE => linear("tune", Unit::Semitones).bipolar(),
        sp::ROOT => linear("root", Unit::Note),
        sp::OPEN => Param::choice("forge", sp::OPEN_NAMES),
        _ => Param::percent("level"),
    };
    p.with_default(shown(id, def.default))
}

/// Engine units to the card's natural units.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        sp::PASSES => value - 1.0,
        sp::LEVEL => value / sp::LEVEL_MAX * 100.0,
        _ => value,
    }
}

/// The card's natural units back to the engine's.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        sp::PASSES => value + 1.0,
        sp::LEVEL => value / 100.0 * sp::LEVEL_MAX,
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

pub fn scomp_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn scomp_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn scomp_is_discrete(param: u32) -> bool {
    matches!(param, sp::PASSES | sp::FILTER | sp::OPEN)
}

pub fn scomp_is_log(param: u32) -> bool {
    matches!(
        param,
        sp::TAKE
            | sp::DROP_MS
            | sp::DECAY
            | sp::HARMONIC
            | sp::RESO
            | sp::RATIO
            | sp::ATTACK
            | sp::RELEASE
            | sp::DRIVE
            | sp::AMP_R
    )
}

pub fn scomp_edits(state: &ScompUi) -> Vec<ParamEdit> {
    let mut state = *state;
    sp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: scomp_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the face needs: its WIDEST page at its narrowest.
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

/// Draw the card. Returns the edits the user made.
pub fn scomp_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ScompUi,
    page: &mut usize,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "scomp", control::DEVICE_TALL_H, |ui| {
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
                    poly_widgets::CurveRegion::Plot => hero(ui, theme, state),
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
                ui.id().with(("scomp_page", index)),
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

/// The hero: the chain, drawn once per pass, and a reading of it.
fn hero(ui: &mut egui::Ui, theme: &Theme, state: &ScompUi) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let value = |id: u32| scomp_value(id, state.norms.get(id as usize).copied().unwrap_or(0.0));
    let passes = value(sp::PASSES).round().max(1.0) as usize;
    let filter = sp::FILTER_NAMES
        .get(value(sp::FILTER).round() as usize)
        .copied()
        .unwrap_or("?");
    painter.text(
        rect.left_top() + egui::vec2(pad, pad),
        egui::Align2::LEFT_TOP,
        format!(
            "SINE  {passes} PASS{}   {filter} x{:.1}   {:.0}:1 @ {:+.0} dB   DRIVE x{:.0}   SHIFT {:+.0} st",
            if passes == 1 { "" } else { "ES" },
            value(sp::HARMONIC),
            value(sp::RATIO),
            value(sp::THRESH),
            value(sp::DRIVE),
            value(sp::SHIFT),
        ),
        mini.clone(),
        theme.text_muted,
    );
    // The chain, as boxes: the sine, then FILTER → COMP → CLIP → BOUNCE,
    // repeated a pass at a time and fading as the picture runs out of
    // room, so a long bounce reads as a long bounce.
    let top = rect.top() + pad * 2.0 + font::MICRO_LABEL;
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad, top),
        rect.max - egui::vec2(pad, pad),
    );
    if body.height() < 12.0 {
        return;
    }
    let box_h = (body.height() / passes.max(1) as f32).min(14.0).max(6.0);
    let stages = ["F", "C", "X", "R"];
    let sine_w = 28.0;
    let box_w = ((body.width() - sine_w - pad) / stages.len() as f32)
        .min(48.0)
        .max(10.0);
    painter.text(
        egui::pos2(body.left(), body.top() + box_h * 0.5),
        egui::Align2::LEFT_CENTER,
        "~",
        egui::FontId::monospace(font::VALUE),
        theme.role_mod,
    );
    for k in 0..passes {
        let y = body.top() + k as f32 * (box_h + 2.0);
        if y + box_h > body.bottom() {
            break;
        }
        let fade = 1.0 - k as f32 / (passes as f32 * 1.5);
        for (i, stage) in stages.iter().enumerate() {
            let x = body.left() + sine_w + pad + i as f32 * box_w;
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(box_w - 2.0, box_h));
            painter.rect_filled(r, 0.0, theme.role_time.gamma_multiply(0.12 * fade));
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, theme.role_mod.gamma_multiply(fade)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *stage,
                mini.clone(),
                theme.text.gamma_multiply(fade),
            );
        }
    }
    crate::ui::hud::brackets(&painter, body, egui::Stroke::new(1.0, theme.outline));
}

fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ScompUi,
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
                            value: scomp_value(*param, *norm),
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
        let edits = scomp_edits(&ScompUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = sp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown: Vec<u32> = PAGES
            .iter()
            .flat_map(|(_, row)| row.iter().copied())
            .collect();
        shown.sort_unstable();
        assert_eq!(
            shown, expect,
            "the pages do not cover the table exactly once"
        );
    }

    #[test]
    fn every_position_is_a_legal_engine_value_and_round_trips() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = scomp_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
                let again = scomp_value(def.id, scomp_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (scomp_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (scomp_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let state = ScompUi::default();
        for def in sp::TABLE {
            let value = scomp_value(def.id, state.norms[def.id as usize]);
            assert!(
                (value - def.default).abs() <= def.default.abs() * 1e-3 + 1e-3,
                "{}: default {} drew as {value}",
                def.name,
                def.default
            );
        }
        assert!(scomp_is_discrete(sp::PASSES) && scomp_is_discrete(sp::FILTER));
        assert!(!scomp_is_discrete(sp::DRIVE));
        assert!(scomp_is_log(sp::RATIO) && !scomp_is_log(sp::TUNE));
    }

    #[test]
    fn the_card_stays_inside_its_budget_and_fits_its_panel() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ScompUi::default();
        let mut page = 0;
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| scomp_card(ui, &theme, &mut state, &mut page))
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
                scomp_card(&mut child, &theme, &mut state, &mut page);
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
        let mut state = ScompUi::default();
        let before = state;
        for page_index in 0..PAGES.len() {
            let mut page = page_index;
            for _ in 0..2 {
                let edits = frame(&ctx, |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| scomp_card(ui, &theme, &mut state, &mut page))
                        .inner
                });
                assert!(
                    edits.is_empty(),
                    "page {page_index} emitted {edits:?} at rest"
                );
            }
            assert_eq!(page, page_index, "drawing changed the page");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }
}
