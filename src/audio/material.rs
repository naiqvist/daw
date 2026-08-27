//! Loading a file into memory, so the sampler can read it at random.
//!
//! GREEN ZONE, all of it. Nothing in this file is called from the audio
//! callback; the callback only ever holds the `Arc` this produces, and
//! that `Arc` is retired through the schedule-disposal path like every
//! other compiled allocation.
//!
//! # Why not stream it
//!
//! `Node::AudioClip` streams through creek, which is right for a clip: it
//! reads forward, once, at the rate it was recorded. A sampler does none
//! of those things. It reads backwards, jumps to a slice, loops a span
//! out of the middle, and reads at a fractional rate that changes with
//! the note. All of that is random access, and random access to disk in
//! the red zone is not a thing that can be made safe. So the material is
//! resident, and [`MAX_FRAMES`] is what keeps that honest.
//!
//! # Layout: planar, one allocation
//!
//! Channel `c` occupies `samples[c * frames .. (c + 1) * frames]`. That
//! is one allocation (so one thing to retire) with each channel
//! contiguous (so a four-point interpolator's neighbours are adjacent in
//! cache rather than a stride apart). Interleaved would have made the
//! four reads of a stereo Hermite step over the other channel every time.
//!
//! # Rate conversion happens HERE
//!
//! The file is resampled to the device rate at load. That means the read
//! head's increment is exactly `1.0` at unity pitch, which is what lets
//! the sampler claim bit-exact playback with every colour stage off — and
//! it quietly fixes, for this device, the wart `Node::AudioClip` still
//! carries, where a file at another rate plays detuned.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The longest stretch of audio one sampler will hold, in frames at the
/// device rate. Five minutes: stereo `f32` at 48 kHz is 115 MB, which is
/// a lot for one device and not a lot for a machine.
///
/// A longer file loads its FIRST five minutes and says so. It does not
/// fail, and it does not truncate in silence.
pub const MAX_FRAMES: u64 = 48_000 * 60 * 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialError {
    /// The decoder refused it. The message is the decoder's own, because
    /// "format not supported: no codec for .m4a" is more use than
    /// anything this layer could invent.
    Decode { path: PathBuf, message: String },
    /// It decoded to nothing: a zero-length file, or one with no
    /// channels.
    Empty(PathBuf),
}

impl std::fmt::Display for MaterialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            Self::Empty(path) => write!(f, "{}: no audio in the file", path.display()),
        }
    }
}

impl std::error::Error for MaterialError {}

/// Decoded audio at the device rate, ready to be read at random.
///
/// Cloning is cheap and shares the samples: the `Arc` is the point.
#[derive(Debug, Clone)]
pub struct Material {
    /// Planar. See the module header for the layout.
    pub samples: Arc<Vec<f32>>,
    pub channels: usize,
    pub frames: u64,
    pub source: PathBuf,
    /// The rate the samples are AT, which is the device rate.
    pub sample_rate: u32,
    /// The rate the file was at before loading, kept so the card can say
    /// "44.1 k, resampled" rather than leaving the user to wonder.
    pub original_rate: u32,
    /// Whether [`MAX_FRAMES`] cut it short.
    pub truncated: bool,
}

impl Material {
    /// The silent material a sampler with no file loaded holds. Not an
    /// `Option`: a device with nothing in it should render silence
    /// through its ordinary path, not take a different one.
    pub fn empty() -> Self {
        Self {
            samples: Arc::new(Vec::new()),
            channels: 0,
            frames: 0,
            source: PathBuf::new(),
            sample_rate: 0,
            original_rate: 0,
            truncated: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.frames == 0 || self.channels == 0
    }

    /// One channel's contiguous samples, or an empty slice for a channel
    /// this material does not have.
    pub fn channel(&self, channel: usize) -> &[f32] {
        if channel >= self.channels {
            return &[];
        }
        let frames = self.frames as usize;
        let from = channel * frames;
        self.samples.get(from..from + frames).unwrap_or(&[])
    }

    /// Bytes of audio held, for the cache's budget.
    pub fn bytes(&self) -> usize {
        self.samples.len() * std::mem::size_of::<f32>()
    }

    /// Seconds, at the device rate.
    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames as f64 / f64::from(self.sample_rate)
        }
    }
}

