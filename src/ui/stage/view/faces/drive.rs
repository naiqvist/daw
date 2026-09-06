//! DRIVE's face: five curves, one of them yours.
//!
//! The section is saturation with five characters, and choosing between
//! them is the whole of using it. So all five are drawn at once — the
//! chosen one solid, the other four behind it — because the reason to
//! pick FOLD over TRANSISTOR is a SHAPE, and a list of five words does
//! not have it in it. At any drive you can see that the fold reflects
//! where the others flatten, that the tape's knee is the roundest, that
//! the fuzz has a step in it and the tube leans to one side.
//!
//! The curve is `console::drive_curve::transfer`, which is the function
//! the core runs.
//!
//! The TILTS are drawn as the pair they are: one before the curve
//! choosing what gets driven, one after it putting the balance back.
//! Both pivot at the same kilohertz, so they are drawn crossing at it —
//! tilt up before and the top breaks first, tilt down after and it comes
//! back. Two numbers could not say that they are a pair.
//!
//! Four readings underneath are measured and not set. IN is the input
//! peak taken BEFORE the tilt and the curve, so neither DRIVE nor OUT
//! moves it. DIRT is the distortion actually added — the difference
//! between wet and dry, taken inside the shaper before the trim, so no
//! part of it is level. TOP is the share of the output above the pivot,
//! which climbs as a driven low note grows harmonics. HEAT is how far up
//! its curve the hottest sample went.

use super::*;
use crate::ui::chrome;

/// One readout row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
/// The share of the glass the transfer takes.
/// @tune 0.35..0.75
const CURVE_SHARE: f32 = 0.50;
const COLUMN_GAP: f32 = 8.0;
/// The tilts' window, in dB either way.
const TILT_DB: f32 = 8.0;
/// The five characters, in the order the parameter numbers them.
const CHARACTERS: [&str; 5] = ["TUBE", "TAPE", "TRAN", "FUZZ", "FOLD"];

/// DRIVE's six controls.
#[derive(Clone, Copy, Debug)]
struct DriveFace {
    curve: egui::Rect,
    character: egui::Rect,
    tilt_plot: egui::Rect,
    tilt_pre: egui::Rect,
    tilt_post: egui::Rect,
    drive: egui::Rect,
    mix: egui::Rect,
    out: egui::Rect,
    said: egui::Rect,
}

impl Layout for DriveFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::drive as p;
        vec![
            (p::CHARACTER, self.character),
            (p::DRIVE, self.drive),
            (p::TILT_PRE, self.tilt_pre),
            (p::TILT_POST, self.tilt_post),
            (p::MIX, self.mix),
            (p::OUT, self.out),
        ]
    }
}

