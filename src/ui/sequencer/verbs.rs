//! The redesign's closed verb table.
//!
//! One verb, one key, everywhere — the grammar's counterpart to `signs.rs`.
//! A panel declares which verbs its nouns support; it may not rebind a
//! verb's key or invent a panel-private verb. A gesture a panel truly needs
//! either generalises into this table (and then works everywhere) or it
//! does not exist. Contract: `notes/20260831-command-grammar.md`.

use eframe::egui;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verb {
    /// The noun's one primary act: toggle a trig, launch a clip, open a page.
    Act,
    /// Toggle the noun under the cursor in the current selection.
    Select,
    /// Widen the current surface's selection to every peer noun.
    SelectAll,
    Delete,
    Yank,
    Put,
    Duplicate,
    /// Duplicate every note at the addressed time.
    StackDuplicate,
    /// Move by the current grid unit. Takes a motion.
    Nudge,
    /// Move every note at the addressed time. Takes a motion. In a
    /// single-note view this is the shifted counterpart of [`Nudge`].
    StackNudge,
    /// Grow or shrink by the current grid unit. Takes a motion.
    Resize,
    /// Resize every note at the addressed time.
    StackResize,
    /// Resize the containing clip rather than any note inside it.
    ClipResize,
    /// Raise or lower velocity. Takes a vertical motion.
    Velocity,
    /// Raise or lower every note's velocity at the addressed time.
    StackVelocity,
    Mute,
    Solo,
    /// Arm the addressed track for recording.
    Arm,
    /// Cycle the addressed track's input monitor through off/in/auto.
    Monitor,
    Rename,
    /// Edit a trig's condition sign.
    Condition,
    /// Fuzzy name-search within the noun's world.
    Search,
    /// Yank every note at the addressed time instead of one note.
    StackYank,
    /// Put a yanked stack instead of one note.
    StackPut,
    /// Enter the Euclidean mode on the step grid: the arrows then cycle
    /// rhythms, Enter keeps one, Escape puts the old steps back.
    Euclid,
    /// Escape while a mode is on. Never on a key of its own: the grammar
    /// speaks it when Escape lands on a modal sentence.
    Cancel,
}

/// The whole vocabulary: verb, key, display name. Tests hold every column
/// to uniqueness. In MIDI mode the letter keys are pitches instead —
/// `midi_typing` consumes them first, which is the announced modal trade.
pub(crate) const TABLE: &[(Verb, egui::Key, &str)] = &[
    (Verb::Act, egui::Key::Enter, "ACT"),
    (Verb::Delete, egui::Key::Delete, "DELETE"),
    (Verb::Select, egui::Key::X, "SELECT"),
    // Verbs spoken mid-navigation sit under the LEFT hand, because the
    // right hand owns the arrows: reach beats mnemonics. Q/W/E form the
    // move-and-copy cluster (nudge lives on W below).
    (Verb::Yank, egui::Key::Q, "YANK"),
    // E is the Euclidean mode; PUT moved to P for it, its mnemonic.
    (Verb::Euclid, egui::Key::E, "EUCLID"),
    (Verb::Put, egui::Key::P, "PUT"),
    (Verb::Duplicate, egui::Key::D, "DUPLICATE"),
    (Verb::Nudge, egui::Key::W, "NUDGE"),
    (Verb::Resize, egui::Key::R, "RESIZE"),
    (Verb::Velocity, egui::Key::F, "VELOCITY"),
    (Verb::Mute, egui::Key::M, "MUTE"),
    (Verb::Solo, egui::Key::S, "SOLO"),
    (Verb::Arm, egui::Key::A, "ARM"),
    // M already means mute and I enters MIDI typing. V keeps monitoring on
    // the left hand without stealing either established word.
    (Verb::Monitor, egui::Key::V, "MONITOR"),
    (Verb::Rename, egui::Key::F2, "RENAME"),
    (Verb::Condition, egui::Key::C, "CONDITION"),
    (Verb::Search, egui::Key::Slash, "SEARCH"),
];

/// Shifted words are distinct verbs, not a modifier leaked into panel
/// policy. Plain Q/W/E address the noun under the cursor; their shifted
/// forms explicitly widen that noun to the whole time-aligned stack.
pub(crate) const SHIFT_TABLE: &[(Verb, egui::Key, &str)] = &[
    (Verb::StackYank, egui::Key::Q, "STACK YANK"),
    (Verb::StackNudge, egui::Key::W, "STACK NUDGE"),
    (Verb::StackPut, egui::Key::P, "STACK PUT"),
    (Verb::StackDuplicate, egui::Key::D, "STACK DUPLICATE"),
    (Verb::StackResize, egui::Key::R, "STACK RESIZE"),
    (Verb::StackVelocity, egui::Key::F, "STACK VELOCITY"),
];

