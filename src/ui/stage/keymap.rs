//! The stage codebook: scope-conditioned keys translated into intents.
//!
//! Scope is part of every binding even while the calibration grid gives
//! every scope the same vocabulary. That keeps the dispatch shape honest for
//! the first real surface, where the same key may mean different things at
//! different depths.

use super::Step;
use super::key::{Key, Mods};
use crate::pages::PageKey;

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
    /// The clip is still open, but the sixteen physical trig keys own the
    /// keyboard instead of the sentence grammar.
    Steps,
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
    /// The floating multi-cell parameter-lock graph owns all editing keys
    /// until it commits or cancels.
    Plock,
    /// The project-wide modulation patchbay: source bank, destination
    /// browser, and response shaper share one keyboard-owned workspace.
    Modulation,
    /// The sample editor is up: one file, full screen, and the keys are
    /// the editor's — cursor, zoom, markers, slices, audition. A place
    /// of its own, like the browser, that Escape leaves.
    Sample,
    /// The song view: the arrangement's lanes, the cursor on a cell or
    /// a block. The tray beneath shows the block's pattern; the verbs
    /// here move, size, and lay blocks.
    Song,
    /// The forge is up: one sCOMP, full screen, its passes stacked and
    /// its knobs as rows. A place of its own, like the sample editor,
    /// that Escape leaves.
    Forge,
    /// The deck's window is up and the step keys are not: the arrows walk
    /// the lit page's eight cells and turn them, the digits pick a cell,
    /// the function keys turn the pages. The grid underneath waits;
    /// Escape puts the window away. With the step keys on, the Steps
    /// scope carries the same cell keys beside the sixteen.
    Deck,
    /// The session clip matrix is up over the song view: the arrows walk
    /// the session small, Enter lays the cell's clip (or the row's whole
    /// scene) at the arrangement's cursor and the cursor rides on. A
    /// window like the deck's; Escape puts it away.
    Matrix,
    /// The lab is the field and the TILER has the keys: Alt chords
    /// open, focus, swap, size and close its windows; Enter goes into
    /// the focused one; Escape leaves the lab.
    Lab,
    /// A KILN window has the keys: Tab walks its bands, the arrows and
    /// digits its macros and sliders, `/` opens the slider filter;
    /// Escape returns to the tiler. The tiler's Alt chords still work.
    Kiln,
    /// The kiln's slider filter is open and the letters are its text;
    /// Escape closes it, Enter keeps the query.
    KilnFilter,
    MidiLab,
    /// The menu of EXTENSIONS is up over the lab: the list of kinds a
    /// window can hold. Up and down walk it, a number picks one
    /// outright, Enter opens the one under the cursor and Escape puts
    /// the menu away without opening anything. It owns the keys while
    /// it stands, so the window underneath cannot answer them.
    LabMenu,
}

impl ScopeContext {
    pub(super) const ALL: [Self; 21] = [
        Self::Root,
        Self::Nested,
        Self::Browser,
        Self::Mixer,
        Self::Chain,
        Self::Steps,
        Self::Clip,
        Self::Rename,
        Self::TrigMenu,
        Self::Plock,
        Self::Modulation,
        Self::Sample,
        Self::Song,
        Self::Forge,
        Self::Deck,
        Self::Matrix,
        Self::Lab,
        Self::Kiln,
        Self::KilnFilter,
        Self::MidiLab,
        Self::LabMenu,
    ];
}

/// A semantic request to the stage state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageIntent {
    Step(Step),
    Page(PageKey),
    PageBack(PageKey),
    Slot(u8),
    Turn {
        up: bool,
        coarse: bool,
    },
    StepKeys,
    StepKey(u8),
    Window(Step),
    /// N in the step-key mode: the selected slot's lock slides.
    Slide,
    /// V: the deck's window, up or away.
    Deck,
    /// Left and Right with the deck up: the previous or next cell.
    SlotStep(Step),
    HeroTool(u8),
    /// Tab with a tall panel up: the keys move INTO the panel's own
    /// controls, on to its next one, and back out to the cell strip.
    /// `back` walks the other way.
    HeroFocus {
        back: bool,
    },
    /// P in the song view: the session clip matrix, up or away.
    Matrix,
    /// Enter in the matrix: lay the cell's clip, or the row's scene, at
    /// the arrangement's cursor; `over` clears the span first.
    MatrixLay {
        over: bool,
    },
    /// S in the matrix: onto the row's head, or back to the cell.
    MatrixHead,
    /// Ctrl+Shift+L, or `:lab`: the lab takes the field, or gives it back.
    Lab,
    /// Alt+Enter in the lab: a new kiln window beside the focused one.
    LabWindow,
    MidiLabOpen,
    MidiLabHear,
    MidiLabPlay,
    MidiLabSend,
    MidiLabStop,
    /// Shift+arrows in the MIDI Lab: change the control under the cursor.
    MidiLabShift(Step),
    /// Tab in the MIDI Lab: the next group of cells.
    MidiLabGroup,
    /// Alt+Q: close the focused window.
    LabClose,
    /// Alt+H/J/K/L: focus the window in that direction.
    LabFocus(Step),
    /// Alt+Shift+H/J/K/L: swap rooms with the window in that direction.
    LabSwap(Step),
    /// Alt+F: the focused window fills the field, or fits again.
    LabFull,
    /// Alt+[ and Alt+]: the focused window's room, smaller or larger.
    LabResize(Step),
    /// Alt+T: the split holding the focused window turns the other way.
    LabSplit,
    /// `A` in a MIDI Lab: the guided ladder, or the full inspector. Both
    /// drive the same composition, so the key is a door and not a mode.
    MidiLadder,
    /// Alt+Space: the next window.
    LabCycle,
    /// Tab in a kiln: the next band (engine, macros, sliders).
    KilnBand,
    /// A digit in a kiln: that macro (Shift for 9–16).
    KilnMacro(u8),
    /// Shift+arrows in a kiln: a coarse walk or turn.
    KilnCoarse(Step),
    /// `/` in a kiln: the slider filter, open or closed.
    KilnFilter,
    /// Space in a kiln: hear the engine's preview.
    KilnHear,
    /// P in a kiln: bake and print to the library.
    KilnPrint,
    /// S in a kiln: bake, save to the browser's kiln folder, and send to
    /// a new sampler track.
    KilnSend,
    KilnReplace,
    KilnOrbit(Step),
    KilnZoom(bool),
    KilnScrub(bool),
    /// E in a kiln: the next engine.
    KilnEngine,
    /// Hear the instrument under the band's cursor without a trig: a
    /// kit's pad in play, a brick's or a sampler's file.
    Hear,
    /// Copy the selected tracks — the session selection's, else the one
    /// the cursor is on — each beside its original.
    DuplicateTracks,
    /// Copy the selected scenes — the session selection's, else the one
    /// the cursor is on — each beneath its original.
    DuplicateScene,
    /// Swap the kit pad under the cursor with the pad in hand.
    SwapPad,
    /// Fill the addressed track's kit from the browsed file's folder.
    Fill,
    /// Jump the band's row cursor to the next or previous parameter
    /// group — the next pad of a kit, the next operator of an FM synth —
    /// rather than walking every row between.
    Group(Step),
    /// Toggle the grid cell under the cursor in the standing selection.
    Select,
    /// Select every address on the active grid.
    SelectAll,
    /// Extend an additive selection while the selection key is held.
    SelectStep(Step),
    Enter,
    Escape,
    /// Toggle the song clock between parked and rolling.
    ToggleTransport,
    /// Start recording every armed track, or stop and commit the take.
    ToggleRecord,
    /// Return the song clock to the top without changing its motion.
    Rewind,
    /// Show or hide the codebook for the current scope.
    Help,
    /// Open the machine-level project/startup deck.
    ProjectManager,
    /// Open audio, project, library, and interface preferences.
    Preferences,
    /// Open the fully specified offline render console.
    ExportConsole,
    /// Open the engine, library, recovery, and build report.
    Diagnostics,
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
    /// Arm or disarm the addressed audio or instrument track.
    ToggleTrackArm,
    /// Walk the addressed audio track through the current interface's
    /// available input routes.
    CycleTrackInput {
        back: bool,
    },
    /// Off, In, Auto for the addressed audio track.
    CycleTrackMonitor,
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
    /// Copy the addressed selection immediately after itself.
    Duplicate,
    /// Summon the trig menu over the trig under the cursor.
    TrigMenu,
    /// Release the parameter lock under the trig menu's cursor.
    ClearLock,
    /// Open the multi-cell parameter-lock graph.
    PlockEditor,
    PlockTab {
        backwards: bool,
    },
    PlockFine(Step),
    /// Address another selected parameter lane without changing its value.
    PlockLane {
        down: bool,
    },
    PlockAlgorithm,
    PlockExtreme {
        high: bool,
    },
    /// Open or close the project-wide modulation workspace.
    Modulation,
    /// Move focus between source, destination, and response zones.
    ModTab {
        backwards: bool,
    },
    /// Change the addressed response control. Shift asks for fine motion.
    ModAdjust {
        increase: bool,
        fine: bool,
    },
    ModAddLfo,
    ModAddFollower,
    ModShape {
        forward: bool,
    },
    ModRate {
        faster: bool,
    },
    ModClock,
    ModToggleWire,
    ModBypass,
    ModSolo,
    ModDelete,
    /// An act in the sample editor, or the act of opening it.
    Sample(SampleIntent),
    /// An act in the forge, or the act of opening it.
    Forge(ForgeIntent),
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
    /// Take hold of the nearest marker — in, out, loop, or a slice — so
    /// the arrows move it; or let go of the one held.
    Grab,
    /// The view fitted to the slice under the cursor, or to the trim.
    Fit,
    /// The whole file in view.
    Whole,
    /// Cut the slice under the cursor in two, at its middle.
    Split,
    /// The previous or next slice, stepped to and sounded.
    PrevSlice,
    NextSlice,
    /// A slice by its number on the row of digits, stepped to and
    /// sounded. Ten sits under the zero.
    Pick(usize),
}

