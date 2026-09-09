//! Sixteen-lane synthesis primitives. Caller-owned state, no allocation.
//! Envelopes and filters: O(lanes) per frame; oscillator: one table lookup;
//! modes: six complex rotations per lane. Latency zero. Finite tails are
//! explicitly flushed below 1e-20. All process APIs accept empty slices.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub const N: usize = 16;
pub type Frame = [f32; N];
pub const MODES: usize = 6;
fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

#[derive(Clone)]
pub struct Envelopes {
    value: Frame,
    attack: Frame,
    decay: Frame,
    sustain: Frame,
    release: Frame,
    stage: [u8; N],
}
impl Default for Envelopes {
    fn default() -> Self {
        Self::new()
    }
}
impl Envelopes {
    pub fn new() -> Self {
        Self {
            value: [0.; N],
            attack: [1.; N],
            decay: [0.; N],
            sustain: [0.; N],
            release: [0.; N],
            stage: [0; N],
        }
    }
    pub fn configure(&mut self, lane: usize, sr: f32, a: f32, d: f32, s: f32, r: f32) {
        if lane >= N {
            return;
        }
        let sr = finite(sr, 48000.).max(1.);
        let (a, d, s, r) = (finite(a, 0.), finite(d, 1.), finite(s, 0.), finite(r, 1.));
        self.attack[lane] = 1. / (a.max(0.) * sr * 0.001).max(1.);
        self.decay[lane] = (-6.907755 / (d.max(0.001) * sr * 0.001)).exp();
        self.release[lane] = (-6.907755 / (r.max(0.001) * sr * 0.001)).exp();
        self.sustain[lane] = s.clamp(0., 1.);
    }
    pub fn on(&mut self, lane: usize) {
        if let (Some(v), Some(s)) = (self.value.get_mut(lane), self.stage.get_mut(lane)) {
            *v = 0.;
            *s = 1;
        }
    }
    pub fn off(&mut self, lane: usize) {
        if let Some(s) = self.stage.get_mut(lane) {
            if *s != 0 {
                *s = 3;
            }
        }
    }
    pub fn reset_lane(&mut self, lane: usize) {
        if let (Some(v), Some(s)) = (self.value.get_mut(lane), self.stage.get_mut(lane)) {
            *v = 0.;
            *s = 0;
        }
    }
    pub fn reset(&mut self) {
        self.value.fill(0.);
        self.stage.fill(0);
    }
    pub fn active(&self, lane: usize) -> bool {
        self.stage.get(lane).copied().unwrap_or(0) != 0
    }
    pub fn current(&self, lane: usize) -> f32 {
        self.value.get(lane).copied().unwrap_or(0.)
    }
    pub fn process(&mut self, out: &mut [Frame]) {
        for frame in out {
            for lane in 0..N {
                let v = &mut self.value[lane];
                match self.stage[lane] {
                    1 => {
                        *v = (*v + self.attack[lane]).min(1.);
                        if *v >= 1. {
                            self.stage[lane] = 2;
                        }
                    }
                    2 => {
                        *v = self.sustain[lane] + (*v - self.sustain[lane]) * self.decay[lane];
                        if *v < 1e-6 && self.sustain[lane] == 0. {
                            *v = 0.;
                            self.stage[lane] = 0;
                        }
                    }
                    3 => {
                        *v *= self.release[lane];
                        if *v < 1e-6 {
                            *v = 0.;
                            self.stage[lane] = 0;
                        }
                    }
                    _ => {
                        *v = 0.;
                    }
                }
                frame[lane] = *v;
            }
        }
    }
}

