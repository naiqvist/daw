//! The action vocabulary: everything a user can ask the app to do.
//!
//! Panels never act — they RETURN wishes from this enum; `main.rs` (the only
//! owner of the Engine) translates them. Buttons, keyboard shortcuts, menus,
//! and any future command palette all speak these same values, so an input
//! method is just a different way to emit an action.
//!
//! Every action carries a `label()`, because anything that can be done must
//! be nameable: that is what lets `keymap` print a shortcut table and a View
//! menu build itself from the panel registry.

use crate::ui::tokens::Density;
use crate::ui::vm::TrackKind;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiAction {
    StartEngine,
    StopEngine,
    /// Space: stop if playing; if stopped, play FROM THE INSERT MARKER —
    /// restarting restarts, it does not resume.
    TogglePlay,
    /// Shift+Space: resume from the stop point, or pause where you are.
    ContinuePlay,
    /// Ctrl+Space: play the arrangement selection and stop at its end.
    PlaySelection,
    /// Halt playback, hold position.
    Pause,
    /// Halt playback AND return to zero — pause and return in one press.
    /// Distinct from `StopEngine`, which shuts the audio device down: this
    /// one is a transport verb, that one is a power switch.
    Stop,
    /// Stop and return to zero.
    Return,
    SetTempo(f64),
    /// Numerator and denominator, e.g. `(3, 4)`.
    SetTimeSignature(u32, u32),
    ToggleMetronome,
    /// Arm or disarm recording. Rolling is `armed && playing`, derived — it
    /// is not a state anyone sets directly.
    ToggleRecord,
    ToggleLoop,
    /// Keep the playhead on screen as it moves.
    ToggleFollow,

    // --- the arrangement's grid ---
    /// Finer divisions: 1/4 becomes 1/8. Ableton calls this Narrow Grid.
    NarrowGrid,
    /// Coarser divisions: 1/8 becomes 1/4.
    WidenGrid,
    /// Turn the current time selection into the loop region, and enable
    /// looping. Ableton's Ctrl+L.
    LoopFromSelection,
    /// Step the arrangement's keyboard cell cursor by this many grid
    /// divisions. Negative is earlier. Collapses the selection to one cell.
    MoveCell(i32),
    /// Move the cursor the same way, but keep the selection's anchor — so
    /// the selection grows or shrinks instead of collapsing. Shift+arrow.
    ExtendCell(i32),
    /// Remove the selected clip. The mouse selects, this disposes.
    DeleteSelected,
    /// The clipboard verbs. Copy stashes the selected clip; paste places it
    /// at the cursor (or selection, or playhead); duplicate places a copy
    /// directly after the original — Ableton's Ctrl+D.
    CopyClip,
    PasteClip,
    DuplicateClip,
    /// Split the clip under the cell cursor at the cursor's beat. Ctrl+E.
    SplitAtCursor,
    /// Merge the MIDI clips under the time selection into one. Ctrl+J.
    Consolidate,
    /// Remove the selected span of time from EVERY track. Ctrl+Shift+Backspace.
    DeleteTime,
    /// Open a copy of the selected span after itself, on EVERY track.
    /// Ctrl+Shift+D.
    DuplicateTime,
    /// Insert empty time: the selection's length at the selection, else one
    /// bar at the cursor. Ctrl+I.
    InsertSilence,
    /// Set a locator at the cursor (or playhead), or remove the one there.
    SetLocator,
    /// Jump the playhead to the neighbouring locator. -1 or 1.
    JumpLocator(i32),
    /// Frame the selected audio clip in the arrangement timeline. Ableton's Z.
    ZoomSelectedAudioClip,
    /// Restore the arrangement view saved by the last clip zoom. Ableton's X.
    ZoomBack,

    /// Flip the main area between the timeline and the clip launcher. Tab.
    ToggleMainView,
    /// Stop every session clip: playback returns to the timeline whole.
    /// Ableton's Back to Arrangement.
    BackToArrangement,
    /// Insert an empty scene below the selected one.
    InsertScene,
    /// Insert a scene holding copies of everything currently playing.
    CaptureScene,

    // --- history ---
    /// Step the arrangement back to before the last edit. Ctrl+Z.
    Undo,
    /// Step forward again through an undone edit. Ctrl+Shift+Z, Ctrl+Y.
    Redo,

    // --- tracks ---
    /// Move the selected track this many places in the stack, clamped at
    /// the ends. The keyboard's route to what a header drag does.
    MoveTrack(i32),
    /// Append a track of this kind and select it. Ctrl+T / Ctrl+Shift+T.
    AddTrack(TrackKind),
    /// Drop the selected track, its clips with it. The last track is never
    /// removed: an arrangement with no lanes has nothing to click.
    RemoveTrack,
    /// Mute or solo the selected track. Both are graph SHAPE — a muted
    /// track leaves the schedule — so they are actions, not knob turns.
    ToggleTrackMute,
    ToggleTrackSolo,
    /// Step the selected track's pan by this much, in `-1..=1` units.
    /// Negative is left. The header knob writes pan directly; this is how
    /// the keyboard reaches it.
    NudgeTrackPan(f32),
    /// Return the selected track's pan to center.
    CenterTrackPan,

    // --- view: the app's own furniture, no engine involved ---
    /// Show/hide a registered panel, by its `Panel::id()`.
    TogglePanel(&'static str),
    /// Bring a center-dock panel to the front of its tab strip.
    FocusPanel(&'static str),
    SetDensity(Density),
}

impl UiAction {
    /// Human name, for menus, tooltips, and the shortcut table.
    pub fn label(self) -> &'static str {
        match self {
            Self::StartEngine => "Start engine",
            Self::StopEngine => "Stop engine",
            Self::TogglePlay => "Play / stop",
            Self::ContinuePlay => "Continue play",
            Self::PlaySelection => "Play selection",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::Return => "Return to zero",
            Self::SetTempo(_) => "Set tempo",
            Self::SetTimeSignature(..) => "Time signature",
            Self::ToggleMetronome => "Metronome",
            Self::ToggleRecord => "Record",
            Self::ToggleLoop => "Loop",
            Self::ToggleFollow => "Follow",
            Self::NarrowGrid => "Narrow grid",
            Self::WidenGrid => "Widen grid",
            Self::LoopFromSelection => "Loop selection",
            Self::MoveCell(_) => "Move cursor",
            Self::ExtendCell(_) => "Extend selection",
            Self::DeleteSelected => "Delete selected clip",
            Self::CopyClip => "Copy clip",
            Self::PasteClip => "Paste clip",
            Self::DuplicateClip => "Duplicate clip",
            Self::SplitAtCursor => "Split clip",
            Self::Consolidate => "Consolidate",
            Self::DeleteTime => "Delete time",
            Self::DuplicateTime => "Duplicate time",
            Self::InsertSilence => "Insert silence",
            Self::SetLocator => "Set / delete locator",
            Self::JumpLocator(_) => "Jump to locator",
            Self::ZoomSelectedAudioClip => "Zoom to selected audio clip",
            Self::ZoomBack => "Zoom back",
            Self::ToggleMainView => "Timeline / Session",
            Self::BackToArrangement => "Back to arrangement",
            Self::InsertScene => "Insert scene",
            Self::CaptureScene => "Capture and insert scene",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::MoveTrack(_) => "Move track",
            Self::AddTrack(kind) => match kind {
                TrackKind::Midi => "New MIDI track",
                TrackKind::Audio => "New audio track",
            },
            Self::RemoveTrack => "Delete track",
            Self::ToggleTrackMute => "Mute track",
            Self::ToggleTrackSolo => "Solo track",
            Self::NudgeTrackPan(_) => "Pan track",
            Self::CenterTrackPan => "Center pan",
            Self::TogglePanel(id) => id,
            Self::FocusPanel(id) => id,
            Self::SetDensity(_) => "Density",
        }
    }

    /// Does performing this action require a running engine? The keymap uses
    /// it to stay quiet when the engine is off, and panels use it to grey
    /// controls out — one answer, in one place, instead of both guessing.
    pub fn needs_engine(self) -> bool {
        match self {
            Self::TogglePlay
            | Self::ContinuePlay
            | Self::PlaySelection
            | Self::Pause
            | Self::Stop
            | Self::Return
            | Self::SetTempo(_)
            | Self::SetTimeSignature(..)
            | Self::ToggleMetronome
            | Self::ToggleRecord
            | Self::ToggleLoop
            | Self::StopEngine => true,
            Self::StartEngine
            | Self::ToggleFollow
            | Self::NarrowGrid
            | Self::WidenGrid
            | Self::LoopFromSelection
            | Self::MoveCell(_)
            | Self::ExtendCell(_)
            | Self::DeleteSelected
            | Self::CopyClip
            | Self::PasteClip
            | Self::DuplicateClip
            | Self::SplitAtCursor
            | Self::Consolidate
            | Self::DeleteTime
            | Self::DuplicateTime
            | Self::InsertSilence
            | Self::SetLocator
            | Self::JumpLocator(_)
            | Self::ZoomSelectedAudioClip
            | Self::ZoomBack
            | Self::ToggleMainView
            | Self::BackToArrangement
            | Self::InsertScene
            | Self::CaptureScene
            | Self::Undo
            | Self::Redo
            | Self::MoveTrack(_)
            | Self::AddTrack(_)
            | Self::RemoveTrack
            | Self::ToggleTrackMute
            | Self::ToggleTrackSolo
            | Self::NudgeTrackPan(_)
            | Self::CenterTrackPan
            | Self::TogglePanel(_)
            | Self::FocusPanel(_)
            | Self::SetDensity(_) => false,
        }
    }
}
