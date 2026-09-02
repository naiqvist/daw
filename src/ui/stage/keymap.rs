//! The stage codebook: scope-conditioned keys translated into intents.
//!
//! Scope is part of every binding even while the calibration grid gives
//! every scope the same vocabulary. That keeps the dispatch shape honest for
//! the first real surface, where the same key may mean different things at
//! different depths.

use super::Step;
use eframe::egui::{Key, Modifiers};

/// The conditioning context in which a key is interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScopeContext {
    Root,
    Nested,
    /// Focus is in the browser. A different place, not a deeper one —
    /// which is why it is a context of its own rather than a stack level.
    Browser,
    /// The mixer is up: the lattice's rows are channels rather than
    /// scenes. A different CONTENT under the same cursor, so the same
    /// keys mean different things — which is the whole reason scope is
    /// part of every binding.
    Mixer,
    /// Focus is in the chain band: a track's devices, and every parameter
    /// each one has. A place beside the session, like the browser, rather
    /// than a level inside it.
    Chain,
    /// Focus is inside a clip: the sequencer is drawn in the field and its
    /// grammar owns the keyboard. The stage keeps only what is global —
    /// time, the codebook, the browser, making tracks — and the one way
    /// out. Arrows, Enter and the verbs are the sequencer's to consume.
    Clip,
    /// A track is being renamed: the letters are its name. The smallest
    /// vocabulary on the stage — commit, abandon, erase — because while a
    /// name is being typed every other key IS a letter, and a chord that
    /// fired mid-word would be a trap.
    Rename,
    /// The trig menu is up: a callout over the sequencer, pointing at
    /// the trig under the cursor, holding the verbs that trig answers
    /// to. A list, so Up and Down walk it, Enter speaks the row and
    /// Escape puts it away; the sequencer's own grammar waits underneath
    /// until it is gone. Time and the ground stay global, as everywhere.
    TrigMenu,
    /// The sample editor is up: one file, full screen, and the keys are
    /// the editor's — cursor, zoom, markers, slices, audition. A place
    /// of its own, like the browser, that Escape leaves.
    Sample,
    /// The song view: the arrangement's lanes, the cursor on a cell or
    /// a block. The tray beneath shows the block's pattern; the verbs
    /// here move, size, and lay blocks.
    Song,
}

impl ScopeContext {
    #[cfg(test)]
    pub(super) const ALL: [Self; 10] = [
        Self::Root,
        Self::Nested,
        Self::Browser,
        Self::Mixer,
        Self::Chain,
        Self::Clip,
        Self::Rename,
        Self::TrigMenu,
        Self::Sample,
        Self::Song,
    ];
}

/// A semantic request to the stage state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageIntent {
    Step(Step),
    Enter,
    Escape,
    /// Toggle the song clock between parked and rolling.
    ToggleTransport,
    /// Return the song clock to the top without changing its motion.
    Rewind,
    /// Show or hide the codebook for the current scope.
    Help,
    /// Summon the browser, or dismiss it. Focus goes with it.
    Browse,
    /// Add one printable character to the browser's filter.
    TypeChar(char),
    /// Remove one character from the browser's filter.
    Backspace,
    /// Append an audio track to the song.
    NewAudioTrack,
    /// Append an instrument track to the song — a MIDI track, in the
    /// words the chord is described in.
    NewInstrumentTrack,
    /// Empty the session slot the cursor stands on.
    Clear,
    /// Fire the scene the cursor stands in, or stop it if it is already
    /// the one playing. What a scene HOLDS is document data; which one is
    /// playing is a fact about a performance, so this changes no document.
    Launch,
    /// Swap what hangs beneath the track heads: the scenes, or the mixer.
    /// One lattice, two contents — the columns never move, so the eye
    /// keeps its place across the change.
    Mix,
    /// Move the focused channel's fader. `fine` is the tenth-decibel
    /// press: the same verb, told how far.
    Gain {
        louder: bool,
        fine: bool,
    },
    /// Move the focused channel's pan one press.
    Pan {
        right: bool,
    },
    /// Silence this track, or stop silencing it.
    Mute,
    /// Make this track the one that sounds, or stop.
    Solo,
    /// Fire every clip in the scene the cursor is in, as a row.
    LaunchScene,
    /// Show or hide the chain band: the addressed track's devices, and
    /// every parameter each one has.
    Devices,
    /// Move the parameter under the cursor. `coarse` is the tenth-of-range
    /// press, for crossing a span rather than settling in it.
    Param {
        up: bool,
        coarse: bool,
    },
    /// Turn the ground over: dark to light and back. A viewing condition
    /// — daylight, a bright room, an unfamiliar display — rather than a
    /// preference about how the app should look.
    Ground,
    /// Step the document back one edit, or forward again.
    Undo,
    Redo,
    /// Write the song to its file.
    Save,
    /// Begin renaming the track the cursor is in. The letters are the
    /// name until Enter keeps it or Escape lets it go.
    Rename,
    /// Take the track the cursor is in out of the song, with its slots.
    DeleteTrack,
    /// Arm a move: the next Left or Right shifts the thing under the
    /// cursor — a device along its chain, a track along the strip — one
    /// place that way. The grammar's NUDGE, spoken the same way here.
    Nudge,
    /// Lift the device under the cursor off its chain and keep it.
    Yank,
    /// Put the kept device down: after the cursor's device in the band,
    /// or at the end of the cursor's track from the session.
    Put,
    /// Summon the trig menu over the trig under the cursor.
    TrigMenu,
    /// Release the parameter lock under the trig menu's cursor.
    ClearLock,
    /// An act in the sample editor, or the act of opening it.
    Sample(SampleIntent),
    /// Scan the library's folders again, so a pack dropped in while the
    /// stage runs turns up without a restart.
    Rescan,
    /// Turn the field over: the session, or the song's arrangement.
    SongView,
    /// The cursor's track to the next group bus.
    Bus,
    /// The song view's own verbs, over the block or cell under the cursor.
    Song(SongIntent),
    /// Arm the arrangement: while the session plays, every launch
    /// writes a block into the song.
    RecordSong,
}

