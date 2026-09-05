//! PREAMP's face: a VU on an arc, a transformer bank, and five
//! instruments that ARE the five parameters.

use super::*;
use crate::ui::chrome;

/// The five PREAMP parameters have five different instruments. Their
/// rectangles are authored together so the keyboard address and the
/// thing it illuminates cannot drift apart.
#[derive(Clone, Copy, Debug)]
struct PreampFace {
    meter: egui::Rect,
    trim: egui::Rect,
    iron: egui::Rect,
    character: egui::Rect,
    phase: egui::Rect,
    colour: egui::Rect,
}

impl PreampFace {
    fn controls(self) -> [(u32, egui::Rect); 5] {
        use crate::params::console::preamp as p;
        [
            (p::TRIM, self.trim),
            (p::IRON, self.iron),
            (p::CHARACTER, self.character),
            (p::PHASE, self.phase),
            (p::COLOUR, self.colour),
        ]
    }

    fn control(self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }
}

fn preamp_face(glass: egui::Rect) -> PreampFace {
    let x = glass.shrink2(egui::vec2(5.0, 0.0));
    let meter = egui::Rect::from_min_max(
        x.min,
        egui::pos2(x.right(), (x.top() + FIGURE_MAX_H).min(x.bottom())),
    );
    let controls = egui::Rect::from_min_max(
        egui::pos2(x.left(), (meter.bottom() + 4.0).min(x.bottom())),
        egui::pos2(x.right(), (x.bottom() - 4.0).max(meter.bottom() + 4.0)),
    );
    let gap = 3.0;
    let usable = (controls.height() - gap * 3.0).max(4.0);
    let trim_h = usable * 0.20;
    let iron_h = usable * 0.30;
    let character_h = usable * 0.20;
    let buttons_h = (usable - trim_h - iron_h - character_h).max(1.0);
    let trim = egui::Rect::from_min_size(controls.min, egui::vec2(controls.width(), trim_h));
    let iron = egui::Rect::from_min_size(
        egui::pos2(controls.left(), trim.bottom() + gap),
        egui::vec2(controls.width(), iron_h),
    );
    let character = egui::Rect::from_min_size(
        egui::pos2(controls.left(), iron.bottom() + gap),
        egui::vec2(controls.width(), character_h),
    );
    let button_row = egui::Rect::from_min_size(
        egui::pos2(controls.left(), character.bottom() + gap),
        egui::vec2(controls.width(), buttons_h),
    );
    let button_gap = 4.0;
    let button_w = ((button_row.width() - button_gap) * 0.5).max(1.0);
    let phase = egui::Rect::from_min_size(button_row.min, egui::vec2(button_w, buttons_h));
    let colour = egui::Rect::from_min_size(
        egui::pos2(phase.right() + button_gap, button_row.top()),
        egui::vec2(button_w, buttons_h),
    );
    PreampFace {
        meter,
        trim,
        iron,
        character,
        phase,
        colour,
    }
}

impl Layout for PreampFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        PreampFace::controls(*self).to_vec()
    }
}

