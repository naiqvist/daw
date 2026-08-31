//! Ferric — the tape looper.
//!
//! A loop of tape that is ALWAYS recording, a read head that the musical
//! grid moves around on it, and the wear a real reel would have.
//!
//! # Why it is not a delay and not a live looper
//!
//! A tape DELAY reads at a fixed distance behind the write head and feeds
//! itself. This does not feed back at all: it reads wherever the grid puts
//! it, which may be right behind the write head (the tape passing through)
//! or four divisions back (the same bar again).
//!
//! A LIVE looper needs a person: arm, punch in, punch out, undo, and a
//! performance built around getting all four right. This needs nobody.
//! The tape is always running, so there is nothing to arm; the loop
//! boundaries are the transport's own grid, so there is nothing to line
//! up; and the read head is moved by a PATTERN, so what would have been
//! four button presses is a knob that is already in the right place.
//!
//! # The one idea
//!
//! The head is re-placed at the start of every DIVISION.
//!
//! That single rule is where the loop sync and the repitching come from,
//! and it is why they do not fight. Run the tape fast and a division's
//! worth of audio plays in less than a division — and then the head is put
//! back and the next one starts on the beat regardless. Run it slow and
//! the division is cut off early, on the beat. The pitch moves freely and
//! the rhythm cannot drift, because nothing is ever asked to stay in time;
//! it is simply placed in time, eight times a bar.
//!
//! A varispeed device without that rule has to choose between staying in
//! tune and staying in time. This one is not asked.
//!
//! # New wiring, not new arithmetic
//!
//! There is no kernel in this file, and that is the architecture working:
//! saturation is [`Waveshaper`], the head bump is an [`EqBand`], the top
//! comes off through a [`OnePole`], the wow is an [`Lfo`], the hiss is
//! [`WhiteNoise`] and the fractional read is [`interp::hermite4`]. Every
//! one of them already existed and was already tested.
//!
//! [`interp::hermite4`]: crate::dsp::interp::hermite4

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::filters::{BandShape, EqBand, OnePole};
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::ferric as p;

/// How much tape there is, in seconds. Eight beats at 60 BPM, which is
/// the slowest tempo any of the patterns can ask for a whole cycle of.
pub const TAPE_SECONDS: f32 = 8.0;
/// The block is walked in chunks so the scratch is a fixed size.
const CHUNK: usize = 128;
/// How far behind the write head the read head is kept at its closest.
/// Four samples, which is what the cubic read needs either side.
const LOOKBACK: f64 = 4.0;
/// Wow depth at full, in samples of read-position wander.
const WOW_SAMPLES: f32 = 26.0;
/// The wow's rate, in hertz. Slow — this is the reel being slightly out
/// of round, not vibrato.
const WOW_HZ: f32 = 0.7;
/// The head bump: the low lift every tape machine has.
const BUMP_HZ: f32 = 78.0;
const BUMP_Q: f32 = 0.7;
const BUMP_MAX_DB: f32 = 3.5;
/// Hiss at full age, as a linear amplitude. Audible under a fade and
/// nowhere else, which is where hiss belongs.
const HISS_AT_FULL: f32 = 0.0022;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FerricParams {
    pub speed_st: f32,
    pub division: f32,
    pub pattern: f32,
    pub groove: f32,
    pub drive: f32,
    pub wow: f32,
    pub age: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for FerricParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            speed_st: d(p::SPEED),
            division: d(p::DIVISION),
            pattern: d(p::PATTERN),
            groove: d(p::GROOVE),
            drive: d(p::DRIVE),
            wow: d(p::WOW),
            age: d(p::AGE),
            mix: d(p::MIX),
            out: d(p::OUT),
        }
    }
}

