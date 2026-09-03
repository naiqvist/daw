//! The hand clap's voice — the instrument half of `Node::Handclap`.
//!
//! `handclap`, not `clap`: CLAP in this codebase is the PLUGIN FORMAT the
//! host loads through clack. A module named `clap` sitting next to a
//! plugin host that scans for CLAP plugins is a name that costs somebody
//! an afternoon.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`.
//!
//! # The path, in order
//!
//! ```text
//!                    ┌─▶ burst env (retriggered N times) ─┐
//! white noise ─▶ BANDPASS                                 ├─▶ HIGHPASS
//!                    └─▶ body env (one long decay) ───────┘
//!                        ─▶ SATURATOR ─▶ out
//! ```
//!
//! # A clap is several hands and then a room
//!
//! One noise burst is a snare. What makes a clap is that it is several
//! hands not quite together, followed by the room they are in — so the
//! same band-passed noise runs through TWO envelopes at once: a very
//! short one retriggered a few times a few milliseconds apart, and one
//! long decay underneath that is the room.
//!
//! Both envelopes read the SAME noise, sample for sample. That is not a
//! saving, it is the sound: two independent noise sources would decorrelate
//! and read as two instruments, where one source through two envelopes
//! reads as one event with a tail.
//!
//! # Why the bursts are unevenly spaced
//!
//! Evenly spaced bursts sum to a flam — audibly periodic, a machine gun —
//! because the ear hears equal intervals as a rhythm however short they
//! are. Real hands CONVERGE: the gaps shrink as the claps come together.
//! [`OFFSETS`](crate::params::handclap::OFFSETS) is that uneven pattern,
//! and it is the single thing most responsible for this reading as one
//! clap rather than four taps.
//!
//! # Why the schedule is in samples on the voice's own clock
//!
//! A burst due 10 ms after the strike must land 480 samples after it,
//! whatever block boundaries the transport happens to put in between.
//! Counting down samples inside the voice is what makes 256 equal
//! 100 + 156 — the property the segmented transport depends on.

use crate::dsp::adsr::ExpDecay;
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::handclap as cp;

/// How many samples one run of the render loop covers.
///
/// Not a control rate: it BOUNDS THE SCRATCH, the same way the hat's
/// does. The caller's block length is not bounded; this is.
pub const CHUNK: usize = 32;

/// A clap's settings, in engine units. Parallel to
/// [`params::handclap::TABLE`](crate::params::handclap::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HandclapParams {
    pub bursts: f32,
    pub spread_ms: f32,
    pub burst_ms: f32,
    pub body: f32,
    pub body_ms: f32,
    pub tone_hz: f32,
    pub width: f32,
    pub hp_hz: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for HandclapParams {
    /// Every default read from the one table, so the card, the node and a
    /// fresh project cannot disagree about what a new clap sounds like.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(cp::TABLE, id).default;
        Self {
            bursts: at(cp::BURSTS),
            spread_ms: at(cp::SPREAD),
            burst_ms: at(cp::BURST_DECAY),
            body: at(cp::BODY),
            body_ms: at(cp::BODY_DECAY),
            tone_hz: at(cp::TONE),
            width: at(cp::WIDTH),
            hp_hz: at(cp::HP_HZ),
            drive: at(cp::DRIVE),
            gain: at(cp::GAIN),
        }
    }
}

impl HandclapParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(cp::TABLE, param, value) else {
            return;
        };
        match param {
            cp::BURSTS => self.bursts = value,
            cp::SPREAD => self.spread_ms = value,
            cp::BURST_DECAY => self.burst_ms = value,
            cp::BODY => self.body = value,
            cp::BODY_DECAY => self.body_ms = value,
            cp::TONE => self.tone_hz = value,
            cp::WIDTH => self.width = value,
            cp::HP_HZ => self.hp_hz = value,
            cp::DRIVE => self.drive = value,
            cp::GAIN => self.gain = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            cp::BURSTS => self.bursts,
            cp::SPREAD => self.spread_ms,
            cp::BURST_DECAY => self.burst_ms,
            cp::BODY => self.body,
            cp::BODY_DECAY => self.body_ms,
            cp::TONE => self.tone_hz,
            cp::WIDTH => self.width,
            cp::HP_HZ => self.hp_hz,
            cp::DRIVE => self.drive,
            cp::GAIN => self.gain,
            _ => 0.0,
        }
    }
}

/// The clap's single voice.
pub struct HandclapVoice {
    sample_rate: f32,
    params: HandclapParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch, which a lock may hold
    /// elsewhere for one hit.
    base: HandclapParams,

