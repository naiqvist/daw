//! Semantic arrangement edits shared by sentences, pointer gestures and the
//! command palette.

use super::state::Selection;
use crate::sequencing::{BlockId, Song, Track, TrackId, TrackKind};

/// The canonical placement returned by a successful clip move/copy/resize.
/// Pointer and sentence edits both consume this result, so selection follows
/// the exact edit that landed rather than reconstructing it independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Placement {
    pub(super) id: BlockId,
    pub(super) track: usize,
    pub(super) start_tick: usize,
    pub(super) length_ticks: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Command {
    CreateClip,
    DeleteClips,
    AddTrack,
    MoveTrackUp,
    MoveTrackDown,
}

impl Command {
    pub(super) const ALL: [Self; 5] = [
        Self::CreateClip,
        Self::DeleteClips,
        Self::AddTrack,
        Self::MoveTrackUp,
        Self::MoveTrackDown,
    ];

    pub(super) const fn title(self) -> &'static str {
        match self {
            Self::CreateClip => "CREATE EMPTY MIDI CLIP",
            Self::DeleteClips => "DELETE CLIPS IN SELECTION",
            Self::AddTrack => "ADD INSTRUMENT TRACK",
            Self::MoveTrackUp => "MOVE TRACK UP",
            Self::MoveTrackDown => "MOVE TRACK DOWN",
        }
    }

    pub(super) const fn hint(self) -> &'static str {
        match self {
            Self::CreateClip => "SEARCH / ACT",
            Self::DeleteClips => "DELETE",
            Self::AddTrack => "A NEW LANE BELOW",
            Self::MoveTrackUp => "THIS LANE, ONE UP",
            Self::MoveTrackDown => "THIS LANE, ONE DOWN",
        }
    }

    pub(super) fn enabled(self, song: &Song, selection: Selection) -> bool {
        match self {
            Self::CreateClip => can_create(song, selection),
            Self::DeleteClips => intersects_block(song, selection),
            Self::AddTrack => true,
            Self::MoveTrackUp => selection.first_track > 0,
            Self::MoveTrackDown => selection.first_track + 1 < song.tracks.len(),
        }
    }
}

/// Append a fresh instrument track. Shared by the palette command and
/// the frame-level Ctrl+T, so the id mint lives once.
pub(crate) fn add_track(song: &mut Song) -> &'static str {
    let id = TrackId(
        song.tracks
            .iter()
            .map(|track| track.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    );
    song.tracks.push(Track {
        id,
        name: format!("INSTRUMENT {:02}", song.tracks.len() + 1),
        kind: TrackKind::Instrument,
        blocks: Vec::new(),
        muted: false,
        solo: false,
        pitch_authority: crate::sequencing::PitchAuthority::default(),
        automation: Vec::new(),
        volume: 1.0,
        pan: 0.0,
    });
    "TRACK ADDED"
}

pub(super) fn apply(command: Command, song: &mut Song, selection: Selection) -> &'static str {
    match command {
        Command::CreateClip => {
            if !can_create(song, selection) {
                return "REGION OCCUPIED / MIDI TRACK REQUIRED";
            }
            let mut created = 0;
            for track in selection.first_track..=selection.last_track {
                if song.tracks.get(track).map(|track| &track.kind) == Some(&TrackKind::Instrument)
                    && song
                        .create_pattern_block(
                            track,
                            selection.start_tick(),
                            selection.end_tick() - selection.start_tick(),
                        )
                        .is_some()
                {
                    created += 1;
                }
            }
            if created == 1 {
                "CLIP CREATED"
            } else {
                "CLIPS CREATED"
            }
        }
        Command::AddTrack => add_track(song),
        Command::MoveTrackUp => {
            let track = selection.first_track;
            if track == 0 || track >= song.tracks.len() {
                return "TOP OF THE FRAME";
            }
            song.tracks.swap(track, track - 1);
            "TRACK MOVED UP"
        }
        Command::MoveTrackDown => {
            let track = selection.first_track;
            if track + 1 >= song.tracks.len() {
                return "BOTTOM OF THE FRAME";
            }
            song.tracks.swap(track, track + 1);
            "TRACK MOVED DOWN"
        }
        Command::DeleteClips => {
            let deleted = song.delete_blocks_in_region(
                selection.first_track,
                selection.last_track,
                selection.start_tick(),
                selection.end_tick(),
            );
            if deleted == 0 {
                "NO CLIP IN SELECTION"
            } else if deleted == 1 {
                "CLIP DELETED"
            } else {
                "CLIPS DELETED"
            }
        }
    }
}

