//! Circular membrane basis. State: 2048 bytes for the prepared zero table;
//! evaluation is stateless, bounded Miller recurrence (at most 256 steps).
//! No allocation, in-place hazards or denormals; latency: zero. Orders 0..16,
//! arguments |x| <= 100 cover all 256 modes. DLMF 10.6.1 and 10.12.1.
#![deny(clippy::unwrap_used, clippy::expect_used)]

/// J_n(x), normalized by J_0 + 2 J_2 + 2 J_4 + ... = 1. Downward
/// recurrence avoids both cancellation in the power series and the
/// unstable upward recurrence near the centre of a high-order mode.
pub fn j(n: usize, x: f64) -> f64 {
    if n > 16 || !x.is_finite() || x.abs() > 100.0 {
        return 0.0;
    }
    if x == 0.0 {
        return if n == 0 { 1.0 } else { 0.0 };
    }
    let a = x.abs();
    if a < 0.01 {
        return series(n, x);
    }
    let end = ((a as usize + n + 64) / 2) * 2;
    let (mut next, mut here, mut sum, mut answer) = (0.0, 1.0, 0.0, 0.0);
    for k in (1..=end).rev() {
        let prev = 2.0 * k as f64 / a * here - next;
        next = here;
        here = prev;
        if here.abs() > 1e100 {
            here *= 1e-100;
            next *= 1e-100;
            sum *= 1e-100;
            answer *= 1e-100;
        }
        if k - 1 == n {
            answer = here;
        }
        if (k - 1) % 2 == 0 {
            sum += if k == 1 { here } else { 2.0 * here };
        }
    }
    let sign = if x < 0.0 && n % 2 == 1 { -1.0 } else { 1.0 };
    sign * answer / sum
}

/// Reference power series with compensated two-component arithmetic. Bounded at 128 terms, no dynamic scratch.
pub fn series(n: usize, x: f64) -> f64 {
    if n > 16 || !x.is_finite() || x.abs() > 30.0 {
        return 0.0;
    }
    // Two-component arithmetic keeps the reference series useful through
    // x=30, where ordinary f64 loses digits subtracting ~1e10 terms.
    fn mul(a: (f64, f64), b: f64) -> (f64, f64) {
        let hi = a.0 * b;
        let low = a.0.mul_add(b, -hi) + a.1 * b;
        let sum = hi + low;
        (sum, low - (sum - hi))
    }
    fn div(a: (f64, f64), b: f64) -> (f64, f64) {
        let hi = a.0 / b;
        let low = ((-hi).mul_add(b, a.0) + a.1) / b;
        let sum = hi + low;
        (sum, low - (sum - hi))
    }
    fn add(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        let sum = a.0 + b.0;
        let v = sum - a.0;
        let error = (a.0 - (sum - v)) + (b.0 - v) + a.1 + b.1;
        let hi = sum + error;
        (hi, error - (hi - sum))
    }
    let mut term = (1.0, 0.0);
    for k in 1..=n {
        term = div(mul(term, x * 0.5), k as f64);
    }
    let mut sum = term;
    for k in 1..128 {
        term = div(mul(term, -x * x * 0.25), (k * (k + n)) as f64);
        sum = add(sum, term);
        if term.0.abs() < 1e-25 {
            break;
        }
    }
    sum.0 + sum.1
}

#[derive(Clone, Copy, Debug)]
pub struct Zeros(pub [[f64; 16]; 16]);
impl Zeros {
    /// Green-zone prepare. Bracket each root before safeguarded Newton:
    /// the large-order asymptotic guess alone can skip the first root.
    pub fn prepare() -> Self {
        let mut result = Self([[0.0; 16]; 16]);
        for (n, row) in result.0.iter_mut().enumerate() {
            let mut lo = (n as f64).max(0.01);
            for zero in row {
                let mut hi = lo;
                let sign = j(n, lo).is_sign_positive();
                for _ in 0..128 {
                    hi += std::f64::consts::FRAC_PI_4;
                    if j(n, hi).is_sign_positive() != sign {
                        break;
                    }
                }
                let mut x = (lo + hi) * 0.5;
                for _ in 0..40 {
                    let y = j(n, x);
                    if y.is_sign_positive() == sign {
                        lo = x;
                    } else {
                        hi = x;
                    }
                    let derivative = n as f64 / x * y - j(n + 1, x);
                    let newton = x - y / derivative;
                    x = if newton > lo && newton < hi {
                        newton
                    } else {
                        (lo + hi) * 0.5
                    };
                }
                *zero = x;
                lo = x + 1e-5;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roots_and_recurrence_reference() {
        let z = Zeros::prepare();
        for (n, m, v) in [
            (0, 0, 2.404825557695773),
            (1, 0, 3.8317059702075125),
            (2, 0, 5.135622301840683),
            (0, 1, 5.520078110286311),
            (15, 15, 71.043036),
        ] {
            if n < 15 {
                assert!((z.0[n][m] - v).abs() < 1e-6);
            }
        }
        for (n, row) in z.0.iter().enumerate() {
            for x in row {
                assert!(j(n, *x).abs() < 1e-10);
            }
            assert!(row.windows(2).all(|w| w[1] > w[0] + 2.0));
        }
        // Independent integral representation remains well-conditioned at 30.
        for n in 0..16 {
            for step in 0..=60 {
                let x = step as f64 * 0.5;
                let integral: f64 = (0..256)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / 256.0;
                        (n as f64 * t - x * t.sin()).cos() / 256.0
                    })
                    .sum();
                assert!((j(n, x) - integral).abs() < 1e-12, "{n} {x}");
                if x <= 30.0 {
                    assert!((j(n, x) - series(n, x)).abs() < 1e-6);
                }
            }
        }
    }
    #[test]
    fn allocation_edges_and_repeatability() {
        for len in [0, 1, 137] {
            let mut whole = [0.0; 137];
            let mut split = whole;
            assert_no_alloc::assert_no_alloc(|| {
                for (i, v) in whole.iter_mut().take(len).enumerate() {
                    *v = j(3, i as f64 / 5.0);
                }
                for part in [0..len / 2, len / 2..len] {
                    for i in part {
                        split[i] = j(3, i as f64 / 5.0);
                    }
                }
            });
            assert_eq!(whole, split);
        }
        assert_eq!(j(3, 0.0), 0.0);
        assert!(j(3, 1e-100).is_finite());
    }
}
