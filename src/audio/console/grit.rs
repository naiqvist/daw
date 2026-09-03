//! GRIT: the lo-fi section — a sampler's converter path, degraded on
//! purpose, with a way to clean up after it if you want to.
//!
//! RATE is the converter clock: `dsp::lofi::Downsampler` runs a
//! tracking anti-alias filter in front of a zero-order hold, the way an
//! S950 did, so what reaches the converter is band-limited and the
//! images on the way back out are the character. BITS is the word
//! length, continuous. JITTER wobbles the clock from block to block
//! with noise, the way a cheap crystal did. HISS is pink noise under
//! everything. POST is a low-pass after the converter — off at the top
//! — for taking the images off when the grain is wanted but the
//! screech is not. MIX blends against the dry.
//!
//! With the clock at the top, sixteen bits, no jitter, no hiss and
//! the post filter open, the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::Cascade;
use crate::dsp::lofi::Downsampler;
use crate::dsp::noise::{PinkNoise, WhiteNoise};
use crate::params::console::grit as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub rate: f32,
    pub bits: f32,
    pub jitter: f32,
    pub hiss: f32,
    pub post: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Grit.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            rate: clamp(p::RATE),
            bits: clamp(p::BITS),
            jitter: clamp(p::JITTER) / 100.0,
            hiss: clamp(p::HISS) / 100.0,
            post: clamp(p::POST),
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn post_off(&self) -> bool {
        self.post >= p::POST_OFF_HZ
    }

    pub fn is_wire(&self) -> bool {
        (self.mix == 0.0
            || (self.rate >= p::RATE_OFF_HZ && self.bits >= 16.0 && self.jitter == 0.0))
            && self.hiss == 0.0
            && self.post_off()
    }
}

pub struct GritCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    converter: [Downsampler; 2],
    post: [Cascade; 2],
    hiss: PinkNoise,
    wobble: WhiteNoise,
    /// Compile-owned scratch: the dry copy, and a block of noise.
    dry: Vec<f32>,
    noise: Vec<f32>,
    hiss_gain: f32,
    level_db: f32,
}

impl GritCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            settings: Settings::of(params),
            sample_rate,
            converter: [Downsampler::new(), Downsampler::new()],
            post: [Cascade::new(), Cascade::new()],
            hiss: PinkNoise::new(),
            wobble: WhiteNoise::new(),
            dry: vec![0.0; block.max(1)],
            noise: vec![0.0; block.max(1)],
            hiss_gain: 0.0,
            level_db: -120.0,
        };
        for converter in &mut core.converter {
            converter.prepare(sample_rate);
        }
        core.hiss.seed(0x6772_6974);
        core.wobble.seed(0x6a69_7474);
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        for ch in 0..2 {
            self.converter[ch].set_rate(s.rate);
            self.converter[ch].set_bits(s.bits);
            self.post[ch].prepare(
                self.sample_rate,
                s.post,
                core::f32::consts::FRAC_1_SQRT_2,
                p::POST_ORDER,
                false,
            );
        }
        self.hiss_gain = if s.hiss > 0.0 {
            10f32.powf(p::HISS_DB / 20.0) * s.hiss
        } else {
            0.0
        };
    }

    fn run(&mut self, ch: usize, io: &mut [f32], wobble: f32) {
        let n = io.len();
        let s = self.settings;
        let dry = &mut self.dry[..n];
        dry.copy_from_slice(io);
        // The clock, wobbled for this block: the same wobble on both
        // sides, so the image does not tear.
        if s.jitter > 0.0 {
            let rate = s.rate * (1.0 + p::JITTER_DEPTH * s.jitter * wobble);
            self.converter[ch].set_rate(rate.max(1_000.0));
        }
        self.converter[ch].process(io);
        if self.hiss_gain > 0.0 {
            for (y, h) in io.iter_mut().zip(&self.noise[..n]) {
                *y += h * self.hiss_gain;
            }
        }
        if !s.post_off() {
            self.post[ch].process(io);
        }
        if s.mix < 1.0 {
            for (y, x) in io.iter_mut().zip(dry.iter()) {
                *y = *x + (*y - *x) * s.mix;
            }
        }
    }
}

