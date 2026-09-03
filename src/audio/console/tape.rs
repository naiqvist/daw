//! TAPE: the first return — a tape delay.
//!
//! A line with a machine's faults in it: the head WOWS slowly and
//! FLUTTERS quickly, the loop rounds off at the top rather than
//! clipping so a hot feedback growls, a tone control dulls each pass,
//! and there is HISS under all of it. Every channel's first send feeds
//! this, and what comes back goes to the mix.
//!
//! The hiss RIDES WHAT THE TAPE IS CARRYING. A real machine hisses
//! whenever it rolls, but a return that hissed into a silent mix would
//! be a fault of ours rather than the machine's, so the floor follows
//! the loop: quick to arrive, slow to leave, gone when nothing has been
//! sent for a while.
//!
//! A return is never OUT — it is a place, not an effect — so there is
//! no mix knob: what arrives is what was sent, and what leaves is the
//! wet alone. The channel's send is the amount.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::filters::OnePole;
use crate::dsp::lfo::{Lfo, LfoShape};
use crate::dsp::noise::WhiteNoise;
use crate::params::console::tape as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub time_ms: f32,
    pub feedback: f32,
    pub wow: f32,
    pub flutter: f32,
    pub hiss: f32,
    pub tone: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Tape.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            time_ms: clamp(p::TIME),
            feedback: clamp(p::FEEDBACK) / 100.0,
            wow: clamp(p::WOW) / 100.0,
            flutter: clamp(p::FLUTTER) / 100.0,
            hiss: clamp(p::HISS) / 100.0,
            tone: clamp(p::TONE),
        }
    }
}

pub struct TapeCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    lines: [DelayLine; 2],
    bufs: [Vec<f32>; 2],
    tone: [OnePole; 2],
    wow: Lfo,
    flutter: Lfo,
    hiss: WhiteNoise,
    noise: Vec<f32>,
    fed: [f32; 2],
    /// How much the tape is carrying, for the hiss to ride.
    carrying: f32,
    hiss_rise: f32,
    hiss_fall: f32,
    max: usize,
    level_db: f32,
}

impl TapeCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let max = (sample_rate * p::MAX_MS / 1000.0).ceil() as usize + 4;
        let mut wow = Lfo::new();
        wow.prepare(sample_rate);
        wow.set_shape(LfoShape::Sine);
        wow.set_rate(p::WOW_HZ);
        let mut flutter = Lfo::new();
        flutter.prepare(sample_rate);
        flutter.set_shape(LfoShape::Sine);
        flutter.set_rate(p::FLUTTER_HZ);
        let mut hiss = WhiteNoise::new();
        hiss.seed(0x7461_7065);
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            lines: [DelayLine::new(), DelayLine::new()],
            bufs: [vec![0.0; buffer_len(max)], vec![0.0; buffer_len(max)]],
            tone: [OnePole::new(), OnePole::new()],
            wow,
            flutter,
            hiss,
            noise: vec![0.0; block.max(1)],
            fed: [0.0; 2],
            carrying: 0.0,
            hiss_rise: crate::dsp::ramps::one_pole_coeff(1000.0 / (p::HISS_RISE_MS * sample_rate)),
            hiss_fall: crate::dsp::ramps::one_pole_coeff(1000.0 / (p::HISS_FALL_MS * sample_rate)),
            max,
            level_db: -120.0,
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

    fn tune(&mut self) {
        let s = self.settings;
        for tone in &mut self.tone {
            tone.prepare(self.sample_rate, s.tone);
        }
    }

    #[inline(always)]
    fn soft(x: f32) -> f32 {
        let k = 1.0 / p::LOOP_DRIVE;
        (x * k).tanh() / k
    }
}

