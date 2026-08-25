//! `lab` — development harness.
//!
//! A scratch GUI for exercising pieces of the DAW in isolation: audio backend
//! probing, frame timing, and later, shader and DSP benches. Not shipped.

use daw::audio::graph::{GraphSpec, NodeId, NodeSpec, Note, SubLoop, expanded_len_beats};
use std::path::PathBuf;

/// Where the lab session and bounces land. The dev harness saves next to the
/// project; the real app will ask.
const SESSION_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/lab-session.ron");
const BOUNCE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/lab-bounce.wav");

/// Everything the Engine bench needs to restore a session. Musical/UI state
/// only — no sample positions, no runtime handles.
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionDoc {
    tone_a: bool,
    tone_b: bool,
    freq_a: f32,
    freq_b: f32,
    pan_a: f32,
    master: f32,
    metronome: bool,
    bpm: f32,
    seq_on: bool,
    pattern: Vec<Vec<bool>>,
    sub_on: bool,
    sub_start: u32,
    sub_end: u32,
    sub_repeats: u32,
    clip_path: String,
    clip_on: bool,
    clip_loop: bool,
    clip_gain: f32,
}
use daw::audio::transport::TransportCmd;
use daw::audio::{BlockSnapshot, Engine, EngineConfig, StreamHealth};
use daw::ui::gallery::Gallery;
use daw::ui::theme::Theme;
use daw::{FrameStats, adapter_label, install_fonts};
use eframe::egui;
use std::collections::VecDeque;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("daw — lab")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([720.0, 480.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "daw-lab",
        options,
        Box::new(|cc| Ok(Box::new(Lab::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bench {
    Audio,
    Engine,
    Sequencer,
    System,
    Timing,
    Ui,
}

impl Bench {
    const ALL: [Self; 6] = [
        Self::Audio,
        Self::Engine,
        Self::Sequencer,
        Self::System,
        Self::Timing,
        Self::Ui,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio backend",
            Self::Engine => "Engine",
            Self::Sequencer => "Sequencer",
            Self::System => "System map",
            Self::Timing => "Frame timing",
            Self::Ui => "UI kit",
        }
    }
}

struct Lab {
    adapter: String,
    frames: FrameStats,
    bench: Bench,
    audio: AudioProbe,
    engine: EngineBench,
    /// The real app's kit and panels, previewed with no engine behind them.
    gallery: Gallery,
    theme: Theme,
}

impl Lab {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);

        Self {
            adapter: adapter_label(cc),
            frames: FrameStats::default(),
            bench: Bench::Audio,
            audio: AudioProbe::new(),
            engine: EngineBench::default(),
            gallery: Gallery::default(),
            theme: Theme::dark(),
        }
    }
}

impl eframe::App for Lab {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frames.push(ui.ctx().input(|i| i.stable_dt));

        egui::Panel::left("benches")
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading("lab");
                ui.separator();
                for bench in Bench::ALL {
                    ui.selectable_value(&mut self.bench, bench, bench.label());
                }
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "{:5.2} ms  avg {:5.2}  peak {:5.2}",
                    self.frames.last_ms(),
                    self.frames.mean_ms(),
                    self.frames.max_ms()
                ));
                ui.separator();
                ui.monospace(&self.adapter);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.bench {
            Bench::Audio => self.audio.ui(ui),
            Bench::Engine => self.engine.ui(ui),
            Bench::Sequencer => sequencer_ui(ui, &mut self.engine),
            Bench::System => system_map(ui, &mut self.engine),
            Bench::Timing => {
                ui.heading("Frame timing");
                ui.add_space(8.0);
                let w = ui.available_width();
                self.frames.sparkline(ui, egui::vec2(w, 120.0));
                ui.add_space(8.0);
                ui.monospace(format!("window: {} frames", self.frames.samples().count()));
                // Timing bench needs continuous frames to mean anything.
                ui.ctx().request_repaint();
            }
            Bench::Ui => self.gallery.ui(ui, &mut self.theme),
        });
    }
}

/// Snapshot of what the rtaudio backend reports. Rescanned on demand rather
/// than per frame — opening a Host touches the sound server.
struct AudioProbe {
    /// Backends actually compiled into librtaudio.
    compiled_apis: Vec<rtaudio::Api>,
    /// Which backend this probe opened.
    selected: rtaudio::Api,
    api_in_use: rtaudio::Api,
    devices: Result<Vec<DeviceRow>, String>,
}

struct DeviceRow {
    name: String,
    out_ch: u32,
    in_ch: u32,
    duplex_ch: u32,
    default_out: bool,
    default_in: bool,
    preferred_rate: u32,
}

impl AudioProbe {
    fn new() -> Self {
        let compiled_apis = rtaudio::compiled_apis();
        // Api::Unspecified resolves to ALSA on this machine even when JACK is
        // compiled in, so default the harness to JACK when it is available.
        let selected = if compiled_apis.contains(&rtaudio::Api::UnixJack) {
            rtaudio::Api::UnixJack
        } else {
            rtaudio::Api::Unspecified
        };
        Self::probe(compiled_apis, selected)
    }

