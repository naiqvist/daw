//! The sheen device card — what the brightener ADDS, as an instrument
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
//! │ +38 % edge                              │
//! │ in     ╷▁▁▁▁▁▁      ╷▁▁▁▁▁▁             │
//! │ added   ▚▂▁          ▚▂▁                │  ← hero
//! │                                         │
//! │  amount   edge      mix       out       │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Why the hero is the DIFFERENCE
//!
//! The obvious picture — input against output — is the wrong one here,
//! and the kernel's own shape says why. A slew brightener adds a little
//! high-frequency energy on the edges and nothing anywhere else, so an
//! overlay of the two waveforms is two lines on top of each other with a
//! disagreement too small to see. Plotting `wet - dry` puts the whole
//! plot on the only thing the device does.
//!
//! It is also the only framing in which every knob moves the picture. The
//! kernel's `lift()` is a function of the signal's slew and its own fixed
//! knee, so a lift curve would sit there unchanged while the user turned
//! `amount` — a display that ignores its own controls. The difference
//! scales with `amount`, changes texture with `edge`, and scales again
//! with `mix`.
//!
//! # The picture is the kernel, not a drawing of it
//!
//! [`trace`] runs a real
//! [`SlewBrighten`](crate::dsp::dynamics::SlewBrighten) over a synthetic
//! pair of drum hits and plots what comes back — the same move the lo-fi
//! card makes with its converter. The two hits are there because the
//! device's whole claim is that the lift ARRIVES with a transient and
//! LEAVES with it, and one hit cannot show "and leaves".
//!
//! The output trim is deliberately not in the picture: it is a level, and
//! the plot is about a shape.
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
use std::f32::consts::TAU;

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

/// The reference rate the picture is drawn at.
///
/// A fixed figure and not the engine's, because the card is drawn in the
/// UI and has no engine to ask. It has to be a real audio rate rather
/// than a convenient small one: the kernel's knee is 3 kHz, and at any
/// rate low enough to plot sample-by-sample that knee would be above
/// Nyquist and the picture would be of a device that cannot exist.
const PLOT_SR: f32 = 48_000.0;

/// How long a window the picture covers, in seconds. Long enough to hold
/// two hits and the gap between them, which is what shows the lift
/// leaving as well as arriving.
const PLOT_SECS: f32 = 0.2;

/// Samples the kernel actually runs over.
const PLOT_N: usize = (PLOT_SR * PLOT_SECS) as usize;

/// Columns drawn. Far fewer than [`PLOT_N`], so each column is a
/// min/max over its bucket — the way any waveform overview is drawn, and
/// the only honest way to put 9,600 samples on 300 pixels.
const PLOT_COLS: usize = 300;

/// Where the two hits start, in seconds.
const HITS: [f32; 2] = [0.015, 0.105];

/// The test hit: a low body and a high edge, each with its own decay.
///
/// Deliberately a pair of decaying sines rather than filtered noise. Noise
/// would be a more realistic drum and a worse PICTURE — at three hundred
/// columns it reads as a grey block, and the point of the plot is the
/// SHAPE of what the device adds.
fn hit(t: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let body = (TAU * 90.0 * t).sin() * (-t * 25.0).exp();
    let edge = (TAU * 3_200.0 * t).sin() * (-t * 120.0).exp();
    0.5 * body + 0.5 * edge
}

/// One column of the picture: the input's extent, and the added signal's.
#[derive(Clone, Copy, Default)]
struct Column {
    dry_lo: f32,
    dry_hi: f32,
    add_lo: f32,
    add_hi: f32,
}

