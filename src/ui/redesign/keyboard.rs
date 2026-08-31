//! Keyboard-first interaction for the redesign frame.
//!
//! This owns directional focus (Tab and Shift+Tab move between regions, the
//! selected region carries the focus bar) and the frame's one
//! [`grammar::Sentence`] — the sentence-in-progress survives a focus change
//! because it lives here, above every panel. Panels pull utterances from
//! the sentence and interpret them against their own nouns
//! (`notes/20260831-command-grammar.md`).
//!
//! Keeping this vocabulary separate from rendering prevents every future
//! control from becoming its own, incompatible keyboard system.

use crate::ui::redesign::{grammar, registers};
use eframe::egui;

/// Regions that can receive global keyboard focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusTarget {
    Transport,
    Browser,
    Arrangement,
    Chain,
    Sequence,
    Tools,
}

const FOCUS_ORDER: [FocusTarget; 6] = [
    FocusTarget::Transport,
    FocusTarget::Browser,
    FocusTarget::Arrangement,
    FocusTarget::Chain,
    FocusTarget::Sequence,
    FocusTarget::Tools,
];

/// Persistent focus and sentence state for the redesign frame.
pub(crate) struct Keyboard {
    focus: FocusTarget,
    pub(crate) sentence: grammar::Sentence,
    /// The grammar's clipboard, shared by every panel so a yank travels.
    pub(crate) registers: registers::Registers,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            focus: FocusTarget::Transport,
            sentence: grammar::Sentence::default(),
            registers: registers::Registers::default(),
        }
    }
}

/// One frame's navigation result: where attention is, and whether the
/// detail strip was asked to flip its occupant.
pub(crate) struct Nav {
    pub(crate) focus: FocusTarget,
    pub(crate) flip_detail: bool,
}

/// Where a spatial travel gesture lands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Travel {
    To(FocusTarget),
    FlipDetail,
}

#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Keyboard {
    /// Apply the small global navigation vocabulary and return the active
    /// region for this frame's rendering.
    ///
    /// Two vocabularies: Tab/Shift+Tab walk the ring (the fallback), and
    /// Ctrl+arrows travel the frame's GEOMETRY — the layout is a map, so
    /// the keys need no memorizing. Ctrl+Down inside the detail strip
    /// flips its occupant (sequencer ↔ chain). `detail` is the focus
    /// target the strip currently shows.
    pub(crate) fn update(&mut self, ctx: &egui::Context, detail: FocusTarget) -> Nav {
        let mut nav = Nav {
            focus: self.focus,
            flip_detail: false,
        };
        // A focused text field owns the keyboard outright.
        if ctx.egui_wants_keyboard_input() {
            return nav;
        }
        let backwards =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
        let forwards = !backwards
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab));

        if backwards {
            self.step(-1);
        } else if forwards {
            self.step(1);
        }

        for (key, dir) in [
            (egui::Key::ArrowLeft, Dir::Left),
            (egui::Key::ArrowRight, Dir::Right),
            (egui::Key::ArrowUp, Dir::Up),
            (egui::Key::ArrowDown, Dir::Down),
        ] {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key)) {
                match travel(self.focus, detail, dir) {
                    Some(Travel::To(target)) => self.focus = target,
                    Some(Travel::FlipDetail) => nav.flip_detail = true,
                    None => {}
                }
            }
        }
        nav.focus = self.focus;
        nav
    }

    pub(crate) fn focus(&mut self, target: FocusTarget) {
        self.focus = target;
    }

    pub(crate) fn current(&self) -> FocusTarget {
        self.focus
    }

    fn step(&mut self, direction: i8) {
        let index = FOCUS_ORDER
            .iter()
            .position(|target| *target == self.focus)
            .unwrap_or_default() as i8;
        let len = FOCUS_ORDER.len() as i8;
        self.focus = FOCUS_ORDER[(index + direction).rem_euclid(len) as usize];
    }
}

/// The frame as a map: transport above, browser left, arrangement center,
/// tools right, the detail strip below. Travelling off an edge goes
/// nowhere — attention never wraps in space, only on the Tab ring.
fn travel(from: FocusTarget, detail: FocusTarget, dir: Dir) -> Option<Travel> {
    use FocusTarget as F;
    let in_detail = from == F::Sequence || from == F::Chain;
    Some(match (from, dir) {
        (F::Transport, Dir::Down) => Travel::To(F::Arrangement),
        (F::Browser, Dir::Up) => Travel::To(F::Transport),
        (F::Browser, Dir::Right) => Travel::To(F::Arrangement),
        (F::Browser, Dir::Down) => Travel::To(detail),
        (F::Arrangement, Dir::Up) => Travel::To(F::Transport),
        (F::Arrangement, Dir::Left) => Travel::To(F::Browser),
        (F::Arrangement, Dir::Right) => Travel::To(F::Tools),
        (F::Arrangement, Dir::Down) => Travel::To(detail),
        (F::Tools, Dir::Up) => Travel::To(F::Transport),
        (F::Tools, Dir::Left) => Travel::To(F::Arrangement),
        (F::Tools, Dir::Down) => Travel::To(detail),
        (_, Dir::Up) if in_detail => Travel::To(F::Arrangement),
        (_, Dir::Left) if in_detail => Travel::To(F::Browser),
        (_, Dir::Right) if in_detail => Travel::To(F::Tools),
        (_, Dir::Down) if in_detail => Travel::FlipDetail,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map is the keybinding: every travel lands where the panel
    /// visibly sits, and Ctrl+Down inside the strip flips its occupant.
    #[test]
    fn spatial_travel_follows_the_layout() {
        use FocusTarget as F;
        let detail = F::Sequence;
        assert_eq!(
            travel(F::Arrangement, detail, Dir::Down),
            Some(Travel::To(F::Sequence))
        );
        assert_eq!(
            travel(F::Arrangement, detail, Dir::Left),
            Some(Travel::To(F::Browser))
        );
        assert_eq!(
            travel(F::Browser, F::Chain, Dir::Down),
            Some(Travel::To(F::Chain))
        );
        assert_eq!(
            travel(F::Sequence, detail, Dir::Down),
            Some(Travel::FlipDetail)
        );
        assert_eq!(
            travel(F::Chain, F::Chain, Dir::Up),
            Some(Travel::To(F::Arrangement))
        );
        assert_eq!(travel(F::Transport, detail, Dir::Up), None, "no wrap");
    }
}
