//! Family 1 of the kernel roadmap: memory & move.
//!
//! `copy`, `clear`, `interleave`/`deinterleave`, format conversion
//! (`i16`/`i24`/`f32`), and gain-and-accumulate — the pure data-movement
//! primitives everything else is built from.
//!
//! These are stateless, so they are free functions rather than kernel
//! structs: there is no state to `prepare()` or `reset()`, and the kernel
//! shape exists for kernels that carry state. Every other contract
//! guarantee applies unchanged — doc headers, no allocation, no panics,
//! and the five-test battery at the bottom of this file.
//!
//! Conventions for everything in this module:
//! - Lengths truncate to the shortest participating slice (zip semantics):
//!   a size mismatch is never a panic.
//! - No indexing that can be out of bounds, no unwrap/expect/assert.
//! - `i24` is carried sign-extended in an `i32` (the in-memory convention;
//!   3-byte file packing is a codec concern handled elsewhere).
//! - Sample formats are full-range: `i16::MIN` and `i24::MIN` mean `-1.0`.

/// Copy samples from `src` into `dst`, truncating to the shorter length.
///
/// State: none (stateless). Per-sample cost: 1 copy, 0 arithmetic.
/// Denormal-safe: yes — no arithmetic; denormal bits pass through verbatim.
/// In-place safe: n/a — `src` and `dst` are distinct by signature (Rust
/// aliasing rules). This is memcpy, not memmove.
/// Latency: 0 samples.
#[inline]
pub fn copy(src: &[f32], dst: &mut [f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = *s;
    }
}

/// Zero every sample in `buf`.
///
/// State: none (stateless). Per-sample cost: 1 store, 0 arithmetic.
/// Denormal-safe: yes — no arithmetic; output is exactly `0.0`.
/// In-place safe: yes — in-place is the entire job.
/// Latency: 0 samples.
#[inline]
pub fn clear(buf: &mut [f32]) {
    buf.fill(0.0);
}

/// Interleave two planar channels into one interleaved buffer:
/// `out = [l0, r0, l1, r1, ...]`.
///
/// Frames processed = `min(left.len(), right.len(), interleaved.len() / 2)`;
/// a trailing odd output sample or a size mismatch is never a panic. The
/// dangling odd sample keeps its previous contents — it is not zeroed — so
/// callers must size interleaved buffers even (or clear them) or the last
/// sample emitted is stale memory, not signal.
///
/// State: none (stateless). Per-sample cost: 1 copy per input sample
/// (2 per output frame), 0 arithmetic.
/// Denormal-safe: yes — no arithmetic.
/// In-place safe: no — `interleaved` must be a distinct buffer; the forward
/// write pattern would clobber planar samples that have not been read yet.
/// Latency: 0 samples.
#[inline]
pub fn interleave(left: &[f32], right: &[f32], interleaved: &mut [f32]) {
    for ([l_out, r_out], (l, r)) in interleaved
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(left.iter().zip(right.iter()))
    {
        *l_out = *l;
        *r_out = *r;
    }
}

/// Deinterleave `[l0, r0, l1, r1, ...]` into two planar channels.
///
/// Frames processed = `min(interleaved.len() / 2, left.len(), right.len())`;
/// a trailing odd sample or a size mismatch is never a panic.
///
/// State: none (stateless). Per-sample cost: 1 copy per output sample,
/// 0 arithmetic.
/// Denormal-safe: yes — no arithmetic.
/// In-place safe: no — `left`/`right` must be distinct buffers. An in-place
/// planarization does exist (right channel backward, then left forward) but
/// is deliberately not provided until a caller needs it.
/// Latency: 0 samples.
#[inline]
pub fn deinterleave(interleaved: &[f32], left: &mut [f32], right: &mut [f32]) {
    for ([l_in, r_in], (l, r)) in interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .zip(left.iter_mut().zip(right.iter_mut()))
    {
        *l = *l_in;
        *r = *r_in;
    }
}