/// What the song view does beyond the cursor's walk and the verbs it
/// shares with the session (Enter, Delete, W, Q, E).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SongIntent {
    /// Fewer bars across: a closer look.
    ZoomIn,
    /// More bars across.
    ZoomOut,
    /// Hold: Left and Right resize the block under the cursor by a
    /// cell until Escape.
    Resize,
    /// The block under the cursor a whole bar longer or shorter.
    Stretch(Step),
    /// The block under the cursor again, right after itself.
    Duplicate,
    /// The block's pattern: the next one its track holds in the session.
    Pick,
    /// The previous one.
    PickBack,
    /// The cursor to the previous block edge on its track.
    JumpPrev,
    /// The next.
    JumpNext,
    /// The loop brace's start at the cursor.
    BraceStart,
    /// The loop brace's end at the cursor's cell.
    BraceEnd,
    /// The brace on or off.
    ToggleLoop,
    /// A locator at the cursor, or the one there taken away.
    Marker,
    /// Render the brace, or the whole song, to a wav.
    Export,
}

impl SongIntent {
    pub fn label(self) -> &'static str {
        match self {
            Self::ZoomIn => "zoom in",
            Self::ZoomOut => "zoom out",
            Self::Resize => "resize block",
            Self::Stretch(Step::Left) => "a bar shorter",
            Self::Stretch(_) => "a bar longer",
            Self::Duplicate => "duplicate block",
            Self::Pick => "next pattern",
            Self::PickBack => "previous pattern",
            Self::JumpPrev => "previous edge",
            Self::JumpNext => "next edge",
            Self::BraceStart => "loop start here",
            Self::BraceEnd => "loop end here",
            Self::ToggleLoop => "loop on / off",
            Self::Marker => "marker",
            Self::Export => "export wav",
        }
    }
}

/// What the sample editor can be told. Its own enum, so the editor's
/// vocabulary is one thing to read and one thing to bind, and so the
/// stage's own verbs are not diluted by two dozen that only mean
/// anything over a waveform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleIntent {
    /// Open the editor on the sampler under the cursor.
    Open,
    Left {
        coarse: bool,
    },
    Right {
        coarse: bool,
    },
    /// Jump to the previous or next marker: trim, loop, slice.
    JumpPrev,
    JumpNext,
    ZoomIn,
    ZoomOut,
    ScrollLeft,
    ScrollRight,
    /// The next page: TRIM, SLICE, ATTR, round.
    Page,
    SetStart,
    SetEnd,
    SetLoop,
    AddSlice,
    RemoveSlice,
    /// Lay a grid of `count` equal slices.
    Grid,
    /// Detect onsets at the sensitivity and slice on them.
    Transients,
    ClearSlices,
    /// More slices for the grid, or more gain on the ATTR page.
    More,
    Less,
    /// A more eager, or a shyer, onset detector.
    Eager,
    Shyer,
    Normalize,
    Reverse,
    Mode,
    LoopMode,
    /// Toggle zero-crossing snap for placed markers.
    Snap,
    /// Play the slice or the trim under the cursor.
    Audition,
    AuditionAll,
}

impl SampleIntent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "sample editor",
            Self::Left { coarse: false } => "cursor left",
            Self::Left { coarse: true } => "cursor left, coarsely",
            Self::Right { coarse: false } => "cursor right",
            Self::Right { coarse: true } => "cursor right, coarsely",
            Self::JumpPrev => "previous marker",
            Self::JumpNext => "next marker",
            Self::ZoomIn => "zoom in",
            Self::ZoomOut => "zoom out",
            Self::ScrollLeft => "scroll left",
            Self::ScrollRight => "scroll right",
            Self::Page => "next page",
            Self::SetStart => "start here",
            Self::SetEnd => "end here",
            Self::SetLoop => "loop from here",
            Self::AddSlice => "slice here",
            Self::RemoveSlice => "remove slice",
            Self::Grid => "grid slices",
            Self::Transients => "slice on onsets",
            Self::ClearSlices => "clear slices",
            Self::More => "more",
            Self::Less => "less",
            Self::Eager => "eager onsets",
            Self::Shyer => "shy onsets",
            Self::Normalize => "normalize",
            Self::Reverse => "reverse",
            Self::Mode => "play mode",
            Self::LoopMode => "loop mode",
            Self::Snap => "zero snap",
            Self::Audition => "audition",
            Self::AuditionAll => "audition all",
        }
    }
}

impl StageIntent {
    /// What this intent is called on the help surface.
    ///
    /// A `match` rather than a lookup table on purpose: a new intent that
    /// forgets its name is a COMPILE ERROR, which is the only way the
    /// codebook stays unable to lie about itself.
    pub fn label(self) -> &'static str {
        match self {
            Self::Step(Step::Up) => "move up",
            Self::Step(Step::Down) => "move down",
            Self::Step(Step::Left) => "move left",
            Self::Step(Step::Right) => "move right",
            Self::Enter => "go in",
            Self::Escape => "go out",
            Self::ToggleTransport => "stop / roll",
            Self::Rewind => "return to top",
            Self::Help => "this list",
            Self::Browse => "browse",
            Self::TypeChar(_) => "type to filter",
            Self::Backspace => "erase filter",
            Self::NewAudioTrack => "new audio track",
            Self::NewInstrumentTrack => "new midi track",
            Self::Clear => "clear slot",
            Self::Launch => "launch clip",
            Self::LaunchScene => "launch scene",
            Self::Mix => "scenes / mixer",
            Self::Gain {
                louder: true,
                fine: false,
            } => "louder",
            Self::Gain {
                louder: false,
                fine: false,
            } => "quieter",
            Self::Gain {
                louder: true,
                fine: true,
            } => "louder, finely",
            Self::Gain {
                louder: false,
                fine: true,
            } => "quieter, finely",
            Self::Pan { right: false } => "pan left",
            Self::Pan { right: true } => "pan right",
            Self::Mute => "mute",
            Self::Solo => "solo",
            Self::Devices => "devices",
            Self::Param {
                up: true,
                coarse: false,
            } => "more",
            Self::Param {
                up: false,
                coarse: false,
            } => "less",
            Self::Param {
                up: true,
                coarse: true,
            } => "more, coarsely",
            Self::Param {
                up: false,
                coarse: true,
            } => "less, coarsely",
            Self::Ground => "dark / light",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Save => "save",
            Self::Rename => "rename track",
            Self::DeleteTrack => "delete track",
            Self::Nudge => "nudge, then a direction",
            Self::Yank => "yank device",
            Self::Put => "put device",
            Self::TrigMenu => "trig menu",
            Self::ClearLock => "clear lock",
            Self::Sample(intent) => intent.label(),
            Self::Rescan => "rescan library",
            Self::SongView => "session / song",
            Self::Bus => "next bus",
            Self::Song(intent) => intent.label(),
            Self::RecordSong => "record session into song",
        }
    }
}

