//! The mixer, drawn: the strip's casing, meters, sends, switches and pan.
//!
//! The model half — channels, returns, the one ruler, the steps — stays
//! in `stage::mixer` and names no toolkit; this is what hangs beneath the
//! heads once that has been measured.

use super::*;
pub(super) use crate::ui::stage::mixer::*;

/// Where a send's rail runs inside a strip.
///
/// The mixer draws the cable that leaves it, and the cable must land on
/// the rail rather than near it — so the one piece of arithmetic that
/// places the rail is shared rather than repeated.
pub fn send_y(strip: egui::Rect, gap: f32, sends: usize, switches: bool, slot: usize) -> f32 {
    let inner = strip.shrink(gap.max(3.0));
    let pan_top = inner.max.y - PAN_H;
    let switch_h = if switches { SWITCH_H + gap } else { 0.0 };
    let sends_top = pan_top - switch_h - (sends as f32 * SEND_H + gap);
    sends_top + slot as f32 * SEND_H + SEND_H * 0.5
}

/// Where the send's mark stands on that rail, across the strip.
pub fn send_x(strip: egui::Rect, gap: f32, send: f32) -> f32 {
    let inner = strip.shrink(gap.max(3.0));
    let left = inner.min.x + 16.0;
    let right = inner.max.x - 4.0;
    left + send.clamp(0.0, 1.0) * (right - left)
}

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

