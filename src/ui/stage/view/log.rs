//! The log column: what the machine has been doing, in the field's
//! empty right, newest at the foot and brightest. Every row names its
//! measured uptime and its severity before the event, so attention does
//! not depend on reading the prose first.

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

fn severity_colour(severity: telemetry::Severity, c: palette::Colours) -> egui::Color32 {
    match severity {
        telemetry::Severity::Info => c.label,
        telemetry::Severity::Live => c.nominal,
        telemetry::Severity::Attention => c.alert,
    }
}

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
        let body_top = top + crate::tune!(ROW_H);
        let rows = ((bottom - body_top) / crate::tune!(ROW_H)).floor().max(0.0) as usize;
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
        // The columns are part of the instrument: UPTIME is this view's
        // clock, SEV is assigned where the fact is observed, and the
        // specific event and its object remain separate fields.
        let header_y = top + crate::tune!(ROW_H) * 0.5;
        for (offset, word) in [
            (0.0, "UPTIME"),
            (8.0, "SEV"),
            (14.0, "EVENT"),
            (23.5, "DETAIL"),
        ] {
            painter.text(
                egui::pos2(x0 + offset * ch, header_y),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                c.label,
            );
        }
        let header_seam = body_top.round() - 0.5;
        painter.line_segment(
            [egui::pos2(x0, header_seam), egui::pos2(x1, header_seam)],
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
                line.severity.word(),
                font.clone(),
                severity_colour(line.severity, c),
            );
            painter.text(
                egui::pos2(x0 + 14.0 * ch, y),
                egui::Align2::LEFT_CENTER,
                line.verb,
                font.clone(),
                if i == 0 { c.label } else { ink },
            );
            painter.text(
                egui::pos2(x0 + 23.5 * ch, y),
                egui::Align2::LEFT_CENTER,
                &line.what,
                font.clone(),
                ink,
            );
        }
    }
}
