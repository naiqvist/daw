//! First executable spectral-instrument voice, independent of the native UI.
//! Shared harmonic performance plane, eight lane-major note voices; prepared
//! instrument-owned audio routing follows their sum. No embedded agent runtime.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use super::spectral_fx;
use crate::dsp::{
    LANES,
    adsr::LaneAdsr,
    harmonic::{HarmonicOsc, PARTIALS, SINE_SIZE},
    noise::LaneWhiteNoise,
    pitch_gesture::{Gesture, GestureConfig, PitchGesture},
    ramps::LinearRamp,
};
use crate::params::spectral as p;

/// Dense, version-stable parameter ids live in params::spectral. Nested arrays
/// serialize without a nonstandard large-array dependency; no lane voice AoS.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SpectralParams {
    pub globals: [f32; 13],
    pub amplitudes: [[f32; 16]; 8],
    pub phases: [[f32; 16]; 8],
    pub performance: [f32; p::EXTRA_COUNT],
}
impl Default for SpectralParams {
    fn default() -> Self {
        let mut params = Self {
            globals: [0.0; 13],
            amplitudes: [[0.0; 16]; 8],
            phases: [[0.0; 16]; 8],
            performance: [0.0; p::EXTRA_COUNT],
        };
        for row in p::TABLE {
            params.set(row.id, row.default);
        }
        params
    }
}
impl SpectralParams {
    pub fn get(&self, id: u32) -> Option<f32> {
        if let Some((h, phase)) = p::harmonic(id) {
            Some(if phase {
                self.phases[h / 16][h % 16]
            } else {
                self.amplitudes[h / 16][h % 16]
            })
        } else if id >= p::EXTRA_BASE {
            self.performance.get((id - p::EXTRA_BASE) as usize).copied()
        } else {
            self.globals.get(id as usize).copied()
        }
    }
    pub fn set(&mut self, id: u32, value: f32) {
        let Some(row) = p::TABLE.get(id as usize) else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(row.min, row.max);
        if let Some((h, phase)) = p::harmonic(id) {
            if phase {
                self.phases[h / 16][h % 16] = value;
            } else {
                self.amplitudes[h / 16][h % 16] = value;
            }
        } else if id >= p::EXTRA_BASE {
            if let Some(slot) = self.performance.get_mut((id - p::EXTRA_BASE) as usize) {
                *slot = if p::discrete(id) {
                    value.round()
                } else {
                    value
                };
            }
        } else if let Some(slot) = self.globals.get_mut(id as usize) {
            *slot = value;
        }
    }
}

/// Standalone prototype artifact; native Device/Sound migration comes later.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpectralPatch {
    pub version: u32,
    pub params: SpectralParams,
    pub fx: spectral_fx::Patch,
    #[serde(default)]
    pub modulation: super::spectral_mod::Patch,
}

/// Non-scalar instrument state stored with Device/Sound. Numeric settings stay
/// in the device's canonical overrides so UI, locks and playback cannot drift.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Routing {
    pub fx: spectral_fx::Patch,
    pub modulation: super::spectral_mod::Patch,
}

