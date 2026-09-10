//! The desk block: the console's own rails, measured every frame and
//! until now shown nowhere. Four group buses, two returns and the mix,
//! each a stereo pair of hairline meters with its held peaks — the one
//! instrument on the session that reads the desk rather than the song.
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
/// A real division of the desk: group buses, returns, or the main mix.
/// @tune 8..24 px
const GROUP_H: f32 = 12.0;
const TYPE_PX: f32 = 11.0;
/// The gap under the block, before the log's own header.
/// @tune 0..40 px
const FOOT: f32 = 20.0;
/// The meter's bar, inside its row.
/// @tune 2..12 px
const BAR_H: f32 = 3.0;
/// The header band over the block: DESK, and the L and R that name the
/// two lanes under it. Measured off the mockup's own raster at 1280x800,
/// where the band runs y 120..147 against the app's former 121..136.
/// @tune 12..48 px
const HEADER_H: f32 = 28.0;
/// Between the master plate's foot and the block's first content row.
/// The header band grows upward from there, so this sets where the whole
/// block sits: the mockup's content starts at y 148, the app's at 138.
/// @tune 0..48 px
const MASTER_GAP: f32 = 22.0;

const BUSES: [(&str, usize); 4] = [
    ("01", vitals::BUS_METER),
    ("02", vitals::BUS_METER + 1),
    ("03", vitals::BUS_METER + 2),
    ("04", vitals::BUS_METER + 3),
];
const RETURNS: [(&str, usize); 2] = [("A", vitals::RETURN_METER), ("B", vitals::RETURN_METER + 1)];
const MAIN: [(&str, usize); 1] = [("MIX", vitals::MIX_METER)];

struct Group {
    name: &'static str,
    rails: &'static [(&'static str, usize)],
}

const GROUPS: [Group; 3] = [
    Group {
        name: "GROUP BUS",
        rails: &BUSES,
    },
    Group {
        name: "RETURNS",
        rails: &RETURNS,
    },
    Group {
        name: "MAIN",
        rails: &MAIN,
    },
];

fn meter_lane(
    painter: &egui::Painter,
    lane: egui::Rect,
    level: f32,
    peak: f32,
    c: palette::Colours,
) {
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

/// The block's height, so the log knows where it may start.
pub(super) fn height() -> f32 {
    vitals::DESK_METERS as f32 * crate::tune!(ROW_H)
        + GROUPS.len() as f32 * crate::tune!(GROUP_H)
        + FOOT
}

impl super::super::Stage {
    pub(super) fn draw_desk(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some((x0, x1)) = super::log::column(self, field) else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let top = heads::master_rect(field).max.y + crate::tune!(MASTER_GAP);
        let master = heads::master_rect(field);
        let bar_x0 = x0 + ch * 5.0;
        let pair_gap = ch * 1.5;
        let lane_w = ((x1 - bar_x0 - pair_gap) * 0.5).max(1.0);
        let lane_left_x = bar_x0;
        let lane_right_x = bar_x0 + lane_w + pair_gap;
        // S07: the block is a SURFACE, not a list of hairlines on bare
        // ground. The mockups give it a lighter header band over a panel
        // body — measured #564f49 over #393733 — and that is what makes
        // DESK read as one instrument instead of seven unrelated rows
        // that happen to be near each other. Painted first, so every
        // name, rail and meter below lands on top of it.
        let pad = ch;
        let header_h = crate::tune!(HEADER_H);
        let body_h = height() - crate::tune!(FOOT);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0 - pad, top - header_h),
                egui::pos2(x1 + pad, top),
            ),
            0.0,
            c.edge,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0 - pad, top), egui::pos2(x1 + pad, top + body_h)),
            0.0,
            c.panel,
        );
        painter.text(
            egui::pos2(x0, top - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "DESK",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(lane_left_x + lane_w * 0.5, top - 4.0),
            egui::Align2::CENTER_BOTTOM,
            "L",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(lane_right_x + lane_w * 0.5, top - 4.0),
            egui::Align2::CENTER_BOTTOM,
            "R",
            font.clone(),
            c.label,
        );
        let seam = top.round() - 0.5;
        painter.line_segment(
            [egui::pos2(x0, seam), egui::pos2(x1, seam)],
            egui::Stroke::new(1.0, c.rule),
        );
        // The block belongs to the pinned master column. Join its head
        // to that real column rule instead of leaving MASTER floating at
        // the edge of an unrelated-looking list.
        painter.line_segment(
            [egui::pos2(x1, seam), egui::pos2(master.center().x, seam)],
            egui::Stroke::new(1.0, c.edge),
        );
        // The same two columns carry every rail. No max-of-stereo
        // collapse: a left/right imbalance remains visible all the way
        // through the four buses, the two returns, and the main mix.
        let mut y = top;
        for group in GROUPS {
            let gy = y + crate::tune!(GROUP_H) * 0.5;
            painter.text(
                egui::pos2(x0, gy),
                egui::Align2::LEFT_CENTER,
                group.name,
                font.clone(),
                c.label,
            );
            let rule_x0 = x0 + (group.name.chars().count() as f32 + 2.5) * ch;
            painter.line_segment(
                [egui::pos2(rule_x0, gy), egui::pos2(x1, gy)],
                egui::Stroke::new(1.0, c.rule),
            );
            y += crate::tune!(GROUP_H);
            for &(name, slot) in group.rails {
                let reading = self.meters.rail(slot);
                let row = egui::Rect::from_min_max(
                    egui::pos2(x0, y),
                    egui::pos2(x1, y + crate::tune!(ROW_H)),
                );
                let active = reading.level.left.max(reading.level.right) > 0.0;
                painter.text(
                    egui::pos2(x0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    font.clone(),
                    if active { c.fg } else { c.dim },
                );
                for (lane_x, level, peak) in [
                    (lane_left_x, reading.level.left, reading.peak.left),
                    (lane_right_x, reading.level.right, reading.peak.right),
                ] {
                    let lane = egui::Rect::from_center_size(
                        egui::pos2(lane_x + lane_w * 0.5, row.center().y),
                        egui::vec2(lane_w, crate::tune!(BAR_H)),
                    );
                    meter_lane(painter, lane, level, peak, c);
                }
                y += crate::tune!(ROW_H);
            }
        }
    }
}
