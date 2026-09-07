//! STAB's device card — the house chord synth.
//!
//! Ids, ranges and the chord table come from [`crate::params::stab`].
//! The hero is a keyboard with the chord lit on it, voiced exactly as
//! the voices voice it, from the same function — so the picture cannot
//! show a chord the instrument is not playing. Four pages of cells:
//! the chord, the tone, the envelope, the grit.

use crate::params::stab as sp;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const PAGES: [(&str, &[u32]); 4] = [
    (
        "chord",
        &[
            sp::CHORD,
            sp::INVERSION,
            sp::OPEN,
            sp::OMIT,
            sp::OCTAVE,
            sp::STRUM,
        ],
    ),
    (
        "tone",
        &[
            sp::TONE,
            sp::DETUNE,
            sp::WIDTH,
            sp::CUTOFF,
            sp::RESO,
            sp::ENV,
            sp::FENV,
        ],
    ),
    ("shape", &[sp::ATTACK, sp::DECAY, sp::SUSTAIN, sp::RELEASE]),
    ("grit", &[sp::DRIVE, sp::CRUSH, sp::RATE, sp::LEVEL]),
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
const HEADER_ROWS: usize = 1;
/// The keyboard: three octaves, the played key at the start of the
/// second, so a dropped note has room below and a ninth above.
const OCTAVES: usize = 3;
const KEY_BASE: i32 = 12;
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn pages() -> usize {
    PAGES.len()
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StabUi {
    pub norms: [f32; 21],
}

impl Default for StabUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(sp::TABLE, id).default)
    }
}

impl StabUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let mut norms = [0.0; 21];
        for (i, slot) in norms.iter_mut().enumerate() {
            let id = i as u32;
            *slot = stab_norm(id, get(id));
        }
        Self { norms }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        self.norms.get_mut(param as usize)
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    fn value(&self, id: u32) -> f32 {
        stab_value(id, self.norms.get(id as usize).copied().unwrap_or(0.0))
    }
}

fn param_of(id: u32) -> Param {
    use crate::ui::device::{Mapping, Unit};
    let def = params::def(sp::TABLE, id);
    let linear = |name, unit| {
        Param::new(
            name,
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            unit,
        )
    };
    let log = |name, unit| {
        Param::new(
            name,
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            unit,
        )
    };
    let p = match id {
        sp::CHORD => Param::choice("chord", sp::CHORD_NAMES),
        sp::INVERSION => Param::choice("inv", sp::INVERSION_NAMES),
        sp::OPEN => Param::choice("voicing", sp::OPEN_NAMES),
        sp::OCTAVE => Param::choice("octave", sp::OCTAVE_NAMES),
        sp::OMIT => Param::choice("omit", sp::OMIT_NAMES),
        sp::STRUM => linear("strum", Unit::Ms),
        sp::TONE => Param::percent("tone"),
        sp::DETUNE => linear("detune", Unit::Cents),
        sp::WIDTH => Param::percent("width"),
        sp::ATTACK => linear("attack", Unit::Ms),
        sp::DECAY => log("decay", Unit::Ms),
        sp::SUSTAIN => Param::percent("sustain"),
        sp::RELEASE => log("release", Unit::Ms),
        sp::CUTOFF => log("cutoff", Unit::Hz),
        sp::RESO => log("reso", Unit::Plain),
        sp::ENV => linear("env", Unit::Plain),
        sp::FENV => log("env dec", Unit::Ms),
        sp::DRIVE => log("drive", Unit::Ratio),
        sp::CRUSH => linear("crush", Unit::Plain),
        sp::RATE => log("rate", Unit::Hz),
        _ => Param::percent("level"),
    };
    p.with_default(shown(id, def.default))
}

/// Engine units to the card's natural units.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        sp::OCTAVE => value + 2.0,
        sp::TONE | sp::WIDTH | sp::SUSTAIN => value * 100.0,
        sp::LEVEL => value / sp::LEVEL_MAX * 100.0,
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        sp::OCTAVE => value - 2.0,
        sp::TONE | sp::WIDTH | sp::SUSTAIN => value / 100.0,
        sp::LEVEL => value / 100.0 * sp::LEVEL_MAX,
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

pub fn stab_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn stab_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn stab_is_discrete(param: u32) -> bool {
    matches!(
        param,
        sp::CHORD | sp::INVERSION | sp::OPEN | sp::OCTAVE | sp::OMIT
    )
}

