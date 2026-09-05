//! The track heads: one plate per shown track across the top of the
//! field, the master pinned right, and the cursor on one of them.
//!
//! Identity plus the two-bit state. A head says what its track IS —
//! number and name — and the two things about it that change while you
//! watch and cost most to miss: whether it is muted or soloed, and
//! whether it is sounding now. Level, pan, sends and clips are other
//! surfaces' to show; a head that showed them would be a card. The
//! instrument's family sign was tried here and taken off: it read as a
//! waveform, and a head is not a scope.
//!
//! Filled chamfered plates, no outlines. Rest is the `surface` rung, a
//! muted head sinks to `well`, and the cursor's head is the one
//! `focus`-bright thing on the glass with its words cut out of it in
//! `ground`. Sounding is a `live` bar along the plate's foot.

use super::*;

/// The field's inset from the window, on every side.
pub(super) const MARGIN: f32 = 16.0;
/// One head's plate.
pub(super) const HEAD_W: f32 = 96.0;
pub(super) const HEAD_H: f32 = 44.0;
/// Between two heads, and between the last head and the master.
pub(super) const GAP: f32 = 8.0;
/// The corner every plate gives up. One size everywhere, so it reads as
/// how things here are made and never as a shape of its own.
pub(super) const CHAMFER: f32 = 6.0;
/// The two state pips and the sounding bar, inside the plate.
const PIP: f32 = 8.0;
const PIP_CHAMFER: f32 = 2.0;
const BAR_H: f32 = 3.0;
const INSET: f32 = 6.0;
const NAME_PX: f32 = 12.0;
const NUMBER_PX: f32 = 9.0;

/// How many track heads fit across a field this wide, leaving the
/// master its own column. Never fewer than one, or the cursor would
/// have nowhere to stand.
pub(super) fn capacity(field_w: f32) -> usize {
    let usable = field_w - MARGIN * 2.0 - (HEAD_W + GAP);
    ((usable / (HEAD_W + GAP)).floor().max(1.0)) as usize
}

/// The `slot`th shown head's plate.
pub(super) fn head_rect(field: egui::Rect, slot: usize) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            field.min.x + MARGIN + slot as f32 * (HEAD_W + GAP),
            field.min.y + MARGIN,
        ),
        egui::vec2(HEAD_W, HEAD_H),
    )
}

/// The master's plate, pinned to the field's right edge whatever
/// scrolls past: it belongs to the song, not to anything in it.
pub(super) fn master_rect(field: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(field.max.x - MARGIN - HEAD_W, field.min.y + MARGIN),
        egui::vec2(HEAD_W, HEAD_H),
    )
}

/// A rectangle with its four corners cut, as a filled figure.
pub(super) fn plate(rect: egui::Rect, chamfer: f32, fill: egui::Color32) -> egui::Shape {
    let c = chamfer.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let (l, r, t, b) = (rect.min.x, rect.max.x, rect.min.y, rect.max.y);
    let points = vec![
        egui::pos2(l + c, t),
        egui::pos2(r - c, t),
        egui::pos2(r, t + c),
        egui::pos2(r, b - c),
        egui::pos2(r - c, b),
        egui::pos2(l + c, b),
        egui::pos2(l, b - c),
        egui::pos2(l, t + c),
    ];
    egui::Shape::convex_polygon(points, fill, egui::Stroke::NONE)
}

/// What the cursor makes of one head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Standing {
    /// The cursor is on this head: the one focus-bright thing.
    Cursor,
    /// The cursor is in this head's column, on a scene below it.
    Column,
    Rest,
}

/// Where a head stands relative to the session cursor. `track` is
/// `None` for the master.
pub(super) fn standing(address: Option<Address>, track: Option<usize>) -> Standing {
    match (address, track) {
        (Some(Address::Head { track: on }), Some(t)) if on == t => Standing::Cursor,
        (Some(Address::Slot { track: on, .. }), Some(t)) if on == t => Standing::Column,
        (Some(Address::Master), None) => Standing::Cursor,
        _ => Standing::Rest,
    }
}

/// The plate's face: what one head has to say.
struct Face<'a> {
    number: Option<usize>,
    name: &'a str,
    muted: bool,
    solo: bool,
    sounding: bool,
    standing: Standing,
}

impl Stage {
    /// The tracks the strip shows, as a range into the song's.
    pub(super) fn shown_tracks(&self, field_w: f32) -> std::ops::Range<usize> {
        let count = self.song.tracks.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.strip_offset.min(count - 1);
        first..first.saturating_add(capacity(field_w)).min(count)
    }

    pub(super) fn draw_heads(&self, painter: &egui::Painter, field: egui::Rect) {
        let address = self.session_address();
        for (slot, track) in self.shown_tracks(field.width()).enumerate() {
            let head = &self.song.tracks[track];
            let face = Face {
                number: Some(track + 1),
                name: &head.name,
                muted: head.muted,
                solo: head.solo,
                sounding: self.playing.get(track).copied().flatten().is_some(),
                standing: standing(address, Some(track)),
            };
            self.draw_head(painter, head_rect(field, slot), &face);
        }
        let master = Face {
            number: None,
            name: "MASTER",
            muted: false,
            solo: false,
            sounding: false,
            standing: standing(address, None),
        };
        self.draw_head(painter, master_rect(field), &master);
    }

