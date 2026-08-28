//! The sheen device card — the brightener's response field as an instrument
//! screen.
//!
//! Same shape as the lo-fi card: normalized knob state here, natural
//! values out as [`ParamEdit`]s. Ids, ranges and defaults come from
//! [`crate::params::sheen`] — the one table this widget, `Node::Sheen`'s
//! apply arm and the app's edit routing all read.
//!
//! # The anatomy, from `notes/20260826-instrument-screen-design-guide.md`
//!
//! ```text
//! ┌ sheen ──────────────────────────────────┐
//! │ SLEW × EDGE                    edge ×1.2│
//! │ fast  ░░▒▒▓▓████████████████           │
//! │ ½lift ┄┄┄┄┄┄┄┄┄┄┄│┄┄┄┄┄┄┄┄           │  ← hero
//! │ slow  ·······░░▒▒▒│▓▓██████            │
//! │        low       1.5 kHz       high     │
//! │  amount   edge      mix       out       │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Why the hero is a response field
//!
//! This device has no signal display. Drawing a waveform here would imply it
//! came from the input when the card has never received input telemetry. A
//! synthetic drum hit may exercise the kernel honestly, but it still reads as
//! programme material, which makes the screen tell the wrong story.
//!
//! The field shows the algorithm instead. Frequency runs left to right; input
//! slew runs slow to fast from bottom to top. Each cell is the product the DSP
//! actually adds:
//!
//! ```text
//! edge-band magnitude × saturating slew lift × amount × mix
//! ```
//!
//! The vertical marker is the edge-band corner. The horizontal marker is the
//! kernel's fixed knee, where a full-scale sine drives half lift. Brightness is
//! the current Amount and Mix, normalised only against the kernel's authored
//! maximum — never auto-fitted — so a stronger setting produces a stronger
//! field and zero produces an exact `edge off` state.
//!
//! The output trim is deliberately not in the picture: it is a level, and
//! the field is about where and when colour is added.
//!
//! # Every value is in a unit you could say out loud
//!
//! The engine stores the kernel's own multiplier, a corner in hertz, a
//! fraction and a linear gain. The widget shows `30 %`, `1.50 kHz`,
//! `100 %` and `-6.0 dB`. The mapping lives in [`sheen_value`] and its
//! inverse, in this file, once.

use crate::params::sheen::{
    AMOUNT, AMOUNT_MAX, EDGE, EDGE_MAX_HZ, EDGE_MIN_HZ, MIX, OUT, OUT_MAX_DB, OUT_MIN_DB, TABLE,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions of one sheen, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SheenUi {
    pub amount: f32,
    pub edge: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SheenUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the knobs use.
        Self {
            amount: sheen_norm(AMOUNT, params::def(TABLE, AMOUNT).default),
            edge: sheen_norm(EDGE, params::def(TABLE, EDGE).default),
            mix: sheen_norm(MIX, params::def(TABLE, MIX).default),
            out: sheen_norm(OUT, params::def(TABLE, OUT).default),
        }
    }
}

impl SheenUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the parameter table cannot route
    /// an edit into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            AMOUNT => &mut self.amount,
            EDGE => &mut self.edge,
            MIX => &mut self.mix,
            OUT => &mut self.out,
            _ => return None,
        })
    }
}

struct Spec {
    amount: Param,
    edge: Param,
    mix: Param,
    out: Param,
}

/// The four controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        // PERCENT of what the kernel will do at all, not of a whole unit:
        // full deflection is the kernel's own ceiling, so a dial reading
        // "30 %" at the top of its travel would be a dial that looks
        // broken. Same argument `sat::shown` makes about bias.
        amount: default(Param::percent("amount"), AMOUNT),
        // LOG-mapped, by `Param::hz`: a filter corner is a frequency, and
        // equal travel per octave is the only mapping where the top of
        // the dial is not all the same amount of "very bright".
        edge: default(Param::hz("edge", EDGE_MIN_HZ, EDGE_MAX_HZ), EDGE),
        mix: default(Param::percent("mix"), MIX),
        out: default(Param::db("out", OUT_MIN_DB, OUT_MAX_DB), OUT),
    }
}

