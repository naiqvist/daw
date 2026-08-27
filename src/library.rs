//! Machine-local sample-library catalog.
//!
//! This is deliberately green-zone code: it walks directories and sends
//! immutable snapshots to the UI over a channel. The audio engine never sees
//! a path, a filesystem call, or a catalog lock.

use crossbeam_channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// eframe storage key for machine-local library configuration.
pub const CONFIG_STORAGE_KEY: &str = "daw.library.config";
/// eframe storage key for the last usable catalog, shown while a fresh scan
/// runs after launch.
pub const CACHE_STORAGE_KEY: &str = "daw.library.cache";

/// Machine-local library roots and user tags. None of this is project data.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    pub user_library: Option<PathBuf>,
    pub sample_folders: Vec<PathBuf>,
    /// Canonical-path string -> user tags. Paths are the first stable identity;
    /// a content hash can replace this later when asset management lands.
    pub tags_by_path: BTreeMap<String, Vec<String>>,
}

impl LibraryConfig {
    /// Add a real directory, canonicalize it, and reject roots already known.
    pub fn add_sample_folder(&mut self, path: impl AsRef<Path>) -> Result<(), LibraryError> {
        let path = canonical_directory(path.as_ref())?;
        if self.roots().iter().any(|root| root.as_path() == path) {
            return Err(LibraryError::DuplicateRoot(path));
        }
        self.sample_folders.push(path);
        self.sample_folders.sort();
        Ok(())
    }

    /// Set the single User Library root. Replacing it is intentional.
    pub fn set_user_library(&mut self, path: impl AsRef<Path>) -> Result<(), LibraryError> {
        let path = canonical_directory(path.as_ref())?;
        if self.sample_folders.contains(&path) {
            return Err(LibraryError::DuplicateRoot(path));
        }
        self.user_library = Some(path);
        Ok(())
    }

    pub fn remove_sample_folder(&mut self, path: &Path) -> bool {
        let before = self.sample_folders.len();
        self.sample_folders.retain(|root| root != path);
        before != self.sample_folders.len()
    }

    fn roots(&self) -> Vec<&PathBuf> {
        self.user_library
            .iter()
            .chain(self.sample_folders.iter())
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("cannot access directory {path}: {source}")]
    Access {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("library root is already configured: {0}")]
    DuplicateRoot(PathBuf),
}

fn canonical_directory(path: &Path) -> Result<PathBuf, LibraryError> {
    if !path.is_dir() {
        return Err(LibraryError::NotDirectory(path.to_path_buf()));
    }
    path.canonicalize().map_err(|source| LibraryError::Access {
        path: path.to_path_buf(),
        source,
    })
}

/// A configured root represented in a browser snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryLocation {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub user_library: bool,
}

/// A searchable audio asset. All fields are green-zone catalog metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssetRecord {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub location_id: String,
    pub name: String,
    pub extension: String,
    pub bytes: u64,
    pub modified_unix_secs: Option<u64>,
    pub tags: Vec<String>,
}

/// A directory containing catalogued assets. Only directories on an asset's
/// path are retained, so empty filesystem folders never clutter the browser.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryFolder {
    pub location_id: String,
    pub relative_path: PathBuf,
    pub name: String,
}

/// Immutable catalog view. A UI frame reads one snapshot and never touches
/// the scanner or filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LibrarySnapshot {
    pub generation: u64,
    pub locations: Vec<LibraryLocation>,
    pub folders: Vec<LibraryFolder>,
    pub assets: Vec<AssetRecord>,
    pub warnings: Vec<String>,
}

/// A result sent by the background scanner.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub snapshot: LibrarySnapshot,
}

enum ScanCommand {
    Scan(LibraryConfig),
}

/// Background scanner owner. Dropping it disconnects the command channel and
/// lets the worker return once its current bounded directory walk is done.
pub struct LibraryService {
    commands: Sender<ScanCommand>,
    results: Receiver<ScanResult>,
}

