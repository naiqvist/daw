//! The stage's machine room: projects, preferences, export and diagnostics.
//!
//! This is one modal state machine rather than four unrelated windows.  It
//! owns the keyboard while visible, emits bounded requests at the host seam,
//! and paints in the same display language as the musical surface.

use super::{EngineState, Stage};
use crate::design::kit::Weight;
use crate::design::{self, circuit};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::prefs::{AudioBackend, Autosave, CursorEnergy, ExportFormat, ExportTail, UiPrefs};
use eframe::egui;
use std::path::{Path, PathBuf};

const PANEL_MAX_W: f32 = 1080.0;
const PANEL_MAX_H: f32 = 680.0;
const NAV_W: f32 = 172.0;
const HEADER_H: f32 = 72.0;
const FOOTER_H: f32 = 34.0;
const ROW_H: f32 = 38.0;
const GAP: f32 = 6.0;
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
    const ALL: [Self; 4] = [
        Self::Projects,
        Self::Preferences,
        Self::Export,
        Self::Diagnostics,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Projects => "PROJECTS",
            Self::Preferences => "PREFERENCES",
            Self::Export => "EXPORT",
            Self::Diagnostics => "SYSTEM",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PrefPage {
    Audio,
    Projects,
    Library,
    Interface,
}

impl PrefPage {
    const ALL: [Self; 4] = [Self::Audio, Self::Projects, Self::Library, Self::Interface];

    const fn label(self) -> &'static str {
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
    const ALL: [Self; 2] = [Self::Song, Self::Loop];

    const fn label(self) -> &'static str {
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

#[derive(Clone, Debug)]
struct Recent {
    path: PathBuf,
    title: String,
    folder: String,
    detail: String,
    missing: bool,
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
enum ProjectOp {
    New,
    Open(PathBuf),
    Recover(PathBuf),
}

#[derive(Clone, Debug)]
enum Confirm {
    Replace(ProjectOp),
    OverwriteProject(PathBuf),
    OverwriteExport(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
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

#[derive(Clone, Debug)]
pub(super) struct Console {
    page: Option<Page>,
    startup: bool,
    row: usize,
    pref_page: PrefPage,
    editing: Option<Field>,
    path: String,
    project_folder: String,
    user_library: String,
    sample_folder: String,
    export_path: String,
    export_range: ExportRange,
    status: Option<String>,
    audio_status: Option<String>,
    audio_devices: Vec<AudioDeviceChoice>,
    audio_devices_for: Option<AudioBackend>,
    audio_scanning: bool,
    host_request: Option<HostRequest>,
    recents: Vec<Recent>,
    confirm: Option<Confirm>,
    confirm_row: usize,
    recovery: Option<PathBuf>,
    /// Recovery belonging to the document currently in memory. The startup
    /// offer above may belong to a different crashed session and must never
    /// be deleted merely because this session saves another project.
    active_recovery: Option<PathBuf>,
    last_recovery: Option<PathBuf>,
    last_export: Option<PathBuf>,
    prefs: UiPrefs,
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

    fn close(&mut self) {
        self.page = None;
        self.startup = false;
        self.editing = None;
        self.confirm = None;
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

    fn rows(&self, stage: &UtilitySnapshot) -> usize {
        match self.page {
            Some(Page::Projects) => 7 + usize::from(self.recovery.is_some()) + self.recents.len(),
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

    fn clamp_row(&mut self, stage: &UtilitySnapshot) {
        self.row = self.row.min(self.rows(stage).saturating_sub(1));
    }

    pub(super) fn consume_input(
        &mut self,
        ctx: &egui::Context,
        stage: &UtilitySnapshot,
    ) -> Option<Action> {
        if self.page.is_none() {
            return None;
        }

        if self.confirm.is_some() {
            let moved_back = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowUp)
                || key(ctx, egui::Modifiers::NONE, egui::Key::K)
                || key(ctx, egui::Modifiers::SHIFT, egui::Key::Tab);
            let moved = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowDown)
                || key(ctx, egui::Modifiers::NONE, egui::Key::J)
                || key(ctx, egui::Modifiers::NONE, egui::Key::Tab);
            if moved {
                self.confirm_row = (self.confirm_row + 1) % 3;
            }
            if moved_back {
                self.confirm_row = (self.confirm_row + 2) % 3;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
                self.confirm = None;
                return None;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
                return Some(Action::ResolveConfirm(self.confirm_row));
            }
            return None;
        }

        // The four doors stay global even from inside the machine room. A
        // nested overwrite/dirty decision above is the sole exception: it
        // must be answered or cancelled before navigation can continue.
        let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        if key(ctx, egui::Modifiers::COMMAND, egui::Key::O) {
            return Some(Action::OpenPage(Page::Projects));
        }
        if key(ctx, egui::Modifiers::COMMAND, egui::Key::Comma) {
            return Some(Action::OpenPage(Page::Preferences));
        }
        if key(ctx, command_shift, egui::Key::E) {
            return Some(Action::OpenPage(Page::Export));
        }
        if key(ctx, command_shift, egui::Key::D) {
            return Some(Action::OpenPage(Page::Diagnostics));
        }
        if self.editing.is_none() && key(ctx, egui::Modifiers::COMMAND, egui::Key::S) {
            return Some(Action::Save);
        }

        if let Some(field) = self.editing {
            if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
                self.editing = None;
                return None;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Backspace) {
                self.field_mut(field).pop();
            }
            if key(ctx, egui::Modifiers::COMMAND, egui::Key::A) {
                self.field_mut(field).clear();
            }
            let additions: Vec<String> = ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Text(text) | egui::Event::Paste(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect()
            });
            for text in additions {
                self.field_mut(field)
                    .extend(text.chars().filter(|ch| !ch.is_control()));
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
                self.editing = None;
                return match field {
                    Field::ProjectFolder => nonblank(&self.project_folder)
                        .map(PathBuf::from)
                        .map(Action::SetProjectFolder),
                    Field::UserLibrary => nonblank(&self.user_library)
                        .map(PathBuf::from)
                        .map(Action::SetUserLibrary),
                    Field::SampleFolder => nonblank(&self.sample_folder)
                        .map(PathBuf::from)
                        .map(Action::AddSampleFolder),
                    Field::ProjectPath | Field::ExportPath => None,
                };
            }
            ctx.request_repaint();
            return None;
        }

        if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
            if stage.exporting && self.page == Some(Page::Export) {
                return Some(Action::CancelExport);
            }
            self.close();
            return None;
        }

        // Shift-left/right walks the four utility rooms. Unshifted motion
        // belongs to the value on the current row.
        if key(ctx, egui::Modifiers::SHIFT, egui::Key::ArrowLeft) {
            self.cycle_page(-1);
            self.clamp_row(stage);
            return None;
        }
        if key(ctx, egui::Modifiers::SHIFT, egui::Key::ArrowRight) {
            self.cycle_page(1);
            self.clamp_row(stage);
            return None;
        }

        let up = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowUp)
            || key(ctx, egui::Modifiers::NONE, egui::Key::K)
            || key(ctx, egui::Modifiers::SHIFT, egui::Key::Tab);
        let down = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowDown)
            || key(ctx, egui::Modifiers::NONE, egui::Key::J)
            || key(ctx, egui::Modifiers::NONE, egui::Key::Tab);
        let rows = self.rows(stage).max(1);
        if down {
            self.row = (self.row + 1) % rows;
            return None;
        }
        if up {
            self.row = (self.row + rows - 1) % rows;
            return None;
        }
        let right = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowRight)
            || key(ctx, egui::Modifiers::NONE, egui::Key::L);
        let left = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowLeft)
            || key(ctx, egui::Modifiers::NONE, egui::Key::H);
        if right || left {
            return self.adjust(if right { 1 } else { -1 }, stage);
        }
        if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
            return self.activate(stage);
        }
        None
    }

    fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::ProjectPath => &mut self.path,
            Field::ProjectFolder => &mut self.project_folder,
            Field::UserLibrary => &mut self.user_library,
            Field::SampleFolder => &mut self.sample_folder,
            Field::ExportPath => &mut self.export_path,
        }
    }

    fn cycle_page(&mut self, delta: isize) {
        let current = self.page.unwrap_or(Page::Projects);
        self.page = Some(cycle(&Page::ALL, current, delta));
        self.row = 0;
        self.editing = None;
        if self.page == Some(Page::Preferences) && self.pref_page == PrefPage::Audio {
            self.request_audio_scan();
        }
    }

    fn adjust(&mut self, delta: isize, stage: &UtilitySnapshot) -> Option<Action> {
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
                    4 => self.prefs.skip_dirty_confirmation = !self.prefs.skip_dirty_confirmation,
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

    fn activate(&mut self, stage: &UtilitySnapshot) -> Option<Action> {
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
                    let mut index = index - 7;
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

fn key(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|input| input.consume_key(modifiers, key))
}

fn nonblank(value: &str) -> Option<&str> {
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
struct DisplayRow {
    code: String,
    label: String,
    value: String,
    enabled: bool,
    alarm: bool,
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

    pub(super) fn draw(&mut self, ui: &mut egui::Ui, stage: &UtilitySnapshot) -> Option<Action> {
        let page = self.page?;
        let whole = ui.max_rect();
        let panel = egui::Rect::from_center_size(
            whole.center(),
            egui::vec2(
                (whole.width() - 40.0).min(PANEL_MAX_W).max(620.0),
                (whole.height() - 36.0).min(PANEL_MAX_H).max(440.0),
            ),
        );
        let painter = ui
            .painter()
            .with_clip_rect(whole)
            .with_layer_id(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("stage-utility-console"),
            ));
        let alpha = design::Alphabet::for_polarity(design::Polarity::Dark);

        painter.rect_filled(whole, 0.0, egui::Color32::from_black_alpha(224));
        let mut shell = Vec::new();
        circuit::panel_variant(
            &mut shell,
            panel,
            Some(alpha.well.color),
            alpha.ground.color,
            Some((Weight::Heavy, alpha.focus.color)),
            3,
        );
        circuit::panel_frame_variant(
            &mut shell,
            panel.shrink(5.0),
            Weight::Hair,
            alpha.edge.color,
            1,
        );
        painter.extend(shell);

        let inner = panel.shrink(18.0);
        let header =
            egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + HEADER_H));
        let footer = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - FOOTER_H),
            inner.max,
        );
        let nav = egui::Rect::from_min_max(
            egui::pos2(inner.left(), header.bottom() + GAP),
            egui::pos2(inner.left() + NAV_W, footer.top() - GAP),
        );
        let content = egui::Rect::from_min_max(
            egui::pos2(nav.right() + 18.0, nav.top()),
            egui::pos2(inner.right(), nav.bottom()),
        );

        self.paint_header(&painter, header, page, stage, alpha);
        let unlocked = self.confirm.is_none();
        let mut action = self.paint_nav(ui, &painter, nav, page, unlocked, alpha);
        let rows = self.display_rows(stage);
        self.clamp_row(stage);
        action = self
            .paint_rows(ui, &painter, content, &rows, stage, unlocked, alpha)
            .or(action);
        self.paint_footer(&painter, footer, alpha);

        if self.confirm.is_some() {
            action = self.paint_confirm(ui, &painter, panel, alpha).or(action);
        }

        crate::shell::screen::register(&painter, panel, crate::shell::screen::State::new(0.0, 0.0));
        action
    }

    fn paint_header(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        page: Page,
        stage: &UtilitySnapshot,
        alpha: &design::Alphabet,
    ) {
        text(
            painter,
            egui::pos2(rect.left() + 14.0, rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            if self.startup {
                "DAW // PROJECT DECK"
            } else {
                "DAW // UTILITY BUS"
            },
            20.0,
            alpha.focus.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 15.0, rect.top() + 40.0),
            egui::Align2::LEFT_TOP,
            format!("{} · BUILD {}", page.label(), env!("CARGO_PKG_VERSION")),
            11.0,
            alpha.ink.color,
        );
        let project = stage
            .project_path
            .as_deref()
            .map(super::document::title)
            .unwrap_or_else(|| "UNTITLED".to_owned());
        text(
            painter,
            egui::pos2(rect.right() - 12.0, rect.top() + 12.0),
            egui::Align2::RIGHT_TOP,
            format!(
                "{}{}",
                project.to_uppercase(),
                if stage.dirty { " *" } else { "" }
            ),
            13.0,
            if stage.dirty {
                alpha.jeopardy_active.color
            } else {
                alpha.ink.color
            },
        );
        text(
            painter,
            egui::pos2(rect.right() - 12.0, rect.top() + 39.0),
            egui::Align2::RIGHT_TOP,
            format!(
                "{:02} TRACKS · {:03} PATTERNS · {:03} DEVICES",
                stage.track_count, stage.pattern_count, stage.device_count
            ),
            10.0,
            alpha.edge.color,
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.bottom() - 1.0),
                egui::pos2(rect.right(), rect.bottom() - 1.0),
            ],
            egui::Stroke::new(1.0, alpha.edge.color),
        );
    }

    fn paint_nav(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        page: Page,
        interactive: bool,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let mut action = None;
        text(
            painter,
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            "SYS://",
            10.0,
            alpha.edge.color,
        );
        for (index, candidate) in Page::ALL.into_iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 22.0 + index as f32 * 44.0),
                egui::vec2(rect.width(), 36.0),
            );
            let active = candidate == page;
            let mut shapes = Vec::new();
            circuit::relic_frame(
                &mut shapes,
                row,
                if active {
                    alpha.surface.color
                } else {
                    alpha.ground.color
                },
                if active { Weight::Bold } else { Weight::Hair },
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            painter.extend(shapes);
            text(
                painter,
                egui::pos2(row.left() + 12.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                format!("0{}  {}", index + 1, candidate.label()),
                11.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.ink.color
                },
            );
            if interactive
                && ui
                    .interact(
                        row,
                        egui::Id::new(("utility-nav", index)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press)
                    .clicked()
            {
                action = Some(Action::OpenPage(candidate));
            }
        }
        let y = rect.bottom() - 66.0;
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y),
            egui::Align2::LEFT_TOP,
            "SHIFT + ←/→  ROOMS",
            9.0,
            alpha.edge.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y + 18.0),
            egui::Align2::LEFT_TOP,
            "↑/↓  ADDRESS",
            9.0,
            alpha.edge.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y + 36.0),
            egui::Align2::LEFT_TOP,
            "ENTER  EXECUTE",
            9.0,
            alpha.edge.color,
        );
        action
    }

    fn paint_rows(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        rows: &[DisplayRow],
        stage: &UtilitySnapshot,
        interactive: bool,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let visible = ((rect.height() + GAP) / (ROW_H + GAP)).floor().max(1.0) as usize;
        let start = self
            .row
            .saturating_add(1)
            .saturating_sub(visible)
            .min(rows.len().saturating_sub(visible));
        let mut clicked = None;
        for (slot, index) in (start..rows.len()).take(visible).enumerate() {
            let data = &rows[index];
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + slot as f32 * (ROW_H + GAP)),
                egui::vec2(rect.width(), ROW_H),
            );
            let active = index == self.row;
            let fill = if active {
                alpha.surface.color
            } else if index % 2 == 0 {
                alpha.ground.color
            } else {
                alpha.well.color.gamma_multiply(0.72)
            };
            let mut shapes = Vec::new();
            circuit::relic_frame(
                &mut shapes,
                row,
                fill,
                if active { Weight::Bold } else { Weight::Hair },
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            painter.extend(shapes);
            let ink = if !data.enabled {
                alpha.edge.color.gamma_multiply(0.55)
            } else if data.alarm {
                alpha.jeopardy_active.color
            } else if active {
                alpha.focus.color
            } else {
                alpha.ink.color
            };
            text(
                painter,
                egui::pos2(row.left() + 10.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                &data.code,
                9.0,
                alpha.edge.color,
            );
            text(
                painter,
                egui::pos2(row.left() + 50.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                &data.label,
                12.0,
                ink,
            );
            text(
                painter,
                egui::pos2(row.right() - 12.0, row.center().y),
                egui::Align2::RIGHT_CENTER,
                &data.value,
                10.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            let response = ui
                .interact(
                    row,
                    egui::Id::new(("utility-row", self.page, self.pref_page, index)),
                    egui::Sense::click(),
                )
                .affords(if data.enabled {
                    Affords::Press
                } else {
                    Affords::Refuse
                });
            if interactive && response.clicked() && data.enabled {
                self.row = index;
                clicked = Some(index);
            }
            if active {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("utility-cursor", self.page, self.pref_page, index),
                    row,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Utility,
                    alpha.focus.color,
                );
            }
        }
        clicked.and_then(|_| self.activate(stage))
    }

    fn paint_footer(&self, painter: &egui::Painter, rect: egui::Rect, alpha: &design::Alphabet) {
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0, alpha.edge.color),
        );
        let status = self.status.as_deref().unwrap_or(
            "TAB/↑↓ MOVE · ←→ CHANGE · ENTER ACT · ESC CLOSE · CTRL+O / CTRL+, / CTRL+SHIFT+E",
        );
        text(
            painter,
            egui::pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            status,
            9.0,
            if self.status.is_some() {
                alpha.ink.color
            } else {
                alpha.edge.color
            },
        );
        text(
            painter,
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "ESC // RETURN TO SIGNAL",
            9.0,
            alpha.edge.color,
        );
    }

    fn paint_confirm(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        parent: egui::Rect,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let rect = egui::Rect::from_center_size(parent.center(), egui::vec2(520.0, 250.0));
        painter.rect_filled(parent, 0.0, egui::Color32::from_black_alpha(190));
        let mut shapes = Vec::new();
        circuit::panel_variant(
            &mut shapes,
            rect,
            Some(alpha.surface.color),
            alpha.ground.color,
            Some((Weight::Heavy, alpha.jeopardy_active.color)),
            2,
        );
        painter.extend(shapes);
        let question = match self.confirm.as_ref()? {
            Confirm::Replace(_) => "UNSAVED SIGNAL IN MEMORY",
            Confirm::OverwriteProject(path) | Confirm::OverwriteExport(path) => {
                text(
                    painter,
                    egui::pos2(rect.center().x, rect.top() + 52.0),
                    egui::Align2::CENTER_CENTER,
                    path.display().to_string(),
                    9.0,
                    alpha.edge.color,
                );
                "DESTINATION ALREADY EXISTS"
            }
        };
        text(
            painter,
            egui::pos2(rect.center().x, rect.top() + 28.0),
            egui::Align2::CENTER_CENTER,
            question,
            15.0,
            alpha.jeopardy_active.color,
        );
        let labels: [&str; 3] = match self.confirm {
            Some(Confirm::Replace(_)) => ["SAVE + CONTINUE", "DISCARD + CONTINUE", "CANCEL"],
            Some(Confirm::OverwriteProject(_)) | Some(Confirm::OverwriteExport(_)) => {
                ["OVERWRITE", "USE A NEW PATH", "CANCEL"]
            }
            None => return None,
        };
        let mut action = None;
        for (index, label) in labels.into_iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 44.0, rect.top() + 80.0 + index as f32 * 44.0),
                egui::vec2(rect.width() - 88.0, 34.0),
            );
            let active = index == self.confirm_row;
            painter.rect_filled(
                row,
                0.0,
                if active {
                    alpha.well.color
                } else {
                    alpha.ground.color
                },
            );
            painter.rect_stroke(
                row,
                0.0,
                egui::Stroke::new(
                    1.0,
                    if active {
                        alpha.focus.color
                    } else {
                        alpha.edge.color
                    },
                ),
                egui::StrokeKind::Inside,
            );
            text(
                painter,
                row.center(),
                egui::Align2::CENTER_CENTER,
                label,
                11.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.ink.color
                },
            );
            if ui
                .interact(
                    row,
                    egui::Id::new(("utility-confirm", index)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                self.confirm_row = index;
                action = Some(Action::ResolveConfirm(index));
            }
            if active {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("utility-confirm-cursor", index),
                    row,
                    crate::ui::nav_cursor::Kind::Prompt,
                    crate::ui::nav_cursor::Layer::Utility,
                    alpha.jeopardy_active.color,
                );
            }
        }
        action
    }

    fn display_rows(&self, stage: &UtilitySnapshot) -> Vec<DisplayRow> {
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
                DisplayRow::new(
                    "GUARD",
                    "CONFIRM DIRTY REPLACE",
                    on_off(!self.prefs.skip_dirty_confirmation),
                ),
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

fn text(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    words: impl ToString,
    size: f32,
    color: egui::Color32,
) {
    painter.text(
        at,
        align,
        words.to_string(),
        egui::FontId::monospace(design::px(size)),
        color,
    );
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

    pub(super) fn update_utility(&mut self, ctx: &egui::Context) {
        if !self.utility.is_open() {
            return;
        }
        let snapshot = self.utility_snapshot();
        if let Some(action) = self.utility.consume_input(ctx, &snapshot) {
            self.apply_utility(action, ctx);
        }
        // Interface preferences preview immediately. Audio remains an
        // explicit restart because changing a live device is not reversible
        // by merely closing the modal.
        self.polarity = if self.utility.prefs().light_ground {
            design::Polarity::Light
        } else {
            design::Polarity::Dark
        };
    }

    pub(super) fn draw_utility(&mut self, ui: &mut egui::Ui) {
        if !self.utility.is_open() {
            return;
        }
        let snapshot = self.utility_snapshot();
        if let Some(action) = self.utility.draw(ui, &snapshot) {
            self.apply_utility(action, ui.ctx());
        }
    }

    fn utility_snapshot(&self) -> UtilitySnapshot {
        UtilitySnapshot {
            project_path: self.path.clone(),
            dirty: self.dirty,
            track_count: self.song.tracks.len(),
            pattern_count: self.song.patterns.len(),
            device_count: self.song.tracks.iter().map(|track| track.chain.len()).sum(),
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

    fn apply_utility(&mut self, action: Action, ctx: &egui::Context) {
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
                ctx.copy_text(self.diagnostics_report());
                self.utility.set_status("DIAGNOSTICS COPIED");
            }
            Action::OpenPage(page) => self.open_utility(page),
        }
    }

    fn request_project_op(&mut self, op: ProjectOp) {
        if self.dirty && !self.utility.prefs().skip_dirty_confirmation {
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

        stage.apply_utility(Action::ApplyAudio, &egui::Context::default());

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
