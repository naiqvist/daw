//! The redesign's bridge onto the application's established state.
//!
//! This layer survives the legacy UI because it projects canonical app data
//! into read-only redesign views and returns typed intents through the old,
//! authoritative mutation paths; it owns neither engine state nor a shadow UI.

use super::*;

/// The complete legacy arrangement vocabulary rendered in the redesign's
/// grayscale material system. Every RGB role is neutral by construction;
/// hierarchy comes from value, weight and geometry rather than hue.
pub(super) fn redesign_arrangement_theme() -> Theme {
    let mut theme = Theme::dark();
    let gray = egui::Color32::from_gray;
    theme.light = false;
    theme.bg = egui::Color32::BLACK;
    theme.surface = gray(10);
    theme.surface_raised = gray(18);
    theme.surface_sunken = egui::Color32::BLACK;
    theme.text = egui::Color32::WHITE;
    theme.text_muted = gray(112);
    theme.text_value = egui::Color32::WHITE;
    theme.outline = gray(42);
    theme.divider = gray(24);
    theme.focus = egui::Color32::WHITE;
    theme.accent = gray(220);
    theme.accent_muted = gray(72);
    theme.role_time = gray(190);
    theme.role_time_dim = gray(82);
    theme.role_level = gray(205);
    theme.role_level_dim = gray(76);
    theme.role_shape = gray(225);
    theme.role_shape_dim = gray(90);
    theme.role_mod = gray(180);
    theme.role_mod_dim = gray(68);
    theme.ok = gray(188);
    theme.warn = gray(214);
    theme.danger = egui::Color32::WHITE;
    theme.red_zone = gray(210);
    theme.green_zone = gray(170);
    theme.playhead = egui::Color32::WHITE;
    theme.loop_region = egui::Color32::from_rgba_premultiplied(90, 90, 90, 30);
    theme.loop_brace = gray(205);
    theme.selection = egui::Color32::from_rgba_premultiplied(110, 110, 110, 44);
    theme.grid_beat = gray(28);
    theme.grid_bar = gray(54);
    theme.grid_sub = gray(16);
    theme.timeline_lane = egui::Color32::BLACK;
    theme.timeline_lane_alt = gray(5);
    theme.timeline_lane_selected = gray(12);
    theme.clip_body = gray(24);
    theme.clip_midi = gray(30);
    theme.clip_midi_header = gray(58);
    theme.clip_audio = gray(22);
    theme.clip_audio_header = gray(48);
    theme.clip_hover = gray(138);
    theme.clip_selected = egui::Color32::WHITE;
    theme.clip_note = gray(210);
    theme.note_fill = gray(184);
    theme.note_fill_selected = gray(220);
    theme.note_edge = gray(20);
    theme.note_hover = gray(204);
    theme.note_ghost = gray(88);
    theme.meter_low = gray(170);
    theme.meter_hot = gray(220);
    theme.meter_clip = egui::Color32::WHITE;
    theme
}

/// The sample rate the tempo warp is computed at.
///
/// Any rate gives the same answer: the warp is a RATIO of two quantities
/// both proportional to it, so the rate cancels. A concrete one is named
/// only because the table needs one.
const WARP_SAMPLE_RATE: f64 = 48_000.0;

/// A tick's position, expressed as the beat the UNIFORM legacy compiler
/// must be told in order to stamp the sample the tempo map really means.
///
/// The legacy path multiplies one beat by one samples-per-beat (graph.rs
/// computes `samples_per_beat = sr*60/bpm` once), so a varying tempo
/// cannot be handed to it directly. It can be BAKED: a tick's true sample
/// is `table.sample_at(tick)`, and dividing by the reference
/// samples-per-beat gives the beat that lands on exactly that sample.
///
/// With an EMPTY tempo map this is the exact identity — `sample_at` is
/// uniform at the reference tempo, so the division returns
/// `tick / TICKS_PER_BEAT`, which is precisely what the projection
/// computed before. That is what makes it safe to apply unconditionally:
/// a project with no tempo marks projects bit for bit as it always did.
///
/// A bridge-era measure, exactly as p-locks ride the same road. At C2 the
/// Song-direct compiler consults the table itself and this disappears.
fn warped_beat(table: &daw::tempo::TempoTable, tick: usize, samples_per_beat: f64) -> f64 {
    if samples_per_beat <= 0.0 {
        return 0.0;
    }
    table.sample_at(tick) as f64 / samples_per_beat
}

fn beats_to_sequence_ticks(beats: f64) -> usize {
    (beats.max(0.0) * daw::sequencing::TICKS_PER_BEAT as f64).round() as usize
}

fn sequence_ticks_to_beats(ticks: usize) -> f64 {
    ticks as f64 / daw::sequencing::TICKS_PER_BEAT as f64
}

const SONG_PATTERN_STEP_TICKS: usize = daw::sequencing::PATTERN_STEP_TICKS;

fn song_pattern_length(song: &daw::sequencing::Song, id: daw::sequencing::PatternId) -> usize {
    daw::ui::sequencer::pattern_length(song, id)
}

fn song_pattern_note_views(
    pattern: &daw::sequencing::Pattern,
    key: &daw::pitch::Key,
) -> Vec<redesign_sequence::NoteView> {
    daw::ui::sequencer::note_views(pattern, key)
}

fn song_note_view(
    note: &daw::sequencing::Note,
    start_ticks: usize,
    probability: f32,
    enabled: bool,
    key: &daw::pitch::Key,
) -> redesign_sequence::NoteView {
    daw::ui::sequencer::note_view(note, start_ticks, probability, enabled, key)
}

/// Below this remainder the legacy MIDI path reproduces a pitch exactly
/// (float noise is orders of magnitude smaller); above it the trig wears
/// the `≈` playback-approximation sign.

/// One whole trig a recorded MIDI take WOULD write. The complete `after`
/// value matters: the approved collision rule replaces the addressed trig,
/// so a preview that carried only the incoming notes would conceal what
/// Enter actually commits.
pub(super) struct MidiTrigWrite {
    pub(super) step: usize,
    pub(super) after: daw::sequencing::Trig,
}

/// Pending lossy note material, held outside the Song until Enter commits
/// or Escape cancels. `:snap-key` and recorded MIDI share the same preview
/// contract and therefore the same single pending address.
pub(super) enum SnapPreview {
    Key {
        pattern: daw::sequencing::PatternId,
        step: usize,
        after: Vec<daw::sequencing::Note>,
    },
    Midi {
        pattern: daw::sequencing::PatternId,
        writes: Vec<MidiTrigWrite>,
        grid: String,
    },
    /// Closure after commit: the new material remains the lower strip's
    /// active noun until the performer returns to the arrangement. It owns
    /// no music, only the same transient attention a cursor owns.
    MidiResult { pattern: daw::sequencing::PatternId },
}

impl SnapPreview {
    fn pattern(&self) -> daw::sequencing::PatternId {
        match self {
            Self::Key { pattern, .. }
            | Self::Midi { pattern, .. }
            | Self::MidiResult { pattern } => *pattern,
        }
    }

    fn is_pending(&self) -> bool {
        !matches!(self, Self::MidiResult { .. })
    }

    fn ghosts(&self, key: &daw::pitch::Key) -> Vec<redesign_sequence::NoteView> {
        match self {
            Self::Key { step, after, .. } => after
                .iter()
                .map(|note| song_note_view(note, step * SONG_PATTERN_STEP_TICKS, 1.0, true, key))
                .collect(),
            Self::Midi { writes, .. } => writes
                .iter()
                .flat_map(|write| {
                    write.after.notes.iter().map(move |note| {
                        song_note_view(
                            note,
                            write.step * SONG_PATTERN_STEP_TICKS,
                            write.after.probability,
                            write.after.enabled,
                            key,
                        )
                    })
                })
                .collect(),
            Self::MidiResult { .. } => Vec::new(),
        }
    }

    fn cancelled_notice(&self) -> &'static str {
        match self {
            Self::Key { .. } => "SNAP-KEY: CANCELLED — NOTHING CHANGED",
            Self::Midi { .. } => "MIDI TAKE: CANCELLED — NOTHING CHANGED",
            Self::MidiResult { .. } => "MIDI TAKE: RESULT STILL SELECTED",
        }
    }
}

/// A `.lens` file from the library, by stem, parsed on demand.
fn lens_file_by_name(
    snapshot: &daw::library::LibrarySnapshot,
    stem: &str,
) -> Option<Result<daw::ui::redesign::lens::Lens, String>> {
    let record = snapshot
        .lenses
        .iter()
        .find(|lens| lens.name.eq_ignore_ascii_case(stem))?;
    Some(match std::fs::read_to_string(&record.path) {
        Ok(source) => daw::ui::redesign::lens::parse_lens(&source),
        Err(error) => Err(format!("cannot read {}: {error}", record.path.display())),
    })
}

fn apply_song_pattern_intents(
    pattern: &mut daw::sequencing::Pattern,
    intents: &[redesign_sequence::Intent],
) -> Option<&'static str> {
    pattern.apply_all(intents)
}

impl App {
    pub(super) fn apply_redesign_sequence_intents(
        &mut self,
        intents: &[redesign_sequence::Intent],
    ) {
        let Some((track, index)) = self.arrangement.selected_clip else {
            return;
        };
        if !self
            .arrangement
            .tracks
            .get(track)
            .is_some_and(|lane| lane.kind == TrackKind::Midi)
        {
            return;
        }

        for intent in intents {
            use redesign_sequence::Intent;
            let (tick, required_end) = match *intent {
                Intent::Toggle {
                    tick,
                    default_length_ticks,
                    ..
                }
                | Intent::SetPrimary {
                    tick,
                    length_ticks: default_length_ticks,
                    ..
                } => (tick, tick.saturating_add(default_length_ticks)),
                Intent::AddNote {
                    tick, length_ticks, ..
                } => (tick, tick.saturating_add(length_ticks)),
                Intent::Clear { tick }
                | Intent::Nudge { tick, .. }
                | Intent::Resize { tick, .. }
                | Intent::SetProbability { tick, .. }
                | Intent::AdjustVelocity { tick, .. } => (tick, tick),
            };

            // A fixed 64-address grid must never accept an inaudible note
            // beyond the clip edge. Grow the clip as needed, respecting the
            // next timeline clip; if that boundary blocks the address, leave
            // the musical data untouched and tell the user why.
            if required_end > 0 {
                let want = sequence_ticks_to_beats(required_end) as f32;
                if want > self.arrangement.clips[track][index].len {
                    let length = clamp_clip_len(
                        &self.arrangement.clips[track],
                        index,
                        want,
                        self.arrangement.grid_beats(),
                    );
                    set_scripted_clip_len(&mut self.arrangement.clips[track][index], length);
                }
                if sequence_ticks_to_beats(tick) as f32 >= self.arrangement.clips[track][index].len
                {
                    self.notice = Some(
                        "sequence step is beyond the clip boundary; move the next clip first"
                            .to_owned(),
                    );
                    continue;
                }
            }

            // Legacy clips store MIDI numbers: the bridge resolves the
            // typed pitch against the project key, green-side, exactly
            // like the song projection does.
            let key = self.song.key.clone();
            let as_midi = |pitch: daw::pitch::Pitch| daw::pitch::nearest_midi(pitch.resolve(&key));
            let clip = &mut self.arrangement.clips[track][index];
            let at_tick = |note: &Note| beats_to_sequence_ticks(note.start) == tick;
            match *intent {
                Intent::Toggle {
                    default_pitch,
                    default_length_ticks,
                    default_velocity,
                    ..
                } => {
                    let present = clip.notes.iter().any(at_tick);
                    if present {
                        let enabled = clip.notes.iter().any(|note| at_tick(note) && !note.muted);
                        for note in clip.notes.iter_mut().filter(|note| at_tick(note)) {
                            note.muted = enabled;
                        }
                    } else {
                        clip.notes.push(Note {
                            pitch: as_midi(default_pitch),
                            start: sequence_ticks_to_beats(tick),
                            len: sequence_ticks_to_beats(default_length_ticks),
                            vel: default_velocity,
                            muted: false,
                            plocks: Vec::new(),
                            prob: 1.0,
                            cond: None,
                        });
                    }
                }
                Intent::SetPrimary {
                    pitch,
                    length_ticks,
                    velocity,
                    ..
                } => {
                    let primary = clip
                        .notes
                        .iter()
                        .enumerate()
                        .filter(|(_, note)| at_tick(note))
                        .min_by_key(|(_, note)| note.pitch)
                        .map(|(index, _)| index);
                    if let Some(primary) = primary {
                        let note = &mut clip.notes[primary];
                        note.pitch = as_midi(pitch);
                        note.len = sequence_ticks_to_beats(length_ticks);
                        note.vel = velocity;
                        note.muted = false;
                    } else {
                        clip.notes.push(Note {
                            pitch: as_midi(pitch),
                            start: sequence_ticks_to_beats(tick),
                            len: sequence_ticks_to_beats(length_ticks),
                            vel: velocity,
                            muted: false,
                            plocks: Vec::new(),
                            prob: 1.0,
                            cond: None,
                        });
                    }
                }
                Intent::Clear { .. } => clip.notes.retain(|note| !at_tick(note)),
                Intent::AddNote {
                    pitch,
                    length_ticks,
                    velocity,
                    probability,
                    ..
                } => {
                    clip.notes.push(Note {
                        pitch: as_midi(pitch),
                        start: sequence_ticks_to_beats(tick),
                        len: sequence_ticks_to_beats(length_ticks),
                        vel: velocity,
                        muted: false,
                        plocks: Vec::new(),
                        prob: probability,
                        cond: None,
                    });
                }
                Intent::AdjustVelocity { delta, .. } => {
                    let mut touched = false;
                    for note in clip.notes.iter_mut().filter(|note| at_tick(note)) {
                        note.vel = (i16::from(note.vel) + delta as i16).clamp(1, 127) as u8;
                        touched = true;
                    }
                    if !touched {
                        self.notice = Some("velocity: no trig here".to_owned());
                    }
                }
                Intent::SetProbability { probability, .. } => {
                    let mut touched = false;
                    for note in clip.notes.iter_mut().filter(|note| at_tick(note)) {
                        note.prob = probability.clamp(0.01, 1.0);
                        touched = true;
                    }
                    if !touched {
                        self.notice = Some("condition: no trig here".to_owned());
                    }
                }
                Intent::Nudge { delta_ticks, .. } => {
                    // The whole trig moves or none of it does: a nudge that
                    // would push any note past an edge is refused, so the
                    // gesture never half-applies.
                    if !clip.notes.iter().any(&at_tick) {
                        self.notice = Some("nudge: no trig here".to_owned());
                        continue;
                    }
                    let clip_end = beats_to_sequence_ticks(f64::from(clip.len));
                    let target = tick as isize + delta_ticks;
                    let blocked = target < 0
                        || clip.notes.iter().filter(|note| at_tick(note)).any(|note| {
                            target as usize + beats_to_sequence_ticks(note.len) > clip_end
                        });
                    if blocked {
                        self.notice = Some("nudge blocked at the clip edge".to_owned());
                        continue;
                    }
                    for note in clip.notes.iter_mut().filter(|note| at_tick(note)) {
                        note.start = sequence_ticks_to_beats(target as usize);
                    }
                }
                Intent::Resize { delta_ticks, .. } => {
                    let clip_end = beats_to_sequence_ticks(f64::from(clip.len));
                    let mut touched = false;
                    for note in clip.notes.iter_mut().filter(|note| at_tick(note)) {
                        let length = beats_to_sequence_ticks(note.len) as isize + delta_ticks;
                        let length = (length.max(1) as usize).min(clip_end.saturating_sub(tick));
                        note.len = sequence_ticks_to_beats(length.max(1));
                        touched = true;
                    }
                    if !touched {
                        self.notice = Some("resize: no trig here".to_owned());
                    }
                }
            }
            clip.notes.sort_by(|a, b| {
                a.start
                    .total_cmp(&b.start)
                    .then_with(|| a.pitch.cmp(&b.pitch))
            });
        }
    }

