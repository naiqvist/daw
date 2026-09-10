//! Harmonic oscillator bank with a continuously addressable spectral envelope.
//! State: two [u32; LANES] phase/increment arrays, two frequency/mask arrays
//! and one sample-rate scalar. Caller provides a 2049-point sine table.
//! Cost: bounded by 128 partials × active lanes × samples; linear table reads.
//! Denormal story: subnormal amplitudes are discarded; other operations rely
//! on the engine's FTZ mode. Zero amplitudes remain exact zero.
//! No implicit smoothing. Caller supplies time-varying spectra via ramp kernels.
//! Out-of-place, planar lane frames. Latency: 0 samples. Fundamental harmonics
//! taper above 0.40*sr and vanish at 0.45*sr; arbitrary modulation is NOT
//! claimed to have an unlimited alias-free bandwidth.
use super::{LANES, LaneFrame};
pub const PARTIALS: usize = 128;
pub const SINE_SIZE: usize = 2048;

pub fn build_sine(table: &mut [f32]) {
    if table.len() != SINE_SIZE + 1 {
        return;
    }
    for (i, x) in table.iter_mut().enumerate() {
        *x = if i == SINE_SIZE {
            0.0
        } else {
            (core::f64::consts::TAU * i as f64 / SINE_SIZE as f64).sin() as f32
        };
    }
}

