//! SHADOW: the second return — the long reverb.
//!
//! A feedback delay network of eight modulated lines through a
//! lossless matrix: the smooth, wide, slowly moving tail that a
//! channel's own ROOM cannot be, because a channel's reverb has to be
//! short enough not to swallow the track. Every channel's second send
//! feeds this, and what comes back goes to the mix.
//!
//! Like the other return it is never OUT and has no mix: what leaves
//! is the tail alone, and the channel's send is the amount. PREDELAY
//! holds it back, SIZE is its decay, DAMP closes its top as it goes.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::fdn::Fdn;
use crate::params::console::shadow as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub predelay_ms: f32,
    /// 0..1.
    pub size: f32,
    pub damp: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Shadow.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            predelay_ms: clamp(p::PREDELAY),
            size: clamp(p::SIZE) / 100.0,
            damp: clamp(p::DAMP) / 100.0,
        }
    }
}

pub struct ShadowCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    fdn: Fdn,
    fdn_buf: Vec<f32>,
    pre: DelayLine,
    pre_buf: Vec<f32>,
    send: Vec<f32>,
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
    level_db: f32,
}

impl ShadowCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let pre_max = (sample_rate * p::MAX_PREDELAY_MS / 1000.0).ceil() as usize + 4;
        let mut core = Self {
            params: params.dense(),
            settings: Settings::of(params),
            sample_rate,
            fdn: Fdn::new(),
            fdn_buf: vec![0.0; Fdn::buffer_len(sample_rate)],
            pre: DelayLine::new(),
            pre_buf: vec![0.0; buffer_len(pre_max)],
            send: vec![0.0; block.max(1)],
            wet_l: vec![0.0; block.max(1)],
            wet_r: vec![0.0; block.max(1)],
            level_db: -120.0,
        };
        core.fdn.prepare(sample_rate, &mut core.fdn_buf);
        core.pre.prepare(pre_max);
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn tune(&mut self) {
        let s = self.settings;
        self.fdn.set_size(s.size);
        self.fdn
            .set_decay(p::SHORT_S + (p::LONG_S - p::SHORT_S) * s.size);
        self.fdn
            .set_damping(p::DAMP_OPEN_HZ * (p::DAMP_SHUT_HZ / p::DAMP_OPEN_HZ).powf(s.damp));
        self.fdn.set_diffusion(p::DIFFUSION);
        self.fdn.set_modulation(p::MODULATION);
        self.pre
            .set_delay((s.predelay_ms * self.sample_rate / 1000.0).max(1.0));
    }
}

impl SectionCore for ShadowCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.fdn.reset(&mut self.fdn_buf);
        self.pre.reset();
        self.pre_buf.fill(0.0);
        self.level_db = -120.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.send.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        let send = &mut self.send[..n];
        for i in 0..n {
            send[i] = if stereo { (l[i] + r[i]) * 0.5 } else { l[i] };
        }
        if s.predelay_ms > 0.5 {
            self.pre.process_smooth(send, &mut self.pre_buf);
        }
        let wet_l = &mut self.wet_l[..n];
        let wet_r = &mut self.wet_r[..n];
        self.fdn.process(send, wet_l, wet_r, &mut self.fdn_buf);
        l[..n].copy_from_slice(wet_l);
        if stereo {
            r[..n].copy_from_slice(wet_r);
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
                self.settings.size,
                self.settings.damp,
                self.settings.predelay_ms,
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

    fn core_with(edits: &[(u32, f32)]) -> ShadowCore {
        let mut params = SectionParams::of(SectionKind::Shadow);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        ShadowCore::new(&params, FS, BLOCK)
    }

    fn run(core: &mut ShadowCore, l: &[f32]) -> Vec<f32> {
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

    fn tail_length(out: &[f32]) -> usize {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let floor = peak * 0.001;
        out.iter().rposition(|s| s.abs() > floor).unwrap_or(0)
    }

    /// A long tail, and only the tail: nothing dry comes back.
    #[test]
    fn it_gives_back_a_long_tail_and_nothing_dry() {
        let n = FS as usize * 3;
        let mut core = core_with(&[(p::SIZE, 80.0), (p::PREDELAY, 20.0)]);
        let out = run(&mut core, &click(n));
        assert!(
            out[..800].iter().all(|s| s.abs() < 1e-3),
            "the dry came back"
        );
        let tail = tail_length(&out);
        assert!(tail > FS as usize, "it rang for only {tail} samples");
        assert!(out.iter().all(|s| s.abs() < 4.0), "it ran away");
    }

    /// Size lengthens the tail; damping dulls it.
    #[test]
    fn size_lengthens_and_damping_dulls() {
        let n = FS as usize * 4;
        let short = tail_length(&run(&mut core_with(&[(p::SIZE, 10.0)]), &click(n)));
        let long = tail_length(&run(&mut core_with(&[(p::SIZE, 100.0)]), &click(n)));
        assert!(long > short + FS as usize / 2, "{short} then {long}");

        let edge = |damp: f32| -> f32 {
            let out = run(
                &mut core_with(&[(p::SIZE, 70.0), (p::DAMP, damp)]),
                &click(n),
            );
            let tail = &out[FS as usize / 2..FS as usize];
            tail.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(tail).max(1e-9)
        };
        assert!(edge(100.0) < edge(0.0) * 0.8, "damping kept the top");
    }

    /// The tail is wide: the two sides are not the same signal.
    #[test]
    fn the_tail_is_wide() {
        let n = FS as usize;
        let (mut ol, mut or) = (click(n), vec![0.0f32; n]);
        let mut core = core_with(&[(p::SIZE, 70.0)]);
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut ol[start..end], &mut or[start..end], &clock());
        }
        let from = FS as usize / 8;
        let apart = rms(&ol[from..]
            .iter()
            .zip(&or[from..])
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>());
        assert!(apart > 1e-3, "the tail came out mono: {apart}");
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = click(1000);
        let edits = [(p::SIZE, 60.0), (p::PREDELAY, 10.0)];
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
        core.set_param(99, 1.0);
    }
}
