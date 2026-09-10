//! Common lifecycle and effect wiring. The four source algorithms remain distinct.
//! Note state is parameter-major across sixteen lanes; overlapping locks are captured.
use super::spectral::{SIZE, Settings, Spectral};
use crate::audio::graph::Ramp;
use crate::dsp::coverage_spine::{self as core, Envelopes, Frame, Lowpass, Modes, N, Oscillators};
use crate::dsp::delay::{DelayLine, FeedbackDelay};
use crate::dsp::fdn::Fdn;
use crate::dsp::filters::{DcBlocker, Disperser, Tilt};
use crate::dsp::osc::{TABLE_LEN, Waveform, build_tables, table_len};
use crate::dsp::shaper::{Mode as Shape, Waveshaper};
const PARAMS: usize = 64;
const CHUNK: usize = 32;
pub trait Patch: Copy + Default {
    const KIND: u8;
    const TABLE: &'static [crate::params::ParamDef];
    fn get(self, id: u32) -> f32;
    fn set(&mut self, id: u32, v: f32);
}

pub struct Bank<P: Patch> {
    base: P,
    live: P,
    locks: u64,
    note_locks: [u64; N],
    values: [Frame; PARAMS],
    sr: f32,
    pitch: [u8; N],
    root: [u8; N],
    age: [u64; N],
    event: [u64; N],
    current_event: u64,
    held: [bool; N],
    velocity: Frame,
    frequency: Frame,
    source_gain: Frame,
    elapsed: [u64; N],
    tail: [usize; N],
    steal: Frame,
    steal_left: [usize; N],
    last: Frame,
    // Glass's mono foundation bypasses internal colour FX. Its separate steal
    // history keeps note stealing click-limited without changing legacy voices.
    sub_last: Frame,
    sub_steal: Frame,
    sub_steal_left: [usize; N],
    amp: Envelopes,
    motion: Envelopes,
    filter: Lowpass,
    osc: Oscillators,
    modes: Modes,
    feedback: Frame,
    tables: Vec<f32>,
    offsets: [usize; 8],
    lens: [usize; 8],
    right: Vec<f32>,
    chunk: usize,
    spectral: Spectral,
    amp_delay: Vec<Frame>,
    delay_pos: usize,
    comb: [FeedbackDelay; 2],
    comb_store: [Vec<f32>; 2],
    echo: [FeedbackDelay; 2],
    echo_store: [Vec<f32>; 2],
    ensemble: [DelayLine; 3],
    ensemble_store: [Vec<f32>; 3],
    tilt: [Tilt; 2],
    disperse: [Disperser; 2],
    dc: [DcBlocker; 2],
    shape: Waveshaper,
    bloom: Fdn,
    bloom_initial: Fdn,
    bloom_store: Vec<f32>,
    fx_phase: f64,
    last_hz: f32,
    shimmer_feedback: [f32; 2],
}
impl<P: Patch> Bank<P> {
    pub fn new() -> Self {
        Self {
            base: P::default(),
            live: P::default(),
            locks: 0,
            note_locks: [0; N],
            values: [[0.; N]; PARAMS],
            sr: 48000.,
            pitch: [0; N],
            root: [0; N],
            age: [0; N],
            event: [0; N],
            current_event: 0,
            held: [false; N],
            velocity: [0.; N],
            frequency: [220.; N],
            source_gain: [1.; N],
            elapsed: [0; N],
            tail: [0; N],
            steal: [0.; N],
            steal_left: [0; N],
            last: [0.; N],
            sub_last: [0.; N],
            sub_steal: [0.; N],
            sub_steal_left: [0; N],
            amp: Envelopes::new(),
            motion: Envelopes::new(),
            filter: Lowpass::new(),
            osc: Oscillators::new(),
            modes: Modes::new(),
            feedback: [0.; N],
            tables: Vec::new(),
            offsets: [0; 8],
            lens: [0; 8],
            right: Vec::new(),
            chunk: 0,
            spectral: Spectral::new(),
            amp_delay: Vec::new(),
            delay_pos: 0,
            comb: std::array::from_fn(|_| FeedbackDelay::new()),
            comb_store: std::array::from_fn(|_| Vec::new()),
            echo: std::array::from_fn(|_| FeedbackDelay::new()),
            echo_store: std::array::from_fn(|_| Vec::new()),
            ensemble: std::array::from_fn(|_| DelayLine::new()),
            ensemble_store: std::array::from_fn(|_| Vec::new()),
            tilt: std::array::from_fn(|_| Tilt::new()),
            disperse: std::array::from_fn(|_| Disperser::new()),
            dc: std::array::from_fn(|_| DcBlocker::new()),
            shape: Waveshaper::new(),
            bloom: Fdn::new(),
            bloom_initial: Fdn::new(),
            bloom_store: Vec::new(),
            fx_phase: 0.,
            last_hz: 220.,
            shimmer_feedback: [0.; 2],
        }
    }
    pub fn sample_rate(&self) -> f32 {
        self.sr
    }
    pub fn prepare(&mut self, sr: f32, block: usize, params: P) {
        self.sr = if sr.is_finite() {
            sr.clamp(8000., 192000.)
        } else {
            48000.
        };
        self.base = P::default();
        for d in P::TABLE {
            self.base.set(d.id, params.get(d.id));
        }
        self.live = self.base;
        self.right.resize(block.max(1), 0.);
        let mut len = 0;
        for (i, w) in Waveform::ALL.iter().enumerate() {
            self.offsets[i] = len;
            self.lens[i] = table_len(*w);
            len += self.lens[i];
        }
        self.tables.resize(len, 0.);
        for (i, w) in Waveform::ALL.iter().enumerate() {
            if let Some(s) = self
                .tables
                .get_mut(self.offsets[i]..self.offsets[i] + self.lens[i])
            {
                build_tables(*w, s);
            }
        }
        for ch in 0..2 {
            self.comb[ch].prepare(self.sr, (self.sr * 0.1) as usize, 6000.);
            self.comb_store[ch].resize(FeedbackDelay::needed_len((self.sr * 0.1) as usize), 0.);
            self.echo[ch].prepare(self.sr, self.sr as usize, 8000.);
            self.echo_store[ch].resize(FeedbackDelay::needed_len(self.sr as usize), 0.);
            self.dc[ch].prepare(self.sr);
        }
        for c in 0..3 {
            let max = (self.sr * 0.08) as usize;
            self.ensemble[c].prepare(max);
            self.ensemble_store[c].resize(crate::dsp::delay::buffer_len(max), 0.);
        }
        if P::KIND == 1 {
            self.bloom_store.resize(Fdn::buffer_len(self.sr), 0.);
            self.bloom.prepare(self.sr, &mut self.bloom_store);
            self.bloom_initial = self.bloom;
        }
        if P::KIND >= 2 {
            self.spectral.prepare(self.sr);
        }
        if P::KIND == 2 {
            self.amp_delay.resize(SIZE, [0.; N]);
        }
        self.reset();
    }
    pub fn reset(&mut self) {
        self.amp.reset();
        self.motion.reset();
        self.held.fill(false);
        self.tail.fill(0);
        self.steal_left.fill(0);
        self.last.fill(0.);
        self.sub_last.fill(0.);
        self.sub_steal.fill(0.);
        self.sub_steal_left.fill(0);
        self.feedback.fill(0.);
        self.chunk = 0;
        self.locks = 0;
        self.live = self.base;
        self.delay_pos = 0;
        self.fx_phase = 0.;
        self.shimmer_feedback = [0.; 2];
        for lane in 0..N {
            self.osc.reset_lane(lane, 0., lane as u32 + 1);
            self.modes.reset_lane(lane);
            self.filter.reset_lane(lane);
        }
        self.spectral.reset();
        self.amp_delay.fill([0.; N]);
        for ch in 0..2 {
            self.comb[ch].reset();
            self.echo[ch].reset();
            self.tilt[ch].reset();
            self.disperse[ch].reset();
            self.dc[ch].reset();
            self.comb_store[ch].fill(0.);
            self.echo_store[ch].fill(0.);
        }
        for ch in 0..3 {
            self.ensemble[ch].reset();
            self.ensemble_store[ch].fill(0.);
        }
        if P::KIND == 1 {
            // Fdn::reset preserves modulation phase for legacy transport
            // users. A reset instrument must instead reproduce a fresh note,
            // including Bloom's initial read-position modulation. Restore
            // the prepared silent kernel; the next chunk retunes its params.
            self.bloom = self.bloom_initial;
            self.bloom.reset(&mut self.bloom_store);
        }
    }
    pub fn set_param(&mut self, id: u32, value: f32) {
        if !value.is_finite() || id as usize >= PARAMS {
            return;
        }
        self.base.set(id, value);
        if self.locks & (1u64 << id) == 0 {
            self.live.set(id, value);
        }
        for lane in 0..N {
            if self.note_locks[lane] & (1u64 << id) == 0 {
                self.values[id as usize][lane] = self.base.get(id);
            }
        }
        self.chunk = 0;
    }
    pub fn plock(&mut self, id: u32, value: Option<f32>) {
        if id as usize >= PARAMS {
            return;
        }
        match value {
            Some(v) if v.is_finite() => {
                self.live.set(id, v);
                self.locks |= 1u64 << id;
            }
            None => {
                self.live.set(id, self.base.get(id));
                self.locks &= !(1u64 << id);
            }
            _ => {}
        }
    }
    pub fn plock_glide(&mut self, id: u32, alpha: f32) {
        if id as usize >= PARAMS || !alpha.is_finite() {
            return;
        }
        let v = self.live.get(id) + (self.base.get(id) - self.live.get(id)) * alpha.clamp(0., 1.);
        self.live.set(id, v);
        for lane in 0..N {
            if self.event[lane] == self.current_event && self.note_locks[lane] & (1u64 << id) != 0 {
                let old = self.v(id as usize, lane);
                self.values[id as usize][lane] =
                    old + (self.base.get(id) - old) * alpha.clamp(0., 1.);
            }
        }
        self.chunk = 0;
    }
    fn v(&self, id: usize, lane: usize) -> f32 {
        self.values
            .get(id)
            .and_then(|r| r.get(lane))
            .copied()
            .unwrap_or(0.)
    }
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        if vel == 0 {
            self.note_off(pitch);
            return;
        }
        self.current_event = age;
        self.last_hz = 440. * 2_f32.powf((pitch as f32 + self.live.get(6) - 69.) / 12.);
        let voicing = if P::KIND == 0 {
            self.live.get(22).round() as usize
        } else {
            0
        };
        let intervals = chord(voicing);
        if P::KIND == 1 && self.live.get(27) > 0.5 {
            self.release_all();
            for lane in 0..N {
                self.amp.configure(lane, self.sr, 0., 5., 0., 2.);
                self.amp.off(lane);
            }
        }
        for (i, &interval) in intervals.iter().enumerate() {
            let inversion = if P::KIND == 0 {
                self.live.get(23).round() as usize
            } else {
                0
            };
            let open = if P::KIND == 0 {
                (self.live.get(24) * 12.).round() as i32
            } else {
                0
            };
            let shift = interval
                + if i < inversion { 12 } else { 0 }
                + if i > 0 && i % 2 == 1 { open } else { 0 };
            self.trigger(
                pitch,
                (pitch as i32 + shift).clamp(0, 127) as u8,
                vel,
                age.wrapping_add(i as u64),
                i,
                intervals.len(),
            );
        }
    }
    fn trigger(
        &mut self,
        root: u8,
        pitch: u8,
        vel: u8,
        age: u64,
        chord_index: usize,
        chord_len: usize,
    ) {
        let lane = (0..N)
            .find(|&l| !self.amp.active(l) && self.tail[l] == 0)
            .unwrap_or_else(|| (0..N).min_by_key(|&l| self.age[l]).unwrap_or(0));
        self.steal[lane] = self.last[lane];
        self.sub_steal[lane] = self.sub_last[lane];
        self.sub_steal_left[lane] = if self.sub_last[lane].abs() > 1e-6 {
            64
        } else {
            0
        };
        self.steal_left[lane] = if self.last[lane].abs() > 1e-6 { 64 } else { 0 };
        self.pitch[lane] = pitch;
        self.root[lane] = root;
        self.age[lane] = age;
        self.event[lane] = self.current_event;
        self.held[lane] = true;
        self.velocity[lane] = vel as f32 / 127.;
        self.elapsed[lane] = 0;
        self.tail[lane] = if P::KIND == 2 { SIZE } else { 0 };
        self.note_locks[lane] = self.locks;
        for d in P::TABLE {
            if let Some(row) = self.values.get_mut(d.id as usize) {
                row[lane] = self.live.get(d.id);
            }
        }
        self.feedback[lane] = 0.;
        let phase = if P::KIND == 0 {
            self.v(18, lane)
        } else if P::KIND == 3 {
            self.v(28, lane)
        } else {
            0.
        };
        let seed = if P::KIND == 2 {
            self.v(28, lane) as u32
        } else {
            1709
        };
        self.osc.reset_lane(
            lane,
            phase,
            seed.wrapping_add((age as u32).wrapping_mul(31))
                .wrapping_add(lane as u32 + 1),
        );
        self.filter.reset_lane(lane);
        self.modes.reset_lane(lane);
        if P::KIND == 2 {
            self.spectral.reset_lane(lane);
            for row in &mut self.amp_delay {
                row[lane] = 0.;
            }
        }
        self.configure_lane(lane);
        self.amp.on(lane);
        self.motion.on(lane);
        self.chunk = 0;
        let _ = (chord_index, chord_len);
    }
    pub fn note_off(&mut self, pitch: u8) {
        for lane in 0..N {
            if self.root[lane] == pitch && self.held[lane] {
                self.held[lane] = false;
                self.amp.off(lane);
            }
        }
    }
    pub fn release_all(&mut self) {
        for lane in 0..N {
            self.held[lane] = false;
            self.amp.off(lane);
        }
    }
    fn configure_lane(&mut self, lane: usize) {
        let values: [f32; PARAMS] = std::array::from_fn(|id| self.v(id, lane));
        let v = |id: usize| values[id];
        let hz = 440. * 2_f32.powf((self.pitch[lane] as f32 + v(6) - 69.) / 12.);
        self.frequency[lane] = hz;
        self.amp.configure(lane, self.sr, v(0), v(1), v(2), v(3));
        let t = match P::KIND {
            0 => v(15),
            2 => v(22),
            3 => v(18),
            _ => v(1),
        };
        self.motion.configure(lane, self.sr, 0., t, 0., t);
        let env = (-6.907755 * self.elapsed[lane] as f32 / (self.sr * v(11).max(1.) * 0.001)).exp();
        let drift = if P::KIND == 0 {
            v(27) * (::core::f32::consts::TAU * self.elapsed[lane] as f32 / self.sr * v(28)).sin()
        } else {
            0.
        };
        let mut cutoff = v(8)
            * 2_f32.powf(((self.pitch[lane] as f32 - 60.) * v(12) + env * v(10) + drift) / 12.);
        if P::KIND == 2 {
            cutoff = cutoff.min(v(20));
            self.source_gain[lane] = prism_noise_gain(v(20), v(14), v(15), self.sr);
        }
        self.filter.configure(lane, self.sr, cutoff, v(9));
        if P::KIND == 1 {
            let mut ratios = material_ratios(v(13), v(14));
            for (i, ratio) in ratios.iter_mut().enumerate() {
                let jitter = ((self.age[lane].wrapping_mul(31).wrapping_add(i as u64 * 73) % 257)
                    as f32
                    / 256.
                    - 0.5)
                    * v(19);
                *ratio *= 2_f32.powf(jitter * 0.035);
            }
            let mut times = [0.; 6];
            let mut weights = [0.; 6];
            for m in 0..6 {
                times[m] = v(1) * 0.001 / (1. + v(15) * (m as f32).powi(2) * 0.8)
                    * (220. / hz.max(20.)).powf(0.2);
                weights[m] = v(21 + m)
                    * (0.2
                        + (::core::f32::consts::PI * (m + 1) as f32 * v(18))
                            .sin()
                            .abs())
                    / (m + 1) as f32;
            }
            self.modes
                .configure(lane, self.sr, hz, &ratios, &times, &weights);
        }
    }
    fn configure_fx(&mut self) {
        let patch = self.live;
        let v = |id| patch.get(id);
        match P::KIND {
            0 => {
                for ch in 0..2 {
                    self.comb[ch].set_delay(self.sr / (self.last_hz * v(29)).max(20.));
                    self.comb[ch].set_feedback(v(30));
                    self.comb[ch].set_damp(self.sr, v(31));
                    self.tilt[ch].prepare(self.sr, 1200., v(34));
                    self.echo[ch].set_delay(self.sr * v(41) * 0.001);
                    self.echo[ch].set_feedback(v(42));
                    self.echo[ch].set_damp(self.sr, v(43));
                }
                self.shape.configure(Shape::Fold, 1. + v(33), v(35), 1.);
            }
            1 => {
                for ch in 0..2 {
                    let f = (220. + (self.last_hz - 220.) * v(32)) * v(30);
                    self.disperse[ch].prepare(
                        self.sr,
                        f,
                        2_f32.powf(v(31)),
                        (v(29) * 8.).round() as u32,
                    );
                }
                self.bloom.set_size(v(33));
                self.bloom.set_decay(v(34));
                self.bloom.set_damping(v(35));
                self.bloom.set_modulation(0.1);
            }
            3 => {
                for ch in 0..2 {
                    self.echo[ch].set_delay((self.sr * v(30) * 0.001 - SIZE as f32).max(1.));
                    self.echo[ch].set_feedback(0.);
                    self.echo[ch].set_damp(self.sr, 10000.);
                    self.tilt[ch].prepare(self.sr, v(34), v(33));
                }
                self.shape.configure(Shape::SoftClip, 1. + v(35), 0., 1.);
            }
            _ => {}
        }
    }
    fn table_read(&self, w: usize, phase: f32, hz: f32) -> f32 {
        let w = w.min(7);
        let table = self
            .tables
            .get(self.offsets[w]..self.offsets[w] + self.lens[w])
            .unwrap_or(&[]);
        mip_cycle(table, phase, hz, self.sr)
    }
    fn sine(&self, op: usize, lane: usize, pm: f32) -> f32 {
        self.osc
            .sine(op, lane, pm, self.tables.get(..TABLE_LEN).unwrap_or(&[]))
    }
    fn source(&mut self, lane: usize, env: f32) -> f32 {
        let hz = self.frequency[lane];
        let n = self.osc.noise(lane);
        let value = match P::KIND {
            0 => {
                let morph = (self.v(13, lane)
                    + self.v(14, lane) * env
                    + self.v(19, lane) * (self.velocity[lane] - 0.5))
                    .clamp(0., 7.);
                let rough = self.v(20, lane);
                let metal = ((morph - 3.) / 3.).clamp(0., 1.);
                let noise = ((morph - 5.2) / 1.8).clamp(0., 1.);
                let pm = self.sine(3, lane, 0.) * metal * rough * 0.18;
                let phase = self.osc.phase(0, lane) + pm;
                let mut wave = wave_pair(morph, phase, hz, |w, p, h| self.table_read(w, p, h));
                if (morph - 3.).abs() < 1. {
                    let pw = self.v(21, lane);
                    let pulse = self.table_read(2, phase, hz) - self.table_read(2, phase + pw, hz);
                    let amount = (1. - (morph - 3.).abs()) * (2. * (pw - 0.5).abs());
                    wave = wave * (1. - amount) + pulse * amount;
                }
                let det = self.v(17, lane);
                let drift = self.v(25, lane)
                    * (::core::f32::consts::TAU * self.elapsed[lane] as f32 / self.sr
                        * self.v(26, lane)
                        + (lane as f32 * 1.79))
                        .sin();
                let other = wave_pair(morph, self.osc.phase(1, lane) + pm, hz, |w, p, h| {
                    self.table_read(w, p, h)
                });
                let mixed = if det + drift.abs() > 0.0001 {
                    (wave + other) * 0.5
                } else {
                    wave
                };
                self.osc.advance(0, lane, hz, self.sr);
                self.osc
                    .advance(1, lane, hz * 2_f32.powf((det + drift) / 1200.), self.sr);
                self.osc.advance(2, lane, hz * 0.5, self.sr);
                self.osc
                    .advance(3, lane, hz * (1. + metal * 0.41421356), self.sr);
                mixed * (1. - noise).sqrt()
                    + n * noise.sqrt() * 0.65
                    + self.sine(2, lane, 0.) * self.v(16, lane) * 0.5
            }
            1 => {
                let contact = (self.v(28, lane) * 0.001 * self.sr).max(1.);
                let age = self.elapsed[lane] as f32;
                let strike = self.v(16, lane);
                let hardness = self.v(17, lane);
                let impact = if age < contact {
                    let w = (1. - age / contact).powf(1. + hardness * 6.);
                    (if self.elapsed[lane] == 0 {
                        1. - strike
                    } else {
                        0.
                    }) + n * strike * w / contact.sqrt()
                } else {
                    0.
                };
                let sustained = if self.held[lane] {
                    n * self.v(20, lane) * 0.01
                } else {
                    0.
                };
                impact + sustained
            }
            2 => {
                let shape = self.v(13, lane).round() as u32;
                let phase = self.osc.phase(0, lane);
                let v = match shape {
                    1 => self.table_read(2, phase, hz),
                    2 => self.table_read(3, phase, hz),
                    _ => n * self.source_gain[lane],
                };
                self.osc.advance(0, lane, hz, self.sr);
                v
            }
            _ => {
                let disorder = self.v(22, lane);
                let ratio = glass_ratio(
                    self.v(14, lane),
                    self.v(15, lane),
                    self.v(16, lane),
                    disorder,
                );
                let pitch_env = self.v(23, lane)
                    * (-6.907755 * self.elapsed[lane] as f32
                        / (self.sr * self.v(24, lane) * 0.001))
                        .exp();
                let fc = hz * 2_f32.powf(pitch_env / 12.);
                let fm = fc * ratio;
                let velocity = 1. - self.v(21, lane) + self.v(21, lane) * self.velocity[lane];
                let index = self.v(17, lane)
                    * (self.v(19, lane) + (1. - self.v(19, lane)) * env)
                    * velocity;
                let guard = ((self.sr * 0.43 - fc) / fm.max(1.) - 2.).max(0.);
                let index = index.min(guard + index * (1. - self.v(25, lane)) * 0.1);
                let feedback = (self.v(20, lane) + disorder * 0.45).min(0.95);
                let algo = self.v(13, lane).round() as u32;
                // Operator C uses the existing prepared four-oscillator kernel.
                // No modulation graph, buffers or per-voice objects are added.
                // Zero index-B takes the exact legacy arithmetic path.
                let index_b = self.v(38, lane)
                    * (self.v(19, lane) + (1. - self.v(19, lane)) * env)
                    * velocity;
                let (b, a, parallel_pm) = if index_b > 0. {
                    let fm_b = fc * self.v(37, lane);
                    let guard_b = ((self.sr * 0.43 - fc.max(fm)) / fm_b.max(1.) - 2.).max(0.);
                    let index_b = index_b.min(guard_b + index_b * (1. - self.v(25, lane)) * 0.1);
                    let c = self.sine(2, lane, 0.) * index_b;
                    let cascade = self.v(39, lane);
                    let b = self.sine(
                        1,
                        lane,
                        self.feedback[lane] * feedback * 0.35
                            + c * cascade / ::core::f32::consts::TAU,
                    );
                    let a = self.sine(
                        0,
                        lane,
                        (b * index + c * (1. - cascade)) / ::core::f32::consts::TAU,
                    );
                    (b, a, c * (1. - cascade) / ::core::f32::consts::TAU)
                } else {
                    let b = self.sine(1, lane, self.feedback[lane] * feedback * 0.35);
                    let a = self.sine(0, lane, b * index / ::core::f32::consts::TAU);
                    (b, a, 0.)
                };
                // Keep phase running even while C is inaudible; locks can open
                // its depth without an arbitrary oscillator-phase discontinuity.
                self.osc.advance(2, lane, fc * self.v(37, lane), self.sr);
                self.feedback[lane] = if algo == 2 { (a + b) * 0.5 } else { b };
                self.osc.advance(0, lane, fc, self.sr);
                self.osc.advance(1, lane, fm, self.sr);
                if algo == 1 {
                    let mix = index / (index + 1.);
                    self.sine(0, lane, parallel_pm) * (1. - mix * 0.5) + b * mix * 0.5
                } else {
                    a
                }
            }
        };
        value
    }
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        for (i, dst) in out.iter_mut().enumerate() {
            if self.chunk == 0 {
                for lane in 0..N {
                    if self.amp.active(lane) || self.tail[lane] > 0 {
                        self.configure_lane(lane);
                    }
                }
                self.configure_fx();
                self.chunk = CHUNK;
            }
            self.chunk -= 1;
            let mut amp = [[0.; N]; 1];
            let mut movement = [[0.; N]; 1];
            self.amp.process(&mut amp);
            self.motion.process(&mut movement);
            let mut frame = [[0.; N]; 1];
            for lane in 0..N {
                if self.amp.active(lane) {
                    frame[0][lane] = self.source(lane, movement[0][lane]);
                    self.elapsed[lane] = self.elapsed[lane].saturating_add(1);
                } else if self.tail[lane] > 0 {
                    self.tail[lane] -= 1;
                }
            }
            if P::KIND == 1 {
                self.modes.process(&mut frame);
            }
            self.filter.process(&mut frame);
            if P::KIND == 2 {
                let mut settings = [Settings::default(); N];
                let mut enabled = [false; N];
                for lane in 0..N {
                    enabled[lane] = self.amp.active(lane) || self.tail[lane] > 0;
                    let t = self.elapsed[lane] as f32 / self.sr;
                    let v = |id| self.v(id, lane);
                    settings[lane] = Settings {
                        hz: self.frequency[lane],
                        sieve: (v(14) + v(21) * movement[0][lane]).clamp(0., 1.),
                        width: v(15),
                        shift: v(16) + v(23) * (::core::f32::consts::TAU * t * v(24)).sin(),
                        tilt: v(17),
                        blur: v(18),
                        freeze: (v(19) + v(25) * (-6.907755 * t / (v(26) * 0.001)).exp())
                            .clamp(0., 1.),
                        rough: v(27),
                        low_ms: v(29),
                        high_ms: v(30),
                        feed: v(31),
                        smear: v(32),
                        halo_decay: v(33),
                        halo_tilt: v(34),
                        halo_damp: v(35),
                        halo_mix: v(36),
                        pitch_ratio: 1.,
                    };
                }
                frame[0] = self.spectral.tick(frame[0], &settings, &enabled);
                let old = self
                    .amp_delay
                    .get(self.delay_pos)
                    .copied()
                    .unwrap_or([0.; N]);
                if let Some(row) = self.amp_delay.get_mut(self.delay_pos) {
                    *row = amp[0];
                }
                amp[0] = old;
                self.delay_pos = (self.delay_pos + 1) % SIZE;
            }
            let mut l = 0.;
            let mut r = 0.;
            let mut sub = 0.;
            for lane in 0..N {
                let mut sample = frame[0][lane]
                    * amp[0][lane]
                    * (1. - self.v(4, lane) + self.v(4, lane) * self.velocity[lane])
                    * self.v(5, lane)
                    * 0.24;
                if self.steal_left[lane] > 0 {
                    let a = self.steal_left[lane] as f32 / 64.;
                    sample = sample * (1. - a) + self.steal[lane] * a;
                    self.steal_left[lane] -= 1;
                }
                self.last[lane] = sample;
                if P::KIND == 3 {
                    // Sine stays mono and outside filter/tilt/shimmer, but still
                    // follows note gates, velocity, level, tune and voice stealing.
                    // Downstream track FX can of course still process this sum.
                    sample *= self.v(42, lane);
                    let mut foundation = if self.amp.active(lane) {
                        let sine = self.sine(3, lane, 0.);
                        self.osc.advance(
                            3,
                            lane,
                            self.frequency[lane] * 2_f32.powf(self.v(41, lane) / 12.),
                            self.sr,
                        );
                        sine * self.v(40, lane)
                            * amp[0][lane]
                            * (1. - self.v(4, lane) + self.v(4, lane) * self.velocity[lane])
                            * self.v(5, lane)
                            * 0.24
                            * ::core::f32::consts::FRAC_1_SQRT_2
                    } else {
                        0.
                    };
                    if self.sub_steal_left[lane] > 0 {
                        let a = self.sub_steal_left[lane] as f32 / 64.;
                        foundation = foundation * (1. - a) + self.sub_steal[lane] * a;
                        self.sub_steal_left[lane] -= 1;
                    }
                    self.sub_last[lane] = foundation;
                    sub += foundation;
                }
                let width = self.v(7, lane);
                let pan_motion = if P::KIND == 3 {
                    self.v(26, lane)
                        * 0.5
                        * (::core::f32::consts::TAU * self.elapsed[lane] as f32 / self.sr
                            * self.v(27, lane))
                        .sin()
                } else {
                    0.
                };
                let pan = (((lane % 4) as f32 / 3. - 0.5) * width + pan_motion).clamp(-0.5, 0.5);
                let angle = (pan + 0.5) * ::core::f32::consts::FRAC_PI_2;
                l += sample * angle.cos();
                r += sample * angle.sin();
            }
            let (l, r) = self.fx(l, r);
            let g = gain.next();
            *dst = (l + sub) * g;
            if let Some(dst) = self.right.get_mut(at + i) {
                *dst = (r + sub) * g;
            }
        }
    }
    fn fx(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mut x = [l, r];
        let patch = self.live;
        let v = |id| patch.get(id);
        match P::KIND {
            0 => {
                for ch in 0..2 {
                    let mut wet = [x[ch]];
                    self.comb[ch].process(&mut wet, &mut self.comb_store[ch]);
                    x[ch] += wet[0] * v(32) * 0.5;
                    let dry = x[ch];
                    let mut tilted = [dry];
                    self.tilt[ch].process(&mut tilted);
                    x[ch] =
                        dry + (self.shape.shape(tilted[0]) - self.shape.shape(0.) - dry) * v(36);
                }
                let mut ensemble_l = 0.;
                let mut ensemble_r = 0.;
                let mono = (x[0] + x[1]) * 0.5;
                for c in 0..3 {
                    let phase = self.fx_phase * v(38) as f64 + c as f64 / 3.;
                    let d = (15.
                        + v(37)
                            * ((::core::f64::consts::TAU * phase).sin() * 0.7
                                + (::core::f64::consts::TAU * phase * 10.).sin() * 0.3)
                                as f32)
                        * 0.001
                        * self.sr;
                    let mut wet = [mono];
                    self.ensemble[c].process_modulated(&mut wet, &mut self.ensemble_store[c], &[d]);
                    let pan = (c as f32 - 1.) * v(39);
                    ensemble_l += wet[0] * (1. - pan) * 0.25;
                    ensemble_r += wet[0] * (1. + pan) * 0.25;
                }
                x[0] += ensemble_l * v(40);
                x[1] += ensemble_r * v(40);
                for ch in 0..2 {
                    let mut wet = [x[ch]];
                    self.echo[ch].process(&mut wet, &mut self.echo_store[ch]);
                    x[ch] += wet[0] * v(44);
                }
            }
            1 => {
                for ch in 0..2 {
                    let mut wet = [x[ch]];
                    self.disperse[ch].process(&mut wet);
                    x[ch] += (wet[0] - x[ch]) * v(29);
                }
                let input = [(x[0] + x[1]) * 0.5];
                let mut wet_l = [0.];
                let mut wet_r = [0.];
                self.bloom
                    .process(&input, &mut wet_l, &mut wet_r, &mut self.bloom_store);
                x[0] += wet_l[0] * v(36);
                x[1] += wet_r[0] * v(36);
            }
            3 => {
                let mut input = [0.; N];
                for ch in 0..2 {
                    let mut delayed = [x[ch] + self.shimmer_feedback[ch] * v(31)];
                    self.echo[ch].process(&mut delayed, &mut self.echo_store[ch]);
                    input[ch] = delayed[0];
                }
                let mut settings = [Settings::default(); N];
                let mut enabled = [false; N];
                for ch in 0..2 {
                    settings[ch].pitch_ratio = 2_f32.powf(v(29) / 12.);
                    enabled[ch] = true;
                }
                let shifted = self.spectral.tick(input, &settings, &enabled);
                for ch in 0..2 {
                    self.shimmer_feedback[ch] = shifted[ch].clamp(-4., 4.);
                    x[ch] += shifted[ch] * v(32);
                    let dry = x[ch];
                    let mut wet = [x[ch]];
                    self.tilt[ch].process(&mut wet);
                    x[ch] = dry + (self.shape.shape(wet[0]) - dry) * v(36);
                }
            }
            _ => {}
        }
        self.fx_phase += 1. / self.sr as f64;
        for ch in 0..2 {
            let mut y = [x[ch]];
            self.dc[ch].process(&mut y);
            x[ch] = if y[0].is_finite() { y[0] } else { 0. };
        }
        (x[0], x[1])
    }
    pub fn right(&self, len: usize) -> &[f32] {
        self.right.get(..len.min(self.right.len())).unwrap_or(&[])
    }
}

