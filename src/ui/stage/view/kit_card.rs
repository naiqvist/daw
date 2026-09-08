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

/// Wider than the shared face: sixteen cells and a picture of the pad
/// in hand both need their room.
pub(super) const WIDTH: f32 = super::face::WIDTH + 150.0;

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

/// The pad's file, above the bank: the one the cursor is on, else the
/// one in hand. The START mark and the hit's shape sit over it, as on
/// the brick's own face.
fn draw_wave(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &KitFace,
    pad: usize,
    data: Option<&crate::ui::stage::SampleData>,
) {
    let c = palette::colours();
    let small = face::font(8.5);
    let micro = face::font(7.5);
    let label_h = 11.0;
    let knobs = &face.params.pads[pad];
    let file = face.file(pad);
    let name = file
        .map(|path| stem(path, 18))
        .unwrap_or_else(|| "no file".to_owned());
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.top()),
        egui::Align2::LEFT_TOP,
        format!("PAD {:02} · {}", pad + 1, key_name(face.params.key_of(pad))),
        micro.clone(),
        c.label,
    );
    painter.text(
        egui::pos2(rect.right() - 4.0, rect.top()),
        egui::Align2::RIGHT_TOP,
        match data {
            Some(data) if data.frames > 0 => format!("{name} · {:.2}s", data.seconds()),
            _ => name,
        },
        micro,
        c.dim,
    );
    let frame = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + label_h), rect.max);
    let inner = face::frame_plot(painter, frame);
    let mid = inner.center().y;
    let half = inner.height() * 0.5 - 1.0;
    painter.line_segment(
        [
            egui::pos2(inner.left(), mid.round() - 0.5),
            egui::pos2(inner.right(), mid.round() - 0.5),
        ],
        egui::Stroke::new(1.0, c.rule),
    );
    let Some(data) = data.filter(|data| data.frames > 0) else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            if file.is_some() {
                "loading"
            } else {
                "browse a hit onto the pad"
            },
            small,
            c.dim,
        );
        return;
    };
    let columns = inner.width().floor().max(1.0) as usize;
    let bins = data.peaks.columns(None, 0.0, 1.0, columns);
    for (i, bin) in bins.iter().enumerate() {
        let x = inner.min.x + i as f32 + 0.5;
        painter.line_segment(
            [
                egui::pos2(x, mid - bin.max.clamp(-1.0, 1.0) * half),
                egui::pos2(x, mid - bin.min.clamp(-1.0, 1.0) * half),
            ],
            egui::Stroke::new(1.0, c.edge),
        );
    }
    let b = &knobs.brick;
    let start = f64::from(b.start.clamp(0.0, 1.0));
    let px = |at: f64| inner.min.x + inner.width() * at as f32;
    if b.reversed() {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(px(1.0 - start), inner.min.y), inner.max),
            0.0,
            alpha(c.ground, 140),
        );
    } else if start > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(inner.min, egui::pos2(px(start), inner.max.y)),
            0.0,
            alpha(c.ground, 140),
        );
    }
    let sx = px(if b.reversed() { 1.0 - start } else { start }).round() - 0.5;
    painter.line_segment(
        [egui::pos2(sx, inner.min.y), egui::pos2(sx, inner.max.y)],
        egui::Stroke::new(1.0, c.alert),
    );
    let seconds = data.seconds().max(0.01) as f32;
    let decay = b.decay * face.params.tight;
    let levels = crate::ui::device::brick::hit_shape(
        b.attack,
        decay,
        b.curve,
        b.punch,
        seconds,
        columns.max(2),
    );
    let peak = levels.iter().cloned().fold(0.0f32, f32::max).max(1.0e-3);
    let points: Vec<egui::Pos2> = levels
        .iter()
        .enumerate()
        .map(|(i, level)| {
            let x = if b.reversed() {
                inner.max.x - inner.width() * i as f32 / columns.max(1) as f32
            } else {
                inner.min.x + inner.width() * i as f32 / columns.max(1) as f32
            };
            egui::pos2(
                x,
                inner.max.y - inner.height() * (level / peak).clamp(0.0, 1.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, c.nominal)));
}

/// The grid.
fn draw_pads(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &KitFace,
    in_hand: usize,
    under: Option<usize>,
    lit: u32,
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
        let sounding = lit & (1 << pad) != 0;
        let fill = if sounding {
            alpha(c.nominal, 70)
        } else if pad == in_hand {
            alpha(c.select, 60)
        } else if file.is_some() {
            alpha(c.edge, 40)
        } else {
            alpha(c.ground, 90)
        };
        painter.rect_filled(cell, 0.0, fill);
        let edge = if sounding {
            egui::Stroke::new(1.0, c.nominal)
        } else if pad == in_hand {
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
        let name_at = egui::pos2(cell.left() + 3.0, cell.center().y + 1.0);
        let galley = painter.layout_no_wrap(word, small.clone(), ink);
        let name_rect = egui::Align2::LEFT_CENTER.anchor_size(name_at, galley.size());
        painter.rect_filled(
            name_rect.expand2(egui::vec2(1.0, 0.0)),
            0.0,
            alpha(c.panel, 210),
        );
        painter.galley(name_rect.min, galley, ink);
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
        // The engine's word on which pads sound, when it has one.
        let lit = self
            .readout(column.id)
            .map_or(0, |said| said.bands[1].max(0.0).min(u32::MAX as f32) as u32);
        draw_pads(&painter, layout.plot, face, in_hand, under, lit);
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
        // The picture takes the top of the rail; the bank keeps the rest.
        let shown = under.unwrap_or(in_hand);
        let wave_h = (layout.params.height() * 0.42).clamp(48.0, 96.0);
        let wave =
            egui::Rect::from_min_size(layout.params.min, egui::vec2(layout.params.width(), wave_h));
        let bank = egui::Rect::from_min_max(
            egui::pos2(layout.params.left(), wave.bottom() + 6.0),
            layout.params.max,
        );
        draw_wave(
            &painter,
            wave,
            face,
            shown,
            face.file(shown).and_then(|path| self.thumb(path)),
        );
        face::draw_params(
            &painter,
            bank,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
            "stage-kit-param-cursor",
            "BANK // PGDN PAD · P HEAR · Q E X DEL",
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
