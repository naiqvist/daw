//! Shared between the `daw` app and the `lab` dev harness.
//!
//! Anything both binaries need lives here. Keeping it a real lib target from
//! the start means the harness can exercise engine code directly instead of
//! duplicating it.

pub mod audio;
pub mod dsp;
pub mod library;
pub mod param_law;
pub mod params;
pub mod pitch;
pub mod render;
pub mod sequencing;
pub mod slice;
pub mod tempo;
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

/// The redesign's project-local typeface.
///
/// Keeping the font in the repository makes the app's typography independent
/// of the fonts installed on the machine that launches it.
const TERMINUS_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/fonts/Terminus-Regular.ttf"
));

/// Make the bundled Terminus the default for both text families.
pub fn install_fonts(ctx: &egui::Context) -> Option<&'static str> {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "terminus".to_owned(),
        Arc::new(egui::FontData::from_static(TERMINUS_REGULAR)),
    );

    // Insert at the front so Terminus wins, while egui's built-ins remain
    // available as fallback for glyphs it does not contain.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "terminus".to_owned());
    }

    ctx.set_fonts(fonts);
    Some("Terminus TTF 4.49.3 (bundled)")
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