/// Completed background WAV import. `path` is either the canonical original
/// or a cached 32-bit float WAV resampled to the requested device rate.
#[derive(Debug, Clone)]
pub struct ImportedWav {
    pub original_path: PathBuf,
    pub path: PathBuf,
    pub sample_rate: u32,
    pub frames: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum WavImportError {
    #[error("cannot access {path}: {source}")]
    Access {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot decode WAV {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("unsupported WAV layout: {0}")]
    Unsupported(String),
    #[error("resampler refused the WAV buffer")]
    ResamplerBuffer,
}

enum WavImportCommand {
    Import { path: PathBuf, target_rate: u32 },
    Reverse { path: PathBuf },
    Render(crate::render::Job),
    Extract { path: PathBuf, from: u64, to: u64 },
}

/// What the offline worker produced.
///
/// One channel for both, because they are the same kind of answer to the
/// same kind of ask, and they must arrive in the ORDER they were asked
/// for: a cut sends an extract and a render together, and the clipboard
/// must be filled from the file as it was before the cut repointed it.
#[derive(Debug, Clone, PartialEq)]
pub enum Offline {
    Rendered(crate::render::Rendered),
    Extracted(crate::render::Extract),
}

/// Green-zone worker for sample-rate conversion. The callback only ever sees
/// the resulting ordinary WAV through the existing creek stream.
pub struct WavImportService {
    commands: Sender<WavImportCommand>,
    results: Receiver<Result<ImportedWav, WavImportError>>,
    renders: Receiver<Result<Offline, WavImportError>>,
}

impl WavImportService {
    pub fn start() -> Self {
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (result_tx, results) = crossbeam_channel::unbounded();
        let (render_tx, renders) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("wav-import".to_owned())
            .spawn(move || {
                while let Ok(command) = command_rx.recv() {
                    // Renders share this thread with imports and
                    // reversals rather than getting one of their own.
                    // They are all the same work — decode a file, write a
                    // file — and a second thread would only put two heads
                    // on one disk.
                    let sent = match command {
                        WavImportCommand::Import { path, target_rate } => {
                            result_tx.send(import_wav(&path, target_rate)).is_ok()
                        }
                        WavImportCommand::Reverse { path } => {
                            result_tx.send(reverse_wav(&path)).is_ok()
                        }
                        WavImportCommand::Render(job) => render_tx
                            .send(crate::render::render(&job).map(Offline::Rendered))
                            .is_ok(),
                        WavImportCommand::Extract { path, from, to } => render_tx
                            .send(crate::render::extract(&path, from, to).map(Offline::Extracted))
                            .is_ok(),
                    };
                    if !sent {
                        return;
                    }
                }
            })
            .expect("WAV import thread must start");
        Self {
            commands,
            results,
            renders,
        }
    }

    pub fn import(&self, path: PathBuf, target_rate: u32) {
        let _ = self
            .commands
            .send(WavImportCommand::Import { path, target_rate });
    }

    /// Ask for `path`'s reversed twin. Idempotent: an already-built cache
    /// comes straight back.
    pub fn reverse(&self, path: PathBuf) {
        let _ = self.commands.send(WavImportCommand::Reverse { path });
    }

    /// Run a destructive edit offline. The answer names a NEW file; the
    /// caller repoints the clip at it, and the old file stays where it is
    /// so undo has something to go back to.
    pub fn render(&self, job: crate::render::Job) {
        let _ = self.commands.send(WavImportCommand::Render(job));
    }

    pub fn try_result(&self) -> Option<Result<ImportedWav, WavImportError>> {
        self.results.try_recv().ok()
    }

    /// Take a stretch of a file for the clipboard.
    pub fn extract(&self, path: PathBuf, from: u64, to: u64) {
        let _ = self
            .commands
            .send(WavImportCommand::Extract { path, from, to });
    }

    pub fn try_offline(&self) -> Option<Result<Offline, WavImportError>> {
        self.renders.try_recv().ok()
    }
}

/// Where `path`'s reversed twin lives.
///
/// DETERMINISTIC, and that is what makes the whole feature cheap: the
/// graph builder can name the file it wants without being told, and a
/// clip stores nothing but a `reversed` flag. Keyed by the source's
/// identity AND its size and mtime, so editing the file underneath
/// produces a different name rather than a stale reversal.
pub fn reverse_cache_path(path: &Path) -> Option<PathBuf> {
    let metadata = path.metadata().ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    let directory = std::env::temp_dir().join("daw-reversed");
    std::fs::create_dir_all(&directory).ok()?;
    Some(directory.join(format!("{:016x}-rev.wav", hasher.finish())))
}

/// Write `path` backwards into the cache, and hand back what plays.
///
/// BY FRAME, never by sample: reversing the interleaved buffer itself
/// would also swap left with right, which is a channel flip wearing a
/// reversal's clothes and sounds almost right, which is worse.
///
/// The whole FILE is reversed, not the clip's region, so trimming a
/// reversed clip afterwards needs no new cache — the region simply maps
/// onto the other end, which is arithmetic the graph builder does.
///
/// Public for deterministic offline tests; the app uses the worker.
pub fn reverse_wav(path: &Path) -> Result<ImportedWav, WavImportError> {
    let original_path = path
        .canonicalize()
        .map_err(|source| WavImportError::Access {
            path: path.to_path_buf(),
            source,
        })?;
    let cache = reverse_cache_path(&original_path).ok_or_else(|| WavImportError::Access {
        path: original_path.clone(),
        source: std::io::Error::other("no cache directory"),
    })?;

    let mut reader =
        hound::WavReader::open(&original_path).map_err(|source| WavImportError::Decode {
            path: original_path.clone(),
            source,
        })?;
    let spec = reader.spec();
    let frames = u64::from(reader.duration());
    if spec.channels == 0 || spec.sample_rate == 0 || frames == 0 {
        return Err(WavImportError::Unsupported(
            "zero channels, sample rate, or frames".to_owned(),
        ));
    }

    // Already built, and still the right shape? Then this is free.
    if let Ok(cached) = hound::WavReader::open(&cache)
        && cached.spec().channels == spec.channels
        && cached.spec().sample_rate == spec.sample_rate
        && u64::from(cached.duration()) == frames
    {
        return Ok(ImportedWav {
            original_path,
            path: cache,
            sample_rate: spec.sample_rate,
            frames,
        });
    }

    let channels = usize::from(spec.channels);
    let samples = read_wav_f32(&mut reader, &original_path)?;
    let output_spec = hound::WavSpec {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer =
        hound::WavWriter::create(&cache, output_spec).map_err(|source| WavImportError::Decode {
            path: cache.clone(),
            source,
        })?;
    for frame in samples.chunks_exact(channels).rev() {
        for sample in frame {
            writer
                .write_sample(*sample)
                .map_err(|source| WavImportError::Decode {
                    path: cache.clone(),
                    source,
                })?;
        }
    }
    writer.finalize().map_err(|source| WavImportError::Decode {
        path: cache.clone(),
        source,
    })?;
    Ok(ImportedWav {
        original_path,
        path: cache,
        sample_rate: spec.sample_rate,
        frames,
    })
}

/// Convert interleaved `f32` from one sample rate to another.
///
/// Lifted out of [`import_wav`] so pasting audio between clips at
/// different rates goes through the SAME resampler an import does. Two
/// resampling paths is two sets of filter characteristics, and a paste
/// that sounded subtly unlike the file it came from would be very hard to
/// account for.
///
/// A no-op when the rates match, which is the common case: the importer
/// has already brought every file in the project to the device rate.
pub(crate) fn resample_interleaved(
    samples: &[f32],
    channels: usize,
    from_rate: u32,
    to_rate: u32,
) -> Result<Vec<f32>, WavImportError> {
    let channels = channels.max(1);
    if from_rate == to_rate || from_rate == 0 || to_rate == 0 {
        return Ok(samples.to_vec());
    }
    let input_frames = (samples.len() / channels) as u64;
    if input_frames == 0 {
        return Ok(Vec::new());
    }
    let adapter = fixed_resample::audioadapter_buffers::direct::InterleavedSlice::new(
        samples,
        channels,
        input_frames as usize,
    )
    .map_err(|_| WavImportError::ResamplerBuffer)?;
    let mut resampler =
        fixed_resample::PacketResampler::<f32, fixed_resample::Interleaved<f32>>::new(
            channels,
            from_rate,
            to_rate,
            Default::default(),
        );
    let output_frames = resampler.out_alloc_frames(input_frames);
    let output_samples = output_frames
        .checked_mul(channels as u64)
        .and_then(|samples| usize::try_from(samples).ok())
        .ok_or_else(|| WavImportError::Unsupported("WAV is too large".to_owned()))?;
    let mut output = Vec::with_capacity(output_samples);
    resampler.process(
        &adapter,
        None,
        None,
        |packet, _| output.extend_from_slice(packet),
        Some(fixed_resample::LastPacketInfo {
            desired_output_frames: Some(output_frames),
        }),
        true,
    );
    Ok(output)
}

/// Decode and, when needed, resample a WAV into the machine-local temporary
/// cache. Public for deterministic offline tests; the app uses the worker.
pub fn import_wav(path: &Path, target_rate: u32) -> Result<ImportedWav, WavImportError> {
    let original_path = path
        .canonicalize()
        .map_err(|source| WavImportError::Access {
            path: path.to_path_buf(),
            source,
        })?;
    let mut reader =
        hound::WavReader::open(&original_path).map_err(|source| WavImportError::Decode {
            path: original_path.clone(),
            source,
        })?;
    let spec = reader.spec();
    let input_frames = u64::from(reader.duration());
    if spec.channels == 0 || spec.sample_rate == 0 || target_rate == 0 || input_frames == 0 {
        return Err(WavImportError::Unsupported(
            "zero channels, sample rate, or frames".to_owned(),
        ));
    }
    if spec.sample_rate == target_rate {
        return Ok(ImportedWav {
            original_path: original_path.clone(),
            path: original_path,
            sample_rate: target_rate,
            frames: input_frames,
        });
    }

    let cache = wav_cache_path(&original_path, target_rate)?;
    if let Ok(cached) = hound::WavReader::open(&cache)
        && cached.spec().sample_rate == target_rate
        && cached.spec().channels == spec.channels
        && cached.duration() > 0
    {
        return Ok(ImportedWav {
            original_path,
            path: cache,
            sample_rate: target_rate,
            frames: u64::from(cached.duration()),
        });
    }

    let channels = usize::from(spec.channels);
    let samples = read_wav_f32(&mut reader, &original_path)?;
    let output = resample_interleaved(&samples, channels, spec.sample_rate, target_rate)?;
    let output_frames = (output.len() / channels) as u64;

    let output_spec = hound::WavSpec {
        channels: spec.channels,
        sample_rate: target_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer =
        hound::WavWriter::create(&cache, output_spec).map_err(|source| WavImportError::Decode {
            path: cache.clone(),
            source,
        })?;
    for sample in output {
        writer
            .write_sample(sample)
            .map_err(|source| WavImportError::Decode {
                path: cache.clone(),
                source,
            })?;
    }
    writer.finalize().map_err(|source| WavImportError::Decode {
        path: cache.clone(),
        source,
    })?;
    Ok(ImportedWav {
        original_path,
        path: cache,
        sample_rate: target_rate,
        frames: output_frames,
    })
}

fn wav_cache_path(path: &Path, target_rate: u32) -> Result<PathBuf, WavImportError> {
    let metadata = path.metadata().map_err(|source| WavImportError::Access {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    target_rate.hash(&mut hasher);
    let directory = std::env::temp_dir().join("daw-resampled");
    std::fs::create_dir_all(&directory).map_err(|source| WavImportError::Access {
        path: directory.clone(),
        source,
    })?;
    Ok(directory.join(format!("{:016x}-{target_rate}.wav", hasher.finish())))
}

/// Decode a WAV's samples as interleaved `f32`, whatever its bit depth.
///
/// `pub(crate)` for `render`, which must decode exactly the way the
/// importer does: two decoders that scaled 24-bit audio differently would
/// make a destructive edit change the level of everything it touched.
pub(crate) fn read_wav_f32<R: std::io::Read>(
    reader: &mut hound::WavReader<R>,
    path: &Path,
) -> Result<Vec<f32>, WavImportError> {
    let spec = reader.spec();
    let decode = |source| WavImportError::Decode {
        path: path.to_path_buf(),
        source,
    };
    match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>(),
        hound::SampleFormat::Int if spec.bits_per_sample <= 8 => reader
            .samples::<i8>()
            .map(|sample| sample.map(|sample| f32::from(sample) / f32::from(i8::MAX)))
            .collect(),
        hound::SampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| f32::from(sample) / f32::from(i16::MAX)))
            .collect(),
        hound::SampleFormat::Int => {
            let peak = ((1u64 << (spec.bits_per_sample - 1)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|sample| sample as f32 / peak))
                .collect()
        }
    }
    .map_err(decode)
}

impl LibraryService {
    pub fn start(config: LibraryConfig) -> Self {
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (result_tx, results) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("library-scan".to_owned())
            .spawn(move || scanner_loop(command_rx, result_tx))
            .expect("library scanner thread must start");
        let service = Self { commands, results };
        service.rescan(config);
        service
    }

    /// Green zone: queue a complete rescan. If the worker is busy, the newest
    /// request follows it; UI and audio threads never wait for the walk.
    pub fn rescan(&self, config: LibraryConfig) {
        let _ = self.commands.send(ScanCommand::Scan(config));
    }

    /// Drain all completed scans and return only the newest snapshot.
    pub fn newest_snapshot(&self) -> Option<LibrarySnapshot> {
        self.results.try_iter().map(|result| result.snapshot).last()
    }
}

fn scanner_loop(commands: Receiver<ScanCommand>, results: Sender<ScanResult>) {
    let mut generation = 0u64;
    while let Ok(ScanCommand::Scan(config)) = commands.recv() {
        generation = generation.wrapping_add(1);
        let snapshot = scan(&config, generation);
        if results.send(ScanResult { snapshot }).is_err() {
            return;
        }
    }
}

/// Scan configured roots. Public for deterministic tests and offline tools;
/// callers in the app use [`LibraryService`] instead.
pub fn scan(config: &LibraryConfig, generation: u64) -> LibrarySnapshot {
    let mut snapshot = LibrarySnapshot {
        generation,
        ..Default::default()
    };
    let mut seen_assets = HashSet::new();
    let mut seen_roots = HashSet::new();
    let mut seen_folders = HashSet::new();

    let mut roots = Vec::new();
    if let Some(root) = &config.user_library {
        roots.push((root, true));
    }
    roots.extend(config.sample_folders.iter().map(|root| (root, false)));

    for (root, is_user_library) in roots {
        let Ok(root) = root.canonicalize() else {
            snapshot
                .warnings
                .push(format!("unavailable library folder: {}", root.display()));
            continue;
        };
        if !root.is_dir() {
            snapshot
                .warnings
                .push(format!("not a library folder: {}", root.display()));
            continue;
        }
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        let id = location_id(&root);
        snapshot.locations.push(LibraryLocation {
            id: id.clone(),
            label: root
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Library")
                .to_owned(),
            path: root.clone(),
            user_library: is_user_library,
        });
        scan_root(
            &root,
            &id,
            config,
            &mut seen_assets,
            &mut seen_folders,
            &mut snapshot,
        );
    }

    snapshot.locations.sort_by(|a, b| a.label.cmp(&b.label));
    snapshot.folders.sort_by(|a, b| {
        a.location_id
            .cmp(&b.location_id)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    snapshot.assets.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    snapshot
}

fn scan_root(
    root: &Path,
    location_id: &str,
    config: &LibraryConfig,
    seen_assets: &mut HashSet<PathBuf>,
    seen_folders: &mut HashSet<(String, PathBuf)>,
    snapshot: &mut LibrarySnapshot,
) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                snapshot
                    .warnings
                    .push(format!("cannot scan {}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    snapshot
                        .warnings
                        .push(format!("cannot inspect {}: {error}", path.display()));
                    continue;
                }
            };
            // Never traverse links: a sample folder must not unexpectedly
            // escape its root or loop back into itself.
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            if !file_type.is_file() || !is_audio_file(&path) {
                continue;
            }
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if !seen_assets.insert(path.clone()) {
                continue;
            }
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    snapshot
                        .warnings
                        .push(format!("cannot read {}: {error}", path.display()));
                    continue;
                }
            };
            let relative_path = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| path.clone());
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("unnamed")
                .to_owned();
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let key = path.to_string_lossy().to_string();
            let mut tags = inferred_tags(&relative_path, &extension);
            tags.extend(config.tags_by_path.get(&key).cloned().unwrap_or_default());
            tags.sort();
            tags.dedup();
            collect_asset_folders(
                location_id,
                &relative_path,
                seen_folders,
                &mut snapshot.folders,
            );
            snapshot.assets.push(AssetRecord {
                path,
                relative_path,
                location_id: location_id.to_owned(),
                name,
                extension,
                bytes: metadata.len(),
                modified_unix_secs: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs()),
                tags,
            });
        }
    }
}

