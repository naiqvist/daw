//! The sine synth device card — the UI face of the engine's built-in
//! sequencer synth, and the first real device.
//!
//! The device layer never imports the engine, so edits leave as plain
//! `(param id, natural value)` data and the app layer translates them to
//! engine letters. The id contract (kept in lockstep with
//! `Node::Seq::apply` in `src/audio/graph.rs`):
//!
//! - `0` — gain, linear `0..=2`
//! - `1` — attack, milliseconds
//! - `2` — release, milliseconds

use crate::ui::device::{card, knob, param::Param};
use crate::ui::theme::Theme;
use eframe::egui;

/// One parameter edit leaving the UI: engine param id + NATURAL value
/// (the engine speaks Hz/ms/gain, not normalized positions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdit {
    pub param: u32,
    pub value: f32,
}

/// Engine param ids, named once.
const P_GAIN: u32 = 0;
const P_ATTACK: u32 = 1;
const P_RELEASE: u32 = 2;

/// UI state of one sine synth device: normalized positions of its knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SineSynthUi {
    pub gain: f32,
    pub attack: f32,
    pub release: f32,
}

impl Default for SineSynthUi {
    fn default() -> Self {
        // Match the engine's SynthParams defaults (gain 1.0, 1 ms, 640 ms).
        let s = spec();
        Self {
            gain: s.gain.default_norm,
            attack: s.attack.default_norm,
            release: s.release.default_norm,
        }
    }
}

struct Spec {
    gain: Param,
    attack: Param,
    release: Param,
}

/// The synth's parameter descriptions. Ranges mirror the engine clamps.
fn spec() -> Spec {
    Spec {
        gain: Param::new(
            "gain",
            crate::ui::device::Mapping::Linear { min: 0.0, max: 2.0 },
            crate::ui::device::Unit::Plain,
        )
        .with_default(1.0),
        attack: Param::ms("attack", 0.05, 5_000.0).with_default(1.0),
        release: Param::ms("release", 1.0, 30_000.0).with_default(640.0),
    }
}

/// Draw the sine synth card. Any knob edit is returned as the natural
/// values the engine expects; an empty vec means nothing changed.
pub fn sine_synth_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut SineSynthUi,
) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    card::card(ui, theme, "sine synth", |ui| {
        // Sections center their content; the knobs just get added.
        card::sections(ui, theme, 3, 1, |ui, i| {
            let (param, value, id) = match i {
                0 => (&s.gain, &mut state.gain, P_GAIN),
                1 => (&s.attack, &mut state.attack, P_ATTACK),
                _ => (&s.release, &mut state.release, P_RELEASE),
            };
            if knob::knob(ui, theme, param, value) {
                edits.push(ParamEdit {
                    param: id,
                    value: param.value(*value),
                });
            }
        });
    });
    edits
}

/// Every parameter of `state` as an edit, whether or not it just changed.
/// What a caller needs after setting the knobs itself — a reset, a preset
/// recall, a project load — since the card only emits on user movement.
pub fn sine_synth_edits(state: &SineSynthUi) -> Vec<ParamEdit> {
    let s = spec();
    vec![
        ParamEdit {
            param: P_GAIN,
            value: s.gain.value(state.gain),
        },
        ParamEdit {
            param: P_ATTACK,
            value: s.attack.value(state.attack),
        },
        ParamEdit {
            param: P_RELEASE,
            value: s.release.value(state.release),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_engine_defaults() {
        // The card must open showing what the engine actually does when no
        // letter has ever been sent: gain 1.0, attack 1 ms, release 640 ms.
        let s = spec();
        let ui = SineSynthUi::default();
        assert!((s.gain.value(ui.gain) - 1.0).abs() < 1e-3);
        assert!((s.attack.value(ui.attack) - 1.0).abs() < 0.05);
        assert!((s.release.value(ui.release) - 640.0).abs() < 5.0);
    }

    #[test]
    fn param_ids_stay_stable() {
        // The wire contract with Node::Seq::apply. Renumbering is a
        // protocol break, not a refactor.
        assert_eq!(P_GAIN, 0);
        assert_eq!(P_ATTACK, 1);
        assert_eq!(P_RELEASE, 2);
    }
}
