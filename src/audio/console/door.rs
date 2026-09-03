//! DOOR: the gate and expander, and the chopper.
//!
//! Not a model of a classic — a gate is a switch with timing, and the
//! classics are remembered for their controls, not their colour — but
//! the best of what they had, in one: a key filter (high-pass and
//! low-pass on the sidechain, so the door listens to the drum it is
//! on), a HOLD, a RANGE so it can duck instead of slam, a RATIO so it is
//! an expander at 2:1 and a gate at 20:1, HYSTERESIS so it never
//! chatters at the threshold, a fixed LOOKAHEAD of two milliseconds so
//! a transient is never clipped by its own opening, and a
//! program-dependent RELEASE — a short burst lets go faster than a
//! sustained note.
//!
//! And the thing no desk had: a RHYTHM mode, where the door opens on
//! the beat grid instead of on a threshold — a trance gate, a chopper —
//! with a DIVISION and a DUTY, shaped by the same ATTACK and RELEASE
//! and cut by the same RANGE. It reads the clock the node is handed
//! and opens when the transport stands still.
//!
//! The floor: RANGE at zero is a door that never shuts, and the section
//! is a wire delayed by the lookahead, which the graph pays back.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::filters::Cascade;
use crate::dsp::ramps::one_pole_coeff;
use crate::params::console::door as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub rhythm: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
    pub range_db: f32,
    pub key_hp: f32,
    pub key_lp: f32,
    pub hysteresis_db: f32,
    pub division: usize,
    pub duty: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Door.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            rhythm: clamp(p::MODE).round() as u32 == p::MODE_RHYTHM,
            threshold_db: clamp(p::THRESHOLD),
            ratio: clamp(p::RATIO),
            attack_ms: clamp(p::ATTACK),
            hold_ms: clamp(p::HOLD),
            release_ms: clamp(p::RELEASE),
            range_db: clamp(p::RANGE),
            key_hp: clamp(p::KEY_HP),
            key_lp: clamp(p::KEY_LP),
            hysteresis_db: clamp(p::HYSTERESIS),
            division: (clamp(p::DIVISION).round().max(0.0) as usize)
                .min(p::DIVISION_BEATS.len() - 1),
            duty: clamp(p::DUTY) / 100.0,
        }
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

/// Whether the door is open, opening, held, or closing — for the
/// hysteresis and the hold, and for the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Shut,
    Open,
    Holding,
}

pub struct DoorCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    /// The lookahead: the audio, delayed; the detector reads the
    /// undelayed input so the gain leads the sound.
    lines: [DelayLine; 2],
    line_bufs: [Vec<f32>; 2],
    lookahead: usize,
    /// The key: the mono sum, filtered.
    key_hp: Cascade,
    key_lp: Cascade,
    key: Vec<f32>,
    /// The detector's envelope, linear, and its coefficients.
    env: f32,
    detect_attack: f32,
    detect_release: f32,
    /// The door's state, how long it has been open (samples), and how
    /// much hold is left (samples).
    state: State,
    open_for: u32,
    hold_left: u32,
    /// The gain, linear, smoothed toward its target with the attack and
    /// release coefficients.
    gain: f32,
    attack: f32,
    release: f32,
    /// What the card reads: the gain in dB this block, at its most
    /// reduced, and the level out.
    reduction_db: f32,
    level_db: f32,
}

