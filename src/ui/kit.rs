//! The widget kit: the ONLY code allowed to hand raw numbers to egui, and the
//! only place `ui.painter()` appears.
//!
//! Panels compose kit calls and tokens; the enforcement tests keep them
//! honest. Every helper takes the theme, so a panel cannot bypass it.
//!
//! Kit helpers are deliberately *small and boring*. The rule of thumb for
//! adding one: if two panels would otherwise write the same three lines of
//! egui, it belongs here. If it needs a color, a size, or a painter, it
//! belongs here whether or not it repeats.

use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, pane, radius, space, stroke};
use eframe::egui::{self, Color32};

// ---------------------------------------------------------------- frames ---

/// Frame for the transport/status bars.
pub fn bar_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .inner_margin(egui::Margin::symmetric(
            theme.sp(space::MD) as i8,
            theme.sp(space::SM) as i8,
        ))
}

/// Frame for a docked side pane.
pub fn pane_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .inner_margin(egui::Margin::same(theme.sp(space::MD) as i8))
}

/// Frame for the main working area.
pub fn central_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new().fill(theme.bg)
}

/// Fixed bar height, density-scaled.
pub fn bar_height(theme: &Theme) -> f32 {
    theme.sp(control::BAR_H)
}

// ------------------------------------------------------------------ text ---

pub fn title(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).heading().color(theme.text));
}

pub fn label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).color(theme.text));
}

pub fn muted(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme.text_muted));
}

/// Numeric readout: monospace, value color.
pub fn value(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .color(theme.text_value),
    );
}

/// A readout that changes color on a condition — xruns, clip counts, anything
/// where "zero" and "not zero" are different news.
pub fn value_state(ui: &mut egui::Ui, theme: &Theme, text: &str, ok: bool) {
    let color = if ok { theme.ok } else { theme.danger };
    ui.label(egui::RichText::new(text).monospace().color(color));
}

/// A line the user must not miss.
pub fn notice(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).color(theme.danger));
}

/// What a panel shows when it has nothing to show. Centered, muted, never
/// blank — an empty panel that looks broken is a bug report.
pub fn empty_state(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme.sp(space::XXL));
        muted(ui, theme, text);
    });
}

// ------------------------------------------------------------- structure ---

/// A titled group box. The standard way to carve a pane into parts.
pub fn section<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    heading: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(theme.surface_raised)
        .corner_radius(radius::PANEL as u8)
        .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
        .show(ui, |ui| {
            muted(ui, theme, heading);
            ui.add_space(theme.sp(space::XS));
            add(ui)
        })
        .inner
}

/// Label on the left, control on the right. The unit a parameter list is
/// made of.
pub fn row<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        muted(ui, theme, name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add)
            .inner
    })
    .inner
}

/// Vertical hairline divider, for bars.
pub fn divider(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(stroke::HAIR, ui.available_height()),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, theme.divider);
}

/// Horizontal hairline rule, for stacked content.
pub fn rule(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), stroke::HAIR),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, theme.divider);
}

/// Density-scaled blank space, named by token.
pub fn gap(ui: &mut egui::Ui, theme: &Theme, token: f32) {
    ui.add_space(theme.sp(token));
}

/// The center dock's tab strip. Returns the id the user clicked, if any.
///
/// `tabs` is `(id, title, is_active)` — the host builds it from the registry,
/// so tabs cannot drift out of sync with the panels that exist.
pub fn tab_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[(&'static str, &'static str, bool)],
) -> Option<&'static str> {
    let mut picked = None;
    egui::Frame::new()
        .fill(theme.surface)
        .inner_margin(egui::Margin::symmetric(
            theme.sp(space::SM) as i8,
            theme.sp(space::XS) as i8,
        ))
        .show(ui, |ui| {
            ui.set_height(theme.sp(pane::TAB_H));
            ui.horizontal_centered(|ui| {
                for &(id, title, active) in tabs {
                    if ui.selectable_label(active, title).clicked() {
                        picked = Some(id);
                    }
                }
            });
        });
    rule(ui, theme);
    picked
}

// --------------------------------------------------------------- controls ---

pub fn button(ui: &mut egui::Ui, _theme: &Theme, text: &str) -> bool {
    ui.button(text).clicked()
}

