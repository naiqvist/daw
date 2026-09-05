//! HIT's face: a note run through the section.
//!
//! A transient shaper cannot be drawn from its knobs. What it does at
//! any instant depends on where in a note you are — the strike is a
//! millisecond, the tail is most of a second — so the picture is a model
//! hit put through the very kernel the core runs
//! (`console::hit_curve`), drawn twice: as it arrived, ghosted, and as
//! it leaves. The gap between the two IS the section, and it is the only
//! honest way to show a device whose whole claim is that it does not
//! care how loud you are.
//!
//! The three figures under it are measured, not set: STRIKE is the
//! transient weight the split found this block, TAIL is how far the
//! quick follower has fallen from the long one, EDGE is the brightness
//! actually added. MOVED is the most the gain moved, signed, so a lift
//! and a cut are told apart.

use super::*;
use crate::ui::chrome;

/// One lever's row.
/// @tune 10..28 px
pub(super) const ROW_H: f32 = 18.0;
/// The share of the glass the note takes. It is the device.
/// @tune 0.4..0.8
const NOTE_SHARE: f32 = 0.58;
const COLUMN_GAP: f32 = 7.0;
/// How long a note the card shows.
/// @tune 100..1200 ms
const SPAN_MS: f32 = 420.0;
/// The model note's decay.
/// @tune 40..800 ms
const DECAY_MS: f32 = 220.0;
/// The window the note is drawn in, in dB.
///
/// The ceiling is above the range a lever can reach, not at 0 dB: a note
/// lifted the full twelve stands twelve dB over its own peak, and a plot
/// that stopped at the peak would push the lift into the card's header.
const FLOOR_DB: f32 = -42.0;
const CEILING_DB: f32 = crate::params::console::hit::RANGE_DB + 3.0;
/// How many instants of the note are plotted.
const POINTS: usize = 160;
/// How the time axis is warped.
///
/// A strike is twenty milliseconds and a tail is most of a second. On a
/// linear axis the strike is a hairline at the left edge and the lever
/// that moves it appears to do nothing — which is the opposite of true.
/// This pulls the early milliseconds open without breaking the axis into
/// two scales.
/// @tune 0.2..1.0
const TIME_WARP: f32 = 0.4;

/// HIT's three levers and the row each one owns.
#[derive(Clone, Copy, Debug)]
struct HitFace {
    note: egui::Rect,
    attack: egui::Rect,
    sustain: egui::Rect,
    bright: egui::Rect,
    /// What the section measured. Not controls.
    said: egui::Rect,
}

impl HitFace {
    fn controls(self) -> [(u32, egui::Rect); 3] {
        use crate::params::console::hit as p;
        [
            (p::ATTACK, self.attack),
            (p::SUSTAIN, self.sustain),
            (p::BRIGHT, self.bright),
        ]
    }
}

impl Layout for HitFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        HitFace::controls(*self).to_vec()
    }
}

