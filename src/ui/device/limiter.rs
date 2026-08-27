//! The character limiter's card.
//!
//! Its hero is THE TRANSFER CURVE — what comes out for what goes in,
//! drawn across both polarities. Three of the device's controls are
//! visible in that one line at once, and they are the three the argument
//! is always about:
//!
//! - `push` steepens it, so the diagonal climbs toward the ceiling sooner;
//! - `ceiling` is the flat top it runs into;
//! - `warmth` BENDS it, and bends the two halves DIFFERENTLY.
//!
//! That last one is the reason the curve is drawn through the origin
//! rather than in the positive quadrant alone. The warmth stage is
//! asymmetric on purpose — that asymmetry is what makes even harmonics
//! rather than odd, and even harmonics are the difference between warmth
//! and distortion. A curve drawn only for positive inputs would be a
//! picture of the device with its distinguishing feature cropped out.
//!
//! The two remaining colour controls are spectral: `fuzz` and `brighten`
//! do their work across frequency, not across level, and an amplitude
//! plot cannot honestly show either. They keep their cells and their
//! tooltips, and the curve does not pretend to draw them.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, limiter as lp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimiterUi {
    pub push: f32,
    pub ceiling: f32,
    pub style: f32,
    pub release: f32,
    pub warmth: f32,
    pub fuzz: f32,
    pub brighten: f32,
}

impl Default for LimiterUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// device agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(lp::TABLE, id).default)
    }
}

impl LimiterUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| limiter_norm(id, get(id));
        Self {
            push: at(lp::PUSH),
            ceiling: at(lp::CEILING),
            style: at(lp::STYLE),
            release: at(lp::RELEASE),
            warmth: at(lp::WARMTH),
            fuzz: at(lp::FUZZ),
            brighten: at(lp::BRIGHTEN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            lp::PUSH => &mut self.push,
            lp::CEILING => &mut self.ceiling,
            lp::STYLE => &mut self.style,
            lp::RELEASE => &mut self.release,
            lp::WARMTH => &mut self.warmth,
            lp::FUZZ => &mut self.fuzz,
            lp::BRIGHTEN => &mut self.brighten,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            lp::PUSH => self.push,
            lp::CEILING => self.ceiling,
            lp::STYLE => self.style,
            lp::RELEASE => self.release,
            lp::WARMTH => self.warmth,
            lp::FUZZ => self.fuzz,
            _ => self.brighten,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 7] = [
    lp::PUSH,
    lp::CEILING,
    lp::STYLE,
    lp::RELEASE,
    lp::WARMTH,
    lp::FUZZ,
    lp::BRIGHTEN,
];

/// One control by wire id. The single place an id becomes a `Param`.
///
/// NATURAL UNITS end to end: decibels, milliseconds and per cent, never
/// `0..1`.
fn param_of(param: u32) -> Param {
    let def = params::def(lp::TABLE, param);
    match param {
        // Both level controls are already IN dB, so they map linearly in
        // it — `Mapping::Db` is for a table that stores amplitude, and
        // this one does not.
        lp::PUSH => Param::new(
            "push",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Db,
        )
        .with_default(def.default),
        lp::CEILING => Param::new(
            "ceiling",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Db,
        )
        .with_default(def.default),
        lp::STYLE => Param::choice("style", lp::STYLE_NAMES).with_default(def.default),
        lp::RELEASE => Param::new(
            "release",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Ms,
        )
        .with_default(def.default),
        lp::WARMTH => Param::percent("warmth").with_default(def.default * 100.0),
        lp::FUZZ => Param::percent("fuzz").with_default(def.default * 100.0),
        _ => Param::percent("brighten").with_default(def.default * 100.0),
    }
}

/// What the widget SHOWS for an engine value. The three colour knobs are
/// stored `0..1` and read as percentages.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        lp::WARMTH | lp::FUZZ | lp::BRIGHTEN => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        lp::WARMTH | lp::FUZZ | lp::BRIGHTEN => value / 100.0,
        lp::STYLE => value.round(),
        _ => value,
    };
    params::def(lp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn limiter_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`limiter_value`], for a state stored in engine units.
pub fn limiter_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. The style does.
pub fn limiter_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. Only the release: the two
/// level controls are already in dB, which is a log scale written down.
pub fn limiter_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// How a value reads on the card, for the editor's own rows.
pub fn limiter_format(param: u32, value: f32) -> String {
    let spec = param_of(param);
    spec.unit.format(shown(param, value))
}

/// How many settings a parameter has, if it is discrete.
pub fn limiter_choices(param: u32) -> Option<u32> {
    param_of(param).choices()
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn limiter_edits(state: &LimiterUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: limiter_value(param, state.get(param)),
        })
        .collect()
}

