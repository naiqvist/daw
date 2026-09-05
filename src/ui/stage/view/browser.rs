//! The browser: the archive, as a panel over the field's left.
//!
//! A tree of shelves — devices, samples, projects — filtered by what has
//! been typed, read straight from `stage::Browser`: its rows, its cursor,
//! its query, what survives the filter. It owns the keys while it is
//! open, so it wears the focused chassis. A directory is blue, content
//! is body text, the characters the query matched are bright, and the
//! cursor's row sits on `select` ground with brackets. No slide: a
//! panel that arrives is reporting where the keys went, not decorating.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::browser::{EntryKind, match_positions};
use eframe::egui;

/// The panel's width.
/// @tune 200..600 px
const WIDTH: f32 = 340.0;
/// One row of the tree.
/// @tune 12..32 px
const ROW_H: f32 = 18.0;
/// Indent per level of the tree.
/// @tune 6..24 px
const INDENT: f32 = 12.0;
const TITLE_H: f32 = 26.0;
const INSET: f32 = 10.0;
const TYPE_PX: f32 = 12.0;

impl super::super::Stage {
    pub(super) fn draw_browser(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some(browser) = self.browser.as_ref() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let panel = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin, field.min.y + margin),
            egui::pos2(
                field.min.x + margin + crate::tune!(WIDTH),
                field.max.y - margin,
            ),
        );
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET);

        // The title row: what this is, what has been typed, what survived.
        let ty = inner.min.y + TITLE_H * 0.5;
        painter.text(
            egui::pos2(inner.min.x, ty),
            egui::Align2::LEFT_CENTER,
            "ARCHIVE",
            font.clone(),
            c.label,
        );
        let query = browser.query();
        let qx = inner.min.x + 8.0 * ch;
        painter.text(
            egui::pos2(qx, ty),
            egui::Align2::LEFT_CENTER,
            query,
            font.clone(),
            c.bright,
        );
        // The caret: where the next letter lands.
        let caret_x = qx + query.chars().count() as f32 * ch;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(caret_x, ty - 6.0),
                egui::pos2(caret_x + 1.5, ty + 6.0),
            ),
            0.0,
            c.alert,
        );
        let count = if self.library_scanning {
            ("scanning".to_owned(), c.alert)
        } else {
            (format!("{}", browser.surviving_leaves()), c.dim)
        };
        painter.text(
            egui::pos2(inner.max.x, ty),
            egui::Align2::RIGHT_CENTER,
            count.0,
            font.clone(),
            count.1,
        );
        let seam_y = (inner.min.y + TITLE_H).round() - 0.5;
        painter.line_segment(
            [
                egui::pos2(inner.min.x, seam_y),
                egui::pos2(inner.max.x, seam_y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );

        // The tree, its window kept around the cursor.
        let rows = browser.rows();
        let top = inner.min.y + TITLE_H + 4.0;
        let row_h = crate::tune!(ROW_H);
        let capacity = ((inner.max.y - top) / row_h).floor().max(1.0) as usize;
        let cursor = browser.cursor();
        let first = cursor
            .map(|cur| cur.saturating_sub(capacity / 2))
            .unwrap_or(0)
            .min(rows.len().saturating_sub(capacity));
        for (i, row) in rows.iter().enumerate().skip(first).take(capacity) {
            let Some(node) = browser.node_at(&row.path) else {
                continue;
            };
            let y = top + (i - first) as f32 * row_h;
            let rect = egui::Rect::from_min_max(
                egui::pos2(inner.min.x, y),
                egui::pos2(inner.max.x, y + row_h),
            );
            let on = cursor == Some(i);
            if on {
                painter.rect_filled(rect, 0.0, c.select);
            }
            let x = inner.min.x + row.depth as f32 * crate::tune!(INDENT);
            let cy = rect.center().y;
            let is_dir = matches!(node.kind, EntryKind::Shelf(_) | EntryKind::Group);
            if node.is_branch() {
                painter.text(
                    egui::pos2(x, cy),
                    egui::Align2::LEFT_CENTER,
                    if node.expanded { "-" } else { "+" },
                    font.clone(),
                    c.rule,
                );
            }
            let lx = x + 2.0 * ch;
            let word = if is_dir { c.dir } else { c.fg };
            painter.text(
                egui::pos2(lx, cy),
                egui::Align2::LEFT_CENTER,
                &node.label,
                font.clone(),
                word,
            );
            // The letters the query matched, over the label, bright.
            if !query.is_empty() {
                for (k, (glyph, hit)) in node
                    .label
                    .chars()
                    .zip(match_positions(&node.label, query))
                    .enumerate()
                {
                    if hit {
                        painter.text(
                            egui::pos2(lx + k as f32 * ch, cy),
                            egui::Align2::LEFT_CENTER,
                            glyph.to_string(),
                            font.clone(),
                            c.bright,
                        );
                    }
                }
            }
            if is_dir && node.is_branch() {
                painter.text(
                    egui::pos2(inner.max.x, cy),
                    egui::Align2::RIGHT_CENTER,
                    node.leaves().to_string(),
                    font.clone(),
                    c.dim,
                );
            }
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("archive", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Surface,
                    c.alert,
                );
            }
        }
        if rows.len() > first + capacity {
            painter.text(
                egui::pos2(inner.max.x, inner.max.y - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{} more", rows.len() - first - capacity),
                font,
                c.dim,
            );
        }
    }
}
