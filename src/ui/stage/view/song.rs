//! The song view: the field turned over. Heads down the left, time
//! across, blocks laid along the lanes, and a minimap of the whole.
//!
//! Every mark is the arrangement's own fact — `stage::arrangement` holds
//! the cursor, the window, the zoom and the selection, and the song holds
//! the blocks, the brace and the locators. A pattern block is a `panel`
//! pad with its pattern's number; an audio block an `edge` outline; the
//! cursor's block wears brackets; selected spans sit on `select`. The
//! playhead is a `nominal` hairline at the transport's tick — reporting,
//! not motion — and the loop brace is two bracket marks on the ruler
//! with a wash between them.

use super::heads;
use super::palette;
use crate::PROFONT;
use crate::sequencing::TICKS_PER_BEAT;
use crate::ui::stage::arrangement::bar_ticks;
use eframe::egui;

/// The lane heads' width.
/// @tune 60..200 px
const HEAD_W: f32 = 120.0;
/// One lane's height.
/// @tune 16..64 px
const LANE_H: f32 = 28.0;
/// Between lanes.
/// @tune 0..16 px
const LANE_GAP: f32 = 4.0;
/// The ruler's height.
/// @tune 12..40 px
const RULER_H: f32 = 20.0;
/// The minimap's height.
/// @tune 8..40 px
const MINIMAP_H: f32 = 14.0;
const TYPE_PX: f32 = 12.0;

/// How often the ruler writes a bar's number, for the width a bar has.
fn label_every(bar_px: f32) -> usize {
    if bar_px >= 34.0 {
        1
    } else if bar_px >= 17.0 {
        2
    } else if bar_px >= 9.0 {
        4
    } else {
        8
    }
}

struct Frame {
    heads: egui::Rect,
    ruler: egui::Rect,
    lanes: egui::Rect,
    minimap: egui::Rect,
    view_start: usize,
    view_len: usize,
    px_per_tick: f32,
    row_offset: usize,
    rows: usize,
}

impl Frame {
    fn x(&self, tick: usize) -> f32 {
        self.ruler.min.x + (tick as f64 - self.view_start as f64) as f32 * self.px_per_tick
    }
    fn view_end(&self) -> usize {
        self.view_start.saturating_add(self.view_len)
    }
    fn lane(&self, row: usize) -> egui::Rect {
        let top = self.lanes.min.y + row as f32 * (crate::tune!(LANE_H) + crate::tune!(LANE_GAP));
        egui::Rect::from_min_max(
            egui::pos2(self.lanes.min.x, top),
            egui::pos2(self.lanes.max.x, top + crate::tune!(LANE_H)),
        )
    }
}

impl super::super::Stage {
    fn song_frame(&self, field: egui::Rect) -> Frame {
        let margin = heads::margin();
        let left = field.min.x + margin + heads::gutter();
        let time_x0 = left + crate::tune!(HEAD_W) + 8.0;
        let time_x1 = (field.max.x - margin).max(time_x0 + 1.0);
        let ruler = egui::Rect::from_min_max(
            egui::pos2(time_x0, field.min.y + margin),
            egui::pos2(time_x1, field.min.y + margin + crate::tune!(RULER_H)),
        );
        let minimap = egui::Rect::from_min_max(
            egui::pos2(
                time_x0,
                (field.max.y - margin - crate::tune!(MINIMAP_H)).max(ruler.max.y),
            ),
            egui::pos2(time_x1, (field.max.y - margin).max(ruler.max.y)),
        );
        let gap = crate::tune!(LANE_GAP);
        let lanes = egui::Rect::from_min_max(
            egui::pos2(time_x0, ruler.max.y + gap),
            egui::pos2(time_x1, (minimap.min.y - gap).max(ruler.max.y + gap)),
        );
        let heads = egui::Rect::from_min_max(
            egui::pos2(left, lanes.min.y),
            egui::pos2(left + crate::tune!(HEAD_W), lanes.max.y),
        );
        let rows =
            (((lanes.height() + gap) / (crate::tune!(LANE_H) + gap)).floor() as usize).max(1);
        let row_offset = self.arrangement.track.saturating_sub(rows - 1);
        let view_len = self.arrangement.view_len(&self.song).max(1);
        Frame {
            heads,
            ruler,
            lanes,
            minimap,
            view_start: self.arrangement.view_start,
            view_len,
            px_per_tick: ruler.width() / view_len as f32,
            row_offset,
            rows,
        }
    }

