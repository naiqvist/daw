//! Adapters between the live project and the replacement Session view.
//!
//! `ui::session_next` was written deliberately knowing nothing about
//! `main.rs`, the engine, or the current Session. This file is the seam
//! the handoff calls for, and it is the ONLY place the two vocabularies
//! meet — everything either side stays in its own language.
//!
//! # Who owns what
//!
//! - **The arrangement owns clip CONTENT.** Notes, audio sources, names
//!   and lengths stay in `arrangement.session.slots`, so every other part
//!   of the app — the piano roll, the audio editor, save and load — keeps
//!   working on the clips it already knows.
//! - **The document owns launch RULES.** Per-clip mode, quantization,
//!   Fill, conditions and follow actions, plus scene tempo, locks and the
//!   two performance memories. The project schema has nowhere to put
//!   these yet, so they live in the app for now and are not saved. That
//!   is the first thing the next merge step owes.
//! - **The runtime owns PLAYBACK state.** The planner decides what is
//!   queued and what is playing; [`mirror_playback`] then writes the
//!   result into the arrangement's existing `playing`/`launch_at` pair,
//!   which is the whole of how session audio already reaches the engine.
//!
//! # Why the audio still goes the old way
//!
//! The handoff's third adapter — compiling a `PendingLaunch` straight
//! into the schedule swap — is NOT done here. The planner decides, and
//! the existing compile path plays. That keeps one authority for what
//! sounds while the new one proves itself, and it keeps the red zone
//! untouched: nothing in this file or in `session_next` is reachable from
//! the audio callback.

use crate::{Arrangement, Clip, Transport};
use daw::ui::device::meter::Ballistics;
use daw::ui::session_next as sx;
use daw::ui::theme::Theme;

/// Adapter 5: the house theme, by semantic role.
///
/// The module ships a default palette so it can compile without the live
/// UI hierarchy. This replaces it — the handoff is explicit that a second
/// authored palette must not survive the merge.
pub fn colors(theme: &Theme) -> sx::SessionColors {
    sx::SessionColors {
        bg: theme.bg,
        surface: theme.surface,
        raised: theme.surface_raised,
        sunken: theme.surface_sunken,
        text: theme.text,
        muted: theme.text_muted,
        divider: theme.divider,
        outline: theme.outline,
        focus: theme.focus,
        accent: theme.accent,
        accent_dim: theme.accent_muted,
        midi: theme.clip_midi,
        audio: theme.clip_audio,
        selected: theme.clip_selected,
        ok: theme.ok,
        warn: theme.warn,
        danger: theme.danger,
        role_time: theme.role_time,
        role_level: theme.role_level,
        role_shape: theme.role_shape,
        role_mod: theme.role_mod,
        meter_low: theme.meter_low,
        meter_hot: theme.meter_hot,
        meter_clip: theme.meter_clip,
    }
}

/// The transport, as the view's clock.
pub fn clock(transport: &Transport, sample_rate: u32) -> sx::TransportClock {
    sx::TransportClock {
        sample: (transport.position * f64::from(sample_rate.max(1))).max(0.0) as u64,
        sample_rate: sample_rate.max(1),
        bpm: transport.bpm,
        beats_per_bar: transport.beats_per_bar.max(1),
    }
}

fn kind_of(kind: crate::TrackKind) -> sx::TrackKind {
    match kind {
        crate::TrackKind::Audio => sx::TrackKind::Audio,
        crate::TrackKind::Midi => sx::TrackKind::Midi,
    }
}

/// How many preview marks a slot draws. Small on purpose: the view is
/// drawing a thumbnail a few dozen pixels wide, and the real note list
/// belongs to the piano roll.
const PREVIEW_MARKS: usize = 48;

/// A clip's contour, normalized, for the slot thumbnail.
///
/// Built from the clip itself rather than handed the peak cache, because
/// a session slot is too small for a waveform to say anything a level
/// contour does not — and because a slot must draw before its analysis
/// has landed.
fn preview(clip: &Clip) -> sx::ClipPreview {
    if clip.audio.is_some() {
        // Without the peak pyramid to hand this is a placeholder shape,
        // not a lie about the audio: a flat band the same height at every
        // column, which reads as "audio, length known, detail pending".
        // The waveform belongs here once the cache is threaded through.
        return sx::ClipPreview::Waveform(vec![[-0.6, 0.6]; 8]);
    }
    if clip.notes.is_empty() || clip.len <= 0.0 {
        return sx::ClipPreview::None;
    }
    let span = f64::from(clip.len);
    let (low, high) = clip.notes.iter().fold((127u8, 0u8), |(low, high), note| {
        (low.min(note.pitch), high.max(note.pitch))
    });
    let range = f32::from(high.saturating_sub(low)).max(1.0);
    let marks = clip
        .notes
        .iter()
        .take(PREVIEW_MARKS)
        .map(|note| {
            let start = (note.start / span).clamp(0.0, 1.0) as f32;
            let length = (note.len / span).clamp(0.0, 1.0) as f32;
            let pitch = f32::from(note.pitch.saturating_sub(low)) / range;
            [start, length, pitch]
        })
        .collect();
    sx::ClipPreview::Notes(marks)
}

