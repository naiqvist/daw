//! Recording: arming, the takes that come back, and where they land.
//!
//! The capture itself belongs to the engine, over a ring buffer. This is
//! the green-zone half — when to start, which routes to listen to, and
//! which lane each finished take belongs on.

use super::*;

fn quantize_tick(tick: usize, grid_ticks: usize) -> Option<usize> {
    if grid_ticks == 0 {
        return None;
    }
    let below = tick / grid_ticks * grid_ticks;
    let remainder = tick % grid_ticks;
    Some(if remainder >= grid_ticks.div_ceil(2) {
        below.saturating_add(grid_ticks)
    } else {
        below
    })
}

impl App {
    /// Whether a take should be WRITING right now.
    ///
    /// Rolling is `armed && playing`, exactly as the action vocabulary
    /// says — one derived answer rather than a third piece of state that
    /// could disagree with the two it is made of. The count-in and the
    /// punch range narrow that intention to the moment, and neither
    /// un-arms the transport: the performer stays armed across a punch
    /// window rather than having to re-arm inside it.
    pub(crate) fn should_record(&self) -> bool {
        self.transport.capturing()
    }

    /// Start, feed and stop the take, once per frame.
    ///
    /// The order is the whole of the correctness here: drain BEFORE
    /// stopping the callback and once more after, or the tail of every
    /// recording is left in the ring and every take ends early.
    pub(crate) fn drive_recording(&mut self) {
        let want = self.should_record();
        let running = self
            .recorder
            .as_ref()
            .is_some_and(record::Recorder::recording);
        if want && !running {
            self.begin_recording();
        } else if !want && running {
            self.finish_recording();
        } else if running && let Some(recorder) = &mut self.recorder {
            recorder.poll();
        }
    }

    /// Which lanes have something to record, and from where.
    ///
    /// An armed lane with no input route is skipped rather than given a
    /// file of silence — being armed says what you INTEND, and the route
    /// is what makes it possible.
    pub(crate) fn record_routes(&self) -> Vec<record::RecordRoute> {
        record_routes(&self.arrangement.tracks)
    }

    /// Where takes are written.
    ///
    /// Beside the project when there is one, so a song and its
    /// recordings move together — and in the system's temp directory
    /// when there is not, which is said out loud rather than hidden,
    /// because a take in a temp directory is a take you will lose.
    pub(crate) fn recording_dir(&self) -> std::path::PathBuf {
        match self.project_path.as_ref().and_then(|path| path.parent()) {
            Some(beside) => beside.join("Recorded"),
            None => std::env::temp_dir().join("daw-recorded"),
        }
    }

    pub(crate) fn begin_recording(&mut self) {
        let routes = self.record_routes();
        if routes.is_empty() {
            // Said once, and it stops the transport asking again every
            // frame: nothing is armed, or what is armed has no input.
            self.transport.armed = false;
            self.notice =
                Some("nothing to record — arm an audio lane and give it an input".to_owned());
            return;
        }
        let dir = self.recording_dir();
        let Some(recorder) = &mut self.recorder else {
            self.transport.armed = false;
            self.notice = Some("the engine is not running".to_owned());
            return;
        };
        if let Err(why) = recorder.begin(&routes, &dir) {
            self.transport.armed = false;
            self.notice = Some(why.to_string());
            return;
        }
        // The callback starts filling only AFTER the ring is drained and
        // the files are open, so the first sample written is the first
        // sample of the take.
        if let Some(engine) = &self.engine {
            engine.set_capturing(true);
            self.recording_overruns = engine.capture_overruns();
        }
        self.recording_from = 0;
        if self.project_path.is_none() {
            self.notice = Some(format!(
                "recording to {} — save the project to keep takes beside it",
                dir.display()
            ));
        }
    }

