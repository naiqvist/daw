//! The chain band — a track's devices, and every parameter each one has.
//!
//! One column per device, in signal order, left to right. A column is not
//! a summary: it carries the device's WHOLE parameter table as a list, and
//! the list scrolls. That is the deal the card tier makes — every value
//! reachable by keyboard on a uniform surface, with the spatial views that
//! some devices deserve living somewhere else and editing the same values.
//!
//! What a column draws comes from the catalog rather than from anything
//! written here: `DeviceSpec.params` is the engine's table (ids, ranges,
//! defaults) and `DeviceSpec.labels` is the parallel table of words (name,
//! unit, group). Every device in the app already declares both, which is
//! why this surface can exist for all of them at once instead of being
//! hand-laid-out thirty-nine times.

use crate::design::codex::ParamFamily;
use crate::devices::{DeviceKind, Family, ParamLabel};
use crate::params::ParamDef;
use crate::sequencing::{Device, Song};

/// One parameter, as the band draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The word the catalog puts on this parameter, qualified by its
    /// group where the bare word would be ambiguous.
    pub name: String,
    /// Its value, with the catalog's unit appended.
    pub value: String,
    /// Whether the value differs from the kind's default — the one fact a
    /// list of forty numbers cannot otherwise give you at a glance: which
    /// of them somebody actually moved.
    pub edited: bool,
    /// What kind of control this is, before its name.
    pub family: ParamFamily,
    /// Where the value sits in its range, 0..1. A gauge's fill.
    pub place: f32,
    /// How many positions a LIST has; zero for a range.
    pub choices: usize,
    /// Which position the value stands on, when it is a list.
    pub choice: usize,
}

/// The sampler facts that turn its chain column into an instrument face.
///
/// A generic device can be understood as a table of parameters. A sampler
/// cannot: its material, trim, loop, and cuts are the thing being played.
/// This small projection keeps the Stage view out of the document model while
/// giving it enough authored truth to draw that material around the same
/// parameter rows every other card exposes.
#[derive(Clone, Debug, PartialEq)]
pub struct SamplerFace {
    /// Full path, used only to match the peak data the host has handed Stage.
    pub path: Option<std::path::PathBuf>,
    /// Classic, one-shot, or slice, as an index into `MODE_NAMES`.
    pub mode: usize,
    /// The part of the source that plays, as file fractions.
    pub start: f32,
    pub end: f32,
    /// Off, forward, or ping-pong, as an index into `LOOP_NAMES`.
    pub loop_mode: usize,
    /// A fraction of the trimmed region, not of the whole file.
    pub loop_start: f32,
    /// Grid, onset, or hand-authored, as an index into
    /// `SLICE_SOURCE_NAMES`.
    pub slice_source: usize,
    /// The grid count used when no authored table has replaced it.
    pub slice_count: usize,
    /// Authored slice starts, already normalized by the song model.
    pub slices: Vec<f64>,
    pub reversed: bool,
}

impl SamplerFace {
    fn from_device(device: &Device) -> Self {
        use crate::params::sampler as sp;

        let choice = |value: f32, choices: usize| {
            if value.is_finite() {
                value.round().max(0.0) as usize
            } else {
                0
            }
            .min(choices.saturating_sub(1))
        };
        let fraction = |value: f32, fallback: f32| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                fallback
            }
        };

        Self {
            path: device.sample.clone(),
            mode: choice(device.value(sp::MODE), sp::MODE_NAMES.len()),
            start: fraction(device.value(sp::START), 0.0),
            end: fraction(device.value(sp::END), 1.0),
            loop_mode: choice(device.value(sp::LOOP_MODE), sp::LOOP_NAMES.len()),
            loop_start: fraction(device.value(sp::LOOP_START), 0.0),
            slice_source: choice(device.value(sp::SLICE_SOURCE), sp::SLICE_SOURCE_NAMES.len()),
            slice_count: if device.value(sp::SLICES).is_finite() {
                device.value(sp::SLICES).round().clamp(1.0, sp::SLICES_MAX) as usize
            } else {
                1
            },
            slices: device.slices.clone(),
            reversed: device.value(sp::REVERSE) >= 0.5,
        }
    }
}

/// Which rail of the desk a column stands on.
///
/// The band is the whole signal path, not just the track's own part of
/// it: after the strip's OUT come the group bus's sections, then the
/// mix's, so walking right along the band is walking the signal from
/// the instrument to the speakers. The two returns are the one thing
/// that is NOT in series — they are fed by the sends and they land back
/// in the mix — so they stand apart at the end of the walk and their
/// cables are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// The track's own: its chain, then its channel strip.
    Channel,
    /// The group bus this track feeds, by index.
    Bus(usize),
    /// The mix bus every group feeds.
    Mix,
    /// A return, by index. Fed by a send, not by the path.
    Return(usize),
}

