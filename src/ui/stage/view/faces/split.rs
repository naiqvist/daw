//! SPLIT's face: the crossover IS the meter.
//!
//! Three-band dynamics on a Linkwitz–Riley crossover that reconstructs —
//! do nothing to the bands and their sum is the input — so the sound is
//! genuinely in three places, and the card puts them where they are: a
//! frequency strip cut at the two corners, each region carrying its own
//! band's live movement as a bipolar column from a centre line. Pulled
//! down reads down, pushed up reads up.
//!
//! The lever under each region is an AMOUNT, not a threshold and a
//! ratio. A band is held toward its OWN long average — a second and a
//! half of it — so louder moments come down and quieter ones go up, and
//! at negative amounts the band is pushed away from that average and its
//! dynamics are exaggerated instead. Held against its own average a band
//! needs no threshold and the lever works at any level, which is why
//! there is no threshold on this card to look for.
//!
//! Each band's ballistics are fixed and its own — the bottom slow, the
//! top quick — and they are printed rather than hidden, because they are
//! the reason the same amount does a different thing to a kick than to a
//! hi-hat.
//!
//! # This one wears more furniture than the rest
//!
//! The strip is a machine watching three things at once and reporting on
//! them, which is the one section on the desk that genuinely reads like
//! a console watching a system, so it is dressed like one: registration
//! marks at its corners, a ruled scale along its head, a numbered header
//! per band with a pip that lights when that band is working, a reticle
//! that closes on whichever band the keyboard is standing in, and a foot
//! of system words.
//!
//! Every one of those is a fact and not a decoration. LR4 is the
//! crossover's actual order. LINK L/R is true — the bands are linked by
//! the louder side so the image holds. AVG is the long average a band is
//! held against, in seconds, and it is the number that makes the amount
//! work without a threshold. The pip is the band's own movement. The
//! reticle is where your hand is.

use super::*;
use crate::ui::chrome;

/// One band's row of controls.
/// @tune 10..24 px
pub(super) const ROW_H: f32 = 18.0;
/// The strip's share of the glass. It is the meter and the crossover at
/// once, so it takes the room a picture takes.
/// @tune 0.3..0.8
const STRIP_SHARE: f32 = 0.52;
/// The reach of a band's movement column, in dB either way.
const MOVE_DB: f32 = 12.0;
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;

/// SPLIT's eight controls.
#[derive(Clone, Copy, Debug)]
struct SplitFace {
    strip: egui::Rect,
    /// The two corners, addressed on the strip itself.
    low_hz: egui::Rect,
    high_hz: egui::Rect,
    /// Per band: the amount, then its gain.
    amount: [egui::Rect; 3],
    gain: [egui::Rect; 3],
}

impl Layout for SplitFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::split as p;
        let mut out = vec![(p::LOW_HZ, self.low_hz), (p::HIGH_HZ, self.high_hz)];
        for (i, id) in [p::LOW, p::MID, p::HIGH].into_iter().enumerate() {
            out.push((id, self.amount[i]));
        }
        for (i, id) in [p::LOW_DB, p::MID_DB, p::HIGH_DB].into_iter().enumerate() {
            out.push((id, self.gain[i]));
        }
        out
    }
}

fn split_face(glass: egui::Rect) -> SplitFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 2.0),
        egui::pos2(glass.right() - 24.0, glass.bottom() - 13.0),
    );
    let row_h = crate::tune!(ROW_H);
    // From the foot up: three band rows, then the strip takes the rest.
    let rows_h = row_h * 3.0 + 2.0;
    let rows_top = x.bottom() - rows_h;
    let strip = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right(), (rows_top - 5.0).max(x.top() + 30.0)),
    );
    // The two corners are addressed under the strip, on the seam each
    // one moves: a corner is a place on a frequency scale, so its
    // control is a place on that scale too.
    let seam_h = 0.0;
    let _ = seam_h;
    let mut amount = [egui::Rect::NOTHING; 3];
    let mut gain = [egui::Rect::NOTHING; 3];
    let gain_w = 96.0f32.min(x.width() * 0.28);
    for i in 0..3 {
        let row = egui::Rect::from_min_size(
            egui::pos2(x.left(), rows_top + i as f32 * (row_h + 1.0)),
            egui::vec2(x.width(), row_h),
        );
        amount[i] = egui::Rect::from_min_max(
            row.min,
            egui::pos2(row.right() - gain_w - 4.0, row.bottom()),
        );
        gain[i] = egui::Rect::from_min_max(egui::pos2(row.right() - gain_w, row.top()), row.max);
    }
    // The corners take a slice off the strip's own foot, one each side
    // of the seam they stand on.
    let corner_h = 13.0f32.min(strip.height() * 0.22);
    let corner_band = egui::Rect::from_min_max(
        egui::pos2(strip.left(), strip.bottom() - corner_h),
        strip.max,
    );
    let half = corner_band.width() * 0.5;
    let low_hz = egui::Rect::from_min_size(corner_band.min, egui::vec2(half - 2.0, corner_h));
    let high_hz = egui::Rect::from_min_size(
        egui::pos2(corner_band.left() + half, corner_band.top()),
        egui::vec2(half, corner_h),
    );
    SplitFace {
        strip,
        low_hz,
        high_hz,
        amount,
        gain,
    }
}

