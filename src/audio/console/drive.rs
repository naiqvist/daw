//! DRIVE: saturation with five characters.
//!
//! TUBE is the preamp's iron again — asymmetric, even harmonics, a
//! soft top. TAPE is the roundest knee there is and it dulls the top
//! as it is driven, the way oxide does. TRANSISTOR is symmetric with a
//! hard knee: odd harmonics, edge. FUZZ is a starved stage — biased,
//! then clipped flat — that gates and spits. FOLD is a wavefolder:
//! identity inside the rails, reflected outside, so a driven note
//! grows harmonics that are not in a clipper's vocabulary at all.
//!
//! Every curve has unit slope at zero, so the drive changes colour
//! before level; every curve runs at 2× through the shaper's
//! oversampler and out through a DC blocker. A TILT before the curve
//! chooses what is driven — tilt up and the top breaks first, tilt
//! down and the bottom does — and a TILT after it puts the balance
//! back or leans it. MIX blends against the dry; OUT trims. At DRIVE
//! zero with both tilts flat and OUT at zero the section is a wire to
//! the sample. The curves live green in `crate::console::drive_curve`
//! so the card draws what the core runs.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::drive_curve;
use crate::dsp::filters::{DcBlocker, OnePole, Tilt};
use crate::dsp::shaper::Oversampler2x;
use crate::params::console::drive as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub character: u32,
    /// 0..1.
    pub drive: f32,
    pub tilt_pre_db: f32,
    pub tilt_post_db: f32,
    /// 0..1.
    pub mix: f32,
    pub out_db: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Drive.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            character: (clamp(p::CHARACTER).round().max(0.0) as u32).min(p::FOLD),
            drive: clamp(p::DRIVE) / 100.0,
            tilt_pre_db: clamp(p::TILT_PRE),
            tilt_post_db: clamp(p::TILT_POST),
            mix: clamp(p::MIX) / 100.0,
            out_db: clamp(p::OUT),
        }
    }

    pub fn is_wire(&self) -> bool {
        (self.drive == 0.0 || self.mix == 0.0)
            && self.tilt_pre_db == 0.0
            && self.tilt_post_db == 0.0
            && self.out_db == 0.0
    }
}

/// The character's curve at drive `k`.
#[inline(always)]
fn curve(character: u32, k: f32, x: f32) -> f32 {
    match character {
        p::TUBE => drive_curve::tube(x, k),
        p::TAPE => drive_curve::tape(x, k),
        p::TRANSISTOR => drive_curve::transistor(x, k),
        p::FUZZ => drive_curve::fuzz(x, k),
        _ => drive_curve::fold(x, k),
    }
}

pub struct DriveCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    pre: [Tilt; 2],
    post: [Tilt; 2],
    top: [OnePole; 2],
    /// The pivot's own one-pole, per channel, run over the OUTPUT: what
    /// it passes is the bottom, so what it does not is the top share.
    share: [OnePole; 2],
    over: [Oversampler2x; 2],
    dc: [DcBlocker; 2],
    lane: Vec<f32>,
    dry: Vec<f32>,
    k: f32,
    out: f32,
    level_db: f32,
    heat: f32,
    /// The held INPUT peak, linear, decayed a block at a time.
    input_peak: f32,
    input_db: f32,
    dirt: f32,
    top_share: f32,
}