/// PREAMP: the meter remains the shared signal picture, then every
/// parameter gets its own instrument. TRM is a small bipolar bar;
/// IRON is the large heat bar; CHARACTER is a pair of transfer
/// curves; PHASE is the phase glyph; COLOUR is the CLR pad. The
/// keyboard cursor lives inside whichever instrument it addresses.
pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::preamp as p;
    let painter = face.painter;
    let piece = face.piece;
    let glass = face.glass;
    // The mark is claimed through the layout, so the parameter index
    // itself is never needed here.
    let phase = face.phase;
    let alpha = face.alpha;
    let level = face.level;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let value = |param: u32| face.value(param);
    let trim = value(p::TRIM);
    let iron = value(p::IRON) / 100.0;
    let steel = value(p::CHARACTER) >= 0.5;
    let flip = value(p::PHASE) >= 0.5;
    let colour = value(p::COLOUR) >= 0.5;
    let lay = preamp_face(glass);
    let trim_place = painter.ctx().animate_value_with_time(
        egui::Id::new(("stage-preamp-trim", piece.index)),
        ((trim + 24.0) / 48.0).clamp(0.0, 1.0),
        0.14,
    );
    let iron_place = painter.ctx().animate_value_with_time(
        egui::Id::new(("stage-preamp-iron", piece.index)),
        iron,
        0.22,
    );
    let mut shapes = Vec::new();

    // A nested, solid meter subassembly sits inside the screen. The
    // well-coloured bezel and black inner glass make depth using only
    // plane changes and hard shadows.
    chrome::panel_variant(
        &mut shapes,
        lay.meter,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge)),
        2,
    );
    let meter_glass = lay.meter.shrink(3.0);
    chrome::panel_variant(
        &mut shapes,
        meter_glass,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );

    // The VU: an arc from −20 to +3, the top three dB in the live
    // ink, the needle from the pivot at the channel's level. It is
    // not a sixth control: it is what comes out of the five below.
    let meter = meter_glass.shrink2(egui::vec2(4.0, 2.0));
    let pivot = preamp_pivot(meter);
    let radius = (meter.height() - 10.0).min(meter.width() * 0.31);
    let (start, end) = (150.0, 30.0);
    let arc: Vec<egui::Pos2> = (0..=24)
        .map(|i| on_arc(pivot, radius, start + (end - start) * i as f32 / 24.0))
        .collect();
    chrome::trace(&mut shapes, &arc, Weight::Heavy, edge.gamma_multiply(1.25));
    let deg_of = |db: f32| start + (end - start) * ((db + 20.0) / 23.0).clamp(0.0, 1.0);
    let hot: Vec<egui::Pos2> = (0..=6)
        .map(|i| {
            on_arc(
                pivot,
                radius,
                deg_of(0.0) + (end - deg_of(0.0)) * i as f32 / 6.0,
            )
        })
        .collect();
    chrome::trace(&mut shapes, &hot, Weight::Heavy, alpha.live_dim.color);
    for db in [-20.0, -10.0, -5.0, 0.0, 3.0] {
        let d = deg_of(db);
        chrome::trace(
            &mut shapes,
            &[
                on_arc(pivot, radius - 4.0, d),
                on_arc(pivot, radius + 1.0, d),
            ],
            Weight::Hair,
            if db >= 0.0 {
                alpha.live_dim.color
            } else {
                edge
            },
        );
    }
    // The transfer has its own little scope to the LEFT of the VU.
    // Keeping its right edge clear of the arc means the curve can
    // bend hard without ever becoming a second meter needle.
    let chord_right = (pivot.x - radius * 0.90 - 3.0)
        .max(meter.left() + 18.0)
        .min(meter.right());
    let chord = egui::Rect::from_min_max(
        egui::pos2(meter.left() + 2.0, meter.top() + 7.0),
        egui::pos2(chord_right, meter.bottom() - 7.0),
    );
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(chord.left(), chord.center().y),
            egui::pos2(chord.right(), chord.center().y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.45),
    );
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(chord.center().x, chord.top()),
            egui::pos2(chord.center().x, chord.bottom()),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.45),
    );
    let curve: Vec<egui::Pos2> = (0..=20)
        .map(|i| {
            let x = -1.0 + 2.0 * i as f32 / 20.0;
            let y = crate::console::preamp_curve::transfer(iron, steel, x);
            egui::pos2(
                chord.left() + (x + 1.0) * 0.5 * chord.width(),
                chord.bottom() - (y + 1.0) * 0.5 * chord.height(),
            )
        })
        .collect();
    chrome::trace(
        &mut shapes,
        &curve,
        Weight::Hair,
        if iron > 0.0 {
            tool::mix_ink(
                alpha.live_dim.color,
                alpha.jeopardy_latent.color,
                iron * 0.65,
            )
        } else {
            edge.gamma_multiply(0.5)
        },
    );
    // The needle.
    let db = level.map_or(-60.0, |peak| {
        if peak <= 1e-6 {
            -60.0
        } else {
            20.0 * peak.log10()
        }
    });
    let needle = deg_of(db);
    chrome::trace(
        &mut shapes,
        &[pivot, on_arc(pivot, radius - 2.0, needle)],
        Weight::Heavy,
        if db >= 0.0 { alpha.live.color } else { live },
    );
    chrome::pad(&mut shapes, pivot, chrome::PAD, ink, true);
    chrome::pad(
        &mut shapes,
        egui::pos2(meter_glass.right() - 7.0, meter_glass.top() + 7.0),
        chrome::PAD - 1.0,
        if db >= 0.0 {
            alpha.jeopardy_active.color
        } else {
            edge
        },
        db >= 0.0,
    );

    // TRM: deliberately little. It is bipolar about the bright
    // centre post, with only the run between unity and the smooth
    // moving cursor awake.
    chrome::panel_frame_variant(&mut shapes, lay.trim, Weight::Hair, edge, 3);
    let trim_bar = egui::Rect::from_min_max(
        egui::pos2(lay.trim.left() + 30.0, lay.trim.center().y - 5.0),
        egui::pos2(lay.trim.right() - 6.0, lay.trim.center().y + 5.0),
    );
    let trim_segments = 17usize;
    for i in 0..trim_segments {
        let t = i as f32 / (trim_segments - 1) as f32;
        let x = egui::lerp(trim_bar.x_range(), t);
        let from = trim_place.min(0.5);
        let to = trim_place.max(0.5);
        let awake = t >= from - 0.001 && t <= to + 0.001;
        let cursor = (t - trim_place).abs() < 0.5 / (trim_segments - 1) as f32;
        let h = if cursor {
            trim_bar.height()
        } else if i % 4 == 0 {
            7.0
        } else {
            4.0
        };
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, trim_bar.center().y - h * 0.5),
                egui::pos2(x, trim_bar.center().y + h * 0.5),
            ],
            if cursor { Weight::Heavy } else { Weight::Hair },
            if awake {
                ink
            } else {
                edge.gamma_multiply(0.65)
            },
        );
    }
    let unity_x = egui::lerp(trim_bar.x_range(), 0.5);
    chrome::pad(
        &mut shapes,
        egui::pos2(unity_x, trim_bar.center().y),
        chrome::PAD - 2.0,
        alpha.focus.color,
        true,
    );

    // IRON: a larger bank. More of it wakes with drive and every
    // live segment moves from the bone ink toward the alphabet's
    // hot hue as the stage is leaned on.
    chrome::panel_variant(
        &mut shapes,
        lay.iron,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge)),
        1,
    );
    let iron_bar = egui::Rect::from_min_max(
        egui::pos2(lay.iron.left() + 38.0, lay.iron.top() + 6.0),
        egui::pos2(lay.iron.right() - 6.0, lay.iron.bottom() - 6.0),
    );
    let iron_segments = 14usize;
    for i in 0..iron_segments {
        let n = iron_segments as f32;
        let t0 = i as f32 / n;
        let t1 = (i + 1) as f32 / n;
        let at = (i as f32 + 0.5) / n;
        let cell = egui::Rect::from_min_max(
            egui::pos2(
                egui::lerp(iron_bar.x_range(), t0),
                iron_bar.bottom() - iron_bar.height() * (0.45 + at * 0.55),
            ),
            egui::pos2(egui::lerp(iron_bar.x_range(), t1) - 1.0, iron_bar.bottom()),
        );
        let awake = at <= iron_place;
        let warmth = (iron_place * 0.78 + at * 0.22).clamp(0.0, 1.0);
        shapes.push(egui::Shape::rect_filled(
            cell,
            0.0,
            if awake {
                tool::mix_ink(ink, alpha.jeopardy_active.color, warmth)
            } else {
                edge.gamma_multiply(0.55)
            },
        ));
    }

    // CHARACTER: not another bar. The two actual transfer families
    // lay each other; the selected path is the readable one.
    chrome::panel_frame_variant(&mut shapes, lay.character, Weight::Hair, edge, 2);
    let split_x = lay.character.center().x;
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(split_x, lay.character.top() + 3.0),
            egui::pos2(split_x, lay.character.bottom() - 3.0),
        ],
        Weight::Hair,
        edge,
    );
    for (right, is_steel) in [(false, false), (true, true)] {
        let half = if right {
            egui::Rect::from_min_max(egui::pos2(split_x, lay.character.top()), lay.character.max)
        } else {
            egui::Rect::from_min_max(
                lay.character.min,
                egui::pos2(split_x, lay.character.bottom()),
            )
        };
        let chosen = steel == is_steel;
        let plot = egui::Rect::from_min_max(
            egui::pos2(half.left() + 20.0, half.top() + 4.0),
            egui::pos2(half.right() - 5.0, half.bottom() - 4.0),
        );
        let curve: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let x = -1.0 + 2.0 * i as f32 / 12.0;
                let y = crate::console::preamp_curve::transfer(0.72, is_steel, x);
                egui::pos2(
                    plot.left() + (x + 1.0) * 0.5 * plot.width(),
                    plot.bottom() - (y + 1.0) * 0.5 * plot.height(),
                )
            })
            .collect();
        chrome::trace(
            &mut shapes,
            &curve,
            if chosen { Weight::Heavy } else { Weight::Hair },
            if chosen {
                ink
            } else {
                edge.gamma_multiply(0.7)
            },
        );
        chrome::pad(
            &mut shapes,
            egui::pos2(half.left() + 12.0, half.center().y),
            chrome::PAD - 1.0,
            if chosen { ink } else { edge },
            chosen,
        );
    }

    // PHASE and COLOUR are switches, so they are buttons and
    // nothing else: the existing phase glyph, and CLR.
    let button = |shapes: &mut Vec<egui::Shape>,
                  rect: egui::Rect,
                  on: bool,
                  lit: egui::Color32,
                  variant: u8| {
        chrome::panel_variant(
            shapes,
            rect,
            Some(if on { lit } else { alpha.ground.color }),
            alpha.ground.color,
            Some((Weight::Hair, if on { lit } else { edge })),
            variant,
        );
        chrome::pad(
            shapes,
            egui::pos2(rect.right() - 7.0, rect.top() + 7.0),
            chrome::PAD - 1.0,
            if on { alpha.ground.color } else { edge },
            on,
        );
    };
    button(&mut shapes, lay.phase, flip, alpha.jeopardy_active.color, 3);
    button(&mut shapes, lay.colour, colour, alpha.live.color, 0);
    painter.extend(shapes);

    let button_ink = |on: bool| if on { alpha.ground.color } else { ink };
    let meter_font = egui::FontId::monospace((font.size * 0.72).max(7.0));
    painter.text(
        egui::pos2(meter_glass.left() + 8.0, meter_glass.top() + 4.0),
        egui::Align2::LEFT_TOP,
        "XFR",
        meter_font.clone(),
        edge,
    );
    painter.text(
        egui::pos2(meter_glass.right() - 13.0, meter_glass.top() + 3.0),
        egui::Align2::RIGHT_TOP,
        "PK",
        meter_font.clone(),
        if db >= 0.0 {
            alpha.jeopardy_active.color
        } else {
            edge
        },
    );
    painter.text(
        egui::pos2(pivot.x, meter_glass.top() + 3.0),
        egui::Align2::CENTER_TOP,
        "VU / dB",
        meter_font,
        edge,
    );
    painter.text(
        egui::pos2(lay.trim.left() + 6.0, lay.trim.center().y),
        egui::Align2::LEFT_CENTER,
        "TRM",
        font.clone(),
        ink,
    );
    painter.text(
        egui::pos2(lay.iron.left() + 6.0, lay.iron.center().y),
        egui::Align2::LEFT_CENTER,
        "IRON",
        font.clone(),
        tool::mix_ink(ink, alpha.jeopardy_active.color, iron_place),
    );
    painter.text(
        egui::pos2(lay.character.left() + 7.0, lay.character.center().y),
        egui::Align2::LEFT_CENTER,
        "FE",
        font.clone(),
        if steel { edge } else { ink },
    );
    painter.text(
        egui::pos2(lay.character.center().x + 7.0, lay.character.center().y),
        egui::Align2::LEFT_CENTER,
        "ST",
        font.clone(),
        if steel { ink } else { edge },
    );
    painter.text(
        lay.phase.center(),
        egui::Align2::CENTER_CENTER,
        "Ø",
        font.clone(),
        button_ink(flip),
    );
    painter.text(
        lay.colour.center(),
        egui::Align2::CENTER_CENTER,
        "CLR",
        font.clone(),
        button_ink(colour),
    );

    // The cursor is not a sixth row below the drawing. Four bright
    // corners sit just inside the addressed instrument itself.
    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    #[test]
    fn every_preamp_parameter_has_one_instrument() {
        use crate::params::console::preamp as p;
        let glass = egui::Rect::from_min_size(egui::pos2(40.0, 500.0), egui::vec2(158.0, 200.0));
        let face = preamp_face(glass);
        let controls = face.controls();
        assert_eq!(
            controls.map(|(id, _)| id),
            [p::TRIM, p::IRON, p::CHARACTER, p::PHASE, p::COLOUR]
        );
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert_eq!(face.control(*id as usize), Some(*rect));
            assert!(glass.contains_rect(*rect), "parameter {id} left the glass");
            assert!(rect.is_positive(), "parameter {id} lost its instrument");
            assert!(
                !face.meter.intersects(*rect),
                "parameter {id} invaded the meter"
            );
            for (other_id, other) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other),
                    "parameters {id} and {other_id} overlap"
                );
            }
        }
    }

    #[test]
    fn iron_owns_the_big_bar_and_the_switches_share_the_foot() {
        let glass = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(158.0, 200.0));
        let face = preamp_face(glass);
        assert!(face.iron.height() > face.trim.height());
        assert!(face.iron.height() > face.character.height());
        assert_eq!(face.phase.y_range(), face.colour.y_range());
        assert!(face.phase.right() < face.colour.left());
    }
}
