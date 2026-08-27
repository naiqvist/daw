//! Family 7 of the kernel roadmap, continued: noise.
//!
//! [`WhiteNoise`] (uniform, seedable, deterministic) and [`PinkNoise`]
//! (−3 dB/octave — equal energy per octave, the noise that sounds flat).
//!
//! # Determinism is the design
//!
//! Audio noise does not want to be random, it wants to be UNCORRELATED
//! and REPRODUCIBLE. The generator is SplitMix64 — pure integer
//! arithmetic, no dependencies, passes the statistical batteries, and a
//! given seed always yields the same stream bit for bit. That is what
//! makes a bounce byte-identical across runs (the engine's flagship
//! guarantee), makes split-block equivalence testable, and lets two
//! voices decorrelate by seed instead of by luck.
//!
//! # Pink
//!
//! White filtered through the Kellett pole bank — seven one-poles whose
//! staggered corners approximate the −3 dB/octave slope. An exact 1/f
//! filter does not exist in finite IIR; Kellett's approximation is good
//! to a fraction of a dB across the musical band, and the test below
//! MEASURES the slope through our own [`Svf`] constant-Q filter bank
//! rather than restating the coefficients' promise.
//!
//! [`Svf`]: crate::dsp::filters::Svf

use crate::dsp::{LANES, LaneFrame};

/// SplitMix64's additive constant (the golden-ratio gamma).
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// One SplitMix64 output for a counter value.
#[inline(always)]
fn mix(mut z: u64) -> u64 {
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Uniform white noise in `[-1, 1)`.
///
/// State: 16 bytes. Per-sample cost: integer only — one add, two
/// multiplies, five shift/xors — plus one int→float convert.
/// Denormal-safe: outputs are uniform, never in the denormal range.
/// In-place safe: n/a — output-only (a generator fills the block).
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct WhiteNoise {
    state: u64,
    seed: u64,
}

impl Default for WhiteNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self { state: 1, seed: 1 }
    }

    /// Green zone: pick the stream. Two generators with different seeds
    /// are uncorrelated; the same seed is the same stream, always.
    pub fn seed(&mut self, seed: u64) {
        self.seed = seed;
        self.state = seed;
    }

    /// Green zone: return to the start of the stream.
    pub fn reset(&mut self) {
        self.state = self.seed;
    }

    /// One sample, `[-1, 1)`.
    #[inline(always)]
    fn next(&mut self) -> f32 {
        self.state = self.state.wrapping_add(GAMMA);
        let z = mix(self.state);
        // Top 24 bits → mantissa-exact float in [0, 2), recentred.
        ((z >> 40) as f32) * (2.0 / 16_777_216.0) - 1.0
    }

    /// Red zone: fill `out`, any length.
    pub fn process(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            *s = self.next();
        }
    }
}

/// Pink noise: −3 dB/octave, equal energy per octave.
///
/// White through the Kellett seven-pole approximation, scaled toward unit
/// practical peak. The peak is PRACTICAL, not guaranteed — filtered noise
/// has no hard bound — and the long-run tests pin what it actually does:
/// peaks near ±0.9 over a million samples, RMS near 0.14.
///
/// The pole corners are tuned for rates near 44.1/48 kHz; the slope test
/// below holds at 48 k across 125 Hz–8 kHz. At very different rates the
/// extremes of the band drift — measure before trusting it at 192 k.
///
/// State: 44 bytes. Per-sample cost: the white generator + 7 mul + 13 add.
/// Denormal-safe: the pole states are continuously re-excited by white
/// noise and never decay toward zero while running; FTZ covers reset
/// tails.
/// In-place safe: n/a — output-only.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct PinkNoise {
    white: WhiteNoise,
    b: [f32; 7],
}

/// Output scale, MEASURED into place: at 0.125 a million-sample run
/// peaked at 1.007 and failed its own bounds test; at 0.11 the same run
/// peaks near 0.89. Filtered noise has no derivable hard bound, so the
/// number comes from the test, and the test keeps it honest.
const PINK_SCALE: f32 = 0.11;

impl Default for PinkNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl PinkNoise {
    pub fn new() -> Self {
        Self {
            white: WhiteNoise::new(),
            b: [0.0; 7],
        }
    }

    /// Green zone: pick the stream (seeds the white source).
    pub fn seed(&mut self, seed: u64) {
        self.white.seed(seed);
        self.b = [0.0; 7];
    }

