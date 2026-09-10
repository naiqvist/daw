//! ROM's factory programs: eight jungle pads, and their effects.
//!
//! A Korg "program" is a patch over a multisound, and this app already
//! has the patch half — a SOUND file carries the machine's overrides AND
//! every section of the strip with its IN state and its own overrides.
//! So a factory program here is a `Sound`, an effect chain is part of
//! it, and loading one is the ordinary sound load the browser and the
//! step locks already do. Nothing new had to be invented to give a pad
//! its room; it only had to be filled in.
//!
//! The eight are 1990s intelligent jungle: warm detuned pads, a choir,
//! a Rhodes, bowed air, and three that put the Triton's syn click at
//! the onset of something soft — which is the trick that record after
//! record was built on.
//!
//! Green zone: static data and a builder. `examples/rom_presets.rs`
//! writes them into the library.

use crate::console::SectionKind;
use crate::params::console::{drive, echo, room};
use crate::params::rom as p;
use crate::sound::{Machine, Section, Sound};

/// One factory program: the machine's cells, and the effects that come
/// with it.
#[derive(Debug, Clone, Copy)]
pub struct Pad {
    pub name: &'static str,
    /// What it is for, in a line. Printed by the writer.
    pub note: &'static str,
    /// The two multisamples, BY NAME, so the bank can be reordered
    /// without rewriting eight presets.
    pub pcm: [&'static str; 2],
    pub machine: &'static [(u32, f32)],
    /// The lane sections ROM puts on its FX key. A section named here is
    /// switched IN by the load; one left out stays as the lane has it.
    pub drive: &'static [(u32, f32)],
    pub echo: &'static [(u32, f32)],
    pub room: &'static [(u32, f32)],
}

/// Tape drive, as every one of these wants it: warmth and a lift, not
/// distortion.
const TAPE: f32 = 1.0;
/// A hall rather than a room: the tail is the point.
const HALL: f32 = 1.0;
/// The echo's divisions, in beats: 1/16, 1/8, dotted 1/8, 1/4, 1/2. The
/// dotted eighth is the one this music is made of.
const DOTTED_EIGHTH: f32 = 3.0;
const EIGHTH: f32 = 2.0;
const QUARTER: f32 = 4.0;

pub const PADS: &[Pad] = &[
    Pad {
        name: "Blue Lagoon",
        note: "the bedrock: warm saws under a choir, dotted-eighth echo",
        pcm: ["Warm", "Choir"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, -0.15),
            (p::DETUNE, 9.0),
            (p::XFADE, 0.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 3400.0),
            (p::RESO, 0.18),
            (p::FENV, 0.25),
            (p::FKEY, 0.35),
            (p::FVEL, 0.20),
            (p::FATTACK, 900.0),
            (p::FDECAY, 2500.0),
            (p::FSUSTAIN, 0.55),
            (p::FRELEASE, 1800.0),
            (p::ATTACK, 700.0),
            (p::DECAY, 4000.0),
            (p::SUSTAIN, 0.90),
            (p::RELEASE, 2400.0),
            (p::VEL, 0.45),
            (p::KEYDEC, 0.15),
            (p::LEVEL, 0.85),
            (p::SHAPE, 1.0),
            (p::RATE, 0.18),
            (p::FADE, 1500.0),
            (p::LCUT, 0.12),
            (p::CRATE, 0.35),
            (p::CDEPTH, 0.45),
            (p::CWIDTH, 0.85),
            (p::CMIX, 0.55),
            (p::VINTAGE, 0.30),
            (p::VRATE, 32_000.0),
            (p::VBITS, 12.0),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 14.0),
            (drive::TILT_PRE, 1.0),
            (drive::MIX, 35.0),
        ],
        echo: &[
            (echo::SYNC, DOTTED_EIGHTH),
            (echo::FEEDBACK, 28.0),
            (echo::TONE, 3200.0),
            (echo::WOW, 14.0),
            (echo::MIX, 14.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 32.0),
            (room::SIZE, 78.0),
            (room::DAMP, 38.0),
            (room::MIX, 30.0),
        ],
    },
    Pad {
        name: "Cygnus Glide",
        note: "four-pole and bowed: the pad that moves under a break",
        pcm: ["Warm", "Bow"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, 0.10),
            (p::DETUNE, 16.0),
            (p::TUNE2, -12.0),
            (p::TYPE, 1.0),
            (p::CUTOFF, 1600.0),
            (p::RESO, 0.30),
            (p::FENV, 0.45),
            (p::FKEY, 0.25),
            (p::FATTACK, 1400.0),
            (p::FDECAY, 3200.0),
            (p::FSUSTAIN, 0.40),
            (p::FRELEASE, 2600.0),
            (p::ATTACK, 1100.0),
            (p::SUSTAIN, 0.95),
            (p::RELEASE, 3000.0),
            (p::VEL, 0.30),
            (p::LEVEL, 0.90),
            (p::SHAPE, 1.0),
            (p::RATE, 0.09),
            (p::FADE, 2600.0),
            (p::LCUT, 0.30),
            (p::CRATE, 0.22),
            (p::CDEPTH, 0.55),
            (p::CWIDTH, 0.95),
            (p::CMIX, 0.45),
            (p::VINTAGE, 0.22),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 22.0),
            (drive::TILT_PRE, 1.5),
            (drive::MIX, 45.0),
        ],
        echo: &[
            (echo::SYNC, DOTTED_EIGHTH),
            (echo::FEEDBACK, 38.0),
            (echo::TONE, 2600.0),
            (echo::WOW, 22.0),
            (echo::PINGPONG, 1.0),
            (echo::MIX, 22.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 45.0),
            (room::SIZE, 86.0),
            (room::DAMP, 45.0),
            (room::MIX, 34.0),
        ],
    },
    Pad {
        name: "Tine Rain",
        note: "the Rhodes chord: tines over a choir, eighth-note repeats",
        pcm: ["Tine", "Choir"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, -0.30),
            (p::DETUNE, 5.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 6200.0),
            (p::RESO, 0.10),
            (p::FENV, 0.15),
            (p::FKEY, 0.45),
            (p::FVEL, 0.35),
            (p::FATTACK, 4.0),
            (p::FDECAY, 1200.0),
            (p::FSUSTAIN, 0.45),
            (p::FRELEASE, 900.0),
            (p::ATTACK, 6.0),
            (p::DECAY, 2600.0),
            (p::SUSTAIN, 0.62),
            (p::RELEASE, 1400.0),
            (p::VEL, 0.70),
            (p::KEYDEC, 0.30),
            (p::LEVEL, 0.88),
            (p::CRATE, 0.55),
            (p::CDEPTH, 0.35),
            (p::CWIDTH, 0.70),
            (p::CMIX, 0.40),
            (p::VINTAGE, 0.38),
            (p::VRATE, 30_000.0),
            (p::VBITS, 11.0),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 18.0),
            (drive::MIX, 40.0),
        ],
        echo: &[
            (echo::SYNC, EIGHTH),
            (echo::FEEDBACK, 34.0),
            (echo::TONE, 4200.0),
            (echo::WOW, 10.0),
            (echo::MIX, 20.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 24.0),
            (room::SIZE, 68.0),
            (room::DAMP, 32.0),
            (room::MIX, 26.0),
        ],
    },
    Pad {
        name: "Syn Click Pad",
        note: "the Triton trick: a knock at the onset, the pad behind it",
        pcm: ["Click", "Warm"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, 0.35),
            (p::LOOP1, 0.0),
            (p::DETUNE, 7.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 4200.0),
            (p::RESO, 0.14),
            (p::FENV, 0.30),
            (p::FKEY, 0.30),
            (p::FVEL, 0.25),
            (p::FATTACK, 2.0),
            (p::FDECAY, 1800.0),
            (p::FSUSTAIN, 0.50),
            (p::FRELEASE, 1500.0),
            (p::ATTACK, 0.5),
            (p::DECAY, 3600.0),
            (p::SUSTAIN, 0.88),
            (p::RELEASE, 2000.0),
            (p::VEL, 0.55),
            (p::LEVEL, 0.86),
            (p::CRATE, 0.40),
            (p::CDEPTH, 0.40),
            (p::CWIDTH, 0.80),
            (p::CMIX, 0.45),
            (p::VINTAGE, 0.45),
            (p::VRATE, 32_000.0),
            (p::VBITS, 12.0),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 16.0),
            (drive::TILT_POST, -1.0),
            (drive::MIX, 38.0),
        ],
        echo: &[
            (echo::SYNC, DOTTED_EIGHTH),
            (echo::FEEDBACK, 26.0),
            (echo::TONE, 3600.0),
            (echo::WOW, 12.0),
            (echo::MIX, 16.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 28.0),
            (room::SIZE, 74.0),
            (room::DAMP, 40.0),
            (room::MIX, 28.0),
        ],
    },
    Pad {
        name: "Glass Ceiling",
        note: "struck glass: the click gives the bell its hammer",
        pcm: ["Glass", "Click"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, -0.40),
            (p::LOOP2, 0.0),
            (p::TUNE1, 12.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 9000.0),
            (p::RESO, 0.08),
            (p::FKEY, 0.50),
            (p::FVEL, 0.30),
            (p::ATTACK, 1.0),
            (p::DECAY, 2200.0),
            (p::SUSTAIN, 0.35),
            (p::RELEASE, 1800.0),
            (p::VEL, 0.65),
            (p::KEYDEC, 0.40),
            (p::LEVEL, 0.78),
            (p::SHAPE, 4.0),
            (p::RATE, 6.0),
            (p::LPITCH, 4.0),
            (p::CRATE, 0.80),
            (p::CDEPTH, 0.30),
            (p::CWIDTH, 1.00),
            (p::CMIX, 0.35),
            (p::VINTAGE, 0.35),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 10.0),
            (drive::MIX, 25.0),
        ],
        echo: &[
            (echo::SYNC, DOTTED_EIGHTH),
            (echo::FEEDBACK, 42.0),
            (echo::TONE, 5200.0),
            (echo::WOW, 8.0),
            (echo::PINGPONG, 1.0),
            (echo::MIX, 26.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 18.0),
            (room::SIZE, 90.0),
            (room::DAMP, 28.0),
            (room::MIX, 36.0),
        ],
    },
    Pad {
        name: "Sub Horizon",
        note: "the bottom of the mix: a sub under a filtered pad, no echo",
        pcm: ["Sub", "Warm"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, -0.20),
            (p::TUNE2, -12.0),
            (p::DETUNE, 4.0),
            (p::TYPE, 1.0),
            (p::CUTOFF, 900.0),
            (p::RESO, 0.22),
            (p::FENV, 0.20),
            (p::FATTACK, 600.0),
            (p::FDECAY, 2400.0),
            (p::FSUSTAIN, 0.60),
            (p::FRELEASE, 1400.0),
            (p::ATTACK, 400.0),
            (p::SUSTAIN, 0.95),
            (p::RELEASE, 1800.0),
            (p::VEL, 0.25),
            (p::LEVEL, 0.95),
            (p::CMIX, 0.15),
            (p::VINTAGE, 0.18),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 26.0),
            (drive::TILT_PRE, -1.0),
            (drive::MIX, 50.0),
        ],
        echo: &[],
        room: &[
            (room::ALGO, 0.0),
            (room::PREDELAY, 12.0),
            (room::SIZE, 40.0),
            (room::DAMP, 60.0),
            (room::MIX, 12.0),
        ],
    },
    Pad {
        name: "Vox Nebula",
        note: "a choir breathing through air: the far end of the room",
        pcm: ["Choir", "Air"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, -0.25),
            (p::DETUNE, 12.0),
            (p::KEY2, 0.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 2600.0),
            (p::RESO, 0.16),
            (p::FENV, 0.35),
            (p::FKEY, 0.40),
            (p::FATTACK, 1800.0),
            (p::FDECAY, 3600.0),
            (p::FSUSTAIN, 0.50),
            (p::FRELEASE, 3000.0),
            (p::ATTACK, 1600.0),
            (p::SUSTAIN, 0.92),
            (p::RELEASE, 3400.0),
            (p::VEL, 0.35),
            (p::LEVEL, 0.82),
            (p::SHAPE, 1.0),
            (p::RATE, 0.12),
            (p::FADE, 3000.0),
            (p::LAMP, 0.22),
            (p::CRATE, 0.28),
            (p::CDEPTH, 0.50),
            (p::CWIDTH, 0.90),
            (p::CMIX, 0.50),
            (p::VINTAGE, 0.26),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 12.0),
            (drive::MIX, 30.0),
        ],
        echo: &[
            (echo::SYNC, QUARTER),
            (echo::FEEDBACK, 30.0),
            (echo::TONE, 2400.0),
            (echo::WOW, 26.0),
            (echo::MIX, 18.0),
        ],
        room: &[
            (room::ALGO, HALL),
            (room::PREDELAY, 60.0),
            (room::SIZE, 94.0),
            (room::DAMP, 34.0),
            (room::MIX, 42.0),
        ],
    },
    Pad {
        name: "Amber Click",
        note: "keys with an attack: the knock over an electric piano",
        pcm: ["Click", "Tine"],
        machine: &[
            (p::MODE, 1.0),
            (p::BALANCE, 0.45),
            (p::LOOP1, 0.0),
            (p::TYPE, 0.0),
            (p::CUTOFF, 5200.0),
            (p::RESO, 0.12),
            (p::FENV, 0.20),
            (p::FKEY, 0.45),
            (p::FVEL, 0.40),
            (p::FATTACK, 2.0),
            (p::FDECAY, 900.0),
            (p::FSUSTAIN, 0.40),
            (p::FRELEASE, 700.0),
            (p::ATTACK, 0.5),
            (p::DECAY, 1800.0),
            (p::SUSTAIN, 0.55),
            (p::RELEASE, 1100.0),
            (p::VEL, 0.75),
            (p::KEYDEC, 0.35),
            (p::LEVEL, 0.84),
            (p::CRATE, 0.60),
            (p::CDEPTH, 0.30),
            (p::CWIDTH, 0.65),
            (p::CMIX, 0.30),
            (p::VINTAGE, 0.42),
            (p::VRATE, 30_000.0),
            (p::VBITS, 11.0),
            (p::VLOOP, 0.35),
        ],
        drive: &[
            (drive::CHARACTER, TAPE),
            (drive::DRIVE, 20.0),
            (drive::MIX, 42.0),
        ],
        echo: &[
            (echo::SYNC, EIGHTH),
            (echo::FEEDBACK, 24.0),
            (echo::TONE, 3800.0),
            (echo::WOW, 16.0),
            (echo::MIX, 15.0),
        ],
        room: &[
            (room::ALGO, 0.0),
            (room::PREDELAY, 20.0),
            (room::SIZE, 60.0),
            (room::DAMP, 44.0),
            (room::MIX, 22.0),
        ],
    },
];

