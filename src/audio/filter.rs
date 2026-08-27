//! The character filter's core — the DSP half of `Node::Filter`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's job is to say which kernel feeds which,
//! and when.
//!
//! # The path, per channel
//!
//! ```text
//! in ─▶ CASCADE (lp/hp, 6-48 dB/oct)  ─┐
//!    ▶ or SVF   (bp/notch)            ─┴─▶ ┌─ 2x: shaper ─┐ ─▶ DC ─▶ out
//!                                          └──────────────┘
//! ```
//!
//! # What a "character" is
//!
//! Two filters with the same corner and the same slope can sound nothing
//! alike, and the difference is almost never the response curve. It is
//! where the nonlinearity sits, what shape it is, and — most of all —
//! what the RESONANCE does when you lean on it. A ladder's peak collapses
//! under drive; an OTA's holds on; a diode folds over almost at once.
//!
//! So the character switch is not a preset over the other knobs. It is
//! the handful of numbers the curve cannot show:
//! [`params::filter::CHARACTERS`](crate::params::filter::CHARACTERS) holds
//! them, and both this node and the card's drawn curve read that one
//! table — so the picture and the audio agree by construction rather than
//! by inspection.
//!
//! # Why this is STEREO
//!
//! Because it lives in a track chain, and every other effect in that
//! chain is. The node this replaces summed to mono, which was harmless
//! while nothing could load it and fatal the moment something could: a
//! mono node in the middle of a stereo chain folds everything downstream
//! of it, so inserting a filter would have collapsed the mix. The
//! saturator's doc says the same thing about the same hazard.
//!
//! Being stereo then makes [`SPREAD`](crate::params::filter::SPREAD)
//! possible, which is the one control here that a mono filter could never
//! have had: the two channels' corners lean in OPPOSITE directions around
//! the cutoff, so the corner you set stays the centre of what you hear
//! and the knob only opens it out.
//!
//! # Latency
//!
//! One half-band round trip, CONSTANT, reported through `spec_latency`.
//! The drive stage stays in the path even at zero drive, because a
//! latency that moved with a knob would slide the track in time as the
//! user turned it. At drive 0 the shaper is skipped entirely, so the
//! stage is a linear-phase wire and the device is transparent apart from
//! that delay.

use crate::dsp::filters::{Cascade, DcBlocker, Mode as FilterMode, Svf};
use crate::dsp::ramps::Smoother;
use crate::dsp::shaper::{Oversampler2x, Waveshaper};
use crate::params::filter as fp;

/// How many ORIGINAL-rate samples one setting of the coefficients covers.
///
/// Sub-block, so a performed sweep is stepless rather than a staircase.
/// `prepare()` is bounded pure math — a `sin_cos` per stage — and a
/// settled filter skips it entirely.
pub const COEFF_INTERVAL: usize = 16;

/// Control smoothing, in ms. The house figure: fast enough to feel
/// immediate, slow enough that a knob never zips.
const SMOOTH_MS: f32 = 15.0;

/// The device's constant latency, in samples: the drive stage's half-band
/// round trip.
///
/// Sample-rate independent, which is what lets `spec_latency` state it
/// without knowing the rate.
pub fn latency() -> usize {
    Oversampler2x::new().latency()
}

/// Floats of block scratch the core needs for a block of `block` frames.
///
/// Four 2x lanes: a round-trip lane and a shaped copy, per channel.
pub fn scratch_len(block: usize) -> usize {
    block * 2 * 4
}

/// A filter's settings, in engine units. Parallel to
/// [`params::filter::TABLE`](crate::params::filter::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FilterParams {
    pub mode: f32,
    pub slope: f32,
    pub cutoff_hz: f32,
    pub res: f32,
    pub drive: f32,
    pub character: f32,
    pub spread_st: f32,
}

