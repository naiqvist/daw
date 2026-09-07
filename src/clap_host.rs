//! CLAP discovery and the host-side plugin lifecycle.
//!
//! This module deliberately separates three trust/thread domains:
//!
//! - [`discover_candidates`] only walks the filesystem. It never loads code and
//!   is safe to run anywhere outside the audio callback.
//! - [`ClapScanner`] loads code only from [`TrustedPluginPath`] values and can be
//!   moved to a background [`ScanJob`]. Native plugin loading is inherently
//!   unsafe; the one unsafe operation is constructing that trust token.
//! - [`PluginControl`] stays on its creating control thread while the `Send`
//!   [`ClapProcessor`] can be handed to the audio thread. Starting, resetting,
//!   stopping and processing do not allocate, lock, log, or perform I/O in host
//!   code. The processor must be stopped on the audio thread and returned for
//!   deactivation and destruction on the control thread.
//!
//! CLAP state serialization is intentionally not forged here. `clack-host`
//! 0.1.1 exposes the generic extension mechanism, while the safe state stream
//! wrappers live in the separate `clack-extensions` crate. The integration seam
//! is [`PluginControl`]: query and call the state extension there, on the green
//! control thread, and persist an opaque [`PluginStateBlob`]. Until that
//! dependency is added and audited, callers can store state blobs but cannot
//! ask this module to capture or apply them.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use ::clack_host::entry::{PluginEntry, PluginEntryError};
use ::clack_host::events::event_types::TransportEvent;
use ::clack_host::events::io::{InputEvents, OutputEvents};
use ::clack_host::host::{HostHandlers, HostInfo, SharedHandler};
use ::clack_host::plugin::{PluginDescriptor, PluginInstance, PluginInstanceError};
pub use ::clack_host::process::ProcessStatus as ClapProcessStatus;
use ::clack_host::process::audio_buffers::{
    AudioPortBuffer, AudioPortBufferType, AudioPorts, InputAudioBuffers, InputChannel,
    OutputAudioBuffers,
};
use ::clack_host::process::{PluginAudioConfiguration, PluginAudioProcessor};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, NulError};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

const REQUEST_RESTART: u8 = 1 << 0;
const REQUEST_PROCESS: u8 = 1 << 1;
const REQUEST_CALLBACK: u8 = 1 << 2;

/// Bounded filesystem and descriptor limits for a scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanOptions {
    /// Descend into ordinary directories.
    pub recursive: bool,
    /// Maximum directory depth below each explicit root.
    pub max_depth: usize,
    /// Maximum number of directory entries inspected in one discovery pass.
    pub max_visited_entries: usize,
    /// Maximum number of candidate libraries returned.
    pub max_candidates: usize,
    /// Maximum number of descriptors accepted from one library.
    pub max_descriptors_per_library: u32,
    /// Maximum number of feature tags copied from one descriptor.
    pub max_features_per_plugin: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            max_depth: 8,
            max_visited_entries: 65_536,
            max_candidates: 4_096,
            max_descriptors_per_library: 4_096,
            max_features_per_plugin: 256,
        }
    }
}

/// Machine-readable class for a scanner diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiagnosticCode {
    PathNotFound,
    MetadataFailed,
    ReadDirectoryFailed,
    SymlinkSkipped,
    DepthLimitReached,
    EntryLimitReached,
    CandidateLimitReached,
    CanonicalizeFailed,
    DuplicateCandidate,
    LibraryLoadFailed,
    MissingPluginFactory,
    DescriptorLimitReached,
    MissingDescriptor,
    InvalidPluginId,
    InvalidPluginName,
    InvalidOptionalText,
    FeatureLimitReached,
    DuplicatePluginId,
}

/// Importance of a scanner diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// A durable diagnostic suitable for a plugin manager UI or scan log.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub path: PathBuf,
    pub descriptor_index: Option<u32>,
    pub detail: String,
}

impl ScanDiagnostic {
    fn new(
        severity: DiagnosticSeverity,
        code: DiagnosticCode,
        path: impl Into<PathBuf>,
        descriptor_index: Option<u32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            path: path.into(),
            descriptor_index,
            detail: detail.into(),
        }
    }
}

/// A canonical candidate found without loading native code.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ClapCandidate {
    path: PathBuf,
}

impl ClapCandidate {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Result of the filesystem-only discovery pass.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiscoveryReport {
    pub candidates: Vec<ClapCandidate>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub visited_entries: usize,
}

/// Find `.clap` files/bundles below explicit files or directories.
///
/// Explicit files are accepted regardless of extension, which supports CLAP
/// libraries distributed as `.so`, `.dll`, or a user-renamed file. Files found
/// while walking a directory must end in `.clap`. Symlinks encountered during
/// traversal are not followed, preventing directory cycles. Returned paths are
/// canonical, sorted, and unique.
pub fn discover_candidates<I, P>(roots: I, options: &ScanOptions) -> DiscoveryReport
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut report = DiscoveryReport::default();
    let mut seen = HashSet::new();
    let mut candidate_limit_reported = false;
    let mut entry_limit_reported = false;

    for root in roots {
        let root = root.as_ref();
        let metadata = match fs::metadata(root) {
            Ok(metadata) => metadata,
            Err(error) => {
                let code = if error.kind() == io::ErrorKind::NotFound {
                    DiagnosticCode::PathNotFound
                } else {
                    DiagnosticCode::MetadataFailed
                };
                report.diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Error,
                    code,
                    root,
                    None,
                    error.to_string(),
                ));
                continue;
            }
        };

        if metadata.is_file() || is_clap_bundle(root) {
            add_candidate(
                root,
                options,
                &mut report,
                &mut seen,
                &mut candidate_limit_reported,
            );
            continue;
        }

        if !metadata.is_dir() {
            report.diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticCode::MetadataFailed,
                root,
                None,
                "path is neither a regular file nor a directory",
            ));
            continue;
        }

        let mut pending = vec![(root.to_path_buf(), 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    report.diagnostics.push(ScanDiagnostic::new(
                        DiagnosticSeverity::Error,
                        DiagnosticCode::ReadDirectoryFailed,
                        &directory,
                        None,
                        error.to_string(),
                    ));
                    continue;
                }
            };

            for entry in entries {
                if report.visited_entries >= options.max_visited_entries {
                    if !entry_limit_reported {
                        report.diagnostics.push(ScanDiagnostic::new(
                            DiagnosticSeverity::Warning,
                            DiagnosticCode::EntryLimitReached,
                            root,
                            None,
                            format!(
                                "stopped after {} directory entries",
                                options.max_visited_entries
                            ),
                        ));
                        entry_limit_reported = true;
                    }
                    pending.clear();
                    break;
                }
                report.visited_entries += 1;

                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        report.diagnostics.push(ScanDiagnostic::new(
                            DiagnosticSeverity::Warning,
                            DiagnosticCode::MetadataFailed,
                            &directory,
                            None,
                            error.to_string(),
                        ));
                        continue;
                    }
                };
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        report.diagnostics.push(ScanDiagnostic::new(
                            DiagnosticSeverity::Warning,
                            DiagnosticCode::MetadataFailed,
                            &path,
                            None,
                            error.to_string(),
                        ));
                        continue;
                    }
                };

                if file_type.is_symlink() {
                    report.diagnostics.push(ScanDiagnostic::new(
                        DiagnosticSeverity::Info,
                        DiagnosticCode::SymlinkSkipped,
                        &path,
                        None,
                        "symlink was not followed during recursive discovery",
                    ));
                } else if (file_type.is_file() || file_type.is_dir()) && is_clap_bundle(&path) {
                    add_candidate(
                        &path,
                        options,
                        &mut report,
                        &mut seen,
                        &mut candidate_limit_reported,
                    );
                } else if file_type.is_dir() && options.recursive {
                    if depth < options.max_depth {
                        pending.push((path, depth + 1));
                    } else {
                        report.diagnostics.push(ScanDiagnostic::new(
                            DiagnosticSeverity::Info,
                            DiagnosticCode::DepthLimitReached,
                            &path,
                            None,
                            format!("maximum discovery depth {} reached", options.max_depth),
                        ));
                    }
                }
            }
        }
    }

    report.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    report
}

