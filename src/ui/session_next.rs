//! Isolated Session View replacement.
//!
//! This module deliberately has no dependency on `main.rs`, the audio engine,
//! or the current Session implementation. It owns a serializable document, a
//! green-zone launch planner, runtime telemetry, exact layout geometry, and an
//! egui surface that returns intents. The eventual merge needs adapters at two
//! edges only:
//!
//! 1. project tracks/clips/scenes -> [`SessionDocument`];
//! 2. [`PendingLaunch`] -> the existing compiled-schedule swap path.
//!
//! It is not registered in `ui/mod.rs` while the live application is being
//! edited concurrently. `tests/session_next.rs` compiles and exercises it as a
//! standalone module.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_SCENES: usize = 8;
pub const MAX_SCENE_LOCKS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SceneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClipId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Midi,
    Audio,
}

impl TrackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI",
            Self::Audio => "AUDIO",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionTrack {
    pub id: TrackId,
    pub name: String,
    pub kind: TrackKind,
    pub mute: bool,
    pub solo: bool,
    pub volume: f32,
    pub pan: f32,
    /// The lane's live input, as a LABEL and a state — the view has no
    /// business knowing what a channel index means, only what to draw
    /// and what to ask for next.
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub monitoring: bool,
    /// How much of this track each return gets, in return order. Shorter
    /// than the return list means zero, exactly as the project's own
    /// send list does: adding a return must not have to write a silence
    /// into every track.
    #[serde(default)]
    pub sends: Vec<f32>,
}

impl Default for SessionTrack {
    fn default() -> Self {
        Self {
            id: TrackId(0),
            name: "Track".to_owned(),
            kind: TrackKind::Midi,
            mute: false,
            solo: false,
            volume: 1.0,
            pan: 0.0,
            input: "—".to_owned(),
            monitoring: false,
            sends: Vec::new(),
        }
    }
}

/// A return bus, as the mixer sees it.
///
/// The view's own shape rather than the project's `ReturnTrack`, for the
/// reason `SessionTrack` is not `Track`: this module may not name the
/// app. What it carries is what a strip draws — a chain is the rack's
/// business, not the mixer's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionReturn {
    pub name: String,
    pub mute: bool,
    pub volume: f32,
    pub pan: f32,
}

