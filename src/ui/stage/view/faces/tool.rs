//! The instruments the faces share.
//!
//! Twenty-six cards, one desk. What more than one section needs lives
//! here so the run reads as one machine and not as twenty-six demos:
//! the octave axis and the decibel axis a filter is drawn on, the
//! response curve taken from real coefficients, the stack of small
//! squares, the arc and its needle, the travelling slider, the stepped
//! rota, the ladder, the comb, the iris, the beat grid.
//!
//! A section's SIGNATURE is what it does with these, not a
//! twenty-seventh kind of knob.
//!
//! Every function here takes a `&mut Vec<Shape>` and adds to it, the
//! way [`crate::ui::chrome`] does, so a face builds one batch and
//! hands it to the painter once.

use super::*;
use crate::ui::chrome;
use eframe::egui::{Color32, Pos2, Rect, Shape, Vec2, pos2, vec2};

// ─── the axes a filter is drawn on ────────────────────────────────────

/// The band the desk draws a spectrum across. Chosen once, so TONE's
/// axis, CUT's axis and FOUR's axis are the same axis and a corner at
/// 1 kHz stands in the same place on all three.
pub const LOW_HZ: f32 = 30.0;
pub const HIGH_HZ: f32 = 18_000.0;

/// Where `hz` falls across `field`, by OCTAVES rather than by hertz —
/// which is how the ear reads a spectrum and how a band that sweeps
/// should move.
pub fn octave_x(field: Rect, hz: f32) -> f32 {
    octave_x_in(field, hz, LOW_HZ, HIGH_HZ)
}

/// The same, across a span of the caller's own choosing: a control
/// that only reaches 200 Hz to 6 kHz should use the whole rail.
pub fn octave_x_in(field: Rect, hz: f32, low: f32, high: f32) -> f32 {
    let span = (high / low).log2().max(f32::EPSILON);
    let at = (hz.max(1.0) / low).log2() / span;
    egui::lerp(field.x_range(), at.clamp(0.0, 1.0))
}

/// The inverse: what frequency the hand is pointing at.
pub fn hz_at(field: Rect, x: f32, low: f32, high: f32) -> f32 {
    let at = ((x - field.left()) / field.width().max(f32::EPSILON)).clamp(0.0, 1.0);
    low * (high / low).powf(at)
}

/// Where `db` stands on a field whose middle is zero and whose top and
/// bottom are `span` decibels away.
pub fn db_y(field: Rect, db: f32, span: f32) -> f32 {
    let at = (db / span.max(f32::EPSILON)).clamp(-1.0, 1.0);
    field.center().y - at * field.height() * 0.5
}

/// Where a level stands on a field read as a fall from silence at the
/// top to `floor` at the bottom.
pub fn level_y(field: Rect, db: f32, floor: f32) -> f32 {
    let at = ((db - floor) / -floor.min(-f32::EPSILON)).clamp(0.0, 1.0);
    egui::lerp(field.bottom()..=field.top(), at)
}

/// A response curve, sampled across the field's octave axis and drawn
/// on its decibel axis. `at` is handed a frequency in hertz and must
/// answer in decibels — from the section's own coefficients, which is
/// the whole point: what is drawn is what is heard.
pub fn response(field: Rect, steps: usize, span_db: f32, at: impl Fn(f32) -> f32) -> Vec<Pos2> {
    let steps = steps.max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(t);
            pos2(egui::lerp(field.x_range(), t), db_y(field, at(hz), span_db))
        })
        .collect()
}

/// A curve of any shape: `at` maps 0..1 across the field to 0..1 up it,
/// one at the top.
pub fn plot(field: Rect, steps: usize, at: impl Fn(f32) -> f32) -> Vec<Pos2> {
    let steps = steps.max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            pos2(
                egui::lerp(field.x_range(), t),
                egui::lerp(field.bottom()..=field.top(), at(t).clamp(0.0, 1.0)),
            )
        })
        .collect()
}

// ─── the instruments ──────────────────────────────────────────────────

/// A run of small squares from `from`, one every `pitch` along `step`.
/// `lit` of them are bright and the rest are the ghost of what could
/// be lit — which is what makes a quantity readable at a glance
/// without a number beside it.
#[allow(clippy::too_many_arguments)]
pub fn cells(
    out: &mut Vec<Shape>,
    from: Pos2,
    step: Vec2,
    count: usize,
    lit: usize,
    side: f32,
    ink: Color32,
    rest: Color32,
) {
    for i in 0..count {
        let centre = from + step * i as f32;
        let on = i < lit;
        chrome::pad(out, centre, side, if on { ink } else { rest }, on);
    }
}

