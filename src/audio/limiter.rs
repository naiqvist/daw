//! The character limiter's core — the DSP half of `Node::Limiter`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's job is to say which kernel feeds which,
//! and when.
//!
//! # What this device is
//!
//! NOT a transparent limiter. A transparent limiter is a safety device —
//! you switch it on and forget it, and if you can hear it, it is broken.
//! This is the other kind: the one you reach for because you want what it
//! does to the sound. It makes things LOUDER, it stops them GETTING
//! loud, and on the way it leaves fingerprints — a little warmth, a
//! little less high-end fuzz, and a lift that puts back the edge the gain
//! reduction takes off.
//!
//! Every one of those is small on its own. Together, and always on, they
//! are the device's voice.
//!
//! # The path, in order
//!
//! ```text
//!  in ─▶ PUSH ─▶ ┌─ 2x ─────────────────┐ ─▶ DC ─▶ LIMITER (linked) ─┐
//!                │ asymmetric soft clip │                            │
//!                └──────────────────────┘                            │
//!       ┌────────────────────────────────────────────────────────────┘
//!       └─▶ ┌─ 2x ────────────────────────────────────┐ ─▶ trim ─▶ out
//!           │ SLEW BRIGHTEN ─▶ HF FUZZ ─▶ hard clip   │
//!           └─────────────────────────────────────────┘
//! ```
//!
//! # Why the warmth is BEFORE the limiter
//!
//! Because that is where the iron is. In the hardware these emulations
//! are named after, the input transformer and the tube stage sit ahead of
//! the gain-reduction element, so the limiter is reducing a signal that
//! has ALREADY been coloured — it catches the harmonics the colour adds,
//! and the ceiling still means what it says. Putting the colour after the
//! limiter would let it reintroduce the peaks the limiter just removed,
//! and the device would need a second limiter to fix the first one.
//!
//! The asymmetry is the point: [`Waveshaper`]'s `bias` makes the curve
//! treat the two halves of the waveform differently, which generates EVEN
//! harmonics — 2nd before 3rd. Even harmonics are consonant with the
//! fundamental (an octave, a twelfth), which is why they read as "warm"
//! where the odd ones read as "distorted". A DC blocker follows, because
//! an asymmetric curve also generates a DC offset and DC is headroom
//! spent on nothing.
//!
//! # Why the brightening and the fuzz are AFTER it
//!
//! Because the thing they are compensating for happens in the limiter.
//! Gain reduction is fastest exactly when a transient arrives, so the
//! moment with the most high-frequency energy is the moment the gain is
//! being pulled down hardest — limiting DULLS, and it dulls the attacks
//! specifically. [`SlewBrighten`] is driven by how fast the signal is
//! actually moving, so it puts the edge back on the transients that lost
//! it and leaves the sustained material alone. A plain shelf here would
//! brighten everything, including what was never dulled, which is how a
//! limited mix ends up harsh and flat at the same time.
//!
//! The fuzz is the same argument one octave up and much quieter: a
//! band-split above [`FUZZ_HZ`], softly clipped and folded back in. It is
//! the "air" of an analogue output stage — the part that is not really
//! frequency response at all but a very small amount of distortion that
//! only exists up there.
//!
//! # And why there is a clipper after BOTH of them
//!
//! Because they run after the limiter, they can put the signal back over
//! the ceiling — by design, since that is where the sound is. The hard
//! clip inside the same 2x round trip catches it. That ordering is not a
//! compromise, it is the modern mastering chain: limiter for the shape,
//! clipper for the last dB, oversampled so the clipping does not alias
//! back down as fizz.
//!
//! The halfband filter on the way down can ring a hair over the rails, so
//! a final clamp runs at 1x after the round trip. It costs one compare
//! and it makes the ceiling ABSOLUTE: no setting of any control on this
//! device can emit a sample above it. That is the one promise the device
//! makes without an asterisk, which is why there is no output trim —
//! [`params::limiter`](crate::params::limiter) says why.
//!
//! # Latency
//!
//! The lookahead plus two half-band round trips, CONSTANT, and reported
//! to the graph through `spec_latency` so plugin delay compensation can
//! absorb it. Constant because nothing a knob does may change it: a
//! latency that moved with a control would slide the track in time as the
//! user turned it. That is why [`LOOKAHEAD`] is a sample count rather
//! than a millisecond figure, and why both shaping stages stay in the
//! path even at zero warmth and zero fuzz.

use crate::dsp::dynamics::{LookaheadLimiter, SlewBrighten};
use crate::dsp::filters::{DcBlocker, Mode as FilterMode, Svf};
use crate::dsp::ramps::Smoother;
use crate::dsp::shaper::{Mode as ShapeMode, Oversampler2x, Waveshaper};
use crate::params::limiter as lp;

/// The limiter's anticipation, in SAMPLES.
///
/// 72 is 1.5 ms at 48 kHz — long enough that the gain is settled before
/// the peak it is catching arrives, short enough that the device does not
/// feel late on a drum track.
///
/// In samples rather than milliseconds because this figure is REPORTED:
/// the graph's latency model counts samples and asks for it without
/// knowing the sample rate, so a millisecond figure would have to be
/// converted in two places that could round differently. Given as a
/// count, what the device reports and what its delay line holds are the
/// same integer by construction.
pub const LOOKAHEAD: usize = 72;

/// Where the fuzz band starts, in Hz.
///
/// 4.5 kHz: high enough that this never touches the body of anything, low
/// enough that it is not just cymbals.
pub const FUZZ_HZ: f32 = 4_500.0;

