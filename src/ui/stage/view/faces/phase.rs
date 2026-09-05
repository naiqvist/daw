//! PHASE's face: two rakes biting a swept spectrum.
//!
//! A phaser is a comb whose teeth walk up and down the spectrum, so the
//! card is the comb and the walking is the section's own sweep. There
//! are two rakes because the section runs two sides, and the second one
//! stands the OFFSET ahead of the first — which is the whole of what
//! that control does and the only honest way to show it.
//!
//! Nothing on this card runs on a clock. Both rakes stand where the
//! section's LFOs stood at the block's last sample; a flattened section
//! reports zero and the rakes come to rest in the middle rather than
//! jamming at whatever angle they stopped on.

use super::*;
use crate::console::SectionParams;
use crate::params::console::phase as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The stage counts the switch steps through: two per position.
const STAGES: [usize; 6] = [2, 4, 6, 8, 10, 12];

struct Lay {
    /// The spectrum the rakes bite.
    field: egui::Rect,
    stages: egui::Rect,
    rate: egui::Rect,
    depth: egui::Rect,
    feedback: egui::Rect,
    offset: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::STAGES, self.stages),
            (p::RATE, self.rate),
            (p::DEPTH, self.depth),
            (p::FEEDBACK, self.feedback),
            (p::OFFSET, self.offset),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let field =
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + 0.58 * h));
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 14.0),
        )
    };
    let first = field.bottom() + 8.0;
    Lay {
        // DEPTH is how far up the field the rakes may travel: the field.
        depth: field,
        field,
        stages: row(first, 0.0, 0.46),
        rate: row(first, 0.54, 1.0),
        feedback: row(first + 19.0, 0.0, 0.46),
        offset: bay
            .or(plinth)
            .unwrap_or_else(|| row(first + 19.0, 0.54, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let field = lay.field;
    let mut shapes = Vec::new();

    let step = face.value(p::STAGES).round().clamp(0.0, 5.0) as usize;
    let teeth = STAGES[step];
    let depth = face.anim("depth", face.place(p::DEPTH), 0.10);
    let fb = face.swing(p::FEEDBACK);

    // THE SPECTRUM the rakes bite into, ruled by decade.
    chrome::lattice(&mut shapes, field, 18.0, tool::fade(edge, 0.30));
    for hz in [200.0, 1_000.0, 6_000.0] {
        let x = tool::octave_x(field, hz);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom() - 3.0),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Hair,
            tool::fade(edge, 0.8),
        );
    }

    // THE TWO RAKES. Each stands where its own LFO stood, and its teeth
    // bite as deep as the depth allows and as sharp as the feedback
    // makes them.
    for (side, (hue, from_top)) in [(0usize, (ink, true)), (1, (face.live(), false))] {
        let sweep = face
            .anim(
                if side == 0 { "rake-l" } else { "rake-r" },
                said.bands[side].clamp(-1.0, 1.0),
                0.03,
            )
            .clamp(-1.0, 1.0);
        // The comb walks between the section's own corners.
        let low = crate::params::console::phase::LOW_HZ;
        let high = crate::params::console::phase::HIGH_HZ;
        let centre = low * (high / low).powf((sweep * 0.5 + 0.5) * depth);
        let bite = field.height() * (0.18 + 0.52 * (0.35 + 0.65 * fb.abs()));
        let spine = if from_top {
            field.top() + 2.0
        } else {
            field.bottom() - 2.0
        };
        let dir = if from_top { 1.0f32 } else { -1.0 };
        // The rake's back, and one tooth per all-pass stage, spread an
        // octave either side of where the comb stands.
        let span = 1.4f32;
        let left = tool::octave_x(field, centre / span.exp2());
        let right = tool::octave_x(field, centre * span.exp2());
        chrome::trace(
            &mut shapes,
            &[egui::pos2(left, spine), egui::pos2(right, spine)],
            Weight::Heavy,
            hue,
        );
        for i in 0..teeth {
            let t = if teeth > 1 {
                i as f32 / (teeth - 1) as f32
            } else {
                0.5
            };
            let x = egui::lerp(left..=right, t);
            // The teeth are longest at the comb's centre, which is
            // where a phaser's notches actually are deepest.
            let reach = bite * (0.35 + 0.65 * (t * core::f32::consts::PI).sin());
            chrome::trace(
                &mut shapes,
                &[egui::pos2(x, spine), egui::pos2(x, spine + dir * reach)],
                Weight::Hair,
                tool::fade(hue, 0.55 + 0.45 * fb.abs()),
            );
        }
        chrome::pad(
            &mut shapes,
            egui::pos2(left, spine),
            chrome::PAD - 2.0,
            hue,
            true,
        );
    }
    tool::halo(&mut shapes, lay.depth, face.lit(p::DEPTH), face.focus());

    // STAGES: one detent per pair of all-passes, so how many there
    // could be is as readable as how many there are.
    let ring = lay.stages;
    tool::rota(
        &mut shapes,
        egui::pos2(ring.left() + 12.0, ring.center().y + 2.0),
        9.0,
        STAGES.len(),
        step as f32,
        tool::mix_ink(ink, face.focus(), face.lit(p::STAGES)),
        tool::fade(edge, 0.7),
    );
    tool::cells(
        &mut shapes,
        egui::pos2(ring.left() + 30.0, ring.center().y),
        egui::vec2(6.0, 0.0),
        STAGES[STAGES.len() - 1] / 2,
        teeth / 2,
        4.0,
        ink,
        tool::fade(edge, 0.45),
    );
    tool::halo(&mut shapes, ring, face.lit(p::STAGES), face.focus());

    // RATE and FEEDBACK: a rail and a bipolar rail. The rate's real
    // reading is the rakes moving; this only says where it is set.
    tool::slider(
        &mut shapes,
        lay.rate.shrink2(egui::vec2(4.0, 4.0)),
        face.place(p::RATE),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::RATE)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.rate, face.lit(p::RATE), face.focus());
    tool::swing_rail(
        &mut shapes,
        lay.feedback.shrink2(egui::vec2(4.0, 4.0)),
        fb,
        11,
        tool::mix_ink(ink, face.focus(), face.lit(p::FEEDBACK)),
        tool::fade(edge, 0.6),
    );
    tool::halo(
        &mut shapes,
        lay.feedback,
        face.lit(p::FEEDBACK),
        face.focus(),
    );

    // OFFSET: the two sides' cycles, drawn as two hands on one dial.
    // Nothing else on the desk says what a degree of stereo offset is
    // as plainly as two hands standing apart.
    let dial = lay.offset;
    let centre = egui::pos2(dial.center().x, dial.center().y);
    let radius = (dial.width().min(dial.height()) * 0.42).max(6.0);
    chrome::trace(
        &mut shapes,
        &tool::arc(centre, radius, 0.0, 360.0, 28),
        Weight::Hair,
        tool::fade(edge, 0.7),
    );
    let turn = face.value(p::OFFSET);
    chrome::trace(
        &mut shapes,
        &[centre, tool::on_arc(centre, radius, 90.0)],
        Weight::Hair,
        ink,
    );
    chrome::trace(
        &mut shapes,
        &[centre, tool::on_arc(centre, radius, 90.0 - turn)],
        Weight::Heavy,
        tool::mix_ink(face.live(), face.focus(), face.lit(p::OFFSET)),
    );
    chrome::pad(&mut shapes, centre, chrome::PAD - 2.0, ink, true);
    tool::halo(&mut shapes, dial, face.lit(p::OFFSET), face.focus());

    face.painter.extend(shapes);

    face.mark_signed(
        &lay,
        nav_cursor::Signature::Sweep(said.bands[0].clamp(-1.0, 1.0)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Phase), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Phase).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Phase),
                strip::plinth_rect(piece, SectionKind::Phase),
            ),
        )
    }

    #[test]
    fn every_phase_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Phase.table() {
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
        assert_eq!(lay.controls().len(), 5);
        let _ = SectionParams::of(SectionKind::Phase);
    }

    /// The rakes bite from opposite edges, so the two sides can never
    /// be mistaken for one, and the comb walks with the sweep.
    #[test]
    fn the_comb_walks_between_the_sections_own_corners() {
        let (_, lay) = laid();
        let low = crate::params::console::phase::LOW_HZ;
        let high = crate::params::console::phase::HIGH_HZ;
        let at = |sweep: f32, depth: f32| {
            tool::octave_x(
                lay.field,
                low * (high / low).powf((sweep * 0.5 + 0.5) * depth),
            )
        };
        // Flat: the comb stands still at the bottom corner.
        assert!((at(-1.0, 0.0) - at(1.0, 0.0)).abs() < 0.01);
        // Deep: it travels, and it stays inside the field.
        assert!(at(1.0, 1.0) > at(-1.0, 1.0) + 20.0);
        for sweep in [-1.0f32, 0.0, 1.0] {
            let x = at(sweep, 1.0);
            assert!(x >= lay.field.left() - 0.01 && x <= lay.field.right() + 0.01);
        }
    }
}
