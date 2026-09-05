//! The mixer: one channel strip per shown track, hanging beneath its
//! head where the scene rows were, and the master's under the master.
//!
//! A meter and a fader are two different quantities on ONE ruler: the
//! model's, decibels from the floor to unity at the top, with the small
//! boost the model allows drawn as overtravel above the meter's ceiling
//! (`stage::mixer::place_of_amp`, `unity_place`). Unity is exactly the
//! line the meter tops out at, so a fader at unity and a meter at full
//! scale are the same height because they are the same number.
//!
//! Meter cells are the machine reporting: `chassis` up the ladder, the
//! top cell `alert` when the signal reaches it. The fader's handle is a
//! chassis square, `bright` on the cursor's track. Nothing here animates
//! but the readings.

use super::heads;
use super::palette;
use crate::PROFONT;
use crate::ui::stage::mixer::{
    CELLS, Channel, SWITCH_H, cell_coverage, channels, gain_label, pan_label, place_of_amp,
    unity_place,
};
use eframe::egui;

/// Between a head's foot and its strip.
/// @tune 0..32 px
const HEAD_GAP: f32 = 8.0;
/// The ladders' width, each.
/// @tune 4..24 px
const LADDER_W: f32 = 10.0;
/// The fader rail's width.
/// @tune 4..24 px
const FADER_W: f32 = 8.0;
/// The pan row's height, its word above the rail.
/// @tune 12..48 px
const PAN_H: f32 = 30.0;
/// One send row.
/// @tune 8..24 px
const SEND_H: f32 = 14.0;
const INSET: f32 = 8.0;
const TYPE_PX: f32 = 12.0;
const GAP: f32 = 2.0;

/// Where a send's rail runs inside a strip.
///
/// The mixer draws the cable that leaves it, and the cable must land on
/// the rail rather than near it — so the one piece of arithmetic that
/// places the rail is shared rather than repeated.
pub fn send_y(strip: egui::Rect, gap: f32, sends: usize, switches: bool, slot: usize) -> f32 {
    let inner = strip.shrink(gap.max(3.0));
    let pan_top = inner.max.y - PAN_H;
    let switch_h = if switches { SWITCH_H + gap } else { 0.0 };
    let sends_top = pan_top - switch_h - (sends as f32 * SEND_H + gap);
    sends_top + slot as f32 * SEND_H + SEND_H * 0.5
}

/// Where the send's mark stands on that rail, across the strip.
pub fn send_x(strip: egui::Rect, gap: f32, send: f32) -> f32 {
    let inner = strip.shrink(gap.max(3.0));
    let left = inner.min.x + 16.0;
    let right = inner.max.x - 4.0;
    left + send.clamp(0.0, 1.0) * (right - left)
}

/// Where a channel strip hangs: everything from beneath the head down to
/// the foot of the field. The column IS the track's address, so the strip
/// takes exactly the head's width and never computes one of its own.
pub fn strip_beneath(head: egui::Rect, bottom: f32, gap: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(head.min.x, head.max.y + gap),
        egui::pos2(head.max.x, bottom),
    )
}

impl super::super::Stage {
    pub(super) fn draw_mixer(&self, painter: &egui::Painter, field: egui::Rect) {
        let readings = self.meters.readings();
        let all = channels(&self.song, &readings);
        let cursor = self
            .session_address()
            .and_then(super::super::Address::track);
        let bottom = field.max.y - heads::margin();
        for (slot, track) in self.shown_tracks(field.width()).enumerate() {
            let Some(channel) = all.get(track) else {
                continue;
            };
            let head = heads::head_rect(field, slot);
            let strip = egui::Rect::from_min_max(
                egui::pos2(head.min.x, head.max.y + crate::tune!(HEAD_GAP)),
                egui::pos2(head.max.x, bottom),
            );
            self.draw_strip(painter, strip, channel, cursor == Some(track));
        }
        // The master: the mix's own reading under the master head.
        let master_reading = self.meters.master();
        let master = Channel {
            gain: self.song.master,
            pan: 0.0,
            muted: false,
            soloed: false,
            audible: true,
            switches: false,
            level: master_reading.level,
            peak: master_reading.peak,
            sends: [None; crate::sequencing::ReturnTrack::MAX],
            send_letters: ['?'; crate::sequencing::ReturnTrack::MAX],
            is_return: false,
        };
        let head = heads::master_rect(field);
        let strip = egui::Rect::from_min_max(
            egui::pos2(head.min.x, head.max.y + crate::tune!(HEAD_GAP)),
            egui::pos2(head.max.x, bottom),
        );
        let on_master = matches!(self.session_address(), Some(super::super::Address::Master));
        self.draw_strip(painter, strip, &master, on_master);
    }

