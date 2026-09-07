//! STAB in the Stage chain: the chord on a keyboard, on the shared
//! instrument face.
//!
//! A chord synth's one fact that a parameter table cannot show is the
//! chord. The picture is a three-octave keyboard with the chord lit on
//! it, voiced by the same function the voices voice it with, so the
//! picture is what plays; its name is the head's big word.

use super::face::{self, Head, Layout, RowMark};
use super::palette;
use crate::params::stab as sp;
use crate::ui::stage::chain::{self, StabFace};
use eframe::egui;

pub(super) use super::face::WIDTH;

/// The keyboard: three octaves, the played key at the start of the
/// second, so a dropped note has room below and a ninth above.
const OCTAVES: i32 = 3;
const KEY_BASE: i32 = 12;

fn black(semitone: i32) -> bool {
    matches!(semitone.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

fn tone_word(tone: f32) -> &'static str {
    if tone < 0.2 {
        "sine"
    } else if tone < 0.6 {
        "keys"
    } else if tone < 0.85 {
        "organ"
    } else {
        "drawbars"
    }
}

/// The keyboard, three octaves, the chord lit.
fn draw_plot(painter: &egui::Painter, rect: egui::Rect, face: &StabFace) {
    let c = palette::colours();
    let keys = face::frame_plot(painter, rect);
    if keys.width() < 40.0 || keys.height() < 12.0 {
        return;
    }
    let small = face::font(8.5);
    let (notes, count) = face.notes();
    let lit = |semitone: i32| notes[..count].contains(&(semitone - KEY_BASE));
    let whites = (OCTAVES * 7) as f32;
    let white_w = keys.width() / whites;
    let mut white_index = 0;
    for semitone in 0..(OCTAVES * 12) {
        if black(semitone) {
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w - 1.0, keys.bottom()),
        );
        painter.rect_filled(key, 0.0, if lit(semitone) { c.alert } else { c.chassis });
        if semitone.rem_euclid(12) == 0 {
            painter.text(
                egui::pos2(key.center().x, key.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                if semitone == KEY_BASE { "C" } else { "·" },
                small.clone(),
                if lit(semitone) { c.bright } else { c.label },
            );
        }
        white_index += 1;
    }
    let black_h = keys.height() * 0.6;
    let mut white_index = 0;
    for semitone in 0..(OCTAVES * 12) {
        if !black(semitone) {
            white_index += 1;
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w - white_w * 0.3;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w * 0.6, keys.top() + black_h),
        );
        painter.rect_filled(key, 0.0, if lit(semitone) { c.alert } else { c.ground });
        painter.rect_stroke(
            key,
            0.0,
            egui::Stroke::new(1.0, if lit(semitone) { c.bright } else { c.rule }),
            egui::StrokeKind::Inside,
        );
    }
}

impl super::super::Stage {
    /// Draw the STAB face.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_stab_chain_card(
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
        let Some(face) = column.stab.as_ref() else {
            return;
        };
        let Some(layout) = Layout::of(card, head_h, false) else {
            return;
        };
        let painter = ui.painter().clone();
        let alpha = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        face::draw_shell(&painter, card, layout, index, selected, &alpha);
        let p = &face.params;
        let inversion = sp::INVERSION_NAMES
            .get(p.inversion.round().max(0.0) as usize)
            .copied()
            .unwrap_or("root");
        let omit = match p.omit.round().max(0.0) as usize {
            0 => String::new(),
            omit => format!(" · no {}", sp::OMIT_NAMES.get(omit).copied().unwrap_or("?")),
        };
        let head = Head {
            title: "STAB // CHORD ENGINE",
            subtitle: format!(
                "{inversion} · {}{omit} · {} · x{:.1}",
                if p.open.round() >= 1.0 {
                    "open"
                } else {
                    "close"
                },
                tone_word(p.tone),
                p.drive
            ),
            word: face.word(),
            plate: None,
        };
        face::draw_head(
            ui,
            layout,
            column,
            index,
            selected,
            &alpha,
            &head,
            "stage-stab",
        );
        draw_plot(&painter, layout.plot, face);
        let (_, count) = face.notes();
        face::draw_facts(
            &painter,
            layout.facts,
            &[
                ("NOTES", format!("{count} x2 · {:.0}ct", p.detune_ct)),
                (
                    "PLUCK",
                    format!("{:.1}k +{:.1}oct", p.cutoff_hz / 1000.0, p.env_oct),
                ),
                ("GRIT", format!("x{:.1} · {:.0}bit", p.drive, p.crush_bits)),
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
            "stage-stab-param-cursor",
            "PARAM BANK // UP/DN  LEFT/RIGHT",
            &|_| RowMark::default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_names_the_chord_from_the_voicing() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Stab);
        assert_eq!(StabFace::from_device(&device).word(), "Cm7");
        device.set(sp::CHORD, 0.0);
        device.set(sp::INVERSION, 1.0);
        assert_eq!(StabFace::from_device(&device).word(), "Cmaj / E");
        device.set(sp::OPEN, 1.0);
        device.set(sp::INVERSION, 0.0);
        assert_eq!(
            StabFace::from_device(&device).word(),
            "Cmaj / E",
            "drop-two on a triad puts the third under"
        );
        device.set(sp::OPEN, 0.0);
        device.set(sp::OMIT, 1.0);
        assert_eq!(
            StabFace::from_device(&device).word(),
            "Cmaj / E",
            "rootless: the third is the bass"
        );
        assert_eq!(tone_word(0.0), "sine");
        assert_eq!(tone_word(1.0), "drawbars");
    }
}
