//! The dynamics transfer curve: what a compressor, limiter, gate or
//! expander does to a level.
//!
//! # One widget, four effects
//!
//! These are not four displays. A **limiter is a compressor with a very
//! high ratio**; a **gate is an expander with a very high ratio**. Drawing
//! them as separate widgets would mean four copies of one piece of
//! arithmetic, drifting apart — and it would hide the thing worth knowing,
//! which is that a limiter and a compressor differ by a number you can
//! drag.
//!
//! # The maths
//!
//! The standard soft-knee gain computer, in dB throughout. For downward
//! compression above a threshold `T` with ratio `R` and knee width `W`:
//!
//! ```text
//! 2(x-T) < -W   →  y = x
//! |2(x-T)| ≤ W  →  y = x + (1/R - 1)(x - T + W/2)² / (2W)
//! 2(x-T) > W    →  y = T + (x - T)/R
//! ```
//!
//! Expansion is the mirror: unity above the threshold, steeper below. The
//! quadratic knee is not decoration — it is what makes the curve
//! continuous in its first derivative, and a hard corner is audible as a
//! click on material that sits right at the threshold.
//!
//! # What the display gets right
//!
//! - **Square.** Input across, output up, the same dB range on both. The
//!   unity diagonal has to be 45 degrees or the eye cannot read how much
//!   is coming off.
//! - **The unity diagonal is drawn**, dim. It is the reference the curve
//!   means nothing without.
//! - **The knee is visible as a curve**, because it is one.
//! - **A live operating point** when a level is supplied: a dot on the
//!   curve with guides down to both axes, and the gain reduction in dB.
//!   That is the number people actually watch.

use crate::ui::device::metrics::{self, Footprint};
use crate::ui::device::{adjust, design};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, stroke};
use eframe::egui;

/// The window, in dB, on both axes. Deep enough to show a gate opening,
/// shallow enough that the working range is not a smear at the top.
pub const VIEW_MIN_DB: f32 = -60.0;
pub const VIEW_MAX_DB: f32 = 0.0;
/// Grid every this many dB.
const GRID_STEP_DB: f32 = 12.0;
/// Curve resolution, in screen points per step.
const CURVE_STEP_PX: f32 = 2.0;

/// Ratio limits. 1:1 is no processing; the top of the range is a limiter
/// or a gate depending on the direction.
pub const RATIO_MIN: f32 = 1.0;
pub const RATIO_MAX: f32 = 60.0;

/// Which way the curve bends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Downward compression ABOVE the threshold. A compressor, or — at a
    /// high enough ratio — a limiter.
    Compress,
    /// Downward expansion BELOW the threshold. An expander, or — at a
    /// high enough ratio — a gate.
    Expand,
}

impl Mode {
    pub const ALL: [Self; 2] = [Self::Compress, Self::Expand];
    pub const NAMES: &'static [&'static str] = &["comp", "gate"];

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }
}

/// A dynamics processor's gain computer, as the display needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dynamics {
    pub mode: Mode,
    /// Where the curve leaves unity, in dBFS.
    pub threshold_db: f32,
    /// `n:1`. 1.0 is no processing.
    pub ratio: f32,
    /// Total knee WIDTH in dB, centred on the threshold. 0 is a hard
    /// corner.
    pub knee_db: f32,
    /// Level added after the curve, in dB.
    pub makeup_db: f32,
    /// How fast the reduction takes hold, in milliseconds.
    ///
    /// These two do NOT change the transfer curve — that is a static map
    /// with no time axis, and no amount of drawing will put one on it.
    /// They are what [`bars`] exists to show: the bar view is the one that
    /// can display a processor RESPONDING rather than a processor's rules.
    pub attack_ms: f32,
    /// How fast it lets go again.
    pub release_ms: f32,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            mode: Mode::Compress,
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            attack_ms: 10.0,
            release_ms: 120.0,
        }
    }
}

/// The reduction envelope: where the gain reduction actually IS, as
/// opposed to where the gain computer says it should be.
///
/// Pure and egui-free. A first-order approach toward the target with two
/// time constants — fast on the way in, slow on the way out — which is
/// what every dynamics processor does and what makes attack and release
/// mean anything.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Envelope {
    /// Reduction currently applied, in dB. Positive is quieter.
    pub shown_db: f32,
}

