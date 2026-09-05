//! Where a real keyboard becomes the stage's keys.
//!
//! The one place egui's `Key` and `Modifiers` are spoken in the same
//! breath as ours. Everything the codebook does happens in its own
//! vocabulary (`stage::key`); this edge translates outward, chord by
//! chord, when a bound chord is taken out of the toolkit's input.

use super::keymap::{self, ScopeContext};
use crate::ui::stage::key::{Key, Mods};
use eframe::egui;

/// Ours, in egui's words. Total: every key the codebook has is a key
/// the toolkit has.
pub(super) fn egui_key(key: Key) -> egui::Key {
    match key {
        Key::A => egui::Key::A,
        Key::B => egui::Key::B,
        Key::C => egui::Key::C,
        Key::D => egui::Key::D,
        Key::E => egui::Key::E,
        Key::F => egui::Key::F,
        Key::G => egui::Key::G,
        Key::L => egui::Key::L,
        Key::M => egui::Key::M,
        Key::N => egui::Key::N,
        Key::O => egui::Key::O,
        Key::P => egui::Key::P,
        Key::Q => egui::Key::Q,
        Key::R => egui::Key::R,
        Key::S => egui::Key::S,
        Key::T => egui::Key::T,
        Key::V => egui::Key::V,
        Key::W => egui::Key::W,
        Key::X => egui::Key::X,
        Key::Z => egui::Key::Z,
        Key::ArrowDown => egui::Key::ArrowDown,
        Key::ArrowLeft => egui::Key::ArrowLeft,
        Key::ArrowRight => egui::Key::ArrowRight,
        Key::ArrowUp => egui::Key::ArrowUp,
        Key::Backspace => egui::Key::Backspace,
        Key::CloseBracket => egui::Key::CloseBracket,
        Key::Comma => egui::Key::Comma,
        Key::Delete => egui::Key::Delete,
        Key::End => egui::Key::End,
        Key::Enter => egui::Key::Enter,
        Key::Equals => egui::Key::Equals,
        Key::Escape => egui::Key::Escape,
        Key::F2 => egui::Key::F2,
        Key::Home => egui::Key::Home,
        Key::Minus => egui::Key::Minus,
        Key::OpenBracket => egui::Key::OpenBracket,
        Key::PageDown => egui::Key::PageDown,
        Key::PageUp => egui::Key::PageUp,
        Key::Plus => egui::Key::Plus,
        Key::Questionmark => egui::Key::Questionmark,
        Key::Slash => egui::Key::Slash,
        Key::Space => egui::Key::Space,
        Key::Tab => egui::Key::Tab,
    }
}

/// Ours, in egui's words: built the way the codebook used to write them,
/// so `consume_key` sees exactly the values it always saw.
pub(super) fn egui_mods(mods: Mods) -> egui::Modifiers {
    let mut out = egui::Modifiers::NONE;
    if mods.command {
        out = out.plus(egui::Modifiers::COMMAND);
    }
    if mods.shift {
        out = out.plus(egui::Modifiers::SHIFT);
    }
    out
}