pub(super) fn place_block(
    song: &mut Song,
    id: BlockId,
    target_track: usize,
    start_tick: usize,
    copy: bool,
) -> Result<Placement, &'static str> {
    let length_ticks = song
        .pattern_block(id)
        .map(|(_, block)| block.length_ticks)
        .ok_or("NO CLIP HERE")?;
    let placed = if copy {
        song.duplicate_pattern_block(id, target_track, start_tick)
    } else {
        song.move_pattern_block(id, target_track, start_tick)
            .then_some(id)
    }
    .ok_or("MOVE BLOCKED")?;
    Ok(Placement {
        id: placed,
        track: target_track,
        start_tick,
        length_ticks,
    })
}

pub(super) fn resize_block(
    song: &mut Song,
    id: BlockId,
    start_tick: usize,
    length_ticks: usize,
) -> Result<Placement, &'static str> {
    let (track, _) = song.pattern_block(id).ok_or("NO CLIP HERE")?;
    if !song.resize_pattern_block(id, start_tick, length_ticks) {
        return Err("RESIZE BLOCKED");
    }
    Ok(Placement {
        id,
        track,
        start_tick,
        length_ticks,
    })
}

fn can_create(song: &Song, selection: Selection) -> bool {
    let start = selection.start_tick();
    let end = selection.end_tick();
    let mut instrument_found = false;
    for track_index in selection.first_track..=selection.last_track {
        let Some(track) = song.tracks.get(track_index) else {
            return false;
        };
        if track.kind != TrackKind::Instrument {
            continue;
        }
        instrument_found = true;
        if track.blocks.iter().any(|block| {
            let block_end = block.start_tick.saturating_add(block.length_ticks);
            block.start_tick < end && start < block_end
        }) {
            return false;
        }
    }
    instrument_found
}

fn intersects_block(song: &Song, selection: Selection) -> bool {
    let start = selection.start_tick();
    let end = selection.end_tick();
    song.tracks
        .get(selection.first_track..=selection.last_track)
        .is_some_and(|tracks| {
            tracks.iter().any(|track| {
                track.blocks.iter().any(|block| {
                    let block_end = block.start_tick.saturating_add(block.length_ticks);
                    block.start_tick < end && start < block_end
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(first_beat: usize, end_beat: usize) -> Selection {
        Selection {
            first_track: 0,
            last_track: 0,
            first_beat,
            end_beat,
        }
    }

    #[test]
    fn palette_and_sentence_command_create_the_selected_duration() {
        let mut song = Song::default();
        let region = selection(16, 20);
        assert!(Command::CreateClip.enabled(&song, region));
        assert_eq!(
            apply(Command::CreateClip, &mut song, region),
            "CLIP CREATED"
        );
        let block = &song.tracks[0].blocks[1];
        assert_eq!(block.start_tick, 16 * crate::sequencing::TICKS_PER_BEAT);
        assert_eq!(block.length_ticks, 4 * crate::sequencing::TICKS_PER_BEAT);
    }

    #[test]
    fn delete_acts_on_every_clip_intersecting_the_selection() {
        let mut song = Song::default();
        assert_eq!(
            apply(Command::DeleteClips, &mut song, selection(3, 4)),
            "CLIP DELETED"
        );
        assert!(song.tracks[0].blocks.is_empty());
    }
}