#[derive(Clone, Debug)]
pub struct HarmonicOsc {
    sr: f32,
    phase: [u32; LANES],
    inc: [u32; LANES],
    hz: LaneFrame,
    active: [bool; LANES],
}
impl Default for HarmonicOsc {
    fn default() -> Self {
        Self::new()
    }
}
impl HarmonicOsc {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            phase: [0; LANES],
            inc: [0; LANES],
            hz: [0.0; LANES],
            active: [false; LANES],
        }
    }
    pub fn prepare(&mut self, sr: f32) {
        self.sr = if sr.is_finite() {
            sr.clamp(8_000.0, 192_000.0)
        } else {
            48_000.0
        };
        self.reset();
    }
    pub fn reset(&mut self) {
        self.phase.fill(0);
        self.active.fill(false);
    }
    pub fn start(&mut self, lane: usize, hz: f32) {
        self.set_frequency(lane, hz);
        if let Some(p) = self.phase.get_mut(lane) {
            *p = 0;
        }
        if let Some(a) = self.active.get_mut(lane) {
            *a = true;
        }
    }
    pub fn set_frequency(&mut self, lane: usize, hz: f32) {
        if lane >= LANES {
            return;
        }
        let hz = if hz.is_finite() {
            hz.clamp(1.0, self.sr * 0.45)
        } else {
            1.0
        };
        self.hz[lane] = hz;
        self.inc[lane] = (hz as f64 / self.sr as f64 * 4_294_967_296.0) as u32;
    }
    pub fn stop(&mut self, lane: usize) {
        if let Some(a) = self.active.get_mut(lane) {
            *a = false;
        }
    }
    pub fn process(
        &mut self,
        out: &mut [LaneFrame],
        amplitudes: &[f32; PARTIALS],
        phases_deg: &[f32; PARTIALS],
        shift: f32,
        sine: &[f32],
    ) {
        if sine.len() != SINE_SIZE + 1 {
            out.fill([0.0; LANES]);
            return;
        }
        let shift = if shift.is_finite() {
            shift.clamp(-44.0, 44.0)
        } else {
            0.0
        };
        // Shared spectrum, independent oscillator phase in every voice lane.
        let mut amps = [0.0; PARTIALS];
        let mut offsets = [0u32; PARTIALS];
        let mut sum = 0.0f32;
        for h in 0..PARTIALS {
            let position = h as f32 - shift;
            let lo = position.floor() as i32;
            let fraction = position - lo as f32;
            let read = |i: i32| -> f32 {
                if i < 0 {
                    return 0.0;
                }
                amplitudes
                    .get(i as usize)
                    .copied()
                    .filter(|x| x.is_finite() && !x.is_subnormal())
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0)
            };
            amps[h] = read(lo) * (1.0 - fraction) + read(lo + 1) * fraction;
            sum += amps[h];
            let turns = if phases_deg[h].is_finite() {
                (phases_deg[h] / 360.0).rem_euclid(1.0)
            } else {
                0.0
            };
            offsets[h] = (turns as f64 * 4_294_967_296.0) as u32;
        }
        let gain = 1.0 / sum.max(1.0);
        for frame in out {
            frame.fill(0.0);
            for h in 0..PARTIALS {
                if amps[h] == 0.0 {
                    continue;
                }
                let harmonic = (h + 1) as u32;
                for (lane, sample) in frame.iter_mut().enumerate() {
                    if !self.active[lane] {
                        continue;
                    }
                    let hz = self.hz[lane] * harmonic as f32;
                    let taper = ((self.sr * 0.45 - hz) / (self.sr * 0.05)).clamp(0.0, 1.0);
                    if taper == 0.0 {
                        continue;
                    }
                    let phase = self.phase[lane]
                        .wrapping_mul(harmonic)
                        .wrapping_add(offsets[h]);
                    let index = (phase >> 21) as usize;
                    let x = (phase & 0x1fffff) as f32 * (1.0 / 2_097_152.0);
                    let a = sine[index];
                    let b = sine[index + 1];
                    *sample += (a + (b - a) * x) * amps[h] * gain * taper;
                }
            }
            for lane in 0..LANES {
                if self.active[lane] {
                    self.phase[lane] = self.phase[lane].wrapping_add(self.inc[lane]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (HarmonicOsc, [f32; SINE_SIZE + 1], [f32; PARTIALS]) {
        let mut osc = HarmonicOsc::new();
        osc.prepare(48_000.0);
        osc.start(0, 375.0);
        let mut table = [0.0; SINE_SIZE + 1];
        build_sine(&mut table);
        let mut amps = [0.0; PARTIALS];
        amps[0] = 1.0;
        (osc, table, amps)
    }
    #[test]
    fn subnormal_amplitudes_are_silent_and_retuning_one_lane_leaves_others_unchanged() {
        let (mut a,table,mut amps)=fixture();a.start(1,440.0);let mut b=a.clone();
        b.set_frequency(0,880.0);let mut x=[[0.0;LANES];257];let mut y=x;
        a.process(&mut x,&amps,&[0.0;PARTIALS],0.0,&table);
        b.process(&mut y,&amps,&[0.0;PARTIALS],0.0,&table);
        for i in 0..257 {assert_eq!(x[i][1..],y[i][1..]);}
        amps.fill(f32::from_bits(1));a.process(&mut x,&amps,&[0.0;PARTIALS],0.0,&table);
        assert!(x.iter().flatten().all(|s|*s==0.0));
    }
    #[test]
    fn reference_pitch_amplitude_phase_and_lane_isolation() {
        let (mut osc, table, amps) = fixture();
        let mut out = [[0.0; LANES]; 257];
        let mut phases = [0.0; PARTIALS];
        phases[0] = 90.0;
        osc.process(&mut out, &amps, &phases, 0.0, &table);
        for (i, frame) in out.iter().enumerate() {
            let want = (core::f64::consts::TAU * i as f64 / 128.0).cos() as f32;
            assert!((frame[0] - want).abs() < 5e-6);
            assert_eq!(frame[1..], [0.0; LANES - 1]);
        }
    }
    #[test]
    fn split_blocks_edges_and_no_allocation() {
        let (mut full, table, mut amps) = fixture();
        amps.fill(0.1);
        for lane in 1..LANES {
            full.start(lane, 150.0 + lane as f32 * 20.0);
        }
        let mut split = full.clone();
        let phases = [37.0; PARTIALS];
        let mut a = [[0.0; LANES]; 257];
        let mut b = a;
        assert_no_alloc::assert_no_alloc(|| {
            full.process(&mut a, &amps, &phases, 2.5, &table);
            split.process(&mut [], &amps, &phases, 2.5, &table);
            split.process(&mut b[..1], &amps, &phases, 2.5, &table);
            split.process(&mut b[1..100], &amps, &phases, 2.5, &table);
            split.process(&mut b[100..], &amps, &phases, 2.5, &table);
        });
        assert_eq!(a, b);
        assert!(
            a.iter()
                .flatten()
                .all(|x| x.is_finite() && x.abs() <= 1.001)
        );
    }
    #[test]
    fn spectral_shift_moves_energy_without_replacing_the_note_frequency() {
        let (mut osc, table, amps) = fixture();
        let mut out = [[0.0; LANES]; 128];
        osc.process(&mut out, &amps, &[0.0; PARTIALS], 1.0, &table);
        for (i, frame) in out.iter().enumerate() {
            let want = (core::f64::consts::TAU * i as f64 / 64.0).sin() as f32;
            assert!((frame[0] - want).abs() < 5e-6);
        }
    }
    #[test]
    fn above_band_harmonics_are_removed_and_reset_is_silent() {
        let (mut osc, table, mut amps) = fixture();
        amps.fill(0.0);
        amps[127] = 1.0;
        let mut out = [[0.0; LANES]; 257];
        osc.process(&mut out, &amps, &[0.0; PARTIALS], 0.0, &table);
        assert!(out.iter().flatten().all(|x| *x == 0.0));
        amps[0] = f32::NAN;
        osc.process(&mut out, &amps, &[f32::NAN; PARTIALS], f32::NAN, &table);
        assert!(out.iter().flatten().all(|x| x.is_finite()));
        osc.reset();
        amps[0] = 1.0;
        osc.process(&mut out, &amps, &[0.0; PARTIALS], 0.0, &table);
        assert!(out.iter().flatten().all(|x| *x == 0.0));
        osc.process(&mut out, &amps, &[0.0; PARTIALS], 0.0, &[]);
        assert!(out.iter().flatten().all(|x| *x == 0.0));
    }
}
