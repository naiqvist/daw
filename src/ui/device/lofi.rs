//! The lo-fi device card — the converter's staircase as an instrument
//! screen.
//!
//! Same shape as the saturator card: normalized knob state here, natural
//! values out as [`ParamEdit`]s. Ids, ranges and defaults come from
//! [`crate::params::lofi`] — the one table this widget, `Node::Lofi`'s
//! apply arm and the app's edit routing all read.
//!
//! # The anatomy, from `notes/20260826-instrument-screen-design-guide.md`
//!
//! ```text
//! ┌ lo-fi ──────────────────────────────────┐
//! │ 4096 levels                             │
//! │           the staircase (hero)          │
//! │                                         │
//! │  rate     bits      mix       out       │
//! └─────────────────────────────────────────┘
//! ```
//!
//! The guide's rules this card is built to:
//!
//! - **A screen answers three questions.** Where am I: the card title.
//!   What is happening: the staircase, and the level count in the corner.
//!   What can I change: four controls, in fixed positions, in natural
//!   units.
//! - **Left to right is the signal.** Rate, then word length, then the
//!   blend, then what leaves — the order the sound actually travels, and
//!   the order the kernel runs its stages in.
//! - **One hero.** The staircase is the only bold stroke on the card.
//! - **Colour is an index.** Nothing here picks a colour. The wet trace
//!   wears `role_mod` because a converter is the destructive edge, and
//!   every cell below takes its family from its own `Unit` through
//!   `poly_widgets::role_color`.
//!
//! # The picture is the kernel, not a drawing of it
//!
//! [`trace`] runs a real [`Downsampler`](crate::dsp::lofi::Downsampler)
//! over a real sine and plots what comes back. It is the same code the
//! audio thread runs — the same tracking pre-filter, the same hold, the
//! same lattice — so the card cannot drift from the device the way a
//! hand-drawn approximation of a staircase would.
//!
//! That is also why the picture goes quiet as `rate` falls: the tracking
//! filter really does close in front of the converter, and a plot that
//! kept the sine at full height while the audio lost its top end would be
//! lying about the one thing this device is for.
//!
//! # Every value is in a unit you could say out loud
//!
//! The engine stores hertz, a bit count, a fraction and a linear gain.
//! The widget shows `22.05 kHz`, `12`, `100 %` and `-6.0 dB`. The mapping
//! between the two lives in [`lofi_value`] and its inverse, in this file,
//! once.

use crate::params::lofi::{BITS, MIX, OUT, OUT_MAX_DB, OUT_MIN_DB, RATE, RATE_MAX, TABLE};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;
use std::f32::consts::TAU;

/// Word lengths, as the strip prints them.
///
/// A `Choice` rather than a number with a `Plain` unit, because bits are
/// integral and `Plain` would print `12.00`. The list is indexed from
/// [`BITS_MIN`](crate::dsp::lofi::BITS_MIN), which is what [`shown`] and
/// [`natural`] convert between — the engine still receives 12.0, not 10.
const BITS_NAMES: &[&str] = &[
    "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];

/// Knob positions of one lo-fi, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LofiUi {
    pub rate: f32,
    pub bits: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for LofiUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the knobs use. A default written here as a knob
        // position would be a second opinion about what a fresh lo-fi is.
        Self {
            rate: lofi_norm(RATE, params::def(TABLE, RATE).default),
            bits: lofi_norm(BITS, params::def(TABLE, BITS).default),
            mix: lofi_norm(MIX, params::def(TABLE, MIX).default),
            out: lofi_norm(OUT, params::def(TABLE, OUT).default),
        }
    }
}

impl LofiUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the parameter table cannot route
    /// an edit into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            RATE => &mut self.rate,
            BITS => &mut self.bits,
            MIX => &mut self.mix,
            OUT => &mut self.out,
            _ => return None,
        })
    }
}

struct Spec {
    rate: Param,
    bits: Param,
    mix: Param,
    out: Param,
}