    fn apply_redesign_chain_intents(&mut self, intents: &[daw::ui::redesign::chain::Intent]) {
        let track = self.arrangement.active_track();
        for intent in intents {
            use daw::ui::redesign::chain::Intent;
            match *intent {
                Intent::SetParam {
                    device,
                    param,
                    value,
                } => {
                    // The TRACK HEAD is not in any chain: its slots are the
                    // Song track's own mixer values, and they must be
                    // written there. Writing the legacy twin would be
                    // erased on the next projection pass, which copies the
                    // Song onto it.
                    if device == daw::ui::redesign::chain::TRACK_HEAD_ID {
                        let Some(index) = self.song_track_for_active_chain() else {
                            self.notice = Some("TRACK: NO SONG TRACK".to_owned());
                            continue;
                        };
                        let returns = self.song.returns.len();
                        let Some(song_track) = self.song.tracks.get_mut(index) else {
                            self.notice = Some("TRACK: NO SONG TRACK".to_owned());
                            continue;
                        };
                        match param {
                            daw::ui::redesign::chain::TRACK_LEVEL_PARAM => {
                                let (min, max) =
                                    daw::targets::span_of(daw::sequencing::TRACK_VOLUME)
                                        .unwrap_or((0.0, 1.0));
                                song_track.volume = value.clamp(min, max);
                            }
                            daw::ui::redesign::chain::TRACK_PAN_PARAM => {
                                let (min, max) = daw::targets::span_of(daw::sequencing::TRACK_PAN)
                                    .unwrap_or((-1.0, 1.0));
                                song_track.pan = value.clamp(min, max);
                            }
                            other => {
                                let Some(send) = daw::ui::redesign::chain::track_send_index(other)
                                else {
                                    self.notice = Some("TRACK: NO SUCH SLOT".to_owned());
                                    continue;
                                };
                                if send >= returns {
                                    let letter = daw::sequencing::ReturnTrack::letter(send);
                                    self.notice = Some(format!("SEND {letter}: NO RETURN"));
                                    continue;
                                }
                                let Some(target) = daw::targets::track_send_target(send) else {
                                    self.notice = Some("SEND: NO SUCH SLOT".to_owned());
                                    continue;
                                };
                                let (min, max) =
                                    daw::targets::span_of(target).unwrap_or((0.0, 1.0));
                                if song_track.sends.len() <= send {
                                    song_track.sends.resize(send + 1, 0.0);
                                }
                                song_track.sends[send] = value.clamp(min, max);
                            }
                        }
                        self.projected_song = None;
                        continue;
                    }
                    let Some(track) = track else { continue };
                    self.apply_device_edits(
                        ChainOwner::Track(track),
                        device,
                        &[device::ParamEdit { param, value }],
                    );
                }
                Intent::ToggleTrackArm => {
                    let Some(index) = self.song_track_for_active_chain() else {
                        self.notice = Some("ARM: NO SONG TRACK".to_owned());
                        continue;
                    };
                    let Some(song_track) = self.song.tracks.get_mut(index) else {
                        self.notice = Some("ARM: NO SONG TRACK".to_owned());
                        continue;
                    };
                    if song_track.kind != daw::sequencing::TrackKind::Audio || song_track.is_group {
                        self.notice = Some("ARM: AUDIO TRACKS ONLY".to_owned());
                        continue;
                    }
                    song_track.armed = !song_track.armed;
                    self.notice = Some(format!(
                        "ARM: {}",
                        if song_track.armed { "ON" } else { "OFF" }
                    ));
                    self.projected_song = None;
                }
                Intent::CycleTrackMonitor => {
                    let Some(index) = self.song_track_for_active_chain() else {
                        self.notice = Some("MONITOR: NO SONG TRACK".to_owned());
                        continue;
                    };
                    let Some(song_track) = self.song.tracks.get_mut(index) else {
                        self.notice = Some("MONITOR: NO SONG TRACK".to_owned());
                        continue;
                    };
                    if song_track.kind != daw::sequencing::TrackKind::Audio || song_track.is_group {
                        self.notice = Some("MONITOR: AUDIO TRACKS ONLY".to_owned());
                        continue;
                    }
                    song_track.monitor = song_track.monitor.cycled();
                    self.notice = Some(format!("MONITOR: {}", song_track.monitor.label()));
                    self.projected_song = None;
                }
                Intent::ToggleBypass { device } => {
                    let Some(track) = track else { continue };
                    if let Some(instance) = self.chain_device_mut(ChainOwner::Track(track), device)
                    {
                        instance.bypass = !instance.bypass;
                        self.arrangement.force_recompile = true;
                    }
                }
                Intent::Reorder { device, target } => {
                    let Some(track) = track else { continue };
                    let moved =
                        self.arrangement.tracks.get_mut(track).is_some_and(|lane| {
                            track::move_device(&mut lane.chain, device, target)
                        });
                    if moved {
                        self.arrangement.force_recompile = true;
                    }
                }
                Intent::AddDevice { catalogue_index } => {
                    if let Some(spec) = devices::DEVICES.get(catalogue_index) {
                        self.add_redesign_device(spec.kind);
                    }
                }
            }
        }
    }

    fn add_redesign_device(&mut self, kind: DeviceKind) {
        if let Some(index) = self.arrangement.return_selected {
            if kind.is_instrument() {
                self.notice = Some(format!(
                    "a return is a bus — no slot for a {}",
                    kind.spec().name
                ));
                return;
            }
            let instance = self.mint_device_instance(kind);
            if let Some(bus) = self.arrangement.returns.get_mut(index)
                && bus.insert_device(instance)
            {
                self.arrangement.force_recompile = true;
            }
            return;
        }
        if self.arrangement.master_selected {
            if kind.is_instrument() {
                self.notice = Some(format!(
                    "the master is a bus — no slot for a {}",
                    kind.spec().name
                ));
                return;
            }
            let instance = self.mint_device_instance(kind);
            if self.arrangement.master.insert_device(instance) {
                self.arrangement.force_recompile = true;
            }
            return;
        }

        let Some(track) = self.arrangement.active_track() else {
            self.notice = Some("no track selected".to_owned());
            return;
        };
        let Some(target) = self.arrangement.tracks.get(track) else {
            self.notice = Some("no track selected".to_owned());
            return;
        };
        if kind.is_instrument() && !target.kind.takes_instrument() {
            let name = target.name.clone();
            let what = if target.is_group {
                "a group"
            } else {
                "an audio track"
            };
            self.notice = Some(format!(
                "{name} is {what} — no slot for a {}",
                kind.spec().name
            ));
            return;
        }

        let instance = self.mint_device_instance(kind);
        let displaced = self.arrangement.tracks[track].insert_device(instance);
        if let Some(displaced) = displaced {
            self.arrangement.forget_device(track, displaced);
        }
        self.arrangement.select_track(track);
        self.arrangement.force_recompile = true;
    }

    /// The new visual layer starts here. The render pass already clears the
    /// client area to black, so drawing nothing is an intentional blank UI.
    /// Drain the controller and enter what it played.
    ///
    /// Note-ONS only: a release ends nothing here, because entering a
    /// note into a step sequencer is an instant act rather than a held
    /// one. When live monitoring exists, note-offs become its business.
    fn pump_midi_input(&mut self) {
        for (_stamp, event) in self.midi_input.drain() {
            if let daw::midi_input::MidiEvent::NoteOn { note, .. } = event {
                self.redesign
                    .enter_pitch(daw::pitch::Pitch::from_midi(note));
            }
        }
    }

