//! The song view: the arrangement, time across and tracks down.
//!
//! The session is a table of what CAN play; the song is a picture of
//! what DOES, laid on a timeline. Tab turns the field from one to the
//! other. The heads stand down the left as the rows' labels, the ruler
//! runs across the top, and every block sits in its track's lane at the
//! bar it plays from, carrying a strip of its pattern's trigs so it
//! reads as rhythm before it reads as a name.
//!
//! One cursor, on a (track, cell) of the grid, and on a block when one
//! stands there. Left and Right walk cells — a bar, or a beat when the
//! view is close enough to show them; Up and Down walk tracks; the view
//! pages after the cursor. Every verb here acts on the block or cell
//! under it, and the tray beneath shows that block's pattern.

use std::collections::HashMap;

use super::*;
use crate::sequencing::{BlockId, PatternBlock, PatternId, TICKS_PER_BEAT, Track};

/// How many bars the time area spans, from the closest look to the
/// widest. The closest shows beats as the grid; the rest walk in bars.
pub const BARS_ACROSS: [usize; 6] = [2, 4, 8, 16, 32, 64];
/// Eight bars across: enough to see a phrase and place the next.
pub const DEFAULT_ZOOM: usize = 2;
/// A view close enough for beats to be the grid.
const BEAT_GRID_BARS: usize = 4;

/// The heads' column, on the left. The session's own head width, so a
/// track is the same object at the same size in both views.
pub const HEAD_W: f32 = 132.0;
/// One lane's height, and the gap between lanes.
pub const LANE_H: f32 = 48.0;
pub const LANE_GAP: f32 = 6.0;
/// The ruler across the top of the lanes.
pub const RULER_H: f32 = 24.0;
/// The minimap along the foot: the whole song, and the window on it.
pub const MINIMAP_H: f32 = 18.0;

/// A held verb: the next Left or Right moves or resizes the block under
/// the cursor rather than walking it, and keeps doing so until Escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hold {
    Nudge,
    Resize,
}

impl Hold {
    pub fn word(self) -> &'static str {
        match self {
            Hold::Nudge => "NUDGE · ← →",
            Hold::Resize => "RESIZE · ← →",
        }
    }
}

/// The song view's state: where the cursor stands, what the view shows,
/// and the memory each track keeps of the last pattern placed on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrangement {
    /// The cursor's track: a row.
    pub track: usize,
    /// The cursor's tick, on the grid.
    pub tick: usize,
    /// The first tick the time area shows.
    pub view_start: usize,
    /// An index into [`BARS_ACROSS`].
    pub zoom: usize,
    pub hold: Option<Hold>,
    /// The last pattern placed on each track, so Enter on an empty cell
    /// lays the one the performer is building with.
    pub last_placed: HashMap<usize, PatternId>,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            track: 0,
            tick: 0,
            view_start: 0,
            zoom: DEFAULT_ZOOM,
            hold: None,
            last_placed: HashMap::new(),
        }
    }
}

/// The ticks in one bar at `tick`, from the song's meter there.
pub fn bar_ticks(song: &Song, tick: usize) -> usize {
    let (num, den) = song.meter_at(tick, (4, 4));
    let beats = (u64::from(num) * 4 / u64::from(den.max(1))).max(1) as usize;
    beats * TICKS_PER_BEAT
}

impl Arrangement {
    pub fn bars_across(&self) -> usize {
        BARS_ACROSS[self.zoom.min(BARS_ACROSS.len() - 1)]
    }

    /// Whether the view is close enough for beats to be the grid.
    pub fn beat_grid(&self) -> bool {
        self.bars_across() <= BEAT_GRID_BARS
    }

    /// One cell of the grid, in ticks: a beat when the view is close, a
    /// bar otherwise.
    pub fn grid_ticks(&self, song: &Song) -> usize {
        if self.beat_grid() {
            TICKS_PER_BEAT
        } else {
            bar_ticks(song, self.tick)
        }
    }