/// Linear gain -> dB, with the table's floor folded onto the dial's
/// bottom rather than producing an infinity.
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
        AMOUNT => value / AMOUNT_MAX * 100.0,
        MIX => value * 100.0,
        OUT => db_of(value),
        // The corner is already the kernel's own hertz.
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows.
///
/// CLAMPED through the row for the reason `sat::natural` gives: the
/// trim's ends are a dB figure and a linear one, converted at runtime by
/// `powf`, and the two forms of the same bound agree to about an ulp —
/// near enough for audio, and not near enough for a range check.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        AMOUNT => value / 100.0 * AMOUNT_MAX,
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
        AMOUNT => s.amount,
        EDGE => s.edge,
        MIX => s.mix,
        _ => s.out,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn sheen_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`sheen_value`], for a state stored in engine units.
pub fn sheen_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings rather than sweeping.
/// None here does — every control is continuous.
pub fn sheen_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The corner does, which is
/// what makes a modulation sweep of it move in octaves the way the knob
/// does.
pub fn sheen_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved — for a reset, a
/// preset recall, or the moment the device is first loaded.
pub fn sheen_edits(state: &SheenUi) -> Vec<ParamEdit> {
    [
        (AMOUNT, state.amount),
        (EDGE, state.edge),
        (MIX, state.mix),
        (OUT, state.out),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: sheen_value(param, norm),
    })
    .collect()
}

/// The reference rate the response field is calculated at. The node's real
/// rate is not UI telemetry; 48 kHz is the project's reference and is also the
/// rate the kernel uses when constructed without a stream.
const PLOT_SR: f32 = 48_000.0;

/// The response field's frequency span. It extends beyond the Edge knob at
/// both ends so the marker is never pinned to the frame and the high-pass
/// transition remains visible at either stop.
const VIEW_MIN_HZ: f32 = 40.0;
const VIEW_MAX_HZ: f32 = 20_000.0;

/// The vertical field is envelope slew relative to the kernel's knee. A
/// symmetric four octaves either side puts the exact half-lift line in the
/// middle while still showing the saturating ceiling and the quiet floor.
const SLEW_RATIO_MIN: f32 = 1.0 / 16.0;
const SLEW_RATIO_MAX: f32 = 16.0;

/// A deliberately discrete data surface: enough cells to read a smooth
/// transition, few enough to look authored rather than like a decorative
/// gradient.
const FIELD_COLS: usize = 32;
const FIELD_ROWS: usize = 12;

/// Log frequency geometry, shared by the cells and the corner marker.
fn hz_at(t: f32) -> f32 {
    VIEW_MIN_HZ * (VIEW_MAX_HZ / VIEW_MIN_HZ).powf(t.clamp(0.0, 1.0))
}

fn hz_to_x(hz: f32) -> f32 {
    (hz.clamp(VIEW_MIN_HZ, VIEW_MAX_HZ) / VIEW_MIN_HZ).ln() / (VIEW_MAX_HZ / VIEW_MIN_HZ).ln()
}

/// The magnitude of `x - lowpass(x)` at `hz`, using the exact exponential
/// one-pole coefficient in `SlewBrighten::prepare` rather than a generic
/// high-pass approximation.
fn edge_magnitude(hz: f32, corner_hz: f32) -> f32 {
    let corner = corner_hz.clamp(20.0, PLOT_SR * 0.45);
    let g = 1.0 - (-core::f32::consts::TAU * corner / PLOT_SR).exp();
    let a = 1.0 - g;
    let w = core::f32::consts::TAU * hz.clamp(0.0, PLOT_SR * 0.5) / PLOT_SR;
    let (cos, sin) = (w.cos(), w.sin());
    let num_re = a * (1.0 - cos);
    let num_im = a * sin;
    let den_re = 1.0 - a * cos;
    let den_im = a * sin;
    let den = (den_re * den_re + den_im * den_im).sqrt();
    if den <= f32::MIN_POSITIVE {
        0.0
    } else {
        (num_re * num_re + num_im * num_im).sqrt() / den
    }
}

