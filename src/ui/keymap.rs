//! The keymap: a TABLE from key gesture to `UiAction`, not a chain of `if`s.
//!
//! Because bindings are data, three things fall out for free: the app loop
//! stops growing a branch per shortcut, a tooltip can ask "what key runs
//! this action?", and a shortcut sheet (or a future command palette) renders
//! itself from the same list that dispatches.
//!
//! Bindings are CONSUMED (`consume_shortcut`), so a bound key never also
//! reaches a widget, and the whole map stands down while a text field has
//! the keyboard.

use crate::ui::action::UiAction;
use crate::ui::vm::{TrackKind, ViewState};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub shortcut: KeyboardShortcut,
    pub action: UiAction,
}

impl Binding {
    pub const fn new(modifiers: Modifiers, key: Key, action: UiAction) -> Self {
        Self {
            shortcut: KeyboardShortcut::new(modifiers, key),
            action,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        // ORDER IS LOAD-BEARING for the two Ctrl+T bindings. egui's
        // `consume_shortcut` matches modifiers LOGICALLY — an extra Shift
        // is ignored — so a plain Ctrl+T pattern also matches Ctrl+Shift+T.
        // The more specific gesture must therefore be listed (and consumed)
        // first, or Ctrl+Shift+T would make an audio track.
        Self::new(vec![
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::T,
                UiAction::AddTrack(TrackKind::Midi),
            ),
            Binding::new(
                Modifiers::COMMAND,
                Key::T,
                UiAction::AddTrack(TrackKind::Audio),
            ),
            // Shift+Space and Ctrl+Space BEFORE plain Space, the same
            // reasoning as the track pair above: `consume_shortcut` matches
            // modifiers logically, and the plain gesture must not swallow
            // the specific ones.
            Binding::new(Modifiers::SHIFT, Key::Space, UiAction::ContinuePlay),
            Binding::new(Modifiers::COMMAND, Key::Space, UiAction::PlaySelection),
            Binding::new(Modifiers::NONE, Key::Space, UiAction::TogglePlay),
            Binding::new(Modifiers::NONE, Key::Home, UiAction::Return),
            Binding::new(Modifiers::NONE, Key::O, UiAction::ToggleMetronome),
            Binding::new(Modifiers::NONE, Key::F9, UiAction::ToggleRecord),
            Binding::new(Modifiers::NONE, Key::F10, UiAction::BackToArrangement),
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::F,
                UiAction::ToggleFollow,
            ),
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::M,
                UiAction::CreateClip,
            ),
            Binding::new(Modifiers::COMMAND, Key::C, UiAction::CopyClip),
            Binding::new(Modifiers::COMMAND, Key::V, UiAction::PasteClip),
            Binding::new(Modifiers::COMMAND, Key::D, UiAction::DuplicateClip),
            // Ctrl+Shift+Z before Ctrl+Z, for the reason spelled out above
            // the track pair: the plain gesture would otherwise match the
            // shifted one and undo when asked to redo. Ctrl+Y is the second
            // redo gesture, so `shortcut_for` reports the first one listed.
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::Z,
                UiAction::Redo,
            ),
            Binding::new(Modifiers::COMMAND, Key::Z, UiAction::Undo),
            Binding::new(Modifiers::COMMAND, Key::Y, UiAction::Redo),
            // The frame regions. MODIFIED gestures, not bare letters,
            // deliberately: the plan is for bare letters to become notes
            // when musical typing lands, and a view toggle that stopped
            // working the day the keyboard became an instrument would be
            // a binding chosen for today only.
            // The rack's two verbs. The palette has advertised these as
            // hints since they were written and nothing was listening —
            // a shortcut printed beside a command that does not answer to
            // it is worse than no shortcut, because it is tried once and
            // then the whole sheet is distrusted.
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::G,
                UiAction::UngroupDevices,
            ),
            Binding::new(Modifiers::COMMAND, Key::G, UiAction::GroupDevices),
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::ALT),
                Key::B,
                UiAction::ToggleBrowser,
            ),
            Binding::new(
                Modifiers::COMMAND.plus(Modifiers::ALT),
                Key::L,
                UiAction::ToggleLower,
            ),
            Binding::new(
                Modifiers::COMMAND
                    .plus(Modifiers::ALT)
                    .plus(Modifiers::SHIFT),
                Key::F,
                UiAction::ToggleChrome,
            ),
        ])
    }
}

