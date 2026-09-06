//! RING's face: where the partials land, on a scale in hertz.
//!
//! Every other spectrum on this desk is drawn in octaves, because that
//! is how the ear hears a filter move. This one is drawn in HERTZ, and
//! the difference is the whole section. Ring modulation is the one
//! effect that moves every partial by the same number of hertz rather
//! than the same ratio: a note at 220 with a carrier at 300 comes out at
//! 80 and 520, its second harmonic at 140 and 740, and nothing about
//! that is a musical interval. Drawn in octaves it would look like a
//! spectrum being smeared. Drawn in hertz it is obvious — the input's
//! partials are a comb of equal steps, and the output's are not.
//!
//! So the chart shows both: the note going in as a dim comb, the note
//! coming out as the sum and the difference of every one of its
//! partials with the carrier. That inharmonic scatter is the bell, and
//! it is arithmetic rather than an assertion.
//!
//! The carrier is marked where it stands, and it stands where the
//! section says it does rather than where the knob is — HOLD walks the
//! pitch on its own clock, so the mark moves and the reading with it.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
const COLUMN_GAP: f32 = 8.0;
/// The probe note the chart is drawn for, in Hz, and how many of its
/// partials are shown.
///
/// A stated note, not the one playing: the card cannot know what is
/// going through it, and a chart that guessed would be a chart that
/// lied. This is the arithmetic of the section, shown on a note whose
/// pitch is written on the glass.
const PROBE_HZ: f32 = 220.0;
const PARTIALS: usize = 6;
/// The scales the chart steps through, in Hz.
///
/// Fixed at the widest, a low carrier puts every partial in the first
/// fifth of the chart and the rest is empty. Fitted exactly, the scatter
/// looks the same at every setting. So it steps, like the EQ's window
/// and the smear's, and the one in force is written on the glass.
const SCALES_HZ: [f32; 4] = [1_500.0, 3_000.0, 6_000.0, 12_000.0];
/// The four carriers, in the order the parameter numbers them.
/// Three letters each, so the four cells are one width and the row
/// cannot come out crowded at one end.
const CARRIERS: [&str; 4] = ["SIN", "TRI", "SQR", "NSE"];

/// RING's four controls.
#[derive(Clone, Copy, Debug)]
struct RingFace {
    chart: egui::Rect,
    carrier: egui::Rect,
    hz: egui::Rect,
    hold: egui::Rect,
    mix: egui::Rect,
}

impl Layout for RingFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::ring as p;
        vec![
            (p::CARRIER, self.carrier),
            (p::HZ, self.hz),
            (p::HOLD_RATE, self.hold),
            (p::MIX, self.mix),
        ]
    }
}

fn ring_face(glass: egui::Rect) -> RingFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let right_w = (x.width() * 0.32).clamp(140.0, 210.0);
    let chart = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right() - right_w - COLUMN_GAP, x.bottom()),
    );
    let right = egui::Rect::from_min_max(egui::pos2(x.right() - right_w, x.top()), x.max);
    let rows_h = ROW_H * 4.0 + 3.0;
    let top = (right.bottom() - rows_h).max(right.top());
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    RingFace {
        chart,
        carrier: row(0),
        hz: row(1),
        hold: row(2),
        mix: row(3),
    }
}

/// The scale that holds `highest` with room above it.
fn scale_for(highest: f32) -> f32 {
    SCALES_HZ
        .into_iter()
        .find(|top| highest * 1.12 <= *top)
        .unwrap_or(SCALES_HZ[SCALES_HZ.len() - 1])
}

/// Where a frequency stands across a chart drawn to `top`. LINEAR, on
/// purpose: this is the one section whose arithmetic is in hertz.
fn place_of_hz(hz: f32, top: f32) -> f32 {
    (hz / top.max(1.0)).clamp(0.0, 1.0)
}

