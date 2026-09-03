//! One keyboard cursor for the whole application.
//!
//! Interactive surfaces report the rectangle currently addressed by the
//! keyboard.  A single overlay owns the visible mark, its motion and its
//! shape, so moving between views cannot leave one static cursor behind while
//! another appears at the destination.

use eframe::egui;

const SPRING: f32 = 285.0;
const DAMPING: f32 = 25.0;
const STYLE_SPEED: f32 = 15.0;
const FADE_SPEED: f32 = 18.0;
const SETTLE_SECONDS: f32 = 0.13;

/// What kind of thing the keyboard is addressing. Every variant is rendered
/// by the same vector grammar; these values only change its proportions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Cell,
    Row,
    Block,
    Column,
    Instrument,
    Playhead,
    Prompt,
}

/// Which compositional plane owns a target. A modal cursor must win over the
/// still-visible surface beneath it regardless of paint order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Layer {
    Surface,
    Overlay,
    Palette,
    Utility,
}

#[derive(Clone, Copy, Debug)]
struct Target {
    id: egui::Id,
    rect: egui::Rect,
    kind: Kind,
    layer: Layer,
    ink: egui::Color32,
}

#[derive(Clone, Debug, Default)]
struct Registry(Option<Target>);

fn registry_id() -> egui::Id {
    egui::Id::new("global-qwerty-cursor-target")
}

fn motion_id() -> egui::Id {
    egui::Id::new("global-qwerty-cursor-motion")
}

fn settings_id() -> egui::Id {
    egui::Id::new("global-qwerty-cursor-settings")
}

#[derive(Clone, Copy, Debug)]
struct Settings {
    reduced_motion: bool,
    energy: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            reduced_motion: false,
            energy: 1.0,
        }
    }
}

/// Apply machine-local presentation preferences without making them part of
/// any target. The cursor remains one object; only its motion budget and ink
/// weight change.
pub fn configure(
    ctx: &egui::Context,
    reduced_motion: bool,
    energy: crate::ui::prefs::CursorEnergy,
) {
    let energy = match energy {
        crate::ui::prefs::CursorEnergy::Quiet => 0.68,
        crate::ui::prefs::CursorEnergy::Normal => 1.0,
        crate::ui::prefs::CursorEnergy::High => 1.32,
    };
    ctx.data_mut(|data| {
        data.insert_temp(
            settings_id(),
            Settings {
                reduced_motion,
                energy,
            },
        );
    });
}

/// Clear only this frame's target. Motion remains persistent across frames.
pub fn begin_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(registry_id(), Registry::default()));
}