fn collect_asset_folders(
    location_id: &str,
    relative_path: &Path,
    seen_folders: &mut HashSet<(String, PathBuf)>,
    folders: &mut Vec<LibraryFolder>,
) {
    let Some(parent) = relative_path.parent() else {
        return;
    };
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        let key = (location_id.to_owned(), current.clone());
        if !seen_folders.insert(key) {
            continue;
        }
        let name = component.as_os_str().to_string_lossy().to_string();
        folders.push(LibraryFolder {
            location_id: location_id.to_owned(),
            relative_path: current.clone(),
            name,
        });
    }
}

/// Baseline tags make `#tag` search useful even before custom tagging lands:
/// directory names describe most sample libraries well enough to be a useful
/// first catalog vocabulary. Persisted user tags are merged on top.
fn inferred_tags(relative_path: &Path, extension: &str) -> Vec<String> {
    let mut tags = relative_path
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components())
        .filter_map(|component| component.as_os_str().to_str())
        .flat_map(|component| component.split(|character: char| !character.is_alphanumeric()))
        .filter(|tag| !tag.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if !extension.is_empty() {
        tags.push(extension.to_ascii_lowercase());
    }
    tags
}

fn location_id(root: &Path) -> String {
    root.to_string_lossy().to_string()
}

fn is_audio_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("wav" | "wave" | "aif" | "aiff" | "flac" | "ogg" | "mp3" | "m4a" | "aac" | "opus")
    )
}

