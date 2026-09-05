//! The trig menu's bubble: where it stands and how it points. What the
//! menu SAYS — its rows, its verbs, what each will do — is
//! `stage::trig_menu`, and is decided before any of this is placed.

pub(super) use crate::ui::stage::trig_menu::*;
use eframe::egui::{Pos2, Rect, pos2, vec2};

/// The casing's width. Fixed: a menu that sized itself to its longest
/// row would be a different shape over every trig.
pub(super) const WIDTH: f32 = 352.0;

/// The head: a title row and a line of the trig's facts beneath it.
pub(super) const HEAD_H: f32 = 54.0;

/// One row of the list.
pub(super) const ROW_H: f32 = 19.0;

/// The casing's inner margin, top and bottom.
pub(super) const MARGIN: f32 = 12.0;

/// How far the tail reaches from the casing to the trig.
pub(super) const TAIL_H: f32 = 18.0;

/// The tail's width where it leaves the casing.
pub(super) const TAIL_W: f32 = 24.0;

/// The least the casing stands off the window's edge.
const KEEP_OFF: f32 = 16.0;

/// Where a menu stands: its casing, and the tail from casing to trig.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bubble {
    pub(super) panel: Rect,
    /// Base left, base right, apex — the apex is the point on the trig.
    pub(super) tail: [Pos2; 3],
    /// Whether the casing stands above the trig (the usual case) or had
    /// to drop below it for want of room.
    pub(super) above: bool,
}

/// The slice strip's height: the file's picture above the list, on a
/// slicing track.
pub(super) const STRIP_H: f32 = 52.0;

/// The casing's height for `rows` rows, with `extra` above the list.
pub(super) fn height(rows: usize, extra: f32) -> f32 {
    MARGIN + HEAD_H + extra + rows as f32 * ROW_H + MARGIN
}

/// Place the menu for a trig drawn at `anchor`, inside `whole`.
///
/// The casing stands above the trig, centred on it, and the tail drops
/// from the casing's foot to the trig's top edge. It is pushed sideways
/// to stay inside the window — the tail's base slides with the trig
/// rather than with the casing, so it still points home — and only
/// when there is no room above at all does the casing go beneath the
/// trig instead, the tail then rising to the trig's foot.
pub(super) fn place(anchor: Rect, whole: Rect, rows: usize, extra: f32) -> Bubble {
    let h = height(rows, extra);
    let above = anchor.top() - TAIL_H - h >= whole.top() + KEEP_OFF;
    let x = (anchor.center().x - WIDTH * 0.5).clamp(
        whole.left() + KEEP_OFF,
        (whole.right() - KEEP_OFF - WIDTH).max(whole.left()),
    );
    let panel = if above {
        Rect::from_min_size(pos2(x, anchor.top() - TAIL_H - h), vec2(WIDTH, h))
    } else {
        Rect::from_min_size(pos2(x, anchor.bottom() + TAIL_H), vec2(WIDTH, h))
    };
    let apex_x = anchor.center().x;
    // The base sits under the apex, but never past the casing's own
    // chamfers, so the wedge always leaves a flat edge.
    let base_x = apex_x.clamp(panel.left() + TAIL_W, panel.right() - TAIL_W);
    let (base_y, apex) = if above {
        (panel.bottom(), pos2(apex_x, anchor.top()))
    } else {
        (panel.top(), pos2(apex_x, anchor.bottom()))
    };
    Bubble {
        panel,
        tail: [
            pos2(base_x - TAIL_W * 0.5, base_y),
            pos2(base_x + TAIL_W * 0.5, base_y),
            apex,
        ],
        above,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The casing stands above the trig, centred, and the tail's apex
    /// is on the trig's top edge at its centre: the bubble points at
    /// the thing it is about.
    #[test]
    fn the_bubble_stands_above_the_trig_and_points_at_it() {
        let whole = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let anchor = Rect::from_min_size(pos2(400.0, 600.0), vec2(40.0, 30.0));
        let bubble = place(anchor, whole, TrigAction::ALL.len(), 0.0);
        assert!(bubble.above);
        assert_eq!(bubble.panel.bottom(), anchor.top() - TAIL_H);
        assert_eq!(bubble.panel.center().x, anchor.center().x);
        assert_eq!(bubble.panel.width(), WIDTH);
        let [l, r, apex] = bubble.tail;
        assert_eq!(apex, pos2(anchor.center().x, anchor.top()));
        assert_eq!(l.y, bubble.panel.bottom());
        assert_eq!(r.y, bubble.panel.bottom());
        assert!(
            l.x < apex.x && apex.x < r.x,
            "the tail does not straddle its apex"
        );
    }

    /// Pushed against the window's side the casing stays inside, and
    /// the tail keeps pointing at the trig rather than at the casing's
    /// middle.
    #[test]
    fn the_bubble_stays_in_the_window_and_the_tail_still_points_home() {
        let whole = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let anchor = Rect::from_min_size(pos2(4.0, 600.0), vec2(40.0, 30.0));
        let bubble = place(anchor, whole, 5, 0.0);
        assert!(whole.contains_rect(bubble.panel));
        assert!(bubble.panel.left() >= KEEP_OFF);
        let [l, r, apex] = bubble.tail;
        assert_eq!(apex.x, anchor.center().x);
        assert!(l.x >= bubble.panel.left(), "the tail left the casing");
        assert!(r.x <= bubble.panel.right(), "the tail left the casing");
    }

    /// With no room above, the casing drops below and the tail rises.
    #[test]
    fn with_no_room_above_the_bubble_hangs_below() {
        let whole = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let anchor = Rect::from_min_size(pos2(400.0, 30.0), vec2(40.0, 30.0));
        let bubble = place(anchor, whole, 5, 0.0);
        assert!(!bubble.above);
        assert_eq!(bubble.panel.top(), anchor.bottom() + TAIL_H);
        assert_eq!(bubble.tail[2], pos2(anchor.center().x, anchor.bottom()));
    }
}
