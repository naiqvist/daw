//! Offline audio rendering: the one road every destructive clip edit takes.
//!
//! The engine streams clips from disk with creek, so there is no buffer in
//! memory for an edit to change. A destructive edit is therefore **a render
//! to a NEW file, and the clip repointed at it** — which is also what makes
//! it undoable, because the path is ordinary project model and the snapshot
//! history covers it for free.
//!
//! Two rules hold that up, and neither is negotiable:
//!
//! - **Nothing here is ever overwritten or deleted.** Undo restores the
//!   model; the model holds the old path; the old file has to still be
//!   there. A cache sweep is a later task, and it will need to be told what
//!   the open project still points at.
//! - **Renders are content-addressed.** The name is a hash of the source's
//!   identity and the operations applied, so the same edit twice is free,
//!   and a redo after an undo re-finds the file rather than rebuilding it.
//!
//! Green zone throughout: this runs on the WAV worker thread, and the audio
//! callback only ever meets the result as an ordinary WAV through the creek
//! stream it already opens.

use crate::library::{WavImportError, read_wav_f32};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The largest source a render will decode, in bytes of `f32` samples.
///
/// A guard rather than a limit anybody should meet: at 48 kHz stereo this
/// is about forty minutes. Past it the honest answer is a refusal the UI
/// can print, not an allocation that takes the machine down.
const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;

/// Which channels an operation touches.
///
/// Mirrors the editor's own mask. A masked operation leaves every other
/// channel BIT-IDENTICAL — not merely unchanged in level, identical — so a
/// stereo file edited on one side stays a file whose other side was never
/// rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Channels(pub u32);

impl Channels {
    pub fn all() -> Self {
        Self(u32::MAX)
    }

    pub fn contains(self, channel: usize) -> bool {
        channel >= 32 || self.0 & (1 << channel) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Which way a destructive fade runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FadeDir {
    In,
    Out,
}

/// What a normalize measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Normalize {
    /// The loudest sample lands on the target.
    Peak,
    /// The root mean square lands on the target. Louder-sounding than
    /// peak at the same number, and able to ask for a gain that clips —
    /// which `allow_clipping` decides the answer to.
    Rms,
}

/// One operation, in SOURCE frames of the file being rendered.
///
/// Every arm carries its own range and channel mask rather than the job
/// carrying one for all of them: a job is a list of edits, and the list is
/// what makes a compound edit (a fade, then a normalize) one render and one
/// undo step instead of two of each.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Zero the range.
    Silence {
        from: u64,
        to: u64,
        channels: Channels,
    },
    /// A constant level change over the range.
    Gain {
        from: u64,
        to: u64,
        channels: Channels,
        db: f32,
    },
    /// Scale the range so its peak or RMS lands on `target_db`.
    ///
    /// Measured ACROSS the masked channels together, not one at a time: a
    /// per-channel normalize would move a stereo image to wherever the
    /// quieter side happened to be.
    Norm {
        from: u64,
        to: u64,
        channels: Channels,
        target_db: f32,
        mode: Normalize,
        /// When false, a gain that would push a sample past full scale is
        /// reduced until it does not. RMS normalizing asks for that
        /// routinely, and clipping a file silently is not an answer.
        allow_clipping: bool,
    },
    /// The range, backwards — BY FRAME, never by sample, so the channels
    /// stay where they are. Reversing the interleaved buffer itself
    /// swaps left with right as well, which sounds almost right, which is
    /// worse.
    Reverse {
        from: u64,
        to: u64,
        channels: Channels,
    },
    /// Flip the sign.
    Invert {
        from: u64,
        to: u64,
        channels: Channels,
    },
    /// Subtract the range's own mean, per channel.
    RemoveDc {
        from: u64,
        to: u64,
        channels: Channels,
    },
    /// A gain ramp across the range, in the same curve family the clip's
    /// own fades use — so committing a fade cannot change how it sounds.
    Fade {
        from: u64,
        to: u64,
        channels: Channels,
        dir: FadeDir,
        curve: crate::params::clip::Curve,
    },
    /// Remove the range. The file gets SHORTER.
    ///
    /// No channel mask, and that is not an oversight: cutting one side of
    /// a stereo file would make it shorter than the other, and there is
    /// no such thing as a file whose channels have different lengths.
    /// Silencing in place is what a one-channel "delete" means, and it is
    /// [`Op::Silence`].
    Cut { from: u64, to: u64 },
    /// Insert silence at `at`. The file gets LONGER.
    InsertSilence { at: u64, frames: u64 },
    /// Insert material at `at`. The file gets LONGER.
    ///
    /// The material is interleaved at the destination's channel count and
    /// sample rate — the paste path converts before it gets here, because
    /// a render is not the place to discover that two files disagree.
    Insert {
        at: u64,
        material: Arc<Vec<f32>>,
        /// The rate the material was taken at. When it differs from the
        /// destination's, [`render`] resamples before the paste — through
        /// the SAME resampler an import uses, so pasted audio cannot
        /// sound subtly unlike the file it came from.
        material_rate: u32,
    },
    /// Keep only the range, discarding everything either side.
    Crop { from: u64, to: u64 },
    /// Exchange the two channels of a stereo file. Whole-file: half a
    /// swap is not a swap.
    SwapChannels,
    /// Fold to ONE channel.
    ToMono(Mono),
    /// Keep one channel, as a mono file.
    TakeChannel(usize),
    /// Make the buffer exactly `frames` long: repeated when `looped`,
    /// padded with silence when not.
    ///
    /// This is how a clip's TIMELINE SPAN is printed into a file. A
    /// looped clip runs through its region many times and a short one
    /// leaves silence at the end; both are what the node does, and
    /// flattening has to reproduce both or it changes the sound.
    Fit { frames: u64, looped: bool },
    /// A breakpoint gain ride, `(frame, linear gain)`, over the whole
    /// buffer — the clip envelope, committed.
    Envelope(Arc<Vec<(u64, f32)>>),
    /// Varispeed: resample the whole buffer so it plays `semitones`
    /// higher, and correspondingly shorter.
    ///
    /// A TAPE MACHINE, not a pitch shifter. Pitch and duration move
    /// together; separating them is time-stretch, which is warping, which
    /// this brief does not do.
    Transpose { semitones: f32 },
}

/// How a fold to mono chooses what to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mono {
    /// The average of every channel.
    ///
    /// The AVERAGE, not the sum: summing a correlated pair doubles it and
    /// clips material that did not clip before, which is the classic way
    /// to make a mono fold sound broken. A −3 dB "compensation" on top of
    /// a sum is the same thing said awkwardly; dividing by the channel
    /// count is the honest version and is exactly right for a correlated
    /// pair.
    Average,
    Left,
    Right,
}

impl Op {
    /// A varispeed transpose's length multiplier: the reciprocal of the
    /// frequency ratio, because pitch and duration move together.
    pub fn transpose_ratio(semitones: f32) -> f32 {
        2f32.powf(semitones / 12.0)
    }