/// A channel's casing: dark powered glass with a cut outer shell, inset
/// signal frame and hard service nodes. A return keeps the same machine
/// language but breaks its inner frame into dashes, so it still reads as a
/// destination rather than a source.
fn draw_casing(
    painter: &egui::Painter,
    strip: egui::Rect,
    is_return: bool,
    alpha: &design::Alphabet,
    variant: u8,
) {
    let mut casing = Vec::new();
    let powered = alpha
        .live_dim
        .color
        .gamma_multiply(if is_return { 0.52 } else { 0.68 });
    circuit::relic_frame(
        &mut casing,
        strip,
        if is_return {
            alpha.ground.color
        } else {
            alpha.well.color
        },
        Weight::Heavy,
        powered,
    );
    let frame = strip.shrink(5.0);
    if is_return {
        let mut points = circuit::relic_points(frame);
        if let Some(first) = points.first().copied() {
            points.push(first);
        }
        circuit::dashes(&mut casing, &points, 0.0, Weight::Hair, powered);
    } else {
        casing.push(egui::Shape::closed_line(
            circuit::relic_points(frame),
            egui::Stroke::new(Weight::Hair.px(), alpha.edge.color),
        ));
    }
    let upper = strip.top() + 8.0;
    circuit::trace(
        &mut casing,
        &[
            egui::pos2(strip.left() + 10.0, upper),
            egui::pos2(strip.center().x - 8.0, upper),
            egui::pos2(strip.center().x, upper + 8.0),
            egui::pos2(strip.right() - 10.0, upper + 8.0),
        ],
        Weight::Hair,
        powered,
    );
    circuit::pad(
        &mut casing,
        egui::pos2(strip.right() - 8.0, strip.bottom() - 8.0),
        circuit::PAD - 1.0,
        powered,
        variant.is_multiple_of(2),
    );
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
            &channel.send_letters[slot].to_string(),
            alpha.edge.color,
        );
        let left = egui::pos2(zone.min.x + 16.0, y);
        let right = egui::pos2(zone.max.x - 4.0, y);
        let powered = alpha.live_dim.color.gamma_multiply(0.58);
        circuit::rail(&mut shapes, left, right, &[0.0, 1.0], powered);
        let x = left.x + send.clamp(0.0, 1.0) * (right.x - left.x);
        let sending = *send > 0.0;
        circuit::relic_node(
            &mut shapes,
            egui::pos2(x, y),
            3.5,
            if sending { alpha.ink.color } else { powered },
            if sending { alpha.ink.color } else { powered },
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
    let powered = alpha.live_dim.color.gamma_multiply(0.66);
    circuit::relic_frame(&mut shapes, well, alpha.ground.color, Weight::Hair, powered);
    circuit::relic_node(
        &mut shapes,
        well.center_top() + egui::vec2(0.0, 7.0),
        5.0,
        powered,
        alpha.live_dim.color,
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
        circuit::tick_bar(&mut shapes, lane, CELLS, lit, ink, alpha.well.color, false);
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
        powered,
    );
    let handle_y = chart.max.y - chart.height() * place_of_amp(channel.gain);
    circuit::relic_node(
        &mut shapes,
        egui::pos2(fader_x, handle_y),
        7.0,
        alpha.ink.color,
        alpha.ink.color,
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
        powered.gamma_multiply(0.82),
    );
    for db in [0_i32, -6, -12, -24, -48] {
        let y = chart.max.y - chart.height() * place_of_db(db as f32);
        circuit::pad(
            &mut scale,
            egui::pos2(rail_x, y),
            circuit::PAD - 2.0,
            powered,
            true,
        );
        block::paint(
            painter,
            egui::Id::new(("mixer-db", chart.min.x.round() as i32, db)),
            egui::pos2(rail_x - gap, y),
            egui::Align2::RIGHT_CENTER,
            block::unit::MICRO,
            &db.to_string(),
            powered,
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

/// Mute and solo, as two small cut-glass keycaps.
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
        let powered = alpha.live_dim.color.gamma_multiply(0.64);
        let mut shapes = Vec::new();
        circuit::relic_frame(
            &mut shapes,
            rect,
            if on {
                alpha.ink.color
            } else {
                alpha.ground.color
            },
            if on { Weight::Bold } else { Weight::Hair },
            if on { alpha.ink.color } else { powered },
        );
        for shape in shapes {
            painter.add(shape);
        }
        block::paint(
            painter,
            egui::Id::new(("mixer-switch", zone.min.x.round() as i32, label)),
            rect.center(),
            egui::Align2::CENTER_CENTER,
            block::unit::MICRO,
            label,
            if on { alpha.ground.color } else { powered },
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
    let powered = alpha.live_dim.color.gamma_multiply(0.62);
    circuit::rail(&mut shapes, left, right, &[0.0, 0.5, 1.0], powered);
    let x = left.x + channel.pan.clamp(-1.0, 1.0).mul_add(0.5, 0.5) * (right.x - left.x);
    circuit::relic_node(
        &mut shapes,
        egui::pos2(x, rail_y),
        5.0,
        alpha.ink.color,
        alpha.ink.color,
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

    /// The cable that leaves a send lands ON its rail, and the mark it
    /// carries stands where the amount says — which is the whole reason
    /// the arithmetic is shared rather than repeated.
    #[test]
    fn the_send_cable_lands_on_the_rail_it_leaves() {
        let strip = egui::Rect::from_min_size(egui::pos2(40.0, 80.0), egui::vec2(96.0, 300.0));
        let gap = 8.0;
        let first = send_y(strip, gap, 2, true, 0);
        let second = send_y(strip, gap, 2, true, 1);
        assert!(second > first, "the second rail is under the first");
        assert!((second - first - SEND_H).abs() < 0.01);
        assert!(first > strip.top() && first < strip.bottom());
        assert!(second > strip.top() && second < strip.bottom());
        // A switchless strip — a return — puts its rails lower, because
        // it has no switch row above the pan to make room for.
        assert!(send_y(strip, gap, 2, false, 0) > first);
        // The mark travels the whole rail and never leaves the strip.
        let shut = send_x(strip, gap, 0.0);
        let open = send_x(strip, gap, 1.0);
        assert!(open > shut + 40.0, "the mark barely moved");
        assert!(shut > strip.left() && open < strip.right());
        assert_eq!(send_x(strip, gap, 2.0), open, "an amount past full clamps");
    }
}
