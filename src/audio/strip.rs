//! The strip — a mini channel: two shelves, an output stage, a switch.
//!
//! Wiring, not a kernel. Every piece of arithmetic below already exists:
//! [`EqBand`](crate::dsp::filters::EqBand) for the four shelves and
//! [`Preamp`](crate::audio::preamp::Preamp) for the colour. `preamp.rs`'s
//! header predicted this device in as many words — "it is a stage a
//! future effect device would want whole" — and this is that device,
//! taking it whole.
//!
//! # The order, and why it is the whole point
//!
//! ```text
//! low shelf -> high shelf -> [warm shelves] -> preamp -> trim
//! ```
//!
//! EVERYTHING IS PRE-DRIVE. The shelves and the switch do not merely
//! shape the tone; they change what the output stage's non-linearity is
//! fed. Lift the bottom and the bottom drives the clip harder, which is
//! more second harmonic on the bass specifically — the thing a console
//! does that the same curve applied afterwards cannot.
//!
//! `params::strip` argues it at length. `the_shelves_drive_the_stage`
//! measures it, and that test was checked against a build with the colour
//! moved to the FRONT before being trusted: with the EQ after the stage
//! the same boost gains 3.570x clean and 3.591x driven — the interaction
//! vanishes, which is precisely what it should do and what the test
//! catches.
//!
//! # What the warm switch is
//!
//! A fixed pair of shelves — a lift at the bottom, a softening at the
//! very top — and NOT a second copy of the preamp's own tilt, which is a
//! see-saw across the whole band pivoting at 1 kHz. This touches the
//! extremes only, so the two stack rather than duplicate.
//!
//! # Red zone
//!
//! `process` allocates nothing, locks nothing and cannot panic. The four
//! bands and the preamp are all in-place per-sample kernels with fixed
//! state, and every value has been clamped through `params::strip::TABLE`
//! on the way in.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::preamp::Preamp;
use crate::dsp::filters::{BandShape, EqBand};
use crate::params::strip as p;
use crate::params::{clamp as clamp_param, def};

/// A strip's editable values, in ENGINE units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StripParams {
    pub low_db: f32,
    pub high_db: f32,
    pub drive: f32,
    /// The switch as its wire INDEX, kept as f32 like every other engine
    /// value here — `SatParams::mode`'s doc gives the reason.
    pub warm: f32,
    pub out: f32,
}

impl Default for StripParams {
    fn default() -> Self {
        Self {
            low_db: def(p::TABLE, p::LOW).default,
            high_db: def(p::TABLE, p::HIGH).default,
            drive: def(p::TABLE, p::DRIVE).default,
            warm: def(p::TABLE, p::WARM).default,
            out: def(p::TABLE, p::OUT).default,
        }
    }
}

impl StripParams {
    /// This state's value for a wire id. One place an id becomes a field.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::LOW => self.low_db,
            p::HIGH => self.high_db,
            p::DRIVE => self.drive,
            p::WARM => self.warm,
            p::OUT => self.out,
            _ => return None,
        })
    }

    /// Set by wire id, ignoring anything the table does not describe.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = clamp_param(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::LOW => self.low_db = value,
            p::HIGH => self.high_db = value,
            p::DRIVE => self.drive = value,
            p::WARM => self.warm = value,
            p::OUT => self.out = value,
            _ => {}
        }
    }

    /// Whether the warm shelves are in the path.
    pub fn warm_on(&self) -> bool {
        self.warm.round() as u32 == p::WARM_ON
    }
}

/// The settings the kernels were last configured for.
///
/// Compared as a whole at the top of every segment so the reconfiguration
/// happens only when something moved — `GlueCore`'s prologue, for its
/// reason: `prepare` is transcendental work and a still knob should not
/// pay for it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    low_db: f32,
    high_db: f32,
    drive: f32,
    warm: bool,
}

impl Resolved {
    fn of(params: &StripParams) -> Self {
        Self {
            low_db: params.low_db,
            high_db: params.high_db,
            drive: params.drive,
            warm: params.warm_on(),
        }
    }
}