/// A stack of squares standing on, or hanging from, `anchor`: up for a
/// positive quantity, down for a negative one. Returns where the stack
/// ended, so a caller can hang something else off it.
#[allow(clippy::too_many_arguments)]
pub fn stack(
    out: &mut Vec<Shape>,
    anchor: Pos2,
    amount: f32,
    cells_full: usize,
    side: f32,
    pitch: f32,
    ink: Color32,
    rest: Color32,
) -> Pos2 {
    let amount = amount.clamp(-1.0, 1.0);
    let steps = (amount.abs() * cells_full as f32).round() as usize;
    let dir = if amount < 0.0 { 1.0 } else { -1.0 };
    for i in 0..steps.max(1) {
        let centre = pos2(anchor.x, anchor.y + dir * (i as f32 + 0.5) * pitch);
        let on = i < steps;
        chrome::pad(out, centre, side, if on { ink } else { rest }, on);
    }
    pos2(anchor.x, anchor.y + dir * steps as f32 * pitch)
}

/// A point on a circle, degrees anticlockwise from east — the way a
/// meter's scale is drawn, not the way the screen's y axis runs.
pub fn on_arc(centre: Pos2, radius: f32, degrees: f32) -> Pos2 {
    let r = degrees.to_radians();
    pos2(centre.x + radius * r.cos(), centre.y - radius * r.sin())
}

/// An arc as a path, for tracing.
pub fn arc(centre: Pos2, radius: f32, from: f32, to: f32, steps: usize) -> Vec<Pos2> {
    let steps = steps.max(2);
    (0..=steps)
        .map(|i| on_arc(centre, radius, from + (to - from) * i as f32 / steps as f32))
        .collect()
}

/// A needle from a pivot, with the pad that holds it.
#[allow(clippy::too_many_arguments)]
pub fn needle(
    out: &mut Vec<Shape>,
    pivot: Pos2,
    radius: f32,
    degrees: f32,
    weight: Weight,
    ink: Color32,
    hub: Color32,
) {
    chrome::trace(out, &[pivot, on_arc(pivot, radius, degrees)], weight, ink);
    chrome::pad(out, pivot, chrome::PAD, hub, true);
}

/// A stepped rotary: a ring of `steps` ticks with the chosen one long
/// and bright, and a pointer standing on it. This is the desk's switch
/// — an SSL ratio, a delay division, a saturator's character — and it
/// says how many positions there are as well as which one is chosen.
#[allow(clippy::too_many_arguments)]
pub fn rota(
    out: &mut Vec<Shape>,
    centre: Pos2,
    radius: f32,
    steps: usize,
    at: f32,
    ink: Color32,
    rest: Color32,
) {
    if steps == 0 {
        return;
    }
    let (from, to) = (210.0, -30.0);
    let deg = |i: f32| from + (to - from) * (i / (steps.max(2) - 1) as f32);
    for i in 0..steps {
        let d = deg(i as f32);
        let on = (at - i as f32).abs() < 0.5;
        let inner = if on { radius - 6.0 } else { radius - 3.0 };
        chrome::trace(
            out,
            &[on_arc(centre, inner, d), on_arc(centre, radius, d)],
            if on { Weight::Heavy } else { Weight::Hair },
            if on { ink } else { rest },
        );
    }
    chrome::trace(out, &arc(centre, radius, from, to, 20), Weight::Hair, rest);
    chrome::trace(
        out,
        &[centre, on_arc(centre, radius - 7.0, deg(at))],
        Weight::Heavy,
        ink,
    );
    chrome::pad(out, centre, chrome::PAD - 1.0, ink, true);
}

