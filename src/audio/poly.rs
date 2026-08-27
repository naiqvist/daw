//! The poly synth's voice bank — the instrument half of `Node::Poly`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when. Read `notes/20260825-synth-brief.md` for the design
//! and `notes/20260825-poly-kernel-commission.md` for what the lane
//! kernels promise.
//!
//! # Shape
//!
//! [`POLY_VOICES`] voices as [`GROUPS`] groups of [`dsp::LANES`]. Every
//! kernel below is lane-major, so one group is one register's worth of
//! each parameter and the per-sample work is the same instruction over
//! eight voices. A voice is an INDEX, never a struct — `voice / LANES`
//! picks the group and `voice % LANES` the lane.
//!
//! # Two rates, on purpose
//!
//! Per SAMPLE, where modulation is a multiply: the amp envelope, the
//! oscillator levels, the pan gains. Per CONTROL CHUNK ([`CHUNK`]
//! samples), where it is a coefficient that costs a transcendental to
//! rebuild: oscillator frequency (pitch envelope, glide) and filter
//! cutoff (filter envelope, keytrack).
//!
//! That split is the honest one. Rebuilding filter coefficients every
//! sample would be a `tan` per lane per sample for no audible gain, and
//! updating the pitch only once a block — 5.3 ms at 256 frames — turns
//! the brief's kick recipe, a 48-semitone drop in 50 ms, into nine
//! audible steps. [`CHUNK`] at 32 samples is 0.67 ms: about 75 steps
//! across that same drop, which is a glide rather than a staircase.
//!
//! # The matrix
//!
//! Four wires of `(source, dest, depth)`, living as ordinary table rows
//! so automation, projects and letters address them like knobs. Phase
//! and level destinations are PER SAMPLE — each live wire is one fused
//! multiply-add pass over a lane-major accumulator, the brief's "dense
//! FMA list" — and coefficient destinations (cutoff, res, pan) ride the
//! control chunk, where coefficients already update. Cross-osc PM is
//! ordered, not graphed: a B→A wire renders B first, and if both
//! directions are asked for the A→B direction wins and the other is
//! dropped for the chunk. No feedback, by construction.
//!
//! Audio sources into coefficient destinations arrive RECTIFIED and
//! chunk-averaged — a built-in envelope follower, because the signed
//! mean of a waveform is zero and would modulate nothing.
//!
//! # Still not here, and why
//!
//! - **LFOs as matrix sources.** The synth has no LFO of its own yet;
//!   the engine's `ModPlan` LFOs are segment-rate and stay on their own
//!   plane per the brief. A `LaneLfo` kernel is the missing piece.
//! - **Per-stage envelope curves** (exp/lin): no table rows, and the
//!   `LaneAdsr` kernel is linear. Both grow together.
//! - **Presets, macros, the spectrum display**: UI-plane work in the
//!   brief's column diagram, none of it engine-blocked.
//!
//! # Red zone
//!
//! Everything except [`PolyVoices::new`] and [`PolyVoices::prepare`] runs
//! in the callback. No allocation: every buffer is sized once at compile
//! and reused. No panics: the kernels bounds-check their own lanes and
//! this file iterates rather than indexes wherever a length is not a
//! compile-time constant.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::graph::Ramp;
use crate::dsp::adsr::LaneAdsr;
use crate::dsp::filters::{LaneCascade, LaneSvf, Mode as FilterMode};
use crate::dsp::noise::{LanePinkNoise, LaneWhiteNoise};
use crate::dsp::osc::{LaneOsc, Waveform, build_tables, table_len};
use crate::dsp::shaper::{LaneOversampler2x, Mode as ShapeMode, Waveshaper};
use crate::dsp::{LANES, LaneFrame};
use crate::params::poly as p;

/// Voices the synth owns, before unison multiplies what one note costs.
pub const POLY_VOICES: usize = p::VOICES;
/// Lane groups the voices are split into.
pub const GROUPS: usize = POLY_VOICES / LANES;
/// Samples between control-rate coefficient updates. See the module doc.
pub const CHUNK: usize = 32;

/// Envelope floor: below this a voice is silent and reusable (-80 dB).
const ENV_FLOOR: f32 = 1e-4;

/// A discrete row's stored value as the index it means.
///
/// The ONE place rounding happens. Stored values are the table's verbatim
/// so a letter round-trips exactly; everything that needs an integer asks
/// here, so there is no second opinion about which way 2.5 goes.
#[inline]
fn idx(value: f32) -> u32 {
    value.round().max(0.0) as u32
}

/// Every waveform's table set, in one allocation.
///
/// All eight are built at compile because the wave is a live parameter: a
/// `ParamChange` letter can select another shape mid-note, and building a
/// table set is a million sine evaluations — the far side of the red-zone
/// line from anything the callback may do. 8 sets is about 570 KB, paid
/// once per synth node.
struct WaveTables {
    data: Vec<f32>,
    offsets: [usize; Waveform::ALL.len()],
    lens: [usize; Waveform::ALL.len()],
}

impl WaveTables {
    /// Green zone: build every shape.
    fn build() -> Self {
        let mut offsets = [0usize; Waveform::ALL.len()];
        let mut lens = [0usize; Waveform::ALL.len()];
        let mut total = 0usize;
        for (i, w) in Waveform::ALL.iter().enumerate() {
            let len = table_len(*w);
            offsets[i] = total;
            lens[i] = len;
            total += len;
        }
        let mut data = vec![0.0f32; total];
        for (i, w) in Waveform::ALL.iter().enumerate() {
            let end = offsets[i] + lens[i];
            if let Some(dst) = data.get_mut(offsets[i]..end) {
                build_tables(*w, dst);
            }
        }
        Self {
            data,
            offsets,
            lens,
        }
    }

    /// Red zone: one shape's tables. An unknown index gives the first
    /// shape rather than nothing — a stale letter should sound wrong, not
    /// silent, because silence is the harder bug to find.
    fn get(&self, index: usize) -> &[f32] {
        let i = index.min(Waveform::ALL.len() - 1);
        self.data
            .get(self.offsets[i]..self.offsets[i] + self.lens[i])
            .unwrap_or(&[])
    }
}

/// One oscillator's tuning, in the units the table speaks.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OscParams {
    /// Index into [`Waveform::ALL`], as the table stores it.
    ///
    /// An `f32` even though it means an integer: the table's discrete rows
    /// are `f32` ranges, and keeping the stored form identical to the wire
    /// form is what makes a letter round-trip EXACTLY. Rounding lives at
    /// the point of use, once, in [`idx`].
    pub wave: f32,
    /// Index into `params::poly::OCTAVES`; `octave = index - OCT_CENTER`.
    pub octave: f32,
    pub semi: f32,
    /// Cents.
    pub fine: f32,
    /// Percent.
    pub level: f32,
    /// Semitones of pitch-envelope depth.
    pub pitch_env: f32,
}

impl Default for OscParams {
    fn default() -> Self {
        PolyParams::default().osc[0]
    }
}

