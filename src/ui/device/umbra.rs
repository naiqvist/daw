//! Umbra's device card — the shadow.
//!
//! Ids, ranges and the STAGES table come from [`crate::params::umbra`] —
//! the card draws the same chain the engine runs, in the same order, from
//! the same array.
//!
//! # The chain, drawn
//!
//! Ten cells across, one per link, in signal order. Each fills as DEPTH
//! passes it, so the figure answers the only question a one-knob device
//! raises: *what is it doing right now?* A cell at a quarter is a stage a
//! quarter of the way in — the crossfade is visible rather than implied,
//! which is what stops the knob from feeling like a mystery.
//!
//! The travelling rule is DEPTH itself. Everything to its left is engaged
//! or arriving; everything to the right has not been reached.
//!
//! # Why not ten knobs
//!
//! Every stage has settings and none are on the face. That IS the device
//! — the ordering and the thresholds are the opinion — so the card's job
//! is not to expose them but to make the one knob legible. Showing the
//! chain is how a macro earns trust: you can see what you bought.

use crate::params::umbra as up;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// Six cells: how far, how long, how bright, how far out of the way, how
/// much, and how loud.
const CELLS: [u32; 6] = [
    up::DEPTH,
    up::MOTION,
    up::DECAY,
    up::COLOUR,
    up::MIX,
    up::OUT,
];

const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;

/// Knob positions, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UmbraUi {
    pub depth: f32,
    pub motion: f32,
    pub decay: f32,
    pub colour: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for UmbraUi {
    fn default() -> Self {
        let at = |id: u32| umbra_norm(id, params::def(up::TABLE, id).default);
        Self {
            depth: at(up::DEPTH),
            motion: at(up::MOTION),
            decay: at(up::DECAY),
            colour: at(up::COLOUR),
            mix: at(up::MIX),
            out: at(up::OUT),
        }
    }
}

impl UmbraUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| umbra_norm(id, get(id));
        Self {
            depth: at(up::DEPTH),
            motion: at(up::MOTION),
            decay: at(up::DECAY),
            colour: at(up::COLOUR),
            mix: at(up::MIX),
            out: at(up::OUT),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            up::DEPTH => &mut self.depth,
            up::MOTION => &mut self.motion,
            up::DECAY => &mut self.decay,
            up::COLOUR => &mut self.colour,
            up::MIX => &mut self.mix,
            up::OUT => &mut self.out,
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
    let def = params::def(up::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        up::DEPTH => with(Param::percent("depth")),
        up::MOTION => with(Param::percent("motion")),
        up::DECAY => with(Param::percent("decay")),
        // Bipolar percent, spelled out: `Param::percent` maps 0..100 and
        // cannot hold a negative, so a colour of -1 came back as 0 and
        // the dark half of the control did not exist.
        up::COLOUR => with(
            Param::new(
                "colour",
                crate::ui::device::Mapping::Linear {
                    min: -100.0,
                    max: 100.0,
                },
                crate::ui::device::Unit::Percent,
            )
            .bipolar(),
        ),
        up::MIX => with(Param::percent("mix")),
        _ => with(Param::db("out", up::OUT_MIN_DB, up::OUT_MAX_DB)),
    }
}

fn shown(param: u32, value: f32) -> f32 {
    match param {
        up::OUT => crate::dsp::arith::gain_to_db(value.max(1e-6)),
        _ => value * 100.0,
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        up::OUT => crate::dsp::arith::db_to_gain(value),
        _ => value / 100.0,
    };
    params::def(up::TABLE, param).clamp(raw)
}

pub fn umbra_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn umbra_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Nothing here snaps: every row is a sweep.
pub fn umbra_is_discrete(_param: u32) -> bool {
    false
}

