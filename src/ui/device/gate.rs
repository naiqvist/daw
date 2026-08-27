//! The gate device card — the transfer curve as the control.
//!
//! Normalized knob state here, natural values out as [`ParamEdit`]s. Ids,
//! ranges and defaults come from [`crate::params::gate`] — the one table
//! this widget, `audio::gate::GateCore` and the app's edit routing all
//! read.
//!
//! # The picture IS the control, and this time it came for free
//!
//! The other small cards in this rack draw read-only heroes, because a
//! staircase and a chirp have no obvious gesture. A dynamics curve does:
//! drag across for the threshold, up and down for the ratio. That widget
//! — [`dynamics::transfer_curve`] — has existed since the kit was built,
//! already handles [`Mode::Expand`](dynamics::Mode::Expand), and until
//! now had no caller outside the gallery. The gate is its first real use.
//!
//! Which means this card owes rule 4 of the device-UI contract a POINTER
//! test, and `dragging_the_curve_moves_the_threshold_and_the_ratio`
//! is it. The widget is one `ui::interact` over one rect, so rule 1 has
//! nothing to catch it on.
//!
//! # The ranges are the display's
//!
//! `params::gate` takes its threshold and ratio bounds from this widget's
//! own axes rather than from the kernel's wider ones, and its doc says
//! why: on this card the curve is not an illustration of the knobs, it is
//! the knobs, so a cell that could reach past the axis it is drawn on
//! would be a card disagreeing with itself.
//!
//! # What is not on the curve
//!
//! Attack, release and range. The first two because a transfer curve is a
//! static map with no time axis — the widget's own doc says so — and the
//! third because it is a floor under the curve rather than a shape in it.
//! All three are cells, which is the honest amount of display a time gets
//! on a picture with no time in it.

use crate::params::gate::{
    ATTACK, ATTACK_MAX_MS, ATTACK_MIN_MS, KNEE_DB, RANGE, RANGE_MAX_DB, RATIO, RATIO_MAX,
    RATIO_MIN, RELEASE, RELEASE_MAX_MS, RELEASE_MIN_MS, TABLE, THRESHOLD, THRESHOLD_MAX_DB,
    THRESHOLD_MIN_DB,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, dynamics, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// Knob positions of one gate, normalized. Serialized into project files,
/// so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GateUi {
    pub threshold: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub range: f32,
}

impl Default for GateUi {
    fn default() -> Self {
        Self {
            threshold: gate_norm(THRESHOLD, params::def(TABLE, THRESHOLD).default),
            ratio: gate_norm(RATIO, params::def(TABLE, RATIO).default),
            attack: gate_norm(ATTACK, params::def(TABLE, ATTACK).default),
            release: gate_norm(RELEASE, params::def(TABLE, RELEASE).default),
            range: gate_norm(RANGE, params::def(TABLE, RANGE).default),
        }
    }
}

impl GateUi {
    /// This state's knob position for a wire id, mutably — public, so
    /// the app's `gate_knobs` can fill one from engine units by walking
    /// the table, the way `glue_knobs` does.
    pub fn slot_mut(&mut self, param: u32) -> Option<&mut f32> {
        self.slot(param)
    }

    /// This state's knob position for a wire id, mutably.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            THRESHOLD => &mut self.threshold,
            RATIO => &mut self.ratio,
            ATTACK => &mut self.attack,
            RELEASE => &mut self.release,
            RANGE => &mut self.range,
            _ => return None,
        })
    }
}

struct Spec {
    threshold: Param,
    ratio: Param,
    attack: Param,
    release: Param,
    range: Param,
}

/// The five controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        threshold: default(
            Param::db("threshold", THRESHOLD_MIN_DB, THRESHOLD_MAX_DB),
            THRESHOLD,
        ),
        // LOG, and shown as a bare number because `8.0` reads as eight to
        // one where "8.00 x" would read as a gain.
        ratio: default(
            Param::new(
                "ratio",
                Mapping::Log {
                    min: RATIO_MIN,
                    max: RATIO_MAX,
                },
                Unit::Plain,
            ),
            RATIO,
        ),
        // LOG: a time is a ratio, and the step from 1 ms to 2 is the same
        // amount of "slower" as from 50 to 100.
        attack: default(
            Param::new(
                "attack",
                Mapping::Log {
                    min: ATTACK_MIN_MS,
                    max: ATTACK_MAX_MS,
                },
                Unit::Ms,
            ),
            ATTACK,
        ),
        release: default(
            Param::new(
                "release",
                Mapping::Log {
                    min: RELEASE_MIN_MS,
                    max: RELEASE_MAX_MS,
                },
                Unit::Ms,
            ),
            RELEASE,
        ),
        // Shown as the DEPTH it reaches, so the cell reads "-60.0 dB"
        // rather than "60.0" — a range is a distance downward, and the
        // sign is the difference between a control that shuts and one
        // that lifts.
        range: default(Param::db("range", -RANGE_MAX_DB, 0.0), RANGE),
    }
}