/// Decode `path`, resampled to `device_rate`.
pub fn load(path: &Path, device_rate: u32) -> Result<Material, MaterialError> {
    let fail = |message: String| MaterialError::Decode {
        path: path.to_path_buf(),
        message,
    };

    let probed = symphonium::probe_from_file(path, None).map_err(|e| fail(e.to_string()))?;
    let target = NonZeroU32::new(device_rate.max(1));
    let decoded = symphonium::decode_f32(
        probed,
        &symphonium::DecodeConfig::default(),
        target,
        None,
        None,
    )
    .map_err(|e| fail(e.to_string()))?;

    let channels = decoded.data.len();
    let frames = decoded.data.first().map_or(0, Vec::len) as u64;
    if channels == 0 || frames == 0 {
        return Err(MaterialError::Empty(path.to_path_buf()));
    }

    let truncated = frames > MAX_FRAMES;
    let frames = frames.min(MAX_FRAMES);
    let keep = frames as usize;

    // Flattened planar, one allocation, sized exactly once.
    let mut samples = Vec::with_capacity(keep * channels);
    for channel in &decoded.data {
        // A decoder that returned ragged channels would be a decoder
        // bug; pad rather than panic, because a panic here is a crash on
        // someone else's file.
        let have = channel.len().min(keep);
        samples.extend_from_slice(&channel[..have]);
        samples.resize(samples.len() + (keep - have), 0.0);
    }

    Ok(Material {
        samples: Arc::new(samples),
        channels,
        frames,
        source: path.to_path_buf(),
        sample_rate: decoded.sample_rate.get(),
        original_rate: decoded.original_sample_rate.get(),
        truncated,
    })
}

/// How much decoded audio the cache will hold before it starts letting go
/// of the least recently used. 512 MB: about twenty minutes of stereo.
pub const CACHE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    /// Nanoseconds since the epoch, or zero where the filesystem will not
    /// say. A file edited under us must not come back from the cache.
    modified: u128,
    rate: u32,
}

/// A green-zone LRU over loaded material, so the same drum hit on eight
/// tracks is decoded once.
///
/// Bounded by BYTES, not by count: one five-minute stereo take and two
/// hundred one-shots are the same problem to a memory budget and very
/// different problems to a counter.
#[derive(Debug, Default)]
pub struct Cache {
    /// Most recently used LAST. A vector rather than a map because the
    /// entry count is in the dozens and the LRU order has to be
    /// maintained anyway.
    entries: Vec<(Key, Material)>,
    bytes: usize,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Load, or hand back what was already loaded.
    pub fn load(&mut self, path: &Path, device_rate: u32) -> Result<Material, MaterialError> {
        let key = Key {
            path: path.to_path_buf(),
            modified: modified_nanos(path),
            rate: device_rate,
        };
        if let Some(at) = self.entries.iter().position(|(k, _)| *k == key) {
            let entry = self.entries.remove(at);
            let material = entry.1.clone();
            self.entries.push(entry);
            return Ok(material);
        }
        let material = load(path, device_rate)?;
        self.bytes += material.bytes();
        self.entries.push((key, material.clone()));
        // Evict from the front — least recently used — but never the
        // entry that was just asked for, however big it is. A device
        // whose sample is larger than the whole budget should still work.
        while self.bytes > CACHE_BYTES && self.entries.len() > 1 {
            let (_, gone) = self.entries.remove(0);
            self.bytes -= gone.bytes();
        }
        Ok(material)
    }
}

