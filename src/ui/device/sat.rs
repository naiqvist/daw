//! The saturator device card — the transfer curve as an instrument
//! screen.
//!
//! Same shape as the reverb card: normalized knob state here, natural
//! values out as [`ParamEdit`]s. Ids, ranges and defaults come from
//! [`crate::params::sat`] — the one table this widget, `Node::Sat::apply`
//! and the app's edit routing all read.
//!
//! # The anatomy, from `notes/20260826-instrument-screen-design-guide.md`
//!
//! ```text
//! ┌ saturator ─────────────────────────────┐
//! │                                        │
//! │          transfer curve (hero)         │
//! │                                        │
//! ├ shape ───┬ drive ───────┬ output ──────┤
//! │  mode    │ drive  bias  │  mix   out   │
//! └──────────┴──────────────┴──────────────┘
//! ```
//!
//! The guide's rules this card is built to, and where each one landed:
//!
//! - **The picture is the control.** The curve is not an illustration
//!   beside the knobs; it IS the drive and bias control, dragged
//!   vertically and horizontally. The knobs below are the precision half
//!   of the same pair, not a second way to do the only thing that works.
//! - **A screen answers three questions.** Where am I: the card title and
//!   the mode named at the display's edge. What is happening: the shape,
//!   drawn from the same `shape()` the audio runs. What can I change:
//!   five controls, in fixed positions, in natural units.
//! - **Left to right is the signal.** Shape, then how hard it is driven,
//!   then what leaves — the order the sound actually travels, which is the
//!   order the poly card reads in too.
//! - **Colour is an index.** Nothing here picks a colour. The display
//!   wears `role_mod` because a drive curve is the destructive edge, and
//!   every control below takes its family from its own `Unit` through
//!   `poly_widgets::role_color` — so `bias`, being bipolar, reads as
//!   modulation and `mix` as level without this file having an opinion.
//! - **One hero.** The curve is the only bold stroke on the card.
//!
//! # Every value is in a unit you could say out loud
//!
//! The engine's units are the kernel's (`1..32` of drive, `-0.9..0.9` of
//! bias, `0..4` of linear gain) and NONE of them is a thing to show a
//! musician. The mapping from one to the other lives in [`sat_value`] and
//! its inverse, in this file, once — the widget shows `4.0x`, `+25 %`,
//! `-6.0 dB`, and the engine still receives exactly what
//! `crate::params::sat::TABLE` describes.

use crate::params::sat::{BIAS, DRIVE, MIX, MODE, OUT, TABLE};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, shaper, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The output trim's window, in dB — the table's own figures, not a
/// second opinion about how loud this device can get. The engine stores
/// linear gain and the knob shows dB; both ends of the one range live in
/// `params::sat` so the two forms cannot drift.
use crate::params::sat::{OUT_MAX_DB, OUT_MIN_DB};

/// Knob positions of one saturator, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SatUi {
    pub mode: f32,
    pub drive: f32,
    pub bias: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SatUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the knobs use. A default written here as a knob position
        // would be a second opinion about what a fresh saturator is.
        Self {
            mode: sat_norm(MODE, params::def(TABLE, MODE).default),
            drive: sat_norm(DRIVE, params::def(TABLE, DRIVE).default),
            bias: sat_norm(BIAS, params::def(TABLE, BIAS).default),
            mix: sat_norm(MIX, params::def(TABLE, MIX).default),
            out: sat_norm(OUT, params::def(TABLE, OUT).default),
        }
    }
}

impl SatUi {
    /// This state's knob position for a wire id, mutably. One place an
    /// id becomes a field, so a loop over the parameter table cannot
    /// route an edit into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            MODE => &mut self.mode,
            DRIVE => &mut self.drive,
            BIAS => &mut self.bias,
            MIX => &mut self.mix,
            OUT => &mut self.out,
            _ => return None,
        })
    }
}

struct Spec {
    mode: Param,
    drive: Param,
    bias: Param,
    mix: Param,
    out: Param,
}

/// The five controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        // The display's own mode list, so the strip and the curve cannot
        // name different shapes.
        mode: default(Param::choice("mode", shaper::Mode::NAMES), MODE),
        // LOG-mapped: drive is a gain, and equal travel per doubling is
        // the only mapping where the top half of the dial is not all the
        // same amount of "very driven".
        drive: default(
            Param::new(
                "drive",
                Mapping::Log {
                    min: shaper::DRIVE_MIN,
                    max: shaper::DRIVE_MAX,
                },
                Unit::Ratio,
            ),
            DRIVE,
        ),
        // BIPOLAR, and shown as percent of the curve's offset. The zero is
        // visible on the dial and on the display's crosshair, because a
        // bipolar control with no visible zero cannot be returned to
        // centre by eye.
        bias: default(
            Param::new(
                "bias",
                Mapping::Linear {
                    min: -100.0,
                    max: 100.0,
                },
                Unit::Percent,
            )
            .bipolar(),
            BIAS,
        ),
        mix: default(Param::percent("mix"), MIX),
        out: default(Param::db("out", OUT_MIN_DB, OUT_MAX_DB), OUT),
    }
}