fn drive_face(glass: egui::Rect) -> DriveFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    // Left: the character strip over the transfer. Right: the tilts,
    // then the three figures, then what the section measured.
    let left_w = (x.width() - COLUMN_GAP) * crate::tune!(CURVE_SHARE);
    let left = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + left_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + COLUMN_GAP, x.top()), x.max);
    let character = egui::Rect::from_min_size(left.min, egui::vec2(left.width(), ROW_H));
    // The four measured readings run across the WHOLE foot of the card,
    // one row of four. They belong to neither column — they are what the
    // section did, not what it is set to — and across two columns each
    // one has room for its word and its figure side by side, which a
    // quarter of one column does not.
    let said = egui::Rect::from_min_max(
        egui::pos2(x.left(), x.bottom() - ROW_H),
        egui::pos2(x.right(), x.bottom()),
    );
    let body_bottom = said.top() - 4.0;
    let curve = egui::Rect::from_min_max(
        egui::pos2(left.left(), character.bottom() + 4.0),
        egui::pos2(left.right(), body_bottom),
    );
    // The right column, from its own foot up: three figures, and the
    // tilts take everything above them.
    let rows_h = ROW_H * 3.0 + 2.0;
    let rows_top = body_bottom - rows_h;
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), rows_top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    let tilt_plot = egui::Rect::from_min_max(
        right.min,
        egui::pos2(right.right(), (rows_top - 5.0).max(right.top() + 26.0)),
    );
    // The two tilts are addressed on their own halves of the plot's
    // foot: before and after, left and right, as they run.
    let strip_h = ROW_H;
    let half = tilt_plot.width() * 0.5;
    let tilt_pre = egui::Rect::from_min_size(
        egui::pos2(tilt_plot.left(), tilt_plot.bottom() - strip_h),
        egui::vec2(half - 2.0, strip_h),
    );
    let tilt_post = egui::Rect::from_min_size(
        egui::pos2(tilt_plot.left() + half, tilt_plot.bottom() - strip_h),
        egui::vec2(half, strip_h),
    );
    DriveFace {
        curve,
        character,
        tilt_plot,
        tilt_pre,
        tilt_post,
        drive: row(0),
        mix: row(1),
        out: row(2),
        said,
    }
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::drive_curve as dc;
    use crate::params::console::drive as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = drive_face(face.glass);
    let value = |id: u32| face.value(id);
    let character = value(p::CHARACTER).round().clamp(0.0, 4.0) as u32;
    let drive = value(p::DRIVE) / 100.0;
    let tilt_pre = value(p::TILT_PRE);
    let tilt_post = value(p::TILT_POST);
    let mix = value(p::MIX);
    let out = value(p::OUT);
    let wire = drive <= 0.0 && tilt_pre == 0.0 && tilt_post == 0.0 && out == 0.0;
    // What the section measured of itself.
    let heat = face.said.reduction_db.abs();
    let input_db = face.said.bands[0];
    let dirt = face.said.bands[1];
    let top = face.said.bands[2];
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();

    // ---- The character strip. ----------------------------------------
    chrome::panel_frame_variant(&mut shapes, lay.character, Weight::Hair, edge, 2);
    let cell_w = lay.character.width() / CHARACTERS.len() as f32;
    for i in 0..CHARACTERS.len() {
        if i as u32 == character {
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    lay.character.left() + i as f32 * cell_w,
                    lay.character.top(),
                ),
                egui::vec2(cell_w, lay.character.height()),
            );
            chrome::brackets(&mut shapes, cell.shrink(2.0), 4.0, Weight::Hair, ink);
        }
    }

    // ---- The five curves. --------------------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.curve,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(lay.curve.left() + 7.0, lay.curve.top() + font.size + 5.0),
        egui::pos2(lay.curve.right() - 7.0, lay.curve.bottom() - 7.0),
    );
    let at = |x: f32, y: f32| {
        egui::pos2(
            plot.left() + (x + 1.0) * 0.5 * plot.width(),
            plot.bottom() - (y.clamp(-1.2, 1.2) + 1.0) * 0.5 * plot.height(),
        )
    };
    for t in [0.25f32, 0.5, 0.75] {
        for line in [
            [
                egui::pos2(egui::lerp(plot.x_range(), t), plot.top()),
                egui::pos2(egui::lerp(plot.x_range(), t), plot.bottom()),
            ],
            [
                egui::pos2(plot.left(), egui::lerp(plot.y_range(), t)),
                egui::pos2(plot.right(), egui::lerp(plot.y_range(), t)),
            ],
        ] {
            chrome::trace(
                &mut shapes,
                &line,
                Weight::Hair,
                edge.gamma_multiply(if (t - 0.5).abs() < 0.01 { 0.55 } else { 0.28 }),
            );
        }
    }
    chrome::dashes(
        &mut shapes,
        &[at(-1.0, -1.0), at(1.0, 1.0)],
        0.0,
        Weight::Hair,
        edge.gamma_multiply(0.7),
    );
    let sweep = |which: u32| -> Vec<egui::Pos2> {
        (0..=72)
            .map(|i| {
                let x = -1.0 + 2.0 * i as f32 / 72.0;
                at(x, dc::transfer(which, drive, x))
            })
            .collect()
    };
    // The four you did not pick, behind: the reason to pick one is a
    // shape, and a shape needs something to be a shape against.
    for other in 0..CHARACTERS.len() as u32 {
        if other == character {
            continue;
        }
        chrome::curve(
            &mut shapes,
            &sweep(other),
            Weight::Hair,
            edge.gamma_multiply(0.85),
        );
    }
    let hot = tool::mix_ink(ink, alpha.jeopardy_latent.color, drive);
    chrome::curve(&mut shapes, &sweep(character), Weight::Heavy, hot);
    // Where the signal sits on it, from the input the section measured
    // before its own tilt and curve.
    if !quiet && input_db > -71.0 {
        let amp = 10f32.powf(input_db / 20.0).clamp(0.0, 1.0);
        for sign in [-1.0f32, 1.0] {
            chrome::pad(
                &mut shapes,
                at(sign * amp, dc::transfer(character, drive, sign * amp)),
                chrome::PAD,
                alpha.live.color,
                true,
            );
        }
    }

    // ---- The tilts, as the pair they are. ----------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.tilt_plot,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let tilt_field = egui::Rect::from_min_max(
        egui::pos2(
            lay.tilt_plot.left() + 7.0,
            lay.tilt_plot.top() + font.size + 4.0,
        ),
        egui::pos2(lay.tilt_plot.right() - 7.0, lay.tilt_pre.top() - 4.0),
    );
    if tilt_field.is_positive() {
        let mid = tilt_field.center().y;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(tilt_field.left(), mid),
                egui::pos2(tilt_field.right(), mid),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.6),
        );
        // The pivot both tilts turn about, drawn once because it is one
        // frequency and they share it.
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(tilt_field.center().x, tilt_field.top()),
                egui::pos2(tilt_field.center().x, tilt_field.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.6),
        );
        for (db, weight, tone) in [
            (tilt_pre, Weight::Heavy, hot),
            (tilt_post, Weight::Hair, alpha.live.color),
        ] {
            let reach = (db / TILT_DB).clamp(-1.0, 1.0) * tilt_field.height() * 0.5;
            chrome::curve(
                &mut shapes,
                &[
                    egui::pos2(tilt_field.left(), mid + reach),
                    egui::pos2(tilt_field.right(), mid - reach),
                ],
                weight,
                if db == 0.0 { edge } else { tone },
            );
        }
    }
    chrome::panel_frame_variant(&mut shapes, lay.tilt_pre, Weight::Hair, edge, 1);
    chrome::panel_frame_variant(&mut shapes, lay.tilt_post, Weight::Hair, edge, 3);
    painter.extend(shapes);

    // ---- The rows and the readings. ----------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "DRIVE");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let gutter = ["DRIVE", "MIX", "OUT", "DIRT", "HEAT", "TOP", "IN"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 8.0;
    let figure = span("-12.0") + 8.0;
    let bar_of = |rect: egui::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.5),
            egui::pos2(rect.right() - figure, rect.center().y + 2.5),
        )
    };
    for (rect, word, share, said, tone) in [
        (
            lay.drive,
            "DRIVE",
            drive,
            format!("{:.0}", drive * 100.0),
            hot,
        ),
        (
            lay.mix,
            "MIX",
            mix / 100.0,
            format!("{mix:.0}"),
            alpha.live.color,
        ),
        (
            lay.out,
            "OUT",
            (out + 24.0) / 36.0,
            format!("{out:+.1}"),
            ink,
        ),
    ] {
        let bar = bar_of(rect);
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.35));
            if share > 0.0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(
                            bar.left() + share.clamp(0.0, 1.0) * bar.width(),
                            bar.bottom(),
                        ),
                    ),
                    0.0,
                    tone,
                );
            }
        }
        words.text(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }
    // What it measured: two rows of two, so the four readings fit the
    // room the column has left.
    let quarter = lay.said.width() * 0.25;
    for (i, (word, share, said, tone)) in [
        (
            "IN",
            ((input_db + 72.0) / 72.0).clamp(0.0, 1.0),
            if quiet || input_db <= -71.0 {
                "--".to_owned()
            } else {
                format!("{input_db:+.0}")
            },
            alpha.live.color,
        ),
        (
            "HEAT",
            heat,
            if wire {
                "--".to_owned()
            } else {
                format!("{heat:.2}")
            },
            tool::mix_ink(ink, alpha.jeopardy_latent.color, heat),
        ),
        (
            "DIRT",
            dirt,
            if wire {
                "--".to_owned()
            } else {
                format!("{:.0}%", dirt * 100.0)
            },
            alpha.jeopardy_latent.color,
        ),
        (
            "TOP",
            top,
            if quiet {
                "--".to_owned()
            } else {
                format!("{:.0}%", top * 100.0)
            },
            alpha.live.color,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.said.left() + i as f32 * quarter, lay.said.top()),
            egui::vec2(quarter - 6.0, ROW_H),
        );
        // A word, its figure, and a hairline under the pair that fills
        // with the reading: at a quarter of the column there is no room
        // for a bar beside them, so the bar goes under them.
        let rule = egui::Rect::from_min_max(
            egui::pos2(cell.left() + 1.0, cell.bottom() - 2.5),
            egui::pos2(cell.right() - 1.0, cell.bottom() - 1.0),
        );
        if rule.is_positive() {
            painter.rect_filled(rule, 0.0, edge.gamma_multiply(0.35));
            if !quiet && share > 0.0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        rule.min,
                        egui::pos2(
                            rule.left() + share.clamp(0.0, 1.0) * rule.width(),
                            rule.bottom(),
                        ),
                    ),
                    0.0,
                    tone,
                );
            }
        }
        words.text(
            egui::pos2(cell.left() + 1.0, cell.center().y - 1.5),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(cell.right() - 1.0, cell.center().y - 1.5),
            egui::Align2::RIGHT_CENTER,
            said,
            ink,
        );
    }

    // ---- The words on the pictures. ----------------------------------
    for (i, word) in CHARACTERS.into_iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(
                lay.character.left() + i as f32 * cell_w,
                lay.character.top(),
            ),
            egui::vec2(cell_w, lay.character.height()),
        );
        words.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            word,
            if i as u32 == character { ink } else { edge },
        );
    }
    words.text(
        egui::pos2(lay.curve.left() + 8.0, lay.curve.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "TRANSFER",
        edge,
    );
    words.text(
        egui::pos2(lay.curve.right() - 8.0, lay.curve.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire { "WIRE" } else { "2x OS" },
        if wire { edge } else { alpha.live.color },
    );
    words.text(
        egui::pos2(lay.tilt_plot.left() + 8.0, lay.tilt_plot.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "TILT",
        edge,
    );
    words.text(
        egui::pos2(lay.tilt_plot.right() - 8.0, lay.tilt_plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.0}k", p::TILT_HZ / 1000.0),
        edge,
    );
    for (rect, word, db) in [
        (lay.tilt_pre, "PRE", tilt_pre),
        (lay.tilt_post, "POST", tilt_post),
    ] {
        words.text(
            egui::pos2(rect.left() + 3.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            edge,
        );
        words.text(
            egui::pos2(rect.right() - 3.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{db:+.1}"),
            if db == 0.0 { edge } else { ink },
        );
    }
    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 210.0))
    }

    #[test]
    fn every_drive_control_has_its_own_place() {
        let face = drive_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 6);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its place");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "controls {id} and {other} overlap"
                );
            }
        }
        assert!(!face.said.intersects(face.out));
    }

    /// The five curves are five DIFFERENT curves at the same drive —
    /// which is the only reason drawing all of them is worth the ink.
    #[test]
    fn the_five_characters_are_five_shapes() {
        use crate::console::drive_curve as dc;
        for drive in [0.35f32, 0.7, 1.0] {
            for a in 0..5u32 {
                for b in (a + 1)..5u32 {
                    let apart = (0..=16)
                        .map(|i| {
                            let x = -1.0 + 2.0 * i as f32 / 16.0;
                            (dc::transfer(a, drive, x) - dc::transfer(b, drive, x)).abs()
                        })
                        .fold(0.0f32, f32::max);
                    assert!(
                        apart > 0.01,
                        "characters {a} and {b} draw the same curve at drive {drive}"
                    );
                }
            }
        }
    }

    /// At no drive every character is the wire, so the card draws five
    /// diagonals and says WIRE rather than five different nothings.
    #[test]
    fn no_drive_is_the_wire_whichever_character_is_chosen() {
        use crate::console::drive_curve as dc;
        for which in 0..5u32 {
            for i in 0..=8 {
                let x = -1.0 + 2.0 * i as f32 / 8.0;
                assert!(
                    (dc::transfer(which, 0.0, x) - x).abs() < 1e-4,
                    "character {which} bent the wire at {x}"
                );
            }
        }
    }
}