/// What comes out when a note of `note` hertz meets a carrier of
/// `carrier` hertz: the sum and the difference of every partial.
///
/// Every partial produces two, and a difference that falls through zero
/// comes back up as its own absolute value — which is why a carrier
/// above the fundamental puts energy BELOW it, and why the result is
/// inharmonic rather than transposed.
fn partials(note: f32, carrier: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(PARTIALS * 2);
    for n in 1..=PARTIALS {
        let f = note * n as f32;
        out.push((f - carrier).abs());
        out.push(f + carrier);
    }
    out
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::ring as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = ring_face(face.glass);
    let carrier = face.value(p::CARRIER).round().clamp(0.0, 3.0) as usize;
    let set_hz = face.value(p::HZ);
    let hold = face.value(p::HOLD_RATE);
    let mix = face.value(p::MIX) / 100.0;
    let wire = mix <= 0.0;
    // Where the carrier ACTUALLY is: the hold walks it, so the knob is
    // only where it started.
    let live_hz = if face.said.bands[0] > 0.0 {
        face.said.bands[0]
    } else {
        set_hz
    };
    let depth = face.said.bands[1];

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
    // The scale comes from the partials, so the scatter fills the chart
    // whatever the carrier is set to.
    let top = scale_for(
        partials(PROBE_HZ, live_hz)
            .into_iter()
            .fold(PROBE_HZ * PARTIALS as f32, f32::max),
    );
    let x_at = |hz: f32| field.left() + place_of_hz(hz, top) * field.width();
    // A kilohertz grid, because the scale is in hertz and a decade grid
    // on a linear axis would be three lines bunched at the left.
    let khz = (top / 1000.0).ceil() as usize;
    for k in 1..khz {
        let x = x_at(k as f32 * 1000.0);
        chrome::trace(
            &mut shapes,
            &[egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
            Weight::Hair,
            edge.gamma_multiply(0.25),
        );
    }
    let floor = field.bottom();
    // The note going in: a comb of equal steps, which is what harmonic
    // means and what the output is about to stop being.
    for n in 1..=PARTIALS {
        let hz = PROBE_HZ * n as f32;
        let height = field.height() * 0.42 / (n as f32).sqrt();
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x_at(hz), floor),
                egui::pos2(x_at(hz), floor - height),
            ],
            Weight::Hair,
            edge.gamma_multiply(1.2),
        );
    }
    // What comes out: every partial's sum and difference with the
    // carrier. Not a smear of the comb — a different comb entirely.
    if !wire {
        for (i, hz) in partials(PROBE_HZ, live_hz).into_iter().enumerate() {
            if hz > top {
                continue;
            }
            let n = (i / 2 + 1) as f32;
            let height = field.height() * 0.78 / n.sqrt();
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(x_at(hz), floor),
                    egui::pos2(x_at(hz), floor - height * mix.max(0.15)),
                ],
                Weight::Heavy,
                if i % 2 == 0 {
                    // The difference tones: the ones that fall below the
                    // fundamental and make the sound hollow.
                    alpha.jeopardy_latent.color
                } else {
                    alpha.live.color
                },
            );
        }
    }
    // The carrier itself, where it stands this instant.
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(x_at(live_hz), field.top()),
            egui::pos2(x_at(live_hz), floor),
        ],
        Weight::Hair,
        if wire { edge } else { alpha.live_dim.color },
    );
    chrome::pad(
        &mut shapes,
        egui::pos2(x_at(live_hz), field.top() + 4.0),
        chrome::PAD,
        if wire { edge } else { alpha.live_dim.color },
        !wire,
    );
    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "RING");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["CARRIER", "HZ", "HOLD", "MIX"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span("5000") + 6.0;
    words.text(
        egui::pos2(lay.chart.left() + 8.0, lay.chart.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "PARTIALS",
        edge,
    );
    words.text(
        egui::pos2(lay.chart.right() - 8.0, lay.chart.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else {
            format!("{PROBE_HZ:.0}Hz NOTE")
        },
        if wire { edge } else { alpha.live.color },
    );
    // The scale's own figures: the ends, and the middle of it.
    for (place, said) in [
        (0.0f32, "0".to_owned()),
        (0.5, format!("{:.1}k", top / 2000.0)),
        (1.0, format!("{:.1}k", top / 1000.0)),
    ] {
        words.text(
            egui::pos2(
                field.left() + place * field.width(),
                lay.chart.bottom() - 2.0,
            ),
            match place {
                p if p <= 0.0 => egui::Align2::LEFT_BOTTOM,
                p if p >= 1.0 => egui::Align2::RIGHT_BOTTOM,
                _ => egui::Align2::CENTER_BOTTOM,
            },
            said,
            edge,
        );
    }
    for (i, word) in CARRIERS.into_iter().enumerate() {
        let cell_w = lay.carrier.width() / CARRIERS.len() as f32;
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.carrier.left() + i as f32 * cell_w, lay.carrier.top()),
            egui::vec2(cell_w, lay.carrier.height()),
        );
        if i == carrier {
            let mut marks = Vec::new();
            chrome::brackets(&mut marks, cell.shrink(2.0), 3.0, Weight::Hair, ink);
            painter.extend(marks);
        }
        words.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            word,
            if i == carrier { ink } else { edge },
        );
    }
    for (rect, word, share, said, tone) in [
        (
            lay.hz,
            "HZ",
            place_of_hz(live_hz, top),
            format!("{live_hz:.0}"),
            alpha.live_dim.color,
        ),
        (
            lay.hold,
            "HOLD",
            hold / 50.0,
            if hold <= 0.0 {
                "OFF".to_owned()
            } else {
                format!("{hold:.1}")
            },
            alpha.jeopardy_latent.color,
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
    let _ = depth;
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
    fn every_ring_control_has_its_own_row() {
        let face = ring_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 4);
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
    }

    /// The claim the whole card is built on: what comes out is NOT a
    /// harmonic series. The input's partials are equally spaced and the
    /// output's are not, and that is the difference between a bell and
    /// a transposition.
    #[test]
    fn what_comes_out_is_inharmonic() {
        let out = {
            let mut out = partials(PROBE_HZ, 300.0);
            out.sort_by(f32::total_cmp);
            out
        };
        // The input is a comb: every gap is the fundamental.
        for n in 2..=PARTIALS {
            let gap = PROBE_HZ * n as f32 - PROBE_HZ * (n - 1) as f32;
            assert!((gap - PROBE_HZ).abs() < 0.01, "the probe was not harmonic");
        }
        // The output is not: no single step divides all of its gaps.
        let gaps: Vec<f32> = out.windows(2).map(|w| w[1] - w[0]).collect();
        let first = gaps[0];
        assert!(
            gaps.iter().any(|gap| (gap - first).abs() > 1.0),
            "the output came out evenly spaced: {gaps:?}"
        );
    }

    /// A carrier above the fundamental puts energy BELOW it, because a
    /// difference that falls through zero comes back up as its own
    /// absolute value. That is why ring modulation sounds hollow and
    /// why the difference tones are drawn in their own ink.
    #[test]
    fn a_high_carrier_folds_below_the_fundamental() {
        let out = partials(PROBE_HZ, PROBE_HZ * 1.5);
        assert!(
            out.iter().any(|hz| *hz < PROBE_HZ && *hz > 0.0),
            "nothing landed under the note: {out:?}"
        );
        // Nothing is ever negative: the fold is an absolute value.
        assert!(out.iter().all(|hz| *hz >= 0.0));
        // Every partial produces exactly two.
        assert_eq!(out.len(), PARTIALS * 2);
    }

    /// The scale is LINEAR, which is the one place this desk departs
    /// from octaves — and it is not a slip: the section's arithmetic is
    /// in hertz, so a chart of it has to be too.
    #[test]
    fn the_scale_is_in_hertz_not_octaves() {
        // Equal steps in hertz are equal steps across the chart.
        let top = SCALES_HZ[2];
        let a = place_of_hz(1_000.0, top) - place_of_hz(500.0, top);
        let b = place_of_hz(3_500.0, top) - place_of_hz(3_000.0, top);
        assert!((a - b).abs() < 1e-5, "{a} against {b}");
        assert_eq!(place_of_hz(0.0, top), 0.0);
        assert_eq!(place_of_hz(top, top), 1.0);
        assert_eq!(place_of_hz(top * 2.0, top), 1.0);
        // And the scale steps to what it has to hold.
        for highest in [400.0f32, 1_400.0, 2_900.0, 5_000.0, 40_000.0] {
            let scale = scale_for(highest);
            assert!(SCALES_HZ.contains(&scale));
            assert!(scale >= highest || scale == SCALES_HZ[SCALES_HZ.len() - 1]);
        }
    }
}
