//! SMEAR: allpass dispersion.
//!
//! A run of allpass sections at one corner, up to thirty-two of them.
//! An allpass passes every frequency at full level and turns its phase,
//! and a run of them turns the phase of what is near the corner much
//! further than the rest — so a transient's low frequencies come out a
//! few milliseconds after its high ones. A snare becomes a laser, a
//! kick becomes a boing, and a whole mix becomes a smear. Nothing is
//! filtered: the magnitude is untouched to the sample, only the timing
//! of the parts moves.
//!
//! AMOUNT is how many sections run, CENTRE where they turn. At no
//! sections the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::Disperser;
use crate::params::console::smear as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub stages: u32,
    pub centre: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Smear.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            stages: clamp(p::AMOUNT).round().max(0.0) as u32,
            centre: clamp(p::CENTRE),
        }
    }

    pub fn is_wire(&self) -> bool {
        self.stages == 0
    }
}

/// THE ONSET CLOCK: the one thing about this section only the
/// callback can know.
///
/// Two one-pole envelopes of the input peak — a fast one that rides a
/// transient's edge and a slow one that holds the bed under it — and
/// the ages of the two most recent times the fast one stood clear of
/// the slow one. Ages are kept in SAMPLES, and `0.0` means "no onset
/// in flight" rather than "an onset this instant": a live onset is
/// always at least one sample old, so the flag costs nothing.
///
/// Two multiply-adds and a compare per sample, plus one `log10` at
/// most once per lockout. No allocation, no lock, no unbounded loop.
struct Onset {
    /// One-pole coefficients for the two envelopes, from the ms time
    /// constants in `params::console::smear`.
    fast_coeff: f32,
    slow_coeff: f32,
    fast: f32,
    slow: f32,
    /// Samples since the newest onset; 0.0 when none is in flight.
    age0: f32,
    /// Samples since the one before it, same convention.
    age1: f32,
    /// The newest onset's strength, 0..1.
    hit0: f32,
    /// The retrigger lockout, in samples.
    lockout: f32,
    /// The age at which an onset stops being in flight, in samples.
    expire: f32,
    /// Sample to millisecond, so `process` can hand `readout` a figure
    /// in the card's units without dividing there.
    ms_per_sample: f32,
}

impl Onset {
    fn new(sample_rate: f32) -> Self {
        let fs = if sample_rate.is_finite() {
            sample_rate.max(1.0)
        } else {
            48_000.0
        };
        let coeff = |ms: f32| 1.0 - (-1000.0 / (ms.max(0.01) * fs)).exp();
        Self {
            fast_coeff: coeff(p::ONSET_FAST_MS),
            slow_coeff: coeff(p::ONSET_SLOW_MS),
            fast: 0.0,
            slow: 0.0,
            age0: 0.0,
            age1: 0.0,
            hit0: 0.0,
            lockout: fs * p::ONSET_LOCKOUT_MS / 1000.0,
            expire: fs * p::ONSET_EXPIRE_MS / 1000.0,
            ms_per_sample: 1000.0 / fs,
        }
    }

    fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
        self.age0 = 0.0;
        self.age1 = 0.0;
        self.hit0 = 0.0;
    }

    /// One sample of the input peak: age the clocks, then decide
    /// whether this sample is an arrival.
    fn feed(&mut self, peak: f32) {
        self.fast += (peak - self.fast) * self.fast_coeff;
        self.slow += (peak - self.slow) * self.slow_coeff;
        if self.age0 > 0.0 {
            self.age0 += 1.0;
            if self.age0 > self.expire {
                // Out of flight: the wavefront has crossed the field
                // and the counter is parked rather than left to grow.
                self.age0 = 0.0;
                self.hit0 = 0.0;
            }
        }
        if self.age1 > 0.0 {
            self.age1 += 1.0;
            if self.age1 > self.expire {
                self.age1 = 0.0;
            }
        }
        let armed = self.age0 == 0.0 || self.age0 > self.lockout;
        if armed && self.fast > p::ONSET_RATIO * self.slow && self.fast > p::ONSET_FLOOR {
            self.age1 = self.age0;
            // One sample of age, not none: 0.0 is the "nothing in
            // flight" flag and an arrival is a something.
            self.age0 = 1.0;
            let over = self.fast / self.slow.max(p::ONSET_BED_FLOOR);
            self.hit0 = (20.0 * over.log10() / p::ONSET_FULL_DB).clamp(0.0, 1.0);
        }
    }

    /// The three bands, in the card's units: ms, 0..1, ms.
    fn bands(&self) -> [f32; 3] {
        [
            self.age0 * self.ms_per_sample,
            self.hit0,
            self.age1 * self.ms_per_sample,
        ]
    }
}

