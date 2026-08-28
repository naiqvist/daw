//! CLAMP's card — the transfer curve, finally used as a compressor.
//!
//! `dynamics::transfer_curve` has drawn a compressor since the kit was
//! built. Nothing ever asked it to: the gate uses it in
//! [`Mode::Expand`](dynamics::Mode::Expand), and `glue` shows a scope
//! instead — the right choice there, because what you watch on a bus is
//! the programme moving against a threshold, not the rules.
//!
//! Surgical work is the other way round. You are aiming at one event,
//! and what you need to see is the SHAPE you are aiming with: where the
//! knee starts bending, how steep it goes after it, and how far the
//! whole thing is from unity. That is a transfer curve and nothing else
//! draws it.
//!
//! # The picture is the control
//!
//! Drag across to move the threshold, up and down for the ratio. The
//! widget has always done this; this is the first card where the two
//! axes it offers are the two knobs the device is actually set by.
//!
//! The GAIN REDUCTION METER beside it is the other half. The curve is a
//! static map with no time on it, so attack and release cannot appear on
//! it — the meter is where a fast compressor shows it is fast.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, dynamics, metrics, poly_widgets,
};
use crate::params::{self, clamp as cp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The card's knob positions, normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClampUi {
    pub threshold: f32,
    pub ratio: f32,
    pub knee: f32,
    pub attack: f32,
    pub release: f32,
    pub makeup: f32,
    pub sc_hp: f32,
    pub warmth: f32,
    pub mix: f32,
}

impl Default for ClampUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(cp::TABLE, id).default)
    }
}

impl ClampUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| clamp_norm(id, get(id));
        Self {
            threshold: at(cp::THRESHOLD),
            ratio: at(cp::RATIO),
            knee: at(cp::KNEE),
            attack: at(cp::ATTACK),
            release: at(cp::RELEASE),
            makeup: at(cp::MAKEUP),
            sc_hp: at(cp::SC_HP),
            warmth: at(cp::WARMTH),
            mix: at(cp::MIX),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            cp::THRESHOLD => &mut self.threshold,
            cp::RATIO => &mut self.ratio,
            cp::KNEE => &mut self.knee,
            cp::ATTACK => &mut self.attack,
            cp::RELEASE => &mut self.release,
            cp::MAKEUP => &mut self.makeup,
            cp::SC_HP => &mut self.sc_hp,
            cp::WARMTH => &mut self.warmth,
            cp::MIX => &mut self.mix,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            cp::THRESHOLD => self.threshold,
            cp::RATIO => self.ratio,
            cp::KNEE => self.knee,
            cp::ATTACK => self.attack,
            cp::RELEASE => self.release,
            cp::MAKEUP => self.makeup,
            cp::SC_HP => self.sc_hp,
            cp::WARMTH => self.warmth,
            _ => self.mix,
        }
    }
}

const CELLS: [u32; 9] = [
    cp::THRESHOLD,
    cp::RATIO,
    cp::KNEE,
    cp::ATTACK,
    cp::RELEASE,
    cp::MAKEUP,
    cp::SC_HP,
    cp::WARMTH,
    cp::MIX,
];

/// One control by wire id, in natural units.
///
/// Times and the sidechain corner are LOG: the difference between 0.05
/// and 0.5 ms is the whole character of this device and the difference
/// between 45 and 50 ms is nothing. The RATIO is log too, for the same
/// reason in the other direction — 1:1 to 4:1 is where every useful
/// setting lives and 15:1 to 20:1 is one sound.
fn param_of(param: u32) -> Param {
    let def = params::def(cp::TABLE, param);
    let log = |name: &'static str, unit| {
        Param::new(
            name,
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            unit,
        )
        .with_default(def.default)
    };
    let linear = |name: &'static str, unit| {
        Param::new(
            name,
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            unit,
        )
        .with_default(def.default)
    };
    match param {
        cp::THRESHOLD => linear("thresh", Unit::Db),
        cp::RATIO => log("ratio", Unit::Ratio),
        cp::KNEE => linear("knee", Unit::Db),
        cp::ATTACK => log("attack", Unit::Ms),
        cp::RELEASE => log("release", Unit::Ms),
        cp::MAKEUP => linear("makeup", Unit::Db),
        cp::SC_HP => log("sc hp", Unit::Hz),
        cp::WARMTH => Param::percent("warmth").with_default(def.default * 100.0),
        _ => Param::percent("mix").with_default(def.default * 100.0),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        cp::WARMTH | cp::MIX => value * 100.0,
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    match param {
        cp::WARMTH | cp::MIX => value / 100.0,
        _ => value,
    }
}

