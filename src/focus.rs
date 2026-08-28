//! The keyboard cursor: what has focus, and how an arrow key moves it.
//!
//! Elements register themselves as they are drawn, so the focusable set is
//! exactly the set of things on screen this frame. `nearest` decides where
//! an arrow lands and `spring_step` carries the ring there.
//!
//! The ring is drawn as CORNER BRACKETS (`ui::hud`), the same mark a
//! widget's own focus ring uses. One thing on screen always means "the
//! keyboard is here", so it has to mean it in one shape.
//!
//! The two pure halves — the navigation model and the spring — are the
//! parts worth pinning by test, and neither needs a window.
//!
//! Lifted out of `main.rs` unchanged.

use daw::ui::theme::Theme;

/// The cursor SURROUNDS the focused element; it is not a box floating in a
/// region. Once the keyboard can reach individual buttons, a marker sitting
/// in the middle of a panel cannot say WHICH button it means.
///
/// It is drawn as four corner brackets rather than a closed rectangle —
/// see `ui::hud` for why, and `RING_RADIUS` for the one case that still
/// closes. The names here stay `RING_*`: what they describe is the mark
/// that rings an element, and renaming a constant because its ink moved
/// would rename the idea too.
pub const RING_STROKE: f32 = 2.0;
/// How far the ring stands off the element, so it surrounds rather than
/// covers it.
pub const RING_PAD: f32 = 3.0;
/// The fallback's corner radius. Square, like everything else — and the
/// fallback only happens under `hud::BRACKET_FLOOR`, where four corner
/// arms would be four dots.
pub const RING_RADIUS: f32 = 0.0;
/// Stiffness of the ring's travel, radians per second. Critically damped, so
/// it accelerates in and settles without overshoot.
pub const RING_OMEGA: f32 = 34.0;
/// Below this the spring is done and we stop asking for frames.
pub const RING_SETTLED_PX: f32 = 0.25;
/// How much a candidate is penalised for being off the axis you pressed.
/// Above 1.0 this prefers the element you are lined up with over one merely
/// closer — which is what makes arrowing along a row of buttons work.
pub const CROSS_PENALTY: f32 = 2.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    pub fn key(self) -> egui::Key {
        match self {
            Self::Up => egui::Key::ArrowUp,
            Self::Down => egui::Key::ArrowDown,
            Self::Left => egui::Key::ArrowLeft,
            Self::Right => egui::Key::ArrowRight,
        }
    }
}

/// One step of a critically damped spring, solved implicitly.
///
/// Implicit rather than the obvious `vel += accel * dt`, because the explicit
/// form blows up when a frame runs long — exactly when a dropped frame would
/// otherwise fling the cursor off screen. This form is unconditionally
/// stable at any `dt`, and being critically damped it never overshoots, so
/// the box arrives without a wobble.
///
/// Pure, so `the_cursor_springs_without_overshooting` can check it with no
/// window in sight.
pub fn spring_step(
    pos: egui::Vec2,
    vel: egui::Vec2,
    target: egui::Vec2,
    dt: f32,
) -> (egui::Vec2, egui::Vec2) {
    let omega = RING_OMEGA;
    let f = 1.0 + 2.0 * dt * omega;
    let oo = omega * omega;
    let hoo = dt * oo;
    let hhoo = dt * hoo;
    let det_inv = 1.0 / (f + hhoo);
    let det_x = f * pos + dt * vel + hhoo * target;
    let det_v = vel + hoo * (target - pos);
    (det_x * det_inv, det_v * det_inv)
}

/// Which element the keyboard is on, and where the ring is on its way there.
///
/// Elements register themselves as they are drawn, so the focusable set is
/// exactly the set of things that exist this frame. A control that is not
/// drawn cannot be focused, and one that is drawn cannot be missed.
#[derive(Default)]
pub struct Focus {
    pub at: Option<egui::Id>,
    pub items: Vec<(egui::Id, egui::Rect)>,
    /// The direction pressed this frame, resolved only once every element has
    /// registered — moving mid-frame would navigate a half-built list.
    pub pending: Option<Dir>,
    pub activate: bool,
    pub ring: Option<egui::Rect>,
    pub v_min: egui::Vec2,
    pub v_max: egui::Vec2,
}

impl Focus {
    /// Take this frame's keyboard input and start collecting elements.
    pub fn begin(&mut self, ctx: &egui::Context) {
        self.items.clear();
        self.pending = None;
        self.activate = false;

        // A focused text field owns the keyboard outright. Escape hands it
        // back — without that you would be stuck inside the search box.
        if ctx.egui_wants_keyboard_input() {
            let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if escape && let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            return;
        }
        ctx.input_mut(|i| {
            for dir in Dir::ALL {
                if i.consume_key(egui::Modifiers::NONE, dir.key()) {
                    self.pending = Some(dir);
                }
            }
            self.activate = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        });
    }