    pub(super) fn draw_redesign_ui(&mut self, ui: &mut egui::Ui) {
        self.pump_midi_input();
        // A pending lossy write owns Enter and Escape before anything else
        // can hear them: commit the loss, or walk away whole. The ghosts in
        // the grid say what is at stake; the palette, when open, still
        // speaks first.
        if self
            .snap_preview
            .as_ref()
            .is_some_and(SnapPreview::is_pending)
            && !self.palette.is_open()
        {
            let commit = ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            let cancel = !commit
                && ui
                    .ctx()
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if commit {
                self.commit_snap_preview();
            } else if cancel {
                let notice = self
                    .snap_preview
                    .as_ref()
                    .map_or("PREVIEW: CANCELLED — NOTHING CHANGED", |preview| {
                        preview.cancelled_notice()
                    });
                self.snap_preview = None;
                self.notice = Some(notice.to_owned());
            }
        }

        let playhead_beats = self.transport.position * self.transport.bpm / 60.0;
        // The device surface sees a green-zone projection of the selected
        // track's canonical chain. Parameter words come from the same
        // formatters as the legacy lock editor and device cards; edits return
        // below through `apply_device_edits`, never through a second state.
        let mut chain_view = self
            .arrangement
            .active_track()
            .and_then(|track| self.arrangement.tracks.get(track))
            .map(|track| daw::ui::redesign::chain::View {
                track_name: Some(track.name.clone()),
                track_state: None,
                devices: track
                    .chain
                    .iter()
                    .map(|instance| {
                        let kind = instance.kind();
                        let spec = kind.spec();
                        daw::ui::redesign::chain::DeviceView {
                            id: instance.id,
                            name: spec.name.to_owned(),
                            bypassed: instance.bypass,
                            instrument: kind.is_instrument(),
                            parent: instance.parent,
                            hero: match kind {
                                devices::DeviceKind::Kick => {
                                    daw::ui::redesign::chain::HeroKind::Kick
                                }
                                devices::DeviceKind::Filter => {
                                    daw::ui::redesign::chain::HeroKind::Filter
                                }
                                devices::DeviceKind::Sat => {
                                    daw::ui::redesign::chain::HeroKind::Saturator
                                }
                                _ => daw::ui::redesign::chain::HeroKind::None,
                            },
                            params: spec
                                .params
                                .iter()
                                .zip(spec.labels)
                                .map(|(def, label)| {
                                    let base = instance.state.value(def.id).unwrap_or(def.default);
                                    daw::ui::redesign::chain::ParamView {
                                        id: def.id,
                                        name: label.name.to_owned(),
                                        min: def.min,
                                        max: def.max,
                                        base,
                                        choices: device_choices(kind, def),
                                        formatted: device_format(kind, def.id, base, label.unit),
                                        // No trig noun is selected on this
                                        // surface yet, so only BASE exists.
                                        lock: None,
                                    }
                                })
                                .collect(),
                        }
                    })
                    .collect(),
                catalogue: Vec::new(),
            })
            .unwrap_or_default();
        // The TRACK HEAD, at the head of the chain — the fader and pan as
        // one more 8-slot page, so the hand travels to them exactly as it
        // travels to any device. Values come from the SONG track, because
        // that is where they persist; the projection copies them onto the
        // legacy twin, so reading the twin would read a shadow.
        if let Some(song_index) = self.song_track_for_active_chain() {
            let track = &self.song.tracks[song_index];
            if track.kind == daw::sequencing::TrackKind::Audio && !track.is_group {
                chain_view.track_state = Some(daw::ui::redesign::chain::TrackStateView {
                    input: track.input.label(),
                    monitor: track.monitor.label().to_owned(),
                    monitoring: track.monitor.hears(track.armed),
                    armed: track.armed,
                    recording: track.armed && self.transport.recording(),
                });
            }
            let level_span =
                daw::targets::span_of(daw::sequencing::TRACK_VOLUME).unwrap_or((0.0, 1.0));
            let pan_span = daw::targets::span_of(daw::sequencing::TRACK_PAN).unwrap_or((-1.0, 1.0));
            let mut params = vec![
                daw::ui::redesign::chain::ParamView {
                    id: daw::ui::redesign::chain::TRACK_LEVEL_PARAM,
                    name: "LEVEL".to_owned(),
                    // The registry's span, not an assumed unit
                    // interval: a fader reaches 1.5, so it can
                    // boost above unity like every other one.
                    min: level_span.0,
                    max: level_span.1,
                    base: track.volume,
                    choices: 0,
                    // DECIBELS, not a unit fraction: real data over
                    // euphemism. The model stores linear because
                    // that is what the engine multiplies by; the
                    // widget owns the dB mapping, which is the only
                    // place that curve belongs.
                    formatted: format_gain_db(track.volume),
                    lock: None,
                },
                daw::ui::redesign::chain::ParamView {
                    id: daw::ui::redesign::chain::TRACK_PAN_PARAM,
                    name: "PAN".to_owned(),
                    min: pan_span.0,
                    max: pan_span.1,
                    base: track.pan,
                    choices: 0,
                    formatted: format_pan(track.pan),
                    lock: None,
                },
            ];
            for return_index in 0..self.song.returns.len() {
                let Some(id) = daw::ui::redesign::chain::track_send_param(return_index) else {
                    continue;
                };
                let Some(target) = daw::targets::track_send_target(return_index) else {
                    continue;
                };
                let (min, max) = daw::targets::span_of(target).unwrap_or((0.0, 1.0));
                let base = track.send(return_index);
                params.push(daw::ui::redesign::chain::ParamView {
                    id,
                    name: format!(
                        "SEND {}",
                        daw::sequencing::ReturnTrack::letter(return_index)
                    ),
                    min,
                    max,
                    base,
                    choices: 0,
                    formatted: format_gain_db(base),
                    lock: None,
                });
            }
            chain_view.devices.insert(
                0,
                daw::ui::redesign::chain::DeviceView {
                    id: daw::ui::redesign::chain::TRACK_HEAD_ID,
                    name: "TRACK".to_owned(),
                    bypassed: false,
                    instrument: false,
                    parent: None,
                    hero: daw::ui::redesign::chain::HeroKind::None,
                    params,
                },
            );
        }
        chain_view.catalogue = devices::DEVICES
            .iter()
            .map(|spec| daw::ui::redesign::chain::CatalogueItem {
                name: spec.name.to_owned(),
                is_instrument: spec.instrument,
            })
            .collect();
        // The lower grid projects whichever model owns the center. SONG mode
        // reads its canonical pattern; legacy mode remains the piano-roll
        // projection and sends its intents back to the selected legacy clip.
        let song_pattern_id = if self.center_song {
            self.snap_preview
                .as_ref()
                .map(SnapPreview::pattern)
                .or_else(|| self.redesign.selected_song_pattern(&self.song))
        } else {
            None
        };
        // The pitch language MIDI typing speaks: the focused song track's
        // authority reads the ambient key; everything else (and the whole
        // legacy world) stays chromatic. Local shadows global.
        let entry_mode = if self.center_song {
            let authority = self
                .redesign
                .selected_song_track(&self.song)
                .and_then(|track| self.song.tracks.get(track))
                .map(|track| track.pitch_authority);
            match authority {
                Some(daw::sequencing::PitchAuthority::Degree) => {
                    daw::ui::redesign::midi_typing::EntryMode::Degree {
                        degrees: self.song.key.degree_count(),
                    }
                }
                _ => daw::ui::redesign::midi_typing::EntryMode::Chromatic,
            }
        } else {
            daw::ui::redesign::midi_typing::EntryMode::Chromatic
        };
        let key_sign = daw::ui::redesign::lens::key_sign(&self.song.key);
        // The active track's lens, resolved against the key each frame:
        // a lens that cannot speak this key falls back to degrees, and
        // the status line says so. Defaults follow the track's authority
        // until `:lens` says otherwise.
        let requested_lens = if self.center_song {
            self.redesign
                .selected_song_track(&self.song)
                .and_then(|track| self.song.tracks.get(track))
                .map(|track| {
                    self.track_lenses
                        .get(&track.id.0)
                        .cloned()
                        .unwrap_or_else(|| match track.pitch_authority {
                            daw::sequencing::PitchAuthority::Degree => "degrees".to_owned(),
                            daw::sequencing::PitchAuthority::Absolute => "notes".to_owned(),
                        })
                })
                .unwrap_or_else(|| "notes".to_owned())
        } else {
            "notes".to_owned()
        };
        let lens_view = if daw::ui::redesign::lens::BUILTIN_LENSES
            .contains(&requested_lens.as_str())
        {
            daw::ui::redesign::lens::LensView::resolve(&requested_lens, &self.song.key, &|_| None)
        } else {
            let file = self.cached_lens_file(&requested_lens);
            daw::ui::redesign::lens::LensView::resolve(&requested_lens, &self.song.key, &|_| {
                file.clone()
            })
        };
        let (sequence_clip_id, sequence_clip_name, sequence_clip_len, sequence_notes) =
            if self.center_song {
                if let Some(pattern) = song_pattern_id.and_then(|id| self.song.pattern(id)) {
                    (
                        Some(pattern.id.0),
                        Some(pattern.name.clone()),
                        Some(song_pattern_length(&self.song, pattern.id)),
                        song_pattern_note_views(pattern, &self.song.key),
                    )
                } else {
                    (None, None, None, Vec::new())
                }
            } else {
                let sequence_clip_id = self.arrangement.active_clip_id();
                let sequence_clip_name = self
                    .arrangement
                    .active_clip_ref()
                    .filter(|_| sequence_clip_id.is_some())
                    .map(|clip| clip.name.clone());
                let sequence_clip_len = self
                    .arrangement
                    .active_clip_ref()
                    .filter(|_| sequence_clip_id.is_some())
                    .map(|clip| beats_to_sequence_ticks(f64::from(clip.len)));
                let sequence_notes = self
                    .arrangement
                    .active_clip_ref()
                    .filter(|_| sequence_clip_id.is_some())
                    .map(|clip| {
                        clip.notes
                            .iter()
                            .map(|note| {
                                redesign_sequence::NoteView::from_midi(
                                    note.pitch,
                                    beats_to_sequence_ticks(note.start),
                                    beats_to_sequence_ticks(note.len).max(1),
                                    note.vel,
                                    note.prob,
                                    !note.muted,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (
                    sequence_clip_id,
                    sequence_clip_name,
                    sequence_clip_len,
                    sequence_notes,
                )
            };
        // The pending lossy write's would-be notes, as ghosts over the grid.
        let sequence_ghosts: Vec<redesign_sequence::NoteView> = self
            .snap_preview
            .as_ref()
            .filter(|preview| Some(preview.pattern()) == song_pattern_id)
            .map(|preview| preview.ghosts(&self.song.key))
            .unwrap_or_default();
        let sequence_view = sequence_clip_id
            .zip(sequence_clip_name.as_deref())
            .zip(sequence_clip_len)
            .map(|((id, name), length_ticks)| redesign_sequence::ClipView {
                id,
                name,
                length_ticks,
                notes: &sequence_notes,
                ghosts: &sequence_ghosts,
            });
        let position = format_position(
            self.transport.position,
            self.transport.bpm,
            u64::from(self.transport.beats_per_bar),
        );
        let outcome = self.redesign.show(
            ui,
            redesign_transport::View {
                playing: self.transport.playing,
                armed: self.transport.armed,
                loop_on: self.transport.loop_on,
                metronome: self.transport.metronome,
                follow: self.transport.follow,
                engine_on: self.engine.is_some(),
                bpm: self.transport.bpm,
                beats_per_bar: self.transport.beats_per_bar,
                beat_unit: self.transport.beat_unit,
                position: &position,
                key_sign: &key_sign,
                notice: self.notice.as_deref(),
                count_in_bars: self.transport.count_in_bars,
                // Armed and rolling but not yet writing: the one state a
                // performer must not confuse with recording.
                counting_in: self.transport.recording() && !self.transport.capturing(),
            },
            redesign_browser::View {
                snapshot: &self.library_snapshot,
                scanning: self.library_scanning,
            },
            &chain_view,
            sequence_view,
            entry_mode,
            &lens_view,
            playhead_beats,
            !self.prefs.browser_hidden,
            !self.prefs.lower_hidden,
        );

        if let Some(tick) = outcome.sequence.cursor_tick {
            self.sequence_cursor_tick = tick;
        }
        if !outcome.sequence.intents.is_empty() {
            if self.center_song {
                if let Some(pattern_id) = song_pattern_id
                    && let Some(pattern) = self.song.pattern_mut(pattern_id)
                    && let Some(notice) =
                        apply_song_pattern_intents(pattern, &outcome.sequence.intents)
                {
                    self.notice = Some(notice.to_owned());
                }
            } else {
                self.apply_redesign_sequence_intents(&outcome.sequence.intents);
            }
        }
        if !outcome.chain.intents.is_empty() {
            self.apply_redesign_chain_intents(&outcome.chain.intents);
        }

        let mut actions = Vec::with_capacity(outcome.transport.intents.len());
        for intent in outcome.transport.intents {
            use redesign_transport::Intent;
            match intent {
                Intent::Return => actions.push(UiAction::Return),
                Intent::TogglePlay => actions.push(UiAction::TogglePlay),
                Intent::Pause => actions.push(UiAction::Pause),
                Intent::Stop => actions.push(UiAction::Stop),
                Intent::ToggleRecord => actions.push(UiAction::ToggleRecord),
                Intent::ToggleLoop => actions.push(UiAction::ToggleLoop),
                Intent::ToggleMetronome => actions.push(UiAction::ToggleMetronome),
                Intent::ToggleFollow => actions.push(UiAction::ToggleFollow),
                Intent::SetTempo(value) => actions.push(UiAction::SetTempo(value)),
                Intent::CycleBeatUnit => actions.push(UiAction::SetTimeSignature(
                    self.transport.beats_per_bar,
                    match self.transport.beat_unit {
                        1 => 2,
                        2 => 4,
                        4 => 8,
                        8 => 16,
                        _ => 1,
                    },
                )),
                Intent::ToggleEngine => {
                    if self.engine.is_some() {
                        self.stop_engine();
                    } else {
                        self.start_engine();
                    }
                }
            }
        }
        for intent in outcome.browser.intents {
            match intent {
                redesign_browser::Intent::SelectSample(path) => self.place_sample(path, None),
                redesign_browser::Intent::LandSample(path) => {
                    // The browser said WHICH sound; the target comes from
                    // where attention already is, so the cursor never
                    // travels to place something. When held track keys
                    // arrive (roadmap item 1) they replace this line and
                    // nothing else.
                    let track = self.redesign.selected_song_track(&self.song);
                    match track {
                        Some(track) => {
                            let (start_beats, _) = self.redesign.song_selection_beats(&self.song);
                            let start_tick = beats_to_sequence_ticks(f64::from(start_beats));
                            self.pending_song_landings
                                .push((path.clone(), track, start_tick));
                            self.place_sample(path, None);
                        }
                        None => {
                            self.notice =
                                Some(daw::sequencing::LandRefusal::NoTrack.sign().to_owned());
                        }
                    }
                }
                redesign_browser::Intent::AuditionSample(path) => {
                    let Some(sample_rate) =
                        self.engine.as_ref().map(|engine| engine.info().sample_rate)
                    else {
                        self.notice = Some("sample audition needs the audio engine".to_owned());
                        continue;
                    };
                    self.audition_loader.request(path, sample_rate);
                }
                redesign_browser::Intent::StopAudition => {
                    self.audition_loader.stop();
                    if let Some(engine) = &mut self.engine {
                        engine.stop_audition();
                    }
                }
            }
        }
        // Consume focus/cursor stops before accepting a worker result from
        // this frame. That ordering prevents a just-finished stale decode
        // from sounding for one callback block on the way out of the panel.
        if let Some(result) = self.audition_loader.try_result() {
            match result {
                Ok(buffer) => {
                    if let Some(engine) = &mut self.engine {
                        engine.audition(buffer);
                    }
                }
                Err(error) => self.notice = Some(format!("sample audition refused: {error}")),
            }
        }

        let arrangement_theme = redesign_arrangement_theme();
        let commands = self.commands();
        if let Some(choice) = self.palette.show(
            ui.ctx(),
            &arrangement_theme,
            &commands,
            App::typed_commands(),
        ) {
            match choice {
                PaletteChoice::Command(id) => self.run_command(id, &mut actions),
                PaletteChoice::Typed(line) => self.run_typed_command(&line),
            }
        } else if !self.palette.is_open()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Colon))
        {
            self.palette.open();
        }
        let palette_open = self.palette.is_open();

        if outcome.arrangement_focused && !palette_open && !self.center_song {
            arrangement_keys(ui.ctx(), &self.arrangement, &mut actions);
        }
        track_rename_keys(ui.ctx(), &mut self.arrangement);
        self.focus.begin_enabled(
            ui.ctx(),
            outcome.arrangement_focused && !palette_open && !self.center_song,
        );

        let center = ui.available_rect_before_wrap();
        if ui.ctx().input(|input| input.pointer.any_pressed())
            && ui
                .ctx()
                .pointer_latest_pos()
                .is_some_and(|pointer| center.contains(pointer))
        {
            if matches!(self.snap_preview, Some(SnapPreview::MidiResult { .. })) {
                self.snap_preview = None;
            }
            self.redesign.focus_arrangement();
        }

        // Preferences (library roots live there) opens from the palette
        // and must draw in this mode too, not only on the legacy path.
        self.draw_preferences(ui.ctx());

        // The center swap: F10 trades the legacy timeline for the SONG
        // arrangement — the new world, made audible by the projection.
        // Both occupy one address; the legacy occupant keeps all its
        // duties (drag-import, automation) until they grow redesign
        // equivalents.
        // Every parameter this track's automation lane may aim at: the
        // two mixer targets, then each device in its chain. Built here
        // because only the app knows the chain; the lane is handed a list
        // and never goes looking.
        let automation_targets = self.automation_targets();
        if self.center_song {
            let _song_outcome = ui
                .scope_builder(egui::UiBuilder::new().max_rect(center), |ui| {
                    self.redesign.show_arrangement(
                        ui,
                        daw::ui::redesign::arrangement::View {
                            automation_targets: &automation_targets,
                            punch: self.transport.punch,
                            song: &mut self.song,
                            playhead_beats,
                            playing: self.transport.playing,
                        },
                    )
                })
                .inner;
            // The lower track head follows the Song cursor. Its legacy twin
            // remains the chain owner, but the address is chosen in the new
            // world; otherwise ARM on lane two would still arm lane one.
            if let Some(song_index) = self.redesign.selected_song_track(&self.song)
                && let Some(legacy) = self
                    .song
                    .tracks
                    .get(song_index)
                    .and_then(|track| self.song_track_map.get(&track.id))
                    .copied()
            {
                self.arrangement.selected_clip = None;
                self.arrangement.selected = Some(legacy);
            }
            self.focus.end(ui, &arrangement_theme);
            self.project_song();
            self.finish_redesign_actions(ui.ctx(), actions);
            return;
        }
        let transport_view = ArrangementTransportView {
            beats_per_bar: self.transport.beats_per_bar,
            bpm: self.transport.bpm,
            playhead: playhead_beats as f32,
            follow: self.transport.follow,
        };
        let mut close_automation_editor = false;
        let arrangement_outcome = ui
            .scope_builder(egui::UiBuilder::new().max_rect(center), |ui| {
                if self.automation_editor {
                    close_automation_editor = automation_editor_body(
                        ui,
                        &arrangement_theme,
                        &mut self.arrangement,
                        self.transport.beats_per_bar,
                        &self.parameter_registry,
                        &mut self.automation_target,
                    );
                    ArrangementOutcome::default()
                } else {
                    arrangement_body(
                        ui,
                        &mut self.focus,
                        &arrangement_theme,
                        &mut self.arrangement,
                        transport_view,
                        &self.waveform_cache,
                        self.drag_import.as_mut(),
                        self.automation_mode,
                        &self.parameter_registry,
                        &mut self.automation_target,
                        &mut self.meters,
                        &mut self.master_meter,
                    )
                }
            })
            .inner;
        if close_automation_editor {
            self.automation_editor = false;
        }
        self.focus.end(ui, &arrangement_theme);

        self.automation_hovered = arrangement_outcome.automation_hovered;
        self.focused_clip = arrangement_outcome.focused_clip;
        self.bounce_in_place_requested |= arrangement_outcome.bounce_in_place;
        if arrangement_outcome.open_clip_editor {
            self.bottom_view = BottomView::ClipEditor;
        }
        if arrangement_outcome.panned {
            self.transport.follow = false;
        }
        if let Some((_, edit)) = arrangement_outcome.clip_fade {
            self.apply_clip_edit(edit);
        }
        if std::mem::take(&mut self.bounce_in_place_requested) {
            self.start_bounce_in_place(ui.ctx());
        }

        self.finish_redesign_actions(ui.ctx(), actions);
    }

    /// The action tail both center occupants share: transport wishes,
    /// clipboard, pending seeks, recording and the engine sync.
    fn finish_redesign_actions(&mut self, ctx: &egui::Context, actions: Vec<UiAction>) {
        let mut wishes = Vec::with_capacity(actions.len());
        for action in actions {
            match action {
                UiAction::StartEngine => self.start_engine(),
                UiAction::StopEngine => self.stop_engine(),
                UiAction::AddReturn if self.center_song => {
                    if let Some(index) = self.song.add_return() {
                        // The chain is projection-only, so mint its legacy
                        // owner in the same edit. The next projection writes
                        // Song's four facts over it unconditionally.
                        if self.arrangement.returns.len() == index {
                            self.arrangement.add_return();
                        }
                        self.arrangement.select_return(index);
                        self.arrangement.force_recompile = true;
                        self.projected_song = None;
                        self.notice = Some(format!(
                            "RETURN {}: ADDED",
                            daw::sequencing::ReturnTrack::letter(index)
                        ));
                    } else {
                        self.notice = Some("RETURN: A THROUGH H ALREADY EXIST".to_owned());
                    }
                }
                UiAction::RemoveReturn if self.center_song => {
                    let Some(index) = self.arrangement.return_selected else {
                        self.notice = Some("DELETE RETURN: NOTHING SHOWN".to_owned());
                        continue;
                    };
                    if self.song.remove_return(index) {
                        // Remove this SAME positional bus now, while the edit
                        // still names it. A later length-only projection
                        // could only truncate the tail and would discard the
                        // wrong effects chain when B is deleted before C.
                        self.arrangement.remove_return(index);
                        self.arrangement.force_recompile = true;
                        self.projected_song = None;
                        self.notice = Some(format!(
                            "RETURN {}: DELETED",
                            daw::sequencing::ReturnTrack::letter(index)
                        ));
                    } else {
                        self.notice = Some("DELETE RETURN: NO RETURN THERE".to_owned());
                    }
                }
                UiAction::Undo | UiAction::Redo => {
                    let stepped = if matches!(action, UiAction::Undo) {
                        self.history.undo(&mut self.arrangement)
                    } else {
                        self.history.redo(&mut self.arrangement)
                    };
                    if stepped {
                        self.push_graph();
                    }
                }
                UiAction::ToggleBrowser => {
                    self.prefs.browser_hidden = !self.prefs.browser_hidden;
                }
                UiAction::ToggleLower => {
                    self.prefs.lower_hidden = !self.prefs.lower_hidden;
                }
                UiAction::ToggleChrome => {
                    let clearing = !self.prefs.browser_hidden || !self.prefs.lower_hidden;
                    self.prefs.browser_hidden = clearing;
                    self.prefs.lower_hidden = clearing;
                }
                UiAction::ReverseAudio => {
                    if let Some(clip) = self.arrangement.active_audio_clip()
                        && let Some(audio) = clip.audio.as_ref()
                    {
                        let from = audio.source_offset;
                        let to = audio.source_offset + audio.source_frames;
                        if !self.request_render(
                            "reverse",
                            vec![daw::render::Op::Reverse {
                                from,
                                to,
                                channels: daw::render::Channels::all(),
                            }],
                            None,
                        ) {
                            self.notice =
                                Some("could not reverse — a render is already running".to_owned());
                        }
                    }
                }
                UiAction::CropFocusedClip => self.crop_focused_clip(),
                other => wishes.push(other),
            }
        }
        self.route_transport(&wishes);
        let copied = wishes
            .iter()
            .any(|action| matches!(action, UiAction::CopyClip | UiAction::CutClip));
        perform(&wishes, &mut self.transport, &mut self.arrangement);
        if copied {
            let summary = self.arrangement.clipboard_summary();
            if !summary.is_empty() {
                ctx.copy_text(summary);
            }
        }

        let pointed = self.arrangement.pending_point.take();
        if let Some(beat) = pointed {
            self.transport.marker = beat;
        }
        let locate = self
            .arrangement
            .pending_seek
            .take()
            .or_else(|| pointed.filter(|_| !self.transport.playing));
        if let Some(beat) = locate {
            let seconds = f64::from(beat) * 60.0 / self.transport.bpm.max(1.0);
            self.transport.position = seconds;
            self.transport.marker = beat;
            if let Some(engine) = &mut self.engine {
                engine.transport(TransportCmd::Seek(
                    (seconds * f64::from(engine.info().sample_rate)) as u64,
                ));
            }
        }
        self.drive_recording();
        self.sync_engine();
    }
}

// --- the typed palette long forms ---
//
// Sentences too long for keys (`notes/20260831-command-grammar.md` §7):
// the palette recognizes the first word and hands the whole line here.
// Everything acts on the ambient contexts — the project key, the
// arrangement cursor's track, the sequence cursor's trig — with no
// dialogs anywhere.

fn parse_track_input(args: &[&str], channels: u32) -> Result<daw::sequencing::TrackInput, String> {
    let [route] = args else {
        return Err("INPUT: SAY NONE, A CHANNEL, OR L/R — :input 1/2".to_owned());
    };
    if route.eq_ignore_ascii_case("none") {
        return Ok(daw::sequencing::TrackInput::None);
    }

    let parse_channel = |word: &str| -> Result<u32, String> {
        let Ok(one_based) = word.parse::<u32>() else {
            return Err(format!("INPUT: {word} IS NOT A CHANNEL"));
        };
        if channels == 0 {
            return Err("INPUT: ENGINE OFF — NO CHANNELS TO CHOOSE".to_owned());
        }
        if !(1..=channels).contains(&one_based) {
            return Err(format!(
                "INPUT: CHANNEL {one_based} IS NOT AVAILABLE (1..{channels})"
            ));
        }
        Ok(one_based - 1)
    };

    if let Some((left, right)) = route.split_once('/') {
        if right.contains('/') || left.is_empty() || right.is_empty() {
            return Err("INPUT: STEREO IS L/R — :input 1/2".to_owned());
        }
        let left = parse_channel(left)?;
        let right = parse_channel(right)?;
        if left == right {
            return Err("INPUT: LEFT AND RIGHT MUST DIFFER".to_owned());
        }
        return Ok(daw::sequencing::TrackInput::Stereo(left, right));
    }

    parse_channel(route).map(daw::sequencing::TrackInput::Mono)
}

impl App {
    pub(super) fn typed_commands() -> &'static [PaletteTyped] {
        &[
            PaletteTyped {
                name: "key",
                usage: "key <tonic> <scale> [mode N] — set the harmonic context",
            },
            PaletteTyped {
                name: "lens",
                usage: "lens <name> — how this track spells pitch",
            },
            PaletteTyped {
                name: "tempo",
                usage: "tempo <bpm> | tempo clear — a tempo change at the cursor",
            },
            PaletteTyped {
                name: "countin",
                usage: "countin <bars> | countin off — bars before a take writes",
            },
            PaletteTyped {
                name: "punch",
                usage: "punch <in> <out> | punch clear — capture only inside these beats",
            },
            PaletteTyped {
                name: "input",
                usage: "input <channel|L/R|none> — route the selected audio track",
            },
            PaletteTyped {
                name: "tune",
                usage: "tune <±cents> — bend the selected trig (additive)",
            },
            PaletteTyped {
                name: "push",
                usage: "push <±ticks> — displace the selected trig in time",
            },
            PaletteTyped {
                name: "quantize-key",
                usage: "quantize-key — re-address the trig onto the key, sound-preserving",
            },
            PaletteTyped {
                name: "free",
                usage: "free — release the trig from the key, sound-preserving",
            },
            PaletteTyped {
                name: "snap-key",
                usage: "snap-key — snap the trig onto the key (LOSSY, previews first)",
            },
        ]
    }

    /// A scale by name: built-ins first, then the library's `.scl`
    /// files. A garbage file refuses HERE, with words, at the moment it
    /// is asked for — never during the scan.
    fn scale_by_name(&self, stem: &str) -> Option<Result<daw::pitch::Scale, String>> {
        if let Some(scale) = daw::pitch::builtin_scale(stem) {
            return Some(Ok(scale));
        }
        let record = self
            .library_snapshot
            .scales
            .iter()
            .find(|scale| scale.name.eq_ignore_ascii_case(stem))?;
        Some(match std::fs::read_to_string(&record.path) {
            Ok(source) => {
                daw::pitch::parse_scl(&record.name, &source).map_err(|error| format!("{error}"))
            }
            Err(error) => Err(format!("cannot read {}: {error}", record.path.display())),
        })
    }

    /// A `.lens` file by name, through a per-generation cache so an
    /// active user lens costs one read per rescan, not one per frame.
    pub(super) fn cached_lens_file(
        &mut self,
        stem: &str,
    ) -> Option<Result<daw::ui::redesign::lens::Lens, String>> {
        if self.lens_cache_generation != self.library_snapshot.generation {
            self.lens_cache.clear();
            self.lens_cache_generation = self.library_snapshot.generation;
        }
        let key = stem.to_ascii_lowercase();
        if !self.lens_cache.contains_key(&key) {
            let loaded = lens_file_by_name(&self.library_snapshot, &key);
            self.lens_cache.insert(key.clone(), loaded);
        }
        self.lens_cache.get(&key).cloned().flatten()
    }

    /// The universal selection the pitch long forms act on: the song
    /// pattern under the arrangement cursor, at the sequence cursor's
    /// step. Refusals are words, prefixed by the caller's verb.
    fn selected_song_trig(&mut self) -> Result<(daw::sequencing::PatternId, usize), String> {
        if !self.center_song {
            return Err("SONG VIEW ONLY — F10".to_owned());
        }
        let pattern = match self.snap_preview.as_ref() {
            Some(SnapPreview::MidiResult { pattern }) => *pattern,
            _ => self
                .redesign
                .selected_song_pattern(&self.song)
                .ok_or_else(|| "NO PATTERN UNDER THE CURSOR".to_owned())?,
        };
        let step = self.sequence_cursor_tick / SONG_PATTERN_STEP_TICKS;
        if step >= daw::sequencing::PATTERN_STEPS {
            return Err("THE CURSOR IS OUTSIDE THE PATTERN".to_owned());
        }
        Ok((pattern, step))
    }

    /// Run one closure over every note of the selected trig; empty trigs
    /// refuse. The selection stays where it is — the result remains the
    /// working selection (composition contract, closure).
    fn edit_selected_trig(
        &mut self,
        verb: &str,
        edit: impl Fn(&mut daw::sequencing::Note, &daw::pitch::Key),
    ) -> Result<usize, String> {
        let (pattern_id, step) = self
            .selected_song_trig()
            .map_err(|error| format!("{verb}: {error}"))?;
        let key = self.song.key.clone();
        let Some(pattern) = self.song.pattern_mut(pattern_id) else {
            return Err(format!("{verb}: THE PATTERN IS GONE"));
        };
        let trig = pattern.trig_mut(step);
        if trig.notes.is_empty() {
            return Err(format!("{verb}: NOTHING HERE"));
        }
        for note in &mut trig.notes {
            edit(note, &key);
        }
        Ok(trig.notes.len())
    }

    /// A tempo change at the arrangement cursor.
    ///
    /// Tempo is its own authority rather than an automation target — an
    /// envelope is read AT a tick, and tempo decides what a tick is worth
    /// — so it gets its own verb rather than riding the lane. A number is
    /// an argument, which is what the typed palette is for.
    fn typed_tempo(&mut self, args: &[&str]) -> Result<String, String> {
        let (start_beats, _) = self.redesign.song_selection_beats(&self.song);
        let tick = beats_to_sequence_ticks(f64::from(start_beats));
        match args.first().copied() {
            None => Err("TEMPO: SAY A BPM, OR CLEAR".to_owned()),
            Some("clear") => {
                if self.song.remove_tempo_mark(tick) {
                    self.projected_song = None;
                    Ok("TEMPO: MARK CLEARED".to_owned())
                } else {
                    Err("TEMPO: NO MARK HERE".to_owned())
                }
            }
            Some(word) => {
                let Ok(bpm) = word.parse::<f64>() else {
                    return Err(format!("TEMPO: {word} IS NOT A BPM"));
                };
                if self.song.set_tempo_mark(tick, bpm) {
                    // The projection warps against the map, so a new mark
                    // has to reach the compiler.
                    self.projected_song = None;
                    Ok(format!("TEMPO: {bpm:.2} FROM HERE"))
                } else {
                    Err(format!("TEMPO: {bpm} IS NOT A TEMPO"))
                }
            }
        }
    }

    /// Bars of count-in before a take starts writing.
    fn typed_count_in(&mut self, args: &[&str]) -> Result<String, String> {
        match args.first().copied() {
            None => Err("COUNTIN: SAY A NUMBER OF BARS, OR OFF".to_owned()),
            Some("off") | Some("0") => {
                self.transport.count_in_bars = 0;
                Ok("COUNTIN: OFF".to_owned())
            }
            Some(word) => match word.parse::<u32>() {
                // A ceiling, because a count-in nobody would wait through
                // is a typo rather than an intention.
                Ok(bars) if (1..=8).contains(&bars) => {
                    self.transport.count_in_bars = bars;
                    Ok(format!("COUNTIN: {bars} BAR"))
                }
                _ => Err(format!("COUNTIN: {word} IS NOT 1 TO 8 BARS")),
            },
        }
    }

    /// The beats a take is allowed to write inside.
    fn typed_punch(&mut self, args: &[&str]) -> Result<String, String> {
        match args {
            [] => Err("PUNCH: SAY IN AND OUT, OR CLEAR".to_owned()),
            ["clear"] => {
                self.transport.set_punch(None);
                Ok("PUNCH: CLEARED".to_owned())
            }
            [start, end] => {
                let (Ok(start), Ok(end)) = (start.parse::<f32>(), end.parse::<f32>()) else {
                    return Err("PUNCH: IN AND OUT ARE BEATS".to_owned());
                };
                if self.transport.set_punch(Some((start, end))) {
                    Ok(format!("PUNCH: {start} TO {end}"))
                } else {
                    // Refused rather than stored: a window that can never
                    // capture arms a recording that never writes, which
                    // on stage looks exactly like broken hardware.
                    Err("PUNCH: THAT WINDOW CANNOT CAPTURE".to_owned())
                }
            }
            _ => Err("PUNCH: SAY IN AND OUT, OR CLEAR".to_owned()),
        }
    }

    /// Route the selected Song audio track from the currently open
    /// interface. The project may remember unavailable channels, but an EDIT
    /// may not invent one: a command must land in hardware that exists now.
    fn typed_input(&mut self, args: &[&str]) -> Result<String, String> {
        if !self.center_song {
            return Err("INPUT: SONG VIEW ONLY — F10".to_owned());
        }
        let Some(index) = self.song_track_for_active_chain() else {
            return Err("INPUT: NO SONG TRACK".to_owned());
        };
        let Some(track) = self.song.tracks.get(index) else {
            return Err("INPUT: NO SONG TRACK".to_owned());
        };
        if track.kind != daw::sequencing::TrackKind::Audio || track.is_group {
            return Err("INPUT: AUDIO TRACKS ONLY".to_owned());
        }

        let route = parse_track_input(args, self.input_channels())?;
        let track = &mut self.song.tracks[index];
        track.input = route;
        self.projected_song = None;
        Ok(format!("INPUT: {}", route.label()))
    }

    pub(super) fn run_typed_command(&mut self, line: &str) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&name, args)) = words.split_first() else {
            return;
        };
        let outcome: Result<String, String> = match name.to_ascii_lowercase().as_str() {
            "key" => {
                let lookup = |stem: &str| self.scale_by_name(stem);
                daw::pitch::parse_key_command(args, &self.song.key, &lookup).map(|key| {
                    let sign = daw::ui::redesign::lens::key_sign(&key);
                    self.song.key = key;
                    format!("KEY: {sign}")
                })
            }
            "lens" => self.typed_lens(args),
            "tempo" => self.typed_tempo(args),
            "countin" => self.typed_count_in(args),
            "punch" => self.typed_punch(args),
            "input" => self.typed_input(args),
            "tune" => match args.first().and_then(|cents| cents.parse::<f32>().ok()) {
                Some(cents) if cents.is_finite() => self
                    .edit_selected_trig("TUNE", |note, _| {
                        note.pitch.offset_cents += cents;
                    })
                    .map(|notes| format!("TUNE: {cents:+.0}¢ ON {notes} NOTES")),
                _ => Err("TUNE: SIGNED CENTS — :tune +14".to_owned()),
            },
            "push" => match args.first().and_then(|ticks| ticks.parse::<i16>().ok()) {
                Some(ticks) => {
                    // Sub-step displacement: a push past the step is a
                    // nudge wearing the wrong verb.
                    let limit = (SONG_PATTERN_STEP_TICKS - 1) as i16;
                    self.edit_selected_trig("PUSH", |note, _| {
                        note.micro_ticks =
                            note.micro_ticks.saturating_add(ticks).clamp(-limit, limit);
                    })
                    .map(|notes| format!("PUSH: {ticks:+}T ON {notes} NOTES"))
                }
                None => Err("PUSH: SIGNED TICKS — :push -3".to_owned()),
            },
            "quantize-key" => self
                .edit_selected_trig("QUANTIZE-KEY", |note, key| {
                    note.pitch = note.pitch.quantize_to(key);
                })
                .map(|notes| format!("QUANTIZE-KEY: {notes} NOTES RE-ADDRESSED, SOUND HELD")),
            "free" => self
                .edit_selected_trig("FREE", |note, key| {
                    note.pitch = note.pitch.free(key);
                })
                .map(|notes| format!("FREE: {notes} NOTES RELEASED, SOUND HELD")),
            "snap-key" => self.typed_snap_key(),
            other => Err(format!("{}: NOT YET SPOKEN", other.to_ascii_uppercase())),
        };
        self.notice = Some(match outcome {
            Ok(answer) => answer,
            Err(refusal) => refusal,
        });
    }

    fn typed_lens(&mut self, args: &[&str]) -> Result<String, String> {
        let Some(&name) = args.first() else {
            return Err(format!(
                "LENS: NAME ONE OF {} OR A .lens FILE",
                daw::ui::redesign::lens::BUILTIN_LENSES.join("/")
            ));
        };
        if !self.center_song {
            return Err("LENS: SONG VIEW ONLY — F10".to_owned());
        }
        let name_lower = name.to_ascii_lowercase();
        let known_builtin = daw::ui::redesign::lens::BUILTIN_LENSES.contains(&name_lower.as_str());
        if !known_builtin {
            match self.cached_lens_file(&name_lower) {
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(format!("LENS: {name} — {error}")),
                None => return Err(format!("LENS: NO LENS NAMED {name}")),
            }
        }
        let track = self
            .redesign
            .selected_song_track(&self.song)
            .and_then(|track| self.song.tracks.get(track))
            .ok_or_else(|| "LENS: NO TRACK UNDER THE CURSOR".to_owned())?;
        let track_name = track.name.clone();
        self.track_lenses.insert(track.id.0, name_lower.clone());
        Ok(format!(
            "LENS: {} READS {}",
            track_name,
            name_lower.to_ascii_uppercase()
        ))
    }

    /// `:snap-key` — quantize, then zero the pitch offsets. LOSSY, so it
    /// lands as ghosts first; Enter commits, Escape cancels.
    fn typed_snap_key(&mut self) -> Result<String, String> {
        let (pattern_id, step) = self
            .selected_song_trig()
            .map_err(|error| format!("SNAP-KEY: {error}"))?;
        let key = self.song.key.clone();
        let Some(pattern) = self.song.pattern(pattern_id) else {
            return Err("SNAP-KEY: THE PATTERN IS GONE".to_owned());
        };
        let trig = pattern.trig(step);
        if trig.notes.is_empty() {
            return Err("SNAP-KEY: NOTHING HERE".to_owned());
        }
        let after: Vec<daw::sequencing::Note> = trig
            .notes
            .iter()
            .map(|note| {
                let mut snapped = note.clone();
                snapped.pitch = note.pitch.quantize_to(&key);
                snapped.pitch.offset_cents = 0.0;
                snapped
            })
            .collect();
        self.snap_preview = Some(SnapPreview::Key {
            pattern: pattern_id,
            step,
            after,
        });
        Ok("SNAP-KEY: GHOSTS SHOW THE LOSS — ENTER COMMITS · ESC CANCELS".to_owned())
    }

    /// Enter, while a snap preview stands: the one moment loss is chosen.
    pub(crate) fn commit_snap_preview(&mut self) {
        let Some(preview) = self.snap_preview.take() else {
            return;
        };
        match preview {
            SnapPreview::Key {
                pattern,
                step,
                after,
            } => {
                if let Some(pattern) = self.song.pattern_mut(pattern) {
                    pattern.trig_mut(step).notes = after;
                    self.notice = Some("SNAP-KEY: SNAPPED ONTO THE KEY".to_owned());
                } else {
                    self.notice = Some("SNAP-KEY: THE PATTERN IS GONE".to_owned());
                }
            }
            SnapPreview::Midi {
                pattern: pattern_id,
                writes,
                grid,
            } => {
                let first = writes.first().map(|write| write.step);
                let count = writes.len();
                if let Some(pattern) = self.song.pattern_mut(pattern_id) {
                    for write in writes {
                        *pattern.trig_mut(write.step) = write.after;
                    }
                    if let Some(step) = first {
                        self.sequence_cursor_tick = step * SONG_PATTERN_STEP_TICKS;
                    }
                    self.projected_song = None;
                    self.notice = Some(format!(
                        "MIDI TAKE: WROTE {count} TRIG(S) ON {grid} — RESULT SELECTED"
                    ));
                    self.snap_preview = Some(SnapPreview::MidiResult {
                        pattern: pattern_id,
                    });
                } else {
                    self.notice =
                        Some("MIDI TAKE: THE PATTERN IS GONE — NOTHING CHANGED".to_owned());
                }
            }
            result @ SnapPreview::MidiResult { .. } => {
                self.snap_preview = Some(result);
            }
        }
    }
}

