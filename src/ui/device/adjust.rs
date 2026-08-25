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

/// Wheel pixels for one DISCRETE step. A mouse notch is ~50 raw pixels
/// on the platforms this runs on, so one notch is one choice — which is
/// the thing everybody expects a wheel over a selector to do.
const NOTCH_PX: f32 = 40.0;

/// Wheel + keyboard steps for a DISCRETE control: how many choices to
/// move, this frame. Usually 0.
///
/// Separate from [`nudge`] because a discrete control cannot use a
/// continuous delta: a hundredth of a choice is either nothing or a whole
/// one, and rounding it lands on "sometimes two settings at once". This
/// counts NOTCHES instead.
///
/// Shift is deliberately ignored: there is no finer than one choice.
pub fn steps(ui: &egui::Ui, response: &egui::Response) -> i32 {
    if response.clicked() {
        response.request_focus();
    }
    let mut steps = 0i32;

    if response.hovered() {
        // Read the WHEEL EVENTS, not `smooth_scroll_delta`. The smoothed
        // delta spreads one notch over several frames, and stepping on
        // each of them turns one flick into four changed settings.
        //
        // A line-unit wheel (a real mouse) reports one line per notch, so
        // one notch is one choice with no accumulation at all. A trackpad
        // reports points, which accumulate until they add up to a notch —
        // otherwise a gentle two-finger drift would rattle through every
        // setting on the way past.
        let (lines, points) = ui.input(|i| {
            let mut lines = 0.0f32;
            let mut points = 0.0f32;
            for event in &i.raw.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = event {
                    match unit {
                        egui::MouseWheelUnit::Line => lines += delta.y,
                        egui::MouseWheelUnit::Page => lines += delta.y,
                        egui::MouseWheelUnit::Point => points += delta.y,
                    }
                }
            }
            (lines, points)
        });

        let mut whole = lines.trunc();
        if points != 0.0 {
            let id = response.id.with("wheel_acc");
            let acc: f32 = ui.data(|d| d.get_temp(id).unwrap_or(0.0)) + points;
            let notches = (acc / NOTCH_PX).trunc();
            whole += notches;
            ui.data_mut(|d| d.insert_temp(id, acc - notches * NOTCH_PX));
        }
        if whole != 0.0 {
            steps += whole as i32;
        }
        if lines != 0.0 || points != 0.0 {
            // Consume it: a wheel spent here must not ALSO scroll the
            // container this control sits in.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }

    if response.has_focus() {
        for (keys, dir) in [
            ([egui::Key::ArrowUp, egui::Key::ArrowRight], 1i32),
            ([egui::Key::ArrowDown, egui::Key::ArrowLeft], -1),
        ] {
            for key in keys {
                if ui
                    .ctx()
                    .input_mut(|i| i.consume_key(egui::Modifiers::NONE, key))
                {
                    steps += dir;
                }
            }
        }
    }
    steps
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
