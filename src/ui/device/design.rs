//! The device design system: the ONLY place a device-layer frame or
//! margin is chosen. Mechanically enforced — `ui::tests` fails the build
//! if any other device file writes `Frame::new`, `inner_margin`, or
//! `Margin::` — so a card CANNOT ship with ad-hoc spacing: every surface
//! it is built from arrives here, pre-margined, on the 4px grid.
//!
//! The spacing rhythm, stated once:
//!
//! - **Card**: no inner margin of its own — its title strip and body carry
//!   the padding, so the outline hugs the silhouette.
//! - **Title strip**: SM horizontal / XS vertical — wide enough to breathe,
//!   shallow enough to read as a label, not a toolbar.
//! - **Body**: SM all around — content never touches the card edge or the
//!   title rule.
//! - **Well**: XS all around — tight, because a well already sits inside
//!   the body's SM and nested padding compounds.
//!
//! Every value is a density-scaled token; nothing here invents a number.

use crate::ui::theme::Theme;
use crate::ui::tokens::{radius, space, stroke};
use eframe::egui;

/// The card silhouette: raised surface, hairline outline, panel radius.
/// Marginless by design — compose [`title_strip`] and [`body`] inside it.
pub fn card_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface_raised)
        .stroke(egui::Stroke::new(stroke::HAIR, theme.outline))
        .corner_radius(radius::PANEL as u8)
}

/// The title strip's padding: SM across, XS down.
pub fn title_strip(theme: &Theme) -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(
        theme.sp(space::SM) as i8,
        theme.sp(space::XS) as i8,
    ))
}

/// The card body's padding: SM all around.
pub fn body(theme: &Theme) -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
}

/// A section well: sunken, hairline-divided, control radius, XS padding.
pub fn well(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface_sunken)
        .stroke(egui::Stroke::new(stroke::HAIR, theme.divider))
        .corner_radius(radius::CTRL as u8)
        .inner_margin(egui::Margin::same(theme.sp(space::XS) as i8))
}

/// The well's inner padding, for layout math that must subtract it
/// (sections' row-height split). Same token the [`well`] frame bakes in.
pub fn well_pad(theme: &Theme) -> f32 {
    theme.sp(space::XS)
}