/// Run the real kernel and reduce it to columns.
///
/// Returns the columns and the peak of the added signal as a fraction of
/// the input's peak — the corner tag's number.
fn trace(state: &SheenUi) -> (Vec<Column>, f32) {
    let mut dry = vec![0.0f32; PLOT_N];
    for (i, s) in dry.iter_mut().enumerate() {
        let t = i as f32 / PLOT_SR;
        *s = HITS.iter().map(|start| hit(t - start)).sum();
    }

    let amount = sheen_value(AMOUNT, state.amount);
    let mut wet = dry.clone();
    let mut brighten = crate::dsp::dynamics::SlewBrighten::new();
    brighten.prepare(PLOT_SR, sheen_value(EDGE, state.edge), amount);
    brighten.process(&mut wet);

    // The blend, so the picture answers "how much of this am I hearing"
    // and not only "what would it do fully wet".
    let mix = sheen_value(MIX, state.mix);

    let mut columns = vec![Column::default(); PLOT_COLS];
    let mut peak_dry = 0.0f32;
    let mut peak_add = 0.0f32;
    for (c, column) in columns.iter_mut().enumerate() {
        let from = c * PLOT_N / PLOT_COLS;
        let to = ((c + 1) * PLOT_N / PLOT_COLS).min(PLOT_N).max(from + 1);
        for i in from..to {
            let d = dry[i];
            let a = (wet[i] - d) * mix;
            column.dry_lo = column.dry_lo.min(d);
            column.dry_hi = column.dry_hi.max(d);
            column.add_lo = column.add_lo.min(a);
            column.add_hi = column.add_hi.max(a);
            peak_dry = peak_dry.max(d.abs());
            peak_add = peak_add.max(a.abs());
        }
    }
    let ratio = if peak_dry > 0.0 {
        peak_add / peak_dry
    } else {
        0.0
    };
    (columns, ratio)
}

/// How much of the plot's height the input strip takes.
///
/// Half. The added signal is the hero, but it is also the SMALLER of the
/// two by a factor of several, so giving it the larger lane bought
/// nothing but empty air above and below it — the height it needed was
/// never the height it had.
const IN_LANE_FRAC: f32 = 0.5;

/// How much the added lane is magnified.
///
/// The two lanes cannot share a scale: what this device adds is a few
/// tens of a percent of what goes in, so at the input's scale the hero
/// would be a thick line. Three, and the lane says so, because a plot
/// that quietly rescales itself is a plot that cannot be compared with
/// the one beside it.
///
/// A FIXED figure rather than an auto-fit, which is the important part:
/// auto-fitting would make the hero fill its lane at every setting, and a
/// display that looks identical whatever the knob does is not a display.
const ADD_GAIN: f32 = 3.0;

/// Draw one lane of min/max columns around its own centre line.
fn lane(
    painter: &egui::Painter,
    rect: egui::Rect,
    columns: &[Column],
    pick: impl Fn(&Column) -> (f32, f32),
    colour: egui::Color32,
    width: f32,
) {
    let mid = rect.center().y;
    let half = rect.height() * 0.5;
    for (c, column) in columns.iter().enumerate() {
        let x = rect.left() + rect.width() * c as f32 / (columns.len() - 1).max(1) as f32;
        let (lo, hi) = pick(column);
        let y0 = mid - hi.clamp(-1.0, 1.0) * half;
        let y1 = mid - lo.clamp(-1.0, 1.0) * half;
        // A column that reduces to nothing still gets a mark, so silence
        // reads as a line rather than as a gap in the drawing.
        painter.line_segment(
            [egui::pos2(x, y0), egui::pos2(x, y1.max(y0 + 0.5))],
            egui::Stroke::new(width, colour),
        );
    }
}

