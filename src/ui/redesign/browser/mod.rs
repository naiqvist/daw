//! Keyboard-first sample browser.
//!
//! The browser reads the library service's immutable snapshot and emits
//! semantic intents. It never walks the filesystem or reaches into the audio
//! engine.

mod state;
mod view;

use crate::library::LibrarySnapshot;
use crate::ui::redesign::grammar::Sentence;
use eframe::egui;
use state::BrowserState;
use std::path::PathBuf;

/// How wide the hand may drag the browser, in points. A constant, never
/// a window fraction.
const MAX_PANEL_W: f32 = 420.0;

pub struct View<'a> {
    pub snapshot: &'a LibrarySnapshot,
    pub scanning: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Intent {
    SelectSample(PathBuf),
    AuditionSample(PathBuf),
    StopAudition,
}

#[derive(Default)]
pub struct Outcome {
    pub intents: Vec<Intent>,
    pub(crate) claim_focus: bool,
}

#[derive(Default)]
pub struct BrowserPanel {
    state: BrowserState,
}

impl BrowserPanel {
    #[allow(private_interfaces)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        sentence: &mut Sentence,
        view: View<'_>,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        egui::Panel::left("redesign-browser")
            .resizable(true)
            .show_separator_line(false)
            .default_size(self.state.width())
            // A fixed cap, not a window fraction: resizing the window
            // must never resize the browser. The seam is still draggable.
            .max_size(MAX_PANEL_W)
            .frame(view::frame())
            .show(ui, |ui| {
                outcome = view::show(ui, focused, sentence, &mut self.state, view);
            });
        outcome
    }
}
