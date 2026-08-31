//! Gauge's device card — the meter.
//!
//! The one card in the tree that reports and never shapes. Its knobs do
//! not touch the signal at all: they decide how the MEASUREMENT is taken
//! — over how long, held for how long, against what floor — which is the
//! only kind of control a meter should have.
//!
//! # What the display says
//!
//! A scale in decibels with the peak riding it as a bright bar and the
//! RMS as a filled one behind, so the gap between them — the CREST — is a
//! visible distance rather than a subtraction. The held peak sits as a
//! line above both, and the number beside it is the one you came for.
//!
//! Underneath, the correlation, drawn as a needle on a scale from −1 to
//! +1 with the centre marked. It is the only reading here that can be
//! BAD rather than merely high: anything left of zero is a mix that
//! partly disappears when somebody sums it, and the scale says so by
//! colouring that half.

use crate::params::gauge as gp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const CELLS: [u32; 3] = [gp::WINDOW, gp::HOLD, gp::RANGE];
/// EVERY card in the rack is this tall. A utility with fewer controls is
/// tempting to draw shorter, and it looks wrong the moment it is stood
/// next to anything else — a chain of cards at two heights reads as a
/// mistake, not as economy. The room goes to the readout instead.
const CARD_H: f32 = control::DEVICE_TALL_H;
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;

/// What the engine last measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// The held peak, in dBFS.
    pub peak_db: f32,
    /// The windowed RMS, in dBFS.
    pub rms_db: f32,
    /// Stereo correlation, `-1..=1`.
    pub correlation: f32,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            peak_db: -120.0,
            rms_db: -120.0,
            correlation: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GaugeUi {
    pub window: f32,
    pub hold: f32,
    pub range: f32,
}

impl Default for GaugeUi {
    fn default() -> Self {
        let at = |id: u32| gauge_norm(id, params::def(gp::TABLE, id).default);
        Self {
            window: at(gp::WINDOW),
            hold: at(gp::HOLD),
            range: at(gp::RANGE),
        }
    }
}

