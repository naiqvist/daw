//! PHASE's face: the teeth, and the ring that walks them.
//!
//! This card is the dark one on the strip, and it is dark because of
//! what it is rather than for effect. A phaser is the one section here
//! that does nothing you can point at: the allpass run changes no
//! magnitude whatever, and the notches exist only because the swept run
//! and the dry signal disagree. Nothing is being removed. Something is
//! cancelling.
//!
//! So the card is drawn on the well rather than the ground — a step
//! darker than every other section — and what is IN it is teeth: the
//! disagreement computed from the run's own phase and dropped into the
//! spectrum, cutting downward. They are not placed. They fall where the
//! arithmetic puts them, and the count that comes out is the stage count
//! halved, which is the thing the pedals were counting.
//!
//! # The ring
//!
//! The sweep is a turn, so it is drawn as one: a ring with a node for
//! every allpass section in the run, and two marks on its rim for where
//! the left and right sweeps stand. OFFSET is the angle between them —
//! up to half a turn — so the control that makes a phaser into a stereo
//! swirl is an angle you can see rather than a number to imagine.
//!
//! FEEDBACK closes an inner ring. It takes the last section's output
//! back to the first and sharpens the notches into resonances, and the
//! teeth on the left get longer as it does, because the response really
//! does.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
const COLUMN_GAP: f32 = 8.0;
/// The spectrum's window.
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;
/// How far down the teeth are drawn before they are simply "gone".
const FLOOR_DB: f32 = -28.0;
/// And how far up, for a fed-back phaser's resonances.
const CEILING_DB: f32 = 10.0;

/// PHASE's five controls.
#[derive(Clone, Copy, Debug)]
struct PhaseFace {
    teeth: egui::Rect,
    ring: egui::Rect,
    stages: egui::Rect,
    rate: egui::Rect,
    depth: egui::Rect,
    feedback: egui::Rect,
    offset: egui::Rect,
}

impl Layout for PhaseFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::phase as p;
        vec![
            (p::STAGES, self.stages),
            (p::RATE, self.rate),
            (p::DEPTH, self.depth),
            (p::FEEDBACK, self.feedback),
            (p::OFFSET, self.offset),
        ]
    }
}

fn phase_face(glass: egui::Rect) -> PhaseFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let teeth_w = (x.width() - COLUMN_GAP) * 0.50;
    let teeth = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + teeth_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(teeth.right() + COLUMN_GAP, x.top()), x.max);
    let rows_h = ROW_H * 4.0 + 3.0;
    let rows_top = right.bottom() - rows_h;
    let ring = egui::Rect::from_min_max(
        right.min,
        egui::pos2(right.right(), (rows_top - 5.0).max(right.top() + 30.0)),
    );
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), rows_top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    PhaseFace {
        teeth,
        ring,
        // The ring's nodes ARE the run, so the ring is that control's
        // own instrument and takes no row of its own — which is also
        // what gives it the height to be a ring rather than a dot.
        stages: ring,
        rate: row(0),
        depth: row(1),
        feedback: row(2),
        offset: row(3),
    }
}

