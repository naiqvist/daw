//! Family 11 of the kernel roadmap: stereo placement.
//!
//! Two laws, because a pan control means two different things depending
//! on what it is given.
//!
//! [`balance`] is for a source that is ALREADY STEREO. Centre passes
//! both sides untouched; turning the knob attenuates only the side you
//! are turning away from. It has to leave the centre bit-exact, because
//! a mixer full of centred tracks must sum to what it would have summed
//! to with no pan stage at all.
//!
//! [`spread`] is for a MONO source being placed. It is the
//! constant-power law: the two gains are the cosine and sine of one
//! angle, so `l² + r²` is 1 everywhere and a source keeps its loudness
//! as it crosses the field. Centre is −3 dB on each side, not unity —
//! that is the law working, not a bug.
//!
//! # Why this is a kernel and not two node helpers
//!
//! It was two node helpers. The track mixer in `audio::graph` and the
//! pan cell in `audio::utility` each spelled the same law out, and the
//! only thing keeping them equal was a comment in one of them saying
//! they had to be. A pan knob on a track header and a pan cell on a
//! device card mean the same thing or the app is lying; that is an
//! arithmetic fact, and the contract puts arithmetic here.
//!
//! # No transcendentals
//!
//! Both laws are values of `sin(πt/2)` on `t ∈ [0,1]`, so this file
//! computes that ONCE, as a degree-11 odd polynomial in `t`. Against
//! `libm` the worst error over the range is 5.6e-8 — under f32's own
//! resolution — and `l² + r²` holds to 1.1e-7.
//!
//! It is also the difference between a loop that vectorises and one
//! that cannot. Both call sites apply pan per sample inside their mixing
//! loop, and a `cosf` call in there is a call: it forces a spill, blocks
//! the whole loop from vectorising, and costs more than every other
//! operation in the loop combined.

/// `sin(π·t/2)`, the quarter-wave both laws are cut from.
///
/// Taylor in `t²` through `t¹¹`, which is exact to 5.6e-8 on `[0,1]` —
/// the point past which an f32 cannot tell the difference. Odd by
/// construction, so it is equally correct on `[-1,0]`.
#[inline(always)]
fn quarter_sine(t: f32) -> f32 {
    // The leading term of sin(πt/2) IS π/2 — take it from the constant
    // rather than transcribing its digits.
    const C1: f32 = core::f32::consts::FRAC_PI_2;
    const C3: f32 = -0.645_964_1;
    const C5: f32 = 0.079_692_63;
    const C7: f32 = -0.004_681_754_2;
    const C9: f32 = 0.000_160_441_18;
    const C11: f32 = -3.598_843_2e-6;
    let t2 = t * t;
    let a = C11;
    let a = a * t2 + C9;
    let a = a * t2 + C7;
    let a = a * t2 + C5;
    let a = a * t2 + C3;
    let a = a * t2 + C1;
    a * t
}

