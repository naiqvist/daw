//! The song view, drawn: heads down the left, time across, the blocks
//! laid along the lanes and a minimap over the whole.
//!
//! The arrangement itself — takes, holds, the clipboard, every verb that
//! moves a block — is `stage::arrangement`, and knows nothing of pixels.

use super::*;
use crate::sequencing::TICKS_PER_BEAT;
use crate::ui::stage::arrangement::*;

/// The song view's geometry for one frame: heads down the left, the
/// ruler over the lanes, the minimap at the foot.
pub struct Frame {
    pub heads: egui::Rect,
    pub ruler: egui::Rect,
    pub lanes: egui::Rect,
    pub minimap: egui::Rect,
    pub view_start: usize,
    pub view_len: usize,
    pub px_per_tick: f32,
    /// The first track shown; the cursor's row is always among them.
    pub row_offset: usize,
    pub rows: usize,
}

impl Frame {
    pub fn of(field: egui::Rect, arrangement: &Arrangement, song: &Song) -> Self {
        let margin = design::px(design::space::ROOM);
        let left = field.min.x + margin;
        let time_x0 = left + HEAD_W + column_gap();
        let time_x1 = (field.max.x - margin).max(time_x0 + 1.0);
        let ruler = egui::Rect::from_min_max(
            egui::pos2(time_x0, field.min.y + margin),
            egui::pos2(time_x1, field.min.y + margin + RULER_H),
        );
        let minimap = egui::Rect::from_min_max(
            egui::pos2(time_x0, (field.max.y - margin - MINIMAP_H).max(ruler.max.y)),
            egui::pos2(time_x1, (field.max.y - margin).max(ruler.max.y)),
        );
        let lanes = egui::Rect::from_min_max(
            egui::pos2(time_x0, ruler.max.y + LANE_GAP),
            egui::pos2(
                time_x1,
                (minimap.min.y - LANE_GAP).max(ruler.max.y + LANE_GAP),
            ),
        );
        let heads = egui::Rect::from_min_max(
            egui::pos2(left, lanes.min.y),
            egui::pos2(left + HEAD_W, lanes.max.y),
        );
        let rows = (((lanes.height() + LANE_GAP) / (LANE_H + LANE_GAP)).floor() as usize).max(1);
        let row_offset = arrangement.track.saturating_sub(rows - 1);
        let view_len = arrangement.view_len(song);
        Self {
            heads,
            ruler,
            lanes,
            minimap,
            view_start: arrangement.view_start,
            view_len,
            px_per_tick: ruler.width() / view_len as f32,
            row_offset,
            rows,
        }
    }

    /// Where `tick` falls, which may be off either side of the area.
    pub fn x(&self, tick: usize) -> f32 {
        self.ruler.min.x + (tick as f64 - self.view_start as f64) as f32 * self.px_per_tick
    }

    pub fn view_end(&self) -> usize {
        self.view_start.saturating_add(self.view_len)
    }

    pub fn lane(&self, row: usize) -> egui::Rect {
        let top = self.lanes.min.y + row as f32 * (LANE_H + LANE_GAP);
        egui::Rect::from_min_max(
            egui::pos2(self.lanes.min.x, top),
            egui::pos2(self.lanes.max.x, top + LANE_H),
        )
    }

    pub fn head(&self, row: usize) -> egui::Rect {
        let lane = self.lane(row);
        egui::Rect::from_min_max(
            egui::pos2(self.heads.min.x, lane.min.y),
            egui::pos2(self.heads.max.x, lane.max.y),
        )
    }

    /// The rect a span of ticks covers in `lane`, cut to the area.
    pub fn span(&self, lane: egui::Rect, start: usize, end: usize) -> egui::Rect {
        let x0 = self.x(start).max(self.lanes.min.x);
        let x1 = self.x(end).min(self.lanes.max.x);
        egui::Rect::from_min_max(egui::pos2(x0, lane.min.y), egui::pos2(x1, lane.max.y))
    }
}

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

