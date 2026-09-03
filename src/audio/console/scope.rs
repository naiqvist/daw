//! SCOPE: the mix's analyser.
//!
//! It passes sound through untouched and says what it heard: the level,
//! and how much energy is in the bottom, the middle and the top. Three
//! bands because three is what the telemetry channel carries; a
//! bin-by-bin spectrum wants a channel of its own, and this one is
//! honest about what it is until that exists.
//!
//! The bands fall slowly so they can be read, and rise at once so a
//! transient is not missed.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::ramps::one_pole_coeff;
use crate::params::console::scope as p;

pub struct ScopeCore {
    bands: crate::dsp::crossover::Crossover3,
    /// Compile-owned scratch: the mono key, and the three bands.
    key: Vec<f32>,
    low: Vec<f32>,
    mid: Vec<f32>,
    high: Vec<f32>,
    /// Each band's level, linear, with the meter's fall.
    held: [f32; 3],
    fall: f32,
    level_db: f32,
}

impl ScopeCore {
    pub fn new(_params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut bands = crate::dsp::crossover::Crossover3::new();
        bands.prepare(sample_rate, p::LOW_HZ, p::HIGH_HZ);
        let n = block.max(1);
        Self {
            bands,
            key: vec![0.0; n],
            low: vec![0.0; n],
            mid: vec![0.0; n],
            high: vec![0.0; n],
            held: [0.0; 3],
            fall: one_pole_coeff(1000.0 / (p::FALL_MS * sample_rate)),
            level_db: -120.0,
        }
    }

    /// The three bands' levels in dBFS, low to high.
    pub fn bands_db(&self) -> [f32; 3] {
        self.held
            .map(|v| if v <= 1e-6 { -120.0 } else { 20.0 * v.log10() })
    }
}

impl SectionCore for ScopeCore {
    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn reset(&mut self) {
        self.bands.reset();
        self.held = [0.0; 3];
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.key.len() {
            return;
        }
        let stereo = r.len() >= n;
        // Nothing is changed: this section listens.
        let key = &mut self.key[..n];
        for i in 0..n {
            key[i] = if stereo { (l[i] + r[i]) * 0.5 } else { l[i] };
        }
        let (low, mid, high) = (&mut self.low[..n], &mut self.mid[..n], &mut self.high[..n]);
        self.bands.process(key, low, mid, high);
        // The fall is per SAMPLE, so a block's worth of it is the
        // coefficient raised to the block's length: how the meter reads
        // must not depend on how the sound was cut up.
        let keep = (1.0 - self.fall).powi(n as i32);
        for (slot, band) in self.held.iter_mut().zip([&*low, &*mid, &*high]) {
            let peak = band.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if peak > *slot {
                *slot = peak;
            } else {
                *slot *= keep;
            }
        }
        let peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: self.bands_db(),
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

    fn core() -> ScopeCore {
        ScopeCore::new(&SectionParams::of(SectionKind::Scope), FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut ScopeCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    /// It never touches the sound.
    #[test]
    fn it_is_a_wire_to_the_sample() {
        let mut core = core();
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// A tone lands in its own band and not the others.
    #[test]
    fn each_tone_lands_in_its_own_band() {
        for (hz, band) in [(60.0, 0usize), (900.0, 1), (9_000.0, 2)] {
            let mut core = core();
            let _ = run(&mut core, &sine(hz, 0.5, FS as usize / 4));
            let levels = core.bands_db();
            assert!(
                levels[band] > -12.0,
                "{hz} Hz is quiet in its band: {levels:?}"
            );
            for (other, level) in levels.iter().enumerate() {
                if other != band {
                    assert!(
                        *level < levels[band] - 12.0,
                        "{hz} Hz leaked into band {other}: {levels:?}"
                    );
                }
            }
        }
    }

    /// The bands rise at once and fall slowly.
    #[test]
    fn the_bands_rise_at_once_and_fall_slowly() {
        let mut core = core();
        let n = FS as usize / 10;
        let _ = run(&mut core, &sine(900.0, 0.5, n));
        let loud = core.bands_db()[1];
        assert!(loud > -12.0);
        // A fifth of a second of silence: the band has fallen, but not
        // to nothing.
        let _ = run(&mut core, &vec![0.0f32; n * 2]);
        let after = core.bands_db()[1];
        assert!(
            after < loud - 3.0,
            "the band did not fall: {loud} to {after}"
        );
        assert!(after > -80.0, "the band fell to nothing: {after}");
    }
}