impl DoorCore {
    /// Green zone: the lines, the key filters, the coefficients.
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let settings = Settings::of(params);
        let lookahead = (sample_rate * p::LOOKAHEAD_MS / 1000.0).round() as usize;
        let mut core = Self {
            params: params.dense(),
            settings,
            sample_rate,
            lines: [DelayLine::new(), DelayLine::new()],
            line_bufs: [
                vec![0.0; buffer_len(lookahead)],
                vec![0.0; buffer_len(lookahead)],
            ],
            lookahead,
            key_hp: Cascade::new(),
            key_lp: Cascade::new(),
            key: Vec::new(),
            env: 0.0,
            detect_attack: one_pole_coeff(1000.0 / (p::DETECT_ATTACK_MS * sample_rate)),
            detect_release: one_pole_coeff(1000.0 / (p::DETECT_RELEASE_MS * sample_rate)),
            state: State::Open,
            open_for: 0,
            hold_left: 0,
            gain: 1.0,
            attack: 1.0,
            release: 1.0,
            reduction_db: 0.0,
            level_db: -120.0,
        };
        for line in &mut core.lines {
            line.prepare(lookahead);
            line.set_delay(lookahead as f32);
        }
        core.key = vec![0.0; _block.max(1)];
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        let fs = self.sample_rate;
        self.key_hp.prepare(
            fs,
            s.key_hp,
            core::f32::consts::FRAC_1_SQRT_2,
            p::KEY_ORDER,
            true,
        );
        self.key_lp.prepare(
            fs,
            s.key_lp,
            core::f32::consts::FRAC_1_SQRT_2,
            p::KEY_ORDER,
            false,
        );
        self.attack = one_pole_coeff(1000.0 / (s.attack_ms * fs));
        self.release = one_pole_coeff(1000.0 / (s.release_ms * fs));
    }

    /// The target gain for a key level, by the threshold, the ratio and
    /// the range: unity above the threshold, falling `ratio − 1` dB per
    /// dB below it, never further than the range.
    fn expander_gain(&self, key_db: f32, open: bool) -> f32 {
        let s = self.settings;
        if open {
            return 1.0;
        }
        let below = (s.threshold_db - key_db).max(0.0);
        let reduction = (below * (s.ratio - 1.0)).min(s.range_db);
        db_to_gain(-reduction)
    }

    /// The door's decision for one sample of key level, with hysteresis
    /// and hold: opens at the threshold, shuts only below the threshold
    /// less the hysteresis, and not before the hold has run.
    fn decide(&mut self, key_db: f32) -> bool {
        let s = self.settings;
        let hold = (s.hold_ms * self.sample_rate / 1000.0) as u32;
        match self.state {
            State::Shut => {
                if key_db >= s.threshold_db {
                    self.state = State::Open;
                    self.open_for = 0;
                    true
                } else {
                    false
                }
            }
            State::Open => {
                self.open_for = self.open_for.saturating_add(1);
                if key_db < s.threshold_db - s.hysteresis_db {
                    self.state = State::Holding;
                    self.hold_left = hold;
                }
                true
            }
            State::Holding => {
                self.open_for = self.open_for.saturating_add(1);
                if key_db >= s.threshold_db {
                    self.state = State::Open;
                    true
                } else if self.hold_left == 0 {
                    self.state = State::Shut;
                    false
                } else {
                    self.hold_left -= 1;
                    true
                }
            }
        }
    }

    /// The release coefficient for how long the door was open: quicker
    /// after a burst, the set time after a sustained note.
    fn release_for(&self) -> f32 {
        let s = self.settings;
        let settle = p::RELEASE_SETTLE_MS * self.sample_rate / 1000.0;
        let share = (self.open_for as f32 / settle).clamp(p::RELEASE_QUICK, 1.0);
        one_pole_coeff(1000.0 / (s.release_ms * share * self.sample_rate))
    }
}

