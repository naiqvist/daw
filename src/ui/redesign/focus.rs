//! The focus sign for the keyboard-first redesign.
//!
//! Focus is one bit of state, and it is drawn twice over: the focused panel
//! carries a solid bar down its left edge, and every unfocused panel is
//! dimmed under a translucent scrim. The scrim makes the focused panel the
//! only place with full-value text, so the eye finds it peripherally; the
//! bar is the crisp local anchor once it lands.
//!
//! Because the scrim covers content, `show` must be the LAST thing a panel
//! paints.

use crate::ui::redesign::OUTLINE;
use eframe::egui;

/// Width of the bar down the focused panel's left edge.
const BAR_W: f32 = 3.0;

/// The veil over unfocused panels. Alpha is chosen so white content reads
/// near gray(165) — clearly legible, clearly not where the keyboard is.
const SCRIM: egui::Color32 = egui::Color32::from_black_alpha(90);

pub(crate) fn show(painter: &egui::Painter, rect: egui::Rect, focused: bool) {
    if focused {
        let bar = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + BAR_W, rect.bottom()),
        );
        painter.rect_filled(bar, 0.0, OUTLINE);
    } else {
        painter.rect_filled(rect, 0.0, SCRIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What white text becomes under the scrim.
    fn dimmed_white() -> f32 {
        255.0 * (1.0 - f32::from(SCRIM.a()) / 255.0)
    }

    /// The scrim spends redundancy on the one signal worth it: it must
    /// visibly separate the unfocused panels without making them illegible.
    /// Too weak and focus is lost again; too strong and the frame stops
    /// being one surface.
    #[test]
    fn unfocused_panels_dim_but_stay_readable() {
        let dimmed = dimmed_white();
        assert!(dimmed <= 200.0, "the scrim is too weak to separate panels");
        assert!(dimmed >= 130.0, "the scrim buries the unfocused panels");
    }

    /// The bar is a mark seen from across the room, not a hairline.
    #[test]
    fn the_focus_bar_is_visible_at_a_glance() {
        assert!(BAR_W >= 2.0);
    }

    /// Focus is the loudest sign in the system: pure white, fully opaque.
    #[test]
    fn nothing_outshines_focus() {
        assert_eq!(OUTLINE, egui::Color32::WHITE);
    }
}
