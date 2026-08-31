//! Sigil's device card — the ring modulator.
//!
//! The hero is THE SEAL: the carrier drawn as a polar plot, one cycle
//! around a circle. The waveform the shape cell names IS the shape the
//! seal takes — sine is the smooth ring, square is the angular rune —
//! and how far the line stands off the base circle is the mix, read off
//! the same way the ear hears it. At mix zero the seal is an exact,
//! faint circle: the device is a bypass, and the display says so with
//! the same honesty the audio keeps (bit for bit).
//!
//! Nothing here moves on its own. A sigil does not rotate until the
//! signal does, and a card whose picture animates without the engine
//! would be decoration pretending to be telemetry.

use crate::params::sigil as sp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const CELLS: [u32; 3] = [sp::SHAPE, sp::FREQ, sp::MIX];
/// EVERY card in the rack is this tall — see tone.rs for why the room
/// goes to the readout rather than to a shorter card.
const CARD_H: f32 = control::DEVICE_TALL_H;
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
/// How far a fully cast seal stands off the base circle, as a fraction
/// of its radius.
const CAST: f32 = 0.34;
/// Points around the seal's circumference.
const SEAL_STEPS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SigilUi {
    pub shape: f32,
    pub freq: f32,
    pub mix: f32,
}

impl Default for SigilUi {
    fn default() -> Self {
        let at = |id: u32| sigil_norm(id, params::def(sp::TABLE, id).default);
        Self {
            shape: at(sp::SHAPE),
            freq: at(sp::FREQ),
            mix: at(sp::MIX),
        }
    }
}

