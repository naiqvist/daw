//! The lab's picture: the tiled windows over the field, and inside a
//! kiln window its three bands — the engine strip, the work area (the
//! object, rendered by its own depth-tested GPU pass), and the controls.

use super::{Stage, palette};
use crate::PROFONT;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::chrome;
use crate::ui::stage::lab::{Band, ENGINES, Instrument, MEMBRANE_MACROS, MEMBRANE_SLIDERS, Place};
use eframe::egui;

/// The gap between windows and around the field.
/// @tune 2..16 px
const GAP: f32 = 6.0;
/// A window's title bar.
/// @tune 14..28 px
const TITLE_H: f32 = 20.0;
/// The kiln's engine strip.
/// @tune 18..40 px
const STRIP_H: f32 = 26.0;
/// The control column's share of a kiln window.
/// @tune 0.25..0.6
const CONTROL_SHARE: f32 = 0.4;
/// A macro cell's height.
/// @tune 28..60 px
const MACRO_H: f32 = 40.0;
/// A slider row.
/// @tune 12..22 px
const ROW_H: f32 = 15.0;
const PAD: f32 = 6.0;

impl Stage {
    /// The lab takes the field: every window in its room, the focused
    /// one framed bright; the tiler's state on the field's foot.
    pub(super) fn draw_lab(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        field: egui::Rect,
    ) {
        // THE LAB OWNS THE KEYBOARD. Its windows carry ordinary egui
        // buttons, and egui's own focus travels on Tab — so one Tab into
        // a button and every key after it is the widget's, not the
        // stage's: the arrows stop turning, the rungs stop climbing, and
        // nothing on screen says why. Focus is surrendered every frame
        // the lab is up, because there is no widget here that should
        // hold it.
        ui.ctx().memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        let c = palette::colours();
        painter.rect_filled(field, 0.0, c.ground);
        let gap = crate::tune!(GAP);
        let inner = field.shrink(gap);
        let places = self.lab.places();
        let font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        if places.is_empty() {
            painter.text(
                inner.center(),
                egui::Align2::CENTER_CENTER,
                "LAB · Alt+Enter chooses an extension · Escape leaves",
                font,
                c.dim,
            );
            return;
        }
        for (id, place) in places {
            let rect = room(inner, place, gap);
            let Some(window) = self.lab.window(id).cloned() else {
                continue;
            };
            let focused = self.lab.focus == Some(id);
            let lit = focused && self.lab.inside;
            painter.rect_filled(rect, 0.0, c.panel);
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(
                    if focused { 1.5 } else { 1.0 },
                    if lit {
                        c.bright
                    } else if focused {
                        c.chassis
                    } else {
                        c.rule
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let title_h = crate::tune!(TITLE_H);
            let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), title_h));
            painter.text(
                egui::pos2(title.left() + PAD, title.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{} {}", window.instrument.name(), id + 1),
                font.clone(),
                if focused { c.bright } else { c.fg },
            );
            painter.text(
                egui::pos2(title.right() - PAD, title.center().y),
                egui::Align2::RIGHT_CENTER,
                if lit {
                    "Escape · tiler"
                } else if focused {
                    "Enter · in"
                } else {
                    ""
                },
                font.clone(),
                c.dim,
            );
            painter.line_segment(
                [title.left_bottom(), title.right_bottom()],
                egui::Stroke::new(1.0, c.rule),
            );
            // One door for every kind, under the pointer as well: the
            // menu, not a button per extension.
            let add = egui::Rect::from_min_size(
                egui::pos2(title.right() - 238., title.top() + 1.),
                egui::vec2(100., 18.),
            );
            if ui.put(add, egui::Button::new("+ extension")).clicked() {
                let _ = self.lab_menu_open();
            }
            let body = egui::Rect::from_min_max(title.left_bottom(), rect.max);
            match &window.instrument {
                Instrument::Kiln(kiln) => self.draw_kiln(painter, body, kiln, lit, id),
                Instrument::Midi(_) => self.draw_midi_lab(ui, body, id),
            }
        }
        self.draw_lab_menu(ui, painter, inner);
    }

