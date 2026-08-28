//! Audio clip waveform analysis and the bottom-region waveform editor.
//!
//! Analysis is green-zone work. A background worker reads the same WAV the
//! audio node streams and reduces it into immutable min/max levels. The UI
//! therefore draws work proportional to its pixel width, never to the file's
//! duration, and the audio callback never sees this module or its allocations.
//!
//! The editor is a view over [`Clip`], just like the piano roll: clip/source
//! data remains owned by the arrangement. The BPM grid is a reference only;
//! audio is not warped or time-stretched here.

use crate::{AudioSource, Clip, Focus, GRID_MIN_PX, claim};
use daw::ui::affordance::{Afford, Affords};
use daw::ui::device::{Mapping, Param, Unit, poly_widgets};
use daw::ui::theme::Theme;
use daw::ui::tokens::{control, font, stroke};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One stored peak covers this many source frames at the finest level.
/// At 48 kHz stereo this costs about 16 MiB per hour — three floats per
/// bin, extremes and RMS — while still putting several peaks under each
/// screen pixel at ordinary editing zooms.
const BASE_FRAMES_PER_PEAK: u64 = 128;
/// Each coarser level merges this many peaks from the previous one.
const LEVEL_REDUCTION: usize = 4;

const HEADER_H: f32 = 30.0;
/// The narrowest the waveform itself may be squeezed to before the
/// controls column stands down entirely. Below this there is no useful
/// picture left to put controls beside.
const MIN_DISPLAY_W: f32 = 220.0;
const CHANNEL_PAD: f32 = 8.0;
const LABEL_PAD: f32 = 6.0;
/// A little more presence than a hairline without turning the waveform
/// back into a solid block at ordinary overview zooms.
const WAVEFORM_STROKE: f32 = 1.25;
const AMPLITUDE_MIN: f32 = 0.25;
const AMPLITUDE_MAX: f32 = 8.0;
const ZOOM_STEP: f64 = 1.2;
const MAX_GRID_LINES: usize = 8_192;
/// The room one time-ladder tick's label needs before the ladder may use
/// a step that fine.
const TIME_LABEL_W: f32 = 64.0;

/// The zoom, in CLIP FRAMES PER PIXEL, from thirty-two pixels per sample
/// to a scale that puts an hour on a laptop screen.
///
/// Frames per pixel rather than pixels per beat because every edit this
/// editor makes is measured in frames, and a view whose native unit was
/// the beat would round-trip through BPM on every gesture. The beat grid
/// converts INTO this space, not the other way round.
const FPP_MIN: f64 = 1.0 / 32.0;
const FPP_MAX: f64 = 4_194_304.0;
/// Below this, the min/max pyramid is the wrong picture: a bin covering a
/// fraction of a pixel draws a bar where there is a curve. Real samples
/// take over here.
const SAMPLE_DRAW_FPP: f64 = 4.0;
/// At this many pixels per frame there is room to mark each sample.
const SAMPLE_DOT_PX: f64 = 8.0;
/// The most frames one sample-window request may ask for. A window is for
/// DRAWING: past a million frames there is no display wide enough to show
/// them one at a time, and the pyramid is the right answer again.
const WINDOW_MAX_FRAMES: u64 = 1 << 20;
/// Frames of slack either side of the visible range, so a small scroll
/// does not fetch again.
const WINDOW_MARGIN: u64 = 1 << 14;
/// The bottom of the dB amplitude scale. Below this a sample draws on the
/// centre line.
const DB_FLOOR: f32 = -72.0;
/// The air left either side of a selection zoomed to fill the display, as
/// a fraction of its length. Filling the display EXACTLY would leave both
/// edges on the display's own edges, where they cannot be grabbed.
const SELECTION_ZOOM_MARGIN: f64 = 0.05;
/// The zero-crossing search's reach, in frames. Beyond this the nearest
/// crossing is far enough away that snapping to it would move the edit
/// somewhere the user did not point.
pub const ZERO_CROSSING_REACH: u64 = 2_048;

pub const DEFAULT_H: f32 = 300.0;
pub const H_RANGE: std::ops::RangeInclusive<f32> = 120.0..=900.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
    /// Root mean square over the same frames the extremes cover.
    ///
    /// Carried beside the extremes rather than derived from them because
    /// it cannot be: two bins with the same min and max can hold wildly
    /// different energy, and the difference between them is exactly what
    /// a level reading is for.
    pub rms: f32,
}

impl Peak {
    const SILENCE: Self = Self {
        min: 0.0,
        max: 0.0,
        rms: 0.0,
    };

    /// Two bins as one.
    ///
    /// The extremes are exact. The RMS is the root mean square of the two
    /// treated as EQUAL weights, which is exact for the bins of one level
    /// — they all cover `frames_per_peak` frames — and approximate only
    /// where the file's final partial bin is one of the two. A level
    /// reading that is a fraction of a dB out on the last 128 frames of a
    /// file is not worth carrying a frame count through the pyramid for.
    fn merged(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
            rms: ((self.rms * self.rms + other.rms * other.rms) * 0.5).sqrt(),
        }
    }

    /// Does this bin hold a sample at or past full scale?
    pub fn clipped(&self) -> bool {
        self.max >= 1.0 || self.min <= -1.0
    }
}

#[derive(Debug, Clone)]
struct PeakLevel {
    frames_per_peak: u64,
    /// Channel-major peak arrays. Every channel has the same number of bins.
    channels: Vec<Vec<Peak>>,
}

/// Immutable multiresolution waveform analysis for one playback file.
#[derive(Debug, Clone)]
pub struct Peaks {
    sample_rate: u32,
    channels: u16,
    frames: u64,
    levels: Vec<PeakLevel>,
}

impl Peaks {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        usize::from(self.channels)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Min/max for a source-frame interval. The selected level leaves at
    /// most a handful of bins to combine for one display pixel.
    pub fn extrema(&self, channel: usize, start: u64, end: u64) -> Option<Peak> {
        let start = start.min(self.frames);
        let end = end.min(self.frames);
        if channel >= self.channels() || start >= end {
            return None;
        }
        let span = end - start;
        let level = self
            .levels
            .iter()
            .rev()
            .find(|level| level.frames_per_peak <= span)
            .or_else(|| self.levels.first())?;
        let peaks = level.channels.get(channel)?;
        let first = (start / level.frames_per_peak) as usize;
        let last = ((end - 1) / level.frames_per_peak) as usize;
        let bins = peaks.get(first..=last)?;
        let (first_bin, rest) = bins.split_first()?;
        // The extremes fold pairwise; the RMS is accumulated over the whole
        // run instead, so every bin weighs the same however many there are.
        // Folding it pairwise would weight the last bin half the total.
        let mut out = *first_bin;
        let mut energy = f64::from(first_bin.rms) * f64::from(first_bin.rms);
        for peak in rest {
            out.min = out.min.min(peak.min);
            out.max = out.max.max(peak.max);
            energy += f64::from(peak.rms) * f64::from(peak.rms);
        }
        out.rms = (energy / bins.len() as f64).sqrt() as f32;
        Some(out)
    }

    /// The loudest sample and the RMS across a source-frame range, in
    /// dBFS — what a selection readout prints.
    ///
    /// Off the pyramid, so a ten-minute selection costs the same as a
    /// ten-millisecond one and neither of them touches the disk.
    pub fn levels(&self, channels: &[usize], start: u64, end: u64) -> Option<(f32, f32)> {
        let mut peak = 0.0f32;
        let mut energy = 0.0f64;
        let mut counted = 0usize;
        for &channel in channels {
            let Some(bin) = self.extrema(channel, start, end) else {
                continue;
            };
            peak = peak.max(bin.max.abs()).max(bin.min.abs());
            energy += f64::from(bin.rms) * f64::from(bin.rms);
            counted += 1;
        }
        (counted > 0).then(|| {
            let rms = (energy / counted as f64).sqrt() as f32;
            (dbfs(peak), dbfs(rms))
        })
    }
}

/// A magnitude as dBFS, with a floor rather than a minus infinity.
pub fn dbfs(magnitude: f32) -> f32 {
    if magnitude > 0.0 {
        (20.0 * magnitude.log10()).max(DB_FLOOR)
    } else {
        DB_FLOOR
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("cannot decode waveform {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("unsupported waveform {path}: {reason}")]
    Unsupported { path: PathBuf, reason: String },
}

enum Command {
    Build(PathBuf),
    Window(WindowKey),
}

pub struct LoadResult {
    pub path: PathBuf,
    pub result: Result<Arc<Peaks>, WaveformError>,
}

/// Which stretch of which file a sample window covers. The editor's
/// request and the worker's answer are the same value, so a late answer
/// to a request the view has already moved past is recognised and
/// dropped rather than drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowKey {
    pub path: PathBuf,
    pub start: u64,
    pub frames: u64,
}

/// Decoded frames of a file, for drawing individual samples.
///
/// The peak pyramid answers every question about a stretch of audio
/// EXCEPT what a single sample is, and past a few frames per pixel that
/// is the only question left. This is bounded by [`WINDOW_MAX_FRAMES`],
/// so it is a picture's worth of samples and never a file's worth.
#[derive(Debug)]
pub struct SampleWindow {
    pub key: WindowKey,
    pub channels: usize,
    /// Interleaved, `channels` per frame, starting at `key.start`.
    samples: Vec<f32>,
}

impl SampleWindow {
    /// One sample, by absolute source frame. `None` outside the window.
    pub fn at(&self, channel: usize, frame: u64) -> Option<f32> {
        if channel >= self.channels || frame < self.key.start {
            return None;
        }
        let index = (frame - self.key.start) as usize;
        self.samples.get(index * self.channels + channel).copied()
    }

    /// Does this window already hold `[from, to)` of `path`?
    pub fn covers(&self, path: &Path, from: u64, to: u64) -> bool {
        self.key.path == path
            && from >= self.key.start
            && to <= self.key.start.saturating_add(self.key.frames)
    }
}

pub struct WindowResult {
    pub key: WindowKey,
    pub result: Result<Arc<SampleWindow>, WaveformError>,
}

/// Background owner for waveform analysis. Requests and results are entirely
/// on the UI/control side; no handle from here enters an audio schedule.
pub struct Service {
    commands: crossbeam_channel::Sender<Command>,
    results: crossbeam_channel::Receiver<LoadResult>,
    windows: crossbeam_channel::Receiver<WindowResult>,
}

impl Service {
    pub fn start() -> Self {
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (result_tx, results) = crossbeam_channel::unbounded();
        let (window_tx, windows) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("waveform-analysis".to_owned())
            .spawn(move || {
                while let Ok(command) = command_rx.recv() {
                    // Two kinds of work, one thread, in the order asked.
                    // A window is milliseconds and a pyramid is a second
                    // at worst, and a second thread would only mean two
                    // heads on the same disk.
                    let sent = match command {
                        Command::Build(path) => {
                            let result = build_peaks(&path).map(Arc::new);
                            result_tx
                                .send(LoadResult {
                                    path: path.clone(),
                                    result,
                                })
                                .is_ok()
                        }
                        Command::Window(key) => {
                            let result = read_window(&key).map(Arc::new);
                            window_tx.send(WindowResult { key, result }).is_ok()
                        }
                    };
                    if !sent {
                        return;
                    }
                }
            })
            .expect("waveform worker must start");
        Self {
            commands,
            results,
            windows,
        }
    }

    pub fn request(&self, path: PathBuf) {
        let _ = self.commands.send(Command::Build(path));
    }

    /// Ask for a stretch of decoded samples. Capped here rather than at
    /// the call site, so no caller can ask the worker for a whole file.
    pub fn request_window(&self, mut key: WindowKey) {
        key.frames = key.frames.min(WINDOW_MAX_FRAMES);
        let _ = self.commands.send(Command::Window(key));
    }

    pub fn try_result(&self) -> Option<LoadResult> {
        self.results.try_recv().ok()
    }

    pub fn try_window(&self) -> Option<WindowResult> {
        self.windows.try_recv().ok()
    }
}

type Wav = hound::WavReader<std::io::BufReader<std::fs::File>>;

/// Open a WAV and check it describes audio at all.
fn open_wav(path: &Path) -> Result<Wav, WaveformError> {
    let reader = hound::WavReader::open(path).map_err(|source| WaveformError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "zero channels or sample rate".to_owned(),
        });
    }
    Ok(reader)
}

/// Every supported sample format as one stream of `f32`.
///
/// ONE dispatch, used by the pyramid and by the sample windows alike:
/// two copies of this match is two places for a bit depth to be handled
/// differently, and a window that scaled 24-bit audio differently from
/// the pyramid would draw a picture that jumped when you zoomed into it.
fn samples_of<'a>(
    reader: &'a mut Wav,
    path: &Path,
) -> Result<Box<dyn Iterator<Item = Result<f32, hound::Error>> + 'a>, WaveformError> {
    let spec = reader.spec();
    Ok(match spec.sample_format {
        hound::SampleFormat::Float => Box::new(reader.samples::<f32>()),
        hound::SampleFormat::Int if (1..=16).contains(&spec.bits_per_sample) => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            Box::new(
                reader
                    .samples::<i16>()
                    .map(move |sample| sample.map(|sample| f32::from(sample) / scale)),
            )
        }
        hound::SampleFormat::Int if (17..=32).contains(&spec.bits_per_sample) => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            Box::new(
                reader
                    .samples::<i32>()
                    .map(move |sample| sample.map(|sample| sample as f32 / scale)),
            )
        }
        _ => {
            return Err(WaveformError::Unsupported {
                path: path.to_path_buf(),
                reason: format!(
                    "{}-bit {:?}, {} channels",
                    spec.bits_per_sample, spec.sample_format, spec.channels
                ),
            });
        }
    })
}

fn build_peaks(path: &Path) -> Result<Peaks, WaveformError> {
    let mut reader = open_wav(path)?;
    let spec = reader.spec();
    let samples = samples_of(&mut reader, path)?;
    build_from_samples(path, spec.sample_rate, spec.channels, samples)
}

/// Decode `key`'s frames, and no more.
///
/// Seeks rather than decoding from the top: at maximum zoom into the end
/// of a long file the frames wanted are a millionth of what is there, and
/// reading past them all to reach them would make zooming feel like
/// loading.
fn read_window(key: &WindowKey) -> Result<SampleWindow, WaveformError> {
    let path = key.path.as_path();
    let mut reader = open_wav(path)?;
    let channels = usize::from(reader.spec().channels);
    let total = u64::from(reader.duration());
    let start = key.start.min(total);
    let frames = key.frames.min(WINDOW_MAX_FRAMES).min(total - start);
    let seek = u32::try_from(start).map_err(|_| WaveformError::Unsupported {
        path: path.to_path_buf(),
        reason: "file is longer than a WAV frame index".to_owned(),
    })?;
    reader
        .seek(seek)
        .map_err(|error| WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: format!("cannot seek to frame {start}: {error}"),
        })?;
    let wanted = (frames as usize).saturating_mul(channels);
    let mut samples = Vec::with_capacity(wanted);
    for sample in samples_of(&mut reader, path)?.take(wanted) {
        let sample = sample.map_err(|source| WaveformError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
        samples.push(if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        });
    }
    // A short read is honest about its length rather than padded: the
    // drawing asks whether a frame is in the window, and silence invented
    // here would be a picture of audio that is not on disk.
    let frames = (samples.len() / channels.max(1)) as u64;
    samples.truncate(frames as usize * channels);
    Ok(SampleWindow {
        key: WindowKey {
            path: key.path.clone(),
            start,
            frames,
        },
        channels,
        samples,
    })
}

fn build_from_samples<I>(
    path: &Path,
    sample_rate: u32,
    channel_count: u16,
    samples: I,
) -> Result<Peaks, WaveformError>
where
    I: Iterator<Item = Result<f32, hound::Error>>,
{
    let channels = usize::from(channel_count);
    let mut base = (0..channels).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut mins = vec![f32::INFINITY; channels];
    let mut maxs = vec![f32::NEG_INFINITY; channels];
    let mut energy = vec![0.0f64; channels];
    let mut sample_count = 0usize;
    let mut frames = 0u64;
    let mut frames_in_peak = 0u64;

    for sample in samples {
        let sample = sample.map_err(|source| WaveformError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
        let channel = sample_count % channels;
        let sample = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        mins[channel] = mins[channel].min(sample);
        maxs[channel] = maxs[channel].max(sample);
        energy[channel] += f64::from(sample) * f64::from(sample);
        sample_count += 1;

        if channel + 1 == channels {
            frames += 1;
            frames_in_peak += 1;
            if frames_in_peak == BASE_FRAMES_PER_PEAK {
                push_peak(&mut base, &mut mins, &mut maxs, &mut energy, frames_in_peak);
                frames_in_peak = 0;
            }
        }
    }

    if !sample_count.is_multiple_of(channels) {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "partial interleaved frame".to_owned(),
        });
    }
    if frames_in_peak > 0 {
        push_peak(&mut base, &mut mins, &mut maxs, &mut energy, frames_in_peak);
    }
    if frames == 0 {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "file contains no audio frames".to_owned(),
        });
    }

    let mut levels = vec![PeakLevel {
        frames_per_peak: BASE_FRAMES_PER_PEAK,
        channels: base,
    }];
    loop {
        let bin_count = levels
            .last()
            .and_then(|level| level.channels.first())
            .map_or(0, Vec::len);
        if bin_count <= 1 {
            break;
        }
        let next = reduce_level(&levels[levels.len() - 1]);
        levels.push(next);
    }

    Ok(Peaks {
        sample_rate,
        channels: channel_count,
        frames,
        levels,
    })
}

fn push_peak(
    base: &mut [Vec<Peak>],
    mins: &mut [f32],
    maxs: &mut [f32],
    energy: &mut [f64],
    frames: u64,
) {
    for channel in 0..base.len() {
        let peak = if mins[channel].is_finite() && maxs[channel].is_finite() {
            Peak {
                min: mins[channel],
                max: maxs[channel],
                rms: (energy[channel] / frames.max(1) as f64).sqrt() as f32,
            }
        } else {
            Peak::SILENCE
        };
        base[channel].push(peak);
        mins[channel] = f32::INFINITY;
        maxs[channel] = f32::NEG_INFINITY;
        energy[channel] = 0.0;
    }
}

fn reduce_level(previous: &PeakLevel) -> PeakLevel {
    let channels = previous
        .channels
        .iter()
        .map(|channel| {
            channel
                .chunks(LEVEL_REDUCTION)
                .map(|chunk| {
                    let Some((first, rest)) = chunk.split_first() else {
                        return Peak::SILENCE;
                    };
                    // Extremes fold; energy is summed across the whole
                    // chunk and divided once, so every child weighs the
                    // same. A pairwise RMS fold would weight the last
                    // child of four at a half rather than a quarter.
                    let mut out = *first;
                    let mut energy = f64::from(first.rms) * f64::from(first.rms);
                    for peak in rest {
                        out.min = out.min.min(peak.min);
                        out.max = out.max.max(peak.max);
                        energy += f64::from(peak.rms) * f64::from(peak.rms);
                    }
                    out.rms = (energy / chunk.len() as f64).sqrt() as f32;
                    out
                })
                .collect()
        })
        .collect();
    PeakLevel {
        frames_per_peak: previous
            .frames_per_peak
            .saturating_mul(LEVEL_REDUCTION as u64),
        channels,
    }
}

/// Which channels an edit covers.
///
/// A bitmask rather than a `Vec<bool>` because a selection is copied on
/// every frame of a drag and a channel count is small — and because
/// "these two channels" is one comparison rather than a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelMask(u32);

impl ChannelMask {
    pub fn all(channels: usize) -> Self {
        Self(if channels >= 32 {
            u32::MAX
        } else {
            (1u32 << channels) - 1
        })
    }

    pub fn single(channel: usize) -> Self {
        let mut mask = Self::default();
        mask.set(channel);
        mask
    }

    pub fn set(&mut self, channel: usize) {
        if channel < 32 {
            self.0 |= 1 << channel;
        }
    }

    pub fn toggle(&mut self, channel: usize) {
        if channel < 32 {
            self.0 ^= 1 << channel;
        }
    }

    pub fn contains(self, channel: usize) -> bool {
        channel < 32 && self.0 & (1 << channel) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The channels it holds, inside a file that has `channels` of them.
    pub fn iter(self, channels: usize) -> impl Iterator<Item = usize> {
        (0..channels).filter(move |channel| self.contains(*channel))
    }
}

/// The renderer speaks the same mask, so a selection made with the
/// pointer is the mask a destructive edit is given — no translation
/// table, no chance of the picture and the edit disagreeing about which
/// side of a stereo file is which.
impl From<ChannelMask> for daw::render::Channels {
    fn from(mask: ChannelMask) -> Self {
        Self(mask.0)
    }
}

/// A half-open range of CLIP frames, and which channels it covers.
///
/// View state: a selection is where you are looking, not something the
/// project remembers, so it never reaches a snapshot and never banks an
/// undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub from: u64,
    pub to: u64,
    pub channels: ChannelMask,
}

impl Selection {
    pub fn frames(self) -> u64 {
        self.to.saturating_sub(self.from)
    }
}

/// A rubber-band drag in flight.
#[derive(Debug, Clone, Copy)]
struct SelectionDrag {
    /// The frame the press landed on. The other end follows the pointer.
    anchor: u64,
    /// Every lane the drag has touched, which is what it selects — a
    /// gesture that crossed a channel meant to include it.
    channels: ChannelMask,
}

/// Which end of a selection a drag has hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    From,
    To,
}

