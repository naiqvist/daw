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
        &mut self,
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

    fn draw_plock_editor(&mut self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(editor) = self.plock_editor.as_ref() else {
            return;
        };
        #[derive(Clone, Copy)]
        enum Action {
            Parameter(usize, bool),
            Graph {
                lane: usize,
                cell: usize,
                fraction: f32,
            },
            Controls,
            OpenPicker,
            Choose(Algorithm),
        }

        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let (pointer, pressed, extend) = painter.ctx().input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_pressed(),
                input.modifiers.shift || input.modifiers.command,
            )
        });
        let mut action = None;
        let panel = whole.shrink(crate::tune!(EDITOR_INSET));
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET + 4.0);

        // The head: address, selection size, and the zone that owns the
        // arrows. The zone is signal-coloured rather than buried in prose.
        let hy = inner.min.y + HEAD_H * 0.5;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            format!(
                "PLOCK  PAT {:02} · TR {:02}",
                editor.pattern.0,
                editor.track + 1
            ),
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(inner.center().x, hy),
            egui::Align2::CENTER_CENTER,
            format!(
                "{} NOTES · {} ACTIVE · {} PARAMS",
                editor.ticks.len(),
                editor.active.iter().filter(|active| **active).count(),
                editor.selected_params.len(),
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
            format!("{}{}", if editor.dirty { "* " } else { "" }, zone),
            font.clone(),
            c.alert,
        );
        let seam = (inner.min.y + HEAD_H + 4.0).round() - 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, seam), egui::pos2(inner.max.x, seam)],
            egui::Stroke::new(1.0, c.rule),
        );

        // Three visibly framed zones: parameters left, lock values right,
        // transformations along the foot. Tab moves between these exact
        // frames; pointing can enter any of them directly.
        let controls_h = 52.0;
        let body = egui::Rect::from_min_max(
            egui::pos2(inner.min.x, seam + 6.0),
            egui::pos2(inner.max.x, inner.max.y - controls_h - 6.0),
        );
        let params_w = (body.width() * 0.34).clamp(170.0, 330.0);
        let params =
            egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + params_w, body.max.y));
        let graphs =
            egui::Rect::from_min_max(egui::pos2(params.max.x + 12.0, body.min.y), body.max);
        let controls =
            egui::Rect::from_min_max(egui::pos2(inner.min.x, body.max.y + 6.0), inner.max);
        painter.rect_filled(params, 0.0, c.panel);
        painter.rect_filled(graphs, 0.0, c.panel);
        painter.rect_filled(controls, 0.0, c.panel);
        chassis::frame(painter, params, editor.focus == Focus::Parameters);
        chassis::frame(painter, graphs, editor.focus == Focus::Graphs);
        chassis::frame(painter, controls, editor.focus == Focus::Controls);

        let row_h = crate::tune!(ROW_H);
        let in_params = editor.focus == Focus::Parameters;
        let zone_head = 21.0;
        let param_body = egui::Rect::from_min_max(
            egui::pos2(params.min.x + 4.0, params.min.y + zone_head),
            egui::pos2(params.max.x - 4.0, params.max.y - 4.0),
        );
        let param_capacity = (param_body.height() / row_h).floor().max(1.0) as usize;
        let (param_start, param_end) = crate::ui::stage::plock_editor::visible_span(
            editor.param_cursor,
            editor.params.len(),
            param_capacity,
        );
        painter.text(
            egui::pos2(params.min.x + 7.0, params.min.y + zone_head * 0.5),
            egui::Align2::LEFT_CENTER,
            "PARAMS  ↑↓ ROW · ←→ VALUE",
            font.clone(),
            if in_params { c.bright } else { c.label },
        );
        painter.text(
            egui::pos2(params.max.x - 7.0, params.min.y + zone_head * 0.5),
            egui::Align2::RIGHT_CENTER,
            format!(
                "{}–{} / {}",
                param_start + 1,
                param_end,
                editor.params.len()
            ),
            font.clone(),
            c.dim,
        );
        let list_painter = painter.with_clip_rect(param_body);
        let active_count = editor.active.iter().filter(|active| **active).count();
        for (slot, i) in (param_start..param_end).enumerate() {
            let param = &editor.params[i];
            let y = param_body.min.y + slot as f32 * row_h;
            let rect = egui::Rect::from_min_max(
                egui::pos2(param_body.min.x, y),
                egui::pos2(param_body.max.x, y + row_h),
            );
            let selected = editor.selected_params.contains(&i);
            let on = in_params && editor.param_cursor == i;
            let hovered = pointer.is_some_and(|point| rect.contains(point));
            if on || hovered {
                list_painter.rect_filled(rect, 0.0, c.select);
            }
            if pressed && hovered && !editor.picker {
                action = Some(Action::Parameter(i, extend));
            }
            list_painter.text(
                egui::pos2(rect.min.x + 2.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                if selected { "+" } else { " " },
                font.clone(),
                c.chassis,
            );
            let value_w = (rect.width() * 0.48).clamp(86.0, 146.0);
            let name_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x - value_w - 4.0, rect.max.y),
            );
            list_painter.with_clip_rect(name_rect).text(
                egui::pos2(rect.min.x + 2.0 * ch, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &param.name,
                font.clone(),
                if selected { c.fg } else { c.dim },
            );
            let locked = editor.lock_count(i);
            let state = if locked == 0 {
                format!("BASE {}", editor.value_text(i))
            } else if active_count <= 1 {
                format!("LOCK {}", editor.value_text(i))
            } else {
                format!("{locked}/{active_count} {}", editor.value_text(i))
            };
            list_painter.text(
                egui::pos2(rect.max.x - 2.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                state,
                font.clone(),
                if locked > 0 { c.alert } else { c.dim },
            );
            let standing = editor.displayed(
                i,
                editor.graph_cell.min(editor.ticks.len().saturating_sub(1)),
            );
            let rail = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + 2.0 * ch, rect.max.y - 2.0),
                egui::pos2(rect.max.x - 2.0, rect.max.y - 1.0),
            );
            list_painter.rect_filled(rail, 0.0, c.rule);
            list_painter.rect_filled(
                egui::Rect::from_min_max(
                    rail.min,
                    egui::pos2(
                        rail.min.x + rail.width() * param.fraction(standing),
                        rail.max.y,
                    ),
                ),
                0.0,
                if locked > 0 { c.alert } else { c.chassis },
            );
            if on {
                crate::ui::nav_cursor::claim(
                    &list_painter,
                    ("plock-param", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    c.alert,
                );
            }
        }

        // Graphs: a useful-height window of selected parameters, and a
        // cursor-following window of addressed notes. One lane now fills the
        // available editor instead of becoming an 80 px postage stamp.
        let lanes = editor.selected_param_indices();
        let graph_body = egui::Rect::from_min_max(
            egui::pos2(graphs.min.x + 4.0, graphs.min.y + zone_head),
            egui::pos2(graphs.max.x - 4.0, graphs.max.y - 4.0),
        );
        let lane_capacity = lanes.len().min(6).max(1);
        let (lane_start, lane_end) = crate::ui::stage::plock_editor::visible_span(
            editor.graph_lane,
            lanes.len(),
            lane_capacity,
        );
        let shown_lanes = lane_end.saturating_sub(lane_start).max(1);
        let lane_gap = 5.0;
        let lane_h = ((graph_body.height() - lane_gap * shown_lanes.saturating_sub(1) as f32)
            / shown_lanes as f32)
            .max(24.0);
        let cell_capacity = ((graph_body.width() / 14.0).floor() as usize).max(1);
        let (cell_start, cell_end) = crate::ui::stage::plock_editor::visible_span(
            editor.graph_cell,
            editor.ticks.len(),
            cell_capacity,
        );
        let shown_cells = cell_end.saturating_sub(cell_start).max(1);
        let cell_w = graph_body.width() / shown_cells as f32;
        painter.text(
            egui::pos2(graphs.min.x + 7.0, graphs.min.y + zone_head * 0.5),
            egui::Align2::LEFT_CENTER,
            "LOCK VALUES  ←→ NOTE · ↑↓ VALUE · CTRL↑↓ LANE",
            font.clone(),
            if editor.focus == Focus::Graphs {
                c.bright
            } else {
                c.label
            },
        );
        painter.text(
            egui::pos2(graphs.max.x - 7.0, graphs.min.y + zone_head * 0.5),
            egui::Align2::RIGHT_CENTER,
            format!(
                "P {}–{} / {} · N {}–{} / {}",
                lane_start + usize::from(!lanes.is_empty()),
                lane_end,
                lanes.len(),
                cell_start + usize::from(!editor.ticks.is_empty()),
                cell_end,
                editor.ticks.len()
            ),
            font.clone(),
            c.dim,
        );
        let graph_painter = painter.with_clip_rect(graph_body);
        for (shown, li) in (lane_start..lane_end).enumerate() {
            let Some(&pi) = lanes.get(li) else {
                continue;
            };
            let Some(param) = editor.params.get(pi) else {
                continue;
            };
            let top = graph_body.min.y + shown as f32 * (lane_h + lane_gap);
            let lane = egui::Rect::from_min_max(
                egui::pos2(graph_body.min.x, top),
                egui::pos2(graph_body.max.x, (top + lane_h).min(graph_body.max.y)),
            );
            graph_painter.rect_filled(lane, 0.0, c.ground);
            let lane_focused = editor.focus == Focus::Graphs && editor.graph_lane == li;
            if lane_focused {
                graph_painter.rect_stroke(
                    lane,
                    0.0,
                    egui::Stroke::new(1.0, c.chassis),
                    egui::StrokeKind::Inside,
                );
            }
            graph_painter.text(
                egui::pos2(lane.min.x + 3.0, lane.min.y + 1.0),
                egui::Align2::LEFT_TOP,
                &param.name,
                font.clone(),
                if lane_focused { c.bright } else { c.label },
            );
            let focus_cell = editor.graph_cell.min(editor.ticks.len().saturating_sub(1));
            let focus_value = editor.displayed(pi, focus_cell);
            let focus_locked = editor
                .locks
                .get(pi)
                .and_then(|values| values.get(focus_cell))
                .copied()
                .flatten()
                .is_some();
            graph_painter.text(
                egui::pos2(lane.max.x - 4.0, lane.min.y + 1.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "{} · {}",
                    if focus_locked { "LOCK" } else { "BASE" },
                    param.face(focus_value)
                ),
                font.clone(),
                if focus_locked { c.alert } else { c.dim },
            );
            let plot = egui::Rect::from_min_max(
                egui::pos2(lane.min.x + 2.0, lane.min.y + row_h),
                egui::pos2(lane.max.x - 2.0, lane.max.y - 3.0),
            );
            let base_y = plot.max.y - plot.height() * param.fraction(param.base);
            graph_painter.line_segment(
                [
                    egui::pos2(plot.min.x, base_y.round() - 0.5),
                    egui::pos2(plot.max.x, base_y.round() - 0.5),
                ],
                egui::Stroke::new(1.0, c.edge),
            );
            for (shown_cell, cell) in (cell_start..cell_end).enumerate() {
                let x0 = plot.min.x + shown_cell as f32 * cell_w;
                let cell_rect = egui::Rect::from_min_max(
                    egui::pos2(x0, plot.min.y),
                    egui::pos2((x0 + cell_w).min(plot.max.x), plot.max.y),
                );
                let active = editor.active.get(cell).copied().unwrap_or(false);
                let value = editor.displayed(pi, cell);
                let f = param.fraction(value);
                let value_y = plot.max.y - plot.height() * f;
                let locked = editor
                    .locks
                    .get(pi)
                    .and_then(|values| values.get(cell))
                    .copied()
                    .flatten()
                    .is_some();
                let on = lane_focused && editor.graph_cell == cell;
                if on {
                    graph_painter.rect_filled(cell_rect, 0.0, c.select);
                }
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x0 + 2.0, value_y.min(plot.max.y - 2.0)),
                    egui::pos2((x0 + cell_w - 2.0).max(x0 + 3.0), plot.max.y),
                );
                graph_painter.rect_filled(
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
                // The cap is visible even at zero, so a minimum/base value
                // cannot collapse to a mathematically correct invisible bar.
                graph_painter.line_segment(
                    [
                        egui::pos2(x0 + 2.0, value_y.clamp(plot.min.y, plot.max.y - 1.0)),
                        egui::pos2(
                            (x0 + cell_w - 2.0).max(x0 + 3.0),
                            value_y.clamp(plot.min.y, plot.max.y - 1.0),
                        ),
                    ],
                    egui::Stroke::new(2.0, if locked { c.alert } else { c.chassis }),
                );
                if shown_cells <= 24 {
                    graph_painter.text(
                        egui::pos2(cell_rect.center().x, plot.max.y - 2.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!(
                            "{:02}",
                            editor.ticks[cell] / crate::sequencing::PATTERN_STEP_TICKS + 1
                        ),
                        font.clone(),
                        c.dim,
                    );
                }
                if pressed
                    && !editor.picker
                    && pointer.is_some_and(|point| cell_rect.contains(point))
                {
                    let point = pointer.expect("tested above");
                    action = Some(Action::Graph {
                        lane: li,
                        cell,
                        fraction: 1.0 - ((point.y - plot.min.y) / plot.height().max(1.0)),
                    });
                }
                if on {
                    crate::ui::nav_cursor::claim(
                        &graph_painter,
                        ("plock-cell", li, cell),
                        cell_rect,
                        crate::ui::nav_cursor::Kind::Cell,
                        crate::ui::nav_cursor::Layer::Overlay,
                        c.alert,
                    );
                }
            }
        }

        // Controls: named, clickable, and with its own cursor claim. The
        // second line is deliberately local help for the open modal.
        let controls_hovered = pointer.is_some_and(|point| controls.contains(point));
        if pressed && controls_hovered && !editor.picker {
            action = Some(Action::Controls);
        }
        let control_y = controls.min.y + 14.0;
        let algo_rect = egui::Rect::from_min_max(
            egui::pos2(controls.min.x + 6.0, controls.min.y + 3.0),
            egui::pos2(controls.min.x + 26.0 * ch, controls.min.y + 25.0),
        );
        if pressed && !editor.picker && pointer.is_some_and(|point| algo_rect.contains(point)) {
            action = Some(Action::OpenPicker);
        }
        painter.text(
            egui::pos2(controls.min.x + 7.0, control_y),
            egui::Align2::LEFT_CENTER,
            format!("/ {}", editor.algorithm.label()),
            font.clone(),
            if editor.focus == Focus::Controls {
                c.bright
            } else {
                c.fg
            },
        );
        painter.text(
            egui::pos2(controls.center().x, control_y),
            egui::Align2::CENTER_CENTER,
            editor.control_text(),
            font.clone(),
            if editor.focus == Focus::Controls {
                c.alert
            } else {
                c.fg
            },
        );
        painter.text(
            egui::pos2(controls.min.x + 7.0, controls.max.y - 10.0),
            egui::Align2::LEFT_CENTER,
            "TAB ZONE · X INCLUDE · CTRL+A ALL · DEL CLEAR",
            font.clone(),
            c.dim,
        );
        painter.text(
            egui::pos2(controls.max.x - 7.0, controls.max.y - 10.0),
            egui::Align2::RIGHT_CENTER,
            "ENTER COMMIT · ESC CANCEL",
            font.clone(),
            c.dim,
        );
        if editor.focus == Focus::Controls && !editor.picker {
            crate::ui::nav_cursor::claim(
                painter,
                ("plock-controls", editor.control_cursor),
                controls.shrink(3.0),
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Overlay,
                c.alert,
            );
        }

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
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("plock-algorithm", i),
                        rect,
                        crate::ui::nav_cursor::Kind::Row,
                        crate::ui::nav_cursor::Layer::Overlay,
                        c.alert,
                    );
                }
                if pressed && pointer.is_some_and(|point| rect.contains(point)) {
                    action = Some(Action::Choose(*algo));
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

        // Pointer actions land after the immutable paint snapshot. Requesting
        // a frame makes the new address/value visible immediately, while lock
        // edits alone enter the normal preview/recompile path.
        let mut preview = false;
        match action {
            Some(Action::Parameter(index, extend)) => {
                if let Some(editor) = self.plock_editor.as_mut() {
                    editor.point_parameter(index, extend);
                }
            }
            Some(Action::Graph {
                lane,
                cell,
                fraction,
            }) => {
                preview = self
                    .plock_editor
                    .as_mut()
                    .is_some_and(|editor| editor.set_graph_fraction(lane, cell, fraction));
            }
            Some(Action::Controls) => {
                if let Some(editor) = self.plock_editor.as_mut() {
                    editor.focus = Focus::Controls;
                    editor.picker = false;
                }
            }
            Some(Action::OpenPicker) => {
                if let Some(editor) = self.plock_editor.as_mut() {
                    editor.open_picker();
                }
            }
            Some(Action::Choose(algorithm)) => {
                if let Some(editor) = self.plock_editor.as_mut() {
                    editor.algorithm = algorithm;
                    editor.picker_enter();
                    preview = true;
                }
            }
            None => {}
        }
        if preview {
            self.preview_plock_editor();
        }
        if action.is_some() {
            painter.ctx().request_repaint();
        }
    }
}
