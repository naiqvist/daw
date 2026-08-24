//! Squiggle grid: a block of small toggleable cells at any division count
//! (3×2, 2×2, 6×1, ...). Each cell is a darker well holding a squiggle —
//! the placeholder glyph for "a division with content" — drawn darker
//! still when the cell is disabled, in the accent when enabled.
//!
//! The widget speaks `&mut [bool]`, row-major, one flag per cell; the
//! caller owns what a cell MEANS (a step, a slot, a voice). Cells beyond
//! the slice's length draw as permanently-off filler and cannot be
//! toggled.

use crate::ui::theme::Theme;
use crate::ui::tokens::{control, radius, space, stroke};
use eframe::egui;

/// Steps in one squiggle polyline.
const SQUIGGLE_STEPS: usize = 24;
/// Full sine cycles across one cell.
const SQUIGGLE_CYCLES: f32 = 2.0;
/// Squiggle amplitude as a fraction of cell height.
const SQUIGGLE_AMP: f32 = 0.25;

/// Draw a `cols`×`rows` grid of toggleable squiggle cells. Click toggles a
/// cell; returns true when any cell changed this frame.
pub fn squiggle_grid(
    ui: &mut egui::Ui,
    theme: &Theme,
    cols: usize,
    rows: usize,
    on: &mut [bool],
) -> bool {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let cell = theme.sp(control::CELL);
    let gap = theme.sp(space::XS);
    let size = egui::vec2(
        cols as f32 * cell + (cols - 1) as f32 * gap,
        rows as f32 * cell + (rows - 1) as f32 * gap,
    );
    let (rect, grid_response) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Cell ids hang off THIS allocation's id, not the parent scope's:
    // two grids in one row must never mint the same ("squiggle", r, c).
    let base_id = grid_response.id;

    let mut changed = false;
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            let cell_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(c as f32 * (cell + gap), r as f32 * (cell + gap)),
                egui::vec2(cell, cell),
            );

            let live = i < on.len();
            let mut enabled = live && on[i];
            if live {
                let response = ui.interact(
                    cell_rect,
                    base_id.with(("squiggle", r, c)),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    enabled = !enabled;
                    on[i] = enabled;
                    changed = true;
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }

            paint_cell(ui, theme, cell_rect, enabled);
        }
    }
    changed
}

fn paint_cell(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, enabled: bool) {
    let painter = ui.painter();

    // The well: darker than the card the grid sits on.
    painter.rect_filled(rect, radius::CTRL, theme.surface_sunken);
    painter.rect_stroke(
        rect,
        radius::CTRL,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    // The squiggle: darker still when off, accent when on.
    let inner = rect.shrink(theme.sp(space::XS));
    let amp = inner.height() * SQUIGGLE_AMP;
    let points: Vec<egui::Pos2> = (0..=SQUIGGLE_STEPS)
        .map(|i| {
            let t = i as f32 / SQUIGGLE_STEPS as f32;
            let a = t * SQUIGGLE_CYCLES * std::f32::consts::TAU;
            egui::pos2(
                inner.left() + inner.width() * t,
                inner.center().y - a.sin() * amp,
            )
        })
        .collect();
    let color = if enabled { theme.accent } else { theme.divider };
    let weight = if enabled { stroke::BOLD } else { stroke::HAIR };
    painter.add(egui::Shape::line(points, egui::Stroke::new(weight, color)));
}
