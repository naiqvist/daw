//! Resident-sample traversal and bounded windowed time stretching.
//!
//! State: fixed POD reader and two overlapping grains, no buffers or Drop.
//! Cost: two stereo Hermite reads/sample in grain mode; Smooth adds a bounded
//! 65 x 24 mono correlation search per hop. Source is immutable and planar.
//! Denormals: finite source stays finite; engine FTZ covers decaying sources.
//! In-place: source and outputs must be disjoint. Latency: 0 (random access
//! source permits read-ahead; this is not a streaming processor).
//! Smooth is waveform-similarity overlap-add, not spectral/formant shifting.

use super::interp::hermite4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    #[default]
    Repitch,
    Beats,
    Smooth,
    Grain,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Loop {
    #[default]
    Off,
    Forward,
    PingPong,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Half-open source region, in frames.
    pub start: f64,
    pub end: f64,
    pub loop_start: f64,
    pub loop_end: f64,
    pub looping: Loop,
    pub crossfade: f64,
    pub pitch: f64,
    /// Signed source frames per output frame, independent of pitch when
    /// stretched. Repitch multiplies this speed by pitch.
    pub speed: f64,
    /// Original phrase clock; temporary reverse/hold does not alter it.
    pub slip_speed: f64,
    pub window: usize,
    pub method: Method,
    pub transient: f32,
    pub hard: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 0.0,
            loop_start: 0.0,
            loop_end: 0.0,
            looping: Loop::Off,
            crossfade: 0.0,
            pitch: 1.0,
            speed: 1.0,
            slip_speed: 1.0,
            window: 2048,
            method: Method::Repitch,
            transient: 1.0,
            hard: false,
        }
    }
}