/// Linear gain -> dB, with the table's silent bottom folded onto the
/// dial's floor rather than producing an infinity.
fn db_of(gain: f32) -> f32 {
    if gain <= 0.0 {
        OUT_MIN_DB
    } else {
        (20.0 * gain.log10()).clamp(OUT_MIN_DB, OUT_MAX_DB)
    }
}

/// What the widget SHOWS for an engine-facing value: the inverse of
/// [`natural`], and the other half of the one mapping this card owns.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        // Percent OF THE LEGAL OFFSET, not of a whole unit: the kernel's
        // bias tops out at `BIAS_MAX`, and a dial reading "90 %" at the
        // end of its travel is a dial that looks broken. Full deflection
        // is 100 % of what bias can be.
        BIAS => value / shaper::BIAS_MAX * 100.0,
        MIX => value * 100.0,
        OUT => db_of(value),
        // Mode is an index and drive is already the kernel's own ratio —
        // for these two the engine's number IS the displayed one.
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows.
///
/// CLAMPED through the row, so a knob at either stop cannot emit a letter
/// the engine has to bin. The trim is why: its ends are a dB figure and a
/// linear one, converted at runtime by `powf`, and the two forms of the
/// same bound agree to about an ulp — near enough for audio, and not near
/// enough for a range check. Guaranteeing legality by construction is
/// better than hoping two roundings land on the same float.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        BIAS => value / 100.0 * shaper::BIAS_MAX,
        MIX => value / 100.0,
        OUT => 10.0f32.powf(value / 20.0),
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id. The single place an id becomes a `Param`, so
/// the four functions below cannot disagree about which knob is which.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        MODE => s.mode,
        DRIVE => s.drive,
        BIAS => s.bias,
        MIX => s.mix,
        _ => s.out,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn sat_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`sat_value`], for a state stored in engine units.
pub fn sat_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings rather than sweeping.
/// Only the mode does — asked of the `Param` itself rather than answered
/// from a list here, so it cannot fall behind the spec.
pub fn sat_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. Drive does, which is what
/// makes a modulation sweep of it move in doublings the way the knob
/// does.
pub fn sat_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved — for a reset, a
/// preset recall, or the moment the device is first loaded.
pub fn sat_edits(state: &SatUi) -> Vec<ParamEdit> {
    [
        (MODE, state.mode),
        (DRIVE, state.drive),
        (BIAS, state.bias),
        (MIX, state.mix),
        (OUT, state.out),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: sat_value(param, norm),
    })
    .collect()
}

/// The curve as the display needs it, built from the card's state — so
/// the drawing is a rendering of the same numbers the engine has, never a
/// copy kept alongside them.
fn shaper_of(state: &SatUi) -> shaper::Shaper {
    let s = spec();
    shaper::Shaper {
        mode: shaper::Mode::from_index(s.mode.index(state.mode)),
        drive: sat_value(DRIVE, state.drive),
        bias: sat_value(BIAS, state.bias),
        mix: sat_value(MIX, state.mix),
    }
}

