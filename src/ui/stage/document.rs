//! The song on disk, and back.
//!
//! One song per file, as RON, wrapped in a version so the day the shape
//! changes has somewhere to put a migration. Loading is deliberately
//! wider than saving: the first frame's project files carry their song
//! under the same `song` key beside a great deal the stage does not
//! model, and serde ignores what it is not asked for — so a song made in
//! the `daw` binary opens here, and only what the stage saves is written
//! back. What the stage writes, the stage can read; nothing else is
//! promised.

use crate::{
    audio::modulation::{ModWire, Modulator},
    sequencing::Song,
};
use std::path::{Path, PathBuf};

/// The suffix a song the stage saved carries.
pub const EXTENSION: &str = "stage.ron";

/// The document's shape on disk.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Document {
    version: u32,
    song: Song,
}

/// The first frame's wrapper is a different document format. In particular,
/// RON writes its BPM as a bare float, not as `Some(float)`, so pretending it
/// is an optional field on the stage document rejects the very files this
/// importer exists to open.
#[derive(Debug, serde::Deserialize)]
struct LegacyDocument {
    #[serde(rename = "version")]
    _version: u32,
    bpm: f64,
    /// The first-frame DAW keeps modulation beside its canonical Song.
    /// Import it explicitly so opening one of those projects in Stage does
    /// not silently leave a working patch bay behind.
    #[serde(default)]
    modulators: Vec<Modulator>,
    #[serde(default)]
    mod_wires: Vec<ModWire>,
    #[serde(default)]
    next_modulator_id: u64,
    song: Song,
}

const VERSION: u32 = 3;
const BACKUP_LIMIT: usize = 20;

/// Move a stage-native document to the current in-memory shape. Versions zero
/// and one predate the default input/output gain utilities; installing their
/// exact-unity nodes is an audible no-op with explicit stable ids. Keeping the
/// step here makes a version we do not understand a refusal rather than a
/// lossy best guess.
fn migrate(version: u32, mut song: Song) -> Result<Song, String> {
    match version {
        0 | 1 => song.install_default_track_gains(),
        2 | VERSION => {}
        future => {
            return Err(format!(
                "project version {future} is newer than this build (supports through {VERSION})"
            ));
        }
    }
    song.normalize_group_depths();
    song.normalize_mixer();
    song.normalize_chains();
    song.normalize_modulation();
    song.normalize_timeline();
    Ok(song)
}

/// Write `song` to `path`, making the directory if it is not there.
///
/// The named document is replaced only after a complete temporary sibling
/// has reached disk. A crash while serialising or writing therefore leaves
/// either the previous document or the next one, never half of either.
pub fn save(path: &Path, song: &Song) -> Result<(), String> {
    let document = Document {
        version: VERSION,
        song: song.clone(),
    };
    let text = ron::ser::to_string_pretty(&document, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())?;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "project path has no file name".to_owned())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temp = path.with_file_name(format!(".{name}.{}.{nonce}.writing", std::process::id()));
    let write = (|| {
        let mut file = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
        use std::io::Write as _;
        file.write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&temp, path).map_err(|error| error.to_string())
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write
}

/// Give a Save-As path the stage document suffix unless it already names a
/// stage or compatible legacy document.
pub fn with_extension(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    if name.ends_with(".stage.ron") || name.ends_with(".daw.ron") {
        return path.to_path_buf();
    }
    let stem = name.strip_suffix(".ron").unwrap_or(name);
    path.with_file_name(format!("{stem}.{EXTENSION}"))
}

/// The first unused numbered sibling of `path`, retaining compound project
/// suffixes and ordinary one-part suffixes such as `.wav`.
pub fn available_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("untitled");
    let (stem, suffix) = if let Some(stem) = name.strip_suffix(".stage.ron") {
        (stem, ".stage.ron")
    } else if let Some(stem) = name.strip_suffix(".daw.ron") {
        (stem, ".daw.ron")
    } else if let Some(dot) = name.rfind('.').filter(|dot| *dot > 0) {
        (&name[..dot], &name[dot..])
    } else {
        (name, "")
    };
    (2..)
        .map(|number| path.with_file_name(format!("{stem} {number}{suffix}")))
        .find(|candidate| !candidate.exists())
        .expect("the integers do not run out")
}

