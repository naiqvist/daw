//! The strip device card — the whole chain's tone, measured.
//!
//! Normalized knob state here, natural values out as [`ParamEdit`]s. Ids,
//! ranges and defaults come from [`crate::params::strip`] — the one table
//! this widget, `audio::strip::StripCore` and the app's edit routing all
//! read.
//!
//! # The hero is the EQ SECTION, measured — and not the whole device
//!
//! [`curve`] builds the four shelves from `dsp::filters::EqBand`, sends
//! an impulse through them and takes the magnitude with
//! [`RealFft`](crate::dsp::fft::RealFft). It does NOT include the output
//! stage, and the reason is a rule rather than a preference.
//!
//! The first version of this card ran the real `audio::strip::StripCore`
//! and measured the whole chain at a level where the clip is linear. It
//! was a better picture — the stage's own tilt showed up, so the drive
//! knob moved the curve. `ui::tests::ui_layers_respect_their_contracts`
//! failed it in four places: **a device card must not know the engine.**
//! The kit has to render without an audio thread behind it, and a card
//! that constructs an engine node is a card that cannot.
//!
//! So the drawing was changed rather than the rule. What the shelves are
//! is not a second opinion, though: the frequencies, the Q and the warm
//! switch's figures all come from `params::strip`, the same constants
//! `audio::strip` prepares its own bands with, so the two cannot drift.
//! Only the arrangement is restated.
//!
//! The drive is therefore a NUMBER on this card rather than a shape. It
//! is not nothing — the corner tag says how far up it is, and `clean` when
//! the colour is a bit-exact wire — but the stage's tilt is not drawn.
//!
//! # The ghost is the warm switch
//!
//! Drawn under the live curve with the button forced off, so what the
//! switch adds is the gap between them. At the bottom that is a lift and
//! at the top a softening; across the mids the two lie on each other,
//! which is the claim `audio::strip` makes about why the switch is not a
//! second copy of the preamp's tilt.
//!
//! What the picture CANNOT show is the other half of the switch, and of
//! the shelves: they sit before the output stage, so they drive it as
//! well as shape it. No magnitude curve can draw that, and this one could
//! not even if it were allowed to run the stage. `audio::strip`'s tests
//! are where that claim lives.

use crate::params::strip::{
    DRIVE, HIGH, LOW, OUT, OUT_MAX_DB, OUT_MIN_DB, SHELF_MAX_DB, TABLE, WARM, WARM_NAMES, WARM_ON,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions of one strip, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StripUi {
    pub low: f32,
    pub high: f32,
    pub drive: f32,
    pub warm: f32,
    pub out: f32,
}

impl Default for StripUi {
    fn default() -> Self {
        Self {
            low: strip_norm(LOW, params::def(TABLE, LOW).default),
            high: strip_norm(HIGH, params::def(TABLE, HIGH).default),
            drive: strip_norm(DRIVE, params::def(TABLE, DRIVE).default),
            warm: strip_norm(WARM, params::def(TABLE, WARM).default),
            out: strip_norm(OUT, params::def(TABLE, OUT).default),
        }
    }
}

impl StripUi {
    /// This state's knob position for a wire id, mutably — public, so the
    /// app's `strip_knobs` can fill one from engine units by walking the
    /// table, the way `glue_knobs` does.
    pub fn slot_mut(&mut self, param: u32) -> Option<&mut f32> {
        self.slot(param)
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            LOW => &mut self.low,
            HIGH => &mut self.high,
            DRIVE => &mut self.drive,
            WARM => &mut self.warm,
            OUT => &mut self.out,
            _ => return None,
        })
    }
}

struct Spec {
    low: Param,
    high: Param,
    drive: Param,
    warm: Param,
    out: Param,
}

