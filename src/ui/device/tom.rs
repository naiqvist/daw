//! The tom's card.
//!
//! Its hero is the PITCH BEND against the AMPLITUDE, drawn on one time
//! axis: the short drop the stick puts into the head, over the long decay
//! the shell rings out with. That relationship is the tom — bend too far
//! or too long and the card is showing a kick, which is the mistake this
//! picture exists to make visible before the ears have to.
//!
//! The simplest card in the rack, and deliberately so. Nine rows fit one
//! strip; the reason to reach for a tom instead of a kick is that a tom
//! is the one you do not have to think about, and a panel that argues
//! otherwise is the wrong panel.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, tom as tp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TomUi {
    pub tune: f32,
    pub decay: f32,
    pub bend: f32,
    pub bend_time: f32,
    pub stick: f32,
    pub stick_time: f32,
    pub tone: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for TomUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// voice agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(tp::TABLE, id).default)
    }
}

impl TomUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| tom_norm(id, get(id));
        Self {
            tune: at(tp::TUNE),
            decay: at(tp::DECAY),
            bend: at(tp::BEND),
            bend_time: at(tp::BEND_TIME),
            stick: at(tp::STICK),
            stick_time: at(tp::STICK_DECAY),
            tone: at(tp::TONE),
            drive: at(tp::DRIVE),
            gain: at(tp::GAIN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            tp::TUNE => &mut self.tune,
            tp::DECAY => &mut self.decay,
            tp::BEND => &mut self.bend,
            tp::BEND_TIME => &mut self.bend_time,
            tp::STICK => &mut self.stick,
            tp::STICK_DECAY => &mut self.stick_time,
            tp::TONE => &mut self.tone,
            tp::DRIVE => &mut self.drive,
            tp::GAIN => &mut self.gain,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            tp::TUNE => self.tune,
            tp::DECAY => self.decay,
            tp::BEND => self.bend,
            tp::BEND_TIME => self.bend_time,
            tp::STICK => self.stick,
            tp::STICK_DECAY => self.stick_time,
            tp::TONE => self.tone,
            tp::DRIVE => self.drive,
            _ => self.gain,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 9] = [
    tp::TUNE,
    tp::DECAY,
    tp::BEND,
    tp::BEND_TIME,
    tp::STICK,
    tp::STICK_DECAY,
    tp::TONE,
    tp::DRIVE,
    tp::GAIN,
];

/// One control by wire id. The single place an id becomes a `Param`.
///
/// NATURAL UNITS end to end: hertz, milliseconds and semitones, never
/// `0..1`. Times and frequencies are LOG.
fn param_of(param: u32) -> Param {
    let def = params::def(tp::TABLE, param);
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
        tp::TUNE => log("tune", Unit::Hz),
        tp::DECAY => log("decay", Unit::Ms),
        tp::BEND_TIME => log("bendtime", Unit::Ms),
        tp::STICK_DECAY => log("sticktime", Unit::Ms),
        tp::TONE => log("tone", Unit::Hz),
        tp::BEND => linear("bend", Unit::Semitones),
        tp::STICK => Param::percent("stick").with_default(def.default * 100.0),
        tp::DRIVE => log("drive", Unit::Ratio),
        _ => linear("gain", Unit::Plain),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        tp::STICK => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        tp::STICK => value / 100.0,
        _ => value,
    };
    params::def(tp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn tom_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`tom_value`], for a state stored in engine units.
pub fn tom_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. None of the tom's do.
pub fn tom_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale.
pub fn tom_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn tom_edits(state: &TomUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: tom_value(param, state.get(param)),
        })
        .collect()
}

/// How far the plot looks, in milliseconds. Long enough to show a floor
/// tom's tail settling, short enough that a 5 ms bend is more than one
/// pixel.
const PLOT_MS: f32 = 800.0;

/// `ln(1000)` — the kernel's own constant, because `ExpDecay`'s figure is
/// a time to fall 60 dB.
const DECADES: f32 = 6.907_755_4;

/// An exponential decay's value at `ms`, `0..=1`.
///
/// The node's own arithmetic, restated. If the drawing and the voice ever
/// disagree the card is a picture of a different drum than the one
/// sounding, which is the one thing this display must never be.
fn decay_at(total_ms: f32, ms: f32) -> f32 {
    let total = total_ms.max(0.01);
    (-DECADES * ms / total).exp()
}

/// The bend at `ms`, in semitones above the tuned note.
fn semitones_at(state: &TomUi, ms: f32) -> f32 {
    let depth = tom_value(tp::BEND, state.bend);
    let time = tom_value(tp::BEND_TIME, state.bend_time);
    depth * decay_at(time, ms)
}

/// The amplitude envelope at `ms`, `0..=1`.
fn amp_at(state: &TomUi, ms: f32) -> f32 {
    decay_at(tom_value(tp::DECAY, state.decay), ms)
}

