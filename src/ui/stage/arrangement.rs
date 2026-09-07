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

use std::collections::{BTreeSet, HashMap, HashSet};

use super::*;
use crate::sequencing::{AudioBlock, BlockId, PatternBlock, PatternId, TICKS_PER_BEAT, Track};

/// Sparse arrangement material, positioned relative to the selection's
/// top-left tick and track. The tick mask is kept separately from the
/// blocks so empty selected cells remain meaningful spacing.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ArrangementClipboard {
    pub width_ticks: usize,
    pub height: usize,
    pub mask: Vec<(usize, usize)>,
    pub patterns: Vec<(usize, usize, PatternBlock)>,
    pub audio: Vec<(usize, usize, AudioBlock)>,
}

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
    /// Tick-granular cells retain their exact time span if the view's
    /// beat/bar grid changes after selection.
    pub selected: BTreeSet<(usize, usize)>,
    pub selection_anchor: Option<(usize, usize)>,
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
            selected: BTreeSet::new(),
            selection_anchor: None,
        }
    }
}

/// The ticks in one bar at `tick`, from the song's meter there.
pub fn bar_ticks(song: &Song, tick: usize) -> usize {
    let (num, den) = song.meter_at(tick, (4, 4));
    (u64::from(num.max(1))
        .saturating_mul(TICKS_PER_BEAT as u64)
        .saturating_mul(4)
        / u64::from(den.max(1)))
    .max(1) as usize
}

/// One notated beat at `tick`. `TICKS_PER_BEAT` is a quarter note, so
/// 7/8 advances in 24-tick eighth-note beats rather than 48-tick quarters.
fn beat_ticks(song: &Song, tick: usize) -> usize {
    let (_, denominator) = song.meter_at(tick, (4, 4));
    (TICKS_PER_BEAT.saturating_mul(4) / denominator.max(1) as usize).max(1)
}

fn meter_origin(song: &Song, tick: usize) -> usize {
    song.meter
        .iter()
        .filter(|mark| mark.tick <= tick && usable_meter_mark(mark))
        .max_by_key(|mark| mark.tick)
        .map_or(0, |mark| mark.tick)
}

fn usable_meter_mark(mark: &crate::sequencing::MeterMark) -> bool {
    (1..=crate::sequencing::MAX_METER_NUMERATOR).contains(&mark.numerator)
        && mark.denominator.is_power_of_two()
        && mark.denominator <= crate::sequencing::MAX_METER_DENOMINATOR
}

/// The next bar line, treating a meter mark as a fresh bar even when it was
/// authored off the previous signature's grid.
fn next_bar_tick(song: &Song, tick: usize) -> usize {
    let origin = meter_origin(song, tick);
    let length = bar_ticks(song, tick).max(1);
    let nominal =
        origin.saturating_add((tick.saturating_sub(origin) / length + 1).saturating_mul(length));
    song.meter
        .iter()
        .filter(|mark| mark.tick > tick && usable_meter_mark(mark))
        .min_by_key(|mark| mark.tick)
        .map_or(nominal, |mark| nominal.min(mark.tick))
}

fn next_grid_tick(song: &Song, tick: usize, beat_grid: bool) -> usize {
    if !beat_grid {
        return next_bar_tick(song, tick);
    }
    let origin = meter_origin(song, tick);
    let length = beat_ticks(song, tick).max(1);
    let nominal =
        origin.saturating_add((tick.saturating_sub(origin) / length + 1).saturating_mul(length));
    song.meter
        .iter()
        .filter(|mark| mark.tick > tick && usable_meter_mark(mark))
        .min_by_key(|mark| mark.tick)
        .map_or(nominal, |mark| nominal.min(mark.tick))
}

fn previous_grid_tick(song: &Song, tick: usize, beat_grid: bool) -> usize {
    let before = tick.saturating_sub(1);
    let origin = meter_origin(song, before);
    let length = if beat_grid {
        beat_ticks(song, before)
    } else {
        bar_ticks(song, before)
    }
    .max(1);
    origin + before.saturating_sub(origin) / length * length
}

impl Arrangement {
    pub fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    pub fn end_selection_gesture(&mut self) {
        self.selection_anchor = None;
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.selection_anchor = None;
    }

    pub fn cell_selected(&self, track: usize, start: usize, end: usize) -> bool {
        self.selected
            .range((track, start)..(track, end))
            .next()
            .is_some()
    }

    pub fn toggle_selection(&mut self, song: &Song) {
        let end = self.tick.saturating_add(self.grid_ticks(song));
        let all = (self.tick..end).all(|tick| self.selected.contains(&(self.track, tick)));
        for tick in self.tick..end {
            if all {
                self.selected.remove(&(self.track, tick));
            } else {
                self.selected.insert((self.track, tick));
            }
        }
        self.expand_touching_blocks(song);
        self.selection_anchor = Some((self.track, self.tick));
    }