fn is_clap_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("clap"))
}

fn add_candidate(
    path: &Path,
    options: &ScanOptions,
    report: &mut DiscoveryReport,
    seen: &mut HashSet<PathBuf>,
    candidate_limit_reported: &mut bool,
) {
    if report.candidates.len() >= options.max_candidates {
        if !*candidate_limit_reported {
            report.diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticCode::CandidateLimitReached,
                path,
                None,
                format!("stopped after {} CLAP candidates", options.max_candidates),
            ));
            *candidate_limit_reported = true;
        }
        return;
    }

    let canonical = match fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(error) => {
            report.diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticCode::CanonicalizeFailed,
                path,
                None,
                error.to_string(),
            ));
            return;
        }
    };
    if !seen.insert(canonical.clone()) {
        report.diagnostics.push(ScanDiagnostic::new(
            DiagnosticSeverity::Info,
            DiagnosticCode::DuplicateCandidate,
            canonical,
            None,
            "candidate was already discovered",
        ));
        return;
    }
    report.candidates.push(ClapCandidate { path: canonical });
}

/// A native library path approved by the caller's plugin trust policy.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TrustedPluginPath {
    path: PathBuf,
}

impl TrustedPluginPath {
    /// Mark a discovered native library as trusted to load into this process.
    ///
    /// # Safety
    ///
    /// Loading native code can execute constructors and a malformed CLAP entry
    /// can violate Rust's safety assumptions. The caller must establish trust
    /// through installation provenance, signature/hash policy, or explicit user
    /// approval. Descriptor validation after loading cannot establish this.
    pub unsafe fn from_candidate(candidate: &ClapCandidate) -> Self {
        Self {
            path: candidate.path.clone(),
        }
    }

    /// Mark an explicit native library path as trusted.
    ///
    /// # Safety
    ///
    /// This carries the same native-code trust requirement as
    /// [`Self::from_candidate`].
    pub unsafe fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            path: fs::canonicalize(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A validated, UTF-8 CLAP plugin identifier.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(id: impl Into<String>) -> Result<Self, PluginIdError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(PluginIdError::Empty);
        }
        if id.as_bytes().contains(&0) {
            return Err(PluginIdError::InteriorNul);
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn as_cstring(&self) -> Result<CString, NulError> {
        CString::new(self.0.as_bytes())
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PluginIdError {
    #[error("CLAP plugin ID is empty")]
    Empty,
    #[error("CLAP plugin ID contains a null byte")]
    InteriorNul,
}

/// Exact restore key for one plugin descriptor.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PluginKey {
    pub library_path: PathBuf,
    pub plugin_id: PluginId,
}

/// Descriptor fields copied out of a native library before it is unloaded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginDescriptorInfo {
    pub key: PluginKey,
    pub factory_index: u32,
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub manual_url: Option<String>,
    pub support_url: Option<String>,
    pub features: Vec<String>,
}

/// Opaque bytes reserved for the audited CLAP state-extension seam.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginStateBlob {
    pub plugin_id: PluginId,
    pub bytes: Vec<u8>,
}

impl PluginStateBlob {
    pub fn belongs_to(&self, key: &PluginKey) -> bool {
        self.plugin_id == key.plugin_id
    }
}

/// Durable identity and last-known host metadata for one CLAP device.
///
/// The path and descriptor ID are both required: descriptor IDs are stable
/// inside CLAP, while one project must still resolve the exact library the
/// user trusted. `reported_latency_samples` is the last value read from the
/// CLAP latency extension by a host version that supports it. This crate
/// currently cannot query that extension safely (see the module docs), so new
/// records begin at zero and never invent a value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClapDeviceState {
    pub key: PluginKey,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PluginStateBlob>,
    #[serde(default)]
    pub reported_latency_samples: u32,
}

impl ClapDeviceState {
    pub fn from_descriptor(descriptor: &PluginDescriptorInfo) -> Self {
        Self {
            key: descriptor.key.clone(),
            display_name: descriptor.name.clone(),
            state: None,
            reported_latency_samples: 0,
        }
    }

    /// Whether an opaque state blob can belong to this descriptor.
    ///
    /// Deserialization is deliberately permissive so a damaged project can be
    /// opened and repaired. Activation is strict and refuses a mismatched blob.
    pub fn state_matches_key(&self) -> bool {
        self.state
            .as_ref()
            .is_none_or(|state| state.belongs_to(&self.key))
    }

    pub fn latency_samples(&self) -> usize {
        self.reported_latency_samples as usize
    }
}

/// Persisted resolution state for an external device.
///
/// `Missing` keeps the complete original identity and opaque state instead of
/// replacing the device with a generic placeholder. A later scan can resolve
/// the same record back to `Clap` without losing sound-design data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum PluginDeviceModel {
    Clap {
        plugin: ClapDeviceState,
    },
    Missing {
        plugin: ClapDeviceState,
        reason: String,
    },
}

impl PluginDeviceModel {
    pub fn plugin(&self) -> &ClapDeviceState {
        match self {
            Self::Clap { plugin } | Self::Missing { plugin, .. } => plugin,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing { .. })
    }

    pub fn into_missing(self, reason: impl Into<String>) -> Self {
        let plugin = match self {
            Self::Clap { plugin } | Self::Missing { plugin, .. } => plugin,
        };
        Self::Missing {
            plugin,
            reason: reason.into(),
        }
    }
}

/// Complete result of loading all trusted candidates in one scan.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanReport {
    pub plugins: Vec<PluginDescriptorInfo>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub libraries_scanned: usize,
    pub libraries_from_cache: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
    is_directory: bool,
}

impl FileFingerprint {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            is_directory: metadata.is_dir(),
        })
    }
}

#[derive(Clone, Debug)]
struct LibraryScan {
    plugins: Vec<PluginDescriptorInfo>,
    diagnostics: Vec<ScanDiagnostic>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    fingerprint: FileFingerprint,
    scan: LibraryScan,
}

/// In-memory scan cache invalidated by canonical path metadata.
#[derive(Debug, Default)]
pub struct ScanCache {
    entries: HashMap<PathBuf, CacheEntry>,
}

impl ScanCache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.entries.remove(path);
    }

    fn get(&self, path: &Path, fingerprint: &FileFingerprint) -> Option<LibraryScan> {
        self.entries
            .get(path)
            .filter(|entry| entry.fingerprint == *fingerprint)
            .map(|entry| entry.scan.clone())
    }

    fn insert(&mut self, path: PathBuf, fingerprint: FileFingerprint, scan: LibraryScan) {
        self.entries.insert(path, CacheEntry { fingerprint, scan });
    }
}

/// Stateful descriptor scanner with metadata-based caching.
#[derive(Debug, Default)]
pub struct ClapScanner {
    cache: ScanCache,
}

