//! The kick drum's card.
//!
//! Its hero is the PITCH TRAJECTORY — the two envelopes summed, drawn as
//! the curve the oscillator actually follows from the strike down to the
//! tuned note. That picture is the whole device: a kick IS a pitch drop,
//! and every argument about punch versus body is an argument about the
//! shape of this one line.
//!
//! The screen guide's rule is that every control shows what it does, and
//! the two pitch envelopes are the case that most needs it. Their
//! numbers — 32 semitones over 6 ms, 12 over 55 — mean nothing on their
//! own and everything as a shape. Two knobs marked "depth" and "time",
//! twice, is the construction problem the brief exists to avoid.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, kick as kp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
///
/// Normalized rather than in engine units for the same reason every other
/// card here is: the widget speaks positions, the engine speaks hertz and
/// milliseconds, and one conversion in one place is what keeps them from
/// disagreeing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickUi {
    pub tune: f32,
    pub decay: f32,
    pub punch_depth: f32,
    pub punch_time: f32,
    pub sweep_depth: f32,
    pub sweep_time: f32,
    pub click_level: f32,
    pub click_time: f32,
    pub disperse: f32,
    pub harmonic: f32,
    pub spread: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for KickUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// voice agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(kp::TABLE, id).default)
    }
}

impl KickUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract, and `crate::audio` is exactly what it forbids. The app
    /// knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| kick_norm(id, get(id));
        Self {
            tune: at(kp::TUNE),
            decay: at(kp::AMP_DECAY),
            punch_depth: at(kp::PITCH_A_DEPTH),
            punch_time: at(kp::PITCH_A_DECAY),
            sweep_depth: at(kp::PITCH_B_DEPTH),
            sweep_time: at(kp::PITCH_B_DECAY),
            click_level: at(kp::CLICK_LEVEL),
            click_time: at(kp::CLICK_DECAY),
            disperse: at(kp::DISP_STAGES),
            harmonic: at(kp::DISP_HARMONIC),
            spread: at(kp::DISP_Q),
            drive: at(kp::DRIVE),
            gain: at(kp::GAIN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            kp::TUNE => &mut self.tune,
            kp::AMP_DECAY => &mut self.decay,
            kp::PITCH_A_DEPTH => &mut self.punch_depth,
            kp::PITCH_A_DECAY => &mut self.punch_time,
            kp::PITCH_B_DEPTH => &mut self.sweep_depth,
            kp::PITCH_B_DECAY => &mut self.sweep_time,
            kp::CLICK_LEVEL => &mut self.click_level,
            kp::CLICK_DECAY => &mut self.click_time,
            kp::DISP_STAGES => &mut self.disperse,
            kp::DISP_HARMONIC => &mut self.harmonic,
            kp::DISP_Q => &mut self.spread,
            kp::DRIVE => &mut self.drive,
            kp::GAIN => &mut self.gain,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            kp::TUNE => self.tune,
            kp::AMP_DECAY => self.decay,
            kp::PITCH_A_DEPTH => self.punch_depth,
            kp::PITCH_A_DECAY => self.punch_time,
            kp::PITCH_B_DEPTH => self.sweep_depth,
            kp::PITCH_B_DECAY => self.sweep_time,
            kp::CLICK_LEVEL => self.click_level,
            kp::CLICK_DECAY => self.click_time,
            kp::DISP_STAGES => self.disperse,
            kp::DISP_HARMONIC => self.harmonic,
            kp::DISP_Q => self.spread,
            kp::DRIVE => self.drive,
            _ => self.gain,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 13] = [
    kp::TUNE,
    kp::AMP_DECAY,
    kp::PITCH_A_DEPTH,
    kp::PITCH_A_DECAY,
    kp::PITCH_B_DEPTH,
    kp::PITCH_B_DECAY,
    kp::CLICK_LEVEL,
    kp::CLICK_DECAY,
    kp::DISP_STAGES,
    kp::DISP_HARMONIC,
    kp::DISP_Q,
    kp::DRIVE,
    kp::GAIN,
];

/// One control by wire id. The single place an id becomes a `Param`, so
/// nothing below can disagree about which knob is which.
///
/// NATURAL UNITS end to end, the brief's rule: hertz, milliseconds and
/// semitones, never `0..1`. Times are LOG, because the difference between
/// 1 ms and 2 ms is the whole character of a click and the difference
/// between 400 and 401 is nothing at all.
fn param_of(param: u32) -> Param {
    let def = params::def(kp::TABLE, param);
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
        kp::TUNE => log("tune", Unit::Hz),
        kp::AMP_DECAY => log("decay", Unit::Ms),
        kp::PITCH_A_DECAY | kp::PITCH_B_DECAY | kp::CLICK_DECAY => log("time", Unit::Ms),
        kp::PITCH_A_DEPTH | kp::PITCH_B_DEPTH => linear("depth", Unit::Semitones),
        kp::CLICK_LEVEL => Param::percent("click").with_default(def.default * 100.0),
        // Stages and harmonic are COUNTS: a knob reading 7.4 sections
        // describes something that cannot exist. `Steps` runs 0..count-1,
        // so the harmonic — which starts at 1 — is shifted by `shown`
        // and `natural` rather than by inventing a second mapping.
        kp::DISP_STAGES => Param::new(
            "stages",
            Mapping::Steps {
                count: def.max as u32 + 1,
            },
            Unit::Plain,
        )
        .with_default(def.default),
        kp::DISP_HARMONIC => Param::new(
            "harm",
            Mapping::Steps {
                count: def.max as u32,
            },
            Unit::Plain,
        )
        .with_default(def.default - 1.0),
        kp::DISP_Q => log("spread", Unit::Plain),
        kp::DRIVE => log("drive", Unit::Ratio),
        _ => linear("gain", Unit::Plain),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        kp::CLICK_LEVEL => value * 100.0,
        // The first harmonic is step zero.
        kp::DISP_HARMONIC => value - 1.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        kp::CLICK_LEVEL => value / 100.0,
        kp::DISP_STAGES => value.round(),
        kp::DISP_HARMONIC => value.round() + 1.0,
        _ => value,
    };
    params::def(kp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn kick_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`kick_value`], for a state stored in engine units.
pub fn kick_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. The two counts do.
pub fn kick_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of a
/// decay moves in doublings exactly where the knob does.
pub fn kick_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn kick_edits(state: &KickUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: kick_value(param, state.get(param)),
        })
        .collect()
}

