//! Lane-major physical/additive source bank for MASS, PLUCK, VOX and PIPE.
//!
//! State: fixed SoA arrays, about 30 KiB including a 2049-entry sine table.
//! Cost: at most 32 linearly interpolated sine reads per active lane/sample;
//! bounded coefficient work every 32 samples. No allocation or dependencies.
//! Denormal-safe: envelope/modal tails are explicitly zeroed. Not in-place.
//! Latency: zero. Nonlinear output coloration belongs to node-side FX.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use core::f32::consts::{PI, TAU};
pub const VOICES: usize = 16;
pub const MODES: usize = 32;
pub const PARAMS: usize = 36;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mass,
    Pluck,
    Vox,
    Pipe,
}

#[derive(Clone, Copy)]
pub struct Spectrum {
    pub ratios: [f32; MODES],
    pub gains: [f32; MODES],
    /// Seconds to lose 20 dB, not RT60. Zero means held amplitude.
    pub decays: [f32; MODES],
    pub phases: [f32; MODES],
}

/// Formant anchors are bass synthesis values; SEX is a continuous tract scale.
pub fn formants(vowel: f32, sex: f32) -> [f32; 3] {
    const F: [[f32; 3]; 5] = [
        [600., 1040., 2250.],
        [400., 1620., 2400.],
        [250., 1750., 2600.],
        [400., 750., 2400.],
        [350., 600., 2400.],
    ];
    let v = vowel.clamp(0., 4.);
    let i = (v as usize).min(3);
    let t = v - i as f32;
    core::array::from_fn(|n| {
        let a = F.get(i).and_then(|x| x.get(n)).copied().unwrap_or(600.);
        let b = F.get(i + 1).and_then(|x| x.get(n)).copied().unwrap_or(a);
        a * (b / a).powf(t) * 2.0_f32.powf((sex.clamp(0., 1.) - 0.35) * 1.2)
    })
}
/// Registration anchors are creative starting points (@tune), not presets
/// overwriting drawbar cells. Drawbars remain signed residuals at every point.
pub fn registration(register: f32, bars: [f32; 9]) -> [f32; 9] {
    const SETS: [[f32; 9]; 5] = [
        [8., 0., 3., 0., 0., 0., 0., 0., 0.],
        [4., 0., 8., 4., 0., 0., 0., 0., 0.],
        [6., 0., 8., 8., 6., 0., 0., 0., 0.],
        [0., 0., 4., 8., 8., 8., 6., 3., 0.],
        [0., 0., 0., 2., 4., 6., 8., 8., 8.],
    ];
    let p = register.clamp(0., 4.);
    let i = (p as usize).min(3);
    let t = p - i as f32;
    core::array::from_fn(|n| {
        let a = SETS.get(i).and_then(|x| x.get(n)).copied().unwrap_or(0.);
        let b = SETS.get(i + 1).and_then(|x| x.get(n)).copied().unwrap_or(a);
        ((a + (b - a) * t + bars.get(n).copied().unwrap_or(0.)).clamp(0., 8.) / 8.).powi(2)
    })
}