pub fn chord(index: usize) -> &'static [i32] {
    match index {
        1 => &[0, 7],
        2 => &[0, 4, 7],
        3 => &[0, 3, 7],
        4 => &[0, 4, 7, 11],
        5 => &[0, 3, 7, 10],
        6 => &[0, 4, 7, 10],
        7 => &[0, 5, 7],
        8 => &[0, 7, 10, 14],
        _ => &[0],
    }
}
/// Fixed noise-source calibration, not an output follower. White noise has
/// variance 1/3; a Butterworth source band has equivalent noise bandwidth
/// pi/(2*sqrt(2)) times its cutoff. The raised-cosine sieve has known mean-square
/// mask power. Aim for 0.5 RMS before the player's cutoff/envelope, capped at 4x
/// so very narrow bands remain naturally quieter. Blur, frequency reassignment,
/// user filter/Q, envelopes and FX deliberately receive no compensation.
fn prism_noise_gain(band: f32, sieve: f32, width: f32, sr: f32) -> f32 {
    let width = width.clamp(0.05, 1.);
    let a = 1. - sieve.clamp(0., 1.);
    let b = sieve.clamp(0., 1.) / width.sqrt();
    let mask_power = a * a + a * b * width + b * b * 3. * width / 8.;
    let band_fraction = (1.1107207 * band.max(100.) / (0.5 * sr.max(8000.))).min(1.);
    (0.5 / (mask_power * band_fraction / 3.).sqrt()).clamp(0.65, 4.)
}

