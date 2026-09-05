//! FOUR's face: the console EQ, with the parts of the sum drawn as well
//! as the sum.
//!
//! Four bands with their own frequencies, and a four-band EQ's whole
//! difficulty is that the composite hides them — two mids fighting each
//! other read as one gentle curve. So every band is drawn behind the
//! sum, faint, and the one under the cursor comes up bright.
//!
//! Two things here are not textbook and both are visible:
//!
//! - The **INDUCTOR BUMP**. A passive low shelf built round an inductor
//!   resonates just inside its corner, and that dip-then-lift is why an
//!   old EQ's bottom sounds tight where a clean shelf sounds woolly. It
//!   is drawn as its own ghost, and switching the low band to a BELL
//!   takes it away — because the bump was the shelf's.
//! - The **ZONES**. The section measures three of them, cut at 200 Hz
//!   and 2 kHz, and the plot's own ground is tinted by what they read.
//!   The background IS the meter, cut exactly where the measurement is,
//!   so a boost landing in the zone that is already loudest says so
//!   without a second instrument to look at.

use super::*;
use crate::ui::chrome;

/// One control's row inside a band column.
/// @tune 10..24 px
const ROW_H: f32 = 14.0;
/// Between two band columns. Wide enough that one column's figure and
/// the next column's name cannot be read as one line.
const COLUMN_GAP: f32 = 13.0;
/// The response's window, in dB either way.
const SPAN_DB: f32 = 16.0;
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;
/// The rate the response is drawn at.
const DRAWN_AT: f32 = 48_000.0;

/// FOUR's twelve parameters: four columns of three.
#[derive(Clone, Copy, Debug)]
struct FourFace {
    curve: egui::Rect,
    /// Column by band, row by control: hz, db, then shape or Q.
    cells: [[egui::Rect; 3]; 4],
}

/// Which parameter stands in each cell, by column and row.
fn ids() -> [[u32; 3]; 4] {
    use crate::params::console::four as p;
    [
        [p::LOW_HZ, p::LOW_DB, p::LOW_SHAPE],
        [p::LMF_HZ, p::LMF_DB, p::LMF_Q],
        [p::HMF_HZ, p::HMF_DB, p::HMF_Q],
        [p::HIGH_HZ, p::HIGH_DB, p::HIGH_SHAPE],
    ]
}

impl Layout for FourFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        let ids = ids();
        let mut out = Vec::with_capacity(12);
        for (column, rects) in self.cells.iter().enumerate() {
            for (row, rect) in rects.iter().enumerate() {
                out.push((ids[column][row], *rect));
            }
        }
        out
    }
}

fn four_face(glass: egui::Rect) -> FourFace {
    // The glass is not the frame. It runs wider than the casing draws on
    // the right, and the casing's own plinth — which carries the
    // section's word — stands across its foot. So the inset is not
    // symmetric: the last column has to end inside the frame and the
    // bottom row has to sit above the plinth, not on it.
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 2.0),
        egui::pos2(glass.right() - 24.0, glass.bottom() - 13.0),
    );
    let row_h = crate::tune!(ROW_H);
    // Measured from the FOOT up. The columns and the zone line need
    // exactly what they need; the response takes everything left, which
    // is how it stays the biggest thing on the card on any glass — and
    // how the card stops running off the bottom of a short one.
    let rows_h = row_h * 3.0 + 2.0;
    // The casing's own plinth stands under the glass, so the card keeps
    // no row of its own down there: the columns sit on the bottom and
    // the response takes everything above them.
    let columns_top = x.bottom() - rows_h;
    let column_w = (x.width() - COLUMN_GAP * 3.0) / 4.0;
    let mut cells = [[egui::Rect::NOTHING; 3]; 4];
    for (column, rects) in cells.iter_mut().enumerate() {
        let left = x.left() + column as f32 * (column_w + COLUMN_GAP);
        for (row, rect) in rects.iter_mut().enumerate() {
            *rect = egui::Rect::from_min_size(
                egui::pos2(left, columns_top + row as f32 * (row_h + 1.0)),
                egui::vec2(column_w, row_h),
            );
        }
    }
    let curve = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right(), (columns_top - 5.0).max(x.top() + 24.0)),
    );
    FourFace { curve, cells }
}

