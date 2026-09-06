//! GRIT's face: the staircase, and four little faces that mind it.
//!
//! The section is a sampler's converter path degraded on purpose, so the
//! card draws the converter's own signature: a smooth wave with the
//! zero-order hold laid over it, its steps as wide as the clock is slow
//! and its levels as coarse as the word is short. RATE and BITS are not
//! two numbers here; they are the width and the height of the same
//! staircase, and you can see one without reading the other.
//!
//! The step width comes from the rate the section REPORTS, not the one
//! it is set to — so with JITTER up the staircase visibly stumbles,
//! because the clock really is wobbling block to block.
//!
//! # The faces
//!
//! Each of the four degradations carries a small face that minds it, and
//! one big one on the plate minds the lot. They are cute on purpose and
//! they are not decoration: a face is a reading you can take from the
//! corner of your eye, which is what you want from a section whose four
//! controls all make things worse in different ways.
//!
//! `-_-` is asleep — that control is doing nothing at all. Then `._.`,
//! `o_o`, `0_0` as it is leaned on, and `>_<` at the end of its range.
//! While a value is still moving, the face is `O_O`: startled. That is
//! not a timer — it is the gap between where the knob is and where the
//! drawing has caught up to, so it appears exactly while a thing is
//! changing and settles on its own.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
/// The share of the glass the staircase takes.
/// @tune 0.3..0.75
const PLOT_SHARE: f32 = 0.48;
const COLUMN_GAP: f32 = 8.0;
/// How long a slice of sound the staircase shows.
/// @tune 2..40 ms
const WINDOW_MS: f32 = 8.0;
/// How far a value must still have to travel to read as startled.
/// @tune 0.01..0.4
const STARTLED: f32 = 0.06;

/// GRIT's six controls.
#[derive(Clone, Copy, Debug)]
struct GritFace {
    plot: egui::Rect,
    rate: egui::Rect,
    bits: egui::Rect,
    jitter: egui::Rect,
    hiss: egui::Rect,
    post: egui::Rect,
    mix: egui::Rect,
}

impl Layout for GritFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::grit as p;
        vec![
            (p::RATE, self.rate),
            (p::BITS, self.bits),
            (p::JITTER, self.jitter),
            (p::HISS, self.hiss),
            (p::POST, self.post),
            (p::MIX, self.mix),
        ]
    }
}

fn grit_face(glass: egui::Rect) -> GritFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let plot_w = (x.width() - COLUMN_GAP) * crate::tune!(PLOT_SHARE);
    let plot = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + plot_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(plot.right() + COLUMN_GAP, x.top()), x.max);
    // Six rows, measured from the foot so a short glass loses nothing.
    let rows_h = ROW_H * 6.0 + 5.0;
    let top = (right.bottom() - rows_h).max(right.top());
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    GritFace {
        plot,
        rate: row(0),
        bits: row(1),
        jitter: row(2),
        hiss: row(3),
        post: row(4),
        mix: row(5),
    }
}

/// The face a control wears at `amount` of its own degradation, and
/// `travel` still to go before the drawing catches the knob.
///
/// Startled beats everything: while a value is moving, that is the fact
/// worth showing, whatever it is moving toward.
fn mood(amount: f32, travel: f32) -> &'static str {
    if travel > crate::tune!(STARTLED) {
        return "O_O";
    }
    match amount {
        a if a <= 0.001 => "-_-",
        a if a < 0.30 => "._.",
        a if a < 0.60 => "o_o",
        a if a < 0.90 => "0_0",
        _ => ">_<",
    }
}

