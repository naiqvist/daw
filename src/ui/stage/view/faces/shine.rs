//! SHINE's face: where the sheen is made, and why it is sheen.
//!
//! The exciter takes the top off with a high-pass, drives it hard enough
//! to grow harmonics that were never there, and adds it back under the
//! whole sound. Three controls, so the card is not busy — what it owes
//! is an answer to the question the section actually raises, which is
//! why this reads as air and not as distortion.
//!
//! The answer is the LADDER, and it is drawn as a COMPARISON, because
//! the obvious version of the claim is false. "The even rungs lead" is
//! true at a nudge and wrong at drive — past about half amount the third
//! harmonic outruns the second, since tanh's own odd term grows faster
//! than anything the bias adds. A card that said EVEN LEADS would be
//! lying at most of its own range.
//!
//! What IS true is across curves: a symmetric curve of the same
//! strength makes almost no even harmonics at all. So the symmetric
//! ladder is drawn behind this one as an outline, and the even rungs
//! standing clear of it are the exciter's own — the octave above rather
//! than the fifth, which is why this reads as sheen and a fuzz does not.
//! Both ladders are measured from the curves the core runs
//! (`console::shine_curve`), not asserted.
//!
//! The SPLIT says where it happens. Below TUNE the sound is left alone
//! and is drawn as such; above it is what gets driven. A section whose
//! only frequency control is a corner should show the corner.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
/// The split strip's height.
/// @tune 16..60 px
const SPLIT_H: f32 = 34.0;
const COLUMN_GAP: f32 = 8.0;
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;

/// SHINE's three controls.
#[derive(Clone, Copy, Debug)]
struct ShineFace {
    ladder: egui::Rect,
    split: egui::Rect,
    amount: egui::Rect,
    tune: egui::Rect,
    mix: egui::Rect,
}

impl Layout for ShineFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::shine as p;
        vec![
            (p::AMOUNT, self.amount),
            (p::TUNE, self.tune),
            (p::MIX, self.mix),
        ]
    }
}

fn shine_face(glass: egui::Rect) -> ShineFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let ladder_w = (x.width() - COLUMN_GAP) * 0.52;
    let ladder = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + ladder_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(ladder.right() + COLUMN_GAP, x.top()), x.max);
    let split = egui::Rect::from_min_size(right.min, egui::vec2(right.width(), SPLIT_H));
    let rows_h = ROW_H * 3.0 + 2.0;
    let top = (right.bottom() - rows_h).max(split.bottom() + 4.0);
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    ShineFace {
        ladder,
        split,
        amount: row(0),
        tune: row(1),
        mix: row(2),
    }
}

