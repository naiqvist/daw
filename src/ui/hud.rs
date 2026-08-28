//! The HUD vocabulary: angular marks, each with one job.
//!
//! The house style is already square — `tokens::radius` is zero
//! throughout — so "angular" is not a corner radius to change. What was
//! missing is a set of MARKS that read as instrument chrome rather than
//! as office chrome, and the rule this module is built under is that a
//! mark earns its place by carrying a state that had no picture, or by
//! carrying an existing one for less ink.
//!
//! Nothing here is atmosphere. Two marks, two jobs:
//!
//! - [`brackets`] — corner ticks. THE FOCUS RING. A closed rectangle
//!   drawn around a control covers the control's own outline, so on a
//!   dense surface "this is focused" and "this is selected" arrive as
//!   the same closed box in two colours, and in greyscale as one box.
//!   Brackets mark the same rectangle with about a quarter of the ink,
//!   at the four points that fix a rectangle unambiguously — so focus
//!   becomes a different SHAPE from selection rather than a different
//!   colour, and both can be read at once.
//!
//! - [`hatch`] — diagonal fill. SIGNAL DOES NOT PASS HERE. Dimming is
//!   the usual answer and it is the wrong one: dim reads as "far away"
//!   or "not important", which is what an inactive control looks like
//!   too. A hatch reads as struck through, which is what a refused or
//!   bypassed thing IS — present, and not in the path.
//!
//! Both are geometry-first: the shapes come from pure functions the
//! tests can measure, and the painters are three lines on top. A mark
//! whose arithmetic cannot be asserted is a mark that drifts.

use eframe::egui;

// ---------------------------------------------------------- brackets ---

/// How much of the shorter side one arm reaches along.
///
/// A FIFTH, and the fifth is arithmetic rather than taste. Eight arms
/// against a square's perimeter is `8 · f · s / 4s`, which is `2f`
/// whatever the square's size — so at a third the mark would ink 60 %
/// of the way round and the whole argument for brackets over a closed
/// ring would be false at every square size at once. The ink test
/// caught exactly that, and this is the number that makes the claim
/// true.
///
/// It also has to stay under a half so two arms never meet and close
/// the box back into the ring it replaces.
pub const ARM_FRAC: f32 = 0.2;
pub const ARM_MIN: f32 = 3.0;
pub const ARM_MAX: f32 = 10.0;

/// Under this, on either side, the brackets would be four dots and the
/// closed rectangle says it better.
///
/// A mark that stops being legible below a size has to say what that
/// size is, or every caller discovers it separately by drawing
/// something illegible.
pub const BRACKET_FLOOR: f32 = 15.0;

/// How far each arm runs from its corner.
pub fn arm_len(rect: egui::Rect) -> f32 {
    let short = rect.width().min(rect.height()).max(0.0);
    (short * ARM_FRAC).clamp(ARM_MIN, ARM_MAX).min(short * 0.5)
}

/// Whether `rect` is big enough for brackets to read as brackets.
pub fn is_bracketed(rect: egui::Rect) -> bool {
    rect.width() >= BRACKET_FLOOR && rect.height() >= BRACKET_FLOOR
}

/// The four corner paths, each `[arm end, corner, arm end]`, clockwise
/// from the top left.
///
/// The middle point of every path is EXACTLY a corner of `rect`: the
/// mark has to fix the same rectangle the ring did, or focus and the
/// thing focused stop agreeing about where the thing is.
pub fn bracket_paths(rect: egui::Rect) -> [[egui::Pos2; 3]; 4] {
    let a = arm_len(rect);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    [
        [egui::pos2(l, t + a), egui::pos2(l, t), egui::pos2(l + a, t)],
        [egui::pos2(r - a, t), egui::pos2(r, t), egui::pos2(r, t + a)],
        [egui::pos2(r, b - a), egui::pos2(r, b), egui::pos2(r - a, b)],
        [egui::pos2(l + a, b), egui::pos2(l, b), egui::pos2(l, b - a)],
    ]
}

/// What fraction of the rectangle's perimeter the brackets actually ink.
///
/// The claim this mark is made on, as a number a test can hold: eight
/// arms against the whole way round.
pub fn ink_fraction(rect: egui::Rect) -> f32 {
    let perimeter = (rect.width() + rect.height()) * 2.0;
    if perimeter <= 0.0 {
        return 0.0;
    }
    (8.0 * arm_len(rect) / perimeter).min(1.0)
}

/// Draw the corner brackets.
pub fn brackets(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    for path in bracket_paths(rect) {
        painter.add(egui::Shape::line(path.to_vec(), stroke));
    }
}

// ------------------------------------------------------------- hatch ---

