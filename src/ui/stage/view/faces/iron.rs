//! IRON's face: a core-loss loop tracer clamped on the bus.
//!
//! One knob, so one instrument, and the instrument is the thing itself:
//! the B-H curve of the transformer the section is modelling, traced on
//! a scope the way a magnetics bench traces one. DRIVE opens the loop —
//! a lightly driven core is a thin diagonal, a hard-driven one is a fat
//! saturating loop with flat shoulders.
//!
//! What the drive knob cannot say, the section measures: how much EVEN
//! harmonic the curve is actually making, which tilts the loop off its
//! diagonal, and how much of what is driving the core is BASS, which is
//! what widens it. So two cores at the same drive setting, fed
//! differently, trace visibly different loops.

use super::*;
use crate::console::SectionParams;
use crate::params::console::iron as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

struct Lay {
    scope: egui::Rect,
    drive: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![(p::DRIVE, self.drive)]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let scope =
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.bottom() - 18.0));
    Lay {
        scope,
        // The drive is the loop's own opening, so the scope IS the
        // control; the bay carries its scale.
        drive: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(egui::pos2(inner.left(), scope.bottom() + 4.0), inner.max)
        }),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let drive = face.anim("drive", said.bands[0].clamp(0.0, 1.0), 0.12);
    let asym = face.anim("asym", said.bands[1].clamp(0.0, 1.0), 0.10);
    let bottom = face.anim("bottom", said.bands[2].clamp(0.0, 1.0), 0.12);
    let heat = said.reduction_db.abs().clamp(0.0, 1.0);

    // THE SCOPE: flux up, drive across, with the diagonal an ideal core
    // would trace so the departure from it is what the eye reads.
    tool::well(&mut shapes, lay.scope, face.ground(), edge);
    let field = lay.scope.shrink(4.0);
    let centre = field.center();
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), centre.y),
            egui::pos2(field.right(), centre.y),
        ],
        Weight::Hair,
        tool::fade(edge, 0.5),
    );
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(centre.x, field.top()),
            egui::pos2(centre.x, field.bottom()),
        ],
        Weight::Hair,
        tool::fade(edge, 0.5),
    );
    chrome::trace(
        &mut shapes,
        &[field.left_bottom(), field.right_top()],
        Weight::Hair,
        tool::fade(edge, 0.35),
    );

    // THE LOOP: a hysteresis curve. Its knee closes with the drive, the
    // even harmonic the section measured tilts it off the diagonal, and
    // the bass share opens the loop's belly — which is exactly what a
    // core fed low frequencies does.
    let half_w = field.width() * 0.42;
    let half_h = field.height() * 0.42;
    let knee = 0.6 + 3.4 * drive;
    let belly = 0.04 + 0.30 * bottom;
    let loop_path: Vec<egui::Pos2> = (0..=72)
        .map(|i| {
            let t = i as f32 / 72.0 * core::f32::consts::TAU;
            let x = t.cos();
            // The core's own curve: a soft knee that flattens as the
            // drive rises, tilted by the even harmonic content.
            let base = (x * knee).tanh() / knee.tanh();
            let tilt = asym * 0.22 * (1.0 - x * x);
            // The belly: the return path lags the outbound one, which
            // is what makes it a LOOP rather than a curve.
            let lag = belly * t.sin();
            egui::pos2(
                centre.x + x * half_w,
                centre.y - (base + tilt + lag).clamp(-1.4, 1.4) * half_h,
            )
        })
        .collect();
    chrome::trace(
        &mut shapes,
        &loop_path,
        Weight::Heavy,
        tool::mix_ink(ink, face.hot(), heat),
    );
    // The two shoulders: where the core has run out of iron.
    for side in [-1.0f32, 1.0] {
        chrome::pad(
            &mut shapes,
            // The curve is normalised so its ends are exactly the
            // shoulders: one unit of flux at one unit of drive.
            egui::pos2(centre.x + side * half_w, centre.y - side * half_h),
            chrome::PAD - 2.0,
            tool::fade(face.hot(), 0.3 + 0.7 * drive),
            drive > 0.02,
        );
    }
    tool::halo(&mut shapes, lay.scope, face.lit(p::DRIVE), face.focus());

    // THE SCALE: the one knob, as the run the loop has been opened to.
    tool::slider(
        &mut shapes,
        lay.drive.shrink2(egui::vec2(4.0, 4.0)),
        face.place(p::DRIVE),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::DRIVE)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.drive, face.lit(p::DRIVE), face.focus());

    face.painter.extend(shapes);

    // The mark is squeezed by the heat the core is running at.
    face.mark_signed(&lay, nav_cursor::Signature::Squeeze(heat));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Iron), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Iron).shrink(3.0);
        (piece, lay(glass, strip::bay_rect(piece, SectionKind::Iron)))
    }

    #[test]
    fn every_iron_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Iron.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(
                piece.expand(strip::TONGUE).contains_rect(rect),
                "{} left the piece",
                def.name
            );
        }
        assert_eq!(lay.controls().len(), 1, "one knob, one instrument");
        let _ = SectionParams::of(SectionKind::Iron);
    }

    /// The loop saturates: a hard drive flattens its shoulders while a
    /// light one is nearly the ideal diagonal.
    #[test]
    fn the_loop_flattens_as_the_core_is_driven() {
        let curve = |drive: f32, x: f32| {
            let knee = 0.6 + 3.4 * drive;
            (x * knee).tanh() / knee.tanh()
        };
        // Both ends are pinned, whatever the drive.
        for drive in [0.0f32, 0.5, 1.0] {
            assert!((curve(drive, 1.0) - 1.0).abs() < 1e-5);
            assert!((curve(drive, -1.0) + 1.0).abs() < 1e-5);
        }
        // Halfway along, a driven core is already further up its curve
        // than a lightly driven one: that is saturation.
        assert!(curve(1.0, 0.5) > curve(0.0, 0.5) + 0.05);
        assert!(curve(1.0, 0.25) > curve(0.0, 0.25));
    }
}
