//! The always-present device chain as a horizontally panning row of cards.
//!
//! Every device is a display unit and owns its parameter cells. Parameters
//! keep table order in four columns, so row `n` always lands at `n % 4` and
//! the flattened Left/Right address walk crosses card boundaries unchanged.
//! A count remains the fast route through that continuous address space.
//!
//! This module owns view state and emits intents. Device values arrive as a
//! green-zone snapshot and every mutation returns to the application layer.

use crate::ui::affordance::{Afford, Affords};
use crate::ui::redesign::focus;
use crate::ui::redesign::grammar::{Motion, Utterance, Voice};
use crate::ui::redesign::verbs::Verb;
use crate::ui::tokens::{font, space, stroke};
use eframe::egui;

const COLUMNS: usize = 4;
const HEADER_H: f32 = 28.0;
const STATUS_H: f32 = 22.0;
const GAP: f32 = 2.0;
const CARD_GAP: f32 = 6.0;
const CARD_PAD: f32 = 4.0;
const CELL_SIDE: f32 = 54.0;
const CELL_MIN_SIDE: f32 = 38.0;
const HERO_H: f32 = 38.0;
const PAN_MARGIN: f32 = 12.0;
const COARSE_FRACTION: f32 = 0.025;
const CHOOSER_W: f32 = 420.0;
const CHOOSER_FIELD_H: f32 = 32.0;
const CHOOSER_ROW_H: f32 = 26.0;
const CHOOSER_ROWS: usize = 7;

const VOID: egui::Color32 = egui::Color32::BLACK;
const BAND: egui::Color32 = egui::Color32::from_gray(12);
const CELL: egui::Color32 = egui::Color32::from_gray(18);
const HOVER: egui::Color32 = egui::Color32::from_gray(23);
const SELECTED: egui::Color32 = egui::Color32::from_gray(27);
const QUIET: egui::Color32 = egui::Color32::from_gray(58);
const MUTED: egui::Color32 = egui::Color32::from_gray(104);
const CURSOR: egui::Color32 = egui::Color32::from_gray(178);
const TEXT: egui::Color32 = egui::Color32::from_gray(216);

/// A signature display that can be reconstructed exactly from the snapshot.
/// Live scopes and meters deliberately have no variant here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeroKind {
    #[default]
    None,
    Kick,
    Filter,
    Saturator,
}

/// The optional lock side of the offset model. The application currently
/// supplies this only when a trig context exists; absence means the page is
/// showing and editing base. Keeping all three faces makes `base + lock =
/// effective` explicit rather than flattening the two authorities together.
#[derive(Clone, Debug, PartialEq)]
pub struct LockView {
    pub offset: f32,
    pub formatted_offset: String,
    pub formatted_effective: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamView {
    pub id: u32,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub base: f32,
    /// Zero means continuous; otherwise one Up/Down is one whole choice.
    pub choices: u32,
    /// The owning device's formatter already applied to `base`.
    pub formatted: String,
    pub lock: Option<LockView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceView {
    pub id: u64,
    pub name: String,
    pub bypassed: bool,
    pub instrument: bool,
    /// Rack siblings may reorder only inside the same parent.
    pub parent: Option<u64>,
    pub hero: HeroKind,
    pub params: Vec<ParamView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogueItem {
    pub name: String,
    pub is_instrument: bool,
}

/// The reserved device id of the TRACK HEAD.
///
/// The head is drawn as a device because, to the hand, it IS one: fixed
/// slots, the same cursor, the same travel. The vision promises muscle
/// memory transfers across every device forever, and the fader is the
/// control a musician touches most — a bespoke widget there would break
/// that promise at the worst possible place. Making it a DeviceView also
/// means the whole chain surface renders and edits it with no new code.
pub const TRACK_HEAD_ID: u64 = u64::MAX;

/// Slot ids on the track head. They never collide with device parameter
/// ids because the head is not backed by a `ParamDef` table at all.
pub const TRACK_LEVEL_PARAM: u32 = 0;
pub const TRACK_PAN_PARAM: u32 = 1;
pub const TRACK_SEND_PARAM_START: u32 = 2;

pub fn track_send_param(index: usize) -> Option<u32> {
    (index < crate::sequencing::ReturnTrack::MAX).then(|| TRACK_SEND_PARAM_START + index as u32)
}

pub fn track_send_index(param: u32) -> Option<usize> {
    let index = param.checked_sub(TRACK_SEND_PARAM_START)? as usize;
    (index < crate::sequencing::ReturnTrack::MAX).then_some(index)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub track_name: Option<String>,
    pub devices: Vec<DeviceView>,
    pub catalogue: Vec<CatalogueItem>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    SetParam {
        device: u64,
        param: u32,
        value: f32,
    },
    ToggleBypass {
        device: u64,
    },
    /// Move `device` onto `target`; the shared rack helper decides which
    /// side from their direction in the canonical chain.
    Reorder {
        device: u64,
        target: u64,
    },
    AddDevice {
        catalogue_index: usize,
    },
}

#[derive(Default)]
pub struct Outcome {
    pub intents: Vec<Intent>,
    pub(crate) claim_focus: bool,
}

#[derive(Default)]
struct DeviceChooser {
    open: bool,
    query: String,
    cursor: usize,
    just_opened: bool,
}

impl DeviceChooser {
    fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = 0;
        self.just_opened = true;
    }

    fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = 0;
        self.just_opened = false;
    }
}

#[derive(Clone, Copy)]
enum ChooserCommand {
    Up,
    Down,
    Accept,
    Cancel,
}

#[derive(Default)]
pub struct ChainPanel {
    selected_device: Option<u64>,
    selected_param: usize,
    refusal: Option<String>,
    chooser: DeviceChooser,
    scroll_x: f32,
}

impl ChainPanel {
    /// Paint into the shared detail strip; the strip itself is owned by
    /// the frame (`mod.rs`), so the chain and the sequencer trade one
    /// address instead of stacking two starved panels.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        voice: &mut Voice<'_>,
        view: &View,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        outcome.claim_focus = ui.ctx().input(|input| input.pointer.any_pressed())
            && ui
                .ctx()
                .pointer_latest_pos()
                .is_some_and(|pointer| area.contains(pointer));

