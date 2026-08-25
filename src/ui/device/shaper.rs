//! The waveshaper transfer curve: what a distortion does to a sample.
//!
//! Input amplitude across, output amplitude up, both `-1..=1`, with the
//! unity diagonal drawn. The same shape of display as [`dynamics`], on
//! LINEAR axes instead of decibels — because a distortion acts on
//! samples, and a level meter's dB scale would hide the thing that
//! matters, which is what happens either side of zero.
//!
//! [`dynamics`]: crate::ui::device::dynamics
//!
//! # The one property that inverts
//!
//! A dynamics curve that folded back would be a bug: louder in, quieter
//! out is not a setting. Here **folding back is a whole effect**. A
//! wavefolder is non-monotonic on purpose, which is exactly why it sounds
//! the way it does, and the test suite asserts the fold IS non-monotonic
//! and that everything else is not. Copying the dynamics test across
//! unexamined would have quietly outlawed the most interesting mode.
//!
//! # What the modes are
//!
//! Five shapes that cover the space rather than five names for one:
//!
//! - **Hard clip** — a corner. Odd harmonics, harsh, the sound of a
//!   converter running out of numbers.
//! - **Soft clip** — `tanh`. Approaches the rails without reaching them.
//! - **Cubic** — `1.5x − 0.5x³`, the classic cheap soft clipper, with a
//!   gentler knee than tanh and an exact corner at the rails.
//! - **Fold** — reflects at the rails instead of stopping. The only
//!   non-monotonic mode.
//! - **Crush** — quantise to a staircase. Here **drive means resolution,
//!   not gain**: a bitcrusher driven harder has FEWER levels, not more
//!   volume, and treating drive as gain would just have made it clip.
//!
//! # Bias, and why it is not decoration
//!
//! `bias` offsets the signal before shaping, so the curve is no longer
//! odd-symmetric. That asymmetry is what generates EVEN harmonics — the
//! difference between a transistor and a valve — and it is visible on the
//! curve as the shape leaning off the origin. There is a test that bias
//! breaks the symmetry, because a bias control that did not would be a
//! knob that does nothing.

use crate::ui::device::metrics::Footprint;
use crate::ui::device::{adjust, design};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, stroke};
use eframe::egui;

/// The view: full scale either way, on both axes.
pub const VIEW_MIN: f32 = -1.0;
pub const VIEW_MAX: f32 = 1.0;
/// Drive limits, as an input gain.
pub const DRIVE_MIN: f32 = 1.0;
pub const DRIVE_MAX: f32 = 32.0;
/// Bias limits, as an offset added before shaping.
pub const BIAS_MAX: f32 = 0.9;
/// Steps a crusher has at minimum drive. Divided by the drive from there.
const CRUSH_STEPS: f32 = 48.0;
/// Curve resolution, in screen points per step.
const CURVE_STEP_PX: f32 = 1.5;
/// Grid lines at the quarters.
const GRID: [f32; 2] = [-0.5, 0.5];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    HardClip,
    SoftClip,
    Cubic,
    Fold,
    Crush,
}

impl Mode {
    pub const ALL: [Self; 5] = [
        Self::HardClip,
        Self::SoftClip,
        Self::Cubic,
        Self::Fold,
        Self::Crush,
    ];
    pub const NAMES: &'static [&'static str] = &["hard", "soft", "cubic", "fold", "crush"];

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    /// Does this mode fold back on itself? Only one does, and it is the
    /// reason the monotonicity test is written the way it is.
    pub fn folds(self) -> bool {
        matches!(self, Self::Fold)
    }
}

/// A waveshaper, as the display needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaper {
    pub mode: Mode,
    /// Input gain before the shaper — except in [`Mode::Crush`], where it
    /// is resolution instead. See the module note.
    pub drive: f32,
    /// Offset added before shaping. Breaks the symmetry, which is what
    /// makes even harmonics.
    pub bias: f32,
    /// Dry/wet, `0..=1`. At 0 the curve is exactly the diagonal.
    pub mix: f32,
}

impl Default for Shaper {
    fn default() -> Self {
        Self {
            mode: Mode::SoftClip,
            drive: 3.0,
            bias: 0.0,
            mix: 1.0,
        }
    }
}

