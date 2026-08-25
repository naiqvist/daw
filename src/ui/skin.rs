//! The theme window: every Gogh palette, picked by ear rather than by name.
//!
//! Ported from audio-workstation. Previewing IS the interface — a swatch
//! strip cannot tell you whether a scheme makes the grid readable or a
//! playhead findable, so moving the cursor applies the scheme immediately
//! and the answer is the window you are sitting in. Nothing is committed
//! until the choice is kept, and leaving puts back whatever you arrived
//! with.

use crate::ui::gogh::{SCHEMES, Scheme};
use crate::ui::theme::Theme;
use crate::ui::tokens::{font, radius, space, stroke};
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

const WIDTH: f32 = 320.0;
const ROW: f32 = 24.0;
/// Rows visible at once. The list scrolls under the cursor rather than
/// paging, so the choice either side of yours stays on screen.
const ROWS: usize = 14;
const SEARCH: f32 = 28.0;
/// The colour chips beside each name.
const CHIP: f32 = 6.0;

/// One row of the picker. The app's own theme comes first, then the
/// vendored terminal schemes — one list, because the question a person is
/// asking is "which theme", not "which kind of theme".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    House(usize),
    Gogh(usize),
}

impl Pick {
    fn name(self) -> &'static str {
        match self {
            Pick::House(_) => "House dark",
            Pick::Gogh(i) => SCHEMES[i].name,
        }
    }

    fn tokens(self) -> Theme {
        match self {
            Pick::House(_) => Theme::dark(),
            Pick::Gogh(i) => from_scheme(&SCHEMES[i]),
        }
    }

    /// The four chips beside the name: the ground, and three colours the
    /// theme is actually made of.
    fn chips(self) -> [Color32; 4] {
        match self {
            Pick::House(_) => {
                let t = Theme::dark();
                [t.bg, t.accent, t.ok, t.warn]
            }
            Pick::Gogh(i) => {
                let s = &SCHEMES[i];
                [rgb(s.bg), rgb(s.ansi[1]), rgb(s.ansi[2]), rgb(s.ansi[4])]
            }
        }
    }
}

/// Every theme, the app's own first. Built once per call; the list is
/// small and the window is not a hot path.
fn every() -> Vec<Pick> {
    (0..1)
        .map(Pick::House)
        .chain((0..SCHEMES.len()).map(Pick::Gogh))
        .collect()
}

#[derive(Default)]
pub struct Skin {
    pub open: bool,
    cursor: usize,
    query: String,
    scroll: usize,
    /// Set the frame the window opens, so any focused text field hands the
    /// keyboard over exactly once.
    just_opened: bool,
    /// What was in force when the window opened, put back on escape.
    arrived_with: Option<Pick>,
    /// The kept choice. `None` has never chosen, and wears the app's own
    /// theme.
    chosen: Option<Pick>,
}

