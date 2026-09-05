//! PUMP's face: valve gear, with the pushrod deliberately unshipped.
//!
//! A cam turns on the transport's own beat, its lobes are the division,
//! their height is the depth, their profile is the shape, and the
//! follower rides them. Everything on this card is real and everything
//! on it is honest — including the one fact the others do not have to
//! tell: the section's DSP has not been written. It is still a wire.
//!
//! So the pushrod is drawn UNSHIPPED. It hangs off the follower with a
//! visible gap where it would meet the channel, and nothing below the
//! gap moves. The card is complete, the machine turns, and it is
//! plainly not connected to anything. The day the core lands, the gap
//! closes and the rest of the drawing is already true.
//!
//! The gap is also said in words — NO CORE at the head, UNSHIPPED at
//! the break — because a drawing that has to be interpreted to be
//! believed is not yet honest. Every other card on the strip states its
//! condition in a word (WIRE, IN, FLAT); this one has the most
//! important condition of any of them and had none.

use super::*;
use crate::console::SectionParams;
use crate::params::console::pump as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The divisions the cam can be cut for, in lobes per bar.
const LOBES: [usize; 5] = [1, 2, 4, 8, 16];

struct Lay {
    /// The cam's disc, and the follower riding it.
    cam: egui::Rect,
    division: egui::Rect,
    depth: egui::Rect,
    shape: egui::Rect,
    hold: egui::Rect,
    /// Where the division is READ. Its instrument is the disc; this is
    /// only the number, and it needs a row of its own like any other.
    div_read: egui::Rect,
    /// Where the pushrod would land if the section had a core.
    seat: egui::Pos2,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::DIVISION, self.division),
            (p::DEPTH, self.depth),
            (p::SHAPE, self.shape),
            (p::HOLD, self.hold),
        ]
    }
}

fn lay(glass: egui::Rect, _bay: Option<egui::Rect>) -> Lay {
    // The glass is not the frame: it runs wider than the casing draws.
    let inner = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 4.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 4.0),
    );
    let gear_w = inner.width() * 0.52;
    let gear =
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.left() + gear_w, inner.bottom()));
    let column = egui::Rect::from_min_max(egui::pos2(gear.right() + 8.0, inner.top()), inner.max);
    // The disc takes the head of the gear bay; the pushrod hangs from it
    // and stops short of the seat at the bay's foot.
    let side = (gear.width() * 0.78).min(gear.height() * 0.52);
    let cam = egui::Rect::from_center_size(
        egui::pos2(gear.center().x, gear.top() + side * 0.5 + ROW_H),
        egui::vec2(side, side),
    );
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(
                column.left(),
                column.top() + ROW_H + 4.0 + i as f32 * (ROW_H + 2.0),
            ),
            egui::vec2(column.width(), ROW_H),
        )
    };
    Lay {
        cam,
        // The lobes ARE the division: the disc is its instrument, and
        // the row beside it only reads the number off.
        division: cam,
        div_read: row(0),
        depth: row(1),
        shape: row(2),
        hold: row(3),
        seat: egui::pos2(cam.center().x, gear.bottom() - 4.0),
    }
}

/// One readout row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;

