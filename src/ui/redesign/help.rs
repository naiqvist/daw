//! The grammar's reference card, summoned with `?`.
//!
//! One overlay listing every sentence the keyboard speaks. The verb rows
//! are GENERATED from `verbs::TABLE` — the same table the bijection tests
//! guard — so the help can never drift from the truth. Everything else on
//! the card names a binding that lives in exactly one other place
//! (keyboard.rs travel, midi_typing, the control plane's transport keys);
//! when one of those moves, its line here moves with it.

use crate::ui::redesign::verbs::{self, Verb};
use crate::ui::redesign::{OUTLINE, SURFACE_FRAME};
use crate::ui::tokens::{font, space};
use eframe::egui;

const HEADING: egui::Color32 = egui::Color32::from_gray(140);
const KEY_INK: egui::Color32 = egui::Color32::WHITE;
const TEXT: egui::Color32 = egui::Color32::from_gray(190);
const SCRIM: egui::Color32 = egui::Color32::from_black_alpha(160);

/// The keycap's short name, as the card prints it.
fn key_label(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Enter => "ENTER",
        egui::Key::Delete => "DEL",
        egui::Key::Slash => "/",
        egui::Key::F2 => "F2",
        egui::Key::Q => "Q",
        egui::Key::W => "W",
        egui::Key::E => "E",
        egui::Key::D => "D",
        egui::Key::R => "R",
        egui::Key::M => "M",
        egui::Key::S => "S",
        egui::Key::C => "C",
        _ => "?",
    }
}

/// What each verb does, in the card's few words.
fn verb_hint(verb: Verb) -> &'static str {
    match verb {
        Verb::Act => "the noun's primary act: trig toggles, clip opens, device bypasses",
        Verb::Delete => "remove the noun under the cursor",
        Verb::Yank => "copy into the register",
        Verb::Put => "place the register's content here",
        Verb::Duplicate => "copy N steps ahead, cursor rides along",
        Verb::Nudge => "then an arrow: move by grid unit",
        Verb::Resize => "then ◄ ►: grow or shrink",
        Verb::Mute => "the noun falls silent but remains",
        Verb::Solo => "the noun alone speaks",
        Verb::Rename => "type a new name, ENTER commits",
        Verb::Condition => "cycle chance; 50 C names a percent",
        Verb::Search => "find by name in this panel's world",
    }
}

pub(crate) fn show(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    egui::Area::new(egui::Id::new("redesign-help"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::Pos2::ZERO)
        .show(ctx, |ui| {
            let screen = ctx.content_rect();
            ui.painter().rect_filled(screen, 0.0, SCRIM);
            let card_width = 640.0_f32.min(screen.width() - 2.0 * space::LG);
            let card = egui::Rect::from_center_size(
                screen.center(),
                egui::vec2(card_width, (screen.height() - 2.0 * space::LG).min(660.0)),
            );
            ui.painter().rect_filled(card, 0.0, SURFACE_FRAME);
            ui.painter()
                .rect_stroke(card, 0.0, (1.0, OUTLINE), egui::StrokeKind::Inside);

            let mono = |size| egui::FontId::new(size, egui::FontFamily::Monospace);
            let mut y = card.top() + space::LG;
            let left = card.left() + space::LG;
            let key_column = left + 110.0;
            let heading = |ui: &egui::Ui, y: &mut f32, text: &str| {
                ui.painter().text(
                    egui::pos2(left, *y),
                    egui::Align2::LEFT_TOP,
                    text,
                    mono(font::MINI_LABEL),
                    HEADING,
                );
                *y += 18.0;
            };
            let row = |ui: &egui::Ui, y: &mut f32, keys: &str, what: &str| {
                ui.painter().text(
                    egui::pos2(left, *y),
                    egui::Align2::LEFT_TOP,
                    keys,
                    mono(font::LABEL),
                    KEY_INK,
                );
                ui.painter().text(
                    egui::pos2(key_column, *y),
                    egui::Align2::LEFT_TOP,
                    what,
                    mono(font::LABEL),
                    TEXT,
                );
                *y += 17.0;
            };

            heading(ui, &mut y, "THE SENTENCE   [count] [hold] VERB [motion]");
            row(ui, &mut y, "4 W \u{2192}", "nudge four grid units right");
            row(ui, &mut y, "50 C", "this trig fires half the time");
            row(ui, &mut y, "ESC", "abandon the sentence in progress");
            y += space::SM;

            heading(ui, &mut y, "VERBS   one key, one meaning, everywhere");
            for (verb, key, name) in verbs::TABLE {
                row(
                    ui,
                    &mut y,
                    &format!("{} {}", key_label(*key), name),
                    verb_hint(*verb),
                );
            }
            y += space::SM;

            heading(ui, &mut y, "HOLDS   a held key recolours the sentence");
            row(
                ui,
                &mut y,
                "\u{21e7} \u{2191}\u{2193}",
                "sequencer: edit the trig under the cursor (velocity)",
            );
            row(
                ui,
                &mut y,
                "\u{21e7} arrows",
                "arrangement: extend the selection",
            );
            y += space::SM;

            heading(ui, &mut y, "ATTENTION   the layout is the keybinding");
            row(
                ui,
                &mut y,
                "CTRL \u{2190}\u{2191}\u{2192}\u{2193}",
                "move focus to the panel in that direction",
            );
            row(
                ui,
                &mut y,
                "CTRL \u{2193} \u{2193}",
                "in the strip: flip sequencer \u{2194} chain",
            );
            row(ui, &mut y, "TAB", "walk the panel ring (the fallback)");
            row(
                ui,
                &mut y,
                "CTRL 4",
                "time detail: step grid \u{2194} piano roll (per pattern)",
            );
            row(
                ui,
                &mut y,
                "F10",
                "center: legacy timeline \u{2194} SONG arrangement",
            );
            y += space::SM;

            heading(ui, &mut y, "MODES   every mode announces itself");
            row(
                ui,
                &mut y,
                "I",
                "MIDI: letters are a piano or key degrees, Z X shift octave / period",
            );
            row(ui, &mut y, "ESC", "leave MIDI / any text field");
            row(ui, &mut y, ":", "the palette: long sentences, typed");
            y += space::SM;

            heading(ui, &mut y, "SONG WORLD");
            row(
                ui,
                &mut y,
                "CTRL Z \u{00b7} \u{21e7}Z",
                "undo \u{00b7} redo (every sentence reversible)",
            );
            row(
                ui,
                &mut y,
                "CTRL SPACE \u{00b7} L",
                "play the selection \u{00b7} loop it",
            );
            row(
                ui,
                &mut y,
                "CTRL T \u{00b7} B",
                "add a track \u{00b7} tap the tempo",
            );
            y += space::SM;

            heading(ui, &mut y, "TRANSPORT");
            row(
                ui,
                &mut y,
                "SPACE",
                "play / pause \u{00b7} \u{21e7}SPACE continue \u{00b7} HOME return",
            );
            row(ui, &mut y, "O  F9", "metronome \u{00b7} record arm");

            ui.painter().text(
                egui::pos2(left, card.bottom() - space::LG),
                egui::Align2::LEFT_BOTTOM,
                "? OR ESC CLOSES",
                mono(font::MINI_LABEL),
                HEADING,
            );
        });
}
