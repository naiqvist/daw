//! The machine room's keys. What the room IS — its pages, fields,
//! values, every action — is `stage::utility`; what it LOOKS like is not
//! yet decided, and nothing here paints. Its keys are kept because a
//! room that owns the keyboard must be able to give it back.

use super::*;
use crate::ui::stage::utility::Page;
use crate::ui::stage::utility::*;

fn key(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|input| input.consume_key(modifiers, key))
}

impl Console {
    pub(super) fn consume_input(
        &mut self,
        ctx: &egui::Context,
        stage: &UtilitySnapshot,
    ) -> Option<Action> {
        if self.page.is_none() {
            return None;
        }

        if self.confirm.is_some() {
            let moved_back = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowUp)
                || key(ctx, egui::Modifiers::NONE, egui::Key::K)
                || key(ctx, egui::Modifiers::SHIFT, egui::Key::Tab);
            let moved = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowDown)
                || key(ctx, egui::Modifiers::NONE, egui::Key::J)
                || key(ctx, egui::Modifiers::NONE, egui::Key::Tab);
            if moved {
                self.confirm_row = (self.confirm_row + 1) % 3;
            }
            if moved_back {
                self.confirm_row = (self.confirm_row + 2) % 3;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
                self.confirm = None;
                return None;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
                return Some(Action::ResolveConfirm(self.confirm_row));
            }
            return None;
        }

        // The four doors stay global even from inside the machine room. A
        // nested overwrite/dirty decision above is the sole exception: it
        // must be answered or cancelled before navigation can continue.
        let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        if key(ctx, egui::Modifiers::COMMAND, egui::Key::O) {
            return Some(Action::OpenPage(Page::Projects));
        }
        if key(ctx, egui::Modifiers::COMMAND, egui::Key::Comma) {
            return Some(Action::OpenPage(Page::Preferences));
        }
        if key(ctx, command_shift, egui::Key::E) {
            return Some(Action::OpenPage(Page::Export));
        }
        if key(ctx, command_shift, egui::Key::D) {
            return Some(Action::OpenPage(Page::Diagnostics));
        }
        if self.editing.is_none() && key(ctx, egui::Modifiers::COMMAND, egui::Key::S) {
            return Some(Action::Save);
        }

        if let Some(field) = self.editing {
            if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
                self.editing = None;
                return None;
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Backspace) {
                self.field_mut(field).pop();
            }
            if key(ctx, egui::Modifiers::COMMAND, egui::Key::A) {
                self.field_mut(field).clear();
            }
            let additions: Vec<String> = ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Text(text) | egui::Event::Paste(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect()
            });
            for text in additions {
                self.field_mut(field)
                    .extend(text.chars().filter(|ch| !ch.is_control()));
            }
            if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
                self.editing = None;
                return match field {
                    Field::ProjectFolder => nonblank(&self.project_folder)
                        .map(PathBuf::from)
                        .map(Action::SetProjectFolder),
                    Field::UserLibrary => nonblank(&self.user_library)
                        .map(PathBuf::from)
                        .map(Action::SetUserLibrary),
                    Field::SampleFolder => nonblank(&self.sample_folder)
                        .map(PathBuf::from)
                        .map(Action::AddSampleFolder),
                    Field::ProjectPath | Field::ExportPath => None,
                };
            }
            ctx.request_repaint();
            return None;
        }

        if key(ctx, egui::Modifiers::NONE, egui::Key::Escape) {
            if stage.exporting && self.page == Some(Page::Export) {
                return Some(Action::CancelExport);
            }
            self.close();
            return None;
        }

        // Shift-left/right walks the four utility rooms. Unshifted motion
        // belongs to the value on the current row.
        if key(ctx, egui::Modifiers::SHIFT, egui::Key::ArrowLeft) {
            self.cycle_page(-1);
            self.clamp_row(stage);
            return None;
        }
        if key(ctx, egui::Modifiers::SHIFT, egui::Key::ArrowRight) {
            self.cycle_page(1);
            self.clamp_row(stage);
            return None;
        }

        let up = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowUp)
            || key(ctx, egui::Modifiers::NONE, egui::Key::K)
            || key(ctx, egui::Modifiers::SHIFT, egui::Key::Tab);
        let down = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowDown)
            || key(ctx, egui::Modifiers::NONE, egui::Key::J)
            || key(ctx, egui::Modifiers::NONE, egui::Key::Tab);
        let rows = self.rows(stage).max(1);
        if down {
            self.row = (self.row + 1) % rows;
            return None;
        }
        if up {
            self.row = (self.row + rows - 1) % rows;
            return None;
        }
        let right = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowRight)
            || key(ctx, egui::Modifiers::NONE, egui::Key::L);
        let left = key(ctx, egui::Modifiers::NONE, egui::Key::ArrowLeft)
            || key(ctx, egui::Modifiers::NONE, egui::Key::H);
        if right || left {
            return self.adjust(if right { 1 } else { -1 }, stage);
        }
        if key(ctx, egui::Modifiers::NONE, egui::Key::Enter) {
            return self.activate(stage);
        }
        None
    }
}

impl Stage {
    pub(super) fn update_utility(&mut self, ctx: &egui::Context) {
        if !self.utility.is_open() {
            return;
        }
        let snapshot = self.utility_snapshot();
        if let Some(action) = self.utility.consume_input(ctx, &snapshot)
            && let Some(text) = self.apply_utility(action)
        {
            ctx.copy_text(text);
        }
        // Interface preferences preview immediately. Audio remains an
        // explicit restart because changing a live device is not reversible
        // by merely closing the modal.
        self.polarity = if self.utility.prefs().light_ground {
            design::Polarity::Light
        } else {
            design::Polarity::Dark
        };
    }
}
