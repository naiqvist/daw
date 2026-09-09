//! The stage's machine room: projects, preferences, export and diagnostics.
//!
//! This is one modal state machine rather than four unrelated windows.  It
//! owns the keyboard while visible, emits bounded requests at the host seam,
//! and paints in the same display language as the musical surface.

use super::{EngineState, Stage};
use crate::design;
use crate::ui::prefs::{AudioBackend, Autosave, CursorEnergy, ExportFormat, ExportTail, UiPrefs};
use std::path::{Path, PathBuf};

pub(super) const NAV_W: f32 = 172.0;
pub(super) const HEADER_H: f32 = 72.0;
pub(super) const FOOTER_H: f32 = 34.0;
pub(super) const ROW_H: f32 = 38.0;
pub(super) const GAP: f32 = 6.0;
const RECENT_LIMIT: usize = 12;
const EXPORT_HISTORY_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Page {
    Projects,
    Preferences,
    Export,
    Diagnostics,
}

impl Page {
    pub(super) const ALL: [Self; 4] = [
        Self::Projects,
        Self::Preferences,
        Self::Export,
        Self::Diagnostics,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Projects => "PROJECTS",
            Self::Preferences => "PREFERENCES",
            Self::Export => "EXPORT",
            Self::Diagnostics => "SYSTEM",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum PrefPage {
    Audio,
    Projects,
    Library,
    Interface,
}

impl PrefPage {
    pub(super) const ALL: [Self; 4] = [Self::Audio, Self::Projects, Self::Library, Self::Interface];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Audio => "AUDIO",
            Self::Projects => "PROJECTS",
            Self::Library => "LIBRARY",
            Self::Interface => "INTERFACE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportRange {
    Song,
    Loop,
}

impl ExportRange {
    pub(super) const ALL: [Self; 2] = [Self::Song, Self::Loop];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Song => "WHOLE SONG",
            Self::Loop => "LOOP BRACE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceChoice {
    pub name: String,
    pub output_channels: u32,
    pub input_channels: u32,
    pub is_default_output: bool,
    pub preferred_rate_hz: u32,
    pub rates_hz: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSettings {
    pub backend: AudioBackend,
    pub device: Option<String>,
    pub rate_hz: Option<u32>,
    pub buffer_frames: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostRequest {
    ScanAudio(AudioBackend),
    RestartAudio(AudioSettings),
}

/// One remembered project, inspected on disk: the view lays these out
/// on the boot plate, so the facts are its to read.
#[derive(Clone, Debug)]
pub(in crate::ui::stage) struct Recent {
    pub(in crate::ui::stage) path: PathBuf,
    pub(in crate::ui::stage) title: String,
    pub(in crate::ui::stage) folder: String,
    pub(in crate::ui::stage) detail: String,
    pub(in crate::ui::stage) missing: bool,
}

impl Recent {
    fn inspect(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let title = super::document::title(&path);
        let folder = path
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "·".to_owned());
        let metadata = std::fs::metadata(&path);
        let missing = metadata.is_err();
        let detail = metadata.map_or_else(
            |_| "MISSING · ENTER TO FORGET".to_owned(),
            |meta| {
                let bytes = human_bytes(meta.len());
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|when| std::time::SystemTime::now().duration_since(when).ok())
                    .map(human_age)
                    .unwrap_or_else(|| "unknown age".to_owned());
                format!("{bytes} · {age}")
            },
        );
        Self {
            path,
            title,
            folder,
            detail,
            missing,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum ProjectOp {
    New,
    Open(PathBuf),
    Recover(PathBuf),
}

#[derive(Clone, Debug)]
pub(super) enum Confirm {
    Replace(ProjectOp),
    OverwriteProject(PathBuf),
    OverwriteExport(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    ProjectPath,
    ProjectFolder,
    UserLibrary,
    SampleFolder,
    ExportPath,
}

#[derive(Clone, Debug)]
pub(super) enum Action {
    New,
    OpenProject(PathBuf),
    Recover(PathBuf),
    Save,
    SaveAs(PathBuf),
    ForgetRecent(PathBuf),
    CleanMissing,
    ResolveConfirm(usize),
    ScanAudio,
    ApplyAudio,
    SetProjectFolder(PathBuf),
    SetUserLibrary(PathBuf),
    AddSampleFolder(PathBuf),
    RemoveSampleFolder(PathBuf),
    RescanLibrary,
    StartExport,
    CancelExport,
    CopyDiagnostics,
    OpenPage(Page),
}

/// Where the recents begin on the projects page: after the seven fixed
/// rows (new, path, open, save, save-as, boot, clean) and before any
/// recovery offer. The plate in `view::room` lays the rows out by this.
pub(in crate::ui::stage) const FIRST_RECENT_ROW: usize = 7;

#[derive(Clone, Debug)]
pub(super) struct Console {
    pub(in crate::ui::stage) page: Option<Page>,
    pub(in crate::ui::stage) startup: bool,
    pub(in crate::ui::stage) row: usize,
    pub(in crate::ui::stage) pref_page: PrefPage,
    pub(in crate::ui::stage) editing: Option<Field>,
    pub(in crate::ui::stage) path: String,
    pub(in crate::ui::stage) project_folder: String,
    pub(in crate::ui::stage) user_library: String,
    pub(in crate::ui::stage) sample_folder: String,
    pub(in crate::ui::stage) export_path: String,
    pub(in crate::ui::stage) export_range: ExportRange,
    pub(in crate::ui::stage) status: Option<String>,
    pub(in crate::ui::stage) audio_status: Option<String>,
    pub(in crate::ui::stage) audio_devices: Vec<AudioDeviceChoice>,
    pub(in crate::ui::stage) audio_devices_for: Option<AudioBackend>,
    pub(in crate::ui::stage) audio_scanning: bool,
    pub(in crate::ui::stage) host_request: Option<HostRequest>,
    pub(in crate::ui::stage) recents: Vec<Recent>,
    pub(in crate::ui::stage) confirm: Option<Confirm>,
    pub(in crate::ui::stage) confirm_row: usize,
    pub(in crate::ui::stage) recovery: Option<PathBuf>,
    /// Recovery belonging to the document currently in memory. The startup
    /// offer above may belong to a different crashed session and must never
    /// be deleted merely because this session saves another project.
    pub(in crate::ui::stage) active_recovery: Option<PathBuf>,
    pub(in crate::ui::stage) last_recovery: Option<PathBuf>,
    pub(in crate::ui::stage) last_export: Option<PathBuf>,
    pub(in crate::ui::stage) prefs: UiPrefs,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            page: None,
            startup: false,
            row: 0,
            pref_page: PrefPage::Audio,
            editing: None,
            path: String::new(),
            project_folder: String::new(),
            user_library: String::new(),
            sample_folder: String::new(),
            export_path: String::new(),
            export_range: ExportRange::Song,
            status: None,
            audio_status: None,
            audio_devices: Vec::new(),
            audio_devices_for: None,
            audio_scanning: false,
            host_request: None,
            recents: Vec::new(),
            confirm: None,
            confirm_row: 0,
            recovery: None,
            active_recovery: None,
            last_recovery: None,
            last_export: None,
            prefs: UiPrefs::default(),
        }
    }
}

impl Console {
    pub(super) fn restore(
        &mut self,
        prefs: UiPrefs,
        project_home: Option<&Path>,
        show_startup: bool,
    ) {
        self.page = None;
        self.startup = false;
        self.editing = None;
        self.confirm = None;
        self.prefs = prefs;
        self.project_folder = self
            .prefs
            .project_folder
            .clone()
            .or_else(|| project_home.map(|path| path.display().to_string()))
            .unwrap_or_default();
        self.refresh_recents();
        if show_startup && !self.prefs.skip_splash {
            self.open(Page::Projects, true);
        }
    }

    pub(super) fn prefs(&self) -> &UiPrefs {
        &self.prefs
    }

    pub(super) fn prefs_mut(&mut self) -> &mut UiPrefs {
        &mut self.prefs
    }

    pub(super) fn is_open(&self) -> bool {
        self.page.is_some()
    }

    pub(super) fn page(&self) -> Option<Page> {
        self.page
    }

    pub(super) fn open(&mut self, page: Page, startup: bool) {
        self.page = Some(page);
        self.startup = startup;
        self.row = 0;
        self.editing = None;
        self.status = None;
        self.confirm = None;
        if page == Page::Projects {
            self.refresh_recents();
        }
        if page == Page::Preferences && self.pref_page == PrefPage::Audio {
            self.request_audio_scan();
        }
    }

    pub(super) fn close(&mut self) {
        self.page = None;
        self.startup = false;
        self.editing = None;
        self.confirm = None;
    }

    /// The remembered project at `index`, chosen in one key: the plate
    /// labels each recent with a digit, so the digit does what walking
    /// the cursor there and pressing Enter would — open it, or forget it
    /// if the disk no longer has it. Only on the projects page, and only
    /// while no field is being typed into.
    pub(super) fn pick_recent(&mut self, index: usize, stage: &UtilitySnapshot) -> Option<Action> {
        if self.page != Some(Page::Projects) || self.editing.is_some() || self.confirm.is_some() {
            return None;
        }
        if index >= self.recents.len() {
            return None;
        }
        self.row = FIRST_RECENT_ROW + usize::from(self.recovery.is_some()) + index;
        self.activate(stage)
    }

    pub(super) fn remember_project(&mut self, path: &Path) {
        let entry = path.display().to_string();
        self.prefs.recent_projects.retain(|known| *known != entry);
        self.prefs.recent_projects.insert(0, entry);
        self.prefs.recent_projects.truncate(RECENT_LIMIT);
        self.refresh_recents();
    }

    fn refresh_recents(&mut self) {
        self.recents = self
            .prefs
            .recent_projects
            .iter()
            .map(PathBuf::from)
            .map(Recent::inspect)
            .collect();
    }

    pub(super) fn take_host_request(&mut self) -> Option<HostRequest> {
        self.host_request.take()
    }

    fn request_audio_scan(&mut self) {
        self.audio_scanning = true;
        self.host_request = Some(HostRequest::ScanAudio(self.prefs.audio_backend));
    }

    pub(super) fn set_audio_devices(
        &mut self,
        backend: AudioBackend,
        devices: Vec<AudioDeviceChoice>,
    ) {
        if backend != self.prefs.audio_backend {
            return;
        }
        self.audio_devices = devices;
        self.audio_devices_for = Some(backend);
        self.audio_scanning = false;
        if let Some(wanted) = &self.prefs.audio_device
            && !self
                .audio_devices
                .iter()
                .any(|device| &device.name == wanted)
        {
            self.audio_status = Some(format!("DEVICE GONE · {wanted} · DEFAULT WILL BE USED"));
        }
    }

    pub(super) fn set_audio_status(&mut self, status: impl Into<String>) {
        self.audio_status = Some(status.into());
    }

    pub(super) fn set_recovery(&mut self, path: Option<PathBuf>) {
        self.recovery = path;
    }

    pub(super) fn recovery_written(&mut self, path: PathBuf) {
        self.last_recovery = Some(path.clone());
        self.recovery = Some(path.clone());
        self.active_recovery = Some(path);
    }

    pub(super) fn export_finished(&mut self, path: &Path, result: &Result<(), String>) {
        match result {
            Ok(()) => {
                let entry = path.display().to_string();
                self.prefs.recent_exports.retain(|known| *known != entry);
                self.prefs.recent_exports.insert(0, entry);
                self.prefs.recent_exports.truncate(EXPORT_HISTORY_LIMIT);
                self.last_export = Some(path.to_path_buf());
                self.status = Some(format!("EXPORT COMPLETE · {}", path.display()));
            }
            Err(error) => self.status = Some(format!("EXPORT FAILED / CANCELLED · {error}")),
        }
    }

    pub(super) fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub(super) fn rows(&self, stage: &UtilitySnapshot) -> usize {
        match self.page {
            Some(Page::Projects) => {
                FIRST_RECENT_ROW + usize::from(self.recovery.is_some()) + self.recents.len()
            }
            Some(Page::Preferences) => match self.pref_page {
                PrefPage::Audio => 9,
                PrefPage::Projects => 7,
                PrefPage::Library => 4 + stage.library_roots.len(),
                PrefPage::Interface => 6,
            },
            Some(Page::Export) => 7 + self.prefs.recent_exports.len().min(3),
            Some(Page::Diagnostics) => 11,
            None => 0,
        }
    }

    pub(super) fn clamp_row(&mut self, stage: &UtilitySnapshot) {
        self.row = self.row.min(self.rows(stage).saturating_sub(1));
    }

    pub(super) fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::ProjectPath => &mut self.path,
            Field::ProjectFolder => &mut self.project_folder,
            Field::UserLibrary => &mut self.user_library,
            Field::SampleFolder => &mut self.sample_folder,
            Field::ExportPath => &mut self.export_path,
        }
    }

    pub(super) fn cycle_page(&mut self, delta: isize) {
        let current = self.page.unwrap_or(Page::Projects);
        self.page = Some(cycle(&Page::ALL, current, delta));
        self.row = 0;
        self.editing = None;
        if self.page == Some(Page::Preferences) && self.pref_page == PrefPage::Audio {
            self.request_audio_scan();
        }
    }

    pub(super) fn adjust(&mut self, delta: isize, stage: &UtilitySnapshot) -> Option<Action> {
        match self.page? {
            Page::Projects => {
                if self.row == 5 {
                    self.prefs.skip_splash = !self.prefs.skip_splash;
                }
            }
            Page::Preferences => match self.pref_page {
                PrefPage::Audio => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, delta);
                        self.row = 0;
                    }
                    1 => {
                        self.prefs.audio_backend = cycle(
                            &[AudioBackend::Jack, AudioBackend::Alsa, AudioBackend::Pulse],
                            self.prefs.audio_backend,
                            delta,
                        );
                        self.prefs.audio_device = None;
                        self.prefs.audio_rate_hz = None;
                        self.request_audio_scan();
                    }
                    2 => self.cycle_audio_device(delta),
                    3 => self.cycle_audio_rate(delta),
                    4 => {
                        const VALUES: [Option<u32>; 7] = [
                            None,
                            Some(64),
                            Some(128),
                            Some(256),
                            Some(512),
                            Some(1024),
                            Some(2048),
                        ];
                        self.prefs.audio_buffer_frames =
                            cycle(&VALUES, self.prefs.audio_buffer_frames, delta);
                    }
                    _ => {}
                },
                PrefPage::Projects => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, delta);
                        self.row = 0;
                    }
                    2 => self.prefs.autosave = cycle(&Autosave::ALL, self.prefs.autosave, delta),
                    3 => self.prefs.disable_backups = !self.prefs.disable_backups,
                    // Kept as a serialized compatibility row, but project
                    // replacement is never allowed to bypass the dirty
                    // interlock. Older preference files may still carry the
                    // retired bit; changing it here must not mint an unsafe
                    // path around Save / Discard / Cancel.
                    4 => self.prefs.skip_dirty_confirmation = false,
                    5 => self.prefs.skip_splash = !self.prefs.skip_splash,
                    _ => {}
                },
                PrefPage::Library => {
                    if self.row == 0 {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, delta);
                        self.row = 0;
                    }
                }
                PrefPage::Interface => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, delta);
                        self.row = 0;
                    }
                    1 => self.prefs.light_ground = !self.prefs.light_ground,
                    2 => {
                        self.prefs.density = match self.prefs.density {
                            crate::ui::tokens::Density::Comfortable => {
                                crate::ui::tokens::Density::Compact
                            }
                            crate::ui::tokens::Density::Compact => {
                                crate::ui::tokens::Density::Comfortable
                            }
                        }
                    }
                    3 => self.prefs.reduced_motion = !self.prefs.reduced_motion,
                    4 => {
                        self.prefs.cursor_energy =
                            cycle(&CursorEnergy::ALL, self.prefs.cursor_energy, delta)
                    }
                    5 => self.prefs.hide_tooltips = !self.prefs.hide_tooltips,
                    _ => {}
                },
            },
            Page::Export => match self.row {
                0 => self.export_range = cycle(&ExportRange::ALL, self.export_range, delta),
                2 => {
                    self.prefs.export_format =
                        cycle(&ExportFormat::ALL, self.prefs.export_format, delta)
                }
                3 => {
                    const RATES: [Option<u32>; 5] =
                        [None, Some(44_100), Some(48_000), Some(88_200), Some(96_000)];
                    self.prefs.export_rate_hz = cycle(&RATES, self.prefs.export_rate_hz, delta);
                }
                4 => {
                    self.prefs.export_tail = cycle(&ExportTail::ALL, self.prefs.export_tail, delta)
                }
                _ => {}
            },
            Page::Diagnostics => {}
        }
        self.clamp_row(stage);
        None
    }

    pub(super) fn activate(&mut self, stage: &UtilitySnapshot) -> Option<Action> {
        match self.page? {
            Page::Projects => match self.row {
                0 => Some(Action::New),
                1 => {
                    self.editing = Some(Field::ProjectPath);
                    None
                }
                2 => nonblank(&self.path)
                    .map(PathBuf::from)
                    .map(Action::OpenProject)
                    .or_else(|| {
                        self.status = Some("OPEN PATH IS EMPTY".to_owned());
                        None
                    }),
                3 => Some(Action::Save),
                4 => nonblank(&self.path)
                    .map(PathBuf::from)
                    .map(Action::SaveAs)
                    .or_else(|| {
                        self.status = Some("SAVE-AS PATH IS EMPTY".to_owned());
                        None
                    }),
                5 => {
                    self.prefs.skip_splash = !self.prefs.skip_splash;
                    None
                }
                6 => Some(Action::CleanMissing),
                index => {
                    let mut index = index - FIRST_RECENT_ROW;
                    if let Some(recovery) = &self.recovery {
                        if index == 0 {
                            return Some(Action::Recover(recovery.clone()));
                        }
                        index -= 1;
                    }
                    self.recents.get(index).map(|recent| {
                        if recent.missing {
                            Action::ForgetRecent(recent.path.clone())
                        } else {
                            Action::OpenProject(recent.path.clone())
                        }
                    })
                }
            },
            Page::Preferences => match self.pref_page {
                PrefPage::Audio => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, 1);
                        self.row = 0;
                        None
                    }
                    1..=4 => self.adjust(1, stage),
                    7 => Some(Action::ScanAudio),
                    8 => Some(Action::ApplyAudio),
                    _ => None,
                },
                PrefPage::Projects => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, 1);
                        self.row = 0;
                        None
                    }
                    1 => {
                        self.editing = Some(Field::ProjectFolder);
                        None
                    }
                    2..=5 => self.adjust(1, stage),
                    6 => Some(Action::CleanMissing),
                    _ => None,
                },
                PrefPage::Library => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, 1);
                        self.row = 0;
                        None
                    }
                    1 => {
                        self.editing = Some(Field::UserLibrary);
                        None
                    }
                    2 => {
                        self.editing = Some(Field::SampleFolder);
                        None
                    }
                    3 => Some(Action::RescanLibrary),
                    index => stage
                        .library_roots
                        .get(index - 4)
                        .cloned()
                        .map(Action::RemoveSampleFolder),
                },
                PrefPage::Interface => match self.row {
                    0 => {
                        self.pref_page = cycle(&PrefPage::ALL, self.pref_page, 1);
                        self.row = 0;
                        None
                    }
                    1..=5 => self.adjust(1, stage),
                    _ => None,
                },
            },
            Page::Export => match self.row {
                0 | 2 | 3 | 4 => self.adjust(1, stage),
                1 => {
                    self.editing = Some(Field::ExportPath);
                    None
                }
                5 => Some(Action::StartExport),
                6 => stage.exporting.then_some(Action::CancelExport),
                index => {
                    if let Some(path) = self.prefs.recent_exports.get(index - 7).cloned() {
                        self.export_path = path;
                    }
                    None
                }
            },
            Page::Diagnostics => match self.row {
                6 => Some(Action::CopyDiagnostics),
                7 => Some(Action::ScanAudio),
                8 => Some(Action::RescanLibrary),
                9 => Some(Action::OpenPage(Page::Preferences)),
                10 => Some(Action::OpenPage(Page::Projects)),
                _ => None,
            },
        }
    }

    fn cycle_audio_device(&mut self, delta: isize) {
        let mut names: Vec<Option<String>> = vec![None];
        names.extend(
            self.audio_devices
                .iter()
                .map(|device| Some(device.name.clone())),
        );
        self.prefs.audio_device = cycle(&names, self.prefs.audio_device.clone(), delta);
        self.prefs.audio_rate_hz = None;
    }

    fn cycle_audio_rate(&mut self, delta: isize) {
        let device = self
            .prefs
            .audio_device
            .as_ref()
            .and_then(|name| {
                self.audio_devices
                    .iter()
                    .find(|device| &device.name == name)
            })
            .or_else(|| {
                self.audio_devices
                    .iter()
                    .find(|device| device.is_default_output)
            });
        let mut rates: Vec<Option<u32>> = vec![None];
        rates.extend(
            device
                .map(|device| device.rates_hz.clone())
                .unwrap_or_else(|| vec![44_100, 48_000, 88_200, 96_000])
                .into_iter()
                .map(Some),
        );
        rates.sort_unstable();
        rates.dedup();
        self.prefs.audio_rate_hz = cycle(&rates, self.prefs.audio_rate_hz, delta);
    }
}