impl SpectralPatch {
    pub fn from_device(device: &crate::sequencing::Device) -> Self {
        let mut patch = Self::default();
        for (id, value) in &device.overrides {
            patch.params.set(*id, *value);
        }
        if let Some(routing) = &device.spectral {
            patch.fx = routing.fx.clone();
            patch.modulation = routing.modulation.clone();
        }
        patch
    }
    pub fn install(&self, device: &mut crate::sequencing::Device) {
        device.overrides.clear();
        for row in p::TABLE {
            if let Some(value) = self.params.get(row.id) {
                device.set(row.id, value);
            }
        }
        device.spectral = Some(Box::new(Routing {
            fx: self.fx.clone(),
            modulation: self.modulation.clone(),
        }));
    }
}
impl Default for SpectralPatch {
    fn default() -> Self {
        Self {
            version: 1,
            params: SpectralParams::default(),
            fx: spectral_fx::Patch::default(),
            modulation: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SpectralVoices {
    level: LinearRamp,
    base: SpectralParams,
    live: SpectralParams,
    sr: f32,
    osc: HarmonicOsc,
    envelope: LaneAdsr,
    sine: Vec<f32>,
    amps: [LinearRamp; PARTIALS],
    phases: [LinearRamp; PARTIALS],
    shift: LinearRamp,
    spectrum: [f32; PARTIALS],
    angles: [f32; PARTIALS],
    pitch: [u8; LANES],
    velocity: [f32; LANES],
    age: [u64; LANES],
    held: [bool; LANES],
    motion: PitchGesture,
    // Shared pitch plane, intentionally distinct from the shared bin gesture.
    pitch_motion: PitchGesture,
    mono_pitch: LinearRamp,
    noise: LaneWhiteNoise,
    noise_mix: LinearRamp,
    fx: spectral_fx::Prepared,
    modulation: super::spectral_mod::Prepared,
}
impl SpectralVoices {
    pub fn reset(&mut self) {
        self.all_sound_off();
    }
    /// Green-only constructor. Invalid serialized params refuse, not poison a
    /// callback. Topology/modulation is compiled here, never during playback.
    pub fn prepare(
        sr: f32,
        block: usize,
        patch: &SpectralPatch,
        workspace_bytes: usize,
    ) -> Result<Self, String> {
        if patch.version != 1 {
            return Err("unsupported spectral instrument version".into());
        }
        for row in p::TABLE {
            let v = patch
                .params
                .get(row.id)
                .ok_or("missing spectral parameter")?;
            if !v.is_finite() || !(row.min..=row.max).contains(&v) {
                return Err(format!("invalid {}", row.name));
            }
        }
        let fx = patch.fx.prepare(sr, block, workspace_bytes)?;
        let modulation = patch.modulation.prepare(sr, &patch.fx)?;
        let mut sine = vec![0.0; SINE_SIZE + 1];
        crate::dsp::harmonic::build_sine(&mut sine);
        let mut osc = HarmonicOsc::new();
        osc.prepare(sr);
        let mut motion = PitchGesture::default();
        motion.prepare(sr);
        let mut pitch_motion = PitchGesture::default();
        pitch_motion.prepare(sr);
        let mut this = Self {
            level: LinearRamp::new(),
            base: patch.params,
            live: patch.params,
            sr,
            osc,
            envelope: LaneAdsr::new(),
            sine,
            amps: [LinearRamp::new(); PARTIALS],
            phases: [LinearRamp::new(); PARTIALS],
            shift: LinearRamp::new(),
            spectrum: [0.0; PARTIALS],
            angles: [0.0; PARTIALS],
            pitch: [0; LANES],
            velocity: [0.0; LANES],
            age: [0; LANES],
            held: [false; LANES],
            motion,
            pitch_motion,
            mono_pitch: LinearRamp::new(),
            noise: LaneWhiteNoise::new(),
            noise_mix: LinearRamp::new(),
            fx,
            modulation,
        };
        this.restore_ramps();
        this.configure_envelope();
        Ok(this)
    }
    fn configure_envelope(&mut self) {
        let g = &self.live.globals;
        self.envelope.prepare(
            self.sr,
            g[p::ATTACK as usize],
            g[p::DECAY as usize],
            g[p::SUSTAIN as usize],
            g[p::RELEASE as usize],
        );
    }
    fn restore_ramps(&mut self) {
        self.level.set_now(self.live.globals[p::LEVEL as usize]);
        for h in 0..PARTIALS {
            self.amps[h].set_now(self.live.amplitudes[h / 16][h % 16]);
            self.phases[h].set_now(self.live.phases[h / 16][h % 16]);
        }
        self.shift.set_now(self.live.globals[p::SHIFT as usize]);
        self.noise_mix.set_now(self.performance(p::NOISE));
        for index in 0..8 {
            self.modulation
                .set_macro(index, self.performance(p::MACRO_1 + index as u32), 0);
        }
        self.configure_vibrato();
    }
    fn performance(&self, id: u32) -> f32 {
        self.live.get(id).unwrap_or(0.0)
    }
    fn configure_vibrato(&mut self) {
        self.pitch_motion.vibrato(
            self.performance(p::VIBRATO_SPEED),
            self.performance(p::VIBRATO_DEPTH),
        );
    }
    fn update(&mut self, id: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        let previous = self.live.get(id);
        self.live.set(id, value);
        let samples = (self.live.globals[p::MORPH as usize] * 0.001 * self.sr).round() as u32;
        if let Some((h, phase)) = p::harmonic(id) {
            if phase {
                let current = self.phases[h].current().rem_euclid(360.0);
                let target = self.live.phases[h / 16][h % 16];
                let delta = (target - current + 180.0).rem_euclid(360.0) - 180.0;
                self.phases[h].set_now(current);
                self.phases[h].glide(current + delta, samples);
            } else {
                self.amps[h].glide(self.live.amplitudes[h / 16][h % 16], samples);
            }
        } else if id == p::SHIFT {
            self.shift
                .glide(self.live.globals[p::SHIFT as usize], samples);
        } else if id == p::LEVEL {
            self.level.glide(
                self.live.globals[p::LEVEL as usize],
                (self.sr * 0.01) as u32,
            );
        } else if id <= p::RELEASE {
            self.configure_envelope();
        } else if id == p::NOISE {
            self.noise_mix
                .glide(self.performance(id), (self.sr * 0.001) as u32);
        } else if (p::MACRO_1..=p::MACRO_8).contains(&id) {
            self.modulation.set_macro(
                (id - p::MACRO_1) as usize,
                self.performance(id),
                (self.sr * 0.002) as u32,
            );
        } else if matches!(id, p::VIBRATO_SPEED | p::VIBRATO_DEPTH) {
            self.configure_vibrato();
        } else if id == p::MONO && previous != self.live.get(id) {
            // A mode change closes gates without discarding reverb/release tails.
            self.release_all();
        }
    }
    pub fn set_param(&mut self, id: u32, value: f32) {
        self.base.set(id, value);
        self.update(id, value);
    }
    pub fn plock(&mut self, id: u32, value: Option<f32>) {
        if let Some(v) = value.or_else(|| self.base.get(id)) {
            self.update(id, v);
        }
    }
    pub fn plock_glide(&mut self, id: u32, alpha: f32) {
        if !alpha.is_finite() {
            return;
        }
        if let (Some(current), Some(base)) = (self.live.get(id), self.base.get(id)) {
            self.update(id, current + (base - current) * alpha.clamp(0.0, 1.0));
        }
    }
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        if vel == 0 {
            self.note_off(pitch);
            return;
        }
        let mono = self.performance(p::MONO) >= 0.5;
        let legato = mono && self.held[0];
        // Last-note priority, like Acid: an old note-off cannot close the new gate.
        // Mono uses one lane. Poly prefers idle, then oldest release/held.
        let lane = if mono {
            0
        } else {
            (0..LANES)
                .min_by_key(|&i| (self.envelope.active(i), self.held[i], self.age[i]))
                .unwrap_or(0)
        };
        if mono {
            if legato {
                self.mono_pitch.glide(
                    pitch as f32,
                    (self.performance(p::GLIDE) * 0.001 * self.sr).round() as u32,
                );
            } else {
                self.mono_pitch.set_now(pitch as f32);
            }
        }
        self.pitch[lane] = pitch;
        self.velocity[lane] = vel.min(127) as f32 / 127.0;
        self.age[lane] = age;
        self.held[lane] = true;
        let hz =
            440.0 * 2.0f32.powf((pitch as f32 + self.live.globals[p::TUNE as usize] - 69.0) / 12.0);
        if !legato {
            self.osc.start(lane, hz);
            // Fresh strikes retrigger; connected mono notes preserve the gate
            // and oscillator phase. Percussion recipes leave room for release.
            self.envelope.gate_on(lane);
            self.modulation.note_on(lane, self.velocity[lane]);
        }
        self.pitch_motion.trigger(
            GestureConfig {
                kind: Gesture::from_index(self.performance(p::PITCH_ORNAMENT).round() as u32),
                time_ms: self.performance(p::PITCH_TIME),
                speed_hz: self.performance(p::PITCH_SPEED),
                from_cents: self.performance(p::PITCH_FROM) * 100.0,
                other_cents: self.performance(p::PITCH_OTHER) * 100.0,
            },
            Some(self.performance(p::PITCH_FROM)),
        );
        // Glide owns the connecting path; shared ornaments add intentional
        // grace/oscillation around it. No extra oscillator/instance is spawned.
        let g = &self.live.globals;
        let config = GestureConfig {
            kind: Gesture::from_index(g[p::ORNAMENT as usize].round() as u32),
            time_ms: g[p::ORNAMENT_TIME as usize],
            speed_hz: g[p::ORNAMENT_SPEED as usize],
            from_cents: g[p::ORNAMENT_FROM as usize] * 100.0,
            other_cents: g[p::ORNAMENT_OTHER as usize] * 100.0,
        };
        // Reuse the gesture's trajectory in BIN units, not as a pitch bend.
        self.motion
            .trigger(config, Some(g[p::ORNAMENT_FROM as usize]));
    }
    pub fn note_off(&mut self, pitch: u8) {
        for lane in 0..LANES {
            if self.held[lane] && self.pitch[lane] == pitch {
                self.held[lane] = false;
                self.envelope.gate_off(lane);
                self.modulation.note_off(lane);
            }
        }
    }
    pub fn release_all(&mut self) {
        for lane in 0..LANES {
            self.held[lane] = false;
            self.envelope.gate_off(lane);
            self.modulation.note_off(lane);
        }
    }
    pub fn all_sound_off(&mut self) {
        self.osc.reset();
        self.envelope.reset();
        self.motion.reset();
        self.pitch_motion.reset();
        self.mono_pitch.reset();
        self.noise.reset();
        self.fx.reset();
        self.modulation.reset();
        self.held.fill(false);
        self.velocity.fill(0.0);
        self.live = self.base;
        self.restore_ramps();
        self.configure_envelope();
    }
    pub fn fx_faulted(&self) -> bool {
        self.fx.faulted()
    }

    pub fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut crate::audio::graph::Ramp) {
        self.render_audio(out);
        for sample in out {
            *sample *= gain.next();
        }
    }

    /// Red-zone mono renderer. Spectrum morphs are shared across active voices;
    /// pitch and amplitude modulation is lane-local. A stopped envelope silences
    /// its oscillator, but the downstream FX tail is still rendered.
    pub fn render_audio(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            let mut value = [0.0];
            for h in 0..PARTIALS {
                self.amps[h].process(&mut value);
                self.spectrum[h] = value[0];
                self.phases[h].process(&mut value);
                self.angles[h] = value[0];
            }
            self.shift.process(&mut value);
            let shift = value[0];
            self.motion.process(&mut value);
            let ornament = value[0];
            self.pitch_motion.process(&mut value);
            let pitch_ornament = value[0];
            self.mono_pitch.process(&mut value);
            let mono_pitch = value[0];
            let controls = self
                .modulation
                .tick(&mut self.spectrum, &mut self.angles, &mut self.fx);
            let mut tone = [[0.0; LANES]];
            let mut envelope = [[0.0; LANES]];
            for lane in 0..LANES {
                if self.envelope.active(lane) {
                    let pitch = (if lane == 0 && self.performance(p::MONO) >= 0.5 {
                        mono_pitch
                    } else {
                        self.pitch[lane] as f32
                    }) + self.live.globals[p::TUNE as usize]
                        + pitch_ornament
                        + controls.pitch[lane];
                    self.osc
                        .set_frequency(lane, 440.0 * 2.0f32.powf((pitch - 69.0) / 12.0));
                } else {
                    self.osc.stop(lane);
                }
            }
            self.osc.process(
                &mut tone,
                &self.spectrum,
                &self.angles,
                shift + ornament + controls.shift,
                &self.sine,
            );
            self.envelope.process(&mut envelope);
            let mut noise = [[0.0; LANES]];
            self.noise.process(&mut noise);
            self.noise_mix.process(&mut value);
            let noise_mix = value[0];
            *sample = 0.0;
            for lane in 0..LANES {
                *sample += (tone[0][lane] * (1.0 - noise_mix) + noise[0][lane] * noise_mix)
                    * envelope[0][lane]
                    * self.velocity[lane]
                    * controls.amplitude[lane];
            }
            self.level.process(&mut value);
            *sample *= value[0] * 0.25;
            // Per-sample dispatch keeps all eligible FX modulation sample-exact.
            self.fx.process(core::slice::from_mut(sample));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn voice() -> SpectralVoices {
        SpectralVoices::prepare(48_000.0, 256, &SpectralPatch::default(), 4_000_000).unwrap()
    }
    #[test]
    fn spectral_mono_glides_without_retrigger_and_old_note_off_does_not_kill_it() {
        let mut a = voice();
        a.set_param(p::MONO, 1.0);
        a.set_param(p::GLIDE, 100.0);
        a.note_on(48, 100, 0);
        a.render_audio(&mut [0.0; 2400]);
        let mut before = [[0.0; LANES]];
        a.envelope.process(&mut before);
        a.note_on(60, 100, 1);
        let mut after = [[0.0; LANES]];
        a.envelope.process(&mut after);
        assert!((before[0][0] - after[0][0]).abs() < 0.01);
        a.note_off(48);
        assert!(a.held[0]);
        a.render_audio(&mut [0.0; 2400]);
        assert!((a.mono_pitch.current() - 54.0).abs() < 0.02);
        a.render_audio(&mut [0.0; 2400]);
        assert_eq!(a.mono_pitch.current(), 60.0);
        assert_eq!(a.held.iter().filter(|held| **held).count(), 1);
        a.note_off(60);
        assert!(!a.held[0]);
        a.set_param(p::MONO, 0.0);
        for pitch in [48, 55, 59, 64] {
            a.note_on(pitch, 80, pitch as u64);
        }
        assert_eq!(a.held.iter().filter(|held| **held).count(), 4);
    }
    #[test]
    fn spectral_performance_noise_pitch_and_macro_reset_split_no_alloc() {
        let mut a = voice();
        for (id, v) in [
            (p::NOISE, 0.65),
            (p::PITCH_ORNAMENT, 3.0),
            (p::PITCH_FROM, 4.0),
            (p::VIBRATO_DEPTH, 25.0),
            (p::MACRO_1, 0.7),
        ] {
            a.set_param(id, v);
        }
        a.reset();
        let mut b = a.clone();
        a.note_on(40, 100, 0);
        b.note_on(40, 100, 0);
        let mut full = [0.0; 4097];
        let mut split = full;
        assert_no_alloc::assert_no_alloc(|| {
            a.render_audio(&mut full);
            b.render_audio(&mut []);
            b.render_audio(&mut split[..1]);
            b.render_audio(&mut split[1..2047]);
            b.render_audio(&mut split[2047..]);
        });
        assert_eq!(full, split);
        assert!(full.iter().all(|v| v.is_finite()));
        assert!(full.iter().any(|v| v.abs() > 0.001));
        a.reset();
        a.render_audio(&mut split);
        assert_eq!(split, [0.0; 4097]);
        a.reset();
        a.note_on(40, 100, 0);
        a.render_audio(&mut split);
        assert_eq!(full, split);
        // Older patches omit performance entirely, retaining audible defaults.
        let older: SpectralParams = ron::from_str("()").unwrap();
        assert_eq!(older, SpectralParams::default());
    }
    #[test]
    fn spectral_params_dense_validated_and_patch_roundtrip() {
        let patch = SpectralPatch::default();
        assert_eq!(p::TABLE.len(), p::COUNT);
        assert_eq!(p::LABELS.len(), p::COUNT);
        for (i, row) in p::TABLE.iter().enumerate() {
            assert_eq!(row.id, i as u32);
            assert_eq!(patch.params.get(row.id), Some(row.default));
        }
        let text = ron::to_string(&patch).unwrap();
        assert_eq!(patch, ron::from_str::<SpectralPatch>(&text).unwrap());
        let mut broken = patch.clone();
        broken.params.globals[0] = f32::NAN;
        assert!(SpectralVoices::prepare(48_000.0, 256, &broken, 4_000_000).is_err());
        assert!(SpectralVoices::prepare(48_000.0, 0, &patch, 4_000_000).is_err());
    }
    #[test]
    fn spectral_voice_split_blocks_polyphony_locks_and_no_allocation() {
        let mut a = voice();
        let mut b = voice();
        for (i, note) in [48, 55, 59, 64].into_iter().enumerate() {
            a.note_on(note, 90, i as u64);
            b.note_on(note, 90, i as u64);
        }
        let mut full = [0.0; 2049];
        let mut split = full;
        assert_no_alloc::assert_no_alloc(|| {
            a.render_audio(&mut full[..500]);
            b.render_audio(&mut split[..1]);
            b.render_audio(&mut split[1..500]);
            a.plock(p::phase(1), Some(150.0));
            b.plock(p::phase(1), Some(150.0));
            a.plock(p::amp(7), Some(0.8));
            b.plock(p::amp(7), Some(0.8));
            a.render_audio(&mut full[500..1500]);
            b.render_audio(&mut []);
            b.render_audio(&mut split[500..1301]);
            b.render_audio(&mut split[1301..1500]);
            a.release_all();
            b.release_all();
            a.plock(p::amp(7), None);
            b.plock(p::amp(7), None);
            a.render_audio(&mut full[1500..]);
            b.render_audio(&mut split[1500..]);
        });
        assert_eq!(full, split);
        assert!(full.iter().any(|v| v.abs() > 0.001));
        assert!(full.iter().all(|v| v.is_finite() && v.abs() < 1.0));
        assert_no_alloc::assert_no_alloc(|| {
            a.all_sound_off();
            a.render_audio(&mut full);
        });
        assert_eq!(full, [0.0; 2049]);
    }
    #[test]
    fn phase_morph_takes_short_path_and_live_base_restores() {
        let mut a = voice();
        a.set_param(p::MORPH, 0.0);
        a.set_param(p::phase(0), 170.0);
        a.set_param(p::MORPH, 100.0);
        a.plock(p::phase(0), Some(-170.0));
        let mut samples = [0.0; 2400];
        a.phases[0].process(&mut samples);
        assert!((samples[2399] - 180.0).abs() < 0.05);
        a.phases[0].process(&mut samples);
        assert!((samples[2399] - 190.0).abs() < 0.05);
        a.set_param(p::phase(0), 120.0);
        a.plock(p::phase(0), Some(-70.0));
        a.plock(p::phase(0), None);
        a.phases[0].process(&mut [0.0; 4800]);
        assert!((a.phases[0].current().rem_euclid(360.0) - 120.0).abs() < 0.01);
    }
    #[test]
    fn all_six_spectral_ornaments_are_distinct_finite_and_leave_note_identity_alone() {
        let mut renders = Vec::new();
        for ornament in 0..=6 {
            let mut a = voice();
            a.set_param(p::ORNAMENT, ornament as f32);
            a.set_param(p::ORNAMENT_SPEED, 7.0);
            a.note_on(48, 100, 0);
            let mut samples = vec![0.0; 24_000];
            a.render_audio(&mut samples);
            assert_eq!(a.pitch[0], 48);
            assert!(samples.iter().all(|v| v.is_finite()));
            for previous in &renders {
                assert_ne!(&samples, previous);
            }
            renders.push(samples);
        }
    }
}
