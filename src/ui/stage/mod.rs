//! The stage — the third frame.
//!
//! `ui::redesign` is the second. This one starts from nothing on purpose:
//! the two before it grew by accretion, and the shape of a surface is very
//! hard to argue with once something is already drawn on it.
//!
//! **What this may depend on.** The vocabulary (`crate::intent`), the
//! model (`crate::sequencing`), the device cards (`crate::ui::device`) and
//! the design system (`theme`, `tokens`, `skin`, `glyph`, `kit`,
//! `legibility`). Those are frame-independent by construction and were
//! measured to be so — the card layer holds zero references to any frame.
//!
//! **What it may NOT depend on.** `ui::redesign`, or anything that reaches
//! back into a frame. A third frame borrowing the second one's parts is
//! how there come to be two answers to the same question. If something in
//! `redesign` is worth having here, it is worth lifting to a shared home
//! first — `crate::intent` is the worked example.
//!
//! The first thing drawn is FOCUS ITSELF: a calibration grid of
//! meaningless squares (see `grid`). Deliberately grayscale — colour is a
//! channel, and no channel is spent before the sync marker has proven
//! itself in luminance alone.

mod browser;
mod grid;
mod keymap;
mod scenes;
mod tracks;
mod transport;

use crate::design;
use crate::library::{LibraryConfig, LibraryService, LibrarySnapshot};
use crate::pitch::Pitch;
use crate::sequencing::{
    Clip, GRID_COLUMNS as PATTERN_COLS, GRID_ROWS as PATTERN_ROWS, PATTERN_STEP_TICKS,
    PATTERN_STEPS, PatternId, PitchAuthority, Song, TrackKind,
};
use crate::ui::glyph;
use crate::ui::sequencer::{self, grammar, lens, midi_typing, registers, sequence};
use eframe::egui;

use browser::{BrowserStatus, sample_nodes};

pub use browser::{Browser, EntryKind, Node, Row, Shelf};
pub use grid::{
    FocusColumn, FocusGrid, FocusLattice, FocusRow, FocusScope, FocusStack, Miniature, Step,
};
pub use keymap::StageIntent;
pub use scenes::Address;
pub use transport::{Motion, Place, Transport};

/// The calibration field. Big enough that a weak focus signal would let
/// the eye lose the cursor — which is the point of the test.
const GRID_COLS: usize = 8;
const GRID_ROWS: usize = 8;

/// Deep enough to test whether the eye keeps track of its level; the cap
/// itself is arbitrary calibration apparatus, not a design commitment.
const MAX_DEPTH: usize = 4;

/// The palette comes from the app's alphabet, not from here. The stage
/// names the ROLE it is drawing and the alphabet decides the value —
/// which is what keeps one palette rather than a fork of one.
const SQUARE: egui::Color32 = design::SURFACE.color;
const FOCUSED: egui::Color32 = design::FOCUS.color;
const REFUSAL: egui::Color32 = design::INK.color;
/// Where focus WILL be when it comes back, drawn while it is somewhere
/// else. Dimmer than focus by a whole rung, so the rule holds that exactly
/// one thing on the screen is ever focus-bright.
const RESTING: egui::Color32 = design::INK.color;

/// What the clip tray is drawn through while the cursor is not in it:
/// GROUND at a little over half strength, which takes the sequencer's
/// white below FOCUS without hiding anything it says.
const VEIL: egui::Color32 = egui::Color32::from_black_alpha(150);

/// The periphery: two fixed strips framing one sovereign field. Zones are
/// keyed by FUNCTION (ancestry, vitals, messages), never by content, and
/// they never take focus — the cursor lives only in the field.
///
/// The height is a LAYOUT dimension rather than a spacing rung: it was
/// settled by eye on the display this is read on, and the spacing ladder
/// is for the distances between things, not for how big a zone is.
const PERIPHERY_H: f32 = 64.0;

/// A track column. A layout dimension like [`BROWSER_W`], and fixed for
/// the same reason: geometry that resized itself with the track count
/// would move the ground under a cursor that had already learned where
/// each track lives.
const TRACK_W: f32 = 132.0;
const TRACK_H: f32 = 64.0;

/// The gutter left of the session where each scene's address is written.
/// A layout dimension: two digits of the smallest type and a breath, and
/// it is reserved whether or not there are scenes to number, so the
/// columns never shift when the first scene arrives.
const ADDRESS_W: f32 = 28.0;

/// The sign for a place with nothing in it: a point, the smallest mark
/// the surface can make. An empty slot is drawn as one point rather than
/// as a plane, so the session reads as clips on a ground instead of a
/// grid of squares — the lattice is still there, in the points' rank
/// and file, but it is the ground and the clips are the figure.
const POINT: f32 = 2.0;

/// The sequencer's band: a fixed tray along the foot of the field, where
/// the clip under the cursor is shown and, once entered, edited. A layout
/// dimension like [`PERIPHERY_H`]: tall enough for the grid at its
/// largest cell with the trig inspector beside it, and it does not move
/// or vanish — a band with no clip to show simply goes quiet, so the
/// session above it never changes shape when a clip opens or closes.
const CLIP_H: f32 = 272.0;

/// The widest the sequencer is drawn: the inspector, the grid at its
/// largest cell, and their gaps. Past this the band is left empty rather
/// than stretched, so the grid keeps a size the eye has learned.
const CLIP_W_MAX: f32 = 1080.0;

/// The browser's column. A layout dimension like [`PERIPHERY_H`]: wide
/// enough that a file name is a name rather than an ellipsis, and it does
/// not move, because a zone the eye has learned is only free to read while
/// it stays where it was learned.
const BROWSER_W: f32 = 320.0;

/// Where every zone sits, as a pure function of the window.
///
/// It takes NO state — not focus, not whether the browser is summoned,
/// not what is being shown. That is the whole point: every zone is a
/// fixed place the eye can learn once and read for free forever, and a
/// place that moves has to be found again every time. Zones go quiet, and
/// they go empty, but they never move and they never resize.
///
/// The browser OVERLAYS the field rather than dividing it. Both own the
/// same corner of the screen and neither yields any of it: the field is
/// laid out as though the browser did not exist, and the browser is drawn
/// over the top of it when summoned. Content beneath is hidden for a
/// moment, and hidden is not the same as moved — nothing has to be found
/// again when the browser goes away.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    vitals: egui::Rect,
    breadcrumb: egui::Rect,
    transport: egui::Rect,
    message: egui::Rect,
    browser: egui::Rect,
    /// The whole middle band: session and clip tray together. The
    /// codebook takes all of it.
    field: egui::Rect,
    /// The session: heads and the scene lattice.
    session: egui::Rect,
    /// The clip tray: the sequencer, or quiet.
    clip: egui::Rect,
}

impl Layout {
    fn of(whole: egui::Rect) -> Self {
        let vitals = egui::Rect::from_min_max(
            whole.min,
            egui::pos2(whole.max.x, whole.min.y + PERIPHERY_H),
        );
        // The time end is a fixed number of the design alphabet's largest
        // spacing cells. It never grows with the readout or meter, so a
        // changing song fact cannot move either half of the strip.
        let transport_w = design::px(design::space::VAST) * 8.0;
        let transport_x = (vitals.max.x - transport_w).max(vitals.min.x);
        let breadcrumb =
            egui::Rect::from_min_max(vitals.min, egui::pos2(transport_x, vitals.max.y));
        let transport = egui::Rect::from_min_max(egui::pos2(transport_x, vitals.min.y), vitals.max);
        let message = egui::Rect::from_min_max(
            egui::pos2(whole.min.x, whole.max.y - PERIPHERY_H),
            whole.max,
        );
        // The browser holds the left of the middle band, INSIDE the strips
        // rather than beside them: vitals and messages speak for the whole
        // app, while the browser is content and sits with the content.
        let band = egui::Rect::from_min_max(
            egui::pos2(whole.min.x, vitals.max.y),
            egui::pos2(whole.max.x, message.min.y),
        );
        let browser =
            egui::Rect::from_min_max(band.min, egui::pos2(band.min.x + BROWSER_W, band.max.y));
        // The clip tray is cut off the FOOT of the field, fixed: the
        // session above keeps the same shape whether or not the tray has
        // anything to show.
        let clip_top = (band.max.y - CLIP_H).max(band.min.y);
        let session = egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, clip_top));
        let clip = egui::Rect::from_min_max(egui::pos2(band.min.x, clip_top), band.max);
        Self {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field: band,
            session,
            clip,
        }
    }
}

/// The clip the cursor went into: which pattern, and from which track's
/// column — the track decides the pitch language the sequencer speaks.
/// Track is an index because tracks only ever append while a clip is
/// open; the pattern is an id because a clip cannot be cleared from
/// inside it, so the id stays valid for as long as the cursor is in there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Opened {
    pub pattern: PatternId,
    pub track: usize,
}

/// Why an intent could not change the stage state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalReason {
    /// A step with no neighbour in that direction in the active scope.
    Edge(Step),
    /// Enter at the depth cap: the focused square would not open.
    Deeper,
    /// Escape at the root: there is no further out.
    Shallower,
    /// Asked of a place that holds nothing yet.
    Empty,
    /// The song clock is already at its first tick.
    AtTop,
    /// The addressed leaf is real, but this scaffolding has no verb for it.
    Unavailable,
}

/// A rejected application. The intent remains attached to its reason so
/// future callers can report precisely what the stage declined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refusal {
    pub intent: StageIntent,
    pub reason: RefusalReason,
}

/// The exhaustive result of applying an intent: it changed the state or it
/// produced a drawable reason. There is no silent third outcome.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    Changed,
    Refused(Refusal),
}

/// Everything the stage knows. Every field added here should have to
/// justify itself — this is the mutable core that the rest of the surface
/// will be a pure function of.
pub struct Stage {
    focus: FocusStack,
    /// Canonical musical facts. Tempo and meter are read from here on every
    /// frame; the transport never keeps a second copy of either.
    song: Song,
    /// The green-zone clock mirror. A frame delta drives it until an engine
    /// can replace that source without changing anything downstream.
    transport: Transport,
    /// The browser, when it has been summoned. `Some` means focus is IN
    /// it: the browser is a place rather than a panel, so it either holds
    /// the cursor or it is not on screen at all.
    ///
    /// Not a level of [`Self::focus`], because it is not inside anything —
    /// descending into a track and going off to the library are different
    /// motions, and a stack that conflated them would draw a false
    /// ancestry.
    browser: Option<Browser>,
    /// Whether the codebook is showing. A summoned surface rather than
    /// standing chrome: a permanent key list is ambient information, and
    /// ambient information is what this stage spends its budget avoiding.
    help: bool,
    /// Latest refusal in this frame. Cleared at the next `show`, and drawn
    /// from one site after every key has been applied.
    refusal: Option<Refusal>,
    /// The first track the strip draws. Retained rather than derived
    /// because MINIMAL scrolling is a memory: where the view already sits
    /// decides whether it needs to move at all, and a view recomputed from
    /// the cursor alone could only recentre.
    strip_offset: usize,
    /// The first scene the lattice draws. The same minimal-scroll memory
    /// as [`Self::strip_offset`], on the other axis.
    scene_offset: usize,
    /// The clip the cursor is inside, while the nested level is the
    /// sequencer. `None` while the nested level is the calibration field
    /// or there is no nested level.
    inside: Option<Opened>,
    /// The sequencer — the same one the second frame draws, lifted to a
    /// neutral home. It owns its own step cursor, resolution and editor
    /// choice; the stage owns WHERE it is drawn and WHEN it has the keys.
    sequencer: sequence::SequencePanel,
    /// The grammar's sentence-in-progress and its registers: the frame's
    /// keyboard layer, above the panel, so a count started before a focus
    /// change does not vanish.
    sentence: grammar::Sentence,
    registers: registers::Registers,
    /// Letters as pitches, while `I` says so. Announced on the message
    /// strip, because a mode that does not announce itself is a trap.
    midi_typing: midi_typing::MidiTyping,
    /// A pitch typed this frame, waiting for the sequencer to enter it.
    entered_pitch: Option<Pitch>,
    /// The sequencer's last notice — a refused edit, in its own words —
    /// held on the message strip until the next edit replaces it.
    notice: Option<&'static str>,
    /// Green-zone scanner ownership and the last immutable answer it gave.
    /// The browser never walks the filesystem from a frame.
    library_service: LibraryService,
    library_snapshot: LibrarySnapshot,
    library_scanning: bool,
}

impl Default for Stage {
    fn default() -> Self {
        Self::with_library(LibraryConfig::default(), LibrarySnapshot::default())
    }
}