impl Lane {
    /// The rail's name, engraved where the band crosses from one rail
    /// to the next.
    pub fn seal(self, song: &Song) -> Option<String> {
        match self {
            Lane::Channel => None,
            Lane::Bus(index) => song.console.buses.get(index).map(|rail| rail.name.clone()),
            Lane::Mix => Some(song.console.mix.name.clone()),
            Lane::Return(index) => song.console.aux.get(index).map(|rail| rail.name.clone()),
        }
    }

    /// Two columns on rails that are IN SERIES mate: a channel's OUT
    /// tongue lies in its bus's GLUE notch, and the bus's IRON in the
    /// mix's. A return mates with nothing — it is a parallel path, and
    /// standing apart is how the eye is told so.
    pub fn in_series(self) -> bool {
        !matches!(self, Lane::Return(_))
    }
}

/// What an sCOMP card draws from: the knobs as the device holds them,
/// and the key of the take they bake, so a card can tell whether the
/// picture it cached is still the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct ScompFace {
    pub params: crate::scomp::ScompParams,
    pub key: crate::scomp::Baked,
}

impl ScompFace {
    pub fn from_device(device: &Device) -> Self {
        let mut params = crate::scomp::ScompParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        Self {
            key: params.baked(),
            params,
        }
    }
}

/// What a STAB card draws from: the knobs as the device holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct StabFace {
    pub params: crate::audio::stab::StabParams,
}

impl StabFace {
    pub fn from_device(device: &Device) -> Self {
        let mut params = crate::audio::stab::StabParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        Self { params }
    }

    /// The chord's notes as semitones above the played key.
    pub fn notes(&self) -> ([i32; crate::params::stab::NOTES], usize) {
        self.params.voicing()
    }

    /// The chord as a keyboard player names it, on C: `Cm7 / Eb`.
    pub fn word(&self) -> String {
        const NAMES: [&str; 12] = [
            "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
        ];
        let chord = self.params.chord.round().max(0.0) as usize;
        let name = crate::params::stab::CHORD_NAMES
            .get(chord)
            .copied()
            .unwrap_or("?");
        let (notes, count) = self.notes();
        let bass = notes.first().copied().unwrap_or(0).rem_euclid(12) as usize;
        let mut word = format!("C{name}");
        if count > 0 && bass != 0 {
            word.push_str(&format!(" / {}", NAMES[bass]));
        }
        word
    }
}

/// What a QUAD card draws from: the knobs as the device holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadFace {
    pub params: crate::audio::quad::QuadParams,
}

impl QuadFace {
    pub fn from_device(device: &Device) -> Self {
        let mut params = crate::audio::quad::QuadParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        Self { params }
    }
}

/// What a BRICK card draws from: the knobs as the device holds them,
/// and the file it hits.
#[derive(Clone, Debug, PartialEq)]
pub struct BrickFace {
    pub params: crate::audio::brick::BrickParams,
    pub path: Option<std::path::PathBuf>,
}

impl BrickFace {
    pub fn from_device(device: &Device) -> Self {
        let mut params = crate::audio::brick::BrickParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        Self {
            params,
            path: device.sample.clone(),
        }
    }
}

/// One device, as a column of the band.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: &'static str,
    /// The catalog's compact machine address. It is stable enough for
    /// automation targets and short enough to cut into a card header.
    pub code: &'static str,
    /// The browser family whose seal heads the card.
    pub family: Family,
    /// Whether it heads the chain rather than shaping what comes in.
    pub instrument: bool,
    pub bypassed: bool,
    /// The file a sampler is playing, by name. `None` on every other
    /// kind, and on a sampler with nothing loaded yet — which is the one
    /// fact about a sampler its parameter table cannot show.
    pub sample: Option<String>,
    /// The material-facing state for a sampler card; absent on every other
    /// device. Kept beside `sample` because the latter remains the compact
    /// header label shared by the generic card contract.
    pub sampler: Option<SamplerFace>,
    /// The bounce-facing state for an sCOMP card; absent on every other
    /// device.
    pub scomp: Option<ScompFace>,
    /// The chord-facing state for a STAB card; absent on every other
    /// device.
    pub stab: Option<StabFace>,
    /// The routing-facing state for a QUAD card; absent on every other
    /// device.
    pub quad: Option<QuadFace>,
    /// The hit-facing state for a BRICK card; absent on every other
    /// device.
    pub brick: Option<BrickFace>,
    pub rows: Vec<Row>,
    /// Which section of the console this column is, when it is one:
    /// drawn as a piece of the strip rather than as a card.
    pub section: Option<crate::console::SectionKind>,
    /// Which rail it stands on.
    pub lane: Lane,
}

