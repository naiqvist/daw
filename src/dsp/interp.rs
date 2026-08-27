//! Fractional table reads — the arithmetic a sampler's play head is made
//! of, and nothing else.
//!
//! Two functions and one convenience over a slice. There is no state, no
//! `prepare` and no `reset`, because an interpolator has no memory: it is
//! a weighted sum of four numbers you already have.
//!
//! # What is deliberately NOT here
//!
//! Looping, reversing, region ends, slice boundaries, channel strides.
//! Every one of those is POLICY, and a kernel that knew about them would
//! be a sampler wearing a kernel's clothes. The caller owns the position
//! and hands over the neighbours; this file owns the weights.
//!
//! # Why Catmull-Rom
//!
//! Linear interpolation of pitched-down material is audibly dull — it is
//! a lowpass whose corner moves with the pitch ratio, which is a filter
//! nobody asked for. The 4-point/3rd-order Hermite (Catmull-Rom) form
//! costs a handful of flops more and reproduces every polynomial up to a
//! QUADRATIC exactly, so the error it does make is high-order and quiet.
//! (It does not reproduce cubics — its tangents are centred differences,
//! and for `x³` the centred difference at zero is 1 where the derivative
//! is 0. Claiming cubic exactness is the easy mistake here and the test
//! below is written to catch it.)
//!
//! It also does the one thing this device's contract rests on: at a
//! fractional position of exactly zero it returns the centre sample
//! untouched, bit for bit. That is what lets the sampler claim it can be
//! transparent when every colour stage is at the bottom of its range —
//! an interpolator with a windowed-sinc kernel could not, because at
//! unity ratio it would still be convolving.
//!
//! State: none.
//! Per-sample cost: `linear` 1 mul + 2 add; `hermite4` 3 mul + 8 add/mul
//! for the coefficients, then 3 fma.
//! Denormal-safe: pure arithmetic on finite inputs.
//! In-place safe: not applicable — nothing is written.
//! Latency: 0 samples.

/// Two-point linear read. `frac` outside `0..=1` extrapolates, which is
/// the caller's business and not an error.
#[inline(always)]
pub fn linear(y0: f32, y1: f32, frac: f32) -> f32 {
    y0 + (y1 - y0) * frac
}

/// Four-point Catmull-Rom read between `y0` and `y1`.
///
/// `ym1` is the sample before `y0` and `y2` the one after `y1`. At
/// `frac == 0.0` this returns `y0` bit-exactly: the polynomial's constant
/// term IS `y0` and every other term carries a factor of `frac`.
#[inline(always)]
pub fn hermite4(ym1: f32, y0: f32, y1: f32, y2: f32, frac: f32) -> f32 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
    ((c3 * frac + c2) * frac + c1) * frac + c0
}