    /// How this operation changes the file's length, and from which
    /// frame. `None` means the length is unchanged.
    ///
    /// The clip's region is adjusted from exactly this, so an operation
    /// that got it wrong would leave the clip playing the right file from
    /// the wrong place — which sounds like the edit worked and landed
    /// somewhere else.
    pub fn length_change(&self, channels: usize) -> Option<(u64, i64)> {
        let channels = channels.max(1) as i64;
        match self {
            Self::Cut { from, to } => Some((*from, -((to.saturating_sub(*from)) as i64))),
            Self::InsertSilence { at, frames } => Some((*at, *frames as i64)),
            Self::Insert { at, material, .. } => {
                Some((*at, (material.len() as i64) / channels.max(1)))
            }
            // A crop removes everything before the range as well, so from
            // the region's point of view the change lands at frame zero.
            Self::Crop { from, .. } => Some((0, -(*from as i64))),
            _ => None,
        }
    }

    /// Operations that change the buffer's LENGTH or its CHANNEL COUNT
    /// rebuild it rather than editing in place, and hand back the new
    /// channel count with it. `None` for the ones that edit in place.
    fn rebuild(
        &self,
        samples: &[f32],
        channels: usize,
        frames: u64,
        rate: u32,
    ) -> Option<(Vec<f32>, usize)> {
        let channels = channels.max(1);
        let clamp = |frame: u64| (frame.min(frames) as usize) * channels;
        match self {
            Self::Fit {
                frames: want,
                looped,
            } => {
                if *want == 0 || frames == 0 {
                    return None;
                }
                let wanted = (*want as usize).saturating_mul(channels);
                let mut out = Vec::with_capacity(wanted);
                if *looped {
                    // Repeated from the top, which is what the node does
                    // at its loop point.
                    while out.len() < wanted {
                        let room = wanted - out.len();
                        out.extend_from_slice(&samples[..room.min(samples.len())]);
                    }
                } else {
                    out.extend_from_slice(&samples[..wanted.min(samples.len())]);
                    // Padded with silence, which is what the node plays
                    // past the end of a short clip's material.
                    out.resize(wanted, 0.0);
                }
                Some((out, channels))
            }
            Self::Transpose { semitones } => {
                if *semitones == 0.0 || !semitones.is_finite() || rate == 0 {
                    return None;
                }
                // Playing back at `rate` a buffer resampled TO a lower
                // rate makes it shorter and higher — a tape machine sped
                // up. The ratio is the frequency multiplier.
                let ratio = 2f32.powf(semitones / 12.0);
                let target = ((f64::from(rate) / f64::from(ratio)).round() as u32).max(1);
                crate::library::resample_interleaved(samples, channels, rate, target)
                    .ok()
                    .map(|out| (out, channels))
            }
            Self::SwapChannels => {
                // Stereo only. Swapping "the two channels" of a file that
                // has five is not a defined request, and guessing which
                // two would be worse than refusing.
                if channels != 2 {
                    return None;
                }
                let mut out = samples.to_vec();
                for frame in out.as_chunks_mut::<2>().0 {
                    frame.swap(0, 1);
                }
                Some((out, 2))
            }
            Self::ToMono(mode) => {
                if channels == 1 {
                    return None;
                }
                let mut out = Vec::with_capacity(frames as usize);
                for frame in samples.chunks_exact(channels) {
                    out.push(match mode {
                        Mono::Average => frame.iter().sum::<f32>() / channels as f32,
                        Mono::Left => frame[0],
                        Mono::Right => frame[1.min(channels - 1)],
                    });
                }
                Some((out, 1))
            }
            Self::TakeChannel(channel) => {
                if *channel >= channels {
                    return None;
                }
                let out = samples
                    .chunks_exact(channels)
                    .map(|frame| frame[*channel])
                    .collect();
                Some((out, 1))
            }
            Self::Cut { from, to } => {
                let (from, to) = (clamp(*from), clamp(*to));
                if to <= from {
                    return None;
                }
                let mut out = Vec::with_capacity(samples.len() - (to - from));
                out.extend_from_slice(&samples[..from]);
                out.extend_from_slice(&samples[to..]);
                Some((out, channels))
            }
            Self::InsertSilence { at, frames: added } => {
                let at = clamp(*at);
                let added = (*added as usize) * channels;
                let mut out = Vec::with_capacity(samples.len() + added);
                out.extend_from_slice(&samples[..at]);
                out.extend(std::iter::repeat_n(0.0, added));
                out.extend_from_slice(&samples[at..]);
                Some((out, channels))
            }
            Self::Insert { at, material, .. } => {
                let at = clamp(*at);
                // Whole frames only. A tail of a partial frame would put
                // every later sample on the wrong channel, which is the
                // sort of bug that sounds like a phasing problem.
                let usable = material.len() - material.len() % channels;
                let mut out = Vec::with_capacity(samples.len() + usable);
                out.extend_from_slice(&samples[..at]);
                out.extend_from_slice(&material[..usable]);
                out.extend_from_slice(&samples[at..]);
                Some((out, channels))
            }
            Self::Crop { from, to } => {
                let (from, to) = (clamp(*from), clamp(*to));
                (to > from).then(|| (samples[from..to].to_vec(), channels))
            }
            _ => None,
        }
    }
}

impl Op {
    fn range(&self) -> (u64, u64) {
        match self {
            Self::Silence { from, to, .. }
            | Self::Gain { from, to, .. }
            | Self::Norm { from, to, .. }
            | Self::Reverse { from, to, .. }
            | Self::Invert { from, to, .. }
            | Self::RemoveDc { from, to, .. }
            | Self::Fade { from, to, .. }
            | Self::Cut { from, to }
            | Self::Crop { from, to } => (*from, *to),
            // The length-changing insertions have a point, not a range;
            // nothing that asks for a range acts on them.
            Self::InsertSilence { at, .. } | Self::Insert { at, .. } => (*at, *at),
            // The whole-file operations have no range.
            Self::SwapChannels
            | Self::ToMono(_)
            | Self::TakeChannel(_)
            | Self::Fit { .. }
            | Self::Envelope(_)
            | Self::Transpose { .. } => (0, 0),
        }
    }

    fn channels(&self) -> Channels {
        match self {
            Self::Silence { channels, .. }
            | Self::Gain { channels, .. }
            | Self::Norm { channels, .. }
            | Self::Reverse { channels, .. }
            | Self::Invert { channels, .. }
            | Self::RemoveDc { channels, .. }
            | Self::Fade { channels, .. } => *channels,
            // Length changes are whole-file by definition: a file cannot
            // have channels of different lengths.
            Self::Cut { .. }
            | Self::InsertSilence { .. }
            | Self::Insert { .. }
            | Self::Crop { .. }
            | Self::SwapChannels
            | Self::ToMono(_)
            | Self::TakeChannel(_)
            | Self::Fit { .. }
            | Self::Envelope(_)
            | Self::Transpose { .. } => Channels::all(),
        }
    }

