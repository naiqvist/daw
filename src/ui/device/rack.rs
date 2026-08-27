//! The rack — a card that holds other cards, with macros across its face.
//!
//! Modelled on Ableton's Instrument and Audio Effect Racks, which is the
//! design everybody already knows: a frame around a chain of devices,
//! with a bank of macro knobs on the left that reach into it.
//!
//! # This is the UI half only
//!
//! A rack that CONTAINS devices needs the project format to nest, and
//! `DeviceState` is `Copy` today — nesting a `Vec` inside it would ripple
//! through every `match instance.state` in the app. So the containment is
//! deliberately not decided here: [`rack_card`] takes a CLOSURE that
//! draws the chain, and the caller decides what a chain is. The gallery
//! passes two real device cards; the app will pass whatever the model
//! turns out to be.
//!
//! That ordering is on purpose. The layout and the mapping gesture are
//! the parts worth being sure about before the invasive change, and they
//! can be sure without it.
//!
//! # The layout, from Ableton
//!
//! ```text
//! ┌ rack ───────────────────────────────────────────────┐
//! │ ┌ macros ────┐ ┌ chain ─────────────────────────┐   │
//! │ │ ◯ 1   ◯ 2  │ │ [ device card ] [ device card ]│   │
//! │ │ ◯ 3   ◯ 4  │ │                                │   │
//! │ │ ◯ 5   ◯ 6  │ │                                │   │
//! │ │ ◯ 7   ◯ 8  │ │                                │   │
//! │ └────────────┘ └────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! Two columns of four, macros at the left, chain to the right of them —
//! and the chain scrolls horizontally, because a rack's whole purpose is
//! to hold more devices than fit.
//!
//! # KNOBS here, and cells everywhere else
//!
//! Every small card in this rack uses `labeled_cell_bar` rather than a
//! dial, and `sat.rs` argues why: a cell spends its height on what a
//! control IS and what it is SET to, and hands the rest to the picture.
//!
//! A macro is the one control that argument does not cover. It has no
//! unit — it is a number between nothing and everything, pointed at
//! something else — so there is no reading for a cell to show. What it
//! has instead is a POSITION, which is what a dial draws and a number
//! does not. It is also the control Ableton made a dial, and a rack that
//! looked nothing like the rack everyone knows would be a worse rack.
//!
//! # Mapping: the last thing you touched
//!
//! Press a macro's `map`, then move any control in the chain, and the two
//! are joined. No modal browser and no drag: the gesture is "this knob,
//! that one", in the order a person thinks of them.
//!
//! It costs nothing to implement, and that is the interesting part. Every
//! card in this tree already returns the edits it just made — the channel
//! the app uses to send letters to the engine — so "the parameter the
//! user last touched" was already being reported. The rack only has to
//! read it while armed.

use crate::ui::device::{Param, Unit, card, knob};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use crate::ui::{device::Mapping, kit};
use eframe::egui;

/// How many macros a rack carries. Eight, as Ableton's did for twenty
/// years — enough to cover a patch, few enough to stay a row of gestures
/// rather than a control surface of its own.
pub const MACRO_COUNT: usize = 8;

/// The macro grid: two columns of four.
const MACRO_COLUMNS: usize = 2;

/// A parameter somewhere inside the chain.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MacroTarget {
    /// The `DeviceInstance` id the parameter belongs to.
    pub device: u64,
    /// Its wire id on that device.
    pub param: u32,
    /// What to print on the macro's face. Carried rather than looked up,
    /// because the rack does not know what kinds of device it holds — and
    /// should not have to, for a caption.
    pub label: String,
}

/// One macro knob.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MacroKnob {
    /// Where the knob is, `0..=1`.
    pub value: f32,
    /// What it drives, once something has been pointed at.
    pub target: Option<MacroTarget>,
}

/// A rack's UI state.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RackUi {
    pub name: String,
    pub macros: Vec<MacroKnob>,
    /// Which macro is waiting to be pointed at something.
    ///
    /// UI state with no engine counterpart, and short-lived — it rides
    /// the instance the way `page` does rather than living in a card,
    /// because a card is rebuilt every frame and would forget it between
    /// the press and the parameter.
    pub arming: Option<usize>,
}