impl Stage {
    /// The song view in the field: the board, the ruler, the heads down
    /// the left, the blocks in their lanes, the playhead, the cursor,
    /// and the minimap at the foot.
    pub(super) fn draw_song(&self, painter: &egui::Painter, field: egui::Rect, phase: Phase) {
        let alpha = self.alphabet();
        let frame = Frame::of(field, &self.arrangement, &self.song);
        let edge = alpha.edge.color;
        let ink = alpha.ink.color;
        let ground = alpha.ground.color;
        let holds_the_keys =
            self.chain.is_none() && self.browser.is_none() && self.focus.depth() == 1;
        let cursor_shade = if holds_the_keys {
            self.focused()
        } else {
            self.resting()
        };
        let tracks = self.song.tracks.len();
        let shown = frame.row_offset..(frame.row_offset + frame.rows).min(tracks);
        let bar = bar_ticks(&self.song, frame.view_start).max(1);
        let bar_px = bar as f32 * frame.px_per_tick;
        let beats_shown = TICKS_PER_BEAT as f32 * frame.px_per_tick >= 10.0;
        let sounding = self.song_mode() && phase.rolling;
        let now = self.transport.tick();

        // The title over the heads: which projection this is, and a
        // held verb while one holds.
        let title = egui::pos2(frame.heads.min.x, frame.ruler.center().y);
        block::paint(
            painter,
            egui::Id::new("stage-song-title"),
            title,
            egui::Align2::LEFT_CENTER,
            block::unit::TITLE,
            "SONG",
            ink,
        );
        if let Some(hold) = self.arrangement.hold {
            painter.text(
                egui::pos2(frame.heads.max.x, frame.ruler.center().y),
                egui::Align2::RIGHT_CENTER,
                hold.word(),
                egui::FontId::monospace(design::px(design::type_scale::MICRO)),
                alpha.live.color,
            );
        }

        // The board: bars as rules, beats as the dimmer rules when they
        // are far enough apart to be seen, on the deck's lattice.
        let area = egui::Rect::from_min_max(frame.lanes.min, frame.lanes.max);
        if area.is_positive() {
            let first_bar = frame.view_start / bar;
            let last_bar = frame.view_end().div_ceil(bar);
            let cols: Vec<f32> = (first_bar..=last_bar)
                .map(|n| frame.x(n * bar))
                .filter(|x| (area.min.x - 1.0..=area.max.x + 1.0).contains(x))
                .collect();
            let rows: Vec<f32> = shown
                .clone()
                .map(|track| frame.lane(track - frame.row_offset).center().y)
                .collect();
            let board_ink = edge.gamma_multiply(0.72);
            let dots = edge.gamma_multiply(0.38);
            kit::cached(
                painter,
                egui::Id::new("stage-song-board"),
                area,
                (
                    board_ink,
                    ground,
                    frame.view_start,
                    frame.view_len,
                    shown.len(),
                    beats_shown,
                ),
                |out| {
                    circuit::lattice(out, area, design::px(design::space::VAST), dots);
                    circuit::board(out, area, &cols, &rows, board_ink, ground);
                    if beats_shown {
                        let beat_ink = edge.gamma_multiply(0.4);
                        let mut tick = frame.view_start / TICKS_PER_BEAT * TICKS_PER_BEAT;
                        while tick <= frame.view_end() {
                            if !tick.is_multiple_of(bar) {
                                let x = frame.x(tick);
                                if x >= area.min.x && x <= area.max.x {
                                    circuit::trace(
                                        out,
                                        &[egui::pos2(x, area.min.y), egui::pos2(x, area.max.y)],
                                        Weight::Hair,
                                        beat_ink,
                                    );
                                }
                            }
                            tick += TICKS_PER_BEAT;
                        }
                    }
                },
            );
        }

        // The ruler: a pad at every bar, the number where there is room
        // for it, and the beats as smaller pads when they are shown.
        {
            let ruler = frame.ruler;
            let every = label_every(bar_px);
            let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
            let mut shapes = Vec::new();
            let first_bar = frame.view_start / bar;
            let last_bar = frame.view_end() / bar + 1;
            for n in first_bar..=last_bar {
                let x = frame.x(n * bar);
                if x < ruler.min.x - 0.5 || x > ruler.max.x + 0.5 {
                    continue;
                }
                circuit::pad(
                    &mut shapes,
                    egui::pos2(x, ruler.bottom() - 3.0),
                    circuit::PAD,
                    ink,
                    true,
                );
                if n.is_multiple_of(every) && x + 4.0 < ruler.max.x {
                    painter.text(
                        egui::pos2(x + 4.0, ruler.center().y - 1.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{}", n + 1),
                        font.clone(),
                        ink,
                    );
                }
            }
            if beats_shown {
                let mut tick = frame.view_start / TICKS_PER_BEAT * TICKS_PER_BEAT;
                while tick <= frame.view_end() {
                    if !tick.is_multiple_of(bar) {
                        let x = frame.x(tick);
                        if x >= ruler.min.x && x <= ruler.max.x {
                            circuit::pad(
                                &mut shapes,
                                egui::pos2(x, ruler.bottom() - 3.0),
                                circuit::PAD - 2.0,
                                edge,
                                false,
                            );
                        }
                    }
                    tick += TICKS_PER_BEAT;
                }
            }
            // The ruler's own rail along its foot.
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(ruler.min.x, ruler.bottom()),
                    egui::pos2(ruler.max.x, ruler.bottom()),
                ],
                Weight::Hair,
                edge,
            );
            painter.extend(shapes);
        }

