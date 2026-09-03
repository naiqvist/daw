//! ECHO: the analogue delay.
//!
//! A line with its repeats bent: the loop has a tone control and a
//! soft top, so each pass is duller and rounder than the last and a
//! hot feedback growls instead of screaming. The head WANDERS — a slow
//! wobble on the delay time — and the time GLIDES to a new setting
//! rather than jumping, so turning the knob is a swoop, which is the
//! thing a bucket-brigade delay does that a digital one has to be
//! asked for.
//!
//! SYNC ties the time to the clock the node is handed, in five
//! divisions from a sixteenth to a half, or lets it run FREE in
//! milliseconds. PING-PONG sends the repeats across the sides, each
//! side feeding the other, which is the two-head trick. MIX at zero is
//! a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::filters::OnePole;
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::params::console::echo as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// `None` is FREE.
    pub sync: Option<f32>,
    pub time_ms: f32,
    pub feedback: f32,
    pub tone: f32,
    pub wow: f32,
    pub pingpong: bool,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Echo.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        let sync = (clamp(p::SYNC).round().max(0.0) as usize).min(p::SYNC_BEATS.len());
        Self {
            sync: (sync != p::FREE as usize).then(|| p::SYNC_BEATS[sync - 1]),
            time_ms: clamp(p::TIME),
            feedback: clamp(p::FEEDBACK) / 100.0,
            tone: clamp(p::TONE),
            wow: clamp(p::WOW) / 100.0,
            pingpong: clamp(p::PINGPONG) >= 0.5,
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }
}

pub struct EchoCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    lines: [DelayLine; 2],
    bufs: [Vec<f32>; 2],
    tone: [OnePole; 2],
    wow: Lfo,
    /// What each line gave back last sample.
    fed: [f32; 2],
    /// The time in samples, glided.
    time: f32,
    target: f32,
    glide: f32,
    max: usize,
    level_db: f32,
    /// What the loop's soft top took at the block's hottest sample, dB.
    reduction_db: f32,
    /// The head's offset from the glided time at the block's last
    /// sample, in ms, signed.
    wander_ms: f32,
    /// One beat in ms at the tempo the block ran at; 0.0 when stopped.
    beat_ms: f32,
}

impl EchoCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let max = (sample_rate * p::MAX_MS / 1000.0).ceil() as usize + 4;
        let settings = Settings::of(params);
        let mut wow = Lfo::new();
        wow.prepare(sample_rate);
        wow.set_shape(LfoShape::Sine);
        wow.set_rate(p::WOW_HZ);
        let mut core = Self {
            params: params.clone(),
            settings,
            sample_rate,
            lines: [DelayLine::new(), DelayLine::new()],
            bufs: [vec![0.0; buffer_len(max)], vec![0.0; buffer_len(max)]],
            tone: [OnePole::new(), OnePole::new()],
            wow,
            fed: [0.0; 2],
            time: settings.time_ms * sample_rate / 1000.0,
            target: settings.time_ms * sample_rate / 1000.0,
            glide: 1.0 - (-1.0 / (p::GLIDE_MS * sample_rate / 1000.0)).exp(),
            max,
            level_db: -120.0,
            reduction_db: 0.0,
            wander_ms: 0.0,
            beat_ms: 0.0,
        };
        for line in &mut core.lines {
            line.prepare(max);
        }
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// The time the line is aimed at, in samples.
    pub fn time_samples(&self) -> f32 {
        self.time
    }

    fn tune(&mut self) {
        let s = self.settings;
        for tone in &mut self.tone {
            tone.prepare(self.sample_rate, s.tone);
        }
    }

    /// The loop's soft top.
    #[inline(always)]
    fn soft(x: f32) -> f32 {
        let k = 1.0 / p::LOOP_DRIVE;
        (x * k).tanh() / k
    }
}

