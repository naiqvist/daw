//! ROM — the rompler. See `notes/20260909-rom-brief.md`.
//!
//! Two oscillators of resident factory multisamples, each with its own
//! key/velocity zone map, through a stereo filter with its own envelope,
//! an amp envelope, an LFO and an ensemble. VINTAGE puts the whole bank
//! back in 1988: the converter's rate, its bits, and a loop cut short.
//!
//! The bank is built green-side ([`bank::bank`]) and handed here as an
//! `Arc`: the audio thread only ever reads it, so switching multisamples
//! — even from a lock, on a step — is an index, never a file load.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod bank;

use crate::audio::graph::Ramp;
use crate::dsp::adsr::Adsr;
use crate::dsp::delay::DelayLine;
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::lfo::{Lfo, LfoShape, SampleHold};
use crate::dsp::lofi::Downsampler;
use crate::dsp::pan;
use crate::dsp::sample_read::{Loop, Method, SampleReader, Settings};
use crate::params::rom as p;
use bank::Bank;
use std::sync::Arc;

/// The most notes at once. VOICES on the Layer page caps it lower.
pub const VOICES: usize = 16;
/// The control chunk: coefficients move here, multiplies move per sample.
const CHUNK: usize = 32;
/// The ensemble's centre delay and its sweep, in milliseconds.
const CHORUS_CENTRE_MS: f32 = 9.0;
const CHORUS_SWEEP_MS: f32 = 4.0;

// ---------------------------------------------------------------- params

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RomParams {
    pub pcm1: f32,
    pub tune1: f32,
    pub fine1: f32,
    pub start1: f32,
    pub key1: f32,
    pub pan1: f32,
    pub loop1: f32,
    pub pcm2: f32,
    pub tune2: f32,
    pub fine2: f32,
    pub start2: f32,
    pub key2: f32,
    pub pan2: f32,
    pub loop2: f32,
    pub mode: f32,
    pub split: f32,
    pub velpt: f32,
    pub balance: f32,
    pub detune: f32,
    pub xfade: f32,
    pub glide: f32,
    pub voices: f32,
    pub ftype: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub fenv: f32,
    pub fkey: f32,
    pub fvel: f32,
    pub fattack: f32,
    pub fdecay: f32,
    pub fsustain: f32,
    pub frelease: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub vel: f32,
    pub keydec: f32,
    pub pan: f32,
    pub level: f32,
    pub shape: f32,
    pub rate: f32,
    pub fade: f32,
    pub lpitch: f32,
    pub lcut: f32,
    pub lamp: f32,
    pub ltrig: f32,
    pub crate_hz: f32,
    pub cdepth: f32,
    pub cwidth: f32,
    pub cmix: f32,
    pub vintage: f32,
    pub vrate: f32,
    pub vbits: f32,
    pub vloop: f32,
}

impl Default for RomParams {
    fn default() -> Self {
        Self {
            pcm1: p::TABLE[0].default,
            tune1: p::TABLE[1].default,
            fine1: p::TABLE[2].default,
            start1: p::TABLE[3].default,
            key1: p::TABLE[4].default,
            pan1: p::TABLE[5].default,
            loop1: p::TABLE[6].default,
            pcm2: p::TABLE[7].default,
            tune2: p::TABLE[8].default,
            fine2: p::TABLE[9].default,
            start2: p::TABLE[10].default,
            key2: p::TABLE[11].default,
            pan2: p::TABLE[12].default,
            loop2: p::TABLE[13].default,
            mode: p::TABLE[14].default,
            split: p::TABLE[15].default,
            velpt: p::TABLE[16].default,
            balance: p::TABLE[17].default,
            detune: p::TABLE[18].default,
            xfade: p::TABLE[19].default,
            glide: p::TABLE[20].default,
            voices: p::TABLE[21].default,
            ftype: p::TABLE[22].default,
            cutoff: p::TABLE[23].default,
            reso: p::TABLE[24].default,
            fenv: p::TABLE[25].default,
            fkey: p::TABLE[26].default,
            fvel: p::TABLE[27].default,
            fattack: p::TABLE[28].default,
            fdecay: p::TABLE[29].default,
            fsustain: p::TABLE[30].default,
            frelease: p::TABLE[31].default,
            attack: p::TABLE[32].default,
            decay: p::TABLE[33].default,
            sustain: p::TABLE[34].default,
            release: p::TABLE[35].default,
            vel: p::TABLE[36].default,
            keydec: p::TABLE[37].default,
            pan: p::TABLE[38].default,
            level: p::TABLE[39].default,
            shape: p::TABLE[40].default,
            rate: p::TABLE[41].default,
            fade: p::TABLE[42].default,
            lpitch: p::TABLE[43].default,
            lcut: p::TABLE[44].default,
            lamp: p::TABLE[45].default,
            ltrig: p::TABLE[46].default,
            crate_hz: p::TABLE[47].default,
            cdepth: p::TABLE[48].default,
            cwidth: p::TABLE[49].default,
            cmix: p::TABLE[50].default,
            vintage: p::TABLE[51].default,
            vrate: p::TABLE[52].default,
            vbits: p::TABLE[53].default,
            vloop: p::TABLE[54].default,
        }
    }
}

impl RomParams {
    pub fn get(&self, id: u32) -> f32 {
        match id {
            p::PCM1 => self.pcm1,
            p::TUNE1 => self.tune1,
            p::FINE1 => self.fine1,
            p::START1 => self.start1,
            p::KEY1 => self.key1,
            p::PAN1 => self.pan1,
            p::LOOP1 => self.loop1,
            p::PCM2 => self.pcm2,
            p::TUNE2 => self.tune2,
            p::FINE2 => self.fine2,
            p::START2 => self.start2,
            p::KEY2 => self.key2,
            p::PAN2 => self.pan2,
            p::LOOP2 => self.loop2,
            p::MODE => self.mode,
            p::SPLIT => self.split,
            p::VELPT => self.velpt,
            p::BALANCE => self.balance,
            p::DETUNE => self.detune,
            p::XFADE => self.xfade,
            p::GLIDE => self.glide,
            p::VOICES => self.voices,
            p::TYPE => self.ftype,
            p::CUTOFF => self.cutoff,
            p::RESO => self.reso,
            p::FENV => self.fenv,
            p::FKEY => self.fkey,
            p::FVEL => self.fvel,
            p::FATTACK => self.fattack,
            p::FDECAY => self.fdecay,
            p::FSUSTAIN => self.fsustain,
            p::FRELEASE => self.frelease,
            p::ATTACK => self.attack,
            p::DECAY => self.decay,
            p::SUSTAIN => self.sustain,
            p::RELEASE => self.release,
            p::VEL => self.vel,
            p::KEYDEC => self.keydec,
            p::PAN => self.pan,
            p::LEVEL => self.level,
            p::SHAPE => self.shape,
            p::RATE => self.rate,
            p::FADE => self.fade,
            p::LPITCH => self.lpitch,
            p::LCUT => self.lcut,
            p::LAMP => self.lamp,
            p::LTRIG => self.ltrig,
            p::CRATE => self.crate_hz,
            p::CDEPTH => self.cdepth,
            p::CWIDTH => self.cwidth,
            p::CMIX => self.cmix,
            p::VINTAGE => self.vintage,
            p::VRATE => self.vrate,
            p::VBITS => self.vbits,
            p::VLOOP => self.vloop,
            _ => 0.0,
        }
    }

    /// A letter from any surface: unknown ids are dropped, values are
    /// clamped into the table's range, and a non-finite value falls back
    /// to the default rather than poisoning the voice.
    pub fn set(&mut self, id: u32, value: f32) {
        let Some(def) = p::TABLE.get(id as usize) else {
            return;
        };
        let v = if value.is_finite() {
            value.clamp(def.min, def.max)
        } else {
            def.default
        };
        match id {
            p::PCM1 => self.pcm1 = v,
            p::TUNE1 => self.tune1 = v,
            p::FINE1 => self.fine1 = v,
            p::START1 => self.start1 = v,
            p::KEY1 => self.key1 = v,
            p::PAN1 => self.pan1 = v,
            p::LOOP1 => self.loop1 = v,
            p::PCM2 => self.pcm2 = v,
            p::TUNE2 => self.tune2 = v,
            p::FINE2 => self.fine2 = v,
            p::START2 => self.start2 = v,
            p::KEY2 => self.key2 = v,
            p::PAN2 => self.pan2 = v,
            p::LOOP2 => self.loop2 = v,
            p::MODE => self.mode = v,
            p::SPLIT => self.split = v,
            p::VELPT => self.velpt = v,
            p::BALANCE => self.balance = v,
            p::DETUNE => self.detune = v,
            p::XFADE => self.xfade = v,
            p::GLIDE => self.glide = v,
            p::VOICES => self.voices = v,
            p::TYPE => self.ftype = v,
            p::CUTOFF => self.cutoff = v,
            p::RESO => self.reso = v,
            p::FENV => self.fenv = v,
            p::FKEY => self.fkey = v,
            p::FVEL => self.fvel = v,
            p::FATTACK => self.fattack = v,
            p::FDECAY => self.fdecay = v,
            p::FSUSTAIN => self.fsustain = v,
            p::FRELEASE => self.frelease = v,
            p::ATTACK => self.attack = v,
            p::DECAY => self.decay = v,
            p::SUSTAIN => self.sustain = v,
            p::RELEASE => self.release = v,
            p::VEL => self.vel = v,
            p::KEYDEC => self.keydec = v,
            p::PAN => self.pan = v,
            p::LEVEL => self.level = v,
            p::SHAPE => self.shape = v,
            p::RATE => self.rate = v,
            p::FADE => self.fade = v,
            p::LPITCH => self.lpitch = v,
            p::LCUT => self.lcut = v,
            p::LAMP => self.lamp = v,
            p::LTRIG => self.ltrig = v,
            p::CRATE => self.crate_hz = v,
            p::CDEPTH => self.cdepth = v,
            p::CWIDTH => self.cwidth = v,
            p::CMIX => self.cmix = v,
            p::VINTAGE => self.vintage = v,
            p::VRATE => self.vrate = v,
            p::VBITS => self.vbits = v,
            p::VLOOP => self.vloop = v,
            _ => {}
        }
    }
}