        self.sync(view);
        if focused
            && !self.chooser.open
            && let Some(utterance) = voice.sentence.consume(ui.ctx())
        {
            self.speak(view, utterance, &mut outcome);
        }

        let status_top = (area.bottom() - STATUS_H).max(area.top());
        let status =
            egui::Rect::from_min_max(egui::pos2(area.left(), status_top), area.right_bottom());
        let cards = egui::Rect::from_min_max(area.min, egui::pos2(area.right(), status.top()));

        self.draw_cards(ui, cards, view, &mut outcome);
        let sentence_display = (!voice.sentence.is_empty()).then(|| voice.sentence.display());
        let overlay = if self.chooser.open {
            Some("TYPING / ENTER ADD / ESC CLOSE")
        } else {
            sentence_display.as_deref().or(self.refusal.as_deref())
        };
        self.draw_status(ui, status, view, overlay);
        if self.chooser.open {
            self.draw_chooser(ui, area, &view.catalogue, &mut outcome);
        }

        // The focus bar/scrim covers content, so this is deliberately
        // the final paint operation in the panel.
        focus::show(ui.painter(), area, focused);
        outcome
    }

    fn sync(&mut self, view: &View) {
        if view.devices.is_empty() {
            self.selected_device = None;
            self.selected_param = 0;
            return;
        }
        if !view
            .devices
            .iter()
            .any(|device| Some(device.id) == self.selected_device)
        {
            self.selected_device = view.devices.first().map(|device| device.id);
            self.selected_param = 0;
        }
        if let Some(device) = self.selected(view) {
            self.selected_param = self
                .selected_param
                .min(device.params.len().saturating_sub(1));
        }
    }

