//! The top bar's layout: a left-to-right cursor and the three anchors.
//!
//! Pure geometry — nothing here touches a `Ui`, a theme or the app, so the
//! bar's whole rhythm is checkable without a window. What a slot LOOKS
//! like stays in `main.rs`; this file only decides where it goes.
//!
//! Lifted out of `main.rs` unchanged.

// The field WELL's height is shared with the code that paints it, which
// still lives in `main.rs`.
use crate::FIELD_H;

/// Square hit area for one transport button.
pub const TRANSPORT_BTN: f32 = 26.0;
/// Gap from the bar's left edge.
pub const TRANSPORT_PAD: f32 = 10.0;
/// Gap between adjacent buttons.
pub const TRANSPORT_GAP: f32 = 4.0;
/// Space between GROUPS of controls. Groups are separated by air, not by
/// rules — this window is fills only, and a divider on the bar would be the
/// first line anywhere in it.
pub const TRANSPORT_GROUP_GAP: f32 = 18.0;

/// Lays the bar out left to right, everything vertically centred.
///
/// A cursor rather than indexed slots, because the bar mixes square buttons,
/// wider fields and readouts, and groups separated by air. Pure — it never
/// touches a `Ui` — so the whole rhythm is checkable.
pub struct Bar {
    pub x: f32,
    pub mid: f32,
}

impl Bar {
    /// A cursor starting at an arbitrary x, for anchoring a group to the
    /// centre or the right edge instead of running everything off the left.
    pub fn at(area: egui::Rect, x: f32) -> Self {
        Self {
            x,
            mid: area.center().y,
        }
    }

    pub fn button(&mut self) -> egui::Rect {
        let rect = egui::Rect::from_center_size(
            egui::pos2(self.x + TRANSPORT_BTN * 0.5, self.mid),
            egui::Vec2::splat(TRANSPORT_BTN),
        );
        self.x += TRANSPORT_BTN + TRANSPORT_GAP;
        rect
    }

    pub fn field(&mut self, width: f32) -> egui::Rect {
        let rect = egui::Rect::from_min_size(
            egui::pos2(self.x, self.mid - FIELD_H * 0.5),
            egui::vec2(width, FIELD_H),
        );
        self.x += width + TRANSPORT_GAP;
        rect
    }

    /// Air between groups, in place of a divider.
    pub fn group(&mut self) {
        self.x += TRANSPORT_GROUP_GAP - TRANSPORT_GAP;
    }
}

/// Width of `n` buttons laid in a row, gaps included.
pub fn buttons_width(n: usize) -> f32 {
    n as f32 * TRANSPORT_BTN + n.saturating_sub(1) as f32 * TRANSPORT_GAP
}

/// Width of a run of fields, gaps included.
pub fn fields_width(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * TRANSPORT_GAP
}

/// Where each group starts, given the bar's width.
///
/// Three anchors: the verbs hold the left edge so muscle memory has somewhere
/// fixed to aim, the readouts sit dead centre because they are what you look
/// at, and the settings hold the right edge. Each anchor is stable under
/// resize — the middle stays middle, the ends stay at their ends.
///
/// If the three would collide, everything falls back to packed-left in the
/// same order. Pure, so both branches are checkable.
pub fn bar_layout(area: egui::Rect, verbs: f32, centre: f32, right: f32) -> (f32, f32, f32, bool) {
    let left_x = area.left() + TRANSPORT_PAD;
    let left_end = left_x + verbs;
    let right_x = area.right() - TRANSPORT_PAD - right;
    let centre_x = area.center().x - centre * 0.5;

    let spread = centre_x > left_end + TRANSPORT_GROUP_GAP
        && centre_x + centre < right_x - TRANSPORT_GROUP_GAP;

    if spread {
        (left_x, centre_x, right_x, true)
    } else {
        let centre_x = left_end + TRANSPORT_GROUP_GAP;
        (
            left_x,
            centre_x,
            centre_x + centre + TRANSPORT_GROUP_GAP,
            false,
        )
    }
}
