//! The snare drum's card.
//!
//! Its hero is THE TWO DECAYS, drawn over one another: the shell's and
//! the wires'. That picture is the whole device, because a snare is not a
//! drum with noise added — it is two sounds of different lengths, and
//! every argument about whether a patch is a snare or a tom with a hiss
//! on it is an argument about which of these two lines finishes first.
//!
//! The screen guide's rule is that every control shows what it does. Two
//! knobs marked "shell" and "snaptime", each printing a number of
//! milliseconds, tell you nothing about the relationship between them.
//! Drawn together they tell you nothing else.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, snare as sp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
///
/// Normalized rather than in engine units for the reason every other card
/// here is: the widget speaks positions, the engine speaks hertz and
/// milliseconds, and one conversion in one place is what keeps them from
/// disagreeing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnareUi {
    pub tune: f32,
    pub ratio: f32,
    pub shell: f32,
    pub bend: f32,
    pub bend_time: f32,
    pub snap: f32,
    pub snap_time: f32,
    pub noise: f32,
    pub width: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for SnareUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// voice agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(sp::TABLE, id).default)
    }
}

impl SnareUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| snare_norm(id, get(id));
        Self {
            tune: at(sp::TUNE),
            ratio: at(sp::RATIO),
            shell: at(sp::TONE_DECAY),
            bend: at(sp::BEND),
            bend_time: at(sp::BEND_TIME),
            snap: at(sp::SNAP),
            snap_time: at(sp::SNAP_DECAY),
            noise: at(sp::NOISE_TONE),
            width: at(sp::NOISE_Q),
            drive: at(sp::DRIVE),
            gain: at(sp::GAIN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            sp::TUNE => &mut self.tune,
            sp::RATIO => &mut self.ratio,
            sp::TONE_DECAY => &mut self.shell,
            sp::BEND => &mut self.bend,
            sp::BEND_TIME => &mut self.bend_time,
            sp::SNAP => &mut self.snap,
            sp::SNAP_DECAY => &mut self.snap_time,
            sp::NOISE_TONE => &mut self.noise,
            sp::NOISE_Q => &mut self.width,
            sp::DRIVE => &mut self.drive,
            sp::GAIN => &mut self.gain,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            sp::TUNE => self.tune,
            sp::RATIO => self.ratio,
            sp::TONE_DECAY => self.shell,
            sp::BEND => self.bend,
            sp::BEND_TIME => self.bend_time,
            sp::SNAP => self.snap,
            sp::SNAP_DECAY => self.snap_time,
            sp::NOISE_TONE => self.noise,
            sp::NOISE_Q => self.width,
            sp::DRIVE => self.drive,
            _ => self.gain,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 11] = [
    sp::TUNE,
    sp::RATIO,
    sp::TONE_DECAY,
    sp::BEND,
    sp::BEND_TIME,
    sp::SNAP,
    sp::SNAP_DECAY,
    sp::NOISE_TONE,
    sp::NOISE_Q,
    sp::DRIVE,
    sp::GAIN,
];

/// One control by wire id. The single place an id becomes a `Param`, so
/// nothing below can disagree about which knob is which.
///
/// NATURAL UNITS end to end, the brief's rule: hertz, milliseconds and
/// semitones, never `0..1`. Times and frequencies are LOG, because the
/// difference between 20 ms and 40 is the whole character of a drum and
/// the difference between 700 and 720 is nothing at all.
fn param_of(param: u32) -> Param {
    let def = params::def(sp::TABLE, param);
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
        sp::TUNE => log("tune", Unit::Hz),
        sp::TONE_DECAY => log("shell", Unit::Ms),
        sp::SNAP_DECAY => log("wires", Unit::Ms),
        sp::BEND_TIME => log("bendtime", Unit::Ms),
        sp::NOISE_TONE => log("noise", Unit::Hz),
        sp::NOISE_Q => log("width", Unit::Plain),
        // The ratio is a MULTIPLE of the fundamental, so it reads as one:
        // "x1.83" says the second mode sits between the first and second
        // harmonic, which is the fact the knob exists to state.
        sp::RATIO => linear("ratio", Unit::Ratio),
        sp::BEND => linear("bend", Unit::Semitones),
        sp::SNAP => Param::percent("snap").with_default(def.default * 100.0),
        sp::DRIVE => log("drive", Unit::Ratio),
        _ => linear("gain", Unit::Plain),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        sp::SNAP => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        sp::SNAP => value / 100.0,
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn snare_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`snare_value`], for a state stored in engine units.
pub fn snare_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. None of the snare's
/// do — every row is a continuous quantity.
pub fn snare_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of a
/// decay moves in doublings exactly where the knob does.
pub fn snare_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn snare_edits(state: &SnareUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: snare_value(param, state.get(param)),
        })
        .collect()
}

