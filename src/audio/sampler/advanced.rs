//! SLICE's note-local playback and effect wiring. Buffers are owned by the
//! bank and allocated during construction, never by a sounding voice.
use super::*;
use crate::dsp::sample_read::{Loop, Method, Settings};

impl SamplerParams {
    pub fn method(&self) -> Method {
        match self.playback.round() as u32 {
            1 => Method::Beats,
            2 => Method::Smooth,
            3 => Method::Grain,
            _ => Method::Repitch,
        }
    }
    pub fn loop_region(
        &self,
        start: f64,
        end: f64,
        sr: f32,
        _pitch: u8,
        bps: f64,
        phase: f64,
        env: f32,
    ) -> (f64, f64) {
        let span = (end - start).max(0.0);
        let fraction = (self.loop_size * (self.env_size * env * 4.0).exp2()).clamp(0.00001, 1.0);
        let hz = 440.0 * 2f64.powf((f64::from(self.root) - 69.0) / 12.0);
        let length = match self.loop_units.round() as u32 {
            1 => 2f64.powf(f64::from(fraction) * 8.0 - 6.0) / bps.max(1e-9),
            2 => f64::from(sr) / hz.max(1.0) * 2f64.powf((f64::from(fraction) - 0.5) * 4.0),
            _ => span * f64::from(fraction),
        }
        .clamp(2.0, span.max(2.0));
        let motion = match self.motion_mode.round() as u32 {
            0 => phase.rem_euclid(1.0),
            1 => 1.0 - (phase.rem_euclid(2.0) - 1.0).abs(),
            _ => phase.clamp(0.0, 1.0),
        };
        let position =
            f64::from(self.loop_start + self.env_position * env) + motion * f64::from(self.travel);
        // Position is measured in the source span, so resizing keeps the
        // left anchor still. Near the tail, size is clipped, never shifted.
        let a = (start + span * position.clamp(0.0, 1.0)).min((end - 2.0).max(start));
        (a, (a + length).min(end))
    }
}

