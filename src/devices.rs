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
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Synth",
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Synth",
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
            },
            ParamLabel {
                name: "Start",
                unit: " %",
                group: "Sample",
            },
            ParamLabel {
                name: "End",
                unit: " %",
                group: "Sample",
            },
            ParamLabel {
                name: "Reverse",
                unit: "",
                group: "Sample",
            },
            ParamLabel {
                name: "Fade In",
                unit: " ms",
                group: "Sample",
            },
            ParamLabel {
                name: "Fade Out",
                unit: " ms",
                group: "Sample",
            },
            ParamLabel {
                name: "Root",
                unit: "",
                group: "Pitch",
            },
            ParamLabel {
                name: "Tune",
                unit: " st",
                group: "Pitch",
            },
            ParamLabel {
                name: "Fine",
                unit: " ct",
                group: "Pitch",
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Loop",
            },
            ParamLabel {
                name: "Start",
                unit: " %",
                group: "Loop",
            },
            ParamLabel {
                name: "Crossfade",
                unit: " ms",
                group: "Loop",
            },
            ParamLabel {
                name: "Slices",
                unit: "",
                group: "Slice",
            },
            ParamLabel {
                name: "Source",
                unit: "",
                group: "Slice",
            },
            ParamLabel {
                name: "Choke",
                unit: "",
                group: "Slice",
            },
            ParamLabel {
                name: "Attack",
                unit: " ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Sustain",
                unit: " %",
                group: "Amp",
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Cutoff",
                unit: " Hz",
                group: "Filter",
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Keytrack",
                unit: " %",
                group: "Filter",
            },
            ParamLabel {
                name: "Attack",
                unit: " ms",
                group: "Mod",
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Mod",
            },
            ParamLabel {
                name: "Sustain",
                unit: " %",
                group: "Mod",
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Mod",
            },
            ParamLabel {
                name: "Destination",
                unit: "",
                group: "Mod",
            },
            ParamLabel {
                name: "Depth",
                unit: " %",
                group: "Mod",
            },
            ParamLabel {
                name: "Velocity",
                unit: " %",
                group: "Mod",
            },
            ParamLabel {
                name: "Drive",
                unit: " %",
                group: "Dirt",
            },
            ParamLabel {
                name: "Rate",
                unit: " Hz",
                group: "Dirt",
            },
            ParamLabel {
                name: "Bits",
                unit: "",
                group: "Dirt",
            },
            ParamLabel {
                name: "Pre-amp",
                unit: " %",
                group: "Dirt",
            },
            ParamLabel {
                name: "Gain",
                unit: " dB",
                group: "Out",
            },
            ParamLabel {
                name: "Pan",
                unit: " %",
                group: "Out",
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
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Body",
            },
            ParamLabel {
                name: "Depth",
                unit: " st",
                group: "Punch",
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Punch",
            },
            ParamLabel {
                name: "Depth",
                unit: " st",
                group: "Sweep",
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Sweep",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Click",
            },
            ParamLabel {
                name: "Time",
                unit: " ms",
                group: "Click",
            },
            ParamLabel {
                name: "Stages",
                unit: "",
                group: "Disperse",
            },
            ParamLabel {
                name: "Harmonic",
                unit: "",
                group: "Disperse",
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Disperse",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
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
            },
            ParamLabel {
                name: "Shape",
                unit: "%",
                group: "Tone",
            },
            ParamLabel {
                name: "Sub",
                unit: "%",
                group: "Tone",
            },
            ParamLabel {
                name: "Drift",
                unit: "%",
                group: "Tone",
            },
            ParamLabel {
                name: "Cutoff",
                unit: "Hz",
                group: "Filter",
            },
            ParamLabel {
                name: "Resonance",
                unit: "%",
                group: "Filter",
            },
            ParamLabel {
                name: "Keytrack",
                unit: "%",
                group: "Filter",
            },
            ParamLabel {
                name: "Env amount",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Attack",
                unit: "s",
                group: "Amp",
            },
            ParamLabel {
                name: "Decay",
                unit: "s",
                group: "Amp",
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Amp",
            },
            ParamLabel {
                name: "Release",
                unit: "s",
                group: "Amp",
            },
            ParamLabel {
                name: "Filter attack",
                unit: "s",
                group: "Filter",
            },
            ParamLabel {
                name: "Filter decay",
                unit: "s",
                group: "Filter",
            },
            ParamLabel {
                name: "Ensemble",
                unit: "%",
                group: "Air",
            },
            ParamLabel {
                name: "Wow",
                unit: "%",
                group: "Air",
            },
            ParamLabel {
                name: "Grain",
                unit: "%",
                group: "Air",
            },
            ParamLabel {
                name: "Warmth",
                unit: "%",
                group: "Air",
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Air",
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
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Semi",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Morph",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Semi",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Noise",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "N Decay",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Cutoff",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Res",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "F Env",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Sustain",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Velocity",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "F Attack",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "F Decay",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "F Sustain",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "F Release",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Unison",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Detune",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "Glide",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "P Decay",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "P Env",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "LFO Rate",
                unit: "",
                group: "Loom",
            },
            ParamLabel {
                name: "LFO Pitch",
                unit: "",
                group: "Loom",
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
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Osc A",
            },
            ParamLabel {
                name: "Semitone",
                unit: "st",
                group: "Osc A",
            },
            ParamLabel {
                name: "Fine",
                unit: "ct",
                group: "Osc A",
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Osc A",
            },
            ParamLabel {
                name: "Pitch Env",
                unit: "st",
                group: "Osc A",
            },
            ParamLabel {
                name: "Wave",
                unit: "",
                group: "Osc B",
            },
            ParamLabel {
                name: "Octave",
                unit: "",
                group: "Osc B",
            },
            ParamLabel {
                name: "Semitone",
                unit: "st",
                group: "Osc B",
            },
            ParamLabel {
                name: "Fine",
                unit: "ct",
                group: "Osc B",
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Osc B",
            },
            ParamLabel {
                name: "Pitch Env",
                unit: "st",
                group: "Osc B",
            },
            ParamLabel {
                name: "Color",
                unit: "",
                group: "Noise",
            },
            ParamLabel {
                name: "Level",
                unit: "%",
                group: "Noise",
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Noise",
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Slope",
                unit: "dB/oct",
                group: "Filter",
            },
            ParamLabel {
                name: "Cutoff",
                unit: "Hz",
                group: "Filter",
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Env Amount",
                unit: "%",
                group: "Filter",
            },
            ParamLabel {
                name: "Keytrack",
                unit: "%",
                group: "Filter",
            },
            ParamLabel {
                name: "Drive",
                unit: "%",
                group: "Filter",
            },
            ParamLabel {
                name: "Drive Position",
                unit: "",
                group: "Filter",
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Amp",
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Amp",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Amp",
            },
            ParamLabel {
                name: "Velocity",
                unit: "%",
                group: "Amp",
            },
            ParamLabel {
                name: "Mode",
                unit: "",
                group: "Voices",
            },
            ParamLabel {
                name: "Glide",
                unit: "ms",
                group: "Voices",
            },
            ParamLabel {
                name: "Unison",
                unit: "",
                group: "Voices",
            },
            ParamLabel {
                name: "Detune",
                unit: "%",
                group: "Voices",
            },
            ParamLabel {
                name: "Spread",
                unit: "%",
                group: "Voices",
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Filter Env",
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Filter Env",
            },
            ParamLabel {
                name: "Sustain",
                unit: "%",
                group: "Filter Env",
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Filter Env",
            },
            ParamLabel {
                name: "Decay",
                unit: "ms",
                group: "Pitch Env",
            },
            ParamLabel {
                name: "Wire 1 Source",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 1 Dest",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 1 Depth",
                unit: "%",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 2 Source",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 2 Dest",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 2 Depth",
                unit: "%",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 3 Source",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 3 Dest",
                unit: "",
                group: "Matrix",
            },
            ParamLabel {
                name: "Wire 3 Depth",
                unit: "%",
                group: "Matrix",
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
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Shell",
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Shell",
            },
            ParamLabel {
                name: "Bend",
                unit: " st",
                group: "Shell",
            },
            ParamLabel {
                name: "Bend Time",
                unit: " ms",
                group: "Shell",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Wires",
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Wires",
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Wires",
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Wires",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
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
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Body",
            },
            ParamLabel {
                name: "Bend",
                unit: " st",
                group: "Body",
            },
            ParamLabel {
                name: "Bend Time",
                unit: " ms",
                group: "Body",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Stick",
            },
            ParamLabel {
                name: "Decay",
                unit: " ms",
                group: "Stick",
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Skin",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
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
            },
            ParamLabel {
                name: "Closed",
                unit: " ms",
                group: "Bank",
            },
            ParamLabel {
                name: "Open",
                unit: " ms",
                group: "Bank",
            },
            ParamLabel {
                name: "Band",
                unit: " Hz",
                group: "Window",
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Window",
            },
            ParamLabel {
                name: "Highpass",
                unit: " Hz",
                group: "Window",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
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
            },
            ParamLabel {
                name: "Spread",
                unit: " ms",
                group: "Hands",
            },
            ParamLabel {
                name: "Snap",
                unit: " ms",
                group: "Hands",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Room",
            },
            ParamLabel {
                name: "Tail",
                unit: " ms",
                group: "Room",
            },
            ParamLabel {
                name: "Tone",
                unit: " Hz",
                group: "Colour",
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Colour",
            },
            ParamLabel {
                name: "Highpass",
                unit: " Hz",
                group: "Colour",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Out",
            },
            ParamLabel {
                name: "Gain",
                unit: "",
                group: "Out",
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
            },
            ParamLabel {
                name: "Pan",
                unit: "",
                group: "Level",
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Image",
            },
            ParamLabel {
                name: "Mono",
                unit: " Hz",
                group: "Image",
            },
            ParamLabel {
                name: "Phase",
                unit: "",
                group: "Repair",
            },
            ParamLabel {
                name: "Channel",
                unit: "",
                group: "Repair",
            },
            ParamLabel {
                name: "DC",
                unit: "",
                group: "Repair",
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
            },
            ParamLabel {
                name: "Rate",
                unit: " Hz",
                group: "Movement",
            },
            ParamLabel {
                name: "Depth",
                unit: " ms",
                group: "Movement",
            },
            ParamLabel {
                name: "Delay",
                unit: "",
                group: "Movement",
            },
            ParamLabel {
                name: "Feedback",
                unit: "",
                group: "Voice",
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Voice",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Voice",
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
            },
            ParamLabel {
                name: "Slope",
                unit: " dB/oct",
                group: "Shape",
            },
            ParamLabel {
                name: "Cutoff",
                unit: " Hz",
                group: "Shape",
            },
            ParamLabel {
                name: "Res",
                unit: "",
                group: "Shape",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Colour",
            },
            ParamLabel {
                name: "Character",
                unit: "",
                group: "Colour",
            },
            ParamLabel {
                name: "Spread",
                unit: " st",
                group: "Colour",
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
            },
            ParamLabel {
                name: "Ceiling",
                unit: " dB",
                group: "Level",
            },
            ParamLabel {
                name: "Style",
                unit: "",
                group: "Level",
            },
            ParamLabel {
                name: "Release",
                unit: " ms",
                group: "Level",
            },
            ParamLabel {
                name: "Warmth",
                unit: "",
                group: "Colour",
            },
            ParamLabel {
                name: "Fuzz",
                unit: "",
                group: "Colour",
            },
            ParamLabel {
                name: "Brighten",
                unit: "",
                group: "Colour",
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
            },
            ParamLabel {
                name: "Size",
                unit: "",
                group: "Space",
            },
            ParamLabel {
                name: "Decay",
                unit: " s",
                group: "Space",
            },
            ParamLabel {
                name: "Damping",
                unit: " Hz",
                group: "Space",
            },
            ParamLabel {
                name: "Low cut",
                unit: " Hz",
                group: "Space",
            },
            ParamLabel {
                name: "Diffusion",
                unit: "",
                group: "Character",
            },
            ParamLabel {
                name: "Modulation",
                unit: "",
                group: "Character",
            },
            ParamLabel {
                name: "Width",
                unit: "",
                group: "Character",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Character",
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
            },
            ParamLabel {
                name: "Time",
                unit: "ms",
                group: "Delay",
            },
            ParamLabel {
                name: "Feedback",
                unit: "%",
                group: "Delay",
            },
            ParamLabel {
                name: "Tone",
                unit: "Hz",
                group: "Delay",
            },
            ParamLabel {
                name: "Drive",
                unit: "%",
                group: "Delay",
            },
            ParamLabel {
                name: "Wow",
                unit: "%",
                group: "Delay",
            },
            ParamLabel {
                name: "Spread",
                unit: "%",
                group: "Delay",
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Delay",
            },
            ParamLabel {
                name: "Send",
                unit: "%",
                group: "Delay",
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
            },
            ParamLabel {
                name: "Ratio",
                unit: ":1",
                group: "Clamp",
            },
            ParamLabel {
                name: "Knee",
                unit: "dB",
                group: "Clamp",
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Clamp",
            },
            ParamLabel {
                name: "Release",
                unit: "ms",
                group: "Clamp",
            },
            ParamLabel {
                name: "Makeup",
                unit: "dB",
                group: "Clamp",
            },
            ParamLabel {
                name: "Sidechain HP",
                unit: "Hz",
                group: "Clamp",
            },
            ParamLabel {
                name: "Warmth",
                unit: "%",
                group: "Clamp",
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Clamp",
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
            },
            ParamLabel {
                name: "High crossover",
                unit: "Hz",
                group: "Prism",
            },
            ParamLabel {
                name: "Grip",
                unit: "%",
                group: "Prism",
            },
            ParamLabel {
                name: "Mix",
                unit: "%",
                group: "Prism",
            },
            ParamLabel {
                name: "Output",
                unit: "dB",
                group: "Prism",
            },
            ParamLabel {
                name: "Low threshold",
                unit: "dB",
                group: "Low",
            },
            ParamLabel {
                name: "Low amount",
                unit: "%",
                group: "Low",
            },
            ParamLabel {
                name: "Low heat",
                unit: "%",
                group: "Low",
            },
            ParamLabel {
                name: "Low trim",
                unit: "dB",
                group: "Low",
            },
            ParamLabel {
                name: "Mid threshold",
                unit: "dB",
                group: "Mid",
            },
            ParamLabel {
                name: "Mid amount",
                unit: "%",
                group: "Mid",
            },
            ParamLabel {
                name: "Mid heat",
                unit: "%",
                group: "Mid",
            },
            ParamLabel {
                name: "Mid trim",
                unit: "dB",
                group: "Mid",
            },
            ParamLabel {
                name: "High threshold",
                unit: "dB",
                group: "High",
            },
            ParamLabel {
                name: "High amount",
                unit: "%",
                group: "High",
            },
            ParamLabel {
                name: "High heat",
                unit: "%",
                group: "High",
            },
            ParamLabel {
                name: "High trim",
                unit: "dB",
                group: "High",
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
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Glue",
            },
            ParamLabel {
                name: "Attack",
                unit: "ms",
                group: "Glue",
            },
            ParamLabel {
                name: "Release",
                unit: "s",
                group: "Glue",
            },
            ParamLabel {
                name: "Makeup",
                unit: "dB",
                group: "Glue",
            },
            ParamLabel {
                name: "Dry/Wet",
                unit: "%",
                group: "Glue",
            },
            ParamLabel {
                name: "Range",
                unit: "dB",
                group: "Glue",
            },
            ParamLabel {
                name: "Clip",
                unit: "",
                group: "Glue",
            },
            ParamLabel {
                name: "SC HP",
                unit: "Hz",
                group: "Glue",
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
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 1",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 1",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 1",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 1",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 2",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 2",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 2",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 2",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 2",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 3",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 3",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 3",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 3",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 3",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 4",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 4",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 4",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 4",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 4",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 5",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 5",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 5",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 5",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 5",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 6",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 6",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 6",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 6",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 6",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 7",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 7",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 7",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 7",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 7",
            },
            ParamLabel {
                name: "On",
                unit: "",
                group: "EQ band 8",
            },
            ParamLabel {
                name: "Type",
                unit: "",
                group: "EQ band 8",
            },
            ParamLabel {
                name: "Freq",
                unit: "Hz",
                group: "EQ band 8",
            },
            ParamLabel {
                name: "Gain",
                unit: "dB",
                group: "EQ band 8",
            },
            ParamLabel {
                name: "Q",
                unit: "",
                group: "EQ band 8",
            },
            ParamLabel {
                name: "Out",
                unit: "dB",
                group: "EQ",
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
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Saturator",
            },
            ParamLabel {
                name: "Bias",
                unit: "",
                group: "Saturator",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Saturator",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Saturator",
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
            },
            ParamLabel {
                name: "Bits",
                unit: "",
                group: "Lo-fi",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Lo-fi",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Lo-fi",
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
            },
            ParamLabel {
                name: "Body",
                unit: "",
                group: "Flint",
            },
            ParamLabel {
                name: "Split",
                unit: "",
                group: "Flint",
            },
            ParamLabel {
                name: "Colour",
                unit: "",
                group: "Flint",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Flint",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Flint",
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
            },
            ParamLabel {
                name: "Formant",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Voice A",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Voice B",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Key",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Scale",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Blend",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sibyl",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Sibyl",
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
            },
            ParamLabel {
                name: "Division",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Pattern",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Groove",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Wow",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Age",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Ferric",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Ferric",
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
            },
            ParamLabel {
                name: "Time",
                unit: "",
                group: "Umbra",
            },
            ParamLabel {
                name: "Tone",
                unit: "",
                group: "Umbra",
            },
            ParamLabel {
                name: "Duck",
                unit: "",
                group: "Umbra",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Umbra",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Umbra",
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
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Tone",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Tone",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Tone",
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
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Sigil",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sigil",
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
            },
            ParamLabel {
                name: "Hold",
                unit: "",
                group: "Gauge",
            },
            ParamLabel {
                name: "Range",
                unit: "",
                group: "Gauge",
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
            },
            ParamLabel {
                name: "Strike",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Place",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Body",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Tone",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Spread",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Tune",
                unit: "",
                group: "Tine",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Tine",
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
            },
            ParamLabel {
                name: "Edge",
                unit: "",
                group: "Sheen",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Sheen",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Sheen",
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
            },
            ParamLabel {
                name: "Freq",
                unit: "",
                group: "Disperser",
            },
            ParamLabel {
                name: "Pinch",
                unit: "",
                group: "Disperser",
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
            },
            ParamLabel {
                name: "Pivot",
                unit: "",
                group: "Tilt",
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
            },
            ParamLabel {
                name: "Centre",
                unit: "",
                group: "Phaser",
            },
            ParamLabel {
                name: "Depth",
                unit: "",
                group: "Phaser",
            },
            ParamLabel {
                name: "Rate",
                unit: "",
                group: "Phaser",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Phaser",
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
            },
            ParamLabel {
                name: "Ratio",
                unit: "",
                group: "Gate",
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Gate",
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Gate",
            },
            ParamLabel {
                name: "Range",
                unit: "",
                group: "Gate",
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
            },
            ParamLabel {
                name: "High",
                unit: "",
                group: "Strip",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Strip",
            },
            ParamLabel {
                name: "Warm",
                unit: "",
                group: "Strip",
            },
            ParamLabel {
                name: "Out",
                unit: "",
                group: "Strip",
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
            },
            ParamLabel {
                name: "Shift",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Attack",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Release",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Warm",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Mix",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 1",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 2",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 3",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 4",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 5",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 6",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 7",
                unit: "",
                group: "Resyn",
            },
            ParamLabel {
                name: "Band 8",
                unit: "",
                group: "Resyn",
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
            },
            ParamLabel {
                name: "Tune",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Cutoff",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Resonance",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Env mod",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Decay",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Accent",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Glide",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Drive",
                unit: "",
                group: "Acid",
            },
            ParamLabel {
                name: "Level",
                unit: "",
                group: "Acid",
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