/// Adapter 1: project content into the document.
///
/// The document is NOT rebuilt from scratch each frame, and that is the
/// whole point of this function. Rebuilding would throw away every launch
/// rule the user has authored, because the project has nowhere to keep
/// them. So the shape is reconciled, the content is refreshed, and
/// anything the arrangement does not know about is left alone.
///
/// Rules follow the CLIP ID, not the slot position: moving a clip to
/// another slot takes its mode, condition and follow action with it,
/// which is what anyone who set them would expect.
pub fn sync_document(document: &mut sx::SessionDocument, arrangement: &Arrangement) {
    let tracks = arrangement.tracks.len();
    let scenes = arrangement.session.scenes.len().max(1);

    // Remember every authored rule before the shape changes under it.
    let mut authored: std::collections::HashMap<u64, sx::LaunchSettings> =
        std::collections::HashMap::new();
    for row in &document.slots {
        for slot in row {
            if let sx::Slot::Clip(clip) = slot {
                authored.insert(clip.id.0, clip.launch.clone());
            }
        }
    }

    // Tracks: keep the ids of the ones that are still there, mint for the
    // rest. A track's id is its position for now — the project has no
    // track identity to preserve, which is noted in the handoff as the
    // debt this merge takes on.
    document.tracks.truncate(tracks);
    while document.tracks.len() < tracks {
        let index = document.tracks.len();
        document.tracks.push(sx::SessionTrack {
            id: sx::TrackId(index as u64 + 1),
            ..sx::SessionTrack::default()
        });
    }
    for (index, track) in arrangement.tracks.iter().enumerate() {
        let Some(target) = document.tracks.get_mut(index) else {
            continue;
        };
        target.name = track.name.clone();
        target.kind = kind_of(track.kind);
        target.mute = track.mute;
        target.solo = track.solo;
        target.pan = track.pan;
        target.volume = track.volume;
        target.is_group = track.is_group;
        target.folded = track.folded;
        target.depth = track.depth;
        target.input = track.input.label();
        target.monitoring = track.monitor.hears(track.armed);
        target.monitor = track.monitor.label().to_owned();
        target.armed = track.armed;
        target.sends.clone_from(&track.sends);
        // A send list is allowed to be SHORT — that is how a track says
        // it has never been asked about a return — but never long, or a
        // strip would draw a row pointing at a bus that is not there.
        target.sends.truncate(arrangement.returns.len());
    }

    // Returns: the project owns all of it, so this is a straight copy
    // rather than a reconcile. Nothing about a return is authored in the
    // session view, which is what makes that safe.
    document.returns.clear();
    document
        .returns
        .extend(arrangement.returns.iter().map(|bus| sx::SessionReturn {
            name: bus.name.clone(),
            mute: bus.mute,
            volume: bus.volume,
            pan: bus.pan,
        }));

    // Scenes: names come from the project, everything else is authored
    // here and survives.
    document.scenes.truncate(scenes);
    while document.scenes.len() < scenes {
        let index = document.scenes.len();
        document.scenes.push(sx::Scene {
            id: sx::SceneId(index as u64 + 1),
            name: format!("Scene {}", index + 1),
            ..sx::Scene::default()
        });
    }
    for (index, scene) in arrangement.session.scenes.iter().enumerate() {
        if let Some(target) = document.scenes.get_mut(index) {
            target.name = scene.name.clone();
        }
    }

    // Slots: content from the project, rules from what was authored.
    document.slots.resize(tracks, Vec::new());
    for (track, column) in document.slots.iter_mut().enumerate() {
        column.resize(scenes, sx::Slot::Empty(sx::EmptyBehavior::Stop));
        for (scene, slot) in column.iter_mut().enumerate() {
            let source = arrangement
                .session
                .slots
                .get(track)
                .and_then(|row| row.get(scene))
                .and_then(|slot| slot.as_ref());
            match source {
                Some(clip) => {
                    let kind = arrangement
                        .tracks
                        .get(track)
                        .map_or(sx::TrackKind::Midi, |track| kind_of(track.kind));
                    let launch = authored.get(&clip.id).cloned().unwrap_or_default();
                    *slot = sx::Slot::Clip(sx::SessionClip {
                        id: sx::ClipId(clip.id),
                        name: clip.name.clone(),
                        kind,
                        length_beats: clip.len.max(0.0),
                        loop_start_beats: 0.0,
                        loop_length_beats: clip.len.max(0.0),
                        active: true,
                        // An audio clip whose file has gone is a real
                        // state the view already draws; saying so here is
                        // cheaper than letting it launch into silence.
                        media_offline: clip
                            .audio
                            .as_ref()
                            .is_some_and(|audio| !audio.path.exists()),
                        launch,
                        preview: preview(clip),
                    });
                }
                // An empty slot that the user has authored as Continue
                // keeps that; anything else is a Stop, which is what the
                // current app's empty slots already mean.
                None => {
                    if !matches!(slot, sx::Slot::Empty(sx::EmptyBehavior::Continue)) {
                        *slot = sx::Slot::Empty(sx::EmptyBehavior::Stop);
                    }
                }
            }
        }
    }
    document.sanitize();
}