pub(super) fn nonblank(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn cycle<T: Clone + PartialEq>(items: &[T], current: T, delta: isize) -> T {
    let len = items.len();
    if len == 0 {
        return current;
    }
    let index = items.iter().position(|item| item == &current).unwrap_or(0);
    let next = (index as isize + delta).rem_euclid(len as isize) as usize;
    items[next].clone()
}

#[derive(Clone, Debug)]
pub(super) struct DisplayRow {
    pub(super) code: String,
    pub(super) label: String,
    pub(super) value: String,
    pub(super) enabled: bool,
    pub(super) alarm: bool,
}

impl DisplayRow {
    fn new(code: impl Into<String>, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            label: label.into(),
            value: value.into(),
            enabled: true,
            alarm: false,
        }
    }

    fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn alarm(mut self) -> Self {
        self.alarm = true;
        self
    }
}

impl Console {
    pub(super) fn prepare_export_path(&mut self, project: Option<&Path>, home: Option<&Path>) {
        if !self.export_path.trim().is_empty() {
            return;
        }
        let name = project
            .map(super::document::title)
            .unwrap_or_else(|| "untitled".to_owned());
        let root = project
            .and_then(Path::parent)
            .or(home)
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        self.export_path = root
            .join("renders")
            .join(format!("{name}-{}.wav", super::arrangement::stamp()))
            .display()
            .to_string();
    }

