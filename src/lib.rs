//! Shared between the `daw` app and the `lab` dev harness.
//!
//! Anything both binaries need lives here. Keeping it a real lib target from
//! the start means the harness can exercise engine code directly instead of
//! duplicating it.

pub mod audio;
pub mod audio_source;
pub mod clap_host;
pub mod console;
pub mod design;
pub mod devices;
pub mod dsp;
pub mod history;
pub mod intent;
pub mod library;
pub mod midi_input;
pub mod param_law;
pub mod params;
pub mod pitch;
pub mod plock_ops;
pub mod record;
pub mod render;
pub mod sample_peaks;
pub mod sequencing;
pub mod shell;
pub mod slice;
pub mod song_graph;
pub mod targets;
pub mod tempo;
pub mod theory;
pub mod tune;
pub mod ui;

use eframe::egui;
use std::collections::VecDeque;
use std::sync::Arc;

/// Aborts on any allocation inside an `assert_no_alloc` block. Debug only —
/// the crate's `disable_release` default makes it a no-op in release builds.
#[cfg(debug_assertions)]
#[global_allocator]
static ALLOC: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

/// The redesign's project-local typeface.
///
/// Keeping the font in the repository makes the app's typography independent
/// of the fonts installed on the machine that launches it.
const TERMINUS_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/fonts/Terminus-Regular.ttf"
));

/// The app's main face: Departure Mono, a pixel monospace cut with a
/// machine's hand — square counters, hard corners, the letterform of a
/// departure board or a terminal that was built rather than written.
/// It leads both text families everywhere, so every `FontId::monospace`
/// and every stock widget lands on it. A bitmap face is sharpest at its
/// own pixel size and its integer multiples; at other sizes it is
/// scaled, and reads as a slightly soft print of itself rather than as a
/// different face. OFL, bundled: `assets/fonts/DepartureMono-LICENSE.txt`.
static DEPARTURE: &[u8] = include_bytes!("../assets/fonts/DepartureMono-Regular.otf");

/// The plaque face: Jersey 20, the block face's letterforms. Installed
/// under its own family so the few carved words reach it and nothing
/// else does. OFL, bundled: `assets/fonts/Jersey20-OFL.txt`.
static JERSEY: &[u8] = include_bytes!("../assets/fonts/Jersey20-Regular.ttf");

/// Put the plaque face into a font table, behind which the main face
/// and Terminus stand for any glyph it lacks.
fn install_plaque(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert(
        design::block::PLAQUE.to_owned(),
        Arc::new(egui::FontData::from_static(JERSEY)),
    );
    fonts.families.insert(
        egui::FontFamily::Name(design::block::PLAQUE.into()),
        vec![
            design::block::PLAQUE.to_owned(),
            "departure".to_owned(),
            "terminus".to_owned(),
        ],
    );
}

/// How much larger the main face is drawn than the size a caller asks
/// for. The surfaces were sized for a typewriter face; one scale,
/// applied where the font is installed, lifts every word on every
/// surface together — the stage's sizes are relationships, and a
/// relationship is kept by scaling all of it, not by re-tuning each
/// call site.
const MAIN_FACE_SCALE: f32 = 1.0;

/// The main face as egui takes it, with its scale applied.
fn main_face() -> egui::FontData {
    egui::FontData::from_static(DEPARTURE).tweak(egui::FontTweak {
        scale: MAIN_FACE_SCALE,
        ..Default::default()
    })
}

/// The stage's other type: a typewriter kept behind the main face for
/// the glyphs it lacks, and an inscription face for the few words that
/// are carved rather than typed.
///
/// iA Writer Mono S is the typewriter, and iA Writer Mono S Bold is the
/// one bold the stage uses, as a family of its own. The inscription face
/// is a geometric grotesque with wide-set capitals, the letterform of a
/// plaque on a machine nobody alive built. It is loaded from the
/// machine's own fonts when present, and falls back to the bundled iA
/// Writer Quattro so a stage without it still has a face for its
/// inscriptions.
pub const INSCRIPTION: &str = "inscription";
pub const TYPEWRITER_BOLD: &str = "typewriter-bold";

static IA_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/iAWriterMonoS-Regular.ttf");
static IA_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/iAWriterMonoS-Bold.ttf");
static IA_QUATTRO_BOLD: &[u8] = include_bytes!("../assets/fonts/iAWriterQuattroS-Bold.ttf");

/// The inscription face, wherever this machine keeps it.
const INSCRIPTION_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/gsfonts/URWGothic-Demi.otf",
    "/usr/share/fonts/urw-base35/URWGothic-Demi.otf",
    "/usr/share/fonts/gsfonts/URWGothic-Book.otf",
];

/// The named family the stage's view sets its console type in.
pub const PROFONT: &str = "profont";
const PROFONT_PATH: &str = "/usr/share/fonts/TTF/ProFontIIxNerdFontMono-Regular.ttf";