impl DriveCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            pre: [Tilt::new(), Tilt::new()],
            post: [Tilt::new(), Tilt::new()],
            top: [OnePole::new(), OnePole::new()],
            share: [OnePole::new(), OnePole::new()],
            over: [Oversampler2x::new(), Oversampler2x::new()],
            dc: [DcBlocker::new(), DcBlocker::new()],
            lane: vec![0.0; Oversampler2x::scratch_len(block.max(1))],
            dry: vec![0.0; block.max(1)],
            k: 1.0,
            out: 1.0,
            level_db: p::SILENT_DB,
            heat: 0.0,
            input_peak: 0.0,
            input_db: p::INPUT_FLOOR_DB,
            dirt: 0.0,
            top_share: 0.0,
        };
        for ch in 0..2 {
            core.over[ch].prepare();
            core.dc[ch].prepare(sample_rate);
            core.share[ch].prepare(sample_rate, p::TILT_HZ);
        }
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        let fs = self.sample_rate;
        self.k = drive_curve::drive_of(s.character, s.drive);
        self.out = 10f32.powf(s.out_db / 20.0);
        let top_hz = p::TAPE_TOP_HZ + (p::TAPE_TOP_DRIVEN_HZ - p::TAPE_TOP_HZ) * s.drive;
        for ch in 0..2 {
            self.pre[ch].prepare(fs, p::TILT_HZ, s.tilt_pre_db);
            self.post[ch].prepare(fs, p::TILT_HZ, s.tilt_post_db);
            self.top[ch].prepare(fs, top_hz);
        }
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let s = self.settings;
        let wet = s.drive > 0.0 && s.mix > 0.0;
        if s.tilt_pre_db != 0.0 {
            self.pre[ch].process(io);
        }
        if wet {
            let dry = &mut self.dry[..n];
            dry.copy_from_slice(io);
            let lane = &mut self.lane[..n * 2];
            self.over[ch].up(io, lane);
            let (character, k) = (s.character, self.k);
            let mut hottest = 0.0f32;
            for x in lane.iter_mut() {
                hottest = hottest.max((*x * k).abs());
                *x = curve(character, k, *x);
            }
            self.over[ch].down(lane, io);
            self.heat = self.heat.max((hottest / p::HEAT_FULL).min(1.0));
            if s.character == p::TAPE {
                self.top[ch].process_lowpass(io);
            }
            self.dc[ch].process(io);
            if s.mix < 1.0 {
                for (y, x) in io.iter_mut().zip(dry.iter()) {
                    *y = *x + (*y - *x) * s.mix;
                }
            }
            // The dirt, measured here and nowhere later: what the
            // shaper and the mix added, against the dry they were
            // handed, with neither the post-tilt nor OUT in the way.
            let (mut added, mut clean) = (0.0f32, 0.0f32);
            for (y, x) in io.iter().zip(dry.iter()) {
                let d = *y - *x;
                added += d * d;
                clean += *x * *x;
            }
            let scale = 1.0 / n as f32;
            let ratio = (added * scale).sqrt() / (clean * scale).sqrt().max(p::DIRT_FLOOR);
            self.dirt = self.dirt.max(ratio.min(1.0));
        }
        if s.tilt_post_db != 0.0 {
            self.post[ch].process(io);
        }
        if self.out != 1.0 {
            for y in io.iter_mut() {
                *y *= self.out;
            }
        }
    }

    /// One channel of OUTPUT, folded into the three figures the meters
    /// want: the peak, the block's energy, and the energy the pivot's
    /// one-pole passed — the bottom. Whatever it did not pass is top.
    fn scan(&mut self, ch: usize, io: &[f32]) -> (f32, f32, f32) {
        let (mut peak, mut all, mut low) = (0.0f32, 0.0f32, 0.0f32);
        for y in io {
            peak = peak.max(y.abs());
            all += *y * *y;
            let under = self.share[ch].tick_lowpass(*y);
            low += under * under;
        }
        (peak, all, low)
    }
}

