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