    /// Green zone: return to the stream start with settled filters.
    pub fn reset(&mut self) {
        self.white.reset();
        self.b = [0.0; 7];
    }

    /// Red zone: fill `out`, any length.
    // Kellett's published constants, verbatim — one has a digit past f32
    // precision, kept so the source is greppable against the reference.
    #[allow(clippy::excessive_precision)]
    pub fn process(&mut self, out: &mut [f32]) {
        let [mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6] = self.b;
        for s in out.iter_mut() {
            let w = self.white.next();
            b0 = 0.99886 * b0 + w * 0.0555179;
            b1 = 0.99332 * b1 + w * 0.0750759;
            b2 = 0.96900 * b2 + w * 0.1538520;
            b3 = 0.86650 * b3 + w * 0.3104856;
            b4 = 0.55000 * b4 + w * 0.5329522;
            b5 = -0.7616 * b5 - w * 0.0168980;
            let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362;
            b6 = w * 0.115926;
            *s = pink * PINK_SCALE;
        }
        self.b = [b0, b1, b2, b3, b4, b5, b6];
    }
}

// ------------------------------------------------------------- lanes ---

/// Decorrelate a base seed into one seed per lane.
///
/// Run through the mixer with the lane folded in, rather than `seed + i`:
/// SplitMix64 states one apart produce streams that are independent by the
/// batteries but arrive from adjacent counter values, and the point here is
/// that eight unison voices sound like eight sources. Mixing first means
/// lane 1 starts somewhere unrelated to lane 0, not one step along.
#[inline]
fn lane_seed(seed: u64, lane: usize) -> u64 {
    mix(seed ^ (lane as u64).wrapping_add(1).wrapping_mul(GAMMA))
}

/// [`WhiteNoise`] for a whole voice group: [`LANES`] independent streams,
/// lane-major.
///
/// Independence is the WHOLE POINT and it is structural, not the caller's
/// job: lanes sharing one generator would make a unison stack collapse
/// into one loud correlated noise, which is the audible bug. [`Self::seed`]
/// derives a decorrelated seed per lane, so there is no way to seed this
/// wrongly.
///
/// State: 128 bytes ([`LANES`] × 2 × u64). Per-sample-per-lane cost: the
/// scalar generator's, unchanged.
/// Denormal-safe: outputs are uniform, never in the denormal range.
/// In-place safe: n/a — output-only.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct LaneWhiteNoise {
    state: [u64; LANES],
    seed: [u64; LANES],
}

impl Default for LaneWhiteNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneWhiteNoise {
    pub fn new() -> Self {
        let mut g = Self {
            state: [0; LANES],
            seed: [0; LANES],
        };
        g.seed(1);
        g
    }

    /// Green zone: pick the group's streams. One number in, [`LANES`]
    /// uncorrelated streams out.
    pub fn seed(&mut self, seed: u64) {
        for (lane, (st, sd)) in self.state.iter_mut().zip(self.seed.iter_mut()).enumerate() {
            *sd = lane_seed(seed, lane);
            *st = *sd;
        }
    }

    /// Green zone: return every lane to the start of its stream.
    pub fn reset(&mut self) {
        self.state = self.seed;
    }

    /// Green zone: return ONE lane to the start of its stream — what
    /// voice stealing wants, without disturbing the voices still sounding.
    pub fn reset_lane(&mut self, lane: usize) {
        if let (Some(st), Some(sd)) = (self.state.get_mut(lane), self.seed.get(lane)) {
            *st = *sd;
        }
    }

    /// Red zone: fill `out`, any length.
    pub fn process(&mut self, out: &mut [LaneFrame]) {
        for frame in out.iter_mut() {
            for (s, st) in frame.iter_mut().zip(self.state.iter_mut()) {
                *st = st.wrapping_add(GAMMA);
                let z = mix(*st);
                *s = ((z >> 40) as f32) * (2.0 / 16_777_216.0) - 1.0;
            }
        }
    }
}

/// [`PinkNoise`] for a whole voice group.
///
/// The pole bank is stored POLE-MAJOR — `[[f32; LANES]; 7]`, seven banks
/// of [`LANES`] — so one pole's update across every lane is a single
/// register-wide multiply-add. Stored the other way round it would be
/// seven strided reads per lane and vectorize nothing, which is the
/// layout mistake this whole family exists to avoid.
///
/// State: the white generator's + 224 bytes.
/// Per-sample-per-lane cost: the scalar pink's, unchanged.
/// Denormal-safe: the poles are continuously re-excited and never decay
/// toward zero while running; FTZ covers reset tails.
/// In-place safe: n/a — output-only.
/// Latency: 0 samples.
#[derive(Debug, Clone, Copy)]
pub struct LanePinkNoise {
    white: LaneWhiteNoise,
    b: [[f32; LANES]; 7],
}