impl SectionCore for DriveCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.pre[ch].reset();
            self.post[ch].reset();
            self.top[ch].reset();
            self.share[ch].reset();
            self.over[ch].reset();
            self.dc[ch].reset();
        }
        self.level_db = p::SILENT_DB;
        self.heat = 0.0;
        self.input_peak = 0.0;
        self.input_db = p::INPUT_FLOOR_DB;
        self.dirt = 0.0;
        self.top_share = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.dry.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= p::READOUT_DECAY;
        self.dirt *= p::READOUT_DECAY;
        self.top_share *= p::READOUT_DECAY;
        self.input_peak *= p::READOUT_DECAY;
        // The input, taken before the pre-tilt: this is what arrived.
        let heard = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.input_peak = self.input_peak.max(heard);
        let held = self.input_peak.max(p::ENERGY_FLOOR);
        self.input_db = (20.0 * held.log10()).max(p::INPUT_FLOOR_DB);
        if !self.settings.is_wire() {
            self.run(0, l);
            if stereo {
                self.run(1, &mut r[..n]);
            }
        }
        let (mut peak, mut all, mut low) = self.scan(0, l);
        if stereo {
            let (other, energy, under) = self.scan(1, &r[..n]);
            peak = peak.max(other);
            all += energy;
            low += under;
        }
        self.level_db = if peak <= p::SILENT_PEAK {
            p::SILENT_DB
        } else {
            20.0 * peak.log10()
        };
        let top = ((all - low) / all.max(p::ENERGY_FLOOR)).clamp(0.0, 1.0);
        self.top_share = self.top_share.max(top);
    }

    /// What the press shows. Every figure here is MEASURED from the
    /// block just processed — none of it is a setting, because the card
    /// already has every setting from the parameter table.
    ///
    /// - `level_db`: the OUTPUT peak of the block over both channels, in
    ///   dBFS, after mix, post-tilt and OUT. `SILENT_DB` (-120) for a
    ///   silent block. No hold: it is that block's own peak.
    /// - `reduction_db`: minus the HEAT, so 0.0 at rest down to -1.0 flat
    ///   out. Heat is dimensionless, not dB: how far up its curve the
    ///   hottest sample of the block went, `max|x*k| / HEAT_FULL`
    ///   clamped to 1, so it is the press working and not a gain
    ///   reduction. Held (see below). Exactly 0 while the section is a
    ///   wire.
    /// - `bands[0]`: the INPUT peak over both channels, in dBFS, taken
    ///   at the section's input BEFORE the pre-tilt and the curve — so
    ///   neither DRIVE nor OUT moves it. Floored at `INPUT_FLOOR_DB`
    ///   (-72) and free to run above 0 for an input over full scale.
    ///   Held on the LINEAR peak, so the reading falls about 1.9 dB a
    ///   block once the sound stops.
    /// - `bands[1]`: DIRT, 0.0..1.0 — `rms(mixed - dry) / rms(dry)`
    ///   taken inside the shaper's own stage, after the mix and before
    ///   the post-tilt and OUT, so it is the distortion the section is
    ///   really adding and no part of it is the trim. Exactly 0.0 while
    ///   the section is a wire, and proportional to MIX otherwise.
    ///   Clamped to 1.0. Held.
    /// - `bands[2]`: TOP SHARE, 0.0..1.0 — the share of the OUTPUT
    ///   block's energy above `TILT_HZ`, as `1 - low/all` through one
    ///   pole at the pivot. A 250 Hz note reads near 0.06, a 6 kHz one
    ///   near 0.97, and driving a low note upward moves it because the
    ///   harmonics are real. 0.0 on silence. Held.
    ///
    /// The hold on the four held figures is a peak-hold multiplied by
    /// `READOUT_DECAY` (0.8) per block and then re-maxed against the new
    /// block: a tenth survives ten blocks (about 53 ms at 48 kHz and a
    /// block of 256), so each reads as a meter and never as a spike.
    /// There is no wall clock and nothing here smooths over time in
    /// seconds — the decay is per block by design, as everywhere on the
    /// desk.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: -self.heat,
            bands: [self.input_db, self.dirt, self.top_share],
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

    fn core_with(edits: &[(u32, f32)]) -> DriveCore {
        let mut params = SectionParams::of(SectionKind::Drive);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        DriveCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut DriveCore, l: &[f32]) -> Vec<f32> {
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

    fn harmonic_db(signal: &[f32], hz: f32, h: u32) -> f32 {
        let bin = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in signal.iter().enumerate() {
                let w = 2.0 * core::f32::consts::PI * f * i as f32 / FS;
                re += s * w.cos();
                im -= s * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        20.0 * (bin(hz * h as f32) / bin(hz).max(1e-9)).log10()
    }

    /// The second and third harmonics of a 100 Hz note at 0.4 through
    /// `character` at `drive` percent, once settled.
    fn harmonics(character: f32, drive: f32) -> (f32, f32) {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 480 * 20;
        let l = sine(100.0, 0.4, n);
        let mut core = core_with(&[(p::CHARACTER, character), (p::DRIVE, drive)]);
        let out = run(&mut core, &l);
        (
            harmonic_db(&out[window.clone()], 100.0, 2),
            harmonic_db(&out[window], 100.0, 3),
        )
    }

    #[test]
    fn the_floor_is_a_wire_to_the_sample() {
        let mut core = core_with(&[(p::CHARACTER, 3.0), (p::MIX, 100.0), (p::DRIVE, 0.0)]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
        let mut dry = core_with(&[(p::DRIVE, 80.0), (p::MIX, 0.0)]);
        assert!(dry.settings().is_wire(), "mix at zero is dry");
    }

    /// Each character has its own harmonics: the tube is even, the
    /// transistor odd, tape is the softest, fuzz the hardest, and the
    /// fold's spectrum is not a clipper's.
    #[test]
    fn five_characters_five_spectra() {
        let (tube2, tube3) = harmonics(0.0, 50.0);
        assert!(tube2 > tube3, "tube: second {tube2}, third {tube3}");
        assert!(tube2 > -40.0);

        let (trans2, trans3) = harmonics(2.0, 70.0);
        assert!(
            trans3 > trans2 + 10.0,
            "transistor: second {trans2}, third {trans3}"
        );

        let (_, tape3) = harmonics(1.0, 70.0);
        assert!(
            tape3 < trans3,
            "tape ({tape3}) is not softer than the transistor ({trans3})"
        );
        assert!(tape3 > -60.0, "tape does nothing");

        let (_, fuzz3) = harmonics(3.0, 70.0);
        assert!(
            fuzz3 > trans3,
            "fuzz ({fuzz3}) is not harder than the transistor ({trans3})"
        );

        // The fold at full drive: the fundamental itself gives way — a
        // clipper's never does — so the third stands over it.
        let (_, fold3) = harmonics(4.0, 100.0);
        assert!(fold3 > -6.0, "the fold did not fold: third at {fold3} dB");
    }

    /// Tape dulls the top as it is driven; the tube does not.
    #[test]
    fn tape_softens_the_top_when_driven() {
        let n = FS as usize / 4;
        let l = sine(12_000.0, 0.05, n);
        let mut tape = core_with(&[(p::CHARACTER, 1.0), (p::DRIVE, 100.0)]);
        let out = run(&mut tape, &l);
        let dulled = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!(dulled < -4.0, "tape kept its top: {dulled} dB");
        let mut tube = core_with(&[(p::CHARACTER, 0.0), (p::DRIVE, 100.0)]);
        let out = run(&mut tube, &l);
        let kept = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!(kept > -1.5, "the tube lost its top: {kept} dB");
    }

    /// The tilt before the curve chooses what is driven: tilted down,
    /// a low note distorts more than a high one; tilted up, the other
    /// way round.
    #[test]
    fn the_pre_tilt_chooses_what_is_driven() {
        let third_at = |hz: f32, tilt: f32| -> f32 {
            let n = FS as usize / 2;
            let period = (FS / hz).round() as usize;
            let window = n / 2..n / 2 + period * 20;
            let l = sine(hz, 0.3, n);
            let mut core = core_with(&[(p::CHARACTER, 2.0), (p::DRIVE, 50.0), (p::TILT_PRE, tilt)]);
            let out = run(&mut core, &l);
            harmonic_db(&out[window], hz, 3)
        };
        let low_down = third_at(100.0, -6.0);
        let low_up = third_at(100.0, 6.0);
        assert!(low_down > low_up + 6.0, "tilt down {low_down}, up {low_up}");
        let high_down = third_at(2_000.0, -6.0);
        let high_up = third_at(2_000.0, 6.0);
        assert!(
            high_up > high_down + 6.0,
            "tilt up {high_up}, down {high_down}"
        );
    }

    /// MIX blends against the dry; OUT is a gain after everything.
    #[test]
    fn mix_blends_and_out_trims() {
        let n = FS as usize / 4;
        let l = sine(100.0, 0.4, n);
        let mut full = core_with(&[(p::CHARACTER, 3.0), (p::DRIVE, 100.0)]);
        let full_out = run(&mut full, &l);
        let mut half = core_with(&[(p::CHARACTER, 3.0), (p::DRIVE, 100.0), (p::MIX, 50.0)]);
        let half_out = run(&mut half, &l);
        let wet_diff = rms(&full_out[n / 2..]
            .iter()
            .zip(&l[n / 2..])
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>());
        let half_diff = rms(&half_out[n / 2..]
            .iter()
            .zip(&l[n / 2..])
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>());
        assert!(
            (half_diff / wet_diff - 0.5).abs() < 0.1,
            "half mix is {} of full",
            half_diff / wet_diff
        );

        let mut trimmed = core_with(&[(p::OUT, -6.0), (p::DRIVE, 0.0)]);
        let out = run(&mut trimmed, &l);
        let change = 20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10();
        assert!((change + 6.0).abs() < 0.1);
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [
            (p::CHARACTER, 0.0),
            (p::DRIVE, 60.0),
            (p::TILT_PRE, 3.0),
            (p::TILT_POST, -2.0),
            (p::MIX, 80.0),
        ];
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
        core.set_param(p::CHARACTER, 9.0);
        assert_eq!(core.settings().character, p::FOLD);
        core.set_param(p::DRIVE, 500.0);
        assert_eq!(core.settings().drive, 1.0);
        core.set_param(p::TILT_PRE, 40.0);
        assert_eq!(core.settings().tilt_pre_db, 6.0);
        core.set_param(99, 1.0);
        core.set_param(p::DRIVE, 0.0);
        core.set_param(p::TILT_PRE, 0.0);
        assert!(core.settings().is_wire());
    }
}
