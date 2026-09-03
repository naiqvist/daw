//! SMEAR's face: when each octave will arrive.
//!
//! The one card on the desk whose horizontal axis is TIME rather than
//! frequency or level. Frequency climbs the glass, and each rung's bar
//! reaches as far right as that octave is held back — the group delay
//! of the very allpass chain the kernel runs, taken from
//! `console::smear_curve`, so the bars are arithmetic and not an
//! impression.
//!
//! When a real transient arrives, a wavefront crawls across the bars in
//! the order the ear will hear it: the fast octaves first, the ones
//! near the centre last. That is the section, drawn.
//!
//! AMOUNT is a rack of plates in the bay — one plate per allpass, and
//! the stack closes across the field as the chain grows.

use super::*;
use crate::console::SectionParams;
use crate::params::console::smear as p;
use crate::ui::nav_cursor;

/// One bar per rung of the spectrum.
const RUNGS: usize = 22;
/// The most milliseconds the time axis shows.
const SPAN_MS: f32 = 26.0;
/// The plates in the rack: the chain's own maximum.
const PLATES: usize = 32;

struct Lay {
    /// Frequency up, time across.
    field: egui::Rect,
    amount: egui::Rect,
    centre: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![(p::AMOUNT, self.amount), (p::CENTRE, self.centre)]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let field = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 10.0, inner.top() + 4.0),
        egui::pos2(inner.right(), inner.bottom() - 4.0),
    );
    Lay {
        field,
        // The rack lives in the bay the chassis cuts down the right
        // wall: a tall narrow slot is exactly a stack of plates.
        amount: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 14.0, field.top()),
                egui::pos2(inner.right(), field.bottom()),
            )
        }),
        // The carriage runs the field's full width at the centre's own
        // height, so the control IS the line the bars bow around.
        centre: field,
    }
}

/// Where `hz` stands up the field: low at the foot, high at the head.
fn rung_y(field: egui::Rect, hz: f32) -> f32 {
    let at = (hz.max(1.0) / tool::LOW_HZ).log2() / (tool::HIGH_HZ / tool::LOW_HZ).log2();
    egui::lerp(field.bottom()..=field.top(), at.clamp(0.0, 1.0))
}

