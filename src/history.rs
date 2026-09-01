//! Snapshot undo/redo over a pure value.
//!
//! Lifted from the control plane's `SongHistory` (the `crate::intent`
//! move: a frame-independent part gets a neutral home before a second
//! frame grows its own copy). The semantics are unchanged and worth
//! stating, because every frame that binds this inherits them:
//!
//! - **Observe, don't command.** The history WATCHES the value at a
//!   settled moment (end of frame) and records one step per observed
//!   change. Edits stay plain mutations; nothing routes through a
//!   command object to become undoable.
//! - **A new edit is a new future**: the redo branch dies the moment a
//!   fresh change is observed. One timeline, no forks pretending
//!   otherwise.
//! - **The floor refuses quietly**: undo/redo at the end of history
//!   return `false`, and the caller reports the refusal — the same
//!   contract as every other refused intent.
//!
//! Snapshots, not inverse commands, on purpose: the Song is a small pure
//! value, frames already compare whole Songs to notice edits, and a
//! restored snapshot is audible for free (the projection sees the change
//! like any edit). If a future value is ever too big to clone per step,
//! that is the day to revisit — not before.

/// Snapshot history over any comparable, clonable value.
#[derive(Clone, Debug)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    present: T,
    cap: usize,
}

/// Enough for a long session; unbounded memory is not a feature.
pub const DEFAULT_CAP: usize = 512;

impl<T: Clone + PartialEq> History<T> {
    pub fn new(initial: T) -> Self {
        Self::with_cap(initial, DEFAULT_CAP)
    }

    pub fn with_cap(initial: T, cap: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            present: initial,
            cap: cap.max(1),
        }
    }

    /// Notice a change since the last settled moment: the departed state
    /// becomes an undo step and the redo branch dies.
    pub fn observe(&mut self, value: &T) {
        if *value == self.present {
            return;
        }
        self.undo
            .push(std::mem::replace(&mut self.present, value.clone()));
        self.redo.clear();
        if self.undo.len() > self.cap {
            self.undo.remove(0);
        }
    }

    /// Step back, writing the restored state into `value`. `false` means
    /// the floor: nothing left to undo, and the caller should say so.
    pub fn undo(&mut self, value: &mut T) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.present, previous.clone()));
        *value = previous;
        true
    }

    /// Step forward again. `false` means no future is recorded.
    pub fn redo(&mut self, value: &mut T) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push(std::mem::replace(&mut self.present, next.clone()));
        *value = next;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::{Note, Song};

    fn edited(song: &Song, step: usize) -> Song {
        let mut next = song.clone();
        let pattern_id = next.tracks[0].blocks[0].pattern_id;
        next.pattern_mut(pattern_id)
            .expect("default pattern")
            .set_primary(step, Note::new(60, 12, 100));
        next
    }

    /// Each observed change is one step; undo walks back through them
    /// and redo walks forward, restoring bit-identical Songs.
    #[test]
    fn undo_and_redo_walk_the_observed_states() {
        let base = Song::default();
        let mut history = History::new(base.clone());
        let first = edited(&base, 0);
        let second = edited(&first, 4);

        let mut song = first.clone();
        history.observe(&song);
        song = second.clone();
        history.observe(&song);

        assert!(history.undo(&mut song));
        assert_eq!(song, first);
        assert!(history.undo(&mut song));
        assert_eq!(song, base);
        assert!(!history.undo(&mut song), "the floor refuses quietly");

        assert!(history.redo(&mut song));
        assert_eq!(song, first);
        assert!(history.redo(&mut song));
        assert_eq!(song, second);
        assert!(!history.redo(&mut song));
    }

    /// A new edit after an undo abandons the redo branch: one timeline,
    /// no forks pretending otherwise.
    #[test]
    fn a_new_edit_kills_the_redo_branch() {
        let base = Song::default();
        let mut history = History::new(base.clone());
        let mut song = edited(&base, 0);
        history.observe(&song);
        history.undo(&mut song);

        song = edited(&base, 8);
        history.observe(&song);
        assert!(!history.redo(&mut song), "the old future is gone");
        assert!(history.undo(&mut song));
        assert_eq!(song, base);
    }

    /// Observing an unchanged song records nothing.
    #[test]
    fn no_change_is_no_step() {
        let base = Song::default();
        let mut history = History::new(base.clone());
        let mut song = base.clone();
        history.observe(&song);
        assert!(!history.undo(&mut song));
    }

    /// A track mute — the stage's first planned verb — is one undoable
    /// song edit.
    #[test]
    fn a_track_mute_is_one_undoable_song_edit() {
        let base = Song::default();
        let mut history = History::new(base.clone());
        let mut song = base.clone();
        song.tracks[0].muted = true;
        history.observe(&song);

        assert!(history.undo(&mut song));
        assert_eq!(song, base);
        assert!(!song.tracks[0].muted);
    }

    /// The cap drops the OLDEST step, never a recent one, and never the
    /// ability to keep editing.
    #[test]
    fn the_cap_forgets_the_oldest_step_first() {
        let mut history = History::with_cap(0u32, 2);
        let mut value = 0u32;
        for next in 1..=3u32 {
            value = next;
            history.observe(&value);
        }

        assert!(history.undo(&mut value));
        assert_eq!(value, 2);
        assert!(history.undo(&mut value));
        assert_eq!(value, 1);
        assert!(!history.undo(&mut value), "0 fell off the capped floor");
    }

    /// The availability queries agree with what undo/redo will answer —
    /// they are what a display draws, so they may not lie.
    #[test]
    fn availability_matches_behaviour() {
        let mut history = History::new(0u32);
        let mut value = 1u32;
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.observe(&value);
        assert!(history.can_undo());
        assert!(!history.can_redo());

        assert!(history.undo(&mut value));
        assert!(!history.can_undo());
        assert!(history.can_redo());
    }
}
