//! The machine room, drawn and driven: its keys, its header and nav,
//! its rows and footer, the confirmation over it. What the room IS —
//! its pages, fields, values, recents, every action and what it does —
//! is `stage::utility`, and names no toolkit.

use super::*;
use crate::design::kit::Weight;
use crate::design::{self, circuit};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::stage::utility::Page;
use crate::ui::stage::utility::*;

fn key(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|input| input.consume_key(modifiers, key))
}

fn text(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    words: impl ToString,
    size: f32,
    color: egui::Color32,
) {
    painter.text(
        at,
        align,
        words.to_string(),
        egui::FontId::monospace(design::px(size)),
        color,
    );
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

    pub(super) fn draw(&mut self, ui: &mut egui::Ui, stage: &UtilitySnapshot) -> Option<Action> {
        let page = self.page?;
        let whole = ui.max_rect();
        // Utility work is the foreground task, not a dialog floating over
        // the song. The console therefore becomes the entire viewport while
        // it owns input; the musical surface returns intact when it closes.
        let panel = whole;
        let painter = ui
            .painter()
            .with_clip_rect(whole)
            .with_layer_id(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("stage-utility-console"),
            ));
        let alpha = design::Alphabet::for_polarity(design::Polarity::Dark);

        painter.rect_filled(whole, 0.0, egui::Color32::from_black_alpha(224));
        let mut shell = Vec::new();
        circuit::panel_variant(
            &mut shell,
            panel,
            Some(alpha.well.color),
            alpha.ground.color,
            Some((Weight::Heavy, alpha.focus.color)),
            3,
        );
        circuit::panel_frame_variant(
            &mut shell,
            panel.shrink(5.0),
            Weight::Hair,
            alpha.edge.color,
            1,
        );
        painter.extend(shell);

        let inner = panel.shrink(18.0);
        let header =
            egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + HEADER_H));
        let footer = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - FOOTER_H),
            inner.max,
        );
        let nav = egui::Rect::from_min_max(
            egui::pos2(inner.left(), header.bottom() + GAP),
            egui::pos2(inner.left() + NAV_W, footer.top() - GAP),
        );
        let content = egui::Rect::from_min_max(
            egui::pos2(nav.right() + 18.0, nav.top()),
            egui::pos2(inner.right(), nav.bottom()),
        );

        self.paint_header(&painter, header, page, stage, alpha);
        let unlocked = self.confirm.is_none();
        let mut action = self.paint_nav(ui, &painter, nav, page, unlocked, alpha);
        let rows = self.display_rows(stage);
        self.clamp_row(stage);
        action = self
            .paint_rows(ui, &painter, content, &rows, stage, unlocked, alpha)
            .or(action);
        self.paint_footer(&painter, footer, alpha);

        if self.confirm.is_some() {
            action = self.paint_confirm(ui, &painter, panel, alpha).or(action);
        }

        crate::shell::screen::register(&painter, panel, crate::shell::screen::State::new(0.0, 0.0));
        action
    }

    fn paint_header(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        page: Page,
        stage: &UtilitySnapshot,
        alpha: &design::Alphabet,
    ) {
        text(
            painter,
            egui::pos2(rect.left() + 14.0, rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            if self.startup {
                "DAW // PROJECT DECK"
            } else {
                "DAW // UTILITY BUS"
            },
            20.0,
            alpha.focus.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 15.0, rect.top() + 40.0),
            egui::Align2::LEFT_TOP,
            format!("{} · BUILD {}", page.label(), env!("CARGO_PKG_VERSION")),
            11.0,
            alpha.ink.color,
        );
        let project = stage
            .project_path
            .as_deref()
            .map(super::document::title)
            .unwrap_or_else(|| "UNTITLED".to_owned());
        text(
            painter,
            egui::pos2(rect.right() - 12.0, rect.top() + 12.0),
            egui::Align2::RIGHT_TOP,
            format!(
                "{}{}",
                project.to_uppercase(),
                if stage.dirty { " *" } else { "" }
            ),
            13.0,
            if stage.dirty {
                alpha.jeopardy_active.color
            } else {
                alpha.ink.color
            },
        );
        text(
            painter,
            egui::pos2(rect.right() - 12.0, rect.top() + 39.0),
            egui::Align2::RIGHT_TOP,
            format!(
                "{:02} TRACKS · {:03} PATTERNS · {:03} DEVICES",
                stage.track_count, stage.pattern_count, stage.device_count
            ),
            10.0,
            alpha.edge.color,
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.bottom() - 1.0),
                egui::pos2(rect.right(), rect.bottom() - 1.0),
            ],
            egui::Stroke::new(1.0, alpha.edge.color),
        );
    }

    fn paint_nav(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        page: Page,
        interactive: bool,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let mut action = None;
        text(
            painter,
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            "SYS://",
            10.0,
            alpha.edge.color,
        );
        for (index, candidate) in Page::ALL.into_iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 22.0 + index as f32 * 44.0),
                egui::vec2(rect.width(), 36.0),
            );
            let active = candidate == page;
            let mut shapes = Vec::new();
            circuit::relic_frame(
                &mut shapes,
                row,
                if active {
                    alpha.surface.color
                } else {
                    alpha.ground.color
                },
                if active { Weight::Bold } else { Weight::Hair },
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            painter.extend(shapes);
            text(
                painter,
                egui::pos2(row.left() + 12.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                format!("0{}  {}", index + 1, candidate.label()),
                11.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.ink.color
                },
            );
            if interactive
                && ui
                    .interact(
                        row,
                        egui::Id::new(("utility-nav", index)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press)
                    .clicked()
            {
                action = Some(Action::OpenPage(candidate));
            }
        }
        let y = rect.bottom() - 66.0;
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y),
            egui::Align2::LEFT_TOP,
            "SHIFT + ←/→  ROOMS",
            9.0,
            alpha.edge.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y + 18.0),
            egui::Align2::LEFT_TOP,
            "↑/↓  ADDRESS",
            9.0,
            alpha.edge.color,
        );
        text(
            painter,
            egui::pos2(rect.left() + 4.0, y + 36.0),
            egui::Align2::LEFT_TOP,
            "ENTER  EXECUTE",
            9.0,
            alpha.edge.color,
        );
        action
    }

    fn paint_rows(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        rows: &[DisplayRow],
        stage: &UtilitySnapshot,
        interactive: bool,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let visible = ((rect.height() + GAP) / (ROW_H + GAP)).floor().max(1.0) as usize;
        let start = self
            .row
            .saturating_add(1)
            .saturating_sub(visible)
            .min(rows.len().saturating_sub(visible));
        let mut clicked = None;
        for (slot, index) in (start..rows.len()).take(visible).enumerate() {
            let data = &rows[index];
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + slot as f32 * (ROW_H + GAP)),
                egui::vec2(rect.width(), ROW_H),
            );
            let active = index == self.row;
            let fill = if active {
                alpha.surface.color
            } else if index % 2 == 0 {
                alpha.ground.color
            } else {
                alpha.well.color.gamma_multiply(0.72)
            };
            let mut shapes = Vec::new();
            circuit::relic_frame(
                &mut shapes,
                row,
                fill,
                if active { Weight::Bold } else { Weight::Hair },
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            painter.extend(shapes);
            let ink = if !data.enabled {
                alpha.edge.color.gamma_multiply(0.55)
            } else if data.alarm {
                alpha.jeopardy_active.color
            } else if active {
                alpha.focus.color
            } else {
                alpha.ink.color
            };
            text(
                painter,
                egui::pos2(row.left() + 10.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                &data.code,
                9.0,
                alpha.edge.color,
            );
            text(
                painter,
                egui::pos2(row.left() + 50.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                &data.label,
                12.0,
                ink,
            );
            text(
                painter,
                egui::pos2(row.right() - 12.0, row.center().y),
                egui::Align2::RIGHT_CENTER,
                &data.value,
                10.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
            let response = ui
                .interact(
                    row,
                    egui::Id::new(("utility-row", self.page, self.pref_page, index)),
                    egui::Sense::click(),
                )
                .affords(if data.enabled {
                    Affords::Press
                } else {
                    Affords::Refuse
                });
            if interactive && response.clicked() && data.enabled {
                self.row = index;
                clicked = Some(index);
            }
            if active {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("utility-cursor", self.page, self.pref_page, index),
                    row,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Utility,
                    alpha.focus.color,
                );
            }
        }
        clicked.and_then(|_| self.activate(stage))
    }

    fn paint_footer(&self, painter: &egui::Painter, rect: egui::Rect, alpha: &design::Alphabet) {
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0, alpha.edge.color),
        );
        let status = self.status.as_deref().unwrap_or(
            "TAB/↑↓ MOVE · ←→ CHANGE · ENTER ACT · ESC CLOSE · CTRL+O / CTRL+, / CTRL+SHIFT+E",
        );
        text(
            painter,
            egui::pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            status,
            9.0,
            if self.status.is_some() {
                alpha.ink.color
            } else {
                alpha.edge.color
            },
        );
        text(
            painter,
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "ESC // RETURN TO SIGNAL",
            9.0,
            alpha.edge.color,
        );
    }

    fn paint_confirm(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        parent: egui::Rect,
        alpha: &design::Alphabet,
    ) -> Option<Action> {
        let rect = egui::Rect::from_center_size(parent.center(), egui::vec2(520.0, 250.0));
        painter.rect_filled(parent, 0.0, egui::Color32::from_black_alpha(190));
        let mut shapes = Vec::new();
        circuit::panel_variant(
            &mut shapes,
            rect,
            Some(alpha.surface.color),
            alpha.ground.color,
            Some((Weight::Heavy, alpha.jeopardy_active.color)),
            2,
        );
        painter.extend(shapes);
        let question = match self.confirm.as_ref()? {
            Confirm::Replace(_) => "UNSAVED SIGNAL IN MEMORY",
            Confirm::OverwriteProject(path) | Confirm::OverwriteExport(path) => {
                text(
                    painter,
                    egui::pos2(rect.center().x, rect.top() + 52.0),
                    egui::Align2::CENTER_CENTER,
                    path.display().to_string(),
                    9.0,
                    alpha.edge.color,
                );
                "DESTINATION ALREADY EXISTS"
            }
        };
        text(
            painter,
            egui::pos2(rect.center().x, rect.top() + 28.0),
            egui::Align2::CENTER_CENTER,
            question,
            15.0,
            alpha.jeopardy_active.color,
        );
        let labels: [&str; 3] = match self.confirm {
            Some(Confirm::Replace(_)) => ["SAVE + CONTINUE", "DISCARD + CONTINUE", "CANCEL"],
            Some(Confirm::OverwriteProject(_)) | Some(Confirm::OverwriteExport(_)) => {
                ["OVERWRITE", "USE A NEW PATH", "CANCEL"]
            }
            None => return None,
        };
        let mut action = None;
        for (index, label) in labels.into_iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 44.0, rect.top() + 80.0 + index as f32 * 44.0),
                egui::vec2(rect.width() - 88.0, 34.0),
            );
            let active = index == self.confirm_row;
            painter.rect_filled(
                row,
                0.0,
                if active {
                    alpha.well.color
                } else {
                    alpha.ground.color
                },
            );
            painter.rect_stroke(
                row,
                0.0,
                egui::Stroke::new(
                    1.0,
                    if active {
                        alpha.focus.color
                    } else {
                        alpha.edge.color
                    },
                ),
                egui::StrokeKind::Inside,
            );
            text(
                painter,
                row.center(),
                egui::Align2::CENTER_CENTER,
                label,
                11.0,
                if active {
                    alpha.focus.color
                } else {
                    alpha.ink.color
                },
            );
            if ui
                .interact(
                    row,
                    egui::Id::new(("utility-confirm", index)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                self.confirm_row = index;
                action = Some(Action::ResolveConfirm(index));
            }
            if active {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("utility-confirm-cursor", index),
                    row,
                    crate::ui::nav_cursor::Kind::Prompt,
                    crate::ui::nav_cursor::Layer::Utility,
                    alpha.jeopardy_active.color,
                );
            }
        }
        action
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

    pub(super) fn draw_utility(&mut self, ui: &mut egui::Ui) {
        if !self.utility.is_open() {
            return;
        }
        let snapshot = self.utility_snapshot();
        if let Some(action) = self.utility.draw(ui, &snapshot)
            && let Some(text) = self.apply_utility(action)
        {
            ui.ctx().copy_text(text);
        }
    }
}