/// Preserve the document that is about to be overwritten. Backups live in a
/// machine-local project folder, never beside samples, and the newest twenty
/// copies of one project are retained.
pub fn backup(path: &Path, home: &Path) -> Result<Option<PathBuf>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let directory = home.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let prefix = format!("{}-", title(path));
    let destination = available_path(
        &directory.join(format!("{prefix}{}.stage.ron", super::arrangement::stamp())),
    );
    std::fs::copy(path, &destination).map_err(|error| error.to_string())?;

    let mut siblings: Vec<_> = std::fs::read_dir(&directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".stage.ron"))
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    siblings.sort_by_key(|(modified, _)| *modified);
    let excess = siblings.len().saturating_sub(BACKUP_LIMIT);
    for (_, old) in siblings.into_iter().take(excess) {
        std::fs::remove_file(old).map_err(|error| error.to_string())?;
    }
    Ok(Some(destination))
}

/// Read a song from `path`: one the stage saved, or a project the first
/// frame saved. The song is repaired at the boundary, exactly as the
/// first frame repairs it, so nothing downstream ever sees illegal data.
pub fn load(path: &Path) -> Result<Song, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    // `.daw.ron` is the first frame's wrapper. Its version numbers belong to
    // that application rather than this document schema; the compatible Song
    // inside is intentionally imported and repaired. Everything else is a
    // stage-native document and must pass this format's version gate.
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".daw.ron"))
        && let Ok(legacy) = ron::from_str::<LegacyDocument>(&text)
    {
        let mut song = legacy.song;
        let _ = song.set_base_bpm(legacy.bpm);
        // These are the editable, saved modulation facts in the first-frame
        // document. They are authoritative even when empty; its nested Song
        // is a projection and may contain stale defaults.
        song.modulators = legacy.modulators;
        song.mod_wires = legacy.mod_wires;
        song.next_modulation_id = legacy.next_modulator_id;
        song.install_default_track_gains();
        song.normalize_group_depths();
        song.normalize_mixer();
        song.normalize_chains();
        song.normalize_modulation();
        song.normalize_timeline();
        return Ok(song);
    }
    // Stage may save back to the path a legacy project already owns. Its
    // native wrapper has no top-level BPM, which distinguishes it from the
    // first-frame format and lets that `.daw.ron` reopen without dropping the
    // modulation now stored inside Song.
    let document: Document = ron::from_str(&text).map_err(|error| error.to_string())?;
    migrate(document.version, document.song)
}

