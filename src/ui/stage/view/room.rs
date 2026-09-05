//! The machine room, painted: projects, preferences, export and
//! diagnostics, in front of the whole of the musical surface.
//!
//! What the room IS — its pages, rows, values and actions — is
//! `stage::utility`; its keys are read in `view::utility`. This only
//! turns the room's own rows (`Console::display_rows`) into paint:
//! pages as tabs, the current one underlined in alert; rows with a code
//! in blue, a label, a value; a disabled row dim; an alarm in fault; the
//! cursor's row on select with brackets; the status along the foot; and
//! a confirmation, when one is pending, as a small box over the rest.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::utility::{Confirm, Page, PrefPage};
use eframe::egui;

/// One row of the room.
/// @tune 12..32 px
const ROW_H: f32 = 20.0;
const HEAD_H: f32 = 26.0;
const INSET: f32 = 14.0;
const TYPE_PX: f32 = 12.0;

impl super::super::Stage {
    pub(super) fn draw_room(&self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(page) = self.utility.page() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin() * 2.0;
        let panel = whole.shrink(margin);
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET);

        // The head: the room, its pages, the current one underlined.
        let hy = inner.min.y + HEAD_H * 0.5 - 2.0;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "MACHINE ROOM",
            font.clone(),
            c.label,
        );
        let mut x = inner.min.x + 15.0 * ch;
        for p in Page::ALL {
            let on = p == page;
            let word = p.label();
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
        if page == Page::Preferences {
            x += 2.0 * ch;
            for p in PrefPage::ALL {
                let on = p == self.utility.pref_page;
                let word = p.label();
                painter.text(
                    egui::pos2(x, hy),
                    egui::Align2::LEFT_CENTER,
                    word,
                    font.clone(),
                    if on { c.fg } else { c.dim },
                );
                if on {
                    painter.line_segment(
                        [
                            egui::pos2(x, hy + 8.0),
                            egui::pos2(x + word.len() as f32 * ch, hy + 8.0),
                        ],
                        egui::Stroke::new(1.0, c.chassis),
                    );
                }
                x += (word.len() as f32 + 2.0) * ch;
            }
        }
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            "esc closes",
            font.clone(),
            c.dim,
        );
        let seam = (inner.min.y + HEAD_H).round() - 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, seam), egui::pos2(inner.max.x, seam)],
            egui::Stroke::new(1.0, c.rule),
        );

        // The rows.
        let snapshot = self.utility_snapshot();
        let rows = self.utility.display_rows(&snapshot);
        let top = inner.min.y + HEAD_H + 6.0;
        let row_h = crate::tune!(ROW_H);
        let foot = inner.max.y - row_h;
        let editing = self.utility.editing.is_some();
        for (i, row) in rows.iter().enumerate() {
            let y = top + i as f32 * row_h;
            if y + row_h > foot {
                break;
            }
            let rect = egui::Rect::from_min_max(
                egui::pos2(inner.min.x, y),
                egui::pos2(inner.max.x, y + row_h),
            );
            let on = i == self.utility.row;
            if on {
                painter.rect_filled(rect, 0.0, c.select);
            }
            let cy = rect.center().y;
            painter.text(
                egui::pos2(rect.min.x + 2.0, cy),
                egui::Align2::LEFT_CENTER,
                &row.code,
                font.clone(),
                c.label,
            );
            painter.text(
                egui::pos2(rect.min.x + 8.0 * ch, cy),
                egui::Align2::LEFT_CENTER,
                &row.label,
                font.clone(),
                if !row.enabled {
                    c.dim
                } else if on {
                    c.bright
                } else {
                    c.fg
                },
            );
            let value_colour = if row.alarm {
                c.fault
            } else if on && editing {
                c.alert
            } else if !row.enabled {
                c.dim
            } else {
                c.fg
            };
            painter.text(
                egui::pos2(rect.max.x - 2.0, cy),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                font.clone(),
                value_colour,
            );
            if on && editing {
                // The caret, after the value being typed.
                let vx = rect.max.x - 2.0;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(vx, cy - 6.0),
                        egui::pos2(vx + 1.5, cy + 6.0),
                    ),
                    0.0,
                    c.alert,
                );
            }
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("room", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Utility,
                    c.alert,
                );
            }
        }

        // The foot: what the room last had to say.
        let fy = inner.max.y - row_h * 0.5;
        if let Some(status) = &self.utility.status {
            painter.text(
                egui::pos2(inner.min.x, fy),
                egui::Align2::LEFT_CENTER,
                status,
                font.clone(),
                c.fg,
            );
        }
        if let Some(audio) = &self.utility.audio_status {
            painter.text(
                egui::pos2(inner.max.x, fy),
                egui::Align2::RIGHT_CENTER,
                audio,
                font.clone(),
                c.dim,
            );
        }

        // A confirmation, over the rest, while one is pending.
        if let Some(confirm) = &self.utility.confirm {
            let (question, options): (&str, [&str; 3]) = match confirm {
                Confirm::Replace(_) => (
                    "the project has unsaved work",
                    ["SAVE + CONTINUE", "DISCARD + CONTINUE", "CANCEL"],
                ),
                Confirm::OverwriteProject(_) => (
                    "a project is already there",
                    ["OVERWRITE", "USE A NEW PATH", "CANCEL"],
                ),
                Confirm::OverwriteExport(_) => (
                    "a file is already there",
                    ["OVERWRITE", "USE A NEW PATH", "CANCEL"],
                ),
            };
            let w = 320.0;
            let h = HEAD_H + 3.0 * row_h + INSET * 2.0;
            let boxr = egui::Rect::from_center_size(panel.center(), egui::vec2(w, h));
            painter.rect_filled(boxr, 0.0, c.ground);
            chassis::frame(painter, boxr, true);
            let bi = boxr.shrink(INSET);
            painter.text(
                egui::pos2(bi.min.x, bi.min.y + HEAD_H * 0.5 - 2.0),
                egui::Align2::LEFT_CENTER,
                question,
                font.clone(),
                c.alert,
            );
            for (i, word) in options.iter().enumerate() {
                let y = bi.min.y + HEAD_H + i as f32 * row_h;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bi.min.x, y),
                    egui::pos2(bi.max.x, y + row_h),
                );
                let on = i == self.utility.confirm_row;
                if on {
                    painter.rect_filled(rect, 0.0, c.select);
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("confirm", i),
                        rect,
                        crate::ui::nav_cursor::Kind::Row,
                        crate::ui::nav_cursor::Layer::Utility,
                        c.alert,
                    );
                }
                painter.text(
                    egui::pos2(rect.min.x + 4.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *word,
                    font.clone(),
                    if on { c.bright } else { c.fg },
                );
            }
        }
    }
}
