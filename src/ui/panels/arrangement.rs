//! The arrangement view — the center dock's default occupant.
//!
//! Deliberately empty. The timeline is bespoke wgpu work (SDF widgets, lyon
//! paths), not egui widgets, and it lands as its own change. What this panel
//! proves today is the wiring: a center-docked panel that the host tabs,
//! preferences remember, and the View menu can hide.

use crate::ui::host::{Dock, Panel, PanelCx, Sizing};
use crate::ui::kit;
use eframe::egui;

#[derive(Default)]
pub struct Arrangement;

impl Panel for Arrangement {
    fn id(&self) -> &'static str {
        "arrangement"
    }

    fn title(&self) -> &'static str {
        "Arrangement"
    }

    fn dock(&self) -> Dock {
        Dock::Center
    }

    fn sizing(&self) -> Sizing {
        Sizing::Fill
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &mut PanelCx<'_>) {
        kit::empty_state(ui, cx.theme, "arrangement — timeline lands here");
    }
}