/// Points between one diagonal and the next.
///
/// Wide enough that the ground still reads through it — a hatch dense
/// enough to become a fill is a fill, and says "this is a different
/// surface" rather than "this is struck out".
pub const HATCH_STEP: f32 = 6.0;

/// Where the diagonals fall inside `rect`, as `(from, to)` pairs at 45°.
///
/// Returned rather than drawn so the geometry is testable: a hatch that
/// leaked past its rectangle would paint over a neighbour, and that is
/// exactly the kind of thing nobody notices until it is shipped.
pub fn hatch_lines(rect: egui::Rect, step: f32) -> Vec<(egui::Pos2, egui::Pos2)> {
    let step = if step.is_finite() && step > 0.5 {
        step
    } else {
        HATCH_STEP
    };
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    // A 45° line hits the top edge at `x` and the left edge at `x - h`,
    // so sweeping the intercept from `-h` to `w` covers the whole
    // rectangle with no gap at either corner.
    let mut out = Vec::new();
    let mut c = -h;
    // Bounded by construction: the count is (w + h) / step, and step is
    // floored above.
    while c <= w {
        let from = clip(rect, c, 0.0, h);
        let to = clip(rect, c, w, h);
        if let (Some(from), Some(to)) = (from, to) {
            out.push((from, to));
        }
        c += step;
    }
    out
}

/// One end of the diagonal with intercept `c`, clamped into the rect.
fn clip(rect: egui::Rect, c: f32, at_x: f32, h: f32) -> Option<egui::Pos2> {
    // y = x - c along the line; the two ends are wherever that leaves
    // the box.
    let x = at_x.clamp(c.max(0.0), (c + h).min(rect.width()));
    let y = x - c;
    if !(0.0..=h).contains(&y) {
        return None;
    }
    Some(egui::pos2(rect.left() + x, rect.top() + y))
}