/// Where a frequency stands across the strip, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::split as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = split_face(face.glass);
    let value = |id: u32| face.value(id);
    let low_hz = value(p::LOW_HZ);
    let high_hz = value(p::HIGH_HZ);
    let amounts = [value(p::LOW), value(p::MID), value(p::HIGH)];
    let gains = [value(p::LOW_DB), value(p::MID_DB), value(p::HIGH_DB)];
    // What each band's dynamics are doing right now, signed.
    let moved = face.said.bands;
    let quiet = face.said.level_db <= -119.0;
    let resting = amounts.iter().all(|a| *a == 0.0) && gains.iter().all(|g| *g == 0.0);
    let hues = [alpha.jeopardy_latent.color, ink, alpha.live.color];

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.strip,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.strip.left() + 7.0, lay.strip.top() + font.size + 5.0),
        egui::pos2(lay.strip.right() - 7.0, lay.low_hz.top() - 3.0),
    );
    let x_of = |hz: f32| field.left() + place_of_hz(hz) * field.width();
    let edges = [field.left(), x_of(low_hz), x_of(high_hz), field.right()];
    // The line a band is held toward: its own long average, which is
    // where a band that is doing nothing sits.
    let centre = field.center().y;
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(field.left(), centre),
            egui::pos2(field.right(), centre),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.75),
    );
    for (i, gain_db) in moved.iter().enumerate() {
        let region = egui::Rect::from_min_max(
            egui::pos2(edges[i], field.top()),
            egui::pos2(edges[i + 1], field.bottom()),
        );
        if !region.is_positive() {
            continue;
        }
        // The region's own ground, so the three bands are visibly three
        // places even before any of them moves.
        shapes.push(egui::Shape::rect_filled(
            region,
            0.0,
            hues[i].gamma_multiply(0.11),
        ));
        if quiet {
            continue;
        }
        // The movement, from the centre: down is pulled down, up is
        // pushed up. The column fills the region it belongs to.
        let reach = (gain_db / MOVE_DB).clamp(-1.0, 1.0) * region.height() * 0.5;
        if reach.abs() > 0.4 {
            let column = egui::Rect::from_min_max(
                egui::pos2(region.left() + 2.0, centre.min(centre - reach)),
                egui::pos2(region.right() - 2.0, centre.max(centre - reach)),
            );
            shapes.push(egui::Shape::rect_filled(
                column,
                0.0,
                hues[i].gamma_multiply(0.55),
            ));
        }
    }
    // The scale along the head of the strip: minor ticks every tenth of
    // a decade, majors on the decades, so the two seams can be read as
    // frequencies and not only as places.
    for step in 0..=30 {
        let t = step as f32 / 30.0;
        let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
        let decade = (hz.log10().fract() < 0.02) || (hz.log10().fract() > 0.98);
        let x = field.left() + t * field.width();
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.top()),
                egui::pos2(x, field.top() + if decade { 5.0 } else { 2.5 }),
            ],
            Weight::Hair,
            edge.gamma_multiply(if decade { 0.9 } else { 0.5 }),
        );
    }
    // The two seams, where the crossover actually cuts.
    for x in [edges[1], edges[2]] {
        chrome::trace(
            &mut shapes,
            &[egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
            Weight::Heavy,
            alpha.live_dim.color,
        );
    }
    // The reticle: it closes on whichever band the keyboard is standing
    // in, so the strip says where your hand is without a second cursor.
    let standing = face.selected.and_then(|param| {
        [
            (p::LOW, 0usize),
            (p::LOW_DB, 0),
            (p::MID, 1),
            (p::MID_DB, 1),
            (p::HIGH, 2),
            (p::HIGH_DB, 2),
        ]
        .into_iter()
        .find_map(|(id, band)| (id as usize == param).then_some(band))
    });
    if let Some(band) = standing {
        let region = egui::Rect::from_min_max(
            egui::pos2(edges[band] + 2.0, field.top() + 2.0),
            egui::pos2(edges[band + 1] - 2.0, field.bottom() - 2.0),
        );
        if region.is_positive() {
            chrome::brackets(&mut shapes, region, 7.0, Weight::Hair, hues[band]);
            // The cross-hairs a reticle has, short, off each edge's
            // middle — enough to read as aim, not enough to be a grid.
            for (a, b) in [
                (
                    egui::pos2(region.center().x, region.top()),
                    egui::pos2(region.center().x, region.top() + 5.0),
                ),
                (
                    egui::pos2(region.center().x, region.bottom() - 5.0),
                    egui::pos2(region.center().x, region.bottom()),
                ),
            ] {
                chrome::trace(&mut shapes, &[a, b], Weight::Hair, hues[band]);
            }
        }
    }
    // Registration marks at the strip's corners.
    chrome::corner_pads(&mut shapes, lay.strip.shrink(2.0), edge);
    chrome::panel_frame_variant(&mut shapes, lay.low_hz, Weight::Hair, edge, 1);
    chrome::panel_frame_variant(&mut shapes, lay.high_hz, Weight::Hair, edge, 2);
    painter.extend(shapes);

    // ---- The words and the rows. -------------------------------------
    // Every word goes through the ledger, which measures where it lands
    // and refuses, in a debug build, to let two of them share a place.
    let mut words = tool::Ledger::new(painter, font.clone(), "SPLIT");
    let mut label = |at: egui::Pos2, align: egui::Align2, text: String, ink: egui::Color32| {
        words.text(at, align, text, ink);
    };
    let hz_word = |hz: f32| {
        if hz >= 1000.0 {
            format!("{:.1}k", hz / 1000.0)
        } else {
            format!("{hz:.0}")
        }
    };
    label(
        egui::pos2(lay.strip.left() + 8.0, lay.strip.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "BANDS".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.strip.right() - 8.0, lay.strip.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if resting {
            "WIRE".to_owned()
        } else {
            format!("+/-{MOVE_DB:.0}dB")
        },
        if resting { edge } else { alpha.live.color },
    );
    // The system words. Every one of them is a fact about the section
    // rather than a word that looks like one: LR4 is the crossover's
    // real order, the link is real and is by the louder side so the
    // image holds, and AVG is the long average a band is held against —
    // the number that lets the amount work with no threshold at all.
    {
        // On the header line, after the section's own word: the strip's
        // foot is where the two corners are addressed.
        let mut x = lay.strip.left()
            + 8.0
            + painter
                .layout_no_wrap("BANDS".to_owned(), font.clone(), ink)
                .rect
                .width()
            + 14.0;
        for (word, tone) in [
            ("LR4".to_owned(), edge),
            ("LINK L/R".to_owned(), edge),
            (format!("AVG {:.1}s", p::AVERAGE_MS / 1000.0), edge),
            (
                if quiet {
                    "IDLE".to_owned()
                } else {
                    "TRACKING".to_owned()
                },
                if quiet { edge } else { alpha.live.color },
            ),
        ] {
            let w = painter
                .layout_no_wrap(word.clone(), font.clone(), ink)
                .rect
                .width();
            if x + w > lay.strip.right() - 8.0 {
                break;
            }
            label(
                egui::pos2(x, lay.strip.top() + 2.0),
                egui::Align2::LEFT_TOP,
                word,
                tone,
            );
            x += w + 10.0;
        }
    }
    // Each region is numbered and named where it stands, with a pip
    // that lights when that band is actually moving.
    for (i, word) in ["LO", "MID", "HI"].into_iter().enumerate() {
        let region = egui::Rect::from_min_max(
            egui::pos2(edges[i], field.top()),
            egui::pos2(edges[i + 1], field.bottom()),
        );
        if region.width() < 34.0 {
            continue;
        }
        label(
            egui::pos2(region.left() + 5.0, field.top() + 7.0),
            egui::Align2::LEFT_TOP,
            format!("{:02} {word}", i + 1),
            hues[i].gamma_multiply(0.85),
        );
        let working = !quiet && moved[i].abs() > 0.2;
        let mut pip = Vec::new();
        chrome::pad(
            &mut pip,
            egui::pos2(region.right() - 7.0, field.top() + 11.0),
            chrome::PAD,
            if working { hues[i] } else { edge },
            working,
        );
        painter.extend(pip);
        if working {
            label(
                egui::pos2(region.right() - 14.0, field.top() + 7.0),
                egui::Align2::RIGHT_TOP,
                format!("{:+.1}", moved[i]),
                hues[i],
            );
        }
    }
    for (rect, word, hz) in [
        (lay.low_hz, "LO/MID", low_hz),
        (lay.high_hz, "MID/HI", high_hz),
    ] {
        label(
            egui::pos2(rect.left() + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            edge,
        );
        label(
            egui::pos2(rect.right() - 4.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            hz_word(hz),
            ink,
        );
    }
    // The widest word each row keeps room for, measured.
    let gutter = ["LOW", "MID", "HIGH"]
        .into_iter()
        .map(|word| {
            painter
                .layout_no_wrap(word.to_owned(), font.clone(), ink)
                .rect
                .width()
        })
        .fold(0.0f32, f32::max)
        + 6.0;
    let names = ["LOW", "MID", "HIGH"];
    for i in 0..3 {
        let hue = hues[i];
        let row = lay.amount[i];
        label(
            egui::pos2(row.left() + 2.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            names[i].to_owned(),
            hue,
        );
        // The amount, bipolar about the average it holds against: to the
        // right it holds the band in, to the left it lets it out.
        let bar = egui::Rect::from_min_max(
            egui::pos2(row.left() + gutter, row.center().y - 4.0),
            egui::pos2(row.right() - 88.0, row.center().y + 4.0),
        );
        if bar.is_positive() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(bar.left(), bar.center().y - 0.5),
                    egui::pos2(bar.right(), bar.center().y + 0.5),
                ),
                0.0,
                edge.gamma_multiply(0.5),
            );
            let mid = bar.center().x;
            let reach = (amounts[i] / 100.0).clamp(-1.0, 1.0) * bar.width() * 0.5;
            if reach.abs() > 0.5 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(mid.min(mid + reach), bar.top()),
                        egui::pos2(mid.max(mid + reach), bar.bottom()),
                    ),
                    0.0,
                    hue,
                );
            }
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(mid - 0.5, bar.top() - 2.0),
                    egui::pos2(mid + 0.5, bar.bottom() + 2.0),
                ),
                0.0,
                edge,
            );
        }
        // The amount says which way it is working, in a word.
        label(
            egui::pos2(row.right() - 48.0, row.center().y),
            egui::Align2::RIGHT_CENTER,
            if amounts[i] > 0.0 {
                "HOLD".to_owned()
            } else if amounts[i] < 0.0 {
                "OPEN".to_owned()
            } else {
                "--".to_owned()
            },
            if amounts[i] == 0.0 { edge } else { hue },
        );
        label(
            egui::pos2(row.right() - 4.0, row.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{:+.0}", amounts[i]),
            if amounts[i] == 0.0 { edge } else { ink },
        );
        // The band's own ballistics, printed: the same amount does a
        // different thing to a kick than to a hi-hat, and this is why.
        let gain_rect = lay.gain[i];
        label(
            egui::pos2(gain_rect.left() + 2.0, gain_rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{:.0}/{:.0}", p::ATTACK_MS[i], p::RELEASE_MS[i]),
            edge,
        );
        label(
            egui::pos2(gain_rect.right() - 2.0, gain_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{:+.1}", gains[i]),
            if gains[i] == 0.0 { edge } else { ink },
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
    fn every_split_control_has_its_own_place() {
        let face = split_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 8);
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
    }

    /// The three band rows stand in frequency order, and each one's
    /// amount and gain sit side by side rather than nested.
    #[test]
    fn the_rows_read_low_to_high() {
        let face = split_face(glass());
        for pair in face.amount.windows(2) {
            assert!(
                pair[0].bottom() <= pair[1].top(),
                "the rows are out of order"
            );
        }
        for i in 0..3 {
            assert!(face.amount[i].right() <= face.gain[i].left());
            assert_eq!(face.amount[i].y_range(), face.gain[i].y_range());
        }
        // The two corners share the strip's foot, one each side.
        assert!(face.low_hz.right() <= face.high_hz.left());
        assert_eq!(face.low_hz.y_range(), face.high_hz.y_range());
    }

    /// The strip is the biggest thing on the card: it is the crossover
    /// and the meter at once.
    #[test]
    fn the_strip_owns_the_glass() {
        let face = split_face(glass());
        assert!(face.strip.height() > face.amount[0].height() * 3.0);
        assert!(face.strip.bottom() <= face.amount[0].top());
    }
}
