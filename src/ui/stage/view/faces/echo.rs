//! ECHO's face: the repeats as rings, and the two sides as halves.
//!
//! A delay drawn along a line is a delay drawn as a ruler. This one is
//! drawn as what it is — a loop — so time is RADIUS: the dry is the
//! centre, the first repeat is the first ring, and every pass after it
//! is a ring further out. Equal repeats are equally spaced rings, so the
//! rhythm of the thing is a shape rather than a row of numbers.
//!
//! The circle is cut in half down the middle and the halves are the two
//! sides. A repeat is drawn as an ARC on the half it comes back on, so
//! ping-pong is immediately what it looks like: arcs alternating left,
//! right, left, right, out from the centre. At no spread both halves
//! carry every ring and the picture is symmetrical, which is also what
//! that setting sounds like.
//!
//! # What the rings say
//!
//! Their reach is the level after that many passes of feedback, and
//! their INK is how much of the top has survived: the loop has a tone
//! control inside it, so each pass is duller than the last, and the
//! rings recede toward the ground as well as thinning. A delay whose
//! repeats only got quieter would be a digital one.
//!
//! # The beat circles
//!
//! The section reports one beat in milliseconds at the tempo the block
//! actually ran at, so the beat circles are drawn from the transport
//! rather than assumed. In SYNC the ring arcs sit on them. In FREE they
//! do not, and how far off is the one question a delay actually raises.
//!
//! # The head
//!
//! The innermost ring is drawn at the time the line is READING, not the
//! time it is set to: the section glides rather than jumping, so a turn
//! of TIME swoops the ring outward, and WOW wobbles it, which is the
//! whole difference between this and a digital delay.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 17.0;
const COLUMN_GAP: f32 = 7.0;
/// The quietest repeat worth a ring, in dB.
/// @tune -60..-12
const TAIL_DB: f32 = -40.0;
/// The most rings drawn, whatever the feedback.
const RINGS_MAX: usize = 24;
/// The least room between two rings, in pixels.
///
/// A train of twelve rings in a dial fifty pixels across is not a train,
/// it is a smudge. So the dial draws as many repeats as it can SEPARATE
/// and says how many that is out of how many there are — a picture that
/// cannot be resolved is worse than one that admits it is showing part.
/// @tune 4..20 px
const RING_GAP: f32 = 9.0;
/// The frequency the ink's dulling is measured at, in Hz.
///
/// A stated one, because "duller" has to be duller than something. It
/// is the top of the band a repeat is judged on: how much of four
/// kilohertz has survived n passes of the loop's tone filter.
const TOP_HZ: f32 = 4_000.0;

/// ECHO's seven controls.
#[derive(Clone, Copy, Debug)]
struct EchoFace {
    rings: egui::Rect,
    sync: egui::Rect,
    time: egui::Rect,
    feedback: egui::Rect,
    tone: egui::Rect,
    wow: egui::Rect,
    pingpong: egui::Rect,
    mix: egui::Rect,
}

impl Layout for EchoFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::echo as p;
        vec![
            (p::SYNC, self.sync),
            (p::TIME, self.time),
            (p::FEEDBACK, self.feedback),
            (p::TONE, self.tone),
            (p::WOW, self.wow),
            (p::PINGPONG, self.pingpong),
            (p::MIX, self.mix),
        ]
    }
}

fn echo_face(glass: egui::Rect) -> EchoFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    // Seven controls in two columns, so the rings keep a square of the
    // card rather than a slot.
    let col_w = ((x.width() - COLUMN_GAP * 2.0) * 0.30).clamp(110.0, 160.0);
    let rings = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right() - col_w * 2.0 - COLUMN_GAP * 2.0, x.bottom()),
    );
    let cell = |col: usize, row: usize| {
        egui::Rect::from_min_size(
            egui::pos2(
                rings.right() + COLUMN_GAP + col as f32 * (col_w + COLUMN_GAP),
                x.top() + row as f32 * (ROW_H + 1.0),
            ),
            egui::vec2(col_w, ROW_H),
        )
    };
    EchoFace {
        rings,
        sync: cell(0, 0),
        time: cell(0, 1),
        feedback: cell(0, 2),
        tone: cell(0, 3),
        wow: cell(1, 0),
        pingpong: cell(1, 1),
        mix: cell(1, 2),
    }
}

