//! The reverb device card — the app's first effect.
//!
//! Same shape as the synth card: normalized knob state here, natural values
//! out as [`ParamEdit`]s. Ids, ranges and defaults come from
//! [`crate::params::reverb`] — the one table the widget, `Node::Reverb::apply`
//! and the app's edit routing all read.

use crate::params::reverb::{DAMP, MIX, SIZE};
use crate::params::{self, reverb::TABLE};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Param, Well, Wells, card, knob};
use crate::ui::theme::Theme;
use eframe::egui;

/// Knob positions of one reverb, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
/// Names and defaults come from the table; the engine's 0..=1 shows as
/// percent, so a default is the table's times 100.
fn spec() -> Spec {
    let pct = |id: u32| {
        let def = params::def(TABLE, id);
        Param::percent(def.name).with_default(def.default * 100.0)
    };
    Spec {
        size: pct(SIZE),
        damp: pct(DAMP),
        mix: pct(MIX),
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
            param: MIX,
            value: natural(state.mix),
        },
        ParamEdit {
            param: SIZE,
            value: natural(state.size),
        },
        ParamEdit {
            param: DAMP,
            value: natural(state.damp),
        },
    ]
}

/// Draw the reverb card. Returns the edits the user just made.
pub fn reverb_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ReverbUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    card::card(ui, theme, "reverb", |ui| {
        // One well divided in THREE — the reverb's parameters are peers
        // and belong to one idea (the room), so they share a well rather
        // than standing in three separate ones. `each` sizes a division
        // from the widest of the three knobs, so all three are equal and
        // every label and readout still fits.
        let layout = Wells::new().row([Well::divided(3, 1)
            .each(
                knob::footprint(ui, theme, &s.size)
                    .union(knob::footprint(ui, theme, &s.damp))
                    .union(knob::footprint(ui, theme, &s.mix)),
                theme,
            )
            .titled("room", ui, theme)]);
        card::wells(ui, theme, &layout, |ui, i| {
            let (param, value, id) = match i {
                0 => (&s.size, &mut state.size, SIZE),
                1 => (&s.damp, &mut state.damp, DAMP),
                _ => (&s.mix, &mut state.mix, MIX),
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
    fn every_table_knob_leaves_as_an_edit() {
        // The widget must cover the table exactly: an id the table knows
        // but the card never emits is a knob that silently stopped working.
        let edits = reverb_edits(&ReverbUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
    }

    #[test]
    fn defaults_are_a_usable_room_not_a_wash() {
        let ui = ReverbUi::default();
        let edits = reverb_edits(&ui);
        assert_eq!(edits.len(), 3);
        // Every value the engine receives is inside its documented range.
        assert!(edits.iter().all(|e| (0.0..=1.0).contains(&e.value)));
        // Loading a reverb must never drown the track it lands on.
        let mix = edits.iter().find(|e| e.param == MIX).map(|e| e.value);
        assert!(mix.is_some_and(|m| m > 0.0 && m < 0.5), "mix {mix:?}");
    }
}
