//! Family marks: a tiny drawing of what a group of devices DOES.
//!
//! The shapes themselves live in `design::glyph`; this module paints them
//! for the surfaces that read colour from a theme.
//!
//! The browser's headings had only their names, and a name is a word you
//! read. In a list you scan a hundred times a day the useful thing is a
//! shape you recognise before you have read anything — so each family
//! carries a miniature of its own response, and after the first day the
//! word underneath it is confirmation rather than information.
//!
//! That is the whole justification, and it is the same one the rest of
//! this interface is built on: a mark earns its place by carrying a
//! distinction that is otherwise expensive. These are not ornaments with
//! a story attached. A compressor's knee, a bell curve, a decaying set
//! of taps — each one is the picture that family's cards already draw,
//! shrunk to eleven points.
//!
//! # Why vectors and not characters
//!
//! Unicode has a symbol for most of these and every one of them is a
//! lie at this size: they carry a typeface's opinion, they sit on a
//! baseline that is not the row's centre, and they fall back to a box
//! on a machine without the font. A polyline is the same on every
//! machine, aligns to the pixel grid the tree is already locked to, and
//! can be as wide as the cell rather than as wide as a glyph.

use crate::ui::theme::Theme;
use eframe::egui;

pub use crate::design::glyph::{Glyph, strokes};

/// Paint the mark inside `cell`.
///
/// The cell is squared and centred first: a mark stretched to a wide
/// cell stops being the shape it was designed as, and these are only
/// eleven points tall to begin with.
pub fn paint(painter: &egui::Painter, cell: egui::Rect, glyph: Glyph, colour: egui::Color32) {
    let side = cell.width().min(cell.height());
    if side <= 1.0 {
        return;
    }
    let box_ = egui::Rect::from_center_size(cell.center(), egui::Vec2::splat(side));
    let stroke = egui::Stroke::new(crate::ui::tokens::stroke::HAIR, colour);
    for run in strokes(glyph) {
        let points: Vec<egui::Pos2> = run
            .into_iter()
            .map(|(x, y)| egui::pos2(box_.left() + x * side, box_.top() + y * side))
            .collect();
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

/// The mark's colour: bright when the family is open, quiet when it is
/// not — the same "dim partner for inactive context" rule the role
/// colours follow.
pub fn ink(theme: &Theme, open: bool) -> egui::Color32 {
    if open { theme.accent } else { theme.outline }
}
