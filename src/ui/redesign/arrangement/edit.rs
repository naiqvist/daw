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
    AddAudioTrack,
    MoveTrackUp,
    MoveTrackDown,
}

impl Command {
    pub(super) const ALL: [Self; 6] = [
        Self::CreateClip,
        Self::DeleteClips,
        Self::AddTrack,
        Self::AddAudioTrack,
        Self::MoveTrackUp,
        Self::MoveTrackDown,
    ];

    pub(super) const fn title(self) -> &'static str {
        match self {
            Self::CreateClip => "CREATE EMPTY MIDI CLIP",
            Self::DeleteClips => "DELETE CLIPS IN SELECTION",
            Self::AddTrack => "ADD INSTRUMENT TRACK",
            Self::AddAudioTrack => "ADD AUDIO TRACK",
            Self::MoveTrackUp => "MOVE TRACK UP",
            Self::MoveTrackDown => "MOVE TRACK DOWN",
        }
    }

    pub(super) const fn hint(self) -> &'static str {
        match self {
            Self::CreateClip => "SEARCH / ACT",
            Self::DeleteClips => "DELETE",
            Self::AddTrack => "A NEW LANE BELOW",
            Self::AddAudioTrack => "A LANE FOR RECORDED SOUND",
            Self::MoveTrackUp => "THIS LANE, ONE UP",
            Self::MoveTrackDown => "THIS LANE, ONE DOWN",
        }
    }

    pub(super) fn enabled(self, song: &Song, selection: Selection) -> bool {
        match self {
            Self::CreateClip => can_create(song, selection),
            Self::DeleteClips => intersects_block(song, selection),
            Self::AddTrack | Self::AddAudioTrack => true,
            Self::MoveTrackUp => previous_sibling(&song.tracks, selection.first_track).is_some(),
            Self::MoveTrackDown => next_sibling(&song.tracks, selection.first_track).is_some(),
        }
    }
}

/// Append a fresh instrument track. Shared by the palette command and
/// the frame-level Ctrl+T, so the id mint lives once.
pub(crate) fn add_track(song: &mut Song) -> &'static str {
    add_track_of(song, TrackKind::Instrument)
}

/// Append a lane for recorded sound.
///
/// Without this there is no way to make an audio track at all, so
/// landing a sample could only ever refuse — the feature existed and was
/// unreachable.
pub(crate) fn add_audio_track(song: &mut Song) -> &'static str {
    add_track_of(song, TrackKind::Audio)
}