/// What the widget SHOWS for an engine-facing value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        // The table stores the DEPTH as a positive number of dB; the cell
        // shows it as the negative it actually is.
        RANGE => -value,
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows, clamped through
/// the row so a knob at either stop cannot emit a letter to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        RANGE => -value,
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        THRESHOLD => s.threshold,
        RATIO => s.ratio,
        ATTACK => s.attack,
        RELEASE => s.release,
        _ => s.range,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn gate_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`gate_value`], for a state stored in engine units.
pub fn gate_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. None here does.
pub fn gate_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The ratio and both times do.
pub fn gate_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn gate_edits(state: &GateUi) -> Vec<ParamEdit> {
    [
        (THRESHOLD, state.threshold),
        (RATIO, state.ratio),
        (ATTACK, state.attack),
        (RELEASE, state.release),
        (RANGE, state.range),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: gate_value(param, norm),
    })
    .collect()
}

/// The curve as the display needs it, built from the card's state — so
/// the drawing renders the same numbers the engine has rather than a copy
/// kept alongside them.
fn curve_of(state: &GateUi) -> dynamics::Dynamics {
    dynamics::Dynamics {
        mode: dynamics::Mode::Expand,
        threshold_db: gate_value(THRESHOLD, state.threshold),
        ratio: gate_value(RATIO, state.ratio),
        // The engine's fixed knee, so the curve's corner is the corner
        // the audio actually has.
        knee_db: KNEE_DB,
        makeup_db: 0.0,
        attack_ms: gate_value(ATTACK, state.attack),
        release_ms: gate_value(RELEASE, state.release),
    }
}

/// The hero, and the two rows it owns. Returns the edits a drag made.
fn transfer(ui: &mut egui::Ui, theme: &Theme, state: &mut GateUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let mut curve = curve_of(state);
    // Centred: the widget is square by contract and the plot region is
    // whatever the card is wide, so left-aligning it would park the
    // picture against one edge.
    // SQUARE, at whatever the panel has left — not the widget's own
    // 160-point footprint, which is larger than a tall card's plot region
    // and printed the curve straight through the value strip below it.
    // `dark_curve_panel` pays for the footer first, so what is available
    // here is exactly what the picture may have.
    let side = ui.available_height().min(ui.available_width());
    ui.vertical_centered(|ui| {
        if dynamics::transfer_curve_sized(ui, theme, &mut curve, None, side) {
            // The values come back OUT of the widget and into the card's
            // normalized state — the picture is the control, and a
            // control that could not write would be a picture.
            state.threshold = gate_norm(THRESHOLD, curve.threshold_db);
            state.ratio = gate_norm(RATIO, curve.ratio);
            for id in [THRESHOLD, RATIO] {
                let Some(norm) = state.slot(id) else { continue };
                edits.push(ParamEdit {
                    param: id,
                    value: gate_value(id, *norm),
                });
            }
        }
    });
    edits
}

/// What ONE cell needs: room for the widest thing it will ever print.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The five cells of the strip, in the order they are drawn — the order a
/// gate is actually set up in: where, how hard, how fast in, how fast
/// out, how far.
fn strip(s: &Spec) -> [(&Param, u32); 5] {
    [
        (&s.threshold, THRESHOLD),
        (&s.ratio, RATIO),
        (&s.attack, ATTACK),
        (&s.release, RELEASE),
        (&s.range, RANGE),
    ]
}

/// The width the strip needs, summed from the same per-cell figure the
/// strip draws each cell at — a share is not a sum.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in. TWO,
/// because a labelled cell draws a value over a name — see
/// `notes/20260827-device-card-layout.md`, rule 1.
const VALUE_ROWS: usize = 2;

