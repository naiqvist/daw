//! HIT's face: a pendulum hammer, a struck bar, and the sparks.
//!
//! After the Charpy impact tester. The hammer is raised as far as the
//! ATTACK lever is pushed and hangs on the other side of the pivot when
//! it is pulled, so a boost and a cut are the same machine facing two
//! ways. The specimen bar lies across the anvil, and the wedge under it
//! is what the strike leaves behind — SUSTAIN sets how far that wedge
//! reaches.
//!
//! Nothing here swings on a clock. The hammer falls because the section
//! measured a strike, the wedge fills because it measured a tail, and
//! the sparks fly because the brightener actually added something.

use super::*;
use crate::console::SectionParams;
use crate::params::console::hit as p;
use crate::ui::nav_cursor;

/// How far the hammer's arm swings, in degrees, at a full lever.
const RAISE_DEG: f32 = 78.0;

struct Lay {
    /// The pivot the hammer hangs from, and the arm's reach.
    pivot: egui::Pos2,
    reach: f32,
    /// The three instruments' grab rects.
    attack: egui::Rect,
    sustain: egui::Rect,
    bright: egui::Rect,
    /// The anvil the bar lies on, and the wedge under it.
    anvil: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::ATTACK, self.attack),
            (p::SUSTAIN, self.sustain),
            (p::BRIGHT, self.bright),
        ]
    }
}

