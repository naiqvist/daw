//! The view: an empty window.
//!
//! The canvas the next look is drawn on. Everything the stage IS still
//! runs behind this glass — every key goes through the codebook, the
//! machine room takes the keyboard when summoned and gives it back, the
//! clock ticks — and nothing is painted but the ground. What was drawn
//! before is on `mvp-port` at 2ee4b64, the worked example of the contract
//! in `notes/20260905-ui-seam-contract.md`.
//!
//! Not yet on this glass, and so not yet reachable: the sequencer (a
//! shared toolkit widget that is shown to be driven), the command palette
//! (never summoned here, so it never owns the keys), and every surface
//! the old view drew.

mod heads;
mod input;
mod utility;

use super::key::{Key, Mods};
use super::*;
use eframe::egui;

/// An axis nothing is drawn along yet can hold everything: no offset
/// needs to move to keep the cursor in sight.
const HOLDS_EVERYTHING: usize = usize::MAX;

impl Stage {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.begin_frame();
        if self.poll_library() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        // A utility room is a true modal: it gets the frame's keyboard
        // before the musical surface and may close itself with Escape.
        self.update_utility(ui.ctx());
        let utility_open = self.utility.is_open();

        self.hold_browser_for_exit();

        let collect_text = self.collects_text();
        let grammar_owns_escape = self.grammar_owns_escape();
        let scope = self.scope_context();
        let selection_scope = self.selection_scope();
        let selection_held = selection_scope && ui.input(|input| input.key_down(egui::Key::X));
        let selection_pressed = selection_scope
            && ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::X,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if *modifiers == egui::Modifiers::NONE
                    )
                })
            });
        let inputs = if utility_open {
            Vec::new()
        } else {
            ui.input_mut(|input| {
                let chords = input::consume_chords(input, scope, |modifiers, key| {
                    grammar_owns_escape && modifiers == Mods::NONE && key == Key::Escape
                });
                let pressed = |wanted: Key| chords.iter().any(|(_, key)| *key == wanted);
                let questionmark_consumed = pressed(Key::Questionmark);
                let space_consumed = pressed(Key::Space);
                let mut stage_inputs: Vec<keymap::StageInput> = chords
                    .iter()
                    .map(|(modifiers, key)| keymap::StageInput::Chord(*modifiers, *key))
                    .collect();
                if collect_text {
                    for event in &input.events {
                        let egui::Event::Text(text) = event else {
                            continue;
                        };
                        // A physical '?' is the codebook chord and egui also
                        // emits it as text; admit each keystroke once.
                        if questionmark_consumed && text == "?" {
                            continue;
                        }
                        if space_consumed && text == " " {
                            continue;
                        }
                        stage_inputs.extend(text.chars().map(keymap::StageInput::Text));
                    }
                }
                stage_inputs
            })
        };
        self.take_inputs(inputs, selection_held, selection_pressed);

        // Inside a clip, the letters may be pitches. Read after the
        // stage's own chords, so `^T` is never read as a T.
        let update = self
            .pitch_entry_mode()
            .map(|mode| self.midi_typing.update(ui.ctx(), mode));
        let enter_held = ui.input(|input| input.key_down(egui::Key::Enter));
        self.take_pitch_entry(update, enter_held);

        let field = ui.available_rect_before_wrap();
        self.follow_cursor(
            heads::capacity(field.width()),
            HOLDS_EVERYTHING,
            HOLDS_EVERYTHING,
        );

        let dt = ui.ctx().input(|input| input.stable_dt);
        self.tick_clock(dt);
        if self.wants_repaint() {
            ui.ctx().request_repaint();
        }

        self.draw(ui);
    }

    /// The ground, and on it what has been drawn so far.
    fn draw(&self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter();
        painter.rect_filled(whole, 0.0, self.alphabet().ground.color);
        self.draw_heads(painter, whole);
    }
}
