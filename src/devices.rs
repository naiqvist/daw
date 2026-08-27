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

/// A device a track can hold: `SineSynth` makes sound, `Reverb` shapes it.
///
/// What a device IS. Every variant is a real node with a real parameter
/// table; the browser's Instruments and Audio Effects folders are how one
/// reaches a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DeviceKind {
    SineSynth,
    Poly,
    Sampler,
    Kick,
    Snare,
    Tom,
    Hat,
    Handclap,
    Reverb,
    Sat,
    Lofi,
    Echo,
    Eq,
    Filter,
    Glue,
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
/// `params` is `&'static` and stays that way: `daw::params::clamp` scans
/// exactly this slice inside the audio callback, so anything heap-backed
/// reaching it would be a red-zone allocation. Descriptive on this side,
/// static on the engine's.
pub struct DeviceSpec {
    pub kind: DeviceKind,
    pub name: &'static str,
    /// Instruments MAKE sound and head the chain; effects SHAPE it.
    pub instrument: bool,
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
        kind: DeviceKind::Poly,
        name: "poly synth",
        instrument: true,
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
        kind: DeviceKind::Glue,
        name: "glue",
        instrument: false,
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
];

/// The device a target's prefix names, if any. The parse side of
/// [`DeviceSpec::prefix`].
pub fn device_by_prefix(prefix: &str) -> Option<&'static DeviceSpec> {
    DEVICES.iter().find(|spec| spec.prefix == prefix)
}
