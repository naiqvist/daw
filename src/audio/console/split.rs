//! SPLIT: three-band dynamics on a Linkwitz–Riley crossover.
//!
//! The sound is cut into LOW, MID and HIGH at two corners by
//! `dsp::crossover::Crossover3`, which reconstructs: do nothing to the
//! bands and their sum is the input. Each band then has one lever and
//! one gain. The lever is an AMOUNT, not a threshold and a ratio: at
//! positive amounts the band is held toward its own long average —
//! louder moments pulled down, quieter moments pushed up, the way the
//! "over the top" multiband does it — and at negative amounts it is
//! pushed away from it, its dynamics exaggerated. Held against its own
//! average, a band needs no threshold and the lever works at any level.
//! The reach is twelve dB either way. Each band's ballistics are fixed
//! and its own: the bottom slow, the top quick.
//!
//! Linked across the channels by the louder side, so the image holds.
//! At every lever and gain at rest the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::arith::gain_to_db;
use crate::dsp::crossover::Crossover3;
use crate::dsp::dynamics::RmsDetector;
use crate::dsp::ramps::one_pole_coeff;
use crate::params::console::split as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub low_hz: f32,
    pub high_hz: f32,
    /// −1..1 per band.
    pub amount: [f32; 3],
    pub gain_db: [f32; 3],
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Split.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        let low_hz = clamp(p::LOW_HZ);
        Self {
            low_hz,
            // The corners never cross: the high one is at least an
            // octave over the low one.
            high_hz: clamp(p::HIGH_HZ).max(low_hz * 2.0),
            amount: [clamp(p::LOW), clamp(p::MID), clamp(p::HIGH)].map(|v| v / 100.0),
            gain_db: [clamp(p::LOW_DB), clamp(p::MID_DB), clamp(p::HIGH_DB)],
        }
    }

    pub fn is_rest(&self) -> bool {
        self.amount.iter().all(|a| *a == 0.0) && self.gain_db.iter().all(|g| *g == 0.0)
    }
}

/// One band's dynamics: its detector, its long average, its gain.
struct Band {
    detector: RmsDetector,
    average: f32,
    average_coeff: f32,
    attack: f32,
    release: f32,
    /// The band's dynamic gain, in dB, smoothed.
    gain_db: f32,
    trim: f32,
}

impl Band {
    fn new(sample_rate: f32, index: usize) -> Self {
        let mut detector = RmsDetector::new();
        detector.prepare(sample_rate, p::DETECT_MS[index]);
        Self {
            detector,
            average: 0.0,
            average_coeff: one_pole_coeff(1000.0 / (p::AVERAGE_MS * sample_rate)),
            attack: one_pole_coeff(1000.0 / (p::ATTACK_MS[index] * sample_rate)),
            release: one_pole_coeff(1000.0 / (p::RELEASE_MS[index] * sample_rate)),
            gain_db: 0.0,
            trim: 1.0,
        }
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.average = 0.0;
        self.gain_db = 0.0;
    }

    /// One sample of the band's key: the gain to apply, linear.
    #[inline(always)]
    fn tick(&mut self, key: f32, amount: f32) -> f32 {
        let level = self.detector.tick(key);
        // The long average follows the level, so the band is held
        // against itself; it settles slowly and rises no faster.
        self.average += (level - self.average) * self.average_coeff;
        let target = if amount == 0.0 || self.average <= 1e-5 {
            0.0
        } else {
            let above = gain_to_db(level.max(1e-6)) - gain_to_db(self.average.max(1e-6));
            (-amount * above).clamp(-p::REACH_DB, p::REACH_DB)
        };
        let coeff = if target < self.gain_db {
            self.attack
        } else {
            self.release
        };
        self.gain_db += (target - self.gain_db) * coeff;
        10f32.powf(self.gain_db / 20.0) * self.trim
    }
}