/// The card's width contract: the strip, and only the strip. The curve
/// takes whatever square the panel has left, so it is never the binding
/// dimension.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the gate card. Returns the edits the user just made.
pub fn gate_card(ui: &mut egui::Ui, theme: &Theme, state: &mut GateUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "gate", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        edits.extend(transfer(ui, theme, state));
                    }
                    poly_widgets::CurveRegion::Footer => {
                        let h = ui.available_height();
                        ui.horizontal(|ui| {
                            for (param, id) in strip(&s) {
                                let w = cell_width(ui, theme, param);
                                let Some(norm) = state.slot(id) else {
                                    continue;
                                };
                                ui.allocate_ui_with_layout(
                                    egui::vec2(w, h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(w);
                                        ui.set_height(h);
                                        if poly_widgets::labeled_cell_bar(
                                            ui, theme, param, norm, None,
                                        ) {
                                            edits.push(ParamEdit {
                                                param: id,
                                                value: gate_value(id, *norm),
                                            });
                                        }
                                    },
                                );
                            }
                        });
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = gate_edits(&GateUi::default());
        assert_eq!(edits.len(), TABLE.len());
        for row in TABLE {
            assert!(
                edits.iter().any(|e| e.param == row.id),
                "`{}` never leaves the card",
                row.name
            );
        }
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for row in TABLE {
            for step in 0..=64 {
                let norm = step as f32 / 64.0;
                let value = gate_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            // Either ORDER: `range` runs backwards on purpose, the way
            // a hardware gate's does — the top of the knob is 0 dB and
            // no gating, and turning it down shuts harder. So the test
            // is that the two ends of the travel reach the two ends of
            // the range, not that they do it in ascending order.
            let (lo, hi) = (gate_value(row.id, 0.0), gate_value(row.id, 1.0));
            let (min, max) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            assert!((min - row.min).abs() < 1e-2, "{} floor: {min}", row.name);
            assert!((max - row.max).abs() < 1e-2, "{} ceiling: {max}", row.name);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = gate_value(row.id, gate_norm(row.id, value));
                assert!(
                    (back - value).abs() <= 1e-2 * row.max.abs().max(1.0),
                    "`{}`: {value} came back as {back}",
                    row.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let fresh = GateUi::default();
        for (id, norm) in [
            (THRESHOLD, fresh.threshold),
            (RATIO, fresh.ratio),
            (ATTACK, fresh.attack),
            (RELEASE, fresh.release),
            (RANGE, fresh.range),
        ] {
            let want = params::def(TABLE, id).default;
            let got = gate_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    #[test]
    fn nothing_steps_and_the_ratio_and_times_are_log() {
        for id in [THRESHOLD, RATIO, ATTACK, RELEASE, RANGE] {
            assert!(!gate_is_discrete(id), "id {id} should sweep");
        }
        for id in [RATIO, ATTACK, RELEASE] {
            assert!(gate_is_log(id), "id {id} should be log");
        }
        for id in [THRESHOLD, RANGE] {
            assert!(!gate_is_log(id), "id {id} should not be log");
        }
    }

    /// The card draws the ENGINE's knee, not a second opinion about it.
    #[test]
    fn the_drawn_curve_is_the_engine_s() {
        let state = GateUi::default();
        let curve = curve_of(&state);
        assert_eq!(curve.mode, dynamics::Mode::Expand, "a gate expands");
        assert_eq!(curve.knee_db, KNEE_DB);
        assert_eq!(curve.threshold_db, gate_value(THRESHOLD, state.threshold));
        assert_eq!(curve.ratio, gate_value(RATIO, state.ratio));
    }

    /// RULE 4 OF THE DEVICE-UI CONTRACT. The transfer curve is this
    /// card's one draggable target and the gate is its first caller
    /// outside the gallery, so the gesture gets a pointer through it:
    /// across moves the threshold, up and down moves the ratio, and a
    /// press that goes nowhere moves neither.
    #[test]
    fn dragging_the_curve_moves_the_threshold_and_the_ratio() {
        const RECT: egui::Rect = egui::Rect {
            min: egui::pos2(0.0, 0.0),
            max: egui::pos2(400.0, 400.0),
        };
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        // ON the widget, derived rather than guessed: it is square at
        // `control::TRANSFER`, centred across the panel by `transfer`,
        // and starts at the top of what it is given. The first draft
        // pressed at (60, 60), which is outside it — and the test failed
        // saying the threshold had not moved, which was true and had
        // nothing to do with the card.
        // Derived from what `transfer` will actually allocate: a square
        // of the available space, centred. Guessing a point cost the
        // first draft of this test a failure that was entirely its own.
        let side = RECT.height().min(RECT.width());
        let at = egui::pos2(RECT.center().x, RECT.top() + side * 0.5);

        // Each run threads the card's own state through the gesture, the
        // way the app does — a card owns nothing between frames.
        let run = |to: egui::Pos2| {
            let mut state = GateUi::default();
            for _ in probe::run(&ctx, RECT, &probe::drag_path(at, to, 8), |ui| {
                transfer(ui, &theme, &mut state)
            }) {}
            state
        };

        let start = GateUi::default();
        let right = run(egui::pos2(at.x + 60.0, at.y));
        assert!(
            gate_value(THRESHOLD, right.threshold) > gate_value(THRESHOLD, start.threshold),
            "dragging right must raise the threshold: {} -> {}",
            gate_value(THRESHOLD, start.threshold),
            gate_value(THRESHOLD, right.threshold)
        );
        assert_eq!(right.ratio, start.ratio, "a sideways drag moved the ratio");

        let up = run(egui::pos2(at.x, at.y - 60.0));
        assert!(
            gate_value(RATIO, up.ratio) > gate_value(RATIO, start.ratio),
            "dragging up must raise the ratio: {} -> {}",
            gate_value(RATIO, start.ratio),
            gate_value(RATIO, up.ratio)
        );
        assert_eq!(
            up.threshold, start.threshold,
            "a vertical drag moved the threshold"
        );
    }

    /// A card that emits on its first frame writes its own defaults over
    /// a loaded patch. One of the five standing card tests.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        const RECT: egui::Rect = egui::Rect {
            min: egui::pos2(0.0, 0.0),
            max: egui::pos2(400.0, 400.0),
        };
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = GateUi::default();
        let emitted: Vec<Vec<ParamEdit>> = probe::run(
            &ctx,
            RECT,
            &probe::click_path(egui::pos2(-50.0, -50.0)),
            |ui| transfer(ui, &theme, &mut state),
        );
        assert!(
            emitted.iter().all(|e| e.is_empty()),
            "the card emitted without being touched"
        );
    }
}
