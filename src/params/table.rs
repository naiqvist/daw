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
pub const MORPH: u32 = 13;
pub const SCAN: u32 = 14;
pub const SCAN_TIME: u32 = 15;
pub const SUB: u32 = 16;
pub const DETUNE: u32 = 17;
pub const PHASE: u32 = 18;
pub const VEL_MORPH: u32 = 19;
pub const ROUGH: u32 = 20;
pub const PW: u32 = 21;
pub const VOICING: u32 = 22;
pub const INVERSION: u32 = 23;
pub const OPEN: u32 = 24;
pub const DRIFT: u32 = 25;
pub const DRIFT_RATE: u32 = 26;
pub const MOTION: u32 = 27;
pub const MOTION_RATE: u32 = 28;
pub const RIPPLE_FOCUS: u32 = 29;
pub const RIPPLE_FEED: u32 = 30;
pub const RIPPLE_DAMP: u32 = 31;
pub const RIPPLE_MIX: u32 = 32;
pub const FOLD: u32 = 33;
pub const FOLD_TILT: u32 = 34;
pub const FOLD_BIAS: u32 = 35;
pub const FOLD_MIX: u32 = 36;
pub const ENSEMBLE_DEPTH: u32 = 37;
pub const ENSEMBLE_RATE: u32 = 38;
pub const ENSEMBLE_SPREAD: u32 = 39;
pub const ENSEMBLE_MIX: u32 = 40;
pub const SLAP_TIME: u32 = 41;
pub const SLAP_FEED: u32 = 42;
pub const SLAP_DAMP: u32 = 43;
pub const SLAP_MIX: u32 = 44;
pub const GAIN: u32 = LEVEL;
pub const WALK_X: u32 = MORPH;
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
        id: MORPH,
        name: "morph",
        min: 0.0,
        max: 7.0,
        default: 2.5,
    },
    ParamDef {
        id: SCAN,
        name: "scan",
        min: -7.0,
        max: 7.0,
        default: 0.0,
    },
    ParamDef {
        id: SCAN_TIME,
        name: "scan_time",
        min: 5.0,
        max: 8000.0,
        default: 1200.0,
    },
    ParamDef {
        id: SUB,
        name: "sub",
        min: 0.0,
        max: 1.0,
        default: 0.1,
    },
    ParamDef {
        id: DETUNE,
        name: "detune",
        min: 0.0,
        max: 50.0,
        default: 8.0,
    },
    ParamDef {
        id: PHASE,
        name: "phase",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: VEL_MORPH,
        name: "vel_morph",
        min: -3.0,
        max: 3.0,
        default: 0.0,
    },
    ParamDef {
        id: ROUGH,
        name: "rough",
        min: 0.0,
        max: 1.0,
        default: 0.65,
    },
    ParamDef {
        id: PW,
        name: "pw",
        min: 0.05,
        max: 0.95,
        default: 0.5,
    },
    ParamDef {
        id: VOICING,
        name: "voicing",
        min: 0.0,
        max: 8.0,
        default: 0.0,
    },
    ParamDef {
        id: INVERSION,
        name: "inversion",
        min: 0.0,
        max: 3.0,
        default: 0.0,
    },
    ParamDef {
        id: OPEN,
        name: "open",
        min: 0.0,
        max: 2.0,
        default: 0.0,
    },
    ParamDef {
        id: DRIFT,
        name: "drift",
        min: 0.0,
        max: 30.0,
        default: 0.0,
    },
    ParamDef {
        id: DRIFT_RATE,
        name: "drift_rate",
        min: 0.02,
        max: 4.0,
        default: 0.17,
    },
    ParamDef {
        id: MOTION,
        name: "motion",
        min: 0.0,
        max: 36.0,
        default: 0.0,
    },
    ParamDef {
        id: MOTION_RATE,
        name: "motion_rate",
        min: 0.02,
        max: 12.0,
        default: 0.3,
    },
    ParamDef {
        id: RIPPLE_FOCUS,
        name: "ripple_focus",
        min: 0.5,
        max: 8.0,
        default: 1.0,
    },
    ParamDef {
        id: RIPPLE_FEED,
        name: "ripple_feed",
        min: -0.95,
        max: 0.95,
        default: 0.35,
    },
    ParamDef {
        id: RIPPLE_DAMP,
        name: "ripple_damp",
        min: 100.0,
        max: 18000.0,
        default: 6000.0,
    },
    ParamDef {
        id: RIPPLE_MIX,
        name: "ripple_mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: FOLD,
        name: "fold",
        min: 0.0,
        max: 12.0,
        default: 0.0,
    },
    ParamDef {
        id: FOLD_TILT,
        name: "fold_tilt",
        min: -18.0,
        max: 18.0,
        default: 0.0,
    },
    ParamDef {
        id: FOLD_BIAS,
        name: "fold_bias",
        min: -0.8,
        max: 0.8,
        default: 0.0,
    },
    ParamDef {
        id: FOLD_MIX,
        name: "fold_mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ENSEMBLE_DEPTH,
        name: "ensemble_depth",
        min: 0.0,
        max: 12.0,
        default: 3.0,
    },
    ParamDef {
        id: ENSEMBLE_RATE,
        name: "ensemble_rate",
        min: 0.05,
        max: 6.0,
        default: 0.6,
    },
    ParamDef {
        id: ENSEMBLE_SPREAD,
        name: "ensemble_spread",
        min: 0.0,
        max: 1.0,
        default: 0.8,
    },
    ParamDef {
        id: ENSEMBLE_MIX,
        name: "ensemble_mix",
        min: 0.0,
        max: 1.0,
        default: 0.12,
    },
    ParamDef {
        id: SLAP_TIME,
        name: "slap_time",
        min: 20.0,
        max: 250.0,
        default: 90.0,
    },
    ParamDef {
        id: SLAP_FEED,
        name: "slap_feed",
        min: -0.9,
        max: 0.9,
        default: 0.3,
    },
    ParamDef {
        id: SLAP_DAMP,
        name: "slap_damp",
        min: 200.0,
        max: 18000.0,
        default: 6000.0,
    },
    ParamDef {
        id: SLAP_MIX,
        name: "slap_mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
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
        group: "Wave",
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
        name: "Morph",
        unit: "",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "Scan",
        unit: "",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "S.Time",
        unit: " ms",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "Sub",
        unit: "",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Detune",
        unit: " ct",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Phase",
        unit: "",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "Vel>Morph",
        unit: "",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "Rough",
        unit: "",
        group: "Wave",
        choices: &[],
    },
    ParamLabel {
        name: "Pw",
        unit: "",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Voicing",
        unit: "",
        group: "Voicing",
        choices: &[
            "NONE", "POWER", "MAJ", "MIN", "MAJ7", "MIN7", "DOM7", "SUS", "STAB",
        ],
    },
    ParamLabel {
        name: "Inversion",
        unit: "",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Open",
        unit: " oct",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Drift",
        unit: " ct",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Drift.Rate",
        unit: " Hz",
        group: "Voicing",
        choices: &[],
    },
    ParamLabel {
        name: "Motion",
        unit: " st",
        group: "Filter",
        choices: &[],
    },
    ParamLabel {
        name: "Motion.Rate",
        unit: " Hz",
        group: "Filter",
        choices: &[],
    },
    ParamLabel {
        name: "Focus",
        unit: "",
        group: "Ripple",
        choices: &[],
    },
    ParamLabel {
        name: "Feed",
        unit: "",
        group: "Ripple",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: " Hz",
        group: "Ripple",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Ripple",
        choices: &[],
    },
    ParamLabel {
        name: "Fold",
        unit: "",
        group: "Fold",
        choices: &[],
    },
    ParamLabel {
        name: "Tilt",
        unit: " dB",
        group: "Fold",
        choices: &[],
    },
    ParamLabel {
        name: "Bias",
        unit: "",
        group: "Fold",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Fold",
        choices: &[],
    },
    ParamLabel {
        name: "Depth",
        unit: " ms",
        group: "Ensemble",
        choices: &[],
    },
    ParamLabel {
        name: "Rate",
        unit: " Hz",
        group: "Ensemble",
        choices: &[],
    },
    ParamLabel {
        name: "Spread",
        unit: "",
        group: "Ensemble",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Ensemble",
        choices: &[],
    },
    ParamLabel {
        name: "Time",
        unit: " ms",
        group: "Slap",
        choices: &[],
    },
    ParamLabel {
        name: "Feed",
        unit: "",
        group: "Slap",
        choices: &[],
    },
    ParamLabel {
        name: "Damp",
        unit: " Hz",
        group: "Slap",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Slap",
        choices: &[],
    },
];
pub const DISCRETE: &[u32] = &[VOICING, INVERSION];
pub const LOG: &[u32] = &[
    DECAY,
    RELEASE,
    CUTOFF,
    FILTER_DECAY,
    SCAN_TIME,
    DRIFT_RATE,
    MOTION_RATE,
    RIPPLE_DAMP,
    ENSEMBLE_RATE,
    SLAP_TIME,
    SLAP_DAMP,
];
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[crate::console::SectionKind::Room];
pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Wave",
                slots: [
                    Some(MORPH),
                    Some(SCAN),
                    Some(SCAN_TIME),
                    Some(ROUGH),
                    Some(PHASE),
                    Some(VEL_MORPH),
                    Some(TUNE),
                    None,
                ],
            },
            SubPage {
                title: "Voicing",
                slots: [
                    Some(VOICING),
                    Some(INVERSION),
                    Some(OPEN),
                    Some(DETUNE),
                    Some(SUB),
                    Some(PW),
                    Some(DRIFT),
                    Some(DRIFT_RATE),
                ],
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
                Some(MOTION),
                Some(MOTION_RATE),
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
                title: "Ripple",
                slots: [
                    Some(RIPPLE_FOCUS),
                    Some(RIPPLE_FEED),
                    Some(RIPPLE_DAMP),
                    Some(RIPPLE_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Fold",
                slots: [
                    Some(FOLD),
                    Some(FOLD_TILT),
                    Some(FOLD_BIAS),
                    Some(FOLD_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Ensemble",
                slots: [
                    Some(ENSEMBLE_DEPTH),
                    Some(ENSEMBLE_RATE),
                    Some(ENSEMBLE_SPREAD),
                    Some(ENSEMBLE_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Slap",
                slots: [
                    Some(SLAP_TIME),
                    Some(SLAP_FEED),
                    Some(SLAP_DAMP),
                    Some(SLAP_MIX),
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
        "chord-stab",
        &[
            (VOICING, 5.),
            (MORPH, 2.4),
            (CUTOFF, 2400.),
            (FILTER_ENV, 28.),
            (FILTER_DECAY, 140.),
            (DECAY, 650.),
            (SUSTAIN, 0.),
            (RELEASE, 180.),
            (ENSEMBLE_MIX, 0.1),
        ],
    ),
    (
        "slow-air",
        &[
            (MORPH, 5.7),
            (SCAN, -2.3),
            (SCAN_TIME, 5000.),
            (ATTACK, 1800.),
            (SUSTAIN, 0.85),
            (RELEASE, 4500.),
            (ROUGH, 0.8),
            (ENSEMBLE_MIX, 0.35),
            (CUTOFF, 4200.),
            (VOICING, 5.),
        ],
    ),
    (
        "wire-bass",
        &[
            (MORPH, 3.),
            (PW, 0.2),
            (SUB, 0.45),
            (CUTOFF, 1200.),
            (FILTER_ENV, 20.),
            (FILTER_DECAY, 180.),
            (FOLD, 3.),
            (FOLD_MIX, 0.3),
            (DETUNE, 0.),
            (SUSTAIN, 0.4),
        ],
    ),
];
