//! KIT in the Stage chain: sixteen pads on the shared instrument face.
//!
//! The picture is the pad grid itself, laid out as a drum machine lays
//! it: pad 1 bottom-left, rows climbing. Each cell says its number,
//! the key it answers to, the file on it, its choke group and where it
//! sits in the pan. The pad in hand — where the next browsed file
//! lands — is boxed; the pad whose rows the cursor is on is underlined,
//! so walking the parameter bank walks the grid.

use super::face::{self, Head, Layout, RowMark};
use super::palette;
use crate::params::kit as kp;
use crate::ui::stage::chain::{self, KitFace};
use eframe::egui;
use egui::Color32;

pub(super) use super::face::WIDTH;

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// `C1`, `D#2`: the key a pad answers to, as a keyboard names it.
fn key_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i32::from(pitch) / 12 - 1
    )
}

/// The file's stem, cut to `cells` characters.
fn stem(path: &std::path::Path, cells: usize) -> String {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.chars().take(cells).collect()
}

/// The pad the band's cursor row belongs to, if it is on a pad row.
fn pad_under(cursor_row: Option<usize>) -> Option<usize> {
    kp::pad_of(cursor_row? as u32).map(|(pad, _)| pad)
}

/// The grid.
fn draw_pads(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &KitFace,
    in_hand: usize,
    under: Option<usize>,
) {
    let c = palette::colours();
    let inner = face::frame_plot(painter, rect);
    let small = face::font(8.5);
    let micro = face::font(7.5);
    let gap = 3.0;
    let cols = 4usize;
    let rows = kp::PADS / cols;
    let cw = (inner.width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let ch = (inner.height() - gap * (rows as f32 - 1.0)) / rows as f32;
    if cw < 20.0 || ch < 14.0 {
        return;
    }
    let p = &face.params;
    let solo = p.soloed();
    let cells = ((cw - 6.0) / 5.2).floor().max(3.0) as usize;
    for pad in 0..kp::PADS {
        let col = pad % cols;
        let row = rows - 1 - pad / cols;
        let cell = egui::Rect::from_min_size(
            egui::pos2(
                inner.left() + (cw + gap) * col as f32,
                inner.top() + (ch + gap) * row as f32,
            ),
            egui::vec2(cw, ch),
        );
        let knobs = &p.pads[pad];
        let file = face.file(pad);
        let quiet = !knobs.is_on() || solo.is_some_and(|s| s != pad);
        let fill = if pad == in_hand {
            alpha(c.select, 60)
        } else if file.is_some() {
            alpha(c.edge, 40)
        } else {
            alpha(c.ground, 90)
        };
        painter.rect_filled(cell, 0.0, fill);
        let edge = if pad == in_hand {
            egui::Stroke::new(1.0, c.select)
        } else {
            egui::Stroke::new(1.0, c.rule)
        };
        painter.rect_stroke(cell, 0.0, edge, egui::StrokeKind::Inside);
        let ink = if quiet { c.dim } else { c.fg };
        let head = format!("{:02}", pad + 1);
        painter.text(
            cell.left_top() + egui::vec2(3.0, 2.0),
            egui::Align2::LEFT_TOP,
            head,
            small.clone(),
            if quiet { c.dim } else { c.label },
        );
        painter.text(
            cell.right_top() + egui::vec2(-3.0, 2.0),
            egui::Align2::RIGHT_TOP,
            key_name(p.key_of(pad)),
            micro.clone(),
            c.dim,
        );
        let word = match file {
            Some(path) => stem(path, cells),
            None => "·".to_owned(),
        };
        painter.text(
            egui::pos2(cell.left() + 3.0, cell.center().y + 1.0),
            egui::Align2::LEFT_CENTER,
            word,
            small.clone(),
            ink,
        );
        // The foot: a mute or solo word, the group letter, the pan.
        let foot = cell.bottom() - 2.0;
        let state = if !knobs.is_on() {
            "MUTE"
        } else if solo == Some(pad) {
            "SOLO"
        } else {
            ""
        };
        let group = knobs.choke_group() as usize;
        let mut left = String::new();
        if !state.is_empty() {
            left.push_str(state);
        }
        if group > 0 {
            if !left.is_empty() {
                left.push(' ');
            }
            left.push_str(kp::GROUP_NAMES.get(group).copied().unwrap_or(""));
        }
        painter.text(
            egui::pos2(cell.left() + 3.0, foot),
            egui::Align2::LEFT_BOTTOM,
            left,
            micro.clone(),
            if group > 0 { c.nominal } else { c.dim },
        );
        // The pan as a tick on a short rail, centre marked.
        let rail_w = (cw * 0.28).min(22.0);
        let rail = egui::Rect::from_min_max(
            egui::pos2(cell.right() - 3.0 - rail_w, foot - 4.0),
            egui::pos2(cell.right() - 3.0, foot - 3.0),
        );
        painter.rect_filled(rail, 0.0, c.rule);
        let pan = knobs.pan.clamp(-1.0, 1.0);
        let x = rail.center().x + pan * rail.width() * 0.5;
        painter.line_segment(
            [
                egui::pos2(x, rail.top() - 3.0),
                egui::pos2(x, rail.bottom() + 2.0),
            ],
            egui::Stroke::new(1.0, if pan.abs() > 0.01 { c.fg } else { c.dim }),
        );
        if under == Some(pad) {
            painter.line_segment(
                [
                    egui::pos2(cell.left() + 2.0, cell.bottom() - 0.5),
                    egui::pos2(cell.right() - 2.0, cell.bottom() - 0.5),
                ],
                egui::Stroke::new(2.0, c.nominal),
            );
        }
    }
}

impl super::super::Stage {
    /// Draw the KIT face.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_kit_chain_card(
        &self,
        ui: &mut egui::Ui,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
    ) {
        let Some(face) = column.kit.as_ref() else {
            return;
        };
        let Some(layout) = Layout::of(card, head_h, false) else {
            return;
        };
        let painter = ui.painter().clone();
        let alpha_ = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        face::draw_shell(&painter, card, layout, index, selected, &alpha_);
        let p = &face.params;
        let in_hand = p.pad_in_hand();
        let under = pad_under(cursor.filter(|(col, _)| *col == index).map(|(_, row)| row));
        let groups = (1..=kp::GROUPS as usize)
            .filter(|g| p.pads.iter().any(|pad| pad.choke_group() as usize == *g))
            .count();
        let head = Head {
            title: "KIT // 16 PADS",
            subtitle: format!(
                "{}/16 loaded · hand on pad {} · {}",
                face.loaded(),
                in_hand + 1,
                match p.soloed() {
                    Some(pad) => format!("solo pad {}", pad + 1),
                    None => format!("{groups} choke groups"),
                }
            ),
            word: format!(
                "{}–{}",
                key_name(p.key_of(0)),
                key_name(p.key_of(kp::PADS - 1))
            ),
            plate: None,
        };
        face::draw_head(
            ui,
            layout,
            column,
            index,
            selected,
            &alpha_,
            &head,
            "stage-kit",
        );
        draw_pads(&painter, layout.plot, face, in_hand, under);
        face::draw_facts(
            &painter,
            layout.facts,
            &[
                (
                    "BASE / TUNE",
                    format!("{} {:+.0}st", key_name(p.base_note()), p.tune),
                ),
                (
                    "TIGHT / LEVEL",
                    format!("x{:.2} {:.0}%", p.tight, p.level * 100.0),
                ),
                (
                    "PAGE",
                    match under {
                        Some(pad) => format!("pad {} · pgup/dn", pad + 1),
                        None => "kit · pgup/dn".to_owned(),
                    },
                ),
            ],
        );
        face::draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
            "stage-kit-param-cursor",
            "PARAM BANK // PGUP/PGDN: PAD",
            &|_| RowMark::default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_carries_the_pads_and_names_keys_as_a_keyboard_does() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Kit);
        device.pads = vec![
            "/k/kick.wav".into(),
            std::path::PathBuf::new(),
            "/k/hat.wav".into(),
        ];
        device.set(kp::BASE, 48.0);
        device.set(kp::pad_param(2, kp::GROUP), 1.0);
        let face = KitFace::from_device(&device);
        assert_eq!(face.loaded(), 2);
        assert!(face.file(1).is_none() && face.file(2).is_some());
        assert_eq!(key_name(face.params.key_of(0)), "C3");
        assert_eq!(key_name(face.params.key_of(15)), "D#4");
        assert_eq!(face.params.pads[2].choke_group(), 1);
        assert_eq!(pad_under(Some(kp::pad_param(5, kp::PAN) as usize)), Some(5));
        assert_eq!(pad_under(Some(kp::TUNE as usize)), None);
        assert_eq!(stem(std::path::Path::new("/k/hat-open.wav"), 5), "hat-o");
    }
}
