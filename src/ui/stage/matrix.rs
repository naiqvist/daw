//! The SESSION CLIP MATRIX: a window over the song view that shows the
//! session small — tracks across, scenes down, every clip by its tag —
//! and lays what the cursor picks at the arrangement's cursor.
//!
//! Enter on a cell lays that clip on ITS OWN track at the arrangement's
//! tick, one pattern long, and the arrangement's cursor RIDES to the
//! end of it, so Enter, Enter, Enter builds a phrase. Enter on a row's
//! head lays the whole scene, every clip on its track, and the cursor
//! rides by the longest. In the way is the lane's refusal; Shift+Enter
//! clears the span first. Escape puts the window away and leaves the
//! cursor where it rode to. Core: no egui here.

use super::{RefusalReason, Stage, Step};
use crate::sequencing::{Clip, PatternId};
use crate::ui::sequencer::pattern_length;

/// Where the matrix's cursor stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Matrix {
    /// The window is up over the song view.
    pub open: bool,
    /// The cursor's column: a track.
    pub track: usize,
    /// The cursor's row: a scene.
    pub scene: usize,
    /// The cursor is on the row's head: the whole scene is the subject.
    pub on_head: bool,
}

/// What one lay did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Laid {
    pub laid: usize,
    pub refused: usize,
    pub longest: usize,
}

impl Stage {
    /// P in the song view: the window up on the arrangement's track and
    /// the last scene used; P again puts it away. The deck's window
    /// closes first: one window at a time.
    pub(super) fn toggle_matrix(&mut self) -> Result<(), RefusalReason> {
        if self.matrix.open {
            self.matrix.open = false;
            return Ok(());
        }
        if !self.song_view || self.song.tracks.is_empty() {
            return Err(RefusalReason::Unavailable);
        }
        self.deck.open = false;
        self.matrix.track = self.arrangement.track.min(self.song.tracks.len() - 1);
        self.matrix.scene = self
            .matrix
            .scene
            .min(self.song.session.scenes.len().saturating_sub(1));
        self.matrix.open = true;
        self.notice = Some(format!(
            "matrix · Enter lays at bar {}",
            self.matrix_bar(self.arrangement.tick)
        ));
        Ok(())
    }

    pub(super) fn close_matrix(&mut self) {
        self.matrix.open = false;
    }

    /// The arrows: scenes down, tracks across; Left from the first
    /// track lands on the row's head, Right from the head goes back.
    pub(super) fn matrix_step(&mut self, step: Step) -> Result<(), RefusalReason> {
        let scenes = self.song.session.scenes.len();
        let tracks = self.song.tracks.len();
        match step {
            Step::Up => {
                if self.matrix.scene == 0 {
                    return Err(RefusalReason::Edge(step));
                }
                self.matrix.scene -= 1;
            }
            Step::Down => {
                if self.matrix.scene + 1 >= scenes {
                    return Err(RefusalReason::Edge(step));
                }
                self.matrix.scene += 1;
            }
            Step::Left => {
                if self.matrix.on_head {
                    return Err(RefusalReason::Edge(step));
                }
                if self.matrix.track == 0 {
                    self.matrix.on_head = true;
                } else {
                    self.matrix.track -= 1;
                }
            }
            Step::Right => {
                if self.matrix.on_head {
                    self.matrix.on_head = false;
                } else if self.matrix.track + 1 >= tracks {
                    return Err(RefusalReason::Edge(step));
                } else {
                    self.matrix.track += 1;
                }
            }
        }
        Ok(())
    }

    /// S: onto the row's head, or back to the cell.
    pub(super) fn matrix_head(&mut self) -> Result<(), RefusalReason> {
        self.matrix.on_head = !self.matrix.on_head;
        Ok(())
    }

    /// Enter: lay the cell's clip on its own track at the arrangement's
    /// tick, or the whole scene from a row head; then the cursor rides.
    /// `over` clears the span first.
    pub(super) fn matrix_lay(&mut self, over: bool) -> Result<(), RefusalReason> {
        let tick = self.arrangement.tick;
        if self.matrix.on_head {
            let scene = self.matrix.scene;
            let done = self.lay_scene(scene, tick, over);
            if done.laid == 0 {
                self.notice = Some(if done.refused > 0 {
                    format!("scene {:02} · {} in the way", scene + 1, done.refused)
                } else {
                    format!("scene {:02} · nothing to lay", scene + 1)
                });
                return Err(RefusalReason::Empty);
            }
            self.ride(tick + done.longest);
            self.notice = Some(if done.refused > 0 {
                format!(
                    "scene {:02} · {} laid · {} in the way",
                    scene + 1,
                    done.laid,
                    done.refused
                )
            } else {
                format!("scene {:02} · {} laid", scene + 1, done.laid)
            });
            return Ok(());
        }
        let track = self.matrix.track;
        let Some(Clip::Pattern(id)) = self.song.slot_clip(track, self.matrix.scene) else {
            self.notice = Some("empty slot".to_owned());
            return Err(RefusalReason::Empty);
        };
        let length = pattern_length(&self.song, id).max(1);
        if over {
            self.clear_span(track, tick, tick + length);
        }
        self.lay_block(track, id, tick, length)?;
        self.ride(tick + length);
        self.notice = Some(format!(
            "laid {} at bar {}",
            self.song.tag_of(id),
            self.matrix_bar(tick)
        ));
        Ok(())
    }

