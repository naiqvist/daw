//! PREAMP's face: three bays across the head of the strip — what comes
//! IN, what the STAGE does to it, and what that MAKES.
//!
//! Visual first. The VU is the signal picture, the transfer curve is the
//! stage itself drawn from the very function the core runs
//! (`console::preamp_curve::transfer`), and the harmonic ladder is a
//! MEASUREMENT of that same curve (`preamp_curve::harmonics`) rather
//! than an illustration of the manual — switch the stage from IRON to
//! STEEL and the even rungs collapse in front of you, because that is
//! what a symmetric curve does to even harmonics.
//!
//! Every word on the glass is a micro label. Nothing here asks to be
//! read before it is seen.

use super::*;
use crate::ui::chrome;

/// The five PREAMP parameters have five different instruments. Their
/// rectangles are authored together so the keyboard address and the
/// thing it illuminates cannot drift apart.
#[derive(Clone, Copy, Debug)]
struct PreampFace {
    /// The VU, top of the IN bay. Not a control: it is what comes out
    /// of the five that are.
    meter: egui::Rect,
    trim: egui::Rect,
    iron: egui::Rect,
    character: egui::Rect,
    phase: egui::Rect,
    colour: egui::Rect,
    /// The hero: the stage's transfer, big, in the middle bay.
    curve: egui::Rect,
    /// The harmonic ladder, and the colour tilt under it.
    ladder: egui::Rect,
    tilt: egui::Rect,
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

/// Three bays across the glass: IN, STAGE, MAKES.
fn preamp_face(glass: egui::Rect) -> PreampFace {
    let x = glass.shrink2(egui::vec2(5.0, 2.0));
    let gap = 6.0;
    let usable = (x.width() - gap * 2.0).max(30.0);
    let in_w = usable * 0.30;
    let stage_w = usable * 0.36;
    let science_w = usable - in_w - stage_w;
    let bay = |left: f32, w: f32| {
        egui::Rect::from_min_max(egui::pos2(left, x.top()), egui::pos2(left + w, x.bottom()))
    };
    let in_bay = bay(x.left(), in_w);
    let stage_bay = bay(in_bay.right() + gap, stage_w);
    let science_bay = bay(stage_bay.right() + gap, science_w);

    // IN: the VU takes the top, TRIM the foot.
    let trim_h = 26.0f32.min(in_bay.height() * 0.24);
    let meter = egui::Rect::from_min_max(
        in_bay.min,
        egui::pos2(
            in_bay.right(),
            (in_bay.bottom() - trim_h - 4.0).max(in_bay.top() + 8.0),
        ),
    );
    let trim =
        egui::Rect::from_min_max(egui::pos2(in_bay.left(), meter.bottom() + 4.0), in_bay.max);

    // STAGE: the curve is the hero, CHARACTER names it, IRON drives it.
    let character_h = 18.0f32.min(stage_bay.height() * 0.16);
    let iron_h = 22.0f32.min(stage_bay.height() * 0.20);
    let curve = egui::Rect::from_min_max(
        stage_bay.min,
        egui::pos2(
            stage_bay.right(),
            (stage_bay.bottom() - character_h - iron_h - 8.0).max(stage_bay.top() + 8.0),
        ),
    );
    let character = egui::Rect::from_min_size(
        egui::pos2(stage_bay.left(), curve.bottom() + 4.0),
        egui::vec2(stage_bay.width(), character_h),
    );
    let iron = egui::Rect::from_min_size(
        egui::pos2(stage_bay.left(), character.bottom() + 4.0),
        egui::vec2(
            stage_bay.width(),
            (stage_bay.bottom() - character.bottom() - 4.0).max(4.0),
        ),
    );

    // MAKES: the ladder, the colour tilt, and the two switches.
    let switch_h = 20.0f32.min(science_bay.height() * 0.18);
    let tilt_h = 40.0f32.min(science_bay.height() * 0.30);
    let ladder = egui::Rect::from_min_max(
        science_bay.min,
        egui::pos2(
            science_bay.right(),
            (science_bay.bottom() - switch_h - tilt_h - 8.0).max(science_bay.top() + 8.0),
        ),
    );
    let tilt = egui::Rect::from_min_size(
        egui::pos2(science_bay.left(), ladder.bottom() + 4.0),
        egui::vec2(science_bay.width(), tilt_h),
    );
    let switch_gap = 4.0;
    let switch_w = ((science_bay.width() - switch_gap) * 0.5).max(1.0);
    let switch_y = tilt.bottom() + 4.0;
    let phase = egui::Rect::from_min_size(
        egui::pos2(science_bay.left(), switch_y),
        egui::vec2(switch_w, (science_bay.bottom() - switch_y).max(4.0)),
    );
    let colour = egui::Rect::from_min_size(
        egui::pos2(phase.right() + switch_gap, switch_y),
        egui::vec2(switch_w, phase.height()),
    );
    PreampFace {
        meter,
        trim,
        iron,
        character,
        phase,
        colour,
        curve,
        ladder,
        tilt,
    }
}

impl Layout for PreampFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        PreampFace::controls(*self).to_vec()
    }
}

