//! The modulato card.
//!
//! Laid out by `notes/20260827-device-card-layout.md`: one row of seven
//! cells, two `POLY_CELL_H` units tall, the card declaring its own width
//! and the row sharing it progressively.
//!
//! Its hero is the DELAY TRAJECTORY — the two channels' delay times
//! wobbling over a couple of cycles. That one picture carries four of
//! the seven controls at once: where the delay sits, how far it swings,
//! how fast, and how far apart the sides are. Those four are meaningless
//! as numbers and obvious as a shape, which is the case the screen
//! guide's "every control shows what it does" rule exists for.

use crate::params::{self, modulato as mp};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions, normalized. Serialized so they survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ModulatoUi {
    pub mode: f32,
    pub rate: f32,
    pub depth: f32,
    pub delay: f32,
    pub feedback: f32,
    pub spread: f32,
    pub mix: f32,
}

impl Default for ModulatoUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(mp::TABLE, id).default)
    }
}

impl ModulatoUi {
    /// The card's state for a patch in engine units. A reader rather than
    /// the engine's struct: a widget must not know `crate::audio`.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| modulato_norm(id, get(id));
        Self {
            mode: at(mp::MODE),
            rate: at(mp::RATE),
            depth: at(mp::DEPTH),
            delay: at(mp::DELAY),
            feedback: at(mp::FEEDBACK),
            spread: at(mp::SPREAD),
            mix: at(mp::MIX),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            mp::MODE => &mut self.mode,
            mp::RATE => &mut self.rate,
            mp::DEPTH => &mut self.depth,
            mp::DELAY => &mut self.delay,
            mp::FEEDBACK => &mut self.feedback,
            mp::SPREAD => &mut self.spread,
            mp::MIX => &mut self.mix,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            mp::MODE => self.mode,
            mp::RATE => self.rate,
            mp::DEPTH => self.depth,
            mp::DELAY => self.delay,
            mp::FEEDBACK => self.feedback,
            mp::SPREAD => self.spread,
            _ => self.mix,
        }
    }
}

/// The strip. Seven cells in one row — the compressor runs nine, and the
/// card declares a width wide enough for them.
const ROWS: [&[u32]; 1] = [&[
    mp::MODE,
    mp::DELAY,
    mp::DEPTH,
    mp::RATE,
    mp::SPREAD,
    mp::FEEDBACK,
    mp::MIX,
]];

/// A labelled cell prints its value on one line and its name on the next.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

fn param_of(param: u32) -> Param {
    let def = params::def(mp::TABLE, param);
    match param {
        mp::MODE => Param::choice("mode", mp::MODE_NAMES),
        // Rate is LOG: the difference between 0.2 Hz and 0.4 Hz is the
        // whole character of a slow chorus, and between 19 and 20 nothing
        // at all.
        mp::RATE => Param::new(
            "rate",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Hz,
        )
        .with_default(def.default),
        mp::DEPTH => Param::new(
            "depth",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Ms,
        )
        .with_default(def.default),
        // The delay knob is a POSITION in the mode's window, but it
        // prints the millisecond figure it resolves to — see `delay_of`.
        // A cell reading `0.35` would be a control describing itself
        // rather than the sound.
        mp::DELAY => Param::new("delay", Mapping::Linear { min: 0.0, max: 1.0 }, Unit::Ms)
            .with_default(def.default),
        mp::FEEDBACK => Param::new(
            "feedback",
            Mapping::Linear {
                min: def.min * 100.0,
                max: def.max * 100.0,
            },
            Unit::Percent,
        )
        .bipolar()
        .with_default(def.default * 100.0),
        mp::SPREAD => Param::new(
            "spread",
            Mapping::Linear {
                min: 0.0,
                // Turns on the wire, DEGREES on the dial: half a turn is
                // 180°, which is the number anyone reaching for a stereo
                // spread already has in mind.
                max: def.max * 360.0,
            },
            Unit::Plain,
        )
        .with_default(def.default * 360.0),
        _ => Param::percent("mix").with_default(def.default * 100.0),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        mp::FEEDBACK | mp::MIX => value * 100.0,
        mp::SPREAD => value * 360.0,
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        mp::FEEDBACK | mp::MIX => value / 100.0,
        mp::SPREAD => value / 360.0,
        mp::MODE => value.round(),
        _ => value,
    };
    params::def(mp::TABLE, param).clamp(raw)
}

pub fn modulato_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn modulato_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn modulato_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

pub fn modulato_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

pub fn modulato_edits(state: &ModulatoUi) -> Vec<ParamEdit> {
    mp::TABLE
        .iter()
        .map(|def| ParamEdit {
            param: def.id,
            value: modulato_value(def.id, state.get(def.id)),
        })
        .collect()
}