/// The corner of the brightener's edge band, in Hz.
pub const BRIGHTEN_HZ: f32 = 1_800.0;

/// How much the warmth knob can bias the curve, as a fraction of the
/// shaper's own maximum.
///
/// Half. At the shaper's full bias the curve is so lopsided that the
/// second harmonic stops being warmth and becomes an octave-down buzz —
/// past the point where anybody would call the result subtle, which is
/// the word the device is built around.
const WARMTH_BIAS: f32 = 0.5;

/// How much drive the warmth knob adds on top of unity.
const WARMTH_DRIVE: f32 = 1.4;

/// The most the fuzz band is boosted before it is clipped and folded back.
const FUZZ_DRIVE: f32 = 6.0;

/// How much of the clipped fuzz band is folded back in, at full knob.
///
/// A twentieth. "Even subtler than the warmth" was the brief, and the
/// warmth is already a few percent — this stage is audible as presence
/// rather than as distortion, and at any more than this it stops being
/// either.
const FUZZ_RETURN: f32 = 0.05;

/// How far the brighten knob drives [`SlewBrighten`]'s amount.
const BRIGHTEN_AMOUNT: f32 = 1.2;

/// Control smoothing, in ms. The saturator's figure, for its reason: fast
/// enough to feel immediate, slow enough that a knob never zips.
const SMOOTH_MS: f32 = 15.0;

/// The device's constant latency, in samples: the lookahead plus both
/// half-band round trips.
///
/// Sample-rate independent, which is what lets `spec_latency` state it
/// without knowing the rate.
pub fn latency() -> usize {
    LOOKAHEAD + 2 * Oversampler2x::new().latency()
}

/// Floats of block scratch the core needs for a block of `block` frames.
///
/// Six lanes at 2x: two channels up-sampled, plus one band copy for the
/// fuzz split. The warmth stage needs only four and runs first, so it
/// borrows from the same allocation.
pub fn scratch_len(block: usize) -> usize {
    block * 2 * 6
}

/// Floats each of the limiter's three delay lines needs.
pub fn line_len() -> usize {
    LookaheadLimiter::scratch_len_samples(LOOKAHEAD)
}

/// A limiter's settings, in engine units. Parallel to
/// [`params::limiter::TABLE`](crate::params::limiter::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LimiterParams {
    pub push_db: f32,
    pub ceiling_db: f32,
    pub style: f32,
    pub release_ms: f32,
    pub warmth: f32,
    pub fuzz: f32,
    pub brighten: f32,
}

impl Default for LimiterParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new limiter sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(lp::TABLE, id).default;
        Self {
            push_db: at(lp::PUSH),
            ceiling_db: at(lp::CEILING),
            style: at(lp::STYLE),
            release_ms: at(lp::RELEASE),
            warmth: at(lp::WARMTH),
            fuzz: at(lp::FUZZ),
            brighten: at(lp::BRIGHTEN),
        }
    }
}

impl LimiterParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(lp::TABLE, param, value) else {
            return;
        };
        match param {
            lp::PUSH => self.push_db = value,
            lp::CEILING => self.ceiling_db = value,
            lp::STYLE => self.style = value,
            lp::RELEASE => self.release_ms = value,
            lp::WARMTH => self.warmth = value,
            lp::FUZZ => self.fuzz = value,
            lp::BRIGHTEN => self.brighten = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            lp::PUSH => self.push_db,
            lp::CEILING => self.ceiling_db,
            lp::STYLE => self.style,
            lp::RELEASE => self.release_ms,
            lp::WARMTH => self.warmth,
            lp::FUZZ => self.fuzz,
            lp::BRIGHTEN => self.brighten,
            _ => 0.0,
        }
    }

    /// The ceiling as a linear amplitude.
    pub fn ceiling(&self) -> f32 {
        db_to_amp(self.ceiling_db)
    }
}

/// Decibels to linear amplitude. Junk reads as unity rather than as a
/// silence nobody asked for.
fn db_to_amp(db: f32) -> f32 {
    if db.is_finite() {
        10.0f32.powf(db / 20.0)
    } else {
        1.0
    }
}

/// The whole device, minus the graph's plumbing.
pub struct LimiterCore {
    sample_rate: f32,
    params: LimiterParams,

    /// The warmth stage: one curve (it is stateless) and per-channel
    /// history.
    warm_shaper: Waveshaper,
    warm_os: [Oversampler2x; 2],
    dc: [DcBlocker; 2],

    /// The gain reduction itself. ONE, because the pair is linked.
    lim: LookaheadLimiter,

    /// The colour stage, all inside one 2x round trip.
    colour_os: [Oversampler2x; 2],
    brighten: [SlewBrighten; 2],
    fuzz_band: [Svf; 2],
    fuzz_shaper: Waveshaper,

    push: Smoother,
    push_target: f32,

    /// What the release-shaping controls were last applied with, so a
    /// settled device re-derives nothing.
    applied_release: f32,
    applied_style: u32,
}

impl Default for LimiterCore {
    fn default() -> Self {
        Self::new()
    }
}