    /// The menu of EXTENSIONS: what a window can hold, as a list. It
    /// stands over the tiler and owns the keys while it is up, so the
    /// window underneath is drawn but does not answer.
    fn draw_lab_menu(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, field: egui::Rect) {
        let Some(at) = self.lab.menu else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
        let row_h = 30.0;
        let width = 420.0f32.min(field.width() - 24.0);
        let height = row_h * crate::ui::stage::lab::EXTENSIONS.len() as f32 + 52.0;
        let rect = egui::Rect::from_center_size(field.center(), egui::vec2(width, height));
        painter.rect_filled(rect, 0.0, c.ground);
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, c.bright),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(rect.left() + PAD, rect.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            "EXTENSIONS",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(rect.right() - PAD, rect.top() + 12.0),
            egui::Align2::RIGHT_CENTER,
            "Enter opens · Escape leaves",
            small.clone(),
            c.dim,
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.top() + 24.0),
                egui::pos2(rect.right(), rect.top() + 24.0),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        for (index, kind) in crate::ui::stage::lab::EXTENSIONS.iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 28.0 + index as f32 * row_h),
                egui::vec2(rect.width(), row_h),
            );
            let chosen = index == at;
            if chosen {
                painter.rect_filled(row, 0.0, c.bright.linear_multiply(0.16));
                crate::ui::nav_cursor::claim(
                    painter,
                    ("lab-extension", index),
                    row,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    c.bright,
                );
            }
            painter.text(
                egui::pos2(row.left() + PAD, row.top() + 9.0),
                egui::Align2::LEFT_CENTER,
                format!("{}  {}", index + 1, kind.word),
                font.clone(),
                if chosen { c.bright } else { c.fg },
            );
            painter.text(
                egui::pos2(row.left() + PAD, row.top() + 22.0),
                egui::Align2::LEFT_CENTER,
                kind.note,
                small.clone(),
                c.dim,
            );
            // The pointer picks a row the same way the number does.
            if ui
                .interact(
                    row,
                    egui::Id::new(("lab-extension", index)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                let _ = self.lab_menu_pick(index);
            }
        }
    }

    /// One kiln window: the engine strip, the object, the controls.
    fn draw_kiln(
        &self,
        painter: &egui::Painter,
        body: egui::Rect,
        kiln: &crate::ui::stage::lab::Kiln,
        lit: bool,
        id: usize,
    ) {
        let c = palette::colours();
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(body));
        let font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(9.5, egui::FontFamily::Name(PROFONT.into()));
        let strip_h = crate::tune!(STRIP_H);

        // --- the engine strip ---------------------------------------
        let strip = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), strip_h));
        let (engine, _what) = ENGINES[kiln.engine.min(ENGINES.len() - 1)];
        let on_strip = lit && kiln.band == Band::Engine;
        painter.text(
            egui::pos2(strip.left() + PAD, strip.center().y),
            egui::Align2::LEFT_CENTER,
            format!("E  {}", engine.to_uppercase()),
            font.clone(),
            if on_strip { c.bright } else { c.fg },
        );
        painter.text(
            egui::pos2(strip.left() + PAD + 110.0, strip.center().y),
            egui::Align2::LEFT_CENTER,
            if kiln.submitted.is_some() {
                "rendering"
            } else {
                &kiln.status
            },
            small.clone(),
            c.dim,
        );
        painter.text(
            egui::pos2(strip.right() - PAD, strip.center().y),
            egui::Align2::RIGHT_CENTER,
            "H hear · P print · S send",
            small.clone(),
            c.dim,
        );
        if on_strip {
            painter.rect_stroke(
                strip.shrink(1.0),
                0.0,
                egui::Stroke::new(1.0, c.bright),
                egui::StrokeKind::Inside,
            );
        }
        painter.line_segment(
            [strip.left_bottom(), strip.right_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );

        if kiln.submitted.is_some() {
            if let Some(job) = &kiln.job {
                painter.line_segment(
                    [
                        strip.left_bottom(),
                        egui::pos2(
                            strip.left() + strip.width() * job.progress(),
                            strip.bottom(),
                        ),
                    ],
                    egui::Stroke::new(2., c.alert),
                );
            }
        }
        let below = egui::Rect::from_min_max(strip.left_bottom(), body.max);
        let share = crate::tune!(CONTROL_SHARE);
        let control_w = (below.width() * share)
            .clamp(180.0, 420.0)
            .min(below.width());
        let work = egui::Rect::from_min_max(
            below.min,
            egui::pos2(below.right() - control_w, below.bottom()),
        );
        let control = egui::Rect::from_min_max(egui::pos2(work.right(), below.top()), below.max);
        painter.line_segment(
            [control.left_top(), control.left_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );

        draw_object(&painter, work, kiln, id);
        self.draw_controls(&painter, control, kiln, lit);
    }

    /// The control column: sixteen macros in two rows, then the sliders.
    fn draw_controls(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        kiln: &crate::ui::stage::lab::Kiln,
        lit: bool,
    ) {
        let c = palette::colours();
        let font = egui::FontId::new(10.5, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
        let macro_h = crate::tune!(MACRO_H);
        let row_h = crate::tune!(ROW_H);

        // Macros: two rows of eight.
        let head_h = 16.0;
        painter.text(
            egui::pos2(rect.left() + PAD, rect.top() + head_h * 0.5),
            egui::Align2::LEFT_CENTER,
            "MACROS  1–8 · Shift 9–16",
            small.clone(),
            if lit && kiln.band == Band::Macros {
                c.fg
            } else {
                c.dim
            },
        );
        let cols = if rect.width() < 340.0 { 4 } else { 8 };
        let cell_w = rect.width() / cols as f32;
        for (i, (name, _)) in MEMBRANE_MACROS.iter().enumerate() {
            let row = i / cols;
            let col = i % cols;
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + col as f32 * cell_w,
                    rect.top() + head_h + row as f32 * macro_h,
                ),
                egui::vec2(cell_w, macro_h),
            )
            .shrink(2.0);
            let here = lit && kiln.band == Band::Macros && kiln.macro_at == i;
            painter.rect_filled(cell, 0.0, c.panel);
            painter.text(
                egui::pos2(cell.left() + 3.0, cell.top() + 3.0),
                egui::Align2::LEFT_TOP,
                *name,
                small.clone(),
                if here { c.bright } else { c.fg },
            );
            let value = kiln.macros[i];
            // The value on its own line under the name: the cells are
            // narrow and a name and a number cannot share one.
            painter.text(
                egui::pos2(cell.right() - 3.0, cell.top() + 15.0),
                egui::Align2::RIGHT_TOP,
                format!("{:.2}", value),
                small.clone(),
                if here { c.bright } else { c.dim },
            );
            let gauge = egui::Rect::from_min_max(
                egui::pos2(cell.left() + 3.0, cell.bottom() - 9.0),
                egui::pos2(cell.right() - 3.0, cell.bottom() - 4.0),
            );
            let mut shapes = Vec::new();
            chrome::tick_bar(
                &mut shapes,
                gauge,
                8,
                value,
                if here { c.bright } else { c.fg },
                c.edge,
                true,
            );
            painter.extend(shapes);
            if here {
                painter.rect_stroke(
                    cell,
                    0.0,
                    egui::Stroke::new(1.5, c.bright),
                    egui::StrokeKind::Inside,
                );
            }
        }

        // The sliders: a filter line, then the list, grouped.
        let list_top = rect.top() + head_h + (16 / cols) as f32 * macro_h + 4.0;
        let on_list = lit && kiln.band == Band::Sliders;
        let filter_text = if kiln.filtering {
            format!("/ {}▏", kiln.filter)
        } else if kiln.filter.is_empty() {
            "SLIDERS  / filter".to_owned()
        } else {
            format!("SLIDERS  / {}", kiln.filter)
        };
        painter.text(
            egui::pos2(rect.left() + PAD, list_top + head_h * 0.5),
            egui::Align2::LEFT_CENTER,
            filter_text,
            small.clone(),
            if on_list { c.fg } else { c.dim },
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), list_top + head_h),
                egui::pos2(rect.right(), list_top + head_h),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let shown = kiln.shown();
        let rows_fit =
            (((rect.bottom() - list_top - head_h - 16.0) / row_h).floor() as usize).max(1);
        // Rows are sliders plus a header each time the group changes;
        // scroll so the cursor's row stays in view.
        let mut rows: Vec<Option<usize>> = Vec::new();
        let mut last_group = "";
        for &i in &shown {
            let group = MEMBRANE_SLIDERS[i].group;
            if group != last_group {
                rows.push(None);
                last_group = group;
            }
            rows.push(Some(i));
        }
        let cursor_row = shown
            .get(kiln.slider_at.min(shown.len().saturating_sub(1)))
            .and_then(|i| rows.iter().position(|r| *r == Some(*i)))
            .unwrap_or(0);
        let first = cursor_row
            .saturating_sub(rows_fit.saturating_sub(1))
            .min(rows.len().saturating_sub(rows_fit));
        let x_value = rect.right() - PAD;
        for (n, row) in rows.iter().skip(first).take(rows_fit).enumerate() {
            let y = list_top + head_h + n as f32 * row_h;
            let line = egui::Rect::from_min_size(
                egui::pos2(rect.left(), y),
                egui::vec2(rect.width(), row_h),
            );
            match row {
                None => {
                    let group = shown
                        .iter()
                        .find(|i| rows.iter().position(|r| *r == Some(**i)) > Some(first + n))
                        .map(|i| MEMBRANE_SLIDERS[*i].group)
                        .unwrap_or("");
                    painter.text(
                        egui::pos2(line.left() + PAD, line.center().y),
                        egui::Align2::LEFT_CENTER,
                        group.to_uppercase(),
                        small.clone(),
                        c.label,
                    );
                }
                Some(i) => {
                    let def = &MEMBRANE_SLIDERS[*i];
                    let here = on_list
                        && shown.get(kiln.slider_at.min(shown.len().saturating_sub(1))) == Some(i);
                    if here {
                        painter.rect_filled(line.shrink2(egui::vec2(1.0, 0.5)), 0.0, c.select);
                    }
                    painter.text(
                        egui::pos2(line.left() + PAD + 10.0, line.center().y),
                        egui::Align2::LEFT_CENTER,
                        def.name,
                        font.clone(),
                        if here { c.bright } else { c.fg },
                    );
                    let value = kiln.sliders.get(*i).copied().unwrap_or(def.default);
                    let place = ((value - def.min) / (def.max - def.min)).clamp(0.0, 1.0);
                    painter.text(
                        egui::pos2(x_value, line.center().y),
                        egui::Align2::RIGHT_CENTER,
                        kiln.reading(*i),
                        small.clone(),
                        if here { c.bright } else { c.dim },
                    );
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(x_value - 120.0, line.center().y - 1.0),
                        egui::pos2(x_value - 62.0, line.center().y + 1.0),
                    );
                    painter.rect_filled(bar, 0.0, c.edge);
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            bar.min,
                            egui::pos2(bar.left() + bar.width() * place, bar.bottom()),
                        ),
                        0.0,
                        if here { c.bright } else { c.chassis },
                    );
                }
            }
        }
        if rows.len() > rows_fit {
            painter.text(
                egui::pos2(rect.right() - PAD, rect.bottom() - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{} of {} sliders", shown.len(), MEMBRANE_SLIDERS.len()),
                small,
                c.dim,
            );
        }
    }
}