impl SigilUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| sigil_norm(id, get(id));
        Self {
            shape: at(sp::SHAPE),
            freq: at(sp::FREQ),
            mix: at(sp::MIX),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            sp::SHAPE => &mut self.shape,
            sp::FREQ => &mut self.freq,
            sp::MIX => &mut self.mix,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    /// Which form the seal takes.
    pub fn shape_index(&self) -> usize {
        (sigil_value(sp::SHAPE, self.shape).round().max(0.0) as usize)
            .min(sp::SHAPE_NAMES.len() - 1)
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(sp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        sp::SHAPE => with(Param::choice("shape", sp::SHAPE_NAMES)),
        // LOG: the carrier is pitched, and chosen in octaves like any
        // other pitch — a linear sweep would spend nine tenths of its
        // travel above 800 Hz.
        sp::FREQ => with(Param::hz("freq", def.min, def.max)),
        _ => with(Param::percent("mix")),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        sp::MIX => value * 100.0,
        _ => value,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        sp::MIX => value / 100.0,
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

pub fn sigil_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn sigil_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Only the shape snaps.
pub fn sigil_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

pub fn sigil_edits(state: &SigilUi) -> Vec<ParamEdit> {
    let mut state = *state;
    sp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: sigil_value(def.id, *norm),
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

/// The carrier at one turn of the seal, in `-1..=1`.
///
/// A DRAWING of the shape, not the kernel's output — the oscillator's
/// band-limited table is an implementation detail, and what the seal
/// needs is the geometry the eye recognises as "sine" or "square".
pub fn seal_wave(shape: usize, phase: f32) -> f32 {
    let t = phase.rem_euclid(1.0);
    match shape {
        0 => (core::f32::consts::TAU * t).sin(),
        1 => 1.0 - 4.0 * (t - 0.5).abs(),
        _ => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

/// One point of the seal: the base circle pushed out by the cast wave.
/// The deviation from the base circle IS the mix — the picture reads
/// like the sound, and at mix zero the seal is a perfect circle.
fn seal_point(shape: usize, mix: f32, center: egui::Pos2, base: f32, angle: f32) -> egui::Pos2 {
    let (s, c) = angle.sin_cos();
    let r = base
        * (1.0 + CAST * mix.clamp(0.0, 1.0) * seal_wave(shape, angle / core::f32::consts::TAU));
    center + egui::vec2(c * r, s * r)
}

/// Draw the generator's card. Returns the edits the user made.
pub fn sigil_card(ui: &mut egui::Ui, theme: &Theme, state: &mut SigilUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "sigil", CARD_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => seal(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the carrier, one cycle around a circle, cast at the mix.
fn seal(ui: &mut egui::Ui, theme: &Theme, state: &SigilUi) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 16.0 || rect.height() < 16.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let shape = state.shape_index();
    let freq = sigil_value(sp::FREQ, state.freq);
    let mix = sigil_value(sp::MIX, state.mix);

    // The reading. A sigil's whole job is to carry a KNOWN frequency, so
    // the number is the point and the seal is the confirmation.
    painter.text(
        egui::pos2(rect.left() + pad, rect.top()),
        egui::Align2::LEFT_TOP,
        format!(
            "{}   {freq:>7.1} Hz   mix {:>3.0}%",
            sp::SHAPE_NAMES.get(shape).copied().unwrap_or("sine"),
            mix * 100.0
        ),
        mini.clone(),
        theme.text_muted,
    );

    // The seal, centred in what is left under the reading.
    let centre = egui::pos2(rect.center().x, rect.center().y + font::MICRO_LABEL * 0.5);
    let base = (rect.height() * 0.5 - pad * 2.0 - font::MICRO_LABEL)
        .min(rect.width() * 0.5 - pad * 2.0)
        .max(4.0);

    // The base circle: the seal at mix zero, always there as the
    // reference the cast stands off — the eye measures the mix against
    // it, which is exactly what the ear does against bypass.
    let cast = mix.clamp(0.0, 1.0) > 0.005;
    let ink = if cast {
        theme.role_shape
    } else {
        theme.divider
    };
    let stroke = egui::Stroke::new(1.0, ink);
    painter.circle_stroke(centre, base, stroke);

    if cast {
        // The seal itself: the carrier's one cycle, closed on itself.
        let steps = SEAL_STEPS;
        let points: Vec<egui::Pos2> = (0..steps)
            .map(|i| {
                let angle = core::f32::consts::TAU * i as f32 / steps as f32;
                seal_point(shape, mix, centre, base, angle)
            })
            .collect();
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.4, theme.role_shape.gamma_multiply(1.0)),
        ));

        // The compass: four strokes at the cardinal points, the rune's
        // own frame. Quieter than the seal — decoration must stay below
        // information, and these only anchor the circle's reading.
        let tick = theme.sp(space::XS) * 0.75;
        let r_out = base * (1.0 + CAST) + tick + 2.0;
        for k in 0..4 {
            let angle = core::f32::consts::FRAC_PI_2 * k as f32;
            let (s, c) = angle.sin_cos();
            let dir = egui::vec2(c, s);
            let a = centre + dir * r_out;
            let b = centre + dir * (r_out + tick);
            painter.line_segment([a, b], egui::Stroke::new(1.0, theme.divider));
        }
    } else {
        // The uncast seal: an exact circle, faint — the device is a
        // bypass, and the display admits it instead of pretending.
        painter.text(
            centre + egui::vec2(0.0, base + 8.0),
            egui::Align2::CENTER_TOP,
            "uncast",
            mini,
            theme.text_muted.gamma_multiply(0.7),
        );
    }

    crate::ui::hud::brackets(&painter, rect, egui::Stroke::new(1.0, theme.outline));
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut SigilUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);

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
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: sigil_value(*id, *slot),
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
        let edits = sigil_edits(&SigilUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = sp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = sigil_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (sigil_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (sigil_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = sigil_value(def.id, i as f32 / 40.0);
                let again = sigil_value(def.id, sigil_norm(def.id, value));
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
        for def in sp::TABLE {
            let mut state = SigilUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = sigil_value(def.id, norm);
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
        let cell = theme.sp(control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(CARD_H) - footer;
        assert!(plot > cell * 3.0, "the display is left only {plot} pt");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SigilUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| sigil_card(ui, &theme, &mut state))
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
        let mut state = SigilUi::default();
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
                sigil_card(&mut child, &theme, &mut state);
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
        let mut state = SigilUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| sigil_card(ui, &theme, &mut state))
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
        let cell_h = theme.sp(control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        for step in 0..48 {
            for dy in [20.0f32, -20.0] {
                let mut state = SigilUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 48.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| sigil_card(ui, &theme, &mut state));
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
mod seal_tests {
    use super::*;

    /// The seal draws the shape it names — bounded, distinct, periodic —
    /// and at mix zero every shape collapses to the exact circle, which
    /// is the picture's version of the bit-exact bypass.
    #[test]
    fn the_seal_wave_is_bounded_distinct_and_periodic() {
        let sample = |shape: usize| -> Vec<f32> {
            (0..64).map(|i| seal_wave(shape, i as f32 / 64.0)).collect()
        };
        for shape in 0..sp::SHAPE_NAMES.len() {
            let w = sample(shape);
            assert!(
                w.iter()
                    .all(|v| v.is_finite() && (-1.001..=1.001).contains(v)),
                "{} left the rails",
                sp::SHAPE_NAMES[shape]
            );
            let span = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(span > 0.3, "{} is flat", sp::SHAPE_NAMES[shape]);
            for i in 0..16 {
                let t = i as f32 / 16.0;
                assert!(
                    (seal_wave(shape, t) - seal_wave(shape, t + 1.0)).abs() < 1e-5,
                    "{} is not periodic",
                    sp::SHAPE_NAMES[shape]
                );
            }
        }
        for a in 0..sp::SHAPE_NAMES.len() {
            for b in (a + 1)..sp::SHAPE_NAMES.len() {
                let (x, y) = (sample(a), sample(b));
                let diff: f32 = x.iter().zip(y.iter()).map(|(p, q)| (p - q).abs()).sum();
                assert!(
                    diff > 1.0,
                    "{} and {} draw the same wave",
                    sp::SHAPE_NAMES[a],
                    sp::SHAPE_NAMES[b]
                );
            }
        }
    }

    /// The geometry of the promise: at mix zero every point of the seal
    /// is exactly on the base circle, and the deviation grows with the
    /// mix.
    #[test]
    fn the_seal_deviates_only_as_the_mix_asks() {
        let centre = egui::pos2(100.0, 100.0);
        let base = 50.0;
        for shape in 0..sp::SHAPE_NAMES.len() {
            for angle in (0..64).map(|i| core::f32::consts::TAU * i as f32 / 64.0) {
                let flat = seal_point(shape, 0.0, centre, base, angle);
                assert!(
                    (flat.distance(centre) - base).abs() < 1e-3,
                    "mix zero must be the exact circle"
                );
                let cast = seal_point(shape, 1.0, centre, base, angle);
                let r = cast.distance(centre);
                // The wave goes both ways around the base circle — the
                // seal breathes IN and OUT, never past the cast.
                assert!(
                    r >= base * (1.0 - CAST) * 0.999,
                    "the seal pushed too far in"
                );
                assert!(r <= base * (1.0 + CAST) * 1.001, "nor past the cast");
            }
        }
    }
}
