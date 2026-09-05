//! SHADOW's face: a steel plate sprung in its frame.
//!
//! The second return, and the counterpart to TAPE: where that card is a
//! machine with a moving part, this one is a single sheet under tension
//! and the felt pressed against it. After the EMT plate — two square
//! metres of steel hung on springs in a frame, a driver at one corner
//! and a pickup at the other, and a damper you wind in to shorten the
//! decay.
//!
//! SIZE is how much plate is hung; DAMP is how far the felt is wound
//! in; PREDELAY is the throw from the driver to the sheet. The plate
//! itself rings with what the return is actually returning, so a plate
//! nothing is being sent to hangs still.

use super::*;
use crate::console::SectionParams;
use crate::params::console::shadow as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The springs the plate hangs on, per side.
const SPRINGS: usize = 4;
/// The felt pads the damper can wind in.
const PADS: usize = 6;

struct Lay {
    /// The frame the plate hangs in.
    frame: egui::Rect,
    predelay: egui::Rect,
    size: egui::Rect,
    damp: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::PREDELAY, self.predelay),
            (p::SIZE, self.size),
            (p::DAMP, self.damp),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let frame = egui::Rect::from_min_max(
        egui::pos2(inner.left(), inner.top() + 0.14 * h),
        egui::pos2(inner.right(), inner.top() + 0.72 * h),
    );
    Lay {
        // SIZE is the plate hung in the frame.
        size: frame,
        frame,
        // The throw from the driver to the sheet is the pre-delay, and
        // it runs along the top of the card.
        predelay: egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), frame.top() - 3.0)),
        // The damper winds in from the right wall, so it takes the bay.
        damp: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.left(), frame.bottom() + 5.0),
                egui::pos2(inner.right(), frame.bottom() + 19.0),
            )
        }),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay());
    let (ink, edge) = (face.ink(), face.edge());
    let mut shapes = Vec::new();

    let size = face.anim("size", face.place(p::SIZE), 0.14);
    let damp = face.place(p::DAMP);
    let ringing = tool::norm(face.level_db(), -54.0, 0.0);
    let beat = face.phase.beat * core::f32::consts::TAU;
    let rolling = if face.phase.rolling { 1.0 } else { 0.0 };

    // THE FRAME: fixed, because a plate's frame is what it is hung from.
    chrome::trace(
        &mut shapes,
        &[
            lay.frame.left_top(),
            lay.frame.right_top(),
            lay.frame.right_bottom(),
            lay.frame.left_bottom(),
            lay.frame.left_top(),
        ],
        Weight::Hair,
        tool::fade(edge, 1.1),
    );

    // THE PLATE: as much steel as the size hangs, and it flexes with
    // what the return is returning. A plate nothing is sent to hangs
    // dead still.
    let sheet = egui::Rect::from_center_size(
        lay.frame.center(),
        egui::vec2(
            lay.frame.width() * (0.40 + 0.52 * size),
            lay.frame.height() * (0.36 + 0.50 * size),
        ),
    );
    shapes.push(egui::Shape::rect_filled(
        sheet,
        0.0,
        tool::fade(face.live(), 0.05 + 0.16 * ringing),
    ));
    // The sheet's own ripple: a flex across it, moving on the transport
    // and scaled by what is actually ringing.
    for i in 0..5 {
        let t = (i as f32 + 1.0) / 6.0;
        let y = egui::lerp(sheet.y_range(), t);
        let flex: Vec<egui::Pos2> = (0..=20)
            .map(|j| {
                let s = j as f32 / 20.0;
                let bow = (s * core::f32::consts::PI).sin()
                    * ringing
                    * 4.0
                    * (beat * 3.0 + t * 5.0 + s * 2.0).sin()
                    * rolling;
                egui::pos2(egui::lerp(sheet.x_range(), s), y + bow)
            })
            .collect();
        chrome::trace(
            &mut shapes,
            &flex,
            Weight::Hair,
            tool::fade(ink, 0.28 + 0.4 * ringing),
        );
    }
    chrome::trace(
        &mut shapes,
        &[
            sheet.left_top(),
            sheet.right_top(),
            sheet.right_bottom(),
            sheet.left_bottom(),
            sheet.left_top(),
        ],
        Weight::Heavy,
        ink,
    );
    // THE SPRINGS: the plate hangs on them, and they stretch as more
    // steel is hung.
    for i in 0..SPRINGS {
        let t = (i as f32 + 0.5) / SPRINGS as f32;
        let x = egui::lerp(sheet.x_range(), t);
        for (from, to) in [
            (lay.frame.top(), sheet.top()),
            (sheet.bottom(), lay.frame.bottom()),
        ] {
            let coil: Vec<egui::Pos2> = (0..=6)
                .map(|j| {
                    let s = j as f32 / 6.0;
                    egui::pos2(
                        x + if j % 2 == 0 { -2.0 } else { 2.0 },
                        egui::lerp(from..=to, s),
                    )
                })
                .collect();
            chrome::trace(&mut shapes, &coil, Weight::Hair, tool::fade(edge, 0.8));
        }
    }
    tool::halo(&mut shapes, lay.size, face.lit(p::SIZE), face.focus());

    // THE DRIVER and its THROW: the pre-delay is the run from the
    // driver to the sheet, so a long one puts the driver far off.
    let throw = lay.predelay;
    let gap = tool::norm(face.value(p::PREDELAY), 0.0, 200.0);
    let driver = egui::pos2(
        egui::lerp(throw.x_range(), 1.0 - gap.clamp(0.04, 0.92)),
        throw.center().y,
    );
    chrome::trace(
        &mut shapes,
        &[driver, egui::pos2(sheet.center().x, sheet.top())],
        Weight::Hair,
        tool::mix_ink(tool::fade(edge, 0.9), face.live(), ringing),
    );
    chrome::octagon(
        &mut shapes,
        egui::Rect::from_center_size(driver, egui::vec2(9.0, 9.0)),
        2.0,
        Some(tool::fade(face.live(), 0.3 + 0.6 * ringing)),
        Some((Weight::Hair, ink)),
    );
    tool::halo(&mut shapes, throw, face.lit(p::PREDELAY), face.focus());

    // THE DAMPER: felt pads wound in against the sheet. At nothing they
    // stand clear of it; wound all the way they are pressed onto it,
    // which is a plate with no tail left.
    let bar = lay.damp;
    let pads = (damp * PADS as f32).round() as usize;
    let reach = bar.height() * 0.5 * damp;
    for i in 0..PADS {
        let t = (i as f32 + 0.5) / PADS as f32;
        let x = egui::lerp(bar.x_range(), t);
        let on = i < pads;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, bar.bottom() - 2.0),
                egui::pos2(x, bar.bottom() - 2.0 - if on { 4.0 + reach } else { 2.0 }),
            ],
            if on { Weight::Heavy } else { Weight::Hair },
            if on {
                tool::mix_ink(ink, face.focus(), face.lit(p::DAMP))
            } else {
                tool::fade(edge, 0.6)
            },
        );
    }
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(bar.left(), bar.bottom() - 1.0),
            egui::pos2(bar.right(), bar.bottom() - 1.0),
        ],
        Weight::Hair,
        tool::fade(edge, 1.0),
    );
    tool::halo(&mut shapes, bar, face.lit(p::DAMP), face.focus());

    face.painter.extend(shapes);

    // The mark carries what the plate is ringing with.
    face.mark_signed(&lay, nav_cursor::Signature::Level(ringing));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Shadow), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Shadow).shrink(3.0);
        (
            piece,
            lay(glass, strip::bay_rect(piece, SectionKind::Shadow)),
        )
    }

    #[test]
    fn every_shadow_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Shadow.table() {
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
        assert_eq!(lay.controls().len(), 3);
        let _ = SectionParams::of(SectionKind::Shadow);
    }

    /// The plate hangs INSIDE its frame at every size, so the springs
    /// always have somewhere to be.
    #[test]
    fn the_plate_always_hangs_inside_its_frame() {
        let (_, lay) = laid();
        for size in [0.0f32, 0.5, 1.0] {
            let sheet = egui::Rect::from_center_size(
                lay.frame.center(),
                egui::vec2(
                    lay.frame.width() * (0.40 + 0.52 * size),
                    lay.frame.height() * (0.36 + 0.50 * size),
                ),
            );
            assert!(lay.frame.contains_rect(sheet), "the plate burst its frame");
            assert!(sheet.top() > lay.frame.top(), "no room for a spring");
            assert!(sheet.bottom() < lay.frame.bottom());
        }
    }

    /// The driver stands off the plate by the pre-delay, and the throw
    /// runs above the frame rather than through it.
    #[test]
    fn the_throw_runs_from_the_driver_to_the_sheet() {
        let (_, lay) = laid();
        assert!(lay.predelay.bottom() <= lay.frame.top() + 0.01);
        let at = |ms: f32| {
            egui::lerp(
                lay.predelay.x_range(),
                1.0 - tool::norm(ms, 0.0, 200.0).clamp(0.04, 0.92),
            )
        };
        assert!(at(200.0) < at(0.0) - 10.0, "the driver did not step back");
        assert!(at(200.0) >= lay.predelay.left() - 0.01);
    }
}
