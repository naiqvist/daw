//! Red zone. Everything in this module is reachable from the audio callback.
//!
//! The callback runs on a realtime thread with a hard deadline of
//! `buffer_frames / sample_rate` seconds — 5.33ms at 48kHz/256. Inside it, and
//! inside anything it calls, there is no allocation, no locking, no syscalls,
//! no logging, no panicking, and no unbounded-time work.
//!
//! See `AGENTS.md`. If a change appears to require breaking one of those,
//! stop and explain rather than writing it.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod acid;
pub mod bounce;
pub mod clamp;
pub mod console;
pub mod eq;
pub mod ferric;
pub mod filter;
pub mod flint;
pub mod gate;
pub mod gauge;
pub mod glue;
pub mod graph;
pub mod handclap;
pub mod hat;
pub mod haze;
pub mod kick;
pub mod lens;
pub mod limiter;
pub mod loom;
pub mod material;
pub mod modulation;
pub mod modulato;
pub mod poly;
pub mod preamp;
pub mod prism;
pub mod project;
pub mod quad;
pub mod resyn;
pub mod sampler;
pub mod scomp;
pub mod sibyl;
pub mod sigil;
pub mod snare;
pub mod stab;
pub mod strip;
pub mod tine;
pub mod tom;
pub mod tone;
pub mod transport;
pub mod umbra;
pub mod utility;

use assert_no_alloc::assert_no_alloc;
use rtaudio::{
    Api, Buffers, DeviceParams, SampleFormat, StreamConfig, StreamFlags, StreamHandle, StreamStatus,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audio::graph::{NodeId, ParamChange, ProcessCtx, Schedule};
use crate::audio::transport::{Transport, TransportCmd};

/// Errors reported by the backend's global error callback. Green zone: the
/// error path may allocate and lock — a stream that is already broken has no
/// deadline left to miss. rtaudio's callback is a process-wide singleton, so
/// this store is static too.
static STREAM_ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// How the stream is doing, judged by two independent signals: errors the
/// backend reported, and whether blocks are still arriving on schedule.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamHealth {
    Running,
    /// No error reported, but the block counter has stopped advancing — the
    /// server died silently or the graph was torn down under us.
    Stalled {
        seconds: f32,
    },
    /// The backend reported an error.
    Errored(String),
}

/// Link the stream's JACK ports to the default sink and source via
/// `wpctl`/`pw-link`. Failures are ignored: the stream still runs, and links
/// can be made by hand in qpwgraph.
fn connect_default_ports() {
    fn default_node(target: &str) -> Option<String> {
        let out = std::process::Command::new("wpctl")
            .args(["inspect", target])
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let line = text.lines().find(|l| l.contains("node.name"))?;
        Some(line.split('"').nth(1)?.to_owned())
    }
    fn link(from: &str, to: &str) {
        let _ = std::process::Command::new("pw-link")
            .args([from, to])
            .status();
    }

    if let Some(sink) = default_node("@DEFAULT_AUDIO_SINK@") {
        link("daw:outport 0", &format!("{sink}:playback_FL"));
        link("daw:outport 1", &format!("{sink}:playback_FR"));
    }
    if let Some(source) = default_node("@DEFAULT_AUDIO_SOURCE@") {
        link(&format!("{source}:capture_FL"), "daw:inport 0");
        link(&format!("{source}:capture_FR"), "daw:inport 1");
    }
}

/// Set flush-to-zero and denormals-are-zero in the x86 MXCSR register.
///
/// Denormals are the tiny not-quite-zero floats a decaying signal passes
/// through on its way down (a reverb tail, a filter ringing out). Hardware
/// handles them via microcode assist at 10-100x the cost of a normal float op,
/// so a silent decaying tail can cost more CPU than a loud signal. FTZ+DAZ
/// makes the FPU treat them as zero, which is inaudible and standard practice
/// in audio engines.
///
/// MXCSR is per-thread state: this must run on the audio thread itself, not
/// the thread that opens the stream. It is a register write — no syscall.
#[inline]
fn flush_denormals_to_zero() {
    // The _mm_getcsr/_mm_setcsr intrinsics are deprecated in favour of inline
    // asm, so read-modify-write MXCSR directly.
    #[cfg(target_arch = "x86_64")]
    // SAFETY: setting FTZ (bit 15) and DAZ (bit 6) only changes how the FPU
    // rounds subnormal values; it cannot fault and affects only this thread.
    unsafe {
        let mut mxcsr: u32 = 0;
        std::arch::asm!(
            "stmxcsr [{ptr}]",
            "or dword ptr [{ptr}], 0x8040",
            "ldmxcsr [{ptr}]",
            ptr = in(reg) &mut mxcsr,
            options(nostack),
        );
    }
}

/// Which backend the engine talks to.
///
/// Our own enum rather than `rtaudio::Api` so the app can offer a choice,
/// persist it and print it without any layer above this one naming the
/// backend crate. `AGENTS.md` says JACK is the one that matters here; the
/// other two exist because a machine without a JACK server should still
/// be able to make a sound rather than refuse to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AudioApi {
    #[default]
    Jack,
    Alsa,
    Pulse,
}

impl AudioApi {
    pub const ALL: [Self; 3] = [Self::Jack, Self::Alsa, Self::Pulse];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jack => "JACK",
            Self::Alsa => "ALSA",
            Self::Pulse => "PulseAudio",
        }
    }

    fn to_rt(self) -> Api {
        match self {
            Self::Jack => Api::UnixJack,
            Self::Alsa => Api::LinuxALSA,
            Self::Pulse => Api::LinuxPulse,
        }
    }

    /// Whether rtaudio was actually built with this backend. A build
    /// missing one succeeds silently, which is the failure `AGENTS.md`
    /// warns about, so the UI asks rather than assumes.
    pub fn compiled(self) -> bool {
        rtaudio::compiled_apis().contains(&self.to_rt())
    }
}

/// One device the app may be pointed at.
///
/// A flattened copy of rtaudio's own, for the same reason [`AudioApi`] is
/// ours: nothing above this module should have to name the backend crate
/// to draw a device list.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDevice {
    /// The device's name, which is also how a CHOICE is remembered.
    ///
    /// rtaudio's `DeviceID` carries a session id as well, and its own doc
    /// says that half does not survive a reboot. The name does, so the
    /// name is what a preference stores and what `Engine::start` resolves
    /// against — falling back to the default if the device has gone.
    pub name: String,
    pub output_channels: u32,
    pub input_channels: u32,
    pub is_default_output: bool,
    pub preferred_sample_rate: u32,
    pub sample_rates: Vec<u32>,
}

/// Green zone: every output device this backend can see.
///
/// Empty when the backend is not compiled in or the host will not open —
/// which is a real answer for the UI to draw, not an error to propagate.
pub fn output_devices(api: AudioApi) -> Vec<AudioDevice> {
    if !api.compiled() {
        return Vec::new();
    }
    let Ok(host) = rtaudio::Host::new(api.to_rt()) else {
        return Vec::new();
    };
    host.iter_output_devices()
        .map(|d| AudioDevice {
            name: d.id.name.clone(),
            output_channels: d.output_channels,
            input_channels: d.input_channels,
            is_default_output: d.is_default_output,
            preferred_sample_rate: d.preferred_sample_rate,
            sample_rates: d.sample_rates.clone(),
        })
        .collect()
}

