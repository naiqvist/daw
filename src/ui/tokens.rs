//! Compile-time design tokens. The only visual numbers that exist.
//!
//! Style values live HERE, named by role on a 4px grid. Domain constants
//! (dB ranges, pixels-per-beat, MIDI ranges) are NOT tokens — they are
//! meaning, and live as named consts beside the code that owns them.
//!
//! If a panel "needs" a value that isn't here, that is a design conversation
//! that adds a token — one visible diff — not a literal in the panel.

/// Spacing scale. Base-4 grid.
///
/// [`XXS`] is the ONE deliberate half-step, and it is documented here
/// rather than smuggled in as a literal somewhere. Compact chrome needs a
/// gap below the grid — four points of padding around a twenty-point dial
/// is a frame competing with its own content — and the alternative was a
/// bare `2.0` inside a widget, which is exactly what the design system
/// exists to prevent. It is the only sub-grid value and it is for chrome
/// around already-miniature content, nothing else.
pub mod space {
    /// The half-step. Compact chrome only — see the module note.
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub mod radius {
    /// The UI uses square corners throughout.
    pub const CTRL: f32 = 0.0;
    /// Kept as a separate semantic token so callers still name their
    /// surface role even though every rectangular surface is square.
    pub const PANEL: f32 = 0.0;
}

pub mod stroke {
    pub const HAIR: f32 = 1.0;
    pub const BOLD: f32 = 1.5;
    pub const FOCUS: f32 = 2.0;
    /// A DATA MARK on a display — a delay's repeats, not a border. It is
    /// the loudest thing on its card, so it outweighs the focus ring
    /// rather than matching it.
    pub const MARK: f32 = 3.0;
}

/// Type scale. VALUE is for numbers (monospace, tabular by font choice).
pub mod font {
    /// Smaller than MINI: the corner tags on a TE-style display, where
    /// the drawing carries the meaning and the words only confirm it.
    pub const MICRO_LABEL: f32 = 9.0;
    pub const MINI_LABEL: f32 = 10.0;
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
    /// A MINI knob's dial. Sized to the interaction floor rather than
    /// smaller: below that it stops being a reliable drag target, and a
    /// knob you have to aim at is worse than one that takes more room.
    pub const KNOB_MINI: f32 = 20.0;
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
    /// The widest a STRETCHY display grows — a spectrum, a level ruler,
    /// anything that takes `available_width`.
    ///
    /// Unbounded, these eat whatever they are given: a ten-octave plot two
    /// metres wide is not more informative, it just flattens every slope
    /// and shoves its neighbours out of the column. One cap, so two
    /// displays side by side agree about how big "as big as possible" is.
    pub const DISPLAY_W_MAX: f32 = 480.0;
    /// Grab radius of a draggable handle (envelope points, XY puck).
    pub const HANDLE: f32 = 5.0;
    /// Device card: every card in a device chain is exactly this tall
    /// (width varies with content), so a rack reads as one strip.
    pub const DEVICE_H: f32 = 168.0;
    /// The INSTRUMENT card's height. An instrument earns more vertical
    /// room than an effect: it carries a hero display plus the rows that
    /// annotate it, and 168 pt forced those into each other's laps. The
    /// rack lives with two silhouettes — Live's does too.
    pub const DEVICE_TALL_H: f32 = 224.0;
    pub const DEVICE_W_MIN: f32 = 96.0;
    /// The width of a SIDE COLUMN: the browser, and the controls column
    /// down the left of the audio editor.
    ///
    /// One number rather than two that happen to agree. A column beside
    /// the browser at a different width does not read as a second column,
    /// it reads as a mistake — and the two are visible at once whenever
    /// the lower region is open.
    pub const SIDE_COLUMN_W: f32 = 240.0;
    /// Minimum width of one card section (`device::sections`).
    pub const SECTION_W_MIN: f32 = 64.0;
    /// The same floor for a COMPACT well. It exists so an empty well is
    /// still visible and clickable, and a compact well holding a 20-point
    /// dial has no business being 64 wide — leaving the full floor in
    /// place cancels the padding saving exactly, which is how the
    /// compact-wells test found it.
    pub const SECTION_W_MIN_MINI: f32 = 36.0;
    /// Horizontal drag distance for full travel of a value readout.
    pub const DRAG_TRAVEL: f32 = 200.0;
    /// Height of a segmented switch's track.
    pub const SWITCH_H: f32 = 22.0;
    /// A dynamics transfer curve is SQUARE — input dB across, output dB
    /// up — because the unity diagonal has to read as 45 degrees. On a
    /// rectangle it does not, and the whole display becomes a lie about
    /// how much is being taken off.
    pub const TRANSFER: f32 = 160.0;
    /// One bar of the dynamics bar view. Thick on purpose: these are
    /// grabbed and read at a glance, not squinted at.
    pub const DYN_BAR_H: f32 = 16.0;
    pub const TRANSFER_MINI: f32 = 56.0;
    /// A MINI response curve: the thumbnail that sits in a device well
    /// beside the knobs driving it, rather than the full-width display.
    pub const CURVE_MINI_W: f32 = 96.0;
    pub const CURVE_MINI_H: f32 = 44.0;
    /// Poly-synth waveform bank: four shapes across, two rows. Large
    /// enough that the thumbnail — not its name — identifies the sound.
    pub const WAVE_BANK_W: f32 = 256.0;
    pub const WAVE_BANK_H: f32 = 104.0;
    /// Three direct-manipulation pitch bands: octave, semitone and cents.
    pub const PITCH_STACK_W: f32 = 192.0;
    pub const PITCH_STACK_H: f32 = 72.0;
    /// The unison field is wider than it is tall because its horizontal
    /// axis represents stereo spread.
    pub const UNISON_W: f32 = 192.0;
    pub const UNISON_H: f32 = 104.0;
    /// The poly synth's visual studies and compact controls. The tabbed
    /// instrument gives each study the full face, so these smaller contracts
    /// remain useful to its individual widgets and gallery examples.
    pub const POLY_OSC_W: f32 = 208.0;
    pub const POLY_SOURCES_W: f32 = 112.0;
    pub const POLY_FILTER_W: f32 = 272.0;
    pub const POLY_DETAIL_W: f32 = 192.0;
    /// The one tabbed poly instrument. Wide enough for a generous waveform
    /// and three-column routing rows, narrow enough to remain one rack device.
    pub const POLY_FACE_W: f32 = 480.0;
    pub const POLY_WAVE_H: f32 = 36.0;
    /// The hero's RESTING height in the tall card. A cap, not a minimum:
    /// left to itself the hero absorbs every point the card grows by,
    /// and the whole reason the card grew was to give the ROWS room.
    /// Shrunk twice with the TE pass: the drawing needs less height once
    /// it stops competing with chrome, and less WIDTH once it stops
    /// being asked to fill a column. A hairline waveform reads at this
    /// size — TE's own are smaller still.
    pub const POLY_HERO_H: f32 = 32.0;
    /// An Elektron-style needle dial on a display: small enough to be an
    /// annotation, big enough for the needle's angle to read.
    pub const POLY_DIAL: f32 = 28.0;
    pub const POLY_CELL_H: f32 = 22.0;
    pub const POLY_MINI_LABEL_W: f32 = 32.0;
    pub const POLY_MINI_VALUE_W: f32 = 52.0;
    pub const POLY_RAIL_H: f32 = 24.0;
    pub const POLY_TAB_H: f32 = 22.0;
    pub const ENV_COMPACT_H: f32 = 36.0;
    pub const CURVE_COMPACT_H: f32 = 44.0;
    /// Length of a level meter's scale, and the height of the clip light
    /// that caps it.
    pub const METER_LEN: f32 = 120.0;
    pub const METER_CLIP_H: f32 = 6.0;
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
