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
///
/// This is not a taste: it is the width of the WIDEST section face, and
/// every cell is the same size, so every cell has to be that. A cell
/// narrower than the face it holds does not show a smaller card — it
/// shows a card with its words on top of each other, which is exactly
/// what the ledger refuses to let happen.
///
/// So the bento would rather show six devices properly than thirty-one
/// badly. It pages, and the reading order runs on across the break.
/// @tune 200..520 px
const CELL_MIN_W: f32 = 520.0;
/// And the height one needs before its instruments are worth drawing.
/// @tune 80..260 px
const CELL_MIN_H: f32 = 190.0;
/// One parameter row inside a cell.
/// @tune 8..20 px
const PARAM_H: f32 = 11.0;
const TYPE_PX: f32 = 12.0;

/// How the open chain is travelled.
///
/// The two share everything except the shape of the walk: same cells,
/// same size, same order, same faces. ROW keeps the signal in a line and
/// scrolls sideways; GRID wraps and pages. Which one is right depends on
/// whether you are following a signal or looking for something, so the
/// card does not choose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Flow {
    Row,
    Grid,
}

/// How many cells fit across `area`, and how wide each one is.
fn grid(area: egui::Rect, cells: usize) -> (usize, f32) {
    let across = ((area.width() + CELL_GAP) / (crate::tune!(CELL_MIN_W) + CELL_GAP))
        .floor()
        .max(1.0) as usize;
    let across = across.min(cells.max(1));
    let width = (area.width() - CELL_GAP * (across.saturating_sub(1)) as f32) / across as f32;
    (across, width)
}

/// How many rows of cells the page can show at the height a face needs.
///
/// A ROW walk has exactly one, whatever the height — which is what makes
/// its cells tall: the whole page belongs to the one line of devices.
fn rows_shown(area: egui::Rect, flow: Flow) -> usize {
    match flow {
        Flow::Row => 1,
        Flow::Grid => {
            (((area.height() + CELL_GAP) / (crate::tune!(CELL_MIN_H) + CELL_GAP)).floor()).max(1.0)
                as usize
        }
    }
}

/// The first row on the page, so the cursor's row is on it.
///
/// A page of twenty-nine devices at the size their faces were drawn to
/// does not fit a screen, and shrinking them until it does is how you
/// get a grid of unreadable cards. So the bento pages, and the reading
/// order runs on across the break.
fn first_row(area: egui::Rect, cells: usize, cursor: Option<usize>, flow: Flow) -> usize {
    let (across, _) = grid(area, cells);
    let lines = match flow {
        // A row walk has one line of every device, and "first" counts
        // along it rather than down.
        Flow::Row => cells.max(1),
        Flow::Grid => cells.div_ceil(across).max(1),
    };
    let shown = rows_shown(area, flow).min(lines);
    let step = match flow {
        Flow::Row => across.max(1),
        Flow::Grid => 1,
    };
    let on = match flow {
        Flow::Row => cursor.unwrap_or(0),
        Flow::Grid => cursor.map_or(0, |index| index / across),
    };
    match flow {
        // Keep the cursor's cell on screen, and never scroll past the
        // last full page.
        Flow::Row => on
            .saturating_sub(step.saturating_sub(1))
            .min(cells.saturating_sub(step)),
        Flow::Grid => on
            .saturating_sub(shown.saturating_sub(1))
            .min(lines.saturating_sub(shown)),
    }
}