/// The devices on `track`'s chain, in signal order. New tracks include
/// exact-unity input and output gain utilities around their editable chain.
pub fn columns(song: &Song, track: usize) -> Vec<Column> {
    song.tracks
        .get(track)
        .map(|track| track.chain.iter().map(column).collect())
        .unwrap_or_default()
}

/// The whole signal path as a run of devices, each with the rail it
/// stands on: the track's chain, its channel strip, the group bus it
/// feeds, the mix, and last the two returns.
///
/// One order, used by the drawing, by the cursor's lattice and by the
/// lookup from a column to its device — so the three can never disagree
/// about what the cursor is standing on.
pub fn band_devices(song: &Song, track: usize) -> Vec<(Lane, &Device)> {
    let Some(lane) = song.tracks.get(track) else {
        return Vec::new();
    };
    let mut out: Vec<(Lane, &Device)> = lane
        .chain
        .iter()
        .chain(lane.strip.iter())
        .map(|device| (Lane::Channel, device))
        .collect();
    let bus = lane.bus as usize;
    if let Some(rail) = song.console.buses.get(bus) {
        out.extend(rail.sections.iter().map(|device| (Lane::Bus(bus), device)));
    }
    out.extend(
        song.console
            .mix
            .sections
            .iter()
            .map(|device| (Lane::Mix, device)),
    );
    for (index, rail) in song.console.aux.iter().enumerate() {
        out.extend(
            rail.sections
                .iter()
                .map(|device| (Lane::Return(index), device)),
        );
    }
    out
}

/// The device a band column addresses, by the same order the band is
/// laid in.
pub fn device_at(song: &Song, track: usize, col: usize) -> Option<crate::sequencing::DeviceId> {
    band_devices(song, track)
        .get(col)
        .map(|(_, device)| device.id)
}

/// The whole band, drawn. Never empty for a track that exists — the
/// strip and the desk are always there — and empty for one that does
/// not.
pub fn band(song: &Song, track: usize) -> Vec<Column> {
    band_devices(song, track)
        .into_iter()
        .map(|(lane, device)| {
            let mut column = column(device);
            column.lane = lane;
            column
        })
        .collect()
}

/// One device reduced to what the band draws.
pub fn column(device: &Device) -> Column {
    let spec = device.kind.spec();
    Column {
        title: device.role.title().unwrap_or(spec.name),
        code: device.role.code().unwrap_or(spec.prefix),
        family: spec.family,
        instrument: spec.instrument,
        bypassed: device.bypassed,
        sample: device
            .sample
            .as_deref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned()),
        sampler: (device.kind == DeviceKind::Sampler).then(|| SamplerFace::from_device(device)),
        scomp: (device.kind == DeviceKind::Scomp).then(|| ScompFace::from_device(device)),
        stab: (device.kind == DeviceKind::Stab).then(|| StabFace::from_device(device)),
        quad: (device.kind == DeviceKind::Quad).then(|| QuadFace::from_device(device)),
        brick: (device.kind == DeviceKind::Brick).then(|| BrickFace::from_device(device)),
        section: match device.kind {
            crate::devices::DeviceKind::Console(kind) => Some(kind),
            _ => None,
        },
        lane: Lane::Channel,
        // The two tables are parallel by construction — the automation
        // target picker already walks them zipped — so a row is one
        // parameter's numbers beside its words.
        rows: numbered(
            spec.params
                .iter()
                .zip(spec.labels)
                .map(|(def, label)| {
                    let value = device.value(def.id);
                    Row {
                        name: qualified(label, spec.labels),
                        value: format_param(def, label, value),
                        edited: value != def.default,
                        family: family_of(def, label),
                        place: place_of(def, value),
                        choices: label.choices.len(),
                        choice: choice_index(def, label, value).unwrap_or(0),
                    }
                })
                .collect(),
        ),
    }
}

/// The last resort: number any rows that STILL read the same.
///
/// The catalog can be ambiguous in a way no rule over it can repair —
/// `loom` has two parameters called Morph, in one group — and a list with
/// two identical rows is a list where one of them cannot be aimed at. A
/// number is a poor name and an honest one; the real fix is in the
/// catalog, and this makes the surface usable until it happens.
fn numbered(mut rows: Vec<Row>) -> Vec<Row> {
    let mut counts = std::collections::HashMap::new();
    for row in &rows {
        *counts.entry(row.name.clone()).or_insert(0usize) += 1;
    }
    let mut seen = std::collections::HashMap::new();
    for row in &mut rows {
        if counts.get(&row.name).copied().unwrap_or(0) > 1 {
            let nth = seen.entry(row.name.clone()).or_insert(0usize);
            *nth += 1;
            row.name = format!("{} {nth}", row.name);
        }
    }
    rows
}

