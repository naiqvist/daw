//! The mixer — the lattice's other content.
//!
//! The session already draws tracks ACROSS as columns, and a mixer makes
//! the same claim about the same objects: one column, one track. So this
//! is not a second surface with a second geometry to reconcile — it is
//! what hangs beneath the heads while the scenes are put away. The heads
//! never move across the change, which is what lets the eye keep its
//! place.
//!
//! # One ruler
//!
//! A meter and a fader are two different quantities, and drawing them on
//! one axis invites reading them as one. They are therefore put on the
//! SAME ruler rather than two: decibels, from [`meter::FLOOR_DB`] to
//! unity at the top, with the small boost the model allows drawn as
//! overtravel ABOVE the meter's ceiling. Unity is exactly the line the
//! meter tops out at. A fader at unity and a meter reading full scale
//! then mean the same height, because they are the same number.
//!
//! The alternative — the fader on its own normalised span — puts unity at
//! two thirds of the way up while full scale is at the top, and the eye
//! reads the two marks as disagreeing about what "the top" is.
//!
//! # Why nothing is dimmed to show focus
//!
//! Mixing is COMPARISON: the level of one track against another. A
//! surface that dims every channel but the focused one destroys exactly
//! the reading it exists to support. So every meter is drawn at the same
//! strength, and the cursor is carried by the frame around a channel — a
//! mark of its own, in the value focus always uses, spent on the one
//! thing that is focus rather than on the biggest thing on screen.
//!
//! The meter is a run of circuit ticks in a recessed well. A separate
//! rail carries the fader, so a moving level cannot hide the value the
//! hand set.

use super::Level;
use crate::design::{self, block, circuit, kit::Weight};
use crate::sequencing::ReturnTrack;
use crate::sequencing::Song;
use crate::ui::device::meter;
use eframe::egui;

/// Cells in one meter. Enough that the eye reads a bar rather than
/// counting, few enough that a single cell is a coarse, findable unit —
/// about four decibels at this span.
pub const CELLS: usize = 14;

/// The most the model lets a fader add: `track.volume` is registered over
/// `0.0..=1.5` linear, and 1.5 is this many decibels. Declared here as
/// what it IS rather than as a round number, so the drawn overtravel
/// cannot drift from the range the document actually permits.
pub const BOOST_DB: f32 = 3.521_825_2;

/// One track reduced to exactly what a channel strip draws.
///
/// Holds no state of its own and is rebuilt every frame from the song and
/// the last thing the engine said — a strip that remembered anything
/// could show a fader the document does not have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    /// Fader, as linear amplitude. 1.0 is unity.
    pub gain: f32,
    /// Constant-power pan, `-1..=1`, centre exact.
    pub pan: f32,
    /// Its own switch, regardless of what that means for what is heard.
    pub muted: bool,
    pub soloed: bool,
    /// Whether this track ACTUALLY sounds — which is neither of the two
    /// above. A track silenced by someone else's solo is not muted, and a
    /// mixer that could not draw that third state would be lying about
    /// two of them.
    pub audible: bool,
    /// Whether this channel has switches to draw at all.
    ///
    /// The master has none: everything that sounds arrives there, so
    /// there is nothing above it to silence it and nothing beside it to
    /// solo against. Two squares that cannot be pressed would be two
    /// controls promising something the surface will not do.
    pub switches: bool,
    /// The level as the meter shows it — the engine's report, through
    /// the meter's ballistics. Silence where there is no engine, which
    /// is the truth about a stage with nothing behind it.
    pub level: Level,
    /// The loudest recent moment, held: the mark above the bar.
    pub peak: Level,
    /// The sends, one per return the song has, in return order: the
    /// share of this channel that goes to each. `None` past the song's
    /// returns, so a strip draws exactly as many rails as there are
    /// places to send to.
    pub sends: [Option<f32>; ReturnTrack::MAX],
    /// Whether this channel IS a return — a place sends arrive rather
    /// than a track that plays. Drawn in its own casing so the two are
    /// never mistaken, and with no solo, because a return has none.
    pub is_return: bool,
}

/// A return, as the mixer draws it beside the tracks: its letter, its
/// name, and its channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Return {
    pub letter: char,
    pub name: String,
    pub channel: Channel,
}

