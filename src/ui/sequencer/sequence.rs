//! The lower sequence region and its persistent UI-local state.

use crate::ui::sequencer::grammar::Voice;
use crate::ui::sequencer::roll::RollPanel;
use crate::ui::sequencer::sequence_grid::SequenceGrid;
use crate::ui::sequencer::trig_info;
use crate::ui::tokens::space;
use eframe::egui;

/// Which editor the strip's time-detail shows. Both project the same
/// pattern; the grid folds time into rows and addresses steps, the roll
/// unfolds it and addresses (step, pitch). Ctrl+4 toggles, and the
/// choice is remembered PER PATTERN: a track edited in the roll stays
/// in the roll — the override the user asked for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Editor {
    #[default]
    Grid,
    Roll,
}

#[derive(Default)]
pub(crate) struct SequencePanel {
    grid: SequenceGrid,
    roll: RollPanel,
    /// Patterns the user has switched to the roll. Absent = the grid,
    /// so the default is unchanged and the memory is per pattern.
    roll_patterns: std::collections::HashSet<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteView {
    /// The stored address: anchor plus intent deviation.
    pub pitch: crate::pitch::Pitch,
    /// The resolved substrate value under the current key, green-side.
    pub hz: f64,
    /// The bridge-era playback pitch: nearest legacy MIDI.
    pub midi: u8,
    /// True when the legacy path cannot reproduce `hz` exactly — the
    /// machine's approximation sign, distinct from the musician's bend.
    pub approx: bool,
    pub start_ticks: usize,
    pub length_ticks: usize,
    /// Signed sub-step onset displacement in ticks (the push).
    pub micro_ticks: i16,
    pub velocity: u8,
    pub probability: f32,
    pub enabled: bool,
}

impl NoteView {
    /// A legacy note seen through the bridge: exact by construction.
    pub fn from_midi(
        midi: u8,
        start_ticks: usize,
        length_ticks: usize,
        velocity: u8,
        probability: f32,
        enabled: bool,
    ) -> Self {
        Self {
            pitch: crate::pitch::Pitch::from_midi(midi),
            hz: crate::pitch::midi_to_hz(midi),
            midi,
            approx: false,
            start_ticks,
            length_ticks,
            micro_ticks: 0,
            velocity,
            probability,
            enabled,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClipView<'a> {
    pub id: u64,
    pub name: &'a str,
    pub length_ticks: usize,
    pub notes: &'a [NoteView],
    /// Preview material: what a pending lossy transform WOULD write,
    /// drawn as ghosts before commit (the note-command preview
    /// contract). Empty when nothing is pending.
    pub ghosts: &'a [NoteView],
}

pub use crate::intent::sequence::Intent;

pub struct Outcome {
    pub intents: Vec<Intent>,
    pub claim_focus: bool,
    /// Where the panel drew, so the frame that owns focus can mark it
    /// (a focus bar, a scrim, an inversion — the frame's sign, not ours).
    pub content_rect: egui::Rect,
    /// The grid cursor's tick: the universal selection's address, read
    /// by the palette's long forms (`:tune`, `:quantize-key`, …).
    /// `None` when the sequence panel did not draw this frame.
    pub cursor_tick: Option<usize>,
}

impl Default for Outcome {
    fn default() -> Self {
        Self {
            intents: Vec::new(),
            claim_focus: false,
            content_rect: egui::Rect::NOTHING,
            cursor_tick: None,
        }
    }
}

impl SequencePanel {
    /// Paint into the shared detail strip. The strip itself — one egui
    /// panel, one remembered height — is owned by the frame; this panel
    /// only ever draws its content into whatever the strip provides.
    #[allow(clippy::too_many_arguments)] // One read-only context per concern.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        mut voice: Voice<'_>,
        entered_pitch: Option<crate::pitch::Pitch>,
        clip: Option<ClipView<'_>>,
        lens: &crate::ui::sequencer::lens::LensView,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        self.grid.update_view(ui.ctx());
        // Ctrl+4 joins the grid-resolution family (Ctrl+1/2/3) as the
        // editor switch: same hand, same neighbourhood, no new verb.
        if focused
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num4))
            && let Some(clip) = clip
        {
            if self.roll_patterns.contains(&clip.id) {
                self.roll_patterns.remove(&clip.id);
            } else {
                self.roll_patterns.insert(clip.id);
            }
        }
        let editor = match clip {
            Some(clip) if self.roll_patterns.contains(&clip.id) => Editor::Roll,
            _ => Editor::Grid,
        };
        if let Some(pitch) = entered_pitch {
            match editor {
                Editor::Grid => self.grid.enter_pitch(pitch, clip, &mut outcome.intents),
                Editor::Roll => {
                    self.roll
                        .enter_pitch(pitch, &crate::pitch::default_key(), &mut outcome.intents)
                }
            }
        }
        let content_rect = ui.available_rect_before_wrap();
        outcome.content_rect = content_rect;
        ui.allocate_rect(content_rect, egui::Sense::hover());
        outcome.claim_focus = ui.ctx().input(|input| input.pointer.any_pressed())
            && ui
                .ctx()
                .pointer_latest_pos()
                .is_some_and(|pointer| content_rect.contains(pointer));
        let trig_rect = trig_info::panel_rect(content_rect);
        let grid_rect = egui::Rect::from_min_max(
            egui::pos2(trig_rect.right() + space::LG, content_rect.top()),
            content_rect.right_bottom(),
        );
        match editor {
            Editor::Grid => {
                self.grid.show(
                    ui,
                    grid_rect,
                    focused,
                    &mut voice,
                    clip,
                    lens,
                    &mut outcome.intents,
                );
                trig_info::show(ui, trig_rect, self.grid.selection(clip));
                outcome.cursor_tick = Some(self.grid.cursor_tick());
            }
            Editor::Roll => {
                self.roll.show(
                    ui,
                    grid_rect,
                    focused,
                    &mut voice,
                    clip,
                    &mut outcome.intents,
                );
                trig_info::show(ui, trig_rect, self.roll.selection(clip));
                outcome.cursor_tick = Some(self.roll.cursor_tick());
            }
        }
        outcome
    }
}
