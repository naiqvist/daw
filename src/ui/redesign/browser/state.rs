//! Browser-local presentation and navigation state.

use crate::library::{self, LibrarySnapshot};
use crate::ui::tokens::control;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RowKind {
    Location { key: String, open: bool },
    Folder { key: String, open: bool },
    Asset(PathBuf),
    Message,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Row {
    pub(super) depth: usize,
    pub(super) label: String,
    pub(super) detail: String,
    pub(super) kind: RowKind,
}

impl Row {
    pub(super) fn is_container(&self) -> bool {
        matches!(self.kind, RowKind::Location { .. } | RowKind::Folder { .. })
    }
}

/// View state only. Catalog data stays in the immutable library snapshot.
pub(super) struct BrowserState {
    width: f32,
    pub(super) query: String,
    pub(super) expanded: HashSet<String>,
    pub(super) cursor: usize,
    pub(super) searching: bool,
    /// The last unsupported sentence, shown in the footer until another
    /// sentence replaces it.
    pub(super) refusal: Option<String>,
    /// Session-local monitor switch. Browsing starts audible; M applies the
    /// shared mute verb to the panel itself without changing the cursor.
    pub(super) audition_enabled: bool,
    audition_candidate: Option<PathBuf>,
    audition_since: f64,
    audition_playing: Option<PathBuf>,
    generation: u64,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            width: control::SIDE_COLUMN_W,
            query: String::new(),
            expanded: HashSet::new(),
            cursor: 0,
            searching: false,
            refusal: None,
            audition_enabled: true,
            audition_candidate: None,
            audition_since: 0.0,
            audition_playing: None,
            generation: 0,
        }
    }
}

impl BrowserState {
    pub(super) fn width(&self) -> f32 {
        self.width
    }

    pub(super) fn rows(&mut self, snapshot: &LibrarySnapshot) -> Vec<Row> {
        if self.generation != snapshot.generation {
            self.generation = snapshot.generation;
            self.cursor = 0;
        }
        let query = self.query.trim();
        let mut rows = if query.is_empty() {
            hierarchy_rows(snapshot, &self.expanded)
        } else {
            search_rows(snapshot, query)
        };
        if rows.is_empty() {
            rows.push(Row {
                depth: 0,
                label: if snapshot.locations.is_empty() {
                    "NO LIBRARY ROOTS".to_owned()
                } else if query.is_empty() {
                    "ARCHIVE IS EMPTY".to_owned()
                } else {
                    "NO SIGNAL".to_owned()
                },
                detail: String::new(),
                kind: RowKind::Message,
            });
        }
        self.cursor = self.cursor.min(rows.len().saturating_sub(1));
        rows
    }

    pub(super) fn step(&mut self, amount: isize, row_count: usize) {
        if row_count == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = self.cursor.saturating_add_signed(amount).min(row_count - 1);
    }

    pub(super) fn set_gate(&mut self, row: &Row, open: bool) -> bool {
        let key = match &row.kind {
            RowKind::Location { key, .. } | RowKind::Folder { key, .. } => key,
            RowKind::Asset(_) | RowKind::Message => return false,
        };
        if open {
            self.expanded.insert(key.clone());
        } else {
            self.expanded.remove(key);
        }
        true
    }

    pub(super) fn toggle_gate(&mut self, row: &Row) -> bool {
        match &row.kind {
            RowKind::Location { open, .. } | RowKind::Folder { open, .. } => {
                self.set_gate(row, !open)
            }
            RowKind::Asset(_) | RowKind::Message => false,
        }
    }

    pub(super) fn update_audition(
        &mut self,
        target: Option<&Path>,
        now: f64,
        intents: &mut Vec<super::Intent>,
    ) -> Option<f64> {
        const DEBOUNCE_S: f64 = 0.150;

        let target = if self.audition_enabled {
            target.map(Path::to_path_buf)
        } else {
            None
        };
        if target != self.audition_candidate {
            self.audition_candidate = target;
            self.audition_since = now;
            if self.audition_playing.take().is_some() {
                intents.push(super::Intent::StopAudition);
            }
        }
        let candidate = self.audition_candidate.as_ref()?;
        if self.audition_playing.is_none() && now - self.audition_since >= DEBOUNCE_S {
            self.audition_playing = Some(candidate.clone());
            intents.push(super::Intent::AuditionSample(candidate.clone()));
        }
        self.audition_playing
            .is_none()
            .then(|| (DEBOUNCE_S - (now - self.audition_since)).max(0.0))
    }
}

fn location_key(id: &str) -> String {
    format!("location:{id}")
}

fn folder_key(id: &str, path: &Path) -> String {
    format!("folder:{id}:{}", path.to_string_lossy())
}