impl RomParams {
    /// The multisample each oscillator plays, as an index into the bank.
    pub fn multi(&self, which: usize) -> usize {
        let raw = if which == 0 { self.pcm1 } else { self.pcm2 };
        (raw.round().max(0.0) as usize).min(bank::MULTIS.len().saturating_sub(1))
    }

    /// SINGLE, DOUBLE, SPLIT or VEL.
    pub fn layer_mode(&self) -> u32 {
        self.mode.round().clamp(0.0, 3.0) as u32
    }

    pub fn filter_mode(&self) -> Mode {
        match self.ftype.round().clamp(0.0, 3.0) as u32 {
            2 => Mode::BandpassUnity,
            3 => Mode::Highpass,
            _ => Mode::Lowpass,
        }
    }

    /// Two poles or four: LP24 is the same filter run twice.
    pub fn filter_stages(&self) -> usize {
        if self.ftype.round() as u32 == 1 { 2 } else { 1 }
    }

    pub fn lfo_shape(&self) -> LfoShape {
        match self.shape.round().clamp(0.0, 4.0) as u32 {
            1 => LfoShape::Sine,
            2 => LfoShape::SawUp,
            3 => LfoShape::Square,
            _ => LfoShape::Triangle,
        }
    }

    /// The sample-and-hold shape is a different kernel, not a waveform.
    pub fn lfo_is_random(&self) -> bool {
        self.shape.round() as u32 == 4
    }

    /// How many voices the Layer page allows.
    pub fn voice_count(&self) -> usize {
        (self.voices.round().clamp(1.0, VOICES as f32) as usize).clamp(1, VOICES)
    }

    /// The two oscillators' gains for one note: what MODE, SPLIT, VELPT,
    /// XFADE and BALANCE come to. Equal power across a crossfade, so a
    /// blend does not dip in the middle.
    pub fn layer_gains(&self, note: u8, velocity: f32) -> [f32; 2] {
        let blend = |value: f32, point: f32| -> f32 {
            let width = self.xfade.max(0.0);
            if width <= f32::EPSILON {
                return if value < point { 0.0 } else { 1.0 };
            }
            ((value - point) / width + 0.5).clamp(0.0, 1.0)
        };
        let pair = match self.layer_mode() {
            1 => [1.0, 1.0],
            2 => {
                let t = blend(f32::from(note), self.split);
                equal_power(t)
            }
            3 => {
                let t = blend(velocity.clamp(0.0, 1.0) * 127.0, self.velpt);
                equal_power(t)
            }
            _ => [1.0, 0.0],
        };
        let balance = self.balance.clamp(-1.0, 1.0);
        [
            pair[0] * (1.0 - balance.max(0.0)),
            pair[1] * (1.0 + balance.min(0.0)),
        ]
    }

    /// The pitch one oscillator reads a zone at, as a speed multiplier.
    /// KEY at 0 pins the sample to its own root: TUNE is then the only
    /// pitch the cell has.
    pub fn osc_ratio(&self, which: usize, note: u8, root: u8, correction: f64) -> f64 {
        let (tune, fine, key) = if which == 0 {
            (self.tune1, self.fine1, self.key1)
        } else {
            (self.tune2, self.fine2, self.key2)
        };
        let detune = if which == 0 {
            -self.detune * 0.5
        } else {
            self.detune * 0.5
        };
        let semitones = (f32::from(note) - f32::from(root)) * key.clamp(0.0, 1.0)
            + tune
            + (fine + detune) / 100.0;
        2f64.powf(f64::from(semitones) / 12.0) * correction
    }

    fn osc_pan(&self, which: usize) -> f32 {
        if which == 0 { self.pan1 } else { self.pan2 }
    }

    fn osc_start(&self, which: usize) -> f32 {
        if which == 0 { self.start1 } else { self.start2 }
    }

    fn osc_loops(&self, which: usize) -> bool {
        let raw = if which == 0 { self.loop1 } else { self.loop2 };
        raw.round() as u32 == 1
    }

    /// The converter VINTAGE asks for: its rate and its bits, faded in by
    /// the macro. At VINTAGE 0 this is the full rate and sixteen bits,
    /// which the downsampler treats as a wire.
    pub fn converter(&self, sample_rate: f32) -> (f32, f32) {
        let depth = self.vintage.clamp(0.0, 1.0);
        let rate = sample_rate + (self.vrate - sample_rate) * depth;
        let bits = 16.0 + (self.vbits - 16.0) * depth;
        (rate.clamp(1000.0, sample_rate), bits.clamp(4.0, 16.0))
    }

    /// How much of a loop VINTAGE keeps.
    pub fn loop_keep(&self) -> f64 {
        let depth = f64::from(self.vintage.clamp(0.0, 1.0) * self.vloop.clamp(0.0, 1.0));
        1.0 - depth * 0.97
    }
}

fn equal_power(t: f32) -> [f32; 2] {
    let t = t.clamp(0.0, 1.0);
    let angle = t * std::f32::consts::FRAC_PI_2;
    [angle.cos(), angle.sin()]
}

// ----------------------------------------------------------------- voice

#[derive(Debug)]
struct Osc {
    reader: SampleReader,
    /// The reader's settings as this voice last set them: the kernel
    /// keeps no accessor, and a chunk only ever moves `pitch`.
    settings: Settings,
    sample: u16,
    /// The speed the note asks for, and the speed the glide has reached.
    target: f64,
    current: f64,
    gain: f32,
    pan: (f32, f32),
    playing: bool,
}

impl Osc {
    fn new() -> Self {
        Self {
            reader: SampleReader::new(),
            settings: Settings::default(),
            sample: 0,
            target: 1.0,
            current: 1.0,
            gain: 0.0,
            pan: (1.0, 1.0),
            playing: false,
        }
    }

    fn reset(&mut self) {
        self.reader.reset();
        self.playing = false;
        self.gain = 0.0;
    }
}

#[derive(Debug)]
struct Voice {
    active: bool,
    gate: bool,
    note: u8,
    velocity: f32,
    age: u64,
    /// The knobs as they stood when this note fired, locks included. A
    /// turn while the note holds moves this too — unless the note was
    /// locked on that parameter, which is what `locked` remembers.
    patch: RomParams,
    locked: [bool; p::COUNT],
    osc: [Osc; 2],
    amp: Adsr,
    feg: Adsr,
    filter: [[Svf; 2]; 2],
    crush: [Downsampler; 2],
    lfo: Lfo,
    random: SampleHold,
    /// How far the LFO's fade has come, in samples.
    faded: f32,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            gate: false,
            note: 60,
            velocity: 1.0,
            age: 0,
            patch: RomParams::default(),
            locked: [false; p::COUNT],
            osc: [Osc::new(), Osc::new()],
            amp: Adsr::new(),
            feg: Adsr::new(),
            filter: [[Svf::new(), Svf::new()], [Svf::new(), Svf::new()]],
            crush: [Downsampler::new(), Downsampler::new()],
            lfo: Lfo::new(),
            random: SampleHold::new(),
            faded: 0.0,
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.lfo.prepare(sample_rate);
        self.random.prepare(sample_rate);
        self.random.seed(0x5eed_1eaf);
        for crush in &mut self.crush {
            crush.prepare(sample_rate);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.active = false;
        self.gate = false;
        self.faded = 0.0;
        self.amp.reset();
        self.feg.reset();
        self.lfo.reset();
        self.random.reset();
        for osc in &mut self.osc {
            osc.reset();
        }
        for channel in &mut self.filter {
            for stage in channel {
                stage.reset();
            }
        }
        for crush in &mut self.crush {
            crush.reset();
        }
    }

    /// A held note still hears a knob that is not locked on it.
    fn letter(&mut self, param: u32, value: f32) {
        if self.locked.get(param as usize).copied().unwrap_or(false) {
            return;
        }
        self.patch.set(param, value);
    }
}

// ------------------------------------------------------------ the machine

pub struct RomVoices {
    sample_rate: f32,
    bank: Arc<Bank>,
    /// The knobs as letters set them: where a lock returns to.
    base: RomParams,
    /// The knobs a new note inherits: `base` with the live locks over it.
    live: RomParams,
    locked: [bool; p::COUNT],
    voices: Vec<Voice>,
    right: Vec<f32>,
    chunk_left: usize,
    chorus: [DelayLine; 2],
    chorus_store: [Vec<f32>; 2],
    chorus_lfo: [Lfo; 2],
    /// The last note played, so GLIDE has somewhere to come from.
    last_note: Option<u8>,
}

impl Default for RomVoices {
    fn default() -> Self {
        Self::new()
    }
}

