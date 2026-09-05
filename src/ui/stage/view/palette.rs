//! The console's colours: a hue is a type, not a decoration.
//!
//! The same scheme as the cockpit's (`~/Work/tachikoma`, `DESIGN.md`):
//! teal is the machine, blue is a name, green is fine, orange wants you,
//! pink is broken. Intensity keeps its own job — how bright a thing is
//! says how important it is — and every tone is a named share of one hue
//! mixed toward the ground. Nothing is mixed at a call site.
//!
//! The stage's core keeps its own alphabet and its polarity; this is the
//! view's, and the view is the only thing that reads it.
//!
//! The scheme lives in `~/Corpus/daw.theme`, a file of OkLCh roles the
//! cockpit's picker can edit while this is running:
//!
//! ```sh
//! cd ~/Corpus && cargo run --release -p palette-picker -- ~/Corpus/daw.theme
//! ```
//!
//! `io/watch` notices the file settle, `color/theme` parses it,
//! `color/oklab` turns each role into something the screen can show. The
//! defaults below are the cockpit's constants and seed the file when it
//! does not exist, so a missing file or a missing role changes nothing.
//! The lock is read once per colour per frame: nothing, and it buys
//! recolouring the whole stage without stopping it.

use eframe::egui::Color32;
use std::sync::RwLock;

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

impl Colours {
    /// Every role by name, so a file and this struct cannot disagree
    /// about what a role is called.
    fn slots(&mut self) -> [(&'static str, &mut Color32); 15] {
        [
            ("ground", &mut self.ground),
            ("ink", &mut self.ink),
            ("panel", &mut self.panel),
            ("bright", &mut self.bright),
            ("fg", &mut self.fg),
            ("dim", &mut self.dim),
            ("rule", &mut self.rule),
            ("edge", &mut self.edge),
            ("chassis", &mut self.chassis),
            ("select", &mut self.select),
            ("label", &mut self.label),
            ("dir", &mut self.dir),
            ("nominal", &mut self.nominal),
            ("alert", &mut self.alert),
            ("fault", &mut self.fault),
        ]
    }
}

static CURRENT: RwLock<Colours> = RwLock::new(Colours::DEFAULT);

/// The colours in force this frame.
pub fn colours() -> Colours {
    CURRENT.read().map(|c| *c).unwrap_or(Colours::DEFAULT)
}

/// Where the theme lives: beside the cockpit's, one console.
pub fn theme_path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    home.unwrap_or_default().join("Corpus").join("daw.theme")
}

/// The theme file, watched.
pub struct Skin {
    path: std::path::PathBuf,
    settle: watch::Settle,
    /// Bumps each time a theme lands; a caller that caches by colour can
    /// key on it.
    pub generation: u64,
    /// What the last load had to say: role count, or why it was refused.
    pub status: String,
}

impl Skin {
    /// Watch `path`, seeding it with the defaults if it does not exist.
    pub fn new(path: std::path::PathBuf) -> Self {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, to_text(&Colours::DEFAULT));
        }
        Self {
            path,
            settle: watch::Settle::new(3),
            generation: 0,
            status: String::new(),
        }
    }

    /// Once a frame. `true` when a new theme just took effect.
    pub fn poll(&mut self) -> bool {
        if !self.settle.observe(watch::fingerprint(&self.path)) {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            self.status = "theme unreadable".to_owned();
            return false;
        };
        match theme_file::parse(&text) {
            Ok(parsed) => {
                let (next, found) = apply(&parsed);
                let ratio = contrast::ratio(srgb(next.fg), srgb(next.ground));
                if let Ok(mut w) = CURRENT.write() {
                    *w = next;
                }
                self.generation += 1;
                self.status = format!("theme: {found} roles, fg/ground {ratio:.1}:1");
                true
            }
            Err(e) => {
                self.status = format!("theme refused: {e:?}");
                false
            }
        }
    }
}

/// The defaults with every role the file names replaced, and how many
/// that was.
fn apply(parsed: &theme_file::Theme) -> (Colours, usize) {
    let mut next = Colours::DEFAULT;
    let mut found = 0;
    for (name, slot) in next.slots() {
        if let Some(lch) = parsed.get(name) {
            *slot = to_color32(lch);
            found += 1;
        }
    }
    (next, found)
}

fn to_color32(lch: [f64; 3]) -> Color32 {
    let rgb = oklab::to_srgb(oklab::from_lch(lch));
    let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

fn srgb(c: Color32) -> [f64; 3] {
    [
        c.r() as f64 / 255.0,
        c.g() as f64 / 255.0,
        c.b() as f64 / 255.0,
    ]
}

fn to_lch(c: Color32) -> [f64; 3] {
    oklab::to_lch(oklab::from_srgb(srgb(c)))
}

/// The scheme as a theme file, for seeding one that does not exist yet.
fn to_text(colours: &Colours) -> String {
    let mut t = theme_file::Theme::new();
    let mut copy = *colours;
    for (name, slot) in copy.slots() {
        t.set(name, to_lch(*slot));
    }
    format!(
        "# daw. name  L  C  h(radians), OkLCh. The same roles as tachikoma's.\n\
         # Edited live by `cargo run -p palette-picker -- <this file>`.\n{}",
        t.to_text()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme written from the defaults reads back as the defaults, to
    /// within a byte per channel — the file is a faithful copy, not a
    /// slow drift.
    #[test]
    fn the_defaults_survive_a_trip_through_the_file() {
        let parsed = theme_file::parse(&to_text(&Colours::DEFAULT)).expect("our own text parses");
        let (back, found) = apply(&parsed);
        assert_eq!(found, 15);
        let mut a = Colours::DEFAULT;
        let mut b = back;
        for ((name, x), (_, y)) in a.slots().into_iter().zip(b.slots()) {
            for (p, q) in [(x.r(), y.r()), (x.g(), y.g()), (x.b(), y.b())] {
                assert!(
                    (p as i32 - q as i32).abs() <= 1,
                    "{name}: {x:?} became {y:?}"
                );
            }
        }
    }

    /// A file that names only some roles leaves the rest at default.
    #[test]
    fn a_missing_role_changes_nothing() {
        let parsed = theme_file::parse("alert 0.9 0.2 1.0\n").expect("parses");
        let (next, found) = apply(&parsed);
        assert_eq!(found, 1);
        assert_eq!(next.ground, Colours::DEFAULT.ground);
        assert_ne!(next.alert, Colours::DEFAULT.alert);
    }
}
