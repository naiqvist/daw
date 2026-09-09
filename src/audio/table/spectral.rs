//! Streaming spectral wiring. Persistent data is bin-major across sixteen lanes.
//! FFT scratch is shared sequentially; all memory is allocated in prepare.
use crate::dsp::coverage_spine::{Frame, N};
use crate::dsp::fft::{PhaseVocoder, RealFft};
pub const SIZE: usize = 1024;
const HOP: usize = 256;
const BINS: usize = SIZE / 2 + 1;
const HISTORY: usize = 40;

#[derive(Clone, Copy)]
pub struct Settings {
    pub hz: f32,
    pub sieve: f32,
    pub width: f32,
    pub shift: f32,
    pub tilt: f32,
    pub blur: f32,
    pub freeze: f32,
    pub rough: f32,
    pub low_ms: f32,
    pub high_ms: f32,
    pub feed: f32,
    pub smear: f32,
    pub halo_decay: f32,
    pub halo_tilt: f32,
    pub halo_damp: f32,
    pub halo_mix: f32,
    pub pitch_ratio: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_resynthesis_preserves_sine_and_noise_power() {
        for noise in [false, true] {
            let mut s = Spectral::new();
            s.prepare(48000.);
            let p = [Settings::default(); N];
            let mut enabled = [false; N];
            enabled[0] = true;
            let mut state = 1739_u32;
            let mut input_power = 0_f64;
            let mut output_power = 0_f64;
            for i in 0..65536 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let mut input = [0.; N];
                input[0] = if noise {
                    state as f32 / u32::MAX as f32 * 2. - 1.
                } else {
                    (::core::f32::consts::TAU * 261.62555 * i as f32 / 48000.).sin()
                };
                let output = s.tick(input, &p, &enabled)[0];
                if (16384 - SIZE..65536 - SIZE).contains(&i) {
                    input_power += f64::from(input[0]).powi(2);
                }
                if i >= 16384 {
                    output_power += f64::from(output).powi(2);
                }
            }
            // Rejecting only DC/Nyquist has negligible broadband power loss.
            // Reassigning unmodified bins to their instantaneous frequencies
            // instead of preserving bin addresses used to fail this by >1 dB.
            let db = 10. * (output_power / input_power).log10();
            assert!(db.abs() < 0.1, "noise={noise}: {db} dB");
        }
    }

    #[test]
    fn freeze_captures_a_nonzero_frame_and_keeps_magnitudes() {
        let mut s = Spectral::new();
        s.prepare(48000.);
        let mut p = [Settings::default(); N];
        p[0].freeze = 1.;
        let mut enabled = [false; N];
        enabled[0] = true;
        for i in 0..4096 {
            let mut input = [0.; N];
            input[0] = (::core::f32::consts::TAU * 375. * i as f32 / 48000.).sin();
            s.tick(input, &p, &enabled);
        }
        let before: Vec<f32> = s.held.iter().map(|f| f[0]).collect();
        let bins_before: Vec<f32> = s.held_bin.iter().map(|f| f[0]).collect();
        assert!(before.iter().sum::<f32>() > 1.);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..8192 {
                s.tick([0.; N], &p, &enabled);
            }
        });
        let after: Vec<f32> = s.held.iter().map(|f| f[0]).collect();
        assert_eq!(before, after);
        assert_eq!(
            bins_before,
            s.held_bin.iter().map(|f| f[0]).collect::<Vec<_>>()
        );
    }
    #[test]
    fn frequency_shift_is_hertz_and_split_independent() {
        let mut s = Spectral::new();
        s.prepare(48000.);
        let mut p = [Settings::default(); N];
        p[0].shift = 100.;
        let mut enabled = [false; N];
        enabled[0] = true;
        let mut out = vec![0.; 32768];
        for (i, x) in out.iter_mut().enumerate() {
            let mut input = [0.; N];
            input[0] = (::core::f32::consts::TAU * 375. * i as f32 / 48000.).sin();
            *x = s.tick(input, &p, &enabled)[0];
        }
        let tail = &out[16384..];
        let crossings = tail.windows(2).filter(|p| p[0] <= 0. && p[1] > 0.).count();
        let hz = crossings as f32 * 48000. / tail.len() as f32;
        assert!((hz - 475.).abs() < 5., "{hz}");
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hz: 220.,
            sieve: 0.,
            width: 1.,
            shift: 0.,
            tilt: 0.,
            blur: 0.,
            freeze: 0.,
            rough: 0.,
            low_ms: 0.,
            high_ms: 0.,
            feed: 0.,
            smear: 0.,
            halo_decay: 2.,
            halo_tilt: 0.,
            halo_damp: 18000.,
            halo_mix: 0.,
            pitch_ratio: 1.,
        }
    }
}

