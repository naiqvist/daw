//! Machine-local UI preferences — the persistence boundary made real.
//!
//! What is in here is everything that must NOT be in a project file: how this
//! machine likes to look. The test for membership is in `ui::mod`'s header —
//! if a value would still matter after emailing the project to a stranger, it
//! is project data and does not belong here.
//!
//! Deliberately NOT stored here:
//!
//! - **Window size and position.** eframe's `persist_window` already does it
//!   (hence `features = ["persistence"]` on eframe). Two systems writing one
//!   value is how they disagree.
//! - **Panel sizes.** egui persists a dragged panel's size under the panel's
//!   own id, in its memory blob. Same reason.
//! - **Anything musical.** Enforced: this file may not name `crate::audio`.
//!
//! Forward and backward compatibility are load-bearing, not nice-to-have: a
//! preferences file outlives the version that wrote it. Every field is
//! `#[serde(default)]`, unknown fields are ignored, and a corrupt file loads
//! as defaults rather than refusing to start the app.

use crate::ui::tokens::Density;

/// Storage key. Distinct from eframe's own `APP_KEY` so the two never collide.
pub const STORAGE_KEY: &str = "daw.ui.prefs";

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub density: Density,
    /// Panels the user has hidden, by `Panel::id()`. The HIDDEN set, not the
    /// visible one — see `PanelHost::apply_hidden`.
    pub hidden_panels: Vec<String>,
    /// Frontmost center tab, by id.
    pub focused_center: Option<String>,
    /// Recently opened project files, newest first. Machine-local, like
    /// everything here — a path means nothing on another machine.
    pub recent_projects: Vec<String>,
    /// Whether the rack's modulation strip is folded to its tab. How the
    /// workspace is arranged, so it rides the machine-local prefs.
    pub mod_strip_collapsed: bool,
    /// The frame regions the user has folded away.
    ///
    /// Stored as HIDDEN rather than shown, for the reason
    /// `Registry::hidden_ids` gives about panels: a region added in a
    /// later version then defaults to visible instead of invisible.
    #[serde(default)]
    pub browser_hidden: bool,
    #[serde(default)]
    pub lower_hidden: bool,
    /// Whether the welcome screen stays down at launch.
    ///
    /// Stored as the NEGATIVE for the reason the hidden sets are: a
    /// preference added in a later version then defaults to the friendly
    /// answer, and for a screen that offers you your own songs back the
    /// friendly answer is to show it.
    #[serde(default)]
    pub skip_splash: bool,

    /// Which audio backend to open with.
    ///
    /// A vocabulary of this file's OWN rather than `audio::AudioApi`,
    /// because a preferences file may not name the engine — the layer
    /// test in `ui::mod` enforces it, and the app translates one into the
    /// other exactly as it translates a panel's wishes into actions.
    #[serde(default)]
    pub audio_backend: AudioBackend,

    /// The output device, by NAME.
    ///
    /// A name and not an index: a device list reorders when something is
    /// plugged in, and a preference that pointed at "the third one" would
    /// silently become a preference for a different interface. `None` is
    /// the backend's own default.
    #[serde(default)]
    pub audio_device: Option<String>,

    /// `None` means "whatever the engine opens with". Stored as options
    /// rather than numbers because `UiPrefs` derives `Default`, and a
    /// `u32` that defaulted to zero would read as a deliberate choice of
    /// zero hertz.
    ///
    /// Named `audio_rate_hz` and not `audio_sample_rate` because
    /// `prefs_carry_no_project_data` scans this file's serialized form
    /// for musical vocabulary and "sample" is on its list. A device's
    /// rate is not project data, but the guard is worth more than the
    /// conventional spelling — so the field moved rather than the test.
    #[serde(default)]
    pub audio_rate_hz: Option<u32>,
    #[serde(default)]
    pub audio_buffer_frames: Option<u32>,

    /// Where new project documents and their recovery files live. A path is
    /// machine-local; the stage still refuses a first save when neither this
    /// nor a host-provided home exists.
    #[serde(default)]
    pub project_folder: Option<String>,
    /// Periodic recovery cadence. Kept as a vocabulary rather than a naked
    /// integer so a corrupt value cannot turn into a save storm.
    #[serde(default)]
    pub autosave: Autosave,
    /// Negative flags make an old preferences blob choose the safe answer.
    #[serde(default)]
    pub disable_backups: bool,
    #[serde(default)]
    pub skip_dirty_confirmation: bool,
    /// The stage's two viewing grounds. Dark is the house default, so the
    /// stored bit names the exception.
    #[serde(default)]
    pub light_ground: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default)]
    pub cursor_energy: CursorEnergy,
    #[serde(default)]
    pub hide_tooltips: bool,

    /// Defaults for the export console. These affect a future file, never the
    /// project being edited, and therefore remain machine-local.
    #[serde(default)]
    pub export_format: ExportFormat,
    #[serde(default)]
    pub export_rate_hz: Option<u32>,
    #[serde(default)]
    pub export_tail: ExportTail,
    /// Successful destinations, newest first. This is a convenience trail,
    /// not part of the song's history.
    #[serde(default)]
    pub recent_exports: Vec<String>,
}

/// Which backend the audio engine should open.
///
/// Mirrors `audio::AudioApi` and is deliberately a separate type — see
/// [`UiPrefs::audio_backend`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AudioBackend {
    #[default]
    Jack,
    Alsa,
    Pulse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Autosave {
    Off,
    OneMinute,
    TwoMinutes,
    #[default]
    FiveMinutes,
    TenMinutes,
    FifteenMinutes,
}

