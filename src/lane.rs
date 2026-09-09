//! Lanes: what a track is FOR, and what the desk gives it for that.
//!
//! A lane kind is a design, not a rule. It says what the track was built
//! to carry and furnishes the track accordingly — which sections of the
//! console strip start IN and at what values, and which bus the strip
//! feeds — and then it steps back. The browser still offers every
//! instrument to every lane; a kick on the bass lane is allowed and simply
//! not what the lane was drawn for. Nothing here refuses anything.
//!
//! Every kind is a long piece of work in its own right — the science of
//! what a drum channel wants is not the science of what a chord channel
//! wants — so kinds are added one at a time. `Plain` is the general-purpose
//! lane every track has until it is told otherwise: the desk's full strip
//! and the music bus, preserving the routing of songs written before lanes.
//!
//! Green zone, crate root, no audio types: the strip's values are set on
//! the song's own devices, and the compiler reads them as it reads any
//! other override.

use crate::console::SectionKind;
use crate::devices::DeviceKind;
use crate::params::console as p;
use crate::sequencing::{BUS_DRUM, BUS_MUSIC};

/// The palette role a lane wears. Kept neutral here: the view translates
/// the role into the current theme, so this model never imports egui.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    Plain,
    Drum,
}

/// What a track was built to carry.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize,
)]
pub enum Lane {
    /// General-purpose fallback: the full desk strip on the music bus.
    #[default]
    Plain,
    /// A drum channel: transients kept, the low end cleaned, the level
    /// held.
    Drum,
}

/// One section of a lane's strip, in the lane's order: whether it starts
/// IN, and what it is set to. A lane's strip is exactly its list of
/// these — a section the lane does not name is not on the channel at
/// all, which is what makes a drum strip a drum strip rather than the
/// whole desk with most of it switched off.
#[derive(Clone, Copy, Debug)]
pub struct SectionPreset {
    pub kind: SectionKind,
    /// Starts IN. The desk's always-in sections are in regardless.
    pub in_: bool,
    pub values: &'static [(u32, f32)],
}

/// The complete design of one lane kind. Every surface asks this table
/// rather than growing a second match over [`Lane`].
#[derive(Clone, Copy, Debug)]
pub struct LaneKind {
    pub word: &'static str,
    pub name: &'static str,
    pub hue: Role,
    pub bus: u8,
    pub strip: &'static [SectionPreset],
    pub fx: &'static [SectionKind],
    pub fltr: Option<SectionKind>,
    pub amp: Option<SectionKind>,
    pub default_machine: Option<DeviceKind>,
    /// The machines this kind leads the browser with, in order. Empty
    /// means the registry as it is, unranked. The rest stays reachable:
    /// a shelf is a recommendation, not a refusal.
    pub shelf: &'static [DeviceKind],
    /// What a FRESH machine of a kind is set to when it enters this
    /// lane: `(machine, values)`, applied by `Song::add_device` and never
    /// by paste or by a sound, which carry their own values. Set by ear,
    /// kind by kind; a test holds every entry to a real parameter.
    pub defaults: &'static [(DeviceKind, &'static [(u32, f32)])],
}

const fn section(kind: SectionKind, in_: bool, values: &'static [(u32, f32)]) -> SectionPreset {
    SectionPreset { kind, in_, values }
}

/// The kinds of a preset list, in order, for the furnisher.
const fn kinds<const N: usize>(presets: &[SectionPreset; N]) -> [SectionKind; N] {
    let mut out = [SectionKind::Preamp; N];
    let mut i = 0;
    while i < N {
        out[i] = presets[i].kind;
        i += 1;
    }
    out
}

