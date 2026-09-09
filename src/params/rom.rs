//! ROM: the rompler. See notes/20260909-rom-brief.md.
//!
//! Two oscillators of resident factory multisamples through a filter,
//! two envelopes, an LFO and an ensemble. The bank is baked from
//! recipes in `crate::audio::rom::bank`; VINTAGE puts it back in 1988.

use super::ParamDef;
use crate::pages::{KeyTable, MachineKey, SubPage};

pub const PCM1: u32 = 0;
pub const TUNE1: u32 = 1;
pub const FINE1: u32 = 2;
pub const START1: u32 = 3;
pub const KEY1: u32 = 4;
pub const PAN1: u32 = 5;
pub const LOOP1: u32 = 6;
pub const PCM2: u32 = 7;
pub const TUNE2: u32 = 8;
pub const FINE2: u32 = 9;
pub const START2: u32 = 10;
pub const KEY2: u32 = 11;
pub const PAN2: u32 = 12;
pub const LOOP2: u32 = 13;
pub const MODE: u32 = 14;
pub const SPLIT: u32 = 15;
pub const VELPT: u32 = 16;
pub const BALANCE: u32 = 17;
pub const DETUNE: u32 = 18;
pub const XFADE: u32 = 19;
pub const GLIDE: u32 = 20;
pub const VOICES: u32 = 21;
pub const TYPE: u32 = 22;
pub const CUTOFF: u32 = 23;
pub const RESO: u32 = 24;
pub const FENV: u32 = 25;
pub const FKEY: u32 = 26;
pub const FVEL: u32 = 27;
pub const FATTACK: u32 = 28;
pub const FDECAY: u32 = 29;
pub const FSUSTAIN: u32 = 30;
pub const FRELEASE: u32 = 31;
pub const ATTACK: u32 = 32;
pub const DECAY: u32 = 33;
pub const SUSTAIN: u32 = 34;
pub const RELEASE: u32 = 35;
pub const VEL: u32 = 36;
pub const KEYDEC: u32 = 37;
pub const PAN: u32 = 38;
pub const LEVEL: u32 = 39;
pub const SHAPE: u32 = 40;
pub const RATE: u32 = 41;
pub const FADE: u32 = 42;
pub const LPITCH: u32 = 43;
pub const LCUT: u32 = 44;
pub const LAMP: u32 = 45;
pub const LTRIG: u32 = 46;
pub const CRATE: u32 = 47;
pub const CDEPTH: u32 = 48;
pub const CWIDTH: u32 = 49;
pub const CMIX: u32 = 50;
pub const VINTAGE: u32 = 51;
pub const VRATE: u32 = 52;
pub const VBITS: u32 = 53;
pub const VLOOP: u32 = 54;

/// The excluded-gain machinery's name for the output level.
pub const GAIN: u32 = LEVEL;
/// How many rows the table has.
pub const COUNT: usize = TABLE.len();

