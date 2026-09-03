//! GRIT's face: a converter's staircase on its own bit lattice.
//!
//! After the sample-and-hold bench of an early sampler. The lattice is
//! the word length — one rung per bit, and rungs vanish as BITS comes
//! down, so the quantiser's floor rises up the glass where the eye can
//! see it. The staircase is the sample rate: one tread per held sample,
//! and the treads widen as the clock slows.
//!
//! JITTER is the clock's own unsteadiness, drawn as the tread edges
//! standing off their rung — deterministic, from the tread's index, so
//! two frames of a parked section are the same picture. HISS is the
//! speck on the floor, and the POST filter is a lid that comes down
//! over the aliasing the staircase threw upward.

use super::*;
use crate::console::SectionParams;
use crate::params::console::grit as p;
use crate::ui::nav_cursor;

/// The most treads the staircase draws at the fastest clock.
const TREADS_MAX: f32 = 26.0;
/// The most rungs the lattice draws, one per bit.
const RUNGS_MAX: usize = 16;

/// A deterministic wobble for tread `i`: the same every frame, so a
/// parked card is a still picture rather than a fizzing one.
fn wobble(i: usize) -> f32 {
    let h = (i as u32).wrapping_mul(2_654_435_761) >> 8;
    (h & 0xff) as f32 / 255.0 * 2.0 - 1.0
}