    /// Move the cursor and add the anchor-to-cursor rectangle. Time is
    /// stored as ticks, so later zoom changes redraw rather than reinterpret
    /// the selection.
    pub fn extend_selection(&mut self, song: &Song, step: Step) -> bool {
        let anchor = *self.selection_anchor.get_or_insert((self.track, self.tick));
        if !self.step(song, step) {
            return false;
        }
        let first_track = anchor.0.min(self.track);
        let last_track = anchor.0.max(self.track);
        let first_tick = anchor.1.min(self.tick);
        let last_tick = anchor
            .1
            .max(self.tick)
            .saturating_add(self.grid_ticks(song));
        for track in first_track..=last_track {
            for tick in first_tick..last_tick {
                self.selected.insert((track, tick));
            }
        }
        self.expand_touching_blocks(song);
        self.selection_anchor = Some(anchor);
        true
    }

    /// Arrangement cells select content by intersection: touching any
    /// part of a block adds that whole block to the exact tick mask.
    fn expand_touching_blocks(&mut self, song: &Song) {
        let mut whole = Vec::new();
        for (track, lane) in song.tracks.iter().enumerate() {
            for block in lane.blocks_in_time_order() {
                if self.cell_selected(track, block.start_tick(), block.end_tick()) {
                    whole.push((track, block.start_tick(), block.end_tick()));
                }
            }
        }
        for (track, start, end) in whole {
            for tick in start..end {
                self.selected.insert((track, tick));
            }
        }
    }

    pub fn selected_track_spans(&self) -> Vec<(usize, usize, usize)> {
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for &(track, tick) in &self.selected {
            match spans.last_mut() {
                Some((last_track, _, end)) if *last_track == track && tick <= *end => {
                    *end = (*end).max(tick.saturating_add(1));
                }
                _ => spans.push((track, tick, tick.saturating_add(1))),
            }
        }
        spans
    }

