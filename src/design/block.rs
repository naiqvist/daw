//! The block face: the deck's inscriptions.
//!
//! Titles and numerals are not typed, they are cut: every glyph is a
//! five-by-seven grid of square cells, heavy and wide, with a stencil
//! break through the stem of every closed letter — the letterform of a
//! plaque on a machine nobody alive built. It is drawn as filled rects,
//! never through the font atlas, so its width is arithmetic rather than a
//! measurement and it looks the same at every size.
//!
//! Body text is the typewriter (iA Writer Mono); this face is for the few
//! words that are carved: the readout, the tempo, a number on a head, a
//! panel's name, the chord in the codebook.

use eframe::egui::{Align2, Color32, Pos2, Rect, Shape, pos2, vec2};

/// Cells across one glyph.
pub const COLS: usize = 5;
/// Cells down one glyph.
pub const ROWS: usize = 7;
/// Cells from one glyph's left edge to the next: five, and a gap.
pub const ADVANCE: usize = COLS + 1;

/// Sizes, as the cell's side in points.
pub mod unit {
    /// A number on a head, a dB mark: 14 pt tall.
    pub const MICRO: f32 = 2.0;
    /// A panel title: 21 pt.
    pub const TITLE: f32 = 3.0;
    /// The readout: 28 pt.
    pub const DISPLAY: f32 = 4.0;
}

/// Letters whose stem gets the stencil break at the middle row.
const STENCIL: &str = "ABDOPQR0689";

