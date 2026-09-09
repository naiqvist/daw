//! The eight parameter pages shared by every keyboard surface.
//!
//! This is green-zone catalog data.  It deliberately knows about neither
//! egui nor the audio graph: a surface resolves a track into subjects and
//! stable parameter ids here, then reads and writes the ordinary song model.

use crate::console::SectionKind;
use crate::devices::{DeviceKind, DeviceSpec};
use crate::sequencing::{TRACK_PAN, TRACK_VOLUME, Track};

/// The fixed row of function keys.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PageKey {
    Trig,
    Src,
    Fltr,
    Amp,
    Lfo,
    Fx,
    Mix,
    All,
}

impl PageKey {
    pub const ALL: [Self; 8] = [
        Self::Trig,
        Self::Src,
        Self::Fltr,
        Self::Amp,
        Self::Lfo,
        Self::Fx,
        Self::Mix,
        Self::All,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Trig => 0,
            Self::Src => 1,
            Self::Fltr => 2,
            Self::Amp => 3,
            Self::Lfo => 4,
            Self::Fx => 5,
            Self::Mix => 6,
            Self::All => 7,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Self::Trig => "TRIG",
            Self::Src => "SRC",
            Self::Fltr => "FLTR",
            Self::Amp => "AMP",
            Self::Lfo => "LFO",
            Self::Fx => "FX",
            Self::Mix => "MIX",
            Self::All => "",
        }
    }
}

/// The hero band's picture for a sub-page: plain data a machine computes
/// from its own parameters and the view draws. Every point is
/// normalised to the unit square; the labels say what the edges mean.
#[derive(Clone, Debug, PartialEq)]
pub struct Hero {
    pub waveform: Option<WaveHero>,
    pub title: String,
    pub series: Vec<HeroSeries>,
    pub marks: Vec<HeroMark>,
    /// The left and right edges of the x axis.
    pub x_labels: [String; 2],
    /// The bottom and top edges of the y axis.
    pub y_labels: [String; 2],
    /// Draw unity as a dashed diagonal: a transfer curve.
    pub diagonal: bool,
}

/// Plain waveform geometry shared by any material-based instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct WaveHero {
    pub columns: Vec<crate::sample_peaks::Bin>,
    pub overview: Vec<crate::sample_peaks::Bin>,
    pub view: (f32, f32),
    pub region: (f32, f32),
    pub loop_region: Option<(f32, f32)>,
    pub fade: f32,
    pub slices: Vec<f32>,
    pub handles: Vec<HeroHandle>,
    pub detail: String,
    pub tools: Vec<String>,
    pub playheads: Vec<(f32, bool)>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroHandle {
    pub param: u32,
    pub at: f32,
    pub value: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroHeight {
    Band,
    Tall,
}
pub fn hero_height(kind: DeviceKind, page: &str) -> HeroHeight {
    if kind == DeviceKind::Sampler
        && matches!(page, "Sample" | "Loop" | "Time" | "File" | "Loop detail")
    {
        HeroHeight::Tall
    } else {
        HeroHeight::Band
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroTool {
    pub key: crate::ui::stage::key::Key,
    pub word: &'static str,
    pub verb: u8,
}
pub fn hero_tools(kind: DeviceKind, page: &str) -> &'static [HeroTool] {
    use crate::ui::stage::key::Key;
    if hero_height(kind, page) != HeroHeight::Tall {
        return &[];
    }
    &[
        HeroTool {
            key: Key::Z,
            word: "ZOOM",
            verb: 0,
        },
        HeroTool {
            key: Key::G,
            word: "GRID",
            verb: 1,
        },
        HeroTool {
            key: Key::C,
            word: "DETECT",
            verb: 2,
        },
        HeroTool {
            key: Key::S,
            word: "SPLIT",
            verb: 3,
        },
        HeroTool {
            key: Key::M,
            word: "MERGE",
            verb: 4,
        },
        HeroTool {
            key: Key::F,
            word: "FIND LOOP",
            verb: 5,
        },
        HeroTool {
            key: Key::O,
            word: "FIND CYCLE",
            verb: 6,
        },
        HeroTool {
            key: Key::P,
            word: "PATTERN",
            verb: 7,
        },
        HeroTool {
            key: Key::T,
            word: "PROFILE",
            verb: 8,
        },
        HeroTool {
            key: Key::R,
            word: "VARIATE",
            verb: 9,
        },
        HeroTool {
            key: Key::B,
            word: "LOAD",
            verb: 10,
        },
        HeroTool {
            key: Key::U,
            word: "PRINT",
            verb: 11,
        },
    ]
}

/// One curve of a picture. A LIT series is the selected cell's: drawn
/// bright and washed to the baseline; the rest are thin lines.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroSeries {
    pub name: &'static str,
    pub points: Vec<(f32, f32)>,
    pub lit: bool,
}

/// A vertical mark on the x axis with a label.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroMark {
    pub x: f32,
    pub label: String,
    pub lit: bool,
}

