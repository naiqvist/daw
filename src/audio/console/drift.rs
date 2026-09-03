//! DRIFT: chorus, flanger, vibrato and ensemble, on a bucket brigade.
//!
//! One kind of line, four ways of sweeping it. The line is a model of
//! the analog bucket-brigade delay every classic chorus and flanger
//! was built on: it is band-limited by its clock's anti-alias filters,
//! duller the longer it is; it has a floor of hiss; and it rounds off
//! at the top rather than clipping, in the loop as well as out of it,
//! which is why a flanger's feedback sings instead of screaming. The
//! sweep is a triangle for the chorus and the flanger, as the CE-1's
//! was, and a sine for the vibrato and the ensemble — and it is not a
//! metronome: a slow random walk wobbles the rate and the depth by a
//! few percent, so two bars never sweep quite alike.
//!
//! CHORUS is a line around seven milliseconds swept a few, mixed with
//! the dry; FLANGER a line around one millisecond swept a few, with
//! feedback; VIBRATO the chorus's line with no dry, which is a pitch
//! wobble; ENSEMBLE three lines a third of a turn apart under a slow
//! sweep, with a second fast, shallow sweep on top — the string
//! machine's. WIDTH offsets the right side's sweep from the left's.
//! MIX at zero is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::OnePole;
use crate::dsp::interp::hermite4;
use crate::dsp::lfo::{Lfo, LfoShape, SampleHold, SlewLimiter};
use crate::dsp::noise::WhiteNoise;
use crate::params::console::drift as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: u32,
    pub rate: f32,
    /// 0..1.
    pub depth: f32,
    /// −0.9..0.9.
    pub feedback: f32,
    pub width: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Drift.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            mode: (clamp(p::MODE).round().max(0.0) as u32).min(p::ENSEMBLE),
            rate: clamp(p::RATE),
            depth: clamp(p::DEPTH) / 100.0,
            feedback: clamp(p::FEEDBACK) / 100.0,
            width: clamp(p::WIDTH) / 100.0,
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }
}

/// A bucket brigade: a ring of samples read at a fractional delay,
/// with the brigade's band limit and its soft top in the loop.
struct Brigade {
    buf: Vec<f32>,
    write: usize,
    band: OnePole,
}

impl Brigade {
    fn new(sample_rate: f32, max_samples: usize) -> Self {
        let mut band = OnePole::new();
        band.prepare(sample_rate, p::BBD_LONG_HZ);
        Self {
            buf: vec![0.0; max_samples + 8],
            write: 0,
            band,
        }
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
        self.band.reset();
    }

    /// The soft top: rounds rather than clips.
    #[inline(always)]
    fn soft(x: f32) -> f32 {
        let k = p::BBD_KNEE;
        let y = x * k;
        y / (1.0 + y * y).sqrt() / k
    }

    /// Read at `delay` samples back, then write `x` plus the feedback
    /// of what was read. Returns the read, band-limited.
    #[inline(always)]
    fn tick(&mut self, x: f32, delay: f32, feedback: f32) -> f32 {
        let len = self.buf.len();
        let delay = delay.clamp(2.0, (len - 4) as f32);
        let pos = self.write as f32 - delay;
        let pos = if pos < 0.0 { pos + len as f32 } else { pos };
        let i = pos as usize;
        let frac = pos - i as f32;
        let at = |k: isize| self.buf[(i as isize + k).rem_euclid(len as isize) as usize];
        let read = hermite4(at(-1), at(0), at(1), at(2), frac);
        let read = self.band.tick_lowpass(read);
        self.buf[self.write] = Self::soft(x + feedback * read);
        self.write = (self.write + 1) % len;
        read
    }
}

/// One side's sweep: its LFO, and the ensemble's two others.
struct Sweep {
    lfo: Lfo,
    second: Lfo,
    third: Lfo,
    fast: Lfo,
}

impl Sweep {
    fn new(sample_rate: f32) -> Self {
        let mut make = || {
            let mut lfo = Lfo::new();
            lfo.prepare(sample_rate);
            lfo
        };
        Self {
            lfo: make(),
            second: make(),
            third: make(),
            fast: make(),
        }
    }
}