impl Stage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the neutral scanner while retaining a cached catalog to show
    /// until its newer answer arrives.
    pub fn with_library(config: LibraryConfig, cached: LibrarySnapshot) -> Self {
        let song = Song::default();
        Self {
            // The root is the session: the song's tracks across, the
            // scenes down, with the heads as the first row. The calibration
            // field remains what a head OPENS INTO: a track's content is
            // not designed yet, and an honest placeholder is better than a
            // surface that pretends to hold clips it cannot.
            focus: FocusStack::new(
                FocusScope::lattice(song.tracks.len(), scenes::lattice_rows(&song)),
                MAX_DEPTH,
            ),
            song,
            transport: Transport::new(),
            browser: None,
            help: false,
            refusal: None,
            strip_offset: 0,
            scene_offset: 0,
            inside: None,
            sequencer: sequence::SequencePanel::default(),
            sentence: grammar::Sentence::default(),
            registers: registers::Registers::default(),
            midi_typing: midi_typing::MidiTyping::default(),
            entered_pitch: None,
            notice: None,
            library_service: LibraryService::start(config),
            library_snapshot: cached,
            library_scanning: true,
        }
    }

    pub fn library_snapshot(&self) -> &LibrarySnapshot {
        &self.library_snapshot
    }

    /// Draw one frame: read the keyboard, advance time, paint the stage.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.refusal = None;
        self.poll_library(ui.ctx());
        let collect_text = self.browser.is_some();
        // While a sentence is being spoken, or the letters are pitches,
        // Escape is the grammar's: it abandons the sentence or leaves
        // pitch entry, and only a bare Escape leaves the clip.
        let grammar_owns_escape =
            self.inside.is_some() && (!self.sentence.is_empty() || self.midi_typing.enabled());
        let scope = self.scope_context();
        let inputs = ui.input_mut(|input| {
            let chords = keymap::consume_chords(input, scope, |modifiers, key| {
                grammar_owns_escape
                    && modifiers == egui::Modifiers::NONE
                    && key == egui::Key::Escape
            });
            let pressed = |wanted: egui::Key| chords.iter().any(|(_, key)| *key == wanted);
            let questionmark_consumed = pressed(egui::Key::Questionmark);
            let space_consumed = pressed(egui::Key::Space);
            let mut stage_inputs: Vec<keymap::StageInput> = chords
                .iter()
                .map(|(modifiers, key)| keymap::StageInput::Chord(*modifiers, *key))
                .collect();
            if collect_text {
                for event in &input.events {
                    let egui::Event::Text(text) = event else {
                        continue;
                    };
                    // A physical '?' is the codebook chord. egui also emits
                    // it as text; admitting both would make one keystroke do
                    // two things. Pasted '?' remains ordinary filter text.
                    if questionmark_consumed && text == "?" {
                        continue;
                    }
                    // Space is a global transport chord even while the
                    // browser owns text. As with '?', admit the physical
                    // key exactly once while leaving pasted whitespace
                    // inside longer text untouched.
                    if space_consumed && text == " " {
                        continue;
                    }
                    stage_inputs.extend(text.chars().map(keymap::StageInput::Text));
                }
            }
            stage_inputs
        });

        // A refused keystroke is drawn, not swallowed: under key repeat
        // the refusal re-arrives every frame, so pressing against a limit
        // reads as a held mark on that limit rather than as a dead key.
        for input in inputs {
            let _ = self.handle_input(input);
        }

        // Inside a clip, the letters may be pitches. This runs before the
        // sequencer draws so a typed note enters on the frame it was
        // typed, and after the stage's own chords so `^T` is never read
        // as a T.
        self.entered_pitch = match self.inside {
            Some(opened) if self.scope_context() == keymap::ScopeContext::Clip => {
                let mode = self.entry_mode(opened);
                self.midi_typing
                    .update(ui.ctx(), mode)
                    .entered
                    .map(|entered| match entered {
                        midi_typing::Entered::Midi(midi) => Pitch::from_midi(midi),
                        midi_typing::Entered::Degree { degree, period } => {
                            Pitch::degree(degree, period)
                        }
                    })
            }
            _ => None,
        };

        // The view follows the cursor BEFORE anything is painted, so the
        // frame that shows a move has already scrolled to contain it —
        // never a frame late, which would read as the mark leaving the
        // strip and coming back.
        let field = Layout::of(ui.available_rect_before_wrap()).session;
        let addressed = self.session_address();
        self.strip_offset = tracks::offset_following(
            self.strip_offset,
            addressed.map(Address::track),
            self.song.tracks.len(),
            Self::strip_capacity(field),
        );
        // The scene axis follows only while the cursor is ON a scene: a
        // cursor on a head is not in any row, and must not drag the rows.
        let scene = match addressed {
            Some(Address::Slot { scene, .. }) => Some(scene),
            _ => None,
        };
        self.scene_offset = tracks::offset_following(
            self.scene_offset,
            scene,
            self.song.session.scenes.len(),
            Self::scene_capacity(field),
        );

        // Same fallback clock as the legacy control plane: the UI's stable
        // frame delta while there is no engine, with continuous frames only
        // while time is actually passing.
        let seconds = f64::from(ui.ctx().input(|input| input.stable_dt));
        self.transport.advance(&self.song, seconds);
        if self.transport.motion().is_rolling() {
            ui.ctx().request_repaint();
        }

        self.draw(ui);
    }

    /// Where focus is standing, which is what every key is conditioned on.
    fn scope_context(&self) -> keymap::ScopeContext {
        if self.browser.is_some() {
            keymap::ScopeContext::Browser
        } else if self.inside.is_some() {
            keymap::ScopeContext::Clip
        } else if self.focus.depth() == 1 {
            keymap::ScopeContext::Root
        } else {
            keymap::ScopeContext::Nested
        }
    }

    /// The application's complete key path, shared by the headless sequence
    /// driver: keymap lookup, then intent application.
    #[cfg(test)]
    fn handle_key(&mut self, modifiers: egui::Modifiers, key: egui::Key) -> Option<ApplyOutcome> {
        self.handle_input(keymap::StageInput::Chord(modifiers, key))
    }

    fn handle_input(&mut self, input: keymap::StageInput) -> Option<ApplyOutcome> {
        let intent = keymap::dispatch(self.scope_context(), input)?;
        Some(self.apply(intent))
    }

    fn poll_library(&mut self, ctx: &egui::Context) {
        if let Some(snapshot) = self.library_service.newest_snapshot() {
            self.library_snapshot = snapshot;
            self.library_scanning = false;
            // The shelf is filled whether or not it happens to be open.
            // A tree keeps every shelf on screen, so "refresh only what is
            // being looked at" would leave a closed SAMPLES holding a
            // stale count that the reader can see.
            let nodes = sample_nodes(&self.library_snapshot.assets);
            if let Some(browser) = &mut self.browser {
                browser.set_children(Shelf::Samples, nodes, BrowserStatus::Ready);
            }
        }
        if self.library_scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// How many track columns the field can show. The strip's geometry is
    /// fixed by rule, so this is a pure function of the window and never of
    /// where the cursor stands.
    fn strip_capacity(field: egui::Rect) -> usize {
        let gap = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        let usable = field.width() - margin * 2.0 - ADDRESS_W + gap;
        if usable <= 0.0 {
            return 1;
        }
        ((usable / (TRACK_W + gap)).floor() as usize).max(1)
    }

    /// How many scene rows fit under the heads, and only whole ones.
    fn scene_capacity(field: egui::Rect) -> usize {
        let gap = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        let head_bottom = Self::head_rect(field, 0).max.y;
        scenes::rows_that_fit(field.max.y - margin - (head_bottom + gap), gap)
    }

    /// The scenes the lattice currently shows, as a range into the
    /// session's rows. The vertical twin of [`Self::strip_window`].
    fn scene_window(&self, field: egui::Rect) -> std::ops::Range<usize> {
        let count = self.song.session.scenes.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.scene_offset.min(count - 1);
        let last = first.saturating_add(Self::scene_capacity(field)).min(count);
        first..last
    }

    /// Where the cursor stands on the session, if it stands on it at all:
    /// `None` while focus is inside a track, in the browser, or the song
    /// has no tracks. Every question about "which track" or "which scene"
    /// the root cursor means is answered here and nowhere else.
    fn session_address(&self) -> Option<Address> {
        match self.focus.levels().first() {
            Some(FocusScope::Lattice(lattice)) => lattice.cursor().map(Address::of),
            _ => None,
        }
    }

    /// The session as drawn: the root lattice, and the shade its cursor
    /// takes. The session is on screen whenever the cursor is on it OR
    /// inside a clip beneath it — the tray below is where focus went, and
    /// the session's cursor is then RESTING: where focus will land when
    /// it comes back, one rung down from where it is. Inside a TRACK
    /// (the calibration field) the session is not drawn at all.
    fn session_lattice(&self) -> Option<(&FocusLattice, egui::Color32)> {
        let FocusScope::Lattice(lattice) = self.focus.levels().first()? else {
            return None;
        };
        match (self.focus.depth(), self.inside) {
            (1, _) => Some((lattice, FOCUSED)),
            (_, Some(_)) => Some((lattice, RESTING)),
            _ => None,
        }
    }

    /// The session address only while the session is where the keyboard
    /// IS: not from inside a track, not from the browser. A verb on a
    /// slot must not fire on a cell the performer is not looking at.
    fn standing_on(&self) -> Option<Address> {
        (self.browser.is_none() && self.focus.depth() == 1)
            .then(|| self.session_address())
            .flatten()
    }

    /// The pitch language typed letters speak inside `opened`: the
    /// track's authority reads the ambient key, exactly as the second
    /// frame decides it. Local shadows global.
    fn entry_mode(&self, opened: Opened) -> midi_typing::EntryMode {
        match self
            .song
            .tracks
            .get(opened.track)
            .map(|track| track.pitch_authority)
        {
            Some(PitchAuthority::Degree) => midi_typing::EntryMode::Degree {
                degrees: self.song.key.degree_count(),
            },
            _ => midi_typing::EntryMode::Chromatic,
        }
    }

    /// The clip the tray shows: the one the cursor is INSIDE, or else the
    /// one the session cursor is resting on. A head or an empty slot
    /// shows nothing, and the tray goes quiet rather than showing a clip
    /// the cursor is not on.
    fn clip_in_view(&self) -> Option<Opened> {
        self.inside.or_else(|| match self.session_address()? {
            Address::Slot { track, scene } => {
                let Clip::Pattern(pattern) = self.song.slot_clip(track, scene)?;
                Some(Opened { pattern, track })
            }
            Address::Head { .. } => None,
        })
    }

    /// The sequencer's edits land on the open pattern through the model's
    /// own applier — the one the second frame's bridge uses, so a nudge
    /// means the same thing in both frames. A refusal is kept for the
    /// message strip; a frame with no edits leaves the last one standing.
    fn apply_sequence(&mut self, id: PatternId, intents: &[sequence::Intent]) {
        if intents.is_empty() {
            return;
        }
        self.notice = self
            .song
            .pattern_mut(id)
            .and_then(|pattern| pattern.apply_all(intents));
    }

    /// The tracks the strip currently shows, as a range into the song's
    /// order. `show` has already moved the window to contain the cursor,
    /// so drawing never decides where to look — it only draws what was
    /// decided. Everything that lines up under a head asks here, so the
    /// strip and the lattice can never disagree about which tracks are on
    /// screen.
    fn strip_window(&self, field: egui::Rect) -> std::ops::Range<usize> {
        let count = self.song.tracks.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.strip_offset.min(count - 1);
        let last = first.saturating_add(Self::strip_capacity(field)).min(count);
        first..last
    }

    /// The head of the `slot`th shown track. The one place a column's
    /// geometry is decided: the lattice takes this rectangle and stacks
    /// beneath it, so a slot is the width of its head by construction
    /// rather than by agreement.
    fn head_rect(field: egui::Rect, slot: usize) -> egui::Rect {
        let gap = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        egui::Rect::from_min_size(
            egui::pos2(
                field.min.x + margin + ADDRESS_W + slot as f32 * (TRACK_W + gap),
                field.min.y + margin,
            ),
            egui::vec2(TRACK_W, TRACK_H),
        )
    }

    /// Apply one semantic request to stage state. A refusal is both returned
    /// and retained in the frame channel; successful later intents do not
    /// erase it, so the slot always holds the latest refusal this frame.
    pub fn apply(&mut self, intent: StageIntent) -> ApplyOutcome {
        let result = match intent {
            // Movement goes wherever focus is standing. There is exactly
            // one cursor in the app, and this is the only place it moves.
            StageIntent::Step(step) => match &mut self.browser {
                Some(browser) => browser
                    .step(step)
                    .then_some(())
                    .ok_or(if browser.is_empty() {
                        RefusalReason::Empty
                    } else {
                        RefusalReason::Edge(step)
                    }),
                None => self
                    .focus
                    .step(step)
                    .then_some(())
                    .ok_or(RefusalReason::Edge(step)),
            },
            // Making a track is a document edit. The strip is the scope
            // that STANDS FOR the document's tracks, so it follows the
            // song rather than being rebuilt beside it.
            StageIntent::NewAudioTrack | StageIntent::NewInstrumentTrack => {
                let kind = match intent {
                    StageIntent::NewAudioTrack => TrackKind::Audio,
                    _ => TrackKind::Instrument,
                };
                self.song.add_track(kind);
                let last = self.song.tracks.len().saturating_sub(1);
                // Focus follows the new track only when the strip is where
                // the cursor is standing. From inside a track, moving an
                // ancestor's cursor would relocate a context the performer
                // is not looking at, and would land somewhere else on the
                // way back out.
                let standing_on_the_session = self.standing_on().is_some();
                if let FocusScope::Lattice(lattice) = self.focus.root_mut() {
                    lattice.resize_cols(last + 1);
                    if standing_on_the_session {
                        // The column changes; the row does not. A performer
                        // on scene four who makes a track is still on scene
                        // four, in the slot the new track just gave it.
                        lattice.focus_col(last);
                    }
                }
                Ok(())
            }
            StageIntent::Enter => match self
                .browser
                .as_ref()
                .and_then(Browser::selected)
                .map(|node| node.is_branch())
            {
                // A heading holds rows and does nothing else, so opening
                // it is the only thing pressing it could honestly mean.
                Some(true) => {
                    if let Some(browser) = &mut self.browser {
                        browser.toggle();
                    }
                    Ok(())
                }
                Some(false) => Err(RefusalReason::Unavailable),
                None if self.browser.is_some() => Err(RefusalReason::Empty),
                // Going into a FILLED slot opens its pattern: the step
                // grid, sixteen by four. Going into an empty slot makes it
                // a place first — a new, empty pattern lands there — and
                // the next Enter opens it. An audio slot has no pattern
                // to take and is refused.
                None => match self.standing_on() {
                    Some(Address::Slot { track, scene }) => {
                        match self.song.slot_clip(track, scene) {
                            Some(Clip::Pattern(pattern)) => {
                                // The level mirrors the sequencer's own
                                // sixteen-by-four cursor, for the breadcrumb.
                                let entered = self
                                    .focus
                                    .enter(FocusScope::grid(PATTERN_COLS, PATTERN_ROWS));
                                if entered {
                                    self.inside = Some(Opened { pattern, track });
                                }
                                entered.then_some(()).ok_or(RefusalReason::Deeper)
                            }
                            None => self
                                .song
                                .fill_slot(track, scene)
                                .map(|_| ())
                                .ok_or(RefusalReason::Unavailable),
                        }
                    }
                    // Inside a clip the sequencer's grammar owns Enter (it
                    // is ACT there); the stage never binds it, and refuses
                    // it if asked directly.
                    _ if self.inside.is_some() => Err(RefusalReason::Unavailable),
                    _ => self
                        .focus
                        .enter(FocusScope::grid(GRID_COLS, GRID_ROWS))
                        .then_some(())
                        .ok_or(RefusalReason::Deeper),
                },
            },
            // Clearing is a slot's verb and nothing else's: on a head, from
            // inside a track, or in the browser there is nothing it could
            // honestly mean. An empty slot is not an error, but it is not
            // a change either, and says so.
            StageIntent::Clear => match self.standing_on() {
                Some(Address::Slot { track, scene }) => self
                    .song
                    .clear_slot(track, scene)
                    .map(|_| ())
                    .ok_or(RefusalReason::Empty),
                _ => Err(RefusalReason::Unavailable),
            },
            // Escape is one meaning everywhere: up and OUT. It leaves
            // whatever is outermost — the codebook first, then the
            // browser, then a scope.
            StageIntent::Escape => {
                if self.help {
                    self.help = false;
                    Ok(())
                } else if self.browser.is_some() {
                    // Climbing the tree is Left's job now. Escape keeps
                    // its one meaning — leave the outermost thing — and
                    // never doubles as a second, quieter way to move.
                    self.browser = None;
                    Ok(())
                } else {
                    let left = self.focus.escape();
                    // Leaving the step grid leaves the pattern behind: the
                    // id is only meaningful while a level stands for it.
                    if left && self.focus.depth() == 1 {
                        self.inside = None;
                        self.notice = None;
                    }
                    left.then_some(()).ok_or(RefusalReason::Shallower)
                }
            }
            StageIntent::ToggleTransport => {
                match self.transport.motion() {
                    Motion::Stopped => self.transport.set_motion(Motion::Rolling),
                    // Space is the shortest emergency path out of a write;
                    // it never silently demotes recording into ordinary roll.
                    Motion::Rolling | Motion::Recording => self.transport.stop(),
                }
                Ok(())
            }
            StageIntent::Rewind => {
                let before = self.transport;
                self.transport.rewind();
                (self.transport != before)
                    .then_some(())
                    .ok_or(RefusalReason::AtTop)
            }
            StageIntent::Help => {
                self.help = !self.help;
                Ok(())
            }
            // Summoning the browser takes focus with it; dismissing gives
            // focus back exactly where it was, because the field's cursor
            // was never touched.
            StageIntent::Browse => {
                self.browser = match self.browser {
                    Some(_) => None,
                    // The library opens at its shelves. What is on them
                    // is not scanned yet.
                    None => Some(Browser::shelves()),
                };
                Ok(())
            }
            StageIntent::TypeChar(ch) => self
                .browser
                .as_mut()
                .map(|browser| browser.type_char(ch))
                .ok_or(RefusalReason::Unavailable),
            StageIntent::Backspace => self
                .browser
                .as_mut()
                .ok_or(RefusalReason::Unavailable)
                .and_then(|browser| {
                    browser
                        .backspace()
                        .then_some(())
                        .ok_or(RefusalReason::Empty)
                }),
        };
        match result {
            Ok(()) => ApplyOutcome::Changed,
            Err(reason) => {
                let refusal = Refusal { intent, reason };
                self.refusal = Some(refusal);
                ApplyOutcome::Refused(refusal)
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter().clone();

        // The constitution: a thin fixed periphery around one sovereign
        // field. The strips hold display only; nothing in them is ever
        // focusable, and their geometry never changes.
        let Layout {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field,
            session,
            clip,
        } = Layout::of(whole);

        // Separation is a STEP IN VALUE, not a rule drawn between things.
        // Two planes that differ in lightness are already divided; a line
        // laid along the seam restates a boundary the eye has read, and a
        // screen full of restatements is the noise floor this surface
        // spends its budget keeping down.
        //
        // The consequence is worth the trade: a stroke now always MEANS
        // something. Every line left on this surface is a sign — a gate, a
        // refusal, the signature — and none of them is furniture.
        painter.rect_filled(vitals, 0.0, design::SURFACE.color);
        painter.rect_filled(message, 0.0, design::SURFACE.color);

        self.draw_breadcrumb(&painter, breadcrumb);
        self.draw_transport(&painter, transport);
        self.draw_message(&painter, message);
        // The codebook takes the whole field while it is up. It is a
        // DISPLAY mode, not a scope: focus never enters it, and the
        // cursor underneath is exactly where it was left.
        if self.help {
            self.draw_help(&painter, field);
        } else {
            // Stacked: the session above, the clip tray below. The tray
            // shows whatever clip the session cursor is on, and is only
            // FOCUSED once entered — Ableton's session over its clip
            // detail, an Elektron's track keys over its trig keys.
            self.draw_field(&painter, session);
            self.draw_clip(ui, clip);
        }
        // Last, and over the top of everything in the field: the browser
        // is a window above the work, not a division of it.
        self.draw_browser(&painter, browser);
    }

    /// The clip tray. The sequencer draws the clip in view — the stage
    /// builds what it reads (notes resolved against the key, the track's
    /// lens) — and has the keys only while the cursor is inside. While
    /// focus is elsewhere the tray is drawn and then veiled, so the
    /// sequencer's own white stays below the one focus-bright thing on
    /// the screen, and lands whatever it asked for on the pattern.
    fn draw_clip(&mut self, ui: &mut egui::Ui, tray: egui::Rect) {
        let Some(shown) = self.clip_in_view() else {
            return;
        };
        let Some(pattern) = self.song.pattern(shown.pattern) else {
            return;
        };
        let lens_name = match self.entry_mode(shown) {
            midi_typing::EntryMode::Degree { .. } => "degrees",
            midi_typing::EntryMode::Chromatic => "notes",
        };
        let lens_view = lens::LensView::resolve(lens_name, &self.song.key, &|_| None);
        let notes = sequencer::note_views(pattern, &self.song.key);
        let name = pattern.name.clone();
        let clip = sequence::ClipView {
            id: shown.pattern.0,
            name: &name,
            length_ticks: sequencer::pattern_length(&self.song, shown.pattern),
            notes: &notes,
            ghosts: &[],
        };
        let focused = self.inside.is_some() && self.browser.is_none();

        // The sequencer sits at the tray's left, inside the field's own
        // margin, and no wider than its natural width: a grid that
        // stretched with the window would be a grid the eye relearns.
        let margin = design::px(design::space::ROOM);
        let area = egui::Rect::from_min_max(
            egui::pos2(tray.min.x + margin, tray.min.y),
            egui::pos2(
                (tray.max.x - margin).min(tray.min.x + margin + CLIP_W_MAX),
                tray.max.y,
            ),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).id_salt("stage-clip"));
        let outcome = self.sequencer.show(
            &mut child,
            focused,
            grammar::Voice {
                sentence: &mut self.sentence,
                registers: &mut self.registers,
            },
            self.entered_pitch.take(),
            Some(clip),
            &lens_view,
        );
        if focused {
            self.apply_sequence(shown.pattern, &outcome.intents);
            // The level under the cursor mirrors the sequencer's step, so
            // the ancestry strip tells the truth about where the performer is.
            if let Some(tick) = outcome.cursor_tick {
                let step = (tick / PATTERN_STEP_TICKS).min(PATTERN_STEPS - 1);
                if let FocusScope::Grid(grid) = self.focus.active_mut() {
                    grid.set_cursor(step % PATTERN_COLS, step / PATTERN_COLS);
                }
            }
        } else {
            // A veil, not a repaint: the tray keeps every mark it would
            // have, one step down in value. Exactly one thing on the
            // screen is focus-bright, and while the cursor is on the
            // session that thing is the cursor.
            ui.painter().rect_filled(area, 0.0, VEIL);
            // A click in the tray is the pointer's way in, the same
            // road Enter takes.
            if outcome.claim_focus && self.browser.is_none() {
                let _ = self.apply(StageIntent::Enter);
            }
        }
    }

    fn draw_field(&self, painter: &egui::Painter, avail: egui::Rect) {
        if let Some((lattice, cursor_shade)) = self.session_lattice() {
            self.draw_tracks(painter, avail, lattice, cursor_shade);
            self.draw_scenes(painter, avail, lattice, cursor_shade);
            return;
        }

        let FocusScope::Grid(active) = self.focus.active() else {
            return;
        };
        let cols = active.cols() as f32;
        let rows = active.rows() as f32;

        // The grid sits centred at a fixed aspect: cells are square, the
        // gap scales with the cell, and nothing about the layout ever
        // depends on where focus is — geometry is constant by rule.
        let cell = ((avail.width() / cols).min(avail.height() / rows) * 0.82).floor();
        let gap = (cell * 0.14).floor().max(2.0);
        let span_x = cols * cell + (cols - 1.0) * gap;
        let span_y = rows * cell + (rows - 1.0) * gap;
        let origin = egui::pos2(
            (avail.center().x - span_x / 2.0).floor(),
            (avail.center().y - span_y / 2.0).floor(),
        );

        // Exactly one thing on the screen is ever FOCUS-bright. While the
        // browser holds the cursor, the field keeps a RESTING mark instead
        // — where focus will land when it comes back, not where it is.
        let cursor_shade = if self.browser.is_some() {
            RESTING
        } else {
            FOCUSED
        };

        let (focus_col, focus_row) = active.cursor();
        for row in 0..active.rows() {
            for col in 0..active.cols() {
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(col as f32 * (cell + gap), row as f32 * (cell + gap)),
                    egui::vec2(cell, cell),
                );
                let shade = if (col, row) == (focus_col, focus_row) {
                    cursor_shade
                } else {
                    SQUARE
                };
                painter.rect_filled(rect, 0.0, shade);
            }
        }

        // The absorbed keystroke, present only on frames where a refusal
        // happened, in the field's own geometry.
        let field = egui::Rect::from_min_size(origin, egui::vec2(span_x, span_y));
        let focused = egui::Rect::from_min_size(
            origin
                + egui::vec2(
                    focus_col as f32 * (cell + gap),
                    focus_row as f32 * (cell + gap),
                ),
            egui::vec2(cell, cell),
        );
        self.draw_grid_refusal(painter, field, focused, gap.max(4.0));
    }

    /// The absorbed keystroke in a grid's geometry. Each limit refuses in
    /// its own shape: an edge marks its side, the root marks the whole
    /// field, the depth cap marks the square that would not open.
    fn draw_grid_refusal(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        focused: egui::Rect,
        inset: f32,
    ) {
        match self.refusal.map(|refusal| refusal.reason) {
            Some(RefusalReason::Edge(step)) => {
                let (a, b) = match step {
                    Step::Up => (
                        egui::pos2(field.left(), field.top() - inset),
                        egui::pos2(field.right(), field.top() - inset),
                    ),
                    Step::Down => (
                        egui::pos2(field.left(), field.bottom() + inset),
                        egui::pos2(field.right(), field.bottom() + inset),
                    ),
                    Step::Left => (
                        egui::pos2(field.left() - inset, field.top()),
                        egui::pos2(field.left() - inset, field.bottom()),
                    ),
                    Step::Right => (
                        egui::pos2(field.right() + inset, field.top()),
                        egui::pos2(field.right() + inset, field.bottom()),
                    ),
                };
                painter.line_segment([a, b], egui::Stroke::new(2.0, REFUSAL));
            }
            Some(RefusalReason::Shallower) => {
                painter.rect_stroke(
                    field.expand(inset),
                    0.0,
                    egui::Stroke::new(2.0, REFUSAL),
                    egui::StrokeKind::Outside,
                );
            }
            Some(RefusalReason::Deeper) => {
                painter.rect_stroke(
                    focused.expand((inset * 0.5).max(2.0)),
                    0.0,
                    egui::Stroke::new(2.0, REFUSAL),
                    egui::StrokeKind::Outside,
                );
            }
            // A refusal that happened in the browser has no geometry in
            // the field — it belongs to the other side of the screen, and
            // the message strip is where it is reported. An empty or
            // unavailable verb on a step is reported the same way.
            Some(RefusalReason::Empty | RefusalReason::AtTop | RefusalReason::Unavailable)
            | None => {}
        }
    }

    /// The track strip: every track in the song, across the top of the
    /// field, one column each. Identity only — name and kind — because a
    /// surface has to say what its objects ARE before it can say what they
    /// are doing.
    fn draw_tracks(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        lattice: &FocusLattice,
        cursor_shade: egui::Color32,
    ) {
        let heads = tracks::heads(&self.song);
        if heads.is_empty() {
            return;
        }

        let gap = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        let pad = design::px(design::space::STEP);
        let name_font = egui::FontId::monospace(design::px(design::type_scale::BODY));
        let kind_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let top = field.min.y + margin;

        let window = self.strip_window(field);
        let (first, last) = (window.start, window.end);
        let shown = &heads[window];

        for (slot, head) in shown.iter().enumerate() {
            let index = first + slot;
            let rect = Self::head_rect(field, slot);
            let focused = lattice.cursor() == Some((index, 0));
            // No outline: the column is a lighter plane than the field
            // it sits on, and the gap between columns is the field showing
            // through. The edge was drawing a boundary the value already
            // drew.
            painter.rect_filled(rect, 0.0, if focused { cursor_shade } else { SQUARE });

            // The focused column inverts, exactly as the field's cursor
            // inverts: one signal drawn one way everywhere. Within the
            // column the name outranks the kind on both sides of the
            // inversion, so the reading order survives it.
            let (name_ink, kind_ink) = if focused {
                (design::GROUND.color, design::SURFACE.color)
            } else {
                (design::INK.color, design::EDGE.color)
            };
            painter.text(
                egui::pos2(rect.min.x + pad, rect.min.y + pad),
                egui::Align2::LEFT_TOP,
                &head.name,
                name_font.clone(),
                name_ink,
            );
            painter.text(
                egui::pos2(rect.min.x + pad, rect.max.y - pad),
                egui::Align2::LEFT_BOTTOM,
                head.kind,
                kind_font.clone(),
                kind_ink,
            );
            // The column's address, the way the sequencer numbers its
            // rows: a track is a channel with a number before it has a
            // name, and the number is what a held key will one day say.
            // On the kind's line, not the name's: a name may run the
            // whole width, and the number must never be under it.
            painter.text(
                egui::pos2(rect.max.x - pad, rect.max.y - pad),
                egui::Align2::RIGHT_BOTTOM,
                format!("{:02}", index + 1),
                kind_font.clone(),
                kind_ink,
            );
        }

        // The absorbed keystroke, in the session's own geometry: the
        // heads and every drawn scene row together, because the cursor
        // can be refused at the bottom of the lattice as well as at the
        // top of the strip. Every edge is drawn, because a swallowed key
        // is indistinguishable from a broken one.
        let drawn = shown.len() as f32;
        let rows = self.scene_window(field).len() as f32;
        let span = egui::Rect::from_min_size(
            egui::pos2(field.min.x + margin, top),
            egui::vec2(
                drawn * TRACK_W + (drawn - 1.0) * gap,
                TRACK_H + rows * (scenes::SLOT_H + gap),
            ),
        );
        let inset = gap.max(4.0);

        // Tracks the window is not showing. A SHORT tick, where a refusal
        // is a full-height rule: the two can never appear on the same edge
        // at the same time — a step toward hidden tracks scrolls instead of
        // refusing — but they are still different marks, because a
        // performer must not have to reason about which one they are
        // looking at.
        let elsewhere = TRACK_H / 3.0;
        let middle = top + TRACK_H / 2.0;
        if first > 0 {
            painter.line_segment(
                [
                    egui::pos2(span.left() - inset, middle - elsewhere / 2.0),
                    egui::pos2(span.left() - inset, middle + elsewhere / 2.0),
                ],
                egui::Stroke::new(2.0, design::INK.color),
            );
        }
        if last < heads.len() {
            painter.line_segment(
                [
                    egui::pos2(span.right() + inset, middle - elsewhere / 2.0),
                    egui::pos2(span.right() + inset, middle + elsewhere / 2.0),
                ],
                egui::Stroke::new(2.0, design::INK.color),
            );
        }
        match self.refusal.map(|refusal| refusal.reason) {
            Some(RefusalReason::Edge(step)) => {
                let (a, b) = match step {
                    Step::Up => (
                        egui::pos2(span.left(), span.top() - inset),
                        egui::pos2(span.right(), span.top() - inset),
                    ),
                    Step::Down => (
                        egui::pos2(span.left(), span.bottom() + inset),
                        egui::pos2(span.right(), span.bottom() + inset),
                    ),
                    Step::Left => (
                        egui::pos2(span.left() - inset, span.top()),
                        egui::pos2(span.left() - inset, span.bottom()),
                    ),
                    Step::Right => (
                        egui::pos2(span.right() + inset, span.top()),
                        egui::pos2(span.right() + inset, span.bottom()),
                    ),
                };
                painter.line_segment([a, b], egui::Stroke::new(2.0, REFUSAL));
            }
            Some(RefusalReason::Shallower) => {
                painter.rect_stroke(
                    span.expand(inset),
                    0.0,
                    egui::Stroke::new(2.0, REFUSAL),
                    egui::StrokeKind::Outside,
                );
            }
            _ => {}
        }
    }

    /// The scene lattice: one slot per (shown track, scene), stacked under
    /// the heads. Faint by construction — a resting slot is a SURFACE
    /// plane on the GROUND, the same step in value a head is, and the
    /// dimmest mark the alphabet has. It holds nothing yet, so it says
    /// nothing louder than "a place exists here".
    fn draw_scenes(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        lattice: &FocusLattice,
        cursor_shade: egui::Color32,
    ) {
        let gap = design::px(design::space::SNUG);
        let pad = design::px(design::space::STEP);
        let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let tracks = self.strip_window(field);
        let rows = self.scene_window(field);
        if tracks.is_empty() || rows.is_empty() {
            return;
        }

        // The focused slot inverts, exactly as a head does: one signal,
        // drawn one way, wherever the cursor stands on the session.
        let focused = lattice.cursor().map(Address::of);
        let focused_scene = match focused {
            Some(Address::Slot { scene, .. }) => Some(scene),
            _ => None,
        };

        // The scene addresses, in the gutter: the row's number, lit when
        // the cursor is on that row and quiet otherwise. The heads get no
        // number here because their number is on them.
        let gutter_x = Self::head_rect(field, 0).min.x - gap;
        for (line, scene) in rows.clone().enumerate() {
            let rect = scenes::slot_beneath(Self::head_rect(field, 0), line, gap);
            let ink = if focused_scene == Some(scene) {
                design::INK.color
            } else {
                design::EDGE.color
            };
            painter.text(
                egui::pos2(gutter_x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{:02}", scene + 1),
                font.clone(),
                ink,
            );
        }

        for (slot, track) in tracks.clone().enumerate() {
            let head = Self::head_rect(field, slot);
            for (line, scene) in rows.clone().enumerate() {
                let rect = scenes::slot_beneath(head, line, gap);
                let here = focused == Some(Address::Slot { track, scene });
                let mark = scenes::mark(&self.song, track, scene);

                // Three states, three values. The cursor is a plane in the
                // focus shade. A clip is a plane one rung up from the
                // ground. An empty place is a point — the lattice shows
                // through as rank and file, and nothing else.
                if here {
                    painter.rect_filled(rect, 0.0, cursor_shade);
                } else if mark.is_some() {
                    painter.rect_filled(rect, 0.0, SQUARE);
                } else {
                    painter.rect_filled(
                        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(POINT)),
                        0.0,
                        design::EDGE.color,
                    );
                }

                // A filled slot: its kind at the left, its number at the
                // right, one quiet line between them. The sign is a rung
                // below the number — the number is the address, the sign
                // only says what kind of thing lives there. Both invert
                // with the fill so the mark survives being focused.
                let Some(mark) = mark else {
                    continue;
                };
                let (sign_ink, number_ink) = if here {
                    (design::SURFACE.color, design::GROUND.color)
                } else {
                    (design::EDGE.color, design::INK.color)
                };
                painter.text(
                    egui::pos2(rect.min.x + pad, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    mark.glyph.to_string(),
                    font.clone(),
                    sign_ink,
                );
                painter.text(
                    egui::pos2(rect.max.x - pad, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    &mark.number,
                    font.clone(),
                    number_ink,
                );
            }
        }

        // Scenes the window is not showing, marked the way hidden tracks
        // are: a short tick past the edge they lie beyond.
        let first_slot = scenes::slot_beneath(Self::head_rect(field, 0), 0, gap);
        let last_slot = scenes::slot_beneath(Self::head_rect(field, 0), rows.len() - 1, gap);
        let elsewhere = TRACK_W / 3.0;
        let middle = first_slot.center().x;
        let inset = gap.max(4.0);
        if rows.start > 0 {
            painter.line_segment(
                [
                    egui::pos2(middle - elsewhere / 2.0, first_slot.top() - inset),
                    egui::pos2(middle + elsewhere / 2.0, first_slot.top() - inset),
                ],
                egui::Stroke::new(2.0, design::INK.color),
            );
        }
        if rows.end < self.song.session.scenes.len() {
            painter.line_segment(
                [
                    egui::pos2(middle - elsewhere / 2.0, last_slot.bottom() + inset),
                    egui::pos2(middle + elsewhere / 2.0, last_slot.bottom() + inset),
                ],
                egui::Stroke::new(2.0, design::INK.color),
            );
        }
    }

    /// The ancestry zone: one miniature grid per level above the active
    /// one, root first, each with its entered square lit. It lives at the
    /// left end of the vitals strip — a constant home the eye learns once;
    /// at the root the strip is simply empty, which itself reads as "top
    /// level, all quiet".
    fn draw_breadcrumb(&self, painter: &egui::Painter, zone: egui::Rect) {
        const MINI_CELL: f32 = 5.0;
        const MINI_GAP: f32 = 1.0;
        const MARGIN: f32 = 16.0;
        const SPACING: f32 = 12.0;

        let levels = self.focus.levels();
        let ancestors = &levels[..levels.len() - 1];
        // Every shape draws through its miniature, so an ancestor the eye
        // must account for is never silently skipped — a level that drew
        // nothing would report "top level, all quiet" from inside a
        // descent, which is a false ancestry rather than a missing mark.
        let tallest = ancestors
            .iter()
            .map(|scope| scope.miniature().rows)
            .max()
            .unwrap_or(0);
        if tallest == 0 {
            return;
        }

        let mini_h = tallest as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP;
        let top = (zone.center().y - mini_h / 2.0).floor();
        let mut corner = egui::pos2(zone.min.x + MARGIN, top);
        for scope in ancestors {
            let level = scope.miniature();
            if level.cols == 0 || level.rows == 0 {
                continue;
            }
            let span = egui::vec2(
                level.cols as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
                level.rows as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
            );
            // Shorter shapes sit centred against the tallest, so the strip
            // reads as one row of levels rather than a ragged top edge.
            corner.y = (top + (mini_h - span.y) / 2.0).floor();
            for row in 0..level.rows {
                for col in 0..level.cols {
                    let rect = egui::Rect::from_min_size(
                        corner
                            + egui::vec2(
                                col as f32 * (MINI_CELL + MINI_GAP),
                                row as f32 * (MINI_CELL + MINI_GAP),
                            ),
                        egui::vec2(MINI_CELL, MINI_CELL),
                    );
                    let shade = if (col, row) == (level.col, level.row) {
                        FOCUSED
                    } else {
                        SQUARE
                    };
                    painter.rect_filled(rect, 0.0, shade);
                }
            }
            corner.x += span.x + SPACING;
        }
    }

    /// The time end of the vitals strip. It reports but never acts: the
    /// keyboard moves time, and focus remains in the sovereign field.
    fn draw_transport(&self, painter: &egui::Painter, zone: egui::Rect) {
        let place = self.transport.place(&self.song);
        let readout = place.readout();
        let beats = beat_cells(place);
        let tempo = format!(
            "{:.0}",
            self.song
                .bpm_at(self.transport.tick(), transport::DEFAULT_BPM)
        );

        let body = egui::FontId::monospace(design::px(design::type_scale::BODY));
        let quiet = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let margin = design::px(design::space::ROOM);
        let gap = design::px(design::space::ROOM);
        let center_y = zone.center().y;
        let mut right = zone.max.x - margin;

        let tempo_rect = painter.text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            tempo,
            quiet,
            design::INK.color,
        );
        right = tempo_rect.min.x - gap;

        let beat_color = if self.transport.motion().is_rolling() {
            design::LIVE.color
        } else {
            design::INK.color
        };
        let beat_rect = painter.text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            beats,
            body.clone(),
            beat_color,
        );
        right = beat_rect.min.x - gap;

        let readout_rect = painter.text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            readout,
            body.clone(),
            design::INK.color,
        );

        if self.transport.motion() == Motion::Recording {
            painter.text(
                egui::pos2(readout_rect.min.x - gap, center_y),
                egui::Align2::RIGHT_CENTER,
                "REC",
                body,
                design::JEOPARDY_ACTIVE.color,
            );
        }
    }

    /// The message zone: the frame's refusal named in words, in the same
    /// gray as the geometric marks and gone the same frame they are. Empty
    /// is the normal state — this strip earns ink only when something was
    /// declined (and later: confirmed, landed, or failed).
    /// The browser: a window that opens ABOVE the work rather than
    /// alongside it.
    ///
    /// Its own opaque ground, so what is beneath is hidden rather than
    /// shining through, and one edge to say where it ends. Nothing under
    /// it moves — it covers the corner it covers and then gives it back.
    ///
    fn draw_browser(&self, painter: &egui::Painter, zone: egui::Rect) {
        let Some(browser) = &self.browser else {
            return;
        };
        // The browser is a PLANE above the work, and its lightness is what
        // says so. It needs no edge drawn along it: a raised surface is
        // already legible as one, and an outline would be the drawing
        // apologising for the value not being trusted.
        // A well, not a surface: the browser is a window cut into the
        // ground, and sits below the planes it covers rather than among them.
        painter.rect_filled(zone, 0.0, design::WELL.color);

        let line = design::px(design::type_scale::BODY);
        let font = egui::FontId::monospace(line);
        let inset = design::px(design::space::ROOM);
        let measure = painter.layout_no_wrap("M".to_owned(), font.clone(), design::INK.color);
        let cell = measure.size();
        let columns = ((zone.width() - inset * 2.0) / cell.x).floor() as usize;
        let rows = ((zone.height() - inset * 2.0) / cell.y).floor() as usize;
        if columns < 8 || rows < 5 {
            return;
        }

        let inner = columns - 2;
        let origin = egui::pos2(zone.min.x + inset, zone.min.y + inset);
        let at = |column: usize, row: usize| {
            origin + egui::vec2(column as f32 * cell.x, row as f32 * cell.y)
        };
        let text = |position: egui::Pos2, words: String, color: egui::Color32| {
            painter.text(position, egui::Align2::LEFT_TOP, words, font.clone(), color);
        };

        let bottom = rows - 1;

        // The pane's rules are DRAWN, not typed. Ruling characters are one
        // glyph per cell, so a border made of them is a row of separate
        // marks with a seam at every cell boundary and a baseline that is
        // not the cell's centre — at this size that reads as hatching
        // rather than as a line. `ui::glyph` already made this argument
        // for the family marks; the same reasoning ends at the same place.
        // A segment is one stroke, pixel-aligned, the same on every
        // machine, and independent of what the font happens to carry.
        let half = egui::vec2(cell.x / 2.0, cell.y / 2.0);
        let snap = |point: egui::Pos2| egui::pos2(point.x.round(), point.y.round());
        let frame =
            egui::Rect::from_min_max(snap(at(0, 0) + half), snap(at(columns - 1, bottom) + half));
        // What is being typed sits in a WELL: a step down from the plane
        // it is cut into, which divides it from the list without a rule
        // between them. A recess also says what the band is for — you
        // write into a surface, not onto one.
        let divider = snap(at(0, 2) + half).y;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(frame.left(), frame.top()),
                egui::pos2(frame.right(), divider),
            ),
            0.0,
            design::GROUND.color,
        );

        // The surface's ONE mute flourish, and the only mark in this pane
        // carrying neither command nor state — the precedent is the
        // transport's phase marks. It is the luminance ladder the pane is
        // drawn from, signed into the bottom rule: three rungs ascending.
        //
        // Two are missing, and their absence is the whole of it. GROUND is
        // the page it would be drawn on, and FOCUS is spent on the cursor,
        // which a signature is not allowed to borrow. A flourish that
        // shouted would be claiming importance it does not have.
        //
        // Recorded as a flourish because the charter permits exactly one
        // per surface and forbids a second: if another is ever wanted
        // here, this is the one that has to go.
        let rungs = [design::SURFACE, design::EDGE, design::INK];
        let pitch = design::px(design::space::SNUG);
        let rise = design::px(design::space::HAIR);
        let mut signature = egui::pos2(frame.right() - pitch * rungs.len() as f32, frame.bottom());
        for rung in rungs {
            painter.line_segment(
                [signature, egui::pos2(signature.x, signature.y - rise)],
                egui::Stroke::new(1.0, rung.color),
            );
            signature.x += pitch;
        }

        // The yield of what has been typed, in the row where it is being
        // typed. A keystroke earns its place by removing uncertainty, and
        // this is the only place the reader can see whether the last one
        // did — the moment the number stops falling, arrowing is cheaper
        // than typing.
        let yield_mark = if browser.query().is_empty() {
            String::new()
        } else {
            browser.surviving_leaves().to_string()
        };
        let typed = fit_cells(
            &format!("{} {}", browser::glyph::PROMPT, browser.query()),
            inner.saturating_sub(yield_mark.chars().count() + 1),
        );
        text(at(1, 1), typed, design::INK.color);
        if !yield_mark.is_empty() {
            let column = columns - 1 - yield_mark.chars().count();
            text(at(column, 1), yield_mark, design::EDGE.color);
        }

        // One drawable line per visible row, plus a NOTE under any open
        // shelf that has nothing to show. The note sits where the missing
        // rows would be, because a sign beside the thing it describes is
        // stronger than a status message detached from it.
        enum Line<'a> {
            Row(usize, &'a browser::Row, &'a Node),
            Note(usize, String),
        }

        let tree = browser.rows();
        let mut lines = Vec::new();
        for (index, row) in tree.iter().enumerate() {
            let Some(node) = browser.node_at(&row.path) else {
                continue;
            };
            lines.push(Line::Row(index, row, node));
            let EntryKind::Shelf(shelf) = node.kind else {
                continue;
            };
            if !node.expanded || !node.children.is_empty() {
                continue;
            }
            lines.push(Line::Note(
                row.depth + 1,
                match browser.status_of(shelf) {
                    BrowserStatus::Scanning => "Scanning".to_owned(),
                    BrowserStatus::Unavailable => "No source".to_owned(),
                    BrowserStatus::Ready => "Empty".to_owned(),
                },
            ));
        }
        if tree.is_empty() {
            lines.push(Line::Note(
                0,
                if browser.query().is_empty() {
                    "Empty".to_owned()
                } else {
                    "No match".to_owned()
                },
            ));
        }

        let visible = rows - 4;
        // Scroll by the LINE the cursor is on, so a note never pushes the
        // addressed row off the bottom.
        let addressed = browser.cursor().and_then(|cursor| {
            lines
                .iter()
                .position(|line| matches!(line, Line::Row(index, _, _) if *index == cursor))
        });
        let start = addressed
            .unwrap_or(0)
            .saturating_sub(visible / 2)
            .min(lines.len().saturating_sub(visible));

        for slot in 0..visible {
            let screen_row = 3 + slot;
            let Some(line) = lines.get(start + slot) else {
                continue;
            };
            match line {
                Line::Row(index, row, node) => {
                    let addressed = Some(*index) == browser.cursor();
                    if addressed {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                at(1, screen_row),
                                egui::vec2(inner as f32 * cell.x, cell.y),
                            ),
                            0.0,
                            FOCUSED,
                        );
                    }

                    // Depth is drawn, not implied: two cells per level, so
                    // the eye finds a heading's children by their left
                    // edge before reading a word of them.
                    let indent = row.depth * 2;

                    // The gate: two hairline segments, and whether it is
                    // open is said by WHERE the second one sits — across
                    // the middle while closed, dropped to the foot once
                    // open. The shape is the second frame's, which solved
                    // this before: an arrow glyph is a whole character of
                    // ink for one bit, and a column of them reads as a
                    // second margin competing with the labels.
                    //
                    // Copied rather than shared. `ui::kit` is the natural
                    // home for it, but every helper there takes a `Theme`
                    // and this frame carries none — and the frame it came
                    // from is the one being replaced. If the two ever have
                    // to agree, the SHAPE is the thing to lift.
                    let structure = if addressed {
                        design::SURFACE.color
                    } else {
                        design::EDGE.color
                    };
                    let content = if addressed {
                        design::GROUND.color
                    } else {
                        design::INK.color
                    };
                    if node.is_branch() {
                        let arm = design::px(design::space::HAIR);
                        let hinge =
                            at(1 + indent, screen_row) + egui::vec2(cell.x / 2.0, cell.y / 2.0);
                        let ink = egui::Stroke::new(1.0, structure);
                        let back = (arm * 0.75).round();
                        painter.line_segment(
                            [
                                hinge + egui::vec2(-back, -arm),
                                hinge + egui::vec2(-back, arm),
                            ],
                            ink,
                        );
                        let foot = if node.expanded { arm } else { 0.0 };
                        painter.line_segment(
                            [
                                hinge + egui::vec2(-back, foot),
                                hinge + egui::vec2(arm, foot),
                            ],
                            ink,
                        );
                    }

                    // A CLOSED branch says how much is behind it. Opening a
                    // heading to find one device is a keystroke that bought
                    // nothing, and the count is the only way to know before
                    // spending it. Open branches drop it: the rows beneath
                    // are the answer, and a mark that repeats what is
                    // already on screen is ornament.
                    // Zero is a count, not a gap. Suppressing it would
                    // make ABSENCE carry the meaning "empty", which reads
                    // identically to a mark that failed to draw — and an
                    // empty shelf is worth exactly the keystroke it saves
                    // by saying so.
                    let count = if node.is_branch() && !node.expanded {
                        node.leaves().to_string()
                    } else {
                        String::new()
                    };
                    if !count.is_empty() {
                        let column = columns - 1 - count.chars().count();
                        text(at(column, screen_row), count.clone(), structure);
                    }

                    // The family's mark, in a cell reserved on EVERY row
                    // whether or not that row has one. A column that
                    // appeared and vanished would move the labels beside
                    // it, and geometry that shifts with content is
                    // geometry the eye has to re-learn each frame.
                    //
                    // Drawn at the structure rung: an esoteric mark
                    // whispers. One that shouted would be claiming an
                    // importance the heading does not have.
                    if let Some(mark) = node.mark {
                        let box_ = egui::Rect::from_min_size(
                            at(1 + indent + 1, screen_row),
                            egui::vec2(cell.x, cell.y),
                        );
                        glyph::paint(painter, box_.shrink(1.0), mark, structure);
                    }

                    // The label, character by character, so the ones the
                    // filter actually consumed can be told from the ones
                    // it merely passed over. With nothing typed every
                    // character is unmatched and the row is drawn flat, so
                    // this costs nothing until it says something.
                    let start = 1 + indent + 3;
                    let room =
                        (columns - 1)
                            .saturating_sub(start)
                            .saturating_sub(if count.is_empty() {
                                0
                            } else {
                                count.chars().count() + 1
                            });
                    let marks = browser::match_positions(&node.label, browser.query());
                    for (offset, letter) in node.label.chars().take(room).enumerate() {
                        let lit = marks.get(offset).copied().unwrap_or(false);
                        let ink = if lit && !addressed {
                            design::FOCUS.color
                        } else if lit {
                            design::GROUND.color
                        } else if addressed {
                            design::SURFACE.color
                        } else {
                            content
                        };
                        text(at(start + offset, screen_row), letter.to_string(), ink);
                    }
                }
                // A note is never addressable, so it never inverts, and it
                // speaks a rung quieter than the rows it stands among.
                Line::Note(depth, words) => {
                    let indent = "  ".repeat(*depth);
                    text(
                        at(1, screen_row),
                        fit_cells(&format!("{indent}  {words}"), inner),
                        design::EDGE.color,
                    );
                }
            }
        }

        // With no addressable row, the typing prompt becomes the one focus
        // signal. When a row exists its inversion is the signal instead.
        if browser.cursor().is_none() {
            painter.rect_filled(
                egui::Rect::from_min_size(at(1, 1), egui::vec2(cell.x, cell.y)),
                0.0,
                FOCUSED,
            );
            text(
                at(1, 1),
                browser::glyph::PROMPT.to_string(),
                design::GROUND.color,
            );
        }
    }

    /// The codebook for the scope focus is standing in, drawn from the
    /// keymap table itself.
    ///
    /// Two columns, both left-aligned on their own axis so the eye reads
    /// down either one: keys at [`design::FOCUS`] because they are the
    /// actionable half, meanings at [`design::INK`]. Monospace does the
    /// alignment for free, which is most of why the whole stage is
    /// monospace.
    fn draw_help(&self, painter: &egui::Painter, field: egui::Rect) {
        let rows: Vec<_> = keymap::bindings_for(self.scope_context()).collect();
        if rows.is_empty() {
            return;
        }

        let line = design::px(design::type_scale::BODY);
        let pitch = line + design::px(design::space::STEP);
        let key_w = design::px(design::space::VAST) * 2.0;
        let block_h = pitch * rows.len() as f32;

        let origin = egui::pos2(
            (field.center().x - key_w).floor(),
            (field.center().y - block_h / 2.0).floor(),
        );

        for (index, (modifiers, key, intent)) in rows.iter().enumerate() {
            let y = origin.y + index as f32 * pitch + pitch / 2.0;
            painter.text(
                egui::pos2(origin.x + key_w, y),
                egui::Align2::RIGHT_CENTER,
                keymap::chord_name(*modifiers, *key),
                egui::FontId::monospace(line),
                design::FOCUS.color,
            );
            painter.text(
                egui::pos2(origin.x + key_w + design::px(design::space::OPEN), y),
                egui::Align2::LEFT_CENTER,
                intent.label(),
                egui::FontId::monospace(line),
                design::INK.color,
            );
        }
    }

    fn draw_message(&self, painter: &egui::Painter, zone: egui::Rect) {
        const MARGIN: f32 = 16.0;

        let Some(refusal) = self.refusal else {
            // With no refusal this frame, the strip carries the
            // sequencer's last notice and the pitch-entry mode: a mode
            // must announce itself, and a refused edit must be read.
            let mut words = Vec::new();
            if self.midi_typing.enabled() {
                words.push("MIDI · letters are pitches");
            }
            if let Some(notice) = self.notice {
                words.push(notice);
            }
            if !words.is_empty() {
                painter.text(
                    egui::pos2(zone.min.x + MARGIN, zone.center().y),
                    egui::Align2::LEFT_CENTER,
                    words.join("   "),
                    egui::FontId::monospace(16.0),
                    design::INK.color,
                );
            }
            return;
        };
        let words = match refusal.reason {
            RefusalReason::Edge(Step::Up) => "Refused · edge up",
            RefusalReason::Edge(Step::Down) => "Refused · edge down",
            RefusalReason::Edge(Step::Left) => "Refused · edge left",
            RefusalReason::Edge(Step::Right) => "Refused · edge right",
            RefusalReason::Deeper => "Refused · depth limit",
            RefusalReason::Shallower => "Refused · no further out",
            RefusalReason::Empty => "Refused · nothing here yet",
            RefusalReason::AtTop => "Refused · already at top",
            RefusalReason::Unavailable => "Refused · no action yet",
        };
        painter.text(
            egui::pos2(zone.min.x + MARGIN, zone.center().y),
            egui::Align2::LEFT_CENTER,
            words,
            egui::FontId::monospace(16.0),
            REFUSAL,
        );
    }
}

