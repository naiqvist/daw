//! The theme window: authored schemes, previewed in place.
//!
//! Previewing IS the interface — a swatch strip cannot tell you whether a
//! scheme makes the grid readable or a playhead findable, so moving the
//! cursor applies the scheme immediately. Nothing is committed until the
//! choice is kept, and leaving puts back whatever you arrived with.

use crate::ui::theme::Theme;
use crate::ui::tokens::{font, radius, space, stroke};
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

const WIDTH: f32 = 320.0;
const ROW: f32 = 24.0;
const ROWS: usize = 4;
const SEARCH: f32 = 28.0;
/// The colour chips beside each name.
const CHIP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Dark,
    Light,
    Industrial,
    Cyberpunk,
}

impl Pick {
    fn name(self) -> &'static str {
        match self {
            Pick::Dark => "Dark",
            Pick::Light => "Light",
            Pick::Industrial => "Industrial",
            Pick::Cyberpunk => "Cyberpunk",
        }
    }

    fn tokens(self) -> Theme {
        match self {
            Pick::Dark => Theme::dark(),
            Pick::Light => Theme::light(),
            Pick::Industrial => Theme::industrial(),
            Pick::Cyberpunk => Theme::cyberpunk(),
        }
    }

    /// The four chips beside the name: the ground, and three colours the
    /// theme is actually made of.
    fn chips(self) -> [Color32; 4] {
        match self {
            Pick::Dark => {
                let t = Theme::dark();
                [t.bg, t.accent, t.ok, t.warn]
            }
            Pick::Light => {
                let t = Theme::light();
                [t.bg, t.accent, t.ok, t.warn]
            }
            Pick::Industrial => {
                let t = Theme::industrial();
                [t.bg, t.accent, t.clip_audio_header, t.playhead]
            }
            Pick::Cyberpunk => {
                let t = Theme::cyberpunk();
                [t.bg, t.accent, t.role_time, t.warn]
            }
        }
    }
}

fn every() -> Vec<Pick> {
    vec![Pick::Dark, Pick::Light, Pick::Industrial, Pick::Cyberpunk]
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
    /// Restrict this picker to the house dark/light polarity. Stage uses
    /// this form so its design alphabet and runtime theme cannot disagree;
    /// the legacy frame retains the wider authored-scheme list.
    house_only: bool,
}

impl Skin {
    /// A picker containing only the two house-ground polarities.
    pub fn house() -> Self {
        Self {
            house_only: true,
            ..Self::default()
        }
    }

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
        self.chosen = load_choice().filter(|pick| self.allows(*pick));
        self.apply(self.chosen.unwrap_or(Pick::Dark), theme);
    }

    fn choices(&self) -> Vec<Pick> {
        if self.house_only {
            vec![Pick::Dark, Pick::Light]
        } else {
            every()
        }
    }

    fn allows(&self, pick: Pick) -> bool {
        !self.house_only || matches!(pick, Pick::Dark | Pick::Light)
    }

    fn matches(&self) -> Vec<Pick> {
        let query = self.query.trim().to_lowercase();
        self.choices()
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
        let pick = hits.get(self.cursor).copied().unwrap_or(Pick::Dark);
        self.apply(pick, theme);
    }

    /// Put back whatever the window was entered with.
    fn revert(&self, theme: &mut Theme) {
        let pick = self.arrived_with.unwrap_or(Pick::Dark);
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

fn choice_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("daw").join("theme"))
}

/// The kept scheme is stored by name, not index, so the preference remains
/// readable and stable if the picker order changes.
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
    parse_choice(text.trim())
}