/// What the forge can be told. Its own enum, like the sample editor's,
/// so the room's vocabulary is one thing.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ForgeIntent {
    /// Open the forge on the sCOMP under the cursor.
    Open,
    /// The row above, or below.
    Up,
    Down,
    /// Turn the row's knob down, or up.
    Left {
        coarse: bool,
    },
    Right {
        coarse: bool,
    },
    /// The next group of rows, round.
    Group,
    /// The row back to its default.
    Reset,
    /// The previous or next pass on show.
    PrevPass,
    NextPass,
    /// A pass on show by its number on the row of digits; zero is the
    /// source.
    Pick(usize),
    /// Hold every row's value as the other side of an A/B.
    Snapshot,
    /// Trade the rows for the held snapshot, and hold what they were.
    Swap,
    /// Nudge a few rows a little, at random.
    Mutate,
    /// Every row somewhere new, at random.
    Randomise,
}

impl ForgeIntent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "forge",
            Self::Up => "row up",
            Self::Down => "row down",
            Self::Left { coarse: false } => "turn down",
            Self::Left { coarse: true } => "turn down, coarsely",
            Self::Right { coarse: false } => "turn up",
            Self::Right { coarse: true } => "turn up, coarsely",
            Self::Group => "next group",
            Self::Reset => "reset the row",
            Self::PrevPass => "previous pass on show",
            Self::NextPass => "next pass on show",
            Self::Pick(_) => "pass on show, by number",
            Self::Snapshot => "hold a snapshot",
            Self::Swap => "trade with the snapshot",
            Self::Mutate => "mutate a little",
            Self::Randomise => "randomise everything",
        }
    }
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
            Self::Grab => "grab or drop the nearest marker",
            Self::Fit => "fit the view to the slice or trim",
            Self::Whole => "the whole file in view",
            Self::Split => "split the slice in half",
            Self::PrevSlice => "previous slice, played",
            Self::NextSlice => "next slice, played",
            Self::Pick(_) => "slice by number, played",
        }
    }
}

impl StageIntent {
    /// What this intent is called on the help surface IN `scope`: the
    /// few verbs that mean something else on the band say so there.
    pub(super) fn label_in(self, scope: ScopeContext) -> &'static str {
        if scope == ScopeContext::Chain {
            match self {
                Self::Clear => return "delete device / pad",
                Self::Mute => return "bypass or in/out",
                Self::Sample(SampleIntent::Open) => return "open its room",
                Self::Yank => return "yank device / pad",
                Self::Put => return "put device / pad",
                Self::Hear => return "hear pad or file",
                _ => {}
            }
        }
        if self == Self::Enter {
            match scope {
                ScopeContext::Deck => return "keep",
                ScopeContext::Steps => return "keep · place trig",
                _ => {}
            }
        }
        self.label()
    }

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
            Self::Page(key) => match key {
                PageKey::Trig => "trig page",
                PageKey::Src => "source page",
                PageKey::Fltr => "filter page",
                PageKey::Amp => "amp page",
                PageKey::Lfo => "lfo page",
                PageKey::Fx => "lane effects page",
                PageKey::Mix => "mix page",
                PageKey::All => "eighth key",
            },
            Self::PageBack(_) => "previous sub-page",
            Self::Slot(_) => "select deck slot",
            Self::Turn {
                up: true,
                coarse: false,
            } => "turn slot up",
            Self::Turn {
                up: false,
                coarse: false,
            } => "turn slot down",
            Self::Turn {
                up: true,
                coarse: true,
            } => "coarse slot up",
            Self::Turn {
                up: false,
                coarse: true,
            } => "coarse slot down",
            Self::StepKeys => "step keys",
            Self::Slide => "slide lock",
            Self::HeroFocus { back: false } => "into the picture",
            Self::HeroFocus { back: true } => "back through the picture",
            Self::HeroTool(verb) => {
                crate::pages::hero_tools(crate::devices::DeviceKind::Sampler, "Sample")
                    .iter()
                    .find(|t| t.verb == verb)
                    .map_or("sample tool", |t| t.word)
            }
            Self::Deck => "deck window",
            Self::Matrix => "clip matrix",
            Self::MatrixLay { over: false } => "lay clip or scene",
            Self::MatrixLay { over: true } => "lay over",
            Self::MatrixHead => "scene head",
            Self::Lab => "the lab",
            Self::LabWindow => "new kiln window",
            Self::MidiLabOpen => "open MIDI Lab",
            Self::MidiLabHear => "hear current chord",
            Self::MidiLabPlay => "play current MIDI clip",
            Self::MidiLabSend => "send MIDI to destination",
            Self::MidiLabStop => "stop MIDI preview",
            Self::MidiLabShift(_) => "change the control",
            Self::MidiLabGroup => "next lab group",
            Self::LabClose => "close window",
            Self::LabFocus(Step::Left) => "focus left",
            Self::LabFocus(Step::Right) => "focus right",
            Self::LabFocus(Step::Up) => "focus up",
            Self::LabFocus(Step::Down) => "focus down",
            Self::LabSwap(Step::Left) => "swap left",
            Self::LabSwap(Step::Right) => "swap right",
            Self::LabSwap(Step::Up) => "swap up",
            Self::LabSwap(Step::Down) => "swap down",
            Self::LabFull => "fullscreen window",
            Self::LabResize(Step::Right) => "grow window",
            Self::LabResize(_) => "shrink window",
            Self::LabSplit => "turn the split",
            Self::MidiLadder => "ladder or inspector",
            Self::LabCycle => "next lab window",
            Self::KilnBand => "next band",
            Self::KilnMacro(_) => "pick macro",
            Self::KilnCoarse(_) => "coarse turn",
            Self::KilnFilter => "filter sliders",
            Self::KilnHear => "hear the preview",
            Self::KilnPrint => "print bake",
            Self::KilnSend => "send to sampler",
            Self::KilnReplace => "replace last send",
            Self::KilnOrbit(_) => "orbit drum",
            Self::KilnZoom(_) => "zoom drum",
            Self::KilnScrub(_) => "scrub strike",
            Self::KilnEngine => "next engine",
            Self::SlotStep(Step::Left) => "previous cell",
            Self::SlotStep(Step::Right) => "next cell",
            Self::SlotStep(_) => "cell",
            Self::StepKey(_) => "hold or tap step",
            Self::Window(Step::Left) => "previous window",
            Self::Window(Step::Right) => "next window",
            Self::Window(_) => "step window",
            Self::Group(Step::Up | Step::Left) => "previous group",
            Self::Group(_) => "next group",
            Self::Select => "select this cell",
            Self::SelectAll => "select all",
            Self::SelectStep(Step::Up) => "select up",
            Self::SelectStep(Step::Down) => "select down",
            Self::SelectStep(Step::Left) => "select left",
            Self::SelectStep(Step::Right) => "select right",
            Self::Enter => "go in",
            Self::Escape => "go out",
            Self::ToggleTransport => "stop / roll",
            Self::ToggleRecord => "record armed",
            Self::Rewind => "return to top",
            Self::Help => "this list",
            Self::ProjectManager => "project deck",
            Self::Preferences => "preferences",
            Self::ExportConsole => "export console",
            Self::Diagnostics => "system diagnostics",
            Self::Browse => "browse",
            Self::TypeChar(_) => "type to filter",
            Self::Backspace => "erase filter",
            Self::NewAudioTrack => "new audio track",
            Self::NewInstrumentTrack => "new midi track",
            Self::ToggleTrackArm => "arm track",
            Self::CycleTrackInput { back: false } => "next track input",
            Self::CycleTrackInput { back: true } => "track input back",
            Self::CycleTrackMonitor => "track monitor",
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
            Self::Nudge => "nudge, then arrow",
            Self::Yank => "yank device",
            Self::Put => "put device",
            Self::Hear => "hear the pad",
            Self::DuplicateTracks => "duplicate track",
            Self::DuplicateScene => "duplicate scene",
            Self::SwapPad => "swap pad with hand",
            Self::Fill => "fill kit from folder",
            Self::Duplicate => "duplicate selection",
            Self::TrigMenu => "trig menu",
            Self::ClearLock => "clear lock",
            Self::PlockEditor => "plock graph",
            Self::PlockTab { backwards: false } => "next editor region",
            Self::PlockTab { backwards: true } => "previous editor region",
            Self::PlockFine(_) => "fine graph adjustment",
            Self::PlockLane { down: false } => "previous parameter lane",
            Self::PlockLane { down: true } => "next parameter lane",
            Self::PlockAlgorithm => "algorithm picker",
            Self::PlockExtreme { high: false } => "parameter minimum",
            Self::PlockExtreme { high: true } => "parameter maximum",
            Self::Modulation => "modulation",
            Self::ModTab { backwards: false } => "next modulation zone",
            Self::ModTab { backwards: true } => "previous modulation zone",
            Self::ModAdjust {
                increase: false,
                fine: false,
            } => "less modulation",
            Self::ModAdjust {
                increase: true,
                fine: false,
            } => "more modulation",
            Self::ModAdjust {
                increase: false,
                fine: true,
            } => "less modulation, finely",
            Self::ModAdjust {
                increase: true,
                fine: true,
            } => "more modulation, finely",
            Self::ModAddLfo => "add LFO",
            Self::ModAddFollower => "add follower",
            Self::ModShape { forward: false } => "previous LFO shape",
            Self::ModShape { forward: true } => "next LFO shape",
            Self::ModRate { faster: false } => "slower modulation",
            Self::ModRate { faster: true } => "faster modulation",
            Self::ModClock => "sync / free",
            Self::ModToggleWire => "patch / unpatch",
            Self::ModBypass => "bypass modulation wire",
            Self::ModSolo => "solo modulation wire",
            Self::ModDelete => "delete modulation selection",
            Self::Sample(intent) => intent.label(),
            Self::Forge(intent) => intent.label(),
            Self::Rescan => "rescan library",
            Self::SongView => "session / song",
            Self::Bus => "next bus",
            Self::Song(intent) => intent.label(),
            Self::RecordSong => "record to song",
        }
    }
}

