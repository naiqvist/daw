//! SMEAR's face: when each octave arrives, and where the wavefront is.
//!
//! The section filters nothing. Its magnitude is flat to a third of a
//! decibel at every setting; all it moves is WHEN the parts of a sound
//! come out. So the card is a chart of when: frequency across, delay
//! down, and the curve is the group delay computed from the very
//! coefficients the kernel is tuned with (`console::smear_curve`) — in
//! milliseconds, which is what the eye is being asked to read.
//!
//! What the chart shows is not the tidy story. The delay is not a slope
//! from high to low: it is a HUMP at the corner, because a run of
//! allpasses turns the phase of what is near its corner much further
//! than anything else. So the frequencies around CENTRE come out last
//! and everything either side of them comes out nearly on time, and the
//! corner is marked on the curve so it is obvious which frequencies are
//! being held. Drawing a ramp would have been prettier and wrong.
//!
//! # The wavefront
//!
//! A chart of when is a static thing, and what the section does is not.
//! The core reports the AGE of the two most recent onsets, in
//! milliseconds, and their strength — so the card draws a line at each
//! age and fills what is above it. Everything above the line has already
//! come out; everything below it is still on its way. On a snare the
//! line sweeps down and you watch the hit come out in pieces — the ends
//! of the spectrum first, the band around the corner trailing after
//! them, which is the chirp. That is the whole of what a disperser does
//! and it is invisible in every other way of drawing one.
//!
//! Two lines, because two onsets can be in flight at once — a second hit
//! landing while the first is still unwinding is exactly when a smear
//! stops being a chirp and starts being a wash.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
const COLUMN_GAP: f32 = 8.0;
/// The chart's window.
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;
/// The rate the delay is drawn at.
const DRAWN_AT: f32 = 48_000.0;
/// The delay windows the chart steps through, in ms.
///
/// The same reasoning as the EQ's: fixed at the widest, a two-section
/// smear is a flat line along the top; fitted exactly, every setting
/// looks the same. So it steps, and the window in force is written on
/// the chart.
const SPANS_MS: [f32; 5] = [2.0, 6.0, 20.0, 60.0, 200.0];

/// SMEAR's two controls.
#[derive(Clone, Copy, Debug)]
struct SmearFace {
    chart: egui::Rect,
    amount: egui::Rect,
    centre: egui::Rect,
    said: egui::Rect,
}

impl Layout for SmearFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::smear as p;
        vec![(p::AMOUNT, self.amount), (p::CENTRE, self.centre)]
    }
}

fn smear_face(glass: egui::Rect) -> SmearFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    // Two controls, so the chart takes nearly all of it.
    let right_w = (x.width() * 0.30).clamp(120.0, 190.0);
    let chart = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right() - right_w - COLUMN_GAP, x.bottom()),
    );
    let right = egui::Rect::from_min_max(egui::pos2(x.right() - right_w, x.top()), x.max);
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), right.top() + i as f32 * (ROW_H + 2.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    SmearFace {
        chart,
        amount: row(0),
        centre: row(1),
        said: egui::Rect::from_min_max(egui::pos2(right.left(), row(1).bottom() + 6.0), right.max),
    }
}

/// Where a frequency stands across the chart, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

