//! Small declarative layout grid for dense redesign panels.
//!
//! A panel defines its column/row count once. Children describe only where
//! they live and how many cells they span, so rearranging an inspector is a
//! data change rather than a new nest of `horizontal`/`vertical` closures.

use eframe::egui;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GridArea {
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) column_span: usize,
    pub(crate) row_span: usize,
}

impl GridArea {
    pub(crate) const fn new(
        column: usize,
        row: usize,
        column_span: usize,
        row_span: usize,
    ) -> Self {
        Self {
            column,
            row,
            column_span,
            row_span,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutGrid {
    bounds: egui::Rect,
    columns: usize,
    rows: usize,
    gap: egui::Vec2,
}

impl LayoutGrid {
    pub(crate) fn new(bounds: egui::Rect, columns: usize, rows: usize, gap: egui::Vec2) -> Self {
        Self {
            bounds,
            columns: columns.max(1),
            rows: rows.max(1),
            gap,
        }
    }

    pub(crate) fn area(self, area: GridArea) -> egui::Rect {
        let column = area.column.min(self.columns - 1);
        let row = area.row.min(self.rows - 1);
        let column_span = area.column_span.max(1).min(self.columns - column);
        let row_span = area.row_span.max(1).min(self.rows - row);

        let cell_width = ((self.bounds.width() - self.gap.x * (self.columns - 1) as f32)
            / self.columns as f32)
            .max(0.0);
        let cell_height = ((self.bounds.height() - self.gap.y * (self.rows - 1) as f32)
            / self.rows as f32)
            .max(0.0);
        let pitch = egui::vec2(cell_width + self.gap.x, cell_height + self.gap.y);
        let min = self.bounds.min + egui::vec2(column as f32 * pitch.x, row as f32 * pitch.y);
        let size = egui::vec2(
            cell_width * column_span as f32 + self.gap.x * (column_span - 1) as f32,
            cell_height * row_span as f32 + self.gap.y * (row_span - 1) as f32,
        );
        egui::Rect::from_min_size(min, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_areas_honor_position_gap_and_span() {
        let grid = LayoutGrid::new(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
            2,
            2,
            egui::vec2(10.0, 10.0),
        );

        assert_eq!(
            grid.area(GridArea::new(1, 0, 1, 1)),
            egui::Rect::from_min_size(egui::pos2(55.0, 0.0), egui::vec2(45.0, 45.0))
        );
        assert_eq!(
            grid.area(GridArea::new(0, 1, 2, 1)),
            egui::Rect::from_min_size(egui::pos2(0.0, 55.0), egui::vec2(100.0, 45.0))
        );
    }
}
