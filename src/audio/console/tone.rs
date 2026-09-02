//! TONE: the three-band, the desk's fast hand.
//!
//! LO is a shelf at 100 Hz, HI a shelf at 8 kHz, MID a bell you can
//! sweep, each ±15 dB — and each with a KILL: the DJ's stop past the
//! end of the knob. A killed LO is a 24 dB/octave high-pass at the
//! shelf's corner and a killed HI its low-pass; a killed MID is the bell
//! driven to −36 dB and widened, which is what a mid kill sounds like.
//! The corners and the Qs were chosen once, which is the whole point of
//! a three-band: three levers reached without looking.
//!
//! At all three flat and no kills the section is a wire to the sample.
//! The shape the core runs is `crate::console::tone_curve`'s, which is
//! also what the card draws.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::tone_curve::Shape;
use crate::dsp::filters::{BandShape, Cascade, EqBand};
use crate::params::console::tone as p;

pub struct ToneCore {
    params: SectionParams,
    shape: Shape,
    sample_rate: f32,
    lo: [EqBand; 2],
    mid: [EqBand; 2],
    hi: [EqBand; 2],
    kill_lo: [Cascade; 2],
    kill_hi: [Cascade; 2],
    level_db: f32,
}

impl ToneCore {
    /// Green zone: every band prepared for the settings in hand.
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            shape: Shape::of(params),
            sample_rate,
            lo: [EqBand::new(), EqBand::new()],
            mid: [EqBand::new(), EqBand::new()],
            hi: [EqBand::new(), EqBand::new()],
            kill_lo: [Cascade::new(), Cascade::new()],
            kill_hi: [Cascade::new(), Cascade::new()],
            level_db: -120.0,
        };
        core.tune();
        core
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    /// Every band's coefficients from the shape. Green zone in spirit —
    /// a handful of transcendentals — and run from `set_param`, which
    /// is a letter arriving between blocks, not inside one.
    fn tune(&mut self) {
        let s = self.shape;
        let fs = self.sample_rate;
        for ch in 0..2 {
            self.lo[ch].prepare(fs, p::LO_HZ, p::SHELF_Q, s.lo_db, BandShape::LowShelf);
            self.hi[ch].prepare(fs, p::HI_HZ, p::SHELF_Q, s.hi_db, BandShape::HighShelf);
            if s.kill_mid {
                self.mid[ch].prepare(fs, s.mid_hz, p::KILL_MID_Q, p::KILL_MID_DB, BandShape::Bell);
            } else {
                self.mid[ch].prepare(fs, s.mid_hz, p::MID_Q, s.mid_db, BandShape::Bell);
            }
            self.kill_lo[ch].prepare(
                fs,
                p::LO_HZ,
                core::f32::consts::FRAC_1_SQRT_2,
                p::KILL_ORDER,
                true,
            );
            self.kill_hi[ch].prepare(
                fs,
                p::HI_HZ,
                core::f32::consts::FRAC_1_SQRT_2,
                p::KILL_ORDER,
                false,
            );
        }
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        let s = self.shape;
        if s.kill_lo {
            self.kill_lo[ch].process(io);
        } else if s.lo_db != 0.0 {
            self.lo[ch].process(io);
        }
        if s.kill_mid || s.mid_db != 0.0 {
            self.mid[ch].process(io);
        }
        if s.kill_hi {
            self.kill_hi[ch].process(io);
        } else if s.hi_db != 0.0 {
            self.hi[ch].process(io);
        }
    }
}