/// Decibels to linear amplitude. The node's own conversion, restated.
fn db_to_amp(db: f32) -> f32 {
    if db.is_finite() {
        10.0f32.powf(db / 20.0)
    } else {
        1.0
    }
}

/// How far out the curve is drawn, in linear amplitude.
///
/// Past full scale on both axes, because the interesting part of a
/// limiter's curve is what happens to a signal that is ALREADY too loud —
/// a plot stopping at 1.0 would show the ceiling and none of the road up
/// to it.
const PLOT_SPAN: f32 = 1.6;

/// The warmth stage's asymmetric soft clip, in the shaper kernel's own
/// terms.
///
/// Restated here rather than imported, the way every other curve on a
/// card is: a widget module must not reach into the engine. That the two
/// agree is not left to inspection — `audio::limiter`'s tests walk this
/// function against the kernel the node actually runs.
///
/// The bias is added AFTER the drive multiply, because that is where the
/// kernel adds it: `(x · drive + bias).tanh()`. Adding it before instead
/// draws a curve with the same character and the wrong numbers, which is
/// the kind of near-miss a picture is least likely to give away.
///
/// Subtracting the value at zero is the DC BLOCKER the node runs
/// immediately after this stage. An asymmetric curve sits off-centre, and
/// the device does not emit that offset — so a plot that showed it would
/// be drawing a signal the node never produces.
fn warm_shape(x: f32, drive: f32, bias: f32) -> f32 {
    let at = |v: f32| (v * drive + bias).tanh();
    at(x) - at(0.0)
}

/// What the device does to a steady input of `x`, ignoring everything
/// that acts across frequency or across time.
///
/// Push, then the warmth curve, then the ceiling. The gain reduction
/// itself shows up here as the ceiling and nothing else, and that is
/// honest: a limiter's release is a behaviour over TIME, and a curve of
/// level against level has no time in it to draw it with.
pub fn transfer(state: &LimiterUi, x: f32) -> f32 {
    let push = db_to_amp(limiter_value(lp::PUSH, state.push));
    let ceiling = db_to_amp(limiter_value(lp::CEILING, state.ceiling));
    let warmth = limiter_value(lp::WARMTH, state.warmth).clamp(0.0, 1.0);

    let mut y = x * push;
    if warmth > 0.0 {
        // The node's own constants: drive rides the knob from unity, and
        // the bias — the asymmetry — is half the shaper's maximum at full
        // warmth.
        y = warm_shape(y, 1.0 + warmth * WARMTH_DRIVE, warmth * WARMTH_BIAS);
    }
    y.clamp(-ceiling, ceiling)
}

/// The node's `WARMTH_DRIVE`, restated. See [`warm_shape`].
const WARMTH_DRIVE: f32 = 1.4;
/// The node's `WARMTH_BIAS`, restated. See [`warm_shape`].
const WARMTH_BIAS: f32 = 0.5;

/// Draw the transfer curve: the hero.
///
/// The unity diagonal sits behind it dimly, so how far the curve has been
/// bent away from "does nothing" is readable at a glance, and the ceiling
/// is a horizontal guide the curve visibly runs into.
fn curve(ui: &mut egui::Ui, theme: &Theme, state: &LimiterUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);

    // Both axes run -PLOT_SPAN..PLOT_SPAN with zero in the middle.
    let to_x = |v: f32| rect.left() + (v / PLOT_SPAN * 0.5 + 0.5) * rect.width();
    let to_y = |v: f32| rect.bottom() - (v / PLOT_SPAN * 0.5 + 0.5) * rect.height();

    // The axes through the origin.
    let zero_y = to_y(0.0);
    let zero_x = to_x(0.0);
    let hair = egui::Stroke::new(stroke::HAIR, theme.grid_beat);
    painter.line_segment(
        [
            egui::pos2(rect.left(), zero_y),
            egui::pos2(rect.right(), zero_y),
        ],
        hair,
    );
    painter.line_segment(
        [
            egui::pos2(zero_x, rect.top()),
            egui::pos2(zero_x, rect.bottom()),
        ],
        hair,
    );

    // The unity diagonal: what "does nothing" would look like.
    painter.line_segment(
        [
            egui::pos2(to_x(-PLOT_SPAN), to_y(-PLOT_SPAN)),
            egui::pos2(to_x(PLOT_SPAN), to_y(PLOT_SPAN)),
        ],
        egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
    );

    // The ceiling, as the rails the curve runs into.
    let ceiling = db_to_amp(limiter_value(lp::CEILING, state.ceiling));
    for rail in [ceiling, -ceiling] {
        painter.line_segment(
            [
                egui::pos2(rect.left(), to_y(rail)),
                egui::pos2(rect.right(), to_y(rail)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
        );
    }

    // The curve itself.
    let mut points = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let x = -PLOT_SPAN + along * PLOT_SPAN * 2.0;
        points.push(egui::pos2(to_x(x), to_y(transfer(state, x))));
    }
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));
}