/// A parameter's name, qualified by its group ONLY where it has to be.
///
/// A synth with two oscillators has two parameters called Wave, and a list
/// showing both as "Wave" is a list that cannot be used — the second one
/// is not a duplicate, it is a different oscillator. But qualifying every
/// row would put "Amp" in front of "Attack" for the one envelope that
/// needs no disambiguating, and a column this narrow cannot afford words
/// that carry nothing.
///
/// So the group is spent exactly where the bare name repeats, which is
/// what the group field is for.
fn qualified(label: &crate::devices::ParamLabel, all: &[crate::devices::ParamLabel]) -> String {
    let same: Vec<&crate::devices::ParamLabel> = all
        .iter()
        .filter(|other| other.name == label.name)
        .collect();
    if same.len() <= 1 || label.group.is_empty() {
        return label.name.to_owned();
    }
    // The group's LAST WORD first: "A Wave" against "B Wave" is the whole
    // distinction, and a narrow column has room for a letter rather than
    // a phrase. But two groups can share it — "Amp Env" and "Filter Env"
    // both end in Env — and a short name that still collides has bought
    // nothing, so those spend the whole group.
    let short = |group: &str| group.rsplit(' ').next().unwrap_or(group).to_owned();
    let mine = short(label.group);
    let distinct = same
        .iter()
        .filter(|other| short(other.group) == mine)
        .count()
        == 1;
    if distinct {
        format!("{mine} {}", label.name)
    } else {
        format!("{} {}", label.group, label.name)
    }
}

/// A parameter's value as the band writes it.
///
/// Precision follows MAGNITUDE rather than being fixed: a cutoff in
/// thousands of hertz has no use for two decimal places, and a mix at 0.35
/// is nothing without them. The column is monospace, so what matters is
/// that the digits land in the same place, not that every value spends the
/// same number of them.
pub fn format_value(value: f32, unit: &str) -> String {
    let magnitude = value.abs();
    let number = if !value.is_finite() {
        "--".to_owned()
    } else if magnitude >= 100.0 {
        format!("{value:.0}")
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}

/// A parameter's value as the band writes it: the NAME of its position
/// when the catalog says it is a list, and the number with its unit when
/// it is a range. "saw" is what an oscillator is doing; "2.00" is the
/// wire it rides on.
pub fn format_param(def: &ParamDef, label: &ParamLabel, value: f32) -> String {
    match choice_of(def, label, value) {
        Some(name) => name.to_owned(),
        None => format_value(value, label.unit),
    }
}

/// The name of the position `value` stands on, when the parameter is a
/// list. Positions are `min + n`, rounded, and clamped to the list — the
/// same single arithmetic step the compiler takes to turn an index back
/// into a choice.
fn choice_of(def: &ParamDef, label: &ParamLabel, value: f32) -> Option<&'static str> {
    choice_index(def, label, value).map(|at| label.choices[at])
}

fn choice_index(def: &ParamDef, label: &ParamLabel, value: f32) -> Option<usize> {
    if label.choices.is_empty() {
        return None;
    }
    let at = (value - def.min).round().max(0.0) as usize;
    Some(at.min(label.choices.len() - 1))
}

/// Where `value` sits in the parameter's range, 0..1.
pub fn place_of(def: &ParamDef, value: f32) -> f32 {
    let span = def.max - def.min;
    if span <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    ((value - def.min) / span).clamp(0.0, 1.0)
}

/// What kind of control a parameter is, read from the catalog's words:
/// a list is a SHAPE; a rate, delay or tuning is TIME; a gain or amount is
/// LEVEL, unless it swings both ways, which makes it MODULATION; a ratio
/// is MODULATION too. The unit is the tell, and the catalog already
/// carries it.
pub fn family_of(def: &ParamDef, label: &ParamLabel) -> ParamFamily {
    if !label.choices.is_empty() {
        return ParamFamily::Shape;
    }
    let bipolar = def.min < 0.0 && def.max > 0.0;
    match label.unit.trim() {
        "Hz" | "ms" | "s" | "st" | "ct" => ParamFamily::Time,
        "dB/oct" => ParamFamily::Shape,
        ":1" => ParamFamily::Modulation,
        _ if bipolar => ParamFamily::Modulation,
        _ => ParamFamily::Level,
    }
}