    pub(super) fn draw_song(&self, painter: &egui::Painter, field: egui::Rect) {
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let f = self.song_frame(field);
        let hair = egui::Stroke::new(1.0, c.rule);
        let arr = &self.arrangement;

        // The ruler: a tick per bar, a number where there is room.
        let bar = bar_ticks(&self.song, f.view_start).max(1);
        let bar_px = bar as f32 * f.px_per_tick;
        let every = label_every(bar_px);
        let ry = f.ruler.max.y.round() - 0.5;
        painter.line_segment(
            [egui::pos2(f.ruler.min.x, ry), egui::pos2(f.ruler.max.x, ry)],
            hair,
        );
        let first_bar = f.view_start / bar;
        let last_bar = f.view_end() / bar + 1;
        for b in first_bar..=last_bar {
            let x = f.x(b * bar).round() - 0.5;
            if x < f.ruler.min.x - 1.0 || x > f.ruler.max.x {
                continue;
            }
            let long = b % every == 0;
            painter.line_segment(
                [
                    egui::pos2(x, ry - if long { 6.0 } else { 3.0 }),
                    egui::pos2(x, ry),
                ],
                hair,
            );
            if long {
                painter.text(
                    egui::pos2(x + 3.0, f.ruler.min.y),
                    egui::Align2::LEFT_TOP,
                    format!("{:03}", b + 1),
                    font.clone(),
                    c.dim,
                );
            }
            // Beat ticks inside a bar, when a bar is wide enough to hold them.
            if bar_px >= 60.0 {
                let beats = bar / TICKS_PER_BEAT;
                for k in 1..beats {
                    let bx = f.x(b * bar + k * TICKS_PER_BEAT).round() - 0.5;
                    painter.line_segment([egui::pos2(bx, ry - 2.0), egui::pos2(bx, ry)], hair);
                }
            }
        }

        // The loop brace: two marks on the ruler, a wash over the lanes.
        if let Some((start, end)) = self.song.loop_brace
            && end > start
        {
            let x0 = f.x(start).max(f.ruler.min.x);
            let x1 = f.x(end).min(f.ruler.max.x);
            if x1 > x0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, f.lanes.min.y),
                        egui::pos2(x1, f.lanes.max.y),
                    ),
                    0.0,
                    c.select,
                );
                for x in [x0, x1] {
                    painter.line_segment(
                        [
                            egui::pos2(x.round() - 0.5, f.ruler.min.y),
                            egui::pos2(x.round() - 0.5, f.ruler.max.y),
                        ],
                        egui::Stroke::new(1.0, c.chassis),
                    );
                }
            }
        }

        // Lanes: a head each, then the blocks.
        let spans = arr.selected_track_spans();
        for row in 0..f.rows {
            let track = f.row_offset + row;
            let Some(t) = self.song.tracks.get(track) else {
                break;
            };
            let lane = f.lane(row);
            let head = egui::Rect::from_min_max(
                egui::pos2(f.heads.min.x, lane.min.y),
                egui::pos2(f.heads.max.x, lane.max.y),
            );
            let on_row = arr.track == track;
            // The lane's own seam, and its head.
            let ly = lane.max.y.round() - 0.5;
            painter.line_segment(
                [egui::pos2(lane.min.x, ly), egui::pos2(lane.max.x, ly)],
                hair,
            );
            painter.text(
                egui::pos2(head.min.x, lane.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{:02}", track + 1),
                font.clone(),
                c.label,
            );
            let fits = ((head.width() - 3.0 * TYPE_PX * 0.6) / (TYPE_PX * 0.6))
                .floor()
                .max(1.0) as usize;
            painter.text(
                egui::pos2(head.min.x + 3.0 * TYPE_PX * 0.6, lane.center().y),
                egui::Align2::LEFT_CENTER,
                t.name.chars().take(fits).collect::<String>(),
                font.clone(),
                if on_row { c.fg } else { c.dim },
            );
            // Selected spans on this track.
            for (st, start, end) in &spans {
                if *st == track {
                    let x0 = f.x(*start).max(lane.min.x);
                    let x1 = f.x(*end).min(lane.max.x);
                    if x1 > x0 {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(x0, lane.min.y),
                                egui::pos2(x1, lane.max.y),
                            ),
                            0.0,
                            c.select,
                        );
                    }
                }
            }
            // Pattern blocks.
            let under = arr.block_index(t);
            for (i, block) in t.blocks.iter().enumerate() {
                let end = block.start_tick.saturating_add(block.length_ticks);
                if end <= f.view_start || block.start_tick >= f.view_end() {
                    continue;
                }
                let rect = egui::Rect::from_min_max(
                    egui::pos2(f.x(block.start_tick).max(lane.min.x), lane.min.y + 2.0),
                    egui::pos2(f.x(end).min(lane.max.x) - 1.0, lane.max.y - 2.0),
                );
                if rect.width() < 1.0 {
                    continue;
                }
                painter.rect_filled(rect, 0.0, c.panel);
                if rect.width() > 3.0 * TYPE_PX {
                    painter.text(
                        egui::pos2(rect.min.x + 4.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{:02}", block.pattern_id.0),
                        font.clone(),
                        c.fg,
                    );
                }
                if on_row && under == Some(i) {
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("block", track, i),
                        rect,
                        crate::ui::nav_cursor::Kind::Block,
                        crate::ui::nav_cursor::Layer::Surface,
                        c.alert,
                    );
                }
            }
            // Audio blocks: an outline, the sample's own thing.
            for block in &t.audio_blocks {
                let end = block.end_tick();
                if end <= f.view_start || block.start_tick >= f.view_end() {
                    continue;
                }
                let rect = egui::Rect::from_min_max(
                    egui::pos2(f.x(block.start_tick).max(lane.min.x), lane.min.y + 2.0),
                    egui::pos2(f.x(end).min(lane.max.x) - 1.0, lane.max.y - 2.0),
                );
                if rect.width() >= 1.0 {
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(1.0, c.edge),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            // The cursor: a tick at its time, in its lane.
            if on_row && arr.tick >= f.view_start && arr.tick < f.view_end() {
                let x = f.x(arr.tick).round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, lane.min.y), egui::pos2(x, lane.max.y)],
                    egui::Stroke::new(1.5, c.alert),
                );
            }
        }

        // Locators: a mark on the ruler and its name.
        for locator in &self.song.locators {
            if locator.tick < f.view_start || locator.tick > f.view_end() {
                continue;
            }
            let x = f.x(locator.tick).round() - 0.5;
            painter.line_segment(
                [egui::pos2(x, f.ruler.min.y), egui::pos2(x, f.lanes.max.y)],
                egui::Stroke::new(1.0, c.edge),
            );
            painter.text(
                egui::pos2(x + 3.0, f.ruler.max.y - 1.0),
                egui::Align2::LEFT_BOTTOM,
                &locator.name,
                font.clone(),
                c.label,
            );
        }

        // The playhead: where the transport is, a nominal hairline.
        let tick = self.transport.tick();
        if tick >= f.view_start && tick < f.view_end() {
            let x = f.x(tick).round() - 0.5;
            painter.line_segment(
                [egui::pos2(x, f.ruler.min.y), egui::pos2(x, f.lanes.max.y)],
                egui::Stroke::new(1.0, c.nominal),
            );
        }

        // The minimap: the whole song, the window on it.
        let song_len = self
            .song
            .tracks
            .iter()
            .flat_map(|t| {
                t.blocks
                    .iter()
                    .map(|b| b.start_tick + b.length_ticks)
                    .chain(t.audio_blocks.iter().map(|b| b.end_tick()))
            })
            .max()
            .unwrap_or(0)
            .max(f.view_end());
        painter.rect_filled(f.minimap, 0.0, c.panel);
        let mx = |tick: usize| {
            f.minimap.min.x + f.minimap.width() * (tick as f32 / song_len.max(1) as f32)
        };
        for t in &self.song.tracks {
            for b in &t.blocks {
                let r = egui::Rect::from_min_max(
                    egui::pos2(mx(b.start_tick), f.minimap.min.y + 2.0),
                    egui::pos2(mx(b.start_tick + b.length_ticks), f.minimap.max.y - 2.0),
                );
                painter.rect_filled(r, 0.0, c.edge);
            }
        }
        let window = egui::Rect::from_min_max(
            egui::pos2(mx(f.view_start), f.minimap.min.y),
            egui::pos2(mx(f.view_end()), f.minimap.max.y),
        );
        painter.rect_stroke(
            window,
            0.0,
            egui::Stroke::new(1.0, c.chassis),
            egui::StrokeKind::Inside,
        );
    }
}