/// Fit one terminal row by CHARACTER count, never byte count. The stage's
/// vocabulary is monospace, so padding here is geometry rather than styling.
fn fit_cells(words: &str, width: usize) -> String {
    let mut fitted: String = words.chars().take(width).collect();
    fitted.extend(std::iter::repeat_n(
        ' ',
        width.saturating_sub(fitted.chars().count()),
    ));
    fitted
}

/// One terminal cell per score beat, with exactly the current one filled.
/// The row's length is the meter display; no spelled-out signature shadows
/// the song's authority.
fn beat_cells(place: Place) -> String {
    (1..=place.beats_per_bar)
        .map(|beat| {
            if beat == place.beat {
                browser::glyph::BLOCK
            } else {
                browser::glyph::SHADE_LIGHT
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Key, Modifiers};

    /// Feed physical keys through the exact application route without
    /// constructing egui: dispatch in the active scope, then apply.
    fn drive(stage: &mut Stage, keys: &[Key]) -> Vec<ApplyOutcome> {
        keys.iter()
            .map(|&key| {
                stage
                    .handle_key(Modifiers::NONE, key)
                    .unwrap_or_else(|| panic!("unbound key in stage sequence: {key:?}"))
            })
            .collect()
    }

    /// The same, for chords that hold a modifier.
    fn command(stage: &mut Stage, key: Key) -> ApplyOutcome {
        stage
            .handle_key(Modifiers::COMMAND, key)
            .unwrap_or_else(|| panic!("unbound chord in stage sequence: ^{key:?}"))
    }

    fn command_shift(stage: &mut Stage, key: Key) -> ApplyOutcome {
        stage
            .handle_key(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
            .unwrap_or_else(|| panic!("unbound chord in stage sequence: ^+{key:?}"))
    }

    fn type_text(stage: &mut Stage, text: &str) -> Vec<ApplyOutcome> {
        text.chars()
            .map(|ch| {
                stage
                    .handle_input(keymap::StageInput::Text(ch))
                    .unwrap_or_else(|| panic!("unbound text in stage sequence: {ch:?}"))
            })
            .collect()
    }

    /// The calibration field now sits one level INSIDE the track strip.
    /// Tests about focus mechanics rather than about the strip descend
    /// through it first, and say so here instead of in every body.
    fn into_field(stage: &mut Stage) {
        assert_eq!(
            drive(stage, &[Key::Enter]),
            vec![ApplyOutcome::Changed],
            "the strip refused to open a track"
        );
    }

    /// The root as the session lattice, which is the only shape the root
    /// may be.
    fn session(stage: &Stage) -> &FocusLattice {
        let Some(FocusScope::Lattice(lattice)) = stage.focus.levels().first() else {
            panic!("the root stopped being the session lattice")
        };
        lattice
    }

    fn active_grid(stage: &Stage) -> &FocusGrid {
        let FocusScope::Grid(grid) = stage.focus.active() else {
            panic!("the calibration stage installed a non-grid scope")
        };
        grid
    }

    fn grid_at(stage: &Stage, depth: usize) -> &FocusGrid {
        let Some(scope) = stage.focus.levels().get(depth) else {
            panic!("missing calibration scope at depth {depth}")
        };
        let FocusScope::Grid(grid) = scope else {
            panic!("the calibration stage installed a non-grid scope")
        };
        grid
    }

    #[test]
    fn a_new_audio_track_lands_at_the_end_and_takes_the_cursor() {
        let mut stage = Stage::new();
        let before = stage.song.tracks.len();

        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);

        assert_eq!(stage.song.tracks.len(), before + 1);
        let made = stage.song.tracks.last().expect("the track was not added");
        assert_eq!(made.kind, TrackKind::Audio);
        assert_eq!(made.name, "Audio 01");
        let session = session(&stage);
        assert_eq!(
            session.cols(),
            before + 1,
            "the strip did not follow the song"
        );
        assert_eq!(
            session.cursor(),
            Some((before, 0)),
            "focus did not land on the head of the track just made"
        );
    }

    #[test]
    fn making_a_track_from_a_scene_stays_on_that_scene() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowDown, Key::ArrowDown]);
        assert_eq!(session(&stage).cursor(), Some((0, 2)));

        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);

        assert_eq!(
            session(&stage).cursor(),
            Some((1, 2)),
            "the cursor left its scene to follow the new track"
        );
        assert_eq!(
            stage.session_address(),
            Some(Address::Slot { track: 1, scene: 1 })
        );
    }

    #[test]
    fn the_shifted_chord_makes_a_midi_track_instead() {
        let mut stage = Stage::new();
        assert_eq!(command_shift(&mut stage, Key::T), ApplyOutcome::Changed);

        let made = stage.song.tracks.last().expect("the track was not added");
        assert_eq!(made.kind, TrackKind::Instrument);
        // One instrument track already exists in a default song, and the
        // numbering counts within a kind.
        assert_eq!(made.name, "Instrument 02");
    }

    #[test]
    fn the_two_chords_differ_only_by_the_modifier_that_names_the_kind() {
        let mut stage = Stage::new();
        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);
        assert_eq!(command_shift(&mut stage, Key::T), ApplyOutcome::Changed);
        let kinds: Vec<_> = stage
            .song
            .tracks
            .iter()
            .map(|track| track.kind.clone())
            .collect();
        assert_eq!(
            kinds,
            vec![
                TrackKind::Instrument,
                TrackKind::Audio,
                TrackKind::Instrument
            ]
        );
    }

    #[test]
    fn making_a_track_from_inside_one_does_not_move_the_context_underneath() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        drive(&mut stage, &[Key::ArrowRight, Key::ArrowDown]);
        let inside = active_grid(&stage).cursor();

        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);

        assert_eq!(
            active_grid(&stage).cursor(),
            inside,
            "making a track disturbed the field the cursor was in"
        );
        assert_eq!(session(&stage).cols(), stage.song.tracks.len());
        assert_eq!(
            session(&stage).cursor(),
            Some((0, 0)),
            "an ancestor cursor was relocated while focus was elsewhere"
        );
    }

    #[test]
    fn the_strip_can_be_walked_once_a_second_track_exists() {
        let mut stage = Stage::new();
        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);
        drive(&mut stage, &[Key::ArrowLeft]);
        assert_eq!(
            session(&stage).cursor(),
            Some((0, 0)),
            "the strip would not walk back to the first track"
        );
    }

    #[test]
    fn the_field_shows_as_many_columns_as_its_own_geometry_allows() {
        let gap = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        // Exactly the width the strip's own drawing would need for `n`.
        let width_for = |n: f32| n * TRACK_W + (n - 1.0) * gap + margin * 2.0 + ADDRESS_W;
        let field = |w: f32| egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, 400.0));

        for n in 1..=5 {
            assert_eq!(
                Stage::strip_capacity(field(width_for(n as f32))),
                n,
                "a field sized for {n} columns did not offer {n}"
            );
            assert_eq!(
                Stage::strip_capacity(field(width_for(n as f32) - 1.0)),
                (n - 1).max(1),
                "a field one pixel short of {n} columns still offered {n}"
            );
        }
    }

    #[test]
    fn a_field_too_narrow_for_any_column_still_offers_one() {
        let cramped = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4.0, 400.0));
        assert_eq!(
            Stage::strip_capacity(cramped),
            1,
            "the cursor was left with nowhere to stand"
        );
    }

    #[test]
    fn the_stage_opens_on_the_song_rather_than_the_calibration_field() {
        let stage = Stage::new();
        let session = session(&stage);
        assert_eq!(
            session.cols(),
            stage.song.tracks.len(),
            "the strip and the song disagree about how many tracks there are"
        );
        assert_eq!(
            session.rows(),
            scenes::lattice_rows(&stage.song),
            "the lattice and the session disagree about how many rows there are"
        );
        assert_eq!(
            stage.session_address(),
            Some(Address::Head { track: 0 }),
            "the stage did not open on the first track's head"
        );
    }

    #[test]
    fn the_strip_draws_one_head_per_track_in_the_songs_order() {
        let stage = Stage::new();
        let heads = super::tracks::heads(&stage.song);
        assert_eq!(heads.len(), stage.song.tracks.len());
        for (head, track) in heads.iter().zip(&stage.song.tracks) {
            assert_eq!(head.name, track.name);
        }
    }

    #[test]
    fn the_heads_are_the_top_of_the_session_and_refuse_up() {
        let mut stage = Stage::new();
        assert_eq!(
            drive(&mut stage, &[Key::ArrowUp]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Step(Step::Up),
                reason: RefusalReason::Edge(Step::Up),
            })],
            "there was something above the heads"
        );
    }

    #[test]
    fn down_walks_the_scenes_and_stops_at_the_last() {
        let mut stage = Stage::new();
        let count = stage.song.session.scenes.len();
        for scene in 0..count {
            assert_eq!(
                drive(&mut stage, &[Key::ArrowDown]),
                vec![ApplyOutcome::Changed]
            );
            assert_eq!(
                stage.session_address(),
                Some(Address::Slot { track: 0, scene }),
                "down did not land on scene {scene}"
            );
        }
        assert_eq!(
            drive(&mut stage, &[Key::ArrowDown]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Step(Step::Down),
                reason: RefusalReason::Edge(Step::Down),
            })],
            "the lattice went past its last scene"
        );
        for _ in 0..count {
            assert_eq!(
                drive(&mut stage, &[Key::ArrowUp]),
                vec![ApplyOutcome::Changed]
            );
        }
        assert_eq!(stage.session_address(), Some(Address::Head { track: 0 }));
    }

    #[test]
    fn scenes_are_walked_across_tracks_in_the_same_row() {
        let mut stage = Stage::new();
        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);
        drive(
            &mut stage,
            &[Key::ArrowLeft, Key::ArrowDown, Key::ArrowRight],
        );
        assert_eq!(
            stage.session_address(),
            Some(Address::Slot { track: 1, scene: 0 }),
            "stepping sideways changed the scene"
        );
    }

    #[test]
    fn enter_on_an_empty_slot_fills_it_and_stays_on_it() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowDown]);
        assert_eq!(
            drive(&mut stage, &[Key::Enter]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.focus.depth(), 1, "filling a slot descended somewhere");
        assert_eq!(
            stage.session_address(),
            Some(Address::Slot { track: 0, scene: 0 })
        );
        let mark = scenes::mark(&stage.song, 0, 0).expect("the slot drew nothing");
        assert_eq!(mark.glyph, browser::glyph::DOT);
        assert_eq!(
            mark.number, "02",
            "the pattern after the default one is the second"
        );
    }

    /// Fill the first slot of the first track and open it.
    fn into_clip(stage: &mut Stage) -> PatternId {
        drive(stage, &[Key::ArrowDown, Key::Enter]);
        let Some(Clip::Pattern(id)) = stage.song.slot_clip(0, 0) else {
            panic!("the slot did not fill")
        };
        assert_eq!(drive(stage, &[Key::Enter]), vec![ApplyOutcome::Changed]);
        id
    }

    #[test]
    fn enter_on_a_filled_slot_opens_its_pattern_as_a_sixteen_by_four_grid() {
        let mut stage = Stage::new();
        let patterns_before = stage.song.patterns.len() + 1;
        let id = into_clip(&mut stage);
        assert_eq!(stage.focus.depth(), 2, "the clip did not open");
        assert_eq!(
            stage.inside,
            Some(Opened {
                pattern: id,
                track: 0
            })
        );
        assert_eq!(stage.scope_context(), keymap::ScopeContext::Clip);
        assert_eq!(
            stage.song.patterns.len(),
            patterns_before,
            "opening a clip minted a pattern"
        );
        let grid = active_grid(&stage);
        assert_eq!((grid.cols(), grid.rows()), (16, 4));
        assert_eq!(grid.cursor(), (0, 0));
    }

    #[test]
    fn escape_leaves_the_clip_and_lands_back_on_its_slot() {
        let mut stage = Stage::new();
        into_clip(&mut stage);
        assert_eq!(
            drive(&mut stage, &[Key::Escape]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.focus.depth(), 1);
        assert_eq!(
            stage.inside, None,
            "the pattern was still held after leaving"
        );
        assert_eq!(
            stage.session_address(),
            Some(Address::Slot { track: 0, scene: 0 })
        );
    }

    #[test]
    fn inside_a_clip_the_stage_leaves_the_grammars_keys_alone() {
        let mut stage = Stage::new();
        into_clip(&mut stage);
        for key in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Enter,
            Key::Delete,
            Key::Backspace,
        ] {
            assert_eq!(
                stage.handle_key(Modifiers::NONE, key),
                None,
                "{key:?} was taken from the sequencer"
            );
        }
        // What stays global stays bound.
        assert_eq!(
            stage.handle_key(Modifiers::NONE, Key::Space),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(
            stage.handle_key(Modifiers::NONE, Key::Space),
            Some(ApplyOutcome::Changed)
        );
        assert!(stage.handle_key(Modifiers::COMMAND, Key::F).is_some());
    }

    #[test]
    fn sequencer_edits_land_on_the_open_pattern_and_refusals_are_kept() {
        let mut stage = Stage::new();
        let id = into_clip(&mut stage);
        let tick = 18 * PATTERN_STEP_TICKS;
        stage.apply_sequence(
            id,
            &[sequence::Intent::Toggle {
                tick,
                default_pitch: Pitch::from_midi(60),
                default_length_ticks: PATTERN_STEP_TICKS,
                default_velocity: 100,
            }],
        );
        let pattern = stage.song.pattern(id).expect("pattern");
        assert!(pattern.trig(18).enabled, "the toggle did not land");
        assert_eq!(pattern.trig(18).primary().map(|n| n.velocity), Some(100));
        assert_eq!(stage.notice, None);

        // A nudge off an empty step is refused in the sequencer's words.
        stage.apply_sequence(
            id,
            &[sequence::Intent::Nudge {
                tick: 40 * PATTERN_STEP_TICKS,
                delta_ticks: 12,
            }],
        );
        assert_eq!(stage.notice, Some("nudge: no trig here"));
        // A frame with nothing to apply leaves the notice standing …
        stage.apply_sequence(id, &[]);
        assert_eq!(stage.notice, Some("nudge: no trig here"));
        // … and leaving the clip clears it.
        assert_eq!(
            drive(&mut stage, &[Key::Escape]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.notice, None);
    }

    #[test]
    fn a_degree_track_types_degrees_and_an_absolute_track_types_notes() {
        let mut stage = Stage::new();
        let id = into_clip(&mut stage);
        let opened = Opened {
            pattern: id,
            track: 0,
        };
        stage.song.tracks[0].pitch_authority = PitchAuthority::Degree;
        assert!(matches!(
            stage.entry_mode(opened),
            midi_typing::EntryMode::Degree { .. }
        ));
        stage.song.tracks[0].pitch_authority = PitchAuthority::Absolute;
        assert_eq!(stage.entry_mode(opened), midi_typing::EntryMode::Chromatic);
    }

    #[test]
    fn enter_on_a_clip_from_the_session_does_not_reach_the_sequencer_by_the_stage() {
        // Enter is the sequencer's ACT inside a clip. If it ever came to
        // the stage there, the honest answer is a refusal, not a second
        // toggle path.
        let mut stage = Stage::new();
        into_clip(&mut stage);
        assert_eq!(
            stage.apply(StageIntent::Enter),
            ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Enter,
                reason: RefusalReason::Unavailable,
            })
        );
    }

    #[test]
    fn the_clip_tray_is_a_fixed_band_at_the_foot_of_the_field() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);
        assert_eq!(layout.clip.height(), CLIP_H);
        assert_eq!(
            layout.clip.max.y, layout.field.max.y,
            "the tray left the foot"
        );
        assert_eq!(
            layout.session.max.y, layout.clip.min.y,
            "session and tray overlap or gap"
        );
        assert_eq!(layout.session.min, layout.field.min);
        assert_eq!(
            layout.session.height() + layout.clip.height(),
            layout.field.height(),
            "the two do not partition the field"
        );
    }

    #[test]
    fn the_tray_shows_the_clip_under_the_cursor_before_it_is_entered() {
        let mut stage = Stage::new();
        assert_eq!(stage.clip_in_view(), None, "a head showed a clip");
        drive(&mut stage, &[Key::ArrowDown]);
        assert_eq!(stage.clip_in_view(), None, "an empty slot showed a clip");
        drive(&mut stage, &[Key::Enter]);
        let Some(Clip::Pattern(id)) = stage.song.slot_clip(0, 0) else {
            panic!("the slot did not fill")
        };
        assert_eq!(
            stage.clip_in_view(),
            Some(Opened {
                pattern: id,
                track: 0
            }),
            "a filled slot under the cursor was not shown"
        );
        assert_eq!(stage.inside, None, "showing is not entering");
        assert_eq!(stage.scope_context(), keymap::ScopeContext::Root);
        // Entering shows the same clip, now with the keys.
        drive(&mut stage, &[Key::Enter]);
        assert_eq!(stage.clip_in_view().map(|o| o.pattern), Some(id));
        assert_eq!(stage.scope_context(), keymap::ScopeContext::Clip);
        // Leaving keeps it in view, because the cursor is still on it.
        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.clip_in_view().map(|o| o.pattern), Some(id));
        drive(&mut stage, &[Key::ArrowUp]);
        assert_eq!(stage.clip_in_view(), None);
    }

    #[test]
    fn the_session_stays_drawn_while_a_clip_is_open_with_a_resting_cursor() {
        let mut stage = Stage::new();
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(FOCUSED)
        );
        into_clip(&mut stage);
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(RESTING),
            "the session vanished, or kept a focus-bright cursor, under an open clip"
        );
        drive(&mut stage, &[Key::Escape]);
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(FOCUSED)
        );
        // Inside a TRACK the field is the calibration grid, not the session.
        drive(&mut stage, &[Key::ArrowUp]);
        into_field(&mut stage);
        assert_eq!(stage.session_lattice(), None);
    }

    #[test]
    fn a_head_still_opens_the_calibration_field_not_a_pattern() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        assert_eq!(stage.inside, None);
        assert_eq!(active_grid(&stage).cols(), GRID_COLS);
    }

    #[test]
    fn an_audio_slot_refuses_a_pattern() {
        let mut stage = Stage::new();
        assert_eq!(command(&mut stage, Key::T), ApplyOutcome::Changed);
        drive(&mut stage, &[Key::ArrowDown]);
        assert_eq!(
            stage.session_address(),
            Some(Address::Slot { track: 1, scene: 0 })
        );
        assert_eq!(
            drive(&mut stage, &[Key::Enter]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Enter,
                reason: RefusalReason::Unavailable,
            })]
        );
        assert_eq!(stage.song.slot_clip(1, 0), None);
    }

    #[test]
    fn delete_clears_a_slot_and_an_empty_one_says_so() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowDown, Key::Enter]);
        assert!(stage.song.slot_clip(0, 0).is_some());
        assert_eq!(
            drive(&mut stage, &[Key::Delete]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.song.slot_clip(0, 0), None);
        assert_eq!(
            drive(&mut stage, &[Key::Backspace]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Clear,
                reason: RefusalReason::Empty,
            })],
            "clearing an empty slot claimed to change something"
        );
    }

    #[test]
    fn clear_means_nothing_off_a_slot() {
        let mut stage = Stage::new();
        let refused = |intent| {
            ApplyOutcome::Refused(Refusal {
                intent,
                reason: RefusalReason::Unavailable,
            })
        };
        assert_eq!(
            drive(&mut stage, &[Key::Delete]),
            vec![refused(StageIntent::Clear)],
            "a head was cleared"
        );
        // Fill a slot, climb back to the head, and go into the track.
        drive(&mut stage, &[Key::ArrowDown, Key::Enter, Key::ArrowUp]);
        into_field(&mut stage);
        assert_eq!(
            drive(&mut stage, &[Key::Delete]),
            vec![refused(StageIntent::Clear)],
            "a slot was cleared from inside a track"
        );
        assert!(
            stage.song.slot_clip(0, 0).is_some(),
            "the slot was cleared anyway"
        );
    }

    #[test]
    fn a_head_still_opens_the_track_from_the_session() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowDown, Key::ArrowUp]);
        into_field(&mut stage);
        assert_eq!(stage.focus.depth(), 2);
    }

    #[test]
    fn a_key_sequence_descends_moves_and_returns_to_its_parent_cursor() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        let outcomes = drive(
            &mut stage,
            &[
                Key::ArrowDown,
                Key::ArrowDown,
                Key::Enter,
                Key::ArrowDown,
                Key::Escape,
            ],
        );

        assert!(
            outcomes
                .iter()
                .all(|outcome| *outcome == ApplyOutcome::Changed)
        );
        assert_eq!(stage.focus.depth(), 2);
        assert_eq!(active_grid(&stage).cursor(), (0, 2));
    }

    #[test]
    fn key_sequences_clamp_at_all_four_edges_and_name_each_refusal() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        assert_eq!(
            drive(&mut stage, &[Key::ArrowUp, Key::ArrowLeft]),
            vec![
                ApplyOutcome::Refused(Refusal {
                    intent: StageIntent::Step(Step::Up),
                    reason: RefusalReason::Edge(Step::Up),
                }),
                ApplyOutcome::Refused(Refusal {
                    intent: StageIntent::Step(Step::Left),
                    reason: RefusalReason::Edge(Step::Left),
                }),
            ]
        );

        let right = [Key::ArrowRight; GRID_COLS];
        let right_outcomes = drive(&mut stage, &right);
        assert_eq!(active_grid(&stage).cursor(), (GRID_COLS - 1, 0));
        assert!(matches!(
            right_outcomes.last(),
            Some(ApplyOutcome::Refused(Refusal {
                reason: RefusalReason::Edge(Step::Right),
                ..
            }))
        ));

        let down = [Key::ArrowDown; GRID_ROWS];
        let down_outcomes = drive(&mut stage, &down);
        assert_eq!(active_grid(&stage).cursor(), (GRID_COLS - 1, GRID_ROWS - 1));
        assert!(matches!(
            down_outcomes.last(),
            Some(ApplyOutcome::Refused(Refusal {
                reason: RefusalReason::Edge(Step::Down),
                ..
            }))
        ));

        assert!(matches!(
            drive(&mut stage, &[Key::ArrowRight]).as_slice(),
            [ApplyOutcome::Refused(Refusal {
                reason: RefusalReason::Edge(Step::Right),
                ..
            })]
        ));
    }

    #[test]
    fn root_escape_is_a_reasoned_refusal_sequence() {
        let mut stage = Stage::new();
        assert_eq!(
            drive(&mut stage, &[Key::Escape]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Escape,
                reason: RefusalReason::Shallower,
            })]
        );
        assert_eq!(stage.focus.depth(), 1);
        assert_eq!(
            stage.refusal,
            Some(Refusal {
                intent: StageIntent::Escape,
                reason: RefusalReason::Shallower,
            })
        );
    }

    #[test]
    fn enter_clamps_at_the_depth_cap_and_names_the_refusal() {
        let mut stage = Stage::new();
        let outcomes = drive(&mut stage, &[Key::Enter; MAX_DEPTH]);

        assert_eq!(stage.focus.depth(), MAX_DEPTH);
        assert!(
            outcomes[..MAX_DEPTH - 1]
                .iter()
                .all(|outcome| *outcome == ApplyOutcome::Changed)
        );
        assert_eq!(
            outcomes.last(),
            Some(&ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Enter,
                reason: RefusalReason::Deeper,
            }))
        );
    }

    #[test]
    fn ancestor_cursors_survive_nested_key_sequences() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        drive(&mut stage, &[Key::ArrowRight, Key::ArrowDown, Key::Enter]);
        assert_eq!(active_grid(&stage).cursor(), (0, 0));

        drive(&mut stage, &[Key::ArrowDown, Key::ArrowDown, Key::Enter]);
        drive(&mut stage, &[Key::ArrowRight, Key::Escape]);
        assert_eq!(stage.focus.depth(), 3);
        assert_eq!(active_grid(&stage).cursor(), (0, 2));

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.focus.depth(), 2);
        assert_eq!(active_grid(&stage).cursor(), (1, 1));
    }

    #[test]
    fn step_keys_only_move_the_active_scope() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        drive(&mut stage, &[Key::Enter, Key::ArrowRight]);

        assert_eq!(grid_at(&stage, 1).cursor(), (0, 0));
        assert_eq!(active_grid(&stage).cursor(), (1, 0));
    }

    #[test]
    fn latest_refusal_survives_a_later_success_in_the_same_sequence() {
        let mut stage = Stage::new();
        into_field(&mut stage);
        drive(&mut stage, &[Key::ArrowUp, Key::ArrowDown]);

        assert_eq!(active_grid(&stage).cursor(), (0, 1));
        assert_eq!(
            stage.refusal,
            Some(Refusal {
                intent: StageIntent::Step(Step::Up),
                reason: RefusalReason::Edge(Step::Up),
            })
        );
    }

    /// This is the refusal-channel contract: a bound keystroke cannot be
    /// absorbed between dispatch and state. It must change focus or leave a
    /// reason in the one refusal slot.
    #[test]
    fn every_dispatched_key_changes_state_or_produces_a_refusal() {
        for scope in keymap::ScopeContext::ALL {
            for (modifiers, key) in keymap::bound_chords() {
                let Some(_intent) =
                    keymap::dispatch(scope, keymap::StageInput::Chord(modifiers, key))
                else {
                    continue;
                };
                let mut stage = Stage::new();
                match scope {
                    keymap::ScopeContext::Nested => {
                        assert_eq!(
                            stage.handle_key(Modifiers::NONE, Key::Enter),
                            Some(ApplyOutcome::Changed)
                        );
                    }
                    keymap::ScopeContext::Browser => {
                        assert_eq!(
                            stage.handle_key(Modifiers::COMMAND, Key::F),
                            Some(ApplyOutcome::Changed)
                        );
                    }
                    keymap::ScopeContext::Clip => {
                        into_clip(&mut stage);
                    }
                    keymap::ScopeContext::Root => {}
                }
                let before = (
                    stage.focus.clone(),
                    stage.transport,
                    stage.help,
                    stage.browser.clone(),
                );

                match stage.handle_key(modifiers, key) {
                    Some(ApplyOutcome::Changed) => {
                        assert_ne!(
                            (
                                stage.focus.clone(),
                                stage.transport,
                                stage.help,
                                stage.browser.clone(),
                            ),
                            before,
                            "{scope:?} + {key:?} lied about changing"
                        );
                        assert_eq!(stage.refusal, None);
                    }
                    Some(ApplyOutcome::Refused(refusal)) => {
                        assert_eq!(
                            (
                                stage.focus.clone(),
                                stage.transport,
                                stage.help,
                                stage.browser.clone(),
                            ),
                            before,
                            "{scope:?} + {key:?} changed and refused"
                        );
                        assert_eq!(stage.refusal, Some(refusal));
                    }
                    None => panic!("{scope:?} + {key:?} disappeared after dispatch"),
                }
            }
        }

        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);
        let before = stage.browser.clone();
        assert_eq!(type_text(&mut stage, "k"), vec![ApplyOutcome::Changed]);
        assert_ne!(stage.browser, before, "browser text was swallowed");
    }

    #[test]
    fn transport_keys_apply_from_every_scope_without_moving_focus() {
        for scope in keymap::ScopeContext::ALL {
            let mut stage = Stage::new();
            match scope {
                keymap::ScopeContext::Root => {}
                keymap::ScopeContext::Nested => {
                    assert_eq!(
                        stage.handle_key(Modifiers::NONE, Key::Enter),
                        Some(ApplyOutcome::Changed)
                    );
                }
                keymap::ScopeContext::Browser => {
                    assert_eq!(
                        stage.handle_key(Modifiers::COMMAND, Key::F),
                        Some(ApplyOutcome::Changed)
                    );
                }
                keymap::ScopeContext::Clip => {
                    into_clip(&mut stage);
                }
            }
            let focus = stage.focus.clone();
            let browser = stage.browser.clone();

            assert_eq!(
                stage.handle_key(Modifiers::NONE, Key::Space),
                Some(ApplyOutcome::Changed)
            );
            assert_eq!(stage.transport.motion(), Motion::Rolling);
            assert_eq!(
                (stage.focus.clone(), stage.browser.clone()),
                (focus, browser)
            );

            stage.transport.seek(crate::sequencing::TICKS_PER_BEAT);
            assert_eq!(
                stage.handle_key(Modifiers::NONE, Key::Home),
                Some(ApplyOutcome::Changed)
            );
            assert_eq!(stage.transport.tick(), 0);
            assert_eq!(stage.scope_context(), scope);
        }
    }

    #[test]
    fn return_at_the_top_is_a_reasoned_refusal() {
        let mut stage = Stage::new();
        assert_eq!(
            drive(&mut stage, &[Key::Home]),
            vec![ApplyOutcome::Refused(Refusal {
                intent: StageIntent::Rewind,
                reason: RefusalReason::AtTop,
            })]
        );
    }

    #[test]
    fn space_stops_an_active_recording() {
        let mut stage = Stage::new();
        stage.transport.set_motion(Motion::Recording);
        assert_eq!(
            drive(&mut stage, &[Key::Space]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.transport.motion(), Motion::Stopped);
    }

    #[test]
    fn beat_row_shape_is_the_meter_and_only_the_current_beat_is_full() {
        let row = beat_cells(Place {
            bar: 12,
            beat: 3,
            beats_per_bar: 7,
            denominator: 8,
        });
        assert_eq!(row.chars().count(), 7);
        assert_eq!(
            row,
            format!(
                "{}{}{}{}{}{}{}",
                browser::glyph::SHADE_LIGHT,
                browser::glyph::SHADE_LIGHT,
                browser::glyph::BLOCK,
                browser::glyph::SHADE_LIGHT,
                browser::glyph::SHADE_LIGHT,
                browser::glyph::SHADE_LIGHT,
                browser::glyph::SHADE_LIGHT,
            )
        );
    }

    #[test]
    fn the_codebook_is_summoned_and_dismissed_by_the_same_key() {
        let mut stage = Stage::new();
        assert!(!stage.help, "the stage does not start explaining itself");

        drive(&mut stage, &[Key::Questionmark]);
        assert!(stage.help);
        drive(&mut stage, &[Key::Questionmark]);
        assert!(!stage.help);
    }

    /// Escape means up and out, and an open codebook is the outermost
    /// thing there is — so it leaves that before it leaves a scope.
    #[test]
    fn escape_closes_the_codebook_before_it_ascends() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::Enter, Key::Questionmark]);
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert!(!stage.help, "escape left the codebook first");
        assert_eq!(stage.focus.depth(), 2, "and left the scope alone");

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.focus.depth(), 1, "the next escape ascends");
    }

    /// The codebook describes where focus stands, so it must follow focus
    /// rather than describe the stage in general.
    #[test]
    fn the_codebook_reports_the_scope_focus_is_standing_in() {
        let mut stage = Stage::new();
        let root: Vec<_> = keymap::bindings_for(stage.scope_context()).collect();

        drive(&mut stage, &[Key::Enter]);
        let nested: Vec<_> = keymap::bindings_for(stage.scope_context()).collect();

        assert!(!root.is_empty() && !nested.is_empty());
        for (modifiers, key, intent) in &nested {
            assert_eq!(
                keymap::dispatch(
                    stage.scope_context(),
                    keymap::StageInput::Chord(*modifiers, *key)
                ),
                Some(*intent),
                "the codebook named a key this scope does not answer to"
            );
        }
    }

    /// Focus is untouched by opening the codebook: it is a display mode,
    /// not a place you can go.
    #[test]
    fn the_codebook_does_not_move_focus() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowRight, Key::ArrowDown]);
        let where_we_were = stage.focus.clone();

        drive(&mut stage, &[Key::Questionmark, Key::Questionmark]);
        assert_eq!(stage.focus, where_we_were);
    }

    /// The guarantee, held as code: NOTHING the stage can be doing moves
    /// a zone. Layout is a function of the window alone, so summoning the
    /// browser, opening the codebook, descending, or being refused all
    /// leave every other zone exactly where the eye last found it.
    #[test]
    fn no_state_the_stage_can_reach_moves_a_zone() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let reference = Layout::of(window);

        let mut stage = Stage::new();
        for chord in [
            (Modifiers::COMMAND, Key::F),
            (Modifiers::NONE, Key::Questionmark),
            (Modifiers::NONE, Key::Enter),
            (Modifiers::NONE, Key::ArrowUp),
            (Modifiers::NONE, Key::Escape),
            (Modifiers::NONE, Key::Escape),
            (Modifiers::COMMAND, Key::F),
        ] {
            let _ = stage.handle_key(chord.0, chord.1);
            assert_eq!(
                Layout::of(window),
                reference,
                "a zone moved because of what the stage was doing"
            );
        }
    }

    /// The browser opens OVER the field, so the field is laid out as
    /// though it did not exist and gets its whole self back the moment
    /// the browser closes.
    #[test]
    fn the_browser_overlays_the_field_rather_than_dividing_it() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);

        assert_eq!(layout.browser.width(), BROWSER_W);
        assert_eq!(
            layout.field.min.x, layout.browser.min.x,
            "the field yielded ground to the browser"
        );
        assert_eq!(layout.field.width(), window.width());
        assert!(layout.field.contains_rect(layout.browser));
    }

    #[test]
    fn the_browser_is_summoned_and_dismissed_by_the_same_chord() {
        let mut stage = Stage::new();
        assert!(
            stage.browser.is_none(),
            "the browser is not standing chrome"
        );

        let _ = command(&mut stage, Key::F);
        assert!(stage.browser.is_some());
        assert_eq!(stage.scope_context(), keymap::ScopeContext::Browser);

        let _ = command(&mut stage, Key::F);
        assert!(stage.browser.is_none());
        assert_eq!(stage.scope_context(), keymap::ScopeContext::Root);
    }

    /// Going to the library is not going deeper. The browser holds focus
    /// without touching the ancestry, and hands it back exactly where it
    /// was picked up.
    #[test]
    fn the_browser_borrows_focus_without_disturbing_the_field() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::ArrowRight, Key::ArrowDown, Key::Enter]);
        let field = stage.focus.clone();
        let depth = stage.focus.depth();

        let _ = command(&mut stage, Key::F);
        assert_eq!(stage.focus, field, "summoning moved the field cursor");
        assert_eq!(stage.focus.depth(), depth, "the browser faked an ancestry");

        drive(&mut stage, &[Key::ArrowDown, Key::ArrowUp]);
        assert_eq!(stage.focus, field, "keys leaked back into the field");

        let _ = command(&mut stage, Key::F);
        assert_eq!(stage.focus, field, "focus came back somewhere else");
    }

    /// Escape means up and out, in one order: the codebook, then the
    /// browser, then a scope. Never two at once.
    ///
    /// The tree is NOT a level of this order. Climbing it is Left's job,
    /// so Escape keeps one meaning instead of doubling as a quieter way
    /// to move.
    #[test]
    fn escape_leaves_the_outermost_thing_first() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::Enter]);
        let _ = command(&mut stage, Key::F);
        drive(&mut stage, &[Key::Enter]);
        drive(&mut stage, &[Key::Questionmark]);

        drive(&mut stage, &[Key::Escape]);
        assert!(!stage.help, "the codebook goes first");
        assert!(stage.browser.is_some());
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert!(stage.browser.is_none(), "then the browser goes");
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.focus.depth(), 1, "and only then does a scope");
    }

    /// The library opens on its three closed shelves, and the cursor
    /// walks them with the same clamping grammar the field uses.
    #[test]
    fn the_browser_opens_on_its_shelves_and_walks_them() {
        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);

        let browser = stage.browser.as_ref().expect("summoned");
        assert_eq!(browser.rows().len(), Shelf::ALL.len());
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Devices")
        );

        assert!(matches!(
            drive(&mut stage, &[Key::ArrowUp]).as_slice(),
            [ApplyOutcome::Refused(Refusal {
                reason: RefusalReason::Edge(Step::Up),
                ..
            })]
        ));
        drive(&mut stage, &[Key::ArrowDown]);
        assert_eq!(
            stage
                .browser
                .as_ref()
                .and_then(|b| b.selected())
                .map(|node| node.label.as_str()),
            Some("Samples")
        );
    }

    #[test]
    fn enter_opens_a_shelf_in_place_and_left_closes_it_again() {
        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);

        assert_eq!(
            drive(&mut stage, &[Key::Enter]),
            vec![ApplyOutcome::Changed]
        );
        let browser = stage.browser.as_ref().expect("browser remains open");
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Devices"),
            "opening a shelf moved the cursor off it"
        );
        assert!(
            browser.rows().len() > Shelf::ALL.len(),
            "the shelf opened without revealing anything"
        );

        assert_eq!(
            drive(&mut stage, &[Key::ArrowLeft]),
            vec![ApplyOutcome::Changed]
        );
        let browser = stage.browser.as_ref().expect("the browser is still open");
        assert_eq!(
            browser.rows().len(),
            Shelf::ALL.len(),
            "left did not close the shelf"
        );
    }

    #[test]
    fn printable_keys_and_backspace_filter_through_dispatch() {
        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);
        drive(&mut stage, &[Key::Enter]);

        assert_eq!(
            type_text(&mut stage, "sn"),
            vec![ApplyOutcome::Changed, ApplyOutcome::Changed]
        );
        assert_eq!(stage.browser.as_ref().map(Browser::query), Some("sn"));
        // Headings on the way to a match survive on purpose, so the
        // promise is about LEAVES: nothing is left standing that the
        // typing does not actually reach.
        let browser = stage.browser.as_ref().expect("open");
        let mut matched = 0;
        for row in browser.rows() {
            let node = browser.node_at(&row.path).expect("a row without a node");
            if node.is_branch() {
                continue;
            }
            assert!(
                browser::matches_query(&node.label, "sn"),
                "the filter kept the leaf {:?}",
                node.label
            );
            matched += 1;
        }
        assert!(matched > 0, "the filter left nothing to stand on");

        assert_eq!(
            drive(&mut stage, &[Key::Backspace]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.browser.as_ref().map(Browser::query), Some("s"));
    }

    /// The codebook follows focus into the browser: it describes where
    /// you are standing, not where the app began.
    #[test]
    fn the_codebook_changes_when_focus_enters_the_browser() {
        let mut stage = Stage::new();
        let field: Vec<_> = keymap::bindings_for(stage.scope_context()).collect();

        let _ = command(&mut stage, Key::F);
        let browser: Vec<_> = keymap::bindings_for(stage.scope_context()).collect();

        assert_ne!(field, browser, "the codebook did not follow focus");
        assert!(browser.iter().all(|(m, k, i)| keymap::dispatch(
            keymap::ScopeContext::Browser,
            keymap::StageInput::Chord(*m, *k)
        ) == Some(*i)));
    }

    /// The stage must not borrow from the frame it replaces. Checked as
    /// code rather than trusted as intent, for the same reason
    /// `crate::intent` is checked: the coupling that matters arrives one
    /// convenient import at a time.
    #[test]
    fn the_stage_does_not_reach_into_the_frame_it_replaces() {
        let src = include_str!("mod.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("redesign"),
            "the stage reached into `ui::redesign` — lift the shared part \
             out to a neutral home instead, as `crate::intent` was"
        );
    }
}