    fn probe(compiled_apis: Vec<rtaudio::Api>, selected: rtaudio::Api) -> Self {
        match rtaudio::Host::new(selected) {
            Ok(mut host) => {
                // ALSA is chatty on stderr about devices it cannot open.
                host.show_warnings(false);
                let api_in_use = host.api();
                let devices = host
                    .devices()
                    .iter()
                    .map(|d| DeviceRow {
                        name: d.name().to_owned(),
                        out_ch: d.output_channels,
                        in_ch: d.input_channels,
                        duplex_ch: d.duplex_channels,
                        default_out: d.is_default_output,
                        default_in: d.is_default_input,
                        preferred_rate: d.preferred_sample_rate,
                    })
                    .collect();
                Self {
                    compiled_apis,
                    selected,
                    api_in_use,
                    devices: Ok(devices),
                }
            }
            Err(e) => Self {
                compiled_apis,
                selected,
                api_in_use: rtaudio::Api::Unspecified,
                devices: Err(format!("{e}")),
            },
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Audio backend");
            if ui.button("Rescan").clicked() {
                *self = Self::probe(self.compiled_apis.clone(), self.selected);
            }
        });
        ui.add_space(8.0);

        // The check that matters: JACK must be compiled in, or we are silently
        // on the ALSA/Pulse compat path with no symptom pointing at the cause.
        let has_jack = self.compiled_apis.contains(&rtaudio::Api::UnixJack);
        ui.horizontal(|ui| {
            ui.monospace("compiled:");
            ui.monospace(
                self.compiled_apis
                    .iter()
                    .map(|a| format!("{a:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        });
        if has_jack {
            ui.colored_label(
                egui::Color32::from_rgb(0x5f, 0xb3, 0x5f),
                "JACK compiled in",
            );
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                "JACK MISSING — rebuild rtaudio with features = [\"jack_linux\"]",
            );
        }

        ui.add_space(6.0);
        let mut pick = self.selected;
        egui::ComboBox::from_label("backend")
            .selected_text(format!("{pick:?}"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut pick, rtaudio::Api::Unspecified, "Unspecified");
                for api in &self.compiled_apis {
                    ui.selectable_value(&mut pick, *api, format!("{api:?}"));
                }
            });
        if pick != self.selected {
            *self = Self::probe(self.compiled_apis.clone(), pick);
        }

        // Unspecified does not mean "best" — it resolved to ALSA here.
        if self.selected == rtaudio::Api::Unspecified && self.api_in_use != rtaudio::Api::UnixJack {
            ui.colored_label(
                egui::Color32::from_rgb(0xd0, 0xa0, 0x5f),
                format!("Unspecified resolved to {:?}, not JACK", self.api_in_use),
            );
        }
        ui.monospace(format!("in use: {:?}", self.api_in_use));
        ui.separator();

        match &self.devices {
            Err(e) => {
                ui.colored_label(
                    egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                    format!("host error: {e}"),
                );
            }
            Ok(devices) if devices.is_empty() => {
                ui.label("no devices reported");
            }
            Ok(devices) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for d in devices {
                        ui.horizontal(|ui| {
                            ui.monospace(format!(
                                "{:>3}o {:>3}i {:>3}dx  {:>6} Hz",
                                d.out_ch, d.in_ch, d.duplex_ch, d.preferred_rate
                            ));
                            ui.label(&d.name);
                            if d.default_out {
                                ui.weak("[default out]");
                            }
                            if d.default_in {
                                ui.weak("[default in]");
                            }
                        });
                    }
                });
            }
        }
    }
}

/// Opens the real duplex stream. The engine writes silence for now; this bench
/// exists to prove the stream negotiates what we asked for.
/// How many blocks the scrolling readout keeps on screen.
const BELT_ROWS: usize = 24;

struct EngineBench {
    engine: Option<Engine>,
    error: Option<String>,
    tone_a: bool,
    tone_b: bool,
    freq_a: f32,
    freq_b: f32,
    master: f32,
    metronome: bool,
    bpm: f32,
    loop_beats: u32,
    loop_on: bool,
    /// 16 steps x 25 semitones, step = a 16th note (0.25 beat). true = note.
    pattern: [[bool; 16]; 25],
    seq_on: bool,
    /// Subloop over the grid, in steps (16ths). Unrolled at compile.
    sub_on: bool,
    sub_start: u32,
    sub_end: u32,
    sub_repeats: u32,
    clip_path: String,
    clip_on: bool,
    clip_loop: bool,
    clip_gain: f32,
    clip_id: Option<NodeId>,
    /// Name tags into the current schedule.
    id_a: Option<NodeId>,
    id_pan_a: Option<NodeId>,
    pan_a: f32,
    id_b: Option<NodeId>,
    id_mixer: Option<NodeId>,
    /// Newest block first. Sampled at UI rate, so this is a thinned view of the
    /// belt, not every block — blocks arrive ~187x/sec, frames redraw ~60x/sec.
    belt: VecDeque<BlockSnapshot>,
}

impl Default for EngineBench {
    fn default() -> Self {
        Self {
            engine: None,
            error: None,
            tone_a: false,
            tone_b: false,
            freq_a: 330.0,
            freq_b: 440.0,
            master: 0.15,
            metronome: false,
            bpm: 120.0,
            loop_beats: 4,
            loop_on: false,
            pattern: [[false; 16]; 25],
            seq_on: false,
            sub_on: false,
            sub_start: 4,
            sub_end: 8,
            sub_repeats: 2,
            clip_path: String::new(),
            clip_on: false,
            clip_loop: true,
            clip_gain: 0.8,
            clip_id: None,
            id_a: None,
            id_pan_a: None,
            pan_a: 0.0,
            id_b: None,
            id_mixer: None,
            belt: VecDeque::new(),
        }
    }
}