impl GaugeUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| gauge_norm(id, get(id));
        Self {
            window: at(gp::WINDOW),
            hold: at(gp::HOLD),
            range: at(gp::RANGE),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            gp::WINDOW => &mut self.window,
            gp::HOLD => &mut self.hold,
            gp::RANGE => &mut self.range,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    /// The floor the scale is drawn to, in dB.
    pub fn floor_db(&self) -> f32 {
        let i = (gauge_value(gp::RANGE, self.range).round().max(0.0) as usize)
            .min(gp::RANGE_FLOORS.len() - 1);
        gp::RANGE_FLOORS.get(i).copied().unwrap_or(-48.0)
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(gp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        gp::RANGE => with(Param::choice("range", gp::RANGE_NAMES)),
        // LOG: an integration time is chosen in ratios — 10 ms and 30 ms
        // are as different as 300 ms and 900 ms.
        gp::WINDOW => with(Param::new(
            "window",
            crate::ui::device::Mapping::Log {
                min: def.min,
                max: def.max,
            },
            crate::ui::device::Unit::Ms,
        )),
        _ => with(Param::new(
            "hold",
            crate::ui::device::Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            crate::ui::device::Unit::Seconds,
        )),
    }
}

pub fn gauge_value(param: u32, norm: f32) -> f32 {
    params::def(gp::TABLE, param).clamp(param_of(param).value(norm))
}

pub fn gauge_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Only the range snaps.
pub fn gauge_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// A meter's window is chosen in ratios.
pub fn gauge_is_log(param: u32) -> bool {
    matches!(
        param_of(param).mapping,
        crate::ui::device::Mapping::Log { .. }
    )
}

pub fn gauge_edits(state: &GaugeUi) -> Vec<ParamEdit> {
    let mut state = *state;
    gp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: gauge_value(def.id, *norm),
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
    let cells = CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32;
    // Three cells is a narrow card, and the readouts above are wider than
    // any of them. Declared, per the layout note's third rule.
    let readout = metrics::mono_w(ui, "PEAK -120.0  RMS -120.0  CREST 00.0", font::MICRO_LABEL)
        + theme.sp(space::SM) * 2.0;
    cells.max(readout)
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// Where a decibel reading sits on a scale, `0..=1` from the floor up.
pub fn scale_at(db: f32, floor: f32) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    ((db - floor) / (0.0 - floor)).clamp(0.0, 1.0)
}

/// Draw the meter's card. Returns the edits the user made.
///
/// `now` is the engine's, because a card is rebuilt every frame.
pub fn gauge_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut GaugeUi,
    now: Reading,
    history: &crate::ui::device::scope::History,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "gauge", CARD_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => display(ui, theme, state, now, history),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the level scale, and the correlation under it.
fn display(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &GaugeUi,
    now: Reading,
    history: &crate::ui::device::scope::History,
) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let floor = state.floor_db();
    let crest = (now.peak_db - now.rms_db).max(0.0);

    // ---- the numbers -------------------------------------------------
    painter.text(
        egui::pos2(rect.left() + pad, rect.top()),
        egui::Align2::LEFT_TOP,
        format!(
            "PEAK {:>6.1}  RMS {:>6.1}  CREST {:>4.1}",
            now.peak_db.max(floor),
            now.rms_db.max(floor),
            crest,
        ),
        mini.clone(),
        theme.text_muted,
    );

    // Laid out in bands rather than by filling: the level bar took the
    // whole panel and read as a block, and its top tick printed through
    // the peak line. A meter wants a SLIM bar with its scale beside it.
    let line = font::MICRO_LABEL + 2.0;
    let head = font::MICRO_LABEL + pad * 2.0;
    let scale_y = rect.top() + head;
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad, scale_y + line),
        egui::pos2(
            rect.right() - pad,
            scale_y + line + (rect.height() * 0.30).max(14.0),
        ),
    );
    if bar.height() < 8.0 || bar.width() < 24.0 {
        return;
    }

    // ---- the level scale ---------------------------------------------
    painter.rect_filled(bar, 0.0, theme.surface_sunken);
    // The RMS, filled — the body of the sound.
    let rms_w = bar.width() * scale_at(now.rms_db, floor);
    painter.rect_filled(
        egui::Rect::from_min_size(bar.min, egui::vec2(rms_w, bar.height())),
        0.0,
        theme.role_time.gamma_multiply(0.55),
    );
    // The peak, as a line — the crest is the gap between the two, which
    // is a DISTANCE here rather than a subtraction you have to do.
    let peak_x = bar.left() + bar.width() * scale_at(now.peak_db, floor);
    painter.line_segment(
        [
            egui::pos2(peak_x, bar.top()),
            egui::pos2(peak_x, bar.bottom()),
        ],
        egui::Stroke::new(
            2.0,
            if now.peak_db > -0.1 {
                theme.danger
            } else {
                theme.role_mod
            },
        ),
    );
    // The ticks, every 6 dB down from unity.
    let mut db = 0.0f32;
    while db > floor {
        let x = bar.left() + bar.width() * scale_at(db, floor);
        painter.line_segment(
            [
                egui::pos2(x, bar.bottom() - 3.0),
                egui::pos2(x, bar.bottom()),
            ],
            egui::Stroke::new(1.0, theme.divider),
        );
        if (db as i32) % 12 == 0 {
            // ABOVE the bar, in its own row — printed inside, the top
            // tick landed under the peak line and neither could be read.
            painter.text(
                egui::pos2(x, scale_y),
                egui::Align2::CENTER_TOP,
                format!("{db:.0}"),
                mini.clone(),
                theme.text_muted,
            );
        }
        db -= 6.0;
    }
    painter.rect_stroke(
        bar,
        0.0,
        egui::Stroke::new(1.0, theme.outline),
        egui::StrokeKind::Middle,
    );

    // ---- the correlation ---------------------------------------------
    let strip = egui::Rect::from_min_max(
        egui::pos2(bar.left(), bar.bottom() + pad * 2.0),
        egui::pos2(
            bar.right(),
            bar.bottom() + pad * 2.0 + (rect.height() * 0.18).max(10.0),
        ),
    );
    if strip.height() >= 3.0 {
        painter.rect_filled(strip, 0.0, theme.surface_sunken);
        // The half that means trouble, marked before anything is in it.
        painter.rect_filled(
            egui::Rect::from_min_max(strip.min, egui::pos2(strip.center().x, strip.bottom())),
            0.0,
            theme.danger.gamma_multiply(0.14),
        );
        for t in [0.0f32, 0.5, 1.0] {
            let x = strip.left() + strip.width() * t;
            painter.line_segment(
                [egui::pos2(x, strip.top()), egui::pos2(x, strip.bottom())],
                egui::Stroke::new(1.0, theme.divider),
            );
        }
        let at = strip.left() + strip.width() * ((now.correlation.clamp(-1.0, 1.0) + 1.0) * 0.5);
        painter.line_segment(
            [
                egui::pos2(at, strip.top() - 2.0),
                egui::pos2(at, strip.bottom() + 2.0),
            ],
            egui::Stroke::new(
                2.0,
                if now.correlation < 0.0 {
                    theme.danger
                } else {
                    theme.role_time
                },
            ),
        );
        painter.text(
            egui::pos2(strip.left(), strip.bottom() + 1.0),
            egui::Align2::LEFT_TOP,
            "-1 OUT OF PHASE",
            mini.clone(),
            theme.text_muted,
        );
        painter.text(
            egui::pos2(strip.right(), strip.bottom() + 1.0),
            egui::Align2::RIGHT_TOP,
            format!("MONO +1   CORR {:>+5.2}", now.correlation),
            mini.clone(),
            theme.text_muted,
        );
    }

    // ---- the last few seconds ----------------------------------------
    //
    // A meter says what is happening; a trace says what HAS happened,
    // which is the question you are actually asking when you notice a
    // number and look up too late. It is the same history every other
    // metering card here keeps, read oldest first.
    let trace = egui::Rect::from_min_max(
        egui::pos2(bar.left(), strip.bottom() + font::MICRO_LABEL + pad * 1.5),
        egui::pos2(bar.right(), rect.bottom() - pad),
    );
    if trace.height() < 12.0 {
        return;
    }
    painter.rect_filled(trace, 0.0, theme.surface_sunken);
    let readings: Vec<crate::ui::device::scope::Reading> = history.oldest_first().collect();
    let n = readings.len().max(2);
    let x_of = |i: usize| trace.left() + trace.width() * i as f32 / (n - 1) as f32;
    let y_of = |db: f32| trace.bottom() - trace.height() * scale_at(db, floor);

    // The RMS as a filled body, the peak as a line over it — the same
    // pair as the bar above, so the two read as one instrument.
    let mut body: Vec<egui::Pos2> = vec![egui::pos2(trace.left(), trace.bottom())];
    body.extend(
        readings
            .iter()
            .enumerate()
            .map(|(i, r)| egui::pos2(x_of(i), y_of(r.reduction_db))),
    );
    body.push(egui::pos2(trace.right(), trace.bottom()));
    painter.add(egui::Shape::convex_polygon(
        body,
        theme.role_time.gamma_multiply(0.30),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::line(
        readings
            .iter()
            .enumerate()
            .map(|(i, r)| egui::pos2(x_of(i), y_of(r.level_db)))
            .collect(),
        egui::Stroke::new(1.2, theme.role_mod),
    ));
    // Unity, so a trace that touched the ceiling is obvious afterwards.
    let unity = y_of(0.0);
    painter.line_segment(
        [
            egui::pos2(trace.left(), unity),
            egui::pos2(trace.right(), unity),
        ],
        egui::Stroke::new(1.0, theme.danger.gamma_multiply(0.5)),
    );
    painter.rect_stroke(
        trace,
        0.0,
        egui::Stroke::new(1.0, theme.outline),
        egui::StrokeKind::Middle,
    );
    painter.text(
        egui::pos2(trace.left() + 2.0, trace.top() + 1.0),
        egui::Align2::LEFT_TOP,
        format!("{}s", crate::ui::device::scope::HISTORY / 60),
        mini,
        theme.text_muted,
    );
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut GaugeUi, edits: &mut Vec<ParamEdit>) {
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
                    value: gauge_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 520.0;
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
        let edits = gauge_edits(&GaugeUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = gp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in gp::TABLE {
            for i in 0..=40 {
                let value = gauge_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (gauge_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} floor",
                def.name
            );
            assert!(
                (gauge_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in gp::TABLE {
            for i in 0..=40 {
                let value = gauge_value(def.id, i as f32 / 40.0);
                let again = gauge_value(def.id, gauge_norm(def.id, value));
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
        for def in gp::TABLE {
            let mut state = GaugeUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = gauge_value(def.id, norm);
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
        let mut state = GaugeUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        gauge_card(
                            ui,
                            &theme,
                            &mut state,
                            Reading::default(),
                            &Default::default(),
                        )
                    })
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
        let mut state = GaugeUi::default();
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
                gauge_card(
                    &mut child,
                    &theme,
                    &mut state,
                    Reading::default(),
                    &Default::default(),
                );
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
        let mut state = GaugeUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| {
                        gauge_card(
                            ui,
                            &theme,
                            &mut state,
                            Reading::default(),
                            &Default::default(),
                        )
                    })
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
                let mut state = GaugeUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 48.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| {
                    gauge_card(
                        ui,
                        &theme,
                        &mut state,
                        Reading::default(),
                        &Default::default(),
                    )
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod scale_tests {
    use super::*;

    /// The scale places a reading where it belongs, and nowhere off it.
    #[test]
    fn the_scale_places_a_reading_and_stays_on_itself() {
        for floor in gp::RANGE_FLOORS {
            assert_eq!(scale_at(0.0, floor), 1.0, "unity is the top");
            assert_eq!(scale_at(floor, floor), 0.0, "the floor is the bottom");
            let half = scale_at(floor * 0.5, floor);
            assert!((half - 0.5).abs() < 1e-5, "half way should be half way");
            // Beyond either end it clamps rather than running off.
            assert_eq!(scale_at(12.0, floor), 1.0);
            assert_eq!(scale_at(floor - 40.0, floor), 0.0);
            assert_eq!(scale_at(f32::NAN, floor), 0.0, "a NaN must not draw");
            assert_eq!(scale_at(f32::NEG_INFINITY, floor), 0.0);
            // Monotonic: louder is never further left.
            let mut previous = 0.0f32;
            for i in 0..=40 {
                let db = floor + (0.0 - floor) * i as f32 / 40.0;
                let at = scale_at(db, floor);
                assert!(at >= previous - 1e-6, "the scale went backwards at {db}");
                previous = at;
            }
        }
    }

    /// Every range is reachable from the knob, and names its own floor.
    #[test]
    fn every_range_is_reachable_and_says_what_it_is() {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..=100 {
            let mut state = GaugeUi::default();
            state.range = i as f32 / 100.0;
            seen.insert(state.floor_db() as i32);
        }
        assert_eq!(seen.len(), gp::RANGE_FLOORS.len(), "a range is unreachable");
        for (name, floor) in gp::RANGE_NAMES.iter().zip(gp::RANGE_FLOORS.iter()) {
            assert_eq!(
                name.parse::<f32>().unwrap_or(f32::NAN),
                *floor,
                "the label and the floor disagree"
            );
        }
    }
}
