//! The SOUND file: everything a track sounds like, minus its notes.
//!
//! A sound is a lane word, the machine in the SRC slot with the values
//! that differ from its table and the sample paths it needs, and every
//! section of the strip with its IN state and its values. It is filed by
//! lane kind in a library outside the project (`dir()`), so a sound made
//! in one song is on the shelf in the next. A KIT is a drum-lane sound
//! whose machine is a kit: one file kind, sixteen pads inside it.
//!
//! Green zone, no egui, no audio types — the neutral home, like
//! `lane.rs` and `pages.rs`. Values travel as `(param id, value)` pairs,
//! the same sparse-by-id shape `Device::overrides` keeps, and land
//! through `Device::set`, so a stale id is ignored and a value out of
//! range is clamped, never refused. Loading is `Song::load_sound`; it
//! lives beside `set_lane` because it needs the song's id mint.
//!
//! Brief: `notes/20260908-sounds-and-kits-brief.md`.

use std::path::{Path, PathBuf};

use crate::console::SectionKind;
use crate::devices::DeviceKind;
use crate::sequencing::{Device, DeviceId, Track};

/// The file's suffix, doubled like the document's so a glance says what
/// it is.
pub const EXTENSION: &str = "sound.ron";
/// The frame version. Bumped when the shape changes; `load` refuses a
/// newer one with words.
pub const VERSION: u32 = 1;
/// The sound `lane <kind>` loads onto a fresh track when the lane's
/// folder has one, so a new lane track sounds at once.
pub const STARTER: &str = "starter";

/// The frame around a sound on disk.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SoundFile {
    version: u32,
    sound: Sound,
}

/// One sound, as captured from a track or read from its file.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sound {
    /// The lane word (`Lane::name`): the kind this sound was made for and
    /// the folder it is filed under.
    pub lane: String,
    /// The machine in the SRC slot. `None` is an intentionally empty
    /// slot, which a sound may well mean.
    #[serde(default)]
    pub machine: Option<Machine>,
    /// Every section of the strip, in strip order.
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// The instrument and what it was set to.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Machine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral: Option<Box<crate::audio::spectral::Routing>>,
    /// `DeviceSpec::prefix` — the stable word the target names already
    /// use in the document, so a machine is named the same way twice.
    pub kind: String,
    /// The values that differ from the machine's table, by parameter id.
    #[serde(default)]
    pub overrides: Vec<(u32, f32)>,
    /// A sampler's or brick's file, absolute, as the document keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<PathBuf>,
    /// A sampler's slices, as fractions of the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slices: Vec<f64>,
    /// A kit's sixteen files, absolute; an empty path is an empty pad.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<PathBuf>,
}

/// One section of the strip.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Section {
    pub kind: SectionKind,
    /// Whether the section is IN. Sections that are always in are
    /// written `true` and read as in regardless.
    pub in_: bool,
    /// The values that differ from the section's table, by parameter id.
    #[serde(default)]
    pub overrides: Vec<(u32, f32)>,
}

/// One sound the library holds, by address. The file is read only when
/// the sound is asked for, so a garbage file refuses at load time with
/// words instead of hiding the shelf.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SoundRecord {
    pub lane: String,
    pub name: String,
    pub path: PathBuf,
}

impl Sound {
    /// What `track` sounds like right now. Overrides that merely restate
    /// a table default are left out, so a saved sound says only what the
    /// hand changed.
    pub fn capture(track: &Track) -> Self {
        Self {
            lane: track.lane.name().to_owned(),
            machine: track.machine.as_ref().map(|device| Machine {
                kind: device.kind.spec().prefix.to_owned(),
                overrides: pruned(device),
                sample: device.sample.clone(),
                slices: device.slices.clone(),
                pads: device.pads.clone(),
                spectral: device.spectral.clone(),
            }),
            sections: track
                .strip
                .iter()
                .filter_map(|device| {
                    let DeviceKind::Console(kind) = device.kind else {
                        return None;
                    };
                    Some(Section {
                        kind,
                        in_: !device.bypassed || kind.always_in(),
                        overrides: pruned(device),
                    })
                })
                .collect(),
        }
    }
}

impl Sound {
    /// The machine this sound names, as a device with no identity (id
    /// zero): what the compiler plays a sound-locked step through. `None`
    /// when the sound has no machine or names one this build lacks.
    pub fn machine_device(&self) -> Option<Device> {
        let machine = self.machine.as_ref()?;
        let spec = crate::devices::device_by_prefix(&machine.kind)?;
        if !spec.kind.is_instrument() {
            return None;
        }
        let mut device = Device::new(DeviceId(0), spec.kind);
        for (param, value) in &machine.overrides {
            device.set(*param, *value);
        }
        device.sample = machine.sample.clone();
        device.slices = machine.slices.clone();
        device.pads = machine.pads.clone();
        device.spectral = machine.spectral.clone();
        Some(device)
    }
}

