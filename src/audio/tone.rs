//! Tone — the test-signal generator.
//!
//! The thing every studio has on the wall: a known signal to push through
//! a chain when you want to know what the CHAIN is doing rather than what
//! the music is doing.
//!
//! Four band-limited waveforms from [`MipOsc`] and two noises from the
//! noise kernel. The waveforms are the oscillator's mip-mapped tables, so
//! a sweep to the top of the range stays a sine rather than turning into
//! a fizz of aliases — which matters more here than anywhere else in the
//! tree, because a test signal that is not what it claims to be is worse
//! than no test signal.
//!
//! MIX at zero is the EXACT identity: this is a utility, and a utility
//! that coloured the signal on its way past could not be trusted in the
//! chain it is measuring.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::noise::{PinkNoise, WhiteNoise};
use crate::dsp::osc::{MipOsc, Waveform};
use crate::params::tone as p;

/// The block is walked in chunks so the scratch is a fixed size.
const CHUNK: usize = 128;
const TONAL_WAVES: [Waveform; 4] = [
    Waveform::Sine,
    Waveform::Triangle,
    Waveform::Saw,
    Waveform::Square,
];

fn waveform_slot(waveform: Waveform) -> usize {
    match waveform {
        Waveform::Sine => 0,
        Waveform::Triangle => 1,
        Waveform::Saw => 2,
        Waveform::Square => 3,
        _ => 0,
    }
}

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ToneParams {
    pub shape: f32,
    pub freq: f32,
    pub level: f32,
    pub mix: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            shape: d(p::SHAPE),
            freq: d(p::FREQ),
            level: d(p::LEVEL),
            mix: d(p::MIX),
        }
    }
}

impl ToneParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::SHAPE => self.shape = value,
            p::FREQ => self.freq = value,
            p::LEVEL => self.level = value,
            p::MIX => self.mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::SHAPE => Some(self.shape),
            p::FREQ => Some(self.freq),
            p::LEVEL => Some(self.level),
            p::MIX => Some(self.mix),
            _ => None,
        }
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE.iter() {
            if let Some(value) = self.get(def.id) {
                let fixed = if value.is_finite() {
                    value
                } else {
                    def.default
                };
                self.set(def.id, def.clamp(fixed));
            }
        }
    }

    /// Which signal, as an index into [`p::SHAPE_NAMES`].
    pub fn shape_index(&self) -> usize {
        (self.shape.round().max(0.0) as usize).min(p::SHAPE_NAMES.len() - 1)
    }

    /// Whether this shape is noise, and so has no frequency.
    pub fn is_noise(&self) -> bool {
        self.shape_index() >= p::FIRST_NOISE
    }

    /// The oscillator waveform this shape asks for, if it is one.
    fn waveform(&self) -> Option<Waveform> {
        match self.shape_index() {
            0 => Some(Waveform::Sine),
            1 => Some(Waveform::Triangle),
            2 => Some(Waveform::Saw),
            3 => Some(Waveform::Square),
            _ => None,
        }
    }
}

/// One generator.
#[derive(Debug, Clone)]
pub struct ToneCore {
    params: ToneParams,
    sample_rate: f32,
    /// Which already-built waveform the oscillator currently addresses.
    built: Option<Waveform>,
    osc: MipOsc,
    /// Every selectable tonal table. A SHAPE letter only changes the slice
    /// selected from this bank; Fourier synthesis never reaches the callback.
    tables: [Vec<f32>; TONAL_WAVES.len()],
    white: WhiteNoise,
    pink: PinkNoise,
    scratch: Vec<f32>,
}

impl ToneCore {
    pub fn new(sample_rate: f32, params: &ToneParams) -> Self {
        let tables = core::array::from_fn(|index| {
            let waveform = TONAL_WAVES[index];
            let mut data = vec![0.0; crate::dsp::osc::table_len(waveform)];
            crate::dsp::osc::build_tables(waveform, &mut data);
            data
        });
        let mut core = Self {
            params: *params,
            sample_rate: 48_000.0,
            built: None,
            osc: MipOsc::new(),
            tables,
            white: WhiteNoise::new(),
            pink: PinkNoise::new(),
            scratch: vec![0.0; CHUNK],
        };
        core.params.sanitize();
        core.prepare(sample_rate);
        core
    }

