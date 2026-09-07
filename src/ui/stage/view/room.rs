//! The machine room, painted: projects, preferences, export and
//! diagnostics, in front of the whole of the musical surface.
//!
//! What the room IS — its pages, rows, values and actions — is
//! `stage::utility`; its keys are read in `view::utility`. This only
//! turns the room's own rows (`Console::display_rows`) into paint:
//! pages as tabs, the current one underlined in alert; rows with a code
//! in blue, a label, a value; a disabled row dim; an alarm in fault; the
//! cursor's row on select with brackets; the status along the foot; and
//! a confirmation, when one is pending, as a small box over the rest.
//!
//! At boot the room opens on the projects page as the first thing the
//! operator sees, and then it is a PLATE rather than a page: a centred
//! chassis with the machine's name and version, the engine's seal, the
//! actions down the left and the remembered projects down the right.
//! The rows are the same rows in the same order — the cursor's index
//! means the same thing on the plate as on the page — only their
//! arrangement differs, so the keys in `view::utility` need not know.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::utility::{Confirm, FIRST_RECENT_ROW, Page, PrefPage};
use crate::ui::stage::vitals::EngineState;
use eframe::egui;

/// One row of the room.
/// @tune 12..32 px
const ROW_H: f32 = 20.0;
const HEAD_H: f32 = 26.0;
const INSET: f32 = 14.0;
const TYPE_PX: f32 = 12.0;

/// The boot plate's width, before the screen clips it.
/// @tune 480..1200 px
const PLATE_W: f32 = 860.0;
/// The boot plate's head: name, version, and the engine's seal.
/// @tune 30..90 px
const PLATE_HEAD_H: f32 = 56.0;
/// The name's type on the plate.
/// @tune 12..40 px
const PLATE_NAME_PX: f32 = 22.0;
/// The share of the plate's width the actions take; the rest is recents.
/// @tune 0.25..0.6
const PLATE_SPLIT: f32 = 0.38;
/// The gap a group of actions leaves before its header.
/// @tune 0..40 px
const GROUP_GAP: f32 = 12.0;
/// The registration marks' arm around the plate.
/// @tune 4..40 px
const PLATE_MARK: f32 = 14.0;
/// How far outside the plate the marks are registered.
/// @tune 0..40 px
const PLATE_MARK_OUT: f32 = 10.0;
/// How far the scrim over the field holds it down behind the plate.
/// @tune 0..255
const SCRIM: u8 = 200;

/// Where a project row stands on the plate: which of the fixed action
/// groups, or the recents column. Indices are `Console::project_rows`'.
const GROUPS: [(&str, std::ops::Range<usize>); 3] =
    [("START", 0..3), ("KEEP", 3..5), ("MACHINE", 5..7)];

/// The keys the plate answers to, as the foot says them.
const PLATE_KEYS: &str = "↑↓ move · enter choose · 1–9 open a recent · esc to the field";

/// Trim a string to `max` columns, marking the cut, so a long path
/// never runs under the column beside it.
fn clip(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max || max < 2 {
        text.to_owned()
    } else {
        let keep = max - 1;
        let start = n - keep;
        let tail: String = text.chars().skip(start).collect();
        format!("…{tail}")
    }
}

impl super::super::Stage {
    pub(super) fn draw_room(&self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(page) = self.utility.page() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin() * 2.0;
        let panel = whole.shrink(margin);
        let row_h = crate::tune!(ROW_H);

        if self.utility.startup && page == Page::Projects {
            self.draw_boot_plate(painter, whole, &font, ch, row_h);
            self.draw_confirm(painter, panel, &font, row_h);
            return;
        }

        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET);