    pub(super) fn display_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
        match self.page {
            Some(Page::Projects) => self.project_rows(stage),
            Some(Page::Preferences) => self.preference_rows(stage),
            Some(Page::Export) => self.export_rows(stage),
            Some(Page::Diagnostics) => self.diagnostic_rows(stage),
            None => Vec::new(),
        }
    }

    fn field_value(&self, field: Field, value: &str) -> String {
        if self.editing == Some(field) {
            format!("{value}_")
        } else if value.trim().is_empty() {
            "<ENTER TO EDIT>".to_owned()
        } else {
            value.to_owned()
        }
    }

    fn project_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
        let mut rows = vec![
            DisplayRow::new("NEW", "EMPTY SONG", "SAFE START"),
            DisplayRow::new(
                "PATH",
                "PROJECT PATH",
                self.field_value(Field::ProjectPath, &self.path),
            ),
            DisplayRow::new("OPEN", "OPEN PATH", "ENTER"),
            DisplayRow::new(
                "SAVE",
                "SAVE CURRENT",
                stage
                    .project_path
                    .as_ref()
                    .map_or("NEXT UNTITLED NAME".to_owned(), |path| {
                        path.display().to_string()
                    }),
            ),
            DisplayRow::new("AS", "SAVE AS PATH", "ENTER"),
            DisplayRow::new("BOOT", "SHOW AT STARTUP", on_off(!self.prefs.skip_splash)),
            DisplayRow::new(
                "CLEAN",
                "FORGET MISSING RECENTS",
                format!(
                    "{} MISSING",
                    self.recents.iter().filter(|entry| entry.missing).count()
                ),
            ),
        ];
        if let Some(path) = &self.recovery {
            rows.push(
                DisplayRow::new("REC", "RECOVER AUTOSAVE", path.display().to_string()).alarm(),
            );
        }
        for (index, recent) in self.recents.iter().enumerate() {
            let value = format!(
                "{} · {} · {}",
                recent.folder,
                recent.detail,
                recent.path.display()
            );
            let row = DisplayRow::new(format!("R{:02}", index + 1), &recent.title, value);
            rows.push(if recent.missing { row.alarm() } else { row });
        }
        rows
    }

    fn preference_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
        let section = DisplayRow::new("PAGE", "PREFERENCE BANK", self.pref_page.label());
        match self.pref_page {
            PrefPage::Audio => {
                let device = self
                    .prefs
                    .audio_device
                    .as_deref()
                    .unwrap_or("BACKEND DEFAULT");
                let rate = self
                    .prefs
                    .audio_rate_hz
                    .map_or_else(|| "DEVICE DEFAULT".to_owned(), |hz| format!("{hz} HZ"));
                let buffer = self.prefs.audio_buffer_frames.map_or_else(
                    || "ENGINE DEFAULT".to_owned(),
                    |frames| {
                        let hz = self.prefs.audio_rate_hz.unwrap_or(48_000).max(1);
                        format!("{frames} FR · {:.2} MS", frames as f32 * 1000.0 / hz as f32)
                    },
                );
                vec![
                    section,
                    DisplayRow::new(
                        "API",
                        "BACKEND",
                        match self.prefs.audio_backend {
                            AudioBackend::Jack => "JACK",
                            AudioBackend::Alsa => "ALSA",
                            AudioBackend::Pulse => "PULSEAUDIO",
                        },
                    ),
                    DisplayRow::new("OUT", "OUTPUT DEVICE", device),
                    DisplayRow::new("RATE", "DEVICE RATE", rate),
                    DisplayRow::new("BUF", "CALLBACK BUFFER", buffer),
                    DisplayRow::new(
                        "LIVE",
                        "NEGOTIATED STREAM",
                        stage.stream.map_or_else(
                            || "NO RUNNING STREAM".to_owned(),
                            |stream| {
                                format!(
                                    "{} · {} HZ · {} FR · {}/{} IO · {}",
                                    stream.backend,
                                    stream.sample_rate,
                                    stream.buffer_frames,
                                    stream.inputs,
                                    stream.outputs,
                                    stream.latency_ms().map_or_else(
                                        || "LATENCY ?".to_owned(),
                                        |ms| format!("{ms:.2} MS")
                                    )
                                )
                            },
                        ),
                    )
                    .disabled(),
                    DisplayRow::new("DSP", "ENGINE HEALTH", health_word(stage.health.as_ref()))
                        .disabled(),
                    DisplayRow::new(
                        "SCAN",
                        "RESCAN AUDIO DEVICES",
                        if self.audio_scanning {
                            "SCANNING…".to_owned()
                        } else {
                            format!("{} FOUND", self.audio_devices.len())
                        },
                    ),
                    DisplayRow::new(
                        "APPLY",
                        "APPLY + RESTART ENGINE",
                        self.audio_status.clone().unwrap_or_else(|| {
                            stage
                                .stream
                                .map(|stream| {
                                    format!(
                                        "RUNNING {} · {} HZ · {} FR",
                                        stream.backend, stream.sample_rate, stream.buffer_frames
                                    )
                                })
                                .unwrap_or_else(|| "NO RUNNING STREAM".to_owned())
                        }),
                    ),
                ]
            }
            PrefPage::Projects => vec![
                section,
                DisplayRow::new(
                    "HOME",
                    "PROJECTS FOLDER",
                    self.field_value(Field::ProjectFolder, &self.project_folder),
                ),
                DisplayRow::new("AUTO", "RECOVERY INTERVAL", self.prefs.autosave.label()),
                DisplayRow::new(
                    "BAK",
                    "BACKUP BEFORE SAVE",
                    on_off(!self.prefs.disable_backups),
                ),
                DisplayRow::new("GUARD", "CONFIRM DIRTY REPLACE", "ALWAYS ON"),
                DisplayRow::new("BOOT", "SHOW PROJECT DECK", on_off(!self.prefs.skip_splash)),
                DisplayRow::new(
                    "CLEAN",
                    "FORGET MISSING RECENTS",
                    format!(
                        "{} MISSING",
                        self.recents.iter().filter(|entry| entry.missing).count()
                    ),
                ),
            ],
            PrefPage::Library => {
                let mut rows = vec![
                    section,
                    DisplayRow::new(
                        "USER",
                        "USER LIBRARY",
                        self.field_value(
                            Field::UserLibrary,
                            if self.user_library.is_empty() {
                                stage
                                    .library_user
                                    .as_deref()
                                    .and_then(Path::to_str)
                                    .unwrap_or("")
                            } else {
                                &self.user_library
                            },
                        ),
                    ),
                    DisplayRow::new(
                        "ADD",
                        "ADD SAMPLE ROOT",
                        self.field_value(Field::SampleFolder, &self.sample_folder),
                    ),
                    DisplayRow::new(
                        "SCAN",
                        "RESCAN LIBRARY",
                        if stage.library_scanning {
                            "SCANNING…".to_owned()
                        } else {
                            format!(
                                "{} WAV · {} SCALE · {} LENS · {} WARN",
                                stage.library_assets,
                                stage.library_scales,
                                stage.library_lenses,
                                stage.library_warnings
                            )
                        },
                    ),
                ];
                rows.extend(stage.library_roots.iter().enumerate().map(|(index, path)| {
                    DisplayRow::new(
                        format!("-{:02}", index + 1),
                        "REMOVE ROOT",
                        path.display().to_string(),
                    )
                }));
                rows
            }
            PrefPage::Interface => vec![
                section,
                DisplayRow::new(
                    "INK",
                    "DISPLAY GROUND",
                    if self.prefs.light_ground {
                        "LIGHT"
                    } else {
                        "DARK"
                    },
                ),
                DisplayRow::new(
                    "DENS",
                    "CONTROL DENSITY",
                    match self.prefs.density {
                        crate::ui::tokens::Density::Comfortable => "COMFORTABLE",
                        crate::ui::tokens::Density::Compact => "COMPACT",
                    },
                ),
                DisplayRow::new(
                    "MOVE",
                    "ANIMATED MOTION",
                    on_off(!self.prefs.reduced_motion),
                ),
                DisplayRow::new("CUR", "CURSOR ENERGY", self.prefs.cursor_energy.label()),
                DisplayRow::new("TIP", "TOOLTIPS", on_off(!self.prefs.hide_tooltips)),
            ],
        }
    }

    fn export_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
        let metrics = export_metrics(self, stage);
        let range = match self.export_range {
            ExportRange::Song => (0, stage.song_end_tick),
            ExportRange::Loop => stage.loop_region.unwrap_or((0, 0)),
        };
        let mut rows = vec![
            DisplayRow::new(
                "RANGE",
                "RENDER RANGE",
                format!(
                    "{} · TICK {} → {}",
                    self.export_range.label(),
                    range.0,
                    range.1
                ),
            ),
            DisplayRow::new(
                "PATH",
                "DESTINATION",
                self.field_value(Field::ExportPath, &self.export_path),
            ),
            DisplayRow::new("FMT", "WAV ENCODING", self.prefs.export_format.label()),
            DisplayRow::new(
                "RATE",
                "OUTPUT RATE",
                self.prefs
                    .export_rate_hz
                    .map_or_else(|| "RUNNING DEVICE".to_owned(), |rate| format!("{rate} HZ")),
            ),
            DisplayRow::new("TAIL", "EFFECT TAIL", self.prefs.export_tail.label()),
            if metrics.valid && !stage.exporting {
                DisplayRow::new(
                    "GO",
                    "START OFFLINE RENDER",
                    format!(
                        "{:.2} S · {} · {}",
                        metrics.seconds,
                        metrics.frames,
                        human_bytes(metrics.bytes)
                    ),
                )
            } else {
                DisplayRow::new("GO", "START OFFLINE RENDER", metrics.reason).disabled()
            },
            if stage.exporting {
                DisplayRow::new(
                    "STOP",
                    "CANCEL ACTIVE RENDER",
                    format!(
                        "{:03}% · ESC",
                        (stage.export_progress.unwrap_or(0.0) * 100.0).round() as u32
                    ),
                )
                .alarm()
            } else {
                DisplayRow::new("STOP", "CANCEL ACTIVE RENDER", "IDLE").disabled()
            },
        ];
        rows.extend(
            self.prefs
                .recent_exports
                .iter()
                .take(3)
                .enumerate()
                .map(|(index, path)| {
                    DisplayRow::new(format!("E{:02}", index + 1), "RECENT EXPORT", path)
                }),
        );
        rows
    }

    fn diagnostic_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
        vec![
            DisplayRow::new(
                "PROJ",
                "CURRENT PROJECT",
                stage
                    .project_path
                    .as_ref()
                    .map_or_else(|| "UNTITLED".to_owned(), |path| path.display().to_string()),
            )
            .disabled(),
            DisplayRow::new(
                "DOC",
                "DOCUMENT STATE",
                format!(
                    "{} · {} TRACK · {} PATTERN · {} DEVICE",
                    if stage.dirty { "DIRTY" } else { "SAVED" },
                    stage.track_count,
                    stage.pattern_count,
                    stage.device_count
                ),
            )
            .disabled(),
            DisplayRow::new(
                "LIVE",
                "AUDIO STREAM",
                stage.stream.map_or_else(
                    || "ABSENT".to_owned(),
                    |stream| {
                        format!(
                            "{} · {} HZ · {} FR · {}/{} IO",
                            stream.backend,
                            stream.sample_rate,
                            stream.buffer_frames,
                            stream.inputs,
                            stream.outputs
                        )
                    },
                ),
            )
            .disabled(),
            DisplayRow::new("DSP", "ENGINE HEALTH", health_word(stage.health.as_ref())).disabled(),
            DisplayRow::new(
                "REC",
                "RECOVERY COPY",
                self.last_recovery
                    .as_deref()
                    .or(self.recovery.as_deref())
                    .map_or_else(|| "NONE".to_owned(), |path| path.display().to_string()),
            )
            .disabled(),
            DisplayRow::new(
                "LAST",
                "LAST EXPORT",
                self.last_export
                    .as_deref()
                    .map_or_else(|| "NONE".to_owned(), |path| path.display().to_string()),
            )
            .disabled(),
            DisplayRow::new("COPY", "COPY DIAGNOSTICS REPORT", "CLIPBOARD"),
            DisplayRow::new(
                "AUDIO",
                "RESCAN AUDIO DEVICES",
                if self.audio_scanning {
                    "SCANNING…"
                } else {
                    "READY"
                },
            ),
            DisplayRow::new(
                "LIB",
                "RESCAN LIBRARY",
                format!(
                    "GEN {} · {} ASSETS",
                    stage.library_generation, stage.library_assets
                ),
            ),
            DisplayRow::new("PREF", "OPEN PREFERENCES", "CTRL+,"),
            DisplayRow::new("PROJ", "OPEN PROJECT DECK", "CTRL+O"),
        ]
    }
}

