//! The stage — the third frame, and deliberately empty.
//!
//! `ui::redesign` is the second. This one starts from nothing on purpose:
//! the two before it grew by accretion, and the shape of a surface is very
//! hard to argue with once something is already drawn on it.
//!
//! **What this may depend on.** The vocabulary (`crate::intent`), the
//! model (`crate::sequencing`), the device cards (`crate::ui::device`) and
//! the design system (`theme`, `tokens`, `skin`, `glyph`, `kit`,
//! `legibility`). Those are frame-independent by construction and were
//! measured to be so — the card layer holds zero references to any frame.
//!
//! **What it may NOT depend on.** `ui::redesign`, or anything that reaches
//! back into a frame. A third frame borrowing the second one's parts is
//! how there come to be two answers to the same question. If something in
//! `redesign` is worth having here, it is worth lifting to a shared home
//! first — `crate::intent` is the worked example.
//!
//! Nothing is drawn yet. A black screen is the honest starting state, not
//! a placeholder: the render pass clears to black, so drawing nothing IS
//! the picture.

use eframe::egui;

/// Everything the stage knows. Empty, and every field added here should
/// have to justify itself — this is the mutable core that the rest of the
/// surface will be a pure function of.
#[derive(Debug, Default)]
pub struct Stage {}

impl Stage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw one frame.
    ///
    /// Takes the whole available rect and paints nothing. The caller has
    /// already cleared to black; this is where that stops being true.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // Consume the space so the panel reports a real size rather than
        // collapsing to nothing — an empty surface should still BE the
        // surface, at full size, ready for the first thing drawn on it.
        let _ = ui.available_rect_before_wrap();
    }
}

#[cfg(test)]
mod tests {
    /// The stage must not borrow from the frame it replaces. Checked as
    /// code rather than trusted as intent, for the same reason
    /// `crate::intent` is checked: the coupling that matters arrives one
    /// convenient import at a time.
    #[test]
    fn the_stage_does_not_reach_into_the_frame_it_replaces() {
        let src = include_str!("mod.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("redesign"),
            "the stage reached into `ui::redesign` — lift the shared part \
             out to a neutral home instead, as `crate::intent` was"
        );
    }
}