    /// The ticks the time area spans.
    pub fn view_len(&self, song: &Song) -> usize {
        (self.bars_across() * bar_ticks(song, self.view_start)).max(1)
    }

    /// `tick` brought down onto the grid.
    pub fn snap(&self, song: &Song, tick: usize) -> usize {
        let grid = self.grid_ticks(song).max(1);
        tick / grid * grid
    }

    /// The view pages after the cursor: the page the cursor is on, and
    /// the cursor keeps its place on it as it walks.
    pub fn fit(&mut self, song: &Song) {
        let len = self.view_len(song);
        self.view_start = self.tick / len * len;
    }

    /// Walk the cursor one cell or one track. Left at the song's start
    /// and Up at the first track refuse; Right has no end, because the
    /// song has none until something is placed at it.
    pub fn step(&mut self, song: &Song, step: Step) -> bool {
        match step {
            Step::Left => {
                if self.tick == 0 {
                    return false;
                }
                let grid = self.grid_ticks(song);
                self.tick = self.snap(song, self.tick.saturating_sub(grid));
                self.fit(song);
                true
            }
            Step::Right => {
                let grid = self.grid_ticks(song);
                self.tick = self.snap(song, self.tick) + grid;
                self.fit(song);
                true
            }
            Step::Up => {
                if self.track == 0 {
                    return false;
                }
                self.track -= 1;
                true
            }
            Step::Down => {
                if self.track + 1 >= song.tracks.len() {
                    return false;
                }
                self.track += 1;
                true
            }
        }
    }

    /// Closer (`true`) or wider. Refused at either end of the range. The
    /// cursor stays where it is, brought onto the new grid.
    pub fn zoom(&mut self, song: &Song, closer: bool) -> bool {
        let next = if closer {
            self.zoom.checked_sub(1)
        } else {
            (self.zoom + 1 < BARS_ACROSS.len()).then_some(self.zoom + 1)
        };
        let Some(next) = next else {
            return false;
        };
        self.zoom = next;
        self.tick = self.snap(song, self.tick);
        self.fit(song);
        true
    }

    /// The edges on the cursor's track: the song's start and every
    /// block's two ends.
    fn edges(&self, song: &Song) -> Vec<usize> {
        let mut edges = vec![0];
        if let Some(track) = song.tracks.get(self.track) {
            for block in track.blocks_in_time_order() {
                edges.push(block.start_tick());
                edges.push(block.end_tick());
            }
        }
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    /// Jump to the next block edge along, or the previous one back.
    pub fn jump(&mut self, song: &Song, forward: bool) -> bool {
        let edges = self.edges(song);
        let to = if forward {
            edges.iter().copied().find(|&edge| edge > self.tick)
        } else {
            edges.iter().copied().rev().find(|&edge| edge < self.tick)
        };
        let Some(to) = to else {
            return false;
        };
        self.tick = to;
        self.fit(song);
        true
    }

    /// The pattern block under the cursor on `track`, by index.
    pub fn block_index(&self, track: &Track) -> Option<usize> {
        track.blocks.iter().position(|block| {
            block.start_tick <= self.tick
                && self.tick < block.start_tick.saturating_add(block.length_ticks)
        })
    }
}

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
    /// Whether the song view holds the keys.
    pub(super) fn in_song(&self) -> bool {
        self.scope_context() == keymap::ScopeContext::Song
    }

    /// The cursor's track, when it exists.
    fn song_track(&self) -> Option<usize> {
        (self.arrangement.track < self.song.tracks.len()).then_some(self.arrangement.track)
    }

    /// The pattern block under the song cursor: its track and a copy.
    pub(super) fn song_block(&self) -> Option<(usize, PatternBlock)> {
        let track = self.song_track()?;
        let index = self.arrangement.block_index(&self.song.tracks[track])?;
        Some((track, self.song.tracks[track].blocks[index].clone()))
    }