    /// A stable identity for the cache key.
    ///
    /// Hand-written because floats do not implement `Hash`, and because
    /// the bit pattern is exactly the right thing to key on here: two
    /// jobs whose gains differ in the last bit are two different files.
    fn hash_into(&self, hasher: &mut impl Hasher) {
        let (from, to) = self.range();
        std::mem::discriminant(self).hash(hasher);
        from.hash(hasher);
        to.hash(hasher);
        self.channels().hash(hasher);
        match self {
            Self::Silence { .. }
            | Self::Reverse { .. }
            | Self::Invert { .. }
            | Self::RemoveDc { .. } => {}
            Self::Gain { db, .. } => db.to_bits().hash(hasher),
            Self::Norm {
                target_db,
                mode,
                allow_clipping,
                ..
            } => {
                target_db.to_bits().hash(hasher);
                mode.hash(hasher);
                allow_clipping.hash(hasher);
            }
            Self::Fade { dir, curve, .. } => {
                dir.hash(hasher);
                curve.shape.to_bits().hash(hasher);
            }
            Self::Cut { .. } | Self::Crop { .. } | Self::SwapChannels => {}
            Self::ToMono(mode) => mode.hash(hasher),
            Self::TakeChannel(channel) => channel.hash(hasher),
            Self::Fit { frames, looped } => {
                frames.hash(hasher);
                looped.hash(hasher);
            }
            Self::Envelope(points) => {
                for (at, gain) in points.iter() {
                    at.hash(hasher);
                    gain.to_bits().hash(hasher);
                }
            }
            Self::Transpose { semitones } => semitones.to_bits().hash(hasher),
            Self::InsertSilence { frames, .. } => frames.hash(hasher),
            // The MATERIAL, not just its length: two pastes of different
            // audio at the same spot are two different files, and a key
            // that could not tell them apart would hand back the first.
            Self::Insert {
                material,
                material_rate,
                ..
            } => {
                material_rate.hash(hasher);
                material.len().hash(hasher);
                for sample in material.iter() {
                    sample.to_bits().hash(hasher);
                }
            }
        }
    }

    /// Apply to an interleaved buffer in place.
    ///
    /// Length-changing operations do not go through here; they rebuild the
    /// buffer, and [`Job::apply`] is where that split lives.
    fn apply(&self, samples: &mut [f32], channels: usize, frames: u64) {
        let (from, to) = self.range();
        let from = from.min(frames) as usize;
        let to = to.min(frames) as usize;
        // The whole-file operations have no range; everything else is a
        // no-op over an empty one.
        if to <= from && !matches!(self, Self::Envelope(_)) {
            return;
        }
        let mask = self.channels();
        let masked = |channel: usize| mask.contains(channel);
        let index = |frame: usize, channel: usize| frame * channels + channel;
        match self {
            Self::Silence { .. } => {
                for frame in from..to {
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        samples[index(frame, channel)] = 0.0;
                    }
                }
            }
            Self::Gain { db, .. } => {
                let factor = db_to_linear(*db);
                for frame in from..to {
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        samples[index(frame, channel)] *= factor;
                    }
                }
            }
            Self::Norm {
                target_db,
                mode,
                allow_clipping,
                ..
            } => {
                let mut peak = 0.0f32;
                let mut energy = 0.0f64;
                let mut counted = 0u64;
                for frame in from..to {
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        let value = samples[index(frame, channel)];
                        peak = peak.max(value.abs());
                        energy += f64::from(value) * f64::from(value);
                        counted += 1;
                    }
                }
                if counted == 0 || peak <= 0.0 {
                    // Silence cannot be normalized. Scaling zero by
                    // anything is still zero, and a factor derived from it
                    // would be an infinity.
                    return;
                }
                let measured = match mode {
                    Normalize::Peak => peak,
                    Normalize::Rms => (energy / counted as f64).sqrt() as f32,
                };
                if measured <= 0.0 {
                    return;
                }
                let mut factor = db_to_linear(*target_db) / measured;
                if !allow_clipping && peak * factor > 1.0 {
                    factor = 1.0 / peak;
                }
                for frame in from..to {
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        samples[index(frame, channel)] *= factor;
                    }
                }
            }
            Self::Reverse { .. } => {
                for channel in (0..channels).filter(|c| masked(*c)) {
                    let (mut head, mut tail) = (from, to - 1);
                    while head < tail {
                        samples.swap(index(head, channel), index(tail, channel));
                        head += 1;
                        tail -= 1;
                    }
                }
            }
            Self::Invert { .. } => {
                for frame in from..to {
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        samples[index(frame, channel)] = -samples[index(frame, channel)];
                    }
                }
            }
            Self::RemoveDc { .. } => {
                // PER CHANNEL. One mean across a stereo pair whose sides
                // sit at different offsets would leave both of them off
                // zero, in opposite directions.
                let span = (to - from) as f64;
                for channel in (0..channels).filter(|c| masked(*c)) {
                    let mut sum = 0.0f64;
                    for frame in from..to {
                        sum += f64::from(samples[index(frame, channel)]);
                    }
                    let mean = (sum / span) as f32;
                    for frame in from..to {
                        samples[index(frame, channel)] -= mean;
                    }
                }
            }
            Self::Fade { dir, curve, .. } => {
                let span = (to - from) as f32;
                for frame in from..to {
                    let along = (frame - from) as f32 / span;
                    let position = match dir {
                        FadeDir::In => along,
                        FadeDir::Out => 1.0 - along,
                    };
                    let level = curve.at(position);
                    for channel in (0..channels).filter(|c| masked(*c)) {
                        samples[index(frame, channel)] *= level;
                    }
                }
            }
            // Handled by `rebuild`, which runs before this: these change
            // the buffer's length and cannot be done in place.
            Self::Cut { .. }
            | Self::InsertSilence { .. }
            | Self::Insert { .. }
            | Self::Crop { .. }
            | Self::SwapChannels
            | Self::ToMono(_)
            | Self::TakeChannel(_)
            | Self::Fit { .. }
            | Self::Transpose { .. } => {}
            // In place: it multiplies, it does not resize.
            Self::Envelope(points) => {
                if points.is_empty() {
                    return;
                }
                let mut cursor = 0usize;
                let total = samples.len() / channels;
                for frame in 0..total {
                    while cursor + 1 < points.len() && points[cursor + 1].0 <= frame as u64 {
                        cursor += 1;
                    }
                    let (at, gain) = points[cursor];
                    let level = match points.get(cursor + 1) {
                        Some(&(next_at, next_gain)) if next_at > at && frame as u64 > at => {
                            let along = (frame as u64 - at) as f32 / (next_at - at) as f32;
                            gain + (next_gain - gain) * along
                        }
                        _ => gain,
                    };
                    for channel in 0..channels {
                        samples[frame * channels + channel] *= level;
                    }
                }
            }
        }
    }
}

/// dB as a linear factor, with a floor rather than a zero-crossing into
/// negative gain.
fn db_to_linear(db: f32) -> f32 {
    if db <= -120.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// A source file and what to do to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub source: PathBuf,
    /// Applied in order. An empty list is not an error: it renders a plain
    /// copy, which is what "flatten a clip that is already flat" means.
    pub ops: Vec<Op>,
}

/// What a finished render produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// The file the job read, so a late answer can be matched to the clip
    /// that asked for it.
    pub source: PathBuf,
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
}

/// Where renders live.
///
/// The user's data directory, NOT the temp directory. The reverse cache
/// gets away with temp because a reversal can always be rebuilt from the
/// forward file; a destructive edit cannot be rebuilt from anything, and a
/// project that lost its audio on reboot would be a project that lost work.
///
/// Found the way `ui::skin` finds its config directory — the XDG variable
/// first, then the conventional path under `$HOME` — rather than by adding
/// a dependency for two lines of string joining.
pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    Some(base.join("daw").join("renders"))
}

