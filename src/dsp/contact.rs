//! Unilateral power-law contact solved against midpoint compliance.
//! State: 24 bytes; at most 40 safeguarded Newton steps/contact/sample.
//! No allocation, no denormal state, stateless/in-place safe; latency: zero.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub stiffness: f64,
    pub exponent: f64,
    pub loss: f64,
}
impl Contact {
    /// penetration = free penetration - compliance * force. Damping uses
    /// the positive compression velocity, so separation never attracts.
    pub fn force(self, free: f64, compliance: f64, previous: f64, dt: f64) -> f64 {
        if compliance <= 0.0 {
            return 0.0;
        }
        let free_end = 2.0 * free - previous;
        if free_end <= 0.0 && previous <= 0.0 {
            return 0.0;
        }
        let law = |x: f64| {
            let delta = x - previous;
            let old = previous.max(0.0);
            let now = x.max(0.0);
            let (g, dg) = if delta.abs() < 1e-7 * (old + now).max(1e-12) {
                let mid = ((x + previous) * 0.5).max(0.0);
                (
                    self.stiffness * mid.powf(self.exponent),
                    0.5 * self.exponent * self.stiffness * mid.powf(self.exponent - 1.0),
                )
            } else {
                let potential = self.stiffness / (self.exponent + 1.0)
                    * (now.powf(self.exponent + 1.0) - old.powf(self.exponent + 1.0));
                let g = potential / delta;
                (
                    g,
                    (self.stiffness * now.powf(self.exponent) * delta - potential)
                        / (delta * delta),
                )
            };
            let velocity = (delta / (2.0 * dt)).max(0.0);
            let damping = 1.0 + self.loss * velocity;
            (
                g * damping,
                dg * damping
                    + if velocity > 0.0 {
                        g * self.loss / (2.0 * dt)
                    } else {
                        0.0
                    },
            )
        };
        let mut hi = free_end;
        let mut lo = free_end - 2.0 * compliance * law(free_end).0;
        let mut x = free_end.min(0.0);
        for _ in 0..40 {
            let (force, slope) = law(x);
            let residual = x + 2.0 * compliance * force - free_end;
            if residual.abs() < 1e-13 {
                break;
            }
            if residual > 0.0 {
                hi = x;
            } else {
                lo = x;
            }
            let candidate = x - residual / (1.0 + 2.0 * compliance * slope);
            x = if candidate > lo && candidate < hi {
                candidate
            } else {
                (lo + hi) * 0.5
            };
        }
        ((free_end - x) / (2.0 * compliance)).max(0.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elastic_collision_conserves_total_mechanical_energy() {
        use crate::dsp::modal::Mode;
        let rate = 48_000.0;
        let h = 0.5 / rate;
        let mass = 0.028;
        let mut head = Mode::prepare(rate, 210.0, 0.0, 0.02);
        let contact = Contact {
            stiffness: 1e12,
            exponent: 1.3,
            loss: 0.0,
        };
        let (mut q, mut v, mut penetration) = (0.0, 3.0, 0.0);
        let energy = 0.5 * mass * v * v;
        for _ in 0..4_800 {
            let (free, response) = head.predict(0.0, 0.0);
            let hm = h * h / mass;
            let force = contact.force(q + h * v - free, hm + response, penetration, h);
            let next = q + 2.0 * h * v - 2.0 * hm * force;
            v = (next - q) / h - v;
            q = next;
            head.finish(force);
            penetration = q - head.q;
            let potential = contact.stiffness / (contact.exponent + 1.0)
                * penetration.max(0.0).powf(contact.exponent + 1.0);
            let total = 0.5 * mass * v * v + head.energy() + potential;
            assert!(
                (total / energy - 1.0).abs() < 1e-6,
                "energy {total} vs {energy}"
            );
        }
    }
    #[test]
    fn analytic_linear_contact_and_no_attraction() {
        let c = Contact {
            stiffness: 1000.0,
            exponent: 1.0,
            loss: 0.0,
        };
        assert_no_alloc::assert_no_alloc(|| {
            assert!((c.force(0.01, 0.002, 0.0, 0.001) - 10.0 / 3.0).abs() < 1e-9);
            assert_eq!(c.force(-0.01, 0.002, 0.0, 0.001), 0.0);
            assert_eq!(c.force(0.0, 0.002, 0.0, 0.001), 0.0);
        });
    }
    #[test]
    fn nonlinear_residual_edges_split_and_tiny_contact() {
        let c = Contact {
            stiffness: 1e12,
            exponent: 1.3,
            loss: 0.3,
        };
        for len in [0, 1, 137] {
            let mut a = vec![0.0; len];
            let mut b = a.clone();
            for (i, f) in a.iter_mut().enumerate() {
                *f = c.force(i as f64 * 1e-5, 1e-6, 0.0, 1.0 / 96000.0);
            }
            for range in [0..len / 2, len / 2..len] {
                for i in range {
                    b[i] = c.force(i as f64 * 1e-5, 1e-6, 0.0, 1.0 / 96000.0);
                }
            }
            assert_eq!(a, b);
        }
        for free in [1e-200, 1e-6, 0.1] {
            let f = c.force(free, 1e-6, 0.0, 1.0 / 96000.0);
            assert!(f.is_finite() && f >= 0.0 && f <= free / 1e-6);
        }
    }
}
