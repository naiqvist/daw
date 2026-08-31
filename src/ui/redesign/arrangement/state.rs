use crate::sequencing::{BlockId, PatternId, Song, TICKS_PER_BEAT};

pub(super) const GRID_BEATS: usize = 64;
/// How far the song may reach, in beats — 64 bars. The view is a
/// GRID_BEATS-wide window onto it, moved in whole bars so the lane
/// gridlines never change phase.
pub(super) const WORLD_BEATS: usize = 256;
const BAR: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Locus {
    pub(super) track: usize,
    pub(super) beat: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Selection {
    pub(super) first_track: usize,
    pub(super) last_track: usize,
    pub(super) first_beat: usize,
    /// Exclusive, so a one-cell selection has a visible duration.
    pub(super) end_beat: usize,
}

impl Selection {
    pub(super) fn beat_count(self) -> usize {
        self.end_beat - self.first_beat
    }

    pub(super) fn start_tick(self) -> usize {
        self.first_beat * TICKS_PER_BEAT
    }

    pub(super) fn end_tick(self) -> usize {
        self.end_beat * TICKS_PER_BEAT
    }
}

#[derive(Default)]
pub(super) struct PaletteState {
    pub(super) open: bool,
    pub(super) query: String,
    pub(super) cursor: usize,
    pub(super) just_opened: bool,
}

/// A track rename in progress: TYPING mode scoped to one header.
pub(super) struct TrackRename {
    pub(super) track: usize,
    pub(super) text: String,
}

#[derive(Clone, Copy)]
pub(super) struct BlockDrag {
    pub(super) id: BlockId,
    pub(super) grab_beats: f32,
    pub(super) copy: bool,
    pub(super) fine: bool,
}

impl PaletteState {
    pub(super) fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = 0;
        self.just_opened = true;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = 0;
        self.just_opened = false;
    }
}

pub(super) struct ArrangementState {
    pub(super) cursor: Locus,
    anchor: Option<Locus>,
    pub(super) palette: PaletteState,
    pub(super) notice: Option<&'static str>,
    /// The last unsupported sentence, spoken in the status area until the
    /// next sentence gives it something newer to say.
    pub(super) refusal: Option<String>,
    pub(super) selected_block: Option<BlockId>,
    pub(super) pointer_anchor: Option<Locus>,
    pub(super) block_drag: Option<BlockDrag>,
    pub(super) rename: Option<TrackRename>,
    /// The view window's first beat — always a whole bar, so the lane
    /// grid keeps its phase as the window travels the world.
    pub(super) view_start: usize,
}

impl Default for ArrangementState {
    fn default() -> Self {
        Self {
            cursor: Locus { track: 0, beat: 0 },
            anchor: None,
            palette: PaletteState::default(),
            notice: None,
            refusal: None,
            selected_block: None,
            pointer_anchor: None,
            block_drag: None,
            rename: None,
            view_start: 0,
        }
    }
}

impl ArrangementState {
    pub(super) fn clamp(&mut self, song: &Song) {
        self.cursor.track = self.cursor.track.min(song.tracks.len().saturating_sub(1));
        self.cursor.beat = self.cursor.beat.min(WORLD_BEATS.saturating_sub(1));
        if let Some(anchor) = &mut self.anchor {
            anchor.track = anchor.track.min(song.tracks.len().saturating_sub(1));
            anchor.beat = anchor.beat.min(WORLD_BEATS.saturating_sub(1));
        }
    }

    pub(super) fn selection(&self) -> Selection {
        let anchor = self.anchor.unwrap_or(self.cursor);
        Selection {
            first_track: anchor.track.min(self.cursor.track),
            last_track: anchor.track.max(self.cursor.track),
            first_beat: anchor.beat.min(self.cursor.beat),
            end_beat: anchor.beat.max(self.cursor.beat) + 1,
        }
    }

    pub(super) fn selected_pattern(&self, song: &Song) -> Option<PatternId> {
        let tick = self.cursor.beat * TICKS_PER_BEAT;
        song.tracks
            .get(self.cursor.track)?
            .blocks
            .iter()
            .find(|block| {
                block.start_tick <= tick
                    && tick < block.start_tick.saturating_add(block.length_ticks)
            })
            .map(|block| block.pattern_id)
    }

    pub(super) fn block_at_cursor(&self, song: &Song) -> Option<BlockId> {
        let tick = self.cursor.beat * TICKS_PER_BEAT;
        song.tracks
            .get(self.cursor.track)?
            .blocks
            .iter()
            .find(|block| {
                block.start_tick <= tick
                    && tick < block.start_tick.saturating_add(block.length_ticks)
            })
            .map(|block| block.id)
    }

    pub(super) fn active_block(&self, song: &Song) -> Option<BlockId> {
        self.selected_block
            .filter(|id| song.pattern_block(*id).is_some())
            .or_else(|| self.block_at_cursor(song))
    }

    /// Keep the cursor inside the window, shifting in whole bars.
    fn follow_cursor(&mut self) {
        if self.cursor.beat < self.view_start {
            self.view_start = self.cursor.beat / BAR * BAR;
        } else if self.cursor.beat >= self.view_start + GRID_BEATS {
            self.view_start = (self.cursor.beat + 1 - GRID_BEATS).div_ceil(BAR) * BAR;
        }
        self.view_start = self.view_start.min(WORLD_BEATS - GRID_BEATS);
    }