/// Convert `f32` samples (nominal `-1.0..=1.0`) to signed 16-bit PCM,
/// rounding half away from zero and clamping to full scale.
///
/// `NaN` converts to 0 (float-to-int casts saturate); ±inf clamps to full
/// scale. Never panics, never invents NaN.
///
/// State: none (stateless). Per-sample cost: 1 mul, 1 round, 1 clamp
/// (2 min/max), 1 cast.
/// Denormal-safe: yes — scales up, so denormal inputs round to a small
/// integer (usually 0) and there is no float output to denormalize.
/// In-place safe: n/a — source and destination have different types.
/// Latency: 0 samples.
#[inline]
pub fn to_i16(src: &[f32], dst: &mut [i16]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = (s * 32_768.0).clamp(-32_768.0, 32_767.0).round() as i16;
    }
}

/// Convert signed 16-bit PCM to `f32` (nominal `-1.0..=1.0`).
///
/// State: none (stateless). Per-sample cost: 1 cast, 1 mul.
/// Denormal-safe: yes — the smallest nonzero output is `1/32768` ≈ 3e-5,
/// far above the denormal range; denormals are never created.
/// In-place safe: n/a — source and destination have different types.
/// Latency: 0 samples.
#[inline]
pub fn from_i16(src: &[i16], dst: &mut [f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = f32::from(*s) * (1.0 / 32_768.0);
    }
}

/// Convert `f32` samples (nominal `-1.0..=1.0`) to signed 24-bit PCM carried
/// sign-extended in an `i32` (the in-memory convention; 3-byte file packing
/// is a codec concern, not a kernel concern).
///
/// `NaN` converts to 0; ±inf clamps to full scale. Positive full scale is
/// `8_388_607` (`0x7FFFFF`), never `8_388_608` — in 24-bit two's complement
/// that bit pattern is `i24::MIN` (-8_388_608), so emitting it would decode
/// as full-scale negative and flip the polarity of the loudest possible
/// sample.
///
/// State: none (stateless). Per-sample cost: 1 mul, 1 round, 1 clamp
/// (2 min/max), 1 cast.
/// Denormal-safe: yes — scales up; see [`to_i16`].
/// In-place safe: n/a — source and destination have different types.
/// Latency: 0 samples.
#[inline]
pub fn to_i24(src: &[f32], dst: &mut [i32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = (s * 8_388_608.0).clamp(-8_388_608.0, 8_388_607.0).round() as i32;
    }
}

/// Convert signed 24-bit PCM (carried in an `i32`) to `f32`.
///
/// Garbage above bit 23 is ignored: the low 24 bits are sign-extended
/// first, so callers may pass raw `i32` loads without masking.
///
/// State: none (stateless). Per-sample cost: 1 shift, 1 cast, 1 mul.
/// Denormal-safe: yes — the smallest nonzero output is `1/8388608` ≈ 1.2e-7,
/// far above the denormal range; denormals are never created.
/// In-place safe: n/a — source and destination have different types.
/// Latency: 0 samples.
#[inline]
pub fn from_i24(src: &[i32], dst: &mut [f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        // Sign-extend from bit 23. Wrapping shift: garbage high bits must
        // not be able to trip a debug overflow check (a panic).
        let sign_extended = (s.wrapping_shl(8)) >> 8;
        *d = sign_extended as f32 * (1.0 / 8_388_608.0);
    }
}

