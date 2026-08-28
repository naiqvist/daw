//! ADSR envelope editor: three draggable handles over a bezier-drawn
//! curve. Values are normalized 0..1 — the caller owns the mapping of
//! attack/decay/release to real time (and can show it with `readout`).

use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::bezier::{Cubic, Pt};
use crate::ui::device::metrics::Footprint;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, stroke};
use eframe::egui;

/// Normalized envelope values. `sustain` is a level; the others are stage
/// lengths as a fraction of each stage's maximum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            attack: 0.1,
            decay: 0.3,
            sustain: 0.7,
            release: 0.25,
        }
    }
}

/// Fraction of the widget width each timed stage may occupy at full value.
const STAGE_W: f32 = 0.3;
/// Fraction of the width the sustain plateau always occupies.
const PLATEAU_W: f32 = 1.0 - 3.0 * STAGE_W;
/// Bend of the timed segments: fast start, the exponential-envelope look.
const BEND: f32 = 0.6;
/// Curve flattening steps per segment.
const STEPS: usize = 24;

/// Draw and edit the envelope across the available width. Returns true
/// when any value changed.
/// The envelope editor's size contract: [`control::ENV_H`] tall and at
/// least an XY pad wide.
///
/// A MINIMUM, unlike the knob's exact figure — the editor is happy to
/// stretch, and does, into whatever width its well gives it. What the
/// contract promises is that below this it stops being editable: the
/// handles need somewhere to go.
pub fn footprint(theme: &Theme) -> Footprint {
    Footprint::new(theme.sp(control::XY_PAD), theme.sp(control::ENV_H))
}

pub fn adsr(ui: &mut egui::Ui, theme: &Theme, env: &mut Adsr) -> bool {
    adsr_with_footprint(ui, theme, env, footprint(theme))
}

/// The envelope editor compressed for an instrument face. It is the same
/// three-handle editor as [`adsr`], not a thumbnail: the larger invisible
/// grab regions stay intact even though the curve itself is shorter.
pub fn footprint_compact(theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::CURVE_MINI_W),
        theme.sp(control::ENV_COMPACT_H),
    )
}

pub fn adsr_compact(ui: &mut egui::Ui, theme: &Theme, env: &mut Adsr) -> bool {
    adsr_with_footprint(ui, theme, env, footprint_compact(theme))
}

/// The compact editor stretched into an exact parent surface.
///
/// Composite synth widgets use this when the envelope graph and its value
/// cells share one dark panel. The handles keep their normal interaction
/// floor; only the curve receives the space its parent reclaimed.
pub fn adsr_fill(ui: &mut egui::Ui, theme: &Theme, env: &mut Adsr) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    adsr_with_footprint(ui, theme, env, Footprint::from_size(size))
}