/// One arithmetic for audio, spectrum pictures and measured source claims.
pub fn spectrum(kind: Kind, p: &[f32; PARAMS], hz: f32, velocity: f32) -> Spectrum {
    let mut s = Spectrum {
        ratios: [1.; MODES],
        gains: [0.; MODES],
        decays: [0.; MODES],
        phases: [0.; MODES],
    };
    let v = velocity.clamp(0., 1.);
    match kind {
        Kind::Mass => {
            let layer = p[1].round() as u32;
            let blend = if layer == 0 { 0. } else { p[3] };
            s.gains[0] = 1. - blend;
            s.ratios[0] = 1.;
            for m in 1..17 {
                let n = m as f32;
                let wave = match layer {
                    1 => {
                        if m % 2 == 1 {
                            1. / n
                        } else {
                            0.
                        }
                    }
                    2 => 1. / n,
                    3 => {
                        if m % 2 == 1 {
                            if m % 4 == 1 {
                                1. / (n * n)
                            } else {
                                -1. / (n * n)
                            }
                        } else {
                            0.
                        }
                    }
                    _ => 0.,
                };
                s.ratios[m] = n * 2.0_f32.powf(p[2].round());
                s.gains[m] = blend * wave;
                s.phases[m] = p[4] * n;
            }
            // The explicit X walker perturbs only an upper shoulder; static
            // harmonics and phase would not create any inharmonicity.
            for m in 17..24 {
                let n = (m - 15) as f32;
                s.ratios[m] = n + p[29] * 0.47 * (n * 0.73).sin();
                s.gains[m] = p[29] * 0.65 / n;
            }
        }
        Kind::Pluck => {
            let b = 0.00002 + (0.065 * p[6].powi(3));
            for m in 0..30 {
                let n = (m / 2 + 1) as f32;
                let side = if m % 2 == 0 { -1. } else { 1. };
                s.ratios[m] =
                    n * ((1. + b * n * n) / (1. + b)).sqrt() * 2.0_f32.powf(side * p[1] / 2400.);
                let pick = (PI * n * p[4]).sin();
                let slope = 2.5 - 1.55 * p[3] - 0.5 * p[8] * v - 0.25 * p[5];
                // Pickup asymmetry adds even harmonics in the resolved modal
                // spectrum, avoiding an unbounded-bandwidth waveshaper here.
                let pickup = if (m / 2 + 1) % 2 == 0 {
                    p[9] * v * v * p[10] * 0.55 / n
                } else {
                    0.
                };
                s.gains[m] = pick / n.powf(slope.max(0.35)) * 0.55 + pickup;
                s.decays[m] =
                    p[2] / (1. + (n - 1.) * (0.015 + (0.22 * (1. - p[3])) + 0.18 * p[14]));
            }
            s.ratios[30] = p[11];
            s.gains[30] = p[10] * p[8] * v * v * 0.35;
            s.decays[30] = p[2] * 0.18;
            s.ratios[31] = 1.004 + p[6] * 0.025;
            s.gains[31] = p[12] * 0.5;
            s.decays[31] = p[13];
        }
        Kind::Vox => {
            let f = formants(p[0], p[1]);
            let width = 0.055 + 0.2 * p[2];
            for m in 0..MODES {
                let n = (m + 1) as f32;
                let freq = hz * n;
                let mouth = f
                    .iter()
                    .enumerate()
                    .map(|(i, &center)| {
                        let distance = (freq / center.max(20.)).ln() / width;
                        (-0.5 * distance * distance).exp() / (1. + i as f32 * 0.55)
                    })
                    .sum::<f32>();
                let pulse = (PI * n * p[4]).sin() / n.powf((1.6 - 0.85 * p[7] * v).max(0.5));
                s.gains[m] = pulse
                    * ((1. - p[14]) * 0.45 + p[14] * (0.025 + mouth * 3.))
                    * (1. - p[3]).sqrt();
                s.ratios[m] = n;
            }
        }
        Kind::Pipe => {
            let bars = registration(
                p[4],
                [p[0], p[1], p[2], p[3], p[8], p[9], p[10], p[11], p[12]],
            );
            let ratios = [0.5, 1.5, 1., 2., 3., 4., 5., 6., 8.];
            for m in 0..9 {
                s.ratios[m] = ratios[m];
                s.gains[m] = bars[m];
            }
            for m in 9..27 {
                let parent = (m - 9) / 2;
                let direction = if m % 2 == 0 { -1. } else { 1. };
                s.ratios[m] = ratios[parent] * 2.0_f32.powf(direction / 12.);
                s.gains[m] = bars[parent] * p[5] * 0.4;
            }
            s.ratios[27] = if p[13] < 1.5 { 2. } else { 3. };
            s.gains[27] = if p[13] < 0.5 { 0. } else { 0.6 };
            s.decays[27] = p[14] * 0.001;
        }
    }
    s
}

