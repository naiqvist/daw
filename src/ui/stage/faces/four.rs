//! FOUR's face: frequency climbs, and four arms bend the signal's spine.
//!
//! After the Neve inductor EQ card, drawn as its own elevation. One
//! spine runs across the glass — the response the engine is actually
//! running, taken from the section's own coefficients — and four arms
//! reach up to it. Each arm has three joints: where along the spectrum
//! it grips, how far it bends the spine, and how wide its grip is (a Q
//! for the two mids, a shelf-or-bell cap for the two ends).
//!
//! Behind the spine, three zones light with what the section actually
//! put out below 200 Hz, between 200 Hz and 2 kHz, and above — so a
//! boost that is doing nothing because nothing is there says so.

use super::*;
use crate::console::SectionParams;
use crate::params::console::four as p;
use crate::ui::nav_cursor;

/// The face's decibel span, top to bottom.
const SPAN_DB: f32 = 16.0;
/// A band's level scale: silence at the foot, this at the head.
const ZONE_FLOOR_DB: f32 = -60.0;

/// The four arms, each as (frequency, gain, width) parameter.
const ARMS: [(u32, u32, u32); 4] = [
    (p::LOW_HZ, p::LOW_DB, p::LOW_SHAPE),
    (p::LMF_HZ, p::LMF_DB, p::LMF_Q),
    (p::HMF_HZ, p::HMF_DB, p::HMF_Q),
    (p::HIGH_HZ, p::HIGH_DB, p::HIGH_SHAPE),
];

struct Lay {
    field: egui::Rect,
    /// Per arm: the grip on the spine, the frequency rail, the width pad.
    gain: [egui::Rect; 4],
    freq: [egui::Rect; 4],
    width: [egui::Rect; 4],
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        let mut out = Vec::with_capacity(12);
        for (i, (hz, db, w)) in ARMS.into_iter().enumerate() {
            out.push((hz, self.freq[i]));
            out.push((db, self.gain[i]));
            out.push((w, self.width[i]));
        }
        out
    }
}