/// The card's anatomy, straight from the screen guide:
///
/// ```text
/// ┌ saturator ──────────────┐  context strip
/// │ ┌ screen ─────────────┐ │
/// │ │  soft  1.0x         │ │
/// │ │        curve        │ │  primary live graphic
/// │ │                     │ │
/// │ │ drive bias mix out  │ │  parameter cells, INSIDE the screen
/// │ └─────────────────────┘ │
/// ├ hard soft cubic fold cr ┤  local rail
/// └─────────────────────────┘
/// ```
///
/// The knobs live INSIDE the screen, along its bottom, rather than in a
/// tray beneath it. That is the Elektron move the guide's reference board
/// points at — values anchored at the display's own edge — and it is what
/// lets the screen be the size of the card instead of one object on it.
/// A panel with the controls outside it is a panel competing with a tray;
/// a panel with the controls in its floor is an instrument.
///
/// The plot cannot overlap them, by construction rather than by margin:
/// [`poly_widgets::dark_curve_panel`] pays for the footer rows first and
/// hands the plot everything left over.
///
/// The mode is the FIRST CELL of that strip, not a rail of its own.
///
/// A five-name strip along the bottom of the card was the widest thing on
/// it — it set the card's width single-handedly — and it cost a whole row
/// that the graphic wanted. As a cell it costs one fifth of a row it was
/// already paying for, and it loses nothing the guide asks of a discrete
/// control: the current choice is written out (`SOFT`, not a dial
/// position), and `drive_cell` gives it click-halves for back and
/// forward, the wheel, the arrow keys and detented dragging.
///
/// What it gives up is seeing all five names at once. That is the trade
/// for a hero twice the size, and a mode is a thing you pick occasionally
/// rather than read continuously.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    // ONE well, and the screen fills it. Nothing else is on this card.
    // Its only declared size is the strip's width; the height it takes
    // is whatever the card has left.
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// The width the value strip needs: every cell wide enough for the widest
/// thing it will ever print, measured rather than guessed.
///
/// This is now the card's ONLY width contract. The well holding the
/// screen has no size of its own — it is the hero, so it is the thing
/// that grows — which means without this the card would collapse to the
/// section floor and the strip would print through itself.
/// What ONE cell needs: room for the widest thing it will ever print,
/// measured through the font atlas rather than guessed.
///
/// The strip's width is the sum of these and each cell is drawn at its
/// own, so the two cannot disagree — which is the whole reason this is a
/// function and not two similar expressions. The first version reserved
/// the SUM of the needs and then drew the cells in EQUAL columns, and a
/// fifth of that total is less than `+12.0 dB` asked for, so the dB
/// readout printed straight out through its neighbours. A share is not a
/// sum; when a layout uses one, the contract has to use the same one.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The five cells of the strip, in the order they are drawn.
fn strip(s: &Spec) -> [(&Param, u32); 5] {
    [
        (&s.mode, MODE),
        (&s.drive, DRIVE),
        (&s.bias, BIAS),
        (&s.mix, MIX),
        (&s.out, OUT),
    ]
}

fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in.
///
/// Two, because a [`poly_widgets::labeled_cell`] draws a value over a
/// name and one row cannot hold both without them touching.
///
/// The strip replaced a row of full KNOBS, and the graph is why. A dial
/// costs its own diameter plus a label above and a value below — near
/// seventy points of a panel that only has a hundred and sixty to give,
/// which left the hero smaller than the controls annotating it. A cell
/// spends its height on the two things a screen owes you (what this is,
/// what it is set to) and nothing on a picture of a knob, and hands the
/// difference back to the plot.
///
/// Nothing is lost by it: a cell drags, wheels, arrows and double-click
/// resets exactly as the knob did, and unlike a mini knob it shows its
/// value at rest rather than only under the pointer.
const VALUE_ROWS: usize = 2;