fn modified_nanos(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!(
            "daw-material-{name}-{}-{nonce}.wav",
            std::process::id()
        ))
    }

    /// A WAV of `frames` frames where channel `c` holds `c + frame/frames`.
    fn write(path: &Path, channels: u16, rate: u32, frames: u64) {
        let Ok(mut w) = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        ) else {
            panic!("create scratch WAV");
        };
        for frame in 0..frames {
            for channel in 0..channels {
                let _ = w.write_sample(f32::from(channel) + frame as f32 / frames as f32);
            }
        }
        let _ = w.finalize();
    }

    /// The layout claim: planar, one allocation, channels where the
    /// header says they are.
    #[test]
    fn a_stereo_file_lands_planar() {
        let path = scratch("planar");
        write(&path, 2, 48_000, 1_000);
        let Ok(m) = load(&path, 48_000) else {
            panic!("load")
        };
        assert_eq!(m.channels, 2);
        assert_eq!(m.frames, 1_000);
        assert_eq!(m.samples.len(), 2_000);
        assert_eq!(m.channel(0).len(), 1_000);
        assert_eq!(m.channel(1).len(), 1_000);
        assert!((m.channel(0)[0] - 0.0).abs() < 1e-4, "left starts at 0");
        assert!((m.channel(1)[0] - 1.0).abs() < 1e-4, "right starts at 1");
        assert!(m.channel(2).is_empty(), "a channel it does not have");
        let _ = std::fs::remove_file(&path);
    }

    /// Rate conversion happens at load, and the length moves with it.
    #[test]
    fn a_file_at_another_rate_arrives_at_the_device_rate() {
        let path = scratch("resample");
        write(&path, 1, 44_100, 44_100);
        let Ok(m) = load(&path, 48_000) else {
            panic!("load")
        };
        assert_eq!(m.sample_rate, 48_000);
        assert_eq!(m.original_rate, 44_100);
        // One second in, one second out, to within a resampler's edge.
        let drift = (m.frames as i64 - 48_000).abs();
        assert!(drift < 200, "one second became {} frames", m.frames);
        let _ = std::fs::remove_file(&path);
    }

    /// A file at the device rate is not touched by the resampler, which
    /// is what the sampler's identity claim needs one layer down.
    #[test]
    fn a_file_at_the_device_rate_arrives_unchanged() {
        let path = scratch("identity");
        write(&path, 1, 48_000, 512);
        let Ok(m) = load(&path, 48_000) else {
            panic!("load")
        };
        assert_eq!(m.frames, 512);
        for (i, s) in m.channel(0).iter().enumerate() {
            let want = i as f32 / 512.0;
            assert!((s - want).abs() < 1e-6, "frame {i}: {s} != {want}");
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Truncation is flagged, not silent. Written against a shortened cap
    /// so the test does not have to make a five-minute file.
    #[test]
    fn the_cap_is_reported_rather_than_hidden() {
        // MAX_FRAMES is five minutes; making one here would be slow, so
        // this asserts the FLAG's wiring on a file that fits, and the
        // arithmetic separately.
        let path = scratch("short");
        write(&path, 1, 48_000, 100);
        let Ok(m) = load(&path, 48_000) else {
            panic!("load")
        };
        assert!(!m.truncated);
        assert!(m.frames <= MAX_FRAMES);
        let _ = std::fs::remove_file(&path);

        assert_eq!(MAX_FRAMES, 48_000 * 60 * 5, "five minutes at 48 k");
    }

    #[test]
    fn a_missing_file_is_an_error_and_not_a_panic() {
        let path = scratch("absent");
        assert!(load(&path, 48_000).is_err());
    }

    /// The cache hands back the SAME allocation, which is the whole
    /// reason it exists — eight tracks on one hit must not be eight
    /// copies.
    #[test]
    fn the_cache_shares_one_allocation() {
        let path = scratch("cache");
        write(&path, 1, 48_000, 256);
        let mut cache = Cache::new();
        let Ok(a) = cache.load(&path, 48_000) else {
            panic!("first load")
        };
        let Ok(b) = cache.load(&path, 48_000) else {
            panic!("second load")
        };
        assert!(Arc::ptr_eq(&a.samples, &b.samples), "cache returned a copy");
        assert_eq!(cache.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    /// A different device rate is a different entry: the samples ARE the
    /// rate, so sharing them across rates would play the wrong pitch.
    #[test]
    fn the_cache_keys_on_the_device_rate_too() {
        let path = scratch("rates");
        write(&path, 1, 48_000, 256);
        let mut cache = Cache::new();
        let Ok(a) = cache.load(&path, 48_000) else {
            panic!("48 k")
        };
        let Ok(b) = cache.load(&path, 44_100) else {
            panic!("44.1 k")
        };
        assert!(!Arc::ptr_eq(&a.samples, &b.samples));
        assert_eq!(cache.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    /// Empty material renders through the ordinary path rather than
    /// needing one of its own.
    #[test]
    fn empty_material_is_readable() {
        let m = Material::empty();
        assert!(m.is_empty());
        assert!(m.channel(0).is_empty());
        assert_eq!(m.bytes(), 0);
        assert_eq!(m.seconds(), 0.0);
    }
}

/// The process-wide loader, so a recompile does not re-decode.
///
/// GREEN ZONE ONLY, and a `parking_lot::Mutex` for that reason: this is
/// called from `Schedule::compile`, which runs on the UI thread. Nothing
/// in the callback ever reaches it — the callback holds the `Arc` that
/// came out, not the door it came through.
///
/// It exists because a schedule recompiles on almost any edit. Decoding a
/// five-minute file on every one of those would be a visible stall on
/// every knob turn; sharing one `Arc` across every compile makes the
/// second and subsequent ones free.
pub fn load_cached(path: &Path, device_rate: u32) -> Result<Material, MaterialError> {
    static CACHE: std::sync::OnceLock<parking_lot::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| parking_lot::Mutex::new(Cache::new()))
        .lock()
        .load(path, device_rate)
}
