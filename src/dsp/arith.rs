//! Family 2 of the kernel roadmap: vector × vector.
//!
//! `add`, `mul` (ring mod), `mac` (multiply-accumulate), `crossfade`,
//! and `dot` — the elementwise binary primitives.
//!
//! Stateless free functions, same conventions as family 1 ([`super::mem`]):
//! zip truncation to the shortest slice (never a panic), no indexing that
//! can be out of bounds, full doc headers, and the five-test battery.
//!
//! Summation order: [`mac`], [`crossfade`], and [`dot`] use explicit
//! `f32::mul_add` in strict sequential order. The single-rounding FMA is
//! pinned instead of left to compiler contraction, and the order is
//! documented because reductions do not split bit-exactly: the sum of the
//! parts rounds differently from the whole. Elementwise kernels DO split
//! bit-exactly; that is the property the transport depends on.

/// Elementwise add: `dst[i] = a[i] + b[i]`, truncating to the shortest
/// of the three slices.
///
/// State: none (stateless). Per-sample cost: 1 add.
/// Denormal-safe: relies on engine FTZ — the sum of two denormals can be
/// denormal. Never invents NaN; NaN in `a` or `b` passes through.
/// In-place safe: n/a — distinct slices by signature.
/// Latency: 0 samples. Split-block: bit-exact.
#[inline]
pub fn add(a: &[f32], b: &[f32], dst: &mut [f32]) {
    for ((x, y), d) in a.iter().zip(b.iter()).zip(dst.iter_mut()) {
        *d = *x + *y;
    }
}

/// Elementwise multiply: `dst[i] = a[i] * b[i]`, truncating to the
/// shortest of the three slices. This is ring modulation: multiplying two
/// signals folds their spectra.
///
/// State: none (stateless). Per-sample cost: 1 mul.
/// Denormal-safe: relies on engine FTZ — the product of two denormals can
/// be denormal or zero. Never invents NaN; NaN passes through.
/// In-place safe: n/a — distinct slices by signature.
/// Latency: 0 samples. Split-block: bit-exact.
#[inline]
pub fn mul(a: &[f32], b: &[f32], dst: &mut [f32]) {
    for ((x, y), d) in a.iter().zip(b.iter()).zip(dst.iter_mut()) {
        *d = *x * *y;
    }
}

/// Multiply-accumulate: `dst[i] += a[i] * b[i]`, truncating to the
/// shortest of the three slices. The general mixing primitive: [`mem::gain_add`](super::mem::gain_add)
/// with a per-sample gain.
///
/// Zero inputs leave `dst` untouched (unless NaN is present).
///
/// State: none (stateless). Per-sample cost: 1 FMA (`f32::mul_add`,
/// single rounding, pinned — never left to compiler contraction).
/// Denormal-safe: relies on engine FTZ — denormal products can stay
/// denormal. Never invents NaN; NaN in `a` or `b` passes through to `dst`.
/// In-place safe: n/a — distinct slices by signature.
/// Latency: 0 samples. Split-block: bit-exact.
#[inline]
pub fn mac(a: &[f32], b: &[f32], dst: &mut [f32]) {
    for ((x, y), d) in a.iter().zip(b.iter()).zip(dst.iter_mut()) {
        *d = x.mul_add(*y, *d);
    }
}

/// Crossfade: `dst[i] = (1 - t) * a[i] + t * b[i]`, truncating to the
/// shortest of the three slices. The wet/dry primitive.
///
/// Endpoint-exact: at `t = 0.0` the output is `a` bit-for-bit, at
/// `t = 1.0` it is `b` bit-for-bit (for finite, nonzero samples — `-0.0`
/// may come back as `+0.0`). `t` outside `0..=1` extrapolates; clamping is
/// the caller's job, wired in front if wanted.
///
/// State: none (stateless). Per-sample cost: 1 FMA + 1 mul (`f32::mul_add`,
/// single rounding, pinned).
/// Denormal-safe: relies on engine FTZ. Never invents NaN; NaN in `a` or
/// `b` passes through regardless of `t` (`0 * NaN` is NaN).
/// In-place safe: n/a — distinct slices by signature.
/// Latency: 0 samples. Split-block: bit-exact.
#[inline]
pub fn crossfade(a: &[f32], b: &[f32], dst: &mut [f32], t: f32) {
    let wa = 1.0 - t;
    for ((x, y), d) in a.iter().zip(b.iter()).zip(dst.iter_mut()) {
        *d = wa.mul_add(*x, *y * t);
    }
}

