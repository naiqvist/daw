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
mod transport;

use crate::design;
use crate::library::{LibraryConfig, LibraryService, LibrarySnapshot};
use crate::sequencing::Song;
use eframe::egui;

use browser::{BrowserStatus, device_entries, project_entries, sample_entries};

pub use browser::{Browser, Entry, EntryKind, Shelf};
pub use grid::{FocusColumn, FocusGrid, FocusScope, FocusStack, Step};
pub use keymap::StageIntent;
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
const SQUARE_EDGE: egui::Color32 = design::EDGE.color;
const FOCUSED: egui::Color32 = design::FOCUS.color;
const REFUSAL: egui::Color32 = design::INK.color;
/// Where focus WILL be when it comes back, drawn while it is somewhere
/// else. Dimmer than focus by a whole rung, so the rule holds that exactly
/// one thing on the screen is ever focus-bright.
const RESTING: egui::Color32 = design::INK.color;

/// The periphery: two fixed strips framing one sovereign field. Zones are
/// keyed by FUNCTION (ancestry, vitals, messages), never by content, and
/// they never take focus — the cursor lives only in the field.
///
/// The height is a LAYOUT dimension rather than a spacing rung: it was
/// settled by eye on the display this is read on, and the spacing ladder
/// is for the distances between things, not for how big a zone is.
const PERIPHERY_H: f32 = 64.0;
const HAIRLINE: egui::Color32 = design::EDGE.color;

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
    field: egui::Rect,
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
        Self {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field: band,
        }
    }
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
        Self {
            focus: FocusStack::new(FocusScope::grid(GRID_COLS, GRID_ROWS), MAX_DEPTH),
            song: Song::default(),
            transport: Transport::new(),
            browser: None,
            help: false,
            refusal: None,
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
        let inputs = ui.input_mut(|input| {
            let mut stage_inputs = Vec::new();
            let mut questionmark_consumed = false;
            let mut space_consumed = false;
            for (modifiers, key) in keymap::bound_chords() {
                if input.consume_key(modifiers, key) {
                    questionmark_consumed |= key == egui::Key::Questionmark;
                    space_consumed |= key == egui::Key::Space;
                    stage_inputs.push(keymap::StageInput::Chord(modifiers, key));
                }
            }
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
            if self
                .browser
                .as_ref()
                .is_some_and(|browser| browser.shelf() == Some(Shelf::Samples))
            {
                let entries = sample_entries(&self.library_snapshot.assets);
                if let Some(browser) = &mut self.browser {
                    browser.refresh_shelf(entries, BrowserStatus::Ready);
                }
            }
        }
        if self.library_scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn shelf_contents(&self, shelf: Shelf) -> (Vec<Entry>, BrowserStatus) {
        match shelf {
            Shelf::Devices => (device_entries(), BrowserStatus::Ready),
            Shelf::Samples => (
                sample_entries(&self.library_snapshot.assets),
                if self.library_scanning {
                    BrowserStatus::Scanning
                } else {
                    BrowserStatus::Ready
                },
            ),
            // No frame-independent project catalog exists yet. This is the
            // rendering end of `browser::project_entries`' explicit seam.
            Shelf::Projects => (project_entries(), BrowserStatus::Unavailable),
        }
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
            StageIntent::Enter => match self
                .browser
                .as_ref()
                .and_then(Browser::selected)
                .map(|entry| entry.kind.clone())
            {
                Some(EntryKind::Shelf(shelf)) => {
                    let (entries, status) = self.shelf_contents(shelf);
                    if let Some(browser) = &mut self.browser {
                        browser.enter_shelf(shelf, entries, status);
                    }
                    Ok(())
                }
                Some(_) => Err(RefusalReason::Unavailable),
                None if self.browser.is_some() => Err(RefusalReason::Empty),
                // Calibration policy, not stack policy: today's stage
                // always opens another identical empty grid. Real scopes
                // will supply their own child later without changing
                // `FocusStack`.
                None => self
                    .focus
                    .enter(FocusScope::grid(GRID_COLS, GRID_ROWS))
                    .then_some(())
                    .ok_or(RefusalReason::Deeper),
            },
            // Escape is one meaning everywhere: up and OUT. It leaves
            // whatever is outermost — the codebook first, then the
            // browser, then a scope.
            StageIntent::Escape => {
                if self.help {
                    self.help = false;
                    Ok(())
                } else if self.browser.is_some() {
                    if self.browser.as_mut().is_some_and(Browser::ascend) {
                        Ok(())
                    } else {
                        self.browser = None;
                        Ok(())
                    }
                } else {
                    self.focus
                        .escape()
                        .then_some(())
                        .ok_or(RefusalReason::Shallower)
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

    fn draw(&self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter();

        // The constitution: a thin fixed periphery around one sovereign
        // field. The strips hold display only; nothing in them is ever
        // focusable, and their geometry never changes.
        let Layout {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field: avail,
        } = Layout::of(whole);

        painter.line_segment(
            [
                egui::pos2(whole.min.x, vitals.max.y),
                egui::pos2(whole.max.x, vitals.max.y),
            ],
            egui::Stroke::new(1.0, HAIRLINE),
        );
        painter.line_segment(
            [
                egui::pos2(whole.min.x, message.min.y),
                egui::pos2(whole.max.x, message.min.y),
            ],
            egui::Stroke::new(1.0, HAIRLINE),
        );

        // One light rule divides space (scope ancestry) from time (song
        // position). Neither side can take focus.
        let hair = design::px(design::space::HAIR);
        painter.line_segment(
            [
                egui::pos2(transport.min.x, vitals.min.y + hair),
                egui::pos2(transport.min.x, vitals.max.y - hair),
            ],
            egui::Stroke::new(1.0, HAIRLINE),
        );

        self.draw_breadcrumb(painter, breadcrumb);
        self.draw_transport(painter, transport);
        self.draw_message(painter, message);
        self.draw_field(painter, avail);
        // Last, and over the top of everything in the field: the browser
        // is a window above the work, not a division of it.
        self.draw_browser(painter, browser);
    }

    fn draw_field(&self, painter: &egui::Painter, avail: egui::Rect) {
        // The codebook takes the field while it is up. It is a DISPLAY
        // mode, not a scope: focus never enters it, and the cursor
        // underneath is exactly where it was left.
        if self.help {
            self.draw_help(painter, avail);
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
                if (col, row) == (focus_col, focus_row) {
                    painter.rect_filled(rect, 0.0, cursor_shade);
                } else {
                    painter.rect_filled(rect, 0.0, SQUARE);
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(1.0, SQUARE_EDGE),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }

        // The absorbed keystroke, present only on frames where a refusal
        // happened. Each limit refuses in its own geometry: an edge marks
        // its side, the root marks the whole field, the depth cap marks
        // the square that would not open.
        let field = egui::Rect::from_min_size(origin, egui::vec2(span_x, span_y));
        let inset = gap.max(4.0);
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
                let focused = egui::Rect::from_min_size(
                    origin
                        + egui::vec2(
                            focus_col as f32 * (cell + gap),
                            focus_row as f32 * (cell + gap),
                        ),
                    egui::vec2(cell, cell),
                );
                painter.rect_stroke(
                    focused.expand((gap * 0.5).max(2.0)),
                    0.0,
                    egui::Stroke::new(2.0, REFUSAL),
                    egui::StrokeKind::Outside,
                );
            }
            // A refusal that happened in the browser has no geometry in
            // the field — it belongs to the other side of the screen, and
            // the message strip is where it is reported.
            Some(RefusalReason::Empty | RefusalReason::AtTop | RefusalReason::Unavailable)
            | None => {}
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
        let Some(first) = ancestors.iter().find_map(|scope| match scope {
            FocusScope::Grid(grid) => Some(grid),
            FocusScope::Column(_) => None,
        }) else {
            return;
        };

        let mini_h = first.rows() as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP;
        let mut corner = egui::pos2(
            zone.min.x + MARGIN,
            (zone.center().y - mini_h / 2.0).floor(),
        );
        for scope in ancestors {
            let FocusScope::Grid(level) = scope else {
                continue;
            };
            let span = egui::vec2(
                level.cols() as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
                level.rows() as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
            );
            let (entered_col, entered_row) = level.cursor();
            for row in 0..level.rows() {
                for col in 0..level.cols() {
                    let rect = egui::Rect::from_min_size(
                        corner
                            + egui::vec2(
                                col as f32 * (MINI_CELL + MINI_GAP),
                                row as f32 * (MINI_CELL + MINI_GAP),
                            ),
                        egui::vec2(MINI_CELL, MINI_CELL),
                    );
                    let shade = if (col, row) == (entered_col, entered_row) {
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
        painter.rect_filled(zone, 0.0, design::GROUND.color);
        painter.line_segment(
            [
                egui::pos2(zone.max.x, zone.min.y),
                egui::pos2(zone.max.x, zone.max.y),
            ],
            egui::Stroke::new(1.0, HAIRLINE),
        );

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

        let rule = browser::glyph::RULE.to_string().repeat(inner);
        text(
            at(0, 0),
            format!(
                "{}{}{}",
                browser::glyph::CORNER_TL,
                rule,
                browser::glyph::CORNER_TR
            ),
            SQUARE_EDGE,
        );

        let query = fit_cells(
            &format!("{}{}", browser::glyph::CARET, browser.query()),
            inner,
        );
        text(
            at(0, 1),
            format!(
                "{}{}{}",
                browser::glyph::STILE,
                query,
                browser::glyph::STILE
            ),
            RESTING,
        );
        text(
            at(0, 2),
            format!(
                "{}{}{}",
                browser::glyph::TEE_L,
                browser::glyph::RULE.to_string().repeat(inner),
                browser::glyph::TEE_R
            ),
            SQUARE_EDGE,
        );

        let bottom = rows - 1;
        text(
            at(0, bottom),
            format!(
                "{}{}{}",
                browser::glyph::CORNER_BL,
                browser::glyph::RULE.to_string().repeat(inner),
                browser::glyph::CORNER_BR
            ),
            SQUARE_EDGE,
        );

        let entries: Vec<_> = browser.matches().collect();
        let status = match browser.status() {
            BrowserStatus::Scanning => Some(format!("{} SCANNING", browser::glyph::SHADE_LIGHT)),
            BrowserStatus::Unavailable => Some("PROJECT SOURCE UNAVAILABLE".to_owned()),
            BrowserStatus::Ready if entries.is_empty() => Some(if browser.query().is_empty() {
                "EMPTY".to_owned()
            } else {
                "NO MATCH".to_owned()
            }),
            BrowserStatus::Ready => None,
        };

        let content_rows = rows - 4;
        let status_rows = usize::from(status.is_some());
        let visible = content_rows.saturating_sub(status_rows);
        let cursor = browser.cursor().unwrap_or(0);
        let start = cursor
            .saturating_sub(visible / 2)
            .min(entries.len().saturating_sub(visible));

        if let Some(status) = status {
            text(at(0, 3), browser::glyph::STILE.to_string(), SQUARE_EDGE);
            text(at(1, 3), fit_cells(&status, inner), RESTING);
            text(
                at(columns - 1, 3),
                browser::glyph::STILE.to_string(),
                SQUARE_EDGE,
            );
        }

        for row in 0..visible {
            let screen_row = 3 + status_rows + row;
            text(
                at(0, screen_row),
                browser::glyph::STILE.to_string(),
                SQUARE_EDGE,
            );
            text(
                at(columns - 1, screen_row),
                browser::glyph::STILE.to_string(),
                SQUARE_EDGE,
            );

            let Some((index, entry)) = entries.get(start + row).map(|entry| (start + row, *entry))
            else {
                continue;
            };
            let words = match entry.kind {
                EntryKind::Device(kind) if kind.is_instrument() => {
                    format!("INSTRUMENT  {}", entry.label)
                }
                EntryKind::Device(_) => format!("EFFECT      {}", entry.label),
                EntryKind::Shelf(_) | EntryKind::Sample(_) | EntryKind::Project(_) => {
                    entry.label.clone()
                }
            };
            let words = fit_cells(&words, inner);
            if Some(index) == browser.cursor() {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        at(1, screen_row),
                        egui::vec2(inner as f32 * cell.x, cell.y),
                    ),
                    0.0,
                    FOCUSED,
                );
                text(at(1, screen_row), words, design::GROUND.color);
            } else {
                text(at(1, screen_row), words, design::INK.color);
            }
        }

        // With no addressable row, the typing caret becomes the one focus
        // signal. When a row exists its inversion is the signal instead.
        if browser.cursor().is_none() {
            painter.rect_filled(
                egui::Rect::from_min_size(at(1, 1), egui::vec2(cell.x, cell.y)),
                0.0,
                FOCUSED,
            );
            text(
                at(1, 1),
                browser::glyph::CARET.to_string(),
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
            return;
        };
        let words = match refusal.reason {
            RefusalReason::Edge(Step::Up) => "REFUSED · EDGE UP",
            RefusalReason::Edge(Step::Down) => "REFUSED · EDGE DOWN",
            RefusalReason::Edge(Step::Left) => "REFUSED · EDGE LEFT",
            RefusalReason::Edge(Step::Right) => "REFUSED · EDGE RIGHT",
            RefusalReason::Deeper => "REFUSED · DEPTH LIMIT",
            RefusalReason::Shallower => "REFUSED · NO FURTHER OUT",
            RefusalReason::Empty => "REFUSED · NOTHING HERE YET",
            RefusalReason::AtTop => "REFUSED · ALREADY AT TOP",
            RefusalReason::Unavailable => "REFUSED · NO ACTION YET",
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

    fn type_text(stage: &mut Stage, text: &str) -> Vec<ApplyOutcome> {
        text.chars()
            .map(|ch| {
                stage
                    .handle_input(keymap::StageInput::Text(ch))
                    .unwrap_or_else(|| panic!("unbound text in stage sequence: {ch:?}"))
            })
            .collect()
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
    fn a_key_sequence_descends_moves_and_returns_to_its_parent_cursor() {
        let mut stage = Stage::new();
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
        assert_eq!(stage.focus.depth(), 1);
        assert_eq!(active_grid(&stage).cursor(), (0, 2));
    }

    #[test]
    fn key_sequences_clamp_at_all_four_edges_and_name_each_refusal() {
        let mut stage = Stage::new();
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
        drive(&mut stage, &[Key::ArrowRight, Key::ArrowDown, Key::Enter]);
        assert_eq!(active_grid(&stage).cursor(), (0, 0));

        drive(&mut stage, &[Key::ArrowDown, Key::ArrowDown, Key::Enter]);
        drive(&mut stage, &[Key::ArrowRight, Key::Escape]);
        assert_eq!(stage.focus.depth(), 2);
        assert_eq!(active_grid(&stage).cursor(), (0, 2));

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.focus.depth(), 1);
        assert_eq!(active_grid(&stage).cursor(), (1, 1));
    }

    #[test]
    fn step_keys_only_move_the_active_scope() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::Enter, Key::ArrowRight]);

        assert_eq!(grid_at(&stage, 0).cursor(), (0, 0));
        assert_eq!(active_grid(&stage).cursor(), (1, 0));
    }

    #[test]
    fn latest_refusal_survives_a_later_success_in_the_same_sequence() {
        let mut stage = Stage::new();
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

    /// Escape means up and out, in one order: the codebook, then a browser
    /// shelf, then the browser itself, then a scope. Never two at once.
    #[test]
    fn escape_leaves_the_outermost_thing_first() {
        let mut stage = Stage::new();
        drive(&mut stage, &[Key::Enter]);
        let _ = command(&mut stage, Key::F);
        drive(&mut stage, &[Key::Enter]);
        assert_eq!(
            stage.browser.as_ref().and_then(Browser::shelf),
            Some(Shelf::Devices)
        );
        drive(&mut stage, &[Key::Questionmark]);

        drive(&mut stage, &[Key::Escape]);
        assert!(!stage.help, "the codebook goes first");
        assert!(stage.browser.is_some());
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(
            stage.browser.as_ref().and_then(Browser::shelf),
            None,
            "the shelf goes next"
        );
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert!(stage.browser.is_none(), "then the browser goes");
        assert_eq!(stage.focus.depth(), 2);

        drive(&mut stage, &[Key::Escape]);
        assert_eq!(stage.focus.depth(), 1, "and only then does a scope");
    }

    /// The library opens at its shelves, and the cursor walks them with
    /// the same clamping grammar the field uses.
    #[test]
    fn the_browser_opens_on_its_shelves_and_walks_them() {
        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);

        let browser = stage.browser.as_ref().expect("summoned");
        assert_eq!(browser.matches().count(), Shelf::ALL.len());
        assert_eq!(
            browser.selected().map(|entry| entry.label.as_str()),
            Some("DEVICES")
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
                .map(|entry| entry.label.as_str()),
            Some("SAMPLES")
        );
    }

    #[test]
    fn enter_descends_into_the_device_shelf_and_escape_climbs_back() {
        let mut stage = Stage::new();
        let _ = command(&mut stage, Key::F);

        assert_eq!(
            drive(&mut stage, &[Key::Enter]),
            vec![ApplyOutcome::Changed]
        );
        let browser = stage.browser.as_ref().expect("browser remains open");
        assert_eq!(browser.shelf(), Some(Shelf::Devices));
        assert_eq!(browser.matches().count(), crate::devices::DEVICES.len());

        assert_eq!(
            drive(&mut stage, &[Key::Escape]),
            vec![ApplyOutcome::Changed]
        );
        let browser = stage
            .browser
            .as_ref()
            .expect("escape climbed, not dismissed");
        assert_eq!(browser.shelf(), None);
        assert_eq!(browser.matches().count(), Shelf::ALL.len());
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
        assert!(
            stage
                .browser
                .as_ref()
                .expect("open")
                .matches()
                .all(|entry| browser::matches_query(&entry.label, "sn"))
        );

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
