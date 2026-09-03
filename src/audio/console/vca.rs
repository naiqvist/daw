//! VCA: the SSL bus compressor, on the channel.
//!
//! The G-series design, which is the compressor that makes drums sound
//! like a record: a FEEDBACK topology — the detector listens to the
//! VCA's output, not its input, so the compressor regulates itself and
//! leans into the sound rather than grabbing it — with the box's
//! stepped controls: ratio 2:1, 4:1, 10:1; attack 0.1 to 30 ms in six
//! steps; release 0.1 to 1.2 s and AUTO, which is two time constants
//! at once so a single hit lets go quickly and a sustained push lets go
//! slowly. Stereo-linked by the louder side, so the image never walks.
//! A soft knee of a few dB, fixed, is what the detector and the loop
//! give it. Threshold and makeup are continuous.
//!
//! What the channel needs that the bus box does not have: a sidechain
//! high-pass so bass does not pump it, and a MIX for parallel
//! compression. MIX at zero is a wire to the sample.
//!
//! The feedback loop is closed per sample: the gain applied to sample
//! `n` comes from what the detector heard at sample `n − 1`, which is
//! the whole topology in one line.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::arith::gain_to_db;
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::dsp::filters::OnePole;
use crate::params::console::vca as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    /// `None` is AUTO.
    pub release_ms: Option<f32>,
    pub makeup_db: f32,
    pub sc_hp: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Vca.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        let step = |id: u32, count: usize| (clamp(id).round().max(0.0) as usize).min(count - 1);
        let release = step(p::RELEASE, p::RELEASE_MS.len() + 1);
        Self {
            threshold_db: clamp(p::THRESHOLD),
            ratio: p::RATIO_VALUES[step(p::RATIO, p::RATIO_VALUES.len())],
            attack_ms: p::ATTACK_MS[step(p::ATTACK, p::ATTACK_MS.len())],
            release_ms: (release != p::RELEASE_AUTO).then(|| p::RELEASE_MS[release]),
            makeup_db: clamp(p::MAKEUP),
            sc_hp: clamp(p::SC_HP),
            mix: clamp(p::MIX) / 100.0,
        }
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub struct VcaCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    detector: RmsDetector,
    /// The same detector, on the SAME key signal but BEFORE the
    /// sidechain high-pass: what the filter is holding back is the
    /// difference between the two, and the card draws it.
    raw: RmsDetector,
    sc_hp: OnePole,
    computer: GainComputer,
    ballistics: Ballistics,
    /// The gain in dB from the previous sample's detector reading —
    /// the feedback.
    reduction_db: f32,
    makeup: f32,
    /// Block peak of the OUTPUT, post-makeup, post-mix, in dBFS.
    level_db: f32,
    /// Block max of the key level BEFORE the sidechain high-pass.
    key_raw_db: f32,
    /// Block max of the key level AFTER it — what the gain computer
    /// was actually fed.
    key_db: f32,
    most_reduced_db: f32,
}

impl VcaCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            detector: RmsDetector::new(),
            raw: RmsDetector::new(),
            sc_hp: OnePole::new(),
            computer: GainComputer::new(),
            ballistics: Ballistics::new(),
            reduction_db: 0.0,
            makeup: 1.0,
            level_db: p::FLOOR_DB,
            key_raw_db: p::FLOOR_DB,
            key_db: p::FLOOR_DB,
            most_reduced_db: 0.0,
        };
        core.detector.prepare(sample_rate, p::DETECT_MS);
        core.raw.prepare(sample_rate, p::DETECT_MS);
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// The gain right now, in dB.
    pub fn gain_db(&self) -> f32 {
        self.reduction_db
    }

    fn tune(&mut self) {
        let s = self.settings;
        self.sc_hp.prepare(self.sample_rate, s.sc_hp);
        self.computer
            .configure(Mode::Compress, s.threshold_db, s.ratio, p::KNEE_DB);
        // AUTO is the ballistics' own program-dependent release, over
        // the slowest fixed time; a fixed release is what it says.
        let release = s
            .release_ms
            .unwrap_or(p::RELEASE_MS[p::RELEASE_MS.len() - 1]);
        self.ballistics
            .prepare(self.sample_rate, s.attack_ms, release);
        self.ballistics.set_auto(s.release_ms.is_none());
        self.makeup = db_to_gain(s.makeup_db);
    }
}

