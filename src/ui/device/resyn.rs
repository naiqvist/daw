//! The resynthesiser card — eight bands, and the six knobs around them.
//!
//! Normalized knob state here, natural values out as [`ParamEdit`]s. Ids,
//! ranges and defaults come from [`crate::params::resyn`] — the one table
//! this widget, `audio::resyn::ResynCore` and the app's edit routing all
//! read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ resyn ──────────────────────────────────┐
//! │                          band 4 · 8 bars│
//! │      ▁▃█▅▂▁▁▁      ← the band gains     │  ← hero
//! │   100      1k        10k                │
//! ├ band  gain    formant  shift ───────────┤
//! │ attack release warm    mix              │
//! └─────────────────────────────────────────┘
//! ```
//!
//! Eight cells will not fit one row at a readable width, so the footer is
//! TWO rows of four — `notes/20260827-device-card-layout.md` rule 2, and
//! its warning about keeping rows balanced rather than putting six beside
//! two.
//!
//! # One band at a time, behind a picker
//!
//! `eq.rs` set this pattern and argued it: eight bands with a row each
//! would be an unreadable strip, so the strip shows ONE band and a cell
//! picks which. The picture is where all eight are visible at once, which
//! is the other half of the trade.
//!
//! The picked band is UI state with no engine counterpart, so it rides
//! `DeviceInstance::page` and is threaded in and out by the app — rule 3
//! of the device-UI contract, and exactly the road `eq.rs` takes.
//!
//! # The hero is the bands, and nothing else
//!
//! Not a measured spectrum. The device's other five controls do things a
//! static picture cannot honestly show — a formant shift depends on the
//! envelope of whatever is playing, an attack and a release are times,
//! and the spectral shift moves a spectrum this card has never heard. A
//! plot of the band gains is the one thing that is true with no signal
//! present, so it is what gets drawn; the rest say their values in their
//! own cells.

use crate::params::resyn::{
    ATTACK, ATTACK_MAX_MS, ATTACK_MIN_MS, BAND_COUNT, BAND_EDGES, BAND_MAX_DB, BAND_MIN_DB, BAND0,
    FORMANT, FORMANT_MAX_ST, MIX, RELEASE, RELEASE_MAX_MS, RELEASE_MIN_MS, SHIFT, SHIFT_MAX_HZ,
    TABLE, WARM, WARM_NAMES,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// What the band picker prints.
const BAND_NAMES: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

/// Knob positions of one resynthesiser, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ResynUi {
    pub formant: f32,
    pub shift: f32,
    pub attack: f32,
    pub release: f32,
    pub warm: f32,
    pub mix: f32,
    pub bands: [f32; BAND_COUNT],
    /// Which band the gain cell edits. UI state with no engine
    /// counterpart — it rides `DeviceInstance::page`, per rule 3 of the
    /// device-UI contract.
    pub selected: usize,
}

impl Default for ResynUi {
    fn default() -> Self {
        Self {
            formant: resyn_norm(FORMANT, params::def(TABLE, FORMANT).default),
            shift: resyn_norm(SHIFT, params::def(TABLE, SHIFT).default),
            attack: resyn_norm(ATTACK, params::def(TABLE, ATTACK).default),
            release: resyn_norm(RELEASE, params::def(TABLE, RELEASE).default),
            warm: resyn_norm(WARM, params::def(TABLE, WARM).default),
            mix: resyn_norm(MIX, params::def(TABLE, MIX).default),
            bands: [resyn_norm(BAND0, 0.0); BAND_COUNT],
            selected: 0,
        }
    }
}

