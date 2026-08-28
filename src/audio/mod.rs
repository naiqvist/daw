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
pub mod eq;
pub mod filter;
pub mod gate;
pub mod glue;
pub mod graph;
pub mod handclap;
pub mod hat;
pub mod haze;
pub mod kick;
pub mod limiter;
pub mod material;
pub mod modulation;
pub mod modulato;
pub mod poly;
pub mod preamp;
pub mod project;
pub mod resyn;
pub mod sampler;
pub mod snare;
pub mod strip;
pub mod tom;
pub mod transport;
pub mod utility;

use assert_no_alloc::assert_no_alloc;
use rtaudio::{
    Api, Buffers, DeviceParams, SampleFormat, StreamConfig, StreamFlags, StreamHandle, StreamStatus,
};
use std::sync::Mutex;
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
    pub track_peaks: [f32; graph::MAX_METERS],

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

    /// Transport, as of this block's start.
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
            track_peaks: [0.0; graph::MAX_METERS],
            device_readouts: [graph::Readout::default(); graph::MAX_METERS],
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

/// Owns the running audio stream. Dropping this stops it.
pub struct Engine {
    _stream: StreamHandle,
    info: StreamInfoSnapshot,
    telemetry: triple_buffer::Output<BlockSnapshot>,
    /// Heartbeat state for health(): last block number seen, and when it
    /// last advanced.
    last_seen_block: u64,
    last_advance: Instant,
    /// New schedules go in here; the callback swaps them in between blocks.
    schedule_tx: rtrb::Producer<Box<Schedule>>,
    /// Parameter letters: 16-byte Copy structs, drained at each block start.
    param_tx: rtrb::Producer<ParamChange>,
    /// Modulation letters: wire chains and source definitions, so a knob
    /// drag is heard now instead of at the next debounced schedule swap.
    mod_tx: rtrb::Producer<modulation::ModEdit>,
    /// Transport commands: one ring, so a stop+seek+play gesture is atomic
    /// by ring order.
    transport_tx: rtrb::Producer<TransportCmd>,
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
        let (schedule_tx, mut schedule_rx) = rtrb::RingBuffer::<Box<Schedule>>::new(4);
        // 256 letters is ~1.4 blocks of continuous 60Hz knob-drag backlog —
        // far more than the callback can fall behind by.
        let (param_tx, mut param_rx) = rtrb::RingBuffer::<ParamChange>::new(256);
        // Modulation edits are sent only on CHANGE, and there are far
        // fewer wires than parameters, so 128 is generous.
        let (mod_tx, mut mod_rx) = rtrb::RingBuffer::<modulation::ModEdit>::new(128);
        let (transport_tx, mut transport_rx) = rtrb::RingBuffer::<TransportCmd>::new(64);
        let mut transport = Transport::new(cfg.sample_rate as f64);
        let (mut trash_tx, trash_rx) = rtrb::RingBuffer::<Box<Schedule>>::new(4);
        let mut schedule: Option<Box<Schedule>> = None;

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

                    // Swap in a newer schedule if one arrived. Pop and push are
                    // lock-free and constant-time; the old Box goes back to the
                    // UI thread to be dropped there.
                    while let Ok(mut new_schedule) = schedule_rx.pop() {
                        // Modulation memory rides across the seam: a
                        // recompile happens for a clip drag or a tempo
                        // nudge, once a second, while audio plays — and a
                        // wire mid-glide must not restart it.
                        if let Some(old) = schedule.as_ref() {
                            new_schedule.adopt_modulation_continuity(old);
                        }
                        if let Some(old) = schedule.replace(new_schedule) {
                            // If trash is somehow full the old schedule leaks
                            // until stream teardown — still never freed here.
                            let _ = trash_tx.push(old);
                        }
                    }