impl RomVoices {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            bank: Arc::new(Bank {
                samples: Vec::new(),
                rate: 48_000,
            }),
            base: RomParams::default(),
            live: RomParams::default(),
            locked: [false; p::COUNT],
            voices: Vec::new(),
            right: Vec::new(),
            chunk_left: 0,
            chorus: [DelayLine::new(), DelayLine::new()],
            chorus_store: [Vec::new(), Vec::new()],
            chorus_lfo: [Lfo::new(), Lfo::new()],
            last_note: None,
        }
    }

    /// The only allocation. The bank comes from the caller because it is
    /// built green-side, once per rate, and shared by every ROM track.
    pub fn prepare(
        &mut self,
        sample_rate: f32,
        max_block: usize,
        params: RomParams,
        bank: Arc<Bank>,
    ) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.bank = bank;
        self.base = params;
        self.live = params;
        self.locked = [false; p::COUNT];
        self.voices.clear();
        self.voices.reserve(VOICES);
        for _ in 0..VOICES {
            let mut voice = Voice::new();
            voice.prepare(self.sample_rate);
            voice.patch = params;
            self.voices.push(voice);
        }
        self.right.clear();
        self.right.resize(max_block.max(1), 0.0);
        let longest = ((CHORUS_CENTRE_MS + CHORUS_SWEEP_MS + 2.0) / 1000.0 * self.sample_rate)
            .ceil()
            .max(4.0) as usize;
        for (line, store) in self.chorus.iter_mut().zip(self.chorus_store.iter_mut()) {
            line.prepare(longest);
            store.clear();
            store.resize(crate::dsp::delay::buffer_len(longest), 0.0);
        }
        for lfo in &mut self.chorus_lfo {
            lfo.prepare(self.sample_rate);
            lfo.set_shape(LfoShape::Sine);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
        for line in &mut self.chorus {
            line.reset();
        }
        for (at, lfo) in self.chorus_lfo.iter_mut().enumerate() {
            lfo.reset();
            // Quadrature: the right channel's sweep is a quarter turn
            // behind, which is what WIDTH opens out into stereo.
            lfo.set_phase(if at == 0 { 0.0 } else { 0.25 });
        }
        self.chunk_left = 0;
        self.last_note = None;
        self.right.fill(0.0);
    }

    /// A knob turn: the base moves, and so does every note that is not
    /// locked on that parameter.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        if self.locked.get(param as usize).copied().unwrap_or(false) {
            return;
        }
        self.live.set(param, value);
        let value = self.base.get(param);
        for voice in self.voices.iter_mut().filter(|voice| voice.active) {
            voice.letter(param, value);
        }
    }

    /// A lock firing at a note boundary. `None` restores the live knob.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let Some(slot) = self.locked.get_mut(param as usize) else {
            return;
        };
        match value {
            Some(value) => {
                *slot = true;
                self.live.set(param, value);
            }
            None => {
                *slot = false;
                let base = self.base.get(param);
                self.live.set(param, base);
            }
        }
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        if !alpha.is_finite() {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let from = self.live.get(param);
        let to = self.base.get(param);
        self.live.set(param, from + (to - from) * alpha);
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8, age: u64) {
        let allowed = self.live.voice_count();
        let Some(at) = self.pick(allowed) else {
            return;
        };
        let previous = self.last_note;
        let patch = self.live;
        let locks = self.locked;
        let bank = &self.bank;
        let Some(voice) = self.voices.get_mut(at) else {
            return;
        };
        voice.patch = patch;
        voice.locked = locks;
        voice.active = true;
        voice.gate = true;
        voice.note = pitch;
        voice.velocity = f32::from(velocity.min(127)) / 127.0;
        voice.age = age;
        voice.faded = 0.0;
        let gains = patch.layer_gains(pitch, voice.velocity);
        for (which, gain) in gains.into_iter().enumerate() {
            let Some(osc) = voice.osc.get_mut(which) else {
                continue;
            };
            osc.gain = gain;
            let (left, right) = pan::balance(patch.osc_pan(which));
            osc.pan = (left, right);
            osc.playing = false;
            osc.reader.reset();
            if osc.gain <= 0.0 {
                continue;
            }
            let multi = patch.multi(which);
            let velocity_byte = (voice.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            let Some(zone) = bank::zone(multi, pitch, velocity_byte) else {
                continue;
            };
            let Some(sample) = bank.sample(zone.sample) else {
                continue;
            };
            osc.sample = zone.sample;
            let layout = sample.layout;
            let looping = layout.looped && patch.osc_loops(which);
            let keep = patch.loop_keep();
            // A shortened loop stays a whole number of cycles, so the M1's
            // tiny loops freeze the timbre without ever clicking.
            let period = layout.period.max(1) as f64;
            let full = (layout.loop_end - layout.loop_start) as f64;
            let cycles = ((full * keep) / period).floor().max(1.0);
            let loop_end = layout.loop_start as f64 + cycles * period;
            let ratio = patch.osc_ratio(which, pitch, sample.root, layout.correction);
            osc.target = ratio;
            osc.current = match previous.filter(|_| patch.glide > 0.0) {
                Some(from) => patch.osc_ratio(which, from, sample.root, layout.correction),
                None => ratio,
            };
            osc.settings = Settings {
                start: 0.0,
                end: layout.frames as f64,
                loop_start: layout.loop_start as f64,
                loop_end: loop_end.min(layout.frames as f64),
                looping: if looping { Loop::Forward } else { Loop::Off },
                crossfade: 0.0,
                pitch: osc.current,
                speed: 1.0,
                slip_speed: 1.0,
                method: Method::Repitch,
                ..Settings::default()
            };
            osc.reader.prepare(osc.settings);
            // START scans the attack of a looped sample and the first
            // half of a one-shot: in both cases the part before the tone
            // settles.
            let reach = if layout.looped {
                layout.loop_start as f64
            } else {
                layout.frames as f64 * 0.5
            };
            osc.reader
                .start(f64::from(patch.osc_start(which).clamp(0.0, 1.0)) * reach);
            osc.playing = true;
        }
        voice.amp.gate_on();
        voice.feg.gate_on();
        if patch.ltrig.round() as u32 != 0 {
            voice.lfo.reset();
            voice.random.reset();
        }
        self.last_note = Some(pitch);
    }

    /// A free voice inside the allowance, or the oldest note in it.
    fn pick(&mut self, allowed: usize) -> Option<usize> {
        let allowed = allowed.min(self.voices.len()).max(1);
        if let Some(at) = self.voices[..allowed]
            .iter()
            .position(|voice| !voice.active)
        {
            return Some(at);
        }
        let mut oldest = 0usize;
        for at in 1..allowed {
            if self.voices[at].age < self.voices[oldest].age {
                oldest = at;
            }
        }
        if let Some(voice) = self.voices.get_mut(oldest) {
            voice.reset();
        }
        Some(oldest)
    }

    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.gate && voice.note == pitch {
                voice.gate = false;
                voice.amp.gate_off();
                voice.feg.gate_off();
            }
        }
    }

    pub fn release_all(&mut self) {
        for voice in &mut self.voices {
            if voice.active && voice.gate {
                voice.gate = false;
                voice.amp.gate_off();
                voice.feg.gate_off();
            }
        }
    }

    pub fn active(&self) -> bool {
        self.voices.iter().any(|voice| voice.active)
    }

    pub fn right(&self, len: usize) -> &[f32] {
        self.right.get(..len).unwrap_or(&[])
    }

    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone. Left into `out`, right into the block-length buffer the
    /// node reads back after the clock's walk.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        let mut done = 0;
        while done < out.len() {
            if self.chunk_left == 0 {
                self.chunk_left = CHUNK;
            }
            let take = (out.len() - done).min(self.chunk_left).clamp(1, CHUNK);
            let mut left = [0.0f32; CHUNK];
            let mut right = [0.0f32; CHUNK];
            {
                let Self {
                    sample_rate,
                    bank,
                    voices,
                    ..
                } = self;
                for voice in voices.iter_mut().filter(|voice| voice.active) {
                    voice_chunk(voice, bank, *sample_rate, take, &mut left, &mut right);
                }
            }
            self.chorus_chunk(take, &mut left, &mut right);
            for i in 0..take {
                let g = gain.next();
                if let Some(sample) = out.get_mut(done + i) {
                    *sample = left[i] * g;
                }
                if let Some(sample) = self.right.get_mut(at + done + i) {
                    *sample = right[i] * g;
                }
            }
            self.chunk_left -= take;
            done += take;
        }
    }

    /// The ensemble: one delay per side, swept in quadrature. WIDTH is
    /// how far apart the two sweeps are allowed to be, so it changes what
    /// DEPTH does rather than repeating it.
    fn chorus_chunk(&mut self, n: usize, left: &mut [f32; CHUNK], right: &mut [f32; CHUNK]) {
        let mix = self.live.cmix.clamp(0.0, 1.0);
        if mix <= 0.0 {
            return;
        }
        let rate = self.live.crate_hz.clamp(0.01, 8.0);
        let depth = self.live.cdepth.clamp(0.0, 1.0);
        let width = self.live.cwidth.clamp(0.0, 1.0);
        let centre = CHORUS_CENTRE_MS / 1000.0 * self.sample_rate;
        let sweep = CHORUS_SWEEP_MS / 1000.0 * self.sample_rate * depth;
        let mut modulation = [[0.0f32; CHUNK]; 2];
        for (side, lfo) in self.chorus_lfo.iter_mut().enumerate() {
            lfo.set_rate(rate);
            let Some(slice) = modulation.get_mut(side) else {
                continue;
            };
            lfo.process(&mut slice[..n]);
        }
        let mut delays = [[0.0f32; CHUNK]; 2];
        for i in 0..n {
            // At WIDTH 0 both sides sweep together — a mono chorus; at 1
            // they are a quarter turn apart and the image opens.
            let a = modulation[0][i];
            let b = modulation[1][i];
            delays[0][i] = centre + sweep * a;
            delays[1][i] = centre + sweep * (a + (b - a) * width);
        }
        let mut wet = [[0.0f32; CHUNK]; 2];
        wet[0][..n].copy_from_slice(&left[..n]);
        wet[1][..n].copy_from_slice(&right[..n]);
        for side in 0..2 {
            let (Some(line), Some(store)) =
                (self.chorus.get_mut(side), self.chorus_store.get_mut(side))
            else {
                continue;
            };
            let (Some(io), Some(taps)) = (wet.get_mut(side), delays.get(side)) else {
                continue;
            };
            line.process_modulated(&mut io[..n], store, &taps[..n]);
        }
        for i in 0..n {
            left[i] += (wet[0][i] - left[i]) * mix;
            right[i] += (wet[1][i] - right[i]) * mix;
        }
    }
}

