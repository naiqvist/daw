//! SCOPE's face: a ternary plate, the mix as one point.
//!
//! Three band levels are three numbers, and three numbers with a common
//! total are a POINT — that is what a ternary diagram is for, and no
//! other card on the desk is one. Low, middle and high are the three
//! corners; where the mix stands between them is where the point sits.
//! A mix leaning on its bottom end sits near the low corner and walks
//! toward the middle as the balance changes, which a row of three bars
//! can never show as one thing.
//!
//! The point's SIZE is the mix's own level, so a quiet passage is a
//! small mark in the same place rather than a mark that has moved.
//! Three arms out to the corners carry the bands themselves, for the
//! reading that a triangle alone cannot give: how loud, not just how
//! balanced.

use super::*;
use crate::ui::nav_cursor;

/// The band scale the plate is read on.
const FLOOR_DB: f32 = -54.0;

struct Lay {
    /// The plate, and the three corners of it. SCOPE has no controls,
    /// so it has no grabbable instruments.
    plate: egui::Rect,
    corners: [egui::Pos2; 3],
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        Vec::new()
    }
}

fn lay(glass: egui::Rect) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let side = inner.width().min(inner.height() * 1.15);
    let plate = egui::Rect::from_center_size(
        egui::pos2(inner.center().x, inner.center().y),
        egui::vec2(side, inner.height()),
    );
    let field = plate.shrink(10.0);
    Lay {
        plate,
        // Low at the bottom left, high at the bottom right, middle at
        // the head — the way a spectrum is read, folded into a triangle.
        corners: [
            field.left_bottom(),
            egui::pos2(field.center().x, field.top()),
            field.right_bottom(),
        ],
    }
}

/// Where three band weights put the point on the plate.
fn point(corners: [egui::Pos2; 3], weights: [f32; 3]) -> egui::Pos2 {
    let total = weights.iter().sum::<f32>().max(1e-4);
    let mut at = egui::Vec2::ZERO;
    for (corner, weight) in corners.into_iter().zip(weights) {
        at += corner.to_vec2() * (weight / total);
    }
    at.to_pos2()
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass);
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    // THE PLATE: the triangle, its three corners marked in the band
    // hues the whole desk uses, and a lattice of thirds inside so the
    // point is read against something.
    circuit::trace(
        &mut shapes,
        &[
            lay.corners[0],
            lay.corners[1],
            lay.corners[2],
            lay.corners[0],
        ],
        Weight::Hair,
        tool::fade(edge, 1.1),
    );
    for step in 1..3 {
        let t = step as f32 / 3.0;
        for (a, b) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let from = lay.corners[a] + (lay.corners[b] - lay.corners[a]) * t;
            let c = (a + 2) % 3;
            let to = lay.corners[c] + (lay.corners[b] - lay.corners[c]) * t;
            circuit::trace(
                &mut shapes,
                &[from, to],
                Weight::Hair,
                tool::fade(edge, 0.22),
            );
        }
    }

    // THE ARMS: each corner's own band level, out along its own edge.
    // The triangle says the balance; the arms say the loudness.
    let mut weights = [0.0f32; 3];
    for band in 0..3 {
        let level = face.anim(
            ["slo", "smid", "shi"][band],
            said.bands[band].clamp(FLOOR_DB, 6.0),
            0.08,
        );
        let share = tool::norm(level, FLOOR_DB, 0.0);
        weights[band] = share.max(0.001);
        let corner = lay.corners[band];
        let inward = (lay.plate.center() - corner).normalized();
        circuit::trace(
            &mut shapes,
            &[corner, corner + inward * (lay.plate.width() * 0.30 * share)],
            Weight::Heavy,
            tool::fade(tool::BAND_INK[band], 0.35 + 0.65 * share),
        );
        circuit::pad(
            &mut shapes,
            corner,
            circuit::PAD,
            tool::BAND_INK[band],
            share > 0.02,
        );
    }

    // THE POINT: where the mix stands between the three. Its size is
    // the mix's own level, so a quiet passage is a small mark in the
    // same place rather than a mark that has wandered.
    let at = point(lay.corners, weights);
    let loud = tool::norm(said.level_db, FLOOR_DB, 0.0);
    circuit::octagon(
        &mut shapes,
        egui::Rect::from_center_size(at, egui::vec2(5.0 + 9.0 * loud, 5.0 + 9.0 * loud)),
        2.0,
        Some(tool::fade(face.live(), 0.35 + 0.55 * loud)),
        Some((Weight::Hair, ink)),
    );
    // The dead centre, for the balance a mix is read against.
    circuit::via(
        &mut shapes,
        point(lay.corners, [1.0, 1.0, 1.0]),
        tool::fade(edge, 1.1),
        face.ground(),
    );

    face.painter.extend(shapes);

    // No parameter to grab; the mark carries the mix's own level.
    face.mark_signed(&lay, nav_cursor::Signature::Level(loud));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Scope), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Scope).shrink(3.0);
        (piece, lay(glass))
    }

    /// The analyser has no controls, so it has no instruments to grab.
    #[test]
    fn the_analyser_is_all_readout() {
        let (_, lay) = laid();
        assert!(SectionKind::Scope.table().is_empty());
        assert!(lay.controls().is_empty());
    }

    /// Three levels are one point: a mix leaning on one band sits near
    /// that corner, and an even mix sits dead centre.
    #[test]
    fn three_bands_become_one_point_on_the_plate() {
        let (_, lay) = laid();
        let even = point(lay.corners, [1.0, 1.0, 1.0]);
        for band in 0..3 {
            let mut lean = [0.05f32; 3];
            lean[band] = 1.0;
            let at = point(lay.corners, lean);
            let to_corner = (at - lay.corners[band]).length();
            let even_to_corner = (even - lay.corners[band]).length();
            assert!(
                to_corner < even_to_corner,
                "leaning on band {band} did not walk toward its corner"
            );
        }
        // The point never leaves the triangle it is read on.
        for weights in [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            let at = point(lay.corners, weights);
            assert!(lay.plate.contains(at), "the point left the plate");
        }
        // And the centre really is between all three corners.
        let centre_x = (lay.corners[0].x + lay.corners[1].x + lay.corners[2].x) / 3.0;
        assert!((even.x - centre_x).abs() < 0.01);
    }
}