pub struct DriftCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    lines: [Brigade; 2],
    /// The ensemble's two more lines per side.
    seconds: [Brigade; 2],
    thirds: [Brigade; 2],
    sweeps: [Sweep; 2],
    drift: SampleHold,
    drift_slew: SlewLimiter,
    depth_drift: SampleHold,
    depth_slew: SlewLimiter,
    hiss: WhiteNoise,
    /// Compile-owned scratch: the drifts, the hiss, and the dry copy.
    rate_drift: Vec<f32>,
    depth_walk: Vec<f32>,
    noise: Vec<f32>,
    dry: Vec<f32>,
    level_db: f32,
    /// Where each side's sweep stood at the block's end, for the card.
    sweep_now: [f32; 2],
}

impl DriftCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let max = (sample_rate * p::MAX_MS / 1000.0).ceil() as usize + 4;
        let mut drift = SampleHold::new();
        drift.prepare(sample_rate);
        drift.seed(0x6472_6966);
        drift.set_rate(p::DRIFT_HZ);
        let mut drift_slew = SlewLimiter::new();
        drift_slew.prepare(sample_rate);
        drift_slew.set_rates(0.6, 0.6);
        let mut depth_drift = SampleHold::new();
        depth_drift.prepare(sample_rate);
        depth_drift.seed(0x6465_7074);
        depth_drift.set_rate(p::DRIFT_HZ * 0.7);
        let mut depth_slew = SlewLimiter::new();
        depth_slew.prepare(sample_rate);
        depth_slew.set_rates(0.5, 0.5);
        let mut hiss = WhiteNoise::new();
        hiss.seed(0x6262_6421);
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            lines: [
                Brigade::new(sample_rate, max),
                Brigade::new(sample_rate, max),
            ],
            seconds: [
                Brigade::new(sample_rate, max),
                Brigade::new(sample_rate, max),
            ],
            thirds: [
                Brigade::new(sample_rate, max),
                Brigade::new(sample_rate, max),
            ],
            sweeps: [Sweep::new(sample_rate), Sweep::new(sample_rate)],
            drift,
            drift_slew,
            depth_drift,
            depth_slew,
            hiss,
            rate_drift: vec![0.0; block.max(1)],
            depth_walk: vec![0.0; block.max(1)],
            noise: vec![0.0; block.max(1)],
            dry: vec![0.0; block.max(1)],
            level_db: -120.0,
            sweep_now: [0.0; 2],
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        let shape = match s.mode {
            p::CHORUS | p::FLANGER => LfoShape::Triangle,
            _ => LfoShape::Sine,
        };
        // The brigade's band: the short line is brighter.
        let band_hz = if s.mode == p::FLANGER {
            p::BBD_SHORT_HZ
        } else {
            p::BBD_LONG_HZ
        };
        for side in 0..2 {
            // The right side's sweep sits a share of a turn behind the
            // left's by the width; the ensemble's three a third apart.
            let offset = if side == 1 { 0.25 * s.width } else { 0.0 };
            let sweep = &mut self.sweeps[side];
            for (lfo, turn) in [
                (&mut sweep.lfo, 0.0),
                (&mut sweep.second, 1.0 / 3.0),
                (&mut sweep.third, 2.0 / 3.0),
            ] {
                lfo.set_shape(shape);
                lfo.set_rate(s.rate);
                lfo.set_phase(turn + offset);
            }
            sweep.fast.set_shape(LfoShape::Sine);
            sweep.fast.set_rate(p::ENSEMBLE_FAST_HZ);
            sweep.fast.set_phase(offset * 2.0);
            for line in [
                &mut self.lines[side],
                &mut self.seconds[side],
                &mut self.thirds[side],
            ] {
                line.band.prepare(self.sample_rate, band_hz);
            }
        }
    }

    fn run(&mut self, side: usize, io: &mut [f32]) {
        let n = io.len();
        let s = self.settings;
        let per_ms = self.sample_rate / 1000.0;
        let base = p::BASE_MS[s.mode as usize] * per_ms;
        let sweep_ms = p::SWEEP_MS[s.mode as usize];
        let hiss_gain = 10f32.powf(p::BBD_NOISE_DB / 20.0);
        let feedback = if s.mode == p::FLANGER {
            s.feedback
        } else {
            0.0
        };
        let dry = &mut self.dry[..n];
        dry.copy_from_slice(io);
        let sweep = &mut self.sweeps[side];
        let mut one = [0.0f32; 1];
        for i in 0..n {
            // The sweep, wobbled: the rate and the depth walk a little.
            let rate = s.rate * (1.0 + p::DRIFT_RATE * self.rate_drift[i]);
            let depth = (s.depth * (1.0 + p::DRIFT_DEPTH * self.depth_walk[i])).clamp(0.0, 1.0);
            let reach = sweep_ms * depth * per_ms;
            sweep.lfo.set_rate(rate);
            sweep.lfo.process(&mut one);
            let x = dry[i];
            let hiss = self.noise[i] * hiss_gain;
            let wet = if s.mode == p::ENSEMBLE {
                sweep.second.set_rate(rate);
                sweep.third.set_rate(rate);
                let mut two = [0.0f32; 1];
                let mut three = [0.0f32; 1];
                let mut fast = [0.0f32; 1];
                sweep.second.process(&mut two);
                sweep.third.process(&mut three);
                sweep.fast.process(&mut fast);
                let wobble = fast[0] * p::ENSEMBLE_FAST_MS * per_ms * depth;
                let a = self.lines[side].tick(x, base + one[0] * reach + wobble, 0.0);
                let b = self.seconds[side].tick(x, base + two[0] * reach - wobble, 0.0);
                let c = self.thirds[side].tick(x, base + three[0] * reach + wobble * 0.5, 0.0);
                (a + b + c) / 3.0
            } else {
                self.lines[side].tick(x, base + one[0] * reach, feedback)
            };
            io[i] = x + (wet + hiss - x) * s.mix;
        }
        self.sweep_now[side] = one[0];
    }
}