/// Offer the keyboard's active rectangle to the global cursor.
pub fn claim(
    painter: &egui::Painter,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: egui::Rect,
    kind: Kind,
    layer: Layer,
    ink: egui::Color32,
) {
    let rect = rect.intersect(painter.clip_rect());
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    let target = Target {
        id: egui::Id::new(id),
        rect,
        kind,
        layer,
        ink,
    };
    painter.ctx().data_mut(|data| {
        let registry = data.get_temp_mut_or_default::<Registry>(registry_id());
        if registry
            .0
            .is_none_or(|standing| target.layer >= standing.layer)
        {
            registry.0 = Some(target);
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Style {
    arm: f32,
    cut: f32,
    side_ticks: f32,
    cross: f32,
    spine: f32,
}

impl Style {
    fn for_kind(kind: Kind) -> Self {
        match kind {
            Kind::Cell => Self::new(0.24, 0.18, 0.28, 0.18, 0.10),
            Kind::Row => Self::new(0.16, 0.10, 1.00, 0.04, 0.18),
            Kind::Block => Self::new(0.30, 0.30, 0.64, 0.12, 0.22),
            Kind::Column => Self::new(0.34, 0.22, 0.78, 0.22, 1.00),
            Kind::Instrument => Self::new(0.29, 0.42, 0.48, 1.00, 0.30),
            Kind::Playhead => Self::new(0.42, 0.55, 0.92, 1.00, 1.00),
            Kind::Prompt => Self::new(0.36, 0.34, 0.70, 0.82, 0.24),
        }
    }

    const fn new(arm: f32, cut: f32, side_ticks: f32, cross: f32, spine: f32) -> Self {
        Self {
            arm,
            cut,
            side_ticks,
            cross,
            spine,
        }
    }

    fn approach(&mut self, target: Self, amount: f32) {
        self.arm = egui::lerp(self.arm..=target.arm, amount);
        self.cut = egui::lerp(self.cut..=target.cut, amount);
        self.side_ticks = egui::lerp(self.side_ticks..=target.side_ticks, amount);
        self.cross = egui::lerp(self.cross..=target.cross, amount);
        self.spine = egui::lerp(self.spine..=target.spine, amount);
    }
}

#[derive(Clone, Debug)]
struct Motion {
    initialized: bool,
    target_id: Option<egui::Id>,
    target_kind: Kind,
    destination: egui::Rect,
    center: egui::Pos2,
    size: egui::Vec2,
    center_velocity: egui::Vec2,
    size_velocity: egui::Vec2,
    style: Style,
    opacity: f32,
    moving: bool,
    settle: Option<f32>,
    hops: u32,
    ink: egui::Color32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            initialized: false,
            target_id: None,
            target_kind: Kind::Cell,
            destination: egui::Rect::NOTHING,
            center: egui::Pos2::ZERO,
            size: egui::Vec2::ZERO,
            center_velocity: egui::Vec2::ZERO,
            size_velocity: egui::Vec2::ZERO,
            style: Style::for_kind(Kind::Cell),
            opacity: 0.0,
            moving: false,
            settle: None,
            hops: 0,
            ink: egui::Color32::WHITE,
        }
    }
}

#[derive(Clone, Copy)]
struct Visual {
    rect: egui::Rect,
    style: Style,
    opacity: f32,
    velocity: egui::Vec2,
    settle: Option<(f32, u32)>,
    ink: egui::Color32,
}

impl Motion {
    fn advance(&mut self, target: Option<Target>, dt: f32, settings: Settings) -> (Visual, bool) {
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 30.0);
        let Some(target) = target else {
            self.opacity = approach(self.opacity, 0.0, FADE_SPEED, dt);
            let visual = self.visual();
            return (visual, self.opacity > 0.01);
        };

        if !self.initialized {
            self.initialized = true;
            self.target_id = Some(target.id);
            self.target_kind = target.kind;
            self.destination = target.rect;
            self.center = target.rect.center();
            self.size = target.rect.size();
            self.style = Style::for_kind(target.kind);
            self.ink = target.ink;
            // The app has a keyboard address on its very first frame. Do not
            // make that address wait through a fade before it becomes legible;
            // subsequent disappearance and return still use the shared fade.
            self.opacity = 1.0;
        } else {
            let redirected = self.target_id != Some(target.id)
                || self.target_kind != target.kind
                || rect_distance(self.destination, target.rect) > 0.35;
            if redirected {
                self.hops = self.hops.wrapping_add(1);
                self.moving = true;
                self.settle = None;
            }
            self.target_id = Some(target.id);
            self.target_kind = target.kind;
            self.destination = target.rect;
            self.ink = target.ink;
        }

        if settings.reduced_motion {
            self.center = self.destination.center();
            self.size = self.destination.size();
            self.center_velocity = egui::Vec2::ZERO;
            self.size_velocity = egui::Vec2::ZERO;
            self.moving = false;
            self.settle = None;
        } else {
            // A true second-order cursor: distance creates acceleration,
            // velocity carries it between panels, and damping catches it at
            // the address. Two bounded substeps keep a dropped frame from
            // changing the feel.
            let sub_dt = dt * 0.5;
            for _ in 0..2 {
                spring_vec2(
                    &mut self.center,
                    &mut self.center_velocity,
                    self.destination.center(),
                    sub_dt,
                );
                spring_size(
                    &mut self.size,
                    &mut self.size_velocity,
                    self.destination.size(),
                    sub_dt,
                );
            }
        }
        self.size = self.size.max(egui::Vec2::splat(2.0));
        let style_amount = if settings.reduced_motion {
            1.0
        } else {
            1.0 - (-STYLE_SPEED * dt).exp()
        };
        self.style
            .approach(Style::for_kind(target.kind), style_amount);
        self.opacity = approach(self.opacity, 1.0, FADE_SPEED, dt);

        let error = self.center.distance(self.destination.center())
            + (self.size - self.destination.size()).length();
        let speed = self.center_velocity.length() + self.size_velocity.length();
        if self.moving && error < 0.65 && speed < 10.0 {
            self.center = self.destination.center();
            self.size = self.destination.size();
            self.center_velocity = egui::Vec2::ZERO;
            self.size_velocity = egui::Vec2::ZERO;
            self.moving = false;
            self.settle = Some(0.0);
        }
        if let Some(time) = &mut self.settle {
            *time += dt;
            if *time >= SETTLE_SECONDS {
                self.settle = None;
            }
        }

        let repaint = self.moving
            || self.settle.is_some()
            || self.opacity < 0.995
            || error > 0.2
            || speed > 0.2;
        (self.visual(), repaint)
    }

    fn visual(&self) -> Visual {
        Visual {
            rect: egui::Rect::from_center_size(self.center, self.size),
            style: self.style,
            opacity: self.opacity,
            velocity: self.center_velocity,
            settle: self.settle.map(|time| (time / SETTLE_SECONDS, self.hops)),
            ink: self.ink,
        }
    }
}

fn approach(value: f32, target: f32, speed: f32, dt: f32) -> f32 {
    egui::lerp(value..=target, 1.0 - (-speed * dt).exp())
}

fn spring_vec2(position: &mut egui::Pos2, velocity: &mut egui::Vec2, target: egui::Pos2, dt: f32) {
    *velocity += (target - *position) * (SPRING * dt);
    *velocity *= (-DAMPING * dt).exp();
    *position += *velocity * dt;
}

fn spring_size(position: &mut egui::Vec2, velocity: &mut egui::Vec2, target: egui::Vec2, dt: f32) {
    *velocity += (target - *position) * (SPRING * dt);
    *velocity *= (-DAMPING * dt).exp();
    *position += *velocity * dt;
}

fn rect_distance(a: egui::Rect, b: egui::Rect) -> f32 {
    a.center().distance(b.center()) + (a.size() - b.size()).length()
}

/// Paint the persistent cursor above every application surface.
pub fn paint(ctx: &egui::Context) {
    let target = ctx.data_mut(|data| {
        data.remove_temp::<Registry>(registry_id())
            .and_then(|registry| registry.0)
    });
    let dt = ctx.input(|input| input.stable_dt);
    let settings = ctx.data(|data| data.get_temp::<Settings>(settings_id()).unwrap_or_default());
    let (visual, repaint) = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Motion>(motion_id())
            .advance(target, dt, settings)
    });
    if repaint {
        ctx.request_repaint();
    }
    if visual.opacity <= 0.01 || !visual.rect.is_positive() {
        return;
    }

    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("global-qwerty-cursor-overlay"),
        ))
        .with_clip_rect(ctx.content_rect());
    draw(&painter, visual, settings.energy);
}

