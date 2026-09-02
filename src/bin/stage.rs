//! `stage` — the third UI, from nothing.
//!
//!     cargo run --bin stage
//!
//! Its own binary on purpose. `daw` is a working DAW and stays that way
//! while this is built beside it: nothing here touches `main.rs`, and
//! there is no third state to invent on a key that already flips between
//! two. When the stage is ready to be the app, it becomes the default —
//! until then the two are simply different programs over one library.
//!
//! What it inherits for free, because they are frame-independent: the
//! model, the intent vocabulary, the device cards and the design system.
//! What it inherits deliberately NOTHING of: the layout, the key map, and
//! the panel structure of either frame before it.
//!
//! # Where the audio lives, and why it is not in the frame
//!
//! **This file owns the `Engine`. `ui::stage` does not, and may not.**
//! The UI's standing rule is that a surface never imports `crate::audio`:
//! it renders state it is handed and reports what the user asked for,
//! while the app layer owns the engine and translates between them. The
//! stage keeps that rule — it exposes the document, what is playing
//! and a revision that changes when what should be sounding changes, and
//! it takes levels back in. Everything between those two facts is here.
//!
//! The loop is: the stage edits the song, the revision moves, this file
//! recompiles the graph and hands it to the engine, and the engine's
//! meters come back the other way. Nothing else crosses.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use daw::audio::AuditionBuffer;
use daw::audio::bounce::{BounceFormat, BounceOptions, bounce_automated};
use daw::audio::material;
use daw::audio::transport::TransportCmd;
use daw::audio::{Engine, EngineConfig, StreamHealth};
use daw::design::Polarity;
use daw::install_stage_fonts;
use daw::library::{LibraryConfig, LibrarySnapshot};
use daw::params;
use daw::shell;
use daw::song_graph::{self, MASTER_METER, SongNodes};
use daw::ui::stage::{EngineState, Health, Level, SampleData, Stage, Stream};
use daw::ui::theme::Theme;
use eframe::egui;

/// The library the stage browses: the folders a musician keeps sounds
/// in, when they exist. Said here rather than guessed by the stage, so
/// a headless stage scans nothing and a real one scans what is there.
fn library_config() -> LibraryConfig {
    let mut config = LibraryConfig::default();
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return config;
    };
    for folder in [
        home.join("Samples"),
        home.join("Music").join("samples"),
        home.join("Music").join("Samples"),
    ] {
        if folder.is_dir() {
            let _ = config.add_sample_folder(&folder);
        }
    }
    config
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    keep_the_stream_driven();
    shell::run("daw — stage", [1280.0, 800.0], [720.0, 480.0], App::new)
}

/// Ask PipeWire to keep the stage's stream processing and to give it a
/// driver of its own.
///
/// Without this a JACK client on PipeWire is scheduled only while the
/// graph around it is awake: with the sink muted or idle the server picks
/// whatever node is running — the microphone, as often as not — as the
/// client's driver, and when THAT suspends the callback stops and the
/// stage reports a stalled engine for no reason of its own. The two
/// properties are what `pw-jack` sets for a client that must never
/// sleep; pipewire-jack reads them from this variable. A user who set the
/// variable themselves is left alone.
fn keep_the_stream_driven() {
    // The quantum is forced to the engine's own block: PipeWire hands a
    // JACK client the graph's quantum whatever the client asked for, and
    // a block larger than the arena is refused as silence — which reads
    // as a stalled engine.
    const PROPS: &str =
        "{ node.always-process = true node.want-driver = true node.force-quantum = 256 }";
    if std::env::var_os("PIPEWIRE_PROPS").is_none() {
        // Set before any thread exists — the engine's are made in
        // `App::new` — which is what makes this sound.
        unsafe { std::env::set_var("PIPEWIRE_PROPS", PROPS) };
    }
}

struct App {
    stage: Stage,
    audio: Audio,
    /// The ground the egui context was last told about. The stage paints
    /// its own marks from the alphabet, but stock widgets — text edits,
    /// scrollbars, the palette's own frame — read the runtime theme, and
    /// a light page with dark scrollbars is two grounds on one screen.
    ground: Option<Polarity>,
}

/// A render in flight: the fraction done, the flag that stops it, and
/// where its result lands.
struct ExportJob {
    progress: Arc<Mutex<f32>>,
    cancel: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Result<(), String>>>>,
}

