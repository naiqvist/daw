//! The desk's calibrated, deterministic component bed.
//!
//! This is the physical bed beneath the console's individual PREAMP, DRIVE,
//! IRON, GLUE, TAPE, and CEILING processors. It establishes the electrical
//! reference, identity-stable component tolerances, self-noise/hum, and the
//! directional crosstalk kernel. The song graph owns the honest tap points:
//! it places one [`DeskPath`] on every channel and rail and one [`DeskBleed`]
//! for each direction between adjacent active channels.
//!
//! # Calibration
//!
//! `-18 dBFS == 0 VU`. Full scale is therefore `+18 VU`, and every constant
//! spelling that relationship is public so meters, fixtures, and later
//! nonlinear stages cannot quietly choose different references.
//!
//! # Determinism
//!
//! A path's traits and noise stream are pure functions of the persisted
//! project seed and a permanent path identity. Processing advances only by
//! samples, never blocks, so splitting a render does not change a bit. The
//! global noise defeat removes self-noise and hum only; component tolerance
//! remains in circuit so a measurement still measures the same desk.

/// The desk's nominal alignment.
pub const VU_REFERENCE_DBFS: f32 = -18.0;
/// Linear peak amplitude which reads 0 VU under that alignment.
pub const VU_REFERENCE_AMPLITUDE: f32 = 0.125_892_53;
/// Clean level available above 0 VU before digital full scale.
pub const NOMINAL_HEADROOM_DB: f32 = 18.0;
/// Ratio from 0 VU to full scale (`10^(18/20)`).
pub const NOMINAL_HEADROOM_GAIN: f32 = 7.943_282;

/// Maximum path gain error, either side of nominal.
pub const GAIN_TOLERANCE_DB: f32 = 0.15;
/// Nominal DC-removal corner, varied per path by [`DC_TOLERANCE`].
pub const DC_CORNER_HZ: f32 = 4.5;
/// Fractional bound around [`DC_CORNER_HZ`] (`3.6..=5.4 Hz`).
pub const DC_TOLERANCE: f32 = 0.20;
/// Nominal one-pole small-signal bandwidth.
pub const BANDWIDTH_HZ: f32 = 30_000.0;
/// Fractional bound around [`BANDWIDTH_HZ`] (`27..=33 kHz`).
pub const BANDWIDTH_TOLERANCE: f32 = 0.10;
/// Per-path white self-noise peak range. Uniform noise is 4.77 dB quieter
/// in RMS, so this is `-106.77..=-100.77 dBFS RMS` before paths sum.
pub const SELF_NOISE_DBFS: (f32, f32) = (-102.0, -96.0);
/// Per-path 60 Hz hum peak range.
pub const HUM_DBFS: (f32, f32) = (-112.0, -104.0);
/// The mains fundamental used by this original desk personality.
pub const HUM_HZ: f32 = 60.0;
/// Hard peak bound of noise plus hum at one path's injection point.
pub const PERSONALITY_BED_MAX_PEAK: f32 = 0.000_022_158_506;
/// Directional adjacent-path coupling at high frequencies. The low band is
/// roughly 13 dB quieter, like capacitive bleed between real desk traces.
pub const CROSSTALK_DB: (f32, f32) = (-78.0, -68.0);
/// Corner range of the frequency-dependent coupling.
pub const CROSSTALK_CORNER_HZ: (f32, f32) = (1_200.0, 3_200.0);

/// The seed older project documents acquire when the personality field is
/// absent. New-project code may replace it before first save; once persisted,
/// it is musical state and must not be regenerated on load.
pub const DEFAULT_PROJECT_SEED: u64 = 0xD35C_A11B_4A7E_2026;

const HASH_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const GAIN_SALT: u64 = 0x17D0_8E31_6C47_9A25;
const DC_SALT: u64 = 0xC6BC_2796_92B5_C323;
const BANDWIDTH_SALT: u64 = 0xD1B5_4A32_D192_ED03;
const NOISE_LEVEL_SALT: u64 = 0x94D0_49BB_1331_11EB;
const HUM_LEVEL_SALT: u64 = 0xBF58_476D_1CE4_E5B9;
const HUM_PHASE_SALT: u64 = 0xDB4F_0B91_75AE_2165;
const NOISE_STREAM_SALT: u64 = 0xA24B_AED4_963E_E407;
const CROSSTALK_LEVEL_SALT: u64 = 0x7A55_16B9_4C82_DF03;
const CROSSTALK_CORNER_SALT: u64 = 0xE4D9_71A2_38BC_605F;
const CROSSTALK_POLARITY_SALT: u64 = 0x29F6_C18D_A75B_430E;

