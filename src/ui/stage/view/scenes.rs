//! Where a scene's slot falls beneath its head. The lattice's own rows
//! and marks are `stage::scenes`; this is only their place on the glass.

use super::*;
pub(super) use crate::ui::stage::scenes::*;

/// The slot `row` rows beneath `head`, sharing its left edge and width
/// exactly. The first row sits one `section` under the head — which the
/// stage sets to nothing, because a head and its cells are one column —
/// and every row after sits one `gap` under the one before.
pub fn slot_beneath(head: egui::Rect, row: usize, gap: f32, section: f32) -> egui::Rect {
    let top = head.max.y + section + row as f32 * (SLOT_H + gap);
    egui::Rect::from_min_size(
        egui::pos2(head.min.x, top),
        egui::vec2(head.width(), SLOT_H),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::SESSION_SCENES;

    const GAP: f32 = 8.0;
    /// The section break under the heads, in these tests' own terms.
    const SECTION: f32 = 0.0;

    fn head() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(50.0, 20.0), egui::vec2(132.0, 64.0))
    }

    #[test]
    fn every_slot_is_exactly_as_wide_as_its_head() {
        for row in 0..SESSION_SCENES {
            let slot = slot_beneath(head(), row, GAP, SECTION);
            assert_eq!(slot.min.x, head().min.x, "row {row} drifted sideways");
            assert_eq!(slot.width(), head().width(), "row {row} changed width");
            assert_eq!(slot.height(), SLOT_H);
        }
    }

    #[test]
    fn rows_stack_downward_by_one_gap_and_never_overlap() {
        let first = slot_beneath(head(), 0, GAP, SECTION);
        assert_eq!(
            first.min.y,
            head().max.y + SECTION,
            "the first row did not take the section break under the heads"
        );
        for row in 1..SESSION_SCENES {
            let above = slot_beneath(head(), row - 1, GAP, SECTION);
            let below = slot_beneath(head(), row, GAP, SECTION);
            assert_eq!(below.min.y, above.max.y + GAP, "row {row} lost its gap");
        }
    }
}