/// A device's overrides without the ones equal to the table default.
/// `Device::set` never removes an entry, so a knob turned away and back
/// leaves a no-op behind; the file does not carry it.
fn pruned(device: &Device) -> Vec<(u32, f32)> {
    let table = device.table();
    device
        .overrides
        .iter()
        .filter(|(id, value)| {
            table
                .iter()
                .find(|def| def.id == *id)
                .is_none_or(|def| def.default != *value)
        })
        .copied()
        .collect()
}

/// The library: `~/Corpus/daw/sounds`.
pub fn dir() -> PathBuf {
    crate::corpus::dir().join("daw").join("sounds")
}

/// Whether `name` can be a file stem the library files and the palette
/// spells back: letters, digits, `-`, `_` and single spaces, not empty,
/// not starting with a dot.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && !name.contains("  ")
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | ' '))
}

/// Where `name` under `lane` lives in `dir`.
pub fn path_of(dir: &Path, lane: &str, name: &str) -> PathBuf {
    dir.join(lane).join(format!("{name}.{EXTENSION}"))
}

/// Write `sound` as `name` under its lane's folder. Atomic: a temp
/// sibling, synced, then renamed over the target, the way the document
/// is saved, so a crash mid-write leaves the old sound or none.
pub fn save(dir: &Path, name: &str, sound: &Sound) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!("not a sound name: {name:?}"));
    }
    let path = path_of(dir, &sound.lane, name);
    let file = SoundFile {
        version: VERSION,
        sound: sound.clone(),
    };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())?;
    let folder = path
        .parent()
        .ok_or_else(|| "sound path has no folder".to_owned())?;
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temp = path.with_file_name(format!(".{name}.{}.{nonce}.writing", std::process::id()));
    let write = (|| {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
        file.write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&temp, &path).map_err(|error| error.to_string())
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write.map(|()| path)
}

/// Read one sound. A newer frame is refused without being guessed at.
pub fn load(path: &Path) -> Result<Sound, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: SoundFile = ron::from_str(&text).map_err(|error| error.to_string())?;
    if file.version > VERSION {
        return Err(format!(
            "sound version {} is newer than this build's {VERSION}",
            file.version
        ));
    }
    Ok(file.sound)
}

/// Move `old` to `new` under `lane`. Refuses to overwrite.
pub fn rename(dir: &Path, lane: &str, old: &str, new: &str) -> Result<PathBuf, String> {
    if !valid_name(new) {
        return Err(format!("not a sound name: {new:?}"));
    }
    let from = path_of(dir, lane, old);
    let to = path_of(dir, lane, new);
    if !from.is_file() {
        return Err(format!("no sound called {old:?} under {lane}"));
    }
    if to.exists() {
        return Err(format!(
            "a sound called {new:?} already exists under {lane}"
        ));
    }
    std::fs::rename(&from, &to).map_err(|error| error.to_string())?;
    Ok(to)
}

/// Every sound the library holds, sorted by lane then name. A missing
/// library is an empty one.
pub fn list(dir: &Path) -> Vec<SoundRecord> {
    let mut out = Vec::new();
    let Ok(lanes) = std::fs::read_dir(dir) else {
        return out;
    };
    for lane in lanes.flatten() {
        let folder = lane.path();
        if !folder.is_dir() {
            continue;
        }
        let lane = lane.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(&folder) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(&format!(".{EXTENSION}")))
            else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            out.push(SoundRecord {
                lane: lane.clone(),
                name: name.to_owned(),
                path,
            });
        }
    }
    out.sort();
    out
}