    /// The pattern the song cursor addresses, for the tray.
    pub(super) fn song_clip(&self) -> Option<Opened> {
        let (track, block) = self.song_block()?;
        Some(Opened {
            pattern: block.pattern_id,
            track,
        })
    }

    /// Where the transport stands inside `opened`'s pattern while the
    /// song plays through a block of it on that track.
    pub(super) fn song_playhead(&self, opened: Opened) -> Option<usize> {
        let track = self.song.tracks.get(opened.track)?;
        let tick = self.transport.tick();
        let block = track.blocks.iter().find(|block| {
            block.pattern_id == opened.pattern
                && block.start_tick <= tick
                && tick < block.start_tick.saturating_add(block.length_ticks)
        })?;
        let length = sequencer::pattern_length(&self.song, opened.pattern).max(1);
        Some((tick - block.start_tick) % length)
    }

    /// The pattern Enter lays on an empty cell of `track`: the last one
    /// placed there, else the one the session cursor holds on that
    /// track, else the first the track's column holds — and a track
    /// with nothing at all gets a new pattern in its first empty slot.
    fn pattern_to_place(&mut self, track: usize) -> Option<PatternId> {
        if let Some(&pattern) = self.arrangement.last_placed.get(&track) {
            return Some(pattern);
        }
        if let Some(Address::Slot { track: at, scene }) = self.session_address()
            && at == track
            && let Some(Clip::Pattern(pattern)) = self.song.slot_clip(track, scene)
        {
            return Some(pattern);
        }
        let scenes = self.song.session.scenes.len();
        for scene in 0..scenes {
            if let Some(Clip::Pattern(pattern)) = self.song.slot_clip(track, scene) {
                return Some(pattern);
            }
        }
        let empty = (0..scenes).find(|&scene| self.song.slot_clip(track, scene).is_none())?;
        let made = self.song.fill_slot(track, empty)?;
        self.touched();
        Some(made)
    }

    /// The song view's own verbs.
    pub(super) fn apply_song(&mut self, intent: SongIntent) -> Result<(), RefusalReason> {
        if !self.in_song() {
            return Err(RefusalReason::Unavailable);
        }
        match intent {
            SongIntent::ZoomIn | SongIntent::ZoomOut => {
                let closer = intent == SongIntent::ZoomIn;
                if self.arrangement.zoom(&self.song, closer) {
                    self.notice = Some(format!("{} bars", self.arrangement.bars_across()));
                    Ok(())
                } else {
                    Err(RefusalReason::Edge(if closer {
                        Step::Up
                    } else {
                        Step::Down
                    }))
                }
            }
            SongIntent::JumpPrev | SongIntent::JumpNext => {
                let forward = intent == SongIntent::JumpNext;
                self.arrangement
                    .jump(&self.song, forward)
                    .then_some(())
                    .ok_or(RefusalReason::Edge(if forward {
                        Step::Right
                    } else {
                        Step::Left
                    }))
            }
            SongIntent::Resize => {
                let (_, block) = self.song_block().ok_or(RefusalReason::Empty)?;
                self.arrangement.hold = Some(Hold::Resize);
                self.notice = Some(format!(
                    "{} · {}",
                    Hold::Resize.word(),
                    crate::ui::sequencer::grid_resolution::bars_label(block.length_ticks)
                ));
                Ok(())
            }
            SongIntent::Stretch(step) => {
                let bar = bar_ticks(&self.song, self.arrangement.tick);
                self.resize_block(step, bar)
            }
            SongIntent::Duplicate => {
                let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
                let start = block.start_tick.saturating_add(block.length_ticks);
                self.lay_block(track, block.pattern_id, start, block.length_ticks)?;
                self.arrangement.tick = start;
                self.arrangement.fit(&self.song);
                Ok(())
            }
            SongIntent::Pick | SongIntent::PickBack => {
                let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
                let column: Vec<PatternId> = (0..self.song.session.scenes.len())
                    .filter_map(|scene| match self.song.slot_clip(track, scene) {
                        Some(Clip::Pattern(pattern)) => Some(pattern),
                        _ => None,
                    })
                    .collect();
                if column.len() < 2 {
                    return Err(RefusalReason::Empty);
                }
                let at = column
                    .iter()
                    .position(|&pattern| pattern == block.pattern_id)
                    .unwrap_or(0);
                let next = if intent == SongIntent::Pick {
                    (at + 1) % column.len()
                } else {
                    (at + column.len() - 1) % column.len()
                };
                let pattern = column[next];
                if let Some(slot) = self.song.tracks[track]
                    .blocks
                    .iter_mut()
                    .find(|candidate| candidate.id == block.id)
                {
                    slot.pattern_id = pattern;
                }
                self.arrangement.last_placed.insert(track, pattern);
                self.notice = self
                    .song
                    .pattern(pattern)
                    .map(|pattern| pattern.name.clone());
                self.touched();
                Ok(())
            }
        }
    }