/// The four controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        // LOG-mapped, by `Param::hz`: a converter clock is a frequency,
        // and equal travel per octave is the only mapping where the
        // bottom of the dial is not all the same amount of "very slow".
        rate: default(
            Param::hz("rate", crate::dsp::lofi::RATE_MIN, RATE_MAX),
            RATE,
        ),
        bits: default(Param::choice("bits", BITS_NAMES), BITS),
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
        // The choice INDEX, not the bit count: `BITS_NAMES[0]` is two
        // bits, so the engine's 12 is the strip's index 10.
        BITS => value - crate::dsp::lofi::BITS_MIN,
        MIX => value * 100.0,
        OUT => db_of(value),
        // Rate is already the kernel's own hertz.
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
        BITS => value + crate::dsp::lofi::BITS_MIN,
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
        RATE => s.rate,
        BITS => s.bits,
        MIX => s.mix,
        _ => s.out,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn lofi_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`lofi_value`], for a state stored in engine units.
pub fn lofi_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings rather than sweeping.
/// Only the word length does — asked of the `Param` itself rather than
/// answered from a list here, so it cannot fall behind the spec.
pub fn lofi_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The rate does, which is what
/// makes a modulation sweep of it move in octaves the way the knob does.
pub fn lofi_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved — for a reset, a
/// preset recall, or the moment the device is first loaded.
pub fn lofi_edits(state: &LofiUi) -> Vec<ParamEdit> {
    [
        (RATE, state.rate),
        (BITS, state.bits),
        (MIX, state.mix),
        (OUT, state.out),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: lofi_value(param, norm),
    })
    .collect()
}

/// The reference rate the picture is drawn at.
///
/// A fixed figure and not the engine's, because the card is drawn in the
/// UI and has no engine to ask. It only sets the plot's horizontal scale:
/// what the staircase SHOWS — how many samples fit inside one hold, how
/// tall a quantisation step is — is the ratio between this and the two
/// knobs, and that ratio is the device.
const PLOT_SR: f32 = 48_000.0;

/// The test tone's pitch. Low enough that two cycles fit the plot with
/// the hold still visible inside each one.
const PLOT_HZ: f32 = 500.0;

/// Samples drawn: ONE cycle at [`PLOT_HZ`].
///
/// One and not two. The tread width on screen is the plot's width divided
/// by the samples in it, so every extra cycle halves the very thing the
/// picture is of. A single cycle is still unmistakably a sine, and its
/// steps are twice as wide as two cycles' would be.
const PLOT_N: usize = (PLOT_SR / PLOT_HZ) as usize;

/// Amplitude of the test tone. Short of the rails, so the quantiser's
/// clamp is not what the picture is about.
const PLOT_AMP: f32 = 0.85;

/// Below this many bits the lattice is drawn behind the trace. Above it
/// the levels are closer together than the pixels are, and a lattice
/// becomes a grey wash that hides the thing it is annotating.
const LATTICE_MAX_BITS: f32 = 6.0;

/// The dry sine and what the converter makes of it, both in `-1..=1`.
///
/// Runs the REAL kernel — see this module's header. The output trim is
/// deliberately not applied: the plot's vertical axis is the converter's
/// own rails, which is what the quantiser's lattice is measured against,
/// and a trim is a level rather than a shape.
fn trace(state: &LofiUi) -> ([f32; PLOT_N], [f32; PLOT_N], bool) {
    let mut dry = [0.0f32; PLOT_N];
    for (i, s) in dry.iter_mut().enumerate() {
        *s = (i as f32 / PLOT_SR * PLOT_HZ * TAU).sin() * PLOT_AMP;
    }
    let mut wet = dry;
    let mut ds = crate::dsp::lofi::Downsampler::new();
    ds.prepare(PLOT_SR);
    ds.set_rate(lofi_value(RATE, state.rate));
    ds.set_bits(lofi_value(BITS, state.bits));
    let clean = ds.is_bypassed();
    ds.process(&mut wet);
    // The blend, so the picture answers "how much of this am I hearing"
    // and not only "what would it sound like fully wet".
    let mix = lofi_value(MIX, state.mix);
    for (w, d) in wet.iter_mut().zip(dry.iter()) {
        *w = *d + (*w - *d) * mix;
    }
    (dry, wet, clean)
}

