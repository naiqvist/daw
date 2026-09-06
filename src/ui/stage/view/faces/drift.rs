//! DRIFT's face: the line, and where its sweep has got to.
//!
//! One kind of delay line and four ways of sweeping it, so the card is a
//! picture of the line: a lane in milliseconds with the mode's own delay
//! marked on it, the reach the sweep covers shaded either side, and the
//! two sides' sweeps riding it where they actually are this instant.
//!
//! Those two markers come from the section's own readout, not from the
//! knobs, and that is what makes WIDTH legible: at nothing the two sit
//! on top of each other, and as it opens they part. A number could tell
//! you the width; only two markers can show you that the sides are on
//! opposite errands.
//!
//! The SWEEP panel draws the shape the mode uses — a triangle for the
//! chorus and the flanger, as the CE-1's was, a sine for the vibrato and
//! the ensemble. It is drawn as the nominal shape and said to be
//! nominal, because the real one is not a metronome: a slow random walk
//! wobbles the rate and the depth by a few percent so two bars never
//! sweep alike, and a card that drew a perfect triangle and left it at
//! that would be promising a steadiness the section does not have.
//!
//! TOP is the one piece of bucket-brigade physics worth a number: the
//! line is band-limited by its own clock, and it is duller the longer it
//! is. The figure moves as the sweep moves, because the line really is
//! getting darker and brighter as it sweeps.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
const COLUMN_GAP: f32 = 8.0;
/// The four modes, in the order the parameter numbers them.
const MODES: [&str; 4] = ["CHORUS", "FLANGER", "VIBRATO", "ENSEMBLE"];

/// DRIFT's six controls.
#[derive(Clone, Copy, Debug)]
struct DriftFace {
    mode: egui::Rect,
    lane: egui::Rect,
    sweep: egui::Rect,
    rate: egui::Rect,
    depth: egui::Rect,
    feedback: egui::Rect,
    width: egui::Rect,
    mix: egui::Rect,
}

impl Layout for DriftFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::drift as p;
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

fn drift_face(glass: egui::Rect) -> DriftFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let mode = egui::Rect::from_min_size(x.min, egui::vec2(x.width(), ROW_H));
    let body = egui::Rect::from_min_max(egui::pos2(x.left(), mode.bottom() + 4.0), x.max);
    let left_w = (body.width() - COLUMN_GAP) * 0.54;
    let left = egui::Rect::from_min_max(body.min, egui::pos2(body.left() + left_w, body.bottom()));
    let right =
        egui::Rect::from_min_max(egui::pos2(left.right() + COLUMN_GAP, body.top()), body.max);
    // The lane takes the head of the left column and the sweep its foot.
    let sweep_h = (left.height() * 0.42).clamp(38.0, 80.0);
    let lane = egui::Rect::from_min_max(
        left.min,
        egui::pos2(left.right(), left.bottom() - sweep_h - 4.0),
    );
    let sweep = egui::Rect::from_min_max(egui::pos2(left.left(), lane.bottom() + 4.0), left.max);
    let rows_h = ROW_H * 5.0 + 4.0;
    let top = (right.bottom() - rows_h).max(right.top());
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    DriftFace {
        mode,
        lane,
        sweep,
        rate: row(0),
        depth: row(1),
        feedback: row(2),
        width: row(3),
        mix: row(4),
    }
}

/// The line's top, in Hz, at a delay of `ms`.
///
/// A bucket brigade is band-limited by its own clock's anti-alias
/// filters, and the longer the line the slower the clock: the same
/// section is duller at seven milliseconds than at one.
fn top_hz(ms: f32) -> f32 {
    use crate::params::console::drift as p;
    let t = (ms / p::MAX_MS).clamp(0.0, 1.0);
    egui::lerp(p::BBD_SHORT_HZ..=p::BBD_LONG_HZ, t)
}

