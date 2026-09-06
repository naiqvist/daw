//! The DOCK: the whole signal path, minified, under the sequencer.
//!
//! The band is a rail, and a rail is the wrong shape for a run of
//! twenty-nine devices — most of it is off screen and walking it is
//! walking a corridor. The dock trades all of the detail for all of the
//! path: one chip per device, in signal order, every one of them on
//! screen at once.
//!
//! A chip says the four things that survive being that small: what it
//! is, whether it is IN, which rail it stands on, and whether the cursor
//! is on it. Everything else is what the other two presentations are
//! for.
//!
//! Reading order is signal order, left to right and then down, which is
//! the same order the rail has and the same order the bento has. The
//! three presentations differ in shape and in nothing else.

use super::palette;
use super::*;
use crate::PROFONT;
use crate::ui::stage::chain;

/// One chip's height.
/// @tune 12..40 px
const CHIP_H: f32 = 20.0;
/// The gap between chips, and between rows of them.
/// @tune 1..12 px
const CHIP_GAP: f32 = 3.0;
/// The room a chip's word needs, before its marks.
/// @tune 30..120 px
const CHIP_W: f32 = 62.0;
const TYPE_PX: f32 = 12.0;

/// Where the `index`th chip stands, given how many fit across.
fn chip_rect(area: egui::Rect, across: usize, index: usize) -> egui::Rect {
    let across = across.max(1);
    let (col, row) = (index % across, index / across);
    egui::Rect::from_min_size(
        egui::pos2(
            area.left() + col as f32 * (crate::tune!(CHIP_W) + CHIP_GAP),
            area.top() + row as f32 * (crate::tune!(CHIP_H) + CHIP_GAP),
        ),
        egui::vec2(crate::tune!(CHIP_W), crate::tune!(CHIP_H)),
    )
}

/// How many chips fit across `area`.
fn across(area: egui::Rect) -> usize {
    ((area.width() + CHIP_GAP) / (crate::tune!(CHIP_W) + CHIP_GAP))
        .floor()
        .max(1.0) as usize
}

/// How tall the dock is for `count` chips in a tray this wide.
///
/// The dock stands at the FOOT of the sequencer rather than in its
/// place: it is small enough to, and having the clip and the path it
/// runs through on screen at once is most of the reason to minify a
/// chain at all. So it says how much room it needs and the sequencer
/// takes the rest.
pub(super) fn height(tray: egui::Rect, count: usize) -> f32 {
    let area = egui::Rect::from_min_max(
        egui::pos2(
            tray.min.x + super::heads::margin() + super::heads::gutter(),
            tray.min.y,
        ),
        egui::pos2(tray.max.x - super::heads::margin(), tray.max.y),
    );
    let rows = count.div_ceil(across(area).max(1)).max(1);
    rows as f32 * (crate::tune!(CHIP_H) + CHIP_GAP) + CHIP_GAP * 2.0
}

impl super::super::Stage {
    /// How many chips the dock has to lay out.
    pub(super) fn dock_len(&self) -> usize {
        self.addressed_track()
            .map(|track| chain::band(&self.song, track).len())
            .unwrap_or(0)
    }

