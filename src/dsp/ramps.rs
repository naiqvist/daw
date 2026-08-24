//! Family 3 of the kernel roadmap: ramps & envelopes.
//!
//! [`LinearRamp`] (param glide), [`Smoother`] (one-pole exponential
//! approach), [`Fade`] (the one-shot declick primitive the transport will
//! consume for seek/loop crossfades), and [`Follower`] (peak envelope
//! detector).
//!
//! These carry state, so they are kernel structs per the contract shape:
//! `new()` (POD, no alloc), green-zone `prepare()`/setters, `reset()`, and
//! a red-zone `process()` over planar `&mut [f32]` of any length. Control
//! changes arrive through setters between blocks; kernels do not smooth
//! their own parameters (these ARE the smoothers).

/// Linear glide from the current value to a target over a fixed number of
/// samples, then hold. The parameter-ramp primitive: process() writes the
/// control signal; endpoints are exact (the final sample IS the target
/// bit-for-bit, no float-accumulation overshoot).
///
/// State: 16 bytes. Per-sample cost: 1 add + 1 branch.
/// Denormal-safe: relies on engine FTZ — a glide toward 0 passes through
/// the denormal range. Never invents NaN from finite settings.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct LinearRamp {
    value: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Default for LinearRamp {
    fn default() -> Self {
        Self::new()
    }
}

impl LinearRamp {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Green zone: jump instantly to `value` (also retargets the hold).
    pub fn set_now(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.step = 0.0;
        self.remaining = 0;
    }

    /// Green zone: glide from the current value to `target` over `samples`
    /// (0 = jump immediately).
    pub fn glide(&mut self, target: f32, samples: u32) {
        self.target = target;
        self.remaining = samples;
        self.step = if samples == 0 {
            self.value = target;
            0.0
        } else {
            (target - self.value) / samples as f32
        };
    }

    /// Zero state, keep nothing: back to construction.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn current(&self) -> f32 {
        self.value
    }

    /// True once the glide has landed (holding the target).
    pub fn done(&self) -> bool {
        self.remaining == 0
    }

    /// Red zone: write the next `out.len()` control samples.
    pub fn process(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            if self.remaining > 1 {
                self.value += self.step;
                self.remaining -= 1;
            } else if self.remaining == 1 {
                // Land exactly: the accumulated value may be a few ulp off.
                self.value = self.target;
                self.remaining = 0;
            }
            *s = self.value;
        }
    }
}

/// One-pole exponential approach to a target — the classic parameter
/// smoother: `z += coeff * (target - z)` per sample.
///
/// State: 12 bytes. Per-sample cost: 1 mul + 2 add (1 FMA form).
/// Denormal-safe: relies on engine FTZ — the difference decays through the
/// denormal range near arrival. Never invents NaN from finite settings.
/// In-place safe: n/a — output-only (fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Smoother {
    z: f32,
    target: f32,
    coeff: f32,
}

impl Default for Smoother {
    fn default() -> Self {
        Self::new()
    }
}

impl Smoother {
    pub fn new() -> Self {
        Self {
            z: 0.0,
            target: 0.0,
            coeff: 1.0,
        }
    }

    /// Green zone: reach ~63% of a step toward the target in `time_ms`.
    /// Non-finite or non-positive times mean "instant".
    pub fn prepare(&mut self, sample_rate: f32, time_ms: f32) {
        let samples = time_ms * 1e-3 * sample_rate;
        self.coeff = if samples.is_finite() && samples >= 1.0 {
            1.0 - (-1.0 / samples).exp()
        } else {
            1.0
        };
    }

    /// Green zone: new destination; the approach starts from wherever the
    /// state currently is.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Green zone: jump state and target at once (transport seeks).
    pub fn set_now(&mut self, value: f32) {
        self.z = value;
        self.target = value;
    }

    /// Zero state, keep coeff.
    pub fn reset(&mut self) {
        self.z = 0.0;
        self.target = 0.0;
    }

    pub fn current(&self) -> f32 {
        self.z
    }

    /// Red zone: write the next `out.len()` control samples.
    pub fn process(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            self.z = self.coeff.mul_add(self.target - self.z, self.z);
            *s = self.z;
        }
    }
}