    /// Close the take and put what was captured on the timeline.
    pub(crate) fn finish_recording(&mut self) {
        if !self
            .recorder
            .as_ref()
            .is_some_and(record::Recorder::recording)
        {
            return;
        }
        // Where it began, and how badly. Read before the flag drops, so
        // the stamp belongs to the run that is ending.
        let (started_at, overruns, latency) = match &self.engine {
            Some(engine) => (
                engine.capture_start(),
                engine
                    .capture_overruns()
                    .saturating_sub(self.recording_overruns),
                engine.info().latency_frames.unwrap_or(0) as u64,
            ),
            None => (self.recording_from, 0, 0),
        };
        if let Some(engine) = &self.engine {
            // The flag drops and the drain follows. A block already
            // inside the callback when the flag flips still commits, and
            // the poll below collects it — but one that commits AFTER
            // that poll is lost, so a take can end up to one block short
            // of where the transport stopped. Five milliseconds at 256
            // frames, and closing it properly wants a generation
            // handshake rather than a bool; the honest note is cheaper
            // than the machinery until someone can hear the difference.
            engine.set_capturing(false);
        }
        let (takes, errors, frames) = match &mut self.recorder {
            // One last drain: whatever is in the ring at this moment is
            // the TAIL of the take, and dropping it would clip the end
            // off every recording by a frame's worth of backlog.
            Some(recorder) => {
                recorder.poll();
                // Read BEFORE finishing, which empties the list.
                let frames = recorder.frames();
                let (takes, errors) = recorder.finish();
                (takes, errors, frames)
            }
            None => (Vec::new(), Vec::new(), 0),
        };
        if let Some(why) = errors.first() {
            self.notice = Some(why.to_string());
        }
        self.place_takes(takes, started_at, latency, overruns, frames);
    }

    /// Put finished takes on their lanes.
    pub(crate) fn place_takes(
        &mut self,
        takes: Vec<record::Take>,
        started_at: u64,
        stream_latency: u64,
        overruns: u64,
        frames: u64,
    ) {
        if takes.is_empty() {
            return;
        }
        let rate = self
            .engine
            .as_ref()
            .map_or(48_000, |engine| engine.info().sample_rate);
        // A take arrives LATE by the round trip the player was hearing
        // through, so pull back both parts of that path. The schedule's
        // PDC is computed from device declarations whose impulse tests
        // MEASURE the delay; the stream figure is only what the backend
        // REPORTS. Neither is a physical loopback measurement of this
        // machine and room — that calibration is separate work.
        //
        // rtaudio exposes one duplex stream-latency number, not separate
        // input and output figures, so there is no divergence to choose
        // between here.
        let latency = stream_latency.saturating_add(self.schedule_latency_frames);
        let at_sample = started_at.saturating_sub(latency);
        let tempo =
            daw::tempo::TempoTable::build(&self.song, f64::from(rate.max(1)), self.transport.bpm);
        let beat = if self.song.tempo.is_empty() {
            // Preserve the legacy answer bit for bit when there is no map,
            // including its sub-tick precision.
            at_sample as f64 / f64::from(rate.max(1)) * self.transport.bpm / 60.0
        } else {
            tempo.tick_at(at_sample) as f64 / daw::sequencing::TICKS_PER_BEAT as f64
        };
        let at = beat as f32;
        let mut placed = 0;
        let mut refused = Vec::new();
        for take in takes {
            let name = take
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("take")
                .to_owned();
            let source = AudioSource {
                transpose: 0.0,
                detune: 0.0,
                transposed_from: None,
                applied_ratio: 1.0,
                path: take.path.clone(),
                sample_rate: take.sample_rate,
                source_offset: 0,
                source_frames: take.frames,
                gain: 1.0,
                looped: false,
                file_frames: take.frames,
                reversed: false,
                fade_in: 0,
                fade_out: 0,
                fade_in_curve: 0.0,
                fade_out_curve: 0.0,
                envelope: Vec::new(),
            };
            if let Some(song_track) = self.song_track_for_legacy_twin(take.track) {
                // The compensation above produces the same absolute sample
                // the legacy placement used. Song time is integral ticks, so
                // the already-built table performs the one necessary unit
                // conversion before the existing AudioBlock landing takes
                // over length, overlap and id minting.
                let start_tick = tempo.tick_at(at_sample);
                match self
                    .song
                    .place_audio(song_track, start_tick, name.clone(), source, &tempo)
                {
                    Ok(_) => {
                        placed += 1;
                        self.projected_song = None;
                    }
                    Err(why) => refused.push(format!(
                        "{} — {}; FILE KEPT AT {}",
                        name,
                        why.sign(),
                        take.path.display()
                    )),
                }
            } else if self
                .arrangement
                .insert_audio(take.track, at.max(0.0), name, source, self.transport.bpm)
                .is_some()
            {
                placed += 1;
                self.arrangement.force_recompile = true;
            } else {
                refused.push(format!(
                    "TAKE: NO TRACK — FILE KEPT AT {}",
                    take.path.display()
                ));
            }
        }
        if placed > 0 {
            // A hole is not a dropout to shrug off: the file is missing
            // samples and the take after the hole is early. Say so.
            let seconds = frames as f64 / f64::from(rate.max(1));
            let recorded = if overruns > 0 {
                format!(
                    "recorded {placed} take(s), {seconds:.1}s at beat {at:.2} — {overruns} block(s) were LOST, the audio has holes"
                )
            } else {
                format!("recorded {placed} take(s), {seconds:.1}s at beat {at:.2}")
            };
            self.notice = Some(if refused.is_empty() {
                recorded
            } else {
                format!("{recorded}; {}", refused.join("; "))
            });
        } else if !refused.is_empty() {
            self.notice = Some(refused.join("; "));
        }
    }