/// The kernel's saturating lift, stated against its knee. At ratio 1 the
/// answer is exactly one half; it approaches one but can never run away.
fn slew_lift(ratio: f32) -> f32 {
    let ratio = ratio.max(0.0);
    ratio / (ratio + 1.0)
}

/// What fraction of the kernel's authored maximum this cell adds. This is
/// not output gain: it is the multiplier on the edge band at one frequency
/// and one envelope slew.
fn response_strength(state: &SheenUi, hz: f32, slew_ratio: f32) -> f32 {
    let amount = sheen_value(AMOUNT, state.amount);
    let mix = sheen_value(MIX, state.mix);
    (amount / AMOUNT_MAX
        * mix
        * edge_magnitude(hz, sheen_value(EDGE, state.edge))
        * slew_lift(slew_ratio))
    .clamp(0.0, 1.0)
}

/// The new hero: a signal-independent map of the actual DSP relationship.
fn response_field(ui: &mut egui::Ui, theme: &Theme, state: &SheenUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);

    let label_h = theme.sp(font::MICRO_LABEL) + theme.sp(space::XXS);
    let field = egui::Rect::from_min_max(
        egui::pos2(plot.left(), plot.top() + label_h),
        egui::pos2(plot.right(), plot.bottom() - label_h),
    );
    let cell_w = field.width() / FIELD_COLS as f32;
    let cell_h = field.height() / FIELD_ROWS as f32;
    let cell_inset = stroke::HAIR * 0.5;

    for row in 0..FIELD_ROWS {
        // Top is FAST. The ratio scale is logarithmic, so the fixed knee
        // (ratio 1) lands exactly halfway up the field.
        let y_t = 1.0 - (row as f32 + 0.5) / FIELD_ROWS as f32;
        let ratio = SLEW_RATIO_MIN * (SLEW_RATIO_MAX / SLEW_RATIO_MIN).powf(y_t);
        for col in 0..FIELD_COLS {
            let x_t = (col as f32 + 0.5) / FIELD_COLS as f32;
            let strength = response_strength(state, hz_at(x_t), ratio);
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    field.left() + col as f32 * cell_w,
                    field.top() + row as f32 * cell_h,
                ),
                egui::vec2(cell_w, cell_h),
            )
            .shrink(cell_inset);
            let ink = if strength <= f32::EPSILON {
                theme.surface
            } else {
                // Square root gives low-but-real activity enough light to
                // read; the data still owns the intensity and is never fit
                // to the strongest cell in the current frame.
                theme.role_mod.gamma_multiply(0.18 + strength.sqrt() * 0.82)
            };
            painter.rect_filled(cell, 0.0, ink);
        }
    }

    // The fixed half-lift knee: `env == knee`, therefore lift == 0.5.
    let knee_y = field.center().y;
    painter.line_segment(
        [
            egui::pos2(field.left(), knee_y),
            egui::pos2(field.right(), knee_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.role_level),
    );

    // The moving edge-band corner. It changes the field too, but the line
    // makes the knob's precise authority readable before the colour settles.
    let edge_hz = sheen_value(EDGE, state.edge);
    let edge_x = field.left() + field.width() * hz_to_x(edge_hz);
    painter.line_segment(
        [
            egui::pos2(edge_x, field.top()),
            egui::pos2(edge_x, field.bottom()),
        ],
        egui::Stroke::new(stroke::BOLD, theme.role_shape),
    );

    let effective = sheen_value(AMOUNT, state.amount) * sheen_value(MIX, state.mix);
    let tag = if effective <= f32::EPSILON {
        // The colour path is exact-off. Do not say `wire`: output trim is
        // deliberately outside this field and may still change level.
        "edge off".to_owned()
    } else {
        format!("edge ×{effective:.2}")
    };
    let micro = egui::FontId::proportional(font::MICRO_LABEL);
    for (at, align, text, colour) in [
        (
            plot.left_top(),
            egui::Align2::LEFT_TOP,
            "SLEW × EDGE".to_owned(),
            theme.text_muted,
        ),
        (
            plot.right_top(),
            egui::Align2::RIGHT_TOP,
            tag,
            theme.text_muted,
        ),
        (
            egui::pos2(field.left(), knee_y),
            egui::Align2::LEFT_CENTER,
            " ½ LIFT ".to_owned(),
            theme.role_level,
        ),
        (
            field.left_top(),
            egui::Align2::LEFT_TOP,
            "FAST".to_owned(),
            theme.text_muted,
        ),
        (
            field.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            "SLOW".to_owned(),
            theme.text_muted,
        ),
        (
            plot.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            "LOW".to_owned(),
            theme.text_muted,
        ),
        (
            plot.right_bottom(),
            egui::Align2::RIGHT_BOTTOM,
            "HIGH".to_owned(),
            theme.text_muted,
        ),
        (
            egui::pos2(edge_x, plot.bottom()),
            egui::Align2::CENTER_BOTTOM,
            param_of(EDGE).format(state.edge),
            theme.role_shape,
        ),
    ] {
        painter.text(at, align, text, micro.clone(), colour);
    }
}