/// Where a frequency stands across the spectrum, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::phase_curve as pc;
    use crate::params::console::phase as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = phase_face(face.glass);
    let step = face.value(p::STAGES).round().clamp(0.0, 5.0) as usize;
    let stages = p::STAGE_COUNTS[step];
    let rate = face.value(p::RATE);
    let depth = face.value(p::DEPTH) / 100.0;
    let feedback = face.value(p::FEEDBACK) / 100.0;
    let offset = face.value(p::OFFSET);
    let wire = depth <= 0.0 && feedback == 0.0;
    // Where each side's sweep stood at the end of the block.
    let swept = [face.said.bands[0], face.said.bands[1]];
    let corner = pc::corner_at(swept[0], depth);

    let mut shapes = Vec::new();

    // ---- The teeth. A step darker than any other card on the strip. --
    chrome::panel_variant(
        &mut shapes,
        lay.teeth,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge.gamma_multiply(0.5))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.teeth.left() + 8.0, lay.teeth.top() + font.size + 6.0),
        egui::pos2(
            lay.teeth.right() - 8.0,
            lay.teeth.bottom() - font.size - 6.0,
        ),
    );
    let x_at = |hz: f32| field.left() + place_of_hz(hz) * field.width();
    let y_at = |db: f32| {
        field.bottom()
            - ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0) * field.height()
    };
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), y_at(0.0)),
            egui::pos2(field.right(), y_at(0.0)),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.8),
    );
    for hz in [100.0f32, 1_000.0, 10_000.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x_at(hz), field.top()),
                egui::pos2(x_at(hz), field.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.22),
        );
    }
    // The sweep's whole reach, so the teeth's walk has a bed.
    if depth > 0.0 {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x_at(pc::corner_at(-1.0, depth)), field.top()),
                egui::pos2(x_at(pc::corner_at(1.0, depth)), field.bottom()),
            ),
            0.0,
            alpha.live_dim.color.gamma_multiply(0.16),
        ));
    }
    // The response itself: nothing is removed, something cancels, and
    // this is where.
    let curve: Vec<egui::Pos2> = (0..=240)
        .map(|i| {
            let t = i as f32 / 240.0;
            let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
            egui::pos2(
                field.left() + t * field.width(),
                y_at(pc::response_db(stages, corner, feedback, hz)),
            )
        })
        .collect();
    chrome::curve(
        &mut shapes,
        &curve,
        Weight::Heavy,
        if wire {
            edge.gamma_multiply(0.85)
        } else {
            tool::mix_ink(ink, alpha.jeopardy_active.color, feedback.abs())
        },
    );

    // ---- The ring: the sweep is a turn, so it is drawn as one. -------
    chrome::panel_variant(
        &mut shapes,
        lay.ring,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge.gamma_multiply(0.5))),
        0,
    );
    let dial = egui::Rect::from_min_max(
        egui::pos2(lay.ring.left() + 6.0, lay.ring.top() + font.size + 4.0),
        egui::pos2(lay.ring.right() - 6.0, lay.ring.bottom() - 5.0),
    );
    let centre = dial.center();
    let radius = (dial.width().min(dial.height()) * 0.42).max(6.0);
    let on_ring = |turn: f32, r: f32| {
        let a = (turn - 0.25) * core::f32::consts::TAU;
        egui::pos2(centre.x + r * a.cos(), centre.y + r * a.sin())
    };
    chrome::curve(
        &mut shapes,
        &(0..=72)
            .map(|i| on_ring(i as f32 / 72.0, radius))
            .collect::<Vec<_>>(),
        Weight::Hair,
        edge.gamma_multiply(1.2),
    );
    // A node for every section in the run. The ring's points ARE the
    // stage count, which is the thing the pedals were counting.
    for i in 0..stages {
        chrome::pad(
            &mut shapes,
            on_ring(i as f32 / stages as f32, radius),
            chrome::PAD - 1.0,
            if wire { edge } else { ink },
            !wire,
        );
    }
    // Feedback closes an inner ring, and the deeper it is the closer it
    // comes to the nodes it feeds.
    if feedback != 0.0 {
        let inner = radius * (0.20 + 0.55 * feedback.abs());
        chrome::curve(
            &mut shapes,
            &(0..=48)
                .map(|i| on_ring(i as f32 / 48.0, inner))
                .collect::<Vec<_>>(),
            Weight::Hair,
            alpha.jeopardy_active.color,
        );
    }
    // The two sides on the rim, and the chord between them: OFFSET is
    // that angle and nothing else.
    if !wire {
        let turn_of = |sweep: f32| (sweep.clamp(-1.0, 1.0) + 1.0) * 0.5;
        let (a, b) = (turn_of(swept[0]), turn_of(swept[1]));
        let (pa, pb) = (on_ring(a, radius), on_ring(b, radius));
        chrome::trace(&mut shapes, &[pa, pb], Weight::Hair, alpha.live_dim.color);
        chrome::pad(&mut shapes, pa, chrome::PAD + 1.0, alpha.live.color, true);
        chrome::pad(
            &mut shapes,
            pb,
            chrome::PAD + 1.0,
            alpha.jeopardy_latent.color,
            true,
        );
        chrome::trace(&mut shapes, &[centre, pa], Weight::Hair, alpha.live.color);
    }
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "PHASE");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["RATE", "DEPTH", "FEEDBACK", "OFFSET"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span("-100") + 6.0;
    words.text(
        egui::pos2(lay.teeth.left() + 8.0, lay.teeth.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "NULLS",
        edge,
    );
    words.text(
        egui::pos2(lay.teeth.right() - 8.0, lay.teeth.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else {
            format!("{}", pc::notches(stages))
        },
        if wire { edge } else { alpha.live.color },
    );
    for (hz, word) in [(100.0f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        words.text(
            egui::pos2(x_at(hz), lay.teeth.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            word,
            edge,
        );
    }
    words.text(
        egui::pos2(lay.ring.left() + 7.0, lay.ring.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "RUN",
        edge,
    );
    words.text(
        egui::pos2(lay.ring.right() - 7.0, lay.ring.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{stages}"),
        ink,
    );
    for (rect, word, share, said, tone) in [
        (
            lay.rate,
            "RATE",
            (rate / 10.0).clamp(0.0, 1.0),
            format!("{rate:.2}"),
            alpha.live.color,
        ),
        (
            lay.depth,
            "DEPTH",
            depth,
            format!("{:.0}", depth * 100.0),
            alpha.live.color,
        ),
        (
            lay.feedback,
            "FEEDBACK",
            feedback.abs(),
            format!("{:+.0}", feedback * 100.0),
            alpha.jeopardy_active.color,
        ),
        (
            lay.offset,
            "OFFSET",
            offset / 180.0,
            format!("{offset:.0}"),
            alpha.jeopardy_latent.color,
        ),
    ] {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.5),
            egui::pos2(rect.right() - figure, rect.center().y + 2.5),
        );
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.3));
            if share > 0.0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(
                            bar.left() + share.clamp(0.0, 1.0) * bar.width(),
                            bar.bottom(),
                        ),
                    ),
                    0.0,
                    tone,
                );
            }
        }
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

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(470.0, 210.0))
    }

    #[test]
    fn every_phase_control_has_its_own_place() {
        let face = phase_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 5);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its place");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "controls {id} and {other} overlap"
                );
            }
        }
        assert!(!face.teeth.intersects(face.ring));
        // The ring is the stages' instrument, not a separate picture.
        assert_eq!(face.stages, face.ring);
    }

    /// The notches are not placed — they are what the arithmetic does.
    /// So the thing to hold is that the arithmetic really produces them,
    /// and produces as many as the stage count says it should.
    #[test]
    fn the_teeth_fall_where_the_phase_puts_them() {
        use crate::console::phase_curve as pc;
        use crate::params::console::phase as p;
        let corner = 1_000.0;
        for stages in p::STAGE_COUNTS {
            // Count the dips across the audible band.
            let sweep: Vec<f32> = (0..=2_000)
                .map(|i| {
                    let t = i as f32 / 2_000.0;
                    pc::response_db(stages, corner, 0.0, 20.0 * 1000f32.powf(t))
                })
                .collect();
            let dips = sweep
                .windows(3)
                .filter(|w| w[1] < w[0] && w[1] < w[2] && w[1] < -6.0)
                .count() as u32;
            assert_eq!(
                dips,
                pc::notches(stages),
                "{stages} stages made {dips} notches"
            );
        }
    }

    /// Nothing is removed anywhere: at no stages the section is exactly
    /// the wire, at any stage count the response passes through zero,
    /// and feedback deepens rather than shifts.
    #[test]
    fn the_run_itself_changes_no_magnitude() {
        use crate::console::phase_curve as pc;
        for hz in [50.0f32, 500.0, 5_000.0] {
            assert_eq!(pc::response_db(0, 1_000.0, 0.0, hz), 0.0);
        }
        // A run with no feedback tops out at unity: adding an allpass to
        // the dry can double it at most, and the halving puts that at 0.
        for stages in [2u32, 8, 16] {
            for i in 0..200 {
                let hz = 20.0 * 1000f32.powf(i as f32 / 200.0);
                assert!(
                    pc::response_db(stages, 1_000.0, 0.0, hz) <= 0.01,
                    "the run made gain at {hz}"
                );
            }
        }
        // Feedback does NOT deepen the nulls — it makes RESONANCES. It
        // takes the run's output back to its input, so at a null the sum
        // is k/(1+k) rather than nothing at all: the null gets
        // shallower, and peaks appear where it comes back in phase.
        // "Sharpens the notches into resonances" means the second thing,
        // and the first version of this test asserted the first.
        let sweep = |k: f32| -> (f32, f32) {
            (0..2_000)
                .map(|i| pc::response_db(8, 1_000.0, k, 20.0 * 1000f32.powf(i as f32 / 2_000.0)))
                .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (dry_floor, dry_top) = sweep(0.0);
        let (fed_floor, fed_top) = sweep(0.7);
        assert!(dry_top <= 0.01, "the run made gain with no feedback");
        assert!(fed_top > 3.0, "feedback made no resonance: {fed_top}");
        assert!(
            fed_floor > dry_floor,
            "feedback should shallow the null, not deepen it"
        );
    }

    /// The sweep walks the corner in octaves between the section's own
    /// ends, and stands still in the middle at no depth.
    #[test]
    fn the_corner_walks_between_the_ends() {
        use crate::console::phase_curve as pc;
        use crate::params::console::phase as p;
        assert!((pc::corner_at(-1.0, 1.0) - p::LOW_HZ).abs() < 1.0);
        assert!((pc::corner_at(1.0, 1.0) - p::HIGH_HZ).abs() < 1.0);
        // No depth is no walk, wherever the sweep is.
        let still = pc::corner_at(0.0, 0.0);
        for sweep in [-1.0f32, -0.3, 0.5, 1.0] {
            assert!((pc::corner_at(sweep, 0.0) - still).abs() < 0.01);
        }
        // And it is a walk in octaves, not in hertz: half the sweep is
        // half the octaves, so the midpoint is the geometric mean.
        let mid = pc::corner_at(0.0, 1.0);
        assert!((mid - (p::LOW_HZ * p::HIGH_HZ).sqrt()).abs() < 1.0);
    }
}