/// A physical chord or text, as the view read it. Both travel through the same
/// scope-conditioned dispatcher before either can become an intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StageInput {
    Chord(Mods, Key),
    Text(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Binding {
    scope: ScopeContext,
    modifiers: Mods,
    key: Key,
    intent: StageIntent,
}

impl Binding {
    /// An unmodified key. The short code, spent on what is done often.
    const fn new(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Mods::NONE,
            key,
            intent,
        }
    }

    /// A held modifier is a longer code, for what is done less often —
    /// and for what must not fire by accident under the fingers.
    const fn command(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Mods::COMMAND,
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
            modifiers: Mods::SHIFT,
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
            modifiers: Mods::COMMAND.plus(Mods::SHIFT),
            key,
            intent,
        }
    }
}

/// The single source of truth for the stage keyboard vocabulary.
const BINDINGS: &[Binding] = &[
    // The machine room is global. These four chords are table data in every
    // scope so the dispatcher, palette, and help surface all tell the same
    // truth about reaching it.
    Binding::command(ScopeContext::Root, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Root, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Root, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Root, Key::D, StageIntent::Diagnostics),
    // The lab: a section of its own, from the field's three projections.
    Binding::command_shift(ScopeContext::Root, Key::L, StageIntent::Lab),
    Binding::command_shift(ScopeContext::Nested, Key::L, StageIntent::Lab),
    Binding::command_shift(ScopeContext::Song, Key::L, StageIntent::Lab),
    Binding::command_shift(ScopeContext::Mixer, Key::L, StageIntent::Lab),
    Binding::command(ScopeContext::Nested, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Nested, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Nested, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Nested, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Browser, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Browser, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Browser, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Browser, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Mixer, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Mixer, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Mixer, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Mixer, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Chain, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Chain, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Chain, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Chain, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Clip, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Clip, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Clip, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Clip, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Rename, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Rename, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Rename, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Rename, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::TrigMenu, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::TrigMenu, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::TrigMenu, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::TrigMenu, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Sample, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Sample, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Sample, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Sample, Key::D, StageIntent::Diagnostics),
    Binding::command(ScopeContext::Song, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Song, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Song, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Song, Key::D, StageIntent::Diagnostics),
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
    // Record is the dedicated transport key and remains the shortest way
    // both into and out of a take from every musical scope.
    Binding::new(ScopeContext::Root, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Nested, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Browser, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Mixer, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Chain, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Clip, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Rename, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::TrigMenu, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Plock, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Sample, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Song, Key::F9, StageIntent::ToggleRecord),
    Binding::new(ScopeContext::Root, Key::X, StageIntent::Select),
    Binding::new(ScopeContext::Nested, Key::X, StageIntent::Select),
    Binding::new(ScopeContext::Song, Key::X, StageIntent::Select),
    Binding::command(ScopeContext::Root, Key::A, StageIntent::SelectAll),
    Binding::command(ScopeContext::Nested, Key::A, StageIntent::SelectAll),
    Binding::command(ScopeContext::Song, Key::A, StageIntent::SelectAll),
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
    // Inside the band: bare arrows walk it — across the devices, down the
    // parameters — and a shifted arrow moves the value under the cursor,
    // the same gesture the mixer's pan already answers to.
    Binding::command(ScopeContext::Root, Key::D, StageIntent::DuplicateTracks),
    Binding::command(ScopeContext::Nested, Key::D, StageIntent::DuplicateTracks),
    Binding::command(ScopeContext::Mixer, Key::D, StageIntent::DuplicateTracks),
    Binding::command(ScopeContext::Chain, Key::D, StageIntent::DuplicateTracks),
    Binding::command(ScopeContext::Song, Key::D, StageIntent::DuplicateTracks),
    Binding::command_shift(ScopeContext::Root, Key::I, StageIntent::DuplicateScene),
    Binding::command_shift(ScopeContext::Nested, Key::I, StageIntent::DuplicateScene),
    Binding::command_shift(ScopeContext::Mixer, Key::I, StageIntent::DuplicateScene),
    Binding::command_shift(ScopeContext::Chain, Key::I, StageIntent::DuplicateScene),
    Binding::new(ScopeContext::Chain, Key::P, StageIntent::Hear),
    Binding::new(ScopeContext::Chain, Key::X, StageIntent::SwapPad),
    Binding::shift(ScopeContext::Browser, Key::Enter, StageIntent::Fill),
    Binding::new(
        ScopeContext::Chain,
        Key::PageUp,
        StageIntent::Group(Step::Up),
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::PageDown,
        StageIntent::Group(Step::Down),
    ),
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
    // The selected track's recording cluster: arm, source, monitor. Kept
    // off the clip scope because its letters belong to pitch entry and the
    // sequencer grammar there.
    Binding::new(ScopeContext::Root, Key::A, StageIntent::ToggleTrackArm),
    Binding::new(ScopeContext::Nested, Key::A, StageIntent::ToggleTrackArm),
    Binding::new(ScopeContext::Mixer, Key::A, StageIntent::ToggleTrackArm),
    Binding::new(ScopeContext::Chain, Key::A, StageIntent::ToggleTrackArm),
    Binding::new(ScopeContext::Song, Key::A, StageIntent::ToggleTrackArm),
    Binding::new(
        ScopeContext::Root,
        Key::I,
        StageIntent::CycleTrackInput { back: false },
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::I,
        StageIntent::CycleTrackInput { back: false },
    ),
    Binding::new(
        ScopeContext::Mixer,
        Key::I,
        StageIntent::CycleTrackInput { back: false },
    ),
    Binding::new(
        ScopeContext::Chain,
        Key::I,
        StageIntent::CycleTrackInput { back: false },
    ),
    Binding::new(
        ScopeContext::Song,
        Key::I,
        StageIntent::CycleTrackInput { back: false },
    ),
    Binding::shift(
        ScopeContext::Root,
        Key::I,
        StageIntent::CycleTrackInput { back: true },
    ),
    Binding::shift(
        ScopeContext::Nested,
        Key::I,
        StageIntent::CycleTrackInput { back: true },
    ),
    Binding::shift(
        ScopeContext::Mixer,
        Key::I,
        StageIntent::CycleTrackInput { back: true },
    ),
    Binding::shift(
        ScopeContext::Chain,
        Key::I,
        StageIntent::CycleTrackInput { back: true },
    ),
    Binding::shift(
        ScopeContext::Song,
        Key::I,
        StageIntent::CycleTrackInput { back: true },
    ),
    Binding::shift(ScopeContext::Root, Key::A, StageIntent::CycleTrackMonitor),
    Binding::shift(ScopeContext::Nested, Key::A, StageIntent::CycleTrackMonitor),
    Binding::shift(ScopeContext::Mixer, Key::A, StageIntent::CycleTrackMonitor),
    Binding::shift(ScopeContext::Chain, Key::A, StageIntent::CycleTrackMonitor),
    Binding::shift(ScopeContext::Song, Key::A, StageIntent::CycleTrackMonitor),
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
    // F2 now owns SRC. Rename keeps its old neighbourhood with Command:
    // the page is the plain key, the track-name operation the modified one.
    Binding::command(ScopeContext::Root, Key::F2, StageIntent::Rename),
    Binding::command(ScopeContext::Nested, Key::F2, StageIntent::Rename),
    Binding::command(ScopeContext::Mixer, Key::F2, StageIntent::Rename),
    // Deleting a track is the one verb here that destroys content, so it
    // takes the modifier — the same key that clears a slot, told to mean more.
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
    Binding::new(ScopeContext::Root, Key::Q, StageIntent::Yank),
    Binding::new(ScopeContext::Nested, Key::Q, StageIntent::Yank),
    Binding::new(ScopeContext::Chain, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Root, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Nested, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Mixer, Key::E, StageIntent::Put),
    Binding::new(ScopeContext::Root, Key::D, StageIntent::Duplicate),
    Binding::new(ScopeContext::Nested, Key::D, StageIntent::Duplicate),
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
    Binding::shift(ScopeContext::Clip, Key::Enter, StageIntent::PlockEditor),
    Binding::command(ScopeContext::Clip, Key::P, StageIntent::PlockEditor),
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
    // Multi-lock window: three Tab regions, bars on the arrows, X using
    // the same selection word, and `/` for the keyboard-only algorithms.
    Binding::new(
        ScopeContext::Plock,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Plock,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Plock,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Plock,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::shift(
        ScopeContext::Plock,
        Key::ArrowUp,
        StageIntent::PlockFine(Step::Up),
    ),
    Binding::shift(
        ScopeContext::Plock,
        Key::ArrowDown,
        StageIntent::PlockFine(Step::Down),
    ),
    Binding::shift(
        ScopeContext::Plock,
        Key::ArrowLeft,
        StageIntent::PlockFine(Step::Left),
    ),
    Binding::shift(
        ScopeContext::Plock,
        Key::ArrowRight,
        StageIntent::PlockFine(Step::Right),
    ),
    Binding::command(
        ScopeContext::Plock,
        Key::ArrowUp,
        StageIntent::PlockLane { down: false },
    ),
    Binding::command(
        ScopeContext::Plock,
        Key::ArrowDown,
        StageIntent::PlockLane { down: true },
    ),
    Binding::new(ScopeContext::Plock, Key::X, StageIntent::Select),
    Binding::command(ScopeContext::Plock, Key::A, StageIntent::SelectAll),
    Binding::new(ScopeContext::Plock, Key::Delete, StageIntent::ClearLock),
    Binding::new(ScopeContext::Plock, Key::Backspace, StageIntent::ClearLock),
    Binding::new(ScopeContext::Plock, Key::Slash, StageIntent::PlockAlgorithm),
    Binding::new(
        ScopeContext::Plock,
        Key::Tab,
        StageIntent::PlockTab { backwards: false },
    ),
    Binding::shift(
        ScopeContext::Plock,
        Key::Tab,
        StageIntent::PlockTab { backwards: true },
    ),
    Binding::new(
        ScopeContext::Plock,
        Key::Home,
        StageIntent::PlockExtreme { high: false },
    ),
    Binding::new(
        ScopeContext::Plock,
        Key::End,
        StageIntent::PlockExtreme { high: true },
    ),
    Binding::new(ScopeContext::Plock, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Plock, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Plock,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::command(ScopeContext::Plock, Key::L, StageIntent::Ground),
    // The modulation room. ^+M reaches it from every non-transactional
    // musical surface and closes it from inside. Within it, Tab moves among
    // the three zones; arrows address and shape; the direct letter verbs are
    // deliberately mnemonic because patching is a flow, not a form.
    Binding::command_shift(ScopeContext::Root, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Nested, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Browser, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Mixer, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Chain, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Clip, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Song, Key::M, StageIntent::Modulation),
    Binding::command_shift(ScopeContext::Modulation, Key::M, StageIntent::Modulation),
    Binding::new(
        ScopeContext::Modulation,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::shift(
        ScopeContext::Modulation,
        Key::ArrowLeft,
        StageIntent::ModAdjust {
            increase: false,
            fine: true,
        },
    ),
    Binding::shift(
        ScopeContext::Modulation,
        Key::ArrowRight,
        StageIntent::ModAdjust {
            increase: true,
            fine: true,
        },
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::Tab,
        StageIntent::ModTab { backwards: false },
    ),
    Binding::shift(
        ScopeContext::Modulation,
        Key::Tab,
        StageIntent::ModTab { backwards: true },
    ),
    Binding::new(ScopeContext::Modulation, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Modulation, Key::X, StageIntent::ModToggleWire),
    Binding::new(ScopeContext::Modulation, Key::L, StageIntent::ModAddLfo),
    Binding::new(
        ScopeContext::Modulation,
        Key::F,
        StageIntent::ModAddFollower,
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::OpenBracket,
        StageIntent::ModShape { forward: false },
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::CloseBracket,
        StageIntent::ModShape { forward: true },
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::Minus,
        StageIntent::ModRate { faster: false },
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::Equals,
        StageIntent::ModRate { faster: true },
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::Plus,
        StageIntent::ModRate { faster: true },
    ),
    Binding::new(ScopeContext::Modulation, Key::M, StageIntent::ModClock),
    Binding::new(ScopeContext::Modulation, Key::B, StageIntent::ModBypass),
    Binding::new(ScopeContext::Modulation, Key::S, StageIntent::ModSolo),
    Binding::new(
        ScopeContext::Modulation,
        Key::Delete,
        StageIntent::ModDelete,
    ),
    Binding::new(
        ScopeContext::Modulation,
        Key::Backspace,
        StageIntent::ModDelete,
    ),
    Binding::new(ScopeContext::Modulation, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Modulation,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Modulation, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Modulation, Key::F9, StageIntent::ToggleRecord),
    Binding::new(
        ScopeContext::Modulation,
        Key::Questionmark,
        StageIntent::Help,
    ),
    Binding::command(ScopeContext::Modulation, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Modulation, Key::Z, StageIntent::Undo),
    Binding::command_shift(ScopeContext::Modulation, Key::Z, StageIntent::Redo),
    Binding::command(ScopeContext::Modulation, Key::S, StageIntent::Save),
    Binding::command(
        ScopeContext::Modulation,
        Key::O,
        StageIntent::ProjectManager,
    ),
    Binding::command(
        ScopeContext::Modulation,
        Key::Comma,
        StageIntent::Preferences,
    ),
    Binding::command_shift(ScopeContext::Modulation, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Modulation, Key::D, StageIntent::Diagnostics),
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
    // The band's own door into a room. The band has no key of its own
    // any more, so this is reachable only by intent; it stays for the
    // tests that walk the rooms until the remake retires them.
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
    // The hand on a marker: +Enter takes hold of the nearest one, the
    // arrows carry it, Enter or Escape lets go. F fits the view to what
    // the cursor is in; +F shows the whole file. X halves a slice. The
    // comma and the period walk the slices and sound each; a digit goes
    // straight to one.
    Binding::shift(
        ScopeContext::Sample,
        Key::Enter,
        StageIntent::Sample(SampleIntent::Grab),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::F,
        StageIntent::Sample(SampleIntent::Fit),
    ),
    Binding::shift(
        ScopeContext::Sample,
        Key::F,
        StageIntent::Sample(SampleIntent::Whole),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::X,
        StageIntent::Sample(SampleIntent::Split),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Comma,
        StageIntent::Sample(SampleIntent::PrevSlice),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Period,
        StageIntent::Sample(SampleIntent::NextSlice),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num1,
        StageIntent::Sample(SampleIntent::Pick(1)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num2,
        StageIntent::Sample(SampleIntent::Pick(2)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num3,
        StageIntent::Sample(SampleIntent::Pick(3)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num4,
        StageIntent::Sample(SampleIntent::Pick(4)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num5,
        StageIntent::Sample(SampleIntent::Pick(5)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num6,
        StageIntent::Sample(SampleIntent::Pick(6)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num7,
        StageIntent::Sample(SampleIntent::Pick(7)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num8,
        StageIntent::Sample(SampleIntent::Pick(8)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num9,
        StageIntent::Sample(SampleIntent::Pick(9)),
    ),
    Binding::new(
        ScopeContext::Sample,
        Key::Num0,
        StageIntent::Sample(SampleIntent::Pick(10)),
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
    // The forge. Entered from the band with Enter on an sCOMP, the same
    // door the sampler's room has. Once up, the arrows are its rows and
    // knobs; the globals stay.
    Binding::new(
        ScopeContext::Forge,
        Key::ArrowUp,
        StageIntent::Forge(ForgeIntent::Up),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::ArrowDown,
        StageIntent::Forge(ForgeIntent::Down),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::ArrowLeft,
        StageIntent::Forge(ForgeIntent::Left { coarse: false }),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::ArrowRight,
        StageIntent::Forge(ForgeIntent::Right { coarse: false }),
    ),
    Binding::shift(
        ScopeContext::Forge,
        Key::ArrowLeft,
        StageIntent::Forge(ForgeIntent::Left { coarse: true }),
    ),
    Binding::shift(
        ScopeContext::Forge,
        Key::ArrowRight,
        StageIntent::Forge(ForgeIntent::Right { coarse: true }),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Tab,
        StageIntent::Forge(ForgeIntent::Group),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::R,
        StageIntent::Forge(ForgeIntent::Reset),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Comma,
        StageIntent::Forge(ForgeIntent::PrevPass),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Period,
        StageIntent::Forge(ForgeIntent::NextPass),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num0,
        StageIntent::Forge(ForgeIntent::Pick(0)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num1,
        StageIntent::Forge(ForgeIntent::Pick(1)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num2,
        StageIntent::Forge(ForgeIntent::Pick(2)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num3,
        StageIntent::Forge(ForgeIntent::Pick(3)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num4,
        StageIntent::Forge(ForgeIntent::Pick(4)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num5,
        StageIntent::Forge(ForgeIntent::Pick(5)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num6,
        StageIntent::Forge(ForgeIntent::Pick(6)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num7,
        StageIntent::Forge(ForgeIntent::Pick(7)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::Num8,
        StageIntent::Forge(ForgeIntent::Pick(8)),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::S,
        StageIntent::Forge(ForgeIntent::Snapshot),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::B,
        StageIntent::Forge(ForgeIntent::Swap),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::M,
        StageIntent::Forge(ForgeIntent::Mutate),
    ),
    Binding::new(
        ScopeContext::Forge,
        Key::X,
        StageIntent::Forge(ForgeIntent::Randomise),
    ),
    Binding::new(ScopeContext::Forge, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Forge,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Forge, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Forge, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Forge, Key::L, StageIntent::Ground),
    Binding::command(ScopeContext::Forge, Key::Z, StageIntent::Undo),
    Binding::command_shift(ScopeContext::Forge, Key::Z, StageIntent::Redo),
    Binding::command(ScopeContext::Forge, Key::S, StageIntent::Save),
    Binding::command(ScopeContext::Forge, Key::O, StageIntent::ProjectManager),
    Binding::command(ScopeContext::Forge, Key::Comma, StageIntent::Preferences),
    Binding::command_shift(ScopeContext::Forge, Key::E, StageIntent::ExportConsole),
    Binding::command_shift(ScopeContext::Forge, Key::D, StageIntent::Diagnostics),
    Binding::new(ScopeContext::Forge, Key::F9, StageIntent::ToggleRecord),
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
    Binding::new(ScopeContext::Song, Key::P, StageIntent::Matrix),
    Binding::command(
        ScopeContext::Song,
        Key::P,
        StageIntent::Song(SongIntent::Pick),
    ),
    Binding::command_shift(
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
    // cursor's track from wherever the hand is standing — the session,
    // the mixer, the song, a clip, a room — closing the room it was in,
    // and closes the band from inside it. Only the two scopes that type
    // letters keep V as a letter. ^D is the duplicate's now.
    Binding::new(ScopeContext::Root, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Nested, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Mixer, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Song, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Chain, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Clip, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::TrigMenu, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Plock, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Modulation, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Sample, Key::V, StageIntent::Deck),
    Binding::new(ScopeContext::Forge, Key::V, StageIntent::Deck),
    // A section IN or OUT: the desk's button, on the band's Shift+Enter
    // as well as the M the effects already answer to.
    Binding::shift(ScopeContext::Chain, Key::Enter, StageIntent::Mute),
];

/// Every binding in one scope, in table order. The help surface reads
/// THIS — it is a projection of the codebook, never prose written beside
/// it, so it cannot describe a key the stage does not actually answer to.
pub(super) fn bindings_for(scope: ScopeContext) -> std::vec::IntoIter<(Mods, Key, StageIntent)> {
    if scope == ScopeContext::Deck {
        return deck_bindings().into_iter();
    }
    if scope == ScopeContext::Matrix {
        return matrix_bindings().into_iter();
    }
    if matches!(
        scope,
        ScopeContext::Lab
            | ScopeContext::Kiln
            | ScopeContext::KilnFilter
            | ScopeContext::MidiLab
            | ScopeContext::LabMenu
    ) {
        return lab_bindings(scope).into_iter();
    }
    let inherited = if scope == ScopeContext::Steps {
        ScopeContext::Clip
    } else {
        scope
    };
    let mut bindings: Vec<_> = BINDINGS
        .iter()
        .filter(|binding| binding.scope == inherited)
        // With the step keys on, the arrows are the deck's: the cells
        // and their values, not the grid's cursor.
        .filter(|binding| {
            scope != ScopeContext::Steps
                || !matches!(
                    binding.key,
                    Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
                )
        })
        .map(|binding| (binding.modifiers, binding.key, binding.intent))
        .collect();
    bindings.extend(page_bindings(scope));
    bindings.into_iter()
}

/// The arrows as the deck reads them, in every scope where the deck's
/// cells are the subject: Left and Right walk the cells, Up and Down
/// turn the one under the cursor, Shift makes the turn coarse.
fn cell_arrows() -> [(Mods, Key, StageIntent); 6] {
    [
        (
            Mods::NONE,
            Key::ArrowLeft,
            StageIntent::SlotStep(Step::Left),
        ),
        (
            Mods::NONE,
            Key::ArrowRight,
            StageIntent::SlotStep(Step::Right),
        ),
        (
            Mods::NONE,
            Key::ArrowUp,
            StageIntent::Turn {
                up: true,
                coarse: false,
            },
        ),
        (
            Mods::NONE,
            Key::ArrowDown,
            StageIntent::Turn {
                up: false,
                coarse: false,
            },
        ),
        (
            Mods::SHIFT,
            Key::ArrowUp,
            StageIntent::Turn {
                up: true,
                coarse: true,
            },
        ),
        (
            Mods::SHIFT,
            Key::ArrowDown,
            StageIntent::Turn {
                up: false,
                coarse: true,
            },
        ),
    ]
}

/// The lab's scopes. The tiler's chords all carry Alt and are the same
/// from the tiler and from inside a window; the globals every scope
/// keeps ride along. Inside a kiln the bare keys are the instrument's;
/// with its filter open the letters are text and only the arrows,
/// Escape, Enter and Backspace are keys.
fn lab_bindings(scope: ScopeContext) -> Vec<(Mods, Key, StageIntent)> {
    let mut out = vec![
        (Mods::NONE, Key::Home, StageIntent::Rewind),
        (Mods::NONE, Key::F9, StageIntent::ToggleRecord),
        (Mods::COMMAND, Key::L, StageIntent::Ground),
        (Mods::NONE, Key::Questionmark, StageIntent::Help),
        (Mods::COMMAND, Key::Z, StageIntent::Undo),
        (Mods::COMMAND.plus(Mods::SHIFT), Key::Z, StageIntent::Redo),
        (Mods::COMMAND.plus(Mods::SHIFT), Key::L, StageIntent::Lab),
        (Mods::NONE, Key::Escape, StageIntent::Escape),
        (Mods::NONE, Key::Enter, StageIntent::Enter),
        (Mods::ALT, Key::Enter, StageIntent::LabWindow),
        (Mods::ALT, Key::M, StageIntent::MidiLabOpen),
        (Mods::ALT, Key::Q, StageIntent::LabClose),
        (Mods::ALT, Key::H, StageIntent::LabFocus(Step::Left)),
        (Mods::ALT, Key::J, StageIntent::LabFocus(Step::Down)),
        (Mods::ALT, Key::K, StageIntent::LabFocus(Step::Up)),
        (Mods::ALT, Key::L, StageIntent::LabFocus(Step::Right)),
        (
            Mods::ALT.plus(Mods::SHIFT),
            Key::H,
            StageIntent::LabSwap(Step::Left),
        ),
        (
            Mods::ALT.plus(Mods::SHIFT),
            Key::J,
            StageIntent::LabSwap(Step::Down),
        ),
        (
            Mods::ALT.plus(Mods::SHIFT),
            Key::K,
            StageIntent::LabSwap(Step::Up),
        ),
        (
            Mods::ALT.plus(Mods::SHIFT),
            Key::L,
            StageIntent::LabSwap(Step::Right),
        ),
        (Mods::ALT, Key::F, StageIntent::LabFull),
        (
            Mods::ALT,
            Key::OpenBracket,
            StageIntent::LabResize(Step::Left),
        ),
        (
            Mods::ALT,
            Key::CloseBracket,
            StageIntent::LabResize(Step::Right),
        ),
        (Mods::ALT, Key::T, StageIntent::LabSplit),
        (Mods::ALT, Key::Space, StageIntent::LabCycle),
    ];
    // Alt and a number: the nth window, whatever extension it holds.
    // A tiler needs somewhere to jump to, not only a way to step.
    for (at, key) in DIGITS.into_iter().enumerate() {
        out.push((Mods::ALT, key, StageIntent::Slot(at as u8)));
    }
    match scope {
        ScopeContext::MidiLab => {
            out.extend([
                (Mods::NONE, Key::H, StageIntent::MidiLabHear),
                (Mods::NONE, Key::P, StageIntent::MidiLabPlay),
                (Mods::NONE, Key::Space, StageIntent::ToggleTransport),
                (Mods::NONE, Key::S, StageIntent::MidiLabSend),
                (Mods::SHIFT, Key::P, StageIntent::MidiLabStop),
                // A page this wide spends its arrows on getting about:
                // they walk every control on the screen, and shift and an
                // arrow changes whatever they have reached.
                (Mods::NONE, Key::ArrowLeft, StageIntent::Step(Step::Left)),
                (Mods::NONE, Key::ArrowRight, StageIntent::Step(Step::Right)),
                (Mods::NONE, Key::ArrowUp, StageIntent::Step(Step::Up)),
                (Mods::NONE, Key::ArrowDown, StageIntent::Step(Step::Down)),
                (
                    Mods::SHIFT,
                    Key::ArrowUp,
                    StageIntent::MidiLabShift(Step::Up),
                ),
                (
                    Mods::SHIFT,
                    Key::ArrowDown,
                    StageIntent::MidiLabShift(Step::Down),
                ),
                (
                    Mods::SHIFT,
                    Key::ArrowLeft,
                    StageIntent::MidiLabShift(Step::Left),
                ),
                (
                    Mods::SHIFT,
                    Key::ArrowRight,
                    StageIntent::MidiLabShift(Step::Right),
                ),
                (Mods::NONE, Key::Tab, StageIntent::MidiLabGroup),
                (Mods::NONE, Key::A, StageIntent::MidiLadder),
            ]);
        }
        ScopeContext::Lab => {
            out.push((Mods::NONE, Key::Space, StageIntent::ToggleTransport));
        }
        // The extension menu: a short list, and the numbers beside it.
        ScopeContext::LabMenu => {
            // The transport belongs to no scope in particular, this one
            // included: a menu must not be a place the song cannot stop.
            out.push((Mods::NONE, Key::Space, StageIntent::ToggleTransport));
            out.push((Mods::NONE, Key::ArrowUp, StageIntent::Step(Step::Up)));
            out.push((Mods::NONE, Key::ArrowDown, StageIntent::Step(Step::Down)));
            out.push((Mods::NONE, Key::K, StageIntent::Step(Step::Up)));
            out.push((Mods::NONE, Key::J, StageIntent::Step(Step::Down)));
            for (at, key) in DIGITS.into_iter().enumerate() {
                out.push((Mods::NONE, key, StageIntent::Slot(at as u8)));
            }
        }
        ScopeContext::Kiln => {
            out.push((Mods::NONE, Key::Tab, StageIntent::KilnBand));
            out.push((Mods::NONE, Key::ArrowUp, StageIntent::Step(Step::Up)));
            out.push((Mods::NONE, Key::ArrowDown, StageIntent::Step(Step::Down)));
            out.push((Mods::NONE, Key::ArrowLeft, StageIntent::Step(Step::Left)));
            out.push((Mods::NONE, Key::ArrowRight, StageIntent::Step(Step::Right)));
            out.push((Mods::SHIFT, Key::ArrowUp, StageIntent::KilnCoarse(Step::Up)));
            out.push((
                Mods::SHIFT,
                Key::ArrowDown,
                StageIntent::KilnCoarse(Step::Down),
            ));
            out.push((
                Mods::SHIFT,
                Key::ArrowLeft,
                StageIntent::KilnCoarse(Step::Left),
            ));
            out.push((
                Mods::SHIFT,
                Key::ArrowRight,
                StageIntent::KilnCoarse(Step::Right),
            ));
            for (n, key) in DIGITS.into_iter().enumerate() {
                out.push((Mods::NONE, key, StageIntent::KilnMacro(n as u8)));
                out.push((Mods::SHIFT, key, StageIntent::KilnMacro(n as u8 + 8)));
            }
            out.push((Mods::NONE, Key::Slash, StageIntent::KilnFilter));
            out.push((Mods::NONE, Key::Space, StageIntent::ToggleTransport));
            out.push((Mods::NONE, Key::H, StageIntent::KilnHear));
            out.push((Mods::NONE, Key::P, StageIntent::KilnPrint));
            out.push((Mods::NONE, Key::S, StageIntent::KilnSend));
            out.push((Mods::SHIFT, Key::S, StageIntent::KilnReplace));
            for (key, step) in [
                (Key::ArrowLeft, Step::Left),
                (Key::ArrowRight, Step::Right),
                (Key::ArrowUp, Step::Up),
                (Key::ArrowDown, Step::Down),
            ] {
                out.push((Mods::COMMAND, key, StageIntent::KilnOrbit(step)));
            }
            out.push((Mods::NONE, Key::Equals, StageIntent::KilnZoom(true)));
            out.push((Mods::NONE, Key::Plus, StageIntent::KilnZoom(true)));
            out.push((Mods::SHIFT, Key::Plus, StageIntent::KilnZoom(true)));
            out.push((Mods::NONE, Key::Minus, StageIntent::KilnZoom(false)));
            out.push((
                Mods::COMMAND,
                Key::OpenBracket,
                StageIntent::KilnScrub(false),
            ));
            out.push((
                Mods::COMMAND,
                Key::CloseBracket,
                StageIntent::KilnScrub(true),
            ));
            out.push((Mods::NONE, Key::E, StageIntent::KilnEngine));
        }
        _ => {
            out.push((Mods::NONE, Key::Space, StageIntent::ToggleTransport));
            out.push((Mods::NONE, Key::ArrowUp, StageIntent::Step(Step::Up)));
            out.push((Mods::NONE, Key::ArrowDown, StageIntent::Step(Step::Down)));
            out.push((Mods::NONE, Key::ArrowLeft, StageIntent::Step(Step::Left)));
            out.push((Mods::NONE, Key::ArrowRight, StageIntent::Step(Step::Right)));
            out.push((Mods::NONE, Key::Backspace, StageIntent::Backspace));
        }
    }
    out
}

/// The matrix's own scope: the globals every window keeps, the arrows
/// over the session small, Enter to lay, S for the row's head, P or
/// Escape to put it away. No page keys: the deck stays down while the
/// matrix is up.
fn matrix_bindings() -> Vec<(Mods, Key, StageIntent)> {
    vec![
        (Mods::NONE, Key::Space, StageIntent::ToggleTransport),
        (Mods::NONE, Key::Home, StageIntent::Rewind),
        (Mods::NONE, Key::F9, StageIntent::ToggleRecord),
        (Mods::COMMAND, Key::L, StageIntent::Ground),
        (Mods::NONE, Key::Questionmark, StageIntent::Help),
        (Mods::COMMAND, Key::Z, StageIntent::Undo),
        (Mods::COMMAND.plus(Mods::SHIFT), Key::Z, StageIntent::Redo),
        (Mods::NONE, Key::Escape, StageIntent::Escape),
        (Mods::NONE, Key::P, StageIntent::Matrix),
        (Mods::NONE, Key::ArrowUp, StageIntent::Step(Step::Up)),
        (Mods::NONE, Key::ArrowDown, StageIntent::Step(Step::Down)),
        (Mods::NONE, Key::ArrowLeft, StageIntent::Step(Step::Left)),
        (Mods::NONE, Key::ArrowRight, StageIntent::Step(Step::Right)),
        (
            Mods::NONE,
            Key::Enter,
            StageIntent::MatrixLay { over: false },
        ),
        (
            Mods::SHIFT,
            Key::Enter,
            StageIntent::MatrixLay { over: true },
        ),
        (Mods::NONE, Key::S, StageIntent::MatrixHead),
    ]
}

/// The deck's own scope: a modal window like the browser, with the
/// globals every scope keeps, the function keys, the cell keys and the
/// arrows. The digits pick a cell directly; the step keys, when on, take
/// the digits back and the Steps scope carries the cells instead.
fn deck_bindings() -> Vec<(Mods, Key, StageIntent)> {
    let mut out = vec![
        (Mods::NONE, Key::Space, StageIntent::ToggleTransport),
        (Mods::NONE, Key::Home, StageIntent::Rewind),
        (Mods::NONE, Key::F9, StageIntent::ToggleRecord),
        (Mods::COMMAND, Key::L, StageIntent::Ground),
        (Mods::NONE, Key::Questionmark, StageIntent::Help),
        (Mods::COMMAND, Key::Z, StageIntent::Undo),
        (Mods::COMMAND.plus(Mods::SHIFT), Key::Z, StageIntent::Redo),
        (Mods::NONE, Key::Escape, StageIntent::Escape),
        (Mods::NONE, Key::V, StageIntent::Deck),
        (Mods::NONE, Key::L, StageIntent::StepKeys),
        // Enter is the deck's while the window is up: KEEP. Left to the
        // grid it would be ACT, which pastes the last chord over every
        // selected cell and throws away the note, velocity and length
        // the cells were just turned to.
        (Mods::NONE, Key::Enter, StageIntent::Enter),
    ];
    for (key, page) in F_KEYS {
        out.push((Mods::SHIFT, key, StageIntent::PageBack(page)));
        out.push((Mods::NONE, key, StageIntent::Page(page)));
    }
    for (slot, key) in DIGITS.into_iter().enumerate() {
        out.push((Mods::NONE, key, StageIntent::Slot(slot as u8)));
    }
    out.extend(
        crate::pages::hero_tools(crate::devices::DeviceKind::Sampler, "Sample")
            .iter()
            .map(|t| (Mods::NONE, t.key, StageIntent::HeroTool(t.verb))),
    );
    // A machine with a tall panel has controls the eight cells cannot
    // hold. Tab is how the keys get to them, and how they come back.
    out.push((Mods::SHIFT, Key::Tab, StageIntent::HeroFocus { back: true }));
    out.push((Mods::NONE, Key::Tab, StageIntent::HeroFocus { back: false }));
    out.extend(cell_arrows());
    out
}

const F_KEYS: [(Key, PageKey); 8] = [
    (Key::F1, PageKey::Trig),
    (Key::F2, PageKey::Src),
    (Key::F3, PageKey::Fltr),
    (Key::F4, PageKey::Amp),
    (Key::F5, PageKey::Lfo),
    (Key::F6, PageKey::Fx),
    (Key::F7, PageKey::Mix),
    (Key::F8, PageKey::All),
];

const DIGITS: [Key; 8] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
];

fn page_bindings(scope: ScopeContext) -> Vec<(Mods, Key, StageIntent)> {
    if matches!(
        scope,
        ScopeContext::Browser
            | ScopeContext::Rename
            | ScopeContext::Matrix
            | ScopeContext::Lab
            | ScopeContext::Kiln
            | ScopeContext::KilnFilter
            | ScopeContext::MidiLab
    ) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (key, page) in [
        (Key::F1, PageKey::Trig),
        (Key::F2, PageKey::Src),
        (Key::F3, PageKey::Fltr),
        (Key::F4, PageKey::Amp),
        (Key::F5, PageKey::Lfo),
        (Key::F6, PageKey::Fx),
        (Key::F7, PageKey::Mix),
        (Key::F8, PageKey::All),
    ] {
        out.push((Mods::SHIFT, key, StageIntent::PageBack(page)));
        out.push((Mods::NONE, key, StageIntent::Page(page)));
    }
    if scope == ScopeContext::Steps {
        for (slot, key) in [
            Key::A,
            Key::S,
            Key::D,
            Key::F,
            Key::G,
            Key::H,
            Key::J,
            Key::K,
        ]
        .into_iter()
        .enumerate()
        {
            out.push((Mods::NONE, key, StageIntent::Slot(slot as u8)));
        }
        for (step, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Q,
            Key::W,
            Key::E,
            Key::R,
            Key::T,
            Key::Y,
            Key::U,
            Key::I,
        ]
        .into_iter()
        .enumerate()
        {
            out.push((Mods::NONE, key, StageIntent::StepKey(step as u8)));
        }
        out.extend(cell_arrows());
        out.extend([
            (
                Mods::NONE,
                Key::OpenBracket,
                StageIntent::Window(Step::Left),
            ),
            (
                Mods::NONE,
                Key::CloseBracket,
                StageIntent::Window(Step::Right),
            ),
            (Mods::NONE, Key::L, StageIntent::StepKeys),
        ]);
    } else {
        for (slot, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
        ]
        .into_iter()
        .enumerate()
        {
            out.push((Mods::ALT, key, StageIntent::Slot(slot as u8)));
        }
        out.extend([
            (
                Mods::ALT,
                Key::ArrowLeft,
                StageIntent::Turn {
                    up: false,
                    coarse: false,
                },
            ),
            (
                Mods::ALT,
                Key::ArrowRight,
                StageIntent::Turn {
                    up: true,
                    coarse: false,
                },
            ),
            (
                Mods::ALT.plus(Mods::SHIFT),
                Key::ArrowLeft,
                StageIntent::Turn {
                    up: false,
                    coarse: true,
                },
            ),
            (
                Mods::ALT.plus(Mods::SHIFT),
                Key::ArrowRight,
                StageIntent::Turn {
                    up: true,
                    coarse: true,
                },
            ),
        ]);
        if scope == ScopeContext::Clip {
            out.push((Mods::NONE, Key::L, StageIntent::StepKeys));
        }
    }
    if scope == ScopeContext::Steps {
        out.push((Mods::NONE, Key::N, StageIntent::Slide));
        // Enter confirms: the selected steps take their trig.
        out.push((Mods::NONE, Key::Enter, StageIntent::Enter));
    }
    out
}

/// Which family a verb belongs to, as the palette's grouping word.
///
/// A closed match rather than a field on the binding: a new intent that
/// forgets its family is a COMPILE ERROR, the same rule `label` follows,
/// so the palette cannot come to hold a verb it has no word for.
fn family(intent: StageIntent) -> &'static str {
    match intent {
        StageIntent::Step(_)
        | StageIntent::Group(_)
        | StageIntent::Select
        | StageIntent::SelectAll
        | StageIntent::SelectStep(_)
        | StageIntent::Enter
        | StageIntent::Escape => "move",
        StageIntent::Page(_)
        | StageIntent::PageBack(_)
        | StageIntent::Slot(_)
        | StageIntent::Turn { .. }
        | StageIntent::StepKeys
        | StageIntent::StepKey(_)
        | StageIntent::Window(_)
        | StageIntent::Slide
        | StageIntent::HeroTool(_)
        | StageIntent::HeroFocus { .. }
        | StageIntent::Deck
        | StageIntent::SlotStep(_) => "pages",
        StageIntent::Lab
        | StageIntent::LabWindow
        | StageIntent::MidiLabOpen
        | StageIntent::MidiLabHear
        | StageIntent::MidiLabPlay
        | StageIntent::MidiLabSend
        | StageIntent::MidiLabStop
        | StageIntent::MidiLabShift(_)
        | StageIntent::MidiLabGroup
        | StageIntent::LabClose
        | StageIntent::LabFocus(_)
        | StageIntent::LabSwap(_)
        | StageIntent::LabFull
        | StageIntent::LabResize(_)
        | StageIntent::LabSplit
        | StageIntent::MidiLadder
        | StageIntent::LabCycle
        | StageIntent::KilnBand
        | StageIntent::KilnMacro(_)
        | StageIntent::KilnCoarse(_)
        | StageIntent::KilnFilter
        | StageIntent::KilnHear
        | StageIntent::KilnPrint
        | StageIntent::KilnSend
        | StageIntent::KilnReplace
        | StageIntent::KilnOrbit(_)
        | StageIntent::KilnZoom(_)
        | StageIntent::KilnScrub(_)
        | StageIntent::KilnEngine => "lab",
        StageIntent::ToggleTransport | StageIntent::ToggleRecord | StageIntent::Rewind => "time",
        StageIntent::Help
        | StageIntent::Browse
        | StageIntent::Mix
        | StageIntent::Ground
        | StageIntent::SongView => "view",
        StageIntent::TypeChar(_) | StageIntent::Backspace | StageIntent::Rescan => "browse",
        StageIntent::ProjectManager
        | StageIntent::Preferences
        | StageIntent::ExportConsole
        | StageIntent::Diagnostics => "system",
        StageIntent::NewAudioTrack
        | StageIntent::NewInstrumentTrack
        | StageIntent::ToggleTrackArm
        | StageIntent::CycleTrackInput { .. }
        | StageIntent::CycleTrackMonitor => "track",
        StageIntent::Clear
        | StageIntent::Launch
        | StageIntent::LaunchScene
        | StageIntent::Matrix
        | StageIntent::MatrixLay { .. }
        | StageIntent::MatrixHead => "session",
        StageIntent::Gain { .. }
        | StageIntent::Pan { .. }
        | StageIntent::Mute
        | StageIntent::Solo => "mixer",
        StageIntent::Devices | StageIntent::Param { .. } => "devices",
        StageIntent::Undo | StageIntent::Redo | StageIntent::Save => "document",
        StageIntent::Rename
        | StageIntent::DeleteTrack
        | StageIntent::DuplicateTracks
        | StageIntent::DuplicateScene => "track",
        StageIntent::Nudge
        | StageIntent::Yank
        | StageIntent::Put
        | StageIntent::Hear
        | StageIntent::SwapPad
        | StageIntent::Fill
        | StageIntent::Duplicate
        | StageIntent::TrigMenu
        | StageIntent::ClearLock
        | StageIntent::PlockEditor
        | StageIntent::PlockTab { .. }
        | StageIntent::PlockFine(_)
        | StageIntent::PlockLane { .. }
        | StageIntent::PlockAlgorithm
        | StageIntent::PlockExtreme { .. } => "edit",
        StageIntent::Modulation
        | StageIntent::ModTab { .. }
        | StageIntent::ModAdjust { .. }
        | StageIntent::ModAddLfo
        | StageIntent::ModAddFollower
        | StageIntent::ModShape { .. }
        | StageIntent::ModRate { .. }
        | StageIntent::ModClock
        | StageIntent::ModToggleWire
        | StageIntent::ModBypass
        | StageIntent::ModSolo
        | StageIntent::ModDelete => "modulation",
        StageIntent::Sample(_) => "sample",
        StageIntent::Forge(_) => "forge",
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
        ScopeContext::ALL
            .into_iter()
            .flat_map(|scope| bindings_for(scope).map(move |binding| (scope, binding)))
            .map(|(scope, (modifiers, key, intent))| {
                let chord: &'static str = String::leak(chord_name(modifiers, key));
                let id: &'static str = String::leak(format!("{scope:?}/{chord}"));
                Entry {
                    scope,
                    command: crate::ui::palette::Command::new(
                        id,
                        family(intent),
                        intent.label_in(scope),
                    )
                    .hint(chord),
                    intent,
                }
            })
            .collect()
    });
    &ENTRIES
}

/// How a chord is written on the codebook.
pub(super) fn chord_name(modifiers: Mods, key: Key) -> String {
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
    if modifiers.alt {
        name.push_str("Alt+");
    }
    name.push_str(key.symbol_or_name());
    name
}

/// Translate one physical key in one scope. This is the only stage key
/// lookup used by both the application and the headless sequence driver.
pub(super) fn dispatch(scope: ScopeContext, input: StageInput) -> Option<StageIntent> {
    match input {
        StageInput::Chord(modifiers, key) => bindings_for(scope)
            .find(|(bound_modifiers, bound_key, _)| {
                *bound_modifiers == modifiers && *bound_key == key
            })
            .map(|(_, _, intent)| intent),
        StageInput::Text(ch)
            if matches!(
                scope,
                ScopeContext::Browser
                    | ScopeContext::Rename
                    | ScopeContext::KilnFilter
                    | ScopeContext::MidiLab
            ) && !ch.is_control() =>
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
pub(super) fn bound_chords() -> impl Iterator<Item = (Mods, Key)> {
    let mut chords: Vec<(Mods, Key)> = ScopeContext::ALL
        .into_iter()
        .flat_map(bindings_for)
        .map(|(mods, key, _)| (mods, key))
        .collect();
    chords.sort_unstable();
    chords.dedup();
    chords.sort_by_key(|(modifiers, _)| std::cmp::Reverse(specificity(*modifiers)));
    chords.into_iter()
}

/// How many modifiers a chord holds.
fn specificity(modifiers: Mods) -> usize {
    usize::from(modifiers.command) + usize::from(modifiers.shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scope_and_chord_maps_to_two_intents() {
        for scope in ScopeContext::ALL {
            let bindings: Vec<_> = bindings_for(scope).collect();
            for (index, (modifiers, key, _)) in bindings.iter().enumerate() {
                assert!(
                    bindings[..index]
                        .iter()
                        .all(|(earlier_modifiers, earlier_key, _)| {
                            (*earlier_modifiers, *earlier_key) != (*modifiers, *key)
                        }),
                    "duplicate stage binding for {scope:?} + {modifiers:?} + {key:?}"
                );
            }
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
                dispatch(scope, StageInput::Chord(Mods::COMMAND, Key::F)),
                Some(StageIntent::Browse),
                "{scope:?} cannot reach the browser"
            );
        }
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Mods::COMMAND, Key::F)
            ),
            Some(StageIntent::Browse)
        );
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Mods::NONE, Key::Escape)
            ),
            Some(StageIntent::Escape)
        );
    }

    #[test]
    fn transport_keys_are_global_table_bindings() {
        for scope in ScopeContext::ALL {
            // While a name is being typed, a space is a space.
            if scope == ScopeContext::Rename {
                continue;
            }
            assert_eq!(
                dispatch(scope, StageInput::Chord(Mods::NONE, Key::Space)),
                Some(StageIntent::ToggleTransport),
                "{scope:?} cannot stop or roll the song"
            );
            if scope != ScopeContext::Plock {
                assert_eq!(
                    dispatch(scope, StageInput::Chord(Mods::NONE, Key::Home)),
                    Some(StageIntent::Rewind),
                    "{scope:?} cannot return the song to the top"
                );
            }
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
        }
    }

    /// Every intent the table can dispatch has a name to show. Guaranteed
    /// by exhaustiveness at compile time; asserted here so the guarantee
    /// is visible as a claim rather than an accident.
    #[test]
    fn every_bound_intent_can_name_itself() {
        for scope in ScopeContext::ALL {
            for (_, _, intent) in bindings_for(scope) {
                assert!(!intent.label_in(scope).is_empty());
            }
        }
    }

    #[test]
    fn page_intent_labels_fit_the_codebook_row() {
        for intent in [
            StageIntent::Page(PageKey::Trig),
            StageIntent::PageBack(PageKey::Src),
            StageIntent::Slot(0),
            StageIntent::Turn {
                up: true,
                coarse: false,
            },
            StageIntent::Turn {
                up: false,
                coarse: true,
            },
            StageIntent::StepKeys,
            StageIntent::StepKey(0),
            StageIntent::Window(Step::Left),
        ] {
            assert!(
                intent.label().chars().count() <= 19,
                "label is too long: {}",
                intent.label()
            );
        }
    }

    #[test]
    fn page_keys_are_everywhere_except_the_typing_scopes_and_the_matrix() {
        for scope in ScopeContext::ALL {
            for (key, page) in [
                (Key::F1, PageKey::Trig),
                (Key::F2, PageKey::Src),
                (Key::F3, PageKey::Fltr),
                (Key::F4, PageKey::Amp),
                (Key::F5, PageKey::Lfo),
                (Key::F6, PageKey::Fx),
                (Key::F7, PageKey::Mix),
                (Key::F8, PageKey::All),
            ] {
                // The matrix keeps the deck down while it is up.
                let expected = (!matches!(
                    scope,
                    ScopeContext::Browser
                        | ScopeContext::Rename
                        | ScopeContext::Matrix
                        | ScopeContext::Lab
                        | ScopeContext::Kiln
                        | ScopeContext::KilnFilter
                        | ScopeContext::MidiLab
                        | ScopeContext::LabMenu
                ))
                .then_some(StageIntent::Page(page));
                assert_eq!(
                    dispatch(scope, StageInput::Chord(Mods::NONE, key)),
                    expected,
                    "{scope:?} / {key:?}"
                );
            }
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
                let narrow_within_wide =
                    (!narrow.shift || wide.shift) && (!narrow.command || wide.command);
                assert!(
                    !(narrow_within_wide && specificity(*narrow) < specificity(*wide)),
                    "{narrow:?}+{key:?} is offered before {wide:?}+{key:?} and would eat it"
                );
            }
        }
    }

    #[test]
    fn modulation_has_a_safe_global_door_and_its_own_x_verb() {
        let open = Mods::COMMAND.plus(Mods::SHIFT);
        for scope in [
            ScopeContext::Root,
            ScopeContext::Nested,
            ScopeContext::Browser,
            ScopeContext::Mixer,
            ScopeContext::Chain,
            ScopeContext::Clip,
            ScopeContext::Song,
            ScopeContext::Modulation,
        ] {
            assert_eq!(
                dispatch(scope, StageInput::Chord(open, Key::M)),
                Some(StageIntent::Modulation),
                "{scope:?} cannot reach the modulation workspace"
            );
        }
        for transactional in [
            ScopeContext::Rename,
            ScopeContext::TrigMenu,
            ScopeContext::Plock,
            ScopeContext::Sample,
            ScopeContext::Forge,
        ] {
            assert_eq!(
                dispatch(transactional, StageInput::Chord(open, Key::M)),
                None
            );
        }
        assert_eq!(
            dispatch(
                ScopeContext::Modulation,
                StageInput::Chord(Mods::NONE, Key::X)
            ),
            Some(StageIntent::ModToggleWire)
        );
    }
}