    /// Every clip of `scene` on its track at `tick`, each its own length.
    /// A track with something in the way is refused alone; `over`
    /// clears each span first. One edit; the caller settles it.
    fn lay_scene(&mut self, scene: usize, tick: usize, over: bool) -> Laid {
        let mut done = Laid::default();
        let clips: Vec<(usize, PatternId)> = (0..self.song.tracks.len())
            .filter_map(|track| match self.song.slot_clip(track, scene) {
                Some(Clip::Pattern(id)) => Some((track, id)),
                None => None,
            })
            .collect();
        for (track, id) in clips {
            let length = pattern_length(&self.song, id).max(1);
            if over {
                self.clear_span(track, tick, tick + length);
            }
            match self.song.place_block(track, id, tick, length) {
                Ok(_) => {
                    self.arrangement.last_placed.insert(track, id);
                    done.laid += 1;
                    done.longest = done.longest.max(length);
                }
                Err(_) => done.refused += 1,
            }
        }
        if done.laid > 0 {
            self.touched();
        }
        done
    }

    /// Lift every pattern block on `track` that touches `start..end`.
    /// The blocks go; their patterns stay — a lay over a block is not a
    /// deletion of what it played. Audio blocks stay, and the lay is
    /// refused over them as it would be anywhere.
    fn clear_span(&mut self, track: usize, start: usize, end: usize) {
        let Some(lane) = self.song.tracks.get(track) else {
            return;
        };
        let in_the_way: Vec<_> = lane
            .blocks
            .iter()
            .filter(|block| {
                block.start_tick < end
                    && block.start_tick.saturating_add(block.length_ticks) > start
            })
            .map(|block| block.id)
            .collect();
        for id in in_the_way {
            let _ = self.song.remove_block(track, id);
        }
    }

    /// The arrangement's cursor moves on to `tick` and the view follows.
    fn ride(&mut self, tick: usize) {
        self.arrangement.tick = tick;
        self.arrangement.fit(&self.song);
    }