/// Where a song with no file of its own is saved, inside the songs
/// folder: the stage does not open a dialog, so a first save has to land
/// somewhere the performer can find afterwards, and the strip says
/// where. A name not already taken there, so a second new song does not
/// write over the first.
pub fn untitled_in(home: &Path) -> PathBuf {
    let first = home.join(format!("untitled.{EXTENSION}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| home.join(format!("untitled {n}.{EXTENSION}")))
        .find(|path| !path.exists())
        .expect("the integers do not run out")
}

/// The name a song is called by on screen: the file's stem, with the
/// stage's own suffix taken off. `song.stage.ron` is one song called
/// `song`.
pub fn title(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("untitled");
    name.strip_suffix(&format!(".{EXTENSION}"))
        .or_else(|| name.strip_suffix(".daw.ron"))
        .or_else(|| name.strip_suffix(".ron"))
        .unwrap_or(name)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::{DeviceRole, TRACK_VOLUME, TrackKind};

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("daw-stage-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_song_comes_back_as_it_went() {
        let mut song = Song::default();
        assert!(song.set_base_bpm(87.0));
        song.add_track(TrackKind::Audio);
        song.rename_track(0, "Keys");
        song.fill_slot(0, 3).expect("a slot fills");
        let source = song.add_lfo().expect("a modulation source fits");
        let wire = song
            .add_mod_wire(source, 0, TRACK_VOLUME)
            .expect("a modulation wire fits");
        let response = song
            .mod_wires
            .iter_mut()
            .find(|candidate| candidate.id == wire)
            .expect("the new wire is present");
        response.depth = -0.42;
        response.curve = 0.35;
        response.steps = 7;
        response.smooth_ms = 85.0;
        let path = scratch("round.stage.ron");
        save(&path, &song).expect("saves");
        let back = load(&path).expect("loads");
        assert_eq!(back, song);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saving_makes_the_directory_it_needs() {
        let dir = scratch("made");
        let path = dir.join("deep").join("song.stage.ron");
        save(&path, &Song::default()).expect("saves into a directory that was not there");
        assert!(load(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_frame_project_opens_by_its_song() {
        // The legacy file is the same key beside fields the stage never
        // models. Serde leaves them alone.
        let mut song = Song::default();
        song.rename_track(0, "From the first frame");
        let mut modulation = Song::default();
        let source = modulation.add_lfo().expect("a source fits");
        modulation
            .add_mod_wire(source, 0, TRACK_VOLUME)
            .expect("a wire fits");
        let song_text = ron::ser::to_string(&song).expect("encodes");
        let sources = ron::ser::to_string(&modulation.modulators).expect("sources encode");
        let wires = ron::ser::to_string(&modulation.mod_wires).expect("wires encode");
        let path = scratch("legacy.daw.ron");
        std::fs::write(
            &path,
            format!(
                "(version: 9, bpm: 93.0, metronome: false, modulators: {sources}, mod_wires: {wires}, next_modulator_id: {}, song: {song_text}, clips: [])",
                modulation.next_modulation_id
            ),
        )
        .expect("writes");
        let back = load(&path).expect("the song inside opens");
        assert_eq!(back.tracks[0].name, "From the first frame");
        assert_eq!(back.base_bpm(), 93.0, "the wrapper tempo was discarded");
        assert_eq!(back.modulators, modulation.modulators);
        assert_eq!(back.mod_wires, modulation.mod_wires);
        assert_eq!(back.next_modulation_id, modulation.next_modulation_id);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saving_back_to_a_legacy_path_keeps_native_modulation_on_reopen() {
        let song = Song::default();
        let mut modulation = Song::default();
        let source = modulation.add_lfo().expect("a source fits");
        modulation
            .add_mod_wire(source, 0, TRACK_VOLUME)
            .expect("a wire fits");
        let song_text = ron::ser::to_string(&song).expect("song encodes");
        let sources = ron::ser::to_string(&modulation.modulators).expect("sources encode");
        let wires = ron::ser::to_string(&modulation.mod_wires).expect("wires encode");
        let path = scratch("legacy-save-reopen.daw.ron");
        std::fs::write(
            &path,
            format!(
                "(version: 9, bpm: 120.0, modulators: {sources}, mod_wires: {wires}, next_modulator_id: {}, song: {song_text})",
                modulation.next_modulation_id
            ),
        )
        .expect("legacy project writes");

        let imported = load(&path).expect("legacy project imports");
        save(&path, &imported).expect("Stage saves to the existing legacy path");
        let reopened = load(&path).expect("the Stage-native wrapper reopens by shape");
        assert_eq!(reopened.modulators, imported.modulators);
        assert_eq!(reopened.mod_wires, imported.mod_wires);
        assert_eq!(reopened.next_modulation_id, imported.next_modulation_id);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_future_stage_document_is_refused_without_touching_it() {
        let song_text = ron::ser::to_string(&Song::default()).expect("encodes");
        let path = scratch("future.stage.ron");
        let text = format!("(version: 99, song: {song_text})");
        std::fs::write(&path, &text).expect("writes");
        let error = load(&path).expect_err("a future schema opened by guessing");
        assert!(error.contains("newer than this build"));
        assert_eq!(std::fs::read_to_string(&path).expect("still there"), text);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn version_zero_takes_the_explicit_migration_path() {
        let mut song = Song::default();
        song.rename_track(0, "Migrated");
        let song_text = ron::ser::to_string(&song).expect("encodes");
        let path = scratch("v0.stage.ron");
        std::fs::write(&path, format!("(version: 0, song: {song_text})")).expect("writes");
        let back = load(&path).expect("version zero migrates");
        assert_eq!(back.tracks[0].name, "Migrated");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn version_one_installs_boundary_gains_once() {
        let mut song = Song::default();
        song.tracks[0].chain.clear();
        let song_text = ron::ser::to_string(&song).expect("encodes");
        let path = scratch("v1-gains.stage.ron");
        std::fs::write(&path, format!("(version: 1, song: {song_text})")).expect("writes");

        let back = load(&path).expect("version one migrates");
        let roles: Vec<_> = back.tracks[0]
            .chain
            .iter()
            .map(|device| device.role)
            .collect();
        assert_eq!(roles, [DeviceRole::InputGain, DeviceRole::OutputGain]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn current_projects_remember_removed_boundary_gains() {
        let mut song = Song::default();
        song.tracks[0].chain.clear();
        let path = scratch("v2-removed-gains.stage.ron");
        save(&path, &song).expect("saves");

        let back = load(&path).expect("loads");
        assert!(back.tracks[0].chain.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loading_repairs_hostile_timeline_maps_before_the_ui_sees_them() {
        let mut song = Song::default();
        song.meter.push(crate::sequencing::MeterMark {
            tick: 0,
            numerator: u32::MAX,
            denominator: 1,
        });
        song.tempo.push(crate::sequencing::TempoMark {
            tick: 96,
            bpm: 90.0,
        });
        song.tempo.push(crate::sequencing::TempoMark {
            tick: 48,
            bpm: 110.0,
        });
        let path = scratch("hostile-timeline.stage.ron");
        save(&path, &song).expect("saves");

        let back = load(&path).expect("loads after repair");
        assert!(back.meter.is_empty());
        assert_eq!(
            back.tempo.iter().map(|mark| mark.tick).collect::<Vec<_>>(),
            [48, 96]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_is_not_a_song_is_refused_with_a_reason() {
        let path = scratch("garbage.stage.ron");
        std::fs::write(&path, "this is not ron").expect("writes");
        let error = load(&path).expect_err("garbage opened");
        assert!(!error.is_empty());
        assert!(load(&scratch("missing.stage.ron")).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_song_is_called_by_its_stem() {
        assert_eq!(title(Path::new("/songs/night.stage.ron")), "night");
        assert_eq!(title(Path::new("/songs/night.daw.ron")), "night");
        assert_eq!(title(Path::new("night")), "night");
    }

    #[test]
    fn a_new_song_never_writes_over_the_last_new_song() {
        let home = scratch("home");
        std::fs::create_dir_all(&home).expect("a folder");
        let first = untitled_in(&home);
        assert_eq!(first, home.join("untitled.stage.ron"));
        save(&first, &Song::default()).expect("saves");
        let second = untitled_in(&home);
        assert_eq!(second, home.join("untitled 2.stage.ron"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn save_as_paths_keep_compound_extensions_and_number_safely() {
        let root = scratch("available");
        std::fs::create_dir_all(&root).expect("a folder");
        assert_eq!(
            with_extension(&root.join("signal")),
            root.join("signal.stage.ron")
        );
        assert_eq!(
            with_extension(&root.join("signal.ron")),
            root.join("signal.stage.ron")
        );
        assert_eq!(
            with_extension(&root.join("signal.daw.ron")),
            root.join("signal.daw.ron")
        );
        let first = root.join("signal.stage.ron");
        save(&first, &Song::default()).expect("first exists");
        assert_eq!(available_path(&first), root.join("signal 2.stage.ron"));
        let wav = root.join("signal.wav");
        std::fs::write(&wav, []).expect("wav placeholder");
        assert_eq!(available_path(&wav), root.join("signal 2.wav"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overwriting_can_preserve_a_loadable_backup() {
        let root = scratch("backup");
        let path = root.join("song.stage.ron");
        let mut first = Song::default();
        first.rename_track(0, "Before");
        save(&path, &first).expect("original saves");
        let preserved = backup(&path, &root)
            .expect("backup succeeds")
            .expect("existing project has a backup");
        let mut second = first.clone();
        second.rename_track(0, "After");
        save(&path, &second).expect("replacement saves");
        assert_eq!(
            load(&preserved).expect("backup loads").tracks[0].name,
            "Before"
        );
        assert_eq!(
            load(&path).expect("new project loads").tracks[0].name,
            "After"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