    pub fn block_selected(&self, track: usize, start: usize, end: usize) -> bool {
        self.cell_selected(track, start, end)
    }

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
            beat_ticks(song, self.tick)
        } else {
            next_bar_tick(song, self.tick)
                .saturating_sub(self.tick)
                .max(1)
        }
    }

    /// The ticks the time area spans.
    pub fn view_len(&self, song: &Song) -> usize {
        let mut end = self.view_start;
        for _ in 0..self.bars_across() {
            end = next_bar_tick(song, end);
        }
        end.saturating_sub(self.view_start).max(1)
    }

    /// `tick` brought down onto the grid.
    pub fn snap(&self, song: &Song, tick: usize) -> usize {
        let origin = meter_origin(song, tick);
        let grid = if self.beat_grid() {
            beat_ticks(song, tick)
        } else {
            bar_ticks(song, tick)
        }
        .max(1);
        origin + tick.saturating_sub(origin) / grid * grid
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
                self.tick = previous_grid_tick(song, self.tick, self.beat_grid());
                self.fit(song);
                true
            }
            Step::Right => {
                self.tick = next_grid_tick(song, self.tick, self.beat_grid());
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

    /// The edges on the cursor's track: the song's start, every block's
    /// two ends, the locators, and the brace's ends.
    fn edges(&self, song: &Song) -> Vec<usize> {
        let mut edges = vec![0];
        edges.extend(song.locators.iter().map(|locator| locator.tick));
        if let Some((start, end)) = song.loop_brace {
            edges.push(start);
            edges.push(end);
        }
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

impl Stage {
    /// Whether the song view holds the keys.
    pub(super) fn in_song(&self) -> bool {
        self.scope_context() == keymap::ScopeContext::Song
    }

    /// The cursor's track, when it exists.
    pub(super) fn song_track(&self) -> Option<usize> {
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
            SongIntent::BraceStart => self.brace_start(),
            SongIntent::BraceEnd => self.brace_end(),
            SongIntent::ToggleLoop => self.toggle_loop(),
            SongIntent::Marker => self.toggle_marker(),
            SongIntent::Export => self.request_export(),
            SongIntent::Resize => {
                let length = if let Some((_, _, region)) = self.selected_arrangement_region() {
                    region.width_ticks
                } else {
                    self.song_block()
                        .ok_or(RefusalReason::Empty)?
                        .1
                        .length_ticks
                };
                self.arrangement.hold = Some(Hold::Resize);
                self.notice = Some(format!(
                    "{} · {}",
                    Hold::Resize.word(),
                    crate::ui::sequencer::grid_resolution::bars_label(length)
                ));
                Ok(())
            }
            SongIntent::Stretch(step) => {
                let bar = bar_ticks(&self.song, self.arrangement.tick);
                if self.arrangement.has_selection() {
                    self.resize_arrangement_selection(step, bar)
                } else {
                    self.resize_block(step, bar)
                }
            }
            SongIntent::Duplicate => {
                if let Some((track, tick, region)) = self.selected_arrangement_region() {
                    self.write_arrangement_region(
                        &region,
                        track,
                        tick.saturating_add(region.width_ticks),
                    )
                } else {
                    let (track, block) = self.song_block().ok_or(RefusalReason::Empty)?;
                    let start = block.start_tick.saturating_add(block.length_ticks);
                    self.lay_block(track, block.pattern_id, start, block.length_ticks)?;
                    self.arrangement.tick = start;
                    self.arrangement.fit(&self.song);
                    Ok(())
                }
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

    fn selected_arrangement_region(&self) -> Option<(usize, usize, ArrangementClipboard)> {
        let min_track = self
            .arrangement
            .selected
            .iter()
            .map(|(track, _)| *track)
            .min()?;
        let max_track = self
            .arrangement
            .selected
            .iter()
            .map(|(track, _)| *track)
            .max()?;
        let min_tick = self
            .arrangement
            .selected
            .iter()
            .map(|(_, tick)| *tick)
            .min()?;
        let max_tick = self
            .arrangement
            .selected
            .iter()
            .map(|(_, tick)| *tick)
            .max()?;
        let mask = self
            .arrangement
            .selected
            .iter()
            .map(|(track, tick)| (track - min_track, tick - min_tick))
            .collect();
        let mut patterns = Vec::new();
        let mut audio = Vec::new();
        for (track, lane) in self.song.tracks.iter().enumerate() {
            for block in &lane.blocks {
                let end = block.start_tick.saturating_add(block.length_ticks);
                if self
                    .arrangement
                    .block_selected(track, block.start_tick, end)
                {
                    patterns.push((
                        track - min_track,
                        block.start_tick - min_tick,
                        block.clone(),
                    ));
                }
            }
            for block in &lane.audio_blocks {
                if self
                    .arrangement
                    .block_selected(track, block.start_tick, block.end_tick())
                {
                    audio.push((
                        track - min_track,
                        block.start_tick - min_tick,
                        block.clone(),
                    ));
                }
            }
        }
        Some((
            min_track,
            min_tick,
            ArrangementClipboard {
                width_ticks: max_tick - min_tick + 1,
                height: max_track - min_track + 1,
                mask,
                patterns,
                audio,
            },
        ))
    }

    fn remove_arrangement_region_content(&mut self, region: &ArrangementClipboard) {
        let pattern_ids: HashSet<_> = region
            .patterns
            .iter()
            .map(|(_, _, block)| block.id)
            .collect();
        let audio_ids: HashSet<_> = region.audio.iter().map(|(_, _, block)| block.id).collect();
        for lane in &mut self.song.tracks {
            lane.blocks.retain(|block| !pattern_ids.contains(&block.id));
            lane.audio_blocks
                .retain(|block| !audio_ids.contains(&block.id));
        }
    }

    /// Replace every destination touched by the sparse mask, then land
    /// copies of its blocks with fresh ids. A selected empty cell therefore
    /// remains an actual hole at the destination.
    fn write_arrangement_region(
        &mut self,
        region: &ArrangementClipboard,
        origin_track: usize,
        origin_tick: usize,
    ) -> Result<(), RefusalReason> {
        if origin_track.saturating_add(region.height) > self.song.tracks.len() {
            return Err(RefusalReason::Edge(Step::Down));
        }
        let destination: BTreeSet<_> = region
            .mask
            .iter()
            .map(|(track, tick)| (origin_track + track, origin_tick + tick))
            .collect();
        for (track, lane) in self.song.tracks.iter_mut().enumerate() {
            lane.blocks.retain(|block| {
                let end = block.start_tick.saturating_add(block.length_ticks);
                !destination
                    .range((track, block.start_tick)..(track, end))
                    .next()
                    .is_some()
            });
            lane.audio_blocks.retain(|block| {
                !destination
                    .range((track, block.start_tick)..(track, block.end_tick()))
                    .next()
                    .is_some()
            });
        }
        for (row, offset, kept) in &region.patterns {
            let mut block = kept.clone();
            block.id = self.song.mint_block_id();
            block.start_tick = origin_tick.saturating_add(*offset);
            self.song.tracks[origin_track + row].blocks.push(block);
        }
        for (row, offset, kept) in &region.audio {
            let mut block = kept.clone();
            block.id = self.song.mint_block_id();
            block.start_tick = origin_tick.saturating_add(*offset);
            self.song.tracks[origin_track + row]
                .audio_blocks
                .push(block);
        }
        for lane in &mut self.song.tracks {
            lane.blocks.sort_by_key(|block| block.start_tick);
            lane.audio_blocks.sort_by_key(|block| block.start_tick);
        }
        self.touched();
        Ok(())
    }

    fn shift_arrangement_selection(&mut self, track_delta: isize, tick_delta: isize) {
        self.arrangement.selected = self
            .arrangement
            .selected
            .iter()
            .map(|(track, tick)| {
                (
                    (*track as isize + track_delta) as usize,
                    (*tick as isize + tick_delta) as usize,
                )
            })
            .collect();
        self.arrangement.selection_anchor =
            self.arrangement.selection_anchor.map(|(track, tick)| {
                (
                    (track as isize + track_delta) as usize,
                    (tick as isize + tick_delta) as usize,
                )
            });
    }

    fn nudge_arrangement_selection(&mut self, step: Step, by: usize) -> Result<(), RefusalReason> {
        let (track, tick, region) = self
            .selected_arrangement_region()
            .ok_or(RefusalReason::Empty)?;
        let (to_track, to_tick, track_delta, tick_delta) = match step {
            Step::Left => (
                track,
                tick.checked_sub(by).ok_or(RefusalReason::Edge(step))?,
                0,
                -(by as isize),
            ),
            Step::Right => (track, tick.saturating_add(by), 0, by as isize),
            Step::Up => (
                track.checked_sub(1).ok_or(RefusalReason::Edge(step))?,
                tick,
                -1,
                0,
            ),
            Step::Down => {
                if track + region.height >= self.song.tracks.len() {
                    return Err(RefusalReason::Edge(step));
                }
                (track + 1, tick, 1, 0)
            }
        };
        self.remove_arrangement_region_content(&region);
        self.write_arrangement_region(&region, to_track, to_tick)?;
        self.shift_arrangement_selection(track_delta, tick_delta);
        self.arrangement.track = (self.arrangement.track as isize + track_delta) as usize;
        self.arrangement.tick = (self.arrangement.tick as isize + tick_delta) as usize;
        self.arrangement.fit(&self.song);
        Ok(())
    }

    /// Resize a multi-block region as one object: its left edge stays
    /// fixed and every start/end is scaled by the same ratio. This keeps
    /// spacing proportional instead of adding the same duration to every
    /// block independently.
    fn resize_arrangement_selection(&mut self, step: Step, by: usize) -> Result<(), RefusalReason> {
        let (track, tick, region) = self
            .selected_arrangement_region()
            .ok_or(RefusalReason::Empty)?;
        let old = region.width_ticks.max(1);
        let minimum = self.arrangement.grid_ticks(&self.song).min(old).max(1);
        let new = match step {
            Step::Left => old.saturating_sub(by).max(minimum),
            Step::Right => old.saturating_add(by),
            _ => return Err(RefusalReason::Unavailable),
        };
        if new == old {
            return Err(RefusalReason::Edge(step));
        }
        let scaled = |position: usize| -> usize {
            ((position as u128 * new as u128 + old as u128 / 2) / old as u128) as usize
        };
        let mut resized = region.clone();
        resized.width_ticks = new;
        resized.mask = region
            .mask
            .iter()
            .flat_map(|(row, at)| {
                let start = scaled(*at).min(new.saturating_sub(1));
                let end = scaled(at.saturating_add(1)).max(start + 1).min(new);
                (start..end).map(move |tick| (*row, tick))
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for (_, offset, block) in &mut resized.patterns {
            let end = offset.saturating_add(block.length_ticks);
            let start = scaled(*offset);
            let end = scaled(end).max(start + 1);
            *offset = start;
            block.length_ticks = end - start;
        }
        for (_, offset, block) in &mut resized.audio {
            let end = offset.saturating_add(block.length_ticks);
            let start = scaled(*offset);
            let end = scaled(end).max(start + 1);
            *offset = start;
            block.length_ticks = end - start;
        }
        self.remove_arrangement_region_content(&region);
        self.write_arrangement_region(&resized, track, tick)?;
        self.arrangement.selected = resized
            .mask
            .iter()
            .map(|(row, at)| (track + row, tick + at))
            .collect();
        if self.arrangement.tick >= tick {
            self.arrangement.tick = tick + scaled(self.arrangement.tick - tick);
            self.arrangement.fit(&self.song);
        }
        self.notice = Some(crate::ui::sequencer::grid_resolution::bars_label(new));
        Ok(())
    }

    /// A step in the song view: the held verb's motion, or the cursor's.
    pub(super) fn song_step(&mut self, step: Step) -> Result<(), RefusalReason> {
        match self.arrangement.hold {
            Some(Hold::Nudge) if self.arrangement.has_selection() => {
                let grid = self.arrangement.grid_ticks(&self.song);
                self.nudge_arrangement_selection(step, grid)
            }
            Some(Hold::Nudge) if matches!(step, Step::Left | Step::Right) => {
                let grid = self.arrangement.grid_ticks(&self.song);
                self.move_block(step, grid)
            }
            Some(Hold::Resize) if matches!(step, Step::Left | Step::Right) => {
                let grid = self.arrangement.grid_ticks(&self.song);
                if self.arrangement.has_selection() {
                    self.resize_arrangement_selection(step, grid)
                } else {
                    self.resize_block(step, grid)
                }
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
        if self.arrangement.has_selection() {
            let spans = self.arrangement.selected_track_spans();
            if spans.is_empty() {
                return Err(RefusalReason::Empty);
            }
            for (track, start, end) in spans {
                let pattern = self
                    .pattern_to_place(track)
                    .ok_or(RefusalReason::Unavailable)?;
                if let Some(lane) = self.song.tracks.get_mut(track) {
                    lane.blocks.retain(|block| {
                        block.start_tick.saturating_add(block.length_ticks) <= start
                            || block.start_tick >= end
                    });
                    lane.audio_blocks
                        .retain(|block| !block.intersects(start, end));
                }
                self.lay_block(track, pattern, start, end.saturating_sub(start))?;
            }
            return Ok(());
        }
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
        if self.arrangement.has_selection() {
            let spans = self.arrangement.selected_track_spans();
            let mut changed = false;
            for (track, start, end) in spans {
                if let Some(lane) = self.song.tracks.get_mut(track) {
                    let before = lane.blocks.len() + lane.audio_blocks.len();
                    lane.blocks.retain(|block| {
                        block.start_tick.saturating_add(block.length_ticks) <= start
                            || block.start_tick >= end
                    });
                    lane.audio_blocks
                        .retain(|block| !block.intersects(start, end));
                    changed |= before != lane.blocks.len() + lane.audio_blocks.len();
                }
            }
            if changed {
                self.touched();
                return Ok(());
            }
            return Err(RefusalReason::Empty);
        }
        if self.song_block().is_none()
            && let Some((track, block)) = self.song_audio_block()
        {
            self.song.tracks[track]
                .audio_blocks
                .retain(|candidate| candidate.id != block.id);
            self.notice = Some(format!("- {}", block.name));
            self.touched();
            return Ok(());
        }
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
        if let Some((_, _, region)) = self.selected_arrangement_region() {
            let count = region.patterns.len() + region.audio.len();
            self.remove_arrangement_region_content(&region);
            self.arrangement_clipboard = Some(region);
            self.block_clipboard = None;
            self.notice = Some(format!("yanked {count} blocks"));
            self.touched();
            return Ok(());
        }
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
        self.arrangement_clipboard = None;
        self.touched();
        Ok(())
    }

    /// Put the kept block down at the cursor, on the cursor's track.
    pub(super) fn song_put(&mut self) -> Result<(), RefusalReason> {
        if let Some(region) = self.arrangement_clipboard.clone() {
            let track = self.song_track().ok_or(RefusalReason::Unavailable)?;
            return self.write_arrangement_region(&region, track, self.arrangement.tick);
        }
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
}

#[cfg(test)]
pub(in crate::ui::stage) mod tests {
    use super::*;

    pub(in crate::ui::stage) fn song_with_tracks(n: usize) -> Song {
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

    #[test]
    fn denominator_and_meter_marks_define_the_grid() {
        let mut song = song_with_tracks(1);
        assert!(song.set_meter_mark(0, 7, 8));
        assert_eq!(bar_ticks(&song, 0), TICKS_PER_BEAT * 7 / 2);

        let mut arr = Arrangement::default();
        assert_eq!(arr.grid_ticks(&song), TICKS_PER_BEAT * 7 / 2);
        assert!(arr.zoom(&song, true));
        assert_eq!(arr.grid_ticks(&song), TICKS_PER_BEAT / 2);

        // A mark between old beats is itself the next grid boundary and a
        // fresh metric origin, rather than being stepped over.
        let mark = TICKS_PER_BEAT / 2 + 5;
        assert!(song.set_meter_mark(mark, 3, 4));
        arr.tick = TICKS_PER_BEAT / 2;
        assert!(arr.step(&song, Step::Right));
        assert_eq!(arr.tick, mark);
        assert!(arr.step(&song, Step::Right));
        assert_eq!(arr.tick, mark + TICKS_PER_BEAT);
        assert!(arr.step(&song, Step::Left));
        assert_eq!(arr.tick, mark);
    }

    #[test]
    fn hostile_meter_data_cannot_turn_one_selection_into_an_unbounded_walk() {
        let mut song = song_with_tracks(1);
        song.meter.push(crate::sequencing::MeterMark {
            tick: 0,
            numerator: u32::MAX,
            denominator: 1,
        });
        assert_eq!(bar_ticks(&song, 0), TICKS_PER_BEAT * 4);

        let mut arr = Arrangement::default();
        arr.toggle_selection(&song);
        assert_eq!(arr.selected.len(), TICKS_PER_BEAT * 4);
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
}

// ------------------------------------------------------------------ the
// brace, the locators, the takes, and the way out

/// A block being written while the session is recorded into the song:
/// the pattern that fired on a track, from when it fired until the
/// moment something else fires there or the transport stops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Take {
    pub track: usize,
    pub pattern: PatternId,
    pub start: usize,
    pub end: Option<usize>,
}

/// What the stage asks the host to render: a range of the song, to a
/// file. The host builds the arrangement's graph and bounces it on a
/// thread, reporting back through [`Stage::set_export_progress`] and
/// [`Stage::export_finished`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRequest {
    pub start_tick: usize,
    pub end_tick: usize,
    pub path: std::path::PathBuf,
    pub format: crate::ui::prefs::ExportFormat,
    /// `None` follows the running device's rate.
    pub rate_hz: Option<u32>,
    /// Silence rendered after the musical range so time-based effects can
    /// decay without changing where the requested range begins or ends.
    pub tail_seconds: u32,
}

/// An export under way, as the strip shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportState {
    pub path: std::path::PathBuf,
    pub progress: f32,
}

/// `YYYYMMDD-HHMMSS`, now, in UTC: a render's name should say when it
/// was made, and should sort by it.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = secs / 86_400;
    let rest = secs % 86_400;
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

impl Stage {
    /// The loop the transport should run while the SONG plays: the
    /// brace, when it is on and has a length.
    pub fn loop_region(&self) -> Option<(usize, usize)> {
        if !self.song_mode() || !self.song.loop_on {
            return None;
        }
        self.song.loop_brace.filter(|(start, end)| end > start)
    }

    /// Whether the arrangement is armed to record the session.
    pub fn arming(&self) -> bool {
        self.arming
    }

    /// Whether takes are being written right now.
    pub fn recording_song(&self) -> bool {
        self.arming && self.rolling() && !self.song_mode()
    }

    /// The export the stage wants rendered, once.
    pub fn take_export(&self) -> Option<ExportRequest> {
        self.export_request.clone()
    }

    /// The host took the request.
    pub fn export_taken(&mut self) {
        self.export_request = None;
    }

    /// Whether the performer abandoned the export in progress, once.
    pub fn export_abandoned(&mut self) -> bool {
        std::mem::take(&mut self.export_abandon)
    }

    pub fn set_export_progress(&mut self, progress: f32) {
        if let Some(export) = &mut self.export {
            export.progress = progress.clamp(0.0, 1.0);
        }
    }

    /// The render ended, one way or the other.
    pub fn export_finished(&mut self, result: Result<(), String>) {
        let Some(export) = self.export.take() else {
            return;
        };
        self.export_request = None;
        self.utility.export_finished(&export.path, &result);
        self.notice = Some(match result {
            Ok(()) => format!("exported → {}", export.path.display()),
            Err(error) => format!("export failed: {error}"),
        });
    }

    pub(super) fn export_state(&self) -> Option<&ExportState> {
        self.export.as_ref()
    }

    /// ^X: render the brace when it is on, else the whole song, to a
    /// wav under the song folder's renders. The host does the work.
    pub(super) fn request_export(&mut self) -> Result<(), RefusalReason> {
        if self.export.is_some() {
            self.notice = Some("EXPORT · already rendering".to_owned());
            return Err(RefusalReason::Unavailable);
        }
        let (start, end) = match (self.song.loop_on, self.song.loop_brace) {
            (true, Some((start, end))) if end > start => (start, end),
            _ => (0, self.song.end_tick()),
        };
        if end <= start {
            self.notice = Some("EXPORT · nothing to render".to_owned());
            return Err(RefusalReason::Empty);
        }
        let home = self.home.clone().unwrap_or_else(std::env::temp_dir);
        let name = self
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map_or_else(
                || "untitled".to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            );
        let path = home.join("renders").join(format!("{name}-{}.wav", stamp()));
        self.request_export_to(
            start,
            end,
            path,
            crate::ui::prefs::ExportFormat::Int24,
            None,
            0,
        )
    }

    /// Queue a fully specified offline render. The utility console owns the
    /// choices; this seam only validates and snapshots them for the host.
    pub(super) fn request_export_to(
        &mut self,
        start: usize,
        end: usize,
        path: std::path::PathBuf,
        format: crate::ui::prefs::ExportFormat,
        rate_hz: Option<u32>,
        tail_seconds: u32,
    ) -> Result<(), RefusalReason> {
        if self.export.is_some() {
            self.notice = Some("EXPORT · already rendering".to_owned());
            return Err(RefusalReason::Unavailable);
        }
        if end <= start {
            self.notice = Some("EXPORT · nothing to render".to_owned());
            return Err(RefusalReason::Empty);
        }
        self.export_request = Some(ExportRequest {
            start_tick: start,
            end_tick: end,
            path: path.clone(),
            format,
            rate_hz,
            tail_seconds,
        });
        self.export = Some(ExportState {
            path,
            progress: 0.0,
        });
        self.notice = Some(format!(
            "EXPORT · {}",
            crate::ui::sequencer::grid_resolution::bars_label(end - start)
        ));
        Ok(())
    }

    /// Escape while a render runs: abandon it. The host removes the
    /// part-written file.
    pub(super) fn abandon_export(&mut self) -> Result<(), RefusalReason> {
        if self.export.is_none() {
            return Err(RefusalReason::Unavailable);
        }
        self.export_abandon = true;
        self.notice = Some("EXPORT · abandoned".to_owned());
        Ok(())
    }

    /// The brace's start at the cursor. A brace that would be
    /// inside-out keeps a bar past the cursor as its end.
    pub(super) fn brace_start(&mut self) -> Result<(), RefusalReason> {
        let at = self.arrangement.tick;
        let bar = bar_ticks(&self.song, at);
        let end = match self.song.loop_brace {
            Some((_, end)) if end > at => end,
            _ => at + bar,
        };
        self.song.loop_brace = Some((at, end));
        self.song.loop_on = true;
        self.say_brace();
        self.touched();
        Ok(())
    }

    /// The brace's end at the cursor: the cursor's cell is the last one
    /// inside it. A brace with no start yet begins at the song's start.
    pub(super) fn brace_end(&mut self) -> Result<(), RefusalReason> {
        let grid = self.arrangement.grid_ticks(&self.song);
        let at = self.arrangement.tick + grid;
        let start = match self.song.loop_brace {
            Some((start, _)) if start < at => start,
            _ => 0,
        };
        self.song.loop_brace = Some((start, at));
        self.song.loop_on = true;
        self.say_brace();
        self.touched();
        Ok(())
    }

    /// L: the brace on or off, without losing where it is.
    pub(super) fn toggle_loop(&mut self) -> Result<(), RefusalReason> {
        if self.song.loop_brace.is_none() {
            self.notice = Some("LOOP · no brace yet · [ and ]".to_owned());
            return Err(RefusalReason::Empty);
        }
        self.song.loop_on = !self.song.loop_on;
        self.say_brace();
        self.touched();
        Ok(())
    }

    fn say_brace(&mut self) {
        let Some((start, end)) = self.song.loop_brace else {
            return;
        };
        let bar = bar_ticks(&self.song, start).max(1);
        self.notice = Some(format!(
            "LOOP {} · {} → {}",
            if self.song.loop_on { "ON" } else { "OFF" },
            start / bar + 1,
            end.div_ceil(bar) + 1
        ));
    }

    /// M: a locator at the cursor, or the one there taken away.
    pub(super) fn toggle_marker(&mut self) -> Result<(), RefusalReason> {
        let at = self.arrangement.tick;
        if let Some(index) = self
            .song
            .locators
            .iter()
            .position(|locator| locator.tick == at)
        {
            let gone = self.song.locators.remove(index);
            self.notice = Some(format!("- {}", gone.name));
        } else {
            let name = format!("M{}", self.song.locators.len() + 1);
            self.song.locators.push(crate::sequencing::Locator {
                tick: at,
                name: name.clone(),
            });
            self.song.locators.sort_by_key(|locator| locator.tick);
            self.notice = Some(format!("+ {name}"));
        }
        self.touched();
        Ok(())
    }

    /// ^Space: arm the arrangement to take down what the session plays.
    pub(super) fn toggle_arming(&mut self) -> Result<(), RefusalReason> {
        self.arming = !self.arming;
        if self.arming {
            self.notice = Some("SONG REC · armed · launches write blocks".to_owned());
            // Already rolling: what plays now begins its take now.
            if self.recording_song() {
                let tick = self.transport.tick();
                for track in 0..self.playing.len() {
                    if let Some(scene) = self.playing[track] {
                        self.open_take(track, scene, tick);
                    }
                }
            }
        } else {
            let tick = self.transport.tick();
            self.commit_takes(tick);
            if self.notice.is_none() {
                self.notice = Some("SONG REC · off".to_owned());
            }
        }
        Ok(())
    }

    fn open_take(&mut self, track: usize, scene: usize, tick: usize) {
        if let Some(Clip::Pattern(pattern)) = self.song.slot_clip(track, scene) {
            self.takes.push(Take {
                track,
                pattern,
                start: tick,
                end: None,
            });
        }
    }

    fn close_take(&mut self, track: usize, tick: usize) {
        for take in &mut self.takes {
            if take.track == track && take.end.is_none() {
                take.end = Some(tick);
            }
        }
    }

    /// Every take lands as a block, in one edit. A take is the
    /// performer's latest word on its span: the pattern blocks it lands
    /// on go, as they would under an arrangement recording. Recorded
    /// sound is not overwritten; a take over an audio block is counted
    /// and left out.
    fn commit_takes(&mut self, tick: usize) {
        let takes = std::mem::take(&mut self.takes);
        if takes.is_empty() {
            return;
        }
        let (mut laid, mut refused) = (0, 0);
        for take in takes {
            let end = take.end.unwrap_or(tick);
            if end <= take.start {
                continue;
            }
            if let Some(lane) = self.song.tracks.get_mut(take.track) {
                lane.blocks.retain(|block| {
                    !(block.start_tick < end
                        && take.start < block.start_tick.saturating_add(block.length_ticks))
                });
            }
            match self
                .song
                .place_block(take.track, take.pattern, take.start, end - take.start)
            {
                Ok(_) => laid += 1,
                Err(_) => refused += 1,
            }
        }
        if laid > 0 {
            self.touched();
        }
        self.notice = Some(match refused {
            0 => format!("recorded {laid} blocks"),
            _ => format!("recorded {laid} blocks · {refused} in the way"),
        });
    }

    /// After every intent: what the session started or stopped playing
    /// becomes takes while the arrangement is armed. Called with what
    /// was playing, and whether takes were being written, before.
    pub(super) fn record_takes(&mut self, before: &[Option<usize>], was_recording: bool) {
        if !self.arming {
            return;
        }
        let recording = self.recording_song();
        let tick = self.transport.tick();
        match (was_recording, recording) {
            (false, true) => {
                for track in 0..self.playing.len() {
                    if let Some(scene) = self.playing[track] {
                        self.open_take(track, scene, tick);
                    }
                }
            }
            (true, false) => {
                let tracks: Vec<usize> = self.takes.iter().map(|take| take.track).collect();
                for track in tracks {
                    self.close_take(track, tick);
                }
                self.commit_takes(tick);
            }
            (true, true) => {
                let count = before.len().max(self.playing.len());
                for track in 0..count {
                    let then = before.get(track).copied().flatten();
                    let now = self.playing.get(track).copied().flatten();
                    if then == now {
                        continue;
                    }
                    self.close_take(track, tick);
                    if let Some(scene) = now {
                        self.open_take(track, scene, tick);
                    }
                }
            }
            (false, false) => {}
        }
    }

    /// A sound from the browser onto an audio track in the song view:
    /// an audio block at the cursor, as long as the file is at the
    /// tempo there.
    pub(super) fn land_audio(
        &mut self,
        track: usize,
        path: std::path::PathBuf,
    ) -> Result<(), RefusalReason> {
        let unreadable = crate::sequencing::LandRefusal::Unreadable.sign().to_owned();
        let Ok(reader) = hound::WavReader::open(&path) else {
            self.notice = Some(unreadable);
            return Err(RefusalReason::Unavailable);
        };
        let frames = u64::from(reader.duration());
        let sample_rate = reader.spec().sample_rate;
        let source = crate::audio_source::AudioSource {
            path: path.clone(),
            sample_rate,
            source_offset: 0,
            source_frames: frames,
            gain: 1.0,
            looped: false,
            transpose: 0.0,
            detune: 0.0,
            transposed_from: None,
            applied_ratio: 1.0,
            file_frames: frames,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tempo = crate::tempo::TempoTable::build(&self.song, 48_000.0, self.bpm());
        let start = self.arrangement.tick;
        match self
            .song
            .place_audio(track, start, name.clone(), source, &tempo)
        {
            Ok(_) => {
                self.notice = Some(format!("+ {name}"));
                self.touched();
                Ok(())
            }
            Err(refusal) => {
                self.notice = Some(refusal.sign().to_owned());
                Err(RefusalReason::Unavailable)
            }
        }
    }

    /// The audio block under the song cursor, when no pattern block is.
    pub(super) fn song_audio_block(&self) -> Option<(usize, crate::sequencing::AudioBlock)> {
        let track = self.song_track()?;
        let tick = self.arrangement.tick;
        self.song.tracks[track]
            .audio_blocks
            .iter()
            .find(|block| block.start_tick <= tick && tick < block.end_tick())
            .map(|block| (track, block.clone()))
    }
}
