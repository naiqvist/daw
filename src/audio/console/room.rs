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
use crate::dsp::ramps::one_pole_coeff;
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
    /// The meters' fall, per sample.
    fall: f32,
    /// The send's peak and the wet return's peak, both linear and both
    /// held with that fall, and the wet return's slew ratio held the
    /// same way. Measured in `process`; see `readout`.
    send_held: f32,
    wet_held: f32,
    bright_held: f32,
    /// The wet return's last sample of the previous block, so the slew
    /// sum does not restart at every block boundary.
    wet_prev: f32,
    /// What `readout` copies, and the only thing it does.
    bands: [f32; 3],
}

/// A meter that rises within the block and falls with `keep`, so a
/// transient is never missed and a tail is never a flicker. Snapped to
/// zero at the quiet floor so the fall cannot trail into denormals.
fn hold(slot: &mut f32, value: f32, keep: f32) {
    *slot = if value > *slot { value } else { *slot * keep };
    if *slot < p::READOUT_QUIET {
        *slot = 0.0;
    }
}

/// A linear peak as a band: `READOUT_FLOOR_DB` reads 0.0, full scale
/// reads 1.0, and everything between is straight dB.
fn band_of(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - p::READOUT_FLOOR_DB) / -p::READOUT_FLOOR_DB).clamp(0.0, 1.0)
}

