//! The house's ornament: character carried in FORM.
//!
//! The alphabet caps what may be transmitted — five rungs of value, two
//! hues, a budget for how loudly a screen speaks. It says nothing about
//! SHAPE. A plane with a cut corner and a plain rectangle at the same rung
//! transmit exactly the same signal; one of them just looks like it was
//! made by somebody. So everything here works in outline and never in
//! value or hue, and the code's argument is untouched by it.
//!
//! One rule holds that up:
//!
//! > **Ornament may not vary with state unless it IS the state.**
//!
//! Constant character is free. Character that flickers as the song
//! changes is a channel nobody budgeted for, and the eye will read it as
//! meaning something whether or not it does.
//!
//! The forms here are not invented from nothing. The corner bracket is
//! already this app's cursor inside the step grid — "four corners, and
//! nothing joins them, so the cursor brackets a cell without boxing it" —
//! and this makes that the house's mark rather than one editor's habit.

use eframe::egui;

/// How much of a corner the cut takes. Small enough to read as a bevel on
/// a made object rather than as a shape in its own right.
pub const CUT: f32 = 6.0;

/// Which corner a plane gives up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cut {
    /// The band's left shoulder.
    TopLeft,
    /// The band's right shoulder.
    TopRight,
    /// The trailing edge, for a thing sound leaves by.
    BottomRight,
}

/// A filled plane, with one corner cut away or none.
///
/// The cut is the same size everywhere and never varies with anything a
/// performer changes: a head is cut the way it is cut whether it is
/// focused, playing or empty. `None` is an ordinary rectangle, which is
/// what a plane in the MIDDLE of a run of them wants — a shoulder only
/// means something at a shoulder.
pub fn plane(painter: &egui::Painter, rect: egui::Rect, fill: egui::Color32, cut: Option<Cut>) {
    let Some(cut) = cut.filter(|_| rect.width() > CUT * 2.0 && rect.height() > CUT * 2.0) else {
        painter.rect_filled(rect, 0.0, fill);
        return;
    };
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    let points = match cut {
        Cut::TopLeft => vec![
            egui::pos2(l + CUT, t),
            egui::pos2(r, t),
            egui::pos2(r, b),
            egui::pos2(l, b),
            egui::pos2(l, t + CUT),
        ],
        Cut::TopRight => vec![
            egui::pos2(l, t),
            egui::pos2(r - CUT, t),
            egui::pos2(r, t + CUT),
            egui::pos2(r, b),
            egui::pos2(l, b),
        ],
        Cut::BottomRight => vec![
            egui::pos2(l, t),
            egui::pos2(r, t),
            egui::pos2(r, b - CUT),
            egui::pos2(r - CUT, b),
            egui::pos2(l, b),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        fill,
        egui::Stroke::NONE,
    ));
}

/// Four corner marks, and nothing joining them.
///
/// Bounds a rectangle without boxing it — the eye completes the frame,
/// which is both less ink than an outline and the reason it reads as
/// apparatus rather than as a border.
pub fn brackets(
    painter: &egui::Painter,
    rect: egui::Rect,
    ink: egui::Color32,
    weight: f32,
    cap: f32,
) {
    let cap = cap.min(rect.width() * 0.4).min(rect.height() * 0.4);
    if cap <= 0.0 {
        return;
    }
    let stroke = egui::Stroke::new(weight, ink);
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    for (from, to) in [
        // top left
        (egui::pos2(l, t), egui::pos2(l + cap, t)),
        (egui::pos2(l, t), egui::pos2(l, t + cap)),
        // top right
        (egui::pos2(r - cap, t), egui::pos2(r, t)),
        (egui::pos2(r, t), egui::pos2(r, t + cap)),
        // bottom left
        (egui::pos2(l, b - cap), egui::pos2(l, b)),
        (egui::pos2(l, b), egui::pos2(l + cap, b)),
        // bottom right
        (egui::pos2(r - cap, b), egui::pos2(r, b)),
        (egui::pos2(r, b - cap), egui::pos2(r, b)),
    ] {
        painter.line_segment([from, to], stroke);
    }
}

/// The ground's own texture: a field of points on a fixed pitch.
///
/// The quietest thing on the surface and the only one that is purely
/// material — it says nothing, which is exactly why it may cover
/// everything. It also states what the app IS: a lattice, with the
/// session, the mixer and the chain all laid on the same rank and file.
pub fn lattice(painter: &egui::Painter, area: egui::Rect, ink: egui::Color32, pitch: f32) {
    if pitch <= 1.0 || !area.is_positive() {
        return;
    }
    let size = egui::Vec2::splat(1.0);
    // Anchored to the AREA rather than to the window, so the field does
    // not crawl under a resize.
    let mut y = area.min.y + pitch;
    while y < area.max.y {
        let mut x = area.min.x + pitch;
        while x < area.max.x {
            painter.rect_filled(
                egui::Rect::from_center_size(egui::pos2(x, y), size),
                0.0,
                ink,
            );
            x += pitch;
        }
        y += pitch;
    }
}