pub fn material_ratios(material: f32, inharm: f32) -> [f32; 6] {
    const RATIOS: [[f32; 6]; 5] = [
        [1., 2.756, 5.404, 8.933, 13.344, 18.645],
        [1., 1.593, 2.136, 2.296, 2.653, 2.918],
        [1., 2., 2.756, 4.07, 5.4, 7.22],
        [1., 2.32, 4.25, 6.63, 9.38, 12.22],
        [1., 3., 5., 7., 9., 11.],
    ];
    let m = material.clamp(0., 4.);
    let lo = m.floor() as usize;
    let hi = (lo + 1).min(4);
    let t = m - lo as f32;
    std::array::from_fn(|i| {
        let shape = RATIOS[lo][i] * 2_f32.powf((RATIOS[hi][i] / RATIOS[lo][i]).log2() * t);
        let h = (i + 1) as f32;
        h * 2_f32.powf((shape / h).log2() * inharm)
    })
}
pub fn glass_ratio(ratio: f32, coarse: f32, fine: f32, disorder: f32) -> f32 {
    let ratio = if coarse > 0.5 {
        ratio.round().max(1.)
    } else {
        ratio
    };
    (ratio + disorder * 0.41421356) * 2_f32.powf(fine / 1200.)
}

/// The phase-coherent neighboring-table interpolation shared with the source hero.
pub fn wave_pair(morph: f32, phase: f32, hz: f32, read: impl Fn(usize, f32, f32) -> f32) -> f32 {
    let position = morph.clamp(0., 7.);
    let lo = position.floor() as usize;
    let hi = (lo + 1).min(7);
    let blend = position - lo as f32;
    let a = read(lo, phase, hz);
    a + (read(hi, phase, hz) - a) * blend
}
pub fn hero<P: Patch>(
    p: P,
    keys: crate::pages::KeyTable,
    page: &str,
    selected: Option<u32>,
) -> Option<crate::pages::Hero> {
    super::picture::hero(p, keys, page, selected)
}