/// A button that advertises its keyboard shortcut in a tooltip. `hint` comes
/// from the keymap, so the label and the key can never disagree.
pub fn button_hint(ui: &mut egui::Ui, _theme: &Theme, text: &str, hint: Option<&str>) -> bool {
    let response = ui.button(text);
    match hint {
        Some(keys) => response.on_hover_text(keys).clicked(),
        None => response.clicked(),
    }
}

/// A toggle that reads as pressed when on.
pub fn toggle(ui: &mut egui::Ui, on: bool, text: &str) -> bool {
    ui.selectable_label(on, text).clicked()
}

/// Tempo drag control. The widget mechanics (speed, precision) live here in
/// kit; the RANGE is domain truth from `vm::limits`.
pub fn bpm_drag(ui: &mut egui::Ui, bpm: &mut f64) -> bool {
    use crate::ui::vm::limits;
    ui.add(
        egui::DragValue::new(bpm)
            .range(limits::BPM_MIN..=limits::BPM_MAX)
            .speed(0.5)
            .fixed_decimals(1)
            .suffix(" bpm"),
    )
    .changed()
}

/// A small status lamp. `role` is a theme color the caller already named —
/// kit does not decide what "on" means.
pub fn led(ui: &mut egui::Ui, theme: &Theme, on: bool, role: Color32) {
    let d = theme.sp(space::SM);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
    let color = if on { role } else { theme.surface_sunken };
    ui.painter().circle_filled(rect.center(), d * 0.5, color);
    ui.painter().circle_stroke(
        rect.center(),
        d * 0.5,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
}

/// Vertical level meter. `norm` is 0..1 already mapped from dB by the caller
/// — meter *scaling* is domain knowledge and does not belong in the kit.
pub fn meter(ui: &mut egui::Ui, theme: &Theme, norm: f32) {
    let size = egui::vec2(theme.sp(control::METER_W), theme.sp(control::FADER_LEN));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CTRL, theme.surface_sunken);

    let norm = norm.clamp(0.0, 1.0);
    if norm <= 0.0 {
        return;
    }
    // Two thresholds, both style, both here: where the meter turns amber and
    // where it turns red. They are fractions of full scale, not dB.
    const HOT: f32 = 0.75;
    const CLIP: f32 = 0.97;
    let color = if norm >= CLIP {
        theme.meter_clip
    } else if norm >= HOT {
        theme.meter_hot
    } else {
        theme.meter_low
    };
    let filled = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - rect.height() * norm),
        rect.max,
    );
    painter.rect_filled(filled, radius::CTRL, color);
}

/// The knob sweep, shared by every rotary control in the app.
///
/// The 270-degree, gap-at-the-bottom convention: 0 sits at 7 o'clock, 0.5
/// at 12, 1.0 at 5, turning CLOCKWISE as the value rises.
pub const KNOB_START: f32 = std::f32::consts::PI * 0.75;
pub const KNOB_SWEEP: f32 = std::f32::consts::PI * 1.5;

/// The angle of a knob at normalized value `t`, for `knob_dir` and for
/// drawing partial arcs.
pub fn knob_angle(t: f32) -> f32 {
    KNOB_START + KNOB_SWEEP * t.clamp(0.0, 1.0)
}

/// Which way a knob's pointer aims at angle `a`, as a unit vector in
/// SCREEN space.
///
/// Screen y grows downward, which is exactly what makes `(cos, sin)`
/// sweep clockwise — no correction is needed or wanted. This used to
/// negate the x, which mirrors the whole sweep: the minimum landed at 5
/// o'clock instead of 7 and every knob in the app turned anticlockwise as
/// its value rose. It read as merely odd on a unipolar knob and as plainly
/// wrong on the arrangement's bipolar pan, where "L40" pointed right.
///
/// One helper because it was the same eight lines of trigonometry in three
/// files, and it was wrong in all three.
pub fn knob_dir(a: f32) -> egui::Vec2 {
    egui::vec2(a.cos(), a.sin())
}

