//! TONE's face: three bands hung from the response the engine is
//! actually running.

use super::*;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// TONE's three bands, each its own hue, so a glance says which lever
/// is which without a word being written. Warm at the bottom, violet
/// in the middle, the deck's own live cyan at the top: the order a
/// spectrum is drawn in everywhere.
const TONE_LO_INK: egui::Color32 = egui::Color32::from_rgb(255, 138, 62);
const TONE_MID_INK: egui::Color32 = egui::Color32::from_rgb(186, 122, 255);
const TONE_HI_INK: egui::Color32 = egui::Color32::from_rgb(139, 245, 255);

/// How many squares a band hangs at full boost or full cut, and so
/// how much each one is worth.
const TONE_CELLS: usize = 8;
/// The squares are SMALL. The curve is the picture; the stack under it
/// says how much of the lever that took, and should not shout it.
const TONE_CELL: f32 = 4.0;
const TONE_CELL_PITCH: f32 = 6.0;
/// The face's frequency span, in Hz: what the horizontal axis means.
const TONE_LOW_HZ: f32 = tool::LOW_HZ;
const TONE_HIGH_HZ: f32 = tool::HIGH_HZ;

/// Where `hz` falls across `field`. The desk's one octave axis, so a
/// corner at 1 kHz stands in the same place on TONE, CUT and FOUR.
fn tone_x(field: egui::Rect, hz: f32) -> f32 {
    tool::octave_x(field, hz)
}

/// TONE's seven parameters as seven places on the glass. Authored
/// together, so the key that addresses one and the art that answers
/// cannot drift apart.
#[derive(Clone, Copy, Debug)]
struct ToneFace {
    /// The whole picture: the curve's field and the bands' ground.
    field: egui::Rect,
    /// One column per band, the middle one wherever its frequency
    /// puts it.
    columns: [egui::Rect; 3],
    /// The rail the middle band slides along, and the kill pads.
    sweep: egui::Rect,
    kills: [egui::Rect; 3],
}

impl ToneFace {
    fn controls(self) -> [(u32, egui::Rect); 7] {
        use crate::params::console::tone as p;
        [
            (p::LO, self.columns[0]),
            (p::MID, self.columns[1]),
            (p::HI, self.columns[2]),
            (p::MID_HZ, self.sweep),
            (p::KILL_LO, self.kills[0]),
            (p::KILL_MID, self.kills[1]),
            (p::KILL_HI, self.kills[2]),
        ]
    }

    fn control(self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }
}

/// Where everything stands, given the glass and where the middle band
/// has been swept to.
fn tone_face(glass: egui::Rect, mid_hz: f32) -> ToneFace {
    use crate::params::console::tone as p;
    let inner = glass.shrink2(egui::vec2(6.0, 4.0));
    let kill_h = 12.0;
    let sweep_h = 9.0;
    let field = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(
            inner.right(),
            (inner.bottom() - kill_h - sweep_h - 6.0).max(inner.top() + 20.0),
        ),
    );
    let sweep = egui::Rect::from_min_max(
        egui::pos2(inner.left(), field.bottom() + 3.0),
        egui::pos2(inner.right(), field.bottom() + 3.0 + sweep_h),
    );
    let column_w = (field.width() * 0.18).clamp(14.0, 34.0);
    let column = |hz: f32| {
        let x = tone_x(field, hz).clamp(
            field.left() + column_w * 0.5,
            field.right() - column_w * 0.5,
        );
        egui::Rect::from_min_max(
            egui::pos2(x - column_w * 0.5, field.top()),
            egui::pos2(x + column_w * 0.5, field.bottom()),
        )
    };
    let columns = [column(p::LO_HZ), column(mid_hz), column(p::HI_HZ)];
    let kills = columns.map(|c| {
        egui::Rect::from_min_max(
            egui::pos2(c.center().x - kill_h * 0.5, sweep.bottom() + 3.0),
            egui::pos2(c.center().x + kill_h * 0.5, sweep.bottom() + 3.0 + kill_h),
        )
    });
    ToneFace {
        field,
        columns,
        sweep,
        kills,
    }
}

impl Layout for ToneFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        ToneFace::controls(*self).to_vec()
    }
}

