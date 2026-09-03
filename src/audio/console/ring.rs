//! RING: ring modulation.
//!
//! The sound multiplied by a carrier, which is the one effect that
//! moves every partial by the same number of hertz instead of the same
//! ratio — so what comes out is inharmonic, bell-like, and nothing a
//! filter or a distortion can imitate. Four carriers: a band-limited
//! SINE, TRIANGLE and SQUARE from the oscillator's mip tables, so a
//! high carrier does not alias, and NOISE, low-passed, which turns the
//! sound to gravel rather than to a bell.
//!
//! HOLD is the thing a plain ring modulator lacks: a sample-and-hold
//! walks the carrier's pitch by up to an octave and a half, stepping on
//! its own clock, so the metal moves. MIX blends against the dry and at
//! zero the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::OnePole;
use crate::dsp::lfo::SampleHold;
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{MipOsc, Waveform, build_tables, table_len};
use crate::params::console::ring as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub carrier: u32,
    pub hz: f32,
    pub hold_rate: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Ring.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            carrier: (clamp(p::CARRIER).round().max(0.0) as u32).min(p::NOISE),
            hz: clamp(p::HZ),
            hold_rate: clamp(p::HOLD_RATE),
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }
}

fn waveform_of(carrier: u32) -> Waveform {
    match carrier {
        p::TRIANGLE => Waveform::Triangle,
        p::SQUARE => Waveform::Square,
        _ => Waveform::Sine,
    }
}

pub struct RingCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    osc: MipOsc,
    /// The tables the oscillator reads, built for the carrier in hand.
    tables: Vec<f32>,
    built: Waveform,
    noise: WhiteNoise,
    band: OnePole,
    hold: SampleHold,
    /// Compile-owned scratch: the carrier, and the hold's walk.
    carrier: Vec<f32>,
    walk: Vec<f32>,
    level_db: f32,
}

impl RingCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let settings = Settings::of(params);
        let waveform = waveform_of(settings.carrier);
        let mut tables = vec![0.0; table_len(waveform)];
        build_tables(waveform, &mut tables);
        let mut osc = MipOsc::new();
        osc.prepare(sample_rate, waveform);
        let mut noise = WhiteNoise::new();
        noise.seed(0x7269_6e67);
        let mut band = OnePole::new();
        band.prepare(sample_rate, p::NOISE_HZ);
        let mut hold = SampleHold::new();
        hold.prepare(sample_rate);
        hold.seed(0x686f_6c64);
        let mut core = Self {
            params: params.clone(),
            settings,
            sample_rate,
            osc,
            tables,
            built: waveform,
            noise,
            band,
            hold,
            carrier: vec![0.0; block.max(1)],
            walk: vec![0.0; block.max(1)],
            level_db: -120.0,
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Green zone: the carrier's tables, when the shape changed.
    fn tune(&mut self) {
        let s = self.settings;
        let waveform = waveform_of(s.carrier);
        if waveform != self.built || self.tables.len() != table_len(waveform) {
            self.tables = vec![0.0; table_len(waveform)];
            build_tables(waveform, &mut self.tables);
            self.osc.prepare(self.sample_rate, waveform);
            self.built = waveform;
        }
        self.osc.set_freq(s.hz);
        self.hold.set_rate(s.hold_rate.max(0.001));
    }
}