/// One row of up to eight engine parameter ids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubPage {
    pub title: &'static str,
    pub slots: [Option<u32>; 8],
}

/// The rows one device contributes to a key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageSpec {
    pub key: PageKey,
    pub subpages: Vec<SubPage>,
}

/// The device instance a parameter id belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Subject {
    Machine,
    Section(SectionKind),
}

/// The eight pieces of step data on TRIG.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TrigField {
    Note,
    Velocity,
    Length,
    Micro,
    Probability,
    Condition,
    Retrig,
    Rate,
    /// The second TRIG sub-page: a whole sound locked on the step.
    Sound,
}

impl TrigField {
    pub const ALL: [Self; 8] = [
        Self::Note,
        Self::Velocity,
        Self::Length,
        Self::Micro,
        Self::Probability,
        Self::Condition,
        Self::Retrig,
        Self::Rate,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Velocity => "VEL",
            Self::Length => "LEN",
            Self::Micro => "MICRO",
            Self::Probability => "PROB",
            Self::Condition => "COND",
            Self::Retrig => "RTRG",
            Self::Rate => "RATE",
            Self::Sound => "SOUND",
        }
    }
}

/// Track values exposed by MIX. OUT's values remain section parameters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TrackField {
    Volume,
    Pan,
    SendA,
    SendB,
    Bus,
}

impl TrackField {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Volume => "VOL",
            Self::Pan => "PAN",
            Self::SendA => "SEND A",
            Self::SendB => "SEND B",
            Self::Bus => "BUS",
        }
    }

    pub const fn target(self) -> Option<&'static str> {
        match self {
            Self::Volume => Some(TRACK_VOLUME),
            Self::Pan => Some(TRACK_PAN),
            Self::SendA | Self::SendB | Self::Bus => None,
        }
    }
}

/// The seven slots of a lane LFO's page.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LfoField {
    Dest,
    Shape,
    Speed,
    Mult,
    Fade,
    Depth,
    Trig,
}

impl LfoField {
    pub const ALL: [Self; 7] = [
        Self::Dest,
        Self::Shape,
        Self::Speed,
        Self::Mult,
        Self::Fade,
        Self::Depth,
        Self::Trig,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Dest => "DEST",
            Self::Shape => "SHAPE",
            Self::Speed => "SPD",
            Self::Mult => "MULT",
            Self::Fade => "FADE",
            Self::Depth => "DEPTH",
            Self::Trig => "TRIG",
        }
    }
}

/// What twisting one deck slot addresses.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Slot {
    Param {
        subject: Subject,
        id: u32,
    },
    Trig(TrigField),
    Track(TrackField),
    /// One of the track's two lane LFOs, `which` 0 or 1.
    Lfo {
        which: u8,
        field: LfoField,
    },
}

/// A page after the selected track has supplied its machine and lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPage {
    pub subject: Option<Subject>,
    pub title: &'static str,
    pub slots: [Option<Slot>; 8],
}

