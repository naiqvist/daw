//! Two modal membranes, a mallet, lumped cavity and unilateral string contacts.
//! Coordinates are metres, forces newtons. Raw Bessel shapes carry their
//! area norm in modal mass; the Berger sum therefore multiplies that norm.
use super::{KilnEngine, Patch, indices::*};
use crate::dsp::{
    bessel::{self, Zeros},
    contact::Contact,
    modal::Mode,
};
use std::f64::consts::{PI, TAU};
use std::sync::{Arc, Mutex, OnceLock};

const FPS: f64 = 240.0;
/// Microphone velocity to digital full scale.
/// @tune 0.01..2
const MIC_GAIN: f64 = 0.16;
/// Shell radiation relative to the heads.
/// @tune 0..0.3
const SHELL_GAIN: f64 = 0.025;

fn zeros() -> &'static Zeros {
    static Z: OnceLock<Zeros> = OnceLock::new();
    Z.get_or_init(Zeros::prepare)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub order: usize,
    pub root: f64,
    pub angle: f64,
    pub sine: bool,
    pub hz: f64,
    pub alpha: f64,
}
impl Shape {
    pub fn at(&self, r: f64, phi: f64) -> f64 {
        if r >= 1.0 {
            return 0.0;
        }
        let angle = self.order as f64 * (phi - self.angle);
        bessel::j(self.order, self.root * r.max(0.0))
            * if self.sine { angle.sin() } else { angle.cos() }
    }
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Animation {
    pub top: Vec<Shape>,
    pub bottom: Vec<Shape>,
    /// q then v for each mode, frame-major. 240 frames per simulated second.
    pub frames: Vec<Vec<f32>>,
    pub wire_frames: Vec<Vec<f32>>,
    pub mallet: Vec<f32>,
    pub duration: f32,
}
impl Animation {
    pub fn amplitudes(&self, t: f32, bottom: bool) -> Vec<f32> {
        let shapes = if bottom { &self.bottom } else { &self.top };
        let offset = if bottom { self.top.len() * 2 } else { 0 };
        let frame = ((t.max(0.0) as f64 * FPS) as usize).min(self.frames.len().saturating_sub(1));
        let Some(data) = self.frames.get(frame) else {
            return vec![0.; shapes.len()];
        };
        let dt = (f64::from(t.max(0.0)) - frame as f64 / FPS)
            .max(0.0)
            .min(1.0 / FPS);
        shapes
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let q = f64::from(data.get(offset + i * 2).copied().unwrap_or(0.));
                let v = f64::from(data.get(offset + i * 2 + 1).copied().unwrap_or(0.));
                let w = TAU * s.hz;
                ((q * (w * dt).cos() + v / w * (w * dt).sin()) * (-s.alpha * dt).exp()) as f32
            })
            .collect()
    }
    pub fn field(&self, t: f32, rings: usize, spokes: usize, bottom: bool) -> Vec<f32> {
        let shapes = if bottom { &self.bottom } else { &self.top };
        let amplitudes = self.amplitudes(t, bottom);
        Self::mesh_field(shapes, &amplitudes, rings, spokes)
    }
    pub fn mesh_field(
        shapes: &[Shape],
        amplitudes: &[f32],
        rings: usize,
        spokes: usize,
    ) -> Vec<f32> {
        let rings = rings.min(128);
        let spokes = spokes.min(256);
        let mut field = vec![0.; rings * spokes];
        // Factor polar basis: only rings*mode_count Bessel evaluations.
        for (s, q) in shapes.iter().zip(amplitudes) {
            if q.abs() < 1e-12 {
                continue;
            }
            let angular: Vec<f64> = (0..spokes)
                .map(|j| {
                    let a = s.order as f64 * (TAU * j as f64 / spokes as f64 - s.angle);
                    if s.sine { a.sin() } else { a.cos() }
                })
                .collect();
            for (i, ring) in field.chunks_mut(spokes.max(1)).enumerate() {
                let r = i as f64 / rings.saturating_sub(1).max(1) as f64;
                let radial = if i + 1 == rings {
                    0.0
                } else {
                    bessel::j(s.order, s.root * r) * f64::from(*q)
                };
                for (value, a) in ring.iter_mut().zip(&angular) {
                    *value += (radial * a) as f32;
                }
            }
        }
        field
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Render {
    pub samples: Arc<Vec<f32>>,
    pub animation: Arc<Animation>,
    pub rate: u32,
    pub millis: f64,
    pub wire_energy: f64,
    pub shell_energy: f64,
    pub max_tension: f64,
}

#[derive(Default)]
pub struct Membrane {
    last: Mutex<Option<(u64, Arc<Animation>)>>,
}
impl KilnEngine for Membrane {
    fn name(&self) -> &'static str {
        "membrane"
    }
    fn preview(&self, p: &Patch, n: u8, v: u8, r: f32) -> Vec<f32> {
        self.render(p, n, v, r as u32, false, &mut |_| true)
            .map_or_else(Vec::new, |r| r.samples.as_ref().clone())
    }
    fn bake(
        &self,
        p: &Patch,
        n: u8,
        v: u8,
        r: f32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Option<Vec<f32>> {
        self.render(p, n, v, r as u32, true, progress)
            .map(|r| r.samples.as_ref().clone())
    }
    fn field(&self, p: &Patch, t: f32, rings: usize, spokes: usize) -> Vec<f32> {
        self.last
            .lock()
            .ok()
            .and_then(|last| {
                last.as_ref()
                    .filter(|(key, _)| *key == p.key(60, 100, 48_000))
                    .map(|(_, a)| a.field(t, rings, spokes, false))
            })
            .unwrap_or_else(|| vec![0.; rings.min(128) * spokes.min(256)])
    }
}

struct Head {
    modes: Vec<Mode>,
    shapes: Vec<Shape>,
    mid: Vec<f64>,
    response: Vec<f64>,
    force: Vec<f64>,
    strike: Vec<f64>,
    mic: Vec<f64>,
    average: Vec<f64>,
    axis: Vec<usize>,
    norm: Vec<f64>,
    hand: Vec<f64>,
    rim: Vec<f64>,
    radius: f64,
    young: f64,
    tension: f64,
}
impl Head {
    fn new(p: &Patch, offset: usize, rate: f64, count: usize, note: u8) -> Self {
        let r = p.get(offset);
        let density = p.get(offset + 2);
        let tension = p.get(offset + 1) * 2f64.powf((f64::from(note) - 60.0) / 6.0);
        let d = p.get(offset + 3) * 3.5e9 * 0.0002f64.powi(3) / (12.0 * (1.0 - 0.2f64.powi(2)));
        let mut candidates = Vec::new();
        for (n, row) in zeros().0.iter().enumerate() {
            for &root in row {
                for sine in [false, true] {
                    if n == 0 && sine {
                        continue;
                    }
                    let cents = match n {
                        1 => -518.,
                        2 => -336.,
                        3 | 4 => -200.,
                        5 => -50.,
                        _ => 0.,
                    };
                    let loading = 2f64.powf(cents * p.get(offset + 6) / 1200.0);
                    let split = if n == 0 {
                        1.0
                    } else {
                        2f64.powf(p.get(offset + 4) * 16.0 * if sine { -1.0 } else { 1.0 } / 1200.0)
                    };
                    let k = root / r;
                    let hz = (k * k * (tension / density + d / density * k * k)).sqrt() / TAU
                        * loading
                        * split;
                    if hz < rate * 0.43 {
                        candidates.push(Shape {
                            order: n,
                            root,
                            angle: p.get(offset + 5).to_radians(),
                            sine,
                            hz,
                            alpha: 0.0,
                        });
                    }
                }
            }
        }
        candidates.sort_by(|a, b| a.hz.total_cmp(&b.hz));
        candidates.truncate(count);
        let mut head = Self {
            modes: Vec::new(),
            shapes: candidates,
            mid: vec![0.; count],
            response: vec![0.; count],
            force: vec![0.; count],
            strike: Vec::new(),
            mic: Vec::new(),
            average: Vec::new(),
            axis: Vec::new(),
            norm: Vec::new(),
            hand: Vec::new(),
            rim: Vec::new(),
            radius: r,
            young: 3.5e9 * 0.0002 / (4.0 * r * r * 0.96),
            tension,
        };
        let mic_r = (p.get(MIC_MIC_DISTANCE) * p.get(MIC_MIC_ANGLE).to_radians().sin()
            / (r + p.get(MIC_MIC_HEIGHT).abs()))
        .clamp(0.0, 0.98);
        for shape in &mut head.shapes {
            let n = shape.order;
            let norm = bessel::j(n + 1, shape.root).powi(2) * if n == 0 { 1.0 } else { 0.5 };
            let k = shape.root / r;
            let loss = (p.get(offset + 7)
                + p.get(offset + 8)
                    * k
                    * k
                    * (shape.root / 2.4048255577).powf(p.get(offset + 9) - 1.0))
                / (2.0 * density)
                * p.get(MODES_DAMP_1 + n);
            let ring = p.get(MUFFLE_RING_WIDTH);
            let mut ring_weight = 0.;
            for i in 0..5 {
                let x =
                    (p.get(MUFFLE_RING_POSITION) + (i as f64 / 4.0 - 0.5) * ring).clamp(0.0, 1.0);
                ring_weight += bessel::j(n, shape.root * x).powi(2) / 5.0;
            }
            let muffle = ring * 250.0 * ring_weight / norm
                + p.get(MUFFLE_PATCH_STRENGTH)
                    * 60.0
                    * shape.at(p.get(MUFFLE_PATCH_POSITION), 0.0).powi(2)
                    / norm
                + if n > 0 {
                    p.get(MUFFLE_RIM_DAMPING) * 6.0
                } else {
                    0.0
                };
            shape.alpha = loss + muffle;
            head.modes.push(Mode::prepare(
                rate,
                shape.hz,
                shape.alpha,
                PI * r * r * density * norm,
            ));
            head.strike.push(
                shape.at(p.get(STRIKE_POSITION), p.get(STRIKE_ANGLE).to_radians())
                    * p.get(MODES_GAIN_1 + n),
            );
            head.mic
                .push(shape.at(mic_r, p.get(MIC_MIC_ANGLE).to_radians()) * p.get(MODES_GAIN_1 + n));
            head.average.push(if n == 0 {
                2.0 * bessel::j(1, shape.root) / shape.root
            } else {
                0.0
            });
            if n == 0 {
                head.axis.push(head.modes.len() - 1);
            }
            head.norm.push(norm * shape.root * shape.root);
            head.rim.push(shape.root * bessel::j(n + 1, shape.root));
            head.hand.push(
                p.get(MUFFLE_HAND_STRENGTH)
                    * 120.0
                    * shape.at(p.get(MUFFLE_HAND_POSITION), 0.0).powi(2)
                    / norm,
            );
        }
        head
    }
    /// Uniform cavity pressure cannot excite diameter modes. In a wire-free
    /// preview, modes with zero strike AND pressure weights remain exactly
    /// zero, even under the Berger term; omit their arithmetic, not physics.
    fn prune_silent(&mut self, struck: bool) {
        let keep: Vec<_> = (0..self.modes.len())
            .filter(|&i| self.average[i] != 0.0 || (struck && self.strike[i] != 0.0))
            .collect();
        macro_rules! retain {
            ($field:ident) => {
                self.$field = keep.iter().map(|&i| self.$field[i].clone()).collect();
            };
        }
        retain!(modes);
        retain!(shapes);
        retain!(mid);
        retain!(response);
        retain!(force);
        retain!(strike);
        retain!(mic);
        retain!(average);
        retain!(norm);
        retain!(rim);
        retain!(hand);
        self.axis = self
            .average
            .iter()
            .enumerate()
            .filter_map(|(i, w)| (*w != 0.0).then_some(i))
            .collect();
    }
    fn predict(&mut self, drop: f64, hand: bool) -> f64 {
        let added = self.young
            * self
                .modes
                .iter()
                .zip(&self.norm)
                .map(|(m, weight)| m.q * m.q * weight)
                .sum::<f64>();
        // Berger's elastic energy is positive. The integrator solves
        // the positive tension stiffness implicitly.
        let added = added + drop * self.tension;
        let scale = added / self.tension;
        for ((((m, mid), response), force), loss) in self
            .modes
            .iter_mut()
            .zip(&mut self.mid)
            .zip(&mut self.response)
            .zip(&mut self.force)
            .zip(&self.hand)
        {
            (*mid, *response) = m.predict(m.omega2 * scale, if hand { *loss } else { 0.0 });
            *force = 0.0;
        }
        added
    }
    fn add(&mut self, weights: &[f64], force: f64) {
        for (((f, mid), response), w) in self
            .force
            .iter_mut()
            .zip(&mut self.mid)
            .zip(&self.response)
            .zip(weights)
        {
            *f += w * force;
            *mid += response * w * force;
        }
    }
    fn finish(&mut self) -> f64 {
        let mut out = 0.0;
        for ((m, force), mic) in self.modes.iter_mut().zip(&self.force).zip(&self.mic) {
            m.finish(*force);
            out += m.v * mic;
        }
        out
    }
    fn snapshot(&self, out: &mut Vec<f32>) {
        for m in &self.modes {
            out.push(m.q as f32);
            out.push(m.v as f32);
        }
    }
}
#[inline(always)]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
#[inline(always)]
fn compliance(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * a * b).sum()
}

