//! PUMP: the transport-locked control-voltage ducker.
//!
//! This is the sidechain sound without a sidechain input: at every chosen
//! grid line the console's virtual VCA closes to DEPTH, holds there, then
//! opens along a profile between a hard switched control voltage and a
//! raised-cosine cam. The clock is the transport clock, not wall time, so an
//! offline bounce, a live callback, and a split transport segment produce
//! the same envelope.
//!
//! DIVISION runs from whole notes through sixteenths. DEPTH at zero is an
//! exact wire. When the transport is stopped (or hands over no usable tempo)
//! the VCA is open, which keeps input monitoring available. Stereo is linked:
//! both sides receive the same gain and the image cannot wander.
//!
//! State: dense parameter table plus three scalar meter values. Per-sample
//! cost while active: one f64 phase reduction, one cosine, a handful of
//! scalar arithmetic, and one multiply per channel. Denormal-safe: relies on
//! the engine's FTZ mode; silence remains exact silence. In-place safe: yes.
//! Latency: 0 samples. The process path allocates nothing.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::{SectionParams, pump_curve};
use crate::params::console::pump as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub division: usize,
    pub depth: f32,
    pub shape: f32,
    pub hold: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Pump.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            division: (clamp(p::DIVISION).round().max(0.0) as usize)
                .min(pump_curve::DIVISION_BEATS.len() - 1),
            depth: clamp(p::DEPTH) / 100.0,
            shape: clamp(p::SHAPE) / 100.0,
            // The face's HOLD travel occupies at most 60% of a step. A
            // recovery always remains, even with the control at its end.
            hold: pump_curve::hold_share(clamp(p::HOLD)),
        }
    }

    pub fn is_wire(&self) -> bool {
        self.depth == 0.0
    }

    pub fn division_beats(&self) -> f64 {
        pump_curve::division_beats(self.division)
    }
}

/// The VCA gain at an absolute transport beat.
///
/// This is also the law drawn by PUMP's face: floor on the grid, HOLD at the
/// floor, then a blend from a hard cam to a raised cosine. It is stateless so
/// a seek lands on the right gain immediately.
#[inline(always)]
fn gain_at(beat: f64, settings: Settings) -> f32 {
    if settings.is_wire() || !beat.is_finite() {
        return 1.0;
    }
    let phase = (beat / settings.division_beats()).rem_euclid(1.0) as f32;
    pump_curve::gain_at_phase(phase, settings.depth, settings.shape, settings.hold)
}

pub struct PumpCore {
    params: SectionParams,
    settings: Settings,
    gain: f32,
    reduction_db: f32,
    level_db: f32,
    phase: f32,
}

impl PumpCore {
    pub fn new(params: &SectionParams, _sample_rate: f32, _block: usize) -> Self {
        Self {
            params: params.dense(),
            settings: Settings::of(params),
            gain: 1.0,
            reduction_db: 0.0,
            level_db: -120.0,
            phase: 0.0,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }
}

impl SectionCore for PumpCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        self.settings = Settings::of(&self.params);
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.reduction_db = 0.0;
        self.level_db = -120.0;
        self.phase = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let active = clock.playing
            && clock.beats_per_sample.is_finite()
            && clock.beats_per_sample > 0.0
            && clock.beat.is_finite()
            && !self.settings.is_wire();
        let mut peak = 0.0f32;
        let mut least_gain = 1.0f32;

        if active {
            let step = self.settings.division_beats();
            if stereo {
                for (i, (left, right)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                    let beat = clock.beat + i as f64 * clock.beats_per_sample;
                    let gain = gain_at(beat, self.settings);
                    *left *= gain;
                    *right *= gain;
                    peak = peak.max(left.abs()).max(right.abs());
                    least_gain = least_gain.min(gain);
                    self.gain = gain;
                    self.phase = (beat / step).rem_euclid(1.0) as f32;
                }
            } else {
                for (i, sample) in l.iter_mut().enumerate() {
                    let beat = clock.beat + i as f64 * clock.beats_per_sample;
                    let gain = gain_at(beat, self.settings);
                    *sample *= gain;
                    peak = peak.max(sample.abs());
                    least_gain = least_gain.min(gain);
                    self.gain = gain;
                    self.phase = (beat / step).rem_euclid(1.0) as f32;
                }
            }
        } else {
            self.gain = 1.0;
            self.phase = 0.0;
            peak = l.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            if stereo {
                peak = r
                    .iter()
                    .take(n)
                    .fold(peak, |peak, sample| peak.max(sample.abs()));
            }
        }

        self.reduction_db = if least_gain <= 1e-6 {
            -120.0
        } else {
            20.0 * least_gain.log10()
        };
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.reduction_db,
            // Live mechanism readings, not duplicated settings: the VCA's
            // present gain and its position inside the current step.
            bands: [self.gain, self.phase, 0.0],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn core_with(edits: &[(u32, f32)]) -> PumpCore {
        let mut params = SectionParams::of(SectionKind::Pump);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        PumpCore::new(&params, FS, BLOCK)
    }