fn slots(ids: impl IntoIterator<Item = u32>) -> [Option<u32>; 8] {
    let mut out = [None; 8];
    for (slot, id) in out.iter_mut().zip(ids) {
        *slot = Some(id);
    }
    out
}

/// A machine's declaration of one function key: the word on the key and
/// the sub-pages under it, each eight parameter ids of the machine's own
/// table. What a NEW machine ships beside its parameter table; the
/// pages ARE its surface.
#[derive(Clone, Copy, Debug)]
pub struct MachineKey {
    pub word: &'static str,
    pub subpages: &'static [SubPage],
}

/// The eight function keys as a machine declares them, F1 to F8 by
/// index. `None` leaves the key to the track: TRIG, the lane's filter
/// and amp sections, the lane LFOs, the lane's FX, MIX. A declared key
/// REPLACES the lane's filter or amp fallback and is APPENDED after the
/// track's own pages on the keys the track owns, so a machine may add to
/// TRIG or LFO but cannot take the sequencer away.
pub type KeyTable = [Option<MachineKey>; 8];

/// A machine's key table, if it has one. A machine without one has no
/// pages: it is not offered by the browser and the window shows only the
/// track's keys for it. There is no fallback; the old instruments are
/// reference, not surface.
pub fn key_table(kind: DeviceKind) -> Option<&'static KeyTable> {
    match kind {
        DeviceKind::Sampler => Some(&crate::params::sampler::KEYS),
        DeviceKind::Drum => Some(&crate::params::drum::KEYS),
        DeviceKind::Thump => Some(&crate::params::thump::KEYS),
        DeviceKind::Clay => Some(&crate::params::clay::KEYS),
        DeviceKind::Table => Some(&crate::params::table::KEYS),
        DeviceKind::Ring => Some(&crate::params::ring::KEYS),
        DeviceKind::PrismVoice => Some(&crate::params::prism_voice::KEYS),
        DeviceKind::Mass => Some(&crate::params::mass::KEYS),
        DeviceKind::Pluck => Some(&crate::params::pluck::KEYS),
        DeviceKind::Vox => Some(&crate::params::vox::KEYS),
        DeviceKind::Pipe => Some(&crate::params::pipe::KEYS),
        DeviceKind::Glass => Some(&crate::params::glass::KEYS),
        DeviceKind::Acid => Some(&crate::params::acid::KEYS),
        _ => None,
    }
}

/// The lane sections a machine wants on its FX key, when it has an
/// opinion: four effects on the key, the machine's own first, and the
/// rest of the lane's strip stays on the channel but off the pages.
/// `None` leaves the key to the lane's own list.
pub fn fx_sections(kind: DeviceKind) -> Option<&'static [SectionKind]> {
    match kind {
        DeviceKind::Sampler => Some(&[SectionKind::Echo, SectionKind::Room]),
        // THUMP brings its saturator, compressor and disperser; the
        // room is the one thing it wants from the lane.
        DeviceKind::Thump => Some(&[SectionKind::Room]),
        // CLAY brings its crack and bloom; colour and ducking from the lane.
        DeviceKind::Clay => Some(&[SectionKind::Drive, SectionKind::Pump]),
        DeviceKind::Table => Some(crate::params::table::FX_SECTIONS),
        DeviceKind::Ring => Some(crate::params::ring::FX_SECTIONS),
        DeviceKind::PrismVoice => Some(crate::params::prism_voice::FX_SECTIONS),
        DeviceKind::Mass => Some(crate::params::mass::FX_SECTIONS),
        DeviceKind::Pluck => Some(crate::params::pluck::FX_SECTIONS),
        DeviceKind::Vox => Some(crate::params::vox::FX_SECTIONS),
        DeviceKind::Pipe => Some(crate::params::pipe::FX_SECTIONS),
        DeviceKind::Glass => Some(crate::params::glass::FX_SECTIONS),
        // The acid has only its drive: colour, ducking, echo and room.
        DeviceKind::Acid => Some(&[
            SectionKind::Drive,
            SectionKind::Pump,
            SectionKind::Echo,
            SectionKind::Room,
        ]),
        _ => None,
    }
}