#[cfg(test)]
pub fn test_bank<P: Patch>() {
    use crate::params::ParamDef;
    let mut bank = Bank::<P>::new();
    bank.prepare(48000., 4096, P::default());
    let mut output = [0.; 4096];
    let mut ramp = Ramp::across(1., 1., 1);
    bank.render(&mut output, 0, &mut ramp);
    assert!(output.iter().all(|x| *x == 0.));
    bank.note_on(60, 100, 1);
    bank.render(&mut output, 0, &mut ramp);
    assert!(output.iter().map(|x| x * x).sum::<f32>() > 1e-5);
    assert!(output.iter().all(|x| x.is_finite()));
    let mut a = Bank::<P>::new();
    a.prepare(48000., 4096, P::default());
    let mut b = Bank::<P>::new();
    b.prepare(48000., 4096, P::default());
    a.note_on(48, 100, 1);
    b.note_on(48, 100, 1);
    let mut x = [0.; 4096];
    let mut y = x;
    a.render(&mut x, 0, &mut Ramp::across(1., 1., 1));
    let mut offset = 0;
    for count in [100, 257, 155, 3584] {
        b.render(
            &mut y[offset..offset + count],
            offset,
            &mut Ramp::across(1., 1., 1),
        );
        offset += count;
    }
    assert_eq!(x, y);
    assert_eq!(a.right(4096), b.right(4096));
    assert_no_alloc::assert_no_alloc(|| {
        bank.note_off(60);
        bank.note_on(64, 127, 2);
        bank.plock(13, Some(1.));
        bank.note_on(67, 100, 3);
        bank.plock(13, None);
        bank.render(&mut output[..257], 0, &mut ramp);
        bank.release_all();
    });
    for ParamDef { id, min, max, .. } in P::TABLE {
        for value in [*min, *max] {
            let mut p = P::default();
            p.set(*id, value);
            assert_eq!(p.get(*id), value);
            bank.reset();
            bank.set_param(*id, value);
            bank.note_on(36, 127, 1);
            bank.render(&mut output, 0, &mut Ramp::across(1., 1., 1));
            assert!(
                output.iter().all(|x| x.is_finite()),
                "kind{} id{} value{}",
                P::KIND,
                id,
                value
            );
            bank.set_param(*id, P::default().get(*id));
        }
    }
    let mut p = P::default();
    let old = p.get(0);
    p.set(0, f32::NAN);
    assert_eq!(p.get(0), old);
    bank.reset();
    bank.plock(13, Some(P::TABLE[13].min));
    bank.note_on(60, 100, 1);
    bank.plock(13, Some(P::TABLE[13].max));
    bank.note_on(64, 100, 2);
    assert_eq!(bank.v(13, 0), P::TABLE[13].min);
    assert_eq!(bank.v(13, 1), P::TABLE[13].max);
    bank.plock(13, None);
    assert_eq!(bank.live.get(13), bank.base.get(13));
}
#[cfg(test)]
pub fn test_pictures<P: Patch>(keys: crate::pages::KeyTable) {
    for key in keys.into_iter().flatten() {
        for page in key.subpages {
            for id in page.slots.into_iter().flatten() {
                let h = hero(P::default(), keys, page.title, Some(id)).expect("picture");
                assert!(!h.series.is_empty());
                assert!(h.series.iter().any(|s| s.lit));
                assert!(
                    h.series
                        .iter()
                        .flat_map(|s| &s.points)
                        .all(|(x, y)| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y))
                );
            }
        }
    }
}