/// Everything a `ParamChange` letter can set, in ENGINE units — the
/// `params::poly` table's own, so a letter is stored after a clamp and
/// never converted twice.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PolyParams {
    pub osc: [OscParams; 2],
    pub noise_color: f32,
    pub noise_level: f32,
    pub noise_decay: f32,
    pub filter_mode: f32,
    pub filter_slope: f32,
    pub cutoff: f32,
    pub res: f32,
    pub filter_env: f32,
    pub keytrack: f32,
    pub drive: f32,
    pub drive_pos: f32,
    pub amp_a: f32,
    pub amp_d: f32,
    pub amp_s: f32,
    pub amp_r: f32,
    pub gain: f32,
    pub velocity: f32,
    pub voice_mode: f32,
    pub glide: f32,
    pub unison: f32,
    pub detune: f32,
    pub spread: f32,
    // The filter envelope's own ADSR and the pitch envelope's decay —
    // three envelopes, three shapes, no borrowing.
    pub fenv_a: f32,
    pub fenv_d: f32,
    pub fenv_s: f32,
    pub fenv_r: f32,
    pub penv_d: f32,
    /// The matrix: `[src, dst, depth]` per wire, exactly the table's
    /// `WIRE_IDS` rows. Stored verbatim like every other row.
    ///
    /// Tolerant on the way in: the matrix once held four wires, and a
    /// project saved then must still open — extra saved wires drop,
    /// missing ones default to off.
    #[serde(default, deserialize_with = "wires_compat")]
    pub wires: [[f32; 3]; p::WIRES],
}

/// Accept however many wires a file carries; keep the first `WIRES`.
fn wires_compat<'de, D>(de: D) -> Result<[[f32; 3]; p::WIRES], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<[f32; 3]> = serde::Deserialize::deserialize(de)?;
    let mut out = [[0.0f32; 3]; p::WIRES];
    for (slot, row) in out.iter_mut().zip(raw) {
        *slot = row;
    }
    Ok(out)
}

impl Default for PolyParams {
    fn default() -> Self {
        let d = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            osc: [
                OscParams {
                    wave: d(p::A_WAVE),
                    octave: d(p::A_OCT),
                    semi: d(p::A_SEMI),
                    fine: d(p::A_FINE),
                    level: d(p::A_LEVEL),
                    pitch_env: d(p::A_PENV),
                },
                OscParams {
                    wave: d(p::B_WAVE),
                    octave: d(p::B_OCT),
                    semi: d(p::B_SEMI),
                    fine: d(p::B_FINE),
                    level: d(p::B_LEVEL),
                    pitch_env: d(p::B_PENV),
                },
            ],
            noise_color: d(p::N_COLOR),
            noise_level: d(p::N_LEVEL),
            noise_decay: d(p::N_DECAY),
            filter_mode: d(p::F_MODE),
            filter_slope: d(p::F_SLOPE),
            cutoff: d(p::F_CUTOFF),
            res: d(p::F_RES),
            filter_env: d(p::F_ENV),
            keytrack: d(p::F_KEY),
            drive: d(p::F_DRIVE),
            drive_pos: d(p::F_POS),
            amp_a: d(p::AMP_A),
            amp_d: d(p::AMP_D),
            amp_s: d(p::AMP_S),
            amp_r: d(p::AMP_R),
            gain: d(p::GAIN),
            velocity: d(p::VEL),
            voice_mode: d(p::V_MODE),
            glide: d(p::V_GLIDE),
            unison: d(p::V_UNISON),
            detune: d(p::V_DETUNE),
            spread: d(p::V_SPREAD),
            fenv_a: d(p::FENV_A),
            fenv_d: d(p::FENV_D),
            fenv_s: d(p::FENV_S),
            fenv_r: d(p::FENV_R),
            penv_d: d(p::PENV_D),
            wires: [[0.0; 3]; p::WIRES],
        }
    }
}

impl PolyParams {
    /// Store one clamped letter. Ids are the table's; an id the table does
    /// not know never reaches here.
    pub fn set(&mut self, param: u32, value: f32) {
        let idx = || value;
        match param {
            p::A_WAVE => self.osc[0].wave = idx(),
            p::A_OCT => self.osc[0].octave = idx(),
            p::A_SEMI => self.osc[0].semi = value,
            p::A_FINE => self.osc[0].fine = value,
            p::A_LEVEL => self.osc[0].level = value,
            p::A_PENV => self.osc[0].pitch_env = value,
            p::B_WAVE => self.osc[1].wave = idx(),
            p::B_OCT => self.osc[1].octave = idx(),
            p::B_SEMI => self.osc[1].semi = value,
            p::B_FINE => self.osc[1].fine = value,
            p::B_LEVEL => self.osc[1].level = value,
            p::B_PENV => self.osc[1].pitch_env = value,
            p::N_COLOR => self.noise_color = idx(),
            p::N_LEVEL => self.noise_level = value,
            p::N_DECAY => self.noise_decay = value,
            p::F_MODE => self.filter_mode = idx(),
            p::F_SLOPE => self.filter_slope = idx(),
            p::F_CUTOFF => self.cutoff = value,
            p::F_RES => self.res = value,
            p::F_ENV => self.filter_env = value,
            p::F_KEY => self.keytrack = value,
            p::F_DRIVE => self.drive = value,
            p::F_POS => self.drive_pos = idx(),
            p::AMP_A => self.amp_a = value,
            p::AMP_D => self.amp_d = value,
            p::AMP_S => self.amp_s = value,
            p::AMP_R => self.amp_r = value,
            p::GAIN => self.gain = value,
            p::VEL => self.velocity = value,
            p::V_MODE => self.voice_mode = idx(),
            p::V_GLIDE => self.glide = value,
            p::V_UNISON => self.unison = idx(),
            p::V_DETUNE => self.detune = value,
            p::V_SPREAD => self.spread = value,
            p::FENV_A => self.fenv_a = value,
            p::FENV_D => self.fenv_d = value,
            p::FENV_S => self.fenv_s = value,
            p::FENV_R => self.fenv_r = value,
            p::PENV_D => self.penv_d = value,
            p::W1_SRC => self.wires[0][0] = value,
            p::W1_DST => self.wires[0][1] = value,
            p::W1_AMT => self.wires[0][2] = value,
            p::W2_SRC => self.wires[1][0] = value,
            p::W2_DST => self.wires[1][1] = value,
            p::W2_AMT => self.wires[1][2] = value,
            p::W3_SRC => self.wires[2][0] = value,
            p::W3_DST => self.wires[2][1] = value,
            p::W3_AMT => self.wires[2][2] = value,
            _ => {}
        }
    }

