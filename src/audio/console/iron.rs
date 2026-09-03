//! IRON: the transformer, on every bus and the mix.
//!
//! What the preamp's IRON does to a channel, the desk does to the
//! whole bus: a low shelf lifts the bottom before an asymmetric curve
//! and takes it off after, so the bass drives the iron hardest and the
//! harmonics are even, and a DC blocker follows because a bent curve
//! makes DC. It runs at 2× so a hot bus does not alias.
//!
//! There is no bypass. The DRIVE knob's floor is above zero because a
//! transformer is always in the path — that is the whole idea of a
//! desk with iron in it — and at the floor the colour is slight rather
//! than absent.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::preamp_curve;
use crate::dsp::filters::{BandShape, DcBlocker, EqBand};
use crate::dsp::shaper::Oversampler2x;
use crate::params::console::iron as p;

/// The transformer's curve: asymmetric soft saturation whose bias —
/// and so whose even harmonics — grows with the drive, the way a core's
/// asymmetry grows with the flux through it. Unit slope at zero.
#[inline(always)]
fn curve(x: f32, k: f32, bias: f32) -> f32 {
    let tb = bias.tanh();
    let slope = k * (1.0 - tb * tb);
    ((k * x + bias).tanh() - tb) / slope
}

pub struct IronCore {
    params: SectionParams,
    drive: f32,
    sample_rate: f32,
    lift: [EqBand; 2],
    unlift: [EqBand; 2],
    over: [Oversampler2x; 2],
    dc: [DcBlocker; 2],
    lane: Vec<f32>,
    k: f32,
    bias: f32,
    makeup: f32,
    level_db: f32,
    heat: f32,
}

impl IronCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            drive: 0.0,
            sample_rate,
            lift: [EqBand::new(), EqBand::new()],
            unlift: [EqBand::new(), EqBand::new()],
            over: [Oversampler2x::new(), Oversampler2x::new()],
            dc: [DcBlocker::new(), DcBlocker::new()],
            lane: vec![0.0; Oversampler2x::scratch_len(block.max(1))],
            k: 1.0,
            bias: p::BIAS_FLOOR,
            makeup: 1.0,
            level_db: -120.0,
            heat: 0.0,
        };
        for ch in 0..2 {
            core.lift[ch].prepare(
                sample_rate,
                p::LIFT_HZ,
                0.7,
                p::LIFT_DB,
                BandShape::LowShelf,
            );
            core.unlift[ch].prepare(
                sample_rate,
                p::LIFT_HZ,
                0.7,
                -p::LIFT_DB,
                BandShape::LowShelf,
            );
            core.over[ch].prepare();
            core.dc[ch].prepare(sample_rate);
        }
        core.tune();
        core
    }

    /// How hard the iron is driven, 0..1.
    pub fn drive(&self) -> f32 {
        self.drive
    }

    fn tune(&mut self) {
        let table = crate::console::SectionKind::Iron.table();
        let raw = self.params.value(p::DRIVE);
        self.drive = table
            .iter()
            .find(|def| def.id == p::DRIVE)
            .map_or(raw, |def| def.clamp(raw))
            / 100.0;
        self.k = 1.0 + p::CURVE_DRIVE * self.drive;
        self.bias = p::BIAS_FLOOR + (p::BIAS_FULL - p::BIAS_FLOOR) * self.drive;
        // Unity for a −10 dBFS sine, whatever the drive: the iron is
        // there to colour a bus, not to turn it down. Measured rather
        // than derived — a bent curve's gain on a sine is not its gain
        // on a sample — by running one cycle through it here, green,
        // where a loop of sixty-four costs nothing.
        let at = p::UNITY_AT;
        let mut sum = 0.0f32;
        for i in 0..64 {
            let x = at * (core::f32::consts::TAU * i as f32 / 64.0).sin();
            let y = curve(x, self.k, self.bias);
            sum += y * y;
        }
        let out_rms = (sum / 64.0).sqrt();
        let in_rms = at / core::f32::consts::SQRT_2;
        self.makeup = (in_rms / out_rms.max(1e-6)).clamp(0.25, 4.0);
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        self.lift[ch].process(io);
        let lane = &mut self.lane[..n * 2];
        self.over[ch].up(io, lane);
        let (k, bias) = (self.k, self.bias);
        let mut hottest = 0.0f32;
        for x in lane.iter_mut() {
            hottest = hottest.max((*x * k).abs());
            *x = curve(*x, k, bias);
        }
        self.over[ch].down(lane, io);
        let makeup = self.makeup;
        if makeup != 1.0 {
            for x in io.iter_mut() {
                *x *= makeup;
            }
        }
        self.unlift[ch].process(io);
        self.dc[ch].process(io);
        self.heat = self.heat.max((hottest / 3.0).min(1.0));
    }
}

impl SectionCore for IronCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.tune();
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.lift[ch].reset();
            self.unlift[ch].reset();
            self.over[ch].reset();
            self.dc[ch].reset();
        }
        self.level_db = -120.0;
        self.heat = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n * 2 > self.lane.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= 0.8;
        self.run(0, l);
        if stereo {
            self.run(1, &mut r[..n]);
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
            bands: [self.drive, 0.0, 0.0],
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

    fn core_with(drive: f32) -> IronCore {
        let mut params = SectionParams::of(SectionKind::Iron);
        params.set(p::DRIVE, drive);
        IronCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut IronCore, l: &[f32]) -> Vec<f32> {
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

    /// Even harmonics, and more of them the harder it is driven.
    #[test]
    fn the_iron_is_even_and_grows_with_the_drive() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 480 * 20;
        let l = sine(100.0, 0.4, n);
        let second_at = |drive: f32| -> f32 {
            let mut core = core_with(drive);
            let out = run(&mut core, &l);
            harmonic_db(&out[window.clone()], 100.0, 2)
        };
        let floor = second_at(5.0);
        let full = second_at(100.0);
        assert!(full > floor + 6.0, "floor {floor} dB, full {full} dB");
        assert!(full > -40.0, "the iron is not colouring: {full} dB");
        let mut core = core_with(100.0);
        let out = run(&mut core, &l);
        let third = harmonic_db(&out[window], 100.0, 3);
        assert!(
            full > third,
            "the iron is odd: second {full}, third {third}"
        );
    }

    /// It colours without moving the level much, and never runs away.
    #[test]
    fn it_colours_without_changing_the_level() {
        let n = FS as usize / 2;
        let l = sine(100.0, 0.3, n);
        let mut core = core_with(100.0);
        let out = run(&mut core, &l);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!(change.abs() < 2.5, "the level moved {change} dB");
        assert!(out.iter().all(|s| s.abs() <= 1.0));
        assert!(core.readout().reduction_db < 0.0, "no heat reported");
    }

    /// There is no bypass: even at the floor the iron is in the path.
    #[test]
    fn there_is_no_bypass() {
        let mut core = core_with(0.0);
        assert!(core.drive() > 0.0, "the floor reached zero");
        let l = sine(100.0, 0.4, BLOCK);
        let out = run(&mut core, &l);
        assert!(out != l, "the floor was a wire");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let mut whole = core_with(60.0);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(60.0);
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
}
