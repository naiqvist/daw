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
pub const SOURCE: u32 = 13;
pub const SIEVE: u32 = 14;
pub const SIEVE_WIDTH: u32 = 15;
pub const SHIFT: u32 = 16;
pub const TILT: u32 = 17;
pub const BLUR: u32 = 18;
pub const FREEZE: u32 = 19;
pub const BAND: u32 = 20;
pub const S_ENV: u32 = 21;
pub const S_TIME: u32 = 22;
pub const SHIFT_LFO: u32 = 23;
pub const SHIFT_RATE: u32 = 24;
pub const F_ENV: u32 = 25;
pub const F_TIME: u32 = 26;
pub const ROUGH: u32 = 27;
pub const SEED: u32 = 28;
pub const SMEAR_LOW: u32 = 29;
pub const SMEAR_HIGH: u32 = 30;
pub const SMEAR_FEED: u32 = 31;
pub const SMEAR_MIX: u32 = 32;
pub const HALO_DECAY: u32 = 33;
pub const HALO_TILT: u32 = 34;
pub const HALO_DAMP: u32 = 35;
pub const HALO_MIX: u32 = 36;
pub const GAIN: u32 = LEVEL;
pub const WALK_X: u32 = SIEVE;
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
        default: 800.0,
    },
    ParamDef {
        id: SUSTAIN,
        name: "sustain",
        min: 0.0,
        max: 1.0,
        default: 0.7,
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
        id: SOURCE,
        name: "source",
        min: 0.0,
        max: 2.0,
        default: 0.0,
    },
    ParamDef {
        id: SIEVE,
        name: "sieve",
        min: 0.0,
        max: 1.0,
        default: 0.55,
    },
    ParamDef {
        id: SIEVE_WIDTH,
        name: "sieve_width",
        min: 0.05,
        max: 1.0,
        default: 0.2,
    },
    ParamDef {
        id: SHIFT,
        name: "shift",
        min: -2000.0,
        max: 2000.0,
        default: 0.0,
    },
    ParamDef {
        id: TILT,
        name: "tilt",
        min: -24.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: BLUR,
        name: "blur",
        min: 0.0,
        max: 1.0,
        default: 0.15,
    },
    ParamDef {
        id: FREEZE,
        name: "freeze",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: BAND,
        name: "band",
        min: 100.0,
        max: 16000.0,
        default: 8000.0,
    },
    ParamDef {
        id: S_ENV,
        name: "s_env",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: S_TIME,
        name: "s_time",
        min: 5.0,
        max: 8000.0,
        default: 900.0,
    },
    ParamDef {
        id: SHIFT_LFO,
        name: "shift_lfo",
        min: 0.0,
        max: 2000.0,
        default: 0.0,
    },
    ParamDef {
        id: SHIFT_RATE,
        name: "shift_rate",
        min: 0.02,
        max: 20.0,
        default: 0.4,
    },
    ParamDef {
        id: F_ENV,
        name: "f_env",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: F_TIME,
        name: "f_time",
        min: 5.0,
        max: 8000.0,
        default: 1200.0,
    },
    ParamDef {
        id: ROUGH,
        name: "rough",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: SEED,
        name: "seed",
        min: 1.0,
        max: 65535.0,
        default: 1709.0,
    },
    ParamDef {
        id: SMEAR_LOW,
        name: "smear_low",
        min: 0.0,
        max: 200.0,
        default: 0.0,
    },
    ParamDef {
        id: SMEAR_HIGH,
        name: "smear_high",
        min: 0.0,
        max: 200.0,
        default: 80.0,
    },
    ParamDef {
        id: SMEAR_FEED,
        name: "smear_feed",
        min: -0.9,
        max: 0.9,
        default: 0.2,
    },
    ParamDef {
        id: SMEAR_MIX,
        name: "smear_mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: HALO_DECAY,
        name: "halo_decay",
        min: 0.05,
        max: 10.0,
        default: 2.0,
    },
    ParamDef {
        id: HALO_TILT,
        name: "halo_tilt",
        min: -24.0,
        max: 24.0,
        default: -6.0,
    },
    ParamDef {
        id: HALO_DAMP,
        name: "halo_damp",
        min: 200.0,
        max: 18000.0,
        default: 8000.0,
    },
    ParamDef {
        id: HALO_MIX,
        name: "halo_mix",
        min: 0.0,
        max: 1.0,
        default: 0.1,
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
        group: "Prism",
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
        name: "Source",
        unit: "",
        group: "Prism",
        choices: &["NOISE", "SAW", "PULSE"],
    },
    ParamLabel {
        name: "Sieve",
        unit: "",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Width",
        unit: "",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Shift",
        unit: " Hz",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Tilt",
        unit: " dB",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Blur",
        unit: "",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Freeze",
        unit: "",
        group: "Prism",
        choices: &[],
    },
    ParamLabel {
        name: "Band",
        unit: " Hz",
        group: "Band",
        choices: &[],
    },
    ParamLabel {
        name: "S.Env",
        unit: "",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "S.Time",
        unit: " ms",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "Sh.Lfo",
        unit: " Hz",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "Sh.Rate",
        unit: " Hz",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "F.Env",
        unit: "",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "F.Time",
        unit: " ms",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "Rough",
        unit: "",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "Seed",
        unit: "",
        group: "Motion",
        choices: &[],
    },
    ParamLabel {
        name: "Low",
        unit: " ms",
        group: "Smear",
        choices: &[],
    },
    ParamLabel {
        name: "High",
        unit: " ms",
        group: "Smear",
        choices: &[],
    },
    ParamLabel {
        name: "Feed",
        unit: "",
        group: "Smear",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Smear",
        choices: &[],
    },
    ParamLabel {
        name: "Decay",
        unit: " s",
        group: "Halo",
        choices: &[],
    },
    ParamLabel {
        name: "Tilt",
        unit: " dB",
        group: "Halo",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: " Hz",
        group: "Halo",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Halo",
        choices: &[],
    },
];
pub const DISCRETE: &[u32] = &[SOURCE, SEED];
pub const LOG: &[u32] = &[
    DECAY,
    RELEASE,
    CUTOFF,
    FILTER_DECAY,
    BAND,
    S_TIME,
    SHIFT_RATE,
    F_TIME,
    HALO_DECAY,
    HALO_DAMP,
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Room,
    crate::console::SectionKind::Echo,
];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Prism",
                slots: [
                    Some(SOURCE),
                    Some(SIEVE),
                    Some(SIEVE_WIDTH),
                    Some(SHIFT),
                    Some(TILT),
                    Some(BLUR),
                    Some(FREEZE),
                    Some(TUNE),
                ],
            },
            SubPage {
                title: "Motion",
                slots: [
                    Some(S_ENV),
                    Some(S_TIME),
                    Some(SHIFT_LFO),
                    Some(SHIFT_RATE),
                    Some(F_ENV),
                    Some(F_TIME),
                    Some(ROUGH),
                    Some(SEED),
                ],
            },
            SubPage {
                title: "Band",
                slots: [Some(BAND), None, None, None, None, None, None, None],
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
                title: "Smear",
                slots: [
                    Some(SMEAR_LOW),
                    Some(SMEAR_HIGH),
                    Some(SMEAR_FEED),
                    Some(SMEAR_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Halo",
                slots: [
                    Some(HALO_DECAY),
                    Some(HALO_TILT),
                    Some(HALO_DAMP),
                    Some(HALO_MIX),
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
        "tuned-breath",
        &[
            (SIEVE, 1.),
            (SIEVE_WIDTH, 0.13),
            (TILT, -4.),
            (ATTACK, 80.),
            (SUSTAIN, 0.8),
            (RELEASE, 1100.),
            (HALO_MIX, 0.2),
        ],
    ),
    (
        "frozen-air",
        &[
            (SIEVE, 0.35),
            (FREEZE, 1.),
            (BLUR, 0.3),
            (ATTACK, 600.),
            (SUSTAIN, 1.),
            (RELEASE, 3000.),
            (SHIFT, 130.),
            (HALO_MIX, 0.3),
        ],
    ),
    (
        "spectral-chirp",
        &[
            (SIEVE, 0.85),
            (SHIFT, -170.),
            (SHIFT_LFO, 390.),
            (SHIFT_RATE, 0.8),
            (SMEAR_HIGH, 180.),
            (SMEAR_LOW, 0.),
            (SMEAR_MIX, 0.65),
            (DECAY, 1600.),
            (SUSTAIN, 0.1),
        ],
    ),
];
