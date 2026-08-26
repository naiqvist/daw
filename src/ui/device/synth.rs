//! The sine synth device card — the UI face of the engine's built-in
//! sequencer synth, and the first real device.
//!
//! The device layer never imports the engine, so edits leave as plain
//! `(param id, natural value)` data and the app layer translates them to
//! engine letters. Ids, ranges and defaults come from [`crate::params::seq`]
//! — the one table this widget, `Node::Seq::apply` and the app's edit
//! routing all read.

use crate::params::seq::{ATTACK, GAIN, RELEASE};
use crate::params::{self, seq::TABLE};
use crate::ui::device::{Well, Wells, card, knob, param::Param};
use crate::ui::theme::Theme;
use eframe::egui;

/// One parameter edit leaving the UI: engine param id + NATURAL value
/// (the engine speaks Hz/ms/gain, not normalized positions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdit {
    pub param: u32,
    pub value: f32,
}

/// UI state of one sine synth device: normalized positions of its knobs.
/// Serialized into project files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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

/// The synth's parameter descriptions. Names, ranges and defaults are the
/// table's — the engine clamps against the same rows, so the knobs cannot
/// drift from what the engine accepts.
fn spec() -> Spec {
    let gain = params::def(TABLE, GAIN);
    let attack = params::def(TABLE, ATTACK);
    let release = params::def(TABLE, RELEASE);
    Spec {
        gain: Param::new(
            gain.name,
            crate::ui::device::Mapping::Linear {
                min: gain.min,
                max: gain.max,
            },
            crate::ui::device::Unit::Plain,
        )
        .with_default(gain.default),
        attack: Param::ms(attack.name, attack.min, attack.max).with_default(attack.default),
        release: Param::ms(release.name, release.min, release.max).with_default(release.default),
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
        // The card says what belongs with what. Attack and release are
        // one idea — the amplitude envelope — and gain is a different one,
        // so the envelope times share a well, subdivided into a sub-well
        // each, while gain stands alone in its own.
        //
        // Grouping by NESTING rather than by proximity means the grouping
        // survives: a gap can be read as "these two happen to be next to
        // each other", a shared well cannot.
        // Wells of DIFFERENT sizes, evenly distributed: the envelope well
        // takes two units to gain's one, and DIVIDES IN TWO — one
        // sub-well per time. The row still tiles the card exactly, and
        // each division reads as a sibling of the single-unit gain well
        // rather than as a different size of thing.
        //
        // Every size here comes from a contract. `each` says what one
        // division must hold, the well's own need becomes two of those
        // side by side, and the card's width is what falls out — so
        // "release" and "1.05 ms" have room reserved before either is
        // drawn, at either level of nesting.
        //
        // The divisions are LEAVES, so the closure just sees 0, 1, 2. The
        // nesting is layout; it is not something this code counts through.
        //
        // The tray is TITLED, because grouping the two times only says
        // "these belong together" — the caption is the other half of the
        // sentence, and it is what makes the grouping mean "envelope"
        // rather than merely "not gain".
        let envelope =
            knob::footprint(ui, theme, &s.attack).union(knob::footprint(ui, theme, &s.release));
        let layout = Wells::new().row([
            Well::one().fits(knob::footprint(ui, theme, &s.gain)),
            Well::divided(2, 1)
                .each(envelope, theme)
                .titled("envelope", ui, theme),
        ]);
        card::wells(ui, theme, &layout, |ui, i| {
            let (param, value, id) = match i {
                0 => (&s.gain, &mut state.gain, GAIN),
                1 => (&s.attack, &mut state.attack, ATTACK),
                _ => (&s.release, &mut state.release, RELEASE),
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

/// The natural value at a normalized knob position, by param id — the
/// mapping the card itself applies, reachable without drawing one.
pub fn sine_synth_value(param: u32, norm: f32) -> f32 {
    let s = spec();
    match param {
        ATTACK => s.attack.value(norm),
        RELEASE => s.release.value(norm),
        _ => s.gain.value(norm),
    }
}

/// The inverse: where a NATURAL value sits on the knob. A device's stored
/// state is engine units, so this is the door back to what the card draws,
/// and it is the same `Param` in both directions — the round trip cannot
/// drift because there is only one mapping.
pub fn sine_synth_norm(param: u32, value: f32) -> f32 {
    let s = spec();
    match param {
        ATTACK => s.attack.mapping.to_norm(value),
        RELEASE => s.release.mapping.to_norm(value),
        _ => s.gain.mapping.to_norm(value),
    }
}

/// Every parameter of `state` as an edit, whether or not it just changed.
/// What a caller needs after setting the knobs itself — a reset, a preset
/// recall, a project load — since the card only emits on user movement.
pub fn sine_synth_edits(state: &SineSynthUi) -> Vec<ParamEdit> {
    [
        (GAIN, state.gain),
        (ATTACK, state.attack),
        (RELEASE, state.release),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: sine_synth_value(param, norm),
    })
    .collect()
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
    fn every_table_knob_leaves_as_an_edit() {
        // The widget must cover the table exactly: an id the table knows
        // but the card never emits is a knob that silently stopped working.
        let edits = sine_synth_edits(&SineSynthUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
    }
}
