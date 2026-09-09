//! VOX parameter and pages contract.
use super::ParamDef;
use crate::devices::ParamLabel;
use crate::pages::{KeyTable, MachineKey, SubPage};
pub const VOWEL: u32 = 0;
pub const SEX: u32 = 1;
pub const THROAT: u32 = 2;
pub const BREATH: u32 = 3;
pub const PW: u32 = 4;
pub const VIBRATO: u32 = 5;
pub const TUNE: u32 = 6;
pub const BITE: u32 = 7;
pub const BEND: u32 = 8;
pub const BEND_TIME: u32 = 9;
pub const VIB_RATE: u32 = 10;
pub const VIB_DELAY: u32 = 11;
pub const TRACK: u32 = 12;
pub const TONE: u32 = 13;
pub const FORMANT: u32 = 14;
pub const AIR: u32 = 15;
pub const ATTACK: u32 = 16;
pub const DECAY: u32 = 17;
pub const SUSTAIN: u32 = 18;
pub const RELEASE: u32 = 19;
pub const VELOCITY: u32 = 20;
pub const LEVEL: u32 = 21;
pub const SPREAD: u32 = 22;
pub const VOICES: u32 = 23;
pub const SENSE: u32 = 24;
pub const RANGE: u32 = 25;
pub const TALK_ATTACK: u32 = 26;
pub const TALK_RELEASE: u32 = 27;
pub const TALK_MIX: u32 = 28;
pub const TALK_BIAS: u32 = 29;
pub const CHOIR_VOICES: u32 = 30;
pub const CHOIR_DETUNE: u32 = 31;
pub const CHOIR_RATE: u32 = 32;
pub const CHOIR_MIX: u32 = 33;
pub const CHOIR_WIDTH: u32 = 34;
pub const CHOIR_DELAY: u32 = 35;
pub const GAIN: u32 = LEVEL;
pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: 0,
        name: "vowel",
        min: 0.0,
        max: 4.0,
        default: 1.0,
    },
    ParamDef {
        id: 1,
        name: "sex",
        min: 0.0,
        max: 1.0,
        default: 0.45,
    },
    ParamDef {
        id: 2,
        name: "throat",
        min: 0.0,
        max: 1.0,
        default: 0.45,
    },
    ParamDef {
        id: 3,
        name: "breath",
        min: 0.0,
        max: 1.0,
        default: 0.12,
    },
    ParamDef {
        id: 4,
        name: "pw",
        min: 0.05,
        max: 0.95,
        default: 0.4,
    },
    ParamDef {
        id: 5,
        name: "vibrato",
        min: 0.0,
        max: 1.0,
        default: 0.15,
    },
    ParamDef {
        id: 6,
        name: "tune",
        min: -36.0,
        max: 36.0,
        default: 0.0,
    },
    ParamDef {
        id: 7,
        name: "bite",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 8,
        name: "bend",
        min: -24.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: 9,
        name: "bend_time",
        min: 5.0,
        max: 2000.0,
        default: 120.0,
    },
    ParamDef {
        id: 10,
        name: "vib_rate",
        min: 0.1,
        max: 12.0,
        default: 5.6,
    },
    ParamDef {
        id: 11,
        name: "vib_delay",
        min: 0.0,
        max: 2000.0,
        default: 300.0,
    },
    ParamDef {
        id: 12,
        name: "track",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: 13,
        name: "tone",
        min: -24.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: 14,
        name: "formant",
        min: 0.0,
        max: 1.0,
        default: 0.9,
    },
    ParamDef {
        id: 15,
        name: "air",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 16,
        name: "attack",
        min: 0.0,
        max: 2000.0,
        default: 30.0,
    },
    ParamDef {
        id: 17,
        name: "decay",
        min: 5.0,
        max: 10000.0,
        default: 900.0,
    },
    ParamDef {
        id: 18,
        name: "sustain",
        min: 0.0,
        max: 1.0,
        default: 0.75,
    },
    ParamDef {
        id: 19,
        name: "release",
        min: 5.0,
        max: 10000.0,
        default: 350.0,
    },
    ParamDef {
        id: 20,
        name: "velocity",
        min: 0.0,
        max: 1.0,
        default: 0.8,
    },
    ParamDef {
        id: 21,
        name: "level",
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    ParamDef {
        id: 22,
        name: "spread",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 23,
        name: "voices",
        min: 1.0,
        max: 16.0,
        default: 16.0,
    },
    ParamDef {
        id: 24,
        name: "sense",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 25,
        name: "range",
        min: -2.0,
        max: 2.0,
        default: 0.8,
    },
    ParamDef {
        id: 26,
        name: "talk_attack",
        min: 1.0,
        max: 500.0,
        default: 25.0,
    },
    ParamDef {
        id: 27,
        name: "talk_release",
        min: 5.0,
        max: 2000.0,
        default: 180.0,
    },
    ParamDef {
        id: 28,
        name: "talk_mix",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: 29,
        name: "talk_bias",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: 30,
        name: "choir_voices",
        min: 1.0,
        max: 3.0,
        default: 3.0,
    },
    ParamDef {
        id: 31,
        name: "choir_detune",
        min: 0.0,
        max: 30.0,
        default: 9.0,
    },
    ParamDef {
        id: 32,
        name: "choir_rate",
        min: 0.05,
        max: 5.0,
        default: 0.35,
    },
    ParamDef {
        id: 33,
        name: "choir_mix",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 34,
        name: "choir_width",
        min: 0.0,
        max: 1.0,
        default: 0.8,
    },
    ParamDef {
        id: 35,
        name: "choir_delay",
        min: 5.0,
        max: 50.0,
        default: 18.0,
    },
];
pub const LABELS: &[ParamLabel] = &[
    ParamLabel {
        name: "Vowel",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Sex",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Throat",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Breath",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Pw",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Vibrato",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Tune",
        unit: " st",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Bite",
        unit: "",
        group: "Voice",
        choices: &[],
    },
    ParamLabel {
        name: "Bend",
        unit: " st",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Bend Time",
        unit: " ms",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Vib Rate",
        unit: " Hz",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Vib Delay",
        unit: " ms",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Track",
        unit: "",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Tone",
        unit: " dB",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Formant",
        unit: "",
        group: "Articulate",
        choices: &[],
    },
    ParamLabel {
        name: "Air",
        unit: "",
        group: "Articulate",
        choices: &[],
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
        unit: "",
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
        name: "Velocity",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    ParamLabel {
        name: "Level",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    ParamLabel {
        name: "Spread",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    ParamLabel {
        name: "Voices",
        unit: "",
        group: "Amp",
        choices: &[
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        ],
    },
    ParamLabel {
        name: "Sense",
        unit: "",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Range",
        unit: "",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Talk Attack",
        unit: " ms",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Talk Releas",
        unit: " ms",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Talk Mix",
        unit: "",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Talk Bias",
        unit: "",
        group: "Talk",
        choices: &[],
    },
    ParamLabel {
        name: "Choir Voice",
        unit: "",
        group: "Choir",
        choices: &["1", "2", "3"],
    },
    ParamLabel {
        name: "Choir Detun",
        unit: " ct",
        group: "Choir",
        choices: &[],
    },
    ParamLabel {
        name: "Choir Rate",
        unit: " Hz",
        group: "Choir",
        choices: &[],
    },
    ParamLabel {
        name: "Choir Mix",
        unit: "",
        group: "Choir",
        choices: &[],
    },
    ParamLabel {
        name: "Choir Width",
        unit: "",
        group: "Choir",
        choices: &[],
    },
    ParamLabel {
        name: "Choir Delay",
        unit: " ms",
        group: "Choir",
        choices: &[],
    },
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Voice",
                slots: [
                    Some(0),
                    Some(1),
                    Some(2),
                    Some(3),
                    Some(4),
                    Some(5),
                    Some(6),
                    Some(7),
                ],
            },
            SubPage {
                title: "Articulate",
                slots: [
                    Some(8),
                    Some(9),
                    Some(10),
                    Some(11),
                    Some(12),
                    Some(13),
                    Some(14),
                    Some(15),
                ],
            },
        ],
    }),
    None,
    Some(MachineKey {
        word: "AMP",
        subpages: &[SubPage {
            title: "Amp",
            slots: [
                Some(16),
                Some(17),
                Some(18),
                Some(19),
                Some(20),
                Some(21),
                Some(22),
                Some(23),
            ],
        }],
    }),
    None,
    Some(MachineKey {
        word: "FX",
        subpages: &[
            SubPage {
                title: "Talk",
                slots: [
                    Some(24),
                    Some(25),
                    Some(26),
                    Some(27),
                    Some(28),
                    Some(29),
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Choir",
                slots: [
                    Some(30),
                    Some(31),
                    Some(32),
                    Some(33),
                    Some(34),
                    Some(35),
                    None,
                    None,
                ],
            },
        ],
    }),
    None,
    None,
];
pub const DISCRETE: &[u32] = &[23, 30];
pub const LOG: &[u32] = &[9, 10, 17, 19, 26, 27, 32, 35];
pub const WALK_X: u32 = 3;
pub const WALK_Y: u32 = 1;
pub const WALK_T: u32 = 17;
pub const DEMOS: &[(&str, &[(u32, f32)])] = &[
    (
        "low reed",
        &[
            (VOWEL, 4.0),
            (SEX, 0.1),
            (BITE, 0.55),
            (BREATH, 0.04),
            (PW, 0.5),
        ],
    ),
    (
        "brass mouth",
        &[
            (VOWEL, 0.0),
            (BITE, 0.9),
            (ATTACK, 8.0),
            (BEND, -2.0),
            (BEND_TIME, 40.0),
            (SENSE, 0.8),
        ],
    ),
    (
        "breath choir",
        &[
            (BREATH, 0.8),
            (ATTACK, 350.0),
            (RELEASE, 1600.0),
            (CHOIR_MIX, 0.65),
            (CHOIR_DETUNE, 18.0),
        ],
    ),
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Echo,
    crate::console::SectionKind::Room,
];
