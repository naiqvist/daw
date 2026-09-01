//! Keyboard-first song arrangement.
//!
//! This view owns only navigation state. Tracks, blocks and patterns stay in
//! the application-owned [`Song`], shared with the wrapped sequence editor.

pub mod automation;
mod edit;
mod palette;
mod state;
mod view;

use crate::sequencing::{PatternId, Song};
use crate::ui::redesign::grammar::Voice;
use eframe::egui;
use state::ArrangementState;

pub struct View<'a> {
    pub song: &'a mut Song,
    pub playhead_beats: f64,
    pub playing: bool,
    /// Every parameter the automation lane may be aimed at on the
    /// selected track. Built by the app, because which parameters exist
    /// depends on the track's device chain — the lane never reaches into
    /// the song to find out.
    pub automation_targets: &'a [automation::TargetOption],
}

#[derive(Default)]
pub struct Outcome {
    pub(crate) claim_focus: bool,
    pub(crate) open_pattern: bool,
}

#[derive(Default)]
pub struct ArrangementPanel {
    state: ArrangementState,
}

impl ArrangementPanel {
    pub fn selected_pattern(&mut self, song: &Song) -> Option<PatternId> {
        self.state.clamp(song);
        self.state.selected_pattern(song)
    }

    /// The song track under the arrangement cursor — the per-track link
    /// of the harmonic scope chain (entry authority, lens).
    pub fn selected_track(&mut self, song: &Song) -> Option<usize> {
        self.state.clamp(song);
        (!song.tracks.is_empty()).then_some(self.state.cursor.track)
    }

    /// Append a fresh instrument track and land the cursor on it —
    /// the frame-level Ctrl+T, sharing the palette command's one mint.
    pub fn add_track(&mut self, song: &mut Song) {
        edit::add_track(song);
        self.state.cursor.track = song.tracks.len().saturating_sub(1);
    }

    /// The selection's beat span — a one-cell selection is one beat, so
    /// there is always a range to loop.
    pub fn selection_range_beats(&mut self, song: &Song) -> (f32, f32) {
        self.state.clamp(song);
        let selection = self.state.selection();
        (selection.first_beat as f32, selection.end_beat as f32)
    }

    #[allow(private_interfaces)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        voice: &mut Voice<'_>,
        input: View<'_>,
    ) -> Outcome {
        self.state.clamp(input.song);
        view::show(ui, focused, voice, &mut self.state, input)
    }
}