impl EngineBench {
    fn to_doc(&self) -> SessionDoc {
        SessionDoc {
            tone_a: self.tone_a,
            tone_b: self.tone_b,
            freq_a: self.freq_a,
            freq_b: self.freq_b,
            pan_a: self.pan_a,
            master: self.master,
            metronome: self.metronome,
            bpm: self.bpm,
            seq_on: self.seq_on,
            pattern: self.pattern.iter().map(|r| r.to_vec()).collect(),
            sub_on: self.sub_on,
            sub_start: self.sub_start,
            sub_end: self.sub_end,
            sub_repeats: self.sub_repeats,
            clip_path: self.clip_path.clone(),
            clip_on: self.clip_on,
            clip_loop: self.clip_loop,
            clip_gain: self.clip_gain,
        }
    }

    fn apply_doc(&mut self, d: SessionDoc) {
        self.tone_a = d.tone_a;
        self.tone_b = d.tone_b;
        self.freq_a = d.freq_a;
        self.freq_b = d.freq_b;
        self.pan_a = d.pan_a;
        self.master = d.master;
        self.metronome = d.metronome;
        self.bpm = d.bpm;
        self.seq_on = d.seq_on;
        for (dst, src) in self.pattern.iter_mut().zip(d.pattern.iter()) {
            for (c, v) in dst.iter_mut().zip(src.iter()) {
                *c = *v;
            }
        }
        self.sub_on = d.sub_on;
        self.sub_start = d.sub_start;
        self.sub_end = d.sub_end;
        self.sub_repeats = d.sub_repeats;
        self.clip_path = d.clip_path;
        self.clip_on = d.clip_on;
        self.clip_loop = d.clip_loop;
        self.clip_gain = d.clip_gain;
    }

    /// Rebuild the same GraphSpec push_graph builds, without swapping it —
    /// for bounce and project export.
    fn build_spec(&mut self) -> GraphSpec {
        let mut spec = GraphSpec::default();
        self.populate_spec(&mut spec);
        spec
    }

    /// Compile the current graph and swap it into the running engine.
    /// Structure changes (tone toggles) go through here; knob turns do not.
    fn push_graph(&mut self) {
        let Some(info) = self.engine.as_ref().map(|e| e.info()) else {
            return;
        };
        let mut spec = GraphSpec::default();
        self.populate_spec(&mut spec);

        match spec.compile(info.sample_rate, info.max_frames) {
            Ok(sched) => {
                if let Some(engine) = &mut self.engine
                    && let Err(e) = engine.set_schedule(Box::new(sched))
                {
                    self.error = Some(e.to_string());
                }
            }
            Err(e) => self.error = Some(format!("graph refused: {e}")),
        }
    }

    /// The one place the lab's graph shape is defined.
    fn populate_spec(&mut self, spec: &mut GraphSpec) {
        // Tones feed a mixer; the mixer feeds the speakers. Sine amps are
        // fixed — loudness lives in the mixer's master gain.
        self.id_a = self.tone_a.then(|| {
            spec.push(NodeSpec::Sine {
                freq: self.freq_a,
                amp: 0.5,
            })
        });
        // Tone A routes through a pan node — the audible stereo demo.
        self.id_pan_a = self.id_a.map(|a| {
            let pan = spec.push(NodeSpec::Pan {
                pan: self.pan_a,
                gain: 1.0,
            });
            spec.connect(a, pan);
            pan
        });
        self.id_b = self.tone_b.then(|| {
            spec.push(NodeSpec::Sine {
                freq: self.freq_b,
                amp: 0.5,
            })
        });
        let seq = self.seq_on.then(|| {
            // Grid -> notes: row 0 is the TOP of the grid (highest pitch);
            // pitch 48 (C3) at the bottom. Step = 0.25 beat, gate 80%.
            let mut notes = Vec::new();
            for (row, steps) in self.pattern.iter().enumerate() {
                let pitch = 48 + (24 - row) as u8;
                for (step, on) in steps.iter().enumerate() {
                    if *on {
                        notes.push(Note {
                            start_beats: step as f64 * 0.25,
                            len_beats: 0.2,
                            pitch,
                            vel: 100,
                        });
                    }
                }
            }
            let subloops = if self.sub_on && self.sub_end > self.sub_start {
                vec![SubLoop {
                    start_beats: self.sub_start as f64 * 0.25,
                    end_beats: self.sub_end as f64 * 0.25,
                    repeats: self.sub_repeats,
                }]
            } else {
                Vec::new()
            };
            let loop_len_beats = Some(expanded_len_beats(4.0, &subloops));
            spec.push(NodeSpec::Seq {
                notes,
                subloops,
                loop_len_beats,
                params: Default::default(),
            })
        });
        self.clip_id = (self.clip_on && !self.clip_path.is_empty()).then(|| {
            spec.push(NodeSpec::AudioClip {
                path: PathBuf::from(&self.clip_path),
                start_beats: 0.0,
                length_beats: None,
                source_offset_frames: 0,
                source_frames: None,
                loop_clip: self.clip_loop,
                gain: self.clip_gain,
            })
        });
        let click = self.metronome.then(|| spec.push(NodeSpec::Click));
        self.id_mixer = (self.id_a.is_some()
            || self.id_b.is_some()
            || click.is_some()
            || seq.is_some()
            || self.clip_id.is_some())
        .then(|| {
            let mixer = spec.push(NodeSpec::Mixer { gain: self.master });
            for id in [self.id_pan_a, self.id_b, click, seq, self.clip_id]
                .into_iter()
                .flatten()
            {
                spec.connect(id, mixer);
            }
            spec.set_output(mixer);
            mixer
        });
    }
}

