//! Tine's device card — the struck-resonator synth.
//!
//! Ids, ranges and the MODE TABLES come from [`crate::params::tine`]. The
//! card draws the same partials the voices ring on, from the same
//! functions, so the picture cannot describe a structure the instrument
//! is not.
//!
//! # The spectrum
//!
//! The hero is what the thing is made of: eight partials on a log axis,
//! with the HARMONIC SERIES drawn faintly behind them.
//!
//! That grid is the whole point. At MATERIAL zero every partial sits on
//! it and the instrument is a string; turn it up and you watch them walk
//! off — which is the difference between a note and a clang, and the one
//! thing about this synth that a list of knob values cannot tell you.
//!
//! Each partial's HEIGHT is how hard the strike excites it, straight from
//! `strike_gain` — so moving PLACE visibly kills partials, and striking
//! at the midpoint blanks every even one in front of you. Its WHISKER is
//! how long it rings. Nothing here is a curve chosen to look right; all
//! four readings are the functions the audio thread calls.

use crate::params::tine as tp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const CELLS: [u32; 9] = [
    tp::MATERIAL,
    tp::STRIKE,
    tp::PLACE,
    tp::DECAY,
    tp::BODY,
    tp::TONE,
    tp::SPREAD,
    tp::TUNE,
    tp::LEVEL,
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;
/// The spectrum's right edge, as a partial ratio. The bar's eighth
/// partial lands at 31.87, so this is the table's own reach.
/// The narrowest the ratio axis is ever drawn. A string's eight
/// partials stop at 8, a bar's reach past 30, and an axis fixed at the
/// bar's would leave the string in the left third — so the axis follows
/// the top partial and this is only its floor.
const RATIO_MIN_SPAN: f32 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TineUi {
    pub material: f32,
    pub strike: f32,
    pub place: f32,
    pub decay: f32,
    pub body: f32,
    pub tone: f32,
    pub spread: f32,
    pub tune: f32,
    pub level: f32,
}

impl Default for TineUi {
    fn default() -> Self {
        let at = |id: u32| tine_norm(id, params::def(tp::TABLE, id).default);
        Self {
            material: at(tp::MATERIAL),
            strike: at(tp::STRIKE),
            place: at(tp::PLACE),
            decay: at(tp::DECAY),
            body: at(tp::BODY),
            tone: at(tp::TONE),
            spread: at(tp::SPREAD),
            tune: at(tp::TUNE),
            level: at(tp::LEVEL),
        }
    }
}

impl TineUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| tine_norm(id, get(id));
        Self {
            material: at(tp::MATERIAL),
            strike: at(tp::STRIKE),
            place: at(tp::PLACE),
            decay: at(tp::DECAY),
            body: at(tp::BODY),
            tone: at(tp::TONE),
            spread: at(tp::SPREAD),
            tune: at(tp::TUNE),
            level: at(tp::LEVEL),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            tp::MATERIAL => &mut self.material,
            tp::STRIKE => &mut self.strike,
            tp::PLACE => &mut self.place,
            tp::DECAY => &mut self.decay,
            tp::BODY => &mut self.body,
            tp::TONE => &mut self.tone,
            tp::SPREAD => &mut self.spread,
            tp::TUNE => &mut self.tune,
            tp::LEVEL => &mut self.level,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(tp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        tp::TUNE => with(
            Param::new(
                "tune",
                crate::ui::device::Mapping::Linear {
                    min: def.min,
                    max: def.max,
                },
                crate::ui::device::Unit::Semitones,
            )
            .bipolar(),
        ),
        tp::TONE => with(
            Param::new(
                "tone",
                crate::ui::device::Mapping::Linear {
                    min: -100.0,
                    max: 100.0,
                },
                crate::ui::device::Unit::Percent,
            )
            .bipolar(),
        ),
        tp::LEVEL => with(Param::new(
            "level",
            crate::ui::device::Mapping::Linear {
                min: 0.0,
                max: 100.0,
            },
            crate::ui::device::Unit::Percent,
        )),
        _ => with(Param::percent(match id {
            tp::MATERIAL => "material",
            tp::STRIKE => "strike",
            tp::PLACE => "place",
            tp::DECAY => "decay",
            tp::BODY => "body",
            _ => "spread",
        })),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        tp::TUNE => value,
        tp::LEVEL => value / tp::LEVEL_MAX * 100.0,
        _ => value * 100.0,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        tp::TUNE => value,
        tp::LEVEL => value / 100.0 * tp::LEVEL_MAX,
        _ => value / 100.0,
    };
    params::def(tp::TABLE, param).clamp(raw)
}

pub fn tine_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn tine_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Nothing here snaps: every row is a sweep.
pub fn tine_is_discrete(_param: u32) -> bool {
    false
}

pub fn tine_edits(state: &TineUi) -> Vec<ParamEdit> {
    let mut state = *state;
    tp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: tine_value(def.id, *norm),
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

/// One partial, as the card draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partial {
    /// Where it sits, as a multiple of the fundamental.
    pub ratio: f32,
    /// How hard the strike excites it, `0..=1`.
    pub gain: f32,
    /// How long it rings, in seconds.
    pub ring_s: f32,
}

/// The structure the knobs describe — read from the same functions the
/// voices call, so the picture and the sound cannot disagree.
pub fn partials(state: &TineUi) -> [Partial; tp::MODES] {
    let material = tine_value(tp::MATERIAL, state.material);
    let place = tine_value(tp::PLACE, state.place);
    let decay = tine_value(tp::DECAY, state.decay);
    let mut out = [Partial {
        ratio: 1.0,
        gain: 0.0,
        ring_s: 0.0,
    }; tp::MODES];
    for (m, slot) in out.iter_mut().enumerate() {
        *slot = Partial {
            ratio: tp::ratio(material, m),
            gain: tp::strike_gain(place, m),
            ring_s: tp::ring_seconds(material, decay, m),
        };
    }
    out
}

/// Draw the synth's card. Returns the edits the user made.
///
/// `voices` is how many are ringing, from the engine.
pub fn tine_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut TineUi,
    voices: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "tine", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => spectrum(ui, theme, state, voices),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the partials, against the harmonic series they left.
fn spectrum(ui: &mut egui::Ui, theme: &Theme, state: &TineUi, voices: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let modes = partials(state);
    let material = tine_value(tp::MATERIAL, state.material);

    let names_h = font::MICRO_LABEL + 3.0;
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(pad, font::MICRO_LABEL + pad * 2.0),
        rect.max - egui::vec2(pad, pad + names_h),
    );
    if plot.width() < 16.0 || plot.height() < 16.0 {
        return;
    }
    // LOG in the ratio, because partials crowd upward and a linear axis
    // would put the first four in the leftmost eighth of the picture.
    let span = modes.iter().map(|p| p.ratio).fold(RATIO_MIN_SPAN, f32::max) * 1.08;
    let x_of = |ratio: f32| {
        let t = (ratio.max(1.0).ln() / span.ln()).clamp(0.0, 1.0);
        plot.left() + plot.width() * t
    };

    // ---- the reading -------------------------------------------------
    let longest = modes.first().map(|p| p.ring_s).unwrap_or(0.0);
    let nodes = modes.iter().filter(|p| p.gain <= 0.01).count();
    painter.text(
        egui::pos2(plot.left(), rect.top()),
        egui::Align2::LEFT_TOP,
        format!(
            "{}   RING {longest:>5.2}s   {nodes} NODE{}   {:.0} VOICES",
            if material < 0.25 {
                "string"
            } else if material < 0.7 {
                "struck"
            } else {
                "bar"
            },
            if nodes == 1 { "" } else { "S" },
            voices,
        ),
        mini.clone(),
        theme.text_muted,
    );

    // ---- the harmonic series, drawn faintly behind ---------------------
    //
    // The grid the partials LEFT. Without it, an inharmonic bank is just
    // eight lines in arbitrary places; with it, you can see them go.
    //
    // A log axis crowds the high harmonics into a picket fence, so a line
    // is drawn only where there is room for it to READ as a line — and
    // the octaves, which are the ones you count by, are drawn brighter.
    let mut last_x = f32::NEG_INFINITY;
    for n in 1..=span as i32 {
        let x = x_of(n as f32);
        let octave = (n as u32).is_power_of_two();
        if !octave && x - last_x < 7.0 {
            continue;
        }
        last_x = x;
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(
                1.0,
                theme
                    .divider
                    .gamma_multiply(if octave { 0.9 } else { 0.45 }),
            ),
        );
    }
    // Quarter rules, so a partial's height is a quantity and not a mood.
    for q in 1..4 {
        let y = plot.bottom() - plot.height() * 0.86 * q as f32 / 4.0;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0, theme.divider.gamma_multiply(0.3)),
        );
    }

    // ---- the partials --------------------------------------------------
    for (m, partial) in modes.iter().enumerate() {
        let x = x_of(partial.ratio);
        let height = plot.height() * partial.gain.clamp(0.0, 1.0) * 0.86;
        let top = plot.bottom() - height;
        let lit = partial.gain > 0.01;
        // A partial the strike cannot reach is still THERE — it is drawn
        // as a stub, because "this one is silent right now" and "this one
        // does not exist" are different facts.
        if !lit {
            painter.line_segment(
                [
                    egui::pos2(x, plot.bottom()),
                    egui::pos2(x, plot.bottom() - 6.0),
                ],
                egui::Stroke::new(1.8, theme.role_mod.gamma_multiply(0.35)),
            );
            continue;
        }
        // The TAIL: the partial's own decay, drawn to the right at the
        // rate it actually dies at. Ring time is the whole point of a
        // modal bank and a bar cannot show it; this can, and the eye
        // reads the envelope of eight tails as the sound's shape.
        let ring = if longest > 1e-6 {
            (partial.ring_s / longest).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let reach = (plot.width() * 0.17 * ring).min(plot.right() - x);
        if reach > 2.0 {
            let steps = 12;
            let mut path = Vec::with_capacity(steps + 3);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                // Exponential, and the same one the voice uses: t60 is
                // 60 dB down, so the drawn tail ends where it is gone.
                let fall = (-t * 6.9).exp();
                path.push(egui::pos2(x + reach * t, plot.bottom() - height * fall));
            }
            path.push(egui::pos2(x + reach, plot.bottom()));
            path.push(egui::pos2(x, plot.bottom()));
            painter.add(egui::Shape::convex_polygon(
                path,
                theme.role_time.gamma_multiply(0.16),
                egui::Stroke::NONE,
            ));
        }
        painter.line_segment(
            [egui::pos2(x, plot.bottom()), egui::pos2(x, top)],
            egui::Stroke::new(1.8, theme.role_mod),
        );
        painter.line_segment(
            [egui::pos2(x - 2.5, top), egui::pos2(x + 2.5, top)],
            egui::Stroke::new(1.0, theme.role_time),
        );
        // The first, third and fifth get a number, so the axis is
        // readable without labelling all eight into a smear.
        if m % 2 == 0 {
            let (anchor, at) = if m == 0 {
                (egui::Align2::LEFT_TOP, plot.left())
            } else {
                (egui::Align2::CENTER_TOP, x)
            };
            painter.text(
                egui::pos2(at, plot.bottom() + 2.0),
                anchor,
                format!("{:.1}", partial.ratio),
                mini.clone(),
                theme.text_muted,
            );
        }
    }

    crate::ui::hud::brackets(&painter, plot, egui::Stroke::new(1.0, theme.outline));
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut TineUi, edits: &mut Vec<ParamEdit>) {
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
                    value: tine_value(*id, *slot),
                });
            }
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
    fn every_table_row_leaves_as_an_edit() {
        let edits = tine_edits(&TineUi::default());
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
                let value = tine_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (tine_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (tine_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in tp::TABLE {
            for i in 0..=40 {
                let value = tine_value(def.id, i as f32 / 40.0);
                let again = tine_value(def.id, tine_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            let low = tine_value(def.id, tine_norm(def.id, def.min));
            assert!(
                (low - def.min).abs() < (def.max - def.min) * 1e-3,
                "{} cannot reach its floor",
                def.name
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in tp::TABLE {
            let mut state = TineUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = tine_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// THE PICTURE IS THE STRUCTURE: what the card draws is what the
    /// voices ring on, because both call the same functions.
    #[test]
    fn the_drawn_partials_are_the_ones_that_sound() {
        let mut state = TineUi::default();

        // A string: every partial on a whole multiple.
        state.material = tine_norm(tp::MATERIAL, 0.0);
        let drawn = partials(&state);
        for (m, p) in drawn.iter().enumerate() {
            assert_eq!(p.ratio, tp::HARMONIC[m], "partial {m} is not the string's");
        }
        // A bar: none of them are, past the first.
        state.material = tine_norm(tp::MATERIAL, 1.0);
        let drawn = partials(&state);
        for (m, p) in drawn.iter().enumerate() {
            assert_eq!(p.ratio, tp::INHARMONIC[m], "partial {m} is not the bar's");
        }

        // PLACE really silences partials, and the card shows it: struck
        // at the midpoint every even partial is dead.
        state.place = tine_norm(tp::PLACE, 0.5);
        let drawn = partials(&state);
        for m in [1usize, 3, 5, 7] {
            assert!(drawn[m].gain < 1e-4, "partial {} should be silent", m + 1);
        }
        for m in [0usize, 2, 4, 6] {
            assert!(drawn[m].gain > 0.99, "partial {} should be full", m + 1);
        }

        // Ring times fall with mode number, always.
        state.decay = tine_norm(tp::DECAY, 0.7);
        let drawn = partials(&state);
        for m in 1..tp::MODES {
            assert!(
                drawn[m].ring_s < drawn[m - 1].ring_s,
                "partial {} rings longer than {}",
                m + 1,
                m
            );
        }
        // And more DECAY rings longer, at every partial.
        let short = {
            state.decay = tine_norm(tp::DECAY, 0.1);
            partials(&state)
        };
        let long = {
            state.decay = tine_norm(tp::DECAY, 0.9);
            partials(&state)
        };
        for m in 0..tp::MODES {
            assert!(long[m].ring_s > short[m].ring_s, "partial {m}");
        }
    }

    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(plot > cell * 3.0, "the spectrum is left only {plot} pt");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = TineUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| tine_card(ui, &theme, &mut state, 3.0))
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
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = TineUi::default();
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
                tine_card(&mut child, &theme, &mut state, 3.0);
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
        let mut state = TineUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| tine_card(ui, &theme, &mut state, 0.0))
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
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, theme.sp(control::DEVICE_TALL_H)),
        );
        let cell_h = theme.sp(crate::ui::tokens::control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        for step in 0..64 {
            for dy in [20.0f32, -20.0] {
                let mut state = TineUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 64.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| {
                    tine_card(ui, &theme, &mut state, 3.0)
                });
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