    /// Lay `pattern` on `track` at `start`, `length` long, saying so.
    fn lay_block(
        &mut self,
        track: usize,
        pattern: PatternId,
        start: usize,
        length: usize,
    ) -> Result<BlockId, RefusalReason> {
        let id = self
            .song
            .place_block(track, pattern, start, length)
            .map_err(|refusal| {
                self.notice = Some(refusal.sign().to_owned());
                RefusalReason::Unavailable
            })?;
        self.arrangement.last_placed.insert(track, pattern);
        let name = self
            .song
            .pattern(pattern)
            .map_or_else(String::new, |pattern| pattern.name.clone());
        self.notice = Some(format!(
            "+ {name} · {}",
            crate::ui::sequencer::grid_resolution::bars_label(length)
        ));
        self.touched();
        Ok(id)
    }

    /// A step in the song view: the held verb's motion, or the cursor's.
    pub(super) fn song_step(&mut self, step: Step) -> Result<(), RefusalReason> {
        match self.arrangement.hold {
            Some(Hold::Nudge) if matches!(step, Step::Left | Step::Right) => {
                let grid = self.arrangement.grid_ticks(&self.song);
                self.move_block(step, grid)
            }
            Some(Hold::Resize) if matches!(step, Step::Left | Step::Right) => {
                let grid = self.arrangement.grid_ticks(&self.song);
                self.resize_block(step, grid)
            }
            _ => self
                .arrangement
                .step(&self.song, step)
                .then_some(())
                .ok_or(RefusalReason::Edge(step)),
        }
    }

    /// The block under the cursor `by` ticks along or back; the cursor
    /// goes with it.
    fn move_block(&mut self, step: Step, by: usize) -> Result<(), RefusalReason> {
        let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
        let start = match step {
            Step::Left if block.start_tick == 0 => return Err(RefusalReason::Edge(step)),
            Step::Left => block.start_tick.saturating_sub(by),
            _ => block.start_tick.saturating_add(by),
        };
        if !self.song.move_block(track, block.id, start) {
            self.notice = Some(crate::sequencing::LandRefusal::Occupied.sign().to_owned());
            return Err(RefusalReason::Unavailable);
        }
        self.arrangement.tick = start + (self.arrangement.tick - block.start_tick);
        self.arrangement.fit(&self.song);
        self.touched();
        Ok(())
    }

    /// The block under the cursor `by` ticks longer or shorter, from its
    /// end. A block is never shorter than one cell.
    fn resize_block(&mut self, step: Step, by: usize) -> Result<(), RefusalReason> {
        let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
        let grid = self.arrangement.grid_ticks(&self.song);
        let length = match step {
            Step::Left => block.length_ticks.saturating_sub(by).max(grid),
            _ => block.length_ticks.saturating_add(by),
        };
        if length == block.length_ticks {
            return Err(RefusalReason::Edge(step));
        }
        if !self.song.resize_block(track, block.id, length) {
            self.notice = Some(crate::sequencing::LandRefusal::Occupied.sign().to_owned());
            return Err(RefusalReason::Unavailable);
        }
        self.notice = Some(crate::ui::sequencer::grid_resolution::bars_label(length));
        // The cursor stays inside the block it is resizing.
        let end = block.start_tick + length;
        if self.arrangement.tick >= end {
            self.arrangement.tick = self.arrangement.snap(&self.song, end - 1);
            self.arrangement.fit(&self.song);
        }
        self.touched();
        Ok(())
    }