/// Where the `index`th cell stands, given the first row on the page.
fn cell_rect(area: egui::Rect, cells: usize, first: usize, index: usize, flow: Flow) -> egui::Rect {
    let (across, width) = grid(area, cells);
    let shown = rows_shown(area, flow);
    let height = (area.height() - CELL_GAP * (shown.saturating_sub(1)) as f32) / shown as f32;
    let (col, row) = match flow {
        // One line: the walk is along it, so "first" slides the column.
        Flow::Row => (index as isize - first as isize, 0isize),
        Flow::Grid => (
            (index % across) as isize,
            index as isize / across as isize - first as isize,
        ),
    };
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
    pub(super) fn draw_bento(&self, painter: &egui::Painter, whole: egui::Rect, flow: Flow) {
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
        let phase = Phase::of(
            self.transport.motion().is_rolling(),
            self.transport.beat_phase(),
        );
        let level = self.playing_on(track).is_some().then_some(1.0);

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
        let first = first_row(area, columns.len(), cursor, flow);
        let (across, _) = grid(area, columns.len());
        let shown = rows_shown(area, flow);
        let on_page = |index: usize| match flow {
            Flow::Row => index >= first && index < first + across,
            Flow::Grid => {
                let row = index / across;
                row >= first && row < first + shown
            }
        };
        for (index, column) in columns.iter().enumerate() {
            if !on_page(index) {
                continue;
            }
            let cell = cell_rect(area, columns.len(), first, index, flow);
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
            {
                let (from, to) = match flow {
                    Flow::Row => (first, (first + across).min(columns.len())),
                    Flow::Grid => (
                        first * across,
                        ((first + shown) * across).min(columns.len()),
                    ),
                };
                format!(
                    "{:02}-{:02} of {} · first to last",
                    from + 1,
                    to,
                    columns.len()
                )
            },
            font.clone(),
            c.dim,
        );

        for (index, column) in columns.iter().enumerate() {
            if !on_page(index) {
                continue;
            }
            let cell = cell_rect(area, columns.len(), first, index, flow);
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
            // The section's OWN face, on the cell's glass. This is the
            // whole point of opening the chain out: the cards were drawn
            // to be read, and the bento is where there is room to read
            // them. A device that is not a console section — a chain
            // effect — has no face, and falls back to its parameters as
            // bars.
            if let Some(kind) = column.section {
                let glass = egui::Rect::from_min_max(
                    egui::pos2(cell.left() + 6.0, cell.top() + TYPE_PX + 8.0),
                    egui::pos2(cell.right() - 6.0, cell.bottom() - 6.0),
                );
                if glass.is_positive() {
                    let piece = strip::Piece {
                        index,
                        rect: cell,
                        kind,
                        notch: false,
                        tongue: false,
                    };
                    let selected = lattice
                        .cursor()
                        .filter(|(col, _)| *col == index)
                        .map(|(_, row)| row);
                    self.draw_figure(painter, piece, column, glass, selected, level, phase);
                }
                continue;
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
        if let Some(index) = cursor.filter(|index| on_page(*index)) {
            crate::ui::nav_cursor::claim(
                painter,
                ("stage-bento-cell", index),
                cell_rect(area, columns.len(), first, index, flow),
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
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 640.0))
    }

    /// The grid reads like a page: along a row, then down. This is the
    /// whole claim the presentation makes, so it is the thing to hold.
    #[test]
    fn the_cells_read_first_to_last_in_reading_order() {
        let (a, n) = (area(), 29usize);
        let (across, _) = grid(a, n);
        assert!(across >= 1);
        for i in 1..n {
            let (before, here) = (
                cell_rect(a, n, 0, i - 1, Flow::Grid),
                cell_rect(a, n, 0, i, Flow::Grid),
            );
            if i % across == 0 {
                assert!(here.left() <= before.left(), "row {i} did not return");
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

    /// Every cell is the same size. It has to be: a cell holds a section
    /// FACE, the faces were drawn to a width, and the widest of them
    /// sets it for all — a cell that shrank to fit its neighbour would
    /// be a card with its words on top of each other.
    #[test]
    fn the_cells_are_one_size_and_wide_enough_for_a_face() {
        let (a, n) = (area(), 29usize);
        let first = cell_rect(a, n, 0, 0, Flow::Grid);
        for i in 0..n {
            // To a tolerance: the positions accumulate a column at a
            // time, so two cells of the same size can differ in the last
            // bit of a float without differing on the glass.
            let size = cell_rect(a, n, 0, i, Flow::Grid).size();
            assert!(
                (size - first.size()).length() < 0.01,
                "cell {i} is a different size: {size:?} against {:?}",
                first.size()
            );
        }
        assert!(
            first.width() >= crate::tune!(CELL_MIN_W) - 0.01,
            "a cell came out narrower than the faces it holds"
        );
        assert!(first.height() >= crate::tune!(CELL_MIN_H) - 0.01);
        // A row of cells spans the area, less its gaps.
        let (across, width) = grid(a, n);
        let spanned = width * across as f32 + CELL_GAP * (across - 1) as f32;
        assert!((spanned - a.width()).abs() < 0.01, "the grid left a margin");
    }

    /// The page follows the cursor, and the reading order runs on across
    /// the break rather than restarting.
    #[test]
    fn the_page_follows_the_cursor() {
        let (a, n) = (area(), 29usize);
        let (across, _) = grid(a, n);
        let shown = rows_shown(a, Flow::Grid);
        let rows = n.div_ceil(across);
        assert!(rows > shown, "this page should need more than one screen");
        // Early on, the first row is the first row.
        assert_eq!(first_row(a, n, Some(0), Flow::Grid), 0);
        // At the end, the last page — and never past it.
        let last = first_row(a, n, Some(n - 1), Flow::Grid);
        assert_eq!(last, rows - shown);
        // Every cursor lands on a page that contains it.
        for index in 0..n {
            let first = first_row(a, n, Some(index), Flow::Grid);
            let row = index / across;
            assert!(
                row >= first && row < first + shown,
                "cell {index} fell off its own page"
            );
        }
    }

    /// The row walk keeps the signal in a line and slides along it: one
    /// row whatever the height, every cell on it, and the cursor always
    /// on screen.
    #[test]
    fn the_row_walk_stays_on_one_line() {
        let (a, n) = (area(), 29usize);
        assert_eq!(rows_shown(a, Flow::Row), 1);
        let (across, _) = grid(a, n);
        // Every cell shares a row, and each is a cell's width along.
        for i in 1..n {
            let before = cell_rect(a, n, 0, i - 1, Flow::Row);
            let here = cell_rect(a, n, 0, i, Flow::Row);
            assert!(
                (before.top() - here.top()).abs() < 0.01,
                "cell {i} left the line"
            );
            assert!(before.right() <= here.left() + 0.01, "cell {i} went back");
        }
        // A cell is taller here than in the grid: the whole page belongs
        // to one line, which is what the row walk buys.
        assert!(
            cell_rect(a, n, 0, 0, Flow::Row).height() > cell_rect(a, n, 0, 0, Flow::Grid).height()
        );
        // The walk follows the cursor and stops at the end.
        assert_eq!(first_row(a, n, Some(0), Flow::Row), 0);
        assert_eq!(first_row(a, n, Some(n - 1), Flow::Row), n - across);
        for index in 0..n {
            let first = first_row(a, n, Some(index), Flow::Row);
            assert!(
                index >= first && index < first + across,
                "cell {index} walked off its own screen"
            );
        }
    }

    /// One device is one cell, and a page too narrow for the smallest
    /// cell still lays out a column rather than dividing by zero.
    #[test]
    fn the_grid_survives_its_ends() {
        let a = area();
        assert_eq!(grid(a, 1).0, 1);
        let thin = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 300.0));
        assert_eq!(grid(thin, 8).0, 1);
        assert!(rows_shown(thin, Flow::Grid) >= 1);
        assert!(cell_rect(thin, 8, 0, 7, Flow::Grid).is_positive());
    }
}
