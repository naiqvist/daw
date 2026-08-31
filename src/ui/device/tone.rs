//! Tone's device card — the test-signal generator.
//!
//! A utility, so the card is a utility too: four cells and a picture of
//! the signal it is about to send. What matters on a generator is that
//! you can see WHAT you are sending before you send it, because the
//! answer arrives in a monitor chain a moment later.
//!
//! The preview is drawn from the shape and the level, and the frequency
//! cell greys out on the two noises — they do not have one, and a control
//! that still read "440 Hz" while making white noise would be lying.

use crate::params::tone as tp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const CELLS: [u32; 4] = [tp::SHAPE, tp::FREQ, tp::LEVEL, tp::MIX];
/// EVERY card in the rack is this tall. A utility with fewer controls is
/// tempting to draw shorter, and it looks wrong the moment it is stood
/// next to anything else — a chain of cards at two heights reads as a
/// mistake, not as economy. The room goes to the readout instead.
const CARD_H: f32 = control::DEVICE_TALL_H;
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
/// Cycles of the waveform the preview draws.
const PREVIEW_CYCLES: f32 = 2.6;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ToneUi {
    pub shape: f32,
    pub freq: f32,
    pub level: f32,
    pub mix: f32,
}

impl Default for ToneUi {
    fn default() -> Self {
        let at = |id: u32| tone_norm(id, params::def(tp::TABLE, id).default);
        Self {
            shape: at(tp::SHAPE),
            freq: at(tp::FREQ),
            level: at(tp::LEVEL),
            mix: at(tp::MIX),
        }
    }
}