impl Skin {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        self.just_opened = true;
        self.arrived_with = self.chosen;
        self.query.clear();
        self.cursor = self
            .chosen
            .and_then(|c| self.matches().iter().position(|p| *p == c))
            .unwrap_or(0);
        self.scroll = 0;
    }

    /// Apply whatever was kept last time, at startup. Nothing kept wears
    /// the house theme, which is what a fresh install sees.
    pub fn restore(&mut self, theme: &mut Theme) {
        self.chosen = load_choice();
        self.apply(self.chosen.unwrap_or(Pick::House(0)), theme);
    }

    fn matches(&self) -> Vec<Pick> {
        let query = self.query.trim().to_lowercase();
        every()
            .into_iter()
            .filter(|p| query.is_empty() || p.name().to_lowercase().contains(&query))
            .collect()
    }

    /// Put a theme in force, keeping the machine-local density preference:
    /// a scheme is colours only, and has no say in how tightly the UI packs.
    fn apply(&self, pick: Pick, theme: &mut Theme) {
        let density = theme.density;
        *theme = pick.tokens();
        theme.density = density;
    }

    /// Paint the theme under the cursor, without keeping it.
    fn preview(&self, hits: &[Pick], theme: &mut Theme) {
        let pick = hits.get(self.cursor).copied().unwrap_or(Pick::House(0));
        self.apply(pick, theme);
    }

    /// Put back whatever the window was entered with.
    fn revert(&self, theme: &mut Theme) {
        let pick = self.arrived_with.unwrap_or(Pick::House(0));
        self.apply(pick, theme);
    }

    /// Draw the window if it is open, and return whether it is open after
    /// this frame (keep and escape both close it).
    ///
    /// Called BEFORE the panels draw, so a preview paints the whole frame
    /// in the scheme under the cursor; the window itself is painted on
    /// egui's foreground layer, so it lands on top no matter when it runs.
    /// While open it owns the keyboard the same way the command palette
    /// does: every key it uses is consumed, and Space/Tab are swallowed so
    /// the app behind cannot act on them.
    pub fn draw(&mut self, ctx: &egui::Context, area: Rect, theme: &mut Theme) {
        if !self.open {
            return;
        }

        // A text field the user was typing in must not keep eating what is
        // typed into the search row.
        if self.just_opened {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            self.just_opened = false;
        }

        // --- keys ----------------------------------------------------
        let mut keep = false;
        let mut leave = false;
        ctx.input_mut(|i| {
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    self.query.push_str(text);
                    self.cursor = 0;
                    self.scroll = 0;
                }
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace) {
                self.query.pop();
                self.cursor = 0;
                self.scroll = 0;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                self.cursor += 1;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                self.cursor = self.cursor.saturating_sub(1);
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                keep = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                leave = true;
            }
            // The app behind must not act on anything typed in here.
            let _ = i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            let _ = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
        });

        let hits = self.matches();
        if hits.is_empty() {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(hits.len() - 1);
        }
        // Keep the cursor's row inside the visible window.
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + ROWS {
            self.scroll = self.cursor + 1 - ROWS;
        }

        if leave {
            self.revert(theme);
            theme.apply(ctx);
            self.open = false;
            return;
        }
        if keep {
            self.chosen = hits.get(self.cursor).copied();
            save_choice(self.chosen);
            theme.apply(ctx);
            self.open = false;
            return;
        }

        // Applied before drawing, so the window itself is painted in the
        // scheme it is offering.
        self.preview(&hits, theme);
        let t = theme.clone();

        // --- window --------------------------------------------------
        let height = SEARCH + ROW * ROWS as f32 + t.sp(space::LG) * 2.0 + ROW;
        let rect = Rect::from_center_size(
            Pos2::new(area.center().x, area.center().y),
            Vec2::new(WIDTH, height),
        );
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("skin")));

        // Swallow clicks so nothing behind reacts to them. A foreground
        // layer, so it stays on top of the panels no matter when it runs.
        egui::Area::new(Id::new("skin-shade"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.allocate_rect(area, Sense::click_and_drag());
            });

        // The shade behind the window.
        painter.rect_filled(
            area,
            0.0,
            Color32::from_black_alpha(if t.light { 60 } else { 102 }),
        );
        painter.rect_filled(rect, radius::PANEL, t.surface_raised);
        painter.rect_stroke(
            rect,
            radius::PANEL,
            Stroke::new(stroke::HAIR, t.outline),
            StrokeKind::Inside,
        );

        let inner = rect.shrink(t.sp(space::LG));
        painter.text(
            Pos2::new(inner.left(), inner.top() + SEARCH / 2.0),
            Align2::LEFT_CENTER,
            if self.query.is_empty() {
                "Theme\u{2026}".to_owned()
            } else {
                self.query.clone()
            },
            FontId::new(font::BODY, FontFamily::Proportional),
            if self.query.is_empty() {
                t.text_muted
            } else {
                t.text
            },
        );
        painter.hline(
            egui::Rangef::new(inner.left(), inner.right()),
            inner.top() + SEARCH,
            Stroke::new(stroke::HAIR, t.divider),
        );

        let mut y = inner.top() + SEARCH + t.sp(space::XS);
        for (row, pick) in hits.iter().skip(self.scroll).take(ROWS).enumerate() {
            let line =
                Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width(), ROW));
            let selected = self.scroll + row == self.cursor;
            if selected {
                painter.rect_filled(line, radius::CTRL, t.accent_muted);
            }

            // Four chips: the ground, and three of the theme's own hues.
            let mut x = line.left() + t.sp(space::XS);
            for color in pick.chips() {
                painter.rect_filled(
                    Rect::from_center_size(
                        Pos2::new(x + CHIP / 2.0, line.center().y),
                        Vec2::splat(CHIP),
                    ),
                    0.0,
                    color,
                );
                x += CHIP + 2.0;
            }

            painter.text(
                Pos2::new(x + t.sp(space::SM), line.center().y),
                Align2::LEFT_CENTER,
                pick.name(),
                FontId::new(font::BODY, FontFamily::Proportional),
                if selected { t.text_value } else { t.text },
            );
            if self.chosen == Some(*pick) {
                painter.text(
                    Pos2::new(line.right() - t.sp(space::XS), line.center().y),
                    Align2::RIGHT_CENTER,
                    "\u{2713}",
                    FontId::new(font::LABEL, FontFamily::Proportional),
                    t.accent,
                );
            }
            y += ROW;
        }

        painter.text(
            Pos2::new(inner.left(), inner.bottom() - ROW / 2.0),
            Align2::LEFT_CENTER,
            format!("{} themes", hits.len()),
            FontId::new(font::LABEL, FontFamily::Proportional),
            t.text_muted,
        );
        painter.text(
            Pos2::new(inner.right(), inner.bottom() - ROW / 2.0),
            Align2::RIGHT_CENTER,
            "\u{2191}\u{2193} preview  \u{21b5} keep  esc cancel",
            FontId::new(font::LABEL, FontFamily::Proportional),
            t.text_muted.gamma_multiply(0.8),
        );

        ctx.request_repaint();
    }
}