impl ResynUi {
    /// This state's knob position for a wire id, mutably — public, so the
    /// app can fill one from engine units by walking the table.
    pub fn slot_mut(&mut self, param: u32) -> Option<&mut f32> {
        self.slot(param)
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            FORMANT => &mut self.formant,
            SHIFT => &mut self.shift,
            ATTACK => &mut self.attack,
            RELEASE => &mut self.release,
            WARM => &mut self.warm,
            MIX => &mut self.mix,
            _ => {
                let index = param.checked_sub(BAND0)? as usize;
                self.bands.get_mut(index)?
            }
        })
    }

    /// The wire id of the band the picker is on.
    fn selected_id(&self) -> u32 {
        BAND0 + self.selected.min(BAND_COUNT - 1) as u32
    }
}

struct Spec {
    band: Param,
    gain: Param,
    formant: Param,
    shift: Param,
    attack: Param,
    release: Param,
    warm: Param,
    mix: Param,
}

/// The controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        band: Param::choice("band", BAND_NAMES),
        // BIPOLAR: a gain with no visible centre cannot be put back to
        // flat by eye.
        gain: default(Param::db("gain", BAND_MIN_DB, BAND_MAX_DB).bipolar(), BAND0),
        formant: default(
            Param::new(
                "formant",
                Mapping::Linear {
                    min: -FORMANT_MAX_ST,
                    max: FORMANT_MAX_ST,
                },
                Unit::Semitones,
            )
            .bipolar(),
            FORMANT,
        ),
        shift: default(
            Param::new(
                "shift",
                Mapping::Linear {
                    min: -SHIFT_MAX_HZ,
                    max: SHIFT_MAX_HZ,
                },
                Unit::Hz,
            )
            .bipolar(),
            SHIFT,
        ),
        // LOG, both: a time is a ratio.
        attack: default(
            Param::new(
                "attack",
                Mapping::Log {
                    min: ATTACK_MIN_MS,
                    max: ATTACK_MAX_MS,
                },
                Unit::Ms,
            ),
            ATTACK,
        ),
        release: default(
            Param::new(
                "release",
                Mapping::Log {
                    min: RELEASE_MIN_MS,
                    max: RELEASE_MAX_MS,
                },
                Unit::Ms,
            ),
            RELEASE,
        ),
        warm: default(Param::choice("warm", WARM_NAMES), WARM),
        mix: default(Param::percent("mix"), MIX),
    }
}

/// What the widget SHOWS for an engine-facing value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        MIX => value * 100.0,
        // Semitones, hertz, milliseconds, decibels and an index are all
        // already what a musician would say out loud.
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        MIX => value / 100.0,
        _ => value,
    };
    params::def(TABLE, param).clamp(raw)
}

/// One control by wire id. Every band shares the gain's description,
/// because every band IS the same control pointed at a different span.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        FORMANT => s.formant,
        SHIFT => s.shift,
        ATTACK => s.attack,
        RELEASE => s.release,
        WARM => s.warm,
        MIX => s.mix,
        _ => s.gain,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn resyn_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`resyn_value`], for a state stored in engine units.
pub fn resyn_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. The warm switch does.
pub fn resyn_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The two times do.
pub fn resyn_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn resyn_edits(state: &ResynUi) -> Vec<ParamEdit> {
    let mut edits: Vec<ParamEdit> = [
        (FORMANT, state.formant),
        (SHIFT, state.shift),
        (ATTACK, state.attack),
        (RELEASE, state.release),
        (WARM, state.warm),
        (MIX, state.mix),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: resyn_value(param, norm),
    })
    .collect();
    for (index, norm) in state.bands.iter().enumerate() {
        let param = BAND0 + index as u32;
        edits.push(ParamEdit {
            param,
            value: resyn_value(param, *norm),
        });
    }
    edits
}