/// PREAMP, in three bays. IN is the VU and the trim; STAGE is the
/// transfer curve with the character that picks it and the iron that
/// drives it; MAKES is the harmonic ladder that curve produces, the
/// colour tilt, and the two switches.
pub(super) fn draw(face: &Face<'_>) {
    use crate::console::preamp_curve as pc;
    use crate::params::console::preamp as p;
    let painter = face.painter;
    let piece = face.piece;
    let glass = face.glass;
    let phase = face.phase;
    let alpha = face.alpha;
    let level = face.level;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    // ONE size on the whole card. Hierarchy is ink and case, never
    // type size — a card with three sizes on it reads as three cards.
    let micro = font.clone();
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
    let hot = tool::mix_ink(ink, alpha.jeopardy_latent.color, iron_place);
    let mut shapes = Vec::new();

    // ---- IN: a straight ladder, read bottom to top. -----------------
    chrome::panel_variant(
        &mut shapes,
        lay.meter,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    // What the section itself measured, falling back to the channel's
    // peak only when the section said nothing.
    let db = if face.said.level_db > -119.0 {
        face.said.level_db
    } else {
        level.map_or(-60.0, |peak| {
            if peak <= 1e-6 {
                -60.0
            } else {
                20.0 * peak.log10()
            }
        })
    };
    // The scale: −24 at the floor, +6 at the head, unity marked.
    let (floor_db, head_db) = (-24.0f32, 6.0f32);
    let place_of = |value: f32| ((value - floor_db) / (head_db - floor_db)).clamp(0.0, 1.0);
    let scale_w = 30.0f32.min(lay.meter.width() * 0.38);
    let column = egui::Rect::from_min_max(
        egui::pos2(lay.meter.left() + 7.0, lay.meter.top() + 22.0),
        egui::pos2(lay.meter.right() - scale_w - 4.0, lay.meter.bottom() - 7.0),
    );
    // The cells. Straight, stacked, and the ones above unity in alert,
    // so an over is a colour rather than a number to notice.
    let cells = 22usize;
    let cell_h = (column.height() / cells as f32).max(2.0);
    let lit = place_of(db);
    for i in 0..cells {
        let seat = (i as f32 + 0.5) / cells as f32;
        let cell = egui::Rect::from_min_max(
            egui::pos2(
                column.left(),
                column.bottom() - (i as f32 + 1.0) * cell_h + 1.0,
            ),
            egui::pos2(column.right(), column.bottom() - i as f32 * cell_h),
        );
        let over = seat >= place_of(0.0);
        shapes.push(egui::Shape::rect_filled(
            cell,
            0.0,
            if seat <= lit {
                if over {
                    alpha.jeopardy_latent.color
                } else {
                    live
                }
            } else if over {
                alpha.jeopardy_latent.color.gamma_multiply(0.14)
            } else {
                edge.gamma_multiply(0.25)
            },
        ));
    }
    // The scale beside it: ticks at the figures that matter, and the
    // unity line drawn across the column because that is the one place
    // on the ladder with a name.
    for mark in [6.0f32, 0.0, -12.0, -24.0] {
        let y = egui::lerp(column.y_range().flip(), place_of(mark));
        let unity = mark == 0.0;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(if unity { column.left() } else { column.right() }, y),
                egui::pos2(column.right() + 3.0, y),
            ],
            Weight::Hair,
            if unity { alpha.focus.color } else { edge },
        );
    }

    // TRIM: bipolar about a bright unity post, its figure to the right.
    chrome::panel_frame_variant(&mut shapes, lay.trim, Weight::Hair, edge, 3);
    let trim_bar = egui::Rect::from_min_max(
        egui::pos2(lay.trim.left() + 30.0, lay.trim.center().y - 5.0),
        egui::pos2(lay.trim.right() - 52.0, lay.trim.center().y + 5.0),
    );
    let trim_segments = 13usize;
    for i in 0..trim_segments {
        let t = i as f32 / (trim_segments - 1) as f32;
        let x = egui::lerp(trim_bar.x_range(), t);
        let from = trim_place.min(0.5);
        let to = trim_place.max(0.5);
        let awake = t >= from - 0.001 && t <= to + 0.001;
        let cursor = (t - trim_place).abs() < 0.5 / (trim_segments - 1) as f32;
        let h = if cursor { trim_bar.height() } else { 5.0 };
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
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(egui::lerp(trim_bar.x_range(), 0.5), trim_bar.top() - 2.0),
            egui::pos2(egui::lerp(trim_bar.x_range(), 0.5), trim_bar.bottom() + 2.0),
        ],
        Weight::Hair,
        alpha.focus.color,
    );

    // ---- STAGE: the transfer, big. ----------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.curve,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = lay.curve.shrink(6.0);
    for t in [0.25f32, 0.5, 0.75] {
        let weight = if (t - 0.5).abs() < 0.01 { 0.55 } else { 0.30 };
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(egui::lerp(plot.x_range(), t), plot.top()),
                egui::pos2(egui::lerp(plot.x_range(), t), plot.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(weight),
        );
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), egui::lerp(plot.y_range(), t)),
                egui::pos2(plot.right(), egui::lerp(plot.y_range(), t)),
            ],
            Weight::Hair,
            edge.gamma_multiply(weight),
        );
    }
    let at = |x: f32, y: f32| {
        egui::pos2(
            plot.left() + (x + 1.0) * 0.5 * plot.width(),
            plot.bottom() - (y + 1.0) * 0.5 * plot.height(),
        )
    };
    // The wire the stage would be if you let it go: the identity, so
    // how far from linear you are is a distance rather than a number.
    chrome::dashes(
        &mut shapes,
        &[at(-1.0, -1.0), at(1.0, 1.0)],
        0.0,
        Weight::Hair,
        edge.gamma_multiply(0.7),
    );
    let curve: Vec<egui::Pos2> = (0..=48)
        .map(|i| {
            let x = -1.0 + 2.0 * i as f32 / 48.0;
            at(x, pc::transfer(iron, steel, x))
        })
        .collect();
    chrome::trace(
        &mut shapes,
        &curve,
        Weight::Heavy,
        if iron > 0.0 {
            hot
        } else {
            edge.gamma_multiply(0.8)
        },
    );
    // Where the signal actually sits on that curve: the part of it the
    // channel is using, which a static curve never tells you.
    let drive_amp = if db > -119.0 {
        (10.0f32.powf(db / 20.0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if drive_amp > 0.001 {
        for sign in [-1.0f32, 1.0] {
            let x = sign * drive_amp;
            let point = at(x, pc::transfer(iron, steel, x));
            chrome::pad(&mut shapes, point, chrome::PAD, alpha.live.color, true);
        }
        chrome::trace(
            &mut shapes,
            &[at(-drive_amp, -1.0), at(-drive_amp, 1.0)],
            Weight::Hair,
            alpha.live_dim.color,
        );
        chrome::trace(
            &mut shapes,
            &[at(drive_amp, -1.0), at(drive_amp, 1.0)],
            Weight::Hair,
            alpha.live_dim.color,
        );
    }

    // CHARACTER: two cells, the chosen one in brackets.
    let half = lay.character.width() * 0.5;
    for (i, (word, is_steel)) in [("IRON", false), ("STEEL", true)].into_iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.character.left() + i as f32 * half, lay.character.top()),
            egui::vec2(half - 3.0, lay.character.height()),
        );
        let chosen = steel == is_steel;
        if chosen {
            chrome::brackets(&mut shapes, cell, 4.0, Weight::Hair, ink);
        }
        let _ = word;
    }

    // IRON: the heat bank, waking with drive.
    chrome::panel_variant(
        &mut shapes,
        lay.iron,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge)),
        1,
    );
    let iron_bar = egui::Rect::from_min_max(
        egui::pos2(lay.iron.left() + 34.0, lay.iron.top() + 5.0),
        egui::pos2(lay.iron.right() - 6.0, lay.iron.bottom() - 5.0),
    );
    let iron_segments = 16usize;
    for i in 0..iron_segments {
        let n = iron_segments as f32;
        let t0 = i as f32 / n;
        let t1 = (i + 1) as f32 / n;
        let seat = (i as f32 + 0.5) / n;
        let cell = egui::Rect::from_min_max(
            egui::pos2(
                egui::lerp(iron_bar.x_range(), t0),
                iron_bar.bottom() - iron_bar.height() * (0.45 + seat * 0.55),
            ),
            egui::pos2(egui::lerp(iron_bar.x_range(), t1) - 1.0, iron_bar.bottom()),
        );
        let awake = seat <= iron_place;
        let warmth = (iron_place * 0.78 + seat * 0.22).clamp(0.0, 1.0);
        shapes.push(egui::Shape::rect_filled(
            cell,
            0.0,
            if awake {
                tool::mix_ink(ink, alpha.jeopardy_latent.color, warmth)
            } else {
                edge.gamma_multiply(0.55)
            },
        ));
    }

    // ---- MAKES: what that curve does to a sine. ---------------------
    chrome::panel_variant(
        &mut shapes,
        lay.ladder,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    // Probed at the signal when there is one, and at a stated rest
    // level when there is not — a card that showed nothing at rest
    // would hide the one thing you are choosing between.
    let probe = if drive_amp > 0.01 {
        drive_amp
    } else {
        pc::REST_PROBE
    };
    let made = pc::harmonics(iron, steel, probe);
    let rungs = lay.ladder.shrink2(egui::vec2(7.0, 4.0));
    // The bars stop short of the foot: the rung numbers live there, and
    // a number under a bar is not a number inside it.
    let rungs = egui::Rect::from_min_max(
        egui::pos2(rungs.left(), rungs.top() + font.size + 3.0),
        egui::pos2(rungs.right(), rungs.bottom() - font.size - 2.0),
    );
    let bars = pc::HARMONICS - 1;
    let bar_w = (rungs.width() / bars as f32).max(2.0);
    for i in 0..bars {
        let harmonic = i + 2;
        let db = made.db[i + 1];
        let share = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
        let x0 = rungs.left() + i as f32 * bar_w;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0 + 1.0, rungs.bottom() - rungs.height() * share),
            egui::pos2(x0 + bar_w - 2.0, rungs.bottom()),
        );
        // Even rungs are the transformer's signature; odd rungs both
        // stages make. Colour says which is which, so switching the
        // stage reads as a shape changing rather than a word.
        let even = harmonic % 2 == 0;
        let tone = if share <= 0.001 {
            edge.gamma_multiply(0.45)
        } else if even {
            // The transformer's own signature, in the same heat its
            // drive is drawn in — the two belong to each other.
            alpha.jeopardy_latent.color
        } else {
            ink
        };
        if share <= 0.001 {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(bar.left(), rungs.bottom()),
                    egui::pos2(bar.right(), rungs.bottom()),
                ],
                Weight::Hair,
                tone,
            );
        } else {
            shapes.push(egui::Shape::rect_filled(bar, 0.0, tone));
        }
    }

    // COLOUR's tilt: the transformer's fixed shape, greyed when out.
    chrome::panel_frame_variant(&mut shapes, lay.tilt, Weight::Hair, edge, 2);
    // Under its own word, never beside it.
    let tilt_plot = egui::Rect::from_min_max(
        egui::pos2(lay.tilt.left() + 7.0, lay.tilt.top() + font.size + 5.0),
        egui::pos2(lay.tilt.right() - 7.0, lay.tilt.bottom() - 4.0),
    );
    let tilt_ink = if colour {
        alpha.live.color
    } else {
        edge.gamma_multiply(0.6)
    };
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(tilt_plot.left(), tilt_plot.center().y),
            egui::pos2(tilt_plot.right(), tilt_plot.center().y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.45),
    );
    // A lift under 100 Hz and a softening over 10 kHz, which is what
    // the switch actually does. Drawn across a decade scale.
    let tilt_curve: Vec<egui::Pos2> = (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let lift = (1.0 - t * 3.2).max(0.0);
            let soften = ((t - 0.72) / 0.28).max(0.0);
            let y = if colour {
                lift * 0.7 - soften * 0.7
            } else {
                0.0
            };
            egui::pos2(
                egui::lerp(tilt_plot.x_range(), t),
                tilt_plot.center().y - y * tilt_plot.height() * 0.5,
            )
        })
        .collect();
    chrome::trace(&mut shapes, &tilt_curve, Weight::Hair, tilt_ink);

    // The two switches.
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
    };
    button(&mut shapes, lay.phase, flip, alpha.jeopardy_active.color, 3);
    button(&mut shapes, lay.colour, colour, alpha.live.color, 0);
    painter.extend(shapes);

    // ---- The micro labels. Every one of them a measured thing. ------
    let button_ink = |on: bool| if on { alpha.ground.color } else { ink };
    let label = |at: egui::Pos2, align: egui::Align2, text: String, ink| {
        painter.text(at, align, text, micro.clone(), ink);
    };
    // How wide a word is, in this one type size.
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), micro.clone(), ink)
            .rect
            .width()
    };
    // The longest form that fits the room there is. A label that does
    // not fit is not shortened by the eye, it is drawn over its
    // neighbour — so the choice is made here, in advance, by measuring.
    let fits = |forms: &[String], room: f32| -> String {
        forms
            .iter()
            .find(|form| span(form) <= room)
            .or_else(|| forms.last())
            .cloned()
            .unwrap_or_default()
    };
    /// The inset every label on this card keeps from its own casing.
    const PAD_X: f32 = 8.0;
    // IN: the bay's word, the reading, and the ladder's own figures.
    label(
        egui::pos2(lay.meter.left() + PAD_X, lay.meter.top() + 3.0),
        egui::Align2::LEFT_TOP,
        "IN".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.meter.right() - PAD_X, lay.meter.top() + 3.0),
        egui::Align2::RIGHT_TOP,
        if db > -59.0 {
            fits(
                &[format!("{db:+.1}"), format!("{db:+.0}")],
                lay.meter.width() - span("IN") - PAD_X * 3.0,
            )
        } else {
            "--".to_owned()
        },
        if db >= 0.0 {
            alpha.jeopardy_latent.color
        } else {
            ink
        },
    );
    for mark in [6.0f32, 0.0, -12.0, -24.0] {
        let y = egui::lerp(column.y_range().flip(), place_of(mark));
        label(
            egui::pos2(column.right() + 7.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{mark:+.0}"),
            if mark == 0.0 { alpha.focus.color } else { edge },
        );
    }
    label(
        egui::pos2(lay.trim.left() + 5.0, lay.trim.center().y),
        egui::Align2::LEFT_CENTER,
        "TRM".to_owned(),
        ink,
    );
    label(
        egui::pos2(lay.trim.right() - PAD_X * 0.5, lay.trim.center().y),
        egui::Align2::RIGHT_CENTER,
        fits(
            &[
                format!("{trim:+.1}dB"),
                format!("{trim:+.0}dB"),
                format!("{trim:+.0}"),
            ],
            48.0,
        ),
        ink,
    );

    // STAGE: what the curve is, and whether it is running at all.
    label(
        egui::pos2(lay.curve.left() + PAD_X, lay.curve.top() + 3.0),
        egui::Align2::LEFT_TOP,
        "TRANSFER".to_owned(),
        edge,
    );
    // At the floor the stage is a wire to the sample and says so; the
    // node's own bypass reads exactly these conditions.
    let wire = iron <= 0.0 && trim == 0.0 && !flip && !colour;
    label(
        egui::pos2(lay.curve.right() - PAD_X, lay.curve.top() + 3.0),
        egui::Align2::RIGHT_TOP,
        fits(
            &[
                if wire { "WIRE" } else { "2x OS" }.to_owned(),
                if wire { "--" } else { "2x" }.to_owned(),
            ],
            lay.curve.width() - span("TRANSFER") - PAD_X * 3.0,
        ),
        if wire { edge } else { alpha.live.color },
    );
    for (i, word) in ["IRON", "STEEL"].into_iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.character.left() + i as f32 * half, lay.character.top()),
            egui::vec2(half - 3.0, lay.character.height()),
        );
        let chosen = steel == (i == 1);
        label(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            word.to_owned(),
            if chosen { ink } else { edge },
        );
    }
    label(
        egui::pos2(lay.iron.left() + 5.0, lay.iron.center().y),
        egui::Align2::LEFT_CENTER,
        "DRV".to_owned(),
        hot,
    );
    label(
        egui::pos2(lay.iron.right() - 5.0, lay.iron.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{:.0}", iron * 100.0),
        hot,
    );

    // MAKES: the ladder's word, its distortion, and its rungs.
    label(
        egui::pos2(lay.ladder.left() + PAD_X, lay.ladder.top() + 3.0),
        egui::Align2::LEFT_TOP,
        "THD".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.ladder.right() - PAD_X - 2.0, lay.ladder.top() + 3.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "--".to_owned()
        } else {
            fits(
                &[
                    format!("{:.1}%", made.thd * 100.0),
                    format!("{:.0}%", made.thd * 100.0),
                ],
                lay.ladder.width() - span("THD") - PAD_X * 3.0,
            )
        },
        if made.thd > 0.05 {
            alpha.jeopardy_latent.color
        } else {
            ink
        },
    );
    for i in 0..bars {
        label(
            egui::pos2(
                rungs.left() + (i as f32 + 0.5) * bar_w,
                rungs.bottom() + 2.0,
            ),
            egui::Align2::CENTER_TOP,
            format!("{}", i + 2),
            edge,
        );
    }
    label(
        egui::pos2(lay.tilt.left() + PAD_X, lay.tilt.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "TILT".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.tilt.right() - PAD_X, lay.tilt.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if colour { "IN" } else { "OUT" }.to_owned(),
        if colour { alpha.live.color } else { edge },
    );
    label(
        lay.phase.center(),
        egui::Align2::CENTER_CENTER,
        "PHASE".to_owned(),
        button_ink(flip),
    );
    label(
        lay.colour.center(),
        egui::Align2::CENTER_CENTER,
        "COLOUR".to_owned(),
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

    /// The three bays stand across the glass in signal order, and each
    /// instrument sits in the bay it belongs to.
    #[test]
    fn the_three_bays_stand_in_signal_order_across_the_glass() {
        let glass = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(430.0, 210.0));
        let face = preamp_face(glass);
        // IN, then STAGE, then MAKES — left to right, none overlapping.
        assert!(face.meter.right() <= face.curve.left());
        assert!(face.curve.right() <= face.ladder.left());
        // Within a bay, top to bottom.
        assert!(face.meter.bottom() <= face.trim.top());
        assert!(face.curve.bottom() <= face.character.top());
        assert!(face.character.bottom() <= face.iron.top());
        assert!(face.ladder.bottom() <= face.tilt.top());
        assert!(face.tilt.bottom() <= face.phase.top());
        assert_eq!(face.phase.y_range(), face.colour.y_range());
        assert!(face.phase.right() < face.colour.left());
        // The hero is the biggest picture on the card.
        assert!(face.curve.area() > face.ladder.area());
        assert!(face.curve.area() > face.tilt.area());
    }
}
