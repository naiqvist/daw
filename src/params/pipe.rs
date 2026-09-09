//! PIPE parameter and pages contract.
use super::ParamDef;
use crate::devices::ParamLabel;
use crate::pages::{KeyTable, MachineKey, SubPage};
pub const BAR_16: u32 = 0;
pub const BAR_5_3: u32 = 1;
pub const BAR_8: u32 = 2;
pub const BAR_4: u32 = 3;
pub const REGISTER: u32 = 4;
pub const LEAK: u32 = 5;
pub const TUNE: u32 = 6;
pub const CLICK: u32 = 7;
pub const BAR_2_3: u32 = 8;
pub const BAR_2: u32 = 9;
pub const BAR_1_3: u32 = 10;
pub const BAR_1_1: u32 = 11;
pub const BAR_1: u32 = 12;
pub const PERC: u32 = 13;
pub const P_DECAY: u32 = 14;
pub const SCANNER: u32 = 15;
pub const ATTACK: u32 = 16;
pub const DECAY: u32 = 17;
pub const SUSTAIN: u32 = 18;
pub const RELEASE: u32 = 19;
pub const VELOCITY: u32 = 20;
pub const LEVEL: u32 = 21;
pub const SPREAD: u32 = 22;
pub const VOICES: u32 = 23;
pub const DRIVE: u32 = 24;
pub const TUBE_BIAS: u32 = 25;
pub const TUBE_TONE: u32 = 26;
pub const TUBE_MIX: u32 = 27;
pub const TUBE_CORNER: u32 = 28;
pub const SAG: u32 = 29;
pub const SPEED: u32 = 30;
pub const ACCEL: u32 = 31;
pub const HORN: u32 = 32;
pub const DRUM: u32 = 33;
pub const ROTARY_MIX: u32 = 34;
pub const DISTANCE: u32 = 35;
pub const GAIN: u32 = LEVEL;
pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: 0,
        name: "bar_16",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 1,
        name: "bar_5_3",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 2,
        name: "bar_8",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 3,
        name: "bar_4",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 4,
        name: "register",
        min: 0.0,
        max: 4.0,
        default: 1.4,
    },
    ParamDef {
        id: 5,
        name: "leak",
        min: 0.0,
        max: 1.0,
        default: 0.08,
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
        name: "click",
        min: 0.0,
        max: 1.0,
        default: 0.18,
    },
    ParamDef {
        id: 8,
        name: "bar_2_3",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 9,
        name: "bar_2",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 10,
        name: "bar_1_3",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 11,
        name: "bar_1_1",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 12,
        name: "bar_1",
        min: -8.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: 13,
        name: "perc",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: 14,
        name: "p_decay",
        min: 20.0,
        max: 1500.0,
        default: 220.0,
    },
    ParamDef {
        id: 15,
        name: "scanner",
        min: 0.0,
        max: 1.0,
        default: 0.15,
    },
    ParamDef {
        id: 16,
        name: "attack",
        min: 0.0,
        max: 2000.0,
        default: 3.0,
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
        default: 1.0,
    },
    ParamDef {
        id: 19,
        name: "release",
        min: 5.0,
        max: 10000.0,
        default: 90.0,
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
        name: "drive",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 25,
        name: "tube_bias",
        min: -1.0,
        max: 1.0,
        default: 0.1,
    },
    ParamDef {
        id: 26,
        name: "tube_tone",
        min: -12.0,
        max: 12.0,
        default: 0.0,
    },
    ParamDef {
        id: 27,
        name: "tube_mix",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: 28,
        name: "tube_corner",
        min: 100.0,
        max: 8000.0,
        default: 900.0,
    },
    ParamDef {
        id: 29,
        name: "sag",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: 30,
        name: "speed",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: 31,
        name: "accel",
        min: 200.0,
        max: 5000.0,
        default: 1800.0,
    },
    ParamDef {
        id: 32,
        name: "horn",
        min: 0.0,
        max: 1.0,
        default: 0.6,
    },
    ParamDef {
        id: 33,
        name: "drum",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: 34,
        name: "rotary_mix",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: 35,
        name: "distance",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
];
pub const LABELS: &[ParamLabel] = &[
    ParamLabel {
        name: "16 ft",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "5 1/3 ft",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "8 ft",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "4 ft",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "Register",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "Leak",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "Tune",
        unit: " st",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "Click",
        unit: "",
        group: "Low",
        choices: &[],
    },
    ParamLabel {
        name: "2 2/3 ft",
        unit: "",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "2 ft",
        unit: "",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "1 3/5 ft",
        unit: "",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "1 1/3 ft",
        unit: "",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "1 ft",
        unit: "",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "Perc",
        unit: "",
        group: "High",
        choices: &["OFF", "2ND", "3RD"],
    },
    ParamLabel {
        name: "P Decay",
        unit: " ms",
        group: "High",
        choices: &[],
    },
    ParamLabel {
        name: "Scanner",
        unit: "",
        group: "High",
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
        name: "Drive",
        unit: "",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Tube Bias",
        unit: "",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Tube Tone",
        unit: " dB",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Tube Mix",
        unit: "",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Tube Corner",
        unit: " Hz",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Sag",
        unit: "",
        group: "Tube",
        choices: &[],
    },
    ParamLabel {
        name: "Speed",
        unit: "",
        group: "Rotary",
        choices: &["STOP", "SLOW", "FAST"],
    },
    ParamLabel {
        name: "Accel",
        unit: " ms",
        group: "Rotary",
        choices: &[],
    },
    ParamLabel {
        name: "Horn",
        unit: "",
        group: "Rotary",
        choices: &[],
    },
    ParamLabel {
        name: "Drum",
        unit: "",
        group: "Rotary",
        choices: &[],
    },
    ParamLabel {
        name: "Rotary Mix",
        unit: "",
        group: "Rotary",
        choices: &[],
    },
    ParamLabel {
        name: "Distance",
        unit: "",
        group: "Rotary",
        choices: &[],
    },
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Low",
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
                title: "High",
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
                title: "Tube",
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
                title: "Rotary",
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
pub const DISCRETE: &[u32] = &[13, 23, 30];
pub const LOG: &[u32] = &[14, 17, 19, 28, 31];
pub const WALK_X: u32 = 5;
pub const WALK_Y: u32 = 4;
pub const WALK_T: u32 = 17;
pub const DEMOS: &[(&str, &[(u32, f32)])] = &[
    (
        "garage hollow",
        &[
            (REGISTER, 1.2),
            (PERC, 2.0),
            (SUSTAIN, 0.0),
            (DECAY, 320.0),
            (ROTARY_MIX, 0.15),
        ],
    ),
    (
        "bright rotor",
        &[
            (REGISTER, 3.6),
            (SPEED, 2.0),
            (ACCEL, 2500.0),
            (ROTARY_MIX, 0.8),
            (DRIVE, 0.6),
        ],
    ),
    (
        "wheel sub",
        &[
            (REGISTER, 0.0),
            (PERC, 0.0),
            (CLICK, 0.0),
            (LEAK, 0.0),
            (SCANNER, 0.0),
            (ROTARY_MIX, 0.0),
        ],
    ),
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Room,
    crate::console::SectionKind::Echo,
];
