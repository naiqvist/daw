//! The narrow right-side tool rail's visual shell.

use crate::ui::redesign::grammar::Sentence;
use crate::ui::redesign::registers::Registers;
use crate::ui::redesign::{OUTLINE, SURFACE_UTILITY, focus};
use crate::ui::tokens::{control, font, radius, space};
use eframe::egui;

/// One compact tool target plus the horizontal room needed to reach it.
const WIDTH: f32 = control::KNOB_MINI + space::XL;

const MUTED: egui::Color32 = egui::Color32::from_gray(96);

/// The right-side tool rail. Mostly reserved space still — but it shows
/// two of the grammar's pronouns (the composition contract's visibility
/// rule): the MATERIAL context (what the register carries, so a put is
/// never a surprise) and the SENTENCE in progress, visible from every
/// panel — a count begun before a focus change is never lost thought.
pub(crate) fn show(ui: &mut egui::Ui, focused: bool, registers: &Registers, sentence: &Sentence) {
    egui::Panel::right("redesign-tools")
        .resizable(false)
        .show_separator_line(false)
        .exact_size(WIDTH)
        .frame(
            egui::Frame::new()
                .fill(SURFACE_UTILITY)
                .corner_radius(radius::PANEL)
                .stroke(egui::Stroke::NONE),
        )
        .show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.take_available_space();
            let center_x = rect.center().x;
            ui.painter().text(
                egui::pos2(center_x, rect.top() + space::LG),
                egui::Align2::CENTER_TOP,
                "REG",
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                MUTED,
            );
            match registers.carried_sign() {
                Some(sign) => {
                    ui.painter().text(
                        egui::pos2(center_x, rect.top() + space::LG + 14.0),
                        egui::Align2::CENTER_TOP,
                        sign,
                        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                        OUTLINE,
                    );
                }
                None => {
                    // An empty hand is a fact worth one quiet mark.
                    ui.painter().text(
                        egui::pos2(center_x, rect.top() + space::LG + 14.0),
                        egui::Align2::CENTER_TOP,
                        "\u{00b7}",
                        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                        MUTED,
                    );
                }
            }
            if !sentence.is_empty() {
                ui.painter().text(
                    egui::pos2(center_x, rect.top() + space::LG + 34.0),
                    egui::Align2::CENTER_TOP,
                    sentence.display(),
                    egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                    OUTLINE,
                );
            }
            focus::show(ui.painter(), rect, focused);
        });
}
