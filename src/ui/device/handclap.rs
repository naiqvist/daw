//! The hand clap's card.
//!
//! `handclap`, not `clap`: CLAP in this codebase is the PLUGIN FORMAT the
//! host loads through clack, and a widget module named `clap` sitting in
//! a tree with a CLAP plugin host is a name that costs somebody an
//! afternoon.
//!
//! Its hero is THE BURST PATTERN — the retriggered short envelope drawn
//! over the long tail underneath it. That picture is the whole device,
//! and it is the one display here that makes an invisible decision
//! visible: the bursts are UNEVENLY SPACED, and no arrangement of a
//! "hands" knob and a "spread" knob can say so. Drawn, it is the first
//! thing you see.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, handclap as cp};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandclapUi {
    pub hands: f32,
    pub spread: f32,
    pub snap: f32,
    pub body: f32,
    pub tail: f32,
    pub tone: f32,
    pub width: f32,
    pub hp: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for HandclapUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// voice agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(cp::TABLE, id).default)
    }
}

impl HandclapUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| handclap_norm(id, get(id));
        Self {
            hands: at(cp::BURSTS),
            spread: at(cp::SPREAD),
            snap: at(cp::BURST_DECAY),
            body: at(cp::BODY),
            tail: at(cp::BODY_DECAY),
            tone: at(cp::TONE),
            width: at(cp::WIDTH),
            hp: at(cp::HP_HZ),
            drive: at(cp::DRIVE),
            gain: at(cp::GAIN),
        }
    }

    /// One knob position by wire id, so the strip can walk ids rather
    /// than naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            cp::BURSTS => &mut self.hands,
            cp::SPREAD => &mut self.spread,
            cp::BURST_DECAY => &mut self.snap,
            cp::BODY => &mut self.body,
            cp::BODY_DECAY => &mut self.tail,
            cp::TONE => &mut self.tone,
            cp::WIDTH => &mut self.width,
            cp::HP_HZ => &mut self.hp,
            cp::DRIVE => &mut self.drive,
            cp::GAIN => &mut self.gain,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            cp::BURSTS => self.hands,
            cp::SPREAD => self.spread,
            cp::BURST_DECAY => self.snap,
            cp::BODY => self.body,
            cp::BODY_DECAY => self.tail,
            cp::TONE => self.tone,
            cp::WIDTH => self.width,
            cp::HP_HZ => self.hp,
            cp::DRIVE => self.drive,
            _ => self.gain,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 10] = [
    cp::BURSTS,
    cp::SPREAD,
    cp::BURST_DECAY,
    cp::BODY,
    cp::BODY_DECAY,
    cp::TONE,
    cp::WIDTH,
    cp::HP_HZ,
    cp::DRIVE,
    cp::GAIN,
];

/// One control by wire id. The single place an id becomes a `Param`.
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
    match param {
        // The hand count is a COUNT: a knob reading 2.4 hands describes
        // something that cannot exist. `Steps` runs 0..count-1, so the
        // count — which starts at one — is shifted by `shown` and
        // `natural` rather than by inventing a second mapping.
        cp::BURSTS => Param::new(
            "hands",
            Mapping::Steps {
                count: def.max as u32,
            },
            Unit::Plain,
        )
        .with_default(def.default - 1.0),
        cp::SPREAD => log("spread", Unit::Ms),
        cp::BURST_DECAY => log("snap", Unit::Ms),
        cp::BODY_DECAY => log("tail", Unit::Ms),
        cp::TONE => log("tone", Unit::Hz),
        cp::WIDTH => log("width", Unit::Plain),
        cp::HP_HZ => log("hp", Unit::Hz),
        cp::DRIVE => log("drive", Unit::Ratio),
        cp::BODY => Param::percent("body").with_default(def.default * 100.0),
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

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        cp::BODY => value * 100.0,
        // One hand is step zero.
        cp::BURSTS => value - 1.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        cp::BODY => value / 100.0,
        cp::BURSTS => value.round() + 1.0,
        _ => value,
    };
    params::def(cp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn handclap_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`handclap_value`], for a state stored in engine units.
pub fn handclap_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. The hand count does.
pub fn handclap_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale.
pub fn handclap_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn handclap_edits(state: &HandclapUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: handclap_value(param, state.get(param)),
        })
        .collect()
}