/// One voice, one control chunk. Coefficients are computed once here;
/// everything inside the sample loop is a multiply.
fn voice_chunk(
    voice: &mut Voice,
    bank: &Bank,
    sample_rate: f32,
    n: usize,
    left: &mut [f32; CHUNK],
    right: &mut [f32; CHUNK],
) {
    let p = voice.patch;
    let note = voice.note;
    let velocity = voice.velocity;

    // Envelopes. Both are PROCESSED every chunk, whether or not their
    // value is read: an envelope that is only read never advances, and
    // then a voice never finishes.
    let key_scale = 2f32.powf(-p.keydec.clamp(-1.0, 1.0) * (f32::from(note) - 60.0) / 12.0);
    voice.amp.prepare(
        sample_rate,
        p.attack,
        p.decay * key_scale,
        p.sustain,
        p.release * key_scale,
    );
    voice.feg.prepare(
        sample_rate,
        p.fattack,
        p.fdecay * key_scale,
        p.fsustain,
        p.frelease * key_scale,
    );
    let mut amp = [0.0f32; CHUNK];
    let mut feg = [0.0f32; CHUNK];
    voice.amp.process(&mut amp[..n]);
    voice.feg.process(&mut feg[..n]);

    // The LFO, and its fade.
    let mut lfo = [0.0f32; CHUNK];
    if p.lfo_is_random() {
        voice.random.set_rate(p.rate);
        voice.random.process(&mut lfo[..n]);
    } else {
        voice.lfo.set_shape(p.lfo_shape());
        voice.lfo.set_rate(p.rate);
        voice.lfo.process(&mut lfo[..n]);
    }
    let fade_samples = (p.fade / 1000.0 * sample_rate).max(0.0);
    let faded = if fade_samples <= 0.0 {
        1.0
    } else {
        (voice.faded / fade_samples).clamp(0.0, 1.0)
    };
    voice.faded += n as f32;
    // One-shot: the LFO stops mattering after its first turn.
    let depth = if p.ltrig.round() as u32 == 2 && voice.faded > sample_rate / p.rate.max(0.01) {
        0.0
    } else {
        faded
    };
    let modulation = lfo[0] * depth;

    // Pitch: the glide comes up to the note's speed over GLIDE.
    let glide_samples = (p.glide / 1000.0 * sample_rate).max(0.0);
    let alpha = if glide_samples <= 1.0 {
        1.0
    } else {
        (n as f64 / f64::from(glide_samples)).min(1.0)
    };
    let bend = 2f64.powf(f64::from(p.lpitch * modulation) / 1200.0);

    let mut mix = [[0.0f32; CHUNK]; 2];
    let mut sounding = false;
    for which in 0..2 {
        let osc = &mut voice.osc[which];
        if !osc.playing || osc.gain <= 0.0 {
            continue;
        }
        let Some(sample) = bank.sample(osc.sample) else {
            continue;
        };
        osc.current += (osc.target - osc.current) * alpha;
        osc.settings.pitch = (osc.current * bend).clamp(0.015_625, 64.0);
        osc.reader.set(osc.settings);
        if !osc.reader.active() {
            osc.playing = false;
            continue;
        }
        sounding = true;
        let source = sample.material.channel(0);
        let gain = osc.gain;
        let (pan_l, pan_r) = osc.pan;
        let (left_mix, right_mix) = mix.split_at_mut(1);
        for (l, r) in left_mix[0][..n]
            .iter_mut()
            .zip(right_mix[0][..n].iter_mut())
        {
            let x = osc.reader.tick(source, &[])[0] * gain;
            *l += x * pan_l;
            *r += x * pan_r;
        }
    }

    // The converter, before the filter: an eight-bit sample is eight bits
    // going INTO the analogue side, not coming out of it.
    let (rate, bits) = p.converter(sample_rate);
    for (channel, crush) in voice.crush.iter_mut().enumerate() {
        crush.set_rate(rate);
        crush.set_bits(bits);
        if let Some(io) = mix.get_mut(channel) {
            crush.process(&mut io[..n]);
        }
    }

    // The filter. One corner per chunk, from the envelope, the key, the
    // velocity and the LFO — all of them in octaves, so they add.
    let octaves = p.fenv.clamp(-1.0, 1.0) * feg[0] * 6.0
        + p.fkey.clamp(0.0, 1.0) * (f32::from(note) - 60.0) / 12.0
        + p.fvel.clamp(0.0, 1.0) * velocity * 4.0
        + p.lcut.clamp(-1.0, 1.0) * modulation * 5.0;
    let cutoff = (p.cutoff * 2f32.powf(octaves)).clamp(20.0, sample_rate * 0.45);
    let q = 0.5 + p.reso.clamp(0.0, 1.0) * 9.5;
    let mode = p.filter_mode();
    let stages = p.filter_stages();
    for (channel, filters) in voice.filter.iter_mut().enumerate() {
        let Some(io) = mix.get_mut(channel) else {
            continue;
        };
        for stage in filters.iter_mut().take(stages) {
            stage.prepare(sample_rate, cutoff, q);
            stage.process(&mut io[..n], mode);
        }
    }

    // Amp, velocity, the LFO's tremolo, and the voice's own balance.
    let velocity_gain = 1.0 - p.vel.clamp(0.0, 1.0) * (1.0 - velocity);
    let (voice_l, voice_r) = pan::balance(p.pan);
    let level = p.level.clamp(0.0, 2.0) * velocity_gain;
    for i in 0..n {
        let tremolo = 1.0 - p.lamp.clamp(0.0, 1.0) * depth * (0.5 - 0.5 * lfo[i]);
        let g = amp[i] * level * tremolo;
        left[i] += mix[0][i] * g * voice_l;
        right[i] += mix[1][i] * g * voice_r;
    }

    // A one-shot ends when its sample does, gate or no gate; a sustained
    // note ends when its release does.
    if !voice.amp.active() || !sounding {
        voice.active = false;
        voice.gate = false;
        for osc in &mut voice.osc {
            osc.reset();
        }
    }
}

// -------------------------------------------------------------- pictures

use crate::pages::{Hero, HeroMark, HeroSeries};

/// The picture above the cells, one per sub-page. Everything here is the
/// ENGINE's arithmetic or the kernels themselves — the zone tables, the
/// same `Adsr`, the same `Lfo`, the same `Downsampler` — so a picture
/// cannot drift away from what is heard.
pub fn hero(params: &RomParams, page: &str, selected: Option<u32>) -> Option<Hero> {
    match page {
        "Osc 1" => Some(osc_picture(params, 0, selected)),
        "Osc 2" => Some(osc_picture(params, 1, selected)),
        "Layer" => Some(layer_picture(params, selected)),
        "Filter" => Some(filter_picture(params, selected)),
        "F EG" => Some(envelope_picture(
            "Filter envelope",
            [
                params.fattack,
                params.fdecay,
                params.fsustain,
                params.frelease,
            ],
            [p::FATTACK, p::FDECAY, p::FSUSTAIN, p::FRELEASE],
            selected,
        )),
        "Amp" => Some(envelope_picture(
            "Amp envelope",
            [params.attack, params.decay, params.sustain, params.release],
            [p::ATTACK, p::DECAY, p::SUSTAIN, p::RELEASE],
            selected,
        )),
        "LFO" => Some(lfo_picture(params, selected)),
        "Chorus" => Some(chorus_picture(params, selected)),
        "Vintage" => Some(vintage_picture(params, selected)),
        _ => None,
    }
}

/// The PCM list for one oscillator's page: every multisample in the
/// bank under its category, what each one is, and which oscillators are
/// on it. The tall panel's left column.
pub fn pcm_list(params: &RomParams, page: &str) -> Option<crate::pages::ListHero> {
    let which = match page {
        "Osc 1" => 0usize,
        "Osc 2" => 1usize,
        _ => return None,
    };
    let rows = bank::MULTIS
        .iter()
        .enumerate()
        .map(|(at, multi)| {
            let recipe = multi
                .zones
                .first()
                .and_then(|zone| bank::RECIPES.get(zone.sample as usize));
            let mut detail = format!("{}z", multi.zones.len());
            if let Some(recipe) = recipe {
                if recipe.decay_ms > 0.0 {
                    detail.push_str(" shot");
                } else if recipe.offset != 0.0 {
                    detail.push_str(&format!(" +{}st", recipe.offset.round() as i32));
                }
            }
            let mut tags = String::new();
            if params.multi(0) == at {
                tags.push('1');
            }
            if params.multi(1) == at {
                tags.push('2');
            }
            crate::pages::ListRow {
                group: multi.category,
                name: multi.name,
                detail,
                tags,
            }
        })
        .collect();
    Some(crate::pages::ListHero {
        title: format!("{} PCM", bank::MULTIS.len()),
        rows,
        selected: params.multi(which),
        param: if which == 0 { p::PCM1 } else { p::PCM2 },
    })
}