impl Default for SessionReturn {
    fn default() -> Self {
        Self {
            name: "Return".to_owned(),
            mute: false,
            volume: 1.0,
            pan: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EmptyBehavior {
    #[default]
    Stop,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LaunchMode {
    #[default]
    Trigger,
    Gate,
    Toggle,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillRule {
    #[default]
    Normal,
    Only,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LaunchCondition {
    #[default]
    Always,
    Probability(u8),
    Every {
        step: u8,
        total: u8,
    },
    First,
    NotFirst,
}

impl LaunchCondition {
    pub fn normalized(self) -> Self {
        match self {
            Self::Probability(percent) => Self::Probability(percent.clamp(1, 100)),
            Self::Every { step, total } => {
                let total = total.clamp(1, 64);
                Self::Every {
                    step: step.clamp(1, total),
                    total,
                }
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FollowAction {
    #[default]
    None,
    Stop,
    Replay,
    Next,
    Previous,
    First,
    Last,
    Random,
    Scene(SceneId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quantization {
    None,
    Sixteenth,
    Eighth,
    Quarter,
    Half,
    #[default]
    Bar,
    Bars2,
    Bars4,
}

impl Quantization {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::Sixteenth,
        Self::Eighth,
        Self::Quarter,
        Self::Half,
        Self::Bar,
        Self::Bars2,
        Self::Bars4,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Sixteenth => "1/16",
            Self::Eighth => "1/8",
            Self::Quarter => "1/4",
            Self::Half => "1/2",
            Self::Bar => "1 BAR",
            Self::Bars2 => "2 BARS",
            Self::Bars4 => "4 BARS",
        }
    }

    pub fn step_beats(self, beats_per_bar: u32) -> Option<f64> {
        match self {
            Self::None => None,
            Self::Sixteenth => Some(0.25),
            Self::Eighth => Some(0.5),
            Self::Quarter => Some(1.0),
            Self::Half => Some(2.0),
            Self::Bar => Some(f64::from(beats_per_bar)),
            Self::Bars2 => Some(f64::from(beats_per_bar) * 2.0),
            Self::Bars4 => Some(f64::from(beats_per_bar) * 4.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum QuantizationSetting {
    #[default]
    Global,
    Override(Quantization),
}

impl QuantizationSetting {
    pub fn resolve(self, global: Quantization) -> Quantization {
        match self {
            Self::Global => global,
            Self::Override(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSettings {
    pub mode: LaunchMode,
    pub quantization: QuantizationSetting,
    pub legato: bool,
    pub follow: FollowAction,
    pub follow_after_beats: f32,
    pub condition: LaunchCondition,
    pub fill: FillRule,
    /// What happens when a condition declines this clip.
    pub fallback: EmptyBehavior,
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            mode: LaunchMode::Trigger,
            quantization: QuantizationSetting::Global,
            legato: false,
            follow: FollowAction::None,
            follow_after_beats: 0.0,
            condition: LaunchCondition::Always,
            fill: FillRule::Normal,
            fallback: EmptyBehavior::Continue,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ClipPreview {
    #[default]
    None,
    /// Normalized `(start, length, pitch)` triples for a miniature density
    /// contour. The UI never reads the real note list.
    Notes(Vec<[f32; 3]>),
    /// Normalized min/max waveform pairs.
    Waveform(Vec<[f32; 2]>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionClip {
    pub id: ClipId,
    pub name: String,
    pub kind: TrackKind,
    pub length_beats: f32,
    pub loop_start_beats: f32,
    pub loop_length_beats: f32,
    pub active: bool,
    pub media_offline: bool,
    pub launch: LaunchSettings,
    pub preview: ClipPreview,
}

impl Default for SessionClip {
    fn default() -> Self {
        Self {
            id: ClipId(0),
            name: "Clip".to_owned(),
            kind: TrackKind::Midi,
            length_beats: 4.0,
            loop_start_beats: 0.0,
            loop_length_beats: 4.0,
            active: true,
            media_offline: false,
            launch: LaunchSettings::default(),
            preview: ClipPreview::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Slot {
    Empty(EmptyBehavior),
    Clip(SessionClip),
}

impl Default for Slot {
    fn default() -> Self {
        Self::Empty(EmptyBehavior::Stop)
    }
}

impl Slot {
    pub fn clip(&self) -> Option<&SessionClip> {
        match self {
            Self::Clip(clip) => Some(clip),
            Self::Empty(_) => None,
        }
    }

    pub fn empty_behavior(&self) -> Option<EmptyBehavior> {
        match self {
            Self::Empty(behavior) => Some(*behavior),
            Self::Clip(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneLock {
    pub track: TrackId,
    pub device: Option<DeviceId>,
    pub parameter: u32,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    /// `None` means no authored key; the theme supplies the neutral surface.
    pub color: Option<[u8; 3]>,
    pub tempo: Option<f64>,
    pub signature: Option<(u32, u32)>,
    pub quantization: QuantizationSetting,
    pub follow: FollowAction,
    pub locks: Vec<SceneLock>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            id: SceneId(0),
            name: "Scene".to_owned(),
            color: None,
            tempo: None,
            signature: None,
            quantization: QuantizationSetting::Global,
            follow: FollowAction::None,
            locks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryValue {
    pub track: TrackId,
    pub device: Option<DeviceId>,
    pub parameter: u32,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PerformanceMemory {
    pub values: Vec<MemoryValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionDocument {
    pub tracks: Vec<SessionTrack>,
    /// The return buses, in send order. Absent from a document written
    /// before returns existed, which reads as a song with none.
    #[serde(default)]
    pub returns: Vec<SessionReturn>,
    /// How many hardware inputs there are to route from, as the engine
    /// last reported. Zero means there is nothing to offer — the engine
    /// is off, or the interface has none — and the strip says so rather
    /// than cycling through routes that are all silence.
    #[serde(default)]
    pub input_channels: u32,
    pub scenes: Vec<Scene>,
    /// Track-major: `slots[track][scene]`.
    pub slots: Vec<Vec<Slot>>,
    pub global_quantization: Quantization,
    pub project_seed: u64,
    pub memories: [PerformanceMemory; 2],
    next_scene_id: u64,
}

impl Default for SessionDocument {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl SessionDocument {
    pub fn new(tracks: Vec<SessionTrack>) -> Self {
        let scenes: Vec<_> = (0..DEFAULT_SCENES)
            .map(|index| Scene {
                id: SceneId(index as u64 + 1),
                name: format!("Scene {}", index + 1),
                ..Scene::default()
            })
            .collect();
        let slots = vec![vec![Slot::default(); scenes.len()]; tracks.len()];
        Self {
            tracks,
            returns: Vec::new(),
            input_channels: 0,
            scenes,
            slots,
            global_quantization: Quantization::Bar,
            project_seed: 0x0053_4553_5349_4f4e,
            memories: [PerformanceMemory::default(), PerformanceMemory::default()],
            next_scene_id: DEFAULT_SCENES as u64 + 1,
        }
    }

    pub fn sanitize(&mut self) {
        if self.scenes.is_empty() {
            let id = SceneId(self.allocate_scene_id());
            self.scenes.push(Scene {
                id,
                name: "Scene 1".to_owned(),
                ..Scene::default()
            });
        }
        self.slots.resize_with(self.tracks.len(), || {
            vec![Slot::default(); self.scenes.len()]
        });
        for column in &mut self.slots {
            column.resize(self.scenes.len(), Slot::default());
            column.truncate(self.scenes.len());
        }
        self.slots.truncate(self.tracks.len());
        for scene in &mut self.scenes {
            scene.locks.truncate(MAX_SCENE_LOCKS);
            if let Some((top, unit)) = scene.signature {
                scene.signature = Some((top.clamp(1, 32), normalize_beat_unit(unit)));
            }
        }
        let max_id = self
            .scenes
            .iter()
            .map(|scene| scene.id.0)
            .max()
            .unwrap_or(0);
        self.next_scene_id = self.next_scene_id.max(max_id.saturating_add(1));
    }

    pub fn slot(&self, track: usize, scene: usize) -> Option<&Slot> {
        self.slots.get(track)?.get(scene)
    }

    pub fn slot_mut(&mut self, track: usize, scene: usize) -> Option<&mut Slot> {
        self.slots.get_mut(track)?.get_mut(scene)
    }

    pub fn insert_scene(&mut self, at: usize, mut scene: Scene, mut slots: Vec<Slot>) -> usize {
        let at = at.min(self.scenes.len());
        if self.scenes.iter().any(|known| known.id == scene.id) || scene.id.0 == 0 {
            scene.id = SceneId(self.allocate_scene_id());
        }
        scene.locks.truncate(MAX_SCENE_LOCKS);
        slots.resize(self.tracks.len(), Slot::default());
        self.scenes.insert(at, scene);
        for (track, column) in self.slots.iter_mut().enumerate() {
            column.insert(at, slots.get(track).cloned().unwrap_or_default());
        }
        at
    }

    pub fn remove_scene(&mut self, at: usize) -> Option<(Scene, Vec<Slot>)> {
        if self.scenes.len() <= 1 || at >= self.scenes.len() {
            return None;
        }
        let scene = self.scenes.remove(at);
        let slots = self
            .slots
            .iter_mut()
            .map(|column| column.remove(at))
            .collect();
        Some((scene, slots))
    }

    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.tracks.len() || to >= self.tracks.len() {
            return false;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        let slots = self.slots.remove(from);
        self.slots.insert(to, slots);
        true
    }

    pub fn move_scene(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.scenes.len() || to >= self.scenes.len() {
            return false;
        }
        let scene = self.scenes.remove(from);
        self.scenes.insert(to, scene);
        for column in &mut self.slots {
            let slot = column.remove(from);
            column.insert(to, slot);
        }
        true
    }

    pub fn scene_index(&self, id: SceneId) -> Option<usize> {
        self.scenes.iter().position(|scene| scene.id == id)
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|track| track.id == id)
    }

    fn allocate_scene_id(&mut self) -> u64 {
        let id = self.next_scene_id.max(1);
        self.next_scene_id = id.saturating_add(1);
        id
    }
}

fn normalize_beat_unit(unit: u32) -> u32 {
    const UNITS: [u32; 6] = [1, 2, 4, 8, 16, 32];
    UNITS
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(unit))
        .unwrap_or(4)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportClock {
    pub sample: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub beats_per_bar: u32,
}

impl TransportClock {
    pub fn beats_per_sample(self) -> f64 {
        if self.sample_rate == 0 || !self.bpm.is_finite() || self.bpm <= 0.0 {
            return 0.0;
        }
        self.bpm / (f64::from(self.sample_rate) * 60.0)
    }

    pub fn beat(self) -> f64 {
        self.sample as f64 * self.beats_per_sample()
    }

    pub fn sample_at_beat(self, beat: f64) -> u64 {
        let per_sample = self.beats_per_sample();
        if per_sample <= 0.0 || !beat.is_finite() || beat <= 0.0 {
            return 0;
        }
        (beat / per_sample).round().clamp(0.0, u64::MAX as f64) as u64
    }

    pub fn next_boundary(self, quantization: Quantization) -> u64 {
        let Some(step) = quantization.step_beats(self.beats_per_bar.max(1)) else {
            return self.sample;
        };
        let beat = self.beat();
        let scaled = beat / step;
        let nearest = scaled.round();
        let boundary_index = if (scaled - nearest).abs() <= 1e-9 {
            nearest
        } else {
            scaled.ceil()
        };
        self.sample_at_beat(boundary_index.max(0.0) * step)
            .max(self.sample)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    Arrangement,
    Stopped,
    Playing {
        scene: SceneId,
        clip: ClipId,
        started_at_sample: u64,
        cycle: u64,
    },
    Recording {
        scene: SceneId,
        started_at_sample: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingTrackAction {
    Start { scene: SceneId, clip: ClipId },
    Stop,
    Arrangement,
    Record { scene: SceneId },
    StopRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrack {
    pub transaction: u64,
    pub queued_at_sample: u64,
    pub at_sample: u64,
    pub scene: Option<SceneId>,
    pub action: PendingTrackAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackRuntime {
    pub playback: PlaybackState,
    pub pending: Option<PendingTrack>,
    /// `0..=1` within the current loop. Authoritative telemetry may replace
    /// this between control-side state transitions.
    pub phase: f32,
    pub peak: f32,
    pub clipped: bool,
}

impl Default for TrackRuntime {
    fn default() -> Self {
        Self {
            playback: PlaybackState::Arrangement,
            pending: None,
            phase: 0.0,
            peak: 0.0,
            clipped: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillState {
    #[default]
    Off,
    Momentary,
    Latched,
}

impl FillState {
    pub fn active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackLaunchOp {
    pub track: usize,
    pub action: PendingTrackAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingLaunch {
    pub id: u64,
    pub queued_at_sample: u64,
    pub at_sample: u64,
    /// Present only while the transaction still represents the complete
    /// scene. A later per-track action strips this identity.
    pub scene: Option<SceneId>,
    pub operations: Vec<TrackLaunchOp>,
    pub tempo: Option<f64>,
    pub signature: Option<(u32, u32)>,
    pub locks: Vec<SceneLock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueRefusal {
    MissingTrack,
    MissingScene,
    IncompatibleClip,
    NothingToLaunch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionRuntime {
    pub tracks: Vec<TrackRuntime>,
    /// Return meters, in return order. `TrackRuntime` reused for its
    /// level half — a return has no clip to be playing, so the rest of
    /// the shape simply sits at rest.
    pub returns: Vec<TrackRuntime>,
    pub pending: Vec<PendingLaunch>,
    pub active_scene: Option<SceneId>,
    pub fill: FillState,
    pub scene_cycles: BTreeMap<SceneId, u64>,
    pub late_launches: u64,
    next_transaction: u64,
}

impl SessionRuntime {
    pub fn new(track_count: usize) -> Self {
        Self {
            tracks: vec![TrackRuntime::default(); track_count],
            returns: Vec::new(),
            next_transaction: 1,
            ..Self::default()
        }
    }

    pub fn sanitize(&mut self, track_count: usize) {
        self.tracks.resize(track_count, TrackRuntime::default());
        self.tracks.truncate(track_count);
        self.pending.retain_mut(|launch| {
            launch.operations.retain(|op| op.track < track_count);
            !launch.operations.is_empty()
        });
        self.rebuild_pending_markers();
    }

    pub fn reset_on_discontinuity(&mut self) {
        self.pending.clear();
        self.scene_cycles.clear();
        self.active_scene = None;
        for track in &mut self.tracks {
            track.playback = PlaybackState::Stopped;
            track.pending = None;
            track.phase = 0.0;
        }
    }

    pub fn queue_clip(
        &mut self,
        document: &SessionDocument,
        track: usize,
        scene: usize,
        clock: TransportClock,
        direct: bool,
    ) -> Result<Option<PendingLaunch>, QueueRefusal> {
        let Some(track_doc) = document.tracks.get(track) else {
            return Err(QueueRefusal::MissingTrack);
        };
        let Some(scene_doc) = document.scenes.get(scene) else {
            return Err(QueueRefusal::MissingScene);
        };
        let Some(slot) = document.slot(track, scene) else {
            return Err(QueueRefusal::MissingTrack);
        };
        let cycle = if direct {
            self.scene_cycles.get(&scene_doc.id).copied().unwrap_or(0) + 1
        } else {
            let cycle = self.scene_cycles.entry(scene_doc.id).or_insert(0);
            *cycle = cycle.saturating_add(1);
            *cycle
        };
        if direct
            && slot
                .clip()
                .is_some_and(|clip| clip.launch.mode == LaunchMode::Toggle)
            && self
                .tracks
                .get(track)
                .is_some_and(|runtime| match (runtime.playback, slot) {
                    (PlaybackState::Playing { clip, .. }, Slot::Clip(slot_clip)) => {
                        clip == slot_clip.id
                    }
                    _ => false,
                })
        {
            let quantization = slot
                .clip()
                .map(|clip| clip.launch.quantization)
                .unwrap_or_default()
                .resolve(document.global_quantization);
            let launch = PendingLaunch {
                id: self.allocate_transaction(),
                queued_at_sample: clock.sample,
                at_sample: clock.next_boundary(quantization),
                scene: None,
                operations: vec![TrackLaunchOp {
                    track,
                    action: PendingTrackAction::Stop,
                }],
                tempo: None,
                signature: None,
                locks: Vec::new(),
            };
            self.install(launch.clone());
            return Ok(Some(launch));
        }
        let action = self.action_for_slot(
            slot,
            track_doc,
            scene_doc.id,
            track,
            cycle,
            document.project_seed,
            direct,
        )?;
        let Some(action) = action else {
            return Ok(None);
        };
        let quantization = slot
            .clip()
            .map(|clip| clip.launch.quantization)
            .unwrap_or(scene_doc.quantization)
            .resolve(document.global_quantization);
        let launch = PendingLaunch {
            id: self.allocate_transaction(),
            queued_at_sample: clock.sample,
            at_sample: clock.next_boundary(quantization),
            scene: None,
            operations: vec![TrackLaunchOp { track, action }],
            tempo: None,
            signature: None,
            locks: Vec::new(),
        };
        self.install(launch.clone());
        Ok(Some(launch))
    }

    /// Release a held Gate clip. Trigger/Toggle/Repeat releases do nothing.
    pub fn release_clip(
        &mut self,
        document: &SessionDocument,
        track: usize,
        scene: usize,
        clock: TransportClock,
    ) -> Result<Option<PendingLaunch>, QueueRefusal> {
        let Some(slot) = document.slot(track, scene) else {
            return Err(QueueRefusal::MissingTrack);
        };
        let Some(clip) = slot.clip() else {
            return Ok(None);
        };
        if clip.launch.mode != LaunchMode::Gate {
            return Ok(None);
        }
        self.queue_stop_track(document, track, clock, false)
            .map(Some)
    }

    /// Plan a clip's follow action on its own track. Conditions still apply;
    /// this is an automatic launch, not the user's direct press.
    pub fn queue_follow(
        &mut self,
        document: &SessionDocument,
        track: usize,
        scene: usize,
        clock: TransportClock,
    ) -> Result<Option<PendingLaunch>, QueueRefusal> {
        let Some(source_scene) = document.scenes.get(scene) else {
            return Err(QueueRefusal::MissingScene);
        };
        let Some(clip) = document.slot(track, scene).and_then(Slot::clip) else {
            return Err(QueueRefusal::NothingToLaunch);
        };
        let target = match clip.launch.follow {
            FollowAction::None => return Ok(None),
            FollowAction::Stop => {
                return self
                    .queue_stop_track(document, track, clock, false)
                    .map(Some);
            }
            FollowAction::Replay => scene,
            FollowAction::Next => (scene + 1).min(document.scenes.len().saturating_sub(1)),
            FollowAction::Previous => scene.saturating_sub(1),
            FollowAction::First => 0,
            FollowAction::Last => document.scenes.len().saturating_sub(1),
            FollowAction::Random => {
                if document.scenes.is_empty() {
                    return Err(QueueRefusal::MissingScene);
                }
                usize::from(deterministic_percent(
                    document.project_seed,
                    source_scene.id,
                    track,
                    self.scene_cycles
                        .get(&source_scene.id)
                        .copied()
                        .unwrap_or(0)
                        + 1,
                )) % document.scenes.len()
            }
            FollowAction::Scene(id) => {
                document.scene_index(id).ok_or(QueueRefusal::MissingScene)?
            }
        };
        self.queue_clip(document, track, target, clock, false)
    }

    pub fn queue_scene(
        &mut self,
        document: &SessionDocument,
        scene: usize,
        clock: TransportClock,
    ) -> Result<PendingLaunch, QueueRefusal> {
        let Some(scene_doc) = document.scenes.get(scene) else {
            return Err(QueueRefusal::MissingScene);
        };
        let cycle = self.scene_cycles.entry(scene_doc.id).or_insert(0);
        *cycle = cycle.saturating_add(1);
        let cycle = *cycle;
        let mut operations = Vec::with_capacity(document.tracks.len());
        for (track, track_doc) in document.tracks.iter().enumerate() {
            let Some(slot) = document.slot(track, scene) else {
                continue;
            };
            if let Some(action) = self.action_for_slot(
                slot,
                track_doc,
                scene_doc.id,
                track,
                cycle,
                document.project_seed,
                false,
            )? {
                operations.push(TrackLaunchOp { track, action });
            }
        }
        let quantization = scene_doc.quantization.resolve(document.global_quantization);
        let launch = PendingLaunch {
            id: self.allocate_transaction(),
            queued_at_sample: clock.sample,
            at_sample: clock.next_boundary(quantization),
            scene: Some(scene_doc.id),
            operations,
            tempo: scene_doc.tempo,
            signature: scene_doc.signature,
            locks: scene_doc
                .locks
                .iter()
                .take(MAX_SCENE_LOCKS)
                .cloned()
                .collect(),
        };
        self.install(launch.clone());
        Ok(launch)
    }

    pub fn queue_stop_track(
        &mut self,
        document: &SessionDocument,
        track: usize,
        clock: TransportClock,
        immediate: bool,
    ) -> Result<PendingLaunch, QueueRefusal> {
        if document.tracks.get(track).is_none() {
            return Err(QueueRefusal::MissingTrack);
        }
        let quantization = if immediate {
            Quantization::None
        } else {
            document.global_quantization
        };
        let launch = PendingLaunch {
            id: self.allocate_transaction(),
            queued_at_sample: clock.sample,
            at_sample: clock.next_boundary(quantization),
            scene: None,
            operations: vec![TrackLaunchOp {
                track,
                action: PendingTrackAction::Stop,
            }],
            tempo: None,
            signature: None,
            locks: Vec::new(),
        };
        self.install(launch.clone());
        Ok(launch)
    }

    pub fn queue_stop_all(
        &mut self,
        document: &SessionDocument,
        clock: TransportClock,
        arrangement: bool,
    ) -> PendingLaunch {
        let action = if arrangement {
            PendingTrackAction::Arrangement
        } else {
            PendingTrackAction::Stop
        };
        let launch = PendingLaunch {
            id: self.allocate_transaction(),
            queued_at_sample: clock.sample,
            at_sample: clock.next_boundary(document.global_quantization),
            scene: None,
            operations: (0..document.tracks.len())
                .map(|track| TrackLaunchOp { track, action })
                .collect(),
            tempo: None,
            signature: None,
            locks: Vec::new(),
        };
        self.install(launch.clone());
        launch
    }

    pub fn apply_due(&mut self, through_sample: u64) -> Vec<PendingLaunch> {
        self.pending
            .sort_by_key(|launch| (launch.at_sample, launch.id));
        let due_count = self
            .pending
            .iter()
            .take_while(|launch| launch.at_sample <= through_sample)
            .count();
        let due: Vec<_> = self.pending.drain(..due_count).collect();
        for launch in &due {
            for operation in &launch.operations {
                let Some(track) = self.tracks.get_mut(operation.track) else {
                    continue;
                };
                track.playback = match operation.action {
                    PendingTrackAction::Start { scene, clip } => PlaybackState::Playing {
                        scene,
                        clip,
                        started_at_sample: launch.at_sample,
                        cycle: 0,
                    },
                    PendingTrackAction::Stop | PendingTrackAction::StopRecording => {
                        PlaybackState::Stopped
                    }
                    PendingTrackAction::Arrangement => PlaybackState::Arrangement,
                    PendingTrackAction::Record { scene } => PlaybackState::Recording {
                        scene,
                        started_at_sample: launch.at_sample,
                    },
                };
                track.phase = 0.0;
                if track
                    .pending
                    .is_some_and(|pending| pending.transaction == launch.id)
                {
                    track.pending = None;
                }
            }
        }
        self.rebuild_pending_markers();
        self.active_scene = common_playing_scene(&self.tracks);
        due
    }

    pub fn cancel_transaction(&mut self, id: u64) -> bool {
        let old = self.pending.len();
        self.pending.retain(|launch| launch.id != id);
        let changed = old != self.pending.len();
        if changed {
            self.rebuild_pending_markers();
        }
        changed
    }

    #[allow(clippy::too_many_arguments)]
    fn action_for_slot(
        &self,
        slot: &Slot,
        track: &SessionTrack,
        scene: SceneId,
        track_index: usize,
        cycle: u64,
        seed: u64,
        direct: bool,
    ) -> Result<Option<PendingTrackAction>, QueueRefusal> {
        match slot {
            Slot::Empty(EmptyBehavior::Stop) => Ok(Some(PendingTrackAction::Stop)),
            Slot::Empty(EmptyBehavior::Continue) => Ok(None),
            Slot::Clip(clip) => {
                if clip.kind != track.kind {
                    return Err(QueueRefusal::IncompatibleClip);
                }
                if !clip.active {
                    return Ok(None);
                }
                let accepted = direct
                    || (fill_accepts(clip.launch.fill, self.fill)
                        && condition_accepts(
                            clip.launch.condition.normalized(),
                            seed,
                            scene,
                            track_index,
                            cycle,
                        ));
                if accepted {
                    Ok(Some(PendingTrackAction::Start {
                        scene,
                        clip: clip.id,
                    }))
                } else {
                    Ok(match clip.launch.fallback {
                        EmptyBehavior::Stop => Some(PendingTrackAction::Stop),
                        EmptyBehavior::Continue => None,
                    })
                }
            }
        }
    }

    fn allocate_transaction(&mut self) -> u64 {
        let id = self.next_transaction.max(1);
        self.next_transaction = id.saturating_add(1);
        id
    }

    fn install(&mut self, mut launch: PendingLaunch) {
        let affected: Vec<usize> = launch.operations.iter().map(|op| op.track).collect();
        for old in &mut self.pending {
            let before = old.operations.len();
            old.operations.retain(|op| !affected.contains(&op.track));
            if old.operations.len() != before {
                old.scene = None;
            }
        }
        self.pending.retain(|old| !old.operations.is_empty());
        collapse_operations(&mut launch.operations);
        self.pending.push(launch);
        self.rebuild_pending_markers();
    }

    fn rebuild_pending_markers(&mut self) {
        for track in &mut self.tracks {
            track.pending = None;
        }
        for launch in &self.pending {
            for operation in &launch.operations {
                if let Some(track) = self.tracks.get_mut(operation.track) {
                    track.pending = Some(PendingTrack {
                        transaction: launch.id,
                        queued_at_sample: launch.queued_at_sample,
                        at_sample: launch.at_sample,
                        scene: launch.scene,
                        action: operation.action,
                    });
                }
            }
        }
    }
}

fn common_playing_scene(tracks: &[TrackRuntime]) -> Option<SceneId> {
    let mut scene = None;
    let mut heard = false;
    for track in tracks {
        match track.playback {
            PlaybackState::Playing { scene: current, .. }
            | PlaybackState::Recording { scene: current, .. } => {
                heard = true;
                if scene.is_some_and(|known| known != current) {
                    return None;
                }
                scene = Some(current);
            }
            PlaybackState::Arrangement | PlaybackState::Stopped => {}
        }
    }
    heard.then_some(scene).flatten()
}

/// One operation per track, in track order.
///
/// STOP LOSES TO START on the same boundary, and that is the sequencing
/// contract's tie rule wearing Session clothes: two things landing on one
/// sample are ordered stop-then-start, so what remains afterwards is the
/// start. A track left stopped because a stop happened to be pushed first
/// is the vanishing-note bug with a different name — the launch you
/// pressed does nothing, once, unreproducibly.
///
/// Sorting alone would not do it: `sort_by_key` is stable, so equal tracks
/// keep insertion order and the winner would be whichever the caller
/// happened to build first.
fn collapse_operations(operations: &mut Vec<TrackLaunchOp>) {
    operations.sort_by_key(|op| (op.track, starts_sound(&op.action)));
    // With starts-last inside each track, the LAST of a run is the one to
    // keep — so the list is reversed, deduped (which keeps the first of
    // each run), and turned back.
    operations.reverse();
    operations.dedup_by_key(|op| op.track);
    operations.reverse();
}

/// Does this action leave the track making sound? Stops and returns do
/// not; starts and records do.
fn starts_sound(action: &PendingTrackAction) -> bool {
    matches!(
        action,
        PendingTrackAction::Start { .. } | PendingTrackAction::Record { .. }
    )
}

fn fill_accepts(rule: FillRule, fill: FillState) -> bool {
    match rule {
        FillRule::Normal => true,
        FillRule::Only => fill.active(),
        FillRule::Not => !fill.active(),
    }
}

fn condition_accepts(
    condition: LaunchCondition,
    seed: u64,
    scene: SceneId,
    track: usize,
    cycle: u64,
) -> bool {
    match condition {
        LaunchCondition::Always => true,
        LaunchCondition::Probability(percent) => {
            deterministic_percent(seed, scene, track, cycle) < percent
        }
        LaunchCondition::Every { step, total } => {
            let step = u64::from(step.max(1));
            let total = u64::from(total.max(1));
            (cycle.saturating_sub(1) % total) + 1 == step.min(total)
        }
        LaunchCondition::First => cycle == 1,
        LaunchCondition::NotFirst => cycle > 1,
    }
}

fn deterministic_percent(seed: u64, scene: SceneId, track: usize, cycle: u64) -> u8 {
    let mut value =
        seed ^ scene.0.rotate_left(17) ^ (track as u64).rotate_left(31) ^ cycle.rotate_left(47);
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value ^ (value >> 31)) % 100) as u8
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneClipboard {
    pub scene: Scene,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionClipboard {
    pub lifted: Option<SceneClipboard>,
}

impl SessionClipboard {
    pub fn lift(
        &mut self,
        document: &SessionDocument,
        runtime: &SessionRuntime,
        name: impl Into<String>,
    ) -> bool {
        if document.tracks.is_empty() {
            return false;
        }
        let mut slots = Vec::with_capacity(document.tracks.len());
        for (track, state) in runtime.tracks.iter().enumerate() {
            let slot = match state.playback {
                PlaybackState::Playing { scene, .. } | PlaybackState::Recording { scene, .. } => {
                    document
                        .scene_index(scene)
                        .and_then(|scene| document.slot(track, scene))
                        .cloned()
                        .unwrap_or_default()
                }
                PlaybackState::Arrangement => Slot::Empty(EmptyBehavior::Continue),
                PlaybackState::Stopped => Slot::Empty(EmptyBehavior::Stop),
            };
            slots.push(slot);
        }
        self.lifted = Some(SceneClipboard {
            scene: Scene {
                id: SceneId(0),
                name: name.into(),
                ..Scene::default()
            },
            slots,
        });
        true
    }

    pub fn drop_into(&self, document: &mut SessionDocument, below: usize) -> Option<usize> {
        let lifted = self.lifted.clone()?;
        Some(document.insert_scene(below.saturating_add(1), lifted.scene, lifted.slots))
    }
}

// -------------------------------------------------------------------------
// View state, geometry, and intents

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionDensity {
    #[default]
    Comfortable,
    Compact,
}

impl SessionDensity {
    fn slot_height(self) -> f32 {
        match self {
            Self::Comfortable => 36.0,
            Self::Compact => 28.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSelection {
    Track(usize),
    Slot { track: usize, scene: usize },
    Scene(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotDragState {
    from: (usize, usize),
    copy: bool,
    target: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackDragState {
    from: usize,
    target: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneDragState {
    from: usize,
    target: usize,
}

#[derive(Debug, Clone)]
pub struct SessionViewState {
    pub selection: Option<GridSelection>,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub mixer_height: f32,
    pub density: SessionDensity,
    pub select_on_launch: bool,
    pub owns_keyboard: bool,
    pub memory_morph: f32,
    /// Which return's devices the rack is showing, if a return's head
    /// was clicked. View state, like every other selection here.
    pub selected_return: Option<usize>,
    /// Peak hold per track, in linear amplitude.
    ///
    /// VIEW state, deliberately: a hold answers "how loud did that get
    /// while I was not looking", which is a question about the reader,
    /// not about the signal. The engine reports an instantaneous peak
    /// and has no opinion about how long a human needs to read one.
    peak_hold: Vec<f32>,
    slot_drag: Option<SlotDragState>,
    track_drag: Option<TrackDragState>,
    scene_drag: Option<SceneDragState>,
}

impl SessionViewState {
    /// Fold this frame's peak into the hold and hand back the held value.
    fn hold_peak(&mut self, track: usize, peak: f32) -> f32 {
        if self.peak_hold.len() <= track {
            self.peak_hold.resize(track + 1, 0.0);
        }
        let hold = &mut self.peak_hold[track];
        *hold = hold.max(peak.max(0.0));
        *hold
    }

    /// Forget one track's hold. The reader has read it.
    fn clear_peak(&mut self, track: usize) {
        if let Some(hold) = self.peak_hold.get_mut(track) {
            *hold = 0.0;
        }
    }
}

impl Default for SessionViewState {
    fn default() -> Self {
        Self {
            selection: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            mixer_height: 156.0,
            density: SessionDensity::Comfortable,
            select_on_launch: true,
            owns_keyboard: false,
            memory_morph: 0.5,
            selected_return: None,
            peak_hold: Vec::new(),
            slot_drag: None,
            track_drag: None,
            scene_drag: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionIntent {
    SelectTrack(usize),
    SelectSlot {
        track: usize,
        scene: usize,
    },
    SelectScene(usize),
    LaunchSlot {
        track: usize,
        scene: usize,
    },
    LaunchScene(usize),
    StopTrack {
        track: usize,
        immediate: bool,
    },
    StopAll,
    BackToArrangement,
    CreateMidiClip {
        track: usize,
        scene: usize,
    },
    MoveSlots {
        from: (usize, usize),
        to: (usize, usize),
        copy: bool,
    },
    ReorderTrack {
        from: usize,
        to: usize,
    },
    ReorderScene {
        from: usize,
        to: usize,
    },
    ClearSelection,
    DeleteSelection,
    InsertSceneBelow(usize),
    CaptureScene,
    Lift,
    Drop,
    SetGlobalQuantization(Quantization),
    SetFill(FillState),
    RecallMemory {
        index: usize,
        pressed: bool,
    },
    MorphMemories(f32),
    ToggleTrackMute(usize),
    ToggleTrackSolo(usize),
    /// Solo this track and NOTHING else — and un-solo it if it was
    /// already the only one. Live's plain solo click, kept apart from
    /// `ToggleTrackSolo` so the additive one stays reachable on Ctrl.
    SoloTrackExclusive(usize),
    SetTrackPan {
        track: usize,
        value: f32,
    },
    SetTrackVolume {
        track: usize,
        value: f32,
    },
    ClearClipHold(usize),
    SetTrackSend {
        track: usize,
        index: usize,
        value: f32,
    },
    /// Step this track's input route. `back` walks the cycle the other
    /// way, which is what makes a cycle usable rather than a hunt.
    CycleTrackInput {
        track: usize,
        back: bool,
    },
    ToggleTrackMonitor(usize),
    SelectReturn(usize),
    ToggleReturnMute(usize),
    SetReturnVolume {
        index: usize,
        value: f32,
    },
    SetReturnPan {
        index: usize,
        value: f32,
    },
}

#[derive(Debug, Clone)]
pub struct SessionColors {
    pub bg: egui::Color32,
    pub surface: egui::Color32,
    pub raised: egui::Color32,
    pub sunken: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub divider: egui::Color32,
    pub outline: egui::Color32,
    pub focus: egui::Color32,
    pub accent: egui::Color32,
    pub accent_dim: egui::Color32,
    pub midi: egui::Color32,
    pub audio: egui::Color32,
    pub selected: egui::Color32,
    pub ok: egui::Color32,
    pub warn: egui::Color32,
    pub danger: egui::Color32,
    pub role_time: egui::Color32,
    pub role_level: egui::Color32,
    pub role_shape: egui::Color32,
    pub role_mod: egui::Color32,
    pub meter_low: egui::Color32,
    pub meter_hot: egui::Color32,
    pub meter_clip: egui::Color32,
}

impl Default for SessionColors {
    fn default() -> Self {
        Self {
            bg: egui::Color32::from_rgb(0x15, 0x14, 0x12),
            surface: egui::Color32::from_rgb(0x1d, 0x1b, 0x18),
            raised: egui::Color32::from_rgb(0x2a, 0x27, 0x22),
            sunken: egui::Color32::from_rgb(0x0d, 0x0c, 0x0b),
            text: egui::Color32::from_rgb(0xe4, 0xe9, 0xed),
            muted: egui::Color32::from_rgb(0x9b, 0xa8, 0xb1),
            divider: egui::Color32::from_rgb(0x35, 0x30, 0x2a),
            outline: egui::Color32::from_rgb(0x4a, 0x43, 0x3a),
            focus: egui::Color32::from_rgb(0x5a, 0xb5, 0xd2),
            accent: egui::Color32::from_rgb(0x5a, 0xb5, 0xd2),
            accent_dim: egui::Color32::from_rgb(0x35, 0x62, 0x71),
            midi: egui::Color32::from_rgb(0x4b, 0x40, 0x34),
            audio: egui::Color32::from_rgb(0x31, 0x47, 0x51),
            selected: egui::Color32::from_rgb(0xd0, 0xb2, 0x8c),
            ok: egui::Color32::from_rgb(0x6c, 0xc2, 0x72),
            warn: egui::Color32::from_rgb(0xdc, 0xae, 0x67),
            danger: egui::Color32::from_rgb(0xde, 0x70, 0x70),
            role_time: egui::Color32::from_rgb(0x62, 0x91, 0xb9),
            role_level: egui::Color32::from_rgb(0xc8, 0xa6, 0x7a),
            role_shape: egui::Color32::from_rgb(0xdd, 0xdf, 0xd4),
            role_mod: egui::Color32::from_rgb(0xf0, 0x54, 0x3d),
            meter_low: egui::Color32::from_rgb(0x63, 0xc5, 0x96),
            meter_hot: egui::Color32::from_rgb(0xdc, 0xae, 0x67),
            meter_clip: egui::Color32::from_rgb(0xde, 0x70, 0x70),
        }
    }
}

pub const CONTROL_HEIGHT: f32 = 28.0;
pub const HEADER_HEIGHT: f32 = 36.0;
pub const SLOT_GAP: f32 = 2.0;
pub const LAUNCH_WIDTH: f32 = 24.0;
pub const TRACK_WIDTH_DEFAULT: f32 = 112.0;
pub const TRACK_WIDTH_MIN: f32 = 84.0;
pub const TRACK_WIDTH_MAX: f32 = 184.0;
pub const SCENE_WIDTH: f32 = 176.0;
pub const MIXER_HEIGHT_MIN: f32 = 64.0;
pub const MIXER_HEIGHT_MAX: f32 = 320.0;
pub const SCROLLBAR_HEIGHT: f32 = 7.0;
const MIXER_SEAM_HEIGHT: f32 = 6.0;
const TRACK_BUTTON_HEIGHT: f32 = 20.0;
const TRACK_BUTTON_WIDTH: f32 = 24.0;
const FADER_WIDTH: f32 = 24.0;

/// The strip's own paddings and row heights.
///
/// Named rather than written into the layout, because every one of them
/// is also a term in the height budget `MixerStrip::new` walks: a number
/// that appears in both the budget and the placement has to be the same
/// number, and the only way to be sure of that is for there to be one.
const STRIP_PAD_X: f32 = 6.0;
const STRIP_PAD_Y: f32 = 6.0;
const STRIP_GAP: f32 = 4.0;
const PAN_HEIGHT: f32 = 13.0;
const READOUT_HEIGHT: f32 = 13.0;
const PEAK_HEIGHT: f32 = 11.0;
const FADER_MIN_HEIGHT: f32 = 26.0;
const METER_WIDTH: f32 = 9.0;
const SCALE_MIN_WIDTH: f32 = 20.0;
const SEND_HEIGHT: f32 = 11.0;
const IO_HEIGHT: f32 = 13.0;
const MONITOR_WIDTH: f32 = 20.0;
const SEND_LETTER_WIDTH: f32 = 9.0;
/// Where return peak holds live in the strip's hold table.
///
/// Far past any lane count, so a return's hold and a track's cannot
/// collide however many tracks a project grows. The table is a `Vec`
/// that grows on demand, so the gap costs a few zeroed floats and buys
/// an index space that needs no bookkeeping.
const RETURN_HOLD_BASE: usize = 4096;
/// The fader's top. A console's fader runs a little past unity so a mix
/// can be pushed as well as pulled; +6 dB is where this one stops.
const MAX_FADER_GAIN: f32 = 2.0;
/// What `Shift` multiplies a drag by. Not a snap and not a mode: the
/// same gesture, moving less.
const FINE_DRAG: f32 = 0.2;
const PHASE_HEIGHT: f32 = 2.0;
const STATE_EDGE: f32 = 2.0;
const PREVIEW_HEIGHT: f32 = 12.0;
const MICRO_FONT: f32 = 9.0;
const LABEL_FONT: f32 = 10.0;
const BODY_FONT: f32 = 11.0;
const REORDER_GRIP_WIDTH: f32 = 14.0;

#[derive(Debug, Clone, Copy)]
pub struct SessionLayout {
    pub area: egui::Rect,
    pub track_width: f32,
    pub slot_height: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub mixer_height: f32,
    tracks: usize,
    /// Return columns, drawn after the tracks in the MIXER BAND only.
    ///
    /// A return has no clip slots, exactly as it has none in Live: the
    /// grid above a return column is empty ground, and `slot_at` never
    /// answers with one because it only ever walks the tracks.
    returns: usize,
    scenes: usize,
}

impl SessionLayout {
    pub fn new(
        area: egui::Rect,
        tracks: usize,
        returns: usize,
        scenes: usize,
        state: &SessionViewState,
    ) -> Self {
        let lane_width = (area.width() - SCENE_WIDTH).max(0.0);
        // Columns, not tracks: a return takes a column's width in the
        // mixer band, so it has to be one of the shares the width is cut
        // into or the returns would sit off the right edge.
        let columns = tracks + returns;
        let track_width = if columns == 0 {
            TRACK_WIDTH_DEFAULT
        } else {
            (lane_width / columns as f32).clamp(TRACK_WIDTH_MIN, TRACK_WIDTH_MAX)
        };
        let mixer_cap = (area.height() * 0.55).clamp(MIXER_HEIGHT_MIN, MIXER_HEIGHT_MAX);
        let mut layout = Self {
            area,
            track_width,
            slot_height: state.density.slot_height(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            mixer_height: state.mixer_height.clamp(MIXER_HEIGHT_MIN, mixer_cap),
            tracks,
            returns,
            scenes,
        };
        layout.scroll_x = state.scroll_x.clamp(0.0, layout.max_scroll_x());
        layout.scroll_y = state.scroll_y.clamp(0.0, layout.max_scroll_y());
        layout
    }

    pub fn control_strip(self) -> egui::Rect {
        egui::Rect::from_min_max(
            self.area.min,
            egui::pos2(self.area.right(), self.area.top() + CONTROL_HEIGHT),
        )
    }

    pub fn tracks_viewport(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.area.left(), self.area.top() + CONTROL_HEIGHT),
            egui::pos2(self.area.right() - SCENE_WIDTH, self.area.bottom()),
        )
    }

    pub fn scene_column(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(
                self.area.right() - SCENE_WIDTH,
                self.area.top() + CONTROL_HEIGHT,
            ),
            self.area.max,
        )
    }

    pub fn rows_viewport(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(
                self.area.left(),
                self.area.top() + CONTROL_HEIGHT + HEADER_HEIGHT,
            ),
            egui::pos2(self.area.right() - SCENE_WIDTH, self.mixer_top()),
        )
    }

    pub fn scene_rows_viewport(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(
                self.area.right() - SCENE_WIDTH,
                self.area.top() + CONTROL_HEIGHT + HEADER_HEIGHT,
            ),
            self.area.max,
        )
    }

    pub fn mixer_top(self) -> f32 {
        self.area.bottom() - SCROLLBAR_HEIGHT - self.mixer_height
    }

    pub fn mixer_seam(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.area.left(), self.mixer_top() - MIXER_SEAM_HEIGHT * 0.5),
            egui::pos2(
                self.area.right() - SCENE_WIDTH,
                self.mixer_top() + MIXER_SEAM_HEIGHT * 0.5,
            ),
        )
    }

    pub fn header(self, track: usize) -> egui::Rect {
        let left = self.track_left(track);
        egui::Rect::from_min_max(
            egui::pos2(left, self.area.top() + CONTROL_HEIGHT),
            egui::pos2(
                left + self.track_width,
                self.area.top() + CONTROL_HEIGHT + HEADER_HEIGHT,
            ),
        )
    }

    pub fn track_grip(self, track: usize) -> egui::Rect {
        let header = self.header(track);
        egui::Rect::from_min_max(
            egui::pos2(header.right() - REORDER_GRIP_WIDTH, header.top()),
            header.max,
        )
    }

    pub fn slot(self, track: usize, scene: usize) -> egui::Rect {
        let left = self.track_left(track) + SLOT_GAP;
        let top = self.area.top() + CONTROL_HEIGHT + HEADER_HEIGHT - self.scroll_y
            + scene as f32 * (self.slot_height + SLOT_GAP);
        egui::Rect::from_min_size(
            egui::pos2(left, top),
            egui::vec2(self.track_width - SLOT_GAP * 2.0, self.slot_height),
        )
    }

    pub fn slot_launch(self, track: usize, scene: usize) -> egui::Rect {
        let slot = self.slot(track, scene);
        egui::Rect::from_min_max(
            slot.min,
            egui::pos2(slot.left() + LAUNCH_WIDTH, slot.bottom()),
        )
    }

    pub fn slot_body(self, track: usize, scene: usize) -> egui::Rect {
        let slot = self.slot(track, scene);
        egui::Rect::from_min_max(egui::pos2(slot.left() + LAUNCH_WIDTH, slot.top()), slot.max)
    }

    pub fn scene(self, scene: usize) -> egui::Rect {
        let top = self.area.top() + CONTROL_HEIGHT + HEADER_HEIGHT - self.scroll_y
            + scene as f32 * (self.slot_height + SLOT_GAP);
        egui::Rect::from_min_size(
            egui::pos2(self.area.right() - SCENE_WIDTH + SLOT_GAP, top),
            egui::vec2(SCENE_WIDTH - SLOT_GAP * 2.0, self.slot_height),
        )
    }

    pub fn scene_launch(self, scene: usize) -> egui::Rect {
        let row = self.scene(scene);
        egui::Rect::from_min_max(row.min, egui::pos2(row.left() + LAUNCH_WIDTH, row.bottom()))
    }

    pub fn scene_body(self, scene: usize) -> egui::Rect {
        let row = self.scene(scene);
        egui::Rect::from_min_max(
            egui::pos2(row.left() + LAUNCH_WIDTH, row.top()),
            egui::pos2(row.right() - REORDER_GRIP_WIDTH, row.bottom()),
        )
    }

    pub fn scene_grip(self, scene: usize) -> egui::Rect {
        let row = self.scene(scene);
        egui::Rect::from_min_max(
            egui::pos2(row.right() - REORDER_GRIP_WIDTH, row.top()),
            row.max,
        )
    }

    pub fn mixer(self, track: usize) -> egui::Rect {
        let left = self.track_left(track);
        egui::Rect::from_min_max(
            egui::pos2(left, self.mixer_top()),
            egui::pos2(
                left + self.track_width,
                self.area.bottom() - SCROLLBAR_HEIGHT,
            ),
        )
    }

    /// One return's strip, in the mixer band after the last track.
    pub fn return_mixer(self, index: usize) -> egui::Rect {
        self.mixer(self.tracks + index)
    }

    /// The seam between the last track's strip and the first return's —
    /// drawn, never interactive, so the eye knows where the mix ends and
    /// what it is sent to begins.
    pub fn return_divider(self) -> Option<egui::Rect> {
        (self.returns > 0).then(|| {
            let left = self.track_left(self.tracks);
            egui::Rect::from_min_max(
                egui::pos2(left - 1.0, self.mixer_top()),
                egui::pos2(left + 1.0, self.area.bottom() - SCROLLBAR_HEIGHT),
            )
        })
    }

    pub fn stop_track(self, track: usize) -> egui::Rect {
        let row = self.scenes;
        self.slot(track, row)
    }

    pub fn stop_all(self) -> egui::Rect {
        self.scene(self.scenes)
    }

    pub fn add_scene(self) -> egui::Rect {
        self.scene(self.scenes + 1)
    }

    pub fn max_scroll_x(self) -> f32 {
        // The return columns count: a mixer you cannot scroll to is a
        // mixer that does not have them.
        ((self.tracks + self.returns) as f32 * self.track_width - self.tracks_viewport().width())
            .max(0.0)
    }

    pub fn max_scroll_y(self) -> f32 {
        let rows = self.scenes + 2;
        let content = rows as f32 * (self.slot_height + SLOT_GAP);
        (content - self.rows_viewport().height()).max(0.0)
    }

    pub fn slot_at(self, position: egui::Pos2) -> Option<(usize, usize)> {
        if !self.rows_viewport().contains(position) {
            return None;
        }
        for track in 0..self.tracks {
            for scene in 0..self.scenes {
                if self.slot(track, scene).contains(position) {
                    return Some((track, scene));
                }
            }
        }
        None
    }

    fn track_left(self, track: usize) -> f32 {
        self.area.left() - self.scroll_x + track as f32 * self.track_width
    }
}

#[derive(Debug, Clone)]
pub struct SessionViewOutput {
    pub intents: Vec<SessionIntent>,
    pub layout: SessionLayout,
}

/// Draw the complete replacement Session surface and return user wishes.
///
/// This function does not mutate musical content or runtime state. UI-local
/// selection, scroll, drag ownership, and density live in `state`; every
/// engine- or project-facing change leaves as a [`SessionIntent`].
pub fn show_session(
    ui: &mut egui::Ui,
    document: &SessionDocument,
    runtime: &SessionRuntime,
    clock: TransportClock,
    clipboard: &SessionClipboard,
    state: &mut SessionViewState,
    colors: &SessionColors,
) -> SessionViewOutput {
    let area = ui.max_rect();
    let mut layout = SessionLayout::new(
        area,
        document.tracks.len(),
        document.returns.len(),
        document.scenes.len(),
        state,
    );
    let mut intents = Vec::new();

    ui.painter().rect_filled(area, 0.0, colors.bg);

    if area.contains(ui.ctx().pointer_latest_pos().unwrap_or(egui::Pos2::ZERO)) {
        let scroll = ui.input(|input| input.smooth_scroll_delta);
        if scroll != egui::Vec2::ZERO {
            let horizontal = ui.input(|input| input.modifiers.shift);
            if horizontal || scroll.x.abs() > scroll.y.abs() {
                let delta = if scroll.x != 0.0 { scroll.x } else { scroll.y };
                state.scroll_x = (state.scroll_x - delta).clamp(0.0, layout.max_scroll_x());
            } else {
                state.scroll_y = (state.scroll_y - scroll.y).clamp(0.0, layout.max_scroll_y());
            }
            layout = SessionLayout::new(
                area,
                document.tracks.len(),
                document.returns.len(),
                document.scenes.len(),
                state,
            );
        }
    }

    paint_control_strip(
        ui,
        document,
        runtime,
        clipboard,
        state,
        colors,
        layout,
        &mut intents,
    );

    let seam = layout.mixer_seam();
    let seam_response = ui.interact(
        seam,
        ui.id().with("session_next_mixer_seam"),
        egui::Sense::drag(),
    );
    if seam_response.dragged()
        && let Some(position) = seam_response.interact_pointer_pos()
    {
        state.mixer_height = (area.bottom() - SCROLLBAR_HEIGHT - position.y)
            .clamp(MIXER_HEIGHT_MIN, MIXER_HEIGHT_MAX);
        layout = SessionLayout::new(
            area,
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            state,
        );
    }

    let track_clip = layout.tracks_viewport();
    let row_clip = layout.rows_viewport();
    for (track_index, track) in document.tracks.iter().enumerate() {
        let header = layout.header(track_index).intersect(track_clip);
        if header.width() > 1.0 {
            paint_track_header(
                ui,
                track,
                track_index,
                runtime.tracks.get(track_index),
                state,
                colors,
                header,
                &mut intents,
            );
        }
        for scene_index in 0..document.scenes.len() {
            let rect = layout.slot(track_index, scene_index);
            if rect.bottom() < row_clip.top() {
                continue;
            }
            if rect.top() > row_clip.bottom() {
                break;
            }
            let clipped = rect.intersect(row_clip).intersect(track_clip);
            if clipped.width() <= 1.0 || clipped.height() <= 1.0 {
                continue;
            }
            let Some(slot) = document.slot(track_index, scene_index) else {
                continue;
            };
            paint_slot(
                ui,
                slot,
                track.kind,
                track_index,
                scene_index,
                document.scenes[scene_index].id,
                runtime.tracks.get(track_index),
                clock,
                state,
                colors,
                layout,
                row_clip.intersect(track_clip),
                &mut intents,
            );
        }
        let stop = layout
            .stop_track(track_index)
            .intersect(row_clip)
            .intersect(track_clip);
        if stop.width() > 1.0 && stop.height() > 1.0 {
            paint_stop_track(ui, track_index, stop, colors, &mut intents);
        }
        let mixer = layout.mixer(track_index).intersect(track_clip);
        if mixer.width() > 1.0 && mixer.height() > 1.0 {
            paint_mixer(
                ui,
                track,
                track_index,
                &document.returns,
                document.input_channels,
                runtime.tracks.get(track_index),
                state,
                mixer,
                colors,
                &mut intents,
            );
        }
    }

    // --- the returns, in the mixer band after the last track.
    //
    // A return column has no clip slots — the grid above it is drawn
    // ground and nothing else, which is exactly where Live puts one. The
    // divider before them says where the mix stops and what it is sent
    // to begins.
    if let Some(divider) = layout.return_divider() {
        ui.painter().rect_filled(divider, 0.0, colors.outline);
    }
    for (index, bus) in document.returns.iter().enumerate() {
        let strip = layout
            .return_mixer(index)
            .intersect(layout.tracks_viewport());
        if strip.width() <= 1.0 || strip.height() <= 1.0 {
            continue;
        }
        paint_return_strip(
            ui,
            bus,
            index,
            runtime.returns.get(index),
            state,
            strip,
            state.selected_return == Some(index),
            colors,
            &mut intents,
        );
    }

    paint_scene_column(ui, document, runtime, state, colors, layout, &mut intents);
    paint_scrollbar(ui, state, colors, layout);

    settle_slot_drag(ui, document, state, layout, &mut intents);
    settle_track_drag(ui, state, colors, layout, &mut intents);
    settle_scene_drag(ui, state, colors, layout, &mut intents);
    keyboard_intents(ui, document, runtime, state, &mut intents);

    SessionViewOutput { intents, layout }
}

#[allow(clippy::too_many_arguments)]
fn paint_control_strip(
    ui: &mut egui::Ui,
    document: &SessionDocument,
    runtime: &SessionRuntime,
    clipboard: &SessionClipboard,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
    intents: &mut Vec<SessionIntent>,
) {
    let strip = layout.control_strip();
    ui.painter().rect_filled(strip, 0.0, colors.surface);
    ui.painter().line_segment(
        [strip.left_bottom(), strip.right_bottom()],
        egui::Stroke::new(1.0, colors.divider),
    );
    let mut left = strip.left() + 4.0;
    let top = strip.top() + 4.0;
    let height = strip.height() - 8.0;

    let quantize_rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(76.0, height));
    let quantize = control_button(
        ui,
        quantize_rect,
        ui.id().with("session_next_quantize"),
        &format!("Q  {}", document.global_quantization.label()),
        false,
        colors.role_time,
        colors,
    );
    if quantize.clicked() {
        let at = Quantization::ALL
            .iter()
            .position(|value| *value == document.global_quantization)
            .unwrap_or(0);
        intents.push(SessionIntent::SetGlobalQuantization(
            Quantization::ALL[(at + 1) % Quantization::ALL.len()],
        ));
    }
    left = quantize_rect.right() + 4.0;

    let fill_rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(48.0, height));
    let fill_id = ui.id().with("session_next_fill");
    let fill = control_button(
        ui,
        fill_rect,
        fill_id,
        "FILL",
        runtime.fill.active(),
        colors.role_mod,
        colors,
    );
    if fill.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_pressed()) {
        let latched = ui.input(|input| input.modifiers.shift);
        intents.push(SessionIntent::SetFill(if latched {
            match runtime.fill {
                FillState::Latched => FillState::Off,
                FillState::Off | FillState::Momentary => FillState::Latched,
            }
        } else {
            FillState::Momentary
        }));
    }
    if ui.input(|input| input.pointer.primary_released())
        && runtime.fill == FillState::Momentary
        && !ui.input(|input| input.modifiers.shift)
    {
        intents.push(SessionIntent::SetFill(FillState::Off));
    }
    left = fill_rect.right() + 8.0;

    for (label, width, intent, enabled) in [
        ("CAP", 42.0, SessionIntent::CaptureScene, true),
        ("LIFT", 48.0, SessionIntent::Lift, true),
        (
            "DROP",
            48.0,
            SessionIntent::Drop,
            clipboard.lifted.is_some(),
        ),
    ] {
        let rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height));
        let response = control_button(
            ui,
            rect,
            ui.id().with(("session_next_control", label)),
            label,
            false,
            colors.role_shape,
            colors,
        );
        if response.clicked() && enabled {
            intents.push(intent);
        }
        if !enabled {
            ui.painter()
                .rect_filled(rect, 0.0, colors.bg.gamma_multiply(0.55));
        }
        left = rect.right() + 4.0;
    }

    let memo_width = 36.0;
    let gap = 4.0;
    let right = strip.right() - 4.0;
    let m2_rect = egui::Rect::from_min_size(
        egui::pos2(right - memo_width, top),
        egui::vec2(memo_width, height),
    );
    let m1_rect = m2_rect.translate(egui::vec2(-(memo_width + gap), 0.0));
    for (index, rect, label) in [(0, m1_rect, "M1"), (1, m2_rect, "M2")] {
        let id = ui.id().with(("session_next_memory", index));
        let held = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
        let response = control_button(ui, rect, id, label, held, colors.role_level, colors);
        if response.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_pressed())
        {
            ui.data_mut(|data| data.insert_temp(id, true));
            intents.push(SessionIntent::RecallMemory {
                index,
                pressed: true,
            });
        }
        if held && ui.input(|input| input.pointer.primary_released()) {
            ui.data_mut(|data| data.insert_temp(id, false));
            intents.push(SessionIntent::RecallMemory {
                index,
                pressed: false,
            });
        }
    }

    let morph_rect = egui::Rect::from_min_max(
        egui::pos2(m1_rect.left() - 74.0, top),
        egui::pos2(m1_rect.left() - gap, top + height),
    );
    let morph_id = ui.id().with("session_next_memory_morph");
    let morph = ui.interact(morph_rect, morph_id, egui::Sense::click_and_drag());
    ui.painter().line_segment(
        [morph_rect.left_center(), morph_rect.right_center()],
        egui::Stroke::new(1.0, colors.divider),
    );
    if (morph.dragged() || morph.clicked() || morph.is_pointer_button_down_on())
        && let Some(position) = morph.interact_pointer_pos()
    {
        state.memory_morph =
            ((position.x - morph_rect.left()) / morph_rect.width()).clamp(0.0, 1.0);
        intents.push(SessionIntent::MorphMemories(state.memory_morph));
    }
    ui.painter().circle_filled(
        egui::pos2(
            egui::lerp(morph_rect.left()..=morph_rect.right(), state.memory_morph),
            morph_rect.center().y,
        ),
        3.0,
        if morph.hovered() || morph.dragged() {
            colors.role_level
        } else {
            colors.outline
        },
    );
}

fn control_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    label: &str,
    active: bool,
    role: egui::Color32,
    colors: &SessionColors,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let fill = if active || response.is_pointer_button_down_on() {
        role.gamma_multiply(0.78)
    } else if response.hovered() {
        colors.raised
    } else {
        colors.sunken
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, if active { role } else { colors.divider }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(LABEL_FONT),
        if active { colors.bg } else { colors.muted },
    );
    response
}

#[allow(clippy::too_many_arguments)]
fn paint_track_header(
    ui: &mut egui::Ui,
    track: &SessionTrack,
    track_index: usize,
    runtime: Option<&TrackRuntime>,
    state: &mut SessionViewState,
    colors: &SessionColors,
    rect: egui::Rect,
    intents: &mut Vec<SessionIntent>,
) {
    let grip_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - REORDER_GRIP_WIDTH, rect.top()),
        rect.max,
    );
    let body_rect = egui::Rect::from_min_max(rect.min, egui::pos2(grip_rect.left(), rect.bottom()));
    let id = ui.id().with(("session_next_track", track.id.0));
    let response = ui.interact(body_rect, id, egui::Sense::click());
    if response.clicked() {
        state.selection = Some(GridSelection::Track(track_index));
        state.owns_keyboard = true;
        intents.push(SessionIntent::SelectTrack(track_index));
    }
    let selected = state.selection == Some(GridSelection::Track(track_index));
    ui.painter().rect_filled(
        rect,
        0.0,
        if selected || response.hovered() {
            colors.raised
        } else {
            colors.surface
        },
    );
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, colors.divider),
    );
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(STATE_EDGE, rect.height())),
            0.0,
            colors.focus,
        );
    }
    if runtime.is_some_and(|track| track.pending.is_some()) {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STATE_EDGE)),
            0.0,
            colors.warn,
        );
    }
    let text_rect = rect.shrink2(egui::vec2(6.0, 4.0));
    ui.painter().text(
        text_rect.left_top(),
        egui::Align2::LEFT_TOP,
        &track.name,
        egui::FontId::proportional(BODY_FONT),
        colors.text,
    );
    ui.painter().text(
        text_rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        track.kind.label(),
        egui::FontId::monospace(MICRO_FONT),
        colors.muted,
    );
    let authority = runtime.map_or("ARR", |runtime| match runtime.playback {
        PlaybackState::Arrangement => "ARR",
        PlaybackState::Stopped => "STOP",
        PlaybackState::Playing { .. } => "PLAY",
        PlaybackState::Recording { .. } => "REC",
    });
    ui.painter().text(
        text_rect.right_bottom(),
        egui::Align2::RIGHT_BOTTOM,
        authority,
        egui::FontId::monospace(MICRO_FONT),
        colors.muted,
    );
    let grip_id = ui.id().with(("session_next_track_grip", track.id.0));
    let grip = ui.interact(grip_rect, grip_id, egui::Sense::drag());
    if grip.drag_started() {
        state.track_drag = Some(TrackDragState {
            from: track_index,
            target: track_index,
        });
    }
    let grip_color = if grip.hovered() || grip.dragged() {
        colors.accent
    } else {
        colors.divider
    };
    for offset in [-3.0, 0.0, 3.0] {
        ui.painter().circle_filled(
            egui::pos2(grip_rect.center().x, grip_rect.center().y + offset),
            1.0,
            grip_color,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_slot(
    ui: &mut egui::Ui,
    slot: &Slot,
    track_kind: TrackKind,
    track: usize,
    scene: usize,
    scene_id: SceneId,
    runtime: Option<&TrackRuntime>,
    clock: TransportClock,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
    clip_rect: egui::Rect,
    intents: &mut Vec<SessionIntent>,
) {
    let full = layout.slot(track, scene);
    let launch_rect = layout.slot_launch(track, scene).intersect(clip_rect);
    let body_rect = layout.slot_body(track, scene).intersect(clip_rect);
    let selected = state.selection == Some(GridSelection::Slot { track, scene });
    // Clip ids are globally stable; row indices are deliberately never
    // treated as stable scene identities just to answer this visual question.
    let playing = runtime.is_some_and(|track_runtime| match (track_runtime.playback, slot) {
        (PlaybackState::Playing { clip, .. }, Slot::Clip(slot_clip)) => clip == slot_clip.id,
        _ => false,
    });
    let recording = runtime.is_some_and(|track_runtime| {
        matches!(
            track_runtime.playback,
            PlaybackState::Recording {
                scene: recording_scene,
                ..
            } if recording_scene == scene_id
        )
    });
    let pending = runtime.and_then(|track_runtime| track_runtime.pending);
    let queued = pending.is_some_and(|pending| match (pending.action, slot) {
        (PendingTrackAction::Start { clip, .. }, Slot::Clip(slot_clip)) => clip == slot_clip.id,
        (PendingTrackAction::Stop, Slot::Empty(EmptyBehavior::Stop)) => {
            pending.scene == Some(scene_id)
        }
        _ => false,
    });

    let base = match slot {
        Slot::Clip(clip) if clip.media_offline => colors.sunken,
        Slot::Clip(clip) => match clip.kind {
            TrackKind::Midi => colors.midi,
            TrackKind::Audio => colors.audio,
        },
        Slot::Empty(_) => colors.bg,
    };
    ui.painter()
        .rect_filled(full.intersect(clip_rect), 0.0, base);
    if playing || recording {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(full.left_top(), egui::vec2(STATE_EDGE, full.height()))
                .intersect(clip_rect),
            0.0,
            if recording { colors.danger } else { colors.ok },
        );
    }
    if queued {
        let remaining = pending.map_or(0, |pending| pending.at_sample.saturating_sub(clock.sample));
        let budget = pending.map_or(1, |pending| {
            pending
                .at_sample
                .saturating_sub(pending.queued_at_sample)
                .max(1)
        });
        let fraction = (remaining as f32 / budget as f32).clamp(0.0, 1.0);
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                full.left_top(),
                egui::vec2(full.width() * fraction.max(0.04), STATE_EDGE),
            )
            .intersect(clip_rect),
            0.0,
            colors.warn,
        );
    }
    if selected {
        ui.painter().rect_stroke(
            full.shrink(1.0),
            0.0,
            egui::Stroke::new(2.0, colors.selected),
            egui::StrokeKind::Inside,
        );
    }

    // Body first, launch rail second. Their rectangles do not overlap, but
    // the order makes foreground ownership explicit if geometry changes.
    let body_id = ui.id().with(("session_next_slot_body", track, scene));
    let body = ui.interact(body_rect, body_id, egui::Sense::click_and_drag());
    if body.clicked() {
        state.selection = Some(GridSelection::Slot { track, scene });
        state.owns_keyboard = true;
        intents.push(SessionIntent::SelectSlot { track, scene });
    }
    if body.double_clicked() && matches!(slot, Slot::Empty(_)) && track_kind == TrackKind::Midi {
        intents.push(SessionIntent::CreateMidiClip { track, scene });
    }
    if body.drag_started() && matches!(slot, Slot::Clip(_)) {
        state.slot_drag = Some(SlotDragState {
            from: (track, scene),
            copy: ui.input(|input| input.modifiers.ctrl || input.modifiers.command),
            target: None,
        });
    }

    let launch_id = ui.id().with(("session_next_slot_launch", track, scene));
    let launch = ui.interact(launch_rect, launch_id, egui::Sense::click());
    if launch.clicked() {
        match slot {
            Slot::Clip(_) => intents.push(SessionIntent::LaunchSlot { track, scene }),
            Slot::Empty(EmptyBehavior::Stop) => intents.push(SessionIntent::StopTrack {
                track,
                immediate: ui.input(|input| input.modifiers.shift),
            }),
            Slot::Empty(EmptyBehavior::Continue) => {}
        }
        if state.select_on_launch {
            state.selection = Some(GridSelection::Slot { track, scene });
        }
        state.owns_keyboard = true;
    }
    paint_launch_glyph(
        ui.painter(),
        launch_rect,
        slot,
        playing,
        recording,
        queued,
        launch.hovered(),
        colors,
    );

    if let Slot::Clip(clip) = slot {
        paint_clip_content(ui.painter(), body_rect, clip, state.density, colors);
        if playing {
            let phase = runtime.map_or(0.0, |runtime| runtime.phase.clamp(0.0, 1.0));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(full.left(), full.bottom() - PHASE_HEIGHT),
                    egui::vec2(full.width() * phase, PHASE_HEIGHT),
                )
                .intersect(clip_rect),
                0.0,
                colors.ok,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_launch_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    slot: &Slot,
    playing: bool,
    recording: bool,
    queued: bool,
    hovered: bool,
    colors: &SessionColors,
) {
    if hovered {
        painter.rect_filled(rect, 0.0, colors.raised.gamma_multiply(0.7));
    }
    let color = if recording {
        colors.danger
    } else if queued {
        colors.warn
    } else if playing {
        colors.ok
    } else {
        colors.muted
    };
    let center = rect.center();
    match slot {
        Slot::Clip(_) if recording => {
            painter.circle_filled(center, 4.0, color);
        }
        Slot::Clip(_) => {
            let points = vec![
                egui::pos2(center.x - 3.0, center.y - 5.0),
                egui::pos2(center.x + 5.0, center.y),
                egui::pos2(center.x - 3.0, center.y + 5.0),
            ];
            if playing {
                painter.add(egui::Shape::convex_polygon(
                    points,
                    color,
                    egui::Stroke::NONE,
                ));
            } else {
                painter.add(egui::Shape::closed_line(
                    points,
                    egui::Stroke::new(1.25, color),
                ));
            }
        }
        Slot::Empty(EmptyBehavior::Stop) => {
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::vec2(7.0, 7.0)),
                0.0,
                color,
            );
        }
        Slot::Empty(EmptyBehavior::Continue) => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.0, center.y),
                    egui::pos2(center.x + 4.0, center.y),
                ],
                egui::Stroke::new(1.0, colors.divider),
            );
        }
    }
}

fn paint_clip_content(
    painter: &egui::Painter,
    rect: egui::Rect,
    clip: &SessionClip,
    density: SessionDensity,
    colors: &SessionColors,
) {
    let inner = rect.shrink2(egui::vec2(5.0, 3.0));
    let text_color = if clip.media_offline {
        colors.danger
    } else if clip.active {
        colors.text
    } else {
        colors.muted
    };
    painter.with_clip_rect(inner).text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        if clip.media_offline {
            format!("{}  MEDIA OFFLINE", clip.name)
        } else {
            clip.name.clone()
        },
        egui::FontId::proportional(BODY_FONT),
        text_color,
    );
    if density == SessionDensity::Comfortable {
        let preview = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - PREVIEW_HEIGHT),
            inner.max,
        );
        match &clip.preview {
            ClipPreview::None => {}
            ClipPreview::Notes(notes) => {
                for note in notes.iter().take(64) {
                    let start = note[0].clamp(0.0, 1.0);
                    let length = note[1].clamp(0.0, 1.0 - start);
                    let pitch = note[2].clamp(0.0, 1.0);
                    let y = egui::lerp(preview.bottom()..=preview.top(), pitch);
                    painter.line_segment(
                        [
                            egui::pos2(egui::lerp(preview.left()..=preview.right(), start), y),
                            egui::pos2(
                                egui::lerp(preview.left()..=preview.right(), start + length),
                                y,
                            ),
                        ],
                        egui::Stroke::new(1.0, colors.text.gamma_multiply(0.45)),
                    );
                }
            }
            ClipPreview::Waveform(points) => {
                if points.len() > 1 {
                    for (index, point) in points.iter().enumerate() {
                        let x = egui::lerp(
                            preview.left()..=preview.right(),
                            index as f32 / (points.len() - 1) as f32,
                        );
                        painter.line_segment(
                            [
                                egui::pos2(
                                    x,
                                    preview.center().y
                                        - point[1].clamp(-1.0, 1.0) * preview.height() * 0.5,
                                ),
                                egui::pos2(
                                    x,
                                    preview.center().y
                                        - point[0].clamp(-1.0, 1.0) * preview.height() * 0.5,
                                ),
                            ],
                            egui::Stroke::new(1.0, colors.text.gamma_multiply(0.38)),
                        );
                    }
                }
            }
        }
    }
    paint_rule_badges(painter, inner, &clip.launch, colors);
}

fn paint_rule_badges(
    painter: &egui::Painter,
    rect: egui::Rect,
    launch: &LaunchSettings,
    colors: &SessionColors,
) {
    let mut labels = Vec::new();
    match launch.fill {
        FillRule::Only => labels.push("F".to_owned()),
        FillRule::Not => labels.push("F̸".to_owned()),
        FillRule::Normal => {}
    }
    match launch.condition.normalized() {
        LaunchCondition::Always => {}
        LaunchCondition::Probability(percent) => labels.push(format!("{percent}%")),
        LaunchCondition::Every { step, total } => labels.push(format!("{step}:{total}")),
        LaunchCondition::First => labels.push("1ST".to_owned()),
        LaunchCondition::NotFirst => labels.push("¬1".to_owned()),
    }
    if launch.follow != FollowAction::None {
        labels.push("→".to_owned());
    }
    if launch.legato {
        labels.push("∞".to_owned());
    }
    if launch.mode != LaunchMode::Trigger {
        labels.push(
            match launch.mode {
                LaunchMode::Gate => "G",
                LaunchMode::Toggle => "T",
                LaunchMode::Repeat => "R",
                LaunchMode::Trigger => "",
            }
            .to_owned(),
        );
    }
    if !labels.is_empty() {
        painter.text(
            rect.right_top(),
            egui::Align2::RIGHT_TOP,
            labels.join("  "),
            egui::FontId::monospace(MICRO_FONT),
            colors.muted,
        );
    }
}

fn paint_stop_track(
    ui: &mut egui::Ui,
    track: usize,
    rect: egui::Rect,
    colors: &SessionColors,
    intents: &mut Vec<SessionIntent>,
) {
    let id = ui.id().with(("session_next_stop_track", track));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked() {
        intents.push(SessionIntent::StopTrack {
            track,
            immediate: ui.input(|input| input.modifiers.shift),
        });
    }
    ui.painter().rect_filled(
        rect,
        0.0,
        if response.hovered() {
            colors.raised
        } else {
            colors.bg
        },
    );
    ui.painter().rect_filled(
        egui::Rect::from_center_size(rect.center(), egui::vec2(7.0, 7.0)),
        0.0,
        colors.muted,
    );
}

// ------------------------------------------------------------ the strip ---

/// Where everything in one mixer strip sits.
///
/// Laid out ONCE, here, so the painter and the pointer tests read the
/// same answer. The strip this replaced computed its pan bar inline and
/// its test recomputed the same arithmetic by hand — two sources for one
/// rectangle, which is exactly how a control drifts out from under its
/// own test without either side noticing.
///
/// The `Option` rows are the height budget made explicit. The strip can
/// be dragged down to `MIXER_HEIGHT_MIN`, and at that size something has
/// to go; what goes is decided here rather than by each painter
/// separately guessing whether it has room.
///
/// The ranking is STRICT, and it costs a few pixels on purpose: a row
/// refused takes every cheaper row below it down too, even where one
/// would have fitted. Spending the gap instead would make the budget
/// non-monotonic — the volume number appearing, then vanishing again as
/// the pan bar it is ranked under finally affords itself — and a row
/// that flickers off while a seam is dragged reads as a rendering fault,
/// not as a budget. Predictable beats full.
///
/// The sends are the exception that proves it. They are a VARIABLE block,
/// so they only begin once every fixed row is already there — from which
/// point the block can only grow, because nothing above it can still
/// arrive and take its space back.
/// What a strip has been asked to hold, beside its fixed furniture.
///
/// A struct and not two more positional arguments: `new(rect, 2, true)`
/// reads as nothing at a call site, and both of these decide whether a
/// ROW EXISTS, which is the thing a reader of the layout most needs to
/// see named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripContent {
    /// How many sends this strip can show — one per return.
    pub sends: usize,
    /// Whether this lane can be routed at all. Audio lanes can; a note
    /// lane has no input path, so it is offered no control rather than a
    /// dead one.
    pub io: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixerStrip {
    pub rect: egui::Rect,
    pub mute: egui::Rect,
    pub solo: egui::Rect,
    pub pan: Option<egui::Rect>,
    /// The monitor switch and the route it names. Both or neither: a
    /// route you cannot hear and a monitor with nothing to hear are each
    /// half a control.
    pub monitor: Option<egui::Rect>,
    pub route: Option<egui::Rect>,
    /// One row per send that fits, in send order.
    pub sends: Vec<egui::Rect>,
    /// How many sends did not fit. Drawn as a count, never silently
    /// dropped: a send you cannot see is still sending.
    pub sends_hidden: usize,
    pub fader: egui::Rect,
    /// The meter column, BESIDE the fader rather than behind it. A meter
    /// under a fader cap is a meter you cannot read at the one moment it
    /// matters, which is while your hand is on the fader.
    pub meter: egui::Rect,
    /// The dB ticks between fader and meter. `None` on a narrow strip.
    pub scale: Option<egui::Rect>,
    pub readout: Option<egui::Rect>,
    pub peak: Option<egui::Rect>,
}

impl MixerStrip {
    pub fn new(rect: egui::Rect, content: StripContent) -> Self {
        let StripContent { sends, io } = content;
        let content_left = rect.left() + STRIP_PAD_X;
        let content_right = (rect.right() - STRIP_PAD_X).max(content_left + 8.0);

        // What fits, decided worst-first. The order is the order a
        // reader can afford to lose things in: the peak number is a
        // luxury, the volume readout is a convenience, the pan bar is a
        // control, and the fader with its two buttons IS the strip.
        let mut spare =
            rect.height() - STRIP_PAD_Y * 2.0 - TRACK_BUTTON_HEIGHT - STRIP_GAP - FADER_MIN_HEIGHT;
        let mut afford = |cost: f32| {
            if spare >= cost {
                spare -= cost;
                true
            } else {
                false
            }
        };
        let wants_pan = afford(PAN_HEIGHT + STRIP_GAP);
        let wants_io = io && wants_pan && afford(IO_HEIGHT + STRIP_GAP);
        let wants_readout = wants_pan && afford(READOUT_HEIGHT + 2.0);
        let wants_peak = wants_readout && afford(PEAK_HEIGHT + 1.0);
        // Only once everything fixed is in place, for the reason above.
        let send_rows = if wants_peak && sends > 0 {
            (((spare - STRIP_GAP) / SEND_HEIGHT).floor().max(0.0) as usize).min(sends)
        } else {
            0
        };

        let row_width = TRACK_BUTTON_WIDTH * 2.0 + 4.0;
        let mut top = rect.top() + STRIP_PAD_Y;
        let mute = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - row_width * 0.5, top),
            egui::vec2(TRACK_BUTTON_WIDTH, TRACK_BUTTON_HEIGHT),
        );
        let solo = mute.translate(egui::vec2(TRACK_BUTTON_WIDTH + 4.0, 0.0));
        top += TRACK_BUTTON_HEIGHT + STRIP_GAP;
        // The I/O row sits at the TOP, under the buttons, which is where
        // a console puts it and where a signal actually enters: read the
        // strip downwards and you read the path.
        let (monitor, route) = if wants_io {
            let row = egui::Rect::from_min_max(
                egui::pos2(content_left, top),
                egui::pos2(content_right, top + IO_HEIGHT),
            );
            top += IO_HEIGHT + STRIP_GAP;
            let split = (row.left() + MONITOR_WIDTH).min(row.right());
            (
                Some(egui::Rect::from_min_max(
                    row.min,
                    egui::pos2(split, row.bottom()),
                )),
                Some(egui::Rect::from_min_max(
                    egui::pos2(split + 2.0, row.top()),
                    row.max,
                )),
            )
        } else {
            (None, None)
        };
        let pan = wants_pan.then(|| {
            let bar = egui::Rect::from_min_max(
                egui::pos2(content_left, top),
                egui::pos2(content_right, top + PAN_HEIGHT),
            );
            top += PAN_HEIGHT + STRIP_GAP;
            bar
        });
        let send_rects: Vec<egui::Rect> = (0..send_rows)
            .map(|row| {
                egui::Rect::from_min_max(
                    egui::pos2(content_left, top + row as f32 * SEND_HEIGHT),
                    egui::pos2(content_right, top + (row + 1) as f32 * SEND_HEIGHT - 1.0),
                )
            })
            .collect();
        if send_rows > 0 {
            top += send_rows as f32 * SEND_HEIGHT + STRIP_GAP;
        }

        let mut bottom = rect.bottom() - STRIP_PAD_Y;
        let peak = wants_peak.then(|| {
            let row = egui::Rect::from_min_max(
                egui::pos2(content_left, bottom - PEAK_HEIGHT),
                egui::pos2(content_right, bottom),
            );
            bottom -= PEAK_HEIGHT + 1.0;
            row
        });
        let readout = wants_readout.then(|| {
            let row = egui::Rect::from_min_max(
                egui::pos2(content_left, bottom - READOUT_HEIGHT),
                egui::pos2(content_right, bottom),
            );
            bottom -= READOUT_HEIGHT + 2.0;
            row
        });

        let band_bottom = bottom.max(top + FADER_MIN_HEIGHT);
        let meter = egui::Rect::from_min_max(
            egui::pos2(content_right - METER_WIDTH, top),
            egui::pos2(content_right, band_bottom),
        );
        let fader = egui::Rect::from_min_max(
            egui::pos2(content_left, top),
            egui::pos2(
                (content_left + FADER_WIDTH)
                    .min(meter.left() - 2.0)
                    .max(content_left + 6.0),
                band_bottom,
            ),
        );
        let ticks_left = fader.right() + 3.0;
        let ticks_right = meter.left() - 3.0;
        let scale = (ticks_right - ticks_left >= SCALE_MIN_WIDTH).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(ticks_left, top),
                egui::pos2(ticks_right, band_bottom),
            )
        });

        Self {
            rect,
            mute,
            solo,
            pan,
            monitor,
            route,
            sends: send_rects,
            sends_hidden: sends - send_rows,
            fader,
            meter,
            scale,
            readout,
            peak,
        }
    }
}

/// The fader's taper, both directions.
///
/// Squared, which is the curve this strip has always used. It lives here
/// as a PAIR so the handle, the ticks, the meter and the gesture cannot
/// drift into disagreeing about where 0 dB is — they all ask this.
fn fader_normalized(volume: f32) -> f32 {
    (volume.clamp(0.0, MAX_FADER_GAIN) / MAX_FADER_GAIN).sqrt()
}

fn fader_volume(normalized: f32) -> f32 {
    normalized.clamp(0.0, 1.0).powi(2) * MAX_FADER_GAIN
}

/// Where a decibel value sits on the fader, `0..=1`.
fn fader_position(db: f32) -> f32 {
    fader_normalized(10.0_f32.powf(db / 20.0))
}

/// A send's taper. The same square law as the fader, over the send's own
/// range — which stops at unity, because a send is how much of a track
/// goes somewhere, and more than all of it is not a quantity.
fn send_normalized(level: f32) -> f32 {
    level.clamp(0.0, 1.0).sqrt()
}

fn send_level(normalized: f32) -> f32 {
    normalized.clamp(0.0, 1.0).powi(2)
}

/// The ticks drawn beside the fader, loud first.
///
/// Chosen for READING rather than for even spacing: 0 is where the mix
/// was built, ±6 are the moves a hand makes without thinking, and the
/// long tail down to −48 exists so the bottom of the fader is not an
/// unmarked void.
const FADER_TICKS: &[(f32, &str)] = &[
    (6.0, "+6"),
    (0.0, "0"),
    (-6.0, "-6"),
    (-12.0, "-12"),
    (-24.0, "-24"),
    (-48.0, "-48"),
];

/// A gain as decibels, or the floor. No unit — the caller knows.
fn db_text(gain: f32) -> String {
    if gain <= 1e-5 {
        "-inf".to_owned()
    } else {
        format!("{:+.1}", 20.0 * gain.log10())
    }
}

/// Pan, said the way a console says it: `50L`, `C`, `12R`.
fn pan_text(pan: f32) -> String {
    let amount = (pan.clamp(-1.0, 1.0) * 50.0).round() as i32;
    match amount {
        0 => "C".to_owned(),
        amount if amount < 0 => format!("{}L", -amount),
        amount => format!("{amount}R"),
    }
}

/// A drag distance, scaled by whether the fine modifier is held.
///
/// Fine dragging is why every control here moves RELATIVELY rather than
/// jumping to the pointer. An absolute control has no room for a gain —
/// wherever the pointer is IS the value — so `Shift` could not mean
/// anything on one, however much a mix wants a tenth of a dB.
fn fine_drag(ui: &egui::Ui, amount: f32) -> f32 {
    if ui.input(|input| input.modifiers.shift) {
        amount * FINE_DRAG
    } else {
        amount
    }
}

/// The pan bar: a fill that GROWS FROM THE CENTRE, which is the shape
/// that says at a glance both how far and which way. A dot on a line
/// says only where, and a mix is read at a glance or not at all.
///
/// Returns the pan it was moved to, if it was moved.
fn pan_control(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    pan: f32,
    colors: &SessionColors,
) -> Option<f32> {
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let moved = if response.double_clicked() {
        Some(0.0)
    } else if response.dragged() {
        let amount = fine_drag(ui, response.drag_delta().x);
        (amount != 0.0).then(|| (pan + amount / rect.width().max(1.0)).clamp(-1.0, 1.0))
    } else {
        None
    };
    let live = response.hovered() || response.dragged();

    ui.painter().rect_filled(rect, 0.0, colors.sunken);
    let centre = rect.center().x;
    let pan = pan.clamp(-1.0, 1.0);
    if pan.abs() > 1e-4 {
        let edge = centre + pan * rect.width() * 0.5;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(centre.min(edge), rect.top() + 2.0),
                egui::pos2(centre.max(edge), rect.bottom() - 2.0),
            ),
            0.0,
            if live {
                colors.accent
            } else {
                colors.accent_dim
            },
        );
    }
    ui.painter().line_segment(
        [
            egui::pos2(centre, rect.top()),
            egui::pos2(centre, rect.bottom()),
        ],
        egui::Stroke::new(1.0, colors.outline),
    );
    ui.painter().text(
        egui::pos2(rect.right() - 2.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        pan_text(pan),
        egui::FontId::monospace(MICRO_FONT - 1.0),
        if live { colors.text } else { colors.muted },
    );
    response.on_hover_text(format!(
        "pan {} · drag to move · shift for fine · double-click centres",
        pan_text(pan)
    ));
    moved
}

/// What a level column was asked to do this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct LevelEdit {
    /// The volume it was dragged to, as linear gain.
    volume: Option<f32>,
    /// The peak hold was clicked and should be forgotten.
    clear_peak: bool,
}

/// The fader, its scale, its meter and its two numbers — the half of a
/// strip a track and a return have IDENTICALLY.
///
/// Shared rather than copied, and the reason is the taper: a return whose
/// fader read 0 dB two pixels from where a track's did would be a mixer
/// nobody could balance by eye. One function, one rail.
#[allow(clippy::too_many_arguments)]
fn paint_level_column(
    ui: &mut egui::Ui,
    strip: &MixerStrip,
    id: egui::Id,
    volume: f32,
    peak: f32,
    hold: f32,
    clipped: bool,
    colors: &SessionColors,
) -> LevelEdit {
    let mut edit = LevelEdit::default();

    // The dB ticks, drawn before the fader so the fader sits over its own
    // scale rather than under it.
    if let Some(scale) = strip.scale {
        for (db, label) in FADER_TICKS {
            let y = egui::lerp(scale.bottom()..=scale.top(), fader_position(*db));
            if y < scale.top() - 0.5 || y > scale.bottom() + 0.5 {
                continue;
            }
            let unity = *db == 0.0;
            ui.painter().line_segment(
                [
                    egui::pos2(scale.left(), y),
                    egui::pos2(scale.left() + if unity { 6.0 } else { 3.0 }, y),
                ],
                egui::Stroke::new(1.0, if unity { colors.muted } else { colors.divider }),
            );
            ui.painter().text(
                egui::pos2(scale.right(), y),
                egui::Align2::RIGHT_CENTER,
                label,
                egui::FontId::monospace(MICRO_FONT - 1.0),
                if unity { colors.muted } else { colors.divider },
            );
        }
    }

    // The meter, on the fader's own taper so a level can be read against
    // the ticks beside it instead of against nothing.
    ui.painter().rect_filled(strip.meter, 0.0, colors.sunken);
    let level = fader_normalized(peak).clamp(0.0, 1.0);
    if level > 0.0 {
        let top = egui::lerp(strip.meter.bottom()..=strip.meter.top(), level);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(strip.meter.left() + 1.0, top),
                egui::pos2(strip.meter.right() - 1.0, strip.meter.bottom()),
            ),
            0.0,
            if peak > 0.9 {
                colors.meter_hot
            } else {
                colors.meter_low
            },
        );
    }
    if hold > 0.0 {
        let y = egui::lerp(
            strip.meter.bottom()..=strip.meter.top(),
            fader_normalized(hold).clamp(0.0, 1.0),
        );
        ui.painter().line_segment(
            [
                egui::pos2(strip.meter.left(), y),
                egui::pos2(strip.meter.right(), y),
            ],
            egui::Stroke::new(
                1.0,
                if clipped {
                    colors.meter_clip
                } else {
                    colors.text
                },
            ),
        );
    }

    // The fader.
    let response = ui.interact(strip.fader, id.with("fader"), egui::Sense::click_and_drag());
    if response.double_clicked() {
        edit.volume = Some(1.0);
    } else if response.dragged() {
        let moved = fine_drag(ui, -response.drag_delta().y);
        if moved != 0.0 {
            edit.volume = Some(fader_volume(
                fader_normalized(volume) + moved / strip.fader.height().max(1.0),
            ));
        }
    }
    ui.painter().rect_filled(strip.fader, 0.0, colors.sunken);
    let unity_y = egui::lerp(
        strip.fader.bottom()..=strip.fader.top(),
        fader_position(0.0),
    );
    ui.painter().line_segment(
        [
            egui::pos2(strip.fader.left(), unity_y),
            egui::pos2(strip.fader.right(), unity_y),
        ],
        egui::Stroke::new(1.0, colors.divider),
    );
    let handle_y = egui::lerp(
        strip.fader.bottom()..=strip.fader.top(),
        fader_normalized(volume),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(strip.fader.left() + 1.0, handle_y),
            egui::pos2(strip.fader.right() - 1.0, strip.fader.bottom()),
        ),
        0.0,
        colors.accent_dim.gamma_multiply(0.6),
    );
    ui.painter().rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(strip.fader.center().x, handle_y),
            egui::vec2(strip.fader.width() + 4.0, 5.0),
        ),
        1.0,
        if response.hovered() || response.dragged() {
            colors.text
        } else {
            colors.muted
        },
    );
    response.on_hover_text(format!(
        "{} dB · drag to move · shift for fine · double-click returns to unity",
        db_text(volume)
    ));

    // The two numbers.
    if let Some(readout) = strip.readout {
        ui.painter().text(
            readout.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} dB", db_text(volume)),
            egui::FontId::monospace(MICRO_FONT),
            colors.text,
        );
    }
    if let Some(row) = strip.peak {
        let response = ui.interact(row, id.with("peak"), egui::Sense::click());
        edit.clear_peak = response.clicked();
        ui.painter().rect_filled(
            row,
            0.0,
            if clipped {
                colors.meter_clip.gamma_multiply(0.3)
            } else if response.hovered() {
                colors.raised
            } else {
                colors.bg
            },
        );
        ui.painter().text(
            row.center(),
            egui::Align2::CENTER_CENTER,
            format!("pk {}", db_text(hold)),
            egui::FontId::monospace(MICRO_FONT - 1.0),
            if clipped {
                colors.meter_clip
            } else {
                colors.muted
            },
        );
        response.on_hover_text("loudest peak since this was last cleared · click to clear");
    } else if clipped {
        // No room for the number, so the clip still has to be sayable: a
        // lamp on the meter, clickable exactly as the number is.
        let lamp = egui::Rect::from_min_max(
            strip.meter.left_top(),
            egui::pos2(strip.meter.right(), strip.meter.top() + 3.0),
        );
        if ui
            .interact(lamp.expand(3.0), id.with("lamp"), egui::Sense::click())
            .clicked()
        {
            edit.clear_peak = true;
        }
        ui.painter().rect_filled(lamp, 0.0, colors.meter_clip);
    }

    edit
}

#[allow(clippy::too_many_arguments)]
fn paint_mixer(
    ui: &mut egui::Ui,
    track: &SessionTrack,
    track_index: usize,
    returns: &[SessionReturn],
    returns_or_inputs: u32,
    runtime: Option<&TrackRuntime>,
    state: &mut SessionViewState,
    rect: egui::Rect,
    colors: &SessionColors,
    intents: &mut Vec<SessionIntent>,
) {
    let strip = MixerStrip::new(
        rect,
        StripContent {
            sends: returns.len(),
            // A note lane has no input path at all, so it is offered no
            // control rather than one that could only ever be silence.
            io: matches!(track.kind, TrackKind::Audio),
        },
    );
    ui.painter().rect_filled(rect, 0.0, colors.surface);
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, colors.divider),
    );

    // ---- mute and solo.
    let mute = control_button(
        ui,
        strip.mute,
        ui.id().with(("session_next_mix_button", track_index, "M")),
        "M",
        track.mute,
        colors.warn,
        colors,
    );
    if mute.clicked() {
        intents.push(SessionIntent::ToggleTrackMute(track_index));
    }
    mute.on_hover_text(if track.mute { "unmute" } else { "mute" });

    let solo = control_button(
        ui,
        strip.solo,
        ui.id().with(("session_next_mix_button", track_index, "S")),
        "S",
        track.solo,
        colors.accent,
        colors,
    );
    if solo.clicked() {
        // EXCLUSIVE by default, additive on Ctrl. Live's grammar, and
        // the useful way round: soloing to hear ONE thing is the move a
        // hand makes constantly, and building up a set of solos is the
        // rare one, so the rare one is the one that carries a modifier.
        intents.push(if ui.input(|input| input.modifiers.command) {
            SessionIntent::ToggleTrackSolo(track_index)
        } else {
            SessionIntent::SoloTrackExclusive(track_index)
        });
    }
    solo.on_hover_text("solo · ctrl-click to add to the solo set");

    // ---- the input route.
    if let Some(button) = strip.monitor
        && let Some(route) = strip.route
    {
        let routable = returns_or_inputs > 0;
        let monitor = control_button(
            ui,
            button,
            ui.id().with(("session_next_monitor", track_index)),
            "IN",
            track.monitoring,
            colors.ok,
            colors,
        );
        if monitor.clicked() {
            intents.push(SessionIntent::ToggleTrackMonitor(track_index));
        }
        monitor.on_hover_text(if track.monitoring {
            "stop monitoring this input"
        } else {
            // Said before it happens, because the thing it can do is
            // put the speakers into the microphone.
            "hear this input through the lane's chain — headphones first"
        });

        let response = ui.interact(
            route,
            ui.id().with(("session_next_route", track_index)),
            egui::Sense::click(),
        );
        if response.clicked() {
            intents.push(SessionIntent::CycleTrackInput {
                track: track_index,
                back: ui.input(|input| input.modifiers.shift),
            });
        }
        ui.painter().rect_filled(
            route,
            0.0,
            if response.hovered() && routable {
                colors.raised
            } else {
                colors.sunken
            },
        );
        ui.painter().text(
            route.center(),
            egui::Align2::CENTER_CENTER,
            &track.input,
            egui::FontId::monospace(MICRO_FONT),
            if routable {
                colors.text
            } else {
                colors.divider
            },
        );
        response.on_hover_text(if routable {
            format!(
                "input {} · click for the next route · shift-click for the last",
                track.input
            )
        } else {
            "no inputs to route from — start the engine, or the interface has none".to_owned()
        });
    }

    // ---- pan.
    if let Some(pan_rect) = strip.pan
        && let Some(value) = pan_control(
            ui,
            pan_rect,
            ui.id().with(("session_next_pan", track_index)),
            track.pan,
            colors,
        )
    {
        intents.push(SessionIntent::SetTrackPan {
            track: track_index,
            value,
        });
    }

    // ---- the sends.
    for (index, row) in strip.sends.iter().enumerate() {
        let level = track.sends.get(index).copied().unwrap_or(0.0);
        let name = returns.get(index).map_or("", |bus| bus.name.as_str());
        if let Some(value) = send_row(
            ui,
            *row,
            ui.id().with(("session_next_send", track_index, index)),
            index,
            level,
            name,
            colors,
        ) {
            intents.push(SessionIntent::SetTrackSend {
                track: track_index,
                index,
                value,
            });
        }
    }
    if strip.sends_hidden > 0
        && let Some(last) = strip.sends.last()
    {
        // A send you cannot see is still sending, so the count is drawn
        // rather than the block quietly ending.
        ui.painter().text(
            egui::pos2(last.right(), last.bottom() + 1.0),
            egui::Align2::RIGHT_TOP,
            format!("+{}", strip.sends_hidden),
            egui::FontId::monospace(MICRO_FONT - 2.0),
            colors.muted,
        );
    }

    // ---- the level column.
    let peak = runtime.map_or(0.0, |runtime| runtime.peak.max(0.0));
    let hold = state.hold_peak(track_index, peak);
    let clipped = runtime.is_some_and(|runtime| runtime.clipped);
    let edit = paint_level_column(
        ui,
        &strip,
        ui.id().with(("session_next_level", track_index)),
        track.volume,
        peak,
        hold,
        clipped,
        colors,
    );
    if let Some(value) = edit.volume {
        intents.push(SessionIntent::SetTrackVolume {
            track: track_index,
            value,
        });
    }
    if edit.clear_peak {
        state.clear_peak(track_index);
        intents.push(SessionIntent::ClearClipHold(track_index));
    }
}

/// One send row: its letter, and how much goes.
///
/// A bar rather than a knob, because a strip is a column and a column
/// has width to spare and no height at all. Returns the level it was
/// dragged to, if it moved.
#[allow(clippy::too_many_arguments)]
fn send_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    index: usize,
    level: f32,
    name: &str,
    colors: &SessionColors,
) -> Option<f32> {
    let letter = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(
            (rect.left() + SEND_LETTER_WIDTH).min(rect.right()),
            rect.bottom(),
        ),
    );
    let bar = egui::Rect::from_min_max(egui::pos2(letter.right(), rect.top()), rect.max);
    let response = ui.interact(bar, id, egui::Sense::click_and_drag());
    let moved = if response.double_clicked() {
        Some(0.0)
    } else if response.dragged() {
        let amount = fine_drag(ui, response.drag_delta().x);
        (amount != 0.0).then(|| send_level(send_normalized(level) + amount / bar.width().max(1.0)))
    } else {
        None
    };
    let live = response.hovered() || response.dragged();

    ui.painter().text(
        letter.center(),
        egui::Align2::CENTER_CENTER,
        daw_return_letter(index),
        egui::FontId::monospace(MICRO_FONT - 1.0),
        if live { colors.text } else { colors.divider },
    );
    ui.painter().rect_filled(bar, 0.0, colors.sunken);
    let filled = send_normalized(level);
    if filled > 0.0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(bar.left(), bar.top() + 1.0),
                egui::pos2(
                    egui::lerp(bar.left()..=bar.right(), filled),
                    bar.bottom() - 1.0,
                ),
            ),
            0.0,
            if live {
                colors.role_mod
            } else {
                colors.role_mod.gamma_multiply(0.55)
            },
        );
    }
    response.on_hover_text(if name.is_empty() {
        format!("send {} · {} dB", daw_return_letter(index), db_text(level))
    } else {
        format!(
            "send {} to {name} · {} dB · drag to open · double-click shuts it",
            daw_return_letter(index),
            db_text(level)
        )
    });
    moved
}

/// The letter a send row and a return column share: `A`, `B`, …
///
/// The view's own copy rather than the project's, because this module may
/// not name the app — the same rule that keeps `SessionTrack` apart from
/// `Track`. There is exactly one alphabet, so the two cannot drift.
fn daw_return_letter(index: usize) -> String {
    if index < 26 {
        ((b'A' + index as u8) as char).to_string()
    } else {
        "?".to_owned()
    }
}

/// A return's strip: the same level column a track has, a mute, a pan,
/// and no sends of its own — a return that sent would be a feedback loop
/// the graph cannot compile.
#[allow(clippy::too_many_arguments)]
fn paint_return_strip(
    ui: &mut egui::Ui,
    bus: &SessionReturn,
    index: usize,
    runtime: Option<&TrackRuntime>,
    state: &mut SessionViewState,
    rect: egui::Rect,
    selected: bool,
    colors: &SessionColors,
    intents: &mut Vec<SessionIntent>,
) {
    let strip = MixerStrip::new(rect, StripContent::default());
    ui.painter().rect_filled(rect, 0.0, colors.surface);
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, colors.divider),
    );
    if selected {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            0.0,
            egui::Stroke::new(1.0, colors.selected),
            egui::StrokeKind::Inside,
        );
    }

    // The letter sits where a track's solo does, because a return has no
    // solo — and the letter is the thing a send row is pointing at, so
    // it is the one label that must be visible from across the room.
    let head = egui::Rect::from_min_max(strip.mute.left_top(), strip.solo.right_bottom());
    let response = ui.interact(
        head,
        ui.id().with(("session_next_return_head", index)),
        egui::Sense::click(),
    );
    if response.clicked() {
        intents.push(SessionIntent::SelectReturn(index));
    }
    let mute = control_button(
        ui,
        strip.mute,
        ui.id().with(("session_next_return_mute", index)),
        "M",
        bus.mute,
        colors.warn,
        colors,
    );
    if mute.clicked() {
        intents.push(SessionIntent::ToggleReturnMute(index));
    }
    mute.on_hover_text(if bus.mute {
        "switch this return back on"
    } else {
        "switch this return off — its sends go with it"
    });
    ui.painter().text(
        strip.solo.center(),
        egui::Align2::CENTER_CENTER,
        daw_return_letter(index),
        egui::FontId::monospace(LABEL_FONT + 1.0),
        if selected {
            colors.selected
        } else {
            colors.muted
        },
    );
    response.on_hover_text(format!("{} · click to show its devices", bus.name));

    if let Some(pan_rect) = strip.pan
        && let Some(value) = pan_control(
            ui,
            pan_rect,
            ui.id().with(("session_next_return_pan", index)),
            bus.pan,
            colors,
        )
    {
        intents.push(SessionIntent::SetReturnPan { index, value });
    }

    // The return's name, in the space its sends would have taken. A
    // return is the one strip whose name is worth repeating down here:
    // `A` says where it sits, and only the name says what it is.
    let name_row = egui::Rect::from_min_max(
        egui::pos2(strip.fader.left(), strip.fader.top() - SEND_HEIGHT),
        egui::pos2(strip.meter.right(), strip.fader.top() - 1.0),
    );
    if name_row.height() > 6.0 && strip.pan.is_some() {
        ui.painter().text(
            name_row.center(),
            egui::Align2::CENTER_CENTER,
            &bus.name,
            egui::FontId::monospace(MICRO_FONT - 1.0),
            colors.muted,
        );
    }

    let peak = runtime.map_or(0.0, |runtime| runtime.peak.max(0.0));
    let slot = RETURN_HOLD_BASE + index;
    let hold = state.hold_peak(slot, peak);
    let clipped = runtime.is_some_and(|runtime| runtime.clipped);
    let edit = paint_level_column(
        ui,
        &strip,
        ui.id().with(("session_next_return_level", index)),
        bus.volume,
        peak,
        hold,
        clipped,
        colors,
    );
    if let Some(value) = edit.volume {
        intents.push(SessionIntent::SetReturnVolume { index, value });
    }
    if edit.clear_peak {
        state.clear_peak(slot);
    }
}