/// What the engine was asked to open with.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    pub api: AudioApi,
    /// The output device by NAME, or `None` for the backend's default.
    /// See [`AudioDevice::name`] for why a name and not an id.
    pub output_device: Option<String>,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub channels: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            api: AudioApi::Jack,
            output_device: None,
            sample_rate: 48_000,
            buffer_frames: 256,
            channels: 2,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// rtaudio was built without the backend that was asked for — see
    /// AGENTS.md. A build missing JACK succeeds silently and runs through
    /// the ALSA/Pulse compat layer instead, which is why this is an error
    /// and not a fallback.
    #[error("rtaudio was compiled without the {0} backend")]
    BackendMissing(&'static str),

    #[error("could not open JACK host: {0}")]
    Host(String),

    #[error("could not open duplex stream: {0}")]
    OpenStream(String),

    #[error("could not start stream: {0}")]
    StartStream(String),

    #[error("schedule queue is full — callback is not draining it")]
    ScheduleQueueFull,

    /// The stream negotiated interleaved buffers. Every buffer index in the
    /// engine assumes planar layout — running would produce garbage audio,
    /// so refuse loudly instead.
    #[error(
        "stream negotiated interleaved buffers (NONINTERLEAVED not honoured);          the engine's planar layout assumptions would silently corrupt audio"
    )]
    InterleavedNotSupported,
}

/// Green-built, sample-rate-matched audio for the callback's one-shot
/// audition voice. Samples are planar, matching [`material::Material`];
/// playback reads at most the first two channels, so work per output frame is
/// constant even for unusual files.
pub struct AuditionBuffer {
    samples: Arc<Vec<f32>>,
    channels: usize,
    frames: usize,
}

impl AuditionBuffer {
    pub fn from_material(material: material::Material) -> Self {
        Self {
            samples: material.samples,
            channels: material.channels,
            frames: usize::try_from(material.frames).unwrap_or(0),
        }
    }

    fn frames(&self) -> usize {
        self.frames
    }
}

enum AuditionCommand {
    Play(basedrop::Owned<AuditionBuffer>),
    Stop,
}

struct AuditionVoice {
    current: Option<basedrop::Owned<AuditionBuffer>>,
    pending: Option<basedrop::Owned<AuditionBuffer>>,
    position: usize,
    fade_frames: usize,
    stop_remaining: Option<usize>,
}

const AUDITION_GAIN: f32 = 0.25;
const AUDITION_FADE_MS: usize = 5;

impl AuditionVoice {
    fn new(sample_rate: u32) -> Self {
        Self {
            current: None,
            pending: None,
            position: 0,
            fade_frames: (sample_rate as usize * AUDITION_FADE_MS / 1_000).max(1),
            stop_remaining: None,
        }
    }

    fn apply(&mut self, command: AuditionCommand) {
        match command {
            AuditionCommand::Play(buffer) if self.current.is_none() => self.start(buffer),
            AuditionCommand::Play(buffer) => {
                self.pending = Some(buffer);
                self.begin_stop();
            }
            AuditionCommand::Stop => {
                self.pending = None;
                self.begin_stop();
            }
        }
    }

    fn start(&mut self, buffer: basedrop::Owned<AuditionBuffer>) {
        self.current = Some(buffer);
        self.position = 0;
        self.stop_remaining = None;
    }

    fn begin_stop(&mut self) {
        if self.stop_remaining.is_some() {
            return;
        }
        let Some(current) = self.current.as_ref() else {
            return;
        };
        let left = current.frames().saturating_sub(self.position);
        self.stop_remaining = Some(self.fade_frames.min(left).max(1));
    }

    fn finish_current(&mut self) {
        // Dropping a basedrop::Owned only appends to its collector's lock-free
        // queue. The allocation itself is reclaimed by Engine::collect_trash
        // on the green thread.
        self.current = None;
        self.stop_remaining = None;
        if let Some(next) = self.pending.take() {
            self.start(next);
        }
    }

    fn mix(&mut self, output: &mut [f32], out_channels: usize, frames: usize) {
        if out_channels == 0 {
            return;
        }
        for frame in 0..frames {
            let Some(current) = self.current.as_ref() else {
                break;
            };
            let total = current.frames();
            if self.position >= total {
                self.finish_current();
                continue;
            }
            let (left, right) = audition_frame(current, self.position);
            let gain = AUDITION_GAIN
                * audition_envelope(self.position, total, self.fade_frames, self.stop_remaining);
            if out_channels == 1 {
                if let Some(sample) = output.get_mut(frame) {
                    *sample += (left + right) * 0.5 * gain;
                }
            } else {
                if let Some(sample) = output.get_mut(frame) {
                    *sample += left * gain;
                }
                if let Some(sample) = output.get_mut(frames + frame) {
                    *sample += right * gain;
                }
            }
            self.position = self.position.saturating_add(1);
            let stopping = self.stop_remaining.map(|left| left.saturating_sub(1));
            self.stop_remaining = stopping;
            if self.position >= total || stopping == Some(0) {
                self.finish_current();
            }
        }
    }
}

fn audition_frame(buffer: &AuditionBuffer, position: usize) -> (f32, f32) {
    let left = buffer.samples.get(position).copied().unwrap_or(0.0);
    let right = if buffer.channels > 1 {
        buffer
            .frames
            .checked_add(position)
            .and_then(|index| buffer.samples.get(index))
            .copied()
            .unwrap_or(left)
    } else {
        left
    };
    (left, right)
}

/// A click-free one-shot envelope. Both the first and final audible frames
/// reach exact zero; an explicit stop uses the same exact-zero tail.
fn audition_envelope(
    position: usize,
    total: usize,
    fade_frames: usize,
    stop_remaining: Option<usize>,
) -> f32 {
    let fade = fade_frames.max(1) as f32;
    let fade_in = position.min(fade_frames) as f32 / fade;
    let natural_left = total.saturating_sub(position.saturating_add(1));
    let fade_out = natural_left.min(fade_frames) as f32 / fade;
    let stop = stop_remaining.map_or(1.0, |left| {
        left.saturating_sub(1).min(fade_frames) as f32 / fade
    });
    fade_in.min(fade_out).min(stop)
}

/// How many leading samples of channel 0 each block reports to the UI.
pub const SNAPSHOT_SAMPLES: usize = 8;

/// One block's worth of telemetry, handed to the UI.
///
/// Fixed size and `Copy` on purpose: writing it is a memcpy into a
/// `triple_buffer`, with no allocation and no lock.
#[derive(Debug, Clone, Copy)]
pub struct BlockSnapshot {
    /// Monotonic block counter since the stream started.
    pub block: u64,
    /// Frames in this block. Should equal the configured buffer size.
    pub frames: usize,
    /// Largest absolute sample in the INPUT block. This is the MIC, not the
    /// mix — the output's levels are reported per track in `track_peaks`.
    pub peak: f32,
    /// Leading samples of input channel 0, as captured from the device.
    pub head: [f32; SNAPSHOT_SAMPLES],
    /// Largest absolute sample this block at each of the schedule's meter
    /// taps — for the daw, one per track, taken post-fader and post-pan.
    /// Linear amplitude, where 1.0 is full scale.
    ///
    /// Fixed size on purpose: this whole struct is a memcpy into a
    /// `triple_buffer`, and a Vec cannot ride in one. A slot with no tap
    /// behind it reads 0.0, which is also what a silent track reads — the
    /// meter cannot tell the difference and does not need to.
    pub track_peaks_l: [f32; graph::MAX_METERS],
    /// The right side of the same reading. A mono node reports the same
    /// figure on both sides.
    pub track_peaks_r: [f32; graph::MAX_METERS],

    /// What each TAPPED DEVICE said about itself this block — how loud
    /// its detector heard, and how hard it worked.
    ///
    /// The same slot space as `track_peaks` and the same fixed size, for
    /// the same reason: this whole struct is a memcpy into a
    /// `triple_buffer`, and a Vec cannot ride in one. A slot with no tap
    /// behind it reads its default, which is silence and no reduction —
    /// exactly what a device that is not working reads, and the display
    /// cannot tell the difference and does not need to.
    pub device_readouts: [graph::Readout; graph::MAX_METERS],
    /// The console's telemetry: what every tapped section said this
    /// block, in its own slot space. Same rules as the readouts.
    pub telemetry: [graph::Readout; graph::MAX_TELEMETRY],

