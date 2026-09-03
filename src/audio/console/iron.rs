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
    /// One pole at the lift's own corner, per channel: the running
    /// bass content of the signal the core is actually fed.
    lp: [f32; 2],
    lp_a: f32,
    level_db: f32,
    heat: f32,
    asym: f32,
    bottom: f32,
}

impl IronCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
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
            lp: [0.0; 2],
            lp_a: 0.0,
            level_db: -120.0,
            heat: 0.0,
            asym: 0.0,
            bottom: 0.0,
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
        // The bass reader's pole, at the same corner the lift turns on,
        // so "the bottom" means exactly the band the lift hands the iron.
        let fs = if self.sample_rate.is_finite() && self.sample_rate > 0.0 {
            self.sample_rate
        } else {
            48_000.0
        };
        self.lp_a = (1.0 - (-core::f32::consts::TAU * p::LIFT_HZ / fs).exp()).clamp(0.0, 1.0);
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
        // How much of the flux driving the core is bottom. Read HERE —
        // after the lift, before the curve — because this is the signal
        // the iron sees, and the lift is the reason it is worth seeing.
        let a = self.lp_a;
        let (mut lo_peak, mut hi_peak) = (0.0f32, 0.0f32);
        for x in io.iter() {
            self.lp[ch] += a * (*x - self.lp[ch]);
            lo_peak = lo_peak.max(self.lp[ch].abs());
            hi_peak = hi_peak.max(x.abs());
        }
        self.bottom = self
            .bottom
            .max((lo_peak / hi_peak.max(p::TELEMETRY_EPS)).min(1.0));
        let lane = &mut self.lane[..n * 2];
        self.over[ch].up(io, lane);
        let (k, bias) = (self.k, self.bias);
        let mut hottest = 0.0f32;
        // The even-order term, measured where it is made: a curve with a
        // bias pushes the shaped signal off centre, and that offset IS
        // the second harmonic's home. The DC blocker below strips it out
        // of the sound; we read it on the way past.
        let (mut sum, mut shaped_peak) = (0.0f32, 0.0f32);
        for x in lane.iter_mut() {
            hottest = hottest.max((*x * k).abs());
            *x = curve(*x, k, bias);
            sum += *x;
            shaped_peak = shaped_peak.max(x.abs());
        }
        let mean = sum / lane.len() as f32;
        self.asym = self
            .asym
            .max((mean.abs() / shaped_peak.max(p::TELEMETRY_EPS) * p::ASYM_SCALE).min(1.0));
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
        self.lp = [0.0; 2];
        self.level_db = -120.0;
        self.heat = 0.0;
        self.asym = 0.0;
        self.bottom = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n * 2 > self.lane.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= p::TELEMETRY_DECAY;
        self.asym *= p::TELEMETRY_DECAY;
        self.bottom *= p::TELEMETRY_DECAY;
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

    /// A pure copy of fields measured in `process` — the graph calls
    /// this from its telemetry step, so it does no work.
    ///
    /// - `level_db`: the loudest sample leaving the stage this block, in
    ///   dBFS, over −120..0-and-a-bit. Per block, no smoothing: it is a
    ///   peak, not a meter.
    /// - `reduction_db`: **flux**, negated so the sign matches every
    ///   other section's reduction. `-heat`, where heat is 0..1 — the
    ///   hottest the curve's input got this block against a full-scale
    ///   of 3.0 — so the figure runs 0 (rest) down to −1 (the iron is
    ///   being pushed as hard as the reading goes).
    /// - `bands[0]`: **drive**, 0..1, the DRIVE setting normalised. A
    ///   setting, not a measurement, and it moves only when the hand
    ///   does — there is no time constant on it.
    /// - `bands[1]`: **asym**, 0..1, the even-harmonic content the curve
    ///   is making right now: the mean of the shaped, oversampled signal
    ///   as a fraction of its peak, times `ASYM_SCALE` (so a quarter of
    ///   the peak reads full), clamped. Zero for a symmetric or silent
    ///   block; grows with drive because the curve's bias walks with it.
    /// - `bands[2]`: **bottom**, 0..1, how much of what drives the core
    ///   is bass: the peak of a one-pole at `LIFT_HZ` over the peak of
    ///   the full band, both taken after the lift shelf and before the
    ///   curve. 1.0 is all bottom, 0.0 is nothing under the corner.
    ///
    /// All three of flux, asym and bottom share one ballistic: peak-hold
    /// within a block, then `TELEMETRY_DECAY` (0.8) of it carried into
    /// the next — a ~5 ms half-life at a 256-sample block, 48 kHz.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: -self.heat,
            bands: [self.drive, self.asym, self.bottom],
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
    fn probe_bands() {
        let n = FS as usize / 4;
        for hz in [60.0f32, 100.0, 1000.0] {
            for drive in [5.0f32, 20.0, 50.0, 100.0] {
                for amp in [0.05f32, 0.2, 0.5] {
                    let l = sine(hz, amp, n);
                    let mut core = core_with(drive);
                    let _ = run(&mut core, &l);
                    let rd = core.readout();
                    println!(
                        "hz {hz} drive {drive} amp {amp} -> asym {:.4} bottom {:.4} heat {:.4}",
                        rd.bands[1], rd.bands[2], -rd.reduction_db
                    );
                }
            }
        }
        let white: Vec<f32> = (0..n)
            .map(|i| {
                let x = (i as f32 * 12.9898).sin() * 43758.547;
                (x - x.floor()) * 0.6 - 0.3
            })
            .collect();
        for drive in [5.0f32, 100.0] {
            let mut core = core_with(drive);
            let _ = run(&mut core, &white);
            let rd = core.readout();
            println!(
                "noise drive {drive} -> asym {:.4} bottom {:.4}",
                rd.bands[1], rd.bands[2]
            );
        }
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
