//! The application's non-visual heartbeat.
//!
//! This green-zone control plane survives every UI because it drains engine
//! and worker results, advances transport mirrors, reconciles canonical state,
//! and schedules continued work without depending on what happens to be drawn.

use super::*;

impl App {
    /// Everything that must keep moving while the visual layer is rebuilt.
    ///
    /// This is deliberately separate from drawing: the audio callback owns
    /// realtime work, while this green-zone tick drains its telemetry,
    /// services workers, reconciles the graph and advances the transport
    /// mirror. A replacement UI can change freely without disconnecting any
    /// of those paths.
    pub(super) fn update_control_plane(&mut self, ctx: &egui::Context) {
        self.pump_engine(ctx);
        if self.engine.is_none() {
            self.advance_meters(&[], ctx.input(|i| i.stable_dt));
            if self.master_meter.moving()
                || self.meters.iter().any(device::meter::Ballistics::moving)
            {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        if let Some(snapshot) = self.library_service.newest_snapshot() {
            self.library_snapshot = snapshot;
            self.library_scanning = false;
        }
        while let Some(result) = self.wav_import_service.try_result() {
            self.wav_import_pending = self.wav_import_pending.saturating_sub(1);
            match result {
                Ok(imported) if self.pending_reversals.remove(&imported.original_path) => {
                    self.arrangement.force_recompile = true;
                }
                Ok(imported) => self.finish_sample_placement(imported),
                Err(error) => self.notice = Some(error.to_string()),
            }
        }
        if self.wav_import_pending == 0 {
            self.pending_drop_spots.clear();
        }
        self.pump_waveforms();
        self.pump_modulators(ctx.input(|i| i.stable_dt));
        self.poll_bounce_in_place();
        self.poll_export();

        // Keep the transport usable during the blank-canvas phase. These
        // are control gestures, not presentation, and use the same action
        // path as the legacy UI.
        let song_world = self.center_song && !ctx.egui_wants_keyboard_input();
        let mut play_song_selection = false;
        let mut actions = Vec::new();
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Space) {
                // In the SONG world the selection that plays is the
                // song's, not the legacy arrangement's.
                if song_world {
                    play_song_selection = true;
                } else {
                    actions.push(UiAction::PlaySelection);
                }
            }
            if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Space) {
                actions.push(UiAction::ContinuePlay);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Space) {
                actions.push(UiAction::TogglePlay);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                actions.push(UiAction::Return);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::O) {
                actions.push(UiAction::ToggleMetronome);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::F9) {
                actions.push(UiAction::ToggleRecord);
            }
        });
        // F10 flips the frame's center between the legacy timeline and
        // the SONG arrangement — the center's Ctrl+Down, one key, two
        // worlds at one address (C1, song-bridge brief).
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F10)) {
            self.center_song = !self.center_song;
        }
        self.drive_song_history(ctx);
        // The SONG world's transport sentences, all wearing the legacy
        // bindings' muscle memory. Play-the-selection mirrors the legacy
        // fence exactly: seek to the start, stop at the end.
        if play_song_selection {
            let (from, to) = self.redesign.song_selection_beats(&self.song);
            self.arrangement.pending_seek = Some(from);
            self.transport.play_until = Some(to);
            self.transport.playing = true;
        }
        // Ctrl+T adds an instrument track, the cursor landing on it.
        if song_world
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::T))
        {
            self.redesign.song_add_track(&mut self.song);
        }
        // Ctrl+B taps the tempo: press it on the beat a few times and
        // the bpm follows. A gap of two seconds starts a fresh count.
        if song_world
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::B))
        {
            let now = std::time::Instant::now();
            if self
                .tap_times
                .last()
                .is_some_and(|last| now.duration_since(*last).as_secs_f64() > 2.0)
            {
                self.tap_times.clear();
            }
            self.tap_times.push(now);
            if self.tap_times.len() > 5 {
                self.tap_times.remove(0);
            }
            if self.tap_times.len() >= 2 {
                let span = self.tap_times[self.tap_times.len() - 1]
                    .duration_since(self.tap_times[0])
                    .as_secs_f64();
                let interval = span / (self.tap_times.len() - 1) as f64;
                if interval > 0.0 {
                    actions.push(UiAction::SetTempo(60.0 / interval));
                }
            }
        }
        // Ctrl+L loops the selection in the SONG world — the legacy
        // binding's muscle memory, aimed at the new model. Pressed again
        // on the same range, it lets the loop go.
        if self.center_song
            && !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::L))
        {
            let range = self.redesign.song_selection_beats(&self.song);
            if self.transport.loop_on && self.arrangement.loop_range == Some(range) {
                self.transport.loop_on = false;
            } else {
                self.arrangement.loop_range = Some(range);
                self.transport.loop_on = true;
            }
        }
        self.route_transport(&actions);
        perform(&actions, &mut self.transport, &mut self.arrangement);

        self.drive_recording();
        self.sync_engine();
        let settled = !ctx.input(|i| i.pointer.any_down())
            && !ctx.egui_is_using_pointer()
            && self.arrangement.ghost.is_none()
            && self.arrangement.rename.is_none()
            && self.arrangement.track_rename.is_none()
            && self.arrangement.scene_rename.is_none()
            && self.arrangement.locator_rename.is_none()
            && self.arrangement.slot_drag.is_none()
            && self.drag_import.is_none()
            && self.wav_import_pending == 0
            && !ctx.egui_wants_keyboard_input();
        self.history.sync(&self.arrangement, settled);

        if self.engine.is_none() && self.transport.playing {
            self.transport.position += f64::from(ctx.input(|i| i.stable_dt));
            if self.transport.loop_on
                && let Some((from, to)) = self.arrangement.loop_range
            {
                self.transport.position =
                    wrap_loop(self.transport.position, self.transport.bpm, from, to);
            }
            ctx.request_repaint();
        }

        if self.library_scanning || self.wav_import_pending > 0 || !self.waveform_pending.is_empty()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

