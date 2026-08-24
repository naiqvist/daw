//! Runtime theme: semantic color roles and density. Panels name ROLES
//! (`text_muted`, `red_zone`), never values — the theme maps role to color,
//! so light mode and user themes become data changes, not code changes.

use crate::ui::tokens::{Density, font, radius, space};
use eframe::egui::{self, Color32};

#[derive(Debug, Clone)]
pub struct Theme {
    /// Whether the ground is light. Drives egui's own light/dark switch so
    /// stock widgets (text fields, scrollbars) follow the scheme instead of
    /// staying pinned to the dark defaults.
    pub light: bool,
    // grounds
    pub bg: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_sunken: Color32,
    // content
    pub text: Color32,
    pub text_muted: Color32,
    /// Numeric readouts — pair with the monospace font.
    pub text_value: Color32,
    // lines
    pub outline: Color32,
    pub divider: Color32,
    pub focus: Color32,
    // identity
    pub accent: Color32,
    pub accent_muted: Color32,
    // state
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    // engine vocabulary (the zones, promoted from lab hardcodes)
    pub red_zone: Color32,
    pub green_zone: Color32,
    // timeline
    pub playhead: Color32,
    pub loop_region: Color32,
    /// The loop brace itself: the region's own colour, at full strength.
    pub loop_brace: Color32,
    /// The time-selection wash, translucent so the grid reads through it.
    pub selection: Color32,
    pub grid_beat: Color32,
    pub grid_bar: Color32,
    /// Grid subdivisions, dimmer than a beat line.
    pub grid_sub: Color32,
    pub clip_body: Color32,
    pub clip_selected: Color32,
    /// Note bars inside clips — kept light enough to read on `clip_body`.
    pub clip_note: Color32,
    // audio
    pub meter_low: Color32,
    pub meter_hot: Color32,
    pub meter_clip: Color32,
    /// Multiplies spacing tokens: 1.0 comfortable, 0.85 compact.
    pub density: f32,
}

/// How close the pointer must get to a panel edge to drag it. Not a visual
/// value — nothing is drawn at this size — so it is not a spacing token.
const RESIZE_GRAB_PX: f32 = 8.0;

impl Theme {
    pub fn dark() -> Self {
        Self {
            // The ground ramp: near-black throughout, warmed by the
            // slightest brown tint. Every step keeps R > G > B by a hair —
            // 2 counts at the darkest, 6 at the lightest — which reads as
            // warmth without ever reading as a color.
            light: false,
            bg: Color32::from_rgb(0x0e, 0x0d, 0x0b),
            surface: Color32::from_rgb(0x14, 0x12, 0x10),
            surface_raised: Color32::from_rgb(0x1c, 0x19, 0x16),
            surface_sunken: Color32::from_rgb(0x07, 0x06, 0x05),
            text: Color32::from_rgb(0xd6, 0xdd, 0xe4),
            text_muted: Color32::from_rgb(0x84, 0x94, 0xa2),
            text_value: Color32::from_rgb(0xb8, 0xcc, 0xd9),
            // Lines carry the same tint, or they read blue against it.
            outline: Color32::from_rgb(0x2b, 0x27, 0x21),
            divider: Color32::from_rgb(0x21, 0x1e, 0x1a),
            focus: Color32::from_rgb(0x4f, 0xa8, 0xc7),
            accent: Color32::from_rgb(0x4f, 0xa8, 0xc7),
            accent_muted: Color32::from_rgb(0x33, 0x5c, 0x6d),
            ok: Color32::from_rgb(0x5f, 0xb3, 0x5f),
            warn: Color32::from_rgb(0xd0, 0xa0, 0x5f),
            danger: Color32::from_rgb(0xd0, 0x5f, 0x5f),
            red_zone: Color32::from_rgb(0xef, 0x6f, 0x5f),
            green_zone: Color32::from_rgb(0x55, 0xb9, 0x8a),
            playhead: Color32::from_rgb(0xe5, 0xc1, 0x5c),
            loop_region: Color32::from_rgba_unmultiplied(0x4f, 0xa8, 0xc7, 0x22),
            // The brace and its wash: the grid's own hue, brightened —
            // same hue (35deg) and saturation as `grid_bar`, three times the
            // lightness — so the loop reads as part of the grid rather than
            // as something imported from the panel seams.
            loop_brace: Color32::from_rgb(0x95, 0x87, 0x73),
            selection: Color32::from_rgba_premultiplied(0x29, 0x23, 0x1c, 0x38),
            grid_beat: Color32::from_rgb(0x22, 0x1f, 0x1a),
            grid_bar: Color32::from_rgb(0x32, 0x2d, 0x26),
            grid_sub: Color32::from_rgb(0x19, 0x17, 0x13),
            // Clips live in the warm ground family like everything else: a
            // block one notch above `surface_raised`, a selection border in
            // the grid's own tan (the loop brace's hue, brightened), and
            // notes in the cream the bar uses for text.
            clip_body: Color32::from_rgb(0x2a, 0x26, 0x20),
            clip_selected: Color32::from_rgb(0xbb, 0xa1, 0x81),
            clip_note: Color32::from_rgb(0xcf, 0xc6, 0xba),
            meter_low: Color32::from_rgb(0x55, 0xb9, 0x8a),
            meter_hot: Color32::from_rgb(0xd0, 0xa0, 0x5f),
            meter_clip: Color32::from_rgb(0xd0, 0x5f, 0x5f),
            density: 1.0,
        }
    }

