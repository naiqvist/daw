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
    CELLS, Channel, Reading, SWITCH_H, cell_coverage, channels, gain_label, pan_label,
    place_of_amp, place_of_db, unity_place,
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
/// The desk's four group buses, two returns, and MIX stay pinned beside the
/// master. They are narrower than source channels: no sends or switches live
/// on them, and a fixed bank keeps their signal-flow order readable.
const DESK_COLUMNS: usize = 7;
const DESK_W: f32 = 62.0;
const DESK_GAP: f32 = 5.0;
const DESK_BANK_PAD: f32 = 16.0;
/// Fixed calibration points on the documented -60 dB..boost ruler.
const CALIBRATION: [(f32, &str); 2] = [(-36.0, "-36"), (-18.0, "-18")];

fn desk_bank_width() -> f32 {
    DESK_COLUMNS as f32 * DESK_W + (DESK_COLUMNS - 1) as f32 * DESK_GAP
}

/// Track capacity while the desk bank is visible. Session view retains the
/// wider head capacity; Mixer deliberately spends those columns on the buses
/// and returns that make its topology real.
pub(super) fn track_capacity(field_w: f32) -> usize {
    let usable = field_w
        - heads::margin() * 2.0
        - heads::gutter()
        - (heads::head_width() + heads::gap())
        - desk_bank_width()
        - DESK_BANK_PAD;
    ((usable / (heads::head_width() + heads::gap()))
        .floor()
        .max(1.0)) as usize
}

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

fn ruler_y(chart: egui::Rect, db: f32) -> f32 {
    chart.max.y - chart.height() * place_of_db(db)
}