/// Where this exact job's output belongs.
///
/// Keyed by the source's identity, size and mtime as well as the ops, so
/// editing the file underneath produces a different name rather than a
/// stale render — the same rule `library::reverse_cache_path` keeps, for
/// the same reason.
pub fn output_path(job: &Job) -> Result<PathBuf, WavImportError> {
    let metadata = job
        .source
        .metadata()
        .map_err(|source| WavImportError::Access {
            path: job.source.clone(),
            source,
        })?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    job.source.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    job.ops.len().hash(&mut hasher);
    for op in &job.ops {
        op.hash_into(&mut hasher);
    }
    let directory = cache_dir().ok_or_else(|| WavImportError::Access {
        path: job.source.clone(),
        source: std::io::Error::other("no data directory"),
    })?;
    std::fs::create_dir_all(&directory).map_err(|source| WavImportError::Access {
        path: directory.clone(),
        source,
    })?;
    Ok(directory.join(format!("{:016x}.wav", hasher.finish())))
}

impl Job {
    /// This job with every pasted buffer brought to `rate`.
    ///
    /// A no-op in the common case: the importer has already brought every
    /// file in the project to the device rate, so two clips almost always
    /// agree.
    fn converted_for(&self, rate: u32, channels: usize) -> Result<Self, WavImportError> {
        let mut ops = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            match op {
                Op::Insert {
                    at,
                    material,
                    material_rate,
                } if *material_rate != rate && *material_rate != 0 => {
                    let converted = crate::library::resample_interleaved(
                        material,
                        channels,
                        *material_rate,
                        rate,
                    )?;
                    ops.push(Op::Insert {
                        at: *at,
                        material: Arc::new(converted),
                        material_rate: rate,
                    });
                }
                other => ops.push(other.clone()),
            }
        }
        Ok(Self {
            source: self.source.clone(),
            ops,
        })
    }

    /// Run the ops over a decoded buffer, handing back the result and its
    /// new length in frames.
    ///
    /// Separate from the file work so the arithmetic is testable without a
    /// disk: every operation's correctness is a statement about samples,
    /// and a test that had to write a WAV to make it would be a test
    /// nobody writes enough of.
    pub fn apply(
        &self,
        mut samples: Vec<f32>,
        channels: usize,
        rate: u32,
    ) -> (Vec<f32>, u64, usize) {
        let mut channels = channels.max(1);
        let mut frames = (samples.len() / channels) as u64;
        for op in &self.ops {
            if let Some((rebuilt, new_channels)) = op.rebuild(&samples, channels, frames, rate) {
                samples = rebuilt;
                channels = new_channels.max(1);
            } else {
                op.apply(&mut samples, channels, frames);
            }
            frames = (samples.len() / channels) as u64;
        }
        (samples, frames, channels)
    }
}