/// TONE: three bands, three hues, and no words.
///
/// Each band is a stack of SQUARES standing on the zero line —
/// lit upward for a boost, downward for a cut, one square for
/// every two and a half dB. The middle band's stack SLIDES along
/// the lay to wherever its frequency is set, so sweeping it is
/// watching the band walk up the spectrum rather than watching a
/// number climb. Under each stack is its kill pad, which fills
/// with the band's own hue when the band is gone; and behind all
/// three, faintly, is the response the engine is actually running,
/// computed from the same coefficients, so what is drawn is what
/// is heard.
///
/// Nothing here is labelled. A square that is lit is a decibel
/// that is happening.
pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::tone as p;
    let painter = face.painter;
    let piece = face.piece;
    let glass = face.glass;
    let selected = face.selected;
    // Nothing on TONE runs on the beat: every figure on it is either a
    // coefficient or a hand.
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let value = |param: u32| face.value(param);
    let gains = [value(p::LO), value(p::MID), value(p::HI)];
    let kills = [
        value(p::KILL_LO) >= 0.5,
        value(p::KILL_MID) >= 0.5,
        value(p::KILL_HI) >= 0.5,
    ];
    let inks = [TONE_LO_INK, TONE_MID_INK, TONE_HI_INK];

    // The swept band walks rather than jumps.
    let mid_hz = painter.ctx().animate_value_with_time(
        egui::Id::new(("stage-tone-mid", piece.index)),
        value(p::MID_HZ),
        0.16,
    );
    let lay = tone_face(glass, mid_hz.max(1.0));
    let mut shapes = Vec::new();

    // The ground: a hairline lattice, the zero line bright across
    // the middle, and a tick at each decade so the axis is a
    // spectrum and not a strip.
    chrome::lattice(
        &mut shapes,
        lay.field,
        design::px(design::space::ROOM),
        edge.gamma_multiply(0.4),
    );
    let zero_y = lay.field.center().y;
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(lay.field.left(), zero_y),
            egui::pos2(lay.field.right(), zero_y),
        ],
        Weight::Hair,
        edge.gamma_multiply(1.2),
    );
    for hz in [100.0, 1_000.0, 10_000.0] {
        let x = tone_x(lay.field, hz);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, lay.field.bottom() - 3.0),
                egui::pos2(x, lay.field.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.8),
        );
    }

    // The response the engine is running, drawn from its own
    // coefficients: the one line on the lay that is measured
    // rather than set.
    let shape = Some(crate::console::tone_curve::Shape::of(&face.params));
    if let Some(shape) = shape {
        let sample_rate = 48_000.0;
        let curve: Vec<egui::Pos2> = (0..=48)
            .map(|i| {
                let at = i as f32 / 48.0;
                let hz = TONE_LOW_HZ * (TONE_HIGH_HZ / TONE_LOW_HZ).powf(at);
                let db = crate::console::tone_curve::response_db(&shape, sample_rate, hz);
                egui::pos2(
                    egui::lerp(lay.field.x_range(), at),
                    zero_y - (db / 18.0).clamp(-1.0, 1.0) * lay.field.height() * 0.46,
                )
            })
            .collect();
        chrome::trace(&mut shapes, &curve, Weight::Hair, alpha.ink.color);
    }

    // The three stacks, each HANGING FROM THE CURVE at its own
    // frequency rather than standing on the zero line. Where the
    // curve is is what the band did; the squares under it are how
    // much of the lever that took. They are small and quiet until
    // the keyboard is holding one, which is when they light.
    let db_at = |hz: f32| {
        shape.map_or(0.0, |shape| {
            crate::console::tone_curve::response_db(&shape, 48_000.0, hz)
        })
    };
    let curve_y = |db: f32| zero_y - (db / 18.0).clamp(-1.0, 1.0) * lay.field.height() * 0.46;
    let held = |band: usize| {
        selected.is_some_and(|param| {
            param == [p::LO, p::MID, p::HI][band] as usize
                || param == [p::KILL_LO, p::KILL_MID, p::KILL_HI][band] as usize
        })
    };
    for band in 0..3 {
        let column = lay.columns[band];
        let ink = inks[band];
        let x = column.center().x;
        let hz = [p::LO_HZ, mid_hz.max(1.0), p::HI_HZ][band];
        let lit = painter.ctx().animate_value_with_time(
            egui::Id::new(("stage-tone-band", piece.index, band)),
            (gains[band] / 15.0).clamp(-1.0, 1.0),
            0.14,
        );
        let killed = kills[band];
        let awake = held(band);
        // Where the curve stands over this band is where the stack
        // hangs from.
        let top = curve_y(db_at(hz)).clamp(
            lay.field.top() + 2.0,
            lay.field.bottom() - TONE_CELL_PITCH * 2.0,
        );
        let steps = if killed {
            TONE_CELLS
        } else {
            (lit.abs() * TONE_CELLS as f32).round() as usize
        };
        let reach = TONE_CELL_PITCH * (steps.max(1) as f32) + 3.0;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, top),
                egui::pos2(x, (top + reach).min(lay.field.bottom())),
            ],
            Weight::Hair,
            if awake {
                ink.gamma_multiply(0.8)
            } else {
                edge.gamma_multiply(0.7)
            },
        );
        for step in 0..steps {
            let y = top + 4.0 + TONE_CELL_PITCH * step as f32;
            if y > lay.field.bottom() - 2.0 {
                break;
            }
            let square =
                egui::Rect::from_center_size(egui::pos2(x, y), egui::Vec2::splat(TONE_CELL));
            if awake {
                shapes.push(egui::Shape::rect_filled(
                    square.expand(1.5),
                    0.0,
                    ink.gamma_multiply(0.3),
                ));
            }
            shapes.push(egui::Shape::rect_filled(
                square,
                0.0,
                if killed {
                    ink.gamma_multiply(if awake { 0.5 } else { 0.28 })
                } else if awake {
                    ink
                } else {
                    ink.gamma_multiply(0.62)
                },
            ));
        }
        if killed {
            let bottom = (top + reach).min(lay.field.bottom());
            let arm = 5.0;
            for (a, b) in [
                (egui::pos2(x - arm, top + 3.0), egui::pos2(x + arm, bottom)),
                (egui::pos2(x + arm, top + 3.0), egui::pos2(x - arm, bottom)),
            ] {
                chrome::trace(
                    &mut shapes,
                    &[a, b],
                    Weight::Heavy,
                    if awake { ink } else { ink.gamma_multiply(0.7) },
                );
            }
        }
    }

    // The sweep rail: the middle band's own axis, with its stack's
    // foot riding it and the two fixed bands marked as posts.
    chrome::rail(
        &mut shapes,
        egui::pos2(lay.sweep.left(), lay.sweep.center().y),
        egui::pos2(lay.sweep.right(), lay.sweep.center().y),
        &[0.0, 0.25, 0.5, 0.75, 1.0],
        edge.gamma_multiply(0.8),
    );
    for (band, ink) in [(0usize, TONE_LO_INK), (2, TONE_HI_INK)] {
        chrome::pad(
            &mut shapes,
            egui::pos2(lay.columns[band].center().x, lay.sweep.center().y),
            chrome::PAD - 2.0,
            ink.gamma_multiply(0.7),
            false,
        );
    }
    let rider = egui::Rect::from_center_size(
        egui::pos2(lay.columns[1].center().x, lay.sweep.center().y),
        egui::vec2(9.0, lay.sweep.height()),
    );
    shapes.push(egui::Shape::rect_filled(rider, 0.0, TONE_MID_INK));

    // The kill pads: hollow while the band sounds, filled in its
    // own hue the moment it does not.
    for band in 0..3 {
        let pad = lay.kills[band];
        if kills[band] {
            shapes.push(egui::Shape::rect_filled(pad, 0.0, inks[band]));
        } else {
            shapes.push(egui::Shape::rect_stroke(
                pad,
                0.0,
                egui::Stroke::new(Weight::Hair.px(), inks[band].gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            ));
        }
    }
    painter.extend(shapes);

    // The cursor: the house brackets around whichever instrument
    // the keyboard is holding, and nothing else on the lay bright.
    // The mark becomes the band it is standing on: the band's own hue
    // on its corners, leaning up for a boost and down for a cut, and
    // shut like a lid on a kill. What the cursor wears is what the
    // engine is doing to that band.
    face.mark_signed(
        &lay,
        match selected {
            Some(band @ 0..=2) => nav_cursor::Signature::Band {
                ink: inks[band],
                amount: (gains[band] / 15.0).clamp(-1.0, 1.0),
            },
            Some(3) => nav_cursor::Signature::Sweep(face.place(p::MID_HZ) * 2.0 - 1.0),
            Some(kill @ 4..=6) => {
                nav_cursor::Signature::Aperture(if kills[kill - 4] { 0.0 } else { 1.0 })
            }
            _ => nav_cursor::Signature::Plain,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    #[test]
    fn every_tone_parameter_has_one_instrument() {
        use crate::params::console::tone as p;
        let glass = egui::Rect::from_min_size(egui::pos2(20.0, 60.0), egui::vec2(200.0, 190.0));
        let face = tone_face(glass, 1_000.0);
        let controls = face.controls();
        for (_, rect) in controls {
            assert!(rect.is_positive(), "an instrument has no room");
            assert!(glass.contains_rect(rect), "an instrument left the glass");
        }
        for table in SectionKind::Tone.table() {
            assert!(
                face.control(table.id as usize).is_some(),
                "{} has no instrument",
                table.name
            );
        }
        // The three stacks stand apart, low to high, left to right.
        assert!(face.columns[0].right() <= face.columns[1].left());
        assert!(face.columns[1].right() <= face.columns[2].left());
        // And the middle one walks when it is swept.
        let low = tone_face(glass, 250.0).columns[1].center().x;
        let high = tone_face(glass, 5_000.0).columns[1].center().x;
        assert!(
            high > low + 20.0,
            "the swept band did not move: {low} to {high}"
        );
        // Each kill pad stands under its own band.
        for band in 0..3 {
            assert!(
                (face.kills[band].center().x - face.columns[band].center().x).abs() < 1.0,
                "kill {band} is not under its band"
            );
        }
        let _ = p::MID_HZ;
    }

    #[test]
    fn the_tone_axis_is_octaves_not_hertz() {
        let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 100.0));
        let a = tone_x(field, 100.0) - tone_x(field, 50.0);
        let b = tone_x(field, 8_000.0) - tone_x(field, 4_000.0);
        assert!((a - b).abs() < 0.5, "an octave is {a} here and {b} there");
        assert!(tone_x(field, 20.0) >= field.left());
        assert!(tone_x(field, 30_000.0) <= field.right());
    }
}