/// The drum strip: a drum channel's tools in a drum channel's order.
///
/// Cleaning first (CUT ahead of the gate, so the key hears no rumble),
/// then the gate, then the transient shaper, then the levers, then the
/// compressor, then colour, then the room, then out. What the desk's
/// full strip has and this does not — FOUR, SPLIT, the modulation and
/// spectral sections, ECHO — is sound design or bus work, and lives on a
/// plain lane. Echo reaches a drum through the TAPE send.
///
/// The VALUES are a first cut, to be set by ear: the compressor's
/// attack, release and ratio in particular are on unitless ranges and
/// are left at the desk's defaults until they are.
const DRUM_STRIP: [SectionPreset; 13] = [
    // A touch of transformer thickens a drum before anything touches it.
    section(SectionKind::Preamp, true, &[(p::preamp::IRON, 15.0)]),
    // Below the kick's fundamental there is only rumble.
    section(SectionKind::Cut, true, &[(p::cut::HP_HZ, 40.0)]),
    // Off, ready: tails tightened, or the rhythmic gate.
    section(SectionKind::Door, false, &[(p::door::KEY_HP, 80.0)]),
    // The drum's core tool: a little more front on every hit.
    section(SectionKind::Hit, true, &[(p::hit::ATTACK, 20.0)]),
    // The three levers with kills, after the shaper so a kill acts on
    // the shaped hit.
    section(SectionKind::Tone, false, &[]),
    // The channel compressor, keyed above the low end, parallel mix
    // exposed because parallel compression is the drum staple.
    section(
        SectionKind::Vca,
        true,
        &[(p::vca::SC_HP, 80.0), (p::vca::MIX, 100.0)],
    ),
    // Colour, after the compressor so it colours a held level.
    section(SectionKind::Drive, false, &[]),
    section(SectionKind::Grit, false, &[]),
    section(SectionKind::Shine, false, &[]),
    // Kept for hats ducked on the beat.
    section(SectionKind::Pump, false, &[]),
    // Kept in the path for legacy Echo migration. The drum page keeps the
    // four performance effects below; echo is reached through ALL until a
    // later drum-lane design gives it a dedicated page.
    section(SectionKind::Echo, false, &[]),
    // The drum room: short, no predelay.
    section(
        SectionKind::Room,
        false,
        &[(p::room::PREDELAY, 0.0), (p::room::SIZE, 25.0)],
    ),
    // A kick that stays in the middle.
    section(SectionKind::Out, true, &[(p::out::BASS_MONO, 100.0)]),
];

const DRUM_SECTIONS: [SectionKind; 13] = kinds(&DRUM_STRIP);

const PLAIN_STRIP: [SectionPreset; 20] = [
    section(SectionKind::Preamp, true, &[]),
    section(SectionKind::Tone, false, &[]),
    section(SectionKind::Door, false, &[]),
    section(SectionKind::Cut, false, &[]),
    section(SectionKind::Hit, false, &[]),
    section(SectionKind::Four, false, &[]),
    section(SectionKind::Vca, false, &[]),
    section(SectionKind::Split, false, &[]),
    section(SectionKind::Pump, false, &[]),
    section(SectionKind::Drive, false, &[]),
    section(SectionKind::Grit, false, &[]),
    section(SectionKind::Shine, false, &[]),
    section(SectionKind::Drift, false, &[]),
    section(SectionKind::Phase, false, &[]),
    section(SectionKind::Smear, false, &[]),
    section(SectionKind::Ring, false, &[]),
    section(SectionKind::Spectra, false, &[]),
    section(SectionKind::Echo, false, &[]),
    section(SectionKind::Room, false, &[]),
    section(SectionKind::Out, true, &[]),
];

const PLAIN_FX: &[SectionKind] = &[
    SectionKind::Tone,
    SectionKind::Door,
    SectionKind::Hit,
    SectionKind::Four,
    SectionKind::Split,
    SectionKind::Pump,
    SectionKind::Drive,
    SectionKind::Grit,
    SectionKind::Shine,
    SectionKind::Drift,
    SectionKind::Phase,
    SectionKind::Smear,
    SectionKind::Ring,
    SectionKind::Spectra,
    SectionKind::Echo,
    SectionKind::Room,
];

const DRUM_FX: &[SectionKind] = &[
    SectionKind::Drive,
    SectionKind::Grit,
    SectionKind::Pump,
    SectionKind::Room,
];

const PLAIN: LaneKind = LaneKind {
    word: "PLAIN",
    name: "plain",
    hue: Role::Plain,
    bus: BUS_MUSIC,
    strip: &PLAIN_STRIP,
    fx: PLAIN_FX,
    fltr: Some(SectionKind::Cut),
    amp: Some(SectionKind::Vca),
    default_machine: None,
    shelf: &[
        DeviceKind::Table,
        DeviceKind::Ring,
        DeviceKind::PrismVoice,
        DeviceKind::Mass,
        DeviceKind::Pluck,
        DeviceKind::Vox,
        DeviceKind::Pipe,
        DeviceKind::Glass,
    ],
    defaults: &[],
};