/// Render-time transport and grid information. Keeping this together makes
/// the editor's geometry explicit and avoids a wide argument list.
#[derive(Clone, Copy)]
pub struct TimeView<'a> {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub playhead: f32,
    pub grid_beats: f32,
    pub grid_name: &'a str,
    /// The transport is running AND following is on. The editor scrolls
    /// to keep the playhead in view only while both are true.
    pub follow: bool,
}

struct PaintView<'a> {
    display: egui::Rect,
    editor: &'a Editor,
    clip: &'a Clip,
    source: &'a AudioSource,
    peaks: Option<&'a Peaks>,
    time: TimeView<'a>,
    /// The clip's timeline span in frames — the whole of the horizontal
    /// axis, and the ceiling every position here is clamped to.
    span: u64,
    scale: Scale,
}

/// The map between clip frames and screen x.
///
/// One place, handed to every painter, because a picture whose grid and
/// whose waveform disagreed about where a frame sits would be a picture
/// of nothing in particular.
#[derive(Clone, Copy)]
struct Scale {
    left: f32,
    scroll: f64,
    fpp: f64,
}

impl Scale {
    fn x_of(&self, frame: f64) -> f32 {
        self.left + ((frame - self.scroll) / self.fpp.max(f64::MIN_POSITIVE)) as f32
    }

    fn frame_at(&self, x: f32) -> f64 {
        self.scroll + f64::from(x - self.left) * self.fpp
    }

    /// The clip frame under `x`, rounded and held inside the clip.
    fn frame_in(&self, x: f32, span: u64) -> u64 {
        self.frame_at(x).round().clamp(0.0, span as f64) as u64
    }

    /// Pixels per frame — the reciprocal zoom, for deciding whether there
    /// is room to draw one sample at a time.
    fn pixels_per_frame(&self) -> f64 {
        1.0 / self.fpp.max(f64::MIN_POSITIVE)
    }
}

/// How many frames one beat is worth in this clip's material.
fn frames_per_beat(sample_rate: u32, bpm: f64) -> f64 {
    f64::from(sample_rate.max(1)) * 60.0 / bpm.max(1.0)
}

/// A signed sample as the height it draws at, in `-1..=1`.
///
/// Linear is the identity. dB spreads the quiet part of the range over
/// the lane so a −40 dB tail is visible at all, which is the whole reason
/// a sample editor offers the scale — and it keeps the SIGN, so the
/// picture is still a waveform and not a pair of rectified humps.
fn amplitude(value: f32, decibels: bool) -> f32 {
    if !decibels {
        return value;
    }
    let magnitude = value.abs();
    if magnitude <= 0.0 {
        return 0.0;
    }
    let scaled = ((dbfs(magnitude) - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
    if value < 0.0 { -scaled } else { scaled }
}

/// View state only. Audio content and placement remain in [`Clip`].
pub struct Editor {
    shown_clip: Option<u64>,
    /// The clip frame at the display's left edge.
    scroll_frames: f64,
    /// The zoom: clip frames per screen pixel.
    frames_per_pixel: f64,
    pub amplitude_zoom: f32,
    /// Amplitude drawn in dB rather than linear.
    decibels: bool,
    fit_pending: bool,
    pub owns_keys: bool,
    /// The selected range, or none. See [`Selection`].
    selection: Option<Selection>,
    /// Where a paste lands and where playing the clip begins. Distinct
    /// from the transport's playhead, which is the engine's business.
    cursor: u64,
    /// A rubber-band gesture in flight.
    drag: Option<SelectionDrag>,
    /// Edges land on zero crossings, so a cut does not click.
    snap_zero: bool,
    /// A length-changing edit takes the clip's timeline length with it,
    /// and moves whatever comes after on the lane.
    ///
    /// Off by default: holding the clip's boundaries still is the less
    /// surprising of the two, and the one that cannot disturb material
    /// somewhere else on the timeline that the user is not looking at.
    ripple: bool,
    /// The span, channel count and display width the last frame drew, so
    /// a command from the palette knows how much "all" is, and how much
    /// room a zoom has, without being handed the clip.
    span: u64,
    channels: usize,
    display_width: f32,
    /// What "apply gain" will apply, in dB.
    ///
    /// The EDITOR's state, not the clip's: it is a setting on a tool, the
    /// way a brush size is, and putting it on the clip would make every
    /// clip remember a number it never uses.
    edit_gain_db: f32,
    /// The gain envelope is drawn and editable.
    ///
    /// HIDDEN IS NOT BYPASSED. A hidden envelope still plays; this only
    /// decides whether it is on screen and under the pointer, and the
    /// header says so.
    show_envelope: bool,
    /// A verb chosen from the context menu, waiting for the app to run
    /// it. The editor knows the names and nothing else — enablement,
    /// meaning and the render all belong to the app, exactly as with
    /// every other wish this view hands back.
    command: Option<&'static str>,
    /// The name being typed in the controls column, while it is being
    /// typed. `None` means the field shows the clip's own name.
    ///
    /// Held HERE rather than in the panel because a card is redrawn from
    /// the clip every frame — a buffer the panel owned would be forgotten
    /// between keystrokes. It clears when the shown clip changes, so a
    /// half-typed name cannot follow you onto the next clip.
    name_edit: Option<String>,
    /// The decoded samples behind the current view, when the zoom has
    /// gone past what the pyramid can honestly draw. View state: a
    /// picture's worth of audio, thrown away as freely as it was asked
    /// for.
    window: Option<Arc<SampleWindow>>,
    /// A request already with the worker, so a scroll does not ask for
    /// the same frames sixty times before the first answer lands.
    window_pending: Option<WindowKey>,
    /// What the last frame decided it needs. The app drains this and
    /// speaks to the service — the editor is a view and holds no handle
    /// to a worker, exactly as it holds none to the engine.
    window_want: Option<WindowKey>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            shown_clip: None,
            scroll_frames: 0.0,
            frames_per_pixel: 1_024.0,
            amplitude_zoom: 1.0,
            decibels: false,
            fit_pending: true,
            owns_keys: false,
            selection: None,
            cursor: 0,
            drag: None,
            snap_zero: false,
            ripple: false,
            span: 0,
            channels: 1,
            display_width: 0.0,
            edit_gain_db: -6.0,
            show_envelope: false,
            command: None,
            name_edit: None,
            window: None,
            window_pending: None,
            window_want: None,
        }
    }
}

impl Editor {
    pub fn follow_clip(&mut self, clip: Option<u64>) {
        if self.shown_clip != clip {
            self.shown_clip = clip;
            self.scroll_frames = 0.0;
            self.amplitude_zoom = 1.0;
            self.fit_pending = true;
            self.name_edit = None;
            self.window_want = None;
            // A selection belongs to the clip it was made in. Carrying it
            // onto the next one would point a destructive verb at audio
            // the user never looked at.
            self.selection = None;
            self.cursor = 0;
            self.drag = None;
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn edit_gain_db(&self) -> f32 {
        self.edit_gain_db
    }

    pub fn take_command(&mut self) -> Option<&'static str> {
        self.command.take()
    }

    pub fn toggle_envelope(&mut self) {
        self.show_envelope = !self.show_envelope;
    }

    pub fn toggle_snap_zero(&mut self) {
        self.snap_zero = !self.snap_zero;
    }

    pub fn ripple(&self) -> bool {
        self.ripple
    }

    pub fn toggle_ripple(&mut self) {
        self.ripple = !self.ripple;
    }

    /// The whole clip, every channel.
    pub fn select_all(&mut self) {
        if self.span == 0 {
            return;
        }
        self.selection = Some(Selection {
            from: 0,
            to: self.span,
            channels: ChannelMask::all(self.channels),
        });
        self.cursor = 0;
    }

    /// Drop the selection, keeping the cursor: clearing what is selected
    /// should not also forget where you were.
    pub fn select_none(&mut self) {
        self.selection = None;
        self.drag = None;
    }

    /// Fit the selection to the display, with a little air either side.
    pub fn zoom_to_selection(&mut self) {
        let Some(selection) = self.selection else {
            return;
        };
        let frames = selection.frames();
        if frames == 0 || self.display_width <= 0.0 {
            return;
        }
        let margin = frames as f64 * SELECTION_ZOOM_MARGIN;
        self.frames_per_pixel = ((frames as f64 + margin * 2.0) / f64::from(self.display_width))
            .clamp(FPP_MIN, FPP_MAX);
        self.scroll_frames = (selection.from as f64 - margin).max(0.0);
        self.fit_pending = false;
    }

    /// Move the cursor, or drag the selection's far end with it.
    ///
    /// `extend` is Shift held: the cursor is the moving end and whatever
    /// the selection's other end was stays put, which is how a keyboard
    /// grows a selection in every editor that has one.
    pub fn move_cursor(&mut self, to: u64, extend: bool) {
        let to = to.min(self.span);
        if !extend {
            self.cursor = to;
            self.selection = None;
            return;
        }
        let anchor = match self.selection {
            // The end that is NOT under the cursor is the one that stays.
            Some(selection) if selection.to == self.cursor => selection.from,
            Some(selection) if selection.from == self.cursor => selection.to,
            Some(selection) => selection.from,
            None => self.cursor,
        };
        let channels = self
            .selection
            .map_or_else(|| ChannelMask::all(self.channels), |s| s.channels);
        self.set_selection(anchor, to, channels);
        self.cursor = to;
    }

    /// Step the cursor by `step` frames, forwards or back.
    pub fn nudge_cursor(&mut self, step: i64, extend: bool) {
        let to = (self.cursor as i64)
            .saturating_add(step)
            .clamp(0, self.span as i64) as u64;
        self.move_cursor(to, extend);
    }

    fn set_selection(&mut self, from: u64, to: u64, channels: ChannelMask) {
        let (from, to) = (from.min(to), from.max(to));
        self.selection =
            (to > from && !channels.is_empty()).then_some(Selection { from, to, channels });
        self.cursor = from;
    }

    /// Zoom by notches, positive being IN. Frames per pixel therefore
    /// falls as the number rises, which is the one place this unit reads
    /// backwards from the old one.
    pub fn step_zoom(&mut self, notches: i32) {
        self.frames_per_pixel =
            (self.frames_per_pixel / ZOOM_STEP.powi(notches)).clamp(FPP_MIN, FPP_MAX);
        self.fit_pending = false;
    }

    pub fn fit(&mut self) {
        self.fit_pending = true;
    }

    pub fn toggle_decibels(&mut self) {
        self.decibels = !self.decibels;
    }

    /// What the editor wants decoded, handed to the app once. Taking it
    /// records it as outstanding, so the next frame asks for nothing.
    pub fn take_window_request(&mut self) -> Option<WindowKey> {
        let want = self.window_want.take()?;
        self.window_pending = Some(want.clone());
        Some(want)
    }

    /// A worker's answer. A window for a request the view has already
    /// scrolled past is still kept — it costs nothing and it may well
    /// cover where the view went.
    pub fn accept_window(&mut self, result: WindowResult) {
        if self.window_pending.as_ref() == Some(&result.key) {
            self.window_pending = None;
        }
        if let Ok(window) = result.result {
            self.window = Some(window);
        }
    }

    /// The samples behind `path`, if the resident window is that file's.
    fn samples_for(&self, path: &Path) -> Option<&SampleWindow> {
        self.window
            .as_deref()
            .filter(|window| window.key.path == path)
    }
}

/// What the editor's keys need to know that its own state does not say.
#[derive(Debug, Clone, Copy)]
pub struct KeyContext {
    pub has_clip: bool,
    /// One grid unit in clip frames — what an arrow key moves the cursor
    /// by. Zero falls back to a single frame, so the arrows still work
    /// with the grid switched off.
    pub grid_frames: u64,
    /// There is audio on the clipboard. The editor cannot know this, and
    /// it decides whether Ctrl+V is the editor's key or the timeline's.
    pub can_paste: bool,
}

/// The editor's keyboard.
///
/// THE CLIPBOARD KEYS SHADOW THE TIMELINE'S. While this editor has focus
/// and a range selected, Ctrl+C, Ctrl+X, Ctrl+V and Delete act on audio
/// rather than on clips — because the thing under the user's attention is
/// a range of samples, and copying the whole clip instead would be a
/// surprising answer to a key pressed while looking at a selection.
///
/// It works by CONSUMING the key here, and this function running before
/// `arrangement_keys` does. Anything not consumed falls through to the
/// timeline untouched, so with nothing selected Ctrl+C still copies the
/// clip.
pub fn keys(ctx: &egui::Context, editor: &mut Editor, context: KeyContext) {
    if !context.has_clip || !editor.owns_keys || ctx.egui_wants_keyboard_input() {
        return;
    }
    let step = context.grid_frames.max(1) as i64;
    ctx.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
        {
            editor.step_zoom(1);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Minus) {
            editor.step_zoom(-1);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::F) {
            editor.fit();
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::D) {
            editor.toggle_decibels();
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Z) {
            editor.toggle_snap_zero();
        }
        if input.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
            editor.select_all();
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            editor.select_none();
        }
        if input.consume_key(egui::Modifiers::COMMAND, egui::Key::F) {
            editor.zoom_to_selection();
        }
        // Home and End are the clip's ends; Shift takes the selection
        // with them, which is how a keyboard selects to a boundary.
        for (modifiers, extend) in [
            (egui::Modifiers::NONE, false),
            (egui::Modifiers::SHIFT, true),
        ] {
            if input.consume_key(modifiers, egui::Key::Home) {
                editor.move_cursor(0, extend);
            }
            if input.consume_key(modifiers, egui::Key::End) {
                let span = editor.span;
                editor.move_cursor(span, extend);
            }
            if input.consume_key(modifiers, egui::Key::ArrowLeft) {
                editor.nudge_cursor(-step, extend);
            }
            if input.consume_key(modifiers, egui::Key::ArrowRight) {
                editor.nudge_cursor(step, extend);
            }
        }
        // The shadowing set. Each one is consumed ONLY when it has
        // something here to act on; otherwise it is left for the
        // timeline, which is the whole point of checking rather than
        // consuming unconditionally.
        let selected = editor.selection.is_some();
        if selected && input.consume_key(egui::Modifiers::COMMAND, egui::Key::C) {
            editor.command = Some("audio.copy");
        }
        if selected && input.consume_key(egui::Modifiers::COMMAND, egui::Key::X) {
            editor.command = Some("audio.cut");
        }
        if context.can_paste && input.consume_key(egui::Modifiers::COMMAND, egui::Key::V) {
            editor.command = Some("audio.paste");
        }
        if selected
            && (input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        {
            editor.command = Some("audio.delete");
        }
    });
}

/// Split the editor into its controls column and everything else.
///
/// Pure, and separate from the drawing, so a test can say where the seam
/// lands without running a frame — and so the display's own geometry has
/// one place to come from.
///
/// A region too narrow to hold both gives the column NOTHING rather than
/// half of a column: at that width the waveform is the thing worth
/// seeing, and a squeezed column of controls is neither usable nor
/// readable.
fn split_controls(area: egui::Rect) -> (egui::Rect, egui::Rect) {
    let want = control::SIDE_COLUMN_W;
    if area.width() < want + MIN_DISPLAY_W {
        // A ZERO-WIDTH rect on the left edge, not `Rect::NOTHING` —
        // that one measures −∞ wide, which is a fine sentinel and a
        // terrible number for whatever lays controls out against it.
        let none = egui::Rect::from_min_max(area.min, egui::pos2(area.left(), area.bottom()));
        return (none, area);
    }
    let seam = area.left() + want;
    (
        egui::Rect::from_min_max(area.min, egui::pos2(seam, area.bottom())),
        egui::Rect::from_min_max(egui::pos2(seam, area.top()), area.max),
    )
}

/// What the clip editor's controls changed, on their way back to the
/// arrangement.
///
/// Returned rather than written, the way every other region here hands
/// its wishes back: the editor is a VIEW over a clip it does not own, and
/// the app is the only thing that may also tell the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipEdit {
    /// Linear gain, the units `params::clip::GAIN` and the node both use.
    Gain(f32),
    Looped(bool),
    Reversed(bool),
    /// Fade length in FRAMES of the clip's timeline span — the node's own
    /// units, so the handle and the envelope are the same number.
    FadeIn(u64),
    FadeOut(u64),
    /// A fade's SHAPE, in `-1..=1`. Zero is linear.
    FadeInCurve(f32),
    FadeOutCurve(f32),
    /// The clip's whole gain envelope, `(frame, dB)`, sorted.
    ///
    /// The WHOLE list rather than one point's move, because a drag past a
    /// neighbour reorders it and "point 3 moved" would then name a
    /// different point on each side of the swap. A clip's envelope is a
    /// handful of pairs; sending all of them is cheaper than being
    /// careful about which one.
    Envelope(Vec<(u64, f32)>),
    /// Where in the FILE the clip starts, in frames. Slides the window;
    /// see `paint_controls`.
    SourceOffset(u64),
    /// Never empty — a clip with no name is a clip you cannot find.
    Rename(String),
}

/// GAIN, as the device layer would draw it.
///
/// Deliberately the same `Param` the cards speak, so a clip's gain reads
/// and drags exactly like a device's — the screen guide's density rule is
/// that one grammar repeated beats a second one invented, and a clip
/// editor that had its own idea of a value cell would be a second one.
fn gain_param() -> Param {
    Param::db("gain", -60.0, 12.0).bipolar().with_default(0.0)
}

fn loop_param() -> Param {
    Param::choice("loop", &["off", "on"])
}

fn reverse_param() -> Param {
    Param::choice("reverse", &["off", "on"])
}

/// How many BEATS one frame of this clip is worth, or `None` when there
/// is no length to measure against. The bridge between a fade (frames,
/// the node's units) and the timeline (beats, the ruler's).
pub fn fade_span_beats(clip: &Clip, source: &AudioSource, bpm: f64) -> Option<f32> {
    let frames = clip_span_frames(clip, source, bpm);
    (frames > 0 && clip.len > 0.0).then(|| clip.len / frames as f32)
}

/// A fade's length in seconds, ceilinged by the clip it lives on. A fade
/// longer than its clip never reaches full level, so there is nothing
/// above the ceiling worth being able to ask for.
fn fade_param(name: &'static str, max_seconds: f32) -> Param {
    Param::new(
        name,
        Mapping::Linear {
            min: 0.0,
            max: max_seconds.max(f32::MIN_POSITIVE),
        },
        Unit::Seconds,
    )
    .with_default(0.0)
}

/// The clip's timeline span in FRAMES — what a fade is measured against,
/// and what the node clamps to.
///
/// The ONE definition. The timeline draws the same fades on the same
/// clips, and two functions agreeing today is two functions that can
/// disagree tomorrow — a handle that sat at a different place in the
/// arrangement than in the editor would be the same fade drawn twice,
/// wrongly, in the same session.
pub fn clip_span_frames(clip: &Clip, source: &AudioSource, bpm: f64) -> u64 {
    let seconds = f64::from(clip.len.max(0.0)) * 60.0 / bpm.max(1.0);
    (seconds * f64::from(source.sample_rate.max(1)))
        .round()
        .max(0.0) as u64
}

/// The fade handles: two grips on the top edge, one per end.
///
/// ON THE PICTURE, not in the panel, because a fade is a shape and the
/// waveform is where its shape is visible. The panel prints the number,
/// which is the division of labour every other control here keeps — the
/// EQ's bands, the delay's taps, the compressor's threshold.
///
/// Each grip owns its own egui interaction, so a drag belongs to the end
/// it started on however far the pointer travels; that is the device UI
/// contract's first rule and the reason this is not one widget doing
/// arithmetic about which half of the clip you are nearer.
fn fade_handles(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PaintView<'_>,
    span: u64,
) -> Option<ClipEdit> {
    if span == 0 || view.clip.len <= 0.0 {
        return None;
    }
    // A fade is measured in frames of the clip's span, and so is the
    // display — so the handle needs no conversion at all any more.
    let x_of = |frames: u64| view.scale.x_of(frames as f64);
    let frames_at = |x: f32| view.scale.frame_in(x, span);

    let grab = theme.sp(control::HANDLE) * 2.0;
    let mut edit = None;
    let ends = [
        (view.source.fade_in, view.source.fade_in_curve, true),
        (view.source.fade_out, view.source.fade_out_curve, false),
    ];
    for (frames, shape, leading) in ends {
        let at = if leading {
            x_of(frames)
        } else {
            x_of(span.saturating_sub(frames))
        };
        if at < view.display.left() - grab || at > view.display.right() + grab {
            continue;
        }
        let hit = egui::Rect::from_center_size(
            egui::pos2(at, view.display.top() + grab),
            egui::vec2(grab * 2.0, grab * 2.0),
        );
        let id = ui.id().with(("fade", leading));
        let response = ui
            .interact(hit, id, egui::Sense::click_and_drag())
            .affords(Affords::SeamX);
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let want = frames_at(pos.x);
            let next = if leading {
                want.min(span)
            } else {
                span.saturating_sub(want.min(span))
            };
            if next != frames {
                edit = Some(if leading {
                    ClipEdit::FadeIn(next)
                } else {
                    ClipEdit::FadeOut(next)
                });
            }
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let live = response.hovered() || response.dragged();
        // The ramp itself, drawn as the CURVE the gain actually follows.
        // A straight line here while the engine applied a shape would be
        // a picture of a different fade than the one playing — the same
        // failure the reversed waveform's mirror exists to avoid.
        let curve = daw::params::clip::Curve::new(shape);
        let (from, to) = if leading {
            (view.display.left().max(x_of(0)), at)
        } else {
            (at, x_of(span))
        };
        if to > from {
            let steps = ((to - from).abs().ceil() as usize).clamp(2, 256);
            let points: Vec<egui::Pos2> = (0..=steps)
                .map(|step| {
                    let along = step as f32 / steps as f32;
                    let x = from + (to - from) * along;
                    // A fade IN rises across its span; a fade OUT falls,
                    // which is the same curve read from the other end.
                    let level = curve.at(if leading { along } else { 1.0 - along });
                    egui::pos2(x, view.display.bottom() - level * view.display.height())
                })
                .collect();
            ui.painter().add(egui::Shape::line(
                points,
                egui::Stroke::new(stroke::HAIR, theme.clip_selected),
            ));
        }
        ui.painter().circle_filled(
            egui::pos2(at, view.display.top() + grab),
            if live { grab * 0.5 } else { grab * 0.35 },
            theme.clip_selected,
        );

        // The SHAPE's own grip, on the curve at its midpoint.
        //
        // Its own interaction with its own id, so a shape drag belongs to
        // the end it started on however far the pointer travels — and so
        // it cannot be confused with the length grip a few pixels away.
        if to > from + grab * 2.0 {
            let mid_x = (from + to) * 0.5;
            let level = curve.at(0.5);
            let mid_y = view.display.bottom() - level * view.display.height();
            let hit = egui::Rect::from_center_size(
                egui::pos2(mid_x, mid_y),
                egui::vec2(grab * 2.0, grab * 2.0),
            );
            let id = ui.id().with(("fade_curve", leading));
            let response = ui
                .interact(hit, id, egui::Sense::click_and_drag())
                .affords(Affords::SeamX);
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            // VERTICAL ONLY. The point lives on the curve's midpoint in
            // x; letting it drift horizontally would mean tracking a
            // second number the engine has no field for.
            if response.dragged() {
                let moved = -response.drag_delta().y / view.display.height().max(1.0);
                let want = (shape + moved * 2.0).clamp(-1.0, 1.0);
                if want != shape {
                    edit = Some(if leading {
                        ClipEdit::FadeInCurve(want)
                    } else {
                        ClipEdit::FadeOutCurve(want)
                    });
                }
            }
            // Double-click puts the shape back to straight, which is the
            // only value anyone ever wants to return to exactly.
            if response.double_clicked() {
                edit = Some(if leading {
                    ClipEdit::FadeInCurve(0.0)
                } else {
                    ClipEdit::FadeOutCurve(0.0)
                });
            }
            let live = response.hovered() || response.dragged();
            ui.painter().circle_stroke(
                egui::pos2(mid_x, mid_y),
                if live { grab * 0.5 } else { grab * 0.3 },
                egui::Stroke::new(stroke::HAIR, theme.clip_selected),
            );
        }
    }
    edit
}