fn lay(glass: egui::Rect, params: &SectionParams) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let rail_h = 11.0;
    let pad_h = 11.0;
    let field = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.right(), inner.bottom() - rail_h - pad_h - 6.0),
    );
    let rail = egui::Rect::from_min_max(
        egui::pos2(inner.left(), field.bottom() + 3.0),
        egui::pos2(inner.right(), field.bottom() + 3.0 + rail_h),
    );
    let pads = egui::Rect::from_min_max(
        egui::pos2(inner.left(), rail.bottom() + 3.0),
        egui::pos2(inner.right(), rail.bottom() + 3.0 + pad_h),
    );
    let half = (field.width() * 0.10).clamp(9.0, 18.0);
    let mut gain = [egui::Rect::NOTHING; 4];
    let mut freq = [egui::Rect::NOTHING; 4];
    let mut width = [egui::Rect::NOTHING; 4];
    for (i, (hz_id, db_id, _)) in ARMS.into_iter().enumerate() {
        let x = tool::octave_x(field, params.value(hz_id))
            .clamp(field.left() + half, field.right() - half);
        let y = tool::db_y(field, params.value(db_id), SPAN_DB);
        gain[i] = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(half * 2.0, 15.0));
        freq[i] = egui::Rect::from_min_max(
            egui::pos2(x - half, rail.top()),
            egui::pos2(x + half, rail.bottom()),
        );
        width[i] = egui::Rect::from_min_max(
            egui::pos2(x - half, pads.top()),
            egui::pos2(x + half, pads.bottom()),
        );
    }
    Lay {
        field,
        gain,
        freq,
        width,
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, &face.params);
    let (ink, edge) = (face.ink(), face.edge());
    let field = lay.field;
    let said = face.said;
    let mut shapes = Vec::new();

    // THE ZONES: what the section actually put out in each third of the
    // spectrum, behind the spine, so a boost with nothing under it is
    // visibly a boost with nothing under it.
    let zone_edges = [
        field.left(),
        tool::octave_x(field, 200.0),
        tool::octave_x(field, 2_000.0),
        field.right(),
    ];
    for band in 0..3 {
        let level = face.anim(
            ["zlo", "zmid", "zhi"][band],
            tool::norm(said.bands[band], ZONE_FLOOR_DB, 0.0),
            0.10,
        );
        let zone = egui::Rect::from_min_max(
            egui::pos2(zone_edges[band], field.bottom() - field.height() * level),
            egui::pos2(zone_edges[band + 1], field.bottom()),
        );
        if zone.is_positive() {
            shapes.push(egui::Shape::rect_filled(
                zone,
                0.0,
                tool::fade(tool::BAND_INK[band], 0.13),
            ));
        }
    }

    // The ruling: the unity line across the middle, a tick per decade.
    let zero = tool::db_y(field, 0.0, SPAN_DB);
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), zero),
            egui::pos2(field.right(), zero),
        ],
        Weight::Hair,
        tool::fade(edge, 1.2),
    );
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

    // THE SPINE: the engine's own response, from the section's own
    // coefficients. The one line on the glass that is measured rather
    // than set.
    let shape = crate::console::four_curve::Shape::of(&face.params);
    let spine = tool::response(field, 56, SPAN_DB, |hz| {
        crate::console::four_curve::response_db(&shape, 48_000.0, hz)
    });
    circuit::trace(&mut shapes, &spine, Weight::Heavy, ink);

    // THE FOUR ARMS. Each grips the spine where its frequency puts it,
    // reaches down to its own rail, and carries its width as a pad.
    for (i, (hz_id, db_id, width_id)) in ARMS.into_iter().enumerate() {
        let hue = tool::BAND_INK[i.min(2)];
        let grip = lay.gain[i].center();
        let lit = face.lit(db_id).max(face.lit(hz_id)).max(face.lit(width_id));
        // The arm: from the pad, up through the rail, to the spine.
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(grip.x, lay.width[i].center().y),
                egui::pos2(grip.x, grip.y),
            ],
            Weight::Hair,
            tool::fade(hue, 0.30 + 0.55 * lit),
        );
        // The grip itself, sized by how far this arm is bending.
        let bend = (face.value(db_id) / 15.0).clamp(-1.0, 1.0);
        circuit::octagon(
            &mut shapes,
            egui::Rect::from_center_size(
                grip,
                egui::vec2(9.0 + 4.0 * bend.abs(), 9.0 + 4.0 * bend.abs()),
            ),
            2.0,
            Some(tool::fade(hue, 0.35 + 0.65 * bend.abs())),
            Some((Weight::Hair, ink)),
        );
        tool::halo(&mut shapes, lay.gain[i], face.lit(db_id), face.focus());

        // The frequency rail: the arm's foot slides along the spectrum.
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(lay.freq[i].left(), lay.freq[i].center().y),
                egui::pos2(lay.freq[i].right(), lay.freq[i].center().y),
            ],
            Weight::Hair,
            tool::fade(hue, 0.4 + 0.6 * face.lit(hz_id)),
        );
        circuit::pad(
            &mut shapes,
            egui::pos2(grip.x, lay.freq[i].center().y),
            circuit::PAD - 1.0,
            hue,
            true,
        );
        tool::halo(&mut shapes, lay.freq[i], face.lit(hz_id), face.focus());

        // The width: a comb whose teeth close as the grip narrows. The
        // two ends carry a shelf-or-bell cap instead, which is a wall
        // that either runs off the edge or turns back on itself.
        let pad = lay.width[i];
        if i == 0 || i == 3 {
            let bell = face.value(width_id) >= 0.5;
            let run = if i == 0 { -1.0f32 } else { 1.0 };
            let top = pad.top() + 2.0;
            let mut path = vec![
                egui::pos2(pad.center().x - run * 7.0, pad.bottom() - 2.0),
                egui::pos2(pad.center().x, top),
            ];
            path.push(if bell {
                egui::pos2(pad.center().x + run * 7.0, pad.bottom() - 2.0)
            } else {
                egui::pos2(pad.center().x + run * 7.0, top)
            });
            circuit::trace(&mut shapes, &path, Weight::Hair, tool::fade(hue, 0.9));
        } else {
            let narrow = tool::norm(face.value(width_id), 0.3, 4.0);
            let half = egui::lerp(7.0..=1.5, narrow);
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(pad.center().x - half, pad.bottom() - 2.0),
                    egui::pos2(pad.center().x, pad.top() + 2.0),
                    egui::pos2(pad.center().x + half, pad.bottom() - 2.0),
                ],
                Weight::Hair,
                tool::fade(hue, 0.9),
            );
        }
        tool::halo(&mut shapes, pad, face.lit(width_id), face.focus());
    }

    face.painter.extend(shapes);

    // The mark takes the arm's own hue and leans the way it is bending.
    face.mark_signed(
        &lay,
        match face.selected {
            Some(s) => {
                let arm = ARMS
                    .iter()
                    .position(|(hz, db, w)| {
                        s == *hz as usize || s == *db as usize || s == *w as usize
                    })
                    .unwrap_or(0);
                nav_cursor::Signature::Band {
                    ink: tool::BAND_INK[arm.min(2)],
                    amount: (face.value(ARMS[arm].1) / 15.0).clamp(-1.0, 1.0),
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
            egui::vec2(strip::width_of(SectionKind::Four), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Four).shrink(3.0);
        (piece, lay(glass, params))
    }

    #[test]
    fn every_four_parameter_has_one_instrument() {
        let params = SectionParams::of(SectionKind::Four);
        let (piece, lay) = laid(&params);
        for def in SectionKind::Four.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(piece.contains_rect(rect), "{} left the piece", def.name);
        }
        assert_eq!(lay.controls().len(), 12, "twelve knobs, twelve instruments");
    }

    /// The four arms stand in frequency order, and an arm's grip walks
    /// up the glass when it is boosted and down when it is cut.
    #[test]
    fn the_arms_stand_in_order_and_the_grip_follows_the_gain() {
        let params = SectionParams::of(SectionKind::Four);
        let (_, flat) = laid(&params);
        for arm in 0..3 {
            assert!(
                flat.freq[arm].center().x < flat.freq[arm + 1].center().x,
                "arm {arm} is out of frequency order"
            );
        }
        // Every arm's three joints share a column.
        for arm in 0..4 {
            assert!((flat.freq[arm].center().x - flat.gain[arm].center().x).abs() < 0.01);
            assert!((flat.width[arm].center().x - flat.gain[arm].center().x).abs() < 0.01);
        }
        let mut boosted = SectionParams::of(SectionKind::Four);
        boosted.set(p::LMF_DB, 12.0);
        let (_, up) = laid(&boosted);
        assert!(up.gain[1].center().y < flat.gain[1].center().y, "a boost sank");
        let mut cut = SectionParams::of(SectionKind::Four);
        cut.set(p::LMF_DB, -12.0);
        let (_, down) = laid(&cut);
        assert!(down.gain[1].center().y > flat.gain[1].center().y, "a cut rose");
        // And sweeping an arm walks it along the spectrum.
        let mut swept = SectionParams::of(SectionKind::Four);
        swept.set(p::LMF_HZ, 1_800.0);
        let (_, moved) = laid(&swept);
        assert!(moved.freq[1].center().x > flat.freq[1].center().x + 15.0);
    }

    /// The spine is the engine's own response, so bending an arm bends
    /// the line the card draws.
    #[test]
    fn the_spine_is_the_engines_own_response() {
        use crate::console::four_curve;
        let flat = four_curve::Shape::of(&SectionParams::of(SectionKind::Four));
        assert!(four_curve::response_db(&flat, 48_000.0, 1_000.0).abs() < 0.01);
        let mut params = SectionParams::of(SectionKind::Four);
        params.set(p::LMF_HZ, 1_000.0);
        params.set(p::LMF_DB, 12.0);
        let bent = four_curve::Shape::of(&params);
        assert!(
            four_curve::response_db(&bent, 48_000.0, 1_000.0) > 6.0,
            "the arm did not bend the spine"
        );
    }
}