impl Default for RackUi {
    fn default() -> Self {
        Self {
            name: "rack".to_owned(),
            macros: vec![MacroKnob::default(); MACRO_COUNT],
            arming: None,
        }
    }
}

impl RackUi {
    /// The macro at `index`, if the state has one.
    pub fn get(&self, index: usize) -> Option<&MacroKnob> {
        self.macros.get(index)
    }

    /// Point a macro at a parameter. Replaces whatever it had.
    pub fn assign(&mut self, index: usize, target: MacroTarget) {
        if let Some(slot) = self.macros.get_mut(index) {
            slot.target = Some(target);
        }
    }

    /// Forget a macro's assignment, keeping its position.
    pub fn clear(&mut self, index: usize) {
        if let Some(slot) = self.macros.get_mut(index) {
            slot.target = None;
        }
    }
}

/// A parameter the user moved inside the chain, as the chain reports it.
///
/// The rack reads these ONLY to complete a mapping. Applying them is the
/// caller's business, exactly as it was before a rack existed.
#[derive(Debug, Clone, PartialEq)]
pub struct Touched {
    pub device: u64,
    pub param: u32,
    pub label: String,
}

/// One macro that moved, and what it points at.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroMove {
    pub index: usize,
    /// The knob's new position, `0..=1`.
    pub norm: f32,
    pub target: MacroTarget,
}

/// What a frame of the rack produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RackOutcome {
    /// Macros that moved AND have somewhere to send it. A macro with no
    /// target still turns — see [`macro_cell`] — it simply has nothing to
    /// report.
    pub moves: Vec<MacroMove>,
    /// A mapping completed this frame: which macro, and what it caught.
    pub mapped: Option<(usize, MacroTarget)>,
}

/// The rack's own height: whichever of its two halves is taller.
///
/// Declared rather than taken, for the reason
/// `notes/20260827-device-card-layout.md` rule 3 gives about width — a
/// container that accepts whatever is going crams whatever it holds. The
/// first version of this used the child card's height alone and clipped
/// its own last two macros, which is the same mistake from the other
/// side: the macro column is a thing the rack holds too.
pub fn height(ui: &egui::Ui, theme: &Theme) -> f32 {
    let chrome = theme.sp(space::LG) * 2.0;
    let child = theme.sp(control::DEVICE_TALL_H);
    let rows = (MACRO_COUNT / MACRO_COLUMNS) as f32;
    let macros =
        macro_row_height(ui, theme) * rows + ui.spacing().item_spacing.y * (rows - 1.0).max(0.0);
    child.max(macros) + chrome
}

/// One macro row: the dial with its own label and value, plus the caption
/// line under it.
fn macro_row_height(ui: &egui::Ui, theme: &Theme) -> f32 {
    knob::footprint(ui, theme, &macro_param(0)).size.y
        + theme.sp(font::MICRO_LABEL)
        + ui.spacing().item_spacing.y
}

/// The macro column's width: two knobs and the gap between them.
fn macros_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    let one = knob::footprint(ui, theme, &macro_param(0)).width();
    one * MACRO_COLUMNS as f32
        + ui.spacing().item_spacing.x * (MACRO_COLUMNS - 1) as f32
        + theme.sp(space::SM) * 2.0
}

/// How a macro describes itself to the knob widget.
///
/// Percent, because that is the only honest unit for a control whose
/// meaning is borrowed: a macro pointed at a cutoff is not measured in
/// hertz, it is measured in how far it has been turned.
fn macro_param(index: usize) -> Param {
    const NAMES: [&str; MACRO_COUNT] = ["1", "2", "3", "4", "5", "6", "7", "8"];
    Param::new(
        NAMES[index.min(MACRO_COUNT - 1)],
        Mapping::Linear {
            min: 0.0,
            max: 100.0,
        },
        Unit::Percent,
    )
}

