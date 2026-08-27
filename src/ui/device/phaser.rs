//! The phaser device card — the notches, and how far they travel.
//!
//! Same shape as the other small cards: normalized knob state here,
//! natural values out as [`ParamEdit`]s. Ids, ranges and defaults come
//! from [`crate::params::phaser`] — the one table this widget,
//! `Node::Phaser`'s apply arm and the app's edit routing all read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ phaser ─────────────────────────────────┐
//! │                              2 notches  │
//! │   ╲╱╲╱  ‹ghosts at the sweep's ends›     │  ← hero
//! │   100      1k        10k                │
//! │ amount centre depth  rate   mix         │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Why three curves and not one
//!
//! A phaser is a moving thing, and a card is not. One curve would be a
//! photograph of a sweep at whatever instant the drawing happened to
//! catch, and it would sit still while the `depth` and `rate` knobs — two
//! of the five — did nothing visible.
//!
//! So the card draws the comb at BOTH ends of the sweep as ghosts and at
//! its centre in full. The bold curve is the shape; the gap between the
//! ghosts is the travel. `depth` opens that gap, `centre` slides all
//! three, `amount` adds notches and `mix` deepens them.
//!
//! `rate` is the one knob with nothing to show, because a still picture
//! has no time axis. It says its value in its own cell and that is the
//! honest amount of display a speed can get here.
//!
//! # Measured, not derived
//!
//! [`comb`] sends a real impulse through a real
//! [`Disperser`](crate::dsp::filters::Disperser), sums it with the dry
//! exactly as `Node::Phaser` does, and takes the magnitude with
//! [`RealFft`](crate::dsp::fft::RealFft) — the same method the tilt card
//! uses, and for the same reason: the notches are an interference
//! pattern, and the honest way to find out where they land is to look.
//!
//! # The mix knob runs backwards, and the picture shows it
//!
//! A notch is the dry and the wet cancelling, so it is deepest at half
//! and gone at the top — an allpass on its own is flat. Turn `mix` up
//! past the middle here and the notches get SHALLOWER. That is the
//! opposite of every other mix in the rack, it is inherent to what a
//! phaser is, and the plot says so plainly the moment the knob moves.

use crate::params::phaser::{
    AMOUNT, CENTRE, CENTRE_MAX_HZ, CENTRE_MIN_HZ, DEPTH, DEPTH_MAX_OCT, MIX, RATE, RATE_MAX_HZ,
    RATE_MIN_HZ, TABLE,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Section counts, as the strip prints them. A `Choice` for the reason
/// the disperser's is: sections are integral, and a `Plain` unit would
/// print `4.00`.
const AMOUNT_NAMES: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];

/// Knob positions of one phaser, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PhaserUi {
    pub amount: f32,
    pub centre: f32,
    pub depth: f32,
    pub rate: f32,
    pub mix: f32,
}

impl Default for PhaserUi {
    fn default() -> Self {
        Self {
            amount: phaser_norm(AMOUNT, params::def(TABLE, AMOUNT).default),
            centre: phaser_norm(CENTRE, params::def(TABLE, CENTRE).default),
            depth: phaser_norm(DEPTH, params::def(TABLE, DEPTH).default),
            rate: phaser_norm(RATE, params::def(TABLE, RATE).default),
            mix: phaser_norm(MIX, params::def(TABLE, MIX).default),
        }
    }
}

impl PhaserUi {
    /// This state's knob position for a wire id, mutably.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            AMOUNT => &mut self.amount,
            CENTRE => &mut self.centre,
            DEPTH => &mut self.depth,
            RATE => &mut self.rate,
            MIX => &mut self.mix,
            _ => return None,
        })
    }
}

struct Spec {
    amount: Param,
    centre: Param,
    depth: Param,
    rate: Param,
    mix: Param,
}

/// The five controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        amount: default(Param::choice("amount", AMOUNT_NAMES), AMOUNT),
        centre: default(Param::hz("centre", CENTRE_MIN_HZ, CENTRE_MAX_HZ), CENTRE),
        // OCTAVES, linear: the table's unit, and the one the ear uses.
        depth: default(
            Param::new(
                "depth",
                Mapping::Linear {
                    min: 0.0,
                    max: DEPTH_MAX_OCT,
                },
                Unit::Plain,
            ),
            DEPTH,
        ),
        // LOG: a sweep speed is a ratio, and the difference between 0.1
        // and 0.2 Hz is the same amount of "faster" as between 4 and 8.
        rate: default(Param::hz("rate", RATE_MIN_HZ, RATE_MAX_HZ), RATE),
        mix: default(Param::percent("mix"), MIX),
    }
}