/// The engine, and everything needed to keep it agreeing with the stage.
///
/// Every field here is a MEMORY of what the engine was last told. That is
/// what makes the sync one comparison per frame instead of a command
/// stream: a schedule is rebuilt when the revision moves and at no other
/// time, because recompiling under a moving cursor would break the sound
/// for nothing.
struct Audio {
    /// `None` when no device would open. A machine with no audio is a
    /// legitimate state — the stage still runs, and says so — not a
    /// reason to refuse to start.
    engine: Option<Engine>,
    /// Why there is no engine, if there is none. Kept rather than
    /// swallowed: silence with no explanation is the failure this project
    /// spends the most effort avoiding.
    trouble: Option<String>,
    /// The stage revision the live schedule was built from.
    built: Option<u64>,
    /// Whether the graph built was the arrangement's. A mode change is
    /// a rebuild even when the song has not changed.
    built_song: bool,
    /// The mix revision whose values the live schedule is carrying.
    ///
    /// Separate from `built` because these two changes are answered in
    /// completely different ways: a new graph, or a letter to a node
    /// already running.
    mixed: Option<u64>,
    /// Where each track's meter and output stage ended up in the graph
    /// the engine is currently running.
    nodes: Option<SongNodes>,
    /// What the engine was last told about time.
    rolling: bool,
    bpm: f64,
    /// The stage's seek count the engine was last made to agree with.
    seeks: u64,
    /// The loop the engine was last given, in samples.
    looped: Option<(u64, u64)>,
    /// A render on its own thread, and the way to watch and stop it.
    export: Option<ExportJob>,
    /// The block the engine had published when it was last told to
    /// seek. Its position is not read back until a LATER block has
    /// arrived: the snapshot in hand was written before the seek, and
    /// handing the stage that position would throw its clock back to
    /// where it just left, for one visible frame.
    seek_block: Option<u64>,
    /// Scratch for the levels handed back to the stage, kept between
    /// frames so a meter costs no allocation per frame.
    levels: Vec<Level>,
    /// The same for the console's telemetry.
    telemetry: Vec<(daw::sequencing::DeviceId, daw::console::Telemetry)>,
}