/// Draw the saturator card. Returns the edits the user just made.
pub fn sat_card(ui: &mut egui::Ui, theme: &Theme, state: &mut SatUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "saturator", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // The hero. Dragging it moves drive and bias, so the
                    // values come back OUT of the widget and into the
                    // card's normalized state — the picture is the
                    // control, and a control that could not write is a
                    // picture.
                    poly_widgets::CurveRegion::Plot => {
                        let mut curve = shaper_of(state);
                        if shaper::transfer_plot(ui, theme, &mut curve) {
                            state.drive = sat_norm(DRIVE, curve.drive);
                            state.bias = sat_norm(BIAS, curve.bias);
                            edits.push(ParamEdit {
                                param: DRIVE,
                                value: sat_value(DRIVE, state.drive),
                            });
                            edits.push(ParamEdit {
                                param: BIAS,
                                value: sat_value(BIAS, state.bias),
                            });
                        }
                    }
                    // Four columns, so the values sit on one baseline at
                    // even centres however wide the card is drawn — and
                    // each cell takes its family colour from its own
                    // `Unit`, so bias reads as modulation and mix as
                    // level without this file choosing a colour.
                    // Each cell at its OWN width, not an equal share —
                    // so the strip is as wide as its numbers need and
                    // `+12.0 dB` cannot print through `100 %`.
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
                                        // The BAR variant: bias is
                                        // bipolar, and the needle a
                                        // bipolar cell would draw is a
                                        // second picture of what the curve
                                        // above already shows by sliding
                                        // off its own crosshair — a stray
                                        // knob on a card whose whole point
                                        // is that it has none.
                                        if poly_widgets::labeled_cell_bar(
                                            ui, theme, param, norm, None,
                                        ) {
                                            edits.push(ParamEdit {
                                                param: id,
                                                value: sat_value(id, *norm),
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

    /// The card fits its budget — and this test exists because the first
    /// version of it did not, by a factor of four.
    ///
    /// A well's width requirement is solved for the whole row, so a cell
    /// whose span is small relative to its content multiplies the CARD.
    /// That failure is invisible in the source (every number involved is
    /// a token, and every widget is the right size) and unmissable on
    /// screen: the card filled the entire rack. Nothing caught it,
    /// because nothing was looking.
    ///
    /// The lower bound matters as much as the upper one: a card that
    /// silently collapsed would pass a ceiling-only check.
    #[test]
    fn the_card_stays_inside_its_budget() {
        // Roughly SQUARE: the card is DEVICE_TALL_H tall, and stacking
        // the controls under the graphic is what keeps its width in the
        // same neighbourhood instead of nearly double it.
        const MAX_W: f32 = 300.0;
        const MIN_W: f32 = 200.0;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SatUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| sat_card(ui, &theme, &mut state))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        // The tall silhouette, exactly — a card is one of the two
        // heights the system has, never a number of its own.
        //
        // Plus the frame's own outline: `card_frame` strokes a hairline
        // and `set_height` sizes the INSIDE, so the outer rect is two
        // hairlines taller than the token by construction. Spelled out
        // rather than absorbed into a round tolerance — a fudge factor
        // here would be exactly the slack a real overflow hides in.
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt height budget"
        );
    }

    /// Drawing the card at rest must not move a control. A layout that
    /// emits on the first frame writes its own defaults over a loaded
    /// patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SatUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| sat_card(ui, &theme, &mut state))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    #[test]
    fn every_table_knob_leaves_as_an_edit() {
        // The widget must cover the table exactly: an id the table knows
        // but the card never emits is a knob that silently stopped
        // working.
        let edits = sat_edits(&SatUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
    }

    /// Every edit this card can emit is inside the range the engine
    /// clamps to — so no knob position can produce a letter the node has
    /// to bin.
    ///
    /// True by construction now that [`natural`] clamps, which is the
    /// point; the second half is what stops that guarantee from being
    /// vacuous. A dial whose ends did NOT reach the row's ends would pass
    /// the legality check while quietly making part of every parameter
    /// unreachable, and clamping is exactly the thing that would hide it.
    #[test]
    fn every_knob_position_is_a_legal_engine_value() {
        for def in TABLE {
            for i in 0..=40 {
                let value = sat_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: knob at {i}/40 gives {value}, outside [{}, {}]",
                    def.name,
                    def.min,
                    def.max
                );
            }
            let (bottom, top) = (sat_value(def.id, 0.0), sat_value(def.id, 1.0));
            assert_eq!(
                bottom, def.min,
                "{}: the dial never reaches its floor",
                def.name
            );
            assert_eq!(
                top, def.max,
                "{}: the dial never reaches its ceiling",
                def.name
            );
        }
    }

    /// The card loads as the table says a saturator is. Stated as a test
    /// because a default written twice is a default that drifts.
    #[test]
    fn defaults_come_from_the_table() {
        let edits = sat_edits(&SatUi::default());
        for def in TABLE {
            let got = edits
                .iter()
                .find(|e| e.param == def.id)
                .map(|e| e.value)
                .unwrap_or(f32::NAN);
            assert!(
                (got - def.default).abs() < 1e-4,
                "{} loads at {got}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Knob position and engine value are inverses, for every parameter.
    /// The property the whole two-way mapping rests on: a value that
    /// comes back from the engine — a project load, an automation pass —
    /// must put the knob where that value lives.
    ///
    /// Stated over VALUES rather than positions, because a position
    /// cannot survive a discrete parameter: five modes share a continuous
    /// dial, so four positions out of five quantize to a neighbour on the
    /// way in and the position is not what has to be preserved. The value
    /// is.
    #[test]
    fn value_and_norm_round_trip() {
        for def in TABLE {
            for i in 0..=40 {
                let norm = i as f32 / 40.0;
                let value = sat_value(def.id, norm);
                let again = sat_value(def.id, sat_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-4,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    /// The curve the card hands the display is the settings the engine
    /// has — the same agreement the node's own test makes on the audio
    /// side, checked here at the point where the two could diverge.
    #[test]
    fn the_display_shows_what_the_engine_was_sent() {
        let state = SatUi {
            mode: spec().mode.at_index(3), // fold
            drive: 0.7,
            bias: 0.8,
            mix: 0.5,
            out: 0.5,
        };
        let curve = shaper_of(&state);
        let edits = sat_edits(&state);
        let value = |id: u32| {
            edits
                .iter()
                .find(|e| e.param == id)
                .map(|e| e.value)
                .unwrap_or(f32::NAN)
        };
        assert_eq!(curve.mode, shaper::Mode::Fold);
        assert!((curve.drive - value(DRIVE)).abs() < 1e-6);
        assert!((curve.bias - value(BIAS)).abs() < 1e-6);
        assert!((curve.mix - value(MIX)).abs() < 1e-6);
    }
}
