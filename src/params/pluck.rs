//! PLUCK parameter and pages contract.
use super::ParamDef;
use crate::devices::ParamLabel;
use crate::pages::{KeyTable, MachineKey, SubPage};
pub const TUNE: u32 = 0;
pub const DETUNE: u32 = 1;
pub const RING: u32 = 2;
pub const BRIGHT: u32 = 3;
pub const PICK: u32 = 4;
pub const STRIKE: u32 = 5;
pub const STIFF: u32 = 6;
pub const STRETCH: u32 = 7;
pub const HAMMER: u32 = 8;
pub const PICKUP: u32 = 9;
pub const BARK: u32 = 10;
pub const OVERTONE: u32 = 11;
pub const TONEBAR: u32 = 12;
pub const TONE_DECAY: u32 = 13;
pub const DAMP: u32 = 14;
pub const CHOKE: u32 = 15;
pub const ATTACK: u32 = 16;
pub const DECAY: u32 = 17;
pub const SUSTAIN: u32 = 18;
pub const RELEASE: u32 = 19;
pub const VELOCITY: u32 = 20;
pub const LEVEL: u32 = 21;
pub const SPREAD: u32 = 22;
pub const VOICES: u32 = 23;
pub const BODY_SIZE: u32 = 24;
pub const BODY_DECAY: u32 = 25;
pub const BODY_DAMP: u32 = 26;
pub const BODY_MIX: u32 = 27;
pub const DIFFUSE: u32 = 28;
pub const MOTION: u32 = 29;
pub const SHINE: u32 = 30;
pub const SHINE_CORNER: u32 = 31;
pub const SHINE_MIX: u32 = 32;
pub const SHINE_ATTACK: u32 = 33;
pub const SHINE_RELEASE: u32 = 34;
pub const SHINE_TILT: u32 = 35;
pub const GAIN: u32 = LEVEL;
pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: 0,
        name: "tune",
        min: -36.0,
        max: 36.0,
        default: 0.0,
    },
    ParamDef {
        id: 1,
        name: "detune",
        min: 0.0,
        max: 40.0,
        default: 5.0,
    },
    ParamDef {
        id: 2,
        name: "ring",
        min: 0.025,
        max: 12.0,
        default: 2.0,
    },
    ParamDef {
        id: 3,
        name: "bright",
        min: 0.0,
        max: 1.0,
        default: 0.65,
    },
    ParamDef {
        id: 4,
        name: "pick",
        min: 0.015,
        max: 0.5,
        default: 0.2,
    },
    ParamDef {
        id: 5,
        name: "strike",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: 6,
        name: "stiff",
        min: 0.0,
        max: 1.0,
        default: 0.15,
    },
    ParamDef {
        id: 7,
        name: "stretch",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: 8,
        name: "hammer",
        min: 0.0,
        max: 1.0,
        default: 0.35,
    },
    ParamDef {
        id: 9,
        name: "pickup",
        min: 0.0,
        max: 1.0,
        default: 0.25,
    },
    ParamDef {
        id: 10,
        name: "bark",
        min: 0.0,
        max: 1.0,
        default: 0.55,
    },
    ParamDef {
        id: 11,
        name: "overtone",
        min: 1.0,
        max: 8.0,
        default: 2.0,
    },
    ParamDef {
        id: 12,
        name: "tonebar",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 13,
        name: "tone_decay",
        min: 0.1,
        max: 12.0,
        default: 3.0,
    },
    ParamDef {
        id: 14,
        name: "damp",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: 15,
        name: "choke",
        min: 0.0,
        max: 1.0,
        default: 0.4,
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
        name: "body_size",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: 25,
        name: "body_decay",
        min: 0.15,
        max: 4.0,
        default: 0.5,
    },
    ParamDef {
        id: 26,
        name: "body_damp",
        min: 300.0,
        max: 16000.0,
        default: 5000.0,
    },
    ParamDef {
        id: 27,
        name: "body_mix",
        min: 0.0,
        max: 1.0,
        default: 0.18,
    },
    ParamDef {
        id: 28,
        name: "diffuse",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: 29,
        name: "motion",
        min: 0.0,
        max: 1.0,
        default: 0.1,
    },
    ParamDef {
        id: 30,
        name: "shine",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 31,
        name: "shine_corner",
        min: 1000.0,
        max: 16000.0,
        default: 5000.0,
    },
    ParamDef {
        id: 32,
        name: "shine_mix",
        min: 0.0,
        max: 1.0,
        default: 0.35,
    },
    ParamDef {
        id: 33,
        name: "shine_attack",
        min: 0.1,
        max: 50.0,
        default: 2.0,
    },
    ParamDef {
        id: 34,
        name: "shine_release",
        min: 5.0,
        max: 500.0,
        default: 70.0,
    },
    ParamDef {
        id: 35,
        name: "shine_tilt",
        min: -12.0,
        max: 12.0,
        default: 0.0,
    },
];
pub const LABELS: &[ParamLabel] = &[
    ParamLabel {
        name: "Tune",
        unit: " st",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Detune",
        unit: " ct",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Ring",
        unit: " s",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Bright",
        unit: "",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Pick",
        unit: "",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Strike",
        unit: "",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Stiff",
        unit: "",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Stretch",
        unit: "",
        group: "String",
        choices: &[],
    },
    ParamLabel {
        name: "Hammer",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Pickup",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Bark",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Overtone",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Tonebar",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Tone Decay",
        unit: " s",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: "",
        group: "Tine",
        choices: &[],
    },
    ParamLabel {
        name: "Choke",
        unit: "",
        group: "Tine",
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
        name: "Body Size",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Body Decay",
        unit: " s",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Body Damp",
        unit: " Hz",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Body Mix",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Diffuse",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Motion",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Shine",
        unit: "",
        group: "Shine",
        choices: &[],
    },
    ParamLabel {
        name: "Shine Corne",
        unit: " Hz",
        group: "Shine",
        choices: &[],
    },
    ParamLabel {
        name: "Shine Mix",
        unit: "",
        group: "Shine",
        choices: &[],
    },
    ParamLabel {
        name: "Shine Attac",
        unit: " ms",
        group: "Shine",
        choices: &[],
    },
    ParamLabel {
        name: "Shine Relea",
        unit: " ms",
        group: "Shine",
        choices: &[],
    },
    ParamLabel {
        name: "Shine Tilt",
        unit: " dB",
        group: "Shine",
        choices: &[],
    },
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "String",
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
                title: "Tine",
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
                title: "Body",
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
                title: "Shine",
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
pub const DISCRETE: &[u32] = &[23];
pub const LOG: &[u32] = &[2, 13, 17, 19, 25, 26, 31, 33, 34];
pub const WALK_X: u32 = 6;
pub const WALK_Y: u32 = 3;
pub const WALK_T: u32 = 2;
pub const DEMOS: &[(&str, &[(u32, f32)])] = &[
    (
        "nylon wire",
        &[
            (STIFF, 0.0),
            (PICKUP, 0.0),
            (HAMMER, 0.0),
            (BRIGHT, 0.65),
            (PICK, 0.23),
        ],
    ),
    (
        "suitcase tine",
        &[
            (STIFF, 0.62),
            (HAMMER, 0.8),
            (PICKUP, 0.7),
            (BARK, 0.85),
            (TONEBAR, 0.55),
            (TONE_DECAY, 5.0),
        ],
    ),
    (
        "tiny steel",
        &[
            (STIFF, 1.0),
            (RING, 0.25),
            (BRIGHT, 0.95),
            (PICK, 0.06),
            (BODY_SIZE, 0.0),
        ],
    ),
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Echo,
    crate::console::SectionKind::Room,
];
