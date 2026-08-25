//! Spectrum grid display: linear-frequency magnitude bins drawn on a
//! log-frequency grid with decade labels. Display-only — analysis, dB
//! scaling, and smoothing are the caller's domain (same convention as
//! `kit::meter`: magnitudes arrive already normalized 0..1).

use crate::ui::device::design;
use crate::ui::device::metrics::Footprint;
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
/// The spectrum display's size contract: [`control::SPECTRUM_H`] tall,
/// and wide enough that its octave labels do not collide.
///
/// A MINIMUM — the display stretches into whatever width it is given, and
/// wants as much as it can get. The floor is what keeps the frequency
/// ruler readable rather than a smear of overlapping numbers.
pub fn footprint(ui: &egui::Ui, theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::XY_PAD),
        // The plot, plus a RULER STRIP beneath it for the frequency
        // labels. The labels used to be painted inside the plot, sitting
        // on the bottom grid line and, at low levels, on the trace itself
        // — an axis that obscures the data it is labelling. Reserving the
        // strip is what makes them legible, and putting it in the
        // contract is what stops the plot growing back over it.
        theme.sp(control::SPECTRUM_H)
            + design::gap(theme)
            + crate::ui::device::metrics::line_h(ui, font::LABEL),
    )
}

pub fn spectrum(ui: &mut egui::Ui, theme: &Theme, mags: &[f32], nyquist_hz: f32) {
    let min = footprint(ui, theme);
    // Stretchy, but BOUNDED. Given a whole window it used to take one,
    // and a ten-octave plot two metres wide is not more informative — it
    // just flattens every slope to nothing and shoves its neighbours
    // around. It grows to fill a device card and stops.
    let size = egui::vec2(
        ui.available_width()
            .clamp(min.width(), theme.sp(control::DISPLAY_W_MAX)),
        min.height(),
    );
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    // The plot is everything above the ruler strip.
    let ruler_h = design::gap(theme) + crate::ui::device::metrics::line_h(ui, font::LABEL);
    let rect =
        egui::Rect::from_min_max(area.min, egui::pos2(area.right(), area.bottom() - ruler_h));
    let painter = ui.painter();

    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);

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
            // Centred UNDER its own grid line, in the reserved strip —
            // so a label marks a frequency instead of covering the data
            // at that frequency.
            painter.text(
                egui::pos2(x, area.bottom()),
                egui::Align2::CENTER_BOTTOM,
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
        design::box_radius(),
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