    fn selected<'a>(&self, view: &'a View) -> Option<&'a DeviceView> {
        let id = self.selected_device?;
        view.devices.iter().find(|device| device.id == id)
    }

    fn speak(&mut self, view: &View, utterance: Utterance, outcome: &mut Outcome) {
        self.sync(view);
        self.refusal = None;
        if self.chooser.open {
            return;
        }
        let count = utterance.count.max(1);
        if utterance.held {
            // Held addresses the lock side of base + lock. This panel has no
            // trig noun, so falling through to a base edit would corrupt the
            // grammar's most important distinction.
            self.refusal = Some("HOLD: NO TRIG HERE".to_owned());
            return;
        }
        if matches!(utterance.verb, Some(Verb::Search)) {
            if view.catalogue.is_empty() {
                self.refusal = Some("SEARCH: NO DEVICES".to_owned());
            } else {
                self.chooser.open();
            }
            return;
        }
        if view.devices.is_empty() {
            self.refusal = Some("DEVICE: NOTHING HERE".to_owned());
            return;
        }
        match (utterance.verb, utterance.motion) {
            (None, Some(Motion::Left)) => self.move_address(view, -(count as isize)),
            (None, Some(Motion::Right)) => self.move_address(view, count as isize),
            (None, Some(motion @ (Motion::Up | Motion::Down))) => {
                self.adjust(view, motion, count, outcome)
            }
            (Some(Verb::Act), _) => {
                if let Some(device) = self.selected(view) {
                    outcome
                        .intents
                        .push(Intent::ToggleBypass { device: device.id });
                }
            }
            (Some(Verb::Nudge), Some(motion @ (Motion::Left | Motion::Right))) => {
                self.reorder(view, motion, count, outcome)
            }
            (Some(Verb::Nudge), Some(_)) => {
                self.refusal = Some("NUDGE: LEFT OR RIGHT".to_owned());
            }
            (Some(verb), _) => {
                self.refusal = Some(format!("{}: NOT HERE", verb.name()));
            }
            (None, None) => {}
        }
    }

    fn chooser_command(
        &mut self,
        catalogue: &[CatalogueItem],
        command: ChooserCommand,
        outcome: &mut Outcome,
    ) {
        let matches = catalogue_matches(catalogue, &self.chooser.query);
        self.chooser.cursor = self.chooser.cursor.min(matches.len().saturating_sub(1));
        match command {
            ChooserCommand::Up if !matches.is_empty() => {
                self.chooser.cursor = self
                    .chooser
                    .cursor
                    .checked_sub(1)
                    .unwrap_or(matches.len() - 1);
            }
            ChooserCommand::Down if !matches.is_empty() => {
                self.chooser.cursor = (self.chooser.cursor + 1) % matches.len();
            }
            ChooserCommand::Accept => {
                if let Some(&catalogue_index) = matches.get(self.chooser.cursor) {
                    outcome.intents.push(Intent::AddDevice { catalogue_index });
                    self.chooser.close();
                }
            }
            ChooserCommand::Cancel => self.chooser.close(),
            ChooserCommand::Up | ChooserCommand::Down => {}
        }
    }

    fn adjust(&mut self, view: &View, motion: Motion, count: usize, outcome: &mut Outcome) {
        let Some(device) = self.selected(view) else {
            return;
        };
        let Some(param) = device.params.get(self.selected_param) else {
            self.refusal = Some("SLOT: EMPTY CARD".to_owned());
            return;
        };
        let direction = if motion == Motion::Up { 1.0 } else { -1.0 };
        let delta = if param.choices > 0 {
            direction * count as f32
        } else {
            direction * (param.max - param.min) * COARSE_FRACTION * count as f32
        };
        let value = (param.base + delta).clamp(param.min, param.max);
        outcome.intents.push(Intent::SetParam {
            device: device.id,
            param: param.id,
            value,
        });
    }

    fn reorder(&mut self, view: &View, motion: Motion, count: usize, outcome: &mut Outcome) {
        let Some(from) = view
            .devices
            .iter()
            .position(|device| Some(device.id) == self.selected_device)
        else {
            return;
        };
        let amount = if motion == Motion::Left {
            -(count as isize)
        } else {
            count as isize
        };
        let target_index = from as isize + amount;
        let Some(target) = usize::try_from(target_index)
            .ok()
            .filter(|target| *target < view.devices.len())
        else {
            self.refusal = Some("NUDGE: CHAIN EDGE".to_owned());
            return;
        };
        let source = &view.devices[from];
        let target_device = &view.devices[target];
        if source.instrument || target_device.instrument {
            self.refusal = Some("NUDGE: SOURCE IS FIXED".to_owned());
            return;
        }
        if source.parent != target_device.parent {
            self.refusal = Some("NUDGE: RACK EDGE".to_owned());
            return;
        }
        outcome.intents.push(Intent::Reorder {
            device: source.id,
            target: target_device.id,
        });
    }

    fn move_address(&mut self, view: &View, amount: isize) {
        let Some(current) = self.address(view) else {
            return;
        };
        let total = address_count(view);
        let target = (current as isize + amount).clamp(0, total.saturating_sub(1) as isize);
        self.select_address(view, target as usize);
    }

    fn address(&self, view: &View) -> Option<usize> {
        let selected = self.selected_device?;
        let mut address = 0;
        for device in &view.devices {
            if device.id == selected {
                return Some(address + self.selected_param.min(address_span(device) - 1));
            }
            address += address_span(device);
        }
        None
    }

    fn select_address(&mut self, view: &View, mut address: usize) {
        for device in &view.devices {
            let span = address_span(device);
            if address < span {
                self.selected_device = Some(device.id);
                self.selected_param = address.min(device.params.len().saturating_sub(1));
                return;
            }
            address -= span;
        }
    }

    fn draw_chooser(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        catalogue: &[CatalogueItem],
        outcome: &mut Outcome,
    ) {
        let field_id = ui.id().with("chain-device-search");
        let command = ui.ctx().input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                Some(ChooserCommand::Cancel)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                Some(ChooserCommand::Up)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                Some(ChooserCommand::Down)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                Some(ChooserCommand::Accept)
            } else {
                None
            }
        });
        if let Some(command) = command {
            self.chooser_command(catalogue, command, outcome);
        }
        if !self.chooser.open {
            ui.ctx()
                .memory_mut(|memory| memory.surrender_focus(field_id));
            return;
        }

        let width = CHOOSER_W.min(area.width().max(1.0));
        let height =
            (CHOOSER_FIELD_H + CHOOSER_ROW_H * CHOOSER_ROWS as f32).min(area.height().max(1.0));
        let rect = egui::Rect::from_center_size(area.center(), egui::vec2(width, height));
        ui.painter().rect_filled(rect, 0.0, BAND);
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(stroke::HAIR, CURSOR),
            egui::StrokeKind::Inside,
        );
        let field_rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(space::SM, space::XS),
            egui::pos2(
                rect.right() - space::SM,
                rect.top() + CHOOSER_FIELD_H - space::XS,
            ),
        );
        let field = ui.put(
            field_rect,
            egui::TextEdit::singleline(&mut self.chooser.query)
                .id(field_id)
                .hint_text("ADD DEVICE")
                .font(egui::FontId::new(font::LABEL, egui::FontFamily::Monospace))
                .text_color(TEXT)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO),
        );
        if self.chooser.just_opened {
            // Search's `/` opened the chooser; it is a verb, not the first
            // character of the device query.
            self.chooser.query.clear();
            field.request_focus();
            self.chooser.just_opened = false;
        }

        let matches = catalogue_matches(catalogue, &self.chooser.query);
        self.chooser.cursor = self.chooser.cursor.min(matches.len().saturating_sub(1));
        let first = self
            .chooser
            .cursor
            .saturating_add(1)
            .saturating_sub(CHOOSER_ROWS);
        for (row, &catalogue_index) in matches.iter().skip(first).take(CHOOSER_ROWS).enumerate() {
            let item = &catalogue[catalogue_index];
            let row_index = first + row;
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left(),
                    rect.top() + CHOOSER_FIELD_H + row as f32 * CHOOSER_ROW_H,
                ),
                egui::vec2(rect.width(), CHOOSER_ROW_H),
            );
            if row_index == self.chooser.cursor {
                ui.painter().rect_filled(row_rect, 0.0, SELECTED);
            }
            ui.painter().text(
                row_rect.left_center() + egui::vec2(space::SM, 0.0),
                egui::Align2::LEFT_CENTER,
                &item.name,
                egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                if row_index == self.chooser.cursor {
                    TEXT
                } else {
                    MUTED
                },
            );
            ui.painter().text(
                row_rect.right_center() - egui::vec2(space::SM, 0.0),
                egui::Align2::RIGHT_CENTER,
                if item.is_instrument {
                    "INSTRUMENT"
                } else {
                    "EFFECT"
                },
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                MUTED,
            );
        }
    }

    fn draw_cards(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
        view: &View,
        outcome: &mut Outcome,
    ) {
        ui.painter().rect_filled(viewport, 0.0, VOID);
        if view.devices.is_empty() {
            draw_text(
                ui.painter(),
                viewport.shrink(space::SM),
                "EMPTY  //  PRESS / TO ADD",
                QUIET,
            );
            return;
        }

        let layouts = card_layouts(
            view,
            viewport.height(),
            self.selected_device,
            self.selected_param,
        );
        let content_width = layouts
            .last()
            .map_or(0.0, |layout| layout.x + layout.geometry.width);
        let max_scroll = (content_width - viewport.width()).max(0.0);

        if let Some(target) =
            selected_cell_range(&layouts, self.selected_device, self.selected_param)
        {
            self.scroll_x = ensure_visible_offset(
                self.scroll_x,
                viewport.width(),
                target.0,
                target.1,
                content_width,
            );
        } else {
            self.scroll_x = self.scroll_x.clamp(0.0, max_scroll);
        }

        // One background interaction owns row panning. It is registered
        // before the cells, so a press on a cell belongs to that cell for
        // the whole gesture while empty ground remains draggable.
        let pan = ui
            .interact(
                viewport,
                ui.id().with("chain-pan"),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Sweep);
        if pan.hovered() {
            let wheel = ui.input(|input| input.smooth_scroll_delta);
            let delta = if wheel.x.abs() > wheel.y.abs() {
                wheel.x
            } else {
                wheel.y
            };
            if delta != 0.0 {
                self.scroll_x = (self.scroll_x - delta).clamp(0.0, max_scroll);
                ui.ctx()
                    .input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        if pan.dragged() {
            let delta = ui.input(|input| input.pointer.delta().x);
            self.scroll_x = (self.scroll_x - delta).clamp(0.0, max_scroll);
        }

        let painter = ui.painter().with_clip_rect(viewport);
        for (device, layout) in view.devices.iter().zip(&layouts) {
            let card = layout.rect(viewport.min, self.scroll_x);
            if !card.intersects(viewport) {
                continue;
            }
            let ink = card_ink(device.bypassed);
            painter.rect_filled(card, 0.0, ink.surface);
            let header = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), HEADER_H));
            let header_response = ui
                .interact(
                    header,
                    ui.id().with(("chain-device", device.id)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press);
            if header_response.clicked() {
                self.selected_device = Some(device.id);
                self.selected_param = 0;
                outcome.claim_focus = true;
            }
            let selected_device = Some(device.id) == self.selected_device;
            painter.rect_filled(
                header,
                0.0,
                if selected_device {
                    ink.selected
                } else if header_response.hovered() {
                    ink.hover
                } else {
                    ink.cell
                },
            );
            let inner = header.shrink2(egui::vec2(space::XS, 0.0));
            let name_rect = egui::Rect::from_min_max(
                inner.min,
                egui::pos2((inner.right() - 30.0).max(inner.left()), inner.bottom()),
            );
            draw_text(&painter, name_rect, &device.name, ink.text);
            painter.text(
                inner.right_center(),
                egui::Align2::RIGHT_CENTER,
                if device.bypassed { "OFF" } else { "ON" },
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                ink.muted,
            );

            let mut top = header.bottom() + CARD_PAD;
            if layout.geometry.hero_h > 0.0 {
                let hero = egui::Rect::from_min_size(
                    egui::pos2(card.left() + CARD_PAD, top),
                    egui::vec2(card.width() - CARD_PAD * 2.0, layout.geometry.hero_h),
                );
                draw_hero(&painter, hero, device, ink);
                top = hero.bottom() + GAP;
            }

            let first = layout.geometry.first_row * COLUMNS;
            let end = ((layout.geometry.first_row + layout.geometry.visible_rows) * COLUMNS)
                .min(device.params.len());
            for (index, param) in device.params.iter().enumerate().take(end).skip(first) {
                let visible_row = index / COLUMNS - layout.geometry.first_row;
                let column = index % COLUMNS;
                let cell = egui::Rect::from_min_size(
                    egui::pos2(
                        card.left() + CARD_PAD + column as f32 * (layout.geometry.cell_side + GAP),
                        top + visible_row as f32 * (layout.geometry.cell_side + GAP),
                    ),
                    egui::vec2(layout.geometry.cell_side, layout.geometry.cell_side),
                );
                if !cell.intersects(viewport) {
                    continue;
                }
                let response = ui
                    .interact(
                        cell,
                        ui.id().with(("chain-slot", device.id, param.id)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press);
                painter.rect_filled(
                    cell,
                    0.0,
                    if response.hovered() {
                        ink.hover
                    } else {
                        ink.cell
                    },
                );
                if response.clicked() {
                    self.selected_device = Some(device.id);
                    self.selected_param = index;
                    outcome.claim_focus = true;
                }
                let selected = selected_device && index == self.selected_param;
                if selected {
                    painter.rect_stroke(
                        cell.shrink(1.0),
                        0.0,
                        egui::Stroke::new(stroke::HAIR, CURSOR),
                        egui::StrokeKind::Inside,
                    );
                }
                if cell.width() >= 30.0 && cell.height() >= 22.0 {
                    let inner = cell.shrink2(egui::vec2(space::XS, space::XS));
                    painter.with_clip_rect(inner).text(
                        inner.left_top(),
                        egui::Align2::LEFT_TOP,
                        &param.name,
                        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                        if selected { ink.text } else { ink.muted },
                    );
                    if cell.width() >= 44.0 && cell.height() >= 40.0 {
                        painter.with_clip_rect(inner).text(
                            inner.left_bottom(),
                            egui::Align2::LEFT_BOTTOM,
                            display_value(param),
                            egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                            ink.text,
                        );
                    }
                }
                let position = if param.max <= param.min {
                    0.0
                } else {
                    ((param.base - param.min) / (param.max - param.min)).clamp(0.0, 1.0)
                };
                let rail = egui::Rect::from_min_size(
                    egui::pos2(cell.left(), cell.bottom() - 2.0),
                    egui::vec2(cell.width() * position, 2.0),
                );
                painter.rect_filled(rail, 0.0, ink.rail);
            }
        }
    }

    fn draw_status(&self, ui: &egui::Ui, rect: egui::Rect, view: &View, overlay: Option<&str>) {
        ui.painter().rect_filled(rect, 0.0, BAND);
        let device_index = self
            .selected_device
            .and_then(|id| view.devices.iter().position(|device| device.id == id));
        let left = overlay.map_or_else(
            || {
                let track = view.track_name.as_deref().unwrap_or("NO TRACK");
                format!("{track}  //  LEFT/RIGHT SLOT")
            },
            str::to_owned,
        );
        let inner = rect.shrink2(egui::vec2(space::SM, 0.0));
        if rect.width() >= 104.0 {
            let left_rect = egui::Rect::from_min_max(
                inner.min,
                egui::pos2((inner.right() - 64.0).max(inner.left()), inner.bottom()),
            );
            draw_text(ui.painter(), left_rect, &left, MUTED);
        }
        ui.painter().text(
            rect.right_center() - egui::vec2(space::SM, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!(
                "DEV {:02}/{:02}",
                device_index.map_or(0, |index| index + 1),
                view.devices.len()
            ),
            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
            MUTED,
        );
    }
}

fn catalogue_matches(catalogue: &[CatalogueItem], query: &str) -> Vec<usize> {
    let query = query.to_ascii_lowercase();
    catalogue
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.name
                .to_ascii_lowercase()
                .contains(&query)
                .then_some(index)
        })
        .collect()
}

