//! The gate — downward expansion, wired as a node.
//!
//! Built on the same three kernels `audio::glue` is: an
//! [`RmsDetector`](crate::dsp::dynamics::RmsDetector) to hear the level,
//! a [`GainComputer`](crate::dsp::dynamics::GainComputer) to decide what
//! the gain should be, and [`Ballistics`](crate::dsp::dynamics::Ballistics)
//! to get there at a musical speed. What makes it a gate rather than a
//! compressor is one enum: the computer runs in
//! [`Mode::Expand`](crate::dsp::dynamics::Mode::Expand) instead of
//! `Compress`.
//!
//! WIRING, NOT NEW ARITHMETIC — the thesis `dsp::mod`'s header states.
//! Every line of maths here already existed and was already tested.
//!
//! # Two things it does the opposite way to the glue
//!
//! **It is FEED-FORWARD.** The glue's detector reads its own output,
//! which is where a bus compressor's character comes from. A gate cannot
//! do that: once it closed, the detector would hear the silence the gate
//! had just made, conclude the signal was still under the threshold, and
//! never open again. This one's detector reads the input, and that is not
//! a preference — it is the only topology that reopens.
//!
//! **Its ballistics are SWAPPED.** [`Ballistics`] uses the compressor's
//! convention: `attack` is whichever direction adds reduction. A gate
//! adds reduction when it CLOSES, so the user's release time is handed to
//! the kernel's attack and the user's attack to the kernel's release. The
//! two are crossed in exactly one place — [`GateCore::process`]'s
//! prologue — and `the_gate_opens_at_its_attack_and_closes_at_its_release`
//! measures both so the crossing cannot quietly come undone. That test
//! was checked against an unswapped build before being trusted: without
//! the crossing the gate sits at -54 dB after 50 ms of loud signal,
//! because it is opening at the release time.
//!
//! # Red zone
//!
//! `process` allocates nothing, locks nothing and cannot panic: the
//! detector, the computer and the ballistics are all per-sample kernels
//! with no state that grows, and every value has already been clamped
//! through `params::gate::TABLE` on the way in.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::params::gate as p;
use crate::params::{clamp as clamp_param, def};

/// A gate's editable values, in ENGINE units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GateParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub range_db: f32,
}

impl Default for GateParams {
    fn default() -> Self {
        Self {
            threshold_db: def(p::TABLE, p::THRESHOLD).default,
            ratio: def(p::TABLE, p::RATIO).default,
            attack_ms: def(p::TABLE, p::ATTACK).default,
            release_ms: def(p::TABLE, p::RELEASE).default,
            range_db: def(p::TABLE, p::RANGE).default,
        }
    }
}

impl GateParams {
    /// This state's value for a wire id. One place an id becomes a field.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::THRESHOLD => self.threshold_db,
            p::RATIO => self.ratio,
            p::ATTACK => self.attack_ms,
            p::RELEASE => self.release_ms,
            p::RANGE => self.range_db,
            _ => return None,
        })
    }

    /// Set by wire id, ignoring anything the table does not describe.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = clamp_param(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::THRESHOLD => self.threshold_db = value,
            p::RATIO => self.ratio = value,
            p::ATTACK => self.attack_ms = value,
            p::RELEASE => self.release_ms = value,
            p::RANGE => self.range_db = value,
            _ => {}
        }
    }
}

/// The settings the kernels were last configured for.
///
/// Compared as a whole at the top of every segment so the reconfiguration
/// happens only when something moved — the same prologue `GlueCore` runs,
/// and for the same reason: `configure` and `prepare` are transcendental
/// work, and a knob that is not moving should not pay for it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
}

impl Resolved {
    fn of(params: &GateParams) -> Self {
        Self {
            threshold_db: params.threshold_db,
            ratio: params.ratio,
            attack_ms: params.attack_ms,
            release_ms: params.release_ms,
        }
    }
}

/// One gate: three kernels and the wiring between them.
#[derive(Debug, Clone, Copy)]
pub struct GateCore {
    params: GateParams,
    prepared: Resolved,
    sample_rate: f32,
    detector: RmsDetector,
    computer: GainComputer,
    ballistics: Ballistics,
    /// Where the gain is now, in dB — at or below zero.
    reduction_db: f32,
    said: crate::audio::graph::Readout,
}