/// A physical chord or text emitted by egui. Both travel through the same
/// scope-conditioned dispatcher before either can become an intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StageInput {
    Chord(Modifiers, Key),
    Text(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Binding {
    scope: ScopeContext,
    modifiers: Modifiers,
    key: Key,
    intent: StageIntent,
}

impl Binding {
    /// An unmodified key. The short code, spent on what is done often.
    const fn new(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::NONE,
            key,
            intent,
        }
    }

    /// A held modifier is a longer code, for what is done less often —
    /// and for what must not fire by accident under the fingers.
    const fn command(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::COMMAND,
            key,
            intent,
        }
    }

    /// A held shift alone: the same key, told to mean the other thing —
    /// used where the shifted verb is the SIBLING of the unshifted one
    /// rather than a rarer verb that merely needed a longer code.
    const fn shift(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::SHIFT,
            key,
            intent,
        }
    }

    /// Command with shift: a longer code again, and the pair of them puts
    /// the two track kinds one modifier apart — the same verb, told which
    /// kind to make.
    const fn command_shift(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::COMMAND.plus(Modifiers::SHIFT),
            key,
            intent,
        }
    }
}

/// The single source of truth for the stage keyboard vocabulary.
const BINDINGS: &[Binding] = &[
    // Time is global rather than conditioned by where focus stands. It is
    // still repeated as table data for every scope: the dispatcher has no
    // hidden universal-key path, and the codebook can therefore report the
    // exact vocabulary available from wherever the cursor is.
    Binding::new(ScopeContext::Root, Key::Space, StageIntent::ToggleTransport),
    Binding::new(ScopeContext::Root, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Nested,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Nested, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Browser,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Browser, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Root, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Root, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Nested, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Nested, Key::Escape, StageIntent::Escape),
    // Enter goes INTO the thing under the cursor; shift-Enter fires it.
    // The same key for the same object, told which of the two things one
    // does with a clip is meant.
    Binding::shift(ScopeContext::Root, Key::Enter, StageIntent::Launch),
    Binding::shift(ScopeContext::Nested, Key::Enter, StageIntent::Launch),
    // The row, one modifier further out: the same verb told how much of
    // the session to fire, the way the two track kinds differ by a
    // modifier rather than by a second word.
    Binding::command_shift(ScopeContext::Root, Key::Enter, StageIntent::LaunchScene),
    Binding::command_shift(ScopeContext::Nested, Key::Enter, StageIntent::LaunchScene),
    Binding::command(ScopeContext::Root, Key::M, StageIntent::Mix),
    Binding::command(ScopeContext::Nested, Key::M, StageIntent::Mix),
    // The mixer's own scope. Up and down are the fader because the
    // cursor has nowhere else to go on that axis here — a channel is
    // addressed as ONE thing, so the vertical axis is free for the one
    // control the strip is mostly made of.
    Binding::new(
        ScopeContext::Mixer,
        Key::ArrowUp,
        StageIntent::Gain {
            louder: true,
            fine: false,
        },
    ),
    Binding::new(
        ScopeContext::Mixer,
        Key::ArrowDown,
        StageIntent::Gain {
            louder: false,
            fine: false,
        },
    ),
    Binding::shift(
        ScopeContext::Mixer,
        Key::ArrowUp,
        StageIntent::Gain {
            louder: true,
            fine: true,
        },
    ),
    Binding::shift(
        ScopeContext::Mixer,
        Key::ArrowDown,
        StageIntent::Gain {
            louder: false,
            fine: true,
        },
    ),
    // Left and right still walk the channels — the strip is a row of
    // them, and moving along it is what that axis means everywhere else
    // on this surface too.
    Binding::new(
        ScopeContext::Mixer,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Mixer,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    // Pan is the same axis with a modifier: the control is horizontal,
    // the gesture is horizontal, and the modifier says it is the pan
    // rather than the neighbour.
    Binding::shift(
        ScopeContext::Mixer,
        Key::ArrowLeft,
        StageIntent::Pan { right: false },
    ),
    Binding::shift(
        ScopeContext::Mixer,
        Key::ArrowRight,
        StageIntent::Pan { right: true },
    ),
    // The two switches, on their own initials, unmodified: they are
    // pressed often and they undo themselves, so they get the short code.
    Binding::new(ScopeContext::Mixer, Key::M, StageIntent::Mute),
    Binding::new(ScopeContext::Mixer, Key::S, StageIntent::Solo),
    Binding::new(ScopeContext::Mixer, Key::B, StageIntent::Bus),
    // Time and the codebook are global, and repeated here as table data
    // for the reason the top of this list gives: there is no hidden
    // universal-key path, so a scope's vocabulary is exactly its rows.
    Binding::new(
        ScopeContext::Mixer,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Mixer, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Mixer, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Mixer, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Mixer, Key::T, StageIntent::NewAudioTrack),
    Binding::command_shift(ScopeContext::Mixer, Key::T, StageIntent::NewInstrumentTrack),
    Binding::command(ScopeContext::Mixer, Key::M, StageIntent::Mix),
    // Escape has one meaning everywhere: leave the outermost thing. Here
    // the outermost thing is the mixer, and leaving it gives the scenes
    // back.
    Binding::new(ScopeContext::Mixer, Key::Escape, StageIntent::Escape),
    // The chain band, reachable from wherever the strip is pointing.
    Binding::command(ScopeContext::Root, Key::D, StageIntent::Devices),
    Binding::command(ScopeContext::Nested, Key::D, StageIntent::Devices),
    Binding::command(ScopeContext::Mixer, Key::D, StageIntent::Devices),
    Binding::command(ScopeContext::Chain, Key::D, StageIntent::Devices),
    // Inside the band: bare arrows walk it — across the devices, down the
    // parameters — and a shifted arrow moves the value under the cursor,
    // the same gesture the mixer's pan already answers to.
    Binding::new(
        ScopeContext::Chain,
        Key::ArrowLeft,
        StageIntent::Param {
            up: false,
            coarse: false,
        },
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::ArrowRight,
        StageIntent::Param {
            up: true,
            coarse: false,
        },
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::shift(
        ScopeContext::Chain,
        Key::ArrowRight,
        StageIntent::Param {
            up: true,
            coarse: true,
        },
    ),
    Binding::shift(
        ScopeContext::Chain,
        Key::ArrowLeft,
        StageIntent::Param {
            up: false,
            coarse: true,
        },
    ),
    Binding::shift(
        ScopeContext::Chain,
        Key::ArrowUp,
        StageIntent::Param {
            up: true,
            coarse: true,
        },
    ),
    Binding::shift(
        ScopeContext::Chain,
        Key::ArrowDown,
        StageIntent::Param {
            up: false,
            coarse: true,
        },
    ),
    // Mute is the word this app already uses for silencing a thing, and a
    // bypassed device is a silenced device that keeps its place.
    Binding::new(ScopeContext::Chain, Key::M, StageIntent::Mute),
    Binding::new(ScopeContext::Chain, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Chain, Key::Backspace, StageIntent::Clear),
    Binding::new(ScopeContext::Chain, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Chain,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Chain, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Chain, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Chain, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Chain, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Root, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Nested, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Mixer, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Browser, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Clip, Key::L, StageIntent::Ground),
    Binding::new(ScopeContext::Root, Key::Questionmark, StageIntent::Help),
    Binding::new(ScopeContext::Nested, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Root, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Nested, Key::F, StageIntent::Browse),
    // Making a track is one verb told which kind to make, so the two
    // chords differ by exactly the modifier that distinguishes them.
    Binding::command(ScopeContext::Root, Key::T, StageIntent::NewAudioTrack),
    Binding::command(ScopeContext::Nested, Key::T, StageIntent::NewAudioTrack),
    Binding::command_shift(ScopeContext::Root, Key::T, StageIntent::NewInstrumentTrack),
    Binding::command_shift(
        ScopeContext::Nested,
        Key::T,
        StageIntent::NewInstrumentTrack,
    ),
    // Clearing a slot is the one destructive verb on the session, and it
    // is unmodified on purpose: it destroys a PLACE-holder, not content —
    // the pattern stays in the song — so it may sit under the fingers.
    // Both erase keys, because a performer reaches for whichever their
    // hands know, and the two never mean different things here.
    Binding::new(ScopeContext::Root, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Root, Key::Backspace, StageIntent::Clear),
    Binding::new(ScopeContext::Nested, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Nested, Key::Backspace, StageIntent::Clear),
    // Inside the browser the vocabulary is small and honest: move, open,
    // close, leave, erase, ask. Text is the pattern binding handled by
    // `dispatch` below because its payload is data rather than one
    // enumerated key.
    //
    // The library is a TREE, so it has a horizontal axis: right opens a
    // heading and then walks into it, left closes one and then climbs out.
    // Same two keys, same two meanings, as everywhere else on the stage.
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Browser, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Browser, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Browser,
        Key::Backspace,
        StageIntent::Backspace,
    ),
    Binding::new(ScopeContext::Browser, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Browser, Key::F, StageIntent::Browse),
    // Inside a clip the stage says almost nothing: the sequencer's grammar
    // is the vocabulary, and every key not listed here reaches it.
    Binding::new(ScopeContext::Clip, Key::Space, StageIntent::ToggleTransport),
    Binding::new(ScopeContext::Clip, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Clip, Key::Escape, StageIntent::Escape),
    Binding::new(ScopeContext::Clip, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Clip, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Clip, Key::T, StageIntent::NewAudioTrack),
    Binding::command_shift(ScopeContext::Clip, Key::T, StageIntent::NewInstrumentTrack),
    // The document's own verbs, reachable from wherever the cursor is:
    // an edit made from the band or the browser is undone from there
    // too, and a song is saved from wherever the performer happens to be
    // standing when they think of it.
    Binding::command(ScopeContext::Root, Key::Z, StageIntent::Undo),
    Binding::command(ScopeContext::Nested, Key::Z, StageIntent::Undo),
    Binding::command(ScopeContext::Mixer, Key::Z, StageIntent::Undo),
    Binding::command(ScopeContext::Chain, Key::Z, StageIntent::Undo),
    Binding::command(ScopeContext::Browser, Key::Z, StageIntent::Undo),
    Binding::command(ScopeContext::Clip, Key::Z, StageIntent::Undo),
    Binding::command_shift(ScopeContext::Root, Key::Z, StageIntent::Redo),
    Binding::command_shift(ScopeContext::Nested, Key::Z, StageIntent::Redo),
    Binding::command_shift(ScopeContext::Mixer, Key::Z, StageIntent::Redo),
    Binding::command_shift(ScopeContext::Chain, Key::Z, StageIntent::Redo),
    Binding::command_shift(ScopeContext::Browser, Key::Z, StageIntent::Redo),
    Binding::command_shift(ScopeContext::Clip, Key::Z, StageIntent::Redo),
    Binding::command(ScopeContext::Root, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Nested, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Mixer, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Chain, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Browser, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Clip, Key::S, StageIntent::Save),
    // The track under the cursor: its name, its place, its existence.
    // Rename is on the key the grammar already spends on it; deleting a
    // track is the one verb here that destroys content, so it takes the
    // modifier — the same key that clears a slot, told to mean more.
    Binding::new(ScopeContext::Root, Key::F2, StageIntent::Rename),
    Binding::new(ScopeContext::Nested, Key::F2, StageIntent::Rename),
    Binding::new(ScopeContext::Mixer, Key::F2, StageIntent::Rename),
    Binding::command(ScopeContext::Root, Key::Delete, StageIntent::DeleteTrack),
    Binding::command(ScopeContext::Nested, Key::Delete, StageIntent::DeleteTrack),
    Binding::command(ScopeContext::Mixer, Key::Delete, StageIntent::DeleteTrack),
    Binding::command(ScopeContext::Root, Key::Backspace, StageIntent::DeleteTrack),
    Binding::command(
        ScopeContext::Nested,
        Key::Backspace,
        StageIntent::DeleteTrack,
    ),
    Binding::command(
        ScopeContext::Mixer,
        Key::Backspace,
        StageIntent::DeleteTrack,
    ),
    // The grammar's move-and-copy cluster, Q/W/E under the left hand, on
    // the things this surface holds: a device in the band, a track on
    // the strip. Nudge takes a direction, as it does in the sequencer.
    // Put is reachable from the session as well as the band, because a
    // track with no devices has no band to open — and putting a device
    // on it is how it gets one.
    Binding::new(ScopeContext::Root, Key::W, StageIntent::Nudge),
    Binding::new(ScopeContext::Nested, Key::W, StageIntent::Nudge),
    Binding::new(ScopeContext::Mixer, Key::W, StageIntent::Nudge),
    Binding::new(ScopeContext::Chain, Key::W, StageIntent::Nudge),
    Binding::new(ScopeContext::Chain, Key::Q, StageIntent::Yank),
    Binding::new(ScopeContext::Chain, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Root, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Nested, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Mixer, Key::E, StageIntent::Put),
    // Renaming: keep, let go, erase. Everything else is a letter, and
    // reaches the name as text rather than as a chord.
    Binding::new(ScopeContext::Rename, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Rename, Key::Escape, StageIntent::Escape),
    Binding::new(ScopeContext::Rename, Key::Backspace, StageIntent::Backspace),
    Binding::command(ScopeContext::Rename, Key::L, StageIntent::Ground),
    // The trig menu. Summoned from inside a clip on the heaviest form of
    // the key that already means "act on this": Enter toggles the trig,
    // and Enter with both hands down asks what ELSE the trig can do.
    // Once up, it is a list and takes the list's keys, nothing more.
    Binding::command_shift(ScopeContext::Clip, Key::Enter, StageIntent::TrigMenu),
    Binding::new(
        ScopeContext::TrigMenu,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::TrigMenu,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(ScopeContext::TrigMenu, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::TrigMenu, Key::Escape, StageIntent::Escape),
    // The sliders: Left and Right move the lock under the cursor by the
    // parameter's own step, shifted for the coarse one — the same
    // words the band uses for a knob, because a lock IS that knob for
    // one trig. Delete lets the lock go.
    Binding::new(
        ScopeContext::TrigMenu,
        Key::ArrowLeft,
        StageIntent::Param {
            up: false,
            coarse: false,
        },
    ),
    Binding::new(
        ScopeContext::TrigMenu,
        Key::ArrowRight,
        StageIntent::Param {
            up: true,
            coarse: false,
        },
    ),
    Binding::shift(
        ScopeContext::TrigMenu,
        Key::ArrowLeft,
        StageIntent::Param {
            up: false,
            coarse: true,
        },
    ),
    Binding::shift(
        ScopeContext::TrigMenu,
        Key::ArrowRight,
        StageIntent::Param {
            up: true,
            coarse: true,
        },
    ),
    Binding::new(ScopeContext::TrigMenu, Key::Delete, StageIntent::ClearLock),
    Binding::new(
        ScopeContext::TrigMenu,
        Key::Backspace,
        StageIntent::ClearLock,
    ),
    Binding::new(
        ScopeContext::TrigMenu,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::TrigMenu, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::TrigMenu, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::TrigMenu, Key::L, StageIntent::Ground),
    // The cutting room. Opened with ^E wherever the cursor addresses a
    // track, and with Enter on a sampler in the band. Once up, its keys
    // are its own; the globals stay.
    Binding::command(
        ScopeContext::Root,
        Key::E,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::command(
        ScopeContext::Nested,
        Key::E,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::command(
        ScopeContext::Mixer,
        Key::E,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::command(
        ScopeContext::Clip,
        Key::E,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::command(
        ScopeContext::Chain,
        Key::E,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::Enter,
        StageIntent::Sample(SampleIntent::Open),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::ArrowLeft,
        StageIntent::Sample(SampleIntent::Left { coarse: false }),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::ArrowRight,
        StageIntent::Sample(SampleIntent::Right { coarse: false }),
    ),
    Binding::shift(
        ScopeContext::Sample,
        Key::ArrowLeft,
        StageIntent::Sample(SampleIntent::Left { coarse: true }),
    ),
    Binding::shift(
        ScopeContext::Sample,
        Key::ArrowRight,
        StageIntent::Sample(SampleIntent::Right { coarse: true }),
    ),
    Binding::command(
        ScopeContext::Sample,
        Key::ArrowLeft,
        StageIntent::Sample(SampleIntent::JumpPrev),
    ),
    Binding::command(
        ScopeContext::Sample,
        Key::ArrowRight,
        StageIntent::Sample(SampleIntent::JumpNext),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::ArrowUp,
        StageIntent::Sample(SampleIntent::ZoomIn),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::ArrowDown,
        StageIntent::Sample(SampleIntent::ZoomOut),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::PageUp,
        StageIntent::Sample(SampleIntent::ScrollLeft),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::PageDown,
        StageIntent::Sample(SampleIntent::ScrollRight),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Tab,
        StageIntent::Sample(SampleIntent::Page),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::S,
        StageIntent::Sample(SampleIntent::SetStart),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::E,
        StageIntent::Sample(SampleIntent::SetEnd),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::L,
        StageIntent::Sample(SampleIntent::SetLoop),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Enter,
        StageIntent::Sample(SampleIntent::AddSlice),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Delete,
        StageIntent::Sample(SampleIntent::RemoveSlice),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Backspace,
        StageIntent::Sample(SampleIntent::RemoveSlice),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::G,
        StageIntent::Sample(SampleIntent::Grid),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::T,
        StageIntent::Sample(SampleIntent::Transients),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::C,
        StageIntent::Sample(SampleIntent::ClearSlices),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Plus,
        StageIntent::Sample(SampleIntent::More),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Equals,
        StageIntent::Sample(SampleIntent::More),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Minus,
        StageIntent::Sample(SampleIntent::Less),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::CloseBracket,
        StageIntent::Sample(SampleIntent::Eager),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::OpenBracket,
        StageIntent::Sample(SampleIntent::Shyer),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::N,
        StageIntent::Sample(SampleIntent::Normalize),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::R,
        StageIntent::Sample(SampleIntent::Reverse),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::M,
        StageIntent::Sample(SampleIntent::Mode),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Q,
        StageIntent::Sample(SampleIntent::LoopMode),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Z,
        StageIntent::Sample(SampleIntent::Snap),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::P,
        StageIntent::Sample(SampleIntent::Audition),
    ),
    Binding::shift(
        ScopeContext::Sample,
        Key::P,
        StageIntent::Sample(SampleIntent::AuditionAll),
    ),
    Binding::new(ScopeContext::Sample, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Sample,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Sample, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Sample, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Sample, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Sample, Key::Z, StageIntent::Undo),
    Binding::command_shift(ScopeContext::Sample, Key::Z, StageIntent::Redo),
    Binding::command(ScopeContext::Sample, Key::S, StageIntent::Save),
    // The library, read again. From the browser, where the result is
    // seen, and from every place the browser can be summoned from.
    Binding::command(ScopeContext::Browser, Key::R, StageIntent::Rescan),
    Binding::command(ScopeContext::Root, Key::R, StageIntent::Rescan),
    Binding::command(ScopeContext::Nested, Key::R, StageIntent::Rescan),
    Binding::command(ScopeContext::Mixer, Key::R, StageIntent::Rescan),
    Binding::command(ScopeContext::Chain, Key::R, StageIntent::Rescan),
    // The band walks its devices on Tab, because the arrows are spent on
    // the row and its value: Up and Down choose the parameter, Left and
    // Right turn it, and Shift makes the turn coarse.
    Binding::new(
        ScopeContext::Chain,
        Key::Tab,
        StageIntent::Step(Step::Right),
    ),
    Binding::shift(ScopeContext::Chain, Key::Tab, StageIntent::Step(Step::Left)),
    // The field turned over: Tab, from the session and its levels and
    // from the mixer. Inside a clip Tab is the grammar's; in the band it
    // walks devices; in the room it turns pages.
    Binding::new(ScopeContext::Root, Key::Tab, StageIntent::SongView),
    Binding::new(ScopeContext::Nested, Key::Tab, StageIntent::SongView),
    Binding::new(ScopeContext::Mixer, Key::Tab, StageIntent::SongView),
    // The song view. Time and the field's turn as everywhere; the
    // cursor's walk and the shared verbs by the same keys the session
    // uses for the same acts; and its own: zoom on the shifted vertical
    // arrows, a bar at a time on the shifted horizontal ones, ^R to
    // hold a resize (rescan is the session's), D to double, P to pick.
    Binding::new(ScopeContext::Song, Key::Space, StageIntent::ToggleTransport),
    Binding::new(ScopeContext::Song, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Song, Key::Tab, StageIntent::SongView),
    Binding::new(ScopeContext::Song, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Song,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Song, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Song, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Song, Key::Backspace, StageIntent::Clear),
    Binding::new(ScopeContext::Song, Key::W, StageIntent::Nudge),
    Binding::new(ScopeContext::Song, Key::Q, StageIntent::Yank),
    Binding::new(ScopeContext::Song, Key::E, StageIntent::Put),
    Binding::new(
        ScopeContext::Song,
        Key::D,
        StageIntent::Song(SongIntent::Duplicate),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::P,
        StageIntent::Song(SongIntent::Pick),
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::P,
        StageIntent::Song(SongIntent::PickBack),
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::ArrowUp,
        StageIntent::Song(SongIntent::ZoomIn),
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::ArrowDown,
        StageIntent::Song(SongIntent::ZoomOut),
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::ArrowLeft,
        StageIntent::Song(SongIntent::Stretch(Step::Left)),
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::ArrowRight,
        StageIntent::Song(SongIntent::Stretch(Step::Right)),
    ),
    Binding::command(
        ScopeContext::Song,
        Key::ArrowLeft,
        StageIntent::Song(SongIntent::JumpPrev),
    ),
    Binding::command(
        ScopeContext::Song,
        Key::ArrowRight,
        StageIntent::Song(SongIntent::JumpNext),
    ),
    Binding::command(
        ScopeContext::Song,
        Key::R,
        StageIntent::Song(SongIntent::Resize),
    ),
    Binding::command(ScopeContext::Song, Key::M, StageIntent::Mix),
    Binding::command(ScopeContext::Song, Key::D, StageIntent::Devices),
    Binding::command(ScopeContext::Song, Key::B, StageIntent::Browse),
    Binding::command(ScopeContext::Song, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Song, Key::Z, StageIntent::Undo),
    Binding::command_shift(ScopeContext::Song, Key::Z, StageIntent::Redo),
    Binding::command(ScopeContext::Song, Key::S, StageIntent::Save),
    Binding::new(ScopeContext::Song, Key::Questionmark, StageIntent::Help),
    Binding::new(
        ScopeContext::Song,
        Key::OpenBracket,
        StageIntent::Song(SongIntent::BraceStart),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::CloseBracket,
        StageIntent::Song(SongIntent::BraceEnd),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::L,
        StageIntent::Song(SongIntent::ToggleLoop),
    ),
    Binding::new(
        ScopeContext::Song,
        Key::M,
        StageIntent::Song(SongIntent::Marker),
    ),
    Binding::command(
        ScopeContext::Song,
        Key::X,
        StageIntent::Song(SongIntent::Export),
    ),
    // Arming the arrangement is asked from wherever the session is
    // being played: the session's levels, the mixer, and the song view
    // itself.
    Binding::command(ScopeContext::Root, Key::Space, StageIntent::RecordSong),
    Binding::command(ScopeContext::Nested, Key::Space, StageIntent::RecordSong),
    Binding::command(ScopeContext::Mixer, Key::Space, StageIntent::RecordSong),
    Binding::command(ScopeContext::Song, Key::Space, StageIntent::RecordSong),
    // The device view on one key: V opens the strip band on the
    // cursor's track from wherever a track is addressed, and closes it
    // from inside. ^D stays as the old hand.
    Binding::new(ScopeContext::Root, Key::V, StageIntent::Devices),
    Binding::new(ScopeContext::Nested, Key::V, StageIntent::Devices),
    Binding::new(ScopeContext::Mixer, Key::V, StageIntent::Devices),
    Binding::new(ScopeContext::Song, Key::V, StageIntent::Devices),
    Binding::new(ScopeContext::Chain, Key::V, StageIntent::Devices),
    // A section IN or OUT: the desk's button, on the band's Shift+Enter
    // as well as the M the effects already answer to.
    Binding::shift(ScopeContext::Chain, Key::Enter, StageIntent::Mute),
];

/// Every binding in one scope, in table order. The help surface reads
/// THIS — it is a projection of the codebook, never prose written beside
/// it, so it cannot describe a key the stage does not actually answer to.
pub(super) fn bindings_for(
    scope: ScopeContext,
) -> impl Iterator<Item = (Modifiers, Key, StageIntent)> {
    BINDINGS
        .iter()
        .filter(move |binding| binding.scope == scope)
        .map(|binding| (binding.modifiers, binding.key, binding.intent))
}

/// Which family a verb belongs to, as the palette's grouping word.
///
/// A closed match rather than a field on the binding: a new intent that
/// forgets its family is a COMPILE ERROR, the same rule `label` follows,
/// so the palette cannot come to hold a verb it has no word for.
fn family(intent: StageIntent) -> &'static str {
    match intent {
        StageIntent::Step(_) | StageIntent::Enter | StageIntent::Escape => "move",
        StageIntent::ToggleTransport | StageIntent::Rewind => "time",
        StageIntent::Help
        | StageIntent::Browse
        | StageIntent::Mix
        | StageIntent::Ground
        | StageIntent::SongView => "view",
        StageIntent::TypeChar(_) | StageIntent::Backspace | StageIntent::Rescan => "browse",
        StageIntent::NewAudioTrack | StageIntent::NewInstrumentTrack => "track",
        StageIntent::Clear | StageIntent::Launch | StageIntent::LaunchScene => "session",
        StageIntent::Gain { .. }
        | StageIntent::Pan { .. }
        | StageIntent::Mute
        | StageIntent::Solo => "mixer",
        StageIntent::Devices | StageIntent::Param { .. } => "devices",
        StageIntent::Undo | StageIntent::Redo | StageIntent::Save => "document",
        StageIntent::Rename | StageIntent::DeleteTrack => "track",
        StageIntent::Nudge
        | StageIntent::Yank
        | StageIntent::Put
        | StageIntent::TrigMenu
        | StageIntent::ClearLock => "edit",
        StageIntent::Sample(_) => "sample",
        StageIntent::Song(_) | StageIntent::RecordSong => "song",
        StageIntent::Bus => "mix",
    }
}

/// One row of the palette: what it reads as, and what it does.
pub(super) struct Entry {
    pub(super) scope: ScopeContext,
    pub(super) command: crate::ui::palette::Command,
    pub(super) intent: StageIntent,
}

/// The codebook, as the palette reads it.
///
/// Built ONCE from the same binding table the keyboard and the help
/// surface read, so the palette is a third projection of one vocabulary
/// rather than a list written beside it — it cannot offer a verb the
/// stage does not answer to, or miss one it does.
///
/// The chord names are leaked deliberately: `palette::Command` holds
/// `&'static str`, the binding table is fixed at compile time, and this
/// runs exactly once. It is a static built late, not a leak that grows.
pub(super) fn palette_entries() -> &'static [Entry] {
    static ENTRIES: std::sync::LazyLock<Vec<Entry>> = std::sync::LazyLock::new(|| {
        BINDINGS
            .iter()
            .map(|binding| {
                let chord: &'static str = String::leak(chord_name(binding.modifiers, binding.key));
                let id: &'static str = String::leak(format!("{:?}/{}", binding.scope, chord));
                Entry {
                    scope: binding.scope,
                    command: crate::ui::palette::Command::new(
                        id,
                        family(binding.intent),
                        binding.intent.label(),
                    )
                    .hint(chord),
                    intent: binding.intent,
                }
            })
            .collect()
    });
    &ENTRIES
}

/// How a chord is written on the codebook.
pub(super) fn chord_name(modifiers: Modifiers, key: Key) -> String {
    // ASCII on purpose. The conventional shift mark is `⇧` (U+21E7),
    // which the bundled Terminus does not carry, and the arrow it does
    // carry (`↑`) already means Up — one sign, one meaning, so shift gets
    // a mark of its own rather than borrowing a direction's.
    let mut name = String::new();
    if modifiers.command {
        name.push('^');
    }
    if modifiers.shift {
        name.push('+');
    }
    name.push_str(key.symbol_or_name());
    name
}

/// Translate one physical key in one scope. This is the only stage key
/// lookup used by both the application and the headless sequence driver.
pub(super) fn dispatch(scope: ScopeContext, input: StageInput) -> Option<StageIntent> {
    match input {
        StageInput::Chord(modifiers, key) => BINDINGS
            .iter()
            .find(|binding| {
                binding.scope == scope && binding.modifiers == modifiers && binding.key == key
            })
            .map(|binding| binding.intent),
        StageInput::Text(ch)
            if matches!(scope, ScopeContext::Browser | ScopeContext::Rename)
                && !ch.is_control() =>
        {
            Some(StageIntent::TypeChar(ch))
        }
        StageInput::Text(_) => None,
    }
}

/// Every chord named by the table, once, MOST SPECIFIC FIRST — and that
/// order is load-bearing. egui's `consume_key` matches modifiers
/// logically, which means a held Shift or Alt the pattern did not ask
/// for is ignored: a pattern of `^T` matches a press of `^+T`. So the
/// chord with more modifiers has to be offered first, or the plainer
/// chord eats it and `^+T` silently becomes `^T`. Within one level of
/// specificity the table's own order stands.
pub(super) fn bound_chords() -> impl Iterator<Item = (Modifiers, Key)> {
    let mut chords: Vec<(Modifiers, Key)> = BINDINGS
        .iter()
        .enumerate()
        .filter_map(|(index, binding)| {
            let first = BINDINGS[..index].iter().all(|earlier| {
                (earlier.modifiers, earlier.key) != (binding.modifiers, binding.key)
            });
            first.then_some((binding.modifiers, binding.key))
        })
        .collect();
    chords.sort_by_key(|(modifiers, _)| std::cmp::Reverse(specificity(*modifiers)));
    chords.into_iter()
}

/// How many modifiers a chord holds. Command and ctrl count once between
/// them, because egui treats them as one logical key off a Mac.
fn specificity(modifiers: Modifiers) -> usize {
    usize::from(modifiers.command || modifiers.ctrl || modifiers.mac_cmd)
        + usize::from(modifiers.shift)
        + usize::from(modifiers.alt)
}

/// Take every chord bound IN `scope` out of this frame's input, in the
/// order [`bound_chords`] dictates, so that no plainer chord shadows a
/// more specific one. Only this scope's chords: a key another scope binds
/// is not the stage's here, and taking it would swallow it — inside a
/// clip the arrows and Enter belong to the sequencer's grammar, and the
/// stage must leave them in the input for it. `yield_to_grammar` names
/// chords that are someone else's THIS frame even though the scope binds
/// them: a sentence in progress owns Escape.
pub(super) fn consume_chords(
    input: &mut eframe::egui::InputState,
    scope: ScopeContext,
    yield_to_grammar: impl Fn(Modifiers, Key) -> bool,
) -> Vec<(Modifiers, Key)> {
    bound_chords()
        .filter(|(modifiers, key)| {
            dispatch(scope, StageInput::Chord(*modifiers, *key)).is_some()
                && !yield_to_grammar(*modifiers, *key)
                && input.consume_key(*modifiers, *key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scope_and_chord_maps_to_two_intents() {
        for (index, binding) in BINDINGS.iter().enumerate() {
            assert!(
                BINDINGS[..index].iter().all(|earlier| {
                    (earlier.scope, earlier.modifiers, earlier.key)
                        != (binding.scope, binding.modifiers, binding.key)
                }),
                "duplicate stage binding for {:?} + {:?}",
                binding.scope,
                binding.key
            );
        }
    }

    #[test]
    fn calibration_scopes_bind_the_same_keys_without_erasing_scope() {
        for (modifiers, key) in bound_chords() {
            assert_eq!(
                dispatch(ScopeContext::Root, StageInput::Chord(modifiers, key)),
                dispatch(ScopeContext::Nested, StageInput::Chord(modifiers, key))
            );
        }
    }

    /// The browser is REACHABLE from everywhere the stage can stand, and
    /// leaves by the same key it arrived by — plus the universal one.
    #[test]
    fn the_browser_can_be_summoned_from_anywhere_and_left_from_inside() {
        for scope in [ScopeContext::Root, ScopeContext::Nested] {
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::COMMAND, Key::F)),
                Some(StageIntent::Browse),
                "{scope:?} cannot reach the browser"
            );
        }
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Modifiers::COMMAND, Key::F)
            ),
            Some(StageIntent::Browse)
        );
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Modifiers::NONE, Key::Escape)
            ),
            Some(StageIntent::Escape)
        );
    }

    /// Inside a clip the sequencer's own command chords are its own:
    /// ^R resizes the clip and ^A selects all, and the stage must not
    /// take either before the grammar sees it. This is the binding that
    /// once shadowed clip resize with the library rescan.
    #[test]
    fn the_clip_scope_leaves_the_grammars_command_chords_alone() {
        for (verb, key, name) in crate::ui::sequencer::verbs::COMMAND_TABLE {
            assert_eq!(
                dispatch(
                    ScopeContext::Clip,
                    StageInput::Chord(Modifiers::COMMAND, *key)
                ),
                None,
                "the stage took ^{key:?} from the grammar's {name} ({verb:?})"
            );
        }
    }

    #[test]
    fn transport_keys_are_global_table_bindings() {
        for scope in ScopeContext::ALL {
            // While a name is being typed, a space is a space.
            if scope == ScopeContext::Rename {
                continue;
            }
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::NONE, Key::Space)),
                Some(StageIntent::ToggleTransport),
                "{scope:?} cannot stop or roll the song"
            );
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::NONE, Key::Home)),
                Some(StageIntent::Rewind),
                "{scope:?} cannot return the song to the top"
            );
        }
    }

    /// The help surface shows every key the scope answers to, and only
    /// those. A codebook that omits a symbol is worse than none, because
    /// it is believed.
    #[test]
    fn the_projection_matches_the_table_exactly() {
        for scope in ScopeContext::ALL {
            let listed: Vec<_> = bindings_for(scope).collect();
            for (modifiers, key, intent) in &listed {
                assert_eq!(
                    dispatch(scope, StageInput::Chord(*modifiers, *key)),
                    Some(*intent),
                    "the help surface would name a key the stage ignores"
                );
            }
            let bound = BINDINGS.iter().filter(|b| b.scope == scope).count();
            assert_eq!(listed.len(), bound, "the help surface would hide a key");
        }
    }

    /// Every intent the table can dispatch has a name to show. Guaranteed
    /// by exhaustiveness at compile time; asserted here so the guarantee
    /// is visible as a claim rather than an accident.
    #[test]
    fn every_bound_intent_can_name_itself() {
        for binding in BINDINGS {
            assert!(!binding.intent.label().is_empty());
        }
    }

    #[test]
    fn printable_text_is_a_browser_binding_not_a_side_channel() {
        assert_eq!(
            dispatch(ScopeContext::Browser, StageInput::Text('k')),
            Some(StageIntent::TypeChar('k'))
        );
        assert_eq!(dispatch(ScopeContext::Root, StageInput::Text('k')), None);
        assert_eq!(
            dispatch(ScopeContext::Browser, StageInput::Text('\n')),
            None,
            "control characters are not filter text"
        );
    }

    /// The bug this guards against shipped: `^+T` made an audio track,
    /// because `^T` was offered to egui first and egui's logical match
    /// ignores the extra Shift. A chord whose modifiers include another
    /// chord's, on the same key, must always be offered before it.
    #[test]
    fn a_more_specific_chord_is_always_offered_before_a_plainer_one() {
        let chords: Vec<_> = bound_chords().collect();
        for (i, (wide, key)) in chords.iter().enumerate() {
            for (narrow, other) in &chords[..i] {
                if key != other {
                    continue;
                }
                let narrow_within_wide = (!narrow.shift || wide.shift)
                    && (!narrow.alt || wide.alt)
                    && (!(narrow.command || narrow.ctrl) || (wide.command || wide.ctrl));
                assert!(
                    !(narrow_within_wide && specificity(*narrow) < specificity(*wide)),
                    "{narrow:?}+{key:?} is offered before {wide:?}+{key:?} and would eat it"
                );
            }
        }
    }

    /// The same, through egui itself rather than through our reading of
    /// it: a real `^+T` press consumed the way `show` consumes it must
    /// come out as `^+T`, not as `^T`.
    #[test]
    fn a_shifted_chord_survives_being_consumed_through_egui() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Modifiers::NONE
        };
        let raw = RawInput {
            events: vec![Event::Key {
                key: Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: press,
            }],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Root, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::T)],
            "the shifted chord was consumed as something else"
        );
        assert_eq!(
            dispatch(
                ScopeContext::Root,
                StageInput::Chord(consumed[0].0, consumed[0].1)
            ),
            Some(StageIntent::NewInstrumentTrack)
        );
    }

    /// Inside a clip the arrows and Enter are the grammar's. The stage's
    /// consumption must leave them in the input — this shipped the other
    /// way, and every key the sequencer needed vanished before it looked.
    #[test]
    fn a_scope_only_consumes_the_chords_it_binds() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let raw = RawInput {
            events: vec![press(Key::ArrowRight), press(Key::Enter), press(Key::Space)],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Clip, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Modifiers::NONE, Key::Space)],
            "the clip scope took a key it does not bind"
        );
        assert!(
            input.consume_key(Modifiers::NONE, Key::ArrowRight),
            "the arrow was swallowed before the grammar could see it"
        );
        assert!(
            input.consume_key(Modifiers::NONE, Key::Enter),
            "Enter was swallowed before the grammar could see it"
        );
    }
}