/// How far the plot looks, in milliseconds. Long enough to show the
/// longest wire decay the table allows settling, short enough that a
/// 20 ms shell is more than one pixel.
const PLOT_MS: f32 = 600.0;

/// An exponential decay's value at `ms`, `0..=1`.
///
/// The node's own arithmetic, restated: `ExpDecay` falls 60 dB — to a
/// thousandth — over its stated time. If the drawing and the voice ever
/// disagree the card is a picture of a different drum than the one
/// sounding, which is the one thing this display must never be.
fn decay_at(total_ms: f32, ms: f32) -> f32 {
    let total = total_ms.max(0.01);
    (-DECADES * ms / total).exp()
}

/// `ln(1000)` — the kernel's own constant, because the decay figure is a
/// time to fall 60 dB.
const DECADES: f32 = 6.907_755_4;

/// Draw the two decays: the hero.
///
/// The SHELL bold, the WIRES behind it, on one grid and one time axis.
/// The wires are drawn at their actual mixed level rather than
/// normalized, because "how much snap" and "how long it rings" are the
/// two halves of one question and a normalized curve would answer only
/// the second.
fn decays(ui: &mut egui::Ui, theme: &Theme, state: &SnareUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);

    let floor = rect.bottom() - stroke::HAIR;
    painter.line_segment(
        [
            egui::pos2(rect.left(), floor),
            egui::pos2(rect.right(), floor),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );

    let shell_ms = snare_value(sp::TONE_DECAY, state.shell);
    let wires_ms = snare_value(sp::SNAP_DECAY, state.snap_time);
    let wires_level = snare_value(sp::SNAP, state.snap);

    let mut shell = Vec::with_capacity(columns);
    let mut wires = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let ms = along * PLOT_MS;
        let x = rect.left() + along * rect.width();
        let up = |v: f32| rect.bottom() - v.clamp(0.0, 1.0) * rect.height() * 0.9;
        shell.push(egui::pos2(x, up(decay_at(shell_ms, ms))));
        wires.push(egui::pos2(x, up(decay_at(wires_ms, ms) * wires_level)));
    }
    painter.add(egui::Shape::line(
        wires,
        egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
    ));
    painter.add(egui::Shape::line(
        shell,
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));
}