fn rgb(bits: u32) -> Color32 {
    Color32::from_rgb((bits >> 16) as u8, (bits >> 8) as u8, bits as u8)
}

/// Turn a terminal scheme into this app's roles.
///
/// Only the colours change. Spacing, radii, type and density are the app's
/// own and no scheme gets a say in them.
pub fn from_scheme(scheme: &Scheme) -> Theme {
    let bg = rgb(scheme.bg);
    let fg = rgb(scheme.fg);
    let toward = |amount: f32| mix(bg, fg, amount);
    let light = is_light(scheme.bg);
    let accent = pick_accent(scheme).unwrap_or(fg);
    // Red, yellow and green mean the same things here as in a terminal, so
    // they come straight across rather than being invented.
    let danger = visible(scheme, [9, 1]).unwrap_or(accent);
    let warn = visible(scheme, [11, 3]).unwrap_or(accent);
    let ok = visible(scheme, [10, 2]).unwrap_or(accent);
    // The meter zones are the status colours, hotter.
    let hot = |c: Color32| mix(c, Color32::WHITE, if light { 0.08 } else { 0.22 });

    Theme {
        light,
        bg,
        surface: toward(0.05),
        surface_raised: toward(0.10),
        // A well is deeper than its ground in a dark theme and in shadow in
        // a light one — darker than the ground either way.
        surface_sunken: mix(bg, Color32::BLACK, if light { 0.10 } else { 0.35 }),
        text: toward(0.80),
        text_muted: toward(0.45),
        text_value: fg,
        outline: toward(0.22),
        divider: toward(0.12),
        focus: accent,
        accent,
        accent_muted: mix(accent, bg, 0.55),
        ok,
        warn,
        danger,
        red_zone: hot(danger),
        green_zone: hot(ok),
        playhead: hot(warn),
        loop_region: Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 0x22),
        // The brace is the region's own colour at full strength, pushed a
        // little toward the ink so it stands off a light ground as well as
        // a dark one.
        loop_brace: mix(accent, fg, 0.25),
        selection: Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 0x38),
        grid_beat: toward(0.06),
        grid_bar: toward(0.13),
        grid_sub: toward(0.03),
        clip_body: toward(0.15),
        clip_selected: accent,
        clip_note: toward(0.90),
        meter_low: ok,
        meter_hot: warn,
        meter_clip: danger,
        density: 1.0,
    }
}

/// The most saturated colour the scheme carries, brights first — a
/// terminal's accent is whichever hue it is proudest of. Greys are skipped
/// rather than averaged in, or a monochrome scheme picks its own background
/// and disappears.
fn pick_accent(scheme: &Scheme) -> Option<Color32> {
    scheme.ansi[8..]
        .iter()
        .chain(scheme.ansi[..8].iter())
        .copied()
        .filter(|bits| off_ground(scheme.bg, *bits) && saturation(*bits) > 0.08)
        .max_by(|a, b| saturation(*a).total_cmp(&saturation(*b)))
        .map(rgb)
}

/// The first of `slots` that stands off the ground, so a red that happens
/// to be the background is not used to mean "recording".
fn visible(scheme: &Scheme, slots: [usize; 2]) -> Option<Color32> {
    slots
        .into_iter()
        .map(|slot| scheme.ansi[slot])
        .find(|bits| off_ground(scheme.bg, *bits))
        .map(rgb)
}