impl ClapScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cache(&self) -> &ScanCache {
        &self.cache
    }

    pub fn cache_mut(&mut self) -> &mut ScanCache {
        &mut self.cache
    }

    /// Scan trusted libraries on the current green-zone thread.
    ///
    /// Prefer [`Self::spawn`] from a UI. No filesystem or loader operation in
    /// this method is suitable for the audio callback.
    pub fn scan_blocking(
        &mut self,
        candidates: &[TrustedPluginPath],
        options: &ScanOptions,
    ) -> ScanReport {
        let mut report = ScanReport::default();
        if candidates.len() > options.max_candidates {
            report.diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticCode::CandidateLimitReached,
                PathBuf::new(),
                None,
                format!(
                    "trusted input contained {} candidates; scanned the configured maximum of {}",
                    candidates.len(),
                    options.max_candidates
                ),
            ));
        }
        let mut seen_paths = HashSet::new();
        for candidate in candidates.iter().take(options.max_candidates) {
            if !seen_paths.insert(candidate.path.clone()) {
                report.diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Info,
                    DiagnosticCode::DuplicateCandidate,
                    candidate.path(),
                    None,
                    "trusted candidate was already scanned in this pass",
                ));
                continue;
            }
            report.libraries_scanned += 1;
            let fingerprint = match FileFingerprint::read(candidate.path()) {
                Ok(fingerprint) => fingerprint,
                Err(error) => {
                    report.diagnostics.push(ScanDiagnostic::new(
                        DiagnosticSeverity::Error,
                        DiagnosticCode::MetadataFailed,
                        candidate.path(),
                        None,
                        error.to_string(),
                    ));
                    continue;
                }
            };

            let library_scan = if let Some(cached) = self.cache.get(candidate.path(), &fingerprint)
            {
                report.libraries_from_cache += 1;
                cached
            } else {
                let scanned = scan_library(candidate, options);
                self.cache
                    .insert(candidate.path.clone(), fingerprint, scanned.clone());
                scanned
            };
            report.plugins.extend(library_scan.plugins);
            report.diagnostics.extend(library_scan.diagnostics);
        }

        report.plugins.sort_by(|left, right| {
            left.key
                .plugin_id
                .cmp(&right.key.plugin_id)
                .then_with(|| left.key.library_path.cmp(&right.key.library_path))
                .then_with(|| left.factory_index.cmp(&right.factory_index))
        });
        let mut first_location = HashMap::new();
        for plugin in &report.plugins {
            if let Some(previous) = first_location.insert(
                plugin.key.plugin_id.clone(),
                plugin.key.library_path.clone(),
            ) && previous != plugin.key.library_path
            {
                report.diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Warning,
                    DiagnosticCode::DuplicatePluginId,
                    &plugin.key.library_path,
                    Some(plugin.factory_index),
                    format!(
                        "plugin ID `{}` is also exposed by `{}`",
                        plugin.key.plugin_id,
                        previous.display()
                    ),
                ));
            }
        }
        report
    }

    /// Move this scanner and its cache to a dedicated scan thread.
    pub fn spawn(
        self,
        candidates: Vec<TrustedPluginPath>,
        options: ScanOptions,
    ) -> io::Result<ScanJob> {
        let handle = thread::Builder::new()
            .name("stage-clap-scan".to_owned())
            .spawn(move || {
                let mut scanner = self;
                let report = scanner.scan_blocking(&candidates, &options);
                ScanCompletion { scanner, report }
            })?;
        Ok(ScanJob {
            handle: Some(handle),
        })
    }
}

/// A non-blocking handle to a background plugin scan.
pub struct ScanJob {
    handle: Option<JoinHandle<ScanCompletion>>,
}

impl ScanJob {
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Take a completed result without blocking the UI thread.
    pub fn try_complete(&mut self) -> Option<Result<ScanCompletion, ScanWorkerError>> {
        if !self.is_finished() {
            return None;
        }
        let handle = self.handle.take()?;
        Some(handle.join().map_err(|_| ScanWorkerError))
    }
}

/// Scanner and report returned together so a later scan retains its cache.
#[derive(Debug)]
pub struct ScanCompletion {
    pub scanner: ClapScanner,
    pub report: ScanReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("CLAP scan worker panicked")]
pub struct ScanWorkerError;

fn scan_library(candidate: &TrustedPluginPath, options: &ScanOptions) -> LibraryScan {
    let path = candidate.path();
    // SAFETY: `TrustedPluginPath` can only be constructed at an explicit unsafe
    // trust boundary. Clack validates the entry ABI/version after loading it.
    let entry = match unsafe { PluginEntry::load(path.as_os_str()) } {
        Ok(entry) => entry,
        Err(error) => {
            return LibraryScan {
                plugins: Vec::new(),
                diagnostics: vec![ScanDiagnostic::new(
                    DiagnosticSeverity::Error,
                    DiagnosticCode::LibraryLoadFailed,
                    path,
                    None,
                    error.to_string(),
                )],
            };
        }
    };
    let Some(factory) = entry.get_plugin_factory() else {
        return LibraryScan {
            plugins: Vec::new(),
            diagnostics: vec![ScanDiagnostic::new(
                DiagnosticSeverity::Error,
                DiagnosticCode::MissingPluginFactory,
                path,
                None,
                "entry does not expose the CLAP plugin factory",
            )],
        };
    };

    let advertised = factory.plugin_count();
    let count = advertised.min(options.max_descriptors_per_library);
    let mut plugins = Vec::with_capacity(count as usize);
    let mut diagnostics = Vec::new();
    if advertised > count {
        diagnostics.push(ScanDiagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticCode::DescriptorLimitReached,
            path,
            None,
            format!(
                "entry advertised {advertised} descriptors; inspected the configured maximum of {count}"
            ),
        ));
    }

    let mut ids = HashSet::new();
    for index in 0..count {
        let Some(descriptor) = factory.plugin_descriptor(index) else {
            diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Error,
                DiagnosticCode::MissingDescriptor,
                path,
                Some(index),
                "factory returned a null descriptor below its advertised count",
            ));
            continue;
        };
        let (plugin, mut descriptor_diagnostics) =
            snapshot_descriptor(path, index, descriptor, options.max_features_per_plugin);
        diagnostics.append(&mut descriptor_diagnostics);
        let Some(plugin) = plugin else {
            continue;
        };
        if !ids.insert(plugin.key.plugin_id.clone()) {
            diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Error,
                DiagnosticCode::DuplicatePluginId,
                path,
                Some(index),
                format!(
                    "duplicate plugin ID `{}` in the same library",
                    plugin.key.plugin_id
                ),
            ));
            continue;
        }
        plugins.push(plugin);
    }
    LibraryScan {
        plugins,
        diagnostics,
    }
}

fn snapshot_descriptor(
    path: &Path,
    index: u32,
    descriptor: &PluginDescriptor,
    max_features: usize,
) -> (Option<PluginDescriptorInfo>, Vec<ScanDiagnostic>) {
    let raw = DescriptorFields {
        id: descriptor.id().map(CStr::to_bytes),
        name: descriptor.name().map(CStr::to_bytes),
        vendor: descriptor.vendor().map(CStr::to_bytes),
        version: descriptor.version().map(CStr::to_bytes),
        description: descriptor.description().map(CStr::to_bytes),
        url: descriptor.url().map(CStr::to_bytes),
        manual_url: descriptor.manual_url().map(CStr::to_bytes),
        support_url: descriptor.support_url().map(CStr::to_bytes),
    };
    let mut diagnostics = Vec::new();
    let Some((plugin_id, name, optional)) =
        validate_descriptor_fields(path, index, raw, &mut diagnostics)
    else {
        return (None, diagnostics);
    };

    let mut features = Vec::new();
    let mut feature_iter = descriptor.features();
    for feature in feature_iter.by_ref().take(max_features) {
        match feature.to_str() {
            Ok(feature) => features.push(feature.to_owned()),
            Err(_) => {
                diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Warning,
                    DiagnosticCode::InvalidOptionalText,
                    path,
                    Some(index),
                    "non-UTF-8 feature tag was copied lossily",
                ));
                features.push(feature.to_string_lossy().into_owned());
            }
        }
    }
    if feature_iter.next().is_some() {
        diagnostics.push(ScanDiagnostic::new(
            DiagnosticSeverity::Warning,
            DiagnosticCode::FeatureLimitReached,
            path,
            Some(index),
            format!("feature list was truncated at {max_features} entries"),
        ));
    }
    features.sort();
    features.dedup();

    (
        Some(PluginDescriptorInfo {
            key: PluginKey {
                library_path: path.to_path_buf(),
                plugin_id,
            },
            factory_index: index,
            name,
            vendor: optional.vendor,
            version: optional.version,
            description: optional.description,
            url: optional.url,
            manual_url: optional.manual_url,
            support_url: optional.support_url,
            features,
        }),
        diagnostics,
    )
}

#[derive(Clone, Copy)]
struct DescriptorFields<'a> {
    id: Option<&'a [u8]>,
    name: Option<&'a [u8]>,
    vendor: Option<&'a [u8]>,
    version: Option<&'a [u8]>,
    description: Option<&'a [u8]>,
    url: Option<&'a [u8]>,
    manual_url: Option<&'a [u8]>,
    support_url: Option<&'a [u8]>,
}