        // The heads, down the left: the same casing as the session's,
        // laid as a row's label. The cursor's row wears the ink edge.
        let name_font = egui::FontId::monospace(design::px(design::type_scale::BODY));
        let kind_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        for track in shown.clone() {
            let row = track - frame.row_offset;
            let head = frame.head(row);
            let here = track == self.arrangement.track;
            let outline = if here { ink } else { edge };
            let fill = self.square();
            let rail_x = head.left() + 16.0;
            kit::cached(
                painter,
                egui::Id::new(("stage-song-head", track)),
                head,
                (fill, ground, outline),
                |out| {
                    circuit::panel_variant(
                        out,
                        head,
                        Some(fill),
                        ground,
                        Some((Weight::Hair, outline)),
                        track as u8,
                    );
                    circuit::rail(
                        out,
                        egui::pos2(rail_x, head.top() + 6.0),
                        egui::pos2(rail_x, head.bottom() - 6.0),
                        &[0.0, 1.0],
                        ink,
                    );
                    Sign::General((track % 32) as u8).paint(
                        out,
                        egui::Rect::from_center_size(
                            egui::pos2(rail_x, head.center().y),
                            egui::Vec2::splat(14.0),
                        ),
                        Weight::Hair,
                        ink,
                    );
                },
            );
            let content_x = head.left() + 30.0;
            let sigil_side = design::px(design::space::STEP);
            // The family mark takes the foot's corner, so the name has
            // the head's whole width above it.
            if let Some(mark) = self.track_sigil(track) {
                let cell = egui::Rect::from_center_size(
                    egui::pos2(
                        head.max.x - 8.0 - sigil_side / 2.0,
                        head.max.y - 8.0 - sigil_side / 2.0,
                    ),
                    egui::Vec2::splat(sigil_side),
                );
                Sign::Seal(mark).painted(
                    painter,
                    egui::Id::new(("stage-song-head-mark", track)),
                    cell,
                    Weight::Hair,
                    ink,
                );
            }
            let head_data = &self.song.tracks[track];
            let cells = (((head.max.x - 6.0) - content_x)
                / painter
                    .layout_no_wrap("M".to_owned(), name_font.clone(), ink)
                    .size()
                    .x
                    .max(1.0))
            .floor()
            .max(1.0) as usize;
            let name: String = head_data.name.chars().take(cells).collect();
            painter.text(
                egui::pos2(content_x, head.min.y + 7.0),
                egui::Align2::LEFT_TOP,
                name,
                name_font.clone(),
                ink,
            );
            let mut kind = match head_data.kind {
                TrackKind::Audio => "AUDIO".to_owned(),
                TrackKind::Instrument => format!("{:02}", track + 1),
            };
            if head_data.muted {
                kind.push_str(" · MUTE");
            } else if head_data.solo {
                kind.push_str(" · SOLO");
            }
            painter.text(
                egui::pos2(content_x, head.max.y - 7.0),
                egui::Align2::LEFT_BOTTOM,
                kind,
                kind_font.clone(),
                ink.gamma_multiply(0.7),
            );
        }