    /// A tick as the bar it falls in, from one.
    fn matrix_bar(&self, tick: usize) -> usize {
        tick / super::arrangement::bar_ticks(&self.song, tick).max(1) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::{DEFAULT_PATTERN_TICKS, TrackKind};
    use crate::ui::stage::key::{Key, Mods};
    use crate::ui::stage::keymap::ScopeContext;
    use crate::ui::stage::{ApplyOutcome, StageIntent};

    /// Two tracks with a clip each in the first scene, the song view up,
    /// the cursor one pattern in (the default song's block sits at 0).
    fn into_matrix() -> Stage {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.song.add_track(TrackKind::Instrument);
        stage.song.fill_slot(0, 0).expect("a clip on track one");
        stage.song.fill_slot(1, 0).expect("a clip on track two");
        stage.settle();
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Tab),
            Some(ApplyOutcome::Changed)
        );
        stage.arrangement.tick = DEFAULT_PATTERN_TICKS;
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::P),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(stage.scope_context(), ScopeContext::Matrix);
        stage
    }

    fn blocks_on(stage: &Stage, track: usize) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> = stage.song.tracks[track]
            .blocks
            .iter()
            .map(|block| (block.start_tick, stage.song.tag_of(block.pattern_id)))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn p_opens_the_matrix_over_the_song_and_p_or_escape_puts_it_away() {
        let mut stage = into_matrix();
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::P),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(stage.scope_context(), ScopeContext::Song);
        let _ = stage.handle_key(Mods::NONE, Key::P);
        assert_eq!(stage.scope_context(), ScopeContext::Matrix);
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert_eq!(stage.scope_context(), ScopeContext::Song);
        // Not from the session.
        let _ = stage.handle_key(Mods::NONE, Key::Tab);
        assert_eq!(stage.scope_context(), ScopeContext::Root);
        assert_eq!(
            stage.apply(StageIntent::Matrix),
            ApplyOutcome::Refused(crate::ui::stage::Refusal {
                intent: StageIntent::Matrix,
                reason: RefusalReason::Unavailable
            })
        );
    }

    #[test]
    fn enter_lays_the_cell_on_its_own_track_and_the_cursor_rides() {
        let mut stage = into_matrix();
        // The cursor starts on the arrangement's track, scene 0.
        assert_eq!((stage.matrix.track, stage.matrix.scene), (0, 0));
        let _ = stage.handle_key(Mods::NONE, Key::ArrowRight);
        assert_eq!(stage.matrix.track, 1);
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(
            blocks_on(&stage, 1),
            vec![(DEFAULT_PATTERN_TICKS, "b0".to_owned())]
        );
        assert_eq!(stage.arrangement.tick, 2 * DEFAULT_PATTERN_TICKS);
        assert_eq!(
            stage.scope_context(),
            ScopeContext::Matrix,
            "the window went away"
        );
        // Enter again lays after it.
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(blocks_on(&stage, 1).len(), 2);
        assert_eq!(stage.arrangement.tick, 3 * DEFAULT_PATTERN_TICKS);
        // Track one's own clip lands on track one, not the cursor's.
        let _ = stage.handle_key(Mods::NONE, Key::ArrowLeft);
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(
            blocks_on(&stage, 0).last().map(|(at, _)| *at),
            Some(3 * DEFAULT_PATTERN_TICKS)
        );
        // The lays are undo steps.
        assert_eq!(
            stage.handle_key(Mods::COMMAND, Key::Z),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(blocks_on(&stage, 0).len(), 1);
    }

    #[test]
    fn in_the_way_refuses_and_shift_enter_lays_over() {
        let mut stage = into_matrix();
        // The default song's block sits at tick 0 on track one.
        stage.arrangement.tick = 0;
        let tick_before = stage.arrangement.tick;
        assert!(matches!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Refused(_))
        ));
        assert_eq!(
            stage.arrangement.tick, tick_before,
            "a refusal moved the cursor"
        );
        assert_eq!(blocks_on(&stage, 0).len(), 1);
        assert_eq!(
            stage.handle_key(Mods::SHIFT, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(blocks_on(&stage, 0), vec![(0, "a1".to_owned())]);
        assert_eq!(stage.arrangement.tick, DEFAULT_PATTERN_TICKS);
    }

    #[test]
    fn the_row_head_lays_the_whole_scene_and_rides_by_the_longest() {
        let mut stage = into_matrix();
        // Left from the first track lands on the head; S is the same door.
        let _ = stage.handle_key(Mods::NONE, Key::ArrowLeft);
        assert!(stage.matrix.on_head);
        let _ = stage.handle_key(Mods::NONE, Key::S);
        assert!(!stage.matrix.on_head);
        let _ = stage.handle_key(Mods::NONE, Key::S);
        // Track two's clip is twice as long: the ride is by it.
        let long = match stage.song.slot_clip(1, 0) {
            Some(Clip::Pattern(id)) => id,
            None => panic!("a clip"),
        };
        stage.song.pattern_mut(long).unwrap().length_ticks = 2 * DEFAULT_PATTERN_TICKS;
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(blocks_on(&stage, 0).len(), 2);
        assert_eq!(blocks_on(&stage, 1).len(), 1);
        assert_eq!(stage.arrangement.tick, 3 * DEFAULT_PATTERN_TICKS);
        assert_eq!(stage.notice.as_deref(), Some("scene 01 · 2 laid"));
        // One track in the way refuses that track only: a block on
        // track one four patterns in, nothing on track two there.
        let far = 4 * DEFAULT_PATTERN_TICKS;
        stage
            .song
            .place_block(0, long, far, DEFAULT_PATTERN_TICKS)
            .expect("a block in the way");
        stage.arrangement.tick = far;
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(
            stage.notice.as_deref(),
            Some("scene 01 · 1 laid · 1 in the way")
        );
    }

    #[test]
    fn the_matrix_walks_scenes_and_tracks_and_refuses_at_its_edges() {
        let mut stage = into_matrix();
        assert!(matches!(
            stage.handle_key(Mods::NONE, Key::ArrowUp),
            Some(ApplyOutcome::Refused(_))
        ));
        let _ = stage.handle_key(Mods::NONE, Key::ArrowDown);
        assert_eq!(stage.matrix.scene, 1);
        let _ = stage.handle_key(Mods::NONE, Key::ArrowRight);
        assert!(matches!(
            stage.handle_key(Mods::NONE, Key::ArrowRight),
            Some(ApplyOutcome::Refused(_))
        ));
        // An empty slot has nothing to lay.
        assert!(matches!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Refused(_))
        ));
        assert_eq!(stage.notice.as_deref(), Some("empty slot"));
    }
}
