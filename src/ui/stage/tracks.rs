//! The track strip — the first marks on the stage that carry model meaning.
//!
//! Tracks run ACROSS: one column each, separated horizontally, each owning
//! the vertical axis beneath it. That is the session shape. The
//! arrangement shape — a track per ROW, time running rightward — is a
//! different surface and will be built as one; nothing here decides for
//! it, and nothing here may be reused to avoid deciding.
//!
//! A head carries IDENTITY and nothing else: what a track IS. State
//! (muted, soloed, armed, its level, what it is playing) composes onto
//! identity later as additional marks. The rule that governs that addition
//! is that a track stays recognisably itself while its state changes — so
//! state is never allowed to repaint the head, only to add to it.

use crate::sequencing::{Song, Track, TrackKind};

/// One track reduced to exactly what the strip draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Head {
    pub name: String,
    /// The kind, as a WORD.
    ///
    /// A glyph here would have to be denser than the word AND enter the
    /// closed vocabulary in `crate::design::signs` to face the bijection
    /// tests. Three kinds do not earn that: `INST` is already short,
    /// already unambiguous, and costs no learning. The compression test is
    /// a test rather than a preference, and a mark for this would fail it.
    pub kind: &'static str,
}

/// Group-ness outranks kind: a group's own `TrackKind` is an artefact of
/// how it was made, and reporting it would name the track something the
/// musician cannot act on.
fn kind_of(track: &Track) -> &'static str {
    if track.is_group {
        return "Group";
    }
    match track.kind {
        TrackKind::Instrument => "Inst",
        TrackKind::Audio => "Audio",
    }
}

/// The heads the strip draws, in the song's own track order.
///
/// Order is the model's, never sorted here: position IS the address the
/// keyboard uses, so a strip that reordered its own columns would move the
/// ground under the cursor.
pub fn heads(song: &Song) -> Vec<Head> {
    song.tracks
        .iter()
        .map(|track| Head {
            name: track.name.clone(),
            kind: kind_of(track),
        })
        .collect()
}

/// How far a view has scrolled, as the index of the first item drawn.
/// Written for the strip; the scene lattice scrolls its rows by the same
/// rule, because the rule is about the cursor and not about tracks.
///
/// Minimal by rule. The strip does not move while the cursor is inside
/// what it already shows, and when the cursor steps outside it moves by
/// exactly enough to bring it back — never to recentre. A view that
/// recentred would move the ground under a performer who was only
/// stepping one track sideways, and the cursor is meant to read as a
/// state change rather than as motion.
///
/// It also never shows empty space past the last track: an offset that
/// would leave a gap on the right is pulled back to the last full view.
pub fn offset_following(
    previous: usize,
    cursor: Option<usize>,
    count: usize,
    capacity: usize,
) -> usize {
    // A strip too narrow for one column still shows one, or the cursor
    // would have nowhere to be.
    let capacity = capacity.max(1);
    let furthest = count.saturating_sub(capacity);
    let mut offset = previous.min(furthest);
    if let Some(cursor) = cursor {
        if cursor < offset {
            offset = cursor;
        } else if cursor >= offset.saturating_add(capacity) {
            offset = cursor.saturating_add(1).saturating_sub(capacity);
        }
    }
    offset.min(furthest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_does_not_move_while_the_cursor_is_already_shown() {
        for cursor in 2..=5 {
            assert_eq!(
                offset_following(2, Some(cursor), 10, 4),
                2,
                "the ground moved under a cursor that was already visible"
            );
        }
    }

    #[test]
    fn stepping_off_an_end_moves_by_exactly_one() {
        // Showing 2..=5 of ten. Stepping to 6 reveals 3..=6, not a recentre.
        assert_eq!(offset_following(2, Some(6), 10, 4), 3);
        // And back the other way.
        assert_eq!(offset_following(3, Some(2), 10, 4), 2);
    }

    #[test]
    fn a_far_jump_brings_the_cursor_to_the_near_edge() {
        assert_eq!(offset_following(0, Some(9), 10, 4), 6);
        assert_eq!(offset_following(6, Some(0), 10, 4), 0);
    }

    #[test]
    fn the_strip_never_shows_empty_space_past_the_last_track() {
        assert_eq!(offset_following(8, Some(9), 10, 4), 6);
        assert_eq!(
            offset_following(5, None, 3, 4),
            0,
            "a strip wider than the song still scrolled"
        );
    }

    #[test]
    fn a_strip_too_narrow_for_a_column_still_shows_the_cursor() {
        assert_eq!(offset_following(0, Some(7), 10, 0), 7);
    }

    #[test]
    fn losing_tracks_pulls_the_view_back_into_the_song() {
        assert_eq!(
            offset_following(7, None, 4, 4),
            0,
            "the view stayed past the end of a shortened song"
        );
    }
    /// Built from the default song's own track so this helper cannot rot
    /// as the model grows fields: only what the strip reads is set here.
    fn track(name: &str, kind: TrackKind, is_group: bool) -> Track {
        let base = Song::default()
            .tracks
            .into_iter()
            .next()
            .expect("the default song has a track");
        Track {
            name: name.to_owned(),
            kind,
            letter: String::new(),
            tags_minted: 0,
            is_group,
            ..base
        }
    }

    #[test]
    fn heads_follow_the_songs_own_track_order() {
        let song = Song {
            tracks: vec![
                track("DRUMS", TrackKind::Instrument, false),
                track("VOX", TrackKind::Audio, false),
            ],
            ..Song::default()
        };
        let heads = heads(&song);
        assert_eq!(
            heads.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
            ["DRUMS", "VOX"],
            "the strip reordered the song"
        );
    }

    #[test]
    fn each_kind_reads_as_itself() {
        let song = Song {
            tracks: vec![
                track("A", TrackKind::Instrument, false),
                track("B", TrackKind::Audio, false),
            ],
            ..Song::default()
        };
        let heads = heads(&song);
        assert_eq!(heads[0].kind, "Inst");
        assert_eq!(heads[1].kind, "Audio");
    }

    #[test]
    fn a_group_reads_as_a_group_whatever_kind_built_it() {
        let song = Song {
            tracks: vec![
                track("BUS", TrackKind::Instrument, true),
                track("BUS2", TrackKind::Audio, true),
            ],
            ..Song::default()
        };
        for head in heads(&song) {
            assert_eq!(head.kind, "Group");
        }
    }

    #[test]
    fn a_song_with_no_tracks_draws_no_heads() {
        let song = Song {
            tracks: Vec::new(),
            ..Song::default()
        };
        assert!(heads(&song).is_empty());
    }
}
