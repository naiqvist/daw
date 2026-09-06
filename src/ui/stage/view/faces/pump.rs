//! PUMP's face: the duck itself, drawn over a bar, and plainly not
//! connected to anything.
//!
//! The section is a rhythmic ducker: a gain that falls on the grid and
//! comes back. So the card draws that gain across one bar — the curve
//! repeating at the DIVISION, falling as far as the DEPTH, with the
//! SHAPE deciding whether it drops square or swings, and the HOLD
//! deciding how long it stays down. The beat grid is under it and the
//! transport's own position rides along it.
//!
//! It was a cam wheel before. The mechanism was honest and the profile
//! was the real one, but a lobed disc in a large empty bay does not read
//! as a duck to anyone who has not already been told it is one, and the
//! bay was mostly air. This is the same function drawn along the axis it
//! happens on, which is the axis DOOR's envelope and HIT's note are
//! already drawn along.
//!
//! # It is still a wire
//!
//! The section's DSP has not been written. Nothing on this card is
//! guesswork about what it would do — the curve is the very function a
//! core would run — but nothing is being done, so the curve is drawn as
//! a GHOST: dashed, dim, with NO CORE at the head and PASSING at the
//! foot. A card that drew a confident solid curve would be claiming an
//! effect that is not in the signal.

use super::*;
use crate::console::SectionParams;
use crate::params::console::pump as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The divisions the cam can be cut for, in lobes per bar.
const LOBES: [usize; 5] = [1, 2, 4, 8, 16];

struct Lay {
    /// The duck across a bar. The lobes ARE the division, so the plot is
    /// that parameter's own instrument.
    plot: egui::Rect,
    division: egui::Rect,
    div_read: egui::Rect,
    depth: egui::Rect,
    shape: egui::Rect,
    hold: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::DIVISION, self.division),
            (p::DEPTH, self.depth),
            (p::SHAPE, self.shape),
            (p::HOLD, self.hold),
        ]
    }
}

/// One readout row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;

fn lay(glass: egui::Rect, _bay: Option<egui::Rect>) -> Lay {
    // The glass is not the frame: it runs wider than the casing draws.
    let inner = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 4.0),
    );
    // From the foot up: four rows, then the plot takes the rest.
    let rows_h = ROW_H * 4.0 + 3.0;
    let rows_top = inner.bottom() - rows_h;
    let plot = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.right(), (rows_top - 5.0).max(inner.top() + 30.0)),
    );
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(inner.left(), rows_top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(inner.width(), ROW_H),
        )
    };
    Lay {
        plot,
        division: plot,
        div_read: row(0),
        depth: row(1),
        shape: row(2),
        hold: row(3),
    }
}

