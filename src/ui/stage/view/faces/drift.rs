//! DRIFT's face: a bucket brigade with two heads sweeping it.
//!
//! After the bucket-brigade delay chip the section models. The line of
//! buckets runs across the glass, and two read heads ride it — one per
//! side. Where each head stands is not a picture of an LFO: it is the
//! LFO, the section's own sweep at the block's last sample, so the
//! heads walk at the rate that is actually running and stop dead when
//! the section is a wire.
//!
//! DEPTH is how far apart the end stops are; WIDTH is how far the two
//! heads are pushed apart inside them, so a wide setting is visibly two
//! machines and a narrow one is visibly one.

use super::*;
use crate::console::SectionParams;
use crate::params::console::drift as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// How many buckets the line is drawn with.
const BUCKETS: usize = 22;

struct Lay {
    /// The line of buckets the heads ride.
    line: egui::Rect,
    mode: egui::Rect,
    rate: egui::Rect,
    depth: egui::Rect,
    feedback: egui::Rect,
    width: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::MODE, self.mode),
            (p::RATE, self.rate),
            (p::DEPTH, self.depth),
            (p::FEEDBACK, self.feedback),
            (p::WIDTH, self.width),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let line = egui::Rect::from_min_max(
        egui::pos2(inner.left(), inner.top() + 0.14 * h),
        egui::pos2(inner.right(), inner.top() + 0.44 * h),
    );
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 14.0),
        )
    };
    let first = line.bottom() + 22.0;
    Lay {
        line,
        // The DEPTH is the stops the heads run between: the line itself.
        depth: line,
        mode: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 22.0, inner.top()),
                egui::pos2(inner.right(), inner.top() + 24.0),
            )
        }),
        rate: row(first, 0.0, 0.46),
        feedback: row(first, 0.54, 1.0),
        width: row(first + 19.0, 0.0, 0.46),
        mix: plinth.unwrap_or_else(|| row(first + 19.0, 0.54, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let depth = face.anim("depth", face.place(p::DEPTH), 0.10);
    let width = face.anim("width", face.place(p::WIDTH), 0.10);
    let mix = face.place(p::MIX);

    // THE LINE: the buckets, each one a cell the charge is walked
    // through. They dim toward the far end, as a bucket brigade does.
    let line = lay.line;
    let pitch = line.width() / BUCKETS as f32;
    for i in 0..BUCKETS {
        let t = (i as f32 + 0.5) / BUCKETS as f32;
        let cell = egui::Rect::from_min_max(
            egui::pos2(line.left() + pitch * i as f32 + 1.0, line.center().y - 5.0),
            egui::pos2(
                line.left() + pitch * (i as f32 + 1.0) - 1.0,
                line.center().y + 5.0,
            ),
        );
        if cell.is_positive() {
            chrome::trace(
                &mut shapes,
                &[
                    cell.left_top(),
                    cell.right_top(),
                    cell.right_bottom(),
                    cell.left_bottom(),
                    cell.left_top(),
                ],
                Weight::Hair,
                tool::fade(edge, 0.65 - 0.35 * t),
            );
        }
    }

    // THE END STOPS: how far of the line the heads may use. Depth is
    // the travel, so at nothing the two stops meet and nothing moves.
    let reach = line.width() * 0.5 * (0.06 + 0.94 * depth);
    let middle = line.center().x;
    for stop in [middle - reach, middle + reach] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(stop, line.top() - 4.0),
                egui::pos2(stop, line.bottom() + 4.0),
            ],
            Weight::Hair,
            tool::mix_ink(tool::fade(edge, 0.9), face.focus(), face.lit(p::DEPTH)),
        );
    }
    tool::halo(&mut shapes, lay.line, face.lit(p::DEPTH), face.focus());

    // THE TWO HEADS: the section's own sweeps, one per side, pushed
    // apart by the width. Nothing here runs on a clock; if the section
    // is flat the sweeps are zero and the heads stand still.
    let spread = 0.15 + 0.85 * width;
    for (side, hue) in [(0usize, ink), (1, face.live())] {
        let sweep = face
            .anim(
                if side == 0 { "sweep-l" } else { "sweep-r" },
                said.bands[side + 1].clamp(-1.0, 1.0),
                0.03,
            )
            .clamp(-1.0, 1.0);
        let at = middle + reach * sweep * if side == 0 { spread } else { -spread };
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(at, line.top() - 6.0),
                egui::pos2(at, line.bottom() + 6.0),
            ],
            Weight::Heavy,
            hue,
        );
        chrome::pad(
            &mut shapes,
            egui::pos2(at, line.top() - 6.0),
            chrome::PAD - 1.0,
            hue,
            true,
        );
    }
    tool::halo(&mut shapes, lay.width, face.lit(p::WIDTH), face.focus());

    // MODE: four detents in the bay, a tall slot in the right wall.
    let slot = lay.mode;
    let mode = face.value(p::MODE).round().clamp(0.0, 3.0) as usize;
    for i in 0..4 {
        let y = egui::lerp((slot.top() + 4.0)..=(slot.bottom() - 4.0), i as f32 / 3.0);
        let here = i == mode;
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
    tool::halo(&mut shapes, slot, face.lit(p::MODE), face.focus());

    // RATE: a gauge, but the thing that says the rate is the heads. So
    // the gauge is small and the head is the reading.
    tool::slider(
        &mut shapes,
        lay.rate.shrink2(egui::vec2(4.0, 4.0)),
        face.place(p::RATE),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::RATE)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.rate, face.lit(p::RATE), face.focus());

    // FEEDBACK: an arc from the line's far end back to its head, above
    // for a positive loop and below for an inverted one — which is what
    // the sign of a bucket brigade's feedback actually does.
    let fb = face.swing(p::FEEDBACK);
    let arc_rect = lay.feedback;
    let lift = arc_rect.height() * 0.5 * fb;
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(arc_rect.right() - 2.0, arc_rect.center().y),
            egui::pos2(arc_rect.center().x, arc_rect.center().y - lift),
            egui::pos2(arc_rect.left() + 2.0, arc_rect.center().y),
        ],
        Weight::Hair,
        tool::mix_ink(tool::fade(edge, 0.8), ink, fb.abs()),
    );
    chrome::pad(
        &mut shapes,
        egui::pos2(arc_rect.left() + 2.0, arc_rect.center().y),
        chrome::PAD - 2.0,
        ink,
        fb.abs() > 0.01,
    );
    tool::halo(&mut shapes, arc_rect, face.lit(p::FEEDBACK), face.focus());

    // WIDTH and MIX, two rails at the foot.
    for (rect, param, value) in [(lay.width, p::WIDTH, width), (lay.mix, p::MIX, mix)] {
        tool::slider(
            &mut shapes,
            rect.shrink2(egui::vec2(4.0, 4.0)),
            value,
            9,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    face.painter.extend(shapes);

    // The mark rides the sweep that is actually running.
    face.mark_signed(
        &lay,
        nav_cursor::Signature::Sweep(said.bands[1].clamp(-1.0, 1.0)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Drift), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Drift).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Drift),
                strip::plinth_rect(piece, SectionKind::Drift),
            ),
        )
    }

    #[test]
    fn every_drift_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Drift.table() {
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
        let _ = SectionParams::of(SectionKind::Drift);
    }

    /// The two heads run between the stops and never leave them, at any
    /// width and any point of the sweep.
    #[test]
    fn the_heads_stay_between_their_stops() {
        let (_, lay) = laid();
        let middle = lay.line.center().x;
        for depth in [0.0f32, 0.5, 1.0] {
            let reach = lay.line.width() * 0.5 * (0.06 + 0.94 * depth);
            for width in [0.0f32, 1.0] {
                let spread = 0.15 + 0.85 * width;
                for sweep in [-1.0f32, 0.0, 1.0] {
                    for side in [1.0f32, -1.0] {
                        let at = middle + reach * sweep * spread * side;
                        assert!(
                            at >= middle - reach - 0.01 && at <= middle + reach + 0.01,
                            "a head left its stops"
                        );
                    }
                }
            }
        }
    }
}
