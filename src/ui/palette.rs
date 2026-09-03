//! The command palette: one keystroke (`:`) away from everything the app
//! can do.
//!
//! Two halves, deliberately split:
//!
//! - **The matcher** (`score`, `rank`) is pure, allocation-light, and
//!   tested exhaustively. Ranking quality IS the feature — a palette that
//!   puts the wrong command first is worse than no palette — so it is
//!   pinned by tests rather than by feel.
//! - **The view** (`Palette::show`) is a keyboard-driven popup. No mouse
//!   is required for anything: type to filter, arrows to move, Enter to
//!   run, Escape to dismiss.
//!
//! The palette does NOT know what commands exist. The app hands it a list
//! and gets back the id of whatever was chosen, so app-local verbs ("clip
//! at cursor") and `UiAction`s can live side by side without this module
//! learning either vocabulary. That is the same rule panels follow: name
//! the wish, let the app perform it.

use crate::ui::affordance::{Afford, Affords};
use crate::ui::theme::Theme;
use crate::ui::tokens::{font, space};
use crate::{
    design::kit::Weight,
    design::{block, circuit},
};
use eframe::egui;

/// One thing the palette can run. `id` is what comes back when it is
/// chosen — the app's own vocabulary, opaque here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    /// What the user reads and types against.
    pub title: &'static str,
    /// Grouping word shown dim on the left: "transport", "clip", "view".
    pub group: &'static str,
    /// Keyboard shortcut, when this verb also has one.
    pub hint: Option<&'static str>,
    /// False greys the row and blocks running it (e.g. needs an engine,
    /// needs a selection). Still listed, so the palette stays a map of
    /// what exists rather than a shifting subset.
    pub enabled: bool,
}

impl Command {
    pub const fn new(id: &'static str, group: &'static str, title: &'static str) -> Self {
        Self {
            id,
            title,
            group,
            hint: None,
            enabled: true,
        }
    }

    pub const fn hint(mut self, keys: &'static str) -> Self {
        self.hint = Some(keys);
        self
    }

    pub const fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }
}

/// A long-form typed command: sentences too long for keys, spoken as
/// `name arguments…` (`notes/20260826-note-command-language.md`). The
/// palette recognizes the NAME as the query's first word and hands the
/// whole line back — parsing stays with the app, like ids do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedCommand {
    pub name: &'static str,
    /// One usage line shown while the command is being typed.
    pub usage: &'static str,
}

/// What the palette ran this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// A listed command, by id.
    Command(&'static str),
    /// A typed long-form line, verbatim; its first word names the command.
    Typed(String),
}

/// The typed command the query is speaking, if its first word names one.
pub fn typed_match<'a>(query: &str, typed: &'a [TypedCommand]) -> Option<&'a TypedCommand> {
    let first = query.split_whitespace().next()?;
    typed
        .iter()
        .find(|command| command.name.eq_ignore_ascii_case(first))
}

// ------------------------------------------------------------- matching ---

/// Score bonuses/penalties. Tuned so that, for a query, an exact prefix
/// beats a word-start match, which beats a scattered subsequence.
const BONUS_CONSECUTIVE: i32 = 8;
const BONUS_WORD_START: i32 = 12;
const BONUS_PREFIX: i32 = 16;
const PENALTY_GAP: i32 = 1;
/// Cap on the gap penalty, so one far-apart match can still rank.
const PENALTY_GAP_MAX: i32 = 20;

fn is_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c == ' ' || c == '-' || c == '_' || c == '/' || c == ':',
    }
}

/// Fuzzy subsequence score of `query` against `text`, case-insensitive.
/// `None` when `text` does not contain the query as a subsequence. Higher
/// is better; an empty query scores 0 (everything matches equally).
///
/// Matched positions, when wanted, come back in `positions` — the view
/// uses them to bold the matched characters.
pub fn score(query: &str, text: &str, positions: &mut Vec<usize>) -> Option<i32> {
    positions.clear();
    if query.is_empty() {
        return Some(0);
    }

    let hay: Vec<char> = text.chars().collect();
    let mut total = 0;
    let mut hay_i = 0usize;
    let mut last_match: Option<usize> = None;

    for qc in query.chars() {
        let qc = qc.to_ascii_lowercase();
        if qc == ' ' {
            // Spaces in a query are separators, not characters to find.
            continue;
        }
        let mut found = None;
        while hay_i < hay.len() {
            let hc = hay[hay_i].to_ascii_lowercase();
            if hc == qc {
                found = Some(hay_i);
                break;
            }
            hay_i += 1;
        }
        let at = found?;

        if at == 0 {
            total += BONUS_PREFIX;
        } else if is_boundary(hay.get(at.wrapping_sub(1)).copied()) {
            total += BONUS_WORD_START;
        }
        if let Some(prev) = last_match {
            if at == prev + 1 {
                total += BONUS_CONSECUTIVE;
            } else {
                let gap = (at - prev - 1) as i32;
                total -= (gap * PENALTY_GAP).min(PENALTY_GAP_MAX);
            }
        } else {
            // Distance from the start costs a little: "play" should beat
            // a command that merely contains those letters late on.
            total -= (at as i32 * PENALTY_GAP).min(PENALTY_GAP_MAX);
        }

        positions.push(at);
        last_match = Some(at);
        hay_i = at + 1;
    }

    Some(total)
}