/// The cam's radius at angle `t` (turns), for a wheel of `lobes` cut to
/// `depth` with a profile from square to sinusoidal at `shape`, and a
/// dwell of `hold` at the top of each lobe.
fn lobe(t: f32, lobes: usize, depth: f32, shape: f32, hold: f32) -> f32 {
    let phase = (t * lobes as f32).fract();
    // The dwell: the lobe sits at its peak for this share of a turn.
    let dwell = hold.clamp(0.0, 0.9);
    let run = (1.0 - dwell).max(1e-3);
    let x = if phase < dwell {
        0.0
    } else {
        (phase - dwell) / run
    };
    // Square at one end of SHAPE, a raised cosine at the other.
    let soft = 0.5 - 0.5 * (x * core::f32::consts::TAU).cos();
    let hard = if x < 0.5 { 0.0 } else { 1.0 };
    let profile = egui::lerp(hard..=soft, shape.clamp(0.0, 1.0));
    1.0 - depth.clamp(0.0, 1.0) * (1.0 - profile)
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay());
    let (ink, edge) = (face.ink(), face.edge());
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let mut shapes = Vec::new();

    let step = face.value(p::DIVISION).round().clamp(0.0, 4.0) as usize;
    let lobes = LOBES[step];
    let depth = face.anim("depth", face.place(p::DEPTH), 0.10);
    let shape = face.anim("shape", face.place(p::SHAPE), 0.10);
    let hold = face.anim("hold", face.place(p::HOLD), 0.10) * 0.6;
    // The transport's own beat. There is no other clock on this card.
    let turn = face.phase.beat;

    // ---- The duck, across a bar. ------------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.plot,
        Some(face.alpha.ground.color),
        face.alpha.well.color,
        Some((Weight::Hair, tool::fade(edge, 0.62))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.plot.left() + 7.0, lay.plot.top() + font.size + 6.0),
        egui::pos2(lay.plot.right() - 7.0, lay.plot.bottom() - font.size - 5.0),
    );
    let x_at = |t: f32| field.left() + t.clamp(0.0, 1.0) * field.width();
    let y_at = |gain: f32| field.bottom() - gain.clamp(0.0, 1.0) * field.height();
    // The grid the duck is cut to: one cell per lobe, so the division is
    // legible as a rhythm and not only as a number.
    for i in 0..=lobes {
        let x = x_at(i as f32 / lobes as f32);
        chrome::trace(
            &mut shapes,
            &[egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
            Weight::Hair,
            tool::fade(edge, if i % lobes == 0 { 0.9 } else { 0.35 }),
        );
    }
    // Unity, and the floor the depth would reach.
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), y_at(1.0)),
            egui::pos2(field.right(), y_at(1.0)),
        ],
        Weight::Hair,
        tool::fade(edge, 0.8),
    );
    if depth > 0.01 {
        chrome::dashes(
            &mut shapes,
            &[
                egui::pos2(field.left(), y_at(1.0 - depth)),
                egui::pos2(field.right(), y_at(1.0 - depth)),
            ],
            0.0,
            Weight::Hair,
            tool::fade(edge, 0.6),
        );
    }
    // The curve, GHOSTED: this is what the section would do, and the
    // section is a wire, so it is drawn as a thing not happening.
    let duck: Vec<egui::Pos2> = (0..=160)
        .map(|i| {
            let t = i as f32 / 160.0;
            egui::pos2(x_at(t), y_at(lobe(t, lobes, depth, shape, hold)))
        })
        .collect();
    chrome::dashes(
        &mut shapes,
        &duck,
        0.0,
        Weight::Heavy,
        tool::fade(ink, 0.75),
    );
    // Where the transport stands, and what the duck would be there.
    let here = turn.fract();
    let now = lobe(here, lobes, depth, shape, hold);
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(x_at(here), field.top()),
            egui::pos2(x_at(here), field.bottom()),
        ],
        Weight::Hair,
        face.live(),
    );
    chrome::pad(
        &mut shapes,
        egui::pos2(x_at(here), y_at(now)),
        chrome::PAD,
        face.live(),
        true,
    );
    tool::halo(&mut shapes, lay.plot, face.lit(p::DIVISION), face.focus());

    // ---- The rows. --------------------------------------------------
    let span = |text: &str| {
        face.painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["DEPTH", "SHAPE", "HOLD", "DIV"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 8.0;
    let figure = span("100") + 8.0;
    let between = |rect: egui::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.top()),
            egui::pos2(rect.right() - figure, rect.bottom()),
        )
    };
    for (rect, param, value) in [(lay.depth, p::DEPTH, depth), (lay.shape, p::SHAPE, shape)] {
        tool::slider(
            &mut shapes,
            between(rect).shrink2(egui::vec2(0.0, 5.0)),
            value,
            9,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }
    tool::beat_grid(
        &mut shapes,
        between(lay.hold).shrink2(egui::vec2(0.0, 4.0)),
        lobes.min(8),
        1.0 - hold / 0.6,
        Some(((turn * lobes as f32) as usize).min(lobes.saturating_sub(1))),
        tool::mix_ink(ink, face.focus(), face.lit(p::HOLD)),
        tool::fade(edge, 0.5),
    );
    tool::halo(&mut shapes, lay.hold, face.lit(p::HOLD), face.focus());
    face.painter.extend(shapes);

    // ---- The words. -------------------------------------------------
    let mut words = tool::Ledger::new(face.painter, font.clone(), "PUMP");
    words.text(
        egui::pos2(lay.plot.left() + 8.0, lay.plot.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "DUCK",
        edge,
    );
    // The condition that matters more than any setting on the card.
    words.text(
        egui::pos2(lay.plot.right() - 8.0, lay.plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        "NO CORE",
        face.focus(),
    );
    words.text(
        egui::pos2(field.left(), lay.plot.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        "PASSING",
        tool::fade(edge, 1.1),
    );
    words.text(
        egui::pos2(field.right(), lay.plot.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        "1 BAR",
        edge,
    );
    for (rect, word, said) in [
        (lay.div_read, "DIV", format!("1/{lobes}")),
        (lay.depth, "DEPTH", format!("{:.0}", face.value(p::DEPTH))),
        (lay.shape, "SHAPE", format!("{:.0}", face.value(p::SHAPE))),
        (lay.hold, "HOLD", format!("{:.0}", face.value(p::HOLD))),
    ] {
        words.text(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }
    words.finish();

    // The mark stands on the grid the duck is cut to.
    face.mark_signed(&lay, face.beat_cell(lobes.min(8)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Pump), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Pump).shrink(3.0);
        (piece, lay(glass, strip::bay_rect(piece, SectionKind::Pump)))
    }

    #[test]
    fn every_pump_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Pump.table() {
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
        let _ = SectionParams::of(SectionKind::Pump);
    }

    /// The cam's profile is the duck it would apply: cut to depth, held
    /// at the top for the dwell, and square or soft with the shape.
    #[test]
    fn the_lobe_is_the_duck_the_section_would_apply() {
        // No depth is a round wheel: nothing ducks.
        for i in 0..8 {
            let t = i as f32 / 8.0;
            assert!((lobe(t, 4, 0.0, 0.5, 0.0) - 1.0).abs() < 1e-5);
        }
        // Full depth reaches the floor at the start of a lobe and comes
        // back to the top by its end.
        assert!(lobe(0.0, 1, 1.0, 1.0, 0.0) < 0.05, "the lobe never bit");
        assert!(lobe(0.5, 1, 1.0, 1.0, 0.0) > 0.95, "the lobe never let go");
        // The dwell holds the floor: at a long hold the first part of
        // every turn is flat.
        let held = lobe(0.1, 1, 1.0, 1.0, 0.5);
        assert!(held < 0.05, "the dwell did not hold: {held}");
        // And it never leaves its own range, at any setting.
        for lobes in [1usize, 2, 4, 8, 16] {
            for i in 0..24 {
                let v = lobe(i as f32 / 24.0, lobes, 1.0, 0.3, 0.4);
                assert!((0.0..=1.0).contains(&v), "the cam left its range: {v}");
            }
        }
    }

    /// The duck repeats on the division, and it falls ON the beat and
    /// recovers between beats — which is the way round a pump works and
    /// the opposite of what a first reading of the profile suggests.
    ///
    /// The lobe is not a dip in an otherwise open gain. It sits at the
    /// FLOOR as each beat lands, then climbs. So the thing to hold is
    /// that every beat bites and every gap recovers.
    #[test]
    fn the_duck_falls_on_the_beat_and_recovers_between() {
        for lobes in [1usize, 2, 4, 8, 16] {
            let (depth, shape, hold) = (0.8, 0.5, 0.2);
            for i in 0..lobes {
                let beat = i as f32 / lobes as f32;
                assert!(
                    lobe(beat, lobes, depth, shape, hold) < 1.0 - depth * 0.9,
                    "beat {i} of {lobes} did not duck"
                );
                // Somewhere before the next beat it comes back up.
                let recovered = (1..20).any(|k| {
                    let t = beat + (k as f32 / 20.0) / lobes as f32;
                    lobe(t, lobes, depth, shape, hold) > 1.0 - depth * 0.25
                });
                assert!(recovered, "the gain never recovered after beat {i}");
            }
        }
    }
}