/// A triangle fold: identity inside the rails, reflected outside.
///
/// `(2/π)·asin(sin(πx/2))` is the exact triangle wave of period 4 that
/// passes through the origin with slope 1 — so it needs no branches and
/// no floor, and it cannot disagree with itself at the seams the way a
/// hand-rolled reflection does.
///
/// The clamp is not decoration: `sin` can return 1.0000001, and `asin` of
/// that is NaN. A NaN here becomes a NaN point, which egui draws as
/// nothing at all — a curve that silently vanishes.
fn triangle_fold(x: f32) -> f32 {
    use std::f32::consts::PI;
    let s = (x * PI * 0.5).sin().clamp(-1.0, 1.0);
    (2.0 / PI) * s.asin()
}

impl Shaper {
    /// The shaped output for an input sample. Both in `-1..=1`.
    ///
    /// Pure, which is the only way a transfer curve can be trusted: a
    /// display that does not match the processor is worse than none,
    /// because it is confidently wrong.
    pub fn shape(&self, x: f32) -> f32 {
        if !x.is_finite() {
            return 0.0;
        }
        let drive = self.drive.clamp(DRIVE_MIN, DRIVE_MAX);
        let bias = self.bias.clamp(-BIAS_MAX, BIAS_MAX);
        let mix = self.mix.clamp(0.0, 1.0);

        let wet = match self.mode {
            Mode::Crush => {
                // Drive is RESOLUTION here. Multiplying a signal that is
                // about to be quantised only clips it, which is a
                // different effect wearing this one's name.
                let steps = (CRUSH_STEPS / drive).round().max(2.0);
                let t = (x + bias).clamp(-1.0, 1.0);
                (t * steps).round() / steps
            }
            mode => {
                let driven = x * drive + bias;
                match mode {
                    Mode::HardClip => driven.clamp(-1.0, 1.0),
                    Mode::SoftClip => driven.tanh(),
                    Mode::Cubic => {
                        let t = driven.clamp(-1.0, 1.0);
                        1.5 * t - 0.5 * t * t * t
                    }
                    Mode::Fold => triangle_fold(driven),
                    Mode::Crush => unreachable!("handled above"),
                }
            }
        };

        let y = x * (1.0 - mix) + wet * mix;
        if y.is_finite() {
            y.clamp(VIEW_MIN, VIEW_MAX)
        } else {
            0.0
        }
    }
}

/// Where an amplitude sits across the view, `0..=1`.
pub fn amp_to_norm(a: f32) -> f32 {
    if a.is_nan() {
        return 0.5;
    }
    ((a - VIEW_MIN) / (VIEW_MAX - VIEW_MIN)).clamp(0.0, 1.0)
}

/// The inverse, for turning a pointer position into an amplitude.
pub fn norm_to_amp(t: f32) -> f32 {
    VIEW_MIN + (VIEW_MAX - VIEW_MIN) * t.clamp(0.0, 1.0)
}

fn drive_to_norm(d: f32) -> f32 {
    let d = d.clamp(DRIVE_MIN, DRIVE_MAX);
    (d / DRIVE_MIN).ln() / (DRIVE_MAX / DRIVE_MIN).ln()
}

fn norm_to_drive(t: f32) -> f32 {
    DRIVE_MIN * (DRIVE_MAX / DRIVE_MIN).powf(t.clamp(0.0, 1.0))
}

// --------------------------------------------------------------- view ---

/// The display's contract: square, at [`control::TRANSFER`].
///
/// Square for the same reason the dynamics curve is: the unity diagonal
/// has to read as 45 degrees, or the eye cannot judge how far the shape
/// has been pulled off it.
pub fn footprint(theme: &Theme) -> Footprint {
    let side = theme.sp(control::TRANSFER);
    Footprint::new(side, side)
}

/// The thumbnail's contract: square and fixed.
pub fn footprint_mini(theme: &Theme) -> Footprint {
    let side = theme.sp(control::TRANSFER_MINI);
    Footprint::new(side, side)
}

/// The curve as screen points. Shared by both renderings, so the
/// thumbnail cannot disagree with the display it is a thumbnail of.
fn curve_points(rect: egui::Rect, shaper: &Shaper) -> Vec<egui::Pos2> {
    let steps = ((rect.width() / CURVE_STEP_PX).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let y = shaper.shape(norm_to_amp(t));
            egui::pos2(
                rect.left() + rect.width() * t,
                rect.bottom() - rect.height() * amp_to_norm(y),
            )
        })
        .collect()
}