impl Default for FilterParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new filter sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(fp::TABLE, id).default;
        Self {
            mode: at(fp::MODE),
            slope: at(fp::SLOPE),
            cutoff_hz: at(fp::CUTOFF),
            res: at(fp::RES),
            drive: at(fp::DRIVE),
            character: at(fp::CHARACTER),
            spread_st: at(fp::SPREAD),
        }
    }
}

impl FilterParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(fp::TABLE, param, value) else {
            return;
        };
        match param {
            fp::MODE => self.mode = value,
            fp::SLOPE => self.slope = value,
            fp::CUTOFF => self.cutoff_hz = value,
            fp::RES => self.res = value,
            fp::DRIVE => self.drive = value,
            fp::CHARACTER => self.character = value,
            fp::SPREAD => self.spread_st = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            fp::MODE => self.mode,
            fp::SLOPE => self.slope,
            fp::CUTOFF => self.cutoff_hz,
            fp::RES => self.res,
            fp::DRIVE => self.drive,
            fp::CHARACTER => self.character,
            fp::SPREAD => self.spread_st,
            _ => 0.0,
        }
    }

    pub fn mode_index(&self) -> u32 {
        self.mode.round().max(0.0) as u32
    }
    pub fn slope_index(&self) -> u32 {
        self.slope.round().max(0.0) as u32
    }
    pub fn character_index(&self) -> u32 {
        self.character.round().max(0.0) as u32
    }
}

/// One channel's signal history. Everything here is per side, because the
/// two sides run at different corners the moment `spread` leaves zero.
struct Channel {
    cascade: Cascade,
    svf: Svf,
    oversampler: Oversampler2x,
    dc: DcBlocker,
    /// What the coefficients were last built with, so a settled channel
    /// re-prepares nothing.
    prepared_cutoff: f32,
    prepared_q: f32,
}

impl Channel {
    fn new() -> Self {
        Self {
            cascade: Cascade::new(),
            svf: Svf::new(),
            oversampler: Oversampler2x::new(),
            dc: DcBlocker::new(),
            prepared_cutoff: 0.0,
            prepared_q: 0.0,
        }
    }

    fn reset(&mut self) {
        self.cascade.reset();
        self.svf.reset();
        self.oversampler.reset();
        self.dc.reset();
    }
}

/// The whole device, minus the graph's plumbing.
pub struct FilterCore {
    sample_rate: f32,
    params: FilterParams,

    ch: [Channel; 2],
    /// One curve between them: a transfer function has no history, so
    /// there is nothing per channel to keep.
    shaper: Waveshaper,

    cutoff: Smoother,
    res: Smoother,
    drive: Smoother,
    spread: Smoother,
    cutoff_target: f32,
    res_target: f32,
    drive_target: f32,
    spread_target: f32,

    /// Live mode/slope/character, and where a letter parks until the next
    /// block edge.
    mode: u32,
    slope: u32,
    character: u32,
    pending_mode: u32,
    pending_slope: u32,
    pending_character: u32,
}