/// The glyph table: rows top to bottom, `1` is a filled cell.
const GLYPHS: &[(char, [&str; ROWS])] = &[
    (
        'A',
        [
            "11111", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
    ),
    (
        'B',
        [
            "11110", "10001", "10001", "11110", "10001", "10001", "11110",
        ],
    ),
    (
        'C',
        [
            "11111", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
    ),
    (
        'D',
        [
            "11110", "10001", "10001", "10001", "10001", "10001", "11110",
        ],
    ),
    (
        'E',
        [
            "11111", "10000", "10000", "11110", "10000", "10000", "11111",
        ],
    ),
    (
        'F',
        [
            "11111", "10000", "10000", "11110", "10000", "10000", "10000",
        ],
    ),
    (
        'G',
        [
            "11111", "10000", "10000", "10111", "10001", "10001", "11111",
        ],
    ),
    (
        'H',
        [
            "10001", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
    ),
    (
        'I',
        [
            "11111", "00100", "00100", "00100", "00100", "00100", "11111",
        ],
    ),
    (
        'J',
        [
            "00111", "00001", "00001", "00001", "00001", "10001", "11111",
        ],
    ),
    (
        'K',
        [
            "10001", "10010", "10100", "11000", "10100", "10010", "10001",
        ],
    ),
    (
        'L',
        [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
    ),
    (
        'M',
        [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
    ),
    (
        'N',
        [
            "10001", "11001", "10101", "10011", "10001", "10001", "10001",
        ],
    ),
    (
        'O',
        [
            "11111", "10001", "10001", "10001", "10001", "10001", "11111",
        ],
    ),
    (
        'P',
        [
            "11111", "10001", "10001", "11111", "10000", "10000", "10000",
        ],
    ),
    (
        'Q',
        [
            "11111", "10001", "10001", "10001", "10101", "10010", "11101",
        ],
    ),
    (
        'R',
        [
            "11111", "10001", "10001", "11111", "10100", "10010", "10001",
        ],
    ),
    (
        'S',
        [
            "11111", "10000", "10000", "11111", "00001", "00001", "11111",
        ],
    ),
    (
        'T',
        [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
    ),
    (
        'U',
        [
            "10001", "10001", "10001", "10001", "10001", "10001", "11111",
        ],
    ),
    (
        'V',
        [
            "10001", "10001", "10001", "10001", "10001", "01010", "00100",
        ],
    ),
    (
        'W',
        [
            "10001", "10001", "10001", "10101", "10101", "11011", "10001",
        ],
    ),
    (
        'X',
        [
            "10001", "10001", "01010", "00100", "01010", "10001", "10001",
        ],
    ),
    (
        'Y',
        [
            "10001", "10001", "01010", "00100", "00100", "00100", "00100",
        ],
    ),
    (
        'Z',
        [
            "11111", "00001", "00010", "00100", "01000", "10000", "11111",
        ],
    ),
    (
        '0',
        [
            "11111", "10001", "10011", "10101", "11001", "10001", "11111",
        ],
    ),
    (
        '1',
        [
            "00100", "01100", "00100", "00100", "00100", "00100", "11111",
        ],
    ),
    (
        '2',
        [
            "11111", "00001", "00001", "11111", "10000", "10000", "11111",
        ],
    ),
    (
        '3',
        [
            "11111", "00001", "00001", "01111", "00001", "00001", "11111",
        ],
    ),
    (
        '4',
        [
            "10001", "10001", "10001", "11111", "00001", "00001", "00001",
        ],
    ),
    (
        '5',
        [
            "11111", "10000", "10000", "11111", "00001", "00001", "11111",
        ],
    ),
    (
        '6',
        [
            "11111", "10000", "10000", "11111", "10001", "10001", "11111",
        ],
    ),
    (
        '7',
        [
            "11111", "00001", "00001", "00010", "00100", "00100", "00100",
        ],
    ),
    (
        '8',
        [
            "11111", "10001", "10001", "11111", "10001", "10001", "11111",
        ],
    ),
    (
        '9',
        [
            "11111", "10001", "10001", "11111", "00001", "00001", "11111",
        ],
    ),
    (
        '.',
        [
            "00000", "00000", "00000", "00000", "00000", "00000", "00100",
        ],
    ),
    (
        ':',
        [
            "00000", "00100", "00000", "00000", "00000", "00100", "00000",
        ],
    ),
    (
        '-',
        [
            "00000", "00000", "00000", "11111", "00000", "00000", "00000",
        ],
    ),
    (
        '/',
        [
            "00001", "00001", "00010", "00100", "01000", "10000", "10000",
        ],
    ),
    (
        '+',
        [
            "00000", "00100", "00100", "11111", "00100", "00100", "00000",
        ],
    ),
    (
        '%',
        [
            "11001", "11010", "00010", "00100", "01000", "01011", "10011",
        ],
    ),
    (
        '#',
        [
            "01010", "11111", "01010", "01010", "01010", "11111", "01010",
        ],
    ),
    (
        '\'',
        [
            "00100", "00100", "00000", "00000", "00000", "00000", "00000",
        ],
    ),
    (
        '(',
        [
            "00110", "01000", "01000", "01000", "01000", "01000", "00110",
        ],
    ),
    (
        ')',
        [
            "01100", "00010", "00010", "00010", "00010", "00010", "01100",
        ],
    ),
    (
        '*',
        [
            "00000", "10101", "01110", "11111", "01110", "10101", "00000",
        ],
    ),
    (
        ' ',
        [
            "00000", "00000", "00000", "00000", "00000", "00000", "00000",
        ],
    ),
];

/// The cell rows of one character, after the stencil rule. Lowercase is
/// folded to capitals; anything unknown is the full block.
pub fn rows_of(c: char) -> [u8; ROWS] {
    let c = c.to_ascii_uppercase();
    let (_, rows) = GLYPHS
        .iter()
        .find(|(k, _)| *k == c)
        .copied()
        .unwrap_or(('#', ["11111"; ROWS]));
    let mut bits = [0u8; ROWS];
    for (i, row) in rows.iter().enumerate() {
        let mut b = 0u8;
        for (col, ch) in row.chars().enumerate() {
            if ch == '1' {
                b |= 1 << (COLS - 1 - col);
            }
        }
        bits[i] = b;
    }
    if STENCIL.contains(c) {
        // the break: the left stem loses its middle cell
        bits[ROWS / 2] &= !(1 << (COLS - 1));
    }
    bits
}

/// The width of `text` at `unit`, exact.
pub fn width(text: &str, unit: f32) -> f32 {
    let n = text.chars().count();
    if n == 0 {
        return 0.0;
    }
    (n * ADVANCE - 1) as f32 * unit
}

/// The height of any text at `unit`.
pub fn height(unit: f32) -> f32 {
    ROWS as f32 * unit
}

/// Cut `text` at `origin` (its top-left), one filled rect per run of
/// cells. Returns the rect it covered.
pub fn text(out: &mut Vec<Shape>, origin: Pos2, unit: f32, text: &str, ink: Color32) -> Rect {
    let origin = pos2(origin.x.round(), origin.y.round());
    let unit = unit.max(1.0).round();
    let mut x = origin.x;
    for c in text.chars() {
        let rows = rows_of(c);
        for (row, bits) in rows.iter().enumerate() {
            let y = origin.y + row as f32 * unit;
            let mut col = 0;
            while col < COLS {
                if (bits >> (COLS - 1 - col)) & 1 == 1 {
                    let start = col;
                    while col < COLS && (bits >> (COLS - 1 - col)) & 1 == 1 {
                        col += 1;
                    }
                    out.push(Shape::rect_filled(
                        Rect::from_min_size(
                            pos2(x + start as f32 * unit, y),
                            vec2((col - start) as f32 * unit, unit),
                        ),
                        0.0,
                        ink,
                    ));
                } else {
                    col += 1;
                }
            }
        }
        x += ADVANCE as f32 * unit;
    }
    Rect::from_min_size(origin, vec2(width(text, unit), height(unit)))
}

/// Cut `text` reading upward, its baseline along the left of `origin`:
/// the label on a rail. `origin` is the bottom-left of the run.
pub fn text_vertical(
    out: &mut Vec<Shape>,
    origin: Pos2,
    unit: f32,
    text: &str,
    ink: Color32,
) -> Rect {
    let origin = pos2(origin.x.round(), origin.y.round());
    let unit = unit.max(1.0).round();
    let mut y = origin.y;
    for c in text.chars() {
        let rows = rows_of(c);
        for (row, bits) in rows.iter().enumerate() {
            // rotated a quarter turn anticlockwise: glyph rows become
            // columns from left to right, glyph columns run bottom to top
            let x = origin.x + row as f32 * unit;
            for col in 0..COLS {
                if (bits >> (COLS - 1 - col)) & 1 == 1 {
                    out.push(Shape::rect_filled(
                        Rect::from_min_size(pos2(x, y - (col + 1) as f32 * unit), vec2(unit, unit)),
                        0.0,
                        ink,
                    ));
                }
            }
        }
        y -= ADVANCE as f32 * unit;
    }
    Rect::from_min_size(
        pos2(origin.x, origin.y - width(text, unit)),
        vec2(height(unit), width(text, unit)),
    )
}

/// Where `text` lands when anchored at `anchor` by `align`.
pub fn place(anchor: Pos2, align: Align2, unit: f32, text: &str) -> Rect {
    align.anchor_size(anchor, vec2(width(text, unit), height(unit)))
}

/// Cut `text` through the mesh cache, anchored like `painter.text`.
pub fn paint(
    painter: &eframe::egui::Painter,
    id: eframe::egui::Id,
    anchor: Pos2,
    align: Align2,
    unit: f32,
    words: &str,
    ink: Color32,
) -> Rect {
    let rect = place(anchor, align, unit, words);
    let origin = rect.min;
    let owned = words.to_owned();
    super::kit::cached(painter, id, rect, (owned.clone(), ink), |out| {
        text(out, origin, unit, &owned, ink);
    });
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_is_a_pure_function_of_length() {
        assert_eq!(width("", 2.0), 0.0);
        assert_eq!(width("A", 2.0), 10.0);
        assert_eq!(width("AB", 2.0), 22.0);
        assert_eq!(width("MASTER", 3.0), (6 * 6 - 1) as f32 * 3.0);
        assert_eq!(height(4.0), 28.0);
    }

    #[test]
    fn every_glyph_is_in_the_table_once_and_fills_its_grid() {
        for (i, (c, rows)) in GLYPHS.iter().enumerate() {
            assert!(
                !GLYPHS[i + 1..].iter().any(|(k, _)| k == c),
                "{c:?} is in the table twice"
            );
            for row in rows {
                assert_eq!(row.len(), COLS, "{c:?} has a row of the wrong width");
                assert!(row.chars().all(|ch| ch == '0' || ch == '1'));
            }
            if *c != ' ' {
                assert!(rows.iter().any(|r| r.contains('1')), "{c:?} draws nothing");
            }
        }
    }

    #[test]
    fn the_stencil_breaks_every_closed_stem() {
        for c in STENCIL.chars() {
            let rows = rows_of(c);
            assert_eq!(
                rows[ROWS / 2] >> (COLS - 1) & 1,
                0,
                "{c} kept its stem whole"
            );
        }
        // and leaves an open letter alone
        assert_eq!(rows_of('E')[ROWS / 2] >> (COLS - 1) & 1, 1);
    }

    #[test]
    fn unknown_characters_become_the_block_and_case_is_folded() {
        assert_eq!(rows_of('?'), ["11111"; ROWS].map(|_| 0b11111));
        assert_eq!(rows_of('a'), rows_of('A'));
    }

    #[test]
    fn text_covers_exactly_the_rect_it_reports() {
        let mut out = Vec::new();
        let rect = text(&mut out, pos2(10.0, 20.0), 2.0, "AB0", Color32::WHITE);
        assert_eq!(
            rect,
            Rect::from_min_size(pos2(10.0, 20.0), vec2(width("AB0", 2.0), 14.0))
        );
        for s in &out {
            if let Shape::Rect(r) = s {
                assert!(
                    rect.contains_rect(r.rect),
                    "{:?} runs outside {rect:?}",
                    r.rect
                );
            }
        }
        assert!(
            out.len() < 60,
            "runs were not merged: {} rects for three glyphs",
            out.len()
        );
    }

    #[test]
    fn vertical_text_is_the_same_cells_turned_a_quarter() {
        let mut flat = Vec::new();
        let mut up = Vec::new();
        text(&mut flat, pos2(0.0, 0.0), 1.0, "L", Color32::WHITE);
        let rect = text_vertical(&mut up, pos2(0.0, 5.0), 1.0, "L", Color32::WHITE);
        assert_eq!(rect.width(), height(1.0));
        assert_eq!(rect.height(), width("L", 1.0));
        let cells = |shapes: &[Shape]| -> usize {
            shapes
                .iter()
                .map(|s| match s {
                    Shape::Rect(r) => (r.rect.width() * r.rect.height()) as usize,
                    _ => 0,
                })
                .sum()
        };
        assert_eq!(
            cells(&flat),
            cells(&up),
            "turning the glyph changed its cell count"
        );
    }
}