impl SectionCore for EchoCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for (line, buf) in self.lines.iter_mut().zip(self.bufs.iter_mut()) {
            line.reset();
            buf.fill(0.0);
        }
        for tone in &mut self.tone {
            tone.reset();
        }
        self.wow.reset();
        self.fed = [0.0; 2];
        self.time = self.target;
        self.level_db = -120.0;
        self.reduction_db = 0.0;
        self.wander_ms = 0.0;
        self.beat_ms = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        let per_ms = self.sample_rate / 1000.0;
        // One beat in milliseconds at this block's tempo, so the face
        // can stand a division at its true time. Zero when the
        // transport hands us no tempo, which is the face's cue to draw
        // no divisions at all.
        self.beat_ms = if clock.beats_per_sample > 0.0 {
            (1.0 / (clock.beats_per_sample * f64::from(per_ms))) as f32
        } else {
            0.0
        };
        // Where the head is aimed: the clock's division, or the knob's
        // milliseconds. Taken every block, wire or not, so the head is
        // placed before the section is mixed in.
        let want = match s.sync {
            Some(beats) if clock.beats_per_sample > 0.0 => {
                (f64::from(beats) / clock.beats_per_sample) as f32
            }
            _ => s.time_ms * per_ms,
        };
        self.target = want.clamp(2.0, (self.max - 4) as f32);
        if !s.is_wire() {
            let inv_per_ms = 1000.0 / self.sample_rate;
            let mut loop_peak = 0.0f32;
            let mut one = [0.0f32; 1];
            for i in 0..n {
                // The time glides toward its target and the head
                // wanders about it.
                self.time += (self.target - self.time) * self.glide;
                self.wow.process(&mut one);
                let wander = 1.0 + p::WOW_DEPTH * s.wow * one[0];
                let at = (self.time * wander).clamp(2.0, (self.max - 4) as f32);
                // What the head is actually reading, minus where it is
                // aimed: the wobble itself, for the face to swing on.
                self.wander_ms = (at - self.time) * inv_per_ms;
                let dry_l = l[i];
                let dry_r = if stereo { r[i] } else { dry_l };
                // Ping-pong: each side's loop is fed by the other's
                // repeat, so a hit walks across the field.
                let (in_l, in_r) = if s.pingpong && stereo {
                    (
                        dry_l + self.fed[1] * s.feedback,
                        dry_r + self.fed[0] * s.feedback,
                    )
                } else {
                    (
                        dry_l + self.fed[0] * s.feedback,
                        dry_r + self.fed[1] * s.feedback,
                    )
                };
                let mut wet_l = [in_l];
                self.lines[0].set_delay(at);
                self.lines[0].process_modulated(&mut wet_l, &mut self.bufs[0], &[at]);
                let hot_l = self.tone[0].tick_lowpass(wet_l[0]);
                loop_peak = loop_peak.max(hot_l.abs());
                let out_l = Self::soft(hot_l);
                self.fed[0] = out_l;
                l[i] = dry_l + (out_l - dry_l) * s.mix;
                if stereo {
                    let mut wet_r = [in_r];
                    self.lines[1].set_delay(at);
                    self.lines[1].process_modulated(&mut wet_r, &mut self.bufs[1], &[at]);
                    let mut out_r = self.tone[1].tick_lowpass(wet_r[0]);
                    out_r = Self::soft(out_r);
                    self.fed[1] = out_r;
                    r[i] = dry_r + (out_r - dry_r) * s.mix;
                }
            }
            // What the soft top took at the block's hottest sample.
            // `soft` is monotone and compressive, so its worst squeeze
            // is at the loop's peak and this is never positive.
            self.reduction_db = if loop_peak > p::READOUT_QUIET {
                20.0 * (Self::soft(loop_peak) / loop_peak).log10()
            } else {
                0.0
            };
        } else {
            // A wire: nothing goes round, so nothing wanders and
            // nothing is squeezed — but the head still snaps to where
            // it will land, or a card set up before MIX comes up shows
            // a stale time.
            self.time = self.target;
            self.wander_ms = 0.0;
            self.reduction_db = 0.0;
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

    /// What the echo says it did, field by field. A pure copy of
    /// fields: every peak, glide and log10 happens in `process`, never
    /// here, because the graph reads this on its telemetry step.
    ///
    /// - `level_db`: the loudest sample of the section's OUTPUT this
    ///   block — dry and wet, after the mix — in dBFS, floored at -120.
    ///   The block's extreme, unsmoothed, as everywhere on the desk.
    /// - `reduction_db`: what the loop's SOFT TOP took at the block's
    ///   hottest sample, in dB. `20*log10(soft(peak)/peak)` where
    ///   `peak` is the largest `|left|` inside the loop, after the tone
    ///   filter and before the saturator. Zero or negative, never
    ///   positive: about -0.9 dB at a loop peak of 0.2, -9.2 dB at 1.0,
    ///   and floored by `soft`'s own ceiling of `LOOP_DRIVE`. Exactly
    ///   0.0 while `is_wire()` or while the loop is under
    ///   `READOUT_QUIET`. Unsmoothed — it is this block's worst.
    /// - `bands[0]` TIME: the delay the line is actually reading, in
    ///   MILLISECONDS, over the line's own range of 2 samples up to
    ///   `MAX_MS`. The GLIDED time, not the setting: a turn of TIME or
    ///   a change of SYNC moves it with a one-pole of `GLIDE_MS`
    ///   (120 ms, so it is ~63% of the way there after 120 ms and
    ///   settled inside half a second), and a tempo change slides it
    ///   while it plays. Live on the wire path too, where it snaps to
    ///   its target rather than gliding, because nothing is being
    ///   heard to swoop.
    /// - `bands[1]` WANDER: the head's live offset from that time, in
    ///   MILLISECONDS and SIGNED — the block's LAST sample's
    ///   `at - time`, so it is the wobble alone with the glide taken
    ///   out. It swings at `WOW_HZ` (0.6 Hz) across
    ///   ±`WOW_DEPTH * wow * bands[0]`, which is ±2 ms at a 500 ms time
    ///   and full WOW. Sampled once a block, not smoothed: it IS the
    ///   LFO. Exactly 0.0 at WOW 0 and while `is_wire()`.
    /// - `bands[2]` BEAT: one beat in MILLISECONDS at the tempo this
    ///   block ran at — 500.0 at 120 BPM — so a division stands at
    ///   `SYNC_BEATS[d] * bands[2]`. Read once a block, unsmoothed, and
    ///   live on the wire path too. Exactly 0.0 when the transport
    ///   hands the block no tempo, which is a stopped desk.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.reduction_db,
            bands: [
                self.time / (self.sample_rate / 1000.0),
                self.wander_ms,
                self.beat_ms,
            ],
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

    fn core_with(edits: &[(u32, f32)]) -> EchoCore {
        let mut params = SectionParams::of(SectionKind::Echo);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        EchoCore::new(&params, FS, BLOCK)
    }

    fn run(core: &mut EchoCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn run_stereo(core: &mut EchoCore, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (l.to_vec(), r.to_vec());
        for start in (0..ol.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(ol.len());
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        (ol, or)
    }

    /// Where the loud moments are, in samples.
    fn hits(signal: &[f32], floor: f32) -> Vec<usize> {
        let mut out = Vec::new();
        let mut armed = true;
        for (i, s) in signal.iter().enumerate() {
            if s.abs() > floor && armed {
                out.push(i);
                armed = false;
            } else if s.abs() < floor * 0.3 {
                armed = true;
            }
        }
        out
    }

    fn click(n: usize) -> Vec<f32> {
        let mut l = vec![0.0f32; n];
        l[0] = 1.0;
        l
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l: Vec<f32> = (0..len).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// The repeats land at the time asked for, and each is quieter
    /// than the one before.
    #[test]
    fn the_repeats_land_on_time_and_fade() {
        let n = FS as usize;
        let l = click(n);
        let mut core = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 60.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ]);
        let out = run(&mut core, &l);
        let at = hits(&out, 0.02);
        assert!(at.len() >= 3, "fewer than three repeats: {at:?}");
        let step = FS as usize / 10;
        for (k, hit) in at.iter().take(3).enumerate() {
            let want = step * (k + 1);
            assert!(
                (*hit as isize - want as isize).abs() < 400,
                "repeat {k} at {hit}, wanted {want}"
            );
        }
        let level = |i: usize| out[i..i + 64].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            level(at[1]) < level(at[0]) * 0.9,
            "the repeats did not fade"
        );
    }

    /// The tone control dulls the repeats: a later repeat has less top
    /// than an earlier one.
    #[test]
    fn each_repeat_is_duller_than_the_last() {
        let n = FS as usize;
        // A click has every frequency; the repeats should lose their
        // top as they go round.
        let l = click(n);
        let mut core = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 70.0),
            (p::MIX, 100.0),
            (p::TONE, 2_000.0),
            (p::WOW, 0.0),
        ]);
        let out = run(&mut core, &l);
        let step = FS as usize / 10;
        let edge = |from: usize| -> f32 {
            let win = &out[from..from + 400];
            win.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>()
                / win.iter().map(|s| s.abs()).sum::<f32>().max(1e-9)
        };
        let first = edge(step - 100);
        let third = edge(step * 3 - 100);
        assert!(
            third < first * 0.9,
            "the repeats kept their top: {first} then {third}"
        );
    }

    /// Synced — which is how it arrives — the time follows the clock
    /// rather than the knob.
    #[test]
    fn sync_follows_the_clock() {
        let n = FS as usize;
        let l = click(n);
        // A quarter note at 120 bpm is half a second.
        let mut core = core_with(&[
            (p::SYNC, 4.0),
            (p::TIME, 50.0),
            (p::FEEDBACK, 40.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ]);
        let out = run(&mut core, &l);
        let at = hits(&out, 0.02);
        assert!(!at.is_empty(), "no repeat");
        let want = FS as usize / 2;
        assert!(
            (at[0] as isize - want as isize).abs() < 2_000,
            "the synced repeat landed at {}, wanted {want}",
            at[0]
        );
    }

    /// A turn of the time knob glides rather than jumping.
    #[test]
    fn the_time_glides_to_its_new_setting() {
        let mut core = core_with(&[(p::SYNC, 0.0), (p::TIME, 100.0), (p::MIX, 100.0)]);
        let settled = core.time_samples();
        core.set_param(p::TIME, 400.0);
        let mut silence = vec![0.0f32; BLOCK];
        core.process(&mut silence, &mut [], &clock());
        let after_a_block = core.time_samples();
        assert!(after_a_block > settled, "the time did not move");
        assert!(
            after_a_block < settled * 1.5,
            "the time jumped: {settled} to {after_a_block}"
        );
        for _ in 0..200 {
            let mut silence = vec![0.0f32; BLOCK];
            core.process(&mut silence, &mut [], &clock());
        }
        assert!((core.time_samples() - 400.0 * FS / 1000.0).abs() < 100.0);
    }

    /// Ping-pong walks the repeats across the field: with a click on
    /// the left, the first repeat is left, the second is right, the
    /// third is left again. Switched off, they all stay put.
    #[test]
    fn ping_pong_walks_the_repeats_across() {
        let n = FS as usize / 2;
        let l = click(n);
        let r = vec![0.0f32; n];
        let step = FS as usize / 10;
        let loudest = |s: &[f32], k: usize| {
            s[step * k - 200..step * k + 200]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()))
        };
        let mut core = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 70.0),
            (p::MIX, 100.0),
            (p::PINGPONG, 1.0),
            (p::WOW, 0.0),
        ]);
        let (ol, or) = run_stereo(&mut core, &l, &r);
        assert!(
            loudest(&ol, 1) > loudest(&or, 1),
            "the first repeat was not on the left"
        );
        assert!(
            loudest(&or, 2) > loudest(&ol, 2),
            "the second repeat did not cross"
        );
        assert!(
            loudest(&ol, 3) > loudest(&or, 3),
            "the third did not cross back"
        );

        let mut put = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 70.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ]);
        let (ol, or) = run_stereo(&mut put, &l, &r);
        assert!(
            loudest(&ol, 2) > loudest(&or, 2),
            "the repeats crossed with ping-pong off"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = click(1000);
        let edits = [
            (p::SYNC, 0.0),
            (p::TIME, 20.0),
            (p::FEEDBACK, 50.0),
            (p::MIX, 80.0),
        ];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-4);
        }
    }

    /// Runs a block at a time and hands back what the section SAID
    /// after each one, which is the only way to watch a band move.
    fn said(core: &mut EchoCore, l: &[f32], clock: &Clock) -> Vec<Readout> {
        let mut buf = l.to_vec();
        let mut out = Vec::new();
        for start in (0..buf.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(buf.len());
            core.process(&mut buf[start..end], &mut [], clock);
            out.push(core.readout());
        }
        out
    }

    /// `bands[1]` is the head's REAL wander and not the WOW setting: it
    /// swings both ways at the LFO's own rate while sound goes round,
    /// and it is dead still when WOW is off.
    #[test]
    fn the_wander_band_swings_with_the_head() {
        let n = FS as usize * 2;
        let tone: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.2).collect();
        let wowed = &[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 30.0),
            (p::MIX, 100.0),
            (p::WOW, 100.0),
        ];
        let mut core = core_with(wowed);
        let seen = said(&mut core, &tone, &clock());
        let hi = seen.iter().fold(f32::MIN, |m, s| m.max(s.bands[1]));
        let lo = seen.iter().fold(f32::MAX, |m, s| m.min(s.bands[1]));
        // WOW_DEPTH is 2% of the time, so ±2 ms at 100 ms, both ways
        // inside the LFO's 1.67 s period.
        assert!(hi > 1.5, "the head never wandered late: {hi} ms");
        assert!(lo < -1.5, "the head never wandered early: {lo} ms");
        // And it is a swing, not a drift: it crosses zero.
        assert!(
            seen.windows(2).any(|w| w[0].bands[1] * w[1].bands[1] < 0.0),
            "the wander never crossed the head's aim"
        );

        let mut still = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::FEEDBACK, 30.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ]);
        for s in said(&mut still, &tone, &clock()) {
            assert_eq!(s.bands[1], 0.0, "the head wandered with WOW at zero");
        }
    }

    /// `bands[2]` is the transport's beat in milliseconds, so the face
    /// can stand a division at its true time — and it is zero when the
    /// desk is not rolling.
    #[test]
    fn the_beat_band_is_the_tempo() {
        let mut core = core_with(&[(p::MIX, 100.0), (p::WOW, 0.0)]);
        let mut quiet = vec![0.0f32; BLOCK];
        core.process(&mut quiet, &mut [], &clock());
        // 120 BPM: half a second to the beat.
        assert!((core.readout().bands[2] - 500.0).abs() < 0.01);
        let double = Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 240.0 / 60.0 / f64::from(FS),
        };
        core.process(&mut quiet, &mut [], &double);
        assert!((core.readout().bands[2] - 250.0).abs() < 0.01);
        let stopped = Clock {
            playing: false,
            beat: 0.0,
            beats_per_sample: 0.0,
        };
        core.process(&mut quiet, &mut [], &stopped);
        assert_eq!(core.readout().bands[2], 0.0);
    }

    /// `reduction_db` is the loop's soft top actually squeezing: a hot
    /// loop growls, a quiet one is left alone, and a wire never growls.
    #[test]
    fn the_soft_top_reports_what_it_took() {
        let hot: Vec<f32> = (0..BLOCK * 32).map(|i| (i as f32 * 0.05).sin()).collect();
        let quiet: Vec<f32> = hot.iter().map(|s| s * 0.005).collect();
        let edits = &[
            (p::SYNC, 0.0),
            (p::TIME, 10.0),
            (p::FEEDBACK, 70.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ];
        let mut core = core_with(edits);
        let worst = said(&mut core, &hot, &clock())
            .iter()
            .fold(0.0f32, |m, s| m.min(s.reduction_db));
        assert!(worst < -3.0, "the loop never growled: {worst} dB");

        let mut easy = core_with(edits);
        let softest = said(&mut easy, &quiet, &clock())
            .iter()
            .fold(0.0f32, |m, s| m.min(s.reduction_db));
        assert!(softest > -0.05, "a quiet loop was squeezed: {softest} dB");
        assert!(softest <= 0.0, "the soft top gave gain: {softest} dB");
    }

    /// At its defaults the section is a wire and says so — no wander,
    /// no growl, no signal. But `bands[0]` and `bands[2]` stay LIVE:
    /// the head is where it will land the moment MIX comes up, not
    /// where it was left.
    #[test]
    fn at_rest_it_says_rest_but_the_head_is_still_placed() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        let mut quiet = vec![0.0f32; BLOCK];
        core.process(&mut quiet, &mut [], &clock());
        let s = core.readout();
        assert_eq!(s.level_db, -120.0, "a wire on silence read a level");
        assert_eq!(s.reduction_db, 0.0, "a wire growled");
        assert_eq!(s.bands[1], 0.0, "a wire wandered");
        // SYNC arrives on the quarter, and a quarter at 120 BPM is
        // 500 ms — which is where the head must already be.
        assert!(
            (s.bands[0] - 500.0).abs() < 1.0,
            "head at {} ms",
            s.bands[0]
        );
        assert!((s.bands[2] - 500.0).abs() < 0.01);

        // And it follows a letter while still a wire, rather than
        // showing the card a stale time.
        core.set_param(p::SYNC, 0.0);
        core.set_param(p::TIME, 250.0);
        core.process(&mut quiet, &mut [], &clock());
        let moved = core.readout().bands[0];
        assert!(
            (moved - 250.0).abs() < 1.0,
            "the wire's head stuck at {moved}"
        );
    }

    /// `bands[0]` is the GLIDED time, so a turn of TIME makes the face
    /// swoop rather than jump — the same glide the ear hears.
    #[test]
    fn the_time_band_glides_rather_than_jumping() {
        let mut core = core_with(&[
            (p::SYNC, 0.0),
            (p::TIME, 100.0),
            (p::MIX, 100.0),
            (p::WOW, 0.0),
        ]);
        let mut quiet = vec![0.0f32; BLOCK];
        core.process(&mut quiet, &mut [], &clock());
        assert!((core.readout().bands[0] - 100.0).abs() < 0.5);
        core.set_param(p::TIME, 400.0);
        core.process(&mut quiet, &mut [], &clock());
        let stepped = core.readout().bands[0];
        assert!(stepped > 100.0, "the band did not move: {stepped} ms");
        assert!(stepped < 150.0, "the band jumped: {stepped} ms");
        for _ in 0..200 {
            core.process(&mut quiet, &mut [], &clock());
        }
        assert!((core.readout().bands[0] - 400.0).abs() < 2.0);
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::SYNC, 99.0);
        assert_eq!(core.settings().sync, Some(2.0));
        core.set_param(p::SYNC, 0.0);
        assert_eq!(core.settings().sync, None);
        core.set_param(p::FEEDBACK, 500.0);
        assert_eq!(core.settings().feedback, 1.0);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }
}