/// Gain-and-accumulate: `dst[i] += src[i] * gain`. The mixing primitive —
/// voices into a bus, wet into dry.
///
/// `gain == 0.0` is not a guaranteed no-op: `0 * NaN` is NaN. NaN in `src`
/// passes through to `dst`; this kernel never invents NaN from finite
/// inputs.
///
/// State: none (stateless). Per-sample cost: 1 FMA (`f32::mul_add`,
/// single rounding, pinned — same convention as the `arith` family, so
/// mixing through `gain_add` and through `mac` round identically).
/// Denormal-safe: relies on engine FTZ — a denormal `gain` with small
/// samples can produce denormal products, which FTZ flushes to 0; without
/// FTZ they remain correct but slow.
/// In-place safe: n/a — `src` and `dst` are distinct by signature.
/// Latency: 0 samples.
#[inline]
pub fn gain_add(src: &[f32], dst: &mut [f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = s.mul_add(gain, *d);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    /// Deterministic pattern for the split-block and no-alloc batteries.
    /// Not random: reference tests want bit-exact reasoning.
    fn pattern(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.125) % 2.0 - 1.0).collect()
    }

    // ---------------------------------------------------------------- copy

    #[test]
    fn copy_copies_and_truncates() {
        let src = [1.0f32, -2.5, 3.25];
        let mut dst = [9.0f32; 5];
        copy(&src, &mut dst);
        assert_eq!(dst, [1.0, -2.5, 3.25, 9.0, 9.0]);

        let mut short = [7.0f32; 2];
        copy(&src, &mut short);
        assert_eq!(short, [1.0, -2.5]);
    }

    // --------------------------------------------------------------- clear

    #[test]
    fn clear_zeroes_everything() {
        let mut buf = [1.0f32, -0.5, 0.25];
        clear(&mut buf);
        assert_eq!(buf, [0.0; 3]);
    }

    // ---------------------------------------------------------- interleave

    #[test]
    fn interleave_reference_layout() {
        let left = [0.0f32, 2.0, 4.0];
        let right = [1.0f32, 3.0, 5.0];
        let mut out = [9.0f32; 6];
        interleave(&left, &right, &mut out);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn interleave_truncates_without_panic() {
        let left = [0.0f32, 2.0, 4.0, 6.0];
        let right = [1.0f32, 3.0];
        let mut out = [9.0f32; 8];
        interleave(&left, &right, &mut out);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 9.0, 9.0, 9.0, 9.0]);

        // Odd output length: the dangling tail is left untouched.
        let mut short = [9.0f32; 5];
        interleave(&left, &right, &mut short);
        assert_eq!(short, [0.0, 1.0, 2.0, 3.0, 9.0]);
    }

    // -------------------------------------------------------- deinterleave

    #[test]
    fn deinterleave_reference_layout() {
        let inter = [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let mut left = [9.0f32; 4];
        let mut right = [9.0f32; 4];
        deinterleave(&inter, &mut left, &mut right);
        assert_eq!(left, [0.0, 2.0, 4.0, 6.0]);
        assert_eq!(right, [1.0, 3.0, 5.0, 7.0]);
    }

    #[test]
    fn deinterleave_roundtrips_with_interleave() {
        let left = pattern(37);
        let right: Vec<f32> = pattern(37).iter().map(|s| -s).collect();
        let mut inter = vec![9.0f32; 74];
        interleave(&left, &right, &mut inter);
        let mut l2 = vec![9.0f32; 37];
        let mut r2 = vec![9.0f32; 37];
        deinterleave(&inter, &mut l2, &mut r2);
        assert!(l2.iter().zip(&left).all(|(a, b)| bits(*a) == bits(*b)));
        assert!(r2.iter().zip(&right).all(|(a, b)| bits(*a) == bits(*b)));
    }

    #[test]
    fn deinterleave_truncates_odd_tail() {
        // Odd length: the last sample is dropped, never a panic.
        let inter = [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut left = [9.0f32; 4];
        let mut right = [9.0f32; 4];
        deinterleave(&inter, &mut left, &mut right);
        assert_eq!(left, [0.0, 2.0, 4.0, 9.0]);
        assert_eq!(right, [1.0, 3.0, 5.0, 9.0]);

        // Destinations shorter than the input: truncation, not panic.
        let mut l = [9.0f32; 2];
        let mut r = [9.0f32; 2];
        deinterleave(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], &mut l, &mut r);
        assert_eq!(l, [0.0, 2.0]);
        assert_eq!(r, [1.0, 3.0]);
    }

    // ------------------------------------------------------- format i16/f32

    #[test]
    fn to_i16_reference_values() {
        let src = [
            0.0f32,
            1.0,
            -1.0,
            0.5,
            -0.5,
            1.0 / 65_536.0,      // half an LSB up: rounds away from zero
            -1.0 / 65_536.0,     // half an LSB down: rounds away from zero
            32_767.5 / 32_768.0, // rounds past max, clamps to full scale
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut dst = [0i16; 11];
        to_i16(&src, &mut dst);
        assert_eq!(
            dst,
            [
                0, 32_767, -32_768, 16_384, -16_384, 1, -1, 32_767, 0, 32_767, -32_768
            ]
        );
    }

    #[test]
    fn from_i16_reference_values() {
        let src = [0i16, 32_767, -32_768, 16_384, -16_384, 1, -1];
        let mut dst = [0.0f32; 7];
        from_i16(&src, &mut dst);
        assert_eq!(dst[0], 0.0);
        assert_eq!(dst[1], 32_767.0 / 32_768.0);
        assert_eq!(dst[2], -1.0); // i16::MIN maps to exactly -1.0
        assert_eq!(dst[3], 0.5);
        assert_eq!(dst[4], -0.5);
        assert_eq!(dst[5], 1.0 / 32_768.0);
        assert_eq!(dst[6], -1.0 / 32_768.0);
    }

    // ------------------------------------------------------- format i24/f32

    #[test]
    fn to_i24_reference_values() {
        let src = [
            0.0f32,
            1.0,
            -1.0,
            0.5,
            -0.5,
            8_388_607.5 / 8_388_608.0, // rounds past max, clamps
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut dst = [0i32; 9];
        to_i24(&src, &mut dst);
        assert_eq!(
            dst,
            [
                0, 8_388_607, -8_388_608, 4_194_304, -4_194_304, 8_388_607, 0, 8_388_607,
                -8_388_608
            ]
        );
        // The 0x800000 bit pattern is i24::MIN (full-scale negative), not a
        // valid positive encoding: no input may ever produce it as output.
        assert!(dst.iter().all(|&v| v != 8_388_608));
    }

    #[test]
    fn from_i24_reference_values() {
        let src = [0i32, 8_388_607, -8_388_608, 4_194_304, -4_194_304, 1, -1];
        let mut dst = [0.0f32; 7];
        from_i24(&src, &mut dst);
        assert_eq!(dst[0], 0.0);
        assert_eq!(dst[1], 8_388_607.0 / 8_388_608.0);
        assert_eq!(dst[2], -1.0); // i24::MIN maps to exactly -1.0
        assert_eq!(dst[3], 0.5);
        assert_eq!(dst[4], -0.5);
        assert_eq!(dst[5], 1.0 / 8_388_608.0);
        assert_eq!(dst[6], -1.0 / 8_388_608.0);
    }

    #[test]
    fn from_i24_ignores_garbage_high_bits() {
        // Raw i32 loads from disk can carry junk above bit 23:
        // 0x7FFF_FFFF and 0x00FF_FFFF both sign-extend to -1;
        // i32::MIN sign-extends to 0; 0x0000_8000 is a clean 32768.
        let src = [i32::MAX, i32::MIN, 0x00FF_FFFF, 0x0000_8000];
        let mut dst = [0.0f32; 4];
        from_i24(&src, &mut dst);
        assert_eq!(dst[0], -1.0 / 8_388_608.0);
        assert_eq!(dst[1], 0.0);
        assert_eq!(dst[2], -1.0 / 8_388_608.0);
        assert_eq!(dst[3], 0.003_906_25);
    }

    // ------------------------------------------------------------- gain_add

    #[test]
    fn gain_add_reference_values() {
        let src = [1.0f32, 2.0, 3.0];
        let mut dst = [10.0f32, 20.0, 30.0];
        gain_add(&src, &mut dst, 0.25);
        assert_eq!(dst, [10.25, 20.5, 30.75]);

        // Zero gain leaves the destination untouched.
        let mut d2 = [10.0f32, 20.0];
        gain_add(&src, &mut d2, 0.0);
        assert_eq!(d2, [10.0, 20.0]);

        // Destination shorter than source: truncation, not panic.
        let mut d3 = [10.0f32; 3];
        gain_add(&src, &mut d3, 1.0);
        assert_eq!(d3, [11.0, 12.0, 13.0]);

        // Accumulates: two passes at 0.25 == one pass at 0.5.
        let mut d4 = [10.0f32, 20.0];
        gain_add(&src, &mut d4, 0.25);
        gain_add(&src, &mut d4, 0.25);
        let mut d5 = [10.0f32, 20.0];
        gain_add(&src, &mut d5, 0.5);
        assert_eq!(d4, d5);
    }

    // ------------------------------------------- split-block bit-exactness

    #[test]
    fn split_block_bit_exact() {
        let src = pattern(256);

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        copy(&src, &mut full);
        copy(&src[..100], &mut split[..100]);
        copy(&src[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let mut full = vec![7.0f32; 256];
        let mut split = vec![7.0f32; 256];
        gain_add(&src, &mut full, 0.75);
        gain_add(&src[..100], &mut split[..100], 0.75);
        gain_add(&src[100..], &mut split[100..], 0.75);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let mut full = vec![0i16; 256];
        let mut split = vec![0i16; 256];
        to_i16(&src, &mut full);
        to_i16(&src[..100], &mut split[..100]);
        to_i16(&src[100..], &mut split[100..]);
        assert_eq!(full, split);

        // from_i16 split-block: convert a full block of i16 once and in two
        // pieces; outputs must be bit-identical.
        let mut ints = vec![0i16; 256];
        to_i16(&src, &mut ints);
        let mut full = vec![0.0f32; 256];
        let mut split = vec![0.0f32; 256];
        from_i16(&ints, &mut full);
        from_i16(&ints[..100], &mut split[..100]);
        from_i16(&ints[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let mut full = vec![0i32; 256];
        let mut split = vec![0i32; 256];
        to_i24(&src, &mut full);
        to_i24(&src[..100], &mut split[..100]);
        to_i24(&src[100..], &mut split[100..]);
        assert_eq!(full, split);

        // from_i24 split-block: convert a full block of i24 once and in two
        // pieces; outputs must be bit-identical.
        let mut ints = vec![0i32; 256];
        to_i24(&src, &mut ints);
        let mut full = vec![0.0f32; 256];
        let mut split = vec![0.0f32; 256];
        from_i24(&ints, &mut full);
        from_i24(&ints[..100], &mut split[..100]);
        from_i24(&ints[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        let left = pattern(256);
        let right: Vec<f32> = pattern(256).iter().map(|s| -s).collect();
        let mut full = vec![7.0f32; 512];
        let mut split = vec![7.0f32; 512];
        interleave(&left, &right, &mut full);
        interleave(&left[..100], &right[..100], &mut split[..200]);
        interleave(&left[100..], &right[100..], &mut split[200..]);
        assert!(full.iter().zip(&split).all(|(a, b)| bits(*a) == bits(*b)));

        // deinterleave split-block: planarize the full interleaved block once
        // and in two frame-aligned pieces (frame 100 = sample 200); both
        // channels must be bit-identical at every position.
        let mut l_full = vec![7.0f32; 256];
        let mut r_full = vec![7.0f32; 256];
        deinterleave(&full, &mut l_full, &mut r_full);

        let mut l_split = vec![7.0f32; 256];
        let mut r_split = vec![7.0f32; 256];
        deinterleave(&full[..200], &mut l_split[..100], &mut r_split[..100]);
        deinterleave(&full[200..], &mut l_split[100..], &mut r_split[100..]);
        assert!(
            l_full
                .iter()
                .zip(&l_split)
                .all(|(a, b)| bits(*a) == bits(*b))
        );
        assert!(
            r_full
                .iter()
                .zip(&r_split)
                .all(|(a, b)| bits(*a) == bits(*b))
        );
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn kernels_do_not_allocate() {
        let src = pattern(64);
        let mut dst = pattern(64);
        let mut buf = pattern(64);
        let mut il = vec![0.0f32; 64];
        let mut i16s = vec![0i16; 64];
        let mut i24s = vec![0i32; 64];

        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                copy(&src, &mut dst);
                clear(&mut buf);
                interleave(&src, &dst, &mut il);
                deinterleave(&il, &mut buf, &mut dst);
                to_i16(&src, &mut i16s);
                from_i16(&i16s, &mut buf);
                to_i24(&src, &mut i24s);
                from_i24(&i24s, &mut dst);
                gain_add(&src, &mut buf, 0.125);
            }
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn edge_lengths_zero_one_and_non_power_of_two() {
        // Len 0: every kernel is a no-op, never a panic.
        let empty: &[f32] = &[];
        let empty_i16: &[i16] = &[];
        let empty_i32: &[i32] = &[];
        let mut d0: Vec<f32> = Vec::new();
        let mut d1: Vec<f32> = Vec::new();
        let mut i0: Vec<i16> = Vec::new();
        let mut i1: Vec<i32> = Vec::new();
        copy(empty, &mut d0);
        clear(&mut d0);
        interleave(empty, empty, &mut d0);
        deinterleave(empty, &mut d0, &mut d1);
        to_i16(empty, &mut i0);
        from_i16(empty_i16, &mut d0);
        to_i24(empty, &mut i1);
        from_i24(empty_i32, &mut d0);
        gain_add(empty, &mut d0, 1.0);

        // Len 1.
        let one = [0.25f32];
        let mut d = [9.0f32; 1];
        copy(&one, &mut d);
        assert_eq!(d, [0.25]);
        let mut d = [9.0f32; 1];
        gain_add(&one, &mut d, 1.0);
        assert_eq!(d, [9.25]);
        let mut i = [0i16; 1];
        to_i16(&one, &mut i);
        assert_eq!(i, [8192]);
        let mut f = [0.0f32; 1];
        from_i16(&[16_384], &mut f);
        assert_eq!(f, [0.5]);
        let mut i = [0i32; 1];
        to_i24(&one, &mut i);
        assert_eq!(i, [2_097_152]);
        let mut f = [0.0f32; 1];
        from_i24(&[4_194_304], &mut f);
        assert_eq!(f, [0.5]);

        // Non-power-of-two: 7.
        let seven = pattern(7);
        let mut d7 = vec![9.0f32; 7];
        copy(&seven, &mut d7);
        assert_eq!(d7[..], seven[..]);
        clear(&mut d7);
        assert!(d7.iter().all(|s| *s == 0.0));
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn silence_in_silence_out() {
        let zero = [0.0f32; 64];

        let mut d = [0.0f32; 64];
        copy(&zero, &mut d);
        assert!(d.iter().all(|s| *s == 0.0));

        let mut il = [0.0f32; 128];
        interleave(&zero, &zero, &mut il);
        assert!(il.iter().all(|s| *s == 0.0));

        let mut i16s = [0i16; 64];
        to_i16(&zero, &mut i16s);
        assert!(i16s.iter().all(|s| *s == 0));

        let mut d3 = [0.0f32; 64];
        from_i16(&i16s, &mut d3);
        assert!(d3.iter().all(|s| *s == 0.0));

        let mut i24s = [0i32; 64];
        to_i24(&zero, &mut i24s);
        assert!(i24s.iter().all(|s| *s == 0));

        let mut d4 = [0.0f32; 64];
        from_i24(&i24s, &mut d4);
        assert!(d4.iter().all(|s| *s == 0.0));

        let mut d2 = [0.0f32; 64];
        gain_add(&zero, &mut d2, 1_000.0);
        assert!(d2.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn denormal_tail_stays_finite() {
        // Decaying tail down through the denormal range, plus pure denormals.
        let mut tail = Vec::new();
        let mut v = 1.0f32;
        for _ in 0..160 {
            tail.push(v);
            v *= 0.5;
        }
        tail.push(f32::from_bits(0x007F_FFFF)); // largest denormal
        tail.push(f32::from_bits(0x0000_0001)); // smallest denormal

        let mut d = vec![0.0f32; tail.len()];
        gain_add(&tail, &mut d, 1.0);
        assert!(d.iter().all(|s| s.is_finite()));

        let mut d2 = vec![0.0f32; tail.len()];
        gain_add(&tail, &mut d2, 1e-30);
        assert!(d2.iter().all(|s| s.is_finite()));

        // Denormal inputs scale up to a round 0 in integer formats.
        let mut i16s = vec![0i16; tail.len()];
        to_i16(&tail, &mut i16s);
        assert_eq!(i16s[i16s.len() - 1], 0);
        assert_eq!(i16s[i16s.len() - 2], 0);

        let mut i24s = vec![0i32; tail.len()];
        to_i24(&tail, &mut i24s);
        assert_eq!(i24s[i24s.len() - 1], 0);
        assert_eq!(i24s[i24s.len() - 2], 0);
    }

    // ------------------------------------------------------ NaN behavior

    #[test]
    fn nan_passes_through_but_is_never_invented() {
        let nan = [f32::NAN];

        // Moves pass NaN through (documented contract behavior).
        let mut d = [1.0f32];
        copy(&nan, &mut d);
        assert!(d[0].is_nan());

        let mut d2 = [1.0f32];
        gain_add(&nan, &mut d2, 0.0); // 0 * NaN is NaN: still passes through
        assert!(d2[0].is_nan());

        // Conversions map NaN to 0 via the saturating cast.
        let mut i16s = [7i16];
        to_i16(&nan, &mut i16s);
        assert_eq!(i16s[0], 0);
        let mut i24s = [7i32];
        to_i24(&nan, &mut i24s);
        assert_eq!(i24s[0], 0);

        // Finite inputs never invent NaN, even at extreme gains.
        let fin = [1.0f32, -1.0, 0.5];
        let mut d3 = [0.0f32; 3];
        gain_add(&fin, &mut d3, f32::MAX);
        assert!(d3.iter().all(|s| s.is_finite()));
    }
}