/// The staircase: a sine, held at `steps` per window and quantised to
/// `levels`. Returns the smooth wave and the held one.
fn staircase(steps: f32, levels: f32, points: usize) -> (Vec<(f32, f32)>, Vec<(f32, f32)>) {
    let mut smooth = Vec::with_capacity(points + 1);
    let mut held = Vec::with_capacity(points + 1);
    let cycles = 2.0;
    for i in 0..=points {
        let t = i as f32 / points as f32;
        let wave = (t * cycles * core::f32::consts::TAU).sin();
        smooth.push((t, wave));
        // The hold: the wave is read at the clock and kept until the
        // next tick, which is what a zero-order hold does.
        let tick = if steps >= points as f32 {
            t
        } else {
            (t * steps).floor() / steps.max(1.0)
        };
        let sampled = (tick * cycles * core::f32::consts::TAU).sin();
        // The word length: the levels the sample is rounded onto.
        let quantised = if levels >= 4096.0 {
            sampled
        } else {
            (sampled * levels * 0.5).round() / (levels * 0.5)
        };
        held.push((t, quantised.clamp(-1.0, 1.0)));
    }
    (smooth, held)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::grit as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = grit_face(face.glass);
    let value = |id: u32| face.value(id);
    let rate = value(p::RATE);
    let bits = value(p::BITS);
    let jitter = value(p::JITTER) / 100.0;
    let hiss = value(p::HISS) / 100.0;
    let post = value(p::POST);
    let mix = value(p::MIX) / 100.0;
    // How far each degradation is pushed, 0 at its resting end.
    let rate_amount = 1.0 - ((rate - 1_000.0) / (p::RATE_OFF_HZ - 1_000.0)).clamp(0.0, 1.0);
    let bits_amount = 1.0 - ((bits - 2.0) / 14.0).clamp(0.0, 1.0);
    let post_amount = 1.0 - ((post - 200.0) / (p::POST_OFF_HZ - 200.0)).clamp(0.0, 1.0);
    let wire = rate_amount <= 0.0 && bits_amount <= 0.0 && jitter <= 0.0 && hiss <= 0.0;
    // The eased values, and how far each still has to travel: that gap
    // IS the startle, so a face goes wide exactly while a knob moves.
    let eased = [
        face.anim("rate", rate_amount, 0.18),
        face.anim("bits", bits_amount, 0.18),
        face.anim("jitter", jitter, 0.18),
        face.anim("hiss", hiss, 0.18),
    ];
    let wanted = [rate_amount, bits_amount, jitter, hiss];
    let travel: Vec<f32> = wanted
        .iter()
        .zip(eased.iter())
        .map(|(want, is)| (want - is).abs())
        .collect();
    // The clock the section REPORTS, which jitter makes wobble.
    let said_rate = if face.said.bands[0] > 0.0 {
        face.said.bands[0]
    } else {
        rate
    };
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.plot,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.plot.left() + 7.0, lay.plot.top() + font.size + 6.0),
        egui::pos2(lay.plot.right() - 7.0, lay.plot.bottom() - font.size - 5.0),
    );
    let at = |t: f32, y: f32| {
        egui::pos2(
            field.left() + t * field.width(),
            field.center().y - y.clamp(-1.0, 1.0) * field.height() * 0.45,
        )
    };
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), field.center().y),
            egui::pos2(field.right(), field.center().y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.5),
    );
    let steps = crate::tune!(WINDOW_MS) * 1e-3 * said_rate;
    let levels = 2f32.powf(bits.clamp(1.0, 24.0));
    let (smooth, held) = staircase(steps, levels, 240);
    // The wave as it arrived, dashed under the staircase. Without it
    // the steps are just a shape; against it they are what was lost.
    chrome::dashes(
        &mut shapes,
        &smooth.iter().map(|(t, y)| at(*t, *y)).collect::<Vec<_>>(),
        0.0,
        Weight::Hair,
        alpha.live_dim.color,
    );
    chrome::trace(
        &mut shapes,
        &held.iter().map(|(t, y)| at(*t, *y)).collect::<Vec<_>>(),
        Weight::Heavy,
        if wire {
            edge.gamma_multiply(0.9)
        } else {
            tool::mix_ink(
                ink,
                alpha.jeopardy_latent.color,
                rate_amount.max(bits_amount),
            )
        },
    );
    painter.extend(shapes);

    // ---- The words, the rows, and the faces. -------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "GRIT");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["RATE", "BITS", "JITTER", "HISS", "POST", "MIX"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let mood_w = span("O_O") + 6.0;
    let figure = span("48.0k") + 6.0;
    let hz_word = |hz: f32| {
        if hz >= 1000.0 {
            format!("{:.1}k", hz / 1000.0)
        } else {
            format!("{hz:.0}")
        }
    };
    for (i, (rect, word, share, said, wears)) in [
        (lay.rate, "RATE", rate_amount, hz_word(rate), true),
        (lay.bits, "BITS", bits_amount, format!("{bits:.0}"), true),
        (
            lay.jitter,
            "JITTER",
            jitter,
            format!("{:.0}", jitter * 100.0),
            true,
        ),
        (lay.hiss, "HISS", hiss, format!("{:.0}", hiss * 100.0), true),
        (
            lay.post,
            "POST",
            post_amount,
            if post >= p::POST_OFF_HZ - 1.0 {
                "OFF".to_owned()
            } else {
                hz_word(post)
            },
            false,
        ),
        (lay.mix, "MIX", mix, format!("{:.0}", mix * 100.0), false),
    ]
    .into_iter()
    .enumerate()
    {
        // POST and MIX wear no face: they are the remedies, not the
        // damage, and a face on them would be saying the wrong thing.
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.5),
            egui::pos2(
                rect.right() - figure - if wears { mood_w } else { 0.0 },
                rect.center().y + 2.5,
            ),
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
                    if wears {
                        tool::mix_ink(ink, alpha.jeopardy_latent.color, share)
                    } else {
                        alpha.live.color
                    },
                );
            }
        }
        words.text(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        if wears {
            let seen = mood(eased[i], travel[i]);
            words.text(
                egui::pos2(rect.right() - figure - 3.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                seen,
                if seen == "-_-" {
                    edge
                } else {
                    tool::mix_ink(ink, alpha.jeopardy_latent.color, eased[i])
                },
            );
        }
        words.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }

    // The one on the plate minds the lot: the worst of the four, and
    // startled if any of them is still moving.
    let worst = eased.iter().fold(0.0f32, |m, v| m.max(*v));
    let moving = travel.iter().fold(0.0f32, |m, v| m.max(*v));
    words.text(
        egui::pos2(lay.plot.right() - 8.0, lay.plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        mood(worst, moving),
        if wire {
            edge
        } else {
            tool::mix_ink(ink, alpha.jeopardy_latent.color, worst)
        },
    );
    words.text(
        egui::pos2(lay.plot.left() + 8.0, lay.plot.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "CONVERTER",
        edge,
    );
    // The clock as it really ran, and the word length beside it.
    words.text(
        egui::pos2(field.left(), lay.plot.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        if wire {
            "WIRE".to_owned()
        } else if quiet {
            format!("{} b", bits.round())
        } else {
            format!("{} · {} b", hz_word(said_rate), bits.round())
        },
        if wire { edge } else { alpha.live.color },
    );
    words.text(
        egui::pos2(field.right(), lay.plot.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{:.0}ms", crate::tune!(WINDOW_MS)),
        edge,
    );
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
    fn every_grit_control_has_its_own_row() {
        let face = grit_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 6);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its row");
            assert!(
                !face.plot.intersects(*rect),
                "control {id} invaded the plot"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "controls {id} and {other} overlap"
                );
            }
        }
    }

    /// The faces are a reading, so they have to read: asleep at rest,
    /// worse as it is leaned on, and startled while it moves — whatever
    /// it is moving toward.
    #[test]
    fn the_faces_say_what_the_control_is_doing() {
        assert_eq!(mood(0.0, 0.0), "-_-");
        assert_eq!(mood(1.0, 0.0), ">_<");
        // Startled beats everything, including asleep: a knob moving
        // toward zero is still a knob moving.
        assert_eq!(mood(0.0, 0.5), "O_O");
        assert_eq!(mood(1.0, 0.5), "O_O");
        // Monotone: more degradation never gives a calmer face.
        let rank = |m: &str| {
            ["-_-", "._.", "o_o", "0_0", ">_<"]
                .iter()
                .position(|s| *s == m)
                .expect("a mood off the scale")
        };
        let mut last = 0;
        for i in 0..=20 {
            let here = rank(mood(i as f32 / 20.0, 0.0));
            assert!(here >= last, "the face got calmer at {i}");
            last = here;
        }
        // Every face is the same width, or a column of them would jitter.
        for m in ["-_-", "._.", "o_o", "0_0", ">_<", "O_O"] {
            assert_eq!(m.chars().count(), 3, "{m} is not three cells wide");
        }
    }

    /// The staircase is the converter: a wire at the top of both
    /// ranges, and visibly stepped below them.
    #[test]
    fn the_staircase_is_the_converter() {
        // A fast clock and a long word: the held wave IS the smooth one.
        let (smooth, held) = staircase(1_000.0, 65_536.0, 240);
        for ((_, a), (_, b)) in smooth.iter().zip(held.iter()) {
            assert!((a - b).abs() < 1e-3, "the wire was not a wire");
        }
        // A slow clock holds: neighbouring points share a value.
        let (_, held) = staircase(8.0, 65_536.0, 240);
        let flats = held.windows(2).filter(|w| w[0].1 == w[1].1).count();
        assert!(flats > 200, "a slow clock did not hold: {flats}");
        // A short word quantises: the values it takes are few.
        let (_, held) = staircase(1_000.0, 8.0, 240);
        let mut levels: Vec<f32> = held.iter().map(|(_, y)| *y).collect();
        levels.sort_by(f32::total_cmp);
        levels.dedup();
        assert!(levels.len() <= 9, "three bits took {} levels", levels.len());
        // And nothing ever leaves the rails.
        for (_, y) in held {
            assert!((-1.0..=1.0).contains(&y));
        }
    }
}
