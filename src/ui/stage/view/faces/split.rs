//! SPLIT's face: an optical bench, two dichroic seams and three rays.
//!
//! A three-band dynamics section is a beam split three ways and put
//! back together, so that is what is drawn. The spectrum runs across
//! the top; two seams stand where the crossover actually is, and each
//! of the three beams below runs through a lens of its own.
//!
//! A lens BENDS its ray. Bent down is compression, bent up is
//! expansion, and the bend the eye sees is not the knob: it is the gain
//! the band's detector is applying right now, so the band that is
//! working is the ray that is moving. The knob is the lens's own
//! curvature, drawn where a lens's curvature is.

use super::*;
use crate::console::SectionParams;
use crate::params::console::split as p;
use crate::ui::nav_cursor;

/// How far a ray bends, in points, at the section's full reach.
const BEND: f32 = 16.0;
/// The reach the bands report against, in dB.
const REACH_DB: f32 = 12.0;

/// Each band's amount and its make-up, in band order.
const BANDS: [(u32, u32); 3] = [
    (p::LOW, p::LOW_DB),
    (p::MID, p::MID_DB),
    (p::HIGH, p::HIGH_DB),
];

struct Lay {
    field: egui::Rect,
    /// The two seams, on the spectrum rail.
    low_hz: egui::Rect,
    high_hz: egui::Rect,
    /// Per band: the lens, and the post its ray leaves by.
    lens: [egui::Rect; 3],
    gain: [egui::Rect; 3],
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        let mut out = vec![(p::LOW_HZ, self.low_hz), (p::HIGH_HZ, self.high_hz)];
        for (i, (amount, gain)) in BANDS.into_iter().enumerate() {
            out.push((amount, self.lens[i]));
            out.push((gain, self.gain[i]));
        }
        out
    }
}

