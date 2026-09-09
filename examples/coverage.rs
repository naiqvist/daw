//! Offline, reproducible timbre-plane measurements. No audio device or strip.
//!
//! RUSTC_WRAPPER="" cargo run --example coverage -- --out /tmp/coverage --samples 3000
//! python3 tools/coverage.py /tmp/coverage --accept
//!
//! The plan's 700 ms render cannot distinguish a 2 s decay from a sustain,
//! and its 500–700 ms window follows note-off. We render two independent
//! 2.4 s trajectories: held throughout, and released at 400 ms. See the
//! accompanying note for exact descriptor, cost, and acceptance definitions.
use daw::{
    audio::{
        acid::AcidVoice, clay::ClayVoice, drum::DrumVoice, glass::GlassVoices, graph::Ramp,
        mass::MassVoices, pipe::PipeVoices, pluck::PluckVoices, prism_voice::PrismVoiceVoices,
        ring::RingVoices, table::TableVoices, thump::ThumpVoice, vox::VoxVoices,
    },
    dsp::fft::{RealFft, Window, fill_window},
    params::{self, ParamDef},
};
use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

const SR: usize = 48_000;
const FRAMES: usize = SR * 24 / 10;
const GATE: usize = SR * 4 / 10;
const FFT: usize = 4096;
const NOTES: [u8; 3] = [36, 48, 60];
const VELOCITIES: [u8; 2] = [64, 127];
const SILENCE: f64 = 1e-7;

/// Adding an engine means implementing this small offline bridge and adding
/// one Adapter row. It does not change descriptors or their acceptance rules.
trait CoverageVoice {
    fn reset(&mut self);
    fn set(&mut self, id: u32, value: f32);
    fn lock(&mut self, id: u32, value: Option<f32>);
    fn on(&mut self, note: u8, velocity: u8);
    fn off(&mut self, note: u8);
    fn render(&mut self, out: &mut [f32]);
    fn latency(&self) -> usize {
        0
    }
}
macro_rules! bridge {
    ($voice:ty, $on:ident, $off:expr) => {
        impl CoverageVoice for $voice {
            fn reset(&mut self) {
                self.reset();
            }
            fn set(&mut self, id: u32, value: f32) {
                self.set_param(id, value);
            }
            fn lock(&mut self, id: u32, value: Option<f32>) {
                self.plock(id, value);
            }
            fn on(&mut self, note: u8, velocity: u8) {
                self.$on(note, velocity);
            }
            fn off(&mut self, note: u8) {
                ($off)(self, note);
            }
            fn render(&mut self, out: &mut [f32]) {
                self.render_add(out, 1.0);
            }
        }
    };
}
bridge!(ThumpVoice, trigger, |v: &mut ThumpVoice, _| v.release());
bridge!(ClayVoice, trigger, |v: &mut ClayVoice, _| v.release());
bridge!(DrumVoice, trigger, |_v: &mut DrumVoice, _| {});
impl CoverageVoice for AcidVoice {
    fn reset(&mut self) {
        self.reset();
    }
    fn set(&mut self, id: u32, value: f32) {
        self.set_param(id, value);
    }
    fn lock(&mut self, id: u32, value: Option<f32>) {
        self.plock(id, value);
    }
    fn on(&mut self, note: u8, velocity: u8) {
        self.note_on(note, velocity);
    }
    fn off(&mut self, note: u8) {
        self.note_off(note);
    }
    fn render(&mut self, out: &mut [f32]) {
        self.render_add(out, self.params().level);
    }
}

macro_rules! poly_bridge {
    ($voice:ty, $velocity:expr) => {
        impl CoverageVoice for $voice {
            fn reset(&mut self) {
                self.reset();
            }
            fn set(&mut self, id: u32, value: f32) {
                self.set_param(id, value);
            }
            fn lock(&mut self, id: u32, value: Option<f32>) {
                self.plock(id, value);
            }
            fn on(&mut self, note: u8, velocity: u8) {
                self.note_on(note, ($velocity)(velocity), 0);
            }
            fn off(&mut self, note: u8) {
                self.note_off(note);
            }
            fn latency(&self) -> usize {
                self.latency()
            }
            fn render(&mut self, out: &mut [f32]) {
                let mut gain = Ramp::across(1., 1., out.len());
                self.render(out, 0, &mut gain);
                // The plane describes the mono-compatible voice output.
                // Stereo cancellation remains visible rather than taking
                // whichever channel happens to give a favorable descriptor.
                let len = out.len();
                for (l, r) in out.iter_mut().zip(self.right(len)) {
                    *l = (*l + *r) * 0.5;
                }
            }
        }
    };
}
poly_bridge!(TableVoices, |v: u8| v);
poly_bridge!(RingVoices, |v: u8| v);
poly_bridge!(PrismVoiceVoices, |v: u8| v);
poly_bridge!(GlassVoices, |v: u8| v);
poly_bridge!(MassVoices, |v: u8| v);
poly_bridge!(PluckVoices, |v: u8| v);
poly_bridge!(VoxVoices, |v: u8| v);
poly_bridge!(PipeVoices, |v: u8| v);