/// How far the plot looks, in milliseconds. Long enough to show the tail
/// settling, short enough that a 6 ms burst is more than one pixel.
const PLOT_MS: f32 = 400.0;

/// `ln(1000)` — the kernel's own constant, because `ExpDecay`'s figure is
/// a time to fall 60 dB.
const DECADES: f32 = 6.907_755_4;

/// An exponential decay's value at `ms` after its own start, `0..=1`.
fn decay_at(total_ms: f32, ms: f32) -> f32 {
    if ms < 0.0 {
        return 0.0;
    }
    (-DECADES * ms / total_ms.max(0.01)).exp()
}

/// The burst envelope at `ms`: the most recent hand that has already
/// struck, decaying.
///
/// The node's own arithmetic, restated. Each hand RETRIGGERS the one
/// envelope rather than adding a second, so what is heard at any moment
/// is the latest strike and not a sum — and drawing a sum here would show
/// a clap that grows louder with every hand, which is not the one
/// sounding.
fn bursts_at(state: &HandclapUi, ms: f32) -> f32 {
    let hands =
        (handclap_value(cp::BURSTS, state.hands).round().max(1.0) as usize).min(cp::OFFSETS.len());
    let spread = handclap_value(cp::SPREAD, state.spread);
    let snap = handclap_value(cp::BURST_DECAY, state.snap);
    let mut level = 0.0f32;
    for offset in cp::OFFSETS.iter().take(hands) {
        let at = offset * spread;
        if ms >= at {
            level = decay_at(snap, ms - at);
        }
    }
    level
}

/// The tail at `ms`: one decay from the first strike, whatever the hands
/// do afterwards.
fn body_at(state: &HandclapUi, ms: f32) -> f32 {
    let level = handclap_value(cp::BODY, state.body);
    decay_at(handclap_value(cp::BODY_DECAY, state.tail), ms) * level
}

/// Draw the burst pattern: the hero.
///
/// The BURSTS bold, the TAIL quieter behind them, on one time axis. The
/// unevenness of the spacing is the thing to see, and it is visible at a
/// glance in a way no pair of numbers is.
fn pattern(ui: &mut egui::Ui, theme: &Theme, state: &HandclapUi) {
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

    let mut tail = Vec::with_capacity(columns);
    let mut hands = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let ms = along * PLOT_MS;
        let x = rect.left() + along * rect.width();
        let up = |v: f32| rect.bottom() - v.clamp(0.0, 1.0) * rect.height() * 0.9;
        tail.push(egui::pos2(x, up(body_at(state, ms))));
        hands.push(egui::pos2(x, up(bursts_at(state, ms))));
    }
    painter.add(egui::Shape::line(
        tail,
        egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
    ));
    painter.add(egui::Shape::line(
        hands,
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));
}

