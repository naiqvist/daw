//! SPECTRA's face: a standing rake of partials on the bin lattice.
//!
//! A phase vocoder works on a linear bin axis, not an octave one, so
//! this is the one card on the desk whose spectrum is drawn linearly —
//! and it looks different from every other card for exactly that
//! reason. The rake is where the section's own analysis says the sound
//! is: its centre of mass, how wide that mass is spread, and how much
//! the frame moved since the last one.
//!
//! A frozen frame stops moving because the flux the engine measured
//! goes to nothing, not because the card was told to stop. The two
//! choir voices stand as ghost rakes at their own intervals, so a
//! chord is visible as a chord.

use super::*;
use crate::console::SectionParams;
use crate::params::console::spectra as p;
use crate::ui::chrome;
use crate::ui::nav_cursor;

/// The teeth the rake is drawn with.
const TEETH: usize = 26;
/// The five things the section can be.
const MODES: usize = 5;

struct Lay {
    field: egui::Rect,
    mode: egui::Rect,
    freeze: egui::Rect,
    blur: egui::Rect,
    pitch: egui::Rect,
    voice_a: egui::Rect,
    voice_b: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::MODE, self.mode),
            (p::FREEZE, self.freeze),
            (p::BLUR, self.blur),
            (p::PITCH, self.pitch),
            (p::VOICE_A, self.voice_a),
            (p::VOICE_B, self.voice_b),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, bay: Option<egui::Rect>, plinth: Option<egui::Rect>) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let field =
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + 0.46 * h));
    let row = |top: f32, from: f32, to: f32| {
        egui::Rect::from_min_max(
            egui::pos2(inner.left() + w * from, top),
            egui::pos2(inner.left() + w * to, top + 13.0),
        )
    };
    let first = field.bottom() + 7.0;
    Lay {
        // PITCH is the rake itself: shifting it walks the whole comb.
        pitch: field,
        field,
        mode: bay.unwrap_or_else(|| {
            egui::Rect::from_min_max(
                egui::pos2(inner.right() - 18.0, inner.top()),
                egui::pos2(inner.right(), inner.top() + 28.0),
            )
        }),
        freeze: row(first, 0.0, 0.20),
        blur: row(first, 0.26, 1.0),
        voice_a: row(first + 17.0, 0.0, 0.46),
        voice_b: row(first + 17.0, 0.54, 1.0),
        mix: plinth.unwrap_or_else(|| row(first + 34.0, 0.0, 1.0)),
    }
}

