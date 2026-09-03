//! SMEAR: allpass dispersion.
//!
//! A run of allpass sections at one corner, up to thirty-two of them.
//! An allpass passes every frequency at full level and turns its phase,
//! and a run of them turns the phase of what is near the corner much
//! further than the rest — so a transient's low frequencies come out a
//! few milliseconds after its high ones. A snare becomes a laser, a
//! kick becomes a boing, and a whole mix becomes a smear. Nothing is
//! filtered: the magnitude is untouched to the sample, only the timing
//! of the parts moves.
//!
//! AMOUNT is how many sections run, CENTRE where they turn. At no
//! sections the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::Disperser;
use crate::params::console::smear as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub stages: u32,
    pub centre: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Smear.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            stages: clamp(p::AMOUNT).round().max(0.0) as u32,
            centre: clamp(p::CENTRE),
        }
    }

    pub fn is_wire(&self) -> bool {
        self.stages == 0
    }
}

pub struct SmearCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    dispersers: [Disperser; 2],
    level_db: f32,
}

impl SmearCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            settings: Settings::of(params),
            sample_rate,
            dispersers: [Disperser::new(), Disperser::new()],
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
        for disperser in &mut self.dispersers {
            disperser.prepare(self.sample_rate, s.centre, p::STAGE_Q, s.stages);
        }
    }
}

impl SectionCore for SmearCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for disperser in &mut self.dispersers {
            disperser.reset();
        }
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        if !self.settings.is_wire() {
            self.dispersers[0].process(l);
            if stereo {
                self.dispersers[1].process(&mut r[..n]);
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
            bands: [self.settings.stages as f32, self.settings.centre, 0.0],
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

    fn core_with(edits: &[(u32, f32)]) -> SmearCore {
        let mut params = SectionParams::of(SectionKind::Smear);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        SmearCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut SmearCore, l: &[f32]) -> Vec<f32> {
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

    #[test]
    fn no_stages_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Allpass: every tone comes out at the level it went in, near the
    /// corner and far from it.
    #[test]
    fn nothing_is_filtered() {
        let n = FS as usize / 2;
        for hz in [100.0, 1_000.0, 4_000.0, 12_000.0] {
            let l = sine(hz, 0.3, n);
            let mut core = core_with(&[(p::AMOUNT, 24.0), (p::CENTRE, 1_000.0)]);
            let out = run(&mut core, &l);
            let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
            assert!(change.abs() < 0.3, "{hz} Hz changed by {change} dB");
        }
    }

    /// A click is spread in time, and further with more sections: the
    /// time by which nine tenths of the energy has arrived grows.
    #[test]
    fn a_click_becomes_a_chirp() {
        let n = 8192;
        let mut l = vec![0.0f32; n];
        l[64] = 1.0;
        let arrival = |stages: f32| -> usize {
            let mut core = core_with(&[(p::AMOUNT, stages), (p::CENTRE, 1_000.0)]);
            let out = run(&mut core, &l);
            let total: f32 = out.iter().map(|s| s * s).sum();
            let mut so_far = 0.0;
            for (i, s) in out.iter().enumerate() {
                so_far += s * s;
                if so_far >= total * 0.9 {
                    return i - 64;
                }
            }
            n
        };
        let none = arrival(0.0);
        let short = arrival(4.0);
        let long = arrival(32.0);
        assert_eq!(none, 0, "a wire spread the click");
        assert!(short > 10, "four sections spread nothing: {short} samples");
        assert!(
            long > short * 2,
            "thirty-two ({long}) against four ({short})"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::AMOUNT, 16.0), (p::CENTRE, 800.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
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
        core.set_param(p::AMOUNT, 99.0);
        assert_eq!(core.settings().stages, 32);
        core.set_param(p::CENTRE, 10.0);
        assert_eq!(core.settings().centre, 100.0);
        core.set_param(99, 1.0);
        core.set_param(p::AMOUNT, 0.0);
        assert!(core.settings().is_wire());
    }
}