/// The five controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        // BIPOLAR, both: a tone control with no visible centre cannot be
        // put back to flat by eye, which is `sat::spec`'s argument about
        // bias and doubly true of an EQ.
        low: default(Param::db("low", -SHELF_MAX_DB, SHELF_MAX_DB).bipolar(), LOW),
        high: default(
            Param::db("high", -SHELF_MAX_DB, SHELF_MAX_DB).bipolar(),
            HIGH,
        ),
        drive: default(Param::percent("drive"), DRIVE),
        warm: default(Param::choice("warm", WARM_NAMES), WARM),
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

/// What the widget SHOWS for an engine-facing value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        DRIVE => value * 100.0,
        OUT => db_of(value),
        // The shelves are already in dB and the switch is already an
        // index.
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows, clamped through
/// the row so a knob at either stop cannot emit a letter to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        DRIVE => value / 100.0,
        OUT => 10.0f32.powf(value / 20.0),
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        LOW => s.low,
        HIGH => s.high,
        DRIVE => s.drive,
        WARM => s.warm,
        _ => s.out,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn strip_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`strip_value`], for a state stored in engine units.
pub fn strip_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. The warm switch does.
pub fn strip_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. None here does — two dB
/// controls, a percentage and a switch.
pub fn strip_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn strip_edits(state: &StripUi) -> Vec<ParamEdit> {
    [
        (LOW, state.low),
        (HIGH, state.high),
        (DRIVE, state.drive),
        (WARM, state.warm),
        (OUT, state.out),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: strip_value(param, norm),
    })
    .collect()
}

/// The reference rate the picture is measured at.
const PLOT_SR: f32 = 48_000.0;

/// Transform size. The lowest shelf is at 100 Hz and the DC blocker is
/// below it, so the bottom of the band needs bins to sit in.
const FFT_N: usize = 4096;

/// The band drawn.
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// The vertical half-range, in dB. The shelves reach twelve, the warm
/// switch and the stage's own tilt add a few more, and the axis is fixed
/// so leaning harder makes the curve bigger rather than rescaling under
/// it — the rule the sheen and the disperser are drawn to.
const DB_SPAN: f32 = 15.0;

/// Where the decade rules and their labels go.
const DECADES: [(f32, &str); 3] = [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")];

/// Points along each curve.
const CURVE_POINTS: usize = 200;

/// The strip's measured magnitude response, as `(hz, dB)` pairs.
///
/// `warm` overrides the switch, so the drawing can ask for the curve
/// without it and show what the button adds. The trim is forced to unity:
/// it is a level, and the plot is about a shape.
///
/// Empty when the transform will not prepare — the fail-open the drawing
/// checks, so a card that cannot measure draws no curve rather than a
/// wrong one.
fn curve(state: &StripUi, warm: bool) -> Vec<(f32, f32)> {
    use crate::dsp::filters::{BandShape, EqBand};
    use crate::params::strip as sp;

    let band = |hz: f32, gain_db: f32, shape: BandShape| {
        let mut b = EqBand::new();
        b.prepare(PLOT_SR, hz, sp::SHELF_Q, gain_db, shape);
        b
    };
    // The same four bands `audio::strip` prepares, from the same table
    // constants — restated arrangement, not restated numbers.
    let mut chain = vec![
        band(sp::LOW_HZ, strip_value(LOW, state.low), BandShape::LowShelf),
        band(
            sp::HIGH_HZ,
            strip_value(HIGH, state.high),
            BandShape::HighShelf,
        ),
    ];
    if warm {
        chain.push(band(sp::WARM_LOW_HZ, sp::WARM_LOW_DB, BandShape::LowShelf));
        chain.push(band(
            sp::WARM_HIGH_HZ,
            sp::WARM_HIGH_DB,
            BandShape::HighShelf,
        ));
    }

    // A unit impulse: everything here is linear, so there is no level to
    // choose and no clip or hiss to work around.
    let mut io = vec![0.0f32; FFT_N];
    io[0] = 1.0;
    for b in chain.iter_mut() {
        b.process(&mut io);
    }
    let l = io;

    let mut fft = crate::dsp::fft::RealFft::new();
    if !fft.prepare(FFT_N) {
        return Vec::new();
    }
    let bins = crate::dsp::fft::RealFft::bins(FFT_N);
    let mut real = vec![0.0f32; bins];
    let mut imag = vec![0.0f32; bins];
    let mut scratch = vec![0.0f32; crate::dsp::fft::RealFft::scratch_len(FFT_N)];
    fft.forward(&l, &mut real, &mut imag, &mut scratch);
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
            let db = if m > 0.0 { 20.0 * m.log10() } else { -DB_SPAN };
            (hz, db.clamp(-DB_SPAN, DB_SPAN))
        })
        .collect()
}

