//! DOOR's face: what the door is listening to, when it opens, and how
//! far it shuts.
//!
//! A gate is a switch with timing, so the card is two pictures of time
//! and one of frequency:
//!
//! - The **LANE** is the level with the threshold standing on it, and the
//!   hysteresis drawn as the band it really is — the door opens at the
//!   top of that band and does not shut until the bottom, which is why
//!   it never chatters. The live level rides the lane.
//! - The **ENVELOPE** is the opening itself, to scale in milliseconds:
//!   the two-millisecond lookahead before it, the attack, the hold, the
//!   release, and the floor RANGE lets it fall to. A door that ducks
//!   instead of slamming has a floor you can see. In RHYTHM the same
//!   picture becomes the beat grid, cut at the duty.
//! - The **KEY** is the sidechain's passband between its two filters, so
//!   "the door listens to the drum it is on" is a band on a scale rather
//!   than two numbers.

use super::*;
use crate::ui::chrome;

/// One numeric row in the right-hand column.
/// @tune 9..24 px
pub(super) const ROW_H: f32 = 17.0;
/// The room a row keeps for its word.
/// @tune 16..64 px
const GUTTER: f32 = 34.0;
/// The share of the glass the two time pictures take.
/// @tune 0.4..0.8
const TIME_SHARE: f32 = 0.56;
/// The gap between the card's two columns.
const COLUMN_GAP: f32 = 7.0;
/// The lane's window: the door's own threshold range.
const LANE_MIN_DB: f32 = -60.0;
const LANE_MAX_DB: f32 = 0.0;
/// The key plot's decades.
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;

/// DOOR's twelve parameters and the rectangle each one owns.
#[derive(Clone, Copy, Debug)]
struct DoorFace {
    mode: egui::Rect,
    threshold: egui::Rect,
    hysteresis: egui::Rect,
    attack: egui::Rect,
    hold: egui::Rect,
    release: egui::Rect,
    key_hp: egui::Rect,
    key_lp: egui::Rect,
    ratio: egui::Rect,
    range: egui::Rect,
    division: egui::Rect,
    duty: egui::Rect,
}

impl DoorFace {
    fn controls(self) -> [(u32, egui::Rect); 12] {
        use crate::params::console::door as p;
        [
            (p::MODE, self.mode),
            (p::THRESHOLD, self.threshold),
            (p::HYSTERESIS, self.hysteresis),
            (p::ATTACK, self.attack),
            (p::HOLD, self.hold),
            (p::RELEASE, self.release),
            (p::KEY_HP, self.key_hp),
            (p::KEY_LP, self.key_lp),
            (p::RATIO, self.ratio),
            (p::RANGE, self.range),
            (p::DIVISION, self.division),
            (p::DUTY, self.duty),
        ]
    }

    /// The envelope's whole picture: the three timing controls laid end
    /// to end, which is also the plot they are drawn in.
    fn envelope(self) -> egui::Rect {
        self.attack.union(self.hold).union(self.release)
    }

    /// The key plot: its two filters own a half each.
    fn key(self) -> egui::Rect {
        self.key_hp.union(self.key_lp)
    }
}

impl Layout for DoorFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        DoorFace::controls(*self).to_vec()
    }
}

fn door_face(glass: egui::Rect) -> DoorFace {
    let x = glass.shrink2(egui::vec2(5.0, 2.0));
    let left_w = (x.width() - COLUMN_GAP) * crate::tune!(TIME_SHARE);
    let left = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + left_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + COLUMN_GAP, x.top()), x.max);
    let row_h = crate::tune!(ROW_H);

    // Left: the mode, the lane, then the envelope with the whole rest.
    let mode = egui::Rect::from_min_size(left.min, egui::vec2(left.width(), row_h + 3.0));
    let lane_h = row_h + 8.0;
    let lane = egui::Rect::from_min_size(
        egui::pos2(left.left(), mode.bottom() + 3.0),
        egui::vec2(left.width(), lane_h),
    );
    // The threshold owns the whole lane. Hysteresis is a figure among
    // the figures: a cell on the lane could hold its word or its number
    // but not both, and a cell that holds neither is decoration.
    let threshold = lane;
    // The envelope, cut into its three times. They are drawn as one
    // picture and addressed as three, which is what they are.
    let env = egui::Rect::from_min_max(egui::pos2(left.left(), lane.bottom() + 4.0), left.max);
    let third = env.width() / 3.0;
    let slice = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(env.left() + i as f32 * third, env.top()),
            egui::vec2(third - 1.0, env.height()),
        )
    };
    let (attack, hold, release) = (slice(0), slice(1), slice(2));

    // Right: the key's passband, then the figures.
    let key_h = (right.height() * 0.34).clamp(46.0, 80.0);
    let key = egui::Rect::from_min_size(right.min, egui::vec2(right.width(), key_h));
    let half = key.width() * 0.5;
    let key_hp = egui::Rect::from_min_size(key.min, egui::vec2(half - 1.0, key.height()));
    let key_lp = egui::Rect::from_min_size(
        egui::pos2(key.left() + half, key.top()),
        egui::vec2(half, key.height()),
    );
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), key.bottom() + 4.0 + i as f32 * (row_h + 1.0)),
            egui::vec2(right.width(), row_h),
        )
    };
    DoorFace {
        mode,
        threshold,
        attack,
        hold,
        release,
        key_hp,
        key_lp,
        hysteresis: row(0),
        ratio: row(1),
        range: row(2),
        division: row(3),
        duty: row(4),
    }
}

