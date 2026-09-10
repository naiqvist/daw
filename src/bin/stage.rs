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
use daw::audio::bounce::{BounceFormat, BounceOptions, bounce_automated_with_tempo_table};
use daw::audio::material;
use daw::audio::modulation::{ModEdit, WireEdit};
use daw::audio::transport::TransportCmd;
use daw::audio::{AudioApi, Engine, EngineConfig, StreamHealth};
use daw::design::Polarity;
use daw::install_stage_fonts;
use daw::library::LibraryConfig;
use daw::midi_input::{MidiEvent, MidiInput};
use daw::params;
use daw::record::{MidiRecorder, MidiTake, Recorder};
use daw::shell;
use daw::song_graph::{self, MASTER_METER, SongNodes};
use daw::tempo::TempoTable;
use daw::ui::prefs::{AudioBackend, STORAGE_KEY, UiPrefs};
use daw::ui::stage::{
    AudioDeviceChoice, AudioSettings, EngineState, Health, Level, SampleData, Stage, StageIntent,
    Stream, UtilityHostRequest,
};
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

fn audio_api(backend: AudioBackend) -> AudioApi {
    match backend {
        AudioBackend::Jack => AudioApi::Jack,
        AudioBackend::Alsa => AudioApi::Alsa,
        AudioBackend::Pulse => AudioApi::Pulse,
    }
}

fn engine_config(prefs: &UiPrefs) -> EngineConfig {
    let base = EngineConfig::default();
    EngineConfig {
        api: audio_api(prefs.audio_backend),
        output_device: prefs.audio_device.clone(),
        sample_rate: prefs.audio_rate_hz.unwrap_or(base.sample_rate),
        buffer_frames: prefs.audio_buffer_frames.unwrap_or(base.buffer_frames),
        channels: base.channels,
    }
}

