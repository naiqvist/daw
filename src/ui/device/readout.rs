//! Typed value text. The display half of the wiring layer: a `Param`
//! formats its natural value (Hz, dB, ms...), these render it — static,
//! or drag-to-edit for panels that want a number instead of a control.

use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::control;
use eframe::egui;

/// Fine-drag multiplier while shift is held.
const FINE: f32 = 0.1;

/// Static formatted value: monospace, value color.
pub fn readout(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: f32) {
    ui.label(
        egui::RichText::new(param.format(norm))
            .monospace()
            .color(theme.text_value),
    );
}

/// Drag-editable formatted value. Horizontal drag sweeps the range over
/// [`control::DRAG_TRAVEL`] pixels, shift is fine, double-click resets to
/// the param default. Returns true when `norm` changed.
pub fn readout_drag(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let text = egui::RichText::new(param.format(*norm))
        .monospace()
        .color(theme.text_value);
    let response = ui
        .add(egui::Label::new(text).sense(egui::Sense::click_and_drag()))
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

    let mut changed = false;
    if response.double_clicked() {
        if *norm != param.default_norm {
            *norm = param.default_norm;
            changed = true;
        }
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = control::DRAG_TRAVEL / if fine { FINE } else { 1.0 };
        let delta = response.drag_delta().x / travel;
        if delta != 0.0 {
            let next = (*norm + delta).clamp(0.0, 1.0);
            if next != *norm {
                *norm = next;
                changed = true;
            }
        }
    }
    // Wheel while hovered, arrows once clicked; Shift is fine.
    let nudge = crate::ui::device::adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let next = (*norm + nudge).clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    }
    if response.hovered() || response.dragged() {
        response.on_hover_text(param.name);
    }
    changed
}