impl Settings {
    fn sane(mut self) -> Self {
        self.start = finite(self.start, 0.0).max(0.0);
        self.end = finite(self.end, self.start).max(self.start);
        self.loop_start = finite(self.loop_start, self.start).clamp(self.start, self.end);
        self.loop_end = finite(self.loop_end, self.end).clamp(self.loop_start, self.end);
        self.pitch = finite(self.pitch, 1.0).clamp(0.015625, 64.0);
        self.speed = finite(self.speed, 1.0).clamp(-8.0, 8.0);
        self.crossfade =
            finite(self.crossfade, 0.0).clamp(0.0, (self.loop_end - self.loop_start) * 0.5);
        self.window = self.window.clamp(
            if self.method == Method::Smooth {
                256
            } else {
                16
            },
            32_768,
        );
        self.slip_speed = finite(self.slip_speed, 1.0).clamp(-8.0, 8.0);
        self.transient = if self.transient.is_finite() {
            self.transient.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if self.loop_end - self.loop_start < 2.0 {
            self.looping = Loop::Off;
        }
        self
    }
}

fn finite(x: f64, fallback: f64) -> f64 {
    if x.is_finite() { x } else { fallback }
}

/// Hermite neighbours are clamped to the selected region, not the file:
/// interpolation can never leak the preceding kick into a snare slice.
pub fn region_read(source: &[f32], pos: f64, lo: f64, hi: f64) -> f32 {
    if source.is_empty() || !pos.is_finite() || hi <= lo {
        return 0.0;
    }
    let first = (lo.ceil().max(0.0) as usize).min(source.len());
    let end = (hi.ceil().max(0.0) as usize).min(source.len());
    if first >= end {
        return 0.0;
    }
    let p = pos.clamp(first as f64, (end - 1) as f64);
    let i = p.floor() as usize;
    let sample = |index: usize| {
        source
            .get(index.clamp(first, end - 1))
            .copied()
            .unwrap_or(0.0)
    };
    hermite4(
        sample(i.saturating_sub(1)),
        sample(i),
        sample(i.saturating_add(1)),
        sample(i.saturating_add(2)),
        (p - i as f64) as f32,
    )
}

#[derive(Clone, Copy, Debug)]
struct Grain {
    pos: f64,
    age: usize,
    len: usize,
    live: bool,
}
impl Grain {
    const EMPTY: Self = Self {
        pos: 0.0,
        age: 0,
        len: 16,
        live: false,
    };
}

#[derive(Clone, Copy, Debug)]
pub struct SampleReader {
    settings: Settings,
    pos: f64,
    slip: f64,
    direction: f64,
    grains: [Grain; 2],
    hop_left: usize,
    next: usize,
    active: bool,
    entered_loop: bool,
    previous: [f32; 2],
    seam: [f32; 2],
    fade_left: usize,
    since: usize,
    onset: f64,
    onsets: [u64; 64],
    onset_count: usize,
    previous_pos: f64,
}

impl Default for SampleReader {
    fn default() -> Self {
        Self::new()
    }
}
impl SampleReader {
    pub fn new() -> Self {
        Self {
            settings: Settings::default(),
            pos: 0.0,
            slip: 0.0,
            direction: 1.0,
            grains: [Grain::EMPTY; 2],
            hop_left: 0,
            next: 0,
            active: false,
            entered_loop: false,
            previous: [0.0; 2],
            seam: [0.0; 2],
            fade_left: 0,
            since: 0,
            onset: 0.0,
            onsets: [0; 64],
            onset_count: 0,
            previous_pos: 0.0,
        }
    }
    pub fn prepare(&mut self, settings: Settings) {
        self.settings = settings.sane();
        self.reset();
    }
    pub fn reset(&mut self) {
        self.active = false;
        self.grains = [Grain::EMPTY; 2];
        self.hop_left = 0;
        self.next = 0;
        self.previous = [0.0; 2];
        self.fade_left = 0;
        self.since = 0;
        self.entered_loop = false;
    }
    pub fn start(&mut self, position: f64) {
        self.reset();
        self.direction = if self.settings.speed < 0.0 { -1.0 } else { 1.0 };
        self.pos = finite(position, self.settings.start).clamp(
            self.settings.start,
            (self.settings.end - 1.0).max(self.settings.start),
        );
        self.slip = self.pos;
        self.onset = self.pos;
        self.previous_pos = self.pos;
        self.active = self.settings.end > self.settings.start;
        self.entered_loop = self.settings.looping != Loop::Off
            && self.pos >= self.settings.loop_start
            && self.pos < self.settings.loop_end;
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn position(&self) -> f64 {
        self.pos
    }
    pub fn slip_position(&self) -> f64 {
        self.slip
    }
    pub fn speed(&self) -> f64 {
        self.settings.speed
    }
    /// Sorted green-zone onset analysis, copied into bounded reader state.
    pub fn set_onsets(&mut self, onsets: &[u64]) {
        self.onset_count = onsets.len().min(64);
        for (dst, src) in self.onsets.iter_mut().zip(onsets) {
            *dst = *src;
        }
    }
    pub fn latency(&self) -> usize {
        0
    }
    /// Changes leave the envelope and grain clock running. Relocations use a
    /// short bounded seam fade; hard mode deliberately exposes discontinuity.
    pub fn set(&mut self, settings: Settings) {
        let next = settings.sane();
        let moved = next.start != self.settings.start
            || next.end != self.settings.end
            || next.method != self.settings.method;
        let shift = next.loop_start - self.settings.loop_start;
        if self.entered_loop && shift != 0.0 {
            self.pos += shift;
            for g in &mut self.grains {
                g.pos += shift;
            }
        }
        if next.speed.signum() != self.settings.speed.signum() && next.speed != 0.0 {
            self.direction = next.speed.signum();
        }
        if self.settings.looping != Loop::Off && next.looping == Loop::Off {
            self.entered_loop = false;
        }
        let jump = shift
            .abs()
            .max((next.loop_end - self.settings.loop_end - shift).abs());
        if self.active
            && !next.hard
            && jump > 32.0
            && (next.looping != Loop::Off || self.settings.looping != Loop::Off)
        {
            self.seam = self.previous;
            self.fade_left = 64;
        }
        self.settings = next;
        if moved {
            self.seek(self.pos.clamp(next.start, (next.end - 1.0).max(next.start)));
        }
    }
    pub fn seek(&mut self, position: f64) {
        self.pos = position.clamp(
            self.settings.start,
            (self.settings.end - 1.0).max(self.settings.start),
        );
        self.grains = [Grain::EMPTY; 2];
        self.hop_left = 0;
        self.next = 0;
        self.onset = self.pos;
        self.previous_pos = self.pos;
        self.since = 0;
        self.seam = self.previous;
        self.fade_left = if self.settings.hard { 0 } else { 64 };
    }
    pub fn rejoin(&mut self) {
        self.seek(self.slip);
    }
    pub fn exit_loop(&mut self) {
        self.settings.looping = Loop::Off;
        self.entered_loop = false;
    }

    fn read(&self, source: &[f32], position: f64) -> f32 {
        let s = self.settings;
        let mut p = position;
        if self.entered_loop && s.looping != Loop::Off {
            if s.looping == Loop::PingPong {
                let length = (s.loop_end - s.loop_start - 1.0).max(1.0);
                let phase = (p - s.loop_start).rem_euclid(2.0 * length);
                p = s.loop_start
                    + if phase <= length {
                        phase
                    } else {
                        2.0 * length - phase
                    };
            } else if p < s.loop_start || p >= s.loop_end {
                let skip = if (self.direction > 0.0 && s.loop_start - s.crossfade < s.start)
                    || (self.direction < 0.0 && s.loop_end + s.crossfade >= s.end)
                {
                    s.crossfade
                } else {
                    0.0
                };
                let period = (s.loop_end - s.loop_start - skip).max(1.0);
                p = if self.direction > 0.0 {
                    s.loop_start + skip + (p - s.loop_end).rem_euclid(period)
                } else {
                    s.loop_end - skip - (s.loop_start - p).rem_euclid(period)
                };
            }
        }
        let a = region_read(source, p, s.start, s.end);
        if s.looping != Loop::Forward || s.crossfade <= 0.0 {
            return a;
        }
        let distance = if self.direction > 0.0 {
            s.loop_end - p
        } else {
            p - s.loop_start
        };
        if distance < 0.0 || distance >= s.crossfade {
            return a;
        }
        let other = if self.direction > 0.0 {
            s.loop_start - s.crossfade + (s.crossfade - distance)
        } else {
            s.loop_end + s.crossfade - (s.crossfade - distance)
        };
        // Prefer source preceding the loop, but stay within the selected
        // region. At a region edge use the corresponding in-loop span.
        let other = if self.direction > 0.0 && s.loop_start - s.crossfade < s.start {
            other + s.crossfade
        } else if self.direction < 0.0 && s.loop_end + s.crossfade >= s.end {
            other - s.crossfade
        } else {
            other
        };
        let t = (1.0 - distance / s.crossfade) as f32;
        a * (1.0 - t) + region_read(source, other, s.start, s.end) * t
    }

    fn aligned(&self, source: &[f32], target: f64, reference: f64) -> f64 {
        let s = self.settings;
        if s.method != Method::Smooth || self.since == 0 {
            return target;
        }
        let radius = (s.window as f64 * 0.2).min(384.0);
        let mut best = target;
        let mut score = f64::NEG_INFINITY;
        for k in 0..65 {
            let candidate = target + (k as f64 - 32.0) * radius / 32.0;
            if candidate < s.start || candidate >= s.end {
                continue;
            }
            let (mut dot, mut aa, mut bb) = (0.0f64, 1e-12f64, 1e-12f64);
            for j in 0..24 {
                let delta = j as f64 * s.pitch * self.direction * 2.0;
                let a = self.read(source, reference + delta) as f64;
                let b = self.read(source, candidate + delta) as f64;
                dot += a * b;
                aa += a * a;
                bb += b * b;
            }
            let quality =
                dot / (aa * bb).sqrt() - 0.0001 * (candidate - target).abs() / radius.max(1.0);
            if quality > score {
                score = quality;
                best = candidate;
            }
        }
        best
    }

    fn advance(&mut self) {
        let s = self.settings;
        let speed = s.speed.abs()
            * self.direction
            * if s.method == Method::Repitch {
                s.pitch
            } else {
                1.0
            };
        self.previous_pos = self.pos;
        self.pos += speed;
        self.slip += s.slip_speed;
        let span = (s.end - s.start).max(1.0);
        self.slip = s.start + (self.slip - s.start).rem_euclid(span);
        if s.looping != Loop::Off {
            let length = s.loop_end - s.loop_start;
            let crossed = if self.direction > 0.0 {
                self.pos
                    >= s.loop_end
                        - if s.looping == Loop::PingPong {
                            1.0
                        } else {
                            0.0
                        }
            } else {
                self.pos < s.loop_start
            };
            if crossed {
                self.entered_loop = true;
                if s.looping == Loop::Forward {
                    let skip = if (self.direction > 0.0 && s.loop_start - s.crossfade < s.start)
                        || (self.direction < 0.0 && s.loop_end + s.crossfade >= s.end)
                    {
                        s.crossfade
                    } else {
                        0.0
                    };
                    let period = (length - skip).max(1.0);
                    self.pos = if self.direction > 0.0 {
                        s.loop_start + skip + (self.pos - s.loop_end).rem_euclid(period)
                    } else {
                        s.loop_end - skip - (s.loop_start - self.pos).rem_euclid(period)
                    };
                } else {
                    let length = (length - 1.0).max(1.0);
                    let unfolded = if self.direction > 0.0 {
                        self.pos - s.loop_start
                    } else {
                        2.0 * length - (self.pos - s.loop_start)
                    };
                    let cycle = unfolded.rem_euclid(2.0 * length);
                    if cycle < length {
                        self.pos = s.loop_start + cycle;
                        self.direction = 1.0;
                    } else {
                        self.pos = s.loop_start + length - (cycle - length);
                        self.direction = -1.0;
                    }
                    self.pos = self.pos.min(s.loop_end - 1.0);
                }
            }
        } else if self.pos >= s.end || self.pos < s.start {
            self.active = false;
        }
        self.since = self.since.saturating_add(1);
    }

    pub fn tick(&mut self, left: &[f32], right: &[f32]) -> [f32; 2] {
        if !self.active {
            return [0.0; 2];
        }
        let s = self.settings;
        if s.method == Method::Beats && s.speed != 0.0 {
            // Bounded binary search on the preanalysed onset table. A new
            // hit resets the grains, not the transport head or envelope.
            let end = self.onset_count.min(self.onsets.len());
            let markers = &self.onsets[..end];
            let i = if self.direction > 0.0 {
                markers
                    .partition_point(|at| (*at as f64) <= self.pos)
                    .saturating_sub(1)
            } else {
                markers.partition_point(|at| (*at as f64) < self.pos)
            };
            if let Some(at) = markers.get(i).map(|at| *at as f64) {
                let crossed = if self.direction > 0.0 {
                    at > self.previous_pos && at <= self.pos
                } else {
                    at < self.previous_pos && at >= self.pos
                };
                if crossed && at >= s.start && at < s.end {
                    self.onset = at;
                    self.since = 0;
                    self.grains = [Grain::EMPTY; 2];
                    self.next = 0;
                    self.hop_left = 0;
                }
            }
        }
        let mut output = [0.0f32; 2];
        if s.method == Method::Repitch {
            if s.speed != 0.0 {
                output = [self.read(left, self.pos), self.read(right, self.pos)];
            }
        } else {
            let len = s.window;
            if self.hop_left == 0 {
                let reference = self
                    .grains
                    .iter()
                    .find(|g| g.live)
                    .map_or(self.pos, |g| g.pos);
                let pos = self.aligned(left, self.pos, reference);
                if let Some(g) = self.grains.get_mut(self.next) {
                    *g = Grain {
                        pos,
                        age: 0,
                        len,
                        live: true,
                    };
                }
                if self.since == 0 {
                    if let Some(g) = self.grains.get_mut(1) {
                        *g = Grain {
                            pos: self.pos,
                            age: len / 2,
                            len,
                            live: true,
                        };
                    }
                }
                self.next = (self.next + 1) % 2;
                self.hop_left = (len / 2).max(1);
            }
            let mut weight = 0.0f32;
            for i in 0..2 {
                let Some(g) = self.grains.get(i).copied() else {
                    continue;
                };
                if !g.live {
                    continue;
                }
                let phase = g.age as f32 / g.len as f32;
                let w = (core::f32::consts::PI * phase).sin().powi(2);
                output[0] += self.read(left, g.pos) * w;
                output[1] += self.read(right, g.pos) * w;
                weight += w;
                if let Some(next) = self.grains.get_mut(i) {
                    next.age += 1;
                    next.pos += s.pitch * self.direction;
                    next.live = next.age < next.len;
                }
            }
            if weight > 1e-8 {
                for value in &mut output {
                    *value /= weight;
                }
            }
            if s.method == Method::Beats {
                // Attacks pass directly for a short window after each onset
                // restart, using source-onset anchors analysed at load.
                let attack = ((128.0 - self.since as f32) / 128.0).clamp(0.0, 1.0) * s.transient;
                let attack_pos = self.onset + self.since as f64 * s.pitch * self.direction;
                let direct = [self.read(left, attack_pos), self.read(right, attack_pos)];
                for (out, dry) in output.iter_mut().zip(direct) {
                    *out = *out * (1.0 - attack) + dry * attack;
                }
            }
            self.hop_left = self.hop_left.saturating_sub(1);
        }
        if self.fade_left > 0 {
            let t = 1.0 - self.fade_left as f32 / 64.0;
            for (out, old) in output.iter_mut().zip(self.seam) {
                *out = old * (1.0 - t) + *out * t;
            }
            self.fade_left -= 1;
        }
        self.previous = output;
        self.advance();
        output
    }
    pub fn process(&mut self, left: &[f32], right: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        for (l, r) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let pair = self.tick(left, right);
            *l = pair[0];
            *r = pair[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reader(method: Method, end: f64) -> SampleReader {
        let mut r = SampleReader::new();
        r.prepare(Settings {
            end,
            loop_end: end,
            method,
            ..Settings::default()
        });
        r.start(0.0);
        r
    }
    #[test]
    fn unity_is_exact_and_region_cannot_leak() {
        let source: Vec<_> = (0..512).map(|x| x as f32 / 512.0).collect();
        let mut r = reader(Method::Repitch, 512.0);
        let mut l = [0.0; 512];
        let mut rr = l;
        r.process(&source, &source, &mut l, &mut rr);
        assert_eq!(l.as_slice(), source);
        assert!(!r.active());
        let source = [99.0, 99.0, 0.0, 0.0, 99.0];
        for i in 0..100 {
            assert_eq!(region_read(&source, 2.0 + i as f64 / 100.0, 2.0, 4.0), 0.0);
        }
    }
    #[test]
    fn split_blocks_all_methods() {
        let source: Vec<_> = (0..4096).map(|i| (i as f32 * 0.057).sin()).collect();
        for mode in [
            Method::Repitch,
            Method::Beats,
            Method::Smooth,
            Method::Grain,
        ] {
            let mut a = reader(mode, 4096.0);
            a.set(Settings {
                end: 4096.0,
                method: mode,
                window: 128,
                speed: 0.3,
                pitch: 1.5,
                ..Settings::default()
            });
            let mut b = a;
            let (mut whole, mut right) = ([0.0; 512], [0.0; 512]);
            a.process(&source, &source, &mut whole, &mut right);
            let mut split = [0.0; 512];
            for range in [0..100, 100..357, 357..512] {
                let n = range.len();
                b.process(&source, &source, &mut split[range], &mut right[..n]);
            }
            assert_eq!(whole, split, "{mode:?}");
        }
    }
    #[test]
    fn duration_is_independent_of_pitch() {
        for method in [Method::Beats, Method::Smooth, Method::Grain] {
            for pitch in [0.5, 1.0, 2.0] {
                let mut r = reader(method, 1000.0);
                r.set(Settings {
                    end: 1000.0,
                    speed: 0.5,
                    pitch,
                    method,
                    ..Settings::default()
                });
                for _ in 0..1999 {
                    r.tick(&[0.25; 1000], &[0.25; 1000]);
                }
                assert!(r.active());
                r.tick(&[0.25; 1000], &[0.25; 1000]);
                assert!(!r.active());
            }
        }
    }
    #[test]
    fn hold_reverse_and_short_loops() {
        let s: Vec<_> = (0..100).map(|i| i as f32).collect();
        let mut r = reader(Method::Repitch, 100.0);
        r.set(Settings {
            end: 100.0,
            speed: -1.0,
            ..Settings::default()
        });
        r.start(99.0);
        for expected in (0..100).rev() {
            assert_eq!(r.tick(&s, &s)[0], expected as f32);
        }
        let mut r = reader(Method::Grain, 100.0);
        r.set(Settings {
            end: 100.0,
            speed: 0.0,
            method: Method::Grain,
            window: 32,
            ..Settings::default()
        });
        for _ in 0..1000 {
            assert!(r.tick(&s, &s)[0].is_finite());
        }
        assert_eq!(r.position(), 0.0);
        assert!(r.active());
        for looping in [Loop::Forward, Loop::PingPong] {
            let mut r = reader(Method::Repitch, 100.0);
            r.set(Settings {
                end: 100.0,
                loop_start: 20.0,
                loop_end: 22.0,
                looping,
                pitch: 64.0,
                ..Settings::default()
            });
            for _ in 0..1000 {
                r.tick(&s, &s);
                assert!((0.0..100.0).contains(&r.position()));
            }
        }
    }
    #[test]
    fn no_alloc_edges_and_denormals() {
        let source = [1e-35; 512];
        for method in [
            Method::Repitch,
            Method::Beats,
            Method::Smooth,
            Method::Grain,
        ] {
            let mut r = reader(method, 512.0);
            let mut l = [0.0; 257];
            let mut rr = l;
            assert_no_alloc::assert_no_alloc(|| {
                r.process(&source, &source, &mut [], &mut []);
                r.process(&source, &source, &mut l[..1], &mut rr[..1]);
                r.process(&source, &source, &mut l, &mut rr);
                r.seek(50.0);
                r.process(&[], &[], &mut l, &mut rr);
            });
            assert!(l.iter().all(|v| v.is_finite()));
            assert!(l[64..].iter().all(|v| *v == 0.0));
        }
    }
    #[test]
    fn stretch_pitch_is_measured() {
        let source: Vec<_> = (0..48000)
            .map(|i| (i as f32 * core::f32::consts::TAU * 440.0 / 48000.0).sin())
            .collect();
        let mut r = reader(Method::Smooth, 48000.0);
        r.set(Settings {
            end: 48000.0,
            method: Method::Smooth,
            pitch: 2.0,
            speed: 0.5,
            window: 2048,
            ..Settings::default()
        });
        let (mut l, mut rr) = (vec![0.0; 24000], vec![0.0; 24000]);
        r.process(&source, &source, &mut l, &mut rr);
        let crossings = l[4000..]
            .windows(2)
            .filter(|p| p[0] <= 0.0 && p[1] > 0.0)
            .count();
        let hz = crossings as f32 * 48000.0 / 20000.0;
        assert!((hz - 880.0).abs() < 15.0, "{hz}");
    }

    #[test]
    fn grain_restart_seams_and_continuous_motion_keep_the_level() {
        for start in [0.0, 5.0, 15.0] {
            let mut r = reader(Method::Grain, 100.0);
            r.set(Settings {
                end: 100.0,
                loop_start: start,
                loop_end: 100.0,
                looping: Loop::Forward,
                crossfade: 10.0,
                window: 32,
                method: Method::Grain,
                ..Settings::default()
            });
            let source = [1.0; 100];
            for _ in 0..64 {
                r.tick(&source, &source);
            }
            r.start(start);
            for _ in 0..500 {
                assert!((r.tick(&source, &source)[0] - 1.0).abs() < 1e-6);
            }
        }
        let source: Vec<_> = (0..48000)
            .map(|i| (i as f32 * core::f32::consts::TAU / 48.0).sin())
            .collect();
        let mut r = reader(Method::Repitch, 48000.0);
        let mut s = Settings {
            end: 48000.0,
            loop_start: 0.0,
            loop_end: 24000.0,
            looping: Loop::Forward,
            ..Settings::default()
        };
        r.set(s);
        let mut power = 0.0;
        for i in 0..4800 {
            if i % 32 == 0 {
                s.loop_start += 0.01;
                s.loop_end += 0.01;
                r.set(s);
            }
            let x = r.tick(&source, &source)[0];
            power += x * x;
        }
        assert!((power / 4800.0).sqrt() > 0.69);
    }

    #[test]
    fn beats_preserves_later_attacks_without_moving_the_phrase_clock() {
        let mut source = [0.0; 2048];
        for at in [0, 512, 1024, 1536] {
            source[at] = 1.0;
        }
        let mut r = reader(Method::Beats, 2048.0);
        r.set(Settings {
            end: 2048.0,
            speed: 0.5,
            method: Method::Beats,
            window: 256,
            ..Settings::default()
        });
        r.set_onsets(&[0, 512, 1024, 1536]);
        let mut output = [0.0; 4096];
        let mut right = output;
        r.process(&source, &source, &mut output, &mut right);
        for at in [0, 1024, 2048, 3072] {
            assert!((output[at] - 1.0).abs() < 1e-6, "missing attack {at}");
        }
        assert!(!r.active());
    }
}
