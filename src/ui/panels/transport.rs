//! Transport bar: engine power, play/return, metronome, tempo, position.
//! A pure function of the view state — it returns wishes, it acts on nothing.

use crate::ui::action::UiAction;
use crate::ui::host::{Dock, Panel, PanelCx, Sizing};
use crate::ui::kit;
use eframe::egui;

#[derive(Default)]
pub struct TransportBar;

impl Panel for TransportBar {
    fn id(&self) -> &'static str {
        "transport"
    }

    fn title(&self) -> &'static str {
        "Transport"
    }

    fn dock(&self) -> Dock {
        Dock::Top
    }

    fn sizing(&self) -> Sizing {
        Sizing::Bar
    }

    /// The transport is not optional furniture.
    fn closable(&self) -> bool {
        false
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &mut PanelCx<'_>) {
        ui.horizontal_centered(|ui| {
            kit::title(ui, cx.theme, "daw");
            kit::divider(ui, cx.theme);

            let running = cx.vs.engine_running;
            let power = if running {
                UiAction::StopEngine
            } else {
                UiAction::StartEngine
            };
            cx.action_toggle(ui, running, "engine", power);
            kit::divider(ui, cx.theme);

            let play_label = if cx.vs.playing { "⏸" } else { "▶" };
            cx.action_button(ui, play_label, UiAction::TogglePlay);
            cx.action_button(ui, "⏮", UiAction::Return);
            cx.action_toggle(ui, cx.vs.metronome_on, "click", UiAction::ToggleMetronome);

            let mut bpm = cx.vs.bpm;
            ui.add_enabled_ui(running, |ui| {
                if kit::bpm_drag(ui, &mut bpm) {
                    cx.act(UiAction::SetTempo(bpm));
                }
            });

            kit::divider(ui, cx.theme);
            kit::value(ui, cx.theme, &format!("beat {:8.2}", cx.vs.beat)); // magic: format width, not layout
            kit::value(ui, cx.theme, &format!("{:7.2}s", cx.vs.position_secs)); // magic: format width, not layout
        });
    }
}
