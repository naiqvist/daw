//! The tilt device card — the see-saw, measured.
//!
//! Same shape as the lo-fi, sheen and disperser cards: normalized knob
//! state here, natural values out as [`ParamEdit`]s. Ids, ranges and
//! defaults come from [`crate::params::tilt`] — the one table this
//! widget, `Node::Tilt`'s apply arm and the app's edit routing all read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ tilt ───────────────────────────────────┐
//! │                            12 dB across │
//! │  ────╲                                  │
//! │       ╲────      ▲                      │  ← hero, and the fulcrum
//! │   100      1k        10k                │
//! │    tilt          pivot                  │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # The first frequency-domain hero in the rack
//!
//! The lo-fi's staircase, the sheen's added lane and the disperser's
//! impulse are all pictures of time. This device is the one that has a
//! magnitude worth drawing — it is the only thing it changes — so the
//! axis is frequency and the curve is the response.
//!
//! # It is MEASURED, not derived
//!
//! [`curve`] sends a real impulse through a real
//! [`Tilt`](crate::dsp::filters::Tilt) and takes the magnitude of the
//! result with [`RealFft`](crate::dsp::fft::RealFft). No transfer
//! function is written out in this file.
//!
//! That is not only the house style — it is the only option. The kernel's
//! gains and its pole coefficient are private, and the one subtlety worth
//! drawing is a correction applied inside `prepare`: a first-order
//! see-saw's unity crossing does NOT sit at its corner, so the kernel
//! places the corner at `pivot × g_hi` to put the crossing where the user
//! pointed. Skipping that slides the pivot by an octave at ±6 dB. A card
//! that recomputed the curve from the pivot knob would draw the
//! uncorrected filter and quietly disagree with the audio — so this one
//! measures, and a test asserts the crossing lands on the pivot.
//!
//! It is also the first thing in the tree to use `dsp::fft` for anything.
//!
//! # Nothing is auto-fitted
//!
//! The vertical axis is fixed at the kernel's own ±24 dB, so leaning
//! harder makes the curve steeper instead of rescaling the axis under it
//! — the same rule the sheen and the disperser are drawn to. At rest the
//! card shows a flat line, because at rest the device is flat.

use crate::params::tilt::{PIVOT, PIVOT_MAX_HZ, PIVOT_MIN_HZ, TABLE, TILT, TILT_MAX_DB};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions of one tilt, normalized. Serialized into project files,
/// so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TiltUi {
    pub tilt: f32,
    pub pivot: f32,
}

impl Default for TiltUi {
    fn default() -> Self {
        Self {
            tilt: tilt_norm(TILT, params::def(TABLE, TILT).default),
            pivot: tilt_norm(PIVOT, params::def(TABLE, PIVOT).default),
        }
    }
}

impl TiltUi {
    /// This state's knob position for a wire id, mutably.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            TILT => &mut self.tilt,
            PIVOT => &mut self.pivot,
            _ => return None,
        })
    }
}

struct Spec {
    tilt: Param,
    pivot: Param,
}

/// The two controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        // BIPOLAR, and in dB because that is what the value IS — the
        // table stores the figure the kernel takes. The zero is visible
        // on the cell for the reason `sat::spec` gives about bias: a
        // bipolar control with no visible centre cannot be returned to
        // it by eye, and "put it back to flat" is half of why anyone
        // touches a tilt.
        tilt: default(Param::db("tilt", -TILT_MAX_DB, TILT_MAX_DB).bipolar(), TILT),
        // LOG-mapped, by `Param::hz`.
        pivot: default(Param::hz("pivot", PIVOT_MIN_HZ, PIVOT_MAX_HZ), PIVOT),
    }
}

/// What the widget SHOWS for an engine-facing value.
///
/// The identity, unusually and honestly: a tilt in dB and a pivot in
/// hertz are already the two things a musician would say out loud, and
/// the table stores exactly those. Kept as a function so the pair with
/// [`natural`] stays visible.
fn shown(_param: u32, value: f32) -> f32 {
    value
}

