//! SHINE: the exciter.
//!
//! An Aphex Aural Exciter's trick and nothing more: take the top off
//! with a high-pass, drive it hard enough to grow harmonics that were
//! never there, and add it back under the whole sound. The harmonics
//! land above the tune point, so the sound gets air and presence
//! without the shelf lift that would just make it louder and harsher.
//!
//! The curve is asymmetric, so the harmonics are even — the octave
//! above rather than the fifth — which is what makes an exciter read as
//! sheen and not as distortion. AMOUNT is how hard the top is driven,
//! TUNE where the top starts, MIX how much of the generated top comes
//! back. At no amount the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::{Cascade, DcBlocker};
use crate::params::console::shine as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// 0..1.
    pub amount: f32,
    pub tune: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Shine.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            amount: clamp(p::AMOUNT) / 100.0,
            tune: clamp(p::TUNE),
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.amount == 0.0 || self.mix == 0.0
    }
}

pub struct ShineCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    split: [Cascade; 2],
    dc: [DcBlocker; 2],
    top: Vec<f32>,
    k: f32,
    back: f32,
    level_db: f32,
    heat: f32,
}

impl ShineCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            split: [Cascade::new(), Cascade::new()],
            dc: [DcBlocker::new(), DcBlocker::new()],
            top: vec![0.0; block.max(1)],
            k: 1.0,
            back: 0.0,
            level_db: -120.0,
            heat: 0.0,
        };
        for ch in 0..2 {
            core.dc[ch].prepare(sample_rate);
        }
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        for ch in 0..2 {
            self.split[ch].prepare(
                self.sample_rate,
                s.tune,
                core::f32::consts::FRAC_1_SQRT_2,
                p::SPLIT_ORDER,
                true,
            );
        }
        self.k = 1.0 + p::DRIVE * s.amount;
        self.back = p::RETURN * s.amount * s.mix;
    }

    /// The exciter's curve: asymmetric soft saturation, unit slope at
    /// zero, so quiet passages grow nothing and loud ones grow the
    /// octave.
    #[inline(always)]
    fn curve(x: f32, k: f32) -> f32 {
        let b = p::BIAS;
        let tb = b.tanh();
        let slope = k * (1.0 - tb * tb);
        ((k * x + b).tanh() - tb) / slope
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let top = &mut self.top[..n];
        top.copy_from_slice(io);
        self.split[ch].process(top);
        let mut hottest = 0.0f32;
        for t in top.iter_mut() {
            hottest = hottest.max((*t * self.k).abs());
            *t = Self::curve(*t, self.k);
        }
        self.dc[ch].process(top);
        self.heat = self.heat.max((hottest / 3.0).min(1.0));
        for (y, t) in io.iter_mut().zip(top.iter()) {
            *y += t * self.back;
        }
    }
}

impl SectionCore for ShineCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.split[ch].reset();
            self.dc[ch].reset();
        }
        self.level_db = -120.0;
        self.heat = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.top.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= 0.8;
        if !self.settings.is_wire() {
            self.run(0, l);
            if stereo {
                self.run(1, &mut r[..n]);
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
            reduction_db: -self.heat,
            bands: [self.settings.amount, self.settings.tune, 0.0],
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

    fn core_with(edits: &[(u32, f32)]) -> ShineCore {
        let mut params = SectionParams::of(SectionKind::Shine);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        ShineCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut ShineCore, l: &[f32]) -> Vec<f32> {
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

    fn harmonic_db(signal: &[f32], hz: f32, h: u32) -> f32 {
        let bin = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in signal.iter().enumerate() {
                let w = 2.0 * core::f32::consts::PI * f * i as f32 / FS;
                re += s * w.cos();
                im -= s * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        20.0 * (bin(hz * h as f32) / bin(hz).max(1e-9)).log10()
    }

    #[test]
    fn no_amount_is_a_wire_to_the_sample() {
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

    /// A tone above the tune point grows an octave; the same tone below
    /// it is left alone.
    #[test]
    fn it_grows_an_octave_above_the_tune_point_only() {
        let n = FS as usize / 2;
        let period = (FS / 6_000.0).round() as usize;
        let window = n / 2..n / 2 + period * 200;
        let l = sine(6_000.0, 0.4, n);
        let mut core = core_with(&[(p::AMOUNT, 100.0), (p::TUNE, 3_000.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let second = harmonic_db(&out[window], 6_000.0, 2);
        assert!(second > -35.0, "no octave: {second} dB");
        assert!(core.readout().reduction_db < 0.0, "no heat reported");

        let low = sine(300.0, 0.4, n);
        let mut core = core_with(&[(p::AMOUNT, 100.0), (p::TUNE, 3_000.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &low);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&low[n / 2..])).log10();
        assert!(change.abs() < 0.5, "the bottom was excited: {change} dB");
    }

    /// More amount is more sheen, and MIX scales what comes back.
    #[test]
    fn amount_and_mix_scale_what_comes_back() {
        let n = FS as usize / 4;
        let l = sine(6_000.0, 0.4, n);
        let added = |amount: f32, mix: f32| -> f32 {
            let mut core = core_with(&[(p::AMOUNT, amount), (p::TUNE, 3_000.0), (p::MIX, mix)]);
            let out = run(&mut core, &l);
            rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>())
        };
        let gentle = added(30.0, 100.0);
        let full = added(100.0, 100.0);
        assert!(full > gentle * 1.5, "gentle {gentle}, full {full}");
        let half = added(100.0, 50.0);
        assert!(
            (half / full - 0.5).abs() < 0.1,
            "half mix is {} of full",
            half / full
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(5_000.0, 0.4, 1000);
        let edits = [(p::AMOUNT, 70.0), (p::TUNE, 3_000.0), (p::MIX, 80.0)];
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
        core.set_param(p::AMOUNT, 500.0);
        assert_eq!(core.settings().amount, 1.0);
        core.set_param(p::TUNE, 10.0);
        assert_eq!(core.settings().tune, 1_000.0);
        core.set_param(99, 1.0);
        core.set_param(p::AMOUNT, 0.0);
        assert!(core.settings().is_wire());
    }
}
