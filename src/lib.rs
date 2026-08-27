//! Shared between the `daw` app and the `lab` dev harness.
//!
//! Anything both binaries need lives here. Keeping it a real lib target from
//! the start means the harness can exercise engine code directly instead of
//! duplicating it.

pub mod audio;
pub mod dsp;
pub mod library;
pub mod params;
pub mod render;
pub mod slice;
pub mod theory;
pub mod ui;

use eframe::egui;
use std::collections::VecDeque;
use std::sync::Arc;

/// Aborts on any allocation inside an `assert_no_alloc` block. Debug only —
/// the crate's `disable_release` default makes it a no-op in release builds.
#[cfg(debug_assertions)]
#[global_allocator]
static ALLOC: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

/// Where Iosevka might live. First hit wins.
///
/// The Mono variant is used for both text families: Iosevka is monospace for
/// latin either way, and the two files are ~14MB each, so loading one instead
/// of two halves the startup parse.
const IOSEVKA_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/IosevkaNerdFontMono-Regular.ttf",
    "/usr/share/fonts/TTF/IosevkaNerdFont-Regular.ttf",
    "/usr/share/fonts/truetype/iosevka/Iosevka-Regular.ttf",
    "/usr/local/share/fonts/IosevkaNerdFontMono-Regular.ttf",
];

/// Make Iosevka the default for both proportional and monospace text.
///
/// Returns the path loaded, or `None` if no candidate was readable — in which
/// case egui keeps its built-in fonts and the app still runs.
pub fn install_fonts(ctx: &egui::Context) -> Option<&'static str> {
    let (path, bytes) = IOSEVKA_CANDIDATES
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (*p, b)))?;

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "iosevka".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );

    // Insert at the front of both families so Iosevka wins, while egui's
    // built-ins stay behind it as fallback for glyphs Iosevka lacks.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "iosevka".to_owned());
    }

    ctx.set_fonts(fonts);
    Some(path)
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