impl SamplerVoices {
    /// Instrument telemetry: the newest sounding head, its slip clock and
    /// the active-voice count. These travel in the existing fixed readout.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        let newest = self
            .voices
            .iter()
            .filter(|v| v.active)
            .max_by_key(|v| v.age);
        let frames = self.material.frames.max(1) as f32;
        crate::audio::graph::Readout {
            bands: newest.map_or([0.0; 3], |v| {
                [
                    v.pos as f32 / frames,
                    v.reader.slip_position() as f32 / frames,
                    self.voices.iter().filter(|v| v.active).count() as f32,
                ]
            }),
            ..Default::default()
        }
    }
    pub fn set_clock(&mut self, beats_per_sample: f64) {
        if beats_per_sample.is_finite() && beats_per_sample > 0.0 {
            self.beats_per_sample = beats_per_sample;
        }
    }

    pub(super) fn prepare_note(v: &mut Voice, p: SamplerParams, sr: f32) {
        v.sample_rate = sr;
        v.amp.prepare(
            sr,
            p.amp_attack_ms,
            p.amp_decay_ms,
            p.amp_sustain,
            p.amp_release_ms,
        );
        v.moden.prepare(
            sr,
            p.mod_attack_ms,
            p.mod_decay_ms,
            p.mod_sustain,
            p.mod_release_ms,
        );
        v.crush_l.set_bits(p.bits);
        v.crush_r.set_bits(p.bits);
        v.filter2_l.reset();
        v.filter2_r.reset();
        v.attack_split.prepare(sr, 20.0);
        v.attack_split.reset();
        let max = (sr * 0.5) as usize;
        v.comb_l.prepare(sr, max, p.comb_damp);
        v.comb_r.prepare(sr, max, p.comb_damp);
        v.comb_l.reset();
        v.comb_r.reset();
        v.last_l = 0.0;
        v.last_r = 0.0;
    }

    fn needs_reader(p: &SamplerParams) -> bool {
        p.playback != 0.0
            || p.loop_mode != 0.0
            || p.time != 100.0
            || p.speed != 100.0
            || p.loop_size != 1.0
            || p.scan != 0.0
            || p.loop_exit != 0.0
            || p.loop_units != 0.0
            || p.env_position != 0.0
            || p.env_size != 0.0
            || p.fit_beats > 0.0
            || p.source_beats > 0.0
            || p.slip > 0.0
            || p.loop_fade != 0.15
            || (p.slicing() && p.loop_mode != 0.0)
    }

    pub(super) fn start_reader(v: &mut Voice, p: SamplerParams, sr: f32, bps: f64, frames: f64) {
        v.source_frames = frames;
        let mut seed = (p.seed as u64) ^ v.age.wrapping_mul(0x9e3779b97f4a7c15);
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let random = (seed.wrapping_mul(2685821657736338717) >> 40) as f32 / 16_777_215.0;
        v.pitch_drift = (random * 2.0 - 1.0) * p.pitch_jitter / 100.0;
        v.motion_phase = f64::from(random * p.spread);
        v.pos = (v.pos + (v.span_hi - v.span_lo) * f64::from(p.start_jitter * random))
            .clamp(v.span_lo, (v.span_hi - 1.0).max(v.span_lo));
        v.use_reader = Self::needs_reader(&p);
        let settings = Self::reader_settings(v, &p, sr, bps, frames);
        v.reader.prepare(settings);
        if settings.speed < 0.0 && v.dir > 0.0 {
            v.pos = (v.span_hi - 1.0).max(v.span_lo);
        } else if settings.speed > 0.0 && v.dir < 0.0 {
            v.pos = v.span_lo;
        }
        if p.speed == 0.0 && settings.looping != Loop::Off {
            v.pos = settings.loop_start;
        }
        v.reader.start(v.pos);
    }

    fn reader_settings(v: &Voice, p: &SamplerParams, sr: f32, bps: f64, frames: f64) -> Settings {
        let (a, b) = p.loop_region(
            v.span_lo,
            v.span_hi,
            sr,
            v.pitch,
            bps,
            v.motion_phase,
            v.moden.current(),
        );
        let tempo = if p.fit_beats > 0.0 {
            (v.span_hi - v.span_lo) * bps / f64::from(p.fit_beats)
        } else if p.source_beats > 0.0 {
            frames * bps / f64::from(p.source_beats)
        } else {
            1.0
        };
        Settings {
            start: v.span_lo,
            end: v.span_hi,
            loop_start: a,
            loop_end: b,
            looping: if v.released_tail {
                Loop::Off
            } else {
                match p.loop_mode.round() as u32 {
                    1 => Loop::Forward,
                    2 => Loop::PingPong,
                    _ => Loop::Off,
                }
            },
            crossfade: if p.hard >= 0.5 {
                0.0
            } else {
                (b - a) * f64::from(p.loop_fade) + f64::from(p.loop_xfade_ms * sr * 0.001)
            },
            pitch: v.inc,
            speed: tempo * 100.0 / f64::from(p.time.max(1.0)) * f64::from(p.speed) / 100.0
                * if p.reversed() { -1.0 } else { 1.0 },
            slip_speed: tempo,
            window: (p.window_ms * sr * 0.001).round() as usize,
            method: p.method(),
            transient: p.transient,
            hard: p.hard >= 0.5,
        }
    }

    pub(super) fn update_reader(v: &mut Voice, p: &SamplerParams, sr: f32, bps: f64) {
        let needed = Self::needs_reader(p);
        if needed && !v.use_reader {
            v.reader
                .prepare(Self::reader_settings(v, p, sr, bps, v.source_frames));
            v.reader.start(v.pos);
        }
        // Once enabled, retain the reader so releasing a freeze can rejoin
        // its slip clock without throwing away the head.
        v.use_reader |= needed;
        if !v.use_reader {
            return;
        }
        v.motion_phase += f64::from(p.scan) * CHUNK as f64 / f64::from(sr);
        let mut settings = Self::reader_settings(v, p, sr, bps, v.source_frames);
        if v.released_tail && settings.speed == 0.0 {
            settings.speed = 1.0;
        }
        let rejoin = p.slip >= 0.5
            && v.reader.speed() != settings.speed
            && settings.speed > 0.0
            && v.reader.speed() <= 0.0;
        v.reader.set(settings);
        if rejoin {
            v.reader.rejoin();
        }
        v.looping = if settings.looping == Loop::Off {
            Looping::Off
        } else {
            Looping::Forward
        };
    }

    pub(super) fn finish_voice(
        v: &mut Voice,
        p: &SamplerParams,
        scratch: &mut Scratch,
        n: usize,
        buffers: &mut (Vec<f32>, Vec<f32>),
    ) {
        let (Some(l), Some(r)) = (scratch.l.get_mut(..n), scratch.r.get_mut(..n)) else {
            return;
        };
        if p.attack_shape != 0.0 {
            let mut weight = [0.0; CHUNK];
            v.attack_split.process(l, &mut weight[..n]);
            for ((a, b), w) in l.iter_mut().zip(r.iter_mut()).zip(weight) {
                let g = if p.attack_shape > 0.0 {
                    1.0 + p.attack_shape * w * 3.0
                } else {
                    1.0 + p.attack_shape * w
                };
                *a *= g;
                *b *= g;
            }
        }
        if p.comb_mix > 0.0 {
            let hz =
                (440.0 * 2f32.powf((f32::from(v.pitch) - 69.0 + p.tune) / 12.0) * p.comb_focus)
                    .max(2.0);
            let delay = v.sample_rate / hz;
            v.comb_l.set_delay(delay);
            v.comb_r.set_delay(delay);
            v.comb_l.set_damp(v.sample_rate, p.comb_damp);
            v.comb_r.set_damp(v.sample_rate, p.comb_damp);
            v.comb_l.set_feedback(p.comb_feed);
            v.comb_r.set_feedback(p.comb_feed);
            let dry_l: [f32; CHUNK] = core::array::from_fn(|i| l.get(i).copied().unwrap_or(0.0));
            let dry_r: [f32; CHUNK] = core::array::from_fn(|i| r.get(i).copied().unwrap_or(0.0));
            v.comb_l.process(l, &mut buffers.0);
            v.comb_r.process(r, &mut buffers.1);
            for (((a, b), dry_a), dry_b) in l.iter_mut().zip(r.iter_mut()).zip(dry_l).zip(dry_r) {
                *a = dry_a + *a * p.comb_mix;
                *b = dry_b + *b * p.comb_mix;
            }
        }
        let vel_gain = 1.0 - p.velocity * (1.0 - v.vel);
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let env = scratch.env.get(i).copied().unwrap_or(0.0);
            let edge = scratch.edge.get(i).copied().unwrap_or(0.0);
            let gain = env * vel_gain * edge;
            *a *= gain;
            *b *= gain;
            if v.steal_left > 0 {
                let t = v.steal_left as f32 / 64.0;
                *a = *a * (1.0 - t) + v.steal_l * t;
                *b = *b * (1.0 - t) + v.steal_r * t;
                v.steal_left -= 1;
            }
            v.last_l = *a;
            v.last_r = *b;
        }
    }
}