/// The hero: two hits, and what the sheen adds to them.
fn lanes(ui: &mut egui::Ui, theme: &Theme, state: &SheenUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);
    let split = plot.top() + plot.height() * IN_LANE_FRAC;
    let in_lane = egui::Rect::from_min_max(plot.min, egui::pos2(plot.right(), split));
    let add_lane = egui::Rect::from_min_max(egui::pos2(plot.left(), split), plot.max);

    let (columns, ratio) = trace(state);

    // The seam between the lanes, so the hero has a floor to stand on.
    painter.line_segment(
        [
            egui::pos2(plot.left(), split),
            egui::pos2(plot.right(), split),
        ],
        egui::Stroke::new(stroke::HAIR, theme.surface_sunken),
    );

    // What went in: quiet, because it is the reference and not the
    // subject.
    lane(
        &painter,
        in_lane,
        &columns,
        |c| (c.dry_lo, c.dry_hi),
        theme.text_muted,
        stroke::HAIR,
    );

    // What the sheen adds. The only bold stroke on the card.
    lane(
        &painter,
        add_lane,
        &columns,
        |c| (c.add_lo * ADD_GAIN, c.add_hi * ADD_GAIN),
        theme.role_mod,
        stroke::HAIR,
    );

    // Which lane is which. Two words, in the smallest type the kit has,
    // because the drawing carries the meaning and the words only confirm
    // it — the corner-tag convention `font::MICRO_LABEL` exists for.
    for (at, text) in [
        (in_lane.left_bottom(), "in".to_owned()),
        (add_lane.left_bottom(), format!("added ×{ADD_GAIN:.0}")),
    ] {
        painter.text(
            at,
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(font::MICRO_LABEL),
            theme.text_muted,
        );
    }

    // The corner tag. `wire` is the kernel's own promise made visible:
    // `SlewBrighten` guarantees amount 0 is BIT-EXACT rather than merely
    // quiet, which is what lets a device leave the stage permanently in
    // its path — and a colour that cannot be removed is a colour that
    // cannot be measured.
    let tag = if ratio <= 0.0 {
        "wire".to_owned()
    } else {
        format!("+{:.0} % edge", ratio * 100.0)
    };
    // Top RIGHT, not left: the input lane starts at the very top of the
    // plot and its first hit lands at the very left, so a left-anchored
    // tag prints straight through the loudest part of the picture.
    painter.text(
        egui::pos2(plot.right(), plot.top()),
        egui::Align2::RIGHT_TOP,
        tag,
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.text_muted,
    );
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
                    poly_widgets::CurveRegion::Plot => lanes(ui, theme, state),
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
            let t = i as f32 / PLOT_SR;
            *s = HITS.iter().map(|start| hit(t - start)).sum();
        }
        let before = buf.clone();
        brighten.process(&mut buf);
        assert_eq!(buf, before, "amount at its floor altered the block");
    }

    /// And the default is NOT that wire, or a freshly loaded sheen would
    /// do nothing at all.
    #[test]
    fn the_default_adds_something() {
        let (_, ratio) = trace(&SheenUi::default());
        assert!(ratio > 0.01, "a fresh sheen adds {ratio}, which is nothing");
    }

    /// The hero has to answer its own controls — the reason it plots the
    /// difference rather than the kernel's `lift()`, which is a function
    /// of the signal alone and would sit still while `amount` moved.
    #[test]
    fn the_picture_answers_every_knob_that_shapes_it() {
        let base = SheenUi::default();
        let (_, at_default) = trace(&base);

        let mut louder = base;
        louder.amount = sheen_norm(AMOUNT, sheen_value(AMOUNT, base.amount) * 2.0);
        let (_, at_louder) = trace(&louder);
        assert!(
            at_louder > at_default,
            "doubling amount left the picture at {at_louder} vs {at_default}"
        );

        let mut quieter = base;
        quieter.mix = sheen_norm(MIX, 0.25);
        let (_, at_quieter) = trace(&quieter);
        assert!(
            at_quieter < at_default,
            "quartering mix left the picture at {at_quieter} vs {at_default}"
        );

        let mut higher = base;
        higher.edge = sheen_norm(EDGE, EDGE_MAX_HZ);
        let (_, at_higher) = trace(&higher);
        assert!(
            (at_higher - at_default).abs() > 1e-4,
            "moving the corner left the picture unchanged at {at_higher}"
        );
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