// --- undo for the SONG world ---
//
// The legacy history tracks only the legacy arrangement; edits to the
// canonical `Song` were irreversible until this. Snapshots are cheap —
// the Song is a small pure value, and the projection already compares
// whole Songs every frame — and restoring one is AUDIBLE for free: the
// copyist sees the change, reprojects, and the compiler follows.

/// Snapshot history for the canonical song. One step per frame that
/// changed it: sentence-granular in practice, since sentences are how
/// frames change it.
pub(super) struct SongHistory {
    undo: Vec<daw::sequencing::Song>,
    redo: Vec<daw::sequencing::Song>,
    present: daw::sequencing::Song,
}

/// Enough for a long session; the Song is small, but unbounded memory
/// is not a feature.
const SONG_HISTORY_CAP: usize = 512;

impl SongHistory {
    pub(super) fn new(initial: daw::sequencing::Song) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            present: initial,
        }
    }

    /// Notice a change since last frame: the departed state becomes an
    /// undo step and the redo branch dies (a new edit is a new future).
    fn observe(&mut self, song: &daw::sequencing::Song) {
        if *song == self.present {
            return;
        }
        self.undo
            .push(std::mem::replace(&mut self.present, song.clone()));
        self.redo.clear();
        if self.undo.len() > SONG_HISTORY_CAP {
            self.undo.remove(0);
        }
    }

    fn undo(&mut self, song: &mut daw::sequencing::Song) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.present, previous.clone()));
        *song = previous;
        true
    }

    fn redo(&mut self, song: &mut daw::sequencing::Song) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push(std::mem::replace(&mut self.present, next.clone()));
        *song = next;
        true
    }
}

impl App {
    /// Song undo, run every frame from the control plane: observe first
    /// (so this frame sees last frame's edits as one step), then answer
    /// Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y — but only while the center shows
    /// the SONG world; the legacy center keeps its own history, and a
    /// focused text field owns the keyboard outright.
    pub(super) fn drive_song_history(&mut self, ctx: &egui::Context) {
        self.song_history.observe(&self.song);
        if !self.center_song || ctx.egui_wants_keyboard_input() {
            return;
        }
        // Redo is matched before undo, or Ctrl+Shift+Z would swallow as
        // undo — same ordering the legacy bindings use.
        let redo = ctx.input_mut(|input| {
            input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        });
        let undo = !redo
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        if redo {
            self.song_history.redo(&mut self.song);
        } else if undo {
            self.song_history.undo(&mut self.song);
        }
        // No projection call here: project_song runs later in the frame
        // and notices the restored Song the same way it notices an edit.
    }
}

#[cfg(test)]
mod song_history_tests {
    use super::SongHistory;
    use daw::sequencing::{Note, Song};

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
        let mut history = SongHistory::new(base.clone());
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
        let mut history = SongHistory::new(base.clone());
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
        let mut history = SongHistory::new(base.clone());
        let mut song = base.clone();
        history.observe(&song);
        assert!(!history.undo(&mut song));
    }

    #[test]
    fn a_track_mute_is_one_undoable_song_edit() {
        let base = Song::default();
        let mut history = SongHistory::new(base.clone());
        let mut song = base.clone();
        song.tracks[0].muted = true;
        history.observe(&song);

        assert!(history.undo(&mut song));
        assert_eq!(song, base);
        assert!(!song.tracks[0].muted);
    }
}