/// The file behind one multisample, for the HEAR tool: the middle of its
/// zone map, as the cache holds it. Reads only — a key press must not
/// render a bank, and by the time a ROM track exists the graph has
/// already baked one.
pub fn audition_file(multi: usize, rate: u32) -> Option<std::path::PathBuf> {
    let zone = bank::zone(multi, 60, 100)?;
    let recipe = bank::RECIPES.get(zone.sample as usize)?;
    let path = bank::wav_path(recipe, rate);
    path.exists().then_some(path)
}

fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(note) / 12 - 1;
    format!("{}{octave}", NAMES[usize::from(note) % 12])
}

/// The zone map — key across, velocity up — or, when the cells that shape
/// the read are selected, the sample the middle of the map plays.
fn osc_picture(params: &RomParams, which: usize, selected: Option<u32>) -> Hero {
    let start = if which == 0 { p::START1 } else { p::START2 };
    let looping = if which == 0 { p::LOOP1 } else { p::LOOP2 };
    if selected == Some(start) || selected == Some(looping) {
        return sample_picture(params, which, selected);
    }
    let multi = params.multi(which);
    let pcm = if which == 0 { p::PCM1 } else { p::PCM2 };
    let lit = selected == Some(pcm);
    let name = bank::MULTIS
        .get(multi)
        .map_or("—", |multi| multi.name)
        .to_owned();
    let zones = bank::MULTIS.get(multi).map_or(&[][..], |multi| multi.zones);
    let mut series = Vec::new();
    let mut marks = Vec::new();
    for zone in zones {
        // Each zone as its own closed rectangle: key across, velocity up.
        // Nothing is lit, so the view lights the map whole and the
        // rectangles stay readable instead of washing over each other.
        let (left, right) = (f32::from(zone.lo) / 127.0, f32::from(zone.hi) / 127.0);
        let (low, high) = (
            f32::from(zone.vel_lo) / 127.0,
            f32::from(zone.vel_hi) / 127.0,
        );
        series.push(HeroSeries {
            name: "zone",
            points: vec![
                (left, low),
                (right, low),
                (right, high),
                (left, high),
                (left, low),
            ],
            lit: false,
        });
        if let Some(recipe) = bank::RECIPES.get(zone.sample as usize) {
            let at = f32::from(recipe.root) / 127.0;
            if !marks
                .iter()
                .any(|mark: &HeroMark| (mark.x - at).abs() < 1e-6)
            {
                marks.push(HeroMark {
                    x: at,
                    label: note_name(recipe.root),
                    lit,
                });
            }
        }
    }
    Hero {
        waveform: None,
        title: format!("{name} · zones"),
        series,
        marks,
        x_labels: ["C-1".to_owned(), "G9".to_owned()],
        y_labels: ["vel 1".to_owned(), "vel 127".to_owned()],
        diagonal: false,
    }
}

