//! PREAMP: the input stage of every channel, always in.
//!
//! At its floor it is a wire to the sample. Leaned on, it is one of two
//! stages. IRON is a transformer-coupled class-A stage: the bottom is
//! lifted before the curve and put back after it, so low frequencies
//! drive the curve harder than highs (a transformer core saturating on
//! flux), and the curve is asymmetric, so the harmonics are even.
//! STEEL is a push-pull op-amp stage: symmetric, a harder knee, odd
//! harmonics, no low emphasis. Both run at 2×, and both are followed
//! by a DC blocker, because a bent curve makes DC.
//!
//! TRIM is a gain in dB, smoothed so a turn never clicks. PHASE flips
//! the sign. COLOUR is the transformer's own tilt as a switch: a lift
//! under 100 Hz and a softening over 10 kHz, fixed.
//!
//! The curves are normalised to unit slope at zero, so the drive
//! changes colour before it changes level; what level it does take is
//! the curve's own compression of the peaks, which is the stage's.
//!
//! Latency: the oversampler's fixed round trip. PREAMP is structural, and
//! the round trip stays in circuit even when IRON is at zero so a live drive
//! change cannot move the channel by 35 samples. At the floor the stage is
//! therefore a wire *behind that declared latency*, not an unreported
//! zero-latency side door around the antialias filters.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::{BandShape, DcBlocker, EqBand};
use crate::dsp::ramps::Smoother;
use crate::dsp::shaper::Oversampler2x;
use crate::params::console::preamp as p;

use crate::console::preamp_curve::{self, IRON_DRIVE, STEEL_DRIVE};

/// The low emphasis the iron puts under its curve, and takes off after.
const IRON_LIFT_HZ: f32 = 120.0;
const IRON_LIFT_DB: f32 = 4.0;
/// The colour switch's fixed shelves.
const COLOUR_LOW_HZ: f32 = 100.0;
const COLOUR_LOW_DB: f32 = 2.0;
const COLOUR_HIGH_HZ: f32 = 10_000.0;
const COLOUR_HIGH_DB: f32 = -1.5;
/// A trim turn reaches its value in about this long.
const TRIM_MS: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    trim_db: f32,
    iron: f32,
    steel: bool,
    flip: bool,
    colour: bool,
}

impl Settings {
    fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Preamp.table();
        let clamp = |id: u32, value: f32| {
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            trim_db: clamp(p::TRIM, params.value(p::TRIM)),
            iron: clamp(p::IRON, params.value(p::IRON)) / 100.0,
            steel: params.value(p::CHARACTER).round() as u32 == p::CHARACTER_STEEL,
            flip: params.value(p::PHASE) >= 0.5,
            colour: params.value(p::COLOUR) >= 0.5,
        }
    }
}

use preamp_curve::{iron as iron_curve, steel as steel_curve};

pub struct PreampCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    trim: Smoother,
    /// The iron's emphasis and its undoing, per channel.
    lift: [EqBand; 2],
    unlift: [EqBand; 2],
    colour_low: [EqBand; 2],
    colour_high: [EqBand; 2],
    over: [Oversampler2x; 2],
    dc: [DcBlocker; 2],
    /// Compile-owned scratch: the control ramp, and the 2× lane.
    ramp: Vec<f32>,
    lane: Vec<f32>,
    level_db: f32,
}

