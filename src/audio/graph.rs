//! The schedule: a compiled, flat execution plan for the audio graph.
//!
//! Green zone compiles `GraphSpec` (nodes + wires) into a `Schedule`; the red
//! zone only walks it. Compile does the thinking — topological order, cycle
//! rejection, slot assignment — so the callback can be a straight-line walk
//! over flat arrays: one `Vec<Step>` in dependency order, one arena of
//! 64-byte-aligned slots indexed by number.

use crate::audio::modulation::{ModEdit, ModPlan, ModSpec};
use creek::{ReadDiskStream, SeekMode, SymphoniaDecoder};
use std::collections::HashMap;
use std::f32::consts::TAU;

/// A node's permanent name tag: a `thunderdome` generational index. The slot
/// may be reused after a removal, but the generation changes — so a stale id
/// can never address the wrong node, only nothing.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct NodeId(thunderdome::Index);

impl NodeId {
    /// Packed form for the wire. Layout is guaranteed by thunderdome:
    /// generation (never 0) in the high 32 bits, slot in the low 32.
    pub fn to_bits(self) -> u64 {
        self.0.to_bits()
    }

    /// The inverse of [`Self::to_bits`]. `None` for a packed form thunderdome
    /// would never mint — generation 0 — so a fabricated id cannot address a
    /// live node.
    pub fn from_bits(bits: u64) -> Option<Self> {
        thunderdome::Index::from_bits(bits).map(Self)
    }
}

/// A note in a pattern. MUSICAL time (beats, f64) per the serialization rule
/// in transport.rs — sample positions are runtime-only, derived at compile.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Note {
    pub start_beats: f64,
    pub len_beats: f64,
    /// MIDI pitch, 0-127 (69 = A4 = 440Hz).
    pub pitch: u8,
    /// MIDI velocity, 1-127.
    pub vel: u8,
    /// Parameter LOCKS: `(param id, engine value)` overrides that apply
    /// the moment this note fires. A lock OVERWRITES the knob for its
    /// note; a later note without a lock on that param RESUMES the knob —
    /// the live knob, not a snapshot, so turning it mid-playback is heard
    /// on every unlocked note. Compile turns these into events; the walk
    /// never sees the notes.
    #[serde(default)]
    pub plocks: Vec<(u32, f32)>,
    /// Locks on OTHER nodes — the track's effects — as `(node bits,
    /// param id, engine value)`. The voice's clock fires them at the
    /// note's own sample into a bounded bus, and the walk hands each to
    /// its node before that node renders the same block: as tight as a
    /// block, which is what a letter gets too. An unlocked note on a
    /// parameter locked anywhere restores the node's knob, the way a
    /// voice lock restores the voice's.
    #[serde(default)]
    pub fx_locks: Vec<(u64, u32, f32)>,
    /// TRIG probability, `0..=1`. Deterministic by DESIGN: the decision
    /// is a hash of the note's identity and the pattern cycle, so it
    /// feels random across cycles while a bounce reproduces the take
    /// bit for bit — the engine's flagship guarantee outranks dice.
    #[serde(default = "prob_default")]
    pub prob: f32,
    /// An Elektron A:B condition: fire only on pass A of every B cycles
    /// (`(1,2)` = first of every two, `(4,4)` = last of every four).
    /// `None` fires every cycle.
    #[serde(default)]
    pub cond: Option<(u8, u8)>,
}

fn prob_default() -> f32 {
    1.0
}

/// Swing one pattern: every ODD step of `grid` shifts toward the next
/// even step by `swing * grid / 2`. Zero is straight; one parks the
/// off-step three quarters of the way to the next step — the classic
/// maximum. Beats and coarser do not swing: delaying whole beats is not
/// a feel, it is a different song.
///
/// Only notes exactly ON the grid swing — a note placed between the
/// lines was put there on purpose. Pure and deterministic, so a bounce
/// reproduces the swung take bit for bit.
pub fn swing_note_starts(notes: &mut [Note], grid: f64, swing: f32) {
    if grid <= 0.0 || grid >= 1.0 || swing <= 0.0 || notes.is_empty() {
        return;
    }
    let amount = f64::from(swing.clamp(0.0, 1.0)) * grid * 0.5;
    let tolerance = grid * 1e-4;
    for note in notes {
        let step = (note.start_beats / grid).round();
        if (note.start_beats - step * grid).abs() > tolerance {
            continue;
        }
        let on = step as i64;
        if on > 0 && on.rem_euclid(2) != 0 {
            note.start_beats += amount;
        }
    }
}

/// A region of a pattern that plays several times before the rest continues.
/// Musical time, like everything stored. Subloops are COMPILE-TIME: they
/// unroll into a plain linear note list, so the red zone never knows they
/// exist — no new runtime state, no new ways to hang a note.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SubLoop {
    pub start_beats: f64,
    pub end_beats: f64,
    /// Total times the region plays. 1 = no effect. Capped at compile.
    pub repeats: u32,
}

/// Unroll subloops into a linear note list. Pure, green zone, and the whole
/// feature: repeats duplicate the region's notes at shifted offsets, and
/// everything after the region shifts right by the added length.
///
/// Rules (documented, tested):
/// - Subloops must be well-formed (end > start, repeats >= 1) and
///   non-overlapping; otherwise `BadSubLoop`.
/// - A note STARTING inside the region is duplicated once per play,
///   keeping its full length even if it rings past the region.
/// - A note starting before the region and sustaining into it plays once,
///   unstretched — it belongs to the timeline, not the region.
pub fn expand_subloops(notes: &[Note], subloops: &[SubLoop]) -> Result<Vec<Note>, CompileError> {
    let mut loops: Vec<SubLoop> = subloops.to_vec();
    for l in &loops {
        if l.end_beats <= l.start_beats
            || !l.end_beats.is_finite()
            || !l.start_beats.is_finite()
            || l.repeats == 0
            || l.repeats > 64
        {
            return Err(CompileError::BadSubLoop);
        }
    }
    loops.sort_by(|a, b| {
        a.start_beats
            .partial_cmp(&b.start_beats)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if loops.windows(2).any(|w| w[1].start_beats < w[0].end_beats) {
        return Err(CompileError::BadSubLoop);
    }

    let mut out = Vec::with_capacity(notes.len() * 2);
    for n in notes {
        // Offset accumulated from every subloop that ends at or before this
        // note's start; then duplication if it starts inside one.
        let mut shift = 0.0f64;
        let mut placed = false;
        for l in &loops {
            let added = (l.repeats - 1) as f64 * (l.end_beats - l.start_beats);
            if n.start_beats >= l.end_beats {
                shift += added;
            } else if n.start_beats >= l.start_beats {
                // Starts inside this loop: one copy per play.
                let period = l.end_beats - l.start_beats;
                for r in 0..l.repeats {
                    out.push(Note {
                        start_beats: n.start_beats + shift + r as f64 * period,
                        ..n.clone()
                    });
                }
                placed = true;
                break;
            } else {
                break; // before this loop (and loops are sorted): plain note
            }
        }
        if !placed {
            out.push(Note {
                start_beats: n.start_beats + shift,
                ..n.clone()
            });
        }
    }
    Ok(out)
}

/// Expanded length of a pattern in beats, given its nominal length.
pub fn expanded_len_beats(nominal: f64, subloops: &[SubLoop]) -> f64 {
    nominal
        + subloops
            .iter()
            .filter(|l| l.end_beats > l.start_beats && l.repeats >= 1)
            .map(|l| (l.repeats - 1) as f64 * (l.end_beats - l.start_beats))
            .sum::<f64>()
}

/// One compiled sequencer event, SAMPLE-stamped.
///
/// Sequencing contract rule 1: "Every event is timeline-sample-stamped
/// (u64). No block-relative events." The stamp is pattern-relative in clip
/// mode and timeline-absolute otherwise; either way it is an integer at the
/// compiled tempo, which is what `compile_at_tempo` exists to produce.
///
/// Beats were the previous stamp, and they cost a float multiply-add, a
/// division and a `floor` PER SAMPLE to compare against. Integers cost a
/// compare, and only at event boundaries.
///
/// Sorted by (sample, rank) — rank 0 = note-off, rank 1 = note-on, so an
/// off at the same sample as an on always lands first (contract rule 3: the
/// same-pitch back-to-back case must not have the off kill the new note).
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct SeqEvent {
    sample: u64,
    rank: u8,
    pitch: u8,
    vel: u8,
    /// Rank-1 (plock) events only: which parameter, and the locked value.
    /// On a RESTORE, zero means an immediate return to the live base and a
    /// positive value is the fraction of the remaining distance used by one
    /// point of a short de-click glide.
    param: u32,
    value: f32,
    restore: bool,
    /// The node a lock is for, as `NodeId::to_bits`; zero is the voice
    /// itself, and anything else goes out on the clock's lock bus.
    node: u64,
    /// Trig condition, carried by the note's ON and OFF alike: both make
    /// the SAME deterministic decision, so a skipped note's off cannot
    /// release some other voice of the same pitch.
    prob: f32,
    cond: (u8, u8),
    /// The note's identity for the probability hash — its on-sample's
    /// low bits, fixed at compile.
    trig_key: u32,
}

/// Does this event's note FIRE on `cycle`? Pure and deterministic: the
/// A:B condition is arithmetic on the cycle number, and probability is a
/// HASH of (note identity, cycle) mapped to `[0, 1)` — the same take
/// every playback and every bounce, varied across cycles.
fn trig_fires(prob: f32, cond: (u8, u8), trig_key: u32, cycle: i64) -> bool {
    let (a, b) = cond;
    if b > 0 && (cycle.rem_euclid(i64::from(b)) + 1) != i64::from(a) {
        return false;
    }
    if prob >= 1.0 {
        return true;
    }
    if prob <= 0.0 {
        return false;
    }
    // SplitMix-style avalanche over the pair; top 24 bits become the
    // unit float, the same mantissa-exact trick the noise kernel uses.
    let mut x = u64::from(trig_key) ^ (cycle as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (((x >> 40) as f32) / 16_777_216.0) < prob
}

/// Polyphony of the built-in sequencer synth.
const SEQ_VOICES: usize = 8;

/// Envelope floor: below this a voice is silent and reusable (-80 dB).
const ENV_FLOOR: f32 = 1e-4;

/// User-facing parameters of the built-in sequencer synth — the first real
/// device. They live in the SPEC so projects persist them, and compile to
/// per-sample rates in the node. `#[serde(default)]` on the spec field
/// keeps old project files loading; the defaults reproduce the previously
/// hardcoded voice (unity gain, ~1 ms attack, ~640 ms release at 48 kHz).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SynthParams {
    /// Post-sum gain, `0..=2`, ramped per block (1.0 = the old fixed level,
    /// applied before the 8-voice headroom scale).
    pub gain: f32,
    /// Voice attack: milliseconds from silent to full level.
    pub attack_ms: f32,
    /// Voice release: milliseconds from full level to inaudible
    /// ([`ENV_FLOOR`], -80 dB).
    pub release_ms: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            attack_ms: 1.0,
            release_ms: 640.0,
        }
    }
}

/// Per-sample envelope increment for an attack time. The clamp makes the
/// division finite for any input — this also runs in the red zone when a
/// ParamChange letter arrives. The bounds are the param table's, so the
/// widget's knob range and this floor are the same row.
fn synth_attack_rate(attack_ms: f32, sample_rate: f32) -> f32 {
    let def = crate::params::seq::TABLE[crate::params::seq::ATTACK as usize];
    1.0 / (def.clamp(attack_ms) * 1e-3 * sample_rate).max(1.0)
}

/// Per-sample envelope multiplier that decays from 1.0 to [`ENV_FLOOR`]
/// over the release time. `powf` is pure bounded math — red-zone legal.
fn synth_release_coeff(release_ms: f32, sample_rate: f32) -> f32 {
    let def = crate::params::seq::TABLE[crate::params::seq::RELEASE as usize];
    ENV_FLOOR.powf(1.0 / (def.clamp(release_ms) * 1e-3 * sample_rate).max(1.0))
}

/// The polyphony, as parallel arrays rather than an array of structs.
///
/// Structure-of-arrays because the per-sample loop touches every voice's
/// `phase`, `step`, `env` and `amp` and nothing else: as SoA those are four
/// contiguous runs of `SEQ_VOICES` floats, which is what a vector unit
/// wants. `[f32; 8]` is exactly one AVX2 register, and `.cargo/config.toml`
/// sets `target-cpu=native`. As AoS the same loop strides over eight
/// structs, touching a different cache line each time and vectorizing
/// nothing.
///
/// The split that makes this work: CONTROL is scalar and happens per EVENT
/// (allocate a voice, steal one, release one), while the dense per-sample
/// arithmetic is uniform across voices. Events are sparse — a handful in a
/// block — and samples are not, so the scalar half is nearly free.
///
/// `step` is the per-sample phase increment, computed once at note-on. The
/// previous shape stored `freq` and divided by the sample rate inside the
/// inner loop: a division per voice per sample, for a value that only
/// changes when a note starts.
///
/// This layout is deliberately settled BEFORE there is more than one
/// instrument. Converting AoS to SoA after three exist is three rewrites.
#[derive(Debug, Clone, Copy)]
pub struct VoiceBank {
    phase: [f32; SEQ_VOICES],
    step: [f32; SEQ_VOICES],
    env: [f32; SEQ_VOICES],
    amp: [f32; SEQ_VOICES],
    gate: [bool; SEQ_VOICES],
    pitch: [u8; SEQ_VOICES],
    /// When each voice was triggered, as a monotonic stamp. Stealing has to
    /// know which note is OLDEST, and envelope level cannot say: a note
    /// that just started is the quietest thing on the keyboard, so
    /// "steal the quietest" steals the note you are still playing.
    age: [u64; SEQ_VOICES],

    // The voice's own settings. These live HERE rather than beside the
    // pattern clock because the clock is an instrument-agnostic walk over
    // stamped events — it decides WHEN a note starts, and has no business
    // knowing what an attack rate is. That split is what lets one walk
    // drive more than one instrument.
    sample_rate: f32,
    attack_rate: f32,
    release_coeff: f32,
    /// The KNOB values, in ms — what letters set, and what a plock
    /// restore returns to. The rates above are the LIVE values, which a
    /// parameter lock may have overridden for the current note.
    base_attack_ms: f32,
    base_release_ms: f32,
}

impl Default for VoiceBank {
    fn default() -> Self {
        let p = SynthParams::default();
        Self {
            phase: [0.0; SEQ_VOICES],
            step: [0.0; SEQ_VOICES],
            env: [0.0; SEQ_VOICES],
            amp: [0.0; SEQ_VOICES],
            gate: [false; SEQ_VOICES],
            pitch: [0; SEQ_VOICES],
            age: [0; SEQ_VOICES],
            sample_rate: 48_000.0,
            attack_rate: synth_attack_rate(p.attack_ms, 48_000.0),
            release_coeff: synth_release_coeff(p.release_ms, 48_000.0),
            base_attack_ms: p.attack_ms,
            base_release_ms: p.release_ms,
        }
    }
}

impl VoiceBank {
    /// The pitches currently gated. Tests only — voice allocation is not
    /// observable from outside the node, and the stealing rules are worth
    /// asserting on directly.
    #[cfg(test)]
    fn held(&self) -> impl Iterator<Item = u8> + '_ {
        (0..SEQ_VOICES)
            .filter(|&v| self.gate[v])
            .map(|v| self.pitch[v])
    }

    /// Green zone. Sample rate and voice times, from the spec's params.
    fn prepare(&mut self, sample_rate: f32, params: SynthParams) {
        self.sample_rate = sample_rate;
        self.attack_rate = synth_attack_rate(params.attack_ms, sample_rate);
        self.release_coeff = synth_release_coeff(params.release_ms, sample_rate);
        self.base_attack_ms = params.attack_ms;
        self.base_release_ms = params.release_ms;
    }

    /// Red zone. Attack time, in ms, as a ParamChange letter delivers it.
    /// A LETTER is the knob: it moves the base a plock restore returns
    /// to, as well as the live rate.
    fn set_attack_ms(&mut self, ms: f32) {
        self.base_attack_ms = ms;
        self.attack_rate = synth_attack_rate(ms, self.sample_rate);
    }

    /// Red zone. Release time, in ms.
    fn set_release_ms(&mut self, ms: f32) {
        self.base_release_ms = ms;
        self.release_coeff = synth_release_coeff(ms, self.sample_rate);
    }

    /// Red zone. A parameter lock: override the LIVE value for the note
    /// about to fire, or restore the knob. Only the voice's own times
    /// answer; gain is the node's and a lock on it is dropped here.
    fn plock(&mut self, param: u32, value: Option<f32>) {
        use crate::params::seq;
        match param {
            seq::ATTACK => {
                let ms = value.unwrap_or(self.base_attack_ms);
                self.attack_rate = synth_attack_rate(ms, self.sample_rate);
            }
            seq::RELEASE => {
                let ms = value.unwrap_or(self.base_release_ms);
                self.release_coeff = synth_release_coeff(ms, self.sample_rate);
            }
            _ => {}
        }
    }

    fn plock_glide(&mut self, param: u32, alpha: f32) {
        use crate::params::seq;
        let alpha = alpha.clamp(0.0, 1.0);
        match param {
            seq::ATTACK => {
                let base = synth_attack_rate(self.base_attack_ms, self.sample_rate);
                self.attack_rate += (base - self.attack_rate) * alpha;
            }
            seq::RELEASE => {
                let base = synth_release_coeff(self.base_release_ms, self.sample_rate);
                self.release_coeff += (base - self.release_coeff) * alpha;
            }
            _ => {}
        }
    }

    /// Red zone. Silence everything, now — what a discontinuity demands.
    ///
    /// Clears the VOICES, not the settings. `*self = default()` would take
    /// the sample rate and the envelope times with it, so every seek would
    /// silently re-tune the synth to 48 kHz and the table defaults.
    fn all_sound_off(&mut self) {
        self.phase = [0.0; SEQ_VOICES];
        self.step = [0.0; SEQ_VOICES];
        self.env = [0.0; SEQ_VOICES];
        self.amp = [0.0; SEQ_VOICES];
        self.gate = [false; SEQ_VOICES];
        self.pitch = [0; SEQ_VOICES];
        self.age = [0; SEQ_VOICES];
    }

    /// Red zone. Release every gate without cutting the tails, which is
    /// what stopping the transport means.
    fn release_all(&mut self) {
        self.gate = [false; SEQ_VOICES];
    }

    /// Red zone. Release the first voice gated on `pitch`. One voice, not
    /// all of them: two note-ons of one pitch are two voices, and one
    /// note-off should end one of them.
    fn note_off(&mut self, pitch: u8) {
        for v in 0..SEQ_VOICES {
            if self.gate[v] && self.pitch[v] == pitch {
                self.gate[v] = false;
                break;
            }
        }
    }

    /// Red zone. Start `pitch` on a free voice, or steal one. Bounded: two
    /// passes over a fixed-size array, no allocation, no panic path.
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        // A free voice is one that is BOTH ungated and faded out. Testing
        // the envelope alone hands back the voice allocated one event
        // earlier at this very sample (its env is still 0.0), which is how
        // a chord collapses onto one voice.
        let slot = (0..SEQ_VOICES)
            .find(|&v| !self.gate[v] && self.env[v] < ENV_FLOOR)
            .unwrap_or_else(|| {
                // Everything is sounding: steal a releasing voice before a
                // held one, and the oldest before the newest. `(gate, age)`
                // orders exactly that — false sorts before true.
                let mut best = 0usize;
                let mut key = (true, u64::MAX);
                for v in 0..SEQ_VOICES {
                    if (self.gate[v], self.age[v]) < key {
                        key = (self.gate[v], self.age[v]);
                        best = v;
                    }
                }
                best
            });
        let freq = 440.0 * ((pitch as f32 - 69.0) / 12.0).exp2();
        self.phase[slot] = 0.0;
        self.step[slot] = freq / self.sample_rate.max(1.0);
        self.env[slot] = 0.0;
        self.amp[slot] = vel as f32 / 127.0;
        self.gate[slot] = true;
        self.pitch[slot] = pitch;
        self.age[slot] = age;
    }

    /// Red zone. Render `out` samples, summing every voice.
    ///
    /// Which lanes are worth stepping is decided ONCE per run, not per
    /// sample. Voices only start at events, and a run is bounded by
    /// events, so the active set cannot GROW inside a run — it can only
    /// shrink as tails decay, and stepping a lane briefly past its death
    /// is far cheaper than asking every lane every sample.
    ///
    /// The uniform, branchless version — step all eight always — was tried
    /// and MEASURED, because `sin` is a scalar libm call and paying eight
    /// of them to avoid a branch is a bad trade when one voice sounds.
    /// `report_seq_cost_per_sample`, release, ns/sample:
    ///
    /// ```text
    ///                      beat-stamped   branchless   per-run lanes
    ///   silent                    10.38        20.85            1.67
    ///   sparse (1 note/beat)      12.92        38.76            6.62
    ///   clip loop (1 bar)         21.48        29.72           14.11
    ///   dense (8 voices held)     85.63        63.71           87.71
    /// ```
    ///
    /// One run each, and they move a few percent between runs — the shape
    /// is the point, not the digits.
    ///
    /// So: branchless wins only when every lane is already sounding, and
    /// loses badly everywhere else. The dense column is the one that sets
    /// the deadline margin, and 20 ns/sample there is ~5us of a 5333us
    /// block budget — immaterial, where the sparse column is the case
    /// every real arrangement spends its time in.
    ///
    /// This inverts once the oscillator becomes vectorizable (`dsp::osc`,
    /// per the synth brief): then the lane loop is a vector step and the
    /// gather is the cost. The layout is already right for it; only this
    /// loop changes.
    ///
    /// `gain` ramps linearly across the run and must advance once per
    /// sample on EVERY path, including silence, or it lands in the wrong
    /// place and the caller's exact-landing assignment hides the drift.
    fn render(&mut self, out: &mut [f32], gain: &mut Ramp) {
        let (attack, release) = (self.attack_rate, self.release_coeff);
        let mut lanes = [0usize; SEQ_VOICES];
        let mut live = 0usize;
        for v in 0..SEQ_VOICES {
            if self.gate[v] || self.env[v] >= ENV_FLOOR {
                lanes[live] = v;
                live += 1;
            } else {
                // Settle a lane the moment it drops out rather than
                // leaving its envelope frozen wherever the run ended.
                // A frozen value is state that depends on where run
                // boundaries fell, and run boundaries depend on block
                // size; zero does not.
                self.env[v] = 0.0;
            }
        }
        if live == 0 {
            for s in out.iter_mut() {
                *s = 0.0;
                gain.next();
            }
            return;
        }
        for s in out.iter_mut() {
            let mut acc = 0.0f32;
            for &v in &lanes[..live] {
                self.env[v] = if self.gate[v] {
                    (self.env[v] + attack).min(1.0)
                } else {
                    self.env[v] * release
                };
                acc += (self.phase[v] * TAU).sin() * self.env[v] * self.amp[v];
                self.phase[v] = (self.phase[v] + self.step[v]).fract();
            }
            *s = acc * 0.25 * gain.next(); // headroom across 8 voices
        }
    }
}

/// What the pattern walk needs of an instrument, and nothing more.
///
/// The walk in [`PatternClock::run`] is entirely instrument-agnostic: it
/// decides WHEN a note starts, wraps a clip, and reconciles the cycle
/// against the transport. It has no opinion about oscillators, envelopes
/// or voice counts. Five methods is the whole surface, which is what lets
/// one heavily-reasoned walk drive both the 8-voice `Seq` synth and the
/// lane-major poly synth instead of the two carrying a copy each.
///
/// Static dispatch — `run` is generic, never `dyn` — so this costs
/// nothing in the callback: it monomorphises to exactly the code the
/// hand-written version was.
trait Voices {
    fn all_sound_off(&mut self);
    fn release_all(&mut self);
    fn note_off(&mut self, pitch: u8);
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64);
    /// A parameter LOCK firing at a note boundary: `Some(value)` is a
    /// note's own override, `None` restores the instrument's LIVE base —
    /// the knob as letters have most recently set it, not a compile-time
    /// snapshot, so a knob turned mid-playback is heard on every
    /// unlocked note.
    fn plock(&mut self, param: u32, value: Option<f32>);
    /// Move the live lock value one prepared fraction toward its base.
    /// The compiler emits a small fixed series for trigless restores;
    /// `alpha == 1` lands exactly on the live knob.
    fn plock_glide(&mut self, param: u32, alpha: f32);
    /// Render `out.len()` samples of the LEFT (or mono) channel.
    ///
    /// `at` is where this run starts within the segment. A mono
    /// instrument ignores it; a STEREO one uses it to place its right
    /// channel in a buffer of its own, which the node reads back after
    /// the walk. The clock stays mono-shaped on purpose — it schedules
    /// notes, and how many channels an instrument has is none of its
    /// business.
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp);
}

impl Voices for VoiceBank {
    fn all_sound_off(&mut self) {
        VoiceBank::all_sound_off(self);
    }
    fn release_all(&mut self) {
        VoiceBank::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        VoiceBank::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        VoiceBank::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        VoiceBank::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        VoiceBank::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut Ramp) {
        VoiceBank::render(self, out, gain);
    }
}

/// Haze as an instrument the pattern clock can play.
///
/// The same shape `PolyVoices` has, one instrument along — which is the
/// point of having extracted the clock in the first place.
impl Voices for crate::audio::haze::Haze {
    fn all_sound_off(&mut self) {
        crate::audio::haze::Haze::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::haze::Haze::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::haze::Haze::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::haze::Haze::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::haze::Haze::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::haze::Haze::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::haze::Haze::render(self, out, at, gain);
    }
}

/// The kick as an instrument the pattern clock can play.
///
/// A ONE-SHOT drum, so half this trait is deliberately empty. A kick has
/// no sustain stage to release and no note to hold: the strike is the
/// whole gesture, and `note_off` arriving a beat later must not cut a
/// tail that is still decaying. `all_sound_off` DOES silence it, because
/// that is the transport saying the position has moved and nothing from
/// before it should still be sounding — the sequencing contract's second
/// rule.
impl Voices for crate::audio::kick::KickVoice {
    fn all_sound_off(&mut self) {
        crate::audio::kick::KickVoice::reset(self);
    }
    fn release_all(&mut self) {
        // Nothing to release. The decay is the sound.
    }
    fn note_off(&mut self, _pitch: u8) {
        // Ditto: a one-shot ignores the gate going down.
    }
    fn note_on(&mut self, pitch: u8, vel: u8, _age: u64) {
        crate::audio::kick::KickVoice::trigger(self, pitch, vel);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::kick::KickVoice::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::kick::KickVoice::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut Ramp) {
        // The trait's contract is WRITE, not add: clear, fill, then ride
        // the ramp. The voice itself adds, because that is what a bank of
        // them would need.
        for sample in out.iter_mut() {
            *sample = 0.0;
        }
        crate::audio::kick::KickVoice::render_add(self, out, 1.0);
        for sample in out.iter_mut() {
            *sample *= gain.next();
        }
    }
}

/// The one-shot drums as instruments the pattern clock can play.
///
/// Four identical trait impls, written once. Every drum in the rack after
/// the kick has exactly the shape the kick's own impl documents — no
/// sustain stage to release, no gate to drop, a strike that IS the whole
/// gesture, and an `all_sound_off` that really does silence it because a
/// seek must leave nothing from before it sounding. Spelling that out
/// four times would be four chances for one of them to drift, and the
/// drift would be silent: a drum that ignored `all_sound_off` would only
/// misbehave after a seek, which is not where anybody looks.
///
/// The kick keeps its own hand-written impl. It is the one with the prose
/// explaining WHY the empty halves are empty, and that explanation is
/// worth more where it is than folded into a macro.
macro_rules! one_shot_drum_voices {
    ($voice:ty) => {
        impl Voices for $voice {
            fn all_sound_off(&mut self) {
                <$voice>::reset(self);
            }
            fn release_all(&mut self) {
                // Nothing to release. The decay is the sound.
            }
            fn note_off(&mut self, _pitch: u8) {
                // Ditto: a one-shot ignores the gate going down.
            }
            fn note_on(&mut self, pitch: u8, vel: u8, _age: u64) {
                <$voice>::trigger(self, pitch, vel);
            }
            fn plock(&mut self, param: u32, value: Option<f32>) {
                <$voice>::plock(self, param, value);
            }
            fn plock_glide(&mut self, param: u32, alpha: f32) {
                <$voice>::plock_glide(self, param, alpha);
            }
            fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut Ramp) {
                // The trait's contract is WRITE, not add: clear, fill,
                // then ride the ramp. The voice itself adds, because that
                // is what a bank of them would need.
                for sample in out.iter_mut() {
                    *sample = 0.0;
                }
                <$voice>::render_add(self, out, 1.0);
                for sample in out.iter_mut() {
                    *sample *= gain.next();
                }
            }
        }
    };
}

/// The acid mono as an instrument the pattern clock can play.
///
/// Unlike the drums this is a GATED instrument, so `note_off` and
/// `release_all` both do real work — and unlike the poly synth there is
/// no voice to steal, so `note_on` never has to choose. `age` is ignored
/// for exactly that reason.
impl Voices for crate::audio::acid::AcidVoice {
    fn all_sound_off(&mut self) {
        // The transport moved: nothing from before it may still sound.
        crate::audio::acid::AcidVoice::reset(self);
    }
    fn release_all(&mut self) {
        // The transport stopped: let the gate down and let the tail ring.
        crate::audio::acid::AcidVoice::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::acid::AcidVoice::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, _age: u64) {
        crate::audio::acid::AcidVoice::note_on(self, pitch, vel);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::acid::AcidVoice::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::acid::AcidVoice::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut Ramp) {
        // The trait's contract is WRITE, not add: clear, fill, then ride
        // the ramp — which is where the LEVEL knob lives.
        for sample in out.iter_mut() {
            *sample = 0.0;
        }
        crate::audio::acid::AcidVoice::render_add(self, out, 1.0);
        for sample in out.iter_mut() {
            *sample *= gain.next();
        }
    }
}

one_shot_drum_voices!(crate::audio::snare::SnareVoice);
one_shot_drum_voices!(crate::audio::tom::TomVoice);
one_shot_drum_voices!(crate::audio::hat::HatVoice);
one_shot_drum_voices!(crate::audio::handclap::HandclapVoice);

impl Voices for crate::audio::poly::PolyVoices {
    fn all_sound_off(&mut self) {
        crate::audio::poly::PolyVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::poly::PolyVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::poly::PolyVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::poly::PolyVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::poly::PolyVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::poly::PolyVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::poly::PolyVoices::render(self, out, at, gain);
    }
}

/// The wavetable synth as an instrument the pattern clock can play.
/// The same shape `PolyVoices` has, one instrument along.
impl Voices for crate::audio::loom::LoomVoices {
    fn all_sound_off(&mut self) {
        crate::audio::loom::LoomVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::loom::LoomVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::loom::LoomVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::loom::LoomVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::loom::LoomVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::loom::LoomVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::loom::LoomVoices::render(self, out, at, gain);
    }
}

/// The sampler as an instrument the pattern clock can play.
///
/// Unlike the one-shot drums, half of this is NOT empty: a sampler in
/// classic mode has a gate to drop and a release to run. One-shot and
/// slice modes ignore note-off, and they do it INSIDE the bank rather
/// than here, because which of the three you are in is a parameter the
/// bank owns.
/// The struck-resonator synth as an instrument the clock can play.
impl Voices for crate::audio::tine::TineVoices {
    fn all_sound_off(&mut self) {
        crate::audio::tine::TineVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::tine::TineVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::tine::TineVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::tine::TineVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::tine::TineVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::tine::TineVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::tine::TineVoices::render(self, out, at, gain);
    }
}

/// The bounced sine as an instrument the clock can play.
impl Voices for crate::audio::scomp::ScompVoices {
    fn all_sound_off(&mut self) {
        crate::audio::scomp::ScompVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::scomp::ScompVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::scomp::ScompVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::scomp::ScompVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::scomp::ScompVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::scomp::ScompVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::scomp::ScompVoices::render(self, out, at, gain);
    }
}

/// The house chord synth as an instrument the clock can play.
impl Voices for crate::audio::stab::StabVoices {
    fn all_sound_off(&mut self) {
        crate::audio::stab::StabVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::stab::StabVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::stab::StabVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::stab::StabVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::stab::StabVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::stab::StabVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::stab::StabVoices::render(self, out, at, gain);
    }
}

/// The four-operator FM synth as an instrument the clock can play.
impl Voices for crate::audio::quad::QuadVoices {
    fn all_sound_off(&mut self) {
        crate::audio::quad::QuadVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::quad::QuadVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::quad::QuadVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::quad::QuadVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::quad::QuadVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::quad::QuadVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::quad::QuadVoices::render(self, out, at, gain);
    }
}

/// The drum one-shot as an instrument the clock can play.
impl Voices for crate::audio::brick::BrickVoices {
    fn all_sound_off(&mut self) {
        crate::audio::brick::BrickVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::brick::BrickVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::brick::BrickVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::brick::BrickVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::brick::BrickVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::brick::BrickVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::brick::BrickVoices::render(self, out, at, gain);
    }
}

impl Voices for crate::audio::sampler::SamplerVoices {
    fn all_sound_off(&mut self) {
        crate::audio::sampler::SamplerVoices::all_sound_off(self);
    }
    fn release_all(&mut self) {
        crate::audio::sampler::SamplerVoices::release_all(self);
    }
    fn note_off(&mut self, pitch: u8) {
        crate::audio::sampler::SamplerVoices::note_off(self, pitch);
    }
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
        crate::audio::sampler::SamplerVoices::note_on(self, pitch, vel, age);
    }
    fn plock(&mut self, param: u32, value: Option<f32>) {
        crate::audio::sampler::SamplerVoices::plock(self, param, value);
    }
    fn plock_glide(&mut self, param: u32, alpha: f32) {
        crate::audio::sampler::SamplerVoices::plock_glide(self, param, alpha);
    }
    fn render(&mut self, out: &mut [f32], at: usize, gain: &mut Ramp) {
        crate::audio::sampler::SamplerVoices::render(self, out, at, gain);
    }
}

/// Where a pattern is, and how it maps onto the timeline.
///
/// Everything about playing a compiled event list that is NOT about the
/// instrument playing it: the cursor into the list, the clip modulus, the
/// cycle counter, and the two guards that keep the derivation honest
/// against a moving tempo.
///
/// Extracted so the poly synth does not carry a second copy. The
/// reasoning in `run` below — integer cycle derivation, the
/// strictly-greater wrap reconciliation, the monotonic phase guard — is
/// the most expensively-earned code in this file, and two copies of it
/// would drift the first time one was fixed.
/// One lock bound for another node, waiting on the clock's bus.
#[derive(Debug, Clone, Copy, Default)]
pub struct FxLock {
    pub node: u64,
    pub param: u32,
    pub value: f32,
    pub restore: bool,
}

/// One effect parameter's knob, remembered by the schedule so a lock
/// can be restored from it.
#[derive(Debug, Clone, Copy)]
struct FxBase {
    node: u64,
    param: u32,
    base: f32,
    live: f32,
    /// A pattern lock is the top performance layer. Automation may keep
    /// moving the value underneath it, but cannot erase the lock before its
    /// explicit restore event arrives.
    locked: bool,
}

/// The most effect locks one clock can post in one block. A fixed
/// array, because the callback allocates nothing; a note with more
/// locks than this on more effects than this loses the surplus for the
/// block, which is a bound and not a crash.
pub const FX_BUS: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct PatternClock {
    /// Index into the compiled event list.
    cursor: usize,
    /// Stamp handed to the next triggered voice; only ever increases.
    next_age: u64,
    /// Clip mode: the pattern cycles every this many SAMPLES at the
    /// compiled tempo, forever, while the timeline rolls forward
    /// (Ableton-style). 0 = one-shot linear, never wraps.
    ///
    /// The single clock. The beat-valued length it is derived from is
    /// deliberately NOT kept alongside it: the cycle a segment belongs
    /// to is derived by integer division on this same modulus, and a
    /// float copy of the same fact would disagree by up to half a
    /// sample at any tempo whose samples-per-beat is not integral.
    loop_samples: u64,
    /// Samples per beat at the COMPILED tempo. Event stamps are in this
    /// space, so the phase must be derived in it too; a live tempo
    /// would silently disagree with every stamp in the list.
    samples_per_beat: f64,
    /// Cycle number of the last processed sample (clip mode). An integer
    /// counter, not a float comparison — wrap detection at segment
    /// boundaries must not be a rounding coin flip (see Click).
    last_cycle: i64,
    /// Pattern-relative sample of the last processed segment. Each
    /// segment re-derives its position from `ctx.beat` rather than
    /// accumulating, so this exists to keep that derivation MONOTONIC:
    /// a float rounding that stepped backward would fire an event twice.
    prev_phase: u64,
    /// Effect locks fired this block, for the walk to deliver.
    fx_out: [FxLock; FX_BUS],
    fx_len: usize,
}

impl PatternClock {
    fn new(loop_samples: u64, samples_per_beat: f64) -> Self {
        Self {
            cursor: 0,
            next_age: 0,
            loop_samples,
            samples_per_beat,
            last_cycle: 0,
            prev_phase: 0,
            fx_out: [FxLock::default(); FX_BUS],
            fx_len: 0,
        }
    }

    /// Red zone. Play `events` into `out` for this segment, gating
    /// `voices` as the stamps say.
    ///
    /// `gain` is the caller's ramp, already spanning the segment; it must
    /// advance once per sample on EVERY path, silence included, which the
    /// instrument's `render` is responsible for.
    /// Take this block's effect locks off the bus, in the order fired.
    fn take_fx(&mut self) -> ([FxLock; FX_BUS], usize) {
        let out = (self.fx_out, self.fx_len);
        self.fx_len = 0;
        out
    }

    fn run<V: Voices>(
        &mut self,
        voices: &mut V,
        events: &[SeqEvent],
        out: &mut [f32],
        ctx: &ProcessCtx<'_>,
        gain: &mut Ramp,
    ) {
        // Where this segment starts, as a PATTERN-RELATIVE sample.
        //
        // Derived from `ctx.beat` once per segment rather than
        // accumulated per sample: accumulating drifts, and rounding
        // the loop length to whole samples would drift the pattern
        // against the timeline over a long take. One float divide
        // and one multiply per segment buys exactness, and every
        // comparison after this is an integer.
        // The COMPILED samples-per-beat, not the live one. Event
        // stamps and `loop_samples` were both frozen at compile, so
        // deriving the phase with a live tempo would put it in
        // different units from the thing it is compared against:
        // a tempo raised after compile makes the live cycle shorter
        // than `loop_samples` and the wrap becomes unreachable;
        // lowered, the phase overshoots and the whole pattern fires
        // at one sample. Using the compiled figure keeps everything
        // in one space, so events land on their correct BEATS and a
        // tempo change is merely un-recompiled until the swap
        // arrives — which is what "compiled at tempo" already means
        // for every audio clip in the graph.
        let spb = self.samples_per_beat;
        // The cycle is derived with the SAME modulus the walk wraps
        // on, by integer division — not by a float `beat / len`.
        //
        // Two clocks that mean the same thing must be one clock.
        // `loop_samples` is `round(len * spb)`, so a float
        // derivation disagrees with the walk by up to half a sample
        // whenever `len * spb` is not integral — which is most
        // tempos: 130bpm at 48kHz gives 22153.846 samples a beat.
        // The walk would wrap one sample early, the derivation
        // would still report the old cycle, and the mismatch
        // handler below would "reconcile" by killing the voices the
        // new cycle had just started. Deriving both from
        // `loop_samples` makes them agree by construction.
        // A one-shot arrangement is already stamped in the transport's
        // absolute sample domain, including through tempo-map changes. A
        // looping session clip instead owns a synthetic, fixed-tempo sample
        // domain so its integer modulus and its event stamps stay identical.
        let absolute = if self.loop_samples == 0 {
            ctx.position
        } else {
            (ctx.beat * spb).max(0.0) as u64
        };
        let (mut cycle, mut phase) = match absolute.checked_div(self.loop_samples) {
            Some(cyc) => (cyc as i64, absolute % self.loop_samples),
            // No loop: one linear pass, position is the phase.
            None => (0, absolute),
        };

        if ctx.discontinuity {
            // All-sound-off, hard: contract rule 2. A wrap or seek
            // must never leave a hanging note. Then reseek — a
            // bounded binary search, no allocation.
            voices.all_sound_off();
            self.cursor = events.partition_point(|e| e.sample < phase);
            self.last_cycle = cycle;
            self.prev_phase = phase;
        } else if cycle > self.last_cycle {
            // A cycle boundary landed exactly on the PREVIOUS
            // segment's last sample, so the walk below never saw
            // it: it stops as soon as the output is full, and the
            // wrap sits one iteration past that. Reconcile here.
            //
            // Whole-bar clips hit this constantly — one bar at
            // 120bpm/48kHz is 96000 samples, an exact multiple of
            // every ordinary block size — and the symptom is a clip
            // that plays once and then goes silent forever, with
            // any note-off clamped to the clip end left hanging.
            //
            // Strictly GREATER, not merely different: the walk can
            // legitimately be a cycle ahead of the derivation for
            // one segment (it wraps the instant phase reaches the
            // boundary; the derivation only reports the new cycle
            // once the beat crosses it). Flushing on that would cut
            // the notes the new cycle just started.
            flush_cycle(voices, events, &mut self.cursor);
        } else if cycle == self.last_cycle && phase < self.prev_phase {
            // The derivation stepped backward within a cycle — a
            // tempo change moved the beat under us. Hold position
            // rather than replaying events already fired.
            phase = self.prev_phase;
        }
        if !ctx.playing {
            // Stop: release everything. No note chase on resume — a
            // note straddling the stop point does not re-sound, its
            // on-event is already behind the cursor.
            voices.release_all();
            // Record where we are before leaving, or the branch
            // above never converges: scrubbing the playhead to
            // another cycle while paused would re-detect the same
            // mismatch and re-walk the whole event list every
            // block, forever, on a transport that should cost
            // nothing.
            self.last_cycle = cycle;
            self.prev_phase = phase;
            voices.render(out, 0, gain);
            return;
        }

        // Walk the segment in RUNS between events. Events are
        // sparse — a handful per block — and samples are not, so
        // the scalar event work happens a few times and the dense
        // arithmetic runs uniformly in between. This is the same
        // shape as the transport's own segment loop, one level
        // down, and it is what makes note timing sample-accurate
        // without a per-sample branch asking "is there an event
        // here?".
        //
        // Progress is proven: every pass either renders at least one
        // sample, consumes at least one event, or wraps (which
        // resets the cursor and moves the phase off the boundary),
        // and both the event list and the sample count are finite.
        // The `steps` bound is belt and braces against the
        // impossible — the same guard the callback's own segment
        // loop carries, for the same reason: an unbounded path in
        // the red zone is a hung render, not a wrong sample.
        let mut done = 0usize;
        let mut steps = 0usize;
        // A cycle costs at most one wrap, one event pass and one
        // render pass; a segment can hold at most `out.len()` cycles
        // because a wrap always leaves at least one sample to
        // render (`loop_samples` is either 0, meaning never wrap,
        // or >= 1). Budgeting the event list ONCE was wrong: the
        // cursor rewinds every wrap, so a pattern shorter than a
        // block re-walks it each cycle.
        let step_bound = (out.len() + 1).saturating_mul(events.len().saturating_mul(2) + 2);
        while done < out.len() && steps < step_bound {
            steps += 1;
            let remaining = (out.len() - done) as u64;

            // A clip wrap ends the run: flush the tail of the
            // cycle, restart the pattern, and carry on.
            let to_wrap = if self.loop_samples > 0 {
                self.loop_samples.saturating_sub(phase)
            } else {
                u64::MAX
            };

            // The next event bounds the run too. Zero-length runs
            // are normal: several events can share one sample.
            let to_event = events
                .get(self.cursor)
                .map(|e| e.sample.saturating_sub(phase))
                .unwrap_or(u64::MAX);

            let run = remaining.min(to_wrap).min(to_event) as usize;
            if run > 0 {
                voices.render(&mut out[done..done + run], done, gain);
                done += run;
                phase += run as u64;
            }
            if done >= out.len() {
                break;
            }

            if to_event <= to_wrap && self.cursor < events.len() {
                // Fire every event stamped at this exact sample, in
                // compiled order — off before on, so a same-pitch
                // back-to-back pair does not kill its successor.
                while let Some(ev) = events.get(self.cursor).copied() {
                    if ev.sample > phase {
                        break;
                    }
                    self.cursor += 1;
                    // The trig decision, made IDENTICALLY by the on,
                    // the off, and the plocks of one note: pure over
                    // (identity, cycle), so no event of a skipped note
                    // can act while the others stand down.
                    if !trig_fires(ev.prob, ev.cond, ev.trig_key, cycle) {
                        continue;
                    }
                    match ev.rank {
                        0 => voices.note_off(ev.pitch),
                        1 if ev.node == 0 && ev.restore && ev.value > 0.0 => {
                            voices.plock_glide(ev.param, ev.value)
                        }
                        1 if ev.node == 0 => {
                            voices.plock(ev.param, if ev.restore { None } else { Some(ev.value) })
                        }
                        1 => {
                            if self.fx_len < FX_BUS {
                                self.fx_out[self.fx_len] = FxLock {
                                    node: ev.node,
                                    param: ev.param,
                                    value: ev.value,
                                    restore: ev.restore,
                                };
                                self.fx_len += 1;
                            }
                        }
                        _ => {
                            let age = self.next_age;
                            self.next_age = self.next_age.wrapping_add(1);
                            voices.note_on(ev.pitch, ev.vel, age);
                        }
                    }
                }
            } else {
                // The wrap, seen mid-segment.
                flush_cycle(voices, events, &mut self.cursor);
                phase = 0;
                cycle += 1;
            }
        }
        if done < out.len() {
            // The bound tripped, which the proof above says cannot
            // happen. Fail to SILENCE rather than to whatever the
            // arena slot held last: a metered node must fill its
            // whole buffer, and the allocator recycles slots
            // between nodes.
            out[done..].fill(0.0);
        }
        self.last_cycle = cycle;
        self.prev_phase = phase;
    }
}

/// A slice table in frames, from fractions of a file `frames` long:
/// sorted, distinct, inside the file, and no more than the sampler holds.
pub(crate) fn slice_frames(fractions: &[f64], frames: u64) -> Vec<u64> {
    let mut table: Vec<u64> = fractions
        .iter()
        .filter(|f| f.is_finite() && (0.0..1.0).contains(*f))
        .map(|f| (f * frames as f64).round() as u64)
        .filter(|at| *at < frames)
        .collect();
    table.sort_unstable();
    table.dedup();
    table.truncate(crate::audio::sampler::MAX_SLICES);
    table
}

/// Bake a pattern's notes into one sorted, sample-stamped event list.
///
/// Instrument-agnostic, like the walk that consumes it: this turns musical
/// time into samples and nothing else, so `Seq` and `Poly` share it rather
/// than each carrying a copy of the clip-clamping rules — every one of
/// which exists because of a specific hanging-note bug, and none of which
/// should have to be fixed twice.
const PLOCK_GLIDE_POINTS: u64 = 4;

#[allow(clippy::too_many_arguments)]
fn push_plock_restore(
    events: &mut Vec<SeqEvent>,
    off: u64,
    stop: u64,
    glide_samples: u64,
    node: u64,
    param: u32,
    prob: f32,
    cond: (u8, u8),
    trig_key: u32,
) {
    let duration = stop.saturating_sub(off).min(glide_samples);
    let points = PLOCK_GLIDE_POINTS.min(duration.saturating_add(1)).max(1);
    for point in 0..points {
        let sample = if points == 1 {
            off
        } else {
            off + duration * point / (points - 1)
        };
        // Recurrent fractions land at equally spaced linear positions:
        // 1/N of the remaining distance, then 1/(N-1), ending at 1.
        let alpha = 1.0 / (points - point) as f32;
        events.push(SeqEvent {
            sample,
            rank: 1,
            pitch: 0,
            vel: 0,
            param,
            value: alpha,
            restore: true,
            node,
            prob,
            cond,
            trig_key,
        });
    }
}

fn compile_events(
    notes: &[Note],
    subloops: &[SubLoop],
    loop_len_beats: Option<f64>,
    samples_per_beat: f64,
    plock_glide_samples: u64,
) -> Result<Vec<SeqEvent>, CompileError> {
    if let Some(len) = loop_len_beats
        && !(len.is_finite() && len > 0.0)
    {
        return Err(CompileError::BadClipLen);
    }
    let notes = expand_subloops(notes, subloops)?;
    // Which parameters are locked ANYWHERE in the pattern. The lock rule
    // needs both halves of the story: a locked note-on sets its value,
    // and every UNLOCKED note-on on a locked-anywhere parameter emits a
    // RESTORE, so the knob comes back the moment a plain note plays —
    // "a non-locked note resumes the defined position", verbatim.
    let mut locked_anywhere: Vec<u32> = notes
        .iter()
        .flat_map(|n| n.plocks.iter().map(|(id, _)| *id))
        .collect();
    locked_anywhere.sort_unstable();
    locked_anywhere.dedup();
    // The same story for the effects: every (node, param) locked by any
    // note gets an event on every note — a value, or a restore.
    let mut fx_locked_anywhere: Vec<(u64, u32)> = notes
        .iter()
        .flat_map(|n| n.fx_locks.iter().map(|(node, id, _)| (*node, *id)))
        .collect();
    fx_locked_anywhere.sort_unstable();
    fx_locked_anywhere.dedup();
    // Bake note starts/ends into one sorted event list.
    // Sort by (beat, rank): offs before ons on exact ties.
    let mut events = Vec::with_capacity(notes.len() * 2);
    for n in &notes {
        let lock_only = n.vel == 0 && (!n.plocks.is_empty() || !n.fx_locks.is_empty());
        if n.len_beats <= 0.0 || (n.vel == 0 && !lock_only) {
            continue; // degenerate notes never enter the engine
        }
        // Clip mode: a note starting past the loop is
        // dropped; one ringing past it is cut at the wrap
        // (off clamped), so the wrap flush can never miss.
        let mut end = n.start_beats + n.len_beats;
        if let Some(len) = loop_len_beats {
            if n.start_beats >= len {
                continue;
            }
            end = end.min(len);
        }
        // Musical time becomes SAMPLES here and only
        // here — the contract's rule 1, and the whole
        // reason `compile_at_tempo` takes a tempo.
        // Rounding is to nearest so a note does not
        // consistently land early.
        let stamp = |beats: f64| -> u64 { (beats * samples_per_beat).round().max(0.0) as u64 };
        let on = stamp(n.start_beats);
        // The drop test above is in BEATS but the stamp
        // rounds to SAMPLES, so a note within half a
        // sample of the clip end survives the first
        // check and lands exactly ON the loop boundary.
        // The wrap flush is already past that point, so
        // such a note-on would gate a voice nothing
        // ever releases — a hanging note from a note
        // too short to hear.
        if let Some(len) = loop_len_beats
            && on >= stamp(len)
        {
            continue;
        }
        // Parameter locks fire BETWEEN the off and the on at this
        // sample (rank 1 of 0..=2): after the off so a same-sample
        // release still hears the old value's tail settings, before the
        // on so THIS note's attack, filter and pitch start under the
        // locked values rather than catching them a sample late.
        let prob = if n.prob.is_finite() {
            n.prob.clamp(0.0, 1.0)
        } else {
            1.0
        };
        // A:B sanitized at compile: A clamped into 1..=B, B into 1..=8.
        // Junk from a hand-edited file becomes the nearest real
        // condition rather than one that can never fire.
        let cond = match n.cond {
            Some((a, b)) => {
                let b = b.clamp(1, 8);
                (a.clamp(1, b), b)
            }
            None => (0, 0),
        };
        let trig_key = (on as u32) ^ (u32::from(n.pitch) << 24);
        let voice_params: Vec<u32> = if lock_only {
            n.plocks.iter().map(|(param, _)| *param).collect()
        } else {
            locked_anywhere.clone()
        };
        for &param in &voice_params {
            let lock = n.plocks.iter().find(|(id, _)| *id == param);
            events.push(SeqEvent {
                sample: on,
                rank: 1,
                pitch: n.pitch,
                vel: 0,
                param,
                value: lock.map(|(_, v)| *v).unwrap_or(0.0),
                restore: lock.is_none(),
                node: 0,
                prob,
                cond,
                trig_key,
            });
        }
        let effect_params: Vec<(u64, u32)> = if lock_only {
            n.fx_locks
                .iter()
                .map(|(node, param, _)| (*node, *param))
                .collect()
        } else {
            fx_locked_anywhere.clone()
        };
        for &(node, param) in &effect_params {
            let lock = n
                .fx_locks
                .iter()
                .find(|(target, id, _)| *target == node && *id == param);
            events.push(SeqEvent {
                sample: on,
                rank: 1,
                pitch: n.pitch,
                vel: 0,
                param,
                value: lock.map(|(_, _, v)| *v).unwrap_or(0.0),
                restore: lock.is_none(),
                node,
                prob,
                cond,
                trig_key,
            });
        }
        if lock_only {
            let off = stamp(end);
            for &(param, _) in &n.plocks {
                let next = notes
                    .iter()
                    .filter(|future| {
                        future.start_beats > n.start_beats
                            && future.len_beats > 0.0
                            && (future.vel > 0 || future.plocks.iter().any(|(id, _)| *id == param))
                    })
                    .map(|future| stamp(future.start_beats))
                    .min();
                let stop = next
                    .into_iter()
                    .chain(loop_len_beats.map(stamp))
                    .min()
                    .unwrap_or_else(|| off.saturating_add(plock_glide_samples));
                push_plock_restore(
                    &mut events,
                    off,
                    stop,
                    plock_glide_samples,
                    0,
                    param,
                    prob,
                    cond,
                    trig_key,
                );
            }
            for &(node, param, _) in &n.fx_locks {
                let next = notes
                    .iter()
                    .filter(|future| {
                        future.start_beats > n.start_beats
                            && future.len_beats > 0.0
                            && (future.vel > 0
                                || future
                                    .fx_locks
                                    .iter()
                                    .any(|(target, id, _)| *target == node && *id == param))
                    })
                    .map(|future| stamp(future.start_beats))
                    .min();
                let stop = next
                    .into_iter()
                    .chain(loop_len_beats.map(stamp))
                    .min()
                    .unwrap_or_else(|| off.saturating_add(plock_glide_samples));
                push_plock_restore(
                    &mut events,
                    off,
                    stop,
                    plock_glide_samples,
                    node,
                    param,
                    prob,
                    cond,
                    trig_key,
                );
            }
        } else {
            events.push(SeqEvent {
                sample: on,
                rank: 2,
                pitch: n.pitch,
                vel: n.vel,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob,
                cond,
                trig_key,
            });
            events.push(SeqEvent {
                sample: stamp(end),
                rank: 0,
                pitch: n.pitch,
                vel: 0,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob,
                cond,
                trig_key,
            });
        }
    }
    // (sample, rank): off < plock < on on exact ties — see the plock
    // comment above for why the locks ride the middle.
    events.sort_by(|a, b| a.sample.cmp(&b.sample).then(a.rank.cmp(&b.rank)));
    Ok(events)
}

/// Red zone. End a pattern cycle: release everything the pattern still owes
/// a note-off, then rewind the cursor.
///
/// Every off is at or before the loop length by the compile clamp, so
/// nothing can hang. Note-ons still ahead of the cursor would be
/// zero-length at this point and are skipped rather than retriggered.
///
/// A free function because it needs the voices, the event list and the
/// cursor at once, and those are three separate bindings destructured out
/// of the node.
fn flush_cycle<V: Voices>(voices: &mut V, events: &[SeqEvent], cursor: &mut usize) {
    while let Some(ev) = events.get(*cursor).copied() {
        *cursor += 1;
        if ev.rank == 0 {
            voices.note_off(ev.pitch);
        }
    }
    *cursor = 0;
}

/// Most inputs one node may have. Fixed so the callback can gather input
/// slices into a stack array — no allocation, bounded work.
pub const MAX_NODE_INPUTS: usize = 8;

/// How many meter taps a schedule reports to the UI.
///
/// Fixed, because the telemetry snapshot is a `Copy` struct riding a
/// `triple_buffer` and a Vec cannot travel in one. Tracks past this many
/// have no meter — they still sound.
pub const MAX_METERS: usize = 32;
/// The console's telemetry slots: enough for every IN section of a
/// handful of strips and the desk's own. Past this, a section is
/// simply not reported, and its card reads silence.
pub const MAX_TELEMETRY: usize = 128;

/// One cache-line-sized, 64-byte-aligned chunk. The arena is a `Vec` of these
/// so every slot starts on a cache-line (and AVX-512 register) boundary.
#[derive(Clone, Copy)]
#[repr(align(64))]
struct AlignedChunk(#[allow(dead_code)] [f32; 16]); // only ever read through the f32 cast

/// One contiguous pool of mono audio slots. Nodes never own buffers; the
/// schedule hands each node slot indices into this.
pub struct Arena {
    chunks: Vec<AlignedChunk>,
    slot_frames: usize,
    chunks_per_slot: usize,
}

impl Arena {
    fn new(num_slots: usize, slot_frames: usize) -> Self {
        // Round frames up to whole chunks so each slot stays aligned.
        let chunks_per_slot = slot_frames.div_ceil(16);
        Self {
            chunks: vec![AlignedChunk([0.0; 16]); num_slots * chunks_per_slot],
            slot_frames,
            chunks_per_slot,
        }
    }

    /// The slot as a plain `&mut [f32]`. Constant-time, no allocation.
    #[cfg(test)]
    fn slot_mut(&mut self, slot: SlotId) -> &mut [f32] {
        let start = slot.0 * self.chunks_per_slot;
        let chunks = &mut self.chunks[start..start + self.chunks_per_slot];
        // SAFETY: AlignedChunk is repr(align(64)) over [f32; 16]; reinterpreting
        // a contiguous run of them as f32s is layout-compatible.
        let floats = unsafe {
            std::slice::from_raw_parts_mut(chunks.as_mut_ptr().cast::<f32>(), chunks.len() * 16)
        };
        &mut floats[..self.slot_frames]
    }

    /// Raw pointer to a slot's samples, for the gather in `Schedule::run`.
    fn slot_ptr(&mut self, slot: SlotId) -> *mut f32 {
        let start = slot.0 * self.chunks_per_slot;
        // SAFETY of the cast as in slot_mut; caller upholds aliasing rules.
        unsafe { self.chunks.as_mut_ptr().add(start).cast::<f32>() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotId(pub usize);

/// Control smoothing for the saturator's drive, bias, mix and trim, in ms.
/// The filter's figure, for the filter's reason — this is the declick
/// layer the kernel contract says the caller wires in front, and two
/// devices in one rack smoothing at different speeds is a difference you
/// would hear without being able to name.
const SAT_SMOOTH_MS: f32 = 15.0;

/// How many ORIGINAL-rate samples one setting of the curve covers.
///
/// The filter re-derives coefficients every `FILTER_COEFF_INTERVAL`
/// samples so a performed sweep is stepless; this is the same trick for
/// the same reason, one size finer. It has to be finer: a filter's
/// coefficient step changes a slope, while a drive step changes a GAIN,
/// and the ear finds a gain step first. At 48 kHz this is a third of a
/// millisecond, and `configure()` is a handful of clamps — the walk costs
/// less than the shaping does.
const SAT_CHUNK: usize = 16;

/// Control smoothing for the lo-fi blend and trim, in ms. The
/// saturator's figure, for the saturator's reason — these are the two
/// rows a wire can move continuously, and the rate and the word length
/// deliberately have no smoother at all.
const LOFI_SMOOTH_MS: f32 = 15.0;

/// How many samples of the lo-fi blend share one walk of the control
/// ramps. Small enough that a moving mix is continuous, large enough
/// that the two stack buffers stay cheap.
const LOFI_CHUNK: usize = 16;

/// The Q every phaser section runs at.
///
/// Broad. `params::phaser` says why it is not a knob: a narrow Q turns
/// the notches into a ringing pitch that fights the sweep instead of
/// riding it, and the disperser next door is the device for aiming a
/// narrow one at a harmonic.
const PHASER_Q: f32 = 0.7;

/// Control smoothing for the phaser's blend, in ms, and how many samples
/// share one step of the sweep. The lo-fi's figures for the blend; the
/// chunk is what makes the corner control-rate rather than per-sample.
const PHASER_SMOOTH_MS: f32 = 15.0;
const PHASER_CHUNK: usize = 32;

/// Control smoothing for the sheen's blend and trim, in ms, and how many
/// samples share one walk of those ramps. The lo-fi's figures, for the
/// lo-fi's reasons — and, like the lo-fi, the two knobs that reconfigure
/// the kernel deliberately have no smoother at all.
const SHEEN_SMOOTH_MS: f32 = 15.0;
const SHEEN_CHUNK: usize = 16;

/// The kernel chain of one [`Node::Sat`], boxed so the Node enum stays
/// lean. Compile builds it in the green zone; the callback only calls
/// configure/process/reset on it.
///
/// # Why the curve is shared and the rest is per channel
///
/// A [`Waveshaper`](crate::dsp::shaper::Waveshaper) is STATELESS — its
/// own docs say so, and that is the whole reason it needs no lane-major
/// twin — so one curve serves both channels and there is nothing to keep
/// in step. The half-band's polyphase history and the DC blocker's
/// one-pole state ARE memory, and memory shared between two channels is
/// the two channels leaking into each other.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct SatCore {
    shaper: crate::dsp::shaper::Waveshaper,
    oversampler: [crate::dsp::shaper::Oversampler2x; 2],
    dc: [crate::dsp::filters::DcBlocker; 2],
}

/// Control smoothing for the echo's feedback, tone, drive, wow, spread
/// and mix, in ms. The filter's figure, for the filter's reason.
const ECHO_SMOOTH_MS: f32 = 15.0;

/// Control smoothing for the echo TIME, in ms — much slower than the
/// rest, and audibly so on purpose.
///
/// A delay whose time moves does not crossfade, it GLIDES: the read
/// pointer walks to its new distance and everything in the buffer bends
/// pitch on the way, which is the sound of a tape machine changing
/// speed. That is the effect people reach a delay's time knob for, so the
/// glide is long enough to hear. Fifteen milliseconds would be a
/// perfectly clean, perfectly characterless jump.
const ECHO_GLIDE_MS: f32 = 120.0;

/// How far the wow LFO can pull the echo time, as a fraction, at full
/// depth.
///
/// One percent. Tape wow is a pitch wobble of a fraction of a percent and
/// flutter less again; at ten percent it stops being a machine and starts
/// being a chorus pedal. The knob spends its whole travel inside the
/// range where the answer is "warmth" rather than "effect".
const ECHO_WOW_MAX: f32 = 0.01;

/// How fast that wobble is, in Hz, and how far apart the two channels
/// run it.
///
/// Slow, and NOT the same phase on both sides: two channels wobbling
/// together is a mono pitch wobble, while a quarter turn apart is the
/// drifting stereo image a pair of tape machines has. The rate is fixed
/// rather than exposed — a wow-rate knob is a control nobody sets twice,
/// and the depth is the part that matters.
const ECHO_WOW_HZ: f32 = 0.7;
const ECHO_WOW_SPREAD_TURNS: f32 = 0.25;

/// What a full drive knob hands the kernel's loop saturator.
///
/// The kernel's `drive` is the tanh gain above unity, and it has no
/// ceiling of its own because the curve cannot run away. Twelve is where
/// a repeat has clearly been through something and is not yet a fuzz
/// pedal — the top of the useful range, so the knob spends its travel
/// inside it.
const ECHO_DRIVE_MAX: f32 = 12.0;

/// The kernel chain of one [`Node::Echo`], boxed so the Node enum stays
/// lean.
///
/// Two independent echoes, because the two channels must not share a
/// history — and two independent wobbles, because they must not share a
/// phase either.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct EchoCore {
    line: [crate::dsp::delay::FeedbackDelay; 2],
    wow: [crate::dsp::lfo::Lfo; 2],
}

/// The fixed menu of workers. An enum, not a trait object: dispatch is one
/// predictable branch instead of a vtable pointer chase per node.
// Seq is much larger than Silence, and clippy wants it boxed. Deliberately
// not: nodes are walked in the hot loop, and a Box is exactly the pointer
// chase the enum-not-dyn design exists to avoid. Node counts are small; the
// per-element padding is cheap, an L1 miss per node is not.
#[allow(clippy::large_enum_variant)]
pub enum Node {
    /// Writes silence.
    Silence,
    /// A sine test tone. Targets arrive via ParamChange (0 = freq, 1 = amp);
    /// amp ramps linearly across each block so changes can never click.
    Sine {
        phase: f32,
        freq: f32,
        amp: f32,
        target_freq: f32,
        target_amp: f32,
        sample_rate: f32,
    },
    /// One channel of the device input (the mic), as a graph citizen.
    Input { channel: u32 },
    /// Sums its wired inputs, then applies a ramped master gain
    /// (ParamChange 0 = gain). Stereo out; mono inputs are centered.
    Mixer { gain: f32, target_gain: f32 },
    /// One calibrated console path. Inputs sum stereo, then each side runs
    /// its own identity-stable tolerance/noise state. Free-running within a
    /// graph; reset on discontinuity so a repeated render starts identically.
    DeskPath {
        left: crate::dsp::desk::DeskPath,
        right: crate::dsp::desk::DeskPath,
    },
    /// Directional, frequency-shaped coupling from one physical channel into
    /// its neighbour's bus. Kept as an ordinary feed-forward graph node so
    /// the analog personality never introduces feedback.
    DeskBleed {
        left: crate::dsp::desk::DeskBleed,
        right: crate::dsp::desk::DeskBleed,
    },
    /// A SEND: the same sum and ramped gain as a mixer, but it listens
    /// for ONE named parameter of the device that owns it, as a
    /// percentage. It exists so a channel can be tapped into a return
    /// without the tap being a device of its own — the amount lives on
    /// the channel's OUT section, where the hand set it, and the letter
    /// that carries a turn reaches this node as well as that section's.
    Send {
        gain: f32,
        target_gain: f32,
        param: u32,
    },
    /// The per-track output stage: constant-power pan (first input's mono
    /// signal -> stereo) and the track's fader level. Both ramped.
    /// ParamChange 0 = pan, 1 = gain.
    Pan {
        pan: f32,
        target_pan: f32,
        gain: f32,
        target_gain: f32,
        /// A compiled Song lane owns this level. Its target is the exact
        /// endpoint of the current deterministic control segment, so the
        /// ramp must land there instead of continuing across the device
        /// block like an ordinary fader letter.
        gain_is_timeline_automated: bool,
    },
    /// Streams an audio file from disk (creek: decode on its own IO thread,
    /// RT-safe reads here). Timeline-locked: the green-side compile guarantees
    /// a stream at the device rate, converting a mismatched WAV into the
    /// importer's deterministic cache before this node exists. Multichannel
    /// files preserve their first two channels. On read error the stream is
    /// FLAGGED dead, never dropped — a drop
    /// here would join an IO thread inside the callback; the schedule swap
    /// disposes of it on the UI thread like everything else.
    AudioClip {
        stream: Option<ReadDiskStream<SymphoniaDecoder>>,
        /// Short looping files are resident so sub-block loops never issue
        /// hundreds of creek seek messages from one callback. Shared Arc
        /// storage comes from the sampler material cache; long takes keep
        /// streaming and never consume arrangement-sized RAM.
        resident: Option<crate::audio::material::Material>,
        file_frames: u64,
        /// Timeline sample at which this clip becomes audible. Compiled from
        /// musical time; runtime and callback state never store beats here.
        timeline_start: u64,
        /// Audible placement length on the timeline in device frames.
        timeline_frames: u64,
        /// First source-file frame and length of the playable source region.
        source_start: u64,
        source_frames: u64,
        loop_clip: bool,
        /// The ABSOLUTE file frame a wrap returns to. Equal to
        /// `source_start` when the clip has no brace of its own, which is
        /// what makes the plain case cost nothing.
        loop_from: u64,
        gain: f32,
        target_gain: f32,
        /// A gain ramp at each end of the clip's timeline span, in
        /// frames. Letters move them, so dragging a fade handle is heard
        /// while it is dragged rather than when the schedule next swaps.
        fade_in: u64,
        fade_out: u64,
        /// The shape each ramp follows. A rational curve, one multiply
        /// and one divide per sample — see `params::clip::Curve` for why
        /// it is not a `powf`.
        fade_in_curve: crate::params::clip::Curve,
        fade_out_curve: crate::params::clip::Curve,
        /// The clip's gain envelope: `(frame, linear gain)`, sorted,
        /// COMPILED — allocated green-side and read-only here, exactly
        /// as `Node::Seq`'s event list is. Sequencing contract rule 4.
        envelope: Vec<(u64, f32)>,
        /// Where in `envelope` the last sample looked. Amortised O(1)
        /// while the transport runs forward; re-found by binary search on
        /// a discontinuity, which is bounded `log n` and is the only
        /// search on this path.
        envelope_cursor: usize,
        failed: bool,
        /// File frame the NEXT read will produce, mirrored here because a
        /// creek seek is asynchronous.
        next_frame: u64,
    },
    /// Pattern sequencer + 8-voice synth. Timeline-locked: events are in
    /// beats, the cursor derives from ctx.beat, voices cut on discontinuity
    /// (the all-sound-off contract), and stop releases everything. The event
    /// list is baked at compile — immutable in the red zone.
    /// Plugin delay compensation. A pure wire that arrives late: an
    /// integer-sample delay per channel, so an impulse comes back
    /// bit-identical N samples later.
    ///
    /// **UNREVIEWED RED ZONE.** This arm has had only the mechanical
    /// checks — no allocation or panic path in the arm itself, buffers
    /// written in full on every path (including `process_exact`'s fail-open
    /// branch, which leaves the dry signal rather than returning early),
    /// and `a_compensated_graph_does_not_allocate` covering both the steady
    /// and the discontinuity paths under `assert_no_alloc`. It has NOT had
    /// the adversarial pass `AGENTS.md` requires before a human reads a
    /// red-zone diff. Run `/rt-review` over it before trusting it.
    ///
    /// Free-running — it holds signal history, not timeline position — but
    /// it DOES cut on discontinuity, because the samples it is holding
    /// belong to a position the transport has left.
    Delay {
        left: crate::dsp::delay::DelayLine,
        right: crate::dsp::delay::DelayLine,
        /// The rings. Two separate buffers: one line per channel, because a
        /// shared ring would interleave the channels' histories.
        left_buf: Vec<f32>,
        right_buf: Vec<f32>,
    },
    /// A live one-input, stereo-f32 CLAP effect. The processor and all pointer
    /// tables are prepared off-thread; basedrop returns them to the instance's
    /// dedicated control thread when this schedule retires.
    Clap {
        processor: basedrop::Owned<crate::clap_host::ClapGraphProcessor>,
    },
    Seq {
        events: Vec<SeqEvent>,
        /// Where the pattern is and how it maps onto the timeline.
        clock: PatternClock,
        /// The instrument the clock plays.
        voices: VoiceBank,
        /// Post-sum gain, ramped per segment like Sine's amp (ParamChange
        /// 0). Attack/release arrive as ms (ParamChange 1/2) and are
        /// converted to per-sample rates by the voice bank on arrival.
        gain: f32,
        target_gain: f32,
    },
    /// The struck-resonator synth. Same clock as every other instrument;
    /// a different half of synthesis.
    Tine {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::tine::TineVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The drum one-shot sampler. Its file was loaded when the node
    /// was built.
    Brick {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::brick::BrickVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The four-operator FM synth.
    Quad {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::quad::QuadVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The house chord synth. One key, one chord.
    Stab {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::stab::StabVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The bounced sine. Its take was rendered when the node was built;
    /// the voices only read it.
    Scomp {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::scomp::ScompVoices>,
        gain: f32,
        target_gain: f32,
    },
    Poly {
        events: Vec<SeqEvent>,
        /// Where the pattern is — the SAME clock `Seq` uses.
        clock: PatternClock,
        /// The instrument the clock plays.
        voices: Box<crate::audio::poly::PolyVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The wavetable synth. Same clock, same shape — the instrument is
    /// `crate::audio::loom`. Stereo out, because unison spread is a
    /// stereo idea.
    Loom {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::loom::LoomVoices>,
        gain: f32,
        target_gain: f32,
    },
    /// The pad synth. Same clock as `Seq` and `Poly`, an instrument
    /// built for one job instead of for all of them.
    Haze {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::haze::Haze>,
        gain: f32,
        target_gain: f32,
    },
    /// The kick drum synth. Same clock as `Seq` and `Poly`, a one-shot
    /// drum voice instead of a bank.
    Kick {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::kick::KickVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// The acid mono — one voice, and the instrument is that it has one.
    ///
    /// Timeline-locked, sharing `PatternClock` with `Seq`/`Poly`/`Kick`,
    /// and it CUTS on discontinuity: a note sounding here belongs to the
    /// position the transport has left.
    ///
    /// Mono out. The machine is mono and the track's pan stage is what
    /// places it, the same argument `Node::Kick` makes.
    Acid {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::acid::AcidVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// The sampler: a file in RAM, eight voices reading it, and the
    /// machine's output stage after the sum.
    ///
    /// Timeline-locked, sharing `PatternClock` with `Seq`/`Poly`/`Kick`,
    /// and it CUTS on discontinuity — a sounding note belongs to the
    /// position the transport has left.
    ///
    /// STEREO, so the walk reads the bank's right channel back once the
    /// segment is done, exactly as `Poly` does.
    ///
    /// The material is an `Arc<Vec<f32>>` loaded green-side at compile.
    /// It is immutable here and is retired through the schedule-disposal
    /// path like every other compiled allocation — the same status
    /// `Seq`'s event list has.
    Sampler {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::sampler::SamplerVoices>,
        /// The output stage. One per device, applied after the voices
        /// sum, because that is what an output stage IS — see
        /// `crate::audio::preamp`.
        preamp: crate::audio::preamp::Preamp,
        gain: f32,
        target_gain: f32,
    },
    /// The snare drum synth. The kick's shape, a different drum.
    Snare {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::snare::SnareVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// The tom synth. The kick's shape, a different drum.
    Tom {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::tom::TomVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// The 808 hi-hat. The kick's shape, and the note picks open or
    /// closed rather than a pitch.
    Hat {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::hat::HatVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// The hand clap. The kick's shape, a different drum.
    Handclap {
        events: Vec<SeqEvent>,
        clock: PatternClock,
        voices: Box<crate::audio::handclap::HandclapVoice>,
        gain: f32,
        target_gain: f32,
    },
    /// Modulato. Everything it owns is inside the effect; this arm is a
    /// wire and a discontinuity cut.
    Modulato {
        core: Box<crate::audio::modulato::Modulato>,
    },
    /// Reverb: the first effect. Sums its inputs to mono, runs the
    /// `dsp::fdn` network over them, and crossfades wet against dry.
    /// Free-running: its tail is wall-history, not timeline position — but
    /// it CUTS on discontinuity, because a seek must not drag the old
    /// room's tail into the new position.
    ///
    /// The delay memory is a plain Vec owned here and allocated at compile
    /// (green zone). The kernel never touches its length.
    Reverb {
        core: crate::dsp::fdn::Fdn,
        buffers: Vec<f32>,
        /// The pre-delay, and its own line memory. Node-side rather than
        /// in the kernel: the network's job is to be a room, and how long
        /// you wait before hearing it is a placement decision.
        predelay: crate::dsp::delay::DelayLine,
        predelay_buf: Vec<f32>,
        /// The pre-delayed input, kept apart because the kernel reads its
        /// input and writes its outputs and the three may not overlap.
        fed: Vec<f32>,
        /// Wet scratch, one block long per side — the kernel writes pure
        /// wet and this node does the mixing.
        wet_l: Vec<f32>,
        wet_r: Vec<f32>,
        /// A highpass on the WET only, so the tail stops carrying the low
        /// end of everything fed to it. On the wet alone, because cutting
        /// the dry would be an EQ the user did not ask for.
        low_cut_l: crate::dsp::filters::OnePole,
        low_cut_r: crate::dsp::filters::OnePole,
        mix: f32,
        target_mix: f32,
        width: f32,
        sample_rate: f32,
    },
    /// The character filter. Cutoff, resonance, slope and a drive stage
    /// over the `dsp::filters` family — see `crate::audio::filter` for
    /// the signal path, the character voicings and why it is stereo.
    ///
    /// STEREO, and that is a change from the version that had no card:
    /// this lives in a track chain where every other effect is stereo,
    /// and a mono node in the middle of one folds everything downstream
    /// of it. Being stereo is also what makes `spread` possible.
    ///
    /// Free-running; cuts every history on discontinuity. Its latency —
    /// the drive stage's half-band round trip — is CONSTANT and reported
    /// through `spec_latency`, so plugin delay compensation absorbs it
    /// and no knob on the device can slide the track in time.
    Filter {
        core: Box<crate::audio::filter::FilterCore>,
        /// Four 2x lanes of block scratch, compile-owned.
        scratch: Vec<f32>,
    },
    /// The character limiter: loudness, a ceiling, and a voice.
    ///
    /// NOT a transparent one — see `crate::audio::limiter` for the whole
    /// argument and the signal path. Stereo, and LINKED: one detector
    /// fed both channels, because a level difference between left and
    /// right is the stereo image and two independent limiters make it
    /// wander on every peak.
    ///
    /// Free-running; cuts every history on discontinuity. Its latency —
    /// the lookahead plus two half-band round trips — is CONSTANT and
    /// reported through `spec_latency`, so plugin delay compensation
    /// absorbs it and no knob on the device can slide the track in time.
    Limiter {
        core: Box<crate::audio::limiter::LimiterCore>,
        /// Six 2x lanes of block scratch, compile-owned.
        scratch: Vec<f32>,
        /// The limiter's three delay lines: the detector's key, and one
        /// per channel. Compile-owned and zeroed; the kernel never
        /// touches their length.
        key: Vec<f32>,
        line_l: Vec<f32>,
        line_r: Vec<f32>,
    },
    /// Saturator: a transfer curve, oversampled. The second kernel-backed
    /// effect, and the one that is nothing BUT the drive stage the filter
    /// carries as a seasoning.
    ///
    /// Five shapes from `dsp::shaper` — hard clip, soft clip, cubic, fold,
    /// crush — run at 2x through the same half-band round trip, then a DC
    /// blocker, then an output trim. Ids and ranges are
    /// `crate::params::sat`'s, in the kernel's own units, so the curve the
    /// widget draws and the curve the audio runs are chosen by the same
    /// number.
    ///
    /// STEREO, unlike the filter: this device's natural place is behind
    /// the poly synth, and a mono-summing effect there would fold the
    /// unison spread the synth exists to make. The curve is shared (it is
    /// stateless); the history is not — see [`SatCore`].
    ///
    /// # The oversampler is permanently in the path
    ///
    /// The filter's rule, for the filter's reason: a latency that moved
    /// with a knob would slide the track in time as the user turned it.
    /// In hard clip at drive 1 the curve is the identity inside the
    /// rails, so the device is then a wire delayed by exactly
    /// `Oversampler2x::latency()` — a constant `spec_latency` reports and
    /// `GraphSpec::compensate` absorbs. That case is the one the round
    /// trip is measured against; the other shapes are not transparent at
    /// unity drive and are not meant to be.
    ///
    /// The dry/wet blend is owned HERE rather than by the shaper's own
    /// `mix`, for the reason the filter documents: `shape()` rails its
    /// output to ±1 even at mix 0, and the clean path must keep its float
    /// headroom. The shaper runs pure on a copy at 2x and this node
    /// crossfades.
    ///
    /// **UNREVIEWED RED ZONE.** Mechanical checks only so far — no
    /// allocation or panic path in the arm, every output buffer written
    /// on every path including the fail-open scratch branch. It has NOT
    /// had the adversarial pass `AGENTS.md` requires. Run `/rt-review`
    /// over it before a human reads the diff.
    ///
    /// Free-running — it holds signal history, not timeline position —
    /// but it CUTS on discontinuity, because the half-band's history
    /// belongs to a position the transport has left.
    Sat {
        core: Box<SatCore>,
        /// Three 2x lanes — left, right, and the shaped copy the node
        /// blends against — so `6 * block` floats, compile-owned.
        scratch2x: Vec<f32>,
        mode: u32,
        /// Letters land here; the switch happens at the next segment edge.
        pending_mode: u32,
        drive: crate::dsp::ramps::Smoother,
        bias: crate::dsp::ramps::Smoother,
        mix: crate::dsp::ramps::Smoother,
        /// Output trim. Named `trim` and not `out` because the process
        /// arm already has an `out`, and a shadowed buffer is a bug
        /// waiting for a careless edit; the wire id is still
        /// `params::sat::OUT`.
        trim: crate::dsp::ramps::Smoother,
        /// Shadow targets, because a discontinuity snaps the smoothers to
        /// their destination and `Smoother` does not expose its own.
        drive_target: f32,
        bias_target: f32,
        mix_target: f32,
        trim_target: f32,
    },
    /// The lo-fi converter — sample-rate reduction and bit reduction, in
    /// the order and with the filtering an early hardware sampler had.
    /// `dsp::lofi::Downsampler` owns all three; this node owns the blend,
    /// the trim and the wiring.
    ///
    /// Free-running — it holds signal history, not timeline position —
    /// but it CUTS on discontinuity, because the tracking filter's poles
    /// and the hold's latched sample belong to a position the transport
    /// has left.
    ///
    /// ONE CONVERTER PER CHANNEL, not one shared. `SatCore`'s doc makes
    /// the same argument about its half-band: the two poles and the held
    /// sample are memory, and memory shared between two channels is the
    /// two channels leaking into each other.
    Lofi {
        core: [crate::dsp::lofi::Downsampler; 2],
        /// The dry copy the blend runs against, because the kernel works
        /// in place — `2 * block` floats, compile-owned.
        dry: Vec<f32>,
        /// Converter clock and word length, as last set. Held so a
        /// discontinuity has something to restate and so the node can be
        /// read back; the kernel is the one that acts on them.
        rate: f32,
        bits: f32,
        mix: crate::dsp::ramps::Smoother,
        /// Output trim. Named `trim` and not `out` for the reason
        /// `Node::Sat`'s is: the process arm already has an `out`, and a
        /// shadowed buffer is a bug waiting for a careless edit. The wire
        /// id is still `params::lofi::OUT`.
        trim: crate::dsp::ramps::Smoother,
        /// Shadow targets, because a discontinuity snaps the smoothers to
        /// their destination and `Smoother` does not expose its own.
        mix_target: f32,
        trim_target: f32,
    },
    /// The sheen — `dsp::dynamics::SlewBrighten`, as a device.
    ///
    /// A high-frequency lift whose size is driven by how fast the signal
    /// is MOVING, so it arrives with a transient and leaves with it. The
    /// kernel owns the edge band, the slew follower and the saturating
    /// ratio that keeps it from ever running away; this node owns the
    /// blend, the trim and the wiring.
    ///
    /// Free-running, and it CUTS on discontinuity: the follower's
    /// envelope and the edge band's pole belong to a position the
    /// transport has left.
    ///
    /// ONE BRIGHTENER PER CHANNEL, for the reason `Node::Lofi` and
    /// `SatCore` both give — the pole, the previous sample and the
    /// envelope are memory, and memory shared between two channels is the
    /// two channels leaking into each other.
    /// The disperser — `dsp::filters::Disperser`, as a device.
    ///
    /// A chain of allpasses: unity gain at every frequency, and nothing
    /// but a frequency-dependent delay. A transient stops arriving all at
    /// once and becomes a descending chirp.
    ///
    /// THE SIMPLEST NODE HERE, and the reason is the device's own: it has
    /// no mix and no trim, because its promise is a flat magnitude and
    /// summing a phase-shifted copy with the dry would comb it into a
    /// phaser. No blend means no dry buffer and no smoothers — the node
    /// is the kernel, two channels of it, and the wiring.
    ///
    /// BOXED, for the reason `SatCore` is: thirty-two SVF sections per
    /// channel is a couple of kilobytes, and `Node` is sized by its
    /// largest variant.
    ///
    /// Free-running, and it CUTS on discontinuity — sixty-four
    /// integrators' worth of history belongs to a position the transport
    /// has left.
    /// The tilt — `dsp::filters::Tilt`, as a device.
    ///
    /// A first-order see-saw: highs up and lows down by the same amount,
    /// unity exactly at the pivot.
    ///
    /// As small as `Node::Disperser` and for the same reason — no mix and
    /// no trim means no dry buffer and no smoothers. Not boxed, though:
    /// a `Tilt` is a one-pole and two gains, so two of them are forty
    /// bytes and nowhere near what sizes the `Node` enum.
    ///
    /// Free-running, and it CUTS on discontinuity: the pole's state
    /// belongs to a position the transport has left.
    ///
    /// One filter per channel, for the reason every device here gives —
    /// a shared pole is the two channels leaking into each other.
    /// The phaser — the disperser's chain, swept and blended.
    ///
    /// Same kernel as `Node::Disperser`; what makes it a different device
    /// is the two things that one refuses. The blend turns the flat
    /// allpass into a comb, and the LFO walks the corner so the notches
    /// travel.
    ///
    /// FREE-RUNNING, and pointedly so: the sweep rides the sample clock,
    /// not the transport, the way `Node::Modulato`'s does. On a
    /// discontinuity the filters reset — sixty-four integrators' worth of
    /// history belongs to a position the transport has left — but THE LFO
    /// DOES NOT. Its phase belongs to the device, not to the timeline,
    /// and snapping it on every seek would be a click the user never
    /// asked for.
    ///
    /// Boxed for the reason `Node::Disperser` is.
    Phaser {
        core: Box<[crate::dsp::filters::Disperser; 2]>,
        lfo: crate::dsp::lfo::Lfo,
        /// The dry copy the blend runs against, because the kernel works
        /// in place — `2 * block` floats, compile-owned.
        dry: Vec<f32>,
        sample_rate: f32,
        stages: u32,
        centre_hz: f32,
        depth_oct: f32,
        mix: crate::dsp::ramps::Smoother,
        /// Shadow target, because a discontinuity snaps the smoother to
        /// its destination and `Smoother` does not expose its own.
        mix_target: f32,
    },
    Tilt {
        core: [crate::dsp::filters::Tilt; 2],
        /// Kept because `Tilt::prepare` takes the rate, the pivot and the
        /// amount together, so moving either knob restates all three.
        sample_rate: f32,
        pivot_hz: f32,
        tilt_db: f32,
    },
    Disperser {
        core: Box<[crate::dsp::filters::Disperser; 2]>,
        /// Kept because `Disperser::prepare` takes the rate, the corner,
        /// the Q and the count together, so moving ANY of the three knobs
        /// has to restate all four.
        sample_rate: f32,
        freq_hz: f32,
        pinch: f32,
        stages: u32,
    },
    Sheen {
        core: [crate::dsp::dynamics::SlewBrighten; 2],
        /// The dry copy the blend runs against, because the kernel works
        /// in place — `2 * block` floats, compile-owned.
        dry: Vec<f32>,
        /// Kept because `SlewBrighten::prepare` takes the rate, the
        /// corner and the amount together, so moving EITHER of the two
        /// knobs has to restate all three.
        sample_rate: f32,
        amount: f32,
        edge_hz: f32,
        mix: crate::dsp::ramps::Smoother,
        /// Output trim, named `trim` for the reason `Node::Sat`'s is.
        /// The wire id is still `params::sheen::OUT`.
        trim: crate::dsp::ramps::Smoother,
        /// Shadow targets, because a discontinuity snaps the smoothers to
        /// their destination and `Smoother` does not expose its own.
        mix_target: f32,
        trim_target: f32,
    },
    /// Analogue delay — the musical one, as opposed to [`Node::Delay`],
    /// which is compensation wiring.
    ///
    /// A stereo pair of `dsp::delay::FeedbackDelay` echoes, each with
    /// damping, saturation and a wow wobble INSIDE its feedback loop, so
    /// every repeat is darker, dirtier and further out of tune than the
    /// one before it. That compounding is the whole difference between an
    /// analogue delay and a delay with a filter after it.
    ///
    /// Stereo, and `spread` walks the right channel's time away from the
    /// left's — a stereo picture out of one control, mono-compatible at
    /// zero.
    ///
    /// TIME is either a division of the transport or a number of
    /// milliseconds, decided by `sync`; `params::echo::time_samples` is
    /// the only place that knows which, and it converts against the
    /// tempo of the SEGMENT being rendered, so a tempo ramp drags the
    /// repeats along with it.
    ///
    /// Time changes GLIDE rather than jump (`ECHO_GLIDE_MS`), which is
    /// what bends the pitch of everything already in the buffer — the
    /// sound a delay's time knob is reached for.
    ///
    /// **UNREVIEWED RED ZONE.** Mechanical checks only: no allocation or
    /// panic path in the arm, every output buffer written on every path
    /// including the fail-open branches, and a no-alloc test over the
    /// steady and discontinuity paths. It has NOT had the adversarial
    /// pass `AGENTS.md` requires. Run `/rt-review` before a human reads
    /// it.
    ///
    /// Free-running — it holds signal history, not timeline position —
    /// but it CUTS on discontinuity: the repeats in the buffer belong to
    /// a position the transport has left.
    Echo {
        core: Box<EchoCore>,
        /// One ring per channel, each exactly the length the kernel was
        /// prepared for. Separate buffers, because a shared ring would
        /// interleave the two channels' histories.
        rings: [Vec<f32>; 2],
        /// Four block-long lanes: the base time, the modulated time, the
        /// wobble, and the wet copy the node blends against.
        scratch: Vec<f32>,
        sync: u32,
        /// Letters land here; the switch happens at the next segment
        /// edge, where the glide picks it up like any other time change.
        pending_sync: u32,
        /// The echo time IN SAMPLES, glided. What the smoother chases
        /// is recomputed every segment, because a synced time depends on
        /// the tempo the segment is playing at.
        time: crate::dsp::ramps::Smoother,
        /// The free-running time as the user set it. Read only when
        /// `sync` is 0 — but kept always, so switching back out of sync
        /// returns to the time that was there.
        time_ms: f32,
        feedback: crate::dsp::ramps::Smoother,
        tone: crate::dsp::ramps::Smoother,
        drive: crate::dsp::ramps::Smoother,
        wow: crate::dsp::ramps::Smoother,
        spread: crate::dsp::ramps::Smoother,
        mix: crate::dsp::ramps::Smoother,
        /// Whether this echo is an AUX rather than an insert: fed by a
        /// tap off its track instead of standing in the signal path.
        ///
        /// Compile decides it and the node never changes its mind —
        /// which side of the split an echo is on is a shape, and a shape
        /// is a recompile. It is here because it is the one thing
        /// `in_gain` cannot express: an insert must ignore the send
        /// entirely, so that automating the send to zero silences an aux
        /// and CANNOT mute an insert.
        aux: bool,
        /// The send level as a LINEAR gain, and its letter target. A
        /// level, so it ramps across the block the way the fader in
        /// [`Node::Pan`] does rather than per segment. Unity on an
        /// insert, where nothing reads it.
        in_gain: f32,
        in_target: f32,
        /// Shadow targets, because a discontinuity snaps the smoothers
        /// to their destination and `Smoother` does not expose its own.
        /// The time's is in SAMPLES and is derived per segment, not set
        /// by a letter.
        time_target: f32,
        feedback_target: f32,
        tone_target: f32,
        drive_target: f32,
        wow_target: f32,
        spread_target: f32,
        mix_target: f32,
        sample_rate: f32,
    },
    /// The eight-band equaliser. Every band, every coefficient and the
    /// output trim live in the core; this variant is the handle.
    ///
    /// Stereo in, stereo out — an equaliser that summed to mono would
    /// undo whatever placed the track, and it sits wherever a user drops
    /// it in a chain.
    Eq { core: Box<crate::audio::eq::EqCore> },
    /// The bus compressor. A FEEDBACK topology, so the whole loop lives
    /// in the core and is closed per sample — see `crate::audio::glue`.
    ///
    /// Stereo in, stereo out, and LINKED: one detector hears both sides,
    /// because a compressor that ducked each channel on its own would
    /// walk the image about whenever the mix leaned one way.
    Glue {
        core: Box<crate::audio::glue::GlueCore>,
    },
    /// The surgical compressor — see `crate::audio::clamp`. A FEEDFORWARD
    /// peak/RMS design with a fast floor, where the glue is a feedback
    /// bus box: this one is meant to catch a transient, not to breathe
    /// with a mix.
    ///
    /// Stereo in, stereo out, and LINKED for the same reason the glue is:
    /// one detector over both sides, so the image does not walk.
    Clamp {
        core: Box<crate::audio::clamp::Clamp>,
    },
    /// The transient shaper — see `crate::audio::flint`. Stereo in,
    /// stereo out, and linked for the same reason the clamp is: one
    /// detector across both sides, so a centred hit does not lean.
    Flint {
        core: Box<crate::audio::flint::Flint>,
    },
    /// Three-band dynamics — see `crate::audio::prism`. A Linkwitz–Riley
    /// split, three of the same detector/computer/ballistics trio, and a
    /// saturation on each band whose amount is that band's own gain
    /// movement.
    ///
    /// Stereo in, stereo out, and each band's detector hears both sides
    /// — three independent stereo compressors would walk the image three
    /// different ways at once.
    Prism {
        core: Box<crate::audio::prism::Prism>,
    },
    /// Downward expansion — see `crate::audio::gate`. The glue's three
    /// kernels with the computer's mode flipped, plus the two things a
    /// gate must do differently: hear its own INPUT, and cross its
    /// attack and release on the way to the ballistics.
    ///
    /// Boxed for the reason the glue's core is: `Node` is sized by its
    /// largest variant.
    ///
    /// Free-running — it holds a detector's history and a gain, not a
    /// timeline position — and it REOPENS on discontinuity, exactly as
    /// the glue arrives un-compressed.
    Gate {
        core: Box<crate::audio::gate::GateCore>,
    },
    /// A mini channel strip — see `crate::audio::strip`. Two shelves, an
    /// optional warm pair, and `audio::preamp::Preamp` whole, in that
    /// order: everything is pre-drive, which is the device's character
    /// rather than an accident of wiring.
    ///
    /// Boxed for the reason the glue's core is: `Node` is sized by its
    /// largest variant.
    ///
    /// Free-running — it holds filter state and a noise source, not a
    /// timeline position — and it CUTS on discontinuity.
    Strip {
        core: Box<crate::audio::strip::StripCore>,
    },
    /// A section of the console — see `crate::audio::console`. One node
    /// kind for every section; the core behind the trait is the kind's.
    /// Free-running, and it cuts on discontinuity.
    Section {
        core: Box<dyn crate::audio::console::SectionCore>,
    },
    /// The spectral resynthesiser — see `crate::audio::resyn`. The first
    /// node in the tree to run an FFT in the callback.
    ///
    /// Boxed, and this one is not a nicety: the frame buffers, the
    /// per-bin state and the output FIFO are a few tens of kilobytes, and
    /// `Node` is sized by its largest variant.
    ///
    /// Free-running, and it CUTS on discontinuity — a frame half-filled
    /// from before the seek would be reconstructed across the join.
    ///
    /// IT HAS REAL LATENCY, which is why `spec_latency` names it: PDC
    /// delays every sibling path to match, and the device's own test
    /// measures the figure rather than trusting it.
    /// The test-signal generator — see `crate::audio::tone`.
    Tone {
        core: Box<crate::audio::tone::ToneCore>,
    },
    /// The ring modulator — see `crate::audio::sigil`. Reflects in
    /// place, adds no latency, and at mix zero is the exact identity.
    Sigil {
        core: Box<crate::audio::sigil::SigilCore>,
    },
    /// The meter — see `crate::audio::gauge`. It measures and passes the
    /// signal on untouched, which is why its arm does not write.
    Gauge {
        core: Box<crate::audio::gauge::GaugeCore>,
    },
    /// The shadow — see `crate::audio::umbra`. One knob, ten stages,
    /// stereo in and out, and a window of latency it reports.
    Umbra {
        core: Box<crate::audio::umbra::UmbraCore>,
    },
    /// The tape looper — see `crate::audio::ferric`. Stereo in, stereo
    /// out, and the only device here that needs the BEAT: its head is
    /// placed on the musical grid rather than after a delay.
    Ferric {
        core: Box<crate::audio::ferric::FerricCore>,
    },
    /// The harmoniser — see `crate::audio::sibyl`. Stereo in, stereo
    /// out, and a whole window of latency it reports.
    Sibyl {
        core: Box<crate::audio::sibyl::SibylCore>,
    },
    Resyn {
        core: Box<crate::audio::resyn::ResynCore>,
    },
    /// Gain, placement and the stereo field — see
    /// `crate::audio::utility`. The device with no tone of its own.
    ///
    /// Stereo in, stereo out, and it must be: width, phase and channel
    /// mode are all statements about two channels, and a mono node
    /// carrying them would be a card of controls that do nothing.
    Utility {
        core: Box<crate::audio::utility::UtilityCore>,
    },
    /// Metronome: a short decaying blip on every integer beat while the
    /// transport rolls. Timeline-locked: fires off an integer beat COUNTER,
    /// not a per-sample float comparison — a crossing that lands exactly on a
    /// segment boundary is a 1-ulp coin flip for stateless floor() tests
    /// (skipped beats), but an integer counter carried across segments cannot
    /// miss. Resyncs on ctx.discontinuity and on backward beat jumps (tempo
    /// drops teleport the derived beat backward).
    Click {
        phase: f32,
        env: f32,
        last_beat: i64,
        sample_rate: f32,
    },
}

/// Everything a node may read besides its wired inputs: the device input and
/// the transport, per segment.
pub struct ProcessCtx<'a> {
    /// Planar device input for the WHOLE block: ch0 samples, then ch1, ...
    pub device_input: &'a [f32],
    pub in_channels: usize,
    /// Frames in the whole device block (the planar stride).
    pub block_frames: usize,
    /// This segment's offset into the block.
    pub offset: usize,
    /// This segment's length; every buffer handed to process() is this long.
    pub len: usize,
    /// Whether the transport is rolling. Gates ADVANCE, not processing —
    /// nodes always run (monitoring, tails); time-driven ones go quiet.
    pub playing: bool,
    /// Timeline position of this segment's first sample, as it will sound at
    /// the output.
    pub position: u64,
    /// Beat of the first sample, derived from position — never accumulated.
    pub beat: f64,
    /// Beats advanced per sample at the current tempo.
    pub beats_per_sample: f64,
    /// This segment does not continue seamlessly from the last playing one
    /// (first play, seek, loop wrap). Contract: nodes holding sounding
    /// voices cut them here. Pause/resume at the same position never sets it.
    pub discontinuity: bool,
}

impl Node {
    /// Output channel count. Compile sizes each node's slots from this.
    fn channels(spec: &NodeSpec) -> u8 {
        match spec {
            NodeSpec::Silence
            | NodeSpec::Sine { .. }
            | NodeSpec::Input { .. }
            | NodeSpec::Click
            | NodeSpec::Reverb { .. }
            | NodeSpec::Kick { .. }
            | NodeSpec::Acid { .. }
            | NodeSpec::Snare { .. }
            | NodeSpec::Tom { .. }
            | NodeSpec::Hat { .. }
            | NodeSpec::Handclap { .. }
            | NodeSpec::Seq { .. } => 1,
            // Stereo: unison spread is a stereo idea, and a node's channel
            // count is fixed by its kind.
            // Stereo for the same reason, one device downstream: a
            // saturator that summed to mono would undo the spread.
            NodeSpec::Haze { .. }
            | NodeSpec::Loom { .. }
            | NodeSpec::Poly { .. }
            | NodeSpec::Tine { .. }
            | NodeSpec::Scomp { .. }
            | NodeSpec::Stab { .. }
            | NodeSpec::Quad { .. }
            | NodeSpec::Brick { .. }
            | NodeSpec::Sampler { .. }
            | NodeSpec::Modulato { .. }
            | NodeSpec::Sat { .. }
            | NodeSpec::Echo { .. }
            | NodeSpec::Eq { .. }
            | NodeSpec::Filter { .. }
            | NodeSpec::Glue { .. }
            | NodeSpec::Clamp { .. }
            | NodeSpec::Flint { .. }
            | NodeSpec::Prism { .. }
            | NodeSpec::Gate { .. }
            | NodeSpec::Strip { .. }
            | NodeSpec::Section { .. }
            | NodeSpec::Resyn { .. }
            | NodeSpec::Sibyl { .. }
            | NodeSpec::Ferric { .. }
            | NodeSpec::Umbra { .. }
            | NodeSpec::Tone { .. }
            | NodeSpec::Sigil { .. }
            | NodeSpec::Gauge { .. }
            | NodeSpec::Limiter { .. }
            | NodeSpec::Utility { .. }
            | NodeSpec::Lofi { .. }
            | NodeSpec::Sheen { .. }
            | NodeSpec::Disperser { .. }
            | NodeSpec::Tilt { .. }
            | NodeSpec::Phaser { .. } => 2,
            NodeSpec::Clap { .. } | NodeSpec::DeskPath { .. } | NodeSpec::DeskBleed { .. } => 2,
            NodeSpec::LatencyBypass { effect } => Self::channels(effect),
            NodeSpec::Delay { channels, .. } => (*channels).clamp(1, 2),
            NodeSpec::Mixer { .. }
            | NodeSpec::Send { .. }
            | NodeSpec::AudioClip { .. }
            | NodeSpec::Pan { .. } => 2,
        }
    }

    /// What this node has to say about itself, if anything.
    ///
    /// Almost nothing does: a node's output level is already metered, and
    /// a node with no opinion about how hard it is working has nothing to
    /// add. A dynamics processor does — see [`Readout`].
    ///
    /// Red zone: a field read, called once per step.
    fn readout(&self) -> Option<Readout> {
        match self {
            Node::Glue { core } => Some(core.readout()),
            Node::Clamp { core } => Some(core.readout()),
            Node::Flint { core } => Some(core.readout()),
            Node::Sibyl { core } => Some(core.readout()),
            Node::Ferric { core } => Some(core.readout()),
            Node::Umbra { core } => Some(core.readout()),
            Node::Gauge { core } => Some(core.readout()),
            Node::Tine { voices, .. } => Some(voices.readout()),
            Node::Scomp { voices, .. } => Some(voices.readout()),
            Node::Stab { voices, .. } => Some(voices.readout()),
            Node::Quad { voices, .. } => Some(voices.readout()),
            Node::Brick { voices, .. } => Some(voices.readout()),
            Node::Prism { core } => Some(core.readout()),
            Node::Gate { core } => Some(core.readout()),
            Node::Section { core } => Some(core.readout()),
            _ => None,
        }
    }

    /// Red zone. Bounded work, no allocation, no panic paths. Mono nodes
    /// write `out.l`; stereo nodes also get `out.r` (compile guarantees it).
    fn process(&mut self, inputs: &[InRef<'_>], out: &mut OutRef<'_>, ctx: &ProcessCtx<'_>) {
        let out_len = out.l.len();
        match self {
            Node::Silence => {
                out.l.fill(0.0);
                if let Some(r) = out.r.as_deref_mut() {
                    r.fill(0.0);
                }
            }

            Node::Sine {
                phase,
                freq,
                amp,
                target_freq,
                target_amp,
                sample_rate,
            } => {
                *freq = *target_freq;
                let step = *freq / *sample_rate;
                // Linear amp ramp across the block: click-free by construction.
                let mut ramp = Ramp::across(*amp, *target_amp, out_len);
                for s in out.l.iter_mut() {
                    *s = (*phase * TAU).sin() * ramp.next();
                    *phase += step;
                    if *phase >= 1.0 {
                        *phase -= 1.0;
                    }
                }
                *amp = *target_amp; // land exactly, no float drift
            }

            Node::Input { channel } => {
                let ch = *channel as usize;
                if ch < ctx.in_channels {
                    let start = ch * ctx.block_frames + ctx.offset;
                    let src = &ctx.device_input[start..start + ctx.len];
                    for (d, s) in out.l.iter_mut().zip(src.iter()) {
                        *d = *s;
                    }
                } else {
                    out.l.fill(0.0); // channel not present on this device
                }
            }

            Node::Send {
                gain, target_gain, ..
            }
            | Node::Mixer { gain, target_gain } => {
                // Stereo sum: stereo inputs go L->L R->R; mono inputs are
                // centered (same signal to both sides).
                let r = out.r.as_deref_mut().unwrap_or(&mut []);
                out.l.fill(0.0);
                r.fill(0.0);
                for input in inputs {
                    for (d, s) in out.l.iter_mut().zip(input.l.iter()) {
                        *d += *s;
                    }
                    let right_src = input.r.unwrap_or(input.l);
                    for (d, s) in r.iter_mut().zip(right_src.iter()) {
                        *d += *s;
                    }
                }
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                for s in out.l.iter_mut() {
                    *s *= ramp.next();
                }
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                for s in r.iter_mut() {
                    *s *= ramp.next();
                }
                *gain = *target_gain; // land exactly, no float drift
            }

            Node::DeskPath { left, right } => {
                let r = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, r);
                if ctx.discontinuity {
                    left.reset_signal();
                    right.reset_signal();
                }
                left.process(out.l);
                right.process(r);
            }

            Node::DeskBleed { left, right } => {
                let r = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, r);
                if ctx.discontinuity {
                    left.reset();
                    right.reset();
                }
                left.process(out.l);
                right.process(r);
            }

            Node::Pan {
                pan,
                target_pan,
                gain,
                target_gain,
                gain_is_timeline_automated,
            } => {
                // Mono uses constant-power placement. Stereo uses a balance
                // law: centre preserves both channels, while either extreme
                // attenuates only the opposite side.
                let input = inputs.first();
                let src = input.map(|input| input.l).unwrap_or(&[]);
                let src_r = input.and_then(|input| input.r);
                let r = out.r.as_deref_mut().unwrap_or(&mut []);
                let mut ramp = Ramp::across(*pan, *target_pan, out_len);
                // An automation target is the endpoint of this deterministic
                // control segment. An ordinary letter, however, is drained
                // once per device block and must keep its block-long glide
                // even when tempo/loop boundaries split that block into a
                // one-frame segment.
                let gain_span = if *gain_is_timeline_automated {
                    out_len
                } else {
                    ctx.block_frames.saturating_sub(ctx.offset).max(out_len)
                };
                let mut level = Ramp::across(*gain, *target_gain, gain_span);
                for i in 0..out_len {
                    let p = ramp.next();
                    let g = level.next();
                    let left = src.get(i).copied().unwrap_or(0.0);
                    if let Some(source_r) = src_r {
                        let right = source_r.get(i).copied().unwrap_or(0.0);
                        let (left_gain, right_gain) = crate::dsp::pan::balance(p);
                        out.l[i] = left * left_gain * g;
                        if let Some(rs) = r.get_mut(i) {
                            *rs = right * right_gain * g;
                        }
                    } else {
                        // A mono source is PLACED, not balanced: the
                        // constant-power law, so it holds its loudness
                        // across the field.
                        let (lg, rg) = crate::dsp::pan::spread(p);
                        out.l[i] = left * lg * g;
                        if let Some(rs) = r.get_mut(i) {
                            *rs = left * rg * g;
                        }
                    }
                }
                // Pan is segment-scoped. Automated gain is too; an ordinary
                // fader carries its intermediate value across transport
                // splits and lands exactly only at the end of the block.
                *pan = *target_pan;
                if *gain_is_timeline_automated || ctx.offset + out_len >= ctx.block_frames {
                    *gain = *target_gain;
                } else {
                    *gain = level.value;
                }
            }

            Node::AudioClip {
                stream,
                resident,
                fade_in,
                fade_out,
                fade_in_curve,
                fade_out_curve,
                envelope: envelope_points,
                envelope_cursor,
                file_frames,
                timeline_start,
                timeline_frames,
                source_start,
                source_frames,
                loop_clip,
                loop_from,
                gain,
                target_gain,
                failed,
                next_frame,
            } => {
                out.l.fill(0.0);
                if let Some(r) = out.r.as_deref_mut() {
                    r.fill(0.0);
                }
                if *failed || !ctx.playing || (stream.is_none() && resident.is_none()) {
                    // Stopped, failed, or no source: silence. A pause keeps
                    // gain settled so resuming at the same position is
                    // seamless; a discontinuity below starts a fresh ramp.
                    *gain = *target_gain;
                    return;
                }
                if *file_frames == 0 || *source_frames == 0 || *timeline_frames == 0 {
                    return;
                }
                if ctx.discontinuity {
                    *gain = 0.0;
                }

                // Intersect this segment with the clip's stamped timeline
                // interval. This handles a clip edge in the middle of a
                // device block without asking the transport to split there.
                let segment_end = ctx.position.saturating_add(out_len as u64);
                let timeline_end = timeline_start.saturating_add(*timeline_frames);
                let active_start = ctx.position.max(*timeline_start);
                let mut active_end = segment_end.min(timeline_end);
                if active_start >= active_end {
                    return;
                }
                let local_frame = active_start - *timeline_start;
                if !*loop_clip {
                    active_end = active_end.min(
                        active_start.saturating_add(source_frames.saturating_sub(local_frame)),
                    );
                    if active_start >= active_end || local_frame >= *source_frames {
                        return;
                    }
                }
                let write_start = (active_start - ctx.position) as usize;
                let active_len = (active_end - active_start) as usize;

                // Where the timeline says the file should be.
                //
                // A looping clip has a HEAD — everything before the brace
                // — that plays once, and a tail that repeats. With the
                // brace at the region's start the head is empty and this
                // is the plain modulus it always was.
                let want = if *loop_clip {
                    let head = loop_from.saturating_sub(*source_start);
                    let tail = source_start
                        .saturating_add(*source_frames)
                        .saturating_sub(*loop_from);
                    if local_frame < head || tail == 0 {
                        *source_start + local_frame
                    } else {
                        *loop_from + (local_frame - head) % tail
                    }
                } else {
                    *source_start + local_frame
                };
                if let Some(material) = resident.as_ref() {
                    // Small loops are random-access resident material. That
                    // turns even a one-frame brace into one bounded memory
                    // walk instead of one creek seek message per sample.
                    let source_at = |local: u64| {
                        if *loop_clip {
                            let head = loop_from.saturating_sub(*source_start);
                            let tail = source_start
                                .saturating_add(*source_frames)
                                .saturating_sub(*loop_from);
                            if local < head || tail == 0 {
                                source_start.saturating_add(local)
                            } else {
                                loop_from.saturating_add((local - head) % tail)
                            }
                        } else {
                            source_start.saturating_add(local)
                        }
                    };
                    let left = material.channel(0);
                    let right = if material.channels > 1 {
                        material.channel(1)
                    } else {
                        left
                    };
                    for i in 0..active_len {
                        let source = source_at(local_frame.saturating_add(i as u64)) as usize;
                        if let (Some(dst), Some(sample)) =
                            (out.l.get_mut(write_start + i), left.get(source))
                        {
                            *dst = *sample;
                        }
                    }
                    if let Some(dst_right) = out.r.as_deref_mut() {
                        for i in 0..active_len {
                            let source = source_at(local_frame.saturating_add(i as u64)) as usize;
                            if let (Some(dst), Some(sample)) =
                                (dst_right.get_mut(write_start + i), right.get(source))
                            {
                                *dst = *sample;
                            }
                        }
                    }
                    *next_frame = source_at(local_frame.saturating_add(active_len as u64));
                } else if let Some(st) = stream.as_mut() {
                    if ctx.discontinuity || want != *next_frame {
                        // Seek is an async request; until data is ready, reads
                        // yield silence — creek's documented behavior.
                        if st.seek(want as usize, SeekMode::Auto).is_err() {
                            *failed = true;
                            return;
                        }
                        *next_frame = want;
                    }
                    // Read in runs, splitting at the loop wrap. HARD-BOUNDED
                    // by the output frames: each success advances at least
                    // one and a zero read breaks.
                    let mut done = 0usize;
                    let mut reads_left = active_len;
                    let source_end = source_start.saturating_add(*source_frames);
                    while done < active_len && reads_left > 0 {
                        reads_left -= 1;
                        if *next_frame >= source_end {
                            if !*loop_clip {
                                break;
                            }
                            // Exactly at the wrap: request the BRACE's start
                            // again, which need not be the region's start.
                            if st.seek(*loop_from as usize, SeekMode::Auto).is_err() {
                                *failed = true;
                                return;
                            }
                            *next_frame = *loop_from;
                        }
                        let until_wrap = source_end.saturating_sub(*next_frame) as usize;
                        let want_now = (active_len - done).min(until_wrap);
                        if want_now == 0 {
                            break;
                        }
                        match st.read(want_now) {
                            Ok(data) => {
                                let got = data.num_frames().min(want_now);
                                if got == 0 {
                                    break; // EOF or stalled: silence the rest
                                }
                                // ch0 -> L; ch1 -> R (mono: same to both).
                                let src_l = data.read_channel(0);
                                let dst = write_start + done;
                                for (d, s) in out.l[dst..dst + got].iter_mut().zip(src_l.iter()) {
                                    *d = *s;
                                }
                                if let Some(r) = out.r.as_deref_mut() {
                                    let src_r = if data.num_channels() > 1 {
                                        data.read_channel(1)
                                    } else {
                                        src_l
                                    };
                                    for (d, s) in r[dst..dst + got].iter_mut().zip(src_r.iter()) {
                                        *d = *s;
                                    }
                                }
                                // Creek's own playhead is the truth.
                                *next_frame = st.playhead() as u64;
                                if *loop_clip && *next_frame >= source_end {
                                    if st.seek(*loop_from as usize, SeekMode::Auto).is_err() {
                                        *failed = true;
                                        return;
                                    }
                                    *next_frame = *loop_from;
                                }
                                done += got;
                            }
                            Err(_) => {
                                // Flag and silence; schedule disposal joins
                                // the stream on the green side.
                                *failed = true;
                                break;
                            }
                        }
                    }
                }
                // The FADES, folded into the gain pass rather than given
                // one of their own: both are a per-sample multiply over
                // the same span, and two passes would read the buffer
                // twice to do one thing.
                //
                // Measured against the clip's TIMELINE span, which is
                // what a fade handle is dragged along — not against the
                // source region, which a looped clip runs through many
                // times.
                let fade_in = *fade_in;
                let fade_out = *fade_out;
                let fade_in_curve = *fade_in_curve;
                let fade_out_curve = *fade_out_curve;
                let span = *timeline_frames;
                // The SHAPE is applied to the linear position, not to the
                // level afterwards: the curve is the map from "how far
                // along" to "how loud", and running the two ends through
                // their own curves before taking the quieter of them is
                // what makes an overlap behave.
                let envelope = |pos: u64| -> f32 {
                    let mut level = 1.0f32;
                    if fade_in > 0 && pos < fade_in {
                        level = fade_in_curve.at(pos as f32 / fade_in as f32);
                    }
                    if fade_out > 0 {
                        let left = span.saturating_sub(pos);
                        if left <= fade_out {
                            level = level.min(fade_out_curve.at(left as f32 / fade_out as f32));
                        }
                    }
                    level
                };
                let faded = fade_in > 0 || fade_out > 0;

                // THE GAIN ENVELOPE, in the same pass. Its cursor is
                // re-found here rather than per sample: the playhead is
                // monotonic within a block, so a walk forward is enough
                // once the starting point is right.
                //
                // A DISCONTINUITY has just moved the playhead somewhere
                // the cursor knows nothing about, so the search is the
                // only honest option — and a binary search is bounded
                // `log n`, which is not an unbounded path.
                let points: &[(u64, f32)] = envelope_points;
                if !points.is_empty() && (ctx.discontinuity || *envelope_cursor >= points.len()) {
                    *envelope_cursor = points.partition_point(|(at, _)| *at <= local_frame);
                    *envelope_cursor = envelope_cursor.saturating_sub(1);
                }
                let mut cursor = *envelope_cursor;
                // Linear in dB between points is what a fader ride sounds
                // like; the compile has already converted each point to a
                // linear gain, so the interpolation here is over the log
                // of them — one `exp2` per SEGMENT would be a
                // transcendental per sample, so the points are stored
                // pre-converted and interpolated linearly in amplitude
                // between neighbours that are close together anyway.
                let envelope_at = |pos: u64, cursor: &mut usize| -> f32 {
                    if points.is_empty() {
                        return 1.0;
                    }
                    while *cursor + 1 < points.len() && points[*cursor + 1].0 <= pos {
                        *cursor += 1;
                    }
                    let (at, gain) = points[*cursor];
                    let Some(&(next_at, next_gain)) = points.get(*cursor + 1) else {
                        return gain;
                    };
                    if pos <= at || next_at <= at {
                        return gain;
                    }
                    let along = (pos - at) as f32 / (next_at - at) as f32;
                    gain + (next_gain - gain) * along
                };
                let rode = !points.is_empty();

                let gain_step = (*target_gain - *gain) / active_len.max(1) as f32;
                let mut g = *gain;
                // The right channel repeats the walk from the same
                // starting cursor rather than sharing one: the two are
                // the same monotonic walk over the same points, and a
                // shared cursor would leave the second channel starting
                // where the first finished.
                let cursor_at_start = cursor;
                for (i, s) in out.l[write_start..write_start + active_len]
                    .iter_mut()
                    .enumerate()
                {
                    let pos = local_frame + i as u64;
                    *s *= g;
                    if faded {
                        *s *= envelope(pos);
                    }
                    if rode {
                        *s *= envelope_at(pos, &mut cursor);
                    }
                    g += gain_step;
                }
                if let Some(r) = out.r.as_deref_mut() {
                    let mut g = *gain;
                    let mut right_cursor = cursor_at_start;
                    for (i, s) in r[write_start..write_start + active_len]
                        .iter_mut()
                        .enumerate()
                    {
                        let pos = local_frame + i as u64;
                        *s *= if faded { envelope(pos) } else { 1.0 };
                        if rode {
                            *s *= envelope_at(pos, &mut right_cursor);
                        }
                        *s *= g;
                        g += gain_step;
                    }
                }
                *envelope_cursor = cursor;
                *gain = *target_gain;
            }

            Node::Seq {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // The whole walk lives in `PatternClock::run` — the node's
                // job here is only to say which instrument plays and to
                // own the gain ramp across the segment.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices, events, out.l, ctx, &mut ramp);
                *gain = *target_gain; // land exactly, no float drift
            }

            Node::Acid {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // `Kick`'s shape, a gated instrument along. Mono, so there
                // is no right channel to read back.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Kick {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // Identical to `Seq`, a different instrument along — which
                // is the whole point of having extracted the clock. Mono,
                // so there is no right channel to read back.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Snare {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // As `Kick`, a different drum along.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Tom {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // As `Kick`, a different drum along.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Hat {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // As `Kick`, a different drum along.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Handclap {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // As `Kick`, a different drum along.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
            }

            Node::Haze {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // `Poly`'s shape exactly: the clock is mono-shaped, so
                // the bank stashes its right channel and the node reads
                // it back once the segment is done.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }

            Node::Tine {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // `Poly`'s shape exactly — which is the clock earning its
                // keep for the fifth instrument running.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }

            Node::Scomp {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }

            Node::Stab {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }
            Node::Quad {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }
            Node::Brick {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }
            Node::Poly {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // Identical to `Seq` above, one instrument along — which is
                // the entire point of having extracted the clock. The only
                // extra step is stereo: the walk is mono-shaped, so the
                // bank stashes its right channel and the node reads it back
                // once the segment is done.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }

            Node::Loom {
                events,
                clock,
                voices,
                gain,
                target_gain,
            } => {
                // `Poly`'s exact shape, a different instrument — the
                // clock is the same, the stereo readback is the same.
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                }
            }

            Node::Sampler {
                events,
                clock,
                voices,
                preamp,
                gain,
                target_gain,
            } => {
                // `Poly`'s shape, plus the output stage. The voices sum
                // into the walk's mono buffer and the bank's own right
                // channel; the pre-amp runs across BOTH once, after the
                // sum, because there is one output stage and eight
                // voices.
                if ctx.discontinuity {
                    preamp.reset();
                }
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                clock.run(voices.as_mut(), events, out.l, ctx, &mut ramp);
                *gain = *target_gain;
                if let Some(r) = out.r.as_deref_mut() {
                    let right = voices.right(out_len);
                    for (d, s) in r.iter_mut().zip(right.iter()) {
                        *d = *s;
                    }
                    preamp.process(out.l, r);
                } else {
                    // No right slot to write into. The pre-amp still runs
                    // on the left, over a throwaway right, so a mono
                    // wiring sounds like the same device rather than like
                    // a different one.
                    let mut discard = [0.0f32; 0];
                    preamp.process(out.l, &mut discard);
                }
            }

            Node::Delay {
                left,
                right,
                left_buf,
                right_buf,
            } => {
                // The samples in flight belong to wherever the transport
                // just was. Carrying them across a seek would drag the old
                // position's audio into the new one.
                if ctx.discontinuity {
                    left.reset();
                    right.reset();
                    crate::dsp::mem::clear(left_buf);
                    crate::dsp::mem::clear(right_buf);
                }
                if let Some(r) = out.r.as_deref_mut() {
                    // A stereo delay is two independent wires. Folding both
                    // input sides into the left here doubled centred mono and
                    // leaked the right channel across the field — exactly the
                    // phase error PDC exists to prevent.
                    sum_inputs_stereo(inputs, out.l, r);
                    left.process_exact(out.l, left_buf);
                    right.process_exact(r, right_buf);
                } else {
                    sum_inputs_mono(inputs, out.l);
                    left.process_exact(out.l, left_buf);
                }
            }

            Node::Clap { processor } => {
                let Some(right) = out.r.as_deref_mut() else {
                    out.l.fill(0.0);
                    return;
                };
                let result = processor.process_filled(
                    out_len,
                    out.l,
                    right,
                    ctx.discontinuity,
                    |left, right| {
                        left.fill(0.0);
                        right.fill(0.0);
                        for input in inputs {
                            for (destination, source) in left.iter_mut().zip(input.l) {
                                *destination += *source;
                            }
                            let source = input.r.unwrap_or(input.l);
                            for (destination, source) in right.iter_mut().zip(source) {
                                *destination += *source;
                            }
                        }
                    },
                );
                if result.is_err() {
                    // A plugin failure is explicit silence, never stale arena
                    // contents or a partially-written native buffer.
                    out.l.fill(0.0);
                    right.fill(0.0);
                }
            }

            Node::Reverb {
                core,
                buffers,
                predelay,
                predelay_buf,
                fed,
                wet_l,
                wet_r,
                low_cut_l,
                low_cut_r,
                mix,
                target_mix,
                width,
                ..
            } => {
                // A seek must not drag the old room along: cut the tail on
                // discontinuity, exactly as a sounding voice does.
                if ctx.discontinuity {
                    core.reset(buffers);
                    predelay.reset();
                    low_cut_l.reset();
                    low_cut_r.reset();
                }

                // Sum the wired inputs to mono, in place in the output.
                sum_inputs_mono(inputs, out.l);

                let n = out.l.len().min(wet_l.len()).min(wet_r.len()).min(fed.len());
                // PRE-DELAY FIRST, into its own scratch: the ROOM hears a
                // delayed source while the listener still hears the dry
                // one on time, which is the whole point of the control.
                let (Some(feed), Some(dry_src)) = (fed.get_mut(..n), out.l.get(..n)) else {
                    return;
                };
                feed.copy_from_slice(dry_src);
                predelay.process_smooth(feed, predelay_buf);

                // The kernel reads its input and writes its two outputs,
                // and the three slices may not overlap — which is why the
                // feed has a buffer of its own.
                let (Some(input), Some(left), Some(right)) =
                    (fed.get(..n), wet_l.get_mut(..n), wet_r.get_mut(..n))
                else {
                    return;
                };
                core.process(input, left, right, buffers);
                // The tail's low end, cut on the wet alone.
                for sample in wet_l[..n].iter_mut() {
                    *sample = low_cut_l.tick_highpass(*sample);
                }
                for sample in wet_r[..n].iter_mut() {
                    *sample = low_cut_r.tick_highpass(*sample);
                }
                // WIDTH as mid/side on the wet: 0 collapses the room to
                // the centre, 1 is the network's own spread, 2 pushes the
                // sides out past it.
                let spread = *width;
                for index in 0..n {
                    let (Some(l), Some(r)) = (wet_l.get(index).copied(), wet_r.get(index).copied())
                    else {
                        continue;
                    };
                    let mid = (l + r) * 0.5;
                    let side = (l - r) * 0.5 * spread;
                    if let Some(slot) = wet_l.get_mut(index) {
                        *slot = mid + side;
                    }
                    if let Some(slot) = wet_r.get_mut(index) {
                        *slot = mid - side;
                    }
                }
                let wet = &*wet_l;

                // Linear mix ramp across the segment: a mix knob must not
                // click, same rule as every other ramped parameter.
                let mut ramp = Ramp::across(*mix, *target_mix, n);
                for (d, w) in out.l.iter_mut().zip(wet.iter()).take(n) {
                    let m = ramp.next();
                    *d = *d * (1.0 - m) + *w * m;
                }
                *mix = *target_mix; // land exactly, no float drift
            }

            Node::Filter { core, scratch } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the old ring along, and must not
                // glide old knob motion into the new position.
                if ctx.discontinuity {
                    core.reset();
                }

                // Compile fixes the channel count at 2, so this holds; it
                // is checked rather than assumed because the check costs
                // nothing and an assumption costs a panic. A mismatch
                // leaves the summed dry signal standing.
                if right.len() == out.l.len() {
                    core.process(out.l, right, scratch);
                }
            }

            Node::Modulato { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not drag the old wobble along.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }

            Node::Limiter {
                core,
                scratch,
                key,
                line_l,
                line_r,
            } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the half-bands' history, the
                // limiter's gain or the brightener's envelope across the
                // jump — and the delay lines are the CALLER'S to clear,
                // which the kernel's contract says in as many words.
                if ctx.discontinuity {
                    core.reset();
                    crate::dsp::mem::clear(key);
                    crate::dsp::mem::clear(line_l);
                    crate::dsp::mem::clear(line_r);
                }

                // Compile fixes the channel count at 2, so this holds; it
                // is checked rather than assumed because the check costs
                // nothing and an assumption costs a panic. A mismatch
                // leaves the summed dry signal standing.
                if right.len() == out.l.len() {
                    core.process(out.l, right, scratch, key, line_l, line_r);
                }
            }

            Node::Phaser {
                core,
                lfo,
                dry,
                sample_rate,
                stages,
                centre_hz,
                depth_oct,
                mix,
                mix_target,
            } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // The filters forget; the SWEEP does not. See
                // `Node::Phaser` — the integrators belong to a position
                // the transport has left, the LFO's phase belongs to the
                // device.
                if ctx.discontinuity {
                    for ch in core.iter_mut() {
                        ch.reset();
                    }
                    mix.set_now(*mix_target);
                }

                let n = out.l.len();
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                let stereo = right.len() == n;

                // Fail open exactly as the lo-fi and the sheen do.
                if dry.len() >= n * 2 {
                    let (dry_l, rest) = dry.split_at_mut(n);
                    let dry_r = &mut rest[..n];
                    dry_l.copy_from_slice(out.l);
                    if stereo {
                        dry_r.copy_from_slice(right);
                    }

                    // The corner moves at CONTROL rate, one step per
                    // chunk, because moving it costs a `prepare` — one
                    // transcendental — and a phaser sweeping at a few
                    // hertz cannot tell the difference between a corner
                    // that steps fifteen hundred times a second and one
                    // that glides. Per-sample would be the same picture
                    // for forty times the arithmetic.
                    let mut start = 0;
                    while start < n {
                        let k = PHASER_CHUNK.min(n - start);
                        let mut sweep = [0.0f32; PHASER_CHUNK];
                        let mut mixr = [0.0f32; PHASER_CHUNK];
                        lfo.process(&mut sweep[..k]);
                        mix.process(&mut mixr[..k]);

                        // Depth is in OCTAVES either side of centre, so
                        // the sweep is the same musical distance wherever
                        // the centre knob is.
                        let corner =
                            (*centre_hz * (*depth_oct * sweep[k - 1]).exp2()).clamp(20.0, 20_000.0);
                        for ch in core.iter_mut() {
                            ch.prepare(*sample_rate, corner, PHASER_Q, *stages);
                        }

                        let end = start + k;
                        core[0].process(&mut out.l[start..end]);
                        if stereo {
                            core[1].process(&mut right[start..end]);
                        }

                        // The blend is what makes the notch: the allpass
                        // alone is flat, and only summing it with the dry
                        // turns a phase turn into a cancellation.
                        for j in 0..k {
                            let i = start + j;
                            let d = dry_l[i];
                            out.l[i] = d + (out.l[i] - d) * mixr[j];
                        }
                        if stereo {
                            for j in 0..k {
                                let i = start + j;
                                let d = dry_r[i];
                                right[i] = d + (right[i] - d) * mixr[j];
                            }
                        }
                        start = end;
                    }
                }
            }
            Node::Tilt { core, .. } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the pole's history into the new
                // position. There are no smoothers to snap: this device
                // has nothing a wire can glide.
                if ctx.discontinuity {
                    for ch in core.iter_mut() {
                        ch.reset();
                    }
                }

                // In place, one filter per channel, and that is the whole
                // node — no blend to compute and so no scratch to run out
                // of, which is why this one has no fail-open branch.
                core[0].process(out.l);
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                if right.len() == out.l.len() {
                    core[1].process(right);
                }
            }
            Node::Disperser { core, .. } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag sixty-four integrators' worth of
                // history into the new position. There are no smoothers to
                // snap: this device has nothing a wire can glide.
                if ctx.discontinuity {
                    for ch in core.iter_mut() {
                        ch.reset();
                    }
                }

                // In place, one chain per channel, and that is the whole
                // node — no blend to compute and so no scratch to run out
                // of, which is why this one has no fail-open branch.
                core[0].process(out.l);
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                if right.len() == out.l.len() {
                    core[1].process(right);
                }
            }
            Node::Sheen {
                core,
                dry,
                mix,
                trim,
                mix_target,
                trim_target,
                ..
            } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the slew follower's envelope or the
                // edge band's pole along, and must not glide old knob
                // motion into the new position.
                if ctx.discontinuity {
                    for ch in core.iter_mut() {
                        ch.reset();
                    }
                    mix.set_now(*mix_target);
                    trim.set_now(*trim_target);
                }

                let n = out.l.len();
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                let stereo = right.len() == n;

                // Fail open exactly as the saturator and the lo-fi do:
                // too little scratch leaves the dry signal standing, never
                // an early return that would hand the graph a buffer
                // nobody wrote.
                if dry.len() >= n * 2 {
                    let (dry_l, rest) = dry.split_at_mut(n);
                    let dry_r = &mut rest[..n];
                    dry_l.copy_from_slice(out.l);
                    if stereo {
                        dry_r.copy_from_slice(right);
                    }

                    core[0].process(out.l);
                    if stereo {
                        core[1].process(right);
                    }

                    // Both channels read the SAME chunk of ramp rather
                    // than each advancing the smoothers, which would run
                    // them at twice the rate they were prepared for.
                    let mut start = 0;
                    while start < n {
                        let k = SHEEN_CHUNK.min(n - start);
                        let mut mixr = [0.0f32; SHEEN_CHUNK];
                        let mut trimr = [0.0f32; SHEEN_CHUNK];
                        mix.process(&mut mixr[..k]);
                        trim.process(&mut trimr[..k]);
                        for j in 0..k {
                            let i = start + j;
                            let d = dry_l[i];
                            out.l[i] = (d + (out.l[i] - d) * mixr[j]) * trimr[j];
                        }
                        if stereo {
                            for j in 0..k {
                                let i = start + j;
                                let d = dry_r[i];
                                right[i] = (d + (right[i] - d) * mixr[j]) * trimr[j];
                            }
                        }
                        start += k;
                    }
                }
            }
            Node::Lofi {
                core,
                dry,
                mix,
                trim,
                mix_target,
                trim_target,
                ..
            } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the hold's latched sample or the
                // tracking filter's poles along, and must not glide old
                // knob motion into the new position.
                if ctx.discontinuity {
                    for ch in core.iter_mut() {
                        ch.reset();
                    }
                    mix.set_now(*mix_target);
                    trim.set_now(*trim_target);
                }

                let n = out.l.len();
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                let stereo = right.len() == n;

                // Fail open exactly as the saturator does: too little
                // scratch leaves the dry signal standing, never an early
                // return that would hand the graph a buffer nobody wrote.
                if dry.len() >= n * 2 {
                    let (dry_l, rest) = dry.split_at_mut(n);
                    let dry_r = &mut rest[..n];
                    dry_l.copy_from_slice(out.l);
                    if stereo {
                        dry_r.copy_from_slice(right);
                    }

                    // In place, one converter per channel — the kernel is
                    // documented in-place safe, and its own bypass checks
                    // make a clean setting cost a compare rather than a
                    // pass over the block.
                    core[0].process(out.l);
                    if stereo {
                        core[1].process(right);
                    }

                    // The blend and the trim walk PER SAMPLE, in chunks,
                    // for the reason `Node::Sat` walks its controls that
                    // way: a gain applied once per block steps audibly at
                    // the segment edge when a wire is moving it.
                    //
                    // Both channels read the SAME chunk of ramp rather
                    // than each advancing the smoothers, which would run
                    // them at twice the rate they were prepared for and
                    // silently halve every smoothing time in the device.
                    let mut start = 0;
                    while start < n {
                        let k = LOFI_CHUNK.min(n - start);
                        let mut mixr = [0.0f32; LOFI_CHUNK];
                        let mut trimr = [0.0f32; LOFI_CHUNK];
                        mix.process(&mut mixr[..k]);
                        trim.process(&mut trimr[..k]);
                        for j in 0..k {
                            let i = start + j;
                            let d = dry_l[i];
                            out.l[i] = (d + (out.l[i] - d) * mixr[j]) * trimr[j];
                        }
                        if stereo {
                            for j in 0..k {
                                let i = start + j;
                                let d = dry_r[i];
                                right[i] = (d + (right[i] - d) * mixr[j]) * trimr[j];
                            }
                        }
                        start += k;
                    }
                }
            }
            Node::Sat {
                core,
                scratch2x,
                mode,
                pending_mode,
                drive,
                bias,
                mix,
                trim,
                drive_target,
                bias_target,
                mix_target,
                trim_target,
            } => {
                // `out.l` and `out.r` are separate fields, so this borrows
                // only the right channel and the left stays reachable.
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // A seek must not drag the half-band's history along, and
                // must not glide old knob motion into the new position.
                if ctx.discontinuity {
                    for os in core.oversampler.iter_mut() {
                        os.reset();
                    }
                    for dc in core.dc.iter_mut() {
                        dc.reset();
                    }
                    drive.set_now(*drive_target);
                    bias.set_now(*bias_target);
                    mix.set_now(*mix_target);
                    trim.set_now(*trim_target);
                }

                // A mode switch lands on the segment edge and clears
                // NOTHING. The filter resets its cascade here because a
                // lowpass's history inside a highpass is a thump; a
                // transfer curve has no history to carry, so there is
                // nothing to clear and nothing gained by pretending
                // otherwise. The step in the curve is inherent to having
                // asked for a different curve.
                *mode = *pending_mode;

                let n = out.l.len();
                let n2 = n * 2;
                // Compile fixes the channel count at 2, so this is true;
                // it is checked rather than assumed because the check
                // costs nothing and an assumption costs a panic.
                let stereo = right.len() == n;
                let trim_start = trim.current();

                // Fail open exactly as the filter's drive stage does: too
                // little scratch leaves the dry signal standing, never an
                // early return that would hand the graph a buffer nobody
                // wrote.
                if scratch2x.len() >= n2 * 3 {
                    let (up_l, rest) = scratch2x.split_at_mut(n2);
                    let (up_r, shaped) = rest.split_at_mut(n2);
                    let shaped = &mut shaped[..n2];
                    core.oversampler[0].up(out.l, up_l);
                    if stereo {
                        core.oversampler[1].up(right, up_r);
                    }

                    // The controls walk at the ORIGINAL rate in short
                    // chunks, and each chunk's settled value configures
                    // the curve for the 2x samples that chunk owns. At 1x
                    // because that is the rate the smoothers were
                    // prepared for: walking them at 2x would silently
                    // halve every smoothing time in the device.
                    let mut mix_prev = mix.current();
                    for start in (0..n).step_by(SAT_CHUNK) {
                        let k = SAT_CHUNK.min(n - start);
                        let mut ctrl = [0.0f32; SAT_CHUNK];
                        drive.process(&mut ctrl[..k]);
                        let drive_now = ctrl[k - 1];
                        bias.process(&mut ctrl[..k]);
                        let bias_now = ctrl[k - 1];
                        mix.process(&mut ctrl[..k]);
                        let mix_now = ctrl[k - 1];
                        // Advances with the rest so its ramp below spans
                        // the same segment; applied at 1x after the round
                        // trip, where a gain change cannot alias.
                        trim.process(&mut ctrl[..k]);

                        core.shaper.configure(
                            crate::params::sat::mode(*mode),
                            drive_now,
                            bias_now,
                            1.0, // pure: the node owns the blend
                        );
                        let (a, b) = (start * 2, (start + k) * 2);
                        sat_chunk(
                            &core.shaper,
                            &mut up_l[a..b],
                            &mut shaped[a..b],
                            mix_prev,
                            mix_now,
                        );
                        if stereo {
                            sat_chunk(
                                &core.shaper,
                                &mut up_r[a..b],
                                &mut shaped[a..b],
                                mix_prev,
                                mix_now,
                            );
                        }
                        mix_prev = mix_now;
                    }

                    core.oversampler[0].down(up_l, out.l);
                    if stereo {
                        core.oversampler[1].down(up_r, right);
                    }
                }

                // Output trim, ramped across the segment like every other
                // audible gain here. On the fail-open path the smoothers
                // never advanced, so start == now and this is a no-op.
                let trim_now = trim.current();
                let mut ramp = Ramp::across(trim_start, trim_now, n);
                for s in out.l.iter_mut() {
                    *s *= ramp.next();
                }
                if stereo {
                    let mut ramp = Ramp::across(trim_start, trim_now, n);
                    for s in right.iter_mut() {
                        *s *= ramp.next();
                    }
                }

                // A biased curve HAS an offset — that is what bias means —
                // and offset downstream is headroom spent on nothing.
                core.dc[0].process(out.l);
                if stereo {
                    core.dc[1].process(right);
                }
            }

            Node::Echo {
                core,
                rings,
                scratch,
                sync,
                pending_sync,
                time,
                time_ms,
                feedback,
                tone,
                drive,
                wow,
                spread,
                mix,
                aux,
                in_gain,
                in_target,
                time_target,
                feedback_target,
                tone_target,
                drive_target,
                wow_target,
                spread_target,
                mix_target,
                sample_rate,
            } => {
                use crate::params::echo as ep;

                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);

                // THE SEND, applied to the tap before the delay sees it.
                //
                // Ahead of the fail-open branch below on purpose: when
                // the scratch is too small this arm passes its input
                // through untouched, and an aux's input is a COPY of a
                // track that is already reaching the master by its own
                // path. Scaled here, a failed aux leaks the tap at the
                // send's level; unscaled, it would leak it at full and
                // the track would arrive at the master twice.
                //
                // The ramp spans the BLOCK, not the segment, for the
                // reason the fader in `Node::Pan` does: letters drain
                // once per block, so a send move already sits in
                // `in_target` when segment 0 runs, and a block split at
                // a loop point can make that segment a single frame —
                // long enough to turn a level move into a click.
                if *aux {
                    // Ahead of the ramp, not with the smoothers further
                    // down: a seek must LAND on the send rather than
                    // glide in from wherever the last position left it,
                    // and a letter that arrived before the first block
                    // ever ran must be in force for that block.
                    if ctx.discontinuity {
                        *in_gain = *in_target;
                    }
                    let remaining = ctx.block_frames.saturating_sub(ctx.offset).max(out_len);
                    let mut level = Ramp::across(*in_gain, *in_target, remaining);
                    for i in 0..out_len {
                        let g = level.next();
                        out.l[i] *= g;
                        if let Some(r) = right.get_mut(i) {
                            *r *= g;
                        }
                    }
                    if ctx.offset + out_len >= ctx.block_frames {
                        *in_gain = *in_target; // land exactly, no float drift
                    } else {
                        *in_gain = level.value;
                    }
                }

                // A sync change is just a time change: it lands on the
                // segment edge and the glide carries it, which is why
                // switching from an eighth to a quarter swoops rather
                // than jumps.
                *sync = *pending_sync;
                // The time in SAMPLES, derived HERE and every segment,
                // because a synced echo's length is a property of the
                // tempo this segment is playing at — a tempo ramp has to
                // drag the repeats with it, and a stored sample count
                // would hold them at the old speed.
                *time_target =
                    ep::time_samples(*sync, *time_ms, *sample_rate, ctx.beats_per_sample);
                time.set_target(*time_target);

                // A seek must not ring the old position's repeats into
                // the new one, and must not glide old knob motion in.
                if ctx.discontinuity {
                    for (line, ring) in core.line.iter_mut().zip(rings.iter_mut()) {
                        line.reset();
                        // The kernel owns the pointer, the caller owns
                        // the memory — so clearing the ring is this
                        // arm's job, and `fill` is not an allocation.
                        ring.fill(0.0);
                    }
                    for w in core.wow.iter_mut() {
                        w.reset();
                    }
                    time.set_now(*time_target);
                    feedback.set_now(*feedback_target);
                    tone.set_now(*tone_target);
                    drive.set_now(*drive_target);
                    wow.set_now(*wow_target);
                    spread.set_now(*spread_target);
                    mix.set_now(*mix_target);
                }

                let n = out.l.len();
                let stereo = right.len() == n;
                let mix_start = mix.current();

                // Fail open exactly as the filter and saturator do: too
                // little scratch leaves the dry signal standing.
                if scratch.len() >= n * 4 && n > 0 {
                    let (base, rest) = scratch.split_at_mut(n);
                    let (times, rest) = rest.split_at_mut(n);
                    let (wobble, wet) = rest.split_at_mut(n);
                    let wet = &mut wet[..n];

                    // The BASE time, per sample, so a moving time bends
                    // the pitch of what is already in the buffer.
                    time.process(base);

                    // Everything else settles once per segment. These
                    // are loop settings rather than sample values — a
                    // segment is a few milliseconds, and the smoothers
                    // in front of them are what keep a turned knob from
                    // stepping.
                    feedback.process(times);
                    let fb_now = times[n - 1];
                    tone.process(times);
                    let tone_now = times[n - 1];
                    drive.process(times);
                    let drive_now = times[n - 1];
                    wow.process(times);
                    let wow_now = times[n - 1];
                    spread.process(times);
                    let spread_now = times[n - 1];
                    mix.process(times);
                    let mix_now = times[n - 1];

                    let depth = wow_now * 0.01 * ECHO_WOW_MAX;
                    for (ch, (line, ring)) in core.line.iter_mut().zip(rings.iter_mut()).enumerate()
                    {
                        if ch == 1 && !stereo {
                            break;
                        }
                        line.set_feedback(fb_now * 0.01);
                        line.set_drive(drive_now * 0.01 * ECHO_DRIVE_MAX);
                        line.set_damp(*sample_rate, tone_now);

                        // The right channel's echo runs LATER by a
                        // fraction of the time: one control, a stereo
                        // picture, and mono-compatible at zero.
                        let stretch = if ch == 1 {
                            1.0 + spread_now * 0.01
                        } else {
                            1.0
                        };
                        core.wow[ch].process(wobble);
                        for ((t, b), w) in times.iter_mut().zip(base.iter()).zip(wobble.iter()) {
                            *t = *b * stretch * (1.0 + depth * *w);
                        }

                        let dry = if ch == 0 { &*out.l } else { &*right };
                        wet.copy_from_slice(dry);
                        line.process_modulated(wet, ring, times);

                        // The node owns the blend, so the dry path keeps
                        // its float headroom whatever the loop does.
                        let dst = if ch == 0 { &mut *out.l } else { &mut *right };
                        let mut ramp = Ramp::across(mix_start * 0.01, mix_now * 0.01, n);
                        for (d, w) in dst.iter_mut().zip(wet.iter()) {
                            let m = ramp.next();
                            *d = *d * (1.0 - m) + *w * m;
                        }
                    }
                }
            }

            Node::Eq { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not ring the old position's filters into
                // the new one, and must not glide old knob motion in.
                if ctx.discontinuity {
                    core.snap();
                }
                core.process(out.l, right);
            }

            Node::Resyn { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not reconstruct a frame half-filled from
                // before it, and must not carry the output FIFO's
                // contents across the join.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Tone { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                core.process(out.l, right);
            }
            Node::Sigil { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                core.process(out.l, right);
            }
            Node::Gauge { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // MEASURES ONLY. The signal is already in place and this
                // arm never writes to it — the contract is kept by the
                // shape of the call, not by remembering not to.
                core.measure(out.l, right);
            }
            Node::Umbra { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not carry a reverb tail, a delay line and a
                // frame of spectrum across the join.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Ferric { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not leave the head somewhere the new
                // position knows nothing about, and the reel itself is
                // now the wrong recording.
                if ctx.discontinuity {
                    core.reset();
                }
                // The BEAT, straight through: the head is placed on the
                // grid, so the grid is what it needs.
                core.process(out.l, right, ctx.beat, ctx.beats_per_sample, ctx.playing);
            }
            Node::Sibyl { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // Same reason as the resynthesiser's, plus one of its
                // own: the phase accumulator is a running total, and
                // carrying it across a splice would smear the seam.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Strip { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not ring the four shelves into the new
                // position, nor carry the output stage's state across it.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Section { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                if ctx.discontinuity {
                    core.reset();
                }
                let clock = crate::audio::console::Clock {
                    playing: ctx.playing,
                    beat: ctx.beat,
                    beats_per_sample: ctx.beats_per_sample,
                };
                core.process(out.l, right, &clock);
            }
            Node::Gate { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not carry the old position's gain into the
                // new one — and the gate REOPENS rather than staying
                // shut, so a transport landing mid-phrase hears the
                // phrase rather than the tail of a decision made
                // somewhere else.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Glue { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not carry the old position's gain
                // reduction into the new one: the compressor arrives
                // open, exactly as it would if the transport had just
                // started there.
                if ctx.discontinuity {
                    core.snap();
                }
                core.process(out.l, right);
            }
            Node::Prism { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not ring the crossover into the new
                // position, nor carry three bands' worth of gain
                // reduction across with it.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Clamp { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // Same reason as the glue's: arrive open, so the seek
                // does not hand the new position a gain reduction that
                // belonged to the old one.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }
            Node::Flint { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not hand the new position an envelope that
                // belonged to the old one — the detector would read the
                // splice itself as the biggest transient in the project.
                if ctx.discontinuity {
                    core.reset();
                }
                core.process(out.l, right);
            }

            Node::Utility { core } => {
                let right = out.r.as_deref_mut().unwrap_or(&mut []);
                sum_inputs_stereo(inputs, out.l, right);
                // A seek must not ring the crossover or the DC blocker
                // into the new position, and must not glide the old
                // one's knob motion in.
                if ctx.discontinuity {
                    core.snap();
                }
                core.process(out.l, right);
            }

            Node::Click {
                phase,
                env,
                last_beat,
                sample_rate,
            } => {
                // Transport jumped (first play, seek, loop wrap): resync the
                // counter. Landing exactly ON a beat fires it; landing mid-beat
                // waits for the next one rather than clicking late.
                if ctx.discontinuity {
                    let idx = ctx.beat.floor() as i64;
                    *last_beat = if ctx.beat.fract() == 0.0 {
                        idx - 1
                    } else {
                        idx
                    };
                }
                // ~1kHz blip, exponential decay (~25ms to inaudible at 48k).
                for (i, s) in out.l.iter_mut().enumerate() {
                    let beat = ctx.beat + i as f64 * ctx.beats_per_sample;
                    let idx = beat.floor() as i64;
                    if ctx.playing {
                        if idx > *last_beat {
                            *env = 1.0;
                            *phase = 0.0;
                            *last_beat = idx;
                        } else if idx < *last_beat {
                            // Tempo drop teleported the derived beat backward
                            // (not a discontinuity — position never moved).
                            // Resync without firing; the next beat clicks.
                            *last_beat = idx;
                        }
                    }
                    *s = (*phase * TAU).sin() * *env * 0.6;
                    *phase = (*phase + 1_000.0 / *sample_rate).fract();
                    *env *= 0.994;
                }
            }
        }
    }

    /// The pattern clock a timeline-locked instrument walks, for the
    /// schedule to take its effect locks from after it renders.
    fn clock_mut(&mut self) -> Option<&mut PatternClock> {
        match self {
            Node::Seq { clock, .. }
            | Node::Tine { clock, .. }
            | Node::Scomp { clock, .. }
            | Node::Stab { clock, .. }
            | Node::Quad { clock, .. }
            | Node::Brick { clock, .. }
            | Node::Poly { clock, .. }
            | Node::Loom { clock, .. }
            | Node::Haze { clock, .. }
            | Node::Kick { clock, .. }
            | Node::Acid { clock, .. }
            | Node::Sampler { clock, .. }
            | Node::Snare { clock, .. }
            | Node::Tom { clock, .. }
            | Node::Hat { clock, .. }
            | Node::Handclap { clock, .. } => Some(clock),
            _ => None,
        }
    }

    /// Apply one parameter letter. Unknown param ids are ignored — a stale
    /// letter after a graph edit must be harmless, not a panic.
    fn apply(&mut self, param: u32, value: f32) {
        // clamp() passes NaN through, so a NaN letter would poison ramped
        // state (gain, freq, pan) until the next finite letter healed it.
        // No parameter means anything non-finite: bin the letter here, once,
        // for every arm — including future ones.
        if !value.is_finite() {
            return;
        }
        // Ids and ranges come from crate::params — the same table the
        // widgets draw from, so a knob cannot drift from what this arm
        // accepts. params::clamp is a bounded scan of a static table:
        // red-zone legal, and an unknown id comes back None (a stale or
        // misrouted letter — binned, never applied).
        use crate::params::{
            clip, disperser, echo, filter, lofi, mixer, pan, phaser, reverb, sat, seq, sheen, sine,
            tilt,
        };
        match self {
            // No parameters: compile decides a delay's length, and it
            // cannot change without a recompile — a letter that moved it
            // would slide the track it is compensating.
            Node::Silence
            | Node::Input { .. }
            | Node::Delay { .. }
            | Node::Clap { .. }
            | Node::DeskPath { .. }
            | Node::DeskBleed { .. } => {}
            // Every knob is a live letter, including the two pitch
            // envelopes' times — the voice rebuilds its envelope timings
            // when one moves, so a decay turned mid-pattern is heard on
            // the next hit rather than at the next recompile.
            Node::Kick {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::kick::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::kick::GAIN {
                    *target_gain = value;
                }
            }

            // Every knob is a live letter, and the LEVEL is the node's own
            // gain ramp rather than a multiply inside the voice — so
            // moving it glides across the segment instead of stepping at
            // its edge.
            Node::Acid {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::acid::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::acid::LEVEL {
                    *target_gain = value;
                }
            }

            // Every knob is a live letter except the FILE, which is not a
            // knob: it is a recompile, and it arrives as a new spec.
            Node::Sampler {
                voices,
                preamp,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::sampler::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                match param {
                    // Decibels on the knob, linear on the wire.
                    crate::params::sampler::GAIN => {
                        *target_gain = 10f32.powf(value / 20.0);
                    }
                    // The output stage is the node's, not the bank's, so
                    // its letter lands here.
                    crate::params::sampler::PREAMP => preamp.set_amount(value),
                    _ => {}
                }
            }
            Node::Snare {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::snare::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::snare::GAIN {
                    *target_gain = value;
                }
            }
            Node::Tom {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::tom::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::tom::GAIN {
                    *target_gain = value;
                }
            }
            Node::Hat {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::hat::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::hat::GAIN {
                    *target_gain = value;
                }
            }
            Node::Handclap {
                voices,
                target_gain,
                ..
            } => {
                let Some(value) =
                    crate::params::clamp(crate::params::handclap::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
                if param == crate::params::handclap::GAIN {
                    *target_gain = value;
                }
            }
            // Every knob is a live letter. Rate, spread and feedback
            // rebuild a coefficient apiece, which is a bounded handful of
            // arithmetic — the same cost the Filter node pays.
            Node::Modulato { core } => {
                if crate::params::clamp(crate::params::modulato::TABLE, param, value).is_some() {
                    core.set_param(param, value);
                }
            }
            Node::Sine {
                target_freq,
                target_amp,
                ..
            } => {
                let Some(value) = crate::params::clamp(sine::TABLE, param, value) else {
                    return;
                };
                match param {
                    sine::FREQ => *target_freq = value,
                    sine::AMP => *target_amp = value,
                    _ => {}
                }
            }
            Node::Mixer { target_gain, .. } => {
                if let Some(value) = crate::params::clamp(mixer::TABLE, param, value)
                    && param == mixer::GAIN
                {
                    *target_gain = value;
                }
            }
            Node::Send {
                target_gain,
                param: wanted,
                ..
            } => {
                // A send listens for ONE parameter of the section that
                // owns it, and ignores the rest of that section's
                // letters — which is what lets the tap live on the
                // channel's OUT without being a device of its own.
                if param == *wanted {
                    *target_gain = (value * 0.01).clamp(0.0, 1.0);
                }
            }
            Node::Click { .. } => {}
            Node::Reverb {
                core,
                predelay,
                low_cut_l,
                low_cut_r,
                target_mix,
                width,
                sample_rate,
                ..
            } => {
                let Some(value) = crate::params::clamp(reverb::TABLE, param, value) else {
                    return;
                };
                // Every row is a live letter. Size and decay rebuild the
                // per-line gains, which is a handful of `powf` on a knob
                // move — the same bounded cost the Filter node pays to
                // re-prepare its cascade.
                match param {
                    reverb::MIX => *target_mix = value,
                    reverb::PREDELAY => {
                        predelay.set_delay(value * 0.001 * *sample_rate);
                    }
                    reverb::SIZE => core.set_size(value),
                    reverb::DECAY => core.set_decay(value),
                    reverb::DAMP => core.set_damping(value),
                    reverb::LOWCUT => {
                        low_cut_l.prepare(*sample_rate, value);
                        low_cut_r.prepare(*sample_rate, value);
                    }
                    reverb::DIFFUSION => core.set_diffusion(value),
                    reverb::MODULATION => core.set_modulation(value),
                    reverb::WIDTH => *width = value,
                    _ => {}
                }
            }
            // Every knob is a live letter. The core decides for itself
            // which of them costs a coefficient rebuild and which has to
            // wait for a block edge — a mode or character switch clears
            // filter state, and a cleared cascade mid-block is a click.
            Node::Filter { core, .. } => {
                let Some(value) = crate::params::clamp(filter::TABLE, param, value) else {
                    return;
                };
                core.set_param(param, value);
            }
            // Every knob is a live letter. The core decides for itself
            // which of them costs a coefficient rebuild, so a settled
            // device re-derives nothing — and the release in particular
            // moves WITHOUT clearing the delay line, which is why the
            // kernel has a setter for it separate from `prepare`.
            Node::Limiter { core, .. } => {
                let Some(value) = crate::params::clamp(crate::params::limiter::TABLE, param, value)
                else {
                    return;
                };
                core.set_param(param, value);
            }
            Node::Lofi {
                core,
                rate,
                bits,
                mix,
                trim,
                mix_target,
                trim_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(lofi::TABLE, param, value) else {
                    return;
                };
                let set = |target: &mut f32, s: &mut crate::dsp::ramps::Smoother| {
                    *target = value;
                    s.set_target(value);
                };
                match param {
                    // Handed to the kernel HERE rather than parked for the
                    // segment edge, and NOT smoothed. Both are coefficient
                    // work — a filter corner and a lattice step — rather
                    // than something a sample is multiplied by, and the
                    // kernel reads them once per block to decide whether
                    // it is bypassed at all. A smoother on either would
                    // buy nothing and would blur the two exact off
                    // switches into approximate ones.
                    lofi::RATE => {
                        *rate = value;
                        for ch in core.iter_mut() {
                            ch.set_rate(value);
                        }
                    }
                    lofi::BITS => {
                        *bits = value;
                        for ch in core.iter_mut() {
                            ch.set_bits(value);
                        }
                    }
                    lofi::MIX => set(mix_target, mix),
                    lofi::OUT => set(trim_target, trim),
                    _ => {}
                }
            }
            Node::Sheen {
                core,
                sample_rate,
                amount,
                edge_hz,
                mix,
                trim,
                mix_target,
                trim_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(sheen::TABLE, param, value) else {
                    return;
                };
                let set = |target: &mut f32, s: &mut crate::dsp::ramps::Smoother| {
                    *target = value;
                    s.set_target(value);
                };
                match param {
                    // Straight to the kernel, unsmoothed. The amount is a
                    // multiplier the kernel applies itself and the corner
                    // is coefficient work — neither is something a sample
                    // is scaled by here, and `prepare` keeps the follower's
                    // state, so restating both costs two exponentials at a
                    // segment edge and no discontinuity.
                    sheen::AMOUNT => {
                        *amount = value;
                        for ch in core.iter_mut() {
                            ch.set_amount(value);
                        }
                    }
                    sheen::EDGE => {
                        *edge_hz = value;
                        for ch in core.iter_mut() {
                            ch.prepare(*sample_rate, value, *amount);
                        }
                    }
                    sheen::MIX => set(mix_target, mix),
                    sheen::OUT => set(trim_target, trim),
                    _ => {}
                }
            }
            Node::Disperser {
                core,
                sample_rate,
                freq_hz,
                pinch,
                stages,
            } => {
                let Some(value) = crate::params::clamp(disperser::TABLE, param, value) else {
                    return;
                };
                match param {
                    disperser::AMOUNT => *stages = value.round().max(0.0) as u32,
                    disperser::FREQ => *freq_hz = value,
                    disperser::PINCH => *pinch = value,
                    _ => return,
                }
                // Re-tuned immediately rather than parked for the segment
                // edge, and with no smoother on any of the three. The
                // kernel is built for exactly this: one transcendental
                // whatever the stage count, and the integrator STATE is
                // deliberately left alone, so a re-tune cannot click. Its
                // own doc says re-tuning from the audio thread is the case
                // it exists for.
                for ch in core.iter_mut() {
                    ch.prepare(*sample_rate, *freq_hz, *pinch, *stages);
                }
            }
            Node::Tilt {
                core,
                sample_rate,
                pivot_hz,
                tilt_db,
            } => {
                let Some(value) = crate::params::clamp(tilt::TABLE, param, value) else {
                    return;
                };
                match param {
                    tilt::TILT => *tilt_db = value,
                    tilt::PIVOT => *pivot_hz = value,
                    _ => return,
                }
                // Re-tuned immediately and unsmoothed. `prepare` recomputes
                // two gains and one pole coefficient and leaves the pole's
                // STATE alone, so a moving knob cannot click — the same
                // property `Node::Disperser` leans on.
                for ch in core.iter_mut() {
                    ch.prepare(*sample_rate, *pivot_hz, *tilt_db);
                }
            }
            Node::Phaser {
                lfo,
                stages,
                centre_hz,
                depth_oct,
                mix,
                mix_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(phaser::TABLE, param, value) else {
                    return;
                };
                match param {
                    // The three that shape the sweep are read by the
                    // process arm every chunk, so they need no smoother
                    // and no re-prepare here — unlike the disperser's,
                    // whose corner is the only thing it has.
                    phaser::AMOUNT => *stages = value.round().max(0.0) as u32,
                    phaser::CENTRE => *centre_hz = value,
                    phaser::DEPTH => *depth_oct = value,
                    phaser::RATE => lfo.set_rate(value),
                    phaser::MIX => {
                        *mix_target = value;
                        mix.set_target(value);
                    }
                    _ => {}
                }
            }
            Node::Sat {
                pending_mode,
                drive,
                bias,
                mix,
                trim,
                drive_target,
                bias_target,
                mix_target,
                trim_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(sat::TABLE, param, value) else {
                    return;
                };
                // Every row is a smoothed control except the mode, which
                // is a choice and cannot be interpolated toward.
                let set = |target: &mut f32, s: &mut crate::dsp::ramps::Smoother| {
                    *target = value;
                    s.set_target(value);
                };
                match param {
                    sat::MODE => *pending_mode = value.round() as u32,
                    sat::DRIVE => set(drive_target, drive),
                    sat::BIAS => set(bias_target, bias),
                    sat::MIX => set(mix_target, mix),
                    sat::OUT => set(trim_target, trim),
                    _ => {}
                }
            }
            Node::Echo {
                pending_sync,
                time_ms,
                feedback,
                tone,
                drive,
                wow,
                spread,
                mix,
                feedback_target,
                tone_target,
                drive_target,
                wow_target,
                spread_target,
                mix_target,
                aux,
                in_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(echo::TABLE, param, value) else {
                    return;
                };
                let set = |target: &mut f32, s: &mut crate::dsp::ramps::Smoother| {
                    *target = value;
                    s.set_target(value);
                };
                match param {
                    echo::SYNC => *pending_sync = value.round() as u32,
                    // The one row with no smoother of its own: the time
                    // is glided in SAMPLES by the process arm, which
                    // recomputes its target every segment. Storing the
                    // millisecond here and converting there is what lets
                    // a synced echo follow the tempo without a letter.
                    echo::TIME => *time_ms = value,
                    echo::FEEDBACK => set(feedback_target, feedback),
                    echo::TONE => set(tone_target, tone),
                    echo::DRIVE => set(drive_target, drive),
                    echo::WOW => set(wow_target, wow),
                    echo::SPREAD => set(spread_target, spread),
                    echo::MIX => set(mix_target, mix),
                    // IGNORED ON AN INSERT, and that is the whole reason
                    // `aux` is a field rather than `in_gain > 0.0`: an
                    // insert's gain is unity and nothing may write it,
                    // so an automation lane that sweeps the send to zero
                    // silences an aux — correctly — and cannot mute a
                    // delay that is standing in the signal path.
                    //
                    // Crossing zero is a SHAPE change, which no letter
                    // can express: the graph builder rewires the track
                    // and this node is rebuilt with the answer.
                    echo::SEND if *aux => *in_target = value * 0.01,
                    _ => {}
                }
            }
            // The whole table, straight through: every range, every
            // clamp and the band an id belongs to are the core's own
            // business, and one door means a letter cannot be routed by
            // two different opinions about what id 17 is.
            Node::Eq { core } => core.set_param(param, value),
            // The whole table, straight through: the core owns every
            // range and every clamp, so a letter cannot be routed by two
            // different opinions about what id 4 is.
            Node::Glue { core } => core.set_param(param, value),
            Node::Clamp { core } => core.set_param(param, value),
            Node::Flint { core } => core.set_param(param, value),
            Node::Prism { core } => core.set_param(param, value),
            // The whole table, straight through, for the reason the glue
            // does it: the core owns every range and every clamp, so a
            // letter cannot be routed by two opinions about what id 3 is.
            Node::Gate { core } => core.set_param(param, value),
            Node::Strip { core } => core.set_param(param, value),
            Node::Section { core } => core.set_param(param, value),
            Node::Resyn { core } => core.set_param(param, value),
            Node::Sibyl { core } => core.set_param(param, value),
            Node::Ferric { core } => core.set_param(param, value),
            Node::Umbra { core } => core.set_param(param, value),
            Node::Tone { core } => core.set_param(param, value),
            Node::Sigil { core } => core.set_param(param, value),
            Node::Gauge { core } => core.set_param(param, value),
            // The whole table, straight through, for the reason the two
            // arms above give: one door, so a letter cannot be routed by
            // two different opinions about what id 4 is.
            Node::Utility { core } => core.set_param(param, value),
            // Every knob is a live letter. The LEVEL is not lifted out
            // to the node's ramp the way the poly's gain is: this
            // instrument applies its own, after a texture chain that is
            // part of the sound rather than after it, and moving the
            // fader outside that would change what the warmth stage is
            // being driven with.
            Node::Haze { voices, .. } => {
                let Some(value) = crate::params::clamp(crate::params::haze::TABLE, param, value)
                else {
                    return;
                };
                voices.set_param(param, value);
            }
            Node::Tine {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::tine::TABLE, param, value)
                else {
                    return;
                };
                // LEVEL is the node's, because the node owns the ramp;
                // every other row is the instrument's.
                if param == crate::params::tine::LEVEL {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Scomp {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::scomp::TABLE, param, value)
                else {
                    return;
                };
                if param == crate::params::scomp::LEVEL {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }

            Node::Stab {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::stab::TABLE, param, value)
                else {
                    return;
                };
                if param == crate::params::stab::LEVEL {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Quad {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::quad::TABLE, param, value)
                else {
                    return;
                };
                if param == crate::params::quad::LEVEL {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Brick {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::brick::TABLE, param, value)
                else {
                    return;
                };
                if param == crate::params::brick::LEVEL {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Poly {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::poly::TABLE, param, value)
                else {
                    return;
                };
                // Gain is the node's, because the node owns the ramp; every
                // other row belongs to the instrument and goes straight
                // there. One table, one clamp, no second opinion about
                // ranges anywhere in this arm.
                if param == crate::params::poly::GAIN {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Loom {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(crate::params::loom::TABLE, param, value)
                else {
                    return;
                };
                if param == crate::params::loom::GAIN {
                    *target_gain = value;
                }
                voices.set_param(param, value);
            }
            Node::Seq {
                target_gain,
                voices,
                ..
            } => {
                let Some(value) = crate::params::clamp(seq::TABLE, param, value) else {
                    return;
                };
                match param {
                    seq::GAIN => *target_gain = value,
                    // The times belong to the instrument, so the letter
                    // goes to the instrument. The node no longer holds a
                    // sample rate to convert against — the voice bank
                    // does, which is the only place it was ever used.
                    seq::ATTACK => voices.set_attack_ms(value),
                    seq::RELEASE => voices.set_release_ms(value),
                    _ => {}
                }
            }
            Node::AudioClip {
                target_gain,
                fade_in,
                fade_out,
                fade_in_curve,
                fade_out_curve,
                timeline_frames,
                ..
            } => {
                let Some(value) = crate::params::clamp(clip::TABLE, param, value) else {
                    return;
                };
                // A fade arrives in FRAMES and is clamped to the span it
                // lives on, for the reason compile clamps it: a fade
                // longer than its clip never reaches full level.
                let frames = |value: f32| (value.max(0.0) as u64).min(*timeline_frames);
                match param {
                    clip::GAIN => *target_gain = value,
                    clip::FADE_IN => *fade_in = frames(value),
                    clip::FADE_OUT => *fade_out = frames(value),
                    clip::FADE_IN_CURVE => *fade_in_curve = clip::Curve::new(value),
                    clip::FADE_OUT_CURVE => *fade_out_curve = clip::Curve::new(value),
                    _ => {}
                }
            }
            Node::Pan {
                target_pan,
                target_gain,
                ..
            } => {
                if let Some(value) = crate::params::clamp(pan::TABLE, param, value) {
                    match param {
                        pan::PAN => *target_pan = value,
                        pan::GAIN => *target_gain = value,
                        _ => {}
                    }
                }
            }
        }
    }
}
/// Mint the next schedule epoch. Green zone only — compile is never called
/// from the callback. Wrapping is unreachable in any real process life, and
/// a wrap would only ever cause a stale letter to be binned one swap late.
fn next_schedule_epoch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A parameter change: "worker `node`, knob `param`, new value". 16 bytes,
/// Copy. `node` is a NodeId in packed form — letters carry the permanent name
/// tag, never a position in line.
#[derive(Debug, Clone, Copy)]
pub struct ParamChange {
    pub node: u64,
    pub param: u32,
    pub value: f32,
}

/// One authored point in a graph-owned automation lane. Musical time stays
/// in beats until compile, when the arrangement tempo table turns it into an
/// absolute sample stamp. `bend` has the Song curve's `-1..=1` meaning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32,
    pub bend: f32,
}

/// Green-zone automation resolved to one concrete node parameter.
#[derive(Debug, Clone, PartialEq)]
struct AutomationLaneSpec {
    node: NodeId,
    param: u32,
    base: f32,
    points: Vec<AutomationPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TimedAutomationPoint {
    sample: u64,
    value: f32,
    bend: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct TimedAutomationLane {
    node: u64,
    param: u32,
    base: f32,
    points: Vec<TimedAutomationPoint>,
}

impl TimedAutomationLane {
    /// Callback-safe curve evaluation. Points are compiled sorted and unique;
    /// the partition is logarithmic and bounded by immutable project data.
    fn value_at(&self, sample: u64) -> f32 {
        let Some(first) = self.points.first() else {
            return self.base;
        };
        if sample < first.sample {
            return self.base;
        }
        let upper = self.points.partition_point(|point| point.sample <= sample);
        let at = upper.saturating_sub(1);
        let a = self.points[at];
        let Some(&b) = self.points.get(at + 1) else {
            return a.value;
        };
        let span = b.sample.saturating_sub(a.sample);
        if span == 0 {
            return b.value;
        }
        let t = (sample.saturating_sub(a.sample) as f32 / span as f32).clamp(0.0, 1.0);
        let bend = a.bend.clamp(-1.0, 1.0);
        let shaped = if bend == 0.0 {
            t
        } else if bend > 0.0 {
            t.powf(1.0 + bend * 5.0)
        } else {
            1.0 - (1.0 - t).powf(1.0 + -bend * 5.0)
        };
        (b.value - a.value).mul_add(shaped, a.value)
    }
}

/// Immutable sample-stamped automation carried by a Schedule. Evaluation is
/// aligned to absolute sample quanta, so device callback size, UI frame rate,
/// and offline render block size cannot change the performance.
#[derive(Debug, Clone, Default, PartialEq)]
struct AutomationPlan {
    lanes: Vec<TimedAutomationLane>,
    /// Exact authored boundaries, shared by all lanes for cheap segment cuts.
    boundaries: Vec<u64>,
}

/// A sub-millisecond control interval at 44.1/48 kHz. Authored breakpoints
/// are additional exact boundaries; bends are piecewise-linearized within
/// these deterministic spans and node smoothers join their endpoints.
const AUTOMATION_QUANTUM: u64 = 32;

impl AutomationPlan {
    fn compile(
        lanes: &[AutomationLaneSpec],
        samples_per_beat: f64,
        timeline: Option<&crate::tempo::TempoTable>,
    ) -> Self {
        let mut compiled = Self::default();
        for lane in lanes {
            let mut points: Vec<TimedAutomationPoint> = lane
                .points
                .iter()
                .filter(|point| point.beat.is_finite() && point.value.is_finite())
                .map(|point| {
                    let beat = point.beat.max(0.0);
                    let sample = timeline.map_or_else(
                        || (beat * samples_per_beat).round().max(0.0) as u64,
                        |map| map.sample_at_beat(beat),
                    );
                    TimedAutomationPoint {
                        sample,
                        value: point.value,
                        bend: if point.bend.is_finite() {
                            point.bend.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        },
                    }
                })
                .collect();
            points.sort_by_key(|point| point.sample);
            // Same-sample points cannot describe a segment. Preserve the
            // authored last-wins rule deterministically after tempo rounding.
            let mut unique: Vec<TimedAutomationPoint> = Vec::with_capacity(points.len());
            for point in points {
                if let Some(last) = unique.last_mut()
                    && last.sample == point.sample
                {
                    *last = point;
                } else {
                    unique.push(point);
                }
            }
            if unique.is_empty() {
                continue;
            }
            compiled
                .boundaries
                .extend(unique.iter().map(|point| point.sample));
            compiled.lanes.push(TimedAutomationLane {
                node: lane.node.to_bits(),
                param: lane.param,
                base: lane.base,
                points: unique,
            });
        }
        compiled.boundaries.sort_unstable();
        compiled.boundaries.dedup();
        compiled
    }

    fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    fn next_boundary_after(&self, sample: u64) -> Option<u64> {
        self.boundaries
            .get(
                self.boundaries
                    .partition_point(|boundary| *boundary <= sample),
            )
            .copied()
    }

    /// The fixed control-grid approximation of one authored curve. If a
    /// caller's block ends inside a control span, return the point on the SAME
    /// line between the surrounding absolute boundaries; the next callback
    /// therefore resumes that line instead of inventing a new curve segment.
    fn value_at(&self, lane: &TimedAutomationLane, sample: u64) -> f32 {
        let Some(first) = lane.points.first() else {
            return lane.base;
        };
        if sample < first.sample {
            return lane.base;
        }
        let lower_quantum = sample / AUTOMATION_QUANTUM * AUTOMATION_QUANTUM;
        let upper_quantum = lower_quantum.saturating_add(AUTOMATION_QUANTUM);
        let split = self
            .boundaries
            .partition_point(|boundary| *boundary <= sample);
        let lower = self
            .boundaries
            .get(split.saturating_sub(1))
            .copied()
            .unwrap_or(0)
            .max(lower_quantum);
        let upper = self
            .boundaries
            .get(split)
            .copied()
            .unwrap_or(u64::MAX)
            .min(upper_quantum);
        if sample <= lower || upper <= lower {
            return lane.value_at(sample);
        }
        let from = lane.value_at(lower);
        let to = lane.value_at(upper);
        let along = (sample - lower) as f32 / (upper - lower) as f32;
        (to - from).mul_add(along, from)
    }
}

/// One wired input as the callback sees it: left channel, and a right channel
/// when the producer is stereo. Mono sources into stereo consumers are
/// centered by the consumer reading `l` for both sides.
pub struct InRef<'a> {
    pub l: &'a [f32],
    pub r: Option<&'a [f32]>,
}

/// A node's output buffers: mono nodes write `l` and ignore `r`; stereo nodes
/// get `Some(r)`, guaranteed by compile matching the node's channel count.
pub struct OutRef<'a> {
    pub l: &'a mut [f32],
    pub r: Option<&'a mut [f32]>,
}

/// Sum every wired input to mono, in place in `dst` — a stereo input folds
/// its right channel in on top of its left. The front half of every mono
/// effect: red-zone safe (zip-bounded, no alloc, no panic), and one place
/// instead of a copy per node.
fn sum_inputs_mono(inputs: &[InRef], dst: &mut [f32]) {
    dst.fill(0.0);
    for input in inputs {
        for (d, s) in dst.iter_mut().zip(input.l.iter()) {
            *d += *s;
        }
        if let Some(r) = input.r.as_ref() {
            for (d, s) in dst.iter_mut().zip(r.iter()) {
                *d += *s;
            }
        }
    }
}

/// Sum every wired input to STEREO, in place — stereo inputs go L->L and
/// R->R, mono inputs are centered (the same signal to both sides). The
/// stereo twin of [`sum_inputs_mono`], and the front half of every stereo
/// effect. Red-zone safe: zip-bounded, no alloc, no panic.
fn sum_inputs_stereo(inputs: &[InRef], l: &mut [f32], r: &mut [f32]) {
    l.fill(0.0);
    r.fill(0.0);
    for input in inputs {
        for (d, s) in l.iter_mut().zip(input.l.iter()) {
            *d += *s;
        }
        for (d, s) in r.iter_mut().zip(input.r.unwrap_or(input.l).iter()) {
            *d += *s;
        }
    }
}

/// One 2x chunk of a saturator lane: shape a copy, blend it back over the
/// dry across the chunk. `from`/`to` are the wet amount at the chunk's
/// first and last sample, so a mix move is stepless across chunk edges.
///
/// A free function rather than a closure because it is called once per
/// channel with disjoint slices, which a closure capturing the core could
/// not express. Red zone: bounded, allocation-free, no panic path — the
/// lengths are minned rather than asserted.
fn sat_chunk(
    shaper: &crate::dsp::shaper::Waveshaper,
    buf: &mut [f32],
    shaped: &mut [f32],
    from: f32,
    to: f32,
) {
    let len = buf.len().min(shaped.len());
    let (buf, shaped) = (&mut buf[..len], &mut shaped[..len]);
    shaped.copy_from_slice(buf);
    shaper.process(shaped);
    let mut ramp = Ramp::across(from, to, len);
    for (d, w) in buf.iter_mut().zip(shaped.iter()) {
        let m = ramp.next();
        *d = *d * (1.0 - m) + *w * m;
    }
}

/// One block's walk of a ramped parameter: the declick rule every audible
/// knob follows, written once. `next()` yields the value for the current
/// sample and THEN steps — the same order every hand-rolled ramp here used,
/// so adopting it is bit-exact. The walk never lands exactly (float drift),
/// which is why every user ends its block with `*param = target`; forgetting
/// that line is the classic drift bug, so it stays visible at the call site
/// rather than hidden in here.
#[derive(Debug, Clone, Copy)]
pub struct Ramp {
    value: f32,
    step: f32,
}

impl Ramp {
    #[inline]
    pub fn across(from: f32, to: f32, samples: usize) -> Self {
        Self {
            value: from,
            step: (to - from) / samples.max(1) as f32,
        }
    }

    // Named `next` because that is what it does, and it long predates
    // being public. It is not an iterator and never will be: an iterator
    // would be fused and optional, and the whole contract here is that it
    // advances exactly once per sample on every path.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn next(&mut self) -> f32 {
        let value = self.value;
        self.value += self.step;
        value
    }
}

/// Unity gain — the serde default for a `Pan` spec written before the
/// fader existed.
/// A clip's envelope, made safe for the callback to walk.
///
/// GREEN SIDE, at compile: the node's walk assumes the points rise and
/// stay inside the clip, and both sorting and clamping there would be an
/// allocation and an unbounded path on the audio thread. Doing it here
/// means the callback's loop is a comparison and an add.
///
/// Points past the clip's span are dropped rather than clamped onto its
/// end, where they would pile up and make the last segment's
/// interpolation divide by zero.
/// Resolve an absolute arrangement node into the piecewise timeline's sample
/// domain while retaining the scalar compiler as the one event validator.
///
/// `samples_per_beat` is only a carrier unit here: multiplying the rewritten
/// beat by it in `compile_events` returns the map's absolute sample. Valid
/// one-shot subloops are expanded before mapping because a nonlinear tempo
/// map cannot be applied correctly after beat-relative repetition.
fn resolve_timeline_spec(
    spec: &mut NodeSpec,
    timeline: &crate::tempo::TempoTable,
    samples_per_beat: f64,
) {
    let rewrite_notes = |notes: &mut Vec<Note>, subloops: &mut Vec<SubLoop>| {
        if !subloops.is_empty()
            && let Ok(expanded) = expand_subloops(notes, subloops)
        {
            *notes = expanded;
            subloops.clear();
        }
        for note in notes {
            let start = note.start_beats;
            let end = start + note.len_beats;
            let on = timeline.sample_at_beat(start);
            let off = timeline.sample_at_beat(end);
            note.start_beats = on as f64 / samples_per_beat;
            note.len_beats = off.saturating_sub(on) as f64 / samples_per_beat;
        }
    };

    match spec {
        NodeSpec::Seq {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Tine {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Scomp {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Stab {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Quad {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Brick {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Poly {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Loom {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Haze {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Kick {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Acid {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Sampler {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Snare {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Tom {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Hat {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        }
        | NodeSpec::Handclap {
            notes,
            subloops,
            loop_len_beats: None,
            ..
        } => rewrite_notes(notes, subloops),
        NodeSpec::AudioClip {
            start_beats,
            length_beats,
            ..
        } => {
            let start = *start_beats;
            let on = timeline.sample_at_beat(start);
            if let Some(length) = *length_beats {
                let off = timeline.sample_at_beat(start + length);
                *length_beats = Some(off.saturating_sub(on) as f64 / samples_per_beat);
            }
            *start_beats = on as f64 / samples_per_beat;
        }
        _ => {}
    }
}

fn sorted_envelope(points: &[(u64, f32)], span: u64) -> Vec<(u64, f32)> {
    let mut out: Vec<(u64, f32)> = points
        .iter()
        .filter(|(at, gain)| *at <= span && gain.is_finite())
        .map(|(at, gain)| (*at, gain.clamp(0.0, 4.0)))
        .collect();
    out.sort_by_key(|(at, _)| *at);
    out.dedup_by_key(|(at, _)| *at);
    out
}

/// Map one frame boundary between sample-rate domains without passing a
/// large project position through `f32`. Boundaries are rounded, not lengths:
/// callers map both ends and subtract, so adjoining edits cannot acquire a
/// one-frame hole from two independent rounds.
fn frame_at_rate(frame: u64, from_rate: u32, to_rate: u32) -> u64 {
    if from_rate == 0 || to_rate == 0 || from_rate == to_rate {
        return frame;
    }
    let numerator = u128::from(frame)
        .saturating_mul(u128::from(to_rate))
        .saturating_add(u128::from(from_rate / 2));
    (numerator / u128::from(from_rate)).min(u128::from(u64::MAX)) as u64
}

/// Open an arrangement stream at the device rate. Imports ordinarily arrive
/// pre-converted, so this is a no-op. It is still an engine invariant rather
/// than a UI convention: reopening a 48 kHz project on a 44.1 kHz interface
/// may never detune every take. A mismatched WAV is converted into the same
/// deterministic float cache the importer uses, green-side, then streamed by
/// creek exactly like a native-rate file.
fn audio_stream_at_rate(
    path: &std::path::Path,
    device_rate: u32,
) -> Option<(ReadDiskStream<SymphoniaDecoder>, u64, u32)> {
    let mut opts = creek::ReadStreamOptions::<SymphoniaDecoder>::default();
    // One permanent cache for the region head and one for a later loop brace.
    // The callback can then wrap either place without depending on disk seek
    // latency. This is bounded stream-owned memory prepared off the callback.
    opts.num_caches = 2;
    let direct = ReadDiskStream::<SymphoniaDecoder>::new(path.to_path_buf(), 0, opts).ok()?;
    let original_frames = direct.info().num_frames as u64;
    let original_rate = direct.info().sample_rate.unwrap_or(device_rate);
    if original_rate == device_rate || original_rate == 0 || device_rate == 0 {
        return Some((direct, original_frames, original_rate.max(device_rate)));
    }
    drop(direct);

    let converted = crate::library::import_wav(path, device_rate).ok()?;
    let mut opts = creek::ReadStreamOptions::<SymphoniaDecoder>::default();
    opts.num_caches = 2;
    let stream = ReadDiskStream::<SymphoniaDecoder>::new(converted.path, 0, opts).ok()?;
    if stream.info().sample_rate != Some(device_rate) {
        return None;
    }
    Some((stream, original_frames, original_rate))
}

/// Serde fallbacks for a reverb written before the network replaced the
/// Freeverb. A project from then has a size, a damp and a mix and knows
/// nothing else; these are what the rest becomes.
fn reverb_decay_default() -> f32 {
    1.8
}

fn reverb_diffusion_default() -> f32 {
    0.8
}

fn unity() -> f32 {
    1.0
}

/// Green-zone construction for a dry path that keeps a declared latency.
fn latency_passthrough_node(samples: usize) -> Node {
    if samples == 0 {
        return Node::Mixer {
            gain: 1.0,
            target_gain: 1.0,
        };
    }
    let len = crate::dsp::delay::buffer_len(samples);
    let mut left = crate::dsp::delay::DelayLine::new();
    let mut right = crate::dsp::delay::DelayLine::new();
    left.prepare(samples);
    right.prepare(samples);
    left.set_delay(samples as f32);
    right.set_delay(samples as f32);
    Node::Delay {
        left,
        right,
        left_buf: vec![0.0; len],
        right_buf: vec![0.0; len],
    }
}

/// A buffer reference of 1 or 2 arena slots (a mono or stereo edge). Stored
/// as a fixed pair so the hot loop never chases a Vec for channel data.
/// Internally this is a slot LIST — widening past stereo later is a number
/// change, not a redesign.
#[derive(Clone, Copy)]
struct ChanBuf {
    slots: [SlotId; 2],
    ch: u8,
}

/// One row of the chore chart: run node `node`, reading `ins`, writing `out`.
struct Step {
    node: usize,
    out: ChanBuf,
    ins: Vec<ChanBuf>,
}

/// A compiled, immutable execution plan plus its working memory. Built in the
/// green zone, executed in the red zone, never mutated structurally after
/// compile. Swapped whole via ring buffer.
pub struct Schedule {
    /// Which COMPILE this schedule came from. Unique for the process.
    ///
    /// Node name tags are only unique within one `thunderdome` arena, and
    /// every compile starts a fresh one — so slot 0 / generation 1 in this
    /// schedule and slot 0 / generation 1 in the next are unrelated nodes
    /// wearing the same tag. The generation compare in [`Self::apply`]
    /// cannot see that, because it is the same number. A letter still in
    /// flight across a schedule swap would be delivered to whatever now
    /// occupies its slot.
    ///
    /// The epoch is what makes the tag unambiguous ACROSS compiles. Letters
    /// carry the epoch they were addressed under and the callback bins the
    /// ones that no longer match.
    epoch: u64,
    /// Piecewise musical clock for an arrangement schedule. Allocated and
    /// cloned only at compile; the callback performs read-only lookups and
    /// the whole schedule retires to the green thread.
    timeline: Option<crate::tempo::TempoTable>,
    nodes: Vec<Node>,
    steps: Vec<Step>,
    arena: Arena,
    /// Which node's slots feed the device output when the walk finishes.
    /// None = no output node set: silence.
    output_slot: Option<ChanBuf>,
    /// Name-tag directory, indexed by thunderdome slot: (generation, dense
    /// index into `nodes`). Generation 0 marks an empty entry — thunderdome
    /// guarantees real generations are never 0. Rebuilt from scratch every
    /// compile, so it can never drift from `nodes`.
    slot_table: Vec<(u32, u32)>,
    /// Meter slot per step, or [`NO_METER`]. Indexed BY STEP, so the hot
    /// loop answers "is this step metered?" with one bounded load and no
    /// search. Built green-side at compile.
    step_meter: Vec<u8>,
    /// Peak per meter slot since the last reset, accumulated across the
    /// segments of a block: `run` is called once per SEGMENT, so a block's
    /// peak is the max over its segments.
    ///
    /// TWO SIDES, because a stereo track that is not centred is two
    /// different levels and a single number cannot say which. A mono node
    /// reports the same figure on both — that is the truth about a mono
    /// source rather than a second meter invented to fill a space.
    peaks_l: [f32; MAX_METERS],
    peaks_r: [f32; MAX_METERS],
    /// Tap slot per step, or [`NO_METER`]. The readout twin of
    /// `step_meter`, and read the same way: one bounded load per step,
    /// no search.
    step_tap: Vec<u8>,
    /// What each tapped device said this block.
    readouts: [Readout; MAX_METERS],
    /// Telemetry slot per step, or [`NO_METER`]: the console's channel,
    /// wider than the meters' so every section on every strip can
    /// report, in its own slot space.
    step_telemetry: Vec<u8>,
    /// What each telemetered section said this block.
    telemetry: [Readout; MAX_TELEMETRY],
    /// Modulation, compiled. Evaluated at the top of every segment, before
    /// the walk, so the values a node reads this segment are this
    /// segment's.
    modulation: ModPlan,
    /// Sample-stamped Song automation, evaluated inside the callback rather
    /// than sampled by UI repaint or offline render-block cadence.
    automation: AutomationPlan,
    /// The knob behind every effect parameter some note locks: what a
    /// restore returns to. Letters move it, so a knob turned mid-playback
    /// is heard on every unlocked note, as with the voice's own locks.
    fx_bases: Vec<FxBase>,
    /// Samples of latency between this schedule's inputs and its output —
    /// the deepest path, after compensation has made every path agree.
    ///
    /// Reported, not compensated: the final output cannot be made earlier.
    /// It is what a transport offsets its recording by and what a latency
    /// readout shows.
    latency: usize,
}

/// What one device has to say about itself over a block.
///
/// A meter reports a node's OUTPUT LEVEL, which is what a mixer strip
/// wants and what every track already gets. A dynamics processor has a
/// second thing to say that no output level can carry: how much it is
/// working. That is this.
///
/// Both figures are the BLOCK's extreme, not its last sample. The UI
/// reads these at frame rate and the engine computes them per sample, so
/// reporting the newest value would miss exactly the fast transient a
/// compressor exists to catch — a 3 ms grab between two repaints would
/// simply never be drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readout {
    /// The loudest the device heard this block, in dBFS.
    pub level_db: f32,
    /// The MOST gain reduction it applied this block, in dB. Zero is
    /// none and negative is reduction, the same sign the gain computer
    /// works in.
    pub reduction_db: f32,
    /// Up to three BANDS' worth of the same figure, for a device that
    /// has bands — negative for reduction, positive for lift.
    ///
    /// Zero on everything else, and that is not a special case: a device
    /// with one band has already said everything it has to say in
    /// `reduction_db`, and three zeroes draw nothing.
    pub bands: [f32; 3],
}

impl Default for Readout {
    fn default() -> Self {
        Self {
            level_db: crate::dsp::dynamics::FLOOR_DB,
            reduction_db: 0.0,
            bands: [0.0; 3],
        }
    }
}

/// "This step is not metered." `MAX_METERS` is 32, so 255 cannot collide
/// with a real slot.
const NO_METER: u8 = u8::MAX;

impl Schedule {
    /// Beat at one absolute transport sample. Arrangement schedules answer
    /// from their compiled tempo map; session schedules retain the scalar
    /// transport clock they have always used.
    pub fn beat_at(&self, position: u64, fallback: crate::audio::transport::TimeMap) -> f64 {
        self.timeline.as_ref().map_or_else(
            || fallback.samples_to_beats(position),
            |timeline| timeline.beat_at_sample(position),
        )
    }

    /// Beats advanced per sample in the constant-tempo span at `position`.
    pub fn beats_per_sample_at(
        &self,
        position: u64,
        fallback: crate::audio::transport::TimeMap,
    ) -> f64 {
        self.timeline.as_ref().map_or_else(
            || fallback.beats_per_sample(),
            |timeline| timeline.beats_per_sample_at(position),
        )
    }

    /// Bound one transport segment so it cannot cross a tempo mark. The
    /// returned value is always in `1..=remaining` when `remaining > 0`.
    pub fn frames_until_tempo_change(&self, position: u64, remaining: usize) -> usize {
        if remaining == 0 {
            return 0;
        }
        self.timeline
            .as_ref()
            .and_then(|timeline| timeline.next_change_after(position))
            .map_or(remaining, |boundary| {
                let distance = boundary.saturating_sub(position).max(1);
                if distance >= remaining as u64 {
                    remaining
                } else {
                    distance as usize
                }
            })
    }

    /// Bound one transport segment at every timing authority the compiled
    /// graph owns: tempo marks, authored automation points, and the absolute
    /// automation control grid. The grid is anchored to timeline sample zero,
    /// so changing callback or bounce block size cannot move a curve.
    pub fn frames_until_control_change(&self, position: u64, remaining: usize) -> usize {
        let mut limited = self.frames_until_tempo_change(position, remaining);
        if limited == 0 || self.automation.is_empty() {
            return limited;
        }
        let to_quantum = AUTOMATION_QUANTUM - position % AUTOMATION_QUANTUM;
        limited = limited.min(to_quantum.min(usize::MAX as u64) as usize);
        if let Some(boundary) = self.automation.next_boundary_after(position) {
            limited = limited.min(
                boundary
                    .saturating_sub(position)
                    .max(1)
                    .min(usize::MAX as u64) as usize,
            );
        }
        limited.max(1)
    }

    /// GREEN ZONE ONLY — blocks, but never forever. Wait until every disk
    /// stream is buffered so an offline render cannot contain buffering
    /// silence. Returns false on timeout — a stalled stream must surface as
    /// an error upstream, not as an infinite hang (creek's own
    /// block_until_ready spins unboundedly if the server stalls). Failed or
    /// missing streams are skipped (they render silence by design).
    pub fn wait_streams_ready(&mut self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        for node in &mut self.nodes {
            if let Node::AudioClip {
                stream: Some(st),
                failed,
                ..
            } = node
                && !*failed
            {
                loop {
                    match st.is_ready() {
                        Ok(true) => break,
                        Ok(false) => {
                            if std::time::Instant::now() >= deadline {
                                return false;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Err(_) => {
                            *failed = true;
                            break;
                        }
                    }
                }
            }
        }
        true
    }

    /// How many arena slots this schedule allocated. A long chain compiles to
    /// 2 regardless of length; the working set is what fits in cache.
    pub fn arena_slots(&self) -> usize {
        self.arena.chunks.len() / self.arena.chunks_per_slot
    }

    /// Red zone. Deliver one letter by name tag: one bounded index into the
    /// slot table, one generation compare. A letter for a departed node finds
    /// generation mismatch (or an empty slot) and is binned — misdelivery is
    /// structurally impossible.
    /// A letter for a MODULATED parameter is diverted to its base value
    /// instead: the node's value belongs to the modulation plan, which
    /// rewrites it every segment, so a letter landing on the node directly
    /// would be erased a moment later — the fader would go dead under a
    /// running LFO. Setting the base is what "modulation is relative"
    /// means in one line of code.
    pub fn apply(&mut self, change: ParamChange) {
        // `Node::apply` bins non-finite letters, and a modulation base must
        // be held to the same standard — it is the SAME letter, taking a
        // different door. A NaN base is worse than a NaN node value: it
        // cannot be clamped away downstream, so every later segment fails
        // its finite check and pins the parameter to the bottom of its
        // range. The track goes silent and STAYS silent, through a bypass,
        // through a solo, until some other finite value happens to arrive.
        if !change.value.is_finite() {
            return;
        }
        if let Some(base) = self
            .fx_bases
            .iter_mut()
            .find(|b| b.node == change.node && b.param == change.param)
        {
            base.base = change.value;
            if !base.locked {
                base.live = change.value;
            }
        }
        let value = self
            .fx_bases
            .iter()
            .find(|b| b.node == change.node && b.param == change.param)
            .map_or(change.value, |base| base.live);
        self.apply_to(change.node, change.param, value);
    }

    /// Land a value on a node's parameter, or on its modulation base
    /// when the parameter is modulated. The letter path and the effect
    /// lock path both end here.
    fn apply_to(&mut self, node: u64, param: u32, value: f32) {
        Self::land(
            &mut self.nodes,
            &mut self.modulation,
            &self.slot_table,
            node,
            param,
            value,
        );
    }

    /// `apply_to` over the schedule's parts, so the walk can deliver a
    /// lock while it holds its steps.
    fn land(
        nodes: &mut [Node],
        modulation: &mut ModPlan,
        slot_table: &[(u32, u32)],
        node: u64,
        param: u32,
        value: f32,
    ) {
        let slot = node as u32 as usize;
        let generation = (node >> 32) as u32;
        if let Some(&(g, dense)) = slot_table.get(slot)
            && g == generation
        {
            if let Some(base) = modulation.base_slot(dense as usize, param) {
                *base = value;
            } else if let Some(node) = nodes.get_mut(dense as usize) {
                node.apply(param, value);
            }
        }
    }

    /// Land an effect lock after modulation has already been evaluated for
    /// this segment. A modulated target must be recomposed from the new base
    /// and the CURRENT wire outputs now; the ordinary `land` door correctly
    /// waits for evaluation and is used by letters/automation before it.
    fn land_effect_lock(
        nodes: &mut [Node],
        modulation: &mut ModPlan,
        slot_table: &[(u32, u32)],
        node: u64,
        param: u32,
        value: f32,
    ) {
        let slot = node as u32 as usize;
        let generation = (node >> 32) as u32;
        if let Some(&(g, dense)) = slot_table.get(slot)
            && g == generation
            && let Some(node) = nodes.get_mut(dense as usize)
        {
            let value = modulation
                .rebase_evaluated_target(dense as usize, param, value)
                .unwrap_or(value);
            node.apply(param, value);
        }
    }

    /// Which compile this schedule came from. A letter addressed under a
    /// different epoch names a node that no longer exists, whatever its
    /// slot and generation say.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Red zone: the LEFT peaks accumulated since the last
    /// [`Self::clear_peaks`], one per meter slot. Linear amplitude, where
    /// 1.0 is full scale.
    pub fn peaks_l(&self) -> &[f32; MAX_METERS] {
        &self.peaks_l
    }

    /// The right side of the same reading.
    pub fn peaks_r(&self) -> &[f32; MAX_METERS] {
        &self.peaks_r
    }

    /// One slot's level as a single number: the louder side. What a
    /// caller with room for one mark asks for, and what everything that
    /// metered a track before it had sides still means.
    pub fn peak(&self, slot: usize) -> f32 {
        let left = self.peaks_l.get(slot).copied().unwrap_or(0.0);
        let right = self.peaks_r.get(slot).copied().unwrap_or(0.0);
        left.max(right)
    }

    /// What every tapped device said this block.
    pub fn readouts(&self) -> &[Readout; MAX_METERS] {
        &self.readouts
    }

    /// Red zone: start a new measurement window. Called once per block,
    /// before its segments run — a fixed-size fill, no allocation.
    ///
    /// The peaks are handed to the modulation plan on the way out, because
    /// this is the last moment they exist: a follower detects the PREVIOUS
    /// block's level, since the current one is not known until after the
    /// walk. That is one block (~5ms at 256 frames) of detector latency,
    /// which is what every sidechain has.
    pub fn clear_peaks(&mut self) {
        // A follower detects a LEVEL, not a side, so modulation is handed
        // the louder of the two. A fixed thirty-two-step fill on the
        // stack: no allocation, and bounded like everything else here.
        let mut combined = [0.0f32; MAX_METERS];
        for (slot, out) in combined.iter_mut().enumerate() {
            *out = self.peaks_l[slot].max(self.peaks_r[slot]);
        }
        self.modulation.note_peaks(&combined);
        self.peaks_l = [0.0; MAX_METERS];
        self.peaks_r = [0.0; MAX_METERS];
        self.readouts = [Readout::default(); MAX_METERS];
        self.telemetry = [Readout::default(); MAX_TELEMETRY];
    }

    /// What every telemetered section said this block, by slot.
    pub fn telemetry(&self) -> &[Readout; MAX_TELEMETRY] {
        &self.telemetry
    }

    /// Total latency from input to output, in samples.
    ///
    /// Computed green-side from the compiled path and each device's declared
    /// latency; the latency-bearing kernels pin those declarations with
    /// impulse measurements. This is distinct from a backend's stream
    /// latency, which is merely reported by the audio API. A recorder that
    /// needs both must read this before handing the schedule to the callback.
    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Red zone: stop every live CLAP processor before this schedule crosses
    /// back to green for destruction. Bounded by the immutable node count;
    /// basedrop makes the later ownership drop a lock-free enqueue.
    pub fn prepare_for_retirement(&mut self) {
        for node in &mut self.nodes {
            if let Node::Clap { processor } = node {
                processor.prepare_for_retirement();
            }
        }
    }

    /// Red zone: this segment's modulation telemetry, for the UI's scopes.
    pub fn modulation(&self) -> &ModPlan {
        &self.modulation
    }

    /// Red zone: inherit the retiring schedule's modulation memory — free
    /// LFO phase, wire lag, follower levels — so a recompile mid-playback
    /// is not heard. Call this on the NEW schedule, with the old one, at
    /// the moment of the swap.
    pub fn adopt_modulation_continuity(&mut self, old: &Schedule) {
        self.modulation.adopt_continuity(&old.modulation);
    }

    /// Red zone: deliver one modulation letter. The live door for knob
    /// drags on a wire's depth or an LFO's rate, which must be heard now
    /// rather than at the next debounced schedule swap.
    pub fn apply_mod_edit(&mut self, edit: ModEdit) {
        self.modulation.apply_edit(edit);
    }

    fn apply_timeline_automation(&mut self, ctx: &ProcessCtx<'_>) {
        if self.automation.is_empty() {
            return;
        }
        // Nodes ramp from the endpoint reached by the previous deterministic
        // control segment to this one's endpoint. A stopped transport reads
        // exactly at its frozen cursor instead of inventing forward motion.
        let sample = if ctx.playing {
            ctx.position.saturating_add(ctx.len as u64)
        } else {
            ctx.position
        };
        let Schedule {
            automation,
            fx_bases,
            nodes,
            modulation,
            slot_table,
            ..
        } = self;
        for lane in &automation.lanes {
            let value = automation.value_at(lane, sample);
            if !value.is_finite() {
                continue;
            }
            if let Some(base) = fx_bases
                .iter_mut()
                .find(|base| base.node == lane.node && base.param == lane.param)
            {
                base.base = value;
                if base.locked {
                    Self::land(
                        nodes, modulation, slot_table, lane.node, lane.param, base.live,
                    );
                    continue;
                }
                base.live = value;
            }
            Self::land(nodes, modulation, slot_table, lane.node, lane.param, value);
        }
    }

    /// Red zone: walk the chart in dependency order for ONE transport
    /// segment, then copy the output node's slot to every device channel.
    /// Buffers are planar; the segment is `ctx.offset..ctx.offset + ctx.len`
    /// within the block. All node buffers are `ctx.len` long.
    pub fn run(&mut self, output: &mut [f32], ctx: &ProcessCtx<'_>) {
        let len = ctx.len;

        self.apply_timeline_automation(ctx);

        // Modulation first, so every node reads THIS segment's values. A
        // segment is the right grain rather than a block: a loop wrap ends
        // one segment and starts another, and a synced LFO reading
        // `ctx.beat` therefore lands on the wrapped beat instead of
        // sliding across the seam.
        //
        // The values go in through `Node::apply` — the same door letters
        // use — so a modulated parameter keeps the per-block ramp that
        // declicks it, and no node needs to know modulation exists.
        if !self.modulation.is_empty() {
            self.modulation.evaluate(ctx.beat as f32, len);
            for (dense, param, value) in self.modulation.writes() {
                if let Some(node) = self.nodes.get_mut(dense) {
                    node.apply(param, value);
                }
            }
        }

        for (step_index, step) in self.steps.iter().enumerate() {
            // Gather input channel slices into a fixed stack array. compile()
            // caps inputs at MAX_NODE_INPUTS, and the slot allocator
            // guarantees no input slot aliases this step's output slots: the
            // out slots are drawn from the free list while every input still
            // holds its slots, and inputs release only after their last
            // consumer's step.
            let n_inputs = step.ins.len().min(MAX_NODE_INPUTS);
            let mut gathered: [InRef<'_>; MAX_NODE_INPUTS] =
                std::array::from_fn(|_| InRef { l: &[], r: None });
            for (dst, cb) in gathered.iter_mut().zip(step.ins.iter()) {
                // SAFETY: input slots != output slots (allocator invariant),
                // pointers are in-arena, borrows end before the next step.
                unsafe {
                    let sf = self.arena.slot_frames.min(len);
                    dst.l = std::slice::from_raw_parts(self.arena.slot_ptr(cb.slots[0]), sf);
                    dst.r = (cb.ch > 1)
                        .then(|| std::slice::from_raw_parts(self.arena.slot_ptr(cb.slots[1]), sf));
                }
            }

            // Output channels: distinct slots by construction, so the two
            // &mut cannot alias each other or any input.
            // SAFETY: as above, plus out.slots[0] != out.slots[1].
            let (l, r) = unsafe {
                let sf = self.arena.slot_frames.min(len);
                let l = std::slice::from_raw_parts_mut(self.arena.slot_ptr(step.out.slots[0]), sf);
                let r = (step.out.ch > 1).then(|| {
                    std::slice::from_raw_parts_mut(self.arena.slot_ptr(step.out.slots[1]), sf)
                });
                (l, r)
            };
            let mut out = OutRef { l, r };
            self.nodes[step.node].process(&gathered[..n_inputs], &mut out, ctx);

            // The effect locks this node's clock fired: delivered now,
            // before the nodes downstream render this block. A restore
            // returns the knob; a lock on a knob nobody registered is
            // binned, like a letter for a departed node.
            if let Some((bus, fired)) = self.nodes[step.node].clock_mut().map(PatternClock::take_fx)
            {
                for lock in &bus[..fired.min(FX_BUS)] {
                    let value = match self
                        .fx_bases
                        .iter_mut()
                        .find(|b| b.node == lock.node && b.param == lock.param)
                    {
                        Some(base) if lock.restore && lock.value > 0.0 => {
                            base.live += (base.base - base.live) * lock.value.clamp(0.0, 1.0);
                            if lock.value >= 1.0 {
                                base.locked = false;
                            }
                            base.live
                        }
                        Some(base) if lock.restore => {
                            base.live = base.base;
                            base.locked = false;
                            base.live
                        }
                        Some(base) => {
                            base.live = lock.value;
                            base.locked = true;
                            base.live
                        }
                        None if lock.restore => continue,
                        None => lock.value,
                    };
                    if value.is_finite() {
                        Self::land_effect_lock(
                            &mut self.nodes,
                            &mut self.modulation,
                            &self.slot_table,
                            lock.node,
                            lock.param,
                            value,
                        );
                    }
                }
            }

            // Metering happens HERE, not after the walk: the allocator
            // recycles a node's slots as soon as its last consumer has run,
            // so this is the only moment this step's output exists. Bounded
            // by the slices themselves; `max` on every path, so a block's
            // peak is the max over its segments.
            //
            // `f32::max` returns the OTHER operand for a NaN, which cuts
            // both ways and is worth knowing: no NaN can ever lodge in
            // `peaks` (good), but an all-NaN buffer meters as silence
            // rather than as trouble (a diagnosis gap, not a hazard). The
            // per-sample test that would catch it is not worth its cost in
            // this loop — the compile and letter doors are both guarded
            // instead, which is where a non-finite level can come from.
            if let Some(&slot) = self.step_meter.get(step_index)
                && slot != NO_METER
            {
                let slot = slot as usize;
                let mut hot_l = self.peaks_l.get(slot).copied().unwrap_or(0.0);
                for sample in out.l.iter() {
                    hot_l = hot_l.max(sample.abs());
                }
                let mut hot_r = self.peaks_r.get(slot).copied().unwrap_or(0.0);
                match out.r.as_deref() {
                    Some(right) => {
                        for sample in right.iter() {
                            hot_r = hot_r.max(sample.abs());
                        }
                    }
                    // Mono: both sides carry the same signal by the time
                    // it reaches a speaker, so both sides report it.
                    None => hot_r = hot_l,
                }
                if let Some(peak) = self.peaks_l.get_mut(slot) {
                    *peak = hot_l;
                }
                if let Some(peak) = self.peaks_r.get_mut(slot) {
                    *peak = hot_r;
                }
            }

            // And what the device itself has to say, if it is tapped and
            // has anything. Accumulated across the block's segments the
            // way the peak above is: the LOUDEST it heard and the MOST it
            // reduced, so a transient inside one segment survives to the
            // frame that draws it.
            if let Some(&slot) = self.step_tap.get(step_index)
                && slot != NO_METER
                && let Some(readout) = self.readouts.get_mut(slot as usize)
                && let Some(node) = self.nodes.get(step.node)
                && let Some(said) = node.readout()
            {
                readout.level_db = readout.level_db.max(said.level_db);
                // Reduction is negative, so "most" is the minimum.
                readout.reduction_db = readout.reduction_db.min(said.reduction_db);
            }
            // The console's telemetry, accumulated the same way: the
            // loudest and the most reduced across the block's segments.
            if let Some(&slot) = self.step_telemetry.get(step_index)
                && slot != NO_METER
                && let Some(readout) = self.telemetry.get_mut(slot as usize)
                && let Some(node) = self.nodes.get(step.node)
                && let Some(said) = node.readout()
            {
                readout.level_db = readout.level_db.max(said.level_db);
                readout.reduction_db = readout.reduction_db.min(said.reduction_db);
                for (mine, theirs) in readout.bands.iter_mut().zip(said.bands) {
                    *mine = if theirs < 0.0 {
                        mine.min(theirs)
                    } else {
                        mine.max(theirs)
                    };
                }
            }
        }

        let device_ch = output.len() / ctx.block_frames.max(1);
        match self.output_slot {
            Some(cb) => {
                for ch in 0..device_ch {
                    // Stereo output node: L,R across device channels; mono:
                    // duplicated. Extra device channels cycle.
                    let slot = cb.slots[ch % cb.ch.max(1) as usize];
                    let src_ptr = self.arena.slot_ptr(slot);
                    let start = ch * ctx.block_frames + ctx.offset;
                    let dst = &mut output[start..start + len];
                    // SAFETY: read-only view of a slot; no step is running.
                    let src = unsafe { std::slice::from_raw_parts(src_ptr, len) };
                    for (d, s) in dst.iter_mut().zip(src.iter()) {
                        *d = *s;
                    }
                }
            }
            None => {
                for ch in 0..device_ch {
                    let start = ch * ctx.block_frames + ctx.offset;
                    output[start..start + len].fill(0.0);
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileError {
    #[error("the graph has a cycle — audio cannot flow in a loop without a delay")]
    Cycle,
    #[error("a wire references a node that no longer exists")]
    DanglingWire,
    #[error("a node has more than {MAX_NODE_INPUTS} inputs")]
    TooManyInputs,
    #[error("the output node no longer exists")]
    DanglingOutput,
    #[error("an automation lane references a node that no longer exists")]
    DanglingAutomation,
    #[error("a subloop is malformed (end <= start, repeats 0 or > 64) or overlaps another")]
    BadSubLoop,
    #[error("a clip loop length must be a positive, finite number of beats")]
    BadClipLen,
    #[error("tempo table sample rate does not match graph compile sample rate")]
    TempoSampleRateMismatch,
    #[error("live CLAP device `{plugin_id}` has no trusted runtime binding")]
    MissingClapRuntime { plugin_id: String },
    #[error("CLAP device `{plugin_id}` could not be prepared: {detail}")]
    ClapPrepare { plugin_id: String, detail: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClapBindError {
    #[error("CLAP factory identity does not match the persisted device")]
    IdentityMismatch,
}

impl NodeSpec {
    /// The notes a timeline-locked instrument plays, for a compiler that
    /// finishes them after the rest of the chain is placed.
    pub fn notes_mut(&mut self) -> Option<&mut Vec<Note>> {
        match self {
            NodeSpec::Seq { notes, .. }
            | NodeSpec::Tine { notes, .. }
            | NodeSpec::Scomp { notes, .. }
            | NodeSpec::Stab { notes, .. }
            | NodeSpec::Quad { notes, .. }
            | NodeSpec::Brick { notes, .. }
            | NodeSpec::Poly { notes, .. }
            | NodeSpec::Loom { notes, .. }
            | NodeSpec::Haze { notes, .. }
            | NodeSpec::Kick { notes, .. }
            | NodeSpec::Acid { notes, .. }
            | NodeSpec::Sampler { notes, .. }
            | NodeSpec::Snare { notes, .. }
            | NodeSpec::Tom { notes, .. }
            | NodeSpec::Hat { notes, .. }
            | NodeSpec::Handclap { notes, .. } => Some(notes),
            _ => None,
        }
    }
}

/// Green zone: the editable graph description — nodes plus wires. Compile
/// turns it into a runnable plan or refuses with a reason.
///
/// Nodes live in a generational arena and are addressed by `NodeId` — the id
/// handed out by `push` stays valid across edits to *other* nodes, and goes
/// permanently dead on `remove`.
#[derive(Default)]
pub struct GraphSpec {
    nodes: thunderdome::Arena<NodeSpec>,
    /// Green-only recipes for live CLAP specs, keyed by the same permanent
    /// name tag as the node. Kept outside `NodeSpec` so that persisted specs
    /// stay cloneable and never pretend a unique native instance can clone.
    clap_factories: HashMap<NodeId, crate::clap_host::ClapNodeFactory>,
    /// Wires: (from, to). Summing order at a mixer is wire insertion order —
    /// irrelevant to the sum, stable for debugging.
    wires: Vec<(NodeId, NodeId)>,
    /// Which node feeds the speakers. None = silence.
    output: Option<NodeId>,
    /// Insertion order, for stable dense layout across compiles.
    order: Vec<NodeId>,
    /// Meter taps: (slot, node). The slot is the CALLER's index — a track
    /// number — so a muted track that compiles to nothing leaves its meter
    /// reading silence instead of shifting every meter after it.
    meters: Vec<(usize, NodeId)>,
    /// Device readout taps: (slot, node), in the meters' slot space. What
    /// a dynamics processor reports about ITSELF rather than about its
    /// output — see [`Readout`].
    taps: Vec<(usize, NodeId)>,
    /// The console's telemetry taps: (slot, node), in their own slot
    /// space of [`MAX_TELEMETRY`], so every section of every strip can
    /// report without taking a meter from a track.
    telemetry: Vec<(usize, NodeId)>,
    /// Modulation, with each wire's `(track, parameter)` target already
    /// resolved to a node and param id by whoever built the graph — the
    /// only place that knows both halves.
    modulation: ModSpec,
    /// Song automation with target strings already resolved to permanent
    /// graph node ids. Compile stamps its beats through the same tempo map as
    /// notes and audio clips.
    automation: Vec<AutomationLaneSpec>,
    /// The knob behind every effect parameter a note locks, so the
    /// schedule can restore it. Registered by the compiler that knows
    /// the device's value; a lock on a parameter nobody registered is
    /// applied but never restored.
    fx_bases: Vec<(NodeId, u32, f32)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NodeSpec {
    Silence,
    Sine {
        freq: f32,
        amp: f32,
    },
    /// One channel of the device input. Wiring this toward the output is a
    /// FEEDBACK LOOP on laptop speakers — monitor through headphones.
    Input {
        channel: u32,
    },
    Mixer {
        gain: f32,
    },
    /// A project-seeded analog path. Identities are already split left/right
    /// by the graph builder and never derive from node or track order.
    DeskPath {
        project_seed: u64,
        left_identity: u64,
        right_identity: u64,
        noise_enabled: bool,
    },
    /// Tiny directional coupling between adjacent physical channel paths.
    /// `from` and `to` are permanent left/right identities, so moving a
    /// track in the document cannot redraw the desk.
    DeskBleed {
        project_seed: u64,
        from_left: u64,
        from_right: u64,
        to_left: u64,
        to_right: u64,
    },
    /// A tap into a return: a gain node that takes its amount from
    /// `param` of the device it is registered under, as a percentage.
    Send {
        gain: f32,
        param: u32,
    },
    /// Metronome blip on every beat while the transport rolls.
    Click,
    /// An audio file streamed from disk and placed in musical time. Compile
    /// stamps the beat boundaries into device samples. `None` length/source
    /// values preserve the old whole-file-from-zero behavior.
    AudioClip {
        path: std::path::PathBuf,
        #[serde(default)]
        start_beats: f64,
        #[serde(default)]
        length_beats: Option<f64>,
        #[serde(default)]
        source_offset_frames: u64,
        #[serde(default)]
        source_frames: Option<u64>,
        loop_clip: bool,
        /// Where a LOOPING clip wraps back to, as frames past
        /// `source_offset_frames`.
        ///
        /// Zero — the default, and what every clip written before braces
        /// carries — wraps to the start of the played region, which is
        /// what looping meant before a clip could loop a part of itself.
        /// Anything else is a head that plays once and a tail that
        /// repeats: the clip's own loop brace, in source frames.
        #[serde(default)]
        loop_start_frames: u64,
        gain: f32,
        /// A gain ramp at each end, in FRAMES of the clip's timeline
        /// span. Zero is no fade, which is what every clip written
        /// before these existed carries.
        #[serde(default)]
        fade_in_frames: u64,
        #[serde(default)]
        fade_out_frames: u64,
        /// Each fade's SHAPE, in `-1..=1`. Zero is linear, which is what
        /// every clip written before shapes existed carries — so a
        /// project loaded from before this sounds exactly as it did.
        #[serde(default)]
        fade_in_shape: f32,
        #[serde(default)]
        fade_out_shape: f32,
        /// A breakpoint gain envelope over the clip's timeline span:
        /// `(frame, linear gain)`, sorted, held flat past the last point.
        ///
        /// Empty is no envelope and costs the node nothing — which is
        /// what every clip written before this carries.
        #[serde(default)]
        envelope: Vec<(u64, f32)>,
    },
    /// The per-track output stage: mono in -> stereo out, panned and
    /// levelled. ParamChange 0 = pan (-1..1), 1 = gain (0..~+6 dB).
    ///
    /// `gain` defaults to unity when absent, so a project saved before the
    /// fader existed loads at the level it was written with.
    Pan {
        pan: f32,
        #[serde(default = "unity")]
        gain: f32,
    },
    /// A reverb on whatever feeds it. `size` is decay, `damp` is how fast
    /// the tail loses its highs, `mix` is wet against dry — all `0..=1`.
    /// ParamChange: 0 = mix, 1 = size, 2 = damp.
    Reverb {
        /// Milliseconds before the room answers.
        #[serde(default)]
        predelay_ms: f32,
        size: f32,
        /// RT60 in seconds — how long the tail rings, which is a
        /// different thing from how big the room is.
        #[serde(default = "reverb_decay_default")]
        decay: f32,
        /// The corner of the damping one-pole, in Hz.
        damp: f32,
        /// A highpass on the wet only, in Hz.
        #[serde(default)]
        low_cut: f32,
        #[serde(default = "reverb_diffusion_default")]
        diffusion: f32,
        #[serde(default)]
        modulation: f32,
        #[serde(default = "unity")]
        width: f32,
        mix: f32,
    },
    /// A resonant filter on whatever feeds it. `mode` and `slope` are
    /// indices into `crate::params::filter`'s lists (lp/hp/bp/notch;
    /// 6-48 dB/octave), `cutoff_hz` is Hz, `q` a resonance Q, `drive`
    /// `0..=1` into the oversampled soft clipper. ParamChange ids and every
    /// range are `crate::params::filter::TABLE`'s.
    Filter {
        /// The 7-row `params::filter` patch, in engine units.
        #[serde(default)]
        params: crate::audio::filter::FilterParams,
    },
    /// The character limiter on whatever feeds it. Every id and range is
    /// `crate::params::limiter::TABLE`'s, in the same engine units the
    /// card speaks.
    ///
    /// Stereo in, stereo out, and the one node in the tree that reports a
    /// latency it did not inherit from an oversampler.
    Limiter {
        /// The 7-row `params::limiter` patch, in engine units.
        #[serde(default)]
        params: crate::audio::limiter::LimiterParams,
    },
    /// A saturator on whatever feeds it: `mode` is an index into
    /// `crate::params::sat`'s five shapes, `drive` and `bias` are the
    /// kernel's own units (`1..32`, `-0.9..0.9`), `mix` is wet against
    /// dry and `out` a linear output trim. ParamChange ids and every
    /// range are `crate::params::sat::TABLE`'s.
    ///
    /// Stereo in, stereo out — it belongs behind the poly synth, and
    /// mono-summing there would fold the spread away.
    Sat {
        mode: u32,
        drive: f32,
        bias: f32,
        mix: f32,
        out: f32,
    },
    /// A lo-fi converter on whatever feeds it. `rate` is the converter's
    /// own clock in Hz and `bits` its word length; `mix` blends against
    /// the dry and `out` is a linear output trim. ParamChange ids and
    /// every range are `crate::params::lofi::TABLE`'s.
    ///
    /// Stereo in, stereo out — it stands in the same chains the saturator
    /// does, and mono-summing there would fold a spread away.
    Lofi {
        rate: f32,
        bits: f32,
        mix: f32,
        out: f32,
    },
    /// A slew-driven brightener on whatever feeds it. `amount` is how much
    /// a fully-triggered lift adds and `edge_hz` the corner of the band it
    /// is added to; `mix` blends against the dry and `out` is a linear
    /// output trim. ParamChange ids and every range are
    /// `crate::params::sheen::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Sheen {
        amount: f32,
        edge_hz: f32,
        mix: f32,
        out: f32,
    },
    /// An allpass disperser on whatever feeds it. `amount` is how many
    /// sections run, `freq_hz` where they are tuned and `pinch` how
    /// tightly the phase turns there. ParamChange ids and every range are
    /// `crate::params::disperser::TABLE`'s.
    ///
    /// No mix and no trim: see `Node::Disperser`.
    ///
    /// Stereo in, stereo out.
    Disperser {
        amount: f32,
        freq_hz: f32,
        pinch: f32,
    },
    /// A tilt on whatever feeds it. `tilt_db` is the gain reached at the
    /// high extreme — the low end mirrors it — and `pivot_hz` is where
    /// the plank balances. ParamChange ids and every range are
    /// `crate::params::tilt::TABLE`'s.
    ///
    /// No mix and no trim: see `Node::Tilt`.
    ///
    /// Stereo in, stereo out.
    Tilt {
        tilt_db: f32,
        pivot_hz: f32,
    },
    /// A swept phaser on whatever feeds it. `amount` is how many allpass
    /// sections run, `centre_hz` where the sweep is centred, `depth_oct`
    /// how far the corner travels either side of it, `rate_hz` how fast,
    /// and `mix` how much wet is summed with the dry — which is what
    /// makes the notches. ParamChange ids and every range are
    /// `crate::params::phaser::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Phaser {
        amount: f32,
        centre_hz: f32,
        depth_oct: f32,
        rate_hz: f32,
        mix: f32,
    },
    /// An analogue delay on whatever feeds it. `sync` is an index into
    /// `crate::params::echo::SYNC_NAMES` (0 = free, and only then is
    /// `time_ms` read); `feedback`, `drive`, `wow`, `spread` and `mix`
    /// are percentages and `tone_hz` is the damping corner inside the
    /// feedback loop. ParamChange ids and every range are
    /// `crate::params::echo::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Echo {
        sync: u32,
        time_ms: f32,
        feedback: f32,
        tone_hz: f32,
        drive: f32,
        wow: f32,
        spread: f32,
        mix: f32,
        /// The SEND, as a percentage — and with it, which side of the
        /// split this delay is on.
        ///
        /// Zero is an INSERT: the whole track runs through the delay,
        /// `mix` blends, and this is what every echo was before the
        /// parameter existed (hence the serde default — an old project
        /// opens as the insert it was saved as). Above zero it is an
        /// AUX: the graph builder taps its track at this level and
        /// returns the delay's output beside the dry instead of through
        /// it, and this is the tap's gain.
        #[serde(default)]
        send: f32,
    },
    /// An eight-band equaliser on whatever feeds it. Every range and
    /// every `ParamChange` id is `crate::params::eq::TABLE`'s; the band
    /// a row belongs to is `eq::split`'s answer.
    ///
    /// Stereo in, stereo out.
    Eq {
        #[serde(default)]
        params: crate::audio::eq::EqParams,
    },
    /// A bus compressor on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::glue::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Glue {
        #[serde(default)]
        params: crate::audio::glue::GlueParams,
    },
    /// A surgical compressor on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::clamp::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Clamp {
        #[serde(default)]
        params: crate::audio::clamp::ClampParams,
    },
    /// The transient shaper on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::flint::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Flint {
        #[serde(default)]
        params: crate::audio::flint::FlintParams,
    },
    /// Three-band dynamics on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::prism::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Prism {
        #[serde(default)]
        params: crate::audio::prism::PrismParams,
    },
    /// A gate on whatever feeds it. Every range and every `ParamChange`
    /// id is `crate::params::gate::TABLE`'s.
    ///
    /// Stereo in, stereo out — a gate that summed to mono would decide
    /// on one channel and act on two.
    Gate {
        #[serde(default)]
        params: crate::audio::gate::GateParams,
    },
    /// A mini channel strip on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::strip::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Strip {
        #[serde(default)]
        params: crate::audio::strip::StripParams,
    },
    /// A section of the console on whatever feeds it. The kind and the
    /// settings are `crate::console`'s; every id is the kind's table's.
    Section {
        params: crate::console::SectionParams,
    },
    /// A spectral resynthesiser on whatever feeds it. Every range and
    /// every `ParamChange` id is `crate::params::resyn::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    /// A test signal, in place of whatever feeds it. Every range and
    /// every `ParamChange` id is `crate::params::tone::TABLE`'s.
    Tone {
        #[serde(default)]
        params: crate::audio::tone::ToneParams,
    },
    /// A ring modulator on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::sigil::TABLE`'s.
    Sigil {
        #[serde(default)]
        params: crate::audio::sigil::SigilParams,
    },
    /// A meter on whatever feeds it, which it passes on untouched.
    /// Every range and every id is `crate::params::gauge::TABLE`'s.
    Gauge {
        #[serde(default)]
        params: crate::audio::gauge::GaugeParams,
    },
    /// The shadow on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::umbra::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Umbra {
        #[serde(default)]
        params: crate::audio::umbra::UmbraParams,
    },
    /// The tape looper on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::ferric::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Ferric {
        #[serde(default)]
        params: crate::audio::ferric::FerricParams,
    },
    /// The harmoniser on whatever feeds it. Every range and every
    /// `ParamChange` id is `crate::params::sibyl::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Sibyl {
        #[serde(default)]
        params: crate::audio::sibyl::SibylParams,
    },
    Resyn {
        #[serde(default)]
        params: crate::audio::resyn::ResynParams,
    },
    /// Gain, pan, width, bass mono, phase and channel mode on whatever
    /// feeds it. Every range and every `ParamChange` id is
    /// `crate::params::utility::TABLE`'s.
    ///
    /// Stereo in, stereo out.
    Utility {
        #[serde(default)]
        params: crate::audio::utility::UtilityParams,
    },
    /// A pattern of notes played by the built-in 8-voice synth. Subloops
    /// unroll at compile — see `expand_subloops`. `loop_len_beats` makes it a
    /// clip: the pattern cycles forever while the timeline rolls (None =
    /// one-shot).
    Seq {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// Synth voice/level parameters. Defaulted so projects saved before
        /// this field existed still open.
        #[serde(default)]
        params: SynthParams,
    },
    /// A pattern of notes played by the poly synth — the workhorse
    /// instrument of `notes/20260825-synth-brief.md`. Same pattern shape
    /// as [`NodeSpec::Seq`] (subloops unroll at compile, `loop_len_beats`
    /// makes it a clip); a different instrument plays it.
    ///
    /// Stereo out, because unison spread is a stereo idea.
    ///
    /// TIMELINE-LOCKED, exactly as `Seq` is: its events are stamped
    /// against the compiled tempo and it CUTS on discontinuity, so an
    /// offline bounce reproduces a live take sample for sample.
    /// TINE: a struck-resonator synth, one pattern, sixteen voices.
    ///
    /// Stereo out, because the fan of voices across the field is part of
    /// the instrument rather than an effect after it.
    Tine {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        #[serde(default)]
        params: crate::audio::tine::TineParams,
    },
    /// BRICK: the drum one-shot sampler. Its file is loaded at compile
    /// as the sampler's is; an empty path compiles to a silent brick.
    Brick {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        #[serde(default)]
        path: std::path::PathBuf,
        #[serde(default)]
        params: crate::audio::brick::BrickParams,
    },
    /// QUAD: four-operator FM. Timeline-locked as `Tine` is; stereo out,
    /// the same on both sides.
    Quad {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        #[serde(default)]
        params: crate::audio::quad::QuadParams,
    },
    /// STAB: the house chord synth, one key a chord. Timeline-locked as
    /// `Tine` is; stereo out, the detuned pairs spread across it.
    Stab {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        #[serde(default)]
        params: crate::audio::stab::StabParams,
    },
    /// sCOMP: a sine bounced through a compressor, rendered to a take
    /// when the node is built and played like a sample. Timeline-locked
    /// as `Tine` is. Stereo out, the same on both sides, so the strip
    /// after it sees what every other instrument gives it.
    Scomp {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        #[serde(default)]
        params: crate::scomp::ScompParams,
    },
    Poly {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 34-row `params::poly` patch, in engine units.
        #[serde(default)]
        params: crate::audio::poly::PolyParams,
    },
    /// HAZE: an analog pad synth, one pattern, sixteen drifting voices.
    ///
    /// Stereo out, because the ensemble that gives it its width is part
    /// of the instrument rather than an effect after it.
    ///
    /// TIMELINE-LOCKED, exactly as `Seq` and `Poly` are: its events are
    /// stamped against the compiled tempo and it CUTS on discontinuity,
    /// so an offline bounce reproduces a live take sample for sample.
    Haze {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The nineteen-row `params::haze` patch, in engine units.
        #[serde(default)]
        params: crate::audio::haze::HazeParams,
    },
    /// The wavetable synth: two morphing oscillators, noise, a
    /// multimode filter. Same pattern shape as its siblings.
    ///
    /// TIMELINE-LOCKED, exactly as `Seq` and `Poly` are: events are
    /// stamped against the compiled tempo and it CUTS on discontinuity,
    /// so an offline bounce reproduces a live take sample for sample.
    Loom {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 28-row `params::loom` patch, in engine units.
        #[serde(default)]
        params: crate::audio::loom::LoomParams,
    },
    /// The kick drum synth: one pattern, one one-shot voice.
    ///
    /// The same pattern shape as [`NodeSpec::Seq`] and [`NodeSpec::Poly`],
    /// a third instrument playing it. Mono out — a kick belongs in the
    /// middle, and the track's own pan stage is what moves it if anyone
    /// insists.
    ///
    /// TIMELINE-LOCKED, exactly as its siblings: events are stamped
    /// against the compiled tempo and it CUTS on discontinuity, so an
    /// offline bounce reproduces a live take sample for sample.
    Kick {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 13-row `params::kick` patch, in engine units.
        #[serde(default)]
        params: crate::audio::kick::KickParams,
    },
    /// The acid mono: one pattern, one voice, and a slide that is two
    /// notes overlapping rather than a flag.
    Acid {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 10-row `params::acid` patch, in engine units.
        #[serde(default)]
        params: crate::audio::acid::AcidParams,
    },
    /// The sampler: one pattern, a file, and eight voices reading it.
    ///
    /// The PATH is part of the spec rather than the params because
    /// changing it is a recompile and not a letter — letters carry an
    /// `f32` and a sample is megabytes. Everything else about the device
    /// is a letter, so dragging a start marker is heard while it is
    /// dragged.
    Sampler {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The file to play. Empty means nothing loaded, which compiles
        /// to a silent sampler rather than to a refused graph — a
        /// missing sample must not mute the project.
        #[serde(default)]
        path: std::path::PathBuf,
        /// The 36-row `params::sampler` patch, in engine units.
        #[serde(default)]
        params: crate::audio::sampler::SamplerParams,
        /// Slice starts as FRACTIONS of the file, `0..1`. Authored green
        /// side (a grid, detected onsets, placed markers) and turned into
        /// frames at compile against the material actually loaded, so
        /// the table is right at whatever rate the device opened. Sorted
        /// and deduplicated at compile, for the reason a clip's envelope
        /// is: the callback walks it assuming it rises.
        #[serde(default)]
        slices: Vec<f64>,
    },
    /// The snare drum synth: one pattern, one one-shot voice.
    ///
    /// [`NodeSpec::Kick`]'s shape exactly, and TIMELINE-LOCKED for the
    /// same reason — events are stamped against the compiled tempo and it
    /// cuts on discontinuity, so an offline bounce reproduces a live take
    /// sample for sample.
    Snare {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 11-row `params::snare` patch, in engine units.
        #[serde(default)]
        params: crate::audio::snare::SnareParams,
    },
    /// The tom synth: one pattern, one one-shot voice.
    Tom {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 9-row `params::tom` patch, in engine units.
        #[serde(default)]
        params: crate::audio::tom::TomParams,
    },
    /// The 808 hi-hat: one pattern, one one-shot voice.
    ///
    /// The note picks OPEN or CLOSED rather than a pitch — see
    /// [`params::hat::OPEN_NOTE`](crate::params::hat::OPEN_NOTE).
    Hat {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 8-row `params::hat` patch, in engine units.
        #[serde(default)]
        params: crate::audio::hat::HatParams,
    },
    /// The hand clap: one pattern, one one-shot voice.
    Handclap {
        notes: Vec<Note>,
        subloops: Vec<SubLoop>,
        loop_len_beats: Option<f64>,
        /// The 10-row `params::handclap` patch, in engine units.
        #[serde(default)]
        params: crate::audio::handclap::HandclapParams,
    },
    /// Modulato: chorus, flanger and vibrato, which are one effect.
    ///
    /// Stereo in, stereo out — the two sides run their own line and their
    /// own oscillator, and the phase between them is what widens it.
    ///
    /// FREE-RUNNING: its delay memory is signal history, not timeline
    /// position. It CUTS on discontinuity all the same, because the
    /// samples it is holding belong to a position the transport has left.
    Modulato {
        #[serde(default)]
        params: crate::audio::modulato::ModulatoParams,
    },
    /// A persisted CLAP effect identity. A live model must be bound to a
    /// [`ClapNodeFactory`](crate::clap_host::ClapNodeFactory) with
    /// [`GraphSpec::push_clap`]; an explicit missing model compiles to a dry
    /// delay of its last-known latency so the rest of the mix stays aligned.
    Clap {
        device: crate::clap_host::PluginDeviceModel,
    },
    /// A bypassed effect's dry, phase-stable place in a chain.
    ///
    /// Keeping the original spec only as a latency declaration lets compile
    /// prepare a plain delay at the actual device sample rate. The effect is
    /// never instantiated and receives no parameter letters, but bypassing a
    /// lookahead, FFT, oversampled or plug-in processor cannot move the track
    /// against its siblings.
    LatencyBypass {
        effect: Box<NodeSpec>,
    },
    /// Plugin delay compensation, inserted by COMPILE — never by a user and
    /// never by the graph builder.
    ///
    /// A node that reports latency delays everything downstream of it; every
    /// other path into the same mixer has to be delayed to match, or the
    /// tracks slide against each other and nothing on screen explains why.
    /// `compensate` works out where these belong and how long each is; see
    /// [`GraphSpec::compensate`].
    Delay {
        samples: usize,
        /// Matched to the producer it is spliced behind, since a node's
        /// channel count is fixed by its kind and a delay must not narrow
        /// a stereo path to mono.
        channels: u8,
    },
}

impl GraphSpec {
    /// Swing every pattern in the graph: each instrument's ODD grid steps
    /// shift toward the next even step by `swing * grid / 2`. Called on
    /// the WHOLE spec before compile, so live playback and every offline
    /// render hear the same swing. The arithmetic lives in
    /// [`swing_note_starts`], pure and pinned by test.
    pub fn apply_swing(&mut self, grid: f32, swing: f32) {
        let grid = f64::from(grid);
        for id in self.order.clone() {
            let Some(spec) = self.nodes.get_mut(id.0) else {
                continue;
            };
            match spec {
                NodeSpec::Seq { notes, .. }
                | NodeSpec::Tine { notes, .. }
                | NodeSpec::Scomp { notes, .. }
                | NodeSpec::Stab { notes, .. }
                | NodeSpec::Quad { notes, .. }
                | NodeSpec::Brick { notes, .. }
                | NodeSpec::Poly { notes, .. }
                | NodeSpec::Haze { notes, .. }
                | NodeSpec::Kick { notes, .. }
                | NodeSpec::Acid { notes, .. }
                | NodeSpec::Sampler { notes, .. }
                | NodeSpec::Snare { notes, .. }
                | NodeSpec::Tom { notes, .. }
                | NodeSpec::Hat { notes, .. }
                | NodeSpec::Handclap { notes, .. } => swing_note_starts(notes, grid, swing),
                _ => {}
            }
        }
    }

    /// Add a node. The returned id is its permanent name tag.
    /// A pushed node, to finish after its neighbours exist — a voice's
    /// notes name the effects they lock, and the effects are pushed
    /// after the voice so the chain reads in signal order.
    /// One node's spec, for a caller that built the graph and wants to
    /// read back what it made.
    pub fn node(&self, id: NodeId) -> Option<&NodeSpec> {
        self.nodes.get(id.0)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut NodeSpec> {
        self.nodes.get_mut(id.0)
    }

    /// The knobs registered behind locked effect parameters.
    pub fn lock_bases(&self) -> &[(NodeId, u32, f32)] {
        &self.fx_bases
    }

    /// Register the knob behind an effect parameter that trigs lock.
    pub fn lock_base(&mut self, node: NodeId, param: u32, base: f32) {
        match self
            .fx_bases
            .iter_mut()
            .find(|(n, p, _)| *n == node && *p == param)
        {
            Some(entry) => entry.2 = base,
            None => self.fx_bases.push((node, param, base)),
        }
    }

    /// Add a live CLAP effect and bind its trusted, cloneable compile recipe.
    pub fn push_clap(
        &mut self,
        plugin: crate::clap_host::ClapDeviceState,
        factory: crate::clap_host::ClapNodeFactory,
    ) -> Result<NodeId, ClapBindError> {
        if !factory.matches(&plugin) {
            return Err(ClapBindError::IdentityMismatch);
        }
        let id = self.push(NodeSpec::Clap {
            device: crate::clap_host::PluginDeviceModel::Clap { plugin },
        });
        self.clap_factories.insert(id, factory);
        Ok(id)
    }

    /// Add the durable placeholder used when a project plugin cannot resolve.
    /// It passes dry audio delayed by the plugin's last-known latency.
    pub fn push_missing_clap(
        &mut self,
        plugin: crate::clap_host::ClapDeviceState,
        reason: impl Into<String>,
    ) -> NodeId {
        self.push(NodeSpec::Clap {
            device: crate::clap_host::PluginDeviceModel::Missing {
                plugin,
                reason: reason.into(),
            },
        })
    }

    pub fn push(&mut self, node: NodeSpec) -> NodeId {
        let id = NodeId(self.nodes.insert(node));
        self.order.push(id);
        id
    }

    /// Remove a node and every wire touching it. Its id goes permanently
    /// dead: letters addressed to it are binned, never redelivered.
    pub fn remove(&mut self, id: NodeId) {
        self.nodes.remove(id.0);
        self.clap_factories.remove(&id);
        self.order.retain(|n| *n != id);
        self.wires.retain(|(a, b)| *a != id && *b != id);
        self.automation.retain(|lane| lane.node != id);
        if self.output == Some(id) {
            self.output = None;
        }
    }

    /// Wire `from`'s output into `to`'s inputs.
    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.wires.push((from, to));
    }

    /// Declare which node feeds the speakers.
    pub fn set_output(&mut self, id: NodeId) {
        self.output = Some(id);
    }

    /// Ask `node` for its READOUT under `slot` — what it is doing, as
    /// opposed to how loud its output is.
    ///
    /// Slots are the meter's slots and the same size table; a node with
    /// nothing to say simply never reports. Slots past [`MAX_METERS`]
    /// are ignored rather than clamped, for the reason `meter` gives:
    /// silently reporting the wrong device would be worse than not
    /// reporting it.
    pub fn tap(&mut self, slot: usize, node: NodeId) {
        if slot < MAX_METERS {
            self.taps.push((slot, node));
        }
    }

    /// Report what `node` says about itself under telemetry `slot`.
    pub fn telemetry(&mut self, slot: usize, node: NodeId) {
        if slot < MAX_TELEMETRY {
            self.telemetry.push((slot, node));
        }
    }

    /// Report `node`'s output level to the UI under `slot`.
    ///
    /// The peak is taken where the node's own step writes it — for a
    /// track's output stage that means POST-fader and post-pan, which is
    /// what a mixer meter is expected to show. Slots past [`MAX_METERS`]
    /// are ignored rather than clamped: silently metering the wrong track
    /// would be worse than not metering it.
    ///
    /// # The invariant a tap depends on
    ///
    /// A tapped node's `process` MUST write its whole output buffer every
    /// block. The meter reads the buffer after the step, and arena slots
    /// are recycled between nodes — so a node that returns early without
    /// filling would have the previous owner's samples metered as its own.
    /// Every node fills unconditionally today; this is the note for the
    /// next one that does not.
    pub fn meter(&mut self, slot: usize, node: NodeId) {
        if slot < MAX_METERS {
            self.meters.push((slot, node));
        }
    }

    /// Nodes in insertion order with their ids — for building a project doc.
    pub fn iter_ordered(&self) -> impl Iterator<Item = (NodeId, &NodeSpec)> + '_ {
        self.order
            .iter()
            .filter_map(|id| self.nodes.get(id.0).map(|n| (*id, n)))
    }

    /// Wires as (from, to) pairs.
    pub fn wires(&self) -> &[(NodeId, NodeId)] {
        &self.wires
    }

    /// Hand the graph its modulation. Replaces whatever was set before —
    /// the builder produces the whole picture in one pass, and a plan
    /// assembled from two sources would be a plan nobody can reason about.
    pub fn set_modulation(&mut self, modulation: ModSpec) {
        self.modulation = modulation;
    }

    /// Add or replace one complete automation lane. The builder calls this
    /// after resolving the Song target against its owning track/device; a
    /// later lane for the same node/parameter is the explicit winner.
    pub fn automate(&mut self, node: NodeId, param: u32, base: f32, points: Vec<AutomationPoint>) {
        let lane = AutomationLaneSpec {
            node,
            param,
            base: if base.is_finite() { base } else { 0.0 },
            points,
        };
        if let Some(existing) = self
            .automation
            .iter_mut()
            .find(|existing| existing.node == node && existing.param == param)
        {
            *existing = lane;
        } else {
            self.automation.push(lane);
        }
    }

    /// The modulation this graph will compile.
    pub fn modulation(&self) -> &ModSpec {
        &self.modulation
    }

    /// The declared output node, if any.
    pub fn output(&self) -> Option<NodeId> {
        self.output
    }

    /// What this node's own processing adds to the signal's arrival time,
    /// in samples at `sample_rate`.
    ///
    /// Constant per kind: a latency that moved with a knob would slide the
    /// track in time as the user turned it, which is why the Filter's drive
    /// stage is permanently in its path rather than switched in at drive
    /// > 0.
    fn spec_latency(spec: &NodeSpec, sample_rate: u32) -> usize {
        match spec {
            // The half-band round trip of the 2x drive stage.
            // Both run their shaping at 2x, and both keep the round
            // trip permanently in the path so the figure is constant.
            NodeSpec::Sat { .. } => crate::dsp::shaper::Oversampler2x::new().latency(),
            // Stated by the device itself, so the figure the graph
            // compensates for and the delay the device actually holds
            // cannot drift apart.
            NodeSpec::Filter { .. } => crate::audio::filter::latency(),
            // The lookahead plus BOTH half-band round trips. Constant,
            // and stated by the device itself so the figure the graph
            // compensates for and the delay the device actually holds
            // cannot drift apart.
            NodeSpec::Limiter { .. } => crate::audio::limiter::latency(),
            // Stated by the device itself, so the figure the graph
            // compensates for and the delay it actually holds cannot
            // drift apart — and `audio::resyn` MEASURES that figure
            // rather than asserting it.
            NodeSpec::Resyn { .. } => crate::audio::resyn::LATENCY,
            NodeSpec::Sibyl { .. } => crate::audio::sibyl::LATENCY,
            NodeSpec::Umbra { .. } => crate::audio::umbra::LATENCY,
            // These console cores look ahead or frame audio and report a real
            // delay. Keep the pure formulas beside PDC and pin them against
            // each core's `latency()` in tests.
            NodeSpec::Section { params } => match params.kind {
                crate::console::SectionKind::Preamp
                | crate::console::SectionKind::Cut
                | crate::console::SectionKind::Drive
                | crate::console::SectionKind::Iron => {
                    crate::dsp::shaper::Oversampler2x::new().latency()
                }
                crate::console::SectionKind::Door => {
                    (sample_rate as f32 * crate::params::console::door::LOOKAHEAD_MS / 1_000.0)
                        .round() as usize
                }
                crate::console::SectionKind::Ceiling => ((sample_rate as f32
                    * crate::params::console::ceiling::LOOKAHEAD_MS
                    / 1_000.0) as usize)
                    .max(1),
                crate::console::SectionKind::Spectra => crate::params::console::spectra::SIZE * 2,
                _ => 0,
            },
            NodeSpec::Clap { device } => device.plugin().latency_samples(),
            NodeSpec::LatencyBypass { effect } => Self::spec_latency(effect, sample_rate),
            NodeSpec::Delay { samples, .. } => *samples,
            _ => 0,
        }
    }

    /// The graph with plugin delay compensation applied: a copy of this one
    /// with [`NodeSpec::Delay`] nodes spliced onto every edge that would
    /// otherwise arrive early.
    ///
    /// # Why compile and not the callback
    ///
    /// Alignment is a property of the WIRING, and the wiring is known here.
    /// Working it out per block would mean the callback carrying a latency
    /// model; working it out here means it carries a delay line, which is a
    /// tested kernel. Same reasoning as the flat schedule: complexity at
    /// compile, execution in the red zone.
    ///
    /// # The rule
    ///
    /// A signal arrives at node `t` at `arrival[t] = max over feeders f of
    /// (arrival[f] + latency[f])`. Any feeder arriving earlier than that
    /// maximum is delayed to match. Nothing is ever made EARLIER — that is
    /// not possible — so the graph's total latency is the deepest path, and
    /// [`Schedule::latency`] reports it for the transport to offset by.
    ///
    /// Returns `None` when nothing needs compensating, which is the common
    /// case and skips the copy entirely.
    fn compensate(&self, sample_rate: u32) -> Option<GraphSpec> {
        // Dense indices, in insertion order — the same correspondence the
        // compiler uses.
        let n = self.order.len();
        let dense_of = |id: &NodeId| self.order.iter().position(|x| x == id);
        let latency: Vec<usize> = self
            .order
            .iter()
            .map(|id| {
                self.nodes
                    .get(id.0)
                    .map_or(0, |spec| Self::spec_latency(spec, sample_rate))
            })
            .collect();
        if latency.iter().all(|l| *l == 0) {
            return None; // nothing in the graph is late
        }

        let mut feeders: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut targets: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (from, to) in &self.wires {
            let (Some(f), Some(t)) = (dense_of(from), dense_of(to)) else {
                return None; // a dangling wire: let compile report it
            };
            feeders[t].push(f);
            targets[f].push(t);
        }

        // Kahn again, on the spec. A cycle is compile's error to report,
        // not ours — bail and let it.
        let mut in_count: Vec<usize> = feeders.iter().map(Vec::len).collect();
        let mut ready: Vec<usize> = (0..n).filter(|i| in_count[*i] == 0).collect();
        let mut topo: Vec<usize> = Vec::with_capacity(n);
        while let Some(i) = ready.pop() {
            topo.push(i);
            for &t in &targets[i] {
                in_count[t] -= 1;
                if in_count[t] == 0 {
                    ready.push(t);
                }
            }
        }
        if topo.len() != n {
            return None;
        }

        // arrival[t] in topo order, so every feeder is settled first.
        let mut arrival = vec![0usize; n];
        for &i in &topo {
            arrival[i] = feeders[i]
                .iter()
                .map(|&f| arrival[f] + latency[f])
                .max()
                .unwrap_or(0);
        }

        // Rebuild, splicing a delay onto every edge that arrives early.
        let mut out = GraphSpec::default();
        let mut remap: Vec<Option<NodeId>> = vec![None; n];
        for (dense, id) in self.order.iter().enumerate() {
            if let Some(spec) = self.nodes.get(id.0) {
                remap[dense] = Some(out.push(spec.clone()));
            }
        }
        let mapped = |dense: usize| -> Option<NodeId> { remap.get(dense).copied().flatten() };

        // Runtime-only CLAP factories follow their persisted node through the
        // same remap. The recipe clones; a live native processor never does.
        for (node, factory) in &self.clap_factories {
            if let Some(mapped_node) = dense_of(node).and_then(mapped) {
                out.clap_factories.insert(mapped_node, factory.clone());
            }
        }

        let mut spliced = false;
        for (from, to) in &self.wires {
            let (Some(f), Some(t)) = (dense_of(from), dense_of(to)) else {
                continue;
            };
            let (Some(nf), Some(nt)) = (mapped(f), mapped(t)) else {
                continue;
            };
            let behind = arrival[t].saturating_sub(arrival[f] + latency[f]);
            if behind == 0 {
                out.connect(nf, nt);
                continue;
            }
            let channels = self.nodes.get(from.0).map_or(1, Node::channels);
            let delay = out.push(NodeSpec::Delay {
                samples: behind,
                channels,
            });
            out.connect(nf, delay);
            out.connect(delay, nt);
            spliced = true;
        }
        if !spliced {
            // Latency exists but every path already agrees — a single
            // chain, which is the ordinary one-track case.
            return None;
        }

        if let Some(o) = self.output.and_then(|id| dense_of(&id)).and_then(mapped) {
            out.set_output(o);
        }
        for (slot, node) in &self.meters {
            // A tap follows its node, so it still reads that node's own
            // output — before any delay spliced BEHIND it, which is what a
            // track meter should show.
            if let Some(m) = dense_of(node).and_then(mapped) {
                out.meter(*slot, m);
            }
        }
        for (slot, node) in &self.taps {
            if let Some(mapped_node) = dense_of(node).and_then(mapped) {
                out.tap(*slot, mapped_node);
            }
        }
        for (slot, node) in &self.telemetry {
            if let Some(mapped_node) = dense_of(node).and_then(mapped) {
                out.telemetry(*slot, mapped_node);
            }
        }
        for (node, param, base) in &self.fx_bases {
            if let Some(mapped_node) = dense_of(node).and_then(mapped) {
                out.lock_base(mapped_node, *param, *base);
            }
        }
        // Modulation addresses nodes by id, and every id just changed.
        let mut modulation = self.modulation.clone();
        modulation.wires.retain_mut(|wire| {
            match dense_of(&wire.node).and_then(mapped) {
                Some(id) => {
                    wire.node = id;
                    true
                }
                // Unreachable while the remap is total; dropping beats
                // letting a wire address a node in the OLD graph.
                None => false,
            }
        });
        out.set_modulation(modulation);
        for lane in &self.automation {
            if let Some(node) = dense_of(&lane.node).and_then(mapped) {
                out.automate(node, lane.param, lane.base, lane.points.clone());
            }
        }
        Some(out)
    }

    /// Compile into a runnable schedule: verify every wire, sort nodes so each
    /// runs after everything it listens to (Kahn's algorithm), refuse cycles,
    /// assign one arena slot per node (reuse comes later), and build the
    /// name-tag directory. All allocation happens here, in the green zone.
    pub fn compile(&self, sample_rate: u32, block_frames: usize) -> Result<Schedule, CompileError> {
        self.compile_at_tempo(sample_rate, block_frames, 120.0)
    }

    /// Compile musical placement into timeline-sample stamps at `bpm`.
    /// Projects retain beats; only the immutable runtime schedule sees
    /// samples, per the sequencing contract.
    pub fn compile_at_tempo(
        &self,
        sample_rate: u32,
        block_frames: usize,
        bpm: f64,
    ) -> Result<Schedule, CompileError> {
        // Alignment first, on a copy: everything below then compiles a
        // graph whose paths already agree, and needs to know nothing about
        // latency. `compensate` returns None when there is nothing to do,
        // which is every graph with no latency-bearing node in it.
        self.compile_resolved(sample_rate, block_frames, bpm, None)
    }

    /// Compile a one-shot arrangement against its complete piecewise tempo
    /// map. Session-launcher clips deliberately keep using
    /// [`Self::compile_at_tempo`]: their loop clock is clip-relative rather
    /// than an absolute song timeline.
    pub fn compile_with_tempo_table(
        &self,
        sample_rate: u32,
        block_frames: usize,
        fallback_bpm: f64,
        timeline: &crate::tempo::TempoTable,
    ) -> Result<Schedule, CompileError> {
        if timeline.sample_rate() != f64::from(sample_rate) {
            return Err(CompileError::TempoSampleRateMismatch);
        }
        self.compile_resolved(sample_rate, block_frames, fallback_bpm, Some(timeline))
    }

    fn compile_resolved(
        &self,
        sample_rate: u32,
        block_frames: usize,
        bpm: f64,
        timeline: Option<&crate::tempo::TempoTable>,
    ) -> Result<Schedule, CompileError> {
        // Alignment first, on a copy: everything below then compiles a
        // graph whose paths already agree. The musical timeline remains a
        // separate immutable compile input and therefore survives the copy.
        if let Some(aligned) = self.compensate(sample_rate) {
            return aligned.compile_inner(sample_rate, block_frames, bpm, timeline);
        }
        self.compile_inner(sample_rate, block_frames, bpm, timeline)
    }

    fn compile_inner(
        &self,
        sample_rate: u32,
        block_frames: usize,
        bpm: f64,
        timeline: Option<&crate::tempo::TempoTable>,
    ) -> Result<Schedule, CompileError> {
        // Clamped to the same range the transport accepts, not merely
        // tested for positivity. The node now trusts the COMPILED tempo
        // rather than the live one, so this is the only thing standing
        // between a caller's tempo and a `samples_per_beat` large enough to
        // saturate every event stamp to `u64::MAX`. `bounce` takes its bpm
        // from a public field with no invariant.
        let bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm.clamp(
                crate::audio::transport::BPM_MIN,
                crate::audio::transport::BPM_MAX,
            )
        } else {
            120.0
        };
        let samples_per_beat = f64::from(sample_rate) * 60.0 / bpm;
        let plock_glide_samples = (u64::from(sample_rate) * 3).div_ceil(1_000).max(1);
        let n = self.order.len();
        let dense_of = |id: &NodeId| self.order.iter().position(|x| x == id);
        if self
            .automation
            .iter()
            .any(|lane| dense_of(&lane.node).is_none())
        {
            return Err(CompileError::DanglingAutomation);
        }

        // Wires as dense indices, refusing dangles up front.
        let mut in_wires: Vec<Vec<usize>> = vec![Vec::new(); n]; // per node: who feeds it
        let mut out_degree_targets: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (from, to) in &self.wires {
            let (Some(f), Some(t)) = (dense_of(from), dense_of(to)) else {
                return Err(CompileError::DanglingWire);
            };
            in_wires[t].push(f);
            out_degree_targets[f].push(t);
        }
        if in_wires.iter().any(|w| w.len() > MAX_NODE_INPUTS) {
            return Err(CompileError::TooManyInputs);
        }

        // Kahn: repeatedly take a node all of whose feeders are done.
        let mut in_count: Vec<usize> = in_wires.iter().map(Vec::len).collect();
        let mut ready: Vec<usize> = (0..n).filter(|i| in_count[*i] == 0).collect();
        let mut topo: Vec<usize> = Vec::with_capacity(n);
        while let Some(i) = ready.pop() {
            topo.push(i);
            for &t in &out_degree_targets[i] {
                in_count[t] -= 1;
                if in_count[t] == 0 {
                    ready.push(t);
                }
            }
        }
        if topo.len() != n {
            return Err(CompileError::Cycle); // nodes left over = wires loop
        }

        // Instantiate nodes in insertion order (dense layout stable across
        // compiles), then emit steps in topo order.
        let nodes: Vec<Node> = self
            .order
            .iter()
            .map(|id| -> Result<Node, CompileError> {
                // Map resolution is green-zone work. The editable spec keeps
                // musical beats; this temporary copy expresses absolute map
                // samples in the scalar compiler's unit so every existing
                // event/clip validation remains one shared implementation.
                let resolved = timeline.and_then(|timeline| {
                    self.nodes.get(id.0).cloned().map(|mut spec| {
                        resolve_timeline_spec(&mut spec, timeline, samples_per_beat);
                        spec
                    })
                });
                let spec = resolved.as_ref().or_else(|| self.nodes.get(id.0));
                // Ids in `order` always resolve: push/remove keep them in sync.
                Ok(match spec {
                    Some(NodeSpec::Silence) | None => Node::Silence,
                    Some(NodeSpec::Sine { freq, amp }) => Node::Sine {
                        phase: 0.0,
                        freq: *freq,
                        amp: 0.0, // ramp in from silence on first block
                        target_freq: *freq,
                        target_amp: *amp,
                        sample_rate: sample_rate as f32,
                    },
                    Some(NodeSpec::Input { channel }) => Node::Input { channel: *channel },
                    Some(NodeSpec::Delay { samples, .. }) => latency_passthrough_node(*samples),
                    Some(NodeSpec::LatencyBypass { effect }) => {
                        latency_passthrough_node(Self::spec_latency(effect, sample_rate))
                    }
                    Some(NodeSpec::Clap { device }) => {
                        let plugin = device.plugin();
                        if device.is_missing() {
                            latency_passthrough_node(plugin.latency_samples())
                        } else {
                            let factory = self.clap_factories.get(id).ok_or_else(|| {
                                CompileError::MissingClapRuntime {
                                    plugin_id: plugin.key.plugin_id.as_str().to_owned(),
                                }
                            })?;
                            let processor = factory
                                .prepare(plugin, sample_rate, block_frames)
                                .map_err(|error| CompileError::ClapPrepare {
                                    plugin_id: plugin.key.plugin_id.as_str().to_owned(),
                                    detail: error.to_string(),
                                })?;
                            Node::Clap { processor }
                        }
                    }
                    Some(NodeSpec::Mixer { gain }) => Node::Mixer {
                        gain: 0.0, // ramp in, same reasoning as sine amp
                        target_gain: *gain,
                    },
                    Some(NodeSpec::DeskPath {
                        project_seed,
                        left_identity,
                        right_identity,
                        noise_enabled,
                    }) => {
                        let mut left = crate::dsp::desk::DeskPath::new();
                        let mut right = crate::dsp::desk::DeskPath::new();
                        left.prepare(
                            sample_rate as f32,
                            *project_seed,
                            *left_identity,
                            *noise_enabled,
                        );
                        right.prepare(
                            sample_rate as f32,
                            *project_seed,
                            *right_identity,
                            *noise_enabled,
                        );
                        Node::DeskPath { left, right }
                    }
                    Some(NodeSpec::DeskBleed {
                        project_seed,
                        from_left,
                        from_right,
                        to_left,
                        to_right,
                    }) => {
                        let mut left = crate::dsp::desk::DeskBleed::new();
                        let mut right = crate::dsp::desk::DeskBleed::new();
                        left.prepare(sample_rate as f32, *project_seed, *from_left, *to_left);
                        right.prepare(sample_rate as f32, *project_seed, *from_right, *to_right);
                        Node::DeskBleed { left, right }
                    }
                    Some(NodeSpec::Send { gain, param }) => Node::Send {
                        gain: 0.0, // ramp in, so a send never arrives as a click
                        target_gain: *gain,
                        param: *param,
                    },
                    Some(NodeSpec::Seq {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        let mut voices = VoiceBank::default();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in; the
                        // default is the sane fallback.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            SynthParams::default().gain
                        };
                        Node::Seq {
                            events,
                            // The pattern's length in samples at this
                            // tempo. Zero when there is no loop, which the
                            // walk reads as "never wrap".
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices,
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Acid {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: builds both wavetable sets and the
                        // chunk buffer; nothing allocates afterwards.
                        let mut voices = crate::audio::acid::AcidVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, 2.0)
                        } else {
                            crate::audio::acid::AcidParams::default().level
                        };
                        Node::Acid {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Kick {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: builds the sine tables the callback
                        // will read and nothing else allocates afterwards.
                        let mut voices = crate::audio::kick::KickVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::kick::KickParams::default().gain
                        };
                        Node::Kick {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Sampler {
                        notes,
                        subloops,
                        loop_len_beats,
                        path,
                        params,
                        slices,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // GREEN ZONE: the file is decoded and resampled to
                        // the device rate HERE, once, behind a cache. An
                        // unreadable or absent file compiles to a SILENT
                        // sampler rather than refusing the whole graph —
                        // the same rule `AudioClip` keeps, because a
                        // missing sample should not mute a project.
                        let material = if path.as_os_str().is_empty() {
                            crate::audio::material::Material::empty()
                        } else {
                            crate::audio::material::load_cached(path, sample_rate)
                                .unwrap_or_else(|_| crate::audio::material::Material::empty())
                        };
                        // The slice table, made safe for the callback to
                        // walk: sorted, deduplicated, inside the file, and
                        // capped. Exactly what `sorted_envelope` does for a
                        // clip, and for the same reason.
                        let mut table = slice_frames(slices, material.frames);
                        // Nothing authored yet: fall back to the grid the
                        // knob asks for, so dropping a break on the device
                        // and switching to slice mode plays slices instead
                        // of silence.
                        if table.is_empty() && material.frames > 0 {
                            table = crate::slice::grid(
                                material.frames,
                                params.slices.round().max(1.0) as usize,
                            );
                        }
                        let mut voices = crate::audio::sampler::SamplerVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                            material,
                        );
                        voices.set_slices(&table);
                        let mut preamp = crate::audio::preamp::Preamp::new();
                        preamp.prepare(sample_rate as f32);
                        preamp.set_amount(params.preamp);
                        // RON round-trips NaN literals, so a hand-edited or
                        // corrupt project could smuggle one in.
                        let gain = if params.gain_db.is_finite() {
                            10f32.powf(params.gain_db.clamp(-60.0, 12.0) / 20.0)
                        } else {
                            1.0
                        };
                        Node::Sampler {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            preamp,
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Snare {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: whatever this voice needs to
                        // allocate, it allocates here and never again.
                        let mut voices = crate::audio::snare::SnareVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::snare::SnareParams::default().gain
                        };
                        Node::Snare {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Tom {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: whatever this voice needs to
                        // allocate, it allocates here and never again.
                        let mut voices = crate::audio::tom::TomVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::tom::TomParams::default().gain
                        };
                        Node::Tom {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Hat {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: whatever this voice needs to
                        // allocate, it allocates here and never again.
                        let mut voices = crate::audio::hat::HatVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::hat::HatParams::default().gain
                        };
                        Node::Hat {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Handclap {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: whatever this voice needs to
                        // allocate, it allocates here and never again.
                        let mut voices = crate::audio::handclap::HandclapVoice::new();
                        voices.prepare(sample_rate as f32, *params);
                        // RON round-trips NaN literals, so a hand-edited
                        // or corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::handclap::HandclapParams::default().gain
                        };
                        Node::Handclap {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain: 0.0,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Haze {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: this builds the tables, the delay
                        // rings and every buffer the callback will ever
                        // need. `block_frames` is the longest segment
                        // there can be, which is what the right channel
                        // has to be able to hold.
                        let voices = crate::audio::haze::Haze::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        Node::Haze {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            // The instrument applies `level` itself, so
                            // the node's ramp rides at unity: spending
                            // it twice would square the fader.
                            gain: 1.0,
                            target_gain: 1.0,
                        }
                    }
                    Some(NodeSpec::Tine {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: every resonator, every scratch
                        // buffer and the body's reel are born here.
                        let voices = crate::audio::tine::TineVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, crate::params::tine::LEVEL_MAX)
                        } else {
                            crate::audio::tine::TineParams::default().level
                        };
                        Node::Tine {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Scomp {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: the take is rendered here, every
                        // pass of it, and the voices are born over it.
                        let voices = crate::audio::scomp::ScompVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, crate::params::scomp::LEVEL_MAX)
                        } else {
                            crate::scomp::ScompParams::default().level
                        };
                        Node::Scomp {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }

                    Some(NodeSpec::Stab {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        let voices = crate::audio::stab::StabVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, crate::params::stab::LEVEL_MAX)
                        } else {
                            crate::audio::stab::StabParams::default().level
                        };
                        Node::Stab {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Quad {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        let voices = crate::audio::quad::QuadVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, crate::params::quad::LEVEL_MAX)
                        } else {
                            crate::audio::quad::QuadParams::default().level
                        };
                        Node::Quad {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Brick {
                        notes,
                        subloops,
                        loop_len_beats,
                        path,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: the file, from the cache, at the
                        // device's rate. A missing file is a silent brick.
                        let material = if path.as_os_str().is_empty() {
                            crate::audio::material::Material::empty()
                        } else {
                            crate::audio::material::load_cached(path, sample_rate)
                                .unwrap_or_else(|_| crate::audio::material::Material::empty())
                        };
                        let voices = crate::audio::brick::BrickVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                            material,
                        );
                        let gain = if params.level.is_finite() {
                            params.level.clamp(0.0, crate::params::brick::LEVEL_MAX)
                        } else {
                            crate::audio::brick::BrickParams::default().level
                        };
                        Node::Brick {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Poly {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: this builds every waveform table and
                        // every scratch buffer the callback will ever need.
                        // `block_frames` is the longest segment there can be.
                        let voices = crate::audio::poly::PolyVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        // RON round-trips NaN literals, so a hand-edited or
                        // corrupt project could smuggle one in.
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::poly::PolyParams::default().gain
                        };
                        Node::Poly {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::Loom {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        let events = compile_events(
                            notes,
                            subloops,
                            *loop_len_beats,
                            samples_per_beat,
                            plock_glide_samples,
                        )?;
                        // Green zone: every waveform table and every
                        // scratch buffer the callback will ever need.
                        let voices = crate::audio::loom::LoomVoices::new(
                            sample_rate as f32,
                            block_frames,
                            *params,
                        );
                        let gain = if params.gain.is_finite() {
                            params.gain.clamp(0.0, 2.0)
                        } else {
                            crate::audio::loom::LoomParams::default().gain
                        };
                        Node::Loom {
                            events,
                            clock: PatternClock::new(
                                loop_len_beats
                                    .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                    .unwrap_or(0),
                                samples_per_beat,
                            ),
                            voices: Box::new(voices),
                            gain,
                            target_gain: gain,
                        }
                    }
                    Some(NodeSpec::AudioClip {
                        path,
                        start_beats,
                        length_beats,
                        source_offset_frames,
                        source_frames,
                        loop_clip,
                        loop_start_frames,
                        gain,
                        fade_in_frames,
                        fade_out_frames,
                        fade_in_shape,
                        fade_out_shape,
                        envelope,
                    }) => {
                        // Green zone: opening (and, if the interface rate
                        // changed, cache conversion) touches disk and may
                        // spawn creek's worker. An unreadable file is a
                        // silent failed clip rather than a refused project.
                        match audio_stream_at_rate(path, sample_rate) {
                            Some((mut st, original_frames, original_rate)) => {
                                let frames = st.info().num_frames as u64;
                                // Edit coordinates belong to the rate of the
                                // file named by the spec. Map BOUNDARIES into
                                // the prepared stream's domain so trim, brace,
                                // fade and envelope all preserve seconds.
                                let original_start = (*source_offset_frames).min(original_frames);
                                let original_available =
                                    original_frames.saturating_sub(original_start);
                                let original_len = (*source_frames)
                                    .unwrap_or(original_available)
                                    .min(original_available);
                                let original_end = original_start.saturating_add(original_len);
                                let source_start =
                                    frame_at_rate(original_start, original_rate, sample_rate)
                                        .min(frames);
                                let source_end =
                                    frame_at_rate(original_end, original_rate, sample_rate)
                                        .min(frames)
                                        .max(source_start);
                                let source_frames = source_end.saturating_sub(source_start);
                                let original_loop = original_start.saturating_add(
                                    (*loop_start_frames).min(original_len.saturating_sub(1)),
                                );
                                let loop_from =
                                    frame_at_rate(original_loop, original_rate, sample_rate).clamp(
                                        source_start,
                                        source_end.saturating_sub(1).max(source_start),
                                    );
                                let timeline_start =
                                    ((*start_beats).max(0.0) * samples_per_beat).round() as u64;
                                let timeline_frames = (*length_beats).map_or_else(
                                    || {
                                        if *loop_clip {
                                            u64::MAX.saturating_sub(timeline_start)
                                        } else {
                                            source_frames
                                        }
                                    },
                                    |beats| (beats.max(0.0) * samples_per_beat).round() as u64,
                                );
                                // Pin both legal jump targets. The seek after
                                // them is required: creek begins prefetch only
                                // after a seek (new -> cache -> seek).
                                let _ = st.cache(0, source_start as usize);
                                if loop_from != source_start && st.num_caches() > 1 {
                                    let _ = st.cache(1, loop_from as usize);
                                }
                                let _ = st.seek(source_start as usize, SeekMode::Auto);
                                Node::AudioClip {
                                    stream: Some(st),
                                    resident: if *loop_clip && frames <= 65_536 {
                                        crate::audio::material::load_cached(path, sample_rate).ok()
                                    } else {
                                        None
                                    },
                                    file_frames: frames,
                                    timeline_start,
                                    timeline_frames,
                                    source_start,
                                    source_frames,
                                    loop_clip: *loop_clip,
                                    // Clamped into the played region, never
                                    // onto its end (zero-length modulus is a
                                    // callback panic).
                                    loop_from,
                                    gain: 0.0, // ramp in
                                    target_gain: *gain,
                                    // Clamped to the span they live on:
                                    // a fade longer than its clip would
                                    // never reach full level, and two
                                    // that overlap would fight.
                                    fade_in: frame_at_rate(
                                        *fade_in_frames,
                                        original_rate,
                                        sample_rate,
                                    )
                                    .min(timeline_frames),
                                    fade_out: frame_at_rate(
                                        *fade_out_frames,
                                        original_rate,
                                        sample_rate,
                                    )
                                    .min(timeline_frames),
                                    fade_in_curve: crate::params::clip::Curve::new(*fade_in_shape),
                                    fade_out_curve: crate::params::clip::Curve::new(
                                        *fade_out_shape,
                                    ),
                                    // Sorted and clamped HERE, green
                                    // side: the callback walks this list
                                    // assuming it rises, and sorting it
                                    // there would be both an allocation
                                    // and an unbounded path.
                                    envelope: sorted_envelope(
                                        &envelope
                                            .iter()
                                            .map(|(at, gain)| {
                                                (
                                                    frame_at_rate(*at, original_rate, sample_rate),
                                                    *gain,
                                                )
                                            })
                                            .collect::<Vec<_>>(),
                                        timeline_frames,
                                    ),
                                    envelope_cursor: 0,
                                    failed: false,
                                    next_frame: source_start,
                                }
                            }
                            None => Node::AudioClip {
                                stream: None,
                                resident: None,
                                file_frames: 0,
                                timeline_start: 0,
                                timeline_frames: 0,
                                source_start: 0,
                                source_frames: 0,
                                loop_clip: *loop_clip,
                                loop_from: 0,
                                gain: 0.0,
                                target_gain: *gain,
                                fade_in: 0,
                                fade_out: 0,
                                fade_in_curve: crate::params::clip::Curve::LINEAR,
                                fade_out_curve: crate::params::clip::Curve::LINEAR,
                                envelope: Vec::new(),
                                envelope_cursor: 0,
                                failed: true,
                                next_frame: 0,
                            },
                        }
                    }
                    Some(NodeSpec::Pan { pan, gain }) => {
                        // Sanitised HERE, because this is the one door into
                        // the node's state that the letter path's finiteness
                        // guard does not cover: a spec comes from a project
                        // file, and RON will happily parse `gain: NaN`. A
                        // bare `clamp` would not do — `f32::clamp` returns
                        // NaN for a NaN input, so the poison would arrive
                        // wearing a clamp's clothes and then stick, since
                        // every block copies target into current.
                        // Non-finite is nonsense and becomes unity — a
                        // corrupt file is not a request to be infinitely
                        // loud. Merely out of range is a request, and gets
                        // clamped.
                        let level = if gain.is_finite() {
                            gain.clamp(0.0, crate::params::pan::TABLE[1].max)
                        } else {
                            1.0
                        };
                        let placement = if pan.is_finite() {
                            pan.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        };
                        Node::Pan {
                            pan: placement,
                            target_pan: placement,
                            // Unlike the master Mixer, a track's level does
                            // NOT ramp in from zero: a schedule swap mid
                            // playback would fade every track back in,
                            // which reads as a glitch, not as politeness.
                            gain: level,
                            target_gain: level,
                            gain_is_timeline_automated: self.automation.iter().any(|lane| {
                                lane.node == *id && lane.param == crate::params::pan::GAIN
                            }),
                        }
                    }
                    Some(NodeSpec::Reverb {
                        predelay_ms,
                        size,
                        decay,
                        damp,
                        low_cut,
                        diffusion,
                        modulation,
                        width,
                        mix,
                    }) => {
                        // Green zone: this is where a reverb's memory is
                        // allowed to be born. The kernel only ever indexes
                        // inside it.
                        let sr = sample_rate as f32;
                        let mut buffers = vec![0.0f32; crate::dsp::fdn::Fdn::buffer_len(sr)];
                        let mut core = crate::dsp::fdn::Fdn::new();
                        core.prepare(sr, &mut buffers);
                        core.set_size(*size);
                        core.set_decay(*decay);
                        core.set_damping(*damp);
                        core.set_diffusion(*diffusion);
                        core.set_modulation(*modulation);
                        // Green zone: every buffer the callback will ever
                        // need, allocated once at compile.
                        let mut predelay = crate::dsp::delay::DelayLine::new();
                        let max_predelay =
                            (crate::params::reverb::PREDELAY_MAX * 0.001 * sr).ceil() as usize;
                        let mut predelay_buf =
                            vec![0.0f32; crate::dsp::delay::buffer_len(max_predelay)];
                        predelay.prepare(max_predelay);
                        predelay.set_delay(*predelay_ms * 0.001 * sr);
                        let mut low_cut_l = crate::dsp::filters::OnePole::new();
                        let mut low_cut_r = crate::dsp::filters::OnePole::new();
                        low_cut_l.prepare(sr, *low_cut);
                        low_cut_r.prepare(sr, *low_cut);
                        let _ = &mut predelay_buf;
                        Node::Reverb {
                            core,
                            buffers,
                            predelay,
                            predelay_buf,
                            fed: vec![0.0f32; block_frames],
                            wet_l: vec![0.0f32; block_frames],
                            wet_r: vec![0.0f32; block_frames],
                            low_cut_l,
                            low_cut_r,
                            mix: mix.clamp(0.0, 1.0),
                            target_mix: mix.clamp(0.0, 1.0),
                            width: *width,
                            sample_rate: sr,
                        }
                    }
                    Some(NodeSpec::Filter { params }) => {
                        // Green zone: everything heap-shaped is born
                        // here. The core itself allocates nothing — the
                        // scratch below is every buffer it touches, and
                        // it clamps each value through the same table
                        // rows the letters will, so a stale project file
                        // cannot smuggle an out-of-range setting in.
                        let mut core = Box::new(crate::audio::filter::FilterCore::new());
                        core.prepare(sample_rate as f32, *params);
                        Node::Filter {
                            core,
                            scratch: vec![0.0f32; crate::audio::filter::scratch_len(block_frames)],
                        }
                    }
                    Some(NodeSpec::Modulato { params }) => {
                        // Green zone: every delay line and scratch the
                        // callback will ever need, born here.
                        Node::Modulato {
                            core: Box::new(crate::audio::modulato::Modulato::new(
                                sample_rate as f32,
                                block_frames,
                                *params,
                            )),
                        }
                    }
                    Some(NodeSpec::Limiter { params }) => {
                        // Green zone: everything heap-shaped is born
                        // here. The core itself allocates nothing — every
                        // buffer it reads or writes is one of these.
                        let mut core = Box::new(crate::audio::limiter::LimiterCore::new());
                        core.prepare(sample_rate as f32, *params);
                        let line = crate::audio::limiter::line_len();
                        Node::Limiter {
                            core,
                            scratch: vec![0.0f32; crate::audio::limiter::scratch_len(block_frames)],
                            key: vec![0.0f32; line],
                            line_l: vec![0.0f32; line],
                            line_r: vec![0.0f32; line],
                        }
                    }
                    Some(NodeSpec::Sat {
                        mode,
                        drive,
                        bias,
                        mix,
                        out,
                    }) => {
                        use crate::params::sat as sp;
                        // Green zone: everything heap-shaped is born here,
                        // and every value clamps through the same table
                        // rows the letters will — a stale project file
                        // cannot smuggle an out-of-range setting in.
                        let sr = sample_rate as f32;
                        let mode = (*mode).min(sp::MODE_MAX);
                        let drive = sp::TABLE[sp::DRIVE as usize].clamp(*drive);
                        let bias = sp::TABLE[sp::BIAS as usize].clamp(*bias);
                        let mix = sp::TABLE[sp::MIX as usize].clamp(*mix);
                        let trim = sp::TABLE[sp::OUT as usize].clamp(*out);
                        let mut core = Box::new(SatCore {
                            shaper: crate::dsp::shaper::Waveshaper::new(),
                            oversampler: [crate::dsp::shaper::Oversampler2x::new(); 2],
                            dc: [crate::dsp::filters::DcBlocker::new(); 2],
                        });
                        core.shaper.configure(sp::mode(mode), drive, bias, 1.0);
                        for dc in core.dc.iter_mut() {
                            dc.prepare(sr);
                        }
                        let smoother = |value: f32| {
                            let mut s = crate::dsp::ramps::Smoother::new();
                            s.prepare(sr, SAT_SMOOTH_MS);
                            s.set_now(value);
                            s
                        };
                        Node::Sat {
                            core,
                            // Three 2x lanes: left, right, and the shaped
                            // copy the node blends against.
                            scratch2x: vec![0.0f32; block_frames * 6],
                            mode,
                            pending_mode: mode,
                            drive: smoother(drive),
                            bias: smoother(bias),
                            mix: smoother(mix),
                            trim: smoother(trim),
                            drive_target: drive,
                            bias_target: bias,
                            mix_target: mix,
                            trim_target: trim,
                        }
                    }
                    Some(NodeSpec::Lofi {
                        rate,
                        bits,
                        mix,
                        out,
                    }) => {
                        use crate::params::lofi as lp;
                        // Green zone: the scratch is born here, once, and
                        // every value clamps through the same table rows
                        // the letters will — a stale project file cannot
                        // smuggle an out-of-range setting in.
                        let sr = sample_rate as f32;
                        let rate = lp::TABLE[lp::RATE as usize].clamp(*rate);
                        let bits = lp::TABLE[lp::BITS as usize].clamp(*bits);
                        let mix = lp::TABLE[lp::MIX as usize].clamp(*mix);
                        let trim = lp::TABLE[lp::OUT as usize].clamp(*out);
                        let mut core = [crate::dsp::lofi::Downsampler::new(); 2];
                        for ch in core.iter_mut() {
                            ch.prepare(sr);
                            ch.set_rate(rate);
                            ch.set_bits(bits);
                        }
                        let smoother = |value: f32| {
                            let mut s = crate::dsp::ramps::Smoother::new();
                            s.prepare(sr, LOFI_SMOOTH_MS);
                            s.set_now(value);
                            s
                        };
                        Node::Lofi {
                            core,
                            // Two lanes of dry, left and right.
                            dry: vec![0.0f32; block_frames * 2],
                            rate,
                            bits,
                            mix: smoother(mix),
                            trim: smoother(trim),
                            mix_target: mix,
                            trim_target: trim,
                        }
                    }
                    Some(NodeSpec::Sheen {
                        amount,
                        edge_hz,
                        mix,
                        out,
                    }) => {
                        use crate::params::sheen as hp;
                        // Green zone: the scratch is born here, once, and
                        // every value clamps through the same table rows
                        // the letters will.
                        let sr = sample_rate as f32;
                        let amount = hp::TABLE[hp::AMOUNT as usize].clamp(*amount);
                        let edge_hz = hp::TABLE[hp::EDGE as usize].clamp(*edge_hz);
                        let mix = hp::TABLE[hp::MIX as usize].clamp(*mix);
                        let trim = hp::TABLE[hp::OUT as usize].clamp(*out);
                        let mut core = [crate::dsp::dynamics::SlewBrighten::new(); 2];
                        for ch in core.iter_mut() {
                            ch.prepare(sr, edge_hz, amount);
                        }
                        let smoother = |value: f32| {
                            let mut s = crate::dsp::ramps::Smoother::new();
                            s.prepare(sr, SHEEN_SMOOTH_MS);
                            s.set_now(value);
                            s
                        };
                        Node::Sheen {
                            core,
                            // Two lanes of dry, left and right.
                            dry: vec![0.0f32; block_frames * 2],
                            sample_rate: sr,
                            amount,
                            edge_hz,
                            mix: smoother(mix),
                            trim: smoother(trim),
                            mix_target: mix,
                            trim_target: trim,
                        }
                    }
                    Some(NodeSpec::Disperser {
                        amount,
                        freq_hz,
                        pinch,
                    }) => {
                        use crate::params::disperser as dp;
                        // Green zone: the box is born here, and every
                        // value clamps through the same table rows the
                        // letters will.
                        let sr = sample_rate as f32;
                        let stages = dp::TABLE[dp::AMOUNT as usize]
                            .clamp(*amount)
                            .round()
                            .max(0.0) as u32;
                        let freq_hz = dp::TABLE[dp::FREQ as usize].clamp(*freq_hz);
                        let pinch = dp::TABLE[dp::PINCH as usize].clamp(*pinch);
                        let mut core = Box::new([crate::dsp::filters::Disperser::new(); 2]);
                        for ch in core.iter_mut() {
                            ch.prepare(sr, freq_hz, pinch, stages);
                        }
                        Node::Disperser {
                            core,
                            sample_rate: sr,
                            freq_hz,
                            pinch,
                            stages,
                        }
                    }
                    Some(NodeSpec::Tilt { tilt_db, pivot_hz }) => {
                        use crate::params::tilt as tp;
                        // Green zone: every value clamps through the same
                        // table rows the letters will.
                        let sr = sample_rate as f32;
                        let tilt_db = tp::TABLE[tp::TILT as usize].clamp(*tilt_db);
                        let pivot_hz = tp::TABLE[tp::PIVOT as usize].clamp(*pivot_hz);
                        let mut core = [crate::dsp::filters::Tilt::new(); 2];
                        for ch in core.iter_mut() {
                            ch.prepare(sr, pivot_hz, tilt_db);
                        }
                        Node::Tilt {
                            core,
                            sample_rate: sr,
                            pivot_hz,
                            tilt_db,
                        }
                    }
                    Some(NodeSpec::Phaser {
                        amount,
                        centre_hz,
                        depth_oct,
                        rate_hz,
                        mix,
                    }) => {
                        use crate::params::phaser as pp;
                        // Green zone: the box and the scratch are born
                        // here, and every value clamps through the same
                        // table rows the letters will.
                        let sr = sample_rate as f32;
                        let stages = pp::TABLE[pp::AMOUNT as usize]
                            .clamp(*amount)
                            .round()
                            .max(0.0) as u32;
                        let centre_hz = pp::TABLE[pp::CENTRE as usize].clamp(*centre_hz);
                        let depth_oct = pp::TABLE[pp::DEPTH as usize].clamp(*depth_oct);
                        let rate_hz = pp::TABLE[pp::RATE as usize].clamp(*rate_hz);
                        let mix = pp::TABLE[pp::MIX as usize].clamp(*mix);
                        let mut core = Box::new([crate::dsp::filters::Disperser::new(); 2]);
                        for ch in core.iter_mut() {
                            ch.prepare(sr, centre_hz, PHASER_Q, stages);
                        }
                        let mut lfo = crate::dsp::lfo::Lfo::new();
                        lfo.prepare(sr);
                        lfo.set_shape(crate::dsp::lfo::LfoShape::Sine);
                        lfo.set_rate(rate_hz);
                        let mut smoother = crate::dsp::ramps::Smoother::new();
                        smoother.prepare(sr, PHASER_SMOOTH_MS);
                        smoother.set_now(mix);
                        Node::Phaser {
                            core,
                            lfo,
                            // Two lanes of dry, left and right.
                            dry: vec![0.0f32; block_frames * 2],
                            sample_rate: sr,
                            stages,
                            centre_hz,
                            depth_oct,
                            mix: smoother,
                            mix_target: mix,
                        }
                    }
                    Some(NodeSpec::Echo {
                        sync,
                        time_ms,
                        feedback,
                        tone_hz,
                        drive,
                        wow,
                        spread,
                        mix,
                        send,
                    }) => {
                        use crate::params::echo as ep;
                        // Green zone: the rings are born here, once, at
                        // the longest echo the table allows. The red zone
                        // never asks for more than this, because
                        // `time_samples` clamps to the same figure.
                        let sr = sample_rate as f32;
                        let clamp = |id: u32, v: f32| ep::TABLE[id as usize].clamp(v);
                        let sync = (*sync).min(ep::SYNC_NAMES.len() as u32 - 1);
                        let time_ms = clamp(ep::TIME, *time_ms);
                        let feedback = clamp(ep::FEEDBACK, *feedback);
                        let tone_hz = clamp(ep::TONE, *tone_hz);
                        let drive = clamp(ep::DRIVE, *drive);
                        let wow = clamp(ep::WOW, *wow);
                        let spread = clamp(ep::SPREAD, *spread);
                        let mix = clamp(ep::MIX, *mix);
                        // An aux by the same rule the graph builder used
                        // when it decided how to wire this node — one
                        // rule, read twice, so the topology and the gain
                        // cannot disagree about which side it is on.
                        let send = clamp(ep::SEND, *send);
                        let aux = send > 0.0;

                        let max_samples = (ep::MAX_MS * 1e-3 * sr).ceil() as usize;
                        let ring_len = crate::dsp::delay::FeedbackDelay::needed_len(max_samples);
                        let mut core = Box::new(EchoCore {
                            line: [crate::dsp::delay::FeedbackDelay::new(); 2],
                            wow: [crate::dsp::lfo::Lfo::new(); 2],
                        });
                        for line in core.line.iter_mut() {
                            line.prepare(sr, max_samples, tone_hz);
                            line.set_feedback(feedback * 0.01);
                            line.set_drive(drive * 0.01 * ECHO_DRIVE_MAX);
                        }
                        for (i, w) in core.wow.iter_mut().enumerate() {
                            w.prepare(sr);
                            w.set_shape(crate::dsp::lfo::LfoShape::Sine);
                            w.set_rate(ECHO_WOW_HZ);
                            // The two sides wobble a quarter turn apart,
                            // so the image drifts instead of the pitch
                            // moving in mono.
                            w.set_phase(i as f32 * ECHO_WOW_SPREAD_TURNS);
                        }
                        let smoother = |value: f32, ms: f32| {
                            let mut s = crate::dsp::ramps::Smoother::new();
                            s.prepare(sr, ms);
                            s.set_now(value);
                            s
                        };
                        // The first segment recomputes this against the
                        // real tempo; starting the glide already there
                        // means a freshly loaded echo does not swoop.
                        let start = ep::time_samples(sync, time_ms, sr, 0.0);
                        Node::Echo {
                            core,
                            rings: [vec![0.0f32; ring_len], vec![0.0f32; ring_len]],
                            scratch: vec![0.0f32; block_frames * 4],
                            sync,
                            pending_sync: sync,
                            time: smoother(start, ECHO_GLIDE_MS),
                            time_ms,
                            feedback: smoother(feedback, ECHO_SMOOTH_MS),
                            tone: smoother(tone_hz, ECHO_SMOOTH_MS),
                            drive: smoother(drive, ECHO_SMOOTH_MS),
                            wow: smoother(wow, ECHO_SMOOTH_MS),
                            spread: smoother(spread, ECHO_SMOOTH_MS),
                            mix: smoother(mix, ECHO_SMOOTH_MS),
                            aux,
                            // An insert reads unity and never looks
                            // again; only an aux carries the tap's gain.
                            in_gain: if aux { send * 0.01 } else { 1.0 },
                            in_target: if aux { send * 0.01 } else { 1.0 },
                            time_target: start,
                            feedback_target: feedback,
                            tone_target: tone_hz,
                            drive_target: drive,
                            wow_target: wow,
                            spread_target: spread,
                            mix_target: mix,
                            sample_rate: sr,
                        }
                    }
                    Some(NodeSpec::Eq { params }) => Node::Eq {
                        // Green zone: every smoother, every coefficient
                        // and the one scratch lane the callback will use.
                        core: Box::new(crate::audio::eq::EqCore::new(
                            sample_rate as f32,
                            block_frames,
                            params,
                        )),
                    },
                    Some(NodeSpec::Tone { params }) => Node::Tone {
                        // Green zone: the wavetables are born here.
                        core: Box::new(crate::audio::tone::ToneCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Sigil { params }) => Node::Sigil {
                        // Green zone: the carrier's tables are born here.
                        core: Box::new(crate::audio::sigil::SigilCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Gauge { params }) => Node::Gauge {
                        core: Box::new(crate::audio::gauge::GaugeCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Umbra { params }) => Node::Umbra {
                        // Green zone: twenty buffers and a table of
                        // wavetables, all born here.
                        core: Box::new(crate::audio::umbra::UmbraCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Ferric { params }) => Node::Ferric {
                        // Green zone: the reel is allocated HERE. It is
                        // the largest thing any device in this file owns
                        // and the callback must never see it born.
                        core: Box::new(crate::audio::ferric::FerricCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Sibyl { params }) => Node::Sibyl {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::sibyl::SibylCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Resyn { params }) => Node::Resyn {
                        // Green zone: every buffer the callback will use,
                        // and there are a lot of them.
                        core: Box::new(crate::audio::resyn::ResynCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Strip { params }) => Node::Strip {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::strip::StripCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Section { params }) => Node::Section {
                        // Green zone: the kind's core, buffers and all.
                        core: crate::audio::console::core_of(
                            params,
                            sample_rate as f32,
                            block_frames,
                        ),
                    },
                    Some(NodeSpec::Gate { params }) => Node::Gate {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::gate::GateCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Prism { params }) => Node::Prism {
                        // Green zone: the crossover, three sets of
                        // dynamics, and the band buffers.
                        core: Box::new(crate::audio::prism::Prism::new(sample_rate as f32, params)),
                    },
                    Some(NodeSpec::Clamp { params }) => Node::Clamp {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::clamp::Clamp::new(sample_rate as f32, params)),
                    },
                    Some(NodeSpec::Flint { params }) => Node::Flint {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::flint::Flint::new(sample_rate as f32, params)),
                    },
                    Some(NodeSpec::Glue { params }) => Node::Glue {
                        // Green zone: every kernel the callback will use.
                        core: Box::new(crate::audio::glue::GlueCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Utility { params }) => Node::Utility {
                        // Green zone: both filters, prepared here.
                        core: Box::new(crate::audio::utility::UtilityCore::new(
                            sample_rate as f32,
                            params,
                        )),
                    },
                    Some(NodeSpec::Click) => Node::Click {
                        phase: 0.0,
                        env: 0.0,
                        last_beat: -1,
                        sample_rate: sample_rate as f32,
                    },
                })
            })
            .collect::<Result<_, _>>()?;

        // Liveness-based slot assignment: a node's slot is released after the
        // step of its LAST consumer, and reused by later steps. The output
        // node's slot is never released — the device copy reads it after the
        // whole walk. Allocation order upholds the aliasing invariant run()
        // depends on: a step's out slot is taken from the free list while its
        // inputs still hold theirs.
        let output_dense = match &self.output {
            None => None,
            Some(id) => Some(dense_of(id).ok_or(CompileError::DanglingOutput)?),
        };

        let mut pos_of = vec![0usize; n]; // node -> its position in topo order
        for (p, &i) in topo.iter().enumerate() {
            pos_of[i] = p;
        }
        // last_use[i]: topo position after which i's slot may be recycled.
        // Default = its own step (an unconsumed node frees immediately after).
        let mut last_use: Vec<usize> = (0..n).map(|i| pos_of[i]).collect();
        for (from, to) in &self.wires {
            // Wires were validated above; positions always resolve here.
            if let (Some(f), Some(t)) = (dense_of(from), dense_of(to)) {
                last_use[f] = last_use[f].max(pos_of[t]);
            }
        }
        if let Some(d) = output_dense {
            last_use[d] = usize::MAX; // survives the walk
        }

        // Channel count per node (by dense index, from the spec).
        let ch_of: Vec<u8> = self
            .order
            .iter()
            .map(|id| self.nodes.get(id.0).map(Node::channels).unwrap_or(1))
            .collect();

        // Liveness allocation, per CHANNEL: a node takes ch_of[i] slots while
        // live and releases them all after its last consumer. Out slots are
        // drawn while inputs still hold theirs — the aliasing invariant.
        let mut slots_of: Vec<[usize; 2]> = vec![[0; 2]; n];
        let mut freed = vec![false; n]; // guards double-free on doubled wires
        let mut free: Vec<usize> = Vec::new();
        let mut num_slots = 0usize;
        let mut alloc_one = |free: &mut Vec<usize>| {
            free.pop().unwrap_or_else(|| {
                num_slots += 1;
                num_slots - 1
            })
        };
        for (p, &i) in topo.iter().enumerate() {
            for slot in slots_of[i].iter_mut().take(ch_of[i] as usize) {
                *slot = alloc_one(&mut free);
            }
            for &f in &in_wires[i] {
                if last_use[f] == p && !freed[f] {
                    freed[f] = true;
                    for &slot in slots_of[f].iter().take(ch_of[f] as usize) {
                        free.push(slot);
                    }
                }
            }
            if last_use[i] == p {
                freed[i] = true;
                for &slot in slots_of[i].iter().take(ch_of[i] as usize) {
                    free.push(slot);
                }
            }
        }

        let chan_buf = |i: usize| ChanBuf {
            slots: [SlotId(slots_of[i][0]), SlotId(slots_of[i][1])],
            ch: ch_of[i],
        };
        let steps: Vec<Step> = topo
            .iter()
            .map(|&i| Step {
                node: i,
                out: chan_buf(i),
                ins: in_wires[i].iter().map(|&f| chan_buf(f)).collect(),
            })
            .collect();

        let output_slot = output_dense.map(chan_buf);

        // Name-tag directory: slot -> (generation, dense index).
        let max_slot = self
            .order
            .iter()
            .map(|id| id.to_bits() as u32 as usize)
            .max()
            .unwrap_or(0);
        let mut slot_table = vec![(0u32, 0u32); if n == 0 { 0 } else { max_slot + 1 }];
        for (dense, id) in self.order.iter().enumerate() {
            let bits = id.to_bits();
            slot_table[bits as u32 as usize] = ((bits >> 32) as u32, dense as u32);
        }

        // Meter taps, resolved from node ids to STEP indices — the hot
        // loop knows steps, not ids. A tap on a node that did not survive
        // compilation (an unreachable branch, a removed node) simply finds
        // no step and reports silence.
        // The readout taps, resolved the same way and into the same slot
        // space. A tap on a node that did not survive compilation simply
        // finds no step and reports nothing.
        let mut step_tap = vec![NO_METER; steps.len()];
        for (slot, node) in &self.taps {
            if *slot >= MAX_METERS {
                continue;
            }
            let Some(dense) = self.order.iter().position(|id| id == node) else {
                continue;
            };
            if let Some(step) = steps.iter().position(|step| step.node == dense) {
                step_tap[step] = *slot as u8;
            }
        }

        let mut step_telemetry = vec![NO_METER; steps.len()];
        for (slot, node) in &self.telemetry {
            if *slot >= MAX_TELEMETRY {
                continue;
            }
            let Some(dense) = self.order.iter().position(|id| id == node) else {
                continue;
            };
            if let Some(step) = steps.iter().position(|step| step.node == dense) {
                step_telemetry[step] = *slot as u8;
            }
        }

        let mut step_meter = vec![NO_METER; steps.len()];
        for (slot, node) in &self.meters {
            if *slot >= MAX_METERS {
                continue;
            }
            // `Step::node` is the DENSE index, and dense order is exactly
            // `self.order` — the same correspondence `slot_table` is built
            // from just below.
            let Some(dense) = self.order.iter().position(|id| id == node) else {
                continue;
            };
            if let Some(step) = steps.iter().position(|step| step.node == dense) {
                step_meter[step] = *slot as u8;
            }
        }

        Ok(Schedule {
            epoch: next_schedule_epoch(),
            timeline: timeline.cloned(),
            nodes,
            steps,
            arena: Arena::new(num_slots.max(1), block_frames),
            output_slot,
            slot_table,
            step_meter,
            step_tap,
            readouts: [Readout::default(); MAX_METERS],
            step_telemetry,
            telemetry: [Readout::default(); MAX_TELEMETRY],
            peaks_l: [0.0; MAX_METERS],
            peaks_r: [0.0; MAX_METERS],
            // Wires resolve against the SAME dense correspondence the
            // name-tag directory uses, so a wire and a letter can never
            // disagree about which node a parameter lives on.
            modulation: ModPlan::compile(&self.modulation, sample_rate as f32, |node| {
                self.order.iter().position(|id| *id == node)
            }),
            automation: AutomationPlan::compile(&self.automation, samples_per_beat, timeline),
            // The deepest path to the output. Compensation has already made
            // every path agree, so summing along any one of them gives the
            // same answer; the walk below takes the max regardless.
            fx_bases: self
                .fx_bases
                .iter()
                .map(|(node, param, base)| FxBase {
                    node: node.to_bits(),
                    param: *param,
                    base: *base,
                    live: *base,
                    locked: false,
                })
                .collect(),
            latency: output_dense.map_or(0, |out| {
                let mut arrival = vec![0usize; n];
                for &i in &topo {
                    arrival[i] = in_wires[i]
                        .iter()
                        .map(|&f| {
                            arrival[f]
                                + self
                                    .order
                                    .get(f)
                                    .and_then(|id| self.nodes.get(id.0))
                                    .map_or(0, |spec| Self::spec_latency(spec, sample_rate))
                        })
                        .max()
                        .unwrap_or(0);
                }
                arrival[out]
                    + self
                        .order
                        .get(out)
                        .and_then(|id| self.nodes.get(id.0))
                        .map_or(0, |spec| Self::spec_latency(spec, sample_rate))
            }),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    const NO_INPUT: &[f32] = &[0.0; 512];

    fn test_note(start: f64) -> Note {
        Note {
            start_beats: start,
            len_beats: 0.25,
            pitch: 60,
            vel: 100,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }
    }

    #[test]
    fn automation_is_sample_stamped_and_same_sample_points_are_last_wins() {
        let mut spec = GraphSpec::default();
        let gain = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.set_output(gain);
        spec.automate(
            gain,
            0,
            1.0,
            vec![
                AutomationPoint {
                    beat: 0.0,
                    value: 0.25,
                    bend: 0.0,
                },
                // Also rounds to sample zero. Authored order is the tie
                // breaker, exactly as it is in the Song lane.
                AutomationPoint {
                    beat: 0.000_001,
                    value: 0.5,
                    bend: 0.0,
                },
                AutomationPoint {
                    beat: 1.0,
                    value: 1.0,
                    bend: 0.0,
                },
            ],
        );
        let sched = spec.compile_at_tempo(48_000, 256, 120.0).unwrap();
        let lane = &sched.automation.lanes[0];
        assert_eq!(lane.points.len(), 2);
        assert_eq!(lane.points[0].sample, 0);
        assert_eq!(lane.points[0].value, 0.5);
        assert_eq!(lane.points[1].sample, 24_000);
        assert_eq!(sched.automation.value_at(lane, 12_000), 0.75);
    }

    #[test]
    fn automation_uses_the_arrangement_tempo_table_and_absolute_control_grid() {
        let mut song = crate::sequencing::Song::default();
        assert!(song.set_tempo_mark(0, 120.0));
        assert!(song.set_tempo_mark(crate::sequencing::TICKS_PER_BEAT, 60.0));
        let timeline = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);
        let mut spec = GraphSpec::default();
        let gain = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.set_output(gain);
        spec.automate(
            gain,
            0,
            1.0,
            vec![AutomationPoint {
                beat: 2.0,
                value: 0.0,
                bend: 0.0,
            }],
        );
        let sched = spec
            .compile_with_tempo_table(48_000, 257, 120.0, &timeline)
            .unwrap();
        assert_eq!(sched.automation.lanes[0].points[0].sample, 72_000);
        assert_eq!(sched.frames_until_control_change(113, 257), 15);
        assert_eq!(sched.frames_until_control_change(71_999, 257), 1);
    }

    #[test]
    fn pattern_lock_stays_above_moving_automation_until_restore() {
        let mut spec = GraphSpec::default();
        let gain = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.set_output(gain);
        spec.lock_base(gain, 0, 1.0);
        spec.automate(
            gain,
            0,
            1.0,
            vec![
                AutomationPoint {
                    beat: 0.0,
                    value: 1.0,
                    bend: 0.0,
                },
                AutomationPoint {
                    beat: 1.0,
                    value: 0.0,
                    bend: 0.0,
                },
            ],
        );
        let mut sched = spec.compile_at_tempo(48_000, 256, 120.0).unwrap();
        let node = gain.to_bits();
        sched.fx_bases[0].live = 0.25;
        sched.fx_bases[0].locked = true;
        sched.apply_to(node, 0, 0.25);

        let mut out = [0.0; 512];
        let during_curve = ProcessCtx {
            position: 11_744,
            beat: 11_744.0 / 24_000.0,
            ..ctx(NO_INPUT)
        };
        sched.run(&mut out, &during_curve);
        assert_eq!(sched.fx_bases[0].live, 0.25, "the lock stays on top");
        assert!(
            (sched.fx_bases[0].base - 0.5).abs() < 0.001,
            "the curve continues underneath the lock: {}",
            sched.fx_bases[0].base
        );

        // Model the explicit final restore event. The next timeline segment
        // is then free to own the audible target again.
        sched.fx_bases[0].live = sched.fx_bases[0].base;
        sched.fx_bases[0].locked = false;
        let after_restore = ProcessCtx {
            position: 12_000,
            beat: 0.5,
            ..ctx(NO_INPUT)
        };
        sched.run(&mut out, &after_restore);
        assert_ne!(sched.fx_bases[0].live, 0.25);
    }

    fn modulated_trigless_effect_lock_schedule() -> (Schedule, usize) {
        use crate::audio::modulation::{Chain, ModKind, ModShape, ModSpec, Modulator, WireSpec};

        let mut spec = GraphSpec::default();
        let mix = spec.push(NodeSpec::Mixer { gain: 0.5 });
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                // At 1 kHz and 60 bpm the lock holds for eight samples;
                // its four-point restore then fires at samples 8..=11.
                len_beats: 0.008,
                pitch: 60,
                vel: 0,
                plocks: Vec::new(),
                fx_locks: vec![(mix.to_bits(), crate::params::mixer::GAIN, 1.0)],
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: SynthParams::default(),
        });
        spec.connect(seq, mix);
        spec.set_output(mix);
        spec.lock_base(mix, crate::params::mixer::GAIN, 0.5);
        spec.set_modulation(ModSpec {
            sources: vec![Modulator {
                id: 1,
                kind: ModKind::Lfo {
                    shape: ModShape::Square,
                    rate_beats: 1.0,
                    free: false,
                    hz: 1.0,
                },
            }],
            wires: vec![WireSpec {
                id: 2,
                source: 1,
                node: mix,
                param: crate::params::mixer::GAIN,
                min: 0.0,
                max: 2.0,
                log: false,
                base: 0.5,
                chain: Chain {
                    depth: 0.1,
                    curve: 0.0,
                    steps: 0,
                    smooth_ms: 0.0,
                },
                enabled: true,
                solo: false,
            }],
        });

        let schedule = spec.compile_at_tempo(1_000, 16, 60.0).unwrap();
        let dense = schedule
            .nodes
            .iter()
            .position(|node| matches!(node, Node::Mixer { .. }))
            .unwrap();
        (schedule, dense)
    }

    fn run_effect_lock_segment(
        schedule: &mut Schedule,
        output: &mut [f32],
        position: u64,
        len: usize,
        discontinuity: bool,
    ) {
        schedule.run(
            output,
            &ProcessCtx {
                device_input: NO_INPUT,
                in_channels: 2,
                block_frames: 16,
                offset: position as usize,
                len,
                playing: true,
                position,
                beat: position as f64 / 1_000.0,
                beats_per_sample: 1.0 / 1_000.0,
                discontinuity,
            },
        );
    }

    fn assert_modulated_mixer_value(schedule: &Schedule, dense: usize, expected: f32) {
        let actual = schedule
            .modulation
            .values()
            .find(|(node, param, _)| *node == dense && *param == crate::params::mixer::GAIN)
            .map(|(_, _, value)| value)
            .unwrap();
        assert!(
            (actual - expected).abs() < 1e-6,
            "modulation plan held {actual}, expected {expected}"
        );
        let Node::Mixer { gain, target_gain } = &schedule.nodes[dense] else {
            panic!("target was not a mixer");
        };
        assert!(
            (*target_gain - expected).abs() < 1e-6,
            "downstream target held {target_gain}, expected {expected}"
        );
        assert!(
            (*gain - expected).abs() < 1e-6,
            "downstream node rendered with {gain}, expected {expected}"
        );
    }

    #[test]
    fn a_modulated_effect_lock_recomposes_in_its_firing_segment() {
        let (mut schedule, dense) = modulated_trigless_effect_lock_schedule();
        let mut output = [0.0; 32];

        // Square is +1 here: depth .1 across a 0..2 span contributes +.2.
        // The sample-zero lock changes the base from .5 to 1.0, so the
        // downstream mixer must see 1.2 during this very segment, not .7.
        run_effect_lock_segment(&mut schedule, &mut output, 0, 8, true);

        assert_eq!(schedule.fx_bases[0].live, 1.0);
        assert!(schedule.fx_bases[0].locked);
        assert_modulated_mixer_value(&schedule, dense, 1.2);
    }

    #[test]
    fn a_modulated_effect_lock_restore_recomposes_in_its_firing_segment() {
        let (mut schedule, dense) = modulated_trigless_effect_lock_schedule();
        let mut output = [0.0; 32];
        run_effect_lock_segment(&mut schedule, &mut output, 0, 8, true);

        // The bounded return fires one point per sample. At the final point
        // the live base is back at .5; current +.2 modulation must therefore
        // land .7 before the downstream mixer renders sample 11.
        for position in 8..=11 {
            run_effect_lock_segment(&mut schedule, &mut output, position, 1, false);
        }

        assert_eq!(schedule.fx_bases[0].live, 0.5);
        assert!(!schedule.fx_bases[0].locked);
        assert_modulated_mixer_value(&schedule, dense, 0.7);
    }

    #[test]
    fn arrangement_compile_stamps_events_and_clock_through_the_tempo_map() {
        let mut song = crate::sequencing::Song::default();
        assert!(song.set_tempo_mark(0, 120.0));
        assert!(song.set_tempo_mark(crate::sequencing::TICKS_PER_BEAT, 60.0));
        let timeline = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);

        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![test_note(2.0)],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: Default::default(),
        });
        spec.set_output(seq);
        let schedule = spec
            .compile_with_tempo_table(48_000, 256, 120.0, &timeline)
            .expect("the mapped arrangement compiles");

        let Node::Seq { events, clock, .. } = &schedule.nodes[0] else {
            panic!("the sequence node changed kind")
        };
        assert_eq!(clock.loop_samples, 0, "arrangement events stay one-shot");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sample, 72_000, "beat 2 follows the slower span");
        assert_eq!(
            events[1].sample, 84_000,
            "the note length uses that span too"
        );
        assert_eq!(schedule.frames_until_tempo_change(0, 48_000), 24_000);
        let fallback = crate::audio::transport::TimeMap {
            bpm: 120.0,
            sample_rate: 48_000.0,
        };
        assert!((schedule.beat_at(48_000, fallback) - 1.5).abs() < 1e-12);
        assert!((schedule.beats_per_sample_at(48_000, fallback) - 1.0 / 48_000.0).abs() < 1e-15);
    }

    #[test]
    fn a_tempo_table_from_another_sample_rate_is_refused() {
        let song = crate::sequencing::Song::default();
        let timeline = crate::tempo::TempoTable::build(&song, 48_000.0, 120.0);
        assert_eq!(
            GraphSpec::default()
                .compile_with_tempo_table(44_100, 256, 120.0, &timeline)
                .err(),
            Some(CompileError::TempoSampleRateMismatch)
        );
    }

    #[test]
    fn dense_tempo_map_can_split_more_than_sixty_four_times_in_one_block() {
        let mut song = crate::sequencing::Song::default();
        assert!(song.set_base_bpm(999.0));
        for tick in 1..=96 {
            let bpm = if tick % 2 == 0 { 998.0 } else { 999.0 };
            assert!(song.set_tempo_mark(tick, bpm));
        }
        let timeline = crate::tempo::TempoTable::build(&song, 48_000.0, 999.0);
        let schedule = GraphSpec::default()
            .compile_with_tempo_table(48_000, 8_192, 999.0, &timeline)
            .expect("the dense map compiles");

        let mut position = 0u64;
        let mut remaining = 8_192usize;
        let mut segments = 0usize;
        while remaining > 0 {
            let len = schedule.frames_until_tempo_change(position, remaining);
            assert!((1..=remaining).contains(&len));
            position += len as u64;
            remaining -= len;
            segments += 1;
        }
        assert!(
            segments > 64,
            "the fixture was not dense enough: {segments}"
        );
        assert_eq!(position, 8_192);
    }

    /// Swing shifts ONLY the odd on-grid steps, and only when the grid is
    /// a true subdivision: zero and whole beats leave the music alone.
    #[test]
    fn swing_shifts_only_odd_on_grid_steps() {
        // 1/16 grid: 0.0625 beats per step.
        let mut notes = vec![
            test_note(0.0),    // step 0, downbeat — never swings
            test_note(0.25),   // step 4, even — stays
            test_note(0.3125), // step 5, odd — swings
            test_note(0.35),   // between the lines — untouched, on purpose
            test_note(-0.0),   // never negative — see the next assertion
        ];
        swing_note_starts(&mut notes, 0.0625, 0.5);
        assert_eq!(notes[0].start_beats, 0.0, "the downbeat stays");
        assert_eq!(notes[1].start_beats, 0.25, "even steps stay");
        assert!(
            (notes[2].start_beats - 0.328_125).abs() < 1e-9,
            "an odd step moves half a grid toward the next even step: {}",
            notes[2].start_beats
        );
        assert_eq!(notes[3].start_beats, 0.35, "off-grid notes stay");

        // Full swing parks the off-step three quarters of the way to the
        // next even step; zero swing is the exact identity.
        let mut full = vec![test_note(0.0625)];
        swing_note_starts(&mut full, 0.0625, 1.0);
        assert!(
            (full[0].start_beats - 0.093_75).abs() < 1e-9,
            "maximum swing is the classic 75% point"
        );
        let mut straight = vec![test_note(0.0625)];
        swing_note_starts(&mut straight, 0.0625, 0.0);
        assert_eq!(straight[0].start_beats, 0.0625, "zero swing is identity");

        // Whole beats and coarser do not swing — delaying a beat is a
        // different song, not a feel.
        let mut beats = vec![test_note(1.0), test_note(3.0)];
        swing_note_starts(&mut beats, 1.0, 1.0);
        assert_eq!(beats[0].start_beats, 1.0);
        assert_eq!(beats[1].start_beats, 3.0);
    }

    /// The whole-graph pass reaches every pattern-carrying node — and
    /// only those.
    #[test]
    fn apply_swing_walks_every_pattern_node() {
        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![test_note(0.0625)],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: SynthParams::default(),
        });
        let kick = spec.push(NodeSpec::Kick {
            notes: vec![test_note(0.0625)],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: crate::audio::kick::KickParams::default(),
        });
        spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(seq, kick);
        spec.apply_swing(0.0625, 1.0);
        for id in [seq, kick] {
            let notes: Vec<f64> = spec
                .iter_ordered()
                .filter(|(i, _)| *i == id)
                .flat_map(|(_, n)| match n {
                    NodeSpec::Seq { notes, .. } => notes.iter().map(|n| n.start_beats).collect(),
                    NodeSpec::Kick { notes, .. } => notes.iter().map(|n| n.start_beats).collect(),
                    _ => Vec::new(),
                })
                .collect();
            assert_eq!(notes.len(), 1, "the pattern kept its one note");
            assert!(
                (notes[0] - 0.093_75).abs() < 1e-9,
                "the pattern swung: {}",
                notes[0]
            );
        }
    }

    /// Whole-block segment with a rolling transport at 120bpm/48k.
    fn ctx(input: &'static [f32]) -> ProcessCtx<'static> {
        ProcessCtx {
            device_input: input,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position: 0,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
            discontinuity: false,
        }
    }

    fn run(sched: &mut Schedule, out: &mut [f32]) {
        sched.run(out, &ctx(NO_INPUT));
    }

    /// Render consecutive rolling blocks (first one flags the first-play
    /// discontinuity), appending channel 0 of each to `out`.
    fn run_rolling(sched: &mut Schedule, blocks: usize, out: &mut Vec<f32>) {
        let bps = 120.0 / 60.0 / 48_000.0;
        for b in 0..blocks {
            let mut block = vec![0.0f32; 512];
            let c = ProcessCtx {
                device_input: NO_INPUT,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (b * 256) as u64,
                beat: (b * 256) as f64 * bps,
                beats_per_sample: bps,
                discontinuity: b == 0,
            };
            sched.run(&mut block, &c);
            out.extend_from_slice(&block[..256]);
        }
    }

    /// The rule that outranks everything else, checked on the path this
    /// change actually added: modulation is evaluated inside `run`, which
    /// is the audio callback. `assert_no_alloc` ABORTS the process on any
    /// allocation in the block, so this test either passes or takes the
    /// test binary down with it — which is the point.
    ///
    /// The letter doors are exercised in the same block, because
    /// `Schedule::apply` now searches the modulation targets first and
    /// `apply_mod_edit` writes into the plan.
    #[test]
    fn modulated_blocks_do_not_allocate() {
        use crate::audio::modulation::{
            Chain, ModEdit, ModKind, ModShape, ModSpec, Modulator, WireEdit, WireSpec,
        };

        let mut spec = GraphSpec::default();
        let sine = spec.push(NodeSpec::Sine {
            freq: 220.0,
            amp: 0.5,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(sine, mix);
        spec.set_output(mix);
        spec.meter(0, mix);

        // Both source kinds, and a wire carrying every stage of the chain
        // — curve (powf), quantize, and the lag (exp).
        let sources = vec![
            Modulator {
                id: 1,
                kind: ModKind::Lfo {
                    shape: ModShape::Sine,
                    rate_beats: 1.0,
                    free: false,
                    hz: 1.0,
                },
            },
            Modulator {
                id: 2,
                kind: ModKind::Lfo {
                    shape: ModShape::Triangle,
                    rate_beats: 0.5,
                    free: true,
                    hz: 3.0,
                },
            },
            Modulator {
                id: 3,
                kind: ModKind::Follower { track: 0 },
            },
        ];
        let wire = |id: u64, source: u64| WireSpec {
            id,
            source,
            node: mix,
            param: crate::params::mixer::GAIN,
            min: 0.0,
            max: 2.0,
            log: false,
            base: 1.0,
            chain: Chain {
                depth: 0.2,
                curve: 0.4,
                steps: 7,
                smooth_ms: 15.0,
            },
            enabled: true,
            solo: false,
        };
        spec.set_modulation(ModSpec {
            sources,
            wires: vec![wire(10, 1), wire(11, 2), wire(12, 3)],
        });

        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                sched.clear_peaks();
                // A base letter for a MODULATED parameter (diverted into
                // the plan) and one for an unmodulated parameter (straight
                // through to the node) — both doors, every block.
                sched.apply(ParamChange {
                    node: mix.to_bits(),
                    param: crate::params::mixer::GAIN,
                    value: 0.8,
                });
                sched.apply(ParamChange {
                    node: sine.to_bits(),
                    param: crate::params::sine::FREQ,
                    value: 300.0,
                });
                sched.apply_mod_edit(ModEdit::Wire(WireEdit {
                    id: 10,
                    chain: Chain {
                        depth: 0.3,
                        curve: -0.2,
                        steps: 0,
                        smooth_ms: 5.0,
                    },
                    enabled: true,
                    solo: false,
                }));
                sched.apply_mod_edit(ModEdit::Source {
                    id: 2,
                    kind: ModKind::Lfo {
                        shape: ModShape::Square,
                        rate_beats: 2.0,
                        free: true,
                        hz: 1.5,
                    },
                });
                // Two segments per block, as a loop wrap would produce.
                for (offset, len) in [(0usize, 128usize), (128, 128)] {
                    let c = ProcessCtx {
                        device_input: NO_INPUT,
                        in_channels: 2,
                        block_frames: 256,
                        offset,
                        len,
                        playing: true,
                        position: (b * 256 + offset) as u64,
                        beat: (b * 256 + offset) as f64 * bps,
                        beats_per_sample: bps,
                        discontinuity: b == 0 && offset == 0,
                    };
                    sched.run(&mut block, &c);
                }
                let mut sources = [0.0; crate::audio::modulation::MAX_MOD_SOURCES];
                let mut ids = [0; crate::audio::modulation::MAX_MOD_WIRES];
                let mut outs = [0.0; crate::audio::modulation::MAX_MOD_WIRES];
                sched.modulation().source_values(&mut sources);
                sched.modulation().wire_outputs(&mut ids, &mut outs);
            }
        });

        // And it actually modulated: the gain landed somewhere other than
        // the 0.8 base the letters kept setting.
        let peak = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.0, "the graph should still be making sound");
    }

    /// The diversion, end to end through a real schedule: a letter for a
    /// MODULATED parameter must move the base and stay moved, rather than
    /// being overwritten by the next segment's modulation write.
    ///
    /// The unit tests prove `ModPlan` does this; this proves the wiring in
    /// `Schedule::apply` reaches it — the fader going dead under a running
    /// LFO is exactly the bug that would slip past a plan-only test.
    #[test]
    fn a_letter_still_moves_a_modulated_parameter() {
        use crate::audio::modulation::{Chain, ModKind, ModShape, ModSpec, Modulator, WireSpec};

        let build = || {
            let mut spec = GraphSpec::default();
            let sine = spec.push(NodeSpec::Sine {
                freq: 220.0,
                amp: 0.5,
            });
            let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
            spec.connect(sine, mix);
            spec.set_output(mix);
            spec.set_modulation(ModSpec {
                sources: vec![Modulator {
                    id: 1,
                    // Square at a whole-beat rate: the modulation offset
                    // is the same constant every block below, so any
                    // difference between the two runs is the letter's.
                    kind: ModKind::Lfo {
                        shape: ModShape::Square,
                        rate_beats: 4.0,
                        free: false,
                        hz: 1.0,
                    },
                }],
                wires: vec![WireSpec {
                    id: 10,
                    source: 1,
                    node: mix,
                    param: crate::params::mixer::GAIN,
                    min: 0.0,
                    max: 2.0,
                    log: false,
                    base: 1.0,
                    chain: Chain {
                        depth: 0.25,
                        curve: 0.0,
                        steps: 0,
                        smooth_ms: 0.0,
                    },
                    enabled: true,
                    solo: false,
                }],
            });
            (spec.compile(48_000, 256).unwrap(), mix)
        };

        // Render a few blocks so the gain ramp settles, then measure.
        let render = |gain: Option<f32>| {
            let (mut sched, mix) = build();
            if let Some(g) = gain {
                sched.apply(ParamChange {
                    node: mix.to_bits(),
                    param: crate::params::mixer::GAIN,
                    value: g,
                });
            }
            let mut out = Vec::new();
            run_rolling(&mut sched, 6, &mut out);
            out[out.len() - 256..]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };

        let at_base = render(None); // compiled base of 1.0
        let quieter = render(Some(0.4));
        assert!(
            quieter < at_base * 0.8,
            "a letter must still move a modulated parameter: {at_base} -> {quieter}"
        );
        assert!(quieter > 0.0, "and not silence it");
    }

    fn seq_graph(params: SynthParams) -> (Schedule, NodeId) {
        let mut spec = GraphSpec::default();
        let id = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 1.0,
                pitch: 69,
                vel: 127,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: vec![],
            loop_len_beats: None,
            params,
        });
        spec.set_output(id);
        (spec.compile(48_000, 256).unwrap(), id)
    }

    /// A chord must sound as a chord. This is the regression that matters:
    /// "free voice" used to mean `env < ENV_FLOOR`, which is TRUE of the
    /// voice allocated one event earlier at the same sample — so every
    /// note-on stole its predecessor and a triad came out as one note.
    #[test]
    fn simultaneous_notes_get_their_own_voices() {
        let chord = |pitches: &[u8]| -> Vec<f32> {
            let mut spec = GraphSpec::default();
            let id = spec.push(NodeSpec::Seq {
                notes: pitches
                    .iter()
                    .map(|p| Note {
                        start_beats: 0.0,
                        len_beats: 4.0,
                        pitch: *p,
                        vel: 127,
                        plocks: Vec::new(),
                        fx_locks: Vec::new(),
                        prob: 1.0,
                        cond: None,
                    })
                    .collect(),
                subloops: vec![],
                loop_len_beats: None,
                params: SynthParams::default(),
            });
            spec.set_output(id);
            let mut sched = spec.compile(48_000, 256).unwrap();
            let mut out = Vec::new();
            run_rolling(&mut sched, 130, &mut out);
            out
        };

        // A major triad, all three starting on the same sample.
        let one = chord(&[60]);
        let three = chord(&[60, 64, 67]);
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let (p1, p3) = (peak(&one), peak(&three));
        assert!(
            p3 > p1 * 2.0,
            "three voices should be far louder than one: {p1} vs {p3}"
        );

        // And the pitches are really distinct: a single note is periodic at
        // its own frequency, a triad is not. Compare each render with itself
        // shifted by one period of middle C (~366 samples at 48k).
        let self_diff = |s: &[f32], lag: usize| -> f32 {
            s.iter()
                .zip(s[lag..].iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max)
        };
        let lag = (48_000.0 / 261.6255_f32).round() as usize;
        // Measure past the attack so the envelope is not the difference.
        let tail = 12_000..30_000;
        assert!(
            self_diff(&one[tail.clone()], lag) < 0.02,
            "one note repeats every period"
        );
        assert!(
            self_diff(&three[tail], lag) > 0.05,
            "a triad does not — the other voices are really there"
        );
    }

    /// Nine notes on eight voices: someone must lose, and it has to be the
    /// OLDEST note, never the one just pressed.
    #[test]
    fn voice_stealing_takes_the_oldest_not_the_newest() {
        let mut spec = GraphSpec::default();
        // Nine notes, one per beat, each long enough to still be held.
        let notes: Vec<Note> = (0..9)
            .map(|i| Note {
                start_beats: i as f64 * 0.25,
                len_beats: 16.0,
                pitch: 60 + i as u8,
                vel: 127,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let id = spec.push(NodeSpec::Seq {
            notes,
            subloops: vec![],
            loop_len_beats: None,
            params: SynthParams::default(),
        });
        spec.set_output(id);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = Vec::new();
        run_rolling(&mut sched, 200, &mut out);

        // The test module lives inside this file, so it can look straight
        // at the compiled node — the only way to ask which voices are held.
        let Some(Node::Seq { voices, .. }) = sched.nodes.first() else {
            panic!("the graph should hold one Seq");
        };
        let held: Vec<u8> = voices.held().collect();
        assert_eq!(held.len(), SEQ_VOICES, "all eight voices are held");
        assert!(
            !held.contains(&60),
            "the oldest note (60) is the one that was stolen: {held:?}"
        );
        assert!(
            held.contains(&68),
            "the newest note (68) must be sounding: {held:?}"
        );
    }

    /// Build sine -> filter -> output at 48k/256.
    fn filter_graph(freq: f32, spec_node: NodeSpec) -> (Schedule, NodeId) {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine { freq, amp: 0.5 });
        let flt = spec.push(spec_node);
        spec.connect(src, flt);
        spec.set_output(flt);
        (spec.compile(48_000, 256).unwrap(), flt)
    }

    /// Steady-state gain of the filter at one frequency, in dB: run long,
    /// measure the tail of the output against the tail of a filterless run.
    fn filter_gain_db(freq: f32, spec_node: NodeSpec) -> f32 {
        let (mut sched, _) = filter_graph(freq, spec_node);
        let mut wet = Vec::new();
        run_rolling(&mut sched, 40, &mut wet);

        let mut dry_spec = GraphSpec::default();
        let src = dry_spec.push(NodeSpec::Sine { freq, amp: 0.5 });
        dry_spec.set_output(src);
        let mut sched = dry_spec.compile(48_000, 256).unwrap();
        let mut dry = Vec::new();
        run_rolling(&mut sched, 40, &mut dry);

        let tail = wet.len() / 2;
        20.0 * (rms(&wet[tail..]) / rms(&dry[tail..])).log10()
    }

    /// THE test this node exists for: the audio's measured response and the
    /// widget's drawn curve are the same curve. Cutoff accuracy, Butterworth
    /// flatness, slope steepness and the resonant peak all ride on it.
    #[test]
    fn filter_node_tracks_the_display_curve() {
        use crate::params::filter as fp;
        use crate::ui::device::filter as ui;
        // The CHARACTER rides along, because it is the one setting that
        // moves the audio without moving a coefficient: a ladder gives up
        // a decibel of level at Q 8 that a clean filter keeps, and before
        // the two sides shared `resonance_loss` the display drew the
        // clean number for every voicing.
        for (mode, ui_mode, q, character) in [
            (fp::MODE_LP, ui::Mode::Lowpass, 0.707, fp::CHAR_CLEAN),
            (fp::MODE_LP, ui::Mode::Lowpass, 8.0, fp::CHAR_CLEAN),
            (fp::MODE_LP, ui::Mode::Lowpass, 8.0, fp::CHAR_LADDER),
            (fp::MODE_LP, ui::Mode::Lowpass, 8.0, fp::CHAR_DIODE),
            (fp::MODE_HP, ui::Mode::Highpass, 0.707, fp::CHAR_LADDER),
        ] {
            for freq in [250.0, 1_000.0, 4_000.0] {
                let measured = filter_gain_db(
                    freq,
                    NodeSpec::Filter {
                        params: crate::audio::filter::FilterParams {
                            mode: mode as f32,
                            slope: 3.0,
                            cutoff_hz: 1_000.0,
                            res: q,
                            drive: 0.0,
                            character: character as f32,
                            ..Default::default()
                        },
                    },
                );
                let drawn = ui::magnitude_db(
                    &ui::Filter {
                        mode: ui_mode,
                        slope: ui::Slope::Db24,
                        cutoff_hz: 1_000.0,
                        q,
                        drive: 0.0,
                        character,
                    },
                    freq,
                    48_000.0,
                );
                // Ignore what neither side claims to render: below the
                // display floor both are "silent".
                if drawn < -50.0 {
                    assert!(measured < -40.0, "{mode}/{q}/{freq}: {measured}");
                    continue;
                }
                assert!(
                    (measured - drawn).abs() < 1.0,
                    "mode {mode} q {q} character {character} at {freq} Hz: \
                     audio {measured:.2} dB, display {drawn:.2} dB"
                );
            }
        }
    }

    /// The default filter (cutoff parked at 20 kHz, drive 0) is a wire
    /// delayed by exactly the drive stage's constant half-band latency —
    /// loading it changes nothing audible, and the latency is a known
    /// number for PDC to absorb when it exists.
    #[test]
    fn default_filter_is_a_transparent_delayed_wire() {
        let latency = crate::dsp::shaper::Oversampler2x::new().latency();
        let spec_node = NodeSpec::Filter {
            params: crate::audio::filter::FilterParams {
                mode: 0.0,
                slope: 3.0,
                cutoff_hz: 20_000.0,
                res: 0.707,
                drive: 0.0,
                ..Default::default()
            },
        };
        let (mut sched, _) = filter_graph(440.0, spec_node);
        let mut wet = Vec::new();
        run_rolling(&mut sched, 20, &mut wet);

        let mut dry_spec = GraphSpec::default();
        let src = dry_spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        dry_spec.set_output(src);
        let mut sched = dry_spec.compile(48_000, 256).unwrap();
        let mut dry = Vec::new();
        run_rolling(&mut sched, 20, &mut dry);

        // Settle past the first block's amp ramp-in, then compare shifted.
        let start = 1_000;
        let worst = (start..4_000)
            .map(|i| (wet[i + latency] - dry[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 0.01,
            "default filter must be a delayed wire; worst error {worst}"
        );
    }

    /// A seek must not drag a resonant ring into the new position — the
    /// reverb's rule, applied to the filter's state and its smoothers.
    #[test]
    fn a_discontinuity_cuts_the_filter_ring() {
        let (mut sched, id) = filter_graph(
            500.0,
            NodeSpec::Filter {
                params: crate::audio::filter::FilterParams {
                    mode: 0.0,
                    slope: 3.0,
                    cutoff_hz: 500.0,
                    res: 24.0,
                    drive: 0.0,
                    ..Default::default()
                },
            },
        );
        // Charge the resonance.
        let mut out = Vec::new();
        run_rolling(&mut sched, 20, &mut out);
        assert!(rms(&out[3_000..]) > 0.05, "resonant filter must ring");

        // Silence the source, then seek: the first block after the
        // discontinuity starts from cleared state and silent input.
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: 99, // unknown id: binned, a stale letter is harmless
            value: 1.0,
        });
        let mut block = vec![0.0f32; 512];
        let mut c = play_ctx(0.0, true);
        c.position = 480_000;
        sched.run(&mut block, &c);
        // The sine source also resets its own amp ramp on nothing — it
        // keeps playing, so only the FILTER state cut is visible in the
        // first samples: no full-scale resonant carry-over.
        assert!(
            block[..64].iter().all(|s| s.abs() < 0.6),
            "no resonant carry-over through a seek"
        );
        assert!(block.iter().all(|s| s.is_finite()));
    }

    /// Drive is audible as new harmonics — the soft clipper working at 2x —
    /// and never as NaN or a blow-up. Third harmonic of 200 Hz, measured by
    /// correlation, must grow by an order of magnitude against the clean run.
    #[test]
    fn filter_drive_grows_harmonics_and_stays_finite() {
        let third_harmonic_level = |drive: f32| {
            let (mut sched, _) = filter_graph(
                200.0,
                NodeSpec::Filter {
                    params: crate::audio::filter::FilterParams {
                        mode: 0.0,
                        slope: 3.0,
                        cutoff_hz: 20_000.0,
                        res: 0.707,
                        drive,
                        ..Default::default()
                    },
                },
            );
            let mut out = Vec::new();
            run_rolling(&mut sched, 40, &mut out);
            assert!(out.iter().all(|s| s.is_finite()));
            let tail = &out[out.len() / 2..];
            let w = std::f32::consts::TAU * 600.0 / 48_000.0;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in tail.iter().enumerate() {
                re += (*s as f64) * (w * i as f32).cos() as f64;
                im += (*s as f64) * (w * i as f32).sin() as f64;
            }
            ((re * re + im * im).sqrt() / tail.len() as f64) as f32
        };
        let clean = third_harmonic_level(0.0);
        let driven = third_harmonic_level(1.0);
        assert!(
            driven > clean * 10.0 && driven > 1e-3,
            "full drive must grow the third harmonic: clean {clean}, driven {driven}"
        );
    }

    /// Letters through the table: a mode switch retargets the audio (bp at
    /// the corner passes, lp far below cutoff still passes), and hostile
    /// values clamp or bin instead of doing anything interesting.
    #[test]
    fn filter_letters_switch_modes_and_bin_nonsense() {
        let (mut sched, id) = filter_graph(
            1_000.0,
            NodeSpec::Filter {
                params: crate::audio::filter::FilterParams {
                    mode: 0.0,
                    slope: 3.0,
                    cutoff_hz: 1_000.0,
                    res: 4.0,
                    drive: 0.0,
                    ..Default::default()
                },
            },
        );
        let mut out = Vec::new();
        run_rolling(&mut sched, 10, &mut out);

        // Hostile letters first: NaN, an unknown id, an insane cutoff.
        for (param, value) in [
            (crate::params::filter::CUTOFF, f32::NAN),
            (77, 1.0),
            (crate::params::filter::CUTOFF, 1e12),
        ] {
            sched.apply(ParamChange {
                node: id.to_bits(),
                param,
                value,
            });
        }
        // The hostile cutoff clamped to 20 kHz — a legal value. Put the
        // corner back on the tone, then switch to bandpass there.
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: crate::params::filter::CUTOFF,
            value: 1_000.0,
        });
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: crate::params::filter::MODE,
            value: crate::params::filter::MODE_BP as f32,
        });
        let mut bp = Vec::new();
        run_rolling(&mut sched, 20, &mut bp);
        assert!(bp.iter().all(|s| s.is_finite()));
        assert!(
            rms(&bp[bp.len() / 2..]) > 0.05,
            "bandpass at its own corner must pass the tone"
        );
    }

    /// The reverb node, end to end through a compiled graph: dry passes at
    /// mix 0, a tail appears at mix 1, and a seek cuts the room.
    #[test]
    fn reverb_node_wets_dries_and_cuts_on_a_seek() {
        let build = |mix: f32| {
            let mut spec = GraphSpec::default();
            let src = spec.push(NodeSpec::Sine {
                freq: 440.0,
                amp: 0.5,
            });
            let rev = spec.push(NodeSpec::Reverb {
                predelay_ms: 0.0,
                size: 0.9,
                decay: 1.8,
                damp: 5_000.0,
                low_cut: 20.0,
                diffusion: 0.8,
                modulation: 2.0,
                width: 1.0,
                mix,
            });
            spec.connect(src, rev);
            spec.set_output(rev);
            (spec.compile(48_000, 256).unwrap(), rev)
        };

        // Fully dry: the reverb is a wire.
        let (mut sched, _) = build(0.0);
        let mut dry = Vec::new();
        run_rolling(&mut sched, 20, &mut dry);
        assert!(rms(&dry[2_000..]) > 0.05, "dry path must pass audio");

        // Fully wet: silent at first (the room has not answered yet), then
        // ringing — and it keeps ringing after the input stops.
        let (mut sched, id) = build(1.0);
        let mut wet = Vec::new();
        run_rolling(&mut sched, 40, &mut wet);
        assert!(rms(&wet[4_000..]) > 1e-4, "wet path must produce a tail");

        // Cut the source, and the tail continues on its own.
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: 0,
            value: 1.0,
        });
        let mut tail = Vec::new();
        run_rolling(&mut sched, 10, &mut tail);
        assert!(tail.iter().all(|s| s.is_finite()));
    }

    /// A seek must not drag the old room's tail into the new position —
    /// the same all-sound-off rule a sounding voice follows.
    #[test]
    fn a_discontinuity_cuts_the_reverb_tail() {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        let rev = spec.push(NodeSpec::Reverb {
            predelay_ms: 0.0,
            size: 0.95,
            decay: 1.8,
            damp: 0.0,
            low_cut: 20.0,
            diffusion: 0.8,
            modulation: 2.0,
            width: 1.0,
            mix: 1.0,
        });
        spec.connect(src, rev);
        spec.set_output(rev);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Charge the room.
        let mut out = Vec::new();
        run_rolling(&mut sched, 40, &mut out);
        assert!(rms(&out[8_000..]) > 1e-4, "the room should be ringing");

        // Silence the source, then seek. The first block after the seek
        // must be silent: the tail was cut, not faded.
        sched.apply(ParamChange {
            node: src.to_bits(),
            param: 1,
            value: 0.0,
        });
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut block = vec![0.0f32; 512];
        let seek = ProcessCtx {
            device_input: NO_INPUT,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position: 500_000,
            beat: 500_000.0 * bps,
            beats_per_sample: bps,
            discontinuity: true,
        };
        sched.run(&mut block, &seek);
        let peak = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak < 1e-6,
            "a seek must cut the room, not carry it: {peak}"
        );
    }

    #[test]
    fn non_finite_letters_are_binned() {
        // clamp() passes NaN through; apply() must not. A NaN letter leaves
        // the previous target untouched instead of poisoning the ramp.
        let (mut sched, id) = seq_graph(SynthParams::default());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            sched.apply(ParamChange {
                node: id.to_bits(),
                param: 0,
                value: bad,
            });
        }
        let mut out = Vec::new();
        run_rolling(&mut sched, 4, &mut out);
        assert!(out.iter().all(|s| s.is_finite()), "NaN letter poisoned out");
        assert!(rms(&out[768..1024]) > 1e-2, "gain should still be 1.0");
    }

    #[test]
    fn synth_param_rates_reproduce_the_old_defaults() {
        // The defaults must sound like the previously hardcoded voice:
        // attack env += 1000/sr, release env *= 0.9997 (at 48k).
        let a = synth_attack_rate(SynthParams::default().attack_ms, 48_000.0);
        assert!((a - 1_000.0 / 48_000.0).abs() / a < 1e-5);
        let r = synth_release_coeff(SynthParams::default().release_ms, 48_000.0);
        assert!((r - 0.9997).abs() < 1e-5, "release coeff {r}");
    }

    #[test]
    fn seq_release_param_shapes_the_tail() {
        // One note, beats 0..1 (0.5s at 120bpm = off at sample 24000).
        // Compare the tail well after the off with a 5ms vs 500ms release.
        let tail = 30_000..46_000;
        let mut short = Vec::new();
        let (mut sched, _) = seq_graph(SynthParams {
            release_ms: 5.0,
            ..Default::default()
        });
        run_rolling(&mut sched, 188, &mut short);
        let mut long = Vec::new();
        let (mut sched, _) = seq_graph(SynthParams {
            release_ms: 500.0,
            ..Default::default()
        });
        run_rolling(&mut sched, 188, &mut long);

        let (rs, rl) = (rms(&short[tail.clone()]), rms(&long[tail]));
        assert!(rs < 1e-4, "5ms release should be dead in the tail: {rs}");
        assert!(rl > 2e-3, "500ms release should still ring: {rl}");
        assert!(rs < rl / 10.0);
    }

    #[test]
    fn seq_gain_param_ramps_to_silence_without_a_step() {
        // A long held note; drop gain to 0 mid-note. The letter block ramps
        // down instead of cutting, and the block after it is exact silence.
        let mut spec = GraphSpec::default();
        let id = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 8.0,
                pitch: 69,
                vel: 127,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: vec![],
            loop_len_beats: None,
            params: Default::default(),
        });
        spec.set_output(id);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut sound = Vec::new();
        run_rolling(&mut sched, 4, &mut sound);
        assert!(rms(&sound[768..1024]) > 1e-2, "note should be sounding");

        sched.apply(ParamChange {
            node: id.to_bits(),
            param: 0,
            value: 0.0,
        });

        // The ramp block: starts audible, ends inaudible.
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut block = vec![0.0f32; 512];
        let c = ProcessCtx {
            device_input: NO_INPUT,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position: 1024,
            beat: 1024.0 * bps,
            beats_per_sample: bps,
            discontinuity: false,
        };
        sched.run(&mut block, &c);
        let ramp = &block[..256];
        let peak = ramp.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 1e-2, "ramp block should still carry signal: {peak}");
        assert!(
            ramp[255].abs() < 1.5e-3,
            "ramp block should end near silence: {}",
            ramp[255]
        );

        // The block after: gain landed exactly on 0.
        let mut after = vec![9.0f32; 512];
        let c2 = ProcessCtx {
            position: 1280,
            beat: 1280.0 * bps,
            ..c
        };
        sched.run(&mut after, &c2);
        assert!(after[..256].iter().all(|s| s.abs() == 0.0));
    }

    #[test]
    fn empty_graph_outputs_silence() {
        let mut sched = GraphSpec::default().compile(48_000, 256).unwrap();
        let mut out = vec![1.0f32; 512];
        run(&mut sched, &mut out);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn sine_reaches_both_channels_planar() {
        let mut spec = GraphSpec::default();
        let id = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        spec.set_output(id);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);

        let (ch0, ch1) = out.split_at(256);
        assert_eq!(ch0, ch1, "both channels get the same mono source");
        let peak = ch0.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.2 && peak <= 0.5,
            "peak {peak} should be ramping toward amp"
        );
        assert_eq!(ch0[0], 0.0, "sine starts at phase 0 and amp 0");
    }

    #[test]
    fn mixer_sums_two_sources_in_topo_order() {
        // Diamond-ish: two sines wired into one mixer, mixer is output.
        // Insertion order is mixer FIRST — topo sort must still run it last.
        let mut spec = GraphSpec::default();
        let mixer = spec.push(NodeSpec::Mixer { gain: 1.0 });
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.2,
        });
        let b = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.2,
        });
        spec.connect(a, mixer);
        spec.connect(b, mixer);
        spec.set_output(mixer);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out); // ramp-in block (amp and gain both rise)
        run(&mut sched, &mut out); // steady state

        // Two identical in-phase sines at 0.2 through unit gain: peak ~0.4.
        let peak = out[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.35 && peak < 0.45, "summed peak {peak}, want ~0.4");
    }

    #[test]
    fn cycle_is_refused() {
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Mixer { gain: 1.0 });
        let b = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(a, b);
        spec.connect(b, a);
        let err = match spec.compile(48_000, 256) {
            Err(e) => e,
            Ok(_) => panic!("cycle must be refused"),
        };
        assert_eq!(err, CompileError::Cycle);
    }

    #[test]
    fn input_node_copies_device_channel() {
        let mut spec = GraphSpec::default();
        let mic = spec.push(NodeSpec::Input { channel: 1 });
        spec.set_output(mic);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Planar device input: ch0 = 0.25, ch1 = -0.5.
        let input: &'static [f32] = Box::leak(Box::new({
            let mut v = vec![0.25f32; 512];
            v[256..].fill(-0.5);
            v
        }))
        .as_slice();
        let mut out = vec![0.0f32; 512];
        sched.run(&mut out, &ctx(input));
        assert!(
            out[..256].iter().all(|s| *s == -0.5),
            "ch1 should reach output"
        );
    }

    #[test]
    fn stale_id_is_binned_not_misdelivered() {
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.1,
        });
        spec.remove(a);
        // Slot likely reused by the next insert — different generation.
        let b = spec.push(NodeSpec::Sine {
            freq: 550.0,
            amp: 0.1,
        });
        spec.set_output(b);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Letter to the DEAD id must not touch the new occupant.
        sched.apply(ParamChange {
            node: a.to_bits(),
            param: 0,
            value: 9_999.0,
        });
        // Letter to the live id must land.
        sched.apply(ParamChange {
            node: b.to_bits(),
            param: 0,
            value: 660.0,
        });

        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        let expected_cycles = 660.0 * 256.0 / 48_000.0; // ~3.5 cycles
        let zero_crossings = out[..256].windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!(
            (zero_crossings as f32 - expected_cycles * 2.0).abs() <= 2.0,
            "zero crossings {zero_crossings} inconsistent with 660Hz"
        );
    }

    #[test]
    fn removed_then_reused_slot_keeps_ids_distinct() {
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Silence);
        spec.remove(a);
        let b = spec.push(NodeSpec::Silence);
        assert_ne!(a, b, "reused slot must carry a new generation");
        assert_ne!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn removing_a_node_drops_its_wires_and_output_role() {
        let mut spec = GraphSpec::default();
        let s = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.2,
        });
        let m = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(s, m);
        spec.set_output(m);
        spec.remove(m);
        // No dangling wire, no dangling output: compiles to silence.
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![1.0f32; 512];
        run(&mut sched, &mut out);
        assert!(out.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn chain_reuses_slots_regardless_of_length() {
        // a -> m1 -> m2 -> m3. Mixers are stereo (2 slots each); at any
        // moment one producer and one consumer are live: peak is m1(2)+m2(2)
        // = 4 slots, reused down the rest of the chain regardless of length.
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        let m1 = spec.push(NodeSpec::Mixer { gain: 1.0 });
        let m2 = spec.push(NodeSpec::Mixer { gain: 1.0 });
        let m3 = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(a, m1);
        spec.connect(m1, m2);
        spec.connect(m2, m3);
        spec.set_output(m3);
        let sched = spec.compile(48_000, 256).unwrap();
        assert_eq!(sched.arena_slots(), 4, "chain must reuse, not accumulate");
    }

    #[test]
    fn diamond_needs_three_slots() {
        // Both mono sources are live when the stereo mixer runs:
        // 1 + 1 + 2 = 4.
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.2,
        });
        let b = spec.push(NodeSpec::Sine {
            freq: 550.0,
            amp: 0.2,
        });
        let m = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(a, m);
        spec.connect(b, m);
        spec.set_output(m);
        let sched = spec.compile(48_000, 256).unwrap();
        assert_eq!(sched.arena_slots(), 4);
    }

    #[test]
    fn output_slot_survives_later_steps() {
        // The output node runs FIRST in topo order; an unrelated node runs
        // after it. If the output's slot were recycled, the device copy would
        // emit the wrong node's audio.
        let mut spec = GraphSpec::default();
        let out = spec.push(NodeSpec::Sine {
            freq: 100.0,
            amp: 0.5,
        });
        let other = spec.push(NodeSpec::Sine {
            freq: 4_000.0,
            amp: 0.5,
        });
        spec.set_output(out);
        let _ = other; // no wires: both are roots, either topo order possible
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut o = vec![0.0f32; 512];
        run(&mut sched, &mut o);
        run(&mut sched, &mut o);
        // 100Hz over 256 frames at 48k = ~0.53 cycles -> at most 2 zero
        // crossings. 4kHz would give ~42. Distinguishes cleanly.
        let crossings = o[..256].windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!(
            crossings <= 4,
            "{crossings} crossings: output slot was recycled"
        );
    }

    #[test]
    fn double_wire_from_one_node_does_not_double_free() {
        // One sine wired into the same mixer twice (doubling trick). The
        // producer's slot must be freed once, not twice — a double free would
        // hand the same slot to two later nodes.
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.2,
        });
        let m = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(a, m);
        spec.connect(a, m);
        let b = spec.push(NodeSpec::Sine {
            freq: 550.0,
            amp: 0.2,
        });
        let m2 = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(m, m2);
        spec.connect(b, m2);
        spec.set_output(m2);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut o = vec![0.0f32; 512];
        run(&mut sched, &mut o);
        run(&mut sched, &mut o);
        // Doubled 0.2 sine + 0.2 sine of different freq through unit gains:
        // peak must exceed the single-sine 0.2 noticeably (sum of 0.4 + 0.2
        // interfering). Mostly this test exists for the allocator; the assert
        // just proves audio still flows sanely.
        let peak = o[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "peak {peak}: doubling lost");
    }

    #[test]
    fn metronome_never_skips_across_segment_aligned_beats() {
        use crate::audio::transport::{Transport, TransportCmd};
        // 120bpm/48k: a beat every 24_000 samples; every 8th beat boundary
        // lands EXACTLY on a 256-frame block edge (192_000 % 256 == 0) — the
        // alignment where the old stateless floor() comparison coin-flipped
        // and skipped. Drive 32 beats through the real splitter and count.
        let mut spec = GraphSpec::default();
        let c = spec.push(NodeSpec::Click);
        spec.set_output(c);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut tr = Transport::new(48_000.0);
        tr.apply(TransportCmd::Play);

        let mut clicks = 0u32;
        let mut prev = 0.0f32;
        let blocks = 32 * 24_000 / 256; // exactly 32 beats
        let mut out = vec![0.0f32; 512];
        for _ in 0..blocks {
            let mut done = 0;
            while done < 256 {
                let seg = tr.next_segment(256 - done);
                let ctx = ProcessCtx {
                    device_input: NO_INPUT,
                    in_channels: 2,
                    block_frames: 256,
                    offset: done,
                    len: seg.len,
                    playing: seg.playing,
                    position: seg.position,
                    beat: seg.beat,
                    beats_per_sample: tr.map.beats_per_sample(),
                    discontinuity: seg.discontinuity,
                };
                sched.run(&mut out, &ctx);
                done += seg.len;
            }
            // A fresh click is an amplitude jump from near-silence to ~0.6.
            for &s in &out[..256] {
                let a = s.abs();
                if a > 0.5 && prev <= 0.5 {
                    clicks += 1;
                }
                prev = a;
            }
        }
        // Envelope oscillation around the threshold could double-count within
        // one click; the assert is exact because a 1kHz sine's first
        // half-cycle rises once above 0.5 and decay drops it below only after
        // ~35 samples, well past the sine's next dip... but count
        // conservatively: at LEAST one rise per beat, no beat missed.
        assert!(clicks >= 32, "only {clicks} clicks in 32 beats — skipped");
        assert!(clicks <= 40, "{clicks} clicks in 32 beats — double-firing");
    }

    #[test]
    fn metronome_survives_tempo_drop_without_going_silent() {
        // A bpm drop teleports the derived beat BACKWARD (position is fixed,
        // map changes). The counter must resync and click on the next beat,
        // not stay mute until beat catches up to the old high-water mark.
        let mut spec = GraphSpec::default();
        let c = spec.push(NodeSpec::Click);
        spec.set_output(c);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut base = ctx(NO_INPUT);
        base.playing = true;
        base.discontinuity = true;
        let mut out = vec![0.0f32; 512];

        // Play to beat ~8 at 120bpm.
        for b in 0..1500usize {
            let mut c2 = ProcessCtx {
                discontinuity: b == 0,
                beat: b as f64 * 256.0 * base.beats_per_sample,
                ..ctx(NO_INPUT)
            };
            c2.playing = true;
            sched.run(&mut out, &c2);
        }
        // Tempo halves: derived beat at same position is now ~4.
        let bps = 60.0 / 60.0 / 48_000.0; // 60bpm
        let start_beat = 4.0 + 0.3;
        let mut clicked = false;
        for b in 0..400usize {
            let mut c2 = ctx(NO_INPUT);
            c2.playing = true;
            c2.beats_per_sample = bps;
            c2.beat = start_beat + (b * 256) as f64 * bps;
            sched.run(&mut out, &c2);
            if out[..256].iter().any(|s| s.abs() > 0.5) {
                clicked = true;
                break;
            }
        }
        assert!(clicked, "metronome stayed mute after tempo drop");
    }

    /// The hazard, DEMONSTRATED rather than asserted. Two schedules built
    /// from independent specs mint the SAME packed node tag for unrelated
    /// nodes, because every compile starts a fresh `thunderdome` arena and
    /// generations restart with it. Slot and generation cannot tell those
    /// two nodes apart — which is why a letter surviving a schedule swap
    /// used to be delivered to whatever now sat in its slot.
    #[test]
    fn two_compiles_mint_the_same_node_tag_and_only_the_epoch_separates_them() {
        let mut a = GraphSpec::default();
        let first = a.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        a.set_output(first);
        let sched_a = a.compile(48_000, 256).unwrap();

        let mut b = GraphSpec::default();
        let second = b.push(NodeSpec::Sine {
            freq: 880.0,
            amp: 0.5,
        });
        b.set_output(second);
        let sched_b = b.compile(48_000, 256).unwrap();

        assert_eq!(
            first.to_bits(),
            second.to_bits(),
            "unrelated nodes in two compiles wear the SAME tag — the hazard"
        );
        assert_ne!(
            sched_a.epoch(),
            sched_b.epoch(),
            "and the epoch is the only thing that can distinguish them"
        );
    }

    /// Epochs never repeat, so a letter can never be mistaken for one
    /// belonging to a later compile that happens to reuse a number.
    #[test]
    fn every_compile_gets_a_fresh_epoch() {
        let build = || {
            let mut spec = GraphSpec::default();
            let n = spec.push(NodeSpec::Sine {
                freq: 440.0,
                amp: 0.0,
            });
            spec.set_output(n);
            spec.compile(48_000, 256).unwrap().epoch()
        };
        let mut seen = std::collections::HashSet::new();
        for _ in 0..8 {
            assert!(seen.insert(build()), "an epoch was reused");
        }
    }

    fn seq_sched(notes: Vec<Note>) -> Schedule {
        let mut spec = GraphSpec::default();
        let q = spec.push(NodeSpec::Seq {
            notes,
            subloops: Vec::new(),
            loop_len_beats: None,
            params: Default::default(),
        });
        spec.set_output(q);
        spec.compile(48_000, 256).unwrap()
    }

    fn play_ctx(beat: f64, disc: bool) -> ProcessCtx<'static> {
        let mut c = ctx(NO_INPUT);
        c.playing = true;
        c.beat = beat;
        // One-shot arrangements are stamped and played in the absolute
        // sample domain. Keep this synthetic context internally coherent:
        // a test seek to beat 100 must not still claim it is at sample zero.
        c.position = (beat / c.beats_per_sample).round().max(0.0) as u64;
        c.discontinuity = disc;
        c
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn subloop_unrolls_region_and_shifts_tail() {
        // Notes at beats 0, 1, 2, 3. Subloop [1, 2) x3.
        // Expect: 0 | 1, 2, 3 (three plays of the region note) | 2->4, 3->5.
        let notes: Vec<Note> = (0..4)
            .map(|i| Note {
                start_beats: i as f64,
                len_beats: 0.5,
                pitch: 60 + i,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let subs = [SubLoop {
            start_beats: 1.0,
            end_beats: 2.0,
            repeats: 3,
        }];
        let out = expand_subloops(&notes, &subs).unwrap();
        let starts: Vec<(f64, u8)> = out.iter().map(|n| (n.start_beats, n.pitch)).collect();
        assert_eq!(
            starts,
            vec![
                (0.0, 60),
                (1.0, 61),
                (2.0, 61),
                (3.0, 61),
                (4.0, 62),
                (5.0, 63)
            ]
        );
        assert_eq!(expanded_len_beats(4.0, &subs), 6.0);
    }

    #[test]
    fn subloop_note_sustaining_in_from_before_plays_once() {
        // A note starting before the region and ringing into it is timeline
        // material, not region material: one copy, unshifted.
        let notes = [Note {
            start_beats: 0.5,
            len_beats: 2.0,
            pitch: 60,
            vel: 100,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }];
        let subs = [SubLoop {
            start_beats: 1.0,
            end_beats: 2.0,
            repeats: 4,
        }];
        let out = expand_subloops(&notes, &subs).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_beats, 0.5);
    }

    #[test]
    fn bad_subloops_are_refused() {
        let n = [Note {
            start_beats: 0.0,
            len_beats: 1.0,
            pitch: 60,
            vel: 100,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }];
        for subs in [
            vec![SubLoop {
                start_beats: 2.0,
                end_beats: 2.0,
                repeats: 2,
            }], // empty
            vec![SubLoop {
                start_beats: 3.0,
                end_beats: 2.0,
                repeats: 2,
            }], // inverted
            vec![SubLoop {
                start_beats: 0.0,
                end_beats: 1.0,
                repeats: 0,
            }], // zero plays
            vec![
                SubLoop {
                    start_beats: 0.0,
                    end_beats: 2.0,
                    repeats: 2,
                },
                SubLoop {
                    start_beats: 1.0,
                    end_beats: 3.0,
                    repeats: 2,
                }, // overlap
            ],
        ] {
            assert_eq!(
                expand_subloops(&n, &subs).unwrap_err(),
                CompileError::BadSubLoop
            );
        }
    }

    #[test]
    fn two_subloops_accumulate_shifts() {
        // Loops [0,1)x2 and [2,3)x2 on notes at 0,1,2,3:
        // 0 -> {0, 1}; 1 -> 2; 2 -> {3, 4}; 3 -> 5. Total 6 beats.
        let notes: Vec<Note> = (0..4)
            .map(|i| Note {
                start_beats: i as f64,
                len_beats: 0.5,
                pitch: 60 + i,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let subs = [
            SubLoop {
                start_beats: 0.0,
                end_beats: 1.0,
                repeats: 2,
            },
            SubLoop {
                start_beats: 2.0,
                end_beats: 3.0,
                repeats: 2,
            },
        ];
        let out = expand_subloops(&notes, &subs).unwrap();
        let starts: Vec<f64> = out.iter().map(|n| n.start_beats).collect();
        assert_eq!(starts, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(expanded_len_beats(4.0, &subs), 6.0);
    }

    fn clap_state(id: &str, latency: u32) -> crate::clap_host::ClapDeviceState {
        crate::clap_host::ClapDeviceState {
            key: crate::clap_host::PluginKey {
                library_path: std::path::PathBuf::from("/plugins/test.clap"),
                plugin_id: crate::clap_host::PluginId::new(id).unwrap(),
            },
            display_name: "Test CLAP".to_owned(),
            state: None,
            reported_latency_samples: latency,
        }
    }

    #[test]
    fn an_unbound_live_clap_device_is_refused_not_faked() {
        let mut spec = GraphSpec::default();
        let node = spec.push(NodeSpec::Clap {
            device: crate::clap_host::PluginDeviceModel::Clap {
                plugin: clap_state("org.example.unbound", 0),
            },
        });
        spec.set_output(node);
        assert!(matches!(
            spec.compile(48_000, 64),
            Err(CompileError::MissingClapRuntime { plugin_id })
                if plugin_id == "org.example.unbound"
        ));
    }

    #[test]
    fn a_missing_clap_device_is_a_latency_preserving_dry_path() {
        const FRAMES: usize = 64;
        const LATENCY: usize = 7;
        let mut spec = GraphSpec::default();
        let left = spec.push(NodeSpec::Input { channel: 0 });
        let missing = spec.push_missing_clap(
            clap_state("org.example.missing", LATENCY as u32),
            "not installed",
        );
        let right = spec.push(NodeSpec::Input { channel: 1 });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(left, missing);
        spec.connect(missing, mix);
        spec.connect(right, mix);
        spec.set_output(mix);

        let mut schedule = spec.compile(48_000, FRAMES).unwrap();
        assert_eq!(schedule.latency(), LATENCY);
        let silence = [0.0f32; FRAMES * 2];
        let mut output = [0.0f32; FRAMES * 2];
        schedule.run(
            &mut output,
            &ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: FRAMES,
                offset: 0,
                len: FRAMES,
                playing: true,
                position: 0,
                beat: 0.0,
                beats_per_sample: 120.0 / 60.0 / 48_000.0,
                discontinuity: true,
            },
        );

        // ProcessCtx input is planar: channel 0 begins at 0 and channel 1
        // begins at FRAMES. Unequal impulses prove that both graph legs reach
        // the compensated sum; accidentally reading channel 0 twice would
        // produce 2.0 instead of 1.25.
        let mut input = [0.0f32; FRAMES * 2];
        input[0] = 1.0;
        input[FRAMES] = 0.25;
        assert_no_alloc::assert_no_alloc(|| {
            schedule.run(
                &mut output,
                &ProcessCtx {
                    device_input: &input,
                    in_channels: 2,
                    block_frames: FRAMES,
                    offset: 0,
                    len: FRAMES,
                    playing: true,
                    position: FRAMES as u64,
                    beat: FRAMES as f64 * 120.0 / 60.0 / 48_000.0,
                    beats_per_sample: 120.0 / 60.0 / 48_000.0,
                    discontinuity: false,
                },
            );
        });
        assert!(output[..LATENCY].iter().all(|sample| *sample == 0.0));
        assert!(
            output[FRAMES..FRAMES + LATENCY]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(output[LATENCY], 1.25);
        assert_eq!(output[FRAMES + LATENCY], 1.25);
    }

    #[test]
    fn compensation_preserves_runtime_and_every_named_side_table() {
        let Ok(executable) = std::env::current_exe() else {
            panic!("the current test executable must have a path");
        };
        // SAFETY: this test never loads the executable as a plugin. It only
        // needs a canonical trusted-path value to exercise identity remapping.
        let Ok(trusted) = (unsafe { crate::clap_host::TrustedPluginPath::from_path(&executable) })
        else {
            panic!("the current test executable path must canonicalize");
        };
        let Ok(plugin_id) = crate::clap_host::PluginId::new("org.example.remap") else {
            panic!("the fixture plugin id must be valid");
        };
        let mut plugin = clap_state("org.example.remap", 11);
        plugin.key.library_path = trusted.path().to_path_buf();
        let factory = crate::clap_host::ClapNodeFactory::new(trusted, plugin_id);

        let mut spec = GraphSpec::default();
        let source = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.1,
        });
        let clap = spec.push_clap(plugin, factory).unwrap();
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(source, clap);
        spec.connect(clap, mix);
        spec.connect(source, mix);
        spec.set_output(mix);
        spec.meter(1, clap);
        spec.tap(2, clap);
        spec.telemetry(3, clap);
        spec.lock_base(clap, 99, 0.75);

        let Some(aligned) = spec.compensate(48_000) else {
            panic!("the CLAP leg must need compensation");
        };
        let Some(mapped) = aligned
            .iter_ordered()
            .find_map(|(id, node)| matches!(node, NodeSpec::Clap { .. }).then_some(id))
        else {
            panic!("the CLAP node must survive compensation");
        };
        assert_eq!(aligned.clap_factories.len(), 1);
        assert!(aligned.clap_factories.contains_key(&mapped));
        assert!(aligned.meters.contains(&(1, mapped)));
        assert!(aligned.taps.contains(&(2, mapped)));
        assert!(aligned.telemetry.contains(&(3, mapped)));
        assert!(aligned.fx_bases.contains(&(mapped, 99, 0.75)));
    }

    #[test]
    fn console_latency_formulas_match_the_real_cores_and_schedule() {
        for kind in [
            crate::console::SectionKind::Preamp,
            crate::console::SectionKind::Cut,
            crate::console::SectionKind::Drive,
            crate::console::SectionKind::Iron,
            crate::console::SectionKind::Door,
            crate::console::SectionKind::Ceiling,
            crate::console::SectionKind::Spectra,
        ] {
            let params = crate::console::SectionParams::of(kind);
            let core = crate::audio::console::core_of(&params, 48_000.0, 64);
            let expected = core.latency();
            assert!(expected > 0, "{kind:?} must declare its real delay");

            let mut spec = GraphSpec::default();
            let section = spec.push(NodeSpec::Section { params });
            spec.set_output(section);
            assert_eq!(
                spec.compile(48_000, 64).unwrap().latency(),
                expected,
                "{kind:?} graph latency drifted from its core"
            );
        }
    }

    /// Two paths into one mixer, one of them through a latency-bearing
    /// node, must arrive together.
    ///
    /// Without compensation the clean path is EARLY by the filter's
    /// half-band round trip, so a transient that should cancel does not —
    /// which is the audible form of "the tracks slid against each other".
    #[test]
    fn compensation_aligns_two_paths_into_one_mixer() {
        let latency = crate::dsp::shaper::Oversampler2x::new().latency();
        assert!(latency > 0, "the test needs a node that is actually late");

        // Same source into both legs; one leg is filtered. Invert the
        // filtered leg's contribution by summing them and looking for
        // cancellation is fragile with a filter in the path, so instead
        // measure WHEN each leg's impulse arrives, one leg at a time.
        let arrival_of = |filtered: bool| -> usize {
            let mut spec = GraphSpec::default();
            let src = spec.push(NodeSpec::Sine {
                freq: 1_000.0,
                amp: 1.0,
            });
            let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
            let clean = spec.push(NodeSpec::Mixer { gain: 1.0 });
            spec.connect(src, clean);
            spec.connect(clean, mix);
            if filtered {
                let f = spec.push(NodeSpec::Filter {
                    params: crate::audio::filter::FilterParams {
                        mode: 0.0,
                        slope: 0.0,
                        cutoff_hz: 20_000.0,
                        res: 0.707,
                        drive: 0.0,
                        ..Default::default()
                    },
                });
                spec.connect(src, f);
                spec.connect(f, mix);
            }
            spec.set_output(mix);
            let sched = spec.compile(48_000, 256).unwrap();
            sched.latency()
        };

        // One leg alone: no compensation needed, but the filtered graph
        // reports the filter's latency as the graph's own.
        assert_eq!(arrival_of(false), 0, "a clean graph is not late");
        assert_eq!(
            arrival_of(true),
            latency,
            "a graph containing a late node reports that latency"
        );
    }

    /// THE LIMITER'S LATENCY IS REAL, REPORTED AND COMPENSATED.
    ///
    /// The device that made the compensation machinery worth having: it
    /// is the only node whose delay is not an oversampler's, and the
    /// figure it states is a lookahead it genuinely holds. A limiter on
    /// one track and not another is exactly the case plugin delay
    /// compensation exists for, so this walks the whole way round —
    /// what the device says, what the graph reports, and what an
    /// unlimited sibling path is delayed by to match.
    #[test]
    fn a_limiter_reports_its_lookahead_and_the_graph_absorbs_it() {
        let latency = crate::audio::limiter::latency();
        // The lookahead is a real part of it, not just the oversamplers —
        // otherwise this device is telling the graph the filter's story.
        assert!(
            latency > 2 * crate::dsp::shaper::Oversampler2x::new().latency(),
            "the reported latency has no lookahead in it: {latency}"
        );

        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 1.0,
        });
        let lim = spec.push(NodeSpec::Limiter {
            params: crate::audio::limiter::LimiterParams::default(),
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(src, lim);
        spec.connect(lim, mix);
        spec.connect(src, mix); // the early leg
        spec.set_output(mix);

        let Some(aligned) = spec.compensate(48_000) else {
            panic!("a graph with a limiter in one leg needs compensating");
        };
        let delays: Vec<usize> = aligned
            .iter_ordered()
            .filter_map(|(_, node)| match node {
                NodeSpec::Delay { samples, .. } => Some(*samples),
                _ => None,
            })
            .collect();
        assert_eq!(
            delays,
            vec![latency],
            "the early leg was not delayed to match the limiter"
        );

        // And the compiled graph owns up to the total.
        let sched = spec.compile(48_000, 256).unwrap();
        assert_eq!(sched.latency(), latency);

        // A graph whose ONLY path runs through the limiter needs no
        // splice — there is no sibling to align with — but still reports.
        let mut alone = GraphSpec::default();
        let src = alone.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 1.0,
        });
        let lim = alone.push(NodeSpec::Limiter {
            params: crate::audio::limiter::LimiterParams::default(),
        });
        alone.connect(src, lim);
        alone.set_output(lim);
        assert_eq!(alone.compile(48_000, 256).unwrap().latency(), latency);
    }

    /// The compensation itself: the clean leg gets a delay node spliced in,
    /// so both legs reach the mixer at the same sample.
    #[test]
    fn compensation_splices_a_delay_onto_the_early_path() {
        let latency = crate::dsp::shaper::Oversampler2x::new().latency();
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 1.0,
        });
        let f = spec.push(NodeSpec::Filter {
            params: crate::audio::filter::FilterParams {
                mode: 0.0,
                slope: 0.0,
                cutoff_hz: 20_000.0,
                res: 0.707,
                drive: 0.0,
                ..Default::default()
            },
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(src, f);
        spec.connect(f, mix);
        spec.connect(src, mix); // the early leg
        spec.set_output(mix);

        let Some(aligned) = spec.compensate(48_000) else {
            panic!("this graph needs compensating");
        };
        let delays: Vec<usize> = aligned
            .iter_ordered()
            .filter_map(|(_, s)| match s {
                NodeSpec::Delay { samples, .. } => Some(*samples),
                _ => None,
            })
            .collect();
        assert_eq!(
            delays,
            vec![latency],
            "exactly one delay, exactly as long as the filter is late"
        );

        // And it still compiles and runs.
        let mut sched = aligned.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run_rolling(&mut sched, 8, &mut out);
        assert_eq!(sched.latency(), latency);
    }

    /// A graph with nothing late must not pay for compensation at all —
    /// no copy, no delay nodes, no extra steps.
    #[test]
    fn a_graph_with_no_latency_is_left_alone() {
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(a, mix);
        spec.set_output(mix);
        assert!(spec.compensate(48_000).is_none());
        assert_eq!(spec.compile(48_000, 256).unwrap().latency(), 0);
    }

    /// A single chain through a late node needs no delay — there is no
    /// other path to disagree with — but its latency still reports.
    #[test]
    fn a_lone_chain_reports_latency_without_splicing() {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.5,
        });
        let f = spec.push(NodeSpec::Filter {
            params: crate::audio::filter::FilterParams {
                mode: 0.0,
                slope: 0.0,
                cutoff_hz: 8_000.0,
                res: 0.707,
                drive: 0.0,
                ..Default::default()
            },
        });
        spec.connect(src, f);
        spec.set_output(f);
        assert!(
            spec.compensate(48_000).is_none(),
            "one path cannot be out of step with itself"
        );
        assert_eq!(
            spec.compile(48_000, 256).unwrap().latency(),
            crate::dsp::shaper::Oversampler2x::new().latency()
        );
    }

    /// The delay node is a WIRE: an impulse comes back bit-identical, the
    /// stated number of samples later.
    #[test]
    fn a_delay_node_is_an_exact_wire() {
        const D: usize = 37;
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 1.0,
        });
        let d = spec.push(NodeSpec::Delay {
            samples: D,
            channels: 1,
        });
        spec.connect(src, d);
        spec.set_output(d);
        let mut delayed = spec.compile(48_000, 256).unwrap();

        let mut plain = GraphSpec::default();
        let p = plain.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 1.0,
        });
        plain.set_output(p);
        let mut undelayed = plain.compile(48_000, 256).unwrap();

        let mut a = Vec::new();
        let mut b = Vec::new();
        run_rolling(&mut delayed, 4, &mut a);
        run_rolling(&mut undelayed, 4, &mut b);
        // Compare well past the ramp-in so both signals are steady.
        let (x, y) = (&a[300..600], &b[300 - D..600 - D]);
        assert_eq!(x, y, "a delay node must reproduce its input exactly");
    }

    /// The compensated graph runs in the red zone without allocating —
    /// including the discontinuity path, which clears both rings.
    ///
    /// `assert_no_alloc` ABORTS the process on any allocation, so this
    /// either passes or takes the test binary down with it.
    #[test]
    fn a_compensated_graph_does_not_allocate() {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.8,
        });
        let f = spec.push(NodeSpec::Filter {
            params: crate::audio::filter::FilterParams {
                mode: 0.0,
                slope: 0.0,
                cutoff_hz: 8_000.0,
                res: 0.707,
                drive: 0.4,
                ..Default::default()
            },
        });
        let pan = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(src, f);
        spec.connect(f, mix);
        // The early legs: one mono, one that widens to stereo, so both a
        // mono and a stereo delay get spliced and exercised.
        spec.connect(src, mix);
        spec.connect(src, pan);
        spec.connect(pan, mix);
        spec.set_output(mix);

        let mut sched = spec.compile(48_000, 256).unwrap();
        assert!(sched.latency() > 0, "the graph should be compensated");
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                sched.clear_peaks();
                let c = ProcessCtx {
                    device_input: NO_INPUT,
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (b * 256) as u64,
                    beat: (b * 256) as f64 * bps,
                    beats_per_sample: bps,
                    // Every other block seeks, so the ring-clearing path
                    // runs inside the guard too.
                    discontinuity: b % 2 == 0,
                };
                sched.run(&mut block, &c);
            }
        });
    }

    // ------------------------------------------------------------ sat ---

    /// Build sine -> sat -> output at 48k/256, and the matching dry
    /// reference. Returns both renders, the sat one shifted back by the
    /// node's constant latency so sample `i` of each is the same instant.
    fn sat_render(freq: f32, amp: f32, spec_node: NodeSpec, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let latency = crate::dsp::shaper::Oversampler2x::new().latency();

        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine { freq, amp });
        let sat = spec.push(spec_node);
        spec.connect(src, sat);
        spec.set_output(sat);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut wet = Vec::new();
        run_rolling(&mut sched, blocks, &mut wet);

        let mut dry_spec = GraphSpec::default();
        let src = dry_spec.push(NodeSpec::Sine { freq, amp });
        dry_spec.set_output(src);
        let mut sched = dry_spec.compile(48_000, 256).unwrap();
        let mut dry = Vec::new();
        run_rolling(&mut sched, blocks, &mut dry);

        (wet.split_off(latency), dry)
    }

    /// A settled window, past the sine's amp ramp-in and the half-band's
    /// fill, and short of the end so the latency shift cannot run off it.
    const SAT_WINDOW: std::ops::Range<usize> = 2_000..4_000;

    /// THE test this node exists for, and the filter's test ported: the
    /// audio runs the curve the widget draws.
    ///
    /// Compared as RMS rather than sample by sample, because these curves
    /// are not bandlimited — hard clip at drive 8 asks for harmonics past
    /// Nyquist, and what comes back is the oversampler's best answer, not
    /// the algebraic one. A sample-wise assert would be measuring Gibbs
    /// ringing at the corners rather than agreement. RMS over a settled
    /// window is blind to that and still moves hard the moment the node
    /// picks a different shape, reads drive in different units, or blends
    /// the wrong way — which are the mistakes this wiring can actually
    /// make.
    #[test]
    fn sat_node_runs_the_curve_the_widget_draws() {
        use crate::params::sat as sp;
        use crate::ui::device::shaper as ui;

        for (mode, ui_mode) in [
            (sp::MODE_HARD, ui::Mode::HardClip),
            (sp::MODE_SOFT, ui::Mode::SoftClip),
            (sp::MODE_CUBIC, ui::Mode::Cubic),
            (sp::MODE_FOLD, ui::Mode::Fold),
            (sp::MODE_CRUSH, ui::Mode::Crush),
        ] {
            for (drive, bias, mix) in [
                (1.0, 0.0, 1.0),
                (4.0, 0.0, 1.0),
                (12.0, 0.25, 1.0),
                (8.0, 0.0, 0.35),
            ] {
                let (wet, dry) = sat_render(
                    500.0,
                    0.5,
                    NodeSpec::Sat {
                        mode,
                        drive,
                        bias,
                        mix,
                        out: 1.0,
                    },
                    24,
                );
                let drawn = ui::Shaper {
                    mode: ui_mode,
                    drive,
                    bias,
                    mix,
                };
                // The widget's curve applied to the same dry signal is
                // the reference. Its DC component is removed the way the
                // node removes it — a biased curve has an offset, and
                // the node's blocker takes it out.
                let mut want: Vec<f32> = dry[SAT_WINDOW].iter().map(|x| drawn.shape(*x)).collect();
                let mean = want.iter().sum::<f32>() / want.len() as f32;
                for s in want.iter_mut() {
                    *s -= mean;
                }
                let got = rms(&wet[SAT_WINDOW]);
                let want = rms(&want);
                let error = (got - want).abs() / want.max(1e-6);
                assert!(
                    error < 0.05,
                    "mode {mode} drive {drive} bias {bias} mix {mix}: \
                     audio {got:.4} rms, display {want:.4} rms ({:.1}% off)",
                    error * 100.0
                );
            }
        }
    }

    /// The identity case: hard clip at unity drive passes everything
    /// inside the rails, so the whole device collapses to a wire delayed
    /// by exactly the figure `spec_latency` reports. This is what holds
    /// the oversampler round trip to unity gain and linear phase — if the
    /// half-band drifted, or the decimation phase flipped, every other
    /// test here would still pass and this one would not.
    #[test]
    fn a_transparent_saturator_is_a_delayed_wire() {
        let (wet, dry) = sat_render(
            440.0,
            0.5,
            NodeSpec::Sat {
                mode: crate::params::sat::MODE_HARD,
                drive: 1.0,
                bias: 0.0,
                mix: 1.0,
                out: 1.0,
            },
            24,
        );
        let worst = SAT_WINDOW
            .map(|i| (wet[i] - dry[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 0.01,
            "an untouched hard clip must be a delayed wire; worst error {worst}"
        );
    }

    /// The device is STEREO, and the two channels are independent: a
    /// shared half-band history or a shared DC blocker would leak one
    /// side into the other, and nothing else here would notice — every
    /// other test drives both channels with the same signal.
    ///
    /// The lane-independence test from the poly kernels, in its
    /// two-channel form, for exactly the same reason.
    #[test]
    fn sat_channels_do_not_leak_into_each_other() {
        let mut spec = GraphSpec::default();
        // Pan hard left: a stereo source with silence on one side.
        let src = spec.push(NodeSpec::Sine {
            freq: 300.0,
            amp: 0.8,
        });
        let pan = spec.push(NodeSpec::Pan {
            pan: -1.0,
            gain: 1.0,
        });
        let sat = spec.push(NodeSpec::Sat {
            mode: crate::params::sat::MODE_FOLD,
            drive: 16.0,
            // NO bias, and that is the test's subject matter, not an
            // oversight: bias offsets the CURVE, so a biased shaper turns
            // a silent channel into a constant one — `shape(0)` is the
            // bias itself. That is the parameter working, not the
            // channels leaking, and a test that cannot tell the two apart
            // is not testing independence.
            bias: 0.0,
            mix: 1.0,
            out: 1.0,
        });
        spec.connect(src, pan);
        spec.connect(pan, sat);
        spec.set_output(sat);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let bps = 120.0 / 60.0 / 48_000.0;
        let mut worst_right = 0.0f32;
        let mut peak_left = 0.0f32;
        for b in 0..24 {
            let mut block = vec![0.0f32; 512];
            let c = ProcessCtx {
                device_input: NO_INPUT,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (b * 256) as u64,
                beat: (b * 256) as f64 * bps,
                beats_per_sample: bps,
                discontinuity: b == 0,
            };
            sched.run(&mut block, &c);
            for s in &block[..256] {
                peak_left = peak_left.max(s.abs());
            }
            for s in &block[256..] {
                worst_right = worst_right.max(s.abs());
            }
        }
        assert!(peak_left > 0.1, "the driven side must actually sound");
        assert!(
            worst_right < 1e-6,
            "the silent channel must stay silent; leaked {worst_right}"
        );
    }

    /// A seek must not drag the half-band's history into the new
    /// position — and the contract is stronger than "goes quiet", which
    /// is why this does not test for quiet.
    ///
    /// A node that has just cut is INDISTINGUISHABLE from one that was
    /// compiled a moment ago: same cleared rings, same snapped smoothers.
    /// So charge one saturator for twenty blocks, seek it, and compare
    /// that block against the FIRST block of a freshly compiled twin fed
    /// the identical input. Any history that survived the cut shows up as
    /// a difference, and no other assertion has to guess how loud a
    /// leftover tail would be.
    ///
    /// The source is `Input` rather than `Sine` precisely so the two runs
    /// can be handed the same samples: a sine node carries a phase, and
    /// phase is state a discontinuity does not reset (correctly — it is
    /// free-running), which would show up here as a difference that is
    /// not a bug.
    #[test]
    fn a_discontinuity_cuts_the_saturator() {
        /// One loud block of planar stereo input, both channels alike.
        fn input_block() -> Vec<f32> {
            let mut buf = vec![0.0f32; 512];
            for i in 0..256 {
                let s = (i as f32 * 0.13).sin() * 0.9;
                buf[i] = s;
                buf[256 + i] = s;
            }
            buf
        }

        fn saturator_graph() -> Schedule {
            let mut spec = GraphSpec::default();
            let src = spec.push(NodeSpec::Input { channel: 0 });
            let sat = spec.push(NodeSpec::Sat {
                mode: crate::params::sat::MODE_HARD,
                drive: 24.0,
                bias: 0.0,
                mix: 1.0,
                out: 1.0,
            });
            spec.connect(src, sat);
            spec.set_output(sat);
            spec.compile(48_000, 256).unwrap()
        }

        let signal = input_block();
        let bps = 120.0 / 60.0 / 48_000.0;
        let ctx_at = |position: u64, discontinuity: bool| ProcessCtx {
            device_input: &signal,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position,
            beat: position as f64 * bps,
            beats_per_sample: bps,
            discontinuity,
        };

        // The charged one: twenty blocks of history, then a seek.
        let mut charged = saturator_graph();
        let mut block = vec![0.0f32; 512];
        for b in 0..20 {
            charged.run(&mut block, &ctx_at(b * 256, b == 0));
        }
        assert!(
            block[..256].iter().any(|s| s.abs() > 0.1),
            "a driven clipper must sound"
        );
        charged.run(&mut block, &ctx_at(96_000, true));

        // The fresh one, at the same place, seeing the same samples.
        let mut fresh = saturator_graph();
        let mut reference = vec![0.0f32; 512];
        fresh.run(&mut reference, &ctx_at(96_000, true));

        let worst = block
            .iter()
            .zip(reference.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-6,
            "a seek must leave the saturator as new; worst difference {worst}"
        );
    }

    /// The red-zone contract, under the guard: both the steady path and
    /// the discontinuity path, in stereo, with every control moving —
    /// a moving control is what walks the chunk loop, and the chunk loop
    /// is where a `Vec` would be easiest to reach for.
    #[test]
    fn sat_run_does_not_allocate() {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.8,
        });
        let sat = spec.push(NodeSpec::Sat {
            mode: crate::params::sat::MODE_SOFT,
            drive: 6.0,
            bias: 0.0,
            mix: 1.0,
            out: 1.0,
        });
        spec.connect(src, sat);
        spec.set_output(sat);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;
        let node = sat.to_bits();

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                use crate::params::sat as sp;
                // Letters land mid-run, including the mode switch.
                for (param, value) in [
                    (sp::MODE, (b % 5) as f32),
                    (sp::DRIVE, 1.0 + b as f32),
                    (sp::BIAS, 0.1 * b as f32),
                    (sp::MIX, 0.1 * b as f32),
                    (sp::OUT, 0.5 + 0.1 * b as f32),
                ] {
                    sched.apply(ParamChange { node, param, value });
                }
                let c = ProcessCtx {
                    device_input: NO_INPUT,
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (b * 256) as u64,
                    beat: (b * 256) as f64 * bps,
                    beats_per_sample: bps,
                    // Every other block seeks, so the cut path runs
                    // inside the guard too.
                    discontinuity: b % 2 == 0,
                };
                sched.run(&mut block, &c);
            }
        });
    }

    /// A block whose length is not a multiple of `SAT_CHUNK`, and a
    /// zero-length one: the chunk walk must handle the ragged tail and
    /// the empty case without an index off the end. The kernel
    /// contract's edge-lengths test, at the node.
    #[test]
    fn sat_survives_ragged_and_empty_segments() {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Sine {
            freq: 700.0,
            amp: 0.7,
        });
        let sat = spec.push(NodeSpec::Sat {
            mode: crate::params::sat::MODE_FOLD,
            drive: 9.0,
            bias: 0.2,
            mix: 0.8,
            out: 1.2,
        });
        spec.connect(src, sat);
        spec.set_output(sat);
        let mut sched = spec.compile(48_000, 256).unwrap();

        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;
        // 0 and 1 are the degenerate cases; 17 and 251 are lengths no
        // chunk size divides.
        for (i, len) in [0usize, 1, 17, 251, 256].into_iter().enumerate() {
            let c = ProcessCtx {
                device_input: NO_INPUT,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len,
                playing: true,
                position: (i * 256) as u64,
                beat: (i * 256) as f64 * bps,
                beats_per_sample: bps,
                discontinuity: i == 0,
            };
            sched.run(&mut block, &c);
        }
        assert!(
            block.iter().all(|s| s.is_finite()),
            "a ragged segment must not produce NaN"
        );
    }
    // ----------------------------------------------------------- echo ---

    /// One loud block of planar stereo input, both channels alike.
    fn echo_input() -> Vec<f32> {
        let mut buf = vec![0.0f32; 512];
        // A single click, so a repeat is a thing you can find by index.
        buf[0] = 1.0;
        buf[256] = 1.0;
        buf
    }

    fn echo_graph(spec_node: NodeSpec) -> Schedule {
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Input { channel: 0 });
        let echo = spec.push(spec_node);
        spec.connect(src, echo);
        spec.set_output(echo);
        spec.compile(48_000, 256).unwrap()
    }

    /// Render `blocks` blocks, feeding the click only in the first, and
    /// return channel 0 concatenated.
    fn echo_render(spec_node: NodeSpec, blocks: usize, bpm: f64) -> Vec<f32> {
        let mut sched = echo_graph(spec_node);
        let signal = echo_input();
        let silence = vec![0.0f32; 512];
        let bps = bpm / 60.0 / 48_000.0;
        let mut out = Vec::new();
        for b in 0..blocks {
            let mut block = vec![0.0f32; 512];
            let c = ProcessCtx {
                device_input: if b == 0 { &signal } else { &silence },
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: (b * 256) as u64,
                beat: (b * 256) as f64 * bps,
                beats_per_sample: bps,
                discontinuity: b == 0,
            };
            sched.run(&mut block, &c);
            out.extend_from_slice(&block[..256]);
        }
        out
    }

    /// Where the loudest repeat after the dry click lands, in samples.
    ///
    /// Panics if there is no repeat to find. Deliberately: `max_by` over
    /// a silent buffer returns its LAST index, which is a plausible
    /// sample number and a completely wrong answer — the first draft of
    /// these tests rendered a window shorter than the echo it was
    /// measuring and got "10239" twice, which reads as a repeat in the
    /// wrong place rather than as no repeat at all.
    fn first_repeat_at(out: &[f32], skip: usize) -> usize {
        let (at, peak) = out
            .iter()
            .enumerate()
            .skip(skip)
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, s)| (i, s.abs()))
            .unwrap_or((0, 0.0));
        assert!(
            peak > 0.05,
            "no repeat in {} samples — the window is shorter than the echo",
            out.len()
        );
        at
    }

    /// Long enough to contain any echo these tests ask for: a quarter
    /// note at the slowest tempo used here is two thirds of a second.
    const ECHO_BLOCKS: usize = 200;

    /// THE test this node exists for: a SYNCED echo repeats on the
    /// division, at the tempo the transport is actually playing.
    ///
    /// Checked at two tempos, because a delay that merely lands on the
    /// right sample at 120 BPM might be reading a constant. The whole
    /// point of deriving the time from `ctx.beats_per_sample` every
    /// segment is that the answer moves when the tempo does.
    #[test]
    fn a_synced_echo_repeats_on_the_division() {
        use crate::params::echo as ep;
        for (bpm, sync, beats) in [
            (120.0f64, 3u32, 1.0f64), // 1/4 at 120 = 0.5 s
            (120.0, 4, 0.5),          // 1/8 at 120 = 0.25 s
            (90.0, 3, 1.0),           // 1/4 at 90  = 0.667 s
        ] {
            let out = echo_render(
                NodeSpec::Echo {
                    sync,
                    time_ms: 350.0,
                    feedback: 0.0, // one repeat, nothing to confuse it
                    tone_hz: 20_000.0,
                    drive: 0.0,
                    wow: 0.0, // a still tape, so the peak is where it is
                    spread: 0.0,
                    mix: 100.0,
                    send: 0.0,
                },
                ECHO_BLOCKS,
                bpm,
            );
            let want = (beats * 60.0 / bpm * 48_000.0) as usize;
            // Skip past the dry click and the glide settling.
            let got = first_repeat_at(&out, 64);
            let err = got.abs_diff(want);
            assert!(
                err < 400,
                "{sync} at {bpm} BPM: repeat at {got}, expected {want} \
                 ({} names it {})",
                ep::SYNC_NAMES[sync as usize],
                want
            );
        }
    }

    /// A FREE echo repeats at the millisecond it was given, and ignores
    /// the tempo entirely.
    #[test]
    fn a_free_echo_repeats_at_its_millisecond() {
        for bpm in [120.0f64, 200.0] {
            let out = echo_render(
                NodeSpec::Echo {
                    sync: 0,
                    time_ms: 250.0,
                    feedback: 0.0,
                    tone_hz: 20_000.0,
                    drive: 0.0,
                    wow: 0.0,
                    spread: 0.0,
                    mix: 100.0,
                    send: 0.0,
                },
                ECHO_BLOCKS,
                bpm,
            );
            let want = (0.250 * 48_000.0) as usize;
            let got = first_repeat_at(&out, 64);
            assert!(
                got.abs_diff(want) < 400,
                "free echo at {bpm} BPM repeated at {got}, expected {want}"
            );
        }
    }

    /// Spread walks the right channel's echo later than the left's, and
    /// zero leaves them together — the mono-compatible case.
    #[test]
    fn spread_separates_the_two_channels() {
        let render = |spread: f32| {
            let mut sched = echo_graph(NodeSpec::Echo {
                sync: 0,
                time_ms: 100.0,
                feedback: 0.0,
                tone_hz: 20_000.0,
                drive: 0.0,
                wow: 0.0,
                spread,
                mix: 100.0,
                send: 0.0,
            });
            let signal = echo_input();
            let silence = vec![0.0f32; 512];
            let bps = 120.0 / 60.0 / 48_000.0;
            let (mut l, mut r) = (Vec::new(), Vec::new());
            for b in 0..ECHO_BLOCKS {
                let mut block = vec![0.0f32; 512];
                let c = ProcessCtx {
                    device_input: if b == 0 { &signal } else { &silence },
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (b * 256) as u64,
                    beat: (b * 256) as f64 * bps,
                    beats_per_sample: bps,
                    discontinuity: b == 0,
                };
                sched.run(&mut block, &c);
                l.extend_from_slice(&block[..256]);
                r.extend_from_slice(&block[256..]);
            }
            (first_repeat_at(&l, 64), first_repeat_at(&r, 64))
        };
        let (l0, r0) = render(0.0);
        assert_eq!(l0, r0, "at spread 0 the channels must be together");
        let (l1, r1) = render(50.0);
        assert!(
            r1 > l1 + 1_000,
            "spread 50% should put the right channel well behind: {l1} vs {r1}"
        );
    }

    /// A seek must not ring the old position's repeats into the new one:
    /// a cut echo is indistinguishable from a freshly compiled one.
    #[test]
    fn a_discontinuity_cuts_the_echo() {
        let spec_node = || NodeSpec::Echo {
            sync: 0,
            time_ms: 200.0,
            feedback: 80.0,
            tone_hz: 12_000.0,
            drive: 30.0,
            wow: 0.0,
            spread: 0.0,
            mix: 100.0,
            send: 0.0,
        };
        let signal = echo_input();
        let bps = 120.0 / 60.0 / 48_000.0;
        let ctx_at = |input: &'static [f32], position: u64, discontinuity: bool| ProcessCtx {
            device_input: input,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position,
            beat: position as f64 * bps,
            beats_per_sample: bps,
            discontinuity,
        };
        let leaked: &'static [f32] = Box::leak(signal.into_boxed_slice());

        let mut charged = echo_graph(spec_node());
        let mut block = vec![0.0f32; 512];
        for b in 0..20 {
            charged.run(&mut block, &ctx_at(leaked, b * 256, b == 0));
        }
        charged.run(&mut block, &ctx_at(leaked, 96_000, true));

        let mut fresh = echo_graph(spec_node());
        let mut reference = vec![0.0f32; 512];
        fresh.run(&mut reference, &ctx_at(leaked, 96_000, true));

        let worst = block
            .iter()
            .zip(reference.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-6,
            "a seek must leave the echo as new; worst difference {worst}"
        );
    }

    /// The red-zone contract under the guard, with every control moving
    /// and the transport seeking — including the ring-clearing path.
    #[test]
    fn echo_run_does_not_allocate() {
        // An AUX, so the send's own per-sample loop runs inside the guard
        // below: an insert never reads it and would leave it untested.
        let mut sched = echo_graph(NodeSpec::Echo {
            sync: 4,
            time_ms: 300.0,
            feedback: 60.0,
            tone_hz: 6_000.0,
            drive: 20.0,
            wow: 30.0,
            spread: 25.0,
            mix: 50.0,
            send: 40.0,
        });
        let node = {
            // The echo is the second node pushed; its id is what the
            // graph handed back, so rebuild the spec to learn it.
            let mut spec = GraphSpec::default();
            let src = spec.push(NodeSpec::Input { channel: 0 });
            let echo = spec.push(NodeSpec::Echo {
                sync: 4,
                time_ms: 300.0,
                feedback: 60.0,
                tone_hz: 6_000.0,
                drive: 20.0,
                wow: 30.0,
                spread: 25.0,
                mix: 50.0,
                send: 0.0,
            });
            spec.connect(src, echo);
            echo.to_bits()
        };
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                use crate::params::echo as ep;
                for (param, value) in [
                    (ep::SYNC, (b % 5) as f32),
                    (ep::TIME, 100.0 + b as f32 * 50.0),
                    (ep::FEEDBACK, 10.0 * b as f32),
                    (ep::TONE, 1_000.0 + b as f32 * 500.0),
                    (ep::DRIVE, 10.0 * b as f32),
                    (ep::WOW, 10.0 * b as f32),
                    (ep::SPREAD, 5.0 * b as f32),
                    (ep::MIX, 10.0 * b as f32),
                    (ep::SEND, 10.0 * b as f32),
                ] {
                    sched.apply(ParamChange { node, param, value });
                }
                let c = ProcessCtx {
                    device_input: NO_INPUT,
                    in_channels: 2,
                    block_frames: 256,
                    offset: 0,
                    len: 256,
                    playing: true,
                    position: (b * 256) as u64,
                    beat: (b * 256) as f64 * bps,
                    beats_per_sample: bps,
                    discontinuity: b % 2 == 0,
                };
                sched.run(&mut block, &c);
            }
        });
    }

    /// The send is the level the tap is fed at — on an AUX. On an insert
    /// the whole track is the input by definition, so nothing may scale
    /// it, and a letter aimed at the send must be ignored rather than
    /// obeyed: automation that sweeps a send to zero has to silence an
    /// aux without ever being able to mute a delay in the signal path.
    #[test]
    fn a_send_scales_an_aux_and_cannot_touch_an_insert() {
        let echo = |send: f32| NodeSpec::Echo {
            sync: 0,
            time_ms: 200.0,
            feedback: 0.0, // one repeat, nothing recirculating to confuse it
            tone_hz: 20_000.0,
            drive: 0.0,
            wow: 0.0,
            spread: 0.0,
            mix: 100.0, // pure wet, which is what a return carries
            send,
        };
        let peak = |out: Vec<f32>| out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let full = peak(echo_render(echo(100.0), ECHO_BLOCKS, 120.0));
        let half = peak(echo_render(echo(50.0), ECHO_BLOCKS, 120.0));
        let quiet = peak(echo_render(echo(1.0), ECHO_BLOCKS, 120.0));
        assert!(full > 0.1, "a send at full must pass the tap: {full}");
        assert!(
            (half / full - 0.5).abs() < 0.01,
            "half the send is half the level: {half} against {full}"
        );
        assert!(quiet < full * 0.02, "a send at 1% is nearly nothing");

        // An INSERT, lettered to zero: it must not budge.
        let letter = |send: NodeSpec, value: f32| {
            let mut spec = GraphSpec::default();
            let src = spec.push(NodeSpec::Input { channel: 0 });
            let node = spec.push(send);
            spec.connect(src, node);
            spec.set_output(node);
            let mut sched = spec.compile(48_000, 256).unwrap();
            sched.apply(ParamChange {
                node: node.to_bits(),
                param: crate::params::echo::SEND,
                value,
            });
            let signal = echo_input();
            let silence = vec![0.0f32; 512];
            let bps = 120.0 / 60.0 / 48_000.0;
            let mut out = Vec::new();
            for b in 0..ECHO_BLOCKS {
                let mut block = vec![0.0f32; 512];
                sched.run(
                    &mut block,
                    &ProcessCtx {
                        device_input: if b == 0 { &signal } else { &silence },
                        in_channels: 2,
                        block_frames: 256,
                        offset: 0,
                        len: 256,
                        playing: true,
                        position: (b * 256) as u64,
                        beat: (b * 256) as f64 * bps,
                        beats_per_sample: bps,
                        discontinuity: b == 0,
                    },
                );
                out.extend_from_slice(&block[..256]);
            }
            out
        };
        let insert = peak(letter(echo(0.0), 0.0));
        assert!(
            insert > 0.1,
            "a send letter must not mute an insert: {insert}"
        );
        // And the same letter DOES move an aux.
        let aux = peak(letter(echo(100.0), 0.0));
        assert!(
            aux < insert * 0.02,
            "the same letter silences an aux: {aux}"
        );
    }

    /// The compressor reaches the graph as a node like any other: it
    /// compiles, it takes letters at the ids its table declares, and a
    /// discontinuity opens it up rather than carrying the old
    /// position's gain reduction across the seek.
    #[test]
    fn glue_runs_in_a_schedule_and_a_seek_opens_it() {
        use crate::params::glue as gp;

        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Input { channel: 0 });
        let glue = spec.push(NodeSpec::Glue {
            params: crate::audio::glue::GlueParams {
                threshold_db: -40.0,
                attack: 0.0,
                release: 0.0,
                ..Default::default()
            },
        });
        spec.connect(src, glue);
        spec.set_output(glue);
        let node = glue.to_bits();
        let mut sched = spec.compile(48_000, 256).unwrap();

        // A loud input, run until the compressor has grabbed.
        let loud: Vec<f32> = (0..512).map(|i| (i as f32 * 0.2).sin() * 0.9).collect();
        let bps = 120.0 / 60.0 / 48_000.0;
        let ctx = |b: usize, discontinuity: bool| ProcessCtx {
            device_input: &loud,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position: (b * 256) as u64,
            beat: (b * 256) as f64 * bps,
            beats_per_sample: bps,
            discontinuity,
        };
        let mut block = vec![0.0f32; 512];
        for b in 0..40 {
            sched.run(&mut block, &ctx(b, b == 0));
        }
        let squashed = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            squashed < 0.8,
            "the compressor should have grabbed by now: peak {squashed:.3}"
        );

        // A letter reaches it: threshold out of the way opens it up.
        sched.apply(ParamChange {
            node,
            param: gp::THRESHOLD,
            value: 10.0,
        });
        for b in 40..80 {
            sched.run(&mut block, &ctx(b, false));
        }
        let opened = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            opened > squashed,
            "a threshold letter never landed: {opened:.3} against {squashed:.3}"
        );

        // A stale id is binned rather than guessed at.
        sched.apply(ParamChange {
            node,
            param: 9_999,
            value: 1.0,
        });
        sched.run(&mut block, &ctx(80, false));
        assert!(block.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn glue_run_does_not_allocate() {
        use crate::params::glue as gp;
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Input { channel: 0 });
        let glue = spec.push(NodeSpec::Glue {
            params: Default::default(),
        });
        spec.connect(src, glue);
        spec.set_output(glue);
        let node = glue.to_bits();
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                for (param, value) in [
                    (gp::THRESHOLD, -30.0 + b as f32),
                    (gp::RATIO, (b % 3) as f32),
                    (gp::ATTACK, (b % 7) as f32),
                    (gp::RELEASE, (b % 7) as f32),
                    (gp::MAKEUP, b as f32),
                    (gp::DRY_WET, (b * 10) as f32),
                    (gp::RANGE, (b * 5) as f32),
                    (gp::CLIP, (b % 2) as f32),
                    (gp::SC_HP, 20.0 + b as f32 * 50.0),
                ] {
                    sched.apply(ParamChange { node, param, value });
                }
                sched.run(
                    &mut block,
                    &ProcessCtx {
                        device_input: NO_INPUT,
                        in_channels: 2,
                        block_frames: 256,
                        offset: 0,
                        len: 256,
                        playing: true,
                        position: (b * 256) as u64,
                        beat: (b * 256) as f64 * bps,
                        beats_per_sample: bps,
                        discontinuity: b % 2 == 0,
                    },
                );
            }
        });
    }

    /// The utility reaches the graph as a node like any other: it
    /// compiles, it takes letters at the ids its table declares, and a
    /// stale id is binned rather than guessed at.
    ///
    /// The property being watched is the one the device exists for —
    /// inserted at its defaults it is a WIRE, so what comes out of the
    /// schedule is what the input node put in.
    #[test]
    fn utility_runs_in_a_schedule_and_passes_its_input_through() {
        use crate::params::utility as up;

        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Input { channel: 0 });
        let util = spec.push(NodeSpec::Utility {
            params: Default::default(),
        });
        spec.connect(src, util);
        spec.set_output(util);
        let node = util.to_bits();
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Interleaved stereo device input, as `Input` reads it.
        let input: Vec<f32> = (0..512).map(|i| (i as f32 * 0.2).sin() * 0.7).collect();
        let bps = 120.0 / 60.0 / 48_000.0;
        let ctx = |b: usize, discontinuity: bool| ProcessCtx {
            device_input: &input,
            in_channels: 2,
            block_frames: 256,
            offset: 0,
            len: 256,
            playing: true,
            position: (b * 256) as u64,
            beat: (b * 256) as f64 * bps,
            beats_per_sample: bps,
            discontinuity,
        };
        let mut block = vec![0.0f32; 512];
        sched.run(&mut block, &ctx(0, true));
        let passed = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(passed > 0.5, "the input never arrived: peak {passed:.3}");

        // A letter reaches it: the trim at its floor takes the level
        // down, which no other row on this table could be mistaken for.
        sched.apply(ParamChange {
            node,
            param: up::GAIN,
            value: up::GAIN_MIN_DB,
        });
        for b in 1..8 {
            sched.run(&mut block, &ctx(b, false));
        }
        let trimmed = block[..256].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            trimmed < passed * 0.1,
            "a gain letter never landed: {trimmed:.4} against {passed:.3}"
        );

        // A stale id is binned rather than guessed at.
        sched.apply(ParamChange {
            node,
            param: 9_999,
            value: 1.0,
        });
        sched.run(&mut block, &ctx(8, false));
        assert!(block.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn utility_run_does_not_allocate() {
        use crate::params::utility as up;
        let mut spec = GraphSpec::default();
        let src = spec.push(NodeSpec::Input { channel: 0 });
        let util = spec.push(NodeSpec::Utility {
            params: Default::default(),
        });
        spec.connect(src, util);
        spec.set_output(util);
        let node = util.to_bits();
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;

        assert_no_alloc::assert_no_alloc(|| {
            for b in 0..8 {
                for (param, value) in [
                    (up::GAIN, -12.0 + b as f32),
                    (up::PAN, (b as f32 / 8.0) * 2.0 - 1.0),
                    (up::WIDTH, (b % 3) as f32),
                    // Every corner INCLUDING the floor, so the segment
                    // that switches the crossover out is inside the
                    // no-alloc window as well as the ones that rebuild it.
                    (up::MONO_HZ, 20.0 + (b % 4) as f32 * 120.0),
                    (up::PHASE, (b % 4) as f32),
                    (up::CHANNEL, (b % 4) as f32),
                    (up::DC, (b % 2) as f32),
                ] {
                    sched.apply(ParamChange { node, param, value });
                }
                sched.run(
                    &mut block,
                    &ProcessCtx {
                        device_input: NO_INPUT,
                        in_channels: 2,
                        block_frames: 256,
                        offset: 0,
                        len: 256,
                        playing: true,
                        position: (b * 256) as u64,
                        beat: (b * 256) as f64 * bps,
                        beats_per_sample: bps,
                        discontinuity: b % 2 == 0,
                    },
                );
            }
        });
    }

    /// THE FADES DO WHAT THEIR NAME SAYS: silence at the clip's edges,
    /// full level in the middle, and a straight line between.
    ///
    /// Stated over the ENVELOPE rather than over a rendered clip, because
    /// a rendered clip needs a file on disk and this is the arithmetic
    /// that can be wrong — an inverted ramp fades in at the end, which is
    /// audible immediately and invisible in a waveform drawn from the
    /// same wrong number.
    #[test]
    fn a_fade_is_silent_at_the_edge_and_whole_in_the_middle() {
        // The node's own expression, lifted out so the test can state it.
        let envelope = |pos: u64, fade_in: u64, fade_out: u64, span: u64| -> f32 {
            let mut level = 1.0f32;
            if fade_in > 0 && pos < fade_in {
                level = pos as f32 / fade_in as f32;
            }
            if fade_out > 0 {
                let left = span.saturating_sub(pos);
                if left <= fade_out {
                    level = level.min(left as f32 / fade_out as f32);
                }
            }
            level
        };
        let span = 1_000u64;

        // A fade IN: silent at the very first frame, whole once past it.
        assert_eq!(envelope(0, 100, 0, span), 0.0, "the first frame is silent");
        assert!((envelope(50, 100, 0, span) - 0.5).abs() < 1e-6, "halfway");
        assert_eq!(envelope(100, 100, 0, span), 1.0, "and whole at the end");
        assert_eq!(envelope(500, 100, 0, span), 1.0, "and stays whole");

        // A fade OUT is its mirror, measured from the clip's end.
        assert_eq!(
            envelope(span, 0, 100, span),
            0.0,
            "the last frame is silent"
        );
        assert!((envelope(span - 50, 0, 100, span) - 0.5).abs() < 1e-6);
        assert_eq!(envelope(span - 100, 0, 100, span), 1.0);
        assert_eq!(envelope(0, 0, 100, span), 1.0, "and the start is untouched");

        // No fades is a wire, at every position.
        for pos in [0u64, 1, 500, 999, 1_000] {
            assert_eq!(envelope(pos, 0, 0, span), 1.0);
        }

        // OVERLAPPING fades take the QUIETER of the two rather than
        // multiplying — a clip shorter than its own fades should dip in
        // the middle, not vanish.
        let both = envelope(500, 800, 800, span);
        assert!(both > 0.0, "a clip covered by both fades still sounds");
        assert!(both < 1.0, "but never reaches full level");
        assert_eq!(envelope(0, 800, 800, span), 0.0, "and its edges are silent");
        assert_eq!(envelope(span, 800, 800, span), 0.0);
    }

    /// Ragged and empty segments: the block walk must handle a length no
    /// chunk divides, and a zero-length one, without an index off the end.
    #[test]
    fn echo_survives_ragged_and_empty_segments() {
        let mut sched = echo_graph(NodeSpec::Echo {
            sync: 0,
            time_ms: 120.0,
            feedback: 50.0,
            tone_hz: 8_000.0,
            drive: 40.0,
            wow: 50.0,
            spread: 30.0,
            mix: 60.0,
            send: 0.0,
        });
        let mut block = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;
        for (i, len) in [0usize, 1, 17, 251, 256].into_iter().enumerate() {
            let c = ProcessCtx {
                device_input: NO_INPUT,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len,
                playing: true,
                position: (i * 256) as u64,
                beat: (i * 256) as f64 * bps,
                beats_per_sample: bps,
                discontinuity: i == 0,
            };
            sched.run(&mut block, &c);
        }
        assert!(
            block.iter().all(|s| s.is_finite()),
            "a ragged segment must not produce NaN"
        );
    }

    /// What a sequencer block costs. Printed, not asserted — a number to
    /// argue with, following the `dsp::noise` cost-report idiom.
    ///
    /// The interesting comparison is a DENSE pattern against a sparse one.
    /// The old consumer did per-sample float work regardless of how many
    /// notes existed, so both cost the same; the run-based walk should make
    /// the sparse case cheaper, because event handling is per event rather
    /// than per sample.
    #[test]
    fn report_seq_cost_per_sample() {
        use std::time::Instant;
        const REPS: usize = 4_000;

        let row = |name: &str, notes: Vec<Note>, loop_len: Option<f64>| {
            let mut spec = GraphSpec::default();
            let q = spec.push(NodeSpec::Seq {
                notes,
                subloops: Vec::new(),
                loop_len_beats: loop_len,
                params: Default::default(),
            });
            spec.set_output(q);
            let Ok(mut sched) = spec.compile(48_000, 256) else {
                return;
            };
            let mut out = vec![0.0f32; 512];
            let bps = 120.0 / 60.0 / 48_000.0;
            let mut beat = 0.0;
            let step = |sched: &mut Schedule, out: &mut Vec<f32>, beat: &mut f64| {
                sched.run(out, &play_ctx(*beat, false));
                *beat += 256.0 * bps;
                // Without this the optimizer can delete the whole loop —
                // the noise bench reported 0.00 ns/sample before it learned
                // this the hard way.
                std::hint::black_box(out);
            };
            for _ in 0..200 {
                step(&mut sched, &mut out, &mut beat);
            }
            let t = Instant::now();
            for _ in 0..REPS {
                step(&mut sched, &mut out, &mut beat);
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * 256) as f64;
            println!("{name:<24} {ns:6.2} ns/sample");
        };

        row("silent (no notes)", Vec::new(), None);
        row(
            "sparse (1 note/beat)",
            (0..16)
                .map(|i| Note {
                    start_beats: f64::from(i),
                    len_beats: 0.5,
                    pitch: 60 + (i % 12) as u8,
                    vel: 100,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                })
                .collect(),
            None,
        );
        row(
            "dense (16ths, 8 voices)",
            (0..256)
                .map(|i| Note {
                    start_beats: f64::from(i) * 0.25,
                    len_beats: 2.0, // overlapping: keeps all 8 voices busy
                    pitch: 48 + (i % 24) as u8,
                    vel: 100,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                })
                .collect(),
            None,
        );
        row(
            "clip loop (1 bar)",
            (0..4)
                .map(|i| Note {
                    start_beats: f64::from(i),
                    len_beats: 0.5,
                    pitch: 60,
                    vel: 100,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                })
                .collect(),
            Some(4.0),
        );
    }

    /// A clip whose cycle length is an exact multiple of the block size.
    ///
    /// The wrap is detected while walking a segment, so a boundary landing
    /// exactly on a segment END was skipped by the "done, stop walking"
    /// exit — the pattern never rewound, and any note-off clamped to the
    /// clip end never fired. One bar at 120bpm/48kHz is 96000 samples and
    /// the default block is 256: 375 blocks exactly. Ordinary clip lengths
    /// hit this, and the existing clip tests all used one beat (93.75
    /// blocks), which never aligns.
    #[test]
    fn a_clip_wrapping_on_a_block_edge_still_repeats() {
        // Four beats, a note on each: cycle length 96000 samples. The
        // last note runs a full beat so its off is CLAMPED to the clip
        // end and lands exactly on the wrap — that off is the hanging-note
        // half of the bug, and a shorter note would never test it.
        let notes: Vec<Note> = (0..4)
            .map(|i| Note {
                start_beats: f64::from(i),
                len_beats: if i == 3 { 1.0 } else { 0.5 },
                pitch: 60,
                vel: 110,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let mut spec = GraphSpec::default();
        let q = spec.push(NodeSpec::Seq {
            notes,
            subloops: Vec::new(),
            loop_len_beats: Some(4.0),
            params: Default::default(),
        });
        spec.set_output(q);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Drive from the TimeMap, not an accumulated float: the aligned
        // case only appears when the beat is derived exactly, which is
        // what bounce and the live callback both do.
        let map = crate::audio::transport::TimeMap {
            bpm: 120.0,
            sample_rate: 48_000.0,
        };
        let level_of_cycle = |sched: &mut Schedule, cycle: u64| -> f32 {
            let mut peak = 0.0f32;
            let mut out = vec![0.0f32; 512];
            // Blocks 0..375 of this cycle.
            for b in 0..375u64 {
                let pos = cycle * 96_000 + b * 256;
                let mut c = play_ctx(map.samples_to_beats(pos), false);
                c.discontinuity = cycle == 0 && b == 0;
                sched.run(&mut out, &c);
                peak = peak.max(out[..256].iter().fold(0.0f32, |m, s| m.max(s.abs())));
            }
            peak
        };

        let first = level_of_cycle(&mut sched, 0);
        assert!(first > 0.05, "the first cycle should sound ({first})");
        let second = level_of_cycle(&mut sched, 1);
        assert!(
            second > 0.05,
            "a clip must keep repeating across an aligned wrap \
             (first {first}, second {second})"
        );
        let third = level_of_cycle(&mut sched, 2);
        assert!(third > 0.05, "and keep going ({third})");
    }

    /// Contract rule 1, made observable: a note starts at an exact SAMPLE,
    /// and moving its start by one sample moves the onset by one sample.
    ///
    /// The old beat-stamped consumer compared an f64 phase per sample, so
    /// onsets landed wherever the accumulated float said; this asserts the
    /// integer stamp instead. A note's first sample is silent by
    /// construction (phase starts at 0, and sin(0) is 0), so the onset is
    /// read as the first sample that moves.
    #[test]
    fn a_note_starts_on_its_exact_sample() {
        const SPB: f64 = 24_000.0; // samples per beat at 120bpm / 48kHz

        let onset_for = |start_sample: u64| -> usize {
            let mut sched = seq_sched(vec![Note {
                start_beats: start_sample as f64 / SPB,
                len_beats: 1.0,
                pitch: 69,
                vel: 127,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }]);
            let mut out = vec![0.0f32; 512];
            sched.run(&mut out, &play_ctx(0.0, true));
            out[..256]
                .iter()
                .position(|s| s.abs() > 0.0)
                .unwrap_or(usize::MAX)
        };

        let a = onset_for(100);
        assert!(
            (100..=102).contains(&a),
            "a note stamped at sample 100 should start there, not at {a}"
        );

        // The real claim: sample resolution, not block or beat resolution.
        let b = onset_for(110);
        assert_eq!(
            b - a,
            10,
            "moving the note 10 samples must move the onset 10 samples \
             ({a} -> {b})"
        );

        // And everything before the onset is exactly silent, not merely
        // quiet — nothing may leak from before a note begins.
        let mut sched = seq_sched(vec![Note {
            start_beats: 100.0 / SPB,
            len_beats: 1.0,
            pitch: 69,
            vel: 127,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }]);
        let mut out = vec![0.0f32; 512];
        sched.run(&mut out, &play_ctx(0.0, true));
        assert!(
            out[..100].iter().all(|s| *s == 0.0),
            "silence before a note must be exact"
        );
    }

    /// Several events landing on ONE sample all fire there, in compiled
    /// order. The run-based walk emits zero-length runs for this, and a
    /// walk that skipped them would drop every event after the first.
    #[test]
    fn stacked_events_on_one_sample_all_fire() {
        // A chord: three notes at the same beat, so six events share two
        // sample stamps.
        let notes: Vec<Note> = [60, 64, 67]
            .into_iter()
            .map(|pitch| Note {
                start_beats: 0.25,
                len_beats: 0.5,
                pitch,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let mut sched = seq_sched(notes);
        let mut out = Vec::new();
        run_rolling(&mut sched, 40, &mut out);

        let Some(Node::Seq { voices, .. }) = sched.nodes.first() else {
            panic!("the graph should hold one Seq");
        };
        let mut held: Vec<u8> = voices.held().collect();
        held.sort_unstable();
        assert_eq!(
            held,
            vec![60, 64, 67],
            "every note of a chord stamped at one sample must sound"
        );
    }

    #[test]
    fn seq_note_sounds_only_inside_its_beats() {
        // One quarter note at beat 1, 120bpm: sounds in [1.0, 2.0) beats.
        let mut sched = seq_sched(vec![Note {
            start_beats: 1.0,
            len_beats: 1.0,
            pitch: 69,
            vel: 100,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }]);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut level_at = |beat: f64, disc: bool, sched: &mut Schedule| {
            sched.run(&mut out, &play_ctx(beat, disc));
            rms(&out[..256])
        };
        assert!(
            level_at(0.5, true, &mut sched) < 1e-4,
            "silent before the note"
        );
        // Walk continuously from 0.9 into the note (no discontinuity).
        let mut b = 0.9;
        let mut peak = 0.0f32;
        while b < 1.5 {
            peak = peak.max(level_at(b, false, &mut sched));
            b += 256.0 * bps;
        }
        assert!(peak > 0.05, "note must sound inside its span (peak {peak})");
        // Well after the note + release: silence again.
        let mut b2 = 2.5;
        let mut tail = 1.0f32;
        while b2 < 4.0 {
            tail = level_at(b2, false, &mut sched);
            b2 += 256.0 * bps;
        }
        assert!(tail < 1e-3, "note must have released (tail {tail})");
    }

    #[test]
    fn same_pitch_back_to_back_notes_both_sound() {
        // Note ends exactly where the next same-pitch note begins. The off
        // must not kill the new note: detect TWO attack transients.
        let mut sched = seq_sched(vec![
            Note {
                start_beats: 0.0,
                len_beats: 1.0,
                pitch: 60,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            },
            Note {
                start_beats: 1.0,
                len_beats: 1.0,
                pitch: 60,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            },
        ]);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut b = 0.0;
        let mut levels: Vec<f32> = Vec::new();
        while b < 2.4 {
            sched.run(&mut out, &play_ctx(b, b == 0.0));
            levels.push(rms(&out[..256]));
            b += 256.0 * bps;
        }
        // The second note's span must carry sound, not the silence that
        // follows a swallowed note-on.
        let one_beat_blocks = (1.0 / (256.0 * bps)) as usize;
        let mid_second = levels[one_beat_blocks + one_beat_blocks / 2];
        assert!(
            mid_second > 0.05,
            "second same-pitch note was swallowed ({mid_second})"
        );
    }

    #[test]
    fn discontinuity_cuts_sounding_notes() {
        // A long note is sounding; a wrap/seek arrives. All-sound-off means
        // the very next block is at most release-tail quiet, and by a few
        // blocks later, silent — no hanging note.
        let mut sched = seq_sched(vec![Note {
            start_beats: 0.0,
            len_beats: 16.0,
            pitch: 57,
            vel: 127,
            plocks: Vec::new(),
            fx_locks: Vec::new(),
            prob: 1.0,
            cond: None,
        }]);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        // Sound it for a while.
        let mut b = 0.0;
        for i in 0..40 {
            sched.run(&mut out, &play_ctx(b, i == 0));
            b += 256.0 * bps;
        }
        assert!(rms(&out[..256]) > 0.05, "note should be sounding");
        // Seek far past the note WITH the discontinuity flag.
        sched.run(&mut out, &play_ctx(100.0, true));
        assert!(rms(&out[..256]) < 1e-4, "voices must cut on discontinuity");
    }

    fn clip_sched(notes: Vec<Note>, len: f64) -> Schedule {
        let mut spec = GraphSpec::default();
        let q = spec.push(NodeSpec::Seq {
            notes,
            subloops: Vec::new(),
            loop_len_beats: Some(len),
            params: Default::default(),
        });
        spec.set_output(q);
        spec.compile(48_000, 256).unwrap()
    }

    #[test]
    fn clip_repeats_forever_while_timeline_rolls() {
        // A 1-beat clip with one short note. Roll the timeline 6 beats with
        // NO transport loop: the note must sound in every cycle.
        let mut sched = clip_sched(
            vec![Note {
                start_beats: 0.0,
                len_beats: 0.3,
                pitch: 69,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            1.0,
        );
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut b = 0.0;
        let mut onsets = 0u32;
        let mut prev_quiet = true;
        while b < 6.0 {
            sched.run(&mut out, &play_ctx(b, b == 0.0));
            let level = rms(&out[..256]);
            if level > 0.05 && prev_quiet {
                onsets += 1;
            }
            prev_quiet = level <= 0.05;
            b += 256.0 * bps;
        }
        assert!(
            onsets >= 5,
            "clip must retrigger every cycle (saw {onsets} onsets in 6 beats)"
        );
    }

    #[test]
    fn note_ringing_past_clip_end_is_cut_at_wrap() {
        // Note len 5 beats in a 1-beat clip. The compile clamp cuts it at
        // each wrap before its retrigger. Without the clamp, every cycle
        // stacks another sustaining voice — level grows cycle over cycle
        // until voice stealing. With it, one voice (plus a 60ms release
        // tail) at a time: level stays bounded.
        let mut sched = clip_sched(
            vec![Note {
                start_beats: 0.0,
                len_beats: 5.0,
                pitch: 45,
                vel: 127,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            1.0,
        );
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut b = 0.0;
        let mut early = 0.0f32; // steady level mid-cycle 0
        let mut late = 0.0f32; // steady level mid-cycle 7
        while b < 8.0 {
            sched.run(&mut out, &play_ctx(b, b == 0.0));
            let phase = b % 1.0;
            if (0.4..0.6).contains(&phase) {
                let l = rms(&out[..256]);
                if b < 1.0 {
                    early = early.max(l);
                } else if b > 7.0 {
                    late = late.max(l);
                }
            }
            b += 256.0 * bps;
        }
        assert!(early > 0.05, "note should sustain within a cycle ({early})");
        assert!(
            late < early * 1.5,
            "voices accumulating across wraps: early {early}, late {late}"
        );
    }

    #[test]
    fn note_starting_past_clip_len_is_dropped() {
        let mut sched = clip_sched(
            vec![Note {
                start_beats: 2.0,
                len_beats: 0.5,
                pitch: 60,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            1.0,
        );
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut b = 0.0;
        let mut peak = 0.0f32;
        while b < 4.0 {
            sched.run(&mut out, &play_ctx(b, b == 0.0));
            peak = peak.max(rms(&out[..256]));
            b += 256.0 * bps;
        }
        assert!(
            peak < 1e-4,
            "out-of-clip note must never sound (peak {peak})"
        );
    }

    /// The SATURATED path: more simultaneous held notes than voices, so
    /// every note-on runs the fallback steal scan. The test above never
    /// fills all eight voices, which left the steal loop unproven under
    /// `assert_no_alloc` — and the steal loop is the part this synth's
    /// polyphony rests on.
    #[test]
    fn seq_voice_stealing_does_not_allocate() {
        // Twelve overlapping long notes against eight voices.
        let notes: Vec<Note> = (0..12)
            .map(|i| Note {
                start_beats: i as f64 * 0.125,
                len_beats: 8.0,
                pitch: 60 + i as u8,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let mut sched = seq_sched(notes);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            let mut b = 0.0;
            for i in 0..200 {
                let mut c = play_ctx(b, i == 0);
                c.beats_per_sample = bps;
                sched.run(&mut out, &c);
                b += 256.0 * bps;
            }
        });
    }

    fn kick_sched(params: crate::audio::kick::KickParams) -> Schedule {
        let notes: Vec<Note> = (0..4)
            .map(|i| Note {
                start_beats: i as f64,
                len_beats: 0.1,
                // The drum machine C, so the tune knob means what it says.
                pitch: 36,
                vel: 110,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let mut spec = GraphSpec::default();
        let k = spec.push(NodeSpec::Kick {
            notes,
            subloops: Vec::new(),
            loop_len_beats: None,
            params,
        });
        spec.set_output(k);
        spec.compile(48_000, 256).unwrap()
    }

    /// The kick reaches the output through the ordinary schedule, on the
    /// beat, and stops when the pattern does.
    #[test]
    fn a_kick_pattern_sounds_through_the_graph() {
        let mut sched = kick_sched(crate::audio::kick::KickParams::default());
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        let mut beat = 0.0;
        let mut loudest = 0.0f32;
        for i in 0..64 {
            let mut c = play_ctx(beat, i == 0);
            c.beats_per_sample = bps;
            sched.run(&mut out, &c);
            loudest = loudest.max(out[..256].iter().fold(0.0f32, |p, s| p.max(s.abs())));
            beat += 256.0 * bps;
        }
        assert!(loudest > 0.05, "the pattern made no sound: {loudest}");
        assert!(loudest.is_finite());
    }

    /// A LETTER MOVES THE KICK while it is playing — including the two
    /// pitch envelopes' times, which rebuild the voice's envelope
    /// coefficients rather than waiting for a recompile.
    #[test]
    fn kick_letters_reach_the_voice() {
        use crate::params::kick as kp;
        let mut spec = GraphSpec::default();
        let k = spec.push(NodeSpec::Kick {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 0.1,
                pitch: 36,
                vel: 110,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: crate::audio::kick::KickParams::default(),
        });
        spec.set_output(k);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // Every row of the table, applied as a letter, lands in range and
        // leaves the graph rendering finite audio.
        let mut out = vec![0.0f32; 512];
        let bps = 120.0 / 60.0 / 48_000.0;
        for def in kp::TABLE {
            for value in [def.min, def.max, (def.min + def.max) * 0.5] {
                sched.apply(ParamChange {
                    node: k.to_bits(),
                    param: def.id,
                    value,
                });
                let mut c = play_ctx(0.0, true);
                c.beats_per_sample = bps;
                sched.run(&mut out, &c);
                assert!(
                    out.iter().all(|s| s.is_finite()),
                    "{} at {value} produced a non-finite sample",
                    def.name
                );
            }
        }
    }

    /// The whole kick path — two pitch envelopes, a noise click, a
    /// thirty-two section disperser and a saturator — allocates nothing
    /// in the callback, including across the note boundaries where the
    /// voice re-tunes its disperser.
    #[test]
    fn kick_run_does_not_allocate() {
        let params = crate::audio::kick::KickParams {
            disperse_stages: crate::params::kick::DISP_STAGES_MAX,
            click_level: 1.0,
            drive: 6.0,
            ..crate::audio::kick::KickParams::default()
        };
        let mut sched = kick_sched(params);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            let mut beat = 0.0;
            for i in 0..200 {
                let mut c = play_ctx(beat, i == 0);
                c.beats_per_sample = bps;
                sched.run(&mut out, &c);
                beat += 256.0 * bps;
            }
        });
    }

    #[test]
    fn seq_run_does_not_allocate() {
        let notes: Vec<Note> = (0..16)
            .map(|i| Note {
                start_beats: i as f64 * 0.25,
                len_beats: 0.2,
                pitch: 60 + (i % 12) as u8,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            })
            .collect();
        let mut sched = seq_sched(notes);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            let mut b = 0.0;
            for i in 0..200 {
                let mut c = play_ctx(b, i == 0);
                c.beats_per_sample = bps;
                sched.run(&mut out, &c);
                b += 256.0 * bps;
            }
        });
    }

    /// Write a 1s 48k mono 440Hz sine wav; returns its path.
    fn test_wav(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("daw-test-{name}.wav"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..48_000 {
            let v = (i as f32 / 48_000.0 * 440.0 * TAU).sin() * 0.5;
            w.write_sample((v * i16::MAX as f32) as i16).unwrap();
        }
        w.finalize().unwrap();
        path
    }

    fn rate_test_wav(name: &str, sample_rate: u32, frames: u32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("daw-test-{name}-{sample_rate}.wav"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..frames {
            writer
                .write_sample((frame as f32 / sample_rate as f32 * 440.0 * TAU).sin() * 0.5)
                .unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    fn constant_test_wav(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("daw-test-{name}.wav"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 480,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..1_024 {
            writer.write_sample(i16::MAX / 2).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    fn clip_audio_sched(path: std::path::PathBuf, loop_clip: bool) -> Schedule {
        let mut spec = GraphSpec::default();
        let c = spec.push(NodeSpec::AudioClip {
            path,
            start_beats: 0.0,
            length_beats: None,
            source_offset_frames: 0,
            source_frames: None,
            loop_clip,
            loop_start_frames: 0,
            gain: 1.0,
            fade_in_frames: 0,
            fade_out_frames: 0,
            fade_in_shape: 0.0,
            fade_out_shape: 0.0,
            envelope: Vec::new(),
        });
        spec.set_output(c);
        spec.compile(48_000, 256).unwrap()
    }

    #[test]
    fn audio_clip_compile_converts_every_frame_coordinate_to_the_device_rate() {
        let path = rate_test_wav("rate-domain", 44_100, 44_100);
        let mut spec = GraphSpec::default();
        let clip = spec.push(NodeSpec::AudioClip {
            path: path.clone(),
            start_beats: 0.0,
            length_beats: None,
            source_offset_frames: 4_410,
            source_frames: Some(8_820),
            loop_clip: true,
            loop_start_frames: 2_205,
            gain: 1.0,
            fade_in_frames: 441,
            fade_out_frames: 882,
            fade_in_shape: 0.0,
            fade_out_shape: 0.0,
            envelope: vec![(0, 1.0), (4_410, 0.5)],
        });
        spec.set_output(clip);
        let sched = spec.compile(48_000, 256).unwrap();
        let Node::AudioClip {
            file_frames,
            source_start,
            source_frames,
            loop_from,
            fade_in,
            fade_out,
            envelope,
            ..
        } = &sched.nodes[0]
        else {
            panic!("the clip compiled to its stream node");
        };
        assert_eq!(*file_frames, 48_000);
        assert_eq!(*source_start, 4_800);
        assert_eq!(*source_frames, 9_600);
        assert_eq!(*loop_from, 7_200);
        assert_eq!(*fade_in, 480);
        assert_eq!(*fade_out, 960);
        assert_eq!(envelope, &[(0, 1.0), (4_800, 0.5)]);

        let cached = crate::library::import_wav(&path, 48_000).unwrap().path;
        drop(sched);
        std::fs::remove_file(cached).ok();
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn audio_clip_frame_mapping_is_exact_for_common_rate_changes() {
        assert_eq!(frame_at_rate(44_100, 44_100, 48_000), 48_000);
        assert_eq!(frame_at_rate(96_000, 96_000, 48_000), 48_000);
        assert_eq!(frame_at_rate(48_000, 48_000, 44_100), 44_100);
        let start = frame_at_rate(u64::MAX - 10, 44_100, 48_000);
        assert_eq!(start, u64::MAX, "extreme coordinates saturate, not wrap");
    }

    #[test]
    fn a_one_frame_audio_loop_can_fill_a_whole_callback() {
        let path = std::env::temp_dir().join(format!(
            "daw-test-one-frame-loop-{}.wav",
            std::process::id()
        ));
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        writer.write_sample(0.5f32).unwrap();
        writer.finalize().unwrap();
        let mut sched = clip_audio_sched(path.clone(), true);
        assert!(sched.wait_streams_ready(std::time::Duration::from_secs(2)));
        let mut out = vec![0.0; 512];
        let mut context = play_ctx(0.0, true);
        context.position = 0;
        sched.run(&mut out, &context);
        context.discontinuity = false;
        context.position = 256;
        sched.run(&mut out, &context);
        assert!(
            out[..256].iter().all(|sample| *sample > 0.49),
            "the loop must not stop at the old eight-read ceiling"
        );
        drop(sched);
        std::fs::remove_file(path).ok();
    }

    /// Run blocks at increasing positions until sound appears (the IO thread
    /// needs a moment) — returns the level once playing, panicking if the
    /// stream never becomes audible.
    fn wait_for_audio(sched: &mut Schedule, out: &mut [f32]) -> f32 {
        for _ in 0..500 {
            let mut c = play_ctx(0.0, true); // re-seek to 0 each attempt
            c.position = 0;
            sched.run(out, &c);
            let l = rms(&out[..256]);
            if l > 0.05 {
                return l;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("audio clip never became ready");
    }

    #[test]
    fn audio_clip_streams_and_loops() {
        let mut sched = clip_audio_sched(test_wav("loop"), true);
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out);

        // Play linearly across the file end: position past 48_000 must wrap
        // and keep sounding (a 1s file looping over a >1s timeline).
        let mut pos: u64 = 47_000;
        let mut c = play_ctx(0.0, true);
        c.position = pos;
        sched.run(&mut out, &c);
        std::thread::sleep(std::time::Duration::from_millis(50)); // let IO chase the seek
        let mut quiet_blocks = 0;
        for _ in 0..40 {
            let mut c = play_ctx(0.0, false);
            c.position = pos;
            sched.run(&mut out, &c);
            if rms(&out[..256]) < 0.05 {
                quiet_blocks += 1;
            }
            pos += 256;
            // The real callback gives the IO thread 5.33ms of wall time per
            // block; a tight loop starves it and tests nothing real.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // Brief silence at the wrap is tolerated (async seek); with the
        // start-of-file cache pinned it should be rare — the loop must
        // recover and keep sounding.
        assert!(
            quiet_blocks < 10,
            "loop did not recover after wrap ({quiet_blocks}/40 quiet)"
        );
    }

    #[test]
    fn audio_clip_one_shot_goes_silent_at_end() {
        let mut sched = clip_audio_sched(test_wav("oneshot"), false);
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out);
        let mut c = play_ctx(0.0, true);
        c.position = 60_000; // past the 48_000-frame file
        sched.run(&mut out, &c);
        let mut c2 = play_ctx(0.0, false);
        c2.position = 60_256;
        sched.run(&mut out, &c2);
        assert!(
            rms(&out[..256]) < 1e-4,
            "one-shot must be silent past its end"
        );
    }

    #[test]
    fn placed_audio_clip_starts_and_ends_inside_a_block_exactly() {
        // At 480 Hz / 120 bpm one beat is 240 device samples. The clip is
        // beat 1..2, so a block at sample 200 contains 40 leading silent
        // frames, 216 audio frames; a block at 400 contains 80 audio frames
        // followed by silence.
        let mut spec = GraphSpec::default();
        let clip = spec.push(NodeSpec::AudioClip {
            path: constant_test_wav("placed-boundaries"),
            start_beats: 1.0,
            length_beats: Some(1.0),
            source_offset_frames: 0,
            source_frames: Some(1_024),
            loop_clip: false,
            loop_start_frames: 0,
            gain: 1.0,
            fade_in_frames: 0,
            fade_out_frames: 0,
            fade_in_shape: 0.0,
            fade_out_shape: 0.0,
            envelope: Vec::new(),
        });
        spec.set_output(clip);
        let mut sched = spec.compile_at_tempo(480, 256, 120.0).unwrap();
        let mut out = vec![0.0f32; 512];

        let run_until_audible = |sched: &mut Schedule, out: &mut [f32], position: u64| {
            for _ in 0..500 {
                let mut ctx = play_ctx(0.0, true);
                ctx.position = position;
                sched.run(out, &ctx);
                if rms(&out[..256]) > 0.01 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            panic!("placed audio never became ready");
        };

        run_until_audible(&mut sched, &mut out, 200);
        assert!(out[..40].iter().all(|sample| *sample == 0.0));
        assert!(rms(&out[41..256]) > 0.1);

        run_until_audible(&mut sched, &mut out, 400);
        assert!(rms(&out[..80]) > 0.1);
        assert!(out[80..256].iter().all(|sample| *sample == 0.0));

        let mut before = play_ctx(0.0, true);
        before.position = 0;
        before.len = 128;
        sched.run(&mut out, &before);
        assert!(out[..128].iter().all(|sample| *sample == 0.0));
        let mut after = play_ctx(0.0, true);
        after.position = 480;
        sched.run(&mut out, &after);
        assert!(out[..256].iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn audio_clip_missing_file_is_silent_not_fatal() {
        // A missing sample must not refuse the graph or make noise.
        let mut sched = clip_audio_sched(std::path::PathBuf::from("/nonexistent/nope.wav"), true);
        let mut out = vec![1.0f32; 512];
        sched.run(&mut out, &play_ctx(0.0, true));
        assert!(out[..256].iter().all(|s| *s == 0.0));
    }

    /// THE SHAPED ENVELOPE IS THE SHARED CURVE, and a shape of zero is
    /// EXACTLY the straight ramp this node applied before shapes existed.
    ///
    /// Stated over the arithmetic rather than over a rendered clip for
    /// the same reason its linear sibling above is: the fade a file
    /// exercises is the fade that already worked, and it is the numbers
    /// that can be wrong.
    #[test]
    fn a_shaped_fade_follows_the_shared_curve() {
        use crate::params::clip::Curve;
        let envelope = |pos: u64,
                        fade_in: u64,
                        fade_out: u64,
                        span: u64,
                        in_curve: Curve,
                        out_curve: Curve| {
            let mut level = 1.0f32;
            if fade_in > 0 && pos < fade_in {
                level = in_curve.at(pos as f32 / fade_in as f32);
            }
            if fade_out > 0 {
                let left = span.saturating_sub(pos);
                if left <= fade_out {
                    level = level.min(out_curve.at(left as f32 / fade_out as f32));
                }
            }
            level
        };
        let span = 1_000u64;
        let flat = Curve::LINEAR;

        // A shape of zero is the old straight ramp, to the bit.
        for pos in [0u64, 1, 37, 50, 99, 100, 500, 999, 1_000] {
            let straight = {
                let mut level = 1.0f32;
                if pos < 100 {
                    level = pos as f32 / 100.0;
                }
                let left = span.saturating_sub(pos);
                if left <= 100 {
                    level = level.min(left as f32 / 100.0);
                }
                level
            };
            assert_eq!(envelope(pos, 100, 100, span, flat, flat), straight, "{pos}");
        }

        // A shaped fade still starts silent and ends whole, whatever the
        // shape — the two properties that make it a fade at all.
        for shape in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            let curve = Curve::new(shape);
            assert_eq!(envelope(0, 100, 0, span, curve, flat), 0.0, "shape {shape}");
            assert!(
                (envelope(100, 100, 0, span, curve, flat) - 1.0).abs() < 1e-5,
                "shape {shape}"
            );
            assert_eq!(envelope(span, 0, 100, span, flat, curve), 0.0);
            assert!((envelope(span - 100, 0, 100, span, flat, curve) - 1.0).abs() < 1e-5);
        }

        // And the two ends carry their OWN shapes: a clip fading in fast
        // and out slow is two different curves on one clip.
        let fast = Curve::new(0.8);
        let slow = Curve::new(-0.8);
        let rising = envelope(50, 100, 0, span, fast, flat);
        let falling = envelope(span - 50, 0, 100, span, flat, slow);
        assert!(rising > 0.5, "the fast end is past halfway at halfway");
        assert!(falling < 0.5, "and the slow one is not");
    }

    /// The envelope is SORTED AND CLAMPED GREEN-SIDE, because the
    /// callback's walk assumes it rises and stays inside the clip.
    #[test]
    fn a_compiled_envelope_is_sorted_and_inside_the_clip() {
        let messy = [
            (500u64, 1.0f32),
            (0, 0.5),
            // Past the span: dropped, not clamped onto the end, where it
            // would pile up and give the last segment a zero-length gap
            // to interpolate across.
            (9_000, 0.25),
            (200, 2.0),
            // A duplicate frame, which would be a vertical step.
            (200, 0.75),
            // Nonsense, which would poison every sample after it.
            (300, f32::NAN),
            (400, f32::INFINITY),
        ];
        let out = sorted_envelope(&messy, 1_000);
        assert_eq!(
            out.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            vec![0, 200, 500],
            "sorted, deduped, inside the clip, and finite"
        );
        assert!(out.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(out.iter().all(|(_, gain)| gain.is_finite()));

        // An empty envelope stays empty — that is the free case, and it
        // must not acquire a point from nowhere.
        assert!(sorted_envelope(&[], 1_000).is_empty());
    }

    /// THE ENVELOPE IS A RAMP, and it lands on its points.
    ///
    /// Stated over the walk rather than over a rendered clip, for the
    /// reason the fades' own test gives: the arithmetic is what can be
    /// wrong, and a file only makes it slower to find out.
    #[test]
    fn the_envelope_interpolates_between_its_points() {
        let points = sorted_envelope(&[(0, 1.0), (1_000, 0.0)], 2_000);
        let mut cursor = 0usize;
        let at = |pos: u64, cursor: &mut usize| -> f32 {
            while *cursor + 1 < points.len() && points[*cursor + 1].0 <= pos {
                *cursor += 1;
            }
            let (at, gain) = points[*cursor];
            let Some(&(next_at, next_gain)) = points.get(*cursor + 1) else {
                return gain;
            };
            if pos <= at || next_at <= at {
                return gain;
            }
            gain + (next_gain - gain) * ((pos - at) as f32 / (next_at - at) as f32)
        };
        assert_eq!(at(0, &mut cursor), 1.0);
        assert!((at(500, &mut cursor) - 0.5).abs() < 1e-6);
        assert_eq!(at(1_000, &mut cursor), 0.0);
        // HELD FLAT past the last point, which is what the picture draws.
        assert_eq!(at(1_500, &mut cursor), 0.0);

        // A single point is a constant, not a ramp to nowhere.
        let one = sorted_envelope(&[(500, 0.25)], 2_000);
        let mut cursor = 0usize;
        let at_one = |pos: u64, cursor: &mut usize| -> f32 {
            while *cursor + 1 < one.len() && one[*cursor + 1].0 <= pos {
                *cursor += 1;
            }
            one[*cursor].1
        };
        for pos in [0u64, 499, 500, 1_999] {
            assert_eq!(at_one(pos, &mut cursor), 0.25);
        }
    }

    /// A DISCONTINUITY RE-FINDS THE CURSOR. Locating into the middle of a
    /// clip must land on the right gain on the FIRST sample after the
    /// jump — a cursor left where the last block finished would ramp from
    /// the wrong place, audibly.
    #[test]
    fn a_discontinuity_re_finds_the_envelope_cursor() {
        let points = sorted_envelope(
            &[(0, 1.0), (1_000, 0.5), (2_000, 0.25), (3_000, 0.125)],
            4_000,
        );
        // The node's own expression for the re-find.
        let refind = |local: u64| {
            points
                .partition_point(|(at, _)| *at <= local)
                .saturating_sub(1)
        };
        assert_eq!(refind(0), 0);
        assert_eq!(refind(999), 0);
        // Landing exactly ON a point starts from that point, so the
        // first sample reads its gain and then ramps towards the next —
        // not from the segment before it.
        assert_eq!(refind(1_000), 1);
        assert_eq!(refind(1_001), 1);
        assert_eq!(refind(2_500), 2);
        assert_eq!(refind(9_999), 3, "past the end holds the last point");

        // And from any of them, the very next sample reads the right
        // gain rather than the one the previous block left behind.
        for local in [0u64, 1_500, 2_999, 3_500] {
            let mut cursor = refind(local);
            while cursor + 1 < points.len() && points[cursor + 1].0 <= local {
                cursor += 1;
            }
            let (at, gain) = points[cursor];
            assert!(at <= local, "the cursor is never ahead of the playhead");
            assert!(gain > 0.0);
        }
    }

    /// AN EMPTY ENVELOPE IS A NO-OP, and so is a flat unity one. A clip
    /// written before envelopes existed must sound exactly as it did.
    ///
    /// Asserted over the ARITHMETIC rather than over two rendered clips.
    /// The first version of this test compiled two schedules over the
    /// same WAV, warmed each with `wait_for_audio`, and compared them
    /// sample by sample — and it was racy by construction: that warm-up
    /// loops until audio appears, creek decodes on its own thread, and
    /// the two streams end up at different playheads. It passed most
    /// runs and failed some, which is worse than not existing.
    ///
    /// What the node actually promises is that the envelope multiply is
    /// SKIPPED when there are no points, and is a multiply by exactly one
    /// when the points are all unity. Both are checkable without a file.
    #[test]
    fn an_empty_envelope_changes_nothing() {
        // Nothing in, nothing compiled — so `rode` is false and the
        // per-sample multiply never runs at all.
        assert!(sorted_envelope(&[], 96_000).is_empty());

        // A flat unity envelope reads exactly 1.0 everywhere, so the
        // multiply that does run changes nothing.
        let flat = sorted_envelope(&[(0, 1.0), (96_000, 1.0)], 96_000);
        assert_eq!(flat.len(), 2);
        let mut cursor = 0usize;
        let at = |pos: u64, cursor: &mut usize| -> f32 {
            while *cursor + 1 < flat.len() && flat[*cursor + 1].0 <= pos {
                *cursor += 1;
            }
            let (at, gain) = flat[*cursor];
            let Some(&(next_at, next_gain)) = flat.get(*cursor + 1) else {
                return gain;
            };
            if pos <= at || next_at <= at {
                return gain;
            }
            gain + (next_gain - gain) * ((pos - at) as f32 / (next_at - at) as f32)
        };
        for pos in [0u64, 1, 12_345, 48_000, 95_999, 96_000, 200_000] {
            assert_eq!(at(pos, &mut cursor), 1.0, "unity is unity at {pos}");
        }

        // And a NON-unity envelope does change something, so the test
        // above is not passing because nothing is wired up.
        let ramp = sorted_envelope(&[(0, 1.0), (96_000, 0.0)], 96_000);
        let mut cursor = 0usize;
        let at = |pos: u64, cursor: &mut usize| -> f32 {
            while *cursor + 1 < ramp.len() && ramp[*cursor + 1].0 <= pos {
                *cursor += 1;
            }
            let (at, gain) = ramp[*cursor];
            let Some(&(next_at, next_gain)) = ramp.get(*cursor + 1) else {
                return gain;
            };
            if pos <= at || next_at <= at {
                return gain;
            }
            gain + (next_gain - gain) * ((pos - at) as f32 / (next_at - at) as f32)
        };
        assert!((at(48_000, &mut cursor) - 0.5).abs() < 1e-4);
    }

    /// The envelope allocates nothing, including across a discontinuity —
    /// where the cursor is re-found, which is the only search on the
    /// path and the place a `Vec` would be easiest to reach for.
    #[test]
    fn an_enveloped_clip_does_not_allocate() {
        let path = test_wav("envnoalloc");
        let mut spec = GraphSpec::default();
        let c = spec.push(NodeSpec::AudioClip {
            path,
            start_beats: 0.0,
            length_beats: None,
            source_offset_frames: 0,
            source_frames: None,
            loop_clip: true,
            loop_start_frames: 0,
            gain: 1.0,
            fade_in_frames: 0,
            fade_out_frames: 0,
            fade_in_shape: 0.0,
            fade_out_shape: 0.0,
            envelope: (0..64).map(|i| (i * 700, 1.0 - i as f32 / 128.0)).collect(),
        });
        spec.set_output(c);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out);
        let mut pos = 0u64;
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..80 {
                let mut ctx = play_ctx(0.0, false);
                ctx.position = pos;
                sched.run(&mut out, &ctx);
                pos += 256;
            }
            // And the re-find path: a discontinuity at a position well
            // past where the walk had got to.
            for jump in [30_000u64, 1_000, 44_000, 200] {
                let mut ctx = play_ctx(0.0, true);
                ctx.position = jump;
                sched.run(&mut out, &ctx);
            }
        });
    }

    #[test]
    fn audio_clip_read_does_not_allocate() {
        let mut sched = clip_audio_sched(test_wav("noalloc"), true);
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out); // warm: stream ready, buffers up
        let mut pos: u64 = 0;
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                let mut c = play_ctx(0.0, false);
                c.position = pos;
                sched.run(&mut out, &c);
                pos += 256;
            }
        });
    }

    /// A CURVED FADE ALLOCATES NOTHING. The curve is a multiply and a
    /// divide per sample and must stay that way: this is the test that
    /// notices if it ever grows a table, a `Vec`, or a `powf` behind a
    /// feature flag that pulls one in.
    #[test]
    fn a_curved_fade_does_not_allocate() {
        let path = test_wav("curvednoalloc");
        let mut spec = GraphSpec::default();
        let c = spec.push(NodeSpec::AudioClip {
            path,
            start_beats: 0.0,
            length_beats: None,
            source_offset_frames: 0,
            source_frames: None,
            loop_clip: true,
            loop_start_frames: 0,
            gain: 1.0,
            // The fade OUT covers most of the clip, so the blocks below
            // land inside it. The fade IN starts at zero and is turned on
            // by a letter further down — warming the stream needs an
            // audible first block, and a clip that starts inside a long
            // fade in has not got one.
            fade_in_frames: 0,
            fade_out_frames: 40_000,
            fade_in_shape: 0.7,
            fade_out_shape: -0.7,
            envelope: Vec::new(),
        });
        spec.set_output(c);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out);
        let mut pos: u64 = 0;
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                let mut c = play_ctx(0.0, false);
                c.position = pos;
                sched.run(&mut out, &c);
                pos += 256;
            }
        });

        // The fade IN, its shape, and both arriving as LETTERS mid-run:
        // that is the whole reason they are letters, and it is where an
        // allocation would hide if one ever crept in.
        assert_no_alloc::assert_no_alloc(|| {
            sched.apply(ParamChange {
                node: c.to_bits(),
                param: crate::params::clip::FADE_IN,
                value: 20_000.0,
            });
            let mut pos: u64 = 0;
            for step in 0..40 {
                sched.apply(ParamChange {
                    node: c.to_bits(),
                    param: crate::params::clip::FADE_IN_CURVE,
                    value: step as f32 / 40.0 - 0.5,
                });
                sched.apply(ParamChange {
                    node: c.to_bits(),
                    param: crate::params::clip::FADE_OUT_CURVE,
                    value: 0.5 - step as f32 / 40.0,
                });
                let mut ctx = play_ctx(0.0, false);
                ctx.position = pos;
                sched.run(&mut out, &ctx);
                pos += 256;
            }
        });
    }

    #[test]
    fn pan_follows_constant_power_law() {
        // Sine -> Pan(hard left) -> output: all energy left, none right.
        // Then center: equal energy both sides, each at cos(45°) ≈ 0.707.
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        let p0 = spec.push(NodeSpec::Pan {
            pan: -1.0,
            gain: 1.0,
        });
        spec.connect(s0, p0);
        spec.set_output(p0);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out); // ramp-in
        run(&mut sched, &mut out);
        let (l, r) = out.split_at(256);
        assert!(rms(l) > 0.2, "hard left must carry the signal");
        assert!(rms(r) < 1e-3, "hard left must silence the right");

        sched.apply(ParamChange {
            node: p0.to_bits(),
            param: 0,
            value: 0.0,
        });
        run(&mut sched, &mut out); // pan ramp
        run(&mut sched, &mut out);
        let (l, r) = out.split_at(256);
        let ratio = rms(l) / rms(r).max(1e-9);
        assert!(
            (0.9..1.1).contains(&ratio),
            "center must balance (ratio {ratio})"
        );
    }

    /// The fader on the track's output stage: it scales, it takes a
    /// letter, and it ramps rather than stepping — a level change must not
    /// be a click.
    #[test]
    fn pan_node_carries_the_fader() {
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        let p0 = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        spec.connect(s0, p0);
        spec.set_output(p0);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out); // sine ramp-in
        run(&mut sched, &mut out);
        let unity = rms(&out[..256]);
        assert!(unity > 0.2, "unity passes the signal");

        // Half amplitude: −6 dB, and the letter is what carries it.
        sched.apply(ParamChange {
            node: p0.to_bits(),
            param: crate::params::pan::GAIN,
            value: 0.5,
        });
        run(&mut sched, &mut out); // the gain ramp
        run(&mut sched, &mut out);
        let halved = rms(&out[..256]);
        let ratio = halved / unity.max(1e-9);
        assert!(
            (0.45..0.55).contains(&ratio),
            "half gain must halve the level (ratio {ratio})"
        );

        // Silence at zero, and no step to get there: the ramp block must
        // not contain a discontinuity bigger than the signal itself.
        sched.apply(ParamChange {
            node: p0.to_bits(),
            param: crate::params::pan::GAIN,
            value: 0.0,
        });
        run(&mut sched, &mut out);
        let jumps = out[..256]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            jumps < 0.1,
            "the level ramps rather than stepping ({jumps})"
        );
        run(&mut sched, &mut out);
        assert!(rms(&out[..256]) < 1e-4, "a closed fader is silent");
    }

    /// A meter has TWO SIDES, and a hard-panned source proves they are
    /// measured separately rather than one figure copied twice.
    ///
    /// This is the test that makes an L/R pair on a mixer honest: without
    /// it, two meters drawn from one number would be a mark claiming a
    /// distinction the engine never made.
    #[test]
    fn a_panned_source_meters_on_the_side_it_was_sent_to() {
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        // Hard right: constant-power pan puts nothing on the left.
        let p0 = spec.push(NodeSpec::Pan {
            pan: 1.0,
            gain: 1.0,
        });
        spec.connect(s0, p0);
        spec.set_output(p0);
        spec.meter(3, p0);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];

        run(&mut sched, &mut out); // the sine and the pan both ramp in
        run(&mut sched, &mut out);
        sched.clear_peaks();
        run(&mut sched, &mut out);

        let left = sched.peaks_l()[3];
        let right = sched.peaks_r()[3];
        assert!(right > 0.2, "nothing arrived on the side it was sent to");
        assert!(
            left < right * 0.1,
            "the left side heard a hard-right source ({left} against {right})"
        );
        assert_eq!(
            sched.peak(3),
            right,
            "the single-number reading is the louder side"
        );
    }

    /// Meter taps report the level at the node they are attached to, per
    /// block, and reset between blocks rather than latching.
    #[test]
    fn meter_taps_report_their_node_s_peak() {
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        let p0 = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        spec.connect(s0, p0);
        spec.set_output(p0);
        spec.meter(3, p0);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];

        sched.clear_peaks();
        run(&mut sched, &mut out); // sine ramps in
        run(&mut sched, &mut out);
        let peak = sched.peak(3);
        assert!(peak > 0.2, "the tapped node's level is reported ({peak})");
        assert!(peak <= 1.0, "and it is an amplitude, not a sum");
        assert_eq!(
            sched.peak(0),
            0.0,
            "an untapped slot stays silent — slots are the CALLER's index"
        );

        // A closed fader reads silence on the very next window: the meter
        // reports what just happened, not what once did.
        sched.apply(ParamChange {
            node: p0.to_bits(),
            param: crate::params::pan::GAIN,
            value: 0.0,
        });
        run(&mut sched, &mut out); // ramp down
        sched.clear_peaks();
        run(&mut sched, &mut out);
        assert!(
            sched.peak(3) < 1e-4,
            "peaks reset per window rather than latching"
        );
    }

    #[test]
    fn stereo_wav_keeps_channels_separate() {
        // L = tone, R = silence on disk must come out the same way.
        let path = std::env::temp_dir().join("daw-test-stereo.wav");
        let spec_w = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec_w).unwrap();
        for i in 0..48_000 {
            let v = (i as f32 / 48_000.0 * 440.0 * TAU).sin() * 0.5;
            w.write_sample((v * i16::MAX as f32) as i16).unwrap(); // L
            w.write_sample(0i16).unwrap(); // R
        }
        w.finalize().unwrap();

        let mut sched = clip_audio_sched(path, true);
        let mut out = vec![0.0f32; 512];
        wait_for_audio(&mut sched, &mut out);
        let (l, r) = out.split_at(256);
        assert!(rms(l) > 0.1, "left channel must carry the tone");
        assert!(
            rms(r) < 1e-3,
            "right channel must stay silent (got {})",
            rms(r)
        );
    }

    #[test]
    fn mono_into_stereo_mixer_is_centered() {
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 500.0,
            amp: 0.4,
        });
        let m = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(s0, m);
        spec.set_output(m);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        run(&mut sched, &mut out);
        let (l, r) = out.split_at(256);
        assert_eq!(l, r, "mono source through a stereo mixer must be centered");
        assert!(rms(l) > 0.1);
    }

    #[test]
    fn slots_are_64_byte_aligned() {
        let mut arena = Arena::new(3, 256);
        for i in 0..3 {
            let ptr = arena.slot_mut(SlotId(i)).as_ptr() as usize;
            assert_eq!(ptr % 64, 0, "slot {i} misaligned");
        }
    }

    #[test]
    fn schedule_run_does_not_allocate() {
        let mut spec = GraphSpec::default();
        let a = spec.push(NodeSpec::Sine {
            freq: 440.0,
            amp: 0.1,
        });
        let b = spec.push(NodeSpec::Sine {
            freq: 550.0,
            amp: 0.1,
        });
        let mic = spec.push(NodeSpec::Input { channel: 0 });
        let m = spec.push(NodeSpec::Mixer { gain: 0.8 });
        spec.connect(a, m);
        spec.connect(b, m);
        spec.connect(mic, m);
        spec.set_output(m);
        // Metered, so the guard covers the peak fold as well as the walk —
        // an untapped graph would exercise only the sentinel check.
        spec.meter(0, m);
        spec.meter(1, a);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        // Same guard the real callback runs under: abort on any allocation.
        let c = ctx(NO_INPUT);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                sched.clear_peaks();
                sched.run(&mut out, &c);
            }
        });
        assert!(sched.peak(0) > 0.0, "the tap did its work under the guard");
    }

    /// A fader jump spreads over the whole BLOCK even when the transport
    /// splits that block into segments. Regression: ramping across the
    /// SEGMENT let a 1-frame leading segment complete the move in one
    /// sample, which is a click.
    #[test]
    fn the_fader_ramps_across_the_block_not_the_segment() {
        let mut spec = GraphSpec::default();
        let s0 = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        let p0 = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        spec.connect(s0, p0);
        spec.set_output(p0);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out); // sine ramp-in
        run(&mut sched, &mut out);

        // Close the fader, then run the block as a 1-frame segment
        // followed by the remaining 255 — a loop point landing at the very
        // start of a block.
        sched.apply(ParamChange {
            node: p0.to_bits(),
            param: crate::params::pan::GAIN,
            value: 0.0,
        });
        let mut first = ctx(NO_INPUT);
        first.len = 1;
        sched.run(&mut out, &first);
        let Node::Pan { gain, .. } = sched.nodes[1] else {
            panic!("node 1 is the pan");
        };
        assert!(
            gain > 0.99,
            "one frame of a 256-frame block moves the level by ~1/256, not all of it ({gain})"
        );

        let mut rest = ctx(NO_INPUT);
        rest.offset = 1;
        rest.len = 255;
        sched.run(&mut out, &rest);
        let Node::Pan { gain, .. } = sched.nodes[1] else {
            panic!("node 1 is the pan");
        };
        assert_eq!(gain, 0.0, "and the block still lands exactly on target");
    }

    /// A Song lane is deliberately different from a live fader letter: its
    /// value names the current fixed-grid control segment's endpoint, so it
    /// must arrive exactly there regardless of the device block size.
    #[test]
    fn timeline_automation_lands_the_fader_at_the_control_segment_end() {
        const SAMPLES_PER_BEAT: f64 = 24_000.0;
        let mut spec = GraphSpec::default();
        let source = spec.push(NodeSpec::Sine {
            freq: 1_000.0,
            amp: 0.5,
        });
        let fader = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        spec.connect(source, fader);
        spec.set_output(fader);
        spec.automate(
            fader,
            crate::params::pan::GAIN,
            1.0,
            vec![AutomationPoint {
                beat: 32.0 / SAMPLES_PER_BEAT,
                value: 0.0,
                bend: 0.0,
            }],
        );

        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];
        let mut first_control_segment = play_ctx(0.0, true);
        first_control_segment.len = 32;
        sched.run(&mut out, &first_control_segment);

        let Node::Pan { gain, .. } = sched.nodes[1] else {
            panic!("node 1 is the pan");
        };
        assert_eq!(
            gain, 0.0,
            "the automated endpoint belongs to sample 32, not block sample 256"
        );
    }

    /// A project file can say anything. A non-finite or out-of-range level
    /// must not become a permanent output multiplier: the letter door
    /// guards itself, so the COMPILE door has to guard too.
    #[test]
    fn a_nonsense_level_in_a_spec_cannot_poison_the_output() {
        // The rule: a NON-FINITE level is nonsense and becomes unity — a
        // corrupt file is not a request to be infinitely loud — while a
        // merely out-of-range one is a request that gets clamped.
        for (written, expected) in [
            (f32::NAN, 1.0),
            (f32::INFINITY, 1.0),
            (f32::NEG_INFINITY, 1.0),
            (-5.0, 0.0),
            (1e30, crate::params::pan::TABLE[1].max),
        ] {
            let mut spec = GraphSpec::default();
            let s0 = spec.push(NodeSpec::Sine {
                freq: 1_000.0,
                amp: 0.5,
            });
            let p0 = spec.push(NodeSpec::Pan {
                pan: 0.0,
                gain: written,
            });
            spec.connect(s0, p0);
            spec.set_output(p0);
            let mut sched = spec.compile(48_000, 256).unwrap();
            let Node::Pan { gain, .. } = sched.nodes[1] else {
                panic!("node 1 is the pan");
            };
            assert_eq!(gain, expected, "a spec gain of {written}");

            let mut out = vec![0.0f32; 512];
            run(&mut sched, &mut out);
            run(&mut sched, &mut out);
            assert!(
                out.iter().all(|s| s.is_finite()),
                "and the output stays finite with {written}"
            );
        }
    }
    // ------------------------------------------- the extracted clock ---

    /// A second, entirely unrelated instrument. Records what the clock
    /// asked of it and renders a constant, so a test can assert on the
    /// GATING rather than on any audio.
    #[derive(Default)]
    struct Probe {
        log: Vec<String>,
        rendered: usize,
    }

    impl Voices for Probe {
        fn all_sound_off(&mut self) {
            self.log.push("cut".to_owned());
        }
        fn release_all(&mut self) {
            self.log.push("release".to_owned());
        }
        fn note_off(&mut self, pitch: u8) {
            self.log.push(format!("off {pitch}"));
        }
        fn note_on(&mut self, pitch: u8, vel: u8, age: u64) {
            self.log.push(format!("on {pitch} v{vel} a{age}"));
        }
        fn plock(&mut self, param: u32, value: Option<f32>) {
            match value {
                Some(v) => self.log.push(format!("lock {param}={v}")),
                None => self.log.push(format!("unlock {param}")),
            }
        }
        fn plock_glide(&mut self, param: u32, alpha: f32) {
            self.log.push(format!("glide {param} {alpha:.3}"));
        }
        fn render(&mut self, out: &mut [f32], _at: usize, gain: &mut Ramp) {
            self.rendered += out.len();
            for s in out.iter_mut() {
                *s = gain.next();
            }
        }
    }

    /// THE POINT OF THE EXTRACTION: the pattern walk drives an instrument
    /// it has never heard of.
    ///
    /// If this compiles and passes, `Node::Poly` does not need a second
    /// copy of the clip/loop/cycle machinery — it needs five methods. The
    /// walk's expensive reasoning (integer cycle derivation, the
    /// strictly-greater wrap reconciliation, the monotonic phase guard)
    /// stays in exactly one place.
    #[test]
    fn the_pattern_clock_drives_an_instrument_it_does_not_know() {
        // Two notes, the second an off at the same sample as an on — the
        // tie-order rule from the sequencing contract.
        let events = vec![
            SeqEvent {
                sample: 0,
                rank: 2,
                pitch: 60,
                vel: 100,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob: 1.0,
                cond: (0, 0),
                trig_key: 0,
            },
            SeqEvent {
                sample: 128,
                rank: 0,
                pitch: 60,
                vel: 0,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob: 1.0,
                cond: (0, 0),
                trig_key: 0,
            },
            SeqEvent {
                sample: 128,
                rank: 2,
                pitch: 67,
                vel: 90,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob: 1.0,
                cond: (0, 0),
                trig_key: 0,
            },
        ];
        let mut clock = PatternClock::new(0, 24_000.0);
        let mut probe = Probe::default();
        let mut out = [0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, out.len());
        clock.run(&mut probe, &events, &mut out, &ctx(NO_INPUT), &mut ramp);

        assert_eq!(probe.rendered, 256, "every sample must be rendered");
        assert_eq!(
            probe.log,
            ["on 60 v100 a0", "off 60", "on 67 v90 a1"],
            "off must land before the on at the same sample"
        );
    }

    #[test]
    fn the_pattern_clock_dispatches_the_whole_trigless_lock_glide() {
        let events = compile_events(
            &[Note {
                start_beats: 0.0,
                len_beats: 0.01,
                pitch: 60,
                vel: 0,
                plocks: vec![(17, 0.75)],
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            &[],
            None,
            1_000.0,
            3,
        )
        .unwrap();
        let mut clock = PatternClock::new(0, 1_000.0);
        let mut probe = Probe::default();
        let mut out = [0.0f32; 16];
        let mut ramp = Ramp::across(1.0, 1.0, out.len());
        clock.run(&mut probe, &events, &mut out, &ctx(NO_INPUT), &mut ramp);

        assert_eq!(
            probe.log,
            [
                "lock 17=0.75",
                "glide 17 0.250",
                "glide 17 0.333",
                "glide 17 0.500",
                "glide 17 1.000",
            ],
            "a trigless lock must return to base without firing a note"
        );
    }

    /// The clock cuts an unknown instrument on a discontinuity, exactly as
    /// it cuts the built-in one — contract rule 2, inherited for free.
    #[test]
    fn the_pattern_clock_cuts_any_instrument_on_a_discontinuity() {
        let events = vec![SeqEvent {
            sample: 0,
            rank: 2,
            pitch: 60,
            vel: 100,
            param: 0,
            value: 0.0,
            restore: false,
            node: 0,
            prob: 1.0,
            cond: (0, 0),
            trig_key: 0,
        }];
        let mut clock = PatternClock::new(0, 24_000.0);
        let mut probe = Probe::default();
        let mut out = [0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, out.len());
        let seek = ProcessCtx {
            discontinuity: true,
            ..ctx(NO_INPUT)
        };
        clock.run(&mut probe, &events, &mut out, &seek, &mut ramp);
        assert_eq!(probe.log.first().map(String::as_str), Some("cut"));
    }

    /// A stopped transport releases rather than cuts, and still fills the
    /// whole buffer — a metered node may never leave a slot unwritten.
    #[test]
    fn the_pattern_clock_releases_when_the_transport_stops() {
        let events = vec![SeqEvent {
            sample: 0,
            rank: 2,
            pitch: 60,
            vel: 100,
            param: 0,
            value: 0.0,
            restore: false,
            node: 0,
            prob: 1.0,
            cond: (0, 0),
            trig_key: 0,
        }];
        let mut clock = PatternClock::new(0, 24_000.0);
        let mut probe = Probe::default();
        let mut out = [0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, out.len());
        let stopped = ProcessCtx {
            playing: false,
            ..ctx(NO_INPUT)
        };
        clock.run(&mut probe, &events, &mut out, &stopped, &mut ramp);
        assert_eq!(probe.log, ["release"]);
        assert_eq!(probe.rendered, 256);
    }

    /// A clip shorter than the block wraps repeatedly inside one segment,
    /// flushing the cycle's owed note-offs at every wrap — the hanging
    /// note the contract forbids, for an instrument the walk never saw.
    #[test]
    fn a_short_clip_wraps_and_flushes_within_one_segment() {
        let events = vec![
            SeqEvent {
                sample: 0,
                rank: 2,
                pitch: 60,
                vel: 100,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob: 1.0,
                cond: (0, 0),
                trig_key: 0,
            },
            SeqEvent {
                sample: 63,
                rank: 0,
                pitch: 60,
                vel: 0,
                param: 0,
                value: 0.0,
                restore: false,
                node: 0,
                prob: 1.0,
                cond: (0, 0),
                trig_key: 0,
            },
        ];
        // 64-sample clip inside a 256-sample segment: four cycles.
        let mut clock = PatternClock::new(64, 24_000.0);
        let mut probe = Probe::default();
        let mut out = [0.0f32; 256];
        let mut ramp = Ramp::across(1.0, 1.0, out.len());
        clock.run(&mut probe, &events, &mut out, &ctx(NO_INPUT), &mut ramp);

        assert_eq!(probe.rendered, 256);
        let ons = probe.log.iter().filter(|l| l.starts_with("on ")).count();
        let offs = probe.log.iter().filter(|l| l.starts_with("off ")).count();
        assert_eq!(ons, 4, "four cycles, four note-ons: {:?}", probe.log);
        assert_eq!(offs, ons, "every note-on is matched: {:?}", probe.log);
    }
    // ------------------------------------------------- the poly synth ---

    use crate::audio::poly::{PolyParams, PolyVoices};
    use crate::params::poly as pp;

    /// A poly synth playing one held note, compiled and run for one block.
    fn poly_graph(params: PolyParams, pitches: &[u8]) -> (Schedule, NodeId) {
        let mut spec = GraphSpec::default();
        let id = spec.push(NodeSpec::Poly {
            notes: pitches
                .iter()
                .map(|p| Note {
                    start_beats: 0.0,
                    len_beats: 1.0,
                    pitch: *p,
                    vel: 127,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                })
                .collect(),
            subloops: vec![],
            loop_len_beats: None,
            params,
        });
        spec.set_output(id);
        (spec.compile(48_000, 256).unwrap(), id)
    }

    /// It makes a sound. The first thing to know about an instrument, and
    /// the one no unit test of a kernel can tell you.
    #[test]
    fn the_poly_synth_sounds_a_note() {
        let (mut sched, _) = poly_graph(PolyParams::default(), &[69]);
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        let level = rms(&out);
        assert!(level > 1e-3, "poly synth was silent (rms {level})");
        assert!(
            out.iter().all(|s| s.is_finite()),
            "poly synth went non-finite"
        );
        assert!(
            out.iter().all(|s| s.abs() <= 1.5),
            "poly synth clipped hard"
        );
    }

    /// It is STEREO, and silence stays silent — the two halves of "the
    /// node fills every sample of every channel it claims".
    #[test]
    fn the_poly_synth_fills_both_channels() {
        let (mut sched, _) = poly_graph(PolyParams::default(), &[69]);
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        let (l, r) = out.split_at(256);
        assert!(rms(l) > 1e-3, "left silent");
        assert!(rms(r) > 1e-3, "right silent");

        // No notes at all: every sample of both channels is exactly zero.
        let (mut sched, _) = poly_graph(PolyParams::default(), &[]);
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        assert!(out.iter().all(|s| *s == 0.0), "an empty pattern made sound");
    }

    /// A chord sounds as a chord — the same regression the Seq synth
    /// carries, re-asserted for an allocator that now hands out unison
    /// stacks as well as single voices.
    #[test]
    fn poly_simultaneous_notes_get_their_own_voices() {
        let one = {
            let (mut s, _) = poly_graph(PolyParams::default(), &[60]);
            let mut o = vec![0.0f32; 512];
            run(&mut s, &mut o);
            rms(&o[..256])
        };
        let three = {
            let (mut s, _) = poly_graph(PolyParams::default(), &[60, 64, 67]);
            let mut o = vec![0.0f32; 512];
            run(&mut s, &mut o);
            rms(&o[..256])
        };
        assert!(three > one * 1.4, "triad {three} vs single {one}");
    }

    /// EVERY row of the table reaches the instrument and is clamped by it.
    ///
    /// The wire contract, end to end: the widget emits ids from
    /// `params::poly`, the node clamps against the same rows, and the
    /// instrument stores what came through. An id the table knows that the
    /// node drops is a knob that silently does nothing.
    #[test]
    fn every_poly_param_reaches_the_instrument() {
        let (mut sched, id) = poly_graph(PolyParams::default(), &[69]);
        for def in pp::TABLE {
            // Deliberately out of range on both sides: the node must clamp
            // rather than store nonsense or drop the letter.
            for raw in [def.min - 1_000.0, def.max + 1_000.0, f32::NAN] {
                sched.apply(ParamChange {
                    node: id.to_bits(),
                    param: def.id,
                    value: raw,
                });
            }
            let mut out = vec![0.0f32; 512];
            run(&mut sched, &mut out);
            assert!(
                out.iter().all(|s| s.is_finite()),
                "{} at its extremes made the synth non-finite",
                def.name
            );
        }
    }

    /// A ParamChange letter actually changes the sound. Gain to zero is
    /// the one every device must honour.
    #[test]
    fn poly_gain_letters_are_heard() {
        let (mut sched, id) = poly_graph(PolyParams::default(), &[69]);
        let mut loud = vec![0.0f32; 512];
        run(&mut sched, &mut loud);
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: pp::GAIN,
            value: 0.0,
        });
        // Two blocks: the first ramps down, the second is silent.
        let mut fading = vec![0.0f32; 512];
        run(&mut sched, &mut fading);
        let mut quiet = vec![0.0f32; 512];
        run(&mut sched, &mut quiet);
        assert!(rms(&loud) > 1e-3);
        assert!(rms(&quiet) < 1e-5, "gain 0 still sounded: {}", rms(&quiet));
    }

    /// Contract rule 2: a discontinuity cuts every sounding voice. The
    /// poly synth inherits this from the shared clock, so the test is
    /// really asking whether the inheritance works.
    #[test]
    fn a_discontinuity_cuts_the_poly_synth() {
        let (mut sched, _) = poly_graph(PolyParams::default(), &[69]);
        let mut out = vec![0.0f32; 512];
        run(&mut sched, &mut out);
        assert!(rms(&out) > 1e-3);

        // A seek: the note is behind the cursor, so nothing re-sounds.
        let seek = ProcessCtx {
            position: 48_000,
            beat: 2.0,
            discontinuity: true,
            ..ctx(NO_INPUT)
        };
        let mut after = vec![0.0f32; 512];
        sched.run(&mut after, &seek);
        assert!(
            rms(&after) < 1e-5,
            "the seek left a hanging voice: {}",
            rms(&after)
        );
    }

    /// Unison takes more voices and spreads them: eight-voice unison with
    /// full spread must decorrelate the channels, where a single voice
    /// sits dead centre.
    #[test]
    fn unison_spread_widens_the_image() {
        let width = |unison: f32, spread: f32| {
            let params = PolyParams {
                unison,
                spread,
                detune: 25.0,
                ..PolyParams::default()
            };
            let (mut s, _) = poly_graph(params, &[60]);
            let mut o = vec![0.0f32; 512];
            // Several blocks: a unison stack widens from its spread start
            // phases AND from the detune beating the voices apart, and the
            // second of those needs more than one 5 ms block to show.
            for _ in 0..8 {
                run(&mut s, &mut o);
            }
            let (l, r) = o.split_at(256);
            // Side energy against mid energy.
            let side: Vec<f32> = l.iter().zip(r).map(|(a, b)| a - b).collect();
            let mid: Vec<f32> = l.iter().zip(r).map(|(a, b)| a + b).collect();
            rms(&side) / rms(&mid).max(1e-9)
        };
        let mono = width(0.0, 0.0);
        let wide = width(7.0, 100.0);
        assert!(mono < 1e-4, "a single voice was not centred: {mono}");
        assert!(wide > 0.05, "unison spread did not widen: {wide}");
    }

    /// The filter envelope is AUDIBLE, which is only possible because the
    /// lane filters take a cutoff per lane. With a low cutoff and a full
    /// positive envelope the note is brighter than with none.
    #[test]
    fn the_filter_envelope_opens_the_filter() {
        let brightness = |env: f32| {
            let params = PolyParams {
                cutoff: 200.0,
                filter_env: env,
                amp_d: 400.0,
                ..PolyParams::default()
            };
            let (mut s, _) = poly_graph(params, &[48]);
            let mut o = vec![0.0f32; 512];
            run(&mut s, &mut o);
            rms(&o[..256])
        };
        let closed = brightness(0.0);
        let open = brightness(100.0);
        assert!(
            open > closed * 1.2,
            "filter env inaudible: {open} vs {closed}"
        );
    }

    /// The instrument allocates nothing once it is built — the rule that
    /// outranks everything else, checked on the real render path.
    #[test]
    fn the_poly_synth_does_not_allocate_while_rendering() {
        let mut voices = PolyVoices::new(48_000.0, 256, PolyParams::default());
        // Wires live, both directions asked for (one gets dropped), and a
        // chunk-rate destination too: the matrix is the newest red-zone
        // path and the one this test most needs to walk.
        for (row, (src, dst, amt)) in [
            (pp::SRC_OSC_B, pp::DST_A_PHASE, 60.0f32),
            (pp::SRC_FENV, pp::DST_CUTOFF, 50.0),
            (pp::SRC_VEL, pp::DST_PAN, 30.0),
        ]
        .into_iter()
        .enumerate()
        {
            let (s_id, d_id, a_id) = pp::WIRE_IDS[row];
            voices.set_param(s_id, src as f32);
            voices.set_param(d_id, dst as f32);
            voices.set_param(a_id, amt);
        }
        let mut out = vec![0.0f32; 256];
        voices.note_on(69, 100, 0);
        assert_no_alloc::assert_no_alloc(|| {
            let mut ramp = Ramp::across(1.0, 1.0, out.len());
            voices.render(&mut out, 0, &mut ramp);
            voices.note_off(69);
            let mut ramp = Ramp::across(1.0, 1.0, out.len());
            voices.render(&mut out, 0, &mut ramp);
            voices.all_sound_off();
        });
    }

    #[test]
    fn a_trigless_lock_glide_does_not_allocate_while_rendering() {
        let mut spec = GraphSpec::default();
        let id = spec.push(NodeSpec::Poly {
            notes: vec![Note {
                start_beats: 0.0,
                // At 120 bpm / 48 kHz this ends at sample 24, leaving the
                // complete 144-sample (3 ms) return inside this block.
                len_beats: 0.001,
                pitch: 60,
                vel: 0,
                plocks: vec![(pp::F_CUTOFF, 150.0)],
                fx_locks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: PolyParams::default(),
        });
        spec.set_output(id);
        let mut sched = spec.compile(48_000, 256).unwrap();
        let mut out = vec![0.0f32; 512];

        assert_no_alloc::assert_no_alloc(|| {
            sched.run(&mut out, &ctx(NO_INPUT));
        });
        assert!(
            out.iter().all(|sample| *sample == 0.0),
            "a trigless lock must not gate a voice"
        );
    }

    /// Stealing is age-based and never leaves a voice gated to nothing:
    /// more notes than voices must still end up silent after their offs.
    #[test]
    fn over_subscribing_the_polyphony_still_settles_to_silence() {
        let mut voices = PolyVoices::new(48_000.0, 256, PolyParams::default());
        let mut out = vec![0.0f32; 256];
        for i in 0..40u8 {
            voices.note_on(40 + i, 100, u64::from(i));
        }
        for i in 0..40u8 {
            voices.note_off(40 + i);
        }
        // Long enough for every release to finish.
        for _ in 0..400 {
            let mut ramp = Ramp::across(1.0, 1.0, out.len());
            voices.render(&mut out, 0, &mut ramp);
        }
        let level = rms(&out);
        assert!(level < 1e-5, "voices hung after their note-offs: {level}");
    }
    /// One wire per test would be a manual; this is the matrix's own
    /// contract: a live wire CHANGES the sound, an "off" wire does not,
    /// and nothing a wire can say makes the synth non-finite.
    #[test]
    fn matrix_wires_are_audible_and_safe() {
        let base = |wires: [[f32; 3]; 3]| {
            let params = PolyParams {
                osc: [
                    crate::audio::poly::OscParams {
                        level: 100.0,
                        ..PolyParams::default().osc[0]
                    },
                    crate::audio::poly::OscParams {
                        level: 0.0,
                        semi: 7.0,
                        ..PolyParams::default().osc[1]
                    },
                ],
                wires,
                ..PolyParams::default()
            };
            let (mut s, _) = poly_graph(params, &[57]);
            let mut o = vec![0.0f32; 512];
            for _ in 0..4 {
                run(&mut s, &mut o);
            }
            o
        };
        let none = [[0.0; 3]; 3];
        let dry = base(none);

        // B -> A phase: FM from a silent modulator — the classic patch,
        // and the proof the RAW (pre-level) output is the source.
        let mut fm = none;
        fm[0] = [pp::SRC_OSC_B as f32, pp::DST_A_PHASE as f32, 80.0];
        let wet = base(fm);
        assert_ne!(dry, wet, "a phase wire changed nothing");
        assert!(wet.iter().all(|s| s.is_finite()));

        // Amp env -> a level: no change in WHETHER it sounds, a change in
        // HOW it moves.
        let mut am = none;
        am[1] = [pp::SRC_AMP as f32, pp::DST_A_LEVEL as f32, -60.0];
        let wet = base(am);
        assert_ne!(dry, wet, "a level wire changed nothing");

        // A depth of zero IS an off wire, whatever src/dst say.
        let mut idle = none;
        idle[2] = [pp::SRC_OSC_B as f32, pp::DST_A_PHASE as f32, 0.0];
        assert_eq!(dry, base(idle), "a zero-depth wire made sound");

        // Both cross-osc directions at once: the A->B direction wins,
        // the render stays finite, and something still sounds.
        let mut both = none;
        both[0] = [pp::SRC_OSC_B as f32, pp::DST_A_PHASE as f32, 90.0];
        both[1] = [pp::SRC_OSC_A as f32, pp::DST_B_PHASE as f32, 90.0];
        let wet = base(both);
        assert!(wet.iter().all(|s| s.is_finite()));
        assert!(rms(&wet[..256]) > 1e-4, "the tie silenced the synth");
    }

    /// A chunk-rate wire moves a coefficient: filter env -> cutoff on a
    /// nearly-closed filter is audibly brighter than no wire, through the
    /// MATRIX rather than the dedicated env-amount knob.
    #[test]
    fn a_cutoff_wire_opens_the_filter() {
        let brightness = |depth: f32| {
            let mut wires = [[0.0; 3]; 3];
            wires[0] = [pp::SRC_FENV as f32, pp::DST_CUTOFF as f32, depth];
            let params = PolyParams {
                cutoff: 150.0,
                fenv_d: 400.0,
                wires,
                ..PolyParams::default()
            };
            let (mut s, _) = poly_graph(params, &[48]);
            let mut o = vec![0.0f32; 512];
            run(&mut s, &mut o);
            rms(&o[..256])
        };
        let (closed, open) = (brightness(0.0), brightness(100.0));
        assert!(
            open > closed * 1.2,
            "cutoff wire inaudible: {open} vs {closed}"
        );
    }

    /// The filter envelope has its OWN times now. Stretching its decay
    /// changes the sound; the amp envelope's length does not move.
    #[test]
    fn the_filter_envelope_times_are_its_own() {
        let render = |fenv_d: f32| {
            let params = PolyParams {
                cutoff: 200.0,
                filter_env: 100.0,
                fenv_d,
                amp_d: 150.0,
                amp_s: 0.0,
                wires: [[0.0; 3]; 3],
                ..PolyParams::default()
            };
            let (mut s, _) = poly_graph(params, &[48]);
            // Far enough in that a 30 ms contour has closed and an 800 ms
            // one is still open — the window where the difference lives.
            let mut o = vec![0.0f32; 512];
            for _ in 0..8 {
                run(&mut s, &mut o);
            }
            o
        };
        let short = render(30.0);
        let long = render(800.0);
        assert_ne!(short, long, "fenv decay is still borrowed from the amp");
    }

    /// The kick recipe, end to end: sine osc, the pitch envelope's own
    /// decay row sweeping +48 semitones down. Early cycles are FAST,
    /// late cycles are slow — measured as zero crossings, not asserted
    /// from the wiring.
    #[test]
    fn the_kick_recipe_drops_in_pitch() {
        let params = PolyParams {
            osc: [
                crate::audio::poly::OscParams {
                    wave: 0.0,
                    pitch_env: 48.0,
                    ..PolyParams::default().osc[0]
                },
                crate::audio::poly::OscParams {
                    level: 0.0,
                    ..PolyParams::default().osc[1]
                },
            ],
            penv_d: 50.0,
            amp_d: 300.0,
            amp_s: 0.0,
            wires: [[0.0; 3]; 3],
            ..PolyParams::default()
        };
        let (mut sched, _) = poly_graph(params, &[36]);
        let mut all = Vec::new();
        for _ in 0..16 {
            let mut o = vec![0.0f32; 512];
            run(&mut sched, &mut o);
            all.extend_from_slice(&o[..256]);
        }
        let zcr = |w: &[f32]| {
            w.windows(2)
                .filter(|p| (p[0] < 0.0) != (p[1] < 0.0))
                .count() as f32
                * 48_000.0
                / (2.0 * w.len() as f32)
        };
        let early = zcr(&all[..480]);
        let late = zcr(&all[2880..3840]);
        assert!(
            early > late * 2.0,
            "no pitch drop: {early:.0} Hz early vs {late:.0} Hz late"
        );
        assert!(all.iter().all(|s| s.is_finite()));
    }
    /// The transport-loop contract the app leans on since it stopped
    /// compiling patterns in clip mode: a ONE-SHOT pattern under a
    /// wrapping transport re-sounds on every pass — the wrap is a
    /// discontinuity, which cuts the voices and reseeks the cursor — and
    /// a playhead parked PAST the material is exact silence, loop brace
    /// or no loop brace.
    ///
    /// The second half is the reported bug: with the pattern compiled in
    /// clip mode (`loop_len_beats` set), its phase was
    /// `position % loop_samples`, so material inside the brace replayed
    /// forever wherever the playhead actually was. One shared
    /// `PatternClock` walk, so this pins `Seq` too.
    #[test]
    fn a_one_shot_pattern_wraps_with_the_transport_and_is_silent_beyond_it() {
        let (mut sched, _) = poly_graph(PolyParams::default(), &[60]);
        let bps = 120.0 / 60.0 / 48_000.0;
        let at = |position: u64, discontinuity: bool| ProcessCtx {
            position,
            beat: position as f64 * bps,
            discontinuity,
            ..ctx(NO_INPUT)
        };
        let level = |sched: &mut Schedule, c: &ProcessCtx<'_>| {
            let mut out = vec![0.0f32; 512];
            sched.run(&mut out, c);
            rms(&out[..256])
        };

        // First pass: the note at beat 0 sounds (first play is itself a
        // discontinuity).
        assert!(level(&mut sched, &at(0, true)) > 1e-4, "first pass silent");
        // The transport wraps to the loop start: position jumps back with
        // the discontinuity flag, and the note fires AGAIN.
        assert!(level(&mut sched, &at(256, false)) > 1e-4);
        assert!(
            level(&mut sched, &at(0, true)) > 1e-4,
            "the wrap did not re-sound the pattern"
        );

        // Park the playhead sixteen beats out — far past the one-beat
        // note — and roll. EXACT silence, every sample: nothing wraps the
        // pattern back into range any more.
        let far = (16.0 / bps) as u64;
        let mut out = vec![0.0f32; 512];
        sched.run(&mut out, &at(far, true));
        assert!(
            out.iter().all(|s| *s == 0.0),
            "the pattern sounded past its own material"
        );
        for block in 1..4u64 {
            let mut out = vec![0.0f32; 512];
            sched.run(&mut out, &at(far + block * 256, false));
            assert!(out.iter().all(|s| *s == 0.0), "block {block} not silent");
        }
    }
    /// Parameter locks, end to end on the poly synth: a locked note-on
    /// OVERRIDES the knob, the next unlocked note-on RESUMES it — and
    /// "the knob" is live, so a letter turned mid-pattern is what an
    /// unlocked note comes back to.
    #[test]
    fn plocks_override_and_unlocked_notes_resume_the_knob() {
        let mut spec = GraphSpec::default();
        let dark = vec![(pp::F_CUTOFF, 150.0f32)];
        // A SAW, not the default sine: a 130 Hz sine barely notices a
        // 150 Hz lowpass, and this test's whole measurement is that the
        // cutoff moves the level.
        let params = PolyParams {
            osc: [
                crate::audio::poly::OscParams {
                    wave: 2.0,
                    ..PolyParams::default().osc[0]
                },
                PolyParams::default().osc[1],
            ],
            ..PolyParams::default()
        };
        let id = spec.push(NodeSpec::Poly {
            notes: vec![
                Note {
                    start_beats: 0.0,
                    len_beats: 0.9,
                    pitch: 48,
                    vel: 110,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                },
                Note {
                    start_beats: 1.0,
                    len_beats: 0.9,
                    pitch: 48,
                    vel: 110,
                    plocks: dark.clone(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                },
                Note {
                    start_beats: 2.0,
                    len_beats: 0.9,
                    pitch: 48,
                    vel: 110,
                    plocks: Vec::new(),
                    fx_locks: Vec::new(),
                    prob: 1.0,
                    cond: None,
                },
            ],
            subloops: vec![],
            loop_len_beats: None,
            params,
        });
        spec.set_output(id);
        let mut sched = spec.compile(48_000, 256).unwrap();

        // One beat at 120 bpm is 24_000 samples = ~94 blocks of 256.
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut note_rms = [0.0f32; 3];
        for block in 0..282u64 {
            let position = block * 256;
            let c = ProcessCtx {
                position,
                beat: position as f64 * bps,
                discontinuity: block == 0,
                ..ctx(NO_INPUT)
            };
            let mut out = vec![0.0f32; 512];
            sched.run(&mut out, &c);
            let note = (position / 24_000).min(2) as usize;
            note_rms[note] += rms(&out[..256]);
        }
        let [bright, locked, resumed] = note_rms;
        assert!(
            locked < bright * 0.6,
            "the cutoff lock did not darken its note: {locked} vs {bright}"
        );
        assert!(
            resumed > locked * 1.5,
            "the unlocked note did not resume the knob: {resumed} vs {locked}"
        );

        // The knob moved mid-life: darken the BASE by letter, recompile
        // nothing — the same unlocked notes must now resume the letter's
        // value, because the base is live, not a compile snapshot.
        sched.apply(ParamChange {
            node: id.to_bits(),
            param: pp::F_CUTOFF,
            value: 150.0,
        });
        let mut out = vec![0.0f32; 512];
        let seek = ProcessCtx {
            position: 0,
            beat: 0.0,
            discontinuity: true,
            ..ctx(NO_INPUT)
        };
        sched.run(&mut out, &seek);
        let darkened_base = rms(&out[..256]);
        assert!(
            darkened_base < bright * 0.6,
            "the letter did not move the restore base: {darkened_base} vs first pass {bright}"
        );
    }

    /// A trigless lock is a parameter gesture, never a silent MIDI note.
    /// It takes effect at the selected cell and explicitly returns to the
    /// live knob at the far edge of that cell.
    #[test]
    fn a_trigless_lock_compiles_to_a_bounded_lock_without_note_events() {
        let notes = vec![Note {
            start_beats: 0.25,
            len_beats: 0.5,
            pitch: 0,
            vel: 0,
            plocks: vec![(pp::F_CUTOFF, 321.0)],
            fx_locks: vec![(77, 9, 0.75)],
            prob: 1.0,
            cond: None,
        }];
        let Ok(events) = compile_events(&notes, &[], Some(1.0), 1_000.0, 3) else {
            panic!("events did not compile");
        };

        assert_eq!(events.len(), 10, "two locks plus four-point returns");
        assert!(events.iter().all(|event| event.rank == 1));
        assert_eq!(
            events
                .iter()
                .filter(|event| !event.restore)
                .map(|event| (event.sample, event.node, event.param))
                .collect::<Vec<_>>(),
            [(250, 0, pp::F_CUTOFF), (250, 77, 9)]
        );
        for (node, param) in [(0, pp::F_CUTOFF), (77, 9)] {
            let restore: Vec<_> = events
                .iter()
                .filter(|event| event.restore && event.node == node && event.param == param)
                .map(|event| (event.sample, event.value))
                .collect();
            assert_eq!(restore.len(), 4);
            assert_eq!(restore.first().map(|event| event.0), Some(750));
            assert_eq!(restore.last().map(|event| event.0), Some(753));
            assert_eq!(restore.last().map(|event| event.1), Some(1.0));
        }
    }

    /// Trig conditions, on the shared clock: an A:B note fires only on
    /// pass A of every B cycles, its OFF stands down with it (no orphan
    /// off releasing someone else's voice), and probability is
    /// DETERMINISTIC — the same take twice, varied across cycles.
    #[test]
    fn trig_conditions_gate_notes_per_cycle() {
        let mk = |cond: Option<(u8, u8)>, prob: f32| {
            let notes = vec![Note {
                start_beats: 0.0,
                len_beats: 0.5,
                pitch: 60,
                vel: 100,
                plocks: Vec::new(),
                fx_locks: Vec::new(),
                prob,
                cond,
            }];
            compile_events(&notes, &[], Some(1.0), 1_000.0, 3).unwrap()
        };
        let run_cycles = |events: &[SeqEvent], cycles: u64| -> Vec<String> {
            let mut clock = PatternClock::new(1_000, 1_000.0);
            let mut probe = Probe::default();
            for block in 0..(cycles * 4) {
                let position = block * 250;
                let c = ProcessCtx {
                    position,
                    beat: position as f64 / 1_000.0,
                    discontinuity: block == 0,
                    ..ctx(NO_INPUT)
                };
                let mut out = [0.0f32; 250];
                let mut ramp = Ramp::across(1.0, 1.0, out.len());
                clock.run(&mut probe, events, &mut out, &c, &mut ramp);
            }
            probe.log
        };

        // 1:2 fires on even cycles, 2:2 on odd — together they tile.
        let first = run_cycles(&mk(Some((1, 2)), 1.0), 4);
        let ons = |log: &[String]| log.iter().filter(|l| l.starts_with("on ")).count();
        let offs = |log: &[String]| log.iter().filter(|l| l.starts_with("off ")).count();
        assert_eq!(ons(&first), 2, "1:2 over four cycles: {first:?}");
        assert_eq!(offs(&first), ons(&first), "an off fired without its on");
        let second = run_cycles(&mk(Some((2, 2)), 1.0), 4);
        assert_eq!(ons(&second), 2, "2:2 over four cycles: {second:?}");
        let last_of_four = run_cycles(&mk(Some((4, 4)), 1.0), 8);
        assert_eq!(ons(&last_of_four), 2, "4:4 over eight cycles");

        // Probability: 0 never, 1 always, and one-half is DETERMINISTIC —
        // two runs agree exactly, and across sixteen cycles it neither
        // always fires nor never does.
        assert_eq!(ons(&run_cycles(&mk(None, 0.0), 8)), 0);
        assert_eq!(ons(&run_cycles(&mk(None, 1.0), 8)), 8);
        let a = run_cycles(&mk(None, 0.5), 16);
        let b = run_cycles(&mk(None, 0.5), 16);
        assert_eq!(a, b, "probability must reproduce — bounces depend on it");
        let n = ons(&a);
        assert!((1..16).contains(&n), "half-probability fired {n}/16");
        assert_eq!(offs(&a), n, "every fired note kept its off");
    }
}