    fn draw_head(&self, painter: &egui::Painter, rect: egui::Rect, face: &Face<'_>) {
        let alpha = self.alphabet();
        let cursor = face.standing == Standing::Cursor;
        // The plate, then everything on it cut out in the ground's own
        // colour when it is the cursor's — one bright thing, its words
        // holes in it.
        let fill = if cursor {
            alpha.focus.color
        } else if face.muted {
            alpha.well.color
        } else {
            alpha.surface.color
        };
        let (word, quiet) = if cursor {
            (alpha.ground.color, alpha.edge.color)
        } else {
            (alpha.ink.color, alpha.edge.color)
        };
        painter.add(plate(rect, CHAMFER, fill));

        let inner = rect.shrink(INSET);
        if let Some(number) = face.number {
            painter.text(
                inner.left_top(),
                egui::Align2::LEFT_TOP,
                format!("{number:02}"),
                egui::FontId::monospace(NUMBER_PX),
                word,
            );
        }
        // The name, on its own row, clipped by character so it never
        // runs off the plate.
        let fits = (inner.width() / (NAME_PX * 0.62)).floor().max(1.0) as usize;
        let name: String = face.name.chars().take(fits).collect();
        painter.text(
            egui::pos2(inner.min.x, inner.min.y + NUMBER_PX + 4.0),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::monospace(NAME_PX),
            word,
        );

        // The two bits, top right beside the number: M and S as pips,
        // lit when on.
        let pips_y = inner.min.y + 1.0;
        for (i, on) in [(0, face.muted), (1, face.solo)] {
            let x = inner.max.x - PIP - i as f32 * (PIP + 4.0);
            let pip = egui::Rect::from_min_size(egui::pos2(x, pips_y), egui::vec2(PIP, PIP));
            painter.add(plate(pip, PIP_CHAMFER, if on { word } else { quiet }));
        }
        // Sounding: a live bar along the foot.
        if face.sounding {
            let bar =
                egui::Rect::from_min_max(egui::pos2(inner.min.x, inner.max.y - BAR_H), inner.max);
            painter.add(plate(bar, 1.0, alpha.live.color));
        }
        // The cursor in this column but below the head: a focus bar under
        // the foot, pointing at where it is.
        if face.standing == Standing::Column {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + CHAMFER, rect.max.y + 3.0),
                egui::pos2(rect.max.x - CHAMFER, rect.max.y + 3.0 + BAR_H),
            );
            painter.add(plate(bar, 1.5, alpha.focus.color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0))
    }

    #[test]
    fn heads_stack_left_to_right_by_one_gap_and_never_reach_the_master() {
        let f = field();
        let n = capacity(f.width());
        assert!(n >= 1);
        for slot in 1..n {
            let (a, b) = (head_rect(f, slot - 1), head_rect(f, slot));
            assert_eq!(b.min.x - a.max.x, GAP);
            assert_eq!(a.min.y, b.min.y);
        }
        assert!(head_rect(f, n - 1).max.x + GAP <= master_rect(f).min.x);
    }

    #[test]
    fn a_field_too_narrow_for_one_head_still_shows_one() {
        assert_eq!(capacity(10.0), 1);
        assert!(capacity(1280.0) > capacity(640.0));
    }

    /// Exactly one head is the cursor's, wherever the cursor stands on
    /// the session — and none is when it is off the session.
    #[test]
    fn exactly_one_head_is_focus_bright() {
        let heads: Vec<Option<usize>> = (0..4).map(Some).chain([None]).collect();
        let addresses = [
            Some(Address::Head { track: 0 }),
            Some(Address::Head { track: 3 }),
            Some(Address::Slot { track: 2, scene: 5 }),
            Some(Address::Master),
            None,
        ];
        for address in addresses {
            let bright = heads
                .iter()
                .filter(|&&t| standing(address, t) == Standing::Cursor)
                .count();
            let columns = heads
                .iter()
                .filter(|&&t| standing(address, t) == Standing::Column)
                .count();
            match address {
                None => assert_eq!(bright + columns, 0),
                Some(Address::Slot { .. }) => assert_eq!((bright, columns), (0, 1)),
                _ => assert_eq!((bright, columns), (1, 0)),
            }
        }
    }

    #[test]
    fn a_plate_gives_up_exactly_its_corners() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let egui::Shape::Path(path) = plate(rect, 8.0, egui::Color32::WHITE) else {
            panic!("a plate is a filled path");
        };
        assert_eq!(path.points.len(), 8);
        assert!(path.points.iter().all(|p| rect.contains(*p)));
        // A chamfer larger than the plate is cut down to it.
        let egui::Shape::Path(tiny) = plate(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4.0, 4.0)),
            8.0,
            egui::Color32::WHITE,
        ) else {
            panic!()
        };
        assert!(tiny.points.iter().all(|p| p.x >= 0.0 && p.x <= 4.0));
    }
}
