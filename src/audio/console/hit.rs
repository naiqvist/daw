//! HIT: the transient shaper.
//!
//! Two levers on the shape of every note, not its level: ATTACK lifts
//! or softens the strike, SUSTAIN lengthens or shortens the tail, each
//! up to twelve dB, and BRIGHT puts an edge on the strike alone. Level
//! does not enter into it — a ghost note and a rimshot get the same
//! treatment — which is what separates a transient shaper from a
//! compressor and is what the tests hold it to.
//!
//! The strike is `dsp::dynamics::TransientSplit`'s weight: the gap
//! between a fast and a slow envelope, divided by the fast one. The tail
//! is the same idea turned round — how far a quick follower has fallen
//! from a long one — so it rises as a note decays. The brightness is
//! the strike's own top: a high-passed copy of the sound, added back in
//! proportion to the strike's weight, so the click of a hit is lifted
//! and the body and the tail are left alone. At all three levers at
//! rest the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::dynamics::TransientSplit;
use crate::dsp::filters::OnePole;
use crate::dsp::ramps::one_pole_coeff;
use crate::params::console::hit as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// −1..1.
    pub attack: f32,
    pub sustain: f32,
    /// 0..1.
    pub bright: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Hit.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            attack: clamp(p::ATTACK) / 100.0,
            sustain: clamp(p::SUSTAIN) / 100.0,
            bright: clamp(p::BRIGHT) / 100.0,
        }
    }

    pub fn is_rest(&self) -> bool {
        self.attack == 0.0 && self.sustain == 0.0 && self.bright == 0.0
    }
}

pub struct HitCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    strike: TransientSplit,
    /// The brightness: a high-pass per channel and the copy it runs on.
    edge: [OnePole; 2],
    top: Vec<f32>,
    /// The tail's two followers, and their coefficients.
    quick: f32,
    long: f32,
    quick_release: f32,
    long_release: f32,
    /// Compile-owned scratch: the key and the strike weight.
    key: Vec<f32>,
    weight: Vec<f32>,
    level_db: f32,
    /// The last strike and tail weights, and the most the gain moved,
    /// for the card.
    strike_now: f32,
    tail_now: f32,
    moved_db: f32,
}

impl HitCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let settings = Settings::of(params);
        let mut core = Self {
            params: params.clone(),
            settings,
            sample_rate,
            strike: TransientSplit::new(),
            edge: [OnePole::new(), OnePole::new()],
            top: vec![0.0; block.max(1)],
            quick: 0.0,
            long: 0.0,
            quick_release: one_pole_coeff(1000.0 / (p::TAIL_QUICK_MS * sample_rate)),
            long_release: one_pole_coeff(1000.0 / (p::TAIL_LONG_MS * sample_rate)),
            key: vec![0.0; block.max(1)],
            weight: vec![0.0; block.max(1)],
            level_db: -120.0,
            strike_now: 0.0,
            tail_now: 0.0,
            moved_db: 0.0,
        };
        core.strike.prepare(sample_rate, p::WINDOW_MS);
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        for edge in &mut self.edge {
            edge.prepare(self.sample_rate, p::BRIGHT_HZ);
        }
    }

    /// The brightness on one channel: the sound's top, added back by
    /// the strike's weight.
    fn brighten(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let top = &mut self.top[..n];
        top.copy_from_slice(io);
        self.edge[ch].process_highpass(top);
        let amount = p::BRIGHT_AMOUNT * self.settings.bright;
        for ((y, t), w) in io.iter_mut().zip(top.iter()).zip(&self.weight[..n]) {
            *y += t * amount * w;
        }
    }
}

