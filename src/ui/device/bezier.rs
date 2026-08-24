//! Multipurpose cubic bezier math. Pure — no egui, no theme, no engine —
//! so the envelope editor, future curve displays, and tests all share one
//! implementation.
//!
//! Everything here is bounded: fixed-step flattening, fixed-iteration
//! solving. Green-zone code, but written so a display path can call it
//! every frame without surprises.

/// A 2D point. Deliberately not an egui type: this module has no UI
/// dependency, and callers convert at the paint site.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }
}

/// Cubic bezier segment: endpoints `p0`/`p3`, control points `p1`/`p2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cubic {
    pub p0: Pt,
    pub p1: Pt,
    pub p2: Pt,
    pub p3: Pt,
}

impl Cubic {
    /// A bendable segment from `(x0, y0)` to `(x1, y1)` — the envelope
    /// curve primitive.
    ///
    /// `bend` is `-1..=1`: `0` is a straight line, positive bows toward the
    /// destination early (fast start, the shape of an exponential attack),
    /// negative arrives late (slow start, the shape of an exponential
    /// decay viewed from the other end). Control points sit at fixed
    /// x-thirds, so x(t) is strictly monotonic and [`y_at_x`](Self::y_at_x)
    /// is always well-defined; `|bend| <= 1` also keeps y monotonic.
    pub fn segment(x0: f32, y0: f32, x1: f32, y1: f32, bend: f32) -> Self {
        let b = bend.clamp(-1.0, 1.0);
        let third = 1.0 / 3.0;
        let ly = |t: f32| y0 + (y1 - y0) * t.clamp(0.0, 1.0);
        Self {
            p0: Pt::new(x0, y0),
            p1: Pt::new(x0 + (x1 - x0) * third, ly(third * (1.0 + b))),
            p2: Pt::new(x0 + (x1 - x0) * 2.0 * third, ly(1.0 - third * (1.0 - b))),
            p3: Pt::new(x1, y1),
        }
    }

    /// Point at parameter `t` (clamped to `0..=1`), by de Casteljau.
    pub fn eval(&self, t: f32) -> Pt {
        let t = t.clamp(0.0, 1.0);
        let a = Pt::lerp(self.p0, self.p1, t);
        let b = Pt::lerp(self.p1, self.p2, t);
        let c = Pt::lerp(self.p2, self.p3, t);
        let ab = Pt::lerp(a, b, t);
        let bc = Pt::lerp(b, c, t);
        Pt::lerp(ab, bc, t)
    }

    /// `y` at a given `x`, for segments whose `x(t)` is monotonic (any
    /// curve built by [`segment`](Self::segment)). Outside the segment's
    /// x-span, clamps to the nearer endpoint's y. Fixed-iteration
    /// bisection: bounded, derivative-free, immune to flat spots.
    pub fn y_at_x(&self, x: f32) -> f32 {
        let (xa, xb) = (self.p0.x, self.p3.x);
        if (xb - xa).abs() <= f32::EPSILON {
            return self.p0.y;
        }
        let ascending = xb > xa;
        if (ascending && x <= xa) || (!ascending && x >= xa) {
            return self.p0.y;
        }
        if (ascending && x >= xb) || (!ascending && x <= xb) {
            return self.p3.y;
        }
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        const ITERS: usize = 24; // halves the span to ~6e-8: below f32 ulp
        for _ in 0..ITERS {
            let mid = 0.5 * (lo + hi);
            let px = self.eval(mid).x;
            if (px < x) == ascending {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        self.eval(0.5 * (lo + hi)).y
    }

    /// Append `steps + 1` evenly-parameterized points to `out` (including
    /// both endpoints). The paint-site helper: convert to positions there.
    pub fn polyline(&self, steps: usize, out: &mut Vec<Pt>) {
        let steps = steps.max(1);
        out.reserve(steps + 1);
        for i in 0..=steps {
            out.push(self.eval(i as f32 / steps as f32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_exact() {
        let c = Cubic::segment(2.0, -1.0, 10.0, 3.0, 0.7);
        assert_eq!(c.eval(0.0), Pt::new(2.0, -1.0));
        assert_eq!(c.eval(1.0), Pt::new(10.0, 3.0));
    }

    #[test]
    fn zero_bend_is_a_straight_line() {
        let c = Cubic::segment(0.0, 0.0, 8.0, 4.0, 0.0);
        for i in 0..=16 {
            let t = i as f32 / 16.0;
            let p = c.eval(t);
            // On the line y = x/2.
            assert!((p.y - p.x * 0.5).abs() < 1e-5, "off-line at t={t}");
        }
        assert!((c.y_at_x(4.0) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn bend_bows_the_expected_way() {
        let fast = Cubic::segment(0.0, 0.0, 1.0, 1.0, 0.8);
        let slow = Cubic::segment(0.0, 0.0, 1.0, 1.0, -0.8);
        // Fast start is above the diagonal at midpoint, slow start below.
        assert!(fast.y_at_x(0.5) > 0.6);
        assert!(slow.y_at_x(0.5) < 0.4);
    }

    #[test]
    fn y_at_x_solves_and_clamps() {
        let c = Cubic::segment(1.0, 0.0, 5.0, 1.0, 0.5);
        // Solved y matches eval at the recovered parameter, everywhere.
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            let p = c.eval(t);
            assert!((c.y_at_x(p.x) - p.y).abs() < 1e-3, "mismatch at t={t}");
        }
        // Outside the span: endpoint y, no extrapolation.
        assert_eq!(c.y_at_x(0.0), 0.0);
        assert_eq!(c.y_at_x(9.0), 1.0);
    }

    #[test]
    fn y_is_monotonic_within_bend_range() {
        for bend in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            let c = Cubic::segment(0.0, 1.0, 1.0, 0.0, bend);
            let mut prev = c.eval(0.0).y;
            for i in 1..=32 {
                let y = c.eval(i as f32 / 32.0).y;
                assert!(y <= prev + 1e-5, "non-monotone at bend {bend}");
                prev = y;
            }
        }
    }

    #[test]
    fn polyline_includes_both_ends() {
        let c = Cubic::segment(0.0, 0.0, 1.0, 1.0, 0.3);
        let mut pts = Vec::new();
        c.polyline(16, &mut pts);
        assert_eq!(pts.len(), 17);
        assert_eq!(pts[0], Pt::new(0.0, 0.0));
        assert_eq!(pts[16], Pt::new(1.0, 1.0));
    }

    #[test]
    fn degenerate_zero_width_segment_never_hangs_or_nans() {
        let c = Cubic::segment(3.0, 0.25, 3.0, 0.75, 0.5);
        assert_eq!(c.y_at_x(3.0), 0.25);
        assert!(c.eval(0.5).y.is_finite());
    }
}