/// One meter as it is drawn: the bar, and the mark above it.
///
/// Both come out of the ballistics rather than straight from the engine.
/// A block's peak is a few milliseconds of truth, and a bar that showed
/// each one raw would flicker between the loudest and quietest block of
/// every frame; the bar rises at once and falls slowly, and the mark
/// stays at the loudest thing it saw for long enough to be read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    pub level: Level,
    pub peak: Level,
}

/// Every track as a channel, in the song's own order.
pub fn channels(song: &Song, readings: &[Reading]) -> Vec<Channel> {
    song.tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let reading = readings.get(index).copied().unwrap_or_default();
            let mut sends = [None; ReturnTrack::MAX];
            for (slot, send) in sends.iter_mut().enumerate().take(song.returns.len()) {
                *send = Some(track.send(slot));
            }
            Channel {
                gain: track.volume,
                pan: track.pan,
                muted: track.muted,
                soloed: track.solo,
                audible: song.audible(index),
                switches: true,
                level: reading.level,
                peak: reading.peak,
                sends,
                is_return: false,
            }
        })
        .collect()
}

/// Every return as a channel, lettered in the song's own order. A
/// return has no meter of its own yet — the engine reports tracks and
/// the master — so its bar reads silence, honestly.
pub fn returns(song: &Song) -> Vec<Return> {
    song.returns
        .iter()
        .enumerate()
        .map(|(index, ret)| Return {
            letter: ReturnTrack::letter(index),
            name: ret.name.clone(),
            channel: Channel {
                gain: ret.volume,
                pan: ret.pan,
                muted: ret.mute,
                soloed: false,
                audible: !ret.mute,
                switches: true,
                level: Level::default(),
                peak: Level::default(),
                sends: [None; ReturnTrack::MAX],
                is_return: true,
            },
        })
        .collect()
}

/// One send's rail: the letter, the rail, and the mark on it.
pub const SEND_H: f32 = 15.0;

/// Where a decibel sits in a strip, as `0..=1` from the bottom.
///
/// The span runs from the meter's floor to unity PLUS the boost the model
/// allows, so one mapping serves the meter and the fader both.
pub fn place_of_db(db: f32) -> f32 {
    let span = BOOST_DB - meter::FLOOR_DB;
    if db.is_nan() {
        return 0.0;
    }
    ((db - meter::FLOOR_DB) / span).clamp(0.0, 1.0)
}

/// Unity's place on that ruler: the line the meter tops out at, and where
/// a fader that neither adds nor takes away sits.
pub fn unity_place() -> f32 {
    place_of_db(0.0)
}

/// A linear amplitude's place on the ruler. Silence is the floor rather
/// than minus infinity, because a mark has to be somewhere.
pub fn place_of_amp(amp: f32) -> f32 {
    let db = meter::amp_to_db(amp);
    if db.is_infinite() {
        0.0
    } else {
        place_of_db(db)
    }
}

/// How much of cell `index` is lit by a level standing at `place`.
///
/// Cells divide the METER's span — floor to unity — rather than the whole
/// ruler: the boost band above unity is the fader's overtravel and has no
/// meter behind it, because a level above full scale is clipping and not
/// a taller bar.
pub fn cell_coverage(index: usize, place: f32) -> f32 {
    let ceiling = unity_place();
    if ceiling <= 0.0 {
        return 0.0;
    }
    let per_cell = ceiling / CELLS as f32;
    let floor = index as f32 * per_cell;
    ((place - floor) / per_cell).clamp(0.0, 1.0)
}

/// The most the model lets a fader hold, as linear amplitude: the top of
/// `track.volume`'s registered span. Named here so the verb and the drawn
/// overtravel cannot come to disagree about the same number.
pub const MAX_GAIN: f32 = 1.5;

/// One press of the fader, in decibels. The unit the ear works in, so a
/// press is the same SIZE of change everywhere on the rail rather than a
/// huge one at the top and an inaudible one at the bottom.
pub const GAIN_STEP_DB: f32 = 1.0;

/// A held modifier's press: a tenth of one, for setting rather than
/// finding.
pub const GAIN_FINE_DB: f32 = 0.1;

/// One press of the pan, over a `-1..=1` span: twenty presses corner to
/// corner, ten from the centre to either side.
pub const PAN_STEP: f32 = 0.1;

