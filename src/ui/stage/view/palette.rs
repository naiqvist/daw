//! The console's colours: a hue is a type, not a decoration.
//!
//! The same scheme as the cockpit's (`~/Work/tachikoma`, `DESIGN.md`):
//! teal is the machine, blue is a name, green is fine, orange wants you,
//! pink is broken. Intensity keeps its own job — how bright a thing is
//! says how important it is — and every tone is a named share of one hue
//! mixed toward the ground. Nothing is mixed at a call site.
//!
//! The stage's core keeps its own alphabet and its polarity; this is the
//! view's, and the view is the only thing that reads it. The defaults
//! below are the cockpit's constants; a theme file will override them
//! live (next step), and a missing file changes nothing.

use eframe::egui::Color32;

/// Every colour the view knows how to name.
#[derive(Clone, Copy, Debug)]
pub struct Colours {
    /// The ground of everything.
    pub ground: Color32,
    /// The colour you read in.
    pub ink: Color32,
    /// A surface a step above the ground.
    pub panel: Color32,
    /// Emphasis by intensity.
    pub bright: Color32,
    /// Body text.
    pub fg: Color32,
    /// Subordinate text.
    pub dim: Color32,
    /// Hairlines, meter tracks.
    pub rule: Color32,
    /// Splits, tree lines, a resting edge.
    pub edge: Color32,
    /// The live chassis: a focused edge, an active rule.
    pub chassis: Color32,
    /// Selection ground.
    pub select: Color32,
    /// A field's name.
    pub label: Color32,
    /// A thing's name, stronger.
    pub dir: Color32,
    /// A reading inside its budget; a thing sounding.
    pub nominal: Color32,
    /// A threshold crossed, a change happening now.
    pub alert: Color32,
    /// A defect the machine reports about itself. At most one on screen.
    pub fault: Color32,
}

/// The ground's channels, so a mix blends toward the real thing.
const G: [u32; 3] = [0x06, 0x18, 0x1b];

const fn mix(c: [u32; 3], pct: u32) -> Color32 {
    Color32::from_rgb(
        ((c[0] * pct + G[0] * (100 - pct)) / 100) as u8,
        ((c[1] * pct + G[1] * (100 - pct)) / 100) as u8,
        ((c[2] * pct + G[2] * (100 - pct)) / 100) as u8,
    )
}

const INK: [u32; 3] = [0xe6, 0xf2, 0xef];
const TEAL: [u32; 3] = [0x5a, 0xd2, 0xc2];
const BLUE: [u32; 3] = [0x6c, 0xa8, 0xe0];
const GREEN: [u32; 3] = [0x74, 0xd1, 0x8c];
const ORANGE: [u32; 3] = [0xf2, 0x9a, 0x4a];
const PINK: [u32; 3] = [0xe8, 0x7f, 0xb0];

impl Colours {
    pub const DEFAULT: Colours = Colours {
        ground: Color32::from_rgb(G[0] as u8, G[1] as u8, G[2] as u8),
        ink: Color32::from_rgb(INK[0] as u8, INK[1] as u8, INK[2] as u8),
        panel: mix(INK, 5),
        bright: Color32::from_rgb(INK[0] as u8, INK[1] as u8, INK[2] as u8),
        fg: mix(INK, 86),
        dim: mix(INK, 42),
        rule: mix(TEAL, 16),
        edge: mix(TEAL, 34),
        chassis: mix(TEAL, 62),
        select: mix(TEAL, 18),
        label: mix(BLUE, 55),
        dir: mix(BLUE, 82),
        nominal: mix(GREEN, 78),
        alert: mix(ORANGE, 92),
        fault: mix(PINK, 88),
    };
}

/// The colours in force this frame.
pub fn colours() -> Colours {
    Colours::DEFAULT
}