/// Adapter 2: telemetry into the runtime.
///
/// Only the parts the ENGINE knows: levels, clip hold, and the loop phase
/// of anything playing. Playback and queue state belong to the planner
/// and are deliberately not touched here — overwriting them from the
/// project every frame would make a queued launch flicker back to idle
/// between the press and the boundary.
pub fn sync_runtime(
    runtime: &mut sx::SessionRuntime,
    arrangement: &Arrangement,
    meters: &[Ballistics],
    clock: sx::TransportClock,
    playing: bool,
) {
    runtime.sanitize(arrangement.tracks.len());
    runtime
        .returns
        .resize(arrangement.returns.len(), sx::TrackRuntime::default());
    let beats_per_sample = clock.beats_per_sample();
    for (index, track) in runtime.tracks.iter_mut().enumerate() {
        if let Some(meter) = meters.get(index) {
            // dBFS to a 0..1 bar the way a meter reads, floored where the
            // view stops drawing rather than at silence.
            runtime_level(track, meter);
        }
        let sx::PlaybackState::Playing {
            started_at_sample,
            clip,
            ..
        } = track.playback
        else {
            track.phase = 0.0;
            continue;
        };
        if !playing {
            continue;
        }
        // PHASE COMES FROM THE TRANSPORT, never from wall time — the
        // visual spec is explicit, and a strip advancing while the
        // transport is paused would be a lie the user acts on.
        let length = clip_length_beats(arrangement, clip).unwrap_or(0.0);
        if length <= 0.0 {
            track.phase = 0.0;
            continue;
        }
        let elapsed = clock.sample.saturating_sub(started_at_sample) as f64 * beats_per_sample;
        track.phase = ((elapsed / f64::from(length)).fract().max(0.0)) as f32;
    }
}

/// The return meters, on their own list because they read their own
/// slots — the graph hands returns meter slots downwards from under the
/// master's, and a lane's list would have to be indexed backwards to
/// find them.
pub fn sync_return_levels(runtime: &mut sx::SessionRuntime, meters: &[Ballistics]) {
    for (bus, meter) in runtime.returns.iter_mut().zip(meters) {
        runtime_level(bus, meter);
    }
}