/// Where a frequency stands across the split strip, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::shine_curve as sc;
    use crate::params::console::shine as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = shine_face(face.glass);
    let amount = face.value(p::AMOUNT) / 100.0;
    let tune = face.value(p::TUNE);
    let mix = face.value(p::MIX) / 100.0;
    let wire = amount <= 0.0;
    let heat = face.said.reduction_db.abs();
    let quiet = face.said.level_db <= -119.0;
    // Probed at the signal when there is one, at a stated rest level
    // when there is not: a ladder that showed nothing at rest would hide
    // the one thing the amount is choosing.
    let probe = if quiet {
        0.25
    } else {
        10f32.powf(face.said.level_db / 20.0).clamp(0.05, 1.0)
    };
    let made = sc::harmonics(amount, probe);
    // The same drive without its bias: what the evens would be if the
    // curve were symmetric. The gap between the two IS the exciter.
    let plain = sc::without_bias(amount, probe);

    let mut shapes = Vec::new();

    // ---- The ladder: which rungs the exciter stands on. --------------
    chrome::panel_variant(
        &mut shapes,
        lay.ladder,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let rungs = egui::Rect::from_min_max(
        egui::pos2(lay.ladder.left() + 8.0, lay.ladder.top() + font.size + 6.0),
        egui::pos2(
            lay.ladder.right() - 8.0,
            // Two rows of type at the foot, not one: the rung numbers,
            // and the sentence the ladder is drawn to support.
            lay.ladder.bottom() - font.size * 2.0 - 12.0,
        ),
    );
    let bars = crate::console::harmonic::COUNT - 1;
    let bar_w = (rungs.width() / bars as f32).max(2.0);
    for i in 0..bars {
        let harmonic = i + 2;
        let db = made.db[i + 1];
        let share = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
        let x0 = rungs.left() + i as f32 * bar_w;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0 + 1.5, rungs.bottom() - rungs.height() * share),
            egui::pos2(x0 + bar_w - 2.5, rungs.bottom()),
        );
        // The even rungs are the sheen. The odd ones are what an
        // exciter is NOT, so they are drawn and not hidden — the gap
        // between the two families IS the reading.
        let even = harmonic % 2 == 0;
        let tone = if share <= 0.001 {
            edge.gamma_multiply(0.45)
        } else if even {
            alpha.live.color
        } else {
            ink.gamma_multiply(0.8)
        };
        if share <= 0.001 {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(bar.left(), rungs.bottom()),
                    egui::pos2(bar.right(), rungs.bottom()),
                ],
                Weight::Hair,
                tone,
            );
        } else {
            shapes.push(egui::Shape::rect_filled(bar, 0.0, tone));
        }
        // What a symmetric curve of the same strength would have made,
        // as an outline. On the odd rungs the two agree and the outline
        // sits on the bar; on the even ones the bar stands clear of it,
        // and that gap is the whole of what an exciter does.
        let plain_share = ((plain.db[i + 1] + 72.0) / 72.0).clamp(0.0, 1.0);
        if !wire && plain_share > 0.001 {
            let y = rungs.bottom() - rungs.height() * plain_share;
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(bar.left() - 1.0, y),
                    egui::pos2(bar.right() + 1.0, y),
                ],
                Weight::Hair,
                edge.gamma_multiply(1.3),
            );
        }
    }

    // ---- The split: what is left alone, and what is driven. ----------
    chrome::panel_variant(
        &mut shapes,
        lay.split,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let strip = egui::Rect::from_min_max(
        egui::pos2(lay.split.left() + 7.0, lay.split.top() + font.size + 4.0),
        egui::pos2(lay.split.right() - 7.0, lay.split.bottom() - 4.0),
    );
    if strip.is_positive() {
        let corner = strip.left() + place_of_hz(tune) * strip.width();
        // Below the corner: untouched, and drawn as untouched.
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(strip.min, egui::pos2(corner, strip.bottom())),
            0.0,
            edge.gamma_multiply(0.25),
        ));
        // Above it: the band the exciter works on, lit by the amount.
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(egui::pos2(corner, strip.top()), strip.max),
            0.0,
            alpha.live.color.gamma_multiply(0.18 + amount * 0.45),
        ));
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(corner, strip.top() - 2.0),
                egui::pos2(corner, strip.bottom() + 2.0),
            ],
            Weight::Heavy,
            alpha.live.color,
        );
        for hz in [100.0f32, 1_000.0, 10_000.0] {
            let x = strip.left() + place_of_hz(hz) * strip.width();
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(x, strip.bottom() - 3.0),
                    egui::pos2(x, strip.bottom()),
                ],
                Weight::Hair,
                edge,
            );
        }
    }
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "SHINE");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["AMOUNT", "TUNE", "MIX"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span("12.0k") + 6.0;
    words.text(
        egui::pos2(lay.ladder.left() + 8.0, lay.ladder.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "HARMONICS",
        edge,
    );
    words.text(
        egui::pos2(lay.ladder.right() - 8.0, lay.ladder.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else {
            format!("{:.1}%", made.thd * 100.0)
        },
        if wire { edge } else { alpha.live.color },
    );
    for i in 0..bars {
        words.text(
            egui::pos2(
                rungs.left() + (i as f32 + 0.5) * bar_w,
                rungs.bottom() + 2.0,
            ),
            egui::Align2::CENTER_TOP,
            format!("{}", i + 2),
            if (i + 2) % 2 == 0 {
                alpha.live.color
            } else {
                edge
            },
        );
    }
    // The one sentence the ladder is drawn to support.
    words.text(
        egui::pos2(lay.ladder.center().x, lay.ladder.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        if wire { "--" } else { "— SYMMETRIC" },
        if wire { edge } else { alpha.live.color },
    );
    words.text(
        egui::pos2(lay.split.left() + 8.0, lay.split.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "SPLIT",
        edge,
    );
    words.text(
        egui::pos2(lay.split.right() - 8.0, lay.split.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if tune >= 1000.0 {
            format!("{:.1}k", tune / 1000.0)
        } else {
            format!("{tune:.0}")
        },
        ink,
    );
    for (rect, word, share, said, tone) in [
        (
            lay.amount,
            "AMOUNT",
            amount,
            format!("{:.0}", amount * 100.0),
            tool::mix_ink(ink, alpha.jeopardy_latent.color, heat.min(1.0)),
        ),
        (
            lay.tune,
            "TUNE",
            place_of_hz(tune),
            if tune >= 1000.0 {
                format!("{:.1}k", tune / 1000.0)
            } else {
                format!("{tune:.0}")
            },
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
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(440.0, 210.0))
    }

    #[test]
    fn every_shine_control_has_its_own_row() {
        let face = shine_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 3);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its row");
            assert!(!face.ladder.intersects(*rect));
            assert!(!face.split.intersects(*rect));
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "controls {id} and {other} overlap"
                );
            }
        }
    }

    /// The claim the card is built to support — and NOT the one that
    /// looks obvious.
    ///
    /// "The even rungs lead the odd ones" is true at a nudge and false
    /// at drive: past about half amount the third outruns the second,
    /// because tanh's own odd term grows faster than anything the bias
    /// adds. The first version of this card said EVEN LEADS on its face
    /// and this test caught it lying over most of its own range.
    ///
    /// What holds everywhere is across curves: the bias is what makes
    /// the evens, so the exciter's even rungs stand far above those of a
    /// symmetric curve driven exactly as hard.
    #[test]
    fn the_bias_is_what_makes_the_even_harmonics() {
        use crate::console::shine_curve as sc;
        for amount in [0.2f32, 0.5, 0.8, 1.0] {
            let made = sc::harmonics(amount, 0.5);
            let plain = sc::without_bias(amount, 0.5);
            // The second and the fourth: present here, absent there.
            for even in [1usize, 3] {
                assert!(
                    made.db[even] > plain.db[even] + 20.0,
                    "at {amount} rung {} was not the bias's doing: {made:?} against {plain:?}",
                    even + 1
                );
            }
            // The odd rungs are the drive's, and both curves have them:
            // the comparison must not flatter the exciter.
            assert!(
                (made.db[2] - plain.db[2]).abs() < 12.0,
                "at {amount} the third differed more than the drive explains"
            );
            assert!(made.thd > 0.0 && plain.thd > 0.0);
        }
    }

    /// At no amount the section is a wire, and the card draws an empty
    /// ladder because the ladder IS empty.
    #[test]
    fn no_amount_makes_nothing() {
        use crate::console::shine_curve as sc;
        let quiet = sc::harmonics(0.0, 0.5);
        assert_eq!(quiet.thd, 0.0);
        assert!(
            quiet
                .db
                .iter()
                .skip(1)
                .all(|db| *db <= crate::console::harmonic::FLOOR_DB)
        );
        // And the curve itself is the identity.
        for i in 0..=8 {
            let x = -1.0 + 2.0 * i as f32 / 8.0;
            assert!((sc::transfer(0.0, x) - x).abs() < 1e-5);
        }
    }

    /// The corner is a place on the same log scale the desk draws every
    /// spectrum on, and it stays inside the strip at both ends.
    #[test]
    fn the_corner_stands_on_the_scale() {
        use crate::params::console::shine as p;
        assert!(place_of_hz(1_000.0) > 0.0 && place_of_hz(1_000.0) < 1.0);
        assert!(place_of_hz(12_000.0) < 1.0);
        assert!(place_of_hz(p::TABLE[1].min) < place_of_hz(p::TABLE[1].max));
    }
}