fn paint_scene_column(
    ui: &mut egui::Ui,
    document: &SessionDocument,
    runtime: &SessionRuntime,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
    intents: &mut Vec<SessionIntent>,
) {
    let column = layout.scene_column();
    ui.painter().rect_filled(column, 0.0, colors.sunken);
    ui.painter().line_segment(
        [column.left_top(), column.left_bottom()],
        egui::Stroke::new(1.0, colors.divider),
    );
    let header = egui::Rect::from_min_max(
        column.min,
        egui::pos2(column.right(), column.top() + HEADER_HEIGHT),
    );
    let session_active = runtime.tracks.iter().any(|track| {
        !matches!(track.playback, PlaybackState::Arrangement)
            || track
                .pending
                .is_some_and(|pending| !matches!(pending.action, PendingTrackAction::Arrangement))
    });
    let back_id = ui.id().with("session_next_back_to_arrangement");
    let back = ui.interact(header.shrink(4.0), back_id, egui::Sense::click());
    if back.clicked() {
        intents.push(SessionIntent::BackToArrangement);
    }
    ui.painter().rect_filled(
        header.shrink(4.0),
        0.0,
        if session_active {
            colors
                .warn
                .gamma_multiply(if back.hovered() { 0.9 } else { 0.68 })
        } else if back.hovered() {
            colors.raised
        } else {
            colors.surface
        },
    );
    ui.painter().text(
        header.center(),
        egui::Align2::CENTER_CENTER,
        if session_active {
            "↩  RETURN TO ARR"
        } else {
            "SCENES / MAIN"
        },
        egui::FontId::monospace(LABEL_FONT),
        if session_active {
            colors.bg
        } else {
            colors.muted
        },
    );

    let rows_clip = layout.scene_rows_viewport();
    for (scene_index, scene) in document.scenes.iter().enumerate() {
        let full = layout.scene(scene_index);
        if full.bottom() < rows_clip.top() {
            continue;
        }
        if full.top() > rows_clip.bottom() {
            break;
        }
        let rect = full.intersect(rows_clip);
        if rect.height() <= 1.0 {
            continue;
        }
        let launch_rect = layout.scene_launch(scene_index).intersect(rows_clip);
        let body_rect = layout.scene_body(scene_index).intersect(rows_clip);
        let grip_rect = layout.scene_grip(scene_index).intersect(rows_clip);
        let selected = state.selection == Some(GridSelection::Scene(scene_index));
        let active = runtime.active_scene == Some(scene.id);
        let queued = runtime
            .pending
            .iter()
            .any(|launch| launch.scene == Some(scene.id));
        ui.painter().rect_filled(
            rect,
            0.0,
            if selected {
                colors.raised
            } else {
                colors.surface
            },
        );
        if active {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(STATE_EDGE, rect.height())),
                0.0,
                colors.ok,
            );
        }
        if queued {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STATE_EDGE)),
                0.0,
                colors.warn,
            );
        }
        if selected {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                0.0,
                egui::Stroke::new(2.0, colors.selected),
                egui::StrokeKind::Inside,
            );
        }
        let body_id = ui.id().with(("session_next_scene_body", scene.id.0));
        let body = ui.interact(body_rect, body_id, egui::Sense::click());
        if body.clicked() {
            state.selection = Some(GridSelection::Scene(scene_index));
            state.owns_keyboard = true;
            intents.push(SessionIntent::SelectScene(scene_index));
        }
        let launch_id = ui.id().with(("session_next_scene_launch", scene.id.0));
        let launch = ui.interact(launch_rect, launch_id, egui::Sense::click());
        if launch.clicked() {
            intents.push(SessionIntent::LaunchScene(scene_index));
            state.owns_keyboard = true;
        }
        paint_scene_triangle(ui.painter(), launch_rect, active, launch.hovered(), colors);
        paint_scene_text(ui.painter(), body_rect, scene_index, scene, active, colors);
        let grip_id = ui.id().with(("session_next_scene_grip", scene.id.0));
        let grip = ui.interact(grip_rect, grip_id, egui::Sense::drag());
        if grip.drag_started() {
            state.scene_drag = Some(SceneDragState {
                from: scene_index,
                target: scene_index,
            });
        }
        let grip_color = if grip.hovered() || grip.dragged() {
            colors.accent
        } else {
            colors.divider
        };
        for offset in [-3.0, 0.0, 3.0] {
            ui.painter().circle_filled(
                egui::pos2(grip_rect.center().x, grip_rect.center().y + offset),
                1.0,
                grip_color,
            );
        }
    }

    let stop_all = layout.stop_all().intersect(rows_clip);
    if stop_all.height() > 1.0 {
        let id = ui.id().with("session_next_stop_all");
        let response = ui.interact(stop_all, id, egui::Sense::click());
        if response.clicked() {
            intents.push(SessionIntent::StopAll);
        }
        ui.painter().rect_filled(
            stop_all,
            0.0,
            if response.hovered() {
                colors.raised
            } else {
                colors.surface
            },
        );
        let square = egui::Rect::from_center_size(
            egui::pos2(stop_all.left() + LAUNCH_WIDTH * 0.5, stop_all.center().y),
            egui::vec2(7.0, 7.0),
        );
        ui.painter().rect_filled(square, 0.0, colors.muted);
        ui.painter().text(
            egui::pos2(stop_all.left() + LAUNCH_WIDTH, stop_all.center().y),
            egui::Align2::LEFT_CENTER,
            "STOP ALL",
            egui::FontId::monospace(LABEL_FONT),
            colors.muted,
        );
    }
    let add = layout.add_scene().intersect(rows_clip);
    if add.height() > 1.0 {
        let id = ui.id().with("session_next_add_scene");
        let response = ui.interact(add, id, egui::Sense::click());
        if response.clicked() {
            let below = match state.selection {
                Some(GridSelection::Scene(scene)) => scene,
                Some(GridSelection::Slot { scene, .. }) => scene,
                _ => document.scenes.len().saturating_sub(1),
            };
            intents.push(SessionIntent::InsertSceneBelow(below));
        }
        if response.hovered() {
            ui.painter().rect_filled(add, 0.0, colors.raised);
        }
        ui.painter().text(
            egui::pos2(add.left() + LAUNCH_WIDTH, add.center().y),
            egui::Align2::LEFT_CENTER,
            "+ SCENE",
            egui::FontId::monospace(LABEL_FONT),
            colors.muted,
        );
    }
}