/// A row of graduations along one edge, longest at the ends.
///
/// What separates an instrument from a diagram of one: a scale you can
/// read a value off without a number beside it.
pub fn graduations(
    painter: &egui::Painter,
    rail: egui::Rect,
    at: &[f32],
    ink: egui::Color32,
    length: f32,
) {
    for place in at {
        let y = rail.max.y - rail.height() * place.clamp(0.0, 1.0);
        painter.line_segment(
            [
                egui::pos2(rail.min.x, y),
                egui::pos2(rail.min.x + length, y),
            ],
            egui::Stroke::new(1.0, ink),
        );
    }
}

/// A run of leader dots between two things that belong together.
///
/// The oldest fix for the oldest problem in a list: a name at one edge
/// and its value at the other, with a gap the eye has to cross without
/// losing the row.
pub fn leaders(
    painter: &egui::Painter,
    from: f32,
    to: f32,
    y: f32,
    ink: egui::Color32,
    pitch: f32,
) {
    if pitch <= 0.0 || to - from < pitch {
        return;
    }
    let size = egui::Vec2::splat(1.0);
    let mut x = from + pitch;
    while x < to {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y), size),
            0.0,
            ink,
        );
        x += pitch;
    }
}

/// The maker's mark.
///
/// A ring, a stile through it, and a notch off one shoulder — drawn from
/// the same parts everything else here is: a circle, a line, a cut. The
/// one piece of pure ornament in the app, and it earns its place the way
/// a badge on an instrument does.
pub fn sigil(painter: &egui::Painter, centre: egui::Pos2, radius: f32, ink: egui::Color32) {
    if radius <= 1.0 {
        return;
    }
    let stroke = egui::Stroke::new(1.0, ink);
    painter.circle_stroke(centre, radius, stroke);
    painter.line_segment(
        [
            egui::pos2(centre.x, centre.y - radius),
            egui::pos2(centre.x, centre.y + radius),
        ],
        stroke,
    );
    // The notch: a short bar off the ring's shoulder, at the same angle
    // every cut plane gives up its corner.
    let shoulder = radius * std::f32::consts::FRAC_1_SQRT_2;
    painter.line_segment(
        [
            egui::pos2(centre.x + shoulder, centre.y - shoulder),
            egui::pos2(centre.x + shoulder + radius * 0.6, centre.y - shoulder),
        ],
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(w, h))
    }

    #[test]
    fn a_bracket_never_grows_past_the_thing_it_bounds() {
        // Four corners that met in the middle would be an outline, and an
        // outline is the thing this form exists instead of.
        for (w, h) in [(100.0, 40.0), (8.0, 8.0), (40.0, 4.0)] {
            let bounds = rect(w, h);
            let cap = 100.0f32
                .min(bounds.width() * 0.4)
                .min(bounds.height() * 0.4);
            assert!(cap * 2.0 <= bounds.width() + 0.01);
            assert!(cap * 2.0 <= bounds.height() + 0.01);
        }
    }

    #[test]
    fn a_lattice_is_anchored_to_its_area_and_stays_inside_it() {
        // Points on a fixed pitch, none of them on or past the edge —
        // a texture that touched the boundary would read as a rule.
        let area = rect(100.0, 60.0);
        let pitch = 20.0;
        let mut count = 0;
        let mut y = area.min.y + pitch;
        while y < area.max.y {
            let mut x = area.min.x + pitch;
            while x < area.max.x {
                assert!(area.contains(egui::pos2(x, y)));
                count += 1;
                x += pitch;
            }
            y += pitch;
        }
        assert_eq!(count, 4 * 2, "the field is not on the pitch it was given");
    }

    #[test]
    fn a_pitch_that_cannot_draw_draws_nothing() {
        // Guarded rather than clamped: a zero pitch is a caller mistake,
        // and an infinite loop is a worse answer than an empty field.
        let ctx = egui::Context::default();
        let painter = ctx.debug_painter();
        lattice(&painter, rect(100.0, 60.0), egui::Color32::WHITE, 0.0);
        leaders(&painter, 0.0, 100.0, 0.0, egui::Color32::WHITE, 0.0);
        sigil(&painter, egui::pos2(0.0, 0.0), 0.0, egui::Color32::WHITE);
    }

    #[test]
    fn a_plane_too_small_to_cut_keeps_its_corners() {
        // A cut that took most of a cell would stop being a bevel and
        // start being the shape.
        let ctx = egui::Context::default();
        let painter = ctx.debug_painter();
        plane(
            &painter,
            rect(4.0, 4.0),
            egui::Color32::WHITE,
            Some(Cut::TopLeft),
        );
        plane(
            &painter,
            rect(200.0, 60.0),
            egui::Color32::WHITE,
            Some(Cut::BottomRight),
        );
    }
}