    /// Turn a closed controller performance into one visible, pending Song
    /// write. Nothing in the Song changes here: quantisation and collision
    /// replacement are lossy, so the lower strip shows the COMPLETE result
    /// as ghosts until Enter commits or Escape cancels.
    ///
    /// `grid_ticks` and `grid` are the visible rhythmic context's value and
    /// sign from the frame that closes the take. Passing both keeps this
    /// landing pure: it does not mint a second grid authority beside the UI.
    pub(crate) fn preview_midi_take(
        &mut self,
        take: record::MidiTake,
        sample_rate: u32,
        grid_ticks: usize,
        grid: impl Into<String>,
    ) {
        let Some(song_track) = self.song_track_for_legacy_twin(take.track) else {
            self.notice = Some("MIDI TAKE: NO SONG TRACK — NOTHING CHANGED".to_owned());
            return;
        };
        let Some(track) = self.song.tracks.get(song_track) else {
            self.notice = Some("MIDI TAKE: NO SONG TRACK — NOTHING CHANGED".to_owned());
            return;
        };
        if track.kind != daw::sequencing::TrackKind::Instrument || track.is_group {
            self.notice = Some("MIDI TAKE: INSTRUMENT TRACKS ONLY — NOTHING CHANGED".to_owned());
            return;
        }
        if take.notes.is_empty() {
            self.notice = Some("MIDI TAKE: NO NOTES — NOTHING CHANGED".to_owned());
            return;
        }
        if grid_ticks == 0 {
            self.notice = Some("MIDI TAKE: GRID HAS NO DURATION — NOTHING CHANGED".to_owned());
            return;
        }

        let tempo = daw::tempo::TempoTable::build(
            &self.song,
            f64::from(sample_rate.max(1)),
            self.transport.bpm,
        );
        let mut destination = None;
        let mut writes = std::collections::BTreeMap::<usize, daw::sequencing::Trig>::new();
        for performed in take.notes {
            let raw_start = tempo.tick_at(performed.start_sample);
            let start_tick = quantize_tick(raw_start, grid_ticks).unwrap_or(raw_start);
            let Some(block) = track.blocks.iter().find(|block| {
                block.start_tick <= start_tick
                    && start_tick < block.start_tick.saturating_add(block.length_ticks)
            }) else {
                self.notice = Some(format!(
                    "MIDI TAKE: NO PATTERN UNDER TICK {start_tick} — NOTHING CHANGED"
                ));
                return;
            };
            if destination.is_some_and(|id| id != block.id) {
                self.notice =
                    Some("MIDI TAKE: CROSSES PATTERN BLOCKS — NOTHING CHANGED".to_owned());
                return;
            }
            destination = Some(block.id);

            let local_tick = start_tick.saturating_sub(block.start_tick);
            let step = local_tick / daw::sequencing::PATTERN_STEP_TICKS;
            if step >= daw::sequencing::PATTERN_STEPS {
                self.notice = Some(
                    "MIDI TAKE: NOTE FALLS PAST THE PATTERN'S LAST TRIG — NOTHING CHANGED"
                        .to_owned(),
                );
                return;
            }
            let raw_end = tempo.tick_at(performed.end_sample.max(performed.start_sample));
            let mut note = daw::sequencing::Note::new(
                performed.pitch.min(127),
                raw_end.saturating_sub(raw_start).max(1),
                performed.velocity,
            );
            note.micro_ticks = (local_tick % daw::sequencing::PATTERN_STEP_TICKS) as i16;
            writes.entry(step).or_default().add_tone(note);
        }

        let Some(block_id) = destination else {
            self.notice = Some("MIDI TAKE: NO NOTES — NOTHING CHANGED".to_owned());
            return;
        };
        let Some((_, block)) = self.song.pattern_block(block_id) else {
            self.notice = Some("MIDI TAKE: THE PATTERN BLOCK IS GONE — NOTHING CHANGED".to_owned());
            return;
        };
        let pattern_id = block.pattern_id;
        let Some(pattern) = self.song.pattern(pattern_id) else {
            self.notice = Some("MIDI TAKE: THE PATTERN IS GONE — NOTHING CHANGED".to_owned());
            return;
        };
        let collisions = writes
            .keys()
            .filter(|step| {
                let trig = pattern.trig(**step);
                trig.enabled || !trig.notes.is_empty()
            })
            .count();
        let writes = writes
            .into_iter()
            .map(|(step, after)| redesign_bridge::MidiTrigWrite { step, after })
            .collect::<Vec<_>>();
        let count = writes.len();
        let grid = grid.into();
        self.snap_preview = Some(redesign_bridge::SnapPreview::Midi {
            pattern: pattern_id,
            writes,
            grid: grid.clone(),
        });
        self.redesign.focus_sequence();
        self.notice = Some(format!(
            "MIDI TAKE: {count} TRIG(S) ON {grid} PREVIEWED — ENTER REPLACES {collisions} COLLISION(S) · ESC CANCELS"
        ));
    }