fn paint_scene_triangle(
    painter: &egui::Painter,
    rect: egui::Rect,
    active: bool,
    hovered: bool,
    colors: &SessionColors,
) {
    if hovered {
        painter.rect_filled(rect, 0.0, colors.raised);
    }
    let center = rect.center();
    let points = vec![
        egui::pos2(center.x - 3.0, center.y - 5.0),
        egui::pos2(center.x + 5.0, center.y),
        egui::pos2(center.x - 3.0, center.y + 5.0),
    ];
    let color = if active { colors.ok } else { colors.muted };
    if active {
        painter.add(egui::Shape::convex_polygon(
            points,
            color,
            egui::Stroke::NONE,
        ));
    } else {
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.25, color),
        ));
    }
}

fn paint_scene_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    index: usize,
    scene: &Scene,
    active: bool,
    colors: &SessionColors,
) {
    let inner = rect.shrink2(egui::vec2(5.0, 3.0));
    painter.with_clip_rect(inner).text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        format!("{:02}  {}", index + 1, scene.name),
        egui::FontId::proportional(BODY_FONT),
        if active { colors.text } else { colors.muted },
    );
    let mut detail = Vec::new();
    if let Some(tempo) = scene.tempo {
        detail.push(format!("{tempo:.0}"));
    }
    if let Some((top, unit)) = scene.signature {
        detail.push(format!("{top}/{unit}"));
    }
    if !scene.locks.is_empty() {
        detail.push(format!("{} LOCK", scene.locks.len()));
    }
    if scene.follow != FollowAction::None {
        detail.push("→".to_owned());
    }
    if !detail.is_empty() {
        painter.text(
            inner.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            detail.join("   "),
            egui::FontId::monospace(MICRO_FONT),
            colors.muted,
        );
    }
    if let Some([r, g, b]) = scene.color {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - STATE_EDGE, rect.top()),
                egui::vec2(STATE_EDGE, rect.height()),
            ),
            0.0,
            egui::Color32::from_rgb(r, g, b),
        );
    }
}

