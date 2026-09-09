//! What the hands asked for, in words no frame owns.
//!
//! An intent is a SEMANTIC request — "toggle the trig at this tick",
//! "land this sample" — carried from whatever surface the performer
//! touched to the code that changes the Song. It names the act and
//! nothing about the pixels that produced it.
//!
//! **These types live outside every frame on purpose.** They were born
//! inside `ui::redesign`'s panels, which was harmless while there was one
//! frame and would stop being harmless the moment there were two: a second
//! frame would either import the frame it was replacing, or mint its own
//! near-identical vocabulary and let the two drift. Two answers to "what
//! does Delete do here" is the failure this module exists to prevent.
//!
//! The browser panel had already written the principle down before there
//! was anywhere to put it — it "emits semantic intents, which is why this
//! layer has stayed clean". This is that layer, given a home.
//!
//! Nothing here may mention egui, a rectangle, or a keystroke. If a variant
//! needs one of those, it is not an intent.

use std::path::PathBuf;

/// What the sequence surface can ask for.
pub mod sequence {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Intent {
        /// Grow or shrink the containing clip by a signed tick delta.
        ResizeClip {
            delta_ticks: isize,
        },
        Toggle {
            tick: usize,
            default_pitch: crate::pitch::Pitch,
            default_length_ticks: usize,
            default_velocity: u8,
        },
        SetPrimary {
            tick: usize,
            pitch: crate::pitch::Pitch,
            length_ticks: usize,
            velocity: u8,
        },
        Clear {
            tick: usize,
        },
        /// Remove one pitch-addressed note at `tick`.
        RemoveNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
        },
        /// Move every note at `tick` by a signed tick delta (grammar: nudge).
        Nudge {
            tick: usize,
            delta_ticks: isize,
        },
        /// Move one pitch-addressed note at `tick`, leaving its stack
        /// siblings in place (grammar: nudge in a single-note view).
        NudgeNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
            delta_ticks: isize,
        },
        /// Transpose every note at `tick` by a signed chromatic interval.
        Transpose {
            tick: usize,
            delta_semitones: isize,
        },
        /// Transpose one pitch-addressed note at `tick`, leaving its stack
        /// siblings in place.
        TransposeNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
            delta_semitones: isize,
        },
        /// Lengthen or shorten every note at `tick` (grammar: resize).
        Resize {
            tick: usize,
            delta_ticks: isize,
        },
        /// Lengthen or shorten one pitch-addressed note at `tick`.
        ResizeNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
            delta_ticks: isize,
        },
        /// Add one note at `tick` without touching its neighbours
        /// (grammar: put and duplicate land whole trigs one note at a time).
        AddNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
            length_ticks: usize,
            velocity: u8,
            probability: f32,
        },
        /// Add one note while retaining the addressed trig's condition
        /// and locks. Step entry uses this after clearing only the notes
        /// at the address, so replacing a chord does not erase the
        /// performance data already attached to that cell.
        AddEntryNote {
            tick: usize,
            pitch: crate::pitch::Pitch,
            length_ticks: usize,
            velocity: u8,
        },
        /// Set the condition sign on every note at `tick` (grammar: condition).
        SetProbability {
            tick: usize,
            probability: f32,
        },
        /// Set or clear the deterministic A:B cycle condition.
        SetCondition {
            tick: usize,
            cond: Option<(u8, u8)>,
        },
        /// Set or clear the step's compile-time retrigger recipe.
        SetRetrig {
            tick: usize,
            retrig: Option<crate::sequencing::Retrig>,
        },
        /// Adjust the velocity of every note at `tick` by a signed amount
        /// (grammar: hold-the-trig + up/down).
        AdjustVelocity {
            tick: usize,
            delta: isize,
        },
        /// Adjust one pitch-addressed note's velocity.
        AdjustNoteVelocity {
            tick: usize,
            pitch: crate::pitch::Pitch,
            delta: isize,
        },
        /// Set one pitch-addressed note's mute state.
        SetNoteMuted {
            tick: usize,
            pitch: crate::pitch::Pitch,
            muted: bool,
        },
        /// Hold one of the voice's parameters at `value` for the trig
        /// at `tick`. The value is the parameter's engine value, whole.
        SetLock {
            tick: usize,
            /// The device the lock is on, by id; `None` is the voice.
            device: Option<u64>,
            param: u32,
            value: f32,
        },
        /// Release the trig's lock on `param`.
        ClearLock {
            tick: usize,
            device: Option<u64>,
            param: u32,
        },
        /// Mark the trig's lock on `param` as a slide to the next lock,
        /// or back to a plain lock. Nothing to mark is a refusal.
        SetSlide {
            tick: usize,
            device: Option<u64>,
            param: u32,
            slide: bool,
        },
        /// Lay another step's SOUND lock on this one. The register
        /// carries the source's address and the stage resolves it: a
        /// sound is not a value an intent can hold.
        CopySound {
            tick: usize,
            from_pattern: u64,
            from_tick: usize,
        },
    }
}

/// What the chain surface can ask for.
pub mod chain {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Intent {
        SetParam {
            device: u64,
            param: u32,
            value: f32,
        },
        ToggleBypass {
            device: u64,
        },
        ToggleTrackArm,
        CycleTrackMonitor,
        /// Move `device` onto `target`; the shared rack helper decides which
        /// side from their direction in the canonical chain.
        Reorder {
            device: u64,
            target: u64,
        },
        AddDevice {
            catalogue_index: usize,
        },
    }
}

/// What the transport surface can ask for.
pub mod transport {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Intent {
        Return,
        TogglePlay,
        Pause,
        Stop,
        ToggleRecord,
        ToggleEngine,
        ToggleLoop,
        ToggleMetronome,
        ToggleFollow,
        SetTempo(f64),
        CycleBeatUnit,
    }
}

/// What the browser surface can ask for.
pub mod browser {
    use super::PathBuf;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum Intent {
        SelectSample(PathBuf),
        AuditionSample(PathBuf),
        StopAudition,
        /// Put this sound onto the arrangement.
        ///
        /// The browser says WHICH sound and nothing else. It does not decide
        /// what landing means, does not know the track, and never reaches
        /// into the Song — it reads an immutable snapshot and emits semantic
        /// intents, which is why this layer has stayed clean. The app
        /// resolves the target and the meaning from the track's kind.
        LandSample(PathBuf),
    }
}

#[cfg(test)]
mod tests {
    /// The vocabulary must stay frame-independent, which is the entire
    /// reason it was lifted out of `ui::redesign`'s panels. A frame type
    /// reaching in here would re-create the coupling this module was made
    /// to remove — and it would do it quietly, one convenient variant at a
    /// time.
    #[test]
    fn no_frame_or_toolkit_type_reaches_the_vocabulary() {
        let src = include_str!("intent.rs");
        // This test's own body necessarily names the words it forbids, and
        // so does the module doc explaining WHY they are forbidden. Judge
        // the code, not the prose about the code.
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["egui", "eframe", "Rect", "Painter", "Response", "redesign"] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` appeared in the intent vocabulary — an intent \
                 names an ACT, never the surface that produced it"
            );
        }
    }
}