struct Adapter {
    name: &'static str,
    table: &'static [ParamDef],
    x: u32,
    y: u32,
    t: u32,
    counts: bool,
    band_required: bool,
    make: fn() -> Box<dyn CoverageVoice>,
}
fn adapters() -> Vec<Adapter> {
    let mut adapters = vec![
        Adapter {
            name: "thump",
            table: params::thump::TABLE,
            x: params::thump::NOISE,
            y: params::thump::COLOR,
            t: params::thump::DECAY,
            counts: true,
            band_required: false,
            make: || {
                let mut v = ThumpVoice::new();
                v.prepare(SR as f32, Default::default());
                Box::new(v)
            },
        },
        Adapter {
            name: "acid",
            table: params::acid::TABLE,
            x: params::acid::WAVE,
            y: params::acid::CUTOFF,
            t: params::acid::DECAY,
            counts: true,
            band_required: false,
            make: || {
                let mut v = AcidVoice::new();
                v.prepare(SR as f32, Default::default());
                Box::new(v)
            },
        },
        Adapter {
            name: "clay",
            table: params::clay::TABLE,
            x: params::clay::MATTER,
            y: params::clay::SIZE,
            t: params::clay::DECAY,
            counts: true,
            band_required: true,
            make: || {
                let mut v = ClayVoice::new();
                v.prepare(SR as f32, Default::default());
                Box::new(v)
            },
        },
        Adapter {
            name: "drum",
            table: params::drum::TABLE,
            x: params::drum::MODEL,
            y: params::drum::TONE,
            t: params::drum::DECAY,
            counts: false,
            band_required: false,
            make: || {
                let mut v = DrumVoice::new();
                v.prepare(SR as f32, Default::default());
                Box::new(v)
            },
        },
    ];
    macro_rules! add {
        ($module:ident,$voice:ty,$name:literal) => {
            adapters.push(Adapter {
                name: $name,
                table: params::$module::TABLE,
                x: params::$module::WALK_X,
                y: params::$module::WALK_Y,
                t: params::$module::WALK_T,
                counts: true,
                band_required: true,
                make: || {
                    let mut voice = <$voice>::new();
                    voice.prepare(SR as f32, 512, Default::default());
                    Box::new(voice)
                },
            });
        };
    }
    add!(table, TableVoices, "table");
    add!(ring, RingVoices, "ring");
    add!(prism_voice, PrismVoiceVoices, "prism_voice");
    add!(mass, MassVoices, "mass");
    add!(pluck, PluckVoices, "pluck");
    add!(vox, VoxVoices, "vox");
    add!(pipe, PipeVoices, "pipe");
    add!(glass, GlassVoices, "glass");
    adapters
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u32 << 24) as f32
    }
    fn index(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }
}
fn lhs(table: &[ParamDef], n: usize, rng: &mut Rng) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0.; table.len()]; n];
    for (j, p) in table.iter().enumerate() {
        let mut strata: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let k = rng.index(i + 1);
            strata.swap(i, k);
        }
        for (i, k) in strata.into_iter().enumerate() {
            out[i][j] = p.min + (p.max - p.min) * (k as f32 + rng.unit()) / n as f32;
        }
    }
    out
}
fn at(table: &[ParamDef], id: u32) -> usize {
    table
        .iter()
        .position(|p| p.id == id)
        .expect("adapter walker must be in its table")
}
fn set_fraction(a: &Adapter, values: &mut [f32], id: u32, fraction: f32) {
    let i = at(a.table, id);
    let p = a.table[i];
    values[i] = p.min + (p.max - p.min) * fraction;
}
fn distance(a: &Adapter, values: &[f32], starter: &[f32]) -> (usize, usize, f64) {
    let mut turns = 0;
    let mut cost5 = 0;
    let mut distance = 0.;
    for ((p, v), s) in a.table.iter().zip(values).zip(starter) {
        let d = ((*v - *s) / (p.max - p.min).max(f32::EPSILON)).abs() as f64;
        turns += usize::from(d > 1e-6);
        cost5 += usize::from(d > 0.05);
        distance += d * d;
    }
    (turns, cost5, distance.sqrt())
}
fn starter(a: &Adapter, dir: &Path) -> (Vec<f32>, String) {
    let mut values: Vec<_> = a.table.iter().map(|p| p.default).collect();
    let mut records = daw::sound::list(dir);
    records.sort_by_key(|r| (r.lane != a.name, r.path.clone()));
    let display_name = if a.name == "prism_voice" {
        "prism"
    } else {
        a.name
    };
    let named_starter = format!("{display_name} starter");
    for record in records
        .iter()
        .filter(|r| r.name == daw::sound::STARTER || r.name == named_starter)
    {
        let Ok(sound) = daw::sound::load(&record.path) else {
            continue;
        };
        let Some(machine) = sound.machine.filter(|m| m.kind == a.name) else {
            continue;
        };
        for (id, value) in machine.overrides {
            if let Some(i) = a.table.iter().position(|p| p.id == id) {
                values[i] = a.table[i].clamp(value);
            }
        }
        return (values, record.path.display().to_string());
    }
    (values, "defaults (no matching machine starter)".into())
}