impl SectionCore for HitCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.strike.reset();
        for edge in &mut self.edge {
            edge.reset();
        }
        self.quick = 0.0;
        self.long = 0.0;
        self.level_db = -120.0;
        self.strike_now = 0.0;
        self.tail_now = 0.0;
        self.moved_db = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.key.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if s.is_rest() {
            self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
            self.moved_db = 0.0;
            return;
        }

        // The key, and the strike's weight along it.
        for i in 0..n {
            self.key[i] = if stereo { (l[i] + r[i]) * 0.5 } else { l[i] };
        }
        self.strike.process(&self.key[..n], &mut self.weight[..n]);

        let mut most_moved = 0.0f32;
        let (mut strike_now, mut tail_now) = (0.0f32, 0.0f32);
        for i in 0..n {
            // The tail: how far the quick follower has fallen from the
            // long one, as a share of the long one.
            // Both followers take a peak at once, so a fresh strike
            // reads as no tail at all; they part as the note decays.
            let x = self.key[i].abs();
            if x > self.quick {
                self.quick = x;
            } else {
                self.quick += (x - self.quick) * self.quick_release;
            }
            if x > self.long {
                self.long = x;
            } else {
                self.long += (x - self.long) * self.long_release;
            }
            let tail = if self.long > 1e-6 {
                ((self.long - self.quick) / self.long).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let strike = self.weight[i];
            let db = p::RANGE_DB * (s.attack * strike + s.sustain * tail);
            let gain = 10f32.powf(db / 20.0);
            l[i] *= gain;
            if stereo {
                r[i] *= gain;
            }
            if db.abs() > most_moved.abs() {
                most_moved = db;
            }
            strike_now = strike;
            tail_now = tail;
        }
        if s.bright > 0.0 {
            self.brighten(0, l);
            if stereo {
                self.brighten(1, &mut r[..n]);
            }
        }
        self.strike_now = strike_now;
        self.tail_now = tail_now;
        self.moved_db = most_moved;
        self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            // The most the gain moved this block, signed: a lift reads
            // positive here, which the card shows as an arrow up.
            reduction_db: self.moved_db,
            bands: [self.strike_now, self.tail_now, 0.0],
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

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> HitCore {
        let mut params = SectionParams::of(SectionKind::Hit);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        HitCore::new(&params, FS, BLOCK)
    }

    fn run(core: &mut HitCore, l: &[f32]) -> Vec<f32> {
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

    /// A drum-like note: a 200 Hz body with a 150 ms decay under a
    /// 6 kHz stick that is gone in three, struck every 400 ms, at `amp`.
    fn drums(amp: f32, hits: usize) -> Vec<f32> {
        let period = FS as usize * 2 / 5;
        let n = period * hits;
        (0..n)
            .map(|i| {
                let t = (i % period) as f32 / FS;
                let body = (-t / 0.15).exp() * (2.0 * core::f32::consts::PI * 200.0 * t).sin();
                let stick = (-t / 0.003).exp() * (2.0 * core::f32::consts::PI * 6_000.0 * t).sin();
                amp * (body + 0.6 * stick)
            })
            .collect()
    }

    /// The strike and the tail of the last hit, as rms: the first 4 ms
    /// and the stretch from 60 to 160 ms.
    fn strike_and_tail(signal: &[f32], hits: usize) -> (f32, f32) {
        let period = FS as usize * 2 / 5;
        let start = period * (hits - 1);
        let strike = &signal[start..start + 192];
        let tail = &signal[start + 2880..start + 7680];
        (rms(strike), rms(tail))
    }

    #[test]
    fn at_rest_it_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        for len in [0usize, 1, 7, BLOCK] {
            let l: Vec<f32> = (0..len).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// ATTACK moves the strike and leaves the tail; SUSTAIN moves the
    /// tail and leaves the strike.
    #[test]
    fn attack_moves_the_strike_and_sustain_moves_the_tail() {
        let hits = 3;
        let l = drums(0.5, hits);
        let (strike_in, tail_in) = strike_and_tail(&l, hits);

        let mut more = core_with(&[(p::ATTACK, 100.0)]);
        let out = run(&mut more, &l);
        let (strike, tail) = strike_and_tail(&out, hits);
        let lift = 20.0 * (strike / strike_in).log10();
        assert!(lift > 4.0, "the strike was lifted by only {lift} dB");
        assert!(
            (20.0 * (tail / tail_in).log10()).abs() < 1.0,
            "the tail moved"
        );

        let mut less = core_with(&[(p::ATTACK, -100.0)]);
        let out = run(&mut less, &l);
        let (strike, _) = strike_and_tail(&out, hits);
        let cut = 20.0 * (strike / strike_in).log10();
        assert!(cut < -4.0, "the strike was softened by only {cut} dB");

        let mut longer = core_with(&[(p::SUSTAIN, 100.0)]);
        let out = run(&mut longer, &l);
        let (strike, tail) = strike_and_tail(&out, hits);
        let lift = 20.0 * (tail / tail_in).log10();
        assert!(lift > 4.0, "the tail was lifted by only {lift} dB");
        assert!(
            (20.0 * (strike / strike_in).log10()).abs() < 1.5,
            "the strike moved"
        );

        let mut shorter = core_with(&[(p::SUSTAIN, -100.0)]);
        let out = run(&mut shorter, &l);
        let (_, tail) = strike_and_tail(&out, hits);
        let cut = 20.0 * (tail / tail_in).log10();
        assert!(cut < -4.0, "the tail was shortened by only {cut} dB");
    }

    /// Level does not enter into it: a quiet hit and a loud one get
    /// the same lift in dB.
    #[test]
    fn a_ghost_note_and_a_rimshot_get_the_same_treatment() {
        let hits = 3;
        let lift_at = |amp: f32| -> f32 {
            let l = drums(amp, hits);
            let (strike_in, _) = strike_and_tail(&l, hits);
            let mut core = core_with(&[(p::ATTACK, 80.0)]);
            let out = run(&mut core, &l);
            let (strike, _) = strike_and_tail(&out, hits);
            20.0 * (strike / strike_in).log10()
        };
        let quiet = lift_at(0.05);
        let loud = lift_at(0.5);
        assert!(
            (quiet - loud).abs() < 1.0,
            "quiet {quiet} dB, loud {loud} dB"
        );
    }

    /// BRIGHT puts an edge on the strike: more of the strike's energy
    /// sits above three kilohertz, and the tail is left alone.
    #[test]
    fn bright_puts_an_edge_on_the_strike() {
        let hits = 3;
        let l = drums(0.5, hits);
        let mut core = core_with(&[(p::BRIGHT, 100.0)]);
        let out = run(&mut core, &l);
        let period = FS as usize * 2 / 5;
        let start = period * (hits - 1);
        // The strike's edge: the difference from one sample to the next
        // is what a slew brightener lifts.
        let edge = |s: &[f32]| rms(&s.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        let strike_in = edge(&l[start..start + 96]);
        let strike_out = edge(&out[start..start + 96]);
        assert!(
            strike_out > strike_in * 1.2,
            "no edge: {strike_in} to {strike_out}"
        );
        let (_, tail_in) = strike_and_tail(&l, hits);
        let (_, tail_out) = strike_and_tail(&out, hits);
        assert!(
            (20.0 * (tail_out / tail_in).log10()).abs() < 1.0,
            "the tail moved"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = drums(0.4, 1);
        let l = &l[..2000];
        let edits = [(p::ATTACK, 60.0), (p::SUSTAIN, -40.0), (p::BRIGHT, 50.0)];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, l);
        let mut pieces = core_with(&edits);
        let mut b = l.to_vec();
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

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::ATTACK, 500.0);
        assert_eq!(core.settings().attack, 1.0);
        core.set_param(p::BRIGHT, -5.0);
        assert_eq!(core.settings().bright, 0.0);
        core.set_param(99, 1.0);
        core.set_param(p::ATTACK, 0.0);
        assert!(core.settings().is_rest());
    }
}