    /// Draw the dock. Returns nothing: the chips are a picture of the
    /// same lattice the rail walks, so the keys are unchanged.
    pub(super) fn draw_dock(&self, painter: &egui::Painter, tray: egui::Rect) {
        let Some(lattice) = self.chain.as_ref() else {
            return;
        };
        let Some(track) = self.addressed_track() else {
            return;
        };
        let columns = chain::band(&self.song, track);
        if columns.is_empty() {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let cursor = lattice.cursor().map(|(col, _)| col);
        let area = egui::Rect::from_min_max(
            egui::pos2(
                tray.min.x + super::heads::margin() + super::heads::gutter(),
                tray.min.y + CHIP_GAP,
            ),
            egui::pos2(tray.max.x - super::heads::margin(), tray.max.y - CHIP_GAP),
        );
        if !area.is_positive() {
            return;
        }
        let across = across(area);
        let mut shapes = Vec::new();
        for (index, column) in columns.iter().enumerate() {
            let chip = chip_rect(area, across, index);
            if chip.bottom() > area.bottom() {
                break;
            }
            let on = cursor == Some(index);
            let out = column.bypassed;
            // The chip's own ground says IN or OUT before any word does.
            shapes.push(egui::Shape::rect_filled(
                chip,
                0.0,
                if out {
                    c.panel
                } else {
                    c.select.gamma_multiply(0.5)
                },
            ));
            chrome::trace(
                &mut shapes,
                &chrome::keyed_outline(chip),
                if on { Weight::Heavy } else { Weight::Hair },
                if on { c.alert } else { c.rule },
            );
            // The rail it stands on, as a mark in the chip's corner: the
            // channel's own run, the bus, the mix, a return.
            let rail = match column.lane {
                chain::Lane::Channel => None,
                chain::Lane::Bus(_) => Some(c.label),
                chain::Lane::Mix => Some(c.nominal),
                chain::Lane::Return(_) => Some(c.alert),
            };
            if let Some(ink) = rail {
                chrome::pad(
                    &mut shapes,
                    egui::pos2(chip.right() - 5.0, chip.top() + 5.0),
                    chrome::PAD - 1.0,
                    ink,
                    true,
                );
            }
        }
        painter.extend(shapes);
        // The words last, so a chip's ground never sits over its name.
        for (index, column) in columns.iter().enumerate() {
            let chip = chip_rect(area, across, index);
            if chip.bottom() > area.bottom() {
                break;
            }
            let on = cursor == Some(index);
            painter.text(
                egui::pos2(chip.left() + 5.0, chip.center().y),
                egui::Align2::LEFT_CENTER,
                column.code.to_ascii_uppercase(),
                font.clone(),
                if column.bypassed {
                    c.dim
                } else if on {
                    c.fg
                } else {
                    c.ink
                },
            );
        }
        if let Some(index) = cursor {
            let chip = chip_rect(area, across, index);
            if chip.bottom() <= area.bottom() {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("stage-dock-chip", index),
                    chip,
                    crate::ui::nav_cursor::Kind::Cell,
                    crate::ui::nav_cursor::Layer::Surface,
                    c.alert,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 120.0))
    }

    /// Reading order is signal order: along a row, then down to the next.
    #[test]
    fn the_chips_read_left_to_right_then_down() {
        let a = area();
        let n = across(a);
        assert!(n > 1, "a dock this wide should fit several chips");
        for i in 1..n {
            let (before, here) = (chip_rect(a, n, i - 1), chip_rect(a, n, i));
            assert!(before.right() <= here.left(), "chip {i} went backwards");
            assert_eq!(before.top(), here.top(), "chip {i} left its row");
        }
        // The chip after the last of a row starts the next one, at the
        // left edge again — which is what reading order means.
        let wrapped = chip_rect(a, n, n);
        assert_eq!(wrapped.left(), chip_rect(a, n, 0).left());
        assert!(wrapped.top() > chip_rect(a, n, 0).top());
    }

    /// Every chip is the same size, whatever it holds: a dock whose
    /// chips changed width with their words would be unreadable as a
    /// path.
    #[test]
    fn every_chip_is_the_same_size() {
        let a = area();
        let n = across(a);
        let first = chip_rect(a, n, 0);
        for i in 0..24 {
            let chip = chip_rect(a, n, i);
            assert_eq!(chip.size(), first.size(), "chip {i} is a different size");
        }
    }

    /// A dock too narrow for even one chip still lays one out rather
    /// than dividing by zero.
    #[test]
    fn a_narrow_dock_still_has_a_row() {
        let thin = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 40.0));
        assert_eq!(across(thin), 1);
        assert!(chip_rect(thin, across(thin), 3).is_positive());
    }
}