    /// The same theme at a different packing. Density is a machine-local
    /// preference, so it arrives from `ui::prefs`, not from a project.
    pub fn with_density(mut self, density: Density) -> Self {
        self.density = density.scale();
        self
    }

    pub fn set_density(&mut self, density: Density) {
        self.density = density.scale();
    }

    /// Density-scaled spacing: `theme.sp(space::MD)`.
    pub fn sp(&self, token: f32) -> f32 {
        token * self.density
    }

    /// Project the theme into egui so BUILT-IN widgets comply too. Without
    /// this, every stock widget is a leak in the token system.
    pub fn apply(&self, ctx: &egui::Context) {
        // egui 0.36 keeps one Style per light/dark theme; we are the theme
        // system, so pin egui to whichever side the ground is on and shape
        // that one style.
        ctx.set_theme(if self.light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        ctx.all_styles_mut(|style| self.shape_style(style));
    }

    fn shape_style(&self, style: &mut egui::Style) {
        // egui's default side-resize hit zone is 3px, which makes a panel
        // edge something you aim at rather than something you grab. Widen it:
        // the handle is invisible either way, so its size is pure ergonomics.
        style.interaction.resize_grab_radius_side = RESIZE_GRAB_PX;

        style.spacing.item_spacing = egui::vec2(self.sp(space::SM), self.sp(space::XS));
        style.spacing.button_padding = egui::vec2(self.sp(space::SM), self.sp(space::XS));
        style.spacing.menu_margin = egui::Margin::same(self.sp(space::SM) as i8);
        style.spacing.window_margin = egui::Margin::same(self.sp(space::MD) as i8);

        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(font::LABEL, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(font::BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(font::BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(font::TITLE, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(font::VALUE, FontFamily::Monospace),
            ),
        ]
        .into();

        let v = &mut style.visuals;
        v.dark_mode = !self.light;
        v.panel_fill = self.surface;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.surface_sunken;
        v.faint_bg_color = self.surface_raised;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.accent_muted;
        v.selection.stroke = egui::Stroke::new(crate::ui::tokens::stroke::HAIR, self.accent);
        v.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(crate::ui::tokens::stroke::HAIR, self.divider);
        v.widgets.inactive.bg_fill = self.surface_raised;
        v.widgets.hovered.bg_fill = self.surface_raised;
        v.widgets.active.bg_fill = self.accent_muted;
        v.widgets.inactive.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.widgets.hovered.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.widgets.active.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.window_corner_radius = egui::CornerRadius::same(radius::PANEL as u8);
    }
}