fn rail_channel(rail: &crate::sequencing::Rail, reading: Reading) -> Channel {
    Channel {
        gain: rail.volume,
        pan: rail.pan,
        muted: false,
        soloed: false,
        audible: true,
        switches: false,
        level: reading.level,
        peak: reading.peak,
        sends: [None; crate::sequencing::ReturnTrack::MAX],
        send_letters: ['?'; crate::sequencing::ReturnTrack::MAX],
        is_return: false,
    }
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
        self.draw_desk_bank(painter, field, bottom);
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

    fn draw_desk_bank(&self, painter: &egui::Painter, field: egui::Rect, bottom: f32) {
        let master = heads::master_rect(field);
        let bank_left = master.min.x - DESK_BANK_PAD - desk_bank_width();
        let shown = self.shown_tracks(field.width());
        if shown.len() > 0 {
            let last = heads::head_rect(field, shown.len() - 1);
            if last.max.x + DESK_BANK_PAD > bank_left {
                return;
            }
        }

        let mut slot = 0usize;
        for (index, rail) in self.song.console.buses.iter().take(4).enumerate() {
            let channel = rail_channel(rail, self.meters.rail(crate::ui::stage::BUS_METER + index));
            self.draw_desk_column(
                painter,
                field,
                bottom,
                bank_left,
                slot,
                &format!("B{}", index + 1),
                &rail.name,
                &channel,
            );
            slot += 1;
        }
        for (index, rail) in self.song.console.aux.iter().take(2).enumerate() {
            let channel = rail_channel(
                rail,
                self.meters.rail(crate::ui::stage::RETURN_METER + index),
            );
            self.draw_desk_column(
                painter,
                field,
                bottom,
                bank_left,
                slot,
                &format!("R{}", index + 1),
                &rail.name,
                &channel,
            );
            slot += 1;
        }
        let mix = rail_channel(
            &self.song.console.mix,
            self.meters.rail(crate::ui::stage::MIX_RAIL_METER),
        );
        self.draw_desk_column(painter, field, bottom, bank_left, slot, "MX", "MIX", &mix);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_desk_column(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        bottom: f32,
        bank_left: f32,
        slot: usize,
        address: &str,
        name: &str,
        channel: &Channel,
    ) {
        let c = palette::colours();
        let head = egui::Rect::from_min_size(
            egui::pos2(
                bank_left + slot as f32 * (DESK_W + DESK_GAP),
                field.min.y + heads::margin(),
            ),
            egui::vec2(DESK_W, heads::head_height()),
        );
        painter.rect_filled(head.shrink(1.0), 0.0, c.panel);
        let key = match slot {
            0 => super::chassis::Key::Left,
            n if n + 1 == DESK_COLUMNS => super::chassis::Key::Right,
            _ => super::chassis::Key::Centre,
        };
        super::chassis::keyed(painter, head, false, key);
        let font = egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into()));
        painter.text(
            head.min + egui::vec2(6.0, 8.0),
            egui::Align2::LEFT_TOP,
            address,
            font.clone(),
            c.label,
        );
        painter.text(
            head.min + egui::vec2(6.0, 27.0),
            egui::Align2::LEFT_CENTER,
            name,
            font,
            c.fg,
        );
        let strip = egui::Rect::from_min_max(
            egui::pos2(head.min.x, head.max.y + crate::tune!(HEAD_GAP)),
            egui::pos2(head.max.x, bottom),
        );
        self.draw_compact_strip(painter, strip, channel);
    }

    fn draw_compact_strip(&self, painter: &egui::Painter, strip: egui::Rect, ch: &Channel) {
        if strip.height() < 60.0 {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into()));
        let inner = strip.shrink2(egui::vec2(6.0, 0.0));
        let chart =
            egui::Rect::from_min_max(inner.min, egui::pos2(inner.max.x, inner.max.y - 36.0));
        let unity_y = ruler_y(chart, 0.0).round() - 0.5;

        for db in [-36.0, -18.0] {
            let y = ruler_y(chart, db).round() - 0.5;
            painter.line_segment(
                [egui::pos2(chart.min.x, y), egui::pos2(chart.max.x, y)],
                egui::Stroke::new(1.0, c.rule),
            );
        }
        painter.line_segment(
            [
                egui::pos2(chart.min.x, unity_y),
                egui::pos2(chart.max.x, unity_y),
            ],
            egui::Stroke::new(1.0, c.edge),
        );

        let ladder_w = 6.0;
        for (index, (amp, peak)) in [
            (ch.level.left, ch.peak.left),
            (ch.level.right, ch.peak.right),
        ]
        .into_iter()
        .enumerate()
        {
            let x = chart.min.x + index as f32 * (ladder_w + 2.0);
            let ladder = egui::Rect::from_min_max(
                egui::pos2(x, unity_y),
                egui::pos2(x + ladder_w, chart.max.y),
            );
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
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(ladder.min.x, y0),
                        egui::pos2(ladder.max.x, y1 - 1.0),
                    ),
                    0.0,
                    if cell == CELLS - 1 {
                        c.alert
                    } else {
                        c.chassis
                    },
                );
            }
            let peak_y = ladder.max.y
                - ladder.height() * (place_of_amp(peak) / unity_place()).clamp(0.0, 1.0);
            painter.line_segment(
                [
                    egui::pos2(ladder.min.x, peak_y),
                    egui::pos2(ladder.max.x, peak_y),
                ],
                egui::Stroke::new(1.0, c.fg),
            );
        }

        let rail_x = chart.max.x - 6.0;
        painter.line_segment(
            [
                egui::pos2(rail_x, chart.min.y),
                egui::pos2(rail_x, chart.max.y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let handle_y = chart.max.y - chart.height() * place_of_amp(ch.gain);
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(rail_x, handle_y), egui::vec2(6.0, 6.0)),
            0.0,
            c.chassis,
        );
        painter.text(
            egui::pos2(inner.min.x, chart.max.y + 9.0),
            egui::Align2::LEFT_CENTER,
            gain_label(ch.gain).replace(" dB", ""),
            font.clone(),
            c.fg,
        );
        painter.text(
            egui::pos2(inner.min.x, chart.max.y + 25.0),
            egui::Align2::LEFT_CENTER,
            format!("P {}", pan_label(ch.pan)),
            font,
            c.dim,
        );
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
        // A strip with sends already gets separation from the send bank.
        // MASTER has none, so reserve the same breathing room explicitly;
        // otherwise its gain word descends into the PAN label at 1280x800.
        let control_gap = if sends > 0 { 6.0 } else { 10.0 };
        let sends_top = pan_top - sends_h - control_gap;
        let labels_h = TYPE_PX + 6.0;
        let chart = egui::Rect::from_min_max(
            egui::pos2(inner.min.x, inner.min.y),
            egui::pos2(inner.max.x, sends_top - labels_h),
        );

        // The ruler: unity is one line across the strip. Two documented
        // calibration values make the lower span readable as decibels,
        // rather than merely as height.
        let unity_y = ruler_y(chart, 0.0).round() - 0.5;
        let lw = crate::tune!(LADDER_W);
        let text_ch = TYPE_PX * 0.6;
        let calibration_x = chart.min.x + 2.0 * (lw + GAP) + 2.0;
        for (db, word) in CALIBRATION {
            let y = ruler_y(chart, db).round() - 0.5;
            let word_w = word.chars().count() as f32 * text_ch;
            painter.line_segment(
                [
                    egui::pos2(chart.min.x, y),
                    egui::pos2(calibration_x - 2.0, y),
                ],
                egui::Stroke::new(1.0, c.rule),
            );
            painter.line_segment(
                [
                    egui::pos2(calibration_x + word_w + 2.0, y),
                    egui::pos2(chart.max.x, y),
                ],
                egui::Stroke::new(1.0, c.rule),
            );
            painter.text(
                egui::pos2(calibration_x, y),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                c.dim,
            );
        }
        painter.line_segment(
            [
                egui::pos2(chart.min.x, unity_y),
                egui::pos2(chart.max.x, unity_y),
            ],
            egui::Stroke::new(1.0, c.edge),
        );

        // Two ladders, left and right, named in the ruler's overtravel
        // where no meter can obscure them.
        let meter_head_y = (chart.min.y + unity_y) * 0.5;
        for (i, side) in ["L", "R"].into_iter().enumerate() {
            let x = chart.min.x + i as f32 * (lw + GAP) + lw * 0.5;
            painter.text(
                egui::pos2(x, meter_head_y),
                egui::Align2::CENTER_CENTER,
                side,
                font.clone(),
                c.label,
            );
        }
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

        // Pan: a named rail with an exact centre, the handle where the
        // document's pan is.
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
            egui::pos2(inner.min.x, py - 9.0),
            egui::Align2::LEFT_BOTTOM,
            "PAN",
            font.clone(),
            c.label,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_ticks_follow_the_shared_decibel_ruler() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(80.0, 200.0));
        let y36 = ruler_y(chart, -36.0);
        let y18 = ruler_y(chart, -18.0);
        let unity = ruler_y(chart, 0.0);
        assert!(chart.top() < unity && unity < y18 && y18 < y36 && y36 < chart.bottom());
    }

    #[test]
    fn mixer_reserves_room_for_the_fixed_console_bank() {
        let field_w = 1_280.0;
        assert!(track_capacity(field_w) < heads::capacity(field_w));
        assert!(track_capacity(field_w) >= 1);

        let occupied = desk_bank_width() + DESK_BANK_PAD;
        assert!(occupied > DESK_COLUMNS as f32 * DESK_W);
    }
}