fn runtime_level(track: &mut sx::TrackRuntime, meter: &Ballistics) {
    const FLOOR_DB: f32 = -60.0;
    track.peak = ((meter.shown_db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    track.clipped = meter.clipped;
}

fn clip_length_beats(arrangement: &Arrangement, clip: sx::ClipId) -> Option<f32> {
    arrangement
        .session
        .slots
        .iter()
        .flatten()
        .flatten()
        .find(|candidate| candidate.id == clip.0)
        .map(|candidate| candidate.len)
}

/// Adapter 3 (partial): the planner's decision, expressed in the terms
/// the existing compile path already understands.
///
/// `playing[track]` plus `launch_at[track]` is the whole of how session
/// audio reaches the engine today: a launched clip is repeated into
/// ordinary timeline clips from that beat. So a QUEUED start is written
/// here as "playing, starting at the future beat" — the schedule is built
/// once, with silence up to the boundary, and no swap happens when the
/// boundary actually arrives.
///
/// Returns whether anything changed, which is what the caller turns into
/// a recompile.
pub fn mirror_playback(
    runtime: &sx::SessionRuntime,
    document: &sx::SessionDocument,
    arrangement: &mut Arrangement,
    clock: sx::TransportClock,
) -> bool {
    let tracks = arrangement.tracks.len();
    arrangement.session.playing.resize(tracks, None);
    arrangement.session.launch_at.resize(tracks, 0.0);
    let per_sample = clock.beats_per_sample();
    let mut changed = false;

    for track in 0..tracks {
        let state = runtime.tracks.get(track);
        // A pending START outranks what is playing: the audio for it has
        // to be compiled BEFORE its boundary, or the boundary passes in
        // silence while the schedule is still being built.
        let queued =
            state
                .and_then(|state| state.pending.as_ref())
                .and_then(|pending| match pending.action {
                    sx::PendingTrackAction::Start { scene, .. } => Some((scene, pending.at_sample)),
                    _ => None,
                });
        let want = match (queued, state.map(|state| state.playback)) {
            (Some((scene, at)), _) => document
                .scene_index(scene)
                .map(|scene| (scene, (at as f64 * per_sample) as f32)),
            (
                None,
                Some(sx::PlaybackState::Playing {
                    scene,
                    started_at_sample,
                    ..
                }),
            ) => document
                .scene_index(scene)
                .map(|scene| (scene, (started_at_sample as f64 * per_sample) as f32)),
            _ => None,
        };
        let (scene, at) = match want {
            Some((scene, at)) => (Some(scene), at),
            None => (None, 0.0),
        };
        if arrangement.session.playing[track] != scene {
            arrangement.session.playing[track] = scene;
            changed = true;
        }
        // Compared with a tolerance of one sample's worth of beat: the
        // conversion runs every frame and an exact float compare would
        // recompile forever on the last bit.
        if (arrangement.session.launch_at[track] - at).abs() > 1e-6 {
            arrangement.session.launch_at[track] = at;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{Note, TrackKind};

    fn arrangement_with(tracks: &[TrackKind]) -> Arrangement {
        let mut arrangement = Arrangement::default();
        arrangement.tracks.clear();
        arrangement.clips.clear();
        for (index, kind) in tracks.iter().enumerate() {
            arrangement
                .tracks
                .push(crate::Track::new(*kind, format!("T{index}")));
            arrangement.clips.push(Vec::new());
        }
        arrangement.session = crate::Session::new(tracks.len());
        arrangement
    }

    fn midi_clip(id: u64, len: f32) -> Clip {
        Clip {
            id,
            name: format!("clip {id}"),
            start: 0.0,
            len,
            notes: vec![Note {
                pitch: 60,
                start: 0.0,
                len: 1.0,
                vel: 100,
                muted: false,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            audio: None,
            ..Clip::default()
        }
    }

    /// THE SHAPE FOLLOWS THE PROJECT and the rules survive it.
    ///
    /// The project has nowhere to store a launch mode, so the document
    /// keeps them — which only works if syncing the project in does not
    /// flatten them back to default every frame. Sixty frames a second is
    /// a fast way to lose an authored performance.
    #[test]
    fn syncing_keeps_authored_launch_rules() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi, TrackKind::Audio]);
        arrangement.session.slots[0][0] = Some(midi_clip(7, 4.0));
        let mut document = sx::SessionDocument::default();

        sync_document(&mut document, &arrangement);
        assert_eq!(document.tracks.len(), 2);
        assert_eq!(document.slots.len(), 2);
        assert_eq!(document.tracks[1].kind, sx::TrackKind::Audio);

        // Author a rule on the clip, then sync again.
        let sx::Slot::Clip(clip) = &mut document.slots[0][0] else {
            panic!("a clip landed in the slot");
        };
        clip.launch.mode = sx::LaunchMode::Toggle;
        clip.launch.fill = sx::FillRule::Only;
        sync_document(&mut document, &arrangement);

        let sx::Slot::Clip(clip) = &document.slots[0][0] else {
            panic!("still a clip");
        };
        assert_eq!(clip.launch.mode, sx::LaunchMode::Toggle);
        assert_eq!(clip.launch.fill, sx::FillRule::Only);
        assert_eq!(
            clip.name, "clip 7",
            "and the content still comes from the project"
        );
    }

    /// A rule follows its CLIP, not the square it was sitting in.
    #[test]
    fn a_launch_rule_travels_with_the_clip_it_was_set_on() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi]);
        arrangement.session.slots[0][0] = Some(midi_clip(7, 4.0));
        let mut document = sx::SessionDocument::default();
        sync_document(&mut document, &arrangement);
        let sx::Slot::Clip(clip) = &mut document.slots[0][0] else {
            panic!("a clip");
        };
        clip.launch.mode = sx::LaunchMode::Gate;

        // The same clip, one row down.
        arrangement.session.slots[0][0] = None;
        arrangement.session.slots[0][2] = Some(midi_clip(7, 4.0));
        sync_document(&mut document, &arrangement);

        assert!(matches!(document.slots[0][0], sx::Slot::Empty(_)));
        let sx::Slot::Clip(moved) = &document.slots[0][2] else {
            panic!("the clip moved");
        };
        assert_eq!(moved.launch.mode, sx::LaunchMode::Gate);
    }

    /// A QUEUED START IS COMPILED EARLY.
    ///
    /// The existing session path builds one schedule containing silence
    /// up to the launch beat. If the mirror waited for the boundary the
    /// compile would begin AT it, and the clip would come in late by
    /// however long the build took — audibly, and differently every time.
    #[test]
    fn a_queued_start_is_mirrored_before_its_boundary() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi]);
        arrangement.session.slots[0][1] = Some(midi_clip(7, 4.0));
        let mut document = sx::SessionDocument::default();
        sync_document(&mut document, &arrangement);
        let mut runtime = sx::SessionRuntime::new(1);
        // 48 kHz at 120 BPM: one beat is 24 000 samples.
        let clock = sx::TransportClock {
            sample: 0,
            sample_rate: 48_000,
            bpm: 120.0,
            beats_per_bar: 4,
        };
        runtime
            .queue_clip(&document, 0, 1, clock, true)
            .expect("the slot queues");

        let changed = mirror_playback(&runtime, &document, &mut arrangement, clock);
        assert!(changed);
        assert_eq!(
            arrangement.session.playing[0],
            Some(1),
            "the track is compiled for scene 1 already"
        );
        let at = arrangement.session.launch_at[0];
        assert!(at >= 0.0 && at.is_finite(), "{at}");

        // Nothing queued and nothing playing is silence, and saying so
        // twice does not keep reporting a change.
        let idle = sx::SessionRuntime::new(1);
        assert!(mirror_playback(&idle, &document, &mut arrangement, clock));
        assert_eq!(arrangement.session.playing[0], None);
        assert!(!mirror_playback(&idle, &document, &mut arrangement, clock));
    }

    /// PHASE COMES FROM THE TRANSPORT. A strip that advanced while the
    /// transport was paused would be a lie the performer acts on.
    #[test]
    fn phase_advances_with_the_transport_and_not_without_it() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi]);
        arrangement.session.slots[0][0] = Some(midi_clip(7, 4.0));
        let mut runtime = sx::SessionRuntime::new(1);
        runtime.tracks[0].playback = sx::PlaybackState::Playing {
            scene: sx::SceneId(1),
            clip: sx::ClipId(7),
            started_at_sample: 0,
            cycle: 0,
        };
        let at = |sample: u64| sx::TransportClock {
            sample,
            sample_rate: 48_000,
            bpm: 120.0,
            beats_per_bar: 4,
        };

        // Two beats into a four-beat clip is halfway round.
        sync_runtime(&mut runtime, &arrangement, &[], at(48_000), true);
        assert!(
            (runtime.tracks[0].phase - 0.5).abs() < 1e-3,
            "{}",
            runtime.tracks[0].phase
        );

        // Stopped, the same sample reports the phase it had rather than
        // running on.
        let before = runtime.tracks[0].phase;
        sync_runtime(&mut runtime, &arrangement, &[], at(96_000), false);
        assert_eq!(runtime.tracks[0].phase, before);

        // And a track that is not playing has no phase at all.
        runtime.tracks[0].playback = sx::PlaybackState::Stopped;
        sync_runtime(&mut runtime, &arrangement, &[], at(48_000), true);
        assert_eq!(runtime.tracks[0].phase, 0.0);
    }

    /// THE WHOLE ROUND TRIP: a press in the new view ends as audio on
    /// the old, proven compile path.
    ///
    /// This is the test that says the merge works. Everything either side
    /// of it is checked in its own file; nothing else checks that the two
    /// halves meet.
    #[test]
    fn a_launched_slot_reaches_the_compile_path() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi]);
        arrangement.session.slots[0][1] = Some(midi_clip(7, 4.0));
        let mut document = sx::SessionDocument::default();
        sync_document(&mut document, &arrangement);
        let mut runtime = sx::SessionRuntime::new(1);
        let clock = sx::TransportClock {
            sample: 0,
            sample_rate: 48_000,
            bpm: 120.0,
            beats_per_bar: 4,
        };

        // Nothing playing yet, so nothing compiles.
        assert!(arrangement.session.compiled(0).is_none());

        // The press.
        runtime
            .queue_clip(&document, 0, 1, clock, true)
            .expect("the slot queues");
        mirror_playback(&runtime, &document, &mut arrangement, clock);

        // And the existing path now has a clip to build.
        let compiled = arrangement
            .session
            .compiled(0)
            .expect("the launched slot compiles");
        assert!(!compiled.is_empty(), "it produced timeline clips");
        assert!(
            compiled.iter().all(|clip| clip.notes.len() == 1),
            "carrying the clip's own notes"
        );

        // Stopping takes it back out again.
        runtime.queue_stop_track(&document, 0, clock, true).ok();
        runtime.apply_due(u64::MAX);
        mirror_playback(&runtime, &document, &mut arrangement, clock);
        assert!(arrangement.session.compiled(0).is_none(), "stop is silence");
    }

    /// A track's kind is enforced at the seam, not discovered later: an
    /// audio clip queued onto a MIDI track is refused before anything is
    /// mirrored, so the compile path never sees a slot it cannot build.
    #[test]
    fn the_planner_refuses_a_clip_of_the_wrong_kind() {
        let mut arrangement = arrangement_with(&[TrackKind::Midi]);
        arrangement.session.slots[0][0] = Some(midi_clip(7, 4.0));
        let mut document = sx::SessionDocument::default();
        sync_document(&mut document, &arrangement);
        // Force the mismatch the way a bad adapter would.
        if let sx::Slot::Clip(clip) = &mut document.slots[0][0] {
            clip.kind = sx::TrackKind::Audio;
        }
        let clock = sx::TransportClock {
            sample: 0,
            sample_rate: 48_000,
            bpm: 120.0,
            beats_per_bar: 4,
        };
        let mut runtime = sx::SessionRuntime::new(1);
        assert_eq!(
            runtime.queue_clip(&document, 0, 0, clock, true),
            Err(sx::QueueRefusal::IncompatibleClip)
        );
        assert!(!mirror_playback(
            &runtime,
            &document,
            &mut arrangement,
            clock
        ));
        assert_eq!(arrangement.session.playing[0], None);
    }

    /// The palette comes from the THEME, so light and dark are one
    /// grammar rather than two authored ones.
    #[test]
    fn colors_come_from_the_theme() {
        let dark = colors(&Theme::dark());
        let light = colors(&Theme::light());
        assert_eq!(dark.bg, Theme::dark().bg);
        assert_eq!(light.bg, Theme::light().bg);
        assert_ne!(dark.bg, light.bg, "the two themes are not the same palette");
        assert_eq!(dark.danger, Theme::dark().danger);
        // The module's own default mirrors the DARK house palette on
        // purpose, so dark matching it proves nothing. Light is the test:
        // if the palette were still the module's authored one, light
        // would come back dark.
        assert_ne!(
            light.bg,
            sx::SessionColors::default().bg,
            "the view is still drawing its own palette"
        );
        assert_eq!(light.text, Theme::light().text);
    }

    #[test]
    fn a_missing_audio_file_is_reported_as_offline() {
        let mut arrangement = arrangement_with(&[TrackKind::Audio]);
        let mut clip = midi_clip(9, 2.0);
        clip.notes.clear();
        clip.audio = Some(crate::AudioSource {
            path: std::path::PathBuf::from("/nowhere/gone.wav"),
            sample_rate: 48_000,
            source_offset: 0,
            source_frames: 96_000,
            gain: 1.0,
            looped: false,
            file_frames: 96_000,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        });
        arrangement.session.slots[0][0] = Some(clip);
        let mut document = sx::SessionDocument::default();
        sync_document(&mut document, &arrangement);
        let sx::Slot::Clip(clip) = &document.slots[0][0] else {
            panic!("a clip");
        };
        assert!(clip.media_offline);
        assert_eq!(clip.kind, sx::TrackKind::Audio);
    }
}
