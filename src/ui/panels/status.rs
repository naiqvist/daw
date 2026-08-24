//! Status bar: engine load, xruns, notices, frame timing, adapter.

use crate::ui::host::{Dock, Panel, PanelCx, Sizing};
use crate::ui::kit;
use eframe::egui;

#[derive(Default)]
pub struct StatusBar;

impl Panel for StatusBar {
    fn id(&self) -> &'static str {
        "status"
    }

    fn title(&self) -> &'static str {
        "Status"
    }

    fn dock(&self) -> Dock {
        Dock::Bottom
    }

    fn sizing(&self) -> Sizing {
        Sizing::Bar
    }

    /// Load and xruns are not optional furniture either.
    fn closable(&self) -> bool {
        false
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &mut PanelCx<'_>) {
        let vs = cx.vs;
        ui.horizontal_centered(|ui| {
            if vs.engine_running {
                kit::value(
                    ui,
                    cx.theme,
                    &format!(
                        "dsp {:5.2}% (worst {:5.2}%)",
                        vs.dsp_load_pct, vs.dsp_worst_pct
                    ), // magic: format width, not layout
                );
                kit::value_state(ui, cx.theme, &format!("xruns {}", vs.xruns), vs.xruns == 0);
            } else {
                kit::muted(ui, cx.theme, "engine off");
            }
            kit::divider(ui, cx.theme);
            if let Some(notice) = &vs.notice {
                kit::notice(ui, cx.theme, notice);
                kit::divider(ui, cx.theme);
            }
            kit::value(ui, cx.theme, &format!("{:5.2} ms/frame", vs.frame_ms)); // magic: format width, not layout
            kit::divider(ui, cx.theme);
            kit::muted(ui, cx.theme, &vs.adapter);
        });
    }
}