impl Envelope {
    /// Advance `dt` seconds toward `target_db` of reduction.
    ///
    /// The coefficient is `1 - exp(-dt / tau)`, the standard one-pole
    /// step. Not a linear ramp: a linear attack has a corner at the top
    /// that a real detector does not, and the exponential is the shape
    /// people have been listening to for seventy years.
    pub fn advance(&mut self, target_db: f32, dt: f32, attack_ms: f32, release_ms: f32) {
        let target = if target_db.is_finite() {
            target_db.max(0.0)
        } else {
            0.0
        };
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // Going up is attack, coming down is release. Which one applies
        // is decided by the DIRECTION, not by whether the signal is over
        // the threshold — a processor already reducing hard and asked to
        // reduce less is releasing, wherever the input sits.
        let tau_ms = if target > self.shown_db {
            attack_ms
        } else {
            release_ms
        };
        let tau = (tau_ms.max(0.01)) * 0.001;
        let coeff = 1.0 - (-dt / tau).exp();
        self.shown_db += (target - self.shown_db) * coeff.clamp(0.0, 1.0);
        if !self.shown_db.is_finite() {
            self.shown_db = target;
        }
    }

    /// Is anything still moving? A processor at rest should stop asking
    /// for repaints.
    pub fn moving(&self) -> bool {
        self.shown_db > 0.01
    }
}

impl Dynamics {
    /// Output level for an input level, both in dB. Makeup included.
    ///
    /// Pure, and the whole reason this widget can be trusted: a transfer
    /// curve that does not match the processor is worse than no display,
    /// because it is confidently wrong.
    pub fn output_db(&self, input_db: f32) -> f32 {
        self.curve_db(input_db) + self.makeup_db
    }

    /// The gain computer alone, before makeup.
    fn curve_db(&self, input_db: f32) -> f32 {
        if !input_db.is_finite() {
            return VIEW_MIN_DB;
        }
        let x = input_db;
        let t = self.threshold_db;
        let r = self.ratio.clamp(RATIO_MIN, RATIO_MAX);
        let w = self.knee_db.max(0.0);
        let over = x - t;

        match self.mode {
            Mode::Compress => {
                if 2.0 * over > w {
                    t + over / r
                } else if w > 0.0 && 2.0 * over.abs() <= w {
                    // The quadratic knee: continuous in value AND slope
                    // at both edges, which is what stops a click on
                    // material sitting right at the threshold.
                    let d = over + w * 0.5;
                    x + (1.0 / r - 1.0) * d * d / (2.0 * w)
                } else {
                    x
                }
            }
            Mode::Expand => {
                if 2.0 * over < -w {
                    t + over * r
                } else if w > 0.0 && 2.0 * over.abs() <= w {
                    let d = over - w * 0.5;
                    x + (1.0 - r) * d * d / (2.0 * w)
                } else {
                    x
                }
            }
        }
    }

    /// How much level the processor is removing at this input, in dB.
    ///
    /// Positive means quieter, which is the sign every gain-reduction
    /// meter in the world uses — and deliberately EXCLUDES makeup, because
    /// makeup is not reduction and counting it would let a compressor
    /// report zero while working hard.
    pub fn reduction_db(&self, input_db: f32) -> f32 {
        input_db - self.curve_db(input_db)
    }
}

/// Where `db` sits across the view, `0..=1`.
pub fn db_to_norm(db: f32) -> f32 {
    if db.is_nan() {
        return 0.0;
    }
    ((db - VIEW_MIN_DB) / (VIEW_MAX_DB - VIEW_MIN_DB)).clamp(0.0, 1.0)
}

/// The inverse, for turning a pointer position into a level.
pub fn norm_to_db(t: f32) -> f32 {
    VIEW_MIN_DB + (VIEW_MAX_DB - VIEW_MIN_DB) * t.clamp(0.0, 1.0)
}

/// Ratio as a `0..=1` position. Log-mapped, because the difference
/// between 2:1 and 4:1 is enormous and the difference between 40:1 and
/// 60:1 is nothing.
fn ratio_to_norm(r: f32) -> f32 {
    let r = r.clamp(RATIO_MIN, RATIO_MAX);
    (r / RATIO_MIN).ln() / (RATIO_MAX / RATIO_MIN).ln()
}

fn norm_to_ratio(t: f32) -> f32 {
    RATIO_MIN * (RATIO_MAX / RATIO_MIN).powf(t.clamp(0.0, 1.0))
}

// --------------------------------------------------------------- view ---

/// The full display's size contract: square, at [`control::TRANSFER`].
pub fn footprint(theme: &Theme) -> Footprint {
    let side = theme.sp(control::TRANSFER);
    Footprint::new(side, side)
}

/// The thumbnail's contract: square, and fixed for the same reason the
/// mini filter curve is — a thumbnail beside its knobs should be the same
/// size every time.
pub fn footprint_mini(theme: &Theme) -> Footprint {
    let side = theme.sp(control::TRANSFER_MINI);
    Footprint::new(side, side)
}