/// WHERE IN THE FILE the clip starts, in seconds. Its ceiling is the
/// room the file has left once this clip's own duration is accounted
/// for, so the control cannot ask for material that is not there.
fn start_param(max_seconds: f32) -> Param {
    Param::new(
        "start",
        Mapping::Linear {
            min: 0.0,
            max: max_seconds.max(f32::MIN_POSITIVE),
        },
        Unit::Seconds,
    )
    .with_default(0.0)
}

/// Linear gain as the dB a control speaks, and back.
fn gain_db(gain: f32) -> f32 {
    if gain > 0.0 {
        (20.0 * gain.log10()).clamp(-60.0, 12.0)
    } else {
        -60.0
    }
}

fn gain_linear(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// The controls column: what this clip is, and the two things about it
/// worth changing from here.
///
/// Only what a clip ACTUALLY carries. `AudioSource` has a gain, a loop
/// flag, an offset and a length; it has no pitch, no fades and no warp,
/// so this panel offers none — a control that moves nothing is worse than
/// a panel that is honestly short, and this card layer has been bitten by
/// exactly that before (a delay printing a time its engine was ignoring).
fn paint_controls(
    ui: &mut egui::Ui,
    theme: &Theme,
    editor: &mut Editor,
    column: egui::Rect,
    clip: Option<&Clip>,
    peaks: Option<&Peaks>,
    bpm: f64,
) -> Option<ClipEdit> {
    if column.width() <= 0.0 {
        return None;
    }
    let painter = ui.painter().clone();
    painter.rect_filled(column, 0.0, theme.surface);
    // ONE seam, on the edge it shares with the waveform. No box: the fill
    // does the grouping, and a border around a filled region is the
    // "borders around borders" the design guide rules out.
    painter.line_segment(
        [column.right_top(), column.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let pad = LABEL_PAD * 2.0;
    let inner = egui::Rect::from_min_max(
        column.min + egui::vec2(pad, 0.0),
        egui::pos2(column.right() - pad, column.bottom()),
    );
    if inner.width() <= 0.0 {
        return None;
    }

    // The title strip, at the waveform header's own height so the two
    // line up across the seam.
    let title_h = HEADER_H.min(column.height());
    painter.line_segment(
        [
            egui::pos2(column.left(), column.top() + title_h),
            egui::pos2(column.right(), column.top() + title_h),
        ],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let mut edit = None;
    match clip {
        // The title IS the rename. Renaming a clip was reachable only
        // from a context menu and the palette — that is to say, not from
        // the place you are already looking at its name.
        Some(clip) => {
            let field = egui::Rect::from_min_max(
                egui::pos2(inner.left(), column.top()),
                egui::pos2(inner.right(), column.top() + title_h),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(field)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let text = editor.name_edit.get_or_insert_with(|| clip.name.clone());
            let response = child.add(
                egui::TextEdit::singleline(text)
                    .frame(egui::Frame::NONE)
                    .desired_width(field.width())
                    .font(egui::FontId::new(
                        font::LABEL,
                        egui::FontFamily::Proportional,
                    ))
                    .text_color(theme.text),
            );
            // ON THE WAY OUT, not per keystroke: a name settles when you
            // are done with it, the same rule the timeline's own inline
            // rename keeps. Never empty — a clip with no name is a clip
            // you cannot find in a list.
            let settled = response.lost_focus() || child.input(|i| i.key_pressed(egui::Key::Enter));
            if settled {
                let typed = text.trim().to_owned();
                if !typed.is_empty() && typed != clip.name {
                    edit = Some(ClipEdit::Rename(typed));
                }
                editor.name_edit = None;
            }
        }
        None => {
            painter.text(
                egui::pos2(inner.left(), column.top() + title_h * 0.5),
                egui::Align2::LEFT_CENTER,
                "clip",
                egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
                theme.text_muted,
            );
        }
    }

    let source = clip.and_then(|clip| clip.audio.as_ref())?;
    let span = clip.map_or(0, |clip| clip_span_frames(clip, source, bpm));

    // What the file IS, in one quiet line. Facts, not controls: the rate
    // and the length are not yours to change here, and printing them as
    // cells would say they were.
    let seconds = source.source_frames as f32 / source.sample_rate.max(1) as f32;
    painter.text(
        egui::pos2(inner.left(), column.top() + title_h + LABEL_PAD * 2.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{:.1} kHz · {seconds:.2} s",
            source.sample_rate as f32 / 1000.0
        ),
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        theme.text_muted,
    );

    // --- the controls ----------------------------------------------------
    let cell_h = control::POLY_CELL_H * 2.0;
    let top = column.top() + title_h + LABEL_PAD * 5.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(inner.left(), top),
                egui::pos2(inner.right(), column.bottom()),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_width(inner.width());

    let param = gain_param();
    let mut norm = param.mapping.to_norm(gain_db(source.gain));
    child.allocate_ui_with_layout(
        egui::vec2(inner.width(), cell_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(inner.width());
            ui.set_height(cell_h);
            if poly_widgets::labeled_cell_bar(ui, theme, &param, &mut norm, None) {
                edit = Some(ClipEdit::Gain(gain_linear(param.value(norm))));
            }
        },
    );

    let looping = loop_param();
    let mut on = looping.at_index(usize::from(source.looped));
    child.allocate_ui_with_layout(
        egui::vec2(inner.width(), cell_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(inner.width());
            ui.set_height(cell_h);
            if poly_widgets::labeled_cell_steps(ui, theme, &looping, &mut on, None) {
                edit = Some(ClipEdit::Looped(looping.index(on) == 1));
            }
        },
    );

    let reverse = reverse_param();
    let mut backwards = reverse.at_index(usize::from(source.reversed));
    child.allocate_ui_with_layout(
        egui::vec2(inner.width(), cell_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(inner.width());
            ui.set_height(cell_h);
            if poly_widgets::labeled_cell_steps(ui, theme, &reverse, &mut backwards, None) {
                edit = Some(ClipEdit::Reversed(reverse.index(backwards) == 1));
            }
        },
    );

    // The FADES' numbers. The shape is on the waveform where a shape
    // belongs; this is the precise value beside it, which is the split
    // every other control here keeps.
    if span > 0 {
        let rate = source.sample_rate.max(1) as f32;
        let ceiling = span as f32 / rate;
        for (name, frames, leading) in [
            ("fade in", source.fade_in, true),
            ("fade out", source.fade_out, false),
        ] {
            let param = fade_param(name, ceiling);
            let mut norm = param.mapping.to_norm(frames as f32 / rate);
            child.allocate_ui_with_layout(
                egui::vec2(inner.width(), cell_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(inner.width());
                    ui.set_height(cell_h);
                    if poly_widgets::labeled_cell_bar(ui, theme, &param, &mut norm, None) {
                        let want = (param.value(norm) * rate).round().max(0.0) as u64;
                        edit = Some(if leading {
                            ClipEdit::FadeIn(want.min(span))
                        } else {
                            ClipEdit::FadeOut(want.min(span))
                        });
                    }
                },
            );
        }
    }

    // WHAT A DESTRUCTIVE GAIN WILL APPLY.
    //
    // Only while something is selected, because that is the only time
    // the verb it feeds can run — a number sitting there with nothing to
    // spend itself on is the "control that moves nothing" this column's
    // own comment warns about.
    //
    // Visibly apart from the controls above it: those describe the CLIP,
    // this one describes the next EDIT, and a reader must not have to
    // work out which of the two a cell belongs to.
    if editor.selection.is_some() {
        let param = Param::db("apply gain", -24.0, 24.0)
            .bipolar()
            .with_default(0.0);
        let mut norm = param.mapping.to_norm(editor.edit_gain_db);
        let mut moved = None;
        child.add_space(LABEL_PAD * 2.0);
        child.allocate_ui_with_layout(
            egui::vec2(inner.width(), cell_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(inner.width());
                ui.set_height(cell_h);
                if poly_widgets::labeled_cell_bar(ui, theme, &param, &mut norm, None) {
                    moved = Some(param.value(norm));
                }
            },
        );
        if let Some(db) = moved {
            editor.edit_gain_db = db;
        }
    }

    // START: which part of the FILE this clip plays.
    //
    // It SLIDES the window rather than resizing it — the clip keeps its
    // length on the timeline and its duration in frames, and a different
    // stretch of the file lands inside it. That is the edit people
    // actually want from here ("start the sample a bit later"); resizing
    // is what the clip's own edges on the timeline are for, and
    // `trim_clip_left` already owns that rule.
    //
    // Only once the file's length is known: without it the slide has no
    // ceiling, and a control that let you scroll past the end of a file
    // would answer with silence and no explanation.
    if let Some(file_frames) = peaks.map(Peaks::frames) {
        let rate = source.sample_rate.max(1) as f32;
        let room = file_frames.saturating_sub(source.source_frames);
        if room > 0 {
            let start = start_param(room as f32 / rate);
            let mut norm = start.mapping.to_norm(source.source_offset as f32 / rate);
            child.allocate_ui_with_layout(
                egui::vec2(inner.width(), cell_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(inner.width());
                    ui.set_height(cell_h);
                    if poly_widgets::labeled_cell_bar(ui, theme, &start, &mut norm, None) {
                        let frames = (start.value(norm) * rate).round().max(0.0) as u64;
                        edit = Some(ClipEdit::SourceOffset(frames.min(room)));
                    }
                },
            );
        }
    }
    edit
}

/// Draw the audio clip editor. The waveform is read-only for now; zoom, pan,
/// fit, amplitude scale, source/clip boundaries, grid and playhead are view
/// operations and never trigger an engine recompile.
pub fn body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    editor: &mut Editor,
    clip: Option<&Clip>,
    peaks: Option<&Peaks>,
    time: TimeView<'_>,
) -> Option<ClipEdit> {
    let area = ui.max_rect();
    claim(ui);
    ui.painter().rect_filled(area, 0.0, theme.surface_sunken);
    // The controls column owns the left edge, full height — the browser's
    // width, from the browser's own token, because the two are on screen
    // together and a column that nearly matched would read as a mistake
    // rather than as a second column.
    //
    // It takes its space BEFORE the header splits, so the header belongs
    // to the waveform and not to the whole region: a strip running over
    // the top of both would tie the column to the display it is meant to
    // sit beside.
    let (controls, rest) = split_controls(area);
    let edit = paint_controls(ui, theme, editor, controls, clip, peaks, time.bpm);

    let header = egui::Rect::from_min_max(
        rest.min,
        egui::pos2(rest.right(), (rest.top() + HEADER_H).min(rest.bottom())),
    );
    let display = egui::Rect::from_min_max(egui::pos2(rest.left(), header.bottom()), rest.max);
    ui.painter().rect_filled(header, 0.0, theme.surface);
    ui.painter().line_segment(
        [header.left_bottom(), header.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let Some(clip) = clip else {
        editor.owns_keys = false;
        ui.painter().text(
            rest.center(),
            egui::Align2::CENTER_CENTER,
            "select an audio clip",
            egui::FontId::new(font::BODY, egui::FontFamily::Proportional),
            theme.text_muted,
        );
        return edit;
    };
    let Some(source) = clip.audio.as_ref() else {
        editor.owns_keys = false;
        return edit;
    };
    if display.width() <= 0.0 || display.height() <= 0.0 {
        return edit;
    }

    let span = clip_span_frames(clip, source, time.bpm);
    if editor.fit_pending {
        fit_to(editor, span, display.width());
    }

    let channels = peaks.map_or(1, Peaks::channels).max(1);
    editor.span = span;
    editor.channels = channels;

    let id = ui.id().with("waveform_display");
    let response = ui
        .interact(display, id, egui::Sense::click_and_drag())
        .affords(Affords::Sweep);
    editor.owns_keys = focus.register(id, display);

    editor.display_width = display.width();
    zoom_and_scroll(ui, editor, display, span);
    // FOLLOW BEFORE CLAMPING, so the scroll it asks for is held inside
    // the clip like any other — and never while a drag is in flight,
    // where the view moving under the pointer would fight the hand.
    if time.follow && editor.drag.is_none() && !ui.ctx().egui_is_using_pointer() {
        follow_playhead(editor, clip, source, time, display.width(), span);
    }
    clamp_scroll(editor, display.width(), span);
    plan_window(editor, source, peaks, span, display.width());
    // Snapping needs the resident samples and the editor is about to be
    // borrowed mutably by the gesture, so the window is taken by handle
    // first. An Arc clone a frame is nothing; a borrow fight is a rewrite.
    let window = editor.window.clone();
    let snap = Snap {
        grid: f64::from(time.grid_beats.max(0.0)) * frames_per_beat(source.sample_rate, time.bpm),
        zero: editor.snap_zero,
        span,
        region_end: region_end(source, peaks.map_or(0, Peaks::frames)),
        window: window
            .as_deref()
            .filter(|window| window.key.path == source.path),
    };
    display_gesture(ui, editor, &response, display, channels, source, snap);
    context_menu(&response, editor);

    let scale = Scale {
        left: display.left(),
        scroll: editor.scroll_frames,
        fpp: editor.frames_per_pixel,
    };
    let view = PaintView {
        display,
        editor,
        clip,
        source,
        peaks,
        time,
        span,
        scale,
    };
    paint_header(ui, theme, header, &view, response.hover_pos());
    paint_channels(ui, theme, display, peaks.map_or(1, Peaks::channels));
    if peaks.is_some() {
        paint_waveform(ui, theme, &view);
    } else {
        ui.painter().text(
            display.center(),
            egui::Align2::CENTER_CENTER,
            "building waveform…",
            egui::FontId::new(font::BODY, egui::FontFamily::Proportional),
            theme.text_muted,
        );
    }
    paint_grid(ui, theme, &view);
    paint_selection(ui, theme, &view, channels);
    paint_clip_end(ui, theme, &view);
    paint_playhead(ui, theme, &view);
    paint_cursor(ui, theme, &view);
    // The handles LAST, so they sit above everything they annotate and
    // win the pointer where they overlap the display's own drag. The
    // fades are last of all: they live on the top edge, where the
    // selection's edges also reach, and a fade grip is the smaller
    // target of the two.
    let fades = fade_handles(ui, theme, &view, span);
    let envelope = gain_envelope(ui, theme, editor, display, source, span);
    channel_labels(ui, theme, editor, display, channels);
    selection_handles(ui, theme, editor, display, source, snap);
    fades.or(envelope).or(edit)
}

/// Keep the playhead on screen while the transport runs through this clip.
///
/// It pages rather than centring: a view that recentred every frame turns
/// the waveform into something sliding continuously past a fixed line,
/// which is unreadable at the zooms this editor now reaches.
fn follow_playhead(
    editor: &mut Editor,
    clip: &Clip,
    source: &AudioSource,
    time: TimeView<'_>,
    width: f32,
    span: u64,
) {
    let local = time.playhead - clip.start;
    if local < 0.0 || local > clip.len {
        return;
    }
    let frame = f64::from(local) * frames_per_beat(source.sample_rate, time.bpm);
    let visible = f64::from(width) * editor.frames_per_pixel;
    if visible <= 0.0 || span == 0 {
        return;
    }
    // A margin inside each edge, so the page turns before the playhead
    // reaches the very edge rather than at it.
    let margin = visible * 0.1;
    if frame < editor.scroll_frames + margin || frame > editor.scroll_frames + visible - margin {
        editor.scroll_frames = (frame - margin).max(0.0);
    }
}

fn zoom_and_scroll(ui: &egui::Ui, editor: &mut Editor, display: egui::Rect, span: u64) {
    let (modifiers, scroll) = ui.input(|input| (input.modifiers, input.smooth_scroll_delta));
    if !ui.rect_contains_pointer(display) || scroll == egui::Vec2::ZERO {
        return;
    }
    if modifiers.ctrl {
        let amount = f64::from(scroll.y) / 50.0;
        if modifiers.shift {
            editor.amplitude_zoom = (editor.amplitude_zoom
                * (ZOOM_STEP as f32).powf(amount as f32))
            .clamp(AMPLITUDE_MIN, AMPLITUDE_MAX);
        } else {
            // ANCHORED ON THE POINTER: the frame under the cursor stays
            // under the cursor, which is what makes a zoom from the whole
            // clip down to one sample a single continuous gesture rather
            // than a series of jumps you have to chase.
            let x = ui
                .ctx()
                .pointer_latest_pos()
                .map_or(display.center().x, |position| position.x);
            zoom_about(
                editor,
                f64::from(x - display.left()),
                ZOOM_STEP.powf(amount),
            );
        }
        ui.ctx().input_mut(|input| {
            input.smooth_scroll_delta = egui::Vec2::ZERO;
        });
    } else {
        editor.scroll_frames -= f64::from(scroll.x + scroll.y) * editor.frames_per_pixel;
    }
    clamp_scroll(editor, display.width(), span);
}

/// What a candidate frame has to be pulled onto before it counts.
#[derive(Clone, Copy)]
struct Snap<'a> {
    /// The grid's spacing in clip frames. Zero or less means no grid.
    grid: f64,
    zero: bool,
    span: u64,
    region_end: u64,
    window: Option<&'a SampleWindow>,
}

impl Snap<'_> {
    /// Pull `frame` onto the grid, then onto a zero crossing, then hold
    /// it inside the clip. `bypass` is Alt held: a gesture must always
    /// have a way to say "exactly here".
    ///
    /// In that order because the grid is the coarse intent and the
    /// crossing is the fine correction — snapping to a crossing and then
    /// rounding it to the grid would undo the correction entirely.
    fn apply(&self, frame: u64, channel: usize, source: &AudioSource, bypass: bool) -> u64 {
        let mut frame = frame.min(self.span);
        if bypass {
            return frame;
        }
        if self.grid > 0.0 {
            frame = ((frame as f64 / self.grid).round() * self.grid).max(0.0) as u64;
            frame = frame.min(self.span);
        }
        if self.zero
            && let Some(found) = self.zero_crossing(frame, channel, source)
        {
            frame = found.min(self.span);
        }
        frame
    }

    /// The nearest clip frame on either side of `frame` where the
    /// material changes sign.
    ///
    /// Searched in CLIP frames rather than in the file, so a looped or
    /// reversed clip finds the crossing that is actually under the
    /// pointer instead of one somewhere else in the file.
    ///
    /// Bounded by [`ZERO_CROSSING_REACH`]: an unbounded search walks a
    /// whole DC-offset recording to find nothing, and a crossing that far
    /// away is not where the user pointed.
    fn zero_crossing(&self, frame: u64, channel: usize, source: &AudioSource) -> Option<u64> {
        let window = self.window?;
        let sample = |at: u64| {
            source_frame_of(source, self.region_end, at)
                .and_then(|source| window.at(channel, source))
        };
        let here = sample(frame)? >= 0.0;
        for step in 1..=ZERO_CROSSING_REACH {
            for candidate in [frame.checked_add(step), frame.checked_sub(step)] {
                let Some(candidate) = candidate.filter(|at| *at <= self.span) else {
                    continue;
                };
                if sample(candidate).is_some_and(|value| (value >= 0.0) != here) {
                    // The crossing lies BETWEEN two frames; name the one
                    // on the far side of it from where we started, so the
                    // snap lands after the sign has actually changed.
                    return Some(candidate);
                }
            }
        }
        None
    }
}

/// The dB an envelope point can be dragged between. The same numbers the
/// app converts with — stated once, here, because a picture drawn against
/// a different range than the model clamps to would let a point be
/// dragged somewhere it does not stay.
const ENVELOPE_FLOOR_DB: f32 = -60.0;
const ENVELOPE_CEIL_DB: f32 = 6.0;

/// A point's dB as a fraction of the lane's height, top being the ceiling.
fn envelope_y(display: egui::Rect, db: f32) -> f32 {
    let span = ENVELOPE_CEIL_DB - ENVELOPE_FLOOR_DB;
    let along = ((db - ENVELOPE_FLOOR_DB) / span).clamp(0.0, 1.0);
    display.bottom() - along * display.height()
}

/// And back: a y as the dB it stands for.
fn envelope_db(display: egui::Rect, y: f32) -> f32 {
    let along = ((display.bottom() - y) / display.height().max(1.0)).clamp(0.0, 1.0);
    ENVELOPE_FLOOR_DB + along * (ENVELOPE_CEIL_DB - ENVELOPE_FLOOR_DB)
}

/// The gain ride: a polyline over the waveform, and a grip per point.
///
/// ONE INTERACTION PER POINT, each with its own id, so a point dragged
/// past its neighbour keeps the gesture — the device UI contract's first
/// rule, and the same trap the selection edges fell into.
///
/// A double-click on the LINE adds a point and a double-click on a POINT
/// removes it. The line's own interaction is allocated first so the
/// points win where they overlap it.
fn gain_envelope(
    ui: &mut egui::Ui,
    theme: &Theme,
    editor: &Editor,
    display: egui::Rect,
    source: &AudioSource,
    span: u64,
) -> Option<ClipEdit> {
    if !editor.show_envelope || span == 0 {
        return None;
    }
    let scale = Scale {
        left: display.left(),
        scroll: editor.scroll_frames,
        fpp: editor.frames_per_pixel,
    };
    // The drawn shape always has ends, so an empty envelope reads as the
    // flat unity line it actually is rather than as nothing at all.
    let mut points = source.envelope.clone();
    if points.is_empty() {
        points.push((0, 0.0));
        points.push((span, 0.0));
    }
    let mut edit = None;

    let line: Vec<egui::Pos2> = points
        .iter()
        .map(|(at, db)| egui::pos2(scale.x_of(*at as f64), envelope_y(display, *db)))
        .collect();
    // Held flat past the last point and before the first, which is what
    // the node does — the picture and the sound agree at the edges.
    let mut drawn = Vec::with_capacity(line.len() + 2);
    if let Some(first) = line.first() {
        drawn.push(egui::pos2(display.left(), first.y));
    }
    drawn.extend(line.iter().copied());
    if let Some(last) = line.last() {
        drawn.push(egui::pos2(display.right(), last.y));
    }
    let stroke = egui::Stroke::new(stroke::BOLD, theme.role_mod);

    // The LINE first: a double-click anywhere on it adds a point there.
    let line_id = ui.id().with("envelope_line");
    let response = ui
        .interact(display, line_id, egui::Sense::click())
        .affords(Affords::Draw);
    if response.double_clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let frame = scale.frame_in(at.x, span);
        let db = envelope_db(display, at.y);
        let mut next = source.envelope.clone();
        let index = next.partition_point(|(existing, _)| *existing < frame);
        next.insert(index, (frame, db));
        next.dedup_by_key(|(existing, _)| *existing);
        edit = Some(ClipEdit::Envelope(next));
    }
    ui.painter().add(egui::Shape::line(drawn, stroke));

    let grab = theme.sp(control::HANDLE) * 1.5;
    for (index, at) in line.iter().enumerate() {
        if at.x < display.left() - grab || at.x > display.right() + grab {
            continue;
        }
        let hit = egui::Rect::from_center_size(*at, egui::vec2(grab * 2.0, grab * 2.0));
        let id = ui.id().with(("envelope_point", index));
        let response = ui
            .interact(hit, id, egui::Sense::click_and_drag())
            .affords(Affords::Steer);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let mut next = if source.envelope.is_empty() {
                points.clone()
            } else {
                source.envelope.clone()
            };
            if let Some(point) = next.get_mut(index) {
                // Clamped BETWEEN ITS NEIGHBOURS in x: two points at the
                // same frame make a vertical step, and the node's
                // interpolation would divide by their zero-length gap.
                let low = index
                    .checked_sub(1)
                    .and_then(|before| next_frame_before(&points, before))
                    .unwrap_or(0);
                let high = point_frame_after(&points, index).unwrap_or(span);
                let want = scale.frame_in(pointer.x, span).clamp(low, high);
                point.0 = want;
                point.1 = envelope_db(display, pointer.y);
                edit = Some(ClipEdit::Envelope(next));
            }
        }
        // A point removed by double-clicking it. The first and last are
        // kept: an envelope with no ends has nothing to interpolate
        // between, and "remove them all" is what the toggle is for.
        if response.double_clicked() && !source.envelope.is_empty() && source.envelope.len() > 1 {
            let mut next = source.envelope.clone();
            if index < next.len() {
                next.remove(index);
                edit = Some(ClipEdit::Envelope(next));
            }
        }
        let live = response.hovered() || response.dragged();
        ui.painter()
            .circle_filled(*at, if live { grab } else { grab * 0.6 }, theme.role_mod);
    }
    edit
}

/// The frame of the point just before `index`, plus one — the lowest a
/// point may be dragged to without landing on its neighbour.
fn next_frame_before(points: &[(u64, f32)], index: usize) -> Option<u64> {
    points.get(index).map(|(at, _)| at.saturating_add(1))
}

/// The frame of the point just after `index`, less one.
fn point_frame_after(points: &[(u64, f32)], index: usize) -> Option<u64> {
    points.get(index + 1).map(|(at, _)| at.saturating_sub(1))
}

/// The destructive verbs the display's context menu offers, in the order
/// a sample editor's menu has them: what changes the level, then what
/// changes the shape, then what changes the direction.
///
/// Named by palette id, so there is ONE list of what these verbs are
/// called and the menu cannot drift from the palette.
const CONTEXT_VERBS: &[(&str, &str)] = &[
    ("audio.gain", "apply gain"),
    ("audio.normalize", "normalize"),
    ("audio.fade_in", "fade in"),
    ("audio.fade_out", "fade out"),
    ("audio.silence", "silence"),
    ("audio.reverse_sel", "reverse"),
    ("audio.invert", "invert polarity"),
    ("audio.remove_dc", "remove DC offset"),
];

/// Which lane `y` is in.
fn channel_at(display: egui::Rect, channels: usize, y: f32) -> usize {
    let channels = channels.max(1);
    let lane = display.height() / channels as f32;
    if lane <= 0.0 {
        return 0;
    }
    (((y - display.top()) / lane).floor().max(0.0) as usize).min(channels - 1)
}

/// The display's own pointer work: select with the left button, pan with
/// the middle one.
///
/// LEFT SELECTS. That is a deliberate break with what this editor did
/// before, where a left drag panned. A sample editor's primary gesture is
/// a selection — everything destructive hangs off one — and pan has two
/// other homes here (the middle button, and the scroll wheel) while a
/// selection has none.
fn display_gesture(
    ui: &egui::Ui,
    editor: &mut Editor,
    response: &egui::Response,
    display: egui::Rect,
    channels: usize,
    source: &AudioSource,
    snap: Snap<'_>,
) {
    let (alt, shift) = ui.input(|input| (input.modifiers.alt, input.modifiers.shift));
    let scale = Scale {
        left: display.left(),
        scroll: editor.scroll_frames,
        fpp: editor.frames_per_pixel,
    };

    // Panning first, and by an INCREMENT: the pointer's travel since the
    // last frame is what egui reports, so anything measured from a
    // remembered origin would spring back every frame.
    if response.dragged_by(egui::PointerButton::Middle) {
        editor.scroll_frames -= f64::from(response.drag_delta().x) * editor.frames_per_pixel;
        clamp_scroll(editor, display.width(), snap.span);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        return;
    }

    if response.double_clicked() {
        editor.select_all();
        return;
    }

    // THE PRESS ORIGIN, not the current pointer. egui only calls a
    // gesture a drag once it has moved past a threshold, and by then the
    // pointer is already a handful of pixels along — anchoring on where
    // the drag was RECOGNISED would silently swallow the first few
    // pixels of every selection.
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(at) = ui
            .input(|input| input.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
    {
        let channel = channel_at(display, channels, at.y);
        let frame = snap.apply(scale.frame_in(at.x, snap.span), channel, source, alt);
        // Shift keeps the cursor as the anchor, which is how a selection
        // is extended without dragging back to where it started.
        let anchor = if shift { editor.cursor } else { frame };
        let mut mask = ChannelMask::single(channel);
        if let Some(existing) = editor.selection.filter(|_| shift) {
            mask = existing.channels;
        }
        editor.drag = Some(SelectionDrag {
            anchor,
            channels: mask,
        });
        if !shift {
            editor.selection = None;
            editor.cursor = frame;
        }
    }

    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(at) = response.interact_pointer_pos()
        && let Some(mut drag) = editor.drag
    {
        // A drag that crossed a lane meant to include it. Clamped into
        // the display first, so a gesture that ran off the bottom does
        // not silently add the last channel.
        let inside = at.y.clamp(display.top(), display.bottom() - 1.0);
        drag.channels.set(channel_at(display, channels, inside));
        editor.drag = Some(drag);
        let channel = drag.channels.iter(channels).next().unwrap_or(0);
        let frame = snap.apply(scale.frame_in(at.x, snap.span), channel, source, alt);
        editor.set_selection(drag.anchor, frame, drag.channels);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }

    if response.drag_stopped() {
        editor.drag = None;
    }

    // A CLICK IS A CURSOR, not a zero-length selection: the two mean
    // different things to every verb that reads them, and a stray click
    // must not leave a selection behind that a later command acts on.
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let channel = channel_at(display, channels, at.y);
        editor.cursor = snap.apply(scale.frame_in(at.x, snap.span), channel, source, alt);
        editor.selection = None;
    }

    if response.hovered() && !response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
}

/// The right-click menu: the destructive verbs, on the thing they act on.
///
/// Every one of them needs a selection, so with nothing selected the menu
/// says that instead of listing eight things that would all refuse.
fn context_menu(response: &egui::Response, editor: &mut Editor) {
    let has_selection = editor.selection.is_some();
    response.context_menu(|ui| {
        if !has_selection {
            ui.label("select a range first");
            return;
        }
        for (id, label) in CONTEXT_VERBS {
            if ui.button(*label).clicked() {
                editor.command = Some(id);
                ui.close();
            }
        }
    });
}

/// Zoom by `factor` (greater than one being IN) about the point `offset`
/// pixels from the display's left edge, which stays over the same frame.
///
/// Pure, and separate from the input handling, because "the frame under
/// the pointer does not move" is the whole feel of a zoom and it should
/// be checkable without a window.
fn zoom_about(editor: &mut Editor, offset: f64, factor: f64) {
    let old = editor.frames_per_pixel;
    let new = (old / factor).clamp(FPP_MIN, FPP_MAX);
    let anchor = editor.scroll_frames + offset * old;
    editor.frames_per_pixel = new;
    editor.scroll_frames = anchor - offset * new;
    editor.fit_pending = false;
}

/// The whole clip in the display, and no more.
fn fit_to(editor: &mut Editor, span: u64, width: f32) {
    if span == 0 || width <= 0.0 {
        return;
    }
    editor.frames_per_pixel = (span as f64 / f64::from(width)).clamp(FPP_MIN, FPP_MAX);
    editor.scroll_frames = 0.0;
    editor.fit_pending = false;
}

fn clamp_scroll(editor: &mut Editor, width: f32, span: u64) {
    let visible = f64::from(width) * editor.frames_per_pixel;
    editor.scroll_frames = editor
        .scroll_frames
        .clamp(0.0, (span as f64 - visible).max(0.0));
}

/// Decide whether the view needs real samples, and for which frames.
///
/// Asks for a MARGIN either side and rounds the request down to a stable
/// boundary, so scrolling a few pixels reuses the window it already has
/// instead of asking the disk again on every frame.
fn plan_window(
    editor: &mut Editor,
    source: &AudioSource,
    peaks: Option<&Peaks>,
    span: u64,
    width: f32,
) {
    if editor.frames_per_pixel >= SAMPLE_DRAW_FPP || span == 0 {
        editor.window_want = None;
        return;
    }
    let Some(file_frames) = peaks.map(Peaks::frames) else {
        return;
    };
    let region_end = region_end(source, file_frames);
    let first = editor.scroll_frames.floor().max(0.0) as u64;
    let last = (editor.scroll_frames + f64::from(width) * editor.frames_per_pixel)
        .ceil()
        .clamp(0.0, span as f64) as u64;
    // Through the same mapping the picture uses, so a reversed or looped
    // clip fetches the material it will actually draw.
    let mut lo = u64::MAX;
    let mut hi = 0u64;
    for frame in [first, last.saturating_sub(1)] {
        if let Some(mapped) = source_frame_of(source, region_end, frame) {
            lo = lo.min(mapped);
            hi = hi.max(mapped + 1);
        }
    }
    if lo >= hi {
        return;
    }
    let lo = lo.saturating_sub(WINDOW_MARGIN) / WINDOW_MARGIN * WINDOW_MARGIN;
    let hi = hi
        .saturating_add(WINDOW_MARGIN)
        .min(file_frames)
        .max(lo + 1);
    let frames = (hi - lo).min(WINDOW_MAX_FRAMES);
    if editor
        .samples_for(&source.path)
        .is_some_and(|window| window.covers(&source.path, lo, lo + frames))
    {
        editor.window_want = None;
        return;
    }
    let key = WindowKey {
        path: source.path.clone(),
        start: lo,
        frames,
    };
    if editor.window_pending.as_ref() == Some(&key) {
        return;
    }
    editor.window_want = Some(key);
}

fn paint_header(
    ui: &egui::Ui,
    theme: &Theme,
    header: egui::Rect,
    view: &PaintView<'_>,
    hover: Option<egui::Pos2>,
) {
    let detail = view.peaks.map_or_else(
        || "analysing…".to_owned(),
        |peaks| {
            let seconds = peaks.frames() as f64 / f64::from(peaks.sample_rate());
            format!(
                "{} ch  ·  {:.1} kHz  ·  {:.3} s",
                peaks.channels(),
                peaks.sample_rate() as f32 / 1_000.0,
                seconds
            )
        },
    );
    ui.painter().text(
        egui::pos2(header.left() + LABEL_PAD, header.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{}  ·  {detail}", view.clip.name),
        egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
        theme.text,
    );
    // WHAT IS SELECTED beats where the pointer is, and both beat the
    // idle line: a selection is a fact you act on, a hover position is a
    // fact you are only looking at.
    let right = match view.editor.selection {
        Some(selection) => selection_readout(view, selection, header.width()),
        None => hover.map_or_else(
            || {
                format!(
                    "{}  ·  {:.2} BPM  ·  {}{}{}",
                    view.time.grid_name,
                    view.time.bpm,
                    if view.editor.decibels { "dB" } else { "lin" },
                    if view.editor.snap_zero {
                        "  ·  0×"
                    } else {
                        ""
                    },
                    if view.editor.ripple {
                        "  ·  ripple"
                    } else {
                        ""
                    }
                )
            },
            |position| {
                // The header sits directly above the display and shares
                // its left edge, so the display's own scale reads the
                // pointer here too rather than a second copy of the
                // arithmetic.
                let frame = view.scale.frame_at(position.x).clamp(0.0, view.span as f64);
                let rate = f64::from(view.source.sample_rate.max(1));
                let seconds = frame / rate;
                format!(
                    "beat {:.3}  ·  {seconds:.4} s  ·  {} f",
                    seconds * view.time.bpm / 60.0,
                    frame as u64
                )
            },
        ),
    };
    ui.painter().text(
        egui::pos2(header.right() - LABEL_PAD, header.center().y),
        egui::Align2::RIGHT_CENTER,
        right,
        egui::FontId::new(font::VALUE, egui::FontFamily::Monospace),
        theme.text_value,
    );
}

/// How wide the readout has to be before it can carry every unit.
const READOUT_FULL_W: f32 = 520.0;

/// What is selected, in as many units as there is room for.
///
/// Bars, seconds and frames are three answers to the same question and a
/// sample editor is asked all three — where in the music, how long in
/// time, and how many samples to cut. Cramped, it keeps seconds, which is
/// the one that means something on its own.
///
/// The LEVELS come off the peak pyramid, so a ten-minute selection reads
/// as fast as a ten-millisecond one and neither of them touches the disk.
fn selection_readout(view: &PaintView<'_>, selection: Selection, width: f32) -> String {
    let rate = f64::from(view.source.sample_rate.max(1));
    let seconds = |frames: u64| frames as f64 / rate;
    let beats = |frames: u64| seconds(frames) * view.time.bpm / 60.0;
    let length = selection.frames();

    let levels = view.peaks.and_then(|peaks| {
        let region = region_end(view.source, peaks.frames());
        let (from, to) = source_span(view.source, region, selection.from, selection.to)?;
        let channels: Vec<usize> = selection.channels.iter(view.peaks?.channels()).collect();
        peaks.levels(&channels, from, to)
    });
    let level = levels.map_or_else(
        // A selection across a loop seam reads no level, and says so
        // rather than printing a number for one of its two halves.
        || "  ·  —".to_owned(),
        |(peak, rms)| format!("  ·  {peak:.1} / {rms:.1} dBFS"),
    );

    if width >= READOUT_FULL_W {
        format!(
            "{:.3} → {:.3}  ·  {:.4} s  ·  {length} f{level}",
            beats(selection.from),
            beats(selection.to),
            seconds(length),
        )
    } else {
        format!("{:.4} s{level}", seconds(length))
    }
}

fn paint_channels(ui: &egui::Ui, theme: &Theme, display: egui::Rect, channels: usize) {
    let channels = channels.max(1);
    let lane_h = display.height() / channels as f32;
    for channel in 0..channels {
        let top = display.top() + channel as f32 * lane_h;
        let center = top + lane_h * 0.5;
        if channel > 0 {
            ui.painter().line_segment(
                [
                    egui::pos2(display.left(), top),
                    egui::pos2(display.right(), top),
                ],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        ui.painter().line_segment(
            [
                egui::pos2(display.left(), center),
                egui::pos2(display.right(), center),
            ],
            egui::Stroke::new(stroke::HAIR, theme.outline),
        );
        let name = match (channels, channel) {
            (2, 0) => "L".to_owned(),
            (2, 1) => "R".to_owned(),
            (_, channel) => format!("{}", channel + 1),
        };
        ui.painter().text(
            egui::pos2(display.left() + LABEL_PAD, top + LABEL_PAD),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
            theme.text_muted,
        );
    }
}

/// One channel's top/bottom envelope as a pair of continuous polylines, drawn
/// as a smooth vector outline (top + bottom envelope) rather than
/// the old per-column bars. `peak_at(pixel)` returns `(y_top, y_bottom)` for
/// that column, or `None` for silence (collapsed to the centre line).
fn paint_envelope<F>(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane: Lane,
    width: usize,
    stroke: egui::Stroke,
    mut peak_at: F,
) where
    F: FnMut(usize) -> Option<(f32, f32)>,
{
    let mut top_pts: Vec<egui::Pos2> = Vec::with_capacity(width);
    let mut bot_pts: Vec<egui::Pos2> = Vec::with_capacity(width);
    for pixel in 0..width {
        let x = rect.left() + pixel as f32;
        let (y0, y1) = match peak_at(pixel) {
            Some((y0, y1)) => (
                y0.clamp(lane.top, lane.top + lane.height),
                y1.clamp(lane.top, lane.top + lane.height),
            ),
            None => (lane.center, lane.center),
        };
        top_pts.push(egui::pos2(x, y0));
        bot_pts.push(egui::pos2(x, y1.max(y0 + 1.0)));
    }
    smooth_vertical(&mut top_pts);
    smooth_vertical(&mut bot_pts);
    painter.add(egui::Shape::line(top_pts, stroke));
    painter.add(egui::Shape::line(bot_pts, stroke));
}

/// One small, symmetric low-pass pass over screen-space Y values.
///
/// The peak pyramid is deliberately conservative and can make adjacent
/// columns jump even when the underlying sound is continuous. Smoothing in
/// screen space keeps the cached extrema exact while making their outline
/// read as one anti-aliased curve. Endpoints stay anchored, and sample-level
/// zoom bypasses this helper so individual samples remain exact.
fn smooth_vertical(points: &mut [egui::Pos2]) {
    if points.len() < 3 {
        return;
    }
    let mut previous = points[0].y;
    for index in 1..points.len() - 1 {
        let current = points[index].y;
        let next = points[index + 1].y;
        points[index].y = previous * 0.25 + current * 0.5 + next * 0.25;
        previous = current;
    }
}

fn paint_waveform(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let Some(peaks) = view.peaks else { return };
    let display = view.display;
    let editor = view.editor;
    let channels = peaks.channels().max(1);
    let lane_h = display.height() / channels as f32;
    // The clip's GAIN is in here beside the view's own zoom, so the
    // picture is of what you will hear rather than of what is on disk. It
    // is also what makes the gain control feel connected to anything:
    // drag it and the waveform moves under your hand, on the frame you
    // moved it, whatever the engine is doing.
    let gain = view.source.gain.clamp(0.0, 16.0);
    let painter = ui.painter();
    // Real samples once the pyramid can no longer tell the truth, and
    // only once they are actually here — until then the envelope keeps
    // drawing, because a blank display while a read completes reads as a
    // clip with nothing in it.
    let samples = (editor.frames_per_pixel < SAMPLE_DRAW_FPP)
        .then(|| editor.samples_for(&view.source.path))
        .flatten();

    for channel in 0..channels {
        let lane_top = display.top() + channel as f32 * lane_h;
        let center = lane_top + lane_h * 0.5;
        let half = (lane_h * 0.5 - CHANNEL_PAD).max(1.0) * editor.amplitude_zoom;
        let lane = Lane {
            top: lane_top,
            height: lane_h,
            center,
            half,
        };
        match samples {
            Some(window) => paint_samples(painter, theme, view, window, channel, lane, gain),
            None => paint_peak_columns(painter, theme, view, peaks, channel, lane, gain),
        }
    }
}

/// One channel's slot in the display.
#[derive(Clone, Copy)]
struct Lane {
    top: f32,
    height: f32,
    center: f32,
    /// Pixels from the centre line to full scale, amplitude zoom included.
    half: f32,
}

impl Lane {
    /// A signed sample as a y coordinate, held inside the lane.
    fn y_of(&self, value: f32, decibels: bool) -> f32 {
        (self.center - amplitude(value, decibels) * self.half)
            .clamp(self.top, self.top + self.height)
    }
}

/// The envelope from the peak pyramid: a clean outline with no fill, plus a
/// mark wherever a bin reached full scale.
fn paint_peak_columns(
    painter: &egui::Painter,
    theme: &Theme,
    view: &PaintView<'_>,
    peaks: &Peaks,
    channel: usize,
    lane: Lane,
    gain: f32,
) {
    let editor = view.editor;
    let columns = waveform_columns(
        view.display.width(),
        view.span,
        editor.scroll_frames,
        editor.frames_per_pixel,
    );
    let decibels = editor.decibels;
    let mut top_pts = Vec::with_capacity(columns);
    let mut bottom_pts = Vec::with_capacity(columns);
    let mut clipped: Vec<egui::Pos2> = Vec::new();
    for column in 0..columns {
        let x = view.display.left() + column as f32;
        let from = (editor.scroll_frames + column as f64 * editor.frames_per_pixel).max(0.0) as u64;
        let to = ((editor.scroll_frames + (column + 1) as f64 * editor.frames_per_pixel)
            .min(view.span as f64)
            .max(0.0) as u64)
            .max(from + 1);
        let Some(peak) = source_extrema(peaks, view.source, channel, from, to) else {
            top_pts.push(egui::pos2(x, lane.center));
            bottom_pts.push(egui::pos2(x, lane.center));
            continue;
        };
        let high = lane.y_of(peak.max * gain, decibels);
        let low = lane.y_of(peak.min * gain, decibels);
        top_pts.push(egui::pos2(x, high));
        bottom_pts.push(egui::pos2(x, low.max(high + 1.0)));
        // Full scale AFTER the clip's gain, which is the level that will
        // actually leave the node — a clip turned down is not clipping,
        // however loud the file is.
        if peak.clipped() || (peak.max * gain).abs() >= 1.0 || (peak.min * gain).abs() >= 1.0 {
            clipped.push(egui::pos2(x, lane.top + stroke::HAIR));
        }
    }
    smooth_vertical(&mut top_pts);
    smooth_vertical(&mut bottom_pts);
    let stroke = egui::Stroke::new(WAVEFORM_STROKE, theme.accent);
    painter.add(egui::Shape::line(top_pts, stroke));
    painter.add(egui::Shape::line(bottom_pts, stroke));
    for at in clipped {
        painter.rect_filled(
            egui::Rect::from_min_size(at, egui::vec2(1.0, theme.sp(control::HANDLE))),
            0.0,
            theme.danger,
        );
    }
}

/// The samples themselves, one point per FRAME rather than per pixel.
///
/// Per pixel is what the envelope does and it is the wrong loop here: at
/// eight pixels to the frame there are eight columns per sample, and
/// asking the window for each of them would draw a staircase where the
/// audio has a line.
fn paint_samples(
    painter: &egui::Painter,
    theme: &Theme,
    view: &PaintView<'_>,
    window: &SampleWindow,
    channel: usize,
    lane: Lane,
    gain: f32,
) {
    let editor = view.editor;
    let decibels = editor.decibels;
    let first = editor.scroll_frames.floor().max(0.0) as u64;
    let last = (editor.scroll_frames + f64::from(view.display.width()) * editor.frames_per_pixel)
        .ceil()
        .clamp(0.0, view.span as f64) as u64;
    if last <= first {
        return;
    }
    let region = region_end(view.source, view.peaks.map_or(0, Peaks::frames));
    let mut points = Vec::with_capacity((last - first + 1) as usize);
    for frame in first..=last {
        let value = source_frame_of(view.source, region, frame)
            .and_then(|source| window.at(channel, source))
            .unwrap_or(0.0);
        points.push(egui::pos2(
            view.scale.x_of(frame as f64),
            lane.y_of(value * gain, decibels),
        ));
    }
    let stroke = egui::Stroke::new(1.0, theme.accent);
    // A dot per sample once there is room for one, so it is visible that
    // these ARE samples and not a curve that happens to be smooth.
    if view.scale.pixels_per_frame() >= SAMPLE_DOT_PX {
        for point in &points {
            painter.circle_filled(*point, 2.0, theme.accent);
        }
    }
    painter.add(egui::Shape::line(points, stroke));
}

pub struct ClipThumbnail<'a> {
    pub full_clip: egui::Rect,
    pub visible_clip: egui::Rect,
    pub clip: &'a Clip,
    pub peaks: &'a Peaks,
    pub bpm: f64,
    pub opacity: f32,
}

/// Draw a compact waveform inside an arrangement clip. Geometry is measured
/// against the FULL clip rect even when only part is on screen; otherwise a
/// panned view would incorrectly stretch the visible source slice to fill the
/// clipped rect. [`source_extrema`] then applies source offset, source length,
/// loop wrapping and the current tempo exactly like the large editor.
pub fn paint_clip_thumbnail(ui: &egui::Ui, theme: &Theme, view: ClipThumbnail<'_>) {
    let Some(source) = view.clip.audio.as_ref() else {
        return;
    };
    if view.full_clip.width() <= 0.0
        || view.full_clip.height() <= 0.0
        || view.visible_clip.width() <= 0.0
    {
        return;
    }
    let label_h = if view.full_clip.height() >= 30.0 {
        18.0
    } else {
        2.0
    };
    let full_waveform = egui::Rect::from_min_max(
        egui::pos2(
            view.full_clip.left(),
            (view.full_clip.top() + label_h).min(view.full_clip.bottom()),
        ),
        egui::pos2(
            view.full_clip.right(),
            (view.full_clip.bottom() - 3.0).max(view.full_clip.top()),
        ),
    );
    let visible = full_waveform.intersect(view.visible_clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }

    let painter = ui.painter().with_clip_rect(visible);
    let channels = view.peaks.channels().max(1);
    let lane_h = full_waveform.height() / channels as f32;
    let columns = visible.width().ceil().max(0.0) as usize;
    let colour = theme.clip_note.gamma_multiply(view.opacity.clamp(0.0, 1.0));
    let stroke = egui::Stroke::new(WAVEFORM_STROKE, colour);
    // The thumbnail measures in beats, because a clip on the timeline is
    // a rectangle of beats; the extrema lookup measures in frames. One
    // conversion, here, rather than a tempo argument threaded through the
    // mapping the editor shares with it.
    let per_frame = frames_per_beat(source.sample_rate, view.bpm);
    let to_frames = |beats: f32| (f64::from(beats.max(0.0)) * per_frame) as u64;
    for channel in 0..channels {
        let lane_top = full_waveform.top() + channel as f32 * lane_h;
        let center = lane_top + lane_h * 0.5;
        let half = (lane_h * 0.5 - 1.5).max(1.0);
        paint_envelope(
            &painter,
            visible,
            Lane {
                top: lane_top,
                height: lane_h,
                center,
                half,
            },
            columns,
            stroke,
            |pixel| {
                let x = visible.left() + pixel as f32;
                let beat0 = thumbnail_local_beat(view.full_clip, view.clip.len, x);
                let beat1 = thumbnail_local_beat(view.full_clip, view.clip.len, x + 1.0).max(beat0);
                let from = to_frames(beat0);
                let to = to_frames(beat1).max(from + 1);
                let peak = source_extrema(view.peaks, source, channel, from, to)?;
                Some((center - peak.max * half, center - peak.min * half))
            },
        );
    }
}

fn thumbnail_local_beat(full_clip: egui::Rect, clip_len: f32, x: f32) -> f32 {
    ((x - full_clip.left()) / full_clip.width().max(1e-6) * clip_len).clamp(0.0, clip_len)
}

/// The waveform issues no more than one vertical primitive per channel per
/// screen column. File length can reduce this count, never increase it.
fn waveform_columns(width: f32, span: u64, scroll: f64, frames_per_pixel: f64) -> usize {
    let viewport = f64::from(width.ceil().max(0.0));
    let remaining = ((span as f64 - scroll).max(0.0) / frames_per_pixel.max(f64::MIN_POSITIVE))
        .ceil()
        .max(0.0);
    viewport.min(remaining) as usize
}

/// A window measured from the START OF THE CLIP, as an absolute range in
/// the FORWARD file — mirrored inside the region when the clip plays
/// backwards.
///
/// The picture has to mirror for the same reason the audio does, and it
/// gets to do it for free: a reversed clip is the forward material read
/// the other way, so the peaks already on hand are the right peaks and
/// only the lookup turns around. Building a second peak pyramid for the
/// reversed file would be the same numbers, twice.
///
/// The arithmetic is the node's own, restated: reading `R[rev_o + r]`
/// where `R[i] = X[F-1-i]` and `rev_o = F - o - n` gives `X[o + n - 1 -
/// r]`, which is this reflection about the region's middle.
fn source_window(source: &AudioSource, region_end: u64, from: u64, to: u64) -> (u64, u64) {
    if !source.reversed {
        return (
            source.source_offset.saturating_add(from),
            source.source_offset.saturating_add(to),
        );
    }
    let region = region_end.saturating_sub(source.source_offset);
    (
        source
            .source_offset
            .saturating_add(region.saturating_sub(to.min(region))),
        source
            .source_offset
            .saturating_add(region.saturating_sub(from.min(region))),
    )
}

/// Where this clip's source region ends in the file — its own end, or
/// the file's, whichever comes first.
pub fn region_end(source: &AudioSource, file_frames: u64) -> u64 {
    source
        .source_offset
        .saturating_add(source.source_frames)
        .min(file_frames)
}

/// The source frame ONE clip frame reads.
///
/// The single-frame case of [`source_window`], loop wrap included — which
/// is what the sample-accurate picture needs, since at that zoom a screen
/// column is a fraction of a frame and there is no range left to take
/// extremes over.
pub fn source_frame_of(source: &AudioSource, region_end: u64, frame: u64) -> Option<u64> {
    if region_end <= source.source_offset {
        return None;
    }
    let region = region_end - source.source_offset;
    let local = if source.looped {
        frame % region
    } else if frame < region {
        frame
    } else {
        return None;
    };
    Some(if source.reversed {
        source.source_offset + (region - 1 - local)
    } else {
        source.source_offset + local
    })
}

/// The source frames a range of CLIP frames reads, or `None` when there
/// is no single answer.
///
/// There is no single answer when a looped clip's range crosses the loop
/// seam: the material either side of it is two different stretches of the
/// file, and there is nothing honest for one range to be. Every
/// destructive verb asks this and refuses when it comes back empty —
/// which is better than silently editing one of the two halves.
pub fn source_span(
    source: &AudioSource,
    region_end: u64,
    from: u64,
    to: u64,
) -> Option<(u64, u64)> {
    if to <= from || region_end <= source.source_offset {
        return None;
    }
    let region = region_end - source.source_offset;
    let (local_from, local_to) = if source.looped {
        let first = from / region;
        let last = (to - 1) / region;
        if first != last {
            return None;
        }
        (from % region, (to - 1) % region + 1)
    } else {
        if from >= region {
            return None;
        }
        (from, to.min(region))
    };
    Some(source_window(source, region_end, local_from, local_to))
}

/// Extremes and RMS for a range of CLIP frames.
///
/// Clip frames are counted at the source's own sample rate, so within one
/// pass of the region they are source frames offset by the region's start
/// — which is why nothing here needs a tempo any more.
fn source_extrema(
    peaks: &Peaks,
    source: &AudioSource,
    channel: usize,
    from: u64,
    to: u64,
) -> Option<Peak> {
    if source.source_frames == 0 || source.sample_rate == 0 {
        return None;
    }
    let relative0 = from;
    let relative1 = to.max(from + 1);
    let region_end = region_end(source, peaks.frames());
    if source.source_offset >= region_end {
        return None;
    }

    if !source.looped {
        let (start, end) = source_window(source, region_end, relative0, relative1);
        return peaks.extrema(channel, start, end.min(region_end));
    }

    let region_frames = region_end - source.source_offset;
    let span = relative1.saturating_sub(relative0);
    if span >= region_frames {
        return peaks.extrema(channel, source.source_offset, region_end);
    }
    let wrapped_start = relative0 % region_frames;
    let wrapped_end = wrapped_start.saturating_add(span.max(1));
    if wrapped_end <= region_frames {
        let (start, end) = source_window(source, region_end, wrapped_start, wrapped_end);
        peaks.extrema(channel, start, end)
    } else {
        // The wrap straddles the loop point: two reads, each mirrored on
        // its own, because reflecting a range that has already been split
        // is reflecting each half.
        let (start, end) = source_window(source, region_end, wrapped_start, region_frames);
        let mut out = peaks.extrema(channel, start, end);
        let (start, end) = source_window(source, region_end, 0, wrapped_end - region_frames);
        if let Some(second) = peaks.extrema(channel, start, end) {
            match &mut out {
                Some(out) => *out = out.merged(second),
                None => out = Some(second),
            }
        }
        out
    }
}

fn paint_grid(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let per_frame = frames_per_beat(view.source.sample_rate, view.time.bpm);
    let pixels_per_beat = (per_frame / view.editor.frames_per_pixel) as f32;
    let scroll_beats = (view.editor.scroll_frames / per_frame) as f32;
    let subdivision = view.time.grid_beats.max(0.001);
    let step = if subdivision * pixels_per_beat < GRID_MIN_PX {
        1.0
    } else {
        subdivision
    };
    // Zoomed into the samples, beats are metres apart and useless as a
    // ruler. A time ladder takes over — the same information a sample
    // editor's ruler carries, and the only one that still has ticks on
    // screen at this scale.
    if view.editor.frames_per_pixel < SAMPLE_DRAW_FPP {
        paint_time_grid(ui, theme, view);
        return;
    }
    let per_bar = view.time.beats_per_bar.max(1) as f32;
    let mut beat = first_grid_beat(view.clip.start, scroll_beats, step);
    for _ in 0..MAX_GRID_LINES {
        let local = beat - view.clip.start;
        let x = view.scale.x_of(f64::from(local) * per_frame);
        if x > view.display.right() {
            break;
        }
        if x >= view.display.left() {
            let on_bar = (beat % per_bar).abs() < 1e-3;
            let on_beat = beat.fract().abs() < 1e-3;
            let colour = if on_bar {
                theme.grid_bar
            } else if on_beat {
                theme.grid_beat
            } else {
                theme.grid_sub
            };
            ui.painter().line_segment(
                [
                    egui::pos2(x, view.display.top()),
                    egui::pos2(x, view.display.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, colour),
            );
            if on_bar {
                let bar = (beat / per_bar).floor().max(0.0) as u64 + 1;
                ui.painter().text(
                    egui::pos2(x + 3.0, view.display.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    bar.to_string(),
                    egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                    theme.text_muted,
                );
            }
        }
        beat += step;
    }
}

fn first_grid_beat(clip_start: f32, scroll: f32, step: f32) -> f32 {
    let step = step.max(0.001);
    ((clip_start + scroll) / step).floor() * step
}

/// The step a time ladder takes, in frames: the smallest 1-2-5 step that
/// still leaves room to label it.
fn time_grid_step(frames_per_pixel: f64) -> u64 {
    let wanted = frames_per_pixel * f64::from(TIME_LABEL_W);
    let mut step = 1.0f64;
    while step < wanted {
        for factor in [2.0, 2.5, 2.0] {
            step *= factor;
            if step >= wanted {
                break;
            }
        }
    }
    step.round().max(1.0) as u64
}

/// How a tick's position reads, given how far apart the ticks are.
fn time_grid_label(frame: u64, step: u64, rate: u32) -> String {
    let rate = f64::from(rate.max(1));
    let seconds = frame as f64 / rate;
    if step as f64 >= rate {
        format!("{seconds:.0}s")
    } else if step as f64 * 1_000.0 >= rate {
        format!("{:.0}ms", seconds * 1_000.0)
    } else {
        format!("{frame}")
    }
}

/// Seconds, then milliseconds, then frame numbers — whichever the current
/// zoom has room for.
fn paint_time_grid(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let step = time_grid_step(view.editor.frames_per_pixel);
    let first = (view.editor.scroll_frames.max(0.0) as u64 / step) * step;
    let painter = ui.painter();
    let mut frame = first;
    for _ in 0..MAX_GRID_LINES {
        let x = view.scale.x_of(frame as f64);
        if x > view.display.right() {
            break;
        }
        if x >= view.display.left() && frame <= view.span {
            painter.line_segment(
                [
                    egui::pos2(x, view.display.top()),
                    egui::pos2(x, view.display.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.grid_beat),
            );
            painter.text(
                egui::pos2(x + 3.0, view.display.top() + 3.0),
                egui::Align2::LEFT_TOP,
                time_grid_label(frame, step, view.source.sample_rate),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                theme.text_muted,
            );
        }
        frame = frame.saturating_add(step);
    }
    // The individual frames, once each is wide enough to see between.
    if view.scale.pixels_per_frame() >= SAMPLE_DOT_PX {
        let first = view.editor.scroll_frames.max(0.0) as u64;
        let last = (view.editor.scroll_frames
            + f64::from(view.display.width()) * view.editor.frames_per_pixel)
            .clamp(0.0, view.span as f64) as u64;
        for frame in first..=last {
            let x = view.scale.x_of(frame as f64);
            painter.line_segment(
                [
                    egui::pos2(x, view.display.bottom() - theme.sp(control::HANDLE)),
                    egui::pos2(x, view.display.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.grid_sub),
            );
        }
    }
}

/// The selected range: a wash over the channels it covers, and an edge
/// stroke at each end so a boundary is findable over dense material.
///
/// Only the covered channels. A wash across a lane that is not selected
/// would say the edit reaches it, and the whole point of a channel mask
/// is that it does not.
fn paint_selection(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>, channels: usize) {
    let Some(selection) = view.editor.selection else {
        return;
    };
    let left = view.scale.x_of(selection.from as f64);
    let right = view.scale.x_of(selection.to as f64);
    if right < view.display.left() || left > view.display.right() {
        return;
    }
    let lane_h = view.display.height() / channels.max(1) as f32;
    let painter = ui.painter();
    for channel in selection.channels.iter(channels) {
        let top = view.display.top() + channel as f32 * lane_h;
        let band = egui::Rect::from_min_max(
            egui::pos2(left.max(view.display.left()), top),
            egui::pos2(right.min(view.display.right()), top + lane_h),
        );
        if band.width() > 0.0 {
            painter.rect_filled(band, 0.0, theme.selection.gamma_multiply(0.35));
        }
    }
    for x in [left, right] {
        if x >= view.display.left() && x <= view.display.right() {
            painter.line_segment(
                [
                    egui::pos2(x, view.display.top()),
                    egui::pos2(x, view.display.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.selection),
            );
        }
    }
}

/// Where a paste lands and where playing the clip begins.
///
/// Quieter than the playhead and a different colour, because they are
/// different facts and they are on screen together: one is where the
/// engine IS, the other is where the next edit WILL BE.
fn paint_cursor(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let x = view.scale.x_of(view.editor.cursor as f64);
    if x < view.display.left() || x > view.display.right() {
        return;
    }
    ui.painter().line_segment(
        [
            egui::pos2(x, view.display.top()),
            egui::pos2(x, view.display.bottom()),
        ],
        egui::Stroke::new(stroke::HAIR, theme.accent),
    );
}

/// One grip per selection edge.
///
/// TWO INTERACTIONS, one per edge, each with its own id — the device UI
/// contract's first rule. A single widget asking "which edge is the
/// pointer nearer?" is the shape that shipped the same bug three times:
/// drag the left edge past the right and the gesture would change its
/// mind about what it had hold of, halfway through.
fn selection_handles(
    ui: &mut egui::Ui,
    theme: &Theme,
    editor: &mut Editor,
    display: egui::Rect,
    source: &AudioSource,
    snap: Snap<'_>,
) {
    let Some(selection) = editor.selection else {
        return;
    };
    let span = editor.span;
    let scale = Scale {
        left: display.left(),
        scroll: editor.scroll_frames,
        fpp: editor.frames_per_pixel,
    };
    let alt = ui.input(|input| input.modifiers.alt);
    let grab = theme.sp(control::HANDLE) * 2.0;
    for edge in [Edge::From, Edge::To] {
        let frame = match edge {
            Edge::From => selection.from,
            Edge::To => selection.to,
        };
        let x = scale.x_of(frame as f64);
        if x < display.left() - grab || x > display.right() + grab {
            continue;
        }
        let hit = egui::Rect::from_min_max(
            egui::pos2(x - grab, display.top()),
            egui::pos2(x + grab, display.bottom()),
        );
        let id = ui.id().with(("selection_edge", edge == Edge::From));
        let response = ui
            .interact(hit, id, egui::Sense::click_and_drag())
            .affords(Affords::SeamX);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        // The anchor is REMEMBERED AT THE PRESS, not re-read from the
        // selection each frame. Re-reading it is a bug with a long tail:
        // once the drag crosses the other edge the two swap, so the
        // "opposite edge" the next frame reads is the one under the
        // pointer — and the selection walks along behind the cursor
        // instead of stretching from where it started.
        if response.drag_started() {
            editor.drag = Some(SelectionDrag {
                anchor: match edge {
                    Edge::From => selection.to,
                    Edge::To => selection.from,
                },
                channels: selection.channels,
            });
        }
        if response.dragged()
            && let Some(at) = response.interact_pointer_pos()
            && let Some(drag) = editor.drag
        {
            let channel = drag.channels.iter(editor.channels).next().unwrap_or(0);
            let want = snap.apply(scale.frame_in(at.x, span), channel, source, alt);
            editor.set_selection(drag.anchor, want, drag.channels);
        }
        if response.drag_stopped() {
            editor.drag = None;
        }
        if response.hovered() || response.dragged() {
            ui.painter().line_segment(
                [
                    egui::pos2(x, display.top()),
                    egui::pos2(x, display.bottom()),
                ],
                egui::Stroke::new(stroke::BOLD, theme.selection),
            );
        }
    }
}

/// The channel labels, as targets: Alt+click adds or removes a channel
/// from the selection without redrawing it.
fn channel_labels(
    ui: &mut egui::Ui,
    theme: &Theme,
    editor: &mut Editor,
    display: egui::Rect,
    channels: usize,
) {
    if channels < 2 {
        return;
    }
    let lane_h = display.height() / channels as f32;
    let size = egui::vec2(theme.sp(control::HANDLE) * 4.0, lane_h.min(20.0));
    for channel in 0..channels {
        let at = egui::pos2(
            display.left() + LABEL_PAD,
            display.top() + channel as f32 * lane_h + LABEL_PAD,
        );
        let hit = egui::Rect::from_min_size(at, size);
        let id = ui.id().with(("channel_label", channel));
        let response = ui
            .interact(hit, id, egui::Sense::click())
            .affords(Affords::Press);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter()
                .rect_filled(hit, 2.0, theme.selection.gamma_multiply(0.15));
        }
        if !response.clicked() {
            continue;
        }
        let alt = ui.input(|input| input.modifiers.alt);
        match editor.selection {
            // Alt TOGGLES this channel in what is already selected; a
            // plain click narrows the selection to this channel alone.
            Some(mut selection) => {
                if alt {
                    selection.channels.toggle(channel);
                } else {
                    selection.channels = ChannelMask::single(channel);
                }
                editor.selection = (!selection.channels.is_empty()).then_some(selection);
            }
            // With nothing selected there is nothing to narrow, so the
            // click selects the whole of this channel — which is what
            // clicking a channel's name means in every editor that has
            // one.
            None => {
                if editor.span > 0 {
                    editor.selection = Some(Selection {
                        from: 0,
                        to: editor.span,
                        channels: ChannelMask::single(channel),
                    });
                    editor.cursor = 0;
                }
            }
        }
    }
}

fn paint_clip_end(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let end_x = view.scale.x_of(view.span as f64);
    if end_x < view.display.right() {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(end_x.max(view.display.left()), view.display.top()),
                view.display.max,
            ),
            0.0,
            theme.bg.gamma_multiply(0.65),
        );
    }
    if end_x >= view.display.left() && end_x <= view.display.right() {
        ui.painter().line_segment(
            [
                egui::pos2(end_x, view.display.top()),
                egui::pos2(end_x, view.display.bottom()),
            ],
            egui::Stroke::new(1.5, theme.clip_selected),
        );
    }
}

fn paint_playhead(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let local = view.time.playhead - view.clip.start;
    if local < 0.0 || local > view.clip.len {
        return;
    }
    let per_frame = frames_per_beat(view.source.sample_rate, view.time.bpm);
    let x = view.scale.x_of(f64::from(local) * per_frame);
    if x >= view.display.left() && x <= view.display.right() {
        ui.painter().line_segment(
            [
                egui::pos2(x, view.display.top()),
                egui::pos2(x, view.display.bottom()),
            ],
            egui::Stroke::new(1.5, theme.playhead),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw::ui::device::probe;

    fn temp_wav() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("daw-waveform-{}-{nonce}.wav", std::process::id()))
    }

    fn peaks(samples: &[f32], channels: u16) -> Peaks {
        build_from_samples(
            Path::new("test.wav"),
            48_000,
            channels,
            samples.iter().copied().map(Ok),
        )
        .expect("test samples are valid")
    }

    #[test]
    fn stereo_peaks_keep_channels_separate_and_sanitize_nonsense() {
        let waveform = peaks(
            &[-1.0, 0.25, 0.5, -0.75, f32::NAN, f32::INFINITY, 0.1, -0.2],
            2,
        );
        assert_eq!(waveform.channels(), 2);
        assert_eq!(waveform.frames(), 4);
        let left = waveform.extrema(0, 0, 4).expect("left");
        assert_eq!((left.min, left.max), (-1.0, 0.5));
        let right = waveform.extrema(1, 0, 4).expect("right");
        assert_eq!((right.min, right.max), (-0.75, 0.25));
    }

    #[test]
    fn wav_decoder_builds_the_playback_files_peak_pyramid() {
        let path = temp_wav();
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("create test WAV");
        for sample in [i16::MIN, 8_192, 16_384, -16_384] {
            writer.write_sample(sample).expect("write test sample");
        }
        writer.finalize().expect("finalize test WAV");

        let waveform = build_peaks(&path).expect("decode test WAV");
        std::fs::remove_file(&path).expect("remove test WAV");

        assert_eq!(waveform.frames(), 2);
        assert_eq!(waveform.channels(), 2);
        let left = waveform.extrema(0, 0, 2).expect("left peaks");
        let right = waveform.extrema(1, 0, 2).expect("right peaks");
        assert!(left.min <= -1.0 && left.max >= 0.5);
        assert!(right.min <= -0.5 && right.max >= 0.25);
    }

    #[test]
    fn pyramid_queries_are_bounded_and_cover_the_requested_signal() {
        let samples = (0..4_096)
            .map(|index| if index == 2_048 { -1.0 } else { 0.5 })
            .collect::<Vec<_>>();
        let waveform = peaks(&samples, 1);
        assert!(waveform.levels.len() > 1);
        let whole = waveform.extrema(0, 0, waveform.frames()).expect("whole");
        assert_eq!((whole.min, whole.max), (-1.0, 0.5));
        assert_eq!(waveform.extrema(1, 0, 10), None);
        assert_eq!(waveform.extrema(0, 10, 10), None);
    }

    #[test]
    fn source_mapping_obeys_offset_end_and_looping() {
        let samples = (0..512)
            .map(|index| index as f32 / 511.0)
            .collect::<Vec<_>>();
        let waveform =
            build_from_samples(Path::new("test.wav"), 128, 1, samples.into_iter().map(Ok))
                .expect("test samples are valid");
        let mut source = AudioSource {
            path: PathBuf::from("test.wav"),
            sample_rate: 128,
            source_offset: 128,
            source_frames: 256,
            gain: 1.0,
            looped: false,
            file_frames: 0,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        // The region is 256 frames: two 128-frame halves of the clip.
        let first = source_extrema(&waveform, &source, 0, 0, 128).expect("first half");
        let second = source_extrema(&waveform, &source, 0, 128, 256).expect("second half");
        assert!(second.min > first.min, "the ramp rises");
        assert!(
            source_extrema(&waveform, &source, 0, 256, 384).is_none(),
            "past the region there is nothing"
        );

        source.looped = true;
        assert_eq!(
            source_extrema(&waveform, &source, 0, 0, 128),
            source_extrema(&waveform, &source, 0, 256, 384),
            "a loop reads the region again"
        );
    }

    /// The clip frame a picture draws is the source frame the node reads.
    #[test]
    fn one_clip_frame_maps_to_one_source_frame() {
        let mut source = AudioSource {
            path: PathBuf::from("s.wav"),
            sample_rate: 48_000,
            source_offset: 1_000,
            source_frames: 400,
            gain: 1.0,
            looped: false,
            file_frames: 10_000,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        let end = region_end(&source, 10_000);
        assert_eq!(source_frame_of(&source, end, 0), Some(1_000));
        assert_eq!(source_frame_of(&source, end, 399), Some(1_399));
        assert_eq!(source_frame_of(&source, end, 400), None, "past the region");

        // Reversed, the single-frame case agrees with the range case to
        // the frame — the two must never drift, or the picture at maximum
        // zoom would show different audio than the picture one notch out.
        source.reversed = true;
        for frame in [0u64, 1, 37, 399] {
            let (start, _) = source_window(&source, end, frame, frame + 1);
            assert_eq!(source_frame_of(&source, end, frame), Some(start));
        }

        // Looped, it wraps.
        source.reversed = false;
        source.looped = true;
        assert_eq!(source_frame_of(&source, end, 400), Some(1_000));
        assert_eq!(source_frame_of(&source, end, 437), Some(1_037));
    }

    #[test]
    fn changing_clips_resets_view_but_reselecting_does_not() {
        let mut editor = Editor::default();
        editor.follow_clip(Some(7));
        editor.scroll_frames = 12_000.0;
        editor.amplitude_zoom = 3.0;
        editor.fit_pending = false;
        editor.follow_clip(Some(7));
        assert_eq!(editor.scroll_frames, 12_000.0);
        assert_eq!(editor.amplitude_zoom, 3.0);

        editor.follow_clip(Some(8));
        assert_eq!(editor.scroll_frames, 0.0);
        assert_eq!(editor.amplitude_zoom, 1.0);
        assert!(editor.fit_pending);
    }

    /// The gain control speaks dB and the clip stores LINEAR, and the two
    /// round-trip. A control that drifted a little on every drag would
    /// walk a clip's level away under the user's hand.
    #[test]
    fn clip_gain_round_trips_through_the_control() {
        for db in [-60.0f32, -24.0, -6.0, 0.0, 6.0, 12.0] {
            let back = gain_db(gain_linear(db));
            assert!((back - db).abs() < 0.001, "{db} dB came back as {back}");
        }
        assert_eq!(gain_linear(0.0), 1.0, "unity is unity");
        assert_eq!(gain_linear(-60.0), 0.0, "the bottom is silence");
        // Silence reads as the bottom rather than as minus infinity,
        // which is what a log of zero would hand the control.
        assert_eq!(gain_db(0.0), -60.0);
        assert!(gain_db(f32::NAN).is_finite() || gain_db(0.0) == -60.0);
    }

    /// A fade is measured in frames of the clip's TIMELINE span, not of
    /// its source region — a looped clip runs through its region many
    /// times and a fade must still reach full level once.
    #[test]
    fn a_fade_is_measured_against_the_clip_not_the_sample() {
        let source = AudioSource {
            path: PathBuf::from("loop.wav"),
            sample_rate: 48_000,
            source_offset: 0,
            // A quarter-second sample...
            source_frames: 12_000,
            gain: 1.0,
            // ...looped across four beats, which at 120 is two seconds.
            looped: true,
            file_frames: 12_000,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        let clip = Clip {
            id: 1,
            name: "loop".to_owned(),
            start: 0.0,
            len: 4.0,
            notes: Vec::new(),
            audio: Some(source.clone()),
            ..Clip::default()
        };
        let span = clip_span_frames(&clip, &source, 120.0);
        assert_eq!(span, 96_000, "four beats at 120 is two seconds");
        assert!(
            span > source.source_frames,
            "the span is the CLIP's, not the sample's"
        );

        // A fade's ceiling is that span, in seconds.
        let param = fade_param("fade in", span as f32 / source.sample_rate as f32);
        assert_eq!(param.value(0.0), 0.0);
        assert!((param.value(1.0) - 2.0).abs() < 1e-4, "two seconds of room");

        // A zero-length clip has no span, and the panel draws no fades.
        let empty = Clip { len: 0.0, ..clip };
        assert_eq!(clip_span_frames(&empty, &source, 120.0), 0);
    }

    /// THE PICTURE MIRRORS WITH THE AUDIO.
    ///
    /// Stated against the node's own arithmetic rather than against
    /// itself: reading `R[rev_o + r]`, where `R[i] = X[F-1-i]` and
    /// `rev_o = F - o - n`, is `X[o + n - 1 - r]`. If the display and
    /// that ever disagree, the waveform is a picture of a different
    /// sound than the one playing — which is the one thing a waveform
    /// must never be.
    #[test]
    fn a_reversed_clip_is_drawn_mirrored() {
        let source = AudioSource {
            path: PathBuf::from("loop.wav"),
            sample_rate: 48_000,
            source_offset: 1_000,
            source_frames: 400,
            gain: 1.0,
            looped: false,
            file_frames: 10_000,
            reversed: true,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        let region_end = source.source_offset + source.source_frames;

        // The clip's FIRST frame is the region's LAST, and its last is
        // the region's first.
        let (start, _) = source_window(&source, region_end, 0, 1);
        assert_eq!(start, 1_399, "frame 0 reads the region's end");
        let (start, _) = source_window(&source, region_end, 399, 400);
        assert_eq!(start, 1_000, "the last frame reads the region's start");

        // Every position agrees with the node's expression, to the frame.
        for r in [0u64, 1, 37, 200, 399] {
            let (start, end) = source_window(&source, region_end, r, r + 1);
            let node = source.source_offset + source.source_frames - 1 - r;
            assert_eq!(start, node, "clip frame {r}");
            assert_eq!(end, node + 1);
        }

        // Forwards is untouched.
        let forward = AudioSource {
            reversed: false,
            ..source
        };
        let (start, end) = source_window(&forward, region_end, 37, 38);
        assert_eq!((start, end), (1_037, 1_038));
    }

    /// The mirror covers the WHOLE region and nothing outside it — a
    /// reflection that slipped by a frame would read the neighbouring
    /// clip's material at one edge and silence at the other.
    #[test]
    fn the_mirror_stays_inside_the_region() {
        let source = AudioSource {
            path: PathBuf::from("loop.wav"),
            sample_rate: 48_000,
            source_offset: 500,
            source_frames: 100,
            gain: 1.0,
            looped: false,
            file_frames: 10_000,
            reversed: true,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        };
        let region_end = source.source_offset + source.source_frames;
        let (start, end) = source_window(&source, region_end, 0, 100);
        assert_eq!((start, end), (500, 600), "the whole region, exactly");

        // A window past the end clamps rather than reading behind the
        // region's start.
        let (start, end) = source_window(&source, region_end, 90, 500);
        assert!(start >= source.source_offset, "never before the region");
        assert!(end <= region_end, "never past it");
    }

    /// START slides the window inside the file, and cannot ask for
    /// material that is not there: its ceiling is what the file has left
    /// once the clip's own duration is accounted for.
    #[test]
    fn the_start_control_cannot_run_off_the_end_of_the_file() {
        // A two-second file holding a half-second clip: a second and a
        // half of room to slide through.
        let rate = 48_000.0f32;
        let file_frames = 96_000u64;
        let clip_frames = 24_000u64;
        let room = file_frames - clip_frames;
        let param = start_param(room as f32 / rate);

        assert_eq!(param.value(0.0), 0.0, "the file's start");
        assert!(
            (param.value(1.0) - 1.5).abs() < 1e-4,
            "1.5 s of room, got {}",
            param.value(1.0)
        );
        // Every position lands inside the file, clip length included.
        for i in 0..=20 {
            let seconds = param.value(i as f32 / 20.0);
            let frames = (seconds * rate).round() as u64;
            assert!(
                frames + clip_frames <= file_frames,
                "{seconds} s would read past the end"
            );
        }
    }

    /// A file with no room to slide offers no control at all, rather
    /// than one whose whole travel is a single value.
    #[test]
    fn a_file_with_no_room_offers_no_start_control() {
        let file_frames = 24_000u64;
        let clip_frames = 24_000u64;
        assert_eq!(
            file_frames.saturating_sub(clip_frames),
            0,
            "the guard the panel checks"
        );
    }

    /// The panel offers exactly what an `AudioSource` HAS.
    ///
    /// A control that moves nothing is worse than a short panel — the
    /// delay printed a time its engine was ignoring once already. If a
    /// field arrives here later (a fade, a pitch), this test is what says
    /// the panel is now allowed to grow.
    #[test]
    fn the_controls_offer_only_what_a_clip_carries() {
        let param = gain_param();
        assert_eq!(param.name, "gain");
        assert_eq!(param.value(0.0), -60.0);
        assert_eq!(param.value(1.0), 12.0);
        let looping = loop_param();
        assert_eq!(looping.choices(), Some(2));
        assert_eq!(looping.index(looping.at_index(1)), 1);
        assert_eq!(looping.index(looping.at_index(0)), 0);
    }

    /// The controls column takes the browser's width off the left, and
    /// the waveform gets everything else — with no gap and no overlap
    /// between them, since whatever lands in the column later will be
    /// laid out against exactly this rectangle.
    #[test]
    fn the_controls_column_takes_the_left_edge_at_browser_width() {
        let area = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(900.0, 300.0));
        let (controls, rest) = split_controls(area);

        assert_eq!(controls.width(), control::SIDE_COLUMN_W, "browser width");
        assert_eq!(controls.left(), area.left(), "it owns the LEFT edge");
        assert_eq!(controls.height(), area.height(), "full height");
        assert_eq!(controls.right(), rest.left(), "no gap, no overlap");
        assert_eq!(rest.right(), area.right());
        assert_eq!(
            controls.width() + rest.width(),
            area.width(),
            "between them they are the whole region"
        );
    }

    /// Squeezed narrow, the column stands DOWN rather than shrinking.
    ///
    /// A half-width column of controls is neither usable nor readable,
    /// and at that size the waveform is the thing worth seeing — so the
    /// picture keeps the whole region instead of both being useless.
    #[test]
    fn a_narrow_editor_gives_the_waveform_everything() {
        let narrow = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(control::SIDE_COLUMN_W + MIN_DISPLAY_W - 1.0, 300.0),
        );
        let (controls, rest) = split_controls(narrow);
        assert_eq!(controls.width(), 0.0, "no column at all");
        assert_eq!(rest, narrow, "and the waveform keeps the region");

        // One point wider and the column arrives, whole.
        let wide = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(control::SIDE_COLUMN_W + MIN_DISPLAY_W, 300.0),
        );
        let (controls, rest) = split_controls(wide);
        assert_eq!(controls.width(), control::SIDE_COLUMN_W);
        assert_eq!(rest.width(), MIN_DISPLAY_W);
    }

    #[test]
    fn scroll_never_exposes_space_beyond_both_clip_edges() {
        // A 10 000-frame clip, 400 px wide at 10 frames per pixel: four
        // thousand frames on screen, so six thousand of scroll is the end.
        let mut editor = Editor {
            scroll_frames: 99_000.0,
            frames_per_pixel: 10.0,
            ..Default::default()
        };
        clamp_scroll(&mut editor, 400.0, 10_000);
        assert_eq!(editor.scroll_frames, 6_000.0);
        editor.scroll_frames = -5.0;
        clamp_scroll(&mut editor, 400.0, 10_000);
        assert_eq!(editor.scroll_frames, 0.0);
    }

    #[test]
    fn waveform_work_is_bounded_by_viewport_not_file_duration() {
        // 400 frames at one per pixel fills half of an 800 px display.
        assert_eq!(waveform_columns(800.0, 400, 0.0, 1.0), 400);
        // A file far longer than the display still costs the display.
        assert_eq!(waveform_columns(800.0, 100_000_000, 0.0, 1.0), 800);
        // Scrolled past its end, a clip draws nothing at all.
        assert_eq!(waveform_columns(800.0, 400, 400.0, 1.0), 0);
        // TEN TIMES WIDER IS TEN TIMES THE WORK, and the file's length
        // never enters into it.
        assert_eq!(waveform_columns(80.0, 100_000_000, 0.0, 1.0), 80);
        assert_eq!(waveform_columns(800.0, 100_000_000, 0.0, 1.0), 800);
    }

    #[test]
    fn clipped_thumbnail_pixels_still_map_against_the_full_clip() {
        let full = egui::Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(400.0, 64.0));
        assert_eq!(thumbnail_local_beat(full, 4.0, 100.0), 0.0);
        assert_eq!(thumbnail_local_beat(full, 4.0, 300.0), 2.0);
        assert_eq!(thumbnail_local_beat(full, 4.0, 500.0), 4.0);
        // If the viewport begins at x=300, that first visible pixel remains
        // beat 2. It must not be remapped to beat 0 and stretch the waveform.
        let visible = full.intersect(egui::Rect::from_min_max(
            egui::pos2(300.0, 0.0),
            egui::pos2(500.0, 64.0),
        ));
        assert_eq!(thumbnail_local_beat(full, 4.0, visible.left()), 2.0);
    }

    #[test]
    fn complete_editor_renders_headlessly_and_fits_the_clip() {
        let samples = vec![0.5; 48_000];
        let waveform = peaks(&samples, 1);
        let clip = Clip {
            id: 42,
            name: "voice".to_owned(),
            start: 8.0,
            len: 2.0,
            notes: Vec::new(),
            audio: Some(AudioSource {
                path: PathBuf::from("voice.wav"),
                sample_rate: 48_000,
                source_offset: 0,
                source_frames: 48_000,
                gain: 1.0,
                looped: false,
                file_frames: 0,
                reversed: false,
                fade_in: 0,
                fade_out: 0,
                fade_in_curve: 0.0,
                fade_out_curve: 0.0,
                envelope: Vec::new(),
            }),
            ..Clip::default()
        };
        let context = egui::Context::default();
        let mut editor = Editor::default();
        editor.follow_clip(Some(clip.id));
        let mut focus = Focus::default();
        let theme = Theme::dark();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    body(
                        ui,
                        &mut focus,
                        &theme,
                        &mut editor,
                        Some(&clip),
                        Some(&waveform),
                        TimeView {
                            bpm: 120.0,
                            beats_per_bar: 4,
                            playhead: 9.0,
                            grid_beats: 0.25,
                            grid_name: "1/16",
                            follow: false,
                        },
                    );
                    let full_clip =
                        egui::Rect::from_min_size(egui::pos2(20.0, 80.0), egui::vec2(600.0, 64.0));
                    paint_clip_thumbnail(
                        ui,
                        &theme,
                        ClipThumbnail {
                            full_clip,
                            visible_clip: full_clip.intersect(egui::Rect::from_min_max(
                                egui::pos2(220.0, 80.0),
                                egui::pos2(620.0, 144.0),
                            )),
                            clip: &clip,
                            peaks: &waveform,
                            bpm: 120.0,
                            opacity: 1.0,
                        },
                    );
                });
            },
        );
        output.textures_delta.clear();

        assert!(!editor.fit_pending);
        assert_eq!(editor.scroll_frames, 0.0);
        // Two beats at 120 BPM is one second: 48 000 frames across the
        // display's width, less the controls column.
        assert!(editor.frames_per_pixel > 0.0);
        assert!(
            editor.frames_per_pixel * f64::from(800.0 - control::SIDE_COLUMN_W) >= 47_000.0,
            "the fit shows the whole second: {}",
            editor.frames_per_pixel
        );
    }

    #[test]
    fn grid_lines_are_absolute_even_when_the_clip_starts_off_grid() {
        assert_eq!(first_grid_beat(1.5, 0.0, 1.0), 1.0);
        assert_eq!(first_grid_beat(1.5, 0.75, 1.0), 2.0);
        assert_eq!(first_grid_beat(5.25, 0.0, 0.5), 5.0);
    }

    // ---- WA-02: a scene a pointer can be driven over ---------------------

    /// A two-beat clip of a known signal, and where its display lands.
    ///
    /// 800 x 300 with the controls column taking the left 240 leaves the
    /// waveform 560 wide, and the header takes the top 30. Every pointer
    /// test below works in those coordinates, which is the whole reason
    /// `split_controls` is a pure function.
    const SCENE: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(800.0, 300.0),
    };
    /// Two beats at 120 BPM is one second: 48 000 frames at 48 kHz.
    const SCENE_SPAN: u64 = 48_000;

    fn scene_display() -> egui::Rect {
        let (_, rest) = split_controls(SCENE);
        egui::Rect::from_min_max(egui::pos2(rest.left(), rest.top() + HEADER_H), rest.max)
    }

    fn scene_clip(channels: u16) -> (Clip, Peaks) {
        // A sine, so there are real zero crossings to snap to.
        let frames = SCENE_SPAN as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for frame in 0..frames {
            let value = (frame as f32 * 0.01).sin();
            for _ in 0..channels {
                samples.push(value);
            }
        }
        let peaks = peaks(&samples, channels);
        let clip = Clip {
            id: 1,
            name: "scene".to_owned(),
            start: 0.0,
            len: 2.0,
            notes: Vec::new(),
            audio: Some(AudioSource {
                path: PathBuf::from("scene.wav"),
                sample_rate: 48_000,
                source_offset: 0,
                source_frames: SCENE_SPAN,
                gain: 1.0,
                looped: false,
                file_frames: SCENE_SPAN,
                reversed: false,
                fade_in: 0,
                fade_out: 0,
                fade_in_curve: 0.0,
                fade_out_curve: 0.0,
                envelope: Vec::new(),
            }),
            ..Clip::default()
        };
        (clip, peaks)
    }

    /// Drive a real pointer over a real editor. `grid` of zero means no
    /// grid snap, so a test can assert an exact frame.
    fn drive(editor: &mut Editor, clip: &Clip, waveform: &Peaks, path: &[probe::Step]) {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut focus = Focus::default();
        probe::run(&context, SCENE, path, |ui| {
            body(
                ui,
                &mut focus,
                &theme,
                editor,
                Some(clip),
                Some(waveform),
                TimeView {
                    bpm: 120.0,
                    beats_per_bar: 4,
                    playhead: -1.0,
                    grid_beats: 0.0,
                    grid_name: "off",
                    follow: false,
                },
            );
        });
    }

    /// The clip frame a display x lands on, once the view has been fitted.
    fn frame_at_x(x: f32) -> u64 {
        let display = scene_display();
        let fpp = f64::from(SCENE_SPAN as f32) / f64::from(display.width());
        (f64::from(x - display.left()) * fpp).round() as u64
    }

    /// LEFT DRAG SELECTS. It used to pan, and the change is the point:
    /// every destructive verb in the editor hangs off a selection, and a
    /// gesture that panned instead would leave them all unreachable.
    #[test]
    fn a_drag_on_the_display_selects_and_does_not_pan() {
        let (clip, waveform) = scene_clip(1);
        let mut editor = Editor::default();
        let display = scene_display();
        let y = display.center().y;
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 60.0, y),
                egui::pos2(display.left() + 260.0, y),
                8,
            ),
        );
        let selection = editor.selection().expect("a drag selects");
        let from = frame_at_x(display.left() + 60.0);
        let to = frame_at_x(display.left() + 260.0);
        assert!(
            selection.from.abs_diff(from) < 200,
            "{} is not near {from}",
            selection.from
        );
        assert!(
            selection.to.abs_diff(to) < 200,
            "{} is not near {to}",
            selection.to
        );
        assert_eq!(editor.scroll_frames, 0.0, "and it did NOT pan");
        assert_eq!(editor.cursor, selection.from, "the cursor follows");
    }

    /// A drag that runs off the display selects to the clip's edge and
    /// stops there, rather than selecting past the end of the audio.
    #[test]
    fn a_drag_off_the_display_clamps_to_the_clip() {
        let (clip, waveform) = scene_clip(1);
        let mut editor = Editor::default();
        let display = scene_display();
        let y = display.center().y;
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 100.0, y),
                egui::pos2(display.right() + 50.0, y),
                6,
            ),
        );
        let selection = editor.selection().expect("a selection");
        assert_eq!(selection.to, SCENE_SPAN, "clamped to the clip's end");
        assert!(selection.from < SCENE_SPAN);
    }

    /// A DRAG BELONGS TO THE EDGE IT STARTED ON.
    ///
    /// Dragged past its neighbour, the grip keeps following the pointer
    /// and the two edges swap — it does not hand the gesture over, and it
    /// does not die halfway. That is the device UI contract's first rule,
    /// and the bug it was written after.
    #[test]
    fn an_edge_drag_stays_on_its_edge_past_the_other_one() {
        let (clip, waveform) = scene_clip(1);
        let mut editor = Editor::default();
        let display = scene_display();
        let y = display.center().y;
        // A selection in the middle of the clip, made with the pointer so
        // the view is fitted exactly as a later drag will see it.
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 200.0, y),
                egui::pos2(display.left() + 300.0, y),
                4,
            ),
        );
        let before = editor.selection().expect("a selection");

        // Now take the LEFT edge and pull it well past the right one.
        let scale = Scale {
            left: display.left(),
            scroll: editor.scroll_frames,
            fpp: editor.frames_per_pixel,
        };
        let left_x = scale.x_of(before.from as f64);
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(left_x, y),
                egui::pos2(display.left() + 460.0, y),
                10,
            ),
        );
        let after = editor.selection().expect("still a selection");
        assert!(after.to > after.from, "the range stayed well formed");
        // The far end is where the pointer ended up: the grip followed it
        // the whole way rather than stopping at the other edge.
        let landed = frame_at_x(display.left() + 460.0);
        assert!(
            after.to.abs_diff(landed) < 300,
            "the drag stopped at {} instead of {landed}",
            after.to
        );
        // And the edge it swapped with is where the old right edge was.
        assert!(
            after.from.abs_diff(before.to) < 300,
            "the other edge moved: {} was {}",
            after.from,
            before.to
        );
    }

    /// A click is a CURSOR, not a zero-length selection — a stray click
    /// must not leave something behind for a destructive verb to act on.
    #[test]
    fn a_click_sets_the_cursor_and_clears_the_selection() {
        let (clip, waveform) = scene_clip(1);
        let mut editor = Editor::default();
        let display = scene_display();
        let y = display.center().y;
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 60.0, y),
                egui::pos2(display.left() + 260.0, y),
                6,
            ),
        );
        assert!(editor.selection().is_some());

        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::click_path(egui::pos2(display.left() + 400.0, y)),
        );
        assert!(editor.selection().is_none(), "the click cleared it");
        let want = frame_at_x(display.left() + 400.0);
        assert!(
            editor.cursor.abs_diff(want) < 200,
            "cursor {} is not near {want}",
            editor.cursor
        );
    }

    /// A drag that crossed a lane meant to include it.
    #[test]
    fn a_drag_across_lanes_selects_both_channels() {
        let (clip, waveform) = scene_clip(2);
        let mut editor = Editor::default();
        let display = scene_display();
        let lane = display.height() / 2.0;
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 60.0, display.top() + lane * 0.5),
                egui::pos2(display.left() + 260.0, display.top() + lane * 1.5),
                8,
            ),
        );
        let selection = editor.selection().expect("a selection");
        assert!(selection.channels.contains(0), "started in the left lane");
        assert!(selection.channels.contains(1), "and crossed into the right");
        assert_eq!(selection.channels.iter(2).count(), 2);

        // A drag that stays in one lane selects one channel.
        let mut editor = Editor::default();
        drive(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(
                egui::pos2(display.left() + 60.0, display.top() + lane * 0.5),
                egui::pos2(display.left() + 260.0, display.top() + lane * 0.6),
                8,
            ),
        );
        let selection = editor.selection().expect("a selection");
        assert_eq!(selection.channels, ChannelMask::single(0));
    }

    #[test]
    fn channel_at_puts_every_pixel_in_a_lane() {
        let display = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(50.0, 200.0));
        assert_eq!(channel_at(display, 2, 100.0), 0);
        assert_eq!(channel_at(display, 2, 149.0), 0);
        assert_eq!(channel_at(display, 2, 151.0), 1);
        assert_eq!(channel_at(display, 2, 199.0), 1);
        // Off either end lands in the nearest lane rather than panicking
        // or naming a channel that is not there.
        assert_eq!(channel_at(display, 2, -500.0), 0);
        assert_eq!(channel_at(display, 2, 5_000.0), 1);
        assert_eq!(channel_at(display, 1, 150.0), 0);
    }

    #[test]
    fn a_channel_mask_holds_what_it_was_given() {
        let mut mask = ChannelMask::default();
        assert!(mask.is_empty());
        mask.set(1);
        assert!(mask.contains(1) && !mask.contains(0));
        mask.toggle(0);
        assert_eq!(mask.iter(2).count(), 2);
        mask.toggle(1);
        assert_eq!(mask.iter(2).collect::<Vec<_>>(), vec![0]);
        assert_eq!(ChannelMask::all(2).iter(2).count(), 2);
        assert_eq!(ChannelMask::all(0), ChannelMask::default());
        // A channel past the mask's width is refused rather than wrapping
        // onto another one.
        let mut wide = ChannelMask::default();
        wide.set(64);
        assert!(wide.is_empty());
    }

    fn snap_scene<'a>(window: Option<&'a SampleWindow>, grid: f64, zero: bool) -> Snap<'a> {
        Snap {
            grid,
            zero,
            span: 48_000,
            region_end: 48_000,
            window,
        }
    }

    #[test]
    fn grid_snap_lands_on_grid_frames() {
        let source = stub_source(48_000, 0, 48_000, 48_000);
        // A 1/16 grid at 120 BPM is 6 000 frames.
        let snap = snap_scene(None, 6_000.0, false);
        assert_eq!(snap.apply(0, 0, &source, false), 0);
        assert_eq!(snap.apply(2_900, 0, &source, false), 0);
        assert_eq!(snap.apply(3_100, 0, &source, false), 6_000);
        assert_eq!(snap.apply(11_000, 0, &source, false), 12_000);
        // Fifty drags in a row do not drift off the grid.
        let mut frame = 0;
        for step in 0..50u64 {
            frame = snap.apply(step * 6_000 + 17, 0, &source, false);
            assert_eq!(frame % 6_000, 0, "step {step} landed on {frame}");
        }
        assert_eq!(frame, 48_000, "and the last one stops at the clip's end");
        // ALT BYPASSES IT: a gesture must always be able to say "here".
        assert_eq!(snap.apply(3_100, 0, &source, true), 3_100);
        // And it never leaves the clip.
        assert_eq!(snap.apply(47_999, 0, &source, false), 48_000);
    }

    /// A sine crosses zero every half period; the snap finds the nearest
    /// one and does not walk off looking for a better one.
    #[test]
    fn zero_crossing_snap_finds_the_nearest_crossing_within_the_window() {
        let source = stub_source(48_000, 0, 48_000, 48_000);
        // Period 200 frames: crossings at 0, 100, 200, ...
        let samples: Vec<f32> = (0..48_000)
            .map(|frame| (frame as f32 * std::f32::consts::TAU / 200.0).sin())
            .collect();
        let window = SampleWindow {
            key: WindowKey {
                path: PathBuf::from("scene.wav"),
                start: 0,
                frames: 48_000,
            },
            channels: 1,
            samples,
        };
        let snap = snap_scene(Some(&window), 0.0, true);
        for (asked, nearest) in [(97u64, 100u64), (104, 100), (203, 200), (1_998, 2_000)] {
            let got = snap.apply(asked, 0, &source, false);
            assert!(
                got.abs_diff(nearest) <= 1,
                "{asked} snapped to {got}, not {nearest}"
            );
        }
        // Alt bypasses this one too.
        assert_eq!(snap.apply(97, 0, &source, true), 97);
    }

    /// A recording with no crossing in reach keeps the frame the user
    /// pointed at, rather than jumping somewhere arbitrary or refusing to
    /// move at all.
    #[test]
    fn zero_crossing_snap_falls_back_when_the_window_holds_none() {
        let source = stub_source(48_000, 0, 48_000, 48_000);
        let window = SampleWindow {
            key: WindowKey {
                path: PathBuf::from("scene.wav"),
                start: 0,
                frames: 48_000,
            },
            channels: 1,
            // Wholly positive: a DC-offset recording has no crossing at all.
            samples: vec![0.5; 48_000],
        };
        let snap = snap_scene(Some(&window), 0.0, true);
        assert_eq!(snap.apply(12_345, 0, &source, false), 12_345);

        // And with no window resident at all, the snap is skipped rather
        // than the frame stalling until a read lands.
        let snap = snap_scene(None, 0.0, true);
        assert_eq!(snap.apply(12_345, 0, &source, false), 12_345);
    }

    #[test]
    fn selection_is_cleared_when_the_editor_follows_a_different_clip() {
        let mut editor = Editor {
            span: 1_000,
            channels: 2,
            ..Default::default()
        };
        editor.follow_clip(Some(1));
        editor.select_all();
        editor.cursor = 400;
        assert_eq!(editor.selection().map(|s| s.to), Some(1_000));

        editor.follow_clip(Some(1));
        assert!(editor.selection().is_some(), "the same clip keeps it");

        editor.follow_clip(Some(2));
        assert!(editor.selection().is_none(), "a different clip does not");
        assert_eq!(editor.cursor, 0);
    }

    #[test]
    fn select_none_keeps_the_cursor() {
        let mut editor = Editor {
            span: 1_000,
            channels: 1,
            ..Default::default()
        };
        editor.select_all();
        editor.cursor = 250;
        editor.select_none();
        assert!(editor.selection().is_none());
        assert_eq!(editor.cursor, 250, "clearing is not forgetting");
    }

    // ---- WA-03: the selection made audible and measurable ---------------

    /// A range of clip frames names ONE range of the file — or nothing,
    /// which is the honest answer across a loop seam.
    #[test]
    fn a_selection_maps_to_one_source_range() {
        let source = stub_source(48_000, 1_000, 400, 10_000);
        let end = region_end(&source, 10_000);
        assert_eq!(source_span(&source, end, 0, 400), Some((1_000, 1_400)));
        assert_eq!(source_span(&source, end, 100, 200), Some((1_100, 1_200)));
        assert_eq!(source_span(&source, end, 400, 500), None, "past the region");
        assert_eq!(source_span(&source, end, 100, 100), None, "empty");

        // REVERSED: the range maps to its mirror, and the mirror agrees
        // with what one frame at a time says — the picture, the readout
        // and the edit must all be looking at the same audio.
        let reversed = AudioSource {
            reversed: true,
            ..source.clone()
        };
        let (from, to) = source_span(&reversed, end, 0, 100).expect("mirrored");
        assert_eq!((from, to), (1_300, 1_400));
        assert_eq!(source_frame_of(&reversed, end, 0), Some(1_399));
        assert_eq!(source_frame_of(&reversed, end, 99), Some(1_300));

        // LOOPED: inside one pass it maps; across the seam it does not,
        // because there is no one stretch of file for it to be.
        let looped = AudioSource {
            looped: true,
            ..source
        };
        assert_eq!(source_span(&looped, end, 500, 600), Some((1_100, 1_200)));
        assert_eq!(source_span(&looped, end, 350, 450), None, "across the seam");
    }

    /// The readout's peak is the file's peak, off the pyramid.
    #[test]
    fn readout_peak_matches_offline_peak() {
        // A quiet ramp with one loud sample in the middle.
        let mut samples = vec![0.1f32; 4_096];
        samples[2_000] = -0.8;
        let waveform = peaks(&samples, 1);
        let (peak, rms) = waveform.levels(&[0], 0, 4_096).expect("levels");
        assert!((peak - dbfs(0.8)).abs() < 0.5, "peak read {peak} dBFS");
        // Nearly everything is 0.1, so the RMS sits just above −20 dBFS.
        assert!(rms > -21.0 && rms < -19.0, "rms read {rms} dBFS");
        assert!(rms < peak, "RMS is never above the peak");

        // Silence reads the floor rather than minus infinity.
        let quiet = peaks(&vec![0.0f32; 1_024], 1);
        let (peak, rms) = quiet.levels(&[0], 0, 1_024).expect("levels");
        assert_eq!((peak, rms), (DB_FLOOR, DB_FLOOR));

        // A channel that is not selected does not count towards it.
        let stereo = peaks(&[0.9, 0.01, 0.9, 0.01], 2);
        let (left, _) = stereo.levels(&[0], 0, 2).expect("left");
        let (right, _) = stereo.levels(&[1], 0, 2).expect("right");
        assert!(left > right + 20.0, "{left} vs {right}");
    }

    #[test]
    fn arrow_keys_move_the_cursor_by_one_grid_unit() {
        let mut editor = Editor {
            span: 48_000,
            channels: 1,
            ..Default::default()
        };
        editor.nudge_cursor(6_000, false);
        assert_eq!(editor.cursor, 6_000);
        assert!(
            editor.selection().is_none(),
            "a plain arrow does not select"
        );
        editor.nudge_cursor(6_000, false);
        assert_eq!(editor.cursor, 12_000);

        // SHIFT drags the selection's far end along, from where the
        // cursor was.
        editor.nudge_cursor(6_000, true);
        let selection = editor.selection().expect("shift selects");
        assert_eq!((selection.from, selection.to), (12_000, 18_000));
        editor.nudge_cursor(6_000, true);
        let selection = editor.selection().expect("and keeps growing");
        assert_eq!((selection.from, selection.to), (12_000, 24_000));
        // Back past the anchor, the range turns round rather than
        // collapsing to nothing.
        editor.nudge_cursor(-24_000, true);
        let selection = editor.selection().expect("still a range");
        assert_eq!((selection.from, selection.to), (0, 12_000));

        // Neither end ever leaves the clip.
        editor.nudge_cursor(-999_999, false);
        assert_eq!(editor.cursor, 0);
        editor.nudge_cursor(999_999, false);
        assert_eq!(editor.cursor, 48_000);
    }

    #[test]
    fn zoom_to_selection_frames_it_with_air_at_both_ends() {
        let mut editor = Editor {
            span: 48_000,
            channels: 1,
            display_width: 500.0,
            frames_per_pixel: 96.0,
            ..Default::default()
        };
        editor.select_all();
        editor.set_selection(10_000, 20_000, ChannelMask::single(0));
        editor.zoom_to_selection();
        let scale = Scale {
            left: 0.0,
            scroll: editor.scroll_frames,
            fpp: editor.frames_per_pixel,
        };
        let left = scale.x_of(10_000.0);
        let right = scale.x_of(20_000.0);
        assert!(left > 0.0, "the near edge is grabbable, not on the edge");
        assert!(right < 500.0, "and so is the far one");
        assert!(right - left > 400.0, "but it fills most of the display");

        // Nothing selected, nothing to zoom to — and the view is left
        // exactly where it was rather than jumping to the origin.
        let mut editor = Editor {
            span: 48_000,
            display_width: 500.0,
            frames_per_pixel: 96.0,
            scroll_frames: 1_234.0,
            ..Default::default()
        };
        editor.zoom_to_selection();
        assert_eq!(editor.frames_per_pixel, 96.0);
        assert_eq!(editor.scroll_frames, 1_234.0);
    }

    /// Follow PAGES rather than centring, and only when the playhead has
    /// actually left the view.
    #[test]
    fn follow_pages_the_view_rather_than_sliding_it() {
        let (clip, _) = scene_clip(1);
        let source = clip.audio.clone().expect("audio");
        let time = |playhead: f32| TimeView {
            bpm: 120.0,
            beats_per_bar: 4,
            playhead,
            grid_beats: 0.0,
            grid_name: "off",
            follow: true,
        };
        // Half a second on screen out of the clip's one second.
        let mut editor = Editor {
            frames_per_pixel: 48.0,
            ..Default::default()
        };
        // The playhead a quarter in is already visible: nothing moves.
        follow_playhead(&mut editor, &clip, &source, time(0.5), 500.0, SCENE_SPAN);
        assert_eq!(editor.scroll_frames, 0.0, "still on screen");

        // Past the far margin, the view pages so it is near the left.
        follow_playhead(&mut editor, &clip, &source, time(1.6), 500.0, SCENE_SPAN);
        assert!(editor.scroll_frames > 0.0);
        let visible = 500.0 * 48.0;
        let frame = 1.6 / 2.0 * SCENE_SPAN as f32;
        assert!(
            f64::from(frame) - editor.scroll_frames < visible * 0.2,
            "it paged rather than centred"
        );

        // A playhead outside the clip is not this clip's business.
        let before = editor.scroll_frames;
        follow_playhead(&mut editor, &clip, &source, time(-3.0), 500.0, SCENE_SPAN);
        assert_eq!(editor.scroll_frames, before);
        follow_playhead(&mut editor, &clip, &source, time(99.0), 500.0, SCENE_SPAN);
        assert_eq!(editor.scroll_frames, before);
    }

    /// FOLLOW DOES NOT FIGHT A DRAG. The view moving under the pointer
    /// mid-selection would make the range jump away from the hand.
    #[test]
    fn follow_does_not_fight_a_drag() {
        let (clip, waveform) = scene_clip(1);
        let mut editor = Editor::default();
        let display = scene_display();
        let y = display.center().y;
        // Zoomed in enough that following would have to scroll.
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut focus = Focus::default();
        let path = probe::drag_path(
            egui::pos2(display.left() + 20.0, y),
            egui::pos2(display.left() + 120.0, y),
            8,
        );
        let scrolls = probe::run(&context, SCENE, &path, |ui| {
            editor.frames_per_pixel = 8.0;
            editor.fit_pending = false;
            body(
                ui,
                &mut focus,
                &theme,
                &mut editor,
                Some(&clip),
                Some(&waveform),
                TimeView {
                    bpm: 120.0,
                    beats_per_bar: 4,
                    // Well past the right of the view: follow would page.
                    playhead: 1.9,
                    grid_beats: 0.0,
                    grid_name: "off",
                    follow: true,
                },
            );
            editor.scroll_frames
        });
        // Before the press, following the playhead is exactly right and
        // it does move. What must not happen is a scroll WHILE the button
        // is down — so every frame from the drag onwards is the same one.
        let during = &scrolls[3..];
        assert!(
            during.windows(2).all(|pair| pair[0] == pair[1]),
            "the view moved mid-drag: {during:?}"
        );
        assert!(editor.selection().is_some(), "and the drag still selected");
    }

    // ---- WA-08: the fade curves ------------------------------------------

    /// Drive the editor over a clip that HAS fades, and collect what it
    /// asked for.
    fn drive_faded(
        editor: &mut Editor,
        clip: &Clip,
        waveform: &Peaks,
        path: &[probe::Step],
    ) -> Vec<ClipEdit> {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut focus = Focus::default();
        probe::run(&context, SCENE, path, |ui| {
            body(
                ui,
                &mut focus,
                &theme,
                editor,
                Some(clip),
                Some(waveform),
                TimeView {
                    bpm: 120.0,
                    beats_per_bar: 4,
                    playhead: -1.0,
                    grid_beats: 0.0,
                    grid_name: "off",
                    follow: false,
                },
            )
        })
        .into_iter()
        .flatten()
        .collect()
    }

    /// A clip with a long fade at each end, so both curve grips are on
    /// screen and far apart.
    fn faded_clip() -> (Clip, Peaks) {
        let (mut clip, peaks) = scene_clip(1);
        if let Some(audio) = clip.audio.as_mut() {
            audio.fade_in = SCENE_SPAN / 4;
            audio.fade_out = SCENE_SPAN / 4;
        }
        (clip, peaks)
    }

    /// EACH GRIP MOVES ITS OWN FADE. Two shapes on one clip, two
    /// interactions, and a drag on one must not touch the other — the
    /// device UI contract's first rule, on the newest pair of handles.
    #[test]
    fn dragging_the_curve_point_changes_only_its_own_fade() {
        let (clip, waveform) = faded_clip();
        let display = scene_display();
        // The fade IN spans the first quarter, so its curve grip sits at
        // an eighth of the way across.
        let x = display.left() + display.width() * 0.125;
        let y = display.bottom() - display.height() * 0.5;
        let mut editor = Editor::default();
        // A settling frame first, so the fit has happened and the grip is
        // where the arithmetic below says it is.
        drive_faded(
            &mut editor,
            &clip,
            &waveform,
            &[probe::Step::moved(egui::pos2(x, y))],
        );
        let edits = drive_faded(
            &mut editor,
            &clip,
            &waveform,
            &probe::drag_path(egui::pos2(x, y), egui::pos2(x, y - 40.0), 6),
        );
        let curves: Vec<&ClipEdit> = edits
            .iter()
            .filter(|edit| matches!(edit, ClipEdit::FadeInCurve(_) | ClipEdit::FadeOutCurve(_)))
            .collect();
        assert!(!curves.is_empty(), "the drag moved a curve");
        assert!(
            curves
                .iter()
                .all(|edit| matches!(edit, ClipEdit::FadeInCurve(_))),
            "and only the leading one: {curves:?}"
        );
        // Dragging UP raises the shape, which is the direction the curve
        // visibly bulges.
        let Some(ClipEdit::FadeInCurve(shape)) = curves.last() else {
            panic!("a leading curve edit");
        };
        assert!(*shape > 0.0, "up is a fuller fade: {shape}");

        // The LENGTH grips are untouched by it.
        assert!(
            !edits
                .iter()
                .any(|edit| matches!(edit, ClipEdit::FadeIn(_) | ClipEdit::FadeOut(_))),
            "a shape drag must not resize the fade"
        );
    }

    /// A curve grip is a VERTICAL control. A drag that wanders sideways
    /// keeps its grip and keeps changing the same number — the failure
    /// this rules out is the one the equaliser shipped, where a handle
    /// that could not move on an axis lost the drag when the pointer
    /// walked off it.
    #[test]
    fn a_curve_drag_that_wanders_horizontally_keeps_its_grip() {
        let (clip, waveform) = faded_clip();
        let display = scene_display();
        let x = display.left() + display.width() * 0.125;
        let y = display.bottom() - display.height() * 0.5;
        let mut editor = Editor::default();
        drive_faded(
            &mut editor,
            &clip,
            &waveform,
            &[probe::Step::moved(egui::pos2(x, y))],
        );
        let edits = drive_faded(
            &mut editor,
            &clip,
            &waveform,
            // Far to the right AND up: well off the grip's own rectangle.
            &probe::drag_path(egui::pos2(x, y), egui::pos2(x + 220.0, y - 60.0), 10),
        );
        let curves: Vec<f32> = edits
            .iter()
            .filter_map(|edit| match edit {
                ClipEdit::FadeInCurve(shape) => Some(*shape),
                _ => None,
            })
            .collect();
        assert!(
            curves.len() > 2,
            "the grip followed the pointer the whole way: {curves:?}"
        );
        assert!(
            !edits
                .iter()
                .any(|edit| matches!(edit, ClipEdit::FadeOutCurve(_))),
            "and never leaked onto the other end"
        );
    }

    // ---- WA-09: the clip gain envelope -----------------------------------

    #[test]
    fn the_envelope_scale_round_trips_between_db_and_pixels() {
        let display = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(200.0, 300.0));
        for db in [ENVELOPE_FLOOR_DB, -30.0, -6.0, 0.0, ENVELOPE_CEIL_DB] {
            let back = envelope_db(display, envelope_y(display, db));
            assert!((back - db).abs() < 0.05, "{db} came back as {back}");
        }
        // The ceiling is the top edge and the floor is the bottom one.
        assert!((envelope_y(display, ENVELOPE_CEIL_DB) - display.top()).abs() < 0.01);
        assert!((envelope_y(display, ENVELOPE_FLOOR_DB) - display.bottom()).abs() < 0.01);
        // And a pointer outside the display is held inside the range
        // rather than naming a gain that does not exist.
        assert_eq!(envelope_db(display, -9_000.0), ENVELOPE_CEIL_DB);
        assert_eq!(envelope_db(display, 9_000.0), ENVELOPE_FLOOR_DB);
    }

    /// A point dragged past its neighbour STOPS BESIDE IT rather than
    /// through it. Two points at one frame is a vertical step, and the
    /// node's interpolation would divide by their zero-length gap.
    #[test]
    fn envelope_points_stay_ordered_under_a_drag_past_a_neighbour() {
        let points = vec![(0u64, 0.0f32), (1_000, -6.0), (2_000, 0.0)];
        // The middle point's room is one frame inside each neighbour.
        assert_eq!(next_frame_before(&points, 0), Some(1));
        assert_eq!(point_frame_after(&points, 1), Some(1_999));
        // The ends have the clip itself as their other bound, which the
        // caller supplies.
        assert_eq!(point_frame_after(&points, 2), None);
        assert_eq!(next_frame_before(&points, 3), None);

        // Dragged far left, the middle point lands one frame after the
        // first — beside it, never on it.
        let low = next_frame_before(&points, 0).unwrap_or(0);
        let high = point_frame_after(&points, 1).unwrap_or(2_000);
        assert_eq!(0u64.clamp(low, high), 1);
        assert_eq!(99_999u64.clamp(low, high), 1_999);
    }

    /// The envelope is drawn ONLY when it is shown, and showing it is not
    /// the same as having one — a clip with no points still draws the
    /// flat unity line, so there is something to double-click on.
    #[test]
    fn a_hidden_envelope_is_neither_drawn_nor_grabbed() {
        let (clip, waveform) = scene_clip(1);
        let display = scene_display();
        let mut editor = Editor::default();
        let at = egui::pos2(display.center().x, display.center().y);

        // Hidden: a double-click in the middle of the display adds
        // nothing, because there is no envelope under the pointer.
        let edits = drive_faded(
            &mut editor,
            &clip,
            &waveform,
            &[
                probe::Step::moved(at),
                probe::Step::press(at),
                probe::Step::release(at),
                probe::Step::press(at),
                probe::Step::release(at),
            ],
        );
        assert!(
            !edits
                .iter()
                .any(|edit| matches!(edit, ClipEdit::Envelope(_))),
            "a hidden envelope is not under the pointer"
        );

        // Shown, the same gesture puts a point where it landed.
        editor.toggle_envelope();
        let edits = drive_faded(
            &mut editor,
            &clip,
            &waveform,
            &[
                probe::Step::moved(at),
                probe::Step::press(at),
                probe::Step::release(at),
                probe::Step::press(at),
                probe::Step::release(at),
            ],
        );
        let added = edits.iter().find_map(|edit| match edit {
            ClipEdit::Envelope(points) => Some(points),
            _ => None,
        });
        let points = added.expect("a double-click adds a point");
        assert!(!points.is_empty());
        // Roughly under the pointer, in both axes.
        let (frame, db) = points[0];
        assert!(frame > 0 && frame < SCENE_SPAN);
        assert!(db > ENVELOPE_FLOOR_DB && db < ENVELOPE_CEIL_DB, "{db}");
    }

    fn stub_source(rate: u32, offset: u64, frames: u64, file: u64) -> AudioSource {
        AudioSource {
            path: PathBuf::from("stub.wav"),
            sample_rate: rate,
            source_offset: offset,
            source_frames: frames,
            gain: 1.0,
            looped: false,
            file_frames: file,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        }
    }

    /// RMS SURVIVES THE PYRAMID.
    ///
    /// A level reading taken at a coarse zoom and one taken at a fine
    /// zoom are the same reading, or the number under the selection
    /// changes as you scroll — which would make it worthless.
    #[test]
    fn peak_rms_merges_correctly_across_levels() {
        // Full-scale square: every bin's RMS is 1, at every level.
        let square = (0..4_096)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect::<Vec<_>>();
        let waveform = peaks(&square, 1);
        assert!(waveform.levels.len() > 2, "several levels to merge across");
        let whole = waveform.extrema(0, 0, 4_096).expect("whole");
        assert!(
            (whole.rms - 1.0).abs() < 1e-4,
            "square RMS is 1: {}",
            whole.rms
        );

        // A sine's RMS is its amplitude over root two, whatever level the
        // query lands on.
        let sine = (0..8_192)
            .map(|index| (index as f32 * 0.05).sin())
            .collect::<Vec<_>>();
        let waveform = peaks(&sine, 1);
        let expected = 1.0 / 2.0f32.sqrt();
        for (start, end) in [(0u64, 8_192u64), (0, 4_096), (2_048, 6_144)] {
            let bin = waveform.extrema(0, start, end).expect("range");
            assert!(
                (bin.rms - expected).abs() < 0.02,
                "{start}..{end} read {} not {expected}",
                bin.rms
            );
        }
    }

    /// THE FRAME UNDER THE POINTER DOES NOT MOVE.
    ///
    /// This is the whole feel of a zoom. Get it wrong and going from the
    /// clip to one sample is a series of jumps the user has to chase
    /// across the display, which is what the old beat-based zoom did once
    /// the ceiling was raised.
    #[test]
    fn zoom_anchors_on_the_pointer() {
        let mut editor = Editor {
            scroll_frames: 5_000.0,
            frames_per_pixel: 64.0,
            ..Default::default()
        };
        for offset in [0.0, 137.0, 799.0] {
            let mut editor = Editor {
                scroll_frames: editor.scroll_frames,
                frames_per_pixel: editor.frames_per_pixel,
                ..Default::default()
            };
            let before = editor.scroll_frames + offset * editor.frames_per_pixel;
            zoom_about(&mut editor, offset, 1.2);
            let after = editor.scroll_frames + offset * editor.frames_per_pixel;
            assert!(
                (before - after).abs() < 1e-6,
                "offset {offset}: {before} became {after}"
            );
        }

        // And it survives the whole way down to the sample, in one run of
        // notches, without drifting.
        let anchor = editor.scroll_frames + 400.0 * editor.frames_per_pixel;
        for _ in 0..80 {
            zoom_about(&mut editor, 400.0, 1.2);
        }
        assert_eq!(editor.frames_per_pixel, FPP_MIN, "all the way in");
        let after = editor.scroll_frames + 400.0 * editor.frames_per_pixel;
        assert!((anchor - after).abs() < 0.5, "{anchor} became {after}");
    }

    #[test]
    fn fit_shows_the_whole_span_and_no_more() {
        let mut editor = Editor::default();
        fit_to(&mut editor, 96_000, 800.0);
        assert_eq!(editor.scroll_frames, 0.0);
        assert_eq!(editor.frames_per_pixel, 120.0);
        assert!(!editor.fit_pending);
        // The last frame lands on the display's right edge, not past it.
        let scale = Scale {
            left: 0.0,
            scroll: editor.scroll_frames,
            fpp: editor.frames_per_pixel,
        };
        assert!((scale.x_of(96_000.0) - 800.0).abs() < 0.01);

        // A degenerate clip leaves the view alone rather than dividing by
        // its length.
        let mut editor = Editor::default();
        let before = editor.frames_per_pixel;
        fit_to(&mut editor, 0, 800.0);
        assert_eq!(editor.frames_per_pixel, before);
    }

    /// A window is a PICTURE'S worth of samples, never a file's.
    #[test]
    fn sample_requests_are_bounded() {
        let samples = vec![0.25f32; 4_096];
        let waveform = peaks(&samples, 1);
        // A ten-minute file, one clip frame per pixel, an absurd display.
        let source = stub_source(48_000, 0, 28_800_000, 28_800_000);
        let mut editor = Editor {
            frames_per_pixel: 1.0,
            ..Default::default()
        };
        plan_window(&mut editor, &source, Some(&waveform), 28_800_000, 100_000.0);
        let want = editor.window_want.clone().expect("a window is wanted");
        assert!(
            want.frames <= WINDOW_MAX_FRAMES,
            "asked for {} frames",
            want.frames
        );

        // Zoomed out, no window is wanted at all: the pyramid is the
        // right answer and a read would be waste.
        let mut editor = Editor {
            frames_per_pixel: SAMPLE_DRAW_FPP,
            ..Default::default()
        };
        plan_window(&mut editor, &source, Some(&waveform), 28_800_000, 800.0);
        assert!(editor.window_want.is_none());
    }

    /// The same request is not made twice, or a scroll would queue sixty
    /// reads a second and the display would never catch up with itself.
    #[test]
    fn a_resident_window_is_not_requested_again() {
        let samples = vec![0.25f32; 4_096];
        let waveform = peaks(&samples, 1);
        let source = stub_source(48_000, 0, 4_096, 4_096);
        let mut editor = Editor {
            frames_per_pixel: 1.0,
            ..Default::default()
        };
        plan_window(&mut editor, &source, Some(&waveform), 4_096, 800.0);
        let key = editor.take_window_request().expect("a first request");
        assert!(editor.window_want.is_none(), "taken, so not wanted again");

        // Still outstanding: the same want does not queue a second read.
        plan_window(&mut editor, &source, Some(&waveform), 4_096, 800.0);
        assert!(editor.window_want.is_none(), "already pending");

        // Once it lands and covers the view, nothing more is wanted.
        editor.accept_window(WindowResult {
            key: key.clone(),
            result: Ok(Arc::new(SampleWindow {
                key: key.clone(),
                channels: 1,
                samples: vec![0.0; key.frames as usize],
            })),
        });
        plan_window(&mut editor, &source, Some(&waveform), 4_096, 800.0);
        assert!(editor.window_want.is_none(), "covered");
    }

    /// The dB scale spreads the quiet part of the range and keeps the
    /// sign, so the picture is still a waveform.
    #[test]
    fn db_scale_maps_the_floor_to_the_centre_line() {
        assert_eq!(amplitude(0.7, false), 0.7, "linear is the identity");
        assert_eq!(amplitude(0.0, true), 0.0, "silence sits on the line");
        assert!(
            (amplitude(1.0, true) - 1.0).abs() < 1e-6,
            "full scale is full"
        );
        assert!((amplitude(-1.0, true) + 1.0).abs() < 1e-6, "and it signs");

        // A sample at the floor draws on the centre line; one below it
        // does not draw below the line.
        let floor = 10f32.powf(DB_FLOOR / 20.0);
        assert!(amplitude(floor, true).abs() < 1e-3);
        assert_eq!(amplitude(floor * 0.001, true), 0.0);

        // Halfway down in dB is halfway up the lane, which is the point:
        // linear would have put −36 dB at a sixty-fourth of the height.
        let half = 10f32.powf(DB_FLOOR / 40.0);
        assert!((amplitude(half, true) - 0.5).abs() < 1e-3);
        assert!(amplitude(half, false) < 0.02, "and linear buries it");
    }

    /// A ladder step is wide enough to label and no wider.
    #[test]
    fn the_time_ladder_leaves_room_for_its_labels() {
        for fpp in [0.03125f64, 0.5, 1.0, 3.9] {
            let step = time_grid_step(fpp);
            let width = step as f64 / fpp;
            assert!(width >= f64::from(TIME_LABEL_W), "fpp {fpp}: {width} px");
            assert!(
                width < f64::from(TIME_LABEL_W) * 3.0,
                "fpp {fpp}: {width} px"
            );
        }
        assert_eq!(time_grid_label(48_000, 48_000, 48_000), "1s");
        assert_eq!(time_grid_label(480, 480, 48_000), "10ms");
        assert_eq!(time_grid_label(37, 5, 48_000), "37");
    }

    /// A window answers by absolute source frame, and refuses politely
    /// outside itself.
    #[test]
    fn a_sample_window_is_addressed_by_source_frame() {
        let window = SampleWindow {
            key: WindowKey {
                path: PathBuf::from("w.wav"),
                start: 1_000,
                frames: 4,
            },
            channels: 2,
            samples: vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3, 0.4, -0.4],
        };
        assert_eq!(window.at(0, 1_000), Some(0.1));
        assert_eq!(window.at(1, 1_000), Some(-0.1));
        assert_eq!(window.at(0, 1_003), Some(0.4));
        assert_eq!(window.at(0, 999), None, "before the window");
        assert_eq!(window.at(0, 1_004), None, "after it");
        assert_eq!(window.at(2, 1_000), None, "no third channel");
        assert!(window.covers(Path::new("w.wav"), 1_000, 1_004));
        assert!(!window.covers(Path::new("w.wav"), 999, 1_004));
        assert!(!window.covers(Path::new("other.wav"), 1_000, 1_004));
    }

    /// Reading a stretch of a file gives the same numbers the pyramid was
    /// built from — one decode path, so a zoom cannot change the audio.
    #[test]
    fn a_sample_window_reads_the_frames_it_was_asked_for() {
        let path = temp_wav();
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .expect("create test WAV");
        for index in 0..1_000i32 {
            writer
                .write_sample(index as f32 / 1_000.0)
                .expect("write test sample");
        }
        writer.finalize().expect("finalize test WAV");

        let window = read_window(&WindowKey {
            path: path.clone(),
            start: 400,
            frames: 100,
        })
        .expect("read the window");
        assert_eq!(window.key.frames, 100);
        assert_eq!(window.channels, 1);
        assert!((window.at(0, 400).expect("first") - 0.4).abs() < 1e-6);
        assert!((window.at(0, 499).expect("last") - 0.499).abs() < 1e-6);

        // A read past the end comes back SHORT rather than padded: the
        // picture must not invent audio that is not on disk.
        let tail = read_window(&WindowKey {
            path: path.clone(),
            start: 950,
            frames: 500,
        })
        .expect("read the tail");
        assert_eq!(tail.key.frames, 50);
        std::fs::remove_file(&path).expect("remove test WAV");
    }
}