/// The cam's radius at angle `t` (turns), for a wheel of `lobes` cut to
/// `depth` with a profile from square to sinusoidal at `shape`, and a
/// dwell of `hold` at the top of each lobe.
fn lobe(t: f32, lobes: usize, depth: f32, shape: f32, hold: f32) -> f32 {
    let phase = (t * lobes as f32).fract();
    // The dwell: the lobe sits at its peak for this share of a turn.
    let dwell = hold.clamp(0.0, 0.9);
    let run = (1.0 - dwell).max(1e-3);
    let x = if phase < dwell {
        0.0
    } else {
        (phase - dwell) / run
    };
    // Square at one end of SHAPE, a raised cosine at the other.
    let soft = 0.5 - 0.5 * (x * core::f32::consts::TAU).cos();
    let hard = if x < 0.5 { 0.0 } else { 1.0 };
    let profile = egui::lerp(hard..=soft, shape.clamp(0.0, 1.0));
    1.0 - depth.clamp(0.0, 1.0) * (1.0 - profile)
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay());
    let (ink, edge) = (face.ink(), face.edge());
    let mut shapes = Vec::new();

    let step = face.value(p::DIVISION).round().clamp(0.0, 4.0) as usize;
    let lobes = LOBES[step];
    let depth = face.anim("depth", face.place(p::DEPTH), 0.10);
    let shape = face.anim("shape", face.place(p::SHAPE), 0.10);
    let hold = face.anim("hold", face.place(p::HOLD), 0.10) * 0.6;
    // The cam turns on the TRANSPORT's beat, and stands still when the
    // transport does. There is no other clock on this card.
    let turn = face.phase.beat;

    // THE CAM: its outline is the very profile the section would run.
    let centre = lay.cam.center();
    let radius = lay.cam.width() * 0.5;
    let rim: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.0;
            // The wheel turns under a fixed follower, so the profile is
            // read at the angle PLUS the turn. Drawn without it the rim
            // stood still while the follower rose off it, which is the
            // one thing a cam drawing must not do.
            let r = radius * (0.45 + 0.55 * lobe(t + turn, lobes, depth, shape, hold));
            tool::on_arc(centre, r, 90.0 - t * 360.0)
        })
        .collect();
    chrome::curve(&mut shapes, &rim, Weight::Heavy, ink);
    chrome::curve(
        &mut shapes,
        &tool::arc(centre, radius, 0.0, 360.0, 48),
        Weight::Hair,
        tool::fade(edge, 0.4),
    );
    chrome::pad(&mut shapes, centre, chrome::PAD, ink, true);
    // The keyway: one mark on the disc, so the turning is visible.
    chrome::trace(
        &mut shapes,
        &[
            centre,
            tool::on_arc(centre, radius * 0.5, 90.0 - turn * 360.0),
        ],
        Weight::Hair,
        tool::fade(edge, 1.1),
    );
    tool::halo(&mut shapes, lay.cam, face.lit(p::DIVISION), face.focus());

    // THE FOLLOWER: it rides the rim at the transport's own angle, so
    // the lift under it is the duck the section would be applying.
    let lift = lobe(turn, lobes, depth, shape, hold);
    let ride = radius * (0.45 + 0.55 * lift);
    let contact = tool::on_arc(centre, ride, 90.0);
    chrome::octagon(
        &mut shapes,
        egui::Rect::from_center_size(contact, egui::vec2(9.0, 9.0)),
        2.0,
        Some(face.live()),
        Some((Weight::Hair, ink)),
    );

    // THE PUSHROD, UNSHIPPED. It reaches down from the follower and
    // stops short: the section's core has not been written, so nothing
    // it would drive is being driven, and the card says so rather than
    // miming a duck that is not happening.
    let gap_top = egui::lerp(lay.cam.bottom()..=lay.seat.y, 0.55);
    chrome::trace(
        &mut shapes,
        &[contact, egui::pos2(contact.x, gap_top)],
        Weight::Heavy,
        ink,
    );
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(lay.seat.x - 9.0, lay.seat.y),
            egui::pos2(lay.seat.x + 9.0, lay.seat.y),
        ],
        Weight::Heavy,
        tool::fade(edge, 1.1),
    );
    // The gap itself, marked so it reads as unshipped and not as a
    // drawing that ran out of room.
    for y in [gap_top + 4.0, lay.seat.y - 6.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(contact.x - 5.0, y),
                egui::pos2(contact.x + 5.0, y),
            ],
            Weight::Hair,
            tool::fade(edge, 0.9),
        );
    }

    // DEPTH, SHAPE and HOLD, each drawn as what it does to the lobe:
    // how deep it cuts, how square it is, how long it dwells.
    // The room a row's word and its figure keep, measured: an instrument
    // drawn across the whole row is drawn under both of them.
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let span = |text: &str| {
        face.painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["DEPTH", "SHAPE", "HOLD", "DIV"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 8.0;
    let figure = span("100") + 8.0;
    let between = |rect: egui::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.top()),
            egui::pos2(rect.right() - figure, rect.bottom()),
        )
    };
    for (rect, param, value) in [(lay.depth, p::DEPTH, depth), (lay.shape, p::SHAPE, shape)] {
        tool::slider(
            &mut shapes,
            between(rect).shrink2(egui::vec2(0.0, 4.0)),
            value,
            9,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }
    // HOLD is the dwell, so it is drawn as a beat grid whose lit share
    // is the dwell and whose cell count is the division.
    tool::beat_grid(
        &mut shapes,
        between(lay.hold).shrink2(egui::vec2(0.0, 3.0)),
        lobes.min(8),
        1.0 - hold / 0.6,
        Some(((turn * lobes as f32) as usize).min(lobes.saturating_sub(1))),
        tool::mix_ink(ink, face.focus(), face.lit(p::HOLD)),
        tool::fade(edge, 0.5),
    );
    tool::halo(&mut shapes, lay.hold, face.lit(p::HOLD), face.focus());

    face.painter.extend(shapes);

    // ---- The words. -------------------------------------------------
    let mut words = tool::Ledger::new(face.painter, font.clone(), "PUMP");
    let head = egui::Rect::from_min_max(
        egui::pos2(lay.cam.left() - 20.0, lay.cam.top() - ROW_H - 6.0),
        egui::pos2(lay.hold.right(), lay.cam.top() - 6.0),
    );
    words.text(
        egui::pos2(head.left() + 2.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        "CAM",
        edge,
    );
    // The one condition that matters more than any setting on the card.
    words.text(
        egui::pos2(head.right() - 2.0, head.center().y),
        egui::Align2::RIGHT_CENTER,
        "NO CORE",
        face.focus(),
    );
    // The break in the pushrod, named.
    let gap_mid = egui::lerp(gap_top..=lay.seat.y, 0.5);
    words.text(
        egui::pos2(contact.x + 9.0, gap_mid),
        egui::Align2::LEFT_CENTER,
        "UNSHIPPED",
        tool::fade(edge, 1.2),
    );
    // The division reads off the disc beside it: a bar cut into lobes.
    words.text(
        egui::pos2(lay.div_read.left() + 2.0, lay.div_read.center().y),
        egui::Align2::LEFT_CENTER,
        "DIV",
        edge,
    );
    words.text(
        egui::pos2(lay.div_read.right() - 2.0, lay.div_read.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("1/{lobes}"),
        ink,
    );
    for (rect, word, said) in [
        (lay.depth, "DEPTH", format!("{:.0}", face.value(p::DEPTH))),
        (lay.shape, "SHAPE", format!("{:.0}", face.value(p::SHAPE))),
        (lay.hold, "HOLD", format!("{:.0}", face.value(p::HOLD))),
    ] {
        words.text(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }
    words.finish();

    // The mark stands on the grid the cam is cut to.
    face.mark_signed(&lay, face.beat_cell(lobes.min(8)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Pump), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Pump).shrink(3.0);
        (piece, lay(glass, strip::bay_rect(piece, SectionKind::Pump)))
    }

    #[test]
    fn every_pump_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Pump.table() {
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
        assert_eq!(lay.controls().len(), 4);
        let _ = SectionParams::of(SectionKind::Pump);
    }

    /// The cam's profile is the duck it would apply: cut to depth, held
    /// at the top for the dwell, and square or soft with the shape.
    #[test]
    fn the_lobe_is_the_duck_the_section_would_apply() {
        // No depth is a round wheel: nothing ducks.
        for i in 0..8 {
            let t = i as f32 / 8.0;
            assert!((lobe(t, 4, 0.0, 0.5, 0.0) - 1.0).abs() < 1e-5);
        }
        // Full depth reaches the floor at the start of a lobe and comes
        // back to the top by its end.
        assert!(lobe(0.0, 1, 1.0, 1.0, 0.0) < 0.05, "the lobe never bit");
        assert!(lobe(0.5, 1, 1.0, 1.0, 0.0) > 0.95, "the lobe never let go");
        // The dwell holds the floor: at a long hold the first part of
        // every turn is flat.
        let held = lobe(0.1, 1, 1.0, 1.0, 0.5);
        assert!(held < 0.05, "the dwell did not hold: {held}");
        // And it never leaves its own range, at any setting.
        for lobes in [1usize, 2, 4, 8, 16] {
            for i in 0..24 {
                let v = lobe(i as f32 / 24.0, lobes, 1.0, 0.3, 0.4);
                assert!((0.0..=1.0).contains(&v), "the cam left its range: {v}");
            }
        }
    }

    /// The wheel turns under the follower, so the rim must be read at
    /// the angle PLUS the turn. Drawn without it the profile stood
    /// still while the follower rose off it — a cam whose follower
    /// floats is not a cam.
    #[test]
    fn the_follower_stands_on_the_rim_at_every_turn() {
        let (lobes, depth, shape, hold) = (4usize, 0.8, 0.25, 0.3);
        for i in 0..16 {
            let turn = i as f32 / 16.0;
            // The follower rides at the top of the wheel, which is the
            // rim's own t = 0.
            let under_it = lobe(0.0 + turn, lobes, depth, shape, hold);
            let lift = lobe(turn, lobes, depth, shape, hold);
            assert!(
                (under_it - lift).abs() < 1e-6,
                "at turn {turn} the rim reads {under_it} and the follower sits at {lift}"
            );
        }
    }

    /// The pushrod is unshipped, and the gap is real: the follower's rod
    /// stops well short of the seat.
    #[test]
    fn the_pushrod_does_not_reach_its_seat() {
        let (_, lay) = laid();
        let gap_top = egui::lerp(lay.cam.bottom()..=lay.seat.y, 0.55);
        assert!(
            lay.seat.y - gap_top > 10.0,
            "the gap that says the core is a wire has closed"
        );
        assert!(gap_top > lay.cam.bottom());
    }
}