        // The head: the room, its pages, the current one underlined.
        let hy = inner.min.y + HEAD_H * 0.5 - 2.0;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "MACHINE ROOM",
            font.clone(),
            c.label,
        );
        let mut x = inner.min.x + 15.0 * ch;
        for p in Page::ALL {
            let on = p == page;
            let word = p.label();
            painter.text(
                egui::pos2(x, hy),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                if on { c.bright } else { c.dim },
            );
            if on {
                painter.line_segment(
                    [
                        egui::pos2(x, hy + 8.0),
                        egui::pos2(x + word.len() as f32 * ch, hy + 8.0),
                    ],
                    egui::Stroke::new(1.0, c.alert),
                );
            }
            x += (word.len() as f32 + 2.0) * ch;
        }
        if page == Page::Preferences {
            x += 2.0 * ch;
            for p in PrefPage::ALL {
                let on = p == self.utility.pref_page;
                let word = p.label();
                painter.text(
                    egui::pos2(x, hy),
                    egui::Align2::LEFT_CENTER,
                    word,
                    font.clone(),
                    if on { c.fg } else { c.dim },
                );
                if on {
                    painter.line_segment(
                        [
                            egui::pos2(x, hy + 8.0),
                            egui::pos2(x + word.len() as f32 * ch, hy + 8.0),
                        ],
                        egui::Stroke::new(1.0, c.chassis),
                    );
                }
                x += (word.len() as f32 + 2.0) * ch;
            }
        }
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            "esc closes",
            font.clone(),
            c.dim,
        );
        let seam = (inner.min.y + HEAD_H).round() - 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, seam), egui::pos2(inner.max.x, seam)],
            egui::Stroke::new(1.0, c.rule),
        );

        // The rows.
        let snapshot = self.utility_snapshot();
        let rows = self.utility.display_rows(&snapshot);
        let top = inner.min.y + HEAD_H + 6.0;
        let foot = inner.max.y - row_h;
        let editing = self.utility.editing.is_some();
        for (i, row) in rows.iter().enumerate() {
            let y = top + i as f32 * row_h;
            if y + row_h > foot {
                break;
            }
            let rect = egui::Rect::from_min_max(
                egui::pos2(inner.min.x, y),
                egui::pos2(inner.max.x, y + row_h),
            );
            let on = i == self.utility.row;
            if on {
                painter.rect_filled(rect, 0.0, c.select);
            }
            let cy = rect.center().y;
            painter.text(
                egui::pos2(rect.min.x + 2.0, cy),
                egui::Align2::LEFT_CENTER,
                &row.code,
                font.clone(),
                c.label,
            );
            painter.text(
                egui::pos2(rect.min.x + 8.0 * ch, cy),
                egui::Align2::LEFT_CENTER,
                &row.label,
                font.clone(),
                if !row.enabled {
                    c.dim
                } else if on {
                    c.bright
                } else {
                    c.fg
                },
            );
            let value_colour = if row.alarm {
                c.fault
            } else if on && editing {
                c.alert
            } else if !row.enabled {
                c.dim
            } else {
                c.fg
            };
            painter.text(
                egui::pos2(rect.max.x - 2.0, cy),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                font.clone(),
                value_colour,
            );
            if on && editing {
                // The caret, after the value being typed.
                let vx = rect.max.x - 2.0;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(vx, cy - 6.0),
                        egui::pos2(vx + 1.5, cy + 6.0),
                    ),
                    0.0,
                    c.alert,
                );
            }
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("room", i),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Utility,
                    c.alert,
                );
            }
        }

        // The foot: what the room last had to say.
        let fy = inner.max.y - row_h * 0.5;
        if let Some(status) = &self.utility.status {
            painter.text(
                egui::pos2(inner.min.x, fy),
                egui::Align2::LEFT_CENTER,
                status,
                font.clone(),
                c.fg,
            );
        }
        if let Some(audio) = &self.utility.audio_status {
            painter.text(
                egui::pos2(inner.max.x, fy),
                egui::Align2::RIGHT_CENTER,
                audio,
                font.clone(),
                c.dim,
            );
        }

        self.draw_confirm(painter, panel, &font, row_h);
    }

    /// The boot plate: the projects page as the machine's front door.
    fn draw_boot_plate(
        &self,
        painter: &egui::Painter,
        whole: egui::Rect,
        font: &egui::FontId,
        ch: f32,
        row_h: f32,
    ) {
        let c = palette::colours();
        let snapshot = self.utility_snapshot();
        let rows = self.utility.display_rows(&snapshot);
        let recents = &self.utility.recents;
        let recovery = self.utility.recovery.as_ref();

        // The plate's size follows what it has to hold: the deeper column
        // decides the body, and the screen clips the rest.
        let group_gap = crate::tune!(GROUP_GAP);
        let head_h = crate::tune!(PLATE_HEAD_H);
        let left_rows = FIRST_RECENT_ROW as f32 * row_h + GROUPS.len() as f32 * (row_h + group_gap);
        let right_count = usize::from(recovery.is_some()) + recents.len().max(1);
        let right_rows = row_h + group_gap + right_count as f32 * row_h;
        let body_h = left_rows.max(right_rows);
        let plate_h = head_h + body_h + row_h * 2.0 + INSET * 2.0;
        let margin = heads::margin() * 2.0;
        let w = crate::tune!(PLATE_W).min(whole.width() - margin * 2.0);
        let h = plate_h.min(whole.height() - margin * 2.0);
        let plate =
            egui::Rect::from_center_size(whole.center().round(), egui::vec2(w.round(), h.round()));

        // The field stays visible behind, held down under a scrim of the
        // ground, so the plate is the one lit thing on the glass.
        let scrim = egui::Color32::from_rgba_unmultiplied(
            c.ground.r(),
            c.ground.g(),
            c.ground.b(),
            crate::tune!(SCRIM),
        );
        painter.rect_filled(whole, 0.0, scrim);
        painter.rect_filled(plate, 0.0, c.ground);
        chassis::frame(painter, plate, true);
        chassis::marks(
            painter,
            plate.expand(crate::tune!(PLATE_MARK_OUT)),
            f64::from(crate::tune!(PLATE_MARK)),
        );
        let inner = plate.shrink(INSET);

        // The head: the machine's name and version, and the engine's seal
        // — every word of it measured, none of it decoration.
        let name_font = egui::FontId::new(
            crate::tune!(PLATE_NAME_PX),
            egui::FontFamily::Name(PROFONT.into()),
        );
        let hy = inner.min.y + head_h * 0.5 - 2.0;
        let name_rect = painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "DAW",
            name_font,
            c.bright,
        );
        painter.text(
            egui::pos2(name_rect.max.x + ch * 1.5, hy + 3.0),
            egui::Align2::LEFT_CENTER,
            env!("CARGO_PKG_VERSION"),
            font.clone(),
            c.label,
        );
        let seal: Vec<(String, egui::Color32)> = {
            let mut words = Vec::new();
            match self.vitals.stream() {
                Some(s) => {
                    words.push((s.backend.to_owned(), c.fg));
                    words.push((format!("{} HZ", s.sample_rate), c.fg));
                    words.push((format!("{} FRAMES", s.buffer_frames), c.fg));
                }
                None => words.push(("NO STREAM".to_owned(), c.dim)),
            }
            match self.vitals.health().map(|h| &h.state) {
                Some(EngineState::Running) => words.push(("RUNNING".to_owned(), c.nominal)),
                Some(_) => words.push(("ABSENT".to_owned(), c.dim)),
                None => words.push(("--".to_owned(), c.dim)),
            }
            words
        };
        let mut x = inner.max.x;
        for (i, (word, colour)) in seal.iter().enumerate().rev() {
            let r = painter.text(
                egui::pos2(x, hy),
                egui::Align2::RIGHT_CENTER,
                word,
                font.clone(),
                *colour,
            );
            x = r.min.x - ch;
            if i > 0 {
                painter.text(
                    egui::pos2(x, hy),
                    egui::Align2::RIGHT_CENTER,
                    "·",
                    font.clone(),
                    c.rule,
                );
                x -= ch * 1.5;
            }
        }
        let seam = (inner.min.y + head_h).round() - 0.5;
        painter.line_segment(
            [egui::pos2(inner.min.x, seam), egui::pos2(inner.max.x, seam)],
            egui::Stroke::new(1.0, c.rule),
        );

        // The body: actions left, recents right, a rule between.
        let body_top = seam + 0.5 + group_gap;
        let foot = inner.max.y - row_h;
        let split_x = (inner.min.x + inner.width() * crate::tune!(PLATE_SPLIT)).round();
        let left = egui::Rect::from_min_max(
            egui::pos2(inner.min.x, body_top),
            egui::pos2(split_x - INSET, foot),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(split_x + INSET, body_top),
            egui::pos2(inner.max.x, foot),
        );
        painter.line_segment(
            [
                egui::pos2(split_x + 0.5, body_top),
                egui::pos2(split_x + 0.5, foot - group_gap),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let header = |painter: &egui::Painter, at: egui::Rect, y: f32, word: &str, aside: &str| {
            painter.text(
                egui::pos2(at.min.x, y + row_h * 0.5),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                c.label,
            );
            if !aside.is_empty() {
                painter.text(
                    egui::pos2(at.max.x, y + row_h * 0.5),
                    egui::Align2::RIGHT_CENTER,
                    aside,
                    font.clone(),
                    c.dim,
                );
            }
        };
        let editing = self.utility.editing.is_some();
        let cursor = self.utility.row;
        let claim = |painter: &egui::Painter, i: usize, rect: egui::Rect| {
            painter.rect_filled(rect, 0.0, c.select);
            crate::ui::nav_cursor::claim(
                painter,
                ("room", i),
                rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Utility,
                c.alert,
            );
        };

        // Left: the fixed actions in their groups.
        let mut y = left.min.y;
        let left_cols = (left.width() / ch).floor() as usize;
        for (word, range) in GROUPS {
            header(painter, left, y, word, "");
            y += row_h;
            for i in range {
                let Some(row) = rows.get(i) else { break };
                if y + row_h > left.max.y {
                    break;
                }
                let rect = egui::Rect::from_min_max(
                    egui::pos2(left.min.x, y),
                    egui::pos2(left.max.x, y + row_h),
                );
                let on = i == cursor;
                if on {
                    claim(painter, i, rect);
                }
                let cy = rect.center().y;
                painter.text(
                    egui::pos2(rect.min.x + ch, cy),
                    egui::Align2::LEFT_CENTER,
                    &row.label,
                    font.clone(),
                    if !row.enabled {
                        c.dim
                    } else if on {
                        c.bright
                    } else {
                        c.fg
                    },
                );
                let room = left_cols.saturating_sub(row.label.chars().count() + 3);
                let value = clip(&row.value, room);
                let value_colour = if row.alarm {
                    c.fault
                } else if on && editing {
                    c.alert
                } else {
                    c.dim
                };
                painter.text(
                    egui::pos2(rect.max.x - ch, cy),
                    egui::Align2::RIGHT_CENTER,
                    &value,
                    font.clone(),
                    value_colour,
                );
                if on && editing {
                    let vx = rect.max.x - ch;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(vx, cy - 6.0),
                            egui::pos2(vx + 1.5, cy + 6.0),
                        ),
                        0.0,
                        c.alert,
                    );
                }
                y += row_h;
            }
            y += group_gap;
        }

        // Right: the autosave to recover, if one waits, then the recents
        // — title, where it lives, and what the disk says about it.
        let mut y = right.min.y;
        let right_cols = (right.width() / ch).floor() as usize;
        let count = if recents.is_empty() {
            String::new()
        } else {
            format!("{} REMEMBERED", recents.len())
        };
        header(painter, right, y, "RECENT", &count);
        y += row_h;
        let mut i = FIRST_RECENT_ROW;
        if let Some(path) = recovery {
            let rect = egui::Rect::from_min_max(
                egui::pos2(right.min.x, y),
                egui::pos2(right.max.x, y + row_h),
            );
            let on = i == cursor;
            if on {
                claim(painter, i, rect);
            }
            let cy = rect.center().y;
            painter.text(
                egui::pos2(rect.min.x + ch, cy),
                egui::Align2::LEFT_CENTER,
                "!",
                font.clone(),
                c.alert,
            );
            painter.text(
                egui::pos2(rect.min.x + ch * 3.0, cy),
                egui::Align2::LEFT_CENTER,
                "RECOVER AUTOSAVE",
                font.clone(),
                c.alert,
            );
            let room = right_cols.saturating_sub("RECOVER AUTOSAVE".len() + 3);
            painter.text(
                egui::pos2(rect.max.x - ch, cy),
                egui::Align2::RIGHT_CENTER,
                clip(&path.display().to_string(), room),
                font.clone(),
                c.dim,
            );
            y += row_h;
            i += 1;
        }
        if recents.is_empty() {
            painter.text(
                egui::pos2(right.min.x + ch, y + row_h * 0.5),
                egui::Align2::LEFT_CENTER,
                "nothing remembered yet — a saved project is kept here",
                font.clone(),
                c.dim,
            );
        }
        for recent in recents {
            if y + row_h > right.max.y {
                break;
            }
            let rect = egui::Rect::from_min_max(
                egui::pos2(right.min.x, y),
                egui::pos2(right.max.x, y + row_h),
            );
            let on = i == cursor;
            if on {
                claim(painter, i, rect);
            }
            let cy = rect.center().y;
            let title_colour = if recent.missing {
                c.dim
            } else if on {
                c.bright
            } else {
                c.fg
            };
            // The digit that opens it, for the first nine; a blank for
            // the rest so the titles still stand in one column.
            let digit = i - FIRST_RECENT_ROW - usize::from(recovery.is_some());
            if digit < 9 {
                painter.text(
                    egui::pos2(rect.min.x + ch, cy),
                    egui::Align2::LEFT_CENTER,
                    (digit + 1).to_string(),
                    font.clone(),
                    c.label,
                );
            }
            let title = painter.text(
                egui::pos2(rect.min.x + ch * 3.0, cy),
                egui::Align2::LEFT_CENTER,
                &recent.title,
                font.clone(),
                title_colour,
            );
            let detail = painter.text(
                egui::pos2(rect.max.x - ch, cy),
                egui::Align2::RIGHT_CENTER,
                &recent.detail,
                font.clone(),
                if recent.missing { c.alert } else { c.dim },
            );
            // The folder fills whatever lies between, clipped from the
            // left so the nearest directory is the part that survives.
            let room = ((detail.min.x - title.max.x) / ch - 3.0).floor();
            if room >= 6.0 {
                painter.text(
                    egui::pos2(title.max.x + ch * 2.0, cy),
                    egui::Align2::LEFT_CENTER,
                    clip(&recent.folder, room as usize),
                    font.clone(),
                    c.dir,
                );
            }
            y += row_h;
            i += 1;
        }

        // The foot: the keys, and the last thing the room said or the
        // home the projects are kept in.
        let fy = inner.max.y - row_h * 0.5;
        painter.text(
            egui::pos2(inner.min.x, fy),
            egui::Align2::LEFT_CENTER,
            PLATE_KEYS,
            font.clone(),
            c.dim,
        );
        let (word, colour) = match &self.utility.status {
            Some(status) => (status.clone(), c.fg),
            None => (format!("home {}", self.utility.project_folder), c.dim),
        };
        let used = PLATE_KEYS.chars().count() + 3;
        let room = ((inner.width() / ch) as usize).saturating_sub(used);
        painter.text(
            egui::pos2(inner.max.x, fy),
            egui::Align2::RIGHT_CENTER,
            clip(&word, room),
            font.clone(),
            colour,
        );
    }

    /// A confirmation, over the rest, while one is pending.
    fn draw_confirm(
        &self,
        painter: &egui::Painter,
        panel: egui::Rect,
        font: &egui::FontId,
        row_h: f32,
    ) {
        let c = palette::colours();
        if let Some(confirm) = &self.utility.confirm {
            let (question, options): (&str, [&str; 3]) = match confirm {
                Confirm::Replace(_) => (
                    "the project has unsaved work",
                    ["SAVE + CONTINUE", "DISCARD + CONTINUE", "CANCEL"],
                ),
                Confirm::OverwriteProject(_) => (
                    "a project is already there",
                    ["OVERWRITE", "USE A NEW PATH", "CANCEL"],
                ),
                Confirm::OverwriteExport(_) => (
                    "a file is already there",
                    ["OVERWRITE", "USE A NEW PATH", "CANCEL"],
                ),
            };
            let w = 320.0;
            let h = HEAD_H + 3.0 * row_h + INSET * 2.0;
            let boxr = egui::Rect::from_center_size(panel.center(), egui::vec2(w, h));
            painter.rect_filled(boxr, 0.0, c.ground);
            chassis::frame(painter, boxr, true);
            let bi = boxr.shrink(INSET);
            painter.text(
                egui::pos2(bi.min.x, bi.min.y + HEAD_H * 0.5 - 2.0),
                egui::Align2::LEFT_CENTER,
                question,
                font.clone(),
                c.alert,
            );
            for (i, word) in options.iter().enumerate() {
                let y = bi.min.y + HEAD_H + i as f32 * row_h;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bi.min.x, y),
                    egui::pos2(bi.max.x, y + row_h),
                );
                let on = i == self.utility.confirm_row;
                if on {
                    painter.rect_filled(rect, 0.0, c.select);
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("confirm", i),
                        rect,
                        crate::ui::nav_cursor::Kind::Row,
                        crate::ui::nav_cursor::Layer::Utility,
                        c.alert,
                    );
                }
                painter.text(
                    egui::pos2(rect.min.x + 4.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *word,
                    font.clone(),
                    if on { c.bright } else { c.fg },
                );
            }
        }
    }
}