/// The value strip's rows, grouped the way the signal flows: the SHELL,
/// then the WIRES. The card's two halves are the plot's two lines, which
/// is the point — a row and a curve name the same thing.
const ROWS: [&[u32]; 2] = [
    &[
        sp::TUNE,
        sp::RATIO,
        sp::TONE_DECAY,
        sp::BEND,
        sp::BEND_TIME,
        sp::GAIN,
    ],
    &[
        sp::SNAP,
        sp::SNAP_DECAY,
        sp::NOISE_TONE,
        sp::NOISE_Q,
        sp::DRIVE,
    ],
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

/// Draw the snare card. Returns the edits the user just made.
pub fn snare_card(ui: &mut egui::Ui, theme: &Theme, state: &mut SnareUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "snare", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => decays(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut SnareUi, edits: &mut Vec<ParamEdit>) {
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
                // cell by cell, rather than divided up in advance: worked
                // out ahead, any cell needing more than its share spends
                // the row's remainder and the last one is pushed off the
                // card's right edge.
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
                        let moved = if snare_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: snare_value(*param, *norm),
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

    /// EVERY ROW ROUND-TRIPS between engine units and knob positions. A
    /// mapping that was not its own inverse would walk a value every time
    /// a project was saved and loaded.
    #[test]
    fn every_parameter_round_trips_through_the_card() {
        for def in sp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = snare_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = snare_norm(def.id, value);
                let again = snare_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults, so a fresh snare on
    /// screen is the fresh snare the engine builds.
    #[test]
    fn the_cards_defaults_match_the_engine_table() {
        let ui = SnareUi::default();
        for def in sp::TABLE {
            let shown = snare_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Every parameter is REACHABLE, exactly once. A row missing from the
    /// layout is a knob that exists in the engine and nowhere on screen;
    /// a row twice is two cells that disagree about one value.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), sp::TABLE.len(), "a parameter is missing");
        for def in sp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), sp::TABLE.len());
        assert_eq!(snare_edits(&SnareUi::default()).len(), sp::TABLE.len());
    }

    /// THE TIMES AND FREQUENCIES SWEEP IN OCTAVES. A decay knob that was
    /// linear would spend nine tenths of its travel between 600 ms and
    /// 1.2 s, where nothing changes.
    #[test]
    fn the_times_and_frequencies_are_log_and_nothing_steps() {
        for id in [
            sp::TUNE,
            sp::TONE_DECAY,
            sp::SNAP_DECAY,
            sp::BEND_TIME,
            sp::NOISE_TONE,
        ] {
            assert!(snare_is_log(id), "{id} should sweep in octaves");
        }
        assert!(!snare_is_log(sp::BEND), "semitones are linear");
        // Nothing on this card is a count, so nothing snaps.
        for def in sp::TABLE {
            assert!(
                !snare_is_discrete(def.id),
                "{} is continuous but snaps",
                def.name
            );
        }
    }

    /// THE PICTURE IS THE VOICE'S OWN ARITHMETIC. `ExpDecay` falls 60 dB
    /// over its stated time; if the drawing used a linear ramp the card
    /// would show a different drum than the one sounding.
    #[test]
    fn the_curve_is_the_kernels_own_exponential() {
        // The kernel's constant, restated here and checked against it.
        assert!((DECADES - 1_000.0f32.ln()).abs() < 1e-4);

        // 60 dB down at the stated time — a thousandth, not zero.
        assert!((decay_at(100.0, 100.0) - 0.001).abs() < 1e-4);
        assert_eq!(decay_at(100.0, 0.0), 1.0);
        // And nearly gone at HALF the time, where a linear ramp would
        // still be at 0.5. That difference is the whole reason the
        // exponential kernel exists.
        assert!(decay_at(100.0, 50.0) < 0.05);

        // It only ever falls.
        let mut previous = f32::MAX;
        for step in 0..=200 {
            let now = decay_at(180.0, step as f32 * 3.0);
            assert!(now <= previous + 1e-6, "the decay rose at {step}");
            previous = now;
        }
    }

    /// THE FOOTER RESERVES TWO LINES PER ROW OF CELLS, and the plot keeps
    /// enough height to be a picture. The panel's own formula, restated.
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
        // And the plot — the hero — keeps more than three units of it.
        let plot = theme.sp(control::DEVICE_TALL_H) - reserved;
        assert!(plot > unit * 3.0, "the plot was squeezed to {plot}");
    }

    /// EVERY ROW FITS THE WIDTH THE CARD ASKS FOR, and every cell can
    /// print its own widest reading — measured in a headless frame rather
    /// than guessed at.
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

    /// The card draws headlessly and emits nothing at rest — the standing
    /// card test, and the one that catches a control that moves on its
    /// own.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SnareUi::default();
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
                edits = snare_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