fn hit_face(glass: egui::Rect) -> HitFace {
    let x = glass.shrink2(egui::vec2(5.0, 2.0));
    let note_w = (x.width() - COLUMN_GAP) * crate::tune!(NOTE_SHARE);
    let note = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + note_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(note.right() + COLUMN_GAP, x.top()), x.max);
    let row_h = crate::tune!(ROW_H);
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), right.top() + i as f32 * (row_h + 2.0)),
            egui::vec2(right.width(), row_h),
        )
    };
    HitFace {
        note,
        attack: row(0),
        sustain: row(1),
        bright: row(2),
        said: egui::Rect::from_min_max(egui::pos2(right.left(), row(2).bottom() + 5.0), right.max),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::hit_curve as hc;
    use crate::params::console::hit as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = hit_face(face.glass);
    // The levers, as the section resolves them.
    let attack = face.value(p::ATTACK) / 100.0;
    let sustain = face.value(p::SUSTAIN) / 100.0;
    let bright = face.value(p::BRIGHT) / 100.0;
    let resting = attack == 0.0 && sustain == 0.0 && bright == 0.0;
    // What it measured of itself, last block.
    let strike_now = face.said.bands[0];
    let tail_now = face.said.bands[1];
    let edge_now = face.said.bands[2];
    let moved = face.said.reduction_db;
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.note,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(lay.note.left() + 7.0, lay.note.top() + font.size + 8.0),
        egui::pos2(lay.note.right() - 7.0, lay.note.bottom() - font.size - 6.0),
    );
    let x_at = |ms: f32| {
        let t = (ms / crate::tune!(SPAN_MS)).clamp(0.0, 1.0);
        plot.left() + t.powf(crate::tune!(TIME_WARP)) * plot.width()
    };
    let y_at = |db: f32| {
        let t = ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0);
        plot.bottom() - t * plot.height()
    };
    // The strike's own window: the millisecond or two the section calls
    // a strike, marked, because every reading on this card divides at it.
    let window = egui::Rect::from_min_max(
        egui::pos2(plot.left(), plot.top()),
        egui::pos2(x_at(p::WINDOW_MS), plot.bottom()),
    );
    shapes.push(egui::Shape::rect_filled(
        window,
        0.0,
        alpha.live_dim.color.gamma_multiply(0.30),
    ));
    for db in [0.0f32, -12.0, -24.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), y_at(db)),
                egui::pos2(plot.right(), y_at(db)),
            ],
            Weight::Hair,
            edge.gamma_multiply(if db == 0.0 { 0.70 } else { 0.28 }),
        );
    }
    let trace = hc::trace(
        attack,
        sustain,
        crate::tune!(DECAY_MS),
        crate::tune!(SPAN_MS),
        POINTS,
    );
    // The note as it arrived: ghosted, so the gap to the solid one is
    // the section's whole doing.
    let plain: Vec<egui::Pos2> = trace
        .iter()
        .map(|point| egui::pos2(x_at(point.ms), y_at(point.plain_db)))
        .collect();
    chrome::dashes(
        &mut shapes,
        &plain,
        0.0,
        Weight::Hair,
        edge.gamma_multiply(0.9),
    );
    let shaped: Vec<egui::Pos2> = trace
        .iter()
        .map(|point| egui::pos2(x_at(point.ms), y_at(point.plain_db + point.gain_db)))
        .collect();
    chrome::trace(
        &mut shapes,
        &shaped,
        Weight::Heavy,
        if resting {
            edge.gamma_multiply(0.9)
        } else {
            ink
        },
    );
    // Where the section is standing in a note RIGHT NOW: the strike it
    // found and the tail it found, on the two axes they belong to.
    if !quiet {
        if strike_now > 0.02 {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(x_at(p::WINDOW_MS * strike_now), plot.top()),
                    egui::pos2(x_at(p::WINDOW_MS * strike_now), plot.bottom()),
                ],
                Weight::Hair,
                alpha.live.color,
            );
        }
        if tail_now > 0.02 {
            let at = trace
                .iter()
                .min_by(|a, b| {
                    (a.tail - tail_now)
                        .abs()
                        .total_cmp(&(b.tail - tail_now).abs())
                })
                .map(|point| point.ms)
                .unwrap_or(0.0);
            chrome::pad(
                &mut shapes,
                egui::pos2(x_at(at), y_at(-6.0)),
                chrome::PAD,
                alpha.live.color,
                true,
            );
        }
    }
    painter.extend(shapes);

    // ---- The three levers, bipolar about a bright centre. ------------
    // Every word goes through the ledger: it measures where each one
    // lands and refuses, in a debug build, to let two of them crowd.
    let mut words = tool::Ledger::new(painter, font.clone(), "HIT");
    let mut label = |at: egui::Pos2, align: egui::Align2, text: String, ink: egui::Color32| {
        words.text(at, align, text, ink);
    };
    // The value column is as wide as the widest figure that can stand
    // in it, measured, not guessed — a signed dB reading with its unit.
    let value_w = painter
        .layout_no_wrap("-12.0dB".to_owned(), font.clone(), ink)
        .rect
        .width()
        + 8.0;
    let gutter = ["ATK", "SUS", "BRT", "STRIKE", "MOVED"]
        .into_iter()
        .map(|word| {
            painter
                .layout_no_wrap(word.to_owned(), font.clone(), ink)
                .rect
                .width()
        })
        .fold(0.0f32, f32::max)
        + 8.0;
    for (rect, word, place, bipolar, said, tone) in [
        (
            lay.attack,
            "ATK",
            attack,
            true,
            format!("{:+.1}dB", attack * p::RANGE_DB),
            alpha.live.color,
        ),
        (
            lay.sustain,
            "SUS",
            sustain,
            true,
            format!("{:+.1}dB", sustain * p::RANGE_DB),
            alpha.live.color,
        ),
        (
            lay.bright,
            "BRT",
            bright,
            false,
            format!("{:.0}%", bright * 100.0),
            alpha.jeopardy_latent.color,
        ),
    ] {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 4.0),
            egui::pos2(rect.right() - value_w, rect.center().y + 4.0),
        );
        if bar.is_positive() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(bar.left(), bar.center().y - 1.0),
                    egui::pos2(bar.right(), bar.center().y + 1.0),
                ),
                0.0,
                edge.gamma_multiply(0.45),
            );
            let origin = if bipolar { bar.center().x } else { bar.left() };
            let reach = place.clamp(-1.0, 1.0) * bar.width() * if bipolar { 0.5 } else { 1.0 };
            if reach.abs() > 0.5 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(origin.min(origin + reach), bar.top()),
                        egui::pos2(origin.max(origin + reach), bar.bottom()),
                    ),
                    0.0,
                    tone,
                );
            }
            if bipolar {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(bar.center().x - 0.5, bar.top() - 2.0),
                        egui::pos2(bar.center().x + 0.5, bar.bottom() + 2.0),
                    ),
                    0.0,
                    edge,
                );
            }
        }
        label(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            edge,
        );
        label(
            egui::pos2(rect.right() - 3.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            if place == 0.0 { edge } else { ink },
        );
    }

    // ---- What it measured. Nothing here comes from a knob. -----------
    let ch = painter
        .layout_no_wrap("M".to_owned(), font.clone(), ink)
        .rect
        .height();
    let mut y = lay.said.top() + 1.0;
    for (word, share, said, tone) in [
        (
            "STRIKE",
            strike_now,
            format!("{:.0}%", strike_now * 100.0),
            alpha.live.color,
        ),
        (
            "TAIL",
            tail_now,
            format!("{:.0}%", tail_now * 100.0),
            alpha.live.color,
        ),
        (
            "EDGE",
            edge_now,
            format!("{:.0}%", edge_now * 100.0),
            alpha.jeopardy_latent.color,
        ),
        (
            "MOVED",
            (moved.abs() / p::RANGE_DB).clamp(0.0, 1.0),
            if quiet {
                "--".to_owned()
            } else {
                format!("{moved:+.1}dB")
            },
            if moved >= 0.0 {
                alpha.live.color
            } else {
                alpha.jeopardy_latent.color
            },
        ),
    ] {
        if y + ch > lay.said.bottom() {
            break;
        }
        let bar = egui::Rect::from_min_max(
            egui::pos2(lay.said.left() + gutter, y + ch * 0.5 - 2.0),
            egui::pos2(lay.said.right() - value_w, y + ch * 0.5 + 2.0),
        );
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.35));
            if !quiet && share > 0.0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(bar.left() + share * bar.width(), bar.bottom()),
                    ),
                    0.0,
                    tone,
                );
            }
        }
        label(
            egui::pos2(lay.said.left() + 2.0, y + ch * 0.5),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            edge,
        );
        label(
            egui::pos2(lay.said.right() - 3.0, y + ch * 0.5),
            egui::Align2::RIGHT_CENTER,
            if quiet { "--".to_owned() } else { said },
            if quiet { edge } else { ink },
        );
        y += ch + 2.0;
    }

    // ---- The note's own words. ---------------------------------------
    label(
        egui::pos2(lay.note.left() + 8.0, lay.note.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "NOTE".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.note.right() - 8.0, lay.note.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if resting {
            "WIRE".to_owned()
        } else {
            "SHAPED".to_owned()
        },
        if resting { edge } else { alpha.live.color },
    );
    label(
        egui::pos2(window.right() - 2.0, lay.note.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        format!("{:.0}", p::WINDOW_MS),
        alpha.live_dim.color,
    );
    label(
        egui::pos2(plot.left(), lay.note.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        "0".to_owned(),
        edge,
    );
    label(
        egui::pos2(plot.right(), lay.note.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{:.0}ms", crate::tune!(SPAN_MS)),
        edge,
    );

    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 210.0))
    }

    #[test]
    fn every_hit_lever_has_its_own_row() {
        let face = hit_face(glass());
        let controls = face.controls();
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "lever {id} left the glass");
            assert!(rect.is_positive(), "lever {id} lost its row");
            assert!(!face.note.intersects(*rect), "lever {id} invaded the note");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "levers {id} and {other} overlap"
                );
            }
        }
        assert!(!face.said.intersects(face.bright));
    }

    /// The note is the device, so it is the biggest thing on the card.
    #[test]
    fn the_note_owns_most_of_the_glass() {
        let face = hit_face(glass());
        assert!(face.note.width() > glass().width() * 0.45);
        assert!(face.note.height() > face.attack.height() * 4.0);
    }
}