/// One-shot linear fade applied in place — the declick primitive. Idle it
/// passes audio through untouched (gain 1) or holds silence (gain 0 after
/// a completed fade-out) at zero per-sample cost beyond the multiply.
///
/// State: 12 bytes. Per-sample cost: 1 mul + 1 add.
/// Denormal-safe: gain is affine in [0,1]; denormal INPUT scaled by a
/// small gain can go denormal — engine FTZ. Never invents NaN.
/// In-place safe: yes — in-place is the entire job.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Fade {
    gain: f32,
    step: f32,
    remaining: u32,
}

impl Default for Fade {
    fn default() -> Self {
        Self::new()
    }
}

impl Fade {
    /// Starts transparent (gain 1, passing audio through).
    pub fn new() -> Self {
        Self {
            gain: 1.0,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Green zone: fade from silence up to unity over `samples`. Gain
    /// snaps to 0 first: a fade-in declicks material that starts NOW.
    pub fn start_in(&mut self, samples: u32) {
        self.gain = 0.0;
        self.remaining = samples;
        self.step = if samples == 0 {
            self.gain = 1.0;
            0.0
        } else {
            1.0 / samples as f32
        };
    }

    /// Green zone: fade from the CURRENT gain down to silence over
    /// `samples` (interrupting a running fade-in mid-flight is legal and
    /// clickless: it ramps from wherever the gain is).
    pub fn start_out(&mut self, samples: u32) {
        self.remaining = samples;
        self.step = if samples == 0 {
            self.gain = 0.0;
            0.0
        } else {
            -self.gain / samples as f32
        };
    }

    /// Back to transparent.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// True while a fade is still moving.
    pub fn active(&self) -> bool {
        self.remaining > 0
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Red zone: scale `io` in place by the advancing gain.
    pub fn process(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            if self.remaining > 1 {
                self.gain += self.step;
                self.remaining -= 1;
            } else if self.remaining == 1 {
                // Land exactly on 0.0 or 1.0, no accumulation residue.
                self.gain = if self.step >= 0.0 { 1.0 } else { 0.0 };
                self.remaining = 0;
            }
            *s *= self.gain;
        }
    }
}

/// Peak envelope follower: one-pole attack on rising |x|, one-pole release
/// on falling. The detector half of a compressor/limiter and the meter's
/// ballistic.
///
/// State: 12 bytes. Per-sample cost: 1 abs + 1 cmp + 1 FMA.
/// Denormal-safe: relies on engine FTZ — the release tail decays through
/// the denormal range. NaN in = NaN out for affected samples; never
/// invents NaN from finite input.
/// In-place safe: yes — `env` may alias... it takes separate in/out slices
/// by signature (Rust forbids overlap), so: n/a.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct Follower {
    z: f32,
    attack: f32,
    release: f32,
}

impl Default for Follower {
    fn default() -> Self {
        Self::new()
    }
}

impl Follower {
    pub fn new() -> Self {
        Self {
            z: 0.0,
            attack: 1.0,
            release: 1.0,
        }
    }

    /// Green zone: ballistics in milliseconds (63% convergence times).
    /// Non-finite or sub-sample times mean "instant".
    pub fn prepare(&mut self, sample_rate: f32, attack_ms: f32, release_ms: f32) {
        let coeff = |ms: f32| {
            let samples = ms * 1e-3 * sample_rate;
            if samples.is_finite() && samples >= 1.0 {
                1.0 - (-1.0 / samples).exp()
            } else {
                1.0
            }
        };
        self.attack = coeff(attack_ms);
        self.release = coeff(release_ms);
    }

    /// Zero state, keep ballistics.
    pub fn reset(&mut self) {
        self.z = 0.0;
    }

    pub fn current(&self) -> f32 {
        self.z
    }

