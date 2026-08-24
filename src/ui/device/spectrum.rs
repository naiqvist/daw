//! Spectrum grid display: linear-frequency magnitude bins drawn on a
//! log-frequency grid with decade labels. Display-only — analysis, dB
//! scaling, and smoothing are the caller's domain (same convention as
//! `kit::meter`: magnitudes arrive already normalized 0..1).

use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, stroke};
use eframe::egui;

/// Lowest frequency the grid shows. Below this nothing musical happens
/// and log-x would stretch to -inf.
const MIN_HZ: f32 = 20.0;
/// Grid lines: decade multiples that read at a glance.
const GRID_HZ: [f32; 9] = [
    50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
];
/// Which of those get a text label (the rest are just lines).
const LABEL_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];

fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.0}k", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

/// Draw `mags` (normalized 0..1, bins linearly spaced from 0 Hz to
/// `nyquist_hz`) across the available width. An empty slice draws the
/// grid alone — the idle state of an analyzer, not an error.
pub fn spectrum(ui: &mut egui::Ui, theme: &Theme, mags: &[f32], nyquist_hz: f32) {
    let size = egui::vec2(ui.available_width(), theme.sp(control::SPECTRUM_H));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, theme.surface_sunken);

    let max_hz = nyquist_hz.max(MIN_HZ * 2.0);
    let span = (max_hz / MIN_HZ).ln();
    let x_at = |hz: f32| rect.left() + rect.width() * (hz.max(MIN_HZ) / MIN_HZ).ln() / span;

    // Frequency grid with decade labels.
    for &hz in &GRID_HZ {
        if hz >= max_hz {
            break;
        }
        let x = x_at(hz);
        let labeled = LABEL_HZ.contains(&hz);
        let color = if labeled {
            theme.grid_bar
        } else {
            theme.grid_beat
        };
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::HAIR, color),
        );
        if labeled {
            painter.text(
                egui::pos2(x + stroke::FOCUS, rect.bottom()),
                egui::Align2::LEFT_BOTTOM,
                hz_label(hz),
                egui::FontId::monospace(font::LABEL),
                theme.text_muted,
            );
        }
    }

    // Level quarter-lines.
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
    }

    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    // The trace. Bin 0 sits at DC, off-grid to the left; clamp it to the
    // left edge like every analyzer does.
    if mags.len() >= 2 {
        let points: Vec<egui::Pos2> = mags
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let hz = i as f32 / (mags.len() - 1) as f32 * nyquist_hz;
                let y = rect.bottom() - rect.height() * m.clamp(0.0, 1.0);
                egui::pos2(x_at(hz), y)
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(stroke::BOLD, theme.accent),
        ));
    }
}