    /// Green zone: the tables and the rate.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.built = None;
        self.white.seed(0x7A57_0001);
        self.pink.seed(0x7A57_0002);
        self.select_waveform();
        self.reset();
    }

    /// Red-zone-safe O(1) selection of an already-built table set.
    fn select_waveform(&mut self) {
        let Some(waveform) = self.params.waveform() else {
            self.built = None;
            return;
        };
        if self.built == Some(waveform) {
            return;
        }
        self.osc.prepare(self.sample_rate, waveform);
        self.osc.set_freq(self.params.freq);
        self.built = Some(waveform);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> ToneParams {
        self.params
    }

    pub fn reset(&mut self) {
        self.osc.reset();
        self.white.reset();
        self.pink.reset();
    }

    /// It generates rather than anticipates, so it delays nothing.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: put the signal into the pair, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.select_waveform();
        self.osc.set_freq(self.params.freq);

        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }
        let mix = self.params.mix.clamp(0.0, 1.0);
        let level = self.params.level;
        let noise = self.params.is_noise();
        let pink = self.params.shape_index() == p::FIRST_NOISE + 1;

        let mut done = 0usize;
        while done < n {
            let take = (n - done).min(CHUNK);
            let Some(buf) = self.scratch.get_mut(..take) else {
                return;
            };
            if noise {
                if pink {
                    self.pink.process(buf);
                } else {
                    self.white.process(buf);
                }
            } else {
                let waveform = self.params.waveform().unwrap_or(Waveform::Sine);
                self.osc.process(buf, &self.tables[waveform_slot(waveform)]);
            }

            for i in 0..take {
                let wet = buf.get(i).copied().unwrap_or(0.0) * level;
                for io in [&mut *l, &mut *r] {
                    let at = done + i;
                    let dry = io.get(at).copied().unwrap_or(0.0);
                    let dry = if dry.is_finite() { dry } else { 0.0 };
                    let y = dry + (wet - dry) * mix;
                    if let Some(slot) = io.get_mut(at) {
                        *slot = if y.is_finite() { y } else { 0.0 };
                    }
                }
            }
            done += take;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(edit: impl Fn(&mut ToneParams)) -> ToneCore {
        let mut params = ToneParams::default();
        edit(&mut params);
        ToneCore::new(FS, &params)
    }

    fn amp(x: &[f32], hz: f32) -> f64 {
        let n = x.len();
        if n == 0 {
            return 0.0;
        }
        let w = core::f64::consts::TAU * (hz / FS) as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for v in x {
            let s0 = *v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / n as f64
    }

    fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// A utility that coloured the signal on its way past could not be
    /// trusted in the chain it is measuring. MIX at zero is the identity
    /// to the bit, at every shape and every level.
    #[test]
    fn mix_at_zero_is_the_exact_identity() {
        for shape in 0..p::SHAPE_NAMES.len() {
            let mut c = core(|p| {
                p.shape = shape as f32;
                p.mix = 0.0;
                p.level = 1.0;
            });
            let signal: Vec<f32> = (0..2_048)
                .map(|i| 0.7 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
                .collect();
            let (mut l, mut r) = (signal.clone(), signal.clone());
            c.process(&mut l, &mut r);
            for (i, (got, want)) in l.iter().zip(signal.iter()).enumerate() {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "shape {shape} moved sample {i}"
                );
            }
        }
    }

    /// Each shape is the signal it says it is.
    #[test]
    fn every_shape_produces_what_it_claims() {
        // The four waveforms all put their fundamental where asked.
        for shape in 0..p::FIRST_NOISE {
            let mut c = core(|p| {
                p.shape = shape as f32;
                p.freq = 440.0;
                p.level = 1.0;
            });
            let (mut l, mut r) = (vec![0.0f32; 8_192], vec![0.0f32; 8_192]);
            c.process(&mut l, &mut r);
            let fundamental = amp(&l, 440.0);
            let elsewhere = amp(&l, 611.0);
            assert!(
                fundamental > 0.1 && fundamental > elsewhere * 4.0,
                "{}: 440 read {fundamental:.4}, 611 read {elsewhere:.4}",
                p::SHAPE_NAMES[shape]
            );
            // ...and the two channels carry the same signal.
            assert_eq!(
                l,
                r,
                "{} is not the same on both sides",
                p::SHAPE_NAMES[shape]
            );
        }

        // The two noises have no fundamental to find, and are not silent.
        for shape in p::FIRST_NOISE..p::SHAPE_NAMES.len() {
            let mut c = core(|p| {
                p.shape = shape as f32;
                p.level = 1.0;
            });
            let (mut l, mut r) = (vec![0.0f32; 8_192], vec![0.0f32; 8_192]);
            c.process(&mut l, &mut r);
            let level = rms(&l);
            assert!(level > 0.02, "{} is silent", p::SHAPE_NAMES[shape]);
            let peak = amp(&l, 440.0) as f32;
            assert!(
                peak < level * 2.0,
                "{} has a tone in it at 440: {peak}",
                p::SHAPE_NAMES[shape]
            );
        }
    }

    /// A tone generator that lied about its level would be worse than no
    /// tone generator: LEVEL is the amplitude, not a suggestion.
    #[test]
    fn the_level_knob_is_the_amplitude() {
        for level in [1.0f32, 0.5, 0.125_892_54, 0.001] {
            let mut c = core(|p| {
                p.shape = 0.0;
                p.level = level;
                p.freq = 1_000.0;
            });
            let (mut l, mut r) = (vec![0.0f32; 8_192], vec![0.0f32; 8_192]);
            c.process(&mut l, &mut r);
            let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(
                (peak - level).abs() < level * 0.05 + 1e-4,
                "level {level} produced a peak of {peak}"
            );
        }
    }

    /// A shape change must not allocate on the audio thread — every tonal
    /// table is prepared before streaming and the callback only selects one.
    #[test]
    fn changing_shape_mid_stream_does_not_allocate() {
        let mut c = core(|_| {});
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        c.process(&mut l, &mut r);
        assert_no_alloc::assert_no_alloc(|| {
            for shape in 0..p::SHAPE_NAMES.len() {
                c.set_param(p::SHAPE, shape as f32);
                for _ in 0..4 {
                    c.process(&mut l, &mut r);
                }
            }
        });
    }

    #[test]
    fn odd_lengths_and_nonsense_stay_finite() {
        let mut c = core(|_| {});
        for len in [0usize, 1, 3, 17, 129, 700] {
            let (mut l, mut r) = (vec![0.0f32; len], vec![0.0f32; len]);
            c.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        let mut c = core(|p| p.mix = 0.5);
        let mut l = vec![f32::NAN, f32::INFINITY, -1e30, 0.5];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert!(l.iter().chain(r.iter()).all(|s| s.is_finite()));
        // A mismatched pair truncates rather than panicking.
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 8];
        c.process(&mut l, &mut r);
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = ToneParams::default();
        for def in p::TABLE.iter() {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "row {}", def.name);
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), Some(def.min), "row {}", def.name);
            params.set(def.id, def.max * 100.0 + 1.0);
            let got = params.get(def.id).unwrap();
            assert!(got <= def.max && got >= def.min, "row {} = {got}", def.name);
        }
        assert_eq!(params.get(9_999), None);

        let mut junk = ToneParams {
            shape: 99.0,
            freq: f32::NAN,
            level: -4.0,
            mix: f32::INFINITY,
        };
        junk.sanitize();
        for def in p::TABLE.iter() {
            let got = junk.get(def.id).unwrap();
            assert!(
                got.is_finite() && got >= def.min && got <= def.max,
                "row {} survived sanitize as {got}",
                def.name
            );
        }
        assert!(junk.shape_index() < p::SHAPE_NAMES.len());
    }
}
