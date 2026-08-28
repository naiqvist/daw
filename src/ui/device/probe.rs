//! Headless pointer probing: drive a widget with a real gesture and see
//! what it did.
//!
//! Test-only, and it exists because of a class of bug the ordinary card
//! tests cannot see. `drawing_at_rest_emits_nothing` proves a card does
//! not move on its own; `every_table_row_leaves_as_an_edit` proves every
//! parameter is reachable. Neither says a word about what happens when a
//! pointer actually presses something — and every interaction bug this
//! codebase has shipped lived exactly there:
//!
//! - a drag that re-decided which handle it was on every frame, so it
//!   leaked onto a neighbour and died halfway;
//! - a control drawn over a display, where both took the same drag;
//! - state a card wrote during a gesture and threw away before the next
//!   frame could read it.
//!
//! All three are invisible to a test that calls a draw function once with
//! no input, and all three are obvious to one that presses and drags.
//!
//! # The three-frame rule
//!
//! egui establishes hover from the PREVIOUS frame's pointer position, so
//! a press delivered on the same frame the pointer first appears lands on
//! nothing at all. Every gesture here therefore begins with a bare move.
//! Get this wrong and the widget under test looks broken when it is fine,
//! or — much worse — looks fine because nothing was ever pressed.

use eframe::egui;

/// One frame's worth of pointer: where it is, and whether the button
/// changed state.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub at: egui::Pos2,
    /// `Some(true)` presses, `Some(false)` releases, `None` just moves.
    pub button: Option<bool>,
    /// What is held while this step happens.
    ///
    /// Modifiers change what a gesture MEANS — Alt copies instead of
    /// moving, Shift constrains, Ctrl+Alt bypasses snap — so a harness
    /// that can only send bare drags cannot test the half of the grammar
    /// that matters most. Defaults to nothing held, so every existing
    /// call site is unchanged.
    pub mods: egui::Modifiers,
}

impl Step {
    pub fn moved(at: egui::Pos2) -> Self {
        Self {
            at,
            button: None,
            mods: egui::Modifiers::NONE,
        }
    }
    pub fn press(at: egui::Pos2) -> Self {
        Self {
            at,
            button: Some(true),
            mods: egui::Modifiers::NONE,
        }
    }
    pub fn release(at: egui::Pos2) -> Self {
        Self {
            at,
            button: Some(false),
            mods: egui::Modifiers::NONE,
        }
    }
    /// The same step with something held down.
    pub fn holding(mut self, mods: egui::Modifiers) -> Self {
        self.mods = mods;
        self
    }
}

/// A drag with a modifier held for the whole gesture — the hover frame
/// included, since egui reads modifiers off the frame the press lands in.
pub fn drag_path_holding(
    from: egui::Pos2,
    to: egui::Pos2,
    steps: usize,
    mods: egui::Modifiers,
) -> Vec<Step> {
    drag_path(from, to, steps)
        .into_iter()
        .map(|s| s.holding(mods))
        .collect()
}

/// A press at `from`, `steps` moves to `to`, then a release — with the
/// bare hover frame in front that egui needs to establish what is under
/// the pointer.
pub fn drag_path(from: egui::Pos2, to: egui::Pos2, steps: usize) -> Vec<Step> {
    let steps = steps.max(1);
    let mut path = vec![Step::moved(from), Step::press(from)];
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        path.push(Step::moved(from + (to - from) * t));
    }
    path.push(Step::release(to));
    path
}

/// A press and release without moving: the gesture egui reports as a
/// CLICK.
pub fn click_path(at: egui::Pos2) -> Vec<Step> {
    vec![Step::moved(at), Step::press(at), Step::release(at)]
}

/// Two clicks in a row at one place — the gesture egui reports as a
/// DOUBLE CLICK, which is how a control is told to go back to its
/// default. egui decides this from the time between the two, and the
/// harness advances time by one frame per step, so four frames is well
/// inside the window.
pub fn double_click_path(at: egui::Pos2) -> Vec<Step> {
    let mut path = click_path(at);
    path.push(Step::press(at));
    path.push(Step::release(at));
    path
}

/// The same gesture with something held for its whole length.
pub fn click_path_holding(at: egui::Pos2, mods: egui::Modifiers) -> Vec<Step> {
    click_path(at)
        .into_iter()
        .map(|s| s.holding(mods))
        .collect()
}

/// Run `f` once per step, inside a ui whose rectangle is exactly `rect`,
/// and collect what it returned each frame.
///
/// The fixed rectangle is what makes a gesture writable: a test can work
/// out where a handle is drawn only if it knows where the widget is.
pub fn run<R>(
    ctx: &egui::Context,
    rect: egui::Rect,
    path: &[Step],
    mut f: impl FnMut(&mut egui::Ui) -> R,
) -> Vec<R> {
    let mut out = Vec::with_capacity(path.len());
    for step in path {
        let mut result = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    rect.max.to_vec2() + egui::vec2(64.0, 64.0),
                )),
                events: {
                    // `RawInput` carries no modifier state of its own —
                    // egui learns what is held from this event — so every
                    // frame states it, including the frames that hold
                    // nothing.
                    let mut events = vec![
                        egui::Event::ModifiersChanged(step.mods),
                        egui::Event::PointerMoved(step.at),
                    ];
                    if let Some(pressed) = step.button {
                        events.push(egui::Event::PointerButton {
                            pos: step.at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: step.mods,
                        });
                    }
                    events
                },
                ..Default::default()
            },
            |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(rect.width());
                child.set_height(rect.height());
                result = Some(f(&mut child));
            },
        );
        run.textures_delta.clear();
        if let Some(result) = result {
            out.push(result);
        }
    }
    out
}