#[derive(Clone, Copy, Debug, Default)]
struct Spectrum {
    rms: f64,
    centroid: f64,
    flatness: f64,
    inharmonicity: f64,
    mean_semitones: f64,
    x: f64,
}
struct Analyzer {
    fft: RealFft,
    window: Vec<f32>,
    input: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    scratch: Vec<f32>,
    power: Vec<f64>,
}
impl Analyzer {
    fn new() -> Self {
        let mut fft = RealFft::new();
        assert!(fft.prepare(FFT));
        let mut window = vec![0.; FFT];
        fill_window(Window::Hann, &mut window);
        Self {
            fft,
            window,
            input: vec![0.; FFT],
            re: vec![0.; FFT / 2 + 1],
            im: vec![0.; FFT / 2 + 1],
            scratch: vec![0.; RealFft::scratch_len(FFT)],
            power: vec![0.; FFT / 2 + 1],
        }
    }
    fn spectrum(&mut self, samples: &[f32]) -> Spectrum {
        let rms = (samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>()
            / samples.len().max(1) as f64)
            .sqrt();
        if rms < SILENCE || !rms.is_finite() {
            return Spectrum {
                rms,
                ..Default::default()
            };
        }
        self.power.fill(0.);
        let mut frames = 0;
        for start in (0..samples.len().saturating_sub(FFT) + 1).step_by(FFT / 2) {
            for (i, x) in self.input.iter_mut().enumerate() {
                *x = samples.get(start + i).copied().unwrap_or(0.) * self.window[i];
            }
            self.fft
                .forward(&self.input, &mut self.re, &mut self.im, &mut self.scratch);
            for ((p, re), im) in self.power.iter_mut().zip(&self.re).zip(&self.im) {
                *p += f64::from(*re).powi(2) + f64::from(*im).powi(2);
            }
            frames += 1;
        }
        for p in &mut self.power {
            *p /= frames.max(1) as f64;
        }
        let lo = (30. * FFT as f64 / SR as f64).ceil() as usize;
        let hi = (16_000. * FFT as f64 / SR as f64).floor() as usize;
        let powers = &self.power[lo..=hi];
        let sum = powers.iter().sum::<f64>();
        if sum <= 1e-30 {
            return Spectrum {
                rms,
                ..Default::default()
            };
        }
        let centroid = powers
            .iter()
            .enumerate()
            .map(|(i, p)| *p * (i + lo) as f64 * SR as f64 / FFT as f64)
            .sum::<f64>()
            / sum;
        let floor = sum / powers.len() as f64 * 1e-12;
        let geometric =
            (powers.iter().map(|p| p.max(floor).ln()).sum::<f64>() / powers.len() as f64).exp();
        let flatness = (geometric / (sum / powers.len() as f64)).clamp(0., 1.);
        let mut peaks: Vec<_> = (lo.max(1)..hi)
            .filter(|i| self.power[*i] > self.power[*i - 1] && self.power[*i] >= self.power[*i + 1])
            .map(|i| {
                let l = self.power[i - 1].max(1e-30).ln();
                let c = self.power[i].max(1e-30).ln();
                let r = self.power[i + 1].max(1e-30).ln();
                let offset = (0.5 * (l - r) / (l - 2. * c + r).min(-1e-15)).clamp(-0.5, 0.5);
                ((i as f64 + offset) * SR as f64 / FFT as f64, self.power[i])
            })
            .collect();
        peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
        let threshold = peaks.first().map_or(0., |p| p.1) * 1e-6;
        peaks.retain(|p| p.1 >= threshold);
        peaks.truncate(8);
        let deviation = |f: f64, f0: f64| {
            let k = (f / f0).round().max(1.);
            (12. * (f / (k * f0)).log2()).abs()
        };
        let fitted = peaks
            .iter()
            .map(|(f0, _)| {
                let relations = peaks
                    .iter()
                    .filter(|(f, _)| deviation(*f, *f0) <= 0.25)
                    .count();
                let mean = peaks.iter().map(|(f, _)| deviation(*f, *f0)).sum::<f64>()
                    / peaks.len().max(1) as f64;
                (relations, mean, *f0)
            })
            .min_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        // Six semitones is half an octave. The plan did not specify its
        // squash, so this declared, monotonic calibration is reproducible.
        let mean_semitones = fitted.map_or(0., |(_, mean, _)| mean);
        let inharmonicity = (mean_semitones / 6.).clamp(0., 1.);
        Spectrum {
            rms,
            centroid,
            flatness,
            inharmonicity,
            mean_semitones,
            x: 0.6 * inharmonicity + 0.4 * flatness,
        }
    }
}
fn calibrate(dir: &Path) -> Result<(), Box<dyn Error>> {
    use daw::dsp::noise::{PinkNoise, WhiteNoise};
    let mut file = BufWriter::new(File::create(dir.join("calibration.csv"))?);
    writeln!(
        file,
        "fixture,reference_note,fundamental_hz,rms,centroid,flatness,mean_semitones,legacy_inharmonicity,legacy_x,v2_25cent_inharmonicity,v2_25cent_x"
    )?;
    let mut analyzer = Analyzer::new();
    let n = SR * 3 / 10;
    for note in NOTES {
        let fundamental = 440. * 2f64.powf((f64::from(note) - 69.) / 12.);
        for fixture in ["sine", "saw", "stiff_string", "bell", "white", "pink"] {
            let mut audio = vec![0.; n];
            match fixture {
                "white" => {
                    let mut noise = WhiteNoise::new();
                    noise.seed(0x51eed + u64::from(note));
                    noise.process(&mut audio);
                }
                "pink" => {
                    let mut noise = PinkNoise::new();
                    noise.seed(0x51eed + u64::from(note));
                    let mut warm = vec![0.; SR];
                    noise.process(&mut warm);
                    noise.process(&mut audio);
                }
                _ => {
                    let partials: Vec<(f64, f64)> = match fixture {
                        "sine" => vec![(1., 1.)],
                        "saw" => (1..=64)
                            .map(|k| (f64::from(k), 1. / f64::from(k)))
                            .collect(),
                        "stiff_string" => (1..=16)
                            .map(|k| {
                                let k = f64::from(k);
                                (k * (1. + 0.003 * k * k).sqrt(), 1. / k)
                            })
                            .collect(),
                        "bell" => [1., 2.756, 5.404, 8.933, 13.34, 18.64]
                            .into_iter()
                            .enumerate()
                            .map(|(i, ratio)| (ratio, 1. / (i + 1) as f64))
                            .collect(),
                        _ => Vec::new(),
                    };
                    for (i, sample) in audio.iter_mut().enumerate() {
                        *sample = partials
                            .iter()
                            .filter(|(ratio, _)| ratio * fundamental < SR as f64 * 0.45)
                            .map(|(ratio, weight)| {
                                (std::f64::consts::TAU * fundamental * ratio * i as f64 / SR as f64)
                                    .sin()
                                    * weight
                            })
                            .sum::<f64>() as f32;
                    }
                }
            }
            let s = analyzer.spectrum(&audio);
            let quarter = s.mean_semitones / (s.mean_semitones + 0.25);
            let x = 0.6 * quarter + 0.4 * s.flatness;
            writeln!(
                file,
                "{},{},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9}",
                fixture,
                note,
                fundamental,
                s.rms,
                s.centroid,
                s.flatness,
                s.mean_semitones,
                s.inharmonicity,
                s.x,
                quarter,
                x
            )?;
            eprintln!(
                "calibration {fixture} note{note}: d={:.4}st flatness={:.4} legacyX={:.4} 25centX={:.4}",
                s.mean_semitones, s.flatness, s.x, x
            );
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Envelope {
    peak: f64,
    decay_ms: f64,
    censored: bool,
    class: &'static str,
}
fn envelope(samples: &[f32]) -> Envelope {
    // 5 ms RMS blocks resolve the 80 ms boundary; a last crossing avoids
    // treating a beat or modulation dip as the end of a sound.
    let rms: Vec<f64> = samples
        .chunks(SR / 200)
        .map(|b| (b.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / b.len() as f64).sqrt())
        .collect();
    let peak = samples.iter().fold(0f64, |p, s| p.max(f64::from(*s).abs()));
    let (peak_i, peak_rms) = rms
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or((0, 0.), |(i, p)| (i, *p));
    if peak_rms < SILENCE {
        return Envelope {
            peak,
            decay_ms: 0.,
            censored: false,
            class: "SILENT",
        };
    }
    let last = rms
        .iter()
        .rposition(|v| *v >= peak_rms * 0.1)
        .unwrap_or(peak_i);
    let censored = last + 20 >= rms.len();
    let decay_ms = (last.saturating_sub(peak_i) + 1) as f64 * 5.;
    let class = if censored || decay_ms >= 2000. {
        "SUSTAINED"
    } else if decay_ms < 80. {
        "TRANSIENT"
    } else {
        "DECAYING"
    };
    Envelope {
        peak,
        decay_ms,
        censored,
        class,
    }
}

fn gate_holds(held: &[f32], released: &[f32]) -> bool {
    let rms =
        |a: &[f32]| (a.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / a.len() as f64).sqrt();
    let early = rms(&held[SR * 15 / 10..SR * 18 / 10]);
    let late = rms(&held[SR * 21 / 10..SR * 24 / 10]);
    let off = rms(&released[SR * 21 / 10..SR * 24 / 10]);
    // Detect an audible, approximately steady level supported by the gate,
    // including sustain levels below -20 dB of a loud initial attack. Merely
    // surviving at one second is not a sustain: a 1.4 s exponential decay
    // must remain DECAYING.
    late > SILENCE && late > off * 4. + SILENCE && late > early * 0.7 && late < early * 1.4
}
fn render(
    voice: &mut dyn CoverageVoice,
    a: &Adapter,
    values: &[f32],
    note: u8,
    velocity: u8,
    held: bool,
    out: &mut [f32],
) {
    voice.reset();
    for (p, v) in a.table.iter().zip(values) {
        voice.set(p.id, *v);
    }
    voice.on(note, velocity);
    let latency = voice.latency();
    let mut raw = vec![0.; out.len() + latency];
    for block in raw[..GATE].chunks_mut(512) {
        voice.render(block);
    }
    if !held {
        voice.off(note);
    }
    for block in raw[GATE..].chunks_mut(512) {
        voice.render(block);
    }
    out.copy_from_slice(&raw[latency..]);
}
fn csv(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}
struct Output<W: Write> {
    renders: BufWriter<W>,
    vectors: BufWriter<W>,
    next: usize,
    analyzer: Analyzer,
    held: Vec<f32>,
    released: Vec<f32>,
}
impl Output<File> {
    fn new(dir: &Path) -> Result<Self, Box<dyn Error>> {
        Self::with_writers(
            File::create(dir.join("renders.csv"))?,
            File::create(dir.join("vectors.csv"))?,
        )
    }
}
impl<W: Write> Output<W> {
    fn with_writers(renders: W, vectors: W) -> Result<Self, Box<dyn Error>> {
        let mut renders = BufWriter::new(renders);
        writeln!(
            renders,
            "machine,vector,source,parent,y_setting,walk_step,note,velocity,turns,cost5,distance,valid,peak,decay_ms,decay_censored,time_class,release_decay_ms,release_censored,attack_rms,attack_centroid,attack_flatness,attack_inharmonicity,attack_x,attack_mean_semitones,held_rms,held_centroid,held_flatness,held_inharmonicity,held_x,held_mean_semitones,post_release_rms,post_release_centroid,post_release_flatness,post_release_inharmonicity,post_release_x,post_release_mean_semitones"
        )?;
        let mut vectors = BufWriter::new(vectors);
        writeln!(
            vectors,
            "machine,vector,source,param,name,value,starter,normalized_distance"
        )?;
        Ok(Self {
            renders,
            vectors,
            next: 0,
            analyzer: Analyzer::new(),
            held: vec![0.; FRAMES],
            released: vec![0.; FRAMES],
        })
    }
    fn measure(
        &mut self,
        a: &Adapter,
        voice: &mut dyn CoverageVoice,
        values: &[f32],
        starter: &[f32],
        source: &str,
        parent: Option<usize>,
        y: Option<usize>,
        step: Option<usize>,
    ) -> Result<usize, Box<dyn Error>> {
        let id = self.next;
        self.next += 1;
        let (turns, cost5, distance) = distance(a, values, starter);
        for ((p, v), s) in a.table.iter().zip(values).zip(starter) {
            writeln!(
                self.vectors,
                "{},{},{},{},{},{:.9},{:.9},{:.9}",
                a.name,
                id,
                source,
                p.id,
                csv(p.name),
                v,
                s,
                (v - s) / (p.max - p.min).max(f32::EPSILON)
            )?;
        }
        for note in NOTES {
            for velocity in VELOCITIES {
                render(voice, a, values, note, velocity, true, &mut self.held);
                render(voice, a, values, note, velocity, false, &mut self.released);
                let valid = self
                    .held
                    .iter()
                    .chain(&self.released)
                    .all(|s| s.is_finite());
                let mut e = envelope(&self.held);
                if gate_holds(&self.held, &self.released) {
                    e.class = "SUSTAINED";
                }
                let release = envelope(&self.released);
                let attack = self.analyzer.spectrum(&self.held[..SR * 3 / 10]);
                let held = self.analyzer.spectrum(&self.held[SR..SR * 13 / 10]);
                let post = self.analyzer.spectrum(&self.released[SR / 2..SR * 7 / 10]);
                write!(
                    self.renders,
                    "{},{},{},{},{},{},{},{},{},{},{:.9},{},{:.9},{:.3},{},{},{:.3},{}",
                    a.name,
                    id,
                    source,
                    parent.map_or(String::new(), |v| v.to_string()),
                    y.map_or(String::new(), |v| v.to_string()),
                    step.map_or(String::new(), |v| v.to_string()),
                    note,
                    velocity,
                    turns,
                    cost5,
                    distance,
                    valid,
                    e.peak,
                    e.decay_ms,
                    e.censored,
                    e.class,
                    release.decay_ms,
                    release.censored
                )?;
                for s in [attack, held, post] {
                    write!(
                        self.renders,
                        ",{:.9},{:.5},{:.9},{:.9},{:.9},{:.9}",
                        s.rms, s.centroid, s.flatness, s.inharmonicity, s.x, s.mean_semitones
                    )?;
                }
                writeln!(self.renders)?;
            }
        }
        Ok(id)
    }
}

/// Contiguous shards retain the serial vector IDs and CSV ordering. All audio
/// state and floating-point settings belong to the worker that renders them.
/// This is strictly offline; no workers or channels enter the audio engine.
fn measure_batch<W: Write>(
    output: &mut Output<W>,
    a: &Adapter,
    voice: &mut dyn CoverageVoice,
    starter: &[f32],
    jobs: &[(Vec<f32>, Option<usize>)],
    source: &str,
    workers: usize,
) -> Result<(), Box<dyn Error>> {
    if jobs.is_empty() {
        return Ok(());
    }
    if workers == 1 {
        for (i, (values, parent)) in jobs.iter().enumerate() {
            output.measure(a, voice, values, starter, source, *parent, None, None)?;
            if (i + 1) % 100 == 0 {
                eprintln!("{}: {}/{} {source} vectors", a.name, i + 1, jobs.len());
            }
        }
        return Ok(());
    }
    let base = output.next;
    let chunk = jobs.len().div_ceil(workers.min(jobs.len()));
    let completed = std::sync::atomic::AtomicUsize::new(0);
    let shards = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (shard, jobs) in jobs.chunks(chunk).enumerate() {
            let completed = &completed;
            let total = jobs.len();
            handles.push(scope.spawn(move || -> Result<(Vec<u8>, Vec<u8>), String> {
                match_callback_float_mode();
                let mut local =
                    Output::with_writers(Vec::new(), Vec::new()).map_err(|e| e.to_string())?;
                local.next = base + shard * chunk;
                let mut voice = (a.make)();
                for (values, parent) in jobs {
                    local
                        .measure(a, &mut *voice, values, starter, source, *parent, None, None)
                        .map_err(|e| e.to_string())?;
                    let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if done % 100 == 0 {
                        eprintln!("{}: {done} {source} vectors rendered", a.name);
                    }
                }
                debug_assert_eq!(local.next, base + shard * chunk + total);
                Ok((
                    local.renders.into_inner().map_err(|e| e.to_string())?,
                    local.vectors.into_inner().map_err(|e| e.to_string())?,
                ))
            }));
        }
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| "coverage worker panicked".to_owned())?
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .map_err(std::io::Error::other)?;
    for (renders, vectors) in shards {
        // Every private output has the exact same one-line schema header.
        let render_start = renders
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("missing render header")?
            + 1;
        let vector_start = vectors
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("missing vector header")?
            + 1;
        output.renders.write_all(&renders[render_start..])?;
        output.vectors.write_all(&vectors[vector_start..])?;
    }
    output.next += jobs.len();
    Ok(())
}

struct Options {
    out: PathBuf,
    samples: usize,
    probes: usize,
    steps: usize,
    seed: u64,
    machines: Vec<String>,
    sounds: PathBuf,
    calibration_only: bool,
    workers: usize,
}
fn options() -> Result<Options, Box<dyn Error>> {
    let mut o = Options {
        out: "/tmp/daw-coverage".into(),
        samples: 3000,
        probes: 300,
        steps: 20,
        seed: 0x5eed_c0de,
        machines: Vec::new(),
        sounds: daw::sound::dir(),
        calibration_only: false,
        workers: 1,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--calibrate-only" {
            o.calibration_only = true;
            continue;
        }
        if arg == "--help" {
            println!(
                "coverage [--out DIR] [--machines NAMES] [--samples 3000] [--probes 300] [--walk-steps 20] [--seed INTEGER] [--sounds DIR] [--calibrate-only] [--workers 1]\nMachines: thump,acid,clay,drum,table,ring,prism_voice,mass,pluck,vox,pipe,glass.\nAll renders: 48 kHz; independent 2.4 s held/released trajectories; notes 36/48/60 and velocities 64/127. Samples 0 skips LHS. Probes 0 skips fine probes. Short runs never certify full coverage."
            );
            std::process::exit(0);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--out" => o.out = value.into(),
            "--samples" => o.samples = value.parse()?,
            "--probes" => o.probes = value.parse()?,
            "--walk-steps" => o.steps = value.parse()?,
            "--seed" => o.seed = value.parse()?,
            "--machines" => o.machines = value.split(',').map(str::to_owned).collect(),
            "--sounds" => o.sounds = value.into(),
            "--workers" => o.workers = value.parse()?,
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    if !(1..=64).contains(&o.workers) {
        return Err("workers must be between 1 and 64".into());
    }
    if o.steps < 2 || o.seed == 0 {
        return Err("walk-steps must be >=2 and seed must be nonzero".into());
    }
    Ok(o)
}
fn main() -> Result<(), Box<dyn Error>> {
    let o = options()?;
    match_callback_float_mode();
    let all = adapters();
    for name in &o.machines {
        if !all.iter().any(|a| a.name == name) {
            return Err(format!("unregistered coverage adapter: {name}").into());
        }
    }
    std::fs::create_dir_all(&o.out)?;
    let executable = std::env::current_exe()?;
    let checksum = std::process::Command::new("sha256sum")
        .arg(&executable)
        .output()?;
    if !checksum.status.success() {
        return Err("sha256sum could not identify the coverage executable".into());
    }
    let checksum = String::from_utf8(checksum.stdout)?;
    let checksum = checksum
        .split_whitespace()
        .next()
        .ok_or("sha256sum returned no digest")?;
    if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid executable checksum".into());
    }
    let built = std::fs::metadata(&executable)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut run = BufWriter::new(File::create(o.out.join("run.csv"))?);
    writeln!(
        run,
        "key,value\nexecutable_sha256,{checksum}\nexecutable_modified_unix,{built}\nstarted_unix,{started}\ncalibration_only,{}",
        o.calibration_only
    )?;
    writeln!(
        run,
        "schema,2\nsample_rate,{SR}\nframes,{FRAMES}\ngate_frames,{GATE}\nlhs_vectors,{}\nfine_probes,{}\nwalk_steps,{}\nseed,{}\nx86_ftz_daz,{}\nlock_trials_per_machine,90\nlock_trajectories_per_machine,270\nheld_window,1.0..1.3\npost_release_window,0.5..0.7\ninharmonicity_squash,mean_abs_semitones/6\ncost_method,verified_starter_macro_plus_two_cells",
        o.samples,
        o.probes,
        o.steps,
        o.seed,
        cfg!(target_arch = "x86_64")
    )?;
    writeln!(run, "workers,{}", o.workers)?;
    writeln!(
        run,
        "build_dev_opt_level_override,{}",
        option_env!("CARGO_PROFILE_DEV_OPT_LEVEL").unwrap_or("manifest")
    )?;
    run.flush()?;
    calibrate(&o.out)?;
    if o.calibration_only {
        return Ok(());
    }
    let mut machines = BufWriter::new(File::create(o.out.join("machines.csv"))?);
    writeln!(
        machines,
        "machine,counts,band_required,x,y,t,latency_samples,starter"
    )?;
    let mut output = Output::new(&o.out)?;
    let mut timings = BufWriter::new(File::create(o.out.join("timings.csv"))?);
    writeln!(
        timings,
        "machine,vectors,conditions,simulated_seconds,wall_seconds,lhs_wall_seconds,probe_wall_seconds,fixed_wall_seconds"
    )?;
    for a in all
        .iter()
        .filter(|a| o.machines.is_empty() || o.machines.iter().any(|n| n == a.name))
    {
        let time = std::time::Instant::now();
        let first_vector = output.next;
        let mut rng = Rng(o.seed
            ^ a.name
                .bytes()
                .fold(0u64, |s, c| s.wrapping_mul(131) + c as u64));
        let (starter, origin) = starter(a, &o.sounds);
        let mut voice = (a.make)();
        writeln!(
            machines,
            "{},{},{},{},{},{},{},{}",
            a.name,
            a.counts,
            a.band_required,
            a.x,
            a.y,
            a.t,
            voice.latency(),
            csv(&origin)
        )?;
        output.measure(
            a,
            &mut *voice,
            &starter,
            &starter,
            "starter",
            None,
            None,
            None,
        )?;
        let mut macros = Vec::new();
        for walker in [a.x, a.y, a.t] {
            for i in 0..5 {
                let mut vector = starter.clone();
                set_fraction(a, &mut vector, walker, i as f32 / 4.);
                let id =
                    output.measure(a, &mut *voice, &vector, &starter, "macro", None, None, None)?;
                macros.push((id, walker, vector));
            }
        }
        let probes_started = std::time::Instant::now();
        let mut probe_jobs = Vec::with_capacity(o.probes);
        for probe in 0..o.probes {
            let (parent, walker, mut vector) = macros[rng.index(macros.len())].clone();
            let mut eligible: Vec<_> = a
                .table
                .iter()
                .filter(|p| p.id != walker)
                .map(|p| p.id)
                .collect();
            for _ in 0..(1 + probe % 2) {
                if eligible.is_empty() {
                    break;
                }
                let id = eligible.swap_remove(rng.index(eligible.len()));
                set_fraction(a, &mut vector, id, rng.index(9) as f32 / 8.);
            }
            probe_jobs.push((vector, Some(parent)));
        }
        measure_batch(
            &mut output,
            a,
            &mut *voice,
            &starter,
            &probe_jobs,
            "probe",
            o.workers,
        )?;
        let probe_seconds = probes_started.elapsed().as_secs_f64();
        for y in 0..3 {
            for step in 0..o.steps {
                let mut vector = starter.clone();
                set_fraction(a, &mut vector, a.y, y as f32 / 2.);
                set_fraction(a, &mut vector, a.x, step as f32 / (o.steps - 1) as f32);
                output.measure(
                    a,
                    &mut *voice,
                    &vector,
                    &starter,
                    "walk",
                    None,
                    Some(y),
                    Some(step),
                )?;
            }
        }
        if a.name == "glass" {
            for y in 0..3 {
                for step in 0..o.steps {
                    let mut vector = starter.clone();
                    set_fraction(a, &mut vector, a.y, y as f32 / 2.);
                    set_fraction(a, &mut vector, params::glass::COARSE, 0.);
                    set_fraction(a, &mut vector, params::glass::DISORDER, 0.);
                    set_fraction(
                        a,
                        &mut vector,
                        params::glass::RATIO,
                        step as f32 / (o.steps - 1) as f32,
                    );
                    output.measure(
                        a,
                        &mut *voice,
                        &vector,
                        &starter,
                        "ratio_walk",
                        None,
                        Some(y),
                        Some(step),
                    )?;
                }
            }
        }
        let lhs_started = std::time::Instant::now();
        let lhs_jobs = lhs(a.table, o.samples, &mut rng)
            .into_iter()
            .map(|v| (v, None))
            .collect::<Vec<_>>();
        measure_batch(
            &mut output,
            a,
            &mut *voice,
            &starter,
            &lhs_jobs,
            "lhs",
            o.workers,
        )?;
        let lhs_seconds = lhs_started.elapsed().as_secs_f64();
        measure_locks(a, &starter, &mut *voice, &o.out, &mut output.analyzer)?;
        output.renders.flush()?;
        output.vectors.flush()?;
        machines.flush()?;
        let vectors = output.next - first_vector;
        writeln!(
            timings,
            "{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3}",
            a.name,
            vectors,
            vectors * NOTES.len() * VELOCITIES.len(),
            vectors as f64 * NOTES.len() as f64 * VELOCITIES.len() as f64 * FRAMES as f64
                / SR as f64
                * 2.
                + 270. * FRAMES as f64 / SR as f64,
            time.elapsed().as_secs_f64(),
            lhs_seconds,
            probe_seconds,
            time.elapsed().as_secs_f64() - lhs_seconds
        )?;
        timings.flush()?;
        eprintln!("{} complete: {:.1}s", a.name, time.elapsed().as_secs_f64());
    }
    eprintln!(
        "Measurements: {}. Analyze with python3 tools/coverage.py {}",
        o.out.display(),
        o.out.display()
    );
    Ok(())
}

fn match_callback_float_mode() {
    // Same thread-local floating-point mode as audio::flush_denormals_to_zero.
    // These kernels explicitly rely on FTZ/DAZ; measuring released filter
    // tails without it measures denormal microcode rather than callback cost.
    #[cfg(target_arch = "x86_64")]
    // SAFETY: only the calling thread's treatment of subnormal values changes.
    unsafe {
        let mut mxcsr = 0u32;
        std::arch::asm!("stmxcsr [{ptr}]", "or dword ptr [{ptr}], 0x8040", "ldmxcsr [{ptr}]",
            ptr=in(reg) &mut mxcsr, options(nostack));
    }
}
fn measure_locks(
    a: &Adapter,
    starter: &[f32],
    voice: &mut dyn CoverageVoice,
    dir: &Path,
    analyzer: &mut Analyzer,
) -> Result<(), Box<dyn Error>> {
    let path = dir.join(format!("locks-{}.csv", a.name));
    let mut file = BufWriter::new(File::create(path)?);
    writeln!(
        file,
        "machine,walker,trial,note,velocity,locked_value,hit,peak,rms,centroid,flatness,inharmonicity,x,mean_semitones"
    )?;
    for walker in [a.x, a.y, a.t] {
        for trial in 0..5 {
            for note in NOTES {
                for velocity in VELOCITIES {
                    voice.reset();
                    for (p, v) in a.table.iter().zip(starter) {
                        voice.set(p.id, *v);
                    }
                    let p = a.table[at(a.table, walker)];
                    let locked = p.min + (p.max - p.min) * trial as f32 / 4.;
                    for hit in ["base", "locked", "restored"] {
                        if hit == "locked" {
                            voice.lock(walker, Some(locked));
                        }
                        if hit == "restored" {
                            voice.lock(walker, None);
                        }
                        let latency = voice.latency();
                        let mut audio = vec![0.; FRAMES + latency];
                        voice.on(note, velocity);
                        for block in audio[..GATE].chunks_mut(512) {
                            voice.render(block);
                        }
                        voice.off(note);
                        for block in audio[GATE..].chunks_mut(512) {
                            voice.render(block);
                        }
                        let audio = &audio[latency..];
                        let spectrum = analyzer.spectrum(&audio[..SR * 3 / 10]);
                        let peak = audio.iter().fold(0f32, |peak, v| peak.max(v.abs()));
                        writeln!(
                            file,
                            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                            a.name,
                            walker,
                            trial,
                            note,
                            velocity,
                            locked,
                            hit,
                            peak,
                            spectrum.rms,
                            spectrum.centroid,
                            spectrum.flatness,
                            spectrum.inharmonicity,
                            spectrum.x,
                            spectrum.mean_semitones
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors_separate_sine_harmonic_stack_and_noise() {
        let mut a = Analyzer::new();
        let n = SR * 3 / 10;
        let sine: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 440. * i as f32 / SR as f32).sin())
            .collect();
        let s = a.spectrum(&sine);
        assert!((s.centroid - 440.).abs() < 2., "{s:?}");
        assert!(s.x < 0.02, "{s:?}");
        let stack: Vec<f32> = (0..n)
            .map(|i| {
                (1..=8)
                    .map(|k| {
                        (std::f32::consts::TAU * 220. * k as f32 * i as f32 / SR as f32).sin()
                            / k as f32
                    })
                    .sum()
            })
            .collect();
        assert!(a.spectrum(&stack).inharmonicity < 0.02);
        let mut rng = Rng(123);
        let noise: Vec<_> = (0..n).map(|_| rng.unit() * 2. - 1.).collect();
        let noise = a.spectrum(&noise);
        assert!(noise.flatness > 0.75, "{noise:?}");
        assert!(noise.x > s.x + 0.3, "{noise:?}");
        assert_eq!(a.spectrum(&vec![0.; n]).rms, 0.);
    }
    #[test]
    fn duration_uses_held_signal_and_marks_censoring() {
        let tone = |decay: f32| {
            (0..FRAMES)
                .map(|i| {
                    (std::f32::consts::TAU * 440. * i as f32 / SR as f32).sin()
                        * (-(i as f32) / SR as f32 / decay).exp()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(envelope(&tone(0.015)).class, "TRANSIENT");
        assert_eq!(envelope(&tone(0.15)).class, "DECAYING");
        assert_eq!(envelope(&tone(0.6)).class, "DECAYING");
        let long = envelope(&tone(10.));
        assert_eq!(long.class, "SUSTAINED");
        assert!(long.censored);
        assert_eq!(envelope(&vec![0.; FRAMES]).class, "SILENT");
    }
    #[test]
    #[ignore = "offline audio determinism check: about two minutes for all twelve adapters"]
    fn parallel_batches_match_serial_csv_for_every_adapter() {
        match_callback_float_mode();
        let mut failures = Vec::new();
        for a in adapters() {
            let starter = a.table.iter().map(|p| p.default).collect::<Vec<_>>();
            let jobs = lhs(a.table, 8, &mut Rng(0x71ed))
                .into_iter()
                .map(|v| (v, Some(0)))
                .collect::<Vec<_>>();
            let run = |workers| {
                let mut output = Output::with_writers(Vec::new(), Vec::new()).unwrap();
                let mut voice = (a.make)();
                output
                    .measure(
                        &a,
                        &mut *voice,
                        &starter,
                        &starter,
                        "starter",
                        None,
                        None,
                        None,
                    )
                    .unwrap();
                measure_batch(
                    &mut output,
                    &a,
                    &mut *voice,
                    &starter,
                    &jobs,
                    "probe",
                    workers,
                )
                .unwrap();
                // Exercise return to the original serial renderer after a batch.
                output
                    .measure(
                        &a,
                        &mut *voice,
                        &starter,
                        &starter,
                        "walk",
                        None,
                        Some(0),
                        Some(0),
                    )
                    .unwrap();
                (
                    output.next,
                    output.renders.into_inner().unwrap(),
                    output.vectors.into_inner().unwrap(),
                )
            };
            let serial = run(1);
            let parallel = run(4);
            assert_eq!(serial.0, parallel.0, "{} vector count", a.name);
            if serial.1 != parallel.1 {
                let path = std::env::temp_dir().join(format!("daw-coverage-equality-{}", a.name));
                std::fs::write(path.with_extension("serial.csv"), &serial.1).unwrap();
                std::fs::write(path.with_extension("parallel.csv"), &parallel.1).unwrap();
                failures.push(a.name);
                eprintln!(
                    "{} render CSV differs; saved {}.*.csv",
                    a.name,
                    path.display()
                );
            }
            assert!(
                serial.2 == parallel.2,
                "{} vector CSV differs with workers",
                a.name
            );
            if serial.1 == parallel.1 {
                eprintln!("{} serial/parallel CSV byte-identical", a.name);
            }
        }
        assert!(
            failures.is_empty(),
            "worker history dependence: {failures:?}"
        );
    }
    #[test]
    fn lhs_stratifies_each_parameter_and_cost_is_actual_distinct_turns() {
        let adapters = adapters();
        let a = &adapters[0];
        let n = 32;
        let vectors = lhs(a.table, n, &mut Rng(1));
        for (j, p) in a.table.iter().enumerate() {
            let mut strata: Vec<_> = vectors
                .iter()
                .map(|v| (((v[j] - p.min) / (p.max - p.min)) * n as f32).floor() as usize)
                .collect();
            strata.sort();
            assert_eq!(strata, (0..n).collect::<Vec<_>>());
        }
        let starter: Vec<_> = a.table.iter().map(|p| p.default).collect();
        let mut edited = starter.clone();
        edited[0] += 0.01 * (a.table[0].max - a.table[0].min);
        let (turns, cost5, _) = distance(a, &edited, &starter);
        assert_eq!((turns, cost5), (1, 0));
    }
}
