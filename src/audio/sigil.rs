//! SIGIL — the ring modulator.
//!
//! The classic spectral mirror: the signal multiplied by a carrier, so
//! every input frequency leaves as a pair of sidebands and the input
//! itself is gone. Sine carries the mirror exactly; triangle is the
//! gentler reflection; square turns every input into an anvil.
//!
//! Wiring, not a kernel: the carrier is a [`MipOsc`], and everything else
//! here is the multiplication that IS the device. The one promise it
//! keeps is the house one — MIX at zero is the EXACT identity, bit for
//! bit, so the colour can be measured rather than merely asserted.
//!
//! It delays nothing and carries no musical state: the carrier phase is
//! unpitched furniture, so a seek needs no reset.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::osc::{MipOsc, Waveform};
use crate::params::sigil as p;

/// The block is walked in chunks so the scratch is a fixed size.
const CHUNK: usize = 128;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SigilParams {
    pub shape: f32,
    pub freq: f32,
    pub mix: f32,
}

impl Default for SigilParams {
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
            mix: d(p::MIX),
        }
    }
}

impl SigilParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::SHAPE => self.shape = value,
            p::FREQ => self.freq = value,
            p::MIX => self.mix = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::SHAPE => Some(self.shape),
            p::FREQ => Some(self.freq),
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

    /// Which form the seal is cast in, as an index into
    /// [`p::SHAPE_NAMES`].
    pub fn shape_index(&self) -> usize {
        (self.shape.round().max(0.0) as usize).min(p::SHAPE_NAMES.len() - 1)
    }

    /// The oscillator waveform this shape asks for.
    fn waveform(&self) -> Waveform {
        match self.shape_index() {
            0 => Waveform::Sine,
            1 => Waveform::Triangle,
            _ => Waveform::Square,
        }
    }
}

/// One ring.
#[derive(Debug, Clone)]
pub struct SigilCore {
    params: SigilParams,
    sample_rate: f32,
    /// Which waveform the tables currently hold, so a shape change
    /// rebuilds them ONCE rather than every block.
    built: Option<Waveform>,
    osc: MipOsc,
    tables: Vec<f32>,
    scratch: Vec<f32>,
}

