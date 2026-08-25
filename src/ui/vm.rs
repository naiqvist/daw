//! The view-model: what panels are allowed to know.
//!
//! `ViewState` is a plain snapshot the app assembles once per frame from
//! engine telemetry. Panels read it and return `UiAction`s — they can look
//! at anything here and touch nothing anywhere. No engine types leak in;
//! this file must never import `crate::audio` (enforced by the layer test).

/// Domain limits panels may name instead of writing literals. These are
/// MEANING, not style — the token system's sibling, not its subject.
pub mod limits {
    pub const BPM_MIN: f64 = 40.0;
    pub const BPM_MAX: f64 = 240.0;
}

#[derive(Debug, Clone, Default)]
pub struct ViewState {
    pub engine_running: bool,
    pub playing: bool,
    pub metronome_on: bool,
    pub bpm: f64,
    pub beat: f64,
    pub position_secs: f64,
    /// Percent of the callback deadline used, current and worst.
    pub dsp_load_pct: f32,
    pub dsp_worst_pct: f32,
    pub xruns: u64,
    pub frame_ms: f32,
    pub adapter: String,
    /// Something the user must see (engine death, refused graph). One line;
    /// the status bar renders it in the danger role.
    pub notice: Option<String>,
}

impl ViewState {
    /// A plausible, fully-populated snapshot with no engine behind it.
    ///
    /// This is what makes a panel testable and previewable: the lab's gallery
    /// renders real panels against this, so panel work does not need a live
    /// audio device — and a panel that only looks right when the engine is
    /// running is a panel reaching past its contract.
    pub fn demo() -> Self {
        Self {
            engine_running: true,
            playing: true,
            metronome_on: true,
            bpm: 128.0,
            beat: 37.5,
            position_secs: 17.578,
            dsp_load_pct: 4.2,
            dsp_worst_pct: 21.7,
            xruns: 0,
            frame_ms: 6.94,
            adapter: "Vulkan / demo adapter".to_owned(),
            notice: None,
        }
    }
}

/// What a track carries.
///
/// The arrangement draws both lanes identically — what differs is what a
/// clip on the lane MEANS: notes the built-in synth plays, or audio
/// streamed from disk. It lives here, beside `ViewState`, because the
/// action vocabulary must be able to name it and `action` may not import
/// the app.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum TrackKind {
    /// Notes. A clip holds a pattern; the track's instrument plays it.
    #[default]
    Midi,
    /// Recorded or imported audio. No instrument slot — the material IS
    /// the sound.
    Audio,
}

impl TrackKind {
    pub const ALL: [Self; 2] = [Self::Midi, Self::Audio];

    /// The badge a track header shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Midi => "midi",
            Self::Audio => "audio",
        }
    }

    /// The word a fresh track's name is built from: "Audio 3".
    pub fn stem(self) -> &'static str {
        match self {
            Self::Midi => "MIDI",
            Self::Audio => "Audio",
        }
    }

    /// Can this track hold an instrument? An audio track's sound is its
    /// material, so loading a synth onto one is meaningless rather than
    /// merely unusual — the browser refuses instead of silently filling a
    /// slot nothing reads.
    pub fn takes_instrument(self) -> bool {
        matches!(self, Self::Midi)
    }
}