const DRUM: LaneKind = LaneKind {
    word: "DRUM",
    name: "drum",
    hue: Role::Drum,
    bus: BUS_DRUM,
    strip: &DRUM_STRIP,
    fx: DRUM_FX,
    fltr: Some(SectionKind::Cut),
    amp: Some(SectionKind::Hit),
    default_machine: Some(DeviceKind::Drum),
    // The shelf is the machines written for the pages; the old drums
    // are reference and parts, not shelf.
    shelf: &[DeviceKind::Drum, DeviceKind::Thump, DeviceKind::Clay],
    defaults: &[],
};

impl Lane {
    /// Every kind, in the order the palette lists them.
    pub const ALL: [Self; 2] = [Self::Plain, Self::Drum];

    /// The one table row that defines this lane.
    pub const fn kind(self) -> &'static LaneKind {
        match self {
            Self::Plain => &PLAIN,
            Self::Drum => &DRUM,
        }
    }

    /// The word the head wears. Empty for a plain lane: no design, no
    /// badge.
    pub const fn word(self) -> &'static str {
        self.kind().word
    }

    /// The word typed after `lane` in the palette.
    pub const fn name(self) -> &'static str {
        self.kind().name
    }

    /// The lane a typed word names. `clear` and `plain` both mean no
    /// design, so the statement that undoes a lane reads either way.
    pub fn parse(word: &str) -> Option<Self> {
        if word.eq_ignore_ascii_case("clear") {
            return Some(Self::Plain);
        }
        Self::ALL
            .into_iter()
            .find(|lane| word.eq_ignore_ascii_case(lane.name()))
    }

    /// The bus a lane of this kind feeds, when it has an opinion.
    pub const fn bus(self) -> u8 {
        self.kind().bus
    }

    /// The sections of this lane's strip, in signal order. The furnisher
    /// builds the channel from this list and nothing else.
    pub const fn sections(self) -> &'static [SectionKind] {
        match self {
            Self::Plain => &SectionKind::STRIP,
            Self::Drum => &DRUM_SECTIONS,
        }
    }

    /// This lane's strip as it furnishes it: every section, in order,
    /// with its state and values. A plain lane has no presets — its
    /// strip is the desk's, bare.
    pub const fn strip(self) -> &'static [SectionPreset] {
        self.kind().strip
    }

    /// How this lane wants `kind`, when it has an opinion.
    pub fn preset(self, kind: SectionKind) -> Option<&'static SectionPreset> {
        self.strip().iter().find(|preset| preset.kind == kind)
    }

    pub const fn fx(self) -> &'static [SectionKind] {
        self.kind().fx
    }

    pub const fn fltr(self) -> Option<SectionKind> {
        self.kind().fltr
    }

    pub const fn amp(self) -> Option<SectionKind> {
        self.kind().amp
    }

    pub const fn default_machine(self) -> Option<DeviceKind> {
        self.kind().default_machine
    }

    /// The machines this lane leads the browser with.
    pub const fn shelf(self) -> &'static [DeviceKind] {
        self.kind().shelf
    }

    /// What a fresh `machine` is set to on this lane, if the lane says.
    pub fn defaults_for(self, machine: DeviceKind) -> &'static [(u32, f32)] {
        self.kind()
            .defaults
            .iter()
            .find(|(kind, _)| *kind == machine)
            .map_or(&[], |(_, values)| values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lane_parses_from_its_own_name_and_clear_is_plain() {
        for lane in Lane::ALL {
            assert_eq!(Lane::parse(lane.name()), Some(lane));
            assert_eq!(Lane::parse(&lane.name().to_uppercase()), Some(lane));
        }
        assert_eq!(Lane::parse("clear"), Some(Lane::Plain));
        assert_eq!(Lane::parse("bongo"), None);
    }

    #[test]
    fn a_preset_names_only_sections_of_the_desk_and_only_their_own_params() {
        for lane in Lane::ALL {
            for preset in lane.strip() {
                assert!(
                    SectionKind::STRIP.contains(&preset.kind),
                    "{:?} furnishes {:?}, which is not a channel section",
                    lane,
                    preset.kind
                );
                let table = crate::devices::DeviceKind::Console(preset.kind)
                    .spec()
                    .params;
                for (id, value) in preset.values {
                    let def = table
                        .iter()
                        .find(|def| def.id == *id)
                        .unwrap_or_else(|| panic!("{:?} has no param {id}", preset.kind));
                    assert!(
                        (def.min..=def.max).contains(value),
                        "{:?}.{} = {value} is outside {}..={}",
                        preset.kind,
                        def.name,
                        def.min,
                        def.max
                    );
                }
            }
        }
    }

    /// Every lane's strip is a channel: it starts at the preamp, ends at
    /// OUT, names each section once, and keeps the desk's order between.
    #[test]
    fn every_strip_is_a_channel_in_the_desks_order() {
        for lane in Lane::ALL {
            let sections = lane.sections();
            assert_eq!(sections.first(), Some(&SectionKind::Preamp), "{lane:?}");
            assert_eq!(sections.last(), Some(&SectionKind::Out), "{lane:?}");
            for (i, kind) in sections.iter().enumerate() {
                assert!(
                    !sections[i + 1..].contains(kind),
                    "{lane:?} names {kind:?} twice"
                );
            }
            for kind in SectionKind::STRIP {
                if kind.always_in() {
                    assert!(sections.contains(&kind), "{lane:?} drops {kind:?}");
                }
            }
            let presets: Vec<SectionKind> = lane.strip().iter().map(|p| p.kind).collect();
            if !presets.is_empty() {
                assert_eq!(presets, sections, "{lane:?}: presets and sections disagree");
            }
        }
        // The drum strip is its own thing, not the desk with lights off.
        assert_eq!(Lane::Drum.sections().len(), 13);
        assert!(!Lane::Drum.sections().contains(&SectionKind::Four));
        let cut = Lane::Drum
            .sections()
            .iter()
            .position(|k| *k == SectionKind::Cut);
        let door = Lane::Drum
            .sections()
            .iter()
            .position(|k| *k == SectionKind::Door);
        assert!(cut < door, "CUT stands ahead of the gate");
    }

    #[test]
    fn every_lane_names_its_pages_inside_its_strip() {
        for lane in Lane::ALL {
            for section in lane.fx() {
                assert!(lane.sections().contains(section), "{lane:?}: {section:?}");
            }
            if let Some(section) = lane.fltr() {
                assert!(lane.sections().contains(&section));
            }
            if let Some(section) = lane.amp() {
                assert!(lane.sections().contains(&section));
            }
        }
        assert_eq!(Lane::Plain.bus(), BUS_MUSIC);
        assert_eq!(Lane::Plain.word(), "PLAIN");
    }

    #[test]
    fn every_shelf_entry_is_an_instrument_named_once() {
        for lane in Lane::ALL {
            let shelf = lane.shelf();
            for (i, kind) in shelf.iter().enumerate() {
                assert!(
                    kind.is_instrument(),
                    "{}: {kind:?} is not a machine",
                    lane.name()
                );
                assert!(
                    !shelf[..i].contains(kind),
                    "{}: {kind:?} twice",
                    lane.name()
                );
            }
            if let Some(default) = lane.default_machine() {
                assert!(
                    shelf.is_empty() || shelf.contains(&default),
                    "{}: its default machine is not on its shelf",
                    lane.name()
                );
            }
        }
    }

    #[test]
    fn every_lane_default_names_a_real_parameter_in_range() {
        for lane in Lane::ALL {
            for (machine, values) in lane.kind().defaults {
                assert!(machine.is_instrument(), "{}: {machine:?}", lane.name());
                let table = machine.spec().params;
                for (param, value) in *values {
                    let def = table
                        .iter()
                        .find(|def| def.id == *param)
                        .unwrap_or_else(|| panic!("{}: {machine:?} has no {param}", lane.name()));
                    assert!(
                        (def.min..=def.max).contains(value),
                        "{}: {machine:?}.{} = {value} is outside {}..={}",
                        lane.name(),
                        def.name,
                        def.min,
                        def.max
                    );
                }
            }
        }
    }
}