/// Render `job`, or hand back the cached file if this exact job has been
/// run before.
///
/// Public and deterministic so it can be tested offline; the app goes
/// through the worker.
pub fn render(job: &Job) -> Result<Rendered, WavImportError> {
    let source = job
        .source
        .canonicalize()
        .map_err(|error| WavImportError::Access {
            path: job.source.clone(),
            source: error,
        })?;
    let job = Job {
        source: source.clone(),
        ops: job.ops.clone(),
    };
    let output = output_path(&job)?;

    let mut reader = hound::WavReader::open(&source).map_err(|error| WavImportError::Decode {
        path: source.clone(),
        source: error,
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(WavImportError::Unsupported(
            "zero channels or sample rate".to_owned(),
        ));
    }

    // Already built? Then this is free — and it must be, because a redo
    // after an undo runs the same job again and must not rewrite a file
    // the history is still pointing at.
    if let Ok(cached) = hound::WavReader::open(&output)
        && cached.spec().sample_rate == spec.sample_rate
    {
        // The CACHED file's channel count, not the source's: a fold to
        // mono produces a file with fewer channels than it read, and
        // reporting the source's would tell the clip to expect a stereo
        // file that is not there.
        return Ok(Rendered {
            source,
            path: output,
            sample_rate: spec.sample_rate,
            channels: cached.spec().channels,
            frames: u64::from(cached.duration()),
        });
    }

    let decoded = u64::from(reader.duration())
        .saturating_mul(u64::from(spec.channels))
        .saturating_mul(4);
    if decoded > MAX_DECODED_BYTES {
        return Err(WavImportError::Unsupported(format!(
            "{} MiB of audio is more than a render will decode",
            decoded / (1024 * 1024)
        )));
    }

    let channels = usize::from(spec.channels);
    let samples = read_wav_f32(&mut reader, &source)?;
    // Pasted material is brought to the destination's rate HERE, where
    // the destination's rate is known and an error can still be
    // returned. The ops themselves are pure arithmetic over a buffer and
    // are not the place to discover that two files disagree.
    let job = job.converted_for(spec.sample_rate, channels)?;
    let (samples, frames, channels) = job.apply(samples, channels, spec.sample_rate);
    let out_channels = u16::try_from(channels).unwrap_or(spec.channels);

    // Written beside the target and moved into place, so a render
    // interrupted halfway cannot leave a truncated file under a name that
    // says it is complete — and the next run would happily hand it back.
    let scratch = output.with_extension("part");
    let mut writer = hound::WavWriter::create(
        &scratch,
        hound::WavSpec {
            channels: out_channels,
            sample_rate: spec.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .map_err(|error| WavImportError::Decode {
        path: scratch.clone(),
        source: error,
    })?;
    for sample in &samples {
        writer
            .write_sample(*sample)
            .map_err(|error| WavImportError::Decode {
                path: scratch.clone(),
                source: error,
            })?;
    }
    writer.finalize().map_err(|error| WavImportError::Decode {
        path: scratch.clone(),
        source: error,
    })?;
    std::fs::rename(&scratch, &output).map_err(|error| WavImportError::Access {
        path: output.clone(),
        source: error,
    })?;

    Ok(Rendered {
        source,
        path: output,
        sample_rate: spec.sample_rate,
        channels: out_channels,
        frames,
    })
}

/// Audio taken off a file, for the clipboard.
///
/// Decoded samples rather than a file: a copy must be cheap, and it must
/// survive the file it came from being edited underneath — which a
/// reference to a range of a path would not.
#[derive(Debug, Clone, PartialEq)]
pub struct Extract {
    pub source: PathBuf,
    pub samples: Arc<Vec<f32>>,
    pub channels: u16,
    pub sample_rate: u32,
}

impl Extract {
    pub fn frames(&self) -> u64 {
        (self.samples.len() / usize::from(self.channels).max(1)) as u64
    }
}

/// Take `from..to` off a file.
///
/// Seeks rather than decoding from the top, for the same reason a sample
/// window does: copying four bars from the end of a long take should not
/// cost the whole take.
pub fn extract(path: &Path, from: u64, to: u64) -> Result<Extract, WavImportError> {
    let mut reader = hound::WavReader::open(path).map_err(|source| WavImportError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || to <= from {
        return Err(WavImportError::Unsupported("nothing to copy".to_owned()));
    }
    let channels = usize::from(spec.channels);
    let total = u64::from(reader.duration());
    let from = from.min(total);
    let to = to.min(total);
    let wanted = ((to - from) as usize).saturating_mul(channels);
    if wanted == 0 {
        return Err(WavImportError::Unsupported("nothing to copy".to_owned()));
    }
    let seek = u32::try_from(from).map_err(|_| {
        WavImportError::Unsupported("file is longer than a WAV frame index".to_owned())
    })?;
    reader.seek(seek).map_err(|error| WavImportError::Access {
        path: path.to_path_buf(),
        source: error,
    })?;
    let mut samples = read_wav_f32(&mut reader, path)?;
    samples.truncate(wanted);
    Ok(Extract {
        source: path.to_path_buf(),
        samples: Arc::new(samples),
        channels: spec.channels,
        sample_rate: spec.sample_rate,
    })
}

/// Read a whole WAV as interleaved `f32`, for tests and for callers that
/// need to compare a render against its source.
pub fn decode(path: &Path) -> Result<(Vec<f32>, u16, u32), WavImportError> {
    let mut reader = hound::WavReader::open(path).map_err(|source| WavImportError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    let samples = read_wav_f32(&mut reader, path)?;
    Ok((samples, spec.channels, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "daw-render-{name}-{}-{nonce}.wav",
            std::process::id()
        ))
    }

    fn write_ramp(path: &Path, channels: u16, frames: u64) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .expect("create source WAV");
        for frame in 0..frames {
            for channel in 0..channels {
                let value = frame as f32 / frames as f32 + f32::from(channel) * 0.5;
                writer.write_sample(value).expect("write sample");
            }
        }
        writer.finalize().expect("finalize source WAV");
    }

    /// The whole point of a masked operation: everything outside the range
    /// and outside the mask is what it was, to the bit.
    #[test]
    fn render_silence_zeroes_only_the_range() {
        let source = scratch("silence");
        write_ramp(&source, 1, 1_000);
        let before = decode(&source).expect("decode source").0;

        let job = Job {
            source: source.clone(),
            ops: vec![Op::Silence {
                from: 200,
                to: 300,
                channels: Channels::all(),
            }],
        };
        let out = render(&job).expect("render");
        assert_eq!(out.frames, 1_000, "silencing does not change the length");
        assert_eq!(out.sample_rate, 48_000);
        assert_eq!(out.channels, 1);

        let (after, _, _) = decode(&out.path).expect("decode render");
        assert_eq!(after.len(), before.len());
        for (frame, (was, now)) in before.iter().zip(&after).enumerate() {
            if (200..300).contains(&frame) {
                assert_eq!(*now, 0.0, "frame {frame} should be silent");
            } else {
                assert_eq!(now, was, "frame {frame} should be untouched");
            }
        }
        std::fs::remove_file(&source).ok();
        std::fs::remove_file(&out.path).ok();
    }

    #[test]
    fn a_channel_masked_op_leaves_the_other_channel_untouched() {
        let source = scratch("mask");
        write_ramp(&source, 2, 500);
        let (before, _, _) = decode(&source).expect("decode source");

        let job = Job {
            source: source.clone(),
            ops: vec![Op::Silence {
                from: 0,
                to: 500,
                // The left channel only.
                channels: Channels(0b01),
            }],
        };
        let out = render(&job).expect("render");
        let (after, channels, _) = decode(&out.path).expect("decode render");
        assert_eq!(channels, 2);
        for frame in 0..500usize {
            assert_eq!(after[frame * 2], 0.0, "left {frame}");
            assert_eq!(after[frame * 2 + 1], before[frame * 2 + 1], "right {frame}");
        }
        std::fs::remove_file(&source).ok();
        std::fs::remove_file(&out.path).ok();
    }

    /// A REDO MUST NOT REWRITE. The same job twice names the same file and
    /// does no second write — which is what lets an undone edit be redone
    /// without paying for it again, and what stops the cache growing by a
    /// file per keystroke.
    #[test]
    fn an_identical_job_reuses_the_cached_render() {
        let source = scratch("cache");
        write_ramp(&source, 1, 300);
        let job = Job {
            source: source.clone(),
            ops: vec![Op::Silence {
                from: 0,
                to: 10,
                channels: Channels::all(),
            }],
        };
        let first = render(&job).expect("first render");
        let stamp = first
            .path
            .metadata()
            .and_then(|meta| meta.modified())
            .expect("mtime");
        let second = render(&job).expect("second render");
        assert_eq!(first.path, second.path, "the same job is the same file");
        let again = second
            .path
            .metadata()
            .and_then(|meta| meta.modified())
            .expect("mtime");
        assert_eq!(stamp, again, "and it was not written a second time");

        // A DIFFERENT job is a different file, so one edit never lands on
        // top of another one's output.
        let other = Job {
            source: source.clone(),
            ops: vec![Op::Silence {
                from: 0,
                to: 11,
                channels: Channels::all(),
            }],
        };
        let other = render(&other).expect("other render");
        assert_ne!(first.path, other.path);

        std::fs::remove_file(&source).ok();
        std::fs::remove_file(&first.path).ok();
        std::fs::remove_file(&other.path).ok();
    }

    /// Editing the file underneath gets a different name rather than a
    /// stale render — the reverse cache's rule, restated here because a
    /// destructive edit that quietly reused yesterday's audio would be far
    /// worse than one that took a second to run again.
    #[test]
    fn a_changed_source_gets_a_different_path() {
        let source = scratch("mtime");
        write_ramp(&source, 1, 100);
        let job = Job {
            source: source.clone(),
            ops: vec![Op::Silence {
                from: 0,
                to: 10,
                channels: Channels::all(),
            }],
        };
        let first = output_path(&Job {
            source: source.canonicalize().expect("canonical"),
            ..job.clone()
        })
        .expect("path");

        write_ramp(&source, 1, 200);
        let second = output_path(&Job {
            source: source.canonicalize().expect("canonical"),
            ..job
        })
        .expect("path");
        assert_ne!(first, second);
        std::fs::remove_file(&source).ok();
    }

    #[test]
    fn ops_apply_in_order_over_one_buffer() {
        let job = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![
                Op::Silence {
                    from: 0,
                    to: 2,
                    channels: Channels::all(),
                },
                Op::Silence {
                    from: 3,
                    to: 4,
                    channels: Channels::all(),
                },
            ],
        };
        let (out, frames, _) = job.apply(vec![1.0; 5], 1, 48_000);
        assert_eq!(frames, 5);
        assert_eq!(out, vec![0.0, 0.0, 1.0, 0.0, 1.0]);

        // A range past the end is clamped rather than panicking: the
        // editor clamps too, and two clamps is the right number for
        // something a file's length can change underneath.
        let job = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Silence {
                from: 3,
                to: 900,
                channels: Channels::all(),
            }],
        };
        let (out, _, _) = job.apply(vec![1.0; 5], 1, 48_000);
        assert_eq!(out, vec![1.0, 1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_cache_directory_is_under_the_data_home() {
        // SAFETY: single-threaded test process; the variable is read by
        // `cache_dir` on this thread only.
        let directory = cache_dir().expect("a data directory");
        assert!(directory.ends_with("daw/renders"), "{directory:?}");
        assert!(directory.is_absolute());
        // NOT the temp directory: a destructive edit has to survive a
        // reboot, unlike a reversal, which can always be rebuilt.
        assert!(
            !directory.starts_with(std::env::temp_dir()),
            "renders must not live in temp: {directory:?}"
        );
    }

    fn run(ops: Vec<Op>, samples: Vec<f32>, channels: usize) -> Vec<f32> {
        Job {
            source: PathBuf::from("none.wav"),
            ops,
        }
        .apply(samples, channels, 48_000)
        .0
    }

    fn whole(from: u64, to: u64) -> (u64, u64, Channels) {
        (from, to, Channels::all())
    }

    #[test]
    fn gain_scales_only_the_range() {
        let (from, to, channels) = whole(2, 4);
        let out = run(
            vec![Op::Gain {
                from,
                to,
                channels,
                db: -6.0206,
            }],
            vec![1.0; 6],
            1,
        );
        assert_eq!(out[0], 1.0);
        assert!((out[2] - 0.5).abs() < 1e-4, "{}", out[2]);
        assert!((out[3] - 0.5).abs() < 1e-4);
        assert_eq!(out[4], 1.0);

        // Silence is a legal gain and does not become a negative one.
        let out = run(
            vec![Op::Gain {
                from: 0,
                to: 4,
                channels: Channels::all(),
                db: -200.0,
            }],
            vec![1.0; 4],
            1,
        );
        assert_eq!(out, vec![0.0; 4]);
    }

    /// Peak-normalizing puts the loudest sample exactly on the target,
    /// and leaves everything outside the range alone.
    #[test]
    fn peak_normalize_lands_on_the_target() {
        let mut samples = vec![0.1f32; 100];
        samples[50] = 0.25;
        samples[99] = 0.9; // outside the range below
        let out = run(
            vec![Op::Norm {
                from: 0,
                to: 60,
                channels: Channels::all(),
                target_db: -0.1,
                mode: Normalize::Peak,
                allow_clipping: false,
            }],
            samples,
            1,
        );
        let target = 10f32.powf(-0.1 / 20.0);
        assert!((out[50] - target).abs() < 1e-4, "{} vs {target}", out[50]);
        assert_eq!(out[99], 0.9, "outside the range, untouched");

        // SILENCE CANNOT BE NORMALIZED, and asking does not produce an
        // infinity or a NaN — it produces silence.
        let out = run(
            vec![Op::Norm {
                from: 0,
                to: 8,
                channels: Channels::all(),
                target_db: 0.0,
                mode: Normalize::Peak,
                allow_clipping: false,
            }],
            vec![0.0; 8],
            1,
        );
        assert!(out.iter().all(|value| *value == 0.0));
    }

    /// RMS normalizing routinely asks for a gain that would clip. Unless
    /// told otherwise it backs off to exactly full scale rather than
    /// writing a clipped file and saying nothing.
    #[test]
    fn rms_normalize_that_would_clip_backs_off() {
        // A square at 0.5: RMS and peak are both 0.5, so asking for
        // 0 dBFS RMS asks for a factor of two and peaks at 1.0 exactly.
        // Asking for +6 asks for four, which would clip.
        let samples: Vec<f32> = (0..64)
            .map(|index| if index % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let out = run(
            vec![Op::Norm {
                from: 0,
                to: 64,
                channels: Channels::all(),
                target_db: 6.0,
                mode: Normalize::Rms,
                allow_clipping: false,
            }],
            samples.clone(),
            1,
        );
        let peak = out.iter().fold(0.0f32, |peak, value| peak.max(value.abs()));
        assert!((peak - 1.0).abs() < 1e-5, "backed off to {peak}");

        // Told to allow it, it does what it was asked.
        let out = run(
            vec![Op::Norm {
                from: 0,
                to: 64,
                channels: Channels::all(),
                target_db: 6.0,
                mode: Normalize::Rms,
                allow_clipping: true,
            }],
            samples,
            1,
        );
        let peak = out.iter().fold(0.0f32, |peak, value| peak.max(value.abs()));
        assert!(peak > 1.9, "allowed to clip, it clipped: {peak}");
    }

    /// Normalize measures the masked channels TOGETHER, so a stereo image
    /// survives it.
    #[test]
    fn normalize_keeps_the_stereo_image() {
        // Left twice as loud as right, throughout.
        let samples: Vec<f32> = (0..32).flat_map(|_| [0.4f32, 0.2]).collect();
        let out = run(
            vec![Op::Norm {
                from: 0,
                to: 32,
                channels: Channels::all(),
                target_db: 0.0,
                mode: Normalize::Peak,
                allow_clipping: false,
            }],
            samples,
            2,
        );
        assert!((out[0] - 1.0).abs() < 1e-5, "left hit the target");
        assert!((out[1] - 0.5).abs() < 1e-5, "and right stayed half of it");
    }

    /// REVERSE BY FRAME. Reversing the interleaved buffer would swap the
    /// channels too, which sounds almost right, which is worse.
    #[test]
    fn reverse_turns_the_range_round_without_swapping_channels() {
        // Left counts up, right counts down.
        let samples: Vec<f32> = (0..4).flat_map(|i| [i as f32, -(i as f32)]).collect();
        let out = run(
            vec![Op::Reverse {
                from: 0,
                to: 4,
                channels: Channels::all(),
            }],
            samples,
            2,
        );
        assert_eq!(out, vec![3.0, -3.0, 2.0, -2.0, 1.0, -1.0, 0.0, -0.0]);

        // Only the range turns round.
        let out = run(
            vec![Op::Reverse {
                from: 1,
                to: 4,
                channels: Channels::all(),
            }],
            vec![0.0, 1.0, 2.0, 3.0, 4.0],
            1,
        );
        assert_eq!(out, vec![0.0, 3.0, 2.0, 1.0, 4.0]);
    }

    /// Inverting twice is the identity, to the bit — which is the test
    /// that says the operation is exactly a sign flip and not a multiply
    /// by something very close to minus one.
    #[test]
    fn invert_twice_is_the_original() {
        let samples: Vec<f32> = (0..50).map(|i| (i as f32 * 0.37).sin()).collect();
        let once = run(
            vec![Op::Invert {
                from: 0,
                to: 50,
                channels: Channels::all(),
            }],
            samples.clone(),
            1,
        );
        assert!(once.iter().zip(&samples).all(|(now, was)| *now == -*was));
        let twice = run(
            vec![Op::Invert {
                from: 0,
                to: 50,
                channels: Channels::all(),
            }],
            once,
            1,
        );
        assert_eq!(twice, samples);
    }

    /// DC is removed PER CHANNEL: one mean across a pair sitting at
    /// different offsets would leave both of them off zero.
    #[test]
    fn remove_dc_centres_each_channel_on_its_own() {
        let samples: Vec<f32> = (0..100).flat_map(|_| [0.3f32, -0.7]).collect();
        let out = run(
            vec![Op::RemoveDc {
                from: 0,
                to: 100,
                channels: Channels::all(),
            }],
            samples,
            2,
        );
        let mean = |channel: usize| out.iter().skip(channel).step_by(2).sum::<f32>() / 100.0;
        assert!(mean(0).abs() < 1e-6, "left mean {}", mean(0));
        assert!(mean(1).abs() < 1e-6, "right mean {}", mean(1));
    }

    /// A destructive fade and the clip's own fade are the SAME curve, so
    /// committing one cannot change how it sounds.
    #[test]
    fn a_destructive_fade_follows_the_shared_curve() {
        use crate::params::clip::Curve;
        for shape in [-0.8f32, -0.3, 0.0, 0.4, 0.9] {
            let curve = Curve::new(shape);
            let out = run(
                vec![Op::Fade {
                    from: 0,
                    to: 100,
                    channels: Channels::all(),
                    dir: FadeDir::In,
                    curve,
                }],
                vec![1.0; 100],
                1,
            );
            for (frame, value) in out.iter().enumerate() {
                let want = curve.at(frame as f32 / 100.0);
                assert!(
                    (value - want).abs() < 1e-6,
                    "shape {shape} frame {frame}: {value} not {want}"
                );
            }
            // A fade IN starts at silence; a fade OUT ends there.
            assert_eq!(out[0], 0.0);
            let out = run(
                vec![Op::Fade {
                    from: 0,
                    to: 100,
                    channels: Channels::all(),
                    dir: FadeDir::Out,
                    curve,
                }],
                vec![1.0; 100],
                1,
            );
            assert_eq!(out[0], 1.0, "a fade out starts at full level");
            for (frame, value) in out.iter().enumerate() {
                let want = curve.at(1.0 - frame as f32 / 100.0);
                assert!(
                    (value - want).abs() < 1e-6,
                    "out, shape {shape} frame {frame}: {value} not {want}"
                );
            }
            // Monotonic, at every shape. A strongly convex fade out is
            // still loud near its end — that is what the shape MEANS —
            // so the fact worth asserting is that it only ever falls.
            assert!(
                out.windows(2).all(|pair| pair[1] <= pair[0]),
                "shape {shape} did not fall throughout"
            );
            assert!(out[99] < out[0]);
        }
    }

    /// Two jobs that differ only in a float are two different files. If
    /// the key ignored the float, a −3 dB gain would hand back the file a
    /// −6 dB gain wrote.
    #[test]
    fn cut_shortens_the_file_by_the_selection() {
        // Stereo, so a cut that walked the channels apart would show.
        let samples: Vec<f32> = (0..6).flat_map(|i| [i as f32, -(i as f32)]).collect();
        let out = run(vec![Op::Cut { from: 2, to: 4 }], samples, 2);
        assert_eq!(out.len(), 8, "two frames gone, both channels");
        assert_eq!(out, vec![0.0, -0.0, 1.0, -1.0, 4.0, -4.0, 5.0, -5.0]);

        let op = Op::Cut { from: 2, to: 4 };
        assert_eq!(op.length_change(2), Some((2, -2)));
    }

    #[test]
    fn insert_silence_lengthens_the_file_at_the_point() {
        let out = run(
            vec![Op::InsertSilence { at: 2, frames: 3 }],
            vec![1.0, 2.0, 3.0, 4.0],
            1,
        );
        assert_eq!(out, vec![1.0, 2.0, 0.0, 0.0, 0.0, 3.0, 4.0]);
        assert_eq!(
            Op::InsertSilence { at: 2, frames: 3 }.length_change(1),
            Some((2, 3))
        );
    }

    /// A paste puts the material in whole FRAMES. A tail of a partial
    /// frame would put every later sample on the wrong channel — which
    /// does not sound like a bug, it sounds like a phasing problem.
    #[test]
    fn insert_pastes_whole_frames_only() {
        let material = Arc::new(vec![9.0f32, -9.0, 8.0, -8.0, 7.0]);
        let out = run(
            vec![Op::Insert {
                at: 1,
                material: material.clone(),
                material_rate: 48_000,
            }],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
        );
        assert_eq!(out, vec![0.0, 0.0, 9.0, -9.0, 8.0, -8.0, 1.0, 1.0]);
        assert_eq!(out.len() % 2, 0, "still whole frames");
        assert_eq!(
            Op::Insert {
                at: 1,
                material,
                material_rate: 48_000
            }
            .length_change(2),
            Some((1, 2))
        );
    }

    #[test]
    fn crop_keeps_only_the_selection() {
        let out = run(
            vec![Op::Crop { from: 1, to: 3 }],
            vec![0.0, 1.0, 2.0, 3.0],
            1,
        );
        assert_eq!(out, vec![1.0, 2.0]);
        // The change lands at frame ZERO: a crop removes what came before
        // the range as well, and the region has to slide by that much.
        assert_eq!(Op::Crop { from: 1, to: 3 }.length_change(1), Some((0, -1)));

        // An empty crop leaves the buffer alone rather than emptying it.
        let out = run(vec![Op::Crop { from: 2, to: 2 }], vec![0.0, 1.0, 2.0], 1);
        assert_eq!(out, vec![0.0, 1.0, 2.0]);
    }

    /// COPY THEN PASTE IS THE MATERIAL, sample for sample. Anything else
    /// and the clipboard is lying about what it holds.
    #[test]
    fn copy_paste_round_trips_the_material() {
        let source: Vec<f32> = (0..20).map(|i| (i as f32 * 0.31).sin()).collect();
        // "Copy" frames 5..12, then paste them at frame 0.
        let material = Arc::new(source[5..12].to_vec());
        let out = run(
            vec![Op::Insert {
                at: 0,
                material: material.clone(),
                material_rate: 48_000,
            }],
            source.clone(),
            1,
        );
        assert_eq!(&out[..7], &material[..]);
        assert_eq!(&out[7..], &source[..]);
    }

    /// A cut then a paste elsewhere is ONE job and one file — which is
    /// what makes a move one undo step rather than two.
    #[test]
    fn ops_that_change_length_compose_in_one_job() {
        let material = Arc::new(vec![5.0f32, 6.0]);
        let out = run(
            vec![
                Op::Cut { from: 0, to: 2 },
                Op::Insert {
                    at: 2,
                    material,
                    material_rate: 48_000,
                },
            ],
            vec![0.0, 1.0, 2.0, 3.0, 4.0],
            1,
        );
        // 0,1 removed leaves 2,3,4; inserting at frame 2 puts 5,6 before
        // the 4.
        assert_eq!(out, vec![2.0, 3.0, 5.0, 6.0, 4.0]);
    }

    /// Swapping is stereo-only, and a file that is not stereo is left
    /// exactly as it was rather than half-swapped.
    #[test]
    fn swap_channels_swaps_and_refuses_anything_but_stereo() {
        let out = run(vec![Op::SwapChannels], vec![1.0, 2.0, 3.0, 4.0], 2);
        assert_eq!(out, vec![2.0, 1.0, 4.0, 3.0]);

        let mono = vec![1.0, 2.0, 3.0];
        assert_eq!(run(vec![Op::SwapChannels], mono.clone(), 1), mono);
    }

    /// AVERAGE, NOT SUM. A correlated pair summed doubles and clips
    /// material that did not clip before — the classic broken mono fold.
    #[test]
    fn folding_to_mono_averages_rather_than_sums() {
        let job = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::ToMono(Mono::Average)],
        };
        // A correlated pair at full scale.
        let (out, frames, channels) = job.apply(vec![1.0, 1.0, -1.0, -1.0], 2, 48_000);
        assert_eq!(channels, 1, "it is a mono file now");
        assert_eq!(frames, 2);
        assert_eq!(out, vec![1.0, -1.0], "and it did not clip");

        // Left and right take a side outright.
        let (out, _, _) = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::ToMono(Mono::Left)],
        }
        .apply(vec![0.2, 0.8, 0.3, 0.9], 2, 48_000);
        assert_eq!(out, vec![0.2, 0.3]);
        let (out, _, _) = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::ToMono(Mono::Right)],
        }
        .apply(vec![0.2, 0.8, 0.3, 0.9], 2, 48_000);
        assert_eq!(out, vec![0.8, 0.9]);

        // A mono file is already mono, and folding it is a no-op rather
        // than an error.
        let (out, _, channels) = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::ToMono(Mono::Average)],
        }
        .apply(vec![0.5, 0.6], 1, 48_000);
        assert_eq!((out, channels), (vec![0.5, 0.6], 1));
    }

    #[test]
    fn taking_a_channel_gives_a_mono_file() {
        let job = |channel: usize| Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::TakeChannel(channel)],
        };
        let (out, frames, channels) = job(1).apply(vec![0.1, 0.2, 0.3, 0.4], 2, 48_000);
        assert_eq!((out, frames, channels), (vec![0.2, 0.4], 2, 1));

        // A channel that is not there leaves the file alone.
        let (out, _, channels) = job(5).apply(vec![0.1, 0.2], 2, 48_000);
        assert_eq!((out, channels), (vec![0.1, 0.2], 2));
    }

    /// A fold and a swap are different files even though neither carries
    /// a number for the key to see.
    #[test]
    fn channel_operations_key_apart() {
        let key = |op: Op| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            op.hash_into(&mut hasher);
            hasher.finish()
        };
        let keys = [
            key(Op::SwapChannels),
            key(Op::ToMono(Mono::Average)),
            key(Op::ToMono(Mono::Left)),
            key(Op::ToMono(Mono::Right)),
            key(Op::TakeChannel(0)),
            key(Op::TakeChannel(1)),
        ];
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b, "two channel operations share a cache key");
            }
        }
    }

    #[test]
    fn the_cache_key_sees_the_numbers() {
        let job = |db: f32| Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Gain {
                from: 0,
                to: 10,
                channels: Channels::all(),
                db,
            }],
        };
        let key = |job: &Job| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for op in &job.ops {
                op.hash_into(&mut hasher);
            }
            hasher.finish()
        };
        assert_ne!(key(&job(-3.0)), key(&job(-6.0)));
        assert_eq!(key(&job(-3.0)), key(&job(-3.0)));

        // And two DIFFERENT operations over the same range are different
        // keys, even where they carry no numbers at all.
        let silence = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Silence {
                from: 0,
                to: 10,
                channels: Channels::all(),
            }],
        };
        let invert = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Invert {
                from: 0,
                to: 10,
                channels: Channels::all(),
            }],
        };
        assert_ne!(key(&silence), key(&invert));
    }

    /// FIT PRINTS THE REPEATS. A looped clip runs through its region
    /// many times, and a flatten that only wrote the region once would
    /// change the sound of every looped clip in the project.
    #[test]
    fn fit_repeats_a_loop_and_pads_a_short_one() {
        let repeated = run(
            vec![Op::Fit {
                frames: 7,
                looped: true,
            }],
            vec![1.0, 2.0, 3.0],
            1,
        );
        assert_eq!(repeated, vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0]);

        // Not looped, the tail is silence — which is what the node plays
        // past the end of a short clip.
        let padded = run(
            vec![Op::Fit {
                frames: 6,
                looped: false,
            }],
            vec![1.0, 2.0, 3.0],
            1,
        );
        assert_eq!(padded, vec![1.0, 2.0, 3.0, 0.0, 0.0, 0.0]);

        // Longer than wanted, it truncates.
        let cut = run(
            vec![Op::Fit {
                frames: 2,
                looped: true,
            }],
            vec![1.0, 2.0, 3.0],
            1,
        );
        assert_eq!(cut, vec![1.0, 2.0]);

        // Whole FRAMES, in stereo.
        let stereo = run(
            vec![Op::Fit {
                frames: 3,
                looped: true,
            }],
            vec![1.0, -1.0, 2.0, -2.0],
            2,
        );
        assert_eq!(stereo, vec![1.0, -1.0, 2.0, -2.0, 1.0, -1.0]);
    }

    /// The committed envelope is the same ramp the node rides, so
    /// flattening cannot change a level.
    #[test]
    fn a_committed_envelope_is_the_same_ramp_the_node_rides() {
        let points = Arc::new(vec![(0u64, 1.0f32), (4, 0.0)]);
        let out = run(vec![Op::Envelope(points)], vec![1.0; 6], 1);
        assert_eq!(out[0], 1.0);
        assert!((out[1] - 0.75).abs() < 1e-6);
        assert!((out[2] - 0.5).abs() < 1e-6);
        assert_eq!(out[4], 0.0);
        // HELD FLAT past the last point, exactly as the node holds it.
        assert_eq!(out[5], 0.0);

        // An empty envelope changes nothing at all.
        let flat = run(vec![Op::Envelope(Arc::new(Vec::new()))], vec![0.5; 4], 1);
        assert_eq!(flat, vec![0.5; 4]);
    }

    /// AN OCTAVE UP HALVES THE LENGTH. Pitch and duration move together,
    /// which is what makes this varispeed and not a pitch shifter.
    #[test]
    fn an_octave_up_halves_the_length() {
        assert!((Op::transpose_ratio(12.0) - 2.0).abs() < 1e-5);
        assert!((Op::transpose_ratio(-12.0) - 0.5).abs() < 1e-5);
        assert_eq!(Op::transpose_ratio(0.0), 1.0);

        let samples: Vec<f32> = (0..4_800).map(|i| (i as f32 * 0.05).sin()).collect();
        let job = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Transpose { semitones: 12.0 }],
        };
        let (out, frames, channels) = job.apply(samples.clone(), 1, 48_000);
        assert_eq!(channels, 1);
        // Within a resampler's edge handling of the exact half.
        let want = 2_400i64;
        assert!(
            (frames as i64 - want).abs() < 64,
            "{frames} frames, wanted about {want}"
        );
        assert_eq!(out.len(), frames as usize);

        // Zero semitones does not resample at all — the buffer comes
        // back untouched rather than round-tripped through a filter.
        let (out, frames, _) = Job {
            source: PathBuf::from("none.wav"),
            ops: vec![Op::Transpose { semitones: 0.0 }],
        }
        .apply(samples.clone(), 1, 48_000);
        assert_eq!(out, samples);
        assert_eq!(frames, 4_800);
    }

    #[test]
    fn a_channel_mask_covers_what_it_says() {
        assert!(Channels::all().contains(0));
        assert!(Channels::all().contains(31));
        assert!(!Channels(0b10).contains(0));
        assert!(Channels(0b10).contains(1));
        assert!(Channels::default().is_empty());
        // Past the mask's width everything is covered rather than nothing:
        // a file with more channels than the mask can name must not be
        // silently half-edited.
        assert!(Channels(0b01).contains(40));
    }
}
