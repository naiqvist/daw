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
    Delete,
    Yank,
    Put,
    Duplicate,
    /// Move by the current grid unit. Takes a motion.
    Nudge,
    /// Grow or shrink by the current grid unit. Takes a motion.
    Resize,
    Mute,
    Solo,
    Rename,
    /// Edit a trig's condition sign.
    Condition,
    /// Fuzzy name-search within the noun's world.
    Search,
}

/// The whole vocabulary: verb, key, display name. Tests hold every column
/// to uniqueness. In MIDI mode the letter keys are pitches instead —
/// `midi_typing` consumes them first, which is the announced modal trade.
pub(crate) const TABLE: &[(Verb, egui::Key, &str)] = &[
    (Verb::Act, egui::Key::Enter, "ACT"),
    (Verb::Delete, egui::Key::Delete, "DELETE"),
    // Verbs spoken mid-navigation sit under the LEFT hand, because the
    // right hand owns the arrows: reach beats mnemonics. Q/W/E form the
    // move-and-copy cluster (nudge lives on W below).
    (Verb::Yank, egui::Key::Q, "YANK"),
    (Verb::Put, egui::Key::E, "PUT"),
    (Verb::Duplicate, egui::Key::D, "DUPLICATE"),
    (Verb::Nudge, egui::Key::W, "NUDGE"),
    (Verb::Resize, egui::Key::R, "RESIZE"),
    (Verb::Mute, egui::Key::M, "MUTE"),
    (Verb::Solo, egui::Key::S, "SOLO"),
    (Verb::Rename, egui::Key::F2, "RENAME"),
    (Verb::Condition, egui::Key::C, "CONDITION"),
    (Verb::Search, egui::Key::Slash, "SEARCH"),
];

impl Verb {
    pub(crate) fn name(self) -> &'static str {
        TABLE
            .iter()
            .find(|(verb, _, _)| *verb == self)
            .map(|(_, _, name)| *name)
            .unwrap_or("?")
    }

    /// A motion verb waits for its direction; the rest act on the spot.
    pub(crate) fn needs_motion(self) -> bool {
        matches!(self, Verb::Nudge | Verb::Resize)
    }
}

#[cfg(test)]
mod tests {
    use super::TABLE;

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