/// One sample end to end: its level, the second partial that moves
/// through its attack, the loop the VINTAGE cut leaves, and where START
/// drops the reader in.
fn sample_picture(params: &RomParams, which: usize, selected: Option<u32>) -> Hero {
    let multi = params.multi(which);
    let recipe = bank::zone(multi, 60, 100)
        .and_then(|zone| bank::RECIPES.get(zone.sample as usize))
        .copied();
    let Some(recipe) = recipe else {
        return Hero {
            waveform: None,
            title: "no sample".to_owned(),
            series: Vec::new(),
            marks: Vec::new(),
            x_labels: ["0".to_owned(), "1".to_owned()],
            y_labels: ["0".to_owned(), "1".to_owned()],
            diagonal: false,
        };
    };
    let layout = recipe.layout(48_000);
    let frames = layout.frames.max(1) as f32;
    let looping = layout.looped && params.osc_loops(which);
    let points = 160usize;
    let mut level = Vec::with_capacity(points);
    let mut partial = Vec::with_capacity(points);
    for step in 0..points {
        let t = step as f32 / (points - 1) as f32;
        let at = t * frames;
        let (amp, second) = if layout.looped {
            let through = (at / layout.loop_start.max(1) as f32).min(1.0);
            (
                1.0,
                recipe.attack_partial2 + (recipe.partial2 - recipe.attack_partial2) * through,
            )
        } else {
            let decay = recipe.decay_ms / 1000.0 * 48_000.0;
            let a = (-at / decay.max(1.0)).exp();
            (a, recipe.partial2 * (-at / (decay * 0.45).max(1.0)).exp())
        };
        level.push((t, amp.clamp(0.0, 1.0)));
        partial.push((t, second.clamp(0.0, 1.0)));
    }
    let mut marks = Vec::new();
    if looping {
        let keep = params.loop_keep() as f32;
        let full = (layout.loop_end - layout.loop_start) as f32;
        marks.push(HeroMark {
            x: layout.loop_start as f32 / frames,
            label: "loop".to_owned(),
            lit: false,
        });
        marks.push(HeroMark {
            x: ((layout.loop_start as f32 + full * keep) / frames).min(1.0),
            label: "end".to_owned(),
            lit: keep < 1.0,
        });
    }
    let reach = if layout.looped {
        layout.loop_start as f32
    } else {
        frames * 0.5
    };
    let start = if which == 0 {
        params.start1
    } else {
        params.start2
    };
    marks.push(HeroMark {
        x: (start.clamp(0.0, 1.0) * reach / frames).min(1.0),
        label: "start".to_owned(),
        lit: selected == Some(if which == 0 { p::START1 } else { p::START2 }),
    });
    Hero {
        waveform: None,
        title: format!("{} · sample", recipe.name),
        series: vec![
            HeroSeries {
                name: "level",
                points: level,
                lit: false,
            },
            HeroSeries {
                name: "2nd partial",
                points: partial,
                lit: false,
            },
        ],
        marks,
        x_labels: ["0".to_owned(), format!("{:.0} ms", frames / 48.0)],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// Which oscillator sounds where: across the keyboard for a split, and
/// across velocity for a switch.
fn layer_picture(params: &RomParams, selected: Option<u32>) -> Hero {
    let by_velocity = params.layer_mode() == 3;
    let mut first = Vec::with_capacity(64);
    let mut second = Vec::with_capacity(64);
    for step in 0..64 {
        let t = step as f32 / 63.0;
        let gains = if by_velocity {
            params.layer_gains(60, t)
        } else {
            params.layer_gains((t * 127.0).round() as u8, 0.8)
        };
        first.push((t, gains[0].clamp(0.0, 1.0)));
        second.push((t, gains[1].clamp(0.0, 1.0)));
    }
    let point = if by_velocity {
        params.velpt / 127.0
    } else {
        params.split / 127.0
    };
    let lit = matches!(
        selected,
        Some(p::MODE) | Some(p::SPLIT) | Some(p::VELPT) | Some(p::XFADE) | Some(p::BALANCE)
    );
    Hero {
        waveform: None,
        title: match params.layer_mode() {
            1 => "double".to_owned(),
            2 => "split".to_owned(),
            3 => "velocity".to_owned(),
            _ => "single".to_owned(),
        },
        series: vec![
            HeroSeries {
                name: "osc 1",
                points: first,
                lit,
            },
            HeroSeries {
                name: "osc 2",
                points: second,
                lit,
            },
        ],
        marks: vec![HeroMark {
            x: point.clamp(0.0, 1.0),
            label: if by_velocity {
                "vel".to_owned()
            } else {
                "split".to_owned()
            },
            lit,
        }],
        x_labels: if by_velocity {
            ["vel 0".to_owned(), "vel 127".to_owned()]
        } else {
            ["C-1".to_owned(), "G9".to_owned()]
        },
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// The magnitude of the state-variable filter, in decibels over a
/// logarithmic axis. The formula is the kernel's own bilinear mapping;
/// a test measures the kernel and holds this curve to it.
pub fn response_db(
    mode: Mode,
    stages: usize,
    cutoff: f32,
    q: f32,
    hz: f32,
    sample_rate: f32,
) -> f32 {
    let g = (std::f32::consts::PI * cutoff.clamp(1.0, sample_rate * 0.49) / sample_rate).tan();
    let w = (std::f32::consts::PI * hz.clamp(1.0, sample_rate * 0.49) / sample_rate).tan() / g;
    let k = 1.0 / q.max(0.5);
    let real = 1.0 - w * w;
    let imaginary = w * k;
    let denominator = (real * real + imaginary * imaginary).max(1e-12).sqrt();
    let magnitude = match mode {
        Mode::Highpass => w * w / denominator,
        Mode::BandpassUnity => w / denominator,
        _ => 1.0 / denominator,
    };
    20.0 * magnitude.max(1e-6).log10() * stages.max(1) as f32
}

fn filter_picture(params: &RomParams, selected: Option<u32>) -> Hero {
    let sample_rate = 48_000.0;
    let mode = params.filter_mode();
    let stages = params.filter_stages();
    let q = 0.5 + params.reso.clamp(0.0, 1.0) * 9.5;
    let curve = |cutoff: f32| -> Vec<(f32, f32)> {
        (0..64)
            .map(|step| {
                let t = step as f32 / 63.0;
                let hz = 20.0 * (20_000.0f32 / 20.0).powf(t);
                let db = response_db(mode, stages, cutoff, q, hz, sample_rate);
                (t, ((db + 48.0) / 60.0).clamp(0.0, 1.0))
            })
            .collect()
    };
    let open = (params.cutoff * 2f32.powf(params.fenv.clamp(-1.0, 1.0) * 6.0))
        .clamp(20.0, sample_rate * 0.45);
    let mut series = vec![HeroSeries {
        name: "response",
        points: curve(params.cutoff),
        lit: matches!(selected, Some(p::CUTOFF) | Some(p::RESO) | Some(p::TYPE)),
    }];
    if params.fenv.abs() > f32::EPSILON {
        series.push(HeroSeries {
            name: "envelope",
            points: curve(open),
            lit: selected == Some(p::FENV),
        });
    }
    let mark = (params.cutoff.clamp(20.0, 20_000.0) / 20.0).log10() / (20_000.0f32 / 20.0).log10();
    Hero {
        waveform: None,
        title: match (mode, stages) {
            (Mode::Lowpass, 2) => "LP24".to_owned(),
            (Mode::Lowpass, _) => "LP12".to_owned(),
            (Mode::BandpassUnity, _) => "band".to_owned(),
            _ => "high".to_owned(),
        },
        series,
        marks: vec![HeroMark {
            x: mark.clamp(0.0, 1.0),
            label: format!("{:.0} Hz", params.cutoff),
            lit: selected == Some(p::CUTOFF),
        }],
        x_labels: ["20 Hz".to_owned(), "20 k".to_owned()],
        y_labels: ["-48 dB".to_owned(), "+12".to_owned()],
        diagonal: false,
    }
}

/// The envelope the `Adsr` kernel actually produces, run at a rate that
/// puts the whole shape in one picture.
fn envelope_picture(title: &str, stage: [f32; 4], ids: [u32; 4], selected: Option<u32>) -> Hero {
    let [attack, decay, sustain, release] = stage;
    let held = (attack + decay).max(1.0) * 0.4;
    let total = (attack + decay + held + release).max(1.0);
    let points = 192usize;
    let rate = points as f32 / (total / 1000.0);
    let mut adsr = Adsr::new();
    adsr.prepare(rate, attack, decay, sustain, release);
    adsr.gate_on();
    let gate_at = (((attack + decay + held) / total) * points as f32).round() as usize;
    let mut curve = Vec::with_capacity(points);
    let mut scratch = [0.0f32; 1];
    for step in 0..points {
        if step == gate_at {
            adsr.gate_off();
        }
        adsr.process(&mut scratch);
        curve.push((
            step as f32 / (points - 1) as f32,
            scratch[0].clamp(0.0, 1.0),
        ));
    }
    let span = |from: f32, to: f32| -> (usize, usize) {
        let at = |ms: f32| {
            ((ms / total) * points as f32)
                .round()
                .clamp(0.0, points as f32) as usize
        };
        (at(from), at(to).min(points))
    };
    let (from, to) = match selected {
        Some(id) if id == ids[0] => span(0.0, attack),
        Some(id) if id == ids[1] => span(attack, attack + decay),
        Some(id) if id == ids[2] => span(attack + decay, attack + decay + held),
        Some(id) if id == ids[3] => span(attack + decay + held, total),
        _ => (0, 0),
    };
    let mut series = vec![HeroSeries {
        name: "envelope",
        points: curve.clone(),
        lit: false,
    }];
    // A lit series only when the segment HAS width; a zero-width one
    // would empty the picture instead of lighting it.
    if to > from + 1 {
        series.push(HeroSeries {
            name: "stage",
            points: curve[from..to.min(curve.len())].to_vec(),
            lit: true,
        });
    }
    Hero {
        waveform: None,
        title: title.to_owned(),
        series,
        marks: vec![HeroMark {
            x: (gate_at as f32 / points as f32).clamp(0.0, 1.0),
            label: "off".to_owned(),
            lit: selected == Some(ids[3]),
        }],
        x_labels: ["0".to_owned(), format!("{total:.0} ms")],
        y_labels: ["0".to_owned(), "1".to_owned()],
        diagonal: false,
    }
}

/// One turn of the LFO, from the kernel that runs in the voice.
fn lfo_picture(params: &RomParams, selected: Option<u32>) -> Hero {
    let points = 128usize;
    let rate = params.rate.clamp(0.01, 40.0);
    let mut shape = [0.0f32; 128];
    if params.lfo_is_random() {
        let mut hold = SampleHold::new();
        hold.prepare(points as f32 * rate);
        hold.seed(0x5eed_1eaf);
        hold.set_rate(rate * 4.0);
        hold.process(&mut shape);
    } else {
        let mut lfo = Lfo::new();
        lfo.prepare(points as f32 * rate);
        lfo.set_shape(params.lfo_shape());
        lfo.set_rate(rate);
        lfo.process(&mut shape);
    }
    let curve = shape
        .iter()
        .enumerate()
        .map(|(at, value)| {
            (
                at as f32 / (points - 1) as f32,
                (value * 0.5 + 0.5).clamp(0.0, 1.0),
            )
        })
        .collect();
    let depths = [
        (p::LPITCH, params.lpitch.abs() / 100.0),
        (p::LCUT, params.lcut.abs()),
        (p::LAMP, params.lamp),
    ];
    let lit = matches!(selected, Some(p::SHAPE) | Some(p::RATE) | Some(p::LTRIG));
    let mut marks = Vec::new();
    for (id, depth) in depths {
        if selected == Some(id) && depth > 0.0 {
            marks.push(HeroMark {
                x: depth.clamp(0.0, 1.0),
                label: "depth".to_owned(),
                lit: true,
            });
        }
    }
    Hero {
        waveform: None,
        title: format!("{:.2} Hz", rate),
        series: vec![HeroSeries {
            name: "lfo",
            points: curve,
            lit,
        }],
        marks,
        x_labels: ["0".to_owned(), "1 turn".to_owned()],
        y_labels: ["-1".to_owned(), "+1".to_owned()],
        diagonal: false,
    }
}

/// The ensemble's two delay taps over one turn — the same arithmetic the
/// chunk uses, so WIDTH's quadrature is visible rather than described.
fn chorus_picture(params: &RomParams, selected: Option<u32>) -> Hero {
    let points = 128usize;
    let rate = params.crate_hz.clamp(0.01, 8.0);
    let mut left = Lfo::new();
    let mut right = Lfo::new();
    for (at, lfo) in [&mut left, &mut right].into_iter().enumerate() {
        lfo.prepare(points as f32 * rate);
        lfo.set_shape(LfoShape::Sine);
        lfo.set_rate(rate);
        lfo.set_phase(if at == 0 { 0.0 } else { 0.25 });
    }
    let mut a = [0.0f32; 128];
    let mut b = [0.0f32; 128];
    left.process(&mut a);
    right.process(&mut b);
    let sweep = CHORUS_SWEEP_MS * params.cdepth.clamp(0.0, 1.0);
    let width = params.cwidth.clamp(0.0, 1.0);
    let scale = |ms: f32| (ms / (CHORUS_CENTRE_MS + CHORUS_SWEEP_MS + 1.0)).clamp(0.0, 1.0);
    let mut first = Vec::with_capacity(points);
    let mut second = Vec::with_capacity(points);
    for at in 0..points {
        let t = at as f32 / (points - 1) as f32;
        first.push((t, scale(CHORUS_CENTRE_MS + sweep * a[at])));
        second.push((
            t,
            scale(CHORUS_CENTRE_MS + sweep * (a[at] + (b[at] - a[at]) * width)),
        ));
    }
    let lit = matches!(
        selected,
        Some(p::CRATE) | Some(p::CDEPTH) | Some(p::CWIDTH) | Some(p::CMIX)
    );
    Hero {
        waveform: None,
        title: format!("{:.0}% wet", params.cmix * 100.0),
        series: vec![
            HeroSeries {
                name: "left",
                points: first,
                lit,
            },
            HeroSeries {
                name: "right",
                points: second,
                lit,
            },
        ],
        marks: Vec::new(),
        x_labels: ["0".to_owned(), "1 turn".to_owned()],
        y_labels: ["0 ms".to_owned(), "14 ms".to_owned()],
        diagonal: false,
    }
}

/// A ramp through the converter the voices use: the staircase VINTAGE
/// puts under everything, against unity.
fn vintage_picture(params: &RomParams, selected: Option<u32>) -> Hero {
    let sample_rate = 48_000.0;
    let (rate, bits) = params.converter(sample_rate);
    let points = 192usize;
    let mut ramp: Vec<f32> = (0..points)
        .map(|at| at as f32 / (points - 1) as f32 * 2.0 - 1.0)
        .collect();
    let mut converter = Downsampler::new();
    converter.prepare(sample_rate);
    converter.set_rate(rate);
    converter.set_bits(bits);
    converter.process(&mut ramp);
    let curve = ramp
        .iter()
        .enumerate()
        .map(|(at, value)| {
            (
                at as f32 / (points - 1) as f32,
                (value * 0.5 + 0.5).clamp(0.0, 1.0),
            )
        })
        .collect();
    Hero {
        waveform: None,
        title: if params.vintage <= 0.0 {
            "clean".to_owned()
        } else {
            format!("{:.1} k · {bits:.0} bit", rate / 1000.0)
        },
        series: vec![HeroSeries {
            name: "converter",
            points: curve,
            lit: matches!(selected, Some(p::VINTAGE) | Some(p::VRATE) | Some(p::VBITS)),
        }],
        marks: vec![HeroMark {
            x: 1.0 - params.loop_keep() as f32,
            label: "loop cut".to_owned(),
            lit: selected == Some(p::VLOOP),
        }],
        x_labels: ["-1".to_owned(), "+1".to_owned()],
        y_labels: ["-1".to_owned(), "+1".to_owned()],
        diagonal: true,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn machine(edit: impl FnOnce(&mut RomParams)) -> RomVoices {
        let mut params = RomParams::default();
        edit(&mut params);
        let mut voices = RomVoices::new();
        voices.prepare(RATE, 4096, params, Arc::new(bank::Bank::render(48_000)));
        voices
    }

    fn block(voices: &mut RomVoices, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        let mut ramp = Ramp::across(1.0, 1.0, frames);
        voices.render(&mut out, 0, &mut ramp);
        out
    }

    fn peak(signal: &[f32]) -> f32 {
        signal.iter().fold(0.0f32, |top, x| top.max(x.abs()))
    }

    /// One bin of a discrete transform, by Goertzel: the magnitude of
    /// `hz` in `signal`. Sines are what the bank makes, so a pitch claim
    /// can be measured rather than described.
    fn magnitude(signal: &[f32], hz: f32) -> f32 {
        let k = std::f32::consts::TAU * hz / RATE;
        let coefficient = 2.0 * k.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for x in signal {
            let s0 = x + coefficient * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coefficient * s1 * s2).max(0.0).sqrt() / signal.len() as f32
    }

    fn hz_of(note: u8) -> f32 {
        440.0 * 2f32.powf((f32::from(note) - 69.0) / 12.0)
    }

    /// The strongest of the semitones around `note`: what the machine is
    /// actually playing, not what it was asked for.
    fn heard(signal: &[f32], around: u8) -> u8 {
        let mut best = (0.0f32, around);
        for note in around.saturating_sub(14)..=(around + 14).min(127) {
            let level = magnitude(signal, hz_of(note));
            if level > best.0 {
                best = (level, note);
            }
        }
        best.1
    }

    fn note(voices: &mut RomVoices, pitch: u8, velocity: u8, frames: usize) -> Vec<f32> {
        voices.note_on(pitch, velocity, 1);
        block(voices, frames)
    }

    // ---- claim 1

    #[test]
    fn silent_until_struck_and_released_on_note_off() {
        let mut voices = machine(|_| {});
        assert_eq!(peak(&block(&mut voices, 512)), 0.0);
        let sounded = note(&mut voices, 60, 100, 4096);
        assert!(peak(&sounded) > 0.05, "a note made no sound");
        voices.note_off(60);
        // RELEASE defaults to 200 ms; a second of silence is well past it.
        let tail = block(&mut voices, 48_000);
        assert!(peak(&tail[40_000..]) < 1e-4, "the release never finished");
        assert!(!voices.active(), "the voice never freed itself");
    }

    // ---- claim 2

    #[test]
    fn the_zone_seam_does_not_jump_an_octave() {
        // 53 and 54 are two sides of a zone boundary in the sine map.
        let mut low = machine(|_| {});
        let mut high = machine(|_| {});
        let below = note(&mut low, 53, 100, 8192);
        let above = note(&mut high, 54, 100, 8192);
        assert_eq!(heard(&below, 53), 53);
        assert_eq!(
            heard(&above, 54),
            54,
            "the note across the seam changed pitch"
        );
    }

    // ---- claim 3

    #[test]
    fn a_hard_velocity_is_a_brighter_timbre_not_a_louder_one() {
        let bright = |velocity: u8| {
            let mut voices = machine(|_| {});
            let signal = note(&mut voices, 60, velocity, 4096);
            let fundamental = magnitude(&signal, hz_of(60)).max(1e-9);
            magnitude(&signal, hz_of(60) * 2.0) / fundamental
        };
        let soft = bright(30);
        let hard = bright(120);
        assert!(
            hard > soft * 4.0,
            "the velocity switch changed level but not timbre: {soft} then {hard}"
        );
    }

    // ---- claim 5

    #[test]
    fn a_one_shot_ends_while_the_gate_is_still_held() {
        // PING is a one-shot: no loop, an exponential decay of half a
        // second. The gate is never lifted.
        let mut voices = machine(|p| {
            p.pcm1 = 3.0;
            p.decay = 8000.0;
            p.sustain = 1.0;
        });
        let struck = note(&mut voices, 60, 100, 4096);
        assert!(peak(&struck) > 0.05);
        let later = block(&mut voices, 96_000);
        assert!(
            peak(&later[80_000..]) < 1e-4,
            "the one-shot outlived its own sample"
        );
        assert!(!voices.active());
    }

    #[test]
    fn a_looped_sample_holds_while_the_gate_does() {
        let mut voices = machine(|p| p.sustain = 1.0);
        let _ = note(&mut voices, 60, 100, 4096);
        let held = block(&mut voices, 96_000);
        assert!(
            peak(&held[80_000..]) > 0.05,
            "a held note stopped sounding: the loop did not hold"
        );
    }

    // ---- claim 6

    #[test]
    fn key_tracking_at_zero_pins_the_sample_to_its_root() {
        // Two notes INSIDE one zone: with tracking they are a fifth
        // apart, without it they are the same pitch.
        let pitch_of = |key: f32, pitch: u8| {
            let mut voices = machine(|p| p.key1 = key);
            heard(&note(&mut voices, pitch, 100, 8192), 60)
        };
        assert_eq!(pitch_of(1.0, 55), 55);
        assert_eq!(pitch_of(1.0, 62), 62);
        assert_eq!(pitch_of(0.0, 55), 60);
        assert_eq!(pitch_of(0.0, 62), 60);
    }

    // ---- claim 7

    #[test]
    fn split_sends_the_low_notes_to_one_oscillator_and_the_high_to_the_other() {
        // OCTAVE is baked an octave above the key it plays, so which
        // oscillator sounded is audible rather than inferred.
        let setup = |p: &mut RomParams| {
            p.pcm1 = 0.0;
            p.pcm2 = 2.0;
            p.mode = 2.0;
            p.split = 60.0;
            p.xfade = 0.0;
        };
        let mut low = machine(setup);
        let mut high = machine(setup);
        assert_eq!(heard(&note(&mut low, 48, 100, 8192), 48), 48);
        assert_eq!(
            heard(&note(&mut high, 72, 100, 8192), 84),
            84,
            "above the split the second oscillator did not take over"
        );
    }

    #[test]
    fn a_crossfade_makes_the_split_a_blend_rather_than_a_wall() {
        let mut voices = machine(|p| {
            p.pcm1 = 0.0;
            p.pcm2 = 2.0;
            p.mode = 2.0;
            p.split = 60.0;
            p.xfade = 24.0;
        });
        let signal = note(&mut voices, 60, 100, 8192);
        let first = magnitude(&signal, hz_of(60));
        let second = magnitude(&signal, hz_of(72));
        assert!(
            first > 1e-4 && second > 1e-4,
            "at the split point only one oscillator sounded: {first} and {second}"
        );
    }

    // ---- claim 8

    #[test]
    fn vintage_at_zero_is_the_clean_path_whatever_the_converter_says() {
        let mut clean = machine(|_| {});
        let mut armed = machine(|p| {
            p.vrate = 4000.0;
            p.vbits = 4.0;
            p.vintage = 0.0;
        });
        assert_eq!(
            note(&mut clean, 60, 100, 4096),
            note(&mut armed, 60, 100, 4096)
        );
    }

    /// What a converter leaves behind is BROADBAND — quantisation noise
    /// and the images of the hold — not a tidy third harmonic. So the
    /// measurement is everything in the steady loop that is not the tone.
    #[test]
    fn vintage_at_one_puts_the_converter_in_the_path() {
        let residual = |vintage: f32| {
            let mut voices = machine(|p| {
                p.vrate = 6000.0;
                p.vbits = 5.0;
                p.vintage = vintage;
            });
            let signal = note(&mut voices, 60, 40, 16_384);
            // Past the attack: the loop is exactly periodic here, so a
            // clean path leaves almost nothing off its own partials.
            let steady = &signal[8_192..];
            let power: f32 = steady.iter().map(|x| x * x).sum::<f32>() / steady.len() as f32;
            let tonal: f32 = (1..=2)
                .map(|harmonic| {
                    let level = magnitude(steady, hz_of(60) * harmonic as f32);
                    2.0 * level * level
                })
                .sum();
            ((power - tonal).max(0.0) / power.max(1e-12)).sqrt()
        };
        let clean = residual(0.0);
        let crushed = residual(1.0);
        assert!(
            crushed > clean * 8.0,
            "the vintage converter left no trace: {clean} then {crushed}"
        );
        let (rate, bits) = RomParams {
            vrate: 6000.0,
            vbits: 5.0,
            vintage: 1.0,
            ..RomParams::default()
        }
        .converter(RATE);
        assert_eq!((rate, bits), (6000.0, 5.0));
    }

    #[test]
    fn a_cut_loop_stays_a_whole_number_of_cycles() {
        let short = RomParams {
            vintage: 1.0,
            vloop: 1.0,
            ..RomParams::default()
        };
        assert!(short.loop_keep() < 0.05);
        assert!(RomParams::default().loop_keep() == 1.0);
        // The engine floors the kept loop to whole cycles, so a shortened
        // loop is still seamless — it only freezes the timbre.
        let mut voices = machine(|p| {
            p.vintage = 1.0;
            p.vloop = 1.0;
        });
        let held = note(&mut voices, 60, 100, 48_000);
        assert!(
            peak(&held[40_000..]) > 0.05,
            "the cut loop stopped sounding"
        );
        assert!(held.iter().all(|x| x.is_finite()));
    }

    // ---- claim 9

    #[test]
    fn a_split_block_renders_the_same_samples() {
        let whole = {
            let mut voices = machine(|_| {});
            note(&mut voices, 60, 100, 512)
        };
        let pieces = {
            let mut voices = machine(|_| {});
            voices.note_on(60, 100, 1);
            let mut out = Vec::new();
            for take in [100usize, 257, 155] {
                out.extend(block(&mut voices, take));
            }
            out
        };
        assert_eq!(whole, pieces);
    }

    // ---- claim 10

    #[test]
    fn rendering_allocates_nothing() {
        let mut voices = machine(|_| {});
        let mut out = vec![0.0; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..64 {
                if i % 8 == 0 {
                    voices.note_on(48 + (i % 24) as u8, 90, i as u64);
                }
                if i % 8 == 5 {
                    voices.note_off(48 + ((i - 5) % 24) as u8);
                }
                if i % 16 == 3 {
                    voices.plock(p::CUTOFF, Some(400.0));
                }
                if i % 16 == 11 {
                    voices.plock(p::CUTOFF, None);
                }
                voices.set_param(p::RESO, 0.4);
                let mut ramp = Ramp::across(1.0, 1.0, out.len());
                voices.render(&mut out, 0, &mut ramp);
            }
        });
    }

    // ---- claim 11

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in p::TABLE {
            for extreme in [def.min, def.max] {
                let mut voices = machine(|p| p.set(def.id, extreme));
                voices.note_on(45, 100, 1);
                voices.note_on(69, 20, 2);
                let out = block(&mut voices, 1024);
                voices.note_off(45);
                let tail = block(&mut voices, 1024);
                assert!(
                    out.iter().chain(tail.iter()).all(|x| x.is_finite()),
                    "{} at {extreme} left the finite world",
                    def.name
                );
            }
        }
    }

    // ---- claim 12

    #[test]
    fn every_parameter_round_trips_and_a_stranger_is_ignored() {
        let mut params = RomParams::default();
        for def in p::TABLE {
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), def.min, "{}", def.name);
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), def.max, "{}", def.name);
            params.set(def.id, def.max + 1000.0);
            assert_eq!(params.get(def.id), def.max, "{} was not clamped", def.name);
            params.set(def.id, f32::NAN);
            assert_eq!(params.get(def.id), def.default, "{} kept a NaN", def.name);
        }
        let before = params;
        params.set(9_999, 1.0);
        assert_eq!(params, before);
    }

    // ---- claim 13

    #[test]
    fn a_lock_is_heard_and_the_knob_comes_back() {
        let brightness = |voices: &mut RomVoices, pitch: u8| {
            let signal = note(voices, pitch, 100, 4096);
            magnitude(&signal, hz_of(60) * 2.0) / magnitude(&signal, hz_of(60)).max(1e-9)
        };
        let mut voices = machine(|p| {
            p.cutoff = 18_000.0;
            p.reso = 0.0;
        });
        let open = brightness(&mut voices, 60);
        voices.note_off(60);
        let _ = block(&mut voices, 24_000);

        voices.plock(p::CUTOFF, Some(200.0));
        let locked = brightness(&mut voices, 60);
        voices.note_off(60);
        let _ = block(&mut voices, 24_000);

        voices.plock(p::CUTOFF, None);
        let back = brightness(&mut voices, 60);
        assert!(locked < open * 0.5, "the lock was not heard");
        assert!(back > locked * 2.0, "the knob did not come back");
    }

    #[test]
    fn a_turn_reaches_a_held_note_unless_that_note_is_locked() {
        let mut voices = machine(|_| {});
        voices.note_on(60, 100, 1);
        voices.set_param(p::LEVEL, 0.25);
        let quiet = peak(&block(&mut voices, 4096));

        let mut locked = machine(|_| {});
        locked.plock(p::LEVEL, Some(1.0));
        locked.note_on(60, 100, 1);
        locked.set_param(p::LEVEL, 0.25);
        let held = peak(&block(&mut locked, 4096));
        assert!(held > quiet * 2.0, "a locked note followed the knob");
    }

    // ---- claim 14

    #[test]
    fn every_sub_page_has_a_picture_inside_the_unit_square() {
        let params = RomParams {
            fenv: 0.5,
            vintage: 0.6,
            cmix: 0.4,
            mode: 2.0,
            ..RomParams::default()
        };
        let mut pages = 0;
        for key in p::KEYS.iter().flatten() {
            for page in key.subpages {
                pages += 1;
                for selected in page
                    .slots
                    .iter()
                    .flatten()
                    .map(|id| Some(*id))
                    .chain([None])
                {
                    let hero = hero(&params, page.title, selected)
                        .unwrap_or_else(|| panic!("{} has no picture", page.title));
                    for series in &hero.series {
                        for (x, y) in &series.points {
                            assert!(
                                (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y),
                                "{}: {x},{y} is outside the unit square",
                                page.title
                            );
                        }
                    }
                    for mark in &hero.marks {
                        assert!(
                            (0.0..=1.0).contains(&mark.x),
                            "{}: a mark at {} is off the picture",
                            page.title,
                            mark.x
                        );
                    }
                }
            }
        }
        assert_eq!(pages, 9, "the key table changed shape");
    }

    /// The filter picture is analytic; the filter is a kernel. This holds
    /// the one to the other, which is what stops the picture drifting.
    #[test]
    fn the_filter_picture_matches_the_filter() {
        for (cutoff, q) in [(400.0f32, 0.707f32), (2000.0, 4.0), (8000.0, 0.707)] {
            for hz in [100.0f32, 500.0, 1500.0, 5000.0] {
                let mut filter = Svf::new();
                filter.prepare(RATE, cutoff, q);
                let frames = 24_000;
                let mut probe: Vec<f32> = (0..frames)
                    .map(|at| (std::f32::consts::TAU * hz * at as f32 / RATE).sin())
                    .collect();
                filter.process(&mut probe, Mode::Lowpass);
                // Skip the transient; measure what settles.
                let settled = &probe[frames / 2..];
                let measured = 20.0
                    * (settled.iter().map(|x| x * x).sum::<f32>() / settled.len() as f32)
                        .sqrt()
                        .max(1e-9)
                        .log10()
                    + 3.0103;
                let drawn = response_db(Mode::Lowpass, 1, cutoff, q, hz, RATE);
                assert!(
                    (measured - drawn).abs() < 1.0,
                    "at {hz} Hz through {cutoff} Hz: the picture says {drawn} dB, the filter does {measured} dB"
                );
            }
        }
    }

    #[test]
    fn the_pcm_list_is_the_bank_with_the_oscillators_marked() {
        let params = RomParams {
            pcm1: 1.0,
            pcm2: 3.0,
            ..RomParams::default()
        };
        let list = pcm_list(&params, "Osc 1").expect("osc 1 has a list");
        assert_eq!(list.rows.len(), bank::MULTIS.len());
        assert_eq!(list.selected, 1);
        assert_eq!(list.param, p::PCM1);
        assert_eq!(list.rows[1].tags, "1");
        assert_eq!(list.rows[3].tags, "2");
        assert!(list.rows[0].tags.is_empty());
        // The groups arrive in bank order, and a row says what it is.
        assert_eq!(list.rows[0].group, "Tone");
        assert_eq!(list.rows[3].group, "Decay");
        assert_eq!(list.rows[0].detail, "10z");
        assert_eq!(list.rows[1].detail, "5z +7st");
        assert_eq!(list.rows[3].detail, "3z shot");
        // Osc 2's list is the same bank addressed through its own cell.
        let second = pcm_list(&params, "Osc 2").expect("osc 2 has a list");
        assert_eq!(second.selected, 3);
        assert_eq!(second.param, p::PCM2);
        assert!(pcm_list(&params, "Layer").is_none());
    }

    /// The same trip a knob makes on every surface: engine value to the
    /// normalised position and back. A choice must land on a segment and
    /// stay there; a sweep must come back where it started.
    #[test]
    fn every_knob_survives_the_trip_to_the_surface_and_back() {
        use crate::ui::device::{rom_is_discrete, rom_norm, rom_value};
        for def in p::TABLE {
            for step in 0..=8 {
                let value = def.min + (def.max - def.min) * step as f32 / 8.0;
                let there = rom_norm(def.id, value);
                assert!(
                    (0.0..=1.0).contains(&there),
                    "{} maps {value} outside the knob",
                    def.name
                );
                let back = rom_value(def.id, there);
                if rom_is_discrete(def.id) {
                    assert!(
                        (back - back.round()).abs() < 1e-4,
                        "{} snapped to {back}, which is not a segment",
                        def.name
                    );
                    assert_eq!(
                        rom_value(def.id, rom_norm(def.id, back)),
                        back,
                        "{} kept moving after it snapped",
                        def.name
                    );
                } else {
                    assert!(
                        (back - value).abs() <= value.abs() * 1e-4 + 1e-4,
                        "{} {value} -> {there} -> {back}",
                        def.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_voice_allowance_is_honoured_and_the_oldest_note_is_stolen() {
        let mut voices = machine(|p| p.voices = 2.0);
        for (at, pitch) in [48u8, 55, 62].into_iter().enumerate() {
            voices.note_on(pitch, 100, at as u64 + 1);
        }
        let signal = block(&mut voices, 8192);
        assert!(
            magnitude(&signal, hz_of(48)) < magnitude(&signal, hz_of(62)) * 0.1,
            "the third note did not steal the first"
        );
        assert!(
            magnitude(&signal, hz_of(55)) > 1e-4,
            "the second note was lost"
        );
    }
}