/// One permanent path's bounded component draw.
///
/// Values are engine units rather than opaque normalized knobs so tests and
/// future graph wiring can state their real audible bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathTraits {
    pub gain_db: f32,
    pub dc_corner_hz: f32,
    pub bandwidth_hz: f32,
    pub self_noise_dbfs: f32,
    pub hum_dbfs: f32,
    /// Initial hum phase in turns (`0..1`), decorrelated between paths.
    pub hum_phase_turns: f32,
}

impl PathTraits {
    /// Green zone: derive one path without allocating or consulting process
    /// state. `identity` comes from a permanent TrackId or RailId, never its
    /// position in a Vec.
    pub fn derive(project_seed: u64, identity: u64) -> Self {
        let gain = bipolar(project_seed, identity, GAIN_SALT);
        let dc = bipolar(project_seed, identity, DC_SALT);
        let bandwidth = bipolar(project_seed, identity, BANDWIDTH_SALT);
        let noise = unipolar(project_seed, identity, NOISE_LEVEL_SALT);
        let hum = unipolar(project_seed, identity, HUM_LEVEL_SALT);
        Self {
            gain_db: gain * GAIN_TOLERANCE_DB,
            dc_corner_hz: DC_CORNER_HZ * (1.0 + dc * DC_TOLERANCE),
            bandwidth_hz: BANDWIDTH_HZ * (1.0 + bandwidth * BANDWIDTH_TOLERANCE),
            self_noise_dbfs: lerp(SELF_NOISE_DBFS.0, SELF_NOISE_DBFS.1, noise),
            hum_dbfs: lerp(HUM_DBFS.0, HUM_DBFS.1, hum),
            hum_phase_turns: unipolar(project_seed, identity, HUM_PHASE_SALT),
        }
    }
}

/// One mono desk path: bounded tolerance plus deterministic noise and hum.
///
/// Instantiate once per side of a stereo path with distinct permanent
/// identities. Free-running within a graph; reset at graph lifecycle or
/// transport discontinuities clear signal history without rewinding the
/// free-running component bed. Noise defeat is global configuration, not a
/// per-channel artistic control.
///
/// State: at most 128 bytes (pinned by test). Per-sample cost: 2 one-poles,
/// one integer noise draw, and a 60 Hz oscillator (4 f64 mul + 2 add; one
/// bounded renormalization every 4096 samples). Denormal-safe: states below
/// `1e-30` are flushed explicitly. In-place safe: yes. Latency: 0 samples.
#[derive(Debug)]
pub struct DeskPath {
    traits: PathTraits,
    gain: f32,
    dc_pole: f32,
    dc_x1: f32,
    dc_y1: f32,
    bandwidth_pole: f32,
    bandwidth_y1: f32,
    noise_peak: f32,
    hum_peak: f32,
    noise_seed: u64,
    noise_state: u64,
    hum_initial_sin: f64,
    hum_initial_cos: f64,
    hum_sin: f64,
    hum_cos: f64,
    hum_step_sin: f64,
    hum_step_cos: f64,
    hum_samples: u32,
    noise_enabled: bool,
}

impl Default for DeskPath {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskPath {
    pub fn new() -> Self {
        let traits = PathTraits::derive(DEFAULT_PROJECT_SEED, 0);
        let mut path = Self {
            traits,
            gain: 1.0,
            dc_pole: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            bandwidth_pole: 0.0,
            bandwidth_y1: 0.0,
            noise_peak: 0.0,
            hum_peak: 0.0,
            noise_seed: 0,
            noise_state: 0,
            hum_initial_sin: 0.0,
            hum_initial_cos: 1.0,
            hum_sin: 0.0,
            hum_cos: 1.0,
            hum_step_sin: 0.0,
            hum_step_cos: 1.0,
            hum_samples: 0,
            noise_enabled: true,
        };
        path.prepare(48_000.0, DEFAULT_PROJECT_SEED, 0, true);
        path
    }

