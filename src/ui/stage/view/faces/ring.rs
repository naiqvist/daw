//! RING's face: the diode bridge, and the carrier burning in it.
//!
//! After the Bode ring modulator: an input transformer, four matched
//! diodes in a diamond, and a carrier transformer across the middle.
//! The four diodes light with the carrier's own mean burn — measured,
//! not set, so the square carrier blazes, the triangle barely glows,
//! and the noise carrier is visibly unsteady while the tones are not.
//!
//! The carrier's wave is drawn at its LIVE frequency, so when HOLD is
//! running and the carrier is chasing the input's pitch, the wave on
//! the glass changes period with it. At MIX zero the bridge is shorted
//! by a jumper straight across it, which is what a wire looks like.

use super::*;
use crate::console::SectionParams;
use crate::params::console::ring as p;
use crate::ui::nav_cursor;

/// The four carriers the bridge can be fed.
const CARRIERS: usize = 4;
/// How many cycles of the carrier the window shows at its top note.
const WINDOW_CYCLES: f32 = 9.0;

struct Lay {
    /// The diamond of diodes.
    bridge: egui::Rect,
    /// The window the carrier's wave is drawn in.
    wave: egui::Rect,
    carrier: egui::Rect,
    hz: egui::Rect,
    hold: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::CARRIER, self.carrier),
            (p::HZ, self.hz),
            (p::HOLD_RATE, self.hold),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let bridge = egui::Rect::from_center_size(
        egui::pos2(inner.center().x, inner.top() + 0.26 * h),
        egui::vec2((0.44 * h).min(0.52 * w), 0.40 * h),
    );
    let wave = egui::Rect::from_min_max(
        egui::pos2(inner.left(), bridge.bottom() + 8.0),
        egui::pos2(inner.right(), bridge.bottom() + 8.0 + 0.22 * h),
    );
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 14.0),
        )
    };
    Lay {
        bridge,
        // HZ is the wave itself: its period IS the frequency.
        hz: wave,
        wave,
        carrier: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 20.0, inner.top()),
                egui::pos2(inner.right(), inner.top() + 24.0),
            )
        }),
        hold: row(wave.bottom() + 6.0, 0.0, 0.46),
        mix: plinth.unwrap_or_else(|| row(wave.bottom() + 6.0, 0.54, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let carrier = face.value(p::CARRIER).round().clamp(0.0, 3.0) as usize;
    let burn = face.anim("burn", said.bands[1].clamp(0.0, 1.0), 0.08);
    let mix = face.place(p::MIX);
    // The carrier's LIVE frequency: the set note while HOLD is off, and
    // whatever the carrier has been walked to while it is on.
    let live_hz = if said.bands[0] > 0.0 {
        said.bands[0]
    } else {
        face.value(p::HZ)
    };

    // THE BRIDGE: four diodes in a diamond, each lit by the carrier's
    // own mean burn. This is the only quantity on the card that could
    // not be guessed from the settings.
    let b = lay.bridge;
    let (top, bottom) = (
        egui::pos2(b.center().x, b.top()),
        egui::pos2(b.center().x, b.bottom()),
    );
    let (left, right) = (
        egui::pos2(b.left(), b.center().y),
        egui::pos2(b.right(), b.center().y),
    );
    for (from, to) in [(top, right), (right, bottom), (bottom, left), (left, top)] {
        circuit::trace(
            &mut shapes,
            &[from, to],
            Weight::Hair,
            tool::fade(edge, 0.9),
        );
        // The diode itself: a triangle into a bar, halfway along.
        let mid = egui::pos2((from.x + to.x) * 0.5, (from.y + to.y) * 0.5);
        let along = (to - from).normalized();
        let across = egui::vec2(-along.y, along.x);
        let hue = tool::mix_ink(tool::fade(edge, 1.2), face.hot(), burn);
        shapes.push(egui::Shape::convex_polygon(
            vec![
                mid + along * 4.0,
                mid - along * 3.0 + across * 4.0,
                mid - along * 3.0 - across * 4.0,
            ],
            hue,
            egui::Stroke::NONE,
        ));
        circuit::trace(
            &mut shapes,
            &[
                mid + along * 4.5 + across * 4.0,
                mid + along * 4.5 - across * 4.0,
            ],
            Weight::Hair,
            hue,
        );
    }
    // The carrier transformer across the middle, and the signal down it.
    circuit::trace(
        &mut shapes,
        &[left, right],
        Weight::Hair,
        tool::fade(edge, 0.6),
    );
    circuit::pad(&mut shapes, top, circuit::PAD - 1.0, ink, true);
    circuit::pad(&mut shapes, bottom, circuit::PAD - 1.0, ink, true);
    // AT MIX ZERO the bridge is shorted out by a jumper, which is what
    // a wire looks like, and no amount of carrier changes that.
    if mix < 0.005 {
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(b.left() - 6.0, b.center().y),
                egui::pos2(b.right() + 6.0, b.center().y),
            ],
            Weight::Bold,
            ink,
        );
    }

    // THE WAVE: the carrier at its live frequency. A higher note packs
    // more cycles into the window; a HOLD that has walked the carrier
    // changes the period on the glass as it walks.
    tool::well(&mut shapes, lay.wave, face.ground(), edge);
    let window = lay.wave.shrink(3.0);
    let cycles = (1.0 + WINDOW_CYCLES * tool::norm(live_hz, 20.0, 5_000.0)).max(1.0);
    let wave: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.0;
            let phase = t * cycles * core::f32::consts::TAU;
            // Each carrier draws its own shape, so the switch and the
            // window say the same thing.
            let s = match carrier {
                0 => phase.sin(),
                1 => (phase.sin().asin()) * (2.0 / core::f32::consts::PI),
                2 => phase.sin().signum(),
                _ => {
                    // The noise carrier: deterministic per column, so a
                    // parked card is a still picture.
                    let h = ((i as u32).wrapping_mul(2_246_822_519) >> 9) & 0xff;
                    h as f32 / 127.5 - 1.0
                }
            };
            egui::pos2(
                egui::lerp(window.x_range(), t),
                window.center().y - s * window.height() * 0.42,
            )
        })
        .collect();
    circuit::trace(
        &mut shapes,
        &wave,
        Weight::Heavy,
        tool::mix_ink(face.live(), face.focus(), face.lit(p::HZ)),
    );
    tool::halo(&mut shapes, lay.wave, face.lit(p::HZ), face.focus());

    // THE CARRIER SWITCH: four detents in the bay.
    let slot = lay.carrier;
    for i in 0..CARRIERS {
        let y = egui::lerp(
            (slot.top() + 4.0)..=(slot.bottom() - 4.0),
            i as f32 / (CARRIERS - 1) as f32,
        );
        let here = i == carrier;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(slot.left() + 2.0, y),
                egui::pos2(slot.right() - if here { 1.0 } else { 4.0 }, y),
            ],
            if here { Weight::Heavy } else { Weight::Hair },
            if here { ink } else { tool::fade(edge, 0.7) },
        );
    }
    tool::halo(&mut shapes, slot, face.lit(p::CARRIER), face.focus());

    // HOLD: the fence the carrier may wander inside, an octave and a
    // half either way. At nothing it is shut and the carrier is fixed.
    let fence = lay.hold;
    let reach = face.place(p::HOLD_RATE);
    let mid = fence.center();
    for side in [-1.0f32, 1.0] {
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(
                    mid.x + side * fence.width() * 0.5 * (0.06 + 0.44 * reach),
                    fence.top() + 2.0,
                ),
                egui::pos2(
                    mid.x + side * fence.width() * 0.5 * (0.06 + 0.44 * reach),
                    fence.bottom() - 2.0,
                ),
            ],
            Weight::Hair,
            tool::mix_ink(tool::fade(edge, 0.9), face.focus(), face.lit(p::HOLD_RATE)),
        );
    }
    circuit::pad(&mut shapes, mid, circuit::PAD - 1.0, face.live(), true);
    tool::halo(&mut shapes, fence, face.lit(p::HOLD_RATE), face.focus());

    // MIX: the balance between the bridge and the wire round it.
    tool::slider(
        &mut shapes,
        lay.mix.shrink2(egui::vec2(4.0, 4.0)),
        mix,
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::MIX)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.mix, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // The mark carries the burn: it is as lit as the diodes are.
    face.mark_signed(&lay, nav_cursor::Signature::Level(burn));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Ring), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Ring).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Ring),
                strip::plinth_rect(piece, SectionKind::Ring),
            ),
        )
    }

    #[test]
    fn every_ring_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Ring.table() {
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
        let _ = SectionParams::of(SectionKind::Ring);
    }

    /// The bridge stands over the window, and the window's period is
    /// the carrier's note: a higher note packs more cycles in.
    #[test]
    fn a_higher_carrier_packs_more_cycles_into_the_window() {
        let (_, lay) = laid();
        assert!(lay.bridge.bottom() <= lay.wave.top() + 0.01);
        let low = 1.0 + WINDOW_CYCLES * tool::norm(60.0, 20.0, 5_000.0);
        let high = 1.0 + WINDOW_CYCLES * tool::norm(4_000.0, 20.0, 5_000.0);
        assert!(high > low + 4.0, "the window did not change period");
        assert!(low >= 1.0, "the window lost its wave");
    }
}