impl Keymap {
    pub fn new(bindings: Vec<Binding>) -> Self {
        debug_assert!(
            {
                let mut seen: Vec<KeyboardShortcut> = Vec::new();
                bindings.iter().all(|b| {
                    let fresh = !seen.contains(&b.shortcut);
                    seen.push(b.shortcut);
                    fresh
                })
            },
            "two bindings share one gesture — the later one would be dead"
        );
        Self { bindings }
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The gesture bound to an action, for tooltips and menus. `None` means
    /// unbound, which is a normal answer, not a failure.
    pub fn shortcut_for(&self, action: UiAction) -> Option<KeyboardShortcut> {
        self.bindings
            .iter()
            .find(|b| b.action == action)
            .map(|b| b.shortcut)
    }

    /// Drain this frame's key gestures into actions.
    ///
    /// Skips everything while egui has the keyboard (a focused DragValue or
    /// text field owns its keys), and skips engine-requiring actions while
    /// the engine is off — so a stray spacebar is silence, not a panic path.
    pub fn resolve(&self, ctx: &egui::Context, vs: &ViewState, out: &mut Vec<UiAction>) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            for binding in &self.bindings {
                if binding.action.needs_engine() && !vs.engine_running {
                    continue;
                }
                if i.consume_shortcut(&binding.shortcut) {
                    out.push(binding.action);
                }
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_are_unique() {
        let map = Keymap::default();
        for (i, a) in map.bindings().iter().enumerate() {
            for b in &map.bindings()[i + 1..] {
                assert_ne!(
                    a.shortcut,
                    b.shortcut,
                    "`{}` and `{}` share a gesture",
                    a.action.label(),
                    b.action.label()
                );
            }
        }
    }

    /// The two track gestures share a key, and `consume_shortcut` ignores
    /// an EXTRA Shift — so Ctrl+Shift+T also matches the plain Ctrl+T
    /// pattern. The only thing that keeps them apart is list order: the
    /// specific gesture must be offered (and consume the press) first.
    ///
    /// This pins that order. Swap the two bindings and Ctrl+Shift+T makes
    /// an audio track, which is exactly the kind of bug nobody thinks to
    /// look for in a table.
    #[test]
    fn shift_specific_track_gesture_wins() {
        let map = Keymap::default();
        let pos = |action: UiAction| {
            map.bindings()
                .iter()
                .position(|b| b.action == action)
                .expect("both track gestures are bound")
        };
        let midi = pos(UiAction::AddTrack(TrackKind::Midi));
        let audio = pos(UiAction::AddTrack(TrackKind::Audio));
        assert!(
            midi < audio,
            "Ctrl+Shift+T must be checked before Ctrl+T, or it never fires"
        );
        let shortcut = map
            .shortcut_for(UiAction::AddTrack(TrackKind::Midi))
            .unwrap();
        assert_eq!(shortcut.logical_key, Key::T);
        assert!(shortcut.modifiers.shift, "the MIDI gesture carries Shift");
        assert!(
            !map.shortcut_for(UiAction::AddTrack(TrackKind::Audio))
                .unwrap()
                .modifiers
                .shift,
            "the audio gesture does not"
        );
    }

    #[test]
    fn lookup_round_trips() {
        let map = Keymap::default();
        let sc = map.shortcut_for(UiAction::TogglePlay).unwrap();
        assert_eq!(sc.logical_key, Key::Space);
        let create = map.shortcut_for(UiAction::CreateClip).unwrap();
        assert_eq!(create.logical_key, Key::M);
        assert!(create.modifiers.command);
        assert!(create.modifiers.shift);
        let browser = map.shortcut_for(UiAction::ToggleBrowser).unwrap();
        assert!(browser.modifiers.command);
        assert!(browser.modifiers.alt);
        assert!(map.shortcut_for(UiAction::StartEngine).is_none());
    }
}