/// Move a fader by `db`, and hand back what the track's volume becomes.
///
/// Two ends, both of which have to be reachable BY HAND rather than by
/// luck: the top is the model's own limit, and the bottom is true
/// silence — not the meter's floor, which is merely the quietest thing
/// the meter can draw. A fader that could only approach silence would
/// leave the one value a musician reaches for most out of reach.
pub fn step_gain(volume: f32, db: f32) -> f32 {
    let from = if volume <= 0.0 {
        meter::FLOOR_DB
    } else {
        meter::amp_to_db(volume)
    };
    let to = from + db;
    if to <= meter::FLOOR_DB {
        // The floor and everything under it is silence, EXACTLY. The
        // floor itself is included so the gesture is its own inverse: a
        // press up from silence leaves the floor by one decibel, and one
        // press back down returns to silence rather than landing on a
        // level too quiet for the meter to draw but not quiet enough to
        // be off.
        return 0.0;
    }
    meter::db_to_amp(to.min(BOOST_DB)).clamp(0.0, MAX_GAIN)
}

/// Move a pan by `delta`, and hand back what the track's pan becomes.
///
/// Centre is EXACT in the model, so a step that would cross it LANDS on
/// it instead. Without that, a pan stepped by tenths from an odd place
/// steps over the centre forever and "back to the middle" becomes a
/// thing only a reset can do.
pub fn step_pan(pan: f32, delta: f32) -> f32 {
    let to = pan + delta;
    if pan != 0.0 && (pan < 0.0) != (to < 0.0) {
        return 0.0;
    }
    to.clamp(-1.0, 1.0)
}

/// A fader's value, said out loud. Decibels, because that is what it was
/// edited in — and silence says so in words rather than as a number no
/// scale contains.
pub fn gain_label(volume: f32) -> String {
    if volume <= 0.0 {
        return "-inf dB".to_owned();
    }
    let db = meter::amp_to_db(volume);
    // Anything that would print as zero IS zero. Without this a fader
    // walked back to unity reads "-0.0 dB", which puts a sign on a value
    // that has none — and the sign is the first thing the eye takes off
    // this readout.
    let db = if db.abs() < 0.05 { 0.0 } else { db };
    format!("{db:+.1} dB")
}

/// A pan's value: which way, and how far, counted in whole percent so
/// the number is short enough to read at a glance. Centre is a WORD,
/// because centre is a state and not a small number.
pub fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        return "centre".to_owned();
    }
    let side = if pan < 0.0 { 'L' } else { 'R' };
    format!("{side}{amount}")
}

/// The pan rail and its always-visible value at the foot of a strip.
pub const PAN_H: f32 = 26.0;

/// The two switches' row. Square by construction — a switch is a square
/// because a square at this size is a shape rather than a glyph, and it
/// survives the distance the meter is meant to be read from.
pub const SWITCH_H: f32 = 16.0;

/// Where a channel strip hangs: everything from beneath the head down to
/// the foot of the field. The column IS the track's address, so the strip
/// takes exactly the head's width and never computes one of its own.
pub fn strip_beneath(head: egui::Rect, bottom: f32, gap: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(head.min.x, head.max.y + gap),
        egui::pos2(head.max.x, bottom),
    )
}

/// Draw one channel.
///
/// Everything here is symmetric about the strip's centre line — the two
/// meters, the handle across them, the switch pair, the pan rail's centre
/// tick — because the thing being read at a distance is a DEPARTURE from
/// centre, and a symmetric ground is what makes a departure visible.
pub fn draw(
    painter: &egui::Painter,
    strip: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
    variant: u8,
) {
    if strip.height() <= 0.0 || strip.width() <= 0.0 {
        return;
    }
    draw_casing(painter, strip, channel.is_return, alpha, variant);

    let inner = strip.shrink(gap.max(3.0));
    let pan_top = inner.max.y - PAN_H;
    let switch_h = if channel.switches {
        SWITCH_H + gap
    } else {
        0.0
    };
    let switch_top = pan_top - switch_h;
    // The sends sit between the meters and the switches: a rail per
    // return, and none at all when the song has nowhere to send to.
    let send_count = channel.sends.iter().flatten().count();
    let sends_h = if send_count > 0 {
        send_count as f32 * SEND_H + gap
    } else {
        0.0
    };
    let sends_top = switch_top - sends_h;
    let meters = egui::Rect::from_min_max(inner.min, egui::pos2(inner.max.x, sends_top - gap));
    if meters.height() > 0.0 {
        draw_meters(painter, meters, channel, gap, alpha);
    }
    if send_count > 0 {
        draw_sends(
            painter,
            egui::Rect::from_min_max(
                egui::pos2(inner.min.x, sends_top),
                egui::pos2(inner.max.x, sends_top + send_count as f32 * SEND_H),
            ),
            channel,
            alpha,
        );
    }
    if channel.switches {
        draw_switches(
            painter,
            egui::Rect::from_min_max(
                egui::pos2(inner.min.x, switch_top),
                egui::pos2(inner.max.x, switch_top + SWITCH_H),
            ),
            channel,
            gap,
            alpha,
        );
    }
    draw_pan(
        painter,
        egui::Rect::from_min_max(egui::pos2(inner.min.x, pan_top), inner.max),
        channel,
        alpha,
    );
}