/// Where a frequency stands across the plot, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::four_curve as fc;
    use crate::params::console::four as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = four_face(face.glass);
    let shape = fc::Shape::of(&face.params);
    let flat = shape.is_flat();
    // Each band keeps one hue, from the alphabet, in frequency order.
    let hues = [
        alpha.jeopardy_latent.color,
        ink,
        alpha.live_dim.color,
        alpha.live.color,
    ];
    // Which band the keyboard is standing in, if any.
    let ids = ids();
    let standing = face.selected.and_then(|param| {
        ids.iter()
            .position(|column| column.iter().any(|id| *id as usize == param))
    });
    // The three zones, as the section measured them.
    let zones = face.said.bands;
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.curve,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(lay.curve.left() + 7.0, lay.curve.top() + font.size + 4.0),
        egui::pos2(
            lay.curve.right() - 7.0,
            lay.curve.bottom() - font.size - 5.0,
        ),
    );
    let x_of = |hz: f32| plot.left() + place_of_hz(hz) * plot.width();
    let y_of = |db: f32| plot.center().y - (db / SPAN_DB).clamp(-1.2, 1.2) * plot.height() * 0.5;

    // ---- The zones: the ground IS the meter. -------------------------
    let edges = [
        plot.left(),
        x_of(p::ZONE_LOW_HZ),
        x_of(p::ZONE_HIGH_HZ),
        plot.right(),
    ];
    for (i, zone_db) in zones.iter().enumerate() {
        let lit = if quiet || *zone_db <= p::ZONE_FLOOR_DB {
            0.0
        } else {
            ((zone_db - p::ZONE_FLOOR_DB) / -p::ZONE_FLOOR_DB).clamp(0.0, 1.0)
        };
        if lit <= 0.001 {
            continue;
        }
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(edges[i], plot.top()),
                egui::pos2(edges[i + 1], plot.bottom()),
            ),
            0.0,
            alpha.live_dim.color.gamma_multiply(0.10 + lit * 0.30),
        ));
    }
    // The two cuts, drawn where the measurement is actually made.
    for x in [edges[1], edges[2]] {
        chrome::trace(
            &mut shapes,
            &[egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            Weight::Hair,
            edge.gamma_multiply(0.55),
        );
    }
    for db in [-12.0f32, 12.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), y_of(db)),
                egui::pos2(plot.right(), y_of(db)),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.28),
        );
    }
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(plot.left(), y_of(0.0)),
            egui::pos2(plot.right(), y_of(0.0)),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.75),
    );

    // ---- The parts, then the sum. ------------------------------------
    let sweep = |f: &dyn Fn(f32) -> f32| -> Vec<egui::Pos2> {
        (0..=84)
            .map(|i| {
                let t = i as f32 / 84.0;
                let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
                egui::pos2(plot.left() + t * plot.width(), y_of(f(hz)))
            })
            .collect()
    };
    for (i, band) in shape.bands.iter().enumerate() {
        if band.db == 0.0 {
            continue;
        }
        let bright = standing == Some(i);
        chrome::trace(
            &mut shapes,
            &sweep(&|hz| fc::band_db(band, DRAWN_AT, hz)),
            Weight::Hair,
            if bright {
                hues[i]
            } else {
                hues[i].gamma_multiply(0.40)
            },
        );
    }
    // The inductor's own bump, dashed: it belongs to the shelf, and
    // switching the low band to a bell takes it away.
    if let Some(bump) = shape.inductor {
        chrome::dashes(
            &mut shapes,
            &sweep(&|hz| fc::band_db(&bump, DRAWN_AT, hz)),
            0.0,
            Weight::Hair,
            alpha.jeopardy_latent.color.gamma_multiply(0.85),
        );
    }
    chrome::trace(
        &mut shapes,
        &sweep(&|hz| fc::response_db(&shape, DRAWN_AT, hz)),
        Weight::Heavy,
        if flat { edge.gamma_multiply(0.85) } else { ink },
    );
    // Each band stands on the curve at its own frequency.
    for (i, band) in shape.bands.iter().enumerate() {
        let at = egui::pos2(
            x_of(band.hz),
            y_of(fc::response_db(&shape, DRAWN_AT, band.hz)),
        );
        chrome::pad(&mut shapes, at, chrome::PAD, hues[i], band.db != 0.0);
        if standing == Some(i) {
            chrome::brackets(
                &mut shapes,
                egui::Rect::from_center_size(at, egui::Vec2::splat(13.0)),
                3.0,
                Weight::Hair,
                hues[i],
            );
        }
    }
    painter.extend(shapes);

    // ---- The four columns. -------------------------------------------
    let label = |at: egui::Pos2, align: egui::Align2, text: String, ink| {
        painter.text(at, align, text, font.clone(), ink);
    };
    let hz_word = |hz: f32| {
        if hz >= 1000.0 {
            format!("{:.1}k", hz / 1000.0)
        } else {
            format!("{hz:.0}")
        }
    };
    let words = ["LOW", "LMF", "HMF", "HIGH"];
    for (i, band) in shape.bands.iter().enumerate() {
        let hue = hues[i];
        let column = lay.cells[i];
        // A hairline down each column's left, in the band's own hue, so
        // the four read as four rather than as a table of twelve.
        {
            let mut rule = Vec::new();
            chrome::trace(
                &mut rule,
                &[
                    egui::pos2(column[0].left() - 4.0, column[0].top()),
                    egui::pos2(column[0].left() - 4.0, column[2].bottom()),
                ],
                Weight::Hair,
                hue.gamma_multiply(0.55),
            );
            painter.extend(rule);
        }
        // The name sits on the frequency row's left; the column is
        // narrow and a heading of its own would cost a whole row.
        label(
            egui::pos2(column[0].left() + 2.0, column[0].center().y),
            egui::Align2::LEFT_CENTER,
            words[i].to_owned(),
            hue,
        );
        label(
            egui::pos2(column[0].right() - 3.0, column[0].center().y),
            egui::Align2::RIGHT_CENTER,
            hz_word(band.hz),
            ink,
        );
        // The gain row is a bipolar bar as well as a figure, so a column
        // is a picture before it is three numbers.
        let bar = egui::Rect::from_min_max(
            egui::pos2(column[1].left() + 2.0, column[1].center().y - 3.0),
            egui::pos2(column[1].right() - 44.0, column[1].center().y + 3.0),
        );
        if bar.is_positive() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(bar.left(), bar.center().y - 0.5),
                    egui::pos2(bar.right(), bar.center().y + 0.5),
                ),
                0.0,
                edge.gamma_multiply(0.5),
            );
            let centre = bar.center().x;
            let reach = (band.db / 15.0).clamp(-1.0, 1.0) * bar.width() * 0.5;
            if reach.abs() > 0.5 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(centre.min(centre + reach), bar.top()),
                        egui::pos2(centre.max(centre + reach), bar.bottom()),
                    ),
                    0.0,
                    hue,
                );
            }
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(centre - 0.5, bar.top() - 2.0),
                    egui::pos2(centre + 0.5, bar.bottom() + 2.0),
                ),
                0.0,
                edge,
            );
        }
        label(
            egui::pos2(column[1].right() - 3.0, column[1].center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{:+.1}", band.db),
            if band.db == 0.0 { edge } else { ink },
        );
        // The third row is a shape on the outer bands and a Q on the
        // mids: the two things a four-band actually differs on.
        let outer = i == 0 || i == 3;
        if outer {
            let bell = band.shape == crate::dsp::filters::BandShape::Bell;
            let half = column[2].width() * 0.5;
            for (cell, word, chosen) in [
                (
                    egui::Rect::from_min_size(
                        column[2].min,
                        egui::vec2(half - 2.0, column[2].height()),
                    ),
                    "SHLF",
                    !bell,
                ),
                (
                    egui::Rect::from_min_size(
                        egui::pos2(column[2].left() + half, column[2].top()),
                        egui::vec2(half, column[2].height()),
                    ),
                    "BELL",
                    bell,
                ),
            ] {
                if chosen {
                    let mut marks = Vec::new();
                    chrome::brackets(&mut marks, cell, 3.0, Weight::Hair, hue);
                    painter.extend(marks);
                }
                label(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    word.to_owned(),
                    if chosen { ink } else { edge },
                );
            }
        } else {
            label(
                egui::pos2(column[2].left() + 2.0, column[2].center().y),
                egui::Align2::LEFT_CENTER,
                "Q".to_owned(),
                edge,
            );
            label(
                egui::pos2(column[2].right() - 3.0, column[2].center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.2}", band.q),
                ink,
            );
        }
    }

    // ---- The words on the plot, and the zones' own figures. ----------
    label(
        egui::pos2(lay.curve.left() + 8.0, lay.curve.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "RESPONSE".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.curve.right() - 8.0, lay.curve.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if flat {
            "WIRE".to_owned()
        } else {
            "IN".to_owned()
        },
        if flat { edge } else { alpha.live.color },
    );
    if let Some(bump) = shape.inductor {
        label(
            egui::pos2(
                x_of(bump.hz),
                y_of(fc::band_db(&bump, DRAWN_AT, bump.hz))
                    + if bump.db < 0.0 { 2.0 } else { -12.0 },
            ),
            egui::Align2::CENTER_TOP,
            "IND".to_owned(),
            alpha.jeopardy_latent.color,
        );
    }
    for (hz, word) in [(100.0f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        label(
            egui::pos2(x_of(hz), lay.curve.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            word.to_owned(),
            edge,
        );
    }
    // Each zone's reading stands INSIDE the zone it measures, along the
    // plot's own foot. A figure in the region it belongs to needs no
    // legend, and the card keeps no row of its own for it — the casing's
    // plinth stands where such a row would have gone.
    for (i, zone_db) in zones.iter().enumerate() {
        label(
            egui::pos2((edges[i] + edges[i + 1]) * 0.5, plot.bottom() - 1.0),
            egui::Align2::CENTER_BOTTOM,
            if quiet || *zone_db <= p::ZONE_FLOOR_DB {
                "--".to_owned()
            } else {
                format!("{zone_db:.0}")
            },
            if quiet { edge } else { alpha.live.color },
        );
    }

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 210.0))
    }

    /// Twelve parameters, twelve cells, none of them overlapping and
    /// none of them on the response.
    #[test]
    fn every_four_parameter_has_its_own_cell() {
        let face = four_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 12);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(
                glass().contains_rect(*rect),
                "parameter {id} left the glass"
            );
            assert!(rect.is_positive(), "parameter {id} lost its cell");
            assert!(
                !face.curve.intersects(*rect),
                "parameter {id} invaded the response"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "parameters {id} and {other} overlap"
                );
            }
        }
    }

    /// The columns stand in frequency order, which is the order the
    /// curve above them reads.
    #[test]
    fn the_columns_stand_in_frequency_order() {
        let face = four_face(glass());
        for pair in face.cells.windows(2) {
            assert!(
                pair[0][0].right() <= pair[1][0].left(),
                "the columns are out of order"
            );
        }
        // Every column's three rows are stacked, and level with its
        // neighbours' — a row means the same thing across the card.
        for column in &face.cells {
            assert!(column[0].bottom() <= column[1].top());
            assert!(column[1].bottom() <= column[2].top());
        }
        for row in 0..3 {
            let y = face.cells[0][row].y_range();
            for column in &face.cells {
                assert_eq!(column[row].y_range(), y, "row {row} is ragged");
            }
        }
    }

    /// The zones are cut on the two frequencies the section measures on,
    /// not on two that merely look tidy.
    #[test]
    fn the_zone_cuts_are_the_measurement_cuts() {
        use crate::params::console::four as p;
        assert!(place_of_hz(p::ZONE_LOW_HZ) > 0.0);
        assert!(place_of_hz(p::ZONE_HIGH_HZ) < 1.0);
        assert!(place_of_hz(p::ZONE_LOW_HZ) < place_of_hz(p::ZONE_HIGH_HZ));
    }

    /// The inductor's bump is the shelf's: a low SHELF has one and a low
    /// BELL does not, which is the one place a switch changes two things.
    #[test]
    fn the_bump_belongs_to_the_shelf() {
        use crate::console::SectionKind;
        use crate::console::four_curve::Shape;
        use crate::params::console::four as p;
        let mut params = crate::console::SectionParams::of(SectionKind::Four);
        params.set(p::LOW_DB, 8.0);
        params.set(p::LOW_SHAPE, p::SHELF as f32);
        assert!(
            Shape::of(&params).inductor.is_some(),
            "a shelf lost its bump"
        );
        params.set(p::LOW_SHAPE, p::BELL as f32);
        assert!(Shape::of(&params).inductor.is_none(), "a bell grew a bump");
    }
}
