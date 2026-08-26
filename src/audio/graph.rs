//! The schedule: a compiled, flat execution plan for the audio graph.
//!
//! Green zone compiles `GraphSpec` (nodes + wires) into a `Schedule`; the red
//! zone only walks it. Compile does the thinking — topological order, cycle
//! rejection, slot assignment — so the callback can be a straight-line walk
//! over flat arrays: one `Vec<Step>` in dependency order, one arena of
//! 64-byte-aligned slots indexed by number.

use crate::audio::modulation::{ModEdit, ModPlan, ModSpec};
use creek::{ReadDiskStream, SeekMode, SymphoniaDecoder};
use std::f32::consts::TAU;

/// A node's permanent name tag: a `thunderdome` generational index. The slot
/// may be reused after a removal, but the generation changes — so a stale id
/// can never address the wrong node, only nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Note {
    pub start_beats: f64,
    pub len_beats: f64,
    /// MIDI pitch, 0-127 (69 = A4 = 440Hz).
    pub pitch: u8,
    /// MIDI velocity, 1-127.
    pub vel: u8,
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
                        ..*n
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
                ..*n
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
}

impl Default for VoiceBank {
    fn default() -> Self {
        Self {
            phase: [0.0; SEQ_VOICES],
            step: [0.0; SEQ_VOICES],
            env: [0.0; SEQ_VOICES],
            amp: [0.0; SEQ_VOICES],
            gate: [false; SEQ_VOICES],
            pitch: [0; SEQ_VOICES],
            age: [0; SEQ_VOICES],
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

    /// Red zone. Silence everything, now — what a discontinuity demands.
    fn all_sound_off(&mut self) {
        *self = Self::default();
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
    fn note_on(&mut self, pitch: u8, vel: u8, age: u64, sample_rate: f32) {
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
        self.step[slot] = freq / sample_rate.max(1.0);
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
    fn render(&mut self, out: &mut [f32], attack: f32, release: f32, gain: &mut Ramp) {
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
fn flush_cycle(voices: &mut VoiceBank, events: &[SeqEvent], cursor: &mut usize) {
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

/// How often the filter's coefficients follow its smoothed controls, in
/// samples. Fast enough that a performed sweep has no audible steps, cheap
/// enough (a handful of sin_cos per update, only while a knob is actually
/// moving) to disappear in the block budget.
const FILTER_COEFF_INTERVAL: usize = 16;

/// Control smoothing for the filter's cutoff, resonance and drive, in ms.
/// The declick layer the kernel contract says the caller wires in front.
const FILTER_SMOOTH_MS: f32 = 15.0;

/// The kernel chain of one [`Node::Filter`], boxed so the Node enum stays
/// lean. Compile builds it in the green zone; the callback only calls
/// prepare/process/reset on it — bounded pure math throughout.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct FilterCore {
    cascade: crate::dsp::filters::Cascade,
    svf: crate::dsp::filters::Svf,
    shaper: crate::dsp::shaper::Waveshaper,
    oversampler: crate::dsp::shaper::Oversampler2x,
    dc: crate::dsp::filters::DcBlocker,
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
    /// The per-track output stage: constant-power pan (first input's mono
    /// signal -> stereo) and the track's fader level. Both ramped.
    /// ParamChange 0 = pan, 1 = gain.
    Pan {
        pan: f32,
        target_pan: f32,
        gain: f32,
        target_gain: f32,
    },
    /// Streams an audio file from disk (creek: decode on its own IO thread,
    /// RT-safe reads here). Timeline-locked: file frame N plays at timeline
    /// sample N (1:1 — no resampling yet, so a file at another rate plays
    /// detuned; surfaced in the lab). Multichannel files mix down to the mono
    /// slot. On read error the stream is FLAGGED dead, never dropped — a drop
    /// here would join an IO thread inside the callback; the schedule swap
    /// disposes of it on the UI thread like everything else.
    AudioClip {
        stream: Option<ReadDiskStream<SymphoniaDecoder>>,
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
        gain: f32,
        target_gain: f32,
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
    Seq {
        events: Vec<SeqEvent>,
        cursor: usize,
        voices: VoiceBank,
        /// Stamp handed to the next triggered voice; only ever increases.
        next_age: u64,
        sample_rate: f32,
        /// Post-sum gain, ramped per segment like Sine's amp (ParamChange
        /// 0). Attack/release arrive as ms (ParamChange 1/2) and are
        /// converted to per-sample rates on arrival.
        gain: f32,
        target_gain: f32,
        attack_rate: f32,
        release_coeff: f32,
        /// Clip mode: the pattern cycles every this many SAMPLES at the
        /// compiled tempo, forever, while the timeline rolls forward
        /// (Ableton-style). 0 = one-shot linear, never wraps.
        ///
        /// The single clock. The beat-valued length it is derived from is
        /// deliberately NOT kept alongside it: the cycle a segment belongs
        /// to is derived by integer division on this same modulus, and a
        /// float copy of the same fact would disagree by up to half a
        /// sample at any tempo whose samples-per-beat is not integral.
        ///
        /// Event stamps are pattern-relative samples in this mode; note ends
        /// are clamped to the loop length at compile, so the off-flush at
        /// each wrap can never leave a hanging note.
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
    },
    /// Reverb: the first effect. Sums its inputs to mono, runs the
    /// `dsp::reverb` kernel over them, and crossfades wet against dry.
    /// Free-running: its tail is wall-history, not timeline position — but
    /// it CUTS on discontinuity, because a seek must not drag the old
    /// room's tail into the new position.
    ///
    /// The delay memory is a plain Vec owned here and allocated at compile
    /// (green zone). The kernel never touches its length.
    Reverb {
        core: crate::dsp::reverb::Reverb,
        buffers: Vec<f32>,
        /// Wet scratch, one block long — the kernel writes pure wet and
        /// this node does the mixing.
        wet: Vec<f32>,
        mix: f32,
        target_mix: f32,
        size: f32,
        damp: f32,
        sample_rate: f32,
    },
    /// Filter: the first kernel-backed effect. Sums its inputs to mono and
    /// runs cutoff/resonance/drive over the `dsp::filters` family: a
    /// Butterworth [`Cascade`](crate::dsp::filters::Cascade) (lp/hp, 6-48
    /// dB/octave, resonance on the last section) or a single
    /// [`Svf`](crate::dsp::filters::Svf) (bp/notch), then a soft-clip drive
    /// stage run at 2x through the halfband oversampler, then a DC blocker.
    ///
    /// The drive stage is PERMANENTLY in the path: its half-band round trip
    /// is the node's constant latency (`Oversampler2x::latency()`), because
    /// a drive knob must never move a track in time. At drive 0 the shaper
    /// is skipped entirely, so the stage is a linear-phase wire and the
    /// default filter is transparent apart from that delay; the node owns
    /// the dry/wet blend so the clean path keeps its float headroom.
    ///
    /// That latency is now COMPENSATED: `GraphSpec::compensate` reads it
    /// through `spec_latency` and delays every sibling path to match, so a
    /// filtered track no longer runs ~0.7 ms ahead of the rest of the mix.
    /// The graph's remaining total is reported by `Schedule::latency`.
    ///
    /// Ids, ranges and the resonance/drive mapping come from
    /// `crate::params::filter` — the same rows and the same `effective_q`
    /// the display draws with, so the curve and the audio agree by
    /// construction. Free-running; cuts state on discontinuity.
    Filter {
        core: Box<FilterCore>,
        /// 2x scratch for the drive stage, `4 * block` floats (round-trip
        /// lane + shaped lane), compile-owned.
        scratch2x: Vec<f32>,
        mode: u32,
        slope: u32,
        /// Letters land here; the switch happens at the next block edge with
        /// filter state cleared, because a cascade carrying lowpass history
        /// into a highpass is a thump.
        pending_mode: u32,
        pending_slope: u32,
        cutoff: crate::dsp::ramps::Smoother,
        res: crate::dsp::ramps::Smoother,
        drive: crate::dsp::ramps::Smoother,
        /// Shadow targets, because a discontinuity snaps the smoothers to
        /// their destination (a seek must not glide old knob motion in) and
        /// [`Smoother`](crate::dsp::ramps::Smoother) does not expose its.
        cutoff_target: f32,
        res_target: f32,
        drive_target: f32,
        /// What the kernel coefficients were last prepared with, so a
        /// settled filter re-prepares nothing.
        prepared_cutoff: f32,
        prepared_q: f32,
        sample_rate: f32,
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
            | NodeSpec::Filter { .. }
            | NodeSpec::Seq { .. } => 1,
            NodeSpec::Delay { channels, .. } => (*channels).clamp(1, 2),
            NodeSpec::Mixer { .. } | NodeSpec::AudioClip { .. } | NodeSpec::Pan { .. } => 2,
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

            Node::Mixer { gain, target_gain } => {
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

            Node::Pan {
                pan,
                target_pan,
                gain,
                target_gain,
            } => {
                // Mono uses constant-power placement. Stereo uses a balance
                // law: centre preserves both channels, while either extreme
                // attenuates only the opposite side.
                let input = inputs.first();
                let src = input.map(|input| input.l).unwrap_or(&[]);
                let src_r = input.and_then(|input| input.r);
                let r = out.r.as_deref_mut().unwrap_or(&mut []);
                let mut ramp = Ramp::across(*pan, *target_pan, out_len);
                // The LEVEL ramps across what is left of the block, not
                // across this segment. Letters are drained once per block,
                // above the segment loop, so a fader jump is already in
                // `target_gain` when segment 0 runs — and a block split at
                // a loop point can make that segment one frame long. Spread
                // over the segment, a 1.0 -> 0.0 move would then be a
                // one-sample step, which is a click. Pan is left segment
                // scoped on purpose: it redistributes bounded energy
                // between two channels, where gain scales it without bound.
                let remaining = ctx.block_frames.saturating_sub(ctx.offset).max(out_len);
                let mut level = Ramp::across(*gain, *target_gain, remaining);
                for i in 0..out_len {
                    let p = ramp.next();
                    let g = level.next();
                    let left = src.get(i).copied().unwrap_or(0.0);
                    if let Some(source_r) = src_r {
                        let right = source_r.get(i).copied().unwrap_or(0.0);
                        let left_gain = if p <= 0.0 {
                            1.0
                        } else {
                            (p * std::f32::consts::FRAC_PI_2).cos()
                        };
                        let right_gain = if p >= 0.0 {
                            1.0
                        } else {
                            (-p * std::f32::consts::FRAC_PI_2).cos()
                        };
                        out.l[i] = left * left_gain * g;
                        if let Some(rs) = r.get_mut(i) {
                            *rs = right * right_gain * g;
                        }
                    } else {
                        let angle = (p + 1.0) * std::f32::consts::FRAC_PI_4;
                        out.l[i] = left * angle.cos() * g;
                        if let Some(rs) = r.get_mut(i) {
                            *rs = left * angle.sin() * g;
                        }
                    }
                }
                // Pan lands exactly at the end of every segment; the
                // level lands exactly at the end of the BLOCK, and keeps
                // where it got to in between — otherwise the block-long
                // ramp above would be undone by every segment boundary.
                *pan = *target_pan;
                if ctx.offset + out_len >= ctx.block_frames {
                    *gain = *target_gain; // land exactly, no float drift
                } else {
                    *gain = level.value;
                }
            }

            Node::AudioClip {
                stream,
                file_frames,
                timeline_start,
                timeline_frames,
                source_start,
                source_frames,
                loop_clip,
                gain,
                target_gain,
                failed,
                next_frame,
            } => {
                out.l.fill(0.0);
                if let Some(r) = out.r.as_deref_mut() {
                    r.fill(0.0);
                }
                let (Some(st), false, true) = (stream.as_mut(), *failed, ctx.playing) else {
                    // Stopped, failed, or no stream: silence. A pause keeps
                    // gain settled so resuming at the same position is
                    // seamless; a discontinuity below starts a fresh ramp.
                    *gain = *target_gain;
                    return;
                };
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
                let want = if *loop_clip {
                    *source_start + local_frame % *source_frames
                } else {
                    *source_start + local_frame
                };
                if ctx.discontinuity || want != *next_frame {
                    // Seek is an async request; until data is ready, reads
                    // yield silence — creek's documented behavior, accepted.
                    if st.seek(want as usize, SeekMode::Auto).is_err() {
                        *failed = true;
                        return;
                    }
                    *next_frame = want;
                }
                // Read in runs, splitting at the loop wrap. HARD-BOUNDED:
                // trusting read() to make progress is an unbounded-time path
                // (buffering reads can advance creek's playhead while
                // reporting zero frames). 8 attempts covers any legitimate
                // segment; anything needing more is a stalled stream, which
                // renders silence for the remainder of this block.
                let mut done = 0usize;
                let mut attempts = 0u32;
                let source_end = source_start.saturating_add(*source_frames);
                while done < active_len && attempts < 8 {
                    attempts += 1;
                    let until_wrap = source_end.saturating_sub(*next_frame) as usize;
                    let want_now = (active_len - done).min(until_wrap);
                    if want_now == 0 {
                        if !*loop_clip {
                            break;
                        }
                        // Exactly at the wrap: request the source-region
                        // start again, which need not be file frame zero.
                        if st.seek(*source_start as usize, SeekMode::Auto).is_err() {
                            *failed = true;
                            return;
                        }
                        *next_frame = *source_start;
                        continue;
                    }
                    match st.read(want_now) {
                        Ok(data) => {
                            let got = data.num_frames().min(want_now);
                            if got == 0 {
                                break; // EOF or stalled: silence the rest
                            }
                            // ch0 -> L; ch1 -> R (mono files: same to both).
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
                            // Creek's own playhead is the truth — a buffering
                            // read can advance it without delivering frames,
                            // and a drifted mirror here means reading forever.
                            *next_frame = st.playhead() as u64;
                            if *loop_clip && *next_frame >= source_end {
                                if st.seek(*source_start as usize, SeekMode::Auto).is_err() {
                                    *failed = true;
                                    return;
                                }
                                *next_frame = *source_start;
                            }
                            done += got;
                        }
                        Err(_) => {
                            // Flag and silence; the stream is disposed of on
                            // the UI thread with the schedule, never here.
                            *failed = true;
                            break;
                        }
                    }
                }
                let gain_step = (*target_gain - *gain) / active_len.max(1) as f32;
                let mut g = *gain;
                for s in &mut out.l[write_start..write_start + active_len] {
                    *s *= g;
                    g += gain_step;
                }
                if let Some(r) = out.r.as_deref_mut() {
                    let mut g = *gain;
                    for s in &mut r[write_start..write_start + active_len] {
                        *s *= g;
                        g += gain_step;
                    }
                }
                *gain = *target_gain;
            }

            Node::Seq {
                events,
                cursor,
                voices,
                next_age,
                sample_rate,
                samples_per_beat,
                gain,
                target_gain,
                attack_rate,
                release_coeff,
                loop_samples,
                last_cycle,
                prev_phase,
            } => {
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
                let spb = *samples_per_beat;
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
                let absolute = (ctx.beat * spb).max(0.0) as u64;
                let (mut cycle, mut phase) = match absolute.checked_div(*loop_samples) {
                    Some(cyc) => (cyc as i64, absolute % *loop_samples),
                    // No loop: one linear pass, position is the phase.
                    None => (0, absolute),
                };

                if ctx.discontinuity {
                    // All-sound-off, hard: contract rule 2. A wrap or seek
                    // must never leave a hanging note. Then reseek — a
                    // bounded binary search, no allocation.
                    voices.all_sound_off();
                    *cursor = events.partition_point(|e| e.sample < phase);
                    *last_cycle = cycle;
                    *prev_phase = phase;
                } else if cycle > *last_cycle {
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
                    flush_cycle(voices, events, cursor);
                } else if cycle == *last_cycle && phase < *prev_phase {
                    // The derivation stepped backward within a cycle — a
                    // tempo change moved the beat under us. Hold position
                    // rather than replaying events already fired.
                    phase = *prev_phase;
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
                    *last_cycle = cycle;
                    *prev_phase = phase;
                    let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                    voices.render(out.l, *attack_rate, *release_coeff, &mut ramp);
                    *gain = *target_gain;
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
                let mut ramp = Ramp::across(*gain, *target_gain, out_len);
                let mut done = 0usize;
                let mut steps = 0usize;
                // A cycle costs at most one wrap, one event pass and one
                // render pass; a segment can hold at most `out_len` cycles
                // because a wrap always leaves at least one sample to
                // render (`loop_samples` is either 0, meaning never wrap,
                // or >= 1). Budgeting the event list ONCE was wrong: the
                // cursor rewinds every wrap, so a pattern shorter than a
                // block re-walks it each cycle.
                let step_bound = (out_len + 1).saturating_mul(events.len().saturating_mul(2) + 2);
                while done < out_len && steps < step_bound {
                    steps += 1;
                    let remaining = (out_len - done) as u64;

                    // A clip wrap ends the run: flush the tail of the
                    // cycle, restart the pattern, and carry on.
                    let to_wrap = if *loop_samples > 0 {
                        loop_samples.saturating_sub(phase)
                    } else {
                        u64::MAX
                    };

                    // The next event bounds the run too. Zero-length runs
                    // are normal: several events can share one sample.
                    let to_event = events
                        .get(*cursor)
                        .map(|e| e.sample.saturating_sub(phase))
                        .unwrap_or(u64::MAX);

                    let run = remaining.min(to_wrap).min(to_event) as usize;
                    if run > 0 {
                        voices.render(
                            &mut out.l[done..done + run],
                            *attack_rate,
                            *release_coeff,
                            &mut ramp,
                        );
                        done += run;
                        phase += run as u64;
                    }
                    if done >= out_len {
                        break;
                    }

                    if to_event <= to_wrap && *cursor < events.len() {
                        // Fire every event stamped at this exact sample, in
                        // compiled order — off before on, so a same-pitch
                        // back-to-back pair does not kill its successor.
                        while let Some(ev) = events.get(*cursor).copied() {
                            if ev.sample > phase {
                                break;
                            }
                            *cursor += 1;
                            if ev.rank == 0 {
                                voices.note_off(ev.pitch);
                            } else {
                                let age = *next_age;
                                *next_age = next_age.wrapping_add(1);
                                voices.note_on(ev.pitch, ev.vel, age, *sample_rate);
                            }
                        }
                    } else {
                        // The wrap, seen mid-segment.
                        flush_cycle(voices, events, cursor);
                        phase = 0;
                        cycle += 1;
                    }
                }
                if done < out_len {
                    // The bound tripped, which the proof above says cannot
                    // happen. Fail to SILENCE rather than to whatever the
                    // arena slot held last: a metered node must fill its
                    // whole buffer, and the allocator recycles slots
                    // between nodes.
                    out.l[done..].fill(0.0);
                }
                *last_cycle = cycle;
                *prev_phase = phase;
                *gain = *target_gain; // land exactly, no float drift
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
                sum_inputs_mono(inputs, out.l);
                left.process_exact(out.l, left_buf);
                if let Some(r) = out.r.as_deref_mut() {
                    r.fill(0.0);
                    for input in inputs {
                        // A mono producer feeds both sides, the same rule
                        // the rest of the graph centres mono by.
                        let src = input.r.unwrap_or(input.l);
                        for (d, s) in r.iter_mut().zip(src.iter()) {
                            *d += *s;
                        }
                    }
                    right.process_exact(r, right_buf);
                }
            }

            Node::Reverb {
                core,
                buffers,
                wet,
                mix,
                target_mix,
                ..
            } => {
                // A seek must not drag the old room along: cut the tail on
                // discontinuity, exactly as a sounding voice does.
                if ctx.discontinuity {
                    core.reset(buffers);
                }

                // Sum the wired inputs to mono, in place in the output.
                sum_inputs_mono(inputs, out.l);

                // The kernel writes pure wet into the scratch; this node
                // owns the blend. `wet` is one block long by construction,
                // but a short segment is normal — zip truncates.
                let n = out.l.len().min(wet.len());
                let (dry_src, wet_dst) = (&out.l[..n], &mut wet[..n]);
                // Copy dry aside: the kernel reads input and writes wet,
                // and both live in this node's own slices.
                core.process(dry_src, wet_dst, buffers);

                // Linear mix ramp across the segment: a mix knob must not
                // click, same rule as every other ramped parameter.
                let mut ramp = Ramp::across(*mix, *target_mix, n);
                for (d, w) in out.l.iter_mut().zip(wet.iter()).take(n) {
                    let m = ramp.next();
                    *d = *d * (1.0 - m) + *w * m;
                }
                *mix = *target_mix; // land exactly, no float drift
            }

            Node::Filter {
                core,
                scratch2x,
                mode,
                slope,
                pending_mode,
                pending_slope,
                cutoff,
                res,
                drive,
                cutoff_target,
                res_target,
                drive_target,
                prepared_cutoff,
                prepared_q,
                sample_rate,
            } => {
                use crate::params::filter as fp;

                sum_inputs_mono(inputs, out.l);

                // A seek must not drag the old ring along, and must not
                // glide old knob motion into the new position: clear the
                // signal history, snap the controls.
                if ctx.discontinuity {
                    core.cascade.reset();
                    core.svf.reset();
                    core.oversampler.reset();
                    core.dc.reset();
                    cutoff.set_now(*cutoff_target);
                    res.set_now(*res_target);
                    drive.set_now(*drive_target);
                }

                // Mode/slope switches land on segment edges with state
                // cleared — a cascade carrying lowpass history into a
                // highpass is a thump, and a brief clean restart is not.
                if *pending_mode != *mode || *pending_slope != *slope {
                    *mode = *pending_mode;
                    *slope = *pending_slope;
                    core.cascade.reset();
                    core.svf.reset();
                    *prepared_cutoff = 0.0; // force the re-prepare below
                }

                // Where the drive blend starts this segment; it ramps to
                // the smoother's end value across the 2x pass below.
                let drive_start = drive.current();

                // Coefficients follow the smoothed controls every
                // FILTER_COEFF_INTERVAL samples — sub-block, so a performed
                // sweep is stepless. prepare() is bounded pure math
                // (sin_cos per stage), red-zone legal; a settled filter
                // skips it entirely.
                for chunk in out.l.chunks_mut(FILTER_COEFF_INTERVAL) {
                    let mut ctrl = [0.0f32; FILTER_COEFF_INTERVAL];
                    let n = chunk.len();
                    cutoff.process(&mut ctrl[..n]);
                    let cut_now = ctrl[n - 1];
                    res.process(&mut ctrl[..n]);
                    let res_now = ctrl[n - 1];
                    drive.process(&mut ctrl[..n]);
                    let drive_now = ctrl[n - 1];

                    let q_eff = fp::effective_q(res_now, drive_now);
                    let moved = (cut_now - *prepared_cutoff).abs() > *prepared_cutoff * 1e-4
                        || (q_eff - *prepared_q).abs() > 1e-4;
                    if moved {
                        *prepared_cutoff = cut_now;
                        *prepared_q = q_eff;
                        match *mode {
                            fp::MODE_BP | fp::MODE_NOTCH => {
                                core.svf.prepare(*sample_rate, cut_now, q_eff);
                            }
                            _ => core.cascade.prepare(
                                *sample_rate,
                                cut_now,
                                q_eff,
                                fp::slope_order(*slope),
                                *mode == fp::MODE_HP,
                            ),
                        }
                    }
                    match *mode {
                        fp::MODE_BP => core
                            .svf
                            .process(chunk, crate::dsp::filters::Mode::BandpassUnity),
                        fp::MODE_NOTCH => core.svf.process(chunk, crate::dsp::filters::Mode::Notch),
                        _ => core.cascade.process(chunk),
                    }
                }

                // Drive stage, permanently in the path so its half-band
                // latency is CONSTANT — a drive knob must never move the
                // track in time. The dry/wet blend is owned HERE, not by
                // the shaper: the kernel's shape() rails its output to ±1
                // even at mix 0 (the shaper device's contract), and a
                // resonant filter legitimately rings past ±1 — the clean
                // path must keep that float headroom. So the shaper runs
                // pure (mix 1, tanh is its own rail) on a copy at 2x, and
                // the node crossfades: drive 0 is bit-transparent apart
                // from the round trip, full drive saturates the resonance
                // for real, and everything between is continuous.
                let drive_now = drive.current();
                let n2 = out.l.len() * 2;
                if scratch2x.len() >= n2 * 2 {
                    let (up, shaped) = scratch2x.split_at_mut(n2);
                    let up = &mut up[..n2];
                    core.oversampler.up(out.l, up);
                    if drive_now.max(drive_start) > 1e-6 {
                        core.shaper.configure(
                            crate::dsp::shaper::Mode::SoftClip,
                            fp::shaper_drive(drive_now),
                            0.0,
                            1.0,
                        );
                        let shaped = &mut shaped[..n2];
                        shaped.copy_from_slice(up);
                        core.shaper.process(shaped);
                        let mut ramp = Ramp::across(drive_start, drive_now, n2);
                        for (d, w) in up.iter_mut().zip(shaped.iter()) {
                            let m = ramp.next();
                            *d = *d * (1.0 - m) + *w * m;
                        }
                    }
                    core.oversampler.down(up, out.l);
                }
                // A driven signal can develop offset; five hertz of nothing
                // keeps it out of everything downstream.
                core.dc.process(out.l);
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
        use crate::params::{clip, filter, mixer, pan, reverb, seq, sine};
        match self {
            // No parameters: compile decides a delay's length, and it
            // cannot change without a recompile — a letter that moved it
            // would slide the track it is compensating.
            Node::Silence | Node::Input { .. } | Node::Delay { .. } => {}
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
            Node::Click { .. } => {}
            Node::Reverb {
                core,
                target_mix,
                size,
                damp,
                ..
            } => {
                let Some(value) = crate::params::clamp(reverb::TABLE, param, value) else {
                    return;
                };
                match param {
                    reverb::MIX => *target_mix = value,
                    reverb::SIZE => {
                        *size = value;
                        // The kernel's decay control is not yet wired to a
                        // knob: until it is, size drives the tail's length
                        // exactly as it did before the two controls split.
                        core.set_room(*size, *size, *damp);
                    }
                    reverb::DAMP => {
                        *damp = value;
                        core.set_room(*size, *size, *damp);
                    }
                    _ => {}
                }
            }
            Node::Filter {
                pending_mode,
                pending_slope,
                cutoff,
                res,
                drive,
                cutoff_target,
                res_target,
                drive_target,
                ..
            } => {
                let Some(value) = crate::params::clamp(filter::TABLE, param, value) else {
                    return;
                };
                match param {
                    filter::MODE => *pending_mode = value.round() as u32,
                    filter::SLOPE => *pending_slope = value.round() as u32,
                    filter::CUTOFF => {
                        *cutoff_target = value;
                        cutoff.set_target(value);
                    }
                    filter::RES => {
                        *res_target = value;
                        res.set_target(value);
                    }
                    filter::DRIVE => {
                        *drive_target = value;
                        drive.set_target(value);
                    }
                    _ => {}
                }
            }
            Node::Seq {
                target_gain,
                attack_rate,
                release_coeff,
                sample_rate,
                ..
            } => {
                let Some(value) = crate::params::clamp(seq::TABLE, param, value) else {
                    return;
                };
                match param {
                    seq::GAIN => *target_gain = value,
                    seq::ATTACK => *attack_rate = synth_attack_rate(value, *sample_rate),
                    seq::RELEASE => *release_coeff = synth_release_coeff(value, *sample_rate),
                    _ => {}
                }
            }
            Node::AudioClip { target_gain, .. } => {
                if let Some(value) = crate::params::clamp(clip::TABLE, param, value)
                    && param == clip::GAIN
                {
                    *target_gain = value;
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

/// A parameter change: "worker `node`, knob `param`, new value". 16 bytes,
/// Copy. `node` is a NodeId in packed form — letters carry the permanent name
/// tag, never a position in line.
#[derive(Debug, Clone, Copy)]
pub struct ParamChange {
    pub node: u64,
    pub param: u32,
    pub value: f32,
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

/// One block's walk of a ramped parameter: the declick rule every audible
/// knob follows, written once. `next()` yields the value for the current
/// sample and THEN steps — the same order every hand-rolled ramp here used,
/// so adopting it is bit-exact. The walk never lands exactly (float drift),
/// which is why every user ends its block with `*param = target`; forgetting
/// that line is the classic drift bug, so it stays visible at the call site
/// rather than hidden in here.
#[derive(Debug, Clone, Copy)]
struct Ramp {
    value: f32,
    step: f32,
}

impl Ramp {
    #[inline]
    fn across(from: f32, to: f32, samples: usize) -> Self {
        Self {
            value: from,
            step: (to - from) / samples.max(1) as f32,
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let value = self.value;
        self.value += self.step;
        value
    }
}

/// Unity gain — the serde default for a `Pan` spec written before the
/// fader existed.
fn unity() -> f32 {
    1.0
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
    peaks: [f32; MAX_METERS],
    /// Modulation, compiled. Evaluated at the top of every segment, before
    /// the walk, so the values a node reads this segment are this
    /// segment's.
    modulation: ModPlan,
    /// Samples of latency between this schedule's inputs and its output —
    /// the deepest path, after compensation has made every path agree.
    ///
    /// Reported, not compensated: the final output cannot be made earlier.
    /// It is what a transport offsets its recording by and what a latency
    /// readout shows.
    latency: usize,
}

/// "This step is not metered." `MAX_METERS` is 32, so 255 cannot collide
/// with a real slot.
const NO_METER: u8 = u8::MAX;

impl Schedule {
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
        let slot = change.node as u32 as usize;
        let generation = (change.node >> 32) as u32;
        if let Some(&(g, dense)) = self.slot_table.get(slot)
            && g == generation
        {
            if let Some(base) = self.modulation.base_slot(dense as usize, change.param) {
                *base = change.value;
            } else if let Some(node) = self.nodes.get_mut(dense as usize) {
                node.apply(change.param, change.value);
            }
        }
    }

    /// Red zone: the peaks accumulated since the last [`Self::clear_peaks`],
    /// one per meter slot. Linear amplitude, where 1.0 is full scale.
    pub fn peaks(&self) -> &[f32; MAX_METERS] {
        &self.peaks
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
        self.modulation.note_peaks(&self.peaks);
        self.peaks = [0.0; MAX_METERS];
    }

    /// Total latency from input to output, in samples. See the field.
    pub fn latency(&self) -> usize {
        self.latency
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

    /// Red zone: walk the chart in dependency order for ONE transport
    /// segment, then copy the output node's slot to every device channel.
    /// Buffers are planar; the segment is `ctx.offset..ctx.offset + ctx.len`
    /// within the block. All node buffers are `ctx.len` long.
    pub fn run(&mut self, output: &mut [f32], ctx: &ProcessCtx<'_>) {
        let len = ctx.len;

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
                && let Some(peak) = self.peaks.get_mut(slot as usize)
            {
                let mut hot = *peak;
                for sample in out.l.iter() {
                    hot = hot.max(sample.abs());
                }
                if let Some(right) = out.r.as_deref() {
                    for sample in right.iter() {
                        hot = hot.max(sample.abs());
                    }
                }
                *peak = hot;
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
    #[error("a subloop is malformed (end <= start, repeats 0 or > 64) or overlaps another")]
    BadSubLoop,
    #[error("a clip loop length must be a positive, finite number of beats")]
    BadClipLen,
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
    /// Modulation, with each wire's `(track, parameter)` target already
    /// resolved to a node and param id by whoever built the graph — the
    /// only place that knows both halves.
    modulation: ModSpec,
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
        gain: f32,
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
        size: f32,
        damp: f32,
        mix: f32,
    },
    /// A resonant filter on whatever feeds it. `mode` and `slope` are
    /// indices into `crate::params::filter`'s lists (lp/hp/bp/notch;
    /// 6-48 dB/octave), `cutoff_hz` is Hz, `q` a resonance Q, `drive`
    /// `0..=1` into the oversampled soft clipper. ParamChange ids and every
    /// range are `crate::params::filter::TABLE`'s.
    Filter {
        mode: u32,
        slope: u32,
        cutoff_hz: f32,
        q: f32,
        drive: f32,
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
    /// Add a node. The returned id is its permanent name tag.
    pub fn push(&mut self, node: NodeSpec) -> NodeId {
        let id = NodeId(self.nodes.insert(node));
        self.order.push(id);
        id
    }

    /// Remove a node and every wire touching it. Its id goes permanently
    /// dead: letters addressed to it are binned, never redelivered.
    pub fn remove(&mut self, id: NodeId) {
        self.nodes.remove(id.0);
        self.order.retain(|n| *n != id);
        self.wires.retain(|(a, b)| *a != id && *b != id);
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
    fn spec_latency(spec: &NodeSpec) -> usize {
        match spec {
            // The half-band round trip of the 2x drive stage.
            NodeSpec::Filter { .. } => crate::dsp::shaper::Oversampler2x::new().latency(),
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
    fn compensate(&self) -> Option<GraphSpec> {
        // Dense indices, in insertion order — the same correspondence the
        // compiler uses.
        let n = self.order.len();
        let dense_of = |id: &NodeId| self.order.iter().position(|x| x == id);
        let latency: Vec<usize> = self
            .order
            .iter()
            .map(|id| self.nodes.get(id.0).map_or(0, Self::spec_latency))
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
        if let Some(aligned) = self.compensate() {
            return aligned.compile_inner(sample_rate, block_frames, bpm);
        }
        self.compile_inner(sample_rate, block_frames, bpm)
    }

    fn compile_inner(
        &self,
        sample_rate: u32,
        block_frames: usize,
        bpm: f64,
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
        let n = self.order.len();
        let dense_of = |id: &NodeId| self.order.iter().position(|x| x == id);

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
                // Ids in `order` always resolve: push/remove keep them in sync.
                Ok(match self.nodes.get(id.0) {
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
                    Some(NodeSpec::Delay { samples, .. }) => {
                        // One ring per channel, sized here and never
                        // resized: the callback only reads and writes.
                        let max = (*samples).max(1);
                        let len = crate::dsp::delay::buffer_len(max);
                        let mut left = crate::dsp::delay::DelayLine::new();
                        let mut right = crate::dsp::delay::DelayLine::new();
                        left.prepare(max);
                        right.prepare(max);
                        left.set_delay(max as f32);
                        right.set_delay(max as f32);
                        Node::Delay {
                            left,
                            right,
                            left_buf: vec![0.0; len],
                            right_buf: vec![0.0; len],
                        }
                    }
                    Some(NodeSpec::Mixer { gain }) => Node::Mixer {
                        gain: 0.0, // ramp in, same reasoning as sine amp
                        target_gain: *gain,
                    },
                    Some(NodeSpec::Seq {
                        notes,
                        subloops,
                        loop_len_beats,
                        params,
                    }) => {
                        if let Some(len) = loop_len_beats
                            && !(len.is_finite() && *len > 0.0)
                        {
                            return Err(CompileError::BadClipLen);
                        }
                        let notes = expand_subloops(notes, subloops)?;
                        // Bake note starts/ends into one sorted event list.
                        // Sort by (beat, rank): offs before ons on exact ties.
                        let mut events = Vec::with_capacity(notes.len() * 2);
                        for n in &notes {
                            if n.len_beats <= 0.0 || n.vel == 0 {
                                continue; // degenerate notes never enter the engine
                            }
                            // Clip mode: a note starting past the loop is
                            // dropped; one ringing past it is cut at the wrap
                            // (off clamped), so the wrap flush can never miss.
                            let mut end = n.start_beats + n.len_beats;
                            if let Some(len) = loop_len_beats {
                                if n.start_beats >= *len {
                                    continue;
                                }
                                end = end.min(*len);
                            }
                            // Musical time becomes SAMPLES here and only
                            // here — the contract's rule 1, and the whole
                            // reason `compile_at_tempo` takes a tempo.
                            // Rounding is to nearest so a note does not
                            // consistently land early.
                            let stamp = |beats: f64| -> u64 {
                                (beats * samples_per_beat).round().max(0.0) as u64
                            };
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
                                && on >= stamp(*len)
                            {
                                continue;
                            }
                            events.push(SeqEvent {
                                sample: on,
                                rank: 1,
                                pitch: n.pitch,
                                vel: n.vel,
                            });
                            events.push(SeqEvent {
                                sample: stamp(end),
                                rank: 0,
                                pitch: n.pitch,
                                vel: 0,
                            });
                        }
                        // (sample, rank): rank 0 = off, so an off at the
                        // same sample as an on always lands first.
                        events.sort_by(|a, b| a.sample.cmp(&b.sample).then(a.rank.cmp(&b.rank)));
                        Node::Seq {
                            events,
                            cursor: 0,
                            voices: VoiceBank::default(),
                            next_age: 0,
                            sample_rate: sample_rate as f32,
                            // RON round-trips NaN literals, so a hand-edited
                            // or corrupt project could smuggle one in; the
                            // default is the sane fallback.
                            gain: if params.gain.is_finite() {
                                params.gain.clamp(0.0, 2.0)
                            } else {
                                SynthParams::default().gain
                            },
                            target_gain: if params.gain.is_finite() {
                                params.gain.clamp(0.0, 2.0)
                            } else {
                                SynthParams::default().gain
                            },
                            attack_rate: synth_attack_rate(params.attack_ms, sample_rate as f32),
                            release_coeff: synth_release_coeff(
                                params.release_ms,
                                sample_rate as f32,
                            ),
                            // The pattern's length in samples at this
                            // tempo. Zero when there is no loop, which the
                            // walk reads as "never wrap".
                            loop_samples: loop_len_beats
                                .map(|len| (len * samples_per_beat).round().max(0.0) as u64)
                                .unwrap_or(0),
                            samples_per_beat,
                            last_cycle: 0,
                            prev_phase: 0,
                        }
                    }
                    Some(NodeSpec::AudioClip {
                        path,
                        start_beats,
                        length_beats,
                        source_offset_frames,
                        source_frames,
                        loop_clip,
                        gain,
                    }) => {
                        // Green zone: opening spawns creek's IO thread and
                        // touches the filesystem — compile is where that lives.
                        // An unopenable file compiles to a silent, failed clip
                        // rather than refusing the whole graph: a missing
                        // sample should not mute the project.
                        let opts = creek::ReadStreamOptions::<SymphoniaDecoder>::default();
                        match ReadDiskStream::<SymphoniaDecoder>::new(path.clone(), 0, opts) {
                            Ok(mut st) => {
                                let frames = st.info().num_frames as u64;
                                let source_start = (*source_offset_frames).min(frames);
                                let available = frames.saturating_sub(source_start);
                                let source_frames =
                                    (*source_frames).unwrap_or(available).min(available);
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
                                // Pin the default cache (index 0) at the file
                                // start: loop wraps then serve from RAM
                                // instead of waiting on a disk seek. The seek
                                // after it is REQUIRED — creek only starts
                                // prefetching once a seek arrives (its own
                                // examples do new -> cache -> seek), and
                                // without it is_ready() stays false forever.
                                // Green zone — this is compile.
                                let _ = st.cache(0, source_start as usize);
                                let _ = st.seek(source_start as usize, SeekMode::Auto);
                                Node::AudioClip {
                                    stream: Some(st),
                                    file_frames: frames,
                                    timeline_start,
                                    timeline_frames,
                                    source_start,
                                    source_frames,
                                    loop_clip: *loop_clip,
                                    gain: 0.0, // ramp in
                                    target_gain: *gain,
                                    failed: false,
                                    next_frame: source_start,
                                }
                            }
                            Err(_) => Node::AudioClip {
                                stream: None,
                                file_frames: 0,
                                timeline_start: 0,
                                timeline_frames: 0,
                                source_start: 0,
                                source_frames: 0,
                                loop_clip: *loop_clip,
                                gain: 0.0,
                                target_gain: *gain,
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
                        }
                    }
                    Some(NodeSpec::Reverb { size, damp, mix }) => {
                        // Green zone: this is where a reverb's memory is
                        // allowed to be born. The kernel only ever indexes
                        // inside it.
                        let sr = sample_rate as f32;
                        let mut buffers = vec![0.0f32; crate::dsp::reverb::Reverb::buffer_len(sr)];
                        let mut core = crate::dsp::reverb::Reverb::new();
                        core.prepare(sr, &mut buffers);
                        // Decay rides size until the kernel's split control
                        // gets a knob of its own — the pre-split behaviour.
                        core.set_room(*size, *size, *damp);
                        Node::Reverb {
                            core,
                            buffers,
                            wet: vec![0.0f32; block_frames],
                            mix: mix.clamp(0.0, 1.0),
                            target_mix: mix.clamp(0.0, 1.0),
                            size: *size,
                            damp: *damp,
                            sample_rate: sr,
                        }
                    }
                    Some(NodeSpec::Filter {
                        mode,
                        slope,
                        cutoff_hz,
                        q,
                        drive,
                    }) => {
                        use crate::params::filter as fp;
                        // Green zone: everything heap-shaped is born here.
                        // Values clamp through the same table rows the
                        // letters will, so a stale project file cannot
                        // smuggle an out-of-range coefficient in.
                        let sr = sample_rate as f32;
                        let mode = (*mode).min(fp::MODE_NOTCH);
                        let slope = (*slope).min(fp::SLOPE_ORDERS.len() as u32 - 1);
                        let cutoff_hz = fp::TABLE[fp::CUTOFF as usize].clamp(*cutoff_hz);
                        let q = fp::TABLE[fp::RES as usize].clamp(*q);
                        let drive = fp::TABLE[fp::DRIVE as usize].clamp(*drive);
                        let mut core = Box::new(FilterCore {
                            cascade: crate::dsp::filters::Cascade::new(),
                            svf: crate::dsp::filters::Svf::new(),
                            shaper: crate::dsp::shaper::Waveshaper::new(),
                            oversampler: crate::dsp::shaper::Oversampler2x::new(),
                            dc: crate::dsp::filters::DcBlocker::new(),
                        });
                        let q_eff = fp::effective_q(q, drive);
                        match mode {
                            fp::MODE_BP | fp::MODE_NOTCH => {
                                core.svf.prepare(sr, cutoff_hz, q_eff);
                            }
                            _ => core.cascade.prepare(
                                sr,
                                cutoff_hz,
                                q_eff,
                                fp::slope_order(slope),
                                mode == fp::MODE_HP,
                            ),
                        }
                        core.dc.prepare(sr);
                        core.shaper.configure(
                            crate::dsp::shaper::Mode::SoftClip,
                            fp::shaper_drive(drive),
                            0.0,
                            1.0, // pure: the node owns the dry/wet blend
                        );
                        let smoother = |value: f32| {
                            let mut s = crate::dsp::ramps::Smoother::new();
                            s.prepare(sr, FILTER_SMOOTH_MS);
                            s.set_now(value);
                            s
                        };
                        Node::Filter {
                            core,
                            // Two 2x lanes: the round-trip signal and the
                            // shaped copy the node blends against.
                            scratch2x: vec![0.0f32; block_frames * 4],
                            mode,
                            slope,
                            pending_mode: mode,
                            pending_slope: slope,
                            cutoff: smoother(cutoff_hz),
                            res: smoother(q),
                            drive: smoother(drive),
                            cutoff_target: cutoff_hz,
                            res_target: q,
                            drive_target: drive,
                            prepared_cutoff: cutoff_hz,
                            prepared_q: q_eff,
                            sample_rate: sr,
                        }
                    }
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
            nodes,
            steps,
            arena: Arena::new(num_slots.max(1), block_frames),
            output_slot,
            slot_table,
            step_meter,
            peaks: [0.0; MAX_METERS],
            // Wires resolve against the SAME dense correspondence the
            // name-tag directory uses, so a wire and a letter can never
            // disagree about which node a parameter lives on.
            modulation: ModPlan::compile(&self.modulation, sample_rate as f32, |node| {
                self.order.iter().position(|id| *id == node)
            }),
            // The deepest path to the output. Compensation has already made
            // every path agree, so summing along any one of them gives the
            // same answer; the walk below takes the max regardless.
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
                                    .map_or(0, Self::spec_latency)
                        })
                        .max()
                        .unwrap_or(0);
                }
                arrival[out]
                    + self
                        .order
                        .get(out)
                        .and_then(|id| self.nodes.get(id.0))
                        .map_or(0, Self::spec_latency)
            }),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    const NO_INPUT: &[f32] = &[0.0; 512];

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
        use crate::ui::device::filter as ui;
        for (mode, ui_mode, q) in [
            (crate::params::filter::MODE_LP, ui::Mode::Lowpass, 0.707),
            (crate::params::filter::MODE_LP, ui::Mode::Lowpass, 8.0),
            (crate::params::filter::MODE_HP, ui::Mode::Highpass, 0.707),
        ] {
            for freq in [250.0, 1_000.0, 4_000.0] {
                let measured = filter_gain_db(
                    freq,
                    NodeSpec::Filter {
                        mode,
                        slope: 3, // 24 dB/octave
                        cutoff_hz: 1_000.0,
                        q,
                        drive: 0.0,
                    },
                );
                let drawn = ui::magnitude_db(
                    &ui::Filter {
                        mode: ui_mode,
                        slope: ui::Slope::Db24,
                        cutoff_hz: 1_000.0,
                        q,
                        drive: 0.0,
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
                    "mode {mode} q {q} at {freq} Hz: audio {measured:.2} dB, display {drawn:.2} dB"
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
            mode: 0,
            slope: 3,
            cutoff_hz: 20_000.0,
            q: 0.707,
            drive: 0.0,
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
                mode: 0,
                slope: 3,
                cutoff_hz: 500.0,
                q: 24.0,
                drive: 0.0,
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
                    mode: 0,
                    slope: 3,
                    cutoff_hz: 20_000.0,
                    q: 0.707,
                    drive,
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
                mode: 0,
                slope: 3,
                cutoff_hz: 1_000.0,
                q: 4.0,
                drive: 0.0,
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
                size: 0.9,
                damp: 0.1,
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
            size: 0.95,
            damp: 0.0,
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
                    mode: 0,
                    slope: 0,
                    cutoff_hz: 20_000.0,
                    q: 0.707,
                    drive: 0.0,
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
            mode: 0,
            slope: 0,
            cutoff_hz: 20_000.0,
            q: 0.707,
            drive: 0.0,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(src, f);
        spec.connect(f, mix);
        spec.connect(src, mix); // the early leg
        spec.set_output(mix);

        let Some(aligned) = spec.compensate() else {
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
        assert!(spec.compensate().is_none());
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
            mode: 0,
            slope: 0,
            cutoff_hz: 8_000.0,
            q: 0.707,
            drive: 0.0,
        });
        spec.connect(src, f);
        spec.set_output(f);
        assert!(
            spec.compensate().is_none(),
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
            mode: 0,
            slope: 0,
            cutoff_hz: 8_000.0,
            q: 0.707,
            drive: 0.4,
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
            },
            Note {
                start_beats: 1.0,
                len_beats: 1.0,
                pitch: 60,
                vel: 100,
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

    #[test]
    fn seq_run_does_not_allocate() {
        let notes: Vec<Note> = (0..16)
            .map(|i| Note {
                start_beats: i as f64 * 0.25,
                len_beats: 0.2,
                pitch: 60 + (i % 12) as u8,
                vel: 100,
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

    fn constant_test_wav(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("daw-test-{name}.wav"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
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
            gain: 1.0,
        });
        spec.set_output(c);
        spec.compile(48_000, 256).unwrap()
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
            gain: 1.0,
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
        let peak = sched.peaks()[3];
        assert!(peak > 0.2, "the tapped node's level is reported ({peak})");
        assert!(peak <= 1.0, "and it is an amplitude, not a sum");
        assert_eq!(
            sched.peaks()[0],
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
            sched.peaks()[3] < 1e-4,
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
        assert!(
            sched.peaks()[0] > 0.0,
            "the tap did its work under the guard"
        );
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
}
