//! The codebook: every chord bound in the scope the keys are in, and
//! what each one means, over the field.
//!
//! A display mode, not a scope: focus never enters it, and the cursor
//! underneath is exactly where it was left. The rows come from the
//! codebook itself (`keymap::bindings_for`), the meanings from the same
//! table the palette reads, so the three can never disagree. Inside a
//! clip the counted phrases worth keeping visible are listed after.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::keymap::{self, ScopeContext};
use eframe::egui;

/// One row of the codebook.
/// @tune 12..32 px
const ROW_H: f32 = 18.0;
/// A column's width.
/// @tune 200..520 px
const COL_W: f32 = 300.0;
const HEAD_H: f32 = 26.0;
const INSET: f32 = 12.0;
const TYPE_PX: f32 = 12.0;
/// The chord column, in cells; the leader runs to its end.
const CHORD_CELLS: usize = 18;
/// Distance between the dots that bind a chord to its command.
/// @tune 3..12 px
const LEADER_STEP: f32 = 6.0;

/// The families in the order the codebook lists them: what moves the
/// hand first, then what the hand does where it is, then the rooms,
/// then the app's own housekeeping last.
const FAMILY_ORDER: [&str; 17] = [
    "move",
    "session",
    "track",
    "mixer",
    "devices",
    "pages",
    "edit",
    "sample",
    "forge",
    "modulation",
    "song",
    "mix",
    "view",
    "time",
    "document",
    "browse",
    "system",
];

/// One line of the codebook.
struct Row {
    chord: String,
    meaning: String,
}

/// A family of lines under its name.
struct Section {
    title: String,
    rows: Vec<Row>,
}

/// A key as the codebook spells it: the letter, or the word.
fn key_word(key: egui::Key) -> String {
    match key {
        egui::Key::Slash => "/".to_owned(),
        egui::Key::Enter => "ENTER".to_owned(),
        egui::Key::Delete => "DELETE".to_owned(),
        egui::Key::Escape => "ESCAPE".to_owned(),
        egui::Key::ArrowLeft => "LEFT".to_owned(),
        egui::Key::ArrowRight => "RIGHT".to_owned(),
        egui::Key::ArrowUp => "UP".to_owned(),
        egui::Key::ArrowDown => "DOWN".to_owned(),
        other => format!("{other:?}").to_ascii_uppercase(),
    }
}

/// Rows that say the same thing share a line when their chords fit it:
/// `DELETE · BACKSPACE  clear slot`, rather than the same words twice.
fn merged(rows: Vec<Row>) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(same) = out.iter_mut().find(|have| have.meaning == row.meaning) {
            let joined = format!("{} · {}", same.chord, row.chord);
            if joined.chars().count() + 2 <= CHORD_CELLS {
                same.chord = joined;
                continue;
            }
        }
        out.push(row);
    }
    out
}

fn leader(painter: &egui::Painter, x0: f32, x1: f32, y: f32, colour: egui::Color32) {
    let mut x = x0;
    while x <= x1 {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(1.0, 1.0)),
            0.0,
            colour,
        );
        x += crate::tune!(LEADER_STEP);
    }
}

