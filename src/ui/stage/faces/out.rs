//! OUT's face: the goniometer, and the two taps under it.
//!
//! The channel's exit, so the card is what a broadcast desk puts at its
//! exit: a phase scope with the stereo field drawn on it. WIDTH opens
//! and closes the jaws the field is squeezed between, and BASS MONO is
//! a clamp that pinches the bottom of it — the two things that can be
//! done to a stereo image, drawn as the two things being done to it.
//!
//! The two sends are the taps at the foot, each in the colour its own
//! return wears everywhere else on the desk: the same orange and blue
//! the band's loom and the mixer's cables use, so a send followed by
//! eye in one view is the same send in the other.

use super::*;
use crate::console::SectionParams;
use crate::params::console::out as p;
use crate::ui::nav_cursor;

/// The two returns' inks, the same the loom and the mixer use.
const TAP_INK: [egui::Color32; 2] = [
    egui::Color32::from_rgb(230, 170, 96),
    egui::Color32::from_rgb(130, 176, 255),
];

struct Lay {
    /// The scope's own field.
    scope: egui::Rect,
    width: egui::Rect,
    bass_mono: egui::Rect,
    send_tape: egui::Rect,
    send_shadow: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::WIDTH, self.width),
            (p::BASS_MONO, self.bass_mono),
            (p::SEND_TAPE, self.send_tape),
            (p::SEND_SHADOW, self.send_shadow),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let scope = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.right(), inner.top() + 0.52 * h),
    );
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 13.0),
        )
    };
    let first = scope.bottom() + 8.0;
    Lay {
        // WIDTH is the jaws the field is squeezed between: the scope.
        width: scope,
        scope,
        // The bass clamp takes the bay when the chassis cuts one.
        bass_mono: bay.unwrap_or_else(|| row(first, 0.0, 1.0)),
        send_tape: row(first + 17.0, 0.0, 0.46),
        send_shadow: plinth.unwrap_or_else(|| row(first + 17.0, 0.54, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let mut shapes = Vec::new();
    let width = face.anim("width", face.place(p::WIDTH), 0.10);
    let level = tool::norm(face.level_db(), -48.0, 0.0);

    // THE SCOPE: mono up the middle, the two sides on the diagonals.
    // The house ruling of a goniometer, so what is drawn on it reads
    // the way an engineer expects.
    tool::well(&mut shapes, lay.scope, face.ground(), edge);
    let field = lay.scope.shrink(4.0);
    let centre = field.center();
    let reach = field.width().min(field.height()) * 0.46;
    for degrees in [45.0f32, 135.0] {
        circuit::trace(
            &mut shapes,
            &[
                tool::on_arc(centre, reach, degrees),
                tool::on_arc(centre, reach, degrees + 180.0),
            ],
            Weight::Hair,
            tool::fade(edge, 0.45),
        );
    }
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(centre.x, centre.y - reach),
            egui::pos2(centre.x, centre.y + reach),
        ],
        Weight::Hair,
        tool::fade(edge, 0.8),
    );

    // THE FIGURE: the field the width leaves. At unity it is a circle;
    // narrowed it collapses toward the mono line; widened it stretches
    // across, which is what widening does.
    let spread = (width * 2.0).clamp(0.0, 2.0);
    let blob: Vec<egui::Pos2> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0 * core::f32::consts::TAU;
            egui::pos2(
                centre.x + t.cos() * reach * 0.72 * spread * (0.25 + 0.75 * level),
                centre.y + t.sin() * reach * 0.72 * (0.25 + 0.75 * level),
            )
        })
        .collect();
    circuit::trace(&mut shapes, &blob, Weight::Heavy, face.live());
    // The jaws themselves, standing where the width has put them.
    for side in [-1.0f32, 1.0] {
        let x = centre.x + side * reach * 0.78 * spread.max(0.06);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, centre.y - reach * 0.5),
                egui::pos2(x, centre.y + reach * 0.5),
            ],
            Weight::Heavy,
            tool::mix_ink(ink, face.focus(), face.lit(p::WIDTH)),
        );
    }
    tool::halo(&mut shapes, lay.scope, face.lit(p::WIDTH), face.focus());

    // BASS MONO: a clamp that pinches the bottom of the field. Its jaws
    // close as the crossover climbs, and at nothing they are open, so
    // an untouched bottom end is visibly untouched.
    let clamp = lay.bass_mono;
    let bite = tool::norm(face.value(p::BASS_MONO), 0.0, 300.0);
    tool::iris(
        &mut shapes,
        clamp.shrink(2.0),
        1.0 - bite,
        tool::mix_ink(tool::fade(edge, 1.1), face.focus(), face.lit(p::BASS_MONO)),
        edge,
    );
    tool::halo(&mut shapes, clamp, face.lit(p::BASS_MONO), face.focus());

    // THE TWO TAPS: each in its return's own colour, with a pad that
    // fills as the send opens and a lead leaving the card the way the
    // loom leaves it. A closed send is plainly carrying nothing.
    for (index, (rect, param)) in [
        (lay.send_tape, p::SEND_TAPE),
        (lay.send_shadow, p::SEND_SHADOW),
    ]
    .into_iter()
    .enumerate()
    {
        let hue = TAP_INK[index];
        let open = face.place(param);
        tool::slider(
            &mut shapes,
            rect.shrink2(egui::vec2(8.0, 4.0)),
            open,
            9,
            tool::mix_ink(hue, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        circuit::pad(
            &mut shapes,
            egui::pos2(rect.left() + 4.0, rect.center().y),
            circuit::PAD,
            tool::fade(hue, 0.3 + 0.7 * open),
            open > 0.005,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    face.painter.extend(shapes);

    // The mark carries what is leaving the channel.
    face.mark_signed(&lay, nav_cursor::Signature::Level(level));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Out), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Out).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Out),
                strip::plinth_rect(piece, SectionKind::Out),
            ),
        )
    }

    #[test]
    fn every_out_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Out.table() {
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
        let _ = SectionParams::of(SectionKind::Out);
    }

    /// The two sends wear the same two colours the band's loom and the
    /// mixer's cables use, so one send is one colour everywhere.
    #[test]
    fn the_taps_wear_the_returns_own_colours() {
        assert_eq!(TAP_INK[0], egui::Color32::from_rgb(230, 170, 96));
        assert_eq!(TAP_INK[1], egui::Color32::from_rgb(130, 176, 255));
        assert_ne!(TAP_INK[0], TAP_INK[1]);
        let (_, lay) = laid();
        assert!(!lay.send_tape.intersects(lay.send_shadow));
    }

    /// Narrowing collapses the field toward mono and widening stretches
    /// it, and the jaws never leave the scope.
    #[test]
    fn the_jaws_open_and_close_with_the_width() {
        let (_, lay) = laid();
        let field = lay.scope.shrink(4.0);
        let reach = field.width().min(field.height()) * 0.46;
        let jaw = |width: f32| reach * 0.78 * (width * 2.0).clamp(0.06, 2.0);
        assert!(jaw(1.0) > jaw(0.0) + 5.0, "narrowing did nothing");
        assert!(jaw(1.0) > jaw(0.5), "the jaws did not open");
        assert!(field.center().x + jaw(1.0) <= lay.scope.right() + 0.01);
    }
}