/// The band drawn.
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// The hero: eight bars, one per band, on the frequency axis they cover.
fn bands(ui: &mut egui::Ui, theme: &Theme, state: &ResynUi) {
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
    // The axis is the band range's own, so a bar at its stop touches the
    // edge of the lane and one at rest sits exactly on the centre line.
    let span = BAND_MIN_DB.abs().max(BAND_MAX_DB);
    let y_of = |db: f32| mid - (db / span).clamp(-1.0, 1.0) * half;

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
            egui::pos2(field.left(), mid),
            egui::pos2(field.right(), mid),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    for index in 0..BAND_COUNT {
        let Some(lo) = BAND_EDGES.get(index) else {
            continue;
        };
        let Some(hi) = BAND_EDGES.get(index + 1) else {
            continue;
        };
        let db = resyn_value(BAND0 + index as u32, state.bands[index]);
        let x0 = x_of(*lo);
        let x1 = x_of(*hi);
        // A hairline of inset, so eight bars read as eight rather than as
        // one striped block.
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0 + 1.0, y_of(db.max(0.0))),
            egui::pos2(x1 - 1.0, y_of(db.min(0.0))),
        );
        // A band at rest still gets a mark, so flat reads as a row of
        // bars rather than as nothing drawn.
        let bar = if bar.height() < 1.0 {
            egui::Rect::from_min_max(
                egui::pos2(bar.left(), mid - 0.5),
                egui::pos2(bar.right(), mid + 0.5),
            )
        } else {
            bar
        };
        let picked = index == state.selected;
        painter.rect_filled(
            bar,
            0.0,
            if picked {
                theme.role_mod
            } else {
                theme.role_mod_dim
            },
        );
    }

    // The corner tag: which band the cell row is pointed at, since the
    // strip below shows one gain and the picture shows eight.
    let tag = format!("band {}", state.selected + 1);
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

/// The two rows of the footer. Balanced at four and four, which the
/// layout note asks for — one row of six beside one of two makes the card
/// as wide as the long row and gives the short one nothing.
fn rows(s: &Spec) -> [[(&Param, u32); 4]; 2] {
    [
        [
            (&s.band, u32::MAX),
            (&s.gain, BAND0),
            (&s.formant, FORMANT),
            (&s.shift, SHIFT),
        ],
        [
            (&s.attack, ATTACK),
            (&s.release, RELEASE),
            (&s.warm, WARM),
            (&s.mix, MIX),
        ],
    ]
}

/// The width the footer needs: the wider of the two rows, each summed
/// from the same per-cell figure it will be drawn at.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    rows(s)
        .iter()
        .map(|row| {
            let sum: f32 = row.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
            sum + ui.spacing().item_spacing.x * (row.len() - 1) as f32
        })
        .fold(0.0f32, f32::max)
}

/// A labelled cell is TWO `POLY_CELL_H` units tall, so two ROWS of cells
/// is four units — rule 1 of the layout note, which reads like an
/// off-by-one until you notice a cell is two lines.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = 2 * CELL_UNITS;