/// The value strip's rows: what it DOES to the level, then what it does
/// to the sound.
const ROWS: [&[u32]; 2] = [
    &[lp::PUSH, lp::CEILING, lp::STYLE, lp::RELEASE],
    &[lp::WARMTH, lp::FUZZ, lp::BRIGHTEN],
];

/// How many `POLY_CELL_H` units one row of labelled cells needs.
///
/// TWO: a labelled cell prints its value on one line and its name on the
/// next, so a row given one unit prints them through each other.
const CELL_UNITS: usize = 2;

/// How many rows of height the footer reserves.
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

/// The NARROWEST a cell may be drawn.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its WIDEST ROW at its narrowest.
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

/// Draw the limiter card. Returns the edits the user just made.
pub fn limiter_card(ui: &mut egui::Ui, theme: &Theme, state: &mut LimiterUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "limiter", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => curve(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut LimiterUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, with the gaps taken off FIRST —
    // dividing the whole height by the row count hands each row a share
    // of the gaps as well and walks them down the card until they
    // overlap.
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
                // cell by cell, rather than divided up in advance.
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
                        // The style gets the STEPPED cell: a switch
                        // sweeping smoothly through settings it cannot
                        // take would be a control lying about what it
                        // does.
                        let moved = if limiter_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: limiter_value(*param, *norm),
                            });
                        }
                    },
                );
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// EVERY ROW ROUND-TRIPS between engine units and knob positions.
    #[test]
    fn every_parameter_round_trips_through_the_card() {
        for def in lp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = limiter_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = limiter_norm(def.id, value);
                let again = limiter_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults — and for this device
    /// that means it opens ALREADY COLOURED. A character limiter that did
    /// nothing until a knob moved would be a safety device wearing the
    /// wrong label.
    #[test]
    fn the_cards_defaults_match_the_engine_table_and_are_not_neutral() {
        let ui = LimiterUi::default();
        for def in lp::TABLE {
            let shown = limiter_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
        for colour in [lp::WARMTH, lp::FUZZ, lp::BRIGHTEN] {
            assert!(
                limiter_value(colour, ui.get(colour)) > 0.0,
                "the device loads transparent"
            );
        }
        assert!(limiter_value(lp::PUSH, ui.push) > 0.0, "and unpushed");
        // And the colours are ordered the way the brief asks: warmth is
        // subtle, the fuzz subtler still.
        assert!(
            limiter_value(lp::WARMTH, ui.warmth) > limiter_value(lp::FUZZ, ui.fuzz),
            "the fuzz is not subtler than the warmth"
        );
    }

    /// Every parameter is REACHABLE, exactly once.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), lp::TABLE.len(), "a parameter is missing");
        for def in lp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), lp::TABLE.len());
        assert_eq!(limiter_edits(&LimiterUi::default()).len(), lp::TABLE.len());
    }

    /// THE STYLE IS A NAMED SWITCH and every one of its settings is
    /// reachable; nothing else on the card snaps.
    #[test]
    fn the_style_steps_through_its_names_and_nothing_else_does() {
        assert!(limiter_is_discrete(lp::STYLE));
        assert_eq!(
            limiter_choices(lp::STYLE),
            Some(lp::STYLE_NAMES.len() as u32)
        );
        for (i, name) in lp::STYLE_NAMES.iter().enumerate() {
            let spec = param_of(lp::STYLE);
            let norm = spec.at_index(i);
            let value = limiter_value(lp::STYLE, norm);
            assert_eq!(value, i as f32, "{name} is not reachable");
            assert!(limiter_format(lp::STYLE, value).contains(name));
        }
        for def in lp::TABLE {
            if def.id != lp::STYLE {
                assert!(
                    !limiter_is_discrete(def.id),
                    "{} is continuous but snaps",
                    def.name
                );
            }
        }
        // The release sweeps in octaves; the two dB controls do not need
        // to, because dB is already a log scale written down.
        assert!(limiter_is_log(lp::RELEASE));
        assert!(!limiter_is_log(lp::PUSH));
        assert!(!limiter_is_log(lp::CEILING));
    }

    /// THE CURVE NEVER LEAVES THE CEILING, whatever any control says.
    /// The device's one promise, restated by its own picture — a plot
    /// showing the signal above the ceiling would be a plot of a device
    /// that leaks.
    #[test]
    fn the_drawn_curve_respects_the_drawn_ceiling() {
        for push in [0.0f32, 12.0, lp::PUSH_MAX_DB] {
            for ceiling_db in [0.0f32, -0.3, -6.0, lp::CEILING_MIN_DB] {
                for warmth in [0.0f32, 0.35, 1.0] {
                    let state = LimiterUi {
                        push: limiter_norm(lp::PUSH, push),
                        ceiling: limiter_norm(lp::CEILING, ceiling_db),
                        warmth: limiter_norm(lp::WARMTH, warmth),
                        ..LimiterUi::default()
                    };
                    let ceiling = db_to_amp(ceiling_db);
                    for i in 0..=400 {
                        let x = -PLOT_SPAN + i as f32 * PLOT_SPAN / 200.0;
                        let y = transfer(&state, x);
                        assert!(
                            y.abs() <= ceiling + 1e-6,
                            "push {push} ceiling {ceiling_db} warmth {warmth} at {x}: {y}"
                        );
                        assert!(y.is_finite());
                    }
                }
            }
        }
    }

    /// THE CURVE SHOWS WHAT THE KNOBS DO: push steepens it, and warmth
    /// makes it ASYMMETRIC. The asymmetry is the whole reason the plot is
    /// drawn through the origin, so it is the thing worth pinning.
    #[test]
    fn the_curve_steepens_with_push_and_bends_asymmetrically_with_warmth() {
        // A small input, well clear of the ceiling, so what is measured
        // is the shape and not the clamp.
        let probe = 0.15f32;

        let at_push = |db: f32| {
            let state = LimiterUi {
                push: limiter_norm(lp::PUSH, db),
                warmth: limiter_norm(lp::WARMTH, 0.0),
                ..LimiterUi::default()
            };
            transfer(&state, probe)
        };
        assert!(
            at_push(12.0) > at_push(0.0) * 2.0,
            "push did not steepen the curve"
        );

        // At zero warmth the curve is ODD — f(-x) == -f(x) — and with
        // warmth it is not. That difference IS the even-harmonic content.
        let cold = LimiterUi {
            push: limiter_norm(lp::PUSH, 0.0),
            warmth: limiter_norm(lp::WARMTH, 0.0),
            ..LimiterUi::default()
        };
        let warm = LimiterUi {
            warmth: limiter_norm(lp::WARMTH, 1.0),
            ..cold
        };
        for i in 1..=20 {
            let x = i as f32 * 0.05;
            let odd = (transfer(&cold, x) + transfer(&cold, -x)).abs();
            assert!(odd < 1e-6, "the cold curve is not odd at {x}: {odd}");
        }
        let bend: f32 = (1..=20)
            .map(|i| {
                let x = i as f32 * 0.05;
                (transfer(&warm, x) + transfer(&warm, -x)).abs()
            })
            .fold(0.0, f32::max);
        assert!(
            bend > 1e-3,
            "the warmth did not bend the two halves apart: {bend}"
        );

        // And through the origin it still passes through zero — an
        // asymmetric curve with an offset would be a DC generator, which
        // is what the node's DC blocker is there to catch.
        assert!(transfer(&warm, 0.0).abs() < 1e-6);
    }

    /// THE FOOTER RESERVES TWO LINES PER ROW OF CELLS, and the plot keeps
    /// enough height to be a picture.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, ROWS.len() * CELL_UNITS);

        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let needed = unit * CELL_UNITS as f32 * ROWS.len() as f32 + gap * (ROWS.len() - 1) as f32;
        assert!(
            reserved >= needed,
            "the footer reserves {reserved} for rows needing {needed}"
        );
        let plot = theme.sp(control::DEVICE_TALL_H) - reserved;
        assert!(plot > unit * 3.0, "the plot was squeezed to {plot}");
    }

    /// EVERY ROW FITS THE WIDTH THE CARD ASKS FOR.
    #[test]
    fn every_row_fits_the_width_the_card_asks_for() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let declared = face_width(ui, &theme);
                assert!(declared > 0.0, "the card declared no width at all");
                for row in ROWS {
                    let natural: f32 = row
                        .iter()
                        .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                        .sum::<f32>()
                        + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                    assert!(
                        natural <= declared + 0.5,
                        "a row needs {natural} but the card asks for {declared}"
                    );
                    for id in row {
                        let param = param_of(*id);
                        let wanted = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
                        assert!(
                            cell_min_width(ui, &theme, &param) >= wanted,
                            "{} cannot print its own value",
                            param.name
                        );
                    }
                }
            },
        );
        run.textures_delta.clear();
    }

    /// The card draws headlessly and emits nothing at rest.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = LimiterUi::default();
        let before = state;
        let mut edits = Vec::new();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                edits = limiter_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
