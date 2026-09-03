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
    /// A resident second side, for a mono block. The limiter is linked
    /// and wants two channels; allocating one per block would be an
    /// allocation in the callback, which is the one rule that outranks
    /// everything else.
    mirror: Vec<f32>,
    level_db: f32,
    most_reduced_db: f32,
    /// Where the limiter's smoothed gain STANDS at the block's last
    /// sample, in dB. Not the same question as `most_reduced_db`: the
    /// block's worst is the blow, this is the recovery, and a surface
    /// that draws the 120 ms release needs the second one.
    held_gain_db: f32,
    /// The loudest sample anywhere inside the lookahead window as the
    /// block ends, in dBFS — a peak that is still in the delay line and
    /// has not been heard yet.
    window_peak_db: f32,
}

impl CeilingCore {
    pub fn new(_params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut limiter = LookaheadLimiter::new();
        limiter.prepare(sample_rate, p::LOOKAHEAD_MS, p::RELEASE_MS);
        limiter.set_ceiling_db(p::CEILING_DB);
        let len = LookaheadLimiter::scratch_len(sample_rate, p::LOOKAHEAD_MS);
        Self {
            limiter,
            key: vec![0.0; len],
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            mirror: vec![0.0; len.max(block.max(1))],
            level_db: p::SILENCE_DB,
            most_reduced_db: 0.0,
            held_gain_db: p::REST_GAIN_DB,
            window_peak_db: p::SILENCE_DB,
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
        self.mirror.fill(0.0);
        self.level_db = p::SILENCE_DB;
        self.most_reduced_db = 0.0;
        self.held_gain_db = p::REST_GAIN_DB;
        self.window_peak_db = p::SILENCE_DB;
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
            // Mono: the same limiter, both sides one signal, through
            // the resident mirror rather than a fresh buffer.
            let room = n.min(self.mirror.len());
            self.mirror[..room].copy_from_slice(&l[..room]);
            let (mirror, key) = (&mut self.mirror[..room], &mut self.key);
            self.limiter
                .process_linked(l, mirror, key, &mut self.buf_l, &mut self.buf_r);
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
            p::SILENCE_DB
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
        // The two figures that are STATE rather than a block summary,
        // read once here so `readout()` stays a pure copy of fields:
        // where the gain stands now (still falling back to unity long
        // after the transient that pushed it down), and what the window
        // can see that has not come out of the delay line yet.
        self.held_gain_db = self.limiter.gain_db();
        self.window_peak_db = self.limiter.window_peak_db();
    }

    /// What CEILING reports, field by field. A surface is written
    /// against these words, so they are the contract:
    ///
    /// - `level_db` — the loudest sample that LEFT the section this
    ///   block, in dBFS, floored at `p::SILENCE_DB` (−120) and in
    ///   practice never above `p::CEILING_DB`. A per-block maximum: no
    ///   smoothing, no tail, it falls to the floor the block after the
    ///   sound stops.
    /// - `reduction_db` — what this block cost the loudest thing in it,
    ///   `20·log10(out_peak / in_peak)`, in dB over −∞..0, zero when
    ///   nothing was taken. Per-block and instantaneous in BOTH
    ///   directions: the blow, gone the next block.
    /// - `bands[0]` — `p::CEILING_DB`, the ceiling in dBFS (−0.3). A
    ///   constant, never moves; it is here so a surface can plot its
    ///   whole axis from the engine's own number instead of a literal.
    /// - `bands[1]` — where the limiter's smoothed gain STANDS at the
    ///   block's last sample, in dB over −∞..0, resting at
    ///   `p::REST_GAIN_DB` (0.0) when the ceiling is open. The only
    ///   field here with a tail: it drops the instant a peak in the
    ///   window demands it (the gain is hard-floored to what the peak
    ///   requires, so there is no attack lag) and recovers as a one-pole
    ///   at the limiter's own `p::RELEASE_MS` — 120 ms. It is still
    ///   negative in blocks where `reduction_db` reads zero, and that
    ///   difference is the point of carrying both.
    /// - `bands[2]` — the sliding-window maximum of the detector key
    ///   over the `p::LOOKAHEAD_MS` (1.5 ms) window as the block ends,
    ///   in dBFS, floored at `p::SILENCE_DB` (−120) and unbounded above
    ///   (a +12 dB peak reads +12). No smoothing whatever, and it is
    ///   read from AHEAD of the playhead: it rises up to 1.5 ms before
    ///   the sample it describes leaves the section, so it leads
    ///   `level_db`, `reduction_db` and `bands[1]` on every transient.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.most_reduced_db,
            bands: [p::CEILING_DB, self.held_gain_db, self.window_peak_db],
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

    /// bands[1] is where the gain STANDS, not what a block cost. Once
    /// the transient has gone the block's own figure says nothing
    /// happened at all, and the piston is still down — walking back up
    /// on the engine's own 120 ms release, block after block, with
    /// nothing arriving to hold it there.
    #[test]
    fn the_piston_holds_after_the_blow_and_releases() {
        let mut core = core();
        let mut hot = sine(200.0, 4.0, BLOCK);
        let mut right = vec![0.0f32; BLOCK];
        core.process(&mut hot, &mut right, &clock());
        let struck = core.readout().bands[1];
        assert!(struck < -6.0, "the blow left the piston at {struck} dB");

        // A second of silence, watched block by block.
        let mut walk = Vec::new();
        for _ in 0..(FS as usize / BLOCK) {
            let mut l = vec![0.0f32; BLOCK];
            let mut r = vec![0.0f32; BLOCK];
            core.process(&mut l, &mut r, &clock());
            walk.push(core.readout());
        }
        // The block after the blow: nothing came in, so nothing was
        // taken off it — and the press is still deep. Two different
        // questions, and this is the block that proves it.
        assert_eq!(walk[0].reduction_db, 0.0);
        assert!(
            walk[0].bands[1] < -6.0,
            "the piston forgot the blow in one block: {} dB",
            walk[0].bands[1]
        );
        // It only ever comes back up, and it is still coming up well
        // after the block that caused it.
        for pair in walk.windows(2) {
            assert!(
                pair[1].bands[1] >= pair[0].bands[1],
                "the piston went back down on silence: {} then {}",
                pair[0].bands[1],
                pair[1].bands[1]
            );
        }
        for i in 0..8 {
            assert!(
                walk[i + 1].bands[1] > walk[i].bands[1],
                "no release between blocks {i} and {}: {} dB",
                i + 1,
                walk[i].bands[1]
            );
        }
        // Half the release should be spent by ~120 ms and the press
        // all but closed by a second.
        let at_120ms = walk[(0.120 * FS) as usize / BLOCK].bands[1];
        assert!(
            at_120ms > struck * 0.6 && at_120ms < struck * 0.1,
            "a 120 ms release went from {struck} dB to {at_120ms} dB"
        );
        let rested = walk[walk.len() - 1].bands[1];
        assert!(
            (-0.1..=0.0).contains(&rested),
            "the piston never came home: {rested} dB"
        );
    }

    /// bands[2] is read from AHEAD of the playhead. A peak that is
    /// still inside the delay line shows in the window a whole block
    /// before it is anywhere in what left the desk.
    #[test]
    fn the_window_peak_leads_what_leaves_the_desk() {
        let n = BLOCK * 3;
        let mut l = sine(200.0, 0.05, n);
        // A spike at the very END of the second block: inside the
        // 1.5 ms window as that block finishes, and not out of the
        // delay line until the third.
        for s in l.iter_mut().take(BLOCK * 2).skip(BLOCK * 2 - 8) {
            *s = 4.0;
        }
        let mut r = vec![0.0f32; n];
        let mut core = core();
        let mut said = Vec::new();
        for start in (0..n).step_by(BLOCK) {
            let end = start + BLOCK;
            core.process(&mut l[start..end], &mut r[start..end], &clock());
            said.push(core.readout());
        }
        // Before it: the window holds nothing but the quiet bed.
        assert!(
            said[0].bands[2] < p::CEILING_DB,
            "the window saw {} dB with only a quiet bed in it",
            said[0].bands[2]
        );
        // The block the spike arrives in: the window is carrying it,
        // in dBFS, and nothing loud has left the section yet.
        let seen = said[1].bands[2];
        assert!(
            (seen - 20.0 * 4.0f32.log10()).abs() < 0.1,
            "the window read {seen} dB for a peak of 4.0"
        );
        assert!(seen > p::CEILING_DB);
        assert!(
            said[1].level_db < -10.0,
            "the peak left the desk in the same block it was seen: {} dB",
            said[1].level_db
        );
        // The next block: now it has left, and at the ceiling.
        assert!(
            said[2].level_db > -3.0 && said[2].level_db <= p::CEILING_DB + 0.1,
            "the peak came out at {} dB",
            said[2].level_db
        );
        // And the window has emptied back to the bed behind it.
        assert!(said[2].bands[2] < p::CEILING_DB);
    }

    /// Silence is REST, and rest is not a blank card: an open ceiling,
    /// a closed press, an empty window — before any audio, and again
    /// after a reset that follows real audio.
    #[test]
    fn silence_reports_rest() {
        let mut core = core();
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        for _ in 0..4 {
            core.process(&mut l, &mut r, &clock());
        }
        let said = core.readout();
        assert_eq!(said.level_db, p::SILENCE_DB);
        assert_eq!(said.reduction_db, 0.0);
        assert_eq!(said.bands, [p::CEILING_DB, p::REST_GAIN_DB, p::SILENCE_DB]);

        let _ = run(&mut core, &sine(200.0, 4.0, 4096));
        assert!(core.readout().bands[1] < -6.0, "the loud pass did nothing");
        core.reset();
        let said = core.readout();
        assert_eq!(said.level_db, p::SILENCE_DB);
        assert_eq!(said.reduction_db, 0.0);
        assert_eq!(said.bands, [p::CEILING_DB, p::REST_GAIN_DB, p::SILENCE_DB]);
    }
}