impl ToneUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| tone_norm(id, get(id));
        Self {
            shape: at(tp::SHAPE),
            freq: at(tp::FREQ),
            level: at(tp::LEVEL),
            mix: at(tp::MIX),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            tp::SHAPE => &mut self.shape,
            tp::FREQ => &mut self.freq,
            tp::LEVEL => &mut self.level,
            tp::MIX => &mut self.mix,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    /// Which signal is selected.
    pub fn shape_index(&self) -> usize {
        (tone_value(tp::SHAPE, self.shape).round().max(0.0) as usize).min(tp::SHAPE_NAMES.len() - 1)
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(tp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        tp::SHAPE => with(Param::choice("shape", tp::SHAPE_NAMES)),
        // LOG: a test frequency is chosen in octaves, and a linear sweep
        // from 20 Hz would spend nine tenths of its travel above 2 kHz.
        tp::FREQ => with(Param::hz("freq", def.min, def.max)),
        tp::LEVEL => with(Param::db("level", tp::LEVEL_MIN_DB, tp::LEVEL_MAX_DB)),
        _ => with(Param::percent("mix")),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        tp::MIX => value * 100.0,
        tp::LEVEL => crate::dsp::arith::gain_to_db(value.max(1e-6)),
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        tp::MIX => value / 100.0,
        tp::LEVEL => crate::dsp::arith::db_to_gain(value),
        _ => value,
    };
    params::def(tp::TABLE, param).clamp(raw)
}

pub fn tone_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn tone_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Only the shape snaps.
pub fn tone_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

pub fn tone_edits(state: &ToneUi) -> Vec<ParamEdit> {
    let mut state = *state;
    tp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: tone_value(def.id, *norm),
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
    CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// One period of the selected shape at `phase` in `0..1`, in `-1..=1`.
///
/// A DRAWING of the waveform, not the kernel's output: the oscillator is
/// band-limited and its table is eighty kilobytes, and neither of those
/// facts helps anybody see whether they have selected a square.
pub fn wave_at(shape: usize, phase: f32) -> f32 {
    let t = phase.rem_euclid(1.0);
    match shape {
        0 => (core::f32::consts::TAU * t).sin(),
        1 => 1.0 - 4.0 * (t - 0.5).abs(),
        2 => 2.0 * t - 1.0,
        3 => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        // The two noises are drawn from a fixed hash, so the picture is
        // steady rather than crawling — it is showing you WHICH signal is
        // selected, not animating one.
        _ => {
            let mut h = ((t * 4_096.0) as u32).wrapping_mul(0x9E37_79B9);
            h ^= h >> 15;
            h = h.wrapping_mul(0x85EB_CA6B);
            h ^= h >> 13;
            let n = (h >> 8) as f32 / 8_388_608.0 - 1.0;
            // Pink is drawn calmer than white, which is the difference
            // anybody can actually see between two noises.
            if shape == tp::FIRST_NOISE + 1 {
                n * 0.55
            } else {
                n
            }
        }
    }
}

/// Draw the generator's card. Returns the edits the user made.
pub fn tone_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ToneUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "tone", CARD_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => preview(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the signal, at the level it will be sent at.
fn preview(ui: &mut egui::Ui, theme: &Theme, state: &ToneUi) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 16.0 || rect.height() < 16.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let shape = state.shape_index();
    let noise = shape >= tp::FIRST_NOISE;
    let level = tone_value(tp::LEVEL, state.level);
    let freq = tone_value(tp::FREQ, state.freq);

    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(pad, font::MICRO_LABEL + pad * 2.0),
        rect.max - egui::vec2(pad, pad),
    );
    if plot.width() < 8.0 || plot.height() < 8.0 {
        return;
    }

    // The reading. A generator's whole job is to be a KNOWN signal, so
    // the numbers are the point and the picture is the confirmation.
    painter.text(
        egui::pos2(plot.left(), rect.top()),
        egui::Align2::LEFT_TOP,
        if noise {
            format!(
                "{}   {:>6.1} dBFS",
                tp::SHAPE_NAMES.get(shape).copied().unwrap_or("sine"),
                crate::dsp::arith::gain_to_db(level.max(1e-6)),
            )
        } else {
            format!(
                "{}   {freq:>7.1} Hz   {:>6.1} dBFS",
                tp::SHAPE_NAMES.get(shape).copied().unwrap_or("sine"),
                crate::dsp::arith::gain_to_db(level.max(1e-6)),
            )
        },
        mini,
        theme.text_muted,
    );

    // Zero, and the rails the level sits between.
    let mid = plot.center().y;
    let half = plot.height() * 0.44;
    painter.line_segment(
        [egui::pos2(plot.left(), mid), egui::pos2(plot.right(), mid)],
        egui::Stroke::new(1.0, theme.divider),
    );
    for side in [-1.0f32, 1.0] {
        let y = mid + half * side;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0, theme.divider.gamma_multiply(0.6)),
        );
    }

    // The trace, drawn at the LEVEL it will be sent at — a preview that
    // ignored the level would be showing a signal nobody is about to
    // hear.
    let steps = (plot.width() as usize).clamp(16, 512);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let v = wave_at(shape, t * PREVIEW_CYCLES) * level;
            egui::pos2(
                plot.left() + plot.width() * t,
                mid - half * v.clamp(-1.0, 1.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.4, theme.role_shape),
    ));

    crate::ui::hud::brackets(&painter, plot, egui::Stroke::new(1.0, theme.outline));
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut ToneUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);
    let noise = state.shape_index() >= tp::FIRST_NOISE;

    ui.horizontal(|ui| {
        for (drawn, id) in CELLS.iter().enumerate() {
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let p = param_of(*id);
            let Some(slot) = state.slot(*id) else {
                continue;
            };
            let before = *slot;
            // A noise has no frequency, and the cell says so by going
            // out rather than by printing a number that means nothing.
            let dim = noise && *id == tp::FREQ;
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if dim {
                        ui.disable();
                    }
                    poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: tone_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 460.0;
    const MIN_W: f32 = 180.0;

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
    fn every_table_row_leaves_as_an_edit() {
        let edits = tone_edits(&ToneUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = tp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in tp::TABLE {
            for i in 0..=40 {
                let value = tone_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (tone_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (tone_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in tp::TABLE {
            for i in 0..=40 {
                let value = tone_value(def.id, i as f32 / 40.0);
                let again = tone_value(def.id, tone_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in tp::TABLE {
            let mut state = ToneUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = tone_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(CARD_H) - footer;
        assert!(plot > cell * 3.0, "the display is left only {plot} pt");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ToneUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| tone_card(ui, &theme, &mut state))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(CARD_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(h <= budget, "the card is {h:.0} pt tall, over {budget:.0}");
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ToneUi::default();
        let floor = frame(&ctx, |ui| face_width(ui, &theme)) + theme.sp(space::SM) * 2.0;
        for panel_w in [floor.ceil(), floor * 1.4, floor * 2.0] {
            let host = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(panel_w, theme.sp(CARD_H) + 8.0),
            );
            let used = frame(&ctx, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(host)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(host.width());
                tone_card(&mut child, &theme, &mut state);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt",
                used.width()
            );
            assert!(used.width() > panel_w * 0.7);
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ToneUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| tone_card(ui, &theme, &mut state))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// POINTER: every cell reachable, one control per drag, both ways.
    #[test]
    fn every_cell_is_reachable_and_a_drag_moves_only_one() {
        use crate::ui::device::probe;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let width = frame(&ctx, |ui| face_width(ui, &theme)).ceil() * 1.2;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, theme.sp(CARD_H)));
        let cell_h = theme.sp(crate::ui::tokens::control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        for step in 0..48 {
            for dy in [20.0f32, -20.0] {
                let mut state = ToneUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 48.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| tone_card(ui, &theme, &mut state));
                let touched: std::collections::BTreeSet<u32> =
                    edits.into_iter().flatten().map(|e| e.param).collect();
                assert!(touched.len() <= 1, "one drag at x={x:.0} moved {touched:?}");
                reached.extend(touched);
            }
        }
        let expect: std::collections::BTreeSet<u32> = CELLS.iter().copied().collect();
        assert_eq!(
            reached,
            expect,
            "a sweep never reached {:?}",
            expect.difference(&reached).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod wave_tests {
    use super::*;

    /// The preview draws the shape it names — bounded, and actually
    /// different from one another, which is the whole job of a picture
    /// whose only purpose is telling six signals apart.
    #[test]
    fn every_shape_draws_a_distinct_bounded_wave() {
        let sample = |shape: usize| -> Vec<f32> {
            (0..64).map(|i| wave_at(shape, i as f32 / 64.0)).collect()
        };
        for shape in 0..tp::SHAPE_NAMES.len() {
            let w = sample(shape);
            assert!(
                w.iter()
                    .all(|v| v.is_finite() && (-1.001..=1.001).contains(v)),
                "{} left the rails",
                tp::SHAPE_NAMES[shape]
            );
            let span = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(span > 0.3, "{} is flat", tp::SHAPE_NAMES[shape]);
        }
        // No two shapes draw the same picture.
        for a in 0..tp::SHAPE_NAMES.len() {
            for b in (a + 1)..tp::SHAPE_NAMES.len() {
                let (x, y) = (sample(a), sample(b));
                let diff: f32 = x.iter().zip(y.iter()).map(|(p, q)| (p - q).abs()).sum();
                assert!(
                    diff > 1.0,
                    "{} and {} draw the same wave",
                    tp::SHAPE_NAMES[a],
                    tp::SHAPE_NAMES[b]
                );
            }
        }
        // The waveforms repeat; a period later is the same sample.
        for shape in 0..tp::FIRST_NOISE {
            for i in 0..16 {
                let t = i as f32 / 16.0;
                assert!(
                    (wave_at(shape, t) - wave_at(shape, t + 1.0)).abs() < 1e-5,
                    "{} is not periodic",
                    tp::SHAPE_NAMES[shape]
                );
            }
        }
    }
}
