//! ROOM: the reverb, two ways.
//!
//! ROOM is a Schroeder/Freeverb — parallel damped combs into series
//! allpasses — which is the small, dense, slightly boxy sound that
//! suits a drum machine. HALL is a feedback delay network of eight
//! modulated lines through a lossless matrix, which is the long,
//! smooth, moving sound that suits everything else. The two are
//! different machines rather than one machine with a size knob,
//! because that is what they are.
//!
//! PREDELAY is a plain line before either, and it is what makes a
//! reverb sit behind a sound instead of on it. SIZE is the room's
//! dimension and the hall's decay; DAMP closes the top as the tail
//! goes, the way air and soft rooms do. MIX at zero is a wire to the
//! sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::fdn::Fdn;
use crate::dsp::reverb::Reverb;
use crate::params::console::room as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub hall: bool,
    pub predelay_ms: f32,
    /// 0..1.
    pub size: f32,
    pub damp: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Room.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            hall: clamp(p::ALGO).round() as u32 == p::HALL,
            predelay_ms: clamp(p::PREDELAY),
            size: clamp(p::SIZE) / 100.0,
            damp: clamp(p::DAMP) / 100.0,
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }
}

pub struct RoomCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    room: Reverb,
    room_buf: Vec<f32>,
    hall: Fdn,
    hall_buf: Vec<f32>,
    pre: [DelayLine; 2],
    pre_buf: [Vec<f32>; 2],
    /// Compile-owned scratch: the mono send, and the two wet returns.
    send: Vec<f32>,
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
    level_db: f32,
}

impl RoomCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let pre_max = (sample_rate * p::MAX_PREDELAY_MS / 1000.0).ceil() as usize + 4;
        let mut core = Self {
            params: params.clone(),
            settings: Settings::of(params),
            sample_rate,
            room: Reverb::new(),
            room_buf: vec![0.0; Reverb::buffer_len(sample_rate)],
            hall: Fdn::new(),
            hall_buf: vec![0.0; Fdn::buffer_len(sample_rate)],
            pre: [DelayLine::new(), DelayLine::new()],
            pre_buf: [
                vec![0.0; buffer_len(pre_max)],
                vec![0.0; buffer_len(pre_max)],
            ],
            send: vec![0.0; block.max(1)],
            wet_l: vec![0.0; block.max(1)],
            wet_r: vec![0.0; block.max(1)],
            level_db: -120.0,
        };
        core.room.prepare(sample_rate, &mut core.room_buf);
        core.hall.prepare(sample_rate, &mut core.hall_buf);
        for line in &mut core.pre {
            line.prepare(pre_max);
        }
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        let damp_hz = p::DAMP_OPEN_HZ * (p::DAMP_SHUT_HZ / p::DAMP_OPEN_HZ).powf(s.damp);
        let decay = p::ROOM_DECAY_LOW + (p::ROOM_DECAY_HIGH - p::ROOM_DECAY_LOW) * s.size;
        self.room.set_room(s.size, decay, s.damp);
        self.hall.set_size(s.size);
        self.hall
            .set_decay(p::HALL_SHORT_S + (p::HALL_LONG_S - p::HALL_SHORT_S) * s.size);
        self.hall.set_damping(damp_hz);
        self.hall.set_diffusion(p::HALL_DIFFUSION);
        self.hall.set_modulation(p::HALL_MODULATION);
        let pre = (s.predelay_ms * self.sample_rate / 1000.0).max(1.0);
        for line in &mut self.pre {
            line.set_delay(pre);
        }
    }
}

