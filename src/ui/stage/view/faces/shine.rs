//! SHINE's face: a prism, and the lines it throws on the plate.
//!
//! After a constant-deviation spectrometer. One beam comes in, meets
//! the prism, and is spread across a plate. What the section MAKES —
//! harmonics that were not in the signal — appears as emission lines on
//! that plate, and they only ever appear ABOVE the tune frequency,
//! which is exactly where the section generates them.
//!
//! TUNE moves the prism, and the lines move with it. AMOUNT is how hard
//! the tube is driven, so the lines brighten and multiply. MIX is the
//! shutter in front of the plate: how much of what was made is let
//! through to the eye. The tube's glow is not the knob — it is the heat
//! the shaper reported.

use super::*;
use crate::console::SectionParams;
use crate::params::console::shine as p;
use crate::ui::nav_cursor;

/// The most emission lines the plate shows at full amount.
const LINES: usize = 9;

struct Lay {
    plate: egui::Rect,
    amount: egui::Rect,
    tune: egui::Rect,
    mix: egui::Rect,
    /// Where the prism stands, and where the beam enters it.
    prism: egui::Pos2,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::AMOUNT, self.amount),
            (p::TUNE, self.tune),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let plate = egui::Rect::from_min_max(
        egui::pos2(inner.left(), inner.top() + 0.30 * h),
        egui::pos2(inner.right(), inner.top() + 0.72 * h),
    );
    Lay {
        plate,
        // AMOUNT is the discharge tube, above the prism.
        amount: egui::Rect::from_min_max(
            egui::pos2(inner.left() + 0.10 * w, inner.top()),
            egui::pos2(inner.right() - 0.10 * w, plate.top() - 6.0),
        ),
        // TUNE is the plate's own axis: the prism's angle sets where
        // the lines begin, so the plate IS the control.
        tune: plate,
        // MIX takes the plinth: a shutter along the foot.
        mix: plinth.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.left(), plate.bottom() + 6.0),
                egui::pos2(inner.right(), plate.bottom() + 20.0),
            )
        }),
        prism: egui::pos2(inner.left() + 0.14 * w, plate.top() - 3.0),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let mut shapes = Vec::new();
    let heat = face.anim("heat", face.said.reduction_db.abs().clamp(0.0, 1.0), 0.12);
    let amount = face.place(p::AMOUNT);
    let mix = face.place(p::MIX);

    // THE TUBE: the stage that makes the harmonics. Its glow is the
    // heat the shaper reported, not the knob — a tube driven into
    // silence does not glow.
    let tube = lay.amount;
    tool::well(&mut shapes, tube, face.ground(), edge);
    let bore = tube.shrink2(egui::vec2(8.0, 4.0));
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(bore.left(), bore.top()),
            egui::pos2(egui::lerp(bore.x_range(), amount), bore.bottom()),
        ),
        0.0,
        tool::mix_ink(tool::fade(ink, 0.35), face.hot(), heat),
    ));
    for i in 0..7 {
        let x = egui::lerp(bore.x_range(), i as f32 / 6.0);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, bore.top() - 2.0),
                egui::pos2(x, bore.bottom() + 2.0),
            ],
            Weight::Hair,
            tool::fade(edge, 0.6),
        );
    }
    tool::halo(&mut shapes, tube, face.lit(p::AMOUNT), face.focus());

    // THE PRISM and the beam through it.
    let tune_x = tool::octave_x(lay.plate, face.value(p::TUNE));
    let tune_x = face.anim("tune", tune_x, 0.12);
    shapes.push(egui::Shape::convex_polygon(
        vec![
            egui::pos2(lay.prism.x, lay.prism.y - 9.0),
            egui::pos2(lay.prism.x - 8.0, lay.prism.y + 5.0),
            egui::pos2(lay.prism.x + 8.0, lay.prism.y + 5.0),
        ],
        tool::fade(edge, 0.8),
        egui::Stroke::NONE,
    ));
    circuit::trace(
        &mut shapes,
        &[egui::pos2(lay.plate.left(), lay.prism.y - 12.0), lay.prism],
        Weight::Hair,
        face.live(),
    );

    // THE PLATE: the spectrum, with the tune's own post standing on it.
    // Everything left of the post is untouched; everything right of it
    // is where the section is allowed to put something new.
    tool::well(&mut shapes, lay.plate, face.ground(), edge);
    let field = lay.plate.shrink(3.0);
    for hz in [1_000.0, 4_000.0, 12_000.0] {
        let x = tool::octave_x(lay.plate, hz);
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom() - 3.0),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Hair,
            tool::fade(edge, 0.7),
        );
    }
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(tune_x, lay.plate.top()),
            egui::pos2(tune_x, lay.plate.bottom()),
        ],
        Weight::Heavy,
        tool::mix_ink(ink, face.focus(), face.lit(p::TUNE)),
    );

    // THE EMISSION LINES: only above the tune, only as many as the
    // amount asks for, and only as bright as the shutter lets through
    // and the tube is actually burning.
    let lit = (amount * LINES as f32).round() as usize;
    for i in 0..LINES {
        let t = (i as f32 + 1.0) / (LINES as f32 + 1.0);
        let x = egui::lerp(tune_x..=(field.right() - 2.0), t);
        if x <= tune_x + 1.0 {
            continue;
        }
        let on = i < lit;
        // A line's height falls with its order, as a harmonic series
        // does; its brightness is the tube's heat through the shutter.
        let tall = field.height() * (0.85 / (i as f32 + 1.0).sqrt());
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom()),
                egui::pos2(x, field.bottom() - if on { tall } else { 2.0 }),
            ],
            if on { Weight::Heavy } else { Weight::Hair },
            if on {
                tool::fade(
                    tool::mix_ink(tool::HI_INK, face.hot(), heat),
                    0.30 + 0.70 * mix,
                )
            } else {
                tool::fade(edge, 0.35)
            },
        );
    }
    tool::halo(&mut shapes, lay.plate, face.lit(p::TUNE), face.focus());

    // THE SHUTTER: how much of the plate the eye is allowed to see.
    let shutter = lay.mix;
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            shutter.min,
            egui::pos2(egui::lerp(shutter.x_range(), mix), shutter.bottom()),
        ),
        0.0,
        tool::fade(ink, 0.5),
    ));
    circuit::trace(
        &mut shapes,
        &[
            shutter.left_top(),
            shutter.right_top(),
            shutter.right_bottom(),
            shutter.left_bottom(),
            shutter.left_top(),
        ],
        Weight::Hair,
        edge,
    );
    tool::halo(&mut shapes, shutter, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // Standing on TUNE the mark sweeps the plate; on the other two it
    // carries the heat the tube is actually burning.
    face.mark_signed(
        &lay,
        match face.selected {
            Some(s) if s == p::TUNE as usize => {
                nav_cursor::Signature::Sweep(face.place(p::TUNE) * 2.0 - 1.0)
            }
            Some(_) => nav_cursor::Signature::Level(heat),
            None => nav_cursor::Signature::Plain,
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
            egui::vec2(strip::width_of(SectionKind::Shine), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Shine).shrink(3.0);
        (
            piece,
            lay(glass, strip::plinth_rect(piece, SectionKind::Shine)),
        )
    }

    #[test]
    fn every_shine_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Shine.table() {
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
        let _ = SectionParams::of(SectionKind::Shine);
    }

    /// The tube stands above the plate and the shutter below it, and
    /// the prism sits where the beam enters.
    #[test]
    fn the_bench_is_stacked_tube_prism_plate_shutter() {
        let (_, lay) = laid();
        assert!(lay.amount.bottom() <= lay.plate.top() + 0.01);
        assert!(lay.mix.top() >= lay.plate.bottom() - 0.01);
        assert!(lay.prism.y <= lay.plate.top() + 0.01);
        assert!(lay.prism.x > lay.plate.left());
    }

    /// The section only makes harmonics above its tune, so the lines
    /// only ever stand to the right of the post — and the post walks
    /// when the prism is turned.
    #[test]
    fn the_lines_stand_above_the_tune_and_walk_with_it() {
        let (_, lay) = laid();
        let low = tool::octave_x(lay.plate, 1_000.0);
        let high = tool::octave_x(lay.plate, 10_000.0);
        assert!(high > low + 20.0, "the prism did not turn");
        // The first line is placed inside the run from the post to the
        // plate's edge, so it can never fall below the tune.
        let first = egui::lerp(high..=(lay.plate.right() - 5.0), 0.1);
        assert!(first > high, "a line fell below the tune");
        assert!(first <= lay.plate.right());
    }
}