impl PreampCore {
    /// Green zone: every buffer and coefficient the callback will use.
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let settings = Settings::of(params);
        let mut core = Self {
            params: params.dense(),
            settings,
            sample_rate,
            trim: Smoother::new(),
            lift: [EqBand::new(), EqBand::new()],
            unlift: [EqBand::new(), EqBand::new()],
            colour_low: [EqBand::new(), EqBand::new()],
            colour_high: [EqBand::new(), EqBand::new()],
            over: [Oversampler2x::new(), Oversampler2x::new()],
            dc: [DcBlocker::new(), DcBlocker::new()],
            ramp: vec![0.0; block.max(1)],
            lane: vec![0.0; Oversampler2x::scratch_len(block.max(1))],
            level_db: -120.0,
        };
        core.trim.prepare(sample_rate, TRIM_MS);
        core.trim.set_now(db_to_gain(settings.trim_db));
        for ch in 0..2 {
            core.lift[ch].prepare(
                sample_rate,
                IRON_LIFT_HZ,
                0.7,
                IRON_LIFT_DB,
                BandShape::LowShelf,
            );
            core.unlift[ch].prepare(
                sample_rate,
                IRON_LIFT_HZ,
                0.7,
                -IRON_LIFT_DB,
                BandShape::LowShelf,
            );
            core.colour_low[ch].prepare(
                sample_rate,
                COLOUR_LOW_HZ,
                0.7,
                COLOUR_LOW_DB,
                BandShape::LowShelf,
            );
            core.colour_high[ch].prepare(
                sample_rate,
                COLOUR_HIGH_HZ,
                0.7,
                COLOUR_HIGH_DB,
                BandShape::HighShelf,
            );
            core.over[ch].prepare();
            core.dc[ch].prepare(sample_rate);
        }
        core
    }

    pub fn settings_iron(&self) -> f32 {
        self.settings.iron
    }

    /// The curve as the card draws it: the stage's transfer at the
    /// current drive, for `x` in −1..=1.
    pub fn transfer(&self, x: f32) -> f32 {
        transfer(self.settings.iron, self.settings.steel, x)
    }

    /// Whether the stage is an identity apart from its fixed latency.
    pub fn is_wire(&self) -> bool {
        let s = self.settings;
        s.trim_db == 0.0 && s.iron == 0.0 && !s.flip && !s.colour && self.trim.current() == 1.0
    }

    fn drive_one(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let s = self.settings;
        let k = if s.steel {
            1.0 + STEEL_DRIVE * s.iron
        } else {
            1.0 + IRON_DRIVE * s.iron
        };
        if !s.steel {
            self.lift[ch].process(io);
        }
        let lane = &mut self.lane[..n * 2];
        self.over[ch].up(io, lane);
        if s.steel {
            for x in lane.iter_mut() {
                *x = steel_curve(*x, k);
            }
        } else {
            for x in lane.iter_mut() {
                *x = iron_curve(*x, k);
            }
        }
        self.over[ch].down(lane, io);
        if !s.steel {
            self.unlift[ch].process(io);
        }
        self.dc[ch].process(io);
    }

    /// Keep the antialias round trip warm while the nonlinear curve is at
    /// its floor. This is the phase-coherent bypass for a structural stage:
    /// the graph sees one constant delay however the IRON knob moves.
    fn round_trip_one(&mut self, ch: usize, io: &mut [f32]) {
        let lane = &mut self.lane[..io.len() * 2];
        self.over[ch].up(io, lane);
        self.over[ch].down(lane, io);
    }
}

/// The stage's transfer at `iron` (0..1) and character, for a display.
pub fn transfer(iron: f32, steel: bool, x: f32) -> f32 {
    preamp_curve::transfer(iron, steel, x)
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl SectionCore for PreampCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next.trim_db != self.settings.trim_db {
            self.trim.set_target(db_to_gain(next.trim_db));
        }
        self.settings = next;
    }

    fn reset(&mut self) {
        self.trim.set_now(db_to_gain(self.settings.trim_db));
        for ch in 0..2 {
            self.lift[ch].reset();
            self.unlift[ch].reset();
            self.colour_low[ch].reset();
            self.colour_high[ch].reset();
            self.over[ch].reset();
            self.dc[ch].reset();
        }
        self.level_db = -120.0;
    }

    fn latency(&self) -> usize {
        self.over[0].latency()
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.ramp.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;

        // The setting can be an identity, but PREAMP remains behind its
        // fixed antialias round trip. Skipping it here made an IRON letter
        // move the whole channel by the filter's group delay.
        let trim_settled = self.trim.current() == 1.0 && s.trim_db == 0.0;

        // PHASE, then TRIM as a ramp so a turn glides.
        let sign = if s.flip { -1.0 } else { 1.0 };
        if !trim_settled || s.flip {
            self.trim.process(&mut self.ramp[..n]);
            for (x, g) in l.iter_mut().zip(&self.ramp[..n]) {
                *x *= g * sign;
            }
            if stereo {
                for (x, g) in r[..n].iter_mut().zip(&self.ramp[..n]) {
                    *x *= g * sign;
                }
            }
        }

        // The stage is always the same length. At the floor only the
        // identity round trip runs; leaned on, the curve lives between it.
        if s.iron > 0.0 {
            self.drive_one(0, l);
            if stereo {
                self.drive_one(1, &mut r[..n]);
            }
        } else {
            self.round_trip_one(0, l);
            if stereo {
                self.round_trip_one(1, &mut r[..n]);
            }
        }

        // COLOUR: the transformer's tilt, on or off.
        if s.colour {
            self.colour_low[0].process(l);
            self.colour_high[0].process(l);
            if stereo {
                self.colour_low[1].process(&mut r[..n]);
                self.colour_high[1].process(&mut r[..n]);
            }
        }
        self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            ..Readout::default()
        }
    }
}

