//! Family marks: a tiny drawing of what a group of devices DOES.
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

/// One family's mark.
///
/// The set is CLOSED and the shapes are deliberately unalike — see
/// `no_two_families_draw_the_same_mark`. Two families that looked
/// similar at eleven points would be worse than no marks at all,
/// because a reader would trust them and be wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// A transfer curve with a knee in it: unity, then bending over.
    Dynamics,
    /// A bell.
    Filter,
    /// Taps, decaying — a delay and its reflections.
    Time,
    /// A sine with its peaks flattened.
    Drive,
    /// A sine, whole. The only smooth periodic mark, which is what
    /// tells it from `Drive` at a glance.
    Modulation,
    /// Bins of unequal height: a spectrum.
    Spectral,
    /// A junction — signal crossing, going elsewhere.
    Utility,
    /// A stem and a head. The one mark that is an OBJECT rather than a
    /// response, because an instrument is a source and has no input to
    /// draw a response to.
    Instrument,
    /// A stack. For a container of families rather than a family: it is
    /// the only mark with no signal in it, which is the point.
    Stack,
}

/// Points on the unit square, `(0,0)` top-left, as the mark is drawn.
///
/// Returned rather than painted so the shapes can be measured — the
/// test that holds them apart needs the geometry, not a screenshot.
pub fn strokes(glyph: Glyph) -> Vec<Vec<(f32, f32)>> {
    let sine = |flat: bool| {
        (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                let y = (t * std::f32::consts::TAU).sin();
                let y = if flat { y.clamp(-0.62, 0.62) / 0.62 } else { y };
                (t, 0.5 - y * 0.42)
            })
            .collect::<Vec<_>>()
    };
    match glyph {
        // Unity to the threshold, then the ratio takes over. The bend is
        // the whole content of the mark.
        Glyph::Dynamics => vec![vec![(0.0, 1.0), (0.45, 0.55), (1.0, 0.32)]],
        // A bell, sampled rather than approximated with corners: the
        // point of this family is that its response is smooth.
        Glyph::Filter => vec![
            (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let x = (t - 0.5) * 5.0;
                    (t, 1.0 - (-x * x * 0.5).exp() * 0.92)
                })
                .collect(),
        ],
        // Three taps, each shorter than the last, standing on a floor.
        Glyph::Time => vec![
            vec![(0.08, 0.05), (0.08, 1.0)],
            vec![(0.5, 0.42), (0.5, 1.0)],
            vec![(0.88, 0.68), (0.88, 1.0)],
        ],
        Glyph::Drive => vec![sine(true)],
        Glyph::Modulation => vec![sine(false)],
        // Bins. Unequal on purpose — an even row would read as a mask
        // or a grille rather than as a spectrum.
        Glyph::Spectral => vec![
            vec![(0.06, 0.62), (0.06, 1.0)],
            vec![(0.29, 0.18), (0.29, 1.0)],
            vec![(0.52, 0.78), (0.52, 1.0)],
            vec![(0.75, 0.34), (0.75, 1.0)],
            vec![(0.96, 0.86), (0.96, 1.0)],
        ],
        // A line going through, and one leaving it.
        Glyph::Utility => vec![
            vec![(0.0, 0.72), (1.0, 0.72)],
            vec![(0.5, 0.72), (0.5, 0.12)],
            vec![(0.28, 0.34), (0.5, 0.12), (0.72, 0.34)],
        ],
        // A stem with a head at its foot.
        Glyph::Instrument => vec![
            vec![(0.66, 0.86), (0.66, 0.06)],
            vec![(0.66, 0.06), (1.0, 0.16)],
            vec![(0.2, 0.72), (0.66, 0.72)],
        ],
        // Three rules, stacked. No signal in it at all.
        Glyph::Stack => vec![
            vec![(0.05, 0.24), (0.95, 0.24)],
            vec![(0.05, 0.5), (0.95, 0.5)],
            vec![(0.05, 0.76), (0.95, 0.76)],
        ],
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Glyph; 9] = [
        Glyph::Dynamics,
        Glyph::Filter,
        Glyph::Time,
        Glyph::Drive,
        Glyph::Modulation,
        Glyph::Spectral,
        Glyph::Utility,
        Glyph::Instrument,
        Glyph::Stack,
    ];

    /// Every mark is inside the box it is given.
    ///
    /// They sit in a row of text a few points tall; one that ran over
    /// would land on the name beside it.
    #[test]
    fn every_mark_stays_inside_its_cell() {
        for glyph in ALL {
            for run in strokes(glyph) {
                assert!(run.len() >= 2, "{glyph:?} has a run with nothing in it");
                for (x, y) in run {
                    assert!(
                        (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                        "{glyph:?} reaches ({x}, {y})"
                    );
                }
            }
        }
    }

    /// Rendered at the size it is actually drawn, each mark is a
    /// DIFFERENT PICTURE.
    ///
    /// Two families that looked alike at eleven points would be worse
    /// than no marks at all, because a reader would trust them and be
    /// wrong. So the shapes are rasterised into a coarse grid — about
    /// what the eye gets at this size — and every pair has to differ in
    /// a good share of it.
    #[test]
    fn no_two_families_draw_the_same_mark() {
        const N: usize = 9;
        let raster = |glyph: Glyph| {
            let mut grid = [false; N * N];
            for run in strokes(glyph) {
                for pair in run.windows(2) {
                    let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                    // Walk the segment finely enough that no cell is
                    // stepped over.
                    for step in 0..=64 {
                        let t = step as f32 / 64.0;
                        let x = x0 + (x1 - x0) * t;
                        let y = y0 + (y1 - y0) * t;
                        let cx = ((x * N as f32) as usize).min(N - 1);
                        let cy = ((y * N as f32) as usize).min(N - 1);
                        grid[cy * N + cx] = true;
                    }
                }
            }
            grid
        };
        let grids: Vec<_> = ALL.into_iter().map(|g| (g, raster(g))).collect();
        for (i, (a, ga)) in grids.iter().enumerate() {
            let lit = ga.iter().filter(|on| **on).count();
            assert!(lit >= 6, "{a:?} draws almost nothing: {lit} cells");
            for (b, gb) in grids.iter().skip(i + 1) {
                let differ = ga
                    .iter()
                    .zip(gb.iter())
                    .filter(|(one, two)| one != two)
                    .count();
                assert!(
                    differ >= 8,
                    "{a:?} and {b:?} differ in only {differ} of {} cells",
                    N * N
                );
            }
        }
    }

    /// The two periodic marks are the pair most at risk, so they are
    /// held to a wider margin: `Drive` is a sine with its peaks taken
    /// off, and if the flattening is ever tuned away they become the
    /// same drawing.
    #[test]
    fn drive_is_visibly_flatter_than_modulation() {
        let peak = |glyph: Glyph| {
            strokes(glyph)
                .into_iter()
                .flatten()
                .map(|(_, y)| (y - 0.5).abs())
                .fold(0.0f32, f32::max)
        };
        let flat_count = |glyph: Glyph| {
            let runs = strokes(glyph);
            let ys: Vec<f32> = runs.into_iter().flatten().map(|(_, y)| y).collect();
            ys.windows(2).filter(|w| (w[0] - w[1]).abs() < 1e-3).count()
        };
        // Same amplitude — the difference is the SHAPE of the top, not
        // how far it goes.
        assert!((peak(Glyph::Drive) - peak(Glyph::Modulation)).abs() < 0.02);
        assert!(
            flat_count(Glyph::Drive) > flat_count(Glyph::Modulation),
            "the drive mark has no flat on it"
        );
    }
}