impl SectionCore for RoomCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.room.reset(&mut self.room_buf);
        self.hall.reset(&mut self.hall_buf);
        for (line, buf) in self.pre.iter_mut().zip(self.pre_buf.iter_mut()) {
            line.reset();
            buf.fill(0.0);
        }
        self.level_db = -120.0;
    }

    fn latency(&self) -> usize {
        0
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.send.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        if !s.is_wire() {
            // The send: mono into the tail, which is what both of these
            // machines take.
            let send = &mut self.send[..n];
            for i in 0..n {
                send[i] = if stereo { (l[i] + r[i]) * 0.5 } else { l[i] };
            }
            if s.predelay_ms > 0.5 {
                self.pre[0].process_smooth(send, &mut self.pre_buf[0]);
            }
            let wet_l = &mut self.wet_l[..n];
            let wet_r = &mut self.wet_r[..n];
            if s.hall {
                self.hall.process(send, wet_l, wet_r, &mut self.hall_buf);
            } else {
                self.room.process(send, wet_l, &mut self.room_buf);
                wet_r.copy_from_slice(wet_l);
            }
            for i in 0..n {
                l[i] += (wet_l[i] - l[i]) * s.mix;
                if stereo {
                    r[i] += (wet_r[i] - r[i]) * s.mix;
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
                if self.settings.hall { 1.0 } else { 0.0 },
                self.settings.size,
                self.settings.mix,
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

    fn core_with(edits: &[(u32, f32)]) -> RoomCore {
        let mut params = SectionParams::of(SectionKind::Room);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        RoomCore::new(&params, FS, BLOCK)
    }

    fn run(core: &mut RoomCore, l: &[f32]) -> Vec<f32> {
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

    /// How long the tail takes to fall to a thousandth of its loudest,
    /// in samples.
    fn tail_length(out: &[f32]) -> usize {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let floor = peak * 0.001;
        out.iter().rposition(|s| s.abs() > floor).unwrap_or(0)
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l: Vec<f32> = (0..len).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Both machines make a tail, and both tails outlast the click.
    #[test]
    fn both_algorithms_ring() {
        let n = FS as usize * 2;
        for algo in [0.0, 1.0] {
            let mut core = core_with(&[(p::ALGO, algo), (p::MIX, 100.0), (p::SIZE, 70.0)]);
            let out = run(&mut core, &click(n));
            let tail = tail_length(&out);
            assert!(
                tail > FS as usize / 4,
                "algo {algo} rang for {tail} samples"
            );
            assert!(out.iter().all(|s| s.abs() < 4.0), "algo {algo} ran away");
        }
    }

    /// A bigger size rings longer, on both machines.
    #[test]
    fn size_makes_the_tail_longer() {
        let n = FS as usize * 3;
        for algo in [0.0, 1.0] {
            let mut small = core_with(&[(p::ALGO, algo), (p::MIX, 100.0), (p::SIZE, 15.0)]);
            let mut large = core_with(&[(p::ALGO, algo), (p::MIX, 100.0), (p::SIZE, 95.0)]);
            let short = tail_length(&run(&mut small, &click(n)));
            let long = tail_length(&run(&mut large, &click(n)));
            assert!(
                long > short + FS as usize / 10,
                "algo {algo}: {short} then {long}"
            );
        }
    }

    /// Damping takes the top out of the tail.
    #[test]
    fn damping_dulls_the_tail() {
        let n = FS as usize;
        let edge = |damp: f32| -> f32 {
            let mut core = core_with(&[(p::MIX, 100.0), (p::SIZE, 70.0), (p::DAMP, damp)]);
            let out = run(&mut core, &click(n));
            let tail = &out[FS as usize / 4..FS as usize / 2];
            let slew: f32 = tail.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
            slew / rms(tail).max(1e-9)
        };
        let open = edge(0.0);
        let shut = edge(100.0);
        assert!(
            shut < open * 0.8,
            "damping kept the top: {open} then {shut}"
        );
    }

    /// The pre-delay holds the tail back.
    #[test]
    fn the_predelay_holds_the_tail_back() {
        let n = FS as usize / 2;
        let onset = |ms: f32| -> usize {
            let mut core = core_with(&[(p::MIX, 100.0), (p::SIZE, 60.0), (p::PREDELAY, ms)]);
            let out = run(&mut core, &click(n));
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            out.iter().position(|s| s.abs() > peak * 0.05).unwrap_or(0)
        };
        let none = onset(0.0);
        let held = onset(100.0);
        let want = (FS * 0.1) as usize;
        assert!(
            held > none + want / 2,
            "the pre-delay did not hold: {none} then {held}"
        );
    }

    /// The hall is stereo; the room is one tail in both sides.
    #[test]
    fn the_hall_is_wide_and_the_room_is_not() {
        let n = FS as usize / 2;
        let spread = |algo: f32| -> f32 {
            let mut core = core_with(&[(p::ALGO, algo), (p::MIX, 100.0), (p::SIZE, 70.0)]);
            let (mut ol, mut or) = (click(n), vec![0.0f32; n]);
            for start in (0..n).step_by(BLOCK) {
                let end = (start + BLOCK).min(n);
                core.process(&mut ol[start..end], &mut or[start..end], &clock());
            }
            let from = FS as usize / 8;
            rms(&ol[from..]
                .iter()
                .zip(&or[from..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>())
        };
        assert!(spread(0.0) < 1e-4, "the room came out wide");
        assert!(spread(1.0) > 1e-3, "the hall came out mono");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = click(1000);
        let edits = [(p::MIX, 60.0), (p::SIZE, 50.0), (p::PREDELAY, 10.0)];
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
        core.set_param(p::SIZE, 500.0);
        assert_eq!(core.settings().size, 1.0);
        core.set_param(p::ALGO, 1.0);
        assert!(core.settings().hall);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }
}