fn requested_engine_config(settings: AudioSettings) -> EngineConfig {
    let base = EngineConfig::default();
    EngineConfig {
        api: audio_api(settings.backend),
        output_device: settings.device,
        sample_rate: settings.rate_hz.unwrap_or(base.sample_rate),
        buffer_frames: settings.buffer_frames.unwrap_or(base.buffer_frames),
        channels: base.channels,
    }
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
    // The stream request now carries the user's buffer choice, so the node
    // must not pin a second, contradictory quantum in its environment.
    const PROPS: &str = "{ node.always-process = true node.want-driver = true }";
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
    presentation: Option<(Polarity, daw::ui::tokens::Density, bool)>,
    last_autosave: std::time::Instant,
    close: CloseInterlock,
    close_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CloseInterlock {
    #[default]
    Idle,
    StoppingTake,
    Confirm,
    Exit,
}

/// A clean idle project needs no modal. A live take must be closed before
/// its newly dirty document can be judged; an already dirty project asks.
fn close_interlock_for(dirty: bool, recording_busy: bool) -> Option<CloseInterlock> {
    if recording_busy {
        Some(CloseInterlock::StoppingTake)
    } else if dirty {
        Some(CloseInterlock::Confirm)
    } else {
        None
    }
}

/// A render in flight: the fraction done, the flag that stops it, and
/// where its result lands.
struct ExportJob {
    progress: Arc<Mutex<f32>>,
    cancel: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Result<(), String>>>>,
}

struct ActiveRecording {
    start_sample: u64,
    start_block: u64,
    last_block: u64,
    last_sample: u64,
    sample_rate: u32,
    latency_frames: u64,
    overruns_at_start: u64,
    has_audio: bool,
    errors: Vec<String>,
}

struct PendingFinish {
    stop_block: u64,
    wait_for_audio: bool,
    deadline: std::time::Instant,
    audio_started_at: u64,
    sample_rate: u32,
    latency_frames: u64,
    overruns: u64,
    elapsed_frames: u64,
    midi_takes: Vec<MidiTake>,
    errors: Vec<String>,
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
    /// The one consumer of the engine's capture ring. It stays beside the
    /// engine for the life of that stream; taking it twice would split one
    /// recording into two incomplete readers.
    recorder: Option<Recorder>,
    /// Hardware MIDI is green-zone input. The first available port is kept
    /// open and retried when controllers are plugged in after launch.
    midi_input: MidiInput,
    midi_recorder: MidiRecorder,
    midi_refreshed: std::time::Instant,
    /// A capture in progress and, after Stop, the take waiting for one
    /// callback boundary before its file is finalized.
    recording: Option<ActiveRecording>,
    finishing: Option<PendingFinish>,
    /// Deepest compiled signal-path latency. Read before the schedule moves
    /// to the callback, then combined with the stream latency when a take
    /// begins so placement is compensated against what was heard.
    schedule_latency_frames: u64,
    /// The backend that owns `engine`. The negotiated stream does not repeat
    /// it, so retaining the exact request is what makes diagnostics honest.
    api: AudioApi,
    /// Why there is no engine, if there is none. Kept rather than
    /// swallowed: silence with no explanation is the failure this project
    /// spends the most effort avoiding.
    trouble: Option<String>,
    /// The stage revision the live schedule was built from.
    built: Option<u64>,
    /// The spec that schedule was compiled from, and the tempo it was
    /// compiled at. Kept so the next compile can say which nodes did not
    /// move and may therefore keep playing across the swap.
    ///
    /// The tempo is half of that answer rather than a detail: event stamps
    /// and loop lengths are baked into a node at compile, so two identical
    /// specs at two tempos are NOT the same node.
    built_spec: Option<daw::audio::graph::GraphSpec>,
    built_bpm: f64,
    /// The revision this loop has seen, when it last moved, and whether it
    /// is moving as part of a burst.
    ///
    /// A held arrow on a note, a trig length or a p-lock repeats at the
    /// OS repeat rate. Compiling on every repeat is a schedule swap every
    /// thirty milliseconds, and even a swap that carries its nodes across
    /// cannot carry the node being edited — so a held key was a song that
    /// stopped until the key came up. A burst is compiled once, when the
    /// hand stops.
    seen: Option<u64>,
    seen_at: std::time::Instant,
    burst: bool,
    /// Whether the graph built was the arrangement's. A mode change is
    /// a rebuild even when the song has not changed.
    built_song: bool,
    /// The arrangement schedule's immutable piecewise sample clock. Session
    /// mode is deliberately `None`: launched clips keep their scalar,
    /// clip-relative loop clock.
    tempo: Option<TempoTable>,
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
    /// The instruments' readouts, by device, refreshed each frame.
    readouts: Vec<(daw::sequencing::DeviceId, daw::console::Telemetry)>,
    /// When the engine was started, and whether it has already been
    /// started again on the fallback backend.
    started: std::time::Instant,
    fell_back: bool,
}

/// How long a freshly started backend gets to deliver its first block
/// before it is given up on.
const FIRST_BLOCK_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
const MIDI_REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
/// A gap longer than this means the hand STOPPED: the edit that follows is
/// a fresh decision and its schedule is compiled at once.
const EDIT_BURST: std::time::Duration = std::time::Duration::from_millis(150);
/// How still a burst of edits has to go before its schedule is compiled.
/// Short enough to feel immediate on release, long enough to swallow an OS
/// key repeat whole.
const EDIT_SETTLE: std::time::Duration = std::time::Duration::from_millis(90);

/// Whether a moved revision is compiled this frame.
///
/// A lone edit compiles at once — it has to be heard on the next trig. One
/// arriving mid-burst waits for the hand to stop, and waiting costs nothing
/// musically: everything that moves the revision changes what the sequencer
/// will PLAY, and the soonest any of it can sound is that same next trig
/// either way.
///
/// Pure, so the rule is checkable without a clock.
fn compile_now(burst: bool, since_change: std::time::Duration) -> bool {
    !burst || since_change >= EDIT_SETTLE
}
const CAPTURE_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

impl Audio {
    fn start(config: EngineConfig) -> Self {
        let api = config.api;
        let (mut engine, trouble) = match Engine::start(config) {
            Ok(engine) => (Some(engine), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let recorder = engine.as_mut().and_then(|engine| {
            let info = engine.info();
            engine
                .take_capture()
                .map(|input| Recorder::new(input, info.in_channels, info.sample_rate))
        });
        let mut midi_input = MidiInput::default();
        midi_input.refresh();
        if !midi_input.ports().is_empty()
            && let Err(error) = midi_input.connect(0)
        {
            eprintln!("stage: {error}");
        }
        Self {
            started: std::time::Instant::now(),
            fell_back: false,
            engine,
            recorder,
            midi_input,
            midi_recorder: MidiRecorder::default(),
            midi_refreshed: std::time::Instant::now(),
            recording: None,
            finishing: None,
            schedule_latency_frames: 0,
            api,
            trouble,
            built: None,
            built_spec: None,
            built_bpm: 0.0,
            seen: None,
            seen_at: std::time::Instant::now(),
            burst: false,
            built_song: false,
            tempo: None,
            mixed: None,
            nodes: None,
            rolling: false,
            bpm: 0.0,
            seeks: 0,
            seek_block: None,
            levels: Vec::new(),
            telemetry: Vec::new(),
            readouts: Vec::new(),
            looped: None,
            export: None,
        }
    }

    /// Replace only the live stream. An offline export belongs to the host,
    /// not that stream, and therefore survives an audio-device restart.
    fn restart(&mut self, config: EngineConfig) -> Result<Stream, String> {
        if self.recording_busy() {
            return Err("stop recording before restarting audio".to_owned());
        }
        let api = config.api;
        self.engine = None;
        self.recorder = None;
        match Engine::start(config) {
            Ok(mut engine) => {
                let info = engine.info();
                self.recorder = engine
                    .take_capture()
                    .map(|input| Recorder::new(input, info.in_channels, info.sample_rate));
                self.engine = Some(engine);
                self.api = api;
                self.trouble = None;
                self.started = std::time::Instant::now();
                self.fell_back = false;
                self.built = None;
                // A new stream is a new sample rate and a new block size,
                // which is everything a node is built from: nothing
                // compiled for the old one may be carried into it.
                self.built_spec = None;
                self.built_song = false;
                self.tempo = None;
                self.mixed = None;
                self.nodes = None;
                self.rolling = false;
                self.bpm = 0.0;
                self.seeks = 0;
                self.looped = None;
                self.seek_block = None;
                self.schedule_latency_frames = 0;
                self.stream()
                    .ok_or_else(|| "audio stream opened without stream information".to_owned())
            }
            Err(error) => {
                let error = error.to_string();
                self.api = api;
                self.trouble = Some(error.clone());
                Err(error)
            }
        }
    }

    /// The stage asked for a render: build the arrangement's graph and
    /// bounce it on a thread; report progress back every frame until it
    /// ends. Runs with or without an engine — a render is offline.
    fn serve_export(&mut self, stage: &mut Stage) {
        if let Some(request) = stage.take_export() {
            stage.export_taken();
            // The render thread owns one immutable document snapshot and the
            // node addresses minted with its graph. Live playback calls the
            // same evaluator below; only the clock feeding it differs.
            let song = request
                .source
                .as_deref()
                .cloned()
                .unwrap_or_else(|| stage.song().clone());
            let (spec, _) = song_graph::build_song(&song);
            let sample_rate = request.rate_hz.unwrap_or_else(|| {
                self.engine
                    .as_ref()
                    .map_or(48_000, |engine| engine.info().sample_rate)
            });
            let ticks = f64::from(daw::sequencing::TICKS_PER_BEAT as u32);
            let bpm = song.base_bpm();
            let timeline = TempoTable::build(&song, f64::from(sample_rate), bpm);
            // BounceOptions::length_beats is the absolute render END, not
            // the post-trim duration: the renderer runs the pre-roll from
            // zero so effects and automation arrive at a later range with
            // their real history, then discards `start_beats`.
            // Tail is real time, so add it in samples and ask the map for the
            // corresponding musical end instead of multiplying by whichever
            // one tempo happened to be active at the selection edge.
            let tail_samples =
                (f64::from(request.tail_seconds) * f64::from(sample_rate)).round() as u64;
            let end_sample = timeline
                .sample_at(request.end_tick)
                .saturating_add(tail_samples);
            let opts = BounceOptions {
                sample_rate,
                block_frames: 256,
                bpm,
                length_beats: timeline.beat_at_sample(end_sample),
                start_beats: request.start_tick as f64 / ticks,
                format: match request.format {
                    daw::ui::prefs::ExportFormat::Float32 => BounceFormat::Float32,
                    daw::ui::prefs::ExportFormat::Int24 => BounceFormat::Int24,
                    daw::ui::prefs::ExportFormat::Int16 => BounceFormat::Int16,
                },
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
                let result = bounce_automated_with_tempo_table(
                    &spec,
                    &opts,
                    &path,
                    &timeline,
                    |_, _| {},
                    |fraction| {
                        if let Ok(mut slot) = progress.lock() {
                            *slot = fraction;
                        }
                        !cancel.load(Ordering::Relaxed)
                    },
                )
                .map_err(|error| error.to_string());
                if result.is_err() {
                    let _ = std::fs::remove_file(&path);
                }
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
        // The band's small pictures — a kit's pads — one file a frame.
        if let Some(path) = stage.wanted_thumb().map(std::path::Path::to_path_buf) {
            let data = match material::load_cached(&path, rate) {
                Ok(loaded) => SampleData::from_planar(
                    path,
                    loaded.samples.clone(),
                    loaded.channels,
                    loaded.frames,
                    loaded.sample_rate,
                ),
                Err(_) => {
                    SampleData::from_planar(path, std::sync::Arc::new(Vec::new()), 1, 0, rate)
                }
            };
            stage.set_thumb(data);
        }
        if stage.take_midi_stop()
            && let Some(engine) = &mut self.engine
        {
            engine.stop_audition();
        }
        if let Some(render) = stage.take_midi_audio()
            && let Some(engine) = &mut self.engine
        {
            engine.audition(AuditionBuffer::from_material(material::Material {
                samples: render.samples,
                channels: 2,
                frames: render.frames as u64,
                sample_rate: render.rate,
                original_rate: render.rate,
                truncated: false,
                source: std::path::PathBuf::from("midi-lab:preview"),
            }));
        }
        if let Some(render) = stage.take_kiln_audio() {
            if let Some(engine) = &mut self.engine {
                let frames = render.samples.len() as u64;
                let piece = material::Material {
                    samples: render.samples,
                    channels: 1,
                    frames,
                    source: std::path::PathBuf::from("kiln:membrane"),
                    sample_rate: render.rate,
                    original_rate: render.rate,
                    truncated: false,
                };
                engine.audition(AuditionBuffer::from_material(piece));
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
            backend: self.api.label(),
        })
    }

    fn recording_busy(&self) -> bool {
        self.recording.is_some() || self.finishing.is_some()
    }

    /// Controllers can appear after launch. Re-scan only while disconnected,
    /// then open the first port deterministically; selection can grow into a
    /// utility surface without changing the capture contract here.
    fn refresh_midi(&mut self) {
        if self.midi_input.connected().is_some()
            || self.midi_refreshed.elapsed() < MIDI_REFRESH_EVERY
        {
            return;
        }
        self.midi_refreshed = std::time::Instant::now();
        self.midi_input.refresh();
        if !self.midi_input.ports().is_empty()
            && let Err(error) = self.midi_input.connect(0)
        {
            eprintln!("stage: {error}");
        }
    }

    fn begin_recording(&mut self, stage: &mut Stage) {
        if self.finishing.is_some() {
            stage.recording_failed("the previous take is still closing");
            return;
        }
        let routes = stage.record_routes();
        let requested_midi = stage.midi_record_tracks();
        let Some(engine) = self.engine.as_mut() else {
            stage.recording_failed("no audio clock");
            return;
        };
        let info = engine.info();
        let snapshot = engine.latest_block();
        let start_sample = samples_at(engine, stage.tick(), stage.bpm(), self.tempo.as_ref());

        let midi_tracks = if self.midi_input.connected().is_some() {
            requested_midi.clone()
        } else {
            Vec::new()
        };
        if routes.is_empty() && midi_tracks.is_empty() {
            stage.recording_failed(if requested_midi.is_empty() {
                "nothing is armed"
            } else {
                "no MIDI input is connected"
            });
            return;
        }

        if !routes.is_empty() {
            let Some(recorder) = self.recorder.as_mut() else {
                stage.recording_failed("capture ring is unavailable");
                return;
            };
            if let Err(error) = recorder.begin(&routes, &stage.recording_directory()) {
                stage.recording_failed(error.to_string());
                return;
            }
        }

        // Messages waiting before the record edge are not part of this take.
        let _ = self.midi_input.drain();
        self.midi_recorder.begin(&midi_tracks);
        if !routes.is_empty() {
            engine.set_capturing(true);
        }
        let mut errors = Vec::new();
        if !requested_midi.is_empty() && midi_tracks.is_empty() {
            errors.push("MIDI: no input connected".to_owned());
        }
        let start_sample = if snapshot.block == 0 {
            start_sample
        } else {
            snapshot.position.max(start_sample)
        };
        self.recording = Some(ActiveRecording {
            start_sample,
            start_block: snapshot.block,
            last_block: snapshot.block,
            last_sample: start_sample,
            sample_rate: info.sample_rate,
            latency_frames: info.latency_frames.unwrap_or(0) as u64 + self.schedule_latency_frames,
            overruns_at_start: engine.capture_overruns(),
            has_audio: !routes.is_empty(),
            errors,
        });
        stage.recording_started(routes.len(), midi_tracks.len());
    }

    /// Drain controller messages at frame rate and pin their relative timing
    /// to the newest engine block. The midir stamp preserves order within a
    /// batch; the engine sample clock is the canonical timeline written into
    /// the take.
    fn pump_midi(&mut self, stage: &mut Stage) {
        self.refresh_midi();
        let events = self.midi_input.drain();
        stage.midi_lab_input(&events);
        let looped = self.looped;
        let Some(active) = self.recording.as_mut() else {
            return;
        };
        if !self.midi_recorder.recording() {
            return;
        }
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let snapshot = engine.latest_block();
        // `BlockSnapshot::position` is already the transport position after
        // the published block — the exact start of the next live block.
        // Adding `frames` again stamps every controller message one complete
        // hardware quantum late. Until one newer block arrives, retain the
        // record edge so an old snapshot cannot place a pre-roll message.
        let fresh = snapshot.block > active.start_block;
        let now = if fresh {
            snapshot.position
        } else {
            active.start_sample
        };
        let wrapped = snapshot.block > active.last_block && now < active.last_sample;
        if snapshot.block > active.last_block {
            if wrapped {
                let (close_sample, resume_sample) = looped
                    .filter(|(start, end)| *start < *end)
                    .map_or((active.last_sample, now), |(start, end)| (end, start));
                self.midi_recorder
                    .discontinuity(close_sample, resume_sample);
            }
            active.last_block = snapshot.block;
            active.last_sample = now;
        }
        if events.is_empty() {
            return;
        }
        let newest_stamp = events.last().map_or(0, |(stamp, _)| *stamp);
        for (stamp, event) in events {
            let micros = newest_stamp.saturating_sub(stamp);
            let behind = ((u128::from(micros) * u128::from(active.sample_rate)) / 1_000_000)
                .min(u128::from(u64::MAX)) as u64;
            let sample = if fresh {
                midi_sample_behind(now, behind, looped, wrapped)
            } else {
                active.start_sample
            };
            match event {
                MidiEvent::NoteOn { note, velocity } => {
                    self.midi_recorder.note_on(sample, note, velocity)
                }
                MidiEvent::NoteOff { note } => self.midi_recorder.note_off(sample, note),
            }
        }
    }

    /// Cross the Stop edge. Audio capture is disabled immediately, but its
    /// WAV stays open until a later callback block proves no producer can
    /// still be writing the tail.
    fn request_recording_finish(&mut self) {
        let Some(active) = self.recording.take() else {
            return;
        };
        let (stop_block, stop_sample, audio_started_at, overruns) = self
            .engine
            .as_mut()
            .map(|engine| {
                let snapshot = engine.latest_block();
                if active.has_audio {
                    engine.set_capturing(false);
                }
                (
                    snapshot.block,
                    // `pump_midi` already followed any loop/seek edge on this
                    // frame, so this is the exact side of the discontinuity
                    // on which still-held controller notes must close.
                    active.last_sample,
                    if active.has_audio {
                        engine.capture_start()
                    } else {
                        active.start_sample
                    },
                    engine
                        .capture_overruns()
                        .saturating_sub(active.overruns_at_start),
                )
            })
            .unwrap_or((0, active.start_sample, active.start_sample, 0));
        let stop_sample = stop_sample.max(active.start_sample);
        let wait_for_audio =
            active.has_audio && self.recorder.as_ref().is_some_and(Recorder::recording);
        self.finishing = Some(PendingFinish {
            stop_block,
            wait_for_audio,
            deadline: std::time::Instant::now() + CAPTURE_DRAIN_TIMEOUT,
            audio_started_at,
            sample_rate: active.sample_rate,
            latency_frames: active.latency_frames,
            overruns,
            elapsed_frames: stop_sample.saturating_sub(active.start_sample),
            midi_takes: self.midi_recorder.finish(stop_sample),
            errors: active.errors,
        });
    }

    fn finish_recording_if_ready(&mut self, stage: &mut Stage, force: bool) {
        let Some(pending) = self.finishing.as_ref() else {
            return;
        };
        if pending.wait_for_audio
            && !force
            && std::time::Instant::now() < pending.deadline
            && self
                .engine
                .as_mut()
                .is_some_and(|engine| engine.latest_block().block <= pending.stop_block)
        {
            return;
        }
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.poll();
        }
        let pending = self.finishing.take().expect("finish checked above");
        let mut frames = 0;
        let (audio_takes, record_errors) = if pending.wait_for_audio {
            if let Some(recorder) = self.recorder.as_mut() {
                frames = recorder.frames();
                recorder.finish()
            } else {
                (Vec::new(), Vec::new())
            }
        } else {
            (Vec::new(), Vec::new())
        };
        let mut errors = pending.errors;
        errors.extend(record_errors.into_iter().map(|error| error.to_string()));
        stage.commit_recording(
            audio_takes,
            pending.midi_takes,
            pending.audio_started_at,
            pending.sample_rate,
            pending.latency_frames,
            pending.overruns,
            frames.max(pending.elapsed_frames),
            errors,
        );
    }

    fn drive_recording(&mut self, stage: &mut Stage) {
        self.pump_midi(stage);
        match (stage.recording(), self.recording.is_some()) {
            (true, false) => self.begin_recording(stage),
            (false, true) => self.request_recording_finish(),
            _ => {}
        }
        if self.recording.is_some()
            && let Some(recorder) = self.recorder.as_mut()
        {
            recorder.poll();
        }
        self.finish_recording_if_ready(stage, false);
    }

    /// Make the engine agree with the stage, then hand back what it heard.
    /// A backend that opened but never delivers is not an engine. JACK
    /// through PipeWire's shim has been seen to do exactly that — the
    /// node scheduled, the callback never called — while ALSA through
    /// the same server delivers at once. So the first backend gets a
    /// grace period for its first block, and then the engine is started
    /// again on ALSA, once. Everything the host remembered about the
    /// old engine is forgotten with it, so the schedule is sent afresh.
    fn fall_back_if_dead(&mut self, stage: &mut Stage) {
        if self.fell_back {
            return;
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        if engine.latest_block().block > 0 || self.started.elapsed() < FIRST_BLOCK_GRACE {
            return;
        }
        if self.recording.is_some() {
            self.pump_midi(stage);
            stage.recording_failed("audio engine stopped; closing take");
            self.request_recording_finish();
        }
        self.finish_recording_if_ready(stage, true);
        self.fell_back = true;
        eprintln!(
            "stage: the audio backend delivered no blocks in {:.1} s — starting again on ALSA",
            FIRST_BLOCK_GRACE.as_secs_f32()
        );
        // The dead backend is LEAKED rather than closed: closing a JACK
        // client the shim never ran has been seen to block forever, and
        // a hang there would take the whole surface with it. One dead
        // client's memory, once per run, is the price of a surface that
        // keeps answering.
        if let Some(dead) = self.engine.take() {
            std::mem::forget(dead);
        }
        let config = EngineConfig {
            api: daw::audio::AudioApi::Alsa,
            ..EngineConfig::default()
        };
        self.recorder = None;
        match Engine::start(config) {
            Ok(mut engine) => {
                eprintln!("stage: the ALSA engine is up: {:?}", engine.info());
                let info = engine.info();
                self.recorder = engine
                    .take_capture()
                    .map(|input| Recorder::new(input, info.in_channels, info.sample_rate));
                self.engine = Some(engine);
                self.api = AudioApi::Alsa;
                self.trouble = None;
            }
            Err(error) => {
                eprintln!("stage: ALSA engine refused: {error}");
                self.trouble = Some(error.to_string());
            }
        }
        self.started = std::time::Instant::now();
        self.built = None;
        self.built_spec = None;
        self.built_song = false;
        self.tempo = None;
        self.mixed = None;
        self.nodes = None;
        self.rolling = false;
        self.seeks = 0;
        self.seek_block = None;
        self.looped = None;
        self.schedule_latency_frames = 0;
    }

    fn follow(&mut self, stage: &mut Stage) {
        self.serve_export(stage);
        self.fall_back_if_dead(stage);
        // Told every frame rather than once: a stage that opened without
        // an engine and one whose engine went away are the same state,
        // and the surface should say so either way — and a block dropped
        // this frame is only news this frame.
        let health = self.health();
        stage.set_health(health);
        stage.set_stream(self.stream());
        self.serve_cutting_room(stage);
        let Some(engine) = &mut self.engine else {
            stage.clear_modulation_readings();
            self.drive_recording(stage);
            return;
        };
        engine.collect_trash();

        // What sounds, when it changed. A rebuild mints fresh node ids,
        // so the mapping is captured with the schedule rather than
        // derived from the song afterwards.
        let revision = stage.revision();
        let now = std::time::Instant::now();
        if self.seen != Some(revision) {
            // How long the song sat still BEFORE this edit is what says
            // whether a hand is mid-gesture: a key repeat arrives every
            // few frames, a decision does not.
            self.burst = self.seen.is_some() && now.duration_since(self.seen_at) < EDIT_BURST;
            self.seen = Some(revision);
            self.seen_at = now;
        }
        let settled = compile_now(self.burst, now.duration_since(self.seen_at));
        if (self.built != Some(revision) && settled) || self.built_song != stage.song_mode() {
            let info = engine.info();
            let song_mode = stage.song_mode();
            let mode_changed = self.built_song != song_mode;
            let base_bpm = stage.song().base_bpm();
            let timeline = song_mode
                .then(|| TempoTable::build(stage.song(), f64::from(info.sample_rate), base_bpm));
            let timeline_changed = self.tempo.as_ref() != timeline.as_ref();
            let (spec, nodes) = if song_mode {
                song_graph::build_song(stage.song())
            } else {
                song_graph::build(stage.song(), stage.playing())
            };
            let bpm = if song_mode { base_bpm } else { stage.bpm() };
            let compiled = match timeline.as_ref() {
                Some(timeline) => spec.compile_with_tempo_table(
                    info.sample_rate,
                    info.max_frames,
                    base_bpm,
                    timeline,
                ),
                None => spec.compile_at_tempo(info.sample_rate, info.max_frames, bpm),
            };
            // Which nodes may keep playing across the swap. Offered only
            // when everything ELSE a node is compiled from has held
            // still — the mode, the tempo map and the tempo — because a
            // spec that reads the same at a different tempo compiles to a
            // different node.
            let plan = self
                .built_spec
                .as_ref()
                .filter(|_| !mode_changed && !timeline_changed && self.built_bpm == bpm)
                .map(|prev| spec.adoption_plan(prev));
            match compiled {
                Ok(mut schedule) => {
                    if let Some(plan) = plan {
                        schedule.set_adoption(plan, engine.live_epoch());
                    }
                    self.schedule_latency_frames = schedule.latency() as u64;
                    match engine.set_schedule(Box::new(schedule)) {
                        Ok(()) => {
                            self.nodes = Some(nodes);
                            self.built = Some(revision);
                            // What the NEXT compile compares against. Kept
                            // only on a schedule that actually reached the
                            // callback: a plan drawn against a spec that
                            // never became a running graph would name nodes
                            // that are not there.
                            self.built_spec = Some(spec);
                            self.built_bpm = bpm;
                            self.built_song = song_mode;
                            self.tempo = timeline;
                            // A new map changes the meaning of the transport's
                            // absolute sample. Relocate once at the schedule
                            // seam so the musician's tick stays put; ordinary
                            // graph rebuilds under an unchanged map do not seek.
                            if mode_changed || timeline_changed {
                                let snapshot = engine.latest_block();
                                let samples = samples_at(
                                    engine,
                                    stage.tick(),
                                    stage.bpm(),
                                    self.tempo.as_ref(),
                                );
                                if engine.transport(TransportCmd::Seek(samples)) {
                                    self.seeks = stage.seeks();
                                    self.seek_block = Some(snapshot.block);
                                }
                            }
                            self.trouble = None;
                        }
                        Err(error) => self.trouble = Some(error.to_string()),
                    }
                }
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
            let mut delivered = true;
            if let Some(nodes) = &self.nodes {
                for (index, output) in nodes.outputs.iter().enumerate() {
                    let (Some(node), Some(track)) = (output, stage.song().tracks.get(index)) else {
                        continue;
                    };
                    delivered &= engine.set_param(*node, params::pan::GAIN, track.volume);
                    delivered &= engine.set_param(*node, params::pan::PAN, track.pan);
                }
                delivered &=
                    engine.set_param(nodes.master, params::mixer::GAIN, stage.song().master);
                // Every knob on every device that reached the graph: a
                // turn is a letter, not a rebuild. Sent whole rather than
                // as a diff, because a diff needs a memory of what the
                // engine was last told and the song itself is that.
                for (id, node) in nodes.device_letter_nodes() {
                    if let Some(device) = stage.song().device(id) {
                        for (param, value) in &device.overrides {
                            delivered &= engine.set_param(node, *param, *value);
                        }
                    }
                }
                // Source and response changes share the stage's live-value
                // revision with faders and device knobs. Send each complete
                // value as one idempotent letter: if the modulation ring is
                // full, `mixed` stays dirty and the authoritative Song state
                // is retried in full next frame.
                for source in &stage.song().modulators {
                    delivered &= engine.set_modulation(ModEdit::Source {
                        id: source.id,
                        kind: source.kind,
                    });
                }
                for wire in &stage.song().mod_wires {
                    delivered &= engine.set_modulation(ModEdit::Wire(WireEdit {
                        id: wire.id,
                        chain: wire.chain(),
                        enabled: wire.enabled,
                        solo: wire.solo,
                    }));
                }
            }
            if delivered {
                self.mixed = Some(stage.mix_revision());
            }
        }

        // Tempo before motion: a transport told to roll should already
        // know how fast.
        let bpm = stage.bpm();
        if bpm != self.bpm {
            if engine.transport(TransportCmd::SetTempo(bpm)) {
                self.bpm = bpm;
            }
        }

        // Where the stage moved its own clock — a return to the top —
        // the engine is sent there too, and its position is not read
        // back until a block written after the seek has arrived.
        let snapshot = engine.latest_block();
        let seeked = stage.seeks() != self.seeks;
        if seeked {
            let samples = samples_at(engine, stage.tick(), bpm, self.tempo.as_ref());
            if engine.transport(TransportCmd::Seek(samples)) {
                self.seeks = stage.seeks();
                self.seek_block = Some(snapshot.block);
            }
        }

        // The brace: the engine loops the song's timeline while the
        // song plays with the brace on, and runs free otherwise.
        let looped = stage.loop_region().map(|(start, end)| {
            (
                samples_at(engine, start, bpm, self.tempo.as_ref()),
                samples_at(engine, end, bpm, self.tempo.as_ref()),
            )
        });
        if looped != self.looped {
            if engine.transport(match looped {
                Some((start, end)) => TransportCmd::SetLoop { start, end },
                None => TransportCmd::ClearLoop,
            }) {
                self.looped = looped;
            }
        }

        let rolling = stage.rolling();
        if rolling != self.rolling {
            if engine.transport(if rolling {
                TransportCmd::Play
            } else {
                TransportCmd::Stop
            }) {
                self.rolling = rolling;
            }
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
        // The desk's own rails, in the graph's slot order: the four
        // group buses, the two returns, then the mix.
        let mut desk = [Level::default(); daw::ui::stage::DESK_METERS];
        for (index, level) in desk.iter_mut().enumerate() {
            let slot = daw::song_graph::BUS_METER_BASE + index;
            if slot < snapshot.track_peaks_l.len() {
                *level = Level {
                    left: snapshot.track_peaks_l[slot],
                    right: snapshot.track_peaks_r[slot],
                };
            }
        }
        stage.set_desk_levels(&self.levels, master, &desk);
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
        // What the instruments say about themselves, by device.
        self.readouts.clear();
        if let Some(nodes) = &self.nodes {
            for (id, slot) in &nodes.readouts {
                let Some(said) = snapshot.device_readouts.get(*slot) else {
                    continue;
                };
                self.readouts.push((
                    *id,
                    daw::console::Telemetry {
                        level_db: said.level_db,
                        reduction_db: said.reduction_db,
                        bands: said.bands,
                    },
                ));
            }
        }
        stage.set_readouts(&self.readouts);
        // The callback's readings are the only truthful scopes: sources are
        // in Song order, while compiled wires return their stable ids because
        // an unresolved destination may leave a hole in document order.
        stage.set_modulation_readings(
            &snapshot.mod_sources,
            &snapshot.mod_wire_ids,
            &snapshot.mod_wires,
        );
        self.drive_recording(stage);
    }
}

/// Move a controller timestamp backwards on the musical sample clock.
///
/// Ordinarily this is saturating subtraction. When the newest callback block
/// crossed an active loop boundary, however, an older event in the same MIDI
/// batch belongs at the loop tail rather than before the loop head. Keep the
/// result inside the brace and permit batches wider than one pass by reducing
/// the distance modulo the loop span.
fn midi_sample_behind(now: u64, behind: u64, looped: Option<(u64, u64)>, wrapped: bool) -> u64 {
    let Some((start, end)) = looped.filter(|(start, end)| wrapped && start < end) else {
        return now.saturating_sub(behind);
    };
    if now < start || now >= end {
        return now.saturating_sub(behind);
    }
    let span = end - start;
    let offset = now - start;
    let back = behind % span;
    let mapped = if back <= offset {
        offset - back
    } else {
        span - (back - offset)
    };
    start + mapped
}

/// A song tick as a sample position, at the tempo the engine is running.
/// The same arithmetic the engine's own time map does, done here once so
/// a seek lands where the stage's readout says it is.
fn samples_at(engine: &Engine, tick: usize, bpm: f64, tempo: Option<&TempoTable>) -> u64 {
    if let Some(tempo) = tempo {
        return tempo.sample_at(tick);
    }
    let beats = tick as f64 / daw::sequencing::TICKS_PER_BEAT as f64;
    let sample_rate = f64::from(engine.info().sample_rate);
    (beats * 60.0 / bpm.max(1.0) * sample_rate).round() as u64
}

impl App {
    fn new(storage: &shell::Storage) -> Self {
        let prefs: UiPrefs = storage.get(STORAGE_KEY).unwrap_or_default();
        let library_config = storage
            .get(daw::library::CONFIG_STORAGE_KEY)
            .unwrap_or_else(library_config);
        let library_snapshot = storage
            .get(daw::library::CACHE_STORAGE_KEY)
            .unwrap_or_default();
        let mut cli_args = std::env::args_os().skip(1);
        let first = cli_args.next();
        let open_midi = first.as_ref().is_some_and(|arg| arg == "--midi-lab");
        let open_lab = first.as_ref().is_some_and(|arg| arg == "--lab");
        let open_meter = first.as_ref().is_some_and(|arg| arg == "--meter");
        // `--meter-demo` opens the section on material of its own, and
        // rolling. A metering surface opened onto an empty parked song
        // shows the one state it never shows, so the door that exists
        // to LOOK at the section brings something to look at.
        let demo_meter = first.as_ref().is_some_and(|arg| arg == "--meter-demo");
        let cli_path = if open_lab || open_midi || open_meter || demo_meter {
            cli_args.next()
        } else {
            first
        }
        .map(std::path::PathBuf::from);

        let mut stage = Stage::with_library(library_config, library_snapshot);
        // Songs live under the music folder unless a preference gives them a
        // more particular home. The utility restore applies that preference
        // after this host fallback has been established.
        if let Some(home) = std::env::var_os("HOME") {
            stage.set_home(std::path::PathBuf::from(home).join("Music").join("daw"));
        }
        // Sounds are filed beside the theme and the tune, one library
        // for every song on this machine.
        stage.set_sound_library(daw::sound::dir());
        stage.restore_preferences(prefs, cli_path.is_none());

        let audio = Audio::start(engine_config(stage.preferences()));
        if let Some(trouble) = &audio.trouble {
            // stderr rather than the surface: the stage has one message
            // strip and it belongs to the musician, not to the console.
            eprintln!("stage: no audio engine — {trouble}");
        }
        // `cargo run --bin stage -- song.stage.ron` opens that song. A
        // file that will not open is said on stderr and the stage opens
        // empty, rather than refusing to start over a path.
        if let Some(path) = cli_path
            && let Err(error) = stage.open(path)
        {
            eprintln!("stage: could not open the song — {error}");
        }
        if open_midi {
            stage.open_midi_lab("");
        }
        if open_lab {
            let _ = stage.apply(daw::ui::stage::StageIntent::Lab);
        }
        if open_meter {
            let _ = stage.apply(daw::ui::stage::StageIntent::Meter(
                daw::ui::stage::MeterIntent::Open,
            ));
        }
        if demo_meter {
            stage.pose_meter("stage-meter");
        }
        Self {
            stage,
            audio,
            presentation: None,
            last_autosave: std::time::Instant::now(),
            close: CloseInterlock::Idle,
            close_error: None,
        }
    }

    fn serve_utility_request(&mut self) {
        let Some(request) = self.stage.take_utility_host_request() else {
            return;
        };
        match request {
            UtilityHostRequest::ScanAudio(backend) => {
                let api = audio_api(backend);
                let devices: Vec<_> = daw::audio::output_devices(api)
                    .into_iter()
                    .map(|device| AudioDeviceChoice {
                        name: device.name,
                        output_channels: device.output_channels,
                        input_channels: device.input_channels,
                        is_default_output: device.is_default_output,
                        preferred_rate_hz: device.preferred_sample_rate,
                        rates_hz: device.sample_rates,
                    })
                    .collect();
                let status = if !api.compiled() {
                    format!(
                        "{} BACKEND IS NOT IN THIS BUILD",
                        api.label().to_uppercase()
                    )
                } else if devices.is_empty() {
                    format!("{} · NO OUTPUT DEVICES", api.label().to_uppercase())
                } else {
                    format!(
                        "{} · {} OUTPUT DEVICE(S)",
                        api.label().to_uppercase(),
                        devices.len()
                    )
                };
                self.stage.set_audio_devices(backend, devices);
                self.stage.set_audio_preferences_status(status);
            }
            UtilityHostRequest::RestartAudio(settings) => {
                match self.audio.restart(requested_engine_config(settings)) {
                    Ok(stream) => self.stage.set_audio_preferences_status(format!(
                        "RUNNING · {} · {} HZ · {} FR · {:.2} MS",
                        stream.backend,
                        stream.sample_rate,
                        stream.buffer_frames,
                        stream.latency_ms().unwrap_or_default()
                    )),
                    Err(error) => self
                        .stage
                        .set_audio_preferences_status(format!("RESTART FAILED · {error}")),
                }
            }
        }
    }

    fn autosave_recovery(&mut self) {
        // Discard means the recovery copy is deliberately gone. The close
        // frame still reaches this method after the modal handles D, so an
        // exit-approved dirty song must not recreate the sidecar it just
        // removed.
        if self.close == CloseInterlock::Exit {
            return;
        }
        if !self.stage.is_dirty() {
            self.last_autosave = std::time::Instant::now();
            return;
        }
        let Some(period) = self.stage.autosave_period() else {
            return;
        };
        if self.last_autosave.elapsed() < period {
            return;
        }
        if let Err(error) = self.stage.write_recovery() {
            eprintln!("stage: recovery failed — {error}");
        }
        self.last_autosave = std::time::Instant::now();
    }

    fn show_close_interlock(&mut self, ui: &mut egui::Ui) {
        let stopping = self.close == CloseInterlock::StoppingTake;
        let (save, discard, cancel) = ui.input_mut(|input| {
            let cancel = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::C);
            let save = !stopping && input.consume_key(egui::Modifiers::NONE, egui::Key::S);
            let discard = !stopping && input.consume_key(egui::Modifiers::NONE, egui::Key::D);
            (save, discard, cancel)
        });

        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        ui.with_layout(
            egui::Layout::top_down(egui::Align::Center).with_main_align(egui::Align::Center),
            |ui| {
                ui.heading(if stopping {
                    "CLOSING RECORDED TAKE"
                } else {
                    "UNSAVED PROJECT"
                });
                ui.add_space(8.0);
                ui.label(if stopping {
                    "Waiting for the audio callback to release the final block."
                } else {
                    "S  SAVE    D  DISCARD    C / ESC  CANCEL"
                });
                if let Some(error) = &self.close_error {
                    ui.add_space(8.0);
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("SAVE FAILED · {error}"),
                    );
                }
            },
        );

        if cancel {
            self.close = CloseInterlock::Idle;
            self.close_error = None;
        } else if save {
            match self.stage.save() {
                Ok(()) => {
                    self.close = CloseInterlock::Exit;
                    self.close_error = None;
                }
                Err(error) => {
                    // Failed save is never approval to lose the project.
                    self.close = CloseInterlock::Confirm;
                    self.close_error = Some(error);
                }
            }
        } else if discard {
            self.stage.discard_recovery();
            self.close = CloseInterlock::Exit;
            self.close_error = None;
        }
    }

    fn advance_close_interlock(&mut self) {
        if self.close == CloseInterlock::StoppingTake && !self.audio.recording_busy() {
            self.close = if self.stage.is_dirty() {
                CloseInterlock::Confirm
            } else {
                CloseInterlock::Exit
            };
        }
    }
}

impl shell::Host for App {
    /// Where the stage is, for the scripted-input trace.
    fn status(&self) -> String {
        self.stage.status_line()
    }


    /// Fonts and the initial stock-widget theme, once the shell has made
    /// the egui context. The custom shell exists so the completed frame
    /// can pass through `shell::post` before presentation.
    fn startup(&mut self, ctx: &egui::Context) {
        install_stage_fonts(ctx);
        let prefs = self.stage.preferences();
        let mut theme = if prefs.light_ground {
            Theme::light()
        } else {
            Theme::dark()
        };
        theme.set_density(prefs.density);
        theme.apply(ctx);
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
        let density = self.stage.preferences().density;
        let hide_tooltips = self.stage.preferences().hide_tooltips;
        let presentation = (polarity, density, hide_tooltips);
        if self.presentation != Some(presentation) {
            let mut theme = match polarity {
                Polarity::Dark => Theme::dark(),
                Polarity::Light => Theme::light(),
            };
            theme.set_density(density);
            theme.apply(ui.ctx());
            ui.ctx().all_styles_mut(|style| {
                if hide_tooltips {
                    style.interaction.tooltip_delay = f32::INFINITY;
                    style.explanation_tooltips = false;
                    style.url_in_tooltip = false;
                }
            });
            self.presentation = Some(presentation);
        }

        if self.close == CloseInterlock::Idle {
            self.stage.show(ui);
            self.serve_utility_request();
        } else {
            self.show_close_interlock(ui);
        }
        self.audio.follow(&mut self.stage);
        self.advance_close_interlock();
        self.autosave_recovery();
        // A meter that only moves when the mouse does is not a meter.
        ui.ctx().request_repaint();
    }

    fn save(&mut self, storage: &mut shell::Storage) {
        storage.set(STORAGE_KEY, self.stage.preferences());
        storage.set(
            daw::library::CONFIG_STORAGE_KEY,
            self.stage.library_preferences(),
        );
        storage.set(daw::library::CACHE_STORAGE_KEY, self.stage.library_cache());
    }

    fn close_requested(&mut self) -> bool {
        if self.close == CloseInterlock::Exit {
            return true;
        }
        if self.close != CloseInterlock::Idle {
            return false;
        }
        let Some(interlock) = close_interlock_for(
            self.stage.is_dirty(),
            self.stage.recording() || self.audio.recording_busy(),
        ) else {
            return true;
        };
        if interlock == CloseInterlock::StoppingTake && self.stage.recording() {
            let _ = self.stage.apply(StageIntent::ToggleRecord);
        }
        self.close = interlock;
        self.close_error = None;
        false
    }

    fn wants_exit(&self) -> bool {
        self.close == CloseInterlock::Exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A held arrow moves the revision every few frames. Compiling on each
    /// one swapped the schedule thirty times a second, and the node being
    /// edited cannot ride a swap — so the song went quiet until the key
    /// came up. The first edit is still heard at once.
    #[test]
    fn a_lone_edit_compiles_at_once_and_a_held_key_compiles_when_it_stops() {
        use std::time::Duration;

        assert!(compile_now(false, Duration::ZERO), "a lone edit waited");
        assert!(
            !compile_now(true, Duration::ZERO),
            "a burst compiled on its first repeat"
        );
        assert!(
            !compile_now(true, EDIT_SETTLE - Duration::from_millis(1)),
            "a burst compiled while the hand was still moving"
        );
        assert!(
            compile_now(true, EDIT_SETTLE),
            "a burst never compiled after the hand stopped"
        );
        assert!(compile_now(true, Duration::from_secs(1)));
    }

    #[test]
    fn clean_idle_close_needs_no_interlock() {
        assert_eq!(close_interlock_for(false, false), None);
    }

    #[test]
    fn dirty_close_confirms_and_recording_closes_first() {
        assert_eq!(
            close_interlock_for(true, false),
            Some(CloseInterlock::Confirm)
        );
        assert_eq!(
            close_interlock_for(false, true),
            Some(CloseInterlock::StoppingTake)
        );
    }

    #[test]
    fn midi_batch_timestamps_walk_back_across_the_loop_tail() {
        let looped = Some((1_000, 2_000));
        assert_eq!(midi_sample_behind(1_010, 5, looped, true), 1_005);
        assert_eq!(
            midi_sample_behind(1_010, 20, looped, true),
            1_990,
            "a pre-wrap event was placed before the loop head"
        );
        assert_eq!(
            midi_sample_behind(1_010, 20, looped, false),
            990,
            "an ordinary first pass was incorrectly wrapped"
        );
        assert_eq!(
            midi_sample_behind(1_010, 1_020, looped, true),
            1_990,
            "a batch wider than one loop did not retain its musical position"
        );
    }
}