    /// Enter in the song view: a block opens in the tray; an empty cell
    /// takes a block, one pattern long or as long as the gap allows.
    pub(super) fn song_enter(&mut self) -> Result<(), RefusalReason> {
        let track = self.song_track().ok_or(RefusalReason::Unavailable)?;
        if let Some((track, block)) = self.song_block() {
            let entered = self
                .focus
                .enter(FocusScope::grid(PATTERN_COLS, PATTERN_ROWS));
            if entered {
                self.inside = Some(Opened {
                    pattern: block.pattern_id,
                    track,
                });
            }
            return entered.then_some(()).ok_or(RefusalReason::Deeper);
        }
        let pattern = self
            .pattern_to_place(track)
            .ok_or(RefusalReason::Unavailable)?;
        let start = self.arrangement.tick;
        let wanted = sequencer::pattern_length(&self.song, pattern).max(1);
        // Cut to the gap before the next block, if one is close.
        let next = self.song.tracks[track]
            .blocks_in_time_order()
            .into_iter()
            .map(|block| block.start_tick())
            .filter(|&at| at > start)
            .min();
        let length = next.map_or(wanted, |at| wanted.min(at - start));
        self.lay_block(track, pattern, start, length).map(|_| ())
    }

    /// Delete in the song view: the block under the cursor goes.
    pub(super) fn song_clear(&mut self) -> Result<(), RefusalReason> {
        let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
        let taken = self
            .song
            .remove_block(track, block.id)
            .ok_or(RefusalReason::Empty)?;
        self.notice = self
            .song
            .pattern(taken.pattern_id)
            .map(|pattern| format!("- {}", pattern.name));
        self.touched();
        Ok(())
    }

    /// Lift the block under the cursor off its lane and keep it.
    pub(super) fn song_yank(&mut self) -> Result<(), RefusalReason> {
        let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
        let taken = self
            .song
            .remove_block(track, block.id)
            .ok_or(RefusalReason::Empty)?;
        self.notice = self
            .song
            .pattern(taken.pattern_id)
            .map(|pattern| format!("yanked {}", pattern.name));
        self.block_clipboard = Some(taken);
        self.touched();
        Ok(())
    }

    /// Put the kept block down at the cursor, on the cursor's track.
    pub(super) fn song_put(&mut self) -> Result<(), RefusalReason> {
        let kept = self.block_clipboard.clone().ok_or(RefusalReason::Empty)?;
        let track = self.song_track().ok_or(RefusalReason::Unavailable)?;
        let start = self.arrangement.tick;
        self.lay_block(track, kept.pattern_id, start, kept.length_ticks)
            .map(|_| ())
    }

    /// Escape in the song view: a held verb lets go; otherwise out to
    /// the session.
    pub(super) fn song_escape(&mut self) -> Result<(), RefusalReason> {
        if self.help {
            self.help = false;
            return Ok(());
        }
        if self.arrangement.hold.take().is_some() {
            self.notice = None;
            return Ok(());
        }
        self.song_view = false;
        self.notice = Some("session".to_owned());
        Ok(())
    }