/// The hero: a sine, and the staircase the converter turns it into.
///
/// Drawn as explicit horizontal-then-vertical segments rather than a
/// polyline through the samples. A polyline would slope between two held
/// samples and draw a gentler wave than the device produces — the corners
/// ARE the effect, and rounding them off is the one way this picture
/// could flatter the thing it documents.
fn staircase(ui: &mut egui::Ui, theme: &Theme, state: &LofiUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);
    let mid = plot.center().y;
    let half = plot.height() * 0.5;
    let x_of = |i: usize| plot.left() + plot.width() * i as f32 / (PLOT_N - 1) as f32;
    let y_of = |v: f32| mid - v.clamp(-1.0, 1.0) * half;

    let (dry, wet, clean) = trace(state);

    // The lattice, when it is coarse enough to read: the levels the
    // quantiser is allowed to land on, which is what "4 levels" means as
    // a picture rather than as a number.
    let bits = lofi_value(BITS, state.bits);
    if bits <= LATTICE_MAX_BITS {
        let steps = 2.0f32.powf(bits);
        let quantum = 2.0 / steps;
        let mut level = -1.0;
        while level <= 1.0 + f32::EPSILON {
            painter.line_segment(
                [
                    egui::pos2(plot.left(), y_of(level)),
                    egui::pos2(plot.right(), y_of(level)),
                ],
                egui::Stroke::new(stroke::HAIR, theme.role_mod_dim),
            );
            level += quantum;
        }
    }

    // Zero, so the wave has something to sit on.
    painter.line_segment(
        [egui::pos2(plot.left(), mid), egui::pos2(plot.right(), mid)],
        egui::Stroke::new(stroke::HAIR, theme.surface_sunken),
    );

    // The dry sine: thin and quiet, because it is the reference and not
    // the subject.
    painter.add(egui::Shape::line(
        (0..PLOT_N)
            .map(|i| egui::pos2(x_of(i), y_of(dry[i])))
            .collect(),
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    ));

    // The body: one hairline per sample, from the centre out to the held
    // value. A closed path would be the obvious way to fill under a wave,
    // and the wrong one — egui tessellates non-convex fills unreliably,
    // and a bipolar staircase is about as non-convex as a shape gets.
    // Verticals are exact, cheap at this sample count, and they draw the
    // hold's own tread as a solid block rather than a shaded guess.
    for (i, &v) in wet.iter().enumerate() {
        painter.line_segment(
            [egui::pos2(x_of(i), mid), egui::pos2(x_of(i), y_of(v))],
            egui::Stroke::new(stroke::HAIR, theme.role_mod_dim),
        );
    }

    // The staircase.
    let mut steps: Vec<egui::Pos2> = Vec::with_capacity(PLOT_N * 2);
    for (i, &v) in wet.iter().enumerate() {
        let y = y_of(v);
        steps.push(egui::pos2(x_of(i), y));
        if i + 1 < PLOT_N {
            steps.push(egui::pos2(x_of(i + 1), y));
        }
    }
    painter.add(egui::Shape::line(
        steps,
        egui::Stroke::new(stroke::BOLD, theme.role_mod),
    ));

    // The corner tag. `clean` is the kernel's own answer, not a guess
    // from the knob positions: `Downsampler::is_bypassed` is what decides
    // whether the audio path is a wire, and saying it here is how the
    // device's one load-bearing promise — that its colour can be removed
    // exactly — is visible rather than merely documented.
    let tag = if clean {
        "clean".to_owned()
    } else {
        format!("{} levels", 2.0f32.powf(bits) as u32)
    };
    painter.text(
        egui::pos2(plot.left(), plot.top()),
        egui::Align2::LEFT_TOP,
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
        (&s.rate, RATE),
        (&s.bits, BITS),
        (&s.mix, MIX),
        (&s.out, OUT),
    ]
}

/// The width the value strip needs, summed from the same per-cell figure
/// the strip draws each cell at — a share is not a sum, and `sat.rs`
/// learned that the hard way.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in.
///
/// TWO, because a [`poly_widgets::labeled_cell`] draws a value over a
/// name and one row cannot hold both without them touching. See
/// `notes/20260827-device-card-layout.md`, rule 1 — this reads like an
/// off-by-one until you notice a cell is two lines.
const VALUE_ROWS: usize = 2;

