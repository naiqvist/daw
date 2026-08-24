//! The reverb device card — the app's first effect.
//!
//! Same shape as the synth card: normalized knob state here, natural values
//! out as [`ParamEdit`]s, and the param ids kept in lockstep with
//! `Node::Reverb::apply` in `src/audio/graph.rs`:
//!
//! - `0` — mix, `0..=1` wet against dry
//! - `1` — size, `0..=1` decay time
//! - `2` — damp, `0..=1` how fast the tail loses its highs

use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Param, card, knob};
use crate::ui::theme::Theme;
use eframe::egui;

const P_MIX: u32 = 0;
const P_SIZE: u32 = 1;
const P_DAMP: u32 = 2;

/// Knob positions of one reverb, normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReverbUi {
    pub size: f32,
    pub damp: f32,
    pub mix: f32,
}

impl Default for ReverbUi {
    fn default() -> Self {
        let s = spec();
        Self {
            size: s.size.default_norm,
            damp: s.damp.default_norm,
            mix: s.mix.default_norm,
        }
    }
}

struct Spec {
    size: Param,
    damp: Param,
    mix: Param,
}

/// All three are plain 0-100% controls: the engine's ranges are already
/// normalized, so a percentage is the honest unit rather than a fake one.
fn spec() -> Spec {
    Spec {
        // A medium room, audible but not a cathedral, is the useful
        // starting point for the first effect anyone loads.
        size: Param::percent("size").with_default(60.0),
        damp: Param::percent("damp").with_default(40.0),
        // Default 25% wet: enough to hear it worked, little enough that
        // loading a reverb never destroys a mix.
        mix: Param::percent("mix").with_default(25.0),
    }
}

/// Natural (engine-facing) value of a normalized knob: the engine takes
/// `0..=1`, the UI shows percent.
fn natural(norm: f32) -> f32 {
    norm.clamp(0.0, 1.0)
}

/// Every parameter as an edit, whether or not it moved — for a reset, a
/// preset recall, or the moment the device is first loaded.
pub fn reverb_edits(state: &ReverbUi) -> Vec<ParamEdit> {
    vec![
        ParamEdit {
            param: P_MIX,
            value: natural(state.mix),
        },
        ParamEdit {
            param: P_SIZE,
            value: natural(state.size),
        },
        ParamEdit {
            param: P_DAMP,
            value: natural(state.damp),
        },
    ]
}

/// Draw the reverb card. Returns the edits the user just made.
pub fn reverb_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ReverbUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    card::card(ui, theme, "reverb", |ui| {
        card::sections(ui, theme, 3, 1, |ui, i| {
            let (param, value, id) = match i {
                0 => (&s.size, &mut state.size, P_SIZE),
                1 => (&s.damp, &mut state.damp, P_DAMP),
                _ => (&s.mix, &mut state.mix, P_MIX),
            };
            if knob::knob(ui, theme, param, value) {
                edits.push(ParamEdit {
                    param: id,
                    value: natural(*value),
                });
            }
        });
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_ids_match_the_engine() {
        // The wire contract with Node::Reverb::apply. Renumbering these is
        // a protocol break, not a refactor.
        assert_eq!(P_MIX, 0);
        assert_eq!(P_SIZE, 1);
        assert_eq!(P_DAMP, 2);
    }

    #[test]
    fn defaults_are_a_usable_room_not_a_wash() {
        let ui = ReverbUi::default();
        let edits = reverb_edits(&ui);
        assert_eq!(edits.len(), 3);
        // Every value the engine receives is inside its documented range.
        assert!(edits.iter().all(|e| (0.0..=1.0).contains(&e.value)));
        // Loading a reverb must never drown the track it lands on.
        let mix = edits.iter().find(|e| e.param == P_MIX).map(|e| e.value);
        assert!(mix.is_some_and(|m| m > 0.0 && m < 0.5), "mix {mix:?}");
    }
}