fn draw(painter: &egui::Painter, visual: Visual, energy: f32) {
    let mut rect = visual.rect.expand(3.0);
    let mut punch = 0.0;
    if let Some((progress, variant)) = visual.settle {
        let scale = settle_scale(progress, variant);
        rect = egui::Rect::from_center_size(rect.center(), rect.size() * scale);
        punch = ((progress * core::f32::consts::PI).sin() * 0.85).max(0.0);
    }

    let ink = visual
        .ink
        .gamma_multiply((visual.opacity * energy.min(1.0)).clamp(0.0, 1.0));
    let stroke = egui::Stroke::new((1.5 + punch) * energy, ink);
    let short = rect.width().min(rect.height()).max(2.0);
    let arm_x = (rect.width() * visual.style.arm).clamp(4.0, 18.0);
    let arm_y = (rect.height() * visual.style.arm).clamp(4.0, 18.0);
    let cut = (short * 0.16 * visual.style.cut).clamp(0.0, 4.0);
    for (corner, sx, sy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.right_bottom(), -1.0, -1.0),
        (rect.left_bottom(), 1.0, -1.0),
    ] {
        painter.add(egui::Shape::line(
            vec![
                corner + egui::vec2(0.0, sy * arm_y),
                corner + egui::vec2(sx * cut, sy * cut),
                corner + egui::vec2(sx * arm_x, 0.0),
            ],
            stroke,
        ));
    }

    let tick_ink = ink.gamma_multiply(visual.style.side_ticks.clamp(0.0, 1.0));
    let tick_stroke = egui::Stroke::new(1.0 + punch * 0.35, tick_ink);
    let tick = (short * 0.16).clamp(2.0, 6.0);
    painter.line_segment(
        [
            rect.left_center() - egui::vec2(tick, 0.0),
            rect.left_center() + egui::vec2(tick, 0.0),
        ],
        tick_stroke,
    );
    painter.line_segment(
        [
            rect.right_center() - egui::vec2(tick, 0.0),
            rect.right_center() + egui::vec2(tick, 0.0),
        ],
        tick_stroke,
    );
    painter.line_segment(
        [
            rect.center_top() - egui::vec2(0.0, tick),
            rect.center_top() + egui::vec2(0.0, tick),
        ],
        tick_stroke,
    );
    painter.line_segment(
        [
            rect.center_bottom() - egui::vec2(0.0, tick),
            rect.center_bottom() + egui::vec2(0.0, tick),
        ],
        tick_stroke,
    );

    let spine_ink = ink.gamma_multiply((visual.style.spine * 0.72).clamp(0.0, 1.0));
    let spine_h = rect.height() * 0.22;
    for x in [rect.left() - 2.0, rect.right() + 2.0] {
        painter.line_segment(
            [
                egui::pos2(x, rect.center().y - spine_h),
                egui::pos2(x, rect.center().y + spine_h),
            ],
            egui::Stroke::new(1.0, spine_ink),
        );
    }

    let cross_ink = ink.gamma_multiply((visual.style.cross * 0.82).clamp(0.0, 1.0));
    let cross = (short * 0.11).clamp(2.0, 5.0);
    painter.line_segment(
        [
            rect.center() - egui::vec2(cross, 0.0),
            rect.center() + egui::vec2(cross, 0.0),
        ],
        egui::Stroke::new(1.0, cross_ink),
    );
    painter.line_segment(
        [
            rect.center() - egui::vec2(0.0, cross),
            rect.center() + egui::vec2(0.0, cross),
        ],
        egui::Stroke::new(1.0, cross_ink),
    );

    // Velocity becomes two short fins behind the moving mark. They are the
    // only motion-specific geometry and disappear completely at rest.
    let speed = visual.velocity.length();
    if speed > 24.0 {
        let forward = visual.velocity / speed;
        let side = egui::vec2(-forward.y, forward.x);
        let tail = rect.center() - forward * (short * 0.65 + 7.0);
        let reach = (speed * 0.018).clamp(4.0, 13.0);
        let fin_ink = ink.gamma_multiply((speed / 850.0).clamp(0.10, 0.42));
        for sign in [-1.0, 1.0] {
            painter.line_segment(
                [
                    tail + side * sign * 3.0,
                    tail - forward * reach + side * sign * 6.0,
                ],
                egui::Stroke::new(1.0, fin_ink),
            );
        }
    }
}