impl Default for FilterCore {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterCore {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: FilterParams::default(),
            ch: [Channel::new(), Channel::new()],
            shaper: Waveshaper::new(),
            cutoff: Smoother::new(),
            res: Smoother::new(),
            drive: Smoother::new(),
            spread: Smoother::new(),
            cutoff_target: 20_000.0,
            res_target: fp::FLAT_Q,
            drive_target: 0.0,
            spread_target: 0.0,
            mode: fp::MODE_LP,
            slope: 3,
            character: fp::CHAR_CLEAN,
            pending_mode: fp::MODE_LP,
            pending_slope: 3,
            pending_character: fp::CHAR_CLEAN,
        }
    }

    /// Green zone: settle every kernel. Allocates nothing — the caller
    /// owns every buffer this device reads or writes.
    pub fn prepare(&mut self, sample_rate: f32, params: FilterParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;
        self.params = params;

        for ch in &mut self.ch {
            ch.oversampler.prepare();
            ch.dc.prepare(fs);
        }

        self.mode = params.mode_index().min(fp::MODE_NOTCH);
        self.slope = params.slope_index();
        self.character = params.character_index().min(fp::CHAR_MAX);
        self.pending_mode = self.mode;
        self.pending_slope = self.slope;
        self.pending_character = self.character;

        let smoother = |value: f32| {
            let mut s = Smoother::new();
            s.prepare(fs, SMOOTH_MS);
            s.set_now(value);
            s
        };
        self.cutoff_target = params.cutoff_hz;
        self.res_target = params.res;
        self.drive_target = params.drive;
        self.spread_target = params.spread_st;
        self.cutoff = smoother(self.cutoff_target);
        self.res = smoother(self.res_target);
        self.drive = smoother(self.drive_target);
        self.spread = smoother(self.spread_target);

        self.reset();
    }

    /// Green zone: forget every history, keep the settings, and snap the
    /// controls to where they were heading.
    ///
    /// What a transport discontinuity calls: a seek must not drag the old
    /// ring along, and must not glide old knob motion into the new
    /// position.
    pub fn reset(&mut self) {
        for ch in &mut self.ch {
            ch.reset();
            // Force the next block to rebuild coefficients rather than
            // trusting history that has just been thrown away.
            ch.prepared_cutoff = 0.0;
            ch.prepared_q = 0.0;
        }
        self.cutoff.set_now(self.cutoff_target);
        self.res.set_now(self.res_target);
        self.drive.set_now(self.drive_target);
        self.spread.set_now(self.spread_target);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            // The three switches land on the next block edge, because
            // changing them mid-block means clearing filter state and a
            // cleared cascade mid-block is a click.
            fp::MODE => self.pending_mode = self.params.mode_index().min(fp::MODE_NOTCH),
            fp::SLOPE => self.pending_slope = self.params.slope_index(),
            fp::CHARACTER => {
                self.pending_character = self.params.character_index().min(fp::CHAR_MAX);
            }
            fp::CUTOFF => {
                self.cutoff_target = self.params.cutoff_hz;
                self.cutoff.set_target(self.cutoff_target);
            }
            fp::RES => {
                self.res_target = self.params.res;
                self.res.set_target(self.res_target);
            }
            fp::DRIVE => {
                self.drive_target = self.params.drive;
                self.drive.set_target(self.drive_target);
            }
            fp::SPREAD => {
                self.spread_target = self.params.spread_st;
                self.spread.set_target(self.spread_target);
            }
            _ => {}
        }
    }

    pub fn params(&self) -> FilterParams {
        self.params
    }

    /// Red zone: filter a stereo pair in place.
    ///
    /// `scratch` is [`scratch_len`] floats. Anything undersized FAILS
    /// OPEN — the dry signal is left standing rather than the graph being
    /// handed a buffer nobody wrote.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], scratch: &mut [f32]) {
        let n = l.len();
        if n == 0 || r.len() != n {
            return;
        }

        // A switch landing clears the signal history: a cascade carrying
        // lowpass state into a highpass is a thump, and a brief clean
        // restart is not. The CHARACTER is in here too, because it moves
        // the resonance the coefficients are built from.
        if self.pending_mode != self.mode
            || self.pending_slope != self.slope
            || self.pending_character != self.character
        {
            self.mode = self.pending_mode;
            self.slope = self.pending_slope;
            self.character = self.pending_character;
            for ch in &mut self.ch {
                ch.cascade.reset();
                ch.svf.reset();
                ch.prepared_cutoff = 0.0; // force the rebuild below
            }
        }

        // Where the drive blend starts this block; it ramps to the
        // smoother's end value across the 2x pass.
        let drive_start = self.drive.current();

        // The controls walk ONCE, on the left channel's pass, and the
        // right reads the settled values. Walking them twice would run
        // every smoother at double speed and silently halve every
        // smoothing time in the device.
        let (cut_end, res_end, drive_end, spread_end) = self.walk_poles(l, 0);
        self.walk_poles_with(r, 1, cut_end, res_end, spread_end);

        self.drive_stage(l, r, scratch, n, drive_start, drive_end);
    }

    /// One channel's pole section, walking the smoothers as it goes.
    ///
    /// Returns where each control ended, so the other channel can be run
    /// at the same settled values without advancing them again.
    fn walk_poles(&mut self, io: &mut [f32], channel: usize) -> (f32, f32, f32, f32) {
        let (mut cut_now, mut res_now, mut drive_now, mut spread_now) = (
            self.cutoff.current(),
            self.res.current(),
            self.drive.current(),
            self.spread.current(),
        );
        for chunk in io.chunks_mut(COEFF_INTERVAL) {
            let n = chunk.len();
            let mut ctrl = [0.0f32; COEFF_INTERVAL];
            self.cutoff.process(&mut ctrl[..n]);
            cut_now = ctrl[n - 1];
            self.res.process(&mut ctrl[..n]);
            res_now = ctrl[n - 1];
            self.drive.process(&mut ctrl[..n]);
            drive_now = ctrl[n - 1];
            self.spread.process(&mut ctrl[..n]);
            spread_now = ctrl[n - 1];
            self.poles(chunk, channel, cut_now, res_now, drive_now, spread_now);
        }
        (cut_now, res_now, drive_now, spread_now)
    }

    /// The other channel, at settled values — no smoother is advanced.
    fn walk_poles_with(&mut self, io: &mut [f32], channel: usize, cut: f32, res: f32, spread: f32) {
        let drive = self.drive.current();
        for chunk in io.chunks_mut(COEFF_INTERVAL) {
            self.poles(chunk, channel, cut, res, drive, spread);
        }
    }

    /// Build this channel's coefficients if anything moved, then run the
    /// poles over one chunk.
    fn poles(
        &mut self,
        chunk: &mut [f32],
        channel: usize,
        cutoff: f32,
        res: f32,
        drive: f32,
        spread: f32,
    ) {
        let (mode, character, slope, fs) =
            (self.mode, self.character, self.slope, self.sample_rate);
        // Left leans down, right leans up, so the corner the knob names
        // stays the geometric centre of the two.
        let side = if channel == 0 { -1.0 } else { 1.0 };
        let hz = fp::spread_cutoff(cutoff, spread, side);
        let q_eff = fp::effective_q_for(res, drive, character);

        let Some(ch) = self.ch.get_mut(channel) else {
            return;
        };
        let moved = (hz - ch.prepared_cutoff).abs() > ch.prepared_cutoff.max(1.0) * 1e-4
            || (q_eff - ch.prepared_q).abs() > 1e-4;
        if moved {
            ch.prepared_cutoff = hz;
            ch.prepared_q = q_eff;
            match mode {
                fp::MODE_BP | fp::MODE_NOTCH => ch.svf.prepare(fs, hz, q_eff),
                _ => ch
                    .cascade
                    .prepare(fs, hz, q_eff, fp::slope_order(slope), mode == fp::MODE_HP),
            }
        }
        match mode {
            fp::MODE_BP => ch.svf.process(chunk, FilterMode::BandpassUnity),
            fp::MODE_NOTCH => ch.svf.process(chunk, FilterMode::Notch),
            _ => ch.cascade.process(chunk),
        }

        // A ladder thins out as its resonance comes up, because the
        // feedback subtracts the input. The character says how much, and
        // an OTA says none — which is most of why the two are told apart
        // by ear on a bass line before anything else.
        let gain = fp::resonance_loss(res, character, mode);
        if gain != 1.0 {
            for sample in chunk.iter_mut() {
                *sample *= gain;
            }
        }
    }

    /// The drive stage: a shaped copy crossfaded against the clean one,
    /// both at 2x.
    ///
    /// The blend is owned HERE and not by the shaper's own `mix`, because
    /// `shape()` rails its output to ±1 even at mix 0 — and a resonant
    /// filter legitimately rings past ±1, so the clean path must keep its
    /// float headroom.
    fn drive_stage(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        scratch: &mut [f32],
        n: usize,
        drive_start: f32,
        drive_end: f32,
    ) {
        let n2 = n * 2;
        if scratch.len() < n2 * 4 {
            return; // fail open: the filtered signal stands undriven
        }
        let driving = drive_end.max(drive_start) > 1e-6;
        if driving {
            let model = fp::character(self.character);
            self.shaper.configure(
                crate::params::sat::mode(model.shape),
                fp::shaper_drive_for(drive_end, self.character),
                model.bias,
                1.0, // pure: this node owns the blend
            );
        }

        let (lane_l, rest) = scratch.split_at_mut(n2);
        let (lane_r, shaped) = rest.split_at_mut(n2);
        let shaped = &mut shaped[..n2];

        for (channel, (io, lane)) in [(l, lane_l), (r, lane_r)].into_iter().enumerate() {
            let Some(ch) = self.ch.get_mut(channel) else {
                continue;
            };
            let lane = &mut lane[..n2];
            ch.oversampler.up(io, lane);
            if driving {
                shaped.copy_from_slice(lane);
                self.shaper.process(shaped);
                // The blend RAMPS across the block rather than stepping,
                // so a drive knob in motion is a fade and not a series of
                // small edges. Written out rather than borrowed from the
                // graph's `Ramp`, because a core that reaches back into
                // the graph to crossfade is a core that cannot be tested
                // without one.
                let step = (drive_end - drive_start) / n2.max(1) as f32;
                let mut m = drive_start;
                for (dry, wet) in lane.iter_mut().zip(shaped.iter()) {
                    *dry = *dry * (1.0 - m) + *wet * m;
                    m += step;
                }
            }
            ch.oversampler.down(lane, io);
            // An asymmetric curve develops an offset, and a driven one
            // can either way; five hertz of nothing keeps it out of
            // everything downstream.
            ch.dc.process(io);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    struct Rig {
        core: FilterCore,
        scratch: Vec<f32>,
    }

    impl Rig {
        fn new(params: FilterParams) -> Self {
            let mut core = FilterCore::new();
            core.prepare(FS, params);
            Self {
                core,
                scratch: vec![0.0; scratch_len(BLOCK)],
            }
        }

        /// Run a whole signal through, one [`BLOCK`] at a time — the way
        /// the graph hands a device its audio.
        fn run(&mut self, l: &mut [f32], r: &mut [f32]) {
            let n = l.len().min(r.len());
            let mut at = 0;
            while at < n {
                let end = (at + BLOCK).min(n);
                self.core
                    .process(&mut l[at..end], &mut r[at..end], &mut self.scratch);
                at = end;
            }
        }

        fn run_once(&mut self, l: &mut [f32], r: &mut [f32]) {
            self.core.process(l, r, &mut self.scratch);
        }
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// The level a steady tone comes out at, in dB relative to its input.
    fn response_db(params: FilterParams, hz: f32) -> f32 {
        let n = 16_384;
        let source = sine(hz, 0.25, n);
        let mut rig = Rig::new(params);
        let (mut l, mut r) = (source.clone(), source.clone());
        rig.run(&mut l, &mut r);
        // Skip the settling head.
        let out = rms(&l[4_096..]);
        let inp = rms(&source[4_096..]);
        20.0 * (out.max(1e-9) / inp.max(1e-9)).log10()
    }

    /// IT FILTERS. A lowpass passes what is under the corner and stops
    /// what is over it — the floor beneath every other claim here.
    #[test]
    fn a_lowpass_passes_below_the_corner_and_stops_above_it() {
        let params = FilterParams {
            cutoff_hz: 1_000.0,
            res: fp::FLAT_Q,
            drive: 0.0,
            character: fp::CHAR_CLEAN as f32,
            ..FilterParams::default()
        };
        let pass = response_db(params, 100.0);
        let corner = response_db(params, 1_000.0);
        let stop = response_db(params, 8_000.0);

        assert!(pass.abs() < 1.0, "the passband is not flat: {pass}");
        assert!(
            (corner + 3.0).abs() < 2.0,
            "the corner should be about -3 dB: {corner}"
        );
        assert!(stop < -30.0, "the stopband is not stopping: {stop}");
        assert!(stop < corner && corner < pass, "the slope runs backwards");
    }

    /// THE SPREAD OPENS THE TWO CHANNELS APART WITHOUT MOVING THE CORNER.
    /// The control a mono filter could never have had, and the reason
    /// this node is stereo at all.
    #[test]
    fn the_spread_pulls_the_channels_apart_around_the_corner() {
        // Flat resonance on purpose: with a peak, a probe tone landing on
        // one channel's corner tells you where the peak is rather than
        // which way the spread leaned.
        let base = FilterParams {
            cutoff_hz: 1_000.0,
            mode: fp::MODE_LP as f32,
            res: fp::FLAT_Q,
            drive: 0.0,
            character: fp::CHAR_CLEAN as f32,
            ..FilterParams::default()
        };

        // With no spread the two channels are IDENTICAL, bit for bit.
        let source = sine(2_500.0, 0.3, 4_096);
        let mut flat = Rig::new(base);
        let (mut l, mut r) = (source.clone(), source.clone());
        flat.run(&mut l, &mut r);
        assert!(
            l.iter().zip(&r).all(|(a, b)| a.to_bits() == b.to_bits()),
            "the channels differ with spread at zero"
        );

        // With spread they are not, and each has moved the way its side
        // says: left down, right up.
        let wide = FilterParams {
            spread_st: fp::SPREAD_MAX_ST,
            ..base
        };
        let mut rig = Rig::new(wide);
        let (mut wl, mut wr) = (source.clone(), source.clone());
        rig.run(&mut wl, &mut wr);
        assert!(
            wl.iter().zip(&wr).any(|(a, b)| a.to_bits() != b.to_bits()),
            "the spread did nothing"
        );
        // A tone well ABOVE the corner: the channel whose corner moved up
        // passes more of it, and the one that moved down passes less.
        assert!(
            rms(&wr[2_048..]) > rms(&wl[2_048..]),
            "the right channel did not open further: {} vs {}",
            rms(&wr[2_048..]),
            rms(&wl[2_048..])
        );

        // And the corner itself has not moved — the mean of the two sides
        // still sits where the knob says.
        let lo = fp::spread_cutoff(1_000.0, fp::SPREAD_MAX_ST, -1.0);
        let hi = fp::spread_cutoff(1_000.0, fp::SPREAD_MAX_ST, 1.0);
        assert!(((lo * hi).sqrt() - 1_000.0).abs() < 1.0);
    }

    /// THE CHARACTERS ARE AUDIBLY DIFFERENT AT THE SAME SETTINGS. The
    /// switch's whole reason to exist: same corner, same slope, same
    /// resonance, four different sounds.
    ///
    /// Measured as the DIFFERENCE BETWEEN THE RENDERED SIGNALS rather
    /// than as any single number about them. A summary statistic invites
    /// a false pass — peak height at the corner has ladder and diode
    /// within a tenth of a dB of each other, a taller peak on one exactly
    /// offset by less bass loss, while the two sound nothing alike. What
    /// the switch promises is that the output changes, so that is what
    /// this measures.
    #[test]
    fn the_characters_sound_different_at_identical_settings() {
        let render = |character: u32| {
            let params = FilterParams {
                cutoff_hz: 800.0,
                res: 14.0,
                drive: 0.8,
                character: character as f32,
                mode: fp::MODE_LP as f32,
                ..FilterParams::default()
            };
            // A square: every harmonic, so nothing about the difference
            // depends on where a single probe tone happened to land.
            let n = 8_192;
            let source: Vec<f32> = (0..n)
                .map(|i| if (i / 60) % 2 == 0 { 0.4 } else { -0.4 })
                .collect();
            let mut rig = Rig::new(params);
            let (mut l, mut r) = (source.clone(), source.clone());
            rig.run(&mut l, &mut r);
            l[2_048..].to_vec()
        };
        let voices: Vec<Vec<f32>> = (0..=fp::CHAR_MAX).map(render).collect();

        for (i, a) in voices.iter().enumerate() {
            assert!(a.iter().all(|s| s.is_finite()), "character {i} went bad");
            for (j, b) in voices.iter().enumerate().skip(i + 1) {
                let diff: Vec<f32> = a.iter().zip(b).map(|(x, y)| x - y).collect();
                let apart = rms(&diff) / rms(a).max(1e-9);
                assert!(
                    apart > 0.05,
                    "characters {i} and {j} render the same: {:.1}% apart",
                    apart * 100.0
                );
            }
        }

        // The ordering the table claims about the RESONANCE, checked on
        // the arithmetic rather than through a saturator that flattens
        // every peak toward every other one: an OTA holds on, a ladder
        // gives ground, a diode has folded.
        let q = |c: u32| fp::effective_q_for(14.0, 0.8, c);
        assert!(
            q(fp::CHAR_OTA) > q(fp::CHAR_LADDER) && q(fp::CHAR_LADDER) > q(fp::CHAR_DIODE),
            "the characters do not order: ota {}, ladder {}, diode {}",
            q(fp::CHAR_OTA),
            q(fp::CHAR_LADDER),
            q(fp::CHAR_DIODE)
        );

        // And the LEVEL differs where the table says it should: a ladder
        // thins out as the resonance comes up, an OTA does not.
        //
        // Measured at drive ZERO, and that is the point of the reading:
        // bass loss is driven by the RESONANCE, so it is fully present
        // here, while the saturator — which compresses a tall peak more
        // than a short one and so quietly pushes the two voicings back
        // together — is not in the path at all.
        let body = |character: u32| {
            response_db(
                FilterParams {
                    cutoff_hz: 800.0,
                    res: 14.0,
                    drive: 0.0,
                    character: character as f32,
                    mode: fp::MODE_LP as f32,
                    ..FilterParams::default()
                },
                80.0,
            )
        };
        assert!(
            body(fp::CHAR_OTA) > body(fp::CHAR_LADDER) + 1.0,
            "the ladder should thin out where the OTA does not: {} vs {}",
            body(fp::CHAR_LADDER),
            body(fp::CHAR_OTA)
        );
    }

    /// A RESONANT FILTER RINGS. Turning resonance up must lift the corner
    /// well above flat, or the knob is decorative.
    #[test]
    fn resonance_lifts_the_corner() {
        let at = |res: f32| {
            response_db(
                FilterParams {
                    cutoff_hz: 1_000.0,
                    res,
                    drive: 0.0,
                    character: fp::CHAR_CLEAN as f32,
                    ..FilterParams::default()
                },
                1_000.0,
            )
        };
        let flat = at(fp::FLAT_Q);
        let rung = at(18.0);
        assert!(
            rung > flat + 10.0,
            "resonance did not ring: {rung} against {flat} dB"
        );
    }

    /// SPLIT-BLOCK EQUIVALENCE — the property the segmented transport
    /// depends on.
    #[test]
    fn processing_is_the_same_however_the_block_is_split() {
        let params = FilterParams {
            cutoff_hz: 900.0,
            res: 8.0,
            drive: 0.6,
            spread_st: 5.0,
            character: fp::CHAR_LADDER as f32,
            ..FilterParams::default()
        };
        let source_l = sine(300.0, 0.7, BLOCK);
        let source_r = sine(1_800.0, 0.5, BLOCK);

        let mut whole = Rig::new(params);
        let (mut al, mut ar) = (source_l.clone(), source_r.clone());
        whole.run_once(&mut al, &mut ar);

        let mut split = Rig::new(params);
        let (mut bl, mut br) = (source_l.clone(), source_r.clone());
        let mut at = 0usize;
        for take in [100usize, 1, 7, 148] {
            let end = (at + take).min(BLOCK);
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
        let mut rig = Rig::new(FilterParams {
            cutoff_hz: 1_200.0,
            res: 24.0,
            drive: 1.0,
            spread_st: fp::SPREAD_MAX_ST,
            character: fp::CHAR_DIODE as f32,
            slope: 5.0,
            ..FilterParams::default()
        });
        let mut l = vec![0.4f32; BLOCK];
        let mut r = vec![-0.4f32; BLOCK];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..64 {
                rig.run_once(&mut l, &mut r);
            }
        });
    }

    /// Undersized scratch FAILS OPEN, and nonsense in stays finite out at
    /// every extreme the table allows.
    #[test]
    fn it_fails_open_and_never_emits_a_non_finite_sample() {
        // Too little scratch leaves the filtered signal standing undriven
        // rather than handing the graph a buffer nobody wrote.
        let mut core = FilterCore::new();
        core.prepare(FS, FilterParams::default());
        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        let mut tiny = vec![0.0f32; 4];
        core.process(&mut l, &mut r, &mut tiny);
        assert!(l.iter().all(|s| s.is_finite()));

        // A mismatched pair is refused rather than half-filtered.
        let mut short = vec![0.5f32; 8];
        let mut long = vec![0.5f32; 9];
        let before = short.clone();
        let mut scratch = vec![0.0; scratch_len(BLOCK)];
        core.process(&mut short, &mut long, &mut scratch);
        assert_eq!(short, before);

        for def in fp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = FilterParams::default();
                params.set(def.id, value);
                let mut rig = Rig::new(params);
                // Hostile input: a full-scale square has every harmonic.
                let mut a: Vec<f32> = (0..BLOCK)
                    .map(|i| if (i / 4) % 2 == 0 { 1.0 } else { -1.0 })
                    .collect();
                let mut b = a.clone();
                rig.run_once(&mut a, &mut b);
                assert!(
                    a.iter().chain(b.iter()).all(|s| s.is_finite()),
                    "{} at {value}",
                    def.name
                );
            }
        }
    }

    /// Every table id round-trips, and an unknown id is ignored.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = FilterParams::default();
        for def in fp::TABLE {
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

        params.set(fp::CUTOFF, 1e9);
        assert!((params.cutoff_hz - 20_000.0).abs() < 1e-3);
        params.set(fp::CUTOFF, -1e9);
        assert!((params.cutoff_hz - 20.0).abs() < 1e-3);
    }

    /// THE LATENCY IS CONSTANT AND HONEST, and no knob moves it — the
    /// reason the drive stage stays in the path even at zero drive.
    #[test]
    fn the_reported_latency_is_the_real_one() {
        let reported = latency();
        assert_eq!(reported, Oversampler2x::new().latency());

        // An impulse through a wide-open clean filter comes out
        // `reported` samples later.
        let params = FilterParams {
            cutoff_hz: 20_000.0,
            res: fp::FLAT_Q,
            drive: 0.0,
            character: fp::CHAR_CLEAN as f32,
            ..FilterParams::default()
        };
        let mut rig = Rig::new(params);
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        l[0] = 0.5;
        r[0] = 0.5;
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
        for def in fp::TABLE {
            for value in [def.min, def.max] {
                let mut params = FilterParams::default();
                params.set(def.id, value);
                let mut core = FilterCore::new();
                core.prepare(FS, params);
                assert_eq!(latency(), reported, "{} at {value} moved it", def.name);
            }
        }
    }
}