// --- the C1 copyist: Song → legacy clips ---
//
// Phase C1 of the decided Song bridge (notes/20260831-song-bridge-brief.md,
// notes/20260831-projection-c1-spec.md). The canonical tick-based `Song` is
// made audible by copying it into legacy clips, which the existing compile
// path already turns into sound — decide-new, play-old, exactly as
// `session_bridge` did for the Session view. The projection is the ONLY
// writer of the legacy tracks it creates; a stray legacy edit to an owned
// clip is erased on the next pass rather than silently kept.

impl App {
    /// Ensure every Song lane has a projection twin. Creation is separate
    /// from copying so all twins can be put into Song order before positional
    /// group depths are written onto them.
    fn ensure_song_twins(&mut self, song: &daw::sequencing::Song) {
        for song_track in &song.tracks {
            if self
                .song_track_map
                .get(&song_track.id)
                .is_some_and(|index| *index < self.arrangement.tracks.len())
            {
                continue;
            }

            // A group is audio-kinded on the legacy side because it has no
            // notes and takes no instrument. Otherwise the twin carries the
            // Song track's kind so audio clips cannot land on a MIDI lane.
            let audio = song_track.is_group || song_track.kind == daw::sequencing::TrackKind::Audio;
            let index = self.arrangement.add_track(if audio {
                TrackKind::Audio
            } else {
                TrackKind::Midi
            });
            // An instrument track is born able to speak. A group starts at
            // its sum, and an audio track already IS the sound.
            if !audio {
                let instance = DeviceInstance {
                    id: self.arrangement.mint_id(),
                    parent: None,
                    state: DeviceState::new(DeviceKind::Poly),
                    bypass: false,
                    page: 0,
                    view_zoom: unit_zoom(),
                    view_scroll: 0.0,
                };
                if let Some(track) = self.arrangement.tracks.get_mut(index) {
                    track.chain.push(instance);
                }
            }
            self.song_track_map.insert(song_track.id, index);
        }
    }

