//! Sibyl's device card — the harmoniser.
//!
//! Ids, ranges and defaults come from [`crate::params::sibyl`] — the one
//! table this widget, `Node::Sibyl`'s core and the app's edit routing all
//! read.
//!
//! # The wheel
//!
//! The hero is the chromatic circle, drawn as a TWELVE-SIDED POLYGON
//! rather than a curve: twelve pitch classes, twelve straight edges,
//! twelve corners you can point at. A circle would have been prettier and
//! would have hidden the only thing the shape is for, which is that pitch
//! class is a discrete, cyclic quantity with exactly twelve places to
//! stand.
//!
//! On it:
//!
//! - the **tonic** is squared off, so the key is findable without reading;
//! - **in-scale degrees** are filled nodes and the rest are hairline dots,
//!   so the scale is a shape rather than a word;
//! - the **note being heard** is a lit node with a spoke to the centre;
//! - each **harmony** is a lit node, and a chord is struck from the heard
//!   note to it.
//!
//! Those chords are the device's entire answer drawn as a figure: change
//! the key and they swing, change the scale and they change length, sing
//! a different note and they rotate. It reads as a sigil because a scale
//! IS one — a fixed set of relations on a wheel — and the diagram people
//! have drawn for that since antiquity happens to be the correct one.
//!
//! When nothing is being heard the spokes go out and the wheel dims. A
//! harmoniser that has lost the pitch must say so; the alternative is a
//! device that appears to be working while the voices have stopped.
//!
//! # Colour
//!
//! `role_time` for the heard note — it is the thing that is happening —
//! and `role_mod` for the voices, because they are what this device
//! writes over it. The same pair `flint` uses, meaning the same thing.

use crate::params::sibyl as sp;
use crate::params::{self};
use crate::theory::Scale;
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The cells, in the order they read: what happens to the voice, then
/// the voices that answer it, then the key they answer in, then level.
const CELLS: [u32; 9] = [
    sp::SHIFT,
    sp::FORMANT,
    sp::VOICE_A,
    sp::VOICE_B,
    sp::KEY,
    sp::SCALE,
    sp::BLEND,
    sp::MIX,
    sp::OUT,
];

/// One row of cells, and a labelled cell is two `POLY_CELL_H` units.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;

/// Knob positions of one harmoniser, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SibylUi {
    pub shift: f32,
    pub formant: f32,
    pub voice_a: f32,
    pub voice_b: f32,
    pub key: f32,
    pub scale: f32,
    pub blend: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for SibylUi {
    fn default() -> Self {
        let at = |id: u32| sibyl_norm(id, params::def(sp::TABLE, id).default);
        Self {
            shift: at(sp::SHIFT),
            formant: at(sp::FORMANT),
            voice_a: at(sp::VOICE_A),
            voice_b: at(sp::VOICE_B),
            key: at(sp::KEY),
            scale: at(sp::SCALE),
            blend: at(sp::BLEND),
            mix: at(sp::MIX),
            out: at(sp::OUT),
        }
    }
}

impl SibylUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| sibyl_norm(id, get(id));
        Self {
            shift: at(sp::SHIFT),
            formant: at(sp::FORMANT),
            voice_a: at(sp::VOICE_A),
            voice_b: at(sp::VOICE_B),
            key: at(sp::KEY),
            scale: at(sp::SCALE),
            blend: at(sp::BLEND),
            mix: at(sp::MIX),
            out: at(sp::OUT),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            sp::SHIFT => &mut self.shift,
            sp::FORMANT => &mut self.formant,
            sp::VOICE_A => &mut self.voice_a,
            sp::VOICE_B => &mut self.voice_b,
            sp::KEY => &mut self.key,
            sp::SCALE => &mut self.scale,
            sp::BLEND => &mut self.blend,
            sp::MIX => &mut self.mix,
            sp::OUT => &mut self.out,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }
}