/// Four phase accumulators and one independent deterministic noise stream per lane.
/// State is oscillator-major / lane-major. Table data belongs to the caller.
pub struct Oscillators {
    phase: [[f64; N]; 4],
    rng: [u32; N],
}
impl Default for Oscillators {
    fn default() -> Self {
        Self::new()
    }
}
impl Oscillators {
    pub fn new() -> Self {
        Self {
            phase: [[0.; N]; 4],
            rng: [1; N],
        }
    }
    pub fn reset_lane(&mut self, lane: usize, phase: f32, seed: u32) {
        for op in &mut self.phase {
            if let Some(p) = op.get_mut(lane) {
                *p = finite(phase, 0.).rem_euclid(1.) as f64;
            }
        }
        if let Some(r) = self.rng.get_mut(lane) {
            *r = seed.max(1);
        }
    }
    pub fn phase(&self, op: usize, lane: usize) -> f32 {
        self.phase
            .get(op)
            .and_then(|v| v.get(lane))
            .copied()
            .unwrap_or(0.) as f32
    }
    pub fn advance(&mut self, op: usize, lane: usize, hz: f32, sr: f32) {
        if let Some(p) = self.phase.get_mut(op).and_then(|v| v.get_mut(lane)) {
            let x = *p + (finite(hz, 0.) / finite(sr, 48000.).max(1.)).clamp(-0.49, 0.49) as f64;
            *p = x - x.floor();
        }
    }
    pub fn noise(&mut self, lane: usize) -> f32 {
        let Some(x) = self.rng.get_mut(lane) else {
            return 0.;
        };
        *x ^= *x << 13;
        *x ^= *x >> 17;
        *x ^= *x << 5;
        (*x as f64 / 2147483648. - 1.) as f32
    }
    pub fn sine(&self, op: usize, lane: usize, pm: f32, table: &[f32]) -> f32 {
        read_cycle(self.phase(op, lane) + pm, table)
    }
}

/// Hermite table read with periodic neighbours; normalized phase may include PM.
pub fn read_cycle(phase: f32, table: &[f32]) -> f32 {
    let len = table.len();
    if len < 4 || !phase.is_finite() {
        return 0.;
    }
    let x = (phase - phase.floor()) * len as f32;
    let raw = x.floor() as usize;
    let i = raw % len;
    let f = (x - raw as f32).clamp(0., 1.);
    let get = |k: usize| table.get(k % len).copied().unwrap_or(0.);
    crate::dsp::interp::hermite4(get((i + len - 1) % len), get(i), get(i + 1), get(i + 2), f)
}

/// Modal impulse responses with independent frequency, T60, and strike weight.
/// State and coefficients: mode-major lane arrays. Exact damped complex rotation.
pub struct Modes {
    re: [Frame; MODES],
    im: [Frame; MODES],
    c: [Frame; MODES],
    s: [Frame; MODES],
    weight: [Frame; MODES],
}
impl Default for Modes {
    fn default() -> Self {
        Self::new()
    }
}
impl Modes {
    pub fn new() -> Self {
        Self {
            re: [[0.; N]; MODES],
            im: [[0.; N]; MODES],
            c: [[0.; N]; MODES],
            s: [[0.; N]; MODES],
            weight: [[0.; N]; MODES],
        }
    }
    pub fn configure(
        &mut self,
        lane: usize,
        sr: f32,
        hz: f32,
        ratios: &[f32; MODES],
        times: &[f32; MODES],
        weights: &[f32; MODES],
    ) {
        if lane >= N {
            return;
        }
        for mode in 0..MODES {
            let sr = finite(sr, 48000.).max(4.);
            let hz = finite(hz, 0.).max(0.);
            let f = hz * finite(ratios[mode], 1.).clamp(0., 128.);
            let r = (-6.907755 / (finite(times[mode], 0.001).clamp(0.001, 60.) * sr.max(1.))).exp();
            let angle = core::f32::consts::TAU * f.min(sr * 0.49) / sr.max(1.);
            self.c[mode][lane] = r * angle.cos();
            self.s[mode][lane] = r * angle.sin();
            self.weight[mode][lane] = finite(weights[mode], 0.).clamp(-16., 16.)
                * (1. - ((f / sr - 0.42) / 0.07).clamp(0., 1.));
        }
    }
    pub fn reset_lane(&mut self, lane: usize) {
        for m in 0..MODES {
            if let (Some(r), Some(i)) = (self.re[m].get_mut(lane), self.im[m].get_mut(lane)) {
                *r = 0.;
                *i = 0.;
            }
        }
    }
    pub fn process(&mut self, io: &mut [Frame]) {
        for frame in io {
            for lane in 0..N {
                let excite = frame[lane];
                let mut sum = 0.;
                for mode in 0..MODES {
                    let re = self.re[mode][lane];
                    let im = self.im[mode][lane];
                    let nr = re * self.c[mode][lane] - im * self.s[mode][lane]
                        + excite * self.weight[mode][lane];
                    let ni = im * self.c[mode][lane] + re * self.s[mode][lane];
                    self.re[mode][lane] = if nr.abs() < 1e-20 { 0. } else { nr };
                    self.im[mode][lane] = if ni.abs() < 1e-20 { 0. } else { ni };
                    sum += nr;
                }
                frame[lane] = sum;
            }
        }
    }
}

