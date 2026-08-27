//! The 808 hi-hat's card.
//!
//! Its hero is THE BANK AND THE WINDOW: the six oscillator frequencies
//! drawn as standing lines on a log axis, with the filter response the
//! sound is actually heard through laid over them. That picture is the
//! whole device, because the 808 hat is exactly those two things — a
//! fixed, inharmonic comb, and a movable window onto it.
//!
//! It is also the only honest way to draw this instrument. A decay curve
//! would be a picture of an envelope, which every drum here has; what
//! makes this one an 808 is WHERE the six lines are, and that they do not
//! move relative to each other. Anyone dragging the band knob can watch
//! the window slide across a comb that stays put — which is the mental
//! model, and it happens to be literally true.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, hat as hp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatUi {
    pub tune: f32,
    pub closed: f32,
    pub open: f32,
    pub band: f32,
    pub width: f32,
    pub hp: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for HatUi {
    /// Every knob at the TABLE's default — which for this device means
    /// the machine: tune at unity puts the bank on its original six.
    fn default() -> Self {
        Self::from_engine(|id| params::def(hp::TABLE, id).default)
    }
}

impl HatUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| hat_norm(id, get(id));
        Self {
            tune: at(hp::TUNE),
            closed: at(hp::CLOSED_DECAY),
            open: at(hp::OPEN_DECAY),
            band: at(hp::BP_HZ),
            width: at(hp::BP_Q),
            hp: at(hp::HP_HZ),
            drive: at(hp::DRIVE),
            gain: at(hp::GAIN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            hp::TUNE => &mut self.tune,
            hp::CLOSED_DECAY => &mut self.closed,
            hp::OPEN_DECAY => &mut self.open,
            hp::BP_HZ => &mut self.band,
            hp::BP_Q => &mut self.width,
            hp::HP_HZ => &mut self.hp,
            hp::DRIVE => &mut self.drive,
            hp::GAIN => &mut self.gain,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            hp::TUNE => self.tune,
            hp::CLOSED_DECAY => self.closed,
            hp::OPEN_DECAY => self.open,
            hp::BP_HZ => self.band,
            hp::BP_Q => self.width,
            hp::HP_HZ => self.hp,
            hp::DRIVE => self.drive,
            _ => self.gain,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 8] = [
    hp::TUNE,
    hp::CLOSED_DECAY,
    hp::OPEN_DECAY,
    hp::BP_HZ,
    hp::BP_Q,
    hp::HP_HZ,
    hp::DRIVE,
    hp::GAIN,
];

/// One control by wire id. The single place an id becomes a `Param`.
fn param_of(param: u32) -> Param {
    let def = params::def(hp::TABLE, param);
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
    match param {
        // The tune is a MULTIPLIER on the whole bank, so it reads as one:
        // "x1.00" is the machine, and that is the fact worth printing.
        hp::TUNE => log("tune", Unit::Ratio),
        hp::CLOSED_DECAY => log("closed", Unit::Ms),
        hp::OPEN_DECAY => log("open", Unit::Ms),
        hp::BP_HZ => log("band", Unit::Hz),
        hp::BP_Q => log("width", Unit::Plain),
        hp::HP_HZ => log("hp", Unit::Hz),
        hp::DRIVE => log("drive", Unit::Ratio),
        _ => Param::new(
            "gain",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )
        .with_default(def.default),
    }
}

/// The engine-facing value at a normalized knob position.
///
/// No `shown`/`natural` pair: every row on this card is already in the
/// unit it prints, so there is nothing to convert and nothing to get
/// wrong in one direction only.
pub fn hat_value(param: u32, norm: f32) -> f32 {
    params::def(hp::TABLE, param).clamp(param_of(param).value(norm))
}

/// The inverse of [`hat_value`], for a state stored in engine units.
pub fn hat_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Whether a parameter snaps rather than sweeping. None of the hat's do.
pub fn hat_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. All but the gain.
pub fn hat_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn hat_edits(state: &HatUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: hat_value(param, state.get(param)),
        })
        .collect()
}

/// The frequency axis the plot spans. Wide enough to hold the lowest the
/// bank can be tuned (205 Hz at half speed, ~103) and the top of the
/// band's own range.
const PLOT_LO_HZ: f32 = 80.0;
const PLOT_HI_HZ: f32 = 20_000.0;