        // The blocks: a casing the length it plays, the pattern's name,
        // and its trigs as a strip along the foot, repeated as the
        // pattern repeats, with a seam at each repeat.
        let cursor_block = self.song_block().map(|(_, block)| block.id);
        for track in shown.clone() {
            let row = track - frame.row_offset;
            let lane = frame.lane(row);
            let lane_inner = lane.shrink2(egui::vec2(0.0, 3.0));
            let data = &self.song.tracks[track];
            for block in &data.blocks {
                let end = block.start_tick.saturating_add(block.length_ticks);
                if end <= frame.view_start || block.start_tick >= frame.view_end() {
                    continue;
                }
                let rect = frame.span(lane_inner, block.start_tick, end);
                if rect.width() < 2.0 {
                    continue;
                }
                let here = cursor_block == Some(block.id);
                let selected = self
                    .arrangement
                    .block_selected(track, block.start_tick, end);
                let playing = sounding && block.start_tick <= now && now < end;
                let fill = if here {
                    cursor_shade
                } else if selected {
                    cursor_shade.gamma_multiply(0.55)
                } else if playing {
                    alpha.live_dim.color
                } else {
                    edge
                };
                let figure = if here || playing { ground } else { ink };
                let outline = if selected {
                    ink
                } else if playing {
                    alpha.live.color
                } else {
                    edge
                };
                let pattern = self.song.pattern(block.pattern_id);
                let pattern_len = pattern.map_or(1, |p| p.length_ticks.max(1));
                let step_ticks = PATTERN_STEP_TICKS.max(1);
                let step_px = step_ticks as f32 * frame.px_per_tick;
                kit::cached(
                    painter,
                    egui::Id::new(("stage-song-block", track, block.id.0)),
                    rect,
                    (fill, figure, outline, frame.view_start, frame.view_len),
                    |out| {
                        circuit::panel_variant(
                            out,
                            rect,
                            Some(fill),
                            ground,
                            Some((Weight::Hair, outline)),
                            (block.id.0 % 7) as u8,
                        );
                        let Some(pattern) = pattern else {
                            return;
                        };
                        // The trig strip along the foot: one cell a step,
                        // lit where the step sounds.
                        let strip_top = rect.max.y - 12.0;
                        let strip_bottom = rect.max.y - 5.0;
                        if step_px >= 2.0 && strip_top > rect.min.y + 14.0 {
                            let steps = (pattern_len / step_ticks).clamp(1, PATTERN_STEPS);
                            let mut offset = 0;
                            while offset < block.length_ticks {
                                let seam_x = frame.x(block.start_tick + offset);
                                if offset > 0 && seam_x > rect.min.x && seam_x < rect.max.x {
                                    circuit::trace(
                                        out,
                                        &[
                                            egui::pos2(seam_x, rect.min.y + 3.0),
                                            egui::pos2(seam_x, rect.max.y - 3.0),
                                        ],
                                        Weight::Hair,
                                        figure.gamma_multiply(0.45),
                                    );
                                }
                                for step in 0..steps {
                                    let at = offset + step * step_ticks;
                                    if at >= block.length_ticks {
                                        break;
                                    }
                                    let x0 = frame.x(block.start_tick + at).max(rect.min.x + 2.0);
                                    let x1 = (x0 + step_px - 1.0).min(rect.max.x - 2.0);
                                    if x1 <= x0 {
                                        continue;
                                    }
                                    let trig = pattern.trig(step);
                                    let hit = trig.enabled && !trig.notes.is_empty();
                                    let cell = egui::Rect::from_min_max(
                                        egui::pos2(x0, strip_top),
                                        egui::pos2(x1, strip_bottom),
                                    );
                                    out.push(egui::Shape::rect_filled(
                                        cell,
                                        0.0,
                                        if hit {
                                            figure.gamma_multiply(0.9)
                                        } else {
                                            figure.gamma_multiply(0.22)
                                        },
                                    ));
                                }
                                offset += pattern_len;
                            }
                        }
                    },
                );
                if let Some(pattern) = pattern
                    && rect.width() >= 30.0
                {
                    let label_w = rect.width() - 10.0;
                    let cells =
                        (label_w / (block::size(block::unit::MICRO) * 0.5)).floor() as usize;
                    let name: String = pattern.name.chars().take(cells.max(1)).collect();
                    block::paint(
                        painter,
                        egui::Id::new(("stage-song-block-name", block.id.0)),
                        egui::pos2(rect.min.x + 6.0, rect.min.y + 4.0),
                        egui::Align2::LEFT_TOP,
                        block::unit::MICRO,
                        &name,
                        figure,
                    );
                }
            }
            // Audio blocks: a casing with the sound's name and a wave
            // rail — the file's peaks are a later note.
            for block in &data.audio_blocks {
                let end = block.end_tick();
                if end <= frame.view_start || block.start_tick >= frame.view_end() {
                    continue;
                }
                let rect = frame.span(lane_inner, block.start_tick, end);
                if rect.width() < 2.0 {
                    continue;
                }
                let selected = self
                    .arrangement
                    .block_selected(track, block.start_tick, end);
                let playing = sounding && block.start_tick <= now && now < end;
                let fill = if selected {
                    cursor_shade.gamma_multiply(0.55)
                } else if playing {
                    alpha.live_dim.color
                } else {
                    self.square()
                };
                let outline = if selected {
                    ink
                } else if playing {
                    alpha.live.color
                } else {
                    edge
                };
                let figure = if playing { ground } else { ink };
                kit::cached(
                    painter,
                    egui::Id::new(("stage-song-audio", track, block.id.0)),
                    rect,
                    (fill, figure, outline, frame.view_start, frame.view_len),
                    |out| {
                        circuit::panel_variant(
                            out,
                            rect,
                            Some(fill),
                            ground,
                            Some((Weight::Hair, outline)),
                            (block.id.0 % 7) as u8,
                        );
                        let y = rect.max.y - 9.0;
                        circuit::rail(
                            out,
                            egui::pos2(rect.min.x + 6.0, y),
                            egui::pos2(rect.max.x - 6.0, y),
                            &[0.0, 0.25, 0.5, 0.75, 1.0],
                            figure.gamma_multiply(0.7),
                        );
                    },
                );
                if rect.width() >= 30.0 {
                    let cells = ((rect.width() - 10.0) / (block::size(block::unit::MICRO) * 0.5))
                        .floor() as usize;
                    let name: String = block.name.chars().take(cells.max(1)).collect();
                    block::paint(
                        painter,
                        egui::Id::new(("stage-song-audio-name", block.id.0)),
                        egui::pos2(rect.min.x + 6.0, rect.min.y + 4.0),
                        egui::Align2::LEFT_TOP,
                        block::unit::MICRO,
                        &name,
                        figure,
                    );
                }
            }
        }