impl GateCore {
    /// Green zone: build a gate at a sample rate, from clamped values.
    pub fn new(sample_rate: f32, params: &GateParams) -> Self {
        let mut params = *params;
        // Every row through the table on the way in, so a stale project
        // file cannot smuggle an out-of-range setting past the callback.
        for row in p::TABLE {
            if let Some(value) = params.get(row.id) {
                params.set(row.id, value);
            }
        }
        let mut detector = RmsDetector::new();
        detector.prepare(sample_rate, p::WINDOW_MS);
        let mut computer = GainComputer::new();
        computer.configure(Mode::Expand, params.threshold_db, params.ratio, p::KNEE_DB);
        let mut ballistics = Ballistics::new();
        // SWAPPED — see the module header.
        ballistics.prepare(sample_rate, params.release_ms, params.attack_ms);
        Self {
            params,
            prepared: Resolved::of(&params),
            sample_rate,
            detector,
            computer,
            ballistics,
            // Open. A gate that arrived shut would swallow the first note
            // of whatever was already playing when it was inserted.
            reduction_db: 0.0,
            said: crate::audio::graph::Readout::default(),
        }
    }

    /// Red zone: one door for every letter, so an id cannot be routed by
    /// two different opinions about what it is. `Node::Glue` takes the
    /// same approach and its comment is the original.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    /// This gate's values, for the app to read back.
    pub fn params(&self) -> GateParams {
        self.params
    }

    /// Green zone: forget the history, keep the settings — and reopen,
    /// for the reason `new` starts open.
    pub fn reset(&mut self) {
        self.detector.reset();
        self.ballistics.reset();
        self.reduction_db = 0.0;
    }