/// Draw the bend against the amplitude: the hero.
///
/// The BEND bold, the AMPLITUDE quieter behind it, on one time axis so
/// the two lengths can be compared at a glance — which is the comparison
/// that decides whether a patch is a tom or a kick.
fn trajectory(ui: &mut egui::Ui, theme: &Theme, state: &TomUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);

    // The tuned note's line, which the bend falls onto.
    let floor = rect.bottom() - stroke::HAIR;
    painter.line_segment(
        [
            egui::pos2(rect.left(), floor),
            egui::pos2(rect.right(), floor),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );

    // Scaled against the depth as it is now, with a floor so an unbent
    // tom draws a flat line rather than dividing by nothing.
    let ceiling = tom_value(tp::BEND, state.bend).max(1.0);

    let mut amp = Vec::with_capacity(columns);
    let mut bend = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let ms = along * PLOT_MS;
        let x = rect.left() + along * rect.width();
        let up = |v: f32| rect.bottom() - v.clamp(0.0, 1.0) * rect.height() * 0.9;
        amp.push(egui::pos2(x, up(amp_at(state, ms))));
        bend.push(egui::pos2(x, up(semitones_at(state, ms) / ceiling)));
    }
    painter.add(egui::Shape::line(
        amp,
        egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
    ));
    painter.add(egui::Shape::line(
        bend,
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));
}

/// The value strip's rows: what the drum IS, then what the stick and the
/// skin do to it.
const ROWS: [&[u32]; 2] = [
    &[tp::TUNE, tp::DECAY, tp::BEND, tp::BEND_TIME, tp::GAIN],
    &[tp::STICK, tp::STICK_DECAY, tp::TONE, tp::DRIVE],
];

/// How many `POLY_CELL_H` units one row of labelled cells needs.
///
/// TWO: a labelled cell prints its value on one line and its name on the
/// next, so a row given one unit prints them through each other.
const CELL_UNITS: usize = 2;

/// How many rows of height the footer reserves.
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

/// The NARROWEST a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over.
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

/// Draw the tom card. Returns the edits the user just made.
pub fn tom_card(ui: &mut egui::Ui, theme: &Theme, state: &mut TomUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "tom", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => trajectory(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut TomUi, edits: &mut Vec<ParamEdit>) {
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
                        let moved = if tom_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: tom_value(*param, *norm),
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
        for def in tp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = tom_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = tom_norm(def.id, value);
                let again = tom_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults.
    #[test]
    fn the_cards_defaults_match_the_engine_table() {
        let ui = TomUi::default();
        for def in tp::TABLE {
            let shown = tom_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Every parameter is REACHABLE, exactly once.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), tp::TABLE.len(), "a parameter is missing");
        for def in tp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), tp::TABLE.len());
        assert_eq!(tom_edits(&TomUi::default()).len(), tp::TABLE.len());
    }

    /// THE TIMES AND FREQUENCIES SWEEP IN OCTAVES, and nothing on this
    /// card is a count, so nothing snaps.
    #[test]
    fn the_times_and_frequencies_are_log_and_nothing_steps() {
        for id in [
            tp::TUNE,
            tp::DECAY,
            tp::BEND_TIME,
            tp::STICK_DECAY,
            tp::TONE,
        ] {
            assert!(tom_is_log(id), "{id} should sweep in octaves");
        }
        assert!(!tom_is_log(tp::BEND), "semitones are linear");
        for def in tp::TABLE {
            assert!(
                !tom_is_discrete(def.id),
                "{} is continuous but snaps",
                def.name
            );
        }
    }

    /// THE PICTURE IS THE VOICE'S OWN ARITHMETIC: the bend falls
    /// exponentially and lands on the tuned note, and it only ever falls.
    #[test]
    fn the_bend_falls_onto_the_tuned_note() {
        assert!((DECADES - 1_000.0f32.ln()).abs() < 1e-4);

        let state = TomUi {
            bend: tom_norm(tp::BEND, 12.0),
            bend_time: tom_norm(tp::BEND_TIME, 40.0),
            ..Default::default()
        };

        // Full depth at the strike, a thousandth of it at the stated
        // time — the kernel's own 60 dB convention.
        assert!((semitones_at(&state, 0.0) - 12.0).abs() < 0.1);
        assert!(semitones_at(&state, 40.0) < 0.02);
        assert!(semitones_at(&state, 400.0) < 1e-4);

        // And it never rises.
        let mut previous = f32::MAX;
        for step in 0..=200 {
            let now = semitones_at(&state, step as f32 * 4.0);
            assert!(now <= previous + 1e-6, "the pitch rose at {step}");
            previous = now;
        }
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
        let mut state = TomUi::default();
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
                edits = tom_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