impl super::super::Stage {
    /// The codebook's sections for `scope`: the stage's chords by
    /// family, and inside a clip the sequencer's own verbs and the
    /// counted phrases after them.
    fn help_sections(&self, scope: ScopeContext) -> Vec<Section> {
        let entries = keymap::palette_entries();
        let mut by_family: Vec<(&'static str, Vec<Row>)> = Vec::new();
        for (mods, key, intent) in keymap::bindings_for(scope) {
            if let super::super::StageIntent::HeroTool(verb) = intent
                && !self.deck_hero_tools().iter().any(|t| t.verb == verb)
            {
                continue;
            }

            let chord = super::super::carved_chord(&keymap::chord_name(mods, key));
            let entry = entries
                .iter()
                .find(|e| e.scope == scope && e.intent == intent);
            let meaning = entry
                .map(|e| e.command.title.to_owned())
                .unwrap_or_else(|| format!("{intent:?}"));
            let family = entry.map_or("other", |e| e.command.group);
            match by_family.iter_mut().find(|(name, _)| *name == family) {
                Some((_, rows)) => rows.push(Row { chord, meaning }),
                None => by_family.push((family, vec![Row { chord, meaning }])),
            }
        }
        by_family.sort_by_key(|(family, _)| {
            FAMILY_ORDER
                .iter()
                .position(|known| known == family)
                .unwrap_or(FAMILY_ORDER.len())
        });
        let mut sections: Vec<Section> = by_family
            .into_iter()
            .map(|(family, rows)| Section {
                title: family.to_ascii_uppercase(),
                rows: merged(rows),
            })
            .collect();
        if scope == ScopeContext::Clip {
            use crate::ui::sequencer::verbs::{COMMAND_TABLE, SHIFT_TABLE, TABLE, Verb};
            let verbs = |title: &str, prefix: &str, table: &[(Verb, egui::Key, &str)]| Section {
                title: title.to_owned(),
                rows: table
                    .iter()
                    .map(|(verb, key, _)| Row {
                        chord: format!("{prefix}{}", key_word(*key)),
                        meaning: verb.brief().to_owned(),
                    })
                    .collect(),
            };
            sections.push(verbs("SEQUENCER", "", TABLE));
            sections.push(verbs("SEQUENCER · SHIFT", "SHIFT ", SHIFT_TABLE));
            sections.push(verbs("SEQUENCER · CTRL", "CTRL ", COMMAND_TABLE));
            sections.push(Section {
                title: "PHRASES".to_owned(),
                rows: super::super::CLIP_HELP_EXAMPLES
                    .iter()
                    .map(|(chord, meaning)| Row {
                        chord: super::super::carved_chord(chord),
                        meaning: (*meaning).to_owned(),
                    })
                    .collect(),
            });
        }
        sections
    }

    pub(super) fn draw_help(&self, painter: &egui::Painter, field: egui::Rect) {
        if !self.help {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let panel = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin, field.min.y + margin),
            egui::pos2(field.max.x - margin, field.max.y - margin),
        );
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        // The field under the codebook keeps its cursor where it was,
        // but does not show it through the cover.
        crate::ui::nav_cursor::dismiss(painter.ctx());
        let inner = panel.shrink(INSET);

        let scope = self.scope_context();
        let sections = self.help_sections(scope);
        let chords: usize = sections.iter().map(|section| section.rows.len()).sum();

        // The head: what this is, where the hand is, how much there is.
        let hy = inner.min.y + HEAD_H * 0.5 - 2.0;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "CODEBOOK",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(inner.min.x + 10.0 * ch, hy),
            egui::Align2::LEFT_CENTER,
            format!("{scope:?}").to_ascii_uppercase(),
            font.clone(),
            c.fg,
        );
        painter.text(
            egui::pos2(inner.min.x + 22.0 * ch, hy),
            egui::Align2::LEFT_CENTER,
            format!("{chords} chords in {} groups", sections.len()),
            font.clone(),
            c.dim,
        );
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            "? closes",
            font.clone(),
            c.dim,
        );
        let seam_y = (inner.min.y + HEAD_H).round() - 0.5;
        painter.line_segment(
            [
                egui::pos2(inner.min.x, seam_y),
                egui::pos2(inner.max.x, seam_y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );

        // The lines: sections flow down a column and on to the next.
        // A section's name never stands alone at the foot of a column,
        // and a gap is left between sections so the names read as names.
        let row_h = crate::tune!(ROW_H);
        let rows_top = inner.min.y + HEAD_H + row_h * 0.5;
        let per_col = ((inner.max.y - rows_top) / row_h).floor().max(1.0) as usize;
        let col_w = crate::tune!(COL_W);
        let cols = ((inner.width() + INSET) / col_w).floor().max(1.0) as usize;
        let mut col = 0usize;
        let mut line = 0usize;
        let mut hidden = 0usize;
        let mut first = true;
        for section in &sections {
            // The name, its rule, and the section under it: a short
            // section moves whole to the next column rather than
            // leaving a line or two orphaned at the foot of this one.
            let need = (section.rows.len() + 1).min(8);
            if !first && line > 0 {
                line += 1;
            }
            if line + need > per_col {
                col += 1;
                line = 0;
            }
            first = false;
            if col >= cols {
                hidden += section.rows.len();
                continue;
            }
            let x = inner.min.x + col as f32 * col_w;
            let head_y = rows_top + line as f32 * row_h + row_h * 0.5;
            painter.text(
                egui::pos2(x, head_y),
                egui::Align2::LEFT_CENTER,
                &section.title,
                font.clone(),
                c.label,
            );
            let rule_y = (rows_top + (line + 1) as f32 * row_h).round() - 0.5;
            painter.line_segment(
                [egui::pos2(x, rule_y), egui::pos2(x + col_w - INSET, rule_y)],
                egui::Stroke::new(1.0, c.rule),
            );
            line += 1;
            for row in &section.rows {
                if line >= per_col {
                    col += 1;
                    line = 0;
                    if col < cols {
                        // The name again, quieter: the section goes on.
                        let x = inner.min.x + col as f32 * col_w;
                        painter.text(
                            egui::pos2(x, rows_top + row_h * 0.5),
                            egui::Align2::LEFT_CENTER,
                            format!("{} ·", section.title),
                            font.clone(),
                            c.dim,
                        );
                        line = 1;
                    }
                }
                if col >= cols {
                    hidden += 1;
                    continue;
                }
                let x = inner.min.x + col as f32 * col_w;
                let y = rows_top + line as f32 * row_h + row_h * 0.5;
                painter.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    &row.chord,
                    font.clone(),
                    c.bright,
                );
                let leader_x0 = x + (row.chord.chars().count() as f32 + 1.0) * ch;
                let leader_x1 = x + CHORD_CELLS as f32 * ch;
                if leader_x0 <= leader_x1 {
                    leader(painter, leader_x0, leader_x1, y, c.rule);
                }
                // A meaning stops short of the next column rather than
                // running under it.
                let room = ((col_w - INSET * 2.0) / ch).floor() as usize - (CHORD_CELLS + 1);
                let meaning = if row.meaning.chars().count() > room {
                    let cut: String = row.meaning.chars().take(room.saturating_sub(1)).collect();
                    format!("{cut}…")
                } else {
                    row.meaning.clone()
                };
                painter.text(
                    egui::pos2(x + (CHORD_CELLS + 1) as f32 * ch, y),
                    egui::Align2::LEFT_CENTER,
                    meaning,
                    font.clone(),
                    c.fg,
                );
                line += 1;
            }
        }
        if hidden > 0 {
            painter.text(
                egui::pos2(inner.max.x, inner.max.y),
                egui::Align2::RIGHT_BOTTOM,
                format!("{hidden} more"),
                font,
                c.dim,
            );
        }
    }
}