    noise: WhiteNoise,
    band: Svf,
    high: Svf,

    /// The short envelope, retriggered once per hand.
    burst: ExpDecay,
    /// The long one underneath: the room.
    body: ExpDecay,

    shaper: Waveshaper,
    velocity: f32,

    /// The bursts still to come, as sample counts from NOW, and how many
    /// of them are live.
    ///
    /// A fixed array rather than a queue: the table caps the count at
    /// [`params::handclap::BURSTS_MAX`](crate::params::handclap::BURSTS_MAX),
    /// and a red-zone path may not grow anything.
    pending: [u32; cp::OFFSETS.len()],
    pending_len: usize,

    tuned_band_hz: f32,
    tuned_width: f32,
    tuned_hp_hz: f32,

    noise_scratch: [f32; CHUNK],
    burst_scratch: [f32; CHUNK],
    body_scratch: [f32; CHUNK],
}

impl Default for HandclapVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl HandclapVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: HandclapParams::default(),
            base: HandclapParams::default(),
            noise: WhiteNoise::new(),
            band: Svf::new(),
            high: Svf::new(),
            burst: ExpDecay::new(),
            body: ExpDecay::new(),
            shaper: Waveshaper::new(),
            velocity: 1.0,
            pending: [0; cp::OFFSETS.len()],
            pending_len: 0,
            tuned_band_hz: 0.0,
            tuned_width: 0.0,
            tuned_hp_hz: 0.0,
            noise_scratch: [0.0; CHUNK],
            burst_scratch: [0.0; CHUNK],
            body_scratch: [0.0; CHUNK],
        }
    }

    /// Green zone: settle every kernel. Nothing here allocates at all —
    /// a clap has no wavetable to build.
    pub fn prepare(&mut self, sample_rate: f32, params: HandclapParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.noise.seed(0xc1a9_0000_1234_abcd);
        self.params = params;
        self.apply_envelopes();
        self.retune(true);
        self.reset();
    }

    fn apply_envelopes(&mut self) {
        let fs = self.sample_rate;
        self.burst.prepare(fs, self.params.burst_ms);
        self.body.prepare(fs, self.params.body_ms);
    }

    /// The band and the output highpass, rebuilt only when something
    /// moved.
    ///
    /// [`Mode::BandpassUnity`] for the band: the width knob must narrow
    /// the band without also making it louder, or "width" is secretly a
    /// second level control.
    fn retune(&mut self, force: bool) {
        let ceiling = self.sample_rate * 0.45;
        let band = self.params.tone_hz.clamp(20.0, ceiling);
        let width = self.params.width;
        let high = self.params.hp_hz.clamp(20.0, ceiling);
        if force
            || (band - self.tuned_band_hz).abs() > self.tuned_band_hz.max(1.0) * 1e-4
            || (width - self.tuned_width).abs() > 1e-4
        {
            self.tuned_band_hz = band;
            self.tuned_width = width;
            self.band.prepare(self.sample_rate, band, width);
        }
        if force || (high - self.tuned_hp_hz).abs() > self.tuned_hp_hz.max(1.0) * 1e-4 {
            self.tuned_hp_hz = high;
            self.high
                .prepare(self.sample_rate, high, core::f32::consts::FRAC_1_SQRT_2);
        }
    }

    /// Green zone: silence everything, keep the settings.
    pub fn reset(&mut self) {
        self.noise.reset();
        self.band.reset();
        self.high.reset();
        self.burst.reset();
        self.body.reset();
        self.pending_len = 0;
    }

    /// A letter: the knob moves, and the live patch with it.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

    /// A parameter LOCK at a note boundary: `Some` holds the live patch
    /// at the note's own value, `None` returns it to the knob. The knob
    /// itself never moves, so a lock is heard on its hit and no other.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let value = value.unwrap_or_else(|| self.base.get(param));
        self.apply_param(param, value);
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        let live = self.params.get(param);
        let base = self.base.get(param);
        self.apply_param(param, live + (base - live) * alpha.clamp(0.0, 1.0));
    }

    fn apply_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            cp::BURST_DECAY | cp::BODY_DECAY => self.apply_envelopes(),
            cp::TONE | cp::WIDTH | cp::HP_HZ => self.retune(false),
            _ => {}
        }
    }

    pub fn params(&self) -> HandclapParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.burst.active() || self.body.active() || self.pending_len > 0
    }

    /// Strike: fire the first hand and schedule the rest.
    pub fn trigger(&mut self, _pitch: u8, velocity: u8) {
        self.velocity = f32::from(velocity.max(1)) / 127.0;

        let hands = (self.params.bursts.round().max(1.0) as usize).min(cp::OFFSETS.len());
        let spread = (self.sample_rate * self.params.spread_ms / 1_000.0).max(0.0);

        // The first hand is now; the rest are queued at their offsets. A
        // retrigger REPLACES the queue rather than adding to it — a clap
        // struck again mid-clap is one clap, not seven hands.
        self.pending_len = 0;
        for offset in cp::OFFSETS.iter().take(hands).skip(1) {
            let at = (offset * spread).round();
            let Some(slot) = self.pending.get_mut(self.pending_len) else {
                break;
            };
            *slot = if at.is_finite() && at > 0.0 {
                at.min(u32::MAX as f32) as u32
            } else {
                0
            };
            self.pending_len += 1;
        }

        self.burst.trigger(1.0);
        self.body.trigger(1.0);
    }

    /// Red zone: render into `out`, ADDING to what is there.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        let mut written = 0usize;
        while written < out.len() {
            // A run stops at the next scheduled burst or at the scratch
            // bound, whichever comes first — so a burst lands on its own
            // sample rather than at the start of whatever block contains
            // it.
            let take = CHUNK.min(out.len() - written).min(self.until_next_burst());
            let take = take.max(1).min(out.len() - written);
            let Some(block) = out.get_mut(written..written + take) else {
                return;
            };
            self.render_run(block, gain);
            self.advance_schedule(take);
            written += take;
        }
    }

    /// Samples until the next scheduled burst, or [`CHUNK`] if none is
    /// due inside this run.
    fn until_next_burst(&self) -> usize {
        let mut soonest = CHUNK;
        for at in self.pending.iter().take(self.pending_len) {
            let at = *at as usize;
            if at > 0 && at < soonest {
                soonest = at;
            }
        }
        soonest.max(1)
    }

    /// Move the schedule on by `n` samples, firing anything that came
    /// due. Bounded by the queue's fixed length.
    fn advance_schedule(&mut self, n: usize) {
        let n = n as u32;
        let mut fired = false;
        let mut kept = 0usize;
        for i in 0..self.pending_len {
            let Some(at) = self.pending.get(i).copied() else {
                break;
            };
            if at <= n {
                fired = true;
            } else {
                let remaining = at - n;
                if let Some(slot) = self.pending.get_mut(kept) {
                    *slot = remaining;
                }
                kept += 1;
            }
        }
        self.pending_len = kept;
        if fired {
            // Another hand. The BODY is left alone — the room is one
            // decay from the first clap, not one per hand, which is what
            // keeps the tail from stepping up in level.
            self.burst.trigger(1.0);
        }
    }

    /// One run of samples: per-sample work only.
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(noise), Some(burst), Some(body)) = (
            self.noise_scratch.get_mut(..n),
            self.burst_scratch.get_mut(..n),
            self.body_scratch.get_mut(..n),
        ) else {
            return;
        };

        // --- ONE noise source, band-limited ---------------------------
        //
        // One, and that is the sound: two independent noise sources under
        // the two envelopes would decorrelate and read as two
        // instruments, where one source through two envelopes reads as
        // one event with a tail.
        self.noise.process(noise);
        self.band.process(noise, Mode::BandpassUnity);

        // --- the two envelopes, summed into one gain ------------------
        //
        // Both advance every sample whether or not they are audible, so
        // neither one's timing depends on the other's level.
        self.burst.process(burst);
        self.body.process(body);
        let body_level = self.params.body;
        for (sample, (hand, room)) in noise.iter_mut().zip(burst.iter().zip(body.iter())) {
            *sample *= *hand + *room * body_level;
        }

        // --- the output stage -----------------------------------------
        self.high.process(noise, Mode::Highpass);
        self.shaper
            .configure(ShapeMode::SoftClip, self.params.drive, 0.0, 1.0);
        self.shaper.process(noise);

        let level = self.params.gain * gain * self.velocity;
        for (sample, voice) in out.iter_mut().zip(noise.iter()) {
            *sample += *voice * level;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(params: HandclapParams) -> HandclapVoice {
        let mut v = HandclapVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut HandclapVoice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// The loudness of each millisecond, which is how a burst pattern is
    /// actually seen: the bursts are peaks in this curve.
    fn envelope(buf: &[f32], window: usize) -> Vec<f32> {
        buf.chunks(window).map(rms).collect()
    }

    #[test]
    fn a_fresh_clap_is_silent_until_it_is_struck() {
        let mut v = voice(HandclapParams::default());
        let quiet = render(&mut v, 512);
        assert!(quiet.iter().all(|s| *s == 0.0), "silent before the strike");
        assert!(!v.active());

        v.trigger(39, 100);
        assert!(v.active());
        let hit = render(&mut v, 512);
        assert!(peak(&hit) > 0.01, "the strike made sound: {}", peak(&hit));
    }

    /// THE HANDS ARE SEPARATE EVENTS, AND THEY ARE UNEVENLY SPACED.
    ///
    /// The whole instrument is this: three sharp attacks a few
    /// milliseconds apart. If they smeared into one the sound would be a
    /// snare, and if they were evenly spaced it would be a flam.
    #[test]
    fn the_bursts_land_on_their_scheduled_samples() {
        let params = HandclapParams {
            bursts: 3.0,
            spread_ms: 10.0,
            burst_ms: 3.0,
            // The room off, so what is measured is the hands alone.
            body: 0.0,
            drive: 1.0,
            ..HandclapParams::default()
        };
        let mut v = voice(params);
        v.trigger(39, 127);
        let buf = render(&mut v, 4_800); // 100 ms

        // One millisecond per bin, so a bin index IS a millisecond.
        let ms = envelope(&buf, 48);
        let loud = |at: usize| ms.get(at).copied().unwrap_or(0.0);

        // The first hand is at sample zero, so it has no "before" to be
        // louder than — it is simply the loudest thing in the sound.
        assert!(loud(0) > 0.01, "the first hand did not arrive: {}", loud(0));

        // The others are at 10 ms and 19 ms — OFFSETS × spread — and each
        // must be a STEP UP from the dying tail of the one before it.
        // With a 3 ms burst the gap is 60 dB down by then, so the step is
        // not subtle.
        for (hand, at) in cp::OFFSETS.iter().enumerate().take(3).skip(1) {
            let want = (at * 10.0).round() as usize;
            let here = loud(want);
            let before = loud(want - 2);
            assert!(
                here > before * 4.0,
                "hand {hand} at {want} ms did not arrive: {here} against {before}"
            );
        }

        // And the gaps SHRINK — 10 ms then 9 ms, not 10 and 10. A
        // compile-time fact about the constant, so it is checked at
        // compile time: evenly spaced bursts are a flam, not a clap.
        const _: () = assert!(cp::OFFSETS[1] - cp::OFFSETS[0] == 1.0);
        const _: () = assert!(
            cp::OFFSETS[2] - cp::OFFSETS[1] < cp::OFFSETS[1] - cp::OFFSETS[0],
            "evenly spaced bursts are a flam, not a clap"
        );

        // By 60 ms the hands are done: with the body off, silence.
        assert!(loud(60) < loud(0) * 0.01, "the hands rang on: {}", loud(60));
    }

    /// THE ROOM OUTLASTS THE HANDS. That tail is what turns three taps
    /// into a clap in a room, and it decays from the FIRST hand rather
    /// than restarting on each one — otherwise the tail steps up in level
    /// halfway through the sound.
    #[test]
    fn the_body_is_one_long_tail_under_all_the_hands() {
        let params = HandclapParams {
            bursts: 3.0,
            spread_ms: 10.0,
            burst_ms: 3.0,
            body: 1.0,
            body_ms: 400.0,
            drive: 1.0,
            ..HandclapParams::default()
        };
        let mut v = voice(params);
        v.trigger(39, 127);
        let buf = render(&mut v, (FS * 0.2) as usize);
        let ms = envelope(&buf, 48);

        // At 60 ms the hands are long gone but the room is not.
        let tail = ms.get(60).copied().unwrap_or(0.0);
        assert!(tail > 1e-4, "the room went with the hands: {tail}");

        // And the tail only ever falls after the last hand. Measured over
        // BROAD windows, because the millisecond-to-millisecond RMS of a
        // noise source is jittery by nature — the question is whether the
        // envelope under it is falling, not whether every bin is.
        let span = |from: usize, to: usize| {
            let slice = &ms[from.min(ms.len())..to.min(ms.len())];
            if slice.is_empty() {
                0.0
            } else {
                slice.iter().sum::<f32>() / slice.len() as f32
            }
        };
        let windows = [span(30, 60), span(60, 100), span(100, 140), span(140, 190)];
        for pair in windows.windows(2) {
            assert!(pair[1] < pair[0], "the tail stepped back up: {windows:?}");
        }
    }

    /// THE HAND COUNT IS A COUNT. One hand is one burst; four is four.
    #[test]
    fn the_hand_count_changes_how_many_bursts_arrive() {
        let base = HandclapParams {
            spread_ms: 12.0,
            burst_ms: 3.0,
            body: 0.0,
            drive: 1.0,
            ..HandclapParams::default()
        };
        let count_peaks = |hands: f32| {
            let mut v = voice(HandclapParams {
                bursts: hands,
                ..base
            });
            v.trigger(39, 127);
            let buf = render(&mut v, 4_800);
            let ms = envelope(&buf, 48);
            // A local maximum that is well clear of its neighbours.
            ms.windows(3)
                .filter(|w| w[1] > w[0] * 1.3 && w[1] > w[2])
                .count()
                + 1 // the burst at sample zero has no bin before it
        };

        assert_eq!(count_peaks(1.0), 1, "one hand should be one burst");
        let three = count_peaks(3.0);
        assert!(
            (3..=4).contains(&three),
            "three hands should be three bursts: {three}"
        );

        // A count above the table's cap is clamped rather than reading
        // past the offsets array.
        let mut v = voice(HandclapParams {
            bursts: cp::BURSTS_MAX + 10.0,
            ..base
        });
        v.trigger(39, 127);
        assert!(v.pending_len < cp::OFFSETS.len());
    }

    /// A RETRIGGER REPLACES THE CLAP. A clap struck again mid-clap is one
    /// clap, not seven hands and 6 dB of extra peak.
    #[test]
    fn a_retrigger_replaces_the_clap_rather_than_layering_on_it() {
        let params = HandclapParams {
            body_ms: 800.0,
            drive: 1.0,
            ..HandclapParams::default()
        };
        let mut once = voice(params);
        once.trigger(39, 127);
        let single = peak(&render(&mut once, 4_800));

        let mut twice = voice(params);
        twice.trigger(39, 127);
        let _ = render(&mut twice, 240);
        twice.trigger(39, 127);
        let doubled = peak(&render(&mut twice, 4_800));

        assert!(
            doubled <= single * 1.05,
            "the retrigger stacked: {doubled} against {single}"
        );
        // And the queue was replaced, not appended to.
        assert!(twice.pending_len < cp::OFFSETS.len());
    }

    /// SPLIT-BLOCK EQUIVALENCE, and it is the burst SCHEDULE that makes
    /// this worth having: a hand due 480 samples in must land there
    /// whatever block boundaries the transport puts in between.
    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = HandclapParams::default();
        let frames = 2_048usize;

        let mut whole = voice(params);
        whole.trigger(39, 100);
        let mut a = vec![0.0f32; frames];
        whole.render_add(&mut a, 1.0);

        // Splits that fall in awkward places relative to the 10 ms
        // spacing and the 32-sample scratch bound alike.
        let mut split = voice(params);
        split.trigger(39, 100);
        let mut b = vec![0.0f32; frames];
        let mut at = 0usize;
        for take in [100usize, 1, 7, 333, 512, 63] {
            let end = (at + take).min(frames);
            split.render_add(&mut b[at..end], 1.0);
            at = end;
        }
        split.render_add(&mut b[at..], 1.0);

        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "the split run diverged from the whole one"
        );
    }

    /// The render path allocates nothing.
    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(HandclapParams {
            bursts: cp::BURSTS_MAX,
            drive: 8.0,
            width: 8.0,
            ..HandclapParams::default()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(39, 100);
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    /// Nonsense in, finite out — at every extreme the table allows.
    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in cp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = HandclapParams::default();
                params.set(def.id, value);
                let mut v = voice(params);
                v.trigger(39, 127);
                let out = render(&mut v, 2_048);
                assert!(out.iter().all(|s| s.is_finite()), "{} at {value}", def.name);
            }
        }

        // A zero-length block advances nothing and returns cleanly.
        let mut v = voice(HandclapParams::default());
        v.trigger(39, 127);
        v.render_add(&mut [], 1.0);
        assert!(v.active());
    }

    /// Every table id round-trips, and an unknown id is ignored.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = HandclapParams::default();
        for def in cp::TABLE {
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

        params.set(cp::TONE, 1e9);
        assert!((params.tone_hz - 4_000.0).abs() < 1e-3);
        params.set(cp::TONE, -1e9);
        assert!((params.tone_hz - 300.0).abs() < 1e-3);
    }
}