fn paint_scrollbar(
    ui: &mut egui::Ui,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
) {
    if layout.max_scroll_x() <= 0.0 {
        return;
    }
    let bar = egui::Rect::from_min_max(
        egui::pos2(layout.area.left(), layout.area.bottom() - SCROLLBAR_HEIGHT),
        egui::pos2(layout.area.right() - SCENE_WIDTH, layout.area.bottom()),
    );
    let visible = layout.tracks_viewport().width();
    let content = layout.tracks as f32 * layout.track_width;
    let thumb_width = (bar.width() * visible / content).clamp(24.0, bar.width());
    let travel = (bar.width() - thumb_width).max(0.0);
    let fraction = if layout.max_scroll_x() > 0.0 {
        layout.scroll_x / layout.max_scroll_x()
    } else {
        0.0
    };
    let thumb = egui::Rect::from_min_size(
        egui::pos2(bar.left() + travel * fraction, bar.top()),
        egui::vec2(thumb_width, bar.height()),
    );
    let id = ui.id().with("session_next_scrollbar");
    let response = ui.interact(bar, id, egui::Sense::click_and_drag());
    if (response.dragged() || response.clicked())
        && let Some(position) = response.interact_pointer_pos()
        && travel > 0.0
    {
        let at = ((position.x - bar.left() - thumb_width * 0.5) / travel).clamp(0.0, 1.0);
        state.scroll_x = at * layout.max_scroll_x();
    }
    ui.painter().rect_filled(bar, 0.0, colors.sunken);
    ui.painter().rect_filled(
        thumb.shrink2(egui::vec2(0.0, 1.5)),
        0.0,
        if response.hovered() || response.dragged() {
            colors.accent
        } else {
            colors.outline
        },
    );
}

