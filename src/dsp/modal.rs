//! Bilinear modal oscillator. State: 72 bytes; ~16 scalar operations/sample.
//! Implicit midpoint conserves undamped quadratic energy. Denormals are
//! cleared below 1e-150. Caller owns storage; in-place safe; latency: zero.
#![deny(clippy::unwrap_used, clippy::expect_used)]

#[derive(Clone, Copy, Debug, Default)]
pub struct Mode {
    pub q: f64,
    pub v: f64,
    pub omega2: f64,
    pub alpha: f64,
    pub inv_mass: f64,
    h: f64,
    inv_h: f64,
    free: f64,
    response: f64,
}
impl Mode {
    pub fn prepare(rate: f64, hz: f64, alpha: f64, mass: f64) -> Self {
        Self {
            omega2: (std::f64::consts::TAU * hz).powi(2),
            alpha: alpha.max(0.0),
            inv_mass: 1.0 / mass.max(1e-12),
            h: 0.5 / rate.max(1.0),
            inv_h: 2.0 * rate.max(1.0),
            ..Self::default()
        }
    }
    pub fn reset(&mut self) {
        self.q = 0.0;
        self.v = 0.0;
        self.free = 0.0;
    }
    /// Midpoint displacement without external force and compliance to force.
    #[inline(always)]
    pub fn predict(&mut self, extra_omega2: f64, extra_loss: f64) -> (f64, f64) {
        let w = self.omega2 + extra_omega2.max(0.0);
        let d = 1.0 + 2.0 * self.h * (self.alpha + extra_loss.max(0.0)) + self.h * self.h * w;
        let inv_d = 1.0 / d;
        self.free = self.q + self.h * (self.v - self.h * w * self.q) * inv_d;
        self.response = self.h * self.h * self.inv_mass * inv_d;
        (self.free, self.response)
    }
    #[inline(always)]
    pub fn finish(&mut self, force: f64) -> f64 {
        let mid = self.free + self.response * force;
        let next = 2.0 * mid - self.q;
        self.v = (next - self.q) * self.inv_h - self.v;
        self.q = next;
        if self.q.abs() + self.v.abs() < 1e-150 {
            self.q = 0.0;
            self.v = 0.0;
        }
        next
    }
    pub fn process(&mut self, force: &[f32], out: &mut [f32]) {
        for (f, o) in force.iter().zip(out) {
            self.predict(0.0, 0.0);
            *o = self.finish(f64::from(*f)) as f32;
        }
    }
    pub fn energy(&self) -> f64 {
        (self.v * self.v + self.omega2 * self.q * self.q) / (2.0 * self.inv_mass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn energy_reference_split_alloc_edges_tail() {
        let mut m = Mode::prepare(48_000.0, 213.0, 0.0, 0.02);
        m.q = 0.001;
        let initial = m.energy();
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..48_000 {
                m.predict(0.0, 0.0);
                m.finish(0.0);
            }
        });
        assert!((m.energy() / initial - 1.0).abs() < 1e-6);
        for len in [0, 1, 137, 256] {
            let mut a = Mode::prepare(48_000.0, 213.0, 4.0, 0.02);
            let mut b = a;
            let mut x = [0.0; 256];
            if len > 0 {
                x[0] = 1.0;
            }
            let (mut y, mut z) = ([0.0; 256], [0.0; 256]);
            a.process(&x[..len], &mut y[..len]);
            b.process(&x[..len / 2], &mut z[..len / 2]);
            b.process(&x[len / 2..len], &mut z[len / 2..len]);
            assert_eq!(y, z);
        }
        let mut damp = Mode::prepare(48_000.0, 400.0, 100.0, 0.02);
        damp.q = 1e-140;
        for _ in 0..500_000 {
            damp.predict(0.0, 0.0);
            damp.finish(0.0);
        }
        assert_eq!(damp.q, 0.0);
    }
}