/// Sixteen voices represented exclusively by field arrays. No scalar voice
/// objects or callback allocations; per-note parameter snapshots are SoA too.
pub struct Bank {
    kind: Kind,
    sr: f32,
    sine: [f32; 2049],
    params: [[f32; VOICES]; PARAMS],
    pitch: [u8; VOICES],
    velocity: [f32; VOICES],
    held: [bool; VOICES],
    active: [bool; VOICES],
    age: [u64; VOICES],
    elapsed: [u64; VOICES],
    envelope: [f32; VOICES],
    stage: [u8; VOICES],
    attack: [f32; VOICES],
    decay: [f32; VOICES],
    release: [f32; VOICES],
    phase: [[f32; VOICES]; MODES],
    applied_phase: [[f32; VOICES]; MODES],
    step: [[f32; VOICES]; MODES],
    weights: [[f32; VOICES]; MODES],
    modal: [[f32; VOICES]; MODES],
    loss: [[f32; VOICES]; MODES],
    filter: [[f32; VOICES]; 4],
    filter_co: [f32; VOICES],
    noise: [u64; VOICES],
    pink: [f32; VOICES],
    noise_z: [[f32; VOICES]; 3],
    noise_prev: [[f32; VOICES]; 3],
    res_a: [[f32; VOICES]; 3],
    res_b: [[f32; VOICES]; 3],
    target_hz: [f32; VOICES],
    hz: [f32; VOICES],
    last: [f32; VOICES],
    steal: [f32; VOICES],
    fade: [u32; VOICES],
    talk: [f32; VOICES],
    chunk: u32,
}
impl Bank {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            sr: 48000.,
            sine: [0.; 2049],
            params: [[0.; VOICES]; PARAMS],
            pitch: [0; VOICES],
            velocity: [0.; VOICES],
            held: [false; VOICES],
            active: [false; VOICES],
            age: [0; VOICES],
            elapsed: [0; VOICES],
            envelope: [0.; VOICES],
            stage: [0; VOICES],
            attack: [1.; VOICES],
            decay: [0.; VOICES],
            release: [0.; VOICES],
            phase: [[0.; VOICES]; MODES],
            applied_phase: [[0.; VOICES]; MODES],
            step: [[0.; VOICES]; MODES],
            weights: [[0.; VOICES]; MODES],
            modal: [[0.; VOICES]; MODES],
            loss: [[1.; VOICES]; MODES],
            filter: [[0.; VOICES]; 4],
            filter_co: [1.; VOICES],
            noise: [1; VOICES],
            pink: [0.; VOICES],
            noise_z: [[0.; VOICES]; 3],
            noise_prev: [[0.; VOICES]; 3],
            res_a: [[0.; VOICES]; 3],
            res_b: [[0.; VOICES]; 3],
            target_hz: [440.; VOICES],
            hz: [440.; VOICES],
            last: [0.; VOICES],
            steal: [0.; VOICES],
            fade: [0; VOICES],
            talk: [0.; VOICES],
            chunk: 0,
        }
    }
    pub fn prepare(&mut self, sr: f32) {
        self.sr = if sr.is_finite() {
            sr.clamp(8000., 192000.)
        } else {
            48000.
        };
        for (i, x) in self.sine.iter_mut().enumerate() {
            *x = (TAU * i as f32 / 2048.).sin();
        }
        self.reset();
    }
    pub fn reset(&mut self) {
        self.held.fill(false);
        self.active.fill(false);
        self.envelope.fill(0.);
        self.stage.fill(0);
        self.filter.fill([0.; VOICES]);
        self.noise_z.fill([0.; VOICES]);
        self.noise_prev.fill([0.; VOICES]);
        self.last.fill(0.);
        self.fade.fill(0);
        self.applied_phase.fill([0.0; VOICES]);
        self.chunk = 0;
    }
    pub fn active(&self) -> bool {
        self.active.iter().any(|&v| v)
    }
    pub fn lane_active(&self, lane: usize) -> bool {
        self.active.get(lane).copied().unwrap_or(false)
    }
    pub fn choose(&self, count: usize) -> usize {
        let n = count.clamp(1, VOICES);
        (0..n).find(|&i| !self.active[i]).unwrap_or_else(|| {
            (0..n)
                .min_by_key(|&i| (self.held[i], self.age[i]))
                .unwrap_or(0)
        })
    }
    pub fn held(&self, lane: usize) -> bool {
        self.held.get(lane).copied().unwrap_or(false)
    }
    pub fn set(&mut self, lane: usize, p: &[f32; PARAMS]) {
        if lane >= VOICES {
            return;
        }
        for (dst, &x) in self.params.iter_mut().zip(p) {
            dst[lane] = x;
        }
        self.attack[lane] = 1. / (p[16] * 0.001 * self.sr).max(1.);
        self.decay[lane] = (-4.60517 / (p[17] * 0.001 * self.sr).max(1.)).exp();
        let damping = if self.kind == Kind::Pluck {
            1.0 - 0.95 * p[15]
        } else {
            1.0
        };
        self.release[lane] = (-6.907755 / (p[19] * damping * 0.001 * self.sr).max(1.)).exp();
        let tune = if self.kind == Kind::Vox || self.kind == Kind::Pipe {
            p[6]
        } else {
            p[0]
        };
        let stretch = if self.kind == Kind::Pluck {
            p[7] * (self.pitch[lane] as f32 - 60.).max(0.) / 24.
        } else {
            0.
        };
        self.target_hz[lane] = (440.
            * 2.0_f32.powf((self.pitch[lane] as f32 - 69. + tune + stretch) / 12.))
        .clamp(8., self.sr * 0.4);
        if self.kind != Kind::Mass || p[7] <= 0. {
            self.hz[lane] = self.target_hz[lane];
        }
        self.configure(lane);
    }
    pub fn trigger(
        &mut self,
        lane: usize,
        pitch: u8,
        velocity: f32,
        age: u64,
        p: &[f32; PARAMS],
        legato: bool,
    ) {
        if lane >= VOICES {
            return;
        }
        let keep = legato && self.held[lane];
        self.steal[lane] = self.last[lane];
        self.fade[lane] = if self.active[lane] && !keep { 64 } else { 0 };
        self.pitch[lane] = pitch;
        self.velocity[lane] = velocity.clamp(0., 1.);
        self.age[lane] = age;
        self.held[lane] = true;
        self.active[lane] = true;
        if !keep {
            self.elapsed[lane] = 0;
            self.envelope[lane] = 0.;
            self.stage[lane] = 1;
            self.talk[lane] = 0.;
            self.noise[lane] = 0x1A2B3C4D_u64
                .wrapping_add(lane as u64 * 0x9E3779B9)
                .wrapping_add(age);
            self.pink[lane] = 0.;
            for f in &mut self.filter {
                f[lane] = 0.;
            }
            for f in &mut self.noise_z {
                f[lane] = 0.;
            }
            for f in &mut self.noise_prev {
                f[lane] = 0.;
            }
        }
        self.set(lane, p);
        if !keep {
            self.hz[lane] = self.target_hz[lane];
            let sp = spectrum(self.kind, p, self.hz[lane], self.velocity[lane]);
            for m in 0..MODES {
                self.phase[m][lane] = sp.phases[m].rem_euclid(1.);
                self.modal[m][lane] = 1.;
            }
            self.configure(lane);
        }
    }
    pub fn note_off(&mut self, pitch: u8) {
        for i in 0..VOICES {
            if self.pitch[i] == pitch && self.held[i] {
                self.held[i] = false;
                self.stage[i] = 3;
            }
        }
    }
    pub fn release_all(&mut self) {
        for i in 0..VOICES {
            self.held[i] = false;
            if self.active[i] {
                self.stage[i] = 3;
            }
        }
    }
    fn configure(&mut self, lane: usize) {
        let mut p = core::array::from_fn(|i| self.params[i][lane]);
        let t = self.elapsed[lane] as f32 / self.sr;
        if self.kind == Kind::Mass {
            let glide = p[7] * 0.001;
            if glide > 0. {
                self.hz[lane] += (self.target_hz[lane] - self.hz[lane])
                    * (1. - (-32. / (glide * self.sr)).exp());
            }
        }
        let mut hz = self.hz[lane];
        if self.kind == Kind::Mass {
            hz *= 2.0_f32.powf(p[5] * (-t / (p[6] * 0.001).max(0.001)).exp() / 12.);
        }
        if self.kind == Kind::Vox {
            let goal = (self.velocity[lane] * self.envelope[lane] * p[24] + p[29]).clamp(0., 1.);
            let time = if goal > self.talk[lane] { p[26] } else { p[27] };
            self.talk[lane] +=
                (goal - self.talk[lane]) * (1. - (-32. / (time * 0.001 * self.sr).max(1.)).exp());
            p[0] = (p[0] + self.talk[lane] * p[25] * p[28]).clamp(0., 4.);
            let vib = p[5]
                * 0.5
                * (TAU * p[10] * t).sin()
                * (t / (p[11] * 0.001).max(0.001)).clamp(0., 1.);
            hz *= 2.0_f32.powf((p[8] * (-t / (p[9] * 0.001).max(0.001)).exp() + vib) / 12.);
            p[1] = (p[1] + p[12] * (self.pitch[lane] as f32 - 60.) / 72.).clamp(0., 1.);
            let fs = formants(p[0], p[1]);
            for (m, &f) in fs.iter().enumerate() {
                let radius = (-PI * f * (0.055 + 0.2 * p[2]) / self.sr).exp();
                self.res_a[m][lane] = 2. * radius * (TAU * f.min(self.sr * 0.44) / self.sr).cos();
                self.res_b[m][lane] = radius * radius;
            }
        }
        let sp = spectrum(self.kind, &p, hz, self.velocity[lane]);
        for m in 0..MODES {
            let f = hz * sp.ratios[m];
            self.step[m][lane] = (f / self.sr).fract();
            let delta = sp.phases[m] - self.applied_phase[m][lane];
            if delta != 0.0 {
                self.phase[m][lane] = (self.phase[m][lane] + delta).rem_euclid(1.0);
                self.applied_phase[m][lane] = sp.phases[m];
                if self.elapsed[lane] > 0 {
                    self.steal[lane] = self.last[lane];
                    self.fade[lane] = 64;
                }
            }
            self.weights[m][lane] =
                sp.gains[m] * ((self.sr * 0.48 - f) / (self.sr * 0.06)).clamp(0., 1.);
            self.loss[m][lane] = if sp.decays[m] > 0. {
                (-2.302585 / (sp.decays[m] * self.sr).max(1.)).exp()
            } else {
                1.
            };
        }
        if self.kind == Kind::Mass {
            let env = p[10] * (-t / (p[11] * 0.001).max(0.001)).exp();
            let cutoff = (p[8]
                * 2.0_f32.powf(env / 12. + p[12] * (self.pitch[lane] as f32 - 48.) / 12.))
            .max(if p[13] > 0.5 { hz * 4. } else { 20. });
            self.filter_co[lane] = 1. - (-TAU * cutoff.min(self.sr * 0.2) / self.sr).exp();
        } else if self.kind == Kind::Vox {
            self.filter_co[lane] = 1.
                - (-TAU * (6000. * 2.0_f32.powf(p[13] / 12.)).clamp(100., self.sr * 0.4) / self.sr)
                    .exp();
        }
    }
    pub fn process(&mut self, out: &mut [[f32; VOICES]]) {
        for frame in out {
            if self.chunk == 0 {
                for i in 0..VOICES {
                    if self.active[i] && (self.kind == Kind::Mass || self.kind == Kind::Vox) {
                        self.configure(i);
                    }
                }
                self.chunk = 32;
            }
            self.chunk -= 1;
            frame.fill(0.);
            for i in 0..VOICES {
                if !self.active[i] {
                    continue;
                }
                match self.stage[i] {
                    1 => {
                        self.envelope[i] = (self.envelope[i] + self.attack[i]).min(1.);
                        if self.envelope[i] >= 1. {
                            self.stage[i] = 2;
                        }
                    }
                    2 => {
                        let s = self.params[18][i];
                        self.envelope[i] = s + (self.envelope[i] - s) * self.decay[i];
                        if s == 0.0 && self.envelope[i] < 1e-6 {
                            self.envelope[i] = 0.0;
                            self.active[i] = false;
                        }
                    }
                    3 => {
                        self.envelope[i] *= self.release[i];
                        if self.envelope[i] < 1e-6 {
                            self.envelope[i] = 0.;
                            self.active[i] = false;
                        }
                    }
                    _ => {}
                }
                let mut y = 0.;
                for m in 0..MODES {
                    let w = self.weights[m][i];
                    if w.abs() >= 1e-8 {
                        let pos = self.phase[m][i] * 2048.;
                        let index = (pos as usize).min(2047);
                        let a = self.sine.get(index).copied().unwrap_or(0.);
                        let b = self.sine.get(index + 1).copied().unwrap_or(a);
                        let wave = a + (b - a) * (pos - index as f32);
                        y += wave * w * self.modal[m][i];
                    }
                    let phase = self.phase[m][i] + self.step[m][i];
                    self.phase[m][i] = if phase >= 1. { phase - 1. } else { phase };
                    self.modal[m][i] *= self.loss[m][i];
                    if self.modal[m][i] < 1e-12 {
                        self.modal[m][i] = 0.;
                    }
                }
                let mut rng = self.noise[i];
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                self.noise[i] = rng;
                let white = (rng >> 40) as f32 / 8388608. - 1.;
                self.pink[i] = 0.96 * self.pink[i] + 0.04 * white;
                if self.kind == Kind::Vox {
                    let mut breath = 0.;
                    for m in 0..3 {
                        let a = self.res_a[m][i];
                        let b = self.res_b[m][i];
                        let z = (1. - b) * white * 0.2 + a * self.noise_z[m][i]
                            - b * self.noise_prev[m][i];
                        self.noise_prev[m][i] = self.noise_z[m][i];
                        self.noise_z[m][i] = z;
                        breath += z;
                    }
                    y += self.params[3][i].sqrt() * (breath + self.params[15][i] * white * 0.2);
                    self.filter[0][i] += self.filter_co[i] * (y - self.filter[0][i]);
                    y = self.filter[0][i];
                }
                if self.kind == Kind::Pipe {
                    let click = (-(self.elapsed[i] as f32) / (self.sr * 0.003)).exp();
                    y += white * click * self.params[7][i] * 0.7;
                }
                if self.kind == Kind::Mass {
                    let dry = y;
                    let feedback = self.params[9][i] * 0.75;
                    let mut x = y - feedback * self.filter[3][i];
                    for pole in &mut self.filter {
                        pole[i] += self.filter_co[i] * (x - pole[i]);
                        x = pole[i];
                    }
                    y = x + dry * self.params[14][i] * 0.25;
                }
                let vel = 1. - self.params[20][i] + self.params[20][i] * self.velocity[i];
                y *= self.envelope[i] * vel * self.params[21][i] * 0.32;
                if self.fade[i] > 0 {
                    let blend = self.fade[i] as f32 / 64.;
                    y = y * (1. - blend) + self.steal[i] * blend;
                    self.fade[i] -= 1;
                }
                self.elapsed[i] = self.elapsed[i].saturating_add(1);
                self.last[i] = y;
                frame[i] = y;
            }
        }
    }
    pub fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> [f32; PARAMS] {
        let mut p = [0.; PARAMS];
        p[16] = 0.;
        p[17] = 100.;
        p[18] = 1.;
        p[19] = 10.;
        p[20] = 1.;
        p[21] = 1.;
        p[2] = 2.;
        p[3] = 0.7;
        p[4] = 0.5;
        p[8] = 0.5;
        p[13] = 2.;
        p
    }
    #[test]
    fn analytic_oscillator_reference() {
        let mut b = Bank::new(Kind::Mass);
        b.prepare(48000.);
        let mut p = params();
        p[0] = 0.;
        p[1] = 0.;
        p[8] = 16000.;
        p[13] = 0.;
        p[14] = 0.;
        b.trigger(0, 69, 1., 0, &p, false);
        let mut x = [[0.; VOICES]; 600];
        b.process(&mut x);
        assert!(x.iter().all(|v| v.iter().all(|v| v.is_finite())));
        assert!(x.iter().any(|v| v[0].abs() > 0.1));
        assert!(x.iter().all(|v| v[1..].iter().all(|v| *v == 0.)));
    }
    #[test]
    fn split_block_exact() {
        for k in [Kind::Mass, Kind::Pluck, Kind::Vox, Kind::Pipe] {
            let mut a = Bank::new(k);
            let mut b = Bank::new(k);
            a.prepare(48000.);
            b.prepare(48000.);
            let p = params();
            a.trigger(0, 60, 0.8, 0, &p, false);
            b.trigger(0, 60, 0.8, 0, &p, false);
            let mut x = [[0.; VOICES]; 512];
            let mut y = x;
            a.process(&mut x);
            b.process(&mut y[..100]);
            b.process(&mut y[100..357]);
            b.process(&mut y[357..]);
            assert_eq!(x, y);
        }
    }
    #[test]
    fn callback_no_alloc_and_edges() {
        let mut b = Bank::new(Kind::Pluck);
        b.prepare(48000.);
        let p = params();
        let mut x = [[0.; VOICES]; 137];
        assert_no_alloc::assert_no_alloc(|| {
            b.trigger(0, 60, 1., 0, &p, false);
            b.process(&mut []);
            b.process(&mut x[..1]);
            b.process(&mut x);
            b.note_off(60);
            b.process(&mut x);
        });
    }
    #[test]
    fn silence_and_finite_tail() {
        let mut b = Bank::new(Kind::Vox);
        b.prepare(48000.);
        let mut x = [[0.; VOICES]; 137];
        b.process(&mut x);
        assert!(x.iter().flatten().all(|v| *v == 0.));
        b.trigger(0, 60, 1., 0, &params(), false);
        b.process(&mut x);
        b.release_all();
        for _ in 0..1000 {
            b.process(&mut x);
            assert!(x.iter().flatten().all(|v| v.is_finite()));
        }
        assert!(!b.active());
        assert!(x.iter().flatten().all(|v| *v == 0.));
    }
    #[test]
    fn stiff_pins_fundamental_and_sharpens_upper_modes() {
        let mut p = params();
        p[1] = 0.;
        p[6] = 0.;
        let a = spectrum(Kind::Pluck, &p, 220., 1.);
        p[6] = 1.;
        let b = spectrum(Kind::Pluck, &p, 220., 1.);
        assert_eq!(a.ratios[0], 1.);
        assert_eq!(b.ratios[0], 1.);
        assert!(b.ratios[14] > 8.5);
        assert!(a.gains[2].abs() < 1e-6);
    }
    #[test]
    fn sex_moves_formants_and_register_moves_centroid() {
        let a = formants(2., 0.);
        let b = formants(2., 1.);
        assert!(a.iter().zip(b).all(|(a, b)| b > *a));
        let lo = registration(0., [0.; 9]);
        let hi = registration(4., [0.; 9]);
        let ratios = [0.5, 1.5, 1., 2., 3., 4., 5., 6., 8.];
        let centroid = |w: [f32; 9]| {
            w.iter().zip(ratios).map(|(w, r)| w * r).sum::<f32>() / w.iter().sum::<f32>()
        };
        assert!(centroid(hi) > 4. * centroid(lo));
    }

    #[test]
    fn isolated_eight_foot_matches_analytic_sine() {
        let mut p = params();
        p[0] = -8.0;
        p[1] = -8.0;
        p[2] = 0.0;
        p[3] = -8.0;
        p[4] = 1.0;
        for x in &mut p[8..13] {
            *x = -8.0;
        }
        p[5] = 0.0;
        p[6] = 0.0;
        p[7] = 0.0;
        p[13] = 0.0;
        let mut b = Bank::new(Kind::Pipe);
        b.prepare(48000.0);
        b.trigger(0, 69, 1.0, 1, &p, false);
        let mut x = [[0.0; VOICES]; 1000];
        b.process(&mut x);
        for (i, frame) in x.iter().enumerate() {
            let expected = 0.32 * (TAU * i as f32 * 440.0 / 48000.0).sin();
            assert!(
                (frame[0] - expected).abs() < 0.00003,
                "sample {i}: {} != {expected}",
                frame[0]
            );
        }
    }
    #[test]
    fn drop_returns_to_pitch_and_release_closes_the_voice() {
        let mut p = params();
        p[0] = 0.0;
        p[1] = 0.0;
        p[5] = 24.0;
        p[6] = 20.0;
        p[7] = 0.0;
        p[8] = 16000.0;
        let mut b = Bank::new(Kind::Mass);
        b.prepare(48000.0);
        b.trigger(0, 45, 1.0, 1, &p, false);
        assert!((b.step[0][0] * 48000.0 - 440.0).abs() < 0.001);
        let mut x = [[0.0; VOICES]; 137];
        for _ in 0..180 {
            b.process(&mut x);
        }
        assert!((b.step[0][0] * 48000.0 - 110.0).abs() < 0.001);
        b.note_off(45);
        for _ in 0..100 {
            b.process(&mut x);
        }
        assert!(!b.active());
    }
    #[test]
    fn physical_decay_is_t20_and_percussion_leaves_the_drawbars_held() {
        let mut p = params();
        p[0] = 0.0;
        p[1] = 0.0;
        p[2] = 0.1;
        p[3] = 1.0;
        p[4] = 0.2;
        p[8] = 0.0;
        p[9] = 0.0;
        p[12] = 0.0;
        let mut b = Bank::new(Kind::Pluck);
        b.prepare(48000.0);
        b.trigger(0, 57, 1.0, 1, &p, false);
        let mut x = [[0.0; VOICES]; 32];
        for _ in 0..150 {
            b.process(&mut x);
        }
        assert!((b.modal[0][0] - 0.1).abs() < 0.001);
        p[4] = 1.0;
        p[13] = 1.0;
        p[14] = 100.0;
        let mut organ = Bank::new(Kind::Pipe);
        organ.prepare(48000.0);
        organ.trigger(0, 57, 1.0, 1, &p, false);
        for _ in 0..150 {
            organ.process(&mut x);
        }
        assert!((organ.modal[27][0] - 0.1).abs() < 0.001);
        assert_eq!(organ.modal[2][0], 1.0);
    }
    #[test]
    fn hammer_and_pickup_change_velocity_normalized_upper_energy() {
        let mut p = params();
        p[3] = 0.65;
        p[4] = 0.22;
        p[8] = 1.0;
        p[9] = 1.0;
        p[10] = 1.0;
        p[11] = 7.0;
        p[12] = 0.0;
        let high = spectrum(Kind::Pluck, &p, 220.0, 1.0);
        let low = spectrum(Kind::Pluck, &p, 220.0, 0.3);
        let energy = |s: Spectrum| {
            s.gains
                .iter()
                .zip(s.ratios)
                .filter(|(_, r)| *r >= 4.0)
                .map(|(g, _)| g * g)
                .sum::<f32>()
        };
        assert!(energy(high) > 2.0 * energy(low));
    }
    #[test]
    fn hidden_modes_keep_their_phase_and_decay_when_pick_moves() {
        let mut p = params();
        p[0] = 0.0;
        p[1] = 0.0;
        p[2] = 0.1;
        p[3] = 0.65;
        p[4] = 0.125;
        p[9] = 0.0;
        p[12] = 0.0;
        let mut b = Bank::new(Kind::Pluck);
        b.prepare(48000.0);
        b.trigger(0, 57, 1.0, 1, &p, false);
        let mut x = [[0.0; VOICES]; 32];
        for _ in 0..150 {
            b.process(&mut x);
        }
        b.process(&mut x[..1]);
        assert!(b.weights[14][0].abs() < 1e-8);
        assert!(b.modal[14][0] < 0.1);
        assert!(b.phase[14][0] > 0.001);
        let decay = b.modal[14][0];
        let phase = b.phase[14][0];
        p[4] = 0.2;
        b.set(0, &p);
        assert!(b.weights[14][0].abs() > 0.01);
        assert_eq!(b.modal[14][0], decay);
        assert_eq!(b.phase[14][0], phase);
    }
    #[test]
    fn live_mass_phase_moves_the_layer_without_retriggering_envelope() {
        let mut p = params();
        p[0] = 0.0;
        p[1] = 1.0;
        p[2] = 0.0;
        p[3] = 0.5;
        p[4] = 0.0;
        p[8] = 16000.0;
        let mut b = Bank::new(Kind::Mass);
        b.prepare(48000.0);
        b.trigger(0, 45, 1.0, 1, &p, false);
        let mut x = [[0.0; VOICES]; 137];
        b.process(&mut x);
        let phase = b.phase[1][0];
        let envelope = b.envelope[0];
        p[4] = 0.25;
        b.set(0, &p);
        assert!((b.phase[1][0] - (phase + 0.25).fract()).abs() < 1e-6);
        assert_eq!(b.envelope[0], envelope);
        assert_eq!(b.fade[0], 64);
    }
}