/// A travelling square on a rail: the rail ruled, the graduations
/// hinted, and one bright cell where the value stands. `at` is 0..1
/// from the rail's start. A wide rail runs left to right; a tall one
/// runs bottom to top.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    out: &mut Vec<Shape>,
    rail: Rect,
    at: f32,
    ticks: usize,
    ink: Color32,
    rest: Color32,
    cell: f32,
) -> Pos2 {
    let at = at.clamp(0.0, 1.0);
    let flat = rail.width() >= rail.height();
    let (a, b) = if flat {
        (rail.left_center(), rail.right_center())
    } else {
        (rail.center_bottom(), rail.center_top())
    };
    chrome::trace(out, &[a, b], Weight::Hair, rest);
    for i in 0..ticks {
        let t = if ticks > 1 {
            i as f32 / (ticks - 1) as f32
        } else {
            0.5
        };
        let p = a + (b - a) * t;
        let n = if flat { vec2(0.0, 2.5) } else { vec2(2.5, 0.0) };
        chrome::trace(out, &[p - n, p + n], Weight::Hair, rest);
    }
    let here = a + (b - a) * at;
    chrome::pad(out, here, cell, ink, true);
    here
}

/// A bipolar rail: the same, but the run between the centre post and
/// the value is what lights, so a boost and a cut of the same size
/// read as mirror images rather than as different amounts.
#[allow(clippy::too_many_arguments)]
pub fn swing_rail(
    out: &mut Vec<Shape>,
    rail: Rect,
    swing: f32,
    segments: usize,
    ink: Color32,
    rest: Color32,
) {
    let swing = swing.clamp(-1.0, 1.0);
    let flat = rail.width() >= rail.height();
    let (a, b) = if flat {
        (rail.left_center(), rail.right_center())
    } else {
        (rail.center_bottom(), rail.center_top())
    };
    let segments = segments.max(3);
    for i in 0..segments {
        let t = i as f32 / (segments - 1) as f32;
        let here = t * 2.0 - 1.0;
        let on = if swing >= 0.0 {
            here >= -0.001 && here <= swing + 0.001
        } else {
            here <= 0.001 && here >= swing - 0.001
        };
        let p = a + (b - a) * t;
        let reach = if i % 4 == 0 { 5.0 } else { 3.0 };
        let n = if flat {
            vec2(0.0, reach)
        } else {
            vec2(reach, 0.0)
        };
        chrome::trace(
            out,
            &[p - n, p + n],
            if on { Weight::Heavy } else { Weight::Hair },
            if on { ink } else { rest },
        );
    }
    let centre = a + (b - a) * 0.5;
    chrome::pad(out, centre, chrome::PAD - 2.0, rest, true);
}

/// A comb: `n` uprights across the rect, each as tall as `height`
/// answers for its place. A spectrum, a tap list, a bit ladder.
pub fn comb(
    out: &mut Vec<Shape>,
    rect: Rect,
    n: usize,
    ink: impl Fn(usize) -> Color32,
    height: impl Fn(usize) -> f32,
) {
    if n == 0 {
        return;
    }
    let pitch = rect.width() / n as f32;
    for i in 0..n {
        let x = rect.left() + pitch * (i as f32 + 0.5);
        let h = (height(i).clamp(0.0, 1.0) * rect.height()).max(1.0);
        chrome::trace(
            out,
            &[pos2(x, rect.bottom()), pos2(x, rect.bottom() - h)],
            Weight::Hair,
            ink(i),
        );
    }
}

/// The beat grid: a row of `n` cells, each filled as far as `open`
/// says, the one the transport is inside marked. The chopper's
/// instrument, and the tempo-locked delay's.
#[allow(clippy::too_many_arguments)]
pub fn beat_grid(
    out: &mut Vec<Shape>,
    rect: Rect,
    n: usize,
    open: f32,
    playing: Option<usize>,
    ink: Color32,
    rest: Color32,
) {
    if n == 0 || !rect.is_positive() {
        return;
    }
    let pitch = rect.width() / n as f32;
    for i in 0..n {
        let cell = Rect::from_min_max(
            pos2(rect.left() + pitch * i as f32 + 1.0, rect.top()),
            pos2(rect.left() + pitch * (i + 1) as f32 - 1.0, rect.bottom()),
        );
        if !cell.is_positive() {
            continue;
        }
        let here = playing == Some(i);
        chrome::trace(
            out,
            &[
                cell.left_top(),
                cell.right_top(),
                cell.right_bottom(),
                cell.left_bottom(),
                cell.left_top(),
            ],
            Weight::Hair,
            if here { ink } else { rest },
        );
        let lit = Rect::from_min_max(
            cell.min,
            pos2(
                cell.left() + cell.width() * open.clamp(0.0, 1.0),
                cell.bottom(),
            ),
        );
        if lit.is_positive() {
            out.push(Shape::rect_filled(
                lit.shrink(1.0),
                0.0,
                if here { ink } else { rest },
            ));
        }
    }
}