                    // Drain parameter letters in arrival order. Bounded by ring
                    // capacity; each apply is two indexes and a store.
                    while let Ok(change) = param_rx.pop() {
                        if let Some(s) = schedule.as_mut() {
                            s.apply(change);
                        }
                    }
                    // Modulation letters, after the parameter letters: both
                    // can arrive in one frame, and a wire's new depth
                    // should be applied against this block's base rather
                    // than the last one's.
                    while let Ok(edit) = mod_rx.pop() {
                        if let Some(s) = schedule.as_mut() {
                            s.apply_mod_edit(edit);
                        }
                    }
                    // Drain transport commands, in order — the whole gesture
                    // lands before any audio is produced, so N seeks in one
                    // block collapse to the last.
                    while let Ok(cmd) = transport_rx.pop() {
                        transport.apply(cmd);
                    }

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
                        // loop points are sample-accurate. Progress is proven
                        // (every segment >= 1 frame); the bound is belt and
                        // braces against the impossible.
                        let mut done = 0usize;
                        let mut segments = 0u32;
                        while done < frames && segments < 64 {
                            let seg = transport.next_segment(frames - done);
                            let ctx = ProcessCtx {
                                device_input: input,
                                in_channels,
                                block_frames: frames,
                                offset: done,
                                len: seg.len,
                                playing: seg.playing,
                                position: seg.position,
                                beat: seg.beat,
                                beats_per_sample: transport.map.beats_per_sample(),
                                discontinuity: seg.discontinuity,
                            };
                            match schedule.as_mut() {
                                Some(s) => s.run(output, &ctx),
                                None => {
                                    for ch in 0..out_channels {
                                        let start = ch * frames + done;
                                        output[start..start + seg.len].fill(0.0);
                                    }
                                }
                            }
                            done += seg.len;
                            segments += 1;
                        }
                        if done < frames {
                            // Segment bound tripped (cannot happen by proof):
                            // fail to silence, not to stale buffer contents.
                            for ch in 0..out_channels {
                                let start = ch * frames + done;
                                output[start..start + (frames - done)].fill(0.0);
                            }
                        }
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
                    let track_peaks = schedule
                        .as_ref()
                        .map_or([0.0; graph::MAX_METERS], |s| *s.peaks());

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

                    telemetry_in.write(BlockSnapshot {
                        block,
                        frames: output.len() / out_channels,
                        peak,
                        head,
                        track_peaks,
                        device_readouts,
                        mod_sources,
                        mod_wire_ids,
                        mod_wires,
                        work_ns,
                        work_max_ns,
                        underflows,
                        overflows,
                        last_xrun_block,
                        playing: transport.playing(),
                        position: transport.position(),
                        beat: transport.map.samples_to_beats(transport.position()),
                        oversized_blocks,
                    });
                });
            })
            .map_err(|e| EngineError::StartStream(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            info,
            telemetry,
            schedule_tx,
            param_tx,
            mod_tx,
            transport_tx,
            trash_rx,
            capture_rx: Some(capture_rx),
            capturing,
            capture_start,
            capture_overruns,
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
        self.schedule_tx
            .push(schedule)
            .map_err(|_| EngineError::ScheduleQueueFull)
    }

    /// Drop any schedules the callback has retired. Cheap; call at UI rate.
    pub fn collect_trash(&mut self) {
        while self.trash_rx.pop().is_ok() {}
    }

    /// Send one transport command. Ring order makes multi-command gestures
    /// (stop + seek + play) atomic with respect to audio.
    pub fn transport(&mut self, cmd: TransportCmd) {
        let _ = self.transport_tx.push(cmd);
    }

    /// Send one parameter change, addressed by the node's permanent name tag.
    /// If the ring is momentarily full (a knob dragged faster than the
    /// callback drains), the newest value is the one that matters — dropping
    /// this letter is fine, the next one supersedes it.
    pub fn set_param(&mut self, node: NodeId, param: u32, value: f32) {
        let _ = self.param_tx.push(ParamChange {
            node: node.to_bits(),
            param,
            value,
        });
    }

    /// Send one modulation edit — a wire's chain, or a source's definition.
    /// Dropped on a full ring for the same reason a parameter letter is:
    /// these carry whole state, so the next one supersedes this one.
    pub fn set_modulation(&mut self, edit: modulation::ModEdit) {
        let _ = self.mod_tx.push(edit);
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;

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