/// The card's only width contract.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the resynthesiser card. Returns the edits the user just made.
pub fn resyn_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ResynUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "resyn", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    // A READOUT. Eight bars could each be a handle, but
                    // that is rule 1's territory — one interaction per
                    // target, and the picker plus the gain cell already
                    // reach every one of them.
                    poly_widgets::CurveRegion::Plot => bands(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => {
                        // The row height comes off the footer with the
                        // GAPS TAKEN FIRST — rule 2. Dividing the height
                        // by the row count hands each row a share of the
                        // gaps as well, and the rows walk down the card.
                        let gap = theme.sp(space::XXS);
                        let count = 2.0f32;
                        let h = ((ui.available_height() - gap * (count - 1.0)) / count).max(1.0);
                        ui.spacing_mut().item_spacing.y = gap;
                        let mut picked = state.selected;
                        for row in rows(&s) {
                            ui.horizontal(|ui| {
                                for (param, id) in row {
                                    let w = cell_width(ui, theme, param);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(w, h),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_width(w);
                                            ui.set_height(h);
                                            if id == u32::MAX {
                                                // The picker: UI state, so
                                                // it emits no edit — the
                                                // app reads it off the
                                                // card and stores it on
                                                // the instance.
                                                let mut norm =
                                                    param.mapping.to_norm(state.selected as f32);
                                                if poly_widgets::labeled_cell_bar(
                                                    ui, theme, param, &mut norm, None,
                                                ) {
                                                    picked = param.mapping.to_value(norm).round()
                                                        as usize;
                                                }
                                                return;
                                            }
                                            // The gain cell edits whichever
                                            // band is picked.
                                            let id =
                                                if id == BAND0 { state.selected_id() } else { id };
                                            let Some(norm) = state.slot(id) else {
                                                return;
                                            };
                                            if poly_widgets::labeled_cell_bar(
                                                ui, theme, param, norm, None,
                                            ) {
                                                edits.push(ParamEdit {
                                                    param: id,
                                                    value: resyn_value(id, *norm),
                                                });
                                            }
                                        },
                                    );
                                }
                            });
                        }
                        state.selected = picked.min(BAND_COUNT - 1);
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

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = resyn_edits(&ResynUi::default());
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
                let value = resyn_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (resyn_value(row.id, 0.0) - row.min).abs() < 1e-2,
                "{} floor",
                row.name
            );
            assert!(
                (resyn_value(row.id, 1.0) - row.max).abs() < 1e-2,
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
                let back = resyn_value(row.id, resyn_norm(row.id, value));
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
        let fresh = ResynUi::default();
        for (id, norm) in [
            (FORMANT, fresh.formant),
            (SHIFT, fresh.shift),
            (ATTACK, fresh.attack),
            (RELEASE, fresh.release),
            (WARM, fresh.warm),
            (MIX, fresh.mix),
        ] {
            let want = params::def(TABLE, id).default;
            let got = resyn_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
        for index in 0..BAND_COUNT {
            let id = BAND0 + index as u32;
            assert!(
                resyn_value(id, fresh.bands[index]).abs() < 1e-2,
                "band {index} did not default flat"
            );
        }
    }

    #[test]
    fn only_the_switches_step_and_the_times_are_log() {
        assert!(resyn_is_discrete(WARM));
        for id in [FORMANT, SHIFT, MIX, BAND0] {
            assert!(!resyn_is_discrete(id), "id {id} should sweep");
        }
        for id in [ATTACK, RELEASE] {
            assert!(resyn_is_log(id), "id {id} should be log");
        }
        for id in [FORMANT, SHIFT, WARM, MIX, BAND0] {
            assert!(!resyn_is_log(id), "id {id} should not be log");
        }
    }

    /// Every band id reaches its own slot, so the picker cannot route a
    /// gain into the wrong band.
    #[test]
    fn each_band_id_reaches_its_own_slot() {
        let mut state = ResynUi::default();
        for index in 0..BAND_COUNT {
            let id = BAND0 + index as u32;
            if let Some(slot) = state.slot_mut(id) {
                *slot = index as f32 / 16.0;
            }
        }
        for index in 0..BAND_COUNT {
            assert!(
                (state.bands[index] - index as f32 / 16.0).abs() < 1e-6,
                "band {index} took the wrong value"
            );
        }
        // And an id past the last band reaches nothing.
        assert!(
            state.slot_mut(BAND0 + BAND_COUNT as u32).is_none(),
            "an id past the last band found a slot"
        );
    }

    /// The picker names the band the gain cell edits, one-indexed on the
    /// face and zero-indexed underneath.
    #[test]
    fn the_picker_points_at_the_band_it_names() {
        assert_eq!(BAND_NAMES.len(), BAND_COUNT);
        let mut state = ResynUi::default();
        for (index, name) in BAND_NAMES.iter().enumerate() {
            state.selected = index;
            assert_eq!(state.selected_id(), BAND0 + index as u32);
            assert_eq!(
                *name,
                (index + 1).to_string(),
                "the picker's face and its index disagree"
            );
        }
    }

    /// The footer reserves two lines for every row of cells — the layout
    /// note's formula, restated, plus enough left for a picture.
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