/// An iris: two lids meeting across an opening, `open` apart. The
/// gate's doorway, the ducker's shutter, the limiter's aperture — a
/// quantity you can see without reading.
pub fn iris(out: &mut Vec<Shape>, rect: Rect, open: f32, ink: Color32, rest: Color32) {
    let open = open.clamp(0.0, 1.0);
    let half = rect.height() * 0.5 * (1.0 - open);
    chrome::trace(
        out,
        &[
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ],
        Weight::Hair,
        rest,
    );
    for lid in [
        Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + half)),
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - half), rect.max),
    ] {
        if lid.is_positive() {
            out.push(Shape::rect_filled(lid, 0.0, ink));
        }
    }
    let gap = rect.center().y;
    chrome::trace(
        out,
        &[pos2(rect.left(), gap), pos2(rect.right(), gap)],
        Weight::Hair,
        if open > 0.02 { ink } else { rest },
    );
}

/// A halo: the awake ring. An instrument under the hand gets one, so
/// a face can carry a dozen controls and still say which one is live
/// without changing its layout.
pub fn halo(out: &mut Vec<Shape>, rect: Rect, amount: f32, ink: Color32) {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.01 {
        return;
    }
    let grown = rect.expand(1.0 + amount * 2.0);
    chrome::trace(
        out,
        &[
            grown.left_top(),
            grown.right_top(),
            grown.right_bottom(),
            grown.left_bottom(),
            grown.left_top(),
        ],
        Weight::Hair,
        fade(ink, amount * 0.55),
    );
}

/// A recess inside the glass: a sub-screen for one instrument, so a
/// face can have layers rather than one flat plane.
pub fn well(out: &mut Vec<Shape>, rect: Rect, ground: Color32, edge: Color32) {
    if !rect.is_positive() {
        return;
    }
    out.push(Shape::rect_filled(rect, 0.0, ground));
    chrome::panel_frame_variant(out, rect, Weight::Hair, edge, 0);
}

// ─── ink ──────────────────────────────────────────────────────────────

