//! Everything the app says to the audio engine, and everything it hears
//! back.
//!
//! One side of a wall. The engine's own rules live in `daw::audio`; this
//! is the green-zone half — building a schedule, pushing it, riding a
//! parameter as a letter rather than a recompile, and reading the meters
//! and telemetry that come back.
//!
//! It came out of `main.rs` as one piece because it already was one:
//! every method here is the app touching the engine, and none of them
//! touches anything else.

use super::*;

struct AuditionLoadCommand {
    serial: u64,
    path: PathBuf,
    sample_rate: u32,
}

struct AuditionLoadResult {
    serial: u64,
    buffer: Result<daw::audio::AuditionBuffer, String>,
}

/// Green-zone owner for browser audition decoding. The resident-material
/// loader is the application's existing decode/resample path; this worker
/// only gives it a request identity so a superseded cursor can never sound.
pub(crate) struct AuditionLoader {
    commands: crossbeam_channel::Sender<AuditionLoadCommand>,
    results: crossbeam_channel::Receiver<AuditionLoadResult>,
    next_serial: u64,
    wanted: Option<u64>,
}

impl AuditionLoader {
    pub(crate) fn start() -> Self {
        let (commands, command_rx) = crossbeam_channel::unbounded::<AuditionLoadCommand>();
        let (result_tx, results) = crossbeam_channel::unbounded::<AuditionLoadResult>();
        std::thread::Builder::new()
            .name("sample-audition".to_owned())
            .spawn(move || {
                while let Ok(mut command) = command_rx.recv() {
                    // A stable cursor is the only request worth decoding.
                    // Collapse anything that queued while the previous file
                    // was being read; all of this remains green-zone work.
                    while let Ok(newer) = command_rx.try_recv() {
                        command = newer;
                    }
                    let buffer = daw::audio::material::load(&command.path, command.sample_rate)
                        .map(daw::audio::AuditionBuffer::from_material)
                        .map_err(|error| error.to_string());
                    if result_tx
                        .send(AuditionLoadResult {
                            serial: command.serial,
                            buffer,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .expect("sample audition thread must start");
        Self {
            commands,
            results,
            next_serial: 0,
            wanted: None,
        }
    }

    pub(crate) fn request(&mut self, path: PathBuf, sample_rate: u32) {
        self.next_serial = self.next_serial.wrapping_add(1);
        let serial = self.next_serial;
        self.wanted = Some(serial);
        if self
            .commands
            .send(AuditionLoadCommand {
                serial,
                path,
                sample_rate,
            })
            .is_err()
        {
            self.wanted = None;
        }
    }

    pub(crate) fn stop(&mut self) {
        self.wanted = None;
    }

    pub(crate) fn try_result(&mut self) -> Option<Result<daw::audio::AuditionBuffer, String>> {
        while let Ok(result) = self.results.try_recv() {
            if self.wanted == Some(result.serial) {
                self.wanted = None;
                return Some(result.buffer);
            }
        }
        None
    }
}

impl App {
    pub(crate) fn shape_hash(&self) -> u64 {
        shape_hash(
            &self.arrangement.tracks,
            &self.arrangement.master,
            &self.arrangement.returns,
        )
    }

    /// The SHAPE of the modulation: which wires exist, what each drives,
    /// which sources exist and of what kind. Not the continuous values —
    /// depth, curve, steps, lag and an LFO's rate all ride live letters, so
    /// folding them in here would swap the whole schedule on every frame of
    /// a knob drag.
    pub(crate) fn mod_shape_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for wire in &self.arrangement.mod_wires {
            wire.id.hash(&mut hasher);
            wire.source.hash(&mut hasher);
            wire.track.hash(&mut hasher);
            wire.target.hash(&mut hasher);
        }
        // Ids in ORDER: telemetry lines sources up by position, so a
        // reorder has to reach the engine even though no source changed.
        // What a source IS travels as a live letter, follower track and
        // all, so it is deliberately not hashed.
        for modulator in &self.arrangement.modulators {
            modulator.id.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The stream to open, from the machine-local preferences.
    ///
    /// The one place `ui::prefs`' vocabulary becomes the engine's. Prefs
    /// may not name `crate::audio` — `ui::mod`'s layer test enforces it —
    /// so the translation lives here, in the app, exactly as a panel's
    /// wishes are translated here rather than performed by the panel.
    ///
    /// Every field falls back to the engine's own default, so a
    /// preferences file that predates this window opens what it always
    /// opened.
    pub(crate) fn engine_config(&self) -> EngineConfig {
        use daw::ui::prefs::AudioBackend;
        let base = EngineConfig::default();
        EngineConfig {
            api: match self.prefs.audio_backend {
                AudioBackend::Jack => daw::audio::AudioApi::Jack,
                AudioBackend::Alsa => daw::audio::AudioApi::Alsa,
                AudioBackend::Pulse => daw::audio::AudioApi::Pulse,
            },
            output_device: self.prefs.audio_device.clone(),
            sample_rate: self.prefs.audio_rate_hz.unwrap_or(base.sample_rate),
            buffer_frames: self.prefs.audio_buffer_frames.unwrap_or(base.buffer_frames),
            channels: base.channels,
        }
    }

    pub(crate) fn start_engine(&mut self) {
        if self.engine.is_some() {
            return;
        }
        match Engine::start(self.engine_config()) {
            Ok(mut engine) => {
                engine.transport(TransportCmd::SetTempo(self.transport.bpm));
                // The capture ring belongs to this stream and is taken
                // once. A recorder made here dies with the stream, which
                // is right: a take cannot outlive the clock it was
                // stamped against.
                let info = engine.info();
                self.recorder = engine
                    .take_capture()
                    .map(|ring| record::Recorder::new(ring, info.in_channels, info.sample_rate));
                self.engine = Some(engine);
                self.notice = None;
                self.sent_loop = None;
                self.push_graph();
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Close the stream. Dropping the Engine stops it; the transport mirror
    /// halts so the stand-in clock does not sprint off from where audio died.
    pub(crate) fn stop_engine(&mut self) {
        // Close the take before the stream goes: a half-written wav with
        // no header is not a recording, and the ring is about to be
        // dropped along with everything still in it.
        self.finish_recording();
        self.recorder = None;
        self.audition_loader.stop();
        self.engine = None;
        self.hud = None;
        self.device_nodes.clear();
        self.readout_slots.clear();
        self.clip_nodes.clear();
        self.sent_loop = None;
        self.last_compile = None;
        self.schedule_latency_frames = 0;
        self.transport.playing = false;
    }

    /// Compile the arrangement into a schedule and swap it in WHOLE —
    /// sequencing contract rule 4: compiled immutable chunks, never streamed.
    pub(crate) fn push_graph(&mut self) {
        let Some(info) = self.engine.as_ref().map(|e| e.info()) else {
            return;
        };
        // The transport loop is the TRANSPORT's business, never the
        // pattern's: patterns compile ONE-SHOT against the timeline, and a
        // loop wrap reaches every node as a discontinuity, which cuts its
        // voices and reseeks its cursor — the same machinery a seek uses.
        //
        // This used to pass the loop region's end as the pattern's clip
        // length "so the cycle and the wrap agree", and that coupling WAS
        // the bug it looks like: clip mode cycles `position % len`
        // wherever the playhead is, so parking the playhead past the
        // brace and pressing play replayed the loop's material over what
        // should be silence — and notes AFTER the loop end were dropped
        // from the compile entirely while looping was on. Clip mode
        // remains what it was built for: session clips, which unroll as
        // timeline copies in `Session::compiled` anyway.
        //
        // What plays, not what is drawn: a launched session clip overrides
        // its track's timeline. `compiled_clips` stores the same thing, so
        // the dirty check compares like with like.
        let playing = self.arrangement.effective_clips();
        let (mut spec, nodes) = build_graph_spec(
            &self.arrangement.tracks,
            &self.arrangement.master,
            &self.arrangement.returns,
            &playing,
            None,
            self.transport.metronome,
        );
        // Swing is baked into the note starts, at the arrangement's grid —
        // the knob in the transport says how far every other grid step
        // leans. Live playback and offline renders share this one call.
        spec.apply_swing(self.arrangement.grid_beats(), self.transport.swing);
        // Modulation rides INSIDE the schedule, so it reaches the callback
        // and the offline renderer by the same road the audio does.
        spec.set_modulation(build_mod_spec(
            &self.arrangement.tracks,
            &self.arrangement.modulators,
            &self.arrangement.mod_wires,
            &self.parameter_registry,
            &nodes,
        ));
        match spec.compile_at_tempo(info.sample_rate, info.max_frames, self.transport.bpm) {
            Ok(sched) => {
                // GREEN ZONE: once the box crosses `set_schedule`, the
                // callback owns it and the recorder has no lawful way to
                // ask it how late the monitored signal is.
                let schedule_latency_frames = sched.latency() as u64;
                let Some(engine) = &mut self.engine else {
                    return;
                };
                match engine.set_schedule(Box::new(sched)) {
                    Ok(()) => {
                        self.schedule_latency_frames = schedule_latency_frames;
                        self.device_nodes = nodes.devices;
                        self.readout_slots = nodes.readouts;
                        self.clip_nodes = nodes.audio_clips;
                        self.pan_ids = nodes.pans;
                        self.send_ids = nodes.sends;
                        self.return_ids = nodes.returns;
                        self.master_out = nodes.master_out;
                        // Fresh ids: every track's pan must be re-sent, so
                        // nothing survives a swap sitting at the node's
                        // compiled-in default while the knob says otherwise.
                        self.sent_pan.clear();
                        self.sent_volume.clear();
                        // A fresh swap mints fresh ids: what was sent to
                        // the old master node was sent to a node that no
                        // longer exists.
                        self.sent_master_pan = f32::NAN;
                        self.sent_master_volume = f32::NAN;
                        self.sent_send.clear();
                        self.sent_return.clear();
                        self.sent_automation.clear();
                        self.compiled_clips = playing;
                        self.graph_key = (
                            self.transport.metronome,
                            self.arrangement.tracks.len(),
                            self.shape_hash(),
                            self.transport.bpm.to_bits(),
                            u64::from(self.transport.swing.to_bits()),
                            self.mod_shape_hash(),
                        );
                        self.last_compile = Some(Instant::now());
                    }
                    Err(e) => self.notice = Some(e.to_string()),
                }
            }
            Err(e) => self.notice = Some(format!("graph refused: {e}")),
        }
    }

    /// Once per frame while a stream runs: retire trashed schedules, mirror
    /// the transport from telemetry, refresh the bar's numbers, judge health.
    /// Runs before anything draws, so the frame shows current numbers.
    pub(crate) fn pump_engine(&mut self, ctx: &egui::Context) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.collect_trash();
        let info = engine.info();
        let snap = engine.latest_block();
        let peaks = snap.track_peaks;
        // One reading per device per FRAME, from the block the engine
        // just finished. The engine reports the block's extremes rather
        // than its newest sample, so a transient between two repaints is
        // still in here — see `graph::Readout`.
        for (id, slot) in &self.readout_slots {
            let Some(said) = snap.device_readouts.get(*slot) else {
                continue;
            };
            self.device_histories
                .entry(*id)
                .or_default()
                .push(device::scope::Reading {
                    level_db: said.level_db,
                    reduction_db: said.reduction_db,
                    bands: said.bands,
                });
        }
        self.mod_telemetry = Some(ModTelemetry {
            sources: snap.mod_sources,
            wire_ids: snap.mod_wire_ids,
            wires: snap.mod_wires,
        });
        self.transport.playing = snap.playing;
        self.transport.position = snap.position as f64 / f64::from(info.sample_rate.max(1));
        self.hud = Some(EngineHud {
            load_pct: (snap.load(info.sample_rate, info.max_frames as u32) * 100.0) as f32,
            xruns: snap.underflows + snap.overflows,
        });

        match engine.health() {
            StreamHealth::Running => {}
            StreamHealth::Stalled { seconds } => {
                self.notice = Some(format!(
                    "stream dead — no blocks for {seconds:.1}s; power off and on to reconnect"
                ));
            }
            StreamHealth::Errored(err) => {
                self.notice = Some(format!(
                    "stream error — {err}; power off and on to reconnect"
                ));
            }
        }
        // After the engine borrow is done with: the meters are UI state,
        // fed from the block that was just read.
        self.advance_meters(&peaks, ctx.input(|i| i.stable_dt));
        // Telemetry only moves if frames keep coming.
        ctx.request_repaint();
    }

    /// This frame's modulation readings, for the strip's animation and the
    /// scopes.
    ///
    /// The engine EVALUATES modulation now — this only reports it. While a
    /// stream runs, the values come back through telemetry, so what a scope
    /// draws is what a node actually received. With no engine there is
    /// nothing to report, so the same arithmetic runs here instead and the
    /// strip keeps moving with the stream powered off.
    pub(crate) fn pump_modulators(&mut self, dt: f32) {
        self.clock_seconds += dt.max(0.0);
        match self.mod_telemetry.take() {
            Some(telemetry) => self.read_modulation_telemetry(&telemetry),
            None => self.simulate_modulation(dt),
        }
        for wire in &self.arrangement.mod_wires {
            let span = self
                .parameter_registry
                .spec(&wire.target)
                .map_or(0.0, |spec| spec.max - spec.min);
            let output = self.wire_outputs.get(&wire.id).copied().unwrap_or(0.0);
            let scope = self.wire_scopes.entry(wire.id).or_default();
            // ~4 seconds at 60fps, normalized to the target's range so the
            // scope reads the same for a dB wire and a percent one.
            scope.push_back(if span > 0.0 { output / span } else { 0.0 });
            while scope.len() > 240 {
                scope.pop_front();
            }
        }
        // A removed wire takes its memory with it.
        let alive: std::collections::HashSet<u64> = self
            .arrangement
            .mod_wires
            .iter()
            .map(|wire| wire.id)
            .collect();
        self.wire_outputs.retain(|id, _| alive.contains(id));
        self.wire_scopes.retain(|id, _| alive.contains(id));
        if self.expanded_wire.is_some_and(|id| !alive.contains(&id)) {
            self.expanded_wire = None;
        }
    }

    /// Take the engine's readings. Sources arrive in the arrangement's
    /// order; wires arrive with their ids, because a wire whose target did
    /// not compile is absent and index alignment would silently attribute
    /// one wire's motion to another.
    pub(crate) fn read_modulation_telemetry(&mut self, telemetry: &ModTelemetry) {
        self.mod_values.clear();
        for (modulator, value) in self
            .arrangement
            .modulators
            .iter()
            .zip(telemetry.sources.iter())
        {
            self.mod_values.insert(modulator.id, *value);
        }
        self.wire_outputs.clear();
        for (id, value) in telemetry.wire_ids.iter().zip(telemetry.wires.iter()) {
            if *id != 0 {
                self.wire_outputs.insert(*id, *value);
            }
        }
    }

    /// The engine-off fallback: the same chain the engine runs, on the UI
    /// clock. Nothing is sounding, so this drives the animation only —
    /// which is why free modulators may use the wall clock here without
    /// costing anyone a reproducible render.
    pub(crate) fn simulate_modulation(&mut self, dt: f32) {
        let beat = (self.transport.position * self.transport.bpm / 60.0) as f32;
        let levels: Vec<f32> = self
            .meters
            .iter()
            .map(|m| device::meter::db_to_norm(m.shown_db))
            .collect();
        self.mod_values.clear();
        for modulator in &self.arrangement.modulators {
            self.mod_values.insert(
                modulator.id,
                modulator_value(&modulator.kind, beat, self.clock_seconds, &levels),
            );
        }
        // Solo is global: while any wire is soloed, the others aim at zero
        // — through their own smoothing, so an audition switch is a glide.
        let any_solo = self.arrangement.mod_wires.iter().any(|wire| wire.solo);
        for wire in &self.arrangement.mod_wires {
            let muted = !wire.enabled || (any_solo && !wire.solo);
            let span = self
                .parameter_registry
                .spec(&wire.target)
                .map_or(0.0, |spec| spec.max - spec.min);
            let source = self.mod_values.get(&wire.source).copied().unwrap_or(0.0);
            let previous = self.wire_outputs.get(&wire.id).copied();
            let output = wire_contribution(wire, source, span, previous, dt, muted);
            self.wire_outputs.insert(wire.id, output);
        }
    }

    /// Send the engine any wire chain or source definition that has moved
    /// since the last frame. Only-on-change, like every other letter: a
    /// still modulation matrix costs nothing, and a knob under the hand
    /// costs one small message per changed frame instead of a debounced
    /// schedule swap a second later.
    ///
    /// STRUCTURE is not sent here — adding, deleting or retargeting a wire
    /// changes the plan's shape and rides a recompile, which is what
    /// `mod_shape_hash` makes the dirty check notice.
    pub(crate) fn sync_modulation(&mut self) {
        use daw::audio::modulation::{ModEdit, WireEdit};

        // Prune first, and unconditionally: a re-used id must never find a
        // stale "already sent" entry waiting for it.
        let wires: std::collections::HashSet<u64> =
            self.arrangement.mod_wires.iter().map(|w| w.id).collect();
        let sources: std::collections::HashSet<u64> =
            self.arrangement.modulators.iter().map(|m| m.id).collect();
        self.sent_mod_wires.retain(|id, _| wires.contains(id));
        self.sent_mod_sources.retain(|id, _| sources.contains(id));

        // With no engine there is nobody to tell, and recording these as
        // sent would swallow every edit made while the stream was off.
        if self.engine.is_none() {
            return;
        }

        let mut edits: Vec<ModEdit> = Vec::new();
        for wire in &self.arrangement.mod_wires {
            let edit = WireEdit {
                id: wire.id,
                chain: wire.chain(),
                enabled: wire.enabled,
                solo: wire.solo,
            };
            if self.sent_mod_wires.get(&wire.id) != Some(&edit) {
                edits.push(ModEdit::Wire(edit));
                self.sent_mod_wires.insert(wire.id, edit);
            }
        }
        for modulator in &self.arrangement.modulators {
            if self.sent_mod_sources.get(&modulator.id) != Some(&modulator.kind) {
                edits.push(ModEdit::Source {
                    id: modulator.id,
                    kind: modulator.kind,
                });
                self.sent_mod_sources.insert(modulator.id, modulator.kind);
            }
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        for edit in edits {
            engine.set_modulation(edit);
        }
    }

    pub(crate) fn advance_meters(&mut self, peaks: &[f32], dt: f32) {
        self.meters
            .resize_with(self.arrangement.tracks.len(), Default::default);
        for (track, meter) in self.meters.iter_mut().enumerate() {
            let peak = peaks.get(track).copied().unwrap_or(0.0);
            meter.advance(
                device::meter::amp_to_db(peak).max(device::meter::FLOOR_DB),
                dt,
            );
        }
        // The returns read the slots handed out downwards from just
        // under the master's, which is where the graph put them.
        self.return_meters
            .resize_with(self.arrangement.returns.len(), Default::default);
        for (index, meter) in self.return_meters.iter_mut().enumerate() {
            let peak = peaks
                .get(MASTER_METER.saturating_sub(1 + index))
                .copied()
                .unwrap_or(0.0);
            meter.advance(
                device::meter::amp_to_db(peak).max(device::meter::FLOOR_DB),
                dt,
            );
        }
        // The master reads its reserved slot, not a lane's.
        let peak = peaks.get(MASTER_METER).copied().unwrap_or(0.0);
        self.master_meter.advance(
            device::meter::amp_to_db(peak).max(device::meter::FLOOR_DB),
            dt,
        );
    }

    /// Translate this frame's transport wishes into engine commands. Runs
    /// BEFORE `perform` flips the mirror, so TogglePlay reads the state the
    /// user saw. The mirror is still updated by `perform` — it is what the
    /// buttons light from, and the whole transport when the engine is off.
    pub(crate) fn route_transport(&mut self, actions: &[UiAction]) {
        let playing = self.transport.playing;
        let Some(engine) = &mut self.engine else {
            return;
        };
        for action in actions {
            match action {
                UiAction::TogglePlay | UiAction::ContinuePlay => {
                    engine.transport(if playing {
                        TransportCmd::Stop
                    } else {
                        TransportCmd::Play
                    });
                }
                // The seek that aims it rides `pending_seek`, consumed just
                // after; Play-while-playing is a no-op, so this is safe
                // whatever state the engine is in.
                UiAction::PlaySelection => engine.transport(TransportCmd::Play),
                // Halt, hold — the engine's Stop.
                UiAction::Pause => engine.transport(TransportCmd::Stop),
                // Halt AND rewind — the engine's Return.
                UiAction::Stop => engine.transport(TransportCmd::Return),
                // Rewind without halting: a seek leaves `playing` alone.
                UiAction::Return => engine.transport(TransportCmd::Seek(0)),
                UiAction::SetTempo(bpm) => engine.transport(TransportCmd::SetTempo(
                    bpm.clamp(limits::BPM_MIN, limits::BPM_MAX),
                )),
                _ => {}
            }
        }
    }

    /// End of frame: reconcile the loop region and the schedule with what
    /// the UI now says. One door for every way the loop or graph can change
    /// (buttons, Ctrl+L, brace drags, tempo, note edits).
    pub(crate) fn sync_engine(&mut self) {
        if self.engine.is_none() {
            return;
        }
        // Loop points ride beats -> samples through the engine's own rule:
        // 60/bpm * rate, rounded — same formula, same rounding (TimeMap).
        let want = if self.transport.loop_on {
            self.arrangement.loop_range.map(|(from, to)| {
                let spb = 60.0 / self.transport.bpm
                    * f64::from(self.engine.as_ref().map_or(0, |e| e.info().sample_rate));
                (
                    (f64::from(from) * spb).round() as u64,
                    (f64::from(to) * spb).round() as u64,
                )
            })
        } else {
            None
        };
        if want != self.sent_loop {
            if let Some(engine) = &mut self.engine {
                match want {
                    Some((start, end)) => engine.transport(TransportCmd::SetLoop { start, end }),
                    None => engine.transport(TransportCmd::ClearLoop),
                }
            }
            self.sent_loop = want;
        }

        self.sync_pans();
        // The wires' own knobs, on the live door — a depth drag must be
        // heard now, and a schedule swap is debounced to once a second.
        self.sync_modulation();

        // The edits that cannot wait, and cannot be inferred: a reorder
        // (which renumbers the ids and pans the app holds by index, while
        // two lanes can trade places without changing shape or clips) and
        // a session launch (where waiting out the debounce would make the
        // grid feel broken). Recompiling re-captures every id and clears
        // `sent_pan`, which is the only way back into step.
        if std::mem::take(&mut self.arrangement.force_recompile) {
            self.push_graph();
            return;
        }
        // Shape changes (metronome, loop length, track count) swap now: they
        // add or remove nodes, which no letter can express. Clip edits are
        // debounced so a drag lands as one swap, not sixty.
        let shape = (
            self.transport.metronome,
            self.arrangement.tracks.len(),
            self.shape_hash(),
            self.transport.bpm.to_bits(),
            u64::from(self.transport.swing.to_bits()),
            self.mod_shape_hash(),
        );
        if shape != self.graph_key {
            self.push_graph();
            return;
        }
        let since = self
            .last_compile
            .map_or(f64::INFINITY, |t| t.elapsed().as_secs_f64());
        // The dirty check covers every way the sounding material can move:
        // notes edited, clips created, deleted, dragged, resized, renamed.
        // Synth params are deliberately NOT in it — they ride param letters
        // that have already landed, and a swap purely for a knob turn would
        // cut sounding voices. `push_graph` reads the tracks' current values,
        // so the next swap for any other reason bakes them in.
        if recompile_due(
            self.arrangement.effective_clips() != self.compiled_clips,
            since,
        ) {
            self.push_graph();
        }
    }

    /// Knob edits from a track's synth card: remembered on that track so the
    /// next recompile preserves them, and sent live to that track's Seq node.
    /// The param ids are the wire contract with `Node::Seq::apply`
    /// (0 gain, 1 attack ms, 2 release ms).
    /// Reverb knob edits: stored on the track and sent to that track's
    /// EFFECT node. The two cards number their parameters the same way, so
    /// this deliberately does not share a path with the synth's letters.
    /// Send a pan letter for every track whose pan has moved since the
    /// last one. `NodeSpec::Pan` param 0 is pan, `-1..=1`.
    ///
    /// A LETTER, not a recompile: the node already exists on every
    /// instrument track, so dragging the header knob costs one 16-byte
    /// message per changed frame instead of a schedule swap per frame.
    pub(crate) fn sync_pans(&mut self) {
        let n = self.arrangement.tracks.len();
        // `f32::NAN != NAN`, so a freshly grown slot always sends once.
        self.sent_pan.resize(n, f32::NAN);
        self.sent_volume.resize(n, f32::NAN);
        let beat = (self.transport.position * self.transport.bpm / 60.0) as f32;
        for i in 0..n {
            let track = &self.arrangement.tracks[i];
            // The BASE only. Modulation rides on top of this inside the
            // engine, which is what lets a fader stay meaningful under a
            // running LFO — and what stops this loop sending a letter per
            // frame per modulated track. A still fader now sends nothing.
            let pan = track
                .automation
                .value_at(TRACK_PAN_TARGET, beat, track.pan)
                .clamp(-1.0, 1.0);
            let volume = track
                .automation
                .value_at(TRACK_VOLUME_TARGET, beat, track.volume)
                .max(0.0);
            if pan == self.sent_pan[i] && volume == self.sent_volume[i] {
                continue;
            }
            let Some(Some(node)) = self.pan_ids.get(i).copied() else {
                continue;
            };
            let Some(engine) = &mut self.engine else {
                return;
            };
            // Pan and volume live on the same node and reconcile through
            // the same door, so a fader move and a knob turn are one
            // comparison each and never a recompile.
            if pan != self.sent_pan[i] {
                engine.set_param(node, daw::params::pan::PAN, pan);
                self.sent_pan[i] = pan;
            }
            if volume != self.sent_volume[i] {
                engine.set_param(node, daw::params::pan::GAIN, volume);
                self.sent_volume[i] = volume;
            }
        }
        self.sync_sends();
        self.sync_returns();
        self.sync_master(beat);
        self.sync_device_automation(beat);
    }

    /// Every send level, through the same only-on-change door the faders
    /// use.
    ///
    /// This is what the per-pair gain node bought: a send opens, closes
    /// and rides an automation curve as a stream of letters, and the
    /// schedule never moves. A send with no compiled node — a muted
    /// return, a silent lane — is skipped rather than queued, because a
    /// letter to a node that does not exist has nowhere to arrive.
    pub(crate) fn sync_sends(&mut self) {
        let tracks = self.arrangement.tracks.len();
        let returns = self.arrangement.returns.len();
        self.sent_send.resize(tracks, Vec::new());
        for row in &mut self.sent_send {
            row.resize(returns, f32::NAN);
        }
        for track in 0..tracks {
            for index in 0..returns {
                let level = self.arrangement.tracks[track]
                    .sends
                    .get(index)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                if level == self.sent_send[track][index] {
                    continue;
                }
                let Some(Some(node)) = self
                    .send_ids
                    .get(track)
                    .and_then(|row| row.get(index))
                    .copied()
                else {
                    continue;
                };
                let Some(engine) = &mut self.engine else {
                    return;
                };
                engine.set_param(node, daw::params::mixer::GAIN, level);
                self.sent_send[track][index] = level;
            }
        }
    }

    /// Each return's own fader and pan, on the same door.
    pub(crate) fn sync_returns(&mut self) {
        let returns = self.arrangement.returns.len();
        self.sent_return.resize(returns, (f32::NAN, f32::NAN));
        for index in 0..returns {
            let bus = &self.arrangement.returns[index];
            let pan = bus.pan.clamp(-1.0, 1.0);
            let volume = bus.volume.max(0.0);
            if (pan, volume) == self.sent_return[index] {
                continue;
            }
            let Some(Some(node)) = self.return_ids.get(index).copied() else {
                continue;
            };
            let Some(engine) = &mut self.engine else {
                return;
            };
            if pan != self.sent_return[index].0 {
                engine.set_param(node, daw::params::pan::PAN, pan);
            }
            if volume != self.sent_return[index].1 {
                engine.set_param(node, daw::params::pan::GAIN, volume);
            }
            self.sent_return[index] = (pan, volume);
        }
    }

    /// The master fader and pan, through the same only-on-change door the
    /// lanes use. It carries no automation of its own yet, so the values
    /// are the ones on the strip.
    pub(crate) fn sync_master(&mut self, _beat: f32) {
        let pan = self.arrangement.master.pan.clamp(-1.0, 1.0);
        let volume = self.arrangement.master.volume.max(0.0);
        if pan == self.sent_master_pan && volume == self.sent_master_volume {
            return;
        }
        let Some(node) = self.master_out else {
            return;
        };
        let Some(engine) = &mut self.engine else {
            return;
        };
        if pan != self.sent_master_pan {
            engine.set_param(node, daw::params::pan::PAN, pan);
            self.sent_master_pan = pan;
        }
        if volume != self.sent_master_volume {
            engine.set_param(node, daw::params::pan::GAIN, volume);
            self.sent_master_volume = volume;
        }
    }

    /// The generic half of playback dispatch: every OTHER envelope a track
    /// carries, resolved through `target_binding` to the node vec and
    /// ParamChange id it drives. Volume and pan keep their dedicated path
    /// above — they exist on every track and earn their two flat vecs —
    /// while device targets ride this map, which only holds what is
    /// actually automated.
    pub(crate) fn sync_device_automation(&mut self, beat: f32) {
        let n = self.arrangement.tracks.len();
        self.sent_automation.resize_with(n, HashMap::new);
        for i in 0..n {
            let track = &self.arrangement.tracks[i];
            let mut sends: Vec<(NodeId, u32, String, f32)> = Vec::new();
            for envelope in &track.automation.envelopes {
                let target = envelope.target.as_str();
                // Volume and pan keep the dedicated path above.
                let Some(TargetRef::Device { id, param }) = target_ref(target) else {
                    continue;
                };
                // A curve for a device the track does not carry automates
                // nothing — and must not letter some other node.
                if !target_applies(track, target) {
                    continue;
                }
                let Some(spec) = self.parameter_registry.spec(target) else {
                    continue;
                };
                let base = parameter_base(track, target, spec);
                // The base only; the engine adds modulation on top.
                let value = track
                    .automation
                    .value_at(target, beat, base)
                    .clamp(spec.min, spec.max);
                if self.sent_automation[i].get(target) == Some(&value) {
                    continue;
                }
                let Some(node) = self.device_nodes.get(&id).copied() else {
                    continue;
                };
                sends.push((node, param, envelope.target.clone(), value));
            }
            for (node, param, target, value) in sends {
                let Some(engine) = &mut self.engine else {
                    return;
                };
                engine.set_param(node, param, value);
                self.sent_automation[i].insert(target, value);
            }
        }
    }

    /// Knob edits from one device's card: remembered on that instance so
    /// the next recompile preserves them, and sent live to the node that
    /// instance compiled to. The instance id is what keeps a letter on its
    /// own device — the two cards number their parameters the same way, and
    /// a misrouted letter would send a reverb's mix to a synth's gain.
    /// What the clip editor's controls changed, written down and — where
    /// the engine can hear it without a rebuild — sent.
    ///
    /// GAIN rides a letter. `Node::AudioClip` ramps its own gain, so the
    /// change is click-free and a drag is heard AS IT HAPPENS rather than
    /// when the debounced recompile catches up; that debounce exists so a
    /// clip drag lands as one swap instead of sixty, and it would make a
    /// gain fader feel like it was underwater.
    ///
    /// LOOPING cannot: whether a clip repeats is baked into the node at
    /// compile, so it takes the ordinary road and the dirty check swaps
    /// the schedule. That is one swap for a switch you flip occasionally,
    /// which is the right trade in the other direction.
    pub(crate) fn apply_clip_edit(&mut self, edit: waveform::ClipEdit) {
        let Some((track, index)) = self.arrangement.selected_clip else {
            return;
        };
        let Some(clip) = self
            .arrangement
            .clips
            .get_mut(track)
            .and_then(|clips| clips.get_mut(index))
        else {
            return;
        };
        let id = clip.id;
        let Some(audio) = clip.audio.as_mut() else {
            return;
        };
        // The pitch knobs need the WHOLE app (the render worker), so the
        // ask rides out of the borrow and lands after the match — the
        // same pattern the ghost and the menu keep.
        let mut transpose_req: Option<(f32, f32)> = None;
        match edit {
            waveform::ClipEdit::Gain(gain) => {
                let gain =
                    daw::params::def(daw::params::clip::TABLE, daw::params::clip::GAIN).clamp(gain);
                audio.gain = gain;
                if let Some(node) = self.clip_nodes.get(&id).copied()
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(node, daw::params::clip::GAIN, gain);
                }
            }
            waveform::ClipEdit::Looped(looped) => {
                audio.looped = looped;
                // No letter for this one — the next dirty check rebuilds.
                self.arrangement.force_recompile = true;
            }
            // SLIDES the window: the clip keeps its duration in frames
            // and its length on the timeline, and a different stretch of
            // the file lands inside it. Resizing is what the clip's own
            // edges are for — `trim_clip_left` owns that rule, and it
            // holds the source END invariant, which this deliberately
            // does not.
            waveform::ClipEdit::SourceOffset(frames) => {
                audio.source_offset = frames;
                self.arrangement.force_recompile = true;
            }
            // The FILE is reversed into a cache and the clip points at
            // it; the node streams forward through it exactly as before,
            // so the audio callback learns nothing new. The work is a
            // decode and a write, so it goes to the same worker every
            // other sample-file job uses — and the clip is silent until
            // the file lands, which `playing_path` decides rather than
            // this.
            //
            // Turning it OFF is free: the forward file is already there.
            waveform::ClipEdit::Reversed(reversed) => {
                audio.reversed = reversed;
                let path = audio.path.clone();
                let ready = audio.playing_path().is_some();
                self.arrangement.force_recompile = true;
                if reversed && !ready {
                    let asked = path.canonicalize().unwrap_or(path.clone());
                    self.pending_reversals.insert(asked);
                    self.wav_import_service.reverse(path);
                    self.wav_import_pending = self.wav_import_pending.saturating_add(1);
                }
            }
            // Fades ride LETTERS, like gain: the node keeps them as
            // fields and applies them per sample, so dragging a handle is
            // heard while it is dragged. Clamped to the clip's span here
            // as well as in the node — the panel should not be able to
            // ask for a fade the engine will quietly cut down.
            waveform::ClipEdit::FadeIn(frames) | waveform::ClipEdit::FadeOut(frames) => {
                let leading = matches!(edit, waveform::ClipEdit::FadeIn(_));
                if leading {
                    audio.fade_in = frames;
                } else {
                    audio.fade_out = frames;
                }
                let param = if leading {
                    daw::params::clip::FADE_IN
                } else {
                    daw::params::clip::FADE_OUT
                };
                if let Some(node) = self.clip_nodes.get(&id).copied()
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(node, param, frames as f32);
                }
            }
            // The SHAPES ride letters exactly as the lengths do, so
            // dragging a curve is heard while it is dragged. Clamped
            // here as well as in `Curve::new` — the panel should not be
            // able to ask for a shape the node will quietly cut down.
            waveform::ClipEdit::FadeInCurve(shape) | waveform::ClipEdit::FadeOutCurve(shape) => {
                let leading = matches!(edit, waveform::ClipEdit::FadeInCurve(_));
                let shape = if shape.is_finite() {
                    shape.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let param = if leading {
                    audio.fade_in_curve = shape;
                    daw::params::clip::FADE_IN_CURVE
                } else {
                    audio.fade_out_curve = shape;
                    daw::params::clip::FADE_OUT_CURVE
                };
                if let Some(node) = self.clip_nodes.get(&id).copied()
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(node, param, shape);
                }
            }
            // A LIST CANNOT RIDE A LETTER, so the envelope takes the
            // ordinary road and the debounced dirty check swaps the
            // schedule. That debounce is what makes a drag one swap
            // rather than sixty — and an envelope point is dragged for a
            // moment, not ridden like a fader.
            waveform::ClipEdit::Envelope(points) => {
                let mut points: Vec<(u64, f32)> = points
                    .into_iter()
                    .filter(|(_, db)| db.is_finite())
                    .map(|(at, db)| (at, db.clamp(ENVELOPE_FLOOR_DB, ENVELOPE_CEIL_DB)))
                    .collect();
                points.sort_by_key(|(at, _)| *at);
                points.dedup_by_key(|(at, _)| *at);
                // A flat unity envelope is NO envelope. Storing one would
                // cost the node a per-sample multiply to change nothing,
                // and would make "has an envelope" untrue of a clip that
                // does not.
                let flat = points.iter().all(|(_, db)| db.abs() < 1e-4);
                audio.envelope = if flat { Vec::new() } else { points };
                self.arrangement.force_recompile = true;
            }
            waveform::ClipEdit::Rename(name) => {
                // Never empty: the panel refuses one, and so does this,
                // because a clip with no name cannot be found in a list.
                if !name.trim().is_empty() {
                    clip.name = name;
                }
            }
            waveform::ClipEdit::Transpose { semitones, cents } => {
                transpose_req = Some((semitones, cents));
            }
        }
        if let Some((semitones, cents)) = transpose_req {
            self.set_clip_transpose(semitones, cents);
        }
    }

    /// One device out of whichever chain owns it.
    pub(crate) fn chain_device_mut(
        &mut self,
        owner: ChainOwner,
        device: u64,
    ) -> Option<&mut DeviceInstance> {
        match owner {
            ChainOwner::Track(track) => self
                .arrangement
                .tracks
                .get_mut(track)
                .and_then(|t| t.device_mut(device)),
            ChainOwner::Return(bus) => self
                .arrangement
                .returns
                .get_mut(bus)
                .and_then(|bus| bus.device_mut(device)),
            ChainOwner::Master => self.arrangement.master.device_mut(device),
        }
    }

    pub(crate) fn apply_device_edits(
        &mut self,
        owner: ChainOwner,
        device: u64,
        edits: &[device::ParamEdit],
    ) {
        let Some(instance) = (match owner {
            ChainOwner::Track(track) => self
                .arrangement
                .tracks
                .get_mut(track)
                .and_then(|t| t.device_mut(device)),
            ChainOwner::Return(bus) => self
                .arrangement
                .returns
                .get_mut(bus)
                .and_then(|bus| bus.device_mut(device)),
            ChainOwner::Master => self.arrangement.master.device_mut(device),
        }) else {
            return;
        };
        // Whether this device is an insert or an aux, BEFORE the edits
        // land: a send that crosses zero rewires the track, and the
        // letters below would be addressed to a node that is about to be
        // retired. The recompile carries the new value instead — see
        // `shape_hash`.
        let was_aux = match instance.state {
            DeviceState::Echo(params) => Some(params.send > 0.0),
            _ => None,
        };
        for edit in edits {
            instance.state.set(edit.param, edit.value);
        }
        let reshaped = match (was_aux, instance.state) {
            (Some(before), DeviceState::Echo(params)) => before != (params.send > 0.0),
            _ => false,
        };
        if reshaped {
            return;
        }
        // A device that is not in the schedule — bypassed, or on a muted
        // track — has nowhere to send to. The next swap bakes the values in.
        let Some(node) = self.device_nodes.get(&device).copied() else {
            return;
        };
        if let Some(engine) = &mut self.engine {
            for edit in edits {
                engine.set_param(node, edit.param, edit.value);
            }
        }
    }
}