impl EngineBench {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Engine");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if self.engine.is_none() {
                if ui.button("Start duplex stream").clicked() {
                    match Engine::start(EngineConfig::default()) {
                        Ok(e) => {
                            self.engine = Some(e);
                            self.error = None;
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            self.engine = None;
                        }
                    }
                }
            } else if ui.button("Stop").clicked() {
                // Dropping the Engine stops and closes the stream.
                self.engine = None;
                self.belt.clear();
                self.tone_a = false;
                self.tone_b = false;
                self.id_a = None;
                self.id_b = None;
                self.id_mixer = None;
            }
            ui.label(if self.engine.is_some() {
                "running"
            } else {
                "stopped"
            });
        });

        ui.add_space(8.0);

        if let Some(err) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(0xd0, 0x5f, 0x5f), err);
        }

        if self.engine.is_some() {
            ui.add_space(8.0);
            ui.separator();
            // Session bar: save/load the whole bench state; bounce renders
            // the current graph offline to a wav — no device involved.
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.strong("session");
                if ui.button("save").clicked() {
                    let doc = self.to_doc();
                    let res = ron::ser::to_string_pretty(&doc, Default::default())
                        .map_err(|e| e.to_string())
                        .and_then(|t| std::fs::write(SESSION_PATH, t).map_err(|e| e.to_string()));
                    self.error = res.err().map(|e| format!("save failed: {e}"));
                }
                if ui.button("load").clicked() {
                    match std::fs::read_to_string(SESSION_PATH)
                        .map_err(|e| e.to_string())
                        .and_then(|t| ron::from_str::<SessionDoc>(&t).map_err(|e| e.to_string()))
                    {
                        Ok(doc) => {
                            self.apply_doc(doc);
                            if let Some(engine) = &mut self.engine {
                                engine.transport(daw::audio::transport::TransportCmd::SetTempo(
                                    self.bpm as f64,
                                ));
                            }
                            self.push_graph();
                            self.error = None;
                        }
                        Err(e) => self.error = Some(format!("load failed: {e}")),
                    }
                }
                if ui.button("bounce 8 beats").clicked() {
                    let spec = self.build_spec();
                    let opts = daw::audio::bounce::BounceOptions {
                        bpm: self.bpm as f64,
                        length_beats: 8.0,
                        ..Default::default()
                    };
                    self.error =
                        daw::audio::bounce::bounce(&spec, &opts, std::path::Path::new(BOUNCE_PATH))
                            .err()
                            .map(|e| format!("bounce failed: {e}"));
                }
                ui.weak(format!(
                    "→ {}",
                    SESSION_PATH.rsplit('/').next().unwrap_or("")
                ));
            });
            ui.add_space(4.0);

            // Structure changes (toggles) recompile and swap the schedule;
            // slider moves send 16-byte letters and never recompile.
            let mut structure_changed = false;
            ui.horizontal(|ui| {
                ui.strong("tone A");
                let label_a = if self.tone_a { "on" } else { "off" };
                structure_changed |= ui.toggle_value(&mut self.tone_a, label_a).changed();
                let a_moved = ui
                    .add(
                        egui::Slider::new(&mut self.freq_a, 40.0..=4000.0)
                            .logarithmic(true)
                            .suffix(" Hz"),
                    )
                    .changed();
                if a_moved
                    && let Some(id) = self.id_a
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(id, 0, self.freq_a);
                }
                ui.label("pan");
                let pan_moved = ui
                    .add(egui::Slider::new(&mut self.pan_a, -1.0..=1.0))
                    .changed();
                if pan_moved
                    && let Some(id) = self.id_pan_a
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(id, 0, self.pan_a);
                }
            });
            ui.horizontal(|ui| {
                ui.strong("tone B");
                let label_b = if self.tone_b { "on" } else { "off" };
                structure_changed |= ui.toggle_value(&mut self.tone_b, label_b).changed();
                let b_moved = ui
                    .add(
                        egui::Slider::new(&mut self.freq_b, 40.0..=4000.0)
                            .logarithmic(true)
                            .suffix(" Hz"),
                    )
                    .changed();
                if b_moved
                    && let Some(id) = self.id_b
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(id, 0, self.freq_b);
                }
            });
            ui.horizontal(|ui| {
                ui.strong("master");
                let m_moved = ui
                    .add(egui::Slider::new(&mut self.master, 0.0..=0.5))
                    .changed();
                if m_moved
                    && let Some(id) = self.id_mixer
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(id, 0, self.master);
                }
            });
            ui.horizontal(|ui| {
                ui.strong("metronome");
                let label_m = if self.metronome { "on" } else { "off" };
                structure_changed |= ui.toggle_value(&mut self.metronome, label_m).changed();
                ui.label("bpm");
                if ui
                    .add(egui::Slider::new(&mut self.bpm, 40.0..=240.0))
                    .changed()
                    && let Some(engine) = &mut self.engine
                {
                    engine.transport(TransportCmd::SetTempo(self.bpm as f64));
                }
            });
            if structure_changed {
                self.push_graph();
            }

            // Audio clip: streamed from disk by creek. 1:1 rate (no
            // resampling yet — a non-48k file plays detuned).
            ui.horizontal(|ui| {
                ui.strong("audio clip");
                let clip_label = if self.clip_on { "on" } else { "off" };
                structure_changed |= ui.toggle_value(&mut self.clip_on, clip_label).changed();
                structure_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.clip_path)
                            .hint_text("path/to/file.wav")
                            .desired_width(240.0),
                    )
                    .lost_focus();
                structure_changed |= ui.checkbox(&mut self.clip_loop, "loop").changed();
                let g_moved = ui
                    .add(egui::Slider::new(&mut self.clip_gain, 0.0..=1.5).text("gain"))
                    .changed();
                if g_moved
                    && let Some(id) = self.clip_id
                    && let Some(engine) = &mut self.engine
                {
                    engine.set_param(id, 0, self.clip_gain);
                }
                if ui.button("generate test loop").clicked() {
                    let path = std::env::temp_dir().join("daw-lab-loop.wav");
                    if write_test_loop(&path).is_ok() {
                        self.clip_path = path.to_string_lossy().into_owned();
                        self.clip_on = true;
                        structure_changed = true;
                    }
                }
            });

            // Transport bar. Commands ride their own ring; ring order makes a
            // gesture atomic with respect to audio.
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.strong("transport");
                if let Some(engine) = &mut self.engine {
                    let latest = engine.latest_block();
                    if latest.playing {
                        if ui.button("⏸ stop").clicked() {
                            engine.transport(TransportCmd::Stop);
                        }
                    } else if ui.button("▶ play").clicked() {
                        engine.transport(TransportCmd::Play);
                    }
                    if ui.button("⏮ return").clicked() {
                        engine.transport(TransportCmd::Return);
                    }
                    let info = engine.info();
                    let secs = latest.position as f64 / info.sample_rate.max(1) as f64;
                    ui.monospace(format!("beat {:8.2}   {:7.2}s", latest.beat, secs));
                    let was_looping = self.loop_on;
                    ui.checkbox(&mut self.loop_on, "loop");
                    let beats_moved = ui
                        .add(egui::Slider::new(&mut self.loop_beats, 1..=16).suffix(" beats"))
                        .changed();
                    if self.loop_on != was_looping || (self.loop_on && beats_moved) {
                        if self.loop_on {
                            // Beats -> samples through the engine's one TimeMap
                            // rule: same formula, same rounding.
                            let spb = 60.0 / self.bpm as f64 * info.sample_rate as f64;
                            let end = (self.loop_beats as f64 * spb).round() as u64;
                            engine.transport(TransportCmd::SetLoop { start: 0, end });
                        } else {
                            engine.transport(TransportCmd::ClearLoop);
                        }
                    }
                }
            });
        }

        let Some(engine) = &mut self.engine else {
            return;
        };
        let info = engine.info();

        // Death notice before anything else: a dead stream makes every other
        // number on this screen stale.
        match engine.health() {
            StreamHealth::Running => {}
            StreamHealth::Stalled { seconds } => {
                ui.add_space(6.0);
                ui.colored_label(
                    egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                    egui::RichText::new(format!(
                        "STREAM DEAD — no blocks for {seconds:.1}s (audio server gone?).                          Stop and Start to reconnect."
                    ))
                    .strong(),
                );
            }
            StreamHealth::Errored(err) => {
                ui.add_space(6.0);
                ui.colored_label(
                    egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                    egui::RichText::new(format!(
                        "STREAM ERROR — {err}. Stop and Start to reconnect."
                    ))
                    .strong(),
                );
            }
        }

        // Pull the newest block and push it onto the belt if it is genuinely new.
        let latest = engine.latest_block();
        if self.belt.front().map(|b| b.block) != Some(latest.block) {
            if self.belt.len() == BELT_ROWS {
                self.belt.pop_back();
            }
            self.belt.push_front(latest);
        }

        ui.separator();

        // The number this project is being built around: how much of the
        // 5.33ms deadline each block actually costs.
        let load = latest.load(info.sample_rate, info.max_frames as u32);
        let max_load = BlockSnapshot {
            work_ns: latest.work_max_ns,
            ..latest
        }
        .load(info.sample_rate, info.max_frames as u32);
        ui.horizontal(|ui| {
            ui.strong("dsp load");
            ui.monospace(format!(
                "{:6.3}%  (worst {:6.3}%)   {:7} ns/block",
                load * 100.0,
                max_load * 100.0,
                latest.work_ns
            ));
        });
        let xrun_color = if latest.underflows + latest.overflows == 0 {
            egui::Color32::from_rgb(0x5f, 0xb3, 0x5f)
        } else {
            egui::Color32::from_rgb(0xd0, 0x5f, 0x5f)
        };
        ui.horizontal(|ui| {
            ui.strong("xruns");
            ui.colored_label(
                xrun_color,
                egui::RichText::new(format!(
                    "{} underflows / {} overflows",
                    latest.underflows, latest.overflows
                ))
                .monospace(),
            );
        });

        ui.separator();
        ui.monospace(format!("sample rate : {} Hz", info.sample_rate));
        ui.monospace(format!("max frames  : {}", info.max_frames));
        ui.monospace(format!(
            "channels    : {} in / {} out",
            info.in_channels, info.out_channels
        ));
        match info.latency_frames {
            Some(l) => ui.monospace(format!("latency     : {l} frames")),
            None => ui.monospace("latency     : not reported"),
        };

        // Planar layout is assumed by the DSP. If NONINTERLEAVED was not
        // honoured, every buffer index in the engine is wrong.
        if info.deinterleaved {
            ui.colored_label(
                egui::Color32::from_rgb(0x5f, 0xb3, 0x5f),
                "deinterleaved (planar)",
            );
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                "INTERLEAVED — planar assumptions are invalid",
            );
        }

        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("the belt");
            ui.weak(format!(
                "— INPUT ch0 (output muted: feedback), newest first, {} of ~187 blocks/sec sampled",
                BELT_ROWS
            ));
        });
        ui.add_space(4.0);

        // Column header, aligned with the rows below it.
        ui.monospace(format!(
            "{:>10}  {:>5}  {:>9}   samples",
            "block", "frames", "peak"
        ));

        let dim = ui.visuals().weak_text_color();
        for (row, b) in self.belt.iter().enumerate() {
            // Fade older rows so the motion is legible rather than a wall of text.
            let t = row as f32 / BELT_ROWS as f32;
            let color = ui.visuals().text_color().lerp_to_gamma(dim, t);

            let mut line = format!("{:>10}  {:>5}  {:>9.6}   ", b.block, b.frames, b.peak);
            for s in b.head.iter() {
                line.push_str(&format!("{s:>+9.6} "));
            }
            ui.colored_label(color, egui::RichText::new(line).monospace());
        }

        // The belt only looks like a belt if we keep redrawing.
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------
// System map: where our threads live on the die, drawn with live numbers.
// ---------------------------------------------------------------------------

