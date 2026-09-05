//! One keyboard cursor for the whole application.
//!
//! Interactive surfaces report the rectangle currently addressed by the
//! keyboard.  A single overlay owns the visible mark, its motion and its
//! shape, so moving between views cannot leave one static cursor behind while
//! another appears at the destination.

use eframe::egui;

// Stiffer than it was (285 / 25): the mark gets going faster and lands
// sooner; damping scaled with the square root of the spring, so the
// overshoot keeps the same character.
const SPRING: f32 = 560.0;
const DAMPING: f32 = 35.0;
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

/// What the mark should BECOME while it stands here.
///
/// On most of the application the cursor is a cursor: four corners and
/// some ticks. On a console card it can be the instrument it is standing
/// on — the compressor's gain reduction squeezing its arms, the gate's
/// aperture closing across it, the band's own hue on its corners, the
/// modulator's sweep sliding a tick along its edge. Every one of these
/// carries a MEASURED quantity: what the section reported of itself in
/// the audio callback, or where the transport is. Nothing here breathes
/// on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signature {
    /// A cursor. Four corners, some ticks, nothing else.
    Plain,
    /// Gain reduction, 0 (none) to 1 (all of it). The arms pull in and
    /// the mark itself is compressed, so the cursor is being squashed by
    /// exactly what is squashing the sound.
    Squeeze(f32),
    /// A level, 0..1. It fills the mark's foot.
    Level(f32),
    /// A band: its hue, and how far it is boosted or cut, −1..1. The
    /// corners take the hue and the mark leans up or down.
    Band { ink: egui::Color32, amount: f32 },
    /// A modulator's position, −1..1. A tick rides the mark's edges.
    Sweep(f32),
    /// How far open, 0 (shut) to 1. Two lids close across the mark.
    Aperture(f32),
    /// A place on a grid of cells: which one, of how many. The mark's
    /// foot becomes the grid, and the cell the transport is inside is
    /// the one that is lit.
    Beat { cell: usize, of: usize },
}

impl Signature {
    /// The scalar this signature eases, so a figure that arrives once a
    /// block does not make the mark flicker at the frame rate.
    fn amount(self) -> f32 {
        match self {
            Signature::Plain | Signature::Beat { .. } => 0.0,
            Signature::Squeeze(v)
            | Signature::Level(v)
            | Signature::Sweep(v)
            | Signature::Aperture(v)
            | Signature::Band { amount: v, .. } => v,
        }
    }

    /// The same signature carrying `amount` instead of its own.
    fn with(self, amount: f32) -> Self {
        match self {
            Signature::Squeeze(_) => Signature::Squeeze(amount),
            Signature::Level(_) => Signature::Level(amount),
            Signature::Sweep(_) => Signature::Sweep(amount),
            Signature::Aperture(_) => Signature::Aperture(amount),
            Signature::Band { ink, .. } => Signature::Band { ink, amount },
            other => other,
        }
    }

    /// Two signatures are the same INSTRUMENT when they are the same
    /// variant, whatever they currently read.
    fn same_instrument(self, other: Self) -> bool {
        core::mem::discriminant(&self) == core::mem::discriminant(&other)
    }
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
    signature: Signature,
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
    claim_signed(painter, id, rect, kind, layer, ink, Signature::Plain);
}