/// Where a frequency sits across the plot, `0..=1`, on a log axis.
fn along(hz: f32) -> f32 {
    let lo = PLOT_LO_HZ.ln();
    let hi = PLOT_HI_HZ.ln();
    ((hz.max(1.0).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The magnitude the sound is heard through at `hz`: the bandpass and the
/// output highpass together, `0..=1`.
///
/// The analogue prototypes of the two kernels the node actually runs — a
/// unity-gain two-pole bandpass and a Butterworth two-pole highpass. The
/// digital responses warp near Nyquist, and this is a picture of where
/// the energy is rather than a measurement, so the prototype is the right
/// amount of arithmetic: exact where the six lines are, and honest about
/// the shape everywhere else.
fn response(state: &HatUi, hz: f32) -> f32 {
    let band = hat_value(hp::BP_HZ, state.band);
    let q = hat_value(hp::BP_Q, state.width).max(0.01);
    let high = hat_value(hp::HP_HZ, state.hp);

    // Unity-gain bandpass: 1 at the corner whatever Q, which is what the
    // node's `BandpassUnity` mode gives and why the width knob is not
    // secretly a level control.
    let r = hz / band.max(1.0);
    let detune = r - 1.0 / r;
    let bp = 1.0 / (1.0 + q * q * detune * detune).sqrt();

    // Butterworth highpass: no resonance of its own, 12 dB per octave
    // below the corner.
    let w = hz / high.max(1.0);
    let w2 = w * w;
    let denominator = ((1.0 - w2) * (1.0 - w2) + 2.0 * w2).sqrt();
    let hpf = if denominator > 1e-9 {
        w2 / denominator
    } else {
        0.0
    };

    (bp * hpf).clamp(0.0, 1.0)
}

/// Draw the bank and the window: the hero.
///
/// Six standing lines at the oscillator frequencies, each drawn as tall
/// as the filter lets it through — so the picture says both WHERE the
/// bank is and HOW MUCH of each partial survives. The response curve
/// itself runs behind them.
fn bank(ui: &mut egui::Ui, theme: &Theme, state: &HatUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);

    let floor = rect.bottom() - stroke::HAIR;
    painter.line_segment(
        [
            egui::pos2(rect.left(), floor),
            egui::pos2(rect.right(), floor),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );

    // The window, behind everything.
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);
    let mut curve = Vec::with_capacity(columns);
    for column in 0..columns {
        let at = column as f32 / (columns - 1).max(1) as f32;
        let hz = (PLOT_LO_HZ.ln() + at * (PLOT_HI_HZ.ln() - PLOT_LO_HZ.ln())).exp();
        curve.push(egui::pos2(
            rect.left() + at * rect.width(),
            rect.bottom() - response(state, hz) * rect.height() * 0.9,
        ));
    }
    painter.add(egui::Shape::line(
        curve,
        egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
    ));

    // The bank: six lines that MOVE TOGETHER and never against each
    // other. Drawn last, over the window, because they are the
    // instrument and it is the view.
    let tune = hat_value(hp::TUNE, state.tune);
    for ratio in hp::RATIOS {
        let hz = ratio * tune;
        let x = rect.left() + along(hz) * rect.width();
        let height = response(state, hz) * rect.height() * 0.9;
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x, rect.bottom() - height),
            ],
            egui::Stroke::new(stroke::BOLD, theme.role_time),
        );
    }
}