/// Where `ms` stands across it.
fn time_x(field: egui::Rect, ms: f32) -> f32 {
    egui::lerp(field.x_range(), (ms / SPAN_MS).clamp(0.0, 1.0))
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay());
    let (ink, edge) = (face.ink(), face.edge());
    let field = lay.field;
    let said = face.said;
    let mut shapes = Vec::new();
    let shape = crate::console::smear_curve::Shape::of(&face.params);

    // THE ZERO RAIL: where every transient enters. Everything to the
    // right of it is time the section is adding.
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), field.top()),
            egui::pos2(field.left(), field.bottom()),
        ],
        Weight::Heavy,
        tool::fade(edge, 1.1),
    );

    // THE BARS: one rung per third of an octave, each reaching as far
    // as the chain actually holds that frequency back.
    let mut apex = (0.0f32, field.center().y);
    for i in 0..RUNGS {
        let t = i as f32 / (RUNGS - 1) as f32;
        let hz = tool::LOW_HZ * (tool::HIGH_HZ / tool::LOW_HZ).powf(t);
        let ms = crate::console::smear_curve::group_delay_ms(&shape, 48_000.0, hz);
        let y = egui::lerp(field.bottom()..=field.top(), t);
        let x = time_x(field, ms);
        if ms > apex.0 {
            apex = (ms, y);
        }
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(field.left(), y - 1.5),
                egui::pos2(x.max(field.left() + 1.0), y + 1.5),
            ),
            0.0,
            tool::fade(ink, 0.45 + 0.4 * (ms / SPAN_MS).clamp(0.0, 1.0)),
        ));
    }

    // THE CARRIAGE: the centre, drawn as the line the bow is built
    // around, with the apex mark where the holding is deepest.
    let centre_y = rung_y(field, face.anim("centre", face.value(p::CENTRE), 0.12));
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(field.left() - 4.0, centre_y),
            egui::pos2(field.right(), centre_y),
        ],
        Weight::Heavy,
        tool::mix_ink(face.live(), face.focus(), face.lit(p::CENTRE)),
    );
    shapes.push(egui::Shape::convex_polygon(
        vec![
            egui::pos2(time_x(field, apex.0), apex.1),
            egui::pos2(time_x(field, apex.0) - 6.0, apex.1 - 4.0),
            egui::pos2(time_x(field, apex.0) - 6.0, apex.1 + 4.0),
        ],
        ink,
        egui::Stroke::NONE,
    ));
    tool::halo(&mut shapes, lay.centre, face.lit(p::CENTRE), face.focus());

    // THE WAVEFRONTS: a real transient, crawling across the bars in the
    // order it will be heard. Two of them, because the section reports
    // the newest onset and the one before it.
    for (index, (age, strength)) in [
        (said.bands[0], said.bands[1]),
        (said.bands[2], said.bands[1] * 0.5),
    ]
    .into_iter()
    .enumerate()
    {
        if age <= 0.0 || age >= SPAN_MS * 12.0 {
            continue;
        }
        let x = time_x(field, age.min(SPAN_MS));
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.top()),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Hair,
            tool::fade(
                face.live(),
                (0.20 + 0.6 * strength.clamp(0.0, 1.0)) / (index as f32 + 1.0),
            ),
        );
        circuit::pad(
            &mut shapes,
            egui::pos2(x, centre_y),
            circuit::PAD - 1.0,
            face.live(),
            index == 0,
        );
    }

    // THE RACK: one plate per allpass running, stacked bottom to top in
    // the bay. Adding a stage adds a plate, and the stack closing is
    // the chain lengthening.
    let rack = lay.amount;
    let stages = face.value(p::AMOUNT).round().clamp(0.0, PLATES as f32) as usize;
    let pitch = rack.height() / PLATES as f32;
    for i in 0..PLATES {
        let y = rack.bottom() - (i as f32 + 0.5) * pitch;
        let on = i < stages;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(rack.left() + 1.0, y),
                egui::pos2(rack.right() - 1.0, y),
            ],
            if on { Weight::Heavy } else { Weight::Hair },
            if on {
                tool::mix_ink(ink, face.focus(), face.lit(p::AMOUNT))
            } else {
                tool::fade(edge, 0.30)
            },
        );
    }
    tool::halo(&mut shapes, rack, face.lit(p::AMOUNT), face.focus());

    face.painter.extend(shapes);

    // The mark wears the onset: it fills as a transient crosses.
    face.mark_signed(
        &lay,
        nav_cursor::Signature::Level(if said.bands[0] > 0.0 {
            (1.0 - said.bands[0] / SPAN_MS).clamp(0.0, 1.0) * said.bands[1].clamp(0.0, 1.0)
        } else {
            0.0
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Smear), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Smear).shrink(3.0);
        (piece, lay(glass, strip::bay_rect(piece, SectionKind::Smear)))
    }

    #[test]
    fn every_smear_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Smear.table() {
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
        assert_eq!(lay.controls().len(), 2);
        let _ = SectionParams::of(SectionKind::Smear);
    }

    /// Frequency climbs the glass and time runs across it — the one
    /// card on the desk whose axis is not frequency across.
    #[test]
    fn frequency_climbs_and_time_runs_across() {
        let (_, lay) = laid();
        assert!(rung_y(lay.field, 8_000.0) < rung_y(lay.field, 100.0));
        assert!(time_x(lay.field, 20.0) > time_x(lay.field, 2.0));
        assert!((time_x(lay.field, 0.0) - lay.field.left()).abs() < 0.01);
        assert_eq!(time_x(lay.field, SPAN_MS * 4.0), lay.field.right());
    }

    /// The bars are the kernel's own group delay: nothing at no stages,
    /// deepest at the centre, and multiplying with the chain.
    #[test]
    fn the_bars_are_the_chains_own_holding() {
        use crate::console::smear_curve::{Shape, group_delay_ms};
        let none = Shape {
            stages: 0,
            centre: 1_000.0,
        };
        assert_eq!(group_delay_ms(&none, 48_000.0, 1_000.0), 0.0);
        let some = Shape {
            stages: 8,
            centre: 1_000.0,
        };
        let at_centre = group_delay_ms(&some, 48_000.0, 1_000.0);
        assert!(at_centre > group_delay_ms(&some, 48_000.0, 60.0));
        assert!(at_centre > group_delay_ms(&some, 48_000.0, 14_000.0));
        assert!(at_centre > 0.0 && at_centre < SPAN_MS);
    }
}
