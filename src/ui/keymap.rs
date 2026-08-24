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
use crate::ui::vm::ViewState;
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
        Self::new(vec![
            Binding::new(Modifiers::NONE, Key::Space, UiAction::TogglePlay),
            Binding::new(Modifiers::NONE, Key::Home, UiAction::Return),
            Binding::new(Modifiers::COMMAND, Key::M, UiAction::ToggleMetronome),
            Binding::new(Modifiers::COMMAND, Key::C, UiAction::CopyClip),
            Binding::new(Modifiers::COMMAND, Key::V, UiAction::PasteClip),
            Binding::new(Modifiers::COMMAND, Key::D, UiAction::DuplicateClip),
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

    #[test]
    fn lookup_round_trips() {
        let map = Keymap::default();
        let sc = map.shortcut_for(UiAction::TogglePlay).unwrap();
        assert_eq!(sc.logical_key, Key::Space);
        assert!(map.shortcut_for(UiAction::StartEngine).is_none());
    }
}