fn adsr_with_footprint(ui: &mut egui::Ui, theme: &Theme, env: &mut Adsr, min: Footprint) -> bool {
    let width = ui.available_width().max(min.width());
    let size = egui::vec2(width, min.height());
    let (rect, env_response) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Handle ids hang off THIS allocation's id, not the parent scope's:
    // two envelopes in one panel must never share an "env-a".
    let base_id = env_response.id;

    // Keep the curve and handles inside the frame.
    let pad = theme.sp(control::HANDLE) + stroke::FOCUS;
    let area = rect.shrink(pad);

    let x_at = |frac: f32| area.left() + area.width() * frac;
    let y_at = |level: f32| area.bottom() - area.height() * level.clamp(0.0, 1.0);

    // Stage boundaries, as width fractions.
    let fa = STAGE_W * env.attack.clamp(0.0, 1.0);
    let fd = fa + STAGE_W * env.decay.clamp(0.0, 1.0);
    let fs = fd + PLATEAU_W;
    let fr = fs + STAGE_W * env.release.clamp(0.0, 1.0);

    // --- interaction: three handles, one id each -------------------------
    let mut changed = false;
    let handle_r = theme.sp(control::HANDLE);
    let grab = handle_r + stroke::FOCUS * 2.0;
    let handle = |ui: &mut egui::Ui, tag: &str, center: egui::Pos2| -> Option<egui::Pos2> {
        let hrect = egui::Rect::from_center_size(center, egui::vec2(grab, grab) * 2.0);
        let response = ui
            .interact(hrect, base_id.with(tag), egui::Sense::drag())
            .affords(Affords::Sweep);
        let pos = response
            .dragged()
            .then(|| response.interact_pointer_pos())??;
        Some(pos)
    };

    let a_pos = egui::pos2(x_at(fa), y_at(1.0));
    let d_pos = egui::pos2(x_at(fd), y_at(env.sustain));
    let r_pos = egui::pos2(x_at(fr), y_at(0.0));

    if let Some(p) = handle(ui, "env-a", a_pos) {
        let next = ((p.x - area.left()) / (area.width() * STAGE_W)).clamp(0.0, 1.0);
        changed |= next != env.attack;
        env.attack = next;
    }
    if let Some(p) = handle(ui, "env-d", d_pos) {
        let next = ((p.x - x_at(fa)) / (area.width() * STAGE_W)).clamp(0.0, 1.0);
        changed |= next != env.decay;
        env.decay = next;
        let level = ((area.bottom() - p.y) / area.height()).clamp(0.0, 1.0);
        changed |= level != env.sustain;
        env.sustain = level;
    }
    if let Some(p) = handle(ui, "env-r", r_pos) {
        let next = ((p.x - x_at(fs)) / (area.width() * STAGE_W)).clamp(0.0, 1.0);
        changed |= next != env.release;
        env.release = next;
    }

    // Recompute after edits so the frame draws the new state, not last
    // frame's — a one-frame lag on a dragged handle reads as slippery.
    let fa = STAGE_W * env.attack.clamp(0.0, 1.0);
    let fd = fa + STAGE_W * env.decay.clamp(0.0, 1.0);
    let fs = fd + PLATEAU_W;
    let fr = fs + STAGE_W * env.release.clamp(0.0, 1.0);

    // --- paint ------------------------------------------------------------
    let painter = ui.painter();
    // NO ground and NO border of its own. TE's envelope screen is a
    // curve on the same black field as everything else — the border and
    // the second grey were a panel-stack habit, and on a display they
    // draw a box around a shape that is already legible.
    let _ = rect;

    // The BASELINE and the stage boundaries, dim — the OP-1's envelope
    // screen draws exactly these and nothing else behind the curve. The
    // old quarter-lines went with them: four horizontal rules across a
    // shape whose whole job is to be read as a shape is noise, and TE
    // draws none.
    painter.line_segment(
        [
            egui::pos2(area.left(), y_at(0.0)),
            egui::pos2(area.right(), y_at(0.0)),
        ],
        egui::Stroke::new(stroke::HAIR, theme.role_time_dim),
    );
    for f in [fa, fd, fs] {
        painter.line_segment(
            [
                egui::pos2(x_at(f), y_at(0.0)),
                egui::pos2(x_at(f), y_at(1.0)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.role_time_dim),
        );
    }

    // THIN strokes, coloured by what the stage IS: the rising and falling
    // TIMES are blue, the DECAY toward a level is ochre. The colour says
    // "this is a time" or "this is a level" before the shape says how
    // much — TE's whole point, and why their screens need no labels.
    let draw_segment = |seg: Cubic, color: egui::Color32| {
        let curve_stroke = egui::Stroke::new(stroke::HAIR, color);
        let mut pts: Vec<Pt> = Vec::new();
        seg.polyline(STEPS, &mut pts);
        let points: Vec<egui::Pos2> = pts
            .iter()
            .map(|p| egui::pos2(x_at(p.x), y_at(p.y)))
            .collect();
        painter.add(egui::Shape::line(points, curve_stroke));
    };

    draw_segment(Cubic::segment(0.0, 0.0, fa, 1.0, BEND), theme.role_time);
    draw_segment(
        Cubic::segment(fa, 1.0, fd, env.sustain, BEND),
        theme.role_level,
    );
    // The sustain plateau is a HOLD, not a move: flat, blue, thin.
    painter.line_segment(
        [
            egui::pos2(x_at(fd), y_at(env.sustain)),
            egui::pos2(x_at(fs), y_at(env.sustain)),
        ],
        egui::Stroke::new(stroke::HAIR, theme.role_time),
    );
    draw_segment(
        Cubic::segment(fs, env.sustain, fr, 0.0, BEND),
        theme.role_time,
    );

    // The DOTS are the handles — the affordance TE actually uses. Their
    // colour says which family the drag belongs to: the peak and the
    // sustain corner set LEVELS (ochre), the release foot sets a TIME
    // (blue). A dot at the origin anchors the shape, undraggable.
    painter.circle_filled(
        egui::pos2(x_at(0.0), y_at(0.0)),
        handle_r * 0.75,
        theme.role_time,
    );
    for (tag, center, color) in [
        ("env-a", egui::pos2(x_at(fa), y_at(1.0)), theme.role_level),
        (
            "env-d",
            egui::pos2(x_at(fd), y_at(env.sustain)),
            theme.role_level,
        ),
        ("env-r", egui::pos2(x_at(fr), y_at(0.0)), theme.role_time),
    ] {
        let id = base_id.with(tag);
        let active = ui.ctx().is_being_dragged(id);
        painter.circle_filled(center, handle_r, color);
        if active {
            painter.circle_stroke(
                center,
                handle_r + stroke::FOCUS,
                egui::Stroke::new(stroke::FOCUS, theme.focus),
            );
        }
    }

    changed
}
