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
            params: params.clone(),
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
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > MAX_BLOCK {
            return;
        }
        let stereo = r.len() >= n;
        if !self.settings.is_wire() {
            // Where the control clock falls in this block, decided once
            // so both sides tune at the same samples.
            let mut tune_at = [false; MAX_BLOCK];
            let mut since = self.since;
            for slot in tune_at.iter_mut().take(n) {
                if since == 0 {
                    *slot = true;
                }
                since = (since + 1) % CONTROL;
            }
            self.since = since;
            self.run(0, l, &tune_at[..n]);
            if stereo {
                self.run(1, &mut r[..n], &tune_at[..n]);
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

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: [
                self.settings.stages as f32,
                self.sweep_now[0],
                self.sweep_now[1],
            ],
        }
    }
}

/// The longest block the section answers: the graph's arena never
/// hands a node more, and a stack array is what keeps the control
/// clock allocation-free.
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
}