impl SectionCore for VcaCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.raw.reset();
        self.sc_hp.reset();
        self.ballistics.reset();
        self.reduction_db = 0.0;
        self.level_db = p::FLOOR_DB;
        self.key_raw_db = p::FLOOR_DB;
        self.key_db = p::FLOOR_DB;
        self.most_reduced_db = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if s.mix <= 0.0 {
            self.reduction_db = 0.0;
            self.most_reduced_db = 0.0;
            // A wire: in IS out, and there is no key, so the two key
            // bands rest on the floor rather than lying about a level.
            self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
            self.key_raw_db = p::FLOOR_DB;
            self.key_db = p::FLOOR_DB;
            return;
        }
        let mut most_reduced = 0.0f32;
        let mut loudest = p::FLOOR_DB;
        let mut raw_loudest = p::FLOOR_DB;
        let mut out_peak = 0.0f32;
        let key_hp = s.sc_hp > 20.5;
        for i in 0..n {
            let dry_l = l[i];
            let dry_r = if stereo { r[i] } else { dry_l };
            // The gain from the previous sample's reading: the feedback.
            let gain = db_to_gain(self.reduction_db);
            let wet_l = dry_l * gain;
            let wet_r = dry_r * gain;
            // The detector hears the output, pre-makeup, linked by the
            // louder side.
            let mut side = wet_l.abs().max(wet_r.abs());
            // The key as the loop hears it before the filter: one
            // one-pole tick, so the card can show what the high-pass
            // takes out.
            let raw_db = gain_to_db(self.raw.tick(side).max(p::FLOOR_GAIN));
            if key_hp {
                side = self.sc_hp.tick_highpass(side);
            }
            let level = self.detector.tick(side);
            let level_db = gain_to_db(level.max(p::FLOOR_GAIN));
            let target = self.computer.gain_db(level_db);
            self.reduction_db = self.ballistics.tick(target);
            loudest = loudest.max(level_db);
            raw_loudest = raw_loudest.max(raw_db);
            most_reduced = most_reduced.min(self.reduction_db);
            let out_l = dry_l + (wet_l * self.makeup - dry_l) * s.mix;
            l[i] = out_l;
            out_peak = out_peak.max(out_l.abs());
            if stereo {
                let out_r = dry_r + (wet_r * self.makeup - dry_r) * s.mix;
                r[i] = out_r;
                out_peak = out_peak.max(out_r.abs());
            }
        }
        self.level_db = if out_peak <= p::FLOOR_GAIN {
            p::FLOOR_DB
        } else {
            20.0 * out_peak.log10()
        };
        self.key_db = loudest;
        self.key_raw_db = raw_loudest;
        self.most_reduced_db = most_reduced;
    }

    /// Everything here is MEASURED; not one field is a setting the
    /// card could read back off its own parameters.
    ///
    /// - `level_db`: the OUTPUT's peak over the block, post-makeup and
    ///   post-mix, in dBFS. Range `FLOOR_DB` (silence) up through 0 and
    ///   above, since makeup can push it over full scale. No smoothing:
    ///   it is a block maximum, so it steps once per block.
    /// - `reduction_db`: the DEEPEST gain the loop reached in the
    ///   block, in dB, `<= 0`; 0 is no reduction. A block minimum of
    ///   the same figure `bands[0]` carries, so it is the peak-hold and
    ///   never reads shallower than `bands[0]`.
    /// - `bands[0]`: the gain right now, in dB, `<= 0`, taken at the
    ///   block's last sample. Its time constant is the user's own:
    ///   `attack_ms` (0.1..30 ms) falling, `release_ms` (100..1200 ms,
    ///   or the two constants of AUTO) rising. Practically 0 to about
    ///   -24 dB.
    /// - `bands[1]`: the key level BEFORE the sidechain high-pass, in
    ///   dB, as a block maximum of a `DETECT_MS` (5 ms) RMS detector on
    ///   the linked, rectified, pre-makeup OUTPUT — the feedback
    ///   topology, so this is what came back round the loop. Range
    ///   `FLOOR_DB` to roughly 0.
    /// - `bands[2]`: the same level AFTER the high-pass — exactly what
    ///   the gain computer was fed, on the same 5 ms window and the
    ///   same block maximum, so `bands[1] - bands[2]` is the dB the
    ///   filter held back and is 0 when the filter is bypassed
    ///   (`sc_hp <= 20.5`).
    ///
    /// At MIX 0 the section is a wire: `level_db` is the input peak
    /// (in equals out), `reduction_db` and `bands[0]` are 0, and the
    /// two key bands rest at `FLOOR_DB` because no key is running.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: self.most_reduced_db,
            bands: [self.reduction_db, self.key_raw_db, self.key_db],
        }
    }
}

fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= p::FLOOR_GAIN {
        p::FLOOR_DB
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

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> VcaCore {
        let mut params = SectionParams::of(SectionKind::Vca);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        VcaCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut VcaCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn db(amp: f32) -> f32 {
        db_to_gain(amp)
    }

    /// The settled change a sine at `amp_db` gets, in dB.
    fn settled_db(core: &mut VcaCore, amp_db: f32) -> f32 {
        let n = FS as usize;
        let l = sine(1_000.0, db(amp_db), n);
        let out = run(core, &l);
        20.0 * (rms(&out[n * 3 / 4..]) / rms(&l[n * 3 / 4..])).log10()
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
        let mut core = core_with(&[(p::MIX, 0.0), (p::THRESHOLD, -40.0)]);
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Under the threshold nothing happens; over it the loop settles
    /// where a feedback compressor does — softer than the ratio says,
    /// which is the design — and a higher ratio settles lower.
    #[test]
    fn over_the_threshold_it_leans_in_by_its_ratio() {
        let mut under = core_with(&[(p::THRESHOLD, -10.0), (p::RATIO, 1.0)]);
        assert!(settled_db(&mut under, -30.0).abs() < 0.2);

        let mut four = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 1.0)]);
        let at_four = settled_db(&mut four, -6.0);
        assert!(
            at_four < -4.0 && at_four > -14.0,
            "4:1 settled at {at_four} dB"
        );
        assert!(four.readout().reduction_db < -4.0);

        let mut ten = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0)]);
        let at_ten = settled_db(&mut ten, -6.0);
        // A feedback loop settles at (in − thr) / (2 − 1/R): the ratios
        // sit closer together than their numbers, which is the design.
        assert!(
            at_ten < at_four - 0.5,
            "10:1 ({at_ten}) is not deeper than 4:1 ({at_four})"
        );

        let mut two = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 0.0)]);
        let at_two = settled_db(&mut two, -6.0);
        assert!(
            at_two > at_four + 0.5,
            "2:1 ({at_two}) is not gentler than 4:1 ({at_four})"
        );
    }

    /// A slow attack lets the first milliseconds of a hit through; a
    /// fast one does not.
    #[test]
    fn the_attack_steps_are_real() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, db(-6.0), n);
        let first = |core: &mut VcaCore| -> f32 {
            let out = run(core, &l);
            rms(&out[..240])
        };
        let mut fast = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::ATTACK, 0.0)]);
        let mut slow = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::ATTACK, 5.0)]);
        let (fast_first, slow_first) = (first(&mut fast), first(&mut slow));
        assert!(
            slow_first > fast_first * 1.3,
            "slow {slow_first}, fast {fast_first}"
        );
    }

    /// AUTO lets go of a short burst faster than the longest fixed
    /// release does.
    #[test]
    fn auto_release_lets_a_burst_go() {
        let n = FS as usize / 2;
        let mut l = vec![0.0f32; n];
        for (i, s) in l.iter_mut().enumerate().take(2400) {
            *s = 0.9 * (2.0 * core::f32::consts::PI * 1_000.0 * i as f32 / FS).sin();
        }
        let gain_after = |core: &mut VcaCore| -> f32 {
            let mut out = l.clone();
            for start in (0..n).step_by(BLOCK) {
                let end = (start + BLOCK).min(n);
                core.process(&mut out[start..end], &mut [], &clock());
                if end >= 2400 + 14_400 {
                    break;
                }
            }
            core.gain_db()
        };
        let mut auto = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::RELEASE, 4.0)]);
        let mut slow = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::RELEASE, 3.0)]);
        let (auto_gain, slow_gain) = (gain_after(&mut auto), gain_after(&mut slow));
        assert!(
            auto_gain > slow_gain + 1.0,
            "auto {auto_gain} dB, 1.2 s {slow_gain} dB"
        );
    }

    /// The sidechain high-pass: bass under it does not pump the loop.
    #[test]
    fn the_sidechain_high_pass_ignores_the_bass() {
        let mut open = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::SC_HP, 20.0)]);
        let n = FS as usize;
        let bass = sine(40.0, db(-6.0), n);
        let out = run(&mut open, &bass);
        let pumped = 20.0 * (rms(&out[n * 3 / 4..]) / rms(&bass[n * 3 / 4..])).log10();
        assert!(
            pumped < -4.0,
            "the bass did not pump an open sidechain: {pumped} dB"
        );

        let mut keyed = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::SC_HP, 400.0)]);
        let out = run(&mut keyed, &bass);
        let left = 20.0 * (rms(&out[n * 3 / 4..]) / rms(&bass[n * 3 / 4..])).log10();
        assert!(left > -1.0, "the bass pumped a keyed sidechain: {left} dB");
    }

    /// Linked: a loud left and a quiet right are reduced by the same
    /// amount, so the image holds.
    #[test]
    fn the_sides_are_linked() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, db(-6.0), n);
        let r = sine(1_000.0, db(-26.0), n);
        let (mut ol, mut or) = (l.clone(), r.clone());
        let mut core = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0)]);
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        let left = 20.0 * (rms(&ol[n * 3 / 4..]) / rms(&l[n * 3 / 4..])).log10();
        let right = 20.0 * (rms(&or[n * 3 / 4..]) / rms(&r[n * 3 / 4..])).log10();
        assert!(
            (left - right).abs() < 0.2,
            "left {left} dB, right {right} dB"
        );
        assert!(left < -4.0);
    }

    /// MIX blends the compressed sound with the dry: half way sits
    /// between the two.
    #[test]
    fn mix_is_parallel_compression() {
        let mut full = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::MIX, 100.0)]);
        let mut half = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::MIX, 50.0)]);
        let at_full = settled_db(&mut full, -6.0);
        let at_half = settled_db(&mut half, -6.0);
        assert!(
            at_half > at_full + 1.0 && at_half < -0.5,
            "full {at_full}, half {at_half}"
        );
    }

    #[test]
    fn makeup_is_a_gain_after_the_loop() {
        let mut plain = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 1.0)]);
        let mut made_up = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 1.0), (p::MAKEUP, 6.0)]);
        let a = settled_db(&mut plain, -6.0);
        let b = settled_db(&mut made_up, -6.0);
        assert!((b - a - 6.0).abs() < 0.3, "plain {a}, made up {b}");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, db(-6.0), 2000);
        let edits = [(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::ATTACK, 1.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244, 256, 256, 256, 232] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    /// The key bands carry a MEASURED level, not the threshold and the
    /// ratio they used to carry: the same section, fed two sines twenty
    /// dB apart, reports two key levels twenty dB apart.
    #[test]
    fn the_key_band_follows_the_sound() {
        let n = FS as usize / 2;
        let heard = |amp_db: f32| -> Readout {
            // Threshold at the top, so almost nothing is taken and the
            // band is reading the sound rather than the loop.
            let mut core = core_with(&[(p::THRESHOLD, 0.0), (p::RATIO, 0.0)]);
            run(&mut core, &sine(1_000.0, db(amp_db), n));
            core.readout()
        };
        let loud = heard(-6.0);
        let quiet = heard(-26.0);
        assert!(
            loud.bands[2] - quiet.bands[2] > 12.0,
            "loud {} dB, quiet {} dB",
            loud.bands[2],
            quiet.bands[2]
        );
        // A rectified sine reads about 3 dB under its peak, so neither
        // band can be the threshold (0) or the ratio (2.0).
        assert!(loud.bands[1] < -6.0 && loud.bands[2] < -6.0);
        assert!(quiet.bands[2] < -20.0);
    }

    /// The two key bands bracket the sidechain filter: bypassed they
    /// agree exactly, and on a 40 Hz key at 400 Hz they are far apart,
    /// which is the dB the filter is holding back.
    #[test]
    fn the_key_bands_bracket_the_sidechain_filter() {
        let n = FS as usize;
        let bass = sine(40.0, db(-6.0), n);
        let mut open = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::SC_HP, 20.0)]);
        run(&mut open, &bass);
        let open = open.readout();
        assert_eq!(
            open.bands[1], open.bands[2],
            "a bypassed filter must hold nothing back"
        );

        let mut keyed = core_with(&[(p::THRESHOLD, -30.0), (p::RATIO, 2.0), (p::SC_HP, 400.0)]);
        run(&mut keyed, &bass);
        let keyed = keyed.readout();
        assert!(
            keyed.bands[1] - keyed.bands[2] > 6.0,
            "pre {} dB, post {} dB",
            keyed.bands[1],
            keyed.bands[2]
        );
        // Same sound into both, but the open sidechain ducked itself,
        // and the key is tapped off that output: its pre-filter band
        // sits lower for it.
        assert!(
            keyed.bands[1] > open.bands[1] + 3.0,
            "keyed {} dB, open {} dB",
            keyed.bands[1],
            open.bands[1]
        );
    }

    /// `level_db` is the OUTPUT, after makeup and after mix — not the
    /// key it used to report.
    #[test]
    fn level_db_is_the_output() {
        let n = FS as usize;
        let tone = sine(1_000.0, db(-6.0), n);
        let out_level = |edits: &[(u32, f32)]| -> f32 {
            let mut core = core_with(edits);
            run(&mut core, &tone);
            core.readout().level_db
        };
        let plain = out_level(&[(p::THRESHOLD, -30.0), (p::RATIO, 1.0)]);
        let made_up = out_level(&[(p::THRESHOLD, -30.0), (p::RATIO, 1.0), (p::MAKEUP, 6.0)]);
        assert!(
            (made_up - plain - 6.0).abs() < 0.5,
            "plain {plain} dB, made up {made_up} dB"
        );
        // The input peaks at -6 dBFS; a compressed output is under it.
        assert!(plain < -8.0, "that is not the output: {plain} dB");

        // MIX at zero is a wire, so in equals out and there is no key.
        let mut wire = core_with(&[(p::MIX, 0.0), (p::THRESHOLD, -40.0)]);
        run(&mut wire, &tone);
        let wire = wire.readout();
        assert!(
            (wire.level_db + 6.0).abs() < 0.2,
            "wire {} dB",
            wire.level_db
        );
        assert_eq!(wire.bands, [0.0, p::FLOOR_DB, p::FLOOR_DB]);
    }

    /// At its defaults the section reports REST: nothing before it has
    /// heard anything, and nothing after a block of silence.
    #[test]
    fn at_its_defaults_it_reports_rest() {
        let mut core = core_with(&[]);
        let fresh = core.readout();
        assert_eq!(fresh.level_db, p::FLOOR_DB);
        assert_eq!(fresh.reduction_db, 0.0);
        assert_eq!(fresh.bands, [0.0, p::FLOOR_DB, p::FLOOR_DB]);

        let mut silence = vec![0.0f32; BLOCK * 4];
        for start in (0..silence.len()).step_by(BLOCK) {
            let end = start + BLOCK;
            core.process(&mut silence[start..end], &mut [], &clock());
        }
        assert!(silence.iter().all(|s| *s == 0.0));
        let rest = core.readout();
        assert_eq!(rest.level_db, p::FLOOR_DB);
        assert_eq!(rest.reduction_db, 0.0);
        assert_eq!(rest.bands[0], 0.0);
        for band in [rest.bands[1], rest.bands[2]] {
            assert!((band - p::FLOOR_DB).abs() < 0.01, "{band} dB is not rest");
        }
    }

    #[test]
    fn letters_land_on_the_steps() {
        let mut core = core_with(&[]);
        core.set_param(p::RATIO, 9.0);
        assert_eq!(core.settings().ratio, 10.0);
        core.set_param(p::ATTACK, 2.4);
        assert_eq!(core.settings().attack_ms, 1.0);
        core.set_param(p::RELEASE, 4.0);
        assert_eq!(core.settings().release_ms, None);
        core.set_param(p::RELEASE, 0.0);
        assert_eq!(core.settings().release_ms, Some(100.0));
        core.set_param(p::THRESHOLD, 50.0);
        assert_eq!(core.settings().threshold_db, 0.0);
        core.set_param(99, 1.0);
    }
}
