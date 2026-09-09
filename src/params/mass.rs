//! MASS parameter and pages contract.
use super::ParamDef;
use crate::devices::ParamLabel;
use crate::pages::{KeyTable, MachineKey, SubPage};
pub const TUNE: u32 = 0;
pub const LAYER: u32 = 1;
pub const OCTAVE: u32 = 2;
pub const BLEND: u32 = 3;
pub const PHASE: u32 = 4;
pub const DROP: u32 = 5;
pub const DROP_TIME: u32 = 6;
pub const GLIDE: u32 = 7;
pub const CUTOFF: u32 = 8;
pub const RESO: u32 = 9;
pub const ENV: u32 = 10;
pub const F_DECAY: u32 = 11;
pub const TRACK: u32 = 12;
pub const KEY_LOW: u32 = 13;
pub const SUB_FLOOR: u32 = 14;
pub const LEGATO: u32 = 15;
pub const ATTACK: u32 = 16;
pub const DECAY: u32 = 17;
pub const SUSTAIN: u32 = 18;
pub const RELEASE: u32 = 19;
pub const VELOCITY: u32 = 20;
pub const LEVEL: u32 = 21;
pub const SPREAD: u32 = 22;
pub const VOICES: u32 = 23;
pub const HARMONIC: u32 = 24;
pub const TONE: u32 = 25;
pub const CORNER: u32 = 26;
pub const BIAS: u32 = 27;
pub const WEIGHT_MIX: u32 = 28;
pub const SKEW: u32 = 29;
pub const CEILING: u32 = 30;
pub const CLAMP_RELEASE: u32 = 31;
pub const GLUE: u32 = 32;
pub const THRESHOLD: u32 = 33;
pub const CLAMP_ATTACK: u32 = 34;
pub const CLAMP_MIX: u32 = 35;
pub const GAIN: u32 = LEVEL;
pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: 0,
        name: "tune",
        min: -36.0,
        max: 36.0,
        default: -12.0,
    },
    ParamDef {
        id: 1,
        name: "layer",
        min: 0.0,
        max: 3.0,
        default: 2.0,
    },
    ParamDef {
        id: 2,
        name: "octave",
        min: 0.0,
        max: 2.0,
        default: 0.0,
    },
    ParamDef {
        id: 3,
        name: "blend",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 4,
        name: "phase",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: 5,
        name: "drop",
        min: 0.0,
        max: 36.0,
        default: 0.0,
    },
    ParamDef {
        id: 6,
        name: "drop_time",
        min: 5.0,
        max: 500.0,
        default: 60.0,
    },
    ParamDef {
        id: 7,
        name: "glide",
        min: 0.0,
        max: 500.0,
        default: 40.0,
    },
    ParamDef {
        id: 8,
        name: "cutoff",
        min: 20.0,
        max: 16000.0,
        default: 420.0,
    },
    ParamDef {
        id: 9,
        name: "reso",
        min: 0.0,
        max: 1.0,
        default: 0.15,
    },
    ParamDef {
        id: 10,
        name: "env",
        min: -48.0,
        max: 48.0,
        default: 8.0,
    },
    ParamDef {
        id: 11,
        name: "f_decay",
        min: 5.0,
        max: 4000.0,
        default: 240.0,
    },
    ParamDef {
        id: 12,
        name: "track",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: 13,
        name: "key_low",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: 14,
        name: "sub_floor",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 15,
        name: "legato",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: 16,
        name: "attack",
        min: 0.0,
        max: 2000.0,
        default: 2.0,
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
        default: 0.85,
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
        default: 1.0,
    },
    ParamDef {
        id: 24,
        name: "harmonic",
        min: 0.0,
        max: 1.0,
        default: 0.25,
    },
    ParamDef {
        id: 25,
        name: "tone",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: 26,
        name: "corner",
        min: 40.0,
        max: 400.0,
        default: 150.0,
    },
    ParamDef {
        id: 27,
        name: "bias",
        min: -1.0,
        max: 1.0,
        default: 0.1,
    },
    ParamDef {
        id: 28,
        name: "weight_mix",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: 29,
        name: "skew",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: 30,
        name: "ceiling",
        min: -18.0,
        max: 0.0,
        default: -1.0,
    },
    ParamDef {
        id: 31,
        name: "clamp_release",
        min: 20.0,
        max: 1000.0,
        default: 120.0,
    },
    ParamDef {
        id: 32,
        name: "glue",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 33,
        name: "threshold",
        min: -36.0,
        max: -3.0,
        default: -18.0,
    },
    ParamDef {
        id: 34,
        name: "clamp_attack",
        min: 0.1,
        max: 100.0,
        default: 8.0,
    },
    ParamDef {
        id: 35,
        name: "clamp_mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
];
pub const LABELS: &[ParamLabel] = &[
    ParamLabel {
        name: "Tune",
        unit: " st",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Layer",
        unit: "",
        group: "Sub",
        choices: &["OFF", "SQUARE", "SAW", "TRI"],
    },
    ParamLabel {
        name: "Octave",
        unit: "",
        group: "Sub",
        choices: &["0", "+1", "+2"],
    },
    ParamLabel {
        name: "Blend",
        unit: "",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Phase",
        unit: "",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Drop",
        unit: " st",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Drop Time",
        unit: " ms",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Glide",
        unit: " ms",
        group: "Sub",
        choices: &[],
    },
    ParamLabel {
        name: "Cutoff",
        unit: " Hz",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "Reso",
        unit: "",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "Env",
        unit: " st",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "F Decay",
        unit: " ms",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "Track",
        unit: "",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "Key Low",
        unit: "",
        group: "Ladder",
        choices: &["OFF", "ON"],
    },
    ParamLabel {
        name: "Sub Floor",
        unit: "",
        group: "Ladder",
        choices: &[],
    },
    ParamLabel {
        name: "Legato",
        unit: "",
        group: "Ladder",
        choices: &["OFF", "ON"],
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
        name: "Harmonic",
        unit: "",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Tone",
        unit: "",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Corner",
        unit: " Hz",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Bias",
        unit: "",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Weight Mix",
        unit: "",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Skew",
        unit: "",
        group: "Weight",
        choices: &[],
    },
    ParamLabel {
        name: "Ceiling",
        unit: " dB",
        group: "Clamp",
        choices: &[],
    },
    ParamLabel {
        name: "Clamp Relea",
        unit: " ms",
        group: "Clamp",
        choices: &[],
    },
    ParamLabel {
        name: "Glue",
        unit: "",
        group: "Clamp",
        choices: &[],
    },
    ParamLabel {
        name: "Threshold",
        unit: " dB",
        group: "Clamp",
        choices: &[],
    },
    ParamLabel {
        name: "Clamp Attac",
        unit: " ms",
        group: "Clamp",
        choices: &[],
    },
    ParamLabel {
        name: "Clamp Mix",
        unit: "",
        group: "Clamp",
        choices: &[],
    },
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[SubPage {
            title: "Sub",
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
        }],
    }),
    Some(MachineKey {
        word: "FLTR",
        subpages: &[SubPage {
            title: "Ladder",
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
        }],
    }),
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
                title: "Weight",
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
                title: "Clamp",
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
pub const DISCRETE: &[u32] = &[1, 2, 13, 15, 23];
pub const LOG: &[u32] = &[6, 8, 11, 17, 19, 26, 31, 34];
pub const WALK_X: u32 = 29;
pub const WALK_Y: u32 = 8;
pub const WALK_T: u32 = 17;
pub const DEMOS: &[(&str, &[(u32, f32)])] = &[
    (
        "pure sub",
        &[
            (LAYER, 0.0),
            (DROP, 0.0),
            (CUTOFF, 1200.0),
            (HARMONIC, 0.0),
            (SUB_FLOOR, 0.0),
        ],
    ),
    (
        "warehouse punch",
        &[
            (DROP, 24.0),
            (DROP_TIME, 30.0),
            (SUSTAIN, 0.0),
            (DECAY, 220.0),
            (HARMONIC, 0.8),
            (BLEND, 0.45),
        ],
    ),
    (
        "crooked shoulders",
        &[
            (SKEW, 0.8),
            (CUTOFF, 1800.0),
            (BLEND, 0.5),
            (WEIGHT_MIX, 0.7),
        ],
    ),
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Drive,
    crate::console::SectionKind::Pump,
];