/// The nominal sweep at `turn` of a cycle: a triangle for the modes that
/// used one, a sine for the modes that used one. −1..1.
fn sweep_at(mode: u32, turn: f32) -> f32 {
    use crate::params::console::drift as p;
    let t = turn.rem_euclid(1.0);
    if mode == p::CHORUS || mode == p::FLANGER {
        // A triangle: up for half a turn, down for the other half.
        if t < 0.5 {
            -1.0 + 4.0 * t
        } else {
            3.0 - 4.0 * t
        }
    } else {
        (t * core::f32::consts::TAU).sin()
    }
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::drift as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = drift_face(face.glass);
    let mode = face.value(p::MODE).round().clamp(0.0, 3.0) as u32;
    let rate = face.value(p::RATE);
    let depth = face.value(p::DEPTH) / 100.0;
    let feedback = face.value(p::FEEDBACK) / 100.0;
    let width = face.value(p::WIDTH) / 100.0;
    let mix = face.value(p::MIX) / 100.0;
    let wire = mix <= 0.0;
    // Where each side's sweep actually stood at the end of the block.
    let swept = [face.said.bands[1], face.said.bands[2]];
    let base = p::BASE_MS[mode as usize];
    let reach = p::SWEEP_MS[mode as usize] * depth;
    let delay = |side: usize| (base + swept[side] * reach).clamp(0.0, p::MAX_MS);

    let mut shapes = Vec::new();

    // ---- The mode strip. ---------------------------------------------
    chrome::panel_frame_variant(&mut shapes, lay.mode, Weight::Hair, edge, 2);
    let cell_w = lay.mode.width() / MODES.len() as f32;
    let mode_cell = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(lay.mode.left() + i as f32 * cell_w, lay.mode.top()),
            egui::vec2(cell_w, lay.mode.height()),
        )
    };
    chrome::brackets(
        &mut shapes,
        mode_cell(mode as usize).shrink(2.0),
        4.0,
        Weight::Hair,
        ink,
    );

    // ---- The lane: the line, in milliseconds. ------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.lane,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.lane.left() + 8.0, lay.lane.top() + font.size + 6.0),
        egui::pos2(lay.lane.right() - 8.0, lay.lane.bottom() - font.size - 6.0),
    );
    let x_at = |ms: f32| field.left() + (ms / p::MAX_MS).clamp(0.0, 1.0) * field.width();
    // The reach: everywhere the sweep can put the line.
    if reach > 0.0 {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x_at(base - reach), field.top()),
                egui::pos2(x_at(base + reach), field.bottom()),
            ),
            0.0,
            alpha.live_dim.color.gamma_multiply(0.35),
        ));
    }
    // The line the mode sits on with no sweep at all.
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(x_at(base), field.top()),
            egui::pos2(x_at(base), field.bottom()),
        ],
        Weight::Hair,
        edge.gamma_multiply(1.1),
    );
    for ms in [0.0f32, 4.0, 8.0, 12.0, 16.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x_at(ms), field.bottom() - 3.0),
                egui::pos2(x_at(ms), field.bottom()),
            ],
            Weight::Hair,
            edge,
        );
    }
    // The two sides, where they are. At no width they sit on each other;
    // as it opens they part, which is the whole of what width does.
    if !wire {
        for (side, tone) in [(0usize, alpha.live.color), (1, alpha.jeopardy_latent.color)] {
            let x = x_at(delay(side));
            let y = if side == 0 {
                field.top() + field.height() * 0.32
            } else {
                field.top() + field.height() * 0.68
            };
            chrome::trace(
                &mut shapes,
                &[egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
                Weight::Heavy,
                tone,
            );
            chrome::pad(&mut shapes, egui::pos2(x, y), chrome::PAD + 1.0, tone, true);
        }
    }

    // ---- The sweep's own shape. --------------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.sweep,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let wave = egui::Rect::from_min_max(
        egui::pos2(lay.sweep.left() + 8.0, lay.sweep.top() + font.size + 5.0),
        egui::pos2(lay.sweep.right() - 8.0, lay.sweep.bottom() - 5.0),
    );
    if wave.is_positive() {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(wave.left(), wave.center().y),
                egui::pos2(wave.right(), wave.center().y),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.5),
        );
        let shape: Vec<egui::Pos2> = (0..=96)
            .map(|i| {
                let t = i as f32 / 96.0;
                egui::pos2(
                    wave.left() + t * wave.width(),
                    wave.center().y - sweep_at(mode, t * 2.0) * depth * wave.height() * 0.44,
                )
            })
            .collect();
        chrome::curve(
            &mut shapes,
            &shape,
            Weight::Heavy,
            if wire { edge.gamma_multiply(0.9) } else { ink },
        );
    }
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "DRIFT");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["RATE", "DEPTH", "FEEDBACK", "WIDTH", "MIX"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span("-100") + 6.0;
    for (i, word) in MODES.into_iter().enumerate() {
        words.text(
            mode_cell(i).center(),
            egui::Align2::CENTER_CENTER,
            word,
            if i as u32 == mode { ink } else { edge },
        );
    }
    words.text(
        egui::pos2(lay.lane.left() + 8.0, lay.lane.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "LINE",
        edge,
    );
    // The line's own top, at the delay it is actually sitting at: the
    // brigade is duller the longer it runs.
    words.text(
        egui::pos2(lay.lane.right() - 8.0, lay.lane.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else {
            format!("{base:.0}ms · top {:.1}k", top_hz(delay(0)) / 1000.0)
        },
        if wire { edge } else { alpha.live.color },
    );
    words.text(
        egui::pos2(field.left(), lay.lane.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        "0",
        edge,
    );
    words.text(
        egui::pos2(field.right(), lay.lane.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{:.0}", p::MAX_MS),
        edge,
    );
    words.text(
        egui::pos2(lay.sweep.left() + 8.0, lay.sweep.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "SWEEP",
        edge,
    );
    // Nominal, and said to be: the real sweep wanders by a few percent.
    words.text(
        egui::pos2(lay.sweep.right() - 8.0, lay.sweep.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{rate:.2}Hz NOM"),
        edge,
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
            alpha.jeopardy_latent.color,
        ),
        (
            lay.width,
            "WIDTH",
            width,
            format!("{:.0}", width * 100.0),
            alpha.live.color,
        ),
        (
            lay.mix,
            "MIX",
            mix,
            format!("{:.0}", mix * 100.0),
            alpha.live.color,
        ),
    ] {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.5),
            egui::pos2(rect.right() - figure, rect.center().y + 2.5),
        );
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.35));
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
    fn every_drift_control_has_its_own_place() {
        let face = drift_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 6);
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
        assert!(!face.lane.intersects(face.sweep));
    }

    /// The line is duller the longer it is, which is the brigade's own
    /// physics and the reason the figure moves as the sweep moves.
    #[test]
    fn a_longer_line_is_a_darker_line() {
        use crate::params::console::drift as p;
        assert!((top_hz(0.0) - p::BBD_SHORT_HZ).abs() < 1.0);
        assert!((top_hz(p::MAX_MS) - p::BBD_LONG_HZ).abs() < 1.0);
        let mut last = f32::MAX;
        for i in 0..=16 {
            let hz = top_hz(i as f32);
            assert!(hz <= last, "the line got brighter as it got longer");
            last = hz;
        }
        // Past the ends it holds rather than running away.
        assert_eq!(top_hz(-5.0), p::BBD_SHORT_HZ);
        assert_eq!(top_hz(p::MAX_MS * 3.0), p::BBD_LONG_HZ);
    }

    /// Two modes sweep on a triangle and two on a sine, and every one of
    /// them stays inside the rails and comes back where it started.
    #[test]
    fn the_sweep_is_the_shape_the_mode_used() {
        use crate::params::console::drift as p;
        for mode in [p::CHORUS, p::FLANGER, p::VIBRATO, p::ENSEMBLE] {
            for i in 0..=32 {
                let v = sweep_at(mode, i as f32 / 32.0);
                assert!((-1.001..=1.001).contains(&v), "mode {mode} left the rails");
            }
            // A cycle is a cycle: the shape repeats.
            for i in 0..8 {
                let t = i as f32 / 8.0;
                assert!(
                    (sweep_at(mode, t) - sweep_at(mode, t + 1.0)).abs() < 1e-5,
                    "mode {mode} did not repeat"
                );
            }
        }
        // The triangle is straight between its corners; the sine is not.
        let straight = |mode: u32| {
            let a = sweep_at(mode, 0.1);
            let b = sweep_at(mode, 0.2);
            let c = sweep_at(mode, 0.3);
            ((a + c) * 0.5 - b).abs()
        };
        assert!(straight(p::CHORUS) < 1e-4, "the chorus was not a triangle");
        assert!(straight(p::VIBRATO) > 1e-3, "the vibrato was not a sine");
    }
}