struct Wire {
    modes: [Mode; 3],
    weights: Vec<f64>,
    mid: f64,
    response: f64,
    previous: f64,
    force: f64,
}
struct Simulation {
    top: Head,
    bottom: Head,
    wires: Vec<Wire>,
    shell: Vec<Mode>,
    drop_amount: f64,
    drop_time: f64,
    hand_time: f64,
    flam_level: f64,
    mallet_compliance: f64,
    rest_gap: f64,
    mic_blend: f64,
    rate: f64,
    h: f64,
    mallet_q: f64,
    mallet_v: f64,
    previous: f64,
    strike_velocity: f64,
    contact: Contact,
    wire_contact: Contact,
    step: usize,
    flam_at: usize,
    flammed: bool,
    air_k: f64,
    air_c: f64,
    wire_energy: f64,
    shell_energy: f64,
    max_tension: f64,
    last_wire_velocity: f64,
}
impl Simulation {
    fn new(p: &Patch, n: u8, v: u8, rate: f64, full: bool) -> Self {
        let count = if full {
            p.get(MODES_MODE_COUNT).round() as usize
        } else {
            32
        };
        let mut top = Head::new(p, 0, rate, count, n);
        let mut bottom = Head::new(p, 10, rate, if full { count } else { 16 }, n);
        if !full {
            top.prune_silent(true);
            bottom.prune_silent(false);
        }
        let mut wires = Vec::new();
        if full {
            for i in 0..p.get(WIRES_COUNT).round() as usize {
                let x = (i as f64 - (p.get(WIRES_COUNT) - 1.0) * 0.5) * p.get(WIRES_SPACING)
                    / bottom.radius;
                let r = x.abs().min(0.98);
                let phi = if x < 0.0 { PI } else { 0.0 };
                let length = p.get(WIRES_LENGTH);
                let mass = p.get(WIRES_DENSITY) * length * 0.5;
                let fundamental = (p.get(WIRES_TENSION) * (0.15 + 1.85 * p.get(WIRES_STRAINER))
                    / p.get(WIRES_DENSITY))
                .sqrt()
                    / (2.0 * length)
                    * (1.0 + 0.011 * i as f64);
                let modes = std::array::from_fn(|j| {
                    let order = (j * 2 + 1) as f64;
                    Mode::prepare(
                        rate,
                        fundamental * order * (1.0 + 0.0008 * order * order).sqrt(),
                        8.0 + p.get(WIRES_WIRE_LOSS) * 180.0 + order * 3.0,
                        mass,
                    )
                });
                wires.push(Wire {
                    modes,
                    weights: bottom.shapes.iter().map(|s| s.at(r, phi)).collect(),
                    mid: 0.,
                    response: 0.,
                    previous: -p.get(WIRES_REST_GAP),
                    force: 0.,
                });
            }
        }
        let shell = (0..p.get(SHELL_SHELL_MODES).round() as usize)
            .map(|i| {
                Mode::prepare(
                    rate,
                    (180.0 + 900.0 * p.get(SHELL_SHELL_STIFFNESS)) * (1.0 + i as f64 * 0.73)
                        / p.get(SHELL_SHELL_HEIGHT).sqrt(),
                    15.0 + 180.0 * p.get(SHELL_SHELL_LOSS),
                    0.4,
                )
            })
            .collect();
        let velocity = if v == 0 {
            0.0
        } else {
            0.5 + 9.5 * (f64::from(v) / 127.0).powf(2.0 - 1.5 * p.get(STRIKE_VELOCITY_MAP))
        };
        let air = PI * top.radius.powi(2) * 1.2 * 343f64.powi(2) / p.get(SHELL_SHELL_HEIGHT)
            * p.get(SHELL_HEAD_COUPLING)
            * (1.0 - p.get(SHELL_PORT));
        Self {
            top,
            bottom,
            wires,
            shell,
            drop_amount: p.get(TIME_TENSION_DROP),
            drop_time: p.get(TIME_DROP_TIME) * 0.001,
            hand_time: p.get(MUFFLE_HAND_TIME) * 0.001,
            flam_level: p.get(STRIKE_FLAM_LEVEL),
            mallet_compliance: (0.5 / rate) * (0.5 / rate) / p.get(STRIKE_MALLET_MASS),
            rest_gap: p.get(WIRES_REST_GAP),
            mic_blend: p.get(MIC_TOP_BOTTOM),
            rate,
            h: 0.5 / rate,
            mallet_q: 0.,
            mallet_v: velocity,
            previous: 0.,
            strike_velocity: velocity,
            contact: Contact {
                stiffness: 10f64.powf(p.get(STRIKE_CONTACT_STIFFNESS)),
                exponent: p.get(STRIKE_HARDNESS),
                loss: p.get(STRIKE_CONTACT_LOSS),
            },
            wire_contact: Contact {
                stiffness: 10f64.powf(p.get(WIRES_CONTACT_STIFFNESS)),
                exponent: 1.3,
                loss: p.get(WIRES_CONTACT_DAMPING),
            },
            step: 0,
            flam_at: (p.get(STRIKE_FLAM_DELAY) * 0.001 * rate) as usize,
            flammed: false,
            air_k: air * p.get(SHELL_CAVITY_STIFFNESS),
            air_c: air * 0.0003 * p.get(SHELL_AIR_DAMPING),
            wire_energy: 0.,
            shell_energy: 0.,
            max_tension: 0.,
            last_wire_velocity: 0.,
        }
    }
    fn tick(&mut self) -> f64 {
        let t = self.step as f64 / self.rate;
        let drop = if self.drop_amount == 0.0 {
            0.0
        } else {
            self.drop_amount * (-t / self.drop_time).exp()
        };
        let hand = t >= self.hand_time;
        let added = self.top.predict(drop, hand);
        self.max_tension = self.max_tension.max(added);
        self.bottom.predict(drop * 0.5, hand);
        let average = |head: &Head, mid: bool| {
            head.axis
                .iter()
                .map(|&i| {
                    if mid {
                        head.mid[i] * head.average[i]
                    } else {
                        head.modes[i].q * head.average[i]
                    }
                })
                .sum::<f64>()
        };
        let mobility = |head: &Head| {
            head.axis
                .iter()
                .map(|&i| head.average[i] * head.average[i] * head.response[i])
                .sum::<f64>()
        };
        let old = average(&self.top, false) - average(&self.bottom, false);
        let difference = average(&self.top, true) - average(&self.bottom, true);
        let air_compliance = mobility(&self.top) + mobility(&self.bottom);
        let air = (self.air_k * difference + self.air_c * (difference - old) / self.h)
            / (1.0 + air_compliance * (self.air_k + self.air_c / self.h));
        for &i in &self.top.axis {
            let w = self.top.average[i];
            self.top.force[i] -= w * air;
            self.top.mid[i] -= self.top.response[i] * w * air;
        }
        for &i in &self.bottom.axis {
            let w = self.bottom.average[i];
            self.bottom.force[i] += w * air;
            self.bottom.mid[i] += self.bottom.response[i] * w * air;
        }
        if !self.flammed && self.step >= self.flam_at && self.flam_level > 0.0 {
            self.mallet_q = dot(&self.top.mid, &self.top.strike);
            self.mallet_v = self.strike_velocity * self.flam_level;
            self.flammed = true;
        }
        let free = self.mallet_q + self.h * self.mallet_v - dot(&self.top.mid, &self.top.strike);
        let hm = self.mallet_compliance;
        let force = self.contact.force(
            free,
            hm + compliance(&self.top.strike, &self.top.response),
            self.previous,
            self.h,
        );
        let mallet_mid = self.mallet_q + self.h * self.mallet_v - hm * force;
        let new_q = 2.0 * mallet_mid - self.mallet_q;
        self.mallet_v = (new_q - self.mallet_q) / self.h - self.mallet_v;
        self.mallet_q = new_q;
        for (i, w) in self.top.strike.iter().enumerate() {
            self.top.force[i] += w * force;
            self.top.mid[i] += self.top.response[i] * w * force;
        }
        let mut wire_sound = 0.0;
        for wire in &mut self.wires {
            wire.mid = 0.;
            wire.response = 0.;
            wire.force = 0.;
            for m in &mut wire.modes {
                let (q, r) = m.predict(0.0, 0.0);
                wire.mid += q;
                wire.response += r;
            }
        }
        // Coupled contact solve: every correction sees all other wires'
        // current forces, including their reaction on the head. A single
        // forward pass lets tightly packed wires add numerical energy.
        for _ in 0..12 {
            let mut change = 0.0f64;
            for wire in &mut self.wires {
                self.bottom.add(&wire.weights, wire.force);
                let free = dot(&self.bottom.mid, &wire.weights) - wire.mid - self.rest_gap;
                let mobility = wire.response + compliance(&wire.weights, &self.bottom.response);
                let force = self
                    .wire_contact
                    .force(free, mobility, wire.previous, self.h);
                change = change.max((force - wire.force).abs() * mobility);
                wire.force = force;
                self.bottom.add(&wire.weights, -wire.force);
            }
            if change < 1e-11 {
                break;
            }
        }
        for wire in &mut self.wires {
            for m in &mut wire.modes {
                m.finish(wire.force);
                wire_sound += m.v;
            }
        }
        let top = self.top.finish();
        let bottom = self.bottom.finish();
        self.previous = self.mallet_q
            - self
                .top
                .modes
                .iter()
                .zip(&self.top.strike)
                .map(|(m, w)| m.q * w)
                .sum::<f64>();
        for wire in &mut self.wires {
            wire.previous = self
                .bottom
                .modes
                .iter()
                .zip(&wire.weights)
                .map(|(m, w)| m.q * w)
                .sum::<f64>()
                - wire.modes.iter().map(|m| m.q).sum::<f64>()
                - self.rest_gap;
        }
        self.wire_energy += wire_sound * wire_sound / self.rate;
        let wire_pressure = (wire_sound - self.last_wire_velocity) * self.rate / (TAU * 1500.0);
        self.last_wire_velocity = wire_sound;
        let rim = self
            .top
            .modes
            .iter()
            .zip(&self.top.rim)
            .map(|(m, w)| m.q * w)
            .sum::<f64>();
        let mut shell_sound = 0.;
        for mode in &mut self.shell {
            mode.predict(0.0, 0.0);
            mode.finish(rim * self.top.tension);
            shell_sound += mode.v;
        }
        self.shell_energy += shell_sound * shell_sound / self.rate;
        let blend = self.mic_blend;
        self.step += 1;
        // A bottom microphone reverses the head phase. Wire radiation is
        // structural string acceleration, never an added noise generator.
        (top * blend - bottom * (1.0 - blend) + wire_pressure * 0.65 + shell_sound * SHELL_GAIN)
            * MIC_GAIN
    }
    fn record(&self, a: &mut Animation) {
        let mut frame = Vec::with_capacity((self.top.modes.len() + self.bottom.modes.len()) * 2);
        self.top.snapshot(&mut frame);
        self.bottom.snapshot(&mut frame);
        a.frames.push(frame);
        a.wire_frames.push(
            self.wires
                .iter()
                .map(|w| w.modes.iter().map(|m| m.q).sum::<f64>() as f32)
                .collect(),
        );
        a.mallet.push(self.mallet_q as f32);
    }
}
impl Membrane {
    pub fn render(
        &self,
        p: &Patch,
        note: u8,
        vel: u8,
        rate: u32,
        full: bool,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Option<Render> {
        let start = std::time::Instant::now();
        let rate = rate.clamp(8_000, 192_000);
        let integration = if full {
            p.get(TIME_SAMPLE_RATE) * p.get(TIME_OVERSAMPLING).round()
        } else {
            f64::from(rate)
        };
        let integration = integration.max(f64::from(rate));
        let mut sim = Simulation::new(p, note, vel, integration, full);
        let length = p.get(TIME_LENGTH);
        let frames = (length * f64::from(rate)).round() as usize;
        let internal_frames = (length * integration).round() as usize;
        let mut samples = Vec::with_capacity(internal_frames);
        let mut animation = Animation {
            top: sim.top.shapes.clone(),
            bottom: sim.bottom.shapes.clone(),
            duration: length as f32,
            ..Default::default()
        };
        sim.record(&mut animation);
        let mut next_picture = 1.0 / FPS;
        let mut low = 0.;
        let mut dc = 0.;
        let distance = (p.get(MIC_MIC_DISTANCE).powi(2) + p.get(MIC_MIC_HEIGHT).powi(2)).sqrt();
        let pre =
            ((p.get(TIME_PRE_ROLL) * 0.001 + distance / 343.0) * integration).round() as usize;
        let gain = 0.3 / distance.max(0.03);
        let proximity = p.get(MIC_PROXIMITY) * 2.0;
        let low_coeff = 1.0 - (-TAU * 193.0 / integration).exp();
        let dc_coeff = 1.0 - (-TAU * 7.64 / integration).exp();
        for i in 0..internal_frames {
            if i % 2048 == 0 && !progress(0.9 * i as f32 / internal_frames.max(1) as f32) {
                return None;
            }
            if i < pre {
                samples.push(0.);
                continue;
            }
            let mut value = sim.tick();
            if !value.is_finite() || value.abs() > 1e8 {
                return None;
            }
            if sim.step as f64 / integration >= next_picture {
                sim.record(&mut animation);
                next_picture += 1.0 / FPS;
            }
            low += low_coeff * (value - low);
            value = (value + low * proximity) * gain;
            dc += dc_coeff * (value - dc);
            value -= dc;
            samples.push(value as f32);
        }
        if integration as u32 != rate {
            samples =
                crate::library::resample_interleaved(&samples, 1, integration as u32, rate).ok()?;
            samples.resize(frames, 0.0);
        }
        if !progress(0.95) {
            return None;
        }
        if full && p.get(MIC_ROOM_MIX) > 0.0 {
            let mut reverb = crate::dsp::reverb::Reverb::new();
            let mut scratch = vec![0.; crate::dsp::reverb::Reverb::buffer_len(rate as f32)];
            reverb.prepare(rate as f32, &mut scratch);
            reverb.set_room(p.get(MIC_ROOM_SIZE) as f32, 0.65, 0.4);
            let mut wet = vec![0.; samples.len()];
            for (i, (input, out)) in samples.chunks(512).zip(wet.chunks_mut(512)).enumerate() {
                if !progress(0.97 + 0.025 * i as f32 * 512.0 / frames.max(1) as f32) {
                    return None;
                }
                reverb.process(input, out, &mut scratch);
            }
            let mix = p.get(MIC_ROOM_MIX) as f32;
            for (dry, wet) in samples.iter_mut().zip(wet) {
                *dry = *dry * (1.0 - mix) + wet * mix;
            }
        }
        // A fixed 3 ms release removes only the artificial file-end edge.
        let fade = (rate as usize * 3 / 1000).min(samples.len());
        let len = samples.len();
        for (i, v) in samples.iter_mut().skip(len - fade).enumerate() {
            *v *= 1.0 - i as f32 / fade.max(1) as f32;
        }
        let peak = samples.iter().fold(0f32, |a, b| a.max(b.abs()));
        if peak > 0.98 {
            for v in &mut samples {
                *v *= 0.98 / peak;
            }
        }
        if !progress(1.0) {
            return None;
        }
        let animation = Arc::new(animation);
        if let Ok(mut last) = self.last.lock() {
            *last = Some((p.key(60, 100, 48_000), animation.clone()));
        }
        Some(Render {
            samples: Arc::new(samples),
            animation,
            rate,
            millis: start.elapsed().as_secs_f64() * 1000.0,
            wire_energy: sim.wire_energy,
            shell_energy: sim.shell_energy,
            max_tension: sim.max_tension,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dry() -> Patch {
        let mut p = Patch::default();
        for i in [
            BATTER_STIFFNESS,
            RESONANT_STIFFNESS,
            BATTER_AIR_LOADING,
            RESONANT_AIR_LOADING,
            MIC_ROOM_MIX,
            WIRES_COUNT,
            SHELL_SHELL_MODES,
            MUFFLE_RIM_DAMPING,
            TIME_PRE_ROLL,
        ] {
            p.set(i, 0.);
        }
        p.set(TIME_LENGTH, 0.2);
        p.set(TIME_SAMPLE_RATE, 48_000.);
        p.set(TIME_OVERSAMPLING, 1.);
        p
    }
    #[test]
    fn frequencies_and_centre_symmetry() {
        let mut p = dry();
        p.set(STRIKE_POSITION, 0.);
        let h = Head::new(&p, 0, 48_000., 32, 60);
        let f = h.shapes[0].hz;
        for (n, m, ratio) in [(1, 0, 1.593), (2, 0, 2.135), (0, 1, 2.295)] {
            let root = zeros().0[n][m];
            let s = h
                .shapes
                .iter()
                .find(|s| s.order == n && (s.root - root).abs() < 1e-8)
                .unwrap();
            assert!((s.hz / f - ratio).abs() < 0.001);
        }
        for (s, k) in h.shapes.iter().zip(&h.strike) {
            if s.order > 0 {
                assert_eq!(*k, 0.);
            }
        }
        let r = Membrane::default()
            .render(&p, 60, 100, 48_000, false, &mut |_| true)
            .unwrap();
        for row in r.animation.field(0.025, 16, 32, false).chunks(32) {
            for v in row {
                assert!((v - row[0]).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn deterministic_cancellable_and_rigid_shell() {
        let p = dry();
        let e = Membrane::default();
        let a = e.render(&p, 60, 100, 48_000, true, &mut |_| true).unwrap();
        let b = e.render(&p, 60, 100, 48_000, true, &mut |_| true).unwrap();
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.shell_energy, 0.);
        assert!(a.samples.iter().any(|v| v.abs() > 1e-4));
        assert!(
            e.render(&p, 60, 100, 48_000, true, &mut |f| f < 0.1)
                .is_none()
        );
    }
    #[test]
    fn preview_split_state_and_velocity_glide() {
        let p = dry();
        let mut a = Simulation::new(&p, 60, 100, 48_000., false);
        let mut b = Simulation::new(&p, 60, 100, 48_000., false);
        let x: Vec<_> = (0..2048).map(|_| a.tick()).collect();
        let mut y: Vec<_> = (0..713).map(|_| b.tick()).collect();
        y.extend((713..2048).map(|_| b.tick()));
        assert_eq!(x, y);
        let e = Membrane::default();
        let hard = e.render(&p, 60, 127, 48_000, true, &mut |_| true).unwrap();
        let soft = e.render(&p, 60, 10, 48_000, true, &mut |_| true).unwrap();
        assert!(hard.max_tension > soft.max_tension * 3.0);
    }
    #[test]
    fn separated_wires_are_silent() {
        let mut p = dry();
        p.set(WIRES_COUNT, 8.);
        p.set(WIRES_REST_GAP, 0.005);
        let e = Membrane::default();
        let far = e.render(&p, 60, 30, 48_000, true, &mut |_| true).unwrap();
        assert_eq!(far.wire_energy, 0.);
        p.set(WIRES_REST_GAP, 0.);
        let near = e.render(&p, 60, 110, 48_000, true, &mut |_| true).unwrap();
        assert!(near.wire_energy > 0.);
    }

    fn spectrum(samples: &[f32], from: usize, to: usize) -> Vec<f64> {
        use crate::dsp::fft::{RealFft, Window, fill_window};
        let n = 2048;
        let mut fft = RealFft::new();
        fft.prepare(n);
        let mut window = vec![0.; n];
        fill_window(Window::Hann, &mut window);
        let mut real = vec![0.; RealFft::bins(n)];
        let mut imag = real.clone();
        let mut power = vec![0.; real.len()];
        let mut scratch = vec![0.; RealFft::scratch_len(n)];
        for offset in (from..to.min(samples.len()).saturating_sub(n)).step_by(n / 4) {
            let x: Vec<_> = samples[offset..offset + n]
                .iter()
                .zip(&window)
                .map(|(x, w)| x * w)
                .collect();
            fft.forward(&x, &mut real, &mut imag, &mut scratch);
            for ((p, r), i) in power.iter_mut().zip(&real).zip(&imag) {
                *p += f64::from(r * r + i * i);
            }
        }
        power
    }
    #[test]
    fn wire_radiation_is_broadband_and_its_tail_dies_before_the_head() {
        let mut p = Patch::default();
        p.set(TIME_LENGTH, 0.25);
        p.set(MIC_ROOM_MIX, 0.0);
        let e = Membrane::default();
        let on = e.render(&p, 60, 100, 48_000, true, &mut |_| true).unwrap();
        p.set(WIRES_COUNT, 0.0);
        let off = e.render(&p, 60, 100, 48_000, true, &mut |_| true).unwrap();
        // Measure the rattle above the low membrane modes. Including the
        // deliberately pitched body would test whether the DRUM is white.
        let power = spectrum(&on.samples, 1440, 9600);
        let bins = &power[86..683];
        let flat = (bins.iter().map(|p| p.max(1e-30).ln()).sum::<f64>() / bins.len() as f64).exp()
            / (bins.iter().sum::<f64>() / bins.len() as f64);
        assert!(flat > 0.7, "2–16 kHz flatness: {flat}");
        let rms = |x: &[f32], a: usize, b: usize| {
            x[a..b].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>()
        };
        let decay = |x: &[f32]| rms(x, 7200, 8640) / rms(x, 1440, 2880);
        assert!(decay(&on.samples) < decay(&off.samples));
    }
    #[test]
    fn cavity_transfers_energy_and_equal_heads_beat() {
        let mut p = dry();
        p.set(MODES_MODE_COUNT, 4.0);
        p.set(RESONANT_TENSION, p.get(BATTER_TENSION) as f32);
        p.set(RESONANT_DENSITY, p.get(BATTER_DENSITY) as f32);
        for i in [
            BATTER_LOSS_LOW,
            BATTER_LOSS_HIGH,
            RESONANT_LOSS_LOW,
            RESONANT_LOSS_HIGH,
            SHELL_AIR_DAMPING,
        ] {
            p.set(i, 0.0);
        }
        let envelope = |patch: &Patch| {
            let mut s = Simulation::new(patch, 60, 0, 48_000., true);
            s.mallet_q = -1.0;
            s.top.modes[0].v = 0.01;
            let mut levels = Vec::new();
            let mut bottom = 0f64;
            for i in 0..24000 {
                s.tick();
                if i > 1000 {
                    let m = &s.top.modes[0];
                    levels.push((m.q * m.q + m.v * m.v / m.omega2).sqrt());
                    bottom = bottom.max(s.bottom.modes[0].energy());
                }
            }
            (levels, bottom)
        };
        let (tied, carry) = envelope(&p);
        assert!(carry > 1e-10);
        let depth = |x: &[f64]| {
            1.0 - x.iter().copied().fold(f64::INFINITY, f64::min)
                / x.iter().copied().fold(0., f64::max)
        };
        p.set(
            RESONANT_TENSION,
            (p.get(BATTER_TENSION) * 0.75f64.powi(2)) as f32,
        );
        let (detuned, _) = envelope(&p);
        assert!(
            depth(&tied) > depth(&detuned),
            "tied {} detuned {}",
            depth(&tied),
            depth(&detuned)
        );
        p.set(SHELL_HEAD_COUPLING, 0.0);
        assert_eq!(envelope(&p).1, 0.0);
    }
    #[test]
    fn size_strike_and_muffle_follow_their_acoustic_claims() {
        let e = Membrane::default();
        let mut p = dry();
        p.set(TIME_LENGTH, 0.3);
        p.turn_macro(0, 0.0);
        let small = Head::new(&p, 0, 48_000., 32, 60);
        p.turn_macro(0, 1.0);
        let large = Head::new(&p, 0, 48_000., 32, 60);
        assert!(large.shapes[0].hz < small.shapes[0].hz / 3.0);
        assert!(large.shapes[0].alpha < small.shapes[0].alpha);
        let centroid = |x: &[f32]| {
            let power = spectrum(x, 0, 4800);
            let total = power.iter().sum::<f64>();
            power
                .iter()
                .enumerate()
                .map(|(i, p)| i as f64 * p)
                .sum::<f64>()
                / total
        };
        p = dry();
        p.turn_macro(3, 0.0);
        let soft = e.render(&p, 60, 100, 48_000, false, &mut |_| true).unwrap();
        p.turn_macro(3, 1.0);
        let hard = e.render(&p, 60, 100, 48_000, false, &mut |_| true).unwrap();
        assert!(centroid(&hard.samples) > centroid(&soft.samples));
        p = dry();
        let open = Head::new(&p, 0, 48_000., 32, 60);
        p.turn_macro(5, 1.0);
        let muffled = Head::new(&p, 0, 48_000., 32, 60);
        assert!(muffled.shapes[0].alpha > open.shapes[0].alpha);
    }
}
