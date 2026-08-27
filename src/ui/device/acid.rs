//! The acid mono's card — the filter, and how far the envelope opens it.
//!
//! Normalized knob state here, natural values out as [`ParamEdit`]s. Ids,
//! ranges and defaults come from [`crate::params::acid`] — the one table
//! this widget, `audio::acid::AcidVoice` and the app's edit routing all
//! read.
//!
//! # The hero is the SWEEP, not the filter
//!
//! One response curve would be a photograph of a filter that never sits
//! still: on this instrument the envelope is what the filter is FOR, and
//! a picture of the cutoff knob alone would leave `env mod` and the
//! resonance's interaction with it invisible.
//!
//! So the card draws two curves — the filter with the envelope fully
//! open, and at rest — and the gap between them is the squelch. `cutoff`
//! slides both, `env mod` opens the gap, and `resonance` grows the peak
//! on each.
//!
//! Measured through the real [`Svf`](crate::dsp::filters::Svf) and
//! [`OnePole`](crate::dsp::filters::OnePole), at the same corners and Q
//! the voice computes, so the eighteen-decibel slope in the picture is
//! the eighteen decibels in the audio. The voice itself is in `audio::`
//! and a card may not know the engine — `ui_layers_respect_their_contracts`
//! saw to that on the strip — so the two kernels are rebuilt here from
//! `params::acid`'s own constants. The arrangement is restated; the
//! numbers are not.
//!
//! # What the picture cannot show
//!
//! The slide and the accent, which are the two gestures the instrument
//! exists for and both of which are properties of a NOTE rather than of
//! a setting. They live in `audio::acid`'s tests, and on the card they
//! are a glide time and an accent depth in their own cells.

use crate::params::acid::{
    ACCENT, CUTOFF, CUTOFF_MAX_HZ, CUTOFF_MIN_HZ, DECAY, DECAY_MAX_MS, DECAY_MIN_MS, DRIVE,
    ENV_MOD, ENV_OCTAVES, GLIDE, GLIDE_MAX_MS, GLIDE_MIN_MS, LEVEL, RESONANCE, TABLE, TUNE,
    TUNE_MAX_ST, WAVE, WAVE_NAMES,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions of one acid mono, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AcidUi {
    pub wave: f32,
    pub tune: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub env_mod: f32,
    pub decay: f32,
    pub accent: f32,
    pub glide: f32,
    pub drive: f32,
    pub level: f32,
}

impl Default for AcidUi {
    fn default() -> Self {
        let at = |id: u32| acid_norm(id, params::def(TABLE, id).default);
        Self {
            wave: at(WAVE),
            tune: at(TUNE),
            cutoff: at(CUTOFF),
            resonance: at(RESONANCE),
            env_mod: at(ENV_MOD),
            decay: at(DECAY),
            accent: at(ACCENT),
            glide: at(GLIDE),
            drive: at(DRIVE),
            level: at(LEVEL),
        }
    }
}

impl AcidUi {
    /// This state's knob position for a wire id, mutably — public, so the
    /// app can fill one from engine units by walking the table.
    pub fn slot_mut(&mut self, param: u32) -> Option<&mut f32> {
        self.slot(param)
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            WAVE => &mut self.wave,
            TUNE => &mut self.tune,
            CUTOFF => &mut self.cutoff,
            RESONANCE => &mut self.resonance,
            ENV_MOD => &mut self.env_mod,
            DECAY => &mut self.decay,
            ACCENT => &mut self.accent,
            GLIDE => &mut self.glide,
            DRIVE => &mut self.drive,
            LEVEL => &mut self.level,
            _ => return None,
        })
    }
}

struct Spec {
    wave: Param,
    tune: Param,
    cutoff: Param,
    resonance: Param,
    env_mod: Param,
    decay: Param,
    accent: Param,
    glide: Param,
    drive: Param,
    level: Param,
}