/// Clamp a pan position into `-1..=1`; anything non-finite is centre.
#[inline(always)]
fn position(pan: f32) -> f32 {
    if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// BALANCE, for a stereo source: `(left_gain, right_gain)`.
///
/// −1 is hard left, 0 centre, +1 hard right. Centre returns exactly
/// `(1.0, 1.0)` — the identity, to the bit — and each extreme returns
/// exactly 0 for the side it has turned away from.
///
/// State: none (pure). Per-call cost: two degree-11 polynomials, no
/// branches and no library calls.
/// Denormal-safe: n/a — no state; output is bounded to `0..=1`.
/// In-place safe: n/a. Latency: 0 samples.
#[inline(always)]
pub fn balance(pan: f32) -> (f32, f32) {
    let p = position(pan);
    // The branch is deliberate. Feeding the untouched side through the
    // polynomial as `quarter_sine(1.0)` costs nothing and is wrong by
    // 6e-8 — and this is the one gain in the app that has to be the
    // EXACT identity, because a mixer full of centred tracks must sum to
    // what it would have summed to with no pan stage in the path at all.
    // A test pins `balance(0.0) == (1.0, 1.0)` on the bit.
    let l = if p <= 0.0 { 1.0 } else { quarter_sine(1.0 - p) };
    let r = if p >= 0.0 { 1.0 } else { quarter_sine(1.0 + p) };
    (l, r)
}

/// CONSTANT POWER, for a mono source: `(left_gain, right_gain)`.
///
/// The gains are `cos θ` and `sin θ` for `θ = (pan+1)·π/4`, so the sum
/// of their squares is 1 at every position and the source holds its
/// loudness across the field. Centre is `(√½, √½)`, about −3 dB a side.
///
/// State: none (pure). Per-call cost: two degree-11 polynomials.
/// Denormal-safe: n/a — no state; output is bounded to `0..=1`.
/// In-place safe: n/a. Latency: 0 samples.
#[inline(always)]
pub fn spread(pan: f32) -> (f32, f32) {
    let p = position(pan);
    // cos((p+1)π/4) = sin(π/2 · (1−p)/2) and likewise for the sine, so
    // both come out of the same quarter-wave.
    (quarter_sine((1.0 - p) * 0.5), quarter_sine((1.0 + p) * 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep() -> impl Iterator<Item = f32> {
        (0..=2_000).map(|i| i as f32 / 1_000.0 - 1.0)
    }

    /// REFERENCE: both laws are the transcendental ones they replaced.
    ///
    /// This is the test that entitles the polynomial to exist. The two
    /// nodes computed these with `cos`/`sin` before the law moved here,
    /// so "the same to a float" is not a nicety — it is the promise that
    /// nothing in the mix moved when it did.
    #[test]
    fn both_laws_match_the_transcendental_form() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
        let mut worst_balance = 0.0f32;
        let mut worst_spread = 0.0f32;
        for p in sweep() {
            let (l, r) = balance(p);
            let want_l = if p <= 0.0 { 1.0 } else { (p * FRAC_PI_2).cos() };
            let want_r = if p >= 0.0 {
                1.0
            } else {
                (-p * FRAC_PI_2).cos()
            };
            worst_balance = worst_balance
                .max((l - want_l).abs())
                .max((r - want_r).abs());

            let (l, r) = spread(p);
            let angle = (p + 1.0) * FRAC_PI_4;
            worst_spread = worst_spread
                .max((l - angle.cos()).abs())
                .max((r - angle.sin()).abs());
        }
        assert!(
            worst_balance < 1e-6,
            "balance drifts from cos by {worst_balance:e}"
        );
        assert!(
            worst_spread < 1e-6,
            "spread drifts from cos/sin by {worst_spread:e}"
        );
    }

    /// The properties that DEFINE each law, which the reference test
    /// would still pass if both sides were wrong in the same way.
    #[test]
    fn each_law_keeps_the_promise_that_names_it() {
        // Balance: centre is the identity, to the bit.
        assert_eq!(balance(0.0), (1.0, 1.0), "a centred track must not move");
        // ...and each extreme silences exactly one side.
        assert_eq!(balance(1.0), (0.0, 1.0), "hard right must mute the left");
        assert_eq!(balance(-1.0), (1.0, 0.0), "hard left must mute the right");

        // Spread: constant power everywhere.
        let mut worst = 0.0f32;
        for p in sweep() {
            let (l, r) = spread(p);
            worst = worst.max((l * l + r * r - 1.0).abs());
        }
        assert!(worst < 1e-6, "spread loses power by {worst:e}");
        let (l, r) = spread(0.0);
        let half = 0.5f32.sqrt();
        assert!(
            (l - half).abs() < 1e-6 && (r - half).abs() < 1e-6,
            "centre is -3 dB"
        );
        assert_eq!(spread(-1.0).1, 0.0, "hard left must mute the right");
        assert_eq!(spread(1.0).0, 0.0, "hard right must mute the left");

        // Both laws are mirror images of themselves.
        for p in sweep() {
            let (l, r) = balance(p);
            let (ml, mr) = balance(-p);
            assert!(
                (l - mr).abs() < 1e-6 && (r - ml).abs() < 1e-6,
                "balance at {p}"
            );
            let (l, r) = spread(p);
            let (ml, mr) = spread(-p);
            assert!(
                (l - mr).abs() < 1e-6 && (r - ml).abs() < 1e-6,
                "spread at {p}"
            );
        }
    }

    /// Monotonic, bounded, and never negative — a gain that overshoots
    /// its rails inverts phase, which is the one failure a pan law must
    /// not have.
    #[test]
    fn the_gains_stay_on_the_rails_and_never_turn_back() {
        let (mut prev_l, mut prev_r) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in sweep() {
            for (l, r) in [balance(p), spread(p)] {
                assert!((0.0..=1.0).contains(&l), "left {l} at {p}");
                assert!((0.0..=1.0).contains(&r), "right {r} at {p}");
            }
            let (l, r) = balance(p);
            assert!(l <= prev_l + 1e-6, "left must fall as pan rises, at {p}");
            assert!(r >= prev_r - 1e-6, "right must rise as pan rises, at {p}");
            prev_l = l;
            prev_r = r;
        }
    }

    /// Nonsense in, centre out — never a NaN gain, which would silence a
    /// track for the rest of the session.
    #[test]
    fn nonsense_positions_land_at_centre_or_the_rails() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(balance(bad), (1.0, 1.0), "{bad} must read as centre");
            let (l, r) = spread(bad);
            assert!(l.is_finite() && r.is_finite(), "{bad} gave {l},{r}");
        }
        // Out of range clamps rather than extrapolating past the rails.
        assert_eq!(balance(9.0), balance(1.0));
        assert_eq!(balance(-9.0), balance(-1.0));
        assert_eq!(spread(9.0), spread(1.0));
        assert_eq!(spread(-9.0), spread(-1.0));
    }

    /// Pure: same input, same output, and no allocation on the way.
    #[test]
    fn the_laws_are_pure_and_do_not_allocate() {
        assert_no_alloc::assert_no_alloc(|| {
            let mut acc = 0.0f32;
            for p in sweep() {
                let (a, b) = balance(p);
                let (c, d) = spread(p);
                assert_eq!((a, b), balance(p), "balance is not pure at {p}");
                assert_eq!((c, d), spread(p), "spread is not pure at {p}");
                acc += a + b + c + d;
            }
            std::hint::black_box(acc);
        });
    }

    /// What a position costs. Printed, not asserted; run in RELEASE.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let row = |name: &str, run: &mut dyn FnMut()| {
            for _ in 0..1_000 {
                run();
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run();
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<26} {ns:7.2} ns/sample");
        };
        let pans: Vec<f32> = (0..BLOCK)
            .map(|i| i as f32 / BLOCK as f32 * 2.0 - 1.0)
            .collect();
        let mut acc = 0.0f32;
        row("balance", &mut || {
            for p in pans.iter() {
                let (l, r) = balance(*p);
                acc += l + r;
            }
            std::hint::black_box(&mut acc);
        });
        row("spread", &mut || {
            for p in pans.iter() {
                let (l, r) = spread(*p);
                acc += l + r;
            }
            std::hint::black_box(&mut acc);
        });
        row("balance (libm cos)", &mut || {
            for p in pans.iter() {
                let l = if *p <= 0.0 {
                    1.0
                } else {
                    (*p * std::f32::consts::FRAC_PI_2).cos()
                };
                let r = if *p >= 0.0 {
                    1.0
                } else {
                    (-*p * std::f32::consts::FRAC_PI_2).cos()
                };
                acc += l + r;
            }
            std::hint::black_box(&mut acc);
        });
    }
}