const RZ: egui::Color32 = egui::Color32::from_rgb(0xef, 0x6f, 0x5f);
const GZ: egui::Color32 = egui::Color32::from_rgb(0x55, 0xb9, 0x8a);
const AMBER: egui::Color32 = egui::Color32::from_rgb(0xd9, 0xa7, 0x43);

/// Draw the CPU/RAM map. Coordinates are designed in an 880x560 virtual space
/// and scaled to whatever width the panel has.
fn system_map(ui: &mut egui::Ui, engine_bench: &mut EngineBench) {
    ui.heading("System map");
    ui.weak("the die, the caches, and where each thread stands — live while the engine runs");
    ui.add_space(6.0);

    // Live numbers, if the engine is up.
    let live = engine_bench
        .engine
        .as_mut()
        .map(|e| (e.info(), e.latest_block()));

    let width = ui.available_width().min(980.0);
    let scale = width / 880.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 560.0 * scale), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let dim = ui.visuals().weak_text_color();
    let ink = ui.visuals().text_color();

    // Virtual-space helpers.
    let pt = |x: f32, y: f32| rect.min + egui::vec2(x * scale, y * scale);
    let boxed =
        |p: &egui::Painter, x: f32, y: f32, w: f32, h: f32, color: egui::Color32, stroke_w: f32| {
            p.rect_stroke(
                egui::Rect::from_min_size(pt(x, y), egui::vec2(w * scale, h * scale)),
                4.0 * scale,
                egui::Stroke::new(stroke_w, color),
                egui::StrokeKind::Middle,
            );
        };
    let label = |p: &egui::Painter, x: f32, y: f32, text: &str, size: f32, color: egui::Color32| {
        p.text(
            pt(x, y),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::monospace(size * scale),
            color,
        );
    };
    let arrow = |p: &egui::Painter, from: (f32, f32), to: (f32, f32), color: egui::Color32| {
        let (a, b) = (pt(from.0, from.1), pt(to.0, to.1));
        p.line_segment([a, b], egui::Stroke::new(1.2, color));
        let dir = (b - a).normalized();
        let n = egui::vec2(-dir.y, dir.x);
        let tip = b;
        let s = 5.0 * scale;
        p.add(egui::Shape::convex_polygon(
            vec![
                tip,
                tip - dir * s + n * s * 0.5,
                tip - dir * s - n * s * 0.5,
            ],
            color,
            egui::Stroke::NONE,
        ));
    };

    // Die outline.
    boxed(&p, 20.0, 10.0, 600.0, 360.0, dim, 1.0);
    label(&p, 36.0, 20.0, "Ryzen AI 9 465 — one die", 11.0, dim);

    // Audio core.
    boxed(&p, 44.0, 46.0, 180.0, 210.0, RZ, 1.5);
    label(&p, 56.0, 54.0, "core — audio thread", 11.0, RZ);
    label(&p, 56.0, 68.0, "red zone / RT", 9.5, dim);
    boxed(&p, 56.0, 84.0, 156.0, 32.0, ink, 1.0);
    label(&p, 64.0, 88.0, "L1d 48 KB", 10.5, ink);
    label(&p, 64.0, 101.0, "schedule + arena live here", 8.5, AMBER);
    boxed(&p, 56.0, 122.0, 156.0, 24.0, ink, 1.0);
    label(&p, 64.0, 128.0, "L1i 32 KB  callback code", 10.0, ink);
    boxed(&p, 56.0, 152.0, 156.0, 24.0, ink, 1.0);
    label(&p, 64.0, 158.0, "L2 1 MB  private", 10.0, ink);
    match &live {
        Some((info, b)) => {
            let budget_ns = info.max_frames as f64 / info.sample_rate.max(1) as f64 * 1e9;
            label(&p, 56.0, 186.0, &format!("block #{}", b.block), 9.5, ink);
            label(
                &p,
                56.0,
                200.0,
                &format!(
                    "work {} ns ({:.2}%)",
                    b.work_ns,
                    b.work_ns as f64 / budget_ns * 100.0
                ),
                9.5,
                ink,
            );
            label(
                &p,
                56.0,
                214.0,
                &format!("worst {} ns", b.work_max_ns),
                9.5,
                dim,
            );
            let xr = b.underflows + b.overflows;
            label(
                &p,
                56.0,
                228.0,
                &format!("xruns {xr}"),
                9.5,
                if xr == 0 { GZ } else { RZ },
            );
        }
        None => {
            label(&p, 56.0, 196.0, "engine stopped —", 9.5, dim);
            label(&p, 56.0, 210.0, "start it in the Engine bench", 9.5, dim);
        }
    }

    // UI core.
    boxed(&p, 248.0, 46.0, 180.0, 210.0, GZ, 1.5);
    label(&p, 260.0, 54.0, "core — UI thread", 11.0, GZ);
    label(&p, 260.0, 68.0, "green zone / egui+wgpu", 9.5, dim);
    boxed(&p, 260.0, 84.0, 156.0, 32.0, ink, 1.0);
    label(&p, 268.0, 88.0, "L1d 48 KB", 10.5, ink);
    label(&p, 268.0, 101.0, "widgets, belt rows", 8.5, dim);
    boxed(&p, 260.0, 122.0, 156.0, 24.0, ink, 1.0);
    label(&p, 268.0, 128.0, "L1i 32 KB", 10.0, ink);
    boxed(&p, 260.0, 152.0, 156.0, 24.0, ink, 1.0);
    label(&p, 268.0, 158.0, "L2 1 MB  private", 10.0, ink);
    label(&p, 260.0, 190.0, "~60 fps redraw", 9.5, dim);
    label(&p, 260.0, 204.0, "compile() allocates here", 9.5, dim);
    label(&p, 260.0, 218.0, "drops retired schedules", 9.5, dim);

    // Other cores.
    for (i, name) in ["pipewire", "wgpu", "", "", "", ""].iter().enumerate() {
        let (col, row) = (i % 2, i / 2);
        let (x, y) = (452.0 + col as f32 * 80.0, 46.0 + row as f32 * 70.0);
        boxed(&p, x, y, 70.0, 60.0, dim, 1.0);
        if !name.is_empty() {
            label(&p, x + 8.0, y + 8.0, name, 9.0, dim);
        }
    }
    label(&p, 452.0, 262.0, "8 more cores, mostly idle", 9.0, dim);

    // L3.
    boxed(&p, 44.0, 290.0, 558.0, 40.0, AMBER, 1.25);
    label(
        &p,
        56.0,
        300.0,
        "L3 24 MB — shared by all cores",
        11.0,
        AMBER,
    );
    arrow(&p, (134.0, 256.0), (134.0, 290.0), RZ);
    arrow(&p, (338.0, 290.0), (338.0, 256.0), GZ);
    label(&p, 150.0, 264.0, "telemetry ~100 B/block", 9.0, dim);

    // RAM.
    boxed(&p, 44.0, 388.0, 558.0, 48.0, ink, 1.0);
    label(&p, 56.0, 396.0, "DDR5 30 GB", 11.0, ink);
    label(
        &p,
        56.0,
        412.0,
        "font atlas 14 MB · wgpu buffers · later: samples via creek",
        9.0,
        dim,
    );
    arrow(&p, (323.0, 330.0), (323.0, 388.0), dim);
    label(&p, 332.0, 352.0, "only on a cache miss — ~90 ns", 9.0, dim);

    // Codec.
    boxed(&p, 660.0, 46.0, 190.0, 110.0, dim, 1.0);
    label(&p, 672.0, 56.0, "ALC245 codec", 10.5, dim);
    label(&p, 672.0, 72.0, "mic in / speakers out", 9.0, dim);
    arrow(&p, (620.0, 96.0), (660.0, 96.0), RZ);
    arrow(&p, (660.0, 124.0), (620.0, 124.0), RZ);
    label(&p, 668.0, 132.0, "256 frames / 5.33 ms", 8.5, dim);

    // Border crossings summary.
    label(
        &p,
        24.0,
        460.0,
        "border crossings (lock-free, the only doors between zones):",
        10.0,
        ink,
    );
    arrow(&p, (60.0, 486.0), (200.0, 486.0), GZ);
    label(
        &p,
        210.0,
        480.0,
        "rtrb: Box<Schedule> — new chart as one 8-byte pointer",
        9.5,
        ink,
    );
    arrow(&p, (200.0, 508.0), (60.0, 508.0), RZ);
    label(
        &p,
        210.0,
        502.0,
        "rtrb: retired schedule back, freed in the green zone",
        9.5,
        ink,
    );
    arrow(&p, (200.0, 530.0), (60.0, 530.0), RZ);
    label(
        &p,
        210.0,
        524.0,
        "triple buffer: BlockSnapshot, latest wins",
        9.5,
        ink,
    );

    if live.is_some() {
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Sequencer: a 16-step x 2-octave grid. Click a cell to toggle a note. Edits
// recompile the graph and swap it into the running engine — the piano roll is
// a GraphSpec editor, nothing more.
// ---------------------------------------------------------------------------

fn sequencer_ui(ui: &mut egui::Ui, eng: &mut EngineBench) {
    ui.heading("Sequencer");
    ui.weak("16 steps of 16ths (4 beats) · C3-C5 · edits swap live");
    ui.add_space(6.0);

    if eng.engine.is_none() {
        ui.label("start the engine in the Engine bench first");
        return;
    }

    let mut changed = false;
    ui.horizontal(|ui| {
        let label = if eng.seq_on { "on" } else { "off" };
        changed |= ui.toggle_value(&mut eng.seq_on, label).changed();
        if ui.button("clear").clicked() {
            eng.pattern = [[false; 16]; 25];
            changed = true;
        }
        let subloops_for_len = if eng.sub_on && eng.sub_end > eng.sub_start {
            vec![SubLoop {
                start_beats: eng.sub_start as f64 * 0.25,
                end_beats: eng.sub_end as f64 * 0.25,
                repeats: eng.sub_repeats,
            }]
        } else {
            Vec::new()
        };
        let total_beats = expanded_len_beats(4.0, &subloops_for_len);
        if ui.button("▶ play clip").clicked()
            && let Some(engine) = &mut eng.engine
        {
            // The clip cycles by itself — the transport just rolls forward,
            // Ableton-style. No transport loop involved.
            engine.transport(daw::audio::transport::TransportCmd::Seek(0));
            engine.transport(daw::audio::transport::TransportCmd::Play);
        }
        if ui.button("⏹").clicked()
            && let Some(engine) = &mut eng.engine
        {
            engine.transport(daw::audio::transport::TransportCmd::Stop);
        }
        ui.monospace(format!("clip: {total_beats:.2} beats, loops forever"));
    });
    ui.horizontal(|ui| {
        ui.strong("subloop");
        let sub_label = if eng.sub_on { "on" } else { "off" };
        changed |= ui.toggle_value(&mut eng.sub_on, sub_label).changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut eng.sub_start)
                    .range(0..=15)
                    .prefix("from "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut eng.sub_end)
                    .range(1..=16)
                    .prefix("to "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut eng.sub_repeats)
                    .range(1..=8)
                    .suffix("x"),
            )
            .changed();
        if eng.sub_end <= eng.sub_start {
            ui.colored_label(
                egui::Color32::from_rgb(0xd0, 0x5f, 0x5f),
                "end must be past start",
            );
        }
    });
    ui.add_space(6.0);

    // The grid, painter-drawn: rows = pitches (top = high), cols = steps.
    let cell = egui::vec2(28.0, 14.0);
    let gap = 2.0;
    let size = egui::vec2(16.0 * (cell.x + gap), 25.0 * (cell.y + gap));
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter_at(rect);

    // Playhead column while looping: reverse-map the expanded timeline beat
    // back into a pattern step, so the highlight sits inside the subloop
    // region while its repeats play.
    let (ss, se, sr) = (
        eng.sub_start as f64 * 0.25,
        eng.sub_end as f64 * 0.25,
        eng.sub_repeats as f64,
    );
    let sub_active = eng.sub_on && eng.sub_end > eng.sub_start;
    let total_beats = if sub_active {
        4.0 + (sr - 1.0) * (se - ss)
    } else {
        4.0
    };
    let play_step: Option<usize> = eng.engine.as_mut().and_then(|e| {
        let b = e.latest_block();
        b.playing.then(|| {
            let beat = b.beat % total_beats.max(0.25);
            let pattern_beat = if !sub_active || beat < ss {
                beat
            } else if beat < ss + sr * (se - ss) {
                ss + (beat - ss) % (se - ss)
            } else {
                beat - (sr - 1.0) * (se - ss)
            };
            (pattern_beat / 0.25) as usize % 16
        })
    });

    let clicked = resp
        .clicked()
        .then(|| resp.interact_pointer_pos())
        .flatten();
    for row in 0..25 {
        for step in 0..16 {
            let min =
                rect.min + egui::vec2(step as f32 * (cell.x + gap), row as f32 * (cell.y + gap));
            let r = egui::Rect::from_min_size(min, cell);
            if let Some(pos) = clicked
                && r.contains(pos)
            {
                eng.pattern[row][step] = !eng.pattern[row][step];
                changed = true;
            }
            let on = eng.pattern[row][step];
            let beat_col = step % 4 == 0;
            let is_playhead = play_step == Some(step);
            let in_sub =
                sub_active && step >= eng.sub_start as usize && step < eng.sub_end as usize;
            let fill = if on && in_sub {
                egui::Color32::from_rgb(0x8a, 0xb3, 0x5f) // note inside subloop: warmer green
            } else if on {
                egui::Color32::from_rgb(0x5f, 0xb3, 0x8a)
            } else if is_playhead {
                ui.visuals().widgets.active.bg_fill
            } else if in_sub {
                ui.visuals().code_bg_color // region tint
            } else if beat_col {
                ui.visuals().faint_bg_color
            } else {
                ui.visuals().extreme_bg_color
            };
            p.rect_filled(r, 2.0, fill);
        }
    }

    if changed {
        eng.push_graph();
    }
    if play_step.is_some() {
        ui.ctx().request_repaint();
    }
}

/// A 2-second 48k drum-ish loop: four kick thumps and offbeat noise hats.
/// Exists so the audio-clip path is testable without hunting for files.
fn write_test_loop(path: &std::path::Path) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    let mut noise_state = 0x12345678u32;
    for i in 0..96_000u32 {
        let t = i as f32 / 48_000.0;
        let beat_t = t % 0.5; // 120bpm quarters
        // Kick: 60Hz sine with a fast pitch drop and 80ms decay.
        let kick = ((beat_t * (60.0 + 120.0 * (-beat_t * 30.0).exp())) * std::f32::consts::TAU)
            .sin()
            * (-beat_t * 12.0).exp();
        // Hat on the offbeat: white noise, 30ms decay.
        let hat_t = (t + 0.25) % 0.5;
        noise_state = noise_state.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = (noise_state >> 16) as f32 / 32_768.0 - 1.0;
        let hat = noise * 0.3 * (-hat_t * 33.0).exp();
        let v = (kick * 0.8 + hat).clamp(-1.0, 1.0);
        w.write_sample((v * i16::MAX as f32 * 0.8) as i16)?;
    }
    w.finalize()
}