    /// While playing, the window page-flips to keep the playhead on
    /// screen — Ableton's follow, at bar resolution.
    pub(super) fn follow_playhead(&mut self, playhead_beats: f64, playing: bool) {
        if !playing || playhead_beats < 0.0 {
            return;
        }
        let beat = playhead_beats as usize;
        if beat < self.view_start || beat >= self.view_start + GRID_BEATS {
            self.view_start = (beat / BAR * BAR).min(WORLD_BEATS - GRID_BEATS);
        }
    }

    pub(super) fn move_cursor(
        &mut self,
        song: &Song,
        track_delta: isize,
        beat_delta: isize,
        extend: bool,
    ) {
        if extend && self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        } else if !extend {
            self.anchor = None;
        }
        self.cursor.track = bounded_step(self.cursor.track, track_delta, song.tracks.len());
        self.cursor.beat = bounded_step(self.cursor.beat, beat_delta, WORLD_BEATS);
        self.follow_cursor();
        self.selected_block = None;
        self.notice = None;
        self.refusal = None;
    }

    pub(super) fn select_region(&mut self, track: usize, first_beat: usize, end_beat: usize) {
        let first = first_beat.min(WORLD_BEATS.saturating_sub(1));
        let last = end_beat
            .saturating_sub(1)
            .min(WORLD_BEATS.saturating_sub(1));
        self.anchor = Some(Locus { track, beat: first });
        self.cursor = Locus { track, beat: last };
        self.selected_block = None;
        self.notice = None;
        self.refusal = None;
    }

    pub(super) fn select_block(
        &mut self,
        id: BlockId,
        track: usize,
        first_beat: usize,
        end_beat: usize,
    ) {
        self.select_region(track, first_beat, end_beat);
        self.selected_block = Some(id);
    }

    pub(super) fn begin_marquee(&mut self, track: usize, beat: usize) {
        let locus = Locus { track, beat };
        self.pointer_anchor = Some(locus);
        self.anchor = Some(locus);
        self.cursor = locus;
        self.selected_block = None;
    }

    pub(super) fn drag_marquee(&mut self, track: usize, beat: usize) {
        if let Some(anchor) = self.pointer_anchor {
            self.anchor = Some(anchor);
            self.cursor = Locus {
                track,
                beat: beat.min(WORLD_BEATS - 1),
            };
            self.selected_block = None;
        }
    }

    pub(super) fn extend_to(&mut self, track: usize, beat: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = Locus {
            track,
            beat: beat.min(WORLD_BEATS - 1),
        };
        self.selected_block = None;
        self.notice = None;
        self.refusal = None;
    }

    pub(super) fn end_pointer_gesture(&mut self) {
        self.pointer_anchor = None;
        self.block_drag = None;
    }
}

fn bounded_step(index: usize, amount: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    index.saturating_add_signed(amount).min(count - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view is a bar-quantized window on a larger world: the cursor
    /// drags it right past beat 64, back left on return, and the window
    /// start never leaves a bar line — so the lane grid keeps its phase.
    #[test]
    fn the_window_follows_the_cursor_in_whole_bars() {
        let song = Song::default();
        let mut state = ArrangementState::default();

        state.move_cursor(&song, 0, 65, false);
        assert_eq!(state.cursor.beat, 65);
        assert_eq!(state.view_start, 4, "one bar in: 65 sits on the right edge");
        assert_eq!(state.view_start % 4, 0);

        state.move_cursor(&song, 0, -65, false);
        assert_eq!(state.cursor.beat, 0);
        assert_eq!(state.view_start, 0, "returning drags the window home");

        state.move_cursor(&song, 0, WORLD_BEATS as isize, false);
        assert_eq!(state.cursor.beat, WORLD_BEATS - 1);
        assert_eq!(
            state.view_start,
            WORLD_BEATS - GRID_BEATS,
            "the window stops at the world's edge"
        );
    }

    /// While playing, the window page-flips to keep the playhead
    /// visible; while stopped, it stays where the hands left it.
    #[test]
    fn the_window_follows_the_playhead_only_while_playing() {
        let mut state = ArrangementState::default();
        state.follow_playhead(70.0, false);
        assert_eq!(state.view_start, 0);
        state.follow_playhead(70.0, true);
        assert_eq!(state.view_start, 68);
        state.follow_playhead(2.0, true);
        assert_eq!(state.view_start, 0, "a loop wrap flips back");
    }

    #[test]
    fn default_locus_selects_the_default_pattern() {
        let song = Song::default();
        let state = ArrangementState::default();
        assert_eq!(state.selected_pattern(&song), Some(song.patterns[0].id));
    }

    #[test]
    fn navigation_remains_bounded_at_song_edges() {
        let song = Song::default();
        let mut state = ArrangementState::default();
        state.move_cursor(&song, -1, -1, false);
        assert_eq!(state.cursor, Locus { track: 0, beat: 0 });
        state.move_cursor(&song, 1, WORLD_BEATS as isize + 4, false);
        assert_eq!(state.cursor.beat, WORLD_BEATS - 1);
    }

    #[test]
    fn shift_navigation_builds_and_retracts_a_time_selection() {
        let song = Song::default();
        let mut state = ArrangementState::default();
        state.move_cursor(&song, 0, 3, true);
        assert_eq!(state.selection().beat_count(), 4);
        state.move_cursor(&song, 0, -1, true);
        assert_eq!(state.selection().beat_count(), 3);
        state.move_cursor(&song, 0, 1, false);
        assert_eq!(state.selection().beat_count(), 1);
    }
}
