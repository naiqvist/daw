//! The lower sequence region and its persistent UI-local state.

use crate::design::kit::Weight;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::sequencer::chrome;
use crate::ui::sequencer::grammar::Voice;
use crate::ui::sequencer::roll::RollPanel;
use crate::ui::sequencer::sequence_grid::SequenceGrid;
use crate::ui::sequencer::trig_info;
use crate::ui::tokens::{font, space};
use eframe::egui;

/// The two equal targets at the left of either editor's header. The
/// editor owns the rest of the header; this piece belongs to their shared
/// parent because choosing a projection is not an edit inside either one.
pub(crate) const EDITOR_SWITCH_WIDTH: f32 = 104.0;
const EDITOR_TAB_WIDTH: f32 = EDITOR_SWITCH_WIDTH * 0.5;
/// Levels above the ground, not colours: see `sequencer::shade`.
const EDITOR_ACTIVE_FILL: u8 = 34;
const EDITOR_RESTING_INK: u8 = 145;
/// How far a resting tab is lifted off the ground behind it.
const EDITOR_RESTING_WASH: u8 = 8;

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

/// Draw the one shared GRID / ROLL choice into an editor's header. The
/// current projection is a filled plane with a bright foot; the other is
/// still a full pointer target, not merely explanatory text.
pub(crate) fn editor_switch(
    ui: &mut egui::Ui,
    header: egui::Rect,
    current: Editor,
    ground: crate::design::Polarity,
) -> Option<Editor> {
    let painter = ui.painter_at(header);
    let mut requested = None;
    for (index, (editor, label)) in [(Editor::Grid, "GRID"), (Editor::Roll, "ROLL")]
        .into_iter()
        .enumerate()
    {
        let tab = egui::Rect::from_min_size(
            header.min + egui::vec2(index as f32 * EDITOR_TAB_WIDTH, 0.0),
            egui::vec2(EDITOR_TAB_WIDTH, header.height()),
        );
        let response = ui
            .interact(
                tab,
                ui.id().with(("sequence-editor", label)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        let alpha = crate::ui::sequencer::alphabet(ground);
        let active = editor == current;
        let hovered = response.hovered() && !active;
        // A selector on the case: square, and marked by an UNDERSCORE
        // rather than by a lit outline. The chosen position is the one
        // with a rule under it, the way a laboratory switch's chosen
        // detent is the one with the mark beside it.
        let plate = tab.shrink2(egui::vec2(1.0, 2.0));
        let mut shell = Vec::new();
        shell.push(egui::Shape::rect_filled(
            plate,
            0.0,
            if active {
                crate::ui::sequencer::shade(EDITOR_ACTIVE_FILL, ground)
            } else if hovered {
                crate::ui::sequencer::wash(EDITOR_RESTING_WASH, ground)
            } else {
                crate::ui::sequencer::shade(0, ground)
            },
        ));
        chrome::trace(
            &mut shell,
            &[
                egui::pos2(plate.left(), plate.bottom()),
                egui::pos2(plate.right(), plate.bottom()),
            ],
            if active { Weight::Heavy } else { Weight::Hair },
            if active {
                alpha.ink.color
            } else {
                alpha.edge.color
            },
        );
        for shape in shell {
            painter.add(shape);
        }
        if active {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    tab.left_bottom() + egui::vec2(space::SM, -2.0),
                    tab.right_bottom() + egui::vec2(-space::SM, 0.0),
                ),
                0.0,
                crate::ui::sequencer::shade(crate::ui::sequencer::INK_LEVEL, ground),
            );
        }
        painter.text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
            if editor == current {
                crate::ui::sequencer::shade(crate::ui::sequencer::INK_LEVEL, ground)
            } else {
                crate::ui::sequencer::shade(EDITOR_RESTING_INK, ground)
            },
        );
        if response.clicked() && editor != current {
            requested = Some(editor);
        }
    }
    requested
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
    /// Deterministic A:B cycle condition on this trig.
    pub cond: Option<(u8, u8)>,
    pub enabled: bool,
    pub muted: bool,
    /// How many parameters the trig this note belongs to holds locked.
    /// A trig's fact carried on each of its notes, because the views are
    /// per note and a lock mark belongs on the cell either way.
    pub locks: u8,
    /// The slice the trig holds locked, from one, if it holds one — the
    /// sampler's SLICE row. A cell on a slicing track wears it as a tag.
    pub slice: Option<u8>,
    /// The trig plays through a locked sound, not the track's machine.
    pub sound: bool,
    /// How many of the trig's locks slide to the next lock.
    pub slides: u8,
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
            cond: None,
            enabled,
            muted: !enabled,
            locks: 0,
            slice: None,
            sound: false,
            slides: 0,
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
    /// Whether the track's voice is a sampler in slice mode. A cell on
    /// such a track wears the trig's locked slice as a tag; the note
    /// stays a pitch, because on this deck a note IS a pitch and the
    /// slice is a row.
    pub slicing: bool,
    /// Each step's rules — locks, condition, retrig, whether a sound is
    /// locked — so a yank carries a trig whole. Empty means none.
    pub rules: &'a [crate::sequencing::TrigRules],
}