/// What the widget SHOWS for an engine-facing value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        MIX => value * 100.0,
        // A count, a frequency, an octave span and a speed are already
        // what a musician would say out loud.
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows, clamped through
/// the row so a knob at either stop cannot emit a letter to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        MIX => value / 100.0,
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        AMOUNT => s.amount,
        CENTRE => s.centre,
        DEPTH => s.depth,
        RATE => s.rate,
        _ => s.mix,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn phaser_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`phaser_value`], for a state stored in engine units.
pub fn phaser_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. The section count does.
pub fn phaser_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The centre and the rate do.
pub fn phaser_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn phaser_edits(state: &PhaserUi) -> Vec<ParamEdit> {
    [
        (AMOUNT, state.amount),
        (CENTRE, state.centre),
        (DEPTH, state.depth),
        (RATE, state.rate),
        (MIX, state.mix),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: phaser_value(param, norm),
    })
    .collect()
}

/// The reference rate the picture is measured at.
const PLOT_SR: f32 = 48_000.0;

/// Transform size. Larger than the tilt's, because a comb has structure a
/// see-saw does not: the notches need bins between them or the curve
/// draws straight through the gaps.
const FFT_N: usize = 4096;

/// The Q every section runs at. `audio::graph::PHASER_Q`'s figure —
/// restated rather than shared, because the card measures at its own
/// reference rate and the two are already separate constants.
const PHASER_Q: f32 = 0.7;

/// The band drawn.
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// The vertical window. Asymmetric on purpose: a comb's peaks are modest
/// and its notches are bottomless, so the room goes downward.
const DB_TOP: f32 = 9.0;
const DB_BOTTOM: f32 = -27.0;