impl SectionCore for GritCore {
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
            self.converter[ch].reset();
            self.post[ch].reset();
        }
        self.hiss.reset();
        self.wobble.reset();
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.dry.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if !s.is_wire() {
            if self.hiss_gain > 0.0 {
                self.hiss.process(&mut self.noise[..n]);
            }
            let wobble = if s.jitter > 0.0 {
                let mut one = [0.0f32; 1];
                self.wobble.process(&mut one);
                one[0]
            } else {
                0.0
            };
            self.run(0, l, wobble);
            if stereo {
                self.run(1, &mut r[..n], wobble);
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
                self.converter[0].rate(),
                self.settings.bits,
                self.settings.post,
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

    fn core_with(edits: &[(u32, f32)]) -> GritCore {
        let mut params = SectionParams::of(SectionKind::Grit);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        GritCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut GritCore, l: &[f32]) -> Vec<f32> {
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

    /// The level at `hz` in `signal`, in dBFS, by a plain DFT bin.
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
    fn the_floor_is_a_wire_to_the_sample() {
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

    /// A slow clock puts an image of the tone above it, and the post
    /// filter takes the image off while leaving the tone.
    #[test]
    fn the_clock_makes_images_and_the_post_filter_takes_them_off() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 4800;
        let l = sine(1_000.0, 0.5, n);
        let mut raw = core_with(&[(p::RATE, 8_000.0)]);
        let out = run(&mut raw, &l);
        let tone = level_at(&out[window.clone()], 1_000.0);
        let image = level_at(&out[window.clone()], 7_000.0);
        assert!(tone > -8.0, "the tone is gone: {tone} dBFS");
        assert!(
            image > tone - 30.0,
            "no image at 7 kHz: {image} against {tone}"
        );

        let mut cleaned = core_with(&[(p::RATE, 8_000.0), (p::POST, 2_500.0)]);
        let out = run(&mut cleaned, &l);
        let tone_clean = level_at(&out[window.clone()], 1_000.0);
        let image_clean = level_at(&out[window], 7_000.0);
        assert!(
            (tone_clean - tone).abs() < 3.0,
            "the post filter took the tone: {tone_clean}"
        );
        assert!(
            image_clean < image - 20.0,
            "the image stayed: {image_clean} from {image}"
        );
    }

    /// Fewer bits is more quantisation noise, and a quiet signal shows
    /// it.
    #[test]
    fn fewer_bits_is_a_higher_floor() {
        let n = FS as usize / 4;
        let l = sine(300.0, 0.1, n);
        let error = |bits: f32| -> f32 {
            let mut core = core_with(&[(p::BITS, bits)]);
            let out = run(&mut core, &l);
            rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>())
        };
        let fine = error(12.0);
        let coarse = error(4.0);
        assert!(coarse > fine * 20.0, "4 bits {coarse}, 12 bits {fine}");
    }

    /// Hiss is a floor under silence; jitter changes the sound from
    /// block to block and stays bounded.
    #[test]
    fn hiss_and_jitter_are_there_when_asked_for() {
        let n = FS as usize / 4;
        let silence = vec![0.0f32; n];
        let mut hissing = core_with(&[(p::HISS, 100.0)]);
        let out = run(&mut hissing, &silence);
        let floor = 20.0 * rms(&out[n / 2..]).log10();
        assert!(
            floor > -45.0 && floor < -20.0,
            "the hiss sits at {floor} dBFS"
        );

        let l = sine(1_000.0, 0.5, n);
        let mut steady = core_with(&[(p::RATE, 8_000.0)]);
        let a = run(&mut steady, &l);
        let mut jittery = core_with(&[(p::RATE, 8_000.0), (p::JITTER, 100.0)]);
        let b = run(&mut jittery, &l);
        let diff = rms(&a[n / 2..]
            .iter()
            .zip(&b[n / 2..])
            .map(|(x, y)| x - y)
            .collect::<Vec<_>>());
        assert!(diff > 0.01, "jitter changed nothing");
        assert!(b.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn mix_blends_against_the_dry() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, 0.5, n);
        let mut full = core_with(&[(p::RATE, 6_000.0), (p::BITS, 6.0)]);
        let a = run(&mut full, &l);
        let mut half = core_with(&[(p::RATE, 6_000.0), (p::BITS, 6.0), (p::MIX, 50.0)]);
        let b = run(&mut half, &l);
        let change = |out: &[f32]| {
            rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(x, y)| x - y)
                .collect::<Vec<_>>())
        };
        let (full_change, half_change) = (change(&a), change(&b));
        assert!(
            (half_change / full_change - 0.5).abs() < 0.1,
            "half is {} of full",
            half_change / full_change
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [
            (p::RATE, 9_000.0),
            (p::BITS, 8.0),
            (p::POST, 4_000.0),
            (p::MIX, 70.0),
        ];
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
        core.set_param(p::RATE, 10.0);
        assert_eq!(core.settings().rate, 1_000.0);
        core.set_param(p::BITS, 99.0);
        assert_eq!(core.settings().bits, 16.0);
        core.set_param(99, 1.0);
        core.set_param(p::RATE, 48_000.0);
        assert!(core.settings().is_wire());
    }
}
