//! CUT's face: a light gate, two knife-edge shutters and the slit
//! between them.
//!
//! After a monochromator's entrance slit. The spectrum runs across the
//! glass in octaves; the high-pass blade slides in from the left and the
//! low-pass blade from the right, and what is left between them is what
//! the section passes — so closing a filter is watching a shutter close,
//! not watching a number fall.
//!
//! Each blade carries a RESONANCE spike at its own edge, and the spike's
//! height is not the knob: it is the loop's measured ring, so a filter
//! that is actually ringing is the one that is lit. CRUNCH is the heat
//! in the slit itself.

use super::*;
use crate::console::SectionParams;
use crate::params::console::cut as p;
use crate::ui::nav_cursor;

/// How tall a resonance spike stands at full ring.
const SPIKE: f32 = 0.42;

struct Lay {
    field: egui::Rect,
    hp: egui::Rect,
    hp_res: egui::Rect,
    lp: egui::Rect,
    lp_res: egui::Rect,
    crunch: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::HP_HZ, self.hp),
            (p::HP_RES, self.hp_res),
            (p::LP_HZ, self.lp),
            (p::LP_RES, self.lp_res),
            (p::CRUNCH, self.crunch),
        ]
    }
}

/// Where every instrument stands, from the glass and the settings
/// alone — so a test can lay the card out without a painter.
fn lay(glass: egui::Rect, params: &SectionParams) -> Lay {
    let value = |id: u32| params.value(id);
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let crunch_h = 13.0;
    let field = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.right(), inner.bottom() - crunch_h - 5.0),
    );
    let hp_x = tool::octave_x(field, value(p::HP_HZ));
    let lp_x = tool::octave_x(field, value(p::LP_HZ));
    // A blade is grabbed by its own edge; its resonance by the spike
    // standing on that edge, which is the top third of the same column.
    let blade = |x: f32| {
        egui::Rect::from_min_max(
            egui::pos2(x - 7.0, field.center().y),
            egui::pos2(x + 7.0, field.bottom()),
        )
    };
    let spike = |x: f32| {
        egui::Rect::from_min_max(
            egui::pos2(x - 7.0, field.top()),
            egui::pos2(x + 7.0, field.center().y),
        )
    };
    Lay {
        field,
        hp: blade(hp_x),
        hp_res: spike(hp_x),
        lp: blade(lp_x),
        lp_res: spike(lp_x),
        crunch: egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - crunch_h),
            inner.max,
        ),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, &face.params);
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let field = lay.field;
    let mut shapes = Vec::new();

    // The ground: the spectrum, ruled by decade so the axis is a
    // spectrum and not a strip.
    circuit::lattice(&mut shapes, field, 18.0, tool::fade(edge, 0.35));
    for hz in [100.0, 1_000.0, 10_000.0] {
        let x = tool::octave_x(field, hz);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom() - 3.0),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Hair,
            tool::fade(edge, 0.8),
        );
    }

    let hp_x = face.anim("hp", tool::octave_x(field, face.value(p::HP_HZ)), 0.10);
    let lp_x = face.anim("lp", tool::octave_x(field, face.value(p::LP_HZ)), 0.10);

    // THE SLIT: what the section passes, filled in the live ink. The
    // eye reads the gap, which is the thing the section is.
    let slit = egui::Rect::from_min_max(
        egui::pos2(hp_x.min(lp_x), field.top()),
        egui::pos2(hp_x.max(lp_x), field.bottom()),
    );
    if slit.is_positive() {
        shapes.push(egui::Shape::rect_filled(
            slit,
            0.0,
            tool::fade(face.live(), 0.14),
        ));
    }

    // THE BLADES: each one a solid shutter closing in from its own
    // side, with a bright knife edge.
    for (x, from_left) in [(hp_x, true), (lp_x, false)] {
        let shutter = if from_left {
            egui::Rect::from_min_max(field.min, egui::pos2(x, field.bottom()))
        } else {
            egui::Rect::from_min_max(egui::pos2(x, field.top()), field.max)
        };
        if shutter.is_positive() {
            shapes.push(egui::Shape::rect_filled(
                shutter,
                0.0,
                tool::fade(edge, 0.30),
            ));
        }
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.top()),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Heavy,
            ink,
        );
    }

    // THE SPIKES: not the resonance KNOB but the loop's measured ring,
    // so the blade that is actually singing is the one that is lit.
    for (index, (x, ring, param)) in [
        (hp_x, said.bands[0], p::HP_RES),
        (lp_x, said.bands[1], p::LP_RES),
    ]
    .into_iter()
    .enumerate()
    {
        let set = face.place(param);
        let ring = face.anim(
            if index == 0 { "hp-ring" } else { "lp-ring" },
            ring.clamp(0.0, 1.0).max(set * 0.25),
            0.09,
        );
        let height = field.height() * SPIKE * ring;
        let peak = egui::pos2(x, field.center().y - height);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x - 5.0, field.center().y),
                peak,
                egui::pos2(x + 5.0, field.center().y),
            ],
            Weight::Hair,
            tool::mix_ink(ink, face.hot(), ring),
        );
        circuit::pad(
            &mut shapes,
            peak,
            circuit::PAD - 2.0,
            tool::mix_ink(ink, face.hot(), ring),
            ring > 0.02,
        );
        tool::halo(&mut shapes, lay.controls()[index * 2 + 1].1, face.lit(param), face.focus());
    }

    // CRUNCH: the heat in the slit, drawn as a bar that fills with what
    // the loop actually cost. The setting sets the knee; the fill is
    // the measurement.
    let heat = face.anim("heat", said.reduction_db.abs().clamp(0.0, 1.0), 0.12);
    tool::well(&mut shapes, lay.crunch, face.ground(), edge);
    let bar = lay.crunch.shrink(2.0);
    let lit = (bar.width() * face.place(p::CRUNCH)).max(0.0);
    if lit > 0.0 {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_size(bar.min, egui::vec2(lit, bar.height())),
            0.0,
            tool::mix_ink(tool::fade(ink, 0.5), face.hot(), heat),
        ));
    }
    tool::halo(&mut shapes, lay.crunch, face.lit(p::CRUNCH), face.focus());

    face.painter.extend(shapes);

    // The mark rides the blade it is standing on, or wears the ring.
    face.mark_signed(
        &lay,
        match face.selected {
            Some(s) if s == p::HP_HZ as usize => {
                nav_cursor::Signature::Sweep(face.place(p::HP_HZ) * 2.0 - 1.0)
            }
            Some(s) if s == p::LP_HZ as usize => {
                nav_cursor::Signature::Sweep(face.place(p::LP_HZ) * 2.0 - 1.0)
            }
            Some(s) if s == p::HP_RES as usize => {
                nav_cursor::Signature::Level(said.bands[0].clamp(0.0, 1.0))
            }
            Some(s) if s == p::LP_RES as usize => {
                nav_cursor::Signature::Level(said.bands[1].clamp(0.0, 1.0))
            }
            Some(s) if s == p::CRUNCH as usize => {
                nav_cursor::Signature::Level(said.reduction_db.abs().clamp(0.0, 1.0))
            }
            _ => nav_cursor::Signature::Plain,
        },
    );
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid(hp: f32, lp: f32) -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Cut), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Cut).shrink(3.0);
        let mut params = SectionParams::of(SectionKind::Cut);
        params.set(p::HP_HZ, hp);
        params.set(p::LP_HZ, lp);
        (piece, lay(glass, &params))
    }

    #[test]
    fn every_cut_parameter_has_one_instrument() {
        let (piece, lay) = laid(20.0, 20_000.0);
        for def in SectionKind::Cut.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(piece.contains_rect(rect), "{} left the piece", def.name);
        }
        assert_eq!(lay.controls().len(), SectionKind::Cut.table().len());
    }

    /// The blades close in from opposite sides, and the slit between
    /// them is what the section passes.
    #[test]
    fn the_two_blades_close_the_slit_from_opposite_sides() {
        let (_, wide) = laid(20.0, 20_000.0);
        let (_, narrow) = laid(900.0, 1_100.0);
        let open = (wide.lp.center().x - wide.hp.center().x).abs();
        let shut = (narrow.lp.center().x - narrow.hp.center().x).abs();
        assert!(open > shut + 60.0, "the slit did not close: {open} to {shut}");
        let (_, raised) = laid(2_000.0, 20_000.0);
        assert!(raised.hp.center().x > wide.hp.center().x);
        assert!((narrow.hp_res.center().x - narrow.hp.center().x).abs() < 0.01);
        assert!((narrow.lp_res.center().x - narrow.lp.center().x).abs() < 0.01);
        assert!(narrow.hp_res.bottom() <= narrow.hp.top() + 0.01);
        assert!(wide.field.width() > 0.0);
    }
}
