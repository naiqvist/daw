//! PHASE: the phaser.
//!
//! A run of allpass sections whose corner sweeps, mixed with the dry.
//! Where the swept phase and the dry disagree there is a notch, and
//! the notches walk up and down the spectrum — two of them at four
//! sections, eight at sixteen. Two, four, six, eight, twelve or
//! sixteen stages, the way the pedals counted them.
//!
//! FEEDBACK takes the last section's output back to the first, which
//! sharpens the notches into resonances; the path has a soft top, so a
//! fed-back phaser sings rather than screams. OFFSET stands the right
//! side's sweep away from the left's, up to half a turn, which is what
//! turns a phaser into a stereo swirl. At no depth and no feedback the
//! section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::Disperser;
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::params::console::phase as p;

/// The corner is retuned every this many samples rather than every
/// sample: at ten hertz the sweep moves a thousandth of an octave in
/// that time, and a transcendental per sample per side is not a price
/// worth paying for it. The counter runs on a continuous sample clock,
/// so where the blocks fall makes no difference.
const CONTROL: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub stages: u32,
    pub rate: f32,
    /// 0..1.
    pub depth: f32,
    /// −0.9..0.9.
    pub feedback: f32,
    /// Turns, 0..0.5.
    pub offset: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Phase.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        let step = (clamp(p::STAGES).round().max(0.0) as usize).min(p::STAGE_COUNTS.len() - 1);
        Self {
            stages: p::STAGE_COUNTS[step],
            rate: clamp(p::RATE),
            depth: clamp(p::DEPTH) / 100.0,
            feedback: clamp(p::FEEDBACK) / 100.0,
            offset: clamp(p::OFFSET) / 360.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.depth == 0.0 && self.feedback == 0.0
    }
}

pub struct PhaseCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    /// One run of allpass sections per side: the kernel tunes them all
    /// with a single transcendental, which is why the sweep is cheap.
    runs: [Disperser; 2],
    lfos: [Lfo; 2],
    /// What the run gave back, per side.
    fed: [f32; 2],
    /// Samples until the corner is tuned again.
    since: u32,
    level_db: f32,
    sweep_now: [f32; 2],
}

impl PhaseCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            runs: [Disperser::new(), Disperser::new()],
            lfos: [Lfo::new(), Lfo::new()],
            fed: [0.0; 2],
            since: 0,
            level_db: -120.0,
            sweep_now: [0.0; 2],
        };
        for lfo in &mut core.lfos {
            lfo.prepare(sample_rate);
            lfo.set_shape(LfoShape::Sine);
        }
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        for side in 0..2 {
            self.lfos[side].set_rate(s.rate);
            self.lfos[side].set_phase(if side == 1 { s.offset } else { 0.0 });
            self.runs[side].prepare(self.sample_rate, p::LOW_HZ, p::STAGE_Q, s.stages);
        }
    }

    /// The soft top on the feedback path.
    #[inline(always)]
    fn soft(x: f32) -> f32 {
        let k = p::FEEDBACK_KNEE;
        let y = x * k;
        y / (1.0 + y * y).sqrt() / k
    }

    /// The corner the sweep stands at, in Hz: the walk is in octaves,
    /// so it is even to the ear rather than to the number line.
    fn corner(&self, turn: f32) -> f32 {
        let span = (p::HIGH_HZ / p::LOW_HZ).log2();
        p::LOW_HZ * 2f32.powf(span * (turn * 0.5 + 0.5) * self.settings.depth)
    }

    fn run(&mut self, side: usize, io: &mut [f32], tune_at: &[bool]) {
        let n = io.len();
        let s = self.settings;
        let mut one = [0.0f32; 1];
        let mut through = [0.0f32; 1];
        for i in 0..n {
            self.lfos[side].process(&mut one);
            if tune_at[i] {
                let hz = self.corner(one[0]);
                self.runs[side].prepare(self.sample_rate, hz, p::STAGE_Q, s.stages);
            }
            through[0] = io[i] + self.fed[side] * s.feedback;
            self.runs[side].process(&mut through);
            self.fed[side] = Self::soft(through[0]);
            // The classic mix: half dry, half turned.
            io[i] = (io[i] + through[0]) * 0.5;
        }
        self.sweep_now[side] = one[0];
    }
}