    fn clock_at(beat: f64, playing: bool) -> Clock {
        Clock {
            playing,
            beat,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    /// The desk registry must reach this core rather than quietly returning
    /// the historical placeholder. At PUMP's useful resting depth, a sample
    /// exactly on the grid is cut in half.
    #[test]
    fn the_registry_reaches_the_real_pump() {
        let params = SectionParams::of(SectionKind::Pump);
        let mut core = crate::audio::console::core_of(&params, FS, BLOCK);
        let mut sample = [1.0f32];
        core.process(&mut sample, &mut [], &clock_at(0.0, true));
        assert_eq!(sample, [0.5]);
        assert!((core.readout().reduction_db + 6.020_600_3).abs() < 1e-5);
    }

    /// The control-voltage law has known points: a hard cam is at the
    /// floor for the first half of a step and open for the second; the
    /// soft cam rises continuously to unity at its middle.
    #[test]
    fn the_cam_has_the_reference_shape() {
        let hard = Settings {
            division: 2,
            depth: 0.75,
            shape: 0.0,
            hold: 0.0,
        };
        assert_eq!(gain_at(0.0, hard), 0.25);
        assert_eq!(gain_at(0.499, hard), 0.25);
        assert_eq!(gain_at(0.5, hard), 1.0);
        assert_eq!(gain_at(0.999, hard), 1.0);

        let soft = Settings { shape: 1.0, ..hard };
        assert!((gain_at(0.25, soft) - 0.625).abs() < 1e-6);
        assert_eq!(gain_at(0.5, soft), 1.0);
        assert!((gain_at(0.75, soft) - 0.625).abs() < 1e-6);
    }

    /// Each DIVISION is a musical note length in the engine's quarter-note
    /// beat unit, not an arbitrary rate tied to block or sample size.
    #[test]
    fn division_names_have_musical_lengths() {
        for (index, beats) in pump_curve::DIVISION_BEATS.into_iter().enumerate() {
            let mut core = core_with(&[(p::DIVISION, index as f32)]);
            assert_eq!(core.settings().division_beats(), beats);
            core.set_param(p::DIVISION, index as f32 + 0.49);
            assert_eq!(core.settings().division_beats(), beats);
        }
    }

    /// DEPTH zero is the console's measured bypass promise, including odd
    /// block sizes, stereo, and values with sign bits worth preserving.
    #[test]
    fn zero_depth_is_an_exact_wire_at_every_edge_length() {
        let mut core = core_with(&[(p::DEPTH, 0.0)]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let mut l: Vec<f32> = (0..len)
                .map(|i| if i % 2 == 0 { -0.0 } else { i as f32 * 0.03125 })
                .collect();
            let mut r: Vec<f32> = l.iter().map(|sample| -*sample).collect();
            let (before_l, before_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r, &clock_at(0.0, true));
            assert_eq!(
                l.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                before_l.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                r.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                before_r.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn stopped_transport_leaves_monitoring_open() {
        let mut core = core_with(&[(p::DEPTH, 100.0)]);
        let mut l = [0.25, -0.5, 0.75];
        let before = l;
        core.process(&mut l, &mut [], &clock_at(0.0, false));
        assert_eq!(l, before);
        assert_eq!(core.readout().reduction_db, 0.0);
        assert_eq!(core.readout().bands[0], 1.0);
    }

    /// A stereo sidechain VCA is linked: opposite-polarity sides retain
    /// equal magnitude through every point on the envelope.
    #[test]
    fn stereo_gain_is_linked() {
        let mut core = core_with(&[(p::DEPTH, 80.0), (p::SHAPE, 100.0)]);
        let mut l = [0.5; BLOCK];
        let mut r = [-0.5; BLOCK];
        core.process(&mut l, &mut r, &clock_at(0.0, true));
        for (left, right) in l.iter().zip(r) {
            assert_eq!(*left, -right);
        }
    }

    /// Transport segmentation cannot change the envelope or the signal.
    #[test]
    fn split_blocks_are_bit_exact() {
        let edits = [
            (p::DIVISION, 4.0),
            (p::DEPTH, 83.0),
            (p::SHAPE, 61.0),
            (p::HOLD, 27.0),
        ];
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.071).sin() * 0.7).collect();
        let bps = 120.0 / 60.0 / f64::from(FS);

        let mut whole = core_with(&edits);
        let mut a = input.clone();
        whole.process(&mut a, &mut [], &clock_at(0.0, true));

        let mut pieces = core_with(&edits);
        let mut b = input;
        let mut at = 0usize;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            let clock = Clock {
                playing: true,
                beat: at as f64 * bps,
                beats_per_sample: bps,
            };
            pieces.process(&mut b[at..end], &mut [], &clock);
            at = end;
        }
        assert_eq!(a, b);
    }

    #[test]
    fn active_process_does_not_allocate() {
        use assert_no_alloc::assert_no_alloc;
        let mut core = core_with(&[(p::DEPTH, 100.0), (p::SHAPE, 70.0)]);
        let mut l = [0.25f32; BLOCK];
        let mut r = [-0.25f32; BLOCK];
        let clock = clock_at(0.125, true);
        assert_no_alloc(|| core.process(&mut l, &mut r, &clock));
    }

    #[test]
    fn silence_and_a_denormal_tail_stay_finite() {
        let mut core = core_with(&[(p::DEPTH, 100.0), (p::SHAPE, 100.0)]);
        let mut silence = [0.0f32; BLOCK];
        core.process(&mut silence, &mut [], &clock_at(0.0, true));
        assert!(silence.iter().all(|sample| sample.to_bits() == 0));

        let mut tail = [f32::MIN_POSITIVE; BLOCK];
        core.process(&mut tail, &mut [], &clock_at(0.2, true));
        assert!(tail.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn letters_are_clamped_and_unknown_ids_are_ignored() {
        let mut core = core_with(&[]);
        core.set_param(p::DIVISION, 99.0);
        core.set_param(p::DEPTH, -10.0);
        core.set_param(p::SHAPE, 500.0);
        core.set_param(p::HOLD, 500.0);
        core.set_param(99, 1.0);
        let settings = core.settings();
        assert_eq!(settings.division, 4);
        assert_eq!(settings.depth, 0.0);
        assert_eq!(settings.shape, 1.0);
        assert!((settings.hold - 0.6).abs() < 1e-6);
    }
}