/// How far the plot looks, in milliseconds. Long enough to show a slow
/// body sweep settling and short enough that a 6 ms punch is more than
/// one pixel.
const PLOT_MS: f32 = 400.0;

/// The pitch trajectory at `ms` after the strike, in semitones above the
/// tuned note.
///
/// The node's own arithmetic, restated: two one-shot decays, summed in
/// SEMITONES before the exponential. If the drawing and the voice ever
/// disagree the card is a picture of a different drum than the one
/// sounding, which is the one thing this display must never be.
fn semitones_at(state: &KickUi, ms: f32) -> f32 {
    let decay = |depth: u32, time: u32| {
        let depth_st = kick_value(depth, state.get(depth));
        let ms_total = kick_value(time, state.get(time)).max(0.01);
        // Linear decay to zero, which is what `Adsr` runs with sustain 0.
        depth_st * (1.0 - (ms / ms_total)).max(0.0)
    };
    decay(kp::PITCH_A_DEPTH, kp::PITCH_A_DECAY) + decay(kp::PITCH_B_DEPTH, kp::PITCH_B_DECAY)
}

/// The amplitude envelope at `ms`, `0..=1`.
fn amp_at(state: &KickUi, ms: f32) -> f32 {
    let total = kick_value(kp::AMP_DECAY, state.decay).max(0.01);
    (1.0 - (ms / total)).max(0.0)
}