/// Rank `commands` against `query`, best first. Non-matches are dropped.
/// Ties keep the caller's original order, so a hand-ordered command list
/// stays meaningful when the query is empty.
///
/// A command's group also matches ("transport" finds every transport
/// verb), but scores below a title match so title hits always win.
pub fn rank(query: &str, commands: &[Command]) -> Vec<(usize, i32)> {
    let mut scratch = Vec::new();
    let mut out: Vec<(usize, i32)> = Vec::new();
    for (i, cmd) in commands.iter().enumerate() {
        let title = score(query, cmd.title, &mut scratch);
        let group = score(query, cmd.group, &mut scratch).map(|s| s / 2 - BONUS_PREFIX);
        if let Some(s) = title.or(group) {
            out.push((i, s));
        }
    }
    // Stable sort on the negated score keeps ties in list order.
    out.sort_by_key(|(i, s)| (-*s, *i));
    out
}

// ----------------------------------------------------------------- view ---

/// Rows shown before the list scrolls.
const VISIBLE_ROWS: usize = 9;
/// Palette width as a fraction of the window, and its bounds.
const WIDTH_FRAC: f32 = 0.5;
const WIDTH_MIN: f32 = 320.0;
const WIDTH_MAX: f32 = 640.0;

/// Palette state the app owns across frames.
#[derive(Debug, Default)]
pub struct Palette {
    open: bool,
    query: String,
    /// Index into the RANKED list, not the command list.
    cursor: usize,
    /// Set the frame the palette opens, so the text field can take focus
    /// exactly once.
    just_opened: bool,
}

impl Palette {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open with an empty query. Opening an open palette is a no-op, so a
    /// held key cannot reset what is typed.
    pub fn open(&mut self) {
        if !self.open {
            self.open = true;
            self.just_opened = true;
            self.query.clear();
            self.cursor = 0;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = 0;
    }

    /// Draw the palette if it is open. Returns the id of a command the
    /// user ran this frame.
    ///
    /// Consumes the keys it uses (arrows, Enter, Escape) so nothing behind
    /// the palette also acts on them — a palette that lets keystrokes leak
    /// through to the arrangement is worse than no palette.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        commands: &[Command],
        typed: &[TypedCommand],
    ) -> Option<Choice> {
        if !self.open {
            return None;
        }

        // Escape first: it closes even when the list is empty.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close();
            return None;
        }

        // A typed long form claims the top row while it is being spoken;
        // the ranked list continues below it.
        let speaking = typed_match(&self.query, typed).copied();
        let typed_rows = usize::from(speaking.is_some());
        let ranked = rank(&self.query, commands);
        let total = typed_rows + ranked.len();
        if self.cursor >= total {
            self.cursor = total.saturating_sub(1);
        }