/// The card's only width contract: the well holding the screen has no
/// size of its own, because the screen is the hero and therefore the
/// thing that grows.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the lo-fi card. Returns the edits the user just made.
pub fn lofi_card(ui: &mut egui::Ui, theme: &Theme, state: &mut LofiUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "lo-fi", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // The hero. It is a READOUT and not a control: the
                    // saturator's curve can be dragged because drive and
                    // bias are two axes of one shape, and a staircase has
                    // no equivalent gesture — dragging it could only mean
                    // "one of rate or bits, depending on which way I
                    // moved", which rule 1 of the device-UI contract is
                    // precisely about not building.
                    poly_widgets::CurveRegion::Plot => staircase(ui, theme, state),
                    // Each cell at its OWN width, not an equal share, so
                    // `22.05 kHz` cannot print through `100 %`; and each
                    // takes its family colour from its own `Unit`, so the
                    // trim reads as level without this file choosing a
                    // colour.
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
                                                value: lofi_value(id, *norm),
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

    /// The card covers the table exactly — every row leaves as an edit,
    /// and no edit names a row that is not there.
    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = lofi_edits(&LofiUi::default());
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
                let value = lofi_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (lofi_value(row.id, 0.0) - row.min).abs() < 1e-3,
                "{}",
                row.name
            );
            assert!(
                (lofi_value(row.id, 1.0) - row.max).abs() < 1e-3,
                "{}",
                row.name
            );
        }
    }

    /// Stated over VALUES, since the word length quantizes.
    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = lofi_value(row.id, lofi_norm(row.id, value));
                let tol = if row.id == BITS {
                    0.51
                } else {
                    1e-2 * row.max.abs().max(1.0)
                };
                assert!(
                    (back - value).abs() <= tol,
                    "`{}`: {value} came back as {back}",
                    row.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let state = LofiUi::default();
        for (id, norm) in [
            (RATE, state.rate),
            (BITS, state.bits),
            (MIX, state.mix),
            (OUT, state.out),
        ] {
            let want = params::def(TABLE, id).default;
            let got = lofi_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    /// The two off switches, which are this device's one load-bearing
    /// promise: the top of each knob must reach the kernel's exact bypass
    /// rather than merely approach it. A colour that cannot be removed is
    /// a colour that cannot be measured.
    #[test]
    fn the_top_of_both_knobs_is_the_kernels_exact_bypass() {
        let mut ds = crate::dsp::lofi::Downsampler::new();
        ds.prepare(PLOT_SR);
        ds.set_rate(lofi_value(RATE, 1.0));
        ds.set_bits(lofi_value(BITS, 1.0));
        assert!(
            ds.is_bypassed(),
            "rate {} and bits {} did not switch the converter off",
            lofi_value(RATE, 1.0),
            lofi_value(BITS, 1.0)
        );

        // And it really is a wire: a block through it comes back bit for
        // bit, which is the claim `dsp::lofi`'s header makes.
        let mut buf = [0.0f32; 64];
        for (i, s) in buf.iter_mut().enumerate() {
            *s = (i as f32 * 0.1).sin() * 0.7;
        }
        let before = buf;
        ds.process(&mut buf);
        assert_eq!(buf, before, "the bypass altered the block");
    }

    /// The default is audibly the device: neither knob may sit on its own
    /// off switch, or a freshly loaded lo-fi would do nothing at all.
    #[test]
    fn the_default_is_not_the_bypass() {
        let state = LofiUi::default();
        let mut ds = crate::dsp::lofi::Downsampler::new();
        ds.prepare(PLOT_SR);
        ds.set_rate(lofi_value(RATE, state.rate));
        ds.set_bits(lofi_value(BITS, state.bits));
        assert!(!ds.is_bypassed(), "a fresh lo-fi is a wire");
    }

    /// The word length is a choice and the rate is a sweep, and the
    /// modulation code routes on exactly that distinction.
    #[test]
    fn only_the_word_length_is_discrete_and_only_the_rate_is_log() {
        assert!(lofi_is_discrete(BITS));
        for id in [RATE, MIX, OUT] {
            assert!(!lofi_is_discrete(id), "id {id} should sweep");
        }
        assert!(lofi_is_log(RATE));
        for id in [BITS, MIX, OUT] {
            assert!(!lofi_is_log(id), "id {id} should not be log");
        }
    }

    /// Every step of the word-length strip names the bit count it sets,
    /// so a cell reading `12` cannot be setting ten.
    #[test]
    fn the_bit_names_are_the_bit_counts() {
        for (index, name) in BITS_NAMES.iter().enumerate() {
            let norm = index as f32 / (BITS_NAMES.len() - 1) as f32;
            let bits = lofi_value(BITS, norm);
            assert_eq!(
                name.parse::<f32>().expect("a bit name is a number"),
                bits,
                "step {index} prints `{name}` and sets {bits}"
            );
        }
    }
}
