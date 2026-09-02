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

use crate::sequencing::Song;
use std::path::{Path, PathBuf};

/// The suffix a song the stage saved carries.
pub const EXTENSION: &str = "stage.ron";

/// The document's shape on disk.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Document {
    version: u32,
    song: Song,
}

const VERSION: u32 = 1;

/// Write `song` to `path`, making the directory if it is not there.
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
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// Read a song from `path`: one the stage saved, or a project the first
/// frame saved. The song is repaired at the boundary, exactly as the
/// first frame repairs it, so nothing downstream ever sees illegal data.
pub fn load(path: &Path) -> Result<Song, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: Document = ron::from_str(&text).map_err(|error| error.to_string())?;
    let mut song = document.song;
    song.normalize_group_depths();
    song.normalize_mixer();
    song.normalize_chains();
    Ok(song)
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
    use crate::sequencing::TrackKind;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("daw-stage-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_song_comes_back_as_it_went() {
        let mut song = Song::default();
        song.add_track(TrackKind::Audio);
        song.rename_track(0, "Keys");
        song.fill_slot(0, 3).expect("a slot fills");
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
        let song_text = ron::ser::to_string(&song).expect("encodes");
        let path = scratch("legacy.daw.ron");
        std::fs::write(
            &path,
            format!("(version: 9, bpm: 120.0, metronome: false, song: {song_text}, clips: [])"),
        )
        .expect("writes");
        let back = load(&path).expect("the song inside opens");
        assert_eq!(back.tracks[0].name, "From the first frame");
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
}