pub use crate::intent::sequence::Intent;

/// One computer/MIDI-keyboard step-entry event, including the complete
/// chord still held after it and whether this event begins, extends, or
/// repeats that held gesture.
#[derive(Clone, Debug)]
pub(crate) struct PitchEntry {
    pub(crate) pitches: Vec<crate::pitch::Pitch>,
    pub(crate) gesture: crate::ui::sequencer::midi_typing::EntryGesture,
}

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
    /// Where the cursor's cell was drawn, so a frame can put a mark ON
    /// the thing under the cursor — a callout, a bubble — rather than
    /// somewhere near the editor. `None` when the cell was off screen.
    pub cursor_rect: Option<egui::Rect>,
}

/// Optional keyboard legend laid over the grid by a frame that turns sixteen
/// physical keys into step pads. It is presentation state only: the frame
/// still owns the gesture and the sequencer still owns the cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepKeysView {
    pub window: usize,
    pub held: u16,
}

impl Default for Outcome {
    fn default() -> Self {
        Self {
            intents: Vec::new(),
            claim_focus: false,
            content_rect: egui::Rect::NOTHING,
            cursor_tick: None,
            cursor_rect: None,
        }
    }
}

impl SequencePanel {
    pub(crate) fn has_selection(&self) -> bool {
        self.grid.has_selection() || self.roll.has_selection()
    }

    fn clear_selection(&mut self) {
        // A selection in the other projection is deliberately not a
        // second hidden mode. Escape means let go of the selection, full
        // stop, even if the editor was switched after making it.
        self.grid.clear_selection();
        self.roll.clear_selection();
    }

    /// Time cells addressed by the current projection. Roll pitch cells
    /// that share time collapse to one entry, because a parameter lock
    /// belongs to the trig rather than to an individual chord tone.
    pub(crate) fn addressed_ticks(&self, clip: Option<ClipView<'_>>) -> Vec<usize> {
        match clip.map_or(Editor::Grid, |clip| self.editor_for(clip.id)) {
            Editor::Grid => self.grid.addressed_ticks(),
            Editor::Roll => self.roll.addressed_ticks(),
        }
    }

    /// The steps a STANDING selection covers in the editor showing
    /// `pattern`, or `None` when nothing is selected: what the deck's
    /// cells lock when a hand has selected first and turns second.
    pub(crate) fn standing_selection_steps(
        &self,
        pattern: u64,
        step_ticks: usize,
    ) -> Option<Vec<usize>> {
        let (selected, ticks) = match self.editor_for(pattern) {
            Editor::Grid => (self.grid.has_selection(), self.grid.addressed_ticks()),
            Editor::Roll => (self.roll.has_selection(), self.roll.addressed_ticks()),
        };
        if !selected {
            return None;
        }
        let mut steps: Vec<usize> = ticks
            .into_iter()
            .map(|tick| tick / step_ticks.max(1))
            .collect();
        steps.sort_unstable();
        steps.dedup();
        Some(steps)
    }

    pub(crate) fn set_time_selected(&mut self, clip: ClipView<'_>, tick: usize, selected: bool) {
        match self.editor_for(clip.id) {
            Editor::Grid => self.grid.set_time_selected(clip.id, tick, selected),
            Editor::Roll => self.roll.set_time_selected(clip.id, tick, selected),
        }
    }

    /// What is under the cursor in whichever editor this clip is shown
    /// in: the same snapshot the inspector states, for a frame that
    /// wants to act on it.
    pub(crate) fn selection(
        &self,
        clip: Option<ClipView<'_>>,
    ) -> crate::ui::sequencer::sequence_grid::TrigSelection {
        match clip.map_or(Editor::Grid, |clip| self.editor_for(clip.id)) {
            Editor::Grid => self.grid.selection(clip),
            Editor::Roll => self.roll.selection(clip),
        }
    }

    fn editor_for(&self, pattern: u64) -> Editor {
        if self.roll_patterns.contains(&pattern) {
            Editor::Roll
        } else {
            Editor::Grid
        }
    }

    fn select_editor(&mut self, pattern: u64, editor: Editor) {
        match editor {
            Editor::Grid => {
                self.roll_patterns.remove(&pattern);
            }
            Editor::Roll => {
                self.roll_patterns.insert(pattern);
            }
        }
    }

    fn toggle_editor(&mut self, pattern: u64) {
        let next = match self.editor_for(pattern) {
            Editor::Grid => Editor::Roll,
            Editor::Roll => Editor::Grid,
        };
        self.select_editor(pattern, next);
    }

