//! The device catalog: what kinds of device exist, and what each one is.
//!
//! One row per device — its name, whether it heads a chain, its stable
//! target prefix, the engine's parameter table and the words the UI puts
//! on those parameters. Adding a device is a row here, a `DeviceState`
//! variant and a node, rather than an edit in nine hardcoded matches.
//!
//! Pure data. Nothing here touches egui, the app or the audio thread —
//! `params` stays `&'static` because `daw::params::clamp` scans exactly
//! that slice inside the callback.
//!
//! Lifted out of `main.rs` unchanged.

// This data now compiles in the library; retain the public `daw::params`
// spelling used throughout the catalogue while resolving it to this crate.
use crate as daw;

/// A device a track can hold: `SineSynth` makes sound, `Reverb` shapes it.
///
/// What a device IS. Every variant is a real node with a real parameter
/// table; the browser's Instruments and Audio Effects folders are how one
/// reaches a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DeviceKind {
    SineSynth,
    Poly,
    Tine,
    Haze,
    Loom,
    Sampler,
    Kick,
    Snare,
    Tom,
    Hat,
    Handclap,
    Reverb,
    Sat,
    Lofi,
    Sheen,
    Disperser,
    Tilt,
    Phaser,
    Gate,
    Strip,
    Resyn,
    Acid,
    Rack,
    Echo,
    Eq,
    Filter,
    Glue,
    Clamp,
    Flint,
    Sibyl,
    Ferric,
    Umbra,
    Tone,
    Sigil,
    Gauge,
    Prism,
    Modulato,
    Utility,
    Limiter,
}

impl DeviceKind {
    /// Instruments MAKE sound, effects SHAPE it. An instrument heads a
    /// chain and there is at most one; effects follow it in order.
    pub fn is_instrument(self) -> bool {
        self.spec().instrument
    }

    /// Everything the app knows about this kind of device. A linear scan of
    /// `DEVICES`, which the tests walk to prove it is total.
    pub fn spec(self) -> &'static DeviceSpec {
        DEVICES
            .iter()
            .find(|spec| spec.kind == self)
            .unwrap_or(&DEVICES[0])
    }
}

/// How one parameter PRESENTS itself: what the picker calls it, the unit a
/// readout appends, and the group it is filed under. Parallel to the
/// device's engine table — the numbers live there, the words live here.
pub struct ParamLabel {
    pub name: &'static str,
    pub unit: &'static str,
    pub group: &'static str,
    /// The names of this parameter's positions, when it is a LIST to walk
    /// rather than a range to sweep: a filter mode, a waveform, a switch.
    /// Empty means continuous. A surface that steps a choice by a
    /// hundredth of its range is twenty-five presses from the next entry,
    /// and one that shows "2.00" for "saw" is showing the wire, not the
    /// word — so the catalog says which parameters are choices, and what
    /// each position is called, beside the name, unit and group it
    /// already carries.
    pub choices: &'static [&'static str],
}

/// Everything the app needs to know about a kind of device, in one place.
/// Adding a device is a row here, a [`DeviceState`] variant and a node —
/// not an edit in nine hardcoded matches.
///
/// The two headings a device can be filed under.
///
/// Instruments MAKE sound and head a chain; effects SHAPE what reaches
/// them. It is the same split `DeviceSpec::instrument` records, named
/// here so a browser can print it — and a test holds the two to each
/// other so they can never disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    Instruments,
    AudioEffects,
}

impl Section {
    pub const ALL: [Self; 2] = [Self::Instruments, Self::AudioEffects];

    pub fn label(self) -> &'static str {
        match self {
            Self::Instruments => "Instruments",
            Self::AudioEffects => "Audio Effects",
        }
    }
}

/// Which family a device belongs to.
///
/// The registry already says what exists, so it says where each device is
/// FILED too. A catalog kept anywhere else drifts the first time someone
/// adds a device to one and not the other — and a device missing from a
/// browser is invisible rather than obviously broken.
///
/// Every family belongs to exactly one section, and nothing is loose: a
/// device sitting outside the headings would read as the odd one out
/// rather than as the uncategorised one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    Synths,
    Drums,
    Sampling,
    Dynamics,
    EqAndFilters,
    DelayAndReverb,
    Distortion,
    Modulation,
    Spectral,
    Utilities,
}

impl Family {
    pub const ALL: [Self; 10] = [
        Self::Synths,
        Self::Drums,
        Self::Sampling,
        Self::Dynamics,
        Self::EqAndFilters,
        Self::DelayAndReverb,
        Self::Distortion,
        Self::Modulation,
        Self::Spectral,
        Self::Utilities,
    ];

    /// The heading this family sits under.
    pub fn section(self) -> Section {
        match self {
            Self::Synths => Section::Instruments,
            Self::Drums => Section::Instruments,
            Self::Sampling => Section::Instruments,
            Self::Dynamics => Section::AudioEffects,
            Self::EqAndFilters => Section::AudioEffects,
            Self::DelayAndReverb => Section::AudioEffects,
            Self::Distortion => Section::AudioEffects,
            Self::Modulation => Section::AudioEffects,
            Self::Spectral => Section::AudioEffects,
            Self::Utilities => Section::AudioEffects,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Synths => "Synths",
            Self::Drums => "Drums",
            Self::Sampling => "Sampling",
            Self::Dynamics => "Dynamics",
            Self::EqAndFilters => "EQ & Filters",
            Self::DelayAndReverb => "Delay & Reverb",
            Self::Distortion => "Distortion",
            Self::Modulation => "Modulation",
            Self::Spectral => "Spectral",
            Self::Utilities => "Utilities",
        }
    }
}