impl Default for LanePinkNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl LanePinkNoise {
    pub fn new() -> Self {
        Self {
            white: LaneWhiteNoise::new(),
            b: [[0.0; LANES]; 7],
        }
    }

    /// Green zone: pick the group's streams.
    pub fn seed(&mut self, seed: u64) {
        self.white.seed(seed);
        self.b = [[0.0; LANES]; 7];
    }

    /// Green zone: return every lane to its stream start, poles settled.
    pub fn reset(&mut self) {
        self.white.reset();
        self.b = [[0.0; LANES]; 7];
    }

    /// Green zone: restart ONE lane — stream and poles — leaving the rest
    /// of the group sounding.
    pub fn reset_lane(&mut self, lane: usize) {
        self.white.reset_lane(lane);
        for bank in self.b.iter_mut() {
            if let Some(v) = bank.get_mut(lane) {
                *v = 0.0;
            }
        }
    }

    /// Red zone: fill `out`, any length.
    // Kellett's published constants, verbatim — the same numbers the
    // scalar kernel uses, so the two cannot drift.
    #[allow(clippy::excessive_precision)]
    pub fn process(&mut self, out: &mut [LaneFrame]) {
        // Poles 0..=5 only. `b6` is not a pole: it is a one-sample
        // feedforward whose OLD value joins the sum before the new one is
        // stored, so it cannot ride the same loop — folding it in as a
        // pole with coefficient 0 would advance it a sample early and put
        // this kernel permanently out of step with the scalar one.
        const A: [f32; 6] = [0.99886, 0.99332, 0.96900, 0.86650, 0.55000, -0.7616];
        const G: [f32; 6] = [
            0.0555179, 0.0750759, 0.1538520, 0.3104856, 0.5329522, -0.0168980,
        ];
        let (poles, tail) = self.b.split_at_mut(6);
        for frame in out.iter_mut() {
            // The white source advances first, once per lane per sample,
            // so a group's pink stream stays a pure function of its seed.
            self.white.process(std::slice::from_mut(frame));
            for (lane, s) in frame.iter_mut().enumerate() {
                let w = *s;
                let mut acc = 0.0f32;
                for ((bank, a), g) in poles.iter_mut().zip(A).zip(G) {
                    // `get_mut` rather than `[lane]`: the red zone takes
                    // no indexing that could panic, even where the bound
                    // is a compile-time constant.
                    let Some(b) = bank.get_mut(lane) else {
                        continue;
                    };
                    *b = a * *b + w * g;
                    acc += *b;
                }
                let Some(b6) = tail.first_mut().and_then(|bank| bank.get_mut(lane)) else {
                    continue;
                };
                let pink = acc + *b6 + w * 0.5362;
                *b6 = w * 0.115926;
                *s = pink * PINK_SCALE;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dsp::filters::{Mode, Svf};

    const FS: f32 = 48_000.0;

    fn white(seed: u64, n: usize) -> Vec<f32> {
        let mut g = WhiteNoise::new();
        g.seed(seed);
        let mut out = vec![0.0f32; n];
        g.process(&mut out);
        out
    }

    fn pink(seed: u64, n: usize) -> Vec<f32> {
        let mut g = PinkNoise::new();
        g.seed(seed);
        let mut out = vec![0.0f32; n];
        g.process(&mut out);
        out
    }

    /// RMS of a signal after a constant-Q bandpass at `hz` — one octave
    /// band of a filter bank, built from our own SVF so the measurement
    /// exercises two kernels at once.
    fn band_rms_db(signal: &[f32], hz: f32) -> f64 {
        let mut f = Svf::new();
        f.prepare(FS, hz, 4.0);
        let mut buf = signal.to_vec();
        f.process(&mut buf, Mode::BandpassUnity);
        // Skip the filter's settling transient.
        let tail = &buf[buf.len() / 4..];
        let ms = tail.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / tail.len() as f64;
        10.0 * ms.max(1e-30).log10()
    }

    // -------------------------------------------------------- reference ---

    /// White noise is FLAT: through a constant-Q bank, band energy rises
    /// +3 dB per octave (bandwidth doubles per octave, density constant).
    /// The spectral claim, measured off the generator running.
    #[test]
    fn white_noise_is_spectrally_flat() {
        let signal = white(42, 1 << 18);
        let bands = [125.0f32, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0];
        let levels: Vec<f64> = bands.iter().map(|hz| band_rms_db(&signal, *hz)).collect();
        for (i, pair) in levels.windows(2).enumerate() {
            let step = pair[1] - pair[0];
            assert!(
                (step - 3.0).abs() < 1.0,
                "white octave {} -> {}: {step:+.2} dB, expected +3",
                bands[i],
                bands[i + 1]
            );
        }
    }

    /// Pink noise is EQUAL ENERGY PER OCTAVE: through the same bank the
    /// bands come out level. This is the definition of pink, not a
    /// restatement of Kellett's coefficients.
    #[test]
    fn pink_noise_is_flat_per_octave() {
        let signal = pink(42, 1 << 18);
        let bands = [125.0f32, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0];
        let levels: Vec<f64> = bands.iter().map(|hz| band_rms_db(&signal, *hz)).collect();
        for pair in levels.windows(2) {
            let step = pair[1] - pair[0];
            assert!(
                step.abs() < 1.25,
                "pink octave step {step:+.2} dB, expected ~0"
            );
        }
        let drift = levels.last().unwrap() - levels.first().unwrap();
        assert!(
            drift.abs() < 2.0,
            "pink drifts {drift:+.2} dB across 125 Hz-8 kHz"
        );
    }

    /// Amplitude behaviour, pinned: white is uniform in [-1, 1) with the
    /// statistics uniformity implies; pink's practical peak and RMS sit
    /// where the scale constant was chosen to put them.
    #[test]
    fn levels_are_what_the_headers_claim() {
        let w = white(7, 1 << 20);
        assert!(w.iter().all(|s| (-1.0..1.0).contains(s)), "white bounds");
        let mean = w.iter().map(|s| *s as f64).sum::<f64>() / w.len() as f64;
        assert!(mean.abs() < 0.005, "white mean {mean:.5}");
        let rms = (w.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / w.len() as f64).sqrt();
        assert!(
            (rms - 1.0 / 3f64.sqrt()).abs() < 0.005,
            "uniform RMS should be 1/sqrt(3), got {rms:.4}"
        );

        let p = pink(7, 1 << 20);
        let peak = p.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 1.0, "pink practical peak {peak:.3} escaped unity");
        assert!(peak > 0.3, "pink peak {peak:.3} suspiciously quiet");
        let rms = (p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / p.len() as f64).sqrt();
        assert!(
            (0.08..0.3).contains(&rms),
            "pink RMS {rms:.4} outside the documented range"
        );
        assert!(p.iter().all(|s| s.is_finite()));
    }

    /// Two seeds decorrelate; one seed is one stream, bit for bit; reset
    /// returns to the very first sample. The properties that make noise
    /// usable in a byte-identical bounce.
    #[test]
    fn seeds_decorrelate_and_streams_are_exact() {
        let a = white(1, 1 << 16);
        let b = white(2, 1 << 16);
        let dot: f64 = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (*x as f64) * (*y as f64))
            .sum();
        let na: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum();
        let nb: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum();
        let corr = dot / (na.sqrt() * nb.sqrt());
        assert!(corr.abs() < 0.02, "seeds 1 and 2 correlate at {corr:.4}");

        let again = white(1, 1 << 16);
        assert!(
            a.iter()
                .zip(&again)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );

        let mut g = PinkNoise::new();
        g.seed(9);
        let mut first = vec![0.0f32; 512];
        g.process(&mut first);
        g.reset();
        let mut back = vec![0.0f32; 512];
        g.process(&mut back);
        assert!(
            first
                .iter()
                .zip(&back)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }

    // ------------------------------------------- split-block equivalence ---

    #[test]
    fn split_block_is_bit_exact() {
        let mut a = PinkNoise::new();
        a.seed(3);
        let mut whole = vec![0.0f32; 256];
        a.process(&mut whole);

        let mut b = PinkNoise::new();
        b.seed(3);
        let mut split = vec![0.0f32; 256];
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "pink: 256 must equal 100 + 156"
        );

        let mut a = WhiteNoise::new();
        a.seed(3);
        let mut whole = vec![0.0f32; 256];
        a.process(&mut whole);
        let mut b = WhiteNoise::new();
        b.seed(3);
        let mut split = vec![0.0f32; 256];
        b.process(&mut split[..100]);
        b.process(&mut split[100..]);
        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits())
        );
    }

    // ------------------------------------------------------------ no-alloc ---

    #[test]
    fn process_does_not_allocate() {
        let mut w = WhiteNoise::new();
        let mut p = PinkNoise::new();
        let mut buf = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                w.process(&mut buf);
                p.process(&mut buf);
            }
        });
    }

    // -------------------------------------------------------- edge lengths ---

    #[test]
    fn any_block_length_is_accepted() {
        let mut w = WhiteNoise::new();
        let mut p = PinkNoise::new();
        for len in [0usize, 1, 3, 7, 63, 100] {
            let mut buf = vec![0.0f32; len];
            w.process(&mut buf);
            p.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()), "len {len}");
        }
    }

    // ---------------------------------------------------------------- cost ---

    /// What a sample costs. Printed, not asserted.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 20_000;
        let mut buf = vec![0.0f32; BLOCK];

        let mut row = |name: &str, run: &mut dyn FnMut(&mut [f32])| {
            for _ in 0..1_000 {
                run(&mut buf);
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run(&mut buf);
                // Without this the optimizer notices nothing reads the
                // buffer, deletes the loop, and reports 0.00 ns — which
                // this test did on its first run.
                std::hint::black_box(&mut buf);
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!("{name:<18} {ns:6.2} ns/sample");
        };
        let mut w = WhiteNoise::new();
        row("white", &mut |b| w.process(b));
        let mut p = PinkNoise::new();
        row("pink", &mut |b| p.process(b));
    }
    // --------------------------------------------------------- lanes ---

    fn lane_white(seed: u64, n: usize) -> Vec<LaneFrame> {
        let mut g = LaneWhiteNoise::new();
        g.seed(seed);
        let mut out = vec![[0.0f32; LANES]; n];
        g.process(&mut out);
        out
    }

    fn lane_pink(seed: u64, n: usize) -> Vec<LaneFrame> {
        let mut g = LanePinkNoise::new();
        g.seed(seed);
        let mut out = vec![[0.0f32; LANES]; n];
        g.process(&mut out);
        out
    }

    fn column(frames: &[LaneFrame], lane: usize) -> Vec<f32> {
        frames.iter().map(|f| f[lane]).collect()
    }

    /// REFERENCE. Every lane is BIT-IDENTICAL to the scalar kernel fed
    /// that lane's own seed.
    ///
    /// This is the strongest reference a lane kernel can have: the scalar
    /// versions are already tested against the spectral definitions of
    /// white and pink, so matching them bit for bit inherits all of it.
    /// It is also what catches a lane port that quietly restructured the
    /// arithmetic — the `b6` feedforward, for one, is a sample out of
    /// step if it is folded in as a pole.
    #[test]
    fn every_lane_is_bit_identical_to_the_scalar_kernel() {
        const N: usize = 4_096;
        let w = lane_white(9, N);
        let p = lane_pink(9, N);
        for lane in 0..LANES {
            let seed = lane_seed(9, lane);
            assert_eq!(column(&w, lane), white(seed, N), "white lane {lane}");
            assert_eq!(column(&p, lane), pink(seed, N), "pink lane {lane}");
        }
    }

    /// LANE INDEPENDENCE. The mandatory sixth test for a lane kernel.
    ///
    /// For a generator, independence means the lanes are DIFFERENT: eight
    /// unison voices sharing one stream would sum to one loud correlated
    /// noise instead of a stack. Pairwise-distinct is the weak half; the
    /// strong half is that no pair CORRELATES, which is what a shared
    /// generator with an offset would still show.
    ///
    /// The run length differs by colour, and that is the measurement
    /// talking rather than a fudge. A sample correlation estimator's
    /// spread goes as the number of INDEPENDENT samples, and pink is
    /// heavily lowpassed — neighbouring samples are not independent, so
    /// the same block of pink carries far fewer of them than white does.
    /// Measured worst pair, same generator, same seeds:
    ///
    /// ```text
    ///   white  2^14  |r| = 0.013
    ///   pink   2^14  |r| = 0.195
    ///   pink   2^18  |r| = 0.028
    /// ```
    ///
    /// Falling as sqrt(N) is the signature of independent streams
    /// measured too briefly, not of correlated ones — so pink gets the
    /// longer run and both get the same honest threshold.
    #[test]
    fn lanes_are_uncorrelated_streams() {
        for (label, frames) in [
            ("white", lane_white(5, 1 << 14)),
            ("pink", lane_pink(5, 1 << 18)),
        ] {
            for a in 0..LANES {
                for b in (a + 1)..LANES {
                    let (x, y) = (column(&frames, a), column(&frames, b));
                    assert_ne!(x, y, "{label}: lanes {a} and {b} are one stream");
                    // Normalized dot product: independent noise lands
                    // near zero, a shared or shifted stream does not.
                    let dot: f64 = x
                        .iter()
                        .zip(&y)
                        .map(|(u, v)| f64::from(*u) * f64::from(*v))
                        .sum();
                    let norm = |v: &[f32]| {
                        v.iter()
                            .map(|s| f64::from(*s) * f64::from(*s))
                            .sum::<f64>()
                            .sqrt()
                    };
                    let r = dot / (norm(&x) * norm(&y)).max(1e-30);
                    assert!(
                        r.abs() < 0.05,
                        "{label}: lanes {a},{b} correlate at r={r:.4}"
                    );
                }
            }
        }
    }

    /// A lane restarts alone: stealing one voice must not disturb the
    /// seven still sounding.
    #[test]
    fn resetting_one_lane_leaves_the_others_running() {
        const N: usize = 256;
        let mut g = LanePinkNoise::new();
        g.seed(3);
        let mut a = vec![[0.0f32; LANES]; N];
        g.process(&mut a);
        // Continue, but restart lane 2 first.
        let mut restarted = g;
        restarted.reset_lane(2);
        let (mut x, mut y) = (vec![[0.0f32; LANES]; N], vec![[0.0f32; LANES]; N]);
        g.process(&mut x);
        restarted.process(&mut y);
        for lane in 0..LANES {
            if lane == 2 {
                assert_eq!(column(&y, lane), column(&a, lane), "lane 2 did not restart");
            } else {
                assert_eq!(column(&y, lane), column(&x, lane), "lane {lane} disturbed");
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact. 256 in one go must equal 100
    /// then 156 — the property the segmented transport depends on.
    #[test]
    fn lane_noise_splits_bit_exactly() {
        const N: usize = 256;
        const CUT: usize = 100;

        let mut split = vec![[0.0f32; LANES]; N];
        let mut w = LaneWhiteNoise::new();
        w.seed(11);
        let (head, tail) = split.split_at_mut(CUT);
        w.process(head);
        w.process(tail);
        assert_eq!(split, lane_white(11, N), "white");

        let mut split = vec![[0.0f32; LANES]; N];
        let mut p = LanePinkNoise::new();
        p.seed(11);
        let (head, tail) = split.split_at_mut(CUT);
        p.process(head);
        p.process(tail);
        assert_eq!(split, lane_pink(11, N), "pink");
    }

    /// NO-ALLOC on the process path.
    #[test]
    fn lane_noise_does_not_allocate() {
        let mut w = LaneWhiteNoise::new();
        let mut p = LanePinkNoise::new();
        let mut buf = vec![[0.0f32; LANES]; 512];
        assert_no_alloc::assert_no_alloc(|| {
            w.process(&mut buf);
            p.process(&mut buf);
        });
    }

    /// EDGE LENGTHS: 0, 1, and a non-power-of-two.
    #[test]
    fn lane_noise_takes_any_block_length() {
        for n in [0usize, 1, 3, 97] {
            assert_eq!(lane_white(2, n).len(), n);
            assert_eq!(lane_pink(2, n).len(), n);
        }
        // A zero-length block must not advance the streams.
        let mut g = LaneWhiteNoise::new();
        g.seed(2);
        let untouched = g;
        g.process(&mut []);
        assert_eq!(g.state, untouched.state);
    }

    /// DENORMAL / FINITENESS: every lane stays finite and in range over a
    /// long run, and pink never invents a NaN out of its pole bank.
    #[test]
    fn lane_noise_stays_finite_and_bounded() {
        const N: usize = 1 << 16;
        for frame in &lane_white(7, N) {
            for s in frame {
                assert!(s.is_finite() && (-1.0..1.0).contains(s), "white {s}");
            }
        }
        for frame in &lane_pink(7, N) {
            for s in frame {
                assert!(s.is_finite() && s.abs() < 1.0, "pink {s}");
            }
        }
    }
}