/// The ten controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    let ms = |name: &'static str, min: f32, max: f32| {
        Param::new(name, Mapping::Log { min, max }, Unit::Ms)
    };
    Spec {
        wave: default(Param::choice("wave", WAVE_NAMES), WAVE),
        tune: default(
            Param::new(
                "tune",
                Mapping::Linear {
                    min: -TUNE_MAX_ST,
                    max: TUNE_MAX_ST,
                },
                Unit::Semitones,
            )
            .bipolar(),
            TUNE,
        ),
        cutoff: default(Param::hz("cutoff", CUTOFF_MIN_HZ, CUTOFF_MAX_HZ), CUTOFF),
        resonance: default(Param::percent("res"), RESONANCE),
        env_mod: default(Param::percent("env"), ENV_MOD),
        decay: default(ms("decay", DECAY_MIN_MS, DECAY_MAX_MS), DECAY),
        accent: default(Param::percent("accent"), ACCENT),
        glide: default(ms("glide", GLIDE_MIN_MS, GLIDE_MAX_MS), GLIDE),
        drive: default(Param::percent("drive"), DRIVE),
        level: default(
            Param::new(
                "level",
                Mapping::Linear {
                    min: 0.0,
                    max: 200.0,
                },
                Unit::Percent,
            ),
            LEVEL,
        ),
    }
}

/// What the widget SHOWS for an engine-facing value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        RESONANCE | ENV_MOD | ACCENT | DRIVE | LEVEL => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        RESONANCE | ENV_MOD | ACCENT | DRIVE | LEVEL => value / 100.0,
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        WAVE => s.wave,
        TUNE => s.tune,
        CUTOFF => s.cutoff,
        RESONANCE => s.resonance,
        ENV_MOD => s.env_mod,
        DECAY => s.decay,
        ACCENT => s.accent,
        GLIDE => s.glide,
        DRIVE => s.drive,
        _ => s.level,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn acid_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`acid_value`], for a state stored in engine units.
pub fn acid_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. The waveform does.
pub fn acid_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale: the cutoff and both times.
pub fn acid_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn acid_edits(state: &AcidUi) -> Vec<ParamEdit> {
    [
        (WAVE, state.wave),
        (TUNE, state.tune),
        (CUTOFF, state.cutoff),
        (RESONANCE, state.resonance),
        (ENV_MOD, state.env_mod),
        (DECAY, state.decay),
        (ACCENT, state.accent),
        (GLIDE, state.glide),
        (DRIVE, state.drive),
        (LEVEL, state.level),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: acid_value(param, norm),
    })
    .collect()
}

/// The reference rate the picture is measured at.
const PLOT_SR: f32 = 48_000.0;
/// Transform size — the cutoff reaches 60 Hz, so the bottom needs bins.
const FFT_N: usize = 4096;
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;
/// The vertical window. Room above unity for the resonant peak, and a
/// long way below it for the slope.
const DB_TOP: f32 = 18.0;
const DB_BOTTOM: f32 = -42.0;
const CURVE_POINTS: usize = 220;

/// The measured response of the voice's filter pair, with the envelope
/// `env` of the way open.
///
/// The corner and the Q are computed exactly as `AcidVoice::render_add`
/// computes them, from the same `params::acid` constants, so the picture
/// and the audio cannot disagree about the slope.
fn curve(state: &AcidUi, env: f32) -> Vec<(f32, f32)> {
    use crate::dsp::filters::{Mode, OnePole, Svf};

    let res = acid_value(RESONANCE, state.resonance).clamp(0.0, 1.0);
    let octaves = acid_value(ENV_MOD, state.env_mod) * env * ENV_OCTAVES;
    let cutoff = (acid_value(CUTOFF, state.cutoff) * octaves.exp2()).clamp(20.0, PLOT_SR * 0.45);
    let q = 0.7 + res * res * 12.0;

    let mut svf = Svf::new();
    svf.prepare(PLOT_SR, cutoff, q);
    let mut pole = OnePole::new();
    pole.prepare(PLOT_SR, (cutoff * 1.5).clamp(20.0, PLOT_SR * 0.45));

    let mut io = vec![0.0f32; FFT_N];
    io[0] = 1.0;
    svf.process(&mut io, Mode::Lowpass);
    pole.process_lowpass(&mut io);

    let mut fft = crate::dsp::fft::RealFft::new();
    if !fft.prepare(FFT_N) {
        return Vec::new();
    }
    let bins = crate::dsp::fft::RealFft::bins(FFT_N);
    let mut re = vec![0.0f32; bins];
    let mut im = vec![0.0f32; bins];
    let mut scratch = vec![0.0f32; crate::dsp::fft::RealFft::scratch_len(FFT_N)];
    fft.forward(&io, &mut re, &mut im, &mut scratch);
    let mut mag = vec![0.0f32; bins];
    let mut ph = vec![0.0f32; bins];
    crate::dsp::fft::magnitude_phase(&re, &im, &mut mag, &mut ph);

    let per_bin = PLOT_SR / FFT_N as f32;
    (0..CURVE_POINTS)
        .map(|i| {
            let t = i as f32 / (CURVE_POINTS - 1) as f32;
            let hz = F_MIN * (F_MAX / F_MIN).powf(t);
            let at = hz / per_bin;
            let lo = (at.floor() as usize).min(bins - 1);
            let hi = (lo + 1).min(bins - 1);
            let frac = at - at.floor();
            let m = mag[lo] + (mag[hi] - mag[lo]) * frac;
            let db = if m > 0.0 { 20.0 * m.log10() } else { DB_BOTTOM };
            (hz, db.clamp(DB_BOTTOM, DB_TOP))
        })
        .collect()
}

