//! VCA's face: the SSL bus compressor, and its meter is a real one.
//!
//! The G-series box has a moving-coil gain-reduction meter, and this is
//! that meter: a needle on an arc, zero at the RIGHT and the reduction
//! swinging it left, because that is which way a GR meter reads. It has
//! a pivot, a counterweight tail past the pivot, a scale with its
//! figures, and a held peak standing where the loudest moment of the
//! last second put it.
//!
//! It is drawn through `chrome::curve` rather than `chrome::trace`: the
//! trace snaps to the pixel grid, which is right for a rule and wrong
//! for an arc — snapped, the scale climbs in visible steps and the
//! needle wobbles as it swings.
//!
//! Beside it, the two things a compressor is: the KNEE — the transfer
//! from in to out with the threshold, the ratio and the three-dB soft
//! knee the detector and the loop actually give it — and the KEY, which
//! shows what the sidechain high-pass took off before the detector
//! heard it. A bass note that is pumping the box shows up as a gap
//! between those two bars and nowhere else.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 10..24 px
const ROW_H: f32 = 14.0;
/// The share of the glass the meter takes.
/// @tune 0.25..0.7
const METER_SHARE: f32 = 0.42;
const COLUMN_GAP: f32 = 7.0;
/// The meter's scale, in dB of reduction: zero at the right.
const METER_MAX_DB: f32 = 20.0;
/// The arc's ends, in degrees. Zero is right, counter-clockwise.
const ARC_FROM: f32 = 34.0;
const ARC_TO: f32 = 146.0;
/// The knee plot's window, in dB.
const KNEE_MIN_DB: f32 = -48.0;
const KNEE_MAX_DB: f32 = 6.0;

/// VCA's seven controls and the row each one owns.
#[derive(Clone, Copy, Debug)]
struct VcaFace {
    meter: egui::Rect,
    knee: egui::Rect,
    key: egui::Rect,
    threshold: egui::Rect,
    ratio: egui::Rect,
    attack: egui::Rect,
    release: egui::Rect,
    makeup: egui::Rect,
    sc_hp: egui::Rect,
    mix: egui::Rect,
}

impl VcaFace {
    fn controls(self) -> [(u32, egui::Rect); 7] {
        use crate::params::console::vca as p;
        [
            (p::THRESHOLD, self.threshold),
            (p::RATIO, self.ratio),
            (p::ATTACK, self.attack),
            (p::RELEASE, self.release),
            (p::MAKEUP, self.makeup),
            (p::SC_HP, self.sc_hp),
            (p::MIX, self.mix),
        ]
    }
}

impl Layout for VcaFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        VcaFace::controls(*self).to_vec()
    }
}

fn vca_face(glass: egui::Rect) -> VcaFace {
    // The glass is wider than the casing draws and its foot carries the
    // plinth, so the inset is not symmetric.
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 2.0),
        egui::pos2(glass.right() - 24.0, glass.bottom() - 13.0),
    );
    let row_h = crate::tune!(ROW_H);
    let meter_w = (x.width() - COLUMN_GAP * 2.0) * crate::tune!(METER_SHARE);
    let meter = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + meter_w, x.bottom()));
    let rest = egui::Rect::from_min_max(egui::pos2(meter.right() + COLUMN_GAP, x.top()), x.max);
    // The knee stands over the key, and the seven rows fill the last
    // column: measured from the foot up, so nothing runs off a short
    // glass.
    let rows_w = (rest.width() - COLUMN_GAP) * 0.46;
    let plots = egui::Rect::from_min_max(
        rest.min,
        egui::pos2(rest.right() - rows_w - COLUMN_GAP, rest.bottom()),
    );
    let key_h = (row_h * 2.0 + 6.0).min(plots.height() * 0.4);
    let knee = egui::Rect::from_min_max(
        plots.min,
        egui::pos2(plots.right(), plots.bottom() - key_h - 4.0),
    );
    let key = egui::Rect::from_min_max(egui::pos2(plots.left(), knee.bottom() + 4.0), plots.max);
    let rows = egui::Rect::from_min_max(egui::pos2(rest.right() - rows_w, rest.top()), rest.max);
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(rows.left(), rows.top() + i as f32 * (row_h + 1.0)),
            egui::vec2(rows.width(), row_h),
        )
    };
    VcaFace {
        meter,
        knee,
        key,
        threshold: row(0),
        ratio: row(1),
        attack: row(2),
        release: row(3),
        makeup: row(4),
        sc_hp: row(5),
        mix: row(6),
    }
}