fn settle_slot_drag(
    ui: &mut egui::Ui,
    document: &SessionDocument,
    state: &mut SessionViewState,
    layout: SessionLayout,
    intents: &mut Vec<SessionIntent>,
) {
    let Some(mut drag) = state.slot_drag else {
        return;
    };
    drag.target = ui
        .ctx()
        .pointer_latest_pos()
        .and_then(|position| layout.slot_at(position))
        .filter(|(track, scene)| {
            let Some(source) = document.slot(drag.from.0, drag.from.1).and_then(Slot::clip) else {
                return false;
            };
            document
                .tracks
                .get(*track)
                .is_some_and(|target| target.kind == source.kind)
                && *scene < document.scenes.len()
        });
    if let Some(target) = drag.target {
        let rect = layout.slot(target.0, target.1);
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            0.0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    let released = ui.input(|input| input.pointer.primary_released());
    if released {
        if let Some(target) = drag.target
            && target != drag.from
        {
            intents.push(SessionIntent::MoveSlots {
                from: drag.from,
                to: target,
                copy: drag.copy,
            });
        }
        state.slot_drag = None;
    } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        state.slot_drag = None;
    } else {
        state.slot_drag = Some(drag);
    }
}

fn settle_track_drag(
    ui: &mut egui::Ui,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
    intents: &mut Vec<SessionIntent>,
) {
    let Some(mut drag) = state.track_drag else {
        return;
    };
    if let Some(position) = ui.ctx().pointer_latest_pos()
        && layout.tracks > 0
    {
        let local = position.x + layout.scroll_x - layout.area.left();
        drag.target = (local / layout.track_width)
            .floor()
            .clamp(0.0, layout.tracks.saturating_sub(1) as f32) as usize;
    }
    let target = layout.header(drag.target);
    ui.painter().line_segment(
        [target.left_top(), target.left_bottom()],
        egui::Stroke::new(2.0, colors.accent),
    );
    if ui.input(|input| input.pointer.primary_released()) {
        if drag.from != drag.target {
            intents.push(SessionIntent::ReorderTrack {
                from: drag.from,
                to: drag.target,
            });
        }
        state.track_drag = None;
    } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        state.track_drag = None;
    } else {
        state.track_drag = Some(drag);
    }
}

fn settle_scene_drag(
    ui: &mut egui::Ui,
    state: &mut SessionViewState,
    colors: &SessionColors,
    layout: SessionLayout,
    intents: &mut Vec<SessionIntent>,
) {
    let Some(mut drag) = state.scene_drag else {
        return;
    };
    if let Some(position) = ui.ctx().pointer_latest_pos()
        && layout.scenes > 0
    {
        let local =
            position.y + layout.scroll_y - layout.area.top() - CONTROL_HEIGHT - HEADER_HEIGHT;
        drag.target = (local / (layout.slot_height + SLOT_GAP))
            .floor()
            .clamp(0.0, layout.scenes.saturating_sub(1) as f32) as usize;
    }
    let target = layout.scene(drag.target);
    ui.painter().line_segment(
        [target.left_top(), target.right_top()],
        egui::Stroke::new(2.0, colors.accent),
    );
    if ui.input(|input| input.pointer.primary_released()) {
        if drag.from != drag.target {
            intents.push(SessionIntent::ReorderScene {
                from: drag.from,
                to: drag.target,
            });
        }
        state.scene_drag = None;
    } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        state.scene_drag = None;
    } else {
        state.scene_drag = Some(drag);
    }
}