/// `a` at zero, `b` at one.
pub fn mix_ink(a: Color32, b: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

/// Dim, clamped, so a caller never asks for more than there is.
pub fn fade(ink: Color32, amount: f32) -> Color32 {
    ink.gamma_multiply(amount.clamp(0.0, 1.0))
}

/// The three band hues the desk uses wherever a picture is split low,
/// middle and high: warm at the bottom, violet in the middle, the
/// deck's own cyan at the top. TONE set them; SCOPE, SPLIT and FOUR
/// keep them, so a band is the same colour wherever it appears.
pub const LO_INK: Color32 = Color32::from_rgb(255, 138, 62);
pub const MID_INK: Color32 = Color32::from_rgb(186, 122, 255);
pub const HI_INK: Color32 = Color32::from_rgb(139, 245, 255);
pub const BAND_INK: [Color32; 3] = [LO_INK, MID_INK, HI_INK];

// ─── arithmetic a face keeps needing ──────────────────────────────────

/// Amplitude to decibels, floored.
pub fn db_of(amp: f32) -> f32 {
    if amp > 1e-6 {
        20.0 * amp.log10()
    } else {
        -120.0
    }
}

/// Decibels to amplitude.
pub fn amp_of(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// `value` as a share of `min..max`, clamped.
pub fn norm(value: f32, min: f32, max: f32) -> f32 {
    ((value - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_is_octaves_so_a_decade_takes_the_same_room_everywhere() {
        let field = Rect::from_min_size(Pos2::ZERO, vec2(300.0, 100.0));
        let low = octave_x(field, 100.0) - octave_x(field, 50.0);
        let high = octave_x(field, 8_000.0) - octave_x(field, 4_000.0);
        assert!((low - high).abs() < 0.5, "{low} here and {high} there");
        assert!(octave_x(field, 10.0) >= field.left());
        assert!(octave_x(field, 40_000.0) <= field.right());
    }

    #[test]
    fn the_axis_reads_back_what_it_wrote() {
        let field = Rect::from_min_size(pos2(10.0, 0.0), vec2(220.0, 60.0));
        for hz in [40.0, 250.0, 1_000.0, 7_500.0, 16_000.0] {
            let x = octave_x_in(field, hz, LOW_HZ, HIGH_HZ);
            let back = hz_at(field, x, LOW_HZ, HIGH_HZ);
            assert!((back / hz - 1.0).abs() < 0.02, "{hz} came back as {back}");
        }
    }

    #[test]
    fn the_decibel_axis_puts_zero_on_the_middle_and_clamps_at_the_walls() {
        let field = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 80.0));
        assert!((db_y(field, 0.0, 15.0) - field.center().y).abs() < 0.01);
        assert!(db_y(field, 15.0, 15.0) <= field.top() + 0.01);
        assert!(db_y(field, -15.0, 15.0) >= field.bottom() - 0.01);
        assert_eq!(db_y(field, 90.0, 15.0), db_y(field, 15.0, 15.0));
    }

    #[test]
    fn a_response_curve_spans_the_field_and_answers_from_the_caller() {
        let field = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 60.0));
        let flat = response(field, 24, 15.0, |_| 0.0);
        assert_eq!(flat.len(), 25);
        assert!((flat[0].x - field.left()).abs() < 0.01);
        assert!((flat[24].x - field.right()).abs() < 0.01);
        for point in &flat {
            assert!((point.y - field.center().y).abs() < 0.01);
        }
        let rising = response(field, 24, 15.0, |hz| if hz > 1_000.0 { 12.0 } else { 0.0 });
        assert!(rising.last().expect("a point").y < rising[0].y);
    }

    #[test]
    fn a_stack_hangs_down_for_a_cut_and_stands_up_for_a_boost() {
        let mut up = Vec::new();
        let end = stack(
            &mut up,
            pos2(10.0, 50.0),
            1.0,
            8,
            4.0,
            6.0,
            Color32::WHITE,
            Color32::GRAY,
        );
        assert!(end.y < 50.0, "a boost did not climb");
        let mut down = Vec::new();
        let end = stack(
            &mut down,
            pos2(10.0, 50.0),
            -1.0,
            8,
            4.0,
            6.0,
            Color32::WHITE,
            Color32::GRAY,
        );
        assert!(end.y > 50.0, "a cut did not fall");
        assert_eq!(up.len(), down.len());
    }

    #[test]
    fn an_arc_runs_anticlockwise_in_the_way_a_meter_is_read() {
        let centre = pos2(50.0, 50.0);
        let east = on_arc(centre, 10.0, 0.0);
        let north = on_arc(centre, 10.0, 90.0);
        assert!((east.x - 60.0).abs() < 0.01 && (east.y - 50.0).abs() < 0.01);
        assert!((north.y - 40.0).abs() < 0.01, "90 degrees is UP the screen");
    }

    #[test]
    fn the_bipolar_rail_lights_from_the_centre_either_way() {
        let rail = Rect::from_min_size(Pos2::ZERO, vec2(80.0, 10.0));
        let mut up = Vec::new();
        swing_rail(&mut up, rail, 1.0, 9, Color32::WHITE, Color32::GRAY);
        let mut down = Vec::new();
        swing_rail(&mut down, rail, -1.0, 9, Color32::WHITE, Color32::GRAY);
        let mut none = Vec::new();
        swing_rail(&mut none, rail, 0.0, 9, Color32::WHITE, Color32::GRAY);
        assert_eq!(up.len(), down.len(), "a boost and a cut draw alike");
        assert_eq!(up.len(), none.len(), "the rail never changes its ruling");
    }

    #[test]
    fn the_iris_shuts_completely_and_opens_completely() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(40.0, 30.0));
        let mut shut = Vec::new();
        iris(&mut shut, rect, 0.0, Color32::WHITE, Color32::GRAY);
        let mut open = Vec::new();
        iris(&mut open, rect, 1.0, Color32::WHITE, Color32::GRAY);
        assert!(!shut.is_empty() && !open.is_empty());
    }

    #[test]
    fn decibels_and_amplitude_are_inverses() {
        for db in [-40.0, -12.0, -6.0, 0.0, 6.0] {
            assert!((db_of(amp_of(db)) - db).abs() < 0.01);
        }
        assert_eq!(db_of(0.0), -120.0);
    }

    #[test]
    fn the_band_hues_are_distinct_and_ordered_warm_to_cold() {
        assert_ne!(LO_INK, MID_INK);
        assert_ne!(MID_INK, HI_INK);
        assert!(LO_INK.r() > LO_INK.b(), "the low band is warm");
        assert!(HI_INK.b() > HI_INK.r(), "the high band is cold");
    }
}