    /// Red zone: write the envelope of `input` into `env`, truncating to
    /// the shorter slice.
    pub fn process(&mut self, input: &[f32], env: &mut [f32]) {
        for (e, x) in env.iter_mut().zip(input.iter()) {
            let level = x.abs();
            let coeff = if level > self.z {
                self.attack
            } else {
                self.release
            };
            self.z = coeff.mul_add(level - self.z, self.z);
            *e = self.z;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    fn bits(x: f32) -> u32 {
        x.to_bits()
    }

    // -------------------------------------------------------- reference ---

    #[test]
    fn linear_ramp_endpoint_is_exact() {
        let mut r = LinearRamp::new();
        r.set_now(0.25);
        // A target with no clean binary path from 0.25 in 7 steps: the
        // accumulated value would miss by ulps; the landing snap must not.
        r.glide(0.9, 7);
        let mut out = [0.0f32; 12];
        r.process(&mut out);
        assert_eq!(bits(out[6]), bits(0.9), "sample 7 lands exactly");
        assert!(out[7..].iter().all(|s| bits(*s) == bits(0.9)), "then holds");
        assert!(r.done());
        // Monotone rise on the way.
        for w in out[..7].windows(2) {
            assert!(w[1] >= w[0]);
        }
    }

    #[test]
    fn smoother_converges_at_the_documented_rate() {
        let mut s = Smoother::new();
        s.prepare(48_000.0, 10.0); // 480 samples to ~63%
        s.set_target(1.0);
        let mut out = vec![0.0f32; 480];
        s.process(&mut out);
        let last = out[479];
        assert!(
            (last - 0.632).abs() < 0.01,
            "one time constant should reach ~63%, got {last}"
        );
    }

    #[test]
    fn fade_out_reaches_exact_zero_and_in_reaches_exact_one() {
        let mut f = Fade::new();
        f.start_out(9);
        let mut io = [1.0f32; 16];
        f.process(&mut io);
        assert_eq!(bits(io[8]), bits(0.0), "fade-out lands on exact 0");
        assert!(io[9..].iter().all(|s| *s == 0.0), "silence holds after");
        assert!(!f.active());

        f.start_in(9);
        let mut io = [1.0f32; 16];
        f.process(&mut io);
        assert_eq!(bits(io[8]), bits(1.0), "fade-in lands on exact 1");
        assert!(io[9..].iter().all(|s| *s == 1.0), "unity holds after");
        // First faded sample starts near silence (declick, not a pop).
        assert!(io[0] < 0.2);
    }

    #[test]
    fn fade_out_interrupting_fade_in_starts_from_current_gain() {
        let mut f = Fade::new();
        f.start_in(100);
        let mut io = [1.0f32; 50];
        f.process(&mut io); // gain now ~0.5
        let mid = f.gain();
        assert!((mid - 0.5).abs() < 0.02);
        f.start_out(10);
        let mut io2 = [1.0f32; 10];
        f.process(&mut io2);
        assert!(io2[0] <= mid + 1e-6, "no jump above the interrupted gain");
        assert_eq!(io2[9], 0.0);
    }

    #[test]
    fn follower_tracks_attack_and_release() {
        let mut f = Follower::new();
        f.prepare(48_000.0, 1.0, 50.0);
        // Burst then silence.
        let mut input = vec![0.8f32; 480];
        input.extend(std::iter::repeat_n(0.0f32, 4_800));
        let mut env = vec![0.0f32; input.len()];
        f.process(&input, &mut env);
        assert!(env[479] > 0.7, "attack reaches the burst level");
        assert!(env[479 + 2_400] < 0.5, "release decays after the burst");
        assert!(env[479 + 4_770] < 0.15, "and keeps decaying");
        // Envelope is nonnegative everywhere.
        assert!(env.iter().all(|e| *e >= 0.0));
    }

    // ------------------------------------------- split-block bit-exactness

    #[test]
    fn split_block_bit_exact() {
        // Same state, same input, one 256 call vs 100+156: bit-identical.
        let mut a = LinearRamp::new();
        let mut b = LinearRamp::new();
        for r in [&mut a, &mut b] {
            r.set_now(-0.3);
            r.glide(0.7, 200);
        }
        let mut full = [0.0f32; 256];
        a.process(&mut full);
        let mut split = [0.0f32; 256];
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let mut a = Smoother::new();
        let mut b = Smoother::new();
        for s in [&mut a, &mut b] {
            s.prepare(48_000.0, 5.0);
            s.set_target(1.0);
        }
        let mut full = [0.0f32; 256];
        a.process(&mut full);
        let mut split = [0.0f32; 256];
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let src: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.13).sin()).collect();
        let mut a = Fade::new();
        let mut b = Fade::new();
        a.start_out(180);
        b.start_out(180);
        let mut full: Vec<f32> = src.clone();
        a.process(&mut full);
        let mut split: Vec<f32> = src.clone();
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));