impl FerricParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::SPEED => self.speed_st = value,
            p::DIVISION => self.division = value,
            p::PATTERN => self.pattern = value,
            p::GROOVE => self.groove = value,
            p::DRIVE => self.drive = value,
            p::WOW => self.wow = value,
            p::AGE => self.age = value,
            p::MIX => self.mix = value,
            p::OUT => self.out = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::SPEED => Some(self.speed_st),
            p::DIVISION => Some(self.division),
            p::PATTERN => Some(self.pattern),
            p::GROOVE => Some(self.groove),
            p::DRIVE => Some(self.drive),
            p::WOW => Some(self.wow),
            p::AGE => Some(self.age),
            p::MIX => Some(self.mix),
            p::OUT => Some(self.out),
            _ => None,
        }
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE.iter() {
            if let Some(value) = self.get(def.id) {
                let fixed = if value.is_finite() {
                    value
                } else {
                    def.default
                };
                self.set(def.id, def.clamp(fixed));
            }
        }
    }

    /// Which grid the head is moved on, in beats.
    pub fn division_beats(&self) -> f32 {
        let i = (self.division.round().max(0.0) as usize).min(p::DIVISION_BEATS.len() - 1);
        p::DIVISION_BEATS.get(i).copied().unwrap_or(0.25)
    }

    /// Which pattern, as an index into [`p::PATTERNS`].
    pub fn pattern_index(&self) -> usize {
        (self.pattern.round().max(0.0) as usize).min(p::PATTERNS.len() - 1)
    }

    /// Tape speed as a ratio. A SPEED control on a tape machine moves
    /// time as well as pitch, which is the whole point.
    pub fn speed_ratio(&self) -> f32 {
        (self.speed_st / 12.0).exp2()
    }
}

/// The settings that cost something to rebuild.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    age: f32,
    drive: f32,
    wow: f32,
}

impl Resolved {
    fn of(params: &FerricParams) -> Self {
        Self {
            age: params.age,
            drive: params.drive,
            wow: params.wow,
        }
    }
}

/// One tape machine.
#[derive(Debug, Clone)]
pub struct FerricCore {
    params: FerricParams,
    prepared: Resolved,
    sample_rate: f32,
    /// The tape itself, one reel per channel.
    tape: [Vec<f32>; 2],
    tape_len: usize,
    /// The write head, in samples. Always moving.
    write_pos: usize,
    /// The read head, fractional because the tape may not be at speed.
    read_pos: f64,
    /// The grid step the head was last placed on.
    last_step: i64,
    sat: Waveshaper,
    loss: [OnePole; 2],
    bump: [EqBand; 2],
    wow_lfo: Lfo,
    hiss: WhiteNoise,
    wow_buf: Vec<f32>,
    hiss_buf: Vec<f32>,
    wet: [Vec<f32>; 2],
    /// Telemetry for the card: the step, how far back the head is, and
    /// how long a division actually lasts at the current tempo.
    step_now: f32,
    disp_now: f32,
    div_ms: f32,
    /// Peak level ARRIVING at the tape, in dBFS — a record meter, which
    /// is the one number that says whether the drive is doing anything.
    record_db: f32,
}

impl FerricCore {
    pub fn new(sample_rate: f32, params: &FerricParams) -> Self {
        let mut core = Self {
            params: *params,
            prepared: Resolved::of(params),
            sample_rate: 48_000.0,
            tape: [Vec::new(), Vec::new()],
            tape_len: 1,
            write_pos: 0,
            read_pos: 0.0,
            last_step: i64::MIN,
            sat: Waveshaper::new(),
            loss: [OnePole::new(); 2],
            bump: [EqBand::new(); 2],
            wow_lfo: Lfo::new(),
            hiss: WhiteNoise::new(),
            wow_buf: vec![0.0; CHUNK],
            hiss_buf: vec![0.0; CHUNK],
            wet: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            step_now: 0.0,
            disp_now: 0.0,
            div_ms: 0.0,
            record_db: crate::dsp::dynamics::FLOOR_DB,
        };
        core.params.sanitize();
        core.prepare(sample_rate);
        core
    }