/// Take, out of this frame's input, every chord the codebook binds in
/// this scope — most specific first, so a shifted chord is not eaten by
/// the unshifted one. Only this scope's chords: a key another scope
/// binds is not the stage's here, and taking it would swallow it —
/// inside a clip the arrows and Enter belong to the sequencer's grammar,
/// and the stage must leave them in the input for it. `yield_to_grammar`
/// names chords that are someone else's THIS frame even though the scope
/// binds them: a sentence in progress owns Escape.
pub(super) fn consume_chords(
    input: &mut egui::InputState,
    scope: ScopeContext,
    yield_to_grammar: impl Fn(Mods, Key) -> bool,
) -> Vec<(Mods, Key)> {
    keymap::bound_chords()
        .filter(|(modifiers, key)| {
            keymap::dispatch(scope, keymap::StageInput::Chord(*modifiers, *key)).is_some()
                && !yield_to_grammar(*modifiers, *key)
                && input.consume_key(egui_mods(*modifiers), egui_key(*key))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::StageIntent;

    /// The plaques carve these; a symbol that drifted from egui's would
    /// move a glyph on the codebook.
    #[test]
    fn the_symbols_are_eguis_own() {
        let pairs = [
            (Key::ArrowDown, egui::Key::ArrowDown),
            (Key::ArrowLeft, egui::Key::ArrowLeft),
            (Key::ArrowRight, egui::Key::ArrowRight),
            (Key::ArrowUp, egui::Key::ArrowUp),
            (Key::Minus, egui::Key::Minus),
            (Key::Questionmark, egui::Key::Questionmark),
            (Key::OpenBracket, egui::Key::OpenBracket),
            (Key::Enter, egui::Key::Enter),
            (Key::Space, egui::Key::Space),
            (Key::PageDown, egui::Key::PageDown),
            (Key::Q, egui::Key::Q),
        ];
        for (ours, theirs) in pairs {
            assert_eq!(ours.symbol_or_name(), theirs.symbol_or_name(), "{ours:?}");
            assert_eq!(ours.name(), theirs.name(), "{ours:?}");
        }
    }

    /// The same, through egui itself rather than through our reading of
    /// it: a real `^+T` press consumed the way `show` consumes it must
    /// come out as `^+T`, not as `^T`.
    #[test]
    fn a_shifted_chord_survives_being_consumed_through_egui() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        };
        let raw = RawInput {
            events: vec![Event::Key {
                key: egui::Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: press,
            }],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Root, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Mods::COMMAND.plus(Mods::SHIFT), Key::T)],
            "the shifted chord was consumed as something else"
        );
        assert_eq!(
            keymap::dispatch(
                ScopeContext::Root,
                keymap::StageInput::Chord(consumed[0].0, consumed[0].1)
            ),
            Some(StageIntent::NewInstrumentTrack)
        );
    }

    /// Inside a clip the arrows and Enter are the grammar's. The stage's
    /// consumption must leave them in the input — this shipped the other
    /// way, and every key the sequencer needed vanished before it looked.
    #[test]
    fn a_scope_only_consumes_the_chords_it_binds() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let raw = RawInput {
            events: vec![
                press(egui::Key::ArrowRight),
                press(egui::Key::Enter),
                press(egui::Key::Space),
            ],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Clip, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Mods::NONE, Key::Space)],
            "the clip scope took a key it does not bind"
        );
        assert!(
            input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            "the arrow was swallowed before the grammar could see it"
        );
        assert!(
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            "Enter was swallowed before the grammar could see it"
        );
    }

    /// egui's key read back into ours, for tests that start from a table
    /// written in egui's.
    fn ours(key: egui::Key) -> Option<Key> {
        Key::ALL.into_iter().find(|&k| egui_key(k) == key)
    }

    /// Two of our keys never become the same egui key, or the way back
    /// would be ambiguous.
    #[test]
    fn the_translation_is_one_to_one() {
        for (i, a) in Key::ALL.into_iter().enumerate() {
            for b in Key::ALL.into_iter().skip(i + 1) {
                assert_ne!(egui_key(a), egui_key(b), "{a:?} and {b:?}");
            }
        }
    }

    /// Inside a clip the sequencer's own command chords are its own:
    /// ^R resizes the clip and ^A selects all, and the stage must not
    /// take either before the grammar sees it. This is the binding that
    /// once shadowed clip resize with the library rescan.
    #[test]
    fn the_clip_scope_leaves_the_grammars_command_chords_alone() {
        for (verb, key, name) in crate::ui::sequencer::verbs::COMMAND_TABLE {
            let Some(key) = ours(*key) else {
                // A key the stage never binds cannot be taken by it.
                continue;
            };
            assert_eq!(
                keymap::dispatch(
                    ScopeContext::Clip,
                    keymap::StageInput::Chord(Mods::COMMAND, key)
                ),
                None,
                "the stage took ^{key:?} from the grammar's {name} ({verb:?})"
            );
        }
    }
}