pub const TABLE: &[ParamDef] = &[
    ParamDef {
        id: PCM1,
        name: "pcm1",
        min: 0.0,
        max: crate::audio::rom::bank::MULTI_MAX,
        default: 0.0,
    },
    ParamDef {
        id: TUNE1,
        name: "tune1",
        min: -24.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: FINE1,
        name: "fine1",
        min: -50.0,
        max: 50.0,
        default: 0.0,
    },
    ParamDef {
        id: START1,
        name: "start1",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: KEY1,
        name: "key1",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: PAN1,
        name: "pan1",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: LOOP1,
        name: "loop1",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: PCM2,
        name: "pcm2",
        min: 0.0,
        max: crate::audio::rom::bank::MULTI_MAX,
        default: 0.0,
    },
    ParamDef {
        id: TUNE2,
        name: "tune2",
        min: -24.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: FINE2,
        name: "fine2",
        min: -50.0,
        max: 50.0,
        default: 0.0,
    },
    ParamDef {
        id: START2,
        name: "start2",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: KEY2,
        name: "key2",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: PAN2,
        name: "pan2",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: LOOP2,
        name: "loop2",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: MODE,
        name: "mode",
        min: 0.0,
        max: 3.0,
        default: 0.0,
    },
    ParamDef {
        id: SPLIT,
        name: "split",
        min: 0.0,
        max: 127.0,
        default: 60.0,
    },
    ParamDef {
        id: VELPT,
        name: "velpt",
        min: 1.0,
        max: 127.0,
        default: 64.0,
    },
    ParamDef {
        id: BALANCE,
        name: "balance",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: DETUNE,
        name: "detune",
        min: 0.0,
        max: 50.0,
        default: 0.0,
    },
    ParamDef {
        id: XFADE,
        name: "xfade",
        min: 0.0,
        max: 24.0,
        default: 0.0,
    },
    ParamDef {
        id: GLIDE,
        name: "glide",
        min: 0.0,
        max: 2000.0,
        default: 0.0,
    },
    ParamDef {
        id: VOICES,
        name: "voices",
        min: 1.0,
        max: 16.0,
        default: 16.0,
    },
    ParamDef {
        id: TYPE,
        name: "ftype",
        min: 0.0,
        max: 3.0,
        default: 0.0,
    },
    ParamDef {
        id: CUTOFF,
        name: "cutoff",
        min: 20.0,
        max: 18000.0,
        default: 18000.0,
    },
    ParamDef {
        id: RESO,
        name: "reso",
        min: 0.0,
        max: 1.0,
        default: 0.1,
    },
    ParamDef {
        id: FENV,
        name: "fenv",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: FKEY,
        name: "fkey",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: FVEL,
        name: "fvel",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: FATTACK,
        name: "fattack",
        min: 0.1,
        max: 8000.0,
        default: 0.1,
    },
    ParamDef {
        id: FDECAY,
        name: "fdecay",
        min: 0.1,
        max: 8000.0,
        default: 400.0,
    },
    ParamDef {
        id: FSUSTAIN,
        name: "fsustain",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: FRELEASE,
        name: "frelease",
        min: 0.1,
        max: 8000.0,
        default: 200.0,
    },
    ParamDef {
        id: ATTACK,
        name: "attack",
        min: 0.1,
        max: 8000.0,
        default: 0.5,
    },
    ParamDef {
        id: DECAY,
        name: "decay",
        min: 0.1,
        max: 8000.0,
        default: 2000.0,
    },
    ParamDef {
        id: SUSTAIN,
        name: "sustain",
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: RELEASE,
        name: "release",
        min: 0.1,
        max: 8000.0,
        default: 200.0,
    },
    ParamDef {
        id: VEL,
        name: "vel",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamDef {
        id: KEYDEC,
        name: "keydec",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: PAN,
        name: "pan",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: LEVEL,
        name: "level",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: SHAPE,
        name: "shape",
        min: 0.0,
        max: 4.0,
        default: 0.0,
    },
    ParamDef {
        id: RATE,
        name: "rate",
        min: 0.01,
        max: 40.0,
        default: 4.0,
    },
    ParamDef {
        id: FADE,
        name: "fade",
        min: 0.0,
        max: 4000.0,
        default: 0.0,
    },
    ParamDef {
        id: LPITCH,
        name: "lpitch",
        min: -100.0,
        max: 100.0,
        default: 0.0,
    },
    ParamDef {
        id: LCUT,
        name: "lcut",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: LAMP,
        name: "lamp",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: LTRIG,
        name: "ltrig",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    ParamDef {
        id: CRATE,
        name: "crate_hz",
        min: 0.05,
        max: 8.0,
        default: 0.6,
    },
    ParamDef {
        id: CDEPTH,
        name: "cdepth",
        min: 0.0,
        max: 1.0,
        default: 0.3,
    },
    ParamDef {
        id: CWIDTH,
        name: "cwidth",
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    ParamDef {
        id: CMIX,
        name: "cmix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: VINTAGE,
        name: "vintage",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: VRATE,
        name: "vrate",
        min: 4000.0,
        max: 48000.0,
        default: 32000.0,
    },
    ParamDef {
        id: VBITS,
        name: "vbits",
        min: 4.0,
        max: 16.0,
        default: 12.0,
    },
    ParamDef {
        id: VLOOP,
        name: "vloop",
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
];

pub const LABELS: &[crate::devices::ParamLabel] = &[
    crate::devices::ParamLabel {
        name: "PCM",
        unit: "",
        group: "Osc 1",
        choices: crate::audio::rom::bank::MULTI_NAMES,
    },
    crate::devices::ParamLabel {
        name: "Tune",
        unit: " st",
        group: "Osc 1",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Fine",
        unit: " ct",
        group: "Osc 1",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Start",
        unit: "",
        group: "Osc 1",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Key Trk",
        unit: "",
        group: "Osc 1",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Pan",
        unit: "",
        group: "Osc 1",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Loop",
        unit: "",
        group: "Osc 1",
        choices: &["off", "fwd"],
    },
    crate::devices::ParamLabel {
        name: "PCM",
        unit: "",
        group: "Osc 2",
        choices: crate::audio::rom::bank::MULTI_NAMES,
    },
    crate::devices::ParamLabel {
        name: "Tune",
        unit: " st",
        group: "Osc 2",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Fine",
        unit: " ct",
        group: "Osc 2",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Start",
        unit: "",
        group: "Osc 2",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Key Trk",
        unit: "",
        group: "Osc 2",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Pan",
        unit: "",
        group: "Osc 2",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Loop",
        unit: "",
        group: "Osc 2",
        choices: &["off", "fwd"],
    },
    crate::devices::ParamLabel {
        name: "Mode",
        unit: "",
        group: "Layer",
        choices: &["single", "double", "split", "vel"],
    },
    crate::devices::ParamLabel {
        name: "Split",
        unit: "",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Vel Pt",
        unit: "",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Balance",
        unit: "",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Detune",
        unit: " ct",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Xfade",
        unit: "",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Glide",
        unit: " ms",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Voices",
        unit: "",
        group: "Layer",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Type",
        unit: "",
        group: "Filter",
        choices: &["LP12", "LP24", "BP", "HP"],
    },
    crate::devices::ParamLabel {
        name: "Cutoff",
        unit: " Hz",
        group: "Filter",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Reso",
        unit: "",
        group: "Filter",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Env",
        unit: "",
        group: "Filter",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Key Trk",
        unit: "",
        group: "Filter",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Vel",
        unit: "",
        group: "Filter",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Attack",
        unit: " ms",
        group: "F EG",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Decay",
        unit: " ms",
        group: "F EG",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Sustain",
        unit: "",
        group: "F EG",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Release",
        unit: " ms",
        group: "F EG",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Attack",
        unit: " ms",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Decay",
        unit: " ms",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Sustain",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Release",
        unit: " ms",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Vel",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Key Dec",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Pan",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Level",
        unit: "",
        group: "Amp",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Shape",
        unit: "",
        group: "LFO",
        choices: &["tri", "sine", "saw", "sqr", "S&H"],
    },
    crate::devices::ParamLabel {
        name: "Rate",
        unit: " Hz",
        group: "LFO",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Fade",
        unit: " ms",
        group: "LFO",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Pitch",
        unit: " ct",
        group: "LFO",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Cutoff",
        unit: "",
        group: "LFO",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Amp",
        unit: "",
        group: "LFO",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Trig",
        unit: "",
        group: "LFO",
        choices: &["free", "retrig", "one"],
    },
    crate::devices::ParamLabel {
        name: "Rate",
        unit: " Hz",
        group: "Chorus",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Depth",
        unit: "",
        group: "Chorus",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Width",
        unit: "",
        group: "Chorus",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Mix",
        unit: "",
        group: "Chorus",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Vintage",
        unit: "",
        group: "Vintage",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Rate",
        unit: " Hz",
        group: "Vintage",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Bits",
        unit: "",
        group: "Vintage",
        choices: &[],
    },
    crate::devices::ParamLabel {
        name: "Loop Cut",
        unit: "",
        group: "Vintage",
        choices: &[],
    },
];

pub const KEYS: KeyTable = [
    None,
    Some(MachineKey {
        word: "SRC",
        subpages: &[
            SubPage {
                title: "Osc 1",
                slots: [
                    Some(0),
                    Some(1),
                    Some(2),
                    Some(3),
                    Some(4),
                    Some(5),
                    Some(6),
                    None,
                ],
            },
            SubPage {
                title: "Osc 2",
                slots: [
                    Some(7),
                    Some(8),
                    Some(9),
                    Some(10),
                    Some(11),
                    Some(12),
                    Some(13),
                    None,
                ],
            },
            SubPage {
                title: "Layer",
                slots: [
                    Some(14),
                    Some(15),
                    Some(16),
                    Some(17),
                    Some(18),
                    Some(19),
                    Some(20),
                    Some(21),
                ],
            },
        ],
    }),
    Some(MachineKey {
        word: "FLTR",
        subpages: &[
            SubPage {
                title: "Filter",
                slots: [
                    Some(22),
                    Some(23),
                    Some(24),
                    Some(25),
                    Some(26),
                    Some(27),
                    None,
                    None,
                ],
            },
            SubPage {
                title: "F EG",
                slots: [
                    Some(28),
                    Some(29),
                    Some(30),
                    Some(31),
                    None,
                    None,
                    None,
                    None,
                ],
            },
        ],
    }),
    Some(MachineKey {
        word: "AMP",
        subpages: &[SubPage {
            title: "Amp",
            slots: [
                Some(32),
                Some(33),
                Some(34),
                Some(35),
                Some(36),
                Some(37),
                Some(38),
                Some(39),
            ],
        }],
    }),
    Some(MachineKey {
        word: "LFO",
        subpages: &[SubPage {
            title: "LFO",
            slots: [
                Some(40),
                Some(41),
                Some(42),
                Some(43),
                Some(44),
                Some(45),
                Some(46),
                None,
            ],
        }],
    }),
    Some(MachineKey {
        word: "FX",
        subpages: &[SubPage {
            title: "Chorus",
            slots: [
                Some(47),
                Some(48),
                Some(49),
                Some(50),
                None,
                None,
                None,
                None,
            ],
        }],
    }),
    None,
    Some(MachineKey {
        word: "ROM",
        subpages: &[SubPage {
            title: "Vintage",
            slots: [
                Some(51),
                Some(52),
                Some(53),
                Some(54),
                None,
                None,
                None,
                None,
            ],
        }],
    }),
];

/// Cells the deck steps one position at a time.
pub const DISCRETE: &[u32] = &[
    PCM1, TUNE1, LOOP1, PCM2, TUNE2, LOOP2, MODE, SPLIT, VELPT, VOICES, TYPE, SHAPE, LTRIG,
];
/// Cells whose feel is logarithmic: hertz, and times with a positive floor.
pub const LOG: &[u32] = &[
    CUTOFF, FATTACK, FDECAY, FRELEASE, ATTACK, DECAY, RELEASE, RATE, CRATE, VRATE,
];

/// Four effects on the FX key: the machine's own ensemble leads, then
/// the lane's colour, echo and room.
pub const FX_SECTIONS: &[crate::console::SectionKind] = &[
    crate::console::SectionKind::Drive,
    crate::console::SectionKind::Echo,
    crate::console::SectionKind::Room,
];

/// The filter modes TYPE walks, as the engine reads them.
pub const FILTER_MODES: usize = 4;