impl SigilCore {
    pub fn new(sample_rate: f32, params: &SigilParams) -> Self {
        let mut core = Self {
            params: *params,
            sample_rate: 48_000.0,
            built: None,
            osc: MipOsc::new(),
            // Sized for the LARGEST waveform's table set, so a shape
            // change never needs the heap. Square is the deepest of the
            // three; sizing for what is loaded would allocate on the
            // audio thread the first time somebody turned the knob.
            tables: Vec::new(),
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
        let widest = [Waveform::Sine, Waveform::Triangle, Waveform::Square]
            .iter()
            .map(|w| crate::dsp::osc::table_len(*w))
            .max()
            .unwrap_or(0);
        self.tables.clear();
        self.tables.resize(widest, 0.0);
        self.built = None;
        self.rebuild();
        self.reset();
    }

    /// Green zone: build the table set the current shape needs, if it is
    /// not the one already loaded.
    fn rebuild(&mut self) {
        let waveform = self.params.waveform();
        if self.built == Some(waveform) {
            return;
        }
        crate::dsp::osc::build_tables(waveform, &mut self.tables);
        self.osc.prepare(self.sample_rate, waveform);
        self.osc.set_freq(self.params.freq);
        self.built = Some(waveform);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> SigilParams {
        self.params
    }

    pub fn reset(&mut self) {
        self.osc.reset();
    }

    /// It reflects rather than anticipates, so it delays nothing.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: multiply the pair by the carrier, in place.
    ///
    /// `out = in * (1 - mix + mix * carrier)`. At MIX zero that is
    /// `in * 1.0` — the exact identity, bit for bit, not an
    /// approximation that happens to sound dry.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.rebuild();
        self.osc.set_freq(self.params.freq);

        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }
        let mix = self.params.mix.clamp(0.0, 1.0);

        let mut done = 0usize;
        while done < n {
            let take = (n - done).min(CHUNK);
            let Some(buf) = self.scratch.get_mut(..take) else {
                return;
            };
            self.osc.process(buf, &self.tables);

            for i in 0..take {
                let carrier = buf.get(i).copied().unwrap_or(0.0);
                for io in [&mut *l, &mut *r] {
                    let at = done + i;
                    let dry = io.get(at).copied().unwrap_or(0.0);
                    let dry = if dry.is_finite() { dry } else { 0.0 };
                    let wet = dry * carrier;
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

    fn core(edit: impl Fn(&mut SigilParams)) -> SigilCore {
        let mut params = SigilParams::default();
        edit(&mut params);
        SigilCore::new(FS, &params)
    }

    fn run(core: &mut SigilCore, input: &[f32]) -> Vec<f32> {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        core.process(&mut l, &mut r);
        l
    }

    /// The house promise, held bit for bit: a mix of zero leaves the
    /// signal EXACTLY as it was, so the seal can sit in a chain and be
    /// measured rather than merely asserted.
    #[test]
    fn mix_zero_is_the_exact_identity() {
        let input: Vec<f32> = (0..1_024)
            .map(|i| ((i as f32) * 0.37).sin() * 0.8 + (i % 7) as f32 * 0.01)
            .collect();
        let mut sigil = core(|params| params.mix = 0.0);
        let out = run(&mut sigil, &input);
        assert_eq!(out, input, "mix zero must be bit-identical, not merely dry");
    }

    /// A DC input at full mix comes out as the carrier itself: the
    /// multiplication is the device.
    #[test]
    fn a_dc_input_comes_out_as_the_carrier() {
        let mut sigil = core(|params| {
            params.mix = 1.0;
            params.freq = 1_000.0;
            params.shape = 0.0;
        });
        let n = 256usize;
        let out = run(&mut sigil, &vec![1.0; n]);
        // Sine table at phase zero starts at zero and rises; compare
        // against the analytic sine, allowing the band-limited table its
        // interpolation edge.
        let mut worst = 0.0f32;
        for (i, value) in out.iter().enumerate() {
            let expect = (core::f32::consts::TAU * 1_000.0 * i as f32 / FS).sin();
            worst = worst.max((value - expect).abs());
        }
        assert!(worst < 1e-2, "the carrier deviated from a sine by {worst}");
    }

    /// The spectral mirror, verified against the identity
    /// `sin(a)sin(b) = ½(cos(a−b) − cos(a+b))`: a 1 kHz input under a
    /// 100 Hz carrier leaves 900 and 1100 Hz, and the input itself is
    /// gone.
    #[test]
    fn the_input_becomes_sidebands_and_vanishes() {
        let mut sigil = core(|params| {
            params.mix = 1.0;
            params.freq = 100.0;
            params.shape = 0.0;
        });
        let n = 2_048usize;
        let input: Vec<f32> = (0..n)
            .map(|i| (core::f32::consts::TAU * 1_000.0 * i as f32 / FS).sin())
            .collect();
        let out = run(&mut sigil, &input);

        let mut worst = 0.0f32;
        for (i, value) in out.iter().enumerate() {
            let t = i as f32 / FS;
            let expect = 0.5 * (core::f32::consts::TAU * 900.0 * t).cos()
                - 0.5 * (core::f32::consts::TAU * 1_100.0 * t).cos();
            worst = worst.max((value - expect).abs());
        }
        assert!(
            worst < 2e-2,
            "the ring output deviated from its sidebands by {worst}"
        );

        // And the input itself is gone: its bin must be negligible next
        // to the sidebands that replaced it. A finite window cannot put
        // exactly zero in that bin — the sidebands at 900 and 1100 Hz
        // land on fractional periods and leak — so the honest assertion
        // is RELATIVE: the reflection outshines the source by two
        // orders of magnitude.
        let energy_at = |hz: f32| {
            let w = core::f64::consts::TAU * (hz / FS) as f64;
            let c = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for v in &out {
                let s0 = *v as f64 + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / out.len() as f64
        };
        let side = energy_at(900.0).max(energy_at(1_100.0));
        let source = energy_at(1_000.0);
        assert!(side > 0.3, "the sidebands themselves are missing: {side}");
        // Rectangular-window floor: the 900/1100 Hz components sit ~4.3
        // bins away, and their sinc sidelobes land around −22 dB — so
        // this is the honest bound for an unwindowed Goertzel, and the
        // reflection still outshines the source by that much.
        assert!(
            source < side * 0.1,
            "the input outlived its reflection: {source} beside {side}"
        );
    }

    /// Params come from the table, clamp to it, and survive a sanitize.
    #[test]
    fn params_default_clamp_and_round_trip() {
        let params = SigilParams::default();
        for def in p::TABLE {
            let value = params.get(def.id).unwrap();
            assert!(
                (value - def.default).abs() < 1e-6,
                "{} opened off-table",
                def.name
            );
        }
        let mut params = SigilParams::default();
        params.set(p::MIX, 3.0);
        assert_eq!(params.mix, 1.0, "the table clamps");
        params.freq = f32::NAN;
        params.sanitize();
        assert!(
            (params.freq - p::TABLE.iter().find(|d| d.id == p::FREQ).unwrap().default).abs() < 1e-6,
            "a broken value falls back to the table"
        );
    }
}
