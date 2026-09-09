//! Node-side shared voice ownership and effects. Sources remain distinct
//! physical algorithms in the dependency-free SoA kernel.
#![deny(clippy::unwrap_used, clippy::expect_used)]
use crate::audio::graph::Ramp;
use crate::dsp::coverage_physical::{Bank, Kind, PARAMS, VOICES, formants, registration, spectrum};
use crate::dsp::delay::{DelayLine, buffer_len};
use crate::dsp::dynamics::{
    Ballistics, GainComputer, LookaheadLimiter, Mode, RmsDetector, SlewBrighten,
};
use crate::dsp::fdn::Fdn;
use crate::dsp::filters::{DcBlocker, OnePole, Tilt};
use crate::dsp::shaper::{Mode as Shape, Oversampler2x, Waveshaper};
use crate::pages::{Hero, HeroMark, HeroSeries, KeyTable};
use crate::params::ParamDef;
const CHUNK: usize = 32;

/// The actual low-band Weight transfer, shared with its picture.
fn weight_transfer(x: f32, p: &[f32; PARAMS]) -> f32 {
    let amount = p[24];
    let drive = 1.0 + amount * 7.0;
    let odd = (x * drive).tanh() / drive;
    let biased = ((x * drive + p[27] * p[25]).tanh() - (p[27] * p[25]).tanh()) / drive;
    x + ((odd * (1.0 - p[25]) + biased * p[25]) - x) * p[28] * amount * 2.0
}