impl SectionCore for DoorCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for (line, buf) in self.lines.iter_mut().zip(self.line_bufs.iter_mut()) {
            line.reset();
            buf.fill(0.0);
        }
        self.key_hp.reset();
        self.key_lp.reset();
        self.env = 0.0;
        self.state = State::Open;
        self.open_for = 0;
        self.hold_left = 0;
        self.gain = 1.0;
        self.reduction_db = 0.0;
        self.level_db = -120.0;
    }

    fn latency(&self) -> usize {
        self.lookahead
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.key.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;

        // The key: the undelayed input, summed and filtered.
        {
            let key = &mut self.key[..n];
            for (i, k) in key.iter_mut().enumerate() {
                *k = if stereo { (l[i] + r[i]) * 0.5 } else { l[i] };
            }
            if s.key_hp > 20.5 {
                self.key_hp.process(key);
            }
            if s.key_lp < 19_999.0 {
                self.key_lp.process(key);
            }
        }

        // The lookahead: the audio delayed, so the gain leads it.
        let (line_l, line_r) = self.lines.split_at_mut(1);
        let (buf_l, buf_r) = self.line_bufs.split_at_mut(1);
        line_l[0].process_exact(l, &mut buf_l[0]);
        if stereo {
            line_r[0].process_exact(&mut r[..n], &mut buf_r[0]);
        }

        // The floor: a door that never shuts. The delay still runs so
        // the latency the graph pays back never changes.
        if s.range_db <= 0.0 {
            self.gain = 1.0;
            self.reduction_db = 0.0;
            self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
            return;
        }

        let mut most_reduced = 0.0f32;
        let release_now = self.release_for();
        for i in 0..n {
            let target = if s.rhythm {
                // The chopper: open for the duty's share of each step
                // of the division, by the clock; open when it stands.
                if clock.playing {
                    let beat = clock.beat + i as f64 * clock.beats_per_sample;
                    let step = f64::from(p::DIVISION_BEATS[s.division]);
                    let phase = (beat / step).rem_euclid(1.0) as f32;
                    self.open_for = self.open_for.saturating_add(1);
                    if phase < s.duty {
                        1.0
                    } else {
                        db_to_gain(-s.range_db)
                    }
                } else {
                    1.0
                }
            } else {
                let x = self.key[i].abs();
                let coeff = if x > self.env {
                    self.detect_attack
                } else {
                    self.detect_release
                };
                self.env += (x - self.env) * coeff;
                let key_db = gain_to_db(self.env);
                let open = self.decide(key_db);
                self.expander_gain(key_db, open)
            };
            let coeff = if target > self.gain {
                self.attack
            } else {
                release_now
            };
            self.gain += (target - self.gain) * coeff;
            if (self.gain - target).abs() < 1e-6 {
                self.gain = target;
            }
            l[i] *= self.gain;
            if stereo {
                r[i] *= self.gain;
            }
            most_reduced = most_reduced.min(gain_to_db(self.gain));
        }
        self.reduction_db = most_reduced;
        self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.reduction_db,
            // The door's state for the card: open, or shut.
            bands: [
                if self.state == State::Shut { 0.0 } else { 1.0 },
                self.gain,
                gain_to_db(self.env),
            ],
        }
    }
}

fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= 1e-6 {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock_at(beat: f64, playing: bool) -> Clock {
        Clock {
            playing,
            beat,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> DoorCore {
        let mut params = SectionParams::of(SectionKind::Door);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        DoorCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    /// Run with the clock rolling from beat zero.
    fn run(core: &mut DoorCore, l: &[f32], playing: bool) -> Vec<f32> {
        let mut out = l.to_vec();
        let bps = 120.0 / 60.0 / f64::from(FS);
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            let clock = Clock {
                playing,
                beat: start as f64 * bps,
                beats_per_sample: bps,
            };
            core.process(&mut out[start..end], &mut [], &clock);
        }
        out
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn db(amp: f32) -> f32 {
        db_to_gain(amp)
    }

    /// RANGE at zero: a wire, delayed by the lookahead the graph pays
    /// back, sample for sample.
    #[test]
    fn the_floor_is_a_wire_behind_the_lookahead() {
        let mut core = core_with(&[(p::RANGE, 0.0)]);
        let ahead = core.latency();
        assert_eq!(ahead, 96);
        let l = sine(440.0, 0.5, 2048);
        let out = run(&mut core, &l, true);
        for i in ahead..l.len() {
            assert!((out[i] - l[i - ahead]).abs() < 1e-6, "sample {i}");
        }
        assert!(out[..ahead].iter().all(|s| *s == 0.0));
    }

    /// Loud passes untouched; quiet is shut by the range.
    #[test]
    fn loud_passes_and_quiet_is_shut_by_the_range() {
        let n = FS as usize / 2;
        let loud = sine(1_000.0, db(-10.0), n);
        let mut core = core_with(&[(p::THRESHOLD, -30.0), (p::RANGE, 40.0)]);
        let out = run(&mut core, &loud, true);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&loud[n / 2..])).log10();
        assert!(change.abs() < 0.1, "loud changed by {change} dB");
        assert_eq!(core.readout().bands[0], 1.0, "the door is not open");

        let quiet = sine(1_000.0, db(-50.0), n);
        let mut core = core_with(&[(p::THRESHOLD, -30.0), (p::RANGE, 40.0)]);
        let out = run(&mut core, &quiet, true);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&quiet[n / 2..])).log10();
        assert!((change + 40.0).abs() < 1.0, "quiet changed by {change} dB");
        assert_eq!(core.readout().bands[0], 0.0, "the door is not shut");
        assert!(core.readout().reduction_db < -39.0);
    }

    /// An expander at 2:1 takes a quiet signal down by as much as it is
    /// under the threshold, not to the floor.
    #[test]
    fn an_expander_takes_a_little() {
        let n = FS as usize / 2;
        let quiet = sine(1_000.0, db(-40.0), n);
        let mut core = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::RANGE, 60.0)]);
        let out = run(&mut core, &quiet, true);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&quiet[n / 2..])).log10();
        assert!((change + 10.0).abs() < 1.0, "expanded by {change} dB");
    }

    /// The lookahead: the door is already open when a burst arrives
    /// out of silence, so its first cycle is whole.
    #[test]
    fn the_door_opens_before_the_transient() {
        let n = 4800;
        let mut l = vec![0.0f32; n];
        let onset = 2400;
        for (i, s) in l.iter_mut().enumerate().skip(onset) {
            *s = 0.5 * (2.0 * core::f32::consts::PI * 2_000.0 * (i - onset) as f32 / FS).sin();
        }
        let mut core = core_with(&[(p::THRESHOLD, -30.0), (p::RANGE, 60.0), (p::ATTACK, 0.05)]);
        let out = run(&mut core, &l, true);
        let ahead = core.latency();
        // The first cycle of the burst, where it lands after the delay.
        let first = onset + ahead..onset + ahead + 24;
        let expected = &l[onset..onset + 24];
        let got = &out[first];
        let peak_in = expected.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let peak_out = got.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak_out > peak_in * 0.9,
            "the onset was clipped: {peak_out} of {peak_in}"
        );
    }

    /// Hysteresis: a signal dipping a dB under the threshold does not
    /// shut the door; three dB under with the hysteresis at two does.
    #[test]
    fn hysteresis_keeps_the_door_from_chattering() {
        let n = FS as usize / 2;
        // Every 10 ms, dip under the threshold for 10 ms: a dB under
        // in the first signal, eight under in the second.
        let dipped = |dip_db: f32| -> Vec<f32> {
            let mut l = sine(1_000.0, db(-29.0), n);
            for (i, s) in l.iter_mut().enumerate() {
                if (i / 480) % 2 == 1 {
                    *s *= db(dip_db);
                }
            }
            l
        };
        // How much of the dips' second halves survives, in dB, once
        // settled, read behind the lookahead.
        let dips_kept = |l: &[f32], out: &[f32], ahead: usize| -> f32 {
            let (mut kept, mut had) = (0.0f32, 0.0f32);
            for i in n / 2..n - ahead {
                if (i / 480) % 2 == 1 && i % 480 >= 240 {
                    kept += out[i + ahead] * out[i + ahead];
                    had += l[i] * l[i];
                }
            }
            10.0 * (kept / had.max(1e-12)).log10()
        };
        let quick = [
            (p::THRESHOLD, -30.0),
            (p::HYSTERESIS, 3.0),
            (p::HOLD, 0.0),
            (p::RANGE, 40.0),
            (p::RELEASE, 5.0),
        ];
        let shallow = dipped(-2.0);
        let mut core = core_with(&quick);
        let out = run(&mut core, &shallow, true);
        let kept = dips_kept(&shallow, &out, core.latency());
        assert!(
            kept > -0.5,
            "the door chattered on a shallow dip: {kept} dB"
        );

        let deep = dipped(-8.0);
        let mut core = core_with(&quick);
        let out = run(&mut core, &deep, true);
        let kept = dips_kept(&deep, &out, core.latency());
        assert!(kept < -5.0, "a real dip did not shut the door: {kept} dB");
    }

    /// The key filter: with the key high-passed at a kilohertz, a loud
    /// low note does not open the door; a high one does.
    #[test]
    fn the_key_filter_decides_who_opens_the_door() {
        let n = FS as usize / 2;
        let low = sine(60.0, db(-10.0), n);
        let mut core = core_with(&[
            (p::THRESHOLD, -30.0),
            (p::KEY_HP, 1_000.0),
            (p::RANGE, 40.0),
        ]);
        let out = run(&mut core, &low, true);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&low[n / 2..])).log10();
        assert!(change < -30.0, "the low note opened the door: {change} dB");

        let high = sine(3_000.0, db(-10.0), n);
        let mut core = core_with(&[
            (p::THRESHOLD, -30.0),
            (p::KEY_HP, 1_000.0),
            (p::RANGE, 40.0),
        ]);
        let out = run(&mut core, &high, true);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&high[n / 2..])).log10();
        assert!(
            change > -0.5,
            "the high note did not open the door: {change} dB"
        );
    }

    /// The chopper: at 120 bpm on sixteenths with half a duty, each
    /// sixteenth's first half passes and its second half is cut by the
    /// range; with the transport stopped, everything passes.
    #[test]
    fn rhythm_mode_chops_on_the_grid() {
        let n = FS as usize; // two beats at 120 bpm
        let l = sine(1_000.0, 0.5, n);
        let edits = [
            (p::MODE, 1.0),
            (p::DIVISION, 2.0),
            (p::DUTY, 50.0),
            (p::RANGE, 40.0),
            (p::ATTACK, 0.05),
            (p::RELEASE, 5.0),
        ];
        let mut core = core_with(&edits);
        let out = run(&mut core, &l, true);
        let ahead = core.latency();
        let step = FS as usize / 8; // a sixteenth at 120 bpm: 6000 samples
        // Read the steady middle of each half, past the transitions.
        let open = &out[ahead + step * 2 + 600..ahead + step * 2 + step / 2 - 100];
        // The shut half, past its release: its last quarter.
        let shut = &out[ahead + step * 2 + step * 3 / 4..ahead + step * 3 - 100];
        let ratio = 20.0 * (rms(shut) / rms(open)).log10();
        assert!((ratio + 40.0).abs() < 1.5, "the chop is {ratio} dB");
        assert!(
            (rms(open) - 0.5 / 2f32.sqrt()).abs() < 0.02,
            "the open half is not whole"
        );

        let mut still = core_with(&edits);
        let out = run(&mut still, &l, false);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!(
            change.abs() < 0.1,
            "a stopped transport chopped: {change} dB"
        );
    }

    /// Splitting the work into blocks of any size gives the same signal.
    #[test]
    fn split_blocks_are_equivalent() {
        let mut l = sine(1_000.0, db(-20.0), 2000);
        for (i, s) in l.iter_mut().enumerate() {
            if (i / 400) % 2 == 1 {
                *s *= 0.02;
            }
        }
        let edits = [(p::THRESHOLD, -30.0), (p::RANGE, 40.0), (p::HOLD, 5.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l, true);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let bps = 120.0 / 60.0 / f64::from(FS);
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244, 256, 256, 256, 232] {
            let end = (at + len).min(b.len());
            let clock = Clock {
                playing: true,
                beat: at as f64 * bps,
                beats_per_sample: bps,
            };
            pieces.process(&mut b[at..end], &mut [], &clock);
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::RATIO, 500.0);
        assert_eq!(core.settings().ratio, 20.0);
        core.set_param(p::DIVISION, 99.0);
        assert_eq!(core.settings().division, 5);
        core.set_param(p::DUTY, 0.0);
        assert_eq!(core.settings().duty, 0.05);
        core.set_param(99, 1.0);
    }
}