/// What the ENGINE receives for a value the widget shows, clamped through
/// the row so a knob at either stop cannot emit a letter to bin.
fn natural(param: u32, value: f32) -> f32 {
    params::def(TABLE, param).clamp(value)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        TILT => s.tilt,
        _ => s.pivot,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn tilt_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`tilt_value`], for a state stored in engine units.
pub fn tilt_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. Neither does — both
/// controls sweep.
pub fn tilt_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The pivot does, which is
/// what makes a modulation sweep of it move in octaves the way the knob
/// does. The tilt is linear in dB, which is already a ratio scale.
pub fn tilt_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn tilt_edits(state: &TiltUi) -> Vec<ParamEdit> {
    [(TILT, state.tilt), (PIVOT, state.pivot)]
        .into_iter()
        .map(|(param, norm)| ParamEdit {
            param,
            value: tilt_value(param, norm),
        })
        .collect()
}

/// The reference rate the picture is measured at.
const PLOT_SR: f32 = 48_000.0;

/// Transform size. 2048 bins the band at about 23 Hz, which is finer than
/// a first-order see-saw bends anywhere — and small enough that measuring
/// the response every frame costs a fraction of a millisecond.
const FFT_N: usize = 2048;

/// The band drawn, and the band a tilt is about.
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// The vertical half-range, in dB: the kernel's own ceiling, so no
/// setting can draw a curve that leaves the lane.
const DB_SPAN: f32 = TILT_MAX_DB;