    /// Green zone: set sample rate and the persisted personality coordinates.
    /// Invalid sample rates fall back to 48 kHz; every derived coefficient is
    /// finite and fixed until the next prepare.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        project_seed: u64,
        identity: u64,
        noise_enabled: bool,
    ) {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 1_000.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.traits = PathTraits::derive(project_seed, identity);
        self.gain = crate::dsp::arith::db_to_gain(self.traits.gain_db);
        self.dc_pole = (-core::f32::consts::TAU * self.traits.dc_corner_hz / sample_rate).exp();
        self.bandwidth_pole =
            (-core::f32::consts::TAU * self.traits.bandwidth_hz / sample_rate).exp();
        self.noise_peak = crate::dsp::arith::db_to_gain(self.traits.self_noise_dbfs);
        self.hum_peak = crate::dsp::arith::db_to_gain(self.traits.hum_dbfs);
        self.noise_seed = avalanche(project_seed ^ identity.rotate_left(23) ^ NOISE_STREAM_SALT);
        self.noise_enabled = noise_enabled;

        let phase = self.traits.hum_phase_turns as f64 * core::f64::consts::TAU;
        (self.hum_initial_sin, self.hum_initial_cos) = phase.sin_cos();
        let step = core::f64::consts::TAU * HUM_HZ as f64 / sample_rate as f64;
        (self.hum_step_sin, self.hum_step_cos) = step.sin_cos();
        self.reset();
    }

    /// Red-zone safe global measurement switch. Component tolerance remains
    /// active; only the additive bed is defeated.
    pub fn set_noise_enabled(&mut self, enabled: bool) {
        self.noise_enabled = enabled;
    }

    pub fn traits(&self) -> PathTraits {
        self.traits
    }

    pub const fn latency(&self) -> usize {
        0
    }

    /// Return signal and deterministic generators to their prepared origin.
    pub fn reset(&mut self) {
        self.reset_signal();
        self.noise_state = self.noise_seed;
        self.hum_sin = self.hum_initial_sin;
        self.hum_cos = self.hum_initial_cos;
        self.hum_samples = 0;
    }

    /// Clear only history which belongs to the signal's old timeline place.
    /// The console's physical noise and mains phase are free-running; seeking
    /// or wrapping a loop must not replay the same bed or create a periodic
    /// noise seam.
    pub fn reset_signal(&mut self) {
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.bandwidth_y1 = 0.0;
    }

    /// Red zone: process any block length in place.
    pub fn process(&mut self, io: &mut [f32]) {
        for sample in io.iter_mut() {
            let input = if sample.is_finite() { *sample } else { 0.0 };
            let driven = input * self.gain;
            let dc = zap(driven - self.dc_x1 + self.dc_pole * self.dc_y1);
            self.dc_x1 = driven;
            self.dc_y1 = dc;
            let bandwidth =
                zap((1.0 - self.bandwidth_pole)
                    .mul_add(dc, self.bandwidth_pole * self.bandwidth_y1));
            self.bandwidth_y1 = bandwidth;

            self.noise_state = self.noise_state.wrapping_add(HASH_GAMMA);
            let word = avalanche(self.noise_state);
            // Top 24 bits are exactly representable: uniform in [-1, 1).
            let white = ((word >> 40) as f32) * (2.0 / 16_777_216.0) - 1.0;
            let hum = self.hum_sin.clamp(-1.0, 1.0) as f32;
            *sample = if self.noise_enabled {
                bandwidth + white * self.noise_peak + hum * self.hum_peak
            } else {
                bandwidth
            };

            let next_sin = self
                .hum_sin
                .mul_add(self.hum_step_cos, self.hum_cos * self.hum_step_sin);
            let next_cos = self
                .hum_cos
                .mul_add(self.hum_step_cos, -(self.hum_sin * self.hum_step_sin));
            self.hum_sin = next_sin;
            self.hum_cos = next_cos;
            self.hum_samples = self.hum_samples.wrapping_add(1);
            // Fixed cadence, hence block-split invariant. This keeps a path
            // left running for days on the unit circle without paying sqrt
            // per sample.
            if self.hum_samples & 0x0fff == 0 {
                let magnitude = self
                    .hum_sin
                    .mul_add(self.hum_sin, self.hum_cos * self.hum_cos)
                    .sqrt();
                if magnitude.is_finite() && magnitude > 0.0 {
                    self.hum_sin /= magnitude;
                    self.hum_cos /= magnitude;
                } else {
                    self.hum_sin = self.hum_initial_sin;
                    self.hum_cos = self.hum_initial_cos;
                }
            }
        }
    }
}