    /// Resolve a projection-owned legacy lane back to its canonical Song
    /// track. A lane absent from this map is genuinely legacy-owned and must
    /// keep using the old placement path.
    pub(crate) fn song_track_for_legacy_twin(&self, legacy: usize) -> Option<usize> {
        self.song.tracks.iter().position(|track| {
            self.song_track_map
                .get(&track.id)
                .is_some_and(|twin| *twin == legacy)
        })
    }

    /// How many hardware inputs there are to route from.
    ///
    /// Zero when the engine is off, and that is the honest answer rather
    /// than a remembered one: a route can only be chosen against an
    /// interface that is actually open, and offering channels from the
    /// last session would offer channels that may not exist.
    pub(crate) fn input_channels(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |engine| engine.info().in_channels as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn app_with_audio_lane() -> App {
        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.arrangement.tracks[0].kind = TrackKind::Audio;
        app
    }

    #[test]
    fn a_song_owned_armed_lane_can_begin_now_that_its_take_has_a_home() {
        let mut app = app_with_audio_lane();
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        app.song.tracks[0].blocks.clear();
        app.song.tracks[0].armed = true;
        app.song.tracks[0].input = TrackInput::Mono(0);
        app.project_song();
        app.transport.armed = true;
        let (tx, rx) = rtrb::RingBuffer::new(16);
        drop(tx);
        app.recorder = Some(record::Recorder::new(rx, 1, RATE));
        let root = std::env::temp_dir().join(format!("daw-p5-begin-{}", std::process::id()));
        app.project_path = Some(root.join("project.daw"));

        app.begin_recording();

        assert!(
            app.transport.armed,
            "the removed guard still dropped record"
        );
        assert!(
            app.recorder
                .as_ref()
                .is_some_and(record::Recorder::recording),
            "the Song route did not open a take"
        );
        let _ = app.recorder.as_mut().map(record::Recorder::finish);
        let _ = std::fs::remove_dir_all(root);
    }

    fn take() -> record::Take {
        record::Take {
            track: 0,
            path: std::path::PathBuf::from("known-take.wav"),
            frames: RATE as u64,
            channels: 1,
            sample_rate: RATE,
        }
    }

    #[test]
    fn a_song_owned_audio_take_lands_as_an_audio_block_and_survives_projection() {
        use daw::sequencing::TICKS_PER_BEAT;

        let mut app = app_with_audio_lane();
        app.transport.bpm = 120.0;
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        app.song.tracks[0].blocks.clear();
        app.song.tracks[0].name = "RECORDED AUDIO".to_owned();
        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        let table = daw::tempo::TempoTable::build(&app.song, RATE as f64, app.transport.bpm);
        let mut recorded = take();
        recorded.track = legacy;

        app.place_takes(
            vec![recorded],
            table.sample_at(4 * TICKS_PER_BEAT),
            0,
            0,
            RATE as u64,
        );

        assert_eq!(app.song.tracks[0].audio_blocks.len(), 1);
        let block = app.song.tracks[0].audio_blocks[0].clone();
        assert_eq!(block.start_tick, 4 * TICKS_PER_BEAT);
        assert_eq!(block.source.path, std::path::Path::new("known-take.wav"));
        assert!(
            app.arrangement.clips[legacy].is_empty(),
            "landed on the twin"
        );

        app.project_song();
        assert_eq!(app.arrangement.clips[legacy].len(), 1);
        app.song.tracks[0].volume = 0.5;
        app.project_song();
        assert_eq!(
            app.song.tracks[0].audio_blocks[0], block,
            "a later Song edit erased or rewrote the canonical take"
        );
        assert_eq!(app.arrangement.clips[legacy].len(), 1);
    }

    #[test]
    fn a_refused_song_landing_keeps_the_new_take_file() {
        let mut app = app_with_audio_lane();
        app.song.tracks[0].kind = daw::sequencing::TrackKind::Audio;
        app.song.tracks[0].name = "OCCUPIED AUDIO".to_owned();
        // Deliberately keep the default pattern block: both typed block
        // lists count as occupancy on an audio lane.
        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        let root = std::env::temp_dir().join(format!("daw-p5-kept-{}", std::process::id()));
        let path = root.join("new-take.wav");
        std::fs::create_dir_all(&root).expect("test directory");
        std::fs::write(&path, b"new take").expect("test take");
        let mut recorded = take();
        recorded.track = legacy;
        recorded.path = path.clone();

        app.place_takes(vec![recorded], 0, 0, 0, RATE as u64);

        assert!(app.song.tracks[0].audio_blocks.is_empty());
        assert!(path.exists(), "a refused landing deleted the new take");
        assert!(app.notice.as_deref().is_some_and(|notice| {
            notice.contains("LAND: BLOCK IN THE WAY") && notice.contains("FILE KEPT AT")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_midi_take_quantises_to_the_visible_grid_then_replaces_only_on_commit() {
        use daw::sequencing::{Note as SongNote, TICKS_PER_BEAT};

        let storage = shell::Storage::default();
        let mut app = App::new(&storage);
        app.transport.bpm = 120.0;
        app.song.tracks[0].name = "RECORDED MIDI".to_owned();
        let pattern_id = app.song.tracks[0].blocks[0].pattern_id;
        app.song
            .pattern_mut(pattern_id)
            .expect("default pattern")
            .set_primary(2, SongNote::new(48, 12, 90));
        app.project_song();
        let legacy = app.song_track_map[&app.song.tracks[0].id];
        let table = daw::tempo::TempoTable::build(&app.song, RATE as f64, app.transport.bpm);
        let before = app.song.clone();
        app.song_history = control_plane::SongHistory::new(before.clone());
        let take = record::MidiTake {
            track: legacy,
            notes: vec![record::MidiTakeNote {
                // Tick 13 rounds UP to tick 24 on the visible 1/8 grid.
                start_sample: table.sample_at(13),
                end_sample: table.sample_at(13 + TICKS_PER_BEAT),
                pitch: 67,
                velocity: 111,
            }],
        };

        app.preview_midi_take(take, RATE, 24, "1/8");

        assert_eq!(app.song, before, "the ghost preview changed the Song");
        let Some(redesign_bridge::SnapPreview::Midi { writes, .. }) = &app.snap_preview else {
            panic!("the take must stand as a MIDI preview");
        };
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].step, 2);
        assert_eq!(writes[0].after.notes.len(), 1);
        assert_eq!(
            writes[0].after.notes[0].pitch,
            daw::pitch::Pitch::from_midi(67)
        );
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| { notice.contains("ENTER REPLACES 1 COLLISION(S)") })
        );

        app.commit_snap_preview();

        let trig = app.song.pattern(pattern_id).expect("pattern").trig(2);
        assert_eq!(trig.notes.len(), 1, "the old trig was merged, not replaced");
        assert_eq!(trig.notes[0].pitch, daw::pitch::Pitch::from_midi(67));
        assert_eq!(trig.notes[0].length_ticks, TICKS_PER_BEAT);
        assert_eq!(
            app.sequence_cursor_tick,
            2 * daw::sequencing::PATTERN_STEP_TICKS
        );
        assert!(matches!(
            app.snap_preview,
            Some(redesign_bridge::SnapPreview::MidiResult { pattern }) if pattern == pattern_id
        ));

        // Song history observes the committed take as ONE edit, and undo
        // restores the collision whole rather than merely deleting the new
        // note and leaving an empty trig behind.
        app.center_song = true;
        let context = egui::Context::default();
        let mut frame = context.run_ui(egui::RawInput::default(), |_ui| {
            app.drive_song_history(&context);
        });
        frame.textures_delta.clear();
        let modifiers = egui::Modifiers::COMMAND;
        let undo = egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(modifiers),
                egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
            ],
            ..Default::default()
        };
        let mut frame = context.run_ui(undo, |_ui| app.drive_song_history(&context));
        frame.textures_delta.clear();
        assert_eq!(app.song, before, "undo did not restore the replaced trig");
    }

    /// A tempo change in front of the take changes samples-per-beat. The
    /// capture stamp therefore has to come back through the song's table,
    /// not through the transport's one fallback tempo.
    #[test]
    fn a_take_after_a_tempo_change_lands_on_the_tick_it_was_played_at() {
        use daw::sequencing::TICKS_PER_BEAT;

        let mut app = app_with_audio_lane();
        app.transport.bpm = 120.0;
        app.song.set_tempo_mark(0, 120.0);
        app.song.set_tempo_mark(4 * TICKS_PER_BEAT, 60.0);
        let played_tick = 5 * TICKS_PER_BEAT;
        let table = daw::tempo::TempoTable::build(&app.song, RATE as f64, app.transport.bpm);
        let captured_at = table.sample_at(played_tick);

        app.place_takes(vec![take()], captured_at, 0, 0, RATE as u64);

        assert_eq!(
            app.arrangement.clips[0][0].start,
            played_tick as f32 / TICKS_PER_BEAT as f32,
            "the flat transport tempo ignored the slower span before the take"
        );

        // With no marks, keep the legacy continuous beat exactly. Routing
        // every capture through ticks would quantize an old project to 1/48
        // beat merely because tempo maps were added to the model.
        let mut legacy = app_with_audio_lane();
        legacy.transport.bpm = 120.0;
        let captured_at = 12_345;
        let legacy_beat = captured_at as f64 / RATE as f64 * legacy.transport.bpm / 60.0;
        legacy.place_takes(vec![take()], captured_at, 0, 0, RATE as u64);
        assert_eq!(legacy.arrangement.clips[0][0].start, legacy_beat as f32);
    }

    /// Monitoring through a latent schedule makes the performance reach the
    /// input after both the stream and graph delays. Both must be removed or
    /// the recorded clip lands late by exactly the graph's PDC.
    #[test]
    fn a_take_monitored_through_pdc_lands_on_the_beat_it_was_played_at() {
        let mut spec = GraphSpec::default();
        let input = spec.push(NodeSpec::Input { channel: 0 });
        let filter = spec.push(NodeSpec::Filter {
            params: daw::audio::filter::FilterParams::default(),
        });
        spec.connect(input, filter);
        spec.set_output(filter);
        let pdc = spec
            .compile(RATE, 256)
            .expect("the monitoring graph compiles")
            .latency() as u64;
        assert!(pdc > 0, "the test needs a latency-bearing chain");

        let mut app = app_with_audio_lane();
        app.transport.bpm = 120.0;
        app.schedule_latency_frames = pdc;
        let played_beat = 4.0;
        let played_at = (played_beat * RATE as f64 * 60.0 / app.transport.bpm) as u64;
        let stream_latency = 256;
        let captured_at = played_at + stream_latency + pdc;

        app.place_takes(vec![take()], captured_at, stream_latency, 0, RATE as u64);

        assert_eq!(
            app.arrangement.clips[0][0].start, played_beat as f32,
            "the take stayed late by the schedule's {pdc}-frame PDC"
        );
    }
}