/// The same, but the mark takes the shape of what it is standing on.
///
/// A surface that has a measured figure for the thing under the cursor
/// hands it over here, and the mark wears it for as long as it stands
/// there. See [`Signature`].
#[allow(clippy::too_many_arguments)]
pub fn claim_signed(
    painter: &egui::Painter,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: egui::Rect,
    kind: Kind,
    layer: Layer,
    ink: egui::Color32,
    signature: Signature,
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
        signature,
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
    /// The instrument the mark is wearing, and the figure it reads.
    /// The figure is eased so a value that arrives once an audio block
    /// does not make the mark flicker at the frame rate.
    signature: Signature,
    /// The launch. Set on every redirect and let go of, so the mark
    /// gathers itself before the spring throws it — the one keyed
    /// anticipation, and the reason a hop reads as a jump rather than
    /// as a slide.
    wind: f32,
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
            signature: Signature::Plain,
            wind: 0.0,
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
    signature: Signature,
    wind: f32,
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
            self.signature = target.signature;
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
                self.wind = 1.0;
            }
            self.target_id = Some(target.id);
            self.target_kind = target.kind;
            self.destination = target.rect;
            // The ink walks rather than cuts, so crossing between two
            // surfaces that carry different inks is one mark changing
            // colour instead of two marks.
            self.ink = approach_ink(self.ink, target.ink, 14.0, dt);
            // A figure that arrives once an audio block is eased to the
            // frame rate; a change of INSTRUMENT is not eased, because
            // the mark has become a different thing.
            self.signature = if self.signature.same_instrument(target.signature) {
                target.signature.with(approach(
                    self.signature.amount(),
                    target.signature.amount(),
                    26.0,
                    dt,
                ))
            } else {
                target.signature
            };
        }
        self.wind = approach(self.wind, 0.0, 21.0, dt);

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
            || self.wind > 0.01
            || error > 0.2
            || speed > 0.2
            || self.signature != Signature::Plain;
        (self.visual(), repaint)
    }

    fn visual(&self) -> Visual {
        // At rest the mark lands on whole pixels: a hairline that has
        // stopped moving should be one crisp line, not two grey ones.
        let rect = egui::Rect::from_center_size(self.center, self.size);
        let rect = if self.moving || self.settle.is_some() {
            rect
        } else {
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x.round(), rect.min.y.round()),
                egui::pos2(rect.max.x.round(), rect.max.y.round()),
            )
        };
        Visual {
            rect,
            style: self.style,
            opacity: self.opacity,
            velocity: self.center_velocity,
            settle: self.settle.map(|time| (time / SETTLE_SECONDS, self.hops)),
            ink: self.ink,
            signature: self.signature,
            wind: self.wind,
        }
    }
}

fn approach(value: f32, target: f32, speed: f32, dt: f32) -> f32 {
    egui::lerp(value..=target, 1.0 - (-speed * dt).exp())
}