pub fn umbra_edits(state: &UmbraUi) -> Vec<ParamEdit> {
    let mut state = *state;
    up::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: umbra_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The narrowest the CHAIN can be drawn: every link wide enough to print
/// its own name.
///
/// The card is six cells wide by the footer's reckoning and ten links
/// wide by the figure's, and the figure is the wider of the two — at the
/// footer's width "shimmer" printed through "haze". Rule three of the
/// layout note: a card that does not declare its footprint gets whatever
/// is going and crams.
fn chain_min_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    // The links are EQUAL columns, so the widest name sets all of them.
    // Summing the individual widths under-reserves by exactly the spread
    // between the longest name and the average, which is how "shimmer"
    // ended up two points over its share.
    // The links are EQUAL columns, so the widest name sets all of them.
    // Summing the individual widths under-reserves by exactly the spread
    // between the longest name and the average, which is how "shimmer"
    // ended up two points over its share.
    let widest = up::STAGES
        .iter()
        .map(|s| metrics::text_w(ui, s.name, font::MICRO_LABEL))
        .fold(0.0f32, f32::max)
        + theme.sp(space::XXS);
    // ...plus the matrix's row labels, which stand outside the grid.
    let rows = up::MACRO_NAMES
        .iter()
        .map(|n| metrics::text_w(ui, n, font::MICRO_LABEL))
        .fold(0.0f32, f32::max)
        + theme.sp(space::XS) * 2.0;
    widest * up::STAGES.len() as f32 + rows
}

pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    let cells = CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32;
    cells.max(chain_min_width(ui, theme))
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// How far into each link the shadow has travelled, from the shared
/// table — a pure function, so the figure is checkable without a frame.
pub fn engaged(depth: f32) -> [f32; up::STAGE_COUNT] {
    let mut out = [0.0f32; up::STAGE_COUNT];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = up::stage_amount(depth, i);
    }
    out
}

/// Draw the shadow's card. Returns the edits the user made.
///
/// `depth` is the engine's SMOOTHED depth, which is what the chain is
/// actually running on — the knob can be ahead of it for a few tens of
/// milliseconds, and the figure should show what is sounding.
pub fn umbra_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut UmbraUi,
    depth: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "umbra", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => chain(ui, theme, state, depth),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the REACH MATRIX — which macro is pulling on which link,