/// One strip: four shelves, an output stage, and the wiring between them.
///
/// Clone but not Copy — `Preamp` carries a noise source and is not Copy,
/// which is correct: a colour stage with a random element is not a value
/// you want silently duplicated.
#[derive(Debug, Clone)]
pub struct StripCore {
    params: StripParams,
    prepared: Resolved,
    sample_rate: f32,
    /// The user's two shelves, per channel.
    low: [EqBand; 2],
    high: [EqBand; 2],
    /// The warm switch's two, per channel. Always prepared; skipped by a
    /// branch when the switch is off, so turning it on does not have to
    /// wait for coefficients.
    warm_low: [EqBand; 2],
    warm_high: [EqBand; 2],
    preamp: Preamp,
}

impl StripCore {
    /// Green zone: build a strip at a sample rate, from clamped values.
    pub fn new(sample_rate: f32, params: &StripParams) -> Self {
        let mut params = *params;
        // Every row through the table on the way in, so a stale project
        // file cannot smuggle an out-of-range setting past the callback.
        for row in p::TABLE {
            if let Some(value) = params.get(row.id) {
                params.set(row.id, value);
            }
        }
        let mut core = Self {
            params,
            prepared: Resolved::of(&params),
            sample_rate,
            low: [EqBand::new(); 2],
            high: [EqBand::new(); 2],
            warm_low: [EqBand::new(); 2],
            warm_high: [EqBand::new(); 2],
            preamp: Preamp::new(),
        };
        core.preamp.prepare(sample_rate);
        core.preamp.set_amount(params.drive);
        core.tune_user();
        core.tune_warm();
        core
    }

    /// The user's shelves, from the current params.
    fn tune_user(&mut self) {
        for ch in 0..2 {
            self.low[ch].prepare(
                self.sample_rate,
                p::LOW_HZ,
                p::SHELF_Q,
                self.params.low_db,
                BandShape::LowShelf,
            );
            self.high[ch].prepare(
                self.sample_rate,
                p::HIGH_HZ,
                p::SHELF_Q,
                self.params.high_db,
                BandShape::HighShelf,
            );
        }
    }

    /// The warm switch's shelves. Fixed figures, so this runs once.
    fn tune_warm(&mut self) {
        for ch in 0..2 {
            self.warm_low[ch].prepare(
                self.sample_rate,
                p::WARM_LOW_HZ,
                p::SHELF_Q,
                p::WARM_LOW_DB,
                BandShape::LowShelf,
            );
            self.warm_high[ch].prepare(
                self.sample_rate,
                p::WARM_HIGH_HZ,
                p::SHELF_Q,
                p::WARM_HIGH_DB,
                BandShape::HighShelf,
            );
        }
    }

    /// Red zone: one door for every letter, so an id cannot be routed by
    /// two different opinions about what it is.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    /// This strip's values, for the app to read back.
    pub fn params(&self) -> StripParams {
        self.params
    }

    /// Green zone: forget the history, keep the settings.
    pub fn reset(&mut self) {
        for ch in 0..2 {
            self.low[ch].reset();
            self.high[ch].reset();
            self.warm_low[ch].reset();
            self.warm_high[ch].reset();
        }
        self.preamp.reset();
    }

    /// Stated by the stage itself, so the figure the graph compensates
    /// for and the delay the device holds cannot drift apart.
    pub fn latency(&self) -> usize {
        self.preamp.latency()
    }

    /// Whether the colour is currently a wire. The shelves are not part
    /// of this question: at zero drive the strip is still an equaliser.
    pub fn colour_bypassed(&self) -> bool {
        self.preamp.is_bypassed()
    }

    /// Red zone: run a stereo pair through the strip, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;

