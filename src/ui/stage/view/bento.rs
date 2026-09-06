//! The BENTO: the whole signal path opened out, read like a page.
//!
//! The rail puts the chain in one long line and the dock puts it in one
//! small line. The bento puts it in a GRID — first to last in reading
//! order, left to right and then down — so a run of twenty-nine devices
//! is read the way a page is read rather than scrolled through like a
//! corridor.
//!
//! It takes nearly the whole window because that is the point: the
//! session is not what you are looking at while you are working on a
//! chain, and pretending otherwise is what made the rail a corridor in
//! the first place.
//!
//! Each cell carries what fits at that size — the device's word, its
//! rail, whether it is IN, and the first few of its parameters as bars.
//! The cursor's cell wears the brackets and is the one thing on the page
//! at full ink.
//!
//! The order is the same order the rail walks and the dock lays out.
//! Three shapes, one sequence: the presentations differ in how much room
//! they give a device, never in which device comes next.

use super::palette;
use super::*;
use crate::PROFONT;
use crate::ui::stage::chain;

/// How much of the window the bento takes.
/// @tune 0.6..1.0
const COVER: f32 = 0.94;
/// The gap between cells.
/// @tune 2..20 px
const CELL_GAP: f32 = 6.0;
/// The smallest a cell may be before the grid takes a column away.
/// @tune 80..260 px
const CELL_MIN_W: f32 = 132.0;
/// One parameter row inside a cell.
/// @tune 8..20 px
const PARAM_H: f32 = 11.0;
const TYPE_PX: f32 = 12.0;

/// How many cells fit across `area`, and how wide each one is.
fn grid(area: egui::Rect, cells: usize) -> (usize, f32) {
    let across = ((area.width() + CELL_GAP) / (crate::tune!(CELL_MIN_W) + CELL_GAP))
        .floor()
        .max(1.0) as usize;
    let across = across.min(cells.max(1));
    let width = (area.width() - CELL_GAP * (across.saturating_sub(1)) as f32) / across as f32;
    (across, width)
}

/// Where the `index`th cell stands.
fn cell_rect(area: egui::Rect, cells: usize, index: usize) -> egui::Rect {
    let (across, width) = grid(area, cells);
    let rows = cells.div_ceil(across).max(1);
    let height = (area.height() - CELL_GAP * (rows.saturating_sub(1)) as f32) / rows as f32;
    let (col, row) = (index % across, index / across);
    egui::Rect::from_min_size(
        egui::pos2(
            area.left() + col as f32 * (width + CELL_GAP),
            area.top() + row as f32 * (height + CELL_GAP),
        ),
        egui::vec2(width, height),
    )
}

impl super::super::Stage {
    /// Draw the bento over the whole window.
    pub(super) fn draw_bento(&self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(lattice) = self.chain.as_ref() else {
            return;
        };
        let Some(track) = self.addressed_track() else {
            return;
        };
        let columns = chain::band(&self.song, track);
        if columns.is_empty() {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let micro = egui::FontId::new(TYPE_PX - 2.0, egui::FontFamily::Name(PROFONT.into()));
        let cursor = lattice.cursor().map(|(col, _)| col);

        // The page holds the field down behind it: this is a view of one
        // thing, not a window over another.
        let cover = crate::tune!(COVER).clamp(0.5, 1.0);
        let page = egui::Rect::from_center_size(whole.center(), whole.size() * cover);
        painter.rect_filled(whole, 0.0, c.ground.gamma_multiply(0.92));
        let mut shapes = Vec::new();
        chrome::panel_variant(
            &mut shapes,
            page,
            Some(c.ground),
            c.chassis,
            Some((Weight::Hair, c.rule)),
            0,
        );
        let head_h = TYPE_PX + 8.0;
        let area = egui::Rect::from_min_max(
            egui::pos2(page.left() + 10.0, page.top() + head_h + 6.0),
            egui::pos2(page.right() - 10.0, page.bottom() - 10.0),
        );
        if !area.is_positive() {
            return;
        }
        for (index, column) in columns.iter().enumerate() {
            let cell = cell_rect(area, columns.len(), index);
            let on = cursor == Some(index);
            chrome::panel_variant(
                &mut shapes,
                cell,
                // A device that is OUT must not be the loudest thing on
                // the page: the ground recedes, the panel comes forward.
                Some(if column.bypassed { c.ground } else { c.panel }),
                c.ground,
                Some((
                    if on { Weight::Heavy } else { Weight::Hair },
                    if on { c.alert } else { c.rule },
                )),
                (index % 4) as u8,
            );
            // The rail, as a bar down the cell's left edge: the channel's
            // own run is unmarked, and everything after it is not.
            let rail = match column.lane {
                chain::Lane::Channel => None,
                chain::Lane::Bus(_) => Some(c.label),
                chain::Lane::Mix => Some(c.nominal),
                chain::Lane::Return(_) => Some(c.alert),
            };
            if let Some(ink) = rail {
                shapes.push(egui::Shape::rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(cell.left() + 2.0, cell.top() + 6.0),
                        egui::pos2(cell.left() + 4.0, cell.bottom() - 6.0),
                    ),
                    0.0,
                    ink,
                ));
            }
        }
        painter.extend(shapes);

