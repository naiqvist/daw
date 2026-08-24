//! Compile-time design tokens. The only visual numbers that exist.
//!
//! Style values live HERE, named by role on a 4px grid. Domain constants
//! (dB ranges, pixels-per-beat, MIDI ranges) are NOT tokens — they are
//! meaning, and live as named consts beside the code that owns them.
//!
//! If a panel "needs" a value that isn't here, that is a design conversation
//! that adds a token — one visible diff — not a literal in the panel.

/// Spacing scale. Base-4 grid; six values, total.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub mod radius {
    /// Buttons, sliders, cells.
    pub const CTRL: f32 = 3.0;
    /// Panels, windows, cards.
    pub const PANEL: f32 = 6.0;
}

pub mod stroke {
    pub const HAIR: f32 = 1.0;
    pub const BOLD: f32 = 1.5;
    pub const FOCUS: f32 = 2.0;
}

/// Type scale. VALUE is for numbers (monospace, tabular by font choice).
pub mod font {
    pub const LABEL: f32 = 11.0;
    pub const VALUE: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 16.0;
}

/// Animation timing, in milliseconds.
pub mod timing {
    pub const HOVER_MS: u64 = 80;
    pub const PANEL_MS: u64 = 150;
}

/// Fixed control geometry.
pub mod control {
    pub const KNOB: f32 = 28.0;
    pub const FADER_LEN: f32 = 160.0;
    pub const FADER_W: f32 = 20.0;
    pub const METER_W: f32 = 8.0;
    pub const TRACK_H_MIN: f32 = 44.0;
    /// Height of the transport and status bars.
    pub const BAR_H: f32 = 36.0;
    /// Device widgets (`ui::device`): XY pad side, envelope editor height,
    /// spectrum display height.
    pub const XY_PAD: f32 = 160.0;
    pub const ENV_H: f32 = 96.0;
    pub const SPECTRUM_H: f32 = 120.0;
    /// Grab radius of a draggable handle (envelope points, XY puck).
    pub const HANDLE: f32 = 5.0;
    /// Device card: every card in a device chain is exactly this tall
    /// (width varies with content), so a rack reads as one strip.
    pub const DEVICE_H: f32 = 168.0;
    pub const DEVICE_W_MIN: f32 = 96.0;
    /// Minimum width of one card section (`device::sections`).
    pub const SECTION_W_MIN: f32 = 64.0;
    /// Horizontal drag distance for full travel of a value readout.
    pub const DRAG_TRAVEL: f32 = 200.0;
}

/// Side-pane geometry. Panels name these instead of picking widths.
pub mod pane {
    pub const SIDE_W: f32 = 240.0;
    pub const SIDE_W_MIN: f32 = 160.0;
    pub const SIDE_W_MAX: f32 = 480.0;
    /// Height of the center dock's tab strip.
    pub const TAB_H: f32 = 26.0;
}

/// How tightly the UI packs. Multiplies every spacing token — the one knob
/// between "comfortable" and "fits more tracks on screen".
///
/// This lives in tokens (not theme) because `action` must be able to name it
/// without importing egui, and tokens is the only layer below both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    pub fn scale(self) -> f32 {
        match self {
            Self::Comfortable => 1.0,
            Self::Compact => 0.85,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Compact => "compact",
        }
    }
}