    /// Declare an element focusable. Returns true if the keyboard is on it.
    pub fn register(&mut self, id: egui::Id, rect: egui::Rect) -> bool {
        self.items.push((id, rect));
        self.at == Some(id)
    }

    /// Put the keyboard on `id` because the POINTER went there.
    ///
    /// Focus otherwise only moves by arrow key, which left the roll — or
    /// any other region you can work in with the mouse — visibly the thing
    /// being edited while its verbs went to whatever the ring was last
    /// parked on. Claiming beats a pending arrow: the hand that just
    /// pressed a button is more recent than the key that started the frame.
    pub fn claim(&mut self, id: egui::Id) {
        if self.at != Some(id) {
            self.at = Some(id);
            self.pending = None;
        }
    }

    /// Focused AND the user pressed Enter.
    pub fn activated(&self, id: egui::Id) -> bool {
        self.activate && self.at == Some(id)
    }

    /// Resolve movement, then ease the ring toward the focused element.
    pub fn end(&mut self, ui: &egui::Ui, theme: &Theme) {
        if self.items.is_empty() {
            return;
        }
        // Land somewhere on the first frame, and recover if whatever was
        // focused stopped being drawn — a folder collapsing under it, say.
        if !self.items.iter().any(|(id, _)| Some(*id) == self.at) {
            self.at = self.items.first().map(|(id, _)| *id);
            self.ring = None;
        }
        if let (Some(dir), Some(from)) = (self.pending, self.rect_of(self.at))
            && let Some(next) = nearest(from, dir, &self.items, self.at)
        {
            self.at = Some(next);
        }

        let Some(target) = self.rect_of(self.at) else {
            return;
        };
        let target = target.expand(RING_PAD);
        let ring = match self.ring {
            None => target,
            Some(ring) => {
                let dt = ui.ctx().input(|i| i.stable_dt);
                let (min, v_min) =
                    spring_step(ring.min.to_vec2(), self.v_min, target.min.to_vec2(), dt);
                let (max, v_max) =
                    spring_step(ring.max.to_vec2(), self.v_max, target.max.to_vec2(), dt);
                let next = egui::Rect::from_min_max(min.to_pos2(), max.to_pos2());
                let settled = (next.min - target.min).length() < RING_SETTLED_PX
                    && (next.max - target.max).length() < RING_SETTLED_PX;
                if settled {
                    self.v_min = egui::Vec2::ZERO;
                    self.v_max = egui::Vec2::ZERO;
                    target
                } else {
                    self.v_min = v_min;
                    self.v_max = v_max;
                    ui.ctx().request_repaint();
                    next
                }
            }
        };
        self.ring = Some(ring);

        // CORNERS, the same mark the per-widget ring draws — see
        // `ui::hud`. Before this the app had two focus marks that
        // disagreed: the travelling cursor was a closed box and the
        // widgets' own mark was brackets, so the one thing on screen
        // that always means "the keyboard is here" meant it in two
        // shapes depending on which surface you were on.
        //
        // The spring is unchanged and carries four corners instead of a
        // rectangle, which is also the cheaper thing to watch move: a
        // box sweeping across a dense grid drags four full edges over
        // everything it passes, and corners drag eight short arms.
        let ink = egui::Stroke::new(RING_STROKE, theme.focus);
        if daw::ui::hud::is_bracketed(ring) {
            daw::ui::hud::brackets(ui.painter(), ring, ink);
        } else {
            ui.painter()
                .rect_stroke(ring, RING_RADIUS, ink, egui::StrokeKind::Middle);
        }
    }

    pub fn rect_of(&self, id: Option<egui::Id>) -> Option<egui::Rect> {
        self.items
            .iter()
            .find(|(i, _)| Some(*i) == id)
            .map(|(_, r)| *r)
    }
}

/// The element an arrow key should land on.
///
/// Candidates must lie genuinely in the pressed direction; among those the
/// winner is closest along that axis, penalised for being off it. That
/// penalty is what makes a row of buttons arrow left-to-right instead of
/// diving at whatever is nearest in a straight line.
///
/// Pure, so the whole navigation model is testable without a window.
pub fn nearest(
    from: egui::Rect,
    dir: Dir,
    items: &[(egui::Id, egui::Rect)],
    current: Option<egui::Id>,
) -> Option<egui::Id> {
    let a = from.center();
    let mut best: Option<(egui::Id, f32)> = None;
    for (id, rect) in items {
        if Some(*id) == current {
            continue;
        }
        let b = rect.center();
        let (along, across) = match dir {
            Dir::Left => (a.x - b.x, (b.y - a.y).abs()),
            Dir::Right => (b.x - a.x, (b.y - a.y).abs()),
            Dir::Up => (a.y - b.y, (b.x - a.x).abs()),
            Dir::Down => (b.y - a.y, (b.x - a.x).abs()),
        };
        if along <= 0.5 {
            continue; // not actually that way
        }
        let score = along + across * CROSS_PENALTY;
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((*id, score));
        }
    }
    best.map(|(id, _)| id)
}