/// The base delay this state resolves to, in milliseconds.
fn delay_of(state: &ModulatoUi) -> f32 {
    let (low, high) = mp::window(modulato_value(mp::MODE, state.mode));
    low + (high - low) * modulato_value(mp::DELAY, state.delay).clamp(0.0, 1.0)
}

/// What each mode's other knobs usually want.
///
/// Returned as EDITS when the mode changes, so they land as ordinary knob
/// positions the user can then move. That is what makes this a preset
/// rather than a mode that disables controls — a vibrato with its mix
/// turned down IS a short chorus, and finding that out is worth more than
/// being stopped from trying.
fn mode_defaults(mode: f32) -> [(u32, f32); 2] {
    match mode.round() {
        m if m == mp::MODE_FLANGER => [(mp::FEEDBACK, 0.55), (mp::MIX, 0.5)],
        m if m == mp::MODE_VIBRATO => [(mp::FEEDBACK, 0.0), (mp::MIX, 1.0)],
        _ => [(mp::FEEDBACK, 0.0), (mp::MIX, 0.5)],
    }
}

/// How many LFO cycles the plot shows.
const PLOT_CYCLES: f32 = 2.0;

/// The delay trajectory: both channels' delay time over two cycles.
///
/// The vertical axis is the mode's whole window, so the curve's HEIGHT
/// on the card says where in the flanger-to-chorus range this patch sits
/// — switching mode visibly rescales the space the wobble lives in,
/// which is the one thing about `mode` that is hard to say in words.
fn trajectory(ui: &mut egui::Ui, theme: &Theme, state: &ModulatoUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);
    let (low, high) = mp::window(modulato_value(mp::MODE, state.mode));
    let base = delay_of(state);
    let swing = modulato_value(mp::DEPTH, state.depth);
    let spread = modulato_value(mp::SPREAD, state.spread);
    // The window plus the swing, so a deep setting that runs past the
    // window's edge is VISIBLY doing so rather than being clipped off
    // the top of the picture.
    let (floor, ceiling) = (low - swing, high + swing);
    let span = (ceiling - floor).max(0.001);

    let curve = |phase_offset: f32| -> Vec<egui::Pos2> {
        (0..columns)
            .map(|column| {
                let along = column as f32 / (columns - 1).max(1) as f32;
                let turns = along * PLOT_CYCLES + phase_offset;
                let wobble = (turns * std::f32::consts::TAU).sin();
                let ms = base + wobble * swing;
                let up = ((ms - floor) / span).clamp(0.0, 1.0);
                egui::pos2(
                    rect.left() + along * rect.width(),
                    rect.bottom() - up * rect.height() * 0.92,
                )
            })
            .collect()
    };

    // The window's edges, so the mode's range is visible behind the
    // curves rather than implied by them.
    for edge in [low, high] {
        let up = ((edge - floor) / span).clamp(0.0, 1.0);
        let y = rect.bottom() - up * rect.height() * 0.92;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
    }
    painter.add(egui::Shape::line(
        curve(spread),
        egui::Stroke::new(stroke::HAIR, theme.role_mod),
    ));
    painter.add(egui::Shape::line(
        curve(0.0),
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    ));
}