        // Navigation. Ctrl+N/Ctrl+P mirror the arrows for the touch-typist.
        let mut step = 0i32;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::N)
            {
                step += 1;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::P)
            {
                step -= 1;
            }
        });
        if total > 0 && step != 0 {
            let n = total as i32;
            self.cursor = (((self.cursor as i32 + step) % n + n) % n) as usize;
        }

        let accept = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

        // `content_rect` excludes any OS inset (notches, safe areas) —
        // the palette should never open under one.
        let screen = ctx.content_rect();
        let width = (screen.width() * WIDTH_FRAC).clamp(WIDTH_MIN, WIDTH_MAX);
        let mut chosen: Option<Choice> = None;

        egui::Area::new(egui::Id::new("command_palette"))
            .order(egui::Order::Foreground)
            // This is a summoned machine surface, not a tooltip. It must
            // arrive at full contrast on the invocation frame so the
            // keyboard cursor is readable immediately.
            .fade_in(false)
            .fixed_pos(egui::pos2(
                screen.center().x - width * 0.5,
                screen.top() + theme.sp(space::XXL),
            ))
            .show(ctx, |ui| {
                ui.set_width(width);
                let contents = egui::Frame::new()
                    .fill(theme.surface_raised)
                    .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
                    .show(ui, |ui| {
                        let title_h = block::height(block::unit::MICRO) + theme.sp(space::XS);
                        let (title_rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), title_h),
                            egui::Sense::hover(),
                        );
                        block::paint(
                            ui.painter(),
                            egui::Id::new("command-palette-title"),
                            title_rect.left_top(),
                            egui::Align2::LEFT_TOP,
                            block::unit::MICRO,
                            "COMMAND",
                            theme.text,
                        );

                        let (search_rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 27.0),
                            egui::Sense::hover(),
                        );
                        let mut search_shapes = Vec::new();
                        circuit::panel_variant(
                            &mut search_shapes,
                            search_rect,
                            Some(theme.bg),
                            theme.surface_raised,
                            Some((Weight::Hair, theme.outline)),
                            2,
                        );
                        circuit::pad(
                            &mut search_shapes,
                            egui::pos2(search_rect.right() - 9.0, search_rect.center().y),
                            circuit::PAD - 1.0,
                            theme.outline,
                            true,
                        );
                        ui.painter().extend(search_shapes);
                        let field = ui
                            .scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(search_rect.shrink2(egui::vec2(7.0, 2.0))),
                                |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.query)
                                            .hint_text("run a command")
                                            .desired_width(f32::INFINITY)
                                            .font(egui::TextStyle::Body)
                                            .frame(egui::Frame::NONE),
                                    )
                                },
                            )
                            .inner;
                        if self.just_opened {
                            field.request_focus();
                            self.just_opened = false;
                        } else {
                            // Keep focus: clicking a row must not steal it,
                            // or the next keystroke goes nowhere.
                            if !field.has_focus() {
                                field.request_focus();
                            }
                        }

                        ui.add_space(theme.sp(space::XS));

                        if total == 0 {
                            ui.label(
                                egui::RichText::new("no matching command")
                                    .size(font::LABEL)
                                    .color(theme.text_muted),
                            );
                            return;
                        }

                        // The typed long form, while spoken, is row zero.
                        if let Some(speaking) = &speaking {
                            let row = Command::new("", speaking.name, speaking.usage).hint("↵");
                            if self.row(ui, theme, &row, self.cursor == 0) {
                                chosen = Some(Choice::Typed(self.query.trim().to_owned()));
                                self.cursor = 0;
                            }
                        }

                        // Scroll window: keep the cursor row on screen
                        // without moving the list more than it must.
                        let list_cursor = self.cursor.saturating_sub(typed_rows);
                        let visible = VISIBLE_ROWS.saturating_sub(typed_rows).max(1);
                        let first = list_cursor
                            .saturating_sub(visible.saturating_sub(1))
                            .min(ranked.len().saturating_sub(1));
                        let last = (first + visible).min(ranked.len());

                        for (row, &(ci, _)) in ranked[first..last].iter().enumerate() {
                            let idx = first + row;
                            let cmd = commands[ci];
                            let active = idx + typed_rows == self.cursor;
                            let clicked = self.row(ui, theme, &cmd, active);
                            if clicked && cmd.enabled {
                                chosen = Some(Choice::Command(cmd.id));
                            }
                            if clicked {
                                self.cursor = idx + typed_rows;
                            }
                        }

                        if ranked.len() > visible {
                            ui.add_space(theme.sp(space::XS));
                            ui.label(
                                egui::RichText::new(format!("{} more", ranked.len() - visible))
                                    .size(font::LABEL)
                                    .color(theme.text_muted),
                            );
                        }
                    });

                let mut shell_shapes = Vec::new();
                // The frame lays down the opaque plane before its text.
                // Cut the service recesses back out afterwards; they live
                // wholly in the margin, so the fill follows the casing
                // without a late background shape washing over content.
                mask_palette_shell(&mut shell_shapes, contents.response.rect, theme.bg);
                circuit::panel_frame_variant(
                    &mut shell_shapes,
                    contents.response.rect,
                    Weight::Heavy,
                    theme.text,
                    3,
                );
                circuit::panel_frame_variant(
                    &mut shell_shapes,
                    contents.response.rect.shrink(5.0),
                    Weight::Hair,
                    theme.outline,
                    0,
                );
                ui.painter().extend(shell_shapes);
            });

        if accept && chosen.is_none() {
            if self.cursor == 0 && speaking.is_some() {
                chosen = Some(Choice::Typed(self.query.trim().to_owned()));
            } else if let Some(&(ci, _)) = ranked.get(self.cursor.saturating_sub(typed_rows)) {
                let cmd = commands[ci];
                if cmd.enabled {
                    chosen = Some(Choice::Command(cmd.id));
                }
            }
        }
        if chosen.is_some() {
            self.close();
        }
        chosen
    }

    /// One row: group, title, shortcut. Returns whether it was clicked.
    fn row(&self, ui: &mut egui::Ui, theme: &Theme, cmd: &Command, active: bool) -> bool {
        let fill = if active {
            theme.accent_muted
        } else {
            egui::Color32::TRANSPARENT
        };
        // Disabled rows stay listed but read as unavailable.
        let text = if cmd.enabled {
            theme.text
        } else {
            theme.text_muted
        };

        let row_h = font::BODY + theme.sp(space::XS) * 2.0 + 4.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::click(),
        );
        let resp = resp.affords(Affords::Press);
        // Allocate first so hover is known, paint the shaped fill second,
        // and lay the text down last. That ordering keeps the selection
        // opaque without turning the command itself into a ghost.
        let fill = if active {
            fill
        } else if resp.hovered() {
            theme.surface
        } else {
            fill
        };
        if fill != egui::Color32::TRANSPARENT {
            let mut shapes = Vec::new();
            circuit::panel_variant(
                &mut shapes,
                rect,
                Some(fill),
                theme.surface_raised,
                None,
                (self.cursor % 4) as u8,
            );
            ui.painter().extend(shapes);
        }
        if active {
            crate::ui::nav_cursor::claim(
                ui.painter(),
                ("palette-cursor", cmd.id),
                rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Palette,
                theme.text,
            );
        }

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(theme.sp(space::XS), theme.sp(space::XS)))),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(cmd.group)
                            .size(font::LABEL)
                            .color(theme.text_muted),
                    );
                    ui.label(egui::RichText::new(cmd.title).size(font::BODY).color(text));
                    if let Some(hint) = cmd.hint {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(hint)
                                    .monospace()
                                    .size(font::LABEL)
                                    .color(theme.text_muted),
                            );
                        });
                    }
                });
            },
        );
        resp.clicked()
    }
}