/// One control by wire id — the single place an id becomes a [`Param`].
fn param_of(id: u32) -> Param {
    let def = params::def(sp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        // The two enumerations. Their VALUES carry the meaning, since a
        // cell prints only the position you are on.
        sp::KEY => with(Param::choice("key", sp::KEY_NAMES)),
        sp::SCALE => with(Param::choice("scale", sp::SCALE_NAMES)),
        // BIPOLAR: zero is the middle and means "leave this alone", which
        // for the two voices means OFF and has to look like the centre.
        sp::SHIFT => with(
            Param::new(
                "shift",
                crate::ui::device::Mapping::Linear {
                    min: def.min,
                    max: def.max,
                },
                crate::ui::device::Unit::Semitones,
            )
            .bipolar(),
        ),
        sp::FORMANT => with(
            Param::new(
                "formant",
                crate::ui::device::Mapping::Linear {
                    min: def.min,
                    max: def.max,
                },
                crate::ui::device::Unit::Semitones,
            )
            .bipolar(),
        ),
        // The voices step in whole SCALE DEGREES, so they are a choice
        // list rather than a sweep — there is no such thing as two and a
        // half degrees, and a continuous control would imply there was.
        sp::VOICE_A => with(Param::choice("voice a", VOICE_NAMES)),
        sp::VOICE_B => with(Param::choice("voice b", VOICE_NAMES)),
        sp::BLEND => with(Param::percent("blend")),
        sp::MIX => with(Param::percent("mix")),
        _ => with(Param::db("out", sp::OUT_MIN_DB, sp::OUT_MAX_DB)),
    }
}

/// The fifteen positions a voice can take, named as they are counted.
/// `--` is off, and it sits in the middle where zero belongs.
pub const VOICE_NAMES: &[&str] = &[
    "-7", "-6", "-5", "-4", "-3", "-2", "-1", "--", "+1", "+2", "+3", "+4", "+5", "+6", "+7",
];

/// What the WIDGET shows for an engine value — the mapping this card owns.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        sp::BLEND | sp::MIX => value * 100.0,
        sp::OUT => crate::dsp::arith::gain_to_db(value.max(1e-6)),
        // A voice's engine value is a signed step count; its widget value
        // is an index into [`VOICE_NAMES`], which puts zero in the middle.
        sp::VOICE_A | sp::VOICE_B => value + sp::VOICE_MAX_STEPS,
        _ => value,
    }
}

/// The inverse of [`shown`], clamped through the row.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        sp::BLEND | sp::MIX => value / 100.0,
        sp::OUT => crate::dsp::arith::db_to_gain(value),
        sp::VOICE_A | sp::VOICE_B => value - sp::VOICE_MAX_STEPS,
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized position, by param id.
pub fn sibyl_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`sibyl_value`], for a state stored in engine units.
pub fn sibyl_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Which rows snap to whole choices: the key, the scale, and the two
/// voices — a harmony is a whole number of degrees or it is nothing.
pub fn sibyl_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// Every parameter as an edit, whether or not it moved.
pub fn sibyl_edits(state: &SibylUi) -> Vec<ParamEdit> {
    let mut state = *state;
    sp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: sibyl_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its nine cells at their narrowest.
pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// The state the wheel draws: which pitch classes are lit and why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// The tonic's pitch class.
    pub key: i16,
    pub scale: Scale,
    /// The note coming IN, in MIDI, if any. What the detector followed.
    pub heard: Option<f32>,
    /// That note after SHIFT — the main voice, and what the wheel marks.
    /// Kept apart from `heard` because they are different facts: the
    /// card said "HEARD E3" while E4 was being sung into it, which is
    /// the device reporting its own output as its input.
    pub main: Option<f32>,
    /// Where each voice landed, in MIDI, if it is on and there is a note.
    pub voices: [Option<f32>; 2],
}

/// Work out everything the wheel needs, once, from the knobs and the
/// engine's last reading.
///
/// A pure function so the diagram can be tested without a pointer or a
/// frame — and so the card and the engine cannot disagree about where a
/// harmony went, since both call
/// [`scale_interval`](crate::theory::scale_interval).
pub fn reading(state: &SibylUi, heard: Option<f32>) -> Reading {
    let key = (sibyl_value(sp::KEY, state.key).round() as i16).rem_euclid(12);
    let index =
        (sibyl_value(sp::SCALE, state.scale).round().max(0.0) as usize).min(Scale::ALL.len() - 1);
    let scale = Scale::ALL.get(index).copied().unwrap_or(Scale::Major);
    let shift = sibyl_value(sp::SHIFT, state.shift);
    let main = heard.map(|m| m + shift);
    let mut voices = [None, None];
    for (slot, id) in voices.iter_mut().zip([sp::VOICE_A, sp::VOICE_B]) {
        let steps = sibyl_value(
            id,
            if id == sp::VOICE_A {
                state.voice_a
            } else {
                state.voice_b
            },
        )
        .round() as i32;
        if steps == 0 {
            continue;
        }
        let Some(note) = heard else { continue };
        let interval = crate::theory::scale_interval(note, key, scale, steps);
        *slot = Some(note + shift + interval);
    }
    Reading {
        key,
        scale,
        heard,
        main,
        voices,
    }
}