/// A channel's casing. A track's is the deck's surface with the house
/// edge; a return's is a WELL — a step down in value — with a dotted
/// frame inset from its edge, so a return reads as a place things are
/// sent into rather than a track that plays, from across the room.
fn draw_casing(
    painter: &egui::Painter,
    strip: egui::Rect,
    is_return: bool,
    alpha: &design::Alphabet,
    variant: u8,
) {
    let mut casing = Vec::new();
    circuit::panel_variant(
        &mut casing,
        strip,
        Some(if is_return {
            alpha.well.color
        } else {
            alpha.surface.color
        }),
        alpha.ground.color,
        Some((Weight::Hair, alpha.edge.color)),
        variant,
    );
    if is_return {
        let frame = strip.shrink(5.0);
        let corners = [
            frame.left_top(),
            frame.right_top(),
            frame.right_bottom(),
            frame.left_bottom(),
            frame.left_top(),
        ];
        circuit::dashes(&mut casing, &corners, 0.0, Weight::Hair, alpha.edge.color);
    }
    for shape in casing {
        painter.add(shape);
    }
}

/// The return's head, above its strip: the same well casing, its letter
/// carved large, and its name beneath.
pub fn draw_return_head(
    painter: &egui::Painter,
    head: egui::Rect,
    ret: &Return,
    alpha: &design::Alphabet,
    variant: u8,
) {
    draw_casing(painter, head, true, alpha, variant);
    let inner = head.shrink(10.0);
    block::paint(
        painter,
        egui::Id::new(("mixer-return-letter", head.min.x.round() as i32)),
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        block::unit::TITLE,
        &ret.letter.to_string(),
        alpha.ink.color,
    );
    block::paint(
        painter,
        egui::Id::new(("mixer-return-word", head.min.x.round() as i32)),
        egui::pos2(inner.left() + 22.0, inner.top() + 2.0),
        egui::Align2::LEFT_TOP,
        block::unit::MICRO,
        "RETURN",
        alpha.edge.color,
    );
    painter.text(
        inner.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        if ret.name.is_empty() {
            "unnamed".to_owned()
        } else {
            ret.name.clone()
        },
        egui::FontId::monospace(design::px(design::type_scale::MICRO)),
        alpha.ink.color,
    );
}

/// The sends: one rail per return, lettered, with the mark at the share
/// sent. A send at nothing keeps its rail and a hollow mark, so a
/// channel sending nowhere still shows where it could.
fn draw_sends(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    alpha: &design::Alphabet,
) {
    let mut shapes = Vec::new();
    for (slot, send) in channel.sends.iter().enumerate() {
        let Some(send) = send else {
            break;
        };
        let y = zone.min.y + slot as f32 * SEND_H + SEND_H * 0.5;
        block::paint(
            painter,
            egui::Id::new(("mixer-send-letter", zone.min.x.round() as i32, slot)),
            egui::pos2(zone.min.x + 2.0, y),
            egui::Align2::LEFT_CENTER,
            block::unit::MICRO,
            &ReturnTrack::letter(slot).to_string(),
            alpha.edge.color,
        );
        let left = egui::pos2(zone.min.x + 16.0, y);
        let right = egui::pos2(zone.max.x - 4.0, y);
        circuit::rail(&mut shapes, left, right, &[0.0, 1.0], alpha.edge.color);
        let x = left.x + send.clamp(0.0, 1.0) * (right.x - left.x);
        let sending = *send > 0.0;
        circuit::pad(
            &mut shapes,
            egui::pos2(x, y),
            circuit::PAD,
            if sending {
                alpha.ink.color
            } else {
                alpha.edge.color
            },
            sending,
        );
    }
    for shape in shapes {
        painter.add(shape);
    }
}