    /// Paint into the shared detail strip. The strip itself — one egui
    /// panel, one remembered height — is owned by the frame; this panel
    /// only ever draws its content into whatever the strip provides.
    #[allow(clippy::too_many_arguments)] // One read-only context per concern.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        mut voice: Voice<'_>,
        entered_pitch: Option<PitchEntry>,
        clip: Option<ClipView<'_>>,
        lens: &crate::ui::sequencer::lens::LensView,
        step_keys: Option<StepKeysView>,
        ground: crate::design::Polarity,
        // Where the transport is INSIDE this pattern, in ticks, when
        // this clip is the one sounding. `None` covers both a parked
        // transport and a clip nobody launched — in either case there is
        // no present moment to draw, and drawing one anyway would be the
        // surface asserting something it does not know.
        playhead: Option<usize>,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        // Ctrl+4 joins the grid-resolution family (Ctrl+1/2/3) as the
        // editor switch: same hand, same neighbourhood, no new verb.
        if focused
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num4))
            && let Some(clip) = clip
        {
            self.toggle_editor(clip.id);
        }
        let editor = clip.map_or(Editor::Grid, |clip| self.editor_for(clip.id));
        if focused
            && self.has_selection()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.clear_selection();
        }
        // Each projection owns its own camera. Only the one on screen may
        // consume the shared zoom chords or move in response to them.
        match editor {
            Editor::Grid => self.grid.update_view(ui.ctx()),
            Editor::Roll => self.roll.update_view(ui.ctx()),
        }
        if let Some(entry) = entered_pitch {
            match editor {
                Editor::Grid => self.grid.enter_pitch(entry, clip, &mut outcome.intents),
                Editor::Roll => self.roll.enter_pitch(
                    entry,
                    &crate::pitch::default_key(),
                    clip,
                    &mut outcome.intents,
                ),
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
        let requested = match editor {
            Editor::Grid => {
                let requested = self.grid.show(
                    ui,
                    grid_rect,
                    focused,
                    &mut voice,
                    clip,
                    lens,
                    step_keys,
                    &mut outcome.intents,
                    ground,
                    playhead,
                );
                trig_info::show(ui, trig_rect, self.grid.selection(clip), ground);
                outcome.cursor_tick = Some(self.grid.cursor_tick());
                outcome.cursor_rect = self.grid.cursor_rect();
                requested
            }
            Editor::Roll => {
                let requested = self.roll.show(
                    ui,
                    grid_rect,
                    focused,
                    &mut voice,
                    clip,
                    &mut outcome.intents,
                    ground,
                    playhead,
                );
                trig_info::show(ui, trig_rect, self.roll.selection(clip), ground);
                outcome.cursor_tick = Some(self.roll.cursor_tick());
                outcome.cursor_rect = self.roll.cursor_rect();
                requested
            }
        };
        if let Some(requested) = requested
            && let Some(clip) = clip
        {
            self.select_editor(clip.id, requested);
            ui.ctx().request_repaint();
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_switch(current: Editor, at: egui::Pos2) -> Option<Editor> {
        let ctx = egui::Context::default();
        let host = egui::Rect::from_min_size(
            egui::pos2(20.0, 20.0),
            egui::vec2(EDITOR_SWITCH_WIDTH, 24.0),
        );
        let mut requested = None;
        // Hover is established one frame before egui can press the target.
        for pressed in [None, Some(true), Some(false)] {
            let mut run = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 120.0),
                    )),
                    events: {
                        let mut events = vec![egui::Event::PointerMoved(at)];
                        if let Some(pressed) = pressed {
                            events.push(egui::Event::PointerButton {
                                pos: at,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                        events
                    },
                    ..Default::default()
                },
                |ui| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(host)
                            .id_salt("editor-switch-test"),
                    );
                    requested = requested.or(editor_switch(
                        &mut child,
                        host,
                        current,
                        crate::design::Polarity::Dark,
                    ));
                },
            );
            run.textures_delta.clear();
        }
        requested
    }

    #[test]
    fn editor_choice_defaults_to_grid_and_is_remembered_per_pattern() {
        let mut panel = SequencePanel::default();
        assert_eq!(panel.editor_for(7), Editor::Grid);
        assert_eq!(panel.editor_for(8), Editor::Grid);

        panel.select_editor(7, Editor::Roll);
        assert_eq!(panel.editor_for(7), Editor::Roll);
        assert_eq!(panel.editor_for(8), Editor::Grid);

        panel.select_editor(7, Editor::Grid);
        assert_eq!(panel.editor_for(7), Editor::Grid);
    }

    #[test]
    fn editor_toggle_is_closed_between_the_two_projections() {
        let mut panel = SequencePanel::default();
        panel.toggle_editor(12);
        assert_eq!(panel.editor_for(12), Editor::Roll);
        panel.toggle_editor(12);
        assert_eq!(panel.editor_for(12), Editor::Grid);
    }

    #[test]
    fn both_editor_names_are_real_pointer_targets() {
        assert_eq!(
            click_switch(Editor::Grid, egui::pos2(98.0, 32.0)),
            Some(Editor::Roll)
        );
        assert_eq!(
            click_switch(Editor::Roll, egui::pos2(46.0, 32.0)),
            Some(Editor::Grid)
        );
        assert_eq!(
            click_switch(Editor::Grid, egui::pos2(46.0, 32.0)),
            None,
            "clicking the standing view spuriously toggled it"
        );
    }
}