        let mut a = Follower::new();
        let mut b = Follower::new();
        for f in [&mut a, &mut b] {
            f.prepare(48_000.0, 2.0, 30.0);
        }
        let mut full = vec![0.0f32; 256];
        a.process(&src, &mut full);
        let mut split = vec![0.0f32; 256];
        b.process(&src[..100], &mut split[..100]);
        b.process(&src[100..], &mut split[100..]);
        assert!(full.iter().zip(&split).all(|(x, y)| bits(*x) == bits(*y)));
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn kernels_do_not_allocate() {
        let mut ramp = LinearRamp::new();
        ramp.glide(1.0, 1_000);
        let mut smoother = Smoother::new();
        smoother.prepare(48_000.0, 10.0);
        smoother.set_target(1.0);
        let mut fade = Fade::new();
        fade.start_out(1_000);
        let mut follower = Follower::new();
        follower.prepare(48_000.0, 1.0, 50.0);

        let src = vec![0.5f32; 64];
        let mut buf = vec![0.0f32; 64];
        let mut env = vec![0.0f32; 64];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                ramp.process(&mut buf);
                smoother.process(&mut buf);
                fade.process(&mut buf);
                follower.process(&src, &mut env);
            }
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn edge_lengths_zero_one_and_non_power_of_two() {
        let mut ramp = LinearRamp::new();
        ramp.glide(1.0, 5);
        let mut empty: [f32; 0] = [];
        ramp.process(&mut empty); // len 0: no-op, no panic, no advance
        assert_eq!(ramp.current(), 0.0);
        let mut one = [0.0f32; 1];
        ramp.process(&mut one); // len 1 advances one step
        assert!(one[0] > 0.0);
        let mut seven = [0.0f32; 7];
        ramp.process(&mut seven); // non-power-of-two, crosses the landing
        assert_eq!(seven[6], 1.0);

        let mut fade = Fade::new();
        fade.start_out(0); // zero-length fade = instant
        let mut io = [1.0f32; 3];
        fade.process(&mut io);
        assert_eq!(io, [0.0; 3]);

        let mut fol = Follower::new();
        fol.prepare(48_000.0, 1.0, 1.0);
        fol.process(&[], &mut []);
        let mut e = [0.0f32; 1];
        fol.process(&[0.5], &mut e);
        assert!(e[0] > 0.0);
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn silence_and_denormal_behavior() {
        // Silence in, silence out (transparent fade).
        let mut fade = Fade::new();
        let mut io = [0.0f32; 64];
        fade.process(&mut io);
        assert!(io.iter().all(|s| *s == 0.0));

        // Follower on silence stays at exact zero.
        let mut fol = Follower::new();
        fol.prepare(48_000.0, 1.0, 50.0);
        let mut env = [1.0f32; 64];
        fol.process(&[0.0; 64], &mut env);
        assert!(env.iter().all(|e| *e == 0.0));

        // Decaying and denormal inputs stay finite through every kernel.
        let mut tail = Vec::new();
        let mut v = 1.0f32;
        for _ in 0..160 {
            tail.push(v);
            v *= 0.5;
        }
        tail.push(f32::from_bits(0x0000_0001)); // smallest denormal
        let n = tail.len();

        let mut fade = Fade::new();
        fade.start_out(n as u32);
        let mut io = tail.clone();
        fade.process(&mut io);
        assert!(io.iter().all(|s| s.is_finite()));

        let mut fol = Follower::new();
        fol.prepare(48_000.0, 0.5, 5.0);
        let mut env = vec![0.0f32; n];
        fol.process(&tail, &mut env);
        assert!(env.iter().all(|e| e.is_finite()));

        // Smoother gliding to 0 from 1: decays through denormals, finite,
        // and never goes negative.
        let mut s = Smoother::new();
        s.prepare(48_000.0, 0.1);
        s.set_now(1.0);
        s.set_target(0.0);
        let mut out = vec![9.0f32; 4_096];
        s.process(&mut out);
        assert!(out.iter().all(|x| x.is_finite() && *x >= 0.0));
    }
}