pub fn install_stage_fonts(ctx: &egui::Context) -> &'static str {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("departure".to_owned(), Arc::new(main_face()));
    fonts.font_data.insert(
        "terminus".to_owned(),
        Arc::new(egui::FontData::from_static(TERMINUS_REGULAR)),
    );
    fonts.font_data.insert(
        "typewriter".to_owned(),
        Arc::new(egui::FontData::from_static(IA_MONO_REGULAR)),
    );
    fonts.font_data.insert(
        TYPEWRITER_BOLD.to_owned(),
        Arc::new(egui::FontData::from_static(IA_MONO_BOLD)),
    );
    let inscription = INSCRIPTION_CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)));
    let label = match inscription {
        Some((path, bytes)) => {
            fonts.font_data.insert(
                INSCRIPTION.to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            path
        }
        None => {
            fonts.font_data.insert(
                INSCRIPTION.to_owned(),
                Arc::new(egui::FontData::from_static(IA_QUATTRO_BOLD)),
            );
            "iA Writer Quattro S Bold (bundled)"
        }
    };
    let has_profont = std::fs::metadata(PROFONT_PATH).is_ok();
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "departure".to_owned());
        // The typewriter and Terminus stay behind it for the glyphs it
        // lacks — box drawing, the odd symbol — and egui's own fonts
        // remain behind all three.
        list.insert(1, "typewriter".to_owned());
        list.insert(2, "terminus".to_owned());
        // The console's face leads when this machine has it, so every
        // widget that asks for "monospace" speaks in it too.
        if has_profont {
            list.insert(0, "profont".to_owned());
        }
    }
    fonts.families.insert(
        egui::FontFamily::Name(INSCRIPTION.into()),
        vec![
            INSCRIPTION.to_owned(),
            "typewriter".to_owned(),
            "terminus".to_owned(),
        ],
    );
    fonts.families.insert(
        egui::FontFamily::Name(TYPEWRITER_BOLD.into()),
        vec![
            TYPEWRITER_BOLD.to_owned(),
            "typewriter".to_owned(),
            "terminus".to_owned(),
        ],
    );
    install_plaque(&mut fonts);
    // The console's face, shared with the cockpit: ProFont, from wherever
    // this machine keeps it. The view asks for it by name; when it is
    // missing the name resolves to the typewriter, and nothing breaks.
    let profont: Vec<String> = match std::fs::read(PROFONT_PATH) {
        Ok(bytes) => {
            fonts.font_data.insert(
                "profont".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            vec!["profont".to_owned(), "typewriter".to_owned()]
        }
        Err(_) => vec!["typewriter".to_owned()],
    };
    fonts
        .families
        .insert(egui::FontFamily::Name(PROFONT.into()), profont);
    ctx.set_fonts(fonts);
    label
}

pub fn install_fonts(ctx: &egui::Context) -> Option<&'static str> {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("departure".to_owned(), Arc::new(main_face()));
    fonts.font_data.insert(
        "terminus".to_owned(),
        Arc::new(egui::FontData::from_static(TERMINUS_REGULAR)),
    );

    // Departure Mono first, Terminus behind it for the glyphs it lacks, and
    // egui's built-ins behind both.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "departure".to_owned());
        list.insert(1, "terminus".to_owned());
    }

    install_plaque(&mut fonts);
    ctx.set_fonts(fonts);
    Some("Departure Mono 1.500 (bundled), Terminus behind")
}

/// Describe the adapter wgpu actually selected. Reported at runtime, not build
/// time, so a silent fallback to software rendering is visible.
pub fn adapter_label(cc: &eframe::CreationContext<'_>) -> String {
    cc.wgpu_render_state
        .as_ref()
        .map(|rs| {
            let info = rs.adapter.get_info();
            format!("{:?} / {}", info.backend, info.name)
        })
        .unwrap_or_else(|| "no wgpu render state".to_owned())
}

/// Rolling window of frame times, for the status line and the timing bench.
pub struct FrameStats {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new(240)
    }
}

impl FrameStats {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Call once per frame with `ctx.input(|i| i.stable_dt)`.
    pub fn push(&mut self, dt_secs: f32) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(dt_secs * 1000.0);
    }

    pub fn last_ms(&self) -> f32 {
        self.samples.back().copied().unwrap_or(0.0)
    }

    pub fn max_ms(&self) -> f32 {
        self.samples.iter().copied().fold(0.0, f32::max)
    }

    pub fn mean_ms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f32>() / self.samples.len() as f32
    }

    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.samples.iter().copied()
    }

    /// Sparkline of the window. Scaled to the worst frame so spikes are visible.
    pub fn sparkline(&self, ui: &mut egui::Ui, size: egui::Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        if self.samples.len() < 2 {
            return;
        }

        // Never scale below 16.7ms, so a calm 60fps trace doesn't look alarming.
        let peak = self.max_ms().max(16.7);
        let dx = rect.width() / (self.capacity.saturating_sub(1)).max(1) as f32;

        let points: Vec<egui::Pos2> = self
            .samples
            .iter()
            .enumerate()
            .map(|(i, ms)| {
                let x = rect.left() + i as f32 * dx;
                let y = rect.bottom() - (ms / peak).clamp(0.0, 1.0) * rect.height();
                egui::pos2(x, y)
            })
            .collect();

        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        ));
    }
}