fn draw_text(painter: &egui::Painter, rect: egui::Rect, text: &str, color: egui::Color32) {
    painter.with_clip_rect(rect).text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        color,
    );
}

fn display_value(param: &ParamView) -> String {
    param.lock.as_ref().map_or_else(
        || param.formatted.clone(),
        |lock| {
            format!(
                "{} {} = {}",
                param.formatted, lock.formatted_offset, lock.formatted_effective
            )
        },
    )
}

fn address_span(device: &DeviceView) -> usize {
    device.params.len().max(1)
}

fn address_count(view: &View) -> usize {
    view.devices.iter().map(address_span).sum::<usize>().max(1)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CardGeometry {
    width: f32,
    height: f32,
    visible_rows: usize,
    first_row: usize,
    cell_side: f32,
    hero_h: f32,
}

fn card_geometry(
    param_count: usize,
    hero: HeroKind,
    available_height: f32,
    selected_param: usize,
) -> CardGeometry {
    let rows = param_rows(param_count);
    let hero_h = if hero == HeroKind::None { 0.0 } else { HERO_H };
    let hero_gap = if hero_h > 0.0 { GAP } else { 0.0 };
    let grid_room = (available_height - HEADER_H - CARD_PAD * 2.0 - hero_h - hero_gap).max(1.0);
    let max_rows = ((grid_room + GAP) / (CELL_MIN_SIDE + GAP)).floor().max(1.0) as usize;
    let visible_rows = rows.min(max_rows);
    let cell_side = if visible_rows == 0 {
        CELL_SIDE
    } else {
        CELL_SIDE
            .min((grid_room - GAP * visible_rows.saturating_sub(1) as f32) / visible_rows as f32)
    }
    .max(1.0);
    let selected_row = selected_param.min(param_count.saturating_sub(1)) / COLUMNS;
    let first_row = selected_row
        .saturating_sub(visible_rows / 2)
        .min(rows.saturating_sub(visible_rows));
    let columns = param_count.clamp(1, COLUMNS);
    // Width is declared at the preferred cell size even when height forces
    // a dense grid. That keeps a four-column instrument visibly broader
    // than a three-control utility instead of making complexity collapse.
    let grid_width = columns as f32 * CELL_SIDE + columns.saturating_sub(1) as f32 * GAP;
    let width = (grid_width + CARD_PAD * 2.0).max(116.0);
    let grid_height = if visible_rows == 0 {
        0.0
    } else {
        visible_rows as f32 * cell_side + visible_rows.saturating_sub(1) as f32 * GAP
    };
    CardGeometry {
        width,
        height: HEADER_H + CARD_PAD * 2.0 + hero_h + hero_gap + grid_height,
        visible_rows,
        first_row,
        cell_side,
        hero_h,
    }
}

fn param_rows(param_count: usize) -> usize {
    param_count.div_ceil(COLUMNS)
}

#[derive(Clone, Copy, Debug)]
struct CardLayout {
    device: u64,
    x: f32,
    geometry: CardGeometry,
}

impl CardLayout {
    fn rect(self, origin: egui::Pos2, scroll_x: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            origin + egui::vec2(self.x - scroll_x, CARD_PAD),
            egui::vec2(self.geometry.width, self.geometry.height),
        )
    }
}