#[derive(Clone, Debug)]
struct ExportMetrics {
    valid: bool,
    reason: String,
    seconds: f64,
    frames: u64,
    bytes: u64,
}

fn export_metrics(console: &Console, stage: &UtilitySnapshot) -> ExportMetrics {
    let range = match console.export_range {
        ExportRange::Song => (0, stage.song_end_tick),
        ExportRange::Loop => stage.loop_region.unwrap_or((0, 0)),
    };
    if range.1 <= range.0 {
        return ExportMetrics {
            valid: false,
            reason: if console.export_range == ExportRange::Loop {
                "NO ACTIVE LOOP BRACE".to_owned()
            } else {
                "SONG HAS NO ARRANGEMENT".to_owned()
            },
            seconds: 0.0,
            frames: 0,
            bytes: 0,
        };
    }
    if console.export_path.trim().is_empty() {
        return ExportMetrics {
            valid: false,
            reason: "DESTINATION IS EMPTY".to_owned(),
            seconds: 0.0,
            frames: 0,
            bytes: 0,
        };
    }
    let beats = (range.1 - range.0) as f64 / crate::sequencing::TICKS_PER_BEAT as f64;
    let seconds =
        beats * 60.0 / stage.bpm.max(1.0) + f64::from(console.prefs.export_tail.seconds());
    let rate = console
        .prefs
        .export_rate_hz
        .or_else(|| stage.stream.map(|stream| stream.sample_rate))
        .unwrap_or(48_000);
    let frames = (seconds * f64::from(rate)).ceil() as u64;
    ExportMetrics {
        valid: true,
        reason: String::new(),
        seconds,
        frames,
        bytes: frames
            .saturating_mul(console.prefs.export_format.bytes_per_stereo_frame())
            .saturating_add(44),
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

fn health_word(health: Option<&super::Health>) -> String {
    let Some(health) = health else {
        return "UNREPORTED".to_owned();
    };
    match &health.state {
        EngineState::Absent => "ABSENT".to_owned(),
        EngineState::Running => {
            format!(
                "RUNNING · {:.1}% LOAD · {} XRUN",
                health.load * 100.0,
                health.xruns
            )
        }
        EngineState::Stalled { seconds } => format!("STALLED · {seconds:.1} S"),
        EngineState::Errored(error) => format!("ERROR · {error}"),
    }
}

impl Stage {
    pub fn restore_preferences(&mut self, prefs: UiPrefs, show_startup: bool) {
        if let Some(folder) = prefs.project_folder.as_deref().map(PathBuf::from) {
            self.home = Some(folder);
        }
        self.polarity = if prefs.light_ground {
            design::Polarity::Light
        } else {
            design::Polarity::Dark
        };
        let home = self.home.clone();
        self.utility.restore(prefs, home.as_deref(), show_startup);
        self.utility.set_recovery(newest_recovery(home.as_deref()));
        // The project deck replaces the old command-palette launch state;
        // skipping it means entering the workspace, not seeing a different
        // modal by surprise.
        self.palette.close();
    }

    pub fn preferences(&self) -> &UiPrefs {
        self.utility.prefs()
    }

    pub fn library_preferences(&self) -> &crate::library::LibraryConfig {
        &self.library_config
    }

    pub fn library_cache(&self) -> &crate::library::LibrarySnapshot {
        &self.library_snapshot
    }

    pub fn utility_page(&self) -> Option<Page> {
        self.utility.page()
    }

    pub fn open_utility(&mut self, page: Page) {
        if page == Page::Export {
            self.utility
                .prepare_export_path(self.path.as_deref(), self.home.as_deref());
        }
        self.palette.close();
        self.utility.open(page, false);
    }

    pub fn take_utility_host_request(&mut self) -> Option<HostRequest> {
        self.utility.take_host_request()
    }

    pub fn set_audio_devices(&mut self, backend: AudioBackend, devices: Vec<AudioDeviceChoice>) {
        self.utility.set_audio_devices(backend, devices);
    }

    pub fn set_audio_preferences_status(&mut self, status: impl Into<String>) {
        self.utility.set_audio_status(status);
    }

    pub fn autosave_period(&self) -> Option<std::time::Duration> {
        self.utility
            .prefs()
            .autosave
            .minutes()
            .map(|minutes| std::time::Duration::from_secs(minutes * 60))
    }

    /// Write one recoverable copy. The named project is never touched; a
    /// recovery is another document under the machine's project home.
    pub fn write_recovery(&mut self) -> Result<Option<PathBuf>, String> {
        if !self.dirty {
            return Ok(None);
        }
        let Some(home) = self.home.as_deref() else {
            return Ok(None);
        };
        let path = recovery_path(home, self.path.as_deref());
        super::document::save(&path, &self.song)?;
        self.utility.recovery_written(path.clone());
        self.notice = Some(format!("recovery saved → {}", path.display()));
        Ok(Some(path))
    }

    pub(super) fn utility_snapshot(&self) -> UtilitySnapshot {
        UtilitySnapshot {
            project_path: self.path.clone(),
            dirty: self.dirty,
            track_count: self.song.tracks.len(),
            pattern_count: self.song.patterns.len(),
            device_count: self
                .song
                .tracks
                .iter()
                .map(|track| usize::from(track.machine.is_some()) + track.strip.len())
                .sum(),
            bpm: self.bpm(),
            song_end_tick: self.song.end_tick(),
            loop_region: self.song.loop_on.then_some(self.song.loop_brace).flatten(),
            stream: self.vitals.stream().copied(),
            health: self.vitals.health().cloned(),
            library_scanning: self.library_scanning,
            library_generation: self.library_snapshot.generation,
            library_assets: self.library_snapshot.assets.len(),
            library_scales: self.library_snapshot.scales.len(),
            library_lenses: self.library_snapshot.lenses.len(),
            library_warnings: self.library_snapshot.warnings.len(),
            library_user: self.library_config.user_library.clone(),
            library_roots: self.library_config.sample_folders.clone(),
            exporting: self.export.is_some(),
            export_progress: self.export.as_ref().map(|export| export.progress),
        }
    }

    /// Apply one of the machine room's actions. Returns the one thing an
    /// action can ask for that only a host can do: words to put on the
    /// clipboard.
    pub(super) fn apply_utility(&mut self, action: Action) -> Option<String> {
        let mut clipboard = None;
        match action {
            Action::New => self.request_project_op(ProjectOp::New),
            Action::OpenProject(path) => self.request_project_op(ProjectOp::Open(path)),
            Action::Recover(path) => self.request_project_op(ProjectOp::Recover(path)),
            Action::Save => match self.save() {
                Ok(()) => self.utility.set_status("PROJECT SAVED"),
                Err(error) => self.utility.set_status(format!("SAVE FAILED · {error}")),
            },
            Action::SaveAs(path) => {
                let path = super::document::with_extension(&path);
                if path.exists() {
                    self.utility.confirm = Some(Confirm::OverwriteProject(path));
                    self.utility.confirm_row = 2;
                } else {
                    self.save_as_project(path);
                }
            }
            Action::ForgetRecent(path) => {
                let path = path.display().to_string();
                self.utility
                    .prefs_mut()
                    .recent_projects
                    .retain(|known| *known != path);
                self.utility.refresh_recents();
                self.utility.set_status("RECENT ENTRY FORGOTTEN");
            }
            Action::CleanMissing => {
                self.utility
                    .prefs_mut()
                    .recent_projects
                    .retain(|path| Path::new(path).is_file());
                self.utility.refresh_recents();
                self.utility.set_status("MISSING RECENTS CLEARED");
            }
            Action::ResolveConfirm(choice) => self.resolve_confirm(choice),
            Action::ScanAudio => self.utility.request_audio_scan(),
            Action::ApplyAudio => {
                let prefs = self.utility.prefs();
                let settings = AudioSettings {
                    backend: prefs.audio_backend,
                    device: prefs.audio_device.clone(),
                    rate_hz: prefs.audio_rate_hz,
                    buffer_frames: prefs.audio_buffer_frames,
                };
                self.utility.host_request = Some(HostRequest::RestartAudio(settings));
                self.utility.set_audio_status("RESTARTING…");
            }
            Action::SetProjectFolder(path) => match std::fs::create_dir_all(&path) {
                Ok(()) => {
                    let path = path.canonicalize().unwrap_or(path);
                    self.home = Some(path.clone());
                    self.utility.prefs_mut().project_folder = Some(path.display().to_string());
                    self.utility.set_recovery(newest_recovery(Some(&path)));
                    self.utility.set_status("PROJECT FOLDER SET");
                }
                Err(error) => self
                    .utility
                    .set_status(format!("PROJECT FOLDER REFUSED · {error}")),
            },
            Action::SetUserLibrary(path) => match self.library_config.set_user_library(&path) {
                Ok(()) => {
                    self.rescan_library();
                    self.utility.set_status("USER LIBRARY SET");
                }
                Err(error) => self
                    .utility
                    .set_status(format!("LIBRARY REFUSED · {error}")),
            },
            Action::AddSampleFolder(path) => match self.library_config.add_sample_folder(&path) {
                Ok(()) => {
                    self.utility.sample_folder.clear();
                    self.rescan_library();
                    self.utility.set_status("SAMPLE ROOT ADDED");
                }
                Err(error) => self.utility.set_status(format!("ROOT REFUSED · {error}")),
            },
            Action::RemoveSampleFolder(path) => {
                if self.library_config.remove_sample_folder(&path) {
                    self.rescan_library();
                    self.utility.set_status("SAMPLE ROOT REMOVED");
                }
            }
            Action::RescanLibrary => {
                self.rescan_library();
                self.utility.set_status("LIBRARY SCAN REQUESTED");
            }
            Action::StartExport => self.start_utility_export(),
            Action::CancelExport => {
                if self.abandon_export().is_ok() {
                    self.utility.set_status("EXPORT CANCELLING…");
                }
            }
            Action::CopyDiagnostics => {
                clipboard = Some(self.diagnostics_report());
                self.utility.set_status("DIAGNOSTICS COPIED");
            }
            Action::OpenPage(page) => self.open_utility(page),
        }
        clipboard
    }

    fn request_project_op(&mut self, op: ProjectOp) {
        if self.dirty {
            self.utility.confirm = Some(Confirm::Replace(op));
            self.utility.confirm_row = 0;
        } else {
            self.perform_project_op(op);
        }
    }

    fn perform_project_op(&mut self, op: ProjectOp) {
        let replacing = matches!(&op, ProjectOp::New | ProjectOp::Open(_));
        let result = match op {
            ProjectOp::New => {
                self.new_project();
                Ok(())
            }
            ProjectOp::Open(path) => self.open(path),
            ProjectOp::Recover(path) => self.recover(path),
        };
        match result {
            Ok(()) => {
                if replacing {
                    self.remove_recovery();
                }
                self.utility.close();
            }
            Err(error) => self
                .utility
                .set_status(format!("PROJECT REFUSED · {error}")),
        }
    }

    fn resolve_confirm(&mut self, choice: usize) {
        let Some(confirm) = self.utility.confirm.take() else {
            return;
        };
        match confirm {
            Confirm::Replace(op) => match choice {
                0 => match self.save() {
                    Ok(()) => self.perform_project_op(op),
                    Err(error) => self.utility.set_status(format!("SAVE FAILED · {error}")),
                },
                1 => self.perform_project_op(op),
                _ => self.utility.set_status("PROJECT CHANGE CANCELLED"),
            },
            Confirm::OverwriteProject(path) => match choice {
                0 => self.save_as_project(path),
                1 => self.save_as_project(super::document::available_path(&path)),
                _ => self.utility.set_status("SAVE-AS CANCELLED"),
            },
            Confirm::OverwriteExport(path) => match choice {
                0 => self.queue_utility_export(path),
                1 => {
                    let path = super::document::available_path(&path);
                    self.utility.export_path = path.display().to_string();
                    self.queue_utility_export(path);
                }
                _ => self.utility.set_status("EXPORT CANCELLED"),
            },
        }
    }

    fn new_project(&mut self) {
        self.remove_recovery();
        self.replace_song(crate::sequencing::Song::default());
        self.history = crate::history::History::new(self.song.clone());
        self.path = None;
        self.dirty = false;
        self.notice = Some("new project".to_owned());
    }

    fn recover(&mut self, path: PathBuf) -> Result<(), String> {
        let song = super::document::load(&path)?;
        self.replace_song(song);
        self.history = crate::history::History::new(self.song.clone());
        self.path = None;
        self.dirty = true;
        self.notice = Some(format!("recovered {} · save to keep", path.display()));
        self.utility.set_recovery(Some(path.clone()));
        self.utility.active_recovery = Some(path);
        Ok(())
    }

    fn save_as_project(&mut self, path: PathBuf) {
        match self.save_as(path) {
            Ok(()) => self.utility.set_status("PROJECT SAVED AS"),
            Err(error) => self.utility.set_status(format!("SAVE-AS FAILED · {error}")),
        }
    }

    fn rescan_library(&mut self) {
        self.library_service.rescan(self.library_config.clone());
        self.library_scanning = true;
        self.library_generation = self.library_generation.wrapping_add(1);
    }

    fn start_utility_export(&mut self) {
        let Some(path) = nonblank(&self.utility.export_path).map(PathBuf::from) else {
            self.utility.set_status("EXPORT DESTINATION IS EMPTY");
            return;
        };
        let path = with_wav_extension(&path);
        if path.exists() {
            self.utility.confirm = Some(Confirm::OverwriteExport(path));
            self.utility.confirm_row = 2;
        } else {
            self.queue_utility_export(path);
        }
    }

    fn queue_utility_export(&mut self, path: PathBuf) {
        let range = match self.utility.export_range {
            ExportRange::Song => (0, self.song.end_tick()),
            ExportRange::Loop => self.loop_region().unwrap_or((0, 0)),
        };
        if range.1 <= range.0 {
            self.utility
                .set_status(if self.utility.export_range == ExportRange::Loop {
                    "EXPORT REFUSED · NO ACTIVE LOOP BRACE"
                } else {
                    "EXPORT REFUSED · SONG HAS NO ARRANGEMENT"
                });
            return;
        }
        let format = self.utility.prefs().export_format;
        let rate_hz = self.utility.prefs().export_rate_hz;
        let tail_seconds = self.utility.prefs().export_tail.seconds();
        match self.request_export_to(
            range.0,
            range.1,
            path.clone(),
            format,
            rate_hz,
            tail_seconds,
        ) {
            Ok(()) => {
                self.utility.export_path = path.display().to_string();
                self.utility.set_status("EXPORT QUEUED · ESC TO CANCEL");
            }
            Err(_) => self.utility.set_status("EXPORT REFUSED"),
        }
    }

    fn diagnostics_report(&self) -> String {
        let snapshot = self.utility_snapshot();
        let project = snapshot
            .project_path
            .as_deref()
            .map_or_else(|| "unsaved".to_owned(), |path| path.display().to_string());
        let stream = snapshot.stream.map_or_else(
            || "absent".to_owned(),
            |stream| {
                format!(
                    "{} {}Hz {}fr {}in/{}out latency={}",
                    stream.backend,
                    stream.sample_rate,
                    stream.buffer_frames,
                    stream.inputs,
                    stream.outputs,
                    stream
                        .latency_ms()
                        .map_or_else(|| "unknown".to_owned(), |ms| format!("{ms:.2}ms"))
                )
            },
        );
        let health = snapshot.health.map_or_else(
            || "unreported".to_owned(),
            |health| match health.state {
                EngineState::Absent => "absent".to_owned(),
                EngineState::Running => {
                    format!(
                        "running load={:.1}% xruns={}",
                        health.load * 100.0,
                        health.xruns
                    )
                }
                EngineState::Stalled { seconds } => format!("stalled {seconds:.1}s"),
                EngineState::Errored(error) => format!("error {error}"),
            },
        );
        format!(
            "daw stage {}\nproject: {}{}\nsong: {} tracks, {} patterns, {} devices, {:.2} bpm\naudio: {}\nhealth: {}\nlibrary: generation {}, {} assets, {} scales, {} lenses, {} warnings\nrecovery: {}\nlast export: {}",
            env!("CARGO_PKG_VERSION"),
            project,
            if snapshot.dirty { " (dirty)" } else { "" },
            snapshot.track_count,
            snapshot.pattern_count,
            snapshot.device_count,
            snapshot.bpm,
            stream,
            health,
            snapshot.library_generation,
            snapshot.library_assets,
            snapshot.library_scales,
            snapshot.library_lenses,
            snapshot.library_warnings,
            self.utility
                .last_recovery
                .as_deref()
                .or(self.utility.recovery.as_deref())
                .map_or_else(|| "none".to_owned(), |path| path.display().to_string()),
            self.utility
                .last_export
                .as_deref()
                .map_or_else(|| "none".to_owned(), |path| path.display().to_string()),
        )
    }

    pub(super) fn remove_recovery(&mut self) {
        if let Some(path) = self.utility.active_recovery.take() {
            let _ = std::fs::remove_file(&path);
            if self.utility.recovery.as_ref() == Some(&path) {
                self.utility.recovery = None;
            }
        }
    }
}

fn recovery_path(home: &Path, project: Option<&Path>) -> PathBuf {
    let name = project
        .map(super::document::title)
        .unwrap_or_else(|| "untitled".to_owned());
    home.join("recovery")
        .join(format!("{name}.recovery.stage.ron"))
}

fn newest_recovery(home: Option<&Path>) -> Option<PathBuf> {
    let dir = home?.join("recovery");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn with_wav_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        path.to_path_buf()
    } else {
        let mut path = path.to_path_buf();
        path.set_extension("wav");
        path
    }
}
#[derive(Clone, Debug)]
pub(super) struct UtilitySnapshot {
    pub project_path: Option<PathBuf>,
    pub dirty: bool,
    pub track_count: usize,
    pub pattern_count: usize,
    pub device_count: usize,
    pub bpm: f64,
    pub song_end_tick: usize,
    pub loop_region: Option<(usize, usize)>,
    pub stream: Option<super::Stream>,
    pub health: Option<super::Health>,
    pub library_scanning: bool,
    pub library_generation: u64,
    pub library_assets: usize,
    pub library_scales: usize,
    pub library_lenses: usize,
    pub library_warnings: usize,
    pub library_user: Option<PathBuf>,
    pub library_roots: Vec<PathBuf>,
    pub exporting: bool,
    pub export_progress: Option<f32>,
}

fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn human_age(age: std::time::Duration) -> String {
    let secs = age.as_secs();
    if secs < 60 {
        "just now".to_owned()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86_400 {
        format!("{} h ago", secs / 3600)
    } else {
        format!("{} d ago", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> UtilitySnapshot {
        UtilitySnapshot {
            project_path: None,
            dirty: false,
            track_count: 2,
            pattern_count: 3,
            device_count: 4,
            bpm: 120.0,
            song_end_tick: crate::sequencing::TICKS_PER_BEAT * 8,
            loop_region: Some((
                crate::sequencing::TICKS_PER_BEAT,
                crate::sequencing::TICKS_PER_BEAT * 5,
            )),
            stream: None,
            health: None,
            library_scanning: false,
            library_generation: 0,
            library_assets: 0,
            library_scales: 0,
            library_lenses: 0,
            library_warnings: 0,
            library_user: None,
            library_roots: Vec::new(),
            exporting: false,
            export_progress: None,
        }
    }

    #[test]
    fn finite_utility_values_wrap_both_ways() {
        let values = [1, 2, 3];
        assert_eq!(cycle(&values, 3, 1), 1);
        assert_eq!(cycle(&values, 1, -1), 3);
        assert_eq!(cycle(&values, 99, 1), 2);
    }

    #[test]
    fn export_estimate_includes_encoding_rate_and_tail() {
        let mut console = Console::default();
        console.export_path = "/tmp/mix.wav".to_owned();
        console.prefs.export_format = ExportFormat::Int16;
        console.prefs.export_rate_hz = Some(96_000);
        console.prefs.export_tail = ExportTail::TwoSeconds;
        let metrics = export_metrics(&console, &snapshot());
        assert!(metrics.valid);
        assert!((metrics.seconds - 6.0).abs() < f64::EPSILON);
        assert_eq!(metrics.frames, 576_000);
        assert_eq!(metrics.bytes, 576_000 * 4 + 44);
    }

    #[test]
    fn startup_deck_obeys_its_machine_preference() {
        let mut console = Console::default();
        console.restore(UiPrefs::default(), None, true);
        assert_eq!(console.page(), Some(Page::Projects));

        let prefs = UiPrefs {
            skip_splash: true,
            ..UiPrefs::default()
        };
        console.restore(prefs, None, true);
        assert_eq!(console.page(), None);
    }

    #[test]
    fn a_digit_picks_the_recent_it_labels() {
        let mut console = Console::default();
        console.prefs.recent_projects = vec![
            "/nowhere/first.stage.ron".to_owned(),
            "/nowhere/second.stage.ron".to_owned(),
        ];
        console.restore(console.prefs.clone(), None, true);
        assert_eq!(console.page(), Some(Page::Projects));

        // Neither file exists, so the digit's act is to forget it — the
        // same act Enter performs on that row.
        let picked = console.pick_recent(1, &snapshot());
        assert!(
            matches!(&picked, Some(Action::ForgetRecent(path)) if path == Path::new("/nowhere/second.stage.ron")),
            "{picked:?}"
        );
        assert_eq!(console.row, FIRST_RECENT_ROW + 1);

        // Past the end, nothing; while typing a path, nothing.
        assert!(console.pick_recent(5, &snapshot()).is_none());
        console.editing = Some(Field::ProjectPath);
        assert!(console.pick_recent(0, &snapshot()).is_none());
    }

    #[test]
    fn recents_are_deduplicated_newest_first_and_bounded() {
        let mut console = Console::default();
        for index in 0..RECENT_LIMIT + 4 {
            console.remember_project(Path::new(&format!("/tmp/song-{index}.stage.ron")));
        }
        console.remember_project(Path::new("/tmp/song-10.stage.ron"));
        assert_eq!(console.prefs.recent_projects.len(), RECENT_LIMIT);
        assert_eq!(console.prefs.recent_projects[0], "/tmp/song-10.stage.ron");
        assert_eq!(
            console
                .prefs
                .recent_projects
                .iter()
                .filter(|path| *path == "/tmp/song-10.stage.ron")
                .count(),
            1
        );
    }

    #[test]
    fn audio_requests_preserve_every_selected_setting() {
        let mut stage = Stage::new();
        stage.utility.prefs.audio_backend = AudioBackend::Alsa;
        stage.utility.prefs.audio_device = Some("Deck I/O".to_owned());
        stage.utility.prefs.audio_rate_hz = Some(96_000);
        stage.utility.prefs.audio_buffer_frames = Some(128);

        stage.apply_utility(Action::ApplyAudio);

        assert_eq!(
            stage.take_utility_host_request(),
            Some(HostRequest::RestartAudio(AudioSettings {
                backend: AudioBackend::Alsa,
                device: Some("Deck I/O".to_owned()),
                rate_hz: Some(96_000),
                buffer_frames: Some(128),
            }))
        );
    }

    #[test]
    fn dirty_project_changes_wait_for_an_explicit_resolution() {
        let mut stage = Stage::new();
        stage.dirty = true;
        let before = stage.song.clone();

        stage.request_project_op(ProjectOp::New);

        assert!(matches!(
            stage.utility.confirm,
            Some(Confirm::Replace(ProjectOp::New))
        ));
        assert_eq!(stage.song, before, "the guarded project was replaced early");
    }

    #[test]
    fn legacy_skip_guard_preference_cannot_bypass_dirty_replacement() {
        let mut stage = Stage::new();
        stage.dirty = true;
        stage.utility.prefs.skip_dirty_confirmation = true;
        let before = stage.song.clone();

        stage.request_project_op(ProjectOp::New);

        assert!(matches!(
            stage.utility.confirm,
            Some(Confirm::Replace(ProjectOp::New))
        ));
        assert_eq!(
            stage.song, before,
            "the legacy guard bit replaced a dirty song"
        );
    }

    #[test]
    fn export_request_carries_the_utility_render_contract() {
        let mut stage = Stage::new();
        let end = crate::sequencing::TICKS_PER_BEAT * 8;
        stage
            .request_export_to(
                crate::sequencing::TICKS_PER_BEAT,
                end,
                PathBuf::from("/tmp/orbit.wav"),
                ExportFormat::Float32,
                Some(96_000),
                5,
            )
            .expect("valid render range");

        let request = stage.take_export().expect("host request");
        assert_eq!(request.start_tick, crate::sequencing::TICKS_PER_BEAT);
        assert_eq!(request.end_tick, end);
        assert_eq!(request.format, ExportFormat::Float32);
        assert_eq!(request.rate_hz, Some(96_000));
        assert_eq!(request.tail_seconds, 5);
    }

    #[test]
    fn export_history_keeps_files_and_reports_failures_without_inventing_one() {
        let mut console = Console::default();
        let finished = Path::new("/tmp/orbit.wav");
        console.export_finished(finished, &Ok(()));
        assert_eq!(console.prefs.recent_exports, vec!["/tmp/orbit.wav"]);
        assert_eq!(console.last_export.as_deref(), Some(finished));

        console.export_finished(Path::new("/tmp/partial.wav"), &Err("cancelled".to_owned()));
        assert_eq!(console.prefs.recent_exports, vec!["/tmp/orbit.wav"]);
        assert!(
            console
                .status
                .as_deref()
                .is_some_and(|status| status.contains("cancelled"))
        );
    }

    #[test]
    fn recovery_names_are_stable_per_project() {
        let home = Path::new("/tmp/deck");
        assert_eq!(
            recovery_path(home, Some(Path::new("/music/orbit.stage.ron"))),
            home.join("recovery/orbit.recovery.stage.ron")
        );
        assert_eq!(
            recovery_path(home, None),
            home.join("recovery/untitled.recovery.stage.ron")
        );
    }
}