/// The value strip's rows: the BANK and its two lengths, then the WINDOW
/// the bank is heard through. The card's two rows are the plot's two
/// halves.
const ROWS: [&[u32]; 2] = [
    &[hp::TUNE, hp::CLOSED_DECAY, hp::OPEN_DECAY, hp::GAIN],
    &[hp::BP_HZ, hp::BP_Q, hp::HP_HZ, hp::DRIVE],
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

/// Draw the hi-hat card. Returns the edits the user just made.
pub fn hat_card(ui: &mut egui::Ui, theme: &Theme, state: &mut HatUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "808 hat", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => bank(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut HatUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, with the gaps taken off FIRST.
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
                        let moved = if hat_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: hat_value(*param, *norm),
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
        for def in hp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = hat_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = hat_norm(def.id, value);
                let again = hat_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults — and for this device
    /// that means the card opens on the machine, tune at exactly unity.
    #[test]
    fn the_cards_defaults_match_the_engine_table() {
        let ui = HatUi::default();
        for def in hp::TABLE {
            let shown = hat_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
        assert!((hat_value(hp::TUNE, ui.tune) - 1.0).abs() < 1e-3);
    }

    /// Every parameter is REACHABLE, exactly once.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), hp::TABLE.len(), "a parameter is missing");
        for def in hp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), hp::TABLE.len());
        assert_eq!(hat_edits(&HatUi::default()).len(), hp::TABLE.len());
    }

    /// EVERYTHING BUT THE GAIN SWEEPS IN OCTAVES, and nothing snaps.
    #[test]
    fn the_frequencies_and_times_are_log_and_nothing_steps() {
        for def in hp::TABLE {
            if def.id == hp::GAIN {
                assert!(!hat_is_log(def.id), "a level is linear");
            } else {
                assert!(hat_is_log(def.id), "{} should sweep in octaves", def.name);
            }
            assert!(
                !hat_is_discrete(def.id),
                "{} is continuous but snaps",
                def.name
            );
        }
    }

    /// THE SIX LINES MOVE TOGETHER AND NEVER APART. The ratios are the
    /// instrument; a tune knob that could shift one against another would
    /// be a knob that turns an 808 into something else.
    #[test]
    fn the_bank_transposes_without_detuning() {
        let at = |tune: f32| {
            let state = HatUi {
                tune: hat_norm(hp::TUNE, tune),
                ..Default::default()
            };
            let scale = hat_value(hp::TUNE, state.tune);
            hp::RATIOS.map(|r| r * scale)
        };
        let unity = at(1.0);
        assert_eq!(unity, hp::RATIOS, "unity must be the machine exactly");

        let doubled = at(2.0);
        for (i, (high, low)) in doubled.iter().zip(unity.iter()).enumerate() {
            assert!(
                (high / low - 2.0).abs() < 1e-3,
                "oscillator {i} did not transpose with the rest"
            );
        }

        // The lowest the bank can go still lands inside the plot.
        let lowest = at(hp::TUNE_MIN)[0];
        assert!(
            lowest > PLOT_LO_HZ,
            "the bank falls off the left of its own plot at {lowest}"
        );
        let highest = at(hp::TUNE_MAX)[hp::RATIOS.len() - 1];
        assert!(highest < PLOT_HI_HZ, "and off the right at {highest}");
    }

    /// THE WINDOW IS A BANDPASS, and the plot draws the one the node
    /// runs: unity at the corner whatever the width, and falling away on
    /// both sides.
    #[test]
    fn the_drawn_response_is_a_band_that_peaks_at_its_corner() {
        // The highpass well below, so what is measured is the band.
        let mut state = HatUi {
            band: hat_norm(hp::BP_HZ, 10_000.0),
            width: hat_norm(hp::BP_Q, 2.0),
            hp: hat_norm(hp::HP_HZ, 1_000.0),
            ..Default::default()
        };

        let at_corner = response(&state, 10_000.0);
        assert!(
            at_corner > 0.9,
            "a unity bandpass should pass its corner: {at_corner}"
        );
        assert!(response(&state, 1_500.0) < at_corner * 0.5, "falls below");
        assert!(response(&state, 19_000.0) < at_corner, "and above");

        // A NARROWER band does not get louder at the corner — the whole
        // reason the node uses the unity-gain mode.
        state.width = hat_norm(hp::BP_Q, 12.0);
        let narrow = response(&state, 10_000.0);
        assert!(
            (narrow - at_corner).abs() < 0.05,
            "width is secretly a level control: {narrow} against {at_corner}"
        );
        // But it IS narrower.
        state.width = hat_norm(hp::BP_Q, 2.0);
        let wide_skirt = response(&state, 5_000.0);
        state.width = hat_norm(hp::BP_Q, 12.0);
        assert!(response(&state, 5_000.0) < wide_skirt, "and not narrower");

        // The highpass really does cut underneath.
        state.hp = hat_norm(hp::HP_HZ, 12_000.0);
        assert!(response(&state, 2_000.0) < 0.05);

        // Nothing anywhere is non-finite or out of range.
        for def in hp::TABLE {
            for at in [0.0f32, 1.0] {
                let mut edge = HatUi::default();
                let Some(slot) = edge.slot(def.id) else {
                    continue;
                };
                *slot = at;
                for hz in [PLOT_LO_HZ, 1_000.0, PLOT_HI_HZ] {
                    let r = response(&edge, hz);
                    assert!((0.0..=1.0).contains(&r), "{} at {at}: {r}", def.name);
                }
            }
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
        let mut state = HatUi::default();
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
                edits = hat_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