    /// Green zone: the rate changed, so the reel and every coefficient is
    /// stale. THIS is where the tape is allocated — never in `process`.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;
        self.tape_len = ((TAPE_SECONDS * fs) as usize).max(CHUNK * 4);
        for reel in self.tape.iter_mut() {
            reel.clear();
            reel.resize(self.tape_len, 0.0);
        }
        self.wow_lfo.prepare(fs);
        self.wow_lfo.set_shape(LfoShape::Sine);
        self.wow_lfo.set_rate(WOW_HZ);
        self.hiss.seed(0x0FEC_1234);
        self.rebuild();
        self.reset();
    }

    /// Green zone: the coefficients the knobs decide.
    fn rebuild(&mut self) {
        let fs = self.sample_rate;
        let age = self.params.age.clamp(0.0, 1.0);
        let cutoff = p::top_hz(age);
        for f in self.loss.iter_mut() {
            f.prepare(fs, cutoff);
        }
        for f in self.bump.iter_mut() {
            f.prepare(fs, BUMP_HZ, BUMP_Q, BUMP_MAX_DB * age, BandShape::Bell);
        }
        // Tape compression: soft, and driven rather than clipped. The
        // knob is input gain into the curve AND the wet amount, so that
        // DRIVE at zero is the identity to the bit — a saturator that
        // coloured the tape at its lowest setting could not be measured
        // against nothing, which is the argument the sampler brief makes
        // and the promise `flint` keeps.
        let knob = self.params.drive.clamp(0.0, 1.0);
        self.sat
            .configure(ShapeMode::SoftClip, 1.0 + knob * 7.0, 0.0, knob);
        self.prepared = Resolved::of(&self.params);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> FerricParams {
        self.params
    }

    /// What the card draws: the step the head is on, and how many
    /// divisions back it was placed.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: self.record_db,
            reduction_db: 0.0,
            bands: [self.step_now, self.disp_now, self.div_ms],
        }
    }

    /// Green zone: wipe the reel.
    pub fn reset(&mut self) {
        for reel in self.tape.iter_mut() {
            for s in reel.iter_mut() {
                *s = 0.0;
            }
        }
        self.write_pos = 0;
        self.read_pos = 0.0;
        self.last_step = i64::MIN;
        self.wow_lfo.reset();
        for f in self.loss.iter_mut() {
            f.reset();
        }
        for f in self.bump.iter_mut() {
            f.reset();
        }
        self.step_now = 0.0;
        self.disp_now = 0.0;
        self.record_db = crate::dsp::dynamics::FLOOR_DB;
    }

    /// Nothing is anticipated, so nothing is delayed.
    pub fn latency(&self) -> usize {
        0
    }

    /// One reel, read at a fractional position, wrapped.
    fn tape_at(reel: &[f32], len: usize, pos: f64) -> f32 {
        if len == 0 {
            return 0.0;
        }
        let p = pos.rem_euclid(len as f64);
        let i = p.floor() as usize;
        let frac = (p - i as f64) as f32;
        let at = |k: isize| {
            let idx = (i as isize + k).rem_euclid(len as isize) as usize;
            reel.get(idx).copied().unwrap_or(0.0)
        };
        crate::dsp::interp::hermite4(at(-1), at(0), at(1), at(2), frac)
    }

    /// Red zone: run a stereo pair through the machine, in place.
    ///
    /// `beat` is the timeline beat of the first sample and `bps` how much
    /// a beat advances per sample — the two numbers the grid is made of.
    /// When the transport is not rolling there is no grid, so the head
    /// simply follows the write head and the device is a tape saturator.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], beat: f64, bps: f64, playing: bool) {
        if self.prepared != Resolved::of(&self.params) {
            self.rebuild();
        }
        let n = l.len().min(r.len());
        if n == 0 || self.tape_len == 0 {
            return;
        }

        let speed = f64::from(self.params.speed_ratio()).clamp(0.05, 8.0);
        let groove = f64::from(self.params.groove.clamp(0.0, 1.0));
        let pattern_index = self.params.pattern_index();
        let reverse = pattern_index == p::REVERSE_PATTERN;
        let steps = p::PATTERNS.get(pattern_index).copied().unwrap_or([0; 8]);
        let div_beats = f64::from(self.params.division_beats()).max(1e-6);
        let bps = if bps.is_finite() && bps > 0.0 {
            bps
        } else {
            0.0
        };
        let div_samples = if bps > 0.0 {
            (div_beats / bps).clamp(8.0, (self.tape_len / 3) as f64)
        } else {
            0.0
        };
        let grid = playing && bps > 0.0;

        let mix = self.params.mix.clamp(0.0, 1.0);
        let out = self.params.out;
        let age = self.params.age.clamp(0.0, 1.0);
        let wow_depth = self.params.wow.clamp(0.0, 1.0) * WOW_SAMPLES;
        let hiss_gain = age * age * HISS_AT_FULL;

        self.div_ms = if div_samples > 0.0 {
            (div_samples / f64::from(self.sample_rate) * 1000.0) as f32
        } else {
            0.0
        };
        let mut peak = 0.0f32;

        let mut done = 0usize;
        while done < n {
            let take = (n - done).min(CHUNK);
            let (Some(wow), Some(hiss)) =
                (self.wow_buf.get_mut(..take), self.hiss_buf.get_mut(..take))
            else {
                return;
            };
            self.wow_lfo.process(wow);
            self.hiss.process(hiss);

            for i in 0..take {
                let at = done + i;
                let (dry_l, dry_r) = (
                    l.get(at).copied().unwrap_or(0.0),
                    r.get(at).copied().unwrap_or(0.0),
                );
                let dry_l = if dry_l.is_finite() { dry_l } else { 0.0 };
                let dry_r = if dry_r.is_finite() { dry_r } else { 0.0 };
                peak = peak.max(dry_l.abs()).max(dry_r.abs());

                // --- RECORD. Saturation happens on the way to the tape,
                // where it happens on a real machine — so a loop that is
                // played back four times was driven once, not four times.
                let w = self.write_pos % self.tape_len;
                if let Some(slot) = self.tape[0].get_mut(w) {
                    *slot = self.sat.shape(dry_l);
                }
                if let Some(slot) = self.tape[1].get_mut(w) {
                    *slot = self.sat.shape(dry_r);
                }
                self.write_pos = self.write_pos.wrapping_add(1);

                // --- PLACE. The one rule: at every division boundary the
                // head is put where the pattern says, and between them it
                // simply runs.
                if grid {
                    let beat_now = beat + (at as f64) * bps;
                    let step = (beat_now / div_beats).floor() as i64;
                    if step != self.last_step {
                        self.last_step = step;
                        let idx = step.rem_euclid(p::PATTERN_STEPS as i64) as usize;
                        let back = f64::from(steps.get(idx).copied().unwrap_or(0)) * groove;
                        // A fast tape has to REACH FURTHER BACK to
                        // arrive on time. Over one division the head
                        // covers `speed` divisions of tape, so at double
                        // speed it must start a division earlier or it
                        // overtakes the write head and reads tape that
                        // has not been recorded yet. Placed this way the
                        // head always finishes the step live, whatever
                        // the speed — which is the same rule as the
                        // grid's, applied to distance instead of time.
                        // Rounded UP to a whole division, so the head
                        // always starts on a boundary of the recorded
                        // material rather than half way into a beat.
                        // Fractional reach put a 1.5x tape down between
                        // two recorded divisions, and everything it
                        // replayed arrived off the grid by the remainder.
                        let reach = (speed - 1.0).max(0.0).ceil();
                        let offset = (back + reach) * div_samples;
                        let room = (self.tape_len as f64 - LOOKBACK - 8.0).max(0.0);
                        self.step_now = idx as f32;
                        self.disp_now = back as f32;
                        self.read_pos = self.write_pos as f64 - LOOKBACK - offset.min(room);
                    }
                } else {
                    self.read_pos = self.write_pos as f64 - LOOKBACK;
                    self.step_now = 0.0;
                    self.disp_now = 0.0;
                }

                let wander = f64::from(wow.get(i).copied().unwrap_or(0.0) * wow_depth);
                let pos = self.read_pos + wander;
                let wet_l = Self::tape_at(&self.tape[0], self.tape_len, pos);
                let wet_r = Self::tape_at(&self.tape[1], self.tape_len, pos);
                if let Some(slot) = self.wet[0].get_mut(i) {
                    *slot = wet_l;
                }
                if let Some(slot) = self.wet[1].get_mut(i) {
                    *slot = wet_r;
                }

                // Backwards is the only place the head runs the other
                // way, and it is a pattern rather than a switch because
                // it is a rhythmic idea, not a mode.
                self.read_pos += if reverse { -speed } else { speed };
            }

            // --- PLAY. The wear, applied to what came off the tape.
            for ch in 0..2 {
                let Some(band) = self.wet.get_mut(ch) else {
                    continue;
                };
                let Some(slice) = band.get_mut(..take) else {
                    continue;
                };
                if let Some(f) = self.loss.get_mut(ch) {
                    f.process_lowpass(slice);
                }
                if let Some(f) = self.bump.get_mut(ch) {
                    f.process(slice);
                }
            }

            for i in 0..take {
                let at = done + i;
                let noise = hiss.get(i).copied().unwrap_or(0.0) * hiss_gain;
                for (ch, io) in [(0usize, &mut *l), (1, &mut *r)] {
                    let dry = io.get(at).copied().unwrap_or(0.0);
                    let dry = if dry.is_finite() { dry } else { 0.0 };
                    let wet = self
                        .wet
                        .get(ch)
                        .and_then(|b| b.get(i))
                        .copied()
                        .unwrap_or(0.0)
                        + noise;
                    let y = (dry + (wet - dry) * mix) * out;
                    if let Some(slot) = io.get_mut(at) {
                        *slot = if y.is_finite() { y } else { 0.0 };
                    }
                }
            }
            done += take;
        }

        self.record_db =
            crate::dsp::arith::gain_to_db(peak.max(1e-6)).max(crate::dsp::dynamics::FLOOR_DB);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;
    /// 120 BPM, in beats per sample.
    const BPS: f64 = 120.0 / 60.0 / 48_000.0;
    /// A sixteenth at 120 BPM.
    const DIV_SAMPLES: usize = 6_000;

    fn core(edit: impl Fn(&mut FerricParams)) -> FerricCore {
        let mut params = FerricParams::default();
        // A transparent machine unless a test asks otherwise: the groove
        // is what most of these are about, and tape colour on top of it
        // would only make the assertions vaguer.
        params.drive = 0.0;
        params.wow = 0.0;
        params.age = 0.0;
        params.mix = 1.0;
        edit(&mut params);
        FerricCore::new(FS, &params)
    }

    /// Run a signal through in blocks, advancing the beat as the
    /// transport would.
    fn run(c: &mut FerricCore, signal: &[f32], block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(signal.len());
        let mut at = 0usize;
        while at < signal.len() {
            let end = (at + block).min(signal.len());
            let mut l = signal[at..end].to_vec();
            let mut r = l.clone();
            c.process(&mut l, &mut r, at as f64 * BPS, BPS, true);
            out.extend_from_slice(&l);
            at = end;
        }
        out
    }

    /// A click at the very start of the first division, then nothing.
    fn click_then_silence(n: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; n];
        for (i, slot) in v.iter_mut().enumerate().take(64) {
            // A short burst rather than one sample, so a cubic read
            // cannot miss it between two positions.
            *slot = if i < 32 { 0.9 } else { 0.0 };
        }
        v
    }

    /// A click at the start of EVERY division, so wherever the head is
    /// placed on the grid there is something recorded there.
    ///
    /// The single-click fixture cannot test a fast tape: a fast head
    /// reaches further back to arrive on time, and at step 1 it lands
    /// before the one click was ever recorded. That is the device
    /// working, not failing, and the fixture had to say so.
    fn click_train(n: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; n];
        for (i, slot) in v.iter_mut().enumerate() {
            let into = i % DIV_SAMPLES;
            *slot = if into < 32 { 0.9 } else { 0.0 };
        }
        v
    }

    /// The loudest sample in a window.
    fn peak_in(x: &[f32], from: usize, len: usize) -> f32 {
        x.get(from..(from + len).min(x.len()))
            .unwrap_or(&[])
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// THE ONE RULE: the head is re-placed at every division, so a
    /// stutter replays the same tape on each of the next three steps.
    ///
    /// One click at the top of step 0 and silence after it. With the
    /// stutter pattern the head is put one, two and three divisions back
    /// on steps 1, 2 and 3 — so the click has to come round again at the
    /// start of each of them, from a tape that has nothing else on it.
    #[test]
    fn a_stutter_replays_the_same_tape_on_every_step() {
        let mut c = core(|p| {
            p.pattern = 1.0; // stutter
            p.groove = 1.0;
            p.division = 3.0; // 1/16
        });
        let signal = click_then_silence(DIV_SAMPLES * 5);
        let out = run(&mut c, &signal, 256);

        for step in 0..4usize {
            let heard = peak_in(&out, step * DIV_SAMPLES, 256);
            assert!(
                heard > 0.4,
                "step {step} should have replayed the click, peak was {heard}"
            );
        }
        // ...and the space between the repeats stays empty: the head is
        // playing silent tape, not smearing the click across the bar.
        let between = peak_in(&out, DIV_SAMPLES / 2, DIV_SAMPLES / 4);
        assert!(
            between < 0.05,
            "the gap between repeats was not quiet: {between}"
        );
    }

    /// THE POINT OF THE RULE: repitching cannot move the grid.
    ///
    /// At double speed a division's tape plays in half a division — and
    /// the head is put back anyway, so the repeats still land on the
    /// beat. A varispeed looper without the re-placement drifts by half
    /// a division per step and is unusable by the second bar.
    #[test]
    fn speed_repitches_without_moving_the_grid() {
        for semitones in [-12.0f32, 0.0, 7.0, 12.0] {
            let mut c = core(|p| {
                p.pattern = 1.0; // stutter
                p.groove = 1.0;
                p.division = 3.0;
                p.speed_st = semitones;
            });
            let signal = click_train(DIV_SAMPLES * 8);
            let out = run(&mut c, &signal, 256);
            // Past the warm-up, so the head has grid-aligned tape to
            // reach back into whatever the speed.
            for step in 4..7usize {
                let heard = peak_in(&out, step * DIV_SAMPLES, 512);
                assert!(
                    heard > 0.3,
                    "at {semitones} st, step {step} did not land on the grid \
                     (peak {heard})"
                );
            }
        }
    }

    /// Speed is a TAPE speed: it moves the pitch.
    #[test]
    fn speed_moves_the_pitch() {
        let tone = |hz: f32, n: usize| -> Vec<f32> {
            (0..n)
                .map(|i| 0.6 * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
                .collect()
        };
        let amp = |x: &[f32], hz: f32| -> f64 {
            let w = core::f64::consts::TAU * (hz / FS) as f64;
            let c = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for v in x {
                let s0 = *v as f64 + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / x.len() as f64
        };
        // RUN, so nothing but the speed is happening.
        let mut c = core(|p| {
            p.pattern = 0.0;
            p.groove = 0.0;
            p.speed_st = 12.0;
        });
        let signal = tone(300.0, DIV_SAMPLES * 4);
        let out = run(&mut c, &signal, 256);
        let settled = out.get(DIV_SAMPLES..).unwrap();
        let doubled = amp(settled, 600.0);
        let original = amp(settled, 300.0);
        assert!(
            doubled > original * 2.0,
            "an octave up should read at 600 Hz: 600 {doubled:.5}, 300 {original:.5}"
        );
    }

    /// GROOVE at zero is the tape passing through: the head never leaves
    /// the write head, so what comes out is what went in.
    #[test]
    fn groove_at_zero_passes_the_tape_through() {
        let mut c = core(|p| {
            p.pattern = 5.0; // drag, which HAS displacement to suppress
            p.groove = 0.0;
        });
        let signal: Vec<f32> = (0..DIV_SAMPLES * 3)
            .map(|i| 0.5 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
            .collect();
        let out = run(&mut c, &signal, 256);
        // Compare past the warm-up, allowing the few samples of lookback.
        let from = DIV_SAMPLES;
        let mut worst = 0.0f32;
        for i in from..signal.len() - 8 {
            let want = signal.get(i.saturating_sub(3)).copied().unwrap_or(0.0);
            let got = out.get(i).copied().unwrap_or(0.0);
            worst = worst.max((got - want).abs());
        }
        assert!(worst < 0.06, "the pass-through drifted by {worst}");
    }

    /// DRIVE at zero writes the input to the tape unchanged — the exact
    /// bypass a colour stage has to have to be measurable at all.
    #[test]
    fn drive_at_zero_is_the_identity_on_the_way_to_the_tape() {
        let c = core(|p| p.drive = 0.0);
        for x in [-1.0f32, -0.4, 0.0, 0.25, 0.9, 1.0] {
            assert_eq!(c.sat.shape(x), x, "drive 0 changed {x}");
        }
        // Driven, it COMPRESSES: a loud sample gains less than a quiet
        // one. It does not get smaller — eight times into a soft clip
        // pushes 0.9 up towards the rail, and asserting otherwise was
        // asserting that a saturator is a limiter.
        let hot = core(|p| p.drive = 1.0);
        let quiet = hot.sat.shape(0.1) / 0.1;
        let loud = hot.sat.shape(0.9) / 0.9;
        assert!(
            loud < quiet * 0.5,
            "drive 1 should compress: quiet gained {quiet:.3}, loud {loud:.3}"
        );
    }

    /// Nothing rolling means no grid: the machine is a tape saturator and
    /// the head simply follows.
    #[test]
    fn a_stopped_transport_leaves_the_head_alone() {
        let mut c = core(|p| {
            p.pattern = 1.0;
            p.groove = 1.0;
        });
        let signal = click_then_silence(DIV_SAMPLES * 3);
        let mut at = 0usize;
        let mut out = Vec::new();
        while at < signal.len() {
            let end = (at + 256).min(signal.len());
            let mut l = signal[at..end].to_vec();
            let mut r = l.clone();
            c.process(&mut l, &mut r, 0.0, BPS, false);
            out.extend_from_slice(&l);
            at = end;
        }
        // The click passes once and never comes back.
        assert!(
            peak_in(&out, 0, 256) > 0.4,
            "the click did not pass through"
        );
        for step in 1..3usize {
            let heard = peak_in(&out, step * DIV_SAMPLES, 512);
            assert!(heard < 0.05, "a stopped transport repeated at step {step}");
        }
    }

    #[test]
    fn silence_stays_silent_and_nonsense_stays_finite() {
        let mut c = core(|p| {
            p.drive = 1.0;
            p.age = 1.0;
            p.wow = 1.0;
        });
        let mut l = vec![0.0f32; 4_096];
        let mut r = l.clone();
        c.process(&mut l, &mut r, 0.0, BPS, true);
        // Hiss is the only thing a blank tape may produce, and it must be
        // far under anything audible.
        let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.02, "a blank tape produced {peak}");

        let mut c = core(|_| {});
        let mut l = vec![f32::NAN, f32::INFINITY, -1e30, 0.5, 0.0, -0.5];
        let mut r = l.clone();
        c.process(&mut l, &mut r, 0.0, BPS, true);
        assert!(
            l.iter().chain(r.iter()).all(|s| s.is_finite()),
            "nonsense escaped: {l:?}"
        );
    }

    #[test]
    fn odd_block_lengths_and_nonsense_tempi_are_accepted() {
        let mut c = core(|_| {});
        for len in [0usize, 1, 3, 17, 129, 1_000] {
            let mut l = vec![0.3f32; len];
            let mut r = l.clone();
            c.process(&mut l, &mut r, 4.0, BPS, true);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        // A stopped or nonsensical clock must not divide by zero.
        for bps in [0.0f64, -1.0, f64::NAN, f64::INFINITY] {
            let mut l = vec![0.3f32; 256];
            let mut r = l.clone();
            c.process(&mut l, &mut r, 0.0, bps, true);
            assert!(l.iter().all(|s| s.is_finite()), "bps {bps}");
        }
        // A mismatched pair truncates rather than panicking.
        let mut l = vec![0.2f32; 64];
        let mut r = vec![0.2f32; 8];
        c.process(&mut l, &mut r, 0.0, BPS, true);
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut c = core(|p| {
            p.pattern = 1.0;
            p.groove = 1.0;
            p.drive = 0.5;
            p.age = 0.5;
            p.wow = 0.5;
        });
        let mut l = vec![0.4f32; 256];
        let mut r = l.clone();
        c.process(&mut l, &mut r, 0.0, BPS, true);
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..40 {
                c.process(&mut l, &mut r, i as f64 * 0.1, BPS, true);
            }
        });
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = FerricParams::default();
        for def in p::TABLE.iter() {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "row {}", def.name);
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), Some(def.min), "row {}", def.name);
            params.set(def.id, def.max * 100.0 + 1.0);
            let got = params.get(def.id).unwrap();
            assert!(got <= def.max && got >= def.min, "row {} = {got}", def.name);
        }
        assert_eq!(params.get(9_999), None);

        let mut junk = FerricParams {
            speed_st: f32::NAN,
            division: 99.0,
            pattern: -4.0,
            groove: 40.0,
            drive: f32::INFINITY,
            wow: -2.0,
            age: f32::NAN,
            mix: 12.0,
            out: -1.0,
        };
        junk.sanitize();
        for def in p::TABLE.iter() {
            let got = junk.get(def.id).unwrap();
            assert!(
                got.is_finite() && got >= def.min && got <= def.max,
                "row {} survived sanitize as {got}",
                def.name
            );
        }
        // The two enumerations always land on something real.
        assert!(p::DIVISION_BEATS.contains(&junk.division_beats()));
        assert!(junk.pattern_index() < p::PATTERNS.len());
    }
}