/// Where the decade rules and their labels go.
const DECADES: [(f32, &str); 3] = [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")];

/// Points along each curve.
const CURVE_POINTS: usize = 240;

/// The measured magnitude of the phaser's output, at one position in the
/// sweep, as `(hz, dB)` pairs.
///
/// `sweep` is the LFO's value: `-1` and `+1` are the ends of the travel
/// and `0` is the centre. The corner is derived exactly as
/// `Node::Phaser`'s process arm derives it, and the dry and wet are
/// summed exactly as it sums them, so the curve is the device's.
///
/// Empty when the transform will not prepare — the fail-open the drawing
/// checks, so a card that cannot measure draws no curve rather than a
/// wrong one.
fn comb(state: &PhaserUi, sweep: f32) -> Vec<(f32, f32)> {
    let stages = phaser_value(AMOUNT, state.amount).round().max(0.0) as u32;
    let centre = phaser_value(CENTRE, state.centre);
    let depth = phaser_value(DEPTH, state.depth);
    let mix = phaser_value(MIX, state.mix);
    let corner = (centre * (depth * sweep).exp2()).clamp(20.0, 20_000.0);

    let mut io = vec![0.0f32; FFT_N];
    io[0] = 1.0;
    let mut chain = crate::dsp::filters::Disperser::new();
    chain.prepare(PLOT_SR, corner, PHASER_Q, stages);
    chain.process(&mut io);
    // The blend, exactly as the node does it: the dry is the impulse, so
    // it contributes only at sample zero.
    for (i, s) in io.iter_mut().enumerate() {
        let d = if i == 0 { 1.0 } else { 0.0 };
        *s = d + (*s - d) * mix;
    }

    let mut fft = crate::dsp::fft::RealFft::new();
    if !fft.prepare(FFT_N) {
        return Vec::new();
    }
    let bins = crate::dsp::fft::RealFft::bins(FFT_N);
    let mut real = vec![0.0f32; bins];
    let mut imag = vec![0.0f32; bins];
    let mut scratch = vec![0.0f32; crate::dsp::fft::RealFft::scratch_len(FFT_N)];
    fft.forward(&io, &mut real, &mut imag, &mut scratch);
    let mut magnitude = vec![0.0f32; bins];
    let mut phase = vec![0.0f32; bins];
    crate::dsp::fft::magnitude_phase(&real, &imag, &mut magnitude, &mut phase);

    let per_bin = PLOT_SR / FFT_N as f32;
    (0..CURVE_POINTS)
        .map(|i| {
            let t = i as f32 / (CURVE_POINTS - 1) as f32;
            let hz = F_MIN * (F_MAX / F_MIN).powf(t);
            let at = hz / per_bin;
            let lo = (at.floor() as usize).min(bins - 1);
            let hi = (lo + 1).min(bins - 1);
            let frac = at - at.floor();
            let m = magnitude[lo] + (magnitude[hi] - magnitude[lo]) * frac;
            let db = if m > 0.0 { 20.0 * m.log10() } else { DB_BOTTOM };
            (hz, db.clamp(DB_BOTTOM, DB_TOP))
        })
        .collect()
}

/// How deep a dip has to be before it counts as a notch, in dB.
const NOTCH_FLOOR: f32 = -3.0;

/// How many notches a measured curve has in the drawn band.
///
/// COUNTED, not computed from the section count, and the measurement is
/// why. Sections do not map onto audible notches by any tidy formula —
/// at 48 kHz, centred at 800 Hz, two octaves of depth:
///
/// ```text
///   sections:  1  2  3  4  6  8  12  16
///    notches:  0  0  1  3  5  8  12  16
/// ```
///
/// It approaches one-per-section, but only once enough of them land
/// inside 20 Hz..20 kHz and deep enough to hear. Both `stages / 2` and
/// `stages` were wrong at one end or the other, so the tag reports what
/// the curve actually has.
fn count_notches(points: &[(f32, f32)]) -> usize {
    points
        .windows(3)
        .filter(|w| w[1].1 < w[0].1 && w[1].1 < w[2].1 && w[1].1 < NOTCH_FLOOR)
        .count()
}

/// The hero: the comb at the middle of the sweep, and where its ends put
/// it.
fn notches(ui: &mut egui::Ui, theme: &Theme, state: &PhaserUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);
    let label_h = theme.sp(font::MICRO_LABEL) + theme.sp(space::XXS);
    let field =
        egui::Rect::from_min_max(plot.min, egui::pos2(plot.right(), plot.bottom() - label_h));
    let x_of = |hz: f32| {
        let t = (hz / F_MIN).max(1e-6).log10() / (F_MAX / F_MIN).log10();
        field.left() + field.width() * t.clamp(0.0, 1.0)
    };
    let y_of = |db: f32| {
        let t = (db - DB_TOP) / (DB_BOTTOM - DB_TOP);
        field.top() + field.height() * t.clamp(0.0, 1.0)
    };

    // The decades, with their names along the floor.
    for (hz, name) in DECADES {
        let x = x_of(hz);
        painter.line_segment(
            [egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
            egui::Stroke::new(stroke::HAIR, theme.grid_sub),
        );
        painter.text(
            egui::pos2(x, plot.bottom()),
            egui::Align2::CENTER_BOTTOM,
            name,
            egui::FontId::proportional(font::MICRO_LABEL),
            theme.text_muted,
        );
    }

    // Unity, so a notch has something to be a notch below.
    painter.line_segment(
        [
            egui::pos2(field.left(), y_of(0.0)),
            egui::pos2(field.right(), y_of(0.0)),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    let draw = |points: &[(f32, f32)], colour, width| {
        if points.is_empty() {
            return;
        }
        painter.add(egui::Shape::line(
            points
                .iter()
                .map(|(hz, db)| egui::pos2(x_of(*hz), y_of(*db)))
                .collect(),
            egui::Stroke::new(width, colour),
        ));
    };

    // The ends of the travel first, so the live shape draws over them.
    for end in [-1.0f32, 1.0] {
        draw(&comb(state, end), theme.role_mod_dim, stroke::HAIR);
    }
    let centre = comb(state, 0.0);
    draw(&centre, theme.role_mod, stroke::BOLD);

    // The corner tag, counted off the curve above rather than derived
    // from the section count — see `count_notches` for the measurement
    // that settled it.
    let stages = phaser_value(AMOUNT, state.amount).round() as u32;
    let found = count_notches(&centre);
    let tag = if stages == 0 {
        // With no sections the wet path IS the dry path, so there is
        // nothing for the blend to cancel against. Said out loud for the
        // reason the lo-fi says `clean`.
        "wire".to_owned()
    } else if found == 1 {
        "1 notch".to_owned()
    } else {
        format!("{found} notches")
    };
    painter.text(
        egui::pos2(field.right(), field.top()),
        egui::Align2::RIGHT_TOP,
        tag,
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.text_muted,
    );
}

/// What ONE cell needs: room for the widest thing it will ever print.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The five cells of the strip, in the order they are drawn.
fn strip(s: &Spec) -> [(&Param, u32); 5] {
    [
        (&s.amount, AMOUNT),
        (&s.centre, CENTRE),
        (&s.depth, DEPTH),
        (&s.rate, RATE),
        (&s.mix, MIX),
    ]
}

/// The width the strip needs, summed from the same per-cell figure the
/// strip draws each cell at — a share is not a sum.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in. TWO,
/// because a labelled cell draws a value over a name — see
/// `notes/20260827-device-card-layout.md`, rule 1.
const VALUE_ROWS: usize = 2;

/// The card's only width contract.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the phaser card. Returns the edits the user just made.
pub fn phaser_card(ui: &mut egui::Ui, theme: &Theme, state: &mut PhaserUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "phaser", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // A READOUT, not a draggable target — rule 1 of the
                    // device-UI contract. Five knobs shape this curve and
                    // a drag could not say which.
                    poly_widgets::CurveRegion::Plot => notches(ui, theme, state),
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
                                                value: phaser_value(id, *norm),
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
    use crate::params::phaser::AMOUNT_MAX;

    fn state(amount: f32, centre: f32, depth: f32, mix: f32) -> PhaserUi {
        PhaserUi {
            amount: phaser_norm(AMOUNT, amount),
            centre: phaser_norm(CENTRE, centre),
            depth: phaser_norm(DEPTH, depth),
            mix: phaser_norm(MIX, mix),
            ..PhaserUi::default()
        }
    }

    /// The deepest notch in a measured curve, in dB.
    fn deepest(points: &[(f32, f32)]) -> f32 {
        points.iter().fold(0.0f32, |a, (_, db)| a.min(*db))
    }

    /// Where the deepest notch sits, in Hz.
    fn notch_hz(points: &[(f32, f32)]) -> f32 {
        points
            .iter()
            .fold(
                (0.0f32, 0.0f32),
                |acc, (hz, db)| {
                    if *db < acc.1 { (*hz, *db) } else { acc }
                },
            )
            .0
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = phaser_edits(&PhaserUi::default());
        assert_eq!(edits.len(), TABLE.len());
        for row in TABLE {
            assert!(
                edits.iter().any(|e| e.param == row.id),
                "`{}` never leaves the card",
                row.name
            );
        }
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for row in TABLE {
            for step in 0..=64 {
                let norm = step as f32 / 64.0;
                let value = phaser_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (phaser_value(row.id, 0.0) - row.min).abs() < 1e-3,
                "{}",
                row.name
            );
            assert!(
                (phaser_value(row.id, 1.0) - row.max).abs() < 1e-3,
                "{}",
                row.name
            );
        }
    }

    /// Stated over VALUES, since the section count quantizes.
    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = phaser_value(row.id, phaser_norm(row.id, value));
                let tol = if row.id == AMOUNT {
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
        let fresh = PhaserUi::default();
        for (id, norm) in [
            (AMOUNT, fresh.amount),
            (CENTRE, fresh.centre),
            (DEPTH, fresh.depth),
            (RATE, fresh.rate),
            (MIX, fresh.mix),
        ] {
            let want = params::def(TABLE, id).default;
            let got = phaser_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    #[test]
    fn only_the_count_steps_and_only_centre_and_rate_are_log() {
        assert!(phaser_is_discrete(AMOUNT));
        for id in [CENTRE, DEPTH, RATE, MIX] {
            assert!(!phaser_is_discrete(id), "id {id} should sweep");
        }
        for id in [CENTRE, RATE] {
            assert!(phaser_is_log(id), "id {id} should be log");
        }
        for id in [AMOUNT, DEPTH, MIX] {
            assert!(!phaser_is_log(id), "id {id} should not be log");
        }
    }

    #[test]
    fn the_amount_names_are_the_section_counts() {
        assert_eq!(AMOUNT_NAMES.len() as f32, AMOUNT_MAX + 1.0);
        for (index, name) in AMOUNT_NAMES.iter().enumerate() {
            let norm = index as f32 / (AMOUNT_NAMES.len() - 1) as f32;
            let stages = phaser_value(AMOUNT, norm);
            assert_eq!(
                name.parse::<f32>().expect("a count is a number"),
                stages,
                "step {index} prints `{name}` and sets {stages}"
            );
        }
    }

    /// The device exists at all: a default phaser actually notches.
    #[test]
    fn a_default_phaser_has_notches() {
        let points = comb(&PhaserUi::default(), 0.0);
        assert!(!points.is_empty(), "the transform refused to prepare");
        assert!(
            deepest(&points) < -6.0,
            "the deepest notch was only {:.1} dB",
            deepest(&points)
        );
    }

    /// With no sections the wet path is the dry path, so there is nothing
    /// to cancel and the output is flat whatever the mix says.
    #[test]
    fn no_sections_is_a_wire() {
        for mix in [0.0f32, 0.5, 1.0] {
            let points = comb(&state(0.0, 800.0, 2.0, mix), 0.0);
            for (hz, db) in &points {
                assert!(
                    db.abs() < 0.1,
                    "no sections measured {db:.3} dB at {hz:.0} Hz with mix {mix}"
                );
            }
        }
    }

    /// THE ONE WORTH HAVING: the mix knob runs backwards, and this is
    /// where that claim is checked rather than merely written down.
    ///
    /// A notch is the dry and the wet cancelling, so it is deepest when
    /// the two are equal and GONE at fully wet, where an allpass on its
    /// own is flat. If this ever inverts, the module header and the
    /// params doc are both lying.
    #[test]
    fn the_notch_is_deepest_at_half_and_gone_at_the_top() {
        let half = deepest(&comb(&state(4.0, 800.0, 2.0, 0.5), 0.0));
        let most = deepest(&comb(&state(4.0, 800.0, 2.0, 1.0), 0.0));
        let none = deepest(&comb(&state(4.0, 800.0, 2.0, 0.0), 0.0));
        assert!(
            half < most - 6.0,
            "half notched {half:.1} dB and fully wet {most:.1} dB"
        );
        assert!(
            most.abs() < 0.1,
            "fully wet should be flat, got {most:.2} dB"
        );
        assert!(
            none.abs() < 0.1,
            "fully dry should be flat, got {none:.2} dB"
        );
    }

    /// The two ghosts are the sweep's ends, so depth has to move them
    /// apart — and at zero depth they must land on each other, because a
    /// sweep of nothing goes nowhere.
    ///
    /// Stated as a COMPARISON between depths rather than as an absolute
    /// ratio, and the first draft's failure is why. Two octaves of depth
    /// moves the corner by sixteen, but the deepest notch is not the
    /// corner — it sits where the chain's phase reaches half a turn — and
    /// with several notches in the band the deepest one can change
    /// identity partway through the sweep. Measured, two octaves moves
    /// the notch about threefold. Asserting a number derived from the
    /// corner would have been asserting a coincidence.
    #[test]
    fn depth_is_the_gap_between_the_ghosts() {
        let gap = |depth: f32| {
            let s = state(4.0, 800.0, depth, 0.5);
            let low = notch_hz(&comb(&s, -1.0)).max(1e-3);
            notch_hz(&comb(&s, 1.0)) / low
        };
        assert!(
            gap(2.0) > 2.0,
            "two octaves only spread the notch {:.2}x",
            gap(2.0)
        );
        assert!(
            gap(2.0) > gap(0.5),
            "two octaves spread {:.2}x and half an octave spread {:.2}x",
            gap(2.0),
            gap(0.5)
        );

        let still = state(4.0, 800.0, 0.0, 0.5);
        let a = notch_hz(&comb(&still, -1.0));
        let b = notch_hz(&comb(&still, 1.0));
        assert_eq!(a, b, "zero depth still swept");
    }

    /// More sections, more notches — the claim the corner tag makes, and
    /// the table in [`count_notches`] pinned at two of its points.
    #[test]
    fn more_sections_put_more_notches_in_the_band() {
        let count = |amount: f32| count_notches(&comb(&state(amount, 800.0, 2.0, 0.5), 0.0));
        assert!(
            count(8.0) > count(4.0) && count(4.0) > count(2.0),
            "8 sections found {}, 4 found {}, 2 found {}",
            count(8.0),
            count(4.0),
            count(2.0)
        );
    }
}
