//! DRIVE's face: a bench press, and the specimen it is squashing.
//!
//! A saturator is a transfer curve and nothing else, so the curve is
//! the card — taken from the section's own `drive_curve`, which is the
//! very arithmetic the shaper runs, at whichever of its five characters
//! is chosen. The press closes on it: the platen comes down as DRIVE
//! goes up, and the specimen wave under it flattens against the curve
//! it is being pressed through.
//!
//! What is measured rather than set: how much of what leaves is DIRT
//! the section made, and how much of the output's energy is TOP. The
//! swarf on the bench is the first; the see-saw at the foot is the
//! second, and it is the only reason the two tilt controls are legible
//! as a pair.

use super::*;
use crate::console::SectionParams;
use crate::params::console::drive as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The five characters the section can wear.
const CHARACTERS: usize = 5;

struct Lay {
    /// The chord the transfer curve is drawn on.
    chord: egui::Rect,
    character: egui::Rect,
    drive: egui::Rect,
    tilt_pre: egui::Rect,
    tilt_post: egui::Rect,
    mix: egui::Rect,
    out: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::CHARACTER, self.character),
            (p::DRIVE, self.drive),
            (p::TILT_PRE, self.tilt_pre),
            (p::TILT_POST, self.tilt_post),
            (p::MIX, self.mix),
            (p::OUT, self.out),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    // The press stands in the upper two thirds; the tilts are a pair of
    // see-saws under it, and the two exits share the foot.
    let chord = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 0.06 * w, inner.top() + 6.0),
        egui::pos2(inner.left() + 0.62 * w, inner.top() + 0.48 * h),
    );
    let tilt_h = 15.0;
    let tilts_top = chord.bottom() + 10.0;
    Lay {
        chord,
        // DRIVE is the press itself: the whole column the platen rides.
        drive: egui::Rect::from_min_max(
            egui::pos2(chord.right() + 8.0, chord.top()),
            egui::pos2(inner.right() - 2.0, chord.bottom()),
        ),
        // The character lives in the bay the chassis cuts for it.
        character: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 24.0, inner.top() + 2.0),
                egui::pos2(inner.right(), inner.top() + 26.0),
            )
        }),
        tilt_pre: egui::Rect::from_min_max(
            egui::pos2(inner.left(), tilts_top),
            egui::pos2(inner.left() + 0.46 * w, tilts_top + tilt_h),
        ),
        tilt_post: egui::Rect::from_min_max(
            egui::pos2(inner.left() + 0.52 * w, tilts_top),
            egui::pos2(inner.right(), tilts_top + tilt_h),
        ),
        mix: egui::Rect::from_min_max(
            egui::pos2(inner.left(), tilts_top + tilt_h + 6.0),
            egui::pos2(inner.left() + 0.46 * w, tilts_top + tilt_h + 20.0),
        ),
        // OUT takes the plinth when the chassis cuts one.
        out: plinth.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.left() + 0.52 * w, tilts_top + tilt_h + 6.0),
                egui::pos2(inner.right(), tilts_top + tilt_h + 20.0),
            )
        }),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let character = face.value(p::CHARACTER).round().clamp(0.0, 4.0) as u32;
    let drive = face.anim("drive", face.value(p::DRIVE), 0.10);
    let dirt = face.anim("dirt", said.bands[1].clamp(0.0, 1.0), 0.10);
    let top = face.anim("top", said.bands[2].clamp(0.0, 1.0), 0.12);
    let heat = said.reduction_db.abs().clamp(0.0, 1.0);

    // THE CHORD: the section's own transfer curve, with the diagonal it
    // would be if nothing were happening. The gap between them IS the
    // saturation, and it is arithmetic, not an impression of one.
    tool::well(&mut shapes, lay.chord, face.ground(), edge);
    let bench = lay.chord.shrink(3.0);
    chrome::trace(
        &mut shapes,
        &[bench.left_bottom(), bench.right_top()],
        Weight::Hair,
        tool::fade(edge, 0.5),
    );
    let curve: Vec<egui::Pos2> = (0..=32)
        .map(|i| {
            let x = -1.0 + 2.0 * i as f32 / 32.0;
            let y = crate::console::drive_curve::transfer(character, drive, x);
            egui::pos2(
                bench.left() + (x + 1.0) * 0.5 * bench.width(),
                bench.bottom() - (y.clamp(-1.0, 1.0) + 1.0) * 0.5 * bench.height(),
            )
        })
        .collect();
    chrome::trace(
        &mut shapes,
        &curve,
        Weight::Heavy,
        tool::mix_ink(ink, face.hot(), heat),
    );

    // THE PLATEN: it comes down as the press is closed, and the swarf
    // under it is the dirt the section actually made.
    let press = lay.drive;
    let closed = tool::norm(drive, 0.0, 100.0);
    let platen_y = egui::lerp(press.top()..=(press.bottom() - 16.0), closed);
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(press.left(), platen_y),
            egui::pos2(press.right(), platen_y + 5.0),
        ),
        0.0,
        tool::mix_ink(ink, face.hot(), heat),
    ));
    for side in [press.left() + 3.0, press.right() - 3.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(side, press.top()),
                egui::pos2(side, press.bottom()),
            ],
            Weight::Hair,
            tool::fade(edge, 0.8),
        );
    }
    // The specimen: a wave squashed flat between the platen and the bed.
    let bed = press.bottom() - 2.0;
    let room = (bed - platen_y - 6.0).max(2.0);
    let wave: Vec<egui::Pos2> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            let s = (t * core::f32::consts::TAU * 2.0).sin();
            egui::pos2(
                egui::lerp(press.x_range(), t),
                bed - room * 0.5 - room * 0.5 * s,
            )
        })
        .collect();
    chrome::trace(&mut shapes, &wave, Weight::Hair, face.live());
    // The swarf: shavings thrown off the press, as many as there is
    // dirt in what leaves.
    let swarf = (dirt * 9.0).round() as usize;
    for i in 0..swarf {
        let t = (i as f32 + 0.5) / 9.0;
        let x = egui::lerp(press.x_range(), t);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, platen_y - 2.0),
                egui::pos2(x + if i % 2 == 0 { 3.0 } else { -3.0 }, platen_y - 7.0),
            ],
            Weight::Hair,
            tool::fade(face.hot(), 0.4 + 0.6 * dirt),
        );
    }
    tool::halo(&mut shapes, lay.drive, face.lit(p::DRIVE), face.focus());

    // THE CHARACTER: five detents in the bay the chassis cut for it, a
    // tall narrow slot, so the switch lives in the crevice.
    let slot = lay.character;
    for i in 0..CHARACTERS {
        let y = egui::lerp(
            (slot.top() + 4.0)..=(slot.bottom() - 4.0),
            i as f32 / (CHARACTERS - 1) as f32,
        );
        let here = i as u32 == character;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(slot.left() + 2.0, y),
                egui::pos2(slot.right() - if here { 1.0 } else { 4.0 }, y),
            ],
            if here { Weight::Heavy } else { Weight::Hair },
            if here { ink } else { tool::fade(edge, 0.7) },
        );
    }
    tool::halo(&mut shapes, slot, face.lit(p::CHARACTER), face.focus());

    // THE TWO SEE-SAWS: tilt before the curve and after it. The one
    // after carries the measured top share as a hand on its beam, so
    // the pair is legible as cause and effect.
    for (rect, param, measured) in [
        (lay.tilt_pre, p::TILT_PRE, None),
        (lay.tilt_post, p::TILT_POST, Some(top)),
    ] {
        let swing = face.swing(param) * 0.42;
        let (cx, cy) = (rect.center().x, rect.center().y);
        let arm = rect.width() * 0.42;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(cx - arm, cy + arm * swing),
                egui::pos2(cx + arm, cy - arm * swing),
            ],
            Weight::Heavy,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
        );
        shapes.push(egui::Shape::convex_polygon(
            vec![
                egui::pos2(cx, cy + 1.0),
                egui::pos2(cx - 4.0, cy + 6.0),
                egui::pos2(cx + 4.0, cy + 6.0),
            ],
            edge,
            egui::Stroke::NONE,
        ));
        if let Some(share) = measured {
            chrome::pad(
                &mut shapes,
                egui::pos2(egui::lerp((cx - arm)..=(cx + arm), share), cy - 6.0),
                chrome::PAD - 1.0,
                face.live(),
                true,
            );
        }
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    // MIX and OUT: two rails at the foot, one blending the press back
    // with what went in, the other letting what is left leave.
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
    tool::swing_rail(
        &mut shapes,
        lay.out.shrink2(egui::vec2(4.0, 5.0)),
        face.swing(p::OUT),
        11,
        tool::mix_ink(ink, face.focus(), face.lit(p::OUT)),
        tool::fade(edge, 0.6),
    );
    tool::halo(&mut shapes, lay.out, face.lit(p::OUT), face.focus());

    face.painter.extend(shapes);

    // The mark is squeezed by the heat: the press is closing on it too.
    face.mark_signed(&lay, nav_cursor::Signature::Squeeze(heat));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Drive), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Drive).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Drive),
                strip::plinth_rect(piece, SectionKind::Drive),
            ),
        )
    }

    #[test]
    fn every_drive_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Drive.table() {
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
    }

    /// The two controls the chassis cut crevices for live in them: the
    /// character in the bay, the output on the plinth.
    #[test]
    fn the_character_lives_in_the_bay_and_the_output_on_the_plinth() {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Drive), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Drive).shrink(3.0);
        let (_, lay) = laid();
        assert_eq!(
            Some(lay.character),
            strip::bay_rect(piece, SectionKind::Drive)
        );
        assert_eq!(Some(lay.out), strip::plinth_rect(piece, SectionKind::Drive));
        assert!(!glass.contains_rect(lay.character), "the bay is not glass");
        assert!(!glass.contains_rect(lay.out), "the plinth is not glass");
    }

    /// The curve on the glass is the shaper's own arithmetic: at rest it
    /// is the diagonal, and driven it bends away from it.
    #[test]
    fn the_chord_is_the_shapers_own_transfer() {
        use crate::console::drive_curve;
        for character in 0..5 {
            assert_eq!(drive_curve::transfer(character, 0.0, 0.6), 0.6);
            let driven = drive_curve::transfer(character, 80.0, 0.6);
            assert!(
                (driven - 0.6).abs() > 0.01,
                "character {character} did nothing at full drive"
            );
            assert!(driven.abs() <= 1.5, "character {character} ran away");
        }
    }
}