pub fn mip_cycle(table: &[f32], phase: f32, hz: f32, sr: f32) -> f32 {
    let levels = table.len() / TABLE_LEN;
    if levels == 0 {
        return 0.;
    }
    let level = (hz.abs() * 1024. / sr)
        .max(1.)
        .log2()
        .clamp(0., (levels - 1) as f32);
    let lo = level.floor() as usize;
    let hi = (lo + 1).min(levels - 1);
    let read = |i| {
        table
            .get(i * TABLE_LEN..(i + 1) * TABLE_LEN)
            .map(|t| core::read_cycle(phase, t))
            .unwrap_or(0.)
    };
    let a = read(lo);
    a + (read(hi) - a) * (level - lo as f32)
}

#[cfg(test)]
mod musical_claims {
    use super::*;
    fn render<P: Patch>(p: P, pitch: u8, samples: usize) -> Vec<f32> {
        let mut bank = Bank::<P>::new();
        bank.prepare(48000., 257, p);
        bank.note_on(pitch, 127, 1);
        let mut out = vec![0.; samples];
        for chunk in out.chunks_mut(257) {
            bank.render(chunk, 0, &mut Ramp::across(1., 1., 1));
        }
        out
    }
    fn spectrum(x: &[f32]) -> Vec<f32> {
        let n = 4096;
        let mut fft = crate::dsp::fft::RealFft::new();
        fft.prepare(n);
        let mut input = vec![0.; n];
        for (i, s) in input.iter_mut().enumerate() {
            *s = x.get(i).copied().unwrap_or(0.)
                * (0.5 - 0.5 * (::core::f32::consts::TAU * i as f32 / n as f32).cos());
        }
        let mut re = vec![0.; n / 2 + 1];
        let mut im = re.clone();
        let mut scratch = vec![0.; crate::dsp::fft::RealFft::scratch_len(n)];
        fft.forward(&input, &mut re, &mut im, &mut scratch);
        re.iter().zip(&im).map(|(r, i)| r * r + i * i).collect()
    }
    fn centroid(x: &[f32]) -> f32 {
        let s = spectrum(x);
        let sum = s.iter().sum::<f32>();
        s.iter()
            .enumerate()
            .map(|(i, p)| i as f32 * 48000. / 4096. * p)
            .sum::<f32>()
            / sum.max(1e-20)
    }
    fn peak_hz(x: &[f32]) -> f32 {
        let s = spectrum(x);
        s.iter()
            .enumerate()
            .skip(1)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i as f32 * 48000. / 4096.)
            .unwrap_or(0.)
    }
    #[test]
    fn prism_noise_has_useful_level_without_undoing_the_played_filter() {
        let mut p = crate::audio::prism_voice::PrismVoiceParams {
            attack: 0.,
            sustain: 1.,
            cutoff: 20000.,
            width: 0.,
            halo_mix: 0.,
            ..Default::default()
        };
        let rms = |x: &[f32]| {
            (x[16384..].iter().map(|v| v * v).sum::<f32>() / (x.len() - 16384) as f32).sqrt()
        };
        let open = rms(&render(p, 60, 32768));
        assert!((0.025..0.10).contains(&open), "starter noise RMS {open}");
        p.cutoff = 200.;
        let closed = rms(&render(p, 60, 32768));
        assert!(
            closed < open * 0.45,
            "filter lost agency: {open} -> {closed}"
        );

        p.cutoff = 20000.;
        p.sieve = 0.;
        p.blur = 0.;
        let noise = rms(&render(p, 60, 32768));
        p.source = 1.;
        let saw = rms(&render(p, 60, 32768));
        assert!(
            (0.6..1.5).contains(&(noise / saw)),
            "noise {noise}, saw {saw}"
        );
        // Narrow-band gain must have a hard finite bound; no adaptive follower
        // is allowed to restore energy removed by sound design or silence.
        for sr in [8000., 44100., 48000., 96000., 192000.] {
            for band in [100., 8000., 16000.] {
                for width in [0.05, 0.2, 1.] {
                    for sieve in [0., 0.55, 1.] {
                        assert!((0.65..=4.).contains(&prism_noise_gain(band, sieve, width, sr)));
                    }
                }
            }
        }
    }

    #[test]
    fn table_scan_moves_from_metal_toward_sine() {
        let p = crate::audio::table::TableParams {
            morph: 0.,
            scan: 6.,
            scan_time: 1500.,
            attack: 0.,
            sustain: 1.,
            cutoff: 20000.,
            sub: 0.,
            detune: 0.,
            ensemble_mix: 0.,
            ..Default::default()
        };
        let x = render(p, 48, 48000);
        let early = centroid(&x[256..]);
        let late = centroid(&x[40000..]);
        assert!(early > late * 1.5, "{early} -> {late}");
    }
    #[test]
    fn table_air_has_broadband_energy_and_sine_is_concentrated() {
        let mut p = crate::audio::table::TableParams {
            morph: 0.,
            attack: 0.,
            sustain: 1.,
            cutoff: 20000.,
            sub: 0.,
            detune: 0.,
            ensemble_mix: 0.,
            ..Default::default()
        };
        let sine = render(p, 60, 10000);
        p.morph = 7.;
        let air = render(p, 60, 10000);
        let a = spectrum(&sine[4096..]);
        let b = spectrum(&air[4096..]);
        let top = |v: &[f32]| v.iter().copied().fold(0., f32::max) / v.iter().sum::<f32>();
        assert!(top(&a) > 0.3);
        assert!(top(&b) < 0.05);
        assert!(centroid(&air[4096..]) > centroid(&sine[4096..]) * 8.);
    }
    #[test]
    fn ring_material_moves_a_measured_partial() {
        let mut p = crate::audio::ring::RingParams {
            attack: 0.,
            sustain: 1.,
            decay: 4000.,
            material: 0.,
            inharm: 1.,
            spread: 0.,
            partial1: 0.,
            partial2: 1.,
            partial3: 0.,
            partial4: 0.,
            partial5: 0.,
            partial6: 0.,
            bloom_mix: 0.,
            cutoff: 20000.,
            ..Default::default()
        };
        let a = render(p, 60, 10000);
        p.material = 4.;
        let b = render(p, 60, 10000);
        let fa = peak_hz(&a[4096..]);
        let fb = peak_hz(&b[4096..]);
        assert!((fa - 261.62555 * 2.756).abs() < 12., "{fa}");
        assert!((fb - 261.62555 * 3.).abs() < 12., "{fb}");
        assert!(fb - fa > 40.);
    }
    #[test]
    fn ring_feed_sustains_a_held_resonator() {
        let mut p = crate::audio::ring::RingParams {
            attack: 0.,
            sustain: 1.,
            decay: 100.,
            feed: 0.,
            bloom_mix: 0.,
            ..Default::default()
        };
        let a = render(p, 60, 48000);
        p.feed = 1.;
        let b = render(p, 60, 48000);
        let power = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        assert!(power(&b[40000..]) > power(&a[40000..]) * 100. + 1e-7);
    }
    #[test]
    fn ring_reset_matches_a_fresh_voice_after_a_different_bloom_tail() {
        use crate::audio::ring::RingParams;
        let previous = RingParams {
            feed: 0.8,
            bloom_size: 1.7,
            bloom_decay: 8.,
            bloom_mix: 1.,
            ..Default::default()
        };
        let target = RingParams {
            bloom_size: 0.6,
            bloom_decay: 3.,
            bloom_mix: 0.8,
            ..Default::default()
        };
        let mut used = Bank::<RingParams>::new();
        used.prepare(48000., 257, previous);
        used.note_on(60, 100, 0);
        let mut scratch = [0.; 257];
        for _ in 0..127 {
            used.render(&mut scratch, 0, &mut Ramp::across(1., 1., 1));
        }
        assert_no_alloc::assert_no_alloc(|| used.reset());
        for p in RingParams::TABLE {
            used.set_param(p.id, target.get(p.id));
        }
        let mut fresh = Bank::<RingParams>::new();
        fresh.prepare(48000., 257, target);
        used.note_on(36, 64, 0);
        fresh.note_on(36, 64, 0);
        for block in 0..240 {
            let mut expected = [0.; 257];
            used.render(&mut scratch, 0, &mut Ramp::across(1., 1., 1));
            fresh.render(&mut expected, 0, &mut Ramp::across(1., 1., 1));
            assert_eq!(scratch, expected, "left at block {block}");
            assert_eq!(used.right(257), fresh.right(257), "right at block {block}");
        }
    }
    #[test]
    fn glass_zero_index_is_the_same_carrier_at_any_ratio() {
        let mut p = crate::audio::glass::GlassParams {
            index: 0.,
            feedback: 0.,
            attack: 0.,
            sustain: 1.,
            cutoff: 20000.,
            ..Default::default()
        };
        let a = render(p, 60, 8192);
        p.ratio = 7.31;
        let b = render(p, 60, 8192);
        assert_eq!(a, b);
        assert!((peak_hz(&a[4096..]) - 261.62555).abs() < 12.);
    }
    #[test]
    fn glass_index_envelope_lowers_centroid() {
        let p = crate::audio::glass::GlassParams {
            index: 8.,
            i_decay: 600.,
            i_sustain: 0.,
            feedback: 0.,
            attack: 0.,
            sustain: 1.,
            cutoff: 20000.,
            ..Default::default()
        };
        let x = render(p, 60, 48000);
        let first = centroid(&x[256..]);
        let last = centroid(&x[40000..]);
        assert!(first > last * 1.5, "{first} -> {last}");
    }
    #[test]
    fn locked_level_can_sound_from_a_zero_base_and_old_notes_keep_their_locks() {
        let p = crate::audio::table::TableParams {
            level: 0.,
            ensemble_mix: 0.,
            ..Default::default()
        };
        let mut bank = Bank::new();
        bank.prepare(48000., 4096, p);
        bank.plock(5, Some(1.));
        bank.note_on(60, 127, u64::MAX);
        bank.plock(5, None);
        let mut x = [0.; 4096];
        assert_no_alloc::assert_no_alloc(|| bank.render(&mut x, 0, &mut Ramp::across(1., 1., 1)));
        assert!(x.iter().map(|x| x * x).sum::<f32>() > 1e-5);
        assert_eq!(bank.base.get(5), 0.);
        assert_eq!(bank.v(5, 0), 1.);
        bank.set_param(5, 0.2);
        assert_eq!(bank.v(5, 0), 1.);
    }
    #[test]
    fn a_released_frozen_prism_eventually_stops() {
        let p = crate::audio::prism_voice::PrismVoiceParams {
            freeze: 1.,
            attack: 0.,
            sustain: 1.,
            release: 10.,
            halo_mix: 0.,
            ..Default::default()
        };
        let mut bank = Bank::new();
        bank.prepare(48000., 1024, p);
        bank.note_on(60, 127, 1);
        let mut x = [0.; 1024];
        for _ in 0..8 {
            bank.render(&mut x, 0, &mut Ramp::across(1., 1., 1));
        }
        assert!(x.iter().map(|x| x * x).sum::<f32>() > 1e-5);
        bank.note_off(60);
        for _ in 0..16 {
            bank.render(&mut x, 0, &mut Ramp::across(1., 1., 1));
        }
        assert!(x.iter().all(|x| x.abs() < 1e-6));
    }
}