fn card_layouts(
    view: &View,
    available_height: f32,
    selected_device: Option<u64>,
    selected_param: usize,
) -> Vec<CardLayout> {
    let mut x = CARD_PAD;
    view.devices
        .iter()
        .map(|device| {
            let geometry = card_geometry(
                device.params.len(),
                device.hero,
                available_height - CARD_PAD * 2.0,
                if Some(device.id) == selected_device {
                    selected_param
                } else {
                    0
                },
            );
            let layout = CardLayout {
                device: device.id,
                x,
                geometry,
            };
            x += geometry.width + CARD_GAP;
            layout
        })
        .collect()
}

fn selected_cell_range(
    layouts: &[CardLayout],
    selected_device: Option<u64>,
    selected_param: usize,
) -> Option<(f32, f32)> {
    let layout = layouts
        .iter()
        .find(|layout| Some(layout.device) == selected_device)?;
    if layout.geometry.visible_rows == 0 {
        return Some((layout.x, layout.x + layout.geometry.width));
    }
    let column = selected_param % COLUMNS;
    let left = layout.x + CARD_PAD + column as f32 * (layout.geometry.cell_side + GAP);
    Some((left, left + layout.geometry.cell_side))
}

/// Move only as far as needed to reveal the cursor, then clamp to the row.
fn ensure_visible_offset(
    current: f32,
    viewport_width: f32,
    target_start: f32,
    target_end: f32,
    content_width: f32,
) -> f32 {
    let max_scroll = (content_width - viewport_width).max(0.0);
    let margin = PAN_MARGIN.min(viewport_width.max(0.0) * 0.25);
    let mut next = current.clamp(0.0, max_scroll);
    if target_start < next + margin {
        next = target_start - margin;
    } else if target_end > next + viewport_width - margin {
        next = target_end + margin - viewport_width;
    }
    next.clamp(0.0, max_scroll)
}