impl SectionCore for PhaseCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for side in 0..2 {
            self.runs[side].reset();
            self.lfos[side].reset();
            self.fed[side] = 0.0;
        }
        self.since = 0;
        self.tune();
        self.level_db = -120.0;
        self.sweep_now = [0.0; 2];
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        if self.settings.is_wire() {
            // A wire has no sweep at all: neither side ran, so the last
            // angle the LFOs stopped at is stale the moment the section
            // goes flat. Park the sweep so the readout comes to REST
            // rather than jamming mid-swing at a number that is no
            // longer being computed.
            self.sweep_now = [0.0; 2];
        } else {
            // Where the control clock falls in this block, decided once
            // so both sides tune at the same samples.
            let mut tune_at = [false; MAX_BLOCK];
            for start in (0..n).step_by(MAX_BLOCK) {
                let end = (start + MAX_BLOCK).min(n);
                let width = end - start;
                let mut since = self.since;
                for slot in tune_at.iter_mut().take(width) {
                    *slot = since == 0;
                    since = (since + 1) % CONTROL;
                }
                self.since = since;
                self.run(0, &mut l[start..end], &tune_at[..width]);
                if stereo {
                    self.run(1, &mut r[start..end], &tune_at[..width]);
                } else {
                    // Mono: side 1's run never happened, so its LFO did not
                    // advance. One sweep is the whole truth for this block.
                    self.sweep_now[1] = self.sweep_now[0];
                }
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
    }

    /// What the section tells the surface. A pure copy of fields the
    /// block already measured — no arithmetic, because the graph calls
    /// this on its telemetry step.
    ///
    /// - `level_db`: the loudest OUTPUT sample of the block, both sides
    ///   together, in dBFS. Range −120..≈0 (a resonant feedback peak can
    ///   nudge just above 0). Unsmoothed and unheld, so its time
    ///   constant is the block itself — about 5 ms at 256 samples — and
    ///   it drops to −120 on the first silent block.
    /// - `reduction_db`: always exactly 0. A phaser is a phase network
    ///   mixed half-and-half with the dry; it has no gain computer and
    ///   reduces nothing, ever.
    /// - `bands[0]`: the LEFT SWEEP. The left LFO's own bipolar output
    ///   at the block's last sample — dimensionless, −1..=1, a sine.
    ///   Its only time constant is the sweep's own period, one cycle per
    ///   `Rate`, so 0.1 s at 10 Hz to 20 s at 0.05 Hz; nothing else
    ///   filters or smooths it, and it steps once per block. It is the
    ///   angle the left comb stands at: the corner in Hz is
    ///   `LOW_HZ * 2^(log2(HIGH_HZ / LOW_HZ) * (bands[0] * 0.5 + 0.5) *
    ///   depth)`, which is [`PhaseCore::corner`] verbatim. Exactly 0 and
    ///   dead still whenever the section is a wire, because then the
    ///   LFOs do not run.
    /// - `bands[1]`: the RIGHT SWEEP — the same quantity, same units,
    ///   same −1..=1 range and same time constant, for side 1, whose
    ///   cycle stands `Offset / 360` of a turn ahead of side 0. On a MONO
    ///   block side 1 never runs and this reads exactly `bands[0]`.
    ///   Exactly 0 when the section is a wire.
    /// - `bands[2]`: unused, always exactly 0. The section has two
    ///   sweeps and nothing else that moves; it carries no third figure
    ///   rather than repeating a setting the surface already holds.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: [self.sweep_now[0], self.sweep_now[1], 0.0],
        }
    }
}

