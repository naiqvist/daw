//! The log column: what the machine has been doing, in the field's
//! empty right, newest at the foot and brightest.

use super::heads;
use super::{palette, telemetry};
use crate::PROFONT;
use eframe::egui;

/// The column's width.
/// @tune 160..480 px
const WIDTH: f32 = 300.0;
/// One line.
/// @tune 10..24 px
const ROW_H: f32 = 15.0;
const TYPE_PX: f32 = 11.0;

/// The right column both the desk block and the log stand in: right of
/// the lattice, left of the master, or nothing at all when the field is
/// too narrow to give it room.
pub(super) fn column(stage: &super::super::Stage, field: egui::Rect) -> Option<(f32, f32)> {
    let margin = heads::margin();
    let master = heads::master_rect(field);
    let lattice_right =
        super::lattice::cell(field, stage.shown_tracks(field.width()).len().max(1) - 1, 0)
            .max
            .x;
    let x0 = (lattice_right + 180.0).max(field.min.x + field.width() * 0.5);
    let x1 = master.min.x - margin;
    if x1 - x0 < crate::tune!(WIDTH) * 0.6 {
        return None;
    }
    Some((x1 - crate::tune!(WIDTH).min(x1 - x0), x1))
}

impl super::super::Stage {
    pub(super) fn draw_log(&self, painter: &egui::Painter, field: egui::Rect) {
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let master = heads::master_rect(field);
        let Some((x0, x1)) = column(self, field) else {
            return;
        };
        // The desk block has the head of the column; the log begins
        // under it.
        let top = master.max.y + 12.0 + super::desk::height();
        let bottom = field.max.y - margin;
        let rows = ((bottom - top) / crate::tune!(ROW_H)).floor().max(1.0) as usize;
        let lines: Vec<telemetry::Line> = {
            let t = super::telemetry();
            t.lines.iter().rev().take(rows).cloned().collect()
        };
        painter.text(
            egui::pos2(x0, top - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "LOG",
            font.clone(),
            c.label,
        );
        let seam = top.round() - 0.5;
        painter.line_segment(
            [egui::pos2(x0, seam), egui::pos2(x1, seam)],
            egui::Stroke::new(1.0, c.rule),
        );
        // Newest at the foot, brightest; older lines climb and fade.
        for (i, line) in lines.iter().enumerate() {
            let y = bottom - (i as f32 + 0.5) * crate::tune!(ROW_H);
            let ink = if i == 0 {
                c.fg
            } else if i < 6 {
                c.dim
            } else {
                c.edge
            };
            painter.text(
                egui::pos2(x0, y),
                egui::Align2::LEFT_CENTER,
                telemetry::stamp(line.at),
                font.clone(),
                c.rule,
            );
            painter.text(
                egui::pos2(x0 + 8.0 * ch, y),
                egui::Align2::LEFT_CENTER,
                line.verb,
                font.clone(),
                if i == 0 { c.label } else { ink },
            );
            painter.text(
                egui::pos2(x0 + 16.0 * ch, y),
                egui::Align2::LEFT_CENTER,
                &line.what,
                font.clone(),
                ink,
            );
        }
    }
}