/// The same, channel by channel, so the mark's ink walks between two
/// surfaces rather than cutting.
fn approach_ink(from: egui::Color32, to: egui::Color32, speed: f32, dt: f32) -> egui::Color32 {
    let amount = 1.0 - (-speed * dt).exp();
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    egui::Color32::from_rgba_unmultiplied(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
        channel(from.a(), to.a()),
    )
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
    // The launch: the mark gathers itself the instant it is redirected
    // and lets go as it travels. Keyed off the hop, not off a clock.
    if visual.wind > 0.001 {
        rect =
            egui::Rect::from_center_size(rect.center(), rect.size() * (1.0 - 0.055 * visual.wind));
    }
    // Squash and stretch, from the mark's OWN velocity: it lengthens
    // along the way it is going and narrows across it, and is exactly
    // square the moment it stops. The stretch is the motion, not a
    // decoration applied to it.
    let speed = visual.velocity.length();
    if speed > 24.0 {
        let forward = visual.velocity / speed;
        let reach = (speed / 1500.0).clamp(0.0, 0.14);
        let (ax, ay) = (forward.x.abs(), forward.y.abs());
        rect = egui::Rect::from_center_size(
            rect.center(),
            rect.size()
                * egui::vec2(
                    1.0 + reach * (ax - 0.55 * ay),
                    1.0 + reach * (ay - 0.55 * ax),
                ),
        );
    }
    // A compressor squeezes the mark exactly as far as it is squeezing
    // the sound: the frame draws in and the arms shorten.
    let squeeze = match visual.signature {
        Signature::Squeeze(amount) => amount.clamp(0.0, 1.0),
        _ => 0.0,
    };
    if squeeze > 0.001 {
        let short = rect.width().min(rect.height()).max(2.0);
        rect = rect.shrink(short * 0.11 * squeeze);
    }

    // The mark's strength is the opacity it has faded to, times the
    // ENERGY the preferences ask of it — a cursor somebody has turned
    // down is turned down everywhere, signature included.
    let strength = (visual.opacity * energy.min(1.0)).clamp(0.0, 1.0);
    let ink = visual.ink.gamma_multiply(strength);
    // A band's mark wears the band's own hue, so the cursor standing on
    // the low shelf is the same colour as the low shelf.
    let corner_ink = match visual.signature {
        Signature::Band { ink: hue, .. } => hue.gamma_multiply(strength),
        _ => ink,
    };
    let stroke = egui::Stroke::new((1.5 + punch) * energy, corner_ink);
    let short = rect.width().min(rect.height()).max(2.0);
    let arm = visual.style.arm * (1.0 - 0.45 * squeeze);
    let arm_x = (rect.width() * arm).clamp(3.0, 18.0);
    let arm_y = (rect.height() * arm).clamp(3.0, 18.0);
    let cut = (short * 0.16 * visual.style.cut).clamp(0.0, 4.0);
    // A band leans: the corners on the side it is boosting toward reach
    // further, so a boost and a cut are told apart before either number
    // is read.
    let lean = match visual.signature {
        Signature::Band { amount, .. } => amount.clamp(-1.0, 1.0),
        _ => 0.0,
    };
    for (corner, sx, sy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.right_bottom(), -1.0, -1.0),
        (rect.left_bottom(), 1.0, -1.0),
    ] {
        // Up for a boost, down for a cut: the two corners the lean
        // favours grow, the other two give way.
        let favour = 1.0 + lean * sy * 0.5;
        painter.add(egui::Shape::line(
            vec![
                corner + egui::vec2(0.0, sy * arm_y * favour),
                corner + egui::vec2(sx * cut, sy * cut),
                corner + egui::vec2(sx * arm_x * favour, 0.0),
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

    signature_marks(painter, rect, visual, ink);

    // Velocity becomes two short fins behind the moving mark. They are the
    // only motion-specific geometry and disappear completely at rest.
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

/// What the mark wears while it stands on a measured instrument.
///
/// Everything here is a quantity the engine reported or the transport
/// knows. Standing still on a card whose section is doing nothing, all
/// of it is inert; standing on one that is working, the mark works with
/// it — which is the point: the cursor stops being a pointer at a
/// control and becomes a reading of it.
fn signature_marks(painter: &egui::Painter, rect: egui::Rect, visual: Visual, ink: egui::Color32) {
    let short = rect.width().min(rect.height()).max(2.0);
    match visual.signature {
        Signature::Plain | Signature::Band { .. } | Signature::Squeeze(_) => {}

        // A level fills the mark's foot, left to right.
        Signature::Level(amount) => {
            let amount = amount.clamp(0.0, 1.0);
            let y = rect.bottom() + 2.0;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0, ink.gamma_multiply(0.28)),
            );
            if amount > 0.004 {
                painter.line_segment(
                    [
                        egui::pos2(rect.left(), y),
                        egui::pos2(egui::lerp(rect.x_range(), amount), y),
                    ],
                    egui::Stroke::new(2.0, ink),
                );
            }
        }

        // A modulator rides the mark: one tick on each long edge, at the
        // sweep's own position, so the cursor breathes with the LFO that
        // is actually running.
        Signature::Sweep(at) => {
            let at = ((at.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
            let x = egui::lerp(rect.x_range(), at);
            let reach = (short * 0.16).clamp(2.5, 6.0);
            for y in [rect.top(), rect.bottom()] {
                painter.line_segment(
                    [egui::pos2(x, y - reach), egui::pos2(x, y + reach)],
                    egui::Stroke::new(1.5, ink),
                );
            }
        }

        // An aperture closes across the mark: two lids meeting in the
        // middle, as far shut as the gate is shut.
        Signature::Aperture(open) => {
            let open = open.clamp(0.0, 1.0);
            let half = rect.height() * 0.5 * (1.0 - open);
            let lid = ink.gamma_multiply(0.34 + 0.5 * (1.0 - open));
            for (from, to) in [
                (rect.top(), rect.top() + half),
                (rect.bottom() - half, rect.bottom()),
            ] {
                if to - from > 0.5 {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), from),
                            egui::pos2(rect.right(), to),
                        ),
                        0.0,
                        lid,
                    );
                }
            }
        }

        // A place on a grid: the mark's foot becomes the grid, and the
        // cell the transport is inside is the lit one.
        Signature::Beat { cell, of } => {
            if of == 0 {
                return;
            }
            let y = rect.bottom() + 3.0;
            let pitch = rect.width() / of as f32;
            for i in 0..of {
                let here = i == cell % of;
                painter.line_segment(
                    [
                        egui::pos2(rect.left() + pitch * i as f32 + 1.0, y),
                        egui::pos2(rect.left() + pitch * (i + 1) as f32 - 1.0, y),
                    ],
                    egui::Stroke::new(
                        if here { 2.0 } else { 1.0 },
                        if here { ink } else { ink.gamma_multiply(0.3) },
                    ),
                );
            }
        }
    }
}

