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
//!
//! Two files, one per POLARITY: `daw.theme` for the dark ground and
//! `daw-light.theme` for the light one (seeded with gruvbox light). The
//! ground turned over (^L, or the light-ground preference) reads the
//! other file; every role keeps its meaning, only its colour changes.

use eframe::egui::Color32;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// The drum lane's hue: a type of channel, worn by its head. The one
    /// role that names something musical rather than something the
    /// machine says about itself.
    pub drum: Color32,
    /// The lit page key's ground. The macOS square concept carries two
    /// different "this one is active" signals and they are not
    /// interchangeable: an orange rule around the cell the cursor is on
    /// (`chassis`), and a saturated blue slab under the page key you are
    /// looking at. Neither reads as the other, so neither role can do
    /// both jobs.
    pub tab: Color32,
}

/// A step from one role toward another.
///
/// The module's standing rule is that nothing is mixed at a call site —
/// which is exactly why this lives here. A raised cell in the reference
/// is not one flat colour: it carries a slight vertical gradient and a
/// lighter line along its top edge, and those tones are DERIVED from the
/// roles rather than being roles of their own. Doing that arithmetic in
/// one named place keeps the promise; doing it in `deck.rs` would break
/// it.
fn toward(a: Color32, b: Color32, pct: u32) -> Color32 {
    let f = |x: u8, y: u8| ((x as u32 * (100 - pct) + y as u32 * pct) / 100) as u8;
    Color32::from_rgb(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
    )
}

/// A raised cell's three tones: the fill at its head, the fill at its
/// foot, and the line around it.
///
/// Traced from `session-macos-square-concept.png`. An unlit key cell runs
/// #2c4461 to #283e59 inside a #42586f line; the lit one runs #1564c0 to
/// #1460b6 inside #227ddb. Both are a lift at the top of about eight
/// units of blue — enough to read as a surface catching light, not enough
/// to read as a colour of its own.
pub fn raised(lit: bool) -> (Color32, Color32, Color32) {
    let c = colours();
    if lit {
        (
            toward(c.tab, c.bright, 8),
            toward(c.tab, c.ground, 6),
            toward(c.tab, c.bright, 26),
        )
    } else {
        (c.edge, toward(c.edge, c.panel, 34), c.rule)
    }
}

/// A vertical gradient across `rect`, as one quad. Two triangles and four
/// vertices: cheaper than banding it, and it does not seam.
pub fn gradient(painter: &eframe::egui::Painter, rect: eframe::egui::Rect, top: Color32, foot: Color32) {
    use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};
    let mut mesh = Mesh::default();
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), foot),
        (rect.left_bottom(), foot),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(eframe::egui::Shape::mesh(mesh));
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
const VIOLET: [u32; 3] = [0xa9, 0x8c, 0xf0];

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
        drum: mix(VIOLET, 84),
        tab: mix(BLUE, 62),
    };
}

/// Gruvbox light, on the same roles: the paper ground, the dark ink,
/// aqua for the machine, blue for a name, green for fine, orange for
/// wants-you, red for broken, purple for the drum lane.
const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Colours {
    pub const GRUVBOX_LIGHT: Colours = Colours {
        ground: rgb(0xfbf1c7),
        ink: rgb(0x282828),
        panel: rgb(0xf2e5bc),
        bright: rgb(0x282828),
        fg: rgb(0x3c3836),
        dim: rgb(0x7c6f64),
        rule: rgb(0xebdbb2),
        edge: rgb(0xd5c4a1),
        chassis: rgb(0x427b58),
        select: rgb(0xd9e0c6),
        label: rgb(0x076678),
        dir: rgb(0x458588),
        nominal: rgb(0x79740e),
        alert: rgb(0xaf3a03),
        fault: rgb(0x9d0006),
        drum: rgb(0x8f3f71),
        tab: rgb(0x458588),
    };
}

impl Colours {
    /// Every role by name, so a file and this struct cannot disagree
    /// about what a role is called.
    fn slots(&mut self) -> [(&'static str, &mut Color32); 17] {
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
            ("drum", &mut self.drum),
            ("tab", &mut self.tab),
        ]
    }
}

