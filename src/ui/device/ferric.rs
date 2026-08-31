//! Ferric's device card — the tape looper.
//!
//! Ids, ranges, the division grid and the groove patterns all come from
//! [`crate::params::ferric`] — one table, read by this widget, by
//! `Node::Ferric`'s core and by the app's edit routing. The card draws
//! the same array the head is placed from, so the picture cannot describe
//! a groove the machine is not playing.
//!
//! # The staircase
//!
//! The hero is the pattern as a STAIRCASE. A baseline across the top is
//! the write head — live tape, this instant. For each of the eight steps
//! the figure drops by however many divisions the head is placed back,
//! and runs flat across that step's width. Reading it left to right is
//! reading the groove: `run` is a straight line at the top, `half` is a
//! pair of stairs, `stutter` is a flight of four.
//!
//! It is drawn from `PATTERNS[pattern]` scaled by GROOVE, so turning the
//! groove down visibly flattens the stairs toward the live line — and at
//! zero the figure IS the live line, which is exactly what the device
//! does there.
//!
//! The lit column is the step the transport is on. The caret under the
//! baseline is the write head, always moving; the caret on the stair is
//! the read head, wherever the pattern has put it.
//!
//! # Angular on purpose
//!
//! Right angles only, no curves: a step boundary is an instant and a
//! displacement is a whole number of divisions, so every line in the
//! figure is either a jump or a hold. A smoothed version would draw
//! motion the machine does not have.

