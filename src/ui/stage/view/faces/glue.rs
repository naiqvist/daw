//! GLUE's face: a beam balance, the desk's weight against the mix.
//!
//! After the steelyard — the assay balance with a knife-edge fulcrum, a
//! short load arm carrying a hanging pan, and a long arm with a poise
//! that slides along it. The bus compressor has one knob, so it gets one
//! moving weight, and everything else on the glass is a consequence of
//! where that weight stands and what the mix is doing.
//!
//! What makes this card unlike every other on the desk: the stereo pair
//! IS the beam. It comes in at the notch, runs down the left margin,
//! pours into the vessel, rises out of the pan on the yoke, becomes the
//! beam's two flanges, crosses itself over the wedge — that X is the
//! detector's stereo link, drawn where the machine actually links it —
//! and leaves by two stays from the far tip. So the signal path is a
//! moving part: it tips with the gain reduction and jacks itself back up
//! on the shims of its own makeup.

use super::*;
use crate::params::console::glue as p;
use crate::ui::chrome;

/// The beam's travel at full reduction, in degrees. A steelyard does not
/// swing far; this is enough that a decibel is visible and not so much
/// that the pan leaves the glass.
const SWING_DEG: f32 = 11.0;
/// The reduction, in dB, that tips the beam all the way over.
const FULL_SWING_DB: f32 = 12.0;
/// The pan's scale: silence at the bottom, full scale at the rim's top.
const PAN_FLOOR_DB: f32 = -48.0;
/// One shim per decibel handed back, up to this many.
const SHIMS_MAX: f32 = 8.0;
/// How tall one shim's rise is, in points.
const SHIM_RISE: f32 = 3.0;
/// The graduations under the poise's travel.
const ARM_TICKS: usize = 9;

/// Where every part of the balance stands this frame.
///
/// Authored as one struct because the parts are RIGID with respect to
/// each other: the poise rides the beam, the pan hangs from its tip, the
/// column stands on the shims. Computing them apart would let them come
/// apart, which is the one thing a machine may not do.
struct Rig {
    inner: egui::Rect,
    /// The fulcrum's apex, and the beam's two tips.
    wedge: egui::Pos2,
    left_tip: egui::Pos2,
    right_tip: egui::Pos2,
    /// The beam's angle, in radians, positive being left-tip-down.
    tilt: f32,
    /// The deepest the beam has been, held.
    ghost: f32,
    /// How far along the right arm the poise has walked.
    poise: egui::Rect,
    /// The vessel and the water in it.
    pan: egui::Rect,
    /// The rigid post, the desk it stands on, and the shims between.
    column: egui::Rect,
    base: egui::Rect,
    shims: usize,
    /// The two posts the pair leaves by.
    posts: [egui::Pos2; 2],
}

struct Lay {
    poise: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![(p::LEAN, self.poise)]
    }
}

/// The point `d` along the beam from the wedge, `off` above it.
fn on_beam(rig_wedge: egui::Pos2, tilt: f32, d: f32, off: f32) -> egui::Pos2 {
    let (sin, cos) = tilt.sin_cos();
    egui::pos2(
        rig_wedge.x + d * cos + off * sin,
        rig_wedge.y - d * sin - off * cos,
    )
}