/// Two meter ladders in one well, their ruler, and a separate fader rail.
fn draw_meters(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
) {
    // The ink a level is drawn in. LIVE is the alphabet's word for the
    // sounding present — meters in motion are its named use. A track that
    // CANNOT sound is drawn in structure ink instead: it has a meter, and
    // the meter has nothing to say.
    let ink = if channel.audible {
        alpha.live.color
    } else {
        alpha.edge.color
    };
    let readout_h = block::height(block::unit::MICRO) + gap;
    let chart = egui::Rect::from_min_max(zone.min, egui::pos2(zone.max.x, zone.max.y - readout_h));
    if !chart.is_positive() {
        return;
    }
    let scale_w = block::measure(painter, "-48", block::unit::MICRO) + gap;
    let fader_w = 20.0;
    let well = egui::Rect::from_min_max(
        egui::pos2(chart.min.x + scale_w, chart.min.y),
        egui::pos2(chart.max.x - fader_w - gap, chart.max.y),
    );
    let mut shapes = Vec::new();
    circuit::octagon(
        &mut shapes,
        well,
        4.0,
        Some(alpha.well.color),
        Some((Weight::Hair, alpha.edge.color)),
    );

    // The meter cells occupy floor-to-unity. The well itself continues
    // into the fader's boost band so the shared ruler stays honest.
    let unity_y = chart.max.y - chart.height() * unity_place();
    let ladder = egui::Rect::from_min_max(
        egui::pos2(well.min.x + gap * 0.5, unity_y),
        egui::pos2(well.max.x - gap * 0.5, well.max.y - 2.0),
    );
    let lane_gap = 3.0;
    let lane_w = ((ladder.width() - lane_gap) * 0.5).max(1.0);
    let sides = [
        (
            egui::Rect::from_min_size(ladder.min, egui::vec2(lane_w, ladder.height())),
            channel.level.left,
            channel.peak.left,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(ladder.max.x - lane_w, ladder.min.y),
                egui::vec2(lane_w, ladder.height()),
            ),
            channel.level.right,
            channel.peak.right,
        ),
    ];
    for (lane, amp, peak) in sides {
        let place = place_of_amp(amp);
        let lit = (0..CELLS)
            .map(|cell| cell_coverage(cell, place))
            .sum::<f32>()
            / CELLS as f32;
        circuit::tick_bar(
            &mut shapes,
            lane,
            CELLS,
            lit,
            ink,
            alpha.surface.color,
            false,
        );
        if amp >= 1.0 {
            let cell_h = lane.height() / CELLS as f32;
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    lane.min,
                    egui::pos2(lane.max.x, lane.min.y + cell_h - 1.0),
                ),
                0.0,
                alpha.jeopardy_active.color,
            ));
        }
        if peak > 0.0 {
            let y = chart.max.y - chart.height() * place_of_amp(peak);
            let peak_ink = if peak >= 1.0 {
                alpha.jeopardy_active.color
            } else {
                alpha.ink.color
            };
            circuit::trace(
                &mut shapes,
                &[egui::pos2(lane.min.x, y), egui::pos2(lane.max.x, y)],
                Weight::Heavy,
                peak_ink,
            );
            circuit::pad(
                &mut shapes,
                egui::pos2(lane.max.x, y),
                circuit::PAD - 2.0,
                peak_ink,
                true,
            );
        }
    }

    // The fader has its own rail beside the moving meters.
    let fader_x = chart.max.x - fader_w * 0.5;
    circuit::rail(
        &mut shapes,
        egui::pos2(fader_x, chart.min.y + 2.0),
        egui::pos2(fader_x, chart.max.y - 2.0),
        &[0.0, 1.0 - unity_place(), 1.0],
        alpha.edge.color,
    );
    let handle_y = chart.max.y - chart.height() * place_of_amp(channel.gain);
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(fader_x - 7.0, handle_y),
            egui::pos2(fader_x + 7.0, handle_y),
        ],
        Weight::Bold,
        alpha.ink.color,
    );
    circuit::pad(
        &mut shapes,
        egui::pos2(fader_x, handle_y),
        circuit::PAD,
        alpha.ink.color,
        true,
    );
    for shape in shapes {
        painter.add(shape);
    }

    // Graduations are pads on one rail; their numbers are cut beside it.
    let rail_x = chart.min.x + scale_w - gap * 0.5;
    let mut scale = Vec::new();
    circuit::trace(
        &mut scale,
        &[
            egui::pos2(rail_x, chart.min.y),
            egui::pos2(rail_x, chart.max.y),
        ],
        Weight::Hair,
        alpha.edge.color,
    );
    for db in [0_i32, -6, -12, -24, -48] {
        let y = chart.max.y - chart.height() * place_of_db(db as f32);
        circuit::pad(
            &mut scale,
            egui::pos2(rail_x, y),
            circuit::PAD - 2.0,
            alpha.edge.color,
            true,
        );
        block::paint(
            painter,
            egui::Id::new(("mixer-db", chart.min.x.round() as i32, db)),
            egui::pos2(rail_x - gap, y),
            egui::Align2::RIGHT_CENTER,
            block::unit::MICRO,
            &db.to_string(),
            alpha.edge.color,
        );
    }
    for shape in scale {
        painter.add(shape);
    }

    block::paint(
        painter,
        egui::Id::new(("mixer-gain", zone.min.x.round() as i32)),
        egui::pos2(zone.center().x, zone.max.y),
        egui::Align2::CENTER_BOTTOM,
        block::unit::MICRO,
        &gain_label(channel.gain),
        alpha.ink.color,
    );
}