static CURRENT: RwLock<Colours> = RwLock::new(Colours::DEFAULT);
static CURRENT_LIGHT: RwLock<Colours> = RwLock::new(Colours::GRUVBOX_LIGHT);
/// Whether the ground is turned over this frame: the light file reads.
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Which polarity the colours answer for. The view says so once a
/// frame, from the stage's own polarity.
pub fn set_polarity(polarity: crate::design::Polarity) {
    LIGHT.store(
        polarity == crate::design::Polarity::Light,
        Ordering::Relaxed,
    );
}

/// A shade of the ground: the ground's own hue and chroma, its lightness
/// raised by `t` of the way toward the reading surface. `0` is the
/// ground, `1` is as light as ink. Every neutral the shared widgets need
/// is one of these, so nothing on the glass is grey.
pub fn shade(t: f32) -> Color32 {
    let c = colours();
    let [l, chroma, h] = to_lch(c.ground);
    let ink_l = to_lch(c.ink)[0];
    let t = f64::from(t.clamp(0.0, 1.0));
    let lch = [l + (ink_l - l) * t, chroma * (1.0 + 1.5 * t), h];
    to_color32(lch)
}

/// The design alphabet, seen through the console: what the shared
/// sequencer draws in once it is on this glass. Its neutral ladder —
/// ground, well, surface, edge, ink, focus — becomes shades of the
/// ground; its signals become the console's types. Tiers and channels
/// are the alphabet's own.
pub fn lift(mut a: crate::design::Alphabet) -> crate::design::Alphabet {
    let c = colours();
    a.ground.color = shade(0.0);
    a.well.color = shade(0.06);
    a.surface.color = shade(0.10);
    a.edge.color = shade(0.28);
    a.ink.color = shade(0.62);
    a.focus.color = c.alert;
    a.jeopardy_latent.color = c.alert;
    a.jeopardy_active.color = c.fault;
    a.live.color = c.nominal;
    a.live_dim.color = c.select;
    a
}

/// The runtime theme the stock widgets read, in the console's colours:
/// every role of `ui::theme::Theme` mapped onto a role of ours, so a
/// widget that was never taught the palette still sits in the same room.
pub fn theme() -> crate::ui::theme::Theme {
    let c = colours();
    let mut t = crate::ui::theme::Theme::dark();
    t.bg = c.ground;
    t.surface = c.panel;
    t.surface_raised = c.ground;
    t.surface_sunken = c.ground;
    t.text = c.fg;
    t.text_muted = c.dim;
    t.text_value = c.bright;
    t.outline = c.chassis;
    t.divider = c.rule;
    t.focus = c.alert;
    t.accent = c.chassis;
    t.accent_muted = c.select;
    t.ok = c.nominal;
    t.warn = c.alert;
    t.danger = c.fault;
    t
}

/// The colours in force this frame, for the ground's polarity.
pub fn colours() -> Colours {
    if LIGHT.load(Ordering::Relaxed) {
        CURRENT_LIGHT
            .read()
            .map(|c| *c)
            .unwrap_or(Colours::GRUVBOX_LIGHT)
    } else {
        CURRENT.read().map(|c| *c).unwrap_or(Colours::DEFAULT)
    }
}

/// Where the theme lives: beside the cockpit's, one console.
pub fn theme_path() -> std::path::PathBuf {
    crate::corpus::dir().join("daw.theme")
}

/// The light ground's theme, beside it.
pub fn light_theme_path() -> std::path::PathBuf {
    crate::corpus::dir().join("daw-light.theme")
}

/// The theme files, watched: the dark ground's and the light ground's.
pub struct Skin {
    path: std::path::PathBuf,
    settle: watch::Settle,
    light_path: std::path::PathBuf,
    light_settle: watch::Settle,
    /// Bumps each time a theme lands; a caller that caches by colour can
    /// key on it.
    pub generation: u64,
    /// What the last load had to say: role count, or why it was refused.
    pub status: String,
}

impl Skin {
    /// Watch `path` and its light sibling, seeding each with its
    /// defaults if it does not exist.
    pub fn new(path: std::path::PathBuf) -> Self {
        let light_path = path
            .parent()
            .map_or_else(|| light_theme_path(), |dir| dir.join("daw-light.theme"));
        seed(&path, &Colours::DEFAULT);
        seed(&light_path, &Colours::GRUVBOX_LIGHT);
        Self {
            path,
            settle: watch::Settle::new(3),
            light_path,
            light_settle: watch::Settle::new(3),
            generation: 0,
            status: String::new(),
        }
    }

