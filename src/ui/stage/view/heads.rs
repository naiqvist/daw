//! The track heads: one plate per shown track across the top of the
//! field, the master pinned right, and the cursor on one of them.
//!
//! Identity plus live state. A head says what its track IS — number and
//! name — and the things about it that change while you watch and cost
//! most to miss: mute, solo, arm, routed input, and whether it is sounding
//! or recording now. Level, pan, sends and clips are other
//! surfaces' to show; a head that showed them would be a card. The
//! instrument's family sign was tried here and taken off: it read as a
//! waveform, and a head is not a scope.
//!
//! Each head wears the console's chassis: a keyed chamfered outline,
//! dashed at rest, solid with brackets for the cursor's. The number is a
//! label (blue), the name is body text, sounding is a nominal bar along
//! the foot, and the two pips go to alert when they are on — a muted
//! track is a thing that wants you.

use super::palette;
use super::*;
use crate::PROFONT;

/// The field's inset from the window, on every side.
/// @tune 0..64 px
pub(super) const MARGIN: f32 = 16.0;
/// The scene gutter, left of the first column: room for a row's number.
/// @tune 0..64 px
pub(super) const GUTTER: f32 = 28.0;
/// One head's width.
/// @tune 48..200 px
pub(super) const HEAD_W: f32 = 96.0;
/// One head's height.
/// @tune 24..96 px
pub(super) const HEAD_H: f32 = 48.0;
/// Between two heads, and between the last head and the master.
/// @tune 0..32 px
pub(super) const GAP: f32 = 8.0;
/// The two state pips and the sounding bar, inside the plate.
const PIP: f32 = 7.0;
const BAR_H: f32 = 2.0;
const INSET: f32 = 8.0;
const TYPE_PX: f32 = 12.0;

/// The field's inset, live.
pub(super) fn margin() -> f32 {
    crate::tune!(MARGIN)
}

/// The scene gutter, live.
pub(super) fn gutter() -> f32 {
    crate::tune!(GUTTER)
}

pub(super) fn head_width() -> f32 {
    crate::tune!(HEAD_W)
}

pub(super) fn head_height() -> f32 {
    crate::tune!(HEAD_H)
}

pub(super) fn gap() -> f32 {
    crate::tune!(GAP)
}

/// How many track heads fit across a field this wide, leaving the
/// master its own column. Never fewer than one, or the cursor would
/// have nowhere to stand.
pub(super) fn capacity(field_w: f32) -> usize {
    let usable = field_w
        - crate::tune!(MARGIN) * 2.0
        - crate::tune!(GUTTER)
        - (crate::tune!(HEAD_W) + crate::tune!(GAP));
    ((usable / (crate::tune!(HEAD_W) + crate::tune!(GAP)))
        .floor()
        .max(1.0)) as usize
}

/// The `slot`th shown head's plate.
pub(super) fn head_rect(field: egui::Rect, slot: usize) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            field.min.x
                + crate::tune!(MARGIN)
                + crate::tune!(GUTTER)
                + slot as f32 * (crate::tune!(HEAD_W) + crate::tune!(GAP)),
            field.min.y + crate::tune!(MARGIN),
        ),
        egui::vec2(crate::tune!(HEAD_W), crate::tune!(HEAD_H)),
    )
}

/// The master's plate, pinned to the field's right edge whatever
/// scrolls past: it belongs to the song, not to anything in it.
pub(super) fn master_rect(field: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            field.max.x - crate::tune!(MARGIN) - crate::tune!(HEAD_W),
            field.min.y + crate::tune!(MARGIN),
        ),
        egui::vec2(crate::tune!(HEAD_W), crate::tune!(HEAD_H)),
    )
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
    armed: bool,
    recording: bool,
    input: Option<String>,
    /// The lane's word and hue, when the track has a design.
    lane: Option<(&'static str, egui::Color32)>,
    sounding: bool,
    standing: Standing,
    key: chassis::Key,
}

/// What a head wears for its lane: the word in the lane's hue, or
/// nothing for a plain lane.
fn lane_badge(
    lane: crate::lane::Lane,
    c: &palette::Colours,
) -> Option<(&'static str, egui::Color32)> {
    match lane {
        crate::lane::Lane::Plain => None,
        crate::lane::Lane::Drum => Some((lane.word(), c.drum)),
    }
}

impl Stage {
    /// The tracks the strip shows, as a range into the song's.
    pub(super) fn shown_tracks(&self, field_w: f32) -> std::ops::Range<usize> {
        let count = self.song.tracks.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.strip_offset.min(count - 1);
        let visible = if self.mixing {
            super::mixer::track_capacity(field_w)
        } else {
            capacity(field_w)
        };
        first..first.saturating_add(visible).min(count)
    }