impl SectionCore for TapeCore {
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
        self.flutter.reset();
        self.hiss.reset();
        self.fed = [0.0; 2];
        self.carrying = 0.0;
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.noise.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        let per_ms = self.sample_rate / 1000.0;
        let base = (s.time_ms * per_ms).clamp(2.0, (self.max - 4) as f32);
        let hiss_gain = if s.hiss > 0.0 {
            10f32.powf(p::HISS_DB / 20.0) * s.hiss
        } else {
            0.0
        };
        if hiss_gain > 0.0 {
            self.hiss.process(&mut self.noise[..n]);
        }
        let mut slow = [0.0f32; 1];
        let mut fast = [0.0f32; 1];
        for i in 0..n {
            self.wow.process(&mut slow);
            self.flutter.process(&mut fast);
            let wander =
                1.0 + p::WOW_DEPTH * s.wow * slow[0] + p::FLUTTER_DEPTH * s.flutter * fast[0];
            let at = (base * wander).clamp(2.0, (self.max - 4) as f32);
            // The floor rides the loop: quick to arrive, slow to leave.
            let carried = l[i].abs().max(self.fed[0].abs().max(self.fed[1].abs()));
            let coeff = if carried > self.carrying {
                self.hiss_rise
            } else {
                self.hiss_fall
            };
            self.carrying += (carried - self.carrying) * coeff;
            let hiss = if hiss_gain > 0.0 {
                self.noise[i] * hiss_gain * (self.carrying * 40.0).min(1.0)
            } else {
                0.0
            };
            for side in 0..2 {
                if side == 1 && !stereo {
                    break;
                }
                let dry = if side == 0 { l[i] } else { r[i] };
                let mut wet = [dry + self.fed[side] * s.feedback];
                self.lines[side].set_delay(at);
                self.lines[side].process_modulated(&mut wet, &mut self.bufs[side], &[at]);
                let out = Self::soft(self.tone[side].tick_lowpass(wet[0])) + hiss;
                self.fed[side] = out;
                if side == 0 {
                    l[i] = out;
                } else {
                    r[i] = out;
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

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: [
                self.settings.time_ms,
                self.settings.feedback,
                self.settings.wow,
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

    fn core_with(edits: &[(u32, f32)]) -> TapeCore {
        let mut params = SectionParams::of(SectionKind::Tape);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        TapeCore::new(&params, FS, BLOCK)
    }

    fn run(core: &mut TapeCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn click(n: usize) -> Vec<f32> {
        let mut l = vec![0.0f32; n];
        l[0] = 1.0;
        l
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// What comes back is the wet alone: a return is a place, not an
    /// effect, so nothing dry leaves it.
    #[test]
    fn a_return_gives_back_only_its_own_sound() {
        let n = FS as usize / 4;
        let mut core = core_with(&[(p::TIME, 200.0), (p::HISS, 0.0)]);
        let out = run(&mut core, &click(n));
        assert!(
            out[..100].iter().all(|s| s.abs() < 1e-6),
            "the dry came back"
        );
        let step = (FS * 0.2) as usize;
        let at_repeat = out[step - 100..step + 100]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            at_repeat > 0.1,
            "no repeat at the time asked for: {at_repeat}"
        );
    }

    /// The repeats fade, and more feedback means more of them.
    #[test]
    fn the_repeats_fade_and_feedback_keeps_them() {
        let n = FS as usize * 2;
        let tail = |fb: f32| -> f32 {
            let mut core = core_with(&[(p::TIME, 100.0), (p::FEEDBACK, fb), (p::HISS, 0.0)]);
            let out = run(&mut core, &click(n));
            rms(&out[n - FS as usize / 2..])
        };
        let little = tail(20.0);
        let lots = tail(80.0);
        assert!(lots > little * 2.0, "little {little}, lots {lots}");
        // And it never runs away.
        let mut hot = core_with(&[(p::TIME, 100.0), (p::FEEDBACK, 100.0)]);
        let out = run(&mut hot, &click(n));
        assert!(out.iter().all(|s| s.abs() < 2.0), "the loop ran away");
    }

    /// Wow and flutter move the head: the repeats do not land in the
    /// same place as they would on a perfect machine.
    #[test]
    fn wow_and_flutter_move_the_head() {
        let n = FS as usize;
        let l = click(n);
        let mut still = core_with(&[
            (p::TIME, 200.0),
            (p::FEEDBACK, 60.0),
            (p::WOW, 0.0),
            (p::FLUTTER, 0.0),
            (p::HISS, 0.0),
        ]);
        let a = run(&mut still, &l);
        let mut moving = core_with(&[
            (p::TIME, 200.0),
            (p::FEEDBACK, 60.0),
            (p::WOW, 100.0),
            (p::FLUTTER, 100.0),
            (p::HISS, 0.0),
        ]);
        let b = run(&mut moving, &l);
        let apart = rms(&a.iter().zip(&b).map(|(x, y)| x - y).collect::<Vec<_>>());
        assert!(apart > 1e-3, "the head stood still: {apart}");
    }

    /// Hiss is a floor under the repeats — and only under them: a
    /// return with nothing sent to it is silent, however much hiss is
    /// asked for.
    #[test]
    fn the_hiss_rides_what_the_tape_carries() {
        let n = FS as usize / 2;
        let mut idle = core_with(&[(p::HISS, 100.0), (p::FEEDBACK, 40.0)]);
        let out = run(&mut idle, &vec![0.0f32; n]);
        assert!(rms(&out) < 1e-6, "an idle return hissed at {}", rms(&out));

        let mut rolling = core_with(&[(p::TIME, 100.0), (p::HISS, 100.0), (p::FEEDBACK, 70.0)]);
        let out = run(&mut rolling, &click(n));
        // Between the repeats, where the tape is running but nothing is
        // sounding, the floor is up.
        let step = (FS * 0.1) as usize;
        let between = rms(&out[step + step / 2..step * 2 - 200]);
        assert!(between > 1e-6, "no hiss under the repeats: {between}");
        assert!(
            between < 0.05,
            "the hiss is louder than the tape: {between}"
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = click(1000);
        let edits = [(p::TIME, 50.0), (p::FEEDBACK, 60.0)];
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
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::TIME, 99_999.0);
        assert_eq!(core.settings().time_ms, 1_500.0);
        core.set_param(p::FEEDBACK, -5.0);
        assert_eq!(core.settings().feedback, 0.0);
        core.set_param(99, 1.0);
    }
}
