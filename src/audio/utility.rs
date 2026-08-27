//! Gain, placement and the stereo field — the effect half of
//! `Node::Utility`.
//!
//! Node-side wiring with no kernel of its own worth the name: the two
//! filters come from [`crate::dsp::filters`], and everything else here is
//! six multiplies and an add. That is the device. It has no tone, and the
//! whole of its job is to leave one alone.
//!
//! # The chain
//!
//! ```text
//!  in ── dc ── [ channel × phase ] ── width ── bass mono ── pan ── gain ── out
//!               one 2x2 matrix          mid/side, one shared pair
//! ```
//!
//! [`crate::params::utility`] states why the stages sit in that order.
//! What this file adds is how they are made STEPLESS, which is the whole
//! of the difficulty:
//!
//! - **The routing is a matrix, and the matrix ramps.** Channel mode and
//!   phase invert are both switches, and a switch on a signal path is a
//!   click — flipping a side's sign is a jump of twice the sample. So
//!   they are folded into one 2x2 matrix whose four coefficients ramp
//!   across the block, and a phase flip becomes a fade THROUGH zero over
//!   a few milliseconds rather than a step. Nothing else in the file is
//!   subtle; this is the reason the routing is not a `match` per sample.
//! - **Width, pan and gain ramp** for the ordinary reason every level in
//!   this engine does.
//! - **The crossover does not ramp**, and cannot: it is a filter, and its
//!   corner is a coefficient. It is rebuilt per SEGMENT, like every other
//!   transcendental in the graph, and only when it moved.
//!
//! # Unity is exact
//!
//! At its defaults this node is a wire, sample for sample: the matrix is
//! the identity, width 1.0 reconstructs `mid ± side` to the bit, the
//! crossover is off, pan is centred under a balance law whose centre is
//! 1.0 on both sides, and the trim is 0 dB. That is tested, and it is the
//! one property a utility device may not get wrong — inserting one must
//! not change a mix.
//!
//! # Red zone
//!
//! Everything except [`UtilityCore::new`] runs in the audio callback: no
//! allocation, no locks, no panic paths, no unbounded loops.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::filters::{DcBlocker, OnePole};
use crate::params::utility as p;

/// The device's editable state, in ENGINE units — dB, a signed pan, a
/// width factor, Hz, and the three switches as the float indices every
/// table row is.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UtilityParams {
    pub gain_db: f32,
    pub pan: f32,
    pub width: f32,
    pub mono_hz: f32,
    /// An index into [`crate::params::utility::PHASE_NAMES`].
    pub phase: f32,
    /// An index into [`crate::params::utility::CHANNEL_NAMES`].
    pub channel: f32,
    /// Above 0.5 the DC blocker runs.
    pub dc: f32,
}

impl Default for UtilityParams {
    fn default() -> Self {
        let d = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            gain_db: d(p::GAIN),
            pan: d(p::PAN),
            width: d(p::WIDTH),
            mono_hz: d(p::MONO_HZ),
            phase: d(p::PHASE),
            channel: d(p::CHANNEL),
            dc: d(p::DC),
        }
    }
}

impl UtilityParams {
    /// This patch's value for a wire id, or `None` for one it does not
    /// have — how a target aimed at the wrong device is refused.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::GAIN => self.gain_db,
            p::PAN => self.pan,
            p::WIDTH => self.width,
            p::MONO_HZ => self.mono_hz,
            p::PHASE => self.phase,
            p::CHANNEL => self.channel,
            p::DC => self.dc,
            _ => return None,
        })
    }

    /// Write a wire id's value. Unknown ids are dropped, never guessed.
    pub fn set(&mut self, param: u32, value: f32) {
        match param {
            p::GAIN => self.gain_db = value,
            p::PAN => self.pan = value,
            p::WIDTH => self.width = value,
            p::MONO_HZ => self.mono_hz = value,
            p::PHASE => self.phase = value,
            p::CHANNEL => self.channel = value,
            p::DC => self.dc = value,
            _ => {}
        }
    }

    pub fn phase_index(self) -> u32 {
        p::index(self.phase, p::PHASE_NAMES.len())
    }

    pub fn channel_index(self) -> u32 {
        p::index(self.channel, p::CHANNEL_NAMES.len())
    }

    pub fn blocking_dc(self) -> bool {
        self.dc >= 0.5
    }
}