impl SectionCore for ToneCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Shape::of(&self.params);
        if next != self.shape {
            self.shape = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.lo[ch].reset();
            self.mid[ch].reset();
            self.hi[ch].reset();
            self.kill_lo[ch].reset();
            self.kill_hi[ch].reset();
        }
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        if !self.shape.is_flat() {
            self.run(0, l);
            if stereo {
                self.run(1, &mut r[..n]);
            }
        }
        let peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            bands: [self.shape.lo_db, self.shape.mid_db, self.shape.hi_db],
            ..Readout::default()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;
    use crate::console::tone_curve::response_db;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> ToneCore {
        let mut params = SectionParams::of(SectionKind::Tone);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        ToneCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    /// How much `core` changes a sine at `hz`, in dB, once settled.
    fn gain_db(core: &mut ToneCore, hz: f32) -> f32 {
        let n = FS as usize / 2;
        let l = sine(hz, 0.25, n);
        let mut out = l.clone();
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut out[start..end], &mut [], &clock());
        }
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
    }

    #[test]
    fn flat_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.shape().is_flat());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Each lever moves its own band and leaves the others alone.
    #[test]
    fn each_lever_moves_its_own_band() {
        let mut lo = core_with(&[(p::LO, 12.0)]);
        assert!((gain_db(&mut lo, 40.0) - 12.0).abs() < 1.0);
        assert!(gain_db(&mut lo, 5_000.0).abs() < 0.5);

        let mut hi = core_with(&[(p::HI, -12.0)]);
        assert!((gain_db(&mut hi, 16_000.0) + 12.0).abs() < 1.5);
        assert!(gain_db(&mut hi, 300.0).abs() < 0.5);

        let mut mid = core_with(&[(p::MID, 9.0), (p::MID_HZ, 1_000.0)]);
        assert!((gain_db(&mut mid, 1_000.0) - 9.0).abs() < 0.5);
        assert!(gain_db(&mut mid, 60.0).abs() < 1.0);
        assert!(gain_db(&mut mid, 12_000.0).abs() < 1.0);
    }

    /// A kill is a kill: the band is gone, the rest is untouched.
    #[test]
    fn a_kill_takes_the_band_away() {
        let mut lo = core_with(&[(p::KILL_LO, 1.0)]);
        assert!(gain_db(&mut lo, 30.0) < -35.0);
        assert!(gain_db(&mut lo, 3_000.0).abs() < 0.5);

        let mut hi = core_with(&[(p::KILL_HI, 1.0)]);
        assert!(gain_db(&mut hi, 18_000.0) < -25.0);
        assert!(gain_db(&mut hi, 500.0).abs() < 0.5);

        let mut mid = core_with(&[(p::KILL_MID, 1.0), (p::MID_HZ, 1_000.0)]);
        assert!(gain_db(&mut mid, 1_000.0) < -30.0);
        assert!(gain_db(&mut mid, 50.0) > -3.0);
        assert!(gain_db(&mut mid, 15_000.0) > -3.0);
    }

    /// The curve the card draws is the curve the core runs.
    #[test]
    fn the_drawn_response_is_the_measured_one() {
        let edits = [
            (p::LO, 6.0),
            (p::MID, -8.0),
            (p::MID_HZ, 800.0),
            (p::HI, 4.0),
        ];
        let mut core = core_with(&edits);
        let shape = core.shape();
        for hz in [50.0, 300.0, 800.0, 2_500.0, 12_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut core, hz);
            assert!(
                (drawn - heard).abs() < 0.75,
                "{hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
        let mut killed = core_with(&[(p::KILL_LO, 1.0)]);
        let shape = killed.shape();
        for hz in [40.0, 100.0, 1_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut killed, hz);
            assert!(
                (drawn - heard).abs() < 1.5,
                "kill at {hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::LO, -6.0), (p::MID, 5.0), (p::HI, 3.0)];
        let mut whole = core_with(&edits);
        let mut a = l.clone();
        for start in (0..a.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(a.len());
            whole.process(&mut a[start..end], &mut [], &clock());
        }
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped_and_retune() {
        let mut core = core_with(&[]);
        core.set_param(p::LO, 40.0);
        assert_eq!(core.shape().lo_db, 15.0);
        core.set_param(p::MID_HZ, 10.0);
        assert_eq!(core.shape().mid_hz, 200.0);
        core.set_param(99, 1.0);
        core.set_param(p::LO, 0.0);
        core.set_param(p::MID_HZ, 1000.0);
        assert!(core.shape().is_flat());
        let said = core.readout();
        assert_eq!(said.bands, [0.0, 0.0, 0.0]);
    }
}