pub struct Instrument {
    kind: Kind,
    table: &'static [ParamDef],
    sr: f32,
    bank: Bank,
    base: [f32; PARAMS],
    live: [f32; PARAMS],
    locked: [bool; PARAMS],
    patches: [[f32; VOICES]; PARAMS],
    voice_locks: [[bool; VOICES]; PARAMS],
    right: Vec<f32>,
    chunk_left: usize,
    tail: usize,
    fdn: Fdn,
    /// Prepared silent state, including the deterministic modulation phases.
    /// Fdn::reset deliberately keeps its phase for legacy transport callers.
    fdn_initial: Fdn,
    body_store: Vec<f32>,
    shine: [SlewBrighten; 2],
    low: [OnePole; 2],
    tilt: [Tilt; 2],
    dc: [DcBlocker; 2],
    oversample: [Oversampler2x; 2],
    delays: [DelayLine; 5],
    delay_store: [Vec<f32>; 5],
    phase: [f32; 5],
    rotor: [f32; 2],
    limiter: LookaheadLimiter,
    limit_store: [Vec<f32>; 3],
    dry_limit: [DelayLine; 2],
    dry_store: [Vec<f32>; 2],
    rms: RmsDetector,
    computer: GainComputer,
    ballistics: Ballistics,
    shaper: Waveshaper,
    prepared: [f32; PARAMS],
    follower: [f32; 2],
    last_lane: Option<usize>,
}
impl Instrument {
    pub fn new(kind: Kind, table: &'static [ParamDef]) -> Self {
        Self {
            kind,
            table,
            sr: 48000.,
            bank: Bank::new(kind),
            base: [0.; PARAMS],
            live: [0.; PARAMS],
            locked: [false; PARAMS],
            patches: [[0.; VOICES]; PARAMS],
            voice_locks: [[false; VOICES]; PARAMS],
            right: Vec::new(),
            chunk_left: 0,
            tail: 0,
            fdn: Fdn::new(),
            fdn_initial: Fdn::new(),
            body_store: Vec::new(),
            shine: [SlewBrighten::new(); 2],
            low: [OnePole::new(); 2],
            tilt: [Tilt::new(); 2],
            dc: [DcBlocker::new(); 2],
            oversample: [Oversampler2x::new(); 2],
            delays: [DelayLine::new(); 5],
            delay_store: core::array::from_fn(|_| Vec::new()),
            phase: [0., 0.2, 0.4, 0.6, 0.8],
            rotor: [0.; 2],
            limiter: LookaheadLimiter::new(),
            limit_store: core::array::from_fn(|_| Vec::new()),
            dry_limit: [DelayLine::new(); 2],
            dry_store: core::array::from_fn(|_| Vec::new()),
            rms: RmsDetector::new(),
            computer: GainComputer::new(),
            ballistics: Ballistics::new(),
            shaper: Waveshaper::new(),
            prepared: [f32::NAN; PARAMS],
            follower: [0.; 2],
            last_lane: None,
        }
    }
    pub fn prepare(&mut self, sr: f32, max_block: usize, params: [f32; PARAMS]) {
        self.sr = if sr.is_finite() {
            sr.clamp(8000., 192000.)
        } else {
            48000.
        };
        for (i, d) in self.table.iter().enumerate() {
            if let Some(x) = self.live.get_mut(i) {
                *x = if params[i].is_finite() {
                    params[i].clamp(d.min, d.max)
                } else {
                    d.default
                };
            }
        }
        self.base = self.live;
        self.right.resize(max_block.max(CHUNK), 0.);
        self.bank.prepare(self.sr);
        self.body_store.resize(Fdn::buffer_len(self.sr), 0.);
        self.fdn.prepare(self.sr, &mut self.body_store);
        self.fdn_initial = self.fdn;
        let max_delay = (self.sr * 0.1).ceil() as usize;
        for (d, store) in self.delays.iter_mut().zip(&mut self.delay_store) {
            d.prepare(max_delay);
            store.resize(buffer_len(max_delay), 0.);
        }
        let limit_size = LookaheadLimiter::scratch_len(self.sr, 2.);
        for s in &mut self.limit_store {
            s.resize(limit_size, 0.);
        }
        self.limiter.prepare(self.sr, 2., 120.);
        let look = self.limiter.latency().max(1);
        for (d, s) in self.dry_limit.iter_mut().zip(&mut self.dry_store) {
            d.prepare(look);
            d.set_delay(look as f32);
            s.resize(buffer_len(look), 0.);
        }
        for o in &mut self.oversample {
            o.prepare();
        }
        for d in &mut self.dc {
            d.prepare(self.sr);
        }
        self.rms.prepare(self.sr, 15.);
        self.prepared = [f32::NAN; PARAMS];
        self.configure_fx();
        self.reset();
    }
    pub fn reset(&mut self) {
        self.bank.reset();
        self.right.fill(0.);
        self.locked.fill(false);
        self.live = self.base;
        self.chunk_left = 0;
        self.tail = 0;
        self.follower = [0.; 2];
        self.last_lane = None;
        self.phase = [0., 0.2, 0.4, 0.6, 0.8];
        self.rotor = [0.; 2];
        self.fdn = self.fdn_initial;
        self.fdn.reset(&mut self.body_store);
        for s in &mut self.delay_store {
            s.fill(0.);
        }
        for s in &mut self.limit_store {
            s.fill(0.);
        }
        for s in &mut self.dry_store {
            s.fill(0.);
        }
        for d in &mut self.delays {
            d.reset();
        }
        for d in &mut self.dry_limit {
            d.reset();
        }
        for d in &mut self.oversample {
            d.reset();
        }
        for d in &mut self.dc {
            d.reset();
        }
        for d in &mut self.low {
            d.reset();
        }
        for d in &mut self.tilt {
            d.reset();
        }
        for d in &mut self.shine {
            d.reset();
        }
        self.limiter.reset();
        self.rms.reset();
        self.ballistics.reset();
        // The restored FDN snapshot has its prepared tuning, not necessarily
        // the current base patch. Reapply every FX coefficient after reset.
        self.prepared = [f32::NAN; PARAMS];
        self.configure_fx();
    }
    fn clamp(&self, id: u32, value: f32) -> Option<f32> {
        self.table.get(id as usize).map(|d| {
            if value.is_finite() {
                value.clamp(d.min, d.max)
            } else {
                d.default
            }
        })
    }
    pub fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.clamp(id, value) else {
            return;
        };
        let i = id as usize;
        if i >= PARAMS {
            return;
        }
        self.base[i] = v;
        if !self.locked[i] {
            self.live[i] = v;
        }
        for lane in 0..VOICES {
            if !self.voice_locks[i][lane] {
                self.patches[i][lane] = v;
                if self.bank.lane_active(lane) {
                    let p = core::array::from_fn(|n| self.patches[n][lane]);
                    self.bank.set(lane, &p);
                }
            }
        }
    }
    pub fn plock(&mut self, id: u32, value: Option<f32>) {
        let i = id as usize;
        if i >= PARAMS {
            return;
        }
        if let Some(v) = value.and_then(|v| self.clamp(id, v)) {
            self.live[i] = v;
            self.locked[i] = true;
        } else {
            self.live[i] = self.base[i];
            self.locked[i] = false;
        }
    }
    pub fn plock_glide(&mut self, id: u32, alpha: f32) {
        let i = id as usize;
        if i < PARAMS && alpha.is_finite() {
            self.live[i] += (self.base[i] - self.live[i]) * alpha.clamp(0., 1.);
            if let Some(lane) = self.last_lane {
                if self.bank.lane_active(lane) && self.voice_locks[i][lane] {
                    self.patches[i][lane] = self.live[i];
                    let p = core::array::from_fn(|n| self.patches[n][lane]);
                    self.bank.set(lane, &p);
                }
            }
        }
    }
    pub fn note_on(&mut self, pitch: u8, vel: f32, age: u64) {
        if !vel.is_finite() || vel <= 0. {
            self.note_off(pitch);
            return;
        }
        let count = self.live[23].round().clamp(1., 16.) as usize;
        let lane = self.bank.choose(count);
        self.last_lane = Some(lane);
        let legato = self.kind == Kind::Mass && count == 1 && self.live[15] > 0.5;
        for i in 0..PARAMS {
            self.patches[i][lane] = self.live[i];
            self.voice_locks[i][lane] = self.locked[i];
        }
        self.bank
            .trigger(lane, pitch, vel.clamp(0., 1.), age, &self.live, legato);
        self.tail = (self.sr * 8.) as usize;
    }
    pub fn note_off(&mut self, pitch: u8) {
        self.bank.note_off(pitch);
    }
    pub fn release_all(&mut self) {
        self.bank.release_all();
    }
    pub fn active(&self) -> bool {
        self.bank.active() || self.tail > 0
    }
    pub fn latency_for(kind: Kind, sr: f32) -> usize {
        let os = Oversampler2x::new().latency();
        if kind == Kind::Mass {
            let mut l = LookaheadLimiter::new();
            l.prepare(sr, 2., 120.);
            os + l.latency()
        } else {
            os
        }
    }
    pub fn latency(&self) -> usize {
        Self::latency_for(self.kind, self.sr)
    }
    pub fn right(&self, len: usize) -> &[f32] {
        self.right.get(..len).unwrap_or(&[])
    }
    fn configure_fx(&mut self) {
        let p = self.live;
        if self.prepared == p {
            return;
        }
        self.prepared = p;
        match self.kind {
            Kind::Mass => {
                for lo in &mut self.low {
                    lo.prepare(self.sr * 2., p[26]);
                }
                self.limiter.set_ceiling_db(p[30]);
                self.limiter.set_release_ms(self.sr, p[31]);
                self.computer.configure(Mode::Compress, p[33], 2., 6.);
                self.ballistics.prepare(self.sr, p[34], p[31]);
            }
            Kind::Pluck => {
                self.fdn.set_size(0.35 + p[24] * 1.4);
                self.fdn.set_decay(p[25]);
                self.fdn.set_damping(p[26]);
                self.fdn.set_diffusion(p[28]);
                self.fdn.set_modulation(p[29] * 8.);
                for s in &mut self.shine {
                    s.prepare(self.sr, p[31], p[30]);
                }
                for t in &mut self.tilt {
                    t.prepare(self.sr, p[31], p[35]);
                }
            }
            Kind::Vox => {}
            Kind::Pipe => {
                for lo in &mut self.low {
                    lo.prepare(self.sr, 800.);
                }
                for t in &mut self.tilt {
                    t.prepare(self.sr * 2., p[28], p[26]);
                }
                self.shaper
                    .configure(Shape::SoftClip, 1. + p[24] * 8., p[25] * 0.4, p[27]);
            }
        }
    }
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        let mut done = 0;
        while done < out.len() {
            if self.chunk_left == 0 {
                self.configure_fx();
                self.chunk_left = CHUNK;
            }
            let n = (out.len() - done).min(self.chunk_left).min(CHUNK);
            let mut lanes = [[0.; VOICES]; CHUNK];
            self.bank.process(&mut lanes[..n]);
            let mut l = [0.; CHUNK];
            let mut r = [0.; CHUNK];
            for i in 0..n {
                for lane in 0..VOICES {
                    let spread = self.patches[22][lane];
                    let pan = (lane as f32 / 15. - 0.5) * spread;
                    let x = lanes[i][lane];
                    l[i] += x * (0.5 - pan * 0.5).sqrt();
                    r[i] += x * (0.5 + pan * 0.5).sqrt();
                }
            }
            self.effects(&mut l[..n], &mut r[..n]);
            for i in 0..n {
                let g = gain.next();
                if let Some(x) = out.get_mut(done + i) {
                    *x = l[i] * g;
                }
                if let Some(x) = self.right.get_mut(at + done + i) {
                    *x = r[i] * g;
                }
            }
            self.chunk_left -= n;
            done += n;
            if self.bank.active() {
                self.tail = (self.sr * 8.) as usize;
            } else {
                self.tail = self.tail.saturating_sub(n);
            }
        }
    }
    fn effects(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let p = self.live;
        // Nonlinear branches run at 2x; the neutral path retains the same
        // reconstruction filter and fixed latency at every mix setting.
        for (ch, io) in [l as &mut [f32], r as &mut [f32]].into_iter().enumerate() {
            let mut up = [0.; CHUNK * 2];
            self.oversample[ch].up(io, &mut up[..n * 2]);
            if self.kind == Kind::Mass {
                for x in &mut up[..n * 2] {
                    let low = self.low[ch].tick_lowpass(*x);
                    *x += weight_transfer(low, &p) - low;
                }
            } else if self.kind == Kind::Pipe {
                self.tilt[ch].process(&mut up[..n * 2]);
                self.shaper.process(&mut up[..n * 2]);
                let zero = self.shaper.shape(0.0);
                for x in &mut up[..n * 2] {
                    *x -= zero;
                }
            }
            self.oversample[ch].down(&up[..n * 2], io);
            self.dc[ch].process(io);
        }
        match self.kind {
            Kind::Mass => {
                let mut dry_l = [0.; CHUNK];
                let mut dry_r = [0.; CHUNK];
                for i in 0..n {
                    let env = self.rms.tick((l[i] + r[i]) * 0.5);
                    let gd = self.computer.gain_db(20. * env.max(1e-8).log10());
                    let db = self.ballistics.tick(gd) * p[32];
                    let g = 10.0_f32.powf(db / 20.);
                    l[i] *= g;
                    r[i] *= g;
                    dry_l[i] = l[i];
                    dry_r[i] = r[i];
                }
                self.dry_limit[0].process_exact(&mut dry_l[..n], &mut self.dry_store[0]);
                self.dry_limit[1].process_exact(&mut dry_r[..n], &mut self.dry_store[1]);
                let [key, sl, sr] = &mut self.limit_store;
                self.limiter.process_linked(l, r, key, sl, sr);
                for i in 0..n {
                    l[i] = dry_l[i] + (l[i] - dry_l[i]) * p[35];
                    r[i] = dry_r[i] + (r[i] - dry_r[i]) * p[35];
                }
            }
            Kind::Pluck => {
                let follower_coeff =
                    [p[33], p[34]].map(|ms| 1.0 - (-1.0 / (ms * 0.001 * self.sr).max(1.0)).exp());
                let mut input = [0.; CHUNK];
                for i in 0..n {
                    input[i] = (l[i] + r[i]) * 0.5;
                }
                let mut bl = [0.; CHUNK];
                let mut br = [0.; CHUNK];
                self.fdn.process(
                    &input[..n],
                    &mut bl[..n],
                    &mut br[..n],
                    &mut self.body_store,
                );
                for (ch, io) in [l as &mut [f32], r as &mut [f32]].into_iter().enumerate() {
                    let mut bright = [0.; CHUNK];
                    bright[..n].copy_from_slice(io);
                    self.shine[ch].process(&mut bright[..n]);
                    self.tilt[ch].process(&mut bright[..n]);
                    for i in 0..n {
                        let level = io[i].abs();
                        let coefficient = follower_coeff[usize::from(level <= self.follower[ch])];
                        self.follower[ch] += (level - self.follower[ch]) * coefficient;
                        let contour = (self.follower[ch] * 8.).clamp(0., 1.);
                        io[i] += (bright[i] - io[i]) * p[32] * contour;
                        io[i] += if ch == 0 {
                            bl[i] * p[27]
                        } else {
                            br[i] * p[27]
                        };
                    }
                }
            }
            Kind::Vox => {
                let count = p[30].round().clamp(1., 3.) as usize;
                let mut send = [0.; CHUNK];
                for i in 0..n {
                    send[i] = (l[i] + r[i]) * 0.5;
                }
                for voice in 0..count {
                    let rate = p[32] * (1. + voice as f32 * 0.073);
                    let depth = (p[31] / 1200. * core::f32::consts::LN_2 * self.sr
                        / (core::f32::consts::TAU * rate.max(0.05)))
                    .min(self.sr * 0.018);
                    let base = p[35] * 0.001 * self.sr + depth;
                    let mut wet = send;
                    let mut times = [0.; CHUNK];
                    for time in &mut times[..n] {
                        self.phase[voice] = (self.phase[voice] + rate / self.sr).fract();
                        *time = base + depth * (core::f32::consts::TAU * self.phase[voice]).sin();
                    }
                    self.delays[voice].process_modulated(
                        &mut wet[..n],
                        &mut self.delay_store[voice],
                        &times[..n],
                    );
                    let pan = if count == 1 {
                        0.
                    } else {
                        (voice as f32 / (count - 1) as f32 - 0.5) * p[34]
                    };
                    for i in 0..n {
                        l[i] += wet[i] * p[33] * (0.5 - pan) / count as f32;
                        r[i] += wet[i] * p[33] * (0.5 + pan) / count as f32;
                    }
                }
            }
            Kind::Pipe => {
                let mut lo_l = [0.; CHUNK];
                let mut lo_r = [0.; CHUNK];
                lo_l[..n].copy_from_slice(l);
                lo_r[..n].copy_from_slice(r);
                self.low[0].process_lowpass(&mut lo_l[..n]);
                self.low[1].process_lowpass(&mut lo_r[..n]);
                let mut horn = [0.; CHUNK];
                let mut drum = [0.; CHUNK];
                for i in 0..n {
                    drum[i] = (lo_l[i] + lo_r[i]) * 0.5;
                    horn[i] = (l[i] + r[i]) * 0.5 - drum[i];
                }
                let mut delayed = [horn, drum];
                let mut amplitudes = [[0.; CHUNK]; 2];
                for rotor in 0..2 {
                    let target = if p[30] < 0.5 {
                        0.
                    } else if p[30] < 1.5 {
                        if rotor == 0 { 0.667 } else { 0.6 }
                    } else {
                        6.2
                    };
                    let tau = p[31] * 0.001 * (if rotor == 0 { 1. } else { 1.8 });
                    let acceleration = 1.0 - (-1.0 / (tau * self.sr).max(1.0)).exp();
                    let mut times = [0.; CHUNK];
                    for i in 0..n {
                        self.rotor[rotor] += (target - self.rotor[rotor]) * acceleration;
                        self.phase[rotor] =
                            (self.phase[rotor] + self.rotor[rotor] / self.sr).fract();
                        let sine = (core::f32::consts::TAU * self.phase[rotor]).sin();
                        let depth = if rotor == 0 { p[32] } else { p[33] };
                        times[i] =
                            self.sr * (0.003 + (0.0007 + rotor as f32 * 0.0004) * sine * depth);
                        amplitudes[rotor][i] = sine * depth * (1. - p[35] * 0.6);
                    }
                    self.delays[rotor].process_modulated(
                        &mut delayed[rotor][..n],
                        &mut self.delay_store[rotor],
                        &times[..n],
                    );
                }
                for i in 0..n {
                    let wet_l = delayed[0][i] * (1. + amplitudes[0][i] * 0.65)
                        + delayed[1][i] * (1. + amplitudes[1][i] * 0.3);
                    let wet_r = delayed[0][i] * (1. - amplitudes[0][i] * 0.65)
                        + delayed[1][i] * (1. - amplitudes[1][i] * 0.3);
                    let mix = if p[30] < 0.5 { 0. } else { p[34] };
                    let sag = 1. / (1. + p[29] * (l[i] + r[i]).abs());
                    l[i] = (l[i] + (wet_l - l[i]) * mix) * sag;
                    r[i] = (r[i] + (wet_r - r[i]) * mix) * sag;
                }
                // Scanner vibrato/chorus after the cabinet is approximated by
                // a separate short delay, retaining an independent phase.
                if p[15] > 0. {
                    let mut times = [0.; CHUNK];
                    for x in &mut times[..n] {
                        self.phase[4] = (self.phase[4] + 6.7 / self.sr).fract();
                        *x = self.sr
                            * (0.003
                                + 0.00045 * p[15] * (core::f32::consts::TAU * self.phase[4]).sin());
                    }
                    let mut a = [0.; CHUNK];
                    let mut b = [0.; CHUNK];
                    a[..n].copy_from_slice(l);
                    b[..n].copy_from_slice(r);
                    self.delays[3].process_modulated(
                        &mut a[..n],
                        &mut self.delay_store[3],
                        &times[..n],
                    );
                    self.delays[4].process_modulated(
                        &mut b[..n],
                        &mut self.delay_store[4],
                        &times[..n],
                    );
                    for i in 0..n {
                        l[i] += (a[i] - l[i]) * p[15] * 0.5;
                        r[i] += (b[i] - r[i]) * p[15] * 0.5;
                    }
                }
            }
        }
    }
}