/// Read `table` at a fractional position, holding the end samples past
/// either edge.
///
/// HOLD, not zero-pad: a play head that walks off the end of its material
/// should meet the last sample, not a step to silence. Silence at a
/// boundary is a fade's job, and the fade knows how long it wants to
/// take.
///
/// A position that is not finite reads as zero rather than panicking. An
/// empty table reads as zero. Neither can happen from a correct caller,
/// and neither is worth a branch in the caller to prevent.
#[inline]
pub fn hermite_at(table: &[f32], pos: f64) -> f32 {
    if table.is_empty() || !pos.is_finite() {
        return 0.0;
    }
    let last = table.len() - 1;
    let base = pos.floor();
    let frac = (pos - base) as f32;
    // `base` can be enormous or negative; saturating both ways keeps the
    // index arithmetic inside `usize` without a cast that could wrap.
    let i0 = if base <= 0.0 {
        0
    } else if base >= last as f64 {
        last
    } else {
        base as usize
    };
    let at = |i: usize| *table.get(i.min(last)).unwrap_or(&0.0);
    let ym1 = at(i0.saturating_sub(1));
    let y0 = at(i0);
    let y1 = at(i0 + 1);
    let y2 = at(i0 + 2);
    // Past either edge there is nothing to interpolate towards, so the
    // held sample is the answer and the weights must not be allowed to
    // wobble around it.
    if base <= 0.0 || base >= last as f64 {
        return y0;
    }
    hermite4(ym1, y0, y1, y2, frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference correctness, half one: a linear read of a ramp is the
    /// ramp.
    #[test]
    fn linear_reproduces_a_ramp() {
        for step in 0..=10 {
            let frac = step as f32 / 10.0;
            assert!((linear(3.0, 4.0, frac) - (3.0 + frac)).abs() < 1e-6);
        }
    }

    /// Reference correctness, half two: Catmull-Rom reproduces every
    /// polynomial up to a QUADRATIC exactly. That is the property that
    /// says the weights are right — get one coefficient wrong and a
    /// parabola is the first thing that stops fitting.
    ///
    /// A cubic is deliberately NOT asserted: the tangents are centred
    /// differences, so `x³` comes back with the wrong slope at every
    /// knot. If someone "improves" this to cubic exactness they will have
    /// changed which interpolator it is.
    #[test]
    fn hermite_reproduces_a_quadratic_exactly() {
        let f = |x: f64| -1.1 * x * x + 0.5 * x + 0.25;
        for step in 0..=20 {
            let frac = step as f64 / 20.0;
            let got = hermite4(
                f(-1.0) as f32,
                f(0.0) as f32,
                f(1.0) as f32,
                f(2.0) as f32,
                frac as f32,
            );
            let want = f(frac) as f32;
            assert!(
                (got - want).abs() < 1e-5,
                "quadratic at {frac}: got {got}, want {want}"
            );
        }
    }

    /// The property the whole device's identity claim rests on.
    #[test]
    fn a_read_at_zero_fraction_is_the_sample_itself() {
        for (a, b, c, d) in [
            (0.1f32, -0.7, 0.33, 0.9),
            (-1.0, 1.0, -1.0, 1.0),
            (0.0, 1e-30, 0.0, -1e-30),
            (12.5, -3.25, 7.0, 0.125),
        ] {
            assert_eq!(hermite4(a, b, c, d, 0.0), b, "frac 0 must return y0");
            assert_eq!(linear(b, c, 0.0), b);
        }
        // And through the slice door, at an interior position.
        let table: Vec<f32> = (0..16).map(|i| (i as f32).sin()).collect();
        for i in 1..15 {
            assert_eq!(hermite_at(&table, i as f64), table[i]);
        }
    }

    /// Split-block equivalence is trivially true — there is no state — so
    /// the equivalent property is that the same arguments give the same
    /// answer regardless of what was read before.
    #[test]
    fn reads_are_independent_of_each_other() {
        let table: Vec<f32> = (0..64).map(|i| (i as f32 * 0.31).sin()).collect();
        let forward: Vec<f32> = (0..600)
            .map(|i| hermite_at(&table, i as f64 * 0.1))
            .collect();
        let backward: Vec<f32> = (0..600)
            .rev()
            .map(|i| hermite_at(&table, i as f64 * 0.1))
            .collect();
        for (i, (f, b)) in forward.iter().zip(backward.iter().rev()).enumerate() {
            assert_eq!(f, b, "read {i} depended on read order");
        }
    }

    #[test]
    fn reading_does_not_allocate() {
        let table: Vec<f32> = (0..256).map(|i| (i as f32 * 0.07).sin()).collect();
        let mut out = vec![0.0f32; 1024];
        assert_no_alloc::assert_no_alloc(|| {
            for (i, s) in out.iter_mut().enumerate() {
                *s = hermite_at(&table, i as f64 * 0.213);
            }
        });
    }

    /// Edge lengths: nothing here may panic, whatever it is handed.
    #[test]
    fn every_degenerate_table_reads_as_a_number() {
        assert_eq!(hermite_at(&[], 0.0), 0.0);
        assert_eq!(hermite_at(&[0.5], 0.0), 0.5);
        assert_eq!(hermite_at(&[0.5], 100.0), 0.5);
        assert_eq!(hermite_at(&[0.5], -100.0), 0.5);
        let two = [1.0f32, 2.0];
        assert_eq!(hermite_at(&two, 0.0), 1.0);
        assert_eq!(hermite_at(&two, 1.0), 2.0);
        // A non-power-of-two table, read all the way past both ends.
        let odd: Vec<f32> = (0..37).map(|i| i as f32).collect();
        for pos in [-1e9, -1.0, 0.0, 18.5, 36.0, 1e9] {
            assert!(
                hermite_at(&odd, pos).is_finite(),
                "pos {pos} was not finite"
            );
        }
        assert_eq!(hermite_at(&odd, 1e9), 36.0, "past the end holds the end");
        assert_eq!(hermite_at(&odd, -1e9), 0.0, "before the start holds it");
    }

    /// Silence in, silence out — and a NaN position is answered with a
    /// number rather than propagated.
    #[test]
    fn silence_reads_silent_and_nonsense_reads_zero() {
        let quiet = vec![0.0f32; 64];
        for i in 0..600 {
            assert_eq!(hermite_at(&quiet, i as f64 * 0.1), 0.0);
        }
        let table: Vec<f32> = (0..16).map(|i| i as f32).collect();
        assert_eq!(hermite_at(&table, f64::NAN), 0.0);
        assert_eq!(hermite_at(&table, f64::INFINITY), 0.0);
    }
}