    /// Move a projection twin through the legacy arrangement and keep every
    /// Song id's index map on the same move. `Arrangement::move_track` carries
    /// all of its parallel data and index-addressed state in one operation.
    fn move_song_twin(&mut self, from: usize, to: usize) -> bool {
        if !self.arrangement.move_track(from, to) {
            return false;
        }
        let shifted = |index: usize| {
            if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        for index in self.song_track_map.values_mut() {
            *index = shifted(*index);
        }
        true
    }

    /// Positional group membership requires the twins to follow Song order.
    /// Projection-created lanes live as one owned run at the tail; unrelated
    /// legacy tracks keep their relative order and are never claimed.
    fn order_song_twins(&mut self, song: &daw::sequencing::Song) {
        if song.tracks.is_empty() || song.tracks.len() > self.arrangement.tracks.len() {
            return;
        }
        let tail = self.arrangement.tracks.len() - song.tracks.len();
        let already_ordered =
            song.tracks.iter().enumerate().all(|(offset, track)| {
                self.song_track_map.get(&track.id) == Some(&(tail + offset))
            });
        if already_ordered {
            return;
        }

        let ids: Vec<daw::sequencing::TrackId> = song.tracks.iter().map(|track| track.id).collect();
        let last = self.arrangement.tracks.len() - 1;
        for id in ids {
            let Some(&from) = self.song_track_map.get(&id) else {
                continue;
            };
            if from != last {
                self.move_song_twin(from, last);
            }
        }
    }

    /// Copy the song into its owned legacy tracks when it has changed.
    /// Runs green-zone, once per frame at most; the equality guard makes
    /// the idle cost one comparison of a small struct.
    pub(super) fn project_song(&mut self) {
        if self.projected_song.as_ref() == Some(&self.song) {
            return;
        }
        // Never touch the project before the song holds a first real
        // edit: an untouched default song must not mint tracks.
        if self.projected_song.is_none() && self.song == daw::sequencing::Song::default() {
            return;
        }
        let song = self.song.clone();
        // Song is the authority for the return list once it has a real edit.
        // Grow and shrink only the legacy buses; their effects chains stay
        // where they are, while these four duplicated mixer facts are always
        // overwritten from Song so load order cannot choose the winner.
        self.arrangement
            .returns
            .resize_with(song.returns.len(), ReturnTrack::default);
        for (source, projected) in song.returns.iter().zip(&mut self.arrangement.returns) {
            projected.name = source.name.clone();
            projected.mute = source.mute;
            projected.volume = source.volume;
            projected.pan = source.pan;
        }
        if self
            .arrangement
            .return_selected
            .is_some_and(|index| index >= song.returns.len())
        {
            self.arrangement.return_selected = None;
        }

        self.ensure_song_twins(&song);
        self.order_song_twins(&song);
        let any_solo = song.tracks.iter().any(|track| track.solo);
        // The tempo map, resolved once for the whole projection. The
        // reference is the transport's own tempo, so an empty map warps
        // nothing at all.
        let reference_bpm = if self.transport.bpm.is_finite() && self.transport.bpm > 0.0 {
            self.transport.bpm
        } else {
            120.0
        };
        let tempo = daw::tempo::TempoTable::build(&song, WARP_SAMPLE_RATE, reference_bpm);
        let samples_per_beat = WARP_SAMPLE_RATE * 60.0 / reference_bpm;
        for (song_index, song_track) in song.tracks.iter().enumerate() {
            let Some(&legacy) = self.song_track_map.get(&song_track.id) else {
                continue;
            };
            let legacy_kind =
                if song_track.is_group || song_track.kind == daw::sequencing::TrackKind::Audio {
                    TrackKind::Audio
                } else {
                    TrackKind::Midi
                };
            let needs_instrument = legacy_kind == TrackKind::Midi
                && !self.arrangement.tracks[legacy]
                    .chain
                    .iter()
                    .any(|device| device.kind().is_instrument());
            let instrument = needs_instrument.then(|| DeviceInstance {
                id: self.arrangement.mint_id(),
                parent: None,
                state: DeviceState::new(DeviceKind::Poly),
                bypass: false,
                page: 0,
                view_zoom: unit_zoom(),
                view_scroll: 0.0,
            });
            let projected = &mut self.arrangement.tracks[legacy];
            projected.kind = legacy_kind;
            if legacy_kind == TrackKind::Audio {
                projected
                    .chain
                    .retain(|device| !device.kind().is_instrument());
            } else if let Some(instrument) = instrument {
                projected.chain.insert(0, instrument);
            }
            // The name follows the song track (the rename verb travels),
            // carrying the ownership sign: legacy editors, look only.
            projected.name = format!("{} §", song_track.name);
            // The legacy compiler already removes muted tracks from the
            // schedule. Project the Song's solo-precedence rule onto that
            // real silence path; keep legacy solo off so Song solo cannot
            // accidentally silence unrelated legacy tracks.
            projected.mute = !song_track_audible(&song.tracks, song_index, any_solo);
            projected.solo = false;
            // The curves travel with the track. Song automation is the
            // offset model's BASE, and the legacy compiler already bakes
            // envelopes into ramps (`automation_letters`) — so a curve
            // drawn in the redesign is audible through C1 with no engine
            // work at all, exactly as p-locks ride the same bridge.
            projected.automation = project_automation(song_track);
            // The mixer values travel as the BASE the envelopes bend. The
            // legacy compiler reads the static fader and then applies
            // `track.volume` / `track.pan` on top of it, which is exactly
            // the offset model's shape — so the Song's knob and the Song's
            // curve arrive as one already-agreeing pair.
            projected.volume = song_track.volume;
            projected.pan = song_track.pan;
            projected.sends = song_track.sends.clone();
            projected.is_group = song_track.is_group;
            projected.folded = song_track.folded;
            projected.depth = song_track.depth;
            projected.input = song_track.input;
            projected.monitor = song_track.monitor;
            projected.armed = song_track.armed;
            let mut clips = Vec::with_capacity(song_track.blocks.len());
            for block in &song_track.blocks {
                let id = self.arrangement.next_clip_id;
                self.arrangement.next_clip_id += 1;
                clips.push(project_block(&song, block, id, &tempo, samples_per_beat));
            }
            // Landed sound travels the same road. The legacy compiler
            // already streams an audio clip (compile.rs's audio lane), so
            // an AudioBlock reaches the speakers through C1 with no
            // engine work — exactly as notes and curves do.
            for block in &song_track.audio_blocks {
                let id = self.arrangement.next_clip_id;
                self.arrangement.next_clip_id += 1;
                clips.push(project_audio_block(block, id, &tempo, samples_per_beat));
            }
            // One lane, one time order. The legacy side assumes a track's
            // clips are sorted by start (arrangement.rs says so outright,
            // and place_clip uses partition_point), so the two lists must
            // be interleaved rather than concatenated.
            clips.sort_by(|left, right| {
                left.start
                    .partial_cmp(&right.start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            self.arrangement.clips[legacy] = clips;
        }
        self.arrangement.force_recompile = true;
        self.projected_song = Some(song);
    }
}

/// A fader level as DECIBELS, because that is what a musician reads.
/// Unity is exactly 0.0 dB and silence says so in words rather than as a
/// number nobody can act on.
fn format_gain_db(linear: f32) -> String {
    if linear <= 0.0 {
        return "-INF".to_owned();
    }
    let db = 20.0 * linear.max(1e-6).log10();
    if db.abs() < 0.05 {
        "0.0dB".to_owned()
    } else {
        format!("{db:+.1}dB")
    }
}

/// Pan as its position, with an exact CENTRE that reads as one character.
/// A centred pan is a fact worth seeing at a glance, and "0.00" makes the
/// reader do arithmetic to learn it.
fn format_pan(pan: f32) -> String {
    if pan.abs() < 0.005 {
        return "C".to_owned();
    }
    let side = if pan < 0.0 { 'L' } else { 'R' };
    format!("{side}{:.0}", pan.abs() * 100.0)
}

/// The Song track whose legacy twin is the chain's active track.
///
/// The chain is addressed through the legacy arrangement, but a track's
/// mixer values live on the Song. This is the one place the two are
/// reconciled, by walking the map the projection already maintains.
impl App {
    /// Everything the automation lane may be aimed at on the active
    /// track: the mixer's two targets, then every parameter of every
    /// device in its chain.
    ///
    /// Device targets name an INSTANCE — `dev.7.reverb.mix` — so a chain
    /// can be reordered without a curve following the wrong device.
    fn automation_targets(&self) -> Vec<daw::ui::redesign::arrangement::automation::TargetOption> {
        use daw::ui::redesign::arrangement::automation::TargetOption;
        let mut targets = vec![
            TargetOption {
                id: daw::sequencing::TRACK_VOLUME.to_owned(),
                label: "LEVEL".to_owned(),
                group: "TRACK".to_owned(),
            },
            TargetOption {
                id: daw::sequencing::TRACK_PAN.to_owned(),
                label: "PAN".to_owned(),
                group: "TRACK".to_owned(),
            },
        ];
        for index in 0..self.song.returns.len() {
            let Some(id) = daw::targets::track_send_target(index) else {
                continue;
            };
            targets.push(TargetOption {
                id: id.to_owned(),
                label: format!("SEND {}", daw::sequencing::ReturnTrack::letter(index)),
                group: "TRACK".to_owned(),
            });
        }
        let Some(track) = self
            .arrangement
            .active_track()
            .and_then(|index| self.arrangement.tracks.get(index))
        else {
            return targets;
        };
        for instance in &track.chain {
            let kind = instance.kind();
            let spec = kind.spec();
            for label in spec.labels {
                targets.push(TargetOption {
                    id: daw::targets::device_target(instance.id, spec, label.name),
                    label: label.name.to_ascii_uppercase(),
                    group: spec.name.to_ascii_uppercase(),
                });
            }
        }
        targets
    }

    fn song_track_for_active_chain(&self) -> Option<usize> {
        let legacy = self.arrangement.active_track()?;
        self.song
            .tracks
            .iter()
            .position(|track| self.song_track_map.get(&track.id) == Some(&legacy))
    }
}

fn song_parent_group(tracks: &[daw::sequencing::Track], index: usize) -> Option<usize> {
    let depth = tracks.get(index)?.depth;
    if depth == 0 {
        return None;
    }
    tracks[..index]
        .iter()
        .rposition(|track| track.is_group && track.depth + 1 == depth)
}

fn song_group_members(tracks: &[daw::sequencing::Track], index: usize) -> std::ops::Range<usize> {
    let Some(group) = tracks.get(index).filter(|track| track.is_group) else {
        return index..index;
    };
    let mut end = index + 1;
    while tracks
        .get(end)
        .is_some_and(|track| track.depth > group.depth)
    {
        end += 1;
    }
    index + 1..end
}

fn song_muted_in_place(tracks: &[daw::sequencing::Track], index: usize) -> bool {
    let mut at = index;
    loop {
        if tracks.get(at).is_some_and(|track| track.muted) {
            return true;
        }
        match song_parent_group(tracks, at) {
            Some(parent) => at = parent,
            None => return false,
        }
    }
}

fn song_solo_in_scope(tracks: &[daw::sequencing::Track], index: usize) -> bool {
    let Some(track) = tracks.get(index) else {
        return false;
    };
    if track.solo {
        return true;
    }
    let mut at = index;
    while let Some(parent) = song_parent_group(tracks, at) {
        if tracks[parent].solo {
            return true;
        }
        at = parent;
    }
    song_group_members(tracks, index).any(|member| song_solo_in_scope(tracks, member))
}

fn song_track_audible(tracks: &[daw::sequencing::Track], index: usize, any_solo: bool) -> bool {
    !song_muted_in_place(tracks, index) && (!any_solo || song_solo_in_scope(tracks, index))
}

/// Song automation, copied onto the legacy track the compiler reads.
///
/// The ONLY conversion is the time unit — song time is ticks, the legacy
/// envelope is beats — and it happens here and nowhere else, the same
/// one-way trip the blocks make. Values, bends and target ids cross
/// untouched: a curve has to sound like the one the redesign drew, and a
/// target id is file format on both sides of the bridge.
///
/// An envelope with no points is dropped rather than projected empty: an
/// automated-but-pointless target reads as the bare knob, which is what
/// `Track::value_at` already promises.
fn project_automation(track: &daw::sequencing::Track) -> TrackAutomation {
    use crate::automation::{AutomationEnvelope, AutomationPoint};
    const TICKS_PER_BEAT: f32 = daw::sequencing::TICKS_PER_BEAT as f32;
    TrackAutomation {
        envelopes: track
            .automation
            .iter()
            .filter(|envelope| !envelope.points.is_empty())
            .map(|envelope| AutomationEnvelope {
                target: envelope.target.clone(),
                points: envelope
                    .points
                    .iter()
                    .map(|point| AutomationPoint {
                        beat: point.tick as f32 / TICKS_PER_BEAT,
                        value: point.value,
                        bend: point.bend,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// One block, copied out as one legacy clip. A pattern shared by many
/// blocks projects into many clips: the duplication is the copyist's job
/// and invisible to the compile path.
fn project_block(
    song: &daw::sequencing::Song,
    block: &daw::sequencing::PatternBlock,
    id: u64,
    tempo: &daw::tempo::TempoTable,
    samples_per_beat: f64,
) -> Clip {
    use daw::sequencing::{PATTERN_STEPS, TICKS_PER_BEAT};
    let step_ticks = TICKS_PER_BEAT / 4;
    // The clip's own origin, warped once — every note is expressed
    // relative to it.
    let clip_beat = warped_beat(tempo, block.start_tick, samples_per_beat);
    let clip_end_beat = warped_beat(
        tempo,
        block.start_tick.saturating_add(block.length_ticks),
        samples_per_beat,
    );
    let mut notes = Vec::new();
    let mut name = String::new();
    if let Some(pattern) = song.pattern(block.pattern_id) {
        name = pattern.name.clone();
        for step in 0..PATTERN_STEPS {
            let start_tick = step * step_ticks;
            if start_tick >= block.length_ticks {
                break;
            }
            let trig = pattern.trig(step);
            if !trig.enabled {
                continue;
            }
            for note in &trig.notes {
                // Phase-1 pitch resolution (pitch-lens spec §6): the
                // stored address becomes Hz green-side, then the nearest
                // legacy MIDI pitch — exact for 12TET-embeddable scales,
                // approximate (and signed `≈` in the views) otherwise.
                // The micro push projects exactly: legacy starts are
                // fractional beats, so no time detail is lost.
                // Everything is warped through the tempo table, so a
                // tempo change mid-song lands the note on the sample the
                // map means. Clip-relative, because the clip's own start
                // is warped the same way just below.
                let pushed = start_tick as f64 + f64::from(note.micro_ticks);
                let absolute = block.start_tick as f64 + pushed.max(0.0);
                let note_beat = warped_beat(tempo, absolute.round() as usize, samples_per_beat);
                let note_end = warped_beat(
                    tempo,
                    (absolute + note.length_ticks.max(1) as f64).round() as usize,
                    samples_per_beat,
                );
                notes.push(Note {
                    pitch: daw::pitch::nearest_midi(note.pitch.resolve(&song.key)),
                    start: (note_beat - clip_beat).max(0.0),
                    len: (note_end - note_beat).max(1.0 / TICKS_PER_BEAT as f64),
                    vel: note.velocity,
                    muted: false,
                    plocks: Vec::new(),
                    prob: trig.probability,
                    cond: None,
                });
            }
        }
    }
    Clip {
        id,
        name,
        start: clip_beat as f32,
        len: (clip_end_beat - clip_beat).max(0.0) as f32,
        notes,
        ..Clip::default()
    }
}

/// One landed sound, copied out as one legacy audio clip.
///
/// The source crosses whole — it is the same `AudioSource` the legacy
/// clip model already carries, because the lift in e33610d made it one
/// type rather than two. Only the time units are converted, and through
/// the tempo table, so a sound keeps its real duration across a tempo
/// change.
fn project_audio_block(
    block: &daw::sequencing::AudioBlock,
    id: u64,
    tempo: &daw::tempo::TempoTable,
    samples_per_beat: f64,
) -> Clip {
    let start = warped_beat(tempo, block.start_tick, samples_per_beat);
    let end = warped_beat(tempo, block.end_tick(), samples_per_beat);
    // The carried name, not the path — an imported wav lives under a
    // content hash, so the path reads as a checksum rather than a sound.
    let name = if block.name.is_empty() {
        "audio".to_owned()
    } else {
        block.name.clone()
    };
    Clip {
        id,
        name,
        start: start as f32,
        len: (end - start).max(0.0) as f32,
        notes: Vec::new(),
        audio: Some(block.source.clone()),
        loop_on: block.loop_brace.is_some(),
        loop_start: block.loop_brace.map_or(0.0, |brace| {
            sequence_ticks_to_beats(brace.start_tick) as f32
        }),
        loop_len: block.loop_brace.map_or(0.0, |brace| {
            sequence_ticks_to_beats(brace.length_ticks) as f32
        }),
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use daw::sequencing::{Note as SongNote, PatternBlock, Song};

    /// Project at ONE steady tempo — what every test that is not about
    /// the tempo map wants, and the case in which the warp is the exact
    /// identity.
    fn project_block_steady(song: &Song, block: &PatternBlock, id: u64) -> Clip {
        let tempo = daw::tempo::TempoTable::build(song, WARP_SAMPLE_RATE, 120.0);
        project_block(song, block, id, &tempo, WARP_SAMPLE_RATE * 60.0 / 120.0)
    }

    fn song_with_trig() -> Song {
        let mut song = Song::default();
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        let pattern = song.pattern_mut(pattern_id).expect("default pattern");
        pattern.set_primary(4, SongNote::new(60, 12, 100));
        pattern.trig_mut(4).probability = 0.75;
        song
    }

    /// A finished import aimed at the Song lands there — and NEVER also
    /// falls through to the legacy arrangement. Two worlds must not both
    /// claim one import.
    #[test]
    fn a_finished_import_lands_on_the_song_track() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        // An audio track to land on.
        app.song.tracks.push(daw::sequencing::Track {
            id: daw::sequencing::TrackId(99),
            name: "AUDIO 01".to_owned(),
            kind: daw::sequencing::TrackKind::Audio,
            blocks: Vec::new(),
            audio_blocks: Vec::new(),
            muted: false,
            solo: false,
            pitch_authority: daw::sequencing::PitchAuthority::default(),
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            sends: Vec::new(),
            is_group: false,
            folded: false,
            depth: 0,
            input: daw::sequencing::TrackInput::None,
            monitor: daw::sequencing::Monitor::Off,
            armed: false,
        });
        let track = app.song.tracks.len() - 1;

        let imported = daw::library::ImportedWav {
            original_path: std::path::PathBuf::from("/samples/iron.wav"),
            path: std::path::PathBuf::from("/cache/iron.wav"),
            sample_rate: 48_000,
            frames: 24_000,
        };
        app.finish_song_landing(&imported, track, 0);

        let landed = &app.song.tracks[track].audio_blocks;
        assert_eq!(landed.len(), 1, "the sound landed on the song track");
        assert_eq!(landed[0].source.source_frames, 24_000);
        // Half a second at 120bpm is one beat.
        assert_eq!(landed[0].length_ticks, daw::sequencing::TICKS_PER_BEAT);
        assert!(
            app.notice.as_deref().is_some_and(|n| n.contains("iron")),
            "and it said so by name"
        );
        // The legacy lane was not also given the clip.
        assert!(app.arrangement.clips.iter().all(|lane| lane.is_empty()));
    }

    /// The whole road: a landed sound reaches the compiler as a legacy
    /// AUDIO clip, on a legacy twin minted with the right kind. A twin
    /// minted as MIDI would refuse the clip and the sound would land and
    /// stay silent — which is the failure this test exists to catch.
    #[test]
    fn a_landed_sound_reaches_the_compiler_as_an_audio_clip() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.tracks.push(daw::sequencing::Track {
            id: daw::sequencing::TrackId(99),
            name: "AUDIO 01".to_owned(),
            kind: daw::sequencing::TrackKind::Audio,
            blocks: Vec::new(),
            audio_blocks: Vec::new(),
            muted: false,
            solo: false,
            pitch_authority: daw::sequencing::PitchAuthority::default(),
            automation: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            sends: Vec::new(),
            is_group: false,
            folded: false,
            depth: 0,
            input: daw::sequencing::TrackInput::None,
            monitor: daw::sequencing::Monitor::Off,
            armed: false,
        });
        let track = app.song.tracks.len() - 1;
        let imported = daw::library::ImportedWav {
            original_path: std::path::PathBuf::from("/samples/iron.wav"),
            path: std::path::PathBuf::from("/cache/iron.wav"),
            sample_rate: 48_000,
            frames: 24_000,
        };
        app.finish_song_landing(&imported, track, daw::sequencing::TICKS_PER_BEAT * 2);

        app.project_song();

        let legacy = app.song_track_map[&app.song.tracks[track].id];
        assert_eq!(
            app.arrangement.tracks[legacy].kind,
            TrackKind::Audio,
            "the twin carries the song track's kind"
        );
        assert!(
            app.arrangement.tracks[legacy].chain.is_empty(),
            "an audio lane needs no instrument — it already is the sound"
        );
        let clips = &app.arrangement.clips[legacy];
        assert_eq!(clips.len(), 1);
        let clip = &clips[0];
        let source = clip.audio.as_ref().expect("it is an AUDIO clip");
        assert_eq!(source.source_frames, 24_000);
        assert_eq!(clip.start, 2.0, "two beats in");
        assert!((clip.len - 1.0).abs() < 1e-6, "half a second is one beat");
        assert_eq!(clip.name, "iron");
    }

    /// A lane holding both kinds hands the legacy side ONE list in time
    /// order. That side assumes clips are sorted by start (place_clip
    /// uses partition_point), so concatenating the two lists would
    /// silently corrupt it.
    #[test]
    fn a_mixed_lane_projects_in_time_order() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        // The default pattern block sits at tick 0; land sound before and
        // after it in the audio list, on the same lane.
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        let imported = daw::library::ImportedWav {
            original_path: std::path::PathBuf::from("/samples/late.wav"),
            path: std::path::PathBuf::from("/cache/late.wav"),
            sample_rate: 48_000,
            frames: 24_000,
        };
        app.finish_song_landing(&imported, 0, daw::sequencing::DEFAULT_PATTERN_TICKS * 2);
        let earlier = daw::library::ImportedWav {
            original_path: std::path::PathBuf::from("/samples/mid.wav"),
            path: std::path::PathBuf::from("/cache/mid.wav"),
            sample_rate: 48_000,
            frames: 24_000,
        };
        app.finish_song_landing(&earlier, 0, daw::sequencing::DEFAULT_PATTERN_TICKS);

        app.project_song();

        let legacy = app.song_track_map[&app.song.tracks[0].id];
        let starts: Vec<f32> = app.arrangement.clips[legacy]
            .iter()
            .map(|clip| clip.start)
            .collect();
        let mut sorted = starts.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        assert_eq!(starts, sorted, "the lane reaches legacy in time order");
        assert_eq!(starts.len(), 3, "one pattern block and two sounds");
    }

    /// A landing that cannot happen says WHY, by name, rather than
    /// leaving the performer wondering whether the key works.
    #[test]
    fn a_refused_landing_names_its_reason() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();

        let imported = daw::library::ImportedWav {
            original_path: std::path::PathBuf::from("/samples/iron.wav"),
            path: std::path::PathBuf::from("/cache/iron.wav"),
            sample_rate: 48_000,
            frames: 24_000,
        };
        // Track 0 is an INSTRUMENT track.
        app.finish_song_landing(&imported, 0, 0);
        assert_eq!(
            app.notice.as_deref(),
            Some(daw::sequencing::LandRefusal::NotAnAudioTrack.sign())
        );
        assert!(app.song.tracks[0].audio_blocks.is_empty());
    }

    /// STEP 8 of the MVP acceptance list: an export matches what the
    /// speakers said.
    ///
    /// It needs no Song-direct render path during the bridge era, and
    /// that is worth proving rather than assuming. What the speakers say
    /// IS the projection — the compile path reads arrangement.clips, and
    /// the projection is the only writer of the lanes it owns. So a Song
    /// exports because it is already the thing being played.
    ///
    /// The test renders a Song-sourced project through the SAME
    /// bounce_automated the export window uses, and listens.
    #[test]
    fn a_song_exports_through_the_projection_it_already_plays() {
        use daw::audio::bounce::{BounceFormat, BounceOptions, bounce_automated};

        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        // A curve on the Song, so the export must carry automation too.
        app.song.tracks[0].insert_point(daw::sequencing::TRACK_VOLUME, 0, 1.0);
        app.song.tracks[0].insert_point(
            daw::sequencing::TRACK_VOLUME,
            daw::sequencing::TICKS_PER_BEAT * 2,
            0.0,
        );
        app.project_song();

        let arrangement = &app.arrangement;
        let (mut spec, nodes) = build_graph_spec(
            &arrangement.tracks,
            &arrangement.master,
            &arrangement.returns,
            &arrangement.clips,
            None,
            false,
        );
        spec.set_modulation(build_mod_spec(
            &arrangement.tracks,
            &arrangement.modulators,
            &arrangement.mod_wires,
            &ParameterRegistry::default(),
            &nodes,
        ));
        let registry = ParameterRegistry::default();
        let tracks = arrangement.tracks.clone();
        let path = std::env::temp_dir().join("daw-test-song-export.wav");
        let opts = BounceOptions {
            length_beats: 4.0,
            format: BounceFormat::Int24,
            ..Default::default()
        };
        bounce_automated(
            &spec,
            &opts,
            &path,
            |beat, out| automation_letters(&tracks, &nodes, &registry, beat, out),
            |_| true,
        )
        .expect("the song renders");

        let all: Vec<i32> = hound::WavReader::open(&path)
            .expect("the export exists")
            .samples::<i32>()
            .map(Result::unwrap)
            .collect();
        let peak = |half: &[i32]| half.iter().fold(0, |peak: i32, s| peak.max(s.abs()));
        let (head, tail) = all.split_at(all.len() / 2);
        assert!(
            peak(head) > 0,
            "a trig written in the SONG must be audible in the export"
        );
        assert!(
            peak(tail) < peak(head),
            "and the Song's own curve must be heard closing the fader: {} then {}",
            peak(head),
            peak(tail)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn song_returns_overwrite_all_four_legacy_facts_but_keep_the_effect_chain() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.returns.push(daw::sequencing::ReturnTrack {
            name: "Plate".to_owned(),
            mute: true,
            volume: 0.625,
            pan: -0.4,
        });

        let mut legacy = ReturnTrack::new(0);
        legacy.name = "STALE".to_owned();
        legacy.mute = false;
        legacy.volume = 0.1;
        legacy.pan = 0.9;
        legacy.chain.push(DeviceInstance {
            id: 777,
            parent: None,
            state: DeviceState::new(DeviceKind::Reverb),
            bypass: false,
            page: 0,
            view_zoom: unit_zoom(),
            view_scroll: 0.0,
        });
        app.arrangement.returns = vec![legacy];

        app.project_song();

        let projected = &app.arrangement.returns[0];
        assert_eq!(projected.name, "Plate");
        assert!(projected.mute);
        assert_eq!(projected.volume, 0.625);
        assert_eq!(projected.pan, -0.4);
        assert_eq!(projected.chain.len(), 1, "the projection-only chain stays");
        assert_eq!(projected.chain[0].id, 777);
    }

    #[test]
    fn a_default_song_leaves_a_legacy_only_projects_returns_untouched() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        let mut legacy = ReturnTrack::new(0);
        legacy.name = "Old Plate".to_owned();
        legacy.mute = true;
        legacy.volume = 0.42;
        legacy.pan = 0.3;
        app.arrangement.returns = vec![legacy.clone()];
        app.song = daw::sequencing::Song::default();
        app.projected_song = None;

        app.project_song();

        assert_eq!(app.arrangement.returns, vec![legacy]);
        assert!(
            app.projected_song.is_none(),
            "the compatibility early-return held"
        );
    }

    #[test]
    fn song_input_monitor_and_arm_project_onto_the_recording_twin() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        let source = &mut app.song.tracks[0];
        source.kind = daw::sequencing::TrackKind::Audio;
        source.blocks.clear();
        source.input = daw::sequencing::TrackInput::Stereo(2, 3);
        source.monitor = daw::sequencing::Monitor::Auto;
        source.armed = true;

        app.project_song();

        let legacy = app.song_track_map[&app.song.tracks[0].id];
        let twin = &app.arrangement.tracks[legacy];
        assert_eq!(twin.input, daw::sequencing::TrackInput::Stereo(2, 3));
        assert_eq!(twin.monitor, daw::sequencing::Monitor::Auto);
        assert!(twin.armed);
    }

    #[test]
    fn a_default_song_leaves_legacy_only_routing_untouched() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.arrangement.tracks[0].input = daw::sequencing::TrackInput::Mono(4);
        app.arrangement.tracks[0].monitor = daw::sequencing::Monitor::In;
        app.arrangement.tracks[0].armed = true;
        app.song = daw::sequencing::Song::default();
        app.projected_song = None;

        app.project_song();

        assert_eq!(
            app.arrangement.tracks[0].input,
            daw::sequencing::TrackInput::Mono(4)
        );
        assert_eq!(
            app.arrangement.tracks[0].monitor,
            daw::sequencing::Monitor::In
        );
        assert!(app.arrangement.tracks[0].armed);
        assert!(app.projected_song.is_none());
    }

    #[test]
    fn groups_project_as_one_contiguous_legacy_run_in_song_order() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        let mut song = song_with_trig();
        song.tracks[0].name = "DRUMS".to_owned();
        song.tracks[0].blocks.clear();
        song.tracks[0].is_group = true;
        song.tracks[0].folded = true;
        let mut kick = song.tracks[0].clone();
        kick.id = daw::sequencing::TrackId(2);
        kick.name = "KICK".to_owned();
        kick.is_group = false;
        kick.folded = false;
        kick.depth = 1;
        let mut bass = kick.clone();
        bass.id = daw::sequencing::TrackId(3);
        bass.name = "BASS".to_owned();
        bass.depth = 0;
        song.tracks.extend([kick, bass]);
        app.song = song;

        app.project_song();

        let indices: Vec<usize> = app
            .song
            .tracks
            .iter()
            .map(|track| app.song_track_map[&track.id])
            .collect();
        assert_eq!(indices[1], indices[0] + 1);
        assert_eq!(indices[2], indices[1] + 1);
        let projected = &app.arrangement.tracks[indices[0]..=indices[2]];
        assert_eq!(
            projected
                .iter()
                .map(|track| track.name.as_str())
                .collect::<Vec<_>>(),
            ["DRUMS §", "KICK §", "BASS §"]
        );
        assert!(projected[0].is_group);
        assert!(projected[0].folded);
        assert_eq!(projected[0].depth, 0);
        assert_eq!(projected[1].depth, 1);
        assert_eq!(projected[2].depth, 0);
        assert!(
            projected[0]
                .chain
                .iter()
                .all(|device| !device.kind().is_instrument()),
            "a group starts at its sum, never at a synth"
        );

        // Reordering the canonical list reorders the physical twins, not
        // merely their copied labels: positional membership reads Song order.
        app.song.tracks.rotate_left(2);
        app.project_song();
        let reordered: Vec<usize> = app
            .song
            .tracks
            .iter()
            .map(|track| app.song_track_map[&track.id])
            .collect();
        assert!(reordered.windows(2).all(|pair| pair[1] == pair[0] + 1));
    }

    #[test]
    fn group_solo_keeps_its_members_and_a_soloed_member_keeps_its_bus() {
        let mut song = daw::sequencing::Song::default();
        let mut child = song.tracks[0].clone();
        child.id = daw::sequencing::TrackId(2);
        child.depth = 1;
        child.solo = false;
        song.tracks[0].is_group = true;
        song.tracks[0].solo = true;
        song.tracks.push(child);
        assert!(song_track_audible(&song.tracks, 0, true));
        assert!(song_track_audible(&song.tracks, 1, true));

        song.tracks[0].solo = false;
        song.tracks[1].solo = true;
        assert!(song_track_audible(&song.tracks, 0, true));
        assert!(song_track_audible(&song.tracks, 1, true));
    }

    #[test]
    fn a_send_slot_writes_song_and_refuses_a_missing_return_out_loud() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.add_return().expect("return A");
        app.project_song();

        app.apply_redesign_chain_intents(&[daw::ui::redesign::chain::Intent::SetParam {
            device: daw::ui::redesign::chain::TRACK_HEAD_ID,
            param: daw::ui::redesign::chain::track_send_param(0).expect("slot A"),
            value: 0.375,
        }]);
        assert_eq!(app.song.tracks[0].sends, vec![0.375]);
        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        assert_eq!(app.arrangement.tracks[legacy].sends, vec![0.375]);

        app.apply_redesign_chain_intents(&[daw::ui::redesign::chain::Intent::SetParam {
            device: daw::ui::redesign::chain::TRACK_HEAD_ID,
            param: daw::ui::redesign::chain::track_send_param(1).expect("slot B"),
            value: 0.9,
        }]);
        assert_eq!(app.notice.as_deref(), Some("SEND B: NO RETURN"));
        assert_eq!(app.song.tracks[0].sends, vec![0.375]);
    }

    #[test]
    fn track_head_arm_and_monitor_edit_song_then_reach_the_twin() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        app.song.tracks[0].blocks.clear();
        app.project_song();

        app.apply_redesign_chain_intents(&[
            daw::ui::redesign::chain::Intent::ToggleTrackArm,
            daw::ui::redesign::chain::Intent::CycleTrackMonitor,
        ]);
        assert!(app.song.tracks[0].armed, "arm landed on the Song");
        assert_eq!(app.song.tracks[0].monitor, daw::sequencing::Monitor::In);

        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        assert!(app.arrangement.tracks[legacy].armed);
        assert_eq!(
            app.arrangement.tracks[legacy].monitor,
            daw::sequencing::Monitor::In
        );
    }

    #[test]
    fn arm_and_monitor_refuse_an_instrument_out_loud() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.project_song();

        app.apply_redesign_chain_intents(&[daw::ui::redesign::chain::Intent::ToggleTrackArm]);
        assert_eq!(app.notice.as_deref(), Some("ARM: AUDIO TRACKS ONLY"));
        app.apply_redesign_chain_intents(&[daw::ui::redesign::chain::Intent::CycleTrackMonitor]);
        assert_eq!(app.notice.as_deref(), Some("MONITOR: AUDIO TRACKS ONLY"));
    }

    #[test]
    fn return_verbs_edit_song_and_every_inapplicable_noun_refuses() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.center_song = true;
        let ctx = egui::Context::default();

        for _ in 0..daw::sequencing::ReturnTrack::MAX {
            app.finish_redesign_actions(&ctx, vec![UiAction::AddReturn]);
        }
        assert_eq!(app.song.returns.len(), 8);
        assert_eq!(app.arrangement.returns.len(), 8);
        app.finish_redesign_actions(&ctx, vec![UiAction::AddReturn]);
        assert_eq!(
            app.notice.as_deref(),
            Some("RETURN: A THROUGH H ALREADY EXIST")
        );

        app.song.tracks[0].sends = (0..8).map(|index| index as f32 / 10.0).collect();
        app.arrangement.return_selected = Some(3);
        app.finish_redesign_actions(&ctx, vec![UiAction::RemoveReturn]);
        assert_eq!(app.song.returns.len(), 7);
        assert_eq!(app.arrangement.returns.len(), 7);
        assert_eq!(
            app.song.tracks[0].sends,
            vec![0.0, 0.1, 0.2, 0.4, 0.5, 0.6, 0.7]
        );

        app.finish_redesign_actions(&ctx, vec![UiAction::RemoveReturn]);
        assert_eq!(app.notice.as_deref(), Some("DELETE RETURN: NOTHING SHOWN"));
    }