pub fn hero(
    kind: Kind,
    p: [f32; PARAMS],
    keys: KeyTable,
    page: &str,
    selected: Option<u32>,
) -> Option<Hero> {
    let sub = keys
        .iter()
        .flatten()
        .flat_map(|k| k.subpages)
        .find(|s| s.title == page)?;
    let ids = sub.slots.iter().flatten().copied().collect::<Vec<_>>();
    let lit = selected.is_none() || selected.is_some_and(|s| ids.contains(&s));
    let mut h = Hero {
        waveform: None,
        title: format!(
            "{} · {}",
            match kind {
                Kind::Mass => "MASS",
                Kind::Pluck => "PLUCK",
                Kind::Vox => "VOX",
                Kind::Pipe => "PIPE",
            },
            page
        ),
        series: Vec::new(),
        marks: Vec::new(),
        x_labels: ["30 Hz".into(), "16 kHz".into()],
        y_labels: ["0".into(), "amplitude".into()],
        diagonal: false,
    };
    let xhz = |hz: f32| ((hz.max(30.) / 30.).ln() / (16000.0_f32 / 30.).ln()).clamp(0., 1.);
    if page == "Amp" {
        let times = [p[16], p[17], p[19]];
        let span = (times.iter().sum::<f32>() + 500.).max(1.);
        let a = p[16] / span;
        let d = a + p[17] / span;
        let r = (span - p[19]) / span;
        let segments = [
            (0.0, a, "attack", 16),
            (a, d, "decay", 17),
            (d, r, "sustain", 18),
            (r, 1.0, "release", 19),
        ];
        let at_release = p[18] + (1.0 - p[18]) * (-4.60517 * ((r - a) * span) / p[17]).exp();
        for (left, right, name, id) in segments {
            h.series.push(HeroSeries {
                name,
                points: (0..33)
                    .map(|i| {
                        let x = left + (right - left) * i as f32 / 32.0;
                        let y = if x < a {
                            if a > 0.0 { x / a } else { 1.0 }
                        } else if x < r {
                            p[18] + (1.0 - p[18]) * (-4.60517 * ((x - a) * span) / p[17]).exp()
                        } else {
                            at_release * (-6.907755 * ((x - r) * span) / p[19]).exp()
                        };
                        (x, y.clamp(0.0, 1.0))
                    })
                    .collect(),
                lit: selected.is_none()
                    || selected == Some(id)
                    || selected.is_some_and(|s| s >= 20),
            });
        }
        h.x_labels = ["note on".into(), format!("{:0.2} s", span * 0.001)];
        h.y_labels = ["silence".into(), "level".into()];
        return Some(h);
    }
    if kind == Kind::Vox
        && ((page == "Articulate" && selected.is_none_or(|id| (8..=11).contains(&id)))
            || (page == "Voice" && selected == Some(5)))
    {
        // Same bend decay and delayed vibrato as Bank::configure, in semitones.
        let span = (p[9] * 0.005).max(p[11] * 0.001 + 1.0).max(1.0);
        let limit = p[8].abs().max(0.5) + 0.5;
        for (name, combined) in [("bend", false), ("pitch", true)] {
            h.series.push(HeroSeries {
                name,
                points: (0..257)
                    .map(|i| {
                        let x = i as f32 / 256.0;
                        let t = x * span;
                        let bend = p[8] * (-t / (p[9] * 0.001).max(0.001)).exp();
                        let vibrato = p[5]
                            * 0.5
                            * (core::f32::consts::TAU * p[10] * t).sin()
                            * (t / (p[11] * 0.001).max(0.001)).clamp(0.0, 1.0);
                        (
                            x,
                            0.5 + (bend + if combined { vibrato } else { 0.0 }) / (2.0 * limit),
                        )
                    })
                    .collect(),
                lit: selected.is_none()
                    || if combined {
                        matches!(selected, Some(5 | 10 | 11))
                    } else {
                        matches!(selected, Some(8 | 9))
                    },
            });
        }
        h.x_labels = ["note on".into(), format!("{span:.2} s")];
        h.y_labels = [format!("−{limit:.1} st"), format!("+{limit:.1} st")];
        return Some(h);
    }
    if (kind == Kind::Vox) && (page == "Voice" || page == "Articulate" || page == "Talk") {
        let v = (p[0]
            + if page == "Talk" {
                p[24] * p[25] * p[28]
            } else {
                0.
            })
        .clamp(0., 4.);
        // Reference A3, including the source's MIDI-key formant tracking.
        let f = formants(v, (p[1] - p[12] / 24.0).clamp(0.0, 1.0));
        for (i, center) in f.iter().copied().enumerate() {
            h.series.push(HeroSeries {
                name: ["first", "second", "third"][i],
                points: (0..129)
                    .map(|n| {
                        let x = n as f32 / 128.;
                        let hz = 30. * (16000.0_f32 / 30.).powf(x);
                        let d = (hz / center).ln() / (0.055 + 0.2 * p[2]);
                        (x, (-0.5 * d * d).exp() / (1. + i as f32 * 0.25))
                    })
                    .collect(),
                lit: lit && selected != Some(3),
            });
            h.marks.push(HeroMark {
                x: xhz(center),
                label: format!("F{} {:0.0}", i + 1, center),
                lit: lit && selected != Some(3),
            });
        }
        // Expected noise transfer through the same three resonator poles.
        h.series.push(HeroSeries {
            name: "breath",
            points: (0..129)
                .map(|n| {
                    let x = n as f32 / 128.0;
                    let hz = 30.0 * (16000.0_f32 / 30.0).powf(x);
                    let w = core::f32::consts::TAU * hz / 48000.0;
                    let response = f
                        .iter()
                        .map(|&center| {
                            let radius = (-core::f32::consts::PI * center * (0.055 + 0.2 * p[2])
                                / 48000.0)
                                .exp();
                            let aa =
                                2.0 * radius * (core::f32::consts::TAU * center / 48000.0).cos();
                            let bb = radius * radius;
                            let re = 1.0 - aa * w.cos() + bb * (2.0 * w).cos();
                            let im = aa * w.sin() - bb * (2.0 * w).sin();
                            (1.0 - bb) * 0.2 / (re * re + im * im).sqrt().max(1e-6)
                        })
                        .sum::<f32>()
                        + p[15] * 0.2;
                    (x, (p[3].sqrt() * response * 0.6).clamp(0.0, 1.0))
                })
                .collect(),
            lit: selected.is_none() || selected == Some(3) || selected == Some(15),
        });
        let root = 220.0 * 2.0_f32.powf(p[6] / 12.0);
        h.marks.push(HeroMark {
            x: xhz(root),
            label: format!("root {root:.0} Hz"),
            lit: selected == Some(6),
        });
        return Some(h);
    }
    if kind == Kind::Pipe && (page == "Low" || page == "High") {
        let weights = registration(
            p[4],
            [p[0], p[1], p[2], p[3], p[8], p[9], p[10], p[11], p[12]],
        );
        let ratios = [0.5, 1.5, 1., 2., 3., 4., 5., 6., 8.];
        let root = 220.0 * 2.0_f32.powf(p[6] / 12.0);
        for i in 0..9 {
            let x = xhz(root * ratios[i]);
            h.series.push(HeroSeries {
                name: "drawbar",
                points: vec![(x, 0.), (x, weights[i])],
                lit: selected.is_none()
                    || selected == Some(4)
                    || selected == Some([0, 1, 2, 3, 8, 9, 10, 11, 12][i]),
            });
        }
        let sp = spectrum(kind, &p, root, 0.8);
        for i in 9..27 {
            let x = xhz(root * sp.ratios[i]);
            h.series.push(HeroSeries {
                name: "leak",
                points: vec![(x, 0.0), (x, sp.gains[i].abs().clamp(0.0, 1.0))],
                lit: selected == Some(5),
            });
        }
        return Some(h);
    }
    if page == "Rotary" {
        let show_speed = selected.is_none_or(|id| id == 30 || id == 31);
        let span = if show_speed { 6.0 } else { 2.0 };
        for rotor in 0..2 {
            let target = if p[30] < 0.5 {
                0.0
            } else if p[30] < 1.5 {
                if rotor == 0 { 0.667 } else { 0.6 }
            } else {
                6.2
            };
            // The audio uses this exponential recurrence each sample. The
            // analytic speed/phase below is its trajectory from rest (or brake
            // from fast), with the same independent horn/drum time constants.
            let initial = if p[30] < 0.5 { 6.2 } else { 0.0 };
            let tau = p[31] * 0.001 * if rotor == 0 { 1.0 } else { 1.8 };
            h.series.push(HeroSeries {
                name: if rotor == 0 { "horn" } else { "drum" },
                points: (0..257)
                    .map(|i| {
                        let x = i as f32 / 256.0;
                        let t = x * span;
                        let decay = (-t / tau).exp();
                        let speed = target + (initial - target) * decay;
                        let phase = rotor as f32 * 0.2
                            + target * t
                            + (initial - target) * tau * (1.0 - decay);
                        let depth = if rotor == 0 { p[32] } else { p[33] };
                        let stereo_gain = if rotor == 0 { 0.65 } else { 0.3 };
                        let mix = if p[30] < 0.5 { 0.0 } else { p[34] };
                        let modulation = (core::f32::consts::TAU * phase).sin()
                            * depth
                            * (1.0 - p[35] * 0.6)
                            * mix
                            * stereo_gain;
                        (
                            x,
                            if show_speed {
                                speed / 6.2
                            } else {
                                0.5 + modulation * 0.5
                            },
                        )
                    })
                    .collect(),
                lit: selected.is_none()
                    || matches!(selected, Some(30 | 31 | 34 | 35))
                    || selected == Some(if rotor == 0 { 32 } else { 33 }),
            });
        }
        h.x_labels = [
            if p[30] < 0.5 {
                "brake from fast".into()
            } else {
                "start from rest".into()
            },
            format!("{span:.1} s"),
        ];
        h.y_labels = if show_speed {
            ["0 Hz".into(), "6.2 Hz".into()]
        } else {
            ["−1 gain".into(), "+1".into()]
        };
        return Some(h);
    }
    if page == "Choir" {
        let count = p[30].round().clamp(1.0, 3.0) as usize;
        let span = 3.0;
        for voice in 0..count {
            let rate = p[32] * (1.0 + voice as f32 * 0.073);
            let depth_ms = (p[31] / 1200.0 * core::f32::consts::LN_2 * 1000.0
                / (core::f32::consts::TAU * rate.max(0.05)))
            .min(18.0);
            let pan = if count == 1 {
                0.0
            } else {
                (voice as f32 / (count - 1) as f32 - 0.5) * p[34]
            };
            h.series.push(HeroSeries {
                name: ["voice 1", "voice 2", "voice 3"][voice],
                points: (0..257)
                    .map(|i| {
                        let x = i as f32 / 256.0;
                        let t = x * span;
                        let delay = p[35]
                            + depth_ms
                                * (1.0
                                    + (core::f32::consts::TAU * (rate * t + voice as f32 * 0.2))
                                        .sin());
                        let y = if selected == Some(34) {
                            0.5 + pan
                        } else if selected == Some(33) {
                            p[33] / count as f32
                        } else {
                            delay / 120.0
                        };
                        (x, y.clamp(0.0, 1.0))
                    })
                    .collect(),
                lit,
            });
        }
        h.x_labels = ["0 s".into(), format!("{span:.1} s")];
        h.y_labels = match selected {
            Some(34) => ["left".into(), "right".into()],
            Some(33) => ["0 send".into(), "1".into()],
            _ => ["0 ms".into(), "120 ms delay".into()],
        };
        return Some(h);
    }
    if page == "Weight" && selected == Some(29) {
        let sp = spectrum(kind, &p, 55.0, 0.8);
        for i in 0..24 {
            let x = xhz(55.0 * sp.ratios[i]);
            h.series.push(HeroSeries {
                name: if i < 17 {
                    "root and layer"
                } else {
                    "skew shoulder"
                },
                points: vec![(x, 0.0), (x, sp.gains[i].abs().clamp(0.0, 1.0))],
                lit: i >= 17,
            });
        }
        return Some(h);
    }
    if page == "Weight" || page == "Tube" {
        let mut shape = Waveshaper::new();
        shape.configure(
            Shape::SoftClip,
            1. + p[24] * 8.,
            if page == "Tube" {
                p[25] * 0.4
            } else {
                p[27] * p[25]
            },
            1.,
        );
        h.series.push(HeroSeries {
            name: "transfer",
            points: (0..129)
                .map(|i| {
                    let x = i as f32 / 128.;
                    let y = if page == "Weight" {
                        weight_transfer(x * 2.0 - 1.0, &p)
                    } else {
                        shape.shape(x * 2.0 - 1.0) - shape.shape(0.0)
                    };
                    (x, (y * 0.5 + 0.5).clamp(0., 1.))
                })
                .collect(),
            lit,
        });
        h.diagonal = true;
        h.x_labels = ["−1 input".into(), "+1".into()];
        h.y_labels = ["−1 output".into(), "+1".into()];
        return Some(h);
    }
    if page == "Clamp" {
        let ceil = 10.0_f32.powf(p[30] / 20.);
        h.series.push(HeroSeries {
            name: "ceiling",
            points: (0..129)
                .map(|i| {
                    let x = i as f32 / 128.;
                    (x, x.min(ceil))
                })
                .collect(),
            lit,
        });
        h.diagonal = true;
        h.x_labels = ["input 0".into(), "1".into()];
        h.y_labels = ["output 0".into(), "1".into()];
        h.marks.push(HeroMark {
            x: ceil,
            label: format!("ceiling {:0.1} dB", p[30]),
            lit,
        });
        return Some(h);
    }
    if page == "Ladder" {
        let co = p[8];
        h.y_labels = ["−60 dB".into(), "0 dB".into()];
        h.series.push(HeroSeries {
            name: "four poles",
            points: (0..129)
                .map(|i| {
                    let x = i as f32 / 128.;
                    let hz = 30. * (16000.0_f32 / 30.).powf(x);
                    let response = (1. + (hz / co).powi(2)).powi(-2);
                    (
                        x,
                        (1. + 20. * response.max(0.001).log10() / 60.).clamp(0., 1.),
                    )
                })
                .collect(),
            lit,
        });
        h.marks.push(HeroMark {
            x: xhz(co),
            label: format!("{:0.0} Hz", co),
            lit,
        });
        return Some(h);
    }
    if page == "Body" {
        // Render the actual FDN, in the green zone. No invented mode ladder.
        let mut fdn = Fdn::new();
        let mut store = vec![0.0; Fdn::buffer_len(48000.0)];
        fdn.prepare(48000.0, &mut store);
        fdn.set_size(0.35 + p[24] * 1.4);
        fdn.set_decay(p[25]);
        fdn.set_damping(p[26]);
        fdn.set_diffusion(p[28]);
        fdn.set_modulation(p[29] * 8.0);
        let mut points = Vec::with_capacity(128);
        let mut input = [0.0; 64];
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        input[0] = 1.0;
        for bin in 0..128 {
            fdn.process(&input, &mut left, &mut right, &mut store);
            input.fill(0.0);
            let energy = left.iter().chain(&right).map(|x| x * x).sum::<f32>() / 128.0;
            points.push((bin as f32 / 127.0, energy.sqrt()));
        }
        let peak = points.iter().map(|p| p.1).fold(0.00001_f32, f32::max);
        for point in &mut points {
            point.1 = (point.1 / peak).clamp(0.0, 1.0);
        }
        h.series.push(HeroSeries {
            name: "FDN impulse",
            points,
            lit,
        });
        h.x_labels = ["impulse".into(), "171 ms".into()];
        h.y_labels = ["silence".into(), "response".into()];
        return Some(h);
    }
    if page == "Shine" {
        h.series.push(HeroSeries {
            name: "transient",
            points: (0..129)
                .map(|i| {
                    let x = i as f32 / 128.;
                    (x, (-x * (4. + p[34] * 0.02)).exp() * (0.5 + 0.45 * p[30]))
                })
                .collect(),
            lit,
        });
        h.x_labels = ["strike".into(), "tail".into()];
        h.y_labels = ["0".into(), "lift".into()];
        return Some(h);
    }
    let root = (if kind == Kind::Mass { 110.0 } else { 220.0 }) * 2.0_f32.powf(p[0] / 12.0);
    let s = spectrum(kind, &p, root, 0.8);
    let max = s
        .gains
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.001_f32, f32::max);
    for m in 0..32 {
        if s.gains[m].abs() > 0.0001 {
            let x = xhz(s.ratios[m] * root);
            h.series.push(HeroSeries {
                name: "mode",
                points: vec![(x, 0.), (x, (s.gains[m].abs() / max).clamp(0., 1.))],
                lit,
            });
        }
    }
    Some(h)
}