impl RoomCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let pre_max = (sample_rate * p::MAX_PREDELAY_MS / 1000.0).ceil() as usize + 4;
        let mut core = Self {
            params: params.dense(),
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
            fall: one_pole_coeff(1000.0 / (p::READOUT_FALL_MS * sample_rate)),
            send_held: 0.0,
            wet_held: 0.0,
            bright_held: 0.0,
            wet_prev: 0.0,
            bands: [0.0; 3],
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
        self.send_held = 0.0;
        self.wet_held = 0.0;
        self.bright_held = 0.0;
        self.wet_prev = 0.0;
        self.bands = [0.0; 3];
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
            // The three bands ride the crossfade's own loop: the send
            // AFTER the pre-delay line, so it says when the room was
            // actually struck, and the wet return BEFORE the crossfade,
            // so the tail is seen whole at any mix.
            let mut send_peak = 0.0f32;
            let mut wet_peak = 0.0f32;
            let mut slew = 0.0f32;
            let mut swing = 0.0f32;
            let mut prev = self.wet_prev;
            for i in 0..n {
                send_peak = send_peak.max(send[i].abs());
                wet_peak = wet_peak.max(wet_l[i].abs()).max(wet_r[i].abs());
                slew += (wet_l[i] - prev).abs();
                swing += wet_l[i].abs();
                prev = wet_l[i];
                l[i] += (wet_l[i] - l[i]) * s.mix;
                if stereo {
                    r[i] += (wet_r[i] - r[i]) * s.mix;
                }
            }
            self.wet_prev = prev;
            // The fall is per SAMPLE, so a block's worth of it is the
            // coefficient raised to the block's length: how the meter
            // reads must not depend on how the sound was cut up.
            let keep = (1.0 - self.fall).powi(n as i32);
            hold(&mut self.send_held, send_peak, keep);
            hold(&mut self.wet_held, wet_peak, keep);
            // Under the quiet floor the ratio is noise over noise, so
            // the band falls rather than reading it.
            let bright = if swing > p::READOUT_QUIET * n as f32 {
                (slew / (std::f32::consts::PI * swing)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            hold(&mut self.bright_held, bright, keep);
            self.bands = [
                band_of(self.send_held),
                band_of(self.wet_held),
                self.bright_held,
            ];
        } else {
            // A wire measures nothing, and that is right: the core does
            // nothing and the face draws a silent room.
            self.send_held = 0.0;
            self.wet_held = 0.0;
            self.bright_held = 0.0;
            self.wet_prev = 0.0;
            self.bands = [0.0; 3];
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

    /// What the room says it did, field by field. A pure copy of
    /// fields: every peak, hold and log10 happens in `process`, never
    /// here.
    ///
    /// - `level_db`: the loudest sample of the section's OUTPUT this
    ///   block — dry and wet, after the crossfade — in dBFS, floored at
    ///   -120. The block's extreme, unsmoothed, as everywhere on the
    ///   desk.
    /// - `reduction_db`: always 0. A reverb takes no gain.
    /// - `bands[0]` SEND: the peak of the mono send AFTER the pre-delay
    ///   line — the level actually striking the room — as 0..1 over a
    ///   60 dB window, `READOUT_FLOOR_DB` (-60 dBFS) reading 0.0 and
    ///   full scale 1.0. Rises within the block it happened in, falls
    ///   with a one-pole of `READOUT_FALL_MS` (300 ms) on the held
    ///   linear peak, which is about 29 dB per second. Raising PREDELAY
    ///   visibly DELAYS this band, because the send is measured on the
    ///   far side of the line.
    /// - `bands[1]` TAIL: the peak of `max(|wet_l|, |wet_r|)` taken
    ///   BEFORE the mix crossfade, on the same 0..1 60 dB window with
    ///   the same 300 ms fall. Because it is the return and not the
    ///   output, it keeps reading while the room rings after the source
    ///   has stopped, and reads the same at MIX 1 as at MIX 100.
    /// - `bands[2]` BRIGHT: the wet return's slew ratio over the block,
    ///   `sum|wet_l[i] - wet_l[i-1]| / (PI * sum|wet_l[i]|)`, clamped to
    ///   0..1 — dimensionless, 0.0 at DC and 1.0 at Nyquist, so it is
    ///   the tail's colour and falls as DAMP closes the top. Held with
    ///   the same 300 ms fall; 0.0 while the return is under
    ///   `READOUT_QUIET`, where the ratio would be noise over noise.
    ///   It is the same measure `damping_dulls_the_tail` asserts on, so
    ///   what DAMP does to the sound and what the face draws are one
    ///   number.
    ///
    /// All three bands are exactly 0.0 while `is_wire()`.
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

    /// The same run as `run`, but keeping what the meters said at the
    /// end of every block.
    fn meter(core: &mut RoomCore, l: &[f32]) -> Vec<Readout> {
        let mut out = l.to_vec();
        let mut said = Vec::new();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
            said.push(core.readout());
        }
        said
    }

    /// What the meters said at the block holding sample `sample`.
    fn at(said: &[Readout], sample: usize) -> Readout {
        said[(sample / BLOCK).min(said.len() - 1)]
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

    /// At the DEFAULTS the section is a wire, and a wire has nothing to
    /// say: all three bands read exactly rest, however loud the sound
    /// passing through it.
    #[test]
    fn the_bands_rest_at_the_defaults() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        let n = FS as usize / 2;
        let loud: Vec<f32> = (0..n).map(|i| (i as f32 * 0.31).sin() * 0.9).collect();
        for said in meter(&mut core, &loud) {
            assert_eq!(said.bands, [0.0; 3], "a wire reported a room");
            assert_eq!(said.reduction_db, 0.0);
        }
    }

    /// bands[0] is the send AFTER the pre-delay line, so it says when
    /// the room was STRUCK, not when the sample played: a 150 ms
    /// pre-delay moves the strike 150 ms later.
    #[test]
    fn the_send_band_arrives_with_the_predelay() {
        let n = FS as usize / 2;
        let struck = |ms: f32| -> usize {
            let mut core = core_with(&[(p::MIX, 100.0), (p::SIZE, 60.0), (p::PREDELAY, ms)]);
            let said = meter(&mut core, &click(n));
            said.iter()
                .position(|m| m.bands[0] > 0.9)
                .unwrap_or(usize::MAX)
        };
        let none = struck(0.0);
        let held = struck(150.0);
        let want = (FS * 0.15) as usize / BLOCK;
        assert_eq!(none, 0, "the send band missed the strike");
        assert!(
            held.abs_diff(want) <= 2,
            "the send band did not walk with the pre-delay: block {held}, wanted {want}"
        );
    }

    /// bands[1] is the WET RETURN taken before the crossfade, so the
    /// room goes on visibly ringing after the source has stopped — and
    /// says the same thing at a mix of 4 as at 100, which is the one
    /// thing a reverb's face has to show.
    #[test]
    fn the_tail_band_outlives_the_source_and_ignores_the_mix() {
        let n = FS as usize * 3;
        let watch = |mix: f32| -> (Readout, Readout, Readout) {
            let mut core = core_with(&[
                (p::ALGO, 1.0),
                (p::MIX, mix),
                (p::SIZE, 85.0),
                (p::DAMP, 20.0),
            ]);
            let said = meter(&mut core, &click(n));
            (
                at(&said, FS as usize / 10),
                at(&said, FS as usize),
                at(&said, FS as usize * 2),
            )
        };
        let (early, late, last) = watch(100.0);
        assert!(early.bands[1] > 0.3, "the tail band missed the strike");
        assert!(late.bands[1] > 0.1, "the tail band died with the source");
        assert!(
            late.bands[1] < early.bands[1],
            "the tail band never fell: {} then {}",
            early.bands[1],
            late.bands[1]
        );
        // Two seconds after a click the source is long gone but the
        // hall is not: the tail band is now ABOVE the send band, which
        // is the whole reason they are two different bands.
        assert!(
            last.bands[1] > last.bands[0],
            "the tail did not outlive the send: send {} tail {}",
            last.bands[0],
            last.bands[1]
        );
        // The wet return is measured before the crossfade, so turning
        // the mix down moves the SOUND and leaves the telemetry alone.
        let (quiet_early, quiet_late, quiet_last) = watch(4.0);
        assert_eq!(
            (quiet_early.bands[1], quiet_late.bands[1]),
            (early.bands[1], late.bands[1]),
            "the mix moved the tail band"
        );
        assert!(
            quiet_last.level_db < late.level_db - 20.0,
            "at mix 4 the output should be far quieter: {} then {}",
            late.level_db,
            quiet_last.level_db
        );
    }

    /// bands[2] is the wet return's slew ratio — the same measure
    /// `damping_dulls_the_tail` asserts on — so what DAMP does to the
    /// sound and what the face draws are one number, measured over the
    /// same part of the tail.
    #[test]
    fn the_bright_band_falls_with_damping() {
        let n = FS as usize;
        let bright = |damp: f32| -> f32 {
            let mut core = core_with(&[(p::MIX, 100.0), (p::SIZE, 70.0), (p::DAMP, damp)]);
            let said = meter(&mut core, &click(n));
            at(&said, FS as usize / 3).bands[2]
        };
        let open = bright(0.0);
        let shut = bright(100.0);
        assert!(open > 0.2, "the bright band never lit: {open}");
        assert!(
            shut < open * 0.8,
            "damping did not dull the band: {open} then {shut}"
        );
    }

    /// The bands are a meter, not a latch. When the room falls silent
    /// they walk back to rest — under the 0.02 the face gates its own
    /// motion on, and exactly zero on the two level bands, which reach
    /// their floor first.
    #[test]
    fn the_bands_return_to_rest_when_the_room_falls_silent() {
        let n = FS as usize * 3;
        let mut core = core_with(&[(p::MIX, 100.0), (p::SIZE, 10.0), (p::DAMP, 90.0)]);
        let said = meter(&mut core, &click(n));
        let struck = said.iter().fold(0.0f32, |m, s| m.max(s.bands[1]));
        assert!(struck > 0.5, "the room was never struck: {struck}");
        let rest = at(&said, n - 1);
        assert_eq!(rest.bands[0], 0.0, "the send band never came to rest");
        assert_eq!(rest.bands[1], 0.0, "the tail band never came to rest");
        assert!(
            rest.bands[2] < 0.02,
            "the bright band never came to rest: {}",
            rest.bands[2]
        );
    }
}