fn lay(glass: egui::Rect, params: &SectionParams) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let rail_h = 13.0;
    let rail = egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + rail_h));
    let field = egui::Rect::from_min_max(egui::pos2(inner.left(), rail.bottom() + 4.0), inner.max);
    let seam = |hz: f32| {
        let x = tool::octave_x(rail, hz).clamp(rail.left() + 7.0, rail.right() - 7.0);
        egui::Rect::from_min_max(
            egui::pos2(x - 7.0, rail.top()),
            egui::pos2(x + 7.0, rail.bottom()),
        )
    };
    // The three beams share the field's height; each carries its lens
    // in the middle of its lane and its post at the lane's right end.
    let lane_h = field.height() / 3.0;
    let lane = |i: usize| {
        egui::Rect::from_min_max(
            egui::pos2(field.left(), field.top() + lane_h * i as f32),
            egui::pos2(field.right(), field.top() + lane_h * (i as f32 + 1.0)),
        )
    };
    let mut lens = [egui::Rect::NOTHING; 3];
    let mut gain = [egui::Rect::NOTHING; 3];
    for i in 0..3 {
        let l = lane(i).shrink2(egui::vec2(0.0, 2.0));
        lens[i] = egui::Rect::from_min_max(
            egui::pos2(l.left() + l.width() * 0.30, l.top()),
            egui::pos2(l.left() + l.width() * 0.52, l.bottom()),
        );
        gain[i] = egui::Rect::from_min_max(
            egui::pos2(l.right() - l.width() * 0.22, l.top()),
            egui::pos2(l.right(), l.bottom()),
        );
    }
    Lay {
        field,
        low_hz: seam(params.value(p::LOW_HZ)),
        high_hz: seam(params.value(p::HIGH_HZ)),
        lens,
        gain,
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, &face.params);
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    // THE SPECTRUM RAIL and the two seams the crossover really stands
    // at. Between them, each band's own stretch is tinted its own hue.
    let rail = egui::Rect::from_min_max(
        egui::pos2(lay.field.left(), lay.field.top() - 17.0),
        egui::pos2(lay.field.right(), lay.field.top() - 4.0),
    );
    let cuts = [
        rail.left(),
        lay.low_hz.center().x,
        lay.high_hz.center().x,
        rail.right(),
    ];
    for band in 0..3 {
        let zone = egui::Rect::from_min_max(
            egui::pos2(cuts[band], rail.top()),
            egui::pos2(cuts[band + 1], rail.bottom()),
        );
        if zone.is_positive() {
            shapes.push(egui::Shape::rect_filled(
                zone,
                0.0,
                tool::fade(tool::BAND_INK[band], 0.20),
            ));
        }
    }
    for (rect, param) in [(lay.low_hz, p::LOW_HZ), (lay.high_hz, p::HIGH_HZ)] {
        let x = rect.center().x;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(x, rail.top() - 2.0),
                egui::pos2(x, lay.field.bottom()),
            ],
            Weight::Hair,
            tool::mix_ink(tool::fade(edge, 0.7), face.focus(), face.lit(param)),
        );
        circuit::pad(
            &mut shapes,
            egui::pos2(x, rail.center().y),
            circuit::PAD,
            ink,
            true,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    // THE THREE RAYS. Each enters level, meets its lens, and leaves
    // bent by the gain that band's detector is actually applying.
    for (band, (amount_id, gain_id)) in BANDS.into_iter().enumerate() {
        let hue = tool::BAND_INK[band];
        let lens = lay.lens[band];
        let post = lay.gain[band];
        let y = lens.center().y;
        // The measured bend: the band's applied gain, in dB, over the
        // section's own reach. Negative is compression, so the ray
        // falls; positive is expansion, so it rises.
        let applied = face.anim(
            ["blo", "bmid", "bhi"][band],
            (said.bands[band] / REACH_DB).clamp(-1.0, 1.0),
            0.09,
        );
        let exit = y - applied * BEND;
        circuit::trace(
            &mut shapes,
            &[egui::pos2(lay.field.left(), y), egui::pos2(lens.left(), y)],
            Weight::Hair,
            tool::fade(hue, 0.55),
        );
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(lens.right(), y),
                egui::pos2(post.left(), exit),
                egui::pos2(post.right(), exit),
            ],
            Weight::Heavy,
            tool::fade(hue, 0.5 + 0.5 * applied.abs()),
        );

        // A lens's curvature IS the knob: convex where the band is
        // compressed, concave where it is expanded, flat glass at
        // nothing — and flat glass bends no ray, which is the truth.
        let curve = face.swing(amount_id).clamp(-1.0, 1.0);
        let bulge = 6.0 * curve;
        for side in [-1.0f32, 1.0] {
            let bow: Vec<egui::Pos2> = (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    let swell = (t * core::f32::consts::PI).sin() * bulge * side;
                    egui::pos2(
                        lens.center().x + side * 4.0 + swell,
                        egui::lerp(lens.y_range(), t),
                    )
                })
                .collect();
            circuit::trace(&mut shapes, &bow, Weight::Hair, tool::fade(hue, 0.9));
        }
        tool::halo(&mut shapes, lens, face.lit(amount_id), face.focus());

        // THE POST: the band's own make-up, a small bipolar rail the
        // exit ray lands on.
        tool::swing_rail(
            &mut shapes,
            egui::Rect::from_min_max(
                egui::pos2(post.left(), post.bottom() - 7.0),
                egui::pos2(post.right(), post.bottom() - 1.0),
            ),
            face.swing(gain_id),
            7,
            tool::mix_ink(hue, face.focus(), face.lit(gain_id)),
            tool::fade(edge, 0.55),
        );
        tool::halo(&mut shapes, post, face.lit(gain_id), face.focus());
    }

    face.painter.extend(shapes);

    // The mark takes the band's hue and leans the way that band's ray
    // is being bent, right now.
    face.mark_signed(
        &lay,
        match face.selected {
            Some(s) => {
                let band = BANDS
                    .iter()
                    .position(|(a, g)| s == *a as usize || s == *g as usize);
                match band {
                    Some(b) => nav_cursor::Signature::Band {
                        ink: tool::BAND_INK[b],
                        amount: (said.bands[b] / REACH_DB).clamp(-1.0, 1.0),
                    },
                    None => nav_cursor::Signature::Sweep(
                        if s == p::LOW_HZ as usize {
                            face.place(p::LOW_HZ)
                        } else {
                            face.place(p::HIGH_HZ)
                        } * 2.0
                            - 1.0,
                    ),
                }
            }
            None => nav_cursor::Signature::Plain,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid(params: &SectionParams) -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Split), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Split).shrink(3.0);
        (piece, lay(glass, params))
    }

    #[test]
    fn every_split_parameter_has_one_instrument() {
        let params = SectionParams::of(SectionKind::Split);
        let (piece, lay) = laid(&params);
        for def in SectionKind::Split.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(piece.contains_rect(rect), "{} left the piece", def.name);
        }
        assert_eq!(lay.controls().len(), 8);
    }

    /// The three beams are stacked low to high, and the two seams stand
    /// where the crossover puts them.
    #[test]
    fn the_beams_stack_and_the_seams_move_with_the_crossover() {
        let params = SectionParams::of(SectionKind::Split);
        let (_, lay) = laid(&params);
        for band in 0..2 {
            assert!(
                lay.lens[band].center().y < lay.lens[band + 1].center().y,
                "band {band} is out of order"
            );
            assert!(!lay.lens[band].intersects(lay.gain[band]));
        }
        assert!(lay.low_hz.center().x < lay.high_hz.center().x);
        let mut moved = SectionParams::of(SectionKind::Split);
        moved.set(p::LOW_HZ, 480.0);
        let (_, wide) = laid(&moved);
        assert!(
            wide.low_hz.center().x > lay.low_hz.center().x + 10.0,
            "the seam did not travel"
        );
        // Every lens shares its lane's ray with its own post.
        for band in 0..3 {
            assert!(lay.gain[band].left() > lay.lens[band].right());
        }
    }
}