/// Three short keyed endings, cycled rather than randomized. The last frame
/// is exactly identity so the cursor never leaves its target distorted.
fn settle_scale(progress: f32, variant: u32) -> egui::Vec2 {
    let p = progress.clamp(0.0, 1.0);
    let peak = match variant % 3 {
        0 => egui::vec2(1.08, 0.95),
        1 => egui::vec2(0.96, 1.09),
        _ => egui::vec2(1.07, 1.07),
    };
    let recoil = match variant % 3 {
        0 => egui::vec2(0.985, 1.025),
        1 => egui::vec2(1.025, 0.985),
        _ => egui::vec2(0.985, 0.985),
    };
    if p < 0.34 {
        egui::lerp(egui::Vec2::ONE..=peak, p / 0.34)
    } else if p < 0.72 {
        egui::lerp(peak..=recoil, (p - 0.34) / 0.38)
    } else {
        egui::lerp(recoil..=egui::Vec2::ONE, (p - 0.72) / 0.28)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_accelerates_toward_a_new_address() {
        let mut position = egui::Pos2::ZERO;
        let mut velocity = egui::Vec2::ZERO;
        let target = egui::pos2(200.0, 0.0);
        spring_vec2(&mut position, &mut velocity, target, 1.0 / 120.0);
        let first_speed = velocity.x;
        spring_vec2(&mut position, &mut velocity, target, 1.0 / 120.0);
        assert!(position.x > 0.0);
        assert!(velocity.x > first_speed, "the cursor never accelerated");
    }

    #[test]
    fn contexts_are_distinct_shapes_in_one_grammar() {
        let row = Style::for_kind(Kind::Row);
        let column = Style::for_kind(Kind::Column);
        let instrument = Style::for_kind(Kind::Instrument);
        assert!(row.side_ticks > row.cross);
        assert!(column.spine > row.spine);
        assert!(instrument.cross > column.cross);
    }

    #[test]
    fn every_keyed_settle_returns_to_the_exact_target_shape() {
        for variant in 0..12 {
            assert_eq!(settle_scale(0.0, variant), egui::Vec2::ONE);
            assert_eq!(settle_scale(1.0, variant), egui::Vec2::ONE);
            assert_ne!(settle_scale(0.34, variant), egui::Vec2::ONE);
        }
    }
}