    /// Read one row back. The exact inverse of [`Self::set`] — an id the
    /// table knows always answers, and the number that comes out is the
    /// number that went in.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::A_WAVE => self.osc[0].wave,
            p::A_OCT => self.osc[0].octave,
            p::A_SEMI => self.osc[0].semi,
            p::A_FINE => self.osc[0].fine,
            p::A_LEVEL => self.osc[0].level,
            p::A_PENV => self.osc[0].pitch_env,
            p::B_WAVE => self.osc[1].wave,
            p::B_OCT => self.osc[1].octave,
            p::B_SEMI => self.osc[1].semi,
            p::B_FINE => self.osc[1].fine,
            p::B_LEVEL => self.osc[1].level,
            p::B_PENV => self.osc[1].pitch_env,
            p::N_COLOR => self.noise_color,
            p::N_LEVEL => self.noise_level,
            p::N_DECAY => self.noise_decay,
            p::F_MODE => self.filter_mode,
            p::F_SLOPE => self.filter_slope,
            p::F_CUTOFF => self.cutoff,
            p::F_RES => self.res,
            p::F_ENV => self.filter_env,
            p::F_KEY => self.keytrack,
            p::F_DRIVE => self.drive,
            p::F_POS => self.drive_pos,
            p::AMP_A => self.amp_a,
            p::AMP_D => self.amp_d,
            p::AMP_S => self.amp_s,
            p::AMP_R => self.amp_r,
            p::GAIN => self.gain,
            p::VEL => self.velocity,
            p::V_MODE => self.voice_mode,
            p::V_GLIDE => self.glide,
            p::V_UNISON => self.unison,
            p::V_DETUNE => self.detune,
            p::V_SPREAD => self.spread,
            p::FENV_A => self.fenv_a,
            p::FENV_D => self.fenv_d,
            p::FENV_S => self.fenv_s,
            p::FENV_R => self.fenv_r,
            p::PENV_D => self.penv_d,
            p::W1_SRC => self.wires[0][0],
            p::W1_DST => self.wires[0][1],
            p::W1_AMT => self.wires[0][2],
            p::W2_SRC => self.wires[1][0],
            p::W2_DST => self.wires[1][1],
            p::W2_AMT => self.wires[1][2],
            p::W3_SRC => self.wires[2][0],
            p::W3_DST => self.wires[2][1],
            p::W3_AMT => self.wires[2][2],
            _ => return None,
        })
    }

    /// How many voices one note takes.
    fn unison_voices(&self) -> usize {
        p::unison(idx(self.unison)) as usize
    }

    /// Semitone offset of an oscillator, envelope aside.
    fn transpose(&self, osc: usize) -> f32 {
        let o = &self.osc[osc.min(1)];
        p::octave(idx(o.octave)) as f32 * 12.0 + o.semi + o.fine / 100.0
    }
}

/// One group of [`LANES`] voices: every kernel, lane-major.
struct Group {
    osc: [LaneOsc; 2],
    white: LaneWhiteNoise,
    pink: LanePinkNoise,
    amp: LaneAdsr,
    filter_env: LaneAdsr,
    pitch_env: LaneAdsr,
    noise_env: LaneAdsr,
    cascade: LaneCascade,
    svf: LaneSvf,
    oversampler: LaneOversampler2x,
    /// Target frequency per lane, in Hz — where a glide is heading.
    target_hz: [f32; LANES],
    /// Current frequency per lane, in Hz — where the glide has got to.
    current_hz: [f32; LANES],
    /// Velocity as a gain, per lane.
    vel: [f32; LANES],
    /// Constant-power pan gains, per lane — where the NOTE put the
    /// voice. The matrix never writes these.
    pan_l: [f32; LANES],
    pan_r: [f32; LANES],
    /// The pans the mix actually uses: the base rotated by whatever pan
    /// wires say this chunk. Copies of the base when no wire aims here.
    pan_l_eff: [f32; LANES],
    pan_r_eff: [f32; LANES],
    /// Detune in cents, per lane — unison's spread around the note.
    detune: [f32; LANES],
}

impl Group {
    fn new() -> Self {
        Self {
            osc: [LaneOsc::new(); 2],
            white: LaneWhiteNoise::new(),
            pink: LanePinkNoise::new(),
            amp: LaneAdsr::new(),
            filter_env: LaneAdsr::new(),
            pitch_env: LaneAdsr::new(),
            noise_env: LaneAdsr::new(),
            cascade: LaneCascade::new(),
            svf: LaneSvf::new(),
            oversampler: LaneOversampler2x::new(),
            target_hz: [0.0; LANES],
            current_hz: [0.0; LANES],
            vel: [0.0; LANES],
            pan_l: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            pan_r: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            pan_l_eff: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            pan_r_eff: [core::f32::consts::FRAC_1_SQRT_2; LANES],
            detune: [0.0; LANES],
        }
    }

    fn reset_lane(&mut self, lane: usize) {
        for o in self.osc.iter_mut() {
            o.reset_lane(lane);
        }
        self.white.reset_lane(lane);
        self.pink.reset_lane(lane);
        self.amp.reset_lane(lane);
        self.filter_env.reset_lane(lane);
        self.pitch_env.reset_lane(lane);
        self.noise_env.reset_lane(lane);
        self.cascade.reset_lane(lane);
        self.svf.reset_lane(lane);
        self.oversampler.reset_lane(lane);
    }

    fn all_off(&mut self) {
        for lane in 0..LANES {
            self.reset_lane(lane);
        }
    }
}

/// Scratch the render walk needs. Sized once, at compile.
struct Scratch {
    osc_a: Vec<LaneFrame>,
    osc_b: Vec<LaneFrame>,
    noise: Vec<LaneFrame>,
    mix: Vec<LaneFrame>,
    amp: Vec<LaneFrame>,
    env: Vec<LaneFrame>,
    /// The filter envelope, per sample — it is a matrix source, and a
    /// source that only updates per chunk would stair-step every wire
    /// it feeds.
    fenv: Vec<LaneFrame>,
    /// Matrix accumulators: phase offsets (turns) and level multipliers.
    /// Each active wire is ONE fused multiply-add pass over one of these
    /// — the brief's "dense FMA list", literally.
    pm_a: Vec<LaneFrame>,
    pm_b: Vec<LaneFrame>,
    mul_a: Vec<LaneFrame>,
    mul_b: Vec<LaneFrame>,
    mul_n: Vec<LaneFrame>,
    over: Vec<LaneFrame>,
    /// The right channel, filled during the walk and read back by the
    /// node — the clock's own buffer is mono.
    right: Vec<f32>,
}

impl Scratch {
    fn new(block: usize) -> Self {
        Self {
            osc_a: vec![[0.0; LANES]; block],
            osc_b: vec![[0.0; LANES]; block],
            noise: vec![[0.0; LANES]; block],
            mix: vec![[0.0; LANES]; block],
            amp: vec![[0.0; LANES]; block],
            env: vec![[0.0; LANES]; block],
            fenv: vec![[0.0; LANES]; block],
            pm_a: vec![[0.0; LANES]; block],
            pm_b: vec![[0.0; LANES]; block],
            mul_a: vec![[0.0; LANES]; block],
            mul_b: vec![[0.0; LANES]; block],
            mul_n: vec![[0.0; LANES]; block],
            over: vec![[0.0; LANES]; block * 2],
            right: vec![0.0; block],
        }
    }
}

