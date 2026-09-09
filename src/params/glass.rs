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
pub const ALGORITHM: u32 = 13;
pub const RATIO: u32 = 14;
pub const COARSE: u32 = 15;
pub const FINE: u32 = 16;
pub const INDEX: u32 = 17;
pub const I_DECAY: u32 = 18;
pub const I_SUSTAIN: u32 = 19;
pub const FEEDBACK: u32 = 20;
pub const VEL_INDEX: u32 = 21;
pub const DISORDER: u32 = 22;
pub const PITCH_ENV: u32 = 23;
pub const PITCH_TIME: u32 = 24;
pub const KEY_INDEX: u32 = 25;
pub const PAN_MOTION: u32 = 26;
pub const PAN_RATE: u32 = 27;
pub const PHASE: u32 = 28;
pub const SHIMMER_PITCH: u32 = 29;
pub const SHIMMER_TIME: u32 = 30;
pub const SHIMMER_FEED: u32 = 31;
pub const SHIMMER_MIX: u32 = 32;
pub const TILT: u32 = 33;
pub const TILT_PIVOT: u32 = 34;
pub const TILT_DRIVE: u32 = 35;
pub const TILT_MIX: u32 = 36;
pub const GAIN: u32 = LEVEL;
pub const WALK_X: u32 = DISORDER;
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
        id: ALGORITHM,
        name: "algorithm",
        min: 0.0,
        max: 2.0,
        default: 0.0,
    },
    ParamDef {
        id: RATIO,
        name: "ratio",
        min: 0.5,
        max: 16.0,
        default: 1.4142135,
    },
    ParamDef {
        id: COARSE,
        name: "coarse",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: FINE,
        name: "fine",
        min: -100.0,
        max: 100.0,
        default: 0.0,
    },
    ParamDef {
        id: INDEX,
        name: "index",
        min: 0.0,
        max: 12.0,
        default: 3.0,
    },
    ParamDef {
        id: I_DECAY,
        name: "i_decay",
        min: 5.0,
        max: 8000.0,
        default: 1000.0,
    },
    ParamDef {
        id: I_SUSTAIN,
        name: "i_sustain",
        min: 0.0,
        max: 1.0,
        default: 0.12,
    },
    ParamDef {
        id: FEEDBACK,
        name: "feedback",
        min: 0.0,
        max: 1.0,
        default: 0.08,
    },
    ParamDef {
        id: VEL_INDEX,
        name: "vel_index",
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    ParamDef {
        id: DISORDER,
        name: "disorder",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: PITCH_ENV,
        name: "pitch_env",
        min: -48.0,
        max: 48.0,
        default: 0.0,
    },
    ParamDef {
        id: PITCH_TIME,
        name: "pitch_time",
        min: 5.0,
        max: 4000.0,
        default: 80.0,
    },
    ParamDef {
        id: KEY_INDEX,
        name: "key_index",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: PAN_MOTION,
        name: "pan_motion",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: PAN_RATE,
        name: "pan_rate",
        min: 0.02,
        max: 12.0,
        default: 0.4,
    },
    ParamDef {
        id: PHASE,
        name: "phase",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: SHIMMER_PITCH,
        name: "shimmer_pitch",
        min: -12.0,
        max: 24.0,
        default: 12.0,
    },
    ParamDef {
        id: SHIMMER_TIME,
        name: "shimmer_time",
        min: 50.0,
        max: 1000.0,
        default: 240.0,
    },
    ParamDef {
        id: SHIMMER_FEED,
        name: "shimmer_feed",
        min: -0.85,
        max: 0.85,
        default: 0.3,
    },
    ParamDef {
        id: SHIMMER_MIX,
        name: "shimmer_mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: TILT,
        name: "tilt",
        min: -18.0,
        max: 18.0,
        default: 0.0,
    },
    ParamDef {
        id: TILT_PIVOT,
        name: "tilt_pivot",
        min: 100.0,
        max: 8000.0,
        default: 1000.0,
    },
    ParamDef {
        id: TILT_DRIVE,
        name: "tilt_drive",
        min: 0.0,
        max: 12.0,
        default: 0.0,
    },
    ParamDef {
        id: TILT_MIX,
        name: "tilt_mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
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
        group: "Pitch",
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
        name: "Algo",
        unit: "",
        group: "Ops",
        choices: &["SERIAL", "PARALLEL", "LOOP"],
    },
    ParamLabel {
        name: "Ratio",
        unit: "",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "Coarse",
        unit: "",
        group: "Ops",
        choices: &["FINE", "INTEGER"],
    },
    ParamLabel {
        name: "Fine",
        unit: " ct",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "Index",
        unit: "",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "I.Decay",
        unit: " ms",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "I.Sustain",
        unit: "",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "Feedback",
        unit: "",
        group: "Ops",
        choices: &[],
    },
    ParamLabel {
        name: "Vel>Index",
        unit: "",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Disorder",
        unit: "",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "P.Env",
        unit: " st",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "P.Time",
        unit: " ms",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Key>Index",
        unit: "",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Pan",
        unit: "",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Rate",
        unit: " Hz",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Phase",
        unit: "",
        group: "Gesture",
        choices: &[],
    },
    ParamLabel {
        name: "Pitch",
        unit: " st",
        group: "Shimmer",
        choices: &[],
    },
    ParamLabel {
        name: "Time",
        unit: " ms",
        group: "Shimmer",
        choices: &[],
    },
    ParamLabel {
        name: "Feed",
        unit: "",
        group: "Shimmer",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Shimmer",
        choices: &[],
    },
    ParamLabel {
        name: "Tilt",
        unit: " dB",
        group: "Tilt",
        choices: &[],
    },
    ParamLabel {
        name: "Pivot",
        unit: " Hz",
        group: "Tilt",
        choices: &[],
    },
    ParamLabel {
        name: "Drive",
        unit: "",
        group: "Tilt",
        choices: &[],
    },
    ParamLabel {
        name: "Mix",
        unit: "",
        group: "Tilt",
        choices: &[],
    },
];
pub const DISCRETE: &[u32] = &[ALGORITHM, COARSE];
pub const LOG: &[u32] = &[
    DECAY,
    RELEASE,
    CUTOFF,
    FILTER_DECAY,
    I_DECAY,
    PITCH_TIME,
    PAN_RATE,
    SHIMMER_TIME,
    TILT_PIVOT,
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
                title: "Ops",
                slots: [
                    Some(ALGORITHM),
                    Some(RATIO),
                    Some(COARSE),
                    Some(FINE),
                    Some(INDEX),
                    Some(I_DECAY),
                    Some(I_SUSTAIN),
                    Some(FEEDBACK),
                ],
            },
            SubPage {
                title: "Gesture",
                slots: [
                    Some(VEL_INDEX),
                    Some(DISORDER),
                    Some(PITCH_ENV),
                    Some(PITCH_TIME),
                    Some(KEY_INDEX),
                    Some(PAN_MOTION),
                    Some(PAN_RATE),
                    Some(PHASE),
                ],
            },
            SubPage {
                title: "Pitch",
                slots: [Some(TUNE), None, None, None, None, None, None, None],
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
                title: "Shimmer",
                slots: [
                    Some(SHIMMER_PITCH),
                    Some(SHIMMER_TIME),
                    Some(SHIMMER_FEED),
                    Some(SHIMMER_MIX),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            SubPage {
                title: "Tilt",
                slots: [
                    Some(TILT),
                    Some(TILT_PIVOT),
                    Some(TILT_DRIVE),
                    Some(TILT_MIX),
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
        "electric-keys",
        &[
            (RATIO, 1.),
            (INDEX, 2.5),
            (I_DECAY, 400.),
            (I_SUSTAIN, 0.05),
            (FEEDBACK, 0.03),
            (DECAY, 1800.),
            (SUSTAIN, 0.12),
            (RELEASE, 450.),
            (CUTOFF, 6000.),
        ],
    ),
    (
        "ice-bell",
        &[
            (RATIO, 1.4142135),
            (INDEX, 5.),
            (I_DECAY, 2200.),
            (I_SUSTAIN, 0.12),
            (DECAY, 4500.),
            (SUSTAIN, 0.),
            (RELEASE, 2500.),
            (SHIMMER_MIX, 0.2),
            (SHIMMER_FEED, 0.45),
        ],
    ),
    (
        "folded-fm-bass",
        &[
            (RATIO, 1.5),
            (INDEX, 4.),
            (I_DECAY, 180.),
            (I_SUSTAIN, 0.25),
            (ALGORITHM, 2.),
            (FEEDBACK, 0.65),
            (CUTOFF, 2200.),
            (DECAY, 500.),
            (SUSTAIN, 0.35),
            (TILT_DRIVE, 2.),
        ],
    ),
];
