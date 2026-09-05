//! TAPE's face: the loop, threaded through the card's own walls.
//!
//! The first of the two returns, and a whole return track in one card,
//! so it gets the machine rather than a panel: a tape span running
//! between two capstans with a record head at one end and a play head
//! at the other. TIME is the span between them — a long delay is a long
//! run of tape and a short one is a tight loop, which is the fact a
//! millisecond figure never conveys.
//!
//! WOW and FLUTTER are the two unsteadinesses a tape machine has, and
//! they are drawn as what they are: a slow bow in the tape's path and a
//! fast shiver on it, both walking on the TRANSPORT's own beat so a
//! parked deck is a still picture. HISS is the oxide's grain, and TONE
//! is the gap the play head is set to.

use super::*;
use crate::console::SectionParams;
use crate::params::console::tape as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// How many passes the loop is drawn holding at full feedback.
const PASSES: usize = 5;
/// The grain the oxide shows at full hiss.
const GRAINS: usize = 30;

/// A deterministic speck, so a parked deck is a still picture.
fn grain(i: usize) -> f32 {
    let h = (i as u32).wrapping_mul(2_654_435_761) >> 11;
    (h & 0xff) as f32 / 255.0
}

struct Lay {
    /// The tape's own span, between the capstans.
    span: egui::Rect,
    time: egui::Rect,
    feedback: egui::Rect,
    wow: egui::Rect,
    flutter: egui::Rect,
    hiss: egui::Rect,
    tone: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::TIME, self.time),
            (p::FEEDBACK, self.feedback),
            (p::WOW, self.wow),
            (p::FLUTTER, self.flutter),
            (p::HISS, self.hiss),
            (p::TONE, self.tone),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let span =
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + 0.42 * h));
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 13.0),
        )
    };
    let first = span.bottom() + 8.0;
    Lay {
        // TIME is the span between the capstans: the tape itself.
        time: span,
        span,
        feedback: row(first, 0.0, 0.46),
        tone: bay.unwrap_or_else(|| row(first, 0.54, 1.0)),
        wow: row(first + 17.0, 0.0, 0.46),
        flutter: row(first + 17.0, 0.54, 1.0),
        hiss: plinth.unwrap_or_else(|| row(first + 34.0, 0.0, 1.0)),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let mut shapes = Vec::new();

    let time = face.anim("time", tool::norm(face.value(p::TIME), 50.0, 1_500.0), 0.12);
    let feedback = face.place(p::FEEDBACK);
    let wow = face.place(p::WOW);
    let flutter = face.place(p::FLUTTER);
    let beat = face.phase.beat * core::f32::consts::TAU;
    let rolling = if face.phase.rolling { 1.0 } else { 0.0 };
    let carrying = tool::norm(face.level_db(), -54.0, 0.0);

    // THE CAPSTANS: the record head fixed at the left, the play head as
    // far right as the time puts it. The tape between them is the delay.
    let span = lay.span;
    let record = egui::pos2(span.left() + 10.0, span.center().y);
    let play = egui::pos2(
        egui::lerp((span.left() + 34.0)..=(span.right() - 10.0), time),
        span.center().y,
    );
    for (hub, filled) in [(record, true), (play, false)] {
        chrome::trace(
            &mut shapes,
            &tool::arc(hub, 8.0, 0.0, 360.0, 20),
            Weight::Hair,
            tool::fade(edge, 1.1),
        );
        chrome::pad(&mut shapes, hub, chrome::PAD - 1.0, ink, filled);
    }

    // THE TAPE: a path from one capstan to the other, bowed slowly by
    // the WOW and shivering with the FLUTTER. Both ride the transport's
    // own beat, so a parked deck is a still picture.
    let path: Vec<egui::Pos2> = (0..=48)
        .map(|i| {
            let t = i as f32 / 48.0;
            let x = egui::lerp(record.x..=play.x, t);
            let bow =
                (t * core::f32::consts::PI).sin() * wow * 7.0 * (beat + t * 0.6).sin() * rolling;
            let shiver = flutter * 2.2 * (beat * 9.0 + t * 22.0).sin() * rolling;
            egui::pos2(x, span.center().y + bow + shiver)
        })
        .collect();
    chrome::trace(&mut shapes, &path, Weight::Heavy, face.live());
    // The passes: one ghost span per lap the feedback will keep.
    let passes = (feedback * PASSES as f32).round() as usize;
    for lap in 1..=passes {
        let drop = lap as f32 * 4.0;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(record.x, span.center().y + drop),
                egui::pos2(play.x, span.center().y + drop),
            ],
            Weight::Hair,
            tool::fade(face.live(), 0.45 / (lap as f32 + 0.6)),
        );
    }
    tool::halo(&mut shapes, lay.span, face.lit(p::TIME), face.focus());

    // THE OXIDE: grain on the tape, as much as the hiss asks for and
    // only where the tape actually is.
    let grains = (face.place(p::HISS) * GRAINS as f32).round() as usize;
    for i in 0..grains {
        let t = grain(i);
        chrome::dot(
            &mut shapes,
            egui::pos2(
                egui::lerp(record.x..=play.x, t),
                span.center().y + (grain(i + 41) - 0.5) * 12.0,
            ),
            tool::fade(ink, 0.35 + 0.45 * carrying),
        );
    }

    // THE HEAD GAP: the tone control, drawn as the gap the play head is
    // set to. A wide gap is a dull head, which is what the control does.
    let gap = lay.tone;
    let open = tool::norm(face.value(p::TONE), 500.0, 10_000.0);
    tool::iris(
        &mut shapes,
        gap.shrink(2.0),
        open,
        tool::mix_ink(tool::fade(edge, 1.1), face.focus(), face.lit(p::TONE)),
        edge,
    );
    tool::halo(&mut shapes, gap, face.lit(p::TONE), face.focus());

    // The rest: rails at the foot, each named by what it does to the
    // tape above rather than by a word.
    for (rect, param, value) in [
        (lay.feedback, p::FEEDBACK, feedback),
        (lay.wow, p::WOW, wow),
        (lay.flutter, p::FLUTTER, flutter),
        (lay.hiss, p::HISS, face.place(p::HISS)),
    ] {
        tool::slider(
            &mut shapes,
            rect.shrink2(egui::vec2(4.0, 4.0)),
            value,
            9,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.6),
            6.0,
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    face.painter.extend(shapes);

    // The mark carries what the return is actually returning.
    face.mark_signed(&lay, nav_cursor::Signature::Level(carrying));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Tape), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Tape).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Tape),
                strip::plinth_rect(piece, SectionKind::Tape),
            ),
        )
    }

    #[test]
    fn every_tape_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Tape.table() {
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
        assert_eq!(lay.controls().len(), 6);
        let _ = SectionParams::of(SectionKind::Tape);
    }

    /// A longer delay is a longer run of tape, and the play head never
    /// leaves the span or backs into the record head.
    #[test]
    fn a_longer_delay_is_a_longer_run_of_tape() {
        let (_, lay) = laid();
        let head = |ms: f32| {
            egui::lerp(
                (lay.span.left() + 34.0)..=(lay.span.right() - 10.0),
                tool::norm(ms, 50.0, 1_500.0),
            )
        };
        assert!(head(1_500.0) > head(50.0) + 20.0);
        assert!(head(50.0) > lay.span.left() + 20.0, "the heads collided");
        assert!(head(1_500.0) <= lay.span.right());
    }

    /// The oxide's grain is the same every frame: a parked deck is a
    /// still picture, not a fizzing one.
    #[test]
    fn the_oxide_does_not_fizz_on_its_own() {
        for i in 0..40 {
            assert_eq!(grain(i), grain(i));
            assert!((0.0..=1.0).contains(&grain(i)));
        }
        let some: Vec<f32> = (0..24).map(grain).collect();
        assert!(some.iter().any(|v| *v > 0.6));
        assert!(some.iter().any(|v| *v < 0.4));
    }
}