/// One macro: the dial, what it drives, and the button that points it.
fn macro_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut RackUi,
    index: usize,
    out: &mut RackOutcome,
) {
    let param = macro_param(index);
    let armed = state.arming == Some(index);
    let Some(knob_state) = state.macros.get_mut(index) else {
        return;
    };
    let mut norm = knob_state.value.clamp(0.0, 1.0);
    let target = knob_state.target.clone();

    ui.vertical(|ui| {
        if knob::knob(ui, theme, &param, &mut norm) {
            if let Some(slot) = state.macros.get_mut(index) {
                slot.value = norm;
            }
            // A macro with nowhere to send it still TURNS. Disabling it
            // would make "arm, then turn" impossible, and the position is
            // worth keeping anyway — mapping it later should not also
            // require setting it again.
            if let Some(target) = target.clone() {
                out.moves.push(MacroMove {
                    index,
                    norm,
                    target,
                });
            }
        }

        // What it drives, under it — AND the button that points it.
        //
        // One line doing both, because two would not fit: four rows of
        // dial-plus-caption-plus-button is taller than the card the rack
        // is wrapped around, and the first version clipped its own last
        // two macros proving it. Making the caption clickable costs
        // nothing and reads better anyway: the thing that says what a
        // macro drives is the thing you press to change it.
        // An unmapped macro shows a DASH, not the word "unmapped": eight
        // of those is a column shouting about what it does not do. A dash
        // is quiet, and — unlike the near-invisible colour the first
        // version used — it is still something you can see to click.
        let caption = if armed {
            "…pick one".to_owned()
        } else {
            target
                .as_ref()
                .map(|t| t.label.clone())
                .unwrap_or_else(|| "—".to_owned())
        };
        let colour = if armed {
            theme.accent
        } else {
            theme.text_muted
        };
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(caption)
                        .size(theme.sp(font::MICRO_LABEL))
                        .color(colour),
                )
                .frame(false),
            )
            .on_hover_text(if armed {
                "move any control in the chain"
            } else {
                "click, then move any control in the chain"
            })
            .clicked()
        {
            state.arming = if armed { None } else { Some(index) };
        }
    });
}

