//! The desk block: the console's own rails, measured every frame and
//! until now shown nowhere. Four group buses, two returns and the mix,
//! each a hairline meter with its held peak — the one instrument on the
//! session that reads the desk rather than the song.
//!
//! It stands at the head of the right column, over the log, because the
//! two are the same kind of thing: what the machine is doing, said in
//! the machine's own terms.

use super::heads;
use super::palette;
use crate::PROFONT;
use crate::ui::stage::vitals;
use eframe::egui;

/// One rail's row.
/// @tune 8..24 px
const ROW_H: f32 = 14.0;
const TYPE_PX: f32 = 11.0;
/// The gap under the block, before the log's own header.
/// @tune 0..40 px
const FOOT: f32 = 28.0;
/// The meter's bar, inside its row.
/// @tune 2..12 px
const BAR_H: f32 = 4.0;

/// The rails, in the graph's slot order, with the name each goes by.
const RAILS: [(&str, usize); vitals::DESK_METERS] = [
    ("bus 1", vitals::BUS_METER),
    ("bus 2", vitals::BUS_METER + 1),
    ("bus 3", vitals::BUS_METER + 2),
    ("bus 4", vitals::BUS_METER + 3),
    ("ret a", vitals::RETURN_METER),
    ("ret b", vitals::RETURN_METER + 1),
    ("mix", vitals::MIX_METER),
];

/// The block's height, so the log knows where it may start.
pub(super) fn height() -> f32 {
    RAILS.len() as f32 * crate::tune!(ROW_H) + FOOT
}

impl super::super::Stage {
    pub(super) fn draw_desk(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some((x0, x1)) = super::log::column(self, field) else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let top = heads::master_rect(field).max.y + 12.0;
        painter.text(
            egui::pos2(x0, top - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "DESK",
            font.clone(),
            c.label,
        );
        let seam = top.round() - 0.5;
        painter.line_segment(
            [egui::pos2(x0, seam), egui::pos2(x1, seam)],
            egui::Stroke::new(1.0, c.rule),
        );
        // The bar runs from after the longest name to the column's edge,
        // so every rail's meter starts and ends on the same two lines.
        let bar_x0 = x0 + ch * 7.0;
        for (i, (name, slot)) in RAILS.iter().enumerate() {
            let y = top + (i as f32 + 0.5) * crate::tune!(ROW_H);
            let reading = self.meters.rail(*slot);
            let level = reading.level.left.max(reading.level.right);
            let peak = reading.peak.left.max(reading.peak.right);
            painter.text(
                egui::pos2(x0, y),
                egui::Align2::LEFT_CENTER,
                *name,
                font.clone(),
                if level > 0.0 { c.fg } else { c.dim },
            );
            let lane = egui::Rect::from_min_max(
                egui::pos2(bar_x0, y - BAR_H * 0.5),
                egui::pos2(x1, y + BAR_H * 0.5),
            );
            painter.rect_filled(lane, 0.0, c.rule);
            if level > 0.0 {
                let fill = egui::Rect::from_min_max(
                    lane.min,
                    egui::pos2(
                        lane.min.x + lane.width() * level.clamp(0.0, 1.0),
                        lane.max.y,
                    ),
                );
                painter.rect_filled(fill, 0.0, if level >= 1.0 { c.alert } else { c.nominal });
            }
            // The held peak: a tick standing on the lane where the rail
            // last got to, alert once it has reached the top.
            if peak > 0.0 {
                let x = lane.min.x + lane.width() * peak.clamp(0.0, 1.0);
                painter.line_segment(
                    [
                        egui::pos2(x, lane.min.y - 1.0),
                        egui::pos2(x, lane.max.y + 1.0),
                    ],
                    egui::Stroke::new(1.0, if peak >= 1.0 { c.alert } else { c.fg }),
                );
            }
        }
    }
}