/// One decoded matrix wire.
#[derive(Debug, Clone, Copy)]
struct Wire {
    src: u32,
    dst: u32,
    /// Normalized: 1.0 at the row's +100 %.
    depth: f32,
}

impl Wire {
    /// Whether this wire does anything at all.
    fn live(self) -> bool {
        self.src != p::SRC_OFF && self.dst != p::DST_OFF && self.depth != 0.0
    }
}

/// The params' wire rows, decoded once per chunk.
fn wires_of(params: &PolyParams) -> [Wire; p::WIRES] {
    let mut out = [Wire {
        src: 0,
        dst: 0,
        depth: 0.0,
    }; p::WIRES];
    for (row, wire) in params.wires.iter().zip(out.iter_mut()) {
        wire.src = idx(row[0]);
        wire.dst = idx(row[1]);
        wire.depth = row[2] / 100.0;
    }
    out
}

/// A wire source as the FMA pass reads it: a per-sample buffer, or one
/// value per lane held for the chunk.
enum SrcView<'a> {
    Buf(&'a [LaneFrame]),
    Lanes([f32; LANES]),
}

/// One wire, applied: `dst += depth * src`, lane-major, over the chunk.
/// THE matrix primitive — every live wire is exactly one of these.
fn wire_pass(dst: &mut [LaneFrame], src: &SrcView<'_>, depth: f32) {
    match src {
        SrcView::Buf(buf) => {
            for (d, s) in dst.iter_mut().zip(buf.iter()) {
                for (dv, sv) in d.iter_mut().zip(s.iter()) {
                    *dv += depth * *sv;
                }
            }
        }
        SrcView::Lanes(lanes) => {
            for d in dst.iter_mut() {
                for (dv, sv) in d.iter_mut().zip(lanes.iter()) {
                    *dv += depth * *sv;
                }
            }
        }
    }
}

/// Build one oscillator's phase accumulator from every live wire aiming
/// at it. Returns whether anything landed, so an unmodulated oscillator
/// keeps its `None` fast path.
///
/// `other` is the OTHER oscillator's output, present only when it has
/// already rendered this chunk — which is how the no-feedback rule is
/// enforced: a wire whose source does not exist yet is simply not built.
#[allow(clippy::too_many_arguments)]
fn build_pm(
    pm: &mut [LaneFrame],
    wires: &[Wire],
    phase_dst: u32,
    other: Option<(&[LaneFrame], u32)>,
    noise: &[LaneFrame],
    amp: &[LaneFrame],
    fenv: &[LaneFrame],
    penv: [f32; LANES],
    vel: [f32; LANES],
) -> bool {
    for f in pm.iter_mut() {
        *f = [0.0; LANES];
    }
    let n = pm.len();
    let mut modulated = false;
    for w in wires {
        if !w.live() || w.dst != phase_dst {
            continue;
        }
        let src = match w.src {
            p::SRC_NOISE => SrcView::Buf(noise.get(..n).unwrap_or(noise)),
            p::SRC_AMP => SrcView::Buf(amp.get(..n).unwrap_or(amp)),
            p::SRC_FENV => SrcView::Buf(fenv.get(..n).unwrap_or(fenv)),
            p::SRC_PENV => SrcView::Lanes(penv),
            p::SRC_VEL => SrcView::Lanes(vel),
            src => match other {
                Some((buf, avail)) if src == avail => SrcView::Buf(buf.get(..n).unwrap_or(buf)),
                _ => continue,
            },
        };
        wire_pass(pm, &src, w.depth);
        modulated = true;
    }
    modulated
}

/// The poly synth's voices.
pub struct PolyVoices {
    groups: [Group; GROUPS],
    tables: WaveTables,
    scratch: Scratch,
    shaper: Waveshaper,
    params: PolyParams,
    /// The KNOBS: what letters set, and what a parameter-lock restore
    /// returns to. `params` above is the LIVE view — equal to this except
    /// where the current note's locks have overridden it.
    base: PolyParams,
    sample_rate: f32,
    /// Allocation, flat across every group. A voice is an index.
    gate: [bool; POLY_VOICES],
    pitch: [u8; POLY_VOICES],
    age: [u64; POLY_VOICES],
    /// The note each voice belongs to, so one note-off releases the whole
    /// unison stack it started.
    note_id: [u64; POLY_VOICES],
    next_note: u64,
    /// The last pitch played, for legato glide.
    last_hz: f32,
}

impl PolyVoices {
    /// Green zone: build the tables and the scratch. `block` is the
    /// longest segment the callback will ever hand `render`.
    pub fn new(sample_rate: f32, block: usize, params: PolyParams) -> Self {
        let mut v = Self {
            groups: [(); GROUPS].map(|()| Group::new()),
            tables: WaveTables::build(),
            scratch: Scratch::new(block.max(1)),
            shaper: Waveshaper::new(),
            params,
            base: params,
            sample_rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            gate: [false; POLY_VOICES],
            pitch: [0; POLY_VOICES],
            age: [0; POLY_VOICES],
            note_id: [0; POLY_VOICES],
            next_note: 0,
            last_hz: 0.0,
        };
        v.prepare();
        v
    }

    /// Green zone: push the parameters into every kernel that caches them.
    /// Called at compile and whenever a letter changes something the
    /// kernels only read at prepare time.
    pub fn prepare(&mut self) {
        let fs = self.sample_rate;
        let q = self.params.res;
        let (a, d, s, r) = (
            self.params.amp_a,
            self.params.amp_d,
            self.params.amp_s / 100.0,
            self.params.amp_r,
        );
        for (g, group) in self.groups.iter_mut().enumerate() {
            for (i, osc) in group.osc.iter_mut().enumerate() {
                osc.prepare(
                    fs,
                    Waveform::from_index(idx(self.params.osc[i].wave) as usize),
                );
            }
            // Every group gets its own noise streams, or the two halves of
            // the polyphony would play the same noise in lockstep.
            group.white.seed(0x51F0 + g as u64);
            group.pink.seed(0xA37B + g as u64);
            group.amp.prepare(fs, a, d, s, r);
            // Three envelopes, three shapes: the filter's contour has its
            // OWN ADSR rows, and the pitch envelope is a one-shot decay
            // with an instant attack — the kick's clock, per the brief.
            group.filter_env.prepare(
                fs,
                self.params.fenv_a,
                self.params.fenv_d,
                self.params.fenv_s / 100.0,
                self.params.fenv_r,
            );
            group
                .pitch_env
                .prepare(fs, 0.0, self.params.penv_d, 0.0, 1.0);
            group
                .noise_env
                .prepare(fs, 0.0, self.params.noise_decay, 0.0, 1.0);
            group.cascade.prepare(
                fs,
                self.params.cutoff,
                q,
                crate::params::filter::slope_order(idx(self.params.filter_slope)),
                idx(self.params.filter_mode) == crate::params::filter::MODE_HP,
            );
            group.svf.prepare(fs, self.params.cutoff, q);
        }
        self.shaper.configure(
            ShapeMode::SoftClip,
            crate::params::filter::shaper_drive(self.params.drive / 100.0),
            0.0,
            1.0,
        );
    }

