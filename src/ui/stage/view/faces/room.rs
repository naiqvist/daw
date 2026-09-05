//! ROOM's face: the chamber, drawn in section.
//!
//! A reverb is a shape and what happens inside it, so the card is the
//! shape. The chamber's walls stand where SIZE puts them; the felt on
//! them is DAMP; the gap between the source and the first wall is the
//! PREDELAY, which is exactly the thing a predelay is.
//!
//! Three things are measured rather than set: what strikes the chamber
//! after the pre-delay, how much is still ringing in it, and how bright
//! that ringing is. So the source flashes when it is struck, the room
//! glows while it rings, and the glow cools as the felt takes the top
//! off it.

use super::*;
use crate::console::SectionParams;
use crate::params::console::room as p;
use crate::ui::nav_cursor;

/// How many felt pads a wall carries at full damping.
const PADS: usize = 7;

struct Lay {
    /// The chamber, and the source standing outside it.
    chamber: egui::Rect,
    algo: egui::Rect,
    predelay: egui::Rect,
    size: egui::Rect,
    damp: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::ALGO, self.algo),
            (p::PREDELAY, self.predelay),
            (p::SIZE, self.size),
            (p::DAMP, self.damp),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let chamber = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 0.22 * w, inner.top() + 0.06 * h),
        egui::pos2(inner.right(), inner.top() + 0.56 * h),
    );
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 13.0),
        )
    };
    let first = chamber.bottom() + 8.0;
    Lay {
        // SIZE is the chamber, and DAMP is its walls, so those two ARE
        // the drawing rather than gauges beside it.
        size: chamber,
        chamber,
        algo: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 18.0, inner.top()),
                egui::pos2(inner.right(), inner.top() + 22.0),
            )
        }),
        // The pre-delay is the run from the source to the near wall.
        predelay: egui::Rect::from_min_max(
            egui::pos2(inner.left(), chamber.center().y - 8.0),
            egui::pos2(chamber.left(), chamber.center().y + 8.0),
        ),
        damp: row(first, 0.0, 0.46),
        mix: plinth.unwrap_or_else(|| row(first, 0.54, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let hall = face.value(p::ALGO) >= 0.5;
    let size = face.anim("size", face.place(p::SIZE), 0.14);
    let damp = face.place(p::DAMP);
    let strike = face.anim("strike", said.bands[0].clamp(0.0, 1.0), 0.06);
    let tail = face.anim("tail", said.bands[1].clamp(0.0, 1.0), 0.16);
    let bright = face.anim("bright", said.bands[2].clamp(0.0, 1.0), 0.14);

    // THE CHAMBER: its walls stand where the size puts them. A hall is
    // drawn tall and a room wide, because that is the difference.
    let full = lay.chamber;
    let box_w = full.width() * (0.30 + 0.70 * size) * if hall { 0.88 } else { 1.0 };
    let box_h = full.height() * (0.34 + 0.66 * size) * if hall { 1.0 } else { 0.82 };
    let walls = egui::Rect::from_min_size(
        egui::pos2(full.left(), full.center().y - box_h * 0.5),
        egui::vec2(box_w, box_h),
    );
    // The ringing itself: the room glows with what is still in it, and
    // the glow cools as the felt takes the top off.
    if walls.is_positive() {
        shapes.push(egui::Shape::rect_filled(
            walls,
            0.0,
            tool::fade(
                tool::mix_ink(tool::LO_INK, tool::HI_INK, bright),
                0.06 + 0.22 * tail,
            ),
        ));
    }
    circuit::trace(
        &mut shapes,
        &[
            walls.left_top(),
            walls.right_top(),
            walls.right_bottom(),
            walls.left_bottom(),
            walls.left_top(),
        ],
        Weight::Heavy,
        ink,
    );
    tool::halo(&mut shapes, lay.size, face.lit(p::SIZE), face.focus());

    // THE FELT: pads on the far wall and the ceiling, as many as the
    // damping asks for. A dead room is a room lined with them.
    let pads = (damp * PADS as f32).round() as usize;
    for i in 0..pads {
        let t = (i as f32 + 0.5) / PADS as f32;
        let y = egui::lerp(walls.y_range(), t);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(walls.right() - 5.0, y),
                egui::pos2(walls.right() - 1.0, y),
            ],
            Weight::Heavy,
            tool::mix_ink(tool::fade(edge, 1.2), face.focus(), face.lit(p::DAMP)),
        );
        let x = egui::lerp(walls.x_range(), t);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, walls.top() + 1.0),
                egui::pos2(x, walls.top() + 5.0),
            ],
            Weight::Heavy,
            tool::mix_ink(tool::fade(edge, 1.2), face.focus(), face.lit(p::DAMP)),
        );
    }
    tool::halo(&mut shapes, lay.damp, face.lit(p::DAMP), face.focus());

    // THE SOURCE and THE PRE-DELAY: the run from the source to the near
    // wall is the delay before the room hears anything, and the source
    // flashes with what actually struck it.
    let run = lay.predelay;
    let gap = tool::norm(face.value(p::PREDELAY), 0.0, 200.0);
    let source = egui::pos2(
        egui::lerp(run.x_range(), 1.0 - gap.clamp(0.05, 1.0)),
        run.center().y,
    );
    circuit::trace(
        &mut shapes,
        &[source, egui::pos2(walls.left(), run.center().y)],
        Weight::Hair,
        tool::mix_ink(tool::fade(edge, 0.9), face.live(), strike),
    );
    circuit::octagon(
        &mut shapes,
        egui::Rect::from_center_size(source, egui::vec2(9.0 + 4.0 * strike, 9.0 + 4.0 * strike)),
        2.0,
        Some(tool::fade(face.live(), 0.35 + 0.65 * strike)),
        Some((Weight::Hair, ink)),
    );
    tool::halo(&mut shapes, run, face.lit(p::PREDELAY), face.focus());

    // ALGO: two detents in the bay — a room, and a hall.
    let slot = lay.algo;
    for (i, here) in [(0usize, !hall), (1, hall)] {
        let y = egui::lerp((slot.top() + 4.0)..=(slot.bottom() - 4.0), i as f32);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(slot.left() + 2.0, y),
                egui::pos2(slot.right() - if here { 1.0 } else { 4.0 }, y),
            ],
            if here { Weight::Heavy } else { Weight::Hair },
            if here { ink } else { tool::fade(edge, 0.7) },
        );
    }
    tool::halo(&mut shapes, slot, face.lit(p::ALGO), face.focus());

    // MIX: how much of the chamber is heard against the dry.
    tool::slider(
        &mut shapes,
        lay.mix.shrink2(egui::vec2(4.0, 4.0)),
        face.place(p::MIX),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::MIX)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.mix, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // The mark carries the tail: it stays lit while the room rings.
    face.mark_signed(&lay, nav_cursor::Signature::Level(tail));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Room), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Room).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Room),
                strip::plinth_rect(piece, SectionKind::Room),
            ),
        )
    }

    #[test]
    fn every_room_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Room.table() {
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
        let _ = SectionParams::of(SectionKind::Room);
    }

    /// The source stands OUTSIDE the chamber, and the run between them
    /// is the pre-delay: more delay puts the source further out.
    #[test]
    fn the_predelay_is_the_run_from_the_source_to_the_wall() {
        let (_, lay) = laid();
        assert!(lay.predelay.right() <= lay.chamber.left() + 0.01);
        let at = |ms: f32| {
            egui::lerp(
                lay.predelay.x_range(),
                1.0 - tool::norm(ms, 0.0, 200.0).clamp(0.05, 1.0),
            )
        };
        assert!(at(200.0) < at(0.0) - 10.0, "the source did not step back");
        assert!(at(200.0) >= lay.predelay.left() - 0.01);
        assert!(at(0.0) <= lay.predelay.right() + 0.01);
    }

    /// The chamber grows with the size and never leaves the space it
    /// was given, and a hall is a different shape from a room.
    #[test]
    fn a_hall_and_a_room_are_different_shapes() {
        let (_, lay) = laid();
        let box_of = |size: f32, hall: bool| {
            (
                lay.chamber.width() * (0.30 + 0.70 * size) * if hall { 0.88 } else { 1.0 },
                lay.chamber.height() * (0.34 + 0.66 * size) * if hall { 1.0 } else { 0.82 },
            )
        };
        let (rw, rh) = box_of(1.0, false);
        let (hw, hh) = box_of(1.0, true);
        assert!(rw > hw, "the room is not the wider of the two");
        assert!(hh > rh, "the hall is not the taller of the two");
        assert!(box_of(1.0, false).0 > box_of(0.0, false).0 + 20.0);
        assert!(rw <= lay.chamber.width() + 0.01);
        assert!(hh <= lay.chamber.height() + 0.01);
    }
}