/// Whether `kind` is a machine the pages can show: it has a key table.
pub fn has_pages(kind: DeviceKind) -> bool {
    key_table(kind).is_some()
}

/// The word on `key` for this track: the machine's, when it declares
/// the key, else the standard row's. F8 has no standard word.
pub fn key_word(track: &Track, key: PageKey) -> &'static str {
    track
        .machine
        .as_ref()
        .and_then(|machine| key_table(machine.kind))
        .and_then(|table| table[key.index()].as_ref())
        .map_or(key.word(), |declared| declared.word)
}

fn filter_rank(name: &str) -> usize {
    let name = name.to_ascii_lowercase();
    if name.contains("mode") || name.contains("type") {
        0
    } else if name.contains("cutoff") || name == "cut" || name.contains("frequency") {
        1
    } else if name.contains("reso") || name == "q" {
        2
    } else if name.contains("env") && !name.contains("attack") && !name.contains("decay") {
        3
    } else if name.contains("attack") {
        4
    } else if name.contains("decay") {
        5
    } else if name.contains("sustain") {
        6
    } else if name.contains("release") {
        7
    } else {
        8
    }
}

fn amp_rank(name: &str) -> usize {
    let name = name.to_ascii_lowercase();
    if name.contains("attack") {
        0
    } else if name.contains("decay") {
        1
    } else if name.contains("sustain") {
        2
    } else if name.contains("release") {
        3
    } else if name.contains("drive") {
        4
    } else if name.contains("pan") {
        5
    } else if name.contains("level") || name.contains("gain") {
        6
    } else if name.contains("send") {
        7
    } else {
        8
    }
}

fn group_pages(
    spec: &'static DeviceSpec,
    key: PageKey,
    title: &'static str,
    mut ids: Vec<u32>,
) -> Vec<SubPage> {
    if matches!(key, PageKey::Fltr | PageKey::Amp) {
        ids.sort_by_key(|id| {
            let position = spec
                .params
                .iter()
                .position(|def| def.id == *id)
                .unwrap_or(0);
            let name = spec.labels.get(position).map_or("", |label| label.name);
            match key {
                PageKey::Fltr => filter_rank(name),
                PageKey::Amp => amp_rank(name),
                _ => 0,
            }
        });
    }

    let mut out = Vec::new();
    while !ids.is_empty() {
        let take = ids.len().min(8);
        let chunk: Vec<u32> = ids.drain(..take).collect();
        let mut row = slots(chunk.iter().copied());
        // Elektron's FLTR muscle memory is invariant: cutoff is slot two.
        // When a machine has no mode/type before it, leave slot one open.
        if key == PageKey::Fltr {
            let cutoff = chunk.iter().position(|id| {
                let at = spec
                    .params
                    .iter()
                    .position(|def| def.id == *id)
                    .unwrap_or(0);
                let name = spec.labels.get(at).map_or("", |label| label.name);
                filter_rank(name) == 1
            });
            if cutoff == Some(0) {
                row.rotate_right(1);
                row[0] = None;
            }
        }
        out.push(SubPage { title, slots: row });
    }
    out
}

