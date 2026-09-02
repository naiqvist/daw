//! Typed registers: the grammar's clipboard.
//!
//! Yank puts a noun here; put places it back at the cursor. A register
//! remembers what KIND it holds, and putting a kind where it cannot go is
//! a spoken refusal, never a coercion (`notes/20260831-command-grammar.md`).
//! Registers are the general cross-panel fallback when no held key names
//! the far end: yank here, travel, put there.
//!
//! One default register today; named registers join when the sentence
//! machinery grows a register prefix.

/// One note of a yanked trig, stored relative to its step so it can land
/// on any tick. The pitch travels as its stored address — a yanked
/// degree stays a degree and reflows under the key at its destination.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TrigNote {
    pub(crate) pitch: crate::pitch::Pitch,
    pub(crate) length_ticks: usize,
    pub(crate) velocity: u8,
    pub(crate) probability: f32,
    pub(crate) enabled: bool,
    pub(crate) muted: bool,
}

/// A yanked arrangement clip: the pattern's content travels WITH the
/// clip (a deep copy, Elektron style), so a put never aliases the
/// original — editing the copy cannot reach back.
// Constructed by the arrangement's yank once it plumbs the Voice through;
// the contract and its tests land first so the wiring is a small step.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClipPayload {
    pub(crate) pattern: crate::sequencing::Pattern,
    pub(crate) length_ticks: usize,
}

/// What a register can hold. Every variant names its kind for refusals.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Payload {
    /// One pitch-addressed note from a single-note editor.
    Note(TrigNote),
    /// Every note that shared one step: a whole trig, locks and all.
    Trig(Vec<TrigNote>),
    #[cfg_attr(not(test), allow(dead_code))]
    Clip(ClipPayload),
}

impl Payload {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Payload::Note(_) => "A NOTE",
            Payload::Trig(_) => "A TRIG",
            Payload::Clip(_) => "A CLIP",
        }
    }
}

#[derive(Default)]
pub(crate) struct Registers {
    default: Option<Payload>,
}

impl Registers {
    pub(crate) fn yank(&mut self, payload: Payload) {
        self.default = Some(payload);
    }

    /// The register's content, if it holds one note.
    pub(crate) fn note(&self) -> Result<&TrigNote, String> {
        match &self.default {
            None => Err("PUT: NOTHING YANKED".to_owned()),
            Some(Payload::Note(note)) => Ok(note),
            Some(other) => Err(format!("PUT: {} DOES NOT GO HERE", other.kind())),
        }
    }

    /// The register's content, if it holds a trig. On a kind mismatch the
    /// caller receives the refusal to speak; the register keeps its
    /// content either way — a refused put must not destroy the yank.
    pub(crate) fn trig(&self) -> Result<&[TrigNote], String> {
        match &self.default {
            None => Err("PUT: NOTHING YANKED".to_owned()),
            Some(Payload::Trig(notes)) => Ok(notes),
            Some(other) => Err(format!("PUT: {} DOES NOT GO HERE", other.kind())),
        }
    }

    /// The register's content, if it holds a clip. Same refusal contract
    /// as [`Registers::trig`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn clip(&self) -> Result<&ClipPayload, String> {
        match &self.default {
            None => Err("PUT: NOTHING YANKED".to_owned()),
            Some(Payload::Clip(clip)) => Ok(clip),
            Some(other) => Err(format!("PUT: {} DOES NOT GO HERE", other.kind())),
        }
    }

    /// The carried material as a short sign, for the frame to display —
    /// the material context made visible, per the composition contract's
    /// rule that a pronoun's referent must be checkable. `T3` is a trig
    /// of three notes; `C` a clip.
    pub(crate) fn carried_sign(&self) -> Option<String> {
        match &self.default {
            None => None,
            Some(Payload::Note(_)) => Some("N".to_owned()),
            Some(Payload::Trig(notes)) => Some(format!("T{}", notes.len())),
            Some(Payload::Clip(_)) => Some("C".to_owned()),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.default.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_trig() -> Payload {
        Payload::Trig(vec![TrigNote {
            pitch: crate::pitch::Pitch::from_midi(60),
            length_ticks: 12,
            velocity: 100,
            probability: 1.0,
            enabled: true,
            muted: false,
        }])
    }

    fn a_note() -> TrigNote {
        TrigNote {
            pitch: crate::pitch::Pitch::from_midi(64),
            length_ticks: 6,
            velocity: 90,
            probability: 0.75,
            enabled: true,
            muted: false,
        }
    }

    #[test]
    fn yank_then_put_round_trips() {
        let mut registers = Registers::default();
        registers.yank(a_trig());
        assert_eq!(registers.trig().expect("holds a trig").len(), 1);
    }

    #[test]
    fn a_note_and_a_trig_are_distinct_register_kinds() {
        let mut registers = Registers::default();
        registers.yank(Payload::Note(a_note()));
        assert_eq!(registers.note(), Ok(&a_note()));
        assert_eq!(
            registers.trig().unwrap_err(),
            "PUT: A NOTE DOES NOT GO HERE"
        );
        assert_eq!(registers.carried_sign().as_deref(), Some("N"));

        registers.yank(a_trig());
        assert_eq!(
            registers.note().unwrap_err(),
            "PUT: A TRIG DOES NOT GO HERE"
        );
    }

    #[test]
    fn putting_from_an_empty_register_is_refused_out_loud() {
        let registers = Registers::default();
        assert_eq!(
            registers.trig().unwrap_err(),
            "PUT: NOTHING YANKED",
            "silence is forbidden"
        );
    }

    #[test]
    fn a_refused_put_keeps_the_yank() {
        let mut registers = Registers::default();
        registers.yank(a_trig());
        let _ = registers.trig();
        assert!(!registers.is_empty());
    }

    /// The typed-register contract across kinds: a clip in the register
    /// refuses a trig put by NAMING what it holds, and vice versa — the
    /// register never coerces, and a refused put never destroys the yank.
    #[test]
    fn kinds_refuse_each_other_by_name() {
        let mut registers = Registers::default();
        registers.yank(Payload::Clip(ClipPayload {
            pattern: crate::sequencing::Pattern::default(),
            length_ticks: 48,
        }));
        assert_eq!(
            registers.trig().unwrap_err(),
            "PUT: A CLIP DOES NOT GO HERE"
        );
        assert!(registers.clip().is_ok());

        registers.yank(Payload::Note(a_note()));
        assert_eq!(
            registers.clip().unwrap_err(),
            "PUT: A NOTE DOES NOT GO HERE"
        );

        registers.yank(a_trig());
        assert_eq!(
            registers.clip().unwrap_err(),
            "PUT: A TRIG DOES NOT GO HERE"
        );
        assert!(registers.trig().is_ok());
    }
}