/// A rake standing at `centre` with `spread`, shifted by `semitones`.
fn rake(field: egui::Rect, centre: f32, spread: f32, semitones: f32) -> (f32, f32) {
    // A semitone is a ratio, and the axis is linear in bins, so a shift
    // MULTIPLIES the centre — which is why a pitched-up rake spreads
    // out as it climbs, exactly as the vocoder's own bins do.
    let shifted = (centre * 2f32.powf(semitones / 12.0)).clamp(0.0, 1.0);
    let width = (spread * 2f32.powf(semitones / 12.0)).clamp(0.01, 1.0);
    (egui::lerp(field.x_range(), shifted), field.width() * width)
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, face.bay(), face.plinth());
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let field = lay.field;
    let mut shapes = Vec::new();

    let centroid = face.anim("centroid", said.bands[0].clamp(0.0, 1.0), 0.09);
    let spread = face.anim("spread", said.bands[1].clamp(0.0, 1.0), 0.11);
    let flux = face.anim("flux", said.bands[2].clamp(0.0, 1.0), 0.08);
    let height = tool::norm(said.reduction_db, -60.0, 0.0);
    let frozen = face.value(p::FREEZE) >= 0.5;
    let mode = face.value(p::MODE).round().clamp(0.0, 4.0) as usize;
    let blur = face.place(p::BLUR);

    // THE BIN LATTICE: linear, and ruled as such, because that is what
    // the section actually works on.
    for i in 0..=8 {
        let x = egui::lerp(field.x_range(), i as f32 / 8.0);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom() - if i % 2 == 0 { 5.0 } else { 3.0 }),
                egui::pos2(x, field.bottom()),
            ],
            Weight::Hair,
            tool::fade(edge, 0.6),
        );
    }

    // THE TWO CHOIR VOICES, behind: ghost rakes at their own intervals,
    // drawn only when the section is actually a choir.
    if mode == 3 {
        for (param, tag) in [(p::VOICE_A, "va"), (p::VOICE_B, "vb")] {
            let (at, wide) = rake(field, centroid, spread, face.value(param));
            for i in 0..TEETH {
                let t = (i as f32 + 0.5) / TEETH as f32;
                let x = at - wide * 0.5 + wide * t;
                if x < field.left() || x > field.right() {
                    continue;
                }
                let mass = (t * core::f32::consts::PI).sin();
                chrome::trace(
                    &mut shapes,
                    &[
                        egui::pos2(x, field.bottom()),
                        egui::pos2(x, field.bottom() - field.height() * 0.45 * mass * height),
                    ],
                    Weight::Hair,
                    tool::fade(tool::MID_INK, 0.26),
                );
            }
            let _ = tag;
        }
    }

    // THE RAKE: where the analysis says the sound is, how wide it is
    // spread, and how much it moved. A frozen frame stops shimmering
    // because the MEASURED flux went to nothing.
    let (at, wide) = rake(field, centroid, spread, face.value(p::PITCH));
    for i in 0..TEETH {
        let t = (i as f32 + 0.5) / TEETH as f32;
        let x = at - wide * 0.5 + wide * t;
        if x < field.left() || x > field.right() {
            continue;
        }
        let mass = (t * core::f32::consts::PI).sin();
        // Blur smears the teeth into each other; flux is how alive the
        // frame is, and it lights the tips.
        let tooth = field.height() * (0.15 + 0.75 * mass * height) * (1.0 - 0.35 * blur);
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x, field.bottom()),
                egui::pos2(x, field.bottom() - tooth),
            ],
            if flux > 0.25 {
                Weight::Heavy
            } else {
                Weight::Hair
            },
            tool::mix_ink(tool::fade(ink, 0.55), face.live(), flux),
        );
    }
    // The centre of mass itself: one post, with the spread bracketed
    // either side of it.
    chrome::trace(
        &mut shapes,
        &[egui::pos2(at, field.top()), egui::pos2(at, field.bottom())],
        Weight::Heavy,
        tool::mix_ink(face.live(), face.focus(), face.lit(p::PITCH)),
    );
    for side in [-1.0f32, 1.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(at + side * wide * 0.5, field.top() + 3.0),
                egui::pos2(at + side * wide * 0.5, field.top() + 9.0),
            ],
            Weight::Hair,
            tool::fade(edge, 1.1),
        );
    }
    tool::halo(&mut shapes, lay.pitch, face.lit(p::PITCH), face.focus());

    // MODE: five detents in the bay.
    let slot = lay.mode;
    for i in 0..MODES {
        let y = egui::lerp(
            (slot.top() + 4.0)..=(slot.bottom() - 4.0),
            i as f32 / (MODES - 1) as f32,
        );
        let here = i == mode;
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
    tool::halo(&mut shapes, slot, face.lit(p::MODE), face.focus());

    // FREEZE: a latch that is closed or open, and when it is closed the
    // rake above has already stopped moving of its own accord.
    let latch = lay.freeze;
    chrome::octagon(
        &mut shapes,
        latch.shrink(2.0),
        2.0,
        Some(if frozen { face.live() } else { face.ground() }),
        Some((Weight::Hair, if frozen { ink } else { edge })),
    );
    tool::halo(&mut shapes, latch, face.lit(p::FREEZE), face.focus());

    // BLUR, the two voices, and MIX: four rails at the foot, the voices
    // bipolar because an interval goes either way.
    tool::slider(
        &mut shapes,
        lay.blur.shrink2(egui::vec2(4.0, 4.0)),
        blur,
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::BLUR)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.blur, face.lit(p::BLUR), face.focus());
    for (rect, param) in [(lay.voice_a, p::VOICE_A), (lay.voice_b, p::VOICE_B)] {
        tool::swing_rail(
            &mut shapes,
            rect.shrink2(egui::vec2(4.0, 4.0)),
            face.swing(param),
            13,
            tool::mix_ink(tool::MID_INK, face.focus(), face.lit(param)),
            tool::fade(edge, 0.55),
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }
    tool::slider(
        &mut shapes,
        lay.mix.shrink2(egui::vec2(4.0, 4.0)),
        face.place(p::MIX),
        9,
        tool::mix_ink(ink, face.focus(), face.lit(p::MIX)),
        tool::fade(edge, 0.6),
        6.0,
    );
    tool::halo(&mut shapes, lay.mix, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // The mark sweeps with the centre of mass: standing on this card,
    // the cursor is the spectrum's own centroid.
    face.mark_signed(&lay, nav_cursor::Signature::Sweep(centroid * 2.0 - 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid() -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Spectra), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Spectra).shrink(3.0);
        (
            piece,
            lay(
                glass,
                strip::bay_rect(piece, SectionKind::Spectra),
                strip::plinth_rect(piece, SectionKind::Spectra),
            ),
        )
    }

    #[test]
    fn every_spectra_parameter_has_one_instrument() {
        let (piece, lay) = laid();
        for def in SectionKind::Spectra.table() {
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
        let _ = SectionParams::of(SectionKind::Spectra);
    }

    /// A shift is a RATIO on a linear bin axis, so pitching up walks
    /// the rake right and widens it — which is what the vocoder does.
    #[test]
    fn a_pitch_shift_multiplies_the_rake_rather_than_sliding_it() {
        let (_, lay) = laid();
        let (at, wide) = rake(lay.field, 0.25, 0.10, 0.0);
        let (up, wider) = rake(lay.field, 0.25, 0.10, 12.0);
        assert!(up > at + 10.0, "an octave up did not move the rake");
        assert!(wider > wide, "an octave up did not widen it");
        let (down, narrower) = rake(lay.field, 0.25, 0.10, -12.0);
        assert!(down < at, "an octave down did not move the rake");
        assert!(narrower < wide);
        // And it never leaves the field, however far it is pushed.
        for shift in [-24.0f32, 24.0] {
            let (x, w) = rake(lay.field, 0.9, 0.5, shift);
            assert!(x >= lay.field.left() - 0.01 && x <= lay.field.right() + 0.01);
            assert!(w > 0.0 && w <= lay.field.width() + 0.01);
        }
    }
}