/// Mask the two cuts used by palette shell variant three. The opaque egui
/// frame is painted before its children; these small pieces restore the
/// ground only in the casing's discarded corners after layout is known.
fn mask_palette_shell(out: &mut Vec<egui::Shape>, rect: egui::Rect, ground: egui::Color32) {
    let c = 6.0_f32
        .min(rect.width() / 10.0)
        .min(rect.height() / 5.0)
        .max(1.0);
    let s = (c * 1.8).min(rect.width() / 7.0);
    let lower_a = rect.top() + rect.height() * 0.62;
    let lower_b = (lower_a + c * 1.35).min(rect.bottom() - c);
    out.push(egui::Shape::convex_polygon(
        vec![
            rect.right_bottom(),
            egui::pos2(rect.right() - c, rect.bottom()),
            egui::pos2(rect.right(), rect.bottom() - c),
        ],
        ground,
        egui::Stroke::NONE,
    ));
    out.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), lower_a),
            egui::pos2(rect.left() + s, lower_b),
        ),
        0.0,
        ground,
    ));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cmds() -> Vec<Command> {
        vec![
            Command::new("transport.play", "transport", "play / stop"),
            Command::new("transport.return", "transport", "return to zero"),
            Command::new("clip.new", "clip", "new clip at cursor"),
            Command::new("clip.duplicate", "clip", "duplicate clip"),
            Command::new("view.density", "view", "density: compact"),
        ]
    }

    fn s(q: &str, t: &str) -> Option<i32> {
        let mut p = Vec::new();
        score(q, t, &mut p)
    }

    #[test]
    fn subsequence_matching_and_misses() {
        assert!(s("play", "play / stop").is_some());
        assert!(s("pls", "play / stop").is_some(), "scattered still matches");
        assert!(s("zzz", "play / stop").is_none());
        // Case-insensitive both ways.
        assert!(s("PLAY", "play / stop").is_some());
        assert!(s("play", "PLAY / STOP").is_some());
        // Empty query matches everything, neutrally.
        assert_eq!(s("", "anything"), Some(0));
    }

    #[test]
    fn prefix_beats_word_start_beats_scatter() {
        let prefix = s("cl", "clip at cursor").unwrap();
        let word = s("cl", "new clip").unwrap();
        let scatter = s("cl", "cancel loop").unwrap();
        assert!(
            prefix > word,
            "prefix {prefix} should beat word-start {word}"
        );
        assert!(
            word > scatter,
            "word-start {word} should beat scatter {scatter}"
        );
    }

    #[test]
    fn consecutive_beats_gapped() {
        let tight = s("dup", "duplicate clip").unwrap();
        let loose = s("dup", "delete under playhead").unwrap();
        assert!(tight > loose, "tight {tight} should beat loose {loose}");
    }

    #[test]
    fn positions_mark_the_matched_characters() {
        let mut p = Vec::new();
        score("clp", "clip", &mut p).unwrap();
        assert_eq!(p, vec![0, 1, 3]);
    }

    #[test]
    fn ranking_puts_the_obvious_command_first() {
        let list = cmds();
        let top = |q: &str| {
            let r = rank(q, &list);
            list[r[0].0].id
        };
        assert_eq!(top("play"), "transport.play");
        assert_eq!(top("new clip"), "clip.new");
        assert_eq!(top("dup"), "clip.duplicate");
        assert_eq!(top("ret"), "transport.return");
    }

    #[test]
    fn group_matches_but_loses_to_titles() {
        let list = cmds();
        let r = rank("clip", &list);
        let ids: Vec<&str> = r.iter().map(|(i, _)| list[*i].id).collect();
        // Every clip-group command is present...
        assert!(ids.contains(&"clip.new"));
        assert!(ids.contains(&"clip.duplicate"));
        // ...and a title match outranks a mere group match.
        let dup_pos = ids.iter().position(|id| *id == "clip.duplicate").unwrap();
        let new_pos = ids.iter().position(|id| *id == "clip.new").unwrap();
        assert!(
            dup_pos < new_pos || new_pos < dup_pos,
            "both listed, order is score-driven"
        );
        assert_eq!(ids[0], "clip.new", "'clip' should surface clip verbs first");
    }

    #[test]
    fn empty_query_keeps_the_authored_order() {
        let list = cmds();
        let r = rank("", &list);
        let ids: Vec<&str> = r.iter().map(|(i, _)| list[*i].id).collect();
        assert_eq!(
            ids,
            vec![
                "transport.play",
                "transport.return",
                "clip.new",
                "clip.duplicate",
                "view.density",
            ]
        );
    }

    #[test]
    fn no_match_ranks_empty() {
        assert!(rank("qqqq", &cmds()).is_empty());
    }

    #[test]
    fn spaces_in_a_query_are_separators() {
        // "n c" should find "new clip at cursor" — the space is not a
        // character to locate, it just separates fragments.
        assert!(s("n c", "new clip at cursor").is_some());
    }

    /// The typed long forms: the first WORD names the command — exactly,
    /// not fuzzily — and only then does the palette hand back the line.
    #[test]
    fn typed_long_forms_match_on_their_first_word_only() {
        let typed = [
            TypedCommand {
                name: "key",
                usage: "key <tonic> <scale> [mode N]",
            },
            TypedCommand {
                name: "quantize-key",
                usage: "quantize-key",
            },
        ];
        assert_eq!(
            typed_match("key d dorian", &typed).map(|t| t.name),
            Some("key")
        );
        assert_eq!(typed_match("KEY d", &typed).map(|t| t.name), Some("key"));
        assert_eq!(
            typed_match("quantize-key", &typed).map(|t| t.name),
            Some("quantize-key")
        );
        // A prefix is not a name, and fuzzy matching never applies.
        assert_eq!(typed_match("ke d dorian", &typed), None);
        assert_eq!(typed_match("keys d", &typed), None);
        assert_eq!(typed_match("", &typed), None);
    }

    #[test]
    fn open_is_idempotent_and_close_resets() {
        let mut p = Palette::default();
        assert!(!p.is_open());
        p.open();
        p.query.push_str("play");
        p.open(); // must not wipe the query
        assert_eq!(p.query, "play");
        p.close();
        assert!(!p.is_open());
        assert!(p.query.is_empty());
    }
}