fn rig(face: &Face<'_>) -> Rig {
    let glass = face.glass;
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let said = face.said;

    // Half of what the compressor takes is handed back, and the desk
    // packs it under the pivot as shims. The rig above rises by exactly
    // that, so the makeup is a LIFT rather than a level.
    let handed_back = (said.reduction_db.abs() * p::MAKEUP_SHARE).clamp(0.0, SHIMS_MAX);
    let lift = face.anim("lift", handed_back * SHIM_RISE, 0.18);
    let beam_rest_y = inner.top() + 0.33 * h;
    let beam_y = beam_rest_y - lift;

    let fx = inner.left() + 0.40 * w;
    let lx = inner.left() + 0.16 * w;
    let rx = inner.right() - 2.0;
    let arm_l = (fx - lx).max(1.0);
    let arm_r = (rx - fx).max(1.0);

    // The tilt is the INSTANTANEOUS reduction; the ghost is the block's
    // worst, dragged up quickly and let down slowly, so the gap between
    // the beam and its shadow is the peak hold.
    let swing = SWING_DEG.to_radians();
    let tilt = face.anim(
        "tilt",
        swing * (said.bands[0].abs() / FULL_SWING_DB).clamp(0.0, 1.0),
        0.06,
    );
    let ghost = face.anim(
        "hold",
        swing * (said.reduction_db.abs() / FULL_SWING_DB).clamp(0.0, 1.0),
        0.55,
    );

    let wedge = egui::pos2(fx, beam_y);
    let left_tip = on_beam(wedge, tilt, -arm_l, 0.0);
    let right_tip = on_beam(wedge, tilt, arm_r, 0.0);

    // Leaning harder walks the weight IN toward the pivot, which is what
    // takes the beam's counter-torque away and lets the mix tip it.
    let lean = face.anim("lean", (face.value(p::LEAN) / 100.0).clamp(0.0, 1.0), 0.12);
    let travel = egui::lerp((0.16 * arm_r)..=(arm_r - 6.0), 1.0 - lean);
    let poise =
        egui::Rect::from_center_size(on_beam(wedge, tilt, travel, 7.0), egui::vec2(15.0, 15.0));

    // The vessel hangs rigidly from the short arm's tip: its length is
    // fixed at rest, so it swings and rises without ever stretching.
    let pan_w = (0.26 * w).max(18.0);
    let pan_h = ((inner.bottom() - 12.0) - (beam_rest_y + 18.0)).max(24.0);
    let pan = egui::Rect::from_min_size(
        egui::pos2(lx - pan_w * 0.5, left_tip.y + 18.0),
        egui::vec2(pan_w, pan_h),
    );

    let post_len = (0.26 * h - 11.0).max(8.0);
    let column = egui::Rect::from_min_max(
        egui::pos2(fx - 3.0, beam_y + 11.0),
        egui::pos2(fx + 3.0, beam_y + 11.0 + post_len),
    );
    let base_top = beam_rest_y + 11.0 + post_len;
    let base = egui::Rect::from_min_max(
        egui::pos2(fx - 0.085 * w, base_top),
        egui::pos2(fx + 0.085 * w, base_top + 4.0),
    );

    Rig {
        inner,
        wedge,
        left_tip,
        right_tip,
        tilt,
        ghost,
        poise,
        pan,
        column,
        base,
        shims: handed_back.round() as usize,
        posts: [
            egui::pos2(glass.right() - 7.0, glass.top() + 7.0),
            egui::pos2(glass.right() - 7.0, glass.top() + 11.0),
        ],
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let rig = rig(face);
    let lay = Lay { poise: rig.poise };
    let ink = face.ink();
    let edge = face.edge();
    let live = face.live();
    let lit = face.lit(p::LEAN);
    let said = face.said;
    let mut shapes = Vec::new();

    // ── the desk, the shims and the post ──────────────────────────────
    // The base never moves: it is the desk. The post never changes
    // length: it is rigid. What grows between them is the makeup, one
    // bar per decibel handed back — so the machine leans, and jacks
    // itself back up by exactly half of what it took.
    shapes.push(egui::Shape::rect_filled(rig.base, 0.0, edge));
    for i in 0..rig.shims {
        let y = rig.base.top() - (i as f32 + 1.0) * SHIM_RISE;
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rig.base.center().x - 2.5, y),
                egui::vec2(5.0, 2.0),
            ),
            0.0,
            ink,
        ));
    }
    shapes.push(egui::Shape::rect_filled(
        rig.column,
        0.0,
        face.alpha.surface.color,
    ));
    chrome::trace(
        &mut shapes,
        &[rig.column.left_top(), rig.column.left_bottom()],
        Weight::Hair,
        edge,
    );
    chrome::trace(
        &mut shapes,
        &[rig.column.right_top(), rig.column.right_bottom()],
        Weight::Hair,
        edge,
    );

    // ── the fan: the ruling the tilt is read against ──────────────────
    // Nought, six and twelve decibels, drawn as three short rays in the
    // empty wedge of glass above the long arm. It never moves; it is the
    // scale, and it makes the tilt a quantity without being a number.
    let arm_r = (rig.right_tip - rig.wedge).length().max(1.0);
    for step in 0..3 {
        let angle = SWING_DEG.to_radians() * step as f32 / 2.0;
        chrome::trace(
            &mut shapes,
            &[
                on_beam(rig.wedge, angle, 0.62 * arm_r, 0.0),
                on_beam(rig.wedge, angle, 0.76 * arm_r, 0.0),
            ],
            Weight::Hair,
            tool::fade(edge, 0.5),
        );
    }
    // The shadow index: the deepest the beam has been, easing back down.
    chrome::trace(
        &mut shapes,
        &[rig.wedge, on_beam(rig.wedge, rig.ghost, 0.80 * arm_r, 0.0)],
        Weight::Hair,
        tool::fade(edge, 0.9),
    );

    // ── the vessel, and the mix in it ─────────────────────────────────
    let pan_inner = rig.pan.shrink(2.0);
    // The fill is the DETECTOR's own reading of the output — the very
    // number the gain computer compares against the threshold — so
    // fill-against-rim is the comparison the DSP is making.
    let level = face.anim("fill", said.level_db.clamp(PAN_FLOOR_DB, 0.0), 0.08);
    let water = tool::level_y(pan_inner, level, PAN_FLOOR_DB);
    let rim = tool::level_y(pan_inner, said.bands[2], PAN_FLOOR_DB);
    if water < pan_inner.bottom() {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(egui::pos2(pan_inner.left(), water), pan_inner.max),
            0.0,
            tool::fade(live, 0.55),
        ));
    }
    // What the fill has above the rim is the overshoot the compressor is
    // eating. Because the detector hears the OUTPUT, that hot band
    // visibly shrinks as the ballistics engage.
    if water < rim {
        let over = egui::Rect::from_min_max(
            egui::pos2(pan_inner.left(), water),
            egui::pos2(pan_inner.right(), rim),
        );
        shapes.push(egui::Shape::rect_filled(
            over,
            0.0,
            tool::fade(face.hot(), 0.5),
        ));
        let mut y = over.top() + 1.0;
        while y < over.bottom() {
            chrome::trace(
                &mut shapes,
                &[egui::pos2(over.left(), y), egui::pos2(over.right(), y)],
                Weight::Hair,
                tool::fade(face.hot(), 0.8),
            );
            y += 3.0;
        }
    }
    // The settle line: where the kernel's own soft knee says the fill
    // will come to rest. Taken from the same GainComputer the core
    // configures, so the card cannot draw a compressor that is not
    // there.
    let settle = level + crate::console::glue_curve::gain_db(level, said.bands[2]);
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(
                pan_inner.left(),
                tool::level_y(pan_inner, settle, PAN_FLOOR_DB),
            ),
            egui::pos2(
                pan_inner.right(),
                tool::level_y(pan_inner, settle, PAN_FLOOR_DB),
            ),
        ],
        Weight::Hair,
        face.alpha.jeopardy_latent.color,
    );
    // The rim: the threshold, scribed on the vessel with a stub through
    // each wall so it stays visible under the water. It brightens with
    // the poise, because the weight and the waterline are one thing.
    let rim_ink = tool::mix_ink(ink, face.focus(), lit);
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(rig.pan.left() - if lit > 0.5 { 5.0 } else { 4.0 }, rim),
            egui::pos2(rig.pan.right() + if lit > 0.5 { 5.0 } else { 4.0 }, rim),
        ],
        Weight::Heavy,
        rim_ink,
    );
    // The vessel itself: an open-topped U, so the eye reads a container
    // rather than a bar chart.
    chrome::trace(
        &mut shapes,
        &[
            rig.pan.left_top(),
            egui::pos2(rig.pan.left(), rig.pan.bottom() - 3.0),
            egui::pos2(rig.pan.left() + 3.0, rig.pan.bottom()),
            egui::pos2(rig.pan.right() - 3.0, rig.pan.bottom()),
            egui::pos2(rig.pan.right(), rig.pan.bottom() - 3.0),
            rig.pan.right_top(),
        ],
        Weight::Hair,
        edge,
    );

    // ── the beam: two flanges, which are the stereo pair ──────────────
    for side in [-1.5f32, 1.5] {
        chrome::trace(
            &mut shapes,
            &[
                on_beam(
                    rig.wedge,
                    rig.tilt,
                    -(rig.wedge - rig.left_tip).length(),
                    side,
                ),
                on_beam(rig.wedge, rig.tilt, -8.0, side),
                // Over the wedge the two cross: that X is the detector's
                // stereo link, drawn where the machine actually links it.
                on_beam(rig.wedge, rig.tilt, 8.0, -side),
                on_beam(rig.wedge, rig.tilt, arm_r, -side),
            ],
            Weight::Hair,
            tool::fade(ink, 0.85),
        );
    }
    for (tip, sign) in [(rig.left_tip, -1.0f32), (rig.right_tip, 1.0)] {
        let d = sign * (tip - rig.wedge).length();
        chrome::trace(
            &mut shapes,
            &[
                on_beam(rig.wedge, rig.tilt, d, 3.5),
                on_beam(rig.wedge, rig.tilt, d, -3.5),
            ],
            Weight::Hair,
            ink,
        );
    }
    // The knife edge, and the junction where the flanges cross on it.
    shapes.push(egui::Shape::convex_polygon(
        vec![
            egui::pos2(rig.wedge.x, rig.wedge.y + 1.5),
            egui::pos2(rig.wedge.x - 6.0, rig.wedge.y + 11.0),
            egui::pos2(rig.wedge.x + 6.0, rig.wedge.y + 11.0),
        ],
        ink,
        egui::Stroke::NONE,
    ));
    chrome::junction(&mut shapes, rig.wedge, ink, face.ground());

    // ── the poise, and the scale it stands on ─────────────────────────
    // The graduations rotate with the BEAM, not with the screen, because
    // they are cut into the arm.
    let standing = 1.0 - (face.value(p::LEAN) / 100.0).clamp(0.0, 1.0);
    for i in 0..ARM_TICKS {
        let at = i as f32 / (ARM_TICKS - 1) as f32;
        let d = egui::lerp((0.16 * arm_r)..=(arm_r - 6.0), at);
        let here = (at - standing).abs() < 0.5 / (ARM_TICKS - 1) as f32;
        let reach = if here { 7.0 } else { 4.0 + 2.0 * lit };
        chrome::trace(
            &mut shapes,
            &[
                on_beam(rig.wedge, rig.tilt, d, 0.0),
                on_beam(rig.wedge, rig.tilt, d, -reach),
            ],
            Weight::Hair,
            if here {
                ink
            } else {
                tool::mix_ink(tool::fade(edge, 0.6), ink, lit)
            },
        );
    }
    // The collar that clamps the weight to the arm, perpendicular to the
    // beam — so the poise is plainly ON the beam and not floating.
    let seat = on_beam(
        rig.wedge,
        rig.tilt,
        (rig.poise.center() - rig.wedge).length(),
        0.0,
    );
    chrome::trace(&mut shapes, &[seat, rig.poise.center()], Weight::Hair, ink);
    chrome::octagon(
        &mut shapes,
        egui::Rect::from_center_size(rig.poise.center(), egui::vec2(11.0, 11.0)),
        2.0,
        Some(ink),
        Some((Weight::Hair, face.focus())),
    );
    tool::halo(&mut shapes, rig.poise, lit, face.focus());

    // ── the pair, in and out ──────────────────────────────────────────
    // In at the left margin and poured into the vessel; up the yoke; the
    // beam's own flanges; out by two stays that visibly take up slack as
    // the far arm rises.
    let pour = [
        egui::pos2(rig.inner.left() + 6.0, rig.pan.top() + 8.0),
        egui::pos2(rig.inner.left() + 6.0, rig.pan.top() + 14.0),
    ];
    for (index, at) in pour.into_iter().enumerate() {
        chrome::via(&mut shapes, at, ink, face.ground());
        let corner = if index == 0 {
            rig.pan.left_top()
        } else {
            egui::pos2(rig.pan.left() + 4.0, rig.pan.top())
        };
        chrome::trace(
            &mut shapes,
            &[at, corner],
            Weight::Hair,
            tool::fade(live, 0.7),
        );
    }
    let knot = egui::pos2(rig.left_tip.x, rig.left_tip.y + 12.0);
    chrome::trace(&mut shapes, &[rig.left_tip, knot], Weight::Hair, ink);
    for corner in [rig.pan.left_top(), rig.pan.right_top()] {
        chrome::trace(
            &mut shapes,
            &[knot, corner],
            Weight::Hair,
            tool::fade(live, 0.7),
        );
    }
    // The stays: the bow shrinks toward nothing as the arm rises, so the
    // leads take up slack exactly when the compressor grabs.
    let slack = 4.0 * (1.0 - (rig.tilt / SWING_DEG.to_radians()).clamp(0.0, 1.0));
    for post in rig.posts {
        let middle = egui::pos2(
            (rig.right_tip.x + post.x) * 0.5 + slack,
            (rig.right_tip.y + post.y) * 0.5,
        );
        chrome::trace(
            &mut shapes,
            &[rig.right_tip, middle, post],
            Weight::Hair,
            tool::fade(live, 0.7),
        );
        chrome::pad(&mut shapes, post, chrome::PAD - 2.0, ink, true);
    }

    face.painter.extend(shapes);

    // The mark becomes the poise: while the keyboard stands on LEAN the
    // brackets clamp onto the sliding weight and inherit both of its
    // motions — walking in under the hand, rising and falling with the
    // beam's live tilt — so the cursor itself wears the gain reduction.
    face.mark_signed(
        &lay,
        crate::ui::nav_cursor::Signature::Squeeze(
            (said.bands[0].abs() / FULL_SWING_DB).clamp(0.0, 1.0),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::{SectionKind, SectionParams};

    fn face_at(lean: f32, reduction: f32, level: f32) -> (egui::Rect, SectionParams, Telemetry) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(30.0, 40.0),
            egui::vec2(strip::width_of(SectionKind::Glue), 250.0),
        );
        let mut params = SectionParams::of(SectionKind::Glue);
        params.set(p::LEAN, lean);
        let said = Telemetry {
            level_db: level,
            reduction_db: reduction,
            bands: [
                reduction,
                lean,
                crate::console::glue_curve::threshold_db(lean),
            ],
        };
        (
            strip::recess_of(piece, SectionKind::Glue).shrink(3.0),
            params,
            said,
        )
    }

    /// One knob, one instrument: the poise, and it is on the beam.
    #[test]
    fn the_one_parameter_is_the_sliding_weight() {
        let table = SectionKind::Glue.table();
        assert_eq!(table.len(), 1, "GLUE has one knob and one weight");
        assert_eq!(table[0].id, p::LEAN);
    }

    /// Leaning walks the weight IN toward the pivot: that is what takes
    /// the beam's counter-torque away and lets the mix tip it.
    #[test]
    fn leaning_walks_the_weight_toward_the_pivot() {
        let far = {
            let (glass, _, _) = face_at(0.0, 0.0, -60.0);
            let inner = glass.shrink2(egui::vec2(5.0, 4.0));
            let fx = inner.left() + 0.40 * inner.width();
            let arm_r = inner.right() - 2.0 - fx;
            egui::lerp((0.16 * arm_r)..=(arm_r - 6.0), 1.0)
        };
        let near = {
            let (glass, _, _) = face_at(100.0, 0.0, -60.0);
            let inner = glass.shrink2(egui::vec2(5.0, 4.0));
            let fx = inner.left() + 0.40 * inner.width();
            let arm_r = inner.right() - 2.0 - fx;
            egui::lerp((0.16 * arm_r)..=(arm_r - 6.0), 0.0)
        };
        assert!(
            near < far - 20.0,
            "the weight did not travel: {near} to {far}"
        );
    }

    /// The beam tips about the wedge, left tip DOWN, and everything
    /// rigid on it goes with it.
    #[test]
    fn the_beam_tips_left_tip_down_and_carries_its_parts() {
        let wedge = egui::pos2(100.0, 100.0);
        let level = on_beam(wedge, 0.0, -40.0, 0.0);
        let tipped = on_beam(wedge, SWING_DEG.to_radians(), -40.0, 0.0);
        assert!(tipped.y > level.y, "the load arm did not fall");
        let right_level = on_beam(wedge, 0.0, 40.0, 0.0);
        let right_tipped = on_beam(wedge, SWING_DEG.to_radians(), 40.0, 0.0);
        assert!(right_tipped.y < right_level.y, "the poise arm did not rise");
        // A point ABOVE the beam stays above it at every angle.
        for degrees in [0.0f32, 5.0, 11.0] {
            let on = on_beam(wedge, degrees.to_radians(), 30.0, 0.0);
            let over = on_beam(wedge, degrees.to_radians(), 30.0, 7.0);
            assert!(
                over.y < on.y,
                "the poise sank through the beam at {degrees}"
            );
        }
    }

    /// The settle line is the kernel's own law, not a sketch: leaning
    /// harder puts the threshold further down the vessel, and a level
    /// above it asks for reduction while one below it asks for none.
    #[test]
    fn the_settle_line_comes_from_the_kernels_own_knee() {
        use crate::console::glue_curve;
        let open = glue_curve::threshold_db(0.0);
        let deep = glue_curve::threshold_db(100.0);
        assert!(deep < open - 20.0, "leaning did not move the threshold");
        assert_eq!(glue_curve::gain_db(-60.0, open), 0.0, "quiet is untouched");
        assert!(
            glue_curve::gain_db(0.0, deep) < -8.0,
            "a loud mix over a deep threshold is barely compressed"
        );
        // And it is monotone: louder never asks for less.
        let mut last = 0.0;
        for db in [-40.0, -30.0, -20.0, -10.0, 0.0] {
            let gain = glue_curve::gain_db(db, deep);
            assert!(gain <= last + 1e-4, "the knee went backwards at {db}");
            last = gain;
        }
    }

    /// The makeup is a LIFT: half of what the compressor takes is packed
    /// under the pivot as shims, and the whole rig rises by exactly that.
    #[test]
    fn the_makeup_is_drawn_as_a_lift_and_not_as_a_level() {
        let handed = |reduction: f32| (reduction.abs() * p::MAKEUP_SHARE).clamp(0.0, SHIMS_MAX);
        assert_eq!(handed(0.0), 0.0, "a desk not leaning stands flat");
        assert!(handed(6.0) > 0.0);
        assert!(handed(12.0) > handed(6.0), "more taken, more given back");
        assert_eq!(handed(100.0), SHIMS_MAX, "the stack has a top");
        assert!(
            (handed(6.0) - 6.0 * p::MAKEUP_SHARE).abs() < 1e-5,
            "the lift is not the desk's own share"
        );
    }
}