#[derive(Default)]
struct OptionalDescriptorText {
    vendor: Option<String>,
    version: Option<String>,
    description: Option<String>,
    url: Option<String>,
    manual_url: Option<String>,
    support_url: Option<String>,
}

fn validate_descriptor_fields(
    path: &Path,
    index: u32,
    fields: DescriptorFields<'_>,
    diagnostics: &mut Vec<ScanDiagnostic>,
) -> Option<(PluginId, String, OptionalDescriptorText)> {
    let id = match fields.id {
        Some(id) => match std::str::from_utf8(id) {
            Ok(id) => match PluginId::new(id) {
                Ok(id) => id,
                Err(error) => {
                    diagnostics.push(ScanDiagnostic::new(
                        DiagnosticSeverity::Error,
                        DiagnosticCode::InvalidPluginId,
                        path,
                        Some(index),
                        error.to_string(),
                    ));
                    return None;
                }
            },
            Err(_) => {
                diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Error,
                    DiagnosticCode::InvalidPluginId,
                    path,
                    Some(index),
                    "plugin ID is not valid UTF-8",
                ));
                return None;
            }
        },
        None => {
            diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Error,
                DiagnosticCode::InvalidPluginId,
                path,
                Some(index),
                "plugin descriptor has no ID",
            ));
            return None;
        }
    };

    let name = match fields.name {
        Some(name) => match std::str::from_utf8(name) {
            Ok(name) if !name.trim().is_empty() => name.to_owned(),
            Ok(_) => {
                diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Error,
                    DiagnosticCode::InvalidPluginName,
                    path,
                    Some(index),
                    "plugin descriptor has a blank name",
                ));
                return None;
            }
            Err(_) => {
                diagnostics.push(ScanDiagnostic::new(
                    DiagnosticSeverity::Error,
                    DiagnosticCode::InvalidPluginName,
                    path,
                    Some(index),
                    "plugin name is not valid UTF-8",
                ));
                return None;
            }
        },
        None => {
            diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Error,
                DiagnosticCode::InvalidPluginName,
                path,
                Some(index),
                "plugin descriptor has no name",
            ));
            return None;
        }
    };

    let optional = OptionalDescriptorText {
        vendor: optional_text(path, index, "vendor", fields.vendor, diagnostics),
        version: optional_text(path, index, "version", fields.version, diagnostics),
        description: optional_text(path, index, "description", fields.description, diagnostics),
        url: optional_text(path, index, "URL", fields.url, diagnostics),
        manual_url: optional_text(path, index, "manual URL", fields.manual_url, diagnostics),
        support_url: optional_text(path, index, "support URL", fields.support_url, diagnostics),
    };
    Some((id, name, optional))
}

fn optional_text(
    path: &Path,
    index: u32,
    field_name: &str,
    bytes: Option<&[u8]>,
    diagnostics: &mut Vec<ScanDiagnostic>,
) -> Option<String> {
    let bytes = bytes?;
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_owned()),
        Err(_) => {
            diagnostics.push(ScanDiagnostic::new(
                DiagnosticSeverity::Warning,
                DiagnosticCode::InvalidOptionalText,
                path,
                Some(index),
                format!("{field_name} is not valid UTF-8 and was copied lossily"),
            ));
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
    }
}

/// Loaded native entry retained while instances from it exist.
pub struct ClapLibrary {
    path: PathBuf,
    entry: PluginEntry,
}