    // ------------------------------------------------------------ drawing

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
                let playing = sounding && block.start_tick <= now && now < end;
                let fill = if here {
                    cursor_shade
                } else if playing {
                    alpha.live_dim.color
                } else {
                    edge
                };
                let figure = if here || playing { ground } else { ink };
                let outline = if playing { alpha.live.color } else { edge };
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
                let playing = sounding && block.start_tick <= now && now < end;
                let fill = if playing {
                    alpha.live_dim.color
                } else {
                    self.square()
                };
                let outline = if playing { alpha.live.color } else { edge };
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
                let bracket_ink = if holds_the_keys { ink } else { edge };
                let mut shapes = Vec::new();
                if self.song_block().is_none() {
                    shapes.push(egui::Shape::rect_filled(
                        target,
                        0.0,
                        cursor_shade.gamma_multiply(0.35),
                    ));
                }
                circuit::brackets(
                    &mut shapes,
                    target.expand(2.0),
                    8.0,
                    Weight::Bold,
                    bracket_ink,
                );
                painter.extend(shapes);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song_with_tracks(n: usize) -> Song {
        let mut song = Song::default();
        // The default song comes furnished; these tests want bare lanes.
        song.tracks.clear();
        for _ in 0..n {
            song.add_track(TrackKind::Instrument);
        }
        song
    }

    /// At the default zoom the grid is a bar; close in, a beat. The
    /// cursor walks in the grid's own unit either way.
    #[test]
    fn the_grid_is_a_bar_far_out_and_a_beat_close_in() {
        let song = song_with_tracks(2);
        let mut arr = Arrangement::default();
        assert_eq!(arr.grid_ticks(&song), TICKS_PER_BEAT * 4);
        assert!(arr.step(&song, Step::Right));
        assert_eq!(arr.tick, TICKS_PER_BEAT * 4);
        assert!(arr.zoom(&song, true));
        assert!(arr.beat_grid());
        assert!(arr.step(&song, Step::Right));
        assert_eq!(arr.tick, TICKS_PER_BEAT * 5);
        assert!(arr.step(&song, Step::Down));
        assert!(!arr.step(&song, Step::Down), "walked past the last track");
        assert!(arr.step(&song, Step::Up));
        assert!(!arr.step(&song, Step::Up), "walked above the first track");
    }

    /// Left at the start refuses; Right pages the view along after the
    /// cursor, and zooming out keeps the cursor on the page it is on.
    #[test]
    fn the_view_pages_after_the_cursor() {
        let song = song_with_tracks(1);
        let mut arr = Arrangement::default();
        assert!(!arr.step(&song, Step::Left));
        let len = arr.view_len(&song);
        for _ in 0..arr.bars_across() {
            assert!(arr.step(&song, Step::Right));
        }
        assert_eq!(
            arr.view_start, len,
            "the cursor left the page and the view stayed"
        );
        assert!(arr.zoom(&song, false));
        assert_eq!(
            arr.view_start, 0,
            "at twice the width the cursor is on the first page"
        );
        assert!(arr.view_start <= arr.tick && arr.tick < arr.view_start + arr.view_len(&song));
    }

    /// The zoom stops at both ends of its range.
    #[test]
    fn zoom_refuses_past_its_range() {
        let song = song_with_tracks(1);
        let mut arr = Arrangement::default();
        while arr.zoom(&song, true) {}
        assert_eq!(arr.bars_across(), BARS_ACROSS[0]);
        while arr.zoom(&song, false) {}
        assert_eq!(arr.bars_across(), *BARS_ACROSS.last().expect("a range"));
    }

    /// Jumps land on block edges and the song's start, and refuse when
    /// there is no edge that way.
    #[test]
    fn jumps_walk_the_edges_of_the_blocks() {
        let mut song = song_with_tracks(1);
        let pattern = song.fill_slot(0, 0).expect("a pattern");
        let bar = bar_ticks(&song, 0);
        song.place_block(0, pattern, 2 * bar, 3 * bar)
            .expect("placed");
        let mut arr = Arrangement::default();
        assert!(!arr.jump(&song, false), "nothing before the start");
        assert!(arr.jump(&song, true));
        assert_eq!(arr.tick, 2 * bar);
        assert!(arr.jump(&song, true));
        assert_eq!(arr.tick, 5 * bar);
        assert!(!arr.jump(&song, true), "nothing after the last edge");
        assert!(arr.jump(&song, false));
        assert_eq!(arr.tick, 2 * bar);
    }

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