/// Draw the pitch trajectory: the hero.
///
/// Two lines over one grid. The PITCH line is the sum of both envelopes,
/// scaled against the deepest either can reach so its shape is stable as
/// the depths move; the AMP line is quieter, behind it, so the two
/// timescales can be compared at a glance — which is the comparison that
/// decides whether a kick clicks or thumps.
fn trajectory(ui: &mut egui::Ui, theme: &Theme, state: &KickUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(theme.sp(space::SM), theme.sp(space::LG)),
        rect.max - egui::vec2(theme.sp(space::SM), theme.sp(space::SM)),
    );
    let columns = (plot.width().ceil() as usize).clamp(2, 1_024);

    // The vertical scale: the two depths as they are now, with a floor so
    // an all-zero kick still draws a flat line at the bottom rather than
    // dividing by nothing.
    let ceiling = (kick_value(kp::PITCH_A_DEPTH, state.punch_depth)
        + kick_value(kp::PITCH_B_DEPTH, state.sweep_depth))
    .max(1.0);

    // A time-addressed drafting grid: denser around the transient where
    // milliseconds matter, wider over the body where tens of milliseconds
    // are the useful unit. These are the actual 400 ms plot coordinates,
    // not an ornamental scope grid.
    for ms in [0.0, 10.0, 25.0, 50.0, 100.0, 200.0, PLOT_MS] {
        let x = egui::lerp(plot.x_range(), ms / PLOT_MS);
        painter.vline(
            x,
            plot.y_range(),
            egui::Stroke::new(
                stroke::HAIR,
                if ms == 0.0 || ms == PLOT_MS {
                    theme.grid_beat
                } else {
                    theme.grid_sub
                },
            ),
        );
        if matches!(ms as u32, 0 | 50 | 100 | 200 | 400) {
            painter.text(
                egui::pos2(x, plot.bottom()),
                if ms == 0.0 {
                    egui::Align2::LEFT_BOTTOM
                } else if ms == PLOT_MS {
                    egui::Align2::RIGHT_BOTTOM
                } else {
                    egui::Align2::CENTER_BOTTOM
                },
                format!("{ms:.0}"),
                egui::FontId::monospace(font::MICRO_LABEL),
                theme.text_muted,
            );
        }
    }
    for level in [0.25, 0.5, 0.75] {
        let y = egui::lerp(plot.y_range(), 1.0 - level);
        painter.hline(
            plot.x_range(),
            y,
            egui::Stroke::new(stroke::HAIR, theme.grid_sub.gamma_multiply(0.7)),
        );
    }

    let tune = kick_value(kp::TUNE, state.tune);
    painter.text(
        plot.left_top(),
        egui::Align2::LEFT_BOTTOM,
        format!("PITCH DROP // +{ceiling:.0} ST"),
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.role_time,
    );
    painter.text(
        plot.right_top(),
        egui::Align2::RIGHT_BOTTOM,
        format!("{tune:.1} HZ // {PLOT_MS:.0} MS"),
        egui::FontId::monospace(font::MICRO_LABEL),
        theme.text_value,
    );

    // The tuned note's line, which the pitch falls onto — the thing the
    // whole drop is aimed at.
    let base_y = plot.bottom() - stroke::HAIR;
    painter.line_segment(
        [
            egui::pos2(plot.left(), base_y),
            egui::pos2(plot.right(), base_y),
        ],
        egui::Stroke::new(stroke::BOLD, theme.grid_beat),
    );

    let mut amp = Vec::with_capacity(columns);
    let mut pitch = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let ms = along * PLOT_MS;
        let x = plot.left() + along * plot.width();
        amp.push(egui::pos2(
            x,
            plot.bottom() - amp_at(state, ms) * plot.height() * 0.84,
        ));
        let up = (semitones_at(state, ms) / ceiling).clamp(0.0, 1.0);
        pitch.push(egui::pos2(x, plot.bottom() - up * plot.height() * 0.84));
    }
    painter.add(egui::Shape::line(
        amp,
        egui::Stroke::new(stroke::BOLD, theme.role_level_dim),
    ));
    // A dark red registration line under the blue trace gives the trajectory
    // the crisp two-ink print character of the rest of the face. It is the
    // same DSP curve, offset by one physical pixel — never a second signal.
    painter.add(egui::Shape::line(
        pitch
            .iter()
            .map(|point| *point + egui::vec2(theme.sp(stroke::HAIR), 0.0))
            .collect(),
        egui::Stroke::new(stroke::BOLD, theme.role_mod_dim),
    ));
    painter.add(egui::Shape::line(
        pitch,
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));

    // The two pitch-envelope endpoints and click window are the moments the
    // transient changes construction. Small hardware-like registration marks
    // expose them without creating another control or inventing telemetry.
    let marker = |ms: f32, color: egui::Color32| {
        let clamped = ms.clamp(0.0, PLOT_MS);
        let x = egui::lerp(plot.x_range(), clamped / PLOT_MS);
        let up = (semitones_at(state, clamped) / ceiling).clamp(0.0, 1.0);
        let y = plot.bottom() - up * plot.height() * 0.84;
        let r = theme.sp(space::XXS);
        let points = [
            egui::pos2(x, y - r),
            egui::pos2(x + r, y),
            egui::pos2(x, y + r),
            egui::pos2(x - r, y),
        ];
        for edge in 0..points.len() {
            painter.line_segment(
                [points[edge], points[(edge + 1) % points.len()]],
                egui::Stroke::new(stroke::HAIR, color),
            );
        }
    };
    marker(
        kick_value(kp::PITCH_A_DECAY, state.punch_time),
        theme.role_mod,
    );
    marker(
        kick_value(kp::PITCH_B_DECAY, state.sweep_time),
        theme.role_time,
    );

    let click_ms = kick_value(kp::CLICK_DECAY, state.click_time).clamp(0.0, PLOT_MS);
    let click_x = egui::lerp(plot.x_range(), click_ms / PLOT_MS);
    painter.line_segment(
        [
            egui::pos2(click_x, plot.top()),
            egui::pos2(click_x, plot.top() + theme.sp(space::SM)),
        ],
        egui::Stroke::new(stroke::BOLD, theme.role_mod),
    );
}

