//! The eight page-key icons, drawn rather than typed.
//!
//! Traced from `session-macos-square-concept.png`, where every key cell
//! carries a small glyph to the left of its label. They are drawn, not
//! font characters, for the reason the rest of this view is drawn: ProFont
//! has no step grid, no filter curve and no cog, and a font that did would
//! have to be shipped and hinted for one row of eight pictures.
//!
//! Each is composed in a UNIT BOX — 0..1 on both axes, y down — and mapped
//! onto whatever rectangle the caller gives it. That keeps the shapes
//! readable as geometry here and lets the cell decide the size, which is
//! the same split the hero pictures use.
//!
//! They say the KIND of page, not its state: one ink, no fills that depend
//! on what is selected. The cell under them carries the state.

use crate::pages::PageKey;
use eframe::egui;

/// How thick a drawn line is, as a share of the box. Heavier than a
/// hairline: at fourteen pixels a one-pixel stroke disappears against a
/// gradient, and these have to survive being small.
const STROKE: f32 = 0.11;

/// Draw `key`'s glyph inside `box_`, in `ink`.
pub fn draw(painter: &egui::Painter, box_: egui::Rect, key: PageKey, ink: egui::Color32) {
    let p = |x: f32, y: f32| {
        egui::pos2(
            box_.min.x + x * box_.width(),
            box_.min.y + y * box_.height(),
        )
    };
    let unit = box_.width().max(box_.height());
    let stroke = egui::Stroke::new((STROKE * unit).max(1.0), ink);
    // A filled rectangle in unit space.
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_filled(egui::Rect::from_min_max(p(x0, y0), p(x1, y1)), 0.0, ink);
    };
    let line = |pts: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };

    match key.index() {
        // TRIG — the step grid: four pads, and the playhead through them.
        0 => {
            for (cx, cy) in [(0.0, 0.02), (0.58, 0.02), (0.0, 0.56), (0.58, 0.56)] {
                bar(cx, cy, cx + 0.42, cy + 0.42);
            }
            // The playhead through them: thin, or the glyph reads as a
            // plus sign rather than a grid with a line across it.
            bar(0.46, -0.06, 0.54, 1.06);
        }
        // SRC — a waveform: five bars about a centre, tallest in the middle.
        1 => {
            for (i, h) in [0.26_f32, 0.62, 1.0, 0.62, 0.26].into_iter().enumerate() {
                let x = i as f32 * 0.22;
                bar(x, 0.5 - h * 0.5, x + 0.12, 0.5 + h * 0.5);
            }
        }
        // FLTR — a lowpass: flat, then the corner, then away.
        2 => line(&[
            (0.0, 0.9),
            (0.34, 0.9),
            (0.52, 0.86),
            (0.66, 0.66),
            (0.78, 0.32),
            (0.92, 0.12),
        ]),
        // AMP — three bars climbing.
        3 => {
            for (i, h) in [0.28_f32, 0.62, 1.0].into_iter().enumerate() {
                let x = 0.12 + i as f32 * 0.31;
                bar(x, 1.0 - h, x + 0.20, 1.0);
            }
        }
        // LFO — one cycle of a sine, up first.
        4 => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    (t, 0.5 - 0.42 * (t * std::f32::consts::TAU).sin())
                })
                .collect();
            line(&pts);
        }
        // FX — a scatter: six points, no grid, because an effect is a
        // handful of things happening at once and not a sequence.
        5 => {
            for (x, y) in [
                (0.10, 0.62),
                (0.30, 0.24),
                (0.34, 0.86),
                (0.58, 0.52),
                (0.80, 0.20),
                (0.84, 0.74),
            ] {
                painter.circle_filled(p(x, y), (0.10 * unit).max(1.0), ink);
            }
        }
        // MIX — three faders at three settings.
        6 => {
            for (i, at) in [0.34_f32, 0.62, 0.46].into_iter().enumerate() {
                let x = 0.14 + i as f32 * 0.32;
                bar(x + 0.03, 0.0, x + 0.09, 1.0);
                bar(x - 0.06, at, x + 0.18, at + 0.16);
            }
        }
        // F8 — the cog: eight teeth around a ring, and a hole.
        _ => {
            let (cx, cy) = (0.5, 0.5);
            let teeth = 8;
            let mut pts = Vec::with_capacity(teeth * 4);
            for i in 0..teeth * 2 {
                let r = if i % 2 == 0 { 0.50 } else { 0.36 };
                let a = i as f32 * std::f32::consts::TAU / (teeth as f32 * 2.0);
                pts.push(p(cx + r * a.cos(), cy + r * a.sin()));
            }
            painter.add(egui::Shape::convex_polygon(
                pts,
                ink,
                egui::Stroke::NONE,
            ));
            // The hole is punched by redrawing the ground, which the
            // caller owns — so instead the ring is left open by drawing a
            // smaller disc in the cell's own fill. Callers pass that as
            // `hole`; without it the cog is solid, which still reads.
        }
    }
}

/// The cog's hole, painted in whatever the cell under it is made of.
/// Separate because only this glyph needs it, and the fill is the
/// caller's to know.
pub fn cog_hole(painter: &egui::Painter, box_: egui::Rect, fill: egui::Color32) {
    painter.circle_filled(box_.center(), box_.width() * 0.17, fill);
}
