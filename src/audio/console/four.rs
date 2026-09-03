//! FOUR: the console EQ.
//!
//! The engineer's hand, where TONE is the DJ's: a low band that is a
//! shelf or a bell, two proportional-Q mids with their own frequency
//! and Q, and a high band that is a shelf or a bell. Fifteen dB either
//! way on each.
//!
//! The one thing that is not textbook is the INDUCTOR BUMP. A passive
//! low shelf built round an inductor resonates a little just inside its
//! corner, and that dip-then-lift is why an old EQ's bottom sounds
//! tight where a clean shelf sounds woolly. So a low SHELF here carries
//! a small bell above its corner, opposite in sign and a quarter of the
//! gain; switch the band to a BELL and the bump goes with the shelf,
//! because it was the shelf's.
//!
//! Flat, the section is a wire to the sample. The shape lives green in
//! `crate::console::four_curve` so the card draws what the core runs.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::four_curve::Shape;
use crate::dsp::filters::EqBand;

/// Four bands and the inductor's bump.
const BANDS: usize = 5;

pub struct FourCore {
    params: SectionParams,
    shape: Shape,
    sample_rate: f32,
    bands: [[EqBand; BANDS]; 2],
    /// Which of the five actually run this setting.
    running: [bool; BANDS],
    level_db: f32,
}

impl FourCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            shape: Shape::of(params),
            sample_rate,
            bands: [[EqBand::new(); BANDS], [EqBand::new(); BANDS]],
            running: [false; BANDS],
            level_db: -120.0,
        };
        core.tune();
        core
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    fn tune(&mut self) {
        let shape = self.shape;
        self.running = [false; BANDS];
        for (index, want) in shape.all().enumerate() {
            if want.db == 0.0 {
                continue;
            }
            self.running[index] = true;
            for ch in 0..2 {
                self.bands[ch][index].prepare(
                    self.sample_rate,
                    want.hz,
                    want.q,
                    want.db,
                    want.shape,
                );
            }
        }
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        for (index, on) in self.running.iter().enumerate() {
            if *on {
                self.bands[ch][index].process(io);
            }
        }
    }
}

impl SectionCore for FourCore {
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
            for band in &mut self.bands[ch] {
                band.reset();
            }
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
            reduction_db: 0.0,
            bands: [
                self.shape.bands[0].db,
                self.shape.bands[1].db + self.shape.bands[2].db,
                self.shape.bands[3].db,
            ],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;
    use crate::console::four_curve::response_db;
    use crate::params::console::four as p;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> FourCore {
        let mut params = SectionParams::of(SectionKind::Four);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        FourCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn gain_db(core: &mut FourCore, hz: f32) -> f32 {
        let n = FS as usize / 2;
        let l = sine(hz, 0.1, n);
        let mut out = l.clone();
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut out[start..end], &mut [], &clock());
        }
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

    /// Each band lands on its own frequency and leaves the others.
    #[test]
    fn four_bands_four_places() {
        let mut lmf = core_with(&[(p::LMF_HZ, 400.0), (p::LMF_DB, 9.0), (p::LMF_Q, 2.0)]);
        assert!((gain_db(&mut lmf, 400.0) - 9.0).abs() < 0.5);
        assert!(gain_db(&mut lmf, 40.0).abs() < 0.5);
        assert!(gain_db(&mut lmf, 10_000.0).abs() < 0.5);

        let mut hmf = core_with(&[(p::HMF_HZ, 3_000.0), (p::HMF_DB, -12.0), (p::HMF_Q, 2.0)]);
        assert!((gain_db(&mut hmf, 3_000.0) + 12.0).abs() < 0.5);
        assert!(gain_db(&mut hmf, 200.0).abs() < 0.5);

        let mut high = core_with(&[(p::HIGH_DB, 6.0)]);
        assert!((gain_db(&mut high, 15_000.0) - 6.0).abs() < 1.0);
        assert!(gain_db(&mut high, 200.0).abs() < 0.5);
    }

    /// Q narrows the mid: at a high Q an octave off centre keeps less
    /// of the boost than at a low one.
    #[test]
    fn the_mids_q_narrows_them() {
        let share = |q: f32| -> f32 {
            let mut core = core_with(&[(p::LMF_HZ, 500.0), (p::LMF_DB, 12.0), (p::LMF_Q, q)]);
            let off = gain_db(&mut core, 1_000.0);
            let mut core = core_with(&[(p::LMF_HZ, 500.0), (p::LMF_DB, 12.0), (p::LMF_Q, q)]);
            off / gain_db(&mut core, 500.0)
        };
        assert!(
            share(0.5) > share(4.0) + 0.2,
            "{} vs {}",
            share(0.5),
            share(4.0)
        );
    }

    /// The inductor bump: a low SHELF dips just above its corner, so
    /// the boost there is less than the shelf alone would give; a low
    /// BELL has no bump at all.
    #[test]
    fn a_shelf_carries_the_inductors_bump_and_a_bell_does_not() {
        let mut shelf = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0)]);
        let deep = gain_db(&mut shelf, 40.0);
        let mut shelf = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0)]);
        let bumped = gain_db(&mut shelf, 160.0);
        assert!(
            (deep - 12.0).abs() < 1.0,
            "the shelf is not a shelf: {deep} dB"
        );
        assert!(
            bumped < deep - 2.0,
            "no bump: deep {deep} dB, at the corner {bumped} dB"
        );

        let mut bell = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0), (p::LOW_SHAPE, 1.0)]);
        assert!(bell.shape().inductor.is_none());
        let mut bell = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0), (p::LOW_SHAPE, 1.0)]);
        assert!(
            gain_db(&mut bell, 30.0) < 6.0,
            "a bell reached the bottom like a shelf"
        );
    }

    /// The curve the card draws is the curve the core runs.
    #[test]
    fn the_drawn_response_is_the_measured_one() {
        let edits = [
            (p::LOW_DB, 6.0),
            (p::LMF_DB, -8.0),
            (p::LMF_HZ, 700.0),
            (p::HMF_DB, 5.0),
            (p::HIGH_DB, -4.0),
        ];
        let mut core = core_with(&edits);
        let shape = core.shape();
        for hz in [40.0, 160.0, 700.0, 2_500.0, 12_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut core, hz);
            assert!(
                (drawn - heard).abs() < 0.75,
                "{hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::LOW_DB, -6.0), (p::LMF_DB, 5.0), (p::HIGH_DB, 3.0)];
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
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::LOW_DB, 40.0);
        assert_eq!(core.shape().bands[0].db, 15.0);
        core.set_param(p::LMF_Q, 99.0);
        assert_eq!(core.shape().bands[1].q, 4.0);
        core.set_param(99, 1.0);
        core.set_param(p::LOW_DB, 0.0);
        assert!(core.shape().is_flat());
    }
}