/// The hero: the filter at both ends of the envelope's travel.
fn sweep(ui: &mut egui::Ui, theme: &Theme, state: &AcidUi) {
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

    for (hz, name) in [(100.0f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
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

    // At REST first, so the open one draws over it. The gap between them
    // is the squelch.
    draw(&curve(state, 0.0), theme.role_mod_dim, stroke::HAIR);
    draw(&curve(state, 1.0), theme.role_mod, stroke::BOLD);

    // The corner tag: how far the envelope travels, in octaves — the
    // number `env mod` actually sets and the one neither cell shows.
    let octaves = acid_value(ENV_MOD, state.env_mod) * ENV_OCTAVES;
    let tag = if octaves < 0.05 {
        "no sweep".to_owned()
    } else {
        format!("sweep {octaves:.1} oct")
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

/// Two balanced rows of five: the tone, then the gesture.
fn rows(s: &Spec) -> [[(&Param, u32); 5]; 2] {
    [
        [
            (&s.wave, WAVE),
            (&s.tune, TUNE),
            (&s.cutoff, CUTOFF),
            (&s.resonance, RESONANCE),
            (&s.env_mod, ENV_MOD),
        ],
        [
            (&s.decay, DECAY),
            (&s.accent, ACCENT),
            (&s.glide, GLIDE),
            (&s.drive, DRIVE),
            (&s.level, LEVEL),
        ],
    ]
}

/// The width the footer needs: the wider of the two rows.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    rows(s)
        .iter()
        .map(|row| {
            let sum: f32 = row.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
            sum + ui.spacing().item_spacing.x * (row.len() - 1) as f32
        })
        .fold(0.0f32, f32::max)
}

/// A labelled cell is TWO `POLY_CELL_H` units, so two rows is four.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = 2 * CELL_UNITS;

/// The card's only width contract.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the acid card. Returns the edits the user just made.
pub fn acid_card(ui: &mut egui::Ui, theme: &Theme, state: &mut AcidUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "acid", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    // A READOUT — ten knobs shape this pair of curves and
                    // a drag could not say which, which is rule 1 of the
                    // device-UI contract.
                    poly_widgets::CurveRegion::Plot => sweep(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => {
                        // Row height off the footer with the GAPS TAKEN
                        // FIRST — rule 2 of the layout note.
                        let gap = theme.sp(space::XXS);
                        let count = 2.0f32;
                        let h = ((ui.available_height() - gap * (count - 1.0)) / count).max(1.0);
                        ui.spacing_mut().item_spacing.y = gap;
                        for row in rows(&s) {
                            ui.horizontal(|ui| {
                                for (param, id) in row {
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
                                                    value: acid_value(id, *norm),
                                                });
                                            }
                                        },
                                    );
                                }
                            });
                        }
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

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = acid_edits(&AcidUi::default());
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
                let value = acid_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (acid_value(row.id, 0.0) - row.min).abs() < 1e-2,
                "{} floor",
                row.name
            );
            assert!(
                (acid_value(row.id, 1.0) - row.max).abs() < 1e-2,
                "{} ceiling",
                row.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = acid_value(row.id, acid_norm(row.id, value));
                let tol = if row.id == WAVE {
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
        let fresh = AcidUi::default();
        for (id, norm) in [
            (WAVE, fresh.wave),
            (TUNE, fresh.tune),
            (CUTOFF, fresh.cutoff),
            (RESONANCE, fresh.resonance),
            (ENV_MOD, fresh.env_mod),
            (DECAY, fresh.decay),
            (ACCENT, fresh.accent),
            (GLIDE, fresh.glide),
            (DRIVE, fresh.drive),
            (LEVEL, fresh.level),
        ] {
            let want = params::def(TABLE, id).default;
            let got = acid_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    #[test]
    fn only_the_waveform_steps_and_the_cutoff_and_times_are_log() {
        assert!(acid_is_discrete(WAVE));
        for id in [TUNE, CUTOFF, RESONANCE, ENV_MOD, ACCENT, DRIVE, LEVEL] {
            assert!(!acid_is_discrete(id), "id {id} should sweep");
        }
        for id in [CUTOFF, DECAY, GLIDE] {
            assert!(acid_is_log(id), "id {id} should be log");
        }
        for id in [WAVE, TUNE, RESONANCE, ENV_MOD, ACCENT, DRIVE, LEVEL] {
            assert!(!acid_is_log(id), "id {id} should not be log");
        }
    }

    /// The hero's whole claim: the envelope opens the filter, so the open
    /// curve passes more at the top than the resting one does.
    #[test]
    fn the_open_curve_is_brighter_than_the_resting_one() {
        let state = AcidUi::default();
        let rest = curve(&state, 0.0);
        let open = curve(&state, 1.0);
        assert!(!rest.is_empty() && !open.is_empty());
        assert!(
            db_at(&open, 4_000.0) > db_at(&rest, 4_000.0) + 12.0,
            "the sweep only moved 4 kHz from {:.1} to {:.1} dB",
            db_at(&rest, 4_000.0),
            db_at(&open, 4_000.0)
        );
    }

    /// And with no env mod the two lie on each other, which is what "no
    /// sweep" means.
    #[test]
    fn no_env_mod_is_no_sweep() {
        let state = AcidUi {
            env_mod: acid_norm(ENV_MOD, 0.0),
            ..AcidUi::default()
        };
        let rest = curve(&state, 0.0);
        let open = curve(&state, 1.0);
        for ((hz, a), (_, b)) in rest.iter().zip(open.iter()) {
            assert!(
                (a - b).abs() < 1e-3,
                "the curves parted at {hz:.0} Hz with no env mod"
            );
        }
    }

    /// The slope is the machine's eighteen decibels per octave, measured
    /// rather than asserted — twelve from the SVF and six from the pole,
    /// which is the whole reason there are two filters.
    #[test]
    fn the_slope_is_eighteen_decibels_per_octave() {
        // Low resonance, so the peak does not tilt the measurement, and a
        // cutoff with room for two octaves above it.
        let state = AcidUi {
            cutoff: acid_norm(CUTOFF, 500.0),
            resonance: acid_norm(RESONANCE, 0.0),
            env_mod: acid_norm(ENV_MOD, 0.0),
            ..AcidUi::default()
        };
        let c = curve(&state, 0.0);
        // ONE and TWO octaves above the corner. The first draft probed at
        // four and eight kilohertz, which is three and four octaves up —
        // fifty-four and seventy-two decibels down, both flattened
        // against the plot's own -42 dB floor, so the measurement read a
        // slope of exactly zero. The clamp is right for the picture; the
        // ruler was in the wrong part of it.
        let a = db_at(&c, 1_000.0);
        let b = db_at(&c, 2_000.0);
        let per_octave = a - b;
        assert!(
            (per_octave - 18.0).abs() < 4.0,
            "the slope measured {per_octave:.1} dB/octave, not eighteen"
        );
    }

    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        assert_eq!(FOOTER_ROWS, 2 * CELL_UNITS);
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let footer =
            theme.sp(control::POLY_CELL_H) * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(
            plot > theme.sp(control::POLY_CELL_H) * 3.0,
            "the footer left the plot only {plot} points"
        );
    }
}