impl SectionCore for RingCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.noise.reset();
        self.band.reset();
        self.hold.reset();
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.carrier.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if !s.is_wire() {
            let carrier = &mut self.carrier[..n];
            if s.carrier == p::NOISE {
                self.noise.process(carrier);
                self.band.process_lowpass(carrier);
                // The band-limited noise is quiet; bring it back to a
                // modulator's range.
                for c in carrier.iter_mut() {
                    *c = (*c * 4.0).clamp(-1.0, 1.0);
                }
            } else if s.hold_rate > 0.0 {
                // The walk steps the pitch; the oscillator is retuned
                // per sample, which is what it is built for.
                self.hold.process(&mut self.walk[..n]);
                for (c, step) in carrier.iter_mut().zip(&self.walk[..n]) {
                    self.osc.set_freq(s.hz * 2f32.powf(p::HOLD_OCTAVES * *step));
                    let mut one = [0.0f32; 1];
                    self.osc.process(&mut one, &self.tables);
                    *c = one[0];
                }
            } else {
                self.osc.set_freq(s.hz);
                self.osc.process(carrier, &self.tables);
            }
            for (y, c) in l.iter_mut().zip(carrier.iter()) {
                *y += (*y * *c - *y) * s.mix;
            }
            if stereo {
                for (y, c) in r[..n].iter_mut().zip(carrier.iter()) {
                    *y += (*y * *c - *y) * s.mix;
                }
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
                self.settings.carrier as f32,
                self.settings.hz,
                self.settings.mix,
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

    fn core_with(edits: &[(u32, f32)]) -> RingCore {
        let mut params = SectionParams::of(SectionKind::Ring);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        RingCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut RingCore, l: &[f32]) -> Vec<f32> {
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

    /// The level at `hz`, in dB relative to `signal`'s loudest bin of
    /// the three tested.
    fn level_at(signal: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in signal.iter().enumerate() {
            let w = 2.0 * core::f32::consts::PI * hz * i as f32 / FS;
            re += s * w.cos();
            im -= s * w.sin();
        }
        let mag = (re * re + im * im).sqrt() * 2.0 / signal.len() as f32;
        20.0 * mag.max(1e-9).log10()
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
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

    /// The sidebands are the sum and the difference, and the tone
    /// itself is gone: that is what ring modulation is.
    #[test]
    fn it_makes_the_sum_and_the_difference_and_keeps_neither() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 9600;
        let l = sine(1_000.0, 0.5, n);
        let mut core = core_with(&[(p::CARRIER, 0.0), (p::HZ, 300.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let low = level_at(&out[window.clone()], 700.0);
        let high = level_at(&out[window.clone()], 1_300.0);
        let tone = level_at(&out[window], 1_000.0);
        assert!(low > -20.0, "no difference sideband: {low} dBFS");
        assert!(high > -20.0, "no sum sideband: {high} dBFS");
        assert!(tone < low - 20.0, "the tone survived: {tone} against {low}");
    }

    /// The four carriers all modulate, and the noise one is not a tone.
    #[test]
    fn every_carrier_modulates() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, 0.5, n);
        for carrier in 0..=3 {
            let mut core = core_with(&[
                (p::CARRIER, carrier as f32),
                (p::HZ, 400.0),
                (p::MIX, 100.0),
            ]);
            let out = run(&mut core, &l);
            let changed = rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>());
            assert!(changed > 0.05, "carrier {carrier} did nothing");
            assert!(
                out.iter().all(|s| s.abs() <= 1.0),
                "carrier {carrier} ran hot"
            );
        }
        // Noise has no sideband to find: its energy is spread.
        let mut noisy = core_with(&[(p::CARRIER, 3.0), (p::HZ, 400.0), (p::MIX, 100.0)]);
        let out = run(&mut noisy, &l);
        let window = n / 2..n / 2 + 4800;
        let sideband = level_at(&out[window], 600.0);
        assert!(
            sideband < -20.0,
            "the noise carrier made a tone: {sideband} dBFS"
        );
    }

    /// The hold walks the carrier, so the sidebands move.
    #[test]
    fn the_hold_walks_the_carrier() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, 0.5, n);
        let mut still = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0)]);
        let a = run(&mut still, &l);
        let mut walking = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0), (p::HOLD_RATE, 8.0)]);
        let b = run(&mut walking, &l);
        let sideband_of = |out: &[f32]| level_at(&out[n / 2..n / 2 + 9600], 700.0);
        assert!(
            sideband_of(&a) > -20.0,
            "the still carrier lost its sideband"
        );
        assert!(
            sideband_of(&b) < sideband_of(&a) - 6.0,
            "the walk left the sideband put: {} against {}",
            sideband_of(&b),
            sideband_of(&a)
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::CARRIER, 1.0), (p::HZ, 500.0), (p::MIX, 80.0)];
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
        core.set_param(p::CARRIER, 9.0);
        assert_eq!(core.settings().carrier, p::NOISE);
        core.set_param(p::HZ, 99_999.0);
        assert_eq!(core.settings().hz, 5_000.0);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }
}