/// The curve as screen points across `rect`. Shared by both renderings so
/// the thumbnail cannot disagree with the display it is a thumbnail of.
fn curve_points(rect: egui::Rect, dyn_: &Dynamics) -> Vec<egui::Pos2> {
    let steps = ((rect.width() / CURVE_STEP_PX).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let out = dyn_.output_db(norm_to_db(t));
            egui::pos2(
                rect.left() + rect.width() * t,
                rect.bottom() - rect.height() * db_to_norm(out),
            )
        })
        .collect()
}

fn draw_grid(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, ticks: bool) {
    if ticks {
        let mut db = VIEW_MIN_DB + GRID_STEP_DB;
        while db < VIEW_MAX_DB {
            let t = db_to_norm(db);
            let x = rect.left() + rect.width() * t;
            let y = rect.bottom() - rect.height() * t;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(stroke::HAIR, theme.grid_sub),
            );
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(stroke::HAIR, theme.grid_sub),
            );
            db += GRID_STEP_DB;
        }
    }
    // Unity. The reference the curve means nothing without — a transfer
    // curve with no diagonal is a squiggle in a box.
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.grid_bar),
    );
}

/// Draw the transfer curve.
///
/// `level_db` is an optional live input level: supply one and the display
/// marks where the processor is working right now, with the gain
/// reduction spelled out. Drag across to move the threshold, up and down
/// to change the ratio. Returns true when the user changed something.
pub fn transfer_curve(
    ui: &mut egui::Ui,
    theme: &Theme,
    dyn_: &mut Dynamics,
    level_db: Option<f32>,
) -> bool {
    transfer_curve_sized(ui, theme, dyn_, level_db, footprint(theme).size.y)
}

/// The same display, at a size the caller chooses. Square, like the
/// footprint version — `side` is both dimensions.
///
/// Added for the gate's card, which draws this inside
/// `poly_widgets::dark_curve_panel`. That panel pays for its footer rows
/// first and hands the plot whatever is left, which on a tall card is
/// about 130 points — less than [`footprint`]'s 160. Allocating the fixed
/// square there overflowed the region and printed the curve through the
/// value strip underneath it, which is mistake one in
/// `notes/20260827-device-card-layout.md`, arriving from the widget's end
/// rather than the card's.
///
/// [`transfer_curve`] is unchanged and still takes the footprint, so the
/// size contract every other caller relies on is intact.
pub fn transfer_curve_sized(
    ui: &mut egui::Ui,
    theme: &Theme,
    dyn_: &mut Dynamics,
    level_db: Option<f32>,
    side: f32,
) -> bool {
    let side = side.max(1.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let mut changed = false;

    if response.dragged() {
        let d = response.drag_delta();
        if d.x != 0.0 {
            let t = db_to_norm(dyn_.threshold_db) + d.x / rect.width();
            let next = norm_to_db(t);
            if next != dyn_.threshold_db {
                dyn_.threshold_db = next;
                changed = true;
            }
        }
        if d.y != 0.0 {
            // Up is MORE ratio: a steeper curve is drawn further from
            // unity, and pulling the knee down toward the diagonal is
            // what less compression looks like.
            let next = norm_to_ratio(ratio_to_norm(dyn_.ratio) - d.y / rect.height());
            if next != dyn_.ratio {
                dyn_.ratio = next;
                changed = true;
            }
        }
    }
    let nudge = adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let next = norm_to_ratio(ratio_to_norm(dyn_.ratio) + nudge);
        if next != dyn_.ratio {
            dyn_.ratio = next;
            changed = true;
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
    draw_grid(painter, theme, rect, true);

    // The threshold, marked on the axis it belongs to.
    let tx = rect.left() + rect.width() * db_to_norm(dyn_.threshold_db);
    painter.line_segment(
        [egui::pos2(tx, rect.top()), egui::pos2(tx, rect.bottom())],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    let points = curve_points(rect, dyn_);
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::BOLD, theme.accent),
    ));

    // The live operating point, with guides to both axes. This is the
    // part people actually watch, so it is drawn last and brightest.
    if let Some(level) = level_db.filter(|l| l.is_finite()) {
        let out = dyn_.output_db(level);
        let px = rect.left() + rect.width() * db_to_norm(level);
        let py = rect.bottom() - rect.height() * db_to_norm(out);
        for seg in [
            [egui::pos2(px, py), egui::pos2(px, rect.bottom())],
            [egui::pos2(px, py), egui::pos2(rect.left(), py)],
        ] {
            painter.line_segment(seg, egui::Stroke::new(stroke::HAIR, theme.accent_muted));
        }
        painter.circle_filled(egui::pos2(px, py), stroke::FOCUS, theme.playhead);
    }

    // The reading: what is set, and what it is doing right now.
    let mut text = format!(
        "{:.0} dB   {:.1}:1",
        dyn_.threshold_db,
        dyn_.ratio.clamp(RATIO_MIN, RATIO_MAX)
    );
    if let Some(level) = level_db.filter(|l| l.is_finite()) {
        text.push_str(&format!("   -{:.1} dB", dyn_.reduction_db(level).max(0.0)));
    }
    painter.text(
        rect.left_top() + egui::vec2(design::gap(theme), design::gap(theme)),
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );

    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    changed
}