/// Search a snapshot without filesystem access. Plain terms are ANDed across
/// name and root-relative path; `#tag` terms are ANDed against tags.
pub fn query<'a>(
    snapshot: &'a LibrarySnapshot,
    location_id: Option<&str>,
    text: &str,
) -> Vec<&'a AssetRecord> {
    query_at(snapshot, location_id, None, text)
}

/// Like [`query`], narrowed to a root-relative folder when one is selected.
pub fn query_at<'a>(
    snapshot: &'a LibrarySnapshot,
    location_id: Option<&str>,
    folder: Option<&Path>,
    text: &str,
) -> Vec<&'a AssetRecord> {
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .collect();
    snapshot
        .assets
        .iter()
        .filter(|asset| location_id.is_none_or(|id| asset.location_id == id))
        .filter(|asset| folder.is_none_or(|folder| asset.relative_path.starts_with(folder)))
        .filter(|asset| {
            let haystack = format!(
                "{} {}",
                asset.name.to_ascii_lowercase(),
                asset.relative_path.to_string_lossy().to_ascii_lowercase()
            );
            terms.iter().all(|term| {
                if let Some(tag) = term.strip_prefix('#') {
                    asset
                        .tags
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(tag))
                } else {
                    haystack.contains(term)
                }
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("daw-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn configured_roots_are_canonical_unique_and_machine_local() {
        let root = temp_root("config");
        let mut config = LibraryConfig::default();
        config.set_user_library(&root).unwrap();
        assert!(matches!(
            config.add_sample_folder(&root),
            Err(LibraryError::DuplicateRoot(_))
        ));
        let text = ron::ser::to_string(&config).unwrap();
        let round: LibraryConfig = ron::from_str(&text).unwrap();
        assert_eq!(config, round);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_and_query_dedupe_filter_and_tag_assets() {
        let root = temp_root("scan");
        let nested = root.join("Drums");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("Electric Bass.wav"), []).unwrap();
        std::fs::write(nested.join("Kick.flac"), []).unwrap();
        std::fs::write(nested.join("ignore.txt"), []).unwrap();
        let root = root.canonicalize().unwrap();
        let asset = root.join("Drums/Electric Bass.wav");
        let mut config = LibraryConfig {
            user_library: Some(root.clone()),
            ..Default::default()
        };
        config.sample_folders.push(root.clone());
        config.tags_by_path.insert(
            asset.to_string_lossy().to_string(),
            vec!["Low".to_owned(), "Bass".to_owned()],
        );
        let snapshot = scan(&config, 7);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.assets.len(), 2, "overlapping roots dedupe");
        assert_eq!(snapshot.folders.len(), 1);
        assert_eq!(query(&snapshot, None, "electric bass").len(), 1);
        assert_eq!(query(&snapshot, None, "#low #bass").len(), 1);
        assert_eq!(query(&snapshot, None, "electric #drums").len(), 1);
        assert_eq!(
            query_at(&snapshot, None, Some(Path::new("Drums")), "kick").len(),
            1
        );
        let location = snapshot.locations[0].id.as_str();
        assert_eq!(query(&snapshot, Some(location), "kick").len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_roots_become_warnings_not_failures() {
        let config = LibraryConfig {
            sample_folders: vec![PathBuf::from("/definitely/not/a/daw-library")],
            ..Default::default()
        };
        let snapshot = scan(&config, 1);
        assert!(snapshot.assets.is_empty());
        assert_eq!(snapshot.warnings.len(), 1);
    }

    /// Reversal is BY FRAME, and the test proves it by reversing a file
    /// whose two channels differ: reversing the interleaved buffer itself
    /// would also swap left with right, which sounds almost right and is
    /// therefore the worst way to be wrong.
    #[test]
    fn reversing_a_wav_flips_time_and_not_the_channels() {
        let root = temp_root("reverse");
        let source = root.join("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        // Left counts up, right counts down: every frame is identifiable
        // and the two channels are never the same value.
        let frames = 64usize;
        for frame in 0..frames {
            writer.write_sample(frame as f32).unwrap();
            writer.write_sample(-(frame as f32)).unwrap();
        }
        writer.finalize().unwrap();

        let reversed = reverse_wav(&source).unwrap();
        assert_eq!(reversed.frames, frames as u64);
        assert_eq!(reversed.sample_rate, 48_000);
        assert_ne!(reversed.path, reversed.original_path, "it is a new file");

        let mut back = hound::WavReader::open(&reversed.path).unwrap();
        let samples: Vec<f32> = back.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples.len(), frames * 2);
        for (i, frame) in samples.chunks_exact(2).enumerate() {
            let want = (frames - 1 - i) as f32;
            assert_eq!(frame[0], want, "left, frame {i}");
            assert_eq!(frame[1], -want, "right stayed right, frame {i}");
        }

        // Asking again is free and answers with the same file.
        let again = reverse_wav(&source).unwrap();
        assert_eq!(again.path, reversed.path);
        std::fs::remove_file(&reversed.path).ok();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wav_import_resamples_to_a_reusable_float_cache() {
        let root = temp_root("resample");
        let source = root.join("source.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        for frame in 0..441 {
            let sample = ((frame as f32 / 441.0 * std::f32::consts::TAU).sin()
                * i16::MAX as f32
                * 0.25) as i16;
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();

        let imported = import_wav(&source, 48_000).unwrap();
        assert_ne!(imported.path, imported.original_path);
        assert_eq!(imported.sample_rate, 48_000);
        assert_eq!(imported.frames, 480);
        let cached = hound::WavReader::open(&imported.path).unwrap();
        assert_eq!(cached.spec().sample_rate, 48_000);
        assert_eq!(cached.spec().sample_format, hound::SampleFormat::Float);
        assert_eq!(cached.duration(), 480);

        let reused = import_wav(&source, 48_000).unwrap();
        assert_eq!(reused.path, imported.path);
        std::fs::remove_file(imported.path).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