/// A multisample's index, by name. A preset names what it wants and the
/// bank answers, so reordering the bank cannot silently repoint a pad.
fn multi(name: &str) -> f32 {
    super::bank::MULTIS
        .iter()
        .position(|multi| multi.name == name)
        .unwrap_or(0) as f32
}

impl Pad {
    /// The sound file this program is: the machine's cells, and the
    /// three effect sections switched IN with their own settings.
    pub fn sound(&self) -> Sound {
        let mut overrides = vec![(p::PCM1, multi(self.pcm[0])), (p::PCM2, multi(self.pcm[1]))];
        overrides.extend_from_slice(self.machine);
        let section = |kind: SectionKind, values: &[(u32, f32)]| Section {
            kind,
            in_: !values.is_empty(),
            overrides: values.to_vec(),
        };
        Sound {
            lane: crate::lane::Lane::Plain.name().to_owned(),
            machine: Some(Machine {
                spectral: None,
                kind: "rom".to_owned(),
                overrides,
                sample: None,
                slices: Vec::new(),
                pads: Vec::new(),
            }),
            sections: vec![
                section(SectionKind::Drive, self.drive),
                section(SectionKind::Echo, self.echo),
                section(SectionKind::Room, self.room),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::rom as p;

    #[test]
    fn there_are_eight_pads_and_three_of_them_click() {
        assert_eq!(PADS.len(), 8);
        let clicking = PADS.iter().filter(|pad| pad.pcm.contains(&"Click")).count();
        assert_eq!(clicking, 3, "the syn click should be mixed into some");
        for pad in PADS {
            assert!(
                crate::sound::valid_name(pad.name),
                "{} is not a file name the library can hold",
                pad.name
            );
            assert!(
                !pad.note.is_empty(),
                "{} says nothing about itself",
                pad.name
            );
        }
    }

    /// A preset names its multisamples and the bank has them: a pad
    /// cannot quietly point at row zero.
    #[test]
    fn every_pad_names_multisamples_the_bank_holds() {
        for pad in PADS {
            for name in pad.pcm {
                assert!(
                    super::super::bank::MULTIS
                        .iter()
                        .any(|multi| multi.name == name),
                    "{} asks for {name}, which the bank lacks",
                    pad.name
                );
            }
        }
    }

    /// Every value a preset writes is a real parameter of the thing it
    /// is writing to, and inside its range — a preset cannot carry a
    /// setting the machine would drop.
    #[test]
    fn every_setting_is_a_real_parameter_in_range() {
        for pad in PADS {
            let sound = pad.sound();
            let machine = sound.machine.as_ref().expect("a pad has a machine");
            for (id, value) in &machine.overrides {
                let def = p::TABLE
                    .iter()
                    .find(|def| def.id == *id)
                    .unwrap_or_else(|| panic!("{}: {id} is not a ROM parameter", pad.name));
                assert!(
                    *value >= def.min && *value <= def.max,
                    "{}: {} is {value}, outside {}..{}",
                    pad.name,
                    def.name,
                    def.min,
                    def.max
                );
            }
            for section in &sound.sections {
                for (id, value) in &section.overrides {
                    let def = section
                        .kind
                        .table()
                        .iter()
                        .find(|def| def.id == *id)
                        .unwrap_or_else(|| {
                            panic!("{}: {id} is not a {:?} parameter", pad.name, section.kind)
                        });
                    assert!(
                        *value >= def.min && *value <= def.max,
                        "{}: {:?} {} is {value}, outside {}..{}",
                        pad.name,
                        section.kind,
                        def.name,
                        def.min,
                        def.max
                    );
                }
            }
        }
    }

    /// THE FEATURE: a preset carries its effects. Every pad switches its
    /// sections IN and brings their settings — an empty section list
    /// means the pad deliberately wants none, and then it is left OUT.
    #[test]
    fn a_pad_carries_its_effects_and_switches_them_in() {
        for pad in PADS {
            let sound = pad.sound();
            let by = |kind: SectionKind| {
                sound
                    .sections
                    .iter()
                    .find(|section| section.kind == kind)
                    .cloned()
                    .unwrap_or_else(|| panic!("{} lost its {kind:?}", pad.name))
            };
            let room = by(SectionKind::Room);
            assert!(room.in_, "{} has no room switched in", pad.name);
            assert!(
                room.overrides
                    .iter()
                    .any(|(id, mix)| *id == room::MIX && *mix > 0.0),
                "{} switched a room in and left it dry",
                pad.name
            );
            let drive = by(SectionKind::Drive);
            assert!(drive.in_, "{} has no colour", pad.name);
            let echo = by(SectionKind::Echo);
            // Sub Horizon is the one that wants no repeats at all.
            assert_eq!(
                echo.in_,
                !pad.echo.is_empty(),
                "{}: the echo's IN state does not match its settings",
                pad.name
            );
        }
    }

    /// EVERY PAD MAKES A SOUND. A preset is a hundred numbers and a
    /// balance; one of them set wrong is a program that loads and says
    /// nothing, which no test of its file would catch.
    #[test]
    fn every_pad_sounds() {
        use crate::audio::rom::{RomParams, RomVoices};
        let bank = std::sync::Arc::new(super::super::bank::Bank::render(48_000));
        for pad in PADS {
            let mut params = RomParams::default();
            let sound = pad.sound();
            for (id, value) in &sound.machine.as_ref().expect("a machine").overrides {
                params.set(*id, *value);
            }
            const BLOCK: usize = 1024;
            let mut voices = RomVoices::new();
            voices.prepare(48_000.0, BLOCK, params, std::sync::Arc::clone(&bank));
            voices.note_on(55, 100, 1);
            // In blocks the graph's size, because that is the contract:
            // the right channel's buffer is one block long.
            let mut peak = 0.0f32;
            let mut right_peak = 0.0f32;
            let mut finite = true;
            for _ in 0..24 {
                let mut out = vec![0.0f32; BLOCK];
                let mut ramp = crate::audio::graph::Ramp::across(1.0, 1.0, out.len());
                voices.render(&mut out, 0, &mut ramp);
                finite &= out.iter().all(|x| x.is_finite());
                peak = out.iter().fold(peak, |top, x| top.max(x.abs()));
                right_peak = voices
                    .right(BLOCK)
                    .iter()
                    .fold(right_peak, |top, x| top.max(x.abs()));
            }
            assert!(finite, "{} left the finite world", pad.name);
            assert!(peak > 0.02, "{} is silent: peak {peak}", pad.name);
            // And the right channel is there too: these are stereo
            // programs, and a pad in one ear only is a mistake.
            assert!(
                right_peak > 0.02,
                "{} is silent on the right: {right_peak}",
                pad.name
            );
        }
    }

    /// And it survives the trip through the file: saved, read back, the
    /// effects are still there and still IN.
    #[test]
    fn the_effects_survive_the_file() {
        let dir = std::env::temp_dir().join(format!("daw-rom-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for pad in PADS {
            let sound = pad.sound();
            let path = crate::sound::save(&dir, pad.name, &sound)
                .unwrap_or_else(|error| panic!("{}: {error}", pad.name));
            let read =
                crate::sound::load(&path).unwrap_or_else(|error| panic!("{}: {error}", pad.name));
            assert_eq!(read, sound, "{} did not survive the file", pad.name);
        }
        // The library lists what was written, under the plain lane.
        let listed = crate::sound::list(&dir);
        assert_eq!(listed.len(), PADS.len());
        assert!(listed.iter().all(|record| record.lane == "plain"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