#[derive(Clone, Copy)]
struct CardInk {
    surface: egui::Color32,
    cell: egui::Color32,
    hover: egui::Color32,
    selected: egui::Color32,
    muted: egui::Color32,
    text: egui::Color32,
    rail: egui::Color32,
}

fn card_ink(bypassed: bool) -> CardInk {
    if bypassed {
        CardInk {
            surface: BAND,
            cell: egui::Color32::from_gray(14),
            hover: CELL,
            selected: HOVER,
            muted: QUIET,
            text: egui::Color32::from_gray(82),
            rail: QUIET,
        }
    } else {
        CardInk {
            surface: CELL,
            cell: SELECTED,
            hover: egui::Color32::from_gray(34),
            selected: egui::Color32::from_gray(40),
            muted: MUTED,
            text: TEXT,
            rail: MUTED,
        }
    }
}

fn param_value(device: &DeviceView, id: u32, fallback: f32) -> f32 {
    device
        .params
        .iter()
        .find(|param| param.id == id)
        .map_or(fallback, |param| param.base)
}

fn draw_hero(painter: &egui::Painter, rect: egui::Rect, device: &DeviceView, ink: CardInk) {
    painter.rect_filled(rect, 0.0, BAND);
    painter.hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(stroke::HAIR, ink.surface),
    );
    match device.hero {
        HeroKind::None => {}
        HeroKind::Kick => draw_kick_hero(painter, rect, device, ink),
        HeroKind::Filter => draw_filter_hero(painter, rect, device, ink),
        HeroKind::Saturator => draw_saturator_hero(painter, rect, device, ink),
    }
}

fn draw_kick_hero(painter: &egui::Painter, rect: egui::Rect, device: &DeviceView, ink: CardInk) {
    use crate::params::kick as kp;

    let depth_a = param_value(device, kp::PITCH_A_DEPTH, 0.0);
    let decay_a = param_value(device, kp::PITCH_A_DECAY, 1.0).max(0.01);
    let depth_b = param_value(device, kp::PITCH_B_DEPTH, 0.0);
    let decay_b = param_value(device, kp::PITCH_B_DECAY, 1.0).max(0.01);
    let amp_decay = param_value(device, kp::AMP_DECAY, 1.0).max(0.01);
    let ceiling = (depth_a + depth_b).abs().max(1.0);
    let mut pitch = Vec::with_capacity(33);
    let mut amp = Vec::with_capacity(33);
    for index in 0..=32 {
        let along = index as f32 / 32.0;
        let ms = along * 400.0;
        let semitones =
            depth_a * (1.0 - ms / decay_a).max(0.0) + depth_b * (1.0 - ms / decay_b).max(0.0);
        pitch.push(egui::pos2(
            egui::lerp(rect.x_range(), along),
            egui::lerp(rect.y_range(), 1.0 - (semitones / ceiling).clamp(0.0, 1.0)),
        ));
        amp.push(egui::pos2(
            egui::lerp(rect.x_range(), along),
            egui::lerp(rect.y_range(), 1.0 - (1.0 - ms / amp_decay).clamp(0.0, 1.0)),
        ));
    }
    painter.add(egui::Shape::line(
        amp,
        egui::Stroke::new(stroke::HAIR, ink.muted),
    ));
    painter.add(egui::Shape::line(pitch, egui::Stroke::new(1.0, ink.rail)));
}

fn draw_filter_hero(painter: &egui::Painter, rect: egui::Rect, device: &DeviceView, ink: CardInk) {
    use crate::params::filter as fp;
    use crate::ui::device::filter;

    let state = filter::FilterUi::from_engine(|id| {
        param_value(device, id, crate::params::def(fp::TABLE, id).default)
    });
    let filter = state.spec();
    let mut points = Vec::with_capacity(49);
    for index in 0..=48 {
        let along = index as f32 / 48.0;
        let hz = 20.0 * 1_000.0_f32.powf(along);
        let db = filter::magnitude_db(&filter, hz, 48_000.0).clamp(-36.0, 12.0);
        points.push(egui::pos2(
            egui::lerp(rect.x_range(), along),
            egui::lerp(rect.y_range(), 1.0 - (db + 36.0) / 48.0),
        ));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, ink.rail)));
}