/// What a sync setting is called. Index 0 is FREE and the rest are the
/// section's own beats, so the word and the arithmetic cannot name
/// different divisions.
fn sync_word(sync: usize) -> String {
    use crate::params::console::echo as p;
    if sync == 0 || sync > p::SYNC_BEATS.len() {
        return "FREE".to_owned();
    }
    let beats = p::SYNC_BEATS[sync - 1];
    if beats >= 1.0 {
        format!("{beats:.0}bt")
    } else {
        format!("1/{:.0}", 4.0 / beats)
    }
}

/// How many repeats are worth drawing at this feedback.
fn rings_at(feedback: f32) -> usize {
    let fb = feedback.clamp(0.0, 0.99);
    if fb <= 0.0 {
        return 1;
    }
    let passes = (TAIL_DB / (20.0 * fb.log10())).ceil();
    (passes.max(1.0) as usize).min(RINGS_MAX)
}

/// How much of [`TOP_HZ`] survives `passes` through a one-pole at
/// `corner`, 0..1.
///
/// The loop's tone filter runs once per pass, so this is its magnitude
/// raised to the pass count — which is why a repeat goes dull far faster
/// than it goes quiet.
fn top_left(corner: f32, passes: usize) -> f32 {
    let ratio = TOP_HZ / corner.max(1.0);
    let once = 1.0 / (1.0 + ratio * ratio).sqrt();
    once.powi(passes as i32).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::echo as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = echo_face(face.glass);
    // Index 0 is FREE; the rest line up with the section's own beats.
    let sync = face.value(p::SYNC).round().clamp(0.0, 5.0) as usize;
    let set_ms = face.value(p::TIME);
    let feedback = face.value(p::FEEDBACK) / 100.0;
    let tone = face.value(p::TONE);
    let wow = face.value(p::WOW) / 100.0;
    let spread = if face.value(p::PINGPONG) >= 0.5 {
        1.0
    } else {
        0.0
    };
    let mix = face.value(p::MIX) / 100.0;
    let wire = mix <= 0.0;
    // What the line is actually doing: the glided time, the head's
    // wander, the beat as the transport ran it, and the loop's bite.
    let live_ms = if face.said.bands[0] > 0.0 {
        face.said.bands[0]
    } else {
        set_ms
    };
    let wander = face.said.bands[1];
    let beat_ms = face.said.bands[2];
    let bite = face.said.reduction_db;

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.rings,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let dial = egui::Rect::from_min_max(
        egui::pos2(lay.rings.left() + 7.0, lay.rings.top() + font.size + 5.0),
        egui::pos2(
            lay.rings.right() - 7.0,
            lay.rings.bottom() - font.size - 5.0,
        ),
    );
    let centre = dial.center();
    let reach = (dial.width().min(dial.height()) * 0.5 - 2.0).max(6.0);
    let audible = rings_at(feedback);
    // As many as the dial can hold apart, which is not always as many as
    // there are.
    let rings = audible.min((reach / crate::tune!(RING_GAP)).floor().max(1.0) as usize);
    // The scale: the outermost ring drawn reaches the edge.
    let furthest = (live_ms + wander.abs()) * rings as f32;
    let r_at = |ms: f32| (ms / furthest.max(1.0)).clamp(0.0, 1.0) * reach;
    let arc = |from: f32, to: f32, r: f32| -> Vec<egui::Pos2> {
        (0..=36)
            .map(|i| {
                let t = from + (to - from) * i as f32 / 36.0;
                let a = t.to_radians();
                egui::pos2(centre.x + r * a.cos(), centre.y - r * a.sin())
            })
            .collect()
    };
    // The beat circles, from the tempo the block actually ran at.
    if beat_ms > 0.0 {
        let mut beat = beat_ms;
        while beat <= furthest && beat > 0.0 {
            chrome::curve(
                &mut shapes,
                &arc(0.0, 360.0, r_at(beat)),
                Weight::Hair,
                edge.gamma_multiply(0.55),
            );
            beat += beat_ms;
        }
    }
    // The line down the middle: the two halves are the two sides.
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(centre.x, centre.y - reach),
            egui::pos2(centre.x, centre.y + reach),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.7),
    );
    // The repeats. Each one is an arc on the half it comes back on, as
    // far out as its time and as bright as what is left of it.
    if !wire {
        for n in 1..=rings {
            let ms = live_ms * n as f32 + wander;
            if ms <= 0.0 || ms > furthest {
                continue;
            }
            let level = feedback.powi(n as i32 - 1);
            let top = top_left(tone, n);
            // Ping-pong: the odd repeats come back on one side and the
            // even ones on the other, and at no spread both halves
            // carry every ring.
            let sides: &[(f32, f32)] = if spread <= 0.01 {
                &[(90.0, 270.0), (270.0, 450.0)]
            } else if n % 2 == 1 {
                &[(270.0, 450.0)]
            } else {
                &[(90.0, 270.0)]
            };
            // Dulling moves a ring toward the DIM ink, not toward the
            // ground. A repeat that has lost its top is still a repeat,
            // and a train whose later rings vanish into the background
            // is not showing the tail — it is hiding it.
            let tone_ink = tool::mix_ink(
                alpha.live.color,
                alpha.live_dim.color,
                (1.0 - top).clamp(0.0, 1.0),
            );
            for (from, to) in sides {
                chrome::curve(
                    &mut shapes,
                    &arc(*from, *to, r_at(ms)),
                    if level > 0.5 {
                        Weight::Heavy
                    } else {
                        Weight::Hair
                    },
                    // And a floor under the level, so the quietest ring
                    // in the train is faint rather than absent.
                    tone_ink.gamma_multiply(0.35 + 0.65 * level),
                );
            }
        }
        // The head: the innermost ring, where the line is READING. It
        // swoops when the time is turned and wobbles with the wow.
        chrome::pad(
            &mut shapes,
            egui::pos2(centre.x, centre.y - r_at(live_ms + wander)),
            chrome::PAD,
            alpha.jeopardy_latent.color,
            true,
        );
    }
    chrome::pad(&mut shapes, centre, chrome::PAD - 1.0, ink, true);
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "ECHO");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["FEEDBACK", "PING", "SYNC"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 5.0;
    words.text(
        egui::pos2(lay.rings.left() + 8.0, lay.rings.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "REPEATS",
        edge,
    );
    words.text(
        egui::pos2(lay.rings.right() - 8.0, lay.rings.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else if rings < audible {
            format!("{live_ms:.0}ms {rings}/{audible}")
        } else {
            format!("{live_ms:.0}ms x{audible}")
        },
        if wire { edge } else { alpha.live.color },
    );
    // The one question a delay raises, answered along the foot.
    words.text(
        egui::pos2(lay.rings.left() + 8.0, lay.rings.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        sync_word(sync),
        if sync == 0 { edge } else { alpha.live.color },
    );
    words.text(
        egui::pos2(lay.rings.right() - 8.0, lay.rings.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        if beat_ms > 0.0 {
            format!("beat {beat_ms:.0}")
        } else {
            "beat --".to_owned()
        },
        if beat_ms > 0.0 { ink } else { edge },
    );
    let hz_word = |hz: f32| {
        if hz >= 1000.0 {
            format!("{:.1}k", hz / 1000.0)
        } else {
            format!("{hz:.0}")
        }
    };
    for (rect, word, share, said, tone_ink) in [
        (
            lay.sync,
            "SYNC",
            sync as f32 / 5.0,
            sync_word(sync),
            alpha.live.color,
        ),
        (
            lay.time,
            "TIME",
            (live_ms / 1000.0).clamp(0.0, 1.0),
            format!("{live_ms:.0}"),
            alpha.jeopardy_latent.color,
        ),
        (
            lay.feedback,
            "FEEDBACK",
            feedback,
            format!("{:.0}", feedback * 100.0),
            alpha.live.color,
        ),
        (
            lay.tone,
            "TONE",
            (tone / 20_000.0).clamp(0.0, 1.0),
            hz_word(tone),
            alpha.live.color,
        ),
        (
            lay.wow,
            "WOW",
            wow,
            format!("{:.0}", wow * 100.0),
            alpha.jeopardy_latent.color,
        ),
        (
            lay.pingpong,
            "PING",
            spread,
            if spread > 0.0 { "ON" } else { "OFF" }.to_owned(),
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
        let figure = span("20.0k") + 4.0;
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.0),
            egui::pos2(rect.right() - figure, rect.center().y + 2.0),
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
                    tone_ink,
                );
            }
        }
        words.text(
            egui::pos2(rect.left() + 1.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(rect.right() - 1.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }
    // The loop's bite, under the two columns: what the soft top took at
    // this block's hottest sample.
    let bite_y = lay.tone.bottom() + ROW_H * 0.5 + 3.0;
    if bite_y < lay.rings.bottom() {
        words.text(
            egui::pos2(lay.wow.left() + 1.0, bite_y),
            egui::Align2::LEFT_CENTER,
            "BITE",
            edge,
        );
        words.text(
            egui::pos2(lay.mix.right() - 1.0, bite_y),
            egui::Align2::RIGHT_CENTER,
            if bite >= -0.05 {
                "--".to_owned()
            } else {
                format!("{bite:.1}dB")
            },
            if bite >= -0.05 {
                edge
            } else {
                alpha.jeopardy_latent.color
            },
        );
    }
    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 210.0))
    }

    #[test]
    fn every_echo_control_has_its_own_cell() {
        let face = echo_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 7);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its cell");
            assert!(
                !face.rings.intersects(*rect),
                "control {id} invaded the rings"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(!rect.intersects(*other_rect), "{id} and {other} overlap");
            }
        }
    }

    /// The train is as long as the feedback earns: no feedback is one
    /// repeat, and more feedback is more rings, up to the cap.
    #[test]
    fn the_train_is_as_long_as_the_feedback_earns() {
        assert_eq!(rings_at(0.0), 1);
        let mut last = 0;
        for fb in [0.1f32, 0.35, 0.6, 0.8, 0.95] {
            let n = rings_at(fb);
            assert!(n >= last, "more feedback gave a shorter train");
            assert!(n <= RINGS_MAX);
            last = n;
        }
        // At the top of the range it is the cap and not an eternity.
        assert_eq!(rings_at(0.99), RINGS_MAX);
        // A repeat at the tail really is under the floor it was chosen
        // for, so the train stops where it stops honestly.
        let fb = 0.6f32;
        let n = rings_at(fb);
        let tail_db = 20.0 * fb.powi(n as i32).log10();
        assert!(tail_db <= TAIL_DB + 6.0, "the train ran on to {tail_db} dB");
    }

    /// Each pass is duller than the last, and far faster than it is
    /// quieter — which is the thing the rings' ink is drawn to show.
    #[test]
    fn the_repeats_go_dull_faster_than_they_go_quiet() {
        // A dark tone eats the top in a couple of passes.
        let dark = top_left(1_000.0, 1);
        assert!(dark < 0.3, "one pass at 1k left {dark} of 4k");
        assert!(top_left(1_000.0, 4) < dark);
        // An open tone leaves it nearly alone.
        assert!(top_left(20_000.0, 1) > 0.95);
        // Monotone in passes, at any corner.
        for corner in [500.0f32, 2_000.0, 8_000.0] {
            let mut last = 1.0;
            for n in 1..6 {
                let left = top_left(corner, n);
                assert!(left <= last, "pass {n} at {corner} was brighter");
                last = left;
            }
        }
        // And it outruns the level: at a middling tone and feedback the
        // fourth repeat has lost more top than volume.
        let level = 0.6f32.powi(3);
        assert!(top_left(3_000.0, 4) < level, "the top outlasted the level");
    }
}