/// Draw a rack around whatever `chain` draws.
///
/// `chain` returns the parameters the user moved inside it — the same
/// edits it was already reporting to the app. The rack reads them only
/// while a macro is armed.
pub fn rack_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut RackUi,
    chain: impl FnOnce(&mut egui::Ui) -> Vec<Touched>,
) -> RackOutcome {
    let mut out = RackOutcome::default();
    let name = state.name.clone();

    let tall = height(ui, theme);
    card::card_sized(ui, theme, &name, tall, |ui| {
        ui.horizontal_top(|ui| {
            // --- the macros ------------------------------------------
            let width = macros_width(ui, theme);
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    for row in 0..MACRO_COUNT / MACRO_COLUMNS {
                        ui.horizontal(|ui| {
                            for column in 0..MACRO_COLUMNS {
                                macro_cell(
                                    ui,
                                    theme,
                                    state,
                                    row * MACRO_COLUMNS + column,
                                    &mut out,
                                );
                            }
                        });
                    }
                },
            );

            // The seam. A rack is two regions and the eye should not have
            // to infer where one ends.
            let seam = ui.available_rect_before_wrap();
            ui.painter().line_segment(
                [seam.left_top(), seam.left_bottom()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
            kit::gap(ui, theme, space::SM);

            // --- the chain -------------------------------------------
            //
            // HORIZONTALLY scrolled, because a rack exists to hold more
            // than fits and the alternative is cards that shrink until
            // none of them can print their own numbers.
            let touched = egui::ScrollArea::horizontal()
                .id_salt("rack.chain")
                .show(ui, |ui| ui.horizontal_top(|ui| chain(ui)).inner)
                .inner;

            // --- the mapping -----------------------------------------
            //
            // The FIRST thing touched wins and disarms. Taking them all
            // would map a macro to whatever happened to be last in a
            // frame where two controls moved, which is a coin toss.
            if let Some(index) = state.arming
                && let Some(first) = touched.first()
            {
                let target = MacroTarget {
                    device: first.device,
                    param: first.param,
                    label: first.label.clone(),
                };
                state.assign(index, target.clone());
                state.arming = None;
                out.mapped = Some((index, target));
            }
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touched(device: u64, param: u32) -> Touched {
        Touched {
            device,
            param,
            label: format!("dev {device} p{param}"),
        }
    }

    #[test]
    fn a_fresh_rack_has_eight_unmapped_macros() {
        let rack = RackUi::default();
        assert_eq!(rack.macros.len(), MACRO_COUNT);
        assert!(rack.macros.iter().all(|m| m.target.is_none()));
        assert!(rack.arming.is_none());
    }

    /// Assigning replaces, clearing keeps the POSITION — mapping a macro
    /// somewhere else should not also reset where it was turned to.
    #[test]
    fn assign_replaces_and_clear_keeps_the_position() {
        let mut rack = RackUi::default();
        if let Some(m) = rack.macros.get_mut(2) {
            m.value = 0.75;
        }
        rack.assign(
            2,
            MacroTarget {
                device: 7,
                param: 3,
                label: "cutoff".to_owned(),
            },
        );
        assert_eq!(
            rack.get(2).and_then(|m| m.target.as_ref()).map(|t| t.param),
            Some(3)
        );
        rack.assign(
            2,
            MacroTarget {
                device: 9,
                param: 1,
                label: "drive".to_owned(),
            },
        );
        assert_eq!(
            rack.get(2)
                .and_then(|m| m.target.as_ref())
                .map(|t| t.device),
            Some(9),
            "assigning did not replace"
        );
        rack.clear(2);
        assert!(rack.get(2).and_then(|m| m.target.as_ref()).is_none());
        assert_eq!(
            rack.get(2).map(|m| m.value),
            Some(0.75),
            "clear moved the knob"
        );
    }

    /// An index past the last macro reaches nothing rather than panicking
    /// — a rack loaded from a file with fewer macros than this build has
    /// is a real case.
    #[test]
    fn an_index_past_the_last_macro_is_harmless() {
        let mut rack = RackUi::default();
        rack.assign(
            99,
            MacroTarget {
                device: 1,
                param: 1,
                label: "x".to_owned(),
            },
        );
        rack.clear(99);
        assert!(rack.get(99).is_none());
        assert!(rack.macros.iter().all(|m| m.target.is_none()));
    }

    /// THE MAPPING GESTURE, without a pointer: arm a macro, hand the rack
    /// a touched parameter, and the two are joined and the mode ends.
    #[test]
    fn arming_catches_the_first_parameter_touched() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut rack = RackUi {
            arming: Some(3),
            ..RackUi::default()
        };

        let out = frame(&ctx, |ui| {
            rack_card(ui, &theme, &mut rack, |_ui| {
                vec![touched(11, 5), touched(22, 6)]
            })
        });

        assert_eq!(
            rack.get(3)
                .and_then(|m| m.target.as_ref())
                .map(|t| t.device),
            Some(11),
            "the SECOND touch won, or none did"
        );
        assert!(rack.arming.is_none(), "the rack stayed armed");
        assert_eq!(out.mapped.map(|(i, t)| (i, t.device)), Some((3, 11)));
    }

    /// And with nothing armed, touching a control maps nothing. Otherwise
    /// every knob turn in a rack would rewrite a macro.
    #[test]
    fn touching_without_arming_maps_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut rack = RackUi::default();

        let out = frame(&ctx, |ui| {
            rack_card(ui, &theme, &mut rack, |_ui| vec![touched(11, 5)])
        });

        assert!(out.mapped.is_none());
        assert!(rack.macros.iter().all(|m| m.target.is_none()));
    }

    /// Drawing at rest emits nothing — one of the five standing card
    /// tests, and the one that stops a card writing its own defaults over
    /// a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut rack = RackUi::default();
        rack.assign(
            0,
            MacroTarget {
                device: 1,
                param: 2,
                label: "cutoff".to_owned(),
            },
        );
        let out = frame(&ctx, |ui| {
            rack_card(ui, &theme, &mut rack, |_ui| Vec::new())
        });
        assert!(out.moves.is_empty());
        assert!(out.mapped.is_none());
    }

    /// The rack declares a height that holds BOTH of its halves — the
    /// child card and its own macro column. The first version checked
    /// only the card and clipped two macros.
    #[test]
    fn the_rack_is_taller_than_either_half() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let (tall, rows) = frame(&ctx, |ui| {
            (height(ui, &theme), macro_row_height(ui, &theme))
        });
        assert!(
            tall > theme.sp(control::DEVICE_TALL_H),
            "a rack no taller than its child would clip it"
        );
        let column = rows * (MACRO_COUNT / MACRO_COLUMNS) as f32;
        assert!(
            tall > column,
            "the macro column needs {column} and the rack is {tall}"
        );
    }

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 600.0),
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
        out.expect("the frame did not run")
    }
}