/// The window that holds `worst` with room above it.
fn span_for(worst: f32) -> f32 {
    SPANS_MS
        .into_iter()
        .find(|span| worst * 1.15 <= *span)
        .unwrap_or(SPANS_MS[SPANS_MS.len() - 1])
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::smear_curve as sc;
    use crate::params::console::smear as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = smear_face(face.glass);
    let shape = sc::Shape::of(&face.params);
    let amount = face.value(p::AMOUNT);
    let centre = face.value(p::CENTRE);
    let wire = shape.stages == 0;
    // The two onsets in flight: how old each is, and how hard the first
    // of them hit. Exactly zero means none.
    let (age_now, strength, age_prev) =
        (face.said.bands[0], face.said.bands[1], face.said.bands[2]);

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.chart,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.chart.left() + 8.0, lay.chart.top() + font.size + 6.0),
        egui::pos2(
            lay.chart.right() - 8.0,
            lay.chart.bottom() - font.size - 6.0,
        ),
    );
    // The delay at every octave, and the worst of them, which sets the
    // window the chart is drawn in.
    let delay_at = |hz: f32| sc::group_delay_ms(&shape, DRAWN_AT, hz).max(0.0);
    let worst = (0..=48)
        .map(|i| delay_at(HZ_MIN * (HZ_MAX / HZ_MIN).powf(i as f32 / 48.0)))
        .fold(0.0f32, f32::max);
    let span = span_for(worst);
    let x_at = |hz: f32| field.left() + place_of_hz(hz) * field.width();
    let y_at = |ms: f32| field.top() + (ms / span).clamp(0.0, 1.0) * field.height();

    for hz in [100.0f32, 1_000.0, 10_000.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x_at(hz), field.top()),
                egui::pos2(x_at(hz), field.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.28),
        );
    }
    // Zero: the line a sound arrives on when nothing has been done to it.
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), y_at(0.0)),
            egui::pos2(field.right(), y_at(0.0)),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.85),
    );
    let curve: Vec<egui::Pos2> = (0..=120)
        .map(|i| {
            let t = i as f32 / 120.0;
            let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
            egui::pos2(field.left() + t * field.width(), y_at(delay_at(hz)))
        })
        .collect();
    // The wavefronts: a line at each onset's age, and everything above
    // it has already come out. The newest is the bright one.
    for (age, tone, weight) in [
        (age_prev, alpha.live_dim.color, Weight::Hair),
        (age_now, alpha.live.color, Weight::Heavy),
    ] {
        if age <= 0.0 || wire {
            continue;
        }
        let y = y_at(age);
        if age <= span {
            chrome::trace(
                &mut shapes,
                &[egui::pos2(field.left(), y), egui::pos2(field.right(), y)],
                weight,
                tone,
            );
        }
    }
    // What has arrived: the part of the chart above the newest front.
    if age_now > 0.0 && !wire {
        let front = y_at(age_now);
        let mut arrived: Vec<egui::Pos2> = curve
            .iter()
            .copied()
            .map(|p| egui::pos2(p.x, p.y.min(front)))
            .collect();
        arrived.push(egui::pos2(field.right(), field.top()));
        arrived.push(egui::pos2(field.left(), field.top()));
        shapes.push(egui::Shape::convex_polygon(
            arrived,
            alpha.live.color.gamma_multiply(0.20),
            egui::Stroke::NONE,
        ));
    }
    chrome::curve(
        &mut shapes,
        &curve,
        Weight::Heavy,
        if wire { edge.gamma_multiply(0.9) } else { ink },
    );
    // The corner the run turns at, standing on its own curve.
    if !wire {
        chrome::pad(
            &mut shapes,
            egui::pos2(x_at(centre), y_at(delay_at(centre))),
            chrome::PAD,
            alpha.jeopardy_latent.color,
            true,
        );
    }
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "SMEAR");
    let span_of = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["AMOUNT", "CENTRE", "NOW", "PREV", "HIT"]
        .into_iter()
        .map(span_of)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span_of("8.0k") + 6.0;
    words.text(
        egui::pos2(lay.chart.left() + 8.0, lay.chart.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "ARRIVAL",
        edge,
    );
    words.text(
        egui::pos2(lay.chart.right() - 8.0, lay.chart.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else {
            format!("0-{span:.0}ms")
        },
        if wire { edge } else { alpha.live.color },
    );
    for (hz, word) in [(100.0f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        words.text(
            egui::pos2(x_at(hz), lay.chart.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            word,
            edge,
        );
    }
    for (rect, word, share, said, tone) in [
        (
            lay.amount,
            "AMOUNT",
            amount / 32.0,
            format!("{amount:.0}"),
            alpha.live.color,
        ),
        (
            lay.centre,
            "CENTRE",
            place_of_hz(centre),
            if centre >= 1000.0 {
                format!("{:.1}k", centre / 1000.0)
            } else {
                format!("{centre:.0}")
            },
            alpha.jeopardy_latent.color,
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
    // The onsets, measured: nothing here comes from a knob.
    let ch = painter
        .layout_no_wrap("M".to_owned(), font.clone(), ink)
        .rect
        .height();
    for (i, (word, said, tone)) in [
        (
            "NOW",
            if age_now > 0.0 {
                format!("{age_now:.0}ms")
            } else {
                "--".to_owned()
            },
            alpha.live.color,
        ),
        (
            "HIT",
            if age_now > 0.0 {
                format!("{:.0}%", strength * 100.0)
            } else {
                "--".to_owned()
            },
            alpha.jeopardy_latent.color,
        ),
        (
            "PREV",
            if age_prev > 0.0 {
                format!("{age_prev:.0}ms")
            } else {
                "--".to_owned()
            },
            alpha.live_dim.color,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = lay.said.top() + ch * 0.5 + i as f32 * (ch + 2.0);
        if y + ch * 0.5 > lay.said.bottom() {
            break;
        }
        words.text(
            egui::pos2(lay.said.left() + 2.0, y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        let quiet = said == "--";
        words.text(
            egui::pos2(lay.said.right() - 2.0, y),
            egui::Align2::RIGHT_CENTER,
            said,
            if quiet { edge } else { tone },
        );
    }
    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 210.0))
    }

    #[test]
    fn every_smear_control_has_its_own_row() {
        let face = smear_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 2);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its row");
            assert!(
                !face.chart.intersects(*rect),
                "control {id} invaded the chart"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(!rect.intersects(*other_rect), "{id} and {other} overlap");
            }
        }
        assert!(!face.said.intersects(face.centre));
        // The chart is nearly the whole card: two controls do not need
        // a column of their own width.
        assert!(face.chart.width() > glass().width() * 0.55);
    }

    /// What the section really does: it holds back what is near its
    /// CORNER, not what is low. The card was nearly drawn as a ramp
    /// from high to low, which is the story an allpass smear is usually
    /// told with and is not what the coefficients do.
    #[test]
    fn the_corner_arrives_last() {
        use crate::console::SectionKind;
        use crate::console::smear_curve as sc;
        use crate::params::console::smear as p;
        let mut params = crate::console::SectionParams::of(SectionKind::Smear);
        params.set(p::AMOUNT, 16.0);
        params.set(p::CENTRE, 1_000.0);
        let shape = sc::Shape::of(&params);
        let at = |hz: f32| sc::group_delay_ms(&shape, 48_000.0, hz);
        // The corner is held back hardest, and by a long way.
        assert!(
            at(1_000.0) > at(8_000.0) * 2.0,
            "the corner was not held back: {} against {}",
            at(1_000.0),
            at(8_000.0)
        );
        assert!(
            at(1_000.0) > at(100.0) * 2.0,
            "the corner was not held back below it either"
        );
        // The ends of the spectrum come out nearly on time — which is
        // what makes this a chirp rather than a delay.
        assert!(at(20_000.0) < at(1_000.0) * 0.25);
    }

    /// A wire delays nothing, and more sections delay more.
    #[test]
    fn no_sections_is_no_delay() {
        use crate::console::SectionKind;
        use crate::console::smear_curve as sc;
        use crate::params::console::smear as p;
        let mut params = crate::console::SectionParams::of(SectionKind::Smear);
        let wire = sc::Shape::of(&params);
        for hz in [100.0f32, 1_000.0, 10_000.0] {
            assert!(
                sc::group_delay_ms(&wire, 48_000.0, hz).abs() < 1e-3,
                "a wire held {hz} back"
            );
        }
        let mut worst = 0.0f32;
        for stages in [4.0f32, 8.0, 16.0, 32.0] {
            params.set(p::AMOUNT, stages);
            let shape = sc::Shape::of(&params);
            let here = sc::group_delay_ms(&shape, 48_000.0, 1_000.0);
            assert!(here > worst, "{stages} sections held back no further");
            worst = here;
        }
    }

    /// The window steps to the chart it holds rather than standing still
    /// or following it exactly, and never shrinks under the smallest.
    #[test]
    fn the_window_steps_to_the_delay() {
        for worst in [0.0f32, 0.5, 1.5, 4.0, 18.0, 90.0, 500.0] {
            let span = span_for(worst);
            assert!(SPANS_MS.contains(&span), "{span} is not one of the steps");
            assert!(span >= worst || span == SPANS_MS[SPANS_MS.len() - 1]);
        }
        assert_eq!(span_for(0.0), SPANS_MS[0]);
        // A delay exactly on a step gets the next one up, so a curve
        // never runs along the foot of the chart.
        assert!(span_for(SPANS_MS[1]) > SPANS_MS[1]);
        // Monotone: more delay never gives a smaller window.
        let mut last = 0.0;
        for worst in [0.0f32, 1.0, 3.0, 10.0, 40.0, 150.0, 900.0] {
            let span = span_for(worst);
            assert!(span >= last);
            last = span;
        }
    }
}