pub struct Spectral {
    sr: f32,
    fft: RealFft,
    pv: PhaseVocoder,
    input: Vec<Frame>,
    output: Vec<Frame>,
    prev: Vec<Frame>,
    acc: Vec<Frame>,
    smooth: Vec<Frame>,
    held: Vec<Frame>,
    held_f: Vec<Frame>,
    held_bin: Vec<Frame>,
    halo: Vec<Frame>,
    halo_f: Vec<Frame>,
    history: Vec<Frame>,
    history_f: Vec<Frame>,
    real: Vec<f32>,
    imag: Vec<f32>,
    scratch: Vec<f32>,
    frame: Vec<f32>,
    mag: Vec<f32>,
    phase: Vec<f32>,
    freq: Vec<f32>,
    prev_work: Vec<f32>,
    acc_work: Vec<f32>,
    mapped: Vec<f32>,
    mapped_f: Vec<f32>,
    window: Vec<f32>,
    pos: usize,
    hop: usize,
    history_pos: usize,
    seen: [usize; N],
}
impl Spectral {
    pub fn new() -> Self {
        Self {
            sr: 48000.,
            fft: RealFft::new(),
            pv: PhaseVocoder::new(),
            input: Vec::new(),
            output: Vec::new(),
            prev: Vec::new(),
            acc: Vec::new(),
            smooth: Vec::new(),
            held: Vec::new(),
            held_f: Vec::new(),
            held_bin: Vec::new(),
            halo: Vec::new(),
            halo_f: Vec::new(),
            history: Vec::new(),
            history_f: Vec::new(),
            real: Vec::new(),
            imag: Vec::new(),
            scratch: Vec::new(),
            frame: Vec::new(),
            mag: Vec::new(),
            phase: Vec::new(),
            freq: Vec::new(),
            prev_work: Vec::new(),
            acc_work: Vec::new(),
            mapped: Vec::new(),
            mapped_f: Vec::new(),
            window: Vec::new(),
            pos: 0,
            hop: 0,
            history_pos: 0,
            seen: [0; N],
        }
    }
    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.fft.prepare(SIZE);
        self.pv.prepare(SIZE, HOP);
        for v in [&mut self.input, &mut self.output] {
            v.resize(SIZE, [0.; N]);
        }
        for v in [
            &mut self.prev,
            &mut self.acc,
            &mut self.smooth,
            &mut self.held,
            &mut self.held_f,
            &mut self.held_bin,
            &mut self.halo,
            &mut self.halo_f,
        ] {
            v.resize(BINS, [0.; N]);
        }
        self.history.resize(HISTORY * BINS, [0.; N]);
        self.history_f.resize(HISTORY * BINS, [0.; N]);
        for v in [
            &mut self.real,
            &mut self.imag,
            &mut self.mag,
            &mut self.phase,
            &mut self.freq,
            &mut self.prev_work,
            &mut self.acc_work,
            &mut self.mapped,
            &mut self.mapped_f,
        ] {
            v.resize(BINS, 0.);
        }
        self.scratch.resize(RealFft::scratch_len(SIZE), 0.);
        self.frame.resize(SIZE, 0.);
        self.window.resize(SIZE, 0.);
        for (i, w) in self.window.iter_mut().enumerate() {
            *w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / SIZE as f32).cos();
        }
        self.reset();
    }
    pub fn reset(&mut self) {
        self.pos = 0;
        self.hop = 0;
        self.history_pos = 0;
        for v in [
            &mut self.input,
            &mut self.output,
            &mut self.prev,
            &mut self.acc,
            &mut self.smooth,
            &mut self.held,
            &mut self.held_f,
            &mut self.held_bin,
            &mut self.halo,
            &mut self.halo_f,
            &mut self.history,
            &mut self.history_f,
        ] {
            v.fill([0.; N]);
        }
        self.seen.fill(0);
    }
    pub fn reset_lane(&mut self, lane: usize) {
        if lane >= N {
            return;
        }
        for v in [
            &mut self.input,
            &mut self.output,
            &mut self.prev,
            &mut self.acc,
            &mut self.smooth,
            &mut self.held,
            &mut self.held_f,
            &mut self.held_bin,
            &mut self.halo,
            &mut self.halo_f,
            &mut self.history,
            &mut self.history_f,
        ] {
            for f in v.iter_mut() {
                f[lane] = 0.;
            }
        }
        self.seen[lane] = 0;
    }
    pub fn tick(&mut self, input: Frame, settings: &[Settings; N], enabled: &[bool; N]) -> Frame {
        if self.input.len() != SIZE {
            return [0.; N];
        }
        let out = self.output.get(self.pos).copied().unwrap_or([0.; N]);
        if let Some(f) = self.output.get_mut(self.pos) {
            *f = [0.; N];
        }
        if let Some(f) = self.input.get_mut(self.pos) {
            *f = input;
        }
        self.pos = (self.pos + 1) % SIZE;
        self.hop += 1;
        if self.hop == HOP {
            self.hop = 0;
            for lane in 0..N {
                if enabled[lane] {
                    self.frame(lane, settings[lane]);
                }
            }
            self.history_pos = (self.history_pos + 1) % HISTORY;
        }
        out
    }
    fn frame(&mut self, lane: usize, p: Settings) {
        for i in 0..SIZE {
            self.frame[i] = self.input[(self.pos + i) % SIZE][lane] * self.window[i];
        }
        self.fft.forward(
            &self.frame,
            &mut self.real,
            &mut self.imag,
            &mut self.scratch,
        );
        crate::dsp::fft::magnitude_phase(&self.real, &self.imag, &mut self.mag, &mut self.phase);
        for k in 0..BINS {
            self.prev_work[k] = self.prev[k][lane];
            self.acc_work[k] = self.acc[k][lane];
        }
        self.pv
            .analyse(&self.phase, &mut self.freq, &mut self.prev_work);
        self.mapped.fill(0.);
        self.mapped_f.fill(0.);
        let bin_hz = self.sr / SIZE as f32;
        let blur = (p.blur.clamp(0., 1.) * 0.995).powf(0.3);
        let freeze = if self.seen[lane] >= 4 {
            p.freeze.clamp(0., 1.)
        } else {
            0.
        };
        self.seen[lane] = self.seen[lane].saturating_add(1);
        for k in 1..BINS - 1 {
            self.prev[k][lane] = self.prev_work[k];
            let f = self.freq[k] * bin_hz;
            let harmonic = (f / p.hz.max(20.)).round().max(1.) * p.hz.max(20.);
            let distance =
                ((f - harmonic).abs() / (p.hz.max(20.) * p.width.max(0.05) * 0.5)).min(1.);
            let keep = 0.5 + 0.5 * (core::f32::consts::PI * distance).cos();
            let mask = 1. - p.sieve.clamp(0., 1.)
                + p.sieve.clamp(0., 1.) * keep / p.width.max(0.05).sqrt();
            let slope =
                (p.tilt * ((k as f32 * bin_hz / 1000.).max(0.03)).log2() / 8.).clamp(-36., 24.);
            let mag = self.mag[k] * mask * 10_f32.powf(slope / 20.);
            let filtered = mag * (1. - blur) + self.smooth[k][lane] * blur;
            self.smooth[k][lane] = filtered;
            let attraction = (harmonic - f) * p.sieve.clamp(0., 1.).powi(3);
            let freq = (f + attraction) * p.pitch_ratio + p.shift;
            // A bin address and its instantaneous frequency are different.
            // Keep k at neutral settings; displace it only by the intentional
            // pitch/Hz/attraction transform. Freeze retains that address too.
            let destination =
                k as f32 * p.pitch_ratio + (attraction * p.pitch_ratio + p.shift) / bin_hz;
            let held = self.held[k][lane] * freeze + filtered * (1. - freeze);
            let held_f = self.held_f[k][lane] * freeze + freq * (1. - freeze);
            let target = self.held_bin[k][lane] * freeze + destination * (1. - freeze);
            self.held[k][lane] = held;
            self.held_f[k][lane] = held_f;
            self.held_bin[k][lane] = target;
            if !(1.0..((BINS - 1) as f32)).contains(&target) {
                continue;
            }
            let index = target.floor() as usize;
            let blend = target - index as f32;
            for (at, w) in [(index, 1. - blend), (index + 1, blend)] {
                if at >= BINS - 1 {
                    continue;
                }
                self.mapped[at] += held * w;
                self.mapped_f[at] += held * w * (held_f / bin_hz);
            }
        }
        for k in 1..BINS - 1 {
            let mag = self.mapped[k];
            let freq = if mag > 1e-12 {
                self.mapped_f[k] / mag
            } else {
                k as f32
            };
            let frames =
                ((p.low_ms + (p.high_ms - p.low_ms) * k as f32 / BINS as f32) * 0.001 * self.sr
                    / HOP as f32)
                    .round()
                    .clamp(0., (HISTORY - 1) as f32) as usize;
            let at = ((self.history_pos + HISTORY - frames) % HISTORY) * BINS + k;
            let write = self.history_pos * BINS + k;
            let delayed = if frames == 0 {
                mag
            } else {
                self.history[at][lane]
            };
            let delayed_f = if frames == 0 {
                freq
            } else {
                self.history_f[at][lane]
            };
            self.history[write][lane] = (mag + delayed * p.feed).max(0.);
            self.history_f[write][lane] = freq;
            let wet = mag * (1. - p.smear) + delayed * p.smear;
            let wet_f = if p.smear > 0.5 && delayed > 1e-9 {
                delayed_f
            } else {
                freq
            };
            let hz = k as f32 * bin_hz;
            let decay = (p.halo_decay
                * 2_f32.powf(p.halo_tilt * (hz / 1000.).max(0.03).log2() / 24.))
            .clamp(0.02, 20.);
            let feedback = (-6.907755 * HOP as f32 / (self.sr * decay)).exp()
                / (1. + (hz / p.halo_damp.max(100.)).powi(4) * 0.02);
            let halo = self.halo[k][lane] * feedback;
            if wet > halo {
                self.halo[k][lane] = wet;
                self.halo_f[k][lane] = wet_f;
            } else {
                self.halo[k][lane] = halo;
            }
            self.mag[k] = (wet + self.halo[k][lane] * p.halo_mix).min(1e6);
            self.freq[k] = if wet < halo * p.halo_mix {
                self.halo_f[k][lane]
            } else {
                wet_f
            };
            self.freq[k] += p.rough * ((k * 17 % 31) as f32 / 31. - 0.5) * 0.7;
        }
        self.mag[0] = 0.;
        self.mag[BINS - 1] = 0.;
        self.freq[0] = 0.;
        self.freq[BINS - 1] = (BINS - 1) as f32;
        self.pv
            .synthesise(&self.freq, &mut self.phase, &mut self.acc_work);
        for k in 0..BINS {
            self.acc[k][lane] = self.acc_work[k];
        }
        crate::dsp::fft::polar_to_cartesian(&self.mag, &self.phase, &mut self.real, &mut self.imag);
        self.fft
            .inverse(&self.real, &self.imag, &mut self.frame, &mut self.scratch);
        for i in 0..SIZE {
            let at = (self.pos + i) % SIZE;
            self.output[at][lane] += self.frame[i] * self.window[i] / 1.5;
        }
    }
}