fn draw_saturator_hero(
    painter: &egui::Painter,
    rect: egui::Rect,
    device: &DeviceView,
    ink: CardInk,
) {
    use crate::params::sat as sp;
    use crate::ui::device::shaper::{Mode, Shaper};

    let shaper = Shaper {
        mode: Mode::from_index(param_value(device, sp::MODE, sp::MODE_SOFT as f32) as usize),
        drive: param_value(device, sp::DRIVE, 1.0),
        bias: param_value(device, sp::BIAS, 0.0),
        mix: param_value(device, sp::MIX, 1.0),
    };
    let mut points = Vec::with_capacity(49);
    for index in 0..=48 {
        let along = index as f32 / 48.0;
        let x = along * 2.0 - 1.0;
        points.push(egui::pos2(
            egui::lerp(rect.x_range(), along),
            egui::lerp(rect.y_range(), (1.0 - shaper.shape(x)) * 0.5),
        ));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, ink.rail)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(id: u32, base: f32, choices: u32, formatted: &str) -> ParamView {
        ParamView {
            id,
            name: format!("P{id}"),
            min: 0.0,
            max: 100.0,
            base,
            choices,
            formatted: formatted.to_owned(),
            lock: None,
        }
    }

    fn device(id: u64, params: usize) -> DeviceView {
        DeviceView {
            id,
            name: format!("D{id}"),
            bypassed: false,
            instrument: false,
            parent: None,
            hero: HeroKind::None,
            params: (0..params)
                .map(|index| param(index as u32, 50.0, 0, "50%"))
                .collect(),
        }
    }

    fn view() -> View {
        View {
            track_name: Some("ONE".to_owned()),
            devices: vec![device(10, 17), device(20, 2)],
            catalogue: vec![
                CatalogueItem {
                    name: "Poly".to_owned(),
                    is_instrument: true,
                },
                CatalogueItem {
                    name: "Filter".to_owned(),
                    is_instrument: false,
                },
                CatalogueItem {
                    name: "Polysaturator".to_owned(),
                    is_instrument: false,
                },
            ],
        }
    }

    fn utter(
        panel: &mut ChainPanel,
        view: &View,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
        held: bool,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        panel.speak(
            view,
            Utterance {
                count,
                verb,
                motion,
                held,
            },
            &mut outcome,
        );
        outcome
    }

    #[test]
    fn seventeen_rows_stack_in_five_rows_of_four() {
        let geometry = card_geometry(17, HeroKind::None, 500.0, 16);
        assert_eq!(param_rows(17), 5);
        assert_eq!(geometry.visible_rows, 5);
        assert_eq!(16 / COLUMNS, 4);
        assert_eq!(16 % COLUMNS, 0);
    }

    #[test]
    fn formatter_output_passes_through_the_slot_unchanged() {
        let params = [param(7, 2.0, 4, "saw")];
        assert_eq!(params[0].formatted, "saw");
        assert_eq!(display_value(&params[0]), "saw");
    }

    #[test]
    fn a_lock_is_shown_as_base_plus_offset_equals_effective() {
        let mut row = param(1, 0.5, 0, "0.50");
        row.lock = Some(LockView {
            offset: 0.25,
            formatted_offset: "+0.25".to_owned(),
            formatted_effective: "0.75".to_owned(),
        });
        assert_eq!(display_value(&row), "0.50 +0.25 = 0.75");
    }

    #[test]
    fn counted_slot_travel_crosses_a_page_and_then_a_device() {
        let view = view();
        let mut panel = ChainPanel::default();
        utter(&mut panel, &view, None, Some(Motion::Right), 8, false);
        assert_eq!(panel.selected_device, Some(10));
        assert_eq!(panel.selected_param, 8);
        utter(&mut panel, &view, None, Some(Motion::Right), 9, false);
        assert_eq!(panel.selected_device, Some(20));
        assert_eq!(panel.selected_param, 0);
    }

    #[test]
    fn up_down_adjust_base_with_choice_and_sweep_steps() {
        let mut view = view();
        view.devices[0].params[0] = param(3, 4.0, 8, "four");
        let mut panel = ChainPanel::default();
        let outcome = utter(&mut panel, &view, None, Some(Motion::Up), 3, false);
        assert_eq!(
            outcome.intents,
            vec![Intent::SetParam {
                device: 10,
                param: 3,
                value: 7.0,
            }]
        );

        view.devices[0].params[0] = param(4, 50.0, 0, "50%");
        let outcome = utter(&mut panel, &view, None, Some(Motion::Down), 2, false);
        assert_eq!(
            outcome.intents,
            vec![Intent::SetParam {
                device: 10,
                param: 4,
                value: 45.0,
            }]
        );
    }

    #[test]
    fn act_toggles_the_selected_devices_bypass() {
        let view = view();
        let mut panel = ChainPanel::default();
        let outcome = utter(&mut panel, &view, Some(Verb::Act), None, 1, false);
        assert_eq!(outcome.intents, vec![Intent::ToggleBypass { device: 10 }]);
    }

    #[test]
    fn nudge_reorders_siblings_and_refuses_the_chain_edge() {
        let view = view();
        let mut panel = ChainPanel::default();
        let outcome = utter(
            &mut panel,
            &view,
            Some(Verb::Nudge),
            Some(Motion::Right),
            1,
            false,
        );
        assert_eq!(
            outcome.intents,
            vec![Intent::Reorder {
                device: 10,
                target: 20,
            }]
        );
        let outcome = utter(
            &mut panel,
            &view,
            Some(Verb::Nudge),
            Some(Motion::Left),
            1,
            false,
        );
        assert!(outcome.intents.is_empty());
        assert_eq!(panel.refusal.as_deref(), Some("NUDGE: CHAIN EDGE"));
    }

    #[test]
    fn held_motion_and_unsupported_verbs_refuse_out_loud() {
        let view = view();
        let mut panel = ChainPanel::default();
        let outcome = utter(&mut panel, &view, None, Some(Motion::Up), 1, true);
        assert!(outcome.intents.is_empty());
        assert_eq!(panel.refusal.as_deref(), Some("HOLD: NO TRIG HERE"));

        utter(&mut panel, &view, Some(Verb::Rename), None, 1, false);
        assert_eq!(panel.refusal.as_deref(), Some("RENAME: NOT HERE"));
    }

    #[test]
    fn catalogue_filtering_is_case_insensitive_and_keeps_registry_order() {
        let view = view();
        assert_eq!(catalogue_matches(&view.catalogue, "pOlY"), vec![0, 2]);
        assert_eq!(catalogue_matches(&view.catalogue, "FILTER"), vec![1]);
        assert_eq!(catalogue_matches(&view.catalogue, ""), vec![0, 1, 2]);
    }

    #[test]
    fn chooser_enter_emits_the_original_catalogue_index() {
        let view = view();
        let mut panel = ChainPanel::default();
        utter(&mut panel, &view, Some(Verb::Search), None, 1, false);
        panel.chooser.query = "poly".to_owned();
        panel.chooser.cursor = 1;
        let mut outcome = Outcome::default();

        panel.chooser_command(&view.catalogue, ChooserCommand::Accept, &mut outcome);

        assert_eq!(
            outcome.intents,
            vec![Intent::AddDevice { catalogue_index: 2 }]
        );
        assert!(!panel.chooser.open);
    }

    #[test]
    fn search_opens_on_an_empty_chain() {
        let mut view = view();
        view.devices.clear();
        let mut panel = ChainPanel::default();

        let outcome = utter(&mut panel, &view, Some(Verb::Search), None, 1, false);

        assert!(outcome.intents.is_empty());
        assert!(panel.chooser.open);
        assert!(panel.refusal.is_none());
    }

    #[test]
    fn chooser_escape_closes_without_emitting() {
        let view = view();
        let mut panel = ChainPanel::default();
        utter(&mut panel, &view, Some(Verb::Search), None, 1, false);
        let mut outcome = Outcome::default();

        panel.chooser_command(&view.catalogue, ChooserCommand::Cancel, &mut outcome);

        assert!(outcome.intents.is_empty());
        assert!(!panel.chooser.open);
    }

    #[test]
    fn an_open_chooser_swallows_chain_verbs() {
        let view = view();
        let mut panel = ChainPanel::default();
        utter(&mut panel, &view, Some(Verb::Search), None, 1, false);

        let outcome = utter(
            &mut panel,
            &view,
            Some(Verb::Nudge),
            Some(Motion::Right),
            1,
            false,
        );

        assert!(outcome.intents.is_empty());
        assert!(panel.chooser.open);
        assert!(panel.refusal.is_none());
    }

    /// Surfaces rise from void to selection; ink rises independently from
    /// furniture to text. Pure white is absent because focus owns it.
    #[test]
    fn value_ladders_stay_ordered_and_below_focus() {
        assert!(VOID.r() < BAND.r());
        assert!(BAND.r() < CELL.r());
        assert!(CELL.r() < HOVER.r());
        assert!(HOVER.r() < SELECTED.r());
        assert!(QUIET.r() < MUTED.r());
        assert!(MUTED.r() < CURSOR.r());
        assert!(CURSOR.r() < TEXT.r());
        assert!(TEXT.r() < egui::Color32::WHITE.r());
    }

    #[test]
    fn card_width_grows_with_parameter_count() {
        let small = card_geometry(3, HeroKind::None, 240.0, 0);
        let large = card_geometry(17, HeroKind::None, 240.0, 0);
        assert!(small.width < large.width);
    }

    #[test]
    fn three_parameters_make_exactly_one_cell_row_plus_header() {
        let geometry = card_geometry(3, HeroKind::None, 240.0, 0);
        assert_eq!(param_rows(3), 1);
        assert_eq!(geometry.visible_rows, 1);
        assert_eq!(
            geometry.height,
            HEADER_H + CARD_PAD * 2.0 + geometry.cell_side
        );
    }

    #[test]
    fn auto_pan_moves_only_enough_to_keep_the_cursor_visible() {
        assert_eq!(
            ensure_visible_offset(0.0, 100.0, 250.0, 300.0, 400.0),
            212.0
        );
        assert_eq!(
            ensure_visible_offset(212.0, 100.0, 250.0, 300.0, 400.0),
            212.0
        );
        assert_eq!(ensure_visible_offset(212.0, 100.0, 10.0, 40.0, 400.0), 0.0);
    }

    #[test]
    fn bypass_drops_the_whole_cards_ink() {
        let active = card_ink(false);
        let bypassed = card_ink(true);
        assert!(bypassed.surface.r() < active.surface.r());
        assert!(bypassed.cell.r() < active.cell.r());
        assert!(bypassed.selected.r() < active.selected.r());
        assert!(bypassed.text.r() < active.text.r());
        assert!(bypassed.rail.r() < active.rail.r());
    }

    #[test]
    fn dragging_empty_ground_pans_the_row() {
        use crate::ui::device::probe;

        let context = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 220.0));
        let mut view = view();
        view.devices = (0..6).map(|index| device(10 + index, 3)).collect();
        let mut panel = ChainPanel {
            selected_device: Some(10),
            ..Default::default()
        };
        let offsets = probe::run(
            &context,
            rect,
            &probe::drag_path(egui::pos2(260.0, 180.0), egui::pos2(100.0, 180.0), 6),
            |ui| {
                let mut outcome = Outcome::default();
                panel.draw_cards(ui, rect, &view, &mut outcome);
                panel.scroll_x
            },
        );
        assert!(offsets.into_iter().any(|offset| offset > 0.0));
    }
}