fn hierarchy_rows(snapshot: &LibrarySnapshot, expanded: &HashSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    for location in &snapshot.locations {
        let key = location_key(&location.id);
        let open = expanded.contains(&key);
        let count = snapshot
            .assets
            .iter()
            .filter(|asset| asset.location_id == location.id)
            .count();
        rows.push(Row {
            depth: 0,
            label: location.label.to_ascii_uppercase(),
            detail: format!("{count:02}"),
            kind: RowKind::Location { key, open },
        });
        if open {
            append_folder(
                snapshot,
                expanded,
                &location.id,
                Path::new(""),
                1,
                &mut rows,
            );
        }
    }
    rows
}

fn append_folder(
    snapshot: &LibrarySnapshot,
    expanded: &HashSet<String>,
    location_id: &str,
    parent: &Path,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    let mut folders: Vec<_> = snapshot
        .folders
        .iter()
        .filter(|folder| {
            folder.location_id == location_id && normalized_parent(&folder.relative_path) == parent
        })
        .collect();
    folders.sort_by_key(|folder| folder.name.to_ascii_lowercase());

    for folder in folders {
        let key = folder_key(location_id, &folder.relative_path);
        let open = expanded.contains(&key);
        let count = snapshot
            .assets
            .iter()
            .filter(|asset| {
                asset.location_id == location_id
                    && asset.relative_path.starts_with(&folder.relative_path)
            })
            .count();
        rows.push(Row {
            depth,
            label: folder.name.to_ascii_uppercase(),
            detail: format!("{count:02}"),
            kind: RowKind::Folder { key, open },
        });
        if open {
            append_folder(
                snapshot,
                expanded,
                location_id,
                &folder.relative_path,
                depth + 1,
                rows,
            );
        }
    }

    let mut assets: Vec<_> = snapshot
        .assets
        .iter()
        .filter(|asset| {
            asset.location_id == location_id && normalized_parent(&asset.relative_path) == parent
        })
        .collect();
    assets.sort_by_key(|asset| asset.name.to_ascii_lowercase());
    rows.extend(assets.into_iter().map(|asset| Row {
        depth,
        label: asset.name.clone(),
        detail: asset.extension.to_ascii_uppercase(),
        kind: RowKind::Asset(asset.path.clone()),
    }));
}

fn normalized_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(""))
}

fn search_rows(snapshot: &LibrarySnapshot, query: &str) -> Vec<Row> {
    library::query(snapshot, None, query)
        .into_iter()
        .map(|asset| Row {
            depth: 0,
            label: asset.name.clone(),
            detail: asset.relative_path.to_string_lossy().into_owned(),
            kind: RowKind::Asset(asset.path.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{AssetRecord, LibraryFolder, LibraryLocation};

    fn snapshot() -> LibrarySnapshot {
        LibrarySnapshot {
            generation: 4,
            locations: vec![LibraryLocation {
                id: "root".to_owned(),
                label: "Drums".to_owned(),
                path: PathBuf::from("/samples"),
                user_library: false,
            }],
            folders: vec![LibraryFolder {
                location_id: "root".to_owned(),
                relative_path: PathBuf::from("kicks"),
                name: "kicks".to_owned(),
            }],
            assets: vec![AssetRecord {
                path: PathBuf::from("/samples/kicks/iron.wav"),
                relative_path: PathBuf::from("kicks/iron.wav"),
                location_id: "root".to_owned(),
                name: "iron".to_owned(),
                extension: "wav".to_owned(),
                bytes: 1,
                modified_unix_secs: None,
                tags: vec!["hard".to_owned()],
            }],
            scales: Vec::new(),
            lenses: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn hierarchy_reveals_only_through_open_gates() {
        let snapshot = snapshot();
        let mut state = BrowserState::default();
        let rows = state.rows(&snapshot);
        assert_eq!(rows.len(), 1);
        state.set_gate(&rows[0], true);
        let rows = state.rows(&snapshot);
        assert_eq!(rows.len(), 2);
        state.set_gate(&rows[1], true);
        let rows = state.rows(&snapshot);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[2].kind, RowKind::Asset(_)));
    }

    #[test]
    fn tag_search_ignores_closed_gates_and_finds_the_asset() {
        let snapshot = snapshot();
        let mut state = BrowserState {
            query: "#hard".to_owned(),
            ..BrowserState::default()
        };
        let rows = state.rows(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "iron");
    }

    #[test]
    fn locus_is_clamped_when_results_contract() {
        let snapshot = snapshot();
        let mut state = BrowserState {
            cursor: 99,
            query: "missing".to_owned(),
            ..BrowserState::default()
        };
        let rows = state.rows(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(state.cursor, 0);
    }
}
