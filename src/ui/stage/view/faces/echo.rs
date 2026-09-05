//! ECHO's face: the loop, drawn as a closed circuit.
//!
//! A delay is a piece of signal going round, so the card is the round
//! trip: a loop with a record head where the signal enters, a play head
//! where it comes back, and a lap for every time the feedback will
//! carry it round again.
//!
//! The loop's SIZE is the time the line is actually reading — the
//! glided time, so a change of division is watched sliding rather than
//! jumping — and the play head WANDERS off it by exactly the wow the
//! engine reported, which is the one thing about a delay that no
//! setting can tell you.

use super::*;
use crate::console::SectionParams;
use crate::params::console::echo as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The divisions the sync switch steps through, and free.
const DIVISIONS: usize = 6;
/// The longest loop the card draws, in milliseconds.
const SPAN_MS: f32 = 1_200.0;
/// The most ghost laps drawn at full feedback.
const LAPS: usize = 5;

struct Lay {
    loop_rect: egui::Rect,
    sync: egui::Rect,
    time: egui::Rect,
    feedback: egui::Rect,
    tone: egui::Rect,
    wow: egui::Rect,
    pingpong: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::SYNC, self.sync),
            (p::TIME, self.time),
            (p::FEEDBACK, self.feedback),
            (p::TONE, self.tone),
            (p::WOW, self.wow),
            (p::PINGPONG, self.pingpong),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let loop_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left() + 0.06 * w, inner.top() + 0.06 * h),
        egui::pos2(inner.right() - 0.06 * w, inner.top() + 0.44 * h),
    );
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 13.0),
        )
    };
    let first = loop_rect.bottom() + 8.0;
    Lay {
        // TIME is the loop's own circumference.
        time: loop_rect,
        loop_rect,
        sync: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 18.0, inner.top()),
                egui::pos2(inner.right(), inner.top() + 30.0),
            )
        }),
        feedback: row(first, 0.0, 0.46),
        tone: row(first, 0.54, 1.0),
        wow: row(first + 17.0, 0.0, 0.46),
        pingpong: row(first + 17.0, 0.54, 0.72),
        mix: plinth.unwrap_or_else(|| row(first + 17.0, 0.78, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    let feedback = face.place(p::FEEDBACK);
    let ping = face.value(p::PINGPONG) >= 0.5;
    // The LIVE time the line is reading, glided — so changing division
    // is watched sliding rather than jumping.
    let live_ms = if said.bands[0] > 0.0 {
        said.bands[0]
    } else {
        face.value(p::TIME)
    };
    let size = face.anim("size", (live_ms / SPAN_MS).clamp(0.05, 1.0), 0.10);

    // THE LOOP: a rounded circuit whose size is the delay itself. A
    // short delay is a small loop and a long one fills the card.
    let full = lay.loop_rect;
    let ring = egui::Rect::from_center_size(
        full.center(),
        egui::vec2(
            full.width() * (0.25 + 0.75 * size),
            full.height() * (0.35 + 0.65 * size),
        ),
    );
    let path = |rect: egui::Rect| {
        let c = 6.0f32.min(rect.width() * 0.4).min(rect.height() * 0.4);
        vec![
            egui::pos2(rect.left() + c, rect.top()),
            egui::pos2(rect.right() - c, rect.top()),
            egui::pos2(rect.right(), rect.top() + c),
            egui::pos2(rect.right(), rect.bottom() - c),
            egui::pos2(rect.right() - c, rect.bottom()),
            egui::pos2(rect.left() + c, rect.bottom()),
            egui::pos2(rect.left(), rect.bottom() - c),
            egui::pos2(rect.left(), rect.top() + c),
            egui::pos2(rect.left() + c, rect.top()),
        ]
    };
    // THE LAPS: one ghost loop per time the feedback will carry it
    // round again, each dimmer than the last. A loop at nothing has
    // none, which is a delay with one repeat.
    let laps = (feedback * LAPS as f32).round() as usize;
    for lap in (1..=laps).rev() {
        let shrunk = ring.shrink(lap as f32 * 3.0);
        if shrunk.is_positive() {
            chrome::trace(
                &mut shapes,
                &path(shrunk),
                Weight::Hair,
                tool::fade(face.live(), 0.5 / (lap as f32 + 1.0)),
            );
        }
    }
    chrome::trace(&mut shapes, &path(ring), Weight::Heavy, ink);
    tool::halo(&mut shapes, lay.loop_rect, face.lit(p::TIME), face.focus());

    // THE RECORD HEAD, fixed at the loop's top-left; and THE PLAY HEAD,
    // which stands where the wow the engine measured has put it.
    chrome::pad(&mut shapes, ring.left_top(), chrome::PAD, ink, true);
    let wander = said.bands[1];
    let drift = (wander / 8.0).clamp(-1.0, 1.0) * ring.width() * 0.12;
    let head = egui::pos2(ring.right() + drift, ring.center().y);
    chrome::octagon(
        &mut shapes,
        egui::Rect::from_center_size(head, egui::vec2(9.0, 9.0)),
        2.0,
        Some(face.live()),
        Some((Weight::Hair, ink)),
    );
    // The wow's own reach, marked either side of the head so the wander
    // is read against what it may do.
    for side in [-1.0f32, 1.0] {
        let reach = ring.width() * 0.12 * face.place(p::WOW);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(ring.right() + side * reach, ring.center().y - 5.0),
                egui::pos2(ring.right() + side * reach, ring.center().y + 5.0),
            ],
            Weight::Hair,
            tool::fade(edge, 0.9),
        );
    }

    // PING-PONG: when it is on, the loop's two sides are crossed, which
    // is exactly what the section does to them.
    let cross = lay.pingpong;
    if ping {
        chrome::trace(
            &mut shapes,
            &[cross.left_top(), cross.right_bottom()],
            Weight::Heavy,
            face.live(),
        );
        chrome::trace(
            &mut shapes,
            &[cross.right_top(), cross.left_bottom()],
            Weight::Heavy,
            face.live(),
        );
    } else {
        for y in [cross.top() + 4.0, cross.bottom() - 4.0] {
            chrome::trace(
                &mut shapes,
                &[egui::pos2(cross.left(), y), egui::pos2(cross.right(), y)],
                Weight::Hair,
                tool::fade(edge, 1.0),
            );
        }
    }
    tool::halo(&mut shapes, cross, face.lit(p::PINGPONG), face.focus());

    // SYNC: the divisions, in the bay. The one that is chosen is the
    // one the loop is actually running, and the free position is the
    // bottom detent.
    let slot = lay.sync;
    let chosen = face.value(p::SYNC).round().clamp(0.0, 5.0) as usize;
    for i in 0..DIVISIONS {
        let y = egui::lerp(
            (slot.top() + 4.0)..=(slot.bottom() - 4.0),
            i as f32 / (DIVISIONS - 1) as f32,
        );
        let here = i == chosen;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(slot.left() + 2.0, y),
                egui::pos2(slot.right() - if here { 1.0 } else { 4.0 }, y),
            ],
            if here { Weight::Heavy } else { Weight::Hair },
            if here { ink } else { tool::fade(edge, 0.7) },
        );
    }
    tool::halo(&mut shapes, slot, face.lit(p::SYNC), face.focus());

    // FEEDBACK, TONE, WOW and MIX: four rails at the foot. TONE is the
    // filter in the loop, so its rail is drawn as a wedge shutting.
    tool::slider(
        &mut shapes,
        lay.feedback.shrink2(egui::vec2(4.0, 4.0)),
        feedback,
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::FEEDBACK)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(
        &mut shapes,
        lay.feedback,
        face.lit(p::FEEDBACK),
        face.focus(),
    );
    let tone = lay.tone;
    let open = tool::norm(face.value(p::TONE), 200.0, 12_000.0);
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(tone.left() + 2.0, tone.bottom() - 2.0),
            egui::pos2(
                egui::lerp((tone.left() + 2.0)..=(tone.right() - 2.0), open.max(0.05)),
                tone.top() + 2.0,
            ),
            egui::pos2(
                egui::lerp((tone.left() + 2.0)..=(tone.right() - 2.0), open.max(0.05)),
                tone.bottom() - 2.0,
            ),
        ],
        Weight::Hair,
        tool::mix_ink(tool::fade(edge, 0.9), ink, open),
    );
    tool::halo(&mut shapes, tone, face.lit(p::TONE), face.focus());
    for (rect, param) in [(lay.wow, p::WOW), (lay.mix, p::MIX)] {
        tool::slider(
            &mut shapes,
            rect.shrink2(egui::vec2(4.0, 4.0)),
            face.place(param),
            9,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    face.painter.extend(shapes);

    // The mark wanders with the play head: the wow is on the cursor too.
    face.mark_signed(
        &lay,
        nav_cursor::Signature::Sweep((wander / 8.0).clamp(-1.0, 1.0)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Echo), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Echo).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Echo),
                strip::plinth_rect(piece, SectionKind::Echo),
            ),
        )
    }

    #[test]
    fn every_echo_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Echo.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(
                piece.expand(strip::TONGUE).contains_rect(rect),
                "{} left the piece",
                def.name
            );
        }
        assert_eq!(lay.controls().len(), 7);
        let _ = SectionParams::of(SectionKind::Echo);
    }

    /// The loop's size is the delay: a longer time is a bigger circuit,
    /// and it never grows past the space it was given.
    #[test]
    fn a_longer_delay_is_a_bigger_loop() {
        let (_, lay) = laid();
        let ring = |ms: f32| {
            let size = (ms / SPAN_MS).clamp(0.05, 1.0);
            egui::Rect::from_center_size(
                lay.loop_rect.center(),
                egui::vec2(
                    lay.loop_rect.width() * (0.25 + 0.75 * size),
                    lay.loop_rect.height() * (0.35 + 0.65 * size),
                ),
            )
        };
        assert!(ring(1_000.0).width() > ring(120.0).width() + 20.0);
        assert!(lay.loop_rect.contains_rect(ring(SPAN_MS * 4.0)));
        assert!(ring(1.0).width() > 4.0, "a short delay vanished");
    }
}