/// A pitch class's corner on the wheel. Twelve o'clock is the TONIC, not
/// C — the diagram is about the key, and a wheel that always put C at the
/// top would make every key look different for no reason.
fn corner(centre: egui::Pos2, radius: f32, key: i16, pitch_class: i16) -> egui::Pos2 {
    let step = (pitch_class - key).rem_euclid(12) as f32;
    let angle = step / 12.0 * core::f32::consts::TAU - core::f32::consts::FRAC_PI_2;
    egui::pos2(
        centre.x + radius * angle.cos(),
        centre.y + radius * angle.sin(),
    )
}

/// Draw the harmoniser's card. Returns the edits the user made.
///
/// `heard` is the engine's live pitch reading in MIDI — the CALLER's,
/// like every other piece of state with no knob attached.
pub fn sibyl_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut SibylUi,
    heard: Option<f32>,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "sibyl", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => wheel(ui, theme, state, heard),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: the chromatic wheel, and the readings beside it.
fn wheel(ui: &mut egui::Ui, theme: &Theme, state: &SibylUi, heard: Option<f32>) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 24.0 || rect.height() < 24.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);
    let r = reading(state, heard);

    // The wheel takes a square on the left; the readings take the rest.
    let side = (rect.height() - pad * 2.0).min(rect.width() * 0.5);
    let centre = egui::pos2(rect.left() + pad + side * 0.5, rect.center().y);
    let radius = side * 0.42;
    if radius < 8.0 {
        return;
    }

    // ---- the twelve-sided frame -------------------------------------
    let live = r.main.is_some();
    let frame_ink = if live { theme.outline } else { theme.divider };
    let ring: Vec<egui::Pos2> = (0..12)
        .map(|pc| corner(centre, radius, r.key, r.key + pc))
        .collect();
    for i in 0..12 {
        let a = ring.get(i).copied().unwrap_or(centre);
        let b = ring.get((i + 1) % 12).copied().unwrap_or(centre);
        painter.line_segment([a, b], egui::Stroke::new(1.0, frame_ink));
    }

    // ---- the scale, as a shape ---------------------------------------
    let degrees = r.scale.degrees();
    for pc in 0..12i16 {
        let at = corner(centre, radius, r.key, r.key + pc);
        let in_scale = degrees.contains(&pc);
        if pc == 0 {
            // The tonic: a square, so the key is findable at a glance
            // without reading the cell.
            let s = 3.0;
            painter.rect_stroke(
                egui::Rect::from_center_size(at, egui::vec2(s * 2.0, s * 2.0)),
                0.0,
                egui::Stroke::new(1.2, theme.role_shape),
                egui::StrokeKind::Middle,
            );
        } else if in_scale {
            painter.circle_filled(at, 2.2, theme.text_muted);
        } else {
            painter.circle_filled(at, 1.0, theme.divider);
        }
    }

    // ---- what is sounding, and what answers it ------------------------
    if let Some(note) = r.main {
        let pc = (note.round() as i16).rem_euclid(12);
        let at = corner(centre, radius, r.key, pc);
        painter.line_segment([centre, at], egui::Stroke::new(1.0, theme.role_time));
        painter.circle_filled(at, 3.4, theme.role_time);

        for voice in r.voices.iter().flatten() {
            let vpc = (voice.round() as i16).rem_euclid(12);
            let to = corner(centre, radius, r.key, vpc);
            // The CHORD, struck from the note to its answer. This is the
            // figure: the shape it makes is the interval.
            painter.line_segment([at, to], egui::Stroke::new(1.2, theme.role_mod));
            painter.circle_filled(to, 3.0, theme.role_mod);
        }
    } else {
        // Nothing heard: say so in the middle of the wheel rather than
        // leaving a diagram that looks like it is working.
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            "--",
            mini.clone(),
            theme.text_muted,
        );
    }

    // ---- the readings -------------------------------------------------
    let mut y = rect.top() + pad;
    let left = centre.x + radius + pad * 2.0;
    let line = font::MICRO_LABEL + 3.0;
    let mut row = |label: &str, value: String, ink: egui::Color32| {
        painter.text(
            egui::pos2(left, y),
            egui::Align2::LEFT_TOP,
            format!("{label:<9}{value}"),
            mini.clone(),
            ink,
        );
        y += line;
    };
    row(
        "KEY",
        format!("{} {}", key_name(r.key), r.scale.label()),
        theme.text_muted,
    );
    // What came IN, and what the shift made of it. Two facts, and the
    // arrow only appears when they differ.
    let heard_text = match (r.heard, r.main) {
        (Some(h), Some(m)) if (m - h).abs() > 0.01 => {
            format!("{} > {}", note_name(h), note_name(m))
        }
        (Some(h), _) => note_name(h),
        _ => "--".into(),
    };
    row(
        "HEARD",
        heard_text,
        if live {
            theme.role_time
        } else {
            theme.text_muted
        },
    );
    for (i, voice) in r.voices.iter().enumerate() {
        let label = if i == 0 { "VOICE A" } else { "VOICE B" };
        let text = match (voice, r.main) {
            (Some(v), Some(h)) => format!("{}  {:+.0} st", note_name(*v), v - h),
            _ => "--".into(),
        };
        row(
            label,
            text,
            if voice.is_some() {
                theme.role_mod
            } else {
                theme.text_muted
            },
        );
    }
}