/// Per-note TPT low-pass; independent cutoff and resonance on every lane.
/// Coefficients use the same topology as filters::LaneSvf, with independent Q.
pub struct Lowpass {
    ic1: Frame,
    ic2: Frame,
    k: Frame,
    a1: Frame,
    a2: Frame,
    a3: Frame,
}
impl Default for Lowpass {
    fn default() -> Self {
        Self::new()
    }
}
impl Lowpass {
    pub fn new() -> Self {
        Self {
            ic1: [0.; N],
            ic2: [0.; N],
            k: [1.414; N],
            a1: [1.; N],
            a2: [0.; N],
            a3: [0.; N],
        }
    }
    pub fn configure(&mut self, lane: usize, sr: f32, hz: f32, q: f32) {
        if lane >= N {
            return;
        }
        let sr = finite(sr, 48000.).max(4.);
        let hz = finite(hz, 1000.);
        let q = finite(q, 0.707);
        let g = (core::f32::consts::PI * hz.clamp(1., sr.max(4.) * 0.49) / sr.max(4.)).tan();
        let k = 1. / q.clamp(0.05, 10000.);
        let a1 = 1. / (1. + g * (g + k));
        self.k[lane] = k;
        self.a1[lane] = a1;
        self.a2[lane] = g * a1;
        self.a3[lane] = g * self.a2[lane];
    }
    pub fn reset_lane(&mut self, lane: usize) {
        if let (Some(a), Some(b)) = (self.ic1.get_mut(lane), self.ic2.get_mut(lane)) {
            *a = 0.;
            *b = 0.;
        }
    }
    pub fn process(&mut self, io: &mut [Frame]) {
        for frame in io {
            for lane in 0..N {
                let v3 = frame[lane] - self.ic2[lane];
                let v1 = self.a1[lane] * self.ic1[lane] + self.a2[lane] * v3;
                let v2 = self.ic2[lane] + self.a2[lane] * self.ic1[lane] + self.a3[lane] * v3;
                let a = 2. * v1 - self.ic1[lane];
                let b = 2. * v2 - self.ic2[lane];
                self.ic1[lane] = if a.abs() < 1e-20 { 0. } else { a };
                self.ic2[lane] = if b.abs() < 1e-20 { 0. } else { b };
                frame[lane] = v2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_reference_and_locks() {
        let mut e = Envelopes::new();
        e.configure(0, 1000., 10., 100., 1., 10.);
        e.configure(1, 1000., 20., 100., 0.2, 10.);
        e.on(0);
        e.on(1);
        let mut x = [[0.; N]; 10];
        e.process(&mut x);
        assert!((x[9][0] - 1.).abs() < 1e-6);
        assert!((x[9][1] - 0.5).abs() < 1e-6);
        e.off(0);
        let mut tail = [[0.; N]; 100];
        e.process(&mut tail);
        assert_eq!(tail[99][0], 0.);
        assert!(tail[99][1] > 0.1);
    }
    #[test]
    fn envelope_split_noalloc_edges() {
        let mut a = Envelopes::new();
        a.configure(0, 48000., 2., 80., 0.5, 80.);
        a.on(0);
        let mut b = a.clone();
        let mut x = [[0.; N]; 257];
        let mut y = x;
        assert_no_alloc::assert_no_alloc(|| {
            a.process(&mut []);
            a.process(&mut x);
            b.process(&mut y[..1]);
            b.process(&mut y[1..100]);
            b.process(&mut y[100..]);
        });
        assert_eq!(x, y);
        assert!(x.iter().flatten().all(|v| v.is_finite()));
    }
    #[test]
    fn oscillator_reference_noalloc_edges() {
        let table: Vec<f32> = (0..2048)
            .map(|i| (core::f32::consts::TAU * i as f32 / 2048.).sin())
            .collect();
        let mut o = Oscillators::new();
        o.reset_lane(0, 0.25, 17);
        assert!((o.sine(0, 0, 0., &table) - 1.).abs() < 1e-6);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..257 {
                o.advance(0, 0, 480., 48000.);
                assert!(o.sine(0, 0, 0., &table).is_finite());
                assert!(o.noise(0).abs() <= 1.);
            }
        });
        assert_eq!(read_cycle(0., &[]), 0.);
        assert_eq!(read_cycle(0., &[1.]), 0.);
    }
    #[test]
    fn oscillator_split_reset_and_invalid_configuration() {
        let table: Vec<f32> = (0..2048)
            .map(|i| (core::f32::consts::TAU * i as f32 / 2048.).sin())
            .collect();
        let mut a = Oscillators::new();
        let mut b = Oscillators::new();
        a.reset_lane(0, 0.17, 37);
        b.reset_lane(0, 0.17, 37);
        let mut whole = [0.; 257];
        let mut split = [0.; 257];
        let run = |o: &mut Oscillators, out: &mut [f32]| {
            for x in out {
                let noise = o.noise(0);
                *x = o.sine(0, 0, noise * 0.1, &table);
                o.advance(0, 0, 127., 48000.);
            }
        };
        run(&mut a, &mut whole);
        run(&mut b, &mut split[..1]);
        run(&mut b, &mut split[1..100]);
        run(&mut b, &mut split[100..]);
        assert_eq!(whole, split);
        b.reset_lane(0, 0.17, 37);
        run(&mut b, &mut split);
        assert_eq!(whole, split);
        let mut env = Envelopes::new();
        env.configure(0, f32::NAN, f32::NAN, f32::NAN, f32::NAN, f32::NAN);
        env.on(0);
        let mut modes = Modes::new();
        modes.configure(
            0,
            f32::NAN,
            f32::NAN,
            &[f32::NAN; 6],
            &[f32::NAN; 6],
            &[f32::NAN; 6],
        );
        let mut filter = Lowpass::new();
        filter.configure(0, f32::NAN, f32::NAN, f32::NAN);
        let mut out = [[0.; N]; 13];
        env.process(&mut out);
        modes.process(&mut out);
        filter.process(&mut out);
        assert!(out.iter().flatten().all(|x| x.is_finite()));
    }
    #[test]
    fn modes_reference_split_noalloc_tails() {
        let make = || {
            let mut m = Modes::new();
            m.configure(
                0,
                48000.,
                1000.,
                &[1.; 6],
                &[0.02; 6],
                &[1., 0., 0., 0., 0., 0.],
            );
            m
        };
        let mut a = make();
        let mut b = make();
        let mut x = [[0.; N]; 257];
        x[0][0] = 1.;
        let mut y = x;
        assert_no_alloc::assert_no_alloc(|| {
            a.process(&mut []);
            a.process(&mut x);
            b.process(&mut y[..1]);
            b.process(&mut y[1..100]);
            b.process(&mut y[100..]);
        });
        assert_eq!(x, y);
        assert!((x[48][0] - (-6.907755 * 0.001 / 0.02_f32).exp()).abs() < 1e-5);
        let mut tail = [[0.; N]; 480];
        for _ in 0..100 {
            tail.fill([0.; N]);
            a.process(&mut tail);
        }
        assert!(
            tail.iter()
                .flatten()
                .all(|x| x.is_finite() && x.abs() < 1e-10)
        );
    }
    #[test]
    fn filter_matches_reference_split_edges() {
        let make = || {
            let mut f = Lowpass::new();
            f.configure(0, 48000., 1200., 0.707);
            f
        };
        let mut a = make();
        let mut b = make();
        let mut scalar = crate::dsp::filters::Svf::new();
        scalar.prepare(48000., 1200., 0.707);
        let mut x = [[0.; N]; 257];
        x[0][0] = 1.;
        let mut y = x;
        let mut reference = [0.; 257];
        reference[0] = 1.;
        scalar.process(&mut reference, crate::dsp::filters::Mode::Lowpass);
        assert_no_alloc::assert_no_alloc(|| {
            a.process(&mut []);
            a.process(&mut x);
            b.process(&mut y[..1]);
            b.process(&mut y[1..100]);
            b.process(&mut y[100..]);
        });
        assert_eq!(x, y);
        for (r, x) in reference.iter().zip(x.iter()) {
            assert_eq!(*r, x[0]);
        }
        let mut tail = [[0.; N]; 257];
        for _ in 0..100 {
            tail.fill([0.; N]);
            a.process(&mut tail);
        }
        assert!(tail.iter().flatten().all(|x| x.is_finite()));
    }
}