/// The bar view's size contract: two thick bars and the gap between,
/// stretching to whatever width it is given.
///
/// A MINIMUM width, unlike the transfer curve's fixed square. A level bar
/// is a ruler — more width is more resolution for setting a threshold
/// against real material — where a transfer curve has to stay square for
/// its diagonal to read.
pub fn footprint_bars(ui: &egui::Ui, theme: &Theme) -> Footprint {
    let bar = theme.sp(control::DYN_BAR_H);
    let gap = design::gap(theme);
    Footprint::new(
        theme.sp(control::XY_PAD),
        // Two bars, the gap between them, and the reading underneath.
        bar * 2.0 + gap * 1.5 + metrics::line_h(ui, font::LABEL),
    )
}

/// The bar view: set the threshold against the signal, and watch the
/// reduction move.
///
/// The complement to [`transfer_curve`], not a smaller version of it. A
/// transfer curve shows the RULES — what would happen to any level — and
/// is the right thing for dialling in a ratio and a knee. This shows what
/// is happening to THIS material right now, which is the only way to put
/// a threshold somewhere sensible, and it is the only one of the two that
/// can show attack and release at all.
///
/// Drag the threshold marker, or anywhere on the level bar, to move it.
/// Returns true when the user changed it.
pub fn bars(ui: &mut egui::Ui, theme: &Theme, dyn_: &mut Dynamics, level_db: f32) -> bool {
    let min = footprint_bars(ui, theme);
    // Stretchy but BOUNDED, the same cap every stretchy display uses.
    // Given a whole window this took one, ran off the end of its column
    // and drew its reading over whatever was beside it.
    let size = egui::vec2(
        ui.available_width()
            .clamp(min.width(), theme.sp(control::DISPLAY_W_MAX)),
        min.height(),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let bar_h = theme.sp(control::DYN_BAR_H);
    let gap = design::gap(theme);
    let mut changed = false;

    // Dragging anywhere on the strip moves the threshold to the pointer.
    // ABSOLUTE, not relative: the whole point of this view is putting the
    // threshold at a level you can see on the bar, so it should go where
    // you point rather than drifting from wherever it was.
    if let Some(pos) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let t = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let next = norm_to_db(t);
        if next != dyn_.threshold_db {
            dyn_.threshold_db = next;
            changed = true;
        }
    }
    let nudge = adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let next = norm_to_db(db_to_norm(dyn_.threshold_db) + nudge);
        if next != dyn_.threshold_db {
            dyn_.threshold_db = next;
            changed = true;
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // The reduction envelope, advanced by attack and release. This is the
    // state that makes the two knobs mean something.
    let id = response.id;
    let mut env: Envelope = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let dt = ui.input(|i| i.stable_dt);
    env.advance(
        dyn_.reduction_db(level_db),
        dt,
        dyn_.attack_ms,
        dyn_.release_ms,
    );

    let level = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), bar_h));
    let gr = egui::Rect::from_min_size(
        egui::pos2(rect.left(), level.bottom() + gap),
        egui::vec2(rect.width(), bar_h),
    );
    let x_at = |db: f32| rect.left() + rect.width() * db_to_norm(db);
    let painter = ui.painter();

    // --- the level bar, with the part over the threshold marked --------
    painter.rect_filled(level, design::box_radius(), theme.surface_sunken);
    let level_x = x_at(level_db);
    let thresh_x = x_at(dyn_.threshold_db);
    if level_x > level.left() {
        painter.rect_filled(
            egui::Rect::from_min_max(level.left_top(), egui::pos2(level_x, level.bottom())),
            design::box_radius(),
            theme.meter_low,
        );
    }
    // Everything past the threshold in the hot colour: that is the part
    // being worked on, and seeing it is the reason to set a threshold
    // here rather than on a transfer curve.
    if level_x > thresh_x {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(thresh_x, level.top()),
                egui::pos2(level_x, level.bottom()),
            ),
            0.0,
            theme.meter_hot,
        );
    }

    // --- the threshold marker ------------------------------------------
    // Drawn across BOTH bars, because it is the one setting that explains
    // both of them.
    painter.line_segment(
        [
            egui::pos2(thresh_x, rect.top()),
            egui::pos2(thresh_x, gr.bottom()),
        ],
        egui::Stroke::new(stroke::FOCUS, theme.text),
    );
    // A grab handle, so it reads as draggable without being hunted for.
    let handle = egui::Rect::from_center_size(
        egui::pos2(thresh_x, level.center().y),
        egui::vec2(stroke::FOCUS * 3.0, bar_h),
    );
    painter.rect_filled(handle, design::box_radius(), theme.text);

    // --- the reduction bar ---------------------------------------------
    // Grows from the RIGHT, leftward: reduction is level being taken
    // away, so it eats into the bar rather than filling it. A GR bar that
    // filled from the left would read as "more is louder".
    painter.rect_filled(gr, design::box_radius(), theme.surface_sunken);
    let span = VIEW_MAX_DB - VIEW_MIN_DB;
    let amount = (env.shown_db / span).clamp(0.0, 1.0);
    if amount > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(gr.right() - gr.width() * amount, gr.top()),
                gr.right_bottom(),
            ),
            design::box_radius(),
            theme.meter_hot,
        );
    }

    // --- the reading ----------------------------------------------------
    painter.text(
        egui::pos2(rect.left(), gr.bottom() + gap * 0.5),
        egui::Align2::LEFT_TOP,
        format!(
            "thresh {:.0}   in {:.0}   gr -{:.1} dB",
            dyn_.threshold_db, level_db, env.shown_db
        ),
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );

    for outline in [level, gr] {
        painter.rect_stroke(
            outline,
            design::box_radius(),
            egui::Stroke::new(stroke::HAIR, theme.outline),
            egui::StrokeKind::Inside,
        );
    }

    if env.moving() {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, env));
    changed
}

