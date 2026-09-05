//! CEILING's face: a loaded roof beam, and the headroom under it.
//!
//! The mix's limiter has no controls at all, so its whole face is a
//! reading — and it is the only card on the desk that can show
//! something before it happens. The beam is the ceiling and never
//! moves. Under it the mix stands as a column. Beside that column, the
//! LOOKAHEAD window rises a moment EARLY, because the limiter can
//! already see the transient the column has not reached yet.
//!
//! Then the piston comes down: where the smoothed gain actually stands,
//! which recovers over its own release long after the block that caused
//! it has gone. The scar on the beam is what the worst block cost.

use super::*;
use crate::ui::nav_cursor;

/// The scale the card is drawn on, from the beam downward.
const FLOOR_DB: f32 = -30.0;
/// The deepest the piston is drawn pressing, in dB.
const PRESS_DB: f32 = 12.0;

struct Lay {
    /// The whole reading. CEILING has no controls, so it has no
    /// grabbable instruments — the layout is empty by design.
    beam: egui::Rect,
    column: egui::Rect,
    window: egui::Rect,
    piston: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        Vec::new()
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let beam = egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + 5.0));
    let body = egui::Rect::from_min_max(
        egui::pos2(inner.left(), beam.bottom() + 4.0),
        egui::pos2(inner.right(), inner.bottom() - 4.0),
    );
    let third = body.width() / 3.0;
    Lay {
        beam,
        column: egui::Rect::from_min_max(
            egui::pos2(body.left() + 4.0, body.top()),
            egui::pos2(body.left() + third - 4.0, body.bottom()),
        ),
        // The lookahead window takes the bay when the chassis cuts one:
        // it is the narrow thing that stands ahead of everything else.
        window: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(body.left() + third + 4.0, body.top()),
                egui::pos2(body.left() + third * 2.0 - 4.0, body.bottom()),
            )
        }),
        piston: plinth.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(body.left() + third * 2.0 + 4.0, body.top()),
                egui::pos2(body.right() - 4.0, body.bottom()),
            )
        }),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    // THE BEAM: the ceiling itself, and it never moves. Everything on
    // the card is read against it.
    shapes.push(egui::Shape::rect_filled(lay.beam, 0.0, ink));
    // The scar: what the worst block of this run cost, held long.
    let scar = face.anim(
        "scar",
        (said.reduction_db.abs() / PRESS_DB).clamp(0.0, 1.0),
        0.7,
    );
    if scar > 0.01 {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                lay.beam.min,
                egui::pos2(
                    egui::lerp(lay.beam.x_range(), scar),
                    lay.beam.bottom() + 2.0,
                ),
            ),
            0.0,
            face.hot(),
        ));
    }

    // THE COLUMN: the mix as it left. It is measured after the limiter,
    // so it can approach the beam and never pass it.
    let level = face.anim("level", said.level_db.clamp(FLOOR_DB, 0.0), 0.06);
    let top = tool::level_y(lay.column, level, FLOOR_DB);
    tool::well(&mut shapes, lay.column, face.ground(), edge);
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(lay.column.left() + 2.0, top),
            egui::pos2(lay.column.right() - 2.0, lay.column.bottom() - 2.0),
        ),
        0.0,
        tool::fade(face.live(), 0.75),
    ));
    // The ceiling, scribed on the column's own scale.
    let ceiling_y = tool::level_y(lay.column, said.bands[0], FLOOR_DB);
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.column.left() - 2.0, ceiling_y),
            egui::pos2(lay.column.right() + 2.0, ceiling_y),
        ],
        Weight::Heavy,
        ink,
    );

    // THE WINDOW: the loudest sample anywhere inside the lookahead — the
    // one figure on the whole desk that comes from AHEAD of the
    // playhead. It rises before the piston moves, which is the entire
    // reason a look-ahead limiter sounds different from one without.
    let ahead = face.anim("ahead", said.bands[2].clamp(FLOOR_DB, 6.0), 0.03);
    let ahead_y = tool::level_y(lay.window, ahead, FLOOR_DB);
    tool::well(&mut shapes, lay.window, face.ground(), edge);
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(lay.window.left() + 2.0, ahead_y),
            egui::pos2(lay.window.right() - 2.0, lay.window.bottom() - 2.0),
        ),
        0.0,
        tool::fade(face.hot(), 0.5),
    ));
    // Over it, where it will meet the ceiling.
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(
                lay.window.left(),
                tool::level_y(lay.window, said.bands[0], FLOOR_DB),
            ),
            egui::pos2(
                lay.window.right(),
                tool::level_y(lay.window, said.bands[0], FLOOR_DB),
            ),
        ],
        Weight::Hair,
        tool::fade(ink, 0.8),
    );

    // THE PISTON: where the smoothed gain actually stands, pressing
    // down from the beam. It recovers over its own release, long after
    // the block that caused it has gone — which is why the card carries
    // both this and the scar.
    let held = face.anim(
        "held",
        (said.bands[1].abs() / PRESS_DB).clamp(0.0, 1.0),
        0.05,
    );
    tool::well(&mut shapes, lay.piston, face.ground(), edge);
    let travel = lay.piston.height() - 8.0;
    let face_y = lay.piston.top() + 4.0 + travel * held;
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(lay.piston.left() + 2.0, lay.piston.top() + 2.0),
            egui::pos2(lay.piston.right() - 2.0, face_y),
        ),
        0.0,
        tool::fade(edge, 1.2),
    ));
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.piston.left() + 1.0, face_y),
            egui::pos2(lay.piston.right() - 1.0, face_y),
        ],
        Weight::Heavy,
        tool::mix_ink(ink, face.hot(), held),
    );
    // The rod, so the piston is plainly hung from the beam.
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.piston.center().x, lay.beam.bottom()),
            egui::pos2(lay.piston.center().x, lay.piston.top()),
        ],
        Weight::Hair,
        tool::fade(edge, 0.9),
    );

    face.painter.extend(shapes);

    // No parameter, so no grab — but the mark still wears the section
    // wherever the keyboard has parked on the card.
    face.mark_signed(&lay, nav_cursor::Signature::Squeeze(held));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Ceiling), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Ceiling).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Ceiling),
                strip::plinth_rect(piece, SectionKind::Ceiling),
            ),
        )
    }

    /// The limiter has no controls at all, so it has no instruments to
    /// grab — and the table agrees.
    #[test]
    fn the_limiter_is_all_readout_and_no_control() {
        let (_, lay) = laid();
        assert!(SectionKind::Ceiling.table().is_empty());
        assert!(lay.controls().is_empty());
        assert_eq!(lay.control(0), None);
    }

    /// The three readings stand apart, all under the beam, and the beam
    /// is at the top where a ceiling belongs.
    #[test]
    fn the_beam_is_over_all_three_readings() {
        let (piece, lay) = laid();
        for part in [lay.column, lay.window, lay.piston] {
            assert!(part.is_positive());
            assert!(
                part.top() >= lay.beam.bottom() - 0.01,
                "a reading is over the beam"
            );
            assert!(piece.expand(strip::TONGUE).contains_rect(part));
        }
        assert!(!lay.column.intersects(lay.window));
        assert!(!lay.window.intersects(lay.piston));
        // Silence is at the foot and the ceiling near the head.
        assert!(
            tool::level_y(lay.column, FLOOR_DB, FLOOR_DB)
                > tool::level_y(lay.column, 0.0, FLOOR_DB)
        );
    }
}
