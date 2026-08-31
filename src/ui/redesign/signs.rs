//! The redesign's closed sign vocabulary.
//!
//! One sign, one meaning, everywhere — a glyph used for PLAY in the
//! transport may never mean something else in another panel, and no second
//! glyph may also mean play. Panels take their glyphs from these constants
//! instead of writing string literals, so a new sign has to enter through
//! this file and face the uniqueness tests below.

/// Return the playhead to the start.
pub(crate) const RETURN: &str = "|<";
/// Start or resume playback.
pub(crate) const PLAY: &str = ">";
/// Pause in place.
pub(crate) const PAUSE: &str = "||";
/// Stop and return.
pub(crate) const STOP: &str = "[]";
/// Record arm.
pub(crate) const RECORD: &str = "O";
/// The view follows the playhead.
pub(crate) const FOLLOW: &str = ">>";
/// Metronome.
pub(crate) const METRONOME: &str = "^";
/// The arrangement loop.
pub(crate) const LOOP: &str = "<>";
/// Audio engine power.
pub(crate) const ENGINE: &str = "IO";
/// INTENT deviation, upward: the musician bent this pitch or pushed this
/// onset. Quiet ink, after the name.
pub(crate) const BEND_UP: &str = "⁺";
/// INTENT deviation, downward.
pub(crate) const BEND_DOWN: &str = "⁻";
/// PLAYBACK approximation: the bridge-era 12TET path cannot reproduce
/// this pitch exactly. The machine, not the musician — a different sign
/// class on purpose (`notes/20260831-pitch-lens-spec.md` §4).
pub(crate) const APPROX: &str = "≈";

/// Every sign in the system, paired with its single meaning. The tests
/// hold this table to a bijection; it is a registry, not runtime data,
/// so outside the tests nothing reads it yet.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const VOCABULARY: &[(&str, &str)] = &[
    (RETURN, "return to start"),
    (PLAY, "play"),
    (PAUSE, "pause"),
    (STOP, "stop"),
    (RECORD, "record arm"),
    (FOLLOW, "follow playhead"),
    (METRONOME, "metronome"),
    (LOOP, "arrangement loop"),
    (ENGINE, "engine power"),
    (BEND_UP, "intent deviation, up"),
    (BEND_DOWN, "intent deviation, down"),
    (APPROX, "playback approximation"),
];

#[cfg(test)]
mod tests {
    use super::VOCABULARY;

    /// No glyph carries two meanings.
    #[test]
    fn every_sign_has_one_meaning() {
        for (i, (glyph, _)) in VOCABULARY.iter().enumerate() {
            for (other, _) in &VOCABULARY[i + 1..] {
                assert_ne!(glyph, other, "the sign {glyph:?} is bound twice");
            }
        }
    }

    /// No meaning is carried by two glyphs.
    #[test]
    fn every_meaning_has_one_sign() {
        for (i, (_, meaning)) in VOCABULARY.iter().enumerate() {
            for (_, other) in &VOCABULARY[i + 1..] {
                assert_ne!(meaning, other, "the meaning {meaning:?} has two signs");
            }
        }
    }

    /// A sign is a mark, not a word: it must stay glanceable. Measured
    /// in characters — `≈` is one mark however many bytes it takes.
    #[test]
    fn signs_are_marks_not_words() {
        for (glyph, _) in VOCABULARY {
            assert!(
                !glyph.is_empty() && glyph.chars().count() <= 2,
                "the sign {glyph:?} is prose, not a mark"
            );
        }
    }
}