    fn draw_strip(&self, painter: &egui::Painter, strip: egui::Rect, ch: &Channel, on: bool) {
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        if strip.height() < 60.0 {
            return;
        }
        let inner = strip.shrink2(egui::vec2(INSET, 0.0));
        let sends = ch.sends.iter().flatten().count();
        let sends_h = sends as f32 * crate::tune!(SEND_H);
        let pan_top = inner.max.y - crate::tune!(PAN_H);
        let sends_top = pan_top - sends_h - if sends > 0 { 6.0 } else { 0.0 };
        let labels_h = TYPE_PX + 6.0;
        let chart = egui::Rect::from_min_max(
            egui::pos2(inner.min.x, inner.min.y),
            egui::pos2(inner.max.x, sends_top - labels_h),
        );

        // The ruler: unity is one line across the strip.
        let unity_y = (chart.max.y - chart.height() * unity_place()).round() - 0.5;
        painter.line_segment(
            [
                egui::pos2(chart.min.x, unity_y),
                egui::pos2(chart.max.x, unity_y),
            ],
            egui::Stroke::new(1.0, c.edge),
        );

        // Two ladders, left and right, on the ruler.
        let lw = crate::tune!(LADDER_W);
        for (i, (amp, peak)) in [
            (ch.level.left, ch.peak.left),
            (ch.level.right, ch.peak.right),
        ]
        .into_iter()
        .enumerate()
        {
            let x = chart.min.x + i as f32 * (lw + GAP);
            let ladder =
                egui::Rect::from_min_max(egui::pos2(x, unity_y), egui::pos2(x + lw, chart.max.y));
            painter.rect_filled(ladder, 0.0, c.panel);
            let place = place_of_amp(amp) / unity_place();
            let cell_h = ladder.height() / CELLS as f32;
            for cell in 0..CELLS {
                let cover = cell_coverage(cell, place.clamp(0.0, 1.0));
                if cover <= 0.0 {
                    continue;
                }
                let y1 = ladder.max.y - cell as f32 * cell_h;
                let y0 = y1 - cell_h * cover;
                let top_cell = cell == CELLS - 1;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(ladder.min.x, y0),
                        egui::pos2(ladder.max.x, y1 - 1.0),
                    ),
                    0.0,
                    if top_cell { c.alert } else { c.chassis },
                );
            }
            let peak_y = (ladder.max.y
                - ladder.height() * (place_of_amp(peak) / unity_place()).clamp(0.0, 1.0))
            .round()
                - 0.5;
            painter.line_segment(
                [
                    egui::pos2(ladder.min.x, peak_y),
                    egui::pos2(ladder.max.x, peak_y),
                ],
                egui::Stroke::new(1.0, c.fg),
            );
        }

        // The fader rail, beside the ladders, on the same ruler — its
        // handle a square at the gain, the overtravel above unity.
        let fw = crate::tune!(FADER_W);
        let rail_x = chart.max.x - fw;
        let rail = egui::Rect::from_min_max(
            egui::pos2(rail_x, chart.min.y),
            egui::pos2(rail_x + fw, chart.max.y),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rail.center().x - 0.5, rail.min.y),
                egui::pos2(rail.center().x + 0.5, rail.max.y),
            ),
            0.0,
            c.rule,
        );
        let handle_y = chart.max.y - chart.height() * place_of_amp(ch.gain).clamp(0.0, 1.0);
        let handle =
            egui::Rect::from_center_size(egui::pos2(rail.center().x, handle_y), egui::vec2(fw, fw));
        painter.rect_filled(handle, 0.0, if on { c.bright } else { c.chassis });

        // The words: gain and pan, said exactly.
        let ly = chart.max.y + labels_h * 0.5;
        painter.text(
            egui::pos2(inner.min.x, ly),
            egui::Align2::LEFT_CENTER,
            gain_label(ch.gain),
            font.clone(),
            if ch.muted { c.dim } else { c.fg },
        );

        // The sends: one lettered rail per return, the mark at the share.
        for (i, (share, letter)) in ch
            .sends
            .iter()
            .zip(ch.send_letters)
            .filter_map(|(s, l)| s.map(|s| (s, l)))
            .enumerate()
        {
            let y = sends_top + i as f32 * crate::tune!(SEND_H) + crate::tune!(SEND_H) * 0.5;
            painter.text(
                egui::pos2(inner.min.x, y),
                egui::Align2::LEFT_CENTER,
                letter.to_string(),
                font.clone(),
                c.label,
            );
            let x0 = inner.min.x + TYPE_PX;
            let bar =
                egui::Rect::from_min_max(egui::pos2(x0, y - 1.0), egui::pos2(inner.max.x, y + 1.0));
            painter.rect_filled(bar, 0.0, c.rule);
            painter.rect_filled(
                egui::Rect::from_min_max(bar.min, egui::pos2(x0 + bar.width() * share, bar.max.y)),
                0.0,
                c.edge,
            );
        }

        // Pan: a rail with an exact centre, the handle where the pan is.
        let py = pan_top + crate::tune!(PAN_H) * 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, py), egui::pos2(inner.max.x, py)],
            egui::Stroke::new(1.0, c.rule),
        );
        let cx = inner.center().x.round() - 0.5;
        painter.line_segment(
            [egui::pos2(cx, py - 4.0), egui::pos2(cx, py + 4.0)],
            egui::Stroke::new(1.0, c.edge),
        );
        painter.text(
            egui::pos2(inner.max.x, py - 9.0),
            egui::Align2::RIGHT_BOTTOM,
            pan_label(ch.pan),
            font.clone(),
            c.dim,
        );
        if ch.switches {
            let hx = inner.min.x + inner.width() * ((ch.pan + 1.0) * 0.5).clamp(0.0, 1.0);
            painter.rect_filled(
                egui::Rect::from_center_size(egui::pos2(hx, py), egui::vec2(6.0, 6.0)),
                0.0,
                if on { c.bright } else { c.fg },
            );
        }
    }
}
