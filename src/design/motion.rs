//! Motion: the one thing that may vary with state, because it IS the state.
//!
//! Ornament never moves on its own. While the transport rolls, current
//! flows: dashes travel along the sounding track's traces one dash per
//! beat, and the playhead pulses on the beat. Stopped, every trace is
//! solid and still. Both come from the transport's own beat phase rather
//! than a wall clock, so a shot and a frame agree, and a parked deck is a
//! parked deck.

use eframe::egui::Color32;

/// Where the transport is inside the beat, and whether it is moving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phase {
    pub rolling: bool,
    /// 0 at the beat, rising to 1 just before the next.
    pub beat: f32,
}

impl Phase {
    /// A parked deck.
    pub const STILL: Phase = Phase {
        rolling: false,
        beat: 0.0,
    };

    pub fn of(rolling: bool, beat: f32) -> Self {
        Self {
            rolling,
            beat: if beat.is_finite() {
                beat.rem_euclid(1.0)
            } else {
                0.0
            },
        }
    }

    /// How far the dashes have travelled, 0..1 of one dash-and-gap. Zero
    /// when still, so a parked trace is drawn solid by the caller.
    pub fn dash(self) -> f32 {
        if self.rolling { self.beat } else { 0.0 }
    }

    /// The beat's decay: 1 on the beat, falling to 0. Zero when still.
    pub fn pulse(self) -> f32 {
        if self.rolling {
            let t = 1.0 - self.beat;
            t * t
        } else {
            0.0
        }
    }
}

/// `cool` at rest, `hot` on the beat, decaying between: the playing bar's
/// colour, and any other mark that breathes with the transport.
pub fn pulse_ink(hot: Color32, cool: Color32, phase: Phase) -> Color32 {
    let t = phase.pulse().clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(cool.r(), hot.r()),
        mix(cool.g(), hot.g()),
        mix(cool.b(), hot.b()),
        mix(cool.a(), hot.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parked_deck_neither_flows_nor_pulses() {
        let still = Phase::of(false, 0.4);
        assert_eq!(still.dash(), 0.0);
        assert_eq!(still.pulse(), 0.0);
        assert_eq!(
            pulse_ink(Color32::WHITE, Color32::BLACK, still),
            Color32::BLACK
        );
    }

    #[test]
    fn the_pulse_is_loudest_on_the_beat_and_gone_before_the_next() {
        assert_eq!(Phase::of(true, 0.0).pulse(), 1.0);
        assert!(Phase::of(true, 0.5).pulse() < 0.3);
        assert!(Phase::of(true, 0.99).pulse() < 0.001);
        assert_eq!(
            pulse_ink(Color32::WHITE, Color32::BLACK, Phase::of(true, 0.0)),
            Color32::WHITE
        );
    }

    #[test]
    fn the_phase_wraps_and_survives_nonsense() {
        assert!((Phase::of(true, 1.25).beat - 0.25).abs() < 1e-6);
        assert!((Phase::of(true, -0.25).beat - 0.75).abs() < 1e-6);
        assert_eq!(Phase::of(true, f32::NAN).beat, 0.0);
    }
}
