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
use crate::ui::stage::browser::{Browser, EntryKind, match_positions};
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
/// The selected row's path back through the archive.
/// @tune 14..40 px
const FOOT_H: f32 = 22.0;
const INSET: f32 = 10.0;
const TYPE_PX: f32 = 12.0;

fn clamped_width(field_width: f32, margin: f32, wanted: f32) -> f32 {
    (field_width - margin.max(0.0) * 2.0)
        .max(0.0)
        .min(wanted.max(0.0))
}

fn tail_chars(text: &str, capacity: usize) -> String {
    let count = text.chars().count();
    if count <= capacity {
        return text.to_owned();
    }
    if capacity <= 2 {
        return "<".repeat(capacity);
    }
    let tail: String = text.chars().skip(count - (capacity - 2)).collect();
    format!("< {tail}")
}

fn ancestry(browser: &Browser, path: &[usize]) -> String {
    (0..path.len())
        .filter_map(|depth| browser.node_at(&path[..=depth]))
        .map(|node| node.label.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

impl super::super::Stage {
    pub(super) fn draw_browser(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some(browser) = self.browser.as_ref() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let available_w = clamped_width(field.width(), margin, f32::INFINITY);
        if available_w < INSET * 2.0 + ch * 8.0 || field.height() < TITLE_H + FOOT_H + 20.0 {
            return;
        }
        let panel_w = clamped_width(field.width(), margin, crate::tune!(WIDTH));
        let panel = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin, field.min.y + margin),
            egui::pos2(field.min.x + margin + panel_w, field.max.y - margin),
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
        let find_x = inner.min.x + 9.0 * ch;
        painter.text(
            egui::pos2(find_x, ty),
            egui::Align2::LEFT_CENTER,
            "FIND",
            font.clone(),
            c.label,
        );
        let qx = find_x + 5.0 * ch;
        let count = if self.library_scanning {
            ("YIELD --  SCAN".to_owned(), c.alert)
        } else {
            (format!("YIELD {:03}", browser.surviving_leaves()), c.dim)
        };
        let count_w = count.0.chars().count() as f32 * ch;
        let query_capacity = ((inner.max.x - count_w - ch - qx) / ch).floor().max(0.0) as usize;
        let shown_query = tail_chars(query, query_capacity);
        painter.text(
            egui::pos2(if query.is_empty() { qx + 2.0 * ch } else { qx }, ty),
            egui::Align2::LEFT_CENTER,
            if query.is_empty() { "--" } else { &shown_query },
            font.clone(),
            if query.is_empty() { c.dim } else { c.bright },
        );
        // The caret: where the next letter lands.
        let caret_x = qx + shown_query.chars().count() as f32 * ch;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(caret_x, ty - 6.0),
                egui::pos2(caret_x + 1.5, ty + 6.0),
            ),
            0.0,
            c.alert,
        );
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
        let most = rows
            .iter()
            .filter_map(|r| browser.node_at(&r.path))
            .filter(|n| n.is_branch())
            .map(|n| n.leaves())
            .max()
            .unwrap_or(0);
        let top = inner.min.y + TITLE_H + 4.0;
        let footer_top = inner.max.y - crate::tune!(FOOT_H);
        let row_h = crate::tune!(ROW_H);
        let capacity = ((footer_top - top) / row_h).floor().max(1.0) as usize;
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
            // One elbow back toward the parent depth: ancestry carried by
            // structure, not an ornamental tree repeated behind every row.
            if row.depth > 0 {
                let parent_x =
                    inner.min.x + (row.depth - 1) as f32 * crate::tune!(INDENT) + ch * 0.5;
                let child_x = x - ch * 0.5;
                painter.line_segment(
                    [egui::pos2(parent_x, rect.min.y), egui::pos2(parent_x, cy)],
                    egui::Stroke::new(1.0, c.rule),
                );
                painter.line_segment(
                    [egui::pos2(parent_x, cy), egui::pos2(child_x, cy)],
                    egui::Stroke::new(1.0, c.rule),
                );
            }
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
                // The count as a small meter against the fullest shelf.
                if most > 0 {
                    let w = 24.0;
                    let x1 = inner.max.x - 4.0 * ch;
                    let track = egui::Rect::from_min_max(
                        egui::pos2(x1 - w, cy - 1.0),
                        egui::pos2(x1, cy + 1.0),
                    );
                    painter.rect_filled(track, 0.0, c.rule);
                    let share = node.leaves() as f32 / most as f32;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            track.min,
                            egui::pos2(track.min.x + w * share, track.max.y),
                        ),
                        0.0,
                        c.edge,
                    );
                }
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
                egui::pos2(inner.max.x, footer_top - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{} more", rows.len() - first - capacity),
                font.clone(),
                c.dim,
            );
        }

        // The cursor's full filing path. Filtering opens branches on the
        // reader's behalf, so indentation alone is not enough to say where
        // a surviving leaf came from.
        let seam_y = footer_top.round() - 0.5;
        painter.line_segment(
            [
                egui::pos2(inner.min.x, seam_y),
                egui::pos2(inner.max.x, seam_y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let path_x = inner.min.x + 6.0 * ch;
        let path_capacity = ((inner.max.x - path_x) / ch).floor().max(0.0) as usize;
        let path = cursor
            .and_then(|index| rows.get(index))
            .map(|row| ancestry(browser, &row.path))
            .unwrap_or_else(|| "--".to_owned());
        let path = tail_chars(&path, path_capacity);
        let path_y = footer_top + crate::tune!(FOOT_H) * 0.5;
        painter.text(
            egui::pos2(inner.min.x, path_y),
            egui::Align2::LEFT_CENTER,
            "PATH",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(path_x, path_y),
            egui::Align2::LEFT_CENTER,
            path,
            font,
            c.dir,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_title_keeps_the_end_being_typed() {
        assert_eq!(tail_chars("compressor", 6), "< ssor");
        assert_eq!(tail_chars("room", 6), "room");
    }

    #[test]
    fn panel_width_never_crosses_the_field_margin() {
        assert_eq!(clamped_width(320.0, 12.0, 340.0), 296.0);
        assert_eq!(clamped_width(640.0, 12.0, 340.0), 340.0);
        assert_eq!(clamped_width(10.0, 12.0, 340.0), 0.0);
    }

    #[test]
    fn ancestry_names_each_real_node_on_the_selected_path() {
        let mut browser = Browser::shelves();
        assert!(browser.step(crate::ui::stage::grid::Step::Right));
        assert!(browser.step(crate::ui::stage::grid::Step::Right));
        let rows = browser.rows();
        let row = rows
            .get(browser.cursor().unwrap_or(0))
            .expect("selected row");
        let path = ancestry(&browser, &row.path);
        assert!(path.starts_with("Devices / "), "{path}");
    }
}