/// The hero: the strip's tone, and what the warm switch adds to it.
fn tone(ui: &mut egui::Ui, theme: &Theme, state: &StripUi) {
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
    let mid = field.center().y;
    let half = field.height() * 0.5;
    let x_of = |hz: f32| {
        let t = (hz / F_MIN).max(1e-6).log10() / (F_MAX / F_MIN).log10();
        field.left() + field.width() * t.clamp(0.0, 1.0)
    };
    let y_of = |db: f32| mid - (db / DB_SPAN).clamp(-1.0, 1.0) * half;

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
    // Half scale, so a gentle curve has something to be gentle against.
    for db in [-DB_SPAN * 0.5, DB_SPAN * 0.5] {
        painter.line_segment(
            [
                egui::pos2(field.left(), y_of(db)),
                egui::pos2(field.right(), y_of(db)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.grid_sub),
        );
    }
    // Unity.
    painter.line_segment(
        [
            egui::pos2(field.left(), mid),
            egui::pos2(field.right(), mid),
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

    let warm_on = strip_value(WARM, state.warm).round() as u32 == WARM_ON;
    // The ghost first, so the live curve draws over it. Only when the
    // switch is ON — with it off the two would be the same line, and a
    // ghost hiding exactly under the curve is a ghost that has nothing to
    // say.
    if warm_on {
        draw(&curve(state, false), theme.role_mod_dim, stroke::HAIR);
    }
    draw(&curve(state, warm_on), theme.role_mod, stroke::BOLD);

    // The corner tag. The drive is the one control whose contribution to
    // the picture is real but modest — the stage's tilt — so the number
    // is worth saying out loud beside it. `clean` is `Preamp`'s own
    // promise: at zero the colour is a bit-exact wire, and the shelves
    // carry on regardless, which is a two-band EQ.
    let drive = strip_value(DRIVE, state.drive);
    let tag = if drive <= 0.0 {
        "clean".to_owned()
    } else {
        format!("drive {:.0} %", drive * 100.0)
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

/// The five cells, in the order the signal meets them.
fn strip_cells(s: &Spec) -> [(&Param, u32); 5] {
    [
        (&s.low, LOW),
        (&s.high, HIGH),
        (&s.warm, WARM),
        (&s.drive, DRIVE),
        (&s.out, OUT),
    ]
}

/// The width the strip needs, summed from the same per-cell figure it
/// draws each cell at — a share is not a sum.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip_cells(s);
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

/// Draw the strip card. Returns the edits the user just made.
pub fn strip_card(ui: &mut egui::Ui, theme: &Theme, state: &mut StripUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "strip", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // A READOUT, not a draggable target — rule 1 of the
                    // device-UI contract. Five knobs shape this curve and
                    // a drag could not say which.
                    poly_widgets::CurveRegion::Plot => tone(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => {
                        let h = ui.available_height();
                        ui.horizontal(|ui| {
                            for (param, id) in strip_cells(&s) {
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
                                                value: strip_value(id, *norm),
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

    /// The dB at a frequency, read off a measured curve.
    fn db_at(points: &[(f32, f32)], hz: f32) -> f32 {
        points
            .iter()
            .min_by(|a, b| {
                (a.0 - hz)
                    .abs()
                    .partial_cmp(&(b.0 - hz).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, db)| *db)
            .unwrap_or(0.0)
    }

    fn state(low: f32, high: f32, drive: f32, warm: bool) -> StripUi {
        StripUi {
            low: strip_norm(LOW, low),
            high: strip_norm(HIGH, high),
            drive: strip_norm(DRIVE, drive),
            warm: strip_norm(WARM, if warm { WARM_ON as f32 } else { 0.0 }),
            out: strip_norm(OUT, 1.0),
        }
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = strip_edits(&StripUi::default());
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
                let value = strip_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (strip_value(row.id, 0.0) - row.min).abs() < 1e-2,
                "{} floor",
                row.name
            );
            assert!(
                (strip_value(row.id, 1.0) - row.max).abs() < 1e-2,
                "{} ceiling",
                row.name
            );
        }
    }

    /// Stated over VALUES, since the switch quantizes.
    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = strip_value(row.id, strip_norm(row.id, value));
                let tol = if row.id == WARM {
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
        let fresh = StripUi::default();
        for (id, norm) in [
            (LOW, fresh.low),
            (HIGH, fresh.high),
            (DRIVE, fresh.drive),
            (WARM, fresh.warm),
            (OUT, fresh.out),
        ] {
            let want = params::def(TABLE, id).default;
            let got = strip_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    #[test]
    fn only_the_switch_steps_and_nothing_is_log() {
        assert!(strip_is_discrete(WARM));
        for id in [LOW, HIGH, DRIVE, OUT] {
            assert!(!strip_is_discrete(id), "id {id} should sweep");
        }
        for id in [LOW, HIGH, DRIVE, WARM, OUT] {
            assert!(!strip_is_log(id), "id {id} should not be log");
        }
    }

    /// The shelves show up where they belong, and the drawing is of the
    /// whole strip rather than of an idea about it.
    #[test]
    fn the_shelves_show_up_in_the_curve() {
        let flat = curve(&state(0.0, 0.0, 0.0, false), false);
        let lifted = curve(&state(9.0, 0.0, 0.0, false), false);
        assert!(
            db_at(&lifted, 40.0) - db_at(&flat, 40.0) > 6.0,
            "a +9 dB low shelf moved 40 Hz by {:.1} dB",
            db_at(&lifted, 40.0) - db_at(&flat, 40.0)
        );
        let bright = curve(&state(0.0, 9.0, 0.0, false), false);
        assert!(
            db_at(&bright, 16_000.0) - db_at(&flat, 16_000.0) > 6.0,
            "a +9 dB high shelf moved 16 kHz by {:.1} dB",
            db_at(&bright, 16_000.0) - db_at(&flat, 16_000.0)
        );
    }

    /// The ghost has something to say: the warm switch lifts the bottom
    /// and softens the top, and leaves the mids where they were.
    #[test]
    fn the_warm_ghost_differs_at_the_ends_and_not_the_middle() {
        let s = state(0.0, 0.0, 0.0, true);
        let with = curve(&s, true);
        let without = curve(&s, false);
        assert!(
            db_at(&with, 50.0) - db_at(&without, 50.0) > 1.0,
            "warm lifted 50 Hz by only {:.2} dB",
            db_at(&with, 50.0) - db_at(&without, 50.0)
        );
        assert!(
            db_at(&with, 16_000.0) - db_at(&without, 16_000.0) < -0.5,
            "warm softened 16 kHz by only {:.2} dB",
            db_at(&with, 16_000.0) - db_at(&without, 16_000.0)
        );
        assert!(
            (db_at(&with, 1_000.0) - db_at(&without, 1_000.0)).abs() < 0.3,
            "warm moved 1 kHz, which is the mids it leaves alone"
        );
    }
}