pub fn clamp_value(param: u32, norm: f32) -> f32 {
    let def = params::def(cp::TABLE, param);
    natural(param, param_of(param).value(norm)).clamp(def.min, def.max)
}

pub fn clamp_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn clamp_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

pub fn clamp_edits(state: &ClampUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: clamp_value(param, state.get(param)),
        })
        .collect()
}

/// The curve as the display needs it, built from the card's own state —
/// so the picture renders the numbers the engine has rather than a copy
/// kept beside them.
fn curve_of(state: &ClampUi) -> dynamics::Dynamics {
    dynamics::Dynamics {
        mode: dynamics::Mode::Compress,
        threshold_db: clamp_value(cp::THRESHOLD, state.threshold),
        ratio: clamp_value(cp::RATIO, state.ratio),
        knee_db: clamp_value(cp::KNEE, state.knee),
        makeup_db: clamp_value(cp::MAKEUP, state.makeup),
        attack_ms: clamp_value(cp::ATTACK, state.attack),
        release_ms: clamp_value(cp::RELEASE, state.release),
    }
}

/// Two rows, grouped the way a compressor is actually set up: WHERE and
/// HOW HARD first, then HOW FAST, then what it is fed and what comes out.
const ROWS: [&[u32]; 2] = [
    &[cp::THRESHOLD, cp::RATIO, cp::KNEE, cp::MAKEUP],
    &[cp::ATTACK, cp::RELEASE, cp::SC_HP, cp::WARMTH, cp::MIX],
];

