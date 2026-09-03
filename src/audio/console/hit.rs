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
    /// What the BLOCK did, for the card: the heaviest strike and tail
    /// weights anywhere in it, the edge BRIGHT actually added as a share
    /// of the output, and the most the gain moved. Block maxima, not
    /// last samples — a strike is one or two milliseconds and a block is
    /// five, so the last sample lands on a strike about one block in
    /// three and a card fed it would stutter through a whole take.
    strike_now: f32,
    tail_now: f32,
    edge_now: f32,
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
            edge_now: 0.0,
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
    /// the strike's weight. Returns the LARGEST edge it added anywhere
    /// in the block, in linear amplitude — the card's only witness that
    /// BRIGHT did something, since the edge is buried inside the sound
    /// it was added to.
    fn brighten(&mut self, ch: usize, io: &mut [f32]) -> f32 {
        let n = io.len();
        let top = &mut self.top[..n];
        top.copy_from_slice(io);
        self.edge[ch].process_highpass(top);
        let amount = p::BRIGHT_AMOUNT * self.settings.bright;
        let mut most_edge = 0.0f32;
        for ((y, t), w) in io.iter_mut().zip(top.iter()).zip(&self.weight[..n]) {
            let added = t * amount * w;
            *y += added;
            most_edge = most_edge.max(added.abs());
        }
        most_edge
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
        self.edge_now = 0.0;
        self.moved_db = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.key.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        // At REST the section is still a wire — no sample below is
        // written — but it still LISTENS. The strike and the tail are
        // properties of the incoming sound, not of the levers, and the
        // card draws them; freezing them the moment all three levers sit
        // at zero would kill the blow, the ring and the wedge exactly
        // while the user is auditioning the section flat and deciding
        // whether to reach for a lever at all.
        let rest = s.is_rest();

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
            let tail = if self.long > p::LEVEL_FLOOR {
                ((self.long - self.quick) / self.long).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let strike = self.weight[i];
            // BLOCK MAXIMA, both of them. The strike is a millisecond or
            // two inside a five-millisecond block, so the last sample of
            // a block is a coin toss; the max is the honest reading. The
            // tail rises monotonically through a decay, so its max is the
            // honest reading too.
            strike_now = strike_now.max(strike);
            tail_now = tail_now.max(tail);
            if !rest {
                let db = p::RANGE_DB * (s.attack * strike + s.sustain * tail);
                let gain = 10f32.powf(db / 20.0);
                l[i] *= gain;
                if stereo {
                    r[i] *= gain;
                }
                if db.abs() > most_moved.abs() {
                    most_moved = db;
                }
            }
        }
        let mut most_edge = 0.0f32;
        if !rest && s.bright > 0.0 {
            most_edge = most_edge.max(self.brighten(0, l));
            if stereo {
                most_edge = most_edge.max(self.brighten(1, &mut r[..n]));
            }
        }
        self.strike_now = strike_now;
        self.tail_now = tail_now;
        self.moved_db = most_moved;
        self.level_db = peak_db(l, if stereo { &r[..n] } else { &[] });
        // The edge as a SHARE of what came out: an absolute amplitude
        // would make the rasp flare with the take's level, and HIT's one
        // promise is that its picture does not grow when the sound gets
        // louder. Divided by the block's own peak, the share depends on
        // the BRIGHT setting and the strike's weight and on nothing else.
        let out_peak = 10f32.powf(self.level_db / 20.0).max(p::LEVEL_FLOOR);
        self.edge_now = (most_edge / out_peak).clamp(0.0, 1.0);
    }

    /// A pure copy of what `process` measured on the last block. Every
    /// figure is a per-block statistic with NO smoothing of its own —
    /// the card does its own easing — so each one's time constant is one
    /// block, over the ballistics named below.
    ///
    /// - `level_db`: the loudest sample OUT this block, in dBFS,
    ///   −120 (silence) to 0 and above. Block peak, no release.
    /// - `reduction_db`: the most the gain moved this block, in dB,
    ///   signed and in −`RANGE_DB`..=+`RANGE_DB` (±12): positive is a
    ///   LIFT, negative a cut. The signed block maximum by magnitude.
    ///   Exactly 0.0 when the section is at rest.
    /// - `bands[0]` — THE BLOW: the heaviest strike weight anywhere in
    ///   the block, 0..1, dimensionless. `TransientSplit`'s gap between
    ///   a fast and a slow envelope over the fast one, so it is a share,
    ///   independent of level: a ghost note and a rimshot read the same.
    ///   Its ballistics are the split's — a sub-millisecond fast attack
    ///   against a `WINDOW_MS` (20 ms) slow one — so it snaps to ~1 on
    ///   an onset and is back near 0 within the window. Live at rest.
    /// - `bands[1]` — THE TAIL: the heaviest tail weight anywhere in the
    ///   block, 0..1, dimensionless. How far a `TAIL_QUICK_MS` (60 ms)
    ///   peak follower has fallen below a `TAIL_LONG_MS` (800 ms) one,
    ///   as a share of the long one. 0 on a fresh strike, rising as the
    ///   note decays, collapsing to 0 on the next strike. Also
    ///   level-independent, and also live at rest.
    /// - `bands[2]` — THE EDGE: the largest sample BRIGHT added anywhere
    ///   in the block, as a linear share of the block's output peak,
    ///   0..1. Exactly 0.0 when BRIGHT is 0 and exactly 0.0 at rest;
    ///   otherwise it rises with the BRIGHT setting AND with the
    ///   strike's weight, because the edge is the high-passed copy
    ///   scaled by both. No release of its own beyond the strike
    ///   weight's.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            // The most the gain moved this block, signed: a lift reads
            // positive here, which the card shows as an arrow up.
            reduction_db: self.moved_db,
            bands: [self.strike_now, self.tail_now, self.edge_now],
        }
    }
}

fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= p::LEVEL_FLOOR {
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

    /// A take run block by block, with what the card would have been
    /// handed after every block: the output, then one readout per block.
    fn watch(core: &mut HitCore, l: &[f32]) -> (Vec<f32>, Vec<Readout>) {
        let mut out = l.to_vec();
        let mut said = Vec::new();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
            said.push(core.readout());
        }
        (out, said)
    }

    fn peak_band(said: &[Readout], band: usize) -> f32 {
        said.iter().fold(0.0f32, |most, s| most.max(s.bands[band]))
    }

    /// `drums` strikes every 400 ms, which at 48 kHz over a 256-sample
    /// block is exactly 75 blocks — so hit `k` lands on the first sample
    /// of block `75 * k` and the block indices below are exact.
    const BLOCKS_PER_HIT: usize = 75;

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

    /// THE BLOW is the block's LOUDEST strike, not its last sample. A
    /// strike is one or two milliseconds and a block is five, so the
    /// last sample of a block lands on the strike about one block in
    /// three: measured here, the last sample is far below the block's
    /// own peak on several blocks of a two-hit take, and the band
    /// reports the peak on every one of them.
    #[test]
    fn the_blow_is_the_blocks_loudest_strike_not_its_last_sample() {
        let l = drums(0.5, 2);
        let mut core = core_with(&[(p::ATTACK, 100.0)]);
        let mut out = l.clone();
        let (mut struck, mut last_sample_lied) = (0, 0);
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
            let n = end - start;
            let band = core.readout().bands[0];
            let most = core.weight[..n].iter().fold(0.0f32, |a, w| a.max(*w));
            let last = core.weight[n - 1];
            assert!(
                (band - most).abs() < 1e-6,
                "bands[0] {band} is not the block max {most}"
            );
            assert!((0.0..=1.0).contains(&band), "bands[0] {band} left 0..1");
            if band > 0.5 {
                struck += 1;
            }
            // The gap the old last-sample reading would have opened:
            // measured at up to 0.24 of full scale on this take.
            if band > last + 0.15 {
                last_sample_lied += 1;
            }
        }
        assert!(
            struck >= 2,
            "only {struck} blocks saw a strike over two hits"
        );
        assert!(
            last_sample_lied >= 4,
            "the last sample never lied ({last_sample_lied} blocks), \
             so this take cannot tell max from last"
        );
    }

    /// THE BLOW and THE RING are live at the section's DEFAULTS, where
    /// the section is a wire. They are properties of the sound arriving,
    /// not of the levers, and the card draws them while the user is
    /// auditioning the section flat. What DOES report rest at the
    /// defaults: the move column and the file's flare, both exactly
    /// zero, and every sample untouched.
    #[test]
    fn at_defaults_the_blow_and_the_ring_are_live_and_the_rest_reports_rest() {
        let l = drums(0.5, 3);
        let mut core = core_with(&[]);
        let (out, said) = watch(&mut core, &l);
        assert_eq!(out, l, "the section at rest changed a sample");
        let blow = peak_band(&said, 0);
        let ring = peak_band(&said, 1);
        assert!(blow > 0.5, "the blow read only {blow} at the defaults");
        assert!(ring > 0.5, "the ring read only {ring} at the defaults");
        for s in &said {
            assert_eq!(s.reduction_db, 0.0, "the desk moved at rest");
            assert_eq!(s.bands[2], 0.0, "the file flared at rest");
        }

        // And on silence every band is at rest, so a stopped transport
        // draws a still card rather than a frozen one.
        let mut quiet = core_with(&[]);
        let (_, said) = watch(&mut quiet, &vec![0.0; BLOCK * 8]);
        for s in &said {
            assert_eq!(s.bands, [0.0, 0.0, 0.0]);
            assert_eq!(s.level_db, -120.0);
        }
    }

    /// THE RING fills through a decay and empties on the next strike:
    /// bands[1] is near nothing one block after each hit and near full
    /// by the block before the next one.
    #[test]
    fn the_ring_fills_through_a_decay_and_empties_on_the_next_strike() {
        let l = drums(0.5, 4);
        let mut core = core_with(&[(p::SUSTAIN, 100.0)]);
        let (_, said) = watch(&mut core, &l);
        for hit in 1..3 {
            let after = said[hit * BLOCKS_PER_HIT + 1].bands[1];
            let before_next = said[(hit + 1) * BLOCKS_PER_HIT - 1].bands[1];
            assert!(after < 0.15, "hit {hit} left the wedge {after} full");
            assert!(before_next > 0.6, "hit {hit} decayed to only {before_next}");
            assert!(
                (0.0..=1.0).contains(&before_next),
                "bands[1] {before_next} left 0..1"
            );
        }
    }

    /// THE EDGE carries what BRIGHT actually added: exactly nothing at
    /// BRIGHT 0, more at 100 than at 50, and only where the strike is.
    #[test]
    fn the_edge_rises_with_bright_and_with_the_strike() {
        let l = drums(0.5, 3);

        let mut flat = core_with(&[]);
        let (_, said) = watch(&mut flat, &l);
        assert_eq!(peak_band(&said, 2), 0.0, "the file has no teeth at 0");

        let mut half = core_with(&[(p::BRIGHT, 50.0)]);
        let (_, said) = watch(&mut half, &l);
        let at_half = peak_band(&said, 2);

        let mut full = core_with(&[(p::BRIGHT, 100.0)]);
        let (_, said) = watch(&mut full, &l);
        let at_full = peak_band(&said, 2);
        assert!(at_half > 0.01, "BRIGHT 50 added nothing: {at_half}");
        assert!(
            at_full > at_half * 1.5,
            "BRIGHT 100 ({at_full}) barely beat 50 ({at_half})"
        );
        assert!(at_full <= 1.0, "bands[2] {at_full} left 0..1");

        // And it follows the STRIKE, not the sound: the block on a hit
        // flares, the block a third of a second into the decay does not.
        let on_hit = said[2 * BLOCKS_PER_HIT].bands[2];
        let in_tail = said[2 * BLOCKS_PER_HIT + 60].bands[2];
        assert!(
            on_hit > in_tail * 10.0,
            "the edge did not follow the strike: {on_hit} on the hit, \
             {in_tail} in the tail"
        );
    }

    /// The edge is a SHARE, so it obeys the section's one promise: a
    /// ghost note and a rimshot flare the file by the same amount.
    #[test]
    fn the_edge_does_not_grow_with_the_take() {
        let flare_at = |amp: f32| -> f32 {
            let mut core = core_with(&[(p::BRIGHT, 100.0)]);
            let (_, said) = watch(&mut core, &drums(amp, 3));
            peak_band(&said, 2)
        };
        let quiet = flare_at(0.05);
        let loud = flare_at(0.5);
        assert!(
            (quiet - loud).abs() < 0.1 * loud.max(1e-3),
            "quiet {quiet}, loud {loud}"
        );
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