/// Dot product of the overlapping prefix of `a` and `b`.
///
/// Accumulates with `f32::mul_add` in strict sequential order — pinned,
/// deterministic, documented. Empty input returns `0.0`.
///
/// State: none (stateless). Per-sample cost: 1 FMA.
/// Denormal-safe: relies on engine FTZ — denormal partial sums can occur
/// on decaying inputs. Never invents NaN; NaN passes through to the result.
/// In-place safe: n/a (reads only).
/// Latency: 0 samples.
/// Split-block: NOT bit-exact — a reduction's partial sums round
/// differently from the whole (`dot(a) + dot(b) ≈ dot(a ++ b)`). This is
/// inherent to floating point, not stateful behavior.
#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        acc = x.mul_add(*y, acc);
    }
    acc
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    /// Deterministic pattern for the split-block and no-alloc batteries.
    fn pattern(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.125) % 2.0 - 1.0).collect()
    }

    // ----------------------------------------------------------------- add

    #[test]
    fn add_reference_values() {
        let a = [1.5f32, 2.25, -3.0];
        let b = [0.5f32, 0.75, 3.0];
        let mut dst = [9.0f32; 3];
        add(&a, &b, &mut dst);
        assert_eq!(dst, [2.0, 3.0, 0.0]);
    }

    #[test]
    fn add_truncates_without_panic() {
        let a = [1.0f32, 2.0, 3.0];
        let b = [10.0f32, 20.0];
        let mut dst = [9.0f32; 4];
        add(&a, &b, &mut dst);
        assert_eq!(dst, [11.0, 22.0, 9.0, 9.0]);
    }

    // ----------------------------------------------------------------- mul

    #[test]
    fn mul_reference_values() {
        let a = [2.0f32, 0.5, -1.0];
        let b = [3.0f32, 8.0, 0.25];
        let mut dst = [9.0f32; 3];
        mul(&a, &b, &mut dst);
        assert_eq!(dst, [6.0, 4.0, -0.25]);
    }

    #[test]
    fn mul_truncates_without_panic() {
        let a = [2.0f32, 3.0];
        let b = [4.0f32];
        let mut dst = [9.0f32; 3];
        mul(&a, &b, &mut dst);
        assert_eq!(dst, [8.0, 9.0, 9.0]);
    }

    // ----------------------------------------------------------------- mac

    #[test]
    fn mac_reference_values() {
        let a = [1.0f32, 2.0, 3.0];
        let b = [4.0f32, 5.0, 6.0];
        let mut dst = [10.0f32, 20.0, 30.0];
        mac(&a, &b, &mut dst);
        assert_eq!(dst, [14.0, 30.0, 48.0]);
    }

    #[test]
    fn mac_zero_inputs_leave_dst_untouched() {
        let zero = [0.0f32; 4];
        let mut dst = [1.0f32, -2.0, 3.5, -0.25];
        mac(&zero, &zero, &mut dst);
        assert_eq!(dst, [1.0, -2.0, 3.5, -0.25]);
    }

    // ------------------------------------------------------------ crossfade

    #[test]
    fn crossfade_endpoints_are_bit_exact() {
        // Offset so no sample is ±0.0: the doc header documents that -0.0
        // may come back as +0.0 (0.0 + (-0.0) = +0.0), which is irrelevant
        // for audio but would break a naive bit-exact assertion here.
        let a: Vec<f32> = pattern(37).iter().map(|s| s + 0.125).collect();
        let b: Vec<f32> = pattern(37).iter().map(|s| 0.125 - s).collect();

        let mut at0 = vec![9.0f32; 37];
        crossfade(&a, &b, &mut at0, 0.0);
        assert!(at0.iter().zip(&a).all(|(x, y)| bits(*x) == bits(*y)));

        let mut at1 = vec![9.0f32; 37];
        crossfade(&a, &b, &mut at1, 1.0);
        assert!(at1.iter().zip(&b).all(|(x, y)| bits(*x) == bits(*y)));
    }

    #[test]
    fn crossfade_reference_values() {
        let a = [1.0f32, 3.0];
        let b = [5.0f32, 7.0];
        let mut dst = [9.0f32; 2];

        crossfade(&a, &b, &mut dst, 0.5);
        assert_eq!(dst, [3.0, 5.0]);

        // t outside 0..=1 extrapolates: (1-2)*1 + 2*2 = 3.
        let a2 = [1.0f32];
        let b2 = [2.0f32];
        let mut d2 = [9.0f32];
        crossfade(&a2, &b2, &mut d2, 2.0);
        assert_eq!(d2, [3.0]);
    }

    #[test]
    fn crossfade_against_silence_is_scaled_signal() {
        // Mixing with silence at t is exactly scaling by t — the wet/dry
        // use case: wet at t, dry at 1-t.
        let sig = pattern(16);
        let zero = [0.0f32; 16];
        let mut wet = vec![9.0f32; 16];
        crossfade(&zero, &sig, &mut wet, 0.25);
        let mut scaled = vec![9.0f32; 16];
        for (s, d) in sig.iter().zip(scaled.iter_mut()) {
            *d = *s * 0.25;
        }
        assert!(wet.iter().zip(&scaled).all(|(x, y)| bits(*x) == bits(*y)));
    }

    // ----------------------------------------------------------------- dot

    #[test]
    fn dot_reference_values() {
        assert_eq!(dot(&[1.0f32, 2.0, 3.0], &[4.0, 5.0, 6.0]), 32.0);
        assert_eq!(dot(&[][..], &[][..]), 0.0);
        // Truncates to the overlapping prefix.
        assert_eq!(dot(&[1.0f32, 2.0, 3.0], &[1.0, 1.0]), 3.0);
        // One sample: a0 * b0.
        assert_eq!(dot(&[0.5f32], &[8.0]), 4.0);
    }

    #[test]
    fn dot_is_the_sequential_mul_add_fold() {
        // Pins the documented summation order bit-exactly.
        let a = pattern(256);
        let b: Vec<f32> = pattern(256).iter().map(|s| -s).collect();
        let mut acc = 0.0f32;
        for (x, y) in a.iter().zip(&b) {
            acc = x.mul_add(*y, acc);
        }
        assert_eq!(bits(dot(&a, &b)), bits(acc));
    }

    // ------------------------------------------- split-block bit-exactness

    #[test]
    fn split_block_bit_exact() {
        let a = pattern(256);
        let b: Vec<f32> = pattern(256).iter().map(|s| 1.0 - s).collect();

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        add(&a, &b, &mut full);
        add(&a[..100], &b[..100], &mut split[..100]);
        add(&a[100..], &b[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        mul(&a, &b, &mut full);
        mul(&a[..100], &b[..100], &mut split[..100]);
        mul(&a[100..], &b[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        mac(&a, &b, &mut full);
        mac(&a[..100], &b[..100], &mut split[..100]);
        mac(&a[100..], &b[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        crossfade(&a, &b, &mut full, 0.375);
        crossfade(&a[..100], &b[..100], &mut split[..100], 0.375);
        crossfade(&a[100..], &b[100..], &mut split[100..], 0.375);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        // dot is intentionally absent: a reduction's parts round
        // differently from the whole (documented in its header).
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn kernels_do_not_allocate() {
        let a = pattern(64);
        let b: Vec<f32> = pattern(64).iter().map(|s| -s).collect();
        let mut dst = pattern(64);

        assert_no_alloc::assert_no_alloc(|| {
            let mut acc = 0.0f32;
            for _ in 0..100 {
                add(&a, &b, &mut dst);
                mul(&a, &b, &mut dst);
                mac(&a, &b, &mut dst);
                crossfade(&a, &b, &mut dst, 0.5);
                acc += dot(&a, &b);
            }
            assert!(acc.is_finite());
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn edge_lengths_zero_one_and_non_power_of_two() {
        let empty: &[f32] = &[];
        let mut d0: Vec<f32> = Vec::new();
        add(empty, empty, &mut d0);
        mul(empty, empty, &mut d0);
        mac(empty, empty, &mut d0);
        crossfade(empty, empty, &mut d0, 0.5);
        assert_eq!(dot(empty, empty), 0.0);

        // Len 1.
        let one_a = [2.0f32];
        let one_b = [3.0f32];
        let mut d = [9.0f32];
        add(&one_a, &one_b, &mut d);
        assert_eq!(d, [5.0]);
        let mut d = [9.0f32];
        mul(&one_a, &one_b, &mut d);
        assert_eq!(d, [6.0]);
        let mut d = [9.0f32];
        mac(&one_a, &one_b, &mut d);
        assert_eq!(d, [15.0]);
        let mut d = [9.0f32];
        crossfade(&one_a, &one_b, &mut d, 0.5);
        assert_eq!(d, [2.5]);
        assert_eq!(dot(&one_a, &one_b), 6.0);

        // Non-power-of-two: 7.
        let seven_a = pattern(7);
        let seven_b = pattern(7);
        let mut d7 = vec![9.0f32; 7];
        add(&seven_a, &seven_b, &mut d7);
        for i in 0..7 {
            assert_eq!(d7[i], seven_a[i] + seven_b[i]);
        }
        assert!(dot(&seven_a, &seven_b).is_finite());
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn silence_in_silence_out() {
        let zero = [0.0f32; 64];
        let mut d = [0.0f32; 64];
        add(&zero, &zero, &mut d);
        assert!(d.iter().all(|s| *s == 0.0));
        let mut d = [0.0f32; 64];
        mul(&zero, &zero, &mut d);
        assert!(d.iter().all(|s| *s == 0.0));
        let mut d = [0.0f32; 64];
        crossfade(&zero, &zero, &mut d, 0.5);
        assert!(d.iter().all(|s| *s == 0.0));
        assert_eq!(dot(&zero, &zero), 0.0);
    }

    #[test]
    fn denormal_tail_stays_finite() {
        let mut tail = Vec::new();
        let mut v = 1.0f32;
        for _ in 0..160 {
            tail.push(v);
            v *= 0.5;
        }
        tail.push(f32::from_bits(0x007F_FFFF)); // largest denormal
        tail.push(f32::from_bits(0x0000_0001)); // smallest denormal
        let n = tail.len();

        let mut d = vec![0.0f32; n];
        add(&tail, &tail, &mut d);
        assert!(d.iter().all(|s| s.is_finite()));
        let mut d = vec![0.0f32; n];
        mul(&tail, &tail, &mut d);
        assert!(d.iter().all(|s| s.is_finite()));
        let mut d = vec![0.0f32; n];
        mac(&tail, &tail, &mut d);
        assert!(d.iter().all(|s| s.is_finite()));
        let mut d = vec![0.0f32; n];
        crossfade(&tail, &tail, &mut d, 0.5);
        assert!(d.iter().all(|s| s.is_finite()));
        assert!(dot(&tail, &tail).is_finite());
    }

    // ------------------------------------------------------ NaN behavior

    #[test]
    fn nan_passes_through_but_is_never_invented() {
        let nan = [f32::NAN];
        let one = [1.0f32];

        let mut d = [1.0f32];
        add(&nan, &one, &mut d);
        assert!(d[0].is_nan());
        let mut d = [1.0f32];
        mul(&nan, &one, &mut d);
        assert!(d[0].is_nan());
        let mut d = [1.0f32];
        mac(&nan, &one, &mut d);
        assert!(d[0].is_nan());
        let mut d = [1.0f32];
        crossfade(&nan, &one, &mut d, 1.0); // 0 * NaN is NaN, even at t=1
        assert!(d[0].is_nan());
        assert!(dot(&nan, &one).is_nan());

        // Finite inputs never invent NaN, even at extreme values.
        // Overflow may give ±inf (documented pass-through of finite inputs
        // to the natural float result), but never NaN.
        let big = [f32::MAX, -f32::MAX];
        let mut d = [0.0f32, 0.0];
        add(&big, &big, &mut d);
        assert!(!d.iter().any(|s| s.is_nan()));
        let mut d = [1.0f32, -1.0];
        mul(&big, &big, &mut d);
        assert!(!d.iter().any(|s| s.is_nan()));
        let mut d = [1.0f32, -1.0];
        mac(&big, &big, &mut d);
        assert!(!d.iter().any(|s| s.is_nan()));
    }
}
