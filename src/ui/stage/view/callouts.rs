//! Callouts over the tray: the trig menu and the p-lock editor.
//!
//! The trig menu is a bubble anchored on the trig under the sequencer's
//! cursor, its tail pointing at it — above the trig where there is room,
//! below where there is not. Two pages: LOCKS, every parameter the trig
//! can lock as a rail with the knob's mark and the lock's value in alert;
//! TRIG, the verbs. The p-lock editor is a panel over the field with
//! three zones the keys move between — parameters, graphs of each
//! selected parameter's value per tick, and the controls of the
//! algorithm being applied — and a picker for that algorithm.

use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::chain;
use crate::ui::stage::plock_editor::{Algorithm, Focus};
use crate::ui::stage::trig_menu::{MenuRow, Page, TrigAction};
use eframe::egui;

/// The bubble's width.
/// @tune 160..480 px
const BUBBLE_W: f32 = 280.0;
/// One row of the bubble.
/// @tune 12..32 px
const ROW_H: f32 = 18.0;
/// The bubble's head.
const HEAD_H: f32 = 22.0;
/// The tail's length.
/// @tune 4..40 px
const TAIL: f32 = 14.0;
const INSET: f32 = 8.0;
const TYPE_PX: f32 = 12.0;
/// The editor's inset from the field's edges.
/// @tune 0..120 px
const EDITOR_INSET: f32 = 40.0;

impl super::super::Stage {
    pub(super) fn draw_callouts(
        &self,
        painter: &egui::Painter,
        whole: egui::Rect,
        anchor: Option<egui::Rect>,
    ) {
        self.draw_trig_menu(painter, whole, anchor);
        self.draw_plock_editor(painter, whole);
    }

    fn draw_trig_menu(
        &self,
        painter: &egui::Painter,
        whole: egui::Rect,
        anchor: Option<egui::Rect>,
    ) {
        let Some(menu) = self.trig_menu.as_ref() else {
            return;
        };
        let Some(anchor) = anchor else {
            return;
        };
        let Some((_, trig)) = self.trig_under_cursor() else {
            return;
        };
        let Some((_, step, rows)) = self.menu_rows_under_cursor() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;

        // What the page lists.
        enum Line {
            Verb(&'static str),
            Lock {
                name: String,
                value: String,
                fraction: f32,
                knob: f32,
                locked: bool,
            },
            Slice {
                standing: u8,
                count: usize,
                locked: bool,
            },
            Trig,
        }
        let lines: Vec<Line> = match menu.page {
            Page::Trig => TrigAction::ALL
                .iter()
                .map(|a| Line::Verb(a.label(&trig)))
                .collect(),
            Page::Locks => rows
                .iter()
                .map(|row| match row {
                    MenuRow::Trig => Line::Trig,
                    MenuRow::Slice(s) => Line::Slice {
                        standing: s.standing(),
                        count: s.count,
                        locked: s.lock.is_some(),
                    },
                    MenuRow::Param(l) => Line::Lock {
                        name: format!("{}{}", l.prefix, l.def.name),
                        value: chain::format_param(l.def, l.label, l.standing()),
                        fraction: l.fraction(l.standing()),
                        knob: l.fraction(l.knob),
                        locked: l.lock.is_some(),
                    },
                })
                .collect(),
        };
        let shown = lines.len().saturating_sub(menu.offset).min(12);
        let overflow = lines.len() > menu.offset + shown;
        let h = HEAD_H + (shown + usize::from(overflow)) as f32 * crate::tune!(ROW_H) + INSET * 2.0;
        let w = crate::tune!(BUBBLE_W);
        // Placement: above the trig when it fits, else below; kept in the window.
        let tail = crate::tune!(TAIL);
        let above = anchor.min.y - tail - h >= whole.min.y + 4.0;
        let top = if above {
            anchor.min.y - tail - h
        } else {
            anchor.max.y + tail
        };
        let left = (anchor.center().x - w * 0.5).clamp(whole.min.x + 4.0, whole.max.x - w - 4.0);
        let bubble = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(w, h));
        painter.rect_filled(bubble, 0.0, c.ground);
        chassis::frame(painter, bubble, true);
        // The tail, home to the trig.
        let tx = anchor
            .center()
            .x
            .clamp(bubble.min.x + 8.0, bubble.max.x - 8.0)
            .round()
            - 0.5;
        let (ty0, ty1) = if above {
            (bubble.max.y, anchor.min.y)
        } else {
            (bubble.min.y, anchor.max.y)
        };
        painter.line_segment(
            [egui::pos2(tx, ty0), egui::pos2(tx, ty1)],
            egui::Stroke::new(1.0, c.chassis),
        );

        // The head: the page, the step.
        let inner = bubble.shrink(INSET);
        let hy = inner.min.y + HEAD_H * 0.5 - 2.0;
        let mut x = inner.min.x;
        for page in [Page::Locks, Page::Trig] {
            let word = match page {
                Page::Locks => "LOCKS",
                Page::Trig => "TRIG",
            };
            let on = page == menu.page;
            painter.text(
                egui::pos2(x, hy),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                if on { c.bright } else { c.dim },
            );
            if on {
                painter.line_segment(
                    [
                        egui::pos2(x, hy + 8.0),
                        egui::pos2(x + word.len() as f32 * ch, hy + 8.0),
                    ],
                    egui::Stroke::new(1.0, c.alert),
                );
            }
            x += (word.len() as f32 + 2.0) * ch;
        }
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            format!("step {:02}", step + 1),
            font.clone(),
            c.label,
        );