fn keyboard_intents(
    ui: &mut egui::Ui,
    document: &SessionDocument,
    runtime: &SessionRuntime,
    state: &mut SessionViewState,
    intents: &mut Vec<SessionIntent>,
) {
    if !state.owns_keyboard || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    if runtime.fill == FillState::Momentary
        && !ui.input(|input| input.key_down(egui::Key::F) || input.pointer.primary_down())
    {
        intents.push(SessionIntent::SetFill(FillState::Off));
    }
    let mut selection = state.selection;
    ui.input_mut(|input| {
        let step_track = if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
            -1
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
            1
        } else {
            0
        };
        let step_scene = if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
            -1
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
            1
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) {
            -8
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
            8
        } else {
            0
        };
        if step_track != 0 || step_scene != 0 {
            let (track, scene) = match selection {
                Some(GridSelection::Slot { track, scene }) => (track, scene),
                Some(GridSelection::Track(track)) => (track, 0),
                Some(GridSelection::Scene(scene)) => (0, scene),
                None => (0, 0),
            };
            let track = (track as i32 + step_track)
                .clamp(0, document.tracks.len().saturating_sub(1) as i32)
                as usize;
            let scene = (scene as i32 + step_scene)
                .clamp(0, document.scenes.len().saturating_sub(1) as i32)
                as usize;
            selection = Some(GridSelection::Slot { track, scene });
            intents.push(SessionIntent::SelectSlot { track, scene });
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
            match selection {
                Some(GridSelection::Slot { track, scene }) => {
                    intents.push(SessionIntent::LaunchSlot { track, scene });
                }
                Some(GridSelection::Scene(scene)) => {
                    intents.push(SessionIntent::LaunchScene(scene))
                }
                Some(GridSelection::Track(_)) | None => {}
            }
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Delete) {
            intents.push(SessionIntent::DeleteSelection);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::F) {
            intents.push(SessionIntent::SetFill(if input.modifiers.shift {
                match runtime.fill {
                    FillState::Latched => FillState::Off,
                    FillState::Off | FillState::Momentary => FillState::Latched,
                }
            } else {
                FillState::Momentary
            }));
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            selection = None;
            intents.push(SessionIntent::ClearSelection);
        }
    });
    state.selection = selection;
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    fn view() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, 620.0))
    }

    fn clock(sample: u64) -> TransportClock {
        TransportClock {
            sample,
            sample_rate: 48_000,
            bpm: 120.0,
            beats_per_bar: 4,
        }
    }

    fn midi_clip(id: u64, name: &str) -> SessionClip {
        SessionClip {
            id: ClipId(id),
            name: name.to_owned(),
            kind: TrackKind::Midi,
            preview: ClipPreview::Notes(vec![[0.0, 0.2, 0.4], [0.5, 0.25, 0.7]]),
            ..SessionClip::default()
        }
    }

    fn audio_clip(id: u64, name: &str) -> SessionClip {
        SessionClip {
            id: ClipId(id),
            name: name.to_owned(),
            kind: TrackKind::Audio,
            preview: ClipPreview::Waveform(vec![[-0.4, 0.3], [-0.8, 0.7], [-0.2, 0.1]]),
            ..SessionClip::default()
        }
    }

    fn document() -> SessionDocument {
        let tracks = vec![
            SessionTrack {
                id: TrackId(10),
                name: "SYNTH".to_owned(),
                kind: TrackKind::Midi,
                ..SessionTrack::default()
            },
            SessionTrack {
                id: TrackId(20),
                name: "TAPE".to_owned(),
                kind: TrackKind::Audio,
                ..SessionTrack::default()
            },
        ];
        let mut document = SessionDocument::new(tracks);
        document.slots[0][0] = Slot::Clip(midi_clip(100, "VERSE"));
        document.slots[1][0] = Slot::Clip(audio_clip(200, "DRUMS"));
        document.slots[0][1] = Slot::Clip(midi_clip(101, "CHORUS"));
        document.slots[1][1] = Slot::Empty(EmptyBehavior::Continue);
        document
    }

    fn render_path(
        document: &SessionDocument,
        runtime: &SessionRuntime,
        state: &mut SessionViewState,
        path: &[probe::Step],
    ) -> Vec<Vec<SessionIntent>> {
        let context = egui::Context::default();
        let colors = SessionColors::default();
        let clipboard = SessionClipboard::default();
        probe::run(&context, view(), path, |ui| {
            show_session(
                ui,
                document,
                runtime,
                clock(12_000),
                &clipboard,
                state,
                &colors,
            )
            .intents
        })
    }

    fn flattened(frames: &[Vec<SessionIntent>]) -> Vec<SessionIntent> {
        frames.iter().flatten().cloned().collect()
    }

    #[test]
    fn fresh_document_is_rectangular() {
        let document = document();
        assert_eq!(document.slots.len(), document.tracks.len());
        assert!(
            document
                .slots
                .iter()
                .all(|column| column.len() == document.scenes.len())
        );
    }

    #[test]
    fn sanitize_repairs_a_hostile_document() {
        let mut document = document();
        document.scenes.clear();
        document.slots = vec![Vec::new()];
        document.sanitize();
        assert_eq!(document.scenes.len(), 1);
        assert_eq!(document.slots.len(), 2);
        assert!(document.slots.iter().all(|column| column.len() == 1));
    }

    #[test]
    fn sanitize_caps_scene_locks_and_signature() {
        let mut document = document();
        document.scenes[0].locks = (0..32)
            .map(|parameter| SceneLock {
                track: TrackId(10),
                device: None,
                parameter,
                value: 0.5,
            })
            .collect();
        document.scenes[0].signature = Some((99, 7));
        document.sanitize();
        assert_eq!(document.scenes[0].locks.len(), MAX_SCENE_LOCKS);
        assert_eq!(document.scenes[0].signature, Some((32, 8)));
    }

    #[test]
    fn quantization_exactly_on_a_boundary_stays_there() {
        assert_eq!(clock(96_000).next_boundary(Quantization::Bar), 96_000);
    }

    #[test]
    fn quantization_moves_to_the_next_musical_edge() {
        assert_eq!(clock(1).next_boundary(Quantization::Bar), 96_000);
        assert_eq!(clock(24_001).next_boundary(Quantization::Quarter), 48_000);
        assert_eq!(clock(6_001).next_boundary(Quantization::Sixteenth), 12_000);
    }

    #[test]
    fn no_quantization_uses_the_current_sample() {
        assert_eq!(clock(12_345).next_boundary(Quantization::None), 12_345);
    }

    #[test]
    fn scene_launch_is_one_atomic_transaction() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_scene(&document, 0, clock(1)).unwrap();
        assert_eq!(launch.at_sample, 96_000);
        assert_eq!(launch.operations.len(), 2);
        assert!(
            launch
                .operations
                .iter()
                .all(|op| { matches!(op.action, PendingTrackAction::Start { .. }) })
        );
        assert!(runtime.tracks.iter().all(|track| {
            track
                .pending
                .is_some_and(|pending| pending.transaction == launch.id)
        }));
    }

    #[test]
    fn continue_slot_does_not_touch_its_track() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_scene(&document, 1, clock(1)).unwrap();
        assert!(launch.operations.iter().any(|op| op.track == 0));
        assert!(!launch.operations.iter().any(|op| op.track == 1));
    }

    #[test]
    fn stop_slot_authors_a_real_stop() {
        let mut document = document();
        document.slots[1][1] = Slot::Empty(EmptyBehavior::Stop);
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_scene(&document, 1, clock(1)).unwrap();
        assert!(
            launch
                .operations
                .iter()
                .any(|op| { op.track == 1 && op.action == PendingTrackAction::Stop })
        );
    }

    #[test]
    fn later_action_replaces_pending_action_for_that_track_only() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let scene = runtime.queue_scene(&document, 0, clock(1)).unwrap();
        let clip = runtime.queue_clip(&document, 0, 1, clock(2), true).unwrap();
        assert!(clip.is_some());
        assert_eq!(runtime.pending.len(), 2);
        let old = runtime
            .pending
            .iter()
            .find(|launch| launch.id == scene.id)
            .unwrap();
        assert_eq!(old.scene, None);
        assert_eq!(old.operations.len(), 1);
        assert_eq!(old.operations[0].track, 1);
    }

    #[test]
    fn direct_launch_bypasses_conditions_and_fill() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                condition: LaunchCondition::Probability(1),
                fill: FillRule::Only,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.fill = FillState::Off;
        let launch = runtime.queue_clip(&document, 0, 0, clock(1), true).unwrap();
        assert!(launch.is_some());
    }

    #[test]
    fn fill_only_and_not_fill_are_opposites() {
        assert!(!fill_accepts(FillRule::Only, FillState::Off));
        assert!(fill_accepts(FillRule::Only, FillState::Momentary));
        assert!(fill_accepts(FillRule::Only, FillState::Latched));
        assert!(fill_accepts(FillRule::Not, FillState::Off));
        assert!(!fill_accepts(FillRule::Not, FillState::Latched));
    }

    #[test]
    fn every_condition_uses_one_based_cycles() {
        let condition = LaunchCondition::Every { step: 2, total: 4 };
        assert!(!condition_accepts(condition, 0, SceneId(1), 0, 1));
        assert!(condition_accepts(condition, 0, SceneId(1), 0, 2));
        assert!(!condition_accepts(condition, 0, SceneId(1), 0, 3));
        assert!(condition_accepts(condition, 0, SceneId(1), 0, 6));
    }

    #[test]
    fn probability_is_reproducible_and_bounded() {
        let a: Vec<_> = (1..100)
            .map(|cycle| deterministic_percent(42, SceneId(7), 3, cycle))
            .collect();
        let b: Vec<_> = (1..100)
            .map(|cycle| deterministic_percent(42, SceneId(7), 3, cycle))
            .collect();
        assert_eq!(a, b);
        assert!(a.iter().all(|value| *value < 100));
        assert!(a.iter().any(|value| *value < 50));
        assert!(a.iter().any(|value| *value >= 50));
    }

    #[test]
    fn declined_condition_uses_its_stop_fallback() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                fill: FillRule::Only,
                fallback: EmptyBehavior::Stop,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_scene(&document, 0, clock(1)).unwrap();
        assert!(
            launch
                .operations
                .iter()
                .any(|op| { op.track == 0 && op.action == PendingTrackAction::Stop })
        );
    }

    #[test]
    fn scene_locks_travel_with_the_transaction() {
        let mut document = document();
        document.scenes[0].locks.push(SceneLock {
            track: TrackId(10),
            device: Some(DeviceId(88)),
            parameter: 4,
            value: 0.75,
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_scene(&document, 0, clock(1)).unwrap();
        assert_eq!(launch.locks, document.scenes[0].locks);
    }

    #[test]
    fn toggle_mode_stops_an_already_playing_clip() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                mode: LaunchMode::Toggle,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.tracks[0].playback = PlaybackState::Playing {
            scene: document.scenes[0].id,
            clip: ClipId(100),
            started_at_sample: 0,
            cycle: 0,
        };
        let launch = runtime
            .queue_clip(&document, 0, 0, clock(1), true)
            .unwrap()
            .unwrap();
        assert_eq!(launch.operations[0].action, PendingTrackAction::Stop);
    }

    #[test]
    fn only_gate_mode_reacts_to_release() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                mode: LaunchMode::Gate,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let release = runtime.release_clip(&document, 0, 0, clock(1)).unwrap();
        assert!(release.is_some());
        let trigger_release = runtime.release_clip(&document, 0, 1, clock(1)).unwrap();
        assert!(trigger_release.is_none());
    }

    #[test]
    fn follow_next_targets_the_next_slot_on_the_same_track() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                follow: FollowAction::Next,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime
            .queue_follow(&document, 0, 0, clock(1))
            .unwrap()
            .unwrap();
        assert_eq!(
            launch.operations[0].action,
            PendingTrackAction::Start {
                scene: document.scenes[1].id,
                clip: ClipId(101),
            }
        );
    }

    #[test]
    fn follow_stop_queues_a_quantized_stop() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                follow: FollowAction::Stop,
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime
            .queue_follow(&document, 0, 0, clock(1))
            .unwrap()
            .unwrap();
        assert_eq!(launch.operations[0].action, PendingTrackAction::Stop);
        assert_eq!(launch.at_sample, 96_000);
    }

    #[test]
    fn named_follow_refuses_a_missing_scene() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                follow: FollowAction::Scene(SceneId(999_999)),
                ..clip.launch
            },
            ..clip
        });
        let mut runtime = SessionRuntime::new(document.tracks.len());
        assert_eq!(
            runtime.queue_follow(&document, 0, 0, clock(1)),
            Err(QueueRefusal::MissingScene)
        );
    }

    #[test]
    fn due_launch_does_not_apply_early() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.queue_scene(&document, 0, clock(1)).unwrap();
        assert!(runtime.apply_due(95_999).is_empty());
        assert!(
            runtime
                .tracks
                .iter()
                .all(|track| track.playback == PlaybackState::Arrangement)
        );
    }

    #[test]
    fn due_launch_starts_every_track_at_the_same_sample() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.queue_scene(&document, 0, clock(1)).unwrap();
        let due = runtime.apply_due(96_000);
        assert_eq!(due.len(), 1);
        for track in &runtime.tracks {
            assert!(matches!(
                track.playback,
                PlaybackState::Playing {
                    started_at_sample: 96_000,
                    ..
                }
            ));
        }
        assert_eq!(runtime.active_scene, Some(document.scenes[0].id));
    }

    #[test]
    fn clips_from_different_scenes_report_mixed_not_a_false_scene() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.tracks[0].playback = PlaybackState::Playing {
            scene: document.scenes[0].id,
            clip: ClipId(100),
            started_at_sample: 0,
            cycle: 0,
        };
        runtime.tracks[1].playback = PlaybackState::Playing {
            scene: document.scenes[1].id,
            clip: ClipId(200),
            started_at_sample: 0,
            cycle: 0,
        };
        assert_eq!(common_playing_scene(&runtime.tracks), None);
    }

    #[test]
    fn back_to_arrangement_is_one_transaction() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime.queue_stop_all(&document, clock(1), true);
        assert_eq!(launch.operations.len(), document.tracks.len());
        assert!(
            launch
                .operations
                .iter()
                .all(|op| { op.action == PendingTrackAction::Arrangement })
        );
    }

    #[test]
    fn discontinuity_clears_queues_cycles_and_sound() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.queue_scene(&document, 0, clock(1)).unwrap();
        runtime.scene_cycles.insert(document.scenes[0].id, 9);
        runtime.reset_on_discontinuity();
        assert!(runtime.pending.is_empty());
        assert!(runtime.scene_cycles.is_empty());
        assert!(
            runtime
                .tracks
                .iter()
                .all(|track| track.playback == PlaybackState::Stopped)
        );
    }

    #[test]
    fn incompatible_clip_is_refused_without_mutating_queue() {
        let mut document = document();
        document.slots[0][0] = Slot::Clip(audio_clip(999, "WRONG"));
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let result = runtime.queue_clip(&document, 0, 0, clock(1), true);
        assert_eq!(result, Err(QueueRefusal::IncompatibleClip));
        assert!(runtime.pending.is_empty());
    }

    #[test]
    fn remove_scene_keeps_columns_parallel() {
        let mut document = document();
        let before = document.scenes.len();
        assert!(document.remove_scene(2).is_some());
        assert_eq!(document.scenes.len(), before - 1);
        assert!(
            document
                .slots
                .iter()
                .all(|column| column.len() == document.scenes.len())
        );
    }

    #[test]
    fn moving_a_track_carries_its_entire_slot_column() {
        let mut document = document();
        let track = document.tracks[0].clone();
        let slots = document.slots[0].clone();
        assert!(document.move_track(0, 1));
        assert_eq!(document.tracks[1], track);
        assert_eq!(document.slots[1], slots);
    }

    #[test]
    fn moving_a_scene_carries_every_slot_in_its_row() {
        let mut document = document();
        let scene = document.scenes[0].clone();
        let row: Vec<_> = document
            .slots
            .iter()
            .map(|column| column[0].clone())
            .collect();
        assert!(document.move_scene(0, 3));
        assert_eq!(document.scenes[3], scene);
        for (track, slot) in row.iter().enumerate() {
            assert_eq!(&document.slots[track][3], slot);
        }
    }

    #[test]
    fn last_scene_cannot_be_removed() {
        let mut document = document();
        while document.scenes.len() > 1 {
            assert!(document.remove_scene(0).is_some());
        }
        assert!(document.remove_scene(0).is_none());
    }

    #[test]
    fn lift_does_not_mutate_document_and_drop_mints_fresh_scene_ids() {
        let mut document = document();
        let original = document.clone();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.tracks[0].playback = PlaybackState::Playing {
            scene: document.scenes[0].id,
            clip: ClipId(100),
            started_at_sample: 0,
            cycle: 0,
        };
        let mut clipboard = SessionClipboard::default();
        assert!(clipboard.lift(&document, &runtime, "CAPTURE"));
        assert_eq!(document, original);
        let first = clipboard.drop_into(&mut document, 0).unwrap();
        let first_id = document.scenes[first].id;
        let second = clipboard.drop_into(&mut document, first).unwrap();
        assert_ne!(first_id, document.scenes[second].id);
    }

    #[test]
    fn project_round_trip_preserves_launch_rules() {
        let mut document = document();
        let clip = document.slots[0][0].clip().unwrap().clone();
        document.slots[0][0] = Slot::Clip(SessionClip {
            launch: LaunchSettings {
                mode: LaunchMode::Toggle,
                condition: LaunchCondition::Every { step: 3, total: 4 },
                fill: FillRule::Only,
                follow: FollowAction::Next,
                ..clip.launch
            },
            ..clip
        });
        let text = ron::to_string(&document).unwrap();
        let restored: SessionDocument = ron::from_str(&text).unwrap();
        assert_eq!(restored, document);
    }

    #[test]
    fn layout_keeps_scene_and_slot_rows_aligned() {
        let document = document();
        let state = SessionViewState::default();
        let layout = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &state,
        );
        for scene in 0..document.scenes.len() {
            assert_eq!(layout.slot(0, scene).top(), layout.scene(scene).top());
            assert_eq!(layout.slot(0, scene).bottom(), layout.scene(scene).bottom());
        }
    }

    #[test]
    fn launch_and_body_targets_are_disjoint() {
        let document = document();
        let state = SessionViewState::default();
        let layout = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &state,
        );
        let launch = layout.slot_launch(0, 0);
        let body = layout.slot_body(0, 0);
        assert_eq!(launch.right(), body.left());
        assert!(!launch.contains(body.center()));
        assert!(!body.contains(launch.center()));
    }

    #[test]
    fn compact_density_changes_rows_not_hit_target_width() {
        let document = document();
        let comfortable = SessionViewState::default();
        let mut compact = comfortable.clone();
        compact.density = SessionDensity::Compact;
        let a = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &comfortable,
        );
        let b = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &compact,
        );
        assert!(b.slot_height < a.slot_height);
        assert_eq!(b.slot_launch(0, 0).width(), LAUNCH_WIDTH);
    }

    #[test]
    fn mixer_height_is_clamped_without_crushing_grid() {
        let document = document();
        let state = SessionViewState {
            mixer_height: 10_000.0,
            ..SessionViewState::default()
        };
        let layout = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &state,
        );
        assert!(layout.mixer_height <= view().height() * 0.55 + f32::EPSILON);
        assert!(layout.rows_viewport().height() > 0.0);
    }

    #[test]
    fn slot_hit_testing_respects_the_rows_viewport() {
        let document = document();
        let state = SessionViewState::default();
        let layout = SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            document.scenes.len(),
            &state,
        );
        assert_eq!(
            layout.slot_at(layout.slot_body(1, 2).center()),
            Some((1, 2))
        );
        assert_eq!(layout.slot_at(layout.header(0).center()), None);
        assert_eq!(layout.slot_at(layout.mixer(0).center()), None);
    }

    /// ONE TRACK, ONE ACTIVE SLOT — the first acceptance rule in the
    /// functional spec, and the one every other launch rule assumes.
    ///
    /// Checked across the two ways a track can be told to play twice: a
    /// second scene launched over the first, and a slot launched over a
    /// scene. Both must leave exactly one playing state and exactly one
    /// queued action per track, never a pair.
    #[test]
    fn one_track_never_holds_two_active_slots() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime
            .queue_scene(&document, 0, clock(0))
            .expect("scene 0 queues");
        runtime
            .queue_scene(&document, 1, clock(0))
            .expect("scene 1 queues");
        // The second scene took the tracks it covers off the first, so no
        // track is named twice across the whole queue.
        let mut named: Vec<usize> = runtime
            .pending
            .iter()
            .flat_map(|launch| launch.operations.iter().map(|op| op.track))
            .collect();
        let before = named.len();
        named.sort_unstable();
        named.dedup();
        assert_eq!(named.len(), before, "a track is queued twice");

        runtime.apply_due(u64::MAX);
        assert!(
            runtime.tracks.iter().all(|track| !matches!(
                track.playback,
                PlaybackState::Playing { .. }
            ) || track.pending.is_none()),
            "a playing track is also queued to start again"
        );

        // And a direct slot launch over a running scene replaces rather
        // than adds.
        runtime
            .queue_clip(&document, 0, 1, clock(0), true)
            .expect("slot queues");
        let for_track_0 = runtime
            .pending
            .iter()
            .flat_map(|launch| launch.operations.iter())
            .filter(|op| op.track == 0)
            .count();
        assert_eq!(for_track_0, 1, "track 0 has two queued actions");
        runtime.apply_due(u64::MAX);
        let playing = runtime
            .tracks
            .iter()
            .filter(|track| matches!(track.playback, PlaybackState::Playing { .. }))
            .count();
        assert!(playing <= runtime.tracks.len());
    }

    /// STOP LOSES TO START ON ONE BOUNDARY.
    ///
    /// Two actions for one track landing on the same sample are ordered
    /// stop-then-start, so what survives is the start. Left to insertion
    /// order the winner would be whichever the caller built first, and a
    /// launch you pressed would silently do nothing — the vanishing-note
    /// bug, in Session clothes.
    #[test]
    fn a_start_and_a_stop_on_one_boundary_leave_the_track_playing() {
        let start = TrackLaunchOp {
            track: 0,
            action: PendingTrackAction::Start {
                scene: SceneId(1),
                clip: ClipId(100),
            },
        };
        let stop = TrackLaunchOp {
            track: 0,
            action: PendingTrackAction::Stop,
        };

        // Either order in, the same order out.
        for pair in [
            vec![stop.clone(), start.clone()],
            vec![start.clone(), stop.clone()],
        ] {
            let mut operations = pair;
            collapse_operations(&mut operations);
            assert_eq!(operations.len(), 1, "one action per track");
            assert!(
                starts_sound(&operations[0].action),
                "the start must be what survives"
            );
        }

        // Other tracks are untouched by the collapse, and the result is
        // in track order for the caller that walks it.
        let mut operations = vec![
            TrackLaunchOp {
                track: 2,
                action: PendingTrackAction::Stop,
            },
            stop,
            start,
            TrackLaunchOp {
                track: 1,
                action: PendingTrackAction::Arrangement,
            },
        ];
        collapse_operations(&mut operations);
        assert_eq!(
            operations.iter().map(|op| op.track).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(starts_sound(&operations[0].action));
        assert_eq!(operations[1].action, PendingTrackAction::Arrangement);
        assert_eq!(operations[2].action, PendingTrackAction::Stop);
    }

    /// A DECISION IS BAKED WHEN IT IS QUEUED, not read again at the
    /// boundary.
    ///
    /// The spec says every random and conditional decision is made
    /// green-side and compiled into the pending transaction. So flipping
    /// Fill after a launch is queued must not rewrite what that launch
    /// does — only what the NEXT one does. A Fill that reached backwards
    /// would make a performance unrepeatable and a capture untruthful.
    #[test]
    fn changing_fill_after_queueing_does_not_rewrite_the_decision() {
        let mut document = document();
        // A clip that only plays during Fill.
        if let Slot::Clip(clip) = &mut document.slots[0][0] {
            clip.launch.fill = FillRule::Only;
            clip.launch.fallback = EmptyBehavior::Stop;
        }
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.fill = FillState::Latched;
        let launch = runtime
            .queue_scene(&document, 0, clock(0))
            .expect("scene queues");
        let queued = launch
            .operations
            .iter()
            .find(|op| op.track == 0)
            .expect("track 0 has an action")
            .action;
        assert!(starts_sound(&queued), "Fill was on, so the clip starts");

        // Fill off, before the boundary. The queued transaction stands.
        runtime.fill = FillState::Off;
        let still = runtime
            .pending
            .iter()
            .flat_map(|launch| launch.operations.iter())
            .find(|op| op.track == 0)
            .expect("still queued")
            .action;
        assert_eq!(still, queued, "the decision was rewritten after the fact");

        // And the NEXT launch sees the new state: the same clip now falls
        // back to its stop rather than starting.
        let next = runtime
            .queue_scene(&document, 0, clock(0))
            .expect("scene queues again");
        let action = next
            .operations
            .iter()
            .find(|op| op.track == 0)
            .expect("track 0 has an action");
        assert_eq!(action.action, PendingTrackAction::Stop);
    }

    /// A LOCK IS AN IDENTITY, NEVER A POSITION.
    ///
    /// The spec is explicit: locks address a stable track/device instance
    /// and "never retarget by position". Reordering a chain must not make
    /// a filter lock land on a delay — which is what an index would do,
    /// silently, the first time anyone moved a device.
    #[test]
    fn a_scene_lock_addresses_a_device_by_id_not_by_chain_position() {
        let mut document = document();
        let lock = SceneLock {
            track: TrackId(20),
            device: Some(DeviceId(7)),
            parameter: 3,
            value: 0.25,
        };
        document.scenes[0].locks = vec![lock.clone()];
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let launch = runtime
            .queue_scene(&document, 0, clock(0))
            .expect("scene queues");
        assert_eq!(launch.locks, vec![lock.clone()]);

        // Reordering the tracks moves the COLUMN, not the lock's target:
        // the lock still names track 20 and device 7.
        document.move_track(0, 1);
        let mut runtime = SessionRuntime::new(document.tracks.len());
        let scene = document
            .scenes
            .iter()
            .position(|scene| scene.locks.contains(&lock))
            .expect("the scene still carries its lock");
        let launch = runtime
            .queue_scene(&document, scene, clock(0))
            .expect("scene queues");
        assert_eq!(launch.locks, vec![lock]);
        assert_eq!(launch.locks[0].track, TrackId(20));
        assert_eq!(launch.locks[0].device, Some(DeviceId(7)));
    }

    /// Drive the view with KEY events rather than pointer ones.
    ///
    /// `probe` sends pointer input only, which is the right shape for
    /// gesture tests and no use at all for "reachable without a pointer".
    fn key_path(
        document: &SessionDocument,
        runtime: &SessionRuntime,
        state: &mut SessionViewState,
        keys: &[(egui::Modifiers, egui::Key)],
    ) -> Vec<SessionIntent> {
        let context = egui::Context::default();
        let colors = SessionColors::default();
        let clipboard = SessionClipboard::default();
        let rect = view();
        let mut out = Vec::new();
        for (modifiers, key) in keys {
            let mut frame = Vec::new();
            let mut run = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        rect.max.to_vec2() + egui::vec2(64.0, 64.0),
                    )),
                    events: vec![
                        egui::Event::Key {
                            key: *key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: *modifiers,
                        },
                        egui::Event::Key {
                            key: *key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: *modifiers,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_width(rect.width());
                    child.set_height(rect.height());
                    frame = show_session(
                        &mut child,
                        document,
                        runtime,
                        clock(12_000),
                        &clipboard,
                        state,
                        &colors,
                    )
                    .intents;
                },
            );
            run.textures_delta.clear();
            out.extend(frame);
        }
        out
    }

    /// EVERY COMMAND IS REACHABLE WITHOUT A POINTER.
    ///
    /// A performance surface that needs a mouse is a surface you cannot
    /// use while playing. Navigation, launch, delete, Fill and Escape all
    /// have to work from the keys alone.
    #[test]
    fn every_command_is_reachable_from_the_keyboard() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState {
            owns_keyboard: true,
            ..SessionViewState::default()
        };

        // Arrows walk the grid and select as they go.
        let intents = key_path(
            &document,
            &runtime,
            &mut state,
            &[
                (egui::Modifiers::NONE, egui::Key::ArrowDown),
                (egui::Modifiers::NONE, egui::Key::ArrowRight),
            ],
        );
        assert!(intents.contains(&SessionIntent::SelectSlot { track: 0, scene: 1 }));
        assert!(intents.contains(&SessionIntent::SelectSlot { track: 1, scene: 1 }));
        assert_eq!(
            state.selection,
            Some(GridSelection::Slot { track: 1, scene: 1 })
        );

        // Enter launches whatever the grid is on.
        let intents = key_path(
            &document,
            &runtime,
            &mut state,
            &[(egui::Modifiers::NONE, egui::Key::Enter)],
        );
        assert!(intents.contains(&SessionIntent::LaunchSlot { track: 1, scene: 1 }));

        // Delete, Fill and Escape.
        let intents = key_path(
            &document,
            &runtime,
            &mut state,
            &[(egui::Modifiers::NONE, egui::Key::Delete)],
        );
        assert!(intents.contains(&SessionIntent::DeleteSelection));

        let intents = key_path(
            &document,
            &runtime,
            &mut state,
            &[(egui::Modifiers::NONE, egui::Key::F)],
        );
        assert!(intents.contains(&SessionIntent::SetFill(FillState::Momentary)));

        let intents = key_path(
            &document,
            &runtime,
            &mut state,
            &[(egui::Modifiers::NONE, egui::Key::Escape)],
        );
        assert!(intents.contains(&SessionIntent::ClearSelection));
        assert_eq!(state.selection, None, "Escape leaves the grid unselected");

        // AND IT STANDS DOWN when it does not own the keyboard, so the
        // rest of the application's keys are not swallowed.
        let mut elsewhere = SessionViewState::default();
        let intents = key_path(
            &document,
            &runtime,
            &mut elsewhere,
            &[(egui::Modifiers::NONE, egui::Key::ArrowDown)],
        );
        assert!(
            !intents
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SelectSlot { .. })),
            "the grid moved without owning the keyboard"
        );
    }

    /// FOCUS STAYS IN THE GRID when Select on Launch is off.
    ///
    /// The option exists so a performer can fire clips without the
    /// selection chasing the pointer around — and a launch that moved the
    /// selection anyway would send the next arrow key somewhere the
    /// performer was not looking.
    #[test]
    fn launching_without_select_on_launch_leaves_the_selection_alone() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &SessionViewState::default());
        // Track 0 scene 1 holds a real clip; the Continue slot beside it
        // would refuse the launch for an unrelated reason.
        let rail = layout.slot_launch(0, 1).center();

        // On: the launch rail selects as well as launching.
        let mut state = SessionViewState::default();
        assert!(state.select_on_launch, "on by default");
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(rail),
        ));
        assert!(intents.contains(&SessionIntent::LaunchSlot { track: 0, scene: 1 }));
        assert_eq!(
            state.selection,
            Some(GridSelection::Slot { track: 0, scene: 1 })
        );

        // Off: the same click launches and the selection does not move.
        let mut state = SessionViewState {
            select_on_launch: false,
            selection: Some(GridSelection::Slot { track: 1, scene: 0 }),
            ..SessionViewState::default()
        };
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(rail),
        ));
        assert!(intents.contains(&SessionIntent::LaunchSlot { track: 0, scene: 1 }));
        assert_eq!(
            state.selection,
            Some(GridSelection::Slot { track: 1, scene: 0 }),
            "the launch moved the selection"
        );
    }

    #[test]
    fn drawing_at_rest_emits_no_intents() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let at = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state)
            .slot_body(0, 0)
            .center();
        let frames = render_path(&document, &runtime, &mut state, &[probe::Step::moved(at)]);
        assert!(flattened(&frames).is_empty());
    }

    #[test]
    fn clicking_launch_rail_launches_without_select_intent() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.slot_launch(0, 0).center()),
        );
        let intents = flattened(&frames);
        assert!(intents.contains(&SessionIntent::LaunchSlot { track: 0, scene: 0 }));
        assert!(!intents.contains(&SessionIntent::SelectSlot { track: 0, scene: 0 }));
    }

    #[test]
    fn clicking_slot_body_selects_without_launching() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.slot_body(0, 0).center()),
        );
        let intents = flattened(&frames);
        assert!(intents.contains(&SessionIntent::SelectSlot { track: 0, scene: 0 }));
        assert!(!intents.contains(&SessionIntent::LaunchSlot { track: 0, scene: 0 }));
    }

    #[test]
    fn clicking_empty_continue_rail_does_nothing() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.slot_launch(1, 1).center()),
        );
        assert!(flattened(&frames).is_empty());
    }

    #[test]
    fn clicking_empty_stop_rail_stops_exactly_that_track() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.slot_launch(0, 2).center()),
        );
        assert!(flattened(&frames).contains(&SessionIntent::StopTrack {
            track: 0,
            immediate: false,
        }));
    }

    #[test]
    fn scene_body_and_launch_rail_have_separate_ownership() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let body = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.scene_body(0).center()),
        );
        assert!(flattened(&body).contains(&SessionIntent::SelectScene(0)));
        let launch = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(layout.scene_launch(0).center()),
        );
        let intents = flattened(&launch);
        assert!(intents.contains(&SessionIntent::LaunchScene(0)));
        assert!(!intents.contains(&SessionIntent::SelectScene(0)));
    }

    #[test]
    fn track_reorder_grip_does_not_select_the_header() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(
                layout.track_grip(0).center(),
                layout.track_grip(1).center(),
                6,
            ),
        );
        let intents = flattened(&frames);
        assert!(intents.contains(&SessionIntent::ReorderTrack { from: 0, to: 1 }));
        assert!(!intents.contains(&SessionIntent::SelectTrack(0)));
    }

    #[test]
    fn scene_reorder_grip_does_not_launch_or_select() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(
                layout.scene_grip(0).center(),
                layout.scene_grip(3).center(),
                8,
            ),
        );
        let intents = flattened(&frames);
        assert!(intents.contains(&SessionIntent::ReorderScene { from: 0, to: 3 }));
        assert!(!intents.contains(&SessionIntent::SelectScene(0)));
        assert!(!intents.contains(&SessionIntent::LaunchScene(0)));
    }

    #[test]
    fn slot_drag_stays_owned_by_body_and_never_launches() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(
                layout.slot_body(0, 0).center(),
                layout.slot_body(0, 2).center(),
                8,
            ),
        );
        let intents = flattened(&frames);
        assert!(intents.contains(&SessionIntent::MoveSlots {
            from: (0, 0),
            to: (0, 2),
            copy: false,
        }));
        assert!(
            !intents
                .iter()
                .any(|intent| matches!(intent, SessionIntent::LaunchSlot { .. }))
        );
    }

    #[test]
    fn pan_gesture_emits_pan_and_not_volume() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let pan = MixerStrip::new(layout.mixer(0), StripContent::default())
            .pan
            .expect("the default strip is tall enough for a pan bar");
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(pan.left_center(), pan.right_center(), 6),
        );
        let intents = flattened(&frames);
        assert!(
            intents
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SetTrackPan { track: 0, .. }))
        );
        assert!(
            !intents
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SetTrackVolume { .. }))
        );
    }

    // ------------------------------------------------- the mixer strip ---

    fn strip() -> MixerStrip {
        let state = SessionViewState::default();
        MixerStrip::new(
            SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state).mixer(0),
            StripContent::default(),
        )
    }

    /// THE TAPER IS ONE FUNCTION, ASKED FOUR TIMES.
    ///
    /// The handle, the ticks, the meter and the drag all place a value on
    /// the same rail. If they disagreed the fader would read one number
    /// and sound like another, and nothing on screen would say which was
    /// lying — so the pair is checked for being a pair.
    #[test]
    fn the_fader_taper_agrees_with_itself() {
        for step in 0..=20 {
            let normalized = step as f32 / 20.0;
            let round = fader_normalized(fader_volume(normalized));
            assert!(
                (round - normalized).abs() < 1e-4,
                "{normalized} came back as {round}"
            );
        }
        assert!((fader_volume(fader_position(0.0)) - 1.0).abs() < 1e-4);
        // Unity sits well up the rail rather than at the top, which is
        // what leaves room to push a mix as well as pull it.
        assert!(fader_position(0.0) > 0.6 && fader_position(0.0) < 0.8);
        assert!(fader_position(6.0) > fader_position(0.0));
        assert!(fader_position(-48.0) < fader_position(-24.0));
        assert_eq!(db_text(1.0), "+0.0");
        assert_eq!(db_text(0.0), "-inf");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-1.0), "50L");
        assert_eq!(pan_text(0.5), "25R");
    }

    /// EVERY TARGET IS INSIDE THE STRIP, AND NO TWO OVERLAP.
    ///
    /// One draggable target is one egui interaction — the device UI
    /// contract's first rule — and two targets sharing a pixel is how a
    /// fader ends up taking a pan's drag. Checked as geometry, because
    /// that is the level the bug lives at.
    #[test]
    fn strip_targets_are_disjoint_and_contained() {
        let strip = strip();
        let mut targets = vec![
            ("mute", strip.mute),
            ("solo", strip.solo),
            ("fader", strip.fader),
        ];
        for (name, rect) in [("pan", strip.pan), ("peak", strip.peak)] {
            if let Some(rect) = rect {
                targets.push((name, rect));
            }
        }
        for (name, rect) in &targets {
            assert!(
                strip.rect.contains_rect(*rect),
                "{name} escaped the strip: {rect:?} outside {:?}",
                strip.rect
            );
        }
        for (first, one) in &targets {
            for (second, two) in &targets {
                if first != second {
                    assert!(
                        !one.intersects(*two),
                        "{first} and {second} share ground: {one:?} and {two:?}"
                    );
                }
            }
        }
        // The meter is drawn, not dragged — but it must still not sit
        // under the fader, which is the whole reason it moved beside it.
        assert!(!strip.meter.intersects(strip.fader));
    }

    /// A SHORT STRIP LOSES THINGS, AND NEVER TAKES ONE BACK.
    ///
    /// The seam can drag the mixer down to its floor, and at the floor
    /// the strip must still be a strip: two buttons and a fader. The
    /// property worth pinning is MONOTONICITY — a taller strip never
    /// shows less than a shorter one. A layout that flickered a row off
    /// as the seam moved down and on again a pixel later would look like
    /// a rendering fault rather than a budget.
    #[test]
    fn a_short_strip_sheds_rows_and_never_takes_one_back() {
        // The sweep runs UPWARDS, so the thing to catch is a row that
        // was there and then was not.
        let mut arrived: [Option<f32>; 3] = [None; 3];
        let mut height = MIXER_HEIGHT_MIN;
        while height <= MIXER_HEIGHT_MAX {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, height));
            let strip = MixerStrip::new(rect, StripContent::default());
            assert!(
                strip.fader.height() >= FADER_MIN_HEIGHT - 0.01,
                "at {height}"
            );
            assert!(strip.rect.contains_rect(strip.mute), "at {height}");
            assert!(strip.rect.contains_rect(strip.fader), "at {height}");
            // A number with no number above it is a stray: the peak row
            // only exists once the volume readout it sits under does.
            if strip.readout.is_none() {
                assert!(
                    strip.peak.is_none(),
                    "a peak outlived its readout at {height}"
                );
            }
            for (index, (name, present)) in [
                ("pan", strip.pan.is_some()),
                ("readout", strip.readout.is_some()),
                ("peak", strip.peak.is_some()),
            ]
            .into_iter()
            .enumerate()
            {
                match (present, arrived[index]) {
                    (true, None) => arrived[index] = Some(height),
                    (false, Some(at)) => {
                        panic!("the {name} row vanished at {height} after arriving at {at}")
                    }
                    _ => {}
                }
            }
            height += 1.0;
        }
        // And the floor really is lean: something had to go.
        let floor = MixerStrip::new(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, MIXER_HEIGHT_MIN)),
            StripContent::default(),
        );
        assert!(floor.peak.is_none() && floor.pan.is_none());
    }

    /// SHIFT MEANS LESS, NOT ELSEWHERE.
    ///
    /// This is the test that proves the fader moves relatively at all: an
    /// absolute control would land on the same value either way, because
    /// wherever the pointer is would BE the value.
    #[test]
    fn a_fine_drag_moves_the_fader_less_than_a_plain_one() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let fader = strip().fader;
        let from = fader.center();
        let to = egui::pos2(from.x, from.y - 30.0);

        let excursion = |mods: egui::Modifiers| {
            let mut state = SessionViewState::default();
            let frames = render_path(
                &document,
                &runtime,
                &mut state,
                &probe::drag_path_holding(from, to, 6, mods),
            );
            flattened(&frames)
                .into_iter()
                .filter_map(|intent| match intent {
                    SessionIntent::SetTrackVolume { track: 0, value } => {
                        Some((value - document.tracks[0].volume).abs())
                    }
                    _ => None,
                })
                .fold(0.0_f32, f32::max)
        };
        let plain = excursion(egui::Modifiers::NONE);
        let fine = excursion(egui::Modifiers::SHIFT);
        assert!(plain > 0.0, "the plain drag moved nothing");
        assert!(fine > 0.0, "the fine drag moved nothing at all");
        assert!(
            fine < plain * 0.5,
            "shift barely helped: {fine} against {plain}"
        );
    }

    /// A DOUBLE CLICK IS THE WAY BACK.
    #[test]
    fn a_double_click_returns_the_fader_to_unity_and_the_pan_to_centre() {
        let mut document = document();
        document.tracks[0].volume = 0.25;
        document.tracks[0].pan = -0.8;
        let runtime = SessionRuntime::new(document.tracks.len());
        let strip = strip();

        let mut state = SessionViewState::default();
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::double_click_path(strip.fader.center()),
        ));
        assert!(intents.contains(&SessionIntent::SetTrackVolume {
            track: 0,
            value: 1.0
        }));

        let mut state = SessionViewState::default();
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::double_click_path(strip.pan.unwrap().center()),
        ));
        assert!(intents.contains(&SessionIntent::SetTrackPan {
            track: 0,
            value: 0.0
        }));
    }

    /// PLAIN SOLO IS EXCLUSIVE; CTRL BUILDS A SET.
    ///
    /// Live's way round, and the useful one — but the two must be
    /// DIFFERENT intents, because the app resolves them differently and
    /// a single toggle could not express "and nothing else".
    #[test]
    fn a_solo_click_is_exclusive_unless_ctrl_is_held() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let solo = strip().solo.center();

        let mut state = SessionViewState::default();
        let plain = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(solo),
        ));
        assert!(
            plain.contains(&SessionIntent::SoloTrackExclusive(0)),
            "{plain:?}"
        );
        assert!(!plain.contains(&SessionIntent::ToggleTrackSolo(0)));

        let mut state = SessionViewState::default();
        let held = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path_holding(solo, egui::Modifiers::COMMAND),
        ));
        assert!(
            held.contains(&SessionIntent::ToggleTrackSolo(0)),
            "{held:?}"
        );
        assert!(!held.contains(&SessionIntent::SoloTrackExclusive(0)));
    }

    /// THE PEAK NUMBER HOLDS, AND CLEARS WHERE IT IS READ.
    ///
    /// A held peak that could only be cleared somewhere else would be a
    /// number nobody trusts. Clicking the number is the whole gesture.
    #[test]
    fn the_peak_readout_holds_a_level_and_clears_where_it_is_shown() {
        let document = document();
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.tracks[0].peak = 0.6;
        runtime.tracks[0].clipped = true;
        let peak = strip().peak.expect("the default strip shows a peak row");

        let mut state = SessionViewState::default();
        // A frame with no click: the hold takes the level and keeps it
        // even after the signal falls away.
        render_path(
            &document,
            &runtime,
            &mut state,
            &[probe::Step::moved(peak.center())],
        );
        assert!(
            (state.peak_hold[0] - 0.6).abs() < 1e-5,
            "{:?}",
            state.peak_hold
        );
        let quiet = SessionRuntime::new(document.tracks.len());
        render_path(
            &document,
            &quiet,
            &mut state,
            &[probe::Step::moved(peak.center())],
        );
        assert!(
            (state.peak_hold[0] - 0.6).abs() < 1e-5,
            "the hold did not hold"
        );

        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(peak.center()),
        ));
        assert!(
            intents.contains(&SessionIntent::ClearClipHold(0)),
            "{intents:?}"
        );
        assert_eq!(state.peak_hold[0], 0.0, "the hold survived being cleared");
    }

    // ------------------------------------------------ sends and returns ---

    /// The same document with `count` returns, and a track that already
    /// sends to the first of them.
    fn with_returns(count: usize) -> SessionDocument {
        let mut document = document();
        document.returns = (0..count)
            .map(|index| SessionReturn {
                name: format!("Return {}", daw_return_letter(index)),
                ..SessionReturn::default()
            })
            .collect();
        document.tracks[0].sends = vec![0.25];
        document
    }

    fn layout_of(document: &SessionDocument, state: &SessionViewState) -> SessionLayout {
        SessionLayout::new(
            view(),
            document.tracks.len(),
            document.returns.len(),
            DEFAULT_SCENES,
            state,
        )
    }

    /// A SEND ROW PER RETURN, AND A COUNT FOR THE ONES THAT DO NOT FIT.
    ///
    /// The block is the strip's variable row, so what matters is that it
    /// never lies: every send is either drawn or counted, and the two
    /// always add up to the number of returns there are.
    #[test]
    fn every_send_is_either_drawn_or_counted() {
        for count in 0..=8 {
            for height in [MIXER_HEIGHT_MIN, 120.0, 156.0, 240.0, MIXER_HEIGHT_MAX] {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, height));
                let strip = MixerStrip::new(
                    rect,
                    StripContent {
                        sends: count,
                        io: false,
                    },
                );
                assert_eq!(
                    strip.sends.len() + strip.sends_hidden,
                    count,
                    "{count} sends went missing at height {height}"
                );
                for row in &strip.sends {
                    assert!(
                        strip.rect.contains_rect(*row),
                        "a send row escaped the strip at height {height}"
                    );
                    assert!(!row.intersects(strip.fader), "a send row sat on the fader");
                }
            }
        }
    }

    /// THE SEND BLOCK ONLY GROWS.
    ///
    /// The fixed rows are strictly ranked so they cannot flicker; the
    /// sends are variable, so they earn the same guarantee a different
    /// way — they do not begin until every fixed row is already there,
    /// from which point nothing above them can arrive and take their
    /// space back.
    #[test]
    fn the_send_block_never_shrinks_as_the_strip_grows() {
        let mut previous = 0;
        let mut height = MIXER_HEIGHT_MIN;
        while height <= MIXER_HEIGHT_MAX {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, height));
            let rows = MixerStrip::new(
                rect,
                StripContent {
                    sends: 8,
                    io: false,
                },
            )
            .sends
            .len();
            assert!(
                rows >= previous,
                "the send block lost a row at height {height}: {previous} then {rows}"
            );
            previous = rows;
            height += 1.0;
        }
        assert!(previous > 0, "the sends never appeared at all");
    }

    /// A SEND OPENS BY DRAG AND SHUTS BY DOUBLE CLICK.
    #[test]
    fn a_send_opens_by_drag_and_shuts_by_double_click() {
        let document = with_returns(2);
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = layout_of(&document, &state);
        let strip = MixerStrip::new(
            layout.mixer(0),
            StripContent {
                sends: document.returns.len(),
                io: false,
            },
        );
        let row = *strip.sends.first().expect("two returns, so a send row");

        let opened = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(row.center(), row.right_center(), 6),
        ));
        let moved: Vec<f32> = opened
            .iter()
            .filter_map(|intent| match intent {
                SessionIntent::SetTrackSend {
                    track: 0,
                    index: 0,
                    value,
                } => Some(*value),
                _ => None,
            })
            .collect();
        assert!(!moved.is_empty(), "the send did not move: {opened:?}");
        assert!(
            moved
                .iter()
                .all(|value| *value > document.tracks[0].sends[0]),
            "dragging right must open a send: {moved:?}"
        );
        assert!(
            moved.iter().all(|value| *value <= 1.0),
            "a send stops at all of it: {moved:?}"
        );
        // And it moved the FIRST send, not the second one under it.
        assert!(
            !opened
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SetTrackSend { index: 1, .. }))
        );

        let mut state = SessionViewState::default();
        let shut = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::double_click_path(row.center()),
        ));
        assert!(shut.contains(&SessionIntent::SetTrackSend {
            track: 0,
            index: 0,
            value: 0.0
        }));
    }

    /// A RETURN COLUMN OWNS NO CLIP SLOT.
    ///
    /// It sits in the mixer band after the last track, and the grid above
    /// it is drawn ground and nothing else — which is where Live puts
    /// one, and what stops a launch gesture finding a bus.
    #[test]
    fn a_return_column_owns_no_clip_slot() {
        let document = with_returns(2);
        let state = SessionViewState::default();
        let layout = layout_of(&document, &state);
        let strip = layout.return_mixer(0);
        assert!(
            strip.left() >= layout.mixer(1).right() - 0.01,
            "the returns sat on top of the tracks"
        );
        assert_eq!(
            layout.slot_at(egui::pos2(strip.center().x, layout.slot(0, 0).center().y)),
            None,
            "a return column answered with a slot"
        );
        assert!(layout.return_divider().is_some());
        // And they are reachable: a mixer you cannot scroll to is a
        // mixer that does not have them.
        let narrow = SessionLayout::new(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 620.0)),
            8,
            4,
            DEFAULT_SCENES,
            &state,
        );
        assert!(narrow.max_scroll_x() > 0.0);
    }

    /// A RETURN'S STRIP IS A LEVEL COLUMN AND A MUTE, AND NO SENDS.
    ///
    /// A return that sent would be a feedback loop the graph cannot
    /// compile, so the control simply is not there to reach for.
    #[test]
    fn a_return_strip_mutes_faders_and_selects_but_never_sends() {
        let document = with_returns(1);
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.returns = vec![TrackRuntime::default()];
        let mut state = SessionViewState::default();
        let layout = layout_of(&document, &state);
        let strip = MixerStrip::new(layout.return_mixer(0), StripContent::default());
        assert!(strip.sends.is_empty() && strip.sends_hidden == 0);

        let muted = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(strip.mute.center()),
        ));
        assert!(
            muted.contains(&SessionIntent::ToggleReturnMute(0)),
            "{muted:?}"
        );
        assert!(
            !muted
                .iter()
                .any(|intent| matches!(intent, SessionIntent::ToggleTrackMute(_))),
            "the return's mute reached a track"
        );

        let mut state = SessionViewState::default();
        let faded = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(
                strip.fader.center(),
                egui::pos2(strip.fader.center().x, strip.fader.top() + 4.0),
                6,
            ),
        ));
        assert!(
            faded
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SetReturnVolume { index: 0, .. })),
            "the return's fader did nothing: {faded:?}"
        );
        assert!(
            !faded
                .iter()
                .any(|intent| matches!(intent, SessionIntent::SetTrackVolume { .. })),
            "the return's fader moved a track"
        );

        // Its head selects it, which is what points the rack at its chain.
        let mut state = SessionViewState::default();
        let head = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(egui::pos2(strip.solo.center().x, strip.solo.center().y)),
        ));
        assert!(head.contains(&SessionIntent::SelectReturn(0)), "{head:?}");
    }

    /// A return's peak hold is its OWN.
    ///
    /// Holds are kept in one table indexed by strip, and a return whose
    /// hold shared an index with a track would show that track's loudest
    /// moment as its own.
    #[test]
    fn a_returns_peak_hold_is_not_a_tracks() {
        let document = with_returns(1);
        let mut runtime = SessionRuntime::new(document.tracks.len());
        runtime.returns = vec![TrackRuntime {
            peak: 0.8,
            ..TrackRuntime::default()
        }];
        let mut state = SessionViewState::default();
        render_path(
            &document,
            &runtime,
            &mut state,
            &[probe::Step::moved(egui::pos2(-50.0, -50.0))],
        );
        assert_eq!(
            state.peak_hold.first().copied(),
            Some(0.0),
            "a silent track held something"
        );
        assert!(
            state
                .peak_hold
                .get(RETURN_HOLD_BASE)
                .is_some_and(|hold| (*hold - 0.8).abs() < 1e-5),
            "the return's hold is not where the strip looks for it"
        );
    }

    // ------------------------------------------------- input routing ---

    /// A note lane has no input path in the engine at all, so it gets no
    /// control rather than one that could only ever be silence.
    #[test]
    fn only_an_audio_lane_carries_an_io_row() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, 200.0));
        let audio = MixerStrip::new(rect, StripContent { sends: 0, io: true });
        assert!(audio.monitor.is_some() && audio.route.is_some());
        assert!(!audio.monitor.unwrap().intersects(audio.route.unwrap()));
        assert!(audio.rect.contains_rect(audio.route.unwrap()));
        // Both or neither: a route you cannot hear and a monitor with
        // nothing to hear are each half a control.
        let note = MixerStrip::new(rect, StripContent::default());
        assert!(note.monitor.is_none() && note.route.is_none());
        for height in [MIXER_HEIGHT_MIN, 96.0, 130.0, MIXER_HEIGHT_MAX] {
            let strip = MixerStrip::new(
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(112.0, height)),
                StripContent { sends: 2, io: true },
            );
            assert_eq!(
                strip.monitor.is_some(),
                strip.route.is_some(),
                "half an I/O row at height {height}"
            );
        }
    }

    /// THE ROUTE CYCLES FORWARD, AND SHIFT WALKS IT BACK.
    #[test]
    fn the_route_cell_cycles_and_the_monitor_toggles() {
        let mut document = document();
        document.input_channels = 2;
        // The second lane is the audio one; the first is notes and has
        // no row at all.
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let layout = layout_of(&document, &state);
        let strip = MixerStrip::new(layout.mixer(1), StripContent { sends: 0, io: true });
        let route = strip.route.expect("an audio lane is routable");

        let forward = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(route.center()),
        ));
        assert!(
            forward.contains(&SessionIntent::CycleTrackInput {
                track: 1,
                back: false
            }),
            "{forward:?}"
        );

        let mut state = SessionViewState::default();
        let back = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path_holding(route.center(), egui::Modifiers::SHIFT),
        ));
        assert!(
            back.contains(&SessionIntent::CycleTrackInput {
                track: 1,
                back: true
            }),
            "{back:?}"
        );

        let mut state = SessionViewState::default();
        let monitored = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(strip.monitor.unwrap().center()),
        ));
        assert!(
            monitored.contains(&SessionIntent::ToggleTrackMonitor(1)),
            "{monitored:?}"
        );
        // And the note lane beside it was not routed by any of that.
        assert!(!monitored.iter().any(|intent| matches!(
            intent,
            SessionIntent::ToggleTrackMonitor(0) | SessionIntent::CycleTrackInput { track: 0, .. }
        )));
    }

    /// A SEAM IS NOT A BUTTON. Dragging the mixer's resize seam must
    /// resize it and touch nothing underneath — the mute of the first
    /// mixer strip sits a few pixels below, and a seam that leaked into
    /// it would silence a track while the performer was only making room
    /// to see one.
    #[test]
    fn the_mixer_seam_does_not_operate_the_row_beneath_it() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let before = state.mixer_height;
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        let seam = layout.mixer_seam().center();
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            // Upwards, which makes the mixer taller and drags the pointer
            // straight across the strip below where it started.
            &probe::drag_path(seam, egui::pos2(seam.x, seam.y - 60.0), 8),
        ));
        assert!(
            state.mixer_height > before,
            "the seam did not resize: {} then {}",
            before,
            state.mixer_height
        );
        assert!(
            intents.is_empty(),
            "the seam reached the strip beneath it: {intents:?}"
        );
    }

    /// A PRESS ON EMPTY GROUND MOVES NOTHING.
    ///
    /// This is the first bug in the device UI contract's failure list —
    /// a widget that asked "what am I nearest?" and acted on whatever it
    /// last touched. Pressed on ground that owns nothing, this view must
    /// answer with silence, and must not disturb a selection made before.
    #[test]
    fn a_press_on_empty_ground_moves_nothing() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let selected = GridSelection::Slot { track: 0, scene: 0 };
        let mut state = SessionViewState {
            selection: Some(selected),
            ..SessionViewState::default()
        };
        let layout = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state);
        // The gutter to the right of the last track column and left of
        // the scene column: drawn ground, owned by nothing.
        let empty = egui::pos2(
            layout.track_left(2) + 4.0,
            layout.slot_body(0, 0).center().y,
        );
        let scroll_before = (state.scroll_x, state.scroll_y);
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::click_path(empty),
        ));
        assert!(intents.is_empty(), "empty ground emitted {intents:?}");
        assert_eq!(state.selection, Some(selected), "the selection moved");
        assert_eq!((state.scroll_x, state.scroll_y), scroll_before);

        // And a DRAG from empty ground is just as quiet — it must not
        // become a rubber band, a reorder, or a slot move.
        let intents = flattened(&render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(empty, egui::pos2(empty.x + 120.0, empty.y + 80.0), 8),
        ));
        assert!(
            intents.is_empty(),
            "a drag from nowhere emitted {intents:?}"
        );
        assert_eq!(state.selection, Some(selected));
    }

    #[test]
    fn memory_morph_reaches_both_ends() {
        let document = document();
        let runtime = SessionRuntime::new(document.tracks.len());
        let mut state = SessionViewState::default();
        let strip = SessionLayout::new(view(), 2, 0, DEFAULT_SCENES, &state).control_strip();
        let m1_left = strip.right() - 4.0 - 36.0 * 2.0 - 4.0;
        let morph = egui::Rect::from_min_max(
            egui::pos2(m1_left - 74.0, strip.top() + 4.0),
            egui::pos2(m1_left - 4.0, strip.bottom() - 4.0),
        );
        let frames = render_path(
            &document,
            &runtime,
            &mut state,
            &probe::drag_path(morph.left_center(), morph.right_center(), 8),
        );
        let intents = flattened(&frames);
        assert!(intents.iter().any(|intent| {
            matches!(intent, SessionIntent::MorphMemories(value) if *value <= 0.01)
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, SessionIntent::MorphMemories(value) if *value >= 0.99)
        }));
    }
}