/// Draw the modulato card. Returns the edits the user just made.
pub fn modulato_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ModulatoUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "modulato", control::DEVICE_TALL_H, |ui| {
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

fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

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

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut ModulatoUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    let rows = ROWS.len() as f32;
    let height = ((ui.available_height() - gap * (rows - 1.0)) / rows).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    // The delay cell prints MILLISECONDS, not its own position in the
    // window — resolved here because it depends on the mode, which the
    // `Param` alone cannot see.
    let resolved_delay = delay_of(state);
    for row in ROWS {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            let mut mode_changed = None;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                let left = (row.len() - drawn) as f32;
                let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
                let width = (room / left).floor().max(1.0);
                let Some(norm) = state.slot(*param) else {
                    continue;
                };
                let before = *norm;
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        let moved = if *param == mp::MODE {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else if *param == mp::DELAY {
                            let show = move |_: f32| format!("{resolved_delay:.2} ms");
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, Some(&show))
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: modulato_value(*param, *norm),
                            });
                        }
                    },
                );
                if *param == mp::MODE && *norm != before {
                    mode_changed = Some(modulato_value(mp::MODE, *norm));
                }
            }
            // Switching mode carries its usual feedback and mix along, as
            // ordinary edits the user can then move.
            if let Some(mode) = mode_changed {
                for (param, value) in mode_defaults(mode) {
                    if let Some(slot) = state.slot(param) {
                        *slot = modulato_norm(param, value);
                    }
                    edits.push(ParamEdit { param, value });
                }
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_round_trips_through_the_card() {
        for def in mp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = modulato_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left its range: {value}",
                    def.name
                );
                let again = modulato_value(def.id, modulato_norm(def.id, value));
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn the_cards_defaults_match_the_engine_table() {
        let edits = modulato_edits(&ModulatoUi::default());
        assert_eq!(edits.len(), mp::TABLE.len());
        for def in mp::TABLE {
            let edit = edits.iter().find(|e| e.param == def.id).expect("every row");
            assert!(
                (edit.value - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {} not {}",
                def.name,
                edit.value,
                def.default
            );
        }
    }

    /// THE DELAY CELL PRINTS MILLISECONDS. Its wire value is a position
    /// in the mode's window, and a cell reading `0.35` would be a control
    /// describing itself rather than the sound.
    #[test]
    fn the_delay_cell_resolves_through_the_mode() {
        let mut state = ModulatoUi::default();
        state.delay = 1.0;
        state.mode = modulato_norm(mp::MODE, mp::MODE_FLANGER);
        let flanger = delay_of(&state);
        state.mode = modulato_norm(mp::MODE, mp::MODE_CHORUS);
        let chorus = delay_of(&state);
        assert!(
            chorus > flanger * 3.0,
            "the same knob should mean very different times: {chorus} vs {flanger}"
        );
        // And the ends of the knob are the ends of the window.
        for mode in [mp::MODE_CHORUS, mp::MODE_FLANGER, mp::MODE_VIBRATO] {
            let (low, high) = mp::window(mode);
            state.mode = modulato_norm(mp::MODE, mode);
            state.delay = 0.0;
            assert!((delay_of(&state) - low).abs() < 1e-3);
            state.delay = 1.0;
            assert!((delay_of(&state) - high).abs() < 1e-3);
        }
    }

    /// Mode is a PRESET, not a lock: it moves feedback and mix, and every
    /// control stays adjustable afterwards.
    #[test]
    fn switching_mode_carries_its_usual_settings() {
        let vibrato = mode_defaults(mp::MODE_VIBRATO);
        assert!(
            vibrato.iter().any(|(p, v)| *p == mp::MIX && *v == 1.0),
            "vibrato is all wet"
        );
        assert!(
            vibrato.iter().any(|(p, v)| *p == mp::FEEDBACK && *v == 0.0),
            "vibrato has no feedback"
        );
        let flanger = mode_defaults(mp::MODE_FLANGER);
        assert!(
            flanger.iter().any(|(p, v)| *p == mp::FEEDBACK && *v > 0.3),
            "a flanger without feedback is a chorus"
        );
        // Every value it sets is legal for its row.
        for mode in [mp::MODE_CHORUS, mp::MODE_FLANGER, mp::MODE_VIBRATO] {
            for (param, value) in mode_defaults(mode) {
                let def = params::def(mp::TABLE, param);
                assert!(value >= def.min && value <= def.max, "{} {value}", def.name);
            }
        }
    }

    #[test]
    fn mode_steps_and_rate_is_log() {
        assert!(modulato_is_discrete(mp::MODE));
        assert!(!modulato_is_discrete(mp::RATE));
        assert!(modulato_is_log(mp::RATE));
        assert!(!modulato_is_log(mp::DEPTH));
        // Every mode is reachable and lands on a whole index.
        for at in 0..=10 {
            let value = modulato_value(mp::MODE, at as f32 / 10.0);
            assert_eq!(value, value.round(), "{value} is not a whole mode");
            assert!(value <= mp::MODE_MAX);
        }
        assert_eq!(mp::MODE_NAMES.len(), mp::MODE_MAX as usize + 1);
    }

    /// The layout note's rules, checked rather than remembered.
    #[test]
    fn the_strip_fits_the_width_and_height_it_asks_for() {
        assert_eq!(CELL_UNITS, 2);
        assert_eq!(FOOTER_ROWS, ROWS.len() * CELL_UNITS);
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let needed = unit * CELL_UNITS as f32 * ROWS.len() as f32;
        assert!(reserved >= needed);
        assert!(control::DEVICE_TALL_H - reserved > unit * 3.0);

        let context = egui::Context::default();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1_200.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let declared = face_width(ui, &theme);
                for row in ROWS {
                    let natural: f32 = row
                        .iter()
                        .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                        .sum::<f32>()
                        + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                    assert!(
                        natural <= declared + 0.5,
                        "{natural} needed, {declared} asked"
                    );
                    for id in row {
                        let param = param_of(*id);
                        let wanted = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
                        assert!(cell_min_width(ui, &theme, &param) >= wanted);
                    }
                }
            },
        );
        run.textures_delta.clear();
    }

    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), mp::TABLE.len(), "a parameter is missing");
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ModulatoUi::default();
        let before = state;
        let mut edits = Vec::new();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1_200.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                edits = modulato_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before);
    }
}
