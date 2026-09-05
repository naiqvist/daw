//! The panel the knobs live on.
//!
//! Every constant marked `@tune` in the view's and the glass's source,
//! grouped by the file it lives in, each one a track you drag while
//! looking at the thing it changes. The cockpit's inspector, in the
//! console's own chassis. See `crate::tune` for what a knob is and where
//! its value comes from.
//!
//! A drag PREVIEWS: it puts the value straight into the registry and
//! writes nothing. Letting go writes the file once. Click a changed value
//! to reset it; double-click to write it into the source as the default.

use super::{chassis, palette};
use crate::PROFONT;
use crate::tune;
use crate::ui::affordance::{Afford, Affords};
use eframe::egui;

const ROW_H: f32 = 22.0;
const NAME_W: f32 = 132.0;
const VALUE_W: f32 = 78.0;
const TRACK_H: f32 = 6.0;
const GROUP_GAP: f32 = 13.0;
const TYPE_PX: f32 = 12.0;
/// The panel's width, on the field's right.
pub const WIDTH: f32 = 360.0;

pub struct Inspector {
    pub open: bool,
    knobs: Vec<tune::Knob>,
    pub status: String,
}

impl Inspector {
    pub fn new() -> Self {
        let knobs = tune::scan_source();
        let status = if knobs.is_empty() {
            "no source to read".to_owned()
        } else {
            format!("{} knobs", knobs.len())
        };
        Self {
            open: false,
            knobs,
            status,
        }
    }

    pub fn rescan(&mut self) {
        self.knobs = tune::scan_source();
    }

    fn span(knob: &tune::Knob) -> (f64, f64) {
        knob.range.unwrap_or_else(|| {
            let reach = (knob.default.abs() * 3.0).max(1.0);
            if knob.default < 0.0 {
                (-reach, 0.0)
            } else {
                (0.0, reach)
            }
        })
    }

    fn format(value: f64, lo: f64, hi: f64) -> String {
        let step = (hi - lo).abs();
        let places = if step <= 1.0 {
            3
        } else if step <= 20.0 {
            2
        } else {
            1
        };
        let s = format!("{value:.places$}");
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let c = palette::colours();
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, c.ground);
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let mut y = rect.top() + 10.0;
        painter.text(
            egui::pos2(rect.left() + 12.0, y),
            egui::Align2::LEFT_TOP,
            "TUNABLES",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(rect.right() - 12.0, y),
            egui::Align2::RIGHT_TOP,
            &self.status,
            font.clone(),
            c.dim,
        );
        y += 22.0;
        let mut group = String::new();
        let mut changed: Option<(String, f64, bool)> = None;
        let mut cleared: Option<String> = None;
        let mut committed: Option<usize> = None;
        for (i, knob) in self.knobs.iter().enumerate() {
            let module = knob.key.split('.').next().unwrap_or("").to_owned();
            if module != group {
                y += GROUP_GAP;
                painter.text(
                    egui::pos2(rect.left() + 12.0, y),
                    egui::Align2::LEFT_TOP,
                    &module,
                    font.clone(),
                    c.dir,
                );
                y += 16.0;
                group = module;
            }
            if y + ROW_H > rect.bottom() - 8.0 {
                painter.text(
                    egui::pos2(rect.left() + 12.0, rect.bottom() - 16.0),
                    egui::Align2::LEFT_TOP,
                    format!("{} more below", self.knobs.len() - i),
                    font.clone(),
                    c.dim,
                );
                break;
            }
            let (lo, hi) = Self::span(knob);
            let overridden = tune::override_of(&knob.key);
            let value = overridden.unwrap_or(knob.default);
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 12.0, y),
                egui::vec2(rect.width() - 24.0, ROW_H),
            );
            painter.text(
                egui::pos2(row.left(), row.center().y),
                egui::Align2::LEFT_CENTER,
                &knob.name,
                font.clone(),
                if overridden.is_some() { c.bright } else { c.fg },
            );
            let track = egui::Rect::from_min_max(
                egui::pos2(row.left() + NAME_W, row.center().y - TRACK_H * 0.5),
                egui::pos2(row.right() - VALUE_W, row.center().y + TRACK_H * 0.5),
            );
            if track.width() > 20.0 {
                let hit = track.expand2(egui::vec2(0.0, 7.0));
                let resp = ui
                    .interact(hit, ui.id().with(&knob.key), egui::Sense::click_and_drag())
                    .affords(Affords::Slide);
                painter.rect_filled(track, 0.0, c.panel);
                let t = if hi > lo {
                    ((value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let filled = egui::Rect::from_min_max(
                    track.min,
                    egui::pos2(track.left() + track.width() * t, track.max.y),
                );
                painter.rect_filled(filled, 0.0, c.chassis);
                let d = if hi > lo {
                    ((knob.default - lo) / (hi - lo)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let dx = track.left() + track.width() * d;
                painter.line_segment(
                    [
                        egui::pos2(dx, track.top() - 4.0),
                        egui::pos2(dx, track.bottom() + 4.0),
                    ],
                    egui::Stroke::new(1.0, c.rule),
                );
                if (resp.dragged() || resp.clicked())
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
                    changed = Some((knob.key.clone(), lo + t * (hi - lo), false));
                }
                if resp.drag_stopped() {
                    changed = Some((knob.key.clone(), value, true));
                }
                if resp.hovered() && !knob.doc.is_empty() {
                    resp.on_hover_text(&knob.doc);
                }
            }
            let value_at = egui::Rect::from_min_max(
                egui::pos2(row.right() - VALUE_W, row.top()),
                egui::pos2(row.right(), row.bottom()),
            );
            let shown = match &knob.unit {
                Some(u) => format!("{} {u}", Self::format(value, lo, hi)),
                None => Self::format(value, lo, hi),
            };
            painter.text(
                egui::pos2(value_at.right(), row.center().y),
                egui::Align2::RIGHT_CENTER,
                shown,
                font.clone(),
                if overridden.is_some() { c.alert } else { c.dim },
            );
            if overridden.is_some() {
                let resp = ui
                    .interact(
                        value_at,
                        ui.id().with((&knob.key, "reset")),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press);
                let resp =
                    resp.on_hover_text("click to reset · double-click to write into the source");
                if resp.double_clicked() {
                    committed = Some(i);
                } else if resp.clicked() {
                    cleared = Some(knob.key.clone());
                }
            }
            y += ROW_H;
        }
        if let Some((key, value, release)) = changed {
            if release {
                match tune::set(&key, value) {
                    Ok(()) => self.status = format!("{key} = {value:.4}"),
                    Err(e) => self.status = format!("write failed: {e}"),
                }
            } else {
                tune::preview(&key, value);
            }
            ui.ctx().request_repaint();
        }
        if let Some(key) = cleared {
            match tune::clear(&key) {
                Ok(()) => self.status = format!("{key} reset"),
                Err(e) => self.status = format!("reset failed: {e}"),
            }
        }
        if let Some(i) = committed {
            let knob = self.knobs[i].clone();
            let value = tune::override_of(&knob.key).unwrap_or(knob.default);
            self.status = match tune::commit(&knob, value) {
                Ok(()) => format!("{} written to {}", knob.name, knob.file.display()),
                Err(e) => format!("commit failed: {e}"),
            };
            self.rescan();
        }
        chassis::frame(&painter, rect.shrink(2.0), false);
    }
}