/// One directional coupling path between adjacent physical channels.
///
/// This node receives the source channel after its fader and lands in the
/// neighbouring channel's assigned bus. It therefore cannot feed itself and
/// cannot create graph feedback. The coupling is deliberately tiny, stable by
/// permanent path identity, and brighter than its low band—the characteristic
/// shape of capacitive trace-to-trace bleed rather than a second dry send.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosstalkTraits {
    pub gain_db: f32,
    pub corner_hz: f32,
    pub polarity: f32,
}

impl CrosstalkTraits {
    pub fn derive(project_seed: u64, from_identity: u64, to_identity: u64) -> Self {
        let direction =
            avalanche(project_seed ^ from_identity.rotate_left(17) ^ to_identity.rotate_right(11));
        let level = unipolar(direction, to_identity, CROSSTALK_LEVEL_SALT);
        let corner = unipolar(direction, from_identity, CROSSTALK_CORNER_SALT);
        let polarity = if avalanche(direction ^ CROSSTALK_POLARITY_SALT) & 1 == 0 {
            -1.0
        } else {
            1.0
        };
        Self {
            gain_db: lerp(CROSSTALK_DB.0, CROSSTALK_DB.1, level),
            corner_hz: lerp(CROSSTALK_CORNER_HZ.0, CROSSTALK_CORNER_HZ.1, corner),
            polarity,
        }
    }
}

/// Callback-side state for one mono half of an adjacent-channel coupling.
/// State: four floats. Latency: zero. Split-block exact. No allocation.
#[derive(Clone, Copy, Debug)]
pub struct DeskBleed {
    traits: CrosstalkTraits,
    gain: f32,
    low_pole: f32,
    low: f32,
}

impl Default for DeskBleed {
    fn default() -> Self {
        Self::new()
    }
}

impl DeskBleed {
    pub fn new() -> Self {
        let mut bleed = Self {
            traits: CrosstalkTraits::derive(DEFAULT_PROJECT_SEED, 0, 1),
            gain: 0.0,
            low_pole: 0.0,
            low: 0.0,
        };
        bleed.prepare(48_000.0, DEFAULT_PROJECT_SEED, 0, 1);
        bleed
    }

    pub fn prepare(
        &mut self,
        sample_rate: f32,
        project_seed: u64,
        from_identity: u64,
        to_identity: u64,
    ) {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 1_000.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.traits = CrosstalkTraits::derive(project_seed, from_identity, to_identity);
        self.gain = crate::dsp::arith::db_to_gain(self.traits.gain_db) * self.traits.polarity;
        self.low_pole = (-core::f32::consts::TAU * self.traits.corner_hz / sample_rate).exp();
        self.reset();
    }

    pub fn traits(&self) -> CrosstalkTraits {
        self.traits
    }

    pub fn reset(&mut self) {
        self.low = 0.0;
    }

    pub fn process(&mut self, io: &mut [f32]) {
        let one_minus = 1.0 - self.low_pole;
        for sample in io {
            let input = if sample.is_finite() { *sample } else { 0.0 };
            self.low = zap(one_minus.mul_add(input, self.low_pole * self.low));
            let bright = input - self.low;
            // A small broadband floor keeps bass coupling real; the rising
            // high-frequency component dominates above the pair's corner.
            *sample = (0.22 * input + 0.78 * bright) * self.gain;
        }
    }
}

#[inline(always)]
fn zap(value: f32) -> f32 {
    if value.abs() < 1.0e-30 { 0.0 } else { value }
}