impl SectionCore for DriftCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for side in 0..2 {
            self.lines[side].reset();
            self.seconds[side].reset();
            self.thirds[side].reset();
            let sweep = &mut self.sweeps[side];
            for lfo in [
                &mut sweep.lfo,
                &mut sweep.second,
                &mut sweep.third,
                &mut sweep.fast,
            ] {
                lfo.reset();
            }
        }
        self.tune();
        self.drift.reset();
        self.drift_slew.reset();
        self.depth_drift.reset();
        self.depth_slew.reset();
        self.hiss.reset();
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.dry.len() {
            return;
        }
        let stereo = r.len() >= n;
        if !self.settings.is_wire() {
            // The walks and the hiss for this block, per sample.
            self.drift.process(&mut self.rate_drift[..n]);
            self.drift_slew.process(&mut self.rate_drift[..n]);
            self.depth_drift.process(&mut self.depth_walk[..n]);
            self.depth_slew.process(&mut self.depth_walk[..n]);
            self.hiss.process(&mut self.noise[..n]);
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
            reduction_db: 0.0,
            bands: [
                self.settings.mode as f32,
                self.sweep_now[0],
                self.sweep_now[1],
            ],
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

    fn core_with(edits: &[(u32, f32)]) -> DriftCore {
        let mut params = SectionParams::of(SectionKind::Drift);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        DriftCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut DriftCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn run_stereo(core: &mut DriftCore, l: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (l.to_vec(), l.to_vec());
        for start in (0..l.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(l.len());
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        (ol, or)
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// The periods between upward zero crossings, in samples.
    fn periods(signal: &[f32]) -> Vec<f32> {
        let mut last = None;
        let mut out = Vec::new();
        for i in 1..signal.len() {
            if signal[i - 1] <= 0.0 && signal[i] > 0.0 {
                let frac = signal[i - 1] / (signal[i - 1] - signal[i]);
                let at = (i - 1) as f32 + frac;
                if let Some(prev) = last {
                    out.push(at - prev);
                }
                last = Some(at);
            }
        }
        out
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
        let mut core = core_with(&[(p::MIX, 0.0), (p::DEPTH, 80.0)]);
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Vibrato is a pitch wobble: the periods of a tone through it
    /// vary, more with more depth, and not at all at none.
    #[test]
    fn vibrato_wobbles_the_pitch() {
        let n = FS as usize;
        let l = sine(440.0, 0.5, n);
        let spread = |depth: f32| -> f32 {
            let mut core = core_with(&[
                (p::MODE, 2.0),
                (p::MIX, 100.0),
                (p::DEPTH, depth),
                (p::RATE, 4.0),
            ]);
            let out = run(&mut core, &l);
            let periods = periods(&out[n / 2..]);
            let (lo, hi) = periods
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            hi - lo
        };
        assert!(
            spread(0.0) < 0.05,
            "no depth still wobbled: {}",
            spread(0.0)
        );
        assert!(
            spread(40.0) > 0.5,
            "some depth did not wobble: {}",
            spread(40.0)
        );
        assert!(spread(100.0) > spread(40.0) * 1.5);
    }

    /// The flanger, unswept and half mixed, is a comb: a tone at the
    /// first notch is cut and one at the first peak is not.
    #[test]
    fn the_flanger_is_a_comb() {
        let n = FS as usize / 4;
        let gain_at = |hz: f32| -> f32 {
            let l = sine(hz, 0.2, n);
            let mut core = core_with(&[
                (p::MODE, 1.0),
                (p::DEPTH, 0.0),
                (p::MIX, 50.0),
                (p::FEEDBACK, 0.0),
            ]);
            let out = run(&mut core, &l);
            20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
        };
        // The line sits at a millisecond: the notch at 500 Hz, the
        // peak at a kilohertz.
        let notch = gain_at(500.0);
        let peak = gain_at(1_000.0);
        assert!(notch < peak - 12.0, "notch {notch} dB, peak {peak} dB");
    }

    /// Feedback sharpens the comb and stays bounded, in either sign.
    #[test]
    fn feedback_sharpens_the_comb_and_holds() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, 0.2, n);
        let peak_at = |fb: f32| -> f32 {
            let mut core = core_with(&[
                (p::MODE, 1.0),
                (p::DEPTH, 0.0),
                (p::MIX, 50.0),
                (p::FEEDBACK, fb),
            ]);
            let out = run(&mut core, &l);
            assert!(
                out.iter().all(|s| s.abs() < 2.0),
                "the loop ran away at {fb}"
            );
            20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
        };
        let plain = peak_at(0.0);
        let fed = peak_at(80.0);
        assert!(
            fed > plain + 3.0,
            "feedback did not lift the peak: {plain} to {fed}"
        );
        let _ = peak_at(-90.0);
    }

    /// The brigade is band-limited: the wet loses its top.
    #[test]
    fn the_brigade_is_dull_on_top() {
        let n = FS as usize / 4;
        let gain_at = |hz: f32| -> f32 {
            let l = sine(hz, 0.2, n);
            let mut core = core_with(&[(p::MODE, 2.0), (p::MIX, 100.0), (p::DEPTH, 0.0)]);
            let out = run(&mut core, &l);
            20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
        };
        let low = gain_at(300.0);
        let high = gain_at(14_000.0);
        assert!(
            high < low - 3.0,
            "the top was kept: {low} dB against {high} dB"
        );
    }

    /// Width parts the sides: at none the two sweeps are one, at full
    /// they differ; the ensemble is thick either way.
    #[test]
    fn width_parts_the_sides_and_the_ensemble_is_three_deep() {
        let n = FS as usize / 2;
        let l = sine(440.0, 0.4, n);
        let apart = |mode: f32, width: f32| -> f32 {
            let mut core = core_with(&[
                (p::MODE, mode),
                (p::MIX, 100.0),
                (p::DEPTH, 60.0),
                (p::WIDTH, width),
            ]);
            let (ol, or) = run_stereo(&mut core, &l);
            rms(&ol[n / 2..]
                .iter()
                .zip(&or[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>())
        };
        assert!(apart(0.0, 0.0) < 1e-4, "no width still parted the sides");
        assert!(
            apart(0.0, 100.0) > 0.05,
            "full width did not part the sides"
        );
        assert!(apart(3.0, 100.0) > 0.05, "the ensemble's sides are one");
        let mut ensemble = core_with(&[(p::MODE, 3.0), (p::MIX, 100.0), (p::DEPTH, 60.0)]);
        let out = run(&mut ensemble, &l);
        assert!(out.iter().all(|s| s.abs() < 1.0));
        assert!(rms(&out[n / 2..]) > 0.15, "the ensemble is quiet");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::MODE, 0.0), (p::DEPTH, 50.0), (p::MIX, 50.0)];
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
            assert!((x - y).abs() < 1e-4);
        }
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::MODE, 9.0);
        assert_eq!(core.settings().mode, p::ENSEMBLE);
        core.set_param(p::FEEDBACK, 200.0);
        assert!((core.settings().feedback - 0.9).abs() < 1e-6);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }
}