/// The value strip's rows: the HANDS, then the ROOM and the colour they
/// are heard in. The card's two rows are the plot's two lines.
const ROWS: [&[u32]; 2] = [
    &[cp::BURSTS, cp::SPREAD, cp::BURST_DECAY, cp::GAIN],
    &[
        cp::BODY,
        cp::BODY_DECAY,
        cp::TONE,
        cp::WIDTH,
        cp::HP_HZ,
        cp::DRIVE,
    ],
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

/// Draw the clap card. Returns the edits the user just made.
pub fn handclap_card(ui: &mut egui::Ui, theme: &Theme, state: &mut HandclapUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "clap", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => pattern(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut HandclapUi, edits: &mut Vec<ParamEdit>) {
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
                        // The hand count gets the STEPPED cell: a count
                        // sweeping smoothly through values it cannot take
                        // would be a control lying about what it does.
                        let moved = if handclap_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: handclap_value(*param, *norm),
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
        for def in cp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = handclap_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = handclap_norm(def.id, value);
                let again = handclap_value(def.id, back);
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
        let ui = HandclapUi::default();
        for def in cp::TABLE {
            let shown = handclap_value(def.id, ui.get(def.id));
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
        assert_eq!(seen.len(), cp::TABLE.len(), "a parameter is missing");
        for def in cp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), cp::TABLE.len());
        assert_eq!(
            handclap_edits(&HandclapUi::default()).len(),
            cp::TABLE.len()
        );
    }

    /// THE HAND COUNT STEPS AND STARTS AT ONE. A knob reading 2.4 hands
    /// describes something that cannot exist, and one reading 0 describes
    /// a clap that makes no sound.
    #[test]
    fn the_hand_count_is_a_whole_number_of_hands() {
        assert!(handclap_is_discrete(cp::BURSTS));
        for at in 0..=20 {
            let value = handclap_value(cp::BURSTS, at as f32 / 20.0);
            assert_eq!(value, value.round(), "{value} is not a whole hand");
            assert!(
                (1.0..=cp::BURSTS_MAX).contains(&value),
                "{value} hands is not a clap"
            );
        }
        // Both stops are reachable: one hand and the full house.
        assert_eq!(handclap_value(cp::BURSTS, 0.0), 1.0);
        assert_eq!(handclap_value(cp::BURSTS, 1.0), cp::BURSTS_MAX);

        // And nothing else on the card snaps.
        for def in cp::TABLE {
            if def.id != cp::BURSTS {
                assert!(
                    !handclap_is_discrete(def.id),
                    "{} is continuous but snaps",
                    def.name
                );
            }
        }
    }

    /// THE TIMES AND FREQUENCIES SWEEP IN OCTAVES.
    #[test]
    fn the_times_and_frequencies_are_log() {
        for id in [
            cp::SPREAD,
            cp::BURST_DECAY,
            cp::BODY_DECAY,
            cp::TONE,
            cp::HP_HZ,
        ] {
            assert!(handclap_is_log(id), "{id} should sweep in octaves");
        }
        assert!(!handclap_is_log(cp::BODY), "a level is linear");
        assert!(!handclap_is_log(cp::BURSTS), "a count is not a sweep");
    }

    /// THE PICTURE IS THE VOICE'S OWN ARITHMETIC: a burst at each offset,
    /// each one RETRIGGERING rather than summing, over one tail that only
    /// falls.
    #[test]
    fn the_pattern_puts_a_burst_at_every_offset() {
        assert!((DECADES - 1_000.0f32.ln()).abs() < 1e-4);

        let mut state = HandclapUi {
            hands: handclap_norm(cp::BURSTS, 3.0),
            spread: handclap_norm(cp::SPREAD, 10.0),
            snap: handclap_norm(cp::BURST_DECAY, 4.0),
            ..Default::default()
        };

        // Full level at every strike: 0, 10 and 19 ms. Taken from the
        // state's OWN spread rather than the 10.0 that was written into
        // it — a log mapping round-trips to 10.000001, and asking about
        // the sample a millionth of a millisecond BEFORE a burst is a
        // question with a correct answer of "not yet".
        let spread = handclap_value(cp::SPREAD, state.spread);
        for offset in cp::OFFSETS.iter().take(3) {
            let at = offset * spread;
            assert!(
                (bursts_at(&state, at) - 1.0).abs() < 1e-3,
                "no hand at {at} ms"
            );
        }
        // And it never exceeds one — a retrigger replaces, it does not
        // stack. A summing display would show 3.0 at the last burst.
        for step in 0..=400 {
            let level = bursts_at(&state, step as f32);
            assert!((0.0..=1.0 + 1e-4).contains(&level), "{level} at {step} ms");
        }
        // Between two hands the level has dropped away.
        assert!(bursts_at(&state, 8.0) < 0.05, "the gap did not open");

        // The fourth hand only appears when it is asked for.
        assert!(bursts_at(&state, cp::OFFSETS[3] * spread + 0.1) < 0.05);
        state.hands = handclap_norm(cp::BURSTS, 4.0);
        assert!((bursts_at(&state, cp::OFFSETS[3] * spread) - 1.0).abs() < 1e-3);

        // THE TAIL ONLY FALLS — one decay from the first strike, not one
        // per hand.
        let mut previous = f32::MAX;
        for step in 0..=200 {
            let now = body_at(&state, step as f32 * 2.0);
            assert!(now <= previous + 1e-6, "the tail rose at {step}");
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
        let mut state = HandclapUi::default();
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
                edits = handclap_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }
}