/// A thumbnail of the transfer curve: the shape and the diagonal, nothing
/// else. Display-only, for the same reason the mini filter curve is — the
/// knobs are right there.
pub fn mini(ui: &mut egui::Ui, theme: &Theme, dyn_: &Dynamics) {
    let (rect, _) = ui.allocate_exact_size(footprint_mini(theme).size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
    // No grid at this size: two dozen points of box cannot carry five
    // lines and still show a curve. The diagonal stays, because without
    // it the shape means nothing.
    draw_grid(painter, theme, rect, false);
    painter.add(egui::Shape::line(
        curve_points(rect, dyn_),
        egui::Stroke::new(stroke::HAIR, theme.accent),
    ));
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
}

/// The gain-reduction meter's contract — a lane the width of a meter,
/// as tall as the transfer curve beside it.
pub fn footprint_reduction(theme: &Theme) -> Footprint {
    Footprint::new(theme.sp(control::METER_W), theme.sp(control::TRANSFER))
}

/// A gain-reduction meter: a bar that grows DOWNWARD from the top.
///
/// The one meter that is upside down, and it has to be. Reduction is
/// something being taken away, so it hangs from unity — a GR meter that
/// filled from the bottom like a level meter would read as "more is
/// louder", which is exactly backwards.
pub fn reduction_meter(ui: &mut egui::Ui, theme: &Theme, reduction_db: f32) {
    let (rect, _) = ui.allocate_exact_size(footprint_reduction(theme).size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);

    let span = VIEW_MAX_DB - VIEW_MIN_DB;
    let amount = (reduction_db.max(0.0) / span).clamp(0.0, 1.0);
    if amount > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.right(), rect.top() + rect.height() * amount),
            ),
            design::box_radius(),
            theme.meter_hot,
        );
    }
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    let _ = metrics::interactive_min(ui);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn comp(threshold: f32, ratio: f32, knee: f32) -> Dynamics {
        Dynamics {
            mode: Mode::Compress,
            threshold_db: threshold,
            ratio,
            knee_db: knee,
            makeup_db: 0.0,
            ..Dynamics::default()
        }
    }

    /// Below the threshold a compressor does nothing at all. The first
    /// thing to get right: a curve that touched quiet material would be
    /// audible on everything.
    #[test]
    fn a_compressor_leaves_quiet_material_alone() {
        let c = comp(-20.0, 4.0, 0.0);
        for db in [-60.0, -50.0, -30.0, -21.0] {
            assert!(
                (c.output_db(db) - db).abs() < 0.001,
                "{db} dB should pass untouched, got {}",
                c.output_db(db)
            );
            assert!(c.reduction_db(db).abs() < 0.001);
        }
    }

    /// Above the threshold the curve's slope is exactly 1/ratio. Measured
    /// over a span rather than asserted at a point, because the slope IS
    /// the ratio — a display whose 4:1 really draws 2:1 is confidently
    /// wrong about the only number that matters.
    #[test]
    fn the_slope_above_threshold_is_one_over_the_ratio() {
        for ratio in [1.5f32, 2.0, 4.0, 8.0, 20.0, 60.0] {
            let c = comp(-40.0, ratio, 0.0);
            let (a, b) = (-20.0f32, -10.0f32);
            let slope = (c.output_db(b) - c.output_db(a)) / (b - a);
            assert!(
                (slope - 1.0 / ratio).abs() < 0.001,
                "{ratio}:1 should slope {:.4}, measured {slope:.4}",
                1.0 / ratio
            );
        }
    }

    /// 1:1 is a straight line everywhere — no threshold, no knee, no
    /// effect. The identity case, and the one a knee formula divides by.
    #[test]
    fn a_ratio_of_one_is_the_identity() {
        for knee in [0.0f32, 6.0, 24.0] {
            let c = comp(-20.0, 1.0, knee);
            for i in 0..=60 {
                let db = -60.0 + i as f32;
                assert!(
                    (c.output_db(db) - db).abs() < 0.001,
                    "1:1 with a {knee} dB knee changed {db} to {}",
                    c.output_db(db)
                );
            }
        }
    }

    /// A very high ratio is a limiter: the output stops rising.
    #[test]
    fn a_high_ratio_is_a_limiter() {
        let l = comp(-12.0, RATIO_MAX, 0.0);
        let at = l.output_db(-12.0);
        let way_over = l.output_db(0.0);
        assert!(
            (way_over - at).abs() < 0.25,
            "12 dB of input over the threshold moved the output {:.2} dB",
            way_over - at
        );
        assert!(l.reduction_db(0.0) > 11.0, "and that is all reduction");
    }

    /// The soft knee is CONTINUOUS and SMOOTH: it meets both asymptotes
    /// in value and in slope. A hard corner is audible as a click on
    /// material sitting right at the threshold, which is precisely the
    /// material a compressor is set up for.
    #[test]
    fn the_knee_joins_both_asymptotes_smoothly() {
        let t = -20.0f32;
        let w = 8.0f32;
        let c = comp(t, 4.0, w);
        let hard = comp(t, 4.0, 0.0);

        // At the knee edges it agrees with the hard-knee curve.
        for edge in [t - w / 2.0, t + w / 2.0] {
            assert!(
                (c.output_db(edge) - hard.output_db(edge)).abs() < 0.01,
                "the knee must meet the corner at {edge}"
            );
        }
        // No jumps anywhere across it.
        let step = 0.05f32;
        let mut prev = c.output_db(t - w);
        let mut prev_slope: Option<f32> = None;
        let mut x = t - w + step;
        while x <= t + w {
            let y = c.output_db(x);
            let slope = (y - prev) / step;
            assert!((y - prev).abs() < 0.2, "a jump at {x}");
            if let Some(before) = prev_slope {
                assert!(
                    (slope - before).abs() < 0.05,
                    "the slope jumps at {x}: {before:.3} -> {slope:.3}"
                );
            }
            prev_slope = Some(slope);
            prev = y;
            x += step;
        }
        // And it sits BELOW the hard corner around the threshold — a
        // soft knee starts working early, which is the point of it.
        assert!(c.output_db(t) < hard.output_db(t));
    }

    /// An expander is the mirror: unity above, steeper below.
    #[test]
    fn an_expander_works_below_the_threshold() {
        let e = Dynamics {
            mode: Mode::Expand,
            threshold_db: -30.0,
            ratio: 4.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            ..Dynamics::default()
        };
        for db in [-20.0, -10.0, 0.0] {
            assert!((e.output_db(db) - db).abs() < 0.001, "unity above at {db}");
        }
        // Below, the slope is the ratio itself — steeper, not shallower.
        let (a, b) = (-50.0f32, -40.0f32);
        let slope = (e.output_db(b) - e.output_db(a)) / (b - a);
        assert!((slope - 4.0).abs() < 0.001, "measured {slope}");
        // Which means quiet material gets quieter, never louder.
        assert!(e.output_db(-50.0) < -50.0);
    }

    /// A high-ratio expander is a gate: below the threshold, gone.
    #[test]
    fn a_high_ratio_expander_is_a_gate() {
        let g = Dynamics {
            mode: Mode::Expand,
            threshold_db: -40.0,
            ratio: RATIO_MAX,
            knee_db: 0.0,
            makeup_db: 0.0,
            ..Dynamics::default()
        };
        assert!(
            g.output_db(-45.0) < VIEW_MIN_DB,
            "5 dB under and it is shut"
        );
        assert!((g.output_db(-20.0) + 20.0).abs() < 0.001, "open above");
    }

    /// Makeup shifts the whole curve and NOTHING else — in particular it
    /// does not change the reported reduction, or a compressor could show
    /// zero while working hard.
    #[test]
    fn makeup_lifts_the_curve_without_faking_the_reduction() {
        let plain = comp(-20.0, 4.0, 6.0);
        let lifted = Dynamics {
            makeup_db: 7.5,
            ..plain
        };
        for i in 0..=60 {
            let db = -60.0 + i as f32;
            assert!((lifted.output_db(db) - plain.output_db(db) - 7.5).abs() < 0.001);
            assert!(
                (lifted.reduction_db(db) - plain.reduction_db(db)).abs() < 0.001,
                "makeup must not flatter the reduction reading"
            );
        }
    }

    /// The curve never folds back. A transfer function that decreased
    /// somewhere would mean louder in, quieter out — which is not a
    /// setting, it is a bug that sounds like modulation.
    #[test]
    fn the_curve_never_folds_back() {
        for mode in Mode::ALL {
            for ratio in [1.0f32, 2.0, 8.0, RATIO_MAX] {
                for knee in [0.0f32, 6.0, 24.0] {
                    let d = Dynamics {
                        mode,
                        threshold_db: -24.0,
                        ratio,
                        knee_db: knee,
                        makeup_db: 0.0,
                        ..Dynamics::default()
                    };
                    let mut last = f32::NEG_INFINITY;
                    for i in 0..=240 {
                        let x = VIEW_MIN_DB + i as f32 * 0.25;
                        let y = d.output_db(x);
                        assert!(
                            y >= last - 0.001,
                            "{mode:?} {ratio}:1 knee {knee} folded back at {x}"
                        );
                        last = y;
                    }
                }
            }
        }
    }

    /// Reduction is positive when working and zero when not — the sign
    /// every gain-reduction meter uses.
    #[test]
    fn reduction_is_positive_and_zero_at_rest() {
        let c = comp(-20.0, 4.0, 6.0);
        assert!(c.reduction_db(-40.0).abs() < 0.001, "not working");
        assert!(c.reduction_db(-5.0) > 0.0, "working");
        assert!(c.reduction_db(0.0) > c.reduction_db(-5.0), "and harder");
    }

    /// Nonsense answers a number. A NaN here becomes a NaN point, and
    /// egui draws that as nothing — a curve that silently disappears.
    #[test]
    fn nonsense_inputs_stay_finite() {
        let wild = Dynamics {
            mode: Mode::Compress,
            threshold_db: 1e9,
            ratio: -5.0,
            knee_db: -3.0,
            makeup_db: f32::MAX,
            ..Dynamics::default()
        };
        for db in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e9, 0.0] {
            assert!(wild.curve_db(db).is_finite(), "curve at {db}");
        }
        assert!(db_to_norm(f32::NAN) == 0.0);
        for db in [-1e9, 1e9, f32::NAN] {
            let n = db_to_norm(db);
            assert!((0.0..=1.0).contains(&n));
        }
    }

    /// The envelope reaches its target, and takes about the time it was
    /// told to. One time constant is 63% of the way — the number that
    /// makes "10 ms attack" mean something rather than being a vibe.
    #[test]
    fn the_envelope_follows_its_time_constants() {
        let (attack, release) = (10.0f32, 200.0f32);
        let mut e = Envelope::default();
        // After exactly one attack time constant, 1 - 1/e of the way.
        let steps = 100;
        let dt = attack * 0.001 / steps as f32;
        for _ in 0..steps {
            e.advance(12.0, dt, attack, release);
        }
        let want = 12.0 * (1.0 - (-1.0f32).exp());
        assert!(
            (e.shown_db - want).abs() < 0.2,
            "one attack constant should reach {want:.2}, got {:.2}",
            e.shown_db
        );
        // And it converges rather than overshooting.
        for _ in 0..2_000 {
            e.advance(12.0, dt, attack, release);
        }
        assert!((e.shown_db - 12.0).abs() < 0.01);
        assert!(e.shown_db <= 12.0 + 1e-4, "an envelope must not overshoot");
    }

    /// Attack and release are DIFFERENT, and which applies is decided by
    /// the direction of travel — not by whether the input is over the
    /// threshold. A processor reducing hard and asked to reduce less is
    /// releasing, wherever the signal sits.
    #[test]
    fn attack_is_the_way_in_and_release_is_the_way_out() {
        let (attack, release) = (5.0f32, 500.0f32);
        let dt = 0.001f32;

        let mut rising = Envelope::default();
        rising.advance(20.0, dt, attack, release);
        let gained = rising.shown_db;

        let mut falling = Envelope { shown_db: 20.0 };
        falling.advance(0.0, dt, attack, release);
        let lost = 20.0 - falling.shown_db;

        assert!(
            gained > lost * 5.0,
            "a 5 ms attack must move far faster than a 500 ms release: {gained:.3} vs {lost:.3}"
        );

        // Partial release: still on its way down, still using release.
        let mut easing = Envelope { shown_db: 20.0 };
        easing.advance(12.0, dt, attack, release);
        assert!(easing.shown_db < 20.0 && easing.shown_db > 19.0, "gentle");
    }

    /// A silent processor settles and stops asking for repaints, and time
    /// standing still moves nothing.
    #[test]
    fn the_envelope_settles_and_ignores_a_stopped_clock() {
        let mut e = Envelope::default();
        assert!(!e.moving());
        e.advance(9.0, 0.05, 10.0, 100.0);
        assert!(e.moving());
        for _ in 0..1_000 {
            e.advance(0.0, 0.01, 10.0, 100.0);
        }
        assert!(!e.moving(), "silence settles: {:?}", e);

        let mut still = Envelope { shown_db: 4.0 };
        still.advance(0.0, 0.0, 10.0, 100.0);
        assert_eq!(still.shown_db, 4.0, "zero dt is not a step");
        still.advance(0.0, -1.0, 10.0, 100.0);
        assert_eq!(still.shown_db, 4.0, "negative dt is not rewind");
    }

    /// Nonsense times and targets answer a number. A zero attack is a
    /// legal setting (instant), not a division by zero.
    #[test]
    fn the_envelope_survives_nonsense() {
        let mut e = Envelope::default();
        e.advance(6.0, 0.01, 0.0, 0.0);
        assert!(e.shown_db.is_finite());
        assert!((e.shown_db - 6.0).abs() < 0.01, "a zero attack is instant");

        for target in [f32::NAN, f32::INFINITY, -50.0] {
            let mut e = Envelope { shown_db: 3.0 };
            e.advance(target, 0.01, 10.0, 10.0);
            assert!(e.shown_db.is_finite(), "target {target}");
            assert!(e.shown_db >= 0.0, "reduction is never negative");
        }
        let mut e = Envelope { shown_db: 3.0 };
        e.advance(6.0, f32::NAN, 10.0, 10.0);
        assert!(e.shown_db.is_finite());
    }

    /// The two views answer different questions, and the bar view is the
    /// only one that can answer the timing one at all.
    #[test]
    fn the_bar_view_stretches_where_the_curve_stays_square() {
        let theme = Theme::dark();
        let square = footprint(&theme);
        assert_eq!(square.width(), square.height());
        // Attack and release do not appear in the transfer curve, by
        // construction: it is a static map with no time axis. Changing
        // them must not move it by so much as a decibel.
        let a = Dynamics::default();
        let b = Dynamics {
            attack_ms: 200.0,
            release_ms: 5.0,
            ..a
        };
        for i in 0..=60 {
            let db = VIEW_MIN_DB + i as f32;
            assert_eq!(a.output_db(db), b.output_db(db));
        }
    }

    /// The axes map and invert, and the ratio mapping is log — the
    /// difference between 2:1 and 4:1 is enormous, between 40:1 and 60:1
    /// nothing, and a linear control would spend most of its travel there.
    #[test]
    fn the_mappings_invert() {
        for db in [VIEW_MIN_DB, -30.0, VIEW_MAX_DB] {
            assert!((norm_to_db(db_to_norm(db)) - db).abs() < 0.01);
        }
        for r in [RATIO_MIN, 2.0, 4.0, 20.0, RATIO_MAX] {
            assert!((norm_to_ratio(ratio_to_norm(r)) - r).abs() < r * 0.001);
        }
        // Log: equal travel is equal RATIO, not equal difference.
        let low = ratio_to_norm(4.0) - ratio_to_norm(2.0);
        let high = ratio_to_norm(16.0) - ratio_to_norm(8.0);
        assert!((low - high).abs() < 1e-4, "{low} vs {high}");
    }

    /// The thumbnail is smaller and square, and both sizes are square —
    /// the unity diagonal has to read as 45 degrees.
    #[test]
    fn both_sizes_are_square() {
        let theme = Theme::dark();
        for fp in [footprint(&theme), footprint_mini(&theme)] {
            assert_eq!(fp.width(), fp.height(), "a transfer curve must be square");
        }
        assert!(footprint_mini(&theme).width() < footprint(&theme).width());
    }
}