pub fn stab_is_log(param: u32) -> bool {
    matches!(
        param,
        sp::DECAY | sp::RELEASE | sp::CUTOFF | sp::RESO | sp::FENV | sp::DRIVE | sp::RATE
    )
}

pub fn stab_edits(state: &StabUi) -> Vec<ParamEdit> {
    let mut state = *state;
    sp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: stab_value(def.id, *norm),
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
    PAGES
        .iter()
        .map(|(_, row)| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// The chord the knobs voice, as semitones above the played key.
pub fn chord_notes(state: &StabUi) -> ([i32; sp::NOTES], usize) {
    sp::voicing(
        state.value(sp::CHORD).round().max(0.0) as usize,
        state.value(sp::INVERSION).round().max(0.0) as usize,
        state.value(sp::OPEN).round() >= 1.0,
        state.value(sp::OMIT).round().max(0.0) as usize,
    )
}

/// The chord's name, as a keyboard player says it: `Cm7 / E`.
pub fn chord_word(state: &StabUi) -> String {
    let chord = state.value(sp::CHORD).round().max(0.0) as usize;
    let name = sp::CHORD_NAMES.get(chord).copied().unwrap_or("?");
    let (notes, count) = chord_notes(state);
    let bass = notes.first().copied().unwrap_or(0).rem_euclid(12) as usize;
    let mut word = format!("C{name}");
    if count > 0 && bass != 0 {
        word.push_str(&format!(" / {}", NOTE_NAMES[bass]));
    }
    word
}

/// Draw the card. Returns the edits the user made.
pub fn stab_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut StabUi,
    page: &mut usize,
    voices: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "stab", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(
                ui,
                theme,
                None,
                0.0,
                HEADER_ROWS,
                FOOTER_ROWS,
                |ui, region| match region {
                    poly_widgets::CurveRegion::Header => rail(ui, theme, page),
                    poly_widgets::CurveRegion::Plot => keyboard(ui, theme, state, voices),
                    poly_widgets::CurveRegion::Footer => {
                        footer(ui, theme, state, *page, &mut edits);
                    }
                },
            );
        });
    });
    edits
}

fn rail(ui: &mut egui::Ui, theme: &Theme, page: &mut usize) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    *page = (*page).min(PAGES.len() - 1);
    let width = rect.width() / PAGES.len() as f32;
    for (index, (label, _)) in PAGES.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            egui::pos2(rect.left() + width * index as f32, rect.top()),
            egui::vec2(width, rect.height()),
        );
        let response = ui
            .interact(
                tab.shrink2(egui::vec2(1.0, 0.0)),
                ui.id().with(("stab_page", index)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if response.clicked() {
            *page = index;
        }
        let live = index == *page;
        if live {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.role_shape_dim);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.surface_raised);
        }
        ui.painter().text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            label.to_uppercase(),
            egui::FontId::proportional(font::MINI_LABEL),
            if live { theme.text } else { theme.text_muted },
        );
    }
}

/// Whether a semitone above C is a black key.
fn black(semitone: i32) -> bool {
    matches!(semitone.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// The hero: a keyboard with the chord on it.
fn keyboard(ui: &mut egui::Ui, theme: &Theme, state: &StabUi, voices: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let (notes, count) = chord_notes(state);
    let lit = |semitone: i32| notes[..count].contains(&(semitone - KEY_BASE));

    let inversion = state.value(sp::INVERSION).round().max(0.0) as usize;
    painter.text(
        rect.left_top() + egui::vec2(pad, pad),
        egui::Align2::LEFT_TOP,
        format!(
            "{}   {}   {}   {} NOTES   {:.0} HELD",
            chord_word(state),
            sp::INVERSION_NAMES
                .get(inversion)
                .copied()
                .unwrap_or("root")
                .to_uppercase(),
            if state.value(sp::OPEN).round() >= 1.0 {
                "OPEN"
            } else {
                "CLOSE"
            },
            count,
            voices
        ),
        mini.clone(),
        theme.text_muted,
    );
    let top = rect.top() + pad * 2.0 + font::MICRO_LABEL;
    let keys = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad, top),
        rect.max - egui::vec2(pad, pad),
    );
    if keys.height() < 10.0 {
        return;
    }
    let whites = (OCTAVES * 7) as f32;
    let white_w = keys.width() / whites;
    // White keys first, then the black ones over them.
    let mut white_index = 0;
    for semitone in 0..(OCTAVES as i32 * 12) {
        if black(semitone) {
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w - 1.0, keys.bottom()),
        );
        let fill = if lit(semitone) {
            theme.role_mod
        } else {
            theme.surface_raised
        };
        painter.rect_filled(key, 0.0, fill);
        if semitone == KEY_BASE {
            painter.text(
                egui::pos2(key.center().x, key.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                "C",
                mini.clone(),
                if lit(semitone) {
                    theme.text
                } else {
                    theme.text_muted
                },
            );
        }
        white_index += 1;
    }
    let black_h = keys.height() * 0.6;
    let mut white_index = 0;
    for semitone in 0..(OCTAVES as i32 * 12) {
        if !black(semitone) {
            white_index += 1;
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w - white_w * 0.3;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w * 0.6, keys.top() + black_h),
        );
        let fill = if lit(semitone) {
            theme.role_mod
        } else {
            theme.outline
        };
        painter.rect_filled(key, 0.0, fill);
    }
    crate::ui::hud::brackets(&painter, keys, egui::Stroke::new(1.0, theme.outline));
}

fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut StabUi,
    page: usize,
    edits: &mut Vec<ParamEdit>,
) {
    let Some((_, row)) = PAGES.get(page.min(PAGES.len() - 1)) else {
        return;
    };
    let height = ui.available_height().max(1.0);
    ui.horizontal(|ui| {
        let gap_x = ui.spacing().item_spacing.x;
        for (drawn, param) in row.iter().enumerate() {
            let spec = param_of(*param);
            let left = (row.len() - drawn) as f32;
            let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let Some(norm) = state.slot(*param) else {
                continue;
            };
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    if poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None) {
                        edits.push(ParamEdit {
                            param: *param,
                            value: stab_value(*param, *norm),
                        });
                    }
                },
            );
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 620.0;
    const MIN_W: f32 = 260.0;

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
    fn every_table_row_leaves_as_an_edit_and_is_on_exactly_one_page() {
        let edits = stab_edits(&StabUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = sp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
        let mut shown: Vec<u32> = PAGES
            .iter()
            .flat_map(|(_, row)| row.iter().copied())
            .collect();
        shown.sort_unstable();
        assert_eq!(shown, expect);
    }

    #[test]
    fn every_position_is_a_legal_engine_value_and_round_trips() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = stab_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
                let again = stab_value(def.id, stab_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (stab_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (stab_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table_and_the_chord_is_named() {
        let state = StabUi::default();
        for def in sp::TABLE {
            let value = stab_value(def.id, state.norms[def.id as usize]);
            assert!(
                (value - def.default).abs() <= def.default.abs() * 1e-3 + 1e-3,
                "{}: default {} drew as {value}",
                def.name,
                def.default
            );
        }
        assert_eq!(chord_word(&state), "Cm7");
        let mut state = StabUi::default();
        state.set_norm(sp::INVERSION, stab_norm(sp::INVERSION, 1.0));
        assert_eq!(
            chord_word(&state),
            "Cm7 / D#",
            "first inversion has the third in the bass"
        );
        assert!(stab_is_discrete(sp::CHORD) && !stab_is_discrete(sp::TONE));
        assert!(stab_is_log(sp::CUTOFF) && !stab_is_log(sp::WIDTH));
    }

    #[test]
    fn the_card_stays_inside_its_budget_and_fits_its_panel() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = StabUi::default();
        let mut page = 0;
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| stab_card(ui, &theme, &mut state, &mut page, 0.0))
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
                stab_card(&mut child, &theme, &mut state, &mut page, 0.0);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at {panel_w:.0} the card drew {:.0}",
                used.width()
            );
            assert!(used.width() > panel_w * 0.7);
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing_on_any_page() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = StabUi::default();
        let before = state;
        for page_index in 0..PAGES.len() {
            let mut page = page_index;
            for _ in 0..2 {
                let edits = frame(&ctx, |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| stab_card(ui, &theme, &mut state, &mut page, 1.0))
                        .inner
                });
                assert!(
                    edits.is_empty(),
                    "page {page_index} emitted {edits:?} at rest"
                );
            }
            assert_eq!(page, page_index);
        }
        assert_eq!(state, before);
    }
}