/// Stack scratch per chunk. Longer public render blocks are walked through
/// several fixed chunks so they cannot silently bypass the section.
const MAX_BLOCK: usize = 4096;

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

    fn core_with(edits: &[(u32, f32)]) -> PhaseCore {
        let mut params = SectionParams::of(SectionKind::Phase);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        PhaseCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut PhaseCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn run_stereo(core: &mut PhaseCore, l: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (l.to_vec(), l.to_vec());
        for start in (0..l.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(l.len());
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        (ol, or)
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// One `bands` reading per block, so a test can WATCH the sweep move
    /// on the section's own clock instead of asking it what it holds.
    fn bands_over(core: &mut PhaseCore, l: &[f32], stereo: bool) -> Vec<[f32; 3]> {
        let (mut ol, mut or) = (l.to_vec(), l.to_vec());
        let mut said = Vec::new();
        for start in (0..l.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(l.len());
            if stereo {
                core.process(&mut ol[start..end], &mut or[start..end], &clock());
            } else {
                core.process(&mut ol[start..end], &mut [], &clock());
            }
            said.push(core.readout().bands);
        }
        said
    }

    fn span(said: &[[f32; 3]], k: usize) -> (f32, f32) {
        said.iter().fold((f32::MAX, f32::MIN), |(lo, hi), b| {
            (lo.min(b[k]), hi.max(b[k]))
        })
    }

    #[test]
    fn no_depth_and_no_feedback_is_a_wire() {
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

    /// A nearly parked sweep is a comb, and more sections means more
    /// notches: a dense probe across the spectrum falls into more
    /// separate notches at sixteen sections than at two.
    #[test]
    fn more_sections_means_more_notches() {
        let n = FS as usize / 8;
        let notches = |stages: f32| -> usize {
            let mut regions = 0;
            let mut was_cut = false;
            for step in 0..80 {
                let hz = 60.0 * 2f32.powf(step as f32 * 8.0 / 80.0);
                let l = sine(hz, 0.3, n);
                let mut core = core_with(&[(p::STAGES, stages), (p::RATE, 0.05), (p::DEPTH, 2.0)]);
                let out = run(&mut core, &l);
                let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
                let cut = change < -6.0;
                if cut && !was_cut {
                    regions += 1;
                }
                was_cut = cut;
            }
            regions
        };
        let few = notches(0.0);
        let many = notches(5.0);
        assert!(few >= 1, "two sections notched nothing");
        assert!(
            many > few + 2,
            "sixteen sections ({many}) against two ({few})"
        );
    }

    /// Feedback sharpens what the phaser does and stays bounded either
    /// way round.
    #[test]
    fn feedback_sharpens_and_holds() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, 0.3, n);
        let depth_of = |fb: f32| -> f32 {
            let mut core = core_with(&[(p::DEPTH, 60.0), (p::RATE, 2.0), (p::FEEDBACK, fb)]);
            let out = run(&mut core, &l);
            assert!(
                out.iter().all(|s| s.abs() < 2.0),
                "the loop ran away at {fb}"
            );
            // How far the level swings as the sweep passes: a sharper
            // notch means a wider swing.
            let chunks: Vec<f32> = out[n / 4..].chunks(480).map(rms).collect();
            let (lo, hi) = chunks
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            hi - lo
        };
        let plain = depth_of(0.0);
        let fed = depth_of(85.0);
        assert!(
            fed > plain * 1.2,
            "feedback did not sharpen: {plain} to {fed}"
        );
        let _ = depth_of(-85.0);
    }

    /// The offset parts the sides.
    #[test]
    fn the_offset_parts_the_sides() {
        let n = FS as usize / 4;
        let l = sine(800.0, 0.3, n);
        let apart = |offset: f32| -> f32 {
            let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 1.0), (p::OFFSET, offset)]);
            let (ol, or) = run_stereo(&mut core, &l);
            rms(&ol[n / 2..]
                .iter()
                .zip(&or[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>())
        };
        assert!(apart(0.0) < 1e-5, "no offset still parted the sides");
        assert!(apart(180.0) > 0.02, "half a turn did not part them");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::DEPTH, 60.0), (p::RATE, 2.0), (p::FEEDBACK, 40.0)];
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

    /// `bands[0]` is the LEFT sweep itself, running on the phaser's own
    /// clock: it completes one cycle per RATE hertz. Counted rather than
    /// asserted — the band is sampled once a block and its sign changes
    /// are counted against the seconds that actually passed.
    #[test]
    fn the_sweep_band_runs_at_the_rate() {
        let seconds = 2.0f32;
        let n = (FS * seconds) as usize;
        let l = sine(440.0, 0.3, n);
        for rate in [1.0f32, 3.0] {
            let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, rate)]);
            let said = bands_over(&mut core, &l, true);
            let crossings = said
                .windows(2)
                .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
                .count();
            // A sine crosses zero twice a cycle.
            let want = (2.0 * rate * seconds) as usize;
            assert!(
                crossings.abs_diff(want) <= 1,
                "{rate} Hz swept {crossings} crossings, wanted about {want}"
            );
            let (lo, hi) = span(&said, 0);
            assert!(lo < -0.98 && hi > 0.98, "the sweep only reached {lo}..{hi}");
        }
    }

    /// `bands[1]` is the RIGHT sweep, standing OFFSET of a turn from the
    /// left one. Half a turn is exact antiphase and a quarter turn exact
    /// quadrature; both are measured off the readout, and the gap
    /// between the two bands is what draws the two carriages apart.
    #[test]
    fn the_two_bands_stand_the_offset_apart() {
        let n = FS as usize;
        let l = sine(440.0, 0.3, n);

        let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 2.0), (p::OFFSET, 180.0)]);
        let said = bands_over(&mut core, &l, true);
        let worst = said.iter().fold(0.0f32, |w, b| w.max((b[0] + b[1]).abs()));
        assert!(
            worst < 3e-3,
            "half a turn was not antiphase: off by {worst}"
        );

        let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 2.0), (p::OFFSET, 90.0)]);
        let said = bands_over(&mut core, &l, true);
        // sin(t)^2 + sin(t + quarter turn)^2 == 1, and only at quadrature.
        let worst = said.iter().fold(0.0f32, |w, b| {
            w.max((b[0] * b[0] + b[1] * b[1] - 1.0).abs())
        });
        assert!(
            worst < 3e-3,
            "a quarter turn was not square: off by {worst}"
        );
        for k in [0usize, 1] {
            let (lo, hi) = span(&said, k);
            assert!(lo < -0.98 && hi > 0.98, "band {k} only swung {lo}..{hi}");
        }
    }

    /// A mono block runs side 0 only, so side 1's LFO never advances.
    /// The right band must still carry a live sweep rather than the
    /// angle it happened to stop at when the block last went stereo.
    #[test]
    fn a_mono_block_still_moves_the_right_band() {
        let n = FS as usize;
        let l = sine(440.0, 0.3, n);
        let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 2.0)]);
        let said = bands_over(&mut core, &l, false);
        for b in &said {
            assert_eq!(b[0], b[1], "the mono block let the right band drift");
        }
        let (lo, hi) = span(&said, 1);
        assert!(lo < -0.98 && hi > 0.98, "the right band sat at {lo}..{hi}");
    }

    /// At its defaults the section is a wire, and the readout must say
    /// so: no sweep at all, and no level once the sound stops. The
    /// second half pins the other end of it — a section that swept and
    /// was then flattened parks its bands instead of freezing at the
    /// angle it stopped on.
    #[test]
    fn the_defaults_report_rest() {
        let n = FS as usize / 4;
        let l = sine(440.0, 0.3, n);

        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for b in &bands_over(&mut core, &l, true) {
            assert_eq!(*b, [0.0, 0.0, 0.0], "a wire reported a sweep");
        }
        let (mut quiet_l, mut quiet_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        core.process(&mut quiet_l, &mut quiet_r, &clock());
        let said = core.readout();
        assert_eq!(said.bands, [0.0, 0.0, 0.0]);
        assert_eq!(said.level_db, -120.0);
        assert_eq!(said.reduction_db, 0.0);

        let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 2.0)]);
        let moving = bands_over(&mut core, &l, true);
        assert!(
            moving.iter().any(|b| b[0].abs() > 0.5),
            "the sweep never ran to begin with"
        );
        core.set_param(p::DEPTH, 0.0);
        assert!(core.settings().is_wire());
        for b in &bands_over(&mut core, &l, true) {
            assert_eq!(*b, [0.0, 0.0, 0.0], "a flattened section jammed mid-swing");
        }
    }

    /// `bands[0]` used to carry `stages as f32`, a number the surface
    /// already holds as its own parameter and which reads 2..16. It now
    /// carries the left sweep, so it stays inside −1..=1 at EVERY stage
    /// position, and `bands[2]` says nothing at all.
    #[test]
    fn the_bands_carry_no_setting() {
        let n = FS as usize / 8;
        let l = sine(440.0, 0.3, n);
        for step in 0..p::STAGE_COUNTS.len() {
            let mut core = core_with(&[(p::STAGES, step as f32), (p::DEPTH, 70.0), (p::RATE, 2.0)]);
            let said = bands_over(&mut core, &l, true);
            for b in &said {
                assert!(
                    b[0].abs() <= 1.0 && b[1].abs() <= 1.0,
                    "step {step} put a setting in the sweep's slot: {b:?}"
                );
                assert_eq!(b[2], 0.0, "step {step} spoke on the silent band");
            }
            assert!(
                said.iter().any(|b| b[0].abs() > 0.5),
                "step {step}: the sweep never ran"
            );
        }
    }

    /// The level band lights the comb, so it has to follow the sound
    /// and not the settings: a loud block reads far above a quiet one,
    /// and silence reads the floor.
    #[test]
    fn the_level_band_follows_the_sound() {
        let n = BLOCK * 8;
        let level_of = |amp: f32| -> f32 {
            let mut core = core_with(&[(p::DEPTH, 70.0), (p::RATE, 2.0)]);
            let l = sine(440.0, amp, n);
            let _ = bands_over(&mut core, &l, true);
            core.readout().level_db
        };
        let loud = level_of(0.5);
        let quiet = level_of(0.05);
        assert!(
            loud - quiet > 15.0,
            "a tenth of the amplitude only moved the level {loud} to {quiet}"
        );
        assert!(loud < 0.5, "the section handed back more than it was given");
        assert_eq!(level_of(0.0), -120.0, "silence did not read the floor");
    }

    #[test]
    fn letters_land_on_the_steps() {
        let mut core = core_with(&[]);
        core.set_param(p::STAGES, 9.0);
        assert_eq!(core.settings().stages, 16);
        core.set_param(p::STAGES, 0.0);
        assert_eq!(core.settings().stages, 2);
        core.set_param(p::FEEDBACK, 200.0);
        assert!((core.settings().feedback - 0.9).abs() < 1e-6);
        core.set_param(99, 1.0);
    }

    #[test]
    fn a_public_render_block_larger_than_the_stack_chunk_still_runs_the_phaser() {
        let input = sine(733.0, 0.4, MAX_BLOCK + 907);
        let mut whole = input.clone();
        let mut split = input.clone();
        let edits = [(p::DEPTH, 80.0), (p::RATE, 1.7), (p::FEEDBACK, 35.0)];
        let mut one = core_with(&edits);
        let mut pieces = core_with(&edits);
        one.process(&mut whole, &mut [], &clock());
        for chunk in split.chunks_mut(MAX_BLOCK) {
            pieces.process(chunk, &mut [], &clock());
        }
        assert_eq!(whole, split, "internal chunking changed the phaser clock");
        assert_ne!(whole, input, "the oversized block silently bypassed PHASE");
    }
}