#[cfg(test)]
pub fn test_contract(kind: Kind, table: &'static [ParamDef], keys: KeyTable) {
    let p = core::array::from_fn(|i| table.get(i).map(|d| d.default).unwrap_or(0.));
    let mut a = Instrument::new(kind, table);
    let mut b = Instrument::new(kind, table);
    a.prepare(48000., 1024, p);
    b.prepare(48000., 1024, p);
    let mut x = [0.; 512];
    let mut g = Ramp::across(1., 1., 512);
    a.render(&mut x, 0, &mut g);
    assert!(x.iter().all(|v| *v == 0.));
    a.reset();
    a.note_on(60, 0.8, 1);
    b.note_on(60, 0.8, 1);
    let mut y = x;
    let mut ga = Ramp::across(1., 1., 512);
    let mut gb = Ramp::across(1., 1., 512);
    a.render(&mut x, 0, &mut ga);
    b.render(&mut y[..100], 0, &mut gb);
    b.render(&mut y[100..357], 100, &mut gb);
    b.render(&mut y[357..], 357, &mut gb);
    assert_eq!(x, y, "split {kind:?}");
    assert_eq!(a.right(512), b.right(512));
    assert!(x.iter().any(|v| v.abs() > 0.001), "audible {kind:?}");
    assert_no_alloc::assert_no_alloc(|| {
        a.note_on(65, 0.8, 2);
        a.note_off(60);
        a.render(&mut x, 0, &mut ga);
        a.release_all();
        a.render(&mut [], 0, &mut ga);
    });
    for d in table {
        for value in [d.min, d.max, f32::NAN, f32::INFINITY] {
            a.set_param(d.id, value);
            a.note_on(48, 1., 2);
            a.render(&mut x, 0, &mut ga);
            assert!(
                x.iter().all(|v| v.is_finite()),
                "finite {kind:?} {}",
                d.name
            );
            a.reset();
        }
    }
    for key in keys.iter().flatten() {
        for sub in key.subpages {
            let pic = hero(
                kind,
                p,
                keys,
                sub.title,
                sub.slots.iter().flatten().copied().next(),
            )
            .unwrap();
            assert!(!pic.series.is_empty());
            for pt in pic.series.iter().flat_map(|s| s.points.iter()) {
                assert!((0.0..=1.0).contains(&pt.0) && (0.0..=1.0).contains(&pt.1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pluck_body_reset_matches_fresh_after_prior_modulation_and_patch_changes() {
        use crate::params::pluck as p;
        let defaults = core::array::from_fn(|i| p::TABLE[i].default);
        let mut target = defaults;
        target[p::BODY_SIZE as usize] = 0.72;
        target[p::BODY_DECAY as usize] = 2.4;
        target[p::BODY_DAMP as usize] = 3100.0;
        target[p::BODY_MIX as usize] = 0.85;
        target[p::DIFFUSE as usize] = 0.77;
        target[p::MOTION as usize] = 0.93;
        target[p::SPREAD as usize] = 0.0;
        let mut fresh = Instrument::new(Kind::Pluck, p::TABLE);
        let mut reused = Instrument::new(Kind::Pluck, p::TABLE);
        fresh.prepare(48000.0, 512, target);
        reused.prepare(48000.0, 512, defaults);
        reused.set_param(p::BODY_SIZE, 0.11);
        reused.set_param(p::BODY_DECAY, 7.0);
        reused.set_param(p::BODY_DAMP, 700.0);
        reused.set_param(p::MOTION, 0.67);
        reused.set_param(p::BODY_MIX, 1.0);
        reused.note_on(39, 0.9, 17);
        let mut a = [0.0; 512];
        let mut b = [0.0; 512];
        let mut prior_gain = Ramp::across(1.0, 1.0, 1);
        for _ in 0..181 {
            reused.render(&mut a[..357], 0, &mut prior_gain);
        }
        for (id, value) in target.iter().copied().enumerate() {
            reused.set_param(id as u32, value);
        }
        // A held FX lock must also disappear on reset, with base coefficients
        // reinstalled rather than the last audible patch or snapshot defaults.
        reused.plock(p::BODY_SIZE, Some(0.08));
        reused.render(&mut a[..100], 0, &mut prior_gain);
        assert_no_alloc::assert_no_alloc(|| reused.reset());
        for (age, pitch) in [45, 52, 64].into_iter().enumerate() {
            fresh.note_on(pitch, 0.83, age as u64);
            reused.note_on(pitch, 0.83, age as u64);
        }
        let mut fresh_gain = Ramp::across(1.0, 1.0, 1);
        let mut reused_gain = Ramp::across(1.0, 1.0, 1);
        let mut wet_stereo_difference = 0.0;
        for block in 0..160 {
            if block == 30 {
                fresh.release_all();
                reused.release_all();
            }
            fresh.render(&mut a, 0, &mut fresh_gain);
            reused.render(&mut b, 0, &mut reused_gain);
            assert_eq!(a, b, "left, block {block}");
            assert_eq!(fresh.right(512), reused.right(512), "right, block {block}");
            wet_stereo_difference += a
                .iter()
                .zip(fresh.right(512))
                .map(|(left, right)| (left - right).abs())
                .sum::<f32>();
        }
        assert!(
            wet_stereo_difference > 0.1,
            "active Body must produce stereo output"
        );
    }
    #[test]
    fn note_locks_are_independent_and_letters_restore() {
        let table = crate::params::pluck::TABLE;
        let p = core::array::from_fn(|i| table[i].default);
        let mut v = Instrument::new(Kind::Pluck, table);
        v.prepare(48000., 512, p);
        v.plock(6, Some(0.9));
        v.note_on(60, 1., 1);
        v.plock(6, None);
        v.note_on(64, 1., 2);
        assert_eq!(v.patches[6][0], 0.9);
        assert_eq!(v.patches[6][1], p[6]);
        v.set_param(6, 0.4);
        assert_eq!(v.patches[6][0], 0.9);
        assert_eq!(v.patches[6][1], 0.4);
        assert_eq!(v.base[6], 0.4);
    }
    #[test]
    fn zero_velocity_releases_instead_of_allocating_voice() {
        let table = crate::params::vox::TABLE;
        let p = core::array::from_fn(|i| table[i].default);
        let mut v = Instrument::new(Kind::Vox, table);
        v.prepare(48000., 512, p);
        v.note_on(60, 1., 1);
        assert!(v.bank.held(0));
        v.note_on(60, 0., 2);
        assert!(!v.bank.held(0));
    }

    #[test]
    fn glide_follows_the_newest_locked_note_without_moving_older_tails() {
        let table = crate::params::pluck::TABLE;
        let p = core::array::from_fn(|i| table[i].default);
        let mut v = Instrument::new(Kind::Pluck, table);
        v.prepare(48000.0, 512, p);
        v.plock(6, Some(0.9));
        v.note_on(60, 1.0, 1);
        v.plock(6, Some(0.7));
        v.note_on(64, 1.0, 2);
        v.plock_glide(6, 0.5);
        assert_eq!(v.patches[6][0], 0.9);
        assert!((v.patches[6][1] - (0.7 + p[6]) * 0.5).abs() < 1e-6);
    }
    #[test]
    fn level_lock_sounds_over_zero_base_and_restores_per_note() {
        let table = crate::params::pluck::TABLE;
        let mut p = core::array::from_fn(|i| table[i].default);
        p[21] = 0.0;
        p[27] = 0.0;
        p[32] = 0.0;
        let mut v = Instrument::new(Kind::Pluck, table);
        v.prepare(48000.0, 512, p);
        v.plock(21, Some(0.8));
        v.note_on(60, 1.0, 1);
        v.plock(21, None);
        v.note_on(64, 1.0, 2);
        assert_eq!(v.patches[21][0], 0.8);
        assert_eq!(v.patches[21][1], 0.0);
        let mut out = [0.0; 512];
        let mut g = Ramp::across(1.0, 1.0, 512);
        v.render(&mut out, 0, &mut g);
        assert!(out.iter().any(|x| x.abs() > 0.01));
        v.reset();
        v.note_on(60, 1.0, 1);
        v.render(&mut out, 0, &mut g);
        assert!(out.iter().all(|x| *x == 0.0));
    }
    #[test]
    fn weight_zero_is_the_wire_and_bias_produces_even_asymmetry() {
        let mut p = core::array::from_fn(|i| crate::params::mass::TABLE[i].default);
        p[24] = 0.0;
        for i in 0..101 {
            let x = i as f32 / 50.0 - 1.0;
            assert_eq!(weight_transfer(x, &p), x);
        }
        p[24] = 1.0;
        p[25] = 1.0;
        p[27] = 0.6;
        p[28] = 0.8;
        assert!((weight_transfer(0.1, &p) + weight_transfer(-0.1, &p)).abs() > 0.01);
    }
    #[test]
    fn mass_clamp_contains_a_sixteen_note_chord() {
        let table = crate::params::mass::TABLE;
        let mut p = core::array::from_fn(|i| table[i].default);
        p[21] = 1.0;
        p[23] = 16.0;
        p[30] = -9.0;
        p[35] = 1.0;
        p[32] = 0.0;
        p[3] = 0.8;
        p[8] = 4000.0;
        let mut v = Instrument::new(Kind::Mass, table);
        v.prepare(48000.0, 512, p);
        for i in 0..16 {
            v.note_on(36 + i, 1.0, i as u64);
        }
        let mut out = [0.0; 512];
        let mut peak = 0.0_f32;
        for _ in 0..80 {
            let mut g = Ramp::across(1.0, 1.0, 512);
            v.render(&mut out, 0, &mut g);
            for &x in out.iter().chain(v.right(512)) {
                peak = peak.max(x.abs());
            }
        }
        assert!(peak > 0.1);
        assert!(peak <= 10.0_f32.powf(-8.9 / 20.0), "peak {peak}");
    }
    #[test]
    fn fast_rotors_settle_at_documented_rates() {
        let table = crate::params::pipe::TABLE;
        let mut p = core::array::from_fn(|i| table[i].default);
        p[30] = 2.0;
        p[31] = 200.0;
        let mut v = Instrument::new(Kind::Pipe, table);
        v.prepare(48000.0, 512, p);
        let mut x = [0.0; 512];
        let mut g = Ramp::across(1.0, 1.0, 512);
        for _ in 0..300 {
            v.render(&mut x, 0, &mut g);
        }
        assert!((v.rotor[0] - 6.2).abs() < 0.02);
        assert!((v.rotor[1] - 6.2).abs() < 0.02);
    }
    #[test]
    fn maximum_motion_effects_are_split_exact_in_stereo() {
        for (kind, table) in [
            (Kind::Pluck, crate::params::pluck::TABLE),
            (Kind::Vox, crate::params::vox::TABLE),
            (Kind::Pipe, crate::params::pipe::TABLE),
        ] {
            let mut p = core::array::from_fn(|i| table[i].default);
            for i in 24..36 {
                p[i] = table[i].max;
            }
            let mut a = Instrument::new(kind, table);
            let mut b = Instrument::new(kind, table);
            a.prepare(48000.0, 1024, p);
            b.prepare(48000.0, 1024, p);
            a.note_on(60, 0.8, 1);
            b.note_on(60, 0.8, 1);
            let mut x = [0.0; 512];
            let mut y = x;
            let mut ga = Ramp::across(1.0, 1.0, 512);
            let mut gb = Ramp::across(1.0, 1.0, 512);
            for _ in 0..24 {
                a.render(&mut x, 0, &mut ga);
                b.render(&mut y[..100], 0, &mut gb);
                b.render(&mut y[100..357], 100, &mut gb);
                b.render(&mut y[357..], 357, &mut gb);
                assert_eq!(x, y, "{kind:?}");
                assert_eq!(a.right(512), b.right(512), "right {kind:?}");
            }
        }
    }
    #[test]
    fn coverage_walkers_redraw_their_own_picture() {
        for (kind, table, keys, id, page) in [
            (
                Kind::Mass,
                crate::params::mass::TABLE,
                crate::params::mass::KEYS,
                29,
                "Weight",
            ),
            (
                Kind::Pluck,
                crate::params::pluck::TABLE,
                crate::params::pluck::KEYS,
                6,
                "String",
            ),
            (
                Kind::Vox,
                crate::params::vox::TABLE,
                crate::params::vox::KEYS,
                3,
                "Voice",
            ),
            (
                Kind::Pipe,
                crate::params::pipe::TABLE,
                crate::params::pipe::KEYS,
                5,
                "Low",
            ),
        ] {
            let mut p = core::array::from_fn(|i| table[i].default);
            p[id] = table[id].min;
            let a = hero(kind, p, keys, page, Some(id as u32));
            p[id] = table[id].max;
            let b = hero(kind, p, keys, page, Some(id as u32));
            assert_ne!(a, b, "{kind:?}");
        }
    }
    #[test]
    fn hero_articulation_and_rotary_show_direction_and_inertia() {
        let mut vox = core::array::from_fn(|i| crate::params::vox::TABLE[i].default);
        vox[8] = 12.0;
        let up = hero(
            Kind::Vox,
            vox,
            crate::params::vox::KEYS,
            "Articulate",
            Some(8),
        )
        .unwrap();
        vox[8] = -12.0;
        let down = hero(
            Kind::Vox,
            vox,
            crate::params::vox::KEYS,
            "Articulate",
            Some(8),
        )
        .unwrap();
        assert!(up.series[0].points[0].1 > 0.9);
        assert!(down.series[0].points[0].1 < 0.1);
        assert!((up.series[0].points.last().unwrap().1 - 0.5).abs() < 0.001);
        let mut pipe = core::array::from_fn(|i| crate::params::pipe::TABLE[i].default);
        pipe[30] = 2.0;
        pipe[31] = 200.0;
        let fast = hero(
            Kind::Pipe,
            pipe,
            crate::params::pipe::KEYS,
            "Rotary",
            Some(30),
        )
        .unwrap();
        assert!(fast.series[0].points.last().unwrap().1 > 0.99);
        pipe[31] = 5000.0;
        let gradual = hero(
            Kind::Pipe,
            pipe,
            crate::params::pipe::KEYS,
            "Rotary",
            Some(31),
        )
        .unwrap();
        assert!(gradual.series[0].points[64].1 < fast.series[0].points[64].1);
        assert!(gradual.series[1].points[64].1 < gradual.series[0].points[64].1);
        pipe[30] = 0.0;
        let brake = hero(
            Kind::Pipe,
            pipe,
            crate::params::pipe::KEYS,
            "Rotary",
            Some(30),
        )
        .unwrap();
        assert!(brake.series[0].points[0].1 > brake.series[0].points[256].1);
        for picture in [&up, &down, &fast, &gradual, &brake] {
            assert!(
                picture
                    .series
                    .iter()
                    .flat_map(|s| &s.points)
                    .all(|&(x, y)| x.is_finite()
                        && y.is_finite()
                        && (0.0..=1.0).contains(&x)
                        && (0.0..=1.0).contains(&y))
            );
        }
    }
}