/// A labelled cell is TWO lines: its value, then its name.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the face needs: its widest row at its narrowest.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    ROWS.iter()
        .map(|row| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// Draw the clamp card. `reduction_db` is what the engine is doing right
/// now — zero is none, negative is reduction.
pub fn clamp_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ClampUi,
    reduction_db: f32,
    level_db: Option<f32>,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "clamp", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        edits.extend(hero(ui, theme, state, reduction_db, level_db));
                    }
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The curve, with the reduction meter standing beside it.
fn hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ClampUi,
    reduction_db: f32,
    level_db: Option<f32>,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let mut curve = curve_of(state);
    // SQUARE, at whatever the panel has left — not the widget's own
    // footprint, which is taller than a tall card's plot region and
    // would print the curve through the value strip below it. The
    // gate's card learned this first.
    let side = ui.available_height().min(ui.available_width());
    ui.horizontal(|ui| {
        if dynamics::transfer_curve_sized(ui, theme, &mut curve, level_db, side) {
            // The values come back OUT of the widget and into the card's
            // state: the picture is the control, and a control that
            // could not write would be a picture.
            state.threshold = clamp_norm(cp::THRESHOLD, curve.threshold_db);
            state.ratio = clamp_norm(cp::RATIO, curve.ratio);
            for id in [cp::THRESHOLD, cp::RATIO] {
                edits.push(ParamEdit {
                    param: id,
                    value: clamp_value(id, state.get(id)),
                });
            }
        }
        // The meter is the half the curve cannot draw. A transfer curve
        // is a static map with no time axis, so attack and release are
        // invisible on it — and "is it fast" is the question this whole
        // device answers.
        dynamics::reduction_meter(ui, theme, reduction_db);
    });
    edits
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut ClampUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, restated rather than divided
    // out of what is available — dividing hands each row a share of the
    // gaps as well and walks the rows down the card until they overlap.
    let gap = theme.sp(space::XXS);
    let rows = ROWS.len() as f32;
    let height = ((ui.available_height() - gap * (rows - 1.0)) / rows).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    for row in ROWS {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                // The share is recomputed from what is ACTUALLY left,
                // cell by cell: worked out in advance, any cell needing
                // more than its share spends the row's remainder and the
                // last one is pushed off the card's right edge.
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
                            let value = clamp_value(*param, *norm);
                            edits.push(ParamEdit {
                                param: *param,
                                value,
                            });
                        }
                    },
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    /// Narrow on purpose: a curve, a meter beside it, and nine cells.
    const MAX_W: f32 = 620.0;
    const MIN_W: f32 = 280.0;

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
        out.expect("the frame ran")
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = clamp_edits(&ClampUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = cp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");

        // And the FACE shows every one of them: the two the hero owns
        // are in the footer as well, because a curve is a coarse control
        // and a surgical device is set by typing-precision numbers.
        let mut face: Vec<u32> = ROWS.concat();
        face.sort_unstable();
        assert_eq!(face, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in cp::TABLE {
            for i in 0..=40 {
                let value = clamp_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!((clamp_value(def.id, 0.0) - def.min).abs() < span * 1e-3);
            assert!((clamp_value(def.id, 1.0) - def.max).abs() < span * 1e-3);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in cp::TABLE {
            for i in 0..=40 {
                let value = clamp_value(def.id, i as f32 / 40.0);
                let again = clamp_value(def.id, clamp_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-4,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in cp::TABLE {
            let mut state = ClampUi::default();
            let norm = *state.slot(def.id).expect("every row has a slot");
            let value = clamp_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ClampUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| clamp_card(ui, &theme, &mut state, 0.0, None))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ClampUi::default();

        for panel_w in [560.0f32, 640.0, 900.0] {
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
                clamp_card(&mut child, &theme, &mut state, 0.0, None);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt wide",
                used.width()
            );
            assert!(
                used.width() > panel_w * 0.7,
                "at a {panel_w:.0} pt panel the card drew only {:.0} pt",
                used.width()
            );
        }
    }

    /// Drawing at rest must not move a control — a card that emits on
    /// its first frame writes its own defaults over a loaded patch.
    /// The hero is a picture that is also a control, which is exactly
    /// the shape that gets this wrong.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ClampUi::default();
        let before = state;
        for reduction in [0.0f32, -6.0] {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| {
                        clamp_card(ui, &theme, &mut state, reduction, Some(-12.0))
                    })
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// A POINTER on the hero. The five card tests above never press
    /// anything, and every interaction bug this codebase has shipped
    /// lived in the gesture — see `notes/20260826-device-ui-contract.md`.
    ///
    /// Across is the threshold. The drag starts in the middle of the
    /// plot, so neither end is a clamp against a rail.
    #[test]
    fn dragging_across_the_curve_moves_the_threshold() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ClampUi::default();
        let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(240.0, 240.0));
        let mid = rect.center();
        let path = probe::drag_path(mid, mid - egui::vec2(60.0, 0.0), 4);

        let edits: Vec<ParamEdit> = probe::run(&ctx, rect, &path, |ui| {
            hero(ui, &theme, &mut state, 0.0, None)
        })
        .concat();

        assert!(
            edits.iter().any(|e| e.param == cp::THRESHOLD),
            "dragging across the curve emitted {edits:?}"
        );
        let opened = clamp_value(cp::THRESHOLD, ClampUi::default().threshold);
        let now = clamp_value(cp::THRESHOLD, state.threshold);
        assert!(
            now < opened - 0.5,
            "the threshold went from {opened} to {now} — leftward should lower it"
        );
    }

    /// And up the plot is the ratio, on the SAME gesture target. Two
    /// axes on one widget is the arrangement that most often turns into
    /// one axis quietly winning.
    #[test]
    fn dragging_up_the_curve_moves_the_ratio() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ClampUi::default();
        let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(240.0, 240.0));
        let mid = rect.center();
        let path = probe::drag_path(mid, mid - egui::vec2(0.0, 60.0), 4);

        let edits: Vec<ParamEdit> = probe::run(&ctx, rect, &path, |ui| {
            hero(ui, &theme, &mut state, 0.0, None)
        })
        .concat();

        assert!(
            edits.iter().any(|e| e.param == cp::RATIO),
            "dragging up the curve emitted {edits:?}"
        );
        let opened = clamp_value(cp::RATIO, ClampUi::default().ratio);
        let now = clamp_value(cp::RATIO, state.ratio);
        assert!(
            (now - opened).abs() > 0.05,
            "the ratio did not move off {opened}"
        );
    }
}
