//! Coverage instrument table: dense ids, shared labels, lockable walkers.
use super::ParamDef;
use crate::devices::ParamLabel;
use crate::pages::{KeyTable, MachineKey, SubPage};
pub const ATTACK: u32 = 0;
pub const DECAY: u32 = 1;
pub const SUSTAIN: u32 = 2;
pub const RELEASE: u32 = 3;
pub const VELOCITY: u32 = 4;
pub const LEVEL: u32 = 5;
pub const TUNE: u32 = 6;
pub const WIDTH: u32 = 7;
pub const CUTOFF: u32 = 8;
pub const RESONANCE: u32 = 9;
pub const FILTER_ENV: u32 = 10;
pub const FILTER_DECAY: u32 = 11;
pub const KEYTRACK: u32 = 12;
pub const MATERIAL: u32 = 13;
pub const INHARM: u32 = 14;
pub const DAMP: u32 = 15;
pub const STRIKE: u32 = 16;
pub const HARD: u32 = 17;
pub const POSITION: u32 = 18;
pub const SPREAD: u32 = 19;
pub const FEED: u32 = 20;
pub const PARTIAL1: u32 = 21;
pub const PARTIAL2: u32 = 22;
pub const PARTIAL3: u32 = 23;
pub const PARTIAL4: u32 = 24;
pub const PARTIAL5: u32 = 25;
pub const PARTIAL6: u32 = 26;
pub const CHOKE: u32 = 27;
pub const CONTACT: u32 = 28;
pub const DISPERSE: u32 = 29;
pub const DISPERSE_FOCUS: u32 = 30;
pub const DISPERSE_WIDTH: u32 = 31;
pub const DISPERSE_FOLLOW: u32 = 32;
pub const BLOOM_SIZE: u32 = 33;
pub const BLOOM_DECAY: u32 = 34;
pub const BLOOM_DAMP: u32 = 35;
pub const BLOOM_MIX: u32 = 36;
pub const GAIN: u32 = LEVEL;
pub const WALK_X: u32 = INHARM;
pub const WALK_Y: u32 = CUTOFF;
pub const WALK_T: u32 = DECAY;
pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: ATTACK,
        name: "attack",
        min: 0.0,
        max: 8000.0,
        default: 3.0,
    },
    ParamDef {
        id: DECAY,
        name: "decay",
        min: 5.0,
        max: 12000.0,
        default: 3600.0,
    },
    ParamDef {
        id: SUSTAIN,
        name: "sustain",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: RELEASE,
        name: "release",
        min: 1.0,
        max: 12000.0,
        default: 700.0,
    },
    ParamDef {
        id: VELOCITY,
        name: "velocity",
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    ParamDef {
        id: LEVEL,
        name: "level",
        min: 0.0,
        max: 2.0,
        default: 0.7,
    },
    ParamDef {
        id: TUNE,
        name: "tune",
        min: -48.0,
        max: 48.0,
        default: 0.0,
    },
    ParamDef {
        id: WIDTH,
        name: "width",
        min: 0.0,
        max: 1.0,
        default: 0.4,
    },
    ParamDef {
        id: CUTOFF,
        name: "cutoff",
        min: 30.0,
        max: 20000.0,
        default: 12000.0,
    },
    ParamDef {
        id: RESONANCE,
        name: "resonance",
        min: 0.5,
        max: 8.0,
        default: 0.707,
    },
    ParamDef {
        id: FILTER_ENV,
        name: "filter_env",
        min: -60.0,
        max: 60.0,
        default: 0.0,
    },
    ParamDef {
        id: FILTER_DECAY,
        name: "filter_decay",
        min: 5.0,
        max: 8000.0,
        default: 600.0,
    },
    ParamDef {
        id: KEYTRACK,
        name: "keytrack",
        min: 0.0,
        max: 2.0,
        default: 0.0,
    },
    ParamDef {
        id: MATERIAL,
        name: "material",
        min: 0.0,
        max: 4.0,
        default: 2.3,
    },
    ParamDef {
        id: INHARM,
        name: "inharm",
        min: 0.0,
        max: 1.0,
        default: 0.75,
    },
    ParamDef {
        id: DAMP,
        name: "damp",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: STRIKE,
        name: "strike",
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: HARD,
        name: "hard",
        min: 0.0,
        max: 1.0,
        default: 0.6,
    },
    ParamDef {
        id: POSITION,
        name: "position",
        min: 0.01,
        max: 0.99,
        default: 0.31,
    },
    ParamDef {
        id: SPREAD,
        name: "spread",
        min: 0.0,
        max: 1.0,
        default: 0.08,
    },
    ParamDef {
        id: FEED,
        name: "feed",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: PARTIAL1,
        name: "partial1",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: PARTIAL2,
        name: "partial2",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: PARTIAL3,
        name: "partial3",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: PARTIAL4,
        name: "partial4",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: PARTIAL5,
        name: "partial5",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: PARTIAL6,
        name: "partial6",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: CHOKE,
        name: "choke",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: CONTACT,
        name: "contact",
        min: 0.1,
        max: 30.0,
        default: 2.0,
    },
    ParamDef {
        id: DISPERSE,
        name: "disperse",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: DISPERSE_FOCUS,
        name: "disperse_focus",
        min: 0.25,
        max: 16.0,
        default: 2.0,
    },
    ParamDef {
        id: DISPERSE_WIDTH,
        name: "disperse_width",
        min: 0.1,
        max: 8.0,
        default: 1.0,
    },
    ParamDef {
        id: DISPERSE_FOLLOW,
        name: "disperse_follow",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: BLOOM_SIZE,
        name: "bloom_size",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: BLOOM_DECAY,
        name: "bloom_decay",
        min: 0.1,
        max: 12.0,
        default: 3.0,
    },
    ParamDef {
        id: BLOOM_DAMP,
        name: "bloom_damp",
        min: 200.0,
        max: 18000.0,
        default: 6000.0,
    },
    ParamDef {
        id: BLOOM_MIX,
        name: "bloom_mix",
        min: 0.0,
        max: 1.0,
        default: 0.14,
    },
];
pub const LABELS: &[ParamLabel] = &[
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
        name: "Tune",
        unit: " st",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Width",
        unit: "",
        group: "Amp",
        choices: &[],
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
        name: "Env",
        unit: " st",
        group: "Filter",
        choices: &[],
    },
    ParamLabel {
        name: "F.Decay",
        unit: " ms",
        group: "Filter",
        choices: &[],
    },
    ParamLabel {
        name: "Track",
        unit: "",
        group: "Filter",
        choices: &[],
    },
    ParamLabel {
        name: "Material",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Inharm",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Strike",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Hard",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Position",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Spread",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Feed",
        unit: "",
        group: "Body",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 1",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 2",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 3",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 4",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 5",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Mode 6",
        unit: "",
        group: "Modes",
        choices: &[],
    },
    ParamLabel {
        name: "Choke",
        unit: "",
        group: "Modes",
        choices: &["POLY", "CHOKE"],
    },
    ParamLabel {
        name: "Contact",
        unit: " ms",
        group: "Contact",
        choices: &[],
    },
    ParamLabel {
        name: "Amount",
        unit: "",
        group: "Disperse",
        choices: &[],
    },
    ParamLabel {
        name: "Focus",
        unit: "",
        group: "Disperse",
        choices: &[],
    },
    ParamLabel {
        name: "Width",
        unit: " oct",
        group: "Disperse",
        choices: &[],
    },
    ParamLabel {
        name: "Follow",
        unit: "",
        group: "Disperse",
        choices: &[],
    },
    ParamLabel {
        name: "Size",
        unit: "",
        group: "Bloom",
        choices: &[],
    },
    ParamLabel {
        name: "Decay",
        unit: " s",
        group: "Bloom",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: " Hz",
        group: "Bloom",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Bloom",
        choices: &[],
    },
];
pub const DISCRETE: &[u32] = &[CHOKE];
pub const LOG: &[u32] = &[
    DECAY,
    RELEASE,
    CUTOFF,
    FILTER_DECAY,
    CONTACT,
    BLOOM_DECAY,
    BLOOM_DAMP,
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Drive,
    crate::console::SectionKind::Pump,
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Body",
                slots: [
                    Some(MATERIAL),
                    Some(INHARM),
                    Some(DAMP),
                    Some(STRIKE),
                    Some(HARD),
                    Some(POSITION),
                    Some(SPREAD),
                    Some(FEED),
                ],
            },
            SubPage {
                title: "Modes",
                slots: [
                    Some(PARTIAL1),
                    Some(PARTIAL2),
                    Some(PARTIAL3),
                    Some(PARTIAL4),
                    Some(PARTIAL5),
                    Some(PARTIAL6),
                    Some(TUNE),
                    Some(CHOKE),
                ],
            },
            SubPage {
                title: "Contact",
                slots: [Some(CONTACT), None, None, None, None, None, None, None],
            },
        ],
    }),
    Some(MachineKey {
        word: "FLTR",
        subpages: &[SubPage {
            title: "Filter",
            slots: [
                Some(CUTOFF),
                Some(RESONANCE),
                Some(FILTER_ENV),
                Some(FILTER_DECAY),
                Some(KEYTRACK),
                None,
                None,
                None,
            ],
        }],
    }),
    Some(MachineKey {
        word: "AMP",
        subpages: &[SubPage {
            title: "Amp",
            slots: [
                Some(ATTACK),
                Some(DECAY),
                Some(SUSTAIN),
                Some(RELEASE),
                Some(VELOCITY),
                Some(LEVEL),
                Some(WIDTH),
                None,
            ],
        }],
    }),
    None,
    Some(MachineKey {
        word: "FX",
        subpages: &[
            SubPage {
                title: "Disperse",
                slots: [
                    Some(DISPERSE),
                    Some(DISPERSE_FOCUS),
                    Some(DISPERSE_WIDTH),
                    Some(DISPERSE_FOLLOW),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Bloom",
                slots: [
                    Some(BLOOM_SIZE),
                    Some(BLOOM_DECAY),
                    Some(BLOOM_DAMP),
                    Some(BLOOM_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
        ],
    }),
    None,
    None,
];

pub const DEMOS: &[(&str, &[(u32, f32)])] = &[
    (
        "bronze-bowl",
        &[
            (MATERIAL, 2.2),
            (INHARM, 0.95),
            (DECAY, 6500.),
            (DAMP, 0.22),
            (POSITION, 0.22),
            (BLOOM_MIX, 0.3),
            (RELEASE, 3500.),
        ],
    ),
    (
        "metal-click",
        &[
            (MATERIAL, 1.8),
            (INHARM, 1.),
            (TUNE, 24.),
            (DECAY, 65.),
            (DAMP, 0.75),
            (STRIKE, 0.8),
            (HARD, 1.),
            (CONTACT, 0.3),
            (BLOOM_MIX, 0.),
        ],
    ),
    (
        "held-resonance",
        &[
            (MATERIAL, 1.4),
            (INHARM, 0.5),
            (FEED, 0.75),
            (SUSTAIN, 0.8),
            (ATTACK, 250.),
            (RELEASE, 2000.),
            (DECAY, 4000.),
            (BLOOM_MIX, 0.35),
        ],
    ),
];