/// `params` is `&'static` and stays that way: `daw::params::clamp` scans
/// exactly this slice inside the audio callback, so anything heap-backed
/// reaching it would be a red-zone allocation. Descriptive on this side,
/// static on the engine's.
pub struct DeviceSpec {
    pub kind: DeviceKind,
    pub name: &'static str,
    /// Instruments MAKE sound and head the chain; effects SHAPE it.
    pub instrument: bool,
    /// Where this device is filed. See [`Family`].
    pub family: Family,
    /// Stable target-id prefix: "synth", "reverb". Part of the file
    /// format — renaming one orphans every wire that names it.
    pub prefix: &'static str,
    /// The engine's own parameter table.
    pub params: &'static [daw::params::ParamDef],
    /// Presentation per parameter, parallel to `params`.
    pub labels: &'static [ParamLabel],
}

/// Name tables shared by more than one device, or owned by a card rather
/// than a params module. Each is the list the card steps through, so the
/// band and the card cannot disagree about what position three is.
const OFF_ON: &[&str] = &["off", "on"];
const FILTER_MODES: &[&str] = &["lp", "hp", "bp", "notch"];
const FILTER_SLOPES: &[&str] = &["6", "12", "18", "24", "36", "48"];
const SHAPER_MODES: &[&str] = &["hard", "soft", "cubic", "fold", "crush"];
const BIT_DEPTHS: &[&str] = &[
    "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];
const SHIFT_VOICES: &[&str] = &[
    "-7", "-6", "-5", "-4", "-3", "-2", "-1", "--", "+1", "+2", "+3", "+4", "+5", "+6", "+7",
];
const STAGES_TO_16: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];
const STAGES_TO_32: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];