        // The head: which track's path this is, and how long.
        painter.text(
            egui::pos2(page.left() + 12.0, page.top() + 6.0),
            egui::Align2::LEFT_TOP,
            format!("BAND tr {:02}", track + 1),
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(page.right() - 12.0, page.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            format!("{} devices · first to last", columns.len()),
            font.clone(),
            c.dim,
        );

        for (index, column) in columns.iter().enumerate() {
            let cell = cell_rect(area, columns.len(), index);
            let on = cursor == Some(index);
            let word_ink = if column.bypassed {
                c.dim
            } else if on {
                c.fg
            } else {
                c.ink
            };
            painter.text(
                egui::pos2(cell.left() + 8.0, cell.top() + 5.0),
                egui::Align2::LEFT_TOP,
                column.code.to_ascii_uppercase(),
                font.clone(),
                word_ink,
            );
            // Its number in the path, so the reading order is stated and
            // not merely implied by where a cell happens to sit.
            painter.text(
                egui::pos2(cell.right() - 6.0, cell.top() + 5.0),
                egui::Align2::RIGHT_TOP,
                format!("{:02}", index + 1),
                micro.clone(),
                c.rule,
            );
            if column.bypassed {
                painter.text(
                    egui::pos2(cell.right() - 6.0, cell.bottom() - 4.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "OUT",
                    micro.clone(),
                    c.dim,
                );
            }
            // As many parameters as the cell has room for, as bars: at
            // this size a value is a length, not a figure.
            let rows = (((cell.height() - TYPE_PX - 16.0) / PARAM_H).floor()).max(0.0) as usize;
            for (row, param) in column.rows.iter().take(rows).enumerate() {
                let y = cell.top() + TYPE_PX + 10.0 + row as f32 * PARAM_H;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(cell.left() + 8.0, y + 2.0),
                    egui::pos2(cell.right() - 8.0, y + 5.0),
                );
                if !bar.is_positive() {
                    break;
                }
                painter.rect_filled(bar, 0.0, c.rule.gamma_multiply(0.6));
                if param.place > 0.0 {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            bar.min,
                            egui::pos2(
                                bar.left() + param.place.clamp(0.0, 1.0) * bar.width(),
                                bar.bottom(),
                            ),
                        ),
                        0.0,
                        if column.bypassed {
                            c.rule
                        } else if param.edited {
                            c.nominal
                        } else {
                            c.ink.gamma_multiply(0.7)
                        },
                    );
                }
            }
        }
        if let Some(index) = cursor {
            crate::ui::nav_cursor::claim(
                painter,
                ("stage-bento-cell", index),
                cell_rect(area, columns.len(), index),
                crate::ui::nav_cursor::Kind::Cell,
                crate::ui::nav_cursor::Layer::Overlay,
                c.alert,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 700.0))
    }

    /// The grid reads like a page: along a row, then down. This is the
    /// whole claim the presentation makes, so it is the thing to hold.
    #[test]
    fn the_cells_read_first_to_last_in_reading_order() {
        let (a, n) = (area(), 29usize);
        let (across, _) = grid(a, n);
        assert!(across > 1);
        for i in 1..n {
            let (before, here) = (cell_rect(a, n, i - 1), cell_rect(a, n, i));
            if i % across == 0 {
                // A new row starts at the left, below the last one.
                assert!(here.left() < before.left(), "row {i} did not return");
                assert!(here.top() > before.top(), "row {i} did not descend");
            } else {
                assert!(before.right() <= here.left() + 0.01, "cell {i} went back");
                assert!(
                    (before.top() - here.top()).abs() < 0.01,
                    "cell {i} left its row"
                );
            }
        }
    }

    /// Every cell is the same size and the grid fills its area: a bento
    /// with a ragged last row is still a bento, but one whose cells
    /// changed size by position would not be a grid at all.
    #[test]
    fn the_cells_are_one_size_and_fill_the_page() {
        let (a, n) = (area(), 29usize);
        let first = cell_rect(a, n, 0);
        for i in 0..n {
            assert_eq!(cell_rect(a, n, i).size(), first.size(), "cell {i} differs");
            assert!(
                a.expand(0.01).contains_rect(cell_rect(a, n, i)),
                "cell {i} left the page"
            );
        }
        // The row of cells spans the area, less its gaps.
        let (across, width) = grid(a, n);
        let spanned = width * across as f32 + CELL_GAP * (across - 1) as f32;
        assert!((spanned - a.width()).abs() < 0.01, "the grid left a margin");
    }

    /// One device is one cell filling the page, and a page too narrow
    /// for the smallest cell still lays out a column.
    #[test]
    fn the_grid_survives_its_ends() {
        let a = area();
        assert_eq!(grid(a, 1).0, 1);
        assert!((cell_rect(a, 1, 0).width() - a.width()).abs() < 0.01);
        let thin = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 300.0));
        assert_eq!(grid(thin, 8).0, 1);
        assert!(cell_rect(thin, 8, 7).is_positive());
    }
}