    #[test]
    fn a_song_send_curve_reaches_the_existing_send_gain_node() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.add_return().expect("return A");
        app.song.tracks[0].sends = vec![0.2];
        app.song.tracks[0].insert_point("track.send.a", 0, 0.75);
        app.project_song();

        let arrangement = &app.arrangement;
        let (_, nodes) = build_graph_spec(
            &arrangement.tracks,
            &arrangement.master,
            &arrangement.returns,
            &arrangement.clips,
            None,
            false,
        );
        let track = app.song_track_map[&app.song.tracks[0].id];
        let send = nodes.sends[track][0].expect("return A has a send node");
        let mut letters = Vec::new();
        automation_letters(
            &arrangement.tracks,
            &nodes,
            &ParameterRegistry::default(),
            0.0,
            &mut letters,
        );
        assert!(letters.iter().any(|letter| {
            letter.node == send.to_bits()
                && letter.param == daw::params::mixer::GAIN
                && (letter.value - 0.75).abs() < 1e-6
        }));
    }

    /// The fader reads in DECIBELS and pan reads its side — real data
    /// over euphemism, and an exact centre that says so in one character
    /// rather than making the reader do arithmetic.
    #[test]
    fn the_track_head_speaks_decibels_and_sides() {
        assert_eq!(format_gain_db(1.0), "0.0dB", "unity is exactly zero");
        assert_eq!(
            format_gain_db(0.0),
            "-INF",
            "silence is a word, not a number"
        );
        assert_eq!(format_gain_db(0.5), "-6.0dB");
        assert!(
            format_gain_db(2.0).starts_with('+'),
            "boost carries its sign"
        );

        assert_eq!(format_pan(0.0), "C", "centre is one character");
        assert_eq!(format_pan(-1.0), "L100");
        assert_eq!(format_pan(1.0), "R100");
        assert_eq!(format_pan(0.5), "R50");
    }

    /// A slot edit on the track head writes the SONG, not the legacy
    /// twin. Writing the twin would be erased by the next projection
    /// pass, which copies the Song onto it — the edit would appear to
    /// work and then silently revert.
    #[test]
    fn a_track_head_edit_lands_on_the_song_not_the_twin() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.project_song();

        app.apply_redesign_chain_intents(&[daw::ui::redesign::chain::Intent::SetParam {
            device: daw::ui::redesign::chain::TRACK_HEAD_ID,
            param: daw::ui::redesign::chain::TRACK_LEVEL_PARAM,
            value: 0.25,
        }]);

        assert_eq!(app.song.tracks[0].volume, 0.25, "the song took the edit");
        // And it survives the next projection rather than being erased.
        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        assert_eq!(app.arrangement.tracks[legacy].volume, 0.25);
        assert_eq!(app.song.tracks[0].volume, 0.25);
    }

    #[test]
    fn typed_input_speaks_mono_stereo_none_and_every_refusal() {
        use daw::sequencing::TrackInput;

        assert_eq!(parse_track_input(&["1"], 4), Ok(TrackInput::Mono(0)));
        assert_eq!(parse_track_input(&["4/2"], 4), Ok(TrackInput::Stereo(3, 1)));
        assert_eq!(parse_track_input(&["none"], 0), Ok(TrackInput::None));
        assert_eq!(
            parse_track_input(&["1"], 0).unwrap_err(),
            "INPUT: ENGINE OFF — NO CHANNELS TO CHOOSE"
        );
        assert_eq!(
            parse_track_input(&["5"], 4).unwrap_err(),
            "INPUT: CHANNEL 5 IS NOT AVAILABLE (1..4)"
        );
        assert_eq!(
            parse_track_input(&["2/2"], 4).unwrap_err(),
            "INPUT: LEFT AND RIGHT MUST DIFFER"
        );
        assert!(
            parse_track_input(&[], 4).is_err(),
            "missing route was silent"
        );
        assert!(
            parse_track_input(&["1", "2"], 4).is_err(),
            "extra words were silently ignored"
        );
    }

    #[test]
    fn typed_input_none_lands_on_the_selected_song_audio_track() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.center_song = true;
        app.song = song_with_trig();
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        app.song.tracks[0].blocks.clear();
        app.song.tracks[0].input = daw::sequencing::TrackInput::Mono(7);
        app.project_song();

        assert_eq!(app.typed_input(&["none"]), Ok("INPUT: —".to_owned()));
        assert_eq!(app.song.tracks[0].input, daw::sequencing::TrackInput::None);

        app.song.tracks[0].kind = daw::sequencing::TrackKind::Instrument;
        assert_eq!(
            app.typed_input(&["none"]).unwrap_err(),
            "INPUT: AUDIO TRACKS ONLY"
        );
    }

    /// A tempo change is placed by the typed palette, because a tempo is
    /// an argument and the palette is the grammar's long-sentence mouth.
    #[test]
    fn the_typed_tempo_command_places_and_clears_a_mark() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();

        assert!(app.typed_tempo(&["140"]).is_ok());
        assert_eq!(app.song.tempo.len(), 1);
        assert_eq!(app.song.bpm_at(0, 120.0), 140.0);

        // Placing again at the same tick edits rather than stacks.
        assert!(app.typed_tempo(&["90"]).is_ok());
        assert_eq!(app.song.tempo.len(), 1);
        assert_eq!(app.song.bpm_at(0, 120.0), 90.0);

        assert!(app.typed_tempo(&["clear"]).is_ok());
        assert!(app.song.tempo.is_empty());
    }

    /// Every refusal names its reason rather than doing nothing, and a
    /// tempo nobody could mean is never stored.
    #[test]
    fn the_typed_tempo_command_refuses_out_loud() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();

        assert!(app.typed_tempo(&[]).is_err(), "no argument");
        assert!(app.typed_tempo(&["fast"]).is_err(), "not a number");
        assert!(app.typed_tempo(&["0"]).is_err(), "not a tempo");
        assert!(app.typed_tempo(&["-120"]).is_err(), "not a tempo");
        assert!(app.typed_tempo(&["clear"]).is_err(), "nothing to clear");
        assert!(app.song.tempo.is_empty(), "and nothing was stored");
    }

    /// Count-in and punch are placed by the typed palette, and every
    /// refusal names its reason rather than doing nothing.
    #[test]
    fn count_in_and_punch_are_reachable_and_refuse_out_loud() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);

        assert!(app.typed_count_in(&["2"]).is_ok());
        assert_eq!(app.transport.count_in_bars, 2);
        assert!(app.typed_count_in(&["off"]).is_ok());
        assert_eq!(app.transport.count_in_bars, 0);
        assert!(app.typed_count_in(&[]).is_err(), "no argument");
        assert!(app.typed_count_in(&["nine"]).is_err(), "not a number");
        assert!(app.typed_count_in(&["99"]).is_err(), "past the ceiling");

        assert!(app.typed_punch(&["4", "8"]).is_ok());
        assert_eq!(app.transport.punch, Some((4.0, 8.0)));
        assert!(app.typed_punch(&["clear"]).is_ok());
        assert_eq!(app.transport.punch, None);
        assert!(app.typed_punch(&[]).is_err());
        assert!(app.typed_punch(&["8", "4"]).is_err(), "inverted");
        assert!(app.typed_punch(&["a", "b"]).is_err(), "not beats");
        assert_eq!(app.transport.punch, None, "and none of them was stored");
    }

    /// Armed-and-waiting is NOT recording, and the transport must be able
    /// to say which it is. A performer who cannot tell them apart starts
    /// playing into nothing.
    #[test]
    fn counting_in_is_a_different_state_from_recording() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.transport.bpm = 120.0;
        app.transport.count_in_bars = 1;
        app.transport.armed = true;
        app.transport.playing = true;
        app.transport.position = 0.0;
        app.transport.begin_pass();

        assert!(app.transport.recording(), "armed and rolling");
        assert!(!app.transport.capturing(), "but not writing yet");

        app.transport.position = 4.0 * 60.0 / 120.0;
        assert!(app.transport.capturing(), "and now it writes");
    }

    /// The tempo map is AUDIBLE, not decorative.
    ///
    /// The legacy compiler stamps `beat * samples_per_beat` at one fixed
    /// tempo, so a map is expressed by moving the beat. At half the
    /// reference tempo every position doubles — which lands the note on
    /// the sample the map actually means, through a compiler that knows
    /// nothing about tempo maps.
    #[test]
    fn a_tempo_mark_moves_where_the_compiler_stamps_the_note() {
        let mut song = song_with_trig();
        let steady = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        let steady_start = steady.notes[0].start;
        assert!((steady_start - 1.0).abs() < 1e-6, "step 4 is one beat in");

        // Half the reference tempo, from the very start.
        assert!(song.set_tempo_mark(0, 60.0));
        let tempo = daw::tempo::TempoTable::build(&song, WARP_SAMPLE_RATE, 120.0);
        let halved = project_block(
            &song,
            &song.tracks[0].blocks[0],
            1,
            &tempo,
            WARP_SAMPLE_RATE * 60.0 / 120.0,
        );
        assert!(
            (halved.notes[0].start - 2.0).abs() < 1e-3,
            "at half tempo the note sits twice as far out, so it sounds at \
             the same wall-clock moment a 60bpm beat 1 would: got {}",
            halved.notes[0].start
        );
        assert!(
            halved.len > steady.len * 1.9,
            "and the clip stretches with it"
        );
    }

    /// A song with no tempo marks must project EXACTLY as it did before
    /// the warp existed. This is what makes the warp safe to apply to
    /// every project unconditionally.
    #[test]
    fn an_empty_tempo_map_warps_nothing_at_all() {
        let song = song_with_trig();
        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        let block = &song.tracks[0].blocks[0];
        let ticks = daw::sequencing::TICKS_PER_BEAT as f64;
        assert_eq!(clip.start, (block.start_tick as f64 / ticks) as f32);
        assert_eq!(clip.len, (block.length_ticks as f64 / ticks) as f32);
        // Step 4 with no push is exactly one beat, to the bit.
        assert!((clip.notes[0].start - 1.0).abs() < 1e-9);
        assert!((clip.notes[0].len - 0.25).abs() < 1e-9);
    }

    /// The copyist's timing: step 4 of a 16th grid lands one beat in, a
    /// 12-tick note is a quarter beat long, and the trig's condition
    /// carries onto the note the compile path will stamp.
    #[test]
    fn a_block_projects_notes_with_timing_and_probability() {
        let song = song_with_trig();
        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 7);
        assert_eq!(clip.id, 7);
        assert_eq!(clip.notes.len(), 1);
        let note = &clip.notes[0];
        assert_eq!(note.pitch, 60);
        assert!((note.start - 1.0).abs() < 1e-9, "step 4 = beat 1");
        assert!((note.len - 0.25).abs() < 1e-9);
        assert_eq!(note.vel, 100);
        assert!((note.prob - 0.75).abs() < 1e-6, "the condition travels");
        assert!((clip.len - 16.0).abs() < 1e-6, "default block = 16 beats");
    }

    /// One pattern, three blocks → three clips with identical notes:
    /// sharing is the song's idea, duplication is the copyist's job.
    #[test]
    fn a_shared_pattern_projects_into_every_block() {
        let mut song = song_with_trig();
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        for (block_id, start) in [(20u64, 16usize), (21, 32)] {
            song.tracks[0].blocks.push(PatternBlock {
                id: daw::sequencing::BlockId(block_id),
                pattern_id,
                start_tick: start * daw::sequencing::TICKS_PER_BEAT,
                length_ticks: daw::sequencing::DEFAULT_PATTERN_TICKS,
            });
        }
        let clips: Vec<Clip> = song.tracks[0]
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| project_block_steady(&song, block, index as u64))
            .collect();
        assert_eq!(clips.len(), 3);
        assert!((clips[1].start - 16.0).abs() < 1e-6);
        assert_eq!(clips[0].notes, clips[1].notes);
        assert_eq!(clips[1].notes, clips[2].notes);
    }

    #[test]
    fn song_solo_precedence_silences_every_other_track() {
        let mut song = Song::default();
        daw::ui::redesign::arrangement::ArrangementPanel::default().add_track(&mut song);
        song.tracks[0].solo = true;
        song.tracks[1].muted = false;

        let any_solo = song.tracks.iter().any(|track| track.solo);
        assert!(song_track_audible(&song.tracks, 0, any_solo));
        assert!(
            !song_track_audible(&song.tracks, 1, any_solo),
            "an unmuted track is still silent beside a solo"
        );
    }

    #[test]
    fn a_muted_song_track_uses_the_legacy_twins_real_mute() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.tracks[0].muted = true;

        app.project_song();

        let track_id = app.song.tracks[0].id;
        let legacy = app.song_track_map[&track_id];
        assert!(app.arrangement.tracks[legacy].mute);
        assert!(!app.arrangement.clips[legacy].is_empty());
        assert!(app.arrangement.force_recompile);
    }

    /// The offset model's base crosses the bridge: a curve drawn on a
    /// Song track lands on the legacy track the compiler already bakes
    /// into ramps, converted from ticks to beats and otherwise untouched.
    #[test]
    fn a_song_curve_reaches_the_legacy_compiler_in_beats() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        // Two beats apart, at 48 ticks to the beat.
        app.song.tracks[0].insert_point("track.volume", 0, 0.25);
        app.song.tracks[0].insert_point("track.volume", 96, 0.75);
        assert!(app.song.tracks[0].bend_point("track.volume", 0, 0.5));

        app.project_song();

        let track_id = app.song.tracks[0].id;
        let legacy = app.song_track_map[&track_id];
        let points = app.arrangement.tracks[legacy]
            .automation
            .points("track.volume");
        assert_eq!(points.len(), 2, "both breakpoints crossed");
        assert_eq!(points[0].beat, 0.0);
        assert_eq!(points[1].beat, 2.0, "96 ticks is beat 2");
        assert_eq!(points[0].value, 0.25);
        assert_eq!(points[1].value, 0.75);
        assert_eq!(points[0].bend, 0.5, "the bend crosses untouched");

        // And the legacy reader agrees with the Song reader at the ends.
        let legacy_mid =
            app.arrangement.tracks[legacy]
                .automation
                .value_at("track.volume", 2.0, 0.0);
        let song_mid = app.song.tracks[0].value_at("track.volume", 96, 0.0);
        assert!((legacy_mid - song_mid).abs() < 1e-6);
    }

    /// The mixer's target ids are FILE FORMAT, and they live in two
    /// places: the Song model (library) and the legacy `targets` table
    /// (binary). If they ever drift, every envelope already written to
    /// disk is silently orphaned — so the two are pinned together here.
    #[test]
    fn the_mixer_target_ids_agree_across_the_bridge() {
        assert_eq!(
            daw::sequencing::TRACK_VOLUME,
            crate::targets::TRACK_VOLUME_TARGET
        );
        assert_eq!(daw::sequencing::TRACK_PAN, crate::targets::TRACK_PAN_TARGET);
    }

    /// The fader and pan cross to the legacy twin the compiler reads,
    /// and a curve on top of them composes rather than replaces.
    #[test]
    fn the_mixer_values_reach_the_legacy_twin() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        app.song.tracks[0].volume = 0.5;
        app.song.tracks[0].pan = -0.25;

        app.project_song();

        let track_id = app.song.tracks[0].id;
        let legacy = app.song_track_map[&track_id];
        assert_eq!(app.arrangement.tracks[legacy].volume, 0.5);
        assert_eq!(app.arrangement.tracks[legacy].pan, -0.25);

        // With no curve, the Song read is the knob itself.
        assert_eq!(app.song.tracks[0].volume_at(0), 0.5);
        assert_eq!(app.song.tracks[0].pan_at(0), -0.25);

        // The legacy reader, given the same base, agrees.
        let legacy_volume = app.arrangement.tracks[legacy].automation.value_at(
            crate::targets::TRACK_VOLUME_TARGET,
            0.0,
            app.arrangement.tracks[legacy].volume,
        );
        assert_eq!(legacy_volume, 0.5);
    }

    /// A target with an envelope but no points is the bare knob, so it
    /// must not project an empty envelope that reads as "automated".
    #[test]
    fn an_envelope_with_no_points_never_projects() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.song = song_with_trig();
        // Mint the envelope, then leave it empty.
        let _ = app.song.tracks[0].points_mut("track.pan");
        assert_eq!(app.song.tracks[0].automation.len(), 1);

        app.project_song();

        let track_id = app.song.tracks[0].id;
        let legacy = app.song_track_map[&track_id];
        assert!(
            app.arrangement.tracks[legacy]
                .automation
                .envelopes
                .is_empty(),
            "an empty envelope is not automation"
        );
    }

    /// A block shorter than the pattern truncates: no note starts past
    /// the clip edge.
    #[test]
    fn a_short_block_truncates_the_pattern() {
        let mut song = song_with_trig();
        // One beat long: step 4 (beat 1) must not sound.
        song.tracks[0].blocks[0].length_ticks = daw::sequencing::TICKS_PER_BEAT;
        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        assert!(clip.notes.is_empty());
    }

    fn builtin_lookup(name: &str) -> Option<Result<daw::pitch::Scale, String>> {
        daw::pitch::builtin_scale(name).map(Ok)
    }

    /// L3, heard through the projection: a Degree anchor reflows when
    /// the key changes; the sound of an Absolute anchor never moves.
    #[test]
    fn a_degree_note_reflows_when_the_key_changes() {
        let mut song = Song::default();
        song.key = daw::pitch::parse_key_command(&["d", "dorian"], &song.key, &builtin_lookup)
            .expect("d dorian");
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        let pattern = song.pattern_mut(pattern_id).expect("default pattern");
        pattern.set_primary(
            0,
            daw::sequencing::Note::with_pitch(daw::pitch::Pitch::degree(2, 0), 12, 100),
        );
        pattern.add_tone(0, SongNote::new(69, 12, 100));

        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        let pitches: Vec<u8> = clip.notes.iter().map(|note| note.pitch).collect();
        assert!(
            pitches.contains(&65),
            "degree 2 of D dorian is F: {pitches:?}"
        );
        assert!(pitches.contains(&69), "the absolute A stays A");

        song.key = daw::pitch::parse_key_command(&["d", "major"], &song.key, &builtin_lookup)
            .expect("d major");
        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        let pitches: Vec<u8> = clip.notes.iter().map(|note| note.pitch).collect();
        assert!(
            pitches.contains(&66),
            "the same degree reads F# in D major: {pitches:?}"
        );
        assert!(pitches.contains(&69), "the absolute A still stays A");
    }

    /// The push survives projection exactly: legacy note starts are
    /// fractional beats, so a 3-tick displacement is 3/48 of a beat.
    #[test]
    fn the_push_projects_exactly_into_fractional_beats() {
        let mut song = song_with_trig();
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        song.pattern_mut(pattern_id)
            .expect("default pattern")
            .trig_mut(4)
            .notes[0]
            .micro_ticks = 3;
        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 1);
        assert!((clip.notes[0].start - (1.0 + 3.0 / 48.0)).abs() < 1e-12);
    }

    /// The `:key` long form sets the ambient context, answers with its
    /// sign, and refuses garbage without touching the key.
    #[test]
    fn the_key_long_form_sets_the_ambient_context_or_refuses() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);

        app.run_typed_command("key d dorian");
        assert_eq!(daw::ui::redesign::lens::key_sign(&app.song.key), "D DORIAN");
        assert_eq!(app.notice.as_deref(), Some("KEY: D DORIAN"));

        app.run_typed_command("key d diatonic mode 9");
        assert_eq!(app.notice.as_deref(), Some("MODE: SCALE HAS 7 DEGREES"));
        assert_eq!(
            daw::ui::redesign::lens::key_sign(&app.song.key),
            "D DORIAN",
            "a refused command leaves the context alone"
        );

        app.run_typed_command("key 264hz 22shruti mode 4");
        assert_eq!(
            daw::ui::redesign::lens::key_sign(&app.song.key),
            "264HZ 22SHRUTI/4"
        );
    }

    /// The bridge era never lies: a pitch the legacy 12TET path cannot
    /// reproduce wears `approx` in its view; an exactly-reproducible one
    /// does not — the machine's sign, not the musician's.
    #[test]
    fn xen_pitches_wear_the_approximation_flag() {
        let mut song = Song::default();
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        song.pattern_mut(pattern_id)
            .expect("default pattern")
            .set_primary(
                0,
                daw::sequencing::Note::with_pitch(daw::pitch::Pitch::degree(1, 0), 12, 100),
            );

        // Chromatic 12TET: degree 1 is C#4, exactly on the table.
        let views = song_pattern_note_views(song.pattern(pattern_id).expect("pattern"), &song.key);
        assert_eq!(views.len(), 1);
        assert!(!views[0].approx);

        // 22 shruti: degree 1 (256/243) misses every 12TET slot.
        song.key = daw::pitch::parse_key_command(&["264hz", "22shruti"], &song.key, &|name| {
            daw::pitch::builtin_scale(name).map(Ok)
        })
        .expect("shruti key");
        let views = song_pattern_note_views(song.pattern(pattern_id).expect("pattern"), &song.key);
        assert!(views[0].approx, "the machine admits the approximation");
    }

    /// The deviation long forms: additive cents, sub-step ticks, and the
    /// two sound-preserving striation transforms — all on the trig the
    /// cursor names, all refusing where there is nothing to act on.
    #[test]
    fn the_pitch_long_forms_act_on_the_selected_trig() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);

        // Outside the song view the whole family refuses by name.
        app.run_typed_command("tune +14");
        assert_eq!(app.notice.as_deref(), Some("TUNE: SONG VIEW ONLY — F10"));

        app.center_song = true;
        let pattern_id = app.song.tracks[0].blocks[0].pattern_id;
        app.song
            .pattern_mut(pattern_id)
            .expect("default pattern")
            .set_primary(0, SongNote::new(69, 12, 100));

        app.run_typed_command("tune +14");
        assert_eq!(app.notice.as_deref(), Some("TUNE: +14¢ ON 1 NOTES"));
        let note =
            |app: &App| app.song.pattern(pattern_id).expect("pattern").trig(0).notes[0].clone();
        assert_eq!(note(&app).pitch.offset_cents, 14.0);
        let bent_hz = note(&app).pitch.resolve(&app.song.key);

        app.run_typed_command("push -3");
        assert_eq!(note(&app).micro_ticks, -3);

        // Quantize: the anchor becomes a degree, the sound holds.
        app.run_typed_command("quantize-key");
        assert!(matches!(
            note(&app).pitch.anchor,
            daw::pitch::Anchor::Degree { .. }
        ));
        let after = note(&app).pitch.resolve(&app.song.key);
        assert!(((after - bent_hz) / bent_hz).abs() < 1e-9, "sound held");

        // Free: back to physics, still the same sound.
        app.run_typed_command("free");
        assert!(matches!(
            note(&app).pitch.anchor,
            daw::pitch::Anchor::Absolute(_)
        ));
        assert_eq!(note(&app).pitch.offset_cents, 0.0);
        let freed = note(&app).pitch.resolve(&app.song.key);
        assert!(((freed - bent_hz) / bent_hz).abs() < 1e-9, "sound held");

        // An empty step refuses out loud.
        app.sequence_cursor_tick = 20 * SONG_PATTERN_STEP_TICKS;
        app.run_typed_command("tune +5");
        assert_eq!(app.notice.as_deref(), Some("TUNE: NOTHING HERE"));
    }

    /// Snap is the one LOSSY transform: it lands as a preview, the model
    /// holds still until Enter, and commit zeroes exactly the offsets.
    #[test]
    fn snap_key_previews_before_it_loses_anything() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.center_song = true;
        let pattern_id = app.song.tracks[0].blocks[0].pattern_id;
        app.song
            .pattern_mut(pattern_id)
            .expect("default pattern")
            .set_primary(0, SongNote::new(69, 12, 100));
        app.run_typed_command("tune +14");

        app.run_typed_command("snap-key");
        assert!(app.snap_preview.is_some(), "the loss is only previewed");
        assert_eq!(
            app.song.pattern(pattern_id).expect("pattern").trig(0).notes[0]
                .pitch
                .offset_cents,
            14.0,
            "nothing changed yet"
        );

        app.commit_snap_preview();
        assert!(app.snap_preview.is_none());
        let note = &app.song.pattern(pattern_id).expect("pattern").trig(0).notes[0];
        assert_eq!(note.pitch.offset_cents, 0.0, "the offset is gone");
        assert!(matches!(
            note.pitch.anchor,
            daw::pitch::Anchor::Degree { .. }
        ));
    }

    /// `:lens` names how a track reads: built-ins always exist, unknown
    /// names refuse, and the choice is per track.
    #[test]
    fn the_lens_long_form_sets_a_per_track_reading() {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);

        app.run_typed_command("lens degrees");
        assert_eq!(app.notice.as_deref(), Some("LENS: SONG VIEW ONLY — F10"));

        app.center_song = true;
        app.run_typed_command("lens degrees");
        let track_id = app.song.tracks[0].id.0;
        assert_eq!(
            app.track_lenses.get(&track_id).map(String::as_str),
            Some("degrees")
        );

        app.run_typed_command("lens sargam");
        assert_eq!(app.notice.as_deref(), Some("LENS: NO LENS NAMED sargam"));
        assert_eq!(
            app.track_lenses.get(&track_id).map(String::as_str),
            Some("degrees"),
            "a refused lens leaves the standing choice alone"
        );
    }

    #[test]
    fn sequence_toggle_at_tick_twelve_lands_on_song_step_one() {
        let mut pattern = daw::sequencing::Pattern::default();
        let notice = apply_song_pattern_intents(
            &mut pattern,
            &[redesign_sequence::Intent::Toggle {
                tick: 12,
                default_pitch: daw::pitch::Pitch::from_midi(67),
                default_length_ticks: 12,
                default_velocity: 100,
            }],
        );

        assert_eq!(notice, None);
        assert!(pattern.trig(1).enabled);
        assert_eq!(
            pattern.trig(1).primary().map(|note| note.pitch),
            Some(daw::pitch::Pitch::from_midi(67))
        );
        assert!(pattern.trig(0).notes.is_empty());
    }

    #[test]
    fn sequence_probability_reaches_the_projected_note() {
        let mut song = Song::default();
        let pattern_id = song.tracks[0].blocks[0].pattern_id;
        let pattern = song.pattern_mut(pattern_id).expect("default pattern");
        pattern.set_primary(1, SongNote::new(60, 12, 100));
        let notice = apply_song_pattern_intents(
            pattern,
            &[redesign_sequence::Intent::SetProbability {
                tick: 12,
                probability: 0.75,
            }],
        );
        assert_eq!(notice, None);
        assert!((pattern.trig(1).probability - 0.75).abs() < f32::EPSILON);

        let clip = project_block_steady(&song, &song.tracks[0].blocks[0], 9);
        assert_eq!(clip.notes.len(), 1);
        assert!((clip.notes[0].prob - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn sequence_add_note_builds_a_song_chord() {
        let mut pattern = daw::sequencing::Pattern::default();
        let intents = [
            redesign_sequence::Intent::SetPrimary {
                tick: 24,
                pitch: daw::pitch::Pitch::from_midi(60),
                length_ticks: 12,
                velocity: 100,
            },
            redesign_sequence::Intent::AddNote {
                tick: 24,
                pitch: daw::pitch::Pitch::from_midi(67),
                length_ticks: 24,
                velocity: 96,
                probability: 1.0,
            },
        ];

        assert_eq!(apply_song_pattern_intents(&mut pattern, &intents), None);
        assert_eq!(pattern.trig(2).notes.len(), 2);
        assert_eq!(
            pattern.trig(2).notes[0].pitch,
            daw::pitch::Pitch::from_midi(60)
        );
        assert_eq!(
            pattern.trig(2).notes[1].pitch,
            daw::pitch::Pitch::from_midi(67)
        );
    }

    #[test]
    fn sequence_nudge_refuses_an_occupied_song_step_atomically() {
        let mut pattern = daw::sequencing::Pattern::default();
        pattern.set_primary(1, SongNote::new(60, 12, 100));
        pattern.set_primary(2, SongNote::new(67, 12, 100));
        let before = pattern.clone();

        let notice = apply_song_pattern_intents(
            &mut pattern,
            &[redesign_sequence::Intent::Nudge {
                tick: 12,
                delta_ticks: 12,
            }],
        );

        assert_eq!(notice, Some("nudge blocked by an occupied step"));
        assert_eq!(pattern, before);
    }
}