fn lay(glass: egui::Rect, _params: &SectionParams) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let pivot = egui::pos2(inner.center().x, inner.top() + 0.10 * h);
    let reach = (0.34 * h).min(0.42 * w);
    let anvil = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 0.14 * w, pivot.y + reach + 6.0),
        egui::pos2(inner.right() - 0.14 * w, pivot.y + reach + 12.0),
    );
    // The hammer's whole arc is the ATTACK lever's target; the wedge
    // under the anvil is SUSTAIN's; the rasp above the bar is BRIGHT's.
    Lay {
        pivot,
        reach,
        attack: egui::Rect::from_min_max(
            egui::pos2(inner.left(), pivot.y - 6.0),
            egui::pos2(inner.right(), pivot.y + reach * 0.55),
        ),
        sustain: egui::Rect::from_min_max(
            egui::pos2(inner.left(), anvil.bottom() + 3.0),
            egui::pos2(inner.right(), inner.bottom() - 2.0),
        ),
        bright: egui::Rect::from_min_max(
            egui::pos2(inner.left() + 0.20 * w, anvil.top() - 16.0),
            egui::pos2(inner.right() - 0.20 * w, anvil.top() - 2.0),
        ),
        anvil,
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, &face.params);
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    // What the section measured of the last block: how hard the strike
    // was, how far the tail has run, and what the brightener added.
    let blow = face.anim("blow", said.bands[0].clamp(0.0, 1.0), 0.05);
    let tail = face.anim("tail", said.bands[1].clamp(0.0, 1.0), 0.14);
    let edge_share = face.anim("edge", said.bands[2].clamp(0.0, 1.0), 0.07);

    // THE ANVIL and the specimen bar across it.
    shapes.push(egui::Shape::rect_filled(lay.anvil, 0.0, tool::fade(edge, 0.9)));
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.anvil.left() - 6.0, lay.anvil.top() - 2.0),
            egui::pos2(lay.anvil.right() + 6.0, lay.anvil.top() - 2.0),
        ],
        Weight::Heavy,
        ink,
    );

    // THE HAMMER. The lever raises it; the measured strike drops it.
    // A pulled lever hangs it on the far side of the pivot, which is a
    // machine that lifts the sustain instead of striking it.
    let raise = face.swing(p::ATTACK);
    let rest = 90.0 - RAISE_DEG * raise;
    let angle = face.anim("arm", rest + (90.0 - rest) * blow, 0.05);
    let head = tool::on_arc(lay.pivot, lay.reach, angle - 90.0);
    circuit::trace(
        &mut shapes,
        &tool::arc(lay.pivot, lay.reach, -90.0, angle - 90.0, 12),
        Weight::Hair,
        tool::fade(edge, 0.45),
    );
    circuit::trace(&mut shapes, &[lay.pivot, head], Weight::Heavy, ink);
    circuit::octagon(
        &mut shapes,
        egui::Rect::from_center_size(head, egui::vec2(13.0, 13.0)),
        3.0,
        Some(tool::mix_ink(ink, face.hot(), blow)),
        Some((Weight::Hair, edge)),
    );
    circuit::pad(&mut shapes, lay.pivot, circuit::PAD, ink, true);
    tool::halo(&mut shapes, lay.attack, face.lit(p::ATTACK), face.focus());

    // THE WEDGE: what the strike leaves behind. Its reach is the lever;
    // how full it stands is the measured tail.
    let wedge = lay.sustain;
    let reach = wedge.width() * (0.12 + 0.88 * face.place(p::SUSTAIN));
    let lifted = face.swing(p::SUSTAIN) >= 0.0;
    let tip = egui::pos2(wedge.left() + reach, wedge.bottom() - wedge.height() * tail);
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(wedge.left(), wedge.bottom()),
            tip,
            egui::pos2(wedge.left() + reach, wedge.bottom()),
        ],
        Weight::Hair,
        if lifted { ink } else { tool::fade(edge, 1.2) },
    );
    for i in 0..8 {
        let at = (i as f32 + 0.5) / 8.0;
        let x = wedge.left() + reach * at;
        let y = egui::lerp(wedge.bottom()..=tip.y, 1.0 - at);
        circuit::trace(
            &mut shapes,
            &[egui::pos2(x, wedge.bottom()), egui::pos2(x, y)],
            Weight::Hair,
            tool::fade(if lifted { face.live() } else { edge }, 0.25 + 0.6 * tail),
        );
    }
    tool::halo(&mut shapes, lay.sustain, face.lit(p::SUSTAIN), face.focus());

    // THE RASP: sparks off the strike. There are as many as the lever
    // asks for; they are as bright as the brightener actually added.
    let rasp = lay.bright;
    let teeth = (2.0 + face.place(p::BRIGHT) * 9.0).round() as usize;
    for i in 0..teeth {
        let at = if teeth > 1 {
            i as f32 / (teeth - 1) as f32
        } else {
            0.5
        };
        let x = egui::lerp(rasp.x_range(), at);
        let lean = if i % 2 == 0 { 3.0 } else { -3.0 };
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, rasp.bottom()),
                egui::pos2(x + lean, rasp.bottom() - rasp.height() * (0.35 + 0.65 * edge_share)),
            ],
            Weight::Hair,
            tool::mix_ink(tool::fade(edge, 0.5), face.hot(), edge_share),
        );
    }
    tool::halo(&mut shapes, lay.bright, face.lit(p::BRIGHT), face.focus());

    face.painter.extend(shapes);

    // The mark takes the blow: standing on ATTACK it is squeezed by the
    // strike the section is measuring, and on the other two it carries
    // the quantity that instrument reads.
    face.mark_signed(
        &lay,
        match face.selected {
            Some(s) if s == p::ATTACK as usize => nav_cursor::Signature::Squeeze(blow),
            Some(s) if s == p::SUSTAIN as usize => nav_cursor::Signature::Level(tail),
            Some(s) if s == p::BRIGHT as usize => nav_cursor::Signature::Level(edge_share),
            _ => nav_cursor::Signature::Plain,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Hit), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Hit).shrink(3.0);
        (piece, lay(glass, &SectionParams::of(SectionKind::Hit)))
    }

    #[test]
    fn every_hit_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Hit.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(piece.contains_rect(rect), "{} left the piece", def.name);
        }
        assert_eq!(lay.controls().len(), 3);
    }

    /// The machine is stacked the way a drop test is: the hammer above,
    /// the bar it strikes, the sparks off it, the wedge underneath.
    #[test]
    fn the_hammer_stands_over_the_anvil_and_the_wedge_under_it() {
        let (_, lay) = laid();
        assert!(lay.pivot.y < lay.anvil.top(), "the hammer hangs below the bar");
        assert!(lay.bright.bottom() <= lay.anvil.top() + 0.01);
        assert!(lay.sustain.top() >= lay.anvil.bottom() - 0.01);
        assert!(lay.reach > 20.0, "the arm has no swing");
        // A raised hammer is up and to one side; a dropped one is down.
        let raised = tool::on_arc(lay.pivot, lay.reach, -12.0);
        let dropped = tool::on_arc(lay.pivot, lay.reach, -90.0);
        assert!(dropped.y > raised.y, "the hammer did not fall");
        assert!((dropped.x - lay.pivot.x).abs() < 0.01, "it fell off centre");
    }
}