/// Where the decade rules and their labels go.
const DECADES: [(f32, &str); 3] = [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")];

/// Points along the curve. Enough that a 6 dB/octave bend reads as a
/// curve rather than as a polyline.
const CURVE_POINTS: usize = 160;

/// The measured magnitude response, as `(hz, dB)` pairs across
/// [`F_MIN`]..=[`F_MAX`].
///
/// A real impulse through a real [`Tilt`], transformed. The FFT is
/// documented unnormalised and a unit impulse has magnitude one in every
/// bin, so what comes back IS `|H(f)|` with nothing to divide out.
///
/// Empty when the transform will not prepare, which is the fail-open the
/// drawing checks: a card that cannot measure draws no curve rather than
/// a wrong one.
fn curve(state: &TiltUi) -> Vec<(f32, f32)> {
    let mut io = vec![0.0f32; FFT_N];
    io[0] = 1.0;
    let mut filter = crate::dsp::filters::Tilt::new();
    filter.prepare(
        PLOT_SR,
        tilt_value(PIVOT, state.pivot),
        tilt_value(TILT, state.tilt),
    );
    filter.process(&mut io);

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
            // Log-spaced, because the axis is.
            let hz = F_MIN * (F_MAX / F_MIN).powf(t);
            // Linear between the two bins either side, so a curve drawn
            // finer than the transform does not go up in steps.
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

/// The hero: the see-saw, and the fulcrum it balances on.
fn seesaw(ui: &mut egui::Ui, theme: &Theme, state: &TiltUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);
    // Room along the bottom for the decade labels, so the curve cannot
    // print through them.
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

    // Half-scale rules, so a gentle tilt still has something to be
    // gentle against.
    for db in [-DB_SPAN * 0.5, DB_SPAN * 0.5] {
        painter.line_segment(
            [
                egui::pos2(field.left(), y_of(db)),
                egui::pos2(field.right(), y_of(db)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.grid_sub),
        );
        painter.text(
            egui::pos2(field.left(), y_of(db)),
            egui::Align2::LEFT_BOTTOM,
            format!("{db:+.0}"),
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    }

    // Unity. The line the plank pivots about, so it is drawn brighter
    // than the rules that are only scale.
    painter.line_segment(
        [
            egui::pos2(field.left(), mid),
            egui::pos2(field.right(), mid),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    let points = curve(state);
    if !points.is_empty() {
        // A one-pixel blue registration beneath the red response gives the
        // measured line a crisp two-ink edge. It is the same response twice,
        // never a second or inferred curve.
        painter.add(egui::Shape::line(
            points
                .iter()
                .map(|(hz, db)| {
                    egui::pos2(x_of(*hz), y_of(*db)) + egui::vec2(0.0, theme.sp(stroke::HAIR))
                })
                .collect(),
            egui::Stroke::new(stroke::BOLD, theme.role_time_dim),
        ));
        painter.add(egui::Shape::line(
            points
                .iter()
                .map(|(hz, db)| egui::pos2(x_of(*hz), y_of(*db)))
                .collect(),
            egui::Stroke::new(stroke::BOLD, theme.role_mod),
        ));
    }

    // THE FULCRUM. A see-saw's whole idea is the point it balances on,
    // and the kernel goes to real trouble to keep the balance exactly
    // here whatever the lean — see this module's header. Drawing it is
    // how that promise becomes visible instead of merely true.
    let pivot_x = x_of(tilt_value(PIVOT, state.pivot));
    let w = theme.sp(space::SM);
    painter.vline(
        pivot_x,
        field.y_range(),
        egui::Stroke::new(stroke::HAIR, theme.role_time_dim.gamma_multiply(0.8)),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(pivot_x, mid + 1.0),
            egui::pos2(pivot_x - w * 0.5, mid + 1.0 + w),
            egui::pos2(pivot_x + w * 0.5, mid + 1.0 + w),
        ],
        theme.role_time_dim,
        egui::Stroke::NONE,
    ));
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(pivot_x, mid),
            egui::vec2(theme.sp(space::XXS), theme.sp(space::XXS)),
        ),
        0.0,
        theme.role_mod,
    );

    // The corner tag: the whole span the plank covers, which is twice the
    // lean and the number neither cell shows.
    let db = tilt_value(TILT, state.tilt);
    let tag = if db == 0.0 {
        // Zero tilt is `g_hi = 1` and `g_delta = 0`, which is `x`. Said
        // out loud for the reason the lo-fi says `clean`.
        "flat".to_owned()
    } else {
        format!("{:.0} dB across", db.abs() * 2.0)
    };
    painter.text(
        egui::pos2(field.right(), field.top()),
        egui::Align2::RIGHT_TOP,
        tag,
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.text_muted,
    );
    if let (Some(low), Some(high)) = (points.first(), points.last()) {
        painter.text(
            egui::pos2(field.left(), field.top()),
            egui::Align2::LEFT_TOP,
            format!("LOW {:+.1} // HIGH {:+.1}", low.1, high.1),
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.role_time_dim,
        );
    }
}

/// What ONE cell needs: room for the widest thing it will ever print.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The two cells of the strip.
fn strip(s: &Spec) -> [(&Param, u32); 2] {
    [(&s.tilt, TILT), (&s.pivot, PIVOT)]
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

/// Draw the tilt card. Returns the edits the user just made.
pub fn tilt_card(ui: &mut egui::Ui, theme: &Theme, state: &mut TiltUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "tilt", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // A READOUT, not a draggable target. Dragging an EQ
                    // curve is a real gesture and a defensible one, but it
                    // would need its own interaction per handle under rule
                    // 1 of the device-UI contract, and a two-knob device
                    // whose knobs are already on screen does not need a
                    // second way to reach them.
                    poly_widgets::CurveRegion::Plot => seesaw(ui, theme, state),
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
                                                value: tilt_value(id, *norm),
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

    /// The dB at a frequency, read off the measured curve.
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

    fn state(tilt_db: f32, pivot_hz: f32) -> TiltUi {
        TiltUi {
            tilt: tilt_norm(TILT, tilt_db),
            pivot: tilt_norm(PIVOT, pivot_hz),
        }
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = tilt_edits(&TiltUi::default());
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
                let value = tilt_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (tilt_value(row.id, 0.0) - row.min).abs() < 1e-3,
                "{}",
                row.name
            );
            assert!(
                (tilt_value(row.id, 1.0) - row.max).abs() < 1e-3,
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
                let back = tilt_value(row.id, tilt_norm(row.id, value));
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
        let fresh = TiltUi::default();
        for (id, norm) in [(TILT, fresh.tilt), (PIVOT, fresh.pivot)] {
            let want = params::def(TABLE, id).default;
            let got = tilt_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    #[test]
    fn nothing_steps_and_only_the_pivot_is_log() {
        for id in [TILT, PIVOT] {
            assert!(!tilt_is_discrete(id), "id {id} should sweep");
        }
        assert!(tilt_is_log(PIVOT));
        assert!(!tilt_is_log(TILT));
    }

    /// A fresh tilt is FLAT, and measurably so — not merely defaulted to
    /// zero on the knob. This is the corrective-device promise the params
    /// module argues for, checked through the same measurement the card
    /// draws.
    #[test]
    fn a_fresh_tilt_is_measurably_flat() {
        let points = curve(&TiltUi::default());
        assert!(!points.is_empty(), "the transform refused to prepare");
        for (hz, db) in &points {
            assert!(
                db.abs() < 0.1,
                "flat tilt measured {db:.3} dB at {hz:.0} Hz"
            );
        }
    }

    /// The see-saw leans the way it says: `+n` lifts the top and drops
    /// the bottom by about the same, and `-n` mirrors it.
    #[test]
    fn the_plank_leans_both_ways_by_what_it_says() {
        for lean in [6.0f32, 12.0] {
            let up = curve(&state(lean, 1_000.0));
            assert!(db_at(&up, 16_000.0) > lean * 0.7, "{lean} dB did not lift");
            assert!(db_at(&up, 30.0) < -lean * 0.7, "{lean} dB did not drop");

            let down = curve(&state(-lean, 1_000.0));
            assert!(db_at(&down, 16_000.0) < -lean * 0.7, "-{lean} did not drop");
            assert!(db_at(&down, 30.0) > lean * 0.7, "-{lean} did not lift");
        }
    }

    /// THE ONE WORTH HAVING: the unity crossing sits on the pivot, at
    /// every lean.
    ///
    /// A first-order see-saw's crossing is NOT at its corner — it sits at
    /// `corner / g_hi` — so `Tilt::prepare` places the corner at
    /// `pivot × g_hi` to put it back. Its doc says skipping that slides
    /// the pivot by an octave at ±6 dB, which is exactly the kind of
    /// drift an ear blames on the material. Measured here through the
    /// card's own transform, so the drawing and the audio are checked
    /// together.
    #[test]
    fn the_curve_crosses_unity_at_the_pivot() {
        // The corners of the range actually shipped, worst case last:
        // `params::tilt::PIVOT_MAX_HZ` records the measurement these
        // bounds were drawn from, and this is that table's promise
        // asserted. Widen either range and this is what fails.
        for pivot in [200.0f32, 1_000.0, PIVOT_MAX_HZ] {
            for lean in [-TILT_MAX_DB, -6.0, 6.0, TILT_MAX_DB] {
                let points = curve(&state(lean, pivot));
                let at = db_at(&points, pivot);
                assert!(
                    at.abs() < 0.75,
                    "pivot {pivot} Hz at {lean:+} dB measured {at:.2} dB, \
                     so the crossing has slid off the pivot"
                );
            }
        }
    }

    /// Leaning harder makes the curve steeper rather than rescaling the
    /// axis — the no-auto-fit rule, asserted.
    #[test]
    fn a_harder_lean_is_a_steeper_curve() {
        let gentle = curve(&state(4.0, 1_000.0));
        let hard = curve(&state(TILT_MAX_DB, 1_000.0));
        let span = |p: &[(f32, f32)]| db_at(p, 16_000.0) - db_at(p, 30.0);
        assert!(
            span(&hard) > span(&gentle) * 2.0,
            "{TILT_MAX_DB} dB spanned {} and 4 dB spanned {}",
            span(&hard),
            span(&gentle)
        );
    }
}