/// A pitch class's name.
fn key_name(pc: i16) -> &'static str {
    sp::KEY_NAMES
        .get(pc.rem_euclid(12) as usize)
        .copied()
        .unwrap_or("C")
}

/// A MIDI note as a name and octave, with the cents it is off by — a
/// singer is not on the grid and the card should not pretend otherwise.
fn note_name(midi: f32) -> String {
    if !midi.is_finite() {
        return "--".into();
    }
    let rounded = midi.round();
    let cents = ((midi - rounded) * 100.0).round() as i32;
    let pc = (rounded as i16).rem_euclid(12);
    let octave = (rounded as i16).div_euclid(12) - 1;
    if cents == 0 {
        format!("{}{octave}", key_name(pc))
    } else {
        format!("{}{octave} {cents:+}c", key_name(pc))
    }
}

/// One row of cells, sharing the width progressively.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut SibylUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);

    ui.horizontal(|ui| {
        for (drawn, id) in CELLS.iter().enumerate() {
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let p = param_of(*id);
            let Some(slot) = state.slot(*id) else {
                continue;
            };
            let before = *slot;
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: sibyl_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MAX_W: f32 = 640.0;
    const MIN_W: f32 = 300.0;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            },
        );
        run.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = sibyl_edits(&SibylUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = sp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = sibyl_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (sibyl_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} does not reach its floor",
                def.name
            );
            assert!(
                (sibyl_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} does not reach its ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in sp::TABLE {
            for i in 0..=40 {
                let value = sibyl_value(def.id, i as f32 / 40.0);
                let again = sibyl_value(def.id, sibyl_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-3,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in sp::TABLE {
            let mut state = SibylUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = sibyl_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// The two voices step in whole degrees, and their OFF position is
    /// the middle of the list — a harmony that could be turned off only
    /// at one end of its travel would be a harmony nobody turns off.
    #[test]
    fn a_voice_steps_in_whole_degrees_and_switches_off_in_the_middle() {
        for id in [sp::VOICE_A, sp::VOICE_B] {
            assert!(sibyl_is_discrete(id), "a voice must snap to whole steps");
            let p = param_of(id);
            assert_eq!(p.choices(), Some(VOICE_NAMES.len() as u32));
            // Every position is a whole number of steps.
            for i in 0..VOICE_NAMES.len() {
                let norm = i as f32 / (VOICE_NAMES.len() - 1) as f32;
                let value = sibyl_value(id, norm);
                assert_eq!(value, value.round(), "position {i} gave {value}");
            }
            // The middle is off.
            let middle = sibyl_value(id, 0.5);
            assert_eq!(middle, 0.0, "the centre of a voice must be OFF");
        }
        // The key and the scale snap too; nothing else does.
        assert!(sibyl_is_discrete(sp::KEY));
        assert!(sibyl_is_discrete(sp::SCALE));
        for id in [sp::SHIFT, sp::FORMANT, sp::BLEND, sp::MIX, sp::OUT] {
            assert!(!sibyl_is_discrete(id), "{id} should sweep");
        }
    }

    /// THE DIAGRAM TELLS THE TRUTH: what the wheel marks is where the
    /// engine actually put the voices, because both ask
    /// [`crate::theory::scale_interval`] rather than each having a view.
    #[test]
    fn the_wheel_marks_where_the_voices_really_go() {
        let mut state = SibylUi::default();
        state.key = sibyl_norm(sp::KEY, 0.0); // C
        state.scale = sibyl_norm(sp::SCALE, 0.0); // major
        state.voice_a = sibyl_norm(sp::VOICE_A, 2.0);
        state.voice_b = sibyl_norm(sp::VOICE_B, 4.0);

        // Middle C, a third and a fifth above: E and G.
        let r = reading(&state, Some(60.0));
        assert_eq!(r.heard, Some(60.0));
        assert_eq!(r.main, Some(60.0));
        assert_eq!(r.voices[0], Some(64.0), "a third above C is E");
        assert_eq!(r.voices[1], Some(67.0), "a fifth above C is G");

        // D: the same DEGREES, different semitones — F and A.
        let r = reading(&state, Some(62.0));
        assert_eq!(r.voices[0], Some(65.0), "a third above D is F");
        assert_eq!(r.voices[1], Some(69.0), "a fifth above D is A");

        // With no note heard there is nothing to answer.
        let r = reading(&state, None);
        assert_eq!(r.heard, None);
        assert_eq!(
            r.voices,
            [None, None],
            "voices sounded with nothing to follow"
        );

        // SHIFT moves the main voice and carries the harmonies with it.
        state.shift = sibyl_norm(sp::SHIFT, -12.0);
        let r = reading(&state, Some(60.0));
        assert_eq!(r.heard, Some(60.0), "HEARD is the input, not the output");
        assert_eq!(r.main, Some(48.0));
        assert_eq!(r.voices[0], Some(52.0), "the third moved with the shift");
    }

    /// A voice at zero is OFF, at every key and scale.
    #[test]
    fn a_voice_at_zero_stays_silent() {
        let mut state = SibylUi::default();
        state.voice_a = sibyl_norm(sp::VOICE_A, 0.0);
        state.voice_b = sibyl_norm(sp::VOICE_B, 0.0);
        for key in 0..12 {
            state.key = sibyl_norm(sp::KEY, key as f32);
            let r = reading(&state, Some(64.0));
            assert_eq!(r.voices, [None, None], "key {key}");
        }
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SibylUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| sibyl_card(ui, &theme, &mut state, Some(69.0)))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SibylUi::default();
        let floor = frame(&ctx, |ui| face_width(ui, &theme)) + theme.sp(space::SM) * 2.0;
        for panel_w in [floor.ceil(), floor * 1.3, floor * 1.8] {
            let host = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(panel_w, theme.sp(control::DEVICE_TALL_H) + 8.0),
            );
            let used = frame(&ctx, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(host)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(host.width());
                sibyl_card(&mut child, &theme, &mut state, Some(69.0));
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt wide",
                used.width()
            );
            assert!(
                used.width() > panel_w * 0.7,
                "at a {panel_w:.0} pt panel the card drew only {:.0} pt",
                used.width()
            );
        }
    }

    /// The footer reserves two lines for its row of cells, and the wheel
    /// keeps enough height to be a diagram.
    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(footer >= cell * CELL_UNITS as f32);
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(plot > cell * 3.0, "the wheel is left only {plot} pt");
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = SibylUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| sibyl_card(ui, &theme, &mut state, Some(69.0)))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// POINTER: every cell is reachable, and a drag edits exactly one.
    /// Geometry-independent, for the reason `flint`'s twin gives.
    #[test]
    fn every_cell_is_reachable_and_a_drag_moves_only_one() {
        use crate::ui::device::probe;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let width = frame(&ctx, |ui| face_width(ui, &theme)).ceil() * 1.2;
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, theme.sp(control::DEVICE_TALL_H)),
        );
        let cell_h = theme.sp(crate::ui::tokens::control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        for step in 0..64 {
            // BOTH directions at each position. One is not enough and
            // cannot be: MIX opens at 100 % so it cannot rise, and KEY
            // opens at C, the bottom of its list, so it cannot fall. A
            // sweep in a single direction reports whichever of those it
            // happened not to be testing as unreachable.
            for dy in [20.0f32, -20.0] {
                let mut state = SibylUi::default();
                let x = rect.left() + rect.width() * (step as f32 + 0.5) / 64.0;
                let y = rect.bottom() - cell_h * 0.75;
                let from = egui::pos2(x, y);
                let path = probe::drag_path(from, from + egui::vec2(0.0, dy), 4);
                let edits = probe::run(&ctx, rect, &path, |ui| {
                    sibyl_card(ui, &theme, &mut state, Some(69.0))
                });
                let touched: std::collections::BTreeSet<u32> =
                    edits.into_iter().flatten().map(|e| e.param).collect();
                assert!(touched.len() <= 1, "one drag at x={x:.0} moved {touched:?}");
                reached.extend(touched);
            }
        }
        let expect: std::collections::BTreeSet<u32> = CELLS.iter().copied().collect();
        assert_eq!(
            reached,
            expect,
            "a sweep across the footer never reached {:?}",
            expect.difference(&reached).collect::<Vec<_>>()
        );
    }
}