/// Rotary control. Drag vertically to change; returns true on change.
///
/// `value` is normalized 0..1 — the caller owns the mapping to Hz, dB, or
/// whatever it means, for the same reason `meter` takes a normalized level.
pub fn knob(ui: &mut egui::Ui, theme: &Theme, value: &mut f32) -> bool {
    let d = theme.sp(control::KNOB);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::drag());

    let mut changed = false;
    if response.dragged() {
        // Full travel over roughly two knob-heights of drag: fine enough to
        // aim, coarse enough to cross the range without a second grab.
        let travel = d * 4.0;
        let delta = -response.drag_delta().y / travel;
        if delta != 0.0 {
            *value = (*value + delta).clamp(0.0, 1.0);
            changed = true;
        }
    }

    // Sweep from 7 o'clock to 5 o'clock — the 270-degree gap-at-the-bottom
    // convention every hardware knob uses.
    const START: f32 = std::f32::consts::PI * 0.75;
    const SWEEP: f32 = std::f32::consts::PI * 1.5;
    let center = rect.center();
    let radius = d * 0.5 - stroke::BOLD;
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
                center + knob_dir(a) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };
    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let filled = theme.accent;
    if *value > 0.0 {
        arc(
            START,
            START + SWEEP * value.clamp(0.0, 1.0),
            egui::Stroke::new(stroke::BOLD, filled),
        );
    }
    // The pointer.
    let a = START + SWEEP * value.clamp(0.0, 1.0);
    let dir = knob_dir(a);
    painter.line_segment(
        [center + dir * (radius * 0.4), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );

    if response.has_focus() || response.dragged() {
        ui.painter().circle_stroke(
            center,
            radius + stroke::FOCUS,
            egui::Stroke::new(stroke::FOCUS, theme.focus),
        );
    }
    changed
}

/// Fixed-width monospace caption, for tables that must not jitter as digits
/// change. Width is in characters, not pixels.
pub fn mono_fixed(ui: &mut egui::Ui, theme: &Theme, text: &str, chars: usize) {
    let width = chars as f32 * font::VALUE * 0.6;
    ui.scope(|ui| {
        ui.set_min_width(width);
        value(ui, theme, text);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The knob sweep runs 7 o'clock -> 12 -> 5 o'clock, CLOCKWISE.
    ///
    /// This is here because it was wrong, silently, in three files at once:
    /// a negated x mirrored the whole sweep, so every knob turned
    /// anticlockwise and the arrangement's pan knob pointed right while its
    /// readout said "L40". Geometry that only a screenshot can falsify is
    /// exactly the geometry to pin.
    #[test]
    fn the_knob_sweeps_clockwise_from_seven_oclock() {
        let at = |t: f32| knob_dir(knob_angle(t));
        let approx = |v: f32, want: f32| (v - want).abs() < 1e-4;

        // Minimum: down and to the LEFT — 7 o'clock. Screen y grows down,
        // so "down" is positive.
        let min = at(0.0);
        assert!(
            min.x < 0.0 && min.y > 0.0,
            "0.0 sits at 7 o'clock, got {min:?}"
        );
        // Middle: straight up.
        let mid = at(0.5);
        assert!(
            approx(mid.x, 0.0) && mid.y < 0.0,
            "0.5 points up, got {mid:?}"
        );
        // Maximum: down and to the RIGHT — 5 o'clock.
        let max = at(1.0);
        assert!(
            max.x > 0.0 && max.y > 0.0,
            "1.0 sits at 5 o'clock, got {max:?}"
        );

        // The two ends are mirror images, and the sweep is 270 degrees.
        assert!(approx(min.x, -max.x) && approx(min.y, max.y));
        assert!(approx(KNOB_SWEEP, std::f32::consts::PI * 1.5));

        // Rising values move rightward from 12 o'clock to 3 — strictly
        // increasing x over that quarter is what "clockwise" means here.
        // Past 3 o'clock x turns back, so the check stops at t = 0.83,
        // where the pointer reaches the 3-o'clock mark.
        for i in 0..8 {
            let a = at(0.5 + i as f32 * 0.04);
            let b = at(0.5 + (i + 1) as f32 * 0.04);
            assert!(b.x > a.x, "the pointer must travel clockwise");
        }

        // Out-of-range values clamp rather than spinning past the gap.
        assert_eq!(at(-1.0), at(0.0));
        assert_eq!(at(2.0), at(1.0));
    }
}