impl Audio {
    fn start() -> Self {
        let (engine, trouble) = match Engine::start(EngineConfig::default()) {
            Ok(engine) => (Some(engine), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            engine,
            trouble,
            built: None,
            built_song: false,
            mixed: None,
            nodes: None,
            rolling: false,
            bpm: 0.0,
            seeks: 0,
            seek_block: None,
            levels: Vec::new(),
            telemetry: Vec::new(),
            looped: None,
            export: None,
        }
    }

    /// The stage asked for a render: build the arrangement's graph and
    /// bounce it on a thread; report progress back every frame until it
    /// ends. Runs with or without an engine — a render is offline.
    fn serve_export(&mut self, stage: &mut Stage) {
        if let Some(request) = stage.take_export() {
            stage.export_taken();
            let (spec, _) = song_graph::build_song(stage.song());
            let sample_rate = self
                .engine
                .as_ref()
                .map_or(48_000, |engine| engine.info().sample_rate);
            let bpm = stage.song().bpm_at(request.start_tick, stage.bpm());
            let ticks = f64::from(daw::sequencing::TICKS_PER_BEAT as u32);
            let opts = BounceOptions {
                sample_rate,
                block_frames: 256,
                bpm,
                length_beats: (request.end_tick - request.start_tick) as f64 / ticks,
                start_beats: request.start_tick as f64 / ticks,
                format: BounceFormat::Int24,
            };
            if let Some(dir) = request.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let job = ExportJob {
                progress: Arc::new(Mutex::new(0.0)),
                cancel: Arc::new(AtomicBool::new(false)),
                done: Arc::new(Mutex::new(None)),
            };
            let (progress, cancel, done) =
                (job.progress.clone(), job.cancel.clone(), job.done.clone());
            let path = request.path.clone();
            std::thread::spawn(move || {
                let result = bounce_automated(
                    &spec,
                    &opts,
                    &path,
                    |_, _| {},
                    |fraction| {
                        if let Ok(mut slot) = progress.lock() {
                            *slot = fraction;
                        }
                        !cancel.load(Ordering::Relaxed)
                    },
                )
                .map_err(|error| error.to_string());
                if let Ok(mut slot) = done.lock() {
                    *slot = Some(result);
                }
            });
            self.export = Some(job);
        }
        if let Some(job) = &self.export {
            if stage.export_abandoned() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            let finished = job.done.lock().ok().and_then(|mut slot| slot.take());
            match finished {
                Some(result) => {
                    stage.export_finished(result);
                    self.export = None;
                }
                None => {
                    let fraction = job.progress.lock().map_or(0.0, |slot| *slot);
                    stage.set_export_progress(fraction);
                }
            }
        }
    }

    /// What the stage should say about the engine: whether there is one,
    /// whether it is alive, and what it has dropped. A machine with no
    /// audio reports itself rather than staying silent about it, and so
    /// does an engine that was there and went away.
    fn health(&mut self) -> Health {
        let Some(engine) = &mut self.engine else {
            return Health {
                state: EngineState::Absent,
                xruns: 0,
                load: 0.0,
            };
        };
        let state = match engine.health() {
            StreamHealth::Running => EngineState::Running,
            StreamHealth::Stalled { seconds } => EngineState::Stalled { seconds },
            StreamHealth::Errored(error) => EngineState::Errored(error),
        };
        let snapshot = engine.latest_block();
        let info = engine.info();
        Health {
            state,
            // Under- and over-runs together, and the blocks refused for
            // being oversized: each was probably heard.
            xruns: snapshot.underflows + snapshot.overflows + snapshot.oversized_blocks,
            load: snapshot.load(info.sample_rate, info.max_frames as u32) as f32,
        }
    }

    /// What the sample editor asks of the host: the file it is looking
    /// at, as the stage may hold it, and the ranges it wants to hear.
    ///
    /// The file comes through the material cache at the device's rate —
    /// the same bytes the sampler node will play — and the stage gets
    /// the material's own `Arc`, so nothing is copied. An audition is a
    /// cut of those samples through the engine's audition voice. Loading
    /// happens on this thread, once per file: a first look at a long
    /// file is a pause, and a second look is free.
    fn serve_cutting_room(&mut self, stage: &mut Stage) {
        let rate = self
            .engine
            .as_ref()
            .map_or(48_000, |engine| engine.info().sample_rate);
        if let Some(path) = stage.wanted_sample().map(std::path::Path::to_path_buf) {
            match material::load_cached(&path, rate) {
                Ok(loaded) => stage.set_sample(SampleData::from_planar(
                    path,
                    loaded.samples.clone(),
                    loaded.channels,
                    loaded.frames,
                    loaded.sample_rate,
                )),
                Err(error) => {
                    eprintln!("stage: could not read {}: {error}", path.display());
                    // An empty file, so the editor stops asking and says
                    // what it has: nothing.
                    stage.set_sample(SampleData::from_planar(
                        path,
                        std::sync::Arc::new(Vec::new()),
                        1,
                        0,
                        rate,
                    ));
                }
            }
        }
        if stage.take_audition_stop()
            && let Some(engine) = &mut self.engine
        {
            engine.stop_audition();
        }
        if let Some(asked) = stage.take_audition()
            && let Some(engine) = &mut self.engine
            && let Ok(whole) = material::load_cached(&asked.path, rate)
        {
            let frames = whole.frames as usize;
            let from = ((asked.from * frames as f64).round() as usize).min(frames);
            let to = ((asked.to * frames as f64).round() as usize).clamp(from, frames);
            if to > from {
                let mut cut = Vec::with_capacity((to - from) * whole.channels);
                for channel in 0..whole.channels {
                    let base = channel * frames;
                    cut.extend_from_slice(&whole.samples[base + from..base + to]);
                }
                let piece = material::Material {
                    samples: std::sync::Arc::new(cut),
                    channels: whole.channels,
                    frames: (to - from) as u64,
                    source: whole.source.clone(),
                    sample_rate: whole.sample_rate,
                    original_rate: whole.original_rate,
                    truncated: whole.truncated,
                };
                engine.audition(AuditionBuffer::from_material(piece));
            }
        }
    }

    /// The stream's standing facts, for the strip's screen.
    fn stream(&self) -> Option<Stream> {
        let engine = self.engine.as_ref()?;
        let info = engine.info();
        Some(Stream {
            sample_rate: info.sample_rate,
            buffer_frames: info.max_frames as u32,
            latency_frames: info.latency_frames.map(|frames| frames as u32),
            inputs: info.in_channels.min(255) as u8,
            outputs: info.out_channels.min(255) as u8,
            backend: EngineConfig::default().api.label(),
        })
    }

    /// Make the engine agree with the stage, then hand back what it heard.
    fn follow(&mut self, stage: &mut Stage) {
        self.serve_export(stage);
        // Told every frame rather than once: a stage that opened without
        // an engine and one whose engine went away are the same state,
        // and the surface should say so either way — and a block dropped
        // this frame is only news this frame.
        let health = self.health();
        stage.set_health(health);
        stage.set_stream(self.stream());
        self.serve_cutting_room(stage);
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.collect_trash();

        // What sounds, when it changed. A rebuild mints fresh node ids,
        // so the mapping is captured with the schedule rather than
        // derived from the song afterwards.
        if self.built != Some(stage.revision()) || self.built_song != stage.song_mode() {
            let info = engine.info();
            let (spec, nodes) = if stage.song_mode() {
                song_graph::build_song(stage.song())
            } else {
                song_graph::build(stage.song(), stage.playing())
            };
            self.built_song = stage.song_mode();
            match spec.compile(info.sample_rate, info.max_frames) {
                Ok(schedule) => match engine.set_schedule(Box::new(schedule)) {
                    Ok(()) => {
                        self.nodes = Some(nodes);
                        self.built = Some(stage.revision());
                        self.trouble = None;
                    }
                    Err(error) => self.trouble = Some(error.to_string()),
                },
                Err(error) => self.trouble = Some(format!("graph refused: {error}")),
            }
        }

        // A fader or a pan moved. These ride LETTERS to the output stage
        // the graph already has, because a level is not a reason to stop
        // the sound and start it again — which is exactly what handing
        // the engine a new schedule would do.
        //
        // A rebuild has already put the current values in the graph (the
        // compiler reads them from the song), so this only ever has to
        // catch up the changes a rebuild did not.
        if self.mixed != Some(stage.mix_revision()) {
            if let Some(nodes) = &self.nodes {
                for (index, output) in nodes.outputs.iter().enumerate() {
                    let (Some(node), Some(track)) = (output, stage.song().tracks.get(index)) else {
                        continue;
                    };
                    engine.set_param(*node, params::pan::GAIN, track.volume);
                    engine.set_param(*node, params::pan::PAN, track.pan);
                }
                // Every knob on every device that reached the graph: a
                // turn is a letter, not a rebuild. Sent whole rather than
                // as a diff, because a diff needs a memory of what the
                // engine was last told and the song itself is that.
                for (id, node) in &nodes.devices {
                    if let Some(device) = stage.song().device(*id) {
                        for (param, value) in &device.overrides {
                            engine.set_param(*node, *param, *value);
                        }
                    }
                }
            }
            self.mixed = Some(stage.mix_revision());
        }

        // Tempo before motion: a transport told to roll should already
        // know how fast.
        let bpm = stage.bpm();
        if bpm != self.bpm {
            engine.transport(TransportCmd::SetTempo(bpm));
            self.bpm = bpm;
        }

        // Where the stage moved its own clock — a return to the top —
        // the engine is sent there too, and its position is not read
        // back until a block written after the seek has arrived.
        let snapshot = engine.latest_block();
        if stage.seeks() != self.seeks {
            let samples = samples_at(engine, stage.tick(), bpm);
            engine.transport(TransportCmd::Seek(samples));
            self.seeks = stage.seeks();
            self.seek_block = Some(snapshot.block);
        }

        // The brace: the engine loops the song's timeline while the
        // song plays with the brace on, and runs free otherwise.
        let looped = stage
            .loop_region()
            .map(|(start, end)| (samples_at(engine, start, bpm), samples_at(engine, end, bpm)));
        if looped != self.looped {
            engine.transport(match looped {
                Some((start, end)) => TransportCmd::SetLoop { start, end },
                None => TransportCmd::ClearLoop,
            });
            self.looped = looped;
        }

        let rolling = stage.rolling();
        if rolling != self.rolling {
            engine.transport(if rolling {
                TransportCmd::Play
            } else {
                TransportCmd::Stop
            });
            self.rolling = rolling;
        }

        // The playhead is what SOUNDED rather than what was counted: the
        // engine's own position goes back to the stage every frame it is
        // rolling, and the stage's frame clock stands in only while a
        // seek is still in flight.
        let settled = self
            .seek_block
            .is_none_or(|seek_block| snapshot.block > seek_block.saturating_add(1));
        if settled {
            self.seek_block = None;
            if snapshot.playing {
                stage.set_position(snapshot.beat);
            }
        }

        // And what came back. A track with no voice in the graph has no
        // meter slot, and reads as silence — which is what it is.
        let tracks = stage.song().tracks.len();
        self.levels.clear();
        self.levels.resize(tracks, Level::default());
        if let Some(nodes) = &self.nodes {
            for (track, slot) in nodes.meters.iter().enumerate() {
                let (Some(slot), Some(level)) = (slot, self.levels.get_mut(track)) else {
                    continue;
                };
                *level = Level {
                    left: snapshot.track_peaks_l[*slot],
                    right: snapshot.track_peaks_r[*slot],
                };
            }
        }
        let master = Level {
            left: snapshot.track_peaks_l[MASTER_METER],
            right: snapshot.track_peaks_r[MASTER_METER],
        };
        stage.set_levels(&self.levels, master);
        // The console's telemetry: every section's own figures, by the
        // device the card draws.
        self.telemetry.clear();
        if let Some(nodes) = &self.nodes {
            for (id, slot) in &nodes.telemetry {
                let Some(said) = snapshot.telemetry.get(*slot) else {
                    continue;
                };
                self.telemetry.push((
                    *id,
                    daw::console::Telemetry {
                        level_db: said.level_db,
                        reduction_db: said.reduction_db,
                        bands: said.bands,
                    },
                ));
            }
        }
        stage.set_telemetry(&self.telemetry);
    }
}

/// A song tick as a sample position, at the tempo the engine is running.
/// The same arithmetic the engine's own time map does, done here once so
/// a seek lands where the stage's readout says it is.
fn samples_at(engine: &Engine, tick: usize, bpm: f64) -> u64 {
    let beats = tick as f64 / daw::sequencing::TICKS_PER_BEAT as f64;
    let sample_rate = f64::from(engine.info().sample_rate);
    (beats * 60.0 / bpm.max(1.0) * sample_rate).round() as u64
}

impl App {
    fn new(_storage: &shell::Storage) -> Self {
        let audio = Audio::start();
        if let Some(trouble) = &audio.trouble {
            // stderr rather than the surface: the stage has one message
            // strip and it belongs to the musician, not to the console.
            eprintln!("stage: no audio engine — {trouble}");
        }
        let mut stage = Stage::with_library(library_config(), LibrarySnapshot::default());
        // Songs live under the music folder unless one is opened from
        // somewhere else. Said here rather than guessed by the stage, so
        // a headless stage never writes to disk by accident.
        if let Some(home) = std::env::var_os("HOME") {
            stage.set_home(std::path::PathBuf::from(home).join("Music").join("daw"));
        }
        // `cargo run --bin stage -- song.stage.ron` opens that song. A
        // file that will not open is said on stderr and the stage opens
        // empty, rather than refusing to start over a path.
        if let Some(path) = std::env::args_os().nth(1)
            && let Err(error) = stage.open(std::path::PathBuf::from(path))
        {
            eprintln!("stage: could not open the song — {error}");
        }
        Self {
            stage,
            audio,
            ground: None,
        }
    }
}

impl shell::Host for App {
    /// Fonts and the initial stock-widget theme, once the shell has made
    /// the egui context. The custom shell exists so the completed frame
    /// can pass through `shell::post` before presentation.
    fn startup(&mut self, ctx: &egui::Context) {
        install_stage_fonts(ctx);
        Theme::dark().apply(ctx);
    }

    /// eframe 0.36 hands the app a `Ui` rather than a `Context` and a
    /// panel to build, so there is nothing between the window and the
    /// stage. That is the whole surface.
    fn ui(&mut self, ui: &mut egui::Ui) {
        // The window's title carries the song's name and whether it is
        // safe, so a glance at the taskbar answers both.
        let title = match self.stage.path() {
            Some(path) => format!(
                "daw — stage — {}{}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                if self.stage.is_dirty() { " *" } else { "" }
            ),
            None => "daw — stage".to_owned(),
        };
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(title));
        // Follow the stage's ground. Applied only when it CHANGES: the
        // theme rebuilds egui's whole style, which is not a thing to do
        // sixty times a second for an answer that is usually the same.
        let polarity = self.stage.polarity();
        if self.ground != Some(polarity) {
            match polarity {
                Polarity::Dark => Theme::dark(),
                Polarity::Light => Theme::light(),
            }
            .apply(ui.ctx());
            self.ground = Some(polarity);
        }

        self.stage.show(ui);
        self.audio.follow(&mut self.stage);
        // A meter that only moves when the mouse does is not a meter.
        ui.ctx().request_repaint();
    }

    fn save(&mut self, _storage: &mut shell::Storage) {}
}