/// Five short keyed endings, cycled rather than randomized. The last frame
/// is exactly identity so the cursor never leaves its target distorted.
fn settle_scale(progress: f32, variant: u32) -> egui::Vec2 {
    let p = progress.clamp(0.0, 1.0);
    let peak = match variant % 5 {
        0 => egui::vec2(1.08, 0.95),
        1 => egui::vec2(0.96, 1.09),
        2 => egui::vec2(1.07, 1.07),
        3 => egui::vec2(1.05, 0.99),
        _ => egui::vec2(0.99, 1.05),
    };
    let recoil = match variant % 5 {
        0 => egui::vec2(0.985, 1.025),
        1 => egui::vec2(1.025, 0.985),
        2 => egui::vec2(0.985, 0.985),
        3 => egui::vec2(0.992, 1.012),
        _ => egui::vec2(1.012, 0.992),
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

    /// A signature carries a figure, and the figure is what eases. A
    /// change of INSTRUMENT is not eased: the mark has become a
    /// different thing, and sliding between two different things would
    /// say something untrue about both.
    #[test]
    fn a_signature_eases_its_figure_but_not_its_instrument() {
        let squeeze = Signature::Squeeze(0.2);
        assert_eq!(squeeze.amount(), 0.2);
        assert_eq!(squeeze.with(0.9), Signature::Squeeze(0.9));
        assert!(squeeze.same_instrument(Signature::Squeeze(0.9)));
        assert!(!squeeze.same_instrument(Signature::Aperture(0.2)));
        assert!(!squeeze.same_instrument(Signature::Plain));
        // A band keeps its hue while its figure walks.
        let band = Signature::Band {
            ink: egui::Color32::RED,
            amount: -0.5,
        };
        assert_eq!(band.amount(), -0.5);
        assert_eq!(
            band.with(0.25),
            Signature::Band {
                ink: egui::Color32::RED,
                amount: 0.25
            }
        );
        // The two that carry no figure ease nothing.
        assert_eq!(Signature::Plain.amount(), 0.0);
        assert_eq!(Signature::Beat { cell: 2, of: 4 }.amount(), 0.0);
        assert_eq!(
            Signature::Beat { cell: 2, of: 4 }.with(0.9),
            Signature::Beat { cell: 2, of: 4 }
        );
    }

    /// The plain mark is exactly what it always was: a signature is
    /// something a surface OPTS INTO, never something that happens to a
    /// cursor that did not ask.
    #[test]
    fn a_cursor_that_asked_for_nothing_wears_nothing() {
        let mut motion = Motion::default();
        let target = Target {
            id: egui::Id::new("a"),
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 20.0)),
            kind: Kind::Cell,
            layer: Layer::Surface,
            ink: egui::Color32::WHITE,
            signature: Signature::Plain,
        };
        let (visual, _) = motion.advance(Some(target), 1.0 / 120.0, Settings::default());
        assert_eq!(visual.signature, Signature::Plain);
        assert_eq!(visual.wind, 0.0, "nothing was redirected, nothing wound up");
    }

    /// A hop winds the mark up and lets it go: the launch is keyed off
    /// the redirect itself, so it cannot happen while the mark is still.
    #[test]
    fn a_redirect_winds_the_mark_up_and_it_lets_go() {
        let mut motion = Motion::default();
        let here = |x: f32, id: &'static str| Target {
            id: egui::Id::new(id),
            rect: egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(40.0, 20.0)),
            kind: Kind::Cell,
            layer: Layer::Surface,
            ink: egui::Color32::WHITE,
            signature: Signature::Plain,
        };
        motion.advance(Some(here(0.0, "a")), 1.0 / 120.0, Settings::default());
        let (wound, _) = motion.advance(Some(here(300.0, "b")), 1.0 / 120.0, Settings::default());
        assert!(wound.wind > 0.5, "the hop did not wind the mark up");
        let mut last = wound.wind;
        for _ in 0..40 {
            let (visual, _) =
                motion.advance(Some(here(300.0, "b")), 1.0 / 120.0, Settings::default());
            assert!(
                visual.wind <= last + 1e-6,
                "the launch grew while travelling"
            );
            last = visual.wind;
        }
        assert!(last < 0.05, "the mark never let go: {last}");
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