/// Derive the section's rows from its catalog group labels, chunked by eight.
pub fn section_pages(kind: SectionKind) -> Vec<SubPage> {
    let spec = DeviceKind::Console(kind).spec();
    let mut groups: Vec<(&'static str, Vec<u32>)> = Vec::new();
    for (def, label) in spec.params.iter().zip(spec.labels) {
        match groups.iter_mut().find(|(group, _)| *group == label.group) {
            Some((_, ids)) => ids.push(def.id),
            None => groups.push((label.group, vec![def.id])),
        }
    }
    groups
        .into_iter()
        .flat_map(|(group, ids)| group_pages(spec, PageKey::Fx, group, ids))
        .collect()
}

fn resolved_params(
    subject: Subject,
    pages: impl IntoIterator<Item = SubPage>,
) -> Vec<ResolvedPage> {
    pages
        .into_iter()
        .map(|page| ResolvedPage {
            subject: Some(subject),
            title: page.title,
            slots: page
                .slots
                .map(|id| id.map(|id| Slot::Param { subject, id })),
        })
        .collect()
}

/// The machine's own sub-pages on `key`, as it declares them.
fn machine_pages(track: &Track, key: PageKey) -> Vec<ResolvedPage> {
    let Some(machine) = &track.machine else {
        return Vec::new();
    };
    let Some(declared) = key_table(machine.kind).and_then(|table| table[key.index()].as_ref())
    else {
        return Vec::new();
    };
    resolved_params(Subject::Machine, declared.subpages.iter().cloned())
}

fn lane_section(track: &Track, kind: Option<SectionKind>) -> Vec<ResolvedPage> {
    let Some(kind) = kind else { return Vec::new() };
    if !track
        .strip
        .iter()
        .any(|device| device.kind == DeviceKind::Console(kind))
    {
        return Vec::new();
    }
    resolved_params(Subject::Section(kind), section_pages(kind))
}

/// Resolve a key against one track. A page with no subject is represented by
/// one empty row, so callers can render its dim key while the stage lands on
/// ALL rather than refusing the press.
pub fn resolve(track: &Track, key: PageKey) -> Vec<ResolvedPage> {
    let resolved = match key {
        PageKey::Trig => {
            let mut pages = vec![
                ResolvedPage {
                    subject: None,
                    title: "TRIG",
                    slots: TrigField::ALL.map(|field| Some(Slot::Trig(field))),
                },
                // The sound lock has a sub-page of its own: one slot, so the
                // eight above keep their muscle memory.
                ResolvedPage {
                    subject: None,
                    title: "SOUND",
                    slots: [
                        Some(Slot::Trig(TrigField::Sound)),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    ],
                },
            ];
            pages.extend(machine_pages(track, key));
            pages
        }
        PageKey::Src => machine_pages(track, key),
        // The lane's two LFOs lead, on every track; the machine's own
        // follow. A and B, so they are never confused with a machine's
        // LFO 1 and LFO 2.
        PageKey::Lfo => {
            let mut pages: Vec<ResolvedPage> = (0..2u8)
                .map(|which| {
                    let mut slots = [None; 8];
                    for (at, field) in LfoField::ALL.into_iter().enumerate() {
                        slots[at] = Some(Slot::Lfo { which, field });
                    }
                    ResolvedPage {
                        subject: None,
                        title: if which == 0 { "LFO A" } else { "LFO B" },
                        slots,
                    }
                })
                .collect();
            pages.extend(machine_pages(track, key));
            pages
        }
        PageKey::Fltr => {
            let machine = machine_pages(track, key);
            if machine.is_empty() {
                lane_section(track, track.lane.fltr())
            } else {
                machine
            }
        }
        PageKey::Amp => {
            let machine = machine_pages(track, key);
            if machine.is_empty() {
                lane_section(track, track.lane.amp())
            } else {
                machine
            }
        }
        PageKey::Fx => {
            let picked = track
                .machine
                .as_ref()
                .and_then(|machine| fx_sections(machine.kind));
            let sections = picked.unwrap_or(track.lane.fx());
            let section_pages_of = |sections: &[SectionKind]| -> Vec<ResolvedPage> {
                sections
                    .iter()
                    .flat_map(|kind| resolved_params(Subject::Section(*kind), section_pages(*kind)))
                    .collect()
            };
            if picked.is_some() {
                // The machine's own effects lead its key; the sections
                // it asked for follow.
                let mut pages = machine_pages(track, key);
                pages.extend(section_pages_of(sections));
                pages
            } else {
                let mut pages = section_pages_of(sections);
                pages.extend(machine_pages(track, key));
                pages
            }
        }
        PageKey::Mix => {
            let mut row = [None; 8];
            for (at, field) in [
                TrackField::Volume,
                TrackField::Pan,
                TrackField::SendA,
                TrackField::SendB,
                TrackField::Bus,
            ]
            .into_iter()
            .enumerate()
            {
                row[at] = Some(Slot::Track(field));
            }
            if let Some(out) = track
                .strip
                .iter()
                .find(|device| device.kind == DeviceKind::Console(SectionKind::Out))
            {
                for (at, def) in out.kind.spec().params.iter().take(2).enumerate() {
                    row[5 + at] = Some(Slot::Param {
                        subject: Subject::Section(SectionKind::Out),
                        id: def.id,
                    });
                }
            }
            let mut pages = vec![ResolvedPage {
                subject: None,
                title: "MIX",
                slots: row,
            }];
            pages.extend(machine_pages(track, key));
            pages
        }
        // F8 is the machine's to declare; the track has nothing on it.
        PageKey::All => machine_pages(track, key),
    };
    if resolved.is_empty() {
        vec![ResolvedPage {
            subject: None,
            title: key.word(),
            slots: [None; 8],
        }]
    } else {
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DEVICES;
    use crate::lane::Lane;
    use crate::sequencing::Song;
    use std::collections::HashSet;

    #[test]
    fn derived_section_pages_cover_every_parameter_once() {
        for kind in SectionKind::ALL {
            let ids: Vec<u32> = section_pages(kind)
                .iter()
                .flat_map(|page| page.slots.into_iter().flatten())
                .collect();
            let unique: HashSet<u32> = ids.iter().copied().collect();
            assert_eq!(ids.len(), unique.len(), "{kind:?} repeats a parameter");
            assert_eq!(ids.len(), kind.table().len(), "{kind:?} omits a parameter");
        }
    }

    #[test]
    fn every_key_resolves_for_every_lane_and_machine() {
        for lane in Lane::ALL {
            for spec in DEVICES.iter().filter(|spec| spec.instrument) {
                let mut song = Song::default();
                song.set_lane(0, lane);
                song.add_device(0, spec.kind);
                let track = &song.tracks[0];
                for key in PageKey::ALL {
                    assert!(
                        !resolve(track, key).is_empty(),
                        "{lane:?} {} {key:?}",
                        spec.name
                    );
                }
            }
        }
    }

    /// A machine with an opinion about its FX key gets four effects on
    /// it: its own first, then the lane sections it asked for. The rest
    /// of the strip stays on the channel, off the pages.
    #[test]
    fn a_machine_picks_the_lane_sections_on_its_fx_key() {
        let mut song = Song::default();
        song.set_lane(0, Lane::Drum);
        song.add_device(0, DeviceKind::Thump);
        let pages = resolve(&song.tracks[0], PageKey::Fx);
        let titles: Vec<&str> = pages.iter().map(|page| page.title).collect();
        assert_eq!(titles[..3], ["Sat", "Comp", "Disperse"]);
        let mut sections = Vec::new();
        for page in &pages {
            if let Some(Subject::Section(kind)) = page.subject
                && sections.last() != Some(&kind)
            {
                sections.push(kind);
            }
        }
        assert_eq!(sections, [SectionKind::Room]);

        let mut song = Song::default();
        song.add_device(0, DeviceKind::Acid);
        let pages = resolve(&song.tracks[0], PageKey::Fx);
        let mut sections = Vec::new();
        for page in &pages {
            if let Some(Subject::Section(kind)) = page.subject
                && sections.last() != Some(&kind)
            {
                sections.push(kind);
            }
        }
        assert_eq!(
            sections,
            [
                SectionKind::Drive,
                SectionKind::Pump,
                SectionKind::Echo,
                SectionKind::Room,
            ]
        );
        assert!(
            pages
                .iter()
                .all(|page| page.subject != Some(Subject::Machine))
        );
    }

    #[test]
    fn drum_fx_are_the_four_lane_sections_in_order() {
        let mut song = Song::default();
        song.set_lane(0, Lane::Drum);
        let subjects: Vec<SectionKind> = resolve(&song.tracks[0], PageKey::Fx)
            .iter()
            .filter_map(|page| match page.subject {
                Some(Subject::Section(kind)) => Some(kind),
                _ => None,
            })
            .collect();
        let mut first = Vec::new();
        for subject in subjects {
            if first.last() != Some(&subject) {
                first.push(subject);
            }
        }
        assert_eq!(
            first,
            [
                SectionKind::Drive,
                SectionKind::Grit,
                SectionKind::Pump,
                SectionKind::Room,
            ]
        );
    }

    #[test]
    fn trig_has_a_sound_sub_page_after_its_eight() {
        let song = crate::sequencing::Song::default();
        let pages = resolve(&song.tracks[0], PageKey::Trig);
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0].slots,
            TrigField::ALL.map(|field| Some(Slot::Trig(field)))
        );
        assert_eq!(pages[1].title, "SOUND");
        assert_eq!(pages[1].slots[0], Some(Slot::Trig(TrigField::Sound)));
        assert!(pages[1].slots[1..].iter().all(Option::is_none));
    }

    #[test]
    fn the_lane_lfos_lead_the_lfo_key_on_every_track() {
        let song = crate::sequencing::Song::default();
        let pages = resolve(&song.tracks[0], PageKey::Lfo);
        assert!(pages.len() >= 2);
        assert_eq!(pages[0].title, "LFO A");
        assert_eq!(pages[1].title, "LFO B");
        assert_eq!(
            pages[1].slots[0],
            Some(Slot::Lfo {
                which: 1,
                field: LfoField::Dest
            })
        );
        assert!(pages[0].slots[7].is_none());
        let mut bare = song.clone();
        bare.tracks[0].machine = None;
        assert_eq!(resolve(&bare.tracks[0], PageKey::Lfo).len(), 2);
    }

    /// Every declared key table is total over its own parameter table:
    /// each slot id exists, none twice, eight or fewer per sub-page;
    /// FLTR keeps cutoff on slot two and AMP attack on slot one.
    #[test]
    fn every_key_table_is_sound() {
        let mut declared = 0;
        for spec in DEVICES.iter().filter(|spec| spec.instrument) {
            let Some(table) = key_table(spec.kind) else {
                continue;
            };
            declared += 1;
            let mut seen = HashSet::new();
            for (at, key) in table.iter().enumerate() {
                let Some(key) = key else { continue };
                assert!(
                    !key.word.is_empty(),
                    "{} F{} has no word",
                    spec.name,
                    at + 1
                );
                assert!(
                    !key.subpages.is_empty(),
                    "{} F{} is empty",
                    spec.name,
                    at + 1
                );
                for page in key.subpages {
                    for id in page.slots.into_iter().flatten() {
                        assert!(
                            spec.params.iter().any(|def| def.id == id),
                            "{} names a parameter it lacks: {id}",
                            spec.name
                        );
                        assert!(seen.insert(id), "{} repeats {id}", spec.name);
                    }
                }
            }
        }
        assert!(declared >= 1, "no machine declares a key table");
    }

    #[test]
    fn a_machine_without_a_key_table_has_no_pages_of_its_own() {
        let song = crate::sequencing::Song::default();
        let track = &song.tracks[0];
        assert!(!has_pages(track.machine.as_ref().unwrap().kind));
        let empty = |key| {
            resolve(track, key)
                .iter()
                .all(|page| page.slots.iter().all(Option::is_none))
        };
        assert!(empty(PageKey::Src));
        assert!(empty(PageKey::All));
        assert_eq!(key_word(track, PageKey::Src), "SRC");
        assert_eq!(key_word(track, PageKey::All), "");
    }
}