impl ClapLibrary {
    /// Load a trusted CLAP entry on the current green-zone control thread.
    pub fn load(path: &TrustedPluginPath) -> Result<Self, LibraryLoadError> {
        // SAFETY: construction of the trust token is the explicit unsafe
        // boundary required by PluginEntry::load.
        let entry = unsafe { PluginEntry::load(path.path.as_os_str()) }.map_err(|source| {
            LibraryLoadError {
                path: path.path.clone(),
                source,
            }
        })?;
        Ok(Self {
            path: path.path.clone(),
            entry,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Instantiate a validated ID. The returned control object is `!Send` by
    /// construction through Clack and therefore remains on this thread.
    pub fn instantiate(&self, plugin_id: &PluginId) -> Result<PluginControl, PluginLoadError> {
        let host = HostInfo::new("Stage", "Stage", "", env!("CARGO_PKG_VERSION"))?;
        self.instantiate_with_host(plugin_id, &host)
    }

    pub fn instantiate_with_host(
        &self,
        plugin_id: &PluginId,
        host: &HostInfo,
    ) -> Result<PluginControl, PluginLoadError> {
        let plugin_id_c = plugin_id.as_cstring()?;
        let instance = PluginInstance::<StageClapHost>::new(
            |_| StageHostShared::default(),
            |_| (),
            &self.entry,
            &plugin_id_c,
            host,
        )?;
        Ok(PluginControl {
            key: PluginKey {
                library_path: self.path.clone(),
                plugin_id: plugin_id.clone(),
            },
            instance,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to load CLAP library `{}`: {source}", path.display())]
pub struct LibraryLoadError {
    pub path: PathBuf,
    #[source]
    pub source: PluginEntryError,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("invalid CLAP host or plugin string: {0}")]
    InvalidString(#[from] NulError),
    #[error("failed to instantiate CLAP plugin: {0}")]
    Instance(#[from] PluginInstanceError),
}

#[derive(Default)]
struct StageHostShared {
    requests: AtomicU8,
}

impl StageHostShared {
    fn snapshot(&self) -> HostRequests {
        HostRequests::from_bits(self.requests.load(Ordering::Acquire))
    }

    fn take(&self) -> HostRequests {
        HostRequests::from_bits(self.requests.swap(0, Ordering::AcqRel))
    }

    fn take_callback(&self) -> bool {
        let previous = self.requests.fetch_and(!REQUEST_CALLBACK, Ordering::AcqRel);
        previous & REQUEST_CALLBACK != 0
    }
}

impl<'a> SharedHandler<'a> for StageHostShared {
    #[inline]
    fn request_restart(&self) {
        self.requests.fetch_or(REQUEST_RESTART, Ordering::Release);
    }

    #[inline]
    fn request_process(&self) {
        self.requests.fetch_or(REQUEST_PROCESS, Ordering::Release);
    }

    #[inline]
    fn request_callback(&self) {
        self.requests.fetch_or(REQUEST_CALLBACK, Ordering::Release);
    }
}

struct StageClapHost;

impl HostHandlers for StageClapHost {
    type Shared<'a> = StageHostShared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

/// Wait-free requests raised by a plugin through the base CLAP host callbacks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostRequests {
    pub restart: bool,
    pub process: bool,
    pub main_thread_callback: bool,
}

impl HostRequests {
    fn from_bits(bits: u8) -> Self {
        Self {
            restart: bits & REQUEST_RESTART != 0,
            process: bits & REQUEST_PROCESS != 0,
            main_thread_callback: bits & REQUEST_CALLBACK != 0,
        }
    }

    pub fn any(self) -> bool {
        self.restart || self.process || self.main_thread_callback
    }
}

/// Panic-free validated activation parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClapAudioConfig {
    sample_rate: f64,
    min_frames: u32,
    max_frames: u32,
}

impl ClapAudioConfig {
    pub fn new(
        sample_rate: f64,
        min_frames: u32,
        max_frames: u32,
    ) -> Result<Self, AudioConfigError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(AudioConfigError::InvalidSampleRate);
        }
        if min_frames == 0 || max_frames == 0 || min_frames > max_frames {
            return Err(AudioConfigError::InvalidFrameRange);
        }
        Ok(Self {
            sample_rate,
            min_frames,
            max_frames,
        })
    }

    pub fn sample_rate(self) -> f64 {
        self.sample_rate
    }

    pub fn min_frames(self) -> u32 {
        self.min_frames
    }

    pub fn max_frames(self) -> u32 {
        self.max_frames
    }

    fn as_clack(self) -> PluginAudioConfiguration {
        PluginAudioConfiguration {
            sample_rate: self.sample_rate,
            min_frames_count: self.min_frames,
            max_frames_count: self.max_frames,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AudioConfigError {
    #[error("sample rate must be finite and greater than zero")]
    InvalidSampleRate,
    #[error("frame range must be nonzero and ordered")]
    InvalidFrameRange,
}

/// Main/control-thread half of a CLAP instance.
pub struct PluginControl {
    key: PluginKey,
    instance: PluginInstance<StageClapHost>,
}

impl PluginControl {
    pub fn key(&self) -> &PluginKey {
        &self.key
    }

    pub fn requests(&self) -> HostRequests {
        self.instance
            .access_shared_handler(StageHostShared::snapshot)
    }

    pub fn take_requests(&self) -> HostRequests {
        self.instance.access_shared_handler(StageHostShared::take)
    }

    /// Service a requested `on_main_thread` callback without clearing restart
    /// or process requests.
    pub fn service_main_thread_callback(&mut self) -> bool {
        let requested = self
            .instance
            .access_shared_handler(StageHostShared::take_callback);
        if requested {
            self.instance.call_on_main_thread_callback();
        }
        requested
    }

    /// Activate on this control thread; start processing later on the audio
    /// thread through the returned processor.
    pub fn activate(
        &mut self,
        config: ClapAudioConfig,
    ) -> Result<ClapProcessor, PluginInstanceError> {
        let processor = self.instance.activate(|_, _| (), config.as_clack())?;
        Ok(ClapProcessor {
            processor: Some(processor.into()),
            config,
        })
    }

    /// Deactivate a stopped processor on this control thread.
    ///
    /// The processor remains untouched on error, including if it belongs to a
    /// different instance. A running processor must first be stopped on the
    /// audio thread.
    pub fn deactivate(&mut self, processor: &mut ClapProcessor) -> Result<(), LifecycleError> {
        let Some(inner) = processor.processor.as_ref() else {
            return Err(LifecycleError::DetachedProcessor);
        };
        if !inner.matches(&self.instance) {
            return Err(LifecycleError::WrongInstance);
        }
        if inner.is_started() {
            return Err(LifecycleError::ProcessorStillStarted);
        }
        let Some(inner) = processor.processor.take() else {
            return Err(LifecycleError::DetachedProcessor);
        };
        self.instance.deactivate(inner.into_stopped());
        Ok(())
    }

    /// Recover after a processor handle was deliberately dropped on a green
    /// thread rather than returned. This must never race the audio thread.
    pub fn try_deactivate_after_processor_drop(&mut self) -> Result<(), PluginInstanceError> {
        self.instance.try_deactivate()
    }
}

/// Audio-thread half of an activated plugin.
///
/// This type is `Send` but not `Sync`. All methods are bounded and perform no
/// allocation, locks, logging, or I/O in host code. Native plugin code remains
/// responsible for its own CLAP realtime obligations. Return this object to a
/// green thread before dropping it; graph retirement should use `basedrop`.
pub struct ClapProcessor {
    processor: Option<PluginAudioProcessor<StageClapHost>>,
    config: ClapAudioConfig,
}

impl ClapProcessor {
    pub fn config(&self) -> ClapAudioConfig {
        self.config
    }

    pub fn is_started(&self) -> bool {
        self.processor
            .as_ref()
            .is_some_and(PluginAudioProcessor::is_started)
    }

    pub fn start_processing(&mut self) -> Result<(), LifecycleError> {
        let processor = self
            .processor
            .as_mut()
            .ok_or(LifecycleError::DetachedProcessor)?;
        if processor.is_started() {
            return Ok(());
        }
        processor.start_processing()?;
        Ok(())
    }

    pub fn stop_processing(&mut self) -> Result<(), LifecycleError> {
        let processor = self
            .processor
            .as_mut()
            .ok_or(LifecycleError::DetachedProcessor)?;
        if !processor.is_started() {
            return Ok(());
        }
        processor.stop_processing()?;
        Ok(())
    }

    pub fn reset(&mut self) -> Result<(), LifecycleError> {
        let processor = self
            .processor
            .as_mut()
            .ok_or(LifecycleError::DetachedProcessor)?;
        processor.reset();
        Ok(())
    }

    /// Process arbitrary pre-wrapped CLAP audio/event buffers.
    ///
    /// `AudioPorts` and event storage must be fully preallocated by the caller
    /// before entering the callback. The frame count is checked against the
    /// activation range before plugin code is called.
    pub fn process_buffers(
        &mut self,
        inputs: &InputAudioBuffers<'_>,
        outputs: &mut OutputAudioBuffers<'_>,
        input_events: &InputEvents<'_>,
        output_events: &mut OutputEvents<'_>,
        steady_time: Option<u64>,
        transport: Option<&TransportEvent>,
    ) -> Result<ClapProcessStatus, ProcessError> {
        let frames = inputs.min_available_frames_with(outputs);
        if frames < self.config.min_frames || frames > self.config.max_frames {
            return Err(ProcessError::FrameCount {
                frames,
                min: self.config.min_frames,
                max: self.config.max_frames,
            });
        }
        let processor = self
            .processor
            .as_mut()
            .ok_or(ProcessError::DetachedProcessor)?;
        let started = processor.as_started_mut().map_err(ProcessError::Plugin)?;
        started
            .process(
                inputs,
                outputs,
                input_events,
                output_events,
                steady_time,
                transport,
            )
            .map_err(ProcessError::Plugin)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LifecycleError {
    #[error("CLAP processor is detached")]
    DetachedProcessor,
    #[error("CLAP processor belongs to a different plugin instance")]
    WrongInstance,
    #[error("CLAP processor is still running and must be stopped on the audio thread")]
    ProcessorStillStarted,
    #[error("CLAP lifecycle operation failed: {0}")]
    Plugin(#[from] PluginInstanceError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProcessError {
    #[error("CLAP processor is detached")]
    DetachedProcessor,
    #[error("CLAP block has {frames} frames; active range is {min}..={max}")]
    FrameCount { frames: u32, min: u32, max: u32 },
    #[error("CLAP process call failed: {0}")]
    Plugin(PluginInstanceError),
}

/// Preallocated adapter for the common one-port, stereo-f32 effect layout.
///
/// Instruments and plugins with other port layouts should use
/// [`ClapProcessor::process_buffers`] with their own green-zone-prepared
/// `AudioPorts`. This adapter copies immutable graph inputs into plugin-owned
/// scratch because CLAP permits plugins mutable access to input buffers.
pub struct StereoBufferAdapter {
    max_frames: u32,
    input_left: Box<[f32]>,
    input_right: Box<[f32]>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
}

impl StereoBufferAdapter {
    /// Allocate all scratch and pointer tables on the green/control thread.
    pub fn new(max_frames: u32) -> Result<Self, AudioConfigError> {
        if max_frames == 0 {
            return Err(AudioConfigError::InvalidFrameRange);
        }
        Ok(Self {
            max_frames,
            input_left: vec![0.0; max_frames as usize].into_boxed_slice(),
            input_right: vec![0.0; max_frames as usize].into_boxed_slice(),
            input_ports: AudioPorts::with_capacity(2, 1),
            output_ports: AudioPorts::with_capacity(2, 1),
        })
    }

    pub fn max_frames(&self) -> u32 {
        self.max_frames
    }

    /// Process stereo audio with no events or transport information.
    pub fn process(
        &mut self,
        processor: &mut ClapProcessor,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        steady_time: Option<u64>,
    ) -> Result<ClapProcessStatus, StereoProcessError> {
        let input_events = InputEvents::empty();
        let mut output_events = OutputEvents::void();
        self.process_with_events(
            processor,
            input_left,
            input_right,
            output_left,
            output_right,
            &input_events,
            &mut output_events,
            steady_time,
            None,
        )
    }

    /// Process one graph block after filling the adapter's resident input.
    ///
    /// The closure receives two zero-cost mutable views of preallocated
    /// scratch. It is the graph seam for summing an arbitrary fixed number of
    /// immutable edges without allocating or aliasing CLAP's mutable inputs
    /// with its outputs.
    pub fn process_filled(
        &mut self,
        processor: &mut ClapProcessor,
        frames: usize,
        output_left: &mut [f32],
        output_right: &mut [f32],
        steady_time: Option<u64>,
        fill: impl FnOnce(&mut [f32], &mut [f32]),
    ) -> Result<ClapProcessStatus, StereoProcessError> {
        if output_left.len() != frames || output_right.len() != frames {
            return Err(StereoProcessError::MismatchedLengths);
        }
        if frames > self.max_frames as usize {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        }
        let Some(scratch_left) = self.input_left.get_mut(..frames) else {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        };
        let Some(scratch_right) = self.input_right.get_mut(..frames) else {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        };
        fill(scratch_left, scratch_right);

        let input_port = AudioPortBuffer {
            channels: AudioPortBufferType::f32_input_only([
                InputChannel::variable(scratch_left),
                InputChannel::variable(scratch_right),
            ]),
            latency: 0,
        };
        let output_port = AudioPortBuffer {
            channels: AudioPortBufferType::f32_output_only([output_left, output_right]),
            latency: 0,
        };
        let inputs = self.input_ports.with_input_buffers([input_port]);
        let mut outputs = self.output_ports.with_output_buffers([output_port]);
        let input_events = InputEvents::empty();
        let mut output_events = OutputEvents::void();
        processor
            .process_buffers(
                &inputs,
                &mut outputs,
                &input_events,
                &mut output_events,
                steady_time,
                None,
            )
            .map_err(StereoProcessError::Process)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_with_events(
        &mut self,
        processor: &mut ClapProcessor,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        input_events: &InputEvents<'_>,
        output_events: &mut OutputEvents<'_>,
        steady_time: Option<u64>,
        transport: Option<&TransportEvent>,
    ) -> Result<ClapProcessStatus, StereoProcessError> {
        let frames = input_left.len();
        if input_right.len() != frames
            || output_left.len() != frames
            || output_right.len() != frames
        {
            return Err(StereoProcessError::MismatchedLengths);
        }
        if frames > self.max_frames as usize {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        }
        let Some(scratch_left) = self.input_left.get_mut(..frames) else {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        };
        let Some(scratch_right) = self.input_right.get_mut(..frames) else {
            return Err(StereoProcessError::AdapterCapacity {
                frames,
                max: self.max_frames,
            });
        };
        for (destination, source) in scratch_left.iter_mut().zip(input_left) {
            *destination = *source;
        }
        for (destination, source) in scratch_right.iter_mut().zip(input_right) {
            *destination = *source;
        }

        let input_port = AudioPortBuffer {
            channels: AudioPortBufferType::f32_input_only([
                InputChannel::variable(scratch_left),
                InputChannel::variable(scratch_right),
            ]),
            latency: 0,
        };
        let output_port = AudioPortBuffer {
            channels: AudioPortBufferType::f32_output_only([output_left, output_right]),
            latency: 0,
        };
        let inputs = self.input_ports.with_input_buffers([input_port]);
        let mut outputs = self.output_ports.with_output_buffers([output_port]);
        processor
            .process_buffers(
                &inputs,
                &mut outputs,
                input_events,
                output_events,
                steady_time,
                transport,
            )
            .map_err(StereoProcessError::Process)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StereoProcessError {
    #[error("stereo CLAP buffers have different frame counts")]
    MismatchedLengths,
    #[error("stereo CLAP block has {frames} frames; adapter capacity is {max}")]
    AdapterCapacity { frames: usize, max: u32 },
    #[error(transparent)]
    Process(#[from] ProcessError),
}

/// A reusable, green-zone recipe for the graph's one-port stereo CLAP node.
///
/// The recipe contains only trusted identity data and is therefore cloneable
/// when PDC rebuilds a [`GraphSpec`](crate::audio::graph::GraphSpec). Every
/// compile creates a fresh plugin instance; a live processor is never cloned.
#[derive(Clone, Debug)]
pub struct ClapNodeFactory {
    path: TrustedPluginPath,
    plugin_id: PluginId,
    setup_timeout: Duration,
}

impl ClapNodeFactory {
    pub fn new(path: TrustedPluginPath, plugin_id: PluginId) -> Self {
        Self {
            path,
            plugin_id,
            setup_timeout: Duration::from_secs(10),
        }
    }

    pub fn with_setup_timeout(mut self, timeout: Duration) -> Self {
        self.setup_timeout = timeout;
        self
    }

    pub fn path(&self) -> &Path {
        self.path.path()
    }

    pub fn plugin_id(&self) -> &PluginId {
        &self.plugin_id
    }

    pub fn matches(&self, plugin: &ClapDeviceState) -> bool {
        self.path.path() == plugin.key.library_path && self.plugin_id == plugin.key.plugin_id
    }

    /// Load, instantiate and activate a fresh graph processor.
    ///
    /// A dedicated green control thread owns `ClapLibrary`, `PluginControl`
    /// and the basedrop collector for the complete instance lifetime. The
    /// returned `Owned` pointer is the only object that crosses into the audio
    /// graph. Retiring it is one lock-free enqueue; the control thread then
    /// stops as a last-resort fallback, deactivates and destroys the instance.
    pub fn prepare(
        &self,
        plugin: &ClapDeviceState,
        sample_rate: u32,
        block_frames: usize,
    ) -> Result<basedrop::Owned<ClapGraphProcessor>, ClapNodePrepareError> {
        if !self.matches(plugin) {
            return Err(ClapNodePrepareError::FactoryIdentityMismatch);
        }
        if !plugin.state_matches_key() {
            return Err(ClapNodePrepareError::StateIdentityMismatch);
        }
        // Loading defaults while silently ignoring authored state would make a
        // project open with the wrong sound. Refuse until the audited state
        // extension named in the module docs is available.
        if plugin.state.is_some() {
            return Err(ClapNodePrepareError::StateRestoreUnsupported);
        }
        let max_frames = u32::try_from(block_frames)
            .ok()
            .filter(|frames| *frames > 0)
            .ok_or(ClapNodePrepareError::InvalidBlockFrames)?;
        let config = ClapAudioConfig::new(f64::from(sample_rate), 1, max_frames)?;
        let path = self.path.clone();
        let plugin_id = self.plugin_id.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("stage-clap-control".to_owned())
            .spawn(move || graph_control_worker(path, plugin_id, config, ready_tx))
            .map_err(ClapNodePrepareError::ThreadSpawn)?;

        match ready_rx.recv_timeout(self.setup_timeout) {
            Ok(Ok(processor)) => Ok(processor),
            Ok(Err(error)) => Err(ClapNodePrepareError::Plugin(error)),
            Err(RecvTimeoutError::Timeout) => Err(ClapNodePrepareError::SetupTimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(ClapNodePrepareError::WorkerDisconnected),
        }
    }
}

/// The live object stored in `Node::Clap` behind `basedrop::Owned`.
///
/// It owns every allocation touched by the callback. `prepare_for_retirement`
/// is called on the audio thread before a schedule swap. If a dead backend
/// makes that handshake impossible, `Drop` deliberately leaks a still-started
/// native handle: CLAP forbids stopping it from this collector thread, and a
/// bounded process-lifetime leak is safer than calling plugin code with the
/// wrong thread affinity.
pub struct ClapGraphProcessor {
    processor: Option<ClapProcessor>,
    adapter: StereoBufferAdapter,
}

impl ClapGraphProcessor {
    pub(crate) fn process_filled(
        &mut self,
        frames: usize,
        output_left: &mut [f32],
        output_right: &mut [f32],
        discontinuity: bool,
        fill: impl FnOnce(&mut [f32], &mut [f32]),
    ) -> Result<ClapProcessStatus, ClapGraphProcessError> {
        let processor = self
            .processor
            .as_mut()
            .ok_or(ClapGraphProcessError::DetachedProcessor)?;
        if discontinuity {
            processor.reset()?;
        }
        processor.start_processing()?;
        self.adapter
            .process_filled(processor, frames, output_left, output_right, None, fill)
            .map_err(ClapGraphProcessError::Audio)
    }

    /// Audio-thread lifecycle edge used immediately before schedule retirement.
    pub(crate) fn prepare_for_retirement(&mut self) {
        if let Some(processor) = self.processor.as_mut() {
            let _ = processor.stop_processing();
        }
    }
}

impl Drop for ClapGraphProcessor {
    fn drop(&mut self) {
        if let Some(processor) = self.processor.take() {
            if processor.is_started() {
                // The callback failed to acknowledge retirement. Calling
                // stop_processing here would violate CLAP's audio-thread
                // contract and lets a strict plugin crash or deadlock during
                // shutdown. Keeping the handle alive also keeps Clack's
                // instance and dynamic-library entry alive, so no native code
                // is unloaded underneath a plugin-owned worker.
                std::mem::forget(processor);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClapGraphProcessError {
    #[error("CLAP graph processor is detached")]
    DetachedProcessor,
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Audio(#[from] StereoProcessError),
}

#[derive(Debug, thiserror::Error)]
pub enum ClapNodePrepareError {
    #[error("CLAP graph factory identity does not match the persisted device")]
    FactoryIdentityMismatch,
    #[error("CLAP state blob belongs to a different descriptor")]
    StateIdentityMismatch,
    #[error("CLAP state restore requires the audited clack-extensions state adapter")]
    StateRestoreUnsupported,
    #[error("CLAP graph block size must fit a nonzero u32")]
    InvalidBlockFrames,
    #[error(transparent)]
    AudioConfig(#[from] AudioConfigError),
    #[error("could not spawn CLAP control thread: {0}")]
    ThreadSpawn(io::Error),
    #[error("CLAP setup exceeded its bounded wait")]
    SetupTimedOut,
    #[error("CLAP setup worker exited without a result")]
    WorkerDisconnected,
    #[error("CLAP setup failed: {0}")]
    Plugin(String),
}

fn graph_control_worker(
    path: TrustedPluginPath,
    plugin_id: PluginId,
    config: ClapAudioConfig,
    ready: mpsc::SyncSender<Result<basedrop::Owned<ClapGraphProcessor>, String>>,
) {
    let library = match ClapLibrary::load(&path) {
        Ok(library) => library,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let mut control = match library.instantiate(&plugin_id) {
        Ok(control) => control,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let adapter = match StereoBufferAdapter::new(config.max_frames()) {
        Ok(adapter) => adapter,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let processor = match control.activate(config) {
        Ok(processor) => processor,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };

    let mut collector = basedrop::Collector::new();
    let owned = basedrop::Owned::new(
        &collector.handle(),
        ClapGraphProcessor {
            processor: Some(processor),
            adapter,
        },
    );
    if ready.send(Ok(owned)).is_err() {
        // The receiver timed out. The failed send dropped `Owned` into this
        // collector, so collection and deactivation still happen here.
        collector.collect();
        let _ = control.try_deactivate_after_processor_drop();
        let _ = collector.try_cleanup();
        return;
    }

    loop {
        collector.collect();
        if collector.alloc_count() == 0 {
            let _ = control.try_deactivate_after_processor_drop();
            let _ = collector.try_cleanup();
            return;
        }
        // Only this thread calls the plugin's main-thread callback. Restart and
        // process requests remain latched for the future supervisor seam.
        let _ = control.service_main_thread_callback();
        thread::sleep(Duration::from_millis(4));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::clack_host::plugin::features::{AUDIO_EFFECT, STEREO};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "stage-clap-host-{label}-{}-{sequence}",
                std::process::id()
            ));
            if let Err(error) = fs::create_dir_all(&path) {
                panic!("could not create test directory: {error}");
            }
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Err(error) = fs::write(path, bytes) {
            panic!("could not write test file: {error}");
        }
    }

    #[test]
    fn discovery_is_recursive_filtered_sorted_and_canonical() {
        let temp = TempDirectory::new("discover");
        let nested = temp.path().join("nested");
        if let Err(error) = fs::create_dir_all(&nested) {
            panic!("could not create nested directory: {error}");
        }
        write(&temp.path().join("z.CLAP"), b"not a library");
        write(&nested.join("a.clap"), b"not a library");
        write(&nested.join("ignored.so"), b"not a candidate");

        let report = discover_candidates([temp.path()], &ScanOptions::default());
        assert_eq!(report.candidates.len(), 2);
        assert!(report.candidates[0].path().is_absolute());
        assert!(report.candidates[0].path() < report.candidates[1].path());
        assert!(
            report
                .candidates
                .iter()
                .all(|candidate| is_clap_bundle(candidate.path()))
        );
    }

    #[test]
    fn explicit_file_does_not_need_clap_extension() {
        let temp = TempDirectory::new("explicit");
        let library = temp.path().join("plugin.so");
        write(&library, b"not a library");
        let expected = match fs::canonicalize(&library) {
            Ok(path) => path,
            Err(error) => panic!("could not canonicalize test library: {error}"),
        };
        let report = discover_candidates([&library], &ScanOptions::default());
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].path(), expected);
    }

    #[test]
    fn missing_root_is_a_diagnostic_not_a_panic() {
        let temp = TempDirectory::new("missing");
        let report = discover_candidates(
            [temp.path().join("does-not-exist")],
            &ScanOptions::default(),
        );
        assert!(report.candidates.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, DiagnosticCode::PathNotFound);
    }

    #[test]
    fn discovery_candidate_limit_is_hard() {
        let temp = TempDirectory::new("candidate-limit");
        for name in ["a.clap", "b.clap", "c.clap"] {
            write(&temp.path().join(name), b"not a library");
        }
        let options = ScanOptions {
            max_candidates: 2,
            ..ScanOptions::default()
        };
        let report = discover_candidates([temp.path()], &options);
        assert_eq!(report.candidates.len(), 2);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::CandidateLimitReached)
        );
    }

    #[test]
    fn duplicate_explicit_candidate_is_reported_once() {
        let temp = TempDirectory::new("duplicate");
        let plugin = temp.path().join("duplicate.clap");
        write(&plugin, b"not a library");
        let report = discover_candidates([&plugin, &plugin], &ScanOptions::default());
        assert_eq!(report.candidates.len(), 1);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateCandidate)
        );
    }

    #[test]
    fn plugin_id_rejects_blank_and_nul() {
        assert_eq!(PluginId::new("  "), Err(PluginIdError::Empty));
        assert_eq!(
            PluginId::new("org.example\0bad"),
            Err(PluginIdError::InteriorNul)
        );
        assert_eq!(
            PluginId::new("org.example.good").map(|id| id.as_str().to_owned()),
            Ok("org.example.good".to_owned())
        );
    }

    #[test]
    fn descriptor_validation_rejects_missing_required_fields() {
        let path = Path::new("bad.clap");
        let mut diagnostics = Vec::new();
        let fields = DescriptorFields {
            id: None,
            name: Some(b"Name"),
            vendor: None,
            version: None,
            description: None,
            url: None,
            manual_url: None,
            support_url: None,
        };
        assert!(validate_descriptor_fields(path, 3, fields, &mut diagnostics).is_none());
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidPluginId);

        diagnostics.clear();
        let fields = DescriptorFields {
            id: Some(b"org.example.good"),
            name: Some(b"\xff"),
            vendor: None,
            version: None,
            description: None,
            url: None,
            manual_url: None,
            support_url: None,
        };
        assert!(validate_descriptor_fields(path, 4, fields, &mut diagnostics).is_none());
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidPluginName);
    }

    #[test]
    fn descriptor_validation_keeps_plugin_with_lossy_optional_text() {
        let mut diagnostics = Vec::new();
        let fields = DescriptorFields {
            id: Some(b"org.example.good"),
            name: Some(b"Good"),
            vendor: Some(b"vendor-\xff"),
            version: None,
            description: None,
            url: None,
            manual_url: None,
            support_url: None,
        };
        let result =
            validate_descriptor_fields(Path::new("good.clap"), 0, fields, &mut diagnostics);
        assert!(result.is_some());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidOptionalText);
    }

    #[test]
    fn clack_descriptor_is_copied_into_an_owned_stable_snapshot() {
        let descriptor = PluginDescriptor::new("org.example.gain", "Example Gain")
            .with_vendor("Example")
            .with_version("1.2.3")
            .with_features([AUDIO_EFFECT, STEREO]);
        let (snapshot, diagnostics) =
            snapshot_descriptor(Path::new("gain.clap"), 7, &descriptor, 32);
        assert!(diagnostics.is_empty());
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => panic!("valid clack descriptor was rejected"),
        };
        assert_eq!(snapshot.key.plugin_id.as_str(), "org.example.gain");
        assert_eq!(snapshot.name, "Example Gain");
        assert_eq!(snapshot.vendor.as_deref(), Some("Example"));
        assert_eq!(snapshot.version.as_deref(), Some("1.2.3"));
        assert_eq!(
            snapshot.features,
            vec!["audio-effect".to_owned(), "stereo".to_owned()]
        );
    }

    #[test]
    fn cache_requires_an_exact_fingerprint() {
        let mut cache = ScanCache::default();
        let path = PathBuf::from("plugin.clap");
        let old = FileFingerprint {
            len: 10,
            modified: None,
            is_directory: false,
        };
        let changed = FileFingerprint {
            len: 11,
            modified: None,
            is_directory: false,
        };
        let scan = LibraryScan {
            plugins: Vec::new(),
            diagnostics: Vec::new(),
        };
        cache.insert(path.clone(), old.clone(), scan);
        assert!(cache.get(&path, &old).is_some());
        assert!(cache.get(&path, &changed).is_none());
        cache.invalidate(&path);
        assert!(cache.is_empty());
    }

    #[test]
    fn invalid_library_diagnostic_is_cached_without_a_system_plugin() {
        let temp = TempDirectory::new("bad-library");
        let path = temp.path().join("bad.clap");
        write(&path, b"definitely not a shared object");
        let candidate = ClapCandidate {
            path: match fs::canonicalize(&path) {
                Ok(path) => path,
                Err(error) => panic!("could not canonicalize test library: {error}"),
            },
        };
        // SAFETY: this fixture is inert data created by this test. The loader
        // rejects it before any native initializer can run.
        let trusted = unsafe { TrustedPluginPath::from_candidate(&candidate) };
        let mut scanner = ClapScanner::new();
        let first = scanner.scan_blocking(std::slice::from_ref(&trusted), &ScanOptions::default());
        assert_eq!(first.libraries_from_cache, 0);
        assert!(
            first
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::LibraryLoadFailed)
        );
        let second = scanner.scan_blocking(&[trusted], &ScanOptions::default());
        assert_eq!(second.libraries_from_cache, 1);
        assert_eq!(first.diagnostics, second.diagnostics);
    }

    #[test]
    fn background_empty_scan_completes_and_returns_cache_owner() {
        let mut job = match ClapScanner::new().spawn(Vec::new(), ScanOptions::default()) {
            Ok(job) => job,
            Err(error) => panic!("could not spawn scan worker: {error}"),
        };
        let started = SystemTime::now();
        loop {
            if let Some(result) = job.try_complete() {
                let completion = match result {
                    Ok(completion) => completion,
                    Err(error) => panic!("scan worker failed: {error}"),
                };
                assert_eq!(completion.report, ScanReport::default());
                assert!(completion.scanner.cache().is_empty());
                break;
            }
            if started.elapsed().is_ok_and(|elapsed| elapsed.as_secs() > 5) {
                panic!("scan worker did not complete");
            }
            thread::yield_now();
        }
    }

    #[test]
    fn audio_configuration_prevents_clack_activation_panics() {
        assert_eq!(
            ClapAudioConfig::new(f64::NAN, 1, 64),
            Err(AudioConfigError::InvalidSampleRate)
        );
        assert_eq!(
            ClapAudioConfig::new(48_000.0, 0, 64),
            Err(AudioConfigError::InvalidFrameRange)
        );
        assert_eq!(
            ClapAudioConfig::new(48_000.0, 128, 64),
            Err(AudioConfigError::InvalidFrameRange)
        );
        assert_eq!(
            ClapAudioConfig::new(48_000.0, 1, 1_024).map(|config| (
                config.sample_rate(),
                config.min_frames(),
                config.max_frames()
            )),
            Ok((48_000.0, 1, 1_024))
        );
    }

    #[test]
    fn base_host_callbacks_are_wait_free_and_coalesced() {
        let shared = StageHostShared::default();
        assert_no_alloc::assert_no_alloc(|| {
            SharedHandler::request_restart(&shared);
            SharedHandler::request_process(&shared);
            SharedHandler::request_callback(&shared);
            SharedHandler::request_callback(&shared);
        });
        assert_eq!(
            shared.take(),
            HostRequests {
                restart: true,
                process: true,
                main_thread_callback: true,
            }
        );
        assert!(!shared.take().any());
    }

    #[test]
    fn stereo_adapter_is_preallocated_and_bounded() {
        assert!(StereoBufferAdapter::new(0).is_err());
        let adapter = match StereoBufferAdapter::new(512) {
            Ok(adapter) => adapter,
            Err(error) => panic!("adapter construction failed: {error}"),
        };
        assert_eq!(adapter.max_frames(), 512);
        assert_eq!(adapter.input_left.len(), 512);
        assert_eq!(adapter.input_right.len(), 512);
    }

    #[test]
    fn processor_half_is_send_for_audio_thread_handoff() {
        fn assert_send<T: Send>() {}
        assert_send::<ClapProcessor>();
    }

    #[test]
    fn state_blob_refuses_a_different_plugin_id() {
        let blob = PluginStateBlob {
            plugin_id: PluginId::new("org.example.one")
                .unwrap_or_else(|error| panic!("valid ID was rejected: {error}")),
            bytes: vec![1, 2, 3],
        };
        let key = PluginKey {
            library_path: PathBuf::from("one.clap"),
            plugin_id: PluginId::new("org.example.two")
                .unwrap_or_else(|error| panic!("valid ID was rejected: {error}")),
        };
        assert!(!blob.belongs_to(&key));
    }

    #[test]
    fn missing_device_round_trip_preserves_identity_state_and_latency() {
        let plugin_id = PluginId::new("org.example.saved")
            .unwrap_or_else(|error| panic!("valid ID was rejected: {error}"));
        let plugin = ClapDeviceState {
            key: PluginKey {
                library_path: PathBuf::from("/plugins/saved.clap"),
                plugin_id: plugin_id.clone(),
            },
            display_name: "Saved effect".to_owned(),
            state: Some(PluginStateBlob {
                plugin_id,
                bytes: vec![4, 8, 15, 16, 23, 42],
            }),
            reported_latency_samples: 127,
        };
        let model = PluginDeviceModel::Clap { plugin }.into_missing("not installed");
        let encoded = ron::to_string(&model)
            .unwrap_or_else(|error| panic!("device did not serialize: {error}"));
        let decoded: PluginDeviceModel = ron::from_str(&encoded)
            .unwrap_or_else(|error| panic!("device did not deserialize: {error}"));
        assert_eq!(decoded, model);
        assert!(decoded.is_missing());
        assert!(decoded.plugin().state_matches_key());
        assert_eq!(decoded.plugin().latency_samples(), 127);
    }

    #[test]
    fn graph_factory_binding_is_identity_exact_without_loading_code() {
        let one = PluginId::new("org.example.one")
            .unwrap_or_else(|error| panic!("valid ID was rejected: {error}"));
        let two = PluginId::new("org.example.two")
            .unwrap_or_else(|error| panic!("valid ID was rejected: {error}"));
        let factory = ClapNodeFactory::new(
            TrustedPluginPath {
                path: PathBuf::from("/plugins/one.clap"),
            },
            one,
        );
        let plugin = ClapDeviceState {
            key: PluginKey {
                library_path: PathBuf::from("/plugins/one.clap"),
                plugin_id: two,
            },
            display_name: "Two".to_owned(),
            state: None,
            reported_latency_samples: 0,
        };
        assert!(!factory.matches(&plugin));
    }

    #[test]
    fn graph_processor_and_deferred_owner_can_cross_to_audio() {
        fn assert_send<T: Send>() {}
        assert_send::<ClapGraphProcessor>();
        assert_send::<basedrop::Owned<ClapGraphProcessor>>();
    }
}