/// What ONE cell needs: room for the widest thing it will ever print,
/// measured through the font atlas rather than guessed.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The four cells of the strip, in the order they are drawn — which is
/// the order the signal meets them.
fn strip(s: &Spec) -> [(&Param, u32); 4] {
    [
        (&s.amount, AMOUNT),
        (&s.edge, EDGE),
        (&s.mix, MIX),
        (&s.out, OUT),
    ]
}

/// The width the value strip needs, summed from the same per-cell figure
/// the strip draws each cell at — a share is not a sum.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in. TWO,
/// because a labelled cell draws a value over a name — see
/// `notes/20260827-device-card-layout.md`, rule 1.
const VALUE_ROWS: usize = 2;

/// The card's only width contract: the well holding the screen has no
/// size of its own, because the screen is the hero.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the sheen card. Returns the edits the user just made.
pub fn sheen_card(ui: &mut egui::Ui, theme: &Theme, state: &mut SheenUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "sheen", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // A READOUT, not a draggable target. Rule 1 of the
                    // device-UI contract is about gestures with no owner,
                    // and a drag here could only mean "amount or edge,
                    // depending which way I moved" — exactly the shape
                    // that note exists to prevent.
                    poly_widgets::CurveRegion::Plot => response_field(ui, theme, state),
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
                                                value: sheen_value(id, *norm),
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

    /// The card covers the table exactly.
    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = sheen_edits(&SheenUi::default());
        assert_eq!(edits.len(), TABLE.len());
        for row in TABLE {
            assert!(
                edits.iter().any(|e| e.param == row.id),
                "`{}` never leaves the card",
                row.name
            );
        }
    }

    /// Every knob position maps to a value the engine will accept, and
    /// the ends of the travel reach the ends of the range.
    #[test]
    fn every_position_is_a_legal_engine_value() {
        for row in TABLE {
            for step in 0..=64 {
                let norm = step as f32 / 64.0;
                let value = sheen_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (sheen_value(row.id, 0.0) - row.min).abs() < 1e-3,
                "{}",
                row.name
            );
            assert!(
                (sheen_value(row.id, 1.0) - row.max).abs() < 1e-3,
                "{}",
                row.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = sheen_value(row.id, sheen_norm(row.id, value));
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
        let state = SheenUi::default();
        for (id, norm) in [
            (AMOUNT, state.amount),
            (EDGE, state.edge),
            (MIX, state.mix),
            (OUT, state.out),
        ] {
            let want = params::def(TABLE, id).default;
            let got = sheen_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    /// The device's one load-bearing promise: the bottom of `amount` is
    /// the kernel's EXACT wire, not merely a quiet setting.
    #[test]
    fn the_bottom_of_amount_is_a_bit_exact_wire() {
        let mut brighten = crate::dsp::dynamics::SlewBrighten::new();
        brighten.prepare(PLOT_SR, sheen_value(EDGE, 0.5), sheen_value(AMOUNT, 0.0));
        let mut buf = vec![0.0f32; 512];
        for (i, s) in buf.iter_mut().enumerate() {
            // Arbitrary finite programme, not the hero's old synthetic
            // display signal. Exact bypass must hold for any input.
            *s = (i % 31) as f32 / 15.0 - 1.0;
        }
        let before = buf.clone();
        brighten.process(&mut buf);
        assert_eq!(buf, before, "amount at its floor altered the block");
    }

    /// And the default is NOT that wire, or a freshly loaded sheen would
    /// do nothing at all.
    #[test]
    fn the_default_adds_something() {
        let strength = response_strength(&SheenUi::default(), 8_000.0, 4.0);
        assert!(
            strength > 0.01,
            "a fresh sheen maps to {strength}, which is nothing"
        );
    }

    /// The hero answers the controls that shape colour. Output trim is
    /// deliberately absent: it changes level after this relationship.
    #[test]
    fn the_response_field_answers_every_knob_that_shapes_it() {
        let base = SheenUi::default();
        let at_default = response_strength(&base, 4_000.0, 2.0);

        let mut louder = base;
        louder.amount = sheen_norm(AMOUNT, sheen_value(AMOUNT, base.amount) * 2.0);
        let at_louder = response_strength(&louder, 4_000.0, 2.0);
        assert!(
            at_louder > at_default,
            "doubling amount left the field at {at_louder} vs {at_default}"
        );

        let mut quieter = base;
        quieter.mix = sheen_norm(MIX, 0.25);
        let at_quieter = response_strength(&quieter, 4_000.0, 2.0);
        assert!(
            at_quieter < at_default,
            "quartering mix left the field at {at_quieter} vs {at_default}"
        );

        let mut higher = base;
        higher.edge = sheen_norm(EDGE, EDGE_MAX_HZ);
        let at_higher = response_strength(&higher, 4_000.0, 2.0);
        assert!(
            at_higher < at_default,
            "raising the edge corner did not darken 4 kHz: {at_higher} vs {at_default}"
        );
    }

    /// The horizontal knee line is not an illustration convention. It is
    /// the kernel's saturating ratio: equal envelope and knee means half
    /// lift, slow is below it, and fast approaches but never reaches one.
    #[test]
    fn the_slew_axis_is_the_kernels_lift_curve() {
        assert!((slew_lift(1.0) - 0.5).abs() < f32::EPSILON);
        assert!(slew_lift(SLEW_RATIO_MIN) < 0.1);
        assert!(slew_lift(SLEW_RATIO_MAX) > 0.9);
        assert!(slew_lift(1_000_000.0) < 1.0);
        assert_eq!(slew_lift(0.0), 0.0);
    }

    /// The frequency direction is the kernel's actual `x - lowpass(x)`:
    /// DC disappears, the authored corner turns the response, and the far
    /// top approaches unity rather than inventing a shelf gain.
    #[test]
    fn the_frequency_axis_is_the_kernels_edge_band() {
        let corner = 1_500.0;
        assert_eq!(edge_magnitude(0.0, corner), 0.0);
        let below = edge_magnitude(100.0, corner);
        let at = edge_magnitude(corner, corner);
        let above = edge_magnitude(18_000.0, corner);
        assert!(below < at && at < above, "{below} < {at} < {above}");
        // The exponential smoother is not a bilinear -3 dB one-pole: its
        // subtracted edge band is about 0.64 at the authored corner.
        assert!((at - 0.64).abs() < 0.01, "corner magnitude was {at}");
        assert!(above < 1.0, "the edge band invented gain: {above}");
    }

    /// Nothing here steps, and only the corner is logarithmic — the
    /// distinction the modulation code routes on.
    #[test]
    fn only_the_corner_is_log_and_nothing_is_discrete() {
        for id in [AMOUNT, EDGE, MIX, OUT] {
            assert!(!sheen_is_discrete(id), "id {id} should sweep");
        }
        assert!(sheen_is_log(EDGE));
        for id in [AMOUNT, MIX, OUT] {
            assert!(!sheen_is_log(id), "id {id} should not be log");
        }
    }
}