/// The routing stage as one 2x2 matrix: what each output side takes from
/// each input side, sign included.
///
/// Channel mode and phase invert are the SAME arithmetic — a linear map
/// from two inputs to two outputs — so they are one object rather than
/// two `match`es in the sample loop. Their composition is
/// `diag(phase) × channel`: phase inverts what the routing already
/// decided, which is why "ø L" means the left OUTPUT however the sides
/// were swapped to get there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Route {
    /// `[left from left, left from right, right from left, right from right]`.
    m: [f32; 4],
}

impl Route {
    fn of(params: &UtilityParams) -> Self {
        // `left`/`right` FOLD to one source on both outputs rather than
        // muting the other side — see `CHANNEL_NAMES`. Soloing one side
        // of a file to check it is the ordinary use, and hearing it out
        // of one speaker while you do is not.
        let channel = match params.channel_index() {
            p::CHANNEL_SWAP => [0.0, 1.0, 1.0, 0.0],
            p::CHANNEL_LEFT => [1.0, 0.0, 1.0, 0.0],
            p::CHANNEL_RIGHT => [0.0, 1.0, 0.0, 1.0],
            _ => [1.0, 0.0, 0.0, 1.0],
        };
        let (sl, sr) = p::phase_signs(params.phase_index());
        Self {
            m: [
                channel[0] * sl,
                channel[1] * sl,
                channel[2] * sr,
                channel[3] * sr,
            ],
        }
    }
}

/// What the settings resolve to once the switches have been read and the
/// non-finite ones refused. Held so the segment prologue can tell whether
/// the crossover actually moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    mono_hz: f32,
    mono_off: bool,
}

impl Resolved {
    fn of(params: &UtilityParams) -> Self {
        let mono_hz = clamp_or_default(p::MONO_HZ, params.mono_hz);
        Self {
            mono_hz,
            mono_off: p::mono_off(mono_hz),
        }
    }
}