    /// Once a frame. `true` when a new theme just took effect.
    pub fn poll(&mut self) -> bool {
        let dark = if self.settle.observe(watch::fingerprint(&self.path)) {
            Self::load(
                &self.path,
                &Colours::DEFAULT,
                &CURRENT,
                &mut self.status,
                "theme",
            )
        } else {
            false
        };
        let light = if self
            .light_settle
            .observe(watch::fingerprint(&self.light_path))
        {
            Self::load(
                &self.light_path,
                &Colours::GRUVBOX_LIGHT,
                &CURRENT_LIGHT,
                &mut self.status,
                "light theme",
            )
        } else {
            false
        };
        if dark || light {
            self.generation += 1;
        }
        dark || light
    }

    /// Read one file into its slot, over `base` for the roles it leaves
    /// unnamed. `true` when it took.
    fn load(
        path: &std::path::Path,
        base: &Colours,
        into: &RwLock<Colours>,
        status: &mut String,
        what: &str,
    ) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            *status = format!("{what} unreadable");
            return false;
        };
        match theme_file::parse(&text) {
            Ok(parsed) => {
                let (next, found) = apply(&parsed, base);
                let ratio = contrast::ratio(srgb(next.fg), srgb(next.ground));
                if let Ok(mut w) = into.write() {
                    *w = next;
                }
                *status = format!("{what}: {found} roles, fg/ground {ratio:.1}:1");
                true
            }
            Err(e) => {
                *status = format!("{what} refused: {e:?}");
                false
            }
        }
    }
}

/// Write `colours` to `path` as a theme file, when there is none.
fn seed(path: &std::path::Path, colours: &Colours) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, to_text(colours));
}

/// `base` with every role the file names replaced, and how many that
/// was.
fn apply(parsed: &theme_file::Theme, base: &Colours) -> (Colours, usize) {
    let mut next = *base;
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

    /// The light ground is paper, not a dimmer dark: its ground is
    /// light, its ink dark, and they read against each other.
    #[test]
    fn gruvbox_light_is_light_and_reads() {
        let c = Colours::GRUVBOX_LIGHT;
        assert!(to_lch(c.ground)[0] > 0.9, "the ground is not light");
        assert!(to_lch(c.ink)[0] < 0.4, "the ink is not dark");
        assert!(contrast::ratio(srgb(c.fg), srgb(c.ground)) > 7.0);
        assert!(contrast::ratio(srgb(c.dim), srgb(c.ground)) > 3.0);
        // The roles keep their meanings: the machine's hue, a name's, and
        // the alarm's are still three hues.
        assert_ne!(c.chassis, c.label);
        assert_ne!(c.alert, c.nominal);
    }

    /// The colours answer for the ground's polarity.
    #[test]
    fn the_colours_follow_the_polarity() {
        set_polarity(crate::design::Polarity::Light);
        let light = colours();
        set_polarity(crate::design::Polarity::Dark);
        let dark = colours();
        assert!(to_lch(light.ground)[0] > to_lch(dark.ground)[0]);
    }

    /// A theme written from the defaults reads back as the defaults, to
    /// within a byte per channel — the file is a faithful copy, not a
    /// slow drift.
    #[test]
    fn the_defaults_survive_a_trip_through_the_file() {
        let parsed = theme_file::parse(&to_text(&Colours::DEFAULT)).expect("our own text parses");
        let (back, found) = apply(&parsed, &Colours::DEFAULT);
        // Seventeen since `tab` joined them. This number is the point of
        // the test: a role added to the struct and forgotten in `slots`
        // would never reach the file, and the file would go on looking
        // complete.
        assert_eq!(found, 17);
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
        let (next, found) = apply(&parsed, &Colours::DEFAULT);
        assert_eq!(found, 1);
        assert_eq!(next.ground, Colours::DEFAULT.ground);
        assert_ne!(next.alert, Colours::DEFAULT.alert);
    }
}