/// Maximal runs of consecutive rows in one family, in row order. A card
/// draws one rail segment per run, with the family's sign once at its
/// top — catalog order is kept, because the cursor addresses rows by
/// position.
pub fn family_runs(rows: &[Row]) -> Vec<(ParamFamily, std::ops::Range<usize>)> {
    let mut runs: Vec<(ParamFamily, std::ops::Range<usize>)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        match runs.last_mut() {
            Some((family, range)) if *family == row.family => range.end = i + 1,
            _ => runs.push((row.family, i..i + 1)),
        }
    }
    runs
}

/// One press of a parameter.
///
/// For a RANGE: a hundredth of it, or a tenth when the press is coarse.
/// For a LIST: one position, whichever press — a list has no "coarse"
/// walk, and a press that skipped entries would be a press whose landing
/// could not be predicted. The catalog says which is which; the shape of
/// a range cannot be read to find out, and this surface no longer tries.
pub fn step_of(def: &ParamDef, label: &ParamLabel, coarse: bool) -> f32 {
    if !label.choices.is_empty() {
        return 1.0;
    }
    let span = def.max - def.min;
    if span <= 0.0 {
        return 0.0;
    }
    if coarse { span / 10.0 } else { span / 100.0 }
}

/// How many parameter rows fit in `room` at `pitch`, and only whole ones.
pub fn rows_that_fit(room: f32, pitch: f32) -> usize {
    if pitch <= 0.0 || room < pitch {
        return 0;
    }
    (room / pitch).floor() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceKind;
    use crate::sequencing::TrackKind;

    fn song_with(kind: DeviceKind) -> (Song, usize) {
        let mut song = Song::default();
        song.add_device(0, kind).expect("the device goes on");
        (song, 0)
    }

    fn added_id(song: &Song, kind: DeviceKind) -> crate::sequencing::DeviceId {
        song.tracks[0]
            .chain
            .iter()
            .find(|device| device.kind == kind && device.role.is_normal())
            .expect("the added device")
            .id
    }

    fn added_column(song: &Song, kind: DeviceKind) -> Column {
        column(song.device(added_id(song, kind)).expect("the added device"))
    }

    #[test]
    fn a_column_carries_the_whole_table_and_not_a_selection() {
        // The claim the card tier makes: everything is reachable here.
        for kind in [DeviceKind::Poly, DeviceKind::Reverb, DeviceKind::Sat] {
            let (song, track) = song_with(kind);
            let column = added_column(&song, kind);
            assert_eq!(
                column.rows.len(),
                kind.spec().params.len(),
                "{} showed {} of its {} parameters",
                kind.spec().name,
                column.rows.len(),
                kind.spec().params.len()
            );
            assert!(column.rows.iter().all(|row| !row.name.is_empty()));
        }
    }

    #[test]
    fn a_row_says_whether_anybody_moved_it() {
        let (mut song, track) = song_with(DeviceKind::Sat);
        let at_rest = added_column(&song, DeviceKind::Sat);
        assert!(
            at_rest.rows.iter().all(|row| !row.edited),
            "a device fresh from the catalog claimed edits"
        );

        let id = added_id(&song, DeviceKind::Sat);
        let drive = crate::params::sat::DRIVE;
        let def = *crate::params::sat::TABLE
            .iter()
            .find(|def| def.id == drive)
            .expect("sat has drive");
        song.device_mut(id)
            .expect("there")
            .set(drive, def.default + (def.max - def.default) / 2.0);

        let edited = added_column(&song, DeviceKind::Sat);
        let moved: Vec<&str> = edited
            .rows
            .iter()
            .filter(|row| row.edited)
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(moved.len(), 1, "the wrong number of rows read as edited");
    }

    #[test]
    fn a_repeated_name_is_told_apart_and_a_unique_one_is_left_alone() {
        let (song, track) = song_with(DeviceKind::Poly);
        let column = added_column(&song, DeviceKind::Poly);
        let names: Vec<&str> = column.rows.iter().map(|row| row.name.as_str()).collect();

        // Two oscillators, each with a Wave — and they must not read the
        // same, or the list cannot be used.
        assert!(
            names.contains(&"A Wave"),
            "osc A's wave was not told apart: {names:?}"
        );
        assert!(
            names.contains(&"B Wave"),
            "osc B's wave was not told apart: {names:?}"
        );
        assert!(!names.contains(&"Wave"), "an ambiguous name survived");

        // Every name in the list is now distinct.
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(*name), "two rows still read as {name}");
        }

        // And a name that never repeated is left as it was.
        let (song, track) = song_with(DeviceKind::Sat);
        let column = added_column(&song, DeviceKind::Sat);
        assert!(
            column.rows.iter().any(|row| row.name == "Drive"),
            "a unique name was qualified for nothing"
        );
    }

    /// Whatever the rule does, no device may end up with two rows a
    /// reader cannot tell apart. Checked across the whole catalog,
    /// because this is the kind of thing one device always breaks.
    #[test]
    fn no_device_in_the_app_shows_two_rows_that_read_the_same() {
        for spec in crate::devices::DEVICES {
            let mut song = Song::default();
            let Some(id) = song.add_device(0, spec.kind) else {
                continue;
            };
            let column = column(song.device(id).expect("there"));
            let mut seen = std::collections::HashSet::new();
            for row in &column.rows {
                assert!(
                    seen.insert(row.name.clone()),
                    "{} shows two rows called {}",
                    spec.name,
                    row.name
                );
            }
        }
    }

    #[test]
    fn a_column_says_what_kind_of_thing_it_is() {
        let (song, track) = song_with(DeviceKind::Poly);
        let column = added_column(&song, DeviceKind::Poly);
        assert_eq!(column.title, DeviceKind::Poly.spec().name);
        assert_eq!(column.code, DeviceKind::Poly.spec().prefix);
        assert!(column.instrument, "an instrument did not say so");
        assert!(!column.bypassed);
    }

    /// The band does not stop at the channel. It is the whole path:
    /// the chain, the strip, the group bus this track feeds, the mix,
    /// and last the two returns — which are the one part of it that is
    /// NOT in series, and say so by standing apart.
    #[test]
    fn the_band_is_the_whole_signal_path() {
        use crate::console::SectionKind;
        let (song, track) = song_with(DeviceKind::Acid);
        let band = band(&song, track);
        let lanes: Vec<Lane> = band.iter().map(|column| column.lane).collect();
        let channel = lanes.iter().filter(|lane| **lane == Lane::Channel).count();
        assert_eq!(
            channel,
            3 + SectionKind::STRIP.len(),
            "the instrument, boundary gains, and strip are the channel's own"
        );
        // The bus this track feeds, then the mix, then the returns —
        // in that order, because that is the order the signal takes.
        let bus = song.tracks[track].bus as usize;
        let rest: Vec<Lane> = lanes[channel..].to_vec();
        let mut want = vec![Lane::Bus(bus); SectionKind::BUS.len()];
        want.extend(vec![Lane::Mix; SectionKind::MIX.len()]);
        want.push(Lane::Return(0));
        want.push(Lane::Return(1));
        assert_eq!(rest, want);
        // Every column of it addresses a real device, once.
        let mut ids: Vec<u64> = (0..band.len())
            .map(|col| device_at(&song, track, col).expect("a device").0)
            .collect();
        let all = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all, "the band addressed one device twice");
        assert_eq!(device_at(&song, track, band.len()), None);
    }

    /// A return is a parallel path, so it mates with nothing. Every
    /// other rail is in series and its pieces join.
    #[test]
    fn only_a_return_stands_off_the_rail() {
        assert!(Lane::Channel.in_series());
        assert!(Lane::Bus(2).in_series());
        assert!(Lane::Mix.in_series());
        assert!(!Lane::Return(0).in_series());
        assert!(!Lane::Return(1).in_series());
    }

    /// Every rail the band crosses onto can name itself, so the seam
    /// is never an unexplained gap.
    #[test]
    fn every_rail_the_band_crosses_onto_has_a_name() {
        let (song, track) = song_with(DeviceKind::Acid);
        for column in band(&song, track) {
            match column.lane {
                Lane::Channel => assert_eq!(column.lane.seal(&song), None),
                lane => {
                    let name = lane
                        .seal(&song)
                        .unwrap_or_else(|| panic!("{lane:?} unnamed"));
                    assert!(!name.is_empty(), "{lane:?} has an empty name");
                }
            }
        }
    }

    #[test]
    fn the_columns_are_the_chain_in_signal_order() {
        let mut song = Song::default();
        song.add_device(0, DeviceKind::Reverb).expect("effect");
        song.add_device(0, DeviceKind::Poly).expect("instrument");
        song.add_device(0, DeviceKind::Sat).expect("effect");
        let titles: Vec<&str> = columns(&song, 0)
            .iter()
            .map(|column| column.title)
            .collect();
        assert_eq!(
            titles,
            [
                DeviceKind::Poly.spec().name,
                "INPUT GAIN",
                DeviceKind::Reverb.spec().name,
                DeviceKind::Sat.spec().name,
                "OUTPUT GAIN",
            ],
            "the band did not draw the chain in the order the graph runs it"
        );
    }

    #[test]
    fn a_new_track_shows_its_boundary_gain_columns() {
        let song = Song::default();
        let titles: Vec<_> = columns(&song, 0)
            .iter()
            .map(|column| column.title)
            .collect();
        assert_eq!(titles, ["INPUT GAIN", "OUTPUT GAIN"]);
        assert!(
            columns(&song, 99).is_empty(),
            "a track that is not there drew"
        );
    }

    #[test]
    fn an_audio_track_shows_the_effects_it_is_allowed() {
        let mut song = Song::default();
        song.add_track(TrackKind::Audio);
        song.add_device(1, DeviceKind::Reverb).expect("effect");
        let columns = columns(&song, 1);
        assert_eq!(columns.len(), 3);
        assert!(columns.iter().all(|column| !column.instrument));
    }

    #[test]
    fn precision_follows_magnitude_and_the_unit_travels() {
        assert_eq!(format_value(0.35, ""), "0.35");
        assert_eq!(format_value(12.5, "ms"), "12.5 ms");
        assert_eq!(format_value(4200.0, "Hz"), "4200 Hz");
        assert_eq!(format_value(-6.0, "dB"), "-6.00 dB");
        // A value with nothing to say says nothing rather than "NaN".
        assert_eq!(format_value(f32::INFINITY, "dB"), "-- dB");
    }

    #[test]
    fn every_parameter_in_the_app_has_a_step_that_crosses_its_range() {
        // A step of zero is a parameter nobody can move, and a step
        // bigger than the range is one with two positions.
        for spec in crate::devices::DEVICES {
            for (def, label) in spec.params.iter().zip(spec.labels) {
                let span = def.max - def.min;
                let fine = step_of(def, label, false);
                let coarse = step_of(def, label, true);
                assert!(
                    fine > 0.0 && coarse > 0.0,
                    "{} / {} cannot be moved at all",
                    spec.name,
                    def.name
                );
                if !label.choices.is_empty() {
                    // A list is walked one position at a time, whichever
                    // press: there is nothing between two entries to land
                    // on, and nothing past the next one to skip to.
                    assert_eq!(fine, 1.0, "{} / {}", spec.name, def.name);
                    assert_eq!(coarse, 1.0, "{} / {}", spec.name, def.name);
                    continue;
                }
                assert!(
                    coarse > fine,
                    "{} / {} has no coarse press",
                    spec.name,
                    def.name
                );
                // A hundred presses crosses the range, and ten coarse ones.
                assert!((span / fine - 100.0).abs() < 0.01);
                assert!((span / coarse - 10.0).abs() < 0.01);
            }
        }
    }

    #[test]
    fn a_choice_is_shown_by_name_and_a_range_by_number() {
        let (mut song, track) = song_with(DeviceKind::Poly);
        let id = added_id(&song, DeviceKind::Poly);
        let wave = crate::params::poly::A_WAVE;
        song.device_mut(id).expect("there").set(wave, 2.0);
        let column = added_column(&song, DeviceKind::Poly);
        let row = column
            .rows
            .iter()
            .find(|row| row.name == "A Wave")
            .expect("the wave row");
        assert_eq!(row.value, "saw", "a list showed its wire value");
        assert!(row.edited);

        let gain = column
            .rows
            .iter()
            .find(|row| row.name == "Gain")
            .expect("the gain row");
        assert!(
            gain.value
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit()),
            "a range was not shown as a number: {}",
            gain.value
        );

        // A value between two positions reads as the nearer one, and one
        // past the end reads as the last: the band never shows a name
        // the engine will not play.
        let def = crate::params::def(crate::params::poly::TABLE, wave);
        let label = &DeviceKind::Poly.spec().labels[0];
        assert_eq!(format_param(def, label, 2.4), "saw");
        assert_eq!(format_param(def, label, 99.0), "air");
        assert_eq!(format_param(def, label, -3.0), "sine");
    }

    #[test]
    fn a_sampler_column_names_its_file() {
        let (mut song, track) = song_with(DeviceKind::Sampler);
        let bare = added_column(&song, DeviceKind::Sampler);
        assert_eq!(bare.sample, None);
        let id = added_id(&song, DeviceKind::Sampler);
        song.device_mut(id).expect("there").sample = Some("/kits/909/kick 01.wav".into());
        let loaded = added_column(&song, DeviceKind::Sampler);
        assert_eq!(loaded.sample.as_deref(), Some("kick 01.wav"));
    }

    #[test]
    fn a_sampler_column_carries_the_material_state_its_face_needs() {
        use crate::params::sampler as sp;

        let (mut song, _) = song_with(DeviceKind::Sampler);
        let id = added_id(&song, DeviceKind::Sampler);
        let device = song.device_mut(id).expect("the sampler");
        device.sample = Some("/kits/amen.wav".into());
        device.set(sp::MODE, sp::MODE_SLICE);
        device.set(sp::START, 0.125);
        device.set(sp::END, 0.875);
        device.set(sp::LOOP_MODE, sp::LOOP_PINGPONG);
        device.set(sp::LOOP_START, 0.4);
        device.set(sp::SLICE_SOURCE, sp::SLICE_CUSTOM);
        device.set(sp::REVERSE, 1.0);
        device.set_slices([0.0, 0.25, 0.625]);

        let column = added_column(&song, DeviceKind::Sampler);
        let face = column.sampler.expect("a dedicated sampler face");
        assert_eq!(
            face.path.as_deref(),
            Some(std::path::Path::new("/kits/amen.wav"))
        );
        assert_eq!(face.mode, sp::MODE_SLICE as usize);
        assert_eq!((face.start, face.end), (0.125, 0.875));
        assert_eq!(face.loop_mode, sp::LOOP_PINGPONG as usize);
        assert_eq!(face.loop_start, 0.4);
        assert_eq!(face.slice_source, sp::SLICE_CUSTOM as usize);
        assert_eq!(face.slices, [0.0, 0.25, 0.625]);
        assert!(face.reversed);
    }

    #[test]
    fn only_the_sampler_gets_a_material_face() {
        for kind in [DeviceKind::Poly, DeviceKind::Sat, DeviceKind::Utility] {
            let (song, _) = song_with(kind);
            assert!(
                added_column(&song, kind).sampler.is_none(),
                "{} grew a sampler face",
                kind.spec().name
            );
        }
    }

    #[test]
    fn a_family_is_read_off_the_catalogs_words() {
        let def = |min: f32, max: f32| ParamDef {
            id: 0,
            name: "x",
            min,
            max,
            default: min,
        };
        let label = |unit: &'static str, choices: &'static [&'static str]| ParamLabel {
            name: "x",
            unit,
            group: "",
            choices,
        };
        assert_eq!(
            family_of(&def(0.0, 1.0), &label("", &["a", "b"])),
            ParamFamily::Shape
        );
        assert_eq!(
            family_of(&def(20.0, 20000.0), &label("Hz", &[])),
            ParamFamily::Time
        );
        assert_eq!(
            family_of(&def(0.0, 1.0), &label(" ms", &[])),
            ParamFamily::Time
        );
        assert_eq!(
            family_of(&def(0.0, 100.0), &label("%", &[])),
            ParamFamily::Level
        );
        assert_eq!(
            family_of(&def(-1.0, 1.0), &label("", &[])),
            ParamFamily::Modulation
        );
        assert_eq!(
            family_of(&def(1.0, 20.0), &label(":1", &[])),
            ParamFamily::Modulation
        );
        assert_eq!(
            family_of(&def(6.0, 48.0), &label("dB/oct", &[])),
            ParamFamily::Shape
        );
        assert_eq!(place_of(&def(0.0, 10.0), 2.5), 0.25);
        assert_eq!(place_of(&def(0.0, 10.0), 99.0), 1.0);
        assert_eq!(place_of(&def(5.0, 5.0), 5.0), 0.0);
    }

    #[test]
    fn family_runs_keep_catalog_order_and_join_neighbours() {
        let row = |family: ParamFamily| Row {
            name: String::new(),
            value: String::new(),
            edited: false,
            family,
            place: 0.0,
            choices: 0,
            choice: 0,
        };
        let rows = [
            row(ParamFamily::Time),
            row(ParamFamily::Time),
            row(ParamFamily::Level),
            row(ParamFamily::Time),
        ];
        let runs = family_runs(&rows);
        assert_eq!(
            runs,
            vec![
                (ParamFamily::Time, 0..2),
                (ParamFamily::Level, 2..3),
                (ParamFamily::Time, 3..4)
            ]
        );
        assert!(family_runs(&[]).is_empty());
    }

    #[test]
    fn every_parameter_in_the_app_has_a_family() {
        // Every unit the catalog uses is one the rule knows, so no device
        // ends up with a card whose rows are unclassified by accident.
        for spec in crate::devices::DEVICES {
            for (def, label) in spec.params.iter().zip(spec.labels) {
                let _ = family_of(def, label);
                assert!((0.0..=1.0).contains(&place_of(def, def.default)));
            }
        }
    }

    #[test]
    fn only_whole_rows_fit() {
        assert_eq!(rows_that_fit(100.0, 20.0), 5);
        assert_eq!(rows_that_fit(99.0, 20.0), 4);
        assert_eq!(rows_that_fit(10.0, 20.0), 0, "half a row is not a row");
        assert_eq!(rows_that_fit(100.0, 0.0), 0);
    }
}
