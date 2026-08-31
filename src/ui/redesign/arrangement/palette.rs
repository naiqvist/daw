//! Sharp-cornered, keyboard-owned arrangement command palette.

use super::edit::Command;
use super::state::{PaletteState, Selection};
use crate::sequencing::Song;
use crate::ui::redesign::OUTLINE;
use crate::ui::tokens::{font, space, stroke};
use eframe::egui;

const WIDTH: f32 = 430.0;
const ROW_H: f32 = 34.0;
const INK_MUTED: egui::Color32 = egui::Color32::from_gray(110);
const SURFACE: egui::Color32 = egui::Color32::from_gray(10);
const ROW: egui::Color32 = egui::Color32::from_gray(18);

pub(super) fn show(
    ctx: &egui::Context,
    state: &mut PaletteState,
    song: &Song,
    selection: Selection,
) -> Option<Command> {
    if !state.open {
        return None;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.close();
        return None;
    }

    let matches: Vec<Command> = Command::ALL
        .into_iter()
        .filter(|command| fuzzy_match(&state.query, command.title()))
        .collect();
    state.cursor = state.cursor.min(matches.len().saturating_sub(1));
    ctx.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) && !matches.is_empty() {
            state.cursor = (state.cursor + 1) % matches.len();
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) && !matches.is_empty() {
            state.cursor = state.cursor.checked_sub(1).unwrap_or(matches.len() - 1);
        }
    });
    let accept = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let mut chosen = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("redesign_arrangement_palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen.center().x - WIDTH * 0.5,
            screen.top() + 72.0,
        ))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::BLACK)
                .stroke(egui::Stroke::new(stroke::FOCUS, OUTLINE))
                .inner_margin(egui::Margin::same(space::SM as i8))
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.painter()
                        .rect_filled(ui.available_rect_before_wrap(), 0.0, SURFACE);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text("ARRANGEMENT COMMAND")
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .frame(egui::Frame::NONE.fill(SURFACE)),
                    );
                    if state.just_opened {
                        field.request_focus();
                        state.just_opened = false;
                    }
                    ui.add_space(space::XS);
                    for (index, command) in matches.iter().copied().enumerate() {
                        let enabled = command.enabled(song, selection);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_H),
                            egui::Sense::click(),
                        );
                        if index == state.cursor {
                            ui.painter().rect_filled(rect, 0.0, ROW);
                            cursor(ui.painter(), rect);
                        }
                        let ink = if enabled { OUTLINE } else { INK_MUTED };
                        ui.painter().text(
                            rect.left_center() + egui::vec2(space::SM, 0.0),
                            egui::Align2::LEFT_CENTER,
                            command.title(),
                            egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                            ink,
                        );
                        ui.painter().text(
                            rect.right_center() - egui::vec2(space::SM, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            command.hint(),
                            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                            INK_MUTED,
                        );
                        if response.clicked() && enabled {
                            chosen = Some(command);
                        }
                    }
                });
        });
    if chosen.is_none() && accept {
        chosen = matches
            .get(state.cursor)
            .copied()
            .filter(|command| command.enabled(song, selection));
    }
    if chosen.is_some() {
        state.close();
    }
    chosen
}

fn fuzzy_match(query: &str, text: &str) -> bool {
    let mut text = text.chars().map(|character| character.to_ascii_lowercase());
    query
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| character.to_ascii_lowercase())
        .all(|needle| text.by_ref().any(|candidate| candidate == needle))
}

fn cursor(painter: &egui::Painter, rect: egui::Rect) {
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        ">",
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        OUTLINE,
    );
}