/// The starter sound for `lane`, if the library has one.
pub fn starter(dir: &Path, lane: &str) -> Option<PathBuf> {
    let path = path_of(dir, lane, STARTER);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::Lane;
    use crate::params::drum as dp;
    use crate::sequencing::Song;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("daw-sound-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A drum track with a kit, a turned knob, a pad and a moved section.
    fn dressed() -> Song {
        let mut song = Song::default();
        song.tracks[0].machine = None;
        song.set_lane(0, Lane::Drum);
        let id = song.tracks[0].machine.as_ref().expect("drum").id;
        let kit = song.device_mut(id).expect("drum device");
        assert_eq!(kit.kind, DeviceKind::Drum);
        kit.set(dp::TUNE, 3.0);
        kit.pads = vec![PathBuf::from("/abs/kick.wav"); 2];
        let door = song.tracks[0]
            .strip
            .iter_mut()
            .find(|device| device.kind == DeviceKind::Console(SectionKind::Door))
            .expect("door");
        door.bypassed = false;
        let first = door.table()[0];
        door.set(first.id, first.max);
        song
    }

    #[test]
    fn a_sound_round_trips_through_its_file() {
        let dir = scratch("round-trip");
        let song = dressed();
        let sound = Sound::capture(&song.tracks[0]);
        let path = save(&dir, "big room", &sound).expect("saves");
        assert_eq!(path, dir.join("drum").join("big room.sound.ron"));
        let back = load(&path).expect("loads");
        assert_eq!(back, sound);

        let mut fresh = Song::default();
        let log = fresh.load_sound(0, &back);
        assert!(log.iter().any(|line| line.contains("drum")), "{log:?}");
        let track = &fresh.tracks[0];
        assert_eq!(track.lane, Lane::Drum);
        let kit = track.machine.as_ref().expect("drum landed");
        assert_eq!(kit.kind, DeviceKind::Drum);
        assert_eq!(kit.value(dp::TUNE), 3.0);
        assert_eq!(kit.pads.len(), 2, "paths travel whatever the machine");
        let door = track
            .strip
            .iter()
            .find(|device| device.kind == DeviceKind::Console(SectionKind::Door))
            .expect("door");
        assert!(!door.bypassed);
        let first = door.table()[0];
        assert_eq!(door.value(first.id), first.max);
        assert_eq!(Sound::capture(track), sound);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_value_equal_to_the_default_is_not_written() {
        let mut song = dressed();
        let id = song.tracks[0].machine.as_ref().expect("kit").id;
        let kit = song.device_mut(id).expect("kit");
        let def = kit.table()[0];
        kit.set(def.id, def.default);
        let sound = Sound::capture(&song.tracks[0]);
        let machine = sound.machine.expect("machine");
        assert!(machine.overrides.iter().all(|(param, _)| *param != def.id));
        assert!(
            machine
                .overrides
                .iter()
                .any(|(param, _)| *param == dp::TUNE)
        );
    }

    #[test]
    fn unknown_things_are_logged_and_never_refuse() {
        let mut sound = Sound::capture(&dressed().tracks[0]);
        sound.machine.as_mut().expect("machine").kind = "theremin".to_owned();
        sound.sections[0].overrides.push((9_999, 1.0));
        let mut song = Song::default();
        let log = song.load_sound(0, &sound);
        assert!(song.tracks[0].machine.is_none());
        assert!(log.iter().any(|line| line.contains("theremin")), "{log:?}");
        assert!(log.iter().any(|line| line.contains("9999")), "{log:?}");

        let mut lane_less = sound.clone();
        lane_less.lane = "tuba".to_owned();
        let mut song = Song::default();
        let log = song.load_sound(0, &lane_less);
        assert_eq!(song.tracks[0].lane, Lane::Plain);
        assert!(log.iter().any(|line| line.contains("tuba")), "{log:?}");
    }

    #[test]
    fn the_library_lists_by_lane_then_name_and_finds_the_starter() {
        let dir = scratch("list");
        let mut sound = Sound::capture(&Song::default().tracks[0]);
        assert_eq!(sound.lane, "plain");
        save(&dir, "zed", &sound).expect("saves");
        save(&dir, "alpha", &sound).expect("saves");
        sound.lane = "drum".to_owned();
        save(&dir, STARTER, &sound).expect("saves");
        std::fs::write(dir.join("drum").join("notes.txt"), "x").expect("stray file");
        let names: Vec<(String, String)> = list(&dir)
            .into_iter()
            .map(|record| (record.lane, record.name))
            .collect();
        assert_eq!(
            names,
            vec![
                ("drum".to_owned(), "starter".to_owned()),
                ("plain".to_owned(), "alpha".to_owned()),
                ("plain".to_owned(), "zed".to_owned()),
            ]
        );
        assert!(starter(&dir, "drum").is_some());
        assert!(starter(&dir, "plain").is_none());
        assert!(list(&dir.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_moves_the_file_and_refuses_to_overwrite() {
        let dir = scratch("rename");
        let sound = Sound::capture(&Song::default().tracks[0]);
        save(&dir, "one", &sound).expect("saves");
        save(&dir, "two", &sound).expect("saves");
        assert!(rename(&dir, "plain", "one", "two").is_err());
        assert!(rename(&dir, "plain", "one", "bad/name").is_err());
        let moved = rename(&dir, "plain", "one", "three").expect("renames");
        assert!(moved.is_file());
        assert!(!path_of(&dir, "plain", "one").exists());
        assert!(rename(&dir, "plain", "one", "four").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_stems_the_palette_can_spell() {
        for good in ["kick", "big room", "808-a", "x_1"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "",
            ".hidden",
            "a/b",
            " lead",
            "lead ",
            "two  spaces",
            "a\tb",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert!(save(Path::new("/nonexistent"), "a/b", &Sound::default()).is_err());
    }

    #[test]
    fn a_newer_frame_is_refused_with_words() {
        let dir = scratch("newer");
        std::fs::create_dir_all(dir.join("plain")).expect("dir");
        let path = path_of(&dir, "plain", "future");
        std::fs::write(
            &path,
            format!("(version: {}, sound: (lane: \"plain\"))", VERSION + 1),
        )
        .expect("write");
        let error = load(&path).expect_err("refused");
        assert!(error.contains("newer"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