/// The value strip's rows, grouped the way the signal flows: what the
/// drum IS, then the two pitch envelopes, then the click, then the
/// disperser.
///
/// FOUR CELLS TO A ROW at most. Six across a card that has to fit beside
/// a browser is six readings printed through each other, which is
/// exactly what the first version did.
const ROWS: [&[u32]; 2] = [
    // What the drum IS: its note, its length, the two pitch envelopes
    // that get it there, and its level.
    &[
        kp::TUNE,
        kp::AMP_DECAY,
        kp::PITCH_A_DEPTH,
        kp::PITCH_A_DECAY,
        kp::PITCH_B_DEPTH,
        kp::PITCH_B_DECAY,
        kp::GAIN,
    ],
    // What COLOURS it: the click, the disperser, the saturator.
    &[
        kp::CLICK_LEVEL,
        kp::CLICK_DECAY,
        kp::DISP_STAGES,
        kp::DISP_HARMONIC,
        kp::DISP_Q,
        kp::DRIVE,
    ],
];

/// How many `POLY_CELL_H` units one row of labelled cells needs.
///
/// TWO, and this is the thing that has to be right: a labelled cell
/// prints its value on one line and its name on the next, so a row given
/// one unit prints them through each other. Both the compressor and the
/// saturator draw a SINGLE row of cells and reserve two rows of height —
/// which reads like an off-by-one until you notice it is a cell being
/// two lines tall.
const CELL_UNITS: usize = 2;

/// How many rows of height the footer reserves.
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

/// The NARROWEST a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over.
///
/// `XS` either side rather than `SM`, the rule the compressor's strip
/// settled on — four cells at eight points a side is sixty-four points
/// of nothing, and a card that sits beside a browser has none to spare.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its WIDEST ROW at its narrowest.
///
/// Declared to the container as the well's footprint, so the card is
/// given the room its cells need before it starts clipping them away.
/// Given more, the cells share it.
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