    /// Red zone: adopt a clamped letter and refresh whatever caches it.
    /// A LETTER is the knob: it moves the restore base as well as the
    /// live value, so a plocked pattern follows the player's hand on
    /// every unlocked note.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_live(param, value);
    }

    /// Red zone: a parameter LOCK at a note boundary — override the live
    /// value, or restore the knob. Never touches `base`.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let value = match value {
            Some(v) => v,
            None => match self.base.get(param) {
                Some(v) => v,
                None => return,
            },
        };
        self.apply_live(param, value);
    }

    /// The live half of a parameter write: store and refresh caches.
    fn apply_live(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        // Only the parameters kernels CACHE need a refresh; the rest are
        // read where they are used. Doing this per letter rather than per
        // block keeps the per-sample path free of parameter logic.
        match param {
            p::A_WAVE | p::B_WAVE => {
                let fs = self.sample_rate;
                let waves = [idx(self.params.osc[0].wave), idx(self.params.osc[1].wave)];
                for group in self.groups.iter_mut() {
                    for (osc, w) in group.osc.iter_mut().zip(waves.iter()) {
                        osc.prepare(fs, Waveform::from_index(*w as usize));
                    }
                }
            }
            p::AMP_A | p::AMP_D | p::AMP_S | p::AMP_R => {
                let fs = self.sample_rate;
                let (a, d, s, r) = (
                    self.params.amp_a,
                    self.params.amp_d,
                    self.params.amp_s / 100.0,
                    self.params.amp_r,
                );
                for group in self.groups.iter_mut() {
                    group.amp.prepare(fs, a, d, s, r);
                }
            }
            p::FENV_A | p::FENV_D | p::FENV_S | p::FENV_R => {
                let fs = self.sample_rate;
                let (a, d, sus, r) = (
                    self.params.fenv_a,
                    self.params.fenv_d,
                    self.params.fenv_s / 100.0,
                    self.params.fenv_r,
                );
                for group in self.groups.iter_mut() {
                    group.filter_env.prepare(fs, a, d, sus, r);
                }
            }
            p::PENV_D => {
                let fs = self.sample_rate;
                let d = self.params.penv_d;
                for group in self.groups.iter_mut() {
                    group.pitch_env.prepare(fs, 0.0, d, 0.0, 1.0);
                }
            }
            p::N_DECAY => {
                let fs = self.sample_rate;
                let d = self.params.noise_decay;
                for group in self.groups.iter_mut() {
                    group.noise_env.prepare(fs, 0.0, d, 0.0, 1.0);
                }
            }
            p::F_DRIVE => {
                self.shaper.configure(
                    ShapeMode::SoftClip,
                    crate::params::filter::shaper_drive(self.params.drive / 100.0),
                    0.0,
                    1.0,
                );
            }
            _ => {}
        }
    }

    pub fn params(&self) -> &PolyParams {
        &self.params
    }

    /// The right channel of the segment just rendered.
    pub fn right(&self, len: usize) -> &[f32] {
        self.scratch.right.get(..len).unwrap_or(&[])
    }

    /// Concert pitch of a MIDI note.
    fn note_hz(pitch: u8) -> f32 {
        440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2()
    }

    /// Red zone. Silence everything, now — what a discontinuity demands.
    pub fn all_sound_off(&mut self) {
        for group in self.groups.iter_mut() {
            group.all_off();
        }
        self.gate = [false; POLY_VOICES];
        self.age = [0; POLY_VOICES];
        self.note_id = [0; POLY_VOICES];
    }

    /// Red zone. Release every gate without cutting the tails.
    pub fn release_all(&mut self) {
        for voice in 0..POLY_VOICES {
            if self.gate[voice] {
                self.gate[voice] = false;
                self.gate_off_voice(voice);
            }
        }
    }

    fn gate_off_voice(&mut self, voice: usize) {
        let (g, lane) = (voice / LANES, voice % LANES);
        if let Some(group) = self.groups.get_mut(g) {
            group.amp.gate_off(lane);
            group.filter_env.gate_off(lane);
        }
    }

    /// True while a voice is still making sound.
    fn sounding(&self, voice: usize) -> bool {
        let (g, lane) = (voice / LANES, voice % LANES);
        self.groups
            .get(g)
            .is_some_and(|group| group.amp.active(lane) && group.amp.current(lane) >= ENV_FLOOR)
    }

    /// Red zone. Release every voice of the OLDEST note gated on `pitch`.
    ///
    /// A note, not a voice: unison means one note-on took several, and one
    /// note-off has to end all of them or the stack hangs.
    pub fn note_off(&mut self, pitch: u8) {
        let mut target = None;
        for voice in 0..POLY_VOICES {
            if self.gate[voice] && self.pitch[voice] == pitch {
                let key = (self.age[voice], self.note_id[voice]);
                if target.is_none_or(|(a, _)| key.0 < a) {
                    target = Some(key);
                }
            }
        }
        let Some((_, note)) = target else {
            return;
        };
        for voice in 0..POLY_VOICES {
            if self.gate[voice] && self.note_id[voice] == note {
                self.gate[voice] = false;
                self.gate_off_voice(voice);
            }
        }
    }

    /// Red zone. Start `pitch`, taking [`PolyParams::unison_voices`] voices.
    pub fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        let note = self.next_note;
        self.next_note = self.next_note.wrapping_add(1);
        let count = self.unison_count();
        let base = Self::note_hz(pitch);
        // Mono and legato glide from wherever the last note was; poly
        // always starts on pitch. `last_hz` is 0 before the first note,
        // which would glide up from DC — start there instead.
        let mono = idx(self.params.voice_mode) != 0;
        let from = if mono && self.last_hz > 0.0 {
            self.last_hz
        } else {
            base
        };
        self.last_hz = base;
        // Legato: a new note while one is held slides without retriggering
        // the envelopes, which is what makes it legato rather than mono.
        let legato = idx(self.params.voice_mode) == 2 && (0..POLY_VOICES).any(|v| self.gate[v]);
        if mono {
            // One note at a time: everything sounding is released first.
            self.release_all();
        }

        for u in 0..count {
            let voice = self.steal();
            let (g, lane) = (voice / LANES, voice % LANES);
            let (detune, pan) = self.unison_place(u, count);
            self.gate[voice] = true;
            self.pitch[voice] = pitch;
            self.age[voice] = age;
            self.note_id[voice] = note;
            let Some(group) = self.groups.get_mut(g) else {
                continue;
            };
            group.detune[lane] = detune;
            group.target_hz[lane] = base;
            group.current_hz[lane] = from;
            group.vel[lane] = f32::from(vel) / 127.0;
            let (l, r) = pan;
            group.pan_l[lane] = l;
            group.pan_r[lane] = r;
            group.pan_l_eff[lane] = l;
            group.pan_r_eff[lane] = r;
            if !legato {
                // Unison voices START SPREAD around the cycle. Eight
                // voices from phase 0 are one voice at eight times the
                // level until the detune pulls them apart; spread, the
                // stack is wide immediately and the attack does not spike.
                let offset = if count > 1 {
                    u as f32 / count as f32
                } else {
                    0.0
                };
                for o in group.osc.iter_mut() {
                    o.reset_lane(lane);
                    o.set_phase(lane, offset);
                }
                group.amp.gate_on(lane);
                group.filter_env.gate_on(lane);
                group.pitch_env.gate_on(lane);
                group.noise_env.gate_on(lane);
            }
        }
    }

    fn unison_count(&self) -> usize {
        self.params.unison_voices().clamp(1, POLY_VOICES)
    }

    /// Where unison voice `u` of `count` sits: detune in cents, and a
    /// constant-power pan pair.
    ///
    /// Symmetric about the note, so a unison stack does not drift the
    /// perceived pitch, and CONSTANT POWER so widening the spread does not
    /// also make it louder.
    fn unison_place(&self, u: usize, count: usize) -> (f32, (f32, f32)) {
        if count <= 1 {
            let c = core::f32::consts::FRAC_1_SQRT_2;
            return (0.0, (c, c));
        }
        // -1..1 across the stack.
        let t = (u as f32 / (count - 1) as f32) * 2.0 - 1.0;
        let detune = t * self.params.detune * 0.5;
        let spread = (self.params.spread / 100.0).clamp(0.0, 1.0);
        let angle = (t * spread + 1.0) * 0.5 * core::f32::consts::FRAC_PI_2;
        (detune, (angle.cos(), angle.sin()))
    }

    /// A free voice, or the best one to steal.
    fn steal(&mut self) -> usize {
        if let Some(v) = (0..POLY_VOICES).find(|v| !self.gate[*v] && !self.sounding(*v)) {
            return v;
        }
        // Everything is sounding: a releasing voice before a held one, and
        // the oldest before the newest. `(gate, age)` orders exactly that.
        let mut best = 0usize;
        let mut key = (true, u64::MAX);
        for v in 0..POLY_VOICES {
            if (self.gate[v], self.age[v]) < key {
                key = (self.gate[v], self.age[v]);
                best = v;
            }
        }
        let (g, lane) = (best / LANES, best % LANES);
        if let Some(group) = self.groups.get_mut(g) {
            group.reset_lane(lane);
        }
        best
    }
    /// Red zone. Render `out.len()` samples of the left channel into
    /// `out`, and the right into the bank's own buffer at `at`.
    ///
    /// Structure: clear, sum every group in, then apply the gain ramp once
    /// per sample to both channels. The ramp advancing exactly once per
    /// sample on every path — silent groups included — is what lets the
    /// caller land it on its target exactly.
    pub fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        let len = out.len();
        for s in out.iter_mut() {
            *s = 0.0;
        }
        if let Some(r) = self.scratch.right.get_mut(at..at + len) {
            for s in r.iter_mut() {
                *s = 0.0;
            }
        }
        for g in 0..GROUPS {
            self.render_group(g, out, at);
        }
        for (i, s) in out.iter_mut().enumerate() {
            let g = gain.next();
            *s *= g;
            if let Some(r) = self.scratch.right.get_mut(at + i) {
                *r *= g;
            }
        }
    }

    /// One group, in control chunks. Accumulates; never clears.
    fn render_group(&mut self, g: usize, out: &mut [f32], at: usize) {
        // A group with nothing sounding costs one check, not a block of
        // arithmetic. The gain ramp is applied by the caller, so skipping
        // here cannot desynchronise it.
        if !self
            .groups
            .get(g)
            .is_some_and(|group| group.amp.any_active())
        {
            return;
        }
        let params = self.params;
        let fs = self.sample_rate;
        let mut done = 0usize;
        while done < out.len() {
            let n = CHUNK.min(out.len() - done);
            self.control_update(g, params, fs);
            self.render_chunk(g, n);
            // Sum the chunk into the output, per lane, with pan.
            let Some(group) = self.groups.get(g) else {
                return;
            };
            let (pan_l, pan_r) = (group.pan_l_eff, group.pan_r_eff);
            let vel_amount = (params.velocity / 100.0).clamp(0.0, 1.0);
            for (i, (mix, env)) in self
                .scratch
                .mix
                .iter()
                .zip(self.scratch.amp.iter())
                .take(n)
                .enumerate()
            {
                let mut acc_l = 0.0f32;
                let mut acc_r = 0.0f32;
                for ((((s, e), v), l), r) in mix
                    .iter()
                    .zip(env.iter())
                    .zip(group.vel.iter())
                    .zip(pan_l.iter())
                    .zip(pan_r.iter())
                {
                    // Velocity scales level by the patch's own amount:
                    // at 0 % a soft note is as loud as a hard one.
                    let vel = 1.0 - vel_amount + vel_amount * *v;
                    let x = *s * *e * vel;
                    acc_l += x * *l;
                    acc_r += x * *r;
                }
                // Headroom across the polyphony, matching the Seq synth's
                // rule: the sum of many voices must not clip before the
                // gain knob gets a say.
                let scale = 0.25;
                if let Some(o) = out.get_mut(done + i) {
                    *o += acc_l * scale;
                }
                if let Some(r) = self.scratch.right.get_mut(at + done + i) {
                    *r += acc_r * scale;
                }
            }
            done += n;
        }
    }

    /// Control rate: everything that costs a transcendental to rebuild.
    fn control_update(&mut self, g: usize, params: PolyParams, fs: f32) {
        let transpose = [params.transpose(0), params.transpose(1)];
        let penv = [params.osc[0].pitch_env, params.osc[1].pitch_env];
        // Glide: one pole per control chunk toward the target. A zero or
        // one-sample time is instant, which is what "no glide" means.
        let glide_samples = (params.glide * 1e-3 * fs).max(1.0);
        let glide = 1.0 - (-(CHUNK as f32) / glide_samples).exp();
        let keytrack = params.keytrack / 100.0;
        let fenv_amount = params.filter_env / 100.0;
        let base_cutoff = params.cutoff;

        let Some(group) = self.groups.get_mut(g) else {
            return;
        };

        // The chunk-rate half of the matrix: wires aimed at cutoff, res
        // or pan. These move COEFFICIENTS, so they update where the
        // coefficients do; a control source (an envelope, velocity) is
        // its current value, and an audio source arrives RECTIFIED and
        // chunk-averaged — a built-in envelope follower, because the
        // signed mean of a waveform is zero and would modulate nothing.
        let wires = wires_of(&params);
        let mut wire_cut = [0.0f32; LANES];
        let mut wire_pan = [0.0f32; LANES];
        let mut wire_res = 0.0f32;
        let mut pan_wired = false;
        for w in &wires {
            if !w.live() || !matches!(w.dst, p::DST_CUTOFF | p::DST_RES | p::DST_PAN) {
                continue;
            }
            let follow = |buf: &[LaneFrame]| {
                let mut lanes = [0.0f32; LANES];
                let frames = buf.get(..CHUNK.min(buf.len())).unwrap_or(buf);
                for f in frames {
                    for (v, x) in lanes.iter_mut().zip(f.iter()) {
                        *v += x.abs();
                    }
                }
                let inv = 1.0 / frames.len().max(1) as f32;
                for v in lanes.iter_mut() {
                    *v *= inv;
                }
                lanes
            };
            let mut sv = [0.0f32; LANES];
            match w.src {
                p::SRC_OSC_A => sv = follow(&self.scratch.osc_a),
                p::SRC_OSC_B => sv = follow(&self.scratch.osc_b),
                p::SRC_NOISE => sv = follow(&self.scratch.noise),
                p::SRC_AMP => {
                    for (lane, v) in sv.iter_mut().enumerate() {
                        *v = group.amp.current(lane);
                    }
                }
                p::SRC_FENV => {
                    for (lane, v) in sv.iter_mut().enumerate() {
                        *v = group.filter_env.current(lane);
                    }
                }
                p::SRC_PENV => {
                    for (lane, v) in sv.iter_mut().enumerate() {
                        *v = group.pitch_env.current(lane);
                    }
                }
                p::SRC_VEL => sv = group.vel,
                _ => continue,
            }
            match w.dst {
                // Four octaves at full depth — the dedicated F_ENV
                // knob's own scale, so the two routes agree.
                p::DST_CUTOFF => {
                    for (c, v) in wire_cut.iter_mut().zip(sv.iter()) {
                        *c += w.depth * *v * 4.0;
                    }
                }
                // Resonance is one number per filter, not per lane, so
                // the wire's lanes average. Two octaves of Q at full
                // depth.
                p::DST_RES => {
                    let mean = sv.iter().sum::<f32>() / LANES as f32;
                    wire_res += w.depth * mean * 2.0;
                }
                p::DST_PAN => {
                    pan_wired = true;
                    for (c, v) in wire_pan.iter_mut().zip(sv.iter()) {
                        *c += w.depth * *v;
                    }
                }
                _ => {}
            }
        }
        // Effective pans: the note's own placement rotated by the wires,
        // still constant-power. Copied even when nothing is wired, so a
        // deleted wire does not leave a stale rotation behind.
        for lane in 0..LANES {
            let (Some(bl), Some(br), Some(el), Some(er)) = (
                group.pan_l.get(lane),
                group.pan_r.get(lane),
                group.pan_l_eff.get_mut(lane),
                group.pan_r_eff.get_mut(lane),
            ) else {
                continue;
            };
            if pan_wired {
                let angle = (br.atan2(*bl)
                    + wire_pan.get(lane).copied().unwrap_or(0.0) * core::f32::consts::FRAC_PI_2)
                    .clamp(0.0, core::f32::consts::FRAC_PI_2);
                *el = angle.cos();
                *er = angle.sin();
            } else {
                *el = *bl;
                *er = *br;
            }
        }

        let mut cutoffs = [0.0f32; LANES];
        for (lane, cutoff) in cutoffs.iter_mut().enumerate() {
            let target = group.target_hz.get(lane).copied().unwrap_or(0.0);
            let Some(cur) = group.current_hz.get_mut(lane) else {
                continue;
            };
            *cur += (target - *cur) * glide;
            let base = *cur;
            let detune = group.detune.get(lane).copied().unwrap_or(0.0);
            let pitch_env = group.pitch_env.current(lane);
            for (i, osc) in group.osc.iter_mut().enumerate() {
                let semis = transpose[i] + detune / 100.0 + pitch_env * penv[i];
                osc.set_freq(lane, base * (semis / 12.0).exp2());
            }
            // Keytrack is relative to middle C, so a patch tuned there
            // stays put and everything else follows the keyboard.
            let key_semis = (base / 261.626).max(1e-6).log2() * 12.0;
            let sweep = fenv_amount * group.filter_env.current(lane) * 4.0
                + wire_cut.get(lane).copied().unwrap_or(0.0);
            let hz = base_cutoff * ((keytrack * key_semis / 12.0) + sweep).exp2();
            *cutoff = hz.clamp(20.0, 20_000.0);
        }
        // Res wires multiply the patch's Q, clamped back into the row's
        // own range — the same clamp a letter gets.
        let q = crate::params::def(p::TABLE, p::F_RES).clamp(params.res * wire_res.exp2());
        match idx(params.filter_mode) {
            crate::params::filter::MODE_BP | crate::params::filter::MODE_NOTCH => {
                group.svf.prepare_lanes(fs, &cutoffs, q);
            }
            mode => {
                group.cascade.prepare_lanes(
                    fs,
                    &cutoffs,
                    q,
                    crate::params::filter::slope_order(idx(params.filter_slope)),
                    mode == crate::params::filter::MODE_HP,
                );
            }
        }
    }

    /// Audio rate: one control chunk of the voice path.
    ///
    /// # The matrix's order of operations
    ///
    /// Sources that depend on nothing render first (noise, the
    /// envelopes); then each oscillator's phase-mod accumulator is built
    /// from every live wire aiming at it, and the oscillator renders.
    /// Cross-osc PM decides the render order: a B->A phase wire renders B
    /// first, an A->B wire renders A first, and if BOTH exist the A->B
    /// direction wins and the B->A wires are dropped for the chunk — the
    /// no-feedback rule, enforced by ORDER rather than by a graph check,
    /// because with two oscillators order IS the whole graph.
    ///
    /// Level wires have no ordering problem: they multiply into the mix
    /// after everything has rendered.
    fn render_chunk(&mut self, g: usize, n: usize) {
        let params = self.params;
        let wires = wires_of(&params);
        let tables_a = self.tables.get(idx(params.osc[0].wave) as usize);
        let tables_b = self.tables.get(idx(params.osc[1].wave) as usize);
        let (Some(group), scratch) = (self.groups.get_mut(g), &mut self.scratch) else {
            return;
        };

        // Independent sources first.
        if let Some(buf) = scratch.noise.get_mut(..n) {
            if idx(params.noise_color) == 0 {
                group.white.process(buf);
            } else {
                group.pink.process(buf);
            }
        }
        // Envelopes. All four advance every sample, whether or not this
        // chunk reads them: an envelope that only moves when something
        // listens is an envelope whose shape depends on the patch.
        if let Some(buf) = scratch.amp.get_mut(..n) {
            group.amp.process(buf);
        }
        if let Some(buf) = scratch.fenv.get_mut(..n) {
            group.filter_env.process(buf);
        }
        if let Some(buf) = scratch.env.get_mut(..n) {
            group.pitch_env.process(buf);
            // The noise envelope goes LAST into the shared buffer: it is
            // the one read back per sample below.
            group.noise_env.process(buf);
        }

        {
            // Split borrows: the FMA passes read one buffer while adding
            // into another, which `scratch.x` field access cannot express.
            let Scratch {
                osc_a,
                osc_b,
                noise,
                amp,
                fenv,
                pm_a,
                pm_b,
                ..
            } = &mut *scratch;
            let mut penv = [0.0f32; LANES];
            for (lane, v) in penv.iter_mut().enumerate() {
                *v = group.pitch_env.current(lane);
            }
            let vel = group.vel;

            // Render order: A first unless a B->A phase wire exists with
            // no A->B one. On a tie the A->B direction wins and the B->A
            // wires are dropped — no feedback, decided by order.
            let feeds = |from: u32, to_phase: u32| {
                wires
                    .iter()
                    .any(|w| w.live() && w.src == from && w.dst == to_phase)
            };
            let b_first =
                feeds(p::SRC_OSC_B, p::DST_A_PHASE) && !feeds(p::SRC_OSC_A, p::DST_B_PHASE);

            let (Some(pm_a), Some(pm_b), Some(buf_a), Some(buf_b)) = (
                pm_a.get_mut(..n),
                pm_b.get_mut(..n),
                osc_a.get_mut(..n),
                osc_b.get_mut(..n),
            ) else {
                return;
            };
            if b_first {
                let m = build_pm(
                    pm_b,
                    &wires,
                    p::DST_B_PHASE,
                    None,
                    noise,
                    amp,
                    fenv,
                    penv,
                    vel,
                );
                group.osc[1].process(buf_b, m.then_some(pm_b), tables_b);
                let m = build_pm(
                    pm_a,
                    &wires,
                    p::DST_A_PHASE,
                    Some((buf_b, p::SRC_OSC_B)),
                    noise,
                    amp,
                    fenv,
                    penv,
                    vel,
                );
                group.osc[0].process(buf_a, m.then_some(pm_a), tables_a);
            } else {
                let m = build_pm(
                    pm_a,
                    &wires,
                    p::DST_A_PHASE,
                    None,
                    noise,
                    amp,
                    fenv,
                    penv,
                    vel,
                );
                group.osc[0].process(buf_a, m.then_some(pm_a), tables_a);
                let m = build_pm(
                    pm_b,
                    &wires,
                    p::DST_B_PHASE,
                    Some((buf_a, p::SRC_OSC_A)),
                    noise,
                    amp,
                    fenv,
                    penv,
                    vel,
                );
                group.osc[1].process(buf_b, m.then_some(pm_b), tables_b);
            }
        }

        {
            // Level wires: every source exists now, so no ordering.
            let Scratch {
                osc_a,
                osc_b,
                noise,
                amp,
                fenv,
                mul_a,
                mul_b,
                mul_n,
                ..
            } = &mut *scratch;
            let view = |src: u32| -> Option<SrcView<'_>> {
                Some(match src {
                    p::SRC_OSC_A => SrcView::Buf(osc_a),
                    p::SRC_OSC_B => SrcView::Buf(osc_b),
                    p::SRC_NOISE => SrcView::Buf(noise),
                    p::SRC_AMP => SrcView::Buf(amp),
                    p::SRC_FENV => SrcView::Buf(fenv),
                    p::SRC_PENV => {
                        let mut lanes = [0.0f32; LANES];
                        for (lane, v) in lanes.iter_mut().enumerate() {
                            *v = group.pitch_env.current(lane);
                        }
                        SrcView::Lanes(lanes)
                    }
                    p::SRC_VEL => SrcView::Lanes(group.vel),
                    _ => return None,
                })
            };
            for (dst, buf) in [
                (p::DST_A_LEVEL, &mut *mul_a),
                (p::DST_B_LEVEL, &mut *mul_b),
                (p::DST_N_LEVEL, &mut *mul_n),
            ] {
                let Some(buf) = buf.get_mut(..n) else {
                    continue;
                };
                // Unity when nothing modulates: the multiplier is
                // 1 + the wires' sum, so depth 0 is a wire that is not
                // there, and a bipolar audio source swings THROUGH zero —
                // which is ring modulation, on purpose.
                for f in buf.iter_mut() {
                    *f = [1.0; LANES];
                }
                for w in &wires {
                    if w.live()
                        && w.dst == dst
                        && let Some(src) = view(w.src)
                    {
                        let src = match src {
                            SrcView::Buf(b) => SrcView::Buf(b.get(..n).unwrap_or(b)),
                            lanes => lanes,
                        };
                        wire_pass(buf, &src, w.depth);
                    }
                }
            }
        }

        // The mix: levels, their modulators, and the noise envelope.
        let (la, lb, ln) = (
            params.osc[0].level / 100.0,
            params.osc[1].level / 100.0,
            params.noise_level / 100.0,
        );
        for i in 0..n {
            let (Some(mix), Some(a), Some(b), Some(no), Some(ne)) = (
                scratch.mix.get_mut(i),
                scratch.osc_a.get(i),
                scratch.osc_b.get(i),
                scratch.noise.get(i),
                scratch.env.get(i),
            ) else {
                continue;
            };
            let (Some(ma), Some(mb), Some(mn)) = (
                scratch.mul_a.get(i),
                scratch.mul_b.get(i),
                scratch.mul_n.get(i),
            ) else {
                continue;
            };
            for lane in 0..LANES {
                let (Some(m), Some(av), Some(bv), Some(nv), Some(nev)) = (
                    mix.get_mut(lane),
                    a.get(lane),
                    b.get(lane),
                    no.get(lane),
                    ne.get(lane),
                ) else {
                    continue;
                };
                let (Some(mav), Some(mbv), Some(mnv)) = (ma.get(lane), mb.get(lane), mn.get(lane))
                else {
                    continue;
                };
                *m = *av * la * *mav + *bv * lb * *mbv + *nv * ln * *nev * *mnv;
            }
        }

        let drive_on = params.drive > 0.0;
        let pre = idx(params.drive_pos) == 0;
        if drive_on && pre {
            Self::drive_chunk(group, &self.shaper, scratch, n);
        }
        if let Some(buf) = scratch.mix.get_mut(..n) {
            match idx(params.filter_mode) {
                crate::params::filter::MODE_BP => group.svf.process(buf, FilterMode::Bandpass),
                crate::params::filter::MODE_NOTCH => group.svf.process(buf, FilterMode::Notch),
                _ => group.cascade.process(buf),
            }
        }
        if drive_on && !pre {
            Self::drive_chunk(group, &self.shaper, scratch, n);
        }
    }

    /// The per-voice drive stage: 2x oversampled, so the harmonics it
    /// makes above Nyquist fold back out of the band rather than into it.
    fn drive_chunk(group: &mut Group, shaper: &Waveshaper, scratch: &mut Scratch, n: usize) {
        let (Some(io), Some(over)) = (scratch.mix.get_mut(..n), scratch.over.get_mut(..n * 2))
        else {
            return;
        };
        group.oversampler.up(io, over);
        shaper.process_lanes(over);
        group.oversampler.down(over, io);
    }
}