/// The louder side's peak, in dBFS.
fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= 1e-6 {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> PreampCore {
        let mut params = SectionParams::of(SectionKind::Preamp);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        PreampCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut PreampCore, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        for start in (0..l.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(l.len());
            let (a, b) = l[start..end].split_at_mut(0);
            let _ = a;
            let right: &mut [f32] = if r.is_empty() {
                &mut []
            } else {
                &mut r[start..end]
            };
            core.process(b, right, &clock());
        }
        (l, r)
    }

    /// The magnitude of harmonic `h` of `hz` in `signal`, in dB relative
    /// to the fundamental.
    fn harmonic_db(signal: &[f32], hz: f32, h: u32) -> f32 {
        let bin = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in signal.iter().enumerate() {
                let w = 2.0 * core::f32::consts::PI * f * i as f32 / FS;
                re += s * w.cos();
                im -= s * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        20.0 * (bin(hz * h as f32) / bin(hz).max(1e-9)).log10()
    }

    /// At the floor the stage is a wire behind the antialias round trip.
    /// The impulse pins the delay the graph must compensate and the two
    /// sides pin one shared topology rather than a mono-only shortcut.
    #[test]
    fn the_floor_is_a_wire_behind_its_reported_latency() {
        let mut core = core_with(&[]);
        assert!(core.is_wire());
        let ahead = core.latency();
        assert_eq!(ahead, Oversampler2x::new().latency());
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        l[0] = 1.0;
        r[0] = -1.0;
        core.process(&mut l, &mut r, &clock());
        let peak = l
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .expect("an impulse has a peak");
        assert_eq!(peak.0, ahead);
        assert!(peak.1.abs() > 0.9, "the round trip lost the impulse");
        assert!(
            l.iter()
                .zip(&r)
                .all(|(a, b)| (*a + *b).abs() <= f32::EPSILON),
            "the stereo lanes must remain equal and opposite"
        );
    }

    /// Trim is a gain: −6 dB halves a sine once the ramp has settled.
    #[test]
    fn trim_is_a_gain_in_db() {
        let mut core = core_with(&[(p::TRIM, -6.0)]);
        let l = sine(440.0, 0.5, FS as usize / 10);
        let (ol, _) = run(&mut core, &l, &[]);
        let tail = &ol[ol.len() - 4800..];
        let peak = tail.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.2505).abs() < 0.01, "peak {peak}");
    }

    /// Phase flips the sign, exactly.
    #[test]
    fn phase_flips_the_sign() {
        let l = sine(440.0, 0.5, BLOCK);
        let mut plain = core_with(&[]);
        let (reference, _) = run(&mut plain, &l, &[]);
        let mut core = core_with(&[(p::PHASE, 1.0)]);
        let (ol, _) = run(&mut core, &l, &[]);
        for (a, b) in ol.iter().zip(&reference) {
            assert!((a + b).abs() < 1e-6);
        }
    }

    /// Iron adds a second harmonic and keeps the peak bounded; steel
    /// adds a third and hardly any second. Neither changes the level of
    /// a moderate signal by more than a dB.
    #[test]
    fn iron_is_even_and_steel_is_odd() {
        let n = FS as usize / 2;
        let l = sine(100.0, 0.25, n);
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let base = rms(&l);

        // Whole periods of 100 Hz at 48 kHz, so the DFT does not leak.
        let window = n / 2..n / 2 + 480 * 20;
        let mut iron = core_with(&[(p::IRON, 50.0)]);
        let (out, _) = run(&mut iron, &l, &[]);
        let steady = &out[window.clone()];
        let second = harmonic_db(steady, 100.0, 2);
        let third = harmonic_db(steady, 100.0, 3);
        assert!(second > -40.0, "iron's second harmonic is {second} dB");
        assert!(second > third, "iron: second {second} dB, third {third} dB");
        assert!(out.iter().all(|s| s.abs() <= 1.0));
        let level = 20.0 * (rms(steady) / base).log10();
        assert!(level.abs() < 1.5, "iron moved the level by {level} dB");
        assert!(iron.readout().level_db > -20.0);

        let mut steel = core_with(&[(p::IRON, 50.0), (p::CHARACTER, 1.0)]);
        let (out, _) = run(&mut steel, &l, &[]);
        let steady = &out[window];
        let second = harmonic_db(steady, 100.0, 2);
        let third = harmonic_db(steady, 100.0, 3);
        assert!(third > -40.0, "steel's third harmonic is {third} dB");
        assert!(
            third > second + 10.0,
            "steel: second {second} dB, third {third} dB"
        );
        let level = 20.0 * (rms(steady) / base).log10();
        assert!(level.abs() < 1.5, "steel moved the level by {level} dB");
    }

    /// The iron's asymmetry makes DC, and the blocker takes it away.
    #[test]
    fn the_dc_the_curve_makes_is_blocked() {
        let n = FS as usize / 2;
        let l = sine(200.0, 0.5, n);
        let mut core = core_with(&[(p::IRON, 100.0)]);
        let (out, _) = run(&mut core, &l, &[]);
        let tail = &out[n / 2..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        let raw: f32 = tail.iter().map(|x| iron_curve(*x, 3.5)).sum::<f32>() / tail.len() as f32;
        assert!(mean.abs() < 0.005, "mean {mean}");
        assert!(
            raw.abs() > mean.abs(),
            "the curve itself does make DC ({raw})"
        );
    }

    /// Colour is a tilt: a low sine comes out louder, a high one quieter.
    #[test]
    fn colour_lifts_the_bottom_and_softens_the_top() {
        let n = FS as usize / 4;
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        for (hz, up) in [(50.0, true), (15_000.0, false)] {
            let l = sine(hz, 0.25, n);
            let mut core = core_with(&[(p::COLOUR, 1.0)]);
            let (out, _) = run(&mut core, &l, &[]);
            let change = 20.0 * (rms(&out[n / 2..]) / rms(&l)).log10();
            assert_eq!(change > 0.3, up, "{hz} Hz changed by {change} dB");
        }
    }

    /// Processing in blocks of any size gives the same signal.
    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let mut whole = core_with(&[(p::IRON, 60.0), (p::COLOUR, 1.0), (p::TRIM, 3.0)]);
        let (a, _) = run(&mut whole, &l, &[]);
        let mut pieces = core_with(&[(p::IRON, 60.0), (p::COLOUR, 1.0), (p::TRIM, 3.0)]);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5, "{x} vs {y}");
        }
    }

    /// A letter lands, clamped; the table's ids are the only ids.
    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::IRON, 500.0);
        assert_eq!(core.settings_iron(), 1.0);
        core.set_param(p::TRIM, -100.0);
        assert_eq!(core.settings.trim_db, -24.0);
        core.set_param(99, 1.0);
        assert!(!core.is_wire());
        core.set_param(p::IRON, 0.0);
        core.set_param(p::TRIM, 0.0);
        core.reset();
        assert!(core.is_wire());
    }

    /// The display's curve is the stage's: unit slope at zero, bent at
    /// the top, and a wire at no drive.
    #[test]
    fn the_transfer_is_unit_slope_and_bends() {
        assert_eq!(transfer(0.0, false, 0.5), 0.5);
        let small = transfer(1.0, false, 0.001) / 0.001;
        assert!((small - 1.0).abs() < 0.05, "slope {small}");
        assert!(transfer(1.0, false, 0.9) < 0.9);
        assert!(transfer(1.0, true, 0.9) < 0.9);
        assert!(
            transfer(1.0, false, 0.9) != -transfer(1.0, false, -0.9),
            "iron is asymmetric"
        );
        assert!(
            (transfer(1.0, true, 0.9) + transfer(1.0, true, -0.9)).abs() < 1e-6,
            "steel is symmetric"
        );
    }
}