/// Draw the hatch, clipped to its own rectangle.
pub fn hatch(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let painter = painter.with_clip_rect(rect);
    for (from, to) in hatch_lines(rect, HATCH_STEP) {
        painter.line_segment([from, to], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(13.0, 7.0), egui::vec2(w, h))
    }

    /// AN ARM NEVER REACHES THE FAR CORNER. Two that met would close the
    /// box, which is the ring this replaces — the mark would silently
    /// stop being a different shape at some size and nobody would know
    /// which size.
    #[test]
    fn an_arm_never_reaches_the_far_corner() {
        for (w, h) in [(15.0, 15.0), (20.0, 60.0), (400.0, 22.0), (900.0, 700.0)] {
            let r = rect(w, h);
            let a = arm_len(r);
            assert!(a * 2.0 <= r.width() + 1e-3, "{w}x{h}: arms meet across");
            assert!(a * 2.0 <= r.height() + 1e-3, "{w}x{h}: arms meet down");
            assert!(a >= ARM_MIN.min(w.min(h) * 0.5), "{w}x{h}: arm vanished");
        }
    }

    /// THE INK CLAIM, as a number. Brackets exist because they cost less
    /// than a closed ring; if that stops being true the mark has no
    /// argument left.
    #[test]
    fn brackets_ink_far_less_than_a_closed_ring() {
        for (w, h) in [(15.0, 15.0), (24.0, 24.0), (120.0, 28.0), (600.0, 400.0)] {
            let f = ink_fraction(rect(w, h));
            assert!(
                f < 0.5,
                "{w}x{h}: brackets ink {:.0}% of the perimeter",
                f * 100.0
            );
            assert!(f > 0.0, "{w}x{h}: brackets ink nothing");
        }
        // And on the shape a control actually is — a row, a cell, a
        // field — it is a fraction of the ring, which is the whole
        // point of the mark rather than an incidental property of it.
        assert!(
            ink_fraction(rect(120.0, 28.0)) < 0.25,
            "brackets on an ordinary control cost as much as the ring"
        );
    }

    /// The mark fixes the SAME rectangle: every path's middle point is a
    /// corner, exactly, and the arms run inward along the edges.
    #[test]
    fn every_bracket_sits_on_a_corner_and_runs_inward() {
        let r = rect(80.0, 40.0);
        let corners = [
            egui::pos2(r.left(), r.top()),
            egui::pos2(r.right(), r.top()),
            egui::pos2(r.right(), r.bottom()),
            egui::pos2(r.left(), r.bottom()),
        ];
        for (path, corner) in bracket_paths(r).into_iter().zip(corners) {
            assert_eq!(path[1], corner, "a bracket left its corner");
            for end in [path[0], path[2]] {
                assert!(r.contains(end), "an arm ran outside the rectangle it marks");
                // One axis moves, the other does not: an arm is an edge
                // run, not a diagonal.
                let moved = [(end.x - corner.x).abs(), (end.y - corner.y).abs()];
                assert!(
                    moved[0] < 1e-3 || moved[1] < 1e-3,
                    "an arm left its edge: {moved:?}"
                );
            }
        }
    }

    /// FOCUS IS A DIFFERENT SHAPE FROM SELECTION, not a different
    /// colour. Below the floor it falls back to the closed ring, and the
    /// floor is stated rather than discovered.
    #[test]
    fn the_fallback_floor_is_where_brackets_stop_reading() {
        assert!(is_bracketed(rect(BRACKET_FLOOR, BRACKET_FLOOR)));
        assert!(!is_bracketed(rect(BRACKET_FLOOR - 1.0, 40.0)));
        assert!(!is_bracketed(rect(40.0, BRACKET_FLOOR - 1.0)));
        // At the floor there is still a real arm, or the fallback is in
        // the wrong place.
        assert!(arm_len(rect(BRACKET_FLOOR, BRACKET_FLOOR)) >= ARM_MIN);
    }

    /// THE TWO FOCUS MARKS AGREE. The travelling keyboard cursor stands
    /// off its element by `focus::RING_PAD` and a widget's own ring by
    /// `stroke::FOCUS`, so the two ask this question about slightly
    /// different rectangles — and if the floor fell between them, the
    /// same control would be bracketed by one mark and boxed by the
    /// other depending on which drew it.
    ///
    /// It cannot: any element at least `BRACKET_FLOOR` across is
    /// bracketed under either pad, and the house minimum for a pointer
    /// target on a dense screen is larger still.
    #[test]
    fn both_focus_marks_bracket_the_same_controls() {
        for pad in [2.0f32, 3.0] {
            for side in [BRACKET_FLOOR, BRACKET_FLOOR + 1.0, 20.0, 240.0] {
                assert!(
                    is_bracketed(rect(side, side).expand(pad)),
                    "a {side} pt control is not bracketed at a {pad} pt stand-off"
                );
            }
        }
    }

    #[test]
    fn a_degenerate_rectangle_marks_nothing_and_does_not_panic() {
        for (w, h) in [(0.0f32, 0.0f32), (0.0, 30.0), (30.0, 0.0), (-4.0, 12.0)] {
            let r = rect(w, h);
            assert!(!is_bracketed(r));
            assert_eq!(ink_fraction(r) > 0.0, w > 0.0 && h > 0.0);
            assert!(hatch_lines(r, HATCH_STEP).is_empty());
        }
    }

    /// THE HATCH STAYS INSIDE ITS OWN RECTANGLE. It is drawn over live
    /// surfaces that have neighbours a point away.
    #[test]
    fn the_hatch_never_leaves_its_rectangle() {
        for (w, h) in [(9.0, 40.0), (60.0, 18.0), (200.0, 200.0)] {
            let r = rect(w, h);
            let lines = hatch_lines(r, HATCH_STEP);
            assert!(!lines.is_empty(), "{w}x{h}: nothing was hatched");
            for (from, to) in &lines {
                for at in [from, to] {
                    assert!(
                        r.expand(1e-3).contains(*at),
                        "{w}x{h}: a hatch line reached {at:?}, outside {r:?}"
                    );
                }
                // 45°, both ways round: a hatch that drifted off the
                // diagonal would read as a texture rather than as a
                // strike-through.
                let d = *to - *from;
                assert!(
                    (d.x - d.y).abs() < 1e-2,
                    "{w}x{h}: a hatch line is not at 45°"
                );
            }
        }
    }

    /// It covers the whole rectangle — corners included, which is where
    /// a naive sweep leaves a triangle bare.
    #[test]
    fn the_hatch_reaches_every_corner() {
        let r = rect(60.0, 40.0);
        let lines = hatch_lines(r, HATCH_STEP);
        let near = |corner: egui::Pos2| {
            lines.iter().any(|(from, to)| {
                from.distance(corner) <= HATCH_STEP * 1.5 || to.distance(corner) <= HATCH_STEP * 1.5
            })
        };
        for corner in [
            r.left_top(),
            r.right_top(),
            r.left_bottom(),
            r.right_bottom(),
        ] {
            assert!(near(corner), "no hatch within a step of {corner:?}");
        }
    }

    /// A nonsense step falls back rather than looping forever or
    /// drawing a solid fill — this runs in a paint path.
    #[test]
    fn a_nonsense_step_falls_back_to_the_token() {
        let r = rect(50.0, 50.0);
        let sane = hatch_lines(r, HATCH_STEP).len();
        for step in [0.0f32, -3.0, f32::NAN, f32::INFINITY] {
            assert_eq!(hatch_lines(r, step).len(), sane, "step {step} was obeyed");
        }
    }
}