        self.draw_song_marks(painter, &frame, phase);

        // Empty selected cells retain a visible wash. Filled cells are
        // represented by the whole selected block above.
        let grid = self.arrangement.grid_ticks(&self.song);
        for &(track, tick) in &self.arrangement.selected {
            if tick % grid != 0
                || !shown.contains(&track)
                || tick < frame.view_start
                || tick >= frame.view_end()
            {
                continue;
            }
            let end = tick.saturating_add(grid);
            if self.song.tracks[track]
                .blocks_in_time_order()
                .iter()
                .any(|block| block.intersects(tick, end))
            {
                continue;
            }
            let lane = frame.lane(track - frame.row_offset);
            let rect = frame.span(lane.shrink2(egui::vec2(0.0, 3.0)), tick, tick + grid);
            painter.rect_filled(rect, 0.0, cursor_shade.gamma_multiply(0.22));
        }

        // The playhead: a heavy rule in the live ink from the ruler to
        // the foot of the lanes, breathing with the beat, with its
        // marker on the ruler. Only while the SONG sounds — in scene
        // mode the transport is the session's, and the song's timeline
        // has nothing to say about it.
        if sounding && now >= frame.view_start && now < frame.view_end() {
            let x = frame.x(now);
            let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
            let mut shapes = Vec::new();
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(x, frame.ruler.min.y + 4.0),
                    egui::pos2(x, frame.lanes.max.y),
                ],
                Weight::Heavy,
                live,
            );
            shapes.push(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x - 5.0, frame.ruler.min.y),
                    egui::pos2(x + 5.0, frame.ruler.min.y),
                    egui::pos2(x, frame.ruler.min.y + 7.0),
                ],
                live,
                egui::Stroke::NONE,
            ));
            painter.extend(shapes);
        }

        // The cursor: the house brackets on the block under it, or on
        // the cell it stands in.
        if let Some(track) = self.song_track()
            && shown.contains(&track)
        {
            let lane = frame.lane(track - frame.row_offset);
            let target = match self.song_block() {
                Some((_, block)) => frame.span(
                    lane.shrink2(egui::vec2(0.0, 3.0)),
                    block.start_tick,
                    block.start_tick.saturating_add(block.length_ticks),
                ),
                None if self.song_audio_block().is_some() => {
                    let (_, block) = self.song_audio_block().expect("just found");
                    frame.span(
                        lane.shrink2(egui::vec2(0.0, 3.0)),
                        block.start_tick,
                        block.end_tick(),
                    )
                }
                None => {
                    let grid = self.arrangement.grid_ticks(&self.song);
                    frame.span(
                        lane.shrink2(egui::vec2(0.0, 3.0)),
                        self.arrangement.tick,
                        self.arrangement.tick + grid,
                    )
                }
            };
            if target.is_positive() {
                let mut shapes = Vec::new();
                if self.song_block().is_none() && self.song_audio_block().is_none() {
                    shapes.push(egui::Shape::rect_filled(
                        target,
                        0.0,
                        cursor_shade.gamma_multiply(0.35),
                    ));
                }
                painter.extend(shapes);
                if holds_the_keys {
                    crate::ui::nav_cursor::claim(
                        painter,
                        "stage-song-cursor",
                        target,
                        crate::ui::nav_cursor::Kind::Block,
                        crate::ui::nav_cursor::Layer::Surface,
                        ink,
                    );
                }
            }
        }

        // The minimap: the whole song along the foot, every block a
        // dash in its track's row, the window drawn on it, the cursor's
        // tick a hairline.
        let map = frame.minimap;
        if map.is_positive() {
            let song_end = self.song.end_tick().max(frame.view_end()).max(1);
            let scale = map.width() / song_end as f32;
            let rows = tracks.max(1);
            let row_h = ((map.height() - 4.0) / rows as f32).max(1.0);
            let mut shapes = Vec::new();
            circuit::panel_frame_variant(&mut shapes, map, Weight::Hair, edge, 3);
            for (track, data) in self.song.tracks.iter().enumerate() {
                let y0 = map.min.y + 2.0 + track as f32 * row_h;
                let y1 = (y0 + row_h - 1.0).max(y0 + 1.0);
                for block in data.blocks_in_time_order() {
                    let x0 = map.min.x + block.start_tick() as f32 * scale;
                    let x1 = (map.min.x + block.end_tick() as f32 * scale).max(x0 + 1.0);
                    let playing = sounding && block.start_tick() <= now && now < block.end_tick();
                    shapes.push(egui::Shape::rect_filled(
                        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)),
                        0.0,
                        if playing {
                            alpha.live.color
                        } else {
                            ink.gamma_multiply(0.7)
                        },
                    ));
                }
            }
            let wx0 = map.min.x + frame.view_start as f32 * scale;
            let wx1 = (map.min.x + frame.view_end() as f32 * scale).min(map.max.x);
            shapes.push(egui::Shape::rect_stroke(
                egui::Rect::from_min_max(egui::pos2(wx0, map.min.y), egui::pos2(wx1, map.max.y)),
                0.0,
                egui::Stroke::new(1.0, ink),
                egui::StrokeKind::Inside,
            ));
            let cx = map.min.x + self.arrangement.tick as f32 * scale;
            circuit::trace(
                &mut shapes,
                &[egui::pos2(cx, map.min.y), egui::pos2(cx, map.max.y)],
                Weight::Hair,
                cursor_shade,
            );
            if sounding {
                let px = map.min.x + now as f32 * scale;
                circuit::trace(
                    &mut shapes,
                    &[egui::pos2(px, map.min.y), egui::pos2(px, map.max.y)],
                    Weight::Hair,
                    alpha.live.color,
                );
            }
            painter.extend(shapes);
        }
    }

    /// The brace, the locators, and the takes being written, over the
    /// lanes: drawn after the blocks and before the playhead.
    pub(super) fn draw_song_marks(&self, painter: &egui::Painter, frame: &Frame, phase: Phase) {
        let alpha = self.alphabet();
        let edge = alpha.edge.color;
        let ink = alpha.ink.color;
        let mut shapes = Vec::new();

        // The brace: brackets on the ruler between its ends, and a wash
        // over the lanes inside it while it is on.
        if let Some((start, end)) = self.song.loop_brace
            && end > start
            && end > frame.view_start
            && start < frame.view_end()
        {
            let x0 = frame.x(start).max(frame.ruler.min.x);
            let x1 = frame.x(end).min(frame.ruler.max.x);
            let brace_ink = if self.song.loop_on {
                alpha.live.color
            } else {
                edge
            };
            let band = egui::Rect::from_min_max(
                egui::pos2(x0, frame.ruler.min.y),
                egui::pos2(x1, frame.ruler.min.y + 9.0),
            );
            if band.is_positive() {
                circuit::brackets(&mut shapes, band, 6.0, Weight::Bold, brace_ink);
                circuit::trace(
                    &mut shapes,
                    &[
                        egui::pos2(x0, frame.ruler.min.y + 1.0),
                        egui::pos2(x1, frame.ruler.min.y + 1.0),
                    ],
                    Weight::Hair,
                    brace_ink,
                );
            }
            if self.song.loop_on {
                shapes.push(egui::Shape::rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, frame.lanes.min.y),
                        egui::pos2(x1, frame.lanes.max.y),
                    ),
                    0.0,
                    alpha.live_dim.color.gamma_multiply(0.12),
                ));
            }
        }

        // Locators: a plaque on the ruler and a hairline down the lanes.
        for locator in &self.song.locators {
            if locator.tick < frame.view_start || locator.tick >= frame.view_end() {
                continue;
            }
            let x = frame.x(locator.tick);
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(x, frame.lanes.min.y),
                    egui::pos2(x, frame.lanes.max.y),
                ],
                Weight::Hair,
                edge.gamma_multiply(0.9),
            );
            shapes.push(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x, frame.ruler.max.y - 1.0),
                    egui::pos2(x - 4.0, frame.ruler.max.y - 7.0),
                    egui::pos2(x + 4.0, frame.ruler.max.y - 7.0),
                ],
                ink,
                egui::Stroke::NONE,
            ));
            block::paint(
                painter,
                egui::Id::new(("stage-song-locator", locator.tick)),
                egui::pos2(x + 6.0, frame.ruler.max.y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                block::unit::MICRO,
                &locator.name,
                ink,
            );
        }

        // Takes: the blocks arriving, dashed in the live ink, the open
        // ones growing to the playhead.
        if !self.takes.is_empty() {
            let now = self.transport.tick();
            let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
            for take in &self.takes {
                if take.track < frame.row_offset || take.track >= frame.row_offset + frame.rows {
                    continue;
                }
                let end = take.end.unwrap_or(now).max(take.start + 1);
                if end <= frame.view_start || take.start >= frame.view_end() {
                    continue;
                }
                let lane = frame.lane(take.track - frame.row_offset);
                let rect = frame.span(lane.shrink2(egui::vec2(0.0, 3.0)), take.start, end);
                if !rect.is_positive() {
                    continue;
                }
                shapes.push(egui::Shape::rect_filled(
                    rect,
                    0.0,
                    alpha.live_dim.color.gamma_multiply(0.35),
                ));
                circuit::dashes(
                    &mut shapes,
                    &[
                        rect.left_top(),
                        rect.right_top(),
                        rect.right_bottom(),
                        rect.left_bottom(),
                        rect.left_top(),
                    ],
                    phase.dash(),
                    Weight::Hair,
                    live,
                );
            }
        }
        painter.extend(shapes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::arrangement::tests::song_with_tracks;

    /// The frame puts the heads on the left, the ruler over the lanes,
    /// and maps ticks across the time area.
    #[test]
    fn the_frame_lays_heads_left_and_time_across() {
        let song = song_with_tracks(3);
        let arr = Arrangement::default();
        let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 400.0));
        let frame = Frame::of(field, &arr, &song);
        assert!(frame.heads.max.x < frame.ruler.min.x);
        assert!(frame.ruler.max.y <= frame.lanes.min.y);
        assert!(frame.lanes.max.y <= frame.minimap.min.y);
        assert_eq!(frame.x(0), frame.ruler.min.x);
        assert!((frame.x(frame.view_len) - frame.ruler.max.x).abs() < 0.01);
        assert_eq!(frame.head(1).min.y, frame.lane(1).min.y);
        assert!(frame.rows >= 3);
    }
}