pub struct SmearCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    dispersers: [Disperser; 2],
    level_db: f32,
    onset: Onset,
    /// What `readout` hands over, computed at the end of `process`.
    bands: [f32; 3],
}

impl SmearCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            dispersers: [Disperser::new(), Disperser::new()],
            level_db: -120.0,
            onset: Onset::new(sample_rate),
            bands: [0.0; 3],
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        for disperser in &mut self.dispersers {
            disperser.prepare(self.sample_rate, s.centre, p::STAGE_Q, s.stages);
        }
    }
}

impl SectionCore for SmearCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for disperser in &mut self.dispersers {
            disperser.reset();
        }
        self.level_db = -120.0;
        self.onset.reset();
        self.bands = [0.0; 3];
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        // The onset clock runs on the INPUT, before the dispersers and
        // whether or not any are running: what the card watches crawl
        // across its time axis is when the transient ARRIVED, and a
        // wire has arrivals too.
        if stereo {
            for (a, b) in l.iter().zip(r[..n].iter()) {
                self.onset.feed(a.abs().max(b.abs()));
            }
        } else {
            for a in l.iter() {
                self.onset.feed(a.abs());
            }
        }
        if !self.settings.is_wire() {
            self.dispersers[0].process(l);
            if stereo {
                self.dispersers[1].process(&mut r[..n]);
            }
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
        self.bands = self.onset.bands();
    }

    /// What the card is told, per field:
    ///
    /// - `level_db`: the loudest sample of the block's OUTPUT, dBFS,
    ///   −120.0 at silence, no smoothing — it is the block's extreme,
    ///   not its last sample. The section is flat to 0.3 dB at every
    ///   setting, so this is the input level too, which is what lets
    ///   the two level columns be drawn from the one figure.
    /// - `reduction_db`: always exactly 0.0. This section takes
    ///   nothing away.
    /// - `bands[0]`: MILLISECONDS SINCE THE NEWEST ONSET, 0..1000 ms,
    ///   and EXACTLY 0.0 when no onset is in flight — a live onset is
    ///   always at least one sample old, so 0.0 is a safe "none". The
    ///   clock is started by a peak envelope of 1 ms time constant
    ///   standing 1.6x clear of one of 120 ms, no faster than once
    ///   every 30 ms, and it is dropped back to 0.0 once it passes
    ///   1000 ms.
    /// - `bands[1]`: THAT ONSET'S STRENGTH, 0..1: how far the fast
    ///   envelope stood over the slow bed at the instant it arrived,
    ///   in dB over 12 dB, clamped. It is latched at the onset and
    ///   held for as long as `bands[0]` runs, and falls to 0.0 with
    ///   it.
    /// - `bands[2]`: MILLISECONDS SINCE THE PREVIOUS ONSET, same
    ///   0..1000 ms range and same exact-0.0 convention, so two
    ///   wavefronts can be in flight at once. It is always the larger
    ///   of the two ages while both are live.
    ///
    /// All of it is measured in `process`; this is a copy.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: self.bands,
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

    fn core_with(edits: &[(u32, f32)]) -> SmearCore {
        let mut params = SectionParams::of(SectionKind::Smear);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        SmearCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut SmearCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    /// Run a signal through in blocks and keep the readout after each
    /// one: the card's view, sampled where the graph samples it.
    fn readouts(core: &mut SmearCore, l: &[f32]) -> Vec<Readout> {
        let mut out = l.to_vec();
        let mut said = Vec::new();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
            said.push(core.readout());
        }
        said
    }

    /// One block of 256 at 48 kHz, in ms — the step the clock takes
    /// between two readouts.
    const BLOCK_MS: f32 = 1000.0 * BLOCK as f32 / FS;

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn no_stages_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Allpass: every tone comes out at the level it went in, near the
    /// corner and far from it.
    #[test]
    fn nothing_is_filtered() {
        let n = FS as usize / 2;
        for hz in [100.0, 1_000.0, 4_000.0, 12_000.0] {
            let l = sine(hz, 0.3, n);
            let mut core = core_with(&[(p::AMOUNT, 24.0), (p::CENTRE, 1_000.0)]);
            let out = run(&mut core, &l);
            let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
            assert!(change.abs() < 0.3, "{hz} Hz changed by {change} dB");
        }
    }

    /// A click is spread in time, and further with more sections: the
    /// time by which nine tenths of the energy has arrived grows.
    #[test]
    fn a_click_becomes_a_chirp() {
        let n = 8192;
        let mut l = vec![0.0f32; n];
        l[64] = 1.0;
        let arrival = |stages: f32| -> usize {
            let mut core = core_with(&[(p::AMOUNT, stages), (p::CENTRE, 1_000.0)]);
            let out = run(&mut core, &l);
            let total: f32 = out.iter().map(|s| s * s).sum();
            let mut so_far = 0.0;
            for (i, s) in out.iter().enumerate() {
                so_far += s * s;
                if so_far >= total * 0.9 {
                    return i - 64;
                }
            }
            n
        };
        let none = arrival(0.0);
        let short = arrival(4.0);
        let long = arrival(32.0);
        assert_eq!(none, 0, "a wire spread the click");
        assert!(short > 10, "four sections spread nothing: {short} samples");
        assert!(
            long > short * 2,
            "thirty-two ({long}) against four ({short})"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::AMOUNT, 16.0), (p::CENTRE, 800.0)];
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
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::AMOUNT, 99.0);
        assert_eq!(core.settings().stages, 32);
        core.set_param(p::CENTRE, 10.0);
        assert_eq!(core.settings().centre, 100.0);
        core.set_param(99, 1.0);
        core.set_param(p::AMOUNT, 0.0);
        assert!(core.settings().is_wire());
    }

    /// At its defaults, on silence, the section reports rest: nothing
    /// in flight, no strength, no reduction, the level floor. And the
    /// bands carry no SETTING any more — a section wound all the way
    /// up but hearing nothing still reads three zeroes, which is what
    /// makes a moving band mean something.
    #[test]
    fn at_rest_the_section_reports_rest() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        assert_eq!(core.readout().bands, [0.0, 0.0, 0.0]);
        assert_eq!(core.readout().reduction_db, 0.0);
        for said in readouts(&mut core, &vec![0.0f32; BLOCK * 8]) {
            assert_eq!(said.bands, [0.0, 0.0, 0.0], "silence started a clock");
            assert_eq!(said.reduction_db, 0.0);
            assert_eq!(said.level_db, -120.0);
        }
        let mut wound_up = core_with(&[(p::AMOUNT, 32.0), (p::CENTRE, 8_000.0)]);
        let said = readouts(&mut wound_up, &vec![0.0f32; BLOCK * 4]);
        assert_eq!(
            said[3].bands,
            [0.0, 0.0, 0.0],
            "a setting leaked into a band"
        );
    }

    /// bands[0] is a CLOCK: a click starts it and it counts real
    /// milliseconds, one block's worth per block, until it lets go.
    #[test]
    fn a_click_starts_a_clock_that_counts_milliseconds() {
        let mut core = core_with(&[(p::AMOUNT, 16.0), (p::CENTRE, 1_000.0)]);
        let mut l = vec![0.0f32; BLOCK * 12];
        l[BLOCK + 8] = 1.0;
        let said = readouts(&mut core, &l);
        assert_eq!(said[0].bands[0], 0.0, "the clock ran before the click");
        let first = said[1].bands[0];
        let expected = 1000.0 * (BLOCK - 8) as f32 / FS;
        assert!(
            (first - expected).abs() < 0.05,
            "the click's own block read {first} ms, not {expected}"
        );
        for k in 2..said.len() {
            let step = said[k].bands[0] - said[k - 1].bands[0];
            assert!(
                (step - BLOCK_MS).abs() < 0.01,
                "block {k} advanced {step} ms, not {BLOCK_MS}"
            );
        }
    }

    /// The clock is not a stopwatch: an onset stays in flight for a
    /// second and then every band drops back to exactly 0.0, which is
    /// the "nothing crawling" the card draws as an empty field.
    #[test]
    fn the_clock_lets_go_after_a_second() {
        let mut core = core_with(&[]);
        let mut l = vec![0.0f32; FS as usize * 2];
        l[BLOCK] = 1.0;
        let said = readouts(&mut core, &l);
        let held = said.iter().fold(0.0f32, |peak, r| peak.max(r.bands[0]));
        assert!(
            (held - p::ONSET_EXPIRE_MS).abs() < BLOCK_MS + 0.1,
            "the clock held {held} ms"
        );
        assert_eq!(said[said.len() - 1].bands, [0.0, 0.0, 0.0]);
    }

    /// bands[1] measures the arrival against the bed it landed on: a
    /// click out of silence is a full-strength hit, a swell of the
    /// same music is not.
    #[test]
    fn the_hit_measures_the_onset_against_its_bed() {
        let bare = {
            let mut core = core_with(&[]);
            let mut l = vec![0.0f32; BLOCK * 4];
            l[BLOCK] = 1.0;
            readouts(&mut core, &l)[1].bands[1]
        };
        let over_a_bed = {
            let mut core = core_with(&[]);
            let mut l = sine(1_000.0, 0.2, BLOCK * 48);
            // Four times as loud from block 40, once the slow envelope
            // has had three time constants to learn the quiet bed.
            for s in l[BLOCK * 40..].iter_mut() {
                *s *= 4.0;
            }
            readouts(&mut core, &l)[40].bands[1]
        };
        assert!(bare > 0.9, "a click out of silence read {bare}");
        assert!(
            over_a_bed > 0.1,
            "a swell over a bed read nothing: {over_a_bed}"
        );
        assert!(
            over_a_bed < bare,
            "a swell ({over_a_bed}) read as hard as a click ({bare})"
        );
    }

    /// bands[2] is the onset BEFORE the newest one, on its own clock,
    /// so two wavefronts can be crawling across the field at once.
    #[test]
    fn the_older_onset_keeps_its_own_clock() {
        let mut core = core_with(&[(p::AMOUNT, 32.0), (p::CENTRE, 1_000.0)]);
        let gap = (FS * 0.2) as usize;
        let mut l = vec![0.0f32; BLOCK * 60];
        l[BLOCK] = 1.0;
        l[BLOCK + gap] = 1.0;
        let said = readouts(&mut core, &l);
        let last = said[said.len() - 1];
        assert!(last.bands[0] > 0.0, "the newest onset is not in flight");
        let apart = last.bands[2] - last.bands[0];
        assert!(
            (apart - 200.0).abs() < 0.1,
            "the two clocks are {apart} ms apart, not 200"
        );
    }

    /// One hit is one wavefront: a second click inside the 30 ms
    /// lockout does not start a clock of its own, so the older band
    /// stays at rest.
    #[test]
    fn a_retrigger_inside_the_lockout_is_ignored() {
        let mut core = core_with(&[]);
        let mut l = vec![0.0f32; BLOCK * 20];
        l[BLOCK] = 1.0;
        l[BLOCK + (FS * 0.010) as usize] = 1.0;
        let said = readouts(&mut core, &l);
        let last = said[said.len() - 1];
        assert_eq!(last.bands[2], 0.0, "the lockout let a second onset through");
        let since_first = 1000.0 * (l.len() - BLOCK) as f32 / FS;
        assert!(
            (last.bands[0] - since_first).abs() < 0.05,
            "the clock reads {} ms, not the first click's {since_first}",
            last.bands[0]
        );
    }

    /// A tone that has arrived stops arriving. While the slow envelope
    /// is still learning a signal that began in digital silence the
    /// clock restarts a few times — that is a real onset each time —
    /// but once the bed has caught up the clock only counts on.
    #[test]
    fn a_settled_tone_stops_onsetting() {
        let mut core = core_with(&[]);
        let said = readouts(&mut core, &sine(1_000.0, 0.4, BLOCK * 100));
        let settled = (2.0 * p::ONSET_SLOW_MS / BLOCK_MS) as usize;
        for k in settled..said.len() {
            let step = said[k].bands[0] - said[k - 1].bands[0];
            assert!(
                (step - BLOCK_MS).abs() < 0.01,
                "block {k} of a held tone re-onset: {step} ms"
            );
        }
        assert!(said[said.len() - 1].bands[0] > 2.0 * p::ONSET_SLOW_MS);
    }

    /// The clock runs when the section is a WIRE, and running it costs
    /// the audio nothing: a wire still has arrivals, and the card's
    /// wavefront must sweep an empty rack too.
    #[test]
    fn a_wire_still_reports_the_onset() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        let mut l = vec![0.0f32; BLOCK * 4];
        l[BLOCK] = 1.0;
        let before = l.clone();
        let mut out = l.clone();
        let mut said = Vec::new();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = start + BLOCK;
            core.process(&mut out[start..end], &mut [], &clock());
            said.push(core.readout());
        }
        assert_eq!(out, before, "the wire changed the sound");
        assert!(said[1].bands[0] > 0.0, "a wire reported no arrival");
        assert!(said[1].bands[1] > 0.9, "a wire reported no strength");
    }

    /// The level is the block's peak in dBFS and the reduction stays
    /// zero at every setting: this section takes nothing away, which
    /// is the promise the card's twin columns are drawn from.
    #[test]
    fn the_level_is_the_block_peak_and_nothing_is_taken_away() {
        let mut core = core_with(&[(p::AMOUNT, 32.0), (p::CENTRE, 1_000.0)]);
        let said = readouts(&mut core, &sine(1_000.0, 0.5, BLOCK * 8));
        let last = said[said.len() - 1];
        assert!(
            (last.level_db - 20.0 * 0.5f32.log10()).abs() < 0.3,
            "a half-scale tone read {} dB",
            last.level_db
        );
        for r in &said {
            assert_eq!(r.reduction_db, 0.0);
        }
    }
}
