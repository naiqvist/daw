//! What a control says about itself BEFORE it is touched.
//!
//! Every interactive rectangle in this app is drawn by hand: there is no
//! `egui::Button` to inherit a hover state or a cursor from. That bought
//! the look, and it quietly cost the two signals a pointer relies on to
//! tell a control from a picture — so both have to be put back by hand,
//! and the only way that stays true across a hundred call sites is for
//! there to be one way to say it.
//!
//! # The two signals
//!
//! **The cursor** answers "is there anything here", and it answers
//! before a click is spent finding out. It is the cheaper of the two and
//! the one this codebase had none of: a pointer that never changes shape
//! over an entire mixer says the mixer is a photograph.
//!
//! **The paint** answers "this one, the one under you now" — hover fill,
//! a brighter stroke, a lit handle. That one cannot be centralised,
//! because every control here paints itself differently; what can be
//! centralised is noticing when it is missing, which
//! `every_control_says_what_it_affords` does by reading this source.
//!
//! # Why an extension trait
//!
//! So it rides the response where the response is made:
//!
//! ```ignore
//! let fader = ui.interact(rect, id, Sense::click_and_drag()).affords(Affords::Slide);
//! ```
//!
//! A free function would have to be given the response and handed it
//! back, and at a hundred sites that is a hundred chances to bind the
//! result to nothing and lose the cursor with it.

use eframe::egui;

/// What a pointer over this control is about to be able to do.
///
/// Named for the GESTURE rather than for the cursor, because the cursor
/// is a rendering of the answer and not the answer. Two controls that
/// afford the same gesture should look the same under the pointer even
/// if one of them later wants a different icon on some platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affords {
    /// A button, a lamp, a cell: press and something happens.
    Press,
    /// A value that moves up and down — a fader, a vertical meter's
    /// handle, an envelope point's level.
    Slide,
    /// A value that moves left and right — a pan bar, a send, a
    /// horizontal scrollbar.
    Sweep,
    /// A knob or a two-axis handle: it goes wherever the hand goes.
    Steer,
    /// A canvas that is drawn ON — an automation lane, a note grid.
    /// Distinct from `Press` because the whole surface is live, and from
    /// `Steer` because there is nothing under the pointer yet.
    Draw,
    /// Something that is picked up and put down somewhere else — a clip,
    /// a lane in a reorder, a device in a chain.
    Carry,
    /// A divider between two regions, dragged to give one of them room.
    SeamX,
    SeamY,
    /// A field that takes typing.
    Write,
    /// Under the pointer, and deliberately doing nothing. The rarest and
    /// the most easily forgotten: a control that is present but refused
    /// has to say so, or it reads as broken rather than as unavailable.
    Refuse,
}

impl Affords {
    /// The pointer's shape for this gesture. `held` is whether the
    /// gesture is already under way, which only the carry distinguishes
    /// — an open hand becomes a closed one.
    pub fn icon(self, held: bool) -> egui::CursorIcon {
        match self {
            Self::Press => egui::CursorIcon::PointingHand,
            Self::Slide => egui::CursorIcon::ResizeVertical,
            Self::Sweep => egui::CursorIcon::ResizeHorizontal,
            Self::Steer => egui::CursorIcon::ResizeNwSe,
            Self::Draw => egui::CursorIcon::Crosshair,
            Self::Carry if held => egui::CursorIcon::Grabbing,
            Self::Carry => egui::CursorIcon::Grab,
            Self::SeamX => egui::CursorIcon::ResizeHorizontal,
            Self::SeamY => egui::CursorIcon::ResizeVertical,
            Self::Write => egui::CursorIcon::Text,
            Self::Refuse => egui::CursorIcon::NotAllowed,
        }
    }
}

pub trait Afford {
    /// Say what this control is for, so the pointer changes shape over
    /// it — and KEEPS that shape for as long as a gesture on it lasts,
    /// even once the hand has dragged past the control's own edge, which
    /// is where a fader spends most of a long move.
    #[must_use]
    fn affords(self, what: Affords) -> Self;
}

impl Afford for egui::Response {
    fn affords(self, what: Affords) -> Self {
        if self.dragged() || self.is_pointer_button_down_on() {
            self.ctx.set_cursor_icon(what.icon(true));
            self
        } else {
            self.on_hover_cursor(what.icon(false))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Every gesture gets its own shape, and only the carry changes
    /// while it is under way.
    #[test]
    fn each_gesture_has_a_shape_of_its_own() {
        let all = [
            Affords::Press,
            Affords::Slide,
            Affords::Sweep,
            Affords::Steer,
            Affords::Draw,
            Affords::Carry,
            Affords::SeamX,
            Affords::SeamY,
            Affords::Write,
            Affords::Refuse,
        ];
        for what in all {
            let resting = what.icon(false);
            assert_ne!(
                resting,
                egui::CursorIcon::Default,
                "{what:?} says nothing under the pointer"
            );
            assert_eq!(
                what.icon(true) != resting,
                what == Affords::Carry,
                "{what:?} changed shape mid-gesture, or the carry did not"
            );
        }
        // A seam and a slider along the same axis DO share a shape, and
        // that is right: they are the same gesture on different things.
        assert_eq!(Affords::SeamY.icon(false), Affords::Slide.icon(false));
        assert_eq!(Affords::SeamX.icon(false), Affords::Sweep.icon(false));
    }

    /// EVERY CONTROL SAYS WHAT IT AFFORDS.
    ///
    /// Read off the source, the way `every_palette_command_has_a_handler`
    /// reads the palette off its own — because the thing being checked
    /// is a habit across a hundred call sites, and a habit is exactly
    /// what a reviewer stops noticing.
    ///
    /// The rule: an `interact` that senses a click or a drag must be
    /// followed, within the same expression, by `.affords(..)`. A
    /// `Sense::hover()` is exempt: it takes no gesture, so there is
    /// nothing for the pointer to promise.
    #[test]
    fn every_control_says_what_it_affords() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing: Vec<String> = Vec::new();
        let mut checked = 0;
        for path in sources(std::path::Path::new(root)) {
            let text = std::fs::read_to_string(&path).expect("a source file reads");
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            for (at, _) in text.match_indices(".interact") {
                // `.interact_bg` and `.interact` both count; a mention
                // inside a comment or a doc line does not.
                let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
                let line = &text[line_start..at];
                if line.trim_start().starts_with("//") || line.contains('"') {
                    continue;
                }
                // The expression runs to its terminating semicolon.
                let rest = &text[at..];
                let end = rest.find(";\n").unwrap_or(rest.len().min(400));
                let expression = &rest[..end];
                if !expression.contains("Sense::click") && !expression.contains("Sense::drag") {
                    continue;
                }
                checked += 1;
                if !expression.contains(".affords(") {
                    let line_no = text[..at].matches('\n').count() + 1;
                    missing.push(format!("{name}:{line_no}"));
                }
            }
        }
        assert!(
            checked > 80,
            "the scan found only {checked} controls — it has stopped matching"
        );
        assert!(
            missing.is_empty(),
            "controls that do not say what they afford, so the pointer \
             never changes shape over them: {missing:#?}"
        );
    }

    fn sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(sources(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
        out
    }
}