/// A point on the meter's arc.
fn on_dial(pivot: egui::Pos2, radius: f32, deg: f32) -> egui::Pos2 {
    let a = deg.to_radians();
    egui::pos2(pivot.x + radius * a.cos(), pivot.y - radius * a.sin())
}

/// Where a reduction stands on the dial, in degrees. Zero reduction is
/// the right-hand end: a GR meter falls away from rest.
fn deg_of_reduction(db: f32) -> f32 {
    let t = (db / METER_MAX_DB).clamp(0.0, 1.0);
    ARC_FROM + (ARC_TO - ARC_FROM) * t
}

/// The compressor's static curve: what a level in becomes on the way
/// out, with the soft knee the box actually has.
fn knee_db(input_db: f32, threshold: f32, ratio: f32, makeup: f32) -> f32 {
    use crate::params::console::vca as p;
    let over = input_db - threshold;
    let knee = p::KNEE_DB;
    let compressed = if over <= -knee {
        input_db
    } else if over >= knee {
        threshold + over / ratio
    } else {
        // The quadratic the soft knee is: slope one at the bottom of it,
        // slope 1/ratio at the top, continuous at both ends.
        let t = over + knee;
        input_db + (1.0 / ratio - 1.0) * t * t / (4.0 * knee)
    };
    compressed + makeup
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::vca as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = vca_face(face.glass);
    let value = |id: u32| face.value(id);
    let threshold = value(p::THRESHOLD);
    let ratio = p::RATIO_VALUES[(value(p::RATIO).round().clamp(0.0, 2.0)) as usize];
    let attack_ms = p::ATTACK_MS[(value(p::ATTACK).round().clamp(0.0, 5.0)) as usize];
    let release_step = value(p::RELEASE).round().clamp(0.0, 4.0) as usize;
    let makeup = value(p::MAKEUP);
    let sc_hp = value(p::SC_HP);
    let mix = value(p::MIX);
    // What the loop is doing, as it reported it.
    let now_db = face.said.bands[0].abs();
    let held_db = face.said.reduction_db.abs();
    let key_raw = face.said.bands[1];
    let key_heard = face.said.bands[2];
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();

    // ---- The meter. --------------------------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.meter,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge)),
        2,
    );
    let dial_glass = lay.meter.shrink(3.0);
    chrome::panel_variant(
        &mut shapes,
        dial_glass,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    // The pivot sits low and centred; the arc takes the width it can.
    let pivot = egui::pos2(
        dial_glass.center().x,
        dial_glass.bottom() - dial_glass.height() * 0.22,
    );
    let radius = (dial_glass.width() * 0.44).min(dial_glass.height() * 0.58);
    let arc: Vec<egui::Pos2> = (0..=64)
        .map(|i| {
            on_dial(
                pivot,
                radius,
                ARC_FROM + (ARC_TO - ARC_FROM) * i as f32 / 64.0,
            )
        })
        .collect();
    chrome::curve(&mut shapes, &arc, Weight::Heavy, ink.gamma_multiply(0.85));
    // The scale. A GR meter's figures crowd at the quiet end, so the
    // marks are the box's own: 1, 2, 4, 6, 10, 20.
    for db in [0.0f32, 1.0, 2.0, 4.0, 6.0, 10.0, 20.0] {
        let deg = deg_of_reduction(db);
        let long = db == 0.0 || db == 6.0 || db == 20.0;
        chrome::curve(
            &mut shapes,
            &[
                on_dial(pivot, radius - if long { 6.0 } else { 3.5 }, deg),
                on_dial(pivot, radius, deg),
            ],
            if long { Weight::Heavy } else { Weight::Hair },
            if db >= 10.0 {
                alpha.jeopardy_latent.color
            } else {
                edge
            },
        );
    }
    // The held peak, behind the needle: where the loudest moment of the
    // last second put it.
    if held_db > 0.05 && !quiet {
        chrome::curve(
            &mut shapes,
            &[
                on_dial(pivot, radius * 0.30, deg_of_reduction(held_db)),
                on_dial(pivot, radius - 3.0, deg_of_reduction(held_db)),
            ],
            Weight::Hair,
            alpha.live_dim.color,
        );
    }
    // The needle, with the counterweight a moving coil carries past its
    // pivot — the small tail is what makes it read as a needle and not
    // as a line from a point.
    let deg = deg_of_reduction(if quiet { 0.0 } else { now_db });
    chrome::curve(
        &mut shapes,
        &[
            on_dial(pivot, -radius * 0.16, deg),
            on_dial(pivot, radius - 4.0, deg),
        ],
        Weight::Heavy,
        if now_db >= 10.0 {
            alpha.jeopardy_latent.color
        } else {
            ink
        },
    );
    shapes.push(egui::Shape::circle_filled(pivot, 3.0, ink));
    shapes.push(egui::Shape::circle_filled(pivot, 1.5, alpha.ground.color));

    // ---- The knee: what a level in becomes on the way out. -----------
    chrome::panel_variant(
        &mut shapes,
        lay.knee,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(lay.knee.left() + 6.0, lay.knee.top() + font.size + 4.0),
        egui::pos2(lay.knee.right() - 6.0, lay.knee.bottom() - 4.0),
    );
    let at = |input: f32, output: f32| {
        let sx = ((input - KNEE_MIN_DB) / (KNEE_MAX_DB - KNEE_MIN_DB)).clamp(0.0, 1.0);
        let sy = ((output - KNEE_MIN_DB) / (KNEE_MAX_DB - KNEE_MIN_DB)).clamp(0.0, 1.0);
        egui::pos2(
            plot.left() + sx * plot.width(),
            plot.bottom() - sy * plot.height(),
        )
    };
    // The wire the box would be if it let everything through.
    chrome::dashes(
        &mut shapes,
        &[at(KNEE_MIN_DB, KNEE_MIN_DB), at(KNEE_MAX_DB, KNEE_MAX_DB)],
        0.0,
        Weight::Hair,
        edge.gamma_multiply(0.7),
    );
    let curve: Vec<egui::Pos2> = (0..=48)
        .map(|i| {
            let input = KNEE_MIN_DB + (KNEE_MAX_DB - KNEE_MIN_DB) * i as f32 / 48.0;
            at(input, knee_db(input, threshold, ratio, makeup))
        })
        .collect();
    chrome::curve(&mut shapes, &curve, Weight::Heavy, ink);
    // The threshold, where the bend begins.
    chrome::curve(
        &mut shapes,
        &[
            egui::pos2(at(threshold, 0.0).x, plot.top()),
            egui::pos2(at(threshold, 0.0).x, plot.bottom()),
        ],
        Weight::Hair,
        alpha.jeopardy_latent.color,
    );
    // Where the signal is standing on that curve right now.
    if !quiet {
        let input = face.said.level_db - makeup + now_db;
        chrome::pad(
            &mut shapes,
            at(input, knee_db(input, threshold, ratio, makeup)),
            chrome::PAD,
            alpha.live.color,
            true,
        );
    }

    // ---- The key: what the sidechain filter took off. ----------------
    chrome::panel_frame_variant(&mut shapes, lay.key, Weight::Hair, edge, 3);
    painter.extend(shapes);
    let label = |at: egui::Pos2, align: egui::Align2, text: String, ink| {
        painter.text(at, align, text, font.clone(), ink);
    };
    let key_bar = |i: usize, db: f32, tone: egui::Color32| {
        let row = egui::Rect::from_min_size(
            egui::pos2(lay.key.left() + 34.0, lay.key.top() + 4.0 + i as f32 * 9.0),
            egui::vec2(lay.key.width() - 42.0, 5.0),
        );
        if !row.is_positive() {
            return;
        }
        painter.rect_filled(row, 0.0, edge.gamma_multiply(0.35));
        if !quiet && db > -60.0 {
            let share = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    row.min,
                    egui::pos2(row.left() + share * row.width(), row.bottom()),
                ),
                0.0,
                tone,
            );
        }
    };
    key_bar(0, key_raw, edge.gamma_multiply(0.9));
    key_bar(1, key_heard, alpha.live.color);

    // ---- The words and the figures. ----------------------------------
    label(
        egui::pos2(dial_glass.left() + 6.0, dial_glass.top() + 3.0),
        egui::Align2::LEFT_TOP,
        "GR".to_owned(),
        edge,
    );
    label(
        egui::pos2(dial_glass.right() - 6.0, dial_glass.top() + 3.0),
        egui::Align2::RIGHT_TOP,
        if quiet {
            "--".to_owned()
        } else {
            format!("-{now_db:.1}")
        },
        if now_db >= 10.0 {
            alpha.jeopardy_latent.color
        } else {
            ink
        },
    );
    for db in [0.0f32, 6.0, 20.0] {
        label(
            // Well inside the arc: the ticks reach in six from it, so a
            // figure any closer sits on their ends.
            on_dial(pivot, radius - 22.0, deg_of_reduction(db)),
            egui::Align2::CENTER_CENTER,
            format!("{db:.0}"),
            if db >= 10.0 {
                alpha.jeopardy_latent.color
            } else {
                edge
            },
        );
    }
    label(
        egui::pos2(lay.knee.left() + 7.0, lay.knee.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "KNEE".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.knee.right() - 7.0, lay.knee.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{ratio:.0}:1"),
        ink,
    );
    label(
        egui::pos2(lay.key.left() + 3.0, lay.key.top() + 4.0),
        egui::Align2::LEFT_TOP,
        "KEY".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.key.left() + 3.0, lay.key.top() + 13.0),
        egui::Align2::LEFT_TOP,
        "HRD".to_owned(),
        edge,
    );
    for (rect, word, said) in [
        (lay.threshold, "THRES", format!("{threshold:.0}")),
        (lay.ratio, "RATIO", format!("{ratio:.0}:1")),
        (
            lay.attack,
            "ATTACK",
            if attack_ms < 1.0 {
                format!("{attack_ms:.1}")
            } else {
                format!("{attack_ms:.0}")
            },
        ),
        (
            lay.release,
            "RELEASE",
            if release_step == p::RELEASE_AUTO {
                "AUTO".to_owned()
            } else {
                format!("{:.0}", p::RELEASE_MS[release_step])
            },
        ),
        (lay.makeup, "MAKEUP", format!("{makeup:.0}")),
        (
            lay.sc_hp,
            "SC HP",
            if sc_hp <= 21.0 {
                "OFF".to_owned()
            } else {
                format!("{sc_hp:.0}")
            },
        ),
        (lay.mix, "MIX", format!("{mix:.0}")),
    ] {
        label(
            egui::pos2(rect.left() + 1.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            edge,
        );
        label(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 210.0))
    }

    #[test]
    fn every_vca_control_has_its_own_row() {
        let face = vca_face(glass());
        let controls = face.controls();
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its row");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "controls {id} and {other} overlap"
                );
            }
            for (what, plot) in [
                ("meter", face.meter),
                ("knee", face.knee),
                ("key", face.key),
            ] {
                assert!(!plot.intersects(*rect), "control {id} invaded the {what}");
            }
        }
    }

    /// The dial reads the way a gain-reduction meter reads: at rest the
    /// needle stands at the RIGHT, and reduction swings it left.
    #[test]
    fn the_needle_falls_away_from_rest() {
        assert_eq!(deg_of_reduction(0.0), ARC_FROM);
        assert_eq!(deg_of_reduction(METER_MAX_DB), ARC_TO);
        assert!(deg_of_reduction(6.0) > deg_of_reduction(2.0));
        // Past the end of the scale it pins rather than swinging round.
        assert_eq!(deg_of_reduction(200.0), ARC_TO);
        // Counter-clockwise from the right is leftward on screen.
        let pivot = egui::pos2(100.0, 100.0);
        assert!(on_dial(pivot, 40.0, deg_of_reduction(20.0)).x < on_dial(pivot, 40.0, ARC_FROM).x);
    }

    /// The static curve is a wire under the threshold, the ratio above
    /// it, and continuous through the knee that joins them.
    #[test]
    fn the_knee_joins_the_wire_to_the_ratio() {
        use crate::params::console::vca as p;
        let (threshold, ratio) = (-20.0f32, 4.0f32);
        // Well below: untouched.
        for input in [-60.0f32, -40.0, -30.0] {
            assert!(
                (knee_db(input, threshold, ratio, 0.0) - input).abs() < 0.01,
                "{input} was touched below the knee"
            );
        }
        // Well above: the ratio, exactly.
        let input = 0.0;
        let want = threshold + (input - threshold) / ratio;
        assert!((knee_db(input, threshold, ratio, 0.0) - want).abs() < 0.01);
        // Continuous across both ends of the knee, and monotone through
        // it — a curve with a step in it would be a click.
        let mut last = f32::MIN;
        let mut step = threshold - p::KNEE_DB - 2.0;
        while step <= threshold + p::KNEE_DB + 2.0 {
            let out = knee_db(step, threshold, ratio, 0.0);
            assert!(out >= last, "the knee went backwards at {step}");
            last = out;
            step += 0.25;
        }
        // Makeup lifts the whole curve and nothing else.
        assert!(
            (knee_db(-30.0, threshold, ratio, 6.0) - knee_db(-30.0, threshold, ratio, 0.0) - 6.0)
                .abs()
                < 0.01
        );
    }
}