fn add_track_of(song: &mut Song, kind: TrackKind) -> &'static str {
    let id = TrackId(
        song.tracks
            .iter()
            .map(|track| track.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    );
    let number = song.tracks.len() + 1;
    let (name, notice) = match kind {
        TrackKind::Instrument => (format!("INSTRUMENT {number:02}"), "TRACK ADDED"),
        TrackKind::Audio => (format!("AUDIO {number:02}"), "AUDIO TRACK ADDED"),
    };
    song.tracks.push(Track {
        id,
        name,
        kind,
        blocks: Vec::new(),
        muted: false,
        solo: false,
        pitch_authority: crate::sequencing::PitchAuthority::default(),
        audio_blocks: Vec::new(),
        automation: Vec::new(),
        volume: 1.0,
        pan: 0.0,
        sends: Vec::new(),
        is_group: false,
        folded: false,
        chain: Vec::new(),
        strip: Vec::new(),
        bus: 0,
        bus_by_hand: false,
        depth: 0,
        input: crate::sequencing::TrackInput::default(),
        monitor: crate::sequencing::Monitor::default(),
        armed: false,
    });
    notice
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
        Command::AddAudioTrack => add_audio_track(song),
        Command::MoveTrackUp => {
            let track = selection.first_track;
            // Normalize HERE, on the canonical model, before either the
            // history or the projection can observe an illegal stack.
            song.normalize_group_depths();
            if !move_track_up(&mut song.tracks, track) {
                return "TRACK: NO SIBLING ABOVE";
            }
            "TRACK MOVED UP"
        }
        Command::MoveTrackDown => {
            let track = selection.first_track;
            song.normalize_group_depths();
            if !move_track_down(&mut song.tracks, track) {
                return "TRACK: NO SIBLING BELOW";
            }
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

/// The selected lane and every descendant it owns. On a plain lane this is
/// exactly one element; a normalized stack never puts a deeper lane below a
/// non-group.
fn track_run(tracks: &[Track], start: usize) -> Option<std::ops::Range<usize>> {
    let depth = tracks.get(start)?.depth;
    let mut end = start + 1;
    while tracks.get(end).is_some_and(|track| track.depth > depth) {
        end += 1;
    }
    Some(start..end)
}

/// The adjacent sibling above `start`, skipping that sibling's whole run.
fn previous_sibling(tracks: &[Track], start: usize) -> Option<usize> {
    let depth = tracks.get(start)?.depth;
    for index in (0..start).rev() {
        match tracks[index].depth.cmp(&depth) {
            std::cmp::Ordering::Equal => return Some(index),
            std::cmp::Ordering::Less => return None,
            std::cmp::Ordering::Greater => {}
        }
    }
    None
}

/// The adjacent sibling below `start`, after the selected lane's whole run.
fn next_sibling(tracks: &[Track], start: usize) -> Option<usize> {
    let run = track_run(tracks, start)?;
    tracks
        .get(run.end)
        .filter(|next| next.depth == tracks[start].depth)
        .map(|_| run.end)
}

fn move_track_up(tracks: &mut [Track], start: usize) -> bool {
    let Some(run) = track_run(tracks, start) else {
        return false;
    };
    let Some(previous) = previous_sibling(tracks, start) else {
        return false;
    };
    tracks[previous..run.end].rotate_left(start - previous);
    true
}

fn move_track_down(tracks: &mut [Track], start: usize) -> bool {
    let Some(run) = track_run(tracks, start) else {
        return false;
    };
    let Some(next) = next_sibling(tracks, start) else {
        return false;
    };
    let Some(next_run) = track_run(tracks, next) else {
        return false;
    };
    tracks[start..next_run.end].rotate_left(run.end - start);
    true
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

    fn track_selection(track: usize) -> Selection {
        Selection {
            first_track: track,
            last_track: track,
            first_beat: 0,
            end_beat: 1,
        }
    }

    fn lane(id: u64, name: &str, depth: u8, is_group: bool) -> Track {
        let mut track = Song::default().tracks.remove(0);
        track.id = TrackId(id);
        track.name = name.to_owned();
        track.blocks.clear();
        track.depth = depth;
        track.is_group = is_group;
        track
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

    #[test]
    fn moving_a_group_down_carries_its_whole_run() {
        let mut song = Song::default();
        song.tracks = vec![
            lane(1, "GROUP", 0, true),
            lane(2, "KICK", 1, false),
            lane(3, "HATS", 1, false),
            lane(4, "BASS", 0, false),
        ];

        assert_eq!(
            apply(Command::MoveTrackDown, &mut song, track_selection(0)),
            "TRACK MOVED DOWN"
        );
        let names: Vec<&str> = song
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(names, ["BASS", "GROUP", "KICK", "HATS"]);
        assert_eq!(
            song.tracks
                .iter()
                .map(|track| track.depth)
                .collect::<Vec<_>>(),
            [0, 0, 1, 1]
        );
    }

    #[test]
    fn moving_a_plain_lane_up_skips_the_whole_group_above() {
        let mut song = Song::default();
        song.tracks = vec![
            lane(1, "GROUP", 0, true),
            lane(2, "KICK", 1, false),
            lane(3, "HATS", 1, false),
            lane(4, "BASS", 0, false),
        ];

        assert_eq!(
            apply(Command::MoveTrackUp, &mut song, track_selection(3)),
            "TRACK MOVED UP"
        );
        let names: Vec<&str> = song
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(names, ["BASS", "GROUP", "KICK", "HATS"]);
        assert_eq!(song.tracks[2].depth, 1, "KICK remains a group member");
    }

    #[test]
    fn movement_stays_among_siblings_and_refuses_at_a_group_edge() {
        let mut song = Song::default();
        song.tracks = vec![
            lane(1, "GROUP", 0, true),
            lane(2, "A", 1, false),
            lane(3, "B", 1, true),
            lane(4, "B CHILD", 2, false),
            lane(5, "C", 1, false),
        ];

        assert_eq!(
            apply(Command::MoveTrackUp, &mut song, track_selection(4)),
            "TRACK MOVED UP"
        );
        let names: Vec<&str> = song
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(names, ["GROUP", "A", "C", "B", "B CHILD"]);
        assert_eq!(
            apply(Command::MoveTrackUp, &mut song, track_selection(1)),
            "TRACK: NO SIBLING ABOVE"
        );
    }

    #[test]
    fn reorder_repairs_depth_on_the_song_before_it_moves() {
        let mut song = Song::default();
        song.tracks = vec![lane(1, "A", 7, false), lane(2, "B", 7, false)];

        assert_eq!(
            apply(Command::MoveTrackUp, &mut song, track_selection(1)),
            "TRACK MOVED UP"
        );
        assert!(song.tracks.iter().all(|track| track.depth == 0));
        assert_eq!(song.tracks[0].name, "B");
    }
}