    /// How far the gate is currently shut, in dB. Telemetry.
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// What this node has to say about itself this segment.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        self.said
    }

    /// Red zone: gate a stereo pair in place, any length.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;

        // --- the segment prologue: settings, only if they moved --------
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            if want.threshold_db != self.prepared.threshold_db || want.ratio != self.prepared.ratio
            {
                self.computer
                    .configure(Mode::Expand, want.threshold_db, want.ratio, p::KNEE_DB);
            }
            if want.attack_ms != self.prepared.attack_ms
                || want.release_ms != self.prepared.release_ms
            {
                // THE SWAP, in the one place it happens. The kernel's
                // `attack` is whichever direction adds reduction, and a
                // gate adds reduction when it closes — so the user's
                // RELEASE is the kernel's attack.
                self.ballistics
                    .prepare(self.sample_rate, want.release_ms, want.attack_ms);
            }
            self.prepared = want;
        }

        // The RANGE cap, as a floor on the gain: the gate may not shut
        // past it, whatever the computer asks for. Zero range is a gate
        // switched off in all but name, and exactly off — a range of
        // nothing is a gain of one.
        let floor_db = -self.params.range_db.clamp(0.0, p::RANGE_MAX_DB);

        // This segment's extremes start from nothing: the schedule keeps
        // the BLOCK's, across however many segments a block was split
        // into, so accumulating here as well would report a stale peak
        // forever.
        self.said = crate::audio::graph::Readout::default();

        for i in 0..n {
            let Some(left) = l.get_mut(i) else { break };
            let dry_l = *left;
            let dry_r = if stereo {
                r.get(i).copied().unwrap_or(dry_l)
            } else {
                dry_l
            };

            // FEED-FORWARD: the detector hears the INPUT. See the module
            // header — a gate that listened to its own output could never
            // reopen.
            //
            // Linked by the larger of the two sides rather than their
            // sum, which cancels on out-of-phase material and would let a
            // wide source fail to open the gate at all.
            let side = dry_l.abs().max(dry_r.abs());
            let level = self.detector.tick(side);
            let level_db = 20.0 * level.max(1e-6).log10();
            let target = self.computer.gain_db(level_db).max(floor_db);
            self.reduction_db = self.ballistics.tick(target);
            self.said.level_db = self.said.level_db.max(level_db);
            self.said.reduction_db = self.said.reduction_db.min(self.reduction_db);

            let gain = 10.0f32.powf(self.reduction_db / 20.0);
            *left = dry_l * gain;
            if stereo && let Some(right) = r.get_mut(i) {
                *right = dry_r * gain;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(params: GateParams) -> GateCore {
        GateCore::new(FS, &params)
    }

    /// A tone at `db`, `ms` long.
    fn tone(db: f32, ms: f32) -> Vec<f32> {
        let n = (FS * ms / 1000.0) as usize;
        let amp = 10.0f32.powf(db / 20.0);
        (0..n)
            .map(|i| (i as f32 / FS * 440.0 * std::f32::consts::TAU).sin() * amp)
            .collect()
    }

    /// Loud material walks through untouched: above the threshold a gate
    /// is a wire.
    #[test]
    fn it_passes_what_is_over_the_threshold() {
        let mut g = core(GateParams::default());
        let mut l = tone(-6.0, 200.0);
        let before = l.clone();
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        // Past the detector's settling, the gain is one.
        let from = (FS * 0.05) as usize;
        for i in from..l.len() {
            assert!(
                (l[i] - before[i]).abs() < 1e-3,
                "sample {i} was {} and came out {}",
                before[i],
                l[i]
            );
        }
    }

    /// And quiet material is shut down.
    #[test]
    fn it_closes_on_what_is_under_the_threshold() {
        let mut g = core(GateParams::default());
        let mut l = tone(-70.0, 1_000.0);
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        assert!(
            g.reduction_db() < -30.0,
            "the gate only reached {:.1} dB",
            g.reduction_db()
        );
    }

    /// ZERO RANGE IS EXACTLY OFF — a range of nothing is a gain of one,
    /// whatever the threshold says. The same exact-off promise the lo-fi
    /// and the sheen make.
    #[test]
    fn zero_range_is_a_wire() {
        let mut g = core(GateParams {
            range_db: 0.0,
            ..GateParams::default()
        });
        let mut l = tone(-70.0, 200.0);
        let before = l.clone();
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        assert_eq!(l, before, "zero range altered the block");
    }

    /// THE ONE WORTH HAVING: attack OPENS and release CLOSES, so the two
    /// times must reach the kernel crossed. Measured, because a swap that
    /// came undone would still compile, still gate, and simply respond at
    /// the wrong speeds.
    #[test]
    fn the_gate_opens_at_its_attack_and_closes_at_its_release() {
        // A fast opening and a slow closing. If the swap were missing,
        // these would land the other way round.
        let params = GateParams {
            attack_ms: 1.0,
            release_ms: 500.0,
            ..GateParams::default()
        };

        // Opening: start shut on silence, then hit it with a loud tone
        // and see how long the gain takes to come most of the way up.
        let mut g = core(params);
        let mut quiet = tone(-90.0, 500.0);
        let mut q2 = quiet.clone();
        g.process(&mut quiet, &mut q2);
        assert!(g.reduction_db() < -30.0, "the gate did not shut first");

        let mut loud = tone(-6.0, 50.0);
        let mut l2 = loud.clone();
        let shut = g.reduction_db();
        g.process(&mut loud, &mut l2);
        let opened = g.reduction_db();
        assert!(
            opened > shut + 20.0,
            "1 ms of attack left the gate at {opened:.1} dB after 50 ms"
        );

        // Closing: from open, feed silence for the same 50 ms. With half
        // a second of release it must barely have moved.
        let mut g = core(params);
        let mut warm = tone(-6.0, 200.0);
        let mut w2 = warm.clone();
        g.process(&mut warm, &mut w2);
        let open = g.reduction_db();
        assert!(open > -1.0, "the gate did not open first: {open:.1} dB");

        let mut hush = tone(-90.0, 50.0);
        let mut h2 = hush.clone();
        g.process(&mut hush, &mut h2);
        let closing = g.reduction_db();
        assert!(
            closing > -12.0,
            "500 ms of release fell to {closing:.1} dB in 50 ms, which is the \
             attack time doing the closing"
        );
    }

    /// Split blocks are the same as one: the segment prologue and the
    /// per-sample state have to survive being cut anywhere.
    #[test]
    fn split_blocks_match_a_whole_one() {
        let params = GateParams::default();
        let signal = tone(-30.0, 100.0);

        let mut whole = signal.clone();
        let mut whole_r = signal.clone();
        core(params).process(&mut whole, &mut whole_r);

        let mut split = signal.clone();
        let mut split_r = signal.clone();
        let mut g = core(params);
        let cut = signal.len() / 3;
        let (a, b) = split.split_at_mut(cut);
        let (ar, br) = split_r.split_at_mut(cut);
        g.process(a, ar);
        g.process(b, br);

        assert_eq!(whole, split, "cutting the block changed the result");
    }

    /// Every table row reaches a field and comes back.
    #[test]
    fn every_row_round_trips() {
        let mut params = GateParams::default();
        for row in p::TABLE {
            let mid = (row.min + row.max) * 0.5;
            params.set(row.id, mid);
            assert_eq!(
                params.get(row.id),
                Some(mid),
                "`{}` did not come back",
                row.name
            );
        }
    }

    /// A gate arrives OPEN, so inserting one mid-note does not swallow
    /// what was already sounding.
    #[test]
    fn a_fresh_gate_is_open() {
        assert_eq!(core(GateParams::default()).reduction_db(), 0.0);
    }
}