/// Where a frequency stands across the key plot, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

/// Where a level stands along the lane.
fn place_of_db(db: f32) -> f32 {
    ((db - LANE_MIN_DB) / (LANE_MAX_DB - LANE_MIN_DB)).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::door as p;
    let painter = face.painter;
    let glass = face.glass;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = door_face(glass);
    let value = |id: u32| face.value(id);
    let rhythm = value(p::MODE) >= 0.5;
    let threshold = value(p::THRESHOLD);
    let hysteresis = value(p::HYSTERESIS);
    let attack = value(p::ATTACK);
    let hold = value(p::HOLD);
    let release = value(p::RELEASE);
    let range = value(p::RANGE);
    let ratio = value(p::RATIO);
    let key_hp = value(p::KEY_HP);
    let key_lp = value(p::KEY_LP);
    let division = value(p::DIVISION).round().clamp(0.0, 5.0) as usize;
    let duty = value(p::DUTY);
    // What the section measured of itself last block: the level it saw
    // and how far it is holding the door shut right now.
    let level_db = face.said.level_db;
    let shut_db = face.said.reduction_db.abs();
    let mut shapes = Vec::new();

    // ---- MODE: two cells, the standing one in brackets. --------------
    chrome::panel_frame_variant(&mut shapes, lay.mode, Weight::Hair, edge, 2);
    let mode_half = lay.mode.width() * 0.5;
    for i in 0..2 {
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.mode.left() + i as f32 * mode_half, lay.mode.top()),
            egui::vec2(mode_half, lay.mode.height()),
        );
        if rhythm == (i == 1) {
            chrome::brackets(&mut shapes, cell.shrink(2.0), 4.0, Weight::Hair, ink);
        }
    }

    // ---- LANE: the level, the threshold, and the hysteresis band. ----
    chrome::panel_variant(
        &mut shapes,
        lay.threshold,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let lane = egui::Rect::from_min_max(
        egui::pos2(
            lay.threshold.left() + crate::tune!(GUTTER),
            lay.threshold.top() + 4.0,
        ),
        egui::pos2(lay.threshold.right() - 44.0, lay.threshold.bottom() - 4.0),
    );
    if lane.is_positive() {
        // The signal, as far as it got.
        if level_db > LANE_MIN_DB {
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    lane.min,
                    egui::pos2(
                        lane.left() + place_of_db(level_db) * lane.width(),
                        lane.bottom(),
                    ),
                ),
                0.0,
                if level_db >= threshold {
                    alpha.live.color
                } else {
                    edge
                },
            ));
        }
        // The band between opening and shutting. The door opens at the
        // top of it and does not shut until the bottom — drawn, because
        // that gap is the whole reason it never chatters.
        let open_at = lane.left() + place_of_db(threshold) * lane.width();
        let shut_at = lane.left() + place_of_db(threshold - hysteresis) * lane.width();
        if hysteresis > 0.0 {
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(shut_at, lane.top()),
                    egui::pos2(open_at, lane.bottom()),
                ),
                0.0,
                alpha.jeopardy_latent.color.gamma_multiply(0.25),
            ));
        }
        for (x, weight) in [(open_at, Weight::Heavy), (shut_at, Weight::Hair)] {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(x, lane.top() - 2.0),
                    egui::pos2(x, lane.bottom() + 2.0),
                ],
                weight,
                alpha.jeopardy_latent.color,
            );
        }
    }

    // ---- ENVELOPE: the opening, to scale in milliseconds. ------------
    let env = lay.envelope();
    chrome::panel_variant(
        &mut shapes,
        env,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(env.left() + 7.0, env.top() + font.size + 5.0),
        egui::pos2(env.right() - 7.0, env.bottom() - font.size - 6.0),
    );
    // The floor the door falls to: RANGE is how far it shuts, and a
    // door that ducks rather than slams has a floor you can see.
    let floor = (1.0 - range / 80.0).clamp(0.0, 1.0);
    let span_ms = (p::LOOKAHEAD_MS + attack + hold + release).max(1.0);
    let x_at = |ms: f32| plot.left() + (ms / span_ms).clamp(0.0, 1.0) * plot.width();
    let y_at = |open: f32| plot.bottom() - open.clamp(0.0, 1.0) * plot.height();
    for line in [0.0f32, 1.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), y_at(line)),
                egui::pos2(plot.right(), y_at(line)),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.35),
        );
    }
    let shape: Vec<egui::Pos2> = if rhythm {
        // RHYTHM: the beat grid, cut at the duty. The picture is the
        // same envelope, repeated on the division.
        let open_share = (duty / 100.0).clamp(0.0, 1.0);
        vec![
            egui::pos2(plot.left(), y_at(floor)),
            egui::pos2(plot.left(), y_at(1.0)),
            egui::pos2(plot.left() + open_share * plot.width(), y_at(1.0)),
            egui::pos2(plot.left() + open_share * plot.width(), y_at(floor)),
            egui::pos2(plot.right(), y_at(floor)),
        ]
    } else {
        vec![
            egui::pos2(x_at(0.0), y_at(floor)),
            egui::pos2(x_at(p::LOOKAHEAD_MS), y_at(floor)),
            egui::pos2(x_at(p::LOOKAHEAD_MS + attack), y_at(1.0)),
            egui::pos2(x_at(p::LOOKAHEAD_MS + attack + hold), y_at(1.0)),
            egui::pos2(x_at(span_ms), y_at(floor)),
        ]
    };
    chrome::trace(&mut shapes, &shape, Weight::Heavy, ink);
    // The lookahead: the door is already moving before the transient
    // arrives, which is the point of it.
    if !rhythm {
        chrome::dashes(
            &mut shapes,
            &[
                egui::pos2(x_at(p::LOOKAHEAD_MS), plot.top()),
                egui::pos2(x_at(p::LOOKAHEAD_MS), plot.bottom()),
            ],
            0.0,
            Weight::Hair,
            alpha.live_dim.color,
        );
    }
    // Where the door stands right now, from what the section measured.
    if shut_db > 0.1 {
        let now = (1.0 - shut_db / 80.0).clamp(0.0, 1.0);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), y_at(now)),
                egui::pos2(plot.right(), y_at(now)),
            ],
            Weight::Hair,
            alpha.jeopardy_latent.color,
        );
    }

    // ---- KEY: the passband the door is listening through. ------------
    let key = lay.key();
    chrome::panel_variant(
        &mut shapes,
        key,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let band = egui::Rect::from_min_max(
        egui::pos2(key.left() + 6.0, key.top() + font.size + 4.0),
        egui::pos2(key.right() - 6.0, key.bottom() - font.size - 4.0),
    );
    if band.is_positive() {
        for hz in [100.0f32, 1_000.0, 10_000.0] {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(band.left() + place_of_hz(hz) * band.width(), band.top()),
                    egui::pos2(band.left() + place_of_hz(hz) * band.width(), band.bottom()),
                ],
                Weight::Hair,
                edge.gamma_multiply(0.30),
            );
        }
        let a = band.left() + place_of_hz(key_hp) * band.width();
        let b = band.left() + place_of_hz(key_lp) * band.width();
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(egui::pos2(a, band.top()), egui::pos2(b, band.bottom())),
            0.0,
            alpha.live_dim.color,
        ));
        for x in [a, b] {
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(x, band.top() - 2.0),
                    egui::pos2(x, band.bottom() + 2.0),
                ],
                Weight::Heavy,
                alpha.live.color,
            );
        }
    }
    painter.extend(shapes);

    // ---- The figures. One size, one gutter. --------------------------
    // Every word goes through the ledger: it measures where each one
    // lands and refuses, in a debug build, to let two of them crowd.
    let mut words = tool::Ledger::new(painter, font.clone(), "DOOR");
    let mut label = |at: egui::Pos2, align: egui::Align2, text: String, ink: egui::Color32| {
        words.text(at, align, text, ink);
    };
    const PAD_X: f32 = 8.0;
    // The gutter is as wide as the longest word that stands in it, plus
    // air. Guessing at it is how a bar comes to be drawn over a label.
    let gutter = ["HYST", "RATIO", "RANGE", "DIV", "DUTY", "THRES"]
        .into_iter()
        .map(|word| {
            painter
                .layout_no_wrap(word.to_owned(), font.clone(), ink)
                .rect
                .width()
        })
        .fold(0.0f32, f32::max)
        + 8.0;
    for (i, word) in ["KEY", "RHYTHM"].into_iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(lay.mode.left() + i as f32 * mode_half, lay.mode.top()),
            egui::vec2(mode_half, lay.mode.height()),
        );
        label(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            word.to_owned(),
            if rhythm == (i == 1) { ink } else { edge },
        );
    }
    label(
        egui::pos2(lay.threshold.left() + 3.0, lay.threshold.center().y),
        egui::Align2::LEFT_CENTER,
        "THRES".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.threshold.right() - 4.0, lay.threshold.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{threshold:.0}"),
        ink,
    );
    label(
        egui::pos2(env.left() + PAD_X, env.top() + 2.0),
        egui::Align2::LEFT_TOP,
        if rhythm { "GRID" } else { "ENVELOPE" }.to_owned(),
        edge,
    );
    label(
        egui::pos2(env.right() - PAD_X, env.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if rhythm {
            format!("1/{:.0}", 1.0 / p::DIVISION_BEATS[division].max(0.001))
        } else {
            format!("{span_ms:.0}ms")
        },
        ink,
    );
    // The three times, each under the part of the picture it shapes.
    if !rhythm {
        for (rect, word, ms) in [
            (lay.attack, "A", attack),
            (lay.hold, "H", hold),
            (lay.release, "R", release),
        ] {
            label(
                egui::pos2(rect.center().x, env.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                if ms >= 100.0 {
                    format!("{word} {ms:.0}")
                } else {
                    format!("{word} {ms:.1}")
                },
                ink,
            );
        }
    } else {
        label(
            egui::pos2(env.center().x, env.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("DUTY {duty:.0}%"),
            ink,
        );
    }
    label(
        egui::pos2(key.left() + PAD_X, key.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "KEY".to_owned(),
        edge,
    );
    label(
        egui::pos2(key.right() - PAD_X, key.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if key_hp <= 21.0 && key_lp >= 19_000.0 {
            "WIDE".to_owned()
        } else {
            "BAND".to_owned()
        },
        if key_hp <= 21.0 && key_lp >= 19_000.0 {
            edge
        } else {
            alpha.live.color
        },
    );
    let hz_word = |hz: f32| {
        if hz >= 1000.0 {
            format!("{:.1}k", hz / 1000.0)
        } else {
            format!("{hz:.0}")
        }
    };
    label(
        egui::pos2(key.left() + PAD_X, key.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        hz_word(key_hp),
        ink,
    );
    label(
        egui::pos2(key.right() - PAD_X, key.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        hz_word(key_lp),
        ink,
    );
    // The figures that are only figures. A bar behind each, so the row
    // is still a picture at a glance.
    for (rect, word, place, said) in [
        (
            lay.hysteresis,
            "HYST",
            hysteresis / 12.0,
            format!("{hysteresis:.0}dB"),
        ),
        (
            lay.ratio,
            "RATIO",
            (ratio - 1.5) / 18.5,
            format!("{ratio:.0}:1"),
        ),
        (lay.range, "RANGE", range / 80.0, format!("{range:.0}dB")),
        (
            lay.division,
            "DIV",
            division as f32 / 5.0,
            format!("1/{:.0}", 1.0 / p::DIVISION_BEATS[division].max(0.001)),
        ),
        (lay.duty, "DUTY", duty / 100.0, format!("{duty:.0}%")),
    ] {
        // In KEY mode the rhythm's two figures are not what the door is
        // doing, and they say so rather than lying quietly.
        let asleep = !rhythm && (rect == lay.division || rect == lay.duty);
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.0),
            egui::pos2(rect.right() - 42.0, rect.center().y + 2.0),
        );
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.35));
            if !asleep {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(
                            bar.left() + place.clamp(0.0, 1.0) * bar.width(),
                            bar.bottom(),
                        ),
                    ),
                    0.0,
                    ink,
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
            if asleep { "--".to_owned() } else { said },
            if asleep { edge } else { ink },
        );
    }

    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 210.0))
    }

    #[test]
    fn every_door_parameter_has_its_own_instrument() {
        let face = door_face(glass());
        let controls = face.controls();
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(
                glass().contains_rect(*rect),
                "parameter {id} left the glass"
            );
            assert!(rect.is_positive(), "parameter {id} lost its instrument");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "parameters {id} and {other} overlap"
                );
            }
        }
    }

    /// The two time pictures own the left, the key and the figures the
    /// right, and the envelope's three times lie end to end.
    #[test]
    fn the_door_reads_as_time_on_the_left_and_frequency_on_the_right() {
        let face = door_face(glass());
        assert!(face.threshold.right() <= face.hysteresis.left());
        assert!(face.attack.right() <= face.hold.left());
        assert!(face.hold.right() <= face.release.left());
        assert_eq!(face.attack.y_range(), face.release.y_range());
        assert!(face.envelope().right() <= face.key().left());
        assert!(face.key_hp.right() <= face.key_lp.left());
        // The envelope is the biggest picture on the card.
        assert!(face.envelope().area() > face.key().area());
    }
}