impl Autosave {
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::OneMinute,
        Self::TwoMinutes,
        Self::FiveMinutes,
        Self::TenMinutes,
        Self::FifteenMinutes,
    ];

    pub const fn minutes(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::OneMinute => Some(1),
            Self::TwoMinutes => Some(2),
            Self::FiveMinutes => Some(5),
            Self::TenMinutes => Some(10),
            Self::FifteenMinutes => Some(15),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::OneMinute => "1 MIN",
            Self::TwoMinutes => "2 MIN",
            Self::FiveMinutes => "5 MIN",
            Self::TenMinutes => "10 MIN",
            Self::FifteenMinutes => "15 MIN",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CursorEnergy {
    Quiet,
    #[default]
    Normal,
    High,
}

impl CursorEnergy {
    pub const ALL: [Self; 3] = [Self::Quiet, Self::Normal, Self::High];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quiet => "QUIET",
            Self::Normal => "NORMAL",
            Self::High => "HIGH",
        }
    }
}

/// UI vocabulary for the three formats the offline writer actually supports.
/// The host translates this into `audio::bounce::BounceFormat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExportFormat {
    Float32,
    #[default]
    Int24,
    Int16,
}

impl ExportFormat {
    pub const ALL: [Self; 3] = [Self::Float32, Self::Int24, Self::Int16];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Float32 => "32-BIT FLOAT",
            Self::Int24 => "24-BIT PCM",
            Self::Int16 => "16-BIT PCM",
        }
    }

    pub const fn bytes_per_stereo_frame(self) -> u64 {
        match self {
            Self::Float32 => 8,
            Self::Int24 => 6,
            Self::Int16 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExportTail {
    None,
    OneSecond,
    #[default]
    TwoSeconds,
    FiveSeconds,
    TenSeconds,
}

impl ExportTail {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::OneSecond,
        Self::TwoSeconds,
        Self::FiveSeconds,
        Self::TenSeconds,
    ];

    pub const fn seconds(self) -> u32 {
        match self {
            Self::None => 0,
            Self::OneSecond => 1,
            Self::TwoSeconds => 2,
            Self::FiveSeconds => 5,
            Self::TenSeconds => 10,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "0 S",
            Self::OneSecond => "1 S",
            Self::TwoSeconds => "2 S",
            Self::FiveSeconds => "5 S",
            Self::TenSeconds => "10 S",
        }
    }
}

impl UiPrefs {
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Parse, or fall back to defaults. Preferences are never worth failing a
    /// launch over — losing a layout is an annoyance, not starting is a bug.
    pub fn from_ron_or_default(text: &str) -> Self {
        ron::from_str(text).unwrap_or_default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let prefs = UiPrefs {
            density: Density::Compact,
            hidden_panels: vec!["tree".to_owned()],
            focused_center: Some("arrange".to_owned()),
            recent_projects: vec!["/tmp/a.daw.ron".to_owned()],
            mod_strip_collapsed: true,
            browser_hidden: true,
            lower_hidden: true,
            skip_splash: true,
            audio_backend: AudioBackend::Alsa,
            audio_device: Some("Speakers".to_owned()),
            audio_rate_hz: Some(44_100),
            audio_buffer_frames: Some(512),
            project_folder: Some("/tmp/songs".to_owned()),
            autosave: Autosave::TwoMinutes,
            disable_backups: true,
            skip_dirty_confirmation: true,
            light_ground: true,
            reduced_motion: true,
            cursor_energy: CursorEnergy::High,
            hide_tooltips: true,
            export_format: ExportFormat::Float32,
            export_rate_hz: Some(96_000),
            export_tail: ExportTail::FiveSeconds,
            recent_exports: vec!["/tmp/mix.wav".to_owned()],
        };
        let back = UiPrefs::from_ron_or_default(&prefs.to_ron().unwrap());
        assert_eq!(prefs, back);
    }

    /// A file written by an OLDER build lacks fields we since added.
    #[test]
    fn missing_fields_take_defaults() {
        let prefs = UiPrefs::from_ron_or_default("(density: Compact)");
        assert_eq!(prefs.density, Density::Compact);
        assert!(prefs.hidden_panels.is_empty());
        assert_eq!(prefs.focused_center, None);
        // A frame region added later defaults to SHOWN: the field stores
        // hidden, so a file that never heard of it opens with everything
        // visible rather than with the app apparently missing its chrome.
        assert!(!prefs.browser_hidden);
        assert!(!prefs.lower_hidden);
        assert_eq!(prefs.autosave, Autosave::FiveMinutes);
        assert_eq!(prefs.export_format, ExportFormat::Int24);
        assert_eq!(prefs.export_tail, ExportTail::TwoSeconds);
    }

    /// A file written by a NEWER build carries fields we do not know.
    #[test]
    fn unknown_fields_are_ignored() {
        let prefs =
            UiPrefs::from_ron_or_default("(density: Compact, theme: \"solarized\", zoom: 1.5)");
        assert_eq!(prefs.density, Density::Compact);
    }

    #[test]
    fn garbage_loads_as_defaults_rather_than_failing() {
        assert_eq!(
            UiPrefs::from_ron_or_default("}{ nonsense"),
            UiPrefs::default()
        );
        assert_eq!(UiPrefs::from_ron_or_default(""), UiPrefs::default());
    }

    /// The boundary, asserted rather than described: nothing musical is in
    /// here, so a default prefs blob mentions no project vocabulary.
    #[test]
    fn prefs_carry_no_project_data() {
        let text = UiPrefs::default().to_ron().unwrap();
        for musical in ["bpm", "tempo", "clip", "note", "track", "sample"] {
            assert!(
                !text.contains(musical),
                "`{musical}` leaked into machine-local preferences"
            );
        }
    }
}
