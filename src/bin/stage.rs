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

use daw::audio::transport::TransportCmd;
use daw::audio::{Engine, EngineConfig, StreamHealth};
use daw::design::Polarity;
use daw::install_fonts;
use daw::params;
use daw::song_graph::{self, MASTER_METER, SongNodes};
use daw::ui::stage::{EngineState, Health, Level, Stage};
use daw::ui::theme::Theme;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("daw — stage")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([720.0, 480.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "daw-stage",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
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
    /// The block the engine had published when it was last told to
    /// seek. Its position is not read back until a LATER block has
    /// arrived: the snapshot in hand was written before the seek, and
    /// handing the stage that position would throw its clock back to
    /// where it just left, for one visible frame.
    seek_block: Option<u64>,
    /// Scratch for the levels handed back to the stage, kept between
    /// frames so a meter costs no allocation per frame.
    levels: Vec<Level>,
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
            mixed: None,
            nodes: None,
            rolling: false,
            bpm: 0.0,
            seeks: 0,
            seek_block: None,
            levels: Vec::new(),
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

    /// Make the engine agree with the stage, then hand back what it heard.
    fn follow(&mut self, stage: &mut Stage) {
        // Told every frame rather than once: a stage that opened without
        // an engine and one whose engine went away are the same state,
        // and the surface should say so either way — and a block dropped
        // this frame is only news this frame.
        let health = self.health();
        stage.set_health(health);
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.collect_trash();

        // What sounds, when it changed. A rebuild mints fresh node ids,
        // so the mapping is captured with the schedule rather than
        // derived from the song afterwards.
        if self.built != Some(stage.revision()) {
            let info = engine.info();
            let (spec, nodes) = song_graph::build(stage.song(), stage.playing());
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
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // The fonts and the theme are the design system, and they are the
        // one part of the old surfaces that carries over unchanged — they
        // encode the aesthetic charter rather than any layout.
        install_fonts(&cc.egui_ctx);
        Theme::dark().apply(&cc.egui_ctx);
        let audio = Audio::start();
        if let Some(trouble) = &audio.trouble {
            // stderr rather than the surface: the stage has one message
            // strip and it belongs to the musician, not to the console.
            eprintln!("stage: no audio engine — {trouble}");
        }
        let mut stage = Stage::new();
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

impl eframe::App for App {
    /// The ground, and meant. Whichever way the polarity is turned, this
    /// is the alphabet's own GROUND rung rather than a colour picked to
    /// look like it — so the window behind the stage and the stage's own
    /// ground can never be two different blacks, or two different papers.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let ground = daw::design::Alphabet::for_polarity(self.stage.polarity())
            .ground
            .color;
        let channel = |v: u8| (v as f32 / 255.0).powf(2.2);
        [
            channel(ground.r()),
            channel(ground.g()),
            channel(ground.b()),
            1.0,
        ]
    }

    /// eframe 0.36 hands the app a `Ui` rather than a `Context` and a
    /// panel to build, so there is nothing between the window and the
    /// stage. That is the whole surface.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
}
