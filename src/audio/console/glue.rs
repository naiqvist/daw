//! GLUE: the bus compressor, with one knob.
//!
//! The same feedback design VCA runs on the channel — the detector
//! listens to the output, so the compressor regulates itself and leans
//! into the mix rather than grabbing it — but with everything except
//! the lean decided by the desk: two and a half to one, ten
//! milliseconds of attack, an auto release, a six dB knee. LEAN walks
//! the threshold from nothing down into the mix, and half of whatever
//! reduction it takes is given back, so leaning harder does not simply
//! turn the bus down.
//!
//! This one is never OUT. At LEAN zero the threshold is at the top of
//! the scale and nothing crosses it, which is as close to absent as a
//! bus compressor gets.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::arith::gain_to_db;
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::params::console::glue as p;

pub struct GlueCore {
    params: SectionParams,
    lean: f32,
    sample_rate: f32,
    detector: RmsDetector,
    computer: GainComputer,
    ballistics: Ballistics,
    reduction_db: f32,
    level_db: f32,
    most_reduced_db: f32,
}

impl GlueCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            lean: 0.0,
            sample_rate,
            detector: RmsDetector::new(),
            computer: GainComputer::new(),
            ballistics: Ballistics::new(),
            reduction_db: 0.0,
            level_db: -120.0,
            most_reduced_db: 0.0,
        };
        core.detector.prepare(sample_rate, p::DETECT_MS);
        core.ballistics
            .prepare(sample_rate, p::ATTACK_MS, p::RELEASE_MS);
        core.ballistics.set_auto(true);
        core.tune();
        core
    }

    /// How hard the desk is leaning, 0..1.
    pub fn lean(&self) -> f32 {
        self.lean
    }

    /// The threshold the lean stands at, in dBFS.
    pub fn threshold_db(&self) -> f32 {
        p::THRESHOLD_HIGH_DB + (p::THRESHOLD_LOW_DB - p::THRESHOLD_HIGH_DB) * self.lean
    }

    fn tune(&mut self) {
        let table = crate::console::SectionKind::Glue.table();
        let raw = self.params.value(p::LEAN);
        self.lean = table
            .iter()
            .find(|def| def.id == p::LEAN)
            .map_or(raw, |def| def.clamp(raw))
            / 100.0;
        self.computer
            .configure(Mode::Compress, self.threshold_db(), p::RATIO, p::KNEE_DB);
    }
}

impl SectionCore for GlueCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.tune();
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.ballistics.reset();
        self.reduction_db = 0.0;
        self.level_db = -120.0;
        self.most_reduced_db = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let mut most_reduced = 0.0f32;
        let mut loudest = -120.0f32;
        for i in 0..n {
            let gain = 10f32.powf(self.reduction_db / 20.0);
            let wet_l = l[i] * gain;
            let wet_r = if stereo { r[i] * gain } else { wet_l };
            // The detector hears the output, linked by the louder side.
            let side = wet_l.abs().max(wet_r.abs());
            let level = self.detector.tick(side);
            let level_db = gain_to_db(level.max(1e-6));
            let target = self.computer.gain_db(level_db);
            self.reduction_db = self.ballistics.tick(target);
            loudest = loudest.max(level_db);
            most_reduced = most_reduced.min(self.reduction_db);
            // Half of what was taken is given back.
            let makeup = 10f32.powf(-self.reduction_db * p::MAKEUP_SHARE / 20.0);
            l[i] = wet_l * makeup;
            if stereo {
                r[i] = wet_r * makeup;
            }
        }
        self.level_db = loudest;
        self.most_reduced_db = most_reduced;
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.most_reduced_db,
            bands: [self.reduction_db, self.lean, self.threshold_db()],
        }
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

    fn core_with(lean: f32) -> GlueCore {
        let mut params = SectionParams::of(SectionKind::Glue);
        params.set(p::LEAN, lean);
        GlueCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut GlueCore, l: &[f32]) -> Vec<f32> {
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

    /// At no lean the threshold is at the top and a mix passes
    /// untouched.
    #[test]
    fn no_lean_lets_the_mix_by() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, 0.25, n);
        let mut core = core_with(0.0);
        let out = run(&mut core, &l);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!(change.abs() < 0.5, "it leaned anyway: {change} dB");
    }

    /// Leaning takes gain, and leaning harder takes more.
    #[test]
    fn leaning_takes_gain_and_more_of_it_further_down() {
        let n = FS as usize;
        let l = sine(1_000.0, 0.5, n);
        let taken = |lean: f32| -> f32 {
            let mut core = core_with(lean);
            let _ = run(&mut core, &l);
            core.readout().reduction_db
        };
        let gentle = taken(50.0);
        let hard = taken(95.0);
        assert!(gentle < -0.5, "a gentle lean took nothing: {gentle} dB");
        assert!(hard < gentle - 2.0, "gentle {gentle} dB, hard {hard} dB");
    }

    /// The makeup keeps the bus's level up: leaning hard does not
    /// simply turn it down.
    #[test]
    fn half_of_what_it_takes_is_given_back() {
        let n = FS as usize;
        let l = sine(1_000.0, 0.5, n);
        let mut core = core_with(90.0);
        let out = run(&mut core, &l);
        let change = 20.0 * (rms(&out[n * 3 / 4..]) / rms(&l[n * 3 / 4..])).log10();
        let took = core.readout().reduction_db;
        assert!(took < -3.0, "it did not lean: {took} dB");
        assert!(
            (change - took * (1.0 - p::MAKEUP_SHARE)).abs() < 1.0,
            "took {took} dB, level moved {change} dB"
        );
    }

    /// Linked: a loud left and a quiet right move together.
    #[test]
    fn the_sides_are_linked() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, 0.5, n);
        let r = sine(1_000.0, 0.05, n);
        let (mut ol, mut or) = (l.clone(), r.clone());
        let mut core = core_with(80.0);
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        let left = 20.0 * (rms(&ol[n / 2..]) / rms(&l[n / 2..])).log10();
        let right = 20.0 * (rms(&or[n / 2..]) / rms(&r[n / 2..])).log10();
        assert!(
            (left - right).abs() < 0.2,
            "left {left} dB, right {right} dB"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.5, 2000);
        let mut whole = core_with(70.0);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(70.0);
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
    fn the_lean_clamps() {
        let mut core = core_with(0.0);
        core.set_param(p::LEAN, 500.0);
        assert_eq!(core.lean(), 1.0);
        assert!((core.threshold_db() - p::THRESHOLD_LOW_DB).abs() < 1e-6);
        core.set_param(99, 1.0);
    }
}