#[inline(always)]
fn avalanche(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[inline]
fn unipolar(seed: u64, identity: u64, salt: u64) -> f32 {
    let word = avalanche(seed ^ identity.rotate_left(23) ^ salt);
    ((word >> 40) as f32) * (1.0 / 16_777_216.0)
}

#[inline]
fn bipolar(seed: u64, identity: u64, salt: u64) -> f32 {
    unipolar(seed, identity, salt) * 2.0 - 1.0
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (b - a).mul_add(t, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u64 = 0x0123_4567_89AB_CDEF;
    const PATH: u64 = 0x5452_4143_4B00_002A;

    fn path(noise: bool) -> DeskPath {
        let mut path = DeskPath::new();
        path.prepare(48_000.0, SEED, PATH, noise);
        path
    }

    fn fixture(len: usize) -> Vec<f32> {
        (0..len)
            .map(|index| {
                let t = index as f32 / 48_000.0;
                (core::f32::consts::TAU * 997.0 * t).sin() * VU_REFERENCE_AMPLITUDE
            })
            .collect()
    }

    #[test]
    fn a_transport_discontinuity_does_not_replay_the_component_bed() {
        let mut continuous = path(true);
        let mut split = path(true);
        let mut expected = vec![0.0; 257];
        continuous.process(&mut expected);
        let mut first = vec![0.0; 113];
        let mut second = vec![0.0; 144];
        split.process(&mut first);
        split.reset_signal();
        split.process(&mut second);
        first.extend(second);
        assert_eq!(first, expected, "seek/loop reset rewound noise or hum");
    }

    #[test]
    fn calibration_is_minus_eighteen_dbfs_with_eighteen_db_of_headroom() {
        assert_eq!(VU_REFERENCE_DBFS, -18.0);
        assert_eq!(NOMINAL_HEADROOM_DB, 18.0);
        let reference = crate::dsp::arith::db_to_gain(VU_REFERENCE_DBFS);
        assert!((VU_REFERENCE_AMPLITUDE - reference).abs() < 2.0e-8);
        assert!((VU_REFERENCE_AMPLITUDE * NOMINAL_HEADROOM_GAIN - 1.0).abs() < 2.0e-7);
    }

    #[test]
    fn every_component_draw_stays_inside_the_published_bounds() {
        for seed in 0..128u64 {
            for identity in 0..64u64 {
                let traits = PathTraits::derive(seed, identity);
                assert!(traits.gain_db.abs() <= GAIN_TOLERANCE_DB);
                assert!(
                    (DC_CORNER_HZ * (1.0 - DC_TOLERANCE)..=DC_CORNER_HZ * (1.0 + DC_TOLERANCE))
                        .contains(&traits.dc_corner_hz)
                );
                assert!(
                    (BANDWIDTH_HZ * (1.0 - BANDWIDTH_TOLERANCE)
                        ..=BANDWIDTH_HZ * (1.0 + BANDWIDTH_TOLERANCE))
                        .contains(&traits.bandwidth_hz)
                );
                assert!((SELF_NOISE_DBFS.0..=SELF_NOISE_DBFS.1).contains(&traits.self_noise_dbfs));
                assert!((HUM_DBFS.0..=HUM_DBFS.1).contains(&traits.hum_dbfs));
                assert!((0.0..1.0).contains(&traits.hum_phase_turns));
            }
        }
    }

    #[test]
    fn contiguous_and_split_blocks_are_bit_exact_with_noise_and_hum() {
        let input = fixture(4_113);
        let mut whole = input.clone();
        let mut split = input;
        let mut a = path(true);
        let mut b = path(true);
        a.process(&mut whole);
        let (first, rest) = split.split_at_mut(997);
        b.process(first);
        let (middle, last) = rest.split_at_mut(2_048);
        b.process(middle);
        b.process(last);
        assert_eq!(whole, split);
    }

    #[test]
    fn process_does_not_allocate() {
        let mut path = path(true);
        let mut signal = [VU_REFERENCE_AMPLITUDE; 257];
        assert_no_alloc::assert_no_alloc(|| path.process(&mut signal));
    }

    #[test]
    fn zero_one_and_odd_lengths_are_valid() {
        let mut path = path(true);
        path.process(&mut []);
        let mut one = [0.0];
        path.process(&mut one);
        assert!(one[0].is_finite());
        let mut odd = [0.0; 37];
        path.process(&mut odd);
        assert!(odd.iter().all(|sample| sample.is_finite()));
        assert_eq!(path.latency(), 0);
    }

    #[test]
    fn measurement_defeat_is_exact_silence_but_keeps_tolerance() {
        let mut measured = path(false);
        let mut silence = [0.0f32; 511];
        measured.process(&mut silence);
        assert!(silence.iter().all(|sample| sample.to_bits() == 0));

        let mut input = fixture(511);
        let untouched = input.clone();
        measured.reset();
        measured.process(&mut input);
        assert_ne!(
            input, untouched,
            "measurement must not remove component tolerance"
        );
    }

    #[test]
    fn one_paths_additive_bed_never_exceeds_its_hard_peak_bound() {
        let mut path = path(true);
        let mut silence = [0.0f32; 48_000];
        path.process(&mut silence);
        let peak = silence
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak <= PERSONALITY_BED_MAX_PEAK, "bed peak {peak}");
        assert!(peak > 0.0);
    }

    #[test]
    fn reset_repeats_a_path_and_identity_changes_it() {
        let mut a = path(true);
        let mut first = [0.0f32; 257];
        let mut repeat = first;
        a.process(&mut first);
        a.reset();
        a.process(&mut repeat);
        assert_eq!(first, repeat);

        let mut b = DeskPath::new();
        b.prepare(48_000.0, SEED, PATH ^ 1, true);
        let mut other = [0.0f32; 257];
        b.process(&mut other);
        assert_ne!(first, other);
    }

    #[test]
    fn a_decaying_tail_flushes_and_never_invents_non_finite_values() {
        let mut path = path(false);
        let mut impulse = [0.0f32; 131_072];
        impulse[0] = 1.0;
        path.process(&mut impulse);
        assert!(impulse.iter().all(|sample| sample.is_finite()));
        assert_eq!(*impulse.last().unwrap_or(&1.0), 0.0);

        let mut invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        path.reset();
        path.process(&mut invalid);
        assert!(invalid.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn adjacent_path_draws_are_bounded_directional_and_repeatable() {
        for from in 0..64u64 {
            let a = CrosstalkTraits::derive(SEED, from, from + 1);
            let again = CrosstalkTraits::derive(SEED, from, from + 1);
            let reverse = CrosstalkTraits::derive(SEED, from + 1, from);
            assert_eq!(a, again);
            assert!((CROSSTALK_DB.0..=CROSSTALK_DB.1).contains(&a.gain_db));
            assert!((CROSSTALK_CORNER_HZ.0..=CROSSTALK_CORNER_HZ.1).contains(&a.corner_hz));
            assert!(a.polarity == -1.0 || a.polarity == 1.0);
            if from == 0 {
                assert_ne!(a, reverse, "the two directions have their own draw");
            }
        }
    }

    #[test]
    fn crosstalk_is_a_quiet_bright_split_exact_signal_path() {
        let input = fixture(4_113);
        let mut whole = input.clone();
        let mut split = input;
        let mut a = DeskBleed::new();
        let mut b = DeskBleed::new();
        a.prepare(48_000.0, SEED, PATH, PATH + 1);
        b.prepare(48_000.0, SEED, PATH, PATH + 1);
        a.process(&mut whole);
        let (first, rest) = split.split_at_mut(997);
        b.process(first);
        let (middle, last) = rest.split_at_mut(2_048);
        b.process(middle);
        b.process(last);
        assert_eq!(whole, split);
        let peak = whole
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak > 0.0 && peak < 0.000_1, "coupling peak {peak}");
        assert_no_alloc::assert_no_alloc(|| b.process(&mut split[..257]));
    }

    #[test]
    fn state_stays_inside_the_documented_size() {
        assert!(core::mem::size_of::<DeskPath>() <= 128);
        assert!(core::mem::size_of::<DeskBleed>() <= 32);
    }
}