/// Conventional command chords remain verbs: the modifier is merely the
/// spelling that keeps a broad, less-frequent action off the home alphabet.
pub(crate) const COMMAND_TABLE: &[(Verb, egui::Key, &str)] = &[
    (Verb::SelectAll, egui::Key::A, "SELECT ALL"),
    (Verb::ClipResize, egui::Key::R, "CLIP RESIZE"),
];

impl Verb {
    /// What the verb does, in a codebook row's worth of words.
    pub(crate) fn brief(self) -> &'static str {
        match self {
            Self::Act => "toggle step / enter",
            Self::Delete => "delete note / sel",
            Self::Select => "select here",
            Self::SelectAll => "select all steps",
            Self::Yank => "yank to register",
            Self::Put => "put register here",
            Self::Duplicate => "copy N steps on",
            Self::Nudge => "then arrow: move",
            Self::Resize => "then ←/→: resize",
            Self::ClipResize => "then ←/→: clip len",
            Self::Velocity => "then ↑/↓: velocity",
            Self::Mute => "mute note / sel",
            Self::Solo => "solo track",
            Self::Arm => "arm track",
            Self::Monitor => "cycle monitor",
            Self::Rename => "rename clip",
            Self::Condition => "chance; 50 C = 50%",
            Self::Search => "find by name",
            Self::Euclid => "euclid mode",
            Self::Cancel => "leave the mode",
            Self::StackYank => "yank whole stack",
            Self::StackPut => "put yanked stack",
            Self::StackDuplicate => "copy stack N on",
            Self::StackNudge => "then arrow: stack",
            Self::StackResize => "then ←/→: stack",
            Self::StackVelocity => "then ↑/↓: stack vel",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        if self == Self::Cancel {
            return "CANCEL";
        }
        TABLE
            .iter()
            .chain(SHIFT_TABLE)
            .chain(COMMAND_TABLE)
            .find(|(verb, _, _)| *verb == self)
            .map(|(_, _, name)| *name)
            .unwrap_or("?")
    }

    /// A motion verb waits for its direction; the rest act on the spot.
    pub(crate) fn needs_motion(self) -> bool {
        matches!(
            self,
            Verb::Nudge
                | Verb::StackNudge
                | Verb::Resize
                | Verb::StackResize
                | Verb::ClipResize
                | Verb::Velocity
                | Verb::StackVelocity
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{COMMAND_TABLE, SHIFT_TABLE, TABLE};

    /// One key per verb, one verb per key: the bijection every panel
    /// depends on for transferable muscle memory.
    #[test]
    fn the_verb_table_is_a_bijection() {
        for (i, (verb, key, name)) in TABLE.iter().enumerate() {
            for (other_verb, other_key, other_name) in &TABLE[i + 1..] {
                assert_ne!(verb, other_verb, "verb {name} is bound twice");
                assert_ne!(key, other_key, "key {key:?} speaks two verbs");
                assert_ne!(name, other_name, "name {name} is used twice");
            }
        }
        for (i, (verb, key, name)) in SHIFT_TABLE.iter().enumerate() {
            for (other_verb, other_key, other_name) in &SHIFT_TABLE[i + 1..] {
                assert_ne!(verb, other_verb, "shifted verb {name} is bound twice");
                assert_ne!(key, other_key, "shift+{key:?} speaks two verbs");
                assert_ne!(name, other_name, "shifted name {name} is used twice");
            }
        }
        for (verb, _, name) in TABLE {
            for (shifted_verb, _, shifted_name) in SHIFT_TABLE.iter().chain(COMMAND_TABLE) {
                assert_ne!(verb, shifted_verb, "verb {name} has two chords");
                assert_ne!(name, shifted_name, "verb name {name} is reused");
            }
        }
        for (shifted_verb, _, shifted_name) in SHIFT_TABLE {
            for (command_verb, _, command_name) in COMMAND_TABLE {
                assert_ne!(
                    shifted_verb, command_verb,
                    "verb {shifted_name} has two chords"
                );
                assert_ne!(
                    shifted_name, command_name,
                    "verb name {shifted_name} is reused"
                );
            }
        }
    }

    /// Keys the frame already speaks globally may not become verbs: Tab is
    /// focus travel, Space and O and F9/F10 belong to the transport, I to
    /// MIDI typing, Escape ends things. A verb on any of these would give
    /// one key two meanings.
    #[test]
    fn verbs_avoid_the_global_vocabulary() {
        use eframe::egui::Key;
        let reserved = [
            Key::Tab,
            Key::Space,
            Key::O,
            Key::I,
            Key::F9,
            Key::F10,
            Key::Escape,
            Key::Home,
        ];
        for (_, key, name) in TABLE {
            assert!(
                !reserved.contains(key),
                "verb {name} sits on the reserved key {key:?}"
            );
        }
    }
}
