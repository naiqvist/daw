//! Param-aware rotary knob: label above, value readout below, bipolar
//! fill, shift for fine drag, double-click to reset. `kit::knob` stays the
//! bare primitive; this is the one a device panel reaches for.

use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Sweep from 7 o'clock to 5 o'clock — the 270-degree gap-at-the-bottom
/// convention every hardware knob uses.
const START: f32 = std::f32::consts::PI * 0.75;
const SWEEP: f32 = std::f32::consts::PI * 1.5;
/// Full travel over roughly four knob-heights of drag; shift divides by 10.
const TRAVEL_KNOBS: f32 = 4.0;
const FINE: f32 = 0.1;

/// Draw the knob with its name and formatted value. Returns true when the
/// user changed `norm` this frame.
pub fn knob(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let d = theme.sp(control::KNOB);
    let mut changed = false;

    // Claim EXACTLY the stack's width with a center-aligned column. A
    // plain `vertical()` here would take the full available width and
    // then shrink, which leaves the stack pinned left inside any
    // centering parent (a section well, say) — invisible until you look.
    let w = d.max(ui.spacing().interact_size.x);
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.set_width(w);
            {
                ui.label(
                    egui::RichText::new(param.name)
                        .size(font::LABEL)
                        .color(theme.text_muted),
                );

                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click_and_drag());

                if response.double_clicked() {
                    if *norm != param.default_norm {
                        *norm = param.default_norm;
                        changed = true;
                    }
                } else if response.dragged() {
                    let fine = ui.input(|i| i.modifiers.shift);
                    let travel = d * TRAVEL_KNOBS / if fine { FINE } else { 1.0 };
                    let delta = -response.drag_delta().y / travel;
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

                paint(ui, theme, rect, param, *norm, &response);

                ui.label(
                    egui::RichText::new(param.format(*norm))
                        .monospace()
                        .size(font::LABEL)
                        .color(theme.text_value),
                );
            }
        },
    );

    changed
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    param: &Param,
    norm: f32,
    response: &egui::Response,
) {
    let center = rect.center();
    let radius = rect.width() * 0.5 - stroke::BOLD;
    let painter = ui.painter();

    painter.circle_filled(center, radius, theme.surface_sunken);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    let arc = |from: f32, to: f32, s: egui::Stroke| {
        const STEPS: usize = 24;
        let points: Vec<egui::Pos2> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                // Screen y grows downward; the angle sweeps clockwise from
                // the 9-o'clock direction through the bottom.
                center + egui::vec2(-a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };

    // The groove.
    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    // The fill: from the minimum, or from the center for bipolar params.
    let at = START + SWEEP * norm.clamp(0.0, 1.0);
    let fill = egui::Stroke::new(stroke::BOLD, theme.accent);
    if param.bipolar {
        let mid = START + SWEEP * 0.5;
        if at != mid {
            arc(mid, at, fill);
        }
    } else if norm > 0.0 {
        arc(START, at, fill);
    }

    // The pointer.
    let dir = egui::vec2(-at.cos(), at.sin());
    painter.line_segment(
        [center + dir * (radius * 0.4), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );

    // Center detent tick for bipolar knobs, at 12 o'clock.
    if param.bipolar {
        let up = egui::vec2(0.0, -1.0);
        painter.line_segment(
            [
                center + up * (radius + stroke::FOCUS),
                center + up * (radius + theme.sp(space::XS)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.text_muted),
        );
    }

    if response.has_focus() || response.dragged() {
        painter.circle_stroke(
            center,
            radius + stroke::FOCUS,
            egui::Stroke::new(stroke::FOCUS, theme.focus),
        );
    }
}