pub static DEVICES: &[DeviceSpec] = &[
    DeviceSpec {
        kind: DeviceKind::SineSynth,
        name: "sine synth",
        instrument: true,
        family: Family::Synths,
        prefix: "synth",
        params: daw::params::seq::TABLE,
        labels: &[
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Synth",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Synth",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Synth",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Sampler,
        name: "sampler",
        instrument: true,
        family: Family::Sampling,
        prefix: "sampler",
        params: daw::params::sampler::TABLE,
        // Grouped the way the card's five pages are, so a modulation
        // target reads "Filter / Cutoff" rather than an id. THIRTY-SIX,
        // exactly as many as the table has: the registry zips the two
        // and would drop the difference in silence.
        labels: &[
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Sample",
                choices: daw::params::sampler::MODE_NAMES,
            },
            ParamLabel {
                name: "Start",
                unit: " %",
                group: "Sample",
                choices: &[],
            },
            ParamLabel {
                name: "End",
                unit: " %",
                group: "Sample",
                choices: &[],
            },
            ParamLabel {
                name: "Reverse",
                unit: "",
                group: "Sample",
                choices: daw::params::sampler::OFF_ON_NAMES,
            },
            ParamLabel {
                name: "Fade In",
                unit: " ms",
                group: "Sample",
                choices: &[],
            },
            ParamLabel {
                name: "Fade Out",
                unit: " ms",
                group: "Sample",
                choices: &[],
            },
            ParamLabel {
                name: "Root",
                unit: "",
                group: "Pitch",
                choices: &[],
            },
            ParamLabel {
                name: "Tune",
                unit: " st",
                group: "Pitch",
                choices: &[],
            },
            ParamLabel {
                name: "Fine",
                unit: " ct",
                group: "Pitch",
                choices: &[],
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Loop",
                choices: daw::params::sampler::LOOP_NAMES,
            },
            ParamLabel {
                name: "Start",
                unit: " %",
                group: "Loop",
                choices: &[],
            },
            ParamLabel {
                name: "Crossfade",
                unit: " ms",
                group: "Loop",
                choices: &[],
            },
            ParamLabel {
                name: "Slices",
                unit: "",
                group: "Slice",
                choices: &[],
            },
            ParamLabel {
                name: "Source",
                unit: "",
                group: "Slice",
                choices: daw::params::sampler::SLICE_SOURCE_NAMES,
            },
            ParamLabel {
                name: "Choke",
                unit: "",
                group: "Slice",
                choices: daw::params::sampler::OFF_ON_NAMES,
            },
            ParamLabel {
                name: "Attack",
                unit: " ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: " %",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Filter",
                choices: daw::params::sampler::FILT_NAMES,
            },
            ParamLabel {
                name: "Cutoff",
                unit: " Hz",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Keytrack",
                unit: " %",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: " ms",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: " %",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Destination",
                unit: "",
                group: "Mod",
                choices: daw::params::sampler::DEST_NAMES,
            },
            ParamLabel {
                name: "Depth",
                unit: " %",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Velocity",
                unit: " %",
                group: "Mod",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: " %",
                group: "Dirt",
                choices: &[],
            },
            ParamLabel {
                name: "Rate",
                unit: " Hz",
                group: "Dirt",
                choices: &[],
            },
            ParamLabel {
                name: "Bits",
                unit: "",
                group: "Dirt",
                choices: &[],
            },
            ParamLabel {
                name: "Pre-amp",
                unit: " %",
                group: "Dirt",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: " dB",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Pan",
                unit: " %",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Kick,
        name: "kick",
        instrument: true,
        family: Family::Drums,
        prefix: "kick",
        params: daw::params::kick::TABLE,
        // Grouped the way the card's sections are, so a modulation
        // target reads "Punch / Depth" rather than an id.
        labels: &[
            ParamLabel {
                name: "Tune",
                unit: " Hz",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Depth",
                unit: " st",
                group: "Punch",
                choices: &[],
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Punch",
                choices: &[],
            },
            ParamLabel {
                name: "Depth",
                unit: " st",
                group: "Sweep",
                choices: &[],
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Sweep",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Click",
                choices: &[],
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Click",
                choices: &[],
            },
            ParamLabel {
                name: "Stages",
                unit: "",
                group: "Disperse",
                choices: &[],
            },
            ParamLabel {
                name: "Harmonic",
                unit: "",
                group: "Disperse",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Disperse",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Haze,
        name: "haze",
        instrument: true,
        family: Family::Synths,
        prefix: "haze",
        params: daw::params::haze::TABLE,
        // Nineteen rows, grouped the way the card's pages are, so a
        // modulation target reads "Filter / Cutoff" rather than an id.
        labels: &[
            ParamLabel {
                name: "Spread",
                unit: "ct",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Shape",
                unit: "%",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Sub",
                unit: "%",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Drift",
                unit: "%",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Cutoff",
                unit: "Hz",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Resonance",
                unit: "%",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Keytrack",
                unit: "%",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Env amount",
                unit: "",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "s",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "s",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "s",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Filter attack",
                unit: "s",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Filter decay",
                unit: "s",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Ensemble",
                unit: "%",
                group: "Air",
                choices: &[],
            },
            ParamLabel {
                name: "Wow",
                unit: "%",
                group: "Air",
                choices: &[],
            },
            ParamLabel {
                name: "Grain",
                unit: "%",
                group: "Air",
                choices: &[],
            },
            ParamLabel {
                name: "Warmth",
                unit: "%",
                group: "Air",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Air",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Loom,
        name: "loom",
        instrument: true,
        family: Family::Synths,
        prefix: "loom",
        params: daw::params::loom::TABLE,
        labels: &[
            ParamLabel {
                name: "Morph",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Loom",
                choices: daw::params::loom::OCTAVES,
            },
            ParamLabel {
                name: "Semi",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Morph",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Loom",
                choices: daw::params::loom::OCTAVES,
            },
            ParamLabel {
                name: "Semi",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Noise",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "N Decay",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Loom",
                choices: daw::params::loom::FILTER_MODES,
            },
            ParamLabel {
                name: "Cutoff",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Res",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "F Env",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Velocity",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "F Attack",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "F Decay",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "F Sustain",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "F Release",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Unison",
                unit: "",
                group: "Loom",
                choices: daw::params::loom::UNISON,
            },
            ParamLabel {
                name: "Detune",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "Glide",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "P Decay",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "P Env",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "LFO Rate",
                unit: "",
                group: "Loom",
                choices: &[],
            },
            ParamLabel {
                name: "LFO Pitch",
                unit: "",
                group: "Loom",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Poly,
        name: "poly synth",
        instrument: true,
        family: Family::Synths,
        prefix: "poly",
        params: daw::params::poly::TABLE,
        // Thirty-four rows, grouped the way the card's sections are, so a
        // modulation target reads "Filter / Cutoff" rather than an id.
        labels: &[
            ParamLabel {
                name: "Wave",
                unit: "",
                group: "Osc A",
                choices: daw::params::poly::WAVES,
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Osc A",
                choices: daw::params::poly::OCTAVES,
            },
            ParamLabel {
                name: "Semitone",
                unit: "st",
                group: "Osc A",
                choices: &[],
            },
            ParamLabel {
                name: "Fine",
                unit: "ct",
                group: "Osc A",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Osc A",
                choices: &[],
            },
            ParamLabel {
                name: "Pitch Env",
                unit: "st",
                group: "Osc A",
                choices: &[],
            },
            ParamLabel {
                name: "Wave",
                unit: "",
                group: "Osc B",
                choices: daw::params::poly::WAVES,
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Osc B",
                choices: daw::params::poly::OCTAVES,
            },
            ParamLabel {
                name: "Semitone",
                unit: "st",
                group: "Osc B",
                choices: &[],
            },
            ParamLabel {
                name: "Fine",
                unit: "ct",
                group: "Osc B",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Osc B",
                choices: &[],
            },
            ParamLabel {
                name: "Pitch Env",
                unit: "st",
                group: "Osc B",
                choices: &[],
            },
            ParamLabel {
                name: "Color",
                unit: "",
                group: "Noise",
                choices: daw::params::poly::NOISE_COLORS,
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Noise",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Noise",
                choices: &[],
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Filter",
                choices: FILTER_MODES,
            },
            ParamLabel {
                name: "Slope",
                unit: "dB/oct",
                group: "Filter",
                choices: FILTER_SLOPES,
            },
            ParamLabel {
                name: "Cutoff",
                unit: "Hz",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Env Amount",
                unit: "%",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Keytrack",
                unit: "%",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "%",
                group: "Filter",
                choices: &[],
            },
            ParamLabel {
                name: "Drive Position",
                unit: "",
                group: "Filter",
                choices: daw::params::poly::DRIVE_POS,
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Velocity",
                unit: "%",
                group: "Amp",
                choices: &[],
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Voices",
                choices: daw::params::poly::VOICE_MODES,
            },
            ParamLabel {
                name: "Glide",
                unit: "ms",
                group: "Voices",
                choices: &[],
            },
            ParamLabel {
                name: "Unison",
                unit: "",
                group: "Voices",
                choices: daw::params::poly::UNISON,
            },
            ParamLabel {
                name: "Detune",
                unit: "%",
                group: "Voices",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "%",
                group: "Voices",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Filter Env",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Filter Env",
                choices: &[],
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Filter Env",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Filter Env",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Pitch Env",
                choices: &[],
            },
            ParamLabel {
                name: "Wire 1 Source",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_SRC,
            },
            ParamLabel {
                name: "Wire 1 Dest",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_DST,
            },
            ParamLabel {
                name: "Wire 1 Depth",
                unit: "%",
                group: "Matrix",
                choices: &[],
            },
            ParamLabel {
                name: "Wire 2 Source",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_SRC,
            },
            ParamLabel {
                name: "Wire 2 Dest",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_DST,
            },
            ParamLabel {
                name: "Wire 2 Depth",
                unit: "%",
                group: "Matrix",
                choices: &[],
            },
            ParamLabel {
                name: "Wire 3 Source",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_SRC,
            },
            ParamLabel {
                name: "Wire 3 Dest",
                unit: "",
                group: "Matrix",
                choices: daw::params::poly::MOD_DST,
            },
            ParamLabel {
                name: "Wire 3 Depth",
                unit: "%",
                group: "Matrix",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Snare,
        name: "snare",
        instrument: true,
        family: Family::Drums,
        prefix: "snare",
        params: daw::params::snare::TABLE,
        // Grouped as the card's two halves are: the SHELL and the WIRES, so a
        // modulation target reads "Wires / Decay" rather than an id.
        labels: &[
            ParamLabel {
                name: "Tune",
                unit: " Hz",
                group: "Shell",
                choices: &[],
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Shell",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Shell",
                choices: &[],
            },
            ParamLabel {
                name: "Bend",
                unit: " st",
                group: "Shell",
                choices: &[],
            },
            ParamLabel {
                name: "Bend Time",
                unit: " ms",
                group: "Shell",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Wires",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Wires",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Wires",
                choices: &[],
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Wires",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Tom,
        name: "tom",
        instrument: true,
        family: Family::Drums,
        prefix: "tom",
        params: daw::params::tom::TABLE,
        // Grouped as the card's rows are.
        labels: &[
            ParamLabel {
                name: "Tune",
                unit: " Hz",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Bend",
                unit: " st",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Bend Time",
                unit: " ms",
                group: "Body",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Stick",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Stick",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Skin",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Hat,
        name: "808 hat",
        instrument: true,
        family: Family::Drums,
        prefix: "hat",
        params: daw::params::hat::TABLE,
        // Grouped as the card's plot is: the BANK, and the WINDOW it is
        // heard through.
        labels: &[
            ParamLabel {
                name: "Tune",
                unit: "",
                group: "Bank",
                choices: &[],
            },
            ParamLabel {
                name: "Closed",
                unit: " ms",
                group: "Bank",
                choices: &[],
            },
            ParamLabel {
                name: "Open",
                unit: " ms",
                group: "Bank",
                choices: &[],
            },
            ParamLabel {
                name: "Band",
                unit: " Hz",
                group: "Window",
                choices: &[],
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Window",
                choices: &[],
            },
            ParamLabel {
                name: "Highpass",
                unit: " Hz",
                group: "Window",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Handclap,
        name: "clap",
        instrument: true,
        family: Family::Drums,
        prefix: "clap",
        params: daw::params::handclap::TABLE,
        // Grouped as the card's two rows are: the HANDS, then the ROOM and
        // the colour they are heard in.
        labels: &[
            ParamLabel {
                name: "Hands",
                unit: "",
                group: "Hands",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: " ms",
                group: "Hands",
                choices: &[],
            },
            ParamLabel {
                name: "Snap",
                unit: " ms",
                group: "Hands",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Room",
                choices: &[],
            },
            ParamLabel {
                name: "Tail",
                unit: " ms",
                group: "Room",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Highpass",
                unit: " Hz",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Utility,
        name: "utility",
        instrument: false,
        family: Family::Utilities,
        prefix: "util",
        params: daw::params::utility::TABLE,
        // Seven rows, in the table's order. Two groups, because the card
        // reads as two: what the device does to the LEVEL and where it
        // puts the track, then the three repairs.
        //
        // The units a MODULATION READOUT appends. Pan, width and the
        // three switches are blank for the reason the saturator's are:
        // the card prints "L35", "120 %" and "swap" through its own
        // formatters, and a second opinion here would be a second answer
        // to the same question.
        labels: &[
            ParamLabel {
                name: "Gain",
                unit: " dB",
                group: "Level",
                choices: &[],
            },
            ParamLabel {
                name: "Pan",
                unit: "",
                group: "Level",
                choices: &[],
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Image",
                choices: &[],
            },
            ParamLabel {
                name: "Mono",
                unit: " Hz",
                group: "Image",
                choices: &[],
            },
            ParamLabel {
                name: "Phase",
                unit: "",
                group: "Repair",
                choices: daw::params::utility::PHASE_NAMES,
            },
            ParamLabel {
                name: "Channel",
                unit: "",
                group: "Repair",
                choices: daw::params::utility::CHANNEL_NAMES,
            },
            ParamLabel {
                name: "DC",
                unit: "",
                group: "Repair",
                choices: daw::params::utility::SWITCH_NAMES,
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Modulato,
        name: "modulato",
        instrument: false,
        family: Family::Modulation,
        prefix: "modulato",
        params: daw::params::modulato::TABLE,
        // Seven rows, in the table's order. Grouped as the card reads:
        // what KIND of movement, then its shape, then how it lands.
        labels: &[
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Modulato",
                choices: daw::params::modulato::MODE_NAMES,
            },
            ParamLabel {
                name: "Rate",
                unit: " Hz",
                group: "Movement",
                choices: &[],
            },
            ParamLabel {
                name: "Depth",
                unit: " ms",
                group: "Movement",
                choices: &[],
            },
            ParamLabel {
                name: "Delay",
                unit: "",
                group: "Movement",
                choices: &[],
            },
            ParamLabel {
                name: "Feedback",
                unit: "",
                group: "Voice",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Voice",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Voice",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Filter,
        name: "filter",
        instrument: false,
        family: Family::EqAndFilters,
        prefix: "filter",
        params: daw::params::filter::TABLE,
        // Grouped as the card's two rows are: what SHAPE the filter is,
        // then what it does to the sound on the way through — so a
        // modulation target reads "Colour / Drive" rather than an id.
        labels: &[
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Shape",
                choices: FILTER_MODES,
            },
            ParamLabel {
                name: "Slope",
                unit: " dB/oct",
                group: "Shape",
                choices: FILTER_SLOPES,
            },
            ParamLabel {
                name: "Cutoff",
                unit: " Hz",
                group: "Shape",
                choices: &[],
            },
            ParamLabel {
                name: "Res",
                unit: "",
                group: "Shape",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Character",
                unit: "",
                group: "Colour",
                choices: daw::params::filter::CHARACTER_NAMES,
            },
            ParamLabel {
                name: "Spread",
                unit: " st",
                group: "Colour",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Limiter,
        name: "limiter",
        instrument: false,
        family: Family::Dynamics,
        prefix: "limiter",
        params: daw::params::limiter::TABLE,
        // Grouped as the card's two rows are: what it does to the LEVEL,
        // then what it does to the SOUND — so a modulation target reads
        // "Colour / Warmth" rather than an id.
        labels: &[
            ParamLabel {
                name: "Push",
                unit: " dB",
                group: "Level",
                choices: &[],
            },
            ParamLabel {
                name: "Ceiling",
                unit: " dB",
                group: "Level",
                choices: &[],
            },
            ParamLabel {
                name: "Style",
                unit: "",
                group: "Level",
                choices: daw::params::limiter::STYLE_NAMES,
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Level",
                choices: &[],
            },
            ParamLabel {
                name: "Warmth",
                unit: "",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Fuzz",
                unit: "",
                group: "Colour",
                choices: &[],
            },
            ParamLabel {
                name: "Brighten",
                unit: "",
                group: "Colour",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Reverb,
        name: "reverb",
        instrument: false,
        family: Family::DelayAndReverb,
        prefix: "reverb",
        params: daw::params::reverb::TABLE,
        // Nine rows, in the table's order, grouped the way the card's
        // two strips are: the SPACE, then how it is PRESENTED.
        labels: &[
            ParamLabel {
                name: "Pre-delay",
                unit: " ms",
                group: "Space",
                choices: &[],
            },
            ParamLabel {
                name: "Size",
                unit: "",
                group: "Space",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: " s",
                group: "Space",
                choices: &[],
            },
            ParamLabel {
                name: "Damping",
                unit: " Hz",
                group: "Space",
                choices: &[],
            },
            ParamLabel {
                name: "Low cut",
                unit: " Hz",
                group: "Space",
                choices: &[],
            },
            ParamLabel {
                name: "Diffusion",
                unit: "",
                group: "Character",
                choices: &[],
            },
            ParamLabel {
                name: "Modulation",
                unit: "",
                group: "Character",
                choices: &[],
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Character",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Character",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Echo,
        name: "delay",
        instrument: false,
        family: Family::DelayAndReverb,
        prefix: "echo",
        params: daw::params::echo::TABLE,
        labels: &[
            ParamLabel {
                name: "Sync",
                unit: "",
                group: "Delay",
                choices: daw::params::echo::SYNC_NAMES,
            },
            ParamLabel {
                name: "Time",
                unit: "ms",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Feedback",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: "Hz",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Wow",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
            ParamLabel {
                name: "Send",
                unit: "%",
                group: "Delay",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Clamp,
        name: "clamp",
        instrument: false,
        family: Family::Dynamics,
        prefix: "clamp",
        params: daw::params::clamp::TABLE,
        labels: &[
            ParamLabel {
                name: "Threshold",
                unit: "dB",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Ratio",
                unit: ":1",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Knee",
                unit: "dB",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Makeup",
                unit: "dB",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Sidechain HP",
                unit: "Hz",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Warmth",
                unit: "%",
                group: "Clamp",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Clamp",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Prism,
        name: "prism",
        instrument: false,
        family: Family::Dynamics,
        prefix: "prism",
        params: daw::params::prism::TABLE,
        labels: &[
            ParamLabel {
                name: "Low crossover",
                unit: "Hz",
                group: "Prism",
                choices: &[],
            },
            ParamLabel {
                name: "High crossover",
                unit: "Hz",
                group: "Prism",
                choices: &[],
            },
            ParamLabel {
                name: "Grip",
                unit: "%",
                group: "Prism",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Prism",
                choices: &[],
            },
            ParamLabel {
                name: "Output",
                unit: "dB",
                group: "Prism",
                choices: &[],
            },
            ParamLabel {
                name: "Low threshold",
                unit: "dB",
                group: "Low",
                choices: &[],
            },
            ParamLabel {
                name: "Low amount",
                unit: "%",
                group: "Low",
                choices: &[],
            },
            ParamLabel {
                name: "Low heat",
                unit: "%",
                group: "Low",
                choices: &[],
            },
            ParamLabel {
                name: "Low trim",
                unit: "dB",
                group: "Low",
                choices: &[],
            },
            ParamLabel {
                name: "Mid threshold",
                unit: "dB",
                group: "Mid",
                choices: &[],
            },
            ParamLabel {
                name: "Mid amount",
                unit: "%",
                group: "Mid",
                choices: &[],
            },
            ParamLabel {
                name: "Mid heat",
                unit: "%",
                group: "Mid",
                choices: &[],
            },
            ParamLabel {
                name: "Mid trim",
                unit: "dB",
                group: "Mid",
                choices: &[],
            },
            ParamLabel {
                name: "High threshold",
                unit: "dB",
                group: "High",
                choices: &[],
            },
            ParamLabel {
                name: "High amount",
                unit: "%",
                group: "High",
                choices: &[],
            },
            ParamLabel {
                name: "High heat",
                unit: "%",
                group: "High",
                choices: &[],
            },
            ParamLabel {
                name: "High trim",
                unit: "dB",
                group: "High",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Glue,
        name: "glue",
        instrument: false,
        family: Family::Dynamics,
        prefix: "glue",
        params: daw::params::glue::TABLE,
        labels: &[
            ParamLabel {
                name: "Threshold",
                unit: "dB",
                group: "Glue",
                choices: &[],
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Glue",
                choices: daw::params::glue::RATIO_NAMES,
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Glue",
                choices: daw::params::glue::ATTACK_NAMES,
            },
            ParamLabel {
                name: "Release",
                unit: "s",
                group: "Glue",
                choices: daw::params::glue::RELEASE_NAMES,
            },
            ParamLabel {
                name: "Makeup",
                unit: "dB",
                group: "Glue",
                choices: &[],
            },
            ParamLabel {
                name: "Dry/Wet",
                unit: "%",
                group: "Glue",
                choices: &[],
            },
            ParamLabel {
                name: "Range",
                unit: "dB",
                group: "Glue",
                choices: &[],
            },
            ParamLabel {
                name: "Clip",
                unit: "",
                group: "Glue",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "SC HP",
                unit: "Hz",
                group: "Glue",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Eq,
        name: "eq",
        instrument: false,
        family: Family::EqAndFilters,
        prefix: "eq",
        params: daw::params::eq::TABLE,
        // One group per BAND, so a modulation picker offering forty-one
        // rows offers them as eight small families rather than as one
        // list nobody can find anything in. The group is what carries
        // which band a row belongs to; the name says only which slot.
        labels: &[
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 1",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 1",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 1",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 1",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 1",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 2",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 2",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 2",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 2",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 2",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 3",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 3",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 3",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 3",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 3",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 4",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 4",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 4",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 4",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 4",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 5",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 5",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 5",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 5",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 5",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 6",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 6",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 6",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 6",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 6",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 7",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 7",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 7",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 7",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 7",
                choices: &[],
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 8",
                choices: OFF_ON,
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 8",
                choices: daw::params::eq::TYPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 8",
                choices: &[],
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 8",
                choices: &[],
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 8",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "dB",
                group: "EQ",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Sat,
        name: "saturator",
        instrument: false,
        family: Family::Distortion,
        prefix: "sat",
        params: daw::params::sat::TABLE,
        // The units a MODULATION READOUT appends, which is why drive and
        // bias are blank: the card shows "4.0x" and "+25 %" through its
        // own `Unit`, and a second opinion here would be a second answer
        // to the same question.
        labels: &[
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Saturator",
                choices: SHAPER_MODES,
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Saturator",
                choices: &[],
            },
            ParamLabel {
                name: "Bias",
                unit: "",
                group: "Saturator",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Saturator",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Saturator",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Lofi,
        name: "lo-fi",
        instrument: false,
        family: Family::Distortion,
        prefix: "lofi",
        params: daw::params::lofi::TABLE,
        // Units blank for the reason the saturator's are: the card prints
        // "22.05 kHz", "12" and "-6.0 dB" through its own `Unit`, and a
        // second opinion here would be a second answer to one question.
        labels: &[
            ParamLabel {
                name: "Rate",
                unit: "",
                group: "Lo-fi",
                choices: &[],
            },
            ParamLabel {
                name: "Bits",
                unit: "",
                group: "Lo-fi",
                choices: BIT_DEPTHS,
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Lo-fi",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Lo-fi",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Flint,
        name: "flint",
        instrument: false,
        family: Family::Dynamics,
        prefix: "flint",
        params: daw::params::flint::TABLE,
        // Units blank for the reason the lo-fi's are: the card prints
        // "+4.5 dB", "12 ms" and "65 %" through its own `Unit`, and a
        // second opinion here would be a second answer to one question.
        labels: &[
            ParamLabel {
                name: "Strike",
                unit: "",
                group: "Flint",
                choices: &[],
            },
            ParamLabel {
                name: "Body",
                unit: "",
                group: "Flint",
                choices: &[],
            },
            ParamLabel {
                name: "Split",
                unit: "",
                group: "Flint",
                choices: &[],
            },
            ParamLabel {
                name: "Colour",
                unit: "",
                group: "Flint",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Flint",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Flint",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Sibyl,
        name: "sibyl",
        instrument: false,
        family: Family::Spectral,
        prefix: "sibyl",
        params: daw::params::sibyl::TABLE,
        // Units blank for the reason the lo-fi's are: the card prints
        // "+7 st", "C", "minor" and "55 %" through its own `Unit`, and a
        // second opinion here would be a second answer to one question.
        labels: &[
            ParamLabel {
                name: "Shift",
                unit: "",
                group: "Sibyl",
                choices: &[],
            },
            ParamLabel {
                name: "Formant",
                unit: "",
                group: "Sibyl",
                choices: &[],
            },
            ParamLabel {
                name: "Voice A",
                unit: "",
                group: "Sibyl",
                choices: SHIFT_VOICES,
            },
            ParamLabel {
                name: "Voice B",
                unit: "",
                group: "Sibyl",
                choices: SHIFT_VOICES,
            },
            ParamLabel {
                name: "Key",
                unit: "",
                group: "Sibyl",
                choices: daw::params::sibyl::KEY_NAMES,
            },
            ParamLabel {
                name: "Scale",
                unit: "",
                group: "Sibyl",
                choices: daw::params::sibyl::SCALE_NAMES,
            },
            ParamLabel {
                name: "Blend",
                unit: "",
                group: "Sibyl",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sibyl",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Sibyl",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Ferric,
        name: "ferric",
        instrument: false,
        family: Family::DelayAndReverb,
        prefix: "ferric",
        params: daw::params::ferric::TABLE,
        labels: &[
            ParamLabel {
                name: "Speed",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Division",
                unit: "",
                group: "Ferric",
                choices: daw::params::ferric::DIVISION_NAMES,
            },
            ParamLabel {
                name: "Pattern",
                unit: "",
                group: "Ferric",
                choices: daw::params::ferric::PATTERN_NAMES,
            },
            ParamLabel {
                name: "Groove",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Wow",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Age",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Ferric",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Umbra,
        name: "umbra",
        instrument: false,
        family: Family::DelayAndReverb,
        prefix: "umbra",
        params: daw::params::umbra::TABLE,
        labels: &[
            ParamLabel {
                name: "Depth",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
            ParamLabel {
                name: "Time",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
            ParamLabel {
                name: "Duck",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Umbra",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Tone,
        name: "tone",
        instrument: false,
        family: Family::Utilities,
        prefix: "tone",
        params: daw::params::tone::TABLE,
        labels: &[
            ParamLabel {
                name: "Shape",
                unit: "",
                group: "Tone",
                choices: daw::params::tone::SHAPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Tone",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Tone",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Sigil,
        name: "sigil",
        instrument: false,
        family: Family::Modulation,
        prefix: "sigil",
        params: daw::params::sigil::TABLE,
        labels: &[
            ParamLabel {
                name: "Shape",
                unit: "",
                group: "Sigil",
                choices: daw::params::sigil::SHAPE_NAMES,
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Sigil",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sigil",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Gauge,
        name: "gauge",
        instrument: false,
        family: Family::Utilities,
        prefix: "gauge",
        params: daw::params::gauge::TABLE,
        labels: &[
            ParamLabel {
                name: "Window",
                unit: "",
                group: "Gauge",
                choices: &[],
            },
            ParamLabel {
                name: "Hold",
                unit: "",
                group: "Gauge",
                choices: &[],
            },
            ParamLabel {
                name: "Range",
                unit: "",
                group: "Gauge",
                choices: daw::params::gauge::RANGE_NAMES,
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Tine,
        name: "tine",
        instrument: true,
        family: Family::Synths,
        prefix: "tine",
        params: daw::params::tine::TABLE,
        labels: &[
            ParamLabel {
                name: "Material",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Strike",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Place",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Body",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Tone",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Tune",
                unit: "",
                group: "Tine",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Tine",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Sheen,
        name: "sheen",
        instrument: false,
        family: Family::Distortion,
        prefix: "sheen",
        params: daw::params::sheen::TABLE,
        // Units blank for the reason the saturator's and the lo-fi's are:
        // the card prints "30 %", "1.50 kHz" and "-6.0 dB" through its own
        // `Unit`, and a second opinion here would be a second answer.
        labels: &[
            ParamLabel {
                name: "Amount",
                unit: "",
                group: "Sheen",
                choices: &[],
            },
            ParamLabel {
                name: "Edge",
                unit: "",
                group: "Sheen",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sheen",
                choices: &[],
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Sheen",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Disperser,
        name: "disperser",
        instrument: false,
        family: Family::Modulation,
        prefix: "disp",
        params: daw::params::disperser::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "8", "500 Hz" and "1.00" through its own `Unit`.
        labels: &[
            ParamLabel {
                name: "Amount",
                unit: "",
                group: "Disperser",
                choices: STAGES_TO_32,
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Disperser",
                choices: &[],
            },
            ParamLabel {
                name: "Pinch",
                unit: "",
                group: "Disperser",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Tilt,
        name: "tilt",
        instrument: false,
        family: Family::EqAndFilters,
        prefix: "tilt",
        params: daw::params::tilt::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "+6.0 dB" and "1.00 kHz" through its own `Unit`.
        labels: &[
            ParamLabel {
                name: "Tilt",
                unit: "",
                group: "Tilt",
                choices: &[],
            },
            ParamLabel {
                name: "Pivot",
                unit: "",
                group: "Tilt",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Phaser,
        name: "phaser",
        instrument: false,
        family: Family::Modulation,
        prefix: "phaser",
        params: daw::params::phaser::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "4", "800 Hz", "2.00", "0.40 Hz" and "50 %" through its own
        // `Unit`.
        labels: &[
            ParamLabel {
                name: "Amount",
                unit: "",
                group: "Phaser",
                choices: STAGES_TO_16,
            },
            ParamLabel {
                name: "Centre",
                unit: "",
                group: "Phaser",
                choices: &[],
            },
            ParamLabel {
                name: "Depth",
                unit: "",
                group: "Phaser",
                choices: &[],
            },
            ParamLabel {
                name: "Rate",
                unit: "",
                group: "Phaser",
                choices: &[],
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Phaser",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Gate,
        name: "gate",
        instrument: false,
        family: Family::Dynamics,
        prefix: "gate",
        params: daw::params::gate::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "-40.0 dB", "8.00", "1.00 ms" and "-60.0 dB" through its own
        // `Unit`.
        labels: &[
            ParamLabel {
                name: "Threshold",
                unit: "",
                group: "Gate",
                choices: &[],
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Gate",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Gate",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Gate",
                choices: &[],
            },
            ParamLabel {
                name: "Range",
                unit: "",
                group: "Gate",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Strip,
        name: "strip",
        instrument: false,
        family: Family::EqAndFilters,
        prefix: "strip",
        params: daw::params::strip::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "+3.0 dB", "35 %" and "on" through its own `Unit`.
        labels: &[
            ParamLabel {
                name: "Low",
                unit: "",
                group: "Strip",
                choices: &[],
            },
            ParamLabel {
                name: "High",
                unit: "",
                group: "Strip",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Strip",
                choices: &[],
            },
            ParamLabel {
                name: "Warm",
                unit: "",
                group: "Strip",
                choices: daw::params::strip::WARM_NAMES,
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Strip",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Resyn,
        name: "resyn",
        instrument: false,
        family: Family::Spectral,
        prefix: "resyn",
        params: daw::params::resyn::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "+2 st", "200 Hz", "80 ms" and "on" through its own `Unit`.
        labels: &[
            ParamLabel {
                name: "Formant",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Shift",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Warm",
                unit: "",
                group: "Resyn",
                choices: daw::params::resyn::WARM_NAMES,
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 1",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 2",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 3",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 4",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 5",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 6",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 7",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
            ParamLabel {
                name: "Band 8",
                unit: "",
                group: "Resyn",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Acid,
        name: "acid",
        instrument: true,
        family: Family::Synths,
        prefix: "acid",
        params: daw::params::acid::TABLE,
        // Units blank for the reason the others' are: the card prints
        // "saw", "+0 st", "400 Hz" and "65 %" through its own `Unit`.
        labels: &[
            ParamLabel {
                name: "Wave",
                unit: "",
                group: "Acid",
                choices: daw::params::acid::WAVE_NAMES,
            },
            ParamLabel {
                name: "Tune",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Cutoff",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Env mod",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Accent",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Glide",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Acid",
                choices: &[],
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Acid",
                choices: &[],
            },
        ],
    },
    DeviceSpec {
        kind: DeviceKind::Rack,
        name: "rack",
        instrument: false,
        family: Family::Utilities,
        prefix: "rack",
        // EMPTY, and that is the device: a rack is a container with no
        // sound and no settings of its own. Its macros are not parameters
        // either — they point at other devices' parameters, and live on
        // the track beside the chain.
        params: &[],
        labels: &[],
    },
];

/// The device a target's prefix names, if any. The parse side of
/// [`DeviceSpec::prefix`].
pub fn device_by_prefix(prefix: &str) -> Option<&'static DeviceSpec> {
    DEVICES.iter().find(|spec| spec.prefix == prefix)
}

/// Where devices are filed. The registry is the only catalog, so these
/// hold it to the two promises a browser built from it depends on:
/// nothing is loose, and the section a device is filed under is the same
/// fact as whether it is an instrument.
#[cfg(test)]
mod family_tests {
    use super::*;

    /// A choice list is the parameter's whole range, one name per
    /// position: a list one short would leave a position the band cannot
    /// name, and one too long would name a position the engine clamps
    /// away. Every device, because this is the kind of thing one device
    /// always gets wrong.
    #[test]
    fn every_choice_list_names_exactly_the_positions_its_range_has() {
        for spec in DEVICES {
            for (def, label) in spec.params.iter().zip(spec.labels) {
                if label.choices.is_empty() {
                    continue;
                }
                let positions = (def.max - def.min) as usize + 1;
                assert_eq!(
                    label.choices.len(),
                    positions,
                    "{} / {} names {} positions for a range of {}",
                    spec.name,
                    label.name,
                    label.choices.len(),
                    positions
                );
                // Whole bounds, so position `n` is exactly `min + n`.
                assert_eq!(def.min.fract(), 0.0, "{} / {}", spec.name, label.name);
                assert_eq!(def.max.fract(), 0.0, "{} / {}", spec.name, label.name);
            }
        }
    }

    /// The catalog knows which of its parameters are lists. Not every
    /// one — a range of `0..=8` that is a modulation depth stays a
    /// range — but the ones the cards step through as lists are lists
    /// here too, or the band and the card disagree about what a press
    /// does.
    #[test]
    fn the_lists_the_cards_walk_are_lists_in_the_catalog() {
        let listed = |kind: DeviceKind, id: u32| {
            let spec = kind.spec();
            let at = spec
                .params
                .iter()
                .position(|def| def.id == id)
                .expect("the id is in the table");
            !spec.labels[at].choices.is_empty()
        };
        assert!(listed(DeviceKind::Poly, daw::params::poly::A_WAVE));
        assert!(listed(DeviceKind::Poly, daw::params::poly::F_MODE));
        assert!(listed(DeviceKind::Filter, daw::params::filter::MODE));
        assert!(listed(DeviceKind::Sat, daw::params::sat::MODE));
        assert!(listed(DeviceKind::Sampler, daw::params::sampler::LOOP_MODE));
        assert!(listed(DeviceKind::Echo, daw::params::echo::SYNC));
        assert!(!listed(DeviceKind::Poly, daw::params::poly::GAIN));
        assert!(!listed(DeviceKind::Reverb, daw::params::reverb::MIX));
    }

    #[test]
    fn a_devices_section_and_its_instrument_flag_are_the_same_fact() {
        for spec in DEVICES {
            let filed_as_instrument = spec.family.section() == Section::Instruments;
            assert_eq!(
                spec.instrument,
                filed_as_instrument,
                "{} is filed under {} but its instrument flag says otherwise",
                spec.name,
                spec.family.section().label()
            );
        }
    }

    #[test]
    fn every_family_holds_something() {
        for family in Family::ALL {
            assert!(
                DEVICES.iter().any(|spec| spec.family == family),
                "the family {} has no devices, so it is a heading over nothing",
                family.label()
            );
        }
    }

    #[test]
    fn every_device_is_reachable_by_walking_the_headings() {
        let walked: usize = Section::ALL
            .into_iter()
            .flat_map(|section| {
                Family::ALL
                    .into_iter()
                    .filter(move |family| family.section() == section)
            })
            .map(|family| DEVICES.iter().filter(|spec| spec.family == family).count())
            .sum();
        assert_eq!(
            walked,
            DEVICES.len(),
            "a device is in the registry but not under any heading"
        );
    }

    #[test]
    fn no_two_families_share_a_name() {
        for (index, family) in Family::ALL.iter().enumerate() {
            for other in &Family::ALL[index + 1..] {
                assert_ne!(family.label(), other.label());
            }
        }
    }
}