fn draw_ground(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, detail: bool) {
    if detail {
        for v in GRID {
            let x = rect.left() + rect.width() * amp_to_norm(v);
            let y = rect.bottom() - rect.height() * amp_to_norm(v);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(stroke::HAIR, theme.grid_sub),
            );
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(stroke::HAIR, theme.grid_sub),
            );
        }
    }
    // The ZERO crosshair, always. On a bipolar plot the origin is the
    // thing every shape is read against — where it crosses, whether it
    // is symmetric about it, how steep it is there.
    let cx = rect.left() + rect.width() * 0.5;
    let cy = rect.top() + rect.height() * 0.5;
    for seg in [
        [egui::pos2(cx, rect.top()), egui::pos2(cx, rect.bottom())],
        [egui::pos2(rect.left(), cy), egui::pos2(rect.right(), cy)],
    ] {
        painter.line_segment(seg, egui::Stroke::new(stroke::HAIR, theme.grid_beat));
    }
    // Unity, dim: the shape means nothing without something to be pulled
    // away from.
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.grid_bar),
    );
}

/// Draw the transfer curve.
///
/// Vertical drag is drive — up is more, and the curve visibly steepens.
/// Horizontal drag is bias, and the shape slides the way you push it.
/// Returns true when the user changed something.
pub fn transfer_curve(ui: &mut egui::Ui, theme: &Theme, shaper: &mut Shaper) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(footprint(theme).size, egui::Sense::click_and_drag());
    let mut changed = false;

    if response.dragged() {
        let d = response.drag_delta();
        if d.y != 0.0 {
            let next = norm_to_drive(drive_to_norm(shaper.drive) - d.y / rect.height());
            if next != shaper.drive {
                shaper.drive = next;
                changed = true;
            }
        }
        if d.x != 0.0 {
            // The curve slides the way the pointer pushes it, which is
            // the only mapping that does not feel inverted.
            let next =
                (shaper.bias + d.x / rect.width() * BIAS_MAX * 2.0).clamp(-BIAS_MAX, BIAS_MAX);
            if next != shaper.bias {
                shaper.bias = next;
                changed = true;
            }
        }
    }
    let nudge = adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let next = norm_to_drive(drive_to_norm(shaper.drive) + nudge);
        if next != shaper.drive {
            shaper.drive = next;
            changed = true;
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
    draw_ground(painter, theme, rect, true);
    painter.add(egui::Shape::line(
        curve_points(rect, shaper),
        egui::Stroke::new(stroke::BOLD, theme.accent),
    ));

    let mut text = format!(
        "{}  x{:.1}",
        Mode::NAMES[Mode::ALL
            .iter()
            .position(|m| *m == shaper.mode)
            .unwrap_or(0)],
        shaper.drive.clamp(DRIVE_MIN, DRIVE_MAX)
    );
    if shaper.bias.abs() > 0.005 {
        text.push_str(&format!("  bias {:+.2}", shaper.bias));
    }
    if shaper.mix < 0.999 {
        text.push_str(&format!("  mix {:.0}%", shaper.mix * 100.0));
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

/// A thumbnail of the shape. Display-only — the knobs are right there.
pub fn mini(ui: &mut egui::Ui, theme: &Theme, shaper: &Shaper) {
    let (rect, _) = ui.allocate_exact_size(footprint_mini(theme).size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
    // No quarter grid at this size — the crosshair and the diagonal are
    // all that survive being this small, and they are the two that carry
    // the meaning.
    draw_ground(painter, theme, rect, false);
    painter.add(egui::Shape::line(
        curve_points(rect, shaper),
        egui::Stroke::new(stroke::HAIR, theme.accent),
    ));
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sweep() -> impl Iterator<Item = f32> {
        (0..=400).map(|i| -1.0 + i as f32 / 200.0)
    }

    fn at(mode: Mode, drive: f32) -> Shaper {
        Shaper {
            mode,
            drive,
            bias: 0.0,
            mix: 1.0,
        }
    }

    /// Fully dry is the identity, in every mode and at every drive. The
    /// case a mix control has to get exactly right, because "no effect"
    /// must mean no effect rather than nearly none.
    #[test]
    fn a_dry_shaper_is_the_identity() {
        for mode in Mode::ALL {
            for drive in [DRIVE_MIN, 4.0, DRIVE_MAX] {
                let s = Shaper {
                    mix: 0.0,
                    bias: 0.5,
                    ..at(mode, drive)
                };
                for x in sweep() {
                    assert!(
                        (s.shape(x) - x).abs() < 1e-6,
                        "{mode:?} at x{drive} changed {x} to {}",
                        s.shape(x)
                    );
                }
            }
        }
    }

    /// Nothing ever leaves the rails. A transfer curve that ran off the
    /// top of its own box would be useless, and a shaper that returned
    /// 30.0 would be a fuzz pedal wired to a speaker.
    #[test]
    fn the_output_never_leaves_the_rails() {
        for mode in Mode::ALL {
            for drive in [DRIVE_MIN, 2.0, 9.0, DRIVE_MAX] {
                for bias in [-BIAS_MAX, 0.0, BIAS_MAX] {
                    let s = Shaper {
                        bias,
                        ..at(mode, drive)
                    };
                    for x in sweep().chain([-1e6, 1e6, -3.0, 3.0]) {
                        let y = s.shape(x);
                        assert!(
                            (VIEW_MIN..=VIEW_MAX).contains(&y),
                            "{mode:?} x{drive} bias {bias} sent {x} to {y}"
                        );
                    }
                }
            }
        }
    }

    /// With no bias every shape is ODD-symmetric: `f(-x) = -f(x)`. That
    /// is what "only odd harmonics" means, and it is the baseline the
    /// bias control exists to break.
    #[test]
    fn without_bias_every_shape_is_odd_symmetric() {
        for mode in Mode::ALL {
            for drive in [DRIVE_MIN, 5.0, DRIVE_MAX] {
                let s = at(mode, drive);
                for x in sweep() {
                    assert!(
                        (s.shape(-x) + s.shape(x)).abs() < 1e-5,
                        "{mode:?} at x{drive} is not odd at {x}: {} vs {}",
                        s.shape(x),
                        s.shape(-x)
                    );
                }
            }
        }
    }

    /// Bias BREAKS that symmetry — which is the whole point of it. A bias
    /// control that left the curve odd would be a knob that does nothing,
    /// and nothing about the drawn result would say so.
    #[test]
    fn bias_breaks_the_symmetry() {
        for mode in Mode::ALL {
            let s = Shaper {
                bias: 0.35,
                ..at(mode, 4.0)
            };
            let worst = sweep()
                .map(|x| (s.shape(-x) + s.shape(x)).abs())
                .fold(0.0f32, f32::max);
            assert!(
                worst > 0.05,
                "{mode:?} stayed symmetric under bias (worst asymmetry {worst})"
            );
        }
    }

    /// THE property that inverts from the dynamics curve: a fold folds
    /// BACK, and nothing else does.
    ///
    /// In a dynamics curve, folding back is a bug — louder in, quieter
    /// out. Here it is an entire effect. Copying that test across without
    /// thinking would have outlawed the most interesting mode.
    #[test]
    fn only_the_fold_folds_back() {
        for mode in Mode::ALL {
            let s = at(mode, 12.0);
            let mut went_backwards = false;
            let mut last = s.shape(-1.0);
            for x in sweep() {
                let y = s.shape(x);
                if y < last - 1e-4 {
                    went_backwards = true;
                }
                last = y;
            }
            assert_eq!(
                went_backwards,
                mode.folds(),
                "{mode:?}: folds() says {} but the curve says {went_backwards}",
                mode.folds()
            );
        }
    }

    /// Hard clip has a flat top; soft clip approaches the rail without
    /// ever getting there. The difference people can hear, stated as the
    /// difference the maths makes.
    #[test]
    fn hard_clips_flat_and_soft_never_quite_arrives() {
        let hard = at(Mode::HardClip, 4.0);
        assert_eq!(hard.shape(0.5), 1.0, "past the knee it is pinned");
        assert_eq!(hard.shape(0.9), 1.0, "and stays pinned");

        let soft = at(Mode::SoftClip, 4.0);
        assert!(soft.shape(1.0) < 1.0, "tanh never reaches the rail");
        assert!(soft.shape(1.0) > 0.99, "but it gets close");
        // And it is still rising where the hard clip has given up.
        assert!(soft.shape(0.9) < soft.shape(1.0));
    }

    /// A crusher is a staircase: finitely many outputs, and FEWER of them
    /// as drive rises. Drive is resolution here, not gain — treating it
    /// as gain would have made this mode a clipper with extra steps.
    #[test]
    fn a_crusher_is_a_staircase_that_coarsens_with_drive() {
        let count = |drive: f32| {
            let s = at(Mode::Crush, drive);
            let mut seen: Vec<i32> = sweep().map(|x| (s.shape(x) * 10_000.0) as i32).collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len()
        };
        let gentle = count(DRIVE_MIN);
        let brutal = count(DRIVE_MAX);
        assert!(gentle > brutal, "{gentle} levels should exceed {brutal}");
        assert!(brutal >= 3, "even at full crush there is a staircase");
        assert!(gentle < 400, "and it is quantised, not continuous");
    }

    /// More drive means more shaping: the curve gets further from the
    /// diagonal. Monotonically, so the knob always does something.
    #[test]
    fn more_drive_pulls_further_from_the_diagonal() {
        for mode in [Mode::HardClip, Mode::SoftClip, Mode::Cubic] {
            let away = |drive: f32| {
                let s = at(mode, drive);
                sweep()
                    .map(|x| (s.shape(x) - x).abs())
                    .fold(0.0f32, f32::max)
            };
            let (a, b, c) = (away(1.5), away(6.0), away(24.0));
            assert!(a < b && b < c, "{mode:?}: {a:.3} {b:.3} {c:.3}");
        }
    }

    /// Mix blends linearly between the diagonal and the shape — halfway
    /// is halfway, not a crossfade with a mind of its own.
    #[test]
    fn mix_blends_linearly_toward_the_shape() {
        let wet = at(Mode::Cubic, 6.0);
        let half = Shaper { mix: 0.5, ..wet };
        for x in sweep() {
            let want = x * 0.5 + wet.shape(x) * 0.5;
            assert!(
                (half.shape(x) - want).abs() < 1e-5,
                "at {x}: {} vs {want}",
                half.shape(x)
            );
        }
    }

    /// Nonsense answers a number. A NaN becomes a NaN point, and egui
    /// draws that as nothing — a curve that silently disappears.
    #[test]
    fn nonsense_stays_finite() {
        let wild = Shaper {
            mode: Mode::Fold,
            drive: -5.0,
            bias: 40.0,
            mix: 9.0,
        };
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30, 0.0] {
            let y = wild.shape(x);
            assert!(y.is_finite(), "{x} gave {y}");
            assert!((VIEW_MIN..=VIEW_MAX).contains(&y));
        }
        // The fold's `asin(sin(x))` is the specific place a NaN could get
        // in: `sin` can return a hair over 1.0.
        for x in [1.0f32, 2.0, 3.0, 1e7, -1e7] {
            assert!(triangle_fold(x).is_finite(), "fold at {x}");
        }
        assert!(
            (triangle_fold(1.0) - 1.0).abs() < 1e-5,
            "identity at the rail"
        );
        assert!(triangle_fold(2.0).abs() < 1e-5, "folded all the way back");
        assert!((triangle_fold(0.5) - 0.5).abs() < 1e-5, "identity inside");
    }

    /// The mappings invert, and drive is log — the difference between x1
    /// and x2 is enormous, between x24 and x32 nothing.
    #[test]
    fn the_mappings_invert() {
        for a in [VIEW_MIN, -0.25, 0.0, VIEW_MAX] {
            assert!((norm_to_amp(amp_to_norm(a)) - a).abs() < 1e-5);
        }
        assert_eq!(
            amp_to_norm(0.0),
            0.5,
            "zero is the middle of a bipolar axis"
        );
        assert_eq!(amp_to_norm(f32::NAN), 0.5, "and so is nonsense");
        for d in [DRIVE_MIN, 2.0, 8.0, DRIVE_MAX] {
            assert!((norm_to_drive(drive_to_norm(d)) - d).abs() < d * 0.001);
        }
        let low = drive_to_norm(4.0) - drive_to_norm(2.0);
        let high = drive_to_norm(16.0) - drive_to_norm(8.0);
        assert!((low - high).abs() < 1e-4, "log: {low} vs {high}");
    }

    /// Both sizes are square, for the same reason the dynamics curve is.
    #[test]
    fn both_sizes_are_square() {
        let theme = Theme::dark();
        for fp in [footprint(&theme), footprint_mini(&theme)] {
            assert_eq!(fp.width(), fp.height());
        }
        assert!(footprint_mini(&theme).width() < footprint(&theme).width());
    }
}
