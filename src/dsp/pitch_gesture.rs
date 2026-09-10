//! Sample-clocked pitch gestures, in semitones. No synth or score ownership.
//!
//! These are adjustable pitch interpretations, not a raga grammar or vocal
//! model. Neighbor intervals are explicit: no hidden scale snapping. Note
//! gestures snapshot their configuration at trigger; vibrato remains live.
use super::lfo::Lfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Gesture {
    #[default]
    Off,
    Kan,
    Meend,
    Gamak,
    Khatka,
    Andolan,
    Murki,
}

impl Gesture {
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Kan,
            2 => Self::Meend,
            3 => Self::Gamak,
            4 => Self::Khatka,
            5 => Self::Andolan,
            6 => Self::Murki,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GestureConfig {
    pub kind: Gesture,
    pub time_ms: f32,
    pub speed_hz: f32,
    pub from_cents: f32,
    pub other_cents: f32,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            kind: Gesture::Off,
            time_ms: 140.0,
            speed_hz: 5.0,
            from_cents: -100.0,
            other_cents: 200.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PitchGesture {
    sample_rate: f32,
    vibrato: Lfo,
    swing: Lfo,
    depth: f32,
    target_depth: f32,
    smoothing: f32,
    config: GestureConfig,
    elapsed: u32,
    duration: u32,
    meend_from: f32,
    last_ornament: f32,
}

impl Default for PitchGesture {
    fn default() -> Self {
        let mut motion = Self {
            sample_rate: 48_000.0,
            vibrato: Lfo::new(),
            swing: Lfo::new(),
            depth: 0.0,
            target_depth: 0.0,
            smoothing: 0.0,
            config: GestureConfig::default(),
            elapsed: 0,
            duration: 0,
            meend_from: 0.0,
            last_ornament: 0.0,
        };
        motion.prepare(48_000.0);
        motion
    }
}

fn finite(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// Piecewise smooth pitch paths. Fixed-size, bounded work, no lookup allocation.
fn path(x: f32, knots: &[(f32, f32)]) -> f32 {
    for pair in knots.windows(2) {
        if let [(a, y), (b, z)] = pair
            && x <= *b
        {
            let t = ((x - a) / (b - a).max(1e-6)).clamp(0.0, 1.0);
            return y + (z - y) * smooth(t);
        }
    }
    0.0
}

impl PitchGesture {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = finite(sample_rate, 48_000.0, 1.0, 768_000.0);
        self.vibrato.prepare(self.sample_rate);
        self.swing.prepare(self.sample_rate);
        self.smoothing = super::ramps::one_pole_coeff(1.0 / (0.020 * self.sample_rate));
        self.reset();
    }

    pub fn reset(&mut self) {
        self.vibrato.reset();
        self.swing.reset();
        self.depth = 0.0;
        self.target_depth = 0.0;
        self.config.kind = Gesture::Off;
        self.elapsed = 0;
        self.duration = 0;
        self.last_ornament = 0.0;
    }

    /// Intensity is peak deviation, not peak-to-peak. Zero is exactly bypassed.
    pub fn vibrato(&mut self, speed_hz: f32, intensity_cents: f32) {
        self.vibrato.set_rate(finite(speed_hz, 5.2, 0.1, 12.0));
        self.target_depth = finite(intensity_cents, 0.0, 0.0, 100.0) * 0.01;
    }

    /// Trigger on every note, including legato notes, without retriggering VCA
    /// or filter. `from` is previous sounding pitch minus destination (st).
    pub fn trigger(&mut self, config: GestureConfig, from: Option<f32>) {
        self.config = GestureConfig {
            kind: config.kind,
            time_ms: finite(config.time_ms, 140.0, 20.0, 2_000.0),
            speed_hz: finite(config.speed_hz, 5.0, 0.1, 16.0),
            from_cents: finite(config.from_cents, -100.0, -1_200.0, 1_200.0),
            other_cents: finite(config.other_cents, 200.0, -1_200.0, 1_200.0),
        };
        self.meend_from = finite(from.unwrap_or(0.0), 0.0, -127.0, 127.0);
        self.duration = (self.config.time_ms * 0.001 * self.sample_rate)
            .round()
            .max(1.0) as u32;
        self.elapsed = 0;
        self.swing.reset();
        self.swing.set_rate(self.config.speed_hz);
        if self.config.kind == Gesture::Gamak {
            self.swing.set_phase(0.75);
        }
        // One vibrato phase across a legato phrase; fresh attacks start at zero.
        if from.is_none() {
            self.vibrato.reset();
        }
    }

    pub fn last_ornament(&self) -> f32 {
        self.last_ornament
    }

    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out {
            let mut vib = [0.0];
            let mut swing = [0.0];
            self.vibrato.process(&mut vib);
            self.swing.process(&mut swing);
            self.depth += (self.target_depth - self.depth) * self.smoothing;
            if (self.target_depth - self.depth).abs() < 1e-7 {
                self.depth = self.target_depth;
            }
            let x = self.elapsed as f32 / self.duration.max(1) as f32;
            let a = self.config.from_cents * 0.01;
            let b = self.config.other_cents * 0.01;
            let value = if self.elapsed >= self.duration {
                0.0
            } else {
                match self.config.kind {
                    Gesture::Off => 0.0,
                    Gesture::Kan => a * (1.0 - smooth(((x - 0.1) / 0.9).clamp(0.0, 1.0))),
                    Gesture::Meend => self.meend_from * (1.0 - smooth(x)),
                    Gesture::Khatka => path(
                        x,
                        &[
                            (0.0, 0.0),
                            (0.12, a),
                            (0.26, 0.0),
                            (0.40, b),
                            (0.58, 0.0),
                            (1.0, 0.0),
                        ],
                    ),
                    Gesture::Murki => path(x, &[(0.0, 0.0), (0.25, a), (0.65, b), (1.0, 0.0)]),
                    Gesture::Gamak | Gesture::Andolan => {
                        // A 20 ms edge joins the periodic gesture to the main
                        // note. Gamak has a firmer, directional excursion;
                        // andolan smoothly visits independently set neighbors.
                        let edge = (0.020 * self.sample_rate)
                            .min(self.duration as f32 * 0.5)
                            .max(1.0);
                        let fade = smooth((self.elapsed as f32 / edge).min(1.0))
                            * smooth(((self.duration - self.elapsed) as f32 / edge).min(1.0));
                        if self.config.kind == Gesture::Gamak {
                            a * ((swing[0] + 1.0) * 0.5).sqrt() * fade
                        } else {
                            let neighbor = if swing[0] >= 0.0 { a } else { b };
                            neighbor * smooth(swing[0].abs()) * fade
                        }
                    }
                }
            };
            self.last_ornament = value;
            *sample = value + vib[0] * self.depth;
            self.elapsed = self.elapsed.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grace_and_meend_reach_the_main_note_at_the_declared_time() {
        for kind in [Gesture::Kan, Gesture::Meend] {
            let mut g = PitchGesture::default();
            g.prepare(1_000.0);
            g.trigger(
                GestureConfig {
                    kind,
                    time_ms: 100.0,
                    from_cents: -200.0,
                    ..Default::default()
                },
                Some(-2.0),
            );
            let mut out = [0.0; 101];
            g.process(&mut out);
            assert_eq!(out[0], -2.0);
            assert_eq!(out[100], 0.0);
            assert!(out.windows(2).all(|w| w[1] >= w[0]));
            if kind == Gesture::Meend {
                assert_eq!(out[50], -1.0);
            }
        }
    }
    #[test]
    fn vibrato_rate_and_peak_intensity_are_real_units() {
        let mut g = PitchGesture::default();
        g.prepare(1_000.0);
        g.vibrato(5.0, 25.0);
        let mut out = [0.0; 2_000];
        g.process(&mut out);
        let steady = &out[1_000..];
        let crossings = steady
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        assert_eq!(crossings, 5);
        assert!((steady.iter().copied().fold(0.0, f32::max) - 0.25).abs() < 1e-4);
        g.vibrato(5.0, 0.0);
        g.process(&mut out);
        assert_eq!(out[1_999], 0.0);
    }
    #[test]
    fn all_gestures_split_exactly_take_edge_lengths_and_do_not_allocate() {
        for index in 0..=6 {
            let mut full = PitchGesture::default();
            full.vibrato(5.7, 23.0);
            full.trigger(
                GestureConfig {
                    kind: Gesture::from_index(index),
                    ..Default::default()
                },
                Some(-7.0),
            );
            let mut split = full.clone();
            let mut a = [0.0; 4_097];
            let mut b = [0.0; 4_097];
            assert_no_alloc::assert_no_alloc(|| {
                full.process(&mut a);
                split.process(&mut []);
                split.process(&mut b[..1]);
                split.process(&mut b[1..101]);
                for block in b[101..].chunks_mut(37) {
                    split.process(block);
                }
            });
            assert_eq!(a, b);
            assert!(a.iter().all(|x| x.is_finite() && x.abs() <= 13.0));
        }
    }
    #[test]
    fn distinct_turns_visit_both_neighbors_and_finish_at_zero() {
        let mut results = Vec::new();
        for kind in [
            Gesture::Khatka,
            Gesture::Murki,
            Gesture::Gamak,
            Gesture::Andolan,
        ] {
            let mut g = PitchGesture::default();
            g.prepare(1_000.0);
            g.trigger(
                GestureConfig {
                    kind,
                    time_ms: 1_000.0,
                    speed_hz: 2.0,
                    ..Default::default()
                },
                None,
            );
            let mut out = [0.0; 1_001];
            g.process(&mut out);
            assert_eq!(out[1_000], 0.0);
            assert!(out.iter().any(|x| *x < -0.9));
            if kind != Gesture::Gamak {
                assert!(out.iter().any(|x| *x > 1.9));
            }
            assert!(!results.contains(&out));
            results.push(out);
        }
    }
    #[test]
    fn reset_silence_invalid_inputs_and_long_tails_are_safe() {
        let mut g = PitchGesture::default();
        g.prepare(f32::NAN);
        g.vibrato(f32::INFINITY, f32::NAN);
        g.trigger(
            GestureConfig {
                kind: Gesture::Murki,
                time_ms: f32::NAN,
                from_cents: f32::INFINITY,
                ..Default::default()
            },
            Some(f32::NAN),
        );
        let mut out = [0.0; 100_001];
        g.process(&mut out);
        assert!(out.iter().all(|x| x.is_finite() && !x.is_subnormal()));
        assert_eq!(out[100_000], 0.0);
        g.reset();
        g.process(&mut out[..31]);
        assert!(out[..31].iter().all(|x| *x == 0.0));
    }
}
