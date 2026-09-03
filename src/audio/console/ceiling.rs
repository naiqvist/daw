//! CEILING: the mix's last stage.
//!
//! A lookahead limiter at −0.3 dBFS, and nothing else. No knobs: the
//! desk's job here is to make sure that whatever the performer does
//! upstream, what leaves the mix is inside the format's rails, and a
//! ceiling with a threshold knob is a second compressor rather than a
//! guarantee.
//!
//! It looks a millisecond and a half ahead, so a peak is turned down
//! before it arrives rather than after, and the graph pays that back
//! at compile. Linked across the sides, so the image never walks.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::dynamics::LookaheadLimiter;
use crate::params::console::ceiling as p;

pub struct CeilingCore {
    limiter: LookaheadLimiter,
    key: Vec<f32>,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    level_db: f32,
    most_reduced_db: f32,
}

impl CeilingCore {
    pub fn new(_params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut limiter = LookaheadLimiter::new();
        limiter.prepare(sample_rate, p::LOOKAHEAD_MS, p::RELEASE_MS);
        limiter.set_ceiling_db(p::CEILING_DB);
        let len = LookaheadLimiter::scratch_len(sample_rate, p::LOOKAHEAD_MS);
        Self {
            limiter,
            key: vec![0.0; len],
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            level_db: -120.0,
            most_reduced_db: 0.0,
        }
    }

    pub fn ceiling_db(&self) -> f32 {
        p::CEILING_DB
    }
}

impl SectionCore for CeilingCore {
    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn reset(&mut self) {
        self.limiter.reset();
        self.key.fill(0.0);
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.level_db = -120.0;
        self.most_reduced_db = 0.0;
    }

    fn latency(&self) -> usize {
        self.limiter.latency()
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        // What arrived, so the block can say what it took off it.
        let came_in = l
            .iter()
            .chain(if r.len() >= n {
                r[..n].iter()
            } else {
                [].iter()
            })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        if r.len() >= n {
            self.limiter.process_linked(
                l,
                &mut r[..n],
                &mut self.key,
                &mut self.buf_l,
                &mut self.buf_r,
            );
        } else {
            // Mono: the same limiter, both sides one signal.
            let mut copy = l.to_vec();
            self.limiter.process_linked(
                l,
                &mut copy,
                &mut self.key,
                &mut self.buf_l,
                &mut self.buf_r,
            );
        }
        let peak = l
            .iter()
            .chain(if r.len() >= n {
                r[..n].iter()
            } else {
                [].iter()
            })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
        // What the block cost the loudest thing in it. The limiter's
        // own figure is where its gain stands at the block's end, which
        // is not the same question.
        self.most_reduced_db = if came_in > 1e-6 && peak < came_in {
            20.0 * (peak / came_in).log10()
        } else {
            0.0
        };
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.most_reduced_db,
            bands: [p::CEILING_DB, 0.0, 0.0],
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

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core() -> CeilingCore {
        CeilingCore::new(&SectionParams::of(SectionKind::Ceiling), FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut CeilingCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        let mut right = vec![0.0f32; l.len()];
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut right[start..end], &clock());
        }
        out
    }

    fn ceiling() -> f32 {
        10f32.powf(p::CEILING_DB / 20.0)
    }

    /// Nothing gets past the ceiling, however hot it arrives.
    #[test]
    fn nothing_gets_past_the_ceiling() {
        let n = FS as usize / 2;
        for amp in [0.5, 1.0, 4.0] {
            let mut core = core();
            let out = run(&mut core, &sine(200.0, amp, n));
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak <= ceiling() * 1.02,
                "an input at {amp} came out at {peak}, ceiling {}",
                ceiling()
            );
        }
    }

    /// Under the ceiling nothing is touched but the delay.
    #[test]
    fn a_quiet_mix_is_a_wire_behind_the_lookahead() {
        let n = 4096;
        let l = sine(200.0, 0.2, n);
        let mut core = core();
        let ahead = core.latency();
        assert!(ahead > 0);
        let out = run(&mut core, &l);
        for i in ahead..n {
            assert!(
                (out[i] - l[i - ahead]).abs() < 1e-5,
                "sample {i}: {} against {}",
                out[i],
                l[i - ahead]
            );
        }
        assert_eq!(core.readout().reduction_db, 0.0);
    }

    /// A peak out of nowhere is caught, not clipped: the lookahead
    /// means the gain is already down when it arrives.
    #[test]
    fn a_sudden_peak_is_caught_before_it_lands() {
        let n = 8192;
        let mut l = sine(200.0, 0.1, n);
        for s in l.iter_mut().skip(4000).take(64) {
            *s = 3.0;
        }
        let mut core = core();
        // The readout is what a BLOCK measured, so the most it ever
        // reduced is watched block by block, the way the engine reads
        // it.
        let mut out = l.clone();
        let mut right = vec![0.0f32; n];
        let mut most = 0.0f32;
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut out[start..end], &mut right[start..end], &clock());
            most = most.min(core.readout().reduction_db);
        }
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= ceiling() * 1.02, "the peak got through at {peak}");
        assert!(most < -3.0, "no reduction reported: {most} dB");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 1.5, 2000);
        let mut whole = core();
        let a = run(&mut whole, &l);
        let mut pieces = core();
        let (mut b, mut br) = (l.clone(), vec![0.0f32; l.len()]);
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244, 256, 256, 256, 232] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut br[at..end], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-4);
        }
    }
}