pub struct SplitCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    crossover: [Crossover3; 2],
    bands: [Band; 3],
    /// Compile-owned scratch: three bands per channel.
    scratch: Vec<f32>,
    level_db: f32,
}

impl SplitCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            crossover: [Crossover3::new(), Crossover3::new()],
            bands: [
                Band::new(sample_rate, 0),
                Band::new(sample_rate, 1),
                Band::new(sample_rate, 2),
            ],
            scratch: vec![0.0; block.max(1) * 6],
            level_db: -120.0,
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        for crossover in &mut self.crossover {
            crossover.prepare(self.sample_rate, s.low_hz, s.high_hz);
        }
        for (band, gain_db) in self.bands.iter_mut().zip(s.gain_db) {
            band.trim = 10f32.powf(gain_db / 20.0);
        }
    }
}

impl SectionCore for SplitCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for crossover in &mut self.crossover {
            crossover.reset();
        }
        for band in &mut self.bands {
            band.reset();
        }
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n * 6 > self.scratch.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if s.is_rest() {
            self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
            return;
        }
        // The bands: low, mid, high for the left, then for the right.
        let (left, right) = self.scratch.split_at_mut(n * 3);
        let (ll, rest) = left.split_at_mut(n);
        let (lm, lh) = rest.split_at_mut(n);
        let (rl, rest) = right.split_at_mut(n);
        let (rm, rh) = rest.split_at_mut(n);
        self.crossover[0].process(l, ll, lm, lh);
        if stereo {
            self.crossover[1].process(&r[..n], rl, rm, rh);
        }
        for i in 0..n {
            let mut out_l = 0.0;
            let mut out_r = 0.0;
            for (b, band) in self.bands.iter_mut().enumerate() {
                let (xl, xr) = match b {
                    0 => (ll[i], if stereo { rl[i] } else { ll[i] }),
                    1 => (lm[i], if stereo { rm[i] } else { lm[i] }),
                    _ => (lh[i], if stereo { rh[i] } else { lh[i] }),
                };
                let gain = band.tick(xl.abs().max(xr.abs()), s.amount[b]);
                out_l += xl * gain;
                out_r += xr * gain;
            }
            l[i] = out_l;
            if stereo {
                r[i] = out_r;
            }
        }
        self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self
                .bands
                .iter()
                .map(|band| band.gain_db)
                .fold(0.0f32, f32::min),
            bands: [
                self.bands[0].gain_db,
                self.bands[1].gain_db,
                self.bands[2].gain_db,
            ],
        }
    }
}

fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= 1e-6 {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> SplitCore {
        let mut params = SectionParams::of(SectionKind::Split);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        SplitCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut SplitCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// A tone that is quiet for a second, then loud for a second, then
    /// quiet again: what dynamics act on.
    fn swell(hz: f32) -> Vec<f32> {
        let n = FS as usize * 3;
        (0..n)
            .map(|i| {
                let amp = if (FS as usize..2 * FS as usize).contains(&i) {
                    0.5
                } else {
                    0.05
                };
                amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin()
            })
            .collect()
    }

    /// The loud second's level against the quiet third's, in dB.
    fn contrast_db(signal: &[f32]) -> f32 {
        let s = FS as usize;
        let loud = rms(&signal[s + s / 2..2 * s - s / 10]);
        let quiet = rms(&signal[2 * s + s / 2..3 * s - s / 10]);
        20.0 * (loud / quiet).log10()
    }

    #[test]
    fn at_rest_it_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_rest());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// With every lever at rest, a gain on one band moves that band
    /// and leaves the others alone: the crossover reconstructs.
    #[test]
    fn a_band_gain_moves_its_band_and_the_split_reconstructs() {
        let n = FS as usize / 2;
        let gain_at = |edits: &[(u32, f32)], hz: f32| -> f32 {
            let l = sine(hz, 0.1, n);
            let mut core = core_with(edits);
            let out = run(&mut core, &l);
            20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
        };
        let low = [(p::LOW_DB, -12.0)];
        assert!((gain_at(&low, 50.0) + 12.0).abs() < 1.0);
        assert!(gain_at(&low, 800.0).abs() < 0.5);
        assert!(gain_at(&low, 8_000.0).abs() < 0.5);
        let high = [(p::HIGH_DB, 6.0)];
        assert!((gain_at(&high, 10_000.0) - 6.0).abs() < 1.0);
        assert!(gain_at(&high, 300.0).abs() < 0.5);
        // The flat split, on a signal across the corners, is a wire but
        // for the allpass phase: the level is the same.
        let mixed = [(p::MID_DB, 0.0001)];
        assert!(gain_at(&mixed, 150.0).abs() < 0.3);
        assert!(gain_at(&mixed, 2_500.0).abs() < 0.3);
    }

    /// A positive amount on a band squashes its swell; a negative one
    /// exaggerates it; the other bands' swells are untouched.
    #[test]
    fn the_lever_squashes_or_exaggerates_its_own_band() {
        let low_tone = swell(60.0);
        let before = contrast_db(&low_tone);
        assert!((before - 20.0).abs() < 0.5);

        let mut squashed = core_with(&[(p::LOW, 100.0)]);
        let out = run(&mut squashed, &low_tone);
        let after = contrast_db(&out);
        assert!(after < before - 6.0, "not squashed: {before} to {after} dB");

        let mut exaggerated = core_with(&[(p::LOW, -100.0)]);
        let out = run(&mut exaggerated, &low_tone);
        let after = contrast_db(&out);
        assert!(
            after > before + 6.0,
            "not exaggerated: {before} to {after} dB"
        );

        let high_tone = swell(6_000.0);
        let mut low_only = core_with(&[(p::LOW, 100.0)]);
        let out = run(&mut low_only, &high_tone);
        let after = contrast_db(&out);
        assert!(
            (after - contrast_db(&high_tone)).abs() < 0.5,
            "the top moved: {after} dB"
        );
    }

    /// Held against its own average, the lever works at any level.
    #[test]
    fn the_lever_does_not_care_how_loud_the_band_is() {
        let quiet: Vec<f32> = swell(60.0).iter().map(|s| s * 0.1).collect();
        let loud = swell(60.0);
        let mut a = core_with(&[(p::MID, 0.0), (p::LOW, 100.0)]);
        let mut b = core_with(&[(p::MID, 0.0), (p::LOW, 100.0)]);
        let squashed_quiet = contrast_db(&run(&mut a, &quiet));
        let squashed_loud = contrast_db(&run(&mut b, &loud));
        assert!(
            (squashed_quiet - squashed_loud).abs() < 1.0,
            "{squashed_quiet} vs {squashed_loud}"
        );
    }

    /// The corners never cross: a high corner asked under the low one
    /// lands an octave above it.
    #[test]
    fn the_corners_keep_their_order() {
        let core = core_with(&[(p::LOW_HZ, 500.0), (p::HIGH_HZ, 1_000.0)]);
        assert_eq!(core.settings().high_hz, 1_000.0);
        let core = core_with(&[(p::LOW_HZ, 500.0), (p::HIGH_HZ, 600.0)]);
        assert!(core.settings().high_hz >= 1_000.0);
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l: Vec<f32> = swell(200.0)[FS as usize - 500..FS as usize + 1500].to_vec();
        let edits = [(p::LOW, 60.0), (p::MID, -40.0), (p::HIGH_DB, 3.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244, 256, 256, 256, 232] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::LOW, 500.0);
        assert_eq!(core.settings().amount[0], 1.0);
        core.set_param(p::HIGH_DB, -40.0);
        assert_eq!(core.settings().gain_db[2], -12.0);
        core.set_param(99, 1.0);
        core.set_param(p::LOW, 0.0);
        core.set_param(p::HIGH_DB, 0.0);
        assert!(core.settings().is_rest());
    }
}