/// A window's rectangle from its room in the unit square.
fn room(inner: egui::Rect, place: Place, gap: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            inner.left() + place.x * inner.width(),
            inner.top() + place.y * inner.height(),
        ),
        egui::vec2(place.w * inner.width(), place.h * inner.height()),
    )
    .shrink(gap * 0.5)
}

/// The shader pass owns projection, lighting, antialiasing and depth.
fn draw_object(
    painter: &egui::Painter,
    work: egui::Rect,
    kiln: &crate::ui::stage::lab::Kiln,
    id: usize,
) {
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(work));
    let c = palette::colours();
    let small = egui::FontId::new(9.5, egui::FontFamily::Name(PROFONT.into()));
    let area = work.shrink(8.0);
    if area.width() < 10.0 || area.height() < 50.0 {
        return;
    }
    let viewport = egui::Rect::from_min_max(
        area.min + egui::vec2(0., 22.),
        area.max - egui::vec2(0., 36.),
    );
    let elapsed = kiln.played.map(|t| t.elapsed().as_secs_f32());
    let time = kiln
        .scrub
        .or_else(|| elapsed.filter(|t| *t < 3.0).map(|t| t * 0.08));
    let selected = kiln.shown().get(kiln.slider_at).copied().unwrap_or(0);
    let order = if (21..37).contains(&selected) {
        selected - 21
    } else if (37..53).contains(&selected) {
        selected - 37
    } else {
        0
    };
    let standing = kiln
        .render
        .as_ref()
        .and_then(|r| r.animation.top.iter().position(|s| s.order == order))
        .unwrap_or(0);
    let bg = egui::Rgba::from(c.ground).to_array();
    painter.add(egui_wgpu::Callback::new_paint_callback(
        viewport,
        crate::ui::kiln::Scene {
            chord: None,
            id,
            patch: kiln.patch(),
            animation: kiln.render.as_ref().map(|r| r.animation.clone()),
            time,
            standing,
            camera: kiln.camera,
            background: bg,
            size: [viewport.width(), viewport.height()],
        },
    ));
    painter.text(
        area.left_top(),
        egui::Align2::LEFT_TOP,
        "MEMBRANE / SECTION VIEW",
        small.clone(),
        c.fg,
    );
    painter.text(
        area.right_top(),
        egui::Align2::RIGHT_TOP,
        if time.is_some() {
            "STRIKE × 0.08"
        } else {
            "MODE SHAPE"
        },
        small.clone(),
        c.label,
    );
    painter.text(
        egui::pos2(area.left(), area.bottom() - 17.),
        egui::Align2::LEFT_BOTTOM,
        if work.width() < 420.0 {
            "Ctrl arrows orbit · −/+ zoom"
        } else {
            "Ctrl arrows orbit · −/+ zoom · Ctrl [ ] scrub"
        },
        small.clone(),
        c.dim,
    );
    painter.text(
        area.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        "H hear · P print · S send · Shift S replace",
        small,
        c.dim,
    );
}