    /// Each modulation source's value as of this block's last segment, in
    /// the arrangement's modulator order.
    ///
    /// The mod strip draws FROM HERE rather than from a copy of the maths
    /// run in the frame loop: the engine is what is actually sounding, and
    /// a scope that shows a parallel simulation is a scope that can lie.
    pub mod_sources: [f32; modulation::MAX_MOD_SOURCES],
    /// The wire each `mod_wires` reading belongs to. Wires whose target did
    /// not survive compilation are absent, so the ID is what matches a
    /// reading back to a wire — never the index. 0 marks an unused slot.
    pub mod_wire_ids: [u64; modulation::MAX_MOD_WIRES],
    /// Each wire's output after its whole chain, in the target's own units.
    pub mod_wires: [f32; modulation::MAX_MOD_WIRES],

    /// How long the callback body took for this block, in nanoseconds.
    pub work_ns: u64,
    /// Worst callback duration since the stream started.
    pub work_max_ns: u64,
    /// Output underflows reported by the device since start. Each one was
    /// probably audible.
    pub underflows: u64,
    /// Input overflows since start — capture data was lost.
    pub overflows: u64,
    /// Block number of the most recent xrun (0 = never). Turns "we had an
    /// xrun at some point" into "it happened at block N", which is the
    /// difference between attributing and guessing.
    pub last_xrun_block: u64,

    /// Transport immediately after this block: the next block's start. The
    /// samples and meters above describe the block which ended here, while
    /// this edge is the clock a frame-rate input service timestamps against.
    pub playing: bool,
    pub position: u64,
    pub beat: f64,
    /// Blocks refused because the server delivered more frames than the
    /// arena was sized for (a live quantum change). Nonzero means silence
    /// was output instead of corrupt audio.
    pub oversized_blocks: u64,
}

impl Default for BlockSnapshot {
    fn default() -> Self {
        Self {
            block: 0,
            frames: 0,
            peak: 0.0,
            head: [0.0; SNAPSHOT_SAMPLES],
            track_peaks_l: [0.0; graph::MAX_METERS],
            track_peaks_r: [0.0; graph::MAX_METERS],
            device_readouts: [graph::Readout::default(); graph::MAX_METERS],
            telemetry: [graph::Readout::default(); graph::MAX_TELEMETRY],
            mod_sources: [0.0; modulation::MAX_MOD_SOURCES],
            mod_wire_ids: [0; modulation::MAX_MOD_WIRES],
            mod_wires: [0.0; modulation::MAX_MOD_WIRES],
            work_ns: 0,
            work_max_ns: 0,
            underflows: 0,
            overflows: 0,
            last_xrun_block: 0,
            playing: false,
            position: 0,
            beat: 0.0,
            oversized_blocks: 0,
        }
    }
}

impl BlockSnapshot {
    /// Every track's level as a single number: the louder side, per slot.
    ///
    /// What a caller with room for one mark reads, and what metering a
    /// track meant before the sides were reported separately — so a
    /// reading taken this way cannot disagree with the one taken then.
    pub fn track_peaks(&self) -> [f32; graph::MAX_METERS] {
        let mut peaks = [0.0f32; graph::MAX_METERS];
        for (slot, out) in peaks.iter_mut().enumerate() {
            *out = self.track_peaks_l[slot].max(self.track_peaks_r[slot]);
        }
        peaks
    }

    /// Fraction of the deadline the callback used, at the given stream config.
    /// 1.0 means the whole budget was spent; past 1.0 the deadline was missed.
    pub fn load(&self, sample_rate: u32, buffer_frames: u32) -> f64 {
        if sample_rate == 0 {
            return 0.0;
        }
        let budget_ns = buffer_frames as f64 / sample_rate as f64 * 1e9;
        self.work_ns as f64 / budget_ns
    }
}

/// What the stream actually negotiated, which is not always what was asked for.
#[derive(Debug, Clone, Copy)]
pub struct StreamInfoSnapshot {
    pub sample_rate: u32,
    pub max_frames: usize,
    pub in_channels: usize,
    pub out_channels: usize,
    /// False means interleaved, which would invalidate the planar assumptions
    /// the DSP is written against.
    pub deinterleaved: bool,
    pub latency_frames: Option<usize>,
}

/// A letter, plus the compile it was addressed under.
///
/// Node name tags are unique only within one `thunderdome` arena and every
/// compile starts a fresh one, so the same slot and generation name
/// different nodes in successive schedules. A letter that misses its
/// schedule swap by a block would otherwise be delivered to whatever now
/// occupies its slot — silently, and to a node the sender never meant.
///
/// The epoch travels WITH the letter rather than being inferred at the far
/// end, because by the time the callback reads it the sender's idea of
/// "current" is exactly the thing in question.
#[derive(Debug, Clone, Copy)]
struct Stamped<T> {
    epoch: u64,
    item: T,
}

/// How many letters each green-to-audio ring holds — and, because they are
/// the SAME constants, how many the callback will take from one in a single
/// block. That identity is the point: see [`drain_bounded`].
const SCHEDULE_RING: usize = 4;
const PARAM_RING: usize = 256;
const MOD_RING: usize = 128;
const TRANSPORT_RING: usize = 64;
const AUDITION_RING: usize = 8;
const SHUTDOWN_RUNNING: u8 = 0;
const SHUTDOWN_REQUESTED: u8 = 1;
const SHUTDOWN_ACKNOWLEDGED: u8 = 2;

/// Take at most `max` letters from `rx`, handing each to `apply`. Returns
/// how many were taken.
///
/// **Red zone, and the bound is the whole reason this exists.**
/// `while let Ok(x) = rx.pop() {}` reads as "drain the ring, so at most
/// capacity iterations" and that is FALSE: `rtrb` reloads the producer's
/// tail when its cached range is exhausted, so a green-side thread writing
/// while the callback drains can keep the loop fed indefinitely. Capacity
/// bounds how many letters may WAIT at once; it never bounded how many can
/// pass through in one block.
///
/// The callback is not allowed an unbounded-time path (`AGENTS.md`), and
/// four of these loops were one. Nothing had been heard yet only because
/// every producer runs at UI speed — which is a fact about today's callers,
/// not a property of the code.
///
/// Anything past `max` stays in the ring and is taken next block: one block
/// of extra latency for a backlog that in practice never forms.
fn drain_bounded<T>(rx: &mut rtrb::Consumer<T>, max: usize, mut apply: impl FnMut(T)) -> usize {
    let mut taken = 0;
    while taken < max {
        let Ok(item) = rx.pop() else {
            break;
        };
        apply(item);
        taken += 1;
    }
    taken
}

/// Move callback-owned retired values toward the green-side collector.
///
/// A failed `rtrb::Producer::push` returns ownership of the value. Ignoring
/// that error would DROP it on the audio thread, so a full ring puts the value
/// straight back into its fixed callback-owned slot. No value is destroyed by
/// this function, and the walk is bounded by `N`.
fn flush_retirement_backlog<T, const N: usize>(
    tx: &mut rtrb::Producer<T>,
    backlog: &mut [Option<T>; N],
) {
    for slot in backlog {
        let Some(item) = slot.take() else {
            continue;
        };
        match tx.push(item) {
            Ok(()) => {}
            Err(rtrb::PushError::Full(item)) => {
                *slot = Some(item);
                // This producer is the only writer. If this push found the
                // ring full, every later push in the same pass would too.
                break;
            }
        }
    }
}

