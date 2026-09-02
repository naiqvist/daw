//! The block face: the deck's inscriptions.
//!
//! Titles and numerals are not typed, they are cut: the plaque face is
//! Jersey 20, a pixel display face with the bearing of a scoreboard or a
//! machine's nameplate — condensed, square-cornered, and made of cells.
//! Body text is the main face; this one is for the few words that are
//! carved: the readout, the tempo, a number on a head, a panel's name,
//! the chord in the codebook.
//!
//! It used to be a hand-built five-by-seven grid drawn as rectangles.
//! The grid's UNITS survive as the face's sizes — a `unit` is one cell
//! of the old grid, and a word at `unit` stands as tall as the old word
//! did — so every plaque on the deck kept its place and its scale when
//! the letterforms changed. OFL, bundled: `assets/fonts/Jersey20-OFL.txt`.

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, pos2, vec2};

/// The font family the face is installed under.
pub const PLAQUE: &str = "plaque";

/// Cells across one glyph, in the old grid. Kept as the meaning of a
/// `unit`: a word at `unit` is `ROWS * unit` points tall.
pub const ROWS: usize = 7;

/// Cells across one glyph.
pub mod unit {
    /// A number on a head, a dB mark: 14 pt tall.
    pub const MICRO: f32 = 2.0;
    /// A panel title: 21 pt.
    pub const TITLE: f32 = 3.0;
    /// The readout: 28 pt.
    pub const DISPLAY: f32 = 4.0;
}

/// The face's capital height as a share of its em. Jersey 20 carries
/// its caps high in the em; this is what makes a word at `unit` stand
/// as tall as the grid it replaced.
const CAP_PER_EM: f32 = 0.74;
/// A typical glyph's advance as a share of the em, for the estimate a
/// caller makes before it has a painter to measure with.
const ADVANCE_PER_EM: f32 = 0.47;

/// The height of a word at `unit`: the old grid's height, in points.
pub fn height(unit: f32) -> f32 {
    ROWS as f32 * unit.max(1.0)
}

/// The font size that stands a capital `height(unit)` tall.
pub fn size(unit: f32) -> f32 {
    (height(unit) / CAP_PER_EM).round()
}

/// The face at `unit`.
pub fn font(unit: f32) -> FontId {
    FontId::new(size(unit), FontFamily::Name(PLAQUE.into()))
}

/// An ESTIMATE of a word's width at `unit`, for laying out before a
/// painter is at hand. Measure with [`measure`] where one is.
pub fn width(text: &str, unit: f32) -> f32 {
    text.chars().count() as f32 * size(unit) * ADVANCE_PER_EM
}

/// A word's width at `unit`, measured on the face.
pub fn measure(painter: &egui::Painter, text: &str, unit: f32) -> f32 {
    painter
        .layout_no_wrap(carve(text), font(unit), Color32::WHITE)
        .size()
        .x
}

/// Where `text` lands when anchored at `anchor` by `align`, by estimate.
pub fn place(anchor: Pos2, align: Align2, unit: f32, text: &str) -> Rect {
    align.anchor_size(anchor, vec2(width(text, unit), height(unit)))
}

/// An inscription is carved in capitals.
fn carve(words: &str) -> String {
    words.to_uppercase()
}

/// Cut `text`, anchored like `painter.text`. The id is kept for the
/// call sites that name their plaques; the atlas does the caching now.
pub fn paint(
    painter: &egui::Painter,
    _id: egui::Id,
    anchor: Pos2,
    align: Align2,
    unit: f32,
    words: &str,
    ink: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(carve(words), font(unit), ink);
    let rect = align.anchor_size(anchor, galley.size());
    let rect = Rect::from_min_size(pos2(rect.min.x.round(), rect.min.y.round()), rect.size());
    painter.galley(rect.min, galley, ink);
    rect
}

/// Cut `text` reading upward from `origin`, a quarter turn
/// anticlockwise: the word climbs a rail, its foot at `origin`. Returns
/// the rect it covers.
pub fn paint_vertical(
    painter: &egui::Painter,
    origin: Pos2,
    unit: f32,
    words: &str,
    ink: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(carve(words), font(unit), ink);
    let size = galley.size();
    let origin = pos2(origin.x.round(), origin.y.round());
    painter.add(egui::Shape::Text(
        egui::epaint::TextShape::new(origin, galley, ink).with_angle(-core::f32::consts::FRAC_PI_2),
    ));
    Rect::from_min_size(pos2(origin.x, origin.y - size.x), vec2(size.y, size.x))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The units keep the old grid's heights, so every plaque on the
    /// deck stands where it stood.
    #[test]
    fn the_units_are_the_old_grids_heights() {
        assert_eq!(height(unit::MICRO), 14.0);
        assert_eq!(height(unit::TITLE), 21.0);
        assert_eq!(height(unit::DISPLAY), 28.0);
        assert!(size(unit::MICRO) < size(unit::TITLE));
        assert!(size(unit::TITLE) < size(unit::DISPLAY));
        assert!(
            size(unit::MICRO) > height(unit::MICRO),
            "the em is taller than its caps"
        );
    }

    #[test]
    fn the_width_estimate_is_a_pure_function_of_length() {
        assert_eq!(width("", unit::TITLE), 0.0);
        assert_eq!(width("ABC", unit::TITLE), width("XYZ", unit::TITLE));
        assert!(width("ABCD", unit::TITLE) > width("ABC", unit::TITLE));
        assert!(width("AB", unit::DISPLAY) > width("AB", unit::MICRO));
    }

    #[test]
    fn a_placed_word_is_anchored_as_asked() {
        let rect = place(pos2(100.0, 50.0), Align2::RIGHT_BOTTOM, unit::MICRO, "AB");
        assert_eq!(rect.max, pos2(100.0, 50.0));
        assert_eq!(rect.height(), height(unit::MICRO));
        let rect = place(pos2(10.0, 10.0), Align2::LEFT_TOP, unit::TITLE, "AB");
        assert_eq!(rect.min, pos2(10.0, 10.0));
    }

    #[test]
    fn an_inscription_is_carved_in_capitals() {
        assert_eq!(carve("Trig 01"), "TRIG 01");
    }
}
