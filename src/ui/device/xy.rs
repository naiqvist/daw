//! XY pad: two params, one gesture. Filter cutoff/resonance, delay
//! time/feedback — anywhere two parameters are really one movement.

use crate::ui::device::design;
use crate::ui::device::metrics::Footprint;
use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, stroke};
use eframe::egui;

/// The XY pad's size contract: a square of [`control::XY_PAD`]. No text
/// of its own — the two params are named by whatever labels the caller
/// puts around it.
pub fn footprint(theme: &Theme) -> Footprint {
    let side = theme.sp(control::XY_PAD);
    Footprint::new(side, side)
}

/// Square pad editing two normalized values. `x` runs left→right, `y` runs
/// bottom→top (up is more, like the fader). Double-click resets both to
/// their param defaults. Returns true when either value changed.
pub fn xy_pad(
    ui: &mut egui::Ui,
    theme: &Theme,
    x_param: &Param,
    y_param: &Param,
    x: &mut f32,
    y: &mut f32,
) -> bool {
    let side = footprint(theme).width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());

    let mut changed = false;
    if response.double_clicked() {
        if *x != x_param.default_norm || *y != y_param.default_norm {
            *x = x_param.default_norm;
            *y = y_param.default_norm;
            changed = true;
        }
    } else if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let ny = (1.0 - (pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        if nx != *x || ny != *y {
            *x = nx;
            *y = ny;
            changed = true;
        }
    }
    // Arrows steer the puck once clicked (Left/Right = x, Up/Down = y),
    // wheel drives y; Shift is fine. Same manners as every other control.
    let (dx, dy) = crate::ui::device::adjust::nudge_xy(ui, &response);
    if dx != 0.0 || dy != 0.0 {
        let nx = (*x + dx).clamp(0.0, 1.0);
        let ny = (*y + dy).clamp(0.0, 1.0);
        if nx != *x || ny != *y {
            *x = nx;
            *y = ny;
            changed = true;
        }
    }

    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);

    // Quarter grid, center lines slightly stronger.
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let color = if i == 2 {
            theme.grid_bar
        } else {
            theme.grid_beat
        };
        let s = egui::Stroke::new(stroke::HAIR, color);
        let gx = rect.left() + rect.width() * t;
        let gy = rect.top() + rect.height() * t;
        painter.line_segment(
            [egui::pos2(gx, rect.top()), egui::pos2(gx, rect.bottom())],
            s,
        );
        painter.line_segment(
            [egui::pos2(rect.left(), gy), egui::pos2(rect.right(), gy)],
            s,
        );
    }
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    // Crosshair through the puck, then the puck.
    let px = rect.left() + rect.width() * x.clamp(0.0, 1.0);
    let py = rect.top() + rect.height() * (1.0 - y.clamp(0.0, 1.0));
    let hair = egui::Stroke::new(stroke::HAIR, theme.accent_muted);
    painter.line_segment(
        [egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())],
        hair,
    );
    painter.line_segment(
        [egui::pos2(rect.left(), py), egui::pos2(rect.right(), py)],
        hair,
    );
    let r = theme.sp(control::HANDLE);
    painter.circle_filled(egui::pos2(px, py), r, theme.accent);
    painter.circle_stroke(
        egui::pos2(px, py),
        r,
        egui::Stroke::new(stroke::HAIR, theme.surface_sunken),
    );

    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }

    if response.hovered() || response.dragged() {
        response.on_hover_text(format!(
            "{}: {}\n{}: {}",
            x_param.name,
            x_param.format(*x),
            y_param.name,
            y_param.format(*y)
        ));
    }

    changed
}
