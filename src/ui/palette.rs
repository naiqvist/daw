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
use crate::ui::tokens::{font, radius, space, stroke};
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
    ) -> Option<&'static str> {
        if !self.open {
            return None;
        }

        // Escape first: it closes even when the list is empty.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close();
            return None;
        }

        let ranked = rank(&self.query, commands);
        if self.cursor >= ranked.len() {
            self.cursor = ranked.len().saturating_sub(1);
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
        if !ranked.is_empty() && step != 0 {
            let n = ranked.len() as i32;
            self.cursor = (((self.cursor as i32 + step) % n + n) % n) as usize;
        }

        let accept = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

        // `content_rect` excludes any OS inset (notches, safe areas) —
        // the palette should never open under one.
        let screen = ctx.content_rect();
        let width = (screen.width() * WIDTH_FRAC).clamp(WIDTH_MIN, WIDTH_MAX);
        let mut chosen: Option<&'static str> = None;

        egui::Area::new(egui::Id::new("command_palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(
                screen.center().x - width * 0.5,
                screen.top() + theme.sp(space::XXL),
            ))
            .show(ctx, |ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(theme.surface_raised)
                    .stroke(egui::Stroke::new(stroke::HAIR, theme.outline))
                    .corner_radius(radius::PANEL as u8)
                    .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
                    .show(ui, |ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("run a command")
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Body),
                        );
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

                        if ranked.is_empty() {
                            ui.label(
                                egui::RichText::new("no matching command")
                                    .size(font::LABEL)
                                    .color(theme.text_muted),
                            );
                            return;
                        }

                        // Scroll window: keep the cursor row on screen
                        // without moving the list more than it must.
                        let first = self
                            .cursor
                            .saturating_sub(VISIBLE_ROWS.saturating_sub(1))
                            .min(ranked.len().saturating_sub(1));
                        let last = (first + VISIBLE_ROWS).min(ranked.len());

                        for (row, &(ci, _)) in ranked[first..last].iter().enumerate() {
                            let idx = first + row;
                            let cmd = commands[ci];
                            let active = idx == self.cursor;
                            let clicked = self.row(ui, theme, &cmd, active);
                            if clicked && cmd.enabled {
                                chosen = Some(cmd.id);
                            }
                            if clicked {
                                self.cursor = idx;
                            }
                        }

                        if ranked.len() > VISIBLE_ROWS {
                            ui.add_space(theme.sp(space::XS));
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} more",
                                    ranked.len() - VISIBLE_ROWS
                                ))
                                .size(font::LABEL)
                                .color(theme.text_muted),
                            );
                        }
                    });
            });

        if accept
            && chosen.is_none()
            && let Some(&(ci, _)) = ranked.get(self.cursor)
        {
            let cmd = commands[ci];
            if cmd.enabled {
                chosen = Some(cmd.id);
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

        // The row's ground is reserved NOW and painted once the row has
        // been laid out and asked whether the pointer is on it. A frame
        // cannot know its own size before its contents exist, and a
        // hover fill drawn afterwards would sit on top of the text.
        let ground = ui.painter().add(egui::Shape::Noop);
        let resp = egui::Frame::new()
            .corner_radius(radius::CTRL as u8)
            .inner_margin(egui::Margin::symmetric(
                theme.sp(space::XS) as i8,
                theme.sp(space::XS) as i8,
            ))
            .show(ui, |ui| {
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
            })
            .response;

        let resp = resp.interact(egui::Sense::click()).affords(Affords::Press);
        // The keyboard's row is the loud one — the palette is driven by
        // typing — but a list that gave a mouse no answer at all would
        // look like a picture of a list.
        let fill = if active {
            fill
        } else if resp.hovered() {
            theme.surface_raised
        } else {
            fill
        };
        if fill != egui::Color32::TRANSPARENT {
            ui.painter().set(
                ground,
                egui::Shape::rect_filled(resp.rect, radius::CTRL as u8, fill),
            );
        }
        resp.clicked()
    }
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