    pub(super) fn draw_heads(&self, painter: &egui::Painter, field: egui::Rect) {
        let address = self.session_address();
        let shown = self.shown_tracks(field.width());
        let (first, last) = (shown.start, shown.end.saturating_sub(1));
        for (slot, track) in shown.clone().enumerate() {
            let head = &self.song.tracks[track];
            let face = Face {
                number: Some(track + 1),
                name: &head.name,
                muted: head.muted,
                solo: head.solo,
                armed: head.armed,
                recording: self.recording() && head.armed,
                input: (head.kind == crate::sequencing::TrackKind::Audio)
                    .then(|| format!("{} {}", head.input.label(), head.monitor.label())),
                lane: lane_badge(head.lane, &palette::colours()),
                sounding: self.playing.get(track).copied().flatten().is_some(),
                standing: standing(address, Some(track)),
                key: match (track == first, track == last) {
                    // Alone, it is still the leftmost: its brackets stay.
                    (true, true) => chassis::Key::Left,
                    (true, false) => chassis::Key::Left,
                    (false, true) => chassis::Key::Right,
                    (false, false) => chassis::Key::Centre,
                },
            };
            self.draw_head(painter, head_rect(field, slot), &face);
        }
        let master = Face {
            number: None,
            name: "MASTER",
            muted: false,
            solo: false,
            armed: false,
            recording: false,
            input: None,
            lane: None,
            sounding: false,
            standing: standing(address, None),
            key: chassis::Key::Right,
        };
        self.draw_head(painter, master_rect(field), &master);
    }

    fn draw_head(&self, painter: &egui::Painter, rect: egui::Rect, face: &Face<'_>) {
        let c = palette::colours();
        let cursor = face.standing == Standing::Cursor;
        chassis::keyed(painter, rect, cursor, face.key);
        if cursor {
            crate::ui::nav_cursor::claim(
                painter,
                ("head", face.number),
                rect,
                crate::ui::nav_cursor::Kind::Column,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
        }
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));

        let inner = rect.shrink(INSET);
        if let Some(number) = face.number {
            painter.text(
                inner.left_top(),
                egui::Align2::LEFT_TOP,
                format!("{number:02}"),
                font.clone(),
                c.label,
            );
        }
        // The name and an audio lane's route share the second row. Reserve
        // the route's measured width before clipping the name, so input 1/2
        // never overwrites the identity it belongs to.
        let input_width = face.input.as_ref().map_or(0.0, |input| {
            input.chars().count() as f32 * TYPE_PX * 0.6 + 5.0
        });
        let fits = ((inner.width() - input_width) / (TYPE_PX * 0.6))
            .floor()
            .max(1.0) as usize;
        let name: String = face.name.chars().take(fits).collect();
        painter.text(
            egui::pos2(inner.min.x, inner.min.y + TYPE_PX + 4.0),
            egui::Align2::LEFT_TOP,
            name,
            font.clone(),
            if face.recording {
                c.alert
            } else if face.muted {
                c.dim
            } else {
                c.fg
            },
        );
        if let Some(input) = &face.input {
            painter.text(
                egui::pos2(inner.max.x, inner.min.y + TYPE_PX + 4.0),
                egui::Align2::RIGHT_TOP,
                input,
                font.clone(),
                if face.armed { c.alert } else { c.label },
            );
        } else if let Some((word, hue)) = face.lane {
            // The lane's badge: its word in its hue at the row's right
            // end, and a hairline of the same hue along the plate's top
            // inside edge, so the kind reads at a glance from across the
            // strip before the word is read at all.
            painter.text(
                egui::pos2(inner.max.x, inner.min.y + TYPE_PX + 4.0),
                egui::Align2::RIGHT_TOP,
                word,
                font.clone(),
                hue,
            );
            let band = egui::Rect::from_min_max(
                egui::pos2(inner.min.x, rect.min.y),
                egui::pos2(inner.max.x, rect.min.y + 2.0),
            );
            painter.rect_filled(band, 0.0, hue);
        }

        // Track recording is independent of session-to-song capture. Both
        // are explicit words rather than overloading the same red pip.
        if face.armed || (self.arming && cursor) {
            painter.text(
                egui::pos2(
                    inner.max.x - 2.0 * PIP - 10.0,
                    inner.min.y + PIP * 0.5 + 2.0,
                ),
                egui::Align2::RIGHT_CENTER,
                if face.recording {
                    "REC"
                } else if face.armed {
                    "ARM"
                } else {
                    "SONG"
                },
                font.clone(),
                c.alert,
            );
        }
        // The two bits, top right beside the number: M and S as pips —
        // rule when off, alert when on.
        for (i, on) in [(0, face.muted), (1, face.solo)] {
            let x = inner.max.x - PIP - i as f32 * (PIP + 4.0);
            let pip =
                egui::Rect::from_min_size(egui::pos2(x, inner.min.y + 2.0), egui::vec2(PIP, PIP));
            painter.rect_filled(pip, 0.0, if on { c.alert } else { c.rule });
        }
        // Sounding: a nominal bar along the foot.
        if face.sounding || face.recording {
            let bar =
                egui::Rect::from_min_max(egui::pos2(inner.min.x, inner.max.y - BAR_H), inner.max);
            painter.rect_filled(bar, 0.0, if face.recording { c.alert } else { c.nominal });
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
            assert_eq!(b.min.x - a.max.x, crate::tune!(GAP));
            assert_eq!(a.min.y, b.min.y);
        }
        assert!(head_rect(f, n - 1).max.x + crate::tune!(GAP) <= master_rect(f).min.x);
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
}