/// and how hard, right now.
///
/// Ten columns, one per link, in signal order. Four rows, one per macro,
/// in the order they sit on the face. A pin at a crossing means that knob
/// reaches that link; how bright and how large it is says how hard it is
/// pulling at this setting.
///
/// A row of bars would have said which stages are on. This says WHY —
/// and it is the only picture that can, because the device's whole idea
/// is that no link has a control of its own. Turn MOTION and five pins
/// brighten in a row; turn DEPTH and a column of four lights at once.
///
/// The pins are squares and the grid is ruled: a crossing either exists
/// or it does not, and a soft round dot would imply a continuum that the
/// matrix does not have. What varies inside a crossing is how much.
fn chain(ui: &mut egui::Ui, theme: &Theme, state: &UmbraUi, depth: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let depth = depth.clamp(0.0, 1.0);
    let values = [
        depth,
        umbra_value(up::MOTION, state.motion),
        umbra_value(up::DECAY, state.decay),
        umbra_value(up::COLOUR, state.colour),
    ];

    // Room for the row labels on the left and the link names below.
    let label_w = up::MACRO_NAMES
        .iter()
        .map(|n| metrics::text_w(ui, n, font::MICRO_LABEL))
        .fold(0.0f32, f32::max)
        + pad;
    let names_h = font::MICRO_LABEL + 3.0;
    let grid = egui::Rect::from_min_max(
        rect.min + egui::vec2(pad + label_w, font::MICRO_LABEL + pad * 2.0),
        rect.max - egui::vec2(pad, pad + names_h),
    );
    if grid.width() < 16.0 || grid.height() < 16.0 {
        return;
    }

    let cols = up::STAGE_COUNT.max(1);
    let rows = up::MACRO_COUNT.max(1);
    let cw = grid.width() / cols as f32;
    let rh = grid.height() / rows as f32;

    // ---- the reading -------------------------------------------------
    let engaged = (0..cols)
        .filter(|s| up::stage_amount(depth, *s) > 0.0)
        .count();
    painter.text(
        egui::pos2(rect.left() + pad, rect.top()),
        egui::Align2::LEFT_TOP,
        format!(
            "DEPTH {:>3.0}%   {engaged}/{} ENGAGED   LATENCY {} smp",
            depth * 100.0,
            up::STAGE_COUNT,
            up::LATENCY_SAMPLES,
        ),
        mini.clone(),
        theme.text_muted,
    );

    // ---- the ruled grid ----------------------------------------------
    for c in 0..=cols {
        let x = grid.left() + cw * c as f32;
        painter.line_segment(
            [egui::pos2(x, grid.top()), egui::pos2(x, grid.bottom())],
            egui::Stroke::new(1.0, theme.divider.gamma_multiply(0.5)),
        );
    }
    for r in 0..=rows {
        let y = grid.top() + rh * r as f32;
        painter.line_segment(
            [egui::pos2(grid.left(), y), egui::pos2(grid.right(), y)],
            egui::Stroke::new(1.0, theme.divider.gamma_multiply(0.5)),
        );
    }

    // ---- the pins ----------------------------------------------------
    for (m, name) in up::MACRO_NAMES.iter().enumerate() {
        let cy = grid.top() + rh * (m as f32 + 0.5);
        // The macro's name, lit while it is reaching anything.
        let pulling = (0..cols).any(|s| up::pull(values, depth, m, s) > 0.01);
        painter.text(
            egui::pos2(grid.left() - pad * 0.5, cy),
            egui::Align2::RIGHT_CENTER,
            *name,
            mini.clone(),
            if pulling { theme.text } else { theme.divider },
        );

        for s in 0..cols {
            let wired = up::REACH
                .get(m)
                .and_then(|row| row.get(s))
                .copied()
                .unwrap_or(0.0);
            if wired <= 0.0 {
                continue;
            }
            let cx = grid.left() + cw * (s as f32 + 0.5);
            let pull = up::pull(values, depth, m, s).clamp(0.0, 1.0);
            // The crossing EXISTS whatever the knobs say, drawn faint —
            // so the wiring is legible even where nothing is flowing.
            let full = (cw.min(rh) - 3.0).max(2.0);
            let idle = egui::Rect::from_center_size(
                egui::pos2(cx, cy),
                egui::vec2(full * 0.28, full * 0.28),
            );
            painter.rect_filled(idle, 0.0, theme.divider);
            if pull > 0.02 {
                let side = full * (0.3 + 0.7 * pull);
                let lit = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(side, side));
                // Depth is the gate and reads in the "what is running"
                // colour; the other three shape it and read in the
                // "what is written over it" colour, which is the same
                // pair every other card in this tree uses.
                let ink = if m == 0 {
                    theme.role_time
                } else {
                    theme.role_mod
                };
                painter.rect_filled(lit, 0.0, ink.gamma_multiply(0.25 + 0.75 * pull));
            }
        }
    }

    // ---- the link names ----------------------------------------------
    for (s, stage) in up::STAGES.iter().enumerate() {
        let cx = grid.left() + cw * (s as f32 + 0.5);
        let live = up::stage_amount(depth, s) > 0.0;
        painter.text(
            egui::pos2(cx, grid.bottom() + 2.0),
            egui::Align2::CENTER_TOP,
            stage.name,
            mini.clone(),
            if live { theme.text } else { theme.divider },
        );
    }

    // ---- where the depth is standing ---------------------------------
    let x = grid.left() + grid.width() * depth;
    painter.line_segment(
        [
            egui::pos2(x, grid.top() - 2.0),
            egui::pos2(x, grid.bottom() + 2.0),
        ],
        egui::Stroke::new(1.0, theme.role_time.gamma_multiply(0.8)),
    );

    crate::ui::hud::brackets(&painter, grid, egui::Stroke::new(1.0, theme.outline));
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut UmbraUi, edits: &mut Vec<ParamEdit>) {
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
                    value: umbra_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 560.0;
    const MIN_W: f32 = 240.0;

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
        let edits = umbra_edits(&UmbraUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = up::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in up::TABLE {
            for i in 0..=40 {
                let value = umbra_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!((umbra_value(def.id, 0.0) - def.min).abs() < span * 1e-3);
            assert!((umbra_value(def.id, 1.0) - def.max).abs() < span * 1e-3);
        }
    }

    /// Both ends of every control exist. TONE is bipolar and
    /// `Param::percent` maps 0..100, so the dark half of it silently did
    /// not — the round-trip is what noticed, and this keeps it noticing.
    #[test]
    fn value_and_norm_round_trip() {
        for def in up::TABLE {
            for i in 0..=40 {
                let value = umbra_value(def.id, i as f32 / 40.0);
                let again = umbra_value(def.id, umbra_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
            // ...including the far end of a bipolar row.
            let low = umbra_value(def.id, umbra_norm(def.id, def.min));
            assert!(
                (low - def.min).abs() < (def.max - def.min) * 1e-3,
                "{} cannot reach its floor: {} came back as {low}",
                def.name,
                def.min
            );
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in up::TABLE {
            let mut state = UmbraUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = umbra_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// THE FIGURE IS THE CHAIN: what the card fills is what the engine
    /// engages, from the same table, in the same order.
    #[test]
    fn the_drawn_chain_is_the_chain_that_runs() {
        // Nothing at the bottom but the first link, which starts there.
        let bottom = engaged(0.0);
        for (i, a) in bottom.iter().enumerate().skip(1) {
            assert_eq!(*a, 0.0, "stage {i} is lit at depth zero");
        }
        // Everything at the top.
        for (i, a) in engaged(1.0).iter().enumerate() {
            assert!(*a >= 1.0 - 1e-6, "stage {i} never arrives");
        }
        // Monotonic: turning the knob up never disengages anything.
        let mut previous = engaged(0.0);
        for step in 1..=50 {
            let now = engaged(step as f32 / 50.0);
            for (i, (a, b)) in now.iter().zip(previous.iter()).enumerate() {
                assert!(*a >= *b - 1e-6, "stage {i} went backwards at {step}/50");
            }
            previous = now;
        }
        // And the count only ever grows.
        let lit = |d: f32| engaged(d).iter().filter(|a| **a > 0.0).count();
        assert!(lit(0.0) < lit(0.5), "half way should have engaged more");
        assert!(lit(0.5) < lit(1.0), "the top should have engaged more");
        assert_eq!(lit(1.0), up::STAGE_COUNT, "the top must engage everything");
    }

    /// The declared width covers the CHAIN, not just the footer. Ten
    /// link names are wider than six cells, and the card asks for the
    /// larger of the two — otherwise "shimmer" prints through "haze".
    #[test]
    fn the_declared_width_covers_the_chain_as_well_as_the_cells() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        frame(&ctx, |ui| {
            let declared = face_width(ui, &theme);
            let chain = chain_min_width(ui, &theme);
            assert!(
                declared >= chain - 0.5,
                "the card asks for {declared:.0} pt and its chain needs {chain:.0}"
            );
            // Every link can print its own name inside its share.
            let col = declared / up::STAGES.len() as f32;
            for stage in up::STAGES {
                let w = metrics::text_w(ui, stage.name, font::MICRO_LABEL);
                assert!(
                    w <= col + 0.5,
                    "{} needs {w:.0} pt in a {col:.0} pt link",
                    stage.name
                );
            }
        });
    }

    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(plot > cell * 3.0, "the chain is left only {plot} pt");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UmbraUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| umbra_card(ui, &theme, &mut state, 0.58))
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

    /// Ten links have to fit the width six cells ask for — the chain is
    /// the widest thing on the card and it is not what declares the
    /// footprint.
    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UmbraUi::default();
        let floor = frame(&ctx, |ui| face_width(ui, &theme)) + theme.sp(space::SM) * 2.0;
        for panel_w in [floor.ceil(), floor * 1.4, floor * 2.0] {
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
                umbra_card(&mut child, &theme, &mut state, 0.58);
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
        let mut state = UmbraUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| umbra_card(ui, &theme, &mut state, 0.58))
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
        for step in 0..48 {
            for dy in [20.0f32, -20.0] {
                let mut state = UmbraUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 48.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| {
                    umbra_card(ui, &theme, &mut state, 0.58)
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