use crate::params::ferric as fp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The cells: what the tape does, then the groove, then the wear, then
/// how much of it you hear.
const CELLS: [u32; 9] = [
    fp::SPEED,
    fp::DIVISION,
    fp::PATTERN,
    fp::GROOVE,
    fp::DRIVE,
    fp::WOW,
    fp::AGE,
    fp::MIX,
    fp::OUT,
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
/// The deepest any pattern reaches, for scaling the staircase. Taken from
/// the table rather than written down, so a new pattern cannot fall off
/// the bottom of its own picture.
fn deepest() -> f32 {
    fp::PATTERNS
        .iter()
        .flat_map(|row| row.iter())
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32
}

/// Knob positions of one machine, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FerricUi {
    pub speed: f32,
    pub division: f32,
    pub pattern: f32,
    pub groove: f32,
    pub drive: f32,
    pub wow: f32,
    pub age: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for FerricUi {
    fn default() -> Self {
        let at = |id: u32| ferric_norm(id, params::def(fp::TABLE, id).default);
        Self {
            speed: at(fp::SPEED),
            division: at(fp::DIVISION),
            pattern: at(fp::PATTERN),
            groove: at(fp::GROOVE),
            drive: at(fp::DRIVE),
            wow: at(fp::WOW),
            age: at(fp::AGE),
            mix: at(fp::MIX),
            out: at(fp::OUT),
        }
    }
}

impl FerricUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| ferric_norm(id, get(id));
        Self {
            speed: at(fp::SPEED),
            division: at(fp::DIVISION),
            pattern: at(fp::PATTERN),
            groove: at(fp::GROOVE),
            drive: at(fp::DRIVE),
            wow: at(fp::WOW),
            age: at(fp::AGE),
            mix: at(fp::MIX),
            out: at(fp::OUT),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            fp::SPEED => &mut self.speed,
            fp::DIVISION => &mut self.division,
            fp::PATTERN => &mut self.pattern,
            fp::GROOVE => &mut self.groove,
            fp::DRIVE => &mut self.drive,
            fp::WOW => &mut self.wow,
            fp::AGE => &mut self.age,
            fp::MIX => &mut self.mix,
            fp::OUT => &mut self.out,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(fp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        fp::DIVISION => with(Param::choice("div", fp::DIVISION_NAMES)),
        fp::PATTERN => with(Param::choice("pattern", fp::PATTERN_NAMES)),
        // BIPOLAR: nominal speed is the middle, and a tape at nominal is
        // the thing every other position is heard against.
        fp::SPEED => with(
            Param::new(
                "speed",
                crate::ui::device::Mapping::Linear {
                    min: def.min,
                    max: def.max,
                },
                crate::ui::device::Unit::Semitones,
            )
            .bipolar(),
        ),
        fp::GROOVE => with(Param::percent("groove")),
        fp::DRIVE => with(Param::percent("drive")),
        fp::WOW => with(Param::percent("wow")),
        fp::AGE => with(Param::percent("age")),
        fp::MIX => with(Param::percent("mix")),
        _ => with(Param::db("out", fp::OUT_MIN_DB, fp::OUT_MAX_DB)),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        fp::GROOVE | fp::DRIVE | fp::WOW | fp::AGE | fp::MIX => value * 100.0,
        fp::OUT => crate::dsp::arith::gain_to_db(value.max(1e-6)),
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        fp::GROOVE | fp::DRIVE | fp::WOW | fp::AGE | fp::MIX => value / 100.0,
        fp::OUT => crate::dsp::arith::db_to_gain(value),
        _ => value,
    };
    params::def(fp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized position.
pub fn ferric_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse, for a state stored in engine units.
pub fn ferric_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// The grid and the pattern snap; everything else sweeps.
pub fn ferric_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

pub fn ferric_edits(state: &FerricUi) -> Vec<ParamEdit> {
    let mut state = *state;
    fp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: ferric_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// How far back the head is placed on each step, in divisions, with
/// GROOVE applied — the staircase, as numbers.
///
/// A pure function of the knobs, so the figure can be checked without a
/// frame, and reading the same `PATTERNS` table the engine places from.
pub fn staircase(state: &FerricUi) -> [f32; fp::PATTERN_STEPS] {
    let index = (ferric_value(fp::PATTERN, state.pattern).round().max(0.0) as usize)
        .min(fp::PATTERNS.len() - 1);
    let groove = ferric_value(fp::GROOVE, state.groove).clamp(0.0, 1.0);
    let row = fp::PATTERNS
        .get(index)
        .copied()
        .unwrap_or([0; fp::PATTERN_STEPS]);
    let mut out = [0.0f32; fp::PATTERN_STEPS];
    for (slot, step) in out.iter_mut().zip(row.iter()) {
        *slot = f32::from(*step) * groove;
    }
    out
}

/// What the machine is doing this instant, from the engine.
///
/// A struct rather than four positional floats: they are all `f32` and a
/// caller that swapped two of them would compile.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transport {
    /// The grid step the head is on.
    pub step: f32,
    /// How many divisions back it was last placed.
    pub reach: f32,
    /// Peak level arriving at the tape, in dBFS.
    pub record_db: f32,
    /// How long a division actually lasts at the current tempo, in ms.
    /// Zero when the transport is not rolling and there is no grid.
    pub div_ms: f32,
}

/// Draw the tape looper's card. Returns the edits the user made.
///
/// `now` is the engine's, because a card is rebuilt from engine units
/// every frame and anything it kept would be forgotten.
pub fn ferric_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut FerricUi,
    now: Transport,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "ferric", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => machine(ui, theme, state, now),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the write head's line, the staircase under it, and the step
/// the transport is standing on.
fn machine(ui: &mut egui::Ui, theme: &Theme, state: &FerricUi, now: Transport) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);

    let stairs = staircase(state);
    let depth = deepest();
    // The record meter stands on the right, so the staircase gives it a
    // column rather than drawing underneath it.
    let meter_w = 13.0f32;
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(pad, font::MICRO_LABEL + pad * 2.0),
        rect.max - egui::vec2(pad * 2.0 + meter_w, pad + font::MICRO_LABEL + 3.0),
    );
    if plot.width() < 16.0 || plot.height() < 16.0 {
        return;
    }
    let steps = fp::PATTERN_STEPS as f32;
    let col = plot.width() / steps;
    // One row per whole division of reach, plus a little air.
    let unit = plot.height() / (depth + 0.6);
    let live_y = plot.top();
    let y_of = |back: f32| live_y + back * unit;

    // ---- the live line: the write head, this instant ------------------
    painter.line_segment(
        [
            egui::pos2(plot.left(), live_y),
            egui::pos2(plot.right(), live_y),
        ],
        egui::Stroke::new(1.0, theme.role_time),
    );
    painter.text(
        egui::pos2(plot.left(), rect.top()),
        egui::Align2::LEFT_TOP,
        {
            // Every figure in real units. "REACH 2" says how the head was
            // placed; "REACH 2 / 250ms" says how far back that IS, which
            // is the thing being listened for — and the division's own
            // length is what makes that number readable at all.
            let div_name = fp::DIVISION_NAMES
                .get(ferric_value(fp::DIVISION, state.division).round().max(0.0) as usize)
                .copied()
                .unwrap_or("1/16");
            let pattern = fp::PATTERN_NAMES
                .get(ferric_value(fp::PATTERN, state.pattern).round().max(0.0) as usize)
                .copied()
                .unwrap_or("run");
            let top = fp::top_hz(ferric_value(fp::AGE, state.age)) / 1000.0;
            let ratio = (ferric_value(fp::SPEED, state.speed) / 12.0).exp2();
            if now.div_ms > 0.0 {
                format!(
                    "x{ratio:.2}  {div_name} {:.0}ms  {pattern}  BACK {:.0}ms  TOP {top:.1}k",
                    now.div_ms,
                    now.reach * now.div_ms,
                )
            } else {
                format!("x{ratio:.2}  {div_name}  {pattern}  STOPPED  TOP {top:.1}k")
            }
        },
        mini.clone(),
        theme.text_muted,
    );

    // ---- the depth rules, one per whole division ----------------------
    //
    // Numbered down the left, so the vertical axis is a reading in
    // DIVISIONS BACK rather than a vague sense of lower-is-older.
    let mut back = 0.0f32;
    while back <= depth {
        let y = y_of(back);
        if back > 0.0 {
            painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                egui::Stroke::new(1.0, theme.divider),
            );
        }
        // Zero is the live line and reads as one of the ladder, which is
        // what it is. A word floated at the other end instead ("LIVE")
        // landed on whatever tread happened to reach the right edge.
        painter.text(
            egui::pos2(plot.left() + 2.0, y + 1.0),
            egui::Align2::LEFT_TOP,
            if back == 0.0 {
                " 0".to_string()
            } else {
                format!("-{back:.0}")
            },
            mini.clone(),
            if back == 0.0 {
                theme.role_time
            } else {
                theme.text_muted
            },
        );
        back += 1.0;
    }

    // ---- the staircase ------------------------------------------------
    let lit = (now.step.round().max(0.0) as usize) % fp::PATTERN_STEPS;
    for (i, drop) in stairs.iter().enumerate() {
        let x0 = plot.left() + col * i as f32;
        let x1 = x0 + col;
        let y = y_of(*drop);
        let on = i == lit;
        let ink = if on { theme.role_mod } else { theme.role_shape };
        let weight = if on { 2.0 } else { 1.2 };

        // The lit column, behind everything, so the eye finds the step
        // before it reads the figure.
        if on {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, plot.top()), egui::pos2(x1, plot.bottom())),
                0.0,
                theme.role_mod.gamma_multiply(0.13),
            );
        }
        // The tread.
        painter.line_segment(
            [egui::pos2(x0, y), egui::pos2(x1, y)],
            egui::Stroke::new(weight, ink),
        );
        // The riser: a right angle, because a step boundary is an
        // instant and the head does not slide into place.
        let previous = if i == 0 {
            stairs.get(fp::PATTERN_STEPS - 1).copied().unwrap_or(0.0)
        } else {
            stairs.get(i - 1).copied().unwrap_or(0.0)
        };
        if (previous - *drop).abs() > 1e-4 {
            painter.line_segment(
                [egui::pos2(x0, y_of(previous)), egui::pos2(x0, y)],
                egui::Stroke::new(weight, ink),
            );
        }
        // The division tick, NUMBERED — eight anonymous marks say the
        // cycle has eight steps and nothing about which one is which,
        // and the lit column is the only other clue.
        painter.line_segment(
            [
                egui::pos2(x0, plot.bottom()),
                egui::pos2(x0, plot.bottom() - 3.0),
            ],
            egui::Stroke::new(1.0, theme.divider),
        );
        painter.text(
            egui::pos2(x0 + col * 0.5, plot.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{}", i + 1),
            mini.clone(),
            if on { theme.role_mod } else { theme.divider },
        );
    }

    // ---- the read head, on the step being played ----------------------
    let head_x = plot.left() + col * (lit as f32 + 0.5);
    let head_y = y_of(stairs.get(lit).copied().unwrap_or(0.0));
    let s = 3.5;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(head_x, head_y - s),
            egui::pos2(head_x + s, head_y + s),
            egui::pos2(head_x - s, head_y + s),
        ],
        theme.role_mod,
        egui::Stroke::NONE,
    ));

    painter.text(
        egui::pos2(plot.left(), plot.bottom() + 2.0),
        egui::Align2::LEFT_TOP,
        "DIVISIONS BACK",
        mini.clone(),
        theme.text_muted,
    );

    // ---- the record meter --------------------------------------------
    //
    // What is arriving AT THE TAPE, which is the level the drive works
    // on: saturation happens on the way to the reel, so a machine that
    // sounds too clean or too crushed is answered here rather than by
    // guessing. Ticks at 0, -6, -12 and -24, and the top of the scale in
    // the warning colour, which is what a meter on a tape machine has
    // always meant.
    let meter = egui::Rect::from_min_max(
        egui::pos2(rect.right() - pad - meter_w, plot.top()),
        egui::pos2(rect.right() - pad, plot.bottom()),
    );
    const METER_FLOOR: f32 = -36.0;
    const METER_TOP: f32 = 6.0;
    let db_y = |db: f32| {
        let t = ((db - METER_FLOOR) / (METER_TOP - METER_FLOOR)).clamp(0.0, 1.0);
        meter.bottom() - meter.height() * t
    };
    painter.rect_filled(meter, 0.0, theme.surface_sunken);
    // The hot band, marked before anything is in it.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(meter.left(), db_y(METER_TOP)),
            egui::pos2(meter.right(), db_y(-6.0)),
        ),
        0.0,
        theme.role_mod.gamma_multiply(0.12),
    );
    let level = now.record_db.clamp(METER_FLOOR, METER_TOP);
    if now.record_db > METER_FLOOR {
        let top = db_y(level);
        let ink = if now.record_db > -6.0 {
            theme.role_mod
        } else {
            theme.role_time
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(meter.left() + 1.0, top),
                egui::pos2(meter.right() - 1.0, meter.bottom()),
            ),
            0.0,
            ink,
        );
    }
    for db in [0.0f32, -6.0, -12.0, -24.0] {
        let y = db_y(db);
        painter.line_segment(
            [egui::pos2(meter.left(), y), egui::pos2(meter.right(), y)],
            egui::Stroke::new(1.0, theme.divider),
        );
    }
    painter.rect_stroke(
        meter,
        0.0,
        egui::Stroke::new(1.0, theme.outline),
        egui::StrokeKind::Middle,
    );
    painter.text(
        egui::pos2(meter.center().x, meter.bottom() + 2.0),
        egui::Align2::CENTER_TOP,
        "REC",
        mini,
        theme.text_muted,
    );

    crate::ui::hud::brackets(&painter, plot, egui::Stroke::new(1.0, theme.outline));
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut FerricUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);

    ui.horizontal(|ui| {
        for (drawn, id) in CELLS.iter().enumerate() {
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let p = param_of(*id);
            let Some(slot) = state.slot(*id) else {
                continue;
            };
            let before = *slot;
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: ferric_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 640.0;
    const MIN_W: f32 = 300.0;

    /// A machine mid-run, for the drawing tests.
    fn probe_now() -> Transport {
        Transport {
            step: 2.0,
            reach: 1.0,
            record_db: -9.0,
            div_ms: 125.0,
        }
    }

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
    fn every_table_row_leaves_as_an_edit() {
        let edits = ferric_edits(&FerricUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = fp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in fp::TABLE {
            for i in 0..=40 {
                let value = ferric_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!((ferric_value(def.id, 0.0) - def.min).abs() < span * 1e-3);
            assert!((ferric_value(def.id, 1.0) - def.max).abs() < span * 1e-3);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in fp::TABLE {
            for i in 0..=40 {
                let value = ferric_value(def.id, i as f32 / 40.0);
                let again = ferric_value(def.id, ferric_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in fp::TABLE {
            let mut state = FerricUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = ferric_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// THE FIGURE IS THE PATTERN: the staircase the card draws is the
    /// array the engine places the head from, scaled by GROOVE.
    ///
    /// Both read `params::ferric::PATTERNS`, so this checks the SCALING
    /// and the wiring rather than a second copy of the data — there is
    /// no second copy, which is the point.
    #[test]
    fn the_staircase_is_the_pattern_the_engine_plays() {
        let mut state = FerricUi::default();
        state.groove = ferric_norm(fp::GROOVE, 1.0);
        for (index, row) in fp::PATTERNS.iter().enumerate() {
            state.pattern = ferric_norm(fp::PATTERN, index as f32);
            let drawn = staircase(&state);
            for (got, want) in drawn.iter().zip(row.iter()) {
                assert_eq!(*got, f32::from(*want), "pattern {index}");
            }
        }

        // GROOVE scales it, and at zero the figure IS the live line —
        // which is exactly what the device does there.
        state.pattern = ferric_norm(fp::PATTERN, 1.0); // stutter
        state.groove = ferric_norm(fp::GROOVE, 0.0);
        assert_eq!(staircase(&state), [0.0; fp::PATTERN_STEPS]);
        state.groove = ferric_norm(fp::GROOVE, 0.5);
        let half = staircase(&state);
        assert_eq!(half[1], 0.5, "half groove should halve the drop");
        assert_eq!(half[3], 1.5);

        // `run` is flat at every groove, because it has nowhere to go.
        state.pattern = ferric_norm(fp::PATTERN, 0.0);
        for groove in [0.0f32, 0.5, 1.0] {
            state.groove = ferric_norm(fp::GROOVE, groove);
            assert_eq!(
                staircase(&state),
                [0.0; fp::PATTERN_STEPS],
                "run at {groove}"
            );
        }
    }

    /// The grid and the pattern snap to whole choices; the rest sweep.
    #[test]
    fn the_grid_and_the_pattern_snap() {
        assert!(ferric_is_discrete(fp::DIVISION));
        assert!(ferric_is_discrete(fp::PATTERN));
        for id in [
            fp::SPEED,
            fp::GROOVE,
            fp::DRIVE,
            fp::WOW,
            fp::AGE,
            fp::MIX,
            fp::OUT,
        ] {
            assert!(!ferric_is_discrete(id), "{id} should sweep");
        }
        // Every division and every pattern is reachable from the knob.
        for (id, count) in [
            (fp::DIVISION, fp::DIVISION_BEATS.len()),
            (fp::PATTERN, fp::PATTERNS.len()),
        ] {
            let mut seen = std::collections::BTreeSet::new();
            for i in 0..=100 {
                seen.insert(ferric_value(id, i as f32 / 100.0).round() as i32);
            }
            assert_eq!(seen.len(), count, "not every position of {id} is reachable");
        }
    }

    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(plot > cell * 3.0, "the transport is left only {plot} pt");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FerricUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| ferric_card(ui, &theme, &mut state, probe_now()))
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
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FerricUi::default();
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
                ferric_card(&mut child, &theme, &mut state, probe_now());
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt",
                used.width()
            );
            assert!(used.width() > panel_w * 0.7);
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FerricUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| ferric_card(ui, &theme, &mut state, probe_now()))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// POINTER: every cell reachable, one control per drag. Both
    /// directions, for the reason `sibyl`'s twin gives.
    #[test]
    fn every_cell_is_reachable_and_a_drag_moves_only_one() {
        use crate::ui::device::probe;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let width = frame(&ctx, |ui| face_width(ui, &theme)).ceil() * 1.2;
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, theme.sp(control::DEVICE_TALL_H)),
        );
        let cell_h = theme.sp(crate::ui::tokens::control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        for step in 0..64 {
            for dy in [20.0f32, -20.0] {
                let mut state = FerricUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 64.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| {
                    ferric_card(ui, &theme, &mut state, probe_now())
                });
                let touched: std::collections::BTreeSet<u32> =
                    edits.into_iter().flatten().map(|e| e.param).collect();
                assert!(touched.len() <= 1, "one drag at x={x:.0} moved {touched:?}");
                reached.extend(touched);
            }
        }
        let expect: std::collections::BTreeSet<u32> = CELLS.iter().copied().collect();
        assert_eq!(
            reached,
            expect,
            "a sweep never reached {:?}",
            expect.difference(&reached).collect::<Vec<_>>()
        );
    }
}