/// Owns the running audio stream. Dropping this stops it.
pub struct Engine {
    _stream: Option<StreamHandle>,
    info: StreamInfoSnapshot,
    telemetry: triple_buffer::Output<BlockSnapshot>,
    /// Heartbeat state for health(): last block number seen, and when it
    /// last advanced.
    last_seen_block: u64,
    last_advance: Instant,
    /// New schedules go in here; the callback swaps them in between blocks.
    schedule_tx: rtrb::Producer<Box<Schedule>>,
    /// The epoch of the schedule most recently handed to the callback, and
    /// therefore the one every outgoing letter is addressed under. Zero
    /// until the first schedule: epochs start at 1, so letters sent before
    /// there is anything to address are binned rather than guessed at.
    epoch: u64,
    /// Parameter letters: 16-byte Copy structs, drained at each block start.
    param_tx: rtrb::Producer<Stamped<ParamChange>>,
    /// Modulation letters: wire chains and source definitions, so a knob
    /// drag is heard now instead of at the next debounced schedule swap.
    mod_tx: rtrb::Producer<Stamped<modulation::ModEdit>>,
    /// Transport commands: one ring, so a stop+seek+play gesture is atomic
    /// by ring order.
    transport_tx: rtrb::Producer<TransportCmd>,
    /// Latest-wins audition commands. The optional producer is taken before
    /// stream teardown so every queued basedrop allocation can be collected.
    audition_tx: Option<rtrb::Producer<AuditionCommand>>,
    audition_retry: Option<AuditionCommand>,
    audition_collector: Option<basedrop::Collector>,
    /// Retired schedules come back here so they are dropped on THIS thread,
    /// never freed inside the callback.
    trash_rx: rtrb::Consumer<Box<Schedule>>,
    /// Captured input, waiting to be written to disk. Taken ONCE by
    /// whoever is going to drain it — see [`Engine::take_capture`].
    capture_rx: Option<rtrb::Consumer<f32>>,
    /// Whether the callback is filling that ring. An atomic and not a
    /// command, because it is one bit that must be readable by the
    /// callback without draining anything.
    capturing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The transport sample the current capture began at, stamped by the
    /// callback on the first block it captures.
    ///
    /// Stamped THERE and not here, because only the callback knows which
    /// block actually caught the flag — the difference between the two
    /// is where a take lands against the grid.
    capture_start: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Blocks of input the callback could not fit into the ring. Nonzero
    /// means the recording has a HOLE in it, which the user must be told
    /// about — it is not a dropout that can be heard and shrugged off.
    capture_overruns: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Control asks the callback to stop active plug-in processors before
    /// stream teardown; the callback acknowledges after doing so on its own
    /// thread. This is an atomic edge, never a blocking red-zone command.
    shutdown: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl Engine {
    /// Open a duplex stream on JACK and start it. The callback currently writes
    /// silence and reads nothing.
    ///
    /// JACK is requested explicitly: `Api::Unspecified` resolves to ALSA on this
    /// machine even when JACK is compiled in.
    pub fn start(cfg: EngineConfig) -> Result<Self, EngineError> {
        if !cfg.api.compiled() {
            return Err(EngineError::BackendMissing(cfg.api.label()));
        }

        let mut host =
            rtaudio::Host::new(cfg.api.to_rt()).map_err(|e| EngineError::Host(e.to_string()))?;
        host.show_warnings(false);

        // The chosen device, resolved by NAME. A device that has been
        // unplugged since the preference was written falls back to the
        // default rather than refusing to start — a missing interface
        // should cost you your choice, not your session.
        let chosen = cfg.output_device.as_ref().and_then(|want| {
            host.iter_output_devices()
                .find(|d| d.id.name == *want)
                .map(|d| d.id.clone())
        });
        let device = |ch| {
            Some(DeviceParams {
                device_id: chosen.clone(),
                num_channels: Some(ch),
                ..Default::default()
            })
        };

        let stream_cfg = StreamConfig {
            output_device: device(cfg.channels),
            input_device: device(cfg.channels),
            sample_format: SampleFormat::Float32,
            sample_rate: Some(cfg.sample_rate),
            buffer_frames: cfg.buffer_frames,
            // NONINTERLEAVED gives planar buffers, which is what the DSP and any
            // later SIMD work assume. SCHEDULE_REALTIME asks for an RT thread.
            // JACK_DONT_CONNECT because auto-connect wired our outputs to our
            // own inputs (a silent loopback); we link explicitly below instead.
            flags: StreamFlags::NONINTERLEAVED
                | StreamFlags::SCHEDULE_REALTIME
                | StreamFlags::JACK_DONT_CONNECT,
            priority: 80,
            name: "daw".to_owned(),
            ..Default::default()
        };

        let mut stream = host
            .open_stream(&stream_cfg)
            .map_err(|(_host, e)| EngineError::OpenStream(e.to_string()))?;

        let info = {
            let i = stream.info();
            StreamInfoSnapshot {
                sample_rate: i.sample_rate,
                max_frames: i.max_frames,
                in_channels: i.in_channels,
                out_channels: i.out_channels,
                deinterleaved: i.deinterleaved,
                latency_frames: i.latency,
            }
        };

        // Hard gate, not a warning: Schedule::run and the meter pass index
        // buffers as planar. An interleaved stream would run fine and sound
        // wrong — the worst failure mode. Refuse before the first callback.
        if !info.deinterleaved {
            return Err(EngineError::InterleavedNotSupported);
        }

        // Telemetry out to the UI. Latest-value-wins is right here: the UI
        // redraws far slower than blocks arrive, and it wants the newest state,
        // not a backlog.
        let (mut telemetry_in, telemetry) = triple_buffer::triple_buffer(&BlockSnapshot::default());

        // Schedule handoff. Capacity 4 is plenty: swaps happen at UI speed.
        // Pushing a Box moves a pointer — the callback never allocates or frees.
        let (schedule_tx, mut schedule_rx) = rtrb::RingBuffer::<Box<Schedule>>::new(SCHEDULE_RING);
        // 256 letters is ~1.4 blocks of continuous 60Hz knob-drag backlog —
        // far more than the callback can fall behind by.
        let (param_tx, mut param_rx) = rtrb::RingBuffer::<Stamped<ParamChange>>::new(PARAM_RING);
        // Modulation edits are sent only on CHANGE, and there are far
        // fewer wires than parameters, so 128 is generous.
        let (mod_tx, mut mod_rx) = rtrb::RingBuffer::<Stamped<modulation::ModEdit>>::new(MOD_RING);
        let (transport_tx, mut transport_rx) =
            rtrb::RingBuffer::<TransportCmd>::new(TRANSPORT_RING);
        let (audition_tx, mut audition_rx) =
            rtrb::RingBuffer::<AuditionCommand>::new(AUDITION_RING);
        let audition_collector = basedrop::Collector::new();
        let mut audition_voice = AuditionVoice::new(cfg.sample_rate);
        let mut transport = Transport::new(cfg.sample_rate as f64);
        let (mut trash_tx, trash_rx) = rtrb::RingBuffer::<Box<Schedule>>::new(4);
        let mut schedule: Option<Box<Schedule>> = None;
        // Retirements which cannot cross the trash ring yet remain owned by
        // the callback in fixed storage. Four slots match the most schedules
        // we can accept in one block; once they are full, schedule handoff
        // simply waits for the green side to collect rather than freeing an
        // old graph in the red zone.
        let mut retirement_backlog: [Option<Box<Schedule>>; SCHEDULE_RING] =
            std::array::from_fn(|_| None);
        // A newly compiled chart has fresh sequencer cursors. Its first
        // segment must therefore be treated like a seek, even when the
        // transport itself continued seamlessly while the green side rebuilt
        // the graph. Without this edge, a mid-play swap starts each fresh
        // cursor at event zero and can fire the whole elapsed arrangement in
        // one callback.
        let mut schedule_discontinuity = false;

        // Captured input on its way to a file. Sized for four seconds of
        // every input channel, which is far more backlog than a green
        // thread that drains once per frame can build up — and it is the
        // one place a recording can lose samples, so it is generous
        // rather than tight.
        let capture_slots = (info.in_channels.max(1) * cfg.sample_rate as usize * 4).max(1);
        let (mut capture_tx, capture_rx) = rtrb::RingBuffer::<f32>::new(capture_slots);
        let capturing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let capture_start = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let capture_overruns = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let callback_capturing = std::sync::Arc::clone(&capturing);
        let callback_capture_start = std::sync::Arc::clone(&capture_start);
        let callback_overruns = std::sync::Arc::clone(&capture_overruns);
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(SHUTDOWN_RUNNING));
        let callback_shutdown = std::sync::Arc::clone(&shutdown);
        // Whether the run in progress has already stamped its start. Owned
        // by the callback alone, so it needs no atomic.
        let mut capture_stamped = false;

        let out_channels = info.out_channels.max(1);
        let slot_capacity = info.max_frames;
        let mut block: u64 = 0;
        let mut oversized_blocks: u64 = 0;
        let mut work_max_ns: u64 = 0;
        let mut underflows: u64 = 0;
        let mut overflows: u64 = 0;
        let mut last_xrun_block: u64 = 0;

        let mut denormals_flushed = false;

        // Loud deaths: the backend's global error callback. Errors are stored
        // and surfaced through health(); a broken stream has no deadline, so
        // the alloc/lock here is fine.
        rtaudio::set_error_callback(|e| {
            if let Ok(mut errs) = STREAM_ERRORS.lock() {
                errs.push(e.to_string());
            }
        });

        // Wire our ports before the first callback ever runs: relinking a
        // LIVE stream makes PipeWire reconfigure the graph mid-flight, which
        // showed up as xruns. Ports exist once the stream is open; links made
        // now are in place before processing begins.
        connect_default_ports();

        stream
            .start(move |buffers, _info, status| {
                // Once, on the callback thread itself — MXCSR is per-thread.
                if !denormals_flushed {
                    flush_denormals_to_zero();
                    denormals_flushed = true;
                }

                // Instant::now() is CLOCK_MONOTONIC via the vDSO on Linux — a
                // plain function call, not a syscall. Safe on this thread.
                let t0 = Instant::now();

                if status.contains(StreamStatus::OUTPUT_UNDERFLOW) {
                    underflows += 1;
                    last_xrun_block = block;
                }
                if status.contains(StreamStatus::INPUT_OVERFLOW) {
                    overflows += 1;
                    last_xrun_block = block;
                }
                // Everything past this point is red zone. assert_no_alloc aborts
                // the process on any allocation here in debug builds; it compiles
                // to a no-op in release.
                assert_no_alloc(|| {
                    let Buffers::Float32 { output, input } = buffers else {
                        return;
                    };

                    // CLAP start/stop belong to the processing thread. Keep
                    // the stream alive until this edge is acknowledged; all
                    // later callbacks stay silent while control tears down.
                    let shutdown_state =
                        callback_shutdown.load(std::sync::atomic::Ordering::Acquire);
                    if shutdown_state != SHUTDOWN_RUNNING {
                        if shutdown_state == SHUTDOWN_REQUESTED {
                            if let Some(active) = schedule.as_mut() {
                                active.prepare_for_retirement();
                            }
                            callback_shutdown
                                .store(SHUTDOWN_ACKNOWLEDGED, std::sync::atomic::Ordering::Release);
                        }
                        output.fill(0.0);
                        return;
                    }

                    // Swap in newer schedules if they arrived. Each retiring
                    // Box first gets a guaranteed callback-owned slot; only
                    // the green side ever drops it. This is slightly more
                    // ceremony than ignoring a full-ring error, but that error
                    // OWNS the Box and would otherwise run its destructor here.
                    flush_retirement_backlog(&mut trash_tx, &mut retirement_backlog);
                    let mut schedules_taken = 0usize;
                    while schedules_taken < SCHEDULE_RING {
                        let retire_slot = if schedule.is_some() {
                            retirement_backlog.iter().position(Option::is_none)
                        } else {
                            // No old schedule means no retirement slot needed.
                            Some(SCHEDULE_RING)
                        };
                        let Some(retire_slot) = retire_slot else {
                            break;
                        };
                        let Ok(mut new_schedule) = schedule_rx.pop() else {
                            break;
                        };
                        // Modulation memory rides across the seam: a
                        // recompile happens for a clip drag or a tempo
                        // nudge, once a second, while audio plays — and a
                        // wire mid-glide must not restart it.
                        if let Some(old) = schedule.as_ref() {
                            new_schedule.adopt_modulation_continuity(old);
                        }
                        if let Some(mut old) = schedule.replace(new_schedule) {
                            old.prepare_for_retirement();
                            retirement_backlog[retire_slot] = Some(old);
                        }
                        schedule_discontinuity = true;
                        schedules_taken += 1;
                    }
                    flush_retirement_backlog(&mut trash_tx, &mut retirement_backlog);

                    // Parameter letters in arrival order; each apply is two
                    // indexes and a store. The count is the bound — the ring's
                    // capacity never was one.
                    drain_bounded(&mut param_rx, PARAM_RING, |change| {
                        if let Some(s) = schedule.as_mut()
                            && change.epoch == s.epoch()
                        {
                            s.apply(change.item);
                        }
                    });
                    // Modulation letters, after the parameter letters: both
                    // can arrive in one frame, and a wire's new depth
                    // should be applied against this block's base rather
                    // than the last one's.
                    drain_bounded(&mut mod_rx, MOD_RING, |edit| {
                        if let Some(s) = schedule.as_mut()
                            && edit.epoch == s.epoch()
                        {
                            s.apply_mod_edit(edit.item);
                        }
                    });
                    // Transport commands, in order — the whole gesture lands
                    // before any audio is produced, so N seeks in one block
                    // collapse to the last. A gesture longer than the ring
                    // would now split across two blocks; at 64 commands that
                    // is a backlog nothing generates, and splitting is still
                    // better than an unbounded callback.
                    drain_bounded(&mut transport_rx, TRANSPORT_RING, |cmd| {
                        transport.apply(cmd);
                    });
                    // This one was already bounded, by a hand-written counter;
                    // it now says so through the same helper as the rest.
                    // Replaced buffers retire through basedrop; no allocation
                    // is freed on this thread.
                    drain_bounded(&mut audition_rx, AUDITION_RING, |command| {
                        audition_voice.apply(command);
                    });

                    let frames = output.len() / out_channels;
                    let in_channels = input.len().checked_div(frames).unwrap_or(0);
                    // Where this block starts on the timeline, read BEFORE
                    // the segment walk advances it. A capture stamps its
                    // start from here, so a take lands against the same
                    // clock the sequencer placed notes against.
                    let block_position = transport.position();

                    // A new measurement window for this block's meters, and
                    // it happens for EVERY block — including the refused
                    // one below. A block that produces silence must report
                    // silence: leaving the array alone would freeze every
                    // meter at the last good reading for as long as the
                    // refusal lasts, which is precisely the lie the
                    // snapshot's own doc says a meter must not tell.
                    if let Some(s) = schedule.as_mut() {
                        s.clear_peaks();
                    }
                    if frames > slot_capacity {
                        // Live quantum change beyond what the arena was sized
                        // for: refuse loudly (silence + counter), never index
                        // out of the slots. Time still advances.
                        oversized_blocks += 1;
                        output.fill(0.0);
                        let mut left = frames;
                        while left > 0 {
                            left -= transport.next_segment(left).len;
                        }
                    } else {
                        // Segment loop: split the block at transport events so
                        // loop points and tempo marks are sample-accurate.
                        // Every segment is at least one frame, so `frames`
                        // iterations is a complete hard bound even for a map
                        // with a boundary on every sample.
                        let mut done = 0usize;
                        let mut segments_left = frames;
                        while done < frames && segments_left > 0 {
                            // A ProcessCtx carries one constant tempo. Bound
                            // the transport's ordinary loop segment at the
                            // arrangement schedule's next tempo mark as well,
                            // so a mark is sample-exact without mutating or
                            // allocating any map state in the callback.
                            let remaining = frames - done;
                            // A stopped clock cannot cross a tempo boundary:
                            // its position is frozen. Walking the same nearby
                            // boundary once per output sample would only repeat
                            // identical work, so stopped monitoring is always
                            // one segment.
                            let limited = if transport.playing() {
                                schedule.as_ref().map_or(remaining, |schedule| {
                                    schedule.frames_until_control_change(
                                        transport.position(),
                                        remaining,
                                    )
                                })
                            } else {
                                remaining
                            };
                            let seg = transport.next_segment(limited);
                            let beat = schedule.as_ref().map_or(seg.beat, |schedule| {
                                schedule.beat_at(seg.position, transport.map)
                            });
                            let beats_per_sample = schedule.as_ref().map_or_else(
                                || transport.map.beats_per_sample(),
                                |schedule| {
                                    schedule.beats_per_sample_at(seg.position, transport.map)
                                },
                            );
                            let ctx = ProcessCtx {
                                device_input: input,
                                in_channels,
                                block_frames: frames,
                                offset: done,
                                len: seg.len,
                                playing: seg.playing,
                                position: seg.position,
                                beat,
                                beats_per_sample,
                                discontinuity: seg.discontinuity || schedule_discontinuity,
                            };
                            match schedule.as_mut() {
                                Some(s) => {
                                    s.run(output, &ctx);
                                    // One segment is enough to reseek every
                                    // PatternClock in the immutable schedule.
                                    schedule_discontinuity = false;
                                }
                                None => {
                                    for ch in 0..out_channels {
                                        let start = ch * frames + done;
                                        output[start..start + seg.len].fill(0.0);
                                    }
                                }
                            }
                            done += seg.len;
                            segments_left -= 1;
                        }
                        if done < frames {
                            // A segment returned zero despite its contract:
                            // fail to silence, not to stale buffer contents.
                            for ch in 0..out_channels {
                                let start = ch * frames + done;
                                output[start..start + (frames - done)].fill(0.0);
                            }
                        }
                        // Outside the compiled schedule by design: an archive
                        // audition never recompiles the song. One bounded pass
                        // over this block, two source reads and at most two
                        // output writes per frame.
                        audition_voice.mix(output, out_channels, frames);
                    }
                    // The mic is deliberately NOT routed to the output — that
                    // would be a feedback loop. Input is only metered below,
                    // and captured to the ring above if someone is recording.

                    // --- capture ------------------------------------------
                    //
                    // The whole block or none of it. A partial write would
                    // misalign the interleave for everything after it, so a
                    // ring with no room loses one block cleanly and says so
                    // rather than corrupting the rest of the take.
                    //
                    // Nothing here allocates: `write_chunk_uninit` reserves
                    // space that already exists and `fill_from_iter` walks
                    // it once.
                    if callback_capturing.load(std::sync::atomic::Ordering::Acquire) {
                        if !capture_stamped {
                            // The transport sample this block starts at,
                            // which is where the take belongs on the
                            // timeline. Taken from the segment walk's own
                            // definition, so it is the same clock the
                            // sequencer placed notes against.
                            callback_capture_start
                                .store(block_position, std::sync::atomic::Ordering::Release);
                            capture_stamped = true;
                        }
                        let wanted = frames * in_channels;
                        match capture_tx.write_chunk_uninit(wanted) {
                            Ok(chunk) => {
                                // Planar in, INTERLEAVED out: the writer on
                                // the other end demultiplexes by channel,
                                // and one frame's channels sitting together
                                // is what lets it do that without a second
                                // buffer.
                                chunk.fill_from_iter((0..frames).flat_map(|frame| {
                                    (0..in_channels).map(move |channel| {
                                        input.get(channel * frames + frame).copied().unwrap_or(0.0)
                                    })
                                }));
                            }
                            Err(_) => {
                                callback_overruns
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    } else {
                        capture_stamped = false;
                    }

                    block = block.wrapping_add(1);

                    // Bounded work: one pass for the peak, one fixed-length copy.
                    // No indexing, so no path that can panic on a short buffer.
                    let peak = input.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    let mut head = [0.0f32; SNAPSHOT_SAMPLES];
                    for (dst, src) in head.iter_mut().zip(input.iter()) {
                        *dst = *src;
                    }
                    // The meters, gathered by the walk itself. A silent
                    // stretch with no schedule reports zeros rather than the
                    // last block's levels — a meter frozen at its last
                    // reading is worse than one that reads nothing.
                    let device_readouts = schedule
                        .as_ref()
                        .map_or([graph::Readout::default(); graph::MAX_METERS], |s| {
                            *s.readouts()
                        });
                    let telemetry = schedule
                        .as_ref()
                        .map_or([graph::Readout::default(); graph::MAX_TELEMETRY], |s| {
                            *s.telemetry()
                        });
                    let track_peaks_l = schedule
                        .as_ref()
                        .map_or([0.0; graph::MAX_METERS], |s| *s.peaks_l());
                    let track_peaks_r = schedule
                        .as_ref()
                        .map_or([0.0; graph::MAX_METERS], |s| *s.peaks_r());

                    // Modulation, as the engine just ran it. Three fixed
                    // fills; no schedule reports zeros, which is the honest
                    // reading for "nothing is modulating anything".
                    let mut mod_sources = [0.0; modulation::MAX_MOD_SOURCES];
                    let mut mod_wire_ids = [0; modulation::MAX_MOD_WIRES];
                    let mut mod_wires = [0.0; modulation::MAX_MOD_WIRES];
                    if let Some(s) = schedule.as_ref() {
                        s.modulation().source_values(&mut mod_sources);
                        s.modulation()
                            .wire_outputs(&mut mod_wire_ids, &mut mod_wires);
                    }

                    // Stop the clock before publishing so the write itself is
                    // not counted; the memcpy is ~100 bytes and constant.
                    let work_ns = t0.elapsed().as_nanos() as u64;
                    work_max_ns = work_max_ns.max(work_ns);

                    let position = transport.position();
                    let beat = schedule.as_ref().map_or_else(
                        || transport.map.samples_to_beats(position),
                        |schedule| schedule.beat_at(position, transport.map),
                    );
                    telemetry_in.write(BlockSnapshot {
                        block,
                        frames: output.len() / out_channels,
                        peak,
                        head,
                        track_peaks_l,
                        track_peaks_r,
                        device_readouts,
                        telemetry,
                        mod_sources,
                        mod_wire_ids,
                        mod_wires,
                        work_ns,
                        work_max_ns,
                        underflows,
                        overflows,
                        last_xrun_block,
                        playing: transport.playing(),
                        position,
                        beat,
                        oversized_blocks,
                    });
                });
            })
            .map_err(|e| EngineError::StartStream(e.to_string()))?;

        Ok(Self {
            _stream: Some(stream),
            info,
            telemetry,
            schedule_tx,
            epoch: 0,
            param_tx,
            mod_tx,
            transport_tx,
            audition_tx: Some(audition_tx),
            audition_retry: None,
            audition_collector: Some(audition_collector),
            trash_rx,
            capture_rx: Some(capture_rx),
            capturing,
            capture_start,
            capture_overruns,
            shutdown,
            last_seen_block: 0,
            last_advance: Instant::now(),
        })
    }

    pub fn info(&self) -> StreamInfoSnapshot {
        self.info
    }

    /// Take the capture ring's reading end. Once — a second caller gets
    /// `None`, because two drains of one ring would each get half the
    /// samples and neither would know.
    pub fn take_capture(&mut self) -> Option<rtrb::Consumer<f32>> {
        self.capture_rx.take()
    }

    /// Start or stop filling the capture ring.
    ///
    /// The caller must have drained whatever was left in it BEFORE
    /// starting: the ring is a pipe, not a session, and stale samples in
    /// front of a new take would shift the whole take late.
    pub fn set_capturing(&self, on: bool) {
        self.capturing
            .store(on, std::sync::atomic::Ordering::Release);
    }

    /// The transport sample the capture in progress began at.
    pub fn capture_start(&self) -> u64 {
        self.capture_start
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Blocks of input the callback could not fit into the ring since the
    /// stream started. Nonzero means a recording has a HOLE in it.
    pub fn capture_overruns(&self) -> u64 {
        self.capture_overruns
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Most recent block the callback published. Cheap; call it once per frame.
    pub fn latest_block(&mut self) -> BlockSnapshot {
        *self.telemetry.read()
    }

    /// Judge the stream by two independent signals: reported errors (loud
    /// deaths) and the block heartbeat (silent ones). Call at UI rate.
    ///
    /// Grace period: blocks arrive every 5.33ms, so 60x that (~320ms) of
    /// silence is a stall beyond doubt, while scheduler hiccups stay invisible.
    pub fn health(&mut self) -> StreamHealth {
        if let Ok(mut errs) = STREAM_ERRORS.lock()
            && let Some(err) = errs.pop()
        {
            errs.clear();
            return StreamHealth::Errored(err);
        }

        let block = self.latest_block().block;
        if block != self.last_seen_block {
            self.last_seen_block = block;
            self.last_advance = Instant::now();
            return StreamHealth::Running;
        }
        let quiet = self.last_advance.elapsed();
        if quiet.as_millis() > 320 {
            StreamHealth::Stalled {
                seconds: quiet.as_secs_f32(),
            }
        } else {
            StreamHealth::Running
        }
    }

    /// Hand the callback a new schedule. Also drains any retired schedules,
    /// dropping them here on the UI thread. Call `collect_trash` periodically
    /// too if schedules are swapped often.
    pub fn set_schedule(&mut self, schedule: Box<Schedule>) -> Result<(), EngineError> {
        self.collect_trash();
        // Read the epoch BEFORE the box crosses: after the push it belongs
        // to the callback. On a failed push the epoch is left alone, so
        // letters keep addressing the schedule that is actually installed.
        let epoch = schedule.epoch();
        self.schedule_tx
            .push(schedule)
            .map_err(|_| EngineError::ScheduleQueueFull)?;
        self.epoch = epoch;
        Ok(())
    }

    /// Drop any schedules the callback has retired. Cheap; call at UI rate.
    pub fn collect_trash(&mut self) {
        self.flush_audition_command();
        if let Some(collector) = &mut self.audition_collector {
            collector.collect();
        }
        while self.trash_rx.pop().is_ok() {}
    }

    /// Start a fixed-gain one-shot outside the compiled schedule. Allocation
    /// happens here; dropping or replacing it in the callback only queues it
    /// back to this thread through basedrop.
    pub fn audition(&mut self, buffer: AuditionBuffer) {
        let Some(collector) = self.audition_collector.as_ref() else {
            return;
        };
        let owned = basedrop::Owned::new(&collector.handle(), buffer);
        self.audition_retry = Some(AuditionCommand::Play(owned));
        self.flush_audition_command();
    }

    /// Ask the callback for a declicked stop. Latest command wins while the
    /// fixed ring is full, so a stop cannot disappear behind cursor traffic.
    pub fn stop_audition(&mut self) {
        self.audition_retry = Some(AuditionCommand::Stop);
        self.flush_audition_command();
    }

    fn flush_audition_command(&mut self) {
        let Some(command) = self.audition_retry.take() else {
            return;
        };
        let Some(tx) = self.audition_tx.as_mut() else {
            self.audition_retry = Some(command);
            return;
        };
        if let Err(rtrb::PushError::Full(command)) = tx.push(command) {
            self.audition_retry = Some(command);
        }
    }

    /// Send one transport command, reporting whether the callback mailbox
    /// accepted it. Callers which mirror transport state must only advance
    /// that mirror after `true`, so a full ring is retried on their next
    /// green-zone pass instead of silently losing the gesture.
    pub fn transport(&mut self, cmd: TransportCmd) -> bool {
        self.transport_tx.push(cmd).is_ok()
    }

    /// Send one parameter change, addressed by the node's permanent name tag.
    /// If the ring is momentarily full, return `false`. State-sync callers
    /// then keep their revision dirty and resend the complete current state
    /// on the next green-zone pass.
    pub fn set_param(&mut self, node: NodeId, param: u32, value: f32) -> bool {
        self.param_tx
            .push(Stamped {
                epoch: self.epoch,
                item: ParamChange {
                    node: node.to_bits(),
                    param,
                    value,
                },
            })
            .is_ok()
    }

    /// Send one modulation edit — a wire's chain, or a source's definition.
    /// Returns whether the callback mailbox accepted the complete edit.
    pub fn set_modulation(&mut self, edit: modulation::ModEdit) -> bool {
        self.mod_tx
            .push(Stamped {
                epoch: self.epoch,
                item: edit,
            })
            .is_ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // A processor which started in the callback must stop there too.
        // Request one final silent callback and wait a bounded interval while
        // the stream is still alive. A backend which has already died cannot
        // acknowledge; stream teardown remains the only possible fallback.
        self.capturing
            .store(false, std::sync::atomic::Ordering::Release);
        self.shutdown
            .store(SHUTDOWN_REQUESTED, std::sync::atomic::Ordering::Release);
        let deadline = Instant::now() + std::time::Duration::from_millis(100);
        while self.shutdown.load(std::sync::atomic::Ordering::Acquire) != SHUTDOWN_ACKNOWLEDGED
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        // Stop the callback first, then drop both ends of its audition ring;
        // every Owned queued or held there can now reach the collector before
        // the collector itself leaves this green thread.
        drop(self._stream.take());
        drop(self.audition_tx.take());
        self.audition_retry = None;
        if let Some(mut collector) = self.audition_collector.take() {
            collector.collect();
            let _ = collector.try_cleanup();
        }
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;

    /// Does a backend deliver blocks at all on this machine? `#[ignore]`
    /// because it opens real hardware. `DAW_TEST_API=pulse` or `alsa`
    /// tries another backend — which is how the JACK shim's silence was
    /// told apart from the engine's.
    #[test]
    #[ignore]
    fn the_engine_delivers_blocks() {
        let mut config = EngineConfig::default();
        if std::env::var("DAW_TEST_API").as_deref() == Ok("pulse") {
            config.api = AudioApi::Pulse;
        } else if std::env::var("DAW_TEST_API").as_deref() == Ok("alsa") {
            config.api = AudioApi::Alsa;
        }
        let Ok(mut engine) = Engine::start(config) else {
            panic!("the test engine did not start");
        };
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let block = engine.latest_block().block;
        eprintln!("blocks after 1.5 s: {block}, info {:?}", engine.info());
        assert!(block > 0, "no callback ran");
    }

    /// A DIAGNOSTIC, not a check: it prints what this machine can see, so
    /// "the device picker is empty" can be answered without a GUI.
    ///
    /// `#[ignore]` because it talks to real hardware — the answer depends
    /// on what is plugged in and whether a JACK server is up, which is
    /// not something a test suite should have an opinion about. Run it
    /// with `cargo test -- --ignored --nocapture list_the_output_devices`.
    #[test]
    #[ignore]
    fn list_the_output_devices() {
        for api in AudioApi::ALL {
            println!("{} compiled: {}", api.label(), api.compiled());
            for d in output_devices(api) {
                println!(
                    "  {:<40} out {:>2}  in {:>2}  default {}  rates {:?}",
                    d.name,
                    d.output_channels,
                    d.input_channels,
                    d.is_default_output,
                    d.sample_rates
                );
            }
        }
    }
}

#[cfg(test)]
mod audition_tests {
    use super::*;

    fn buffer(samples: &[f32], channels: usize, frames: usize) -> AuditionBuffer {
        AuditionBuffer {
            samples: Arc::new(samples.to_vec()),
            channels,
            frames,
        }
    }

    #[test]
    fn mono_and_stereo_frames_reach_the_expected_outputs() {
        let mono = buffer(&[0.25, -0.5], 1, 2);
        assert_eq!(audition_frame(&mono, 0), (0.25, 0.25));
        assert_eq!(audition_frame(&mono, 1), (-0.5, -0.5));

        let stereo = buffer(&[0.25, 0.5, -0.25, -0.5], 2, 2);
        assert_eq!(audition_frame(&stereo, 0), (0.25, -0.25));
        assert_eq!(audition_frame(&stereo, 1), (0.5, -0.5));
    }

    #[test]
    fn every_declick_tail_reaches_exact_zero() {
        assert_eq!(audition_envelope(0, 32, 4, None), 0.0);
        assert_eq!(audition_envelope(31, 32, 4, None), 0.0);
        assert_eq!(audition_envelope(12, 32, 4, Some(1)), 0.0);
        assert_eq!(audition_envelope(4, 32, 4, None), 1.0);
    }

    #[test]
    fn an_owned_buffer_crosses_the_command_ring_and_collects_green_side() {
        let mut collector = basedrop::Collector::new();
        let owned = basedrop::Owned::new(&collector.handle(), buffer(&[1.0], 1, 1));
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1);
        assert!(tx.push(AuditionCommand::Play(owned)).is_ok());
        let command = rx.pop().ok();
        assert!(matches!(command, Some(AuditionCommand::Play(_))));
        drop(command);
        collector.collect();
        assert_eq!(collector.alloc_count(), 0);
        assert!(collector.try_cleanup().is_ok());
    }

    #[test]
    fn callback_side_command_and_mix_work_do_not_allocate() {
        let mut collector = basedrop::Collector::new();
        let owned = basedrop::Owned::new(&collector.handle(), buffer(&[1.0; 64], 2, 32));
        let mut voice = AuditionVoice::new(48_000);
        let mut output = [0.0; 64];
        assert_no_alloc::assert_no_alloc(|| {
            voice.apply(AuditionCommand::Play(owned));
            voice.mix(&mut output, 2, 16);
            voice.apply(AuditionCommand::Stop);
            voice.mix(&mut output, 2, 16);
        });
        drop(voice);
        collector.collect();
        assert_eq!(collector.alloc_count(), 0);
        assert!(collector.try_cleanup().is_ok());
    }
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DropSpy(Arc<AtomicUsize>);

    impl Drop for DropSpy {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// THE test. A producer that writes while the callback drains is what
    /// makes `while let Ok(..) = pop()` unbounded, and it is made
    /// deterministic here by refilling from inside `apply` itself: under
    /// the old loop this never terminates, because every take frees a slot
    /// the producer immediately reuses.
    #[test]
    fn a_producer_refilling_during_the_drain_cannot_extend_it() {
        let (mut tx, mut rx) = rtrb::RingBuffer::<u32>::new(4);
        for i in 0..4 {
            let _ = tx.push(i);
        }
        let mut seen = 0usize;
        let taken = drain_bounded(&mut rx, 4, |_| {
            seen += 1;
            // The concurrent producer, standing over the bucket.
            let _ = tx.push(99);
        });
        assert_eq!(taken, 4, "the bound must hold even as the ring refills");
        assert_eq!(seen, 4, "and apply runs exactly that many times");
        // Proof the refills really happened and are simply left for the
        // next block, which is the intended behaviour rather than a loss.
        assert!(rx.pop().is_ok(), "what arrived mid-drain waits its turn");
    }

    #[test]
    fn it_takes_at_most_max_and_leaves_the_rest() {
        let (mut tx, mut rx) = rtrb::RingBuffer::<u32>::new(8);
        for i in 0..8 {
            let _ = tx.push(i);
        }
        let taken = drain_bounded(&mut rx, 3, |_| {});
        assert_eq!(taken, 3);
        let rest = drain_bounded(&mut rx, 8, |_| {});
        assert_eq!(rest, 5, "the remainder is still there next block");
    }

    #[test]
    fn an_empty_ring_stops_immediately() {
        let (_tx, mut rx) = rtrb::RingBuffer::<u32>::new(4);
        assert_eq!(drain_bounded(&mut rx, 256, |_| {}), 0);
    }

    #[test]
    fn a_zero_bound_takes_nothing_and_does_not_spin() {
        let (mut tx, mut rx) = rtrb::RingBuffer::<u32>::new(4);
        let _ = tx.push(1);
        assert_eq!(drain_bounded(&mut rx, 0, |_| {}), 0);
        assert!(rx.pop().is_ok(), "and the letter is untouched");
    }

    #[test]
    fn a_full_retirement_ring_keeps_ownership_in_the_fixed_backlog() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1);
        tx.push(DropSpy(Arc::clone(&dropped)))
            .expect("the ring starts empty");
        let mut backlog = [
            Some(DropSpy(Arc::clone(&dropped))),
            Some(DropSpy(Arc::clone(&dropped))),
        ];

        flush_retirement_backlog(&mut tx, &mut backlog);
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            0,
            "red-side push dropped a value"
        );
        assert!(backlog.iter().all(Option::is_some));

        drop(rx.pop().expect("the green side takes the resident value"));
        flush_retirement_backlog(&mut tx, &mut backlog);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(backlog.iter().filter(|slot| slot.is_some()).count(), 1);

        drop(rx.pop().expect("the first retirement crossed"));
        flush_retirement_backlog(&mut tx, &mut backlog);
        drop(rx.pop().expect("the second retirement crossed"));
        assert!(backlog.iter().all(Option::is_none));
        assert_eq!(dropped.load(Ordering::Relaxed), 3);
    }

    /// Order is preserved: these are letters, and a stop+seek+play gesture
    /// is only atomic if it arrives in the order it was written.
    #[test]
    fn letters_arrive_in_the_order_they_were_written() {
        let (mut tx, mut rx) = rtrb::RingBuffer::<u32>::new(8);
        for i in 0..5 {
            let _ = tx.push(i);
        }
        let mut got = Vec::new();
        drain_bounded(&mut rx, 8, |v| got.push(v));
        assert_eq!(got, vec![0, 1, 2, 3, 4]);
    }

    /// The drain bound and the ring capacity are the same constant, so they
    /// cannot drift into disagreeing about what "one block's worth" means.
    #[test]
    fn every_ring_is_drained_to_exactly_its_own_capacity() {
        assert_eq!(SCHEDULE_RING, 4);
        assert_eq!(PARAM_RING, 256);
        assert_eq!(MOD_RING, 128);
        assert_eq!(TRANSPORT_RING, 64);
        assert_eq!(AUDITION_RING, 8);
    }
}
