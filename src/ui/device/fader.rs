//! Faders: the vertical channel fader and its horizontal sibling.
//!
//! Rail semantics: click or drag anywhere on the rail and the handle goes
//! to the pointer — a fader is an absolute control, unlike the knob's
//! relative drag. Double-click resets to the param default.

use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, radius, stroke};
use eframe::egui;

/// Vertical fader. Up is 1.0. Returns true when the user changed `norm`.
pub fn fader(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(theme.sp(control::FADER_W), theme.sp(control::FADER_LEN));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let changed = edit(ui, &response, rect, param, norm, /* vertical */ true);

    paint(ui, theme, rect, param, *norm, &response, true);
    if response.hovered() || response.dragged() {
        response.clone().on_hover_text(param.format(*norm));
    }
    changed
}

/// Horizontal slider. Right is 1.0. Returns true when the user changed
/// `norm`.
pub fn slider(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(theme.sp(control::FADER_LEN), theme.sp(control::FADER_W));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let changed = edit(ui, &response, rect, param, norm, /* vertical */ false);

    paint(ui, theme, rect, param, *norm, &response, false);
    if response.hovered() || response.dragged() {
        response.clone().on_hover_text(param.format(*norm));
    }
    changed
}

fn edit(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    param: &Param,
    norm: &mut f32,
    vertical: bool,
) -> bool {
    let mut changed = false;
    if response.double_clicked() {
        if *norm != param.default_norm {
            *norm = param.default_norm;
            changed = true;
        }
    } else if response.dragged() && ui.input(|i| i.modifiers.shift) {
        // Shift-drag: RELATIVE fine adjust instead of rail-absolute — the
        // way you land a level within a tenth of a dB without overshooting.
        let d = response.drag_delta();
        let delta = if vertical { -d.y } else { d.x }
            / (theme_len(rect, vertical) * (1.0 / crate::ui::device::adjust::FINE));
        let next = (*norm + delta).clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    } else if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let next = if vertical {
            1.0 - (pos.y - rect.top()) / rect.height()
        } else {
            (pos.x - rect.left()) / rect.width()
        }
        .clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    }
    // Wheel while hovered, arrows once clicked; Shift is fine.
    let nudge = crate::ui::device::adjust::nudge(ui, response);
    if nudge != 0.0 {
        let next = (*norm + nudge).clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    }
    changed
}

/// The rail length in the drag axis.
fn theme_len(rect: egui::Rect, vertical: bool) -> f32 {
    if vertical {
        rect.height()
    } else {
        rect.width()
    }
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    param: &Param,
    norm: f32,
    response: &egui::Response,
    vertical: bool,
) {
    let painter = ui.painter();
    let norm = norm.clamp(0.0, 1.0);

    // Groove.
    painter.rect_filled(rect, radius::CTRL, theme.surface_sunken);
    painter.rect_stroke(
        rect,
        radius::CTRL,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    // Fill: from the minimum end, or from center for bipolar params.
    let along = |t: f32| -> f32 {
        if vertical {
            rect.bottom() - rect.height() * t
        } else {
            rect.left() + rect.width() * t
        }
    };
    let (a, b) = if param.bipolar {
        (along(0.5), along(norm))
    } else {
        (along(0.0), along(norm))
    };
    let (lo, hi) = (a.min(b), a.max(b));
    if hi > lo {
        let fill = if vertical {
            egui::Rect::from_min_max(egui::pos2(rect.left(), lo), egui::pos2(rect.right(), hi))
        } else {
            egui::Rect::from_min_max(egui::pos2(lo, rect.top()), egui::pos2(hi, rect.bottom()))
        };
        painter.rect_filled(fill, radius::CTRL, theme.accent_muted);
    }

    // Handle: a bold line across the rail at the value.
    let at = along(norm);
    let handle = egui::Stroke::new(stroke::FOCUS, theme.text);
    if vertical {
        painter.line_segment(
            [egui::pos2(rect.left(), at), egui::pos2(rect.right(), at)],
            handle,
        );
    } else {
        painter.line_segment(
            [egui::pos2(at, rect.top()), egui::pos2(at, rect.bottom())],
            handle,
        );
    }

    // Center detent tick for bipolar rails.
    if param.bipolar {
        let mid = along(0.5);
        let tick = egui::Stroke::new(stroke::HAIR, theme.text_muted);
        if vertical {
            painter.line_segment(
                [
                    egui::pos2(rect.left(), mid),
                    egui::pos2(rect.left() + rect.width() * 0.25, mid),
                ],
                tick,
            );
        } else {
            painter.line_segment(
                [
                    egui::pos2(mid, rect.top()),
                    egui::pos2(mid, rect.top() + rect.height() * 0.25),
                ],
                tick,
            );
        }
    }

    if response.has_focus() || response.dragged() {
        painter.rect_stroke(
            rect.expand(stroke::FOCUS),
            radius::CTRL,
            egui::Stroke::new(stroke::FOCUS, theme.focus),
            egui::StrokeKind::Outside,
        );
    }
}