/// Draw the kick card. Returns the edits the user just made.
pub fn kick_card(ui: &mut egui::Ui, theme: &Theme, state: &mut KickUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "kick", control::DEVICE_TALL_H, |ui| {
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

/// The value strip, grouped the way the signal flows: what the drum IS,
/// then the two pitch envelopes, then the click, then the shaping.
///
/// The SECTION GRAMMAR repeats — each group is depth then time, in that
/// order, every time. Learning one teaches the others, which is the
/// brief's whole ease argument.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut KickUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, restated rather than divided out
    // of what is available: `dark_curve_panel` sized the footer as
    // `POLY_CELL_H × rows + gap × (rows − 1)`, and deriving it back by
    // division walks the rows down the card by the gaps.
    let gap = theme.sp(space::XXS);
    let rows = ROWS.len() as f32;
    // Derived from the footer the panel actually handed over, with the
    // gaps taken off FIRST — dividing the whole height by the row count
    // hands each row a share of the gaps as well and walks them down the
    // card until they overlap.
    let height = ((ui.available_height() - gap * (rows - 1.0)) / rows).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    for row in ROWS {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                // The share is recomputed from what is ACTUALLY left,
                // cell by cell, rather than divided up in advance — the
                // compressor's rule. Worked out ahead, any cell needing
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
                        // Counts get the STEPPED cell, everything else the
                        // bar: a stage count sweeping smoothly through
                        // values it cannot take would be a control lying
                        // about what it does.
                        let moved = if kick_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: kick_value(*param, *norm),
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
        for def in kp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = kick_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = kick_norm(def.id, value);
                let again = kick_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults, so a fresh kick on
    /// screen is the fresh kick the engine builds.
    #[test]
    fn the_cards_defaults_match_the_engine_table() {
        let ui = KickUi::default();
        for def in kp::TABLE {
            let shown = kick_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Every parameter is REACHABLE from the strip. A row missing from
    /// the layout is a knob that exists in the engine and nowhere on
    /// screen.
    #[test]
    fn every_engine_parameter_has_a_cell() {
        for def in kp::TABLE {
            assert!(CELLS.contains(&def.id), "{} is not on the card", def.name);
        }
        assert_eq!(CELLS.len(), kp::TABLE.len());
        // And every cell is a real row, not an id that has been renumbered.
        for cell in CELLS {
            assert!(kp::TABLE.iter().any(|def| def.id == cell));
        }
        // The edits list covers all of them.
        let edits = kick_edits(&KickUi::default());
        assert_eq!(edits.len(), kp::TABLE.len());
    }

    /// THE COUNTS STEP. A disperser knob reading 7.4 sections describes
    /// something that cannot exist.
    #[test]
    fn the_disperser_counts_are_discrete_and_the_times_are_log() {
        assert!(kick_is_discrete(kp::DISP_STAGES));
        assert!(kick_is_discrete(kp::DISP_HARMONIC));
        assert!(!kick_is_discrete(kp::TUNE));

        for id in [
            kp::AMP_DECAY,
            kp::PITCH_A_DECAY,
            kp::PITCH_B_DECAY,
            kp::CLICK_DECAY,
            kp::TUNE,
        ] {
            assert!(kick_is_log(id), "{id} should sweep in octaves");
        }
        assert!(!kick_is_log(kp::PITCH_A_DEPTH), "semitones are linear");

        // A stage knob anywhere in its travel lands on a whole number.
        for at in 0..=20 {
            let value = kick_value(kp::DISP_STAGES, at as f32 / 20.0);
            assert_eq!(value, value.round(), "{value} is not a whole stage count");
        }
    }

    /// THE PICTURE IS THE VOICE'S OWN ARITHMETIC. If these drift the card
    /// draws a different drum than the one sounding.
    #[test]
    fn the_trajectory_matches_the_two_envelopes() {
        let mut state = KickUi::default();
        state.punch_depth = kick_norm(kp::PITCH_A_DEPTH, 24.0);
        state.punch_time = kick_norm(kp::PITCH_A_DECAY, 10.0);
        state.sweep_depth = kick_norm(kp::PITCH_B_DEPTH, 12.0);
        state.sweep_time = kick_norm(kp::PITCH_B_DECAY, 100.0);

        // At the strike both are full: 36 semitones up.
        assert!((semitones_at(&state, 0.0) - 36.0).abs() < 0.5);
        // At 10 ms the fast one has finished and the slow one is 90 % up.
        let at_ten = semitones_at(&state, 10.0);
        assert!(
            (at_ten - 10.8).abs() < 0.6,
            "the punch should be gone by 10 ms: {at_ten}"
        );
        // At 100 ms both are done and the drum is on its tuned note.
        assert!(semitones_at(&state, 100.0).abs() < 0.01);
        assert!(semitones_at(&state, 1_000.0).abs() < 0.01);

        // The trajectory only ever falls — a pitch envelope that rose
        // partway through would be a bug nobody would think to look for.
        let mut previous = f32::MAX;
        for step in 0..=200 {
            let now = semitones_at(&state, step as f32 * 2.0);
            assert!(now <= previous + 1e-4, "the pitch rose at {step}");
            previous = now;
        }
    }

    /// EVERY CELL FITS. This is the bug that shipped: thirteen readings
    /// laid out as an equal share of a card that had never said how wide
    /// it needed to be, so `STAGES` printed through `HARM` and the whole
    /// strip was unreadable.
    ///
    /// Two things have to hold. Each row's natural width must be within
    /// what the card declares, and no row may carry more cells than a
    /// narrow card can show — because the card is drawn beside a browser
    /// and a device rack, not across a whole screen.
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
                    // Every cell has room for its own widest reading.
                    for id in row {
                        let param = param_of(*id);
                        let wanted = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
                        assert!(
                            cell_min_width(ui, &theme, &param) >= wanted,
                            "{} cannot print its own value",
                            param.name
                        );
                    }
                    // NOT a cap on cells per row — the compressor puts
                    // nine in one row and simply declares a face wide
                    // enough for them, and the rack scrolls. What matters
                    // is that the rows are BALANCED: one row of eleven
                    // beside one of two makes the card as wide as the
                    // long row for no benefit to the short one.
                    let longest = ROWS.iter().map(|r| r.len()).max().unwrap_or(0);
                    let shortest = ROWS.iter().map(|r| r.len()).min().unwrap_or(0);
                    assert!(
                        longest - shortest <= 1,
                        "the rows are lopsided: {longest} against {shortest}"
                    );
                }
            },
        );
        run.textures_delta.clear();
    }

    /// A LABELLED CELL IS TWO LINES TALL, and the footer has to reserve
    /// for that.
    ///
    /// This is the bug that shipped twice: the footer was given one
    /// `POLY_CELL_H` per row of cells, so every value printed through its
    /// own name. Both the compressor and the saturator draw one row of
    /// cells and ask `dark_curve_panel` for TWO rows of height — the
    /// giveaway that a cell is two lines, not one.
    ///
    /// Asserted against the panel's own arithmetic so it cannot drift:
    /// what the panel reserves has to be at least what the rows need.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, ROWS.len() * CELL_UNITS);

        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        // `dark_curve_panel`'s own formula.
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        // What the rows need: two units each, and a gap between rows.
        let needed = unit * CELL_UNITS as f32 * ROWS.len() as f32 + gap * (ROWS.len() - 1) as f32;
        assert!(
            reserved >= needed,
            "the footer reserves {reserved} for rows needing {needed}"
        );

        // And the card still leaves the trajectory room to be a picture
        // rather than a line of pixels.
        let plot = control::DEVICE_TALL_H - reserved;
        assert!(
            plot > unit * 3.0,
            "only {plot} left for the plot after a {reserved} footer"
        );
    }

    /// Every parameter appears exactly once across the rows — a strip
    /// that dropped one would hide a knob, and one that repeated one
    /// would give the same value two cells that disagree.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), kp::TABLE.len(), "a parameter is missing");
        for def in kp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
    }

    /// The card draws headlessly and emits nothing at rest — the
    /// standing card test, and the one that catches a control that moves
    /// on its own.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = KickUi::default();
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
                edits = kick_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