        // The rows, from the menu's offset, the cursor's on select.
        let rows_top = inner.min.y + HEAD_H;
        for (i, line) in lines.iter().enumerate().skip(menu.offset).take(shown) {
            let y = rows_top + (i - menu.offset) as f32 * crate::tune!(ROW_H);
            let rect = egui::Rect::from_min_max(
                egui::pos2(inner.min.x, y),
                egui::pos2(inner.max.x, y + crate::tune!(ROW_H)),
            );
            let on = i == menu.row;
            if on {
                painter.rect_filled(rect.expand2(egui::vec2(3.0, 0.0)), 0.0, c.select);
            }
            let cy = rect.center().y;
            match line {
                Line::Verb(word) => {
                    painter.text(
                        egui::pos2(rect.min.x, cy),
                        egui::Align2::LEFT_CENTER,
                        *word,
                        font.clone(),
                        if on { c.bright } else { c.fg },
                    );
                }
                Line::Trig => {
                    painter.text(
                        egui::pos2(rect.min.x, cy),
                        egui::Align2::LEFT_CENTER,
                        "trig",
                        font.clone(),
                        c.label,
                    );
                    painter.text(
                        egui::pos2(rect.max.x, cy),
                        egui::Align2::RIGHT_CENTER,
                        format!(
                            "vel {} · {}",
                            trig.velocity,
                            if trig.muted { "muted" } else { "on" }
                        ),
                        font.clone(),
                        c.fg,
                    );
                }
                Line::Slice {
                    standing,
                    count,
                    locked,
                } => {
                    painter.text(
                        egui::pos2(rect.min.x, cy),
                        egui::Align2::LEFT_CENTER,
                        "slice",
                        font.clone(),
                        if on { c.fg } else { c.dim },
                    );
                    painter.text(
                        egui::pos2(rect.max.x, cy),
                        egui::Align2::RIGHT_CENTER,
                        format!("{:02}/{:02}", standing + 1, count),
                        font.clone(),
                        if *locked { c.alert } else { c.fg },
                    );
                }
                Line::Lock {
                    name,
                    value,
                    fraction,
                    knob,
                    locked,
                } => {
                    painter.text(
                        egui::pos2(rect.min.x, cy - 2.0),
                        egui::Align2::LEFT_CENTER,
                        name,
                        font.clone(),
                        if on { c.fg } else { c.dim },
                    );
                    painter.text(
                        egui::pos2(rect.max.x, cy - 2.0),
                        egui::Align2::RIGHT_CENTER,
                        value,
                        font.clone(),
                        if *locked {
                            c.alert
                        } else if on {
                            c.bright
                        } else {
                            c.fg
                        },
                    );
                    // The rail: the knob's mark, and the standing value.
                    let ry = rect.max.y - 3.0;
                    painter.line_segment(
                        [egui::pos2(rect.min.x, ry), egui::pos2(rect.max.x, ry)],
                        egui::Stroke::new(1.0, c.rule),
                    );
                    let kx = rect.min.x + rect.width() * knob;
                    painter.line_segment(
                        [egui::pos2(kx, ry - 3.0), egui::pos2(kx, ry + 3.0)],
                        egui::Stroke::new(1.0, c.edge),
                    );
                    let vx = rect.min.x + rect.width() * fraction;
                    painter.rect_filled(
                        egui::Rect::from_center_size(egui::pos2(vx, ry), egui::vec2(5.0, 5.0)),
                        0.0,
                        if *locked { c.alert } else { c.chassis },
                    );
                }
            }
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("trig-menu", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    c.alert,
                );
            }
        }
        if overflow {
            painter.text(
                egui::pos2(inner.max.x, inner.max.y),
                egui::Align2::RIGHT_BOTTOM,
                format!("{} more", lines.len() - menu.offset - shown),
                font,
                c.dim,
            );
        }
    }

    fn draw_plock_editor(&self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(editor) = self.plock_editor.as_ref() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let panel = whole.shrink(crate::tune!(EDITOR_INSET));
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET + 4.0);

        // The head: what is being edited, the algorithm, the zone.
        let hy = inner.min.y + HEAD_H * 0.5;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "PLOCK",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(inner.min.x + 7.0 * ch, hy),
            egui::Align2::LEFT_CENTER,
            format!(
                "pat {:02}  tr {:02}  {} ticks",
                editor.pattern.0,
                editor.track + 1,
                editor.ticks.len()
            ),
            font.clone(),
            c.fg,
        );
        let zone = match editor.focus {
            Focus::Parameters => "PARAMETERS",
            Focus::Graphs => "GRAPHS",
            Focus::Controls => "CONTROLS",
        };
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            format!("{}  ·  {}", editor.algorithm.label(), zone),
            font.clone(),
            c.dim,
        );
        if editor.dirty {
            painter.text(
                egui::pos2(inner.max.x - 26.0 * ch, hy),
                egui::Align2::RIGHT_CENTER,
                "*",
                font.clone(),
                c.alert,
            );
        }
        let seam = (inner.min.y + HEAD_H + 4.0).round() - 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, seam), egui::pos2(inner.max.x, seam)],
            egui::Stroke::new(1.0, c.rule),
        );

        // Three zones: parameters left, graphs right, controls along the foot.
        let controls_h = 24.0;
        let body = egui::Rect::from_min_max(
            egui::pos2(inner.min.x, seam + 6.0),
            egui::pos2(inner.max.x, inner.max.y - controls_h),
        );
        let params_w = (body.width() * 0.3).clamp(140.0, 280.0);
        let params =
            egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + params_w, body.max.y));
        let graphs =
            egui::Rect::from_min_max(egui::pos2(params.max.x + 12.0, body.min.y), body.max);

        let row_h = crate::tune!(ROW_H);
        let in_params = editor.focus == Focus::Parameters;
        for (i, param) in editor.params.iter().enumerate() {
            let y = params.min.y + i as f32 * row_h;
            if y + row_h > params.max.y {
                break;
            }
            let rect = egui::Rect::from_min_max(
                egui::pos2(params.min.x, y),
                egui::pos2(params.max.x, y + row_h),
            );
            let selected = editor.selected_params.contains(&i);
            let on = in_params && editor.param_cursor == i;
            if on {
                painter.rect_filled(rect, 0.0, c.select);
            }
            painter.text(
                egui::pos2(rect.min.x + 2.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                if selected { "+" } else { " " },
                font.clone(),
                c.chassis,
            );
            painter.text(
                egui::pos2(rect.min.x + 2.0 * ch, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &param.name,
                font.clone(),
                if selected { c.fg } else { c.dim },
            );
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("plock-param", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    c.alert,
                );
            }
        }

        // Graphs: one lane per selected parameter, a bar per tick.
        let lanes = editor.selected_param_indices();
        let ticks = editor.ticks.len().max(1);
        let lane_h = if lanes.is_empty() {
            0.0
        } else {
            ((graphs.height() - 4.0 * lanes.len() as f32) / lanes.len() as f32).clamp(18.0, 80.0)
        };
        let cell_w = graphs.width() / ticks as f32;
        for (li, &pi) in lanes.iter().enumerate() {
            let Some(param) = editor.params.get(pi) else {
                continue;
            };
            let top = graphs.min.y + li as f32 * (lane_h + 4.0);
            if top + lane_h > graphs.max.y {
                break;
            }
            let lane = egui::Rect::from_min_max(
                egui::pos2(graphs.min.x, top),
                egui::pos2(graphs.max.x, top + lane_h),
            );
            painter.rect_filled(lane, 0.0, c.panel);
            painter.text(
                egui::pos2(lane.min.x + 3.0, lane.min.y + 1.0),
                egui::Align2::LEFT_TOP,
                &param.name,
                font.clone(),
                c.label,
            );
            let base_y = lane.max.y - lane.height() * param.fraction(param.base);
            painter.line_segment(
                [
                    egui::pos2(lane.min.x, base_y.round() - 0.5),
                    egui::pos2(lane.max.x, base_y.round() - 0.5),
                ],
                egui::Stroke::new(1.0, c.edge),
            );
            for cell in 0..ticks {
                let x0 = lane.min.x + cell as f32 * cell_w;
                let active = editor.active.get(cell).copied().unwrap_or(false);
                let value = editor.displayed(pi, cell);
                let f = param.fraction(value);
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x0 + 1.0, lane.max.y - lane.height() * f),
                    egui::pos2(x0 + cell_w - 1.0, lane.max.y),
                );
                let locked = editor
                    .locks
                    .get(pi)
                    .and_then(|l| l.get(cell))
                    .copied()
                    .flatten()
                    .is_some();
                painter.rect_filled(
                    bar,
                    0.0,
                    if !active {
                        c.rule
                    } else if locked {
                        c.alert
                    } else {
                        c.chassis
                    },
                );
                let on = editor.focus == Focus::Graphs
                    && editor.graph_lane == li
                    && editor.graph_cell == cell;
                if on {
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("plock-cell", li, cell),
                        egui::Rect::from_min_max(
                            egui::pos2(x0, lane.min.y),
                            egui::pos2(x0 + cell_w, lane.max.y),
                        ),
                        crate::ui::nav_cursor::Kind::Cell,
                        crate::ui::nav_cursor::Layer::Overlay,
                        c.alert,
                    );
                }
            }
        }

        // Controls: the algorithm's own words, along the foot.
        let cy = inner.max.y - controls_h * 0.5;
        painter.text(
            egui::pos2(inner.min.x, cy),
            egui::Align2::LEFT_CENTER,
            editor.control_text(),
            font.clone(),
            if editor.focus == Focus::Controls {
                c.bright
            } else {
                c.fg
            },
        );

        // The picker, over everything, while it is up.
        if editor.picker {
            let list = Algorithm::ALL;
            let w = 200.0;
            let h = list.len() as f32 * row_h + INSET * 2.0;
            let pick = egui::Rect::from_center_size(panel.center(), egui::vec2(w, h));
            painter.rect_filled(pick, 0.0, c.ground);
            chassis::frame(painter, pick, true);
            for (i, algo) in list.iter().enumerate() {
                let y = pick.min.y + INSET + i as f32 * row_h;
                let on = *algo == editor.algorithm;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(pick.min.x + INSET, y),
                    egui::pos2(pick.max.x - INSET, y + row_h),
                );
                if on {
                    painter.rect_filled(rect, 0.0, c.select);
                }
                painter.text(
                    egui::pos2(rect.min.x + 2.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    algo.label(),
                    font.clone(),
                    if on { c.bright } else { c.fg },
                );
            }
        }
    }
}