fn saturation(bits: u32) -> f32 {
    let (r, g, b) = (
        (bits >> 16) as f32 / 255.0,
        ((bits >> 8) & 0xff) as f32 / 255.0,
        (bits & 0xff) as f32 / 255.0,
    );
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    if high <= 0.0 {
        0.0
    } else {
        (high - low) / high * high
    }
}

/// Whether a colour is far enough from the ground to be seen against it.
/// Some schemes take their background straight out of their own palette —
/// the C64's is its famous blue, which is also its most saturated colour —
/// and picking it would paint every accent the colour of the thing behind.
fn off_ground(ground: u32, bits: u32) -> bool {
    let split = |v: u32| {
        (
            (v >> 16) as i32,
            ((v >> 8) & 0xff) as i32,
            (v & 0xff) as i32,
        )
    };
    let (gr, gg, gb) = split(ground);
    let (r, g, b) = split(bits);
    (r - gr).abs() + (g - gg).abs() + (b - gb).abs() > 90
}

/// Rec.601 luma, which is what the eye does with the three channels.
fn is_light(bits: u32) -> bool {
    let r = (bits >> 16) as f32;
    let g = ((bits >> 8) & 0xff) as f32;
    let b = (bits & 0xff) as f32;
    0.299 * r + 0.587 * g + 0.114 * b > 128.0
}

fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(
        lerp(from.r(), to.r()),
        lerp(from.g(), to.g()),
        lerp(from.b(), to.b()),
    )
}

fn choice_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("daw").join("theme"))
}

/// The kept scheme is stored by name, not index: the table is generated
/// and its order is free to change — a number written last week would name
/// a different theme this week.
fn save_choice(pick: Option<Pick>) {
    let Some(path) = choice_path() else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let text = pick.map(Pick::name).unwrap_or("");
    if let Err(err) = std::fs::write(&path, text) {
        eprintln!("daw: could not remember the theme: {err}");
    }
}

fn load_choice() -> Option<Pick> {
    let text = std::fs::read_to_string(choice_path()?).ok()?;
    let name = text.trim();
    every().into_iter().find(|p| p.name() == name)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::tokens::Density;

    fn scheme(name: &str) -> &'static Scheme {
        SCHEMES.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn dark_and_light_grounds_are_detected() {
        assert!(!from_scheme(scheme("Nord")).light);
        assert!(from_scheme(scheme("Solarized Light")).light);
    }

    #[test]
    fn a_scheme_keeps_its_ground_and_ink() {
        let s = scheme("Gruvbox Dark");
        let t = from_scheme(s);
        assert_eq!(t.bg, rgb(s.bg));
        assert_eq!(t.text_value, rgb(s.fg));
    }

    #[test]
    fn the_accent_never_becomes_the_background() {
        // The C64 scheme takes its background out of its own palette —
        // its most saturated colour — so a naive pick paints every accent
        // the colour of the thing behind.
        let t = from_scheme(scheme("C64"));
        assert_ne!(t.accent, t.bg);
    }

    #[test]
    fn a_scheme_does_not_move_the_furniture() {
        let house = Theme::dark();
        let t = from_scheme(scheme("Tokyo Night"));
        assert_eq!(t.density, house.density);
        // The scheme decides colours, never how tightly the UI packs.
        for (a, b) in [
            (t.surface, t.bg),
            (t.surface_raised, t.bg),
            (t.clip_body, t.bg),
        ] {
            assert_ne!(a, b, "roles must stay distinct");
        }
    }

    #[test]
    fn the_density_preference_survives_a_theme_swap() {
        let mut theme = Theme::dark().with_density(Density::Compact);
        let skin = Skin::default();
        skin.apply(Pick::Gogh(0), &mut theme);
        assert_eq!(theme.density, Density::Compact.scale());
    }

    #[test]
    fn search_filters_by_name() {
        let skin = Skin {
            open: true,
            query: "nord".to_owned(),
            ..Skin::default()
        };
        let hits = skin.matches();
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .all(|p| p.name().to_lowercase().contains("nord"))
        );
    }

    #[test]
    fn every_list_starts_with_the_house_theme() {
        let all = every();
        assert_eq!(all.first(), Some(&Pick::House(0)));
        assert_eq!(all.len(), SCHEMES.len() + 1);
    }
}