fn parse_choice(name: &str) -> Option<Pick> {
    every().into_iter().find(|p| p.name() == name).or_else(|| {
        // Builds before the two-theme reset stored one of hundreds of Gogh
        // names. Preserve the broad side of that choice: explicitly light
        // palettes migrate to Light; every dark or unknown palette returns
        // to the house Dark scheme.
        (!name.is_empty()).then(|| {
            if name.to_ascii_lowercase().contains("light") {
                Pick::Light
            } else {
                Pick::Dark
            }
        })
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::tokens::Density;

    fn luminance(color: Color32) -> f32 {
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (bright, dark) = if luminance(a) >= luminance(b) {
            (luminance(a), luminance(b))
        } else {
            (luminance(b), luminance(a))
        };
        (bright + 0.05) / (dark + 0.05)
    }

    #[test]
    fn authored_schemes_have_the_expected_grounds_and_distinct_roles() {
        let dark = Theme::dark();
        let light = Theme::light();
        let cyberpunk = Theme::cyberpunk();
        let industrial = Theme::industrial();
        assert!(!dark.light);
        assert!(light.light);
        assert!(!industrial.light);
        assert!(!cyberpunk.light);
        for theme in [dark, light, industrial, cyberpunk] {
            assert_ne!(theme.surface, theme.bg);
            assert_ne!(theme.surface_raised, theme.bg);
            assert_ne!(theme.surface_sunken, theme.bg);
            assert_ne!(theme.text, theme.bg);
            assert_ne!(theme.accent, theme.bg);
            assert_ne!(theme.clip_body, theme.clip_note);
            assert_ne!(theme.grid_bar, theme.grid_beat);
            assert_ne!(theme.grid_beat, theme.grid_sub);
        }
    }

    #[test]
    fn dark_schemes_have_readable_text_and_ordered_elevations() {
        for theme in [Theme::dark(), Theme::industrial(), Theme::cyberpunk()] {
            assert!(contrast(theme.text, theme.bg) >= 7.0);
            assert!(contrast(theme.text_muted, theme.bg) >= 4.5);
            assert!(contrast(theme.text, theme.surface_raised) >= 7.0);

            assert!(luminance(theme.surface_sunken) < luminance(theme.bg));
            assert!(luminance(theme.bg) < luminance(theme.surface));
            assert!(luminance(theme.surface) < luminance(theme.surface_raised));
            assert!(luminance(theme.grid_sub) < luminance(theme.grid_beat));
            assert!(luminance(theme.grid_beat) < luminance(theme.grid_bar));
            assert!(luminance(theme.surface_raised) < luminance(theme.clip_body));
        }
    }

    #[test]
    fn the_density_preference_survives_a_theme_swap() {
        let mut theme = Theme::dark().with_density(Density::Compact);
        let skin = Skin::default();
        skin.apply(Pick::Light, &mut theme);
        assert_eq!(theme.density, Density::Compact.scale());
        assert!(theme.light);
    }

    #[test]
    fn search_filters_by_name() {
        let skin = Skin {
            open: true,
            query: "light".to_owned(),
            ..Skin::default()
        };
        let hits = skin.matches();
        assert_eq!(hits, vec![Pick::Light]);
    }

    #[test]
    fn the_picker_contains_all_authored_themes() {
        let all = every();
        assert_eq!(
            all,
            vec![Pick::Dark, Pick::Light, Pick::Industrial, Pick::Cyberpunk]
        );
    }

    #[test]
    fn the_house_picker_offers_only_the_two_ground_polarities() {
        let skin = Skin::house();
        assert_eq!(skin.matches(), vec![Pick::Dark, Pick::Light]);
        assert!(!skin.allows(Pick::Industrial));
        assert!(!skin.allows(Pick::Cyberpunk));
    }

    #[test]
    fn old_theme_names_migrate_to_their_broad_side() {
        assert_eq!(parse_choice("Cyberpunk"), Some(Pick::Cyberpunk));
        assert_eq!(parse_choice("Industrial"), Some(Pick::Industrial));
        assert_eq!(parse_choice("Solarized Light"), Some(Pick::Light));
        assert_eq!(parse_choice("Gruvbox Dark"), Some(Pick::Dark));
        assert_eq!(parse_choice("Nord"), Some(Pick::Dark));
        assert_eq!(parse_choice(""), None);
    }
}
