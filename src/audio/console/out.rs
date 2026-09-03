//! OUT: the channel's last stage — width, bass mono, and the sends.
//!
//! WIDTH is a mid/side balance: at unity the sides are what they were,
//! under it they close toward mono, over it they open past the
//! speakers. BASS MONO takes everything under its corner to the middle,
//! which is what keeps a wide mix's bottom from wandering. Both are off
//! at their resting ends, and with both off the section is a wire to
//! the sample.
//!
//! The two SENDS are not processing: they are the shares of this
//! channel that reach the TAPE and SHADOW returns, and the graph reads
//! them when it wires the desk. They live here because this is where a
//! channel's routing belongs, and because a send that could be locked
//! per trig is a send worth having.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::Cascade;
use crate::params::console::out as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// 0..2, where 1 is unity.
    pub width: f32,
    pub bass_mono: f32,
    pub send_tape: f32,
    pub send_shadow: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Out.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            width: clamp(p::WIDTH) / 100.0,
            bass_mono: clamp(p::BASS_MONO),
            send_tape: clamp(p::SEND_TAPE) / 100.0,
            send_shadow: clamp(p::SEND_SHADOW) / 100.0,
        }
    }

    pub fn width_off(&self) -> bool {
        self.width == p::WIDTH_OFF / 100.0
    }

    pub fn bass_off(&self) -> bool {
        self.bass_mono <= p::BASS_MONO_OFF_HZ
    }

    pub fn is_wire(&self) -> bool {
        self.width_off() && self.bass_off()
    }
}

pub struct OutCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    /// The side's bottom, taken out and put back in the middle.
    side_low: Cascade,
    side_high: Cascade,
    side: Vec<f32>,
    level_db: f32,
}

impl OutCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            settings: Settings::of(params),
            sample_rate,
            side_low: Cascade::new(),
            side_high: Cascade::new(),
            side: vec![0.0; block.max(1)],
            level_db: -120.0,
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        let hz = s.bass_mono.max(20.0);
        let q = core::f32::consts::FRAC_1_SQRT_2;
        self.side_low
            .prepare(self.sample_rate, hz, q, p::BASS_ORDER, false);
        self.side_high
            .prepare(self.sample_rate, hz, q, p::BASS_ORDER, true);
    }
}

impl SectionCore for OutCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.side_low.reset();
        self.side_high.reset();
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.side.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        // Mono in, mono out: neither knob has anything to act on.
        if !s.is_wire() && stereo {
            let side = &mut self.side[..n];
            for i in 0..n {
                side[i] = (l[i] - r[i]) * 0.5;
            }
            if !s.bass_off() {
                // The side's bottom goes; the middle keeps its own.
                self.side_high.process(side);
            }
            let width = s.width;
            for i in 0..n {
                let mid = (l[i] + r[i]) * 0.5;
                let side = side[i] * width;
                l[i] = mid + side;
                r[i] = mid - side;
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
                self.settings.width,
                self.settings.send_tape,
                self.settings.send_shadow,
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

    fn core_with(edits: &[(u32, f32)]) -> OutCore {
        let mut params = SectionParams::of(SectionKind::Out);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        OutCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// A tone panned hard to the sides: the same in both, opposite in
    /// sign, which is pure side and no middle.
    fn sided(hz: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let l = sine(hz, 0.4, n);
        let r: Vec<f32> = l.iter().map(|s| -s).collect();
        (l, r)
    }

    fn run(core: &mut OutCore, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (l.to_vec(), r.to_vec());
        for start in (0..ol.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(ol.len());
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        (ol, or)
    }

    #[test]
    fn both_off_is_a_wire_to_the_sample() {
        let mut core = core_with(&[(p::SEND_TAPE, 50.0)]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| s * 0.3).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Width closes the sides toward mono and opens them past unity.
    #[test]
    fn width_closes_and_opens_the_sides() {
        let n = FS as usize / 4;
        let (l, r) = sided(1_000.0, n);
        let side_after = |width: f32| -> f32 {
            let mut core = core_with(&[(p::WIDTH, width)]);
            let (ol, or) = run(&mut core, &l, &r);
            rms(&ol[n / 2..]
                .iter()
                .zip(&or[n / 2..])
                .map(|(a, b)| (a - b) * 0.5)
                .collect::<Vec<_>>())
        };
        let unity = side_after(100.0);
        assert!(side_after(0.0) < unity * 0.02, "mono still had sides");
        assert!((side_after(50.0) / unity - 0.5).abs() < 0.05);
        assert!((side_after(200.0) / unity - 2.0).abs() < 0.05);
    }

    /// Bass mono takes the bottom to the middle and leaves the top
    /// where it was.
    #[test]
    fn bass_mono_centres_the_bottom_only() {
        let n = FS as usize / 4;
        let side_of = |hz: f32| -> f32 {
            let (l, r) = sided(hz, n);
            let mut core = core_with(&[(p::BASS_MONO, 200.0)]);
            let (ol, or) = run(&mut core, &l, &r);
            let side = rms(&ol[n / 2..]
                .iter()
                .zip(&or[n / 2..])
                .map(|(a, b)| (a - b) * 0.5)
                .collect::<Vec<_>>());
            side / rms(&l[n / 2..])
        };
        assert!(
            side_of(50.0) < 0.2,
            "the bottom stayed wide: {}",
            side_of(50.0)
        );
        assert!(
            side_of(4_000.0) > 0.9,
            "the top was narrowed: {}",
            side_of(4_000.0)
        );
    }

    /// The sends are carried, not processed.
    #[test]
    fn the_sends_are_carried_for_the_graph() {
        let core = core_with(&[(p::SEND_TAPE, 40.0), (p::SEND_SHADOW, 100.0)]);
        assert!((core.settings().send_tape - 0.4).abs() < 1e-6);
        assert_eq!(core.settings().send_shadow, 1.0);
        assert!(core.settings().is_wire(), "a send is not processing");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let (l, r) = sided(330.0, 1000);
        let edits = [(p::WIDTH, 150.0), (p::BASS_MONO, 150.0)];
        let mut whole = core_with(&edits);
        let (a, _) = run(&mut whole, &l, &r);
        let mut pieces = core_with(&edits);
        let (mut b, mut br) = (l.clone(), r.clone());
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut br[at..end], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::WIDTH, 500.0);
        assert_eq!(core.settings().width, 2.0);
        core.set_param(99, 1.0);
        core.set_param(p::WIDTH, 100.0);
        assert!(core.settings().is_wire());
    }
}
