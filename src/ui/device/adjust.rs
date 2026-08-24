//! Shared fine-control manners for device widgets: scroll-wheel adjust
//! while hovered, arrow-key adjust once clicked (egui focus), Shift as the
//! universal fine modifier. One module so every control behaves the same —
//! learning one knob teaches all of them.

use eframe::egui;

/// Normalized step for one arrow press or wheel notch (a full sweep is
/// 100 presses; with Shift, 1000).
pub const STEP: f32 = 0.01;
/// Shift multiplier: everything moves at a tenth.
pub const FINE: f32 = 0.1;
/// Wheel pixels for a full sweep (typical notch ≈ 50px → ~STEP).
const WHEEL_TRAVEL: f32 = 500.0;

/// Wheel + keyboard delta for a 1-D control. Call after the widget's own
/// drag handling; add the result to the normalized value (clamp yourself).
/// Clicking focuses the widget so arrows work; Up/Right raise, Down/Left
/// lower. Consumed keys don't leak to other widgets or panels.
pub fn nudge(ui: &egui::Ui, response: &egui::Response) -> f32 {
    if response.clicked() {
        response.request_focus();
    }
    let fine = if ui.input(|i| i.modifiers.shift) {
        FINE
    } else {
        1.0
    };

    let mut delta = 0.0f32;
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            delta += scroll / WHEEL_TRAVEL * fine;
            // Consume it: a wheel spent on this control must not ALSO
            // scroll whatever container the control sits in.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }
    if response.has_focus() {
        for (keys, dir) in [
            ([egui::Key::ArrowUp, egui::Key::ArrowRight], 1.0f32),
            ([egui::Key::ArrowDown, egui::Key::ArrowLeft], -1.0),
        ] {
            for key in keys {
                for m in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
                    if ui.ctx().input_mut(|i| i.consume_key(m, key)) {
                        delta += dir * STEP * if m.shift { FINE } else { 1.0 };
                    }
                }
            }
        }
    }
    delta
}

/// Two-axis variant for the XY pad: Left/Right drive x, Up/Down drive y.
/// Wheel drives y (the vertical intuition), Shift+wheel is fine.
pub fn nudge_xy(ui: &egui::Ui, response: &egui::Response) -> (f32, f32) {
    if response.clicked() {
        response.request_focus();
    }
    let fine = if ui.input(|i| i.modifiers.shift) {
        FINE
    } else {
        1.0
    };

    let mut dx = 0.0f32;
    let mut dy = 0.0f32;
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            dy += scroll / WHEEL_TRAVEL * fine;
            // Consume it — same rule as `nudge`.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }
    if response.has_focus() {
        for (key, axis, dir) in [
            (egui::Key::ArrowRight, 0u8, 1.0f32),
            (egui::Key::ArrowLeft, 0, -1.0),
            (egui::Key::ArrowUp, 1, 1.0),
            (egui::Key::ArrowDown, 1, -1.0),
        ] {
            for m in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
                if ui.ctx().input_mut(|i| i.consume_key(m, key)) {
                    let step = dir * STEP * if m.shift { FINE } else { 1.0 };
                    if axis == 0 {
                        dx += step;
                    } else {
                        dy += step;
                    }
                }
            }
        }
    }
    (dx, dy)
}
