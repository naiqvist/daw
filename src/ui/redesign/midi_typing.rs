//! Computer-keyboard MIDI entry for the redesign.
//!
//! The mode is explicit because these letter keys normally belong to the
//! rest of the application. `I` transfers ownership to/from MIDI entry.
//! While owned, the letters speak whichever pitch language the focused
//! track's authority declares (`notes/20260831-pitch-lens-spec.md` §5):
//!
//! - CHROMATIC (Absolute tracks): Ableton's one-octave core map enters a
//!   MIDI pitch; `Z`/`X` move the octave.
//! - DEGREE (Degree tracks): the letter rows are degrees 1..N of the
//!   ambient key — bottom row first, top row continuing, wrapping into
//!   the next period when the rows outrun the scale. `Z`/`X` shift by
//!   PERIOD. No wrong notes by construction.

use eframe::egui;

const OCTAVE_MIN: i8 = -1;
const OCTAVE_MAX: i8 = 8;
const DEFAULT_OCTAVE: i8 = 3;
const PERIOD_MIN: i8 = -4;
const PERIOD_MAX: i8 = 4;

const KEY_MAP: [(egui::Key, u8); 12] = [
    (egui::Key::A, 0),
    (egui::Key::W, 1),
    (egui::Key::S, 2),
    (egui::Key::E, 3),
    (egui::Key::D, 4),
    (egui::Key::F, 5),
    (egui::Key::T, 6),
    (egui::Key::G, 7),
    (egui::Key::Y, 8),
    (egui::Key::H, 9),
    (egui::Key::U, 10),
    (egui::Key::J, 11),
];

/// The degree ladder: home row first, then the top row. `I` stays the
/// mode toggle and `O` stays the metronome, so neither is a rung.
const DEGREE_KEYS: [egui::Key; 17] = [
    egui::Key::A,
    egui::Key::S,
    egui::Key::D,
    egui::Key::F,
    egui::Key::G,
    egui::Key::H,
    egui::Key::J,
    egui::Key::K,
    egui::Key::L,
    egui::Key::Q,
    egui::Key::W,
    egui::Key::E,
    egui::Key::R,
    egui::Key::T,
    egui::Key::Y,
    egui::Key::U,
    egui::Key::P,
];

/// Which pitch language the letters speak this frame. Decided by the
/// focused track's authority, passed in read-only — the typing layer
/// never owns harmonic policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryMode {
    Chromatic,
    /// Degrees 1..N of the ambient key.
    Degree {
        degrees: usize,
    },
}

/// One entered pitch, in the language it was spoken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Entered {
    Midi(u8),
    Degree { degree: i32, period: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Status {
    pub(crate) enabled: bool,
    pub(crate) octave: i8,
    pub(crate) period_shift: i8,
    /// True when the letters speak degrees — the plate says which.
    pub(crate) degree_mode: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Update {
    pub(crate) status: Status,
    pub(crate) entered: Option<Entered>,
}

pub(crate) struct MidiTyping {
    enabled: bool,
    octave: i8,
    period_shift: i8,
}

impl Default for MidiTyping {
    fn default() -> Self {
        Self {
            enabled: false,
            octave: DEFAULT_OCTAVE,
            period_shift: 0,
        }
    }
}

impl MidiTyping {
    pub(crate) fn update(&mut self, ctx: &egui::Context, mode: EntryMode) -> Update {
        let toggle = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::I));
        if toggle {
            self.enabled = !self.enabled;
        }
        // Escape drops back to NORMAL: leaving pitch entry is the same
        // reflex as abandoning any other sentence. Only consumed while
        // the mode is on, so Escape keeps its other meanings otherwise.
        if self.enabled
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.enabled = false;
        }

        let mut entered = None;
        if self.enabled {
            let down =
                ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Z));
            let up = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::X));
            match mode {
                EntryMode::Chromatic => {
                    if down {
                        self.octave = (self.octave - 1).max(OCTAVE_MIN);
                    } else if up {
                        self.octave = (self.octave + 1).min(OCTAVE_MAX);
                    }
                    for (key, semitone) in KEY_MAP {
                        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                            entered = Some(Entered::Midi(midi_pitch(self.octave, semitone)));
                            break;
                        }
                    }
                }
                EntryMode::Degree { degrees } => {
                    if down {
                        self.period_shift = (self.period_shift - 1).max(PERIOD_MIN);
                    } else if up {
                        self.period_shift = (self.period_shift + 1).min(PERIOD_MAX);
                    }
                    for (index, key) in DEGREE_KEYS.into_iter().enumerate() {
                        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                            let (degree, period) = degree_at(index, degrees);
                            entered = Some(Entered::Degree {
                                degree,
                                period: period + i32::from(self.period_shift),
                            });
                            break;
                        }
                    }
                }
            }
        }

        Update {
            status: Status {
                enabled: self.enabled,
                octave: self.octave,
                period_shift: self.period_shift,
                degree_mode: matches!(mode, EntryMode::Degree { .. }),
            },
            entered,
        }
    }
}

fn midi_pitch(octave: i8, semitone: u8) -> u8 {
    ((i16::from(octave) + 1) * 12 + i16::from(semitone)).clamp(0, 127) as u8
}

/// The continuous degree ladder under the fingers: key `index` speaks
/// degree `index mod N`, `index / N` periods up — so a seven-note scale
/// wraps into its second period at the eighth key.
fn degree_at(index: usize, degrees: usize) -> (i32, i32) {
    let degrees = degrees.max(1);
    ((index % degrees) as i32, (index / degrees) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ableton_core_map_is_one_chromatic_octave() {
        assert_eq!(KEY_MAP.first(), Some(&(egui::Key::A, 0)));
        assert_eq!(KEY_MAP.last(), Some(&(egui::Key::J, 11)));
        assert!(KEY_MAP.windows(2).all(|pair| pair[1].1 == pair[0].1 + 1));
    }

    #[test]
    fn octave_transposition_is_twelve_semitones() {
        assert_eq!(midi_pitch(3, 0), 48);
        assert_eq!(midi_pitch(4, 0), 60);
        assert_eq!(midi_pitch(4, 11), 71);
    }

    /// The degree ladder wraps into the next period when the rows outrun
    /// the scale — and covers scales wider than one row without wrapping.
    #[test]
    fn the_degree_ladder_wraps_by_scale_size() {
        // Diatonic: eighth key is the tonic one period up.
        assert_eq!(degree_at(0, 7), (0, 0));
        assert_eq!(degree_at(6, 7), (6, 0));
        assert_eq!(degree_at(7, 7), (0, 1));
        assert_eq!(degree_at(16, 7), (2, 2));
        // A 17-degree scale uses every rung before wrapping.
        assert_eq!(degree_at(16, 17), (16, 0));
        // Degenerate scales never divide by zero.
        assert_eq!(degree_at(3, 0), (0, 3));
    }

    /// The two entry maps keep the global keys out of the ladder: `I`
    /// toggles the mode and `O` speaks the metronome, so neither may
    /// ever be a pitch.
    #[test]
    fn reserved_keys_never_enter_pitches() {
        for key in [egui::Key::I, egui::Key::O, egui::Key::Z, egui::Key::X] {
            assert!(!DEGREE_KEYS.contains(&key), "{key:?} is a global key");
            assert!(
                !KEY_MAP.iter().any(|(mapped, _)| *mapped == key),
                "{key:?} is a global key"
            );
        }
    }
}