/// Mute and solo, as two labelled squares.
fn draw_switches(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
) {
    let size = SWITCH_H.min(zone.height()).min(zone.width());
    let centre = zone.center().x;
    // A return has no solo: one switch, centred, rather than a pair
    // with one that cannot be pressed.
    let switches: Vec<(f32, bool, &str)> = if channel.is_return {
        vec![(centre - size * 0.5, channel.muted, "M")]
    } else {
        vec![
            (centre - gap * 0.5 - size, channel.muted, "M"),
            (centre + gap * 0.5, channel.soloed, "S"),
        ]
    };
    for (x, on, label) in switches {
        let rect = egui::Rect::from_min_size(egui::pos2(x, zone.min.y), egui::vec2(size, size));
        if on {
            painter.rect_filled(rect, 0.0, alpha.ink.color);
        } else {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, alpha.edge.color),
                egui::StrokeKind::Inside,
            );
        }
        block::paint(
            painter,
            egui::Id::new(("mixer-switch", zone.min.x.round() as i32, label)),
            rect.center(),
            egui::Align2::CENTER_CENTER,
            block::unit::MICRO,
            label,
            if on {
                alpha.ground.color
            } else {
                alpha.edge.color
            },
        );
    }
}

/// The pan rail: centre is exact in the model, so it is exact here — a
/// tick at the middle, and the mark that departs from it.
fn draw_pan(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    alpha: &design::Alphabet,
) {
    let rail_y = zone.min.y + 6.0;
    let left = egui::pos2(zone.min.x + 4.0, rail_y);
    let right = egui::pos2(zone.max.x - 4.0, rail_y);
    let mut shapes = Vec::new();
    circuit::rail(&mut shapes, left, right, &[0.0, 0.5, 1.0], alpha.edge.color);
    let x = left.x + channel.pan.clamp(-1.0, 1.0).mul_add(0.5, 0.5) * (right.x - left.x);
    circuit::pad(
        &mut shapes,
        egui::pos2(x, rail_y),
        circuit::PAD + 1.0,
        alpha.ink.color,
        true,
    );
    for shape in shapes {
        painter.add(shape);
    }
    painter.text(
        egui::pos2(zone.center().x, zone.max.y),
        egui::Align2::CENTER_BOTTOM,
        pan_label(channel.pan),
        egui::FontId::monospace(design::px(design::type_scale::MICRO)),
        alpha.ink.color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::TrackKind;

    #[test]
    fn a_fader_reaches_silence_and_the_models_own_ceiling() {
        // Down from silence stays silent; the bottom is a floor, not a hole.
        assert_eq!(step_gain(0.0, -GAIN_STEP_DB), 0.0);

        // And up from silence leaves it, so the gesture is reversible.
        let up = step_gain(0.0, GAIN_STEP_DB);
        assert!(up > 0.0, "a silenced fader could not be brought back");
        assert_eq!(
            step_gain(up, -GAIN_STEP_DB),
            0.0,
            "one press down from one press up was not silence again"
        );

        // The top is the document's limit and nothing beyond it.
        let mut loud = 1.0;
        for _ in 0..20 {
            loud = step_gain(loud, GAIN_STEP_DB);
        }
        assert!(
            loud <= MAX_GAIN,
            "the fader went past what the document allows ({loud})"
        );
        assert!(
            (loud - MAX_GAIN).abs() < 1e-3,
            "the fader stopped short ({loud})"
        );
    }

    #[test]
    fn a_fader_step_is_the_same_size_wherever_it_is_pressed() {
        for start in [0.05f32, 0.25, 0.5, 1.0] {
            let moved = step_gain(start, GAIN_STEP_DB);
            let grew = meter::amp_to_db(moved) - meter::amp_to_db(start);
            assert!(
                (grew - GAIN_STEP_DB).abs() < 1e-3,
                "a press at {start} moved {grew} dB, not {GAIN_STEP_DB}"
            );
        }
    }

    #[test]
    fn unity_is_reachable_by_hand_from_either_side() {
        // A press is a whole decibel and unity is zero of them, so it is
        // an exact place on the rail rather than one to be approached.
        let below = step_gain(meter::db_to_amp(-1.0), GAIN_STEP_DB);
        assert!(
            (below - 1.0).abs() < 1e-4,
            "coming up, unity was missed ({below})"
        );
        let above = step_gain(meter::db_to_amp(1.0), -GAIN_STEP_DB);
        assert!(
            (above - 1.0).abs() < 1e-4,
            "coming down, unity was missed ({above})"
        );
    }

    #[test]
    fn a_pan_crossing_the_centre_lands_on_it() {
        // The case that makes this necessary: an odd starting place.
        let odd = 0.05;
        assert_eq!(
            step_pan(odd, -PAN_STEP),
            0.0,
            "the pan stepped over the centre"
        );
        assert_eq!(step_pan(-0.05, PAN_STEP), 0.0);
        // And having landed there, the next press leaves.
        assert!((step_pan(0.0, -PAN_STEP) + PAN_STEP).abs() < 1e-6);
        // The ends hold.
        assert_eq!(step_pan(1.0, PAN_STEP), 1.0);
        assert_eq!(step_pan(-1.0, -PAN_STEP), -1.0);
    }

    #[test]
    fn the_values_say_themselves() {
        assert_eq!(gain_label(0.0), "-inf dB");
        assert_eq!(gain_label(1.0), "+0.0 dB");
        // Walked back to unity rather than set to it: the arithmetic
        // leaves a hair either side, and neither may grow a sign.
        assert_eq!(
            gain_label(step_gain(meter::db_to_amp(-1.0), GAIN_STEP_DB)),
            "+0.0 dB"
        );
        assert_eq!(
            gain_label(step_gain(meter::db_to_amp(1.0), -GAIN_STEP_DB)),
            "+0.0 dB"
        );
        assert_eq!(gain_label(0.5), "-6.0 dB");
        assert_eq!(pan_label(0.0), "centre");
        assert_eq!(pan_label(-0.5), "L50");
        assert_eq!(pan_label(1.0), "R100");
    }

    /// A channel carries one send per return the song has and none past
    /// them; a return is a channel of its own, marked as one, with no
    /// sends and no solo, lettered in order.
    #[test]
    fn sends_follow_the_returns_and_a_return_is_marked_as_one() {
        let mut song = Song::default();
        assert!(channels(&song, &[])[0].sends.iter().all(Option::is_none));
        assert!(returns(&song).is_empty());

        song.add_return();
        song.add_return();
        song.returns[1].name = "tape".to_owned();
        song.returns[1].mute = true;
        song.tracks[0].sends = vec![0.35];
        let channel = channels(&song, &[])[0];
        assert_eq!(channel.sends[0], Some(0.35));
        assert_eq!(
            channel.sends[1],
            Some(0.0),
            "a send unset is not a send absent"
        );
        assert_eq!(channel.sends[2], None, "a send past the returns");
        assert!(!channel.is_return);

        let returns = returns(&song);
        assert_eq!(returns.len(), 2);
        assert_eq!(returns[0].letter, 'A');
        assert_eq!(returns[1].letter, 'B');
        assert_eq!(returns[1].name, "tape");
        assert!(returns[1].channel.is_return);
        assert!(returns[1].channel.muted);
        assert!(!returns[1].channel.audible);
        assert!(!returns[1].channel.soloed);
        assert!(returns[1].channel.sends.iter().all(Option::is_none));
    }

    #[test]
    fn a_channel_reports_the_tracks_own_mixer_facts() {
        let mut song = Song::default();
        song.tracks[0].volume = 0.5;
        song.tracks[0].pan = -0.25;
        let channels = channels(&song, &[]);
        assert_eq!(channels[0].gain, 0.5);
        assert_eq!(channels[0].pan, -0.25);
        assert!(channels[0].audible);
        assert_eq!(
            channels[0].level,
            Level::default(),
            "a meter with no engine behind it invented a reading"
        );
        assert_eq!(channels[0].peak, Level::default());

        let heard = super::channels(
            &song,
            &[Reading {
                level: Level {
                    left: 0.5,
                    right: 0.25,
                },
                peak: Level {
                    left: 0.8,
                    right: 0.4,
                },
            }],
        );
        assert_eq!(
            heard[0].level.right, 0.25,
            "the sides were flattened into one"
        );
        assert_eq!(heard[0].peak.left, 0.8, "the peak mark was lost");
    }

    #[test]
    fn the_third_state_is_visible_as_itself() {
        let mut song = Song::default();
        song.add_track(TrackKind::Instrument);
        song.tracks[1].solo = true;
        let channels = channels(&song, &[]);
        assert!(
            !channels[0].muted && !channels[0].audible,
            "a track silenced by another's solo read as muted or as sounding"
        );
        assert!(channels[1].audible && channels[1].soloed);
    }

    #[test]
    fn unity_is_the_line_the_meter_tops_out_at() {
        // Full scale and a fader at unity are the same number, so they
        // must be the same height. That is the whole point of one ruler.
        assert_eq!(place_of_amp(1.0), unity_place());
        assert!(
            unity_place() < 1.0,
            "no room left for the boost the model allows"
        );
        assert!(unity_place() > 0.9, "the boost band swallowed the meter");
    }

    #[test]
    fn the_ruler_runs_from_the_floor_to_the_boost() {
        assert_eq!(place_of_db(meter::FLOOR_DB), 0.0);
        assert_eq!(place_of_db(BOOST_DB), 1.0);
        assert_eq!(
            place_of_db(meter::FLOOR_DB - 20.0),
            0.0,
            "below the floor is the floor"
        );
        assert_eq!(place_of_amp(0.0), 0.0, "silence has to be somewhere");
    }

    #[test]
    fn the_boost_band_is_exactly_what_the_model_permits() {
        // 1.5 linear is the registered top of `track.volume`.
        let top = meter::amp_to_db(1.5);
        assert!(
            (top - BOOST_DB).abs() < 1e-4,
            "the drawn overtravel ({BOOST_DB}) is not the range the document allows ({top})"
        );
    }

    #[test]
    fn silence_lights_nothing_and_full_scale_lights_everything() {
        let silent = place_of_amp(0.0);
        let full = place_of_amp(1.0);
        for cell in 0..CELLS {
            assert_eq!(
                cell_coverage(cell, silent),
                0.0,
                "cell {cell} lit at silence"
            );
            // Full scale is the top of the ladder by construction, so the
            // last cell only misses it by the arithmetic — not by a rung.
            assert!(
                cell_coverage(cell, full) > 0.999,
                "cell {cell} dark at full scale"
            );
        }
    }

    #[test]
    fn cells_light_from_the_bottom_and_the_top_one_is_partial() {
        // Deliberately NOT half: half of the ladder falls exactly on a
        // cell boundary, which is the one level with no partial cell.
        let level = unity_place() * 0.55;
        let lit: Vec<f32> = (0..CELLS).map(|cell| cell_coverage(cell, level)).collect();
        assert_eq!(lit[0], 1.0, "the bottom cell is not full below half scale");
        assert_eq!(lit[CELLS - 1], 0.0, "the top cell lit below half scale");
        // Never brighter going up: a meter that did would not read as a bar.
        for pair in lit.windows(2) {
            assert!(pair[0] >= pair[1], "coverage climbed: {lit:?}");
        }
        assert!(
            lit.iter().any(|c| *c > 0.0 && *c < 1.0),
            "no partial cell — the meter can only move a whole cell at a time"
        );
    }

    #[test]
    fn a_level_on_a_cell_boundary_is_the_one_with_no_partial_cell() {
        // Stated as a test rather than left as a surprise: exactly seven
        // fourteenths lights seven whole cells and no fraction.
        let lit: Vec<f32> = (0..CELLS)
            .map(|cell| cell_coverage(cell, unity_place() / 2.0))
            .collect();
        assert!(lit.iter().all(|c| *c == 0.0 || *c == 1.0), "{lit:?}");
    }
}