/// A table row's value, with non-finite input refused rather than
/// clamped.
///
/// NOT `ParamDef::clamp` alone: `f32::clamp` returns NaN for a NaN input,
/// so a poisoned value walks straight through a range check and into the
/// signal. RON round-trips NaN literals, so a hand-edited or corrupt
/// project is a real way to get one — and one NaN in a stereo matrix
/// silences both channels for the rest of the session.
fn clamp_or_default(id: u32, value: f32) -> f32 {
    let def = crate::params::def(p::TABLE, id);
    if value.is_finite() {
        def.clamp(value)
    } else {
        def.default
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The device's state, boxed by the node so `Node` stays lean.
pub struct UtilityCore {
    /// One blocker per side. Block-form: this is the only stage that
    /// wants the whole segment at once, and it runs before the sample
    /// loop for exactly that reason.
    dc_l: DcBlocker,
    dc_r: DcBlocker,
    /// The bass-mono crossover, on the SIDE signal only. One signal, so
    /// one filter — the side is mono by construction.
    ///
    /// ONE pole, and that is a decision made against a measurement. The
    /// obvious improvement is to cascade a second for a steeper slope,
    /// and it makes the device WORSE at its job: what stays in the side
    /// is `side - lowpass(side)`, which is a vector subtraction, and a
    /// second pole doubles the phase lag it subtracts at. Measured at a
    /// 200 Hz corner, a 60 Hz note kept 29 % of its amplitude in the
    /// sides through one pole and 55 % through two. The lag put the
    /// energy back.
    ///
    /// First order is the one order where the pair is complementary in
    /// PHASE as well as amplitude — low and high sum back to the input
    /// exactly, with no crossing bump and no rotation — and phase
    /// coherence in the bottom octaves is the entire reason anyone
    /// centres their bass. The slope is gentle, so the corner is set
    /// well ABOVE what is being collapsed; the range goes to 500 Hz for
    /// that reason and not because anyone wants 500 Hz mono.
    mono: OnePole,
    /// The last corner `prepare` was called with, so a parked crossover
    /// rebuilds nothing.
    prepared: Resolved,
    params: UtilityParams,
    /// Where the last segment left the four ramped values. Each is the
    /// FROM end of this segment's ramp, so a control that moved between
    /// blocks glides rather than steps.
    route: Route,
    width: f32,
    pan: f32,
    gain: f32,
    /// Whether the blocker ran last segment. A stage switched on carries
    /// its old history into the new signal unless it is cleared, and that
    /// history is a step at the top of the block.
    dc_running: bool,
    sample_rate: f32,
}

impl UtilityCore {
    /// GREEN ZONE. Builds both filters; nothing here is called again from
    /// the audio thread.
    pub fn new(sample_rate: f32, params: &UtilityParams) -> Self {
        let resolved = Resolved::of(params);
        let mut dc_l = DcBlocker::new();
        dc_l.prepare(sample_rate);
        let mut dc_r = DcBlocker::new();
        dc_r.prepare(sample_rate);
        let mut mono = OnePole::new();
        mono.prepare(sample_rate, resolved.mono_hz);
        Self {
            dc_l,
            dc_r,
            mono,
            prepared: resolved,
            params: *params,
            // The ramped values START where the patch says, not at unity:
            // a device that opened at defaults and glided to its saved
            // settings would sweep every loaded project's first block.
            route: Route::of(params),
            width: clamp_or_default(p::WIDTH, params.width),
            pan: clamp_or_default(p::PAN, params.pan),
            gain: db_to_gain(clamp_or_default(p::GAIN, params.gain_db)),
            dc_running: params.blocking_dc(),
            sample_rate,
        }
    }

    /// Red zone: a letter. Stored now, resolved at the top of the next
    /// segment — rebuilding a coefficient per letter would mean doing it
    /// several times for one mouse move.
    pub fn set_param(&mut self, param: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        self.params.set(param, value);
    }

    /// Red zone: a seek. Forget the filters' history and LAND every ramp,
    /// so the new position starts where its settings say rather than
    /// gliding the old position's in.
    pub fn snap(&mut self) {
        self.dc_l.reset();
        self.dc_r.reset();
        self.mono.reset();
        self.route = Route::of(&self.params);
        self.width = clamp_or_default(p::WIDTH, self.params.width);
        self.pan = clamp_or_default(p::PAN, self.params.pan);
        self.gain = db_to_gain(clamp_or_default(p::GAIN, self.params.gain_db));
        self.dc_running = self.params.blocking_dc();
    }

    /// Red zone: one segment, in place. `r` shorter than `l` is a mono
    /// caller; the stereo stages then have nothing to do and the node
    /// falls back to trim and nothing else.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;

        // --- the segment prologue: the one coefficient, if it moved ----
        let want = Resolved::of(&self.params);
        if want.mono_hz != self.prepared.mono_hz {
            self.mono.prepare(self.sample_rate, want.mono_hz);
        }
        if want.mono_off && !self.prepared.mono_off {
            // Switched out. Clear it now rather than on the way back in,
            // so the state cannot age across a passage of the song and
            // then arrive as a thump when the stage returns.
            self.mono.reset();
        }
        self.prepared = want;

        // --- dc, which wants the whole segment at once -----------------
        let blocking = self.params.blocking_dc();
        if blocking {
            if !self.dc_running {
                self.dc_l.reset();
                self.dc_r.reset();
            }
            self.dc_l.process(l);
            if stereo && let Some(r) = r.get_mut(..n) {
                self.dc_r.process(r);
            }
        }
        self.dc_running = blocking;

        // --- what everything ramps TO ----------------------------------
        let route_to = Route::of(&self.params);
        let width_to = clamp_or_default(p::WIDTH, self.params.width);
        let pan_to = clamp_or_default(p::PAN, self.params.pan);
        let gain_to = db_to_gain(clamp_or_default(p::GAIN, self.params.gain_db));

        let inv = 1.0 / n as f32;
        let mut m = self.route.m;
        let step = [
            (route_to.m[0] - m[0]) * inv,
            (route_to.m[1] - m[1]) * inv,
            (route_to.m[2] - m[2]) * inv,
            (route_to.m[3] - m[3]) * inv,
        ];
        let (mut width, width_step) = (self.width, (width_to - self.width) * inv);
        let (mut pan, pan_step) = (self.pan, (pan_to - self.pan) * inv);
        let (mut gain, gain_step) = (self.gain, (gain_to - self.gain) * inv);

        // The mid/side stage, SKIPPED when it would be the identity.
        //
        // Not an optimisation — a correctness requirement. `mid ± side`
        // over `(a±b)/2` is algebraically exact and in f32 it is not:
        // the halving and the re-addition each round, and the round trip
        // comes back a few ulps out. A few ulps is inaudible and it is
        // also not a WIRE, and this device's one promise is that
        // inserting it at its defaults changes nothing at all. Width is
        // checked at BOTH ends of the ramp, so a block in which it moves
        // still goes the long way round.
        let field = !(width == 1.0 && width_to == 1.0 && want.mono_off);
        let mono_off = want.mono_off;

        // --- the loop ---------------------------------------------------
        for i in 0..n {
            let Some(left) = l.get(i).copied() else { break };
            let right = if stereo {
                r.get(i).copied().unwrap_or(left)
            } else {
                left
            };

            // Routing and phase, as one map.
            let mut a = left * m[0] + right * m[1];
            let mut b = left * m[2] + right * m[3];

            if stereo && field {
                let mid = (a + b) * 0.5;
                let mut side = (a - b) * 0.5;
                side *= width;
                // Bass mono: what the crossover does NOT pass stays in
                // the side, so the low end collapses to the middle and
                // everything above it keeps the width it was given.
                // Written as a subtraction rather than as a highpass
                // kernel because THAT is what makes the pair exactly
                // complementary — whatever leaves the side arrives in
                // the mid, at any order and any corner, with no gap and
                // no bump at the crossing.
                if !mono_off {
                    side = self.mono.tick_highpass(side);
                }
                a = mid + side;
                b = mid - side;
            }

            // Pan, under the BALANCE law `Node::Pan` uses on a stereo
            // input: centre passes both sides untouched, and either
            // extreme attenuates only the side it is moving away from.
            // The same law in both places, because a pan knob on a track
            // header and a pan cell on this card have to mean the same
            // thing.
            let left_gain = if pan <= 0.0 {
                1.0
            } else {
                (pan * std::f32::consts::FRAC_PI_2).cos()
            };
            let right_gain = if pan >= 0.0 {
                1.0
            } else {
                (-pan * std::f32::consts::FRAC_PI_2).cos()
            };

            if let Some(slot) = l.get_mut(i) {
                *slot = a * left_gain * gain;
            }
            if stereo && let Some(slot) = r.get_mut(i) {
                *slot = b * right_gain * gain;
            }

            for (value, step) in m.iter_mut().zip(step.iter()) {
                *value += *step;
            }
            width += width_step;
            pan += pan_step;
            gain += gain_step;
        }

        // Land on the targets exactly rather than on whatever the ramps
        // accumulated to: n multiply-adds of a step drift, and a value
        // that never quite arrives makes the next block ramp again from
        // almost-there forever.
        self.route = route_to;
        self.width = width_to;
        self.pan = pan_to;
        self.gain = gain_to;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn core(params: UtilityParams) -> UtilityCore {
        UtilityCore::new(SR, &params)
    }

    /// A signal with something in the mid, something in the side, and an
    /// offset — enough that every stage has something to change.
    fn signal(n: usize) -> (Vec<f32>, Vec<f32>) {
        let l: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 0.11).sin() * 0.5 + (i as f32 * 0.017).sin() * 0.2)
            .collect();
        let r: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 0.11).sin() * 0.5 - (i as f32 * 0.017).sin() * 0.2)
            .collect();
        (l, r)
    }

    /// THE property. Inserted and untouched, this device is a wire —
    /// bit for bit, not "close enough": a utility that coloured a track
    /// by existing would be worse than no utility at all.
    #[test]
    fn at_its_defaults_it_is_a_wire() {
        let mut core = core(UtilityParams::default());
        let (mut l, mut r) = signal(512);
        let (want_l, want_r) = (l.clone(), r.clone());
        core.process(&mut l, &mut r);
        assert_eq!(l, want_l, "the left channel was not passed through");
        assert_eq!(r, want_r, "the right channel was not passed through");
    }

    /// And it stays a wire across a block boundary — the ramps land on
    /// their targets rather than creeping toward them.
    #[test]
    fn it_is_still_a_wire_on_the_second_block() {
        let mut core = core(UtilityParams::default());
        for _ in 0..4 {
            let (mut l, mut r) = signal(128);
            let (want_l, want_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            assert_eq!(l, want_l);
            assert_eq!(r, want_r);
        }
    }

    #[test]
    fn width_zero_is_mono_and_width_two_doubles_the_side() {
        for (width, side_scale) in [(0.0, 0.0), (2.0, 2.0)] {
            let mut core = core(UtilityParams {
                width,
                ..Default::default()
            });
            let (mut l, mut r) = signal(256);
            let (in_l, in_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            // The tail, after the ramp has arrived.
            for i in 200..256 {
                let mid = (in_l[i] + in_r[i]) * 0.5;
                let side = (in_l[i] - in_r[i]) * 0.5 * side_scale;
                assert!((l[i] - (mid + side)).abs() < 1e-5, "left at {i}");
                assert!((r[i] - (mid - side)).abs() < 1e-5, "right at {i}");
            }
        }
    }

    #[test]
    fn phase_inverts_the_side_it_names() {
        for (index, (sl, sr)) in [
            (p::PHASE_NONE, (1.0, 1.0)),
            (p::PHASE_L, (-1.0, 1.0)),
            (p::PHASE_R, (1.0, -1.0)),
            (p::PHASE_BOTH, (-1.0, -1.0)),
        ] {
            let mut core = core(UtilityParams {
                phase: index as f32,
                ..Default::default()
            });
            let (mut l, mut r) = signal(256);
            let (in_l, in_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            for i in 200..256 {
                assert!((l[i] - in_l[i] * sl).abs() < 1e-5, "index {index} left");
                assert!((r[i] - in_r[i] * sr).abs() < 1e-5, "index {index} right");
            }
        }
    }

    #[test]
    fn the_channel_modes_route_what_they_say() {
        let cases: [(u32, fn(f32, f32) -> (f32, f32)); 4] = [
            (p::CHANNEL_STEREO, |l, r| (l, r)),
            (p::CHANNEL_SWAP, |l, r| (r, l)),
            (p::CHANNEL_LEFT, |l, _| (l, l)),
            (p::CHANNEL_RIGHT, |_, r| (r, r)),
        ];
        for (index, want) in cases {
            let mut core = core(UtilityParams {
                channel: index as f32,
                ..Default::default()
            });
            let (mut l, mut r) = signal(256);
            let (in_l, in_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            for i in 200..256 {
                let (wl, wr) = want(in_l[i], in_r[i]);
                assert!((l[i] - wl).abs() < 1e-5, "mode {index} left at {i}");
                assert!((r[i] - wr).abs() < 1e-5, "mode {index} right at {i}");
            }
        }
    }

    /// A switch on a signal path is a click, and the matrix ramp is what
    /// stops it: no two neighbouring samples may jump by anything like
    /// the size of the flip itself.
    #[test]
    fn flipping_the_phase_fades_rather_than_steps() {
        let mut core = core(UtilityParams::default());
        let (mut l, mut r) = signal(512);
        core.process(&mut l, &mut r);
        core.set_param(p::PHASE, p::PHASE_BOTH as f32);
        let (mut l, mut r) = signal(512);
        core.process(&mut l, &mut r);
        // Twice the signal is what a hard flip would cost; a fade over
        // 512 samples costs a fraction of one sample per step.
        for pair in l.windows(2) {
            assert!(
                (pair[1] - pair[0]).abs() < 0.1,
                "the flip stepped by {}",
                (pair[1] - pair[0]).abs()
            );
        }
        for pair in r.windows(2) {
            assert!((pair[1] - pair[0]).abs() < 0.1);
        }
    }

    /// Bass mono centres the low end and leaves the rest alone. Measured
    /// as ENERGY in the side signal: a low tone panned hard should end up
    /// in the middle, and a high one should not move.
    ///
    /// The corner sits WELL above the tone it is collapsing, which is how
    /// a first-order crossover is used — see the field on `mono`. A 60 Hz
    /// note under a 500 Hz corner is three octaves down the slope.
    #[test]
    fn bass_mono_collapses_the_low_side_only() {
        for (hz, collapses) in [(60.0f32, true), (8_000.0f32, false)] {
            let mut core = core(UtilityParams {
                mono_hz: 500.0,
                ..Default::default()
            });
            let n = 24_000;
            // Hard left: all mid, all side, equal parts.
            let tone: Vec<f32> = (0..n)
                .map(|i| (i as f32 * hz * std::f32::consts::TAU / SR).sin() * 0.5)
                .collect();
            let mut l = tone.clone();
            let mut r = vec![0.0; n];
            core.process(&mut l, &mut r);
            // The second half, after the filter has settled.
            let side: f32 = (n / 2..n).map(|i| (l[i] - r[i]).powi(2)).sum::<f32>();
            let mid: f32 = (n / 2..n).map(|i| (l[i] + r[i]).powi(2)).sum::<f32>();
            if collapses {
                assert!(
                    side < mid * 0.05,
                    "{hz} Hz kept {side} of side against {mid}"
                );
            } else {
                assert!(
                    (side - mid).abs() < mid * 0.05,
                    "{hz} Hz moved: side {side}, mid {mid}"
                );
            }
        }
    }

    /// At the floor the crossover is OFF, and off means untouched — not
    /// "a filter at 20 Hz you cannot hear".
    #[test]
    fn the_crossover_floor_is_a_bypass() {
        let mut core = core(UtilityParams {
            mono_hz: p::MONO_MIN_HZ,
            ..Default::default()
        });
        let (mut l, mut r) = signal(512);
        let (want_l, want_r) = (l.clone(), r.clone());
        core.process(&mut l, &mut r);
        assert_eq!(l, want_l);
        assert_eq!(r, want_r);
    }

    #[test]
    fn the_trim_is_decibels() {
        for db in [-12.0f32, -6.0, 6.0, 12.0] {
            let mut core = core(UtilityParams {
                gain_db: db,
                ..Default::default()
            });
            let (mut l, mut r) = signal(512);
            let (in_l, _) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            let want = 10f32.powf(db / 20.0);
            for i in 400..512 {
                assert!((l[i] - in_l[i] * want).abs() < 1e-4, "{db} dB at {i}");
            }
        }
    }

    /// The balance law, at both extremes and in the middle — and the
    /// middle is the one that matters, because a centred pan must be the
    /// wire the defaults promise.
    #[test]
    fn pan_is_a_balance_and_its_centre_is_unity() {
        for (pan, want_l, want_r) in [(0.0f32, 1.0f32, 1.0f32), (1.0, 0.0, 1.0), (-1.0, 1.0, 0.0)] {
            let mut core = core(UtilityParams {
                pan,
                ..Default::default()
            });
            let (mut l, mut r) = signal(512);
            let (in_l, in_r) = (l.clone(), r.clone());
            core.process(&mut l, &mut r);
            for i in 400..512 {
                assert!((l[i] - in_l[i] * want_l).abs() < 1e-5, "pan {pan} left");
                assert!((r[i] - in_r[i] * want_r).abs() < 1e-5, "pan {pan} right");
            }
        }
    }

    /// The DC blocker removes an offset, and only when it is asked to.
    #[test]
    fn dc_is_removed_only_when_it_is_switched_on() {
        for on in [false, true] {
            let mut core = core(UtilityParams {
                dc: if on { 1.0 } else { 0.0 },
                ..Default::default()
            });
            let n = 24_000;
            let mut l = vec![0.5f32; n];
            let mut r = vec![0.5f32; n];
            core.process(&mut l, &mut r);
            let tail: f32 = (n - 100..n).map(|i| l[i]).sum::<f32>() / 100.0;
            if on {
                assert!(tail.abs() < 0.01, "the offset survived at {tail}");
            } else {
                assert!((tail - 0.5).abs() < 1e-4, "the offset was touched: {tail}");
            }
        }
    }

    /// A mono caller gets the trim and nothing that needs two channels —
    /// and nothing reads past the end of the short slice.
    #[test]
    fn a_mono_caller_gets_the_trim_and_no_panic() {
        let mut core = core(UtilityParams {
            gain_db: -6.0,
            width: 0.0,
            channel: p::CHANNEL_SWAP as f32,
            ..Default::default()
        });
        let mut l = vec![1.0f32; 256];
        let mut r: Vec<f32> = Vec::new();
        core.process(&mut l, &mut r);
        let want = 10f32.powf(-6.0 / 20.0);
        for s in &l[200..] {
            assert!((s - want).abs() < 1e-4, "mono trim gave {s}");
        }
    }

    /// A poisoned project cannot reach the signal. RON round-trips NaN,
    /// and one of them in a stereo matrix silences both channels forever.
    #[test]
    fn a_non_finite_setting_is_refused_not_clamped() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut core = core(UtilityParams {
                gain_db: bad,
                pan: bad,
                width: bad,
                mono_hz: bad,
                phase: bad,
                channel: bad,
                dc: bad,
            });
            let (mut l, mut r) = signal(256);
            core.process(&mut l, &mut r);
            assert!(
                l.iter().chain(r.iter()).all(|s| s.is_finite()),
                "a non-finite setting reached the signal"
            );
            // And a letter carrying one is dropped rather than stored.
            core.set_param(p::GAIN, bad);
            core.process(&mut l, &mut r);
            assert!(l.iter().chain(r.iter()).all(|s| s.is_finite()));
        }
    }

    /// Splitting a block in two must give the same samples as processing
    /// it whole — the ramps are spread over the segment, so this is the
    /// test that catches one that resets or double-counts.
    #[test]
    fn a_settled_core_is_split_block_equivalent() {
        let settings = UtilityParams {
            gain_db: -3.0,
            pan: 0.3,
            width: 1.6,
            mono_hz: 150.0,
            phase: p::PHASE_R as f32,
            channel: p::CHANNEL_SWAP as f32,
            dc: 1.0,
        };
        let (base_l, base_r) = signal(512);

        let mut whole = core(settings);
        let (mut wl, mut wr) = (base_l.clone(), base_r.clone());
        whole.process(&mut wl, &mut wr);

        let mut split = core(settings);
        let (mut sl, mut sr) = (base_l.clone(), base_r.clone());
        {
            let (l_a, l_b) = sl.split_at_mut(200);
            let (r_a, r_b) = sr.split_at_mut(200);
            split.process(l_a, r_a);
            split.process(l_b, r_b);
        }
        for i in 0..512 {
            assert!(
                (wl[i] - sl[i]).abs() < 1e-5 && (wr[i] - sr[i]).abs() < 1e-5,
                "sample {i} differs: {} vs {}",
                wl[i],
                sl[i]
            );
        }
    }

    /// A seek lands every ramp and forgets both filters, so the new
    /// position starts where its settings say.
    #[test]
    fn a_seek_lands_the_controls_and_clears_the_filters() {
        let mut core = core(UtilityParams::default());
        let (mut l, mut r) = signal(256);
        core.process(&mut l, &mut r);
        core.set_param(p::GAIN, -12.0);
        core.snap();
        let (mut l, mut r) = signal(256);
        let (in_l, _) = (l.clone(), r.clone());
        core.process(&mut l, &mut r);
        let want = 10f32.powf(-12.0 / 20.0);
        // Landed on the FIRST sample, not glided into over the block.
        assert!((l[0] - in_l[0] * want).abs() < 1e-4, "the seek glided");
    }
}