impl LimiterCore {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: LimiterParams::default(),
            warm_shaper: Waveshaper::new(),
            warm_os: [Oversampler2x::new(); 2],
            dc: [DcBlocker::new(); 2],
            lim: LookaheadLimiter::new(),
            colour_os: [Oversampler2x::new(); 2],
            brighten: [SlewBrighten::new(); 2],
            fuzz_band: [Svf::new(); 2],
            fuzz_shaper: Waveshaper::new(),
            push: Smoother::new(),
            push_target: 1.0,
            applied_release: -1.0,
            applied_style: u32::MAX,
        }
    }

    /// Green zone: settle every kernel. Allocates nothing — the caller
    /// owns every buffer this device reads or writes.
    pub fn prepare(&mut self, sample_rate: f32, params: LimiterParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;
        self.params = params;

        for os in &mut self.warm_os {
            os.prepare();
        }
        for os in &mut self.colour_os {
            os.prepare();
        }
        for dc in &mut self.dc {
            dc.prepare(fs);
        }
        // The colour stage runs at 2x, so everything in it is prepared
        // for 2x. A kernel prepared at the base rate and run at double
        // would silently halve every time constant in it.
        for b in &mut self.brighten {
            b.prepare(fs * 2.0, BRIGHTEN_HZ, 0.0);
        }
        for band in &mut self.fuzz_band {
            band.prepare(fs * 2.0, FUZZ_HZ, core::f32::consts::FRAC_1_SQRT_2);
        }

        self.lim.prepare_samples(fs, LOOKAHEAD, params.release_ms);
        self.lim.set_ceiling_db(params.ceiling_db);
        self.applied_release = -1.0;
        self.applied_style = u32::MAX;
        self.apply_release();

        self.push.prepare(fs, SMOOTH_MS);
        self.push_target = db_to_amp(params.push_db);
        self.push.set_now(self.push_target);
    }

    /// The release the kernel actually runs: the knob, scaled by the
    /// style. Re-applied only when one of them moved — the kernel's
    /// setter costs a transcendental and neither control changes often.
    fn apply_release(&mut self) {
        let style = self.params.style.round().max(0.0) as u32;
        let scale = lp::style_release_scale(style);
        let wanted = (self.params.release_ms * scale).clamp(1.0, 5_000.0);
        if (wanted - self.applied_release).abs() <= 1e-4 && style == self.applied_style {
            return;
        }
        self.applied_release = wanted;
        self.applied_style = style;
        self.lim.set_release_ms(self.sample_rate, wanted);
    }

    /// Green zone: forget every history, keep the settings.
    ///
    /// What a transport discontinuity calls. A seek must not drag the
    /// half-band's history, the limiter's gain or the brightener's
    /// envelope across the jump, and must not glide old knob motion into
    /// the new position.
    pub fn reset(&mut self) {
        for os in &mut self.warm_os {
            os.reset();
        }
        for os in &mut self.colour_os {
            os.reset();
        }
        for dc in &mut self.dc {
            dc.reset();
        }
        for b in &mut self.brighten {
            b.reset();
        }
        for band in &mut self.fuzz_band {
            band.reset();
        }
        self.lim.reset();
        self.push.set_now(self.push_target);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            lp::PUSH => {
                self.push_target = db_to_amp(self.params.push_db);
                self.push.set_target(self.push_target);
            }
            lp::CEILING => self.lim.set_ceiling_db(self.params.ceiling_db),
            lp::RELEASE | lp::STYLE => self.apply_release(),
            _ => {}
        }
    }

    pub fn params(&self) -> LimiterParams {
        self.params
    }

    /// Peak gain reduction over the last block, in dB ≥ 0 — what a GR
    /// meter shows.
    pub fn reduction_db(&self) -> f32 {
        self.lim.reduction_db()
    }

    /// Red zone: run the whole device over one stereo block, in place.
    ///
    /// `scratch` is [`scratch_len`] floats of block scratch; `key`,
    /// `line_l` and `line_r` are each [`line_len`] floats of delay-line
    /// storage, zeroed once at compile. Anything undersized FAILS OPEN —
    /// the dry signal is left standing rather than the graph being handed
    /// a buffer nobody wrote.
    pub fn process(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        scratch: &mut [f32],
        key: &mut [f32],
        line_l: &mut [f32],
        line_r: &mut [f32],
    ) {
        let n = l.len();
        if n == 0 || r.len() != n {
            return;
        }
        let n2 = n * 2;
        if scratch.len() < n2 * 3 {
            return; // fail open
        }
        let ceiling = self.params.ceiling();

        // ---- push, at 1x where a gain change cannot alias -------------
        //
        // Smoothed and walked per sample, so a knob turn is a ramp rather
        // than a step.
        {
            let (gain, _) = scratch.split_at_mut(n);
            self.push.process(gain);
            for (sample, g) in l.iter_mut().zip(gain.iter()) {
                *sample *= *g;
            }
            for (sample, g) in r.iter_mut().zip(gain.iter()) {
                *sample *= *g;
            }
        }

        // ---- warmth: asymmetric soft clip at 2x ----------------------
        self.warmth_stage(l, r, scratch, n, n2);

        // ---- the gain reduction, LINKED ------------------------------
        self.lim.process_linked(l, r, key, line_l, line_r);

        // ---- colour: brighten, fuzz and the clipper, all at 2x -------
        self.colour_stage(l, r, scratch, n, n2, ceiling);

        // ---- the ceiling, made absolute ------------------------------
        //
        // The half-band on the way down can ring a hair over the rails.
        // One compare per sample, and no setting of any control on this
        // device can emit a sample above the ceiling.
        for sample in l.iter_mut().chain(r.iter_mut()) {
            *sample = sample.clamp(-ceiling, ceiling);
        }
    }

    /// The input colour: an asymmetric curve run at 2x, then DC blocked.
    ///
    /// Permanently in the path even at warmth 0, because its half-band
    /// round trip is part of the latency the device reports and a latency
    /// that moved with a knob would slide the track in time.
    fn warmth_stage(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        scratch: &mut [f32],
        n: usize,
        n2: usize,
    ) {
        let warmth = self.params.warmth.clamp(0.0, 1.0);
        let (up_l, rest) = scratch.split_at_mut(n2);
        let (up_r, _) = rest.split_at_mut(n2);
        self.warm_os[0].up(l, up_l);
        self.warm_os[1].up(r, up_r);

        if warmth > 0.0 {
            // Bias makes the curve treat the two halves of the waveform
            // differently, which is what generates EVEN harmonics — the
            // consonant ones, an octave and a twelfth up, that read as
            // warmth rather than as distortion.
            self.warm_shaper.configure(
                ShapeMode::SoftClip,
                1.0 + warmth * WARMTH_DRIVE,
                warmth * WARMTH_BIAS,
                1.0,
            );
            self.warm_shaper.process(&mut up_l[..n2]);
            self.warm_shaper.process(&mut up_r[..n2]);
        }

        self.warm_os[0].down(&up_l[..n2], &mut l[..n]);
        self.warm_os[1].down(&up_r[..n2], &mut r[..n]);
        // An asymmetric curve offsets the signal, and DC is headroom
        // spent on nothing — the limiter below would spend real gain
        // reduction holding it down.
        self.dc[0].process(l);
        self.dc[1].process(r);
    }

    /// The output colour: the brightener, the fuzz band and the safety
    /// clipper, all inside ONE round trip.
    ///
    /// One, because all three want to be at 2x and a second trip would be
    /// a second helping of latency for nothing: the brightener is lifting
    /// the top octave, the fuzz stage is making new harmonics above it,
    /// and the clipper is making the sharpest edges of all.
    fn colour_stage(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        scratch: &mut [f32],
        n: usize,
        n2: usize,
        ceiling: f32,
    ) {
        let fuzz = self.params.fuzz.clamp(0.0, 1.0);
        let brighten = self.params.brighten.clamp(0.0, 1.0);
        let (up_l, rest) = scratch.split_at_mut(n2);
        let (up_r, band) = rest.split_at_mut(n2);
        let band = &mut band[..n2];

        self.colour_os[0].up(l, up_l);
        self.colour_os[1].up(r, up_r);

        if fuzz > 0.0 {
            self.fuzz_shaper
                .configure(ShapeMode::SoftClip, 1.0 + fuzz * FUZZ_DRIVE, 0.0, 1.0);
        }

        for channel in 0..2 {
            let lane: &mut [f32] = if channel == 0 { up_l } else { up_r };

            // The brightener first: it is putting back the edge the
            // limiter just took off, and the fuzz stage below should be
            // hearing the restored signal rather than the dull one.
            self.brighten[channel].set_amount(brighten * BRIGHTEN_AMOUNT);
            self.brighten[channel].process(lane);

            // The fuzz: split the top off, drive it, fold a little back.
            if fuzz > 0.0 {
                band.copy_from_slice(lane);
                self.fuzz_band[channel].process(band, FilterMode::Highpass);
                self.fuzz_shaper.process(band);
                let amount = fuzz * FUZZ_RETURN;
                for (sample, air) in lane.iter_mut().zip(band.iter()) {
                    *sample += *air * amount;
                }
            }

            // The clipper, at 2x so its edges do not alias back down as
            // fizz. This is the stage that catches whatever the two above
            // just added — which is why they are allowed to add it.
            for sample in lane.iter_mut() {
                *sample = sample.clamp(-ceiling, ceiling);
            }
        }

        self.colour_os[0].down(&up_l[..n2], &mut l[..n]);
        self.colour_os[1].down(&up_r[..n2], &mut r[..n]);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    /// The block the rig feeds the core in.
    ///
    /// The graph hands a device one block at a time and sizes its scratch
    /// for exactly that, so the tests do the same. Feeding a whole
    /// multi-second signal in one call would be handing the core less
    /// scratch than it needs, and it would correctly fail open — leaving
    /// a test that measures nothing and passes anyway.
    const BLOCK: usize = 256;

    struct Rig {
        core: LimiterCore,
        scratch: Vec<f32>,
        key: Vec<f32>,
        line_l: Vec<f32>,
        line_r: Vec<f32>,
    }

    impl Rig {
        fn new(params: LimiterParams) -> Self {
            let mut core = LimiterCore::new();
            core.prepare(FS, params);
            Self {
                core,
                scratch: vec![0.0; scratch_len(BLOCK)],
                key: vec![0.0; line_len()],
                line_l: vec![0.0; line_len()],
                line_r: vec![0.0; line_len()],
            }
        }

        /// Run a whole signal through, one [`BLOCK`] at a time.
        fn run(&mut self, l: &mut [f32], r: &mut [f32]) {
            let n = l.len().min(r.len());
            let mut at = 0usize;
            while at < n {
                let end = (at + BLOCK).min(n);
                self.core.process(
                    &mut l[at..end],
                    &mut r[at..end],
                    &mut self.scratch,
                    &mut self.key,
                    &mut self.line_l,
                    &mut self.line_r,
                );
                at = end;
            }
        }

        /// One call straight through, for the tests that are ABOUT how
        /// the block is cut.
        fn run_once(&mut self, l: &mut [f32], r: &mut [f32]) {
            self.core.process(
                l,
                r,
                &mut self.scratch,
                &mut self.key,
                &mut self.line_l,
                &mut self.line_r,
            );
        }
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// THE CEILING IS ABSOLUTE. The device's one promise without an
    /// asterisk: no setting of any control emits a sample above it —
    /// including the settings where the colour stages are running flat
    /// out AFTER the gain reduction, which is exactly where a naive
    /// chain leaks.
    #[test]
    fn no_setting_can_break_the_ceiling() {
        for ceiling_db in [0.0f32, -0.3, -6.0, lp::CEILING_MIN_DB] {
            for warmth in [0.0f32, 1.0] {
                for fuzz in [0.0f32, 1.0] {
                    for brighten in [0.0f32, 1.0] {
                        let params = LimiterParams {
                            push_db: lp::PUSH_MAX_DB,
                            ceiling_db,
                            warmth,
                            fuzz,
                            brighten,
                            ..LimiterParams::default()
                        };
                        let mut rig = Rig::new(params);
                        let ceiling = params.ceiling();

                        // Sines, a square and a click: periodic, edgy and
                        // impulsive all reach the clipper differently.
                        let mut l = sine(220.0, 1.4, 1_024);
                        let mut r = sine(3_500.0, 1.4, 1_024);
                        rig.run(&mut l, &mut r);
                        assert!(
                            peak(&l).max(peak(&r)) <= ceiling + 1e-6,
                            "sines broke {ceiling_db} dB at w{warmth} f{fuzz} b{brighten}: {}",
                            peak(&l).max(peak(&r))
                        );

                        let mut sl: Vec<f32> = (0..1_024)
                            .map(|i| if (i / 8) % 2 == 0 { 1.9 } else { -1.9 })
                            .collect();
                        let mut sr = sl.clone();
                        rig.run(&mut sl, &mut sr);
                        assert!(
                            peak(&sl).max(peak(&sr)) <= ceiling + 1e-6,
                            "a square broke {ceiling_db} dB: {}",
                            peak(&sl)
                        );

                        let mut cl = vec![0.0f32; 1_024];
                        let mut cr = vec![0.0f32; 1_024];
                        cl[100] = 4.0;
                        cr[500] = -4.0;
                        rig.run(&mut cl, &mut cr);
                        assert!(
                            peak(&cl).max(peak(&cr)) <= ceiling + 1e-6,
                            "a click broke {ceiling_db} dB: {}",
                            peak(&cl).max(peak(&cr))
                        );
                    }
                }
            }
        }
    }

    /// IT MAKES THINGS LOUDER. Push is the loudness control, and turning
    /// it up must raise the average level while the peak stays pinned —
    /// which is the entire job description.
    #[test]
    fn push_raises_the_average_while_the_peak_stays_put() {
        let source = sine(220.0, 0.5, 24_000);
        let loudness = |push_db: f32| {
            let params = LimiterParams {
                push_db,
                ceiling_db: -0.3,
                ..LimiterParams::default()
            };
            let mut rig = Rig::new(params);
            let (mut l, mut r) = (source.clone(), source.clone());
            rig.run(&mut l, &mut r);
            // Skip the first block: the smoother and the lookahead are
            // still filling.
            (rms(&l[4_800..]), peak(&l[4_800..]))
        };

        let ceiling = db_to_amp(-0.3);

        // BOTH settings drive into the ceiling. That is the comparison
        // worth making: below the ceiling a limiter is a volume knob and
        // the peak moves with the push, exactly as it should. The claim
        // is about what happens once it is WORKING.
        let (some_rms, some_peak) = loudness(8.0);
        let (loud_rms, loud_peak) = loudness(20.0);

        // 1.15 and not something grander, because there is a hard limit
        // on how much louder a SINE can get once it is pinned: all the
        // extra push can do is flatten it toward a square, and a square
        // is only 3 dB above a sine of the same peak. Twelve more dB in
        // buys most of that. On real programme — which has a crest
        // factor to give up — the figure is far larger, but a test wants
        // a signal whose ceiling it can compute.
        assert!(
            loud_rms > some_rms * 1.15,
            "push did not make it louder: {loud_rms} against {some_rms}"
        );
        assert!(some_peak <= ceiling + 1e-6 && loud_peak <= ceiling + 1e-6);
        // Twelve more dB in, and the peak does not move: the average
        // climbs into a ceiling that stays where it was put.
        assert!(
            (loud_peak - some_peak).abs() < ceiling * 0.02,
            "the peak moved with the push: {loud_peak} against {some_peak}"
        );

        // And below the ceiling it really is just a volume knob — the
        // other half of the same fact, and what makes the device usable
        // as a colour box at settings where it never limits.
        let (low_rms, _) = loudness(0.0);
        assert!(low_rms < some_rms, "push does nothing below the ceiling");
    }

    /// IT IS NOT TRANSPARENT, AND THAT IS THE POINT.
    ///
    /// At its defaults, well below the ceiling where NO gain reduction is
    /// happening, the device must still have done something — otherwise
    /// the character is only present when the limiter is working, which
    /// is not what "always on" means.
    #[test]
    fn the_character_is_there_even_when_nothing_is_being_limited() {
        // Quiet enough that the limiter never engages.
        let source = sine(700.0, 0.05, 8_192);
        let params = LimiterParams {
            push_db: 0.0,
            ..LimiterParams::default()
        };
        let mut rig = Rig::new(params);
        let (mut l, mut r) = (source.clone(), source.clone());
        rig.run(&mut l, &mut r);
        assert!(
            rig.core.reduction_db() < 0.01,
            "this test needs the limiter idle, but it reduced {} dB",
            rig.core.reduction_db()
        );

        // Compare against the same signal delayed by the device's own
        // reported latency: what is left is the colour, not the delay.
        let lat = latency();
        let difference: f32 = l[lat + 512..]
            .iter()
            .zip(source[512..].iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(
            difference > 1e-4,
            "the device was transparent at its defaults: {difference}"
        );

        // And with every colour knob at zero it is very nearly a wire —
        // so the difference above really is the colour and not a bug.
        let clean = LimiterParams {
            push_db: 0.0,
            warmth: 0.0,
            fuzz: 0.0,
            brighten: 0.0,
            ..LimiterParams::default()
        };
        let mut plain = Rig::new(clean);
        let (mut pl, mut pr) = (source.clone(), source.clone());
        plain.run(&mut pl, &mut pr);
        let plain_difference: f32 = pl[lat + 512..]
            .iter()
            .zip(source[512..].iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(
            plain_difference < difference * 0.5,
            "the colour knobs did nothing: {plain_difference} against {difference}"
        );
    }

    /// THE WARMTH IS EVEN HARMONICS. That is what separates warmth from
    /// distortion: an asymmetric curve makes the 2nd before the 3rd, and
    /// the 2nd is an octave — consonant, which is why it reads as warm.
    #[test]
    fn the_warmth_makes_even_harmonics() {
        // A tone quiet enough that only the warmth stage acts on it.
        let hz = 500.0;
        let n = 16_384;
        let source = sine(hz, 0.2, n);

        let harmonic = |params: LimiterParams, multiple: f32| {
            let mut rig = Rig::new(params);
            let (mut l, mut r) = (source.clone(), source.clone());
            rig.run(&mut l, &mut r);
            // A single-bin Goertzel-style projection: correlate against
            // the harmonic and take the magnitude. Enough to compare two
            // harmonics of one tone, which is all this needs.
            let tail = &l[4_096..];
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in tail.iter().enumerate() {
                let w =
                    core::f64::consts::TAU * f64::from(hz * multiple) * i as f64 / f64::from(FS);
                re += f64::from(*s) * w.cos();
                im += f64::from(*s) * w.sin();
            }
            ((re * re + im * im).sqrt() / tail.len() as f64) as f32
        };

        let warm = LimiterParams {
            push_db: 0.0,
            warmth: 1.0,
            fuzz: 0.0,
            brighten: 0.0,
            ..LimiterParams::default()
        };
        let cold = LimiterParams {
            warmth: 0.0,
            ..warm
        };

        let second_warm = harmonic(warm, 2.0);
        let second_cold = harmonic(cold, 2.0);
        let third_warm = harmonic(warm, 3.0);

        assert!(
            second_warm > second_cold * 4.0,
            "the warmth added no second harmonic: {second_warm} against {second_cold}"
        );
        assert!(
            second_warm > third_warm,
            "the warmth is odd-order, which is distortion not warmth: \
             2nd {second_warm}, 3rd {third_warm}"
        );
        // SUBTLE. A few percent of the fundamental, not a fuzz box.
        let fundamental = harmonic(warm, 1.0);
        assert!(
            second_warm < fundamental * 0.15,
            "the warmth is not subtle: {second_warm} against {fundamental}"
        );
    }

    /// THE BRIGHTENING IS PROGRAM-DEPENDENT. It lifts a transient-rich
    /// signal and leaves a sustained tone alone — the difference between
    /// this and a shelf, and the reason it does not make a limited mix
    /// harsh.
    #[test]
    fn the_brightening_follows_the_transients() {
        let lift = |brighten: f32, source: &[f32]| {
            let params = LimiterParams {
                push_db: 0.0,
                warmth: 0.0,
                fuzz: 0.0,
                brighten,
                ..LimiterParams::default()
            };
            let mut rig = Rig::new(params);
            let (mut l, mut r) = (source.to_vec(), source.to_vec());
            rig.run(&mut l, &mut r);
            rms(&l[1_024..])
        };

        // A sustained low tone: the brightener should barely touch it.
        let sustained = sine(150.0, 0.3, 16_384);
        let flat = lift(0.0, &sustained);
        let bright = lift(1.0, &sustained);
        let sustained_change = (bright / flat - 1.0).abs();

        // Clicks: nothing but edges.
        let mut clicks = vec![0.0f32; 16_384];
        for i in (0..16_384).step_by(512) {
            clicks[i] = 0.6;
        }
        let click_flat = lift(0.0, &clicks);
        let click_bright = lift(1.0, &clicks);
        let click_change = (click_bright / click_flat - 1.0).abs();

        assert!(
            click_change > sustained_change * 3.0,
            "the brightener is a shelf, not a transient device: \
             clicks {click_change}, sustained {sustained_change}"
        );
        assert!(
            click_change > 0.02,
            "the brightener did nothing to a transient: {click_change}"
        );
    }

    /// THE STYLES ARE DIFFERENT SPEEDS. Smash recovers faster than warm,
    /// which is the whole content of the switch.
    #[test]
    fn the_styles_recover_at_different_speeds() {
        let recovery = |style: u32| {
            let params = LimiterParams {
                push_db: 12.0,
                ceiling_db: -6.0,
                style: style as f32,
                release_ms: 200.0,
                ..LimiterParams::default()
            };
            let mut rig = Rig::new(params);
            // A loud burst, then something quiet: how much of the quiet
            // part survives says how fast the gain came back.
            let mut l = vec![0.0f32; 24_000];
            for (i, s) in l.iter_mut().enumerate() {
                let amp = if i < 4_800 { 1.5 } else { 0.08 };
                *s = amp * (core::f32::consts::TAU * 200.0 * i as f32 / FS).sin();
            }
            let mut r = l.clone();
            rig.run(&mut l, &mut r);
            rms(&l[6_000..9_000])
        };

        let warm = recovery(lp::STYLE_WARM);
        let punch = recovery(lp::STYLE_PUNCH);
        let smash = recovery(lp::STYLE_SMASH);
        assert!(
            smash > punch && punch > warm,
            "the styles do not order: warm {warm}, punch {punch}, smash {smash}"
        );

        // And the scales are the table's, in the order the names are.
        assert_eq!(lp::STYLE_RELEASE_SCALE.len(), lp::STYLE_NAMES.len());
        assert!(lp::style_release_scale(lp::STYLE_WARM) > 1.0);
        assert_eq!(lp::style_release_scale(lp::STYLE_PUNCH), 1.0);
        assert!(lp::style_release_scale(lp::STYLE_SMASH) < 1.0);
        // A stale index still limits.
        assert_eq!(lp::style_release_scale(99), 1.0);
    }

    /// THE STEREO IMAGE DOES NOT MOVE. One linked gain, so a pair whose
    /// channels differ in level keeps that difference — the property that
    /// separates this from two mono limiters.
    #[test]
    fn limiting_holds_the_stereo_image_still() {
        let params = LimiterParams {
            push_db: 12.0,
            // The colour off: what is measured is the gain reduction.
            warmth: 0.0,
            fuzz: 0.0,
            brighten: 0.0,
            ..LimiterParams::default()
        };
        let mut rig = Rig::new(params);
        let mut l = sine(300.0, 1.2, 8_192);
        let mut r: Vec<f32> = l.iter().map(|s| s * 0.5).collect();
        rig.run(&mut l, &mut r);

        for i in 512..l.len() {
            if l[i].abs() > 1e-3 {
                let ratio = r[i] / l[i];
                assert!(
                    (ratio - 0.5).abs() < 1e-3,
                    "the image moved at {i}: {ratio}"
                );
            }
        }
    }

    /// SPLIT-BLOCK EQUIVALENCE — the property the segmented transport
    /// depends on: 512 must equal the same samples in any pieces.
    #[test]
    fn processing_is_the_same_however_the_block_is_split() {
        let params = LimiterParams {
            push_db: 9.0,
            ..LimiterParams::default()
        };
        let n = BLOCK;
        let source_l = sine(180.0, 1.1, n);
        let source_r = sine(2_200.0, 0.9, n);

        // One call for the whole block...
        let mut whole = Rig::new(params);
        let (mut al, mut ar) = (source_l.clone(), source_r.clone());
        whole.run_once(&mut al, &mut ar);

        // ...against the same samples cut into awkward pieces.
        let mut split = Rig::new(params);
        let (mut bl, mut br) = (source_l.clone(), source_r.clone());
        let mut at = 0usize;
        for take in [100usize, 1, 7, 148] {
            let end = (at + take).min(n);
            split.run_once(&mut bl[at..end], &mut br[at..end]);
            at = end;
        }
        split.run_once(&mut bl[at..], &mut br[at..]);

        assert!(
            al.iter().zip(&bl).all(|(x, y)| x.to_bits() == y.to_bits()),
            "the split run diverged on the left"
        );
        assert!(
            ar.iter().zip(&br).all(|(x, y)| x.to_bits() == y.to_bits()),
            "the split run diverged on the right"
        );
    }

    /// The process path allocates nothing, at the settings that do the
    /// most work.
    #[test]
    fn processing_does_not_allocate() {
        let mut rig = Rig::new(LimiterParams {
            push_db: lp::PUSH_MAX_DB,
            warmth: 1.0,
            fuzz: 1.0,
            brighten: 1.0,
            ..LimiterParams::default()
        });
        let mut l = vec![0.7f32; 256];
        let mut r = vec![-0.7f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..64 {
                rig.run(&mut l, &mut r);
            }
        });
    }

    /// Undersized scratch FAILS OPEN, and nonsense in stays finite out.
    #[test]
    fn it_fails_open_and_never_emits_a_non_finite_sample() {
        let params = LimiterParams::default();
        let mut core = LimiterCore::new();
        core.prepare(FS, params);

        // Too little block scratch: the dry signal is left standing
        // rather than the graph being handed a buffer nobody wrote.
        let mut l = vec![0.5f32; 128];
        let mut r = vec![0.5f32; 128];
        let before = l.clone();
        let mut tiny = vec![0.0f32; 4];
        let mut key = vec![0.0; line_len()];
        let mut ll = vec![0.0; line_len()];
        let mut lr = vec![0.0; line_len()];
        core.process(&mut l, &mut r, &mut tiny, &mut key, &mut ll, &mut lr);
        assert_eq!(l, before, "undersized scratch was used anyway");

        // A mismatched pair likewise.
        let mut short = vec![0.5f32; 8];
        let mut long = vec![0.5f32; 9];
        let untouched = short.clone();
        let mut scratch = vec![0.0; scratch_len(1_024)];
        core.process(
            &mut short,
            &mut long,
            &mut scratch,
            &mut key,
            &mut ll,
            &mut lr,
        );
        assert_eq!(short, untouched, "a mismatched pair was processed anyway");

        // Every extreme the table allows, on hostile input.
        for def in lp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = LimiterParams::default();
                params.set(def.id, value);
                let mut rig = Rig::new(params);
                let mut hot = vec![0.0f32; 512];
                for (i, s) in hot.iter_mut().enumerate() {
                    *s = if i % 3 == 0 { 9.0 } else { -9.0 };
                }
                let mut other = vec![0.0f32; 512];
                rig.run(&mut hot, &mut other);
                assert!(
                    hot.iter().chain(other.iter()).all(|s| s.is_finite()),
                    "{} at {value}",
                    def.name
                );
            }
        }
    }

    /// Every table id round-trips, and an unknown id is ignored.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = LimiterParams::default();
        for def in lp::TABLE {
            let want = (def.min + def.max) * 0.5;
            params.set(def.id, want);
            assert!(
                (params.get(def.id) - want).abs() < 1e-4,
                "{} did not round-trip",
                def.name
            );
        }
        let before = params;
        params.set(9_999, 1.0);
        assert_eq!(params, before, "an unknown id must change nothing");

        params.set(lp::CEILING, 1e9);
        assert!((params.ceiling_db - lp::CEILING_MAX_DB).abs() < 1e-3);
        params.set(lp::CEILING, -1e9);
        assert!((params.ceiling_db - lp::CEILING_MIN_DB).abs() < 1e-3);

        // The ceiling converts the way dB does.
        params.set(lp::CEILING, 0.0);
        assert!((params.ceiling() - 1.0).abs() < 1e-6);
        params.set(lp::CEILING, -6.0);
        assert!((params.ceiling() - 0.501_187).abs() < 1e-4);
    }

    /// THE LATENCY IS CONSTANT AND HONEST: what the device reports is
    /// what it actually delays by, and nothing a knob does changes it.
    #[test]
    fn the_reported_latency_is_the_real_one() {
        let reported = latency();
        assert_eq!(reported, LOOKAHEAD + 2 * Oversampler2x::new().latency());

        // An impulse comes out `reported` samples later. Measured with
        // the colour off, so what is timed is the delay and not a curve.
        let params = LimiterParams {
            push_db: 0.0,
            warmth: 0.0,
            fuzz: 0.0,
            brighten: 0.0,
            ..LimiterParams::default()
        };
        let mut rig = Rig::new(params);
        let mut l = vec![0.0f32; 1_024];
        let mut r = vec![0.0f32; 1_024];
        l[0] = 0.25;
        r[0] = 0.25;
        rig.run(&mut l, &mut r);

        let at = l
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        assert!(
            at.abs_diff(reported) <= 2,
            "the impulse landed at {at}, the device reports {reported}"
        );

        // And no setting moves it.
        for def in lp::TABLE {
            for value in [def.min, def.max] {
                let mut params = LimiterParams::default();
                params.set(def.id, value);
                let mut core = LimiterCore::new();
                core.prepare(FS, params);
                assert_eq!(latency(), reported, "{} at {value} moved it", def.name);
            }
        }
    }

    /// THE CARD DRAWS THE CURVE THE NODE RUNS.
    ///
    /// The widget layer may not import the engine, so its transfer curve
    /// is a restatement rather than a call — and a restatement is exactly
    /// the thing that drifts. This walks the two against each other, and
    /// it is the test that catches a bias applied on the wrong side of a
    /// multiply: same character, wrong numbers, and nothing on screen
    /// looks obviously broken.
    #[test]
    fn the_card_draws_the_curve_the_node_runs() {
        use crate::ui::device::limiter as ui;

        for warmth in [0.0f32, 0.35, 1.0] {
            for push_db in [0.0f32, 6.0, lp::PUSH_MAX_DB] {
                for ceiling_db in [0.0f32, -0.3, -6.0] {
                    let state = ui::LimiterUi {
                        push: ui::limiter_norm(lp::PUSH, push_db),
                        ceiling: ui::limiter_norm(lp::CEILING, ceiling_db),
                        warmth: ui::limiter_norm(lp::WARMTH, warmth),
                        ..ui::LimiterUi::default()
                    };

                    // The node's own stage, built the way `warmth_stage`
                    // builds it.
                    let mut shaper = Waveshaper::new();
                    shaper.configure(
                        ShapeMode::SoftClip,
                        1.0 + warmth * WARMTH_BIAS_CHECK.0,
                        warmth * WARMTH_BIAS_CHECK.1,
                        1.0,
                    );
                    let push = db_to_amp(push_db);
                    let ceiling = db_to_amp(ceiling_db);

                    for i in 0..=200 {
                        let x = -1.6 + i as f32 * 1.6 / 100.0;
                        let pushed = x * push;
                        let shaped = if warmth > 0.0 {
                            // `shape(0.0)` IS the offset the DC blocker
                            // takes back out, which is why the card
                            // subtracts it.
                            shaper.shape(pushed) - shaper.shape(0.0)
                        } else {
                            pushed
                        };
                        let kernel = shaped.clamp(-ceiling, ceiling);
                        let drawn = ui::transfer(&state, x);
                        assert!(
                            (kernel - drawn).abs() < 1e-5,
                            "warmth {warmth} push {push_db} ceiling {ceiling_db} at {x}: \
                             node {kernel}, card {drawn}"
                        );
                    }
                }
            }
        }
    }

    /// The constants the card restates are the ones the node uses. If
    /// either moves, the test above starts comparing the card against a
    /// node it is no longer a picture of — so the numbers themselves are
    /// pinned, here, next to the comparison that depends on them.
    const WARMTH_BIAS_CHECK: (f32, f32) = (WARMTH_DRIVE, WARMTH_BIAS);
}