struct Lay {
    field: egui::Rect,
    rate: egui::Rect,
    bits: egui::Rect,
    jitter: egui::Rect,
    hiss: egui::Rect,
    post: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::RATE, self.rate),
            (p::BITS, self.bits),
            (p::JITTER, self.jitter),
            (p::HISS, self.hiss),
            (p::POST, self.post),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let foot = 15.0;
    let field = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 12.0, inner.top() + 4.0),
        egui::pos2(inner.right(), inner.top() + 0.62 * h),
    );
    let row = |top: f32, height: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + height),
        )
    };
    let under = field.bottom() + 6.0;
    Lay {
        field,
        // The staircase's own treads are RATE's target: the whole field.
        rate: field,
        // BITS is the lattice down the left margin.
        bits: egui::Rect::from_min_max(
            egui::pos2(inner.left(), field.top()),
            egui::pos2(inner.left() + 10.0, field.bottom()),
        ),
        jitter: row(under, foot, 0.0, 0.46),
        hiss: row(under, foot, 0.52, 1.0),
        // The lid takes the bay; the blend takes the plinth.
        post: bay.unwrap_or_else(|| row(under + foot + 5.0, foot, 0.0, 0.46)),
        mix: plinth.unwrap_or_else(|| row(under + foot + 5.0, foot, 0.52, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let field = lay.field;
    let mut shapes = Vec::new();

    // THE LATTICE: one rung per bit. Rungs vanish as the word shortens,
    // so the quantiser's floor rises where it can be seen.
    let bits = face.anim("bits", face.value(p::BITS), 0.12).clamp(2.0, 16.0);
    let kept = bits.round() as usize;
    for i in 0..RUNGS_MAX {
        let y = egui::lerp(field.bottom()..=field.top(), i as f32 / (RUNGS_MAX - 1) as f32);
        let on = i < kept;
        circuit::trace(
            &mut shapes,
            &[egui::pos2(lay.bits.left(), y), egui::pos2(field.right(), y)],
            Weight::Hair,
            if on {
                tool::fade(edge, 0.55)
            } else {
                tool::fade(edge, 0.14)
            },
        );
        circuit::pad(
            &mut shapes,
            egui::pos2(lay.bits.left() + 4.0, y),
            circuit::PAD - 2.0,
            if on { ink } else { tool::fade(edge, 0.3) },
            on,
        );
    }
    tool::halo(&mut shapes, lay.bits, face.lit(p::BITS), face.focus());

    // THE STAIRCASE: one tread per held sample, quantised onto the very
    // rungs the lattice is showing, so the two facts are one picture.
    let rate = face.anim("rate", face.value(p::RATE), 0.12);
    let treads = (2.0 + (TREADS_MAX - 2.0) * tool::norm(rate, 1_000.0, 48_000.0))
        .round()
        .max(2.0) as usize;
    let jitter = face.place(p::JITTER);
    let step = field.width() / treads as f32;
    let mut stair: Vec<egui::Pos2> = Vec::with_capacity(treads * 2 + 2);
    for i in 0..treads {
        let t = (i as f32 + 0.5) / treads as f32;
        let s = (t * core::f32::consts::TAU).sin() * 0.5 + 0.5;
        let level = (s * (kept.max(2) - 1) as f32).round() / (kept.max(2) - 1) as f32;
        let y = egui::lerp(field.bottom()..=field.top(), level);
        // The clock's unsteadiness moves the tread's EDGE, not its
        // height: that is what jitter is.
        let skew = wobble(i) * jitter * step * 0.35;
        stair.push(egui::pos2(field.left() + step * i as f32 + skew, y));
        stair.push(egui::pos2(field.left() + step * (i as f32 + 1.0) + skew, y));
    }
    circuit::trace(&mut shapes, &stair, Weight::Heavy, face.live());
    tool::halo(&mut shapes, lay.rate, face.lit(p::RATE), face.focus());

    // THE LID: the post filter, a wall coming down over the aliasing
    // the staircase threw upward. Open at the top of its range, so a
    // lid that is doing nothing is plainly a lid that is up.
    let lid = lay.post;
    let shut = 1.0 - tool::norm(face.value(p::POST), 200.0, 20_000.0);
    let drop = lid.height() * shut;
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(lid.min, egui::pos2(lid.right(), lid.top() + drop)),
        0.0,
        tool::fade(edge, 0.9),
    ));
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lid.left(), lid.top() + drop),
            egui::pos2(lid.right(), lid.top() + drop),
        ],
        Weight::Heavy,
        ink,
    );
    tool::halo(&mut shapes, lid, face.lit(p::POST), face.focus());

    // JITTER and HISS: a wobble gauge and a speck field, both drawn as
    // what they do rather than as a number.
    let wob = lay.jitter;
    for i in 0..11 {
        let t = i as f32 / 10.0;
        let x = egui::lerp(wob.x_range(), t);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, wob.center().y - 4.0 - wobble(i) * jitter * 4.0),
                egui::pos2(x, wob.center().y + 4.0 + wobble(i + 7) * jitter * 4.0),
            ],
            Weight::Hair,
            tool::mix_ink(tool::fade(edge, 0.7), ink, jitter),
        );
    }
    tool::halo(&mut shapes, wob, face.lit(p::JITTER), face.focus());

    let speck = lay.hiss;
    let grains = (face.place(p::HISS) * 28.0).round() as usize;
    for i in 0..grains {
        circuit::dot(
            &mut shapes,
            egui::pos2(
                egui::lerp(speck.x_range(), (wobble(i * 3) + 1.0) * 0.5),
                egui::lerp(speck.y_range(), (wobble(i * 3 + 1) + 1.0) * 0.5),
            ),
            tool::fade(ink, 0.5 + 0.5 * face.lit(p::HISS)),
        );
    }
    tool::halo(&mut shapes, speck, face.lit(p::HISS), face.focus());

    // MIX: how much of the converter is heard against what went in.
    tool::slider(
        &mut shapes,
        lay.mix.shrink2(egui::vec2(4.0, 5.0)),
        face.place(p::MIX),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::MIX)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.mix, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // The mark carries what is leaving, so a converter chewing on
    // silence is a mark with nothing in it.
    face.mark_signed(
        &lay,
        nav_cursor::Signature::Level(tool::norm(face.said.level_db, -60.0, 0.0)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Grit), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Grit).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Grit),
                strip::plinth_rect(piece, SectionKind::Grit),
            ),
        )
    }

    #[test]
    fn every_grit_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Grit.table() {
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
        assert_eq!(lay.controls().len(), 6);
        let _ = SectionParams::of(SectionKind::Grit);
    }

    /// The card's speckle and wobble are the SAME every frame: a parked
    /// converter is a still picture, not a fizzing one.
    #[test]
    fn the_grain_is_deterministic_and_spread() {
        for i in 0..40 {
            assert_eq!(wobble(i), wobble(i), "the grain moved on its own");
            assert!((-1.0..=1.0).contains(&wobble(i)), "the grain left its range");
        }
        // It is a spread, not a constant: the values differ and both
        // signs occur.
        let some: Vec<f32> = (0..24).map(wobble).collect();
        assert!(some.iter().any(|v| *v > 0.2));
        assert!(some.iter().any(|v| *v < -0.2));
    }

    /// The lattice and the staircase share the same field, so a tread
    /// lands on a rung rather than near one.
    #[test]
    fn the_staircase_stands_on_the_lattice() {
        let (_, lay) = laid();
        assert_eq!(lay.rate, lay.field);
        assert!((lay.bits.top() - lay.field.top()).abs() < 0.01);
        assert!((lay.bits.bottom() - lay.field.bottom()).abs() < 0.01);
        assert!(lay.bits.right() <= lay.field.left() + 0.01);
    }
}