        // --- the segment prologue: settings, only if they moved --------
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            if want.low_db != self.prepared.low_db || want.high_db != self.prepared.high_db {
                self.tune_user();
            }
            if want.drive != self.prepared.drive {
                self.preamp.set_amount(want.drive);
            }
            self.prepared = want;
        }
        let warm = self.params.warm_on();

        // --- the chain, in order ---------------------------------------
        //
        // The shelves run over the whole block per band rather than all
        // bands per sample — `Cascade`'s choice, for its reason: one
        // section's coefficients and state stay in registers for a block
        // instead of four sets being reloaded every sample.
        self.low[0].process(l);
        self.high[0].process(l);
        if warm {
            self.warm_low[0].process(l);
            self.warm_high[0].process(l);
        }
        if stereo {
            let r = &mut r[..n];
            self.low[1].process(r);
            self.high[1].process(r);
            if warm {
                self.warm_low[1].process(r);
                self.warm_high[1].process(r);
            }
        }

        // THE COLOUR, and it comes last of the processing for the reason
        // the module header gives: everything above is feeding it.
        if stereo {
            let (r, _) = r.split_at_mut(n);
            self.preamp.process(l, r);
        } else {
            self.preamp.process(l, &mut []);
        }

        // The trim, after everything — it is a level, not a shape, and a
        // level inside the clip would be a second drive control.
        let trim = self.params.out;
        if trim != 1.0 {
            for s in l.iter_mut() {
                *s *= trim;
            }
            if stereo {
                for s in r.iter_mut().take(n) {
                    *s *= trim;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(params: StripParams) -> StripCore {
        StripCore::new(FS, &params)
    }

    /// A tone at `db`, `ms` long.
    fn tone(hz: f32, db: f32, ms: f32) -> Vec<f32> {
        let n = (FS * ms / 1000.0) as usize;
        let amp = 10.0f32.powf(db / 20.0);
        (0..n)
            .map(|i| (i as f32 / FS * hz * std::f32::consts::TAU).sin() * amp)
            .collect()
    }

    /// Peak of the settled second half, which skips the filters' onset.
    fn settled_peak(x: &[f32]) -> f32 {
        x[x.len() / 2..].iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    /// Run a tone through a strip and return the output.
    fn run(params: StripParams, hz: f32, db: f32) -> Vec<f32> {
        let mut l = tone(hz, db, 200.0);
        let mut r = l.clone();
        core(params).process(&mut l, &mut r);
        l
    }

    /// Everything at rest and the drive at zero is a clean two-band EQ —
    /// flat, and exactly so, because `Preamp` promises a bit-exact bypass
    /// at amount zero and the shelves at 0 dB are unity.
    #[test]
    fn flat_and_undriven_is_a_wire() {
        let params = StripParams {
            drive: 0.0,
            ..StripParams::default()
        };
        assert!(core(params).colour_bypassed(), "zero drive is not bypassed");
        let quiet = tone(1_000.0, -12.0, 50.0);
        let mut l = quiet.clone();
        let mut r = quiet.clone();
        core(params).process(&mut l, &mut r);
        for (i, (got, want)) in l.iter().zip(quiet.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "sample {i}: {want} came out {got}"
            );
        }
    }

    /// The shelves do what they say, in the band they say it in.
    #[test]
    fn the_shelves_lift_their_own_ends() {
        let flat = StripParams {
            drive: 0.0,
            ..StripParams::default()
        };
        let lifted = StripParams {
            low_db: 6.0,
            drive: 0.0,
            ..StripParams::default()
        };
        let low_before = settled_peak(&run(flat, 60.0, -20.0));
        let low_after = settled_peak(&run(lifted, 60.0, -20.0));
        assert!(
            low_after > low_before * 1.6,
            "60 Hz went from {low_before:.4} to {low_after:.4} on a +6 dB shelf"
        );

        // And leaves the far end alone.
        let top_before = settled_peak(&run(flat, 4_000.0, -20.0));
        let top_after = settled_peak(&run(lifted, 4_000.0, -20.0));
        assert!(
            (top_after / top_before - 1.0).abs() < 0.15,
            "the low shelf moved 4 kHz from {top_before:.4} to {top_after:.4}"
        );
    }

    /// The warm switch lifts the bottom and softens the top, and does
    /// nothing to the middle — which is what makes it a different thing
    /// from the preamp's own tilt.
    #[test]
    fn warm_touches_the_ends_and_leaves_the_middle() {
        let off = StripParams {
            drive: 0.0,
            ..StripParams::default()
        };
        let on = StripParams {
            warm: p::WARM_ON as f32,
            drive: 0.0,
            ..StripParams::default()
        };
        let at = |hz: f32, params: StripParams| settled_peak(&run(params, hz, -20.0));

        assert!(
            at(50.0, on) > at(50.0, off) * 1.1,
            "warm did not lift 50 Hz: {:.4} -> {:.4}",
            at(50.0, off),
            at(50.0, on)
        );
        assert!(
            at(16_000.0, on) < at(16_000.0, off) * 0.95,
            "warm did not soften 16 kHz: {:.4} -> {:.4}",
            at(16_000.0, off),
            at(16_000.0, on)
        );
        assert!(
            (at(1_000.0, on) / at(1_000.0, off) - 1.0).abs() < 0.05,
            "warm moved 1 kHz, which is the mids it is supposed to leave"
        );
    }

    /// THE ONE WORTH HAVING: the shelves are PRE-DRIVE, so the same boost
    /// buys more non-linearity when the stage is being driven.
    ///
    /// Measured as the extra output the boost produces: through a linear
    /// path that ratio is the shelf's own gain and nothing else, whatever
    /// the rest of the chain is doing. Through a saturating one it is
    /// LESS, because the clip gives back diminishing returns — and the
    /// gap between the two is the interaction the module header claims.
    /// An EQ sitting after the colour would show the same ratio both
    /// times.
    #[test]
    fn the_shelves_drive_the_stage() {
        let ratio_at = |drive: f32| {
            let flat = StripParams {
                drive,
                ..StripParams::default()
            };
            let lifted = StripParams {
                low_db: 12.0,
                drive,
                ..StripParams::default()
            };
            // Hot enough to reach the clip when it is switched in.
            settled_peak(&run(lifted, 60.0, -3.0)) / settled_peak(&run(flat, 60.0, -3.0))
        };
        let clean = ratio_at(0.0);
        let driven = ratio_at(1.0);
        assert!(
            driven < clean * 0.95,
            "the boost gained {clean:.3}x clean and {driven:.3}x driven, so the \
             shelves are not feeding the stage"
        );
    }

    /// Split blocks are the same as one: the filters' state and the
    /// segment prologue have to survive being cut anywhere.
    #[test]
    fn split_blocks_match_a_whole_one() {
        let params = StripParams {
            low_db: 4.0,
            high_db: -3.0,
            warm: p::WARM_ON as f32,
            ..StripParams::default()
        };
        let signal = tone(500.0, -12.0, 100.0);

        let mut whole = signal.clone();
        let mut whole_r = signal.clone();
        core(params).process(&mut whole, &mut whole_r);

        let mut split = signal.clone();
        let mut split_r = signal.clone();
        let mut c = core(params);
        let cut = signal.len() / 3;
        let (a, b) = split.split_at_mut(cut);
        let (ar, br) = split_r.split_at_mut(cut);
        c.process(a, ar);
        c.process(b, br);

        assert_eq!(whole, split, "cutting the block changed the result");
    }

    /// Silence in, silence out — which the preamp's gated hiss is what
    /// makes true, and which a strip carrying a permanent noise floor
    /// would break.
    #[test]
    fn silence_stays_silent() {
        let mut l = vec![0.0f32; 4_096];
        let mut r = vec![0.0f32; 4_096];
        core(StripParams::default()).process(&mut l, &mut r);
        assert!(l.iter().all(|s| *s == 0.0), "the strip hissed into silence");
    }

    /// Every table row reaches a field and comes back.
    #[test]
    fn every_row_round_trips() {
        let mut params = StripParams::default();
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
}
