//! Selected-trig inspector for the sequence region.
//!
//! Values are placeholders sourced from the UI grid selection. The eventual
//! sequence model can provide the same snapshot without changing this view.

use crate::design::{Polarity, circuit, kit::Weight};
use crate::ui::sequencer::grid_resolution::length_label;
use crate::ui::sequencer::layout_grid::{GridArea, LayoutGrid};
use crate::ui::sequencer::lens::degree_label;
use crate::ui::sequencer::sequence_grid::{TrigSelection, note_name};
use crate::ui::sequencer::{INK_LEVEL, shade};
use crate::ui::tokens::{control, font, space};
use eframe::egui;

const PANEL_HEIGHT_MAX: f32 = 224.0;
const HEADER_HEIGHT: f32 = 32.0;
const SUM_HEIGHT: f32 = 28.0;
const FIELD_COLUMNS: usize = 2;
const FIELD_ROWS: usize = 4;
/// Two planes and no rules: the panel is a recess, its header one rung
/// up, and the fields sit on the recess with nothing drawn between them.
/// Alignment does the dividing.
/// Levels above the ground, not colours: see `sequencer::shade`.
const HEADER_FILL: u8 = 18;
const LABEL_COLOR: u8 = 112;
const PANEL_FILL: u8 = 10;

struct Field<'a> {
    label: &'static str,
    value: &'a str,
    area: GridArea,
}

impl<'a> Field<'a> {
    const fn new(label: &'static str, value: &'a str, area: GridArea) -> Self {
        Self { label, value, area }
    }
}

pub(crate) fn panel_rect(available: egui::Rect) -> egui::Rect {
    let inset = space::MD;
    let width = control::SIDE_COLUMN_W.min((available.width() - inset * 2.0).max(1.0));
    let height = PANEL_HEIGHT_MAX.min((available.height() - inset * 2.0).max(1.0));
    egui::Rect::from_center_size(
        egui::pos2(available.left() + inset + width * 0.5, available.center().y),
        egui::vec2(width, height),
    )
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    selection: TrigSelection,
    ground: Polarity,
) {
    let painter = ui.painter_at(rect);
    let mut panel = Vec::new();
    circuit::panel_variant(
        &mut panel,
        rect,
        Some(shade(PANEL_FILL, ground)),
        shade(0, ground),
        Some((Weight::Hair, shade(LABEL_COLOR, ground))),
        1,
    );
    for shape in panel {
        painter.add(shape);
    }

    let header_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.top() + HEADER_HEIGHT),
    );
    let mut header = Vec::new();
    circuit::panel_variant(
        &mut header,
        header_rect,
        Some(shade(HEADER_FILL, ground)),
        shade(PANEL_FILL, ground),
        Some((Weight::Hair, shade(LABEL_COLOR, ground))),
        3,
    );
    for shape in header {
        painter.add(shape);
    }
    // The header names the noun and, at its right, the noun's address —
    // the same address the grid's cursor brackets.
    painter.text(
        header_rect.left_center() + egui::vec2(space::SM, 0.0),
        egui::Align2::LEFT_CENTER,
        "TRIG",
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        shade(INK_LEVEL, ground),
    );
    painter.text(
        header_rect.right_center() - egui::vec2(space::LG, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{:02}", selection.step + 1),
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        shade(LABEL_COLOR, ground),
    );
    let step = format!("{:02}", selection.step + 1);
    let bar = selection.tick / crate::ui::sequencer::grid_resolution::TICKS_PER_BAR + 1;
    let beat = selection.tick % crate::ui::sequencer::grid_resolution::TICKS_PER_BAR
        / (crate::ui::sequencer::grid_resolution::TICKS_PER_BAR / 4)
        + 1;
    let position = format!("{bar}.{beat}");
    let primary = selection.primary.as_ref();
    let pitch = primary.map(anchor_label).unwrap_or_else(|| "--".to_owned());
    let state = if primary.is_some_and(|note| note.enabled) {
        "ON"
    } else {
        "OFF"
    };
    let length = primary
        .map(|note| length_label(note.length_ticks))
        .unwrap_or_else(|| "--".to_owned());
    let velocity = primary
        .map(|note| note.velocity.to_string())
        .unwrap_or_else(|| "--".to_owned());
    let chance = format!(
        "{:.0}%",
        primary.map_or(1.0, |note| note.probability) * 100.0
    );
    let notes = format!("{:02}", selection.tone_count);
    let fields = [
        Field::new("STEP", &step, GridArea::new(0, 0, 1, 1)),
        Field::new("POSITION", &position, GridArea::new(1, 0, 1, 1)),
        Field::new("PITCH", &pitch, GridArea::new(0, 1, 1, 1)),
        Field::new("LENGTH", &length, GridArea::new(1, 1, 1, 1)),
        Field::new("VELOCITY", &velocity, GridArea::new(0, 2, 1, 1)),
        Field::new("CHANCE", &chance, GridArea::new(1, 2, 1, 1)),
        Field::new("NOTES", &notes, GridArea::new(0, 3, 1, 1)),
        Field::new("STATE", state, GridArea::new(1, 3, 1, 1)),
    ];

    let field_bounds = egui::Rect::from_min_max(
        header_rect.left_bottom() + egui::vec2(space::SM, space::SM),
        rect.right_bottom() - egui::vec2(space::SM, space::SM + SUM_HEIGHT),
    );
    let layout = LayoutGrid::new(field_bounds, FIELD_COLUMNS, FIELD_ROWS, egui::Vec2::ZERO);
    for field in fields {
        draw_field(
            &painter,
            layout.area(field.area),
            field.label,
            field.value,
            ground,
        );
    }

    // The display law: when two authorities compose a value, show the sum
    // AS a sum — anchor, deviation, substrate, one line.
    let sum_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + space::SM, rect.bottom() - SUM_HEIGHT),
        egui::pos2(rect.right() - space::SM, rect.bottom()),
    );
    if let Some(note) = primary {
        painter.text(
            sum_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            sum_line(note),
            egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
            shade(LABEL_COLOR, ground),
        );
    }
}

/// The anchor's name alone: the address half of the sum.
fn anchor_label(note: &crate::ui::sequencer::sequence::NoteView) -> String {
    match note.pitch.anchor {
        crate::pitch::Anchor::Degree { degree, period } => degree_label(degree, period),
        crate::pitch::Anchor::Absolute(_) => note_name(note.midi),
    }
}

/// `^3 +14¢ = 331.1HZ` — the whole identity of the pitch, spelled as the
/// sum it is. A zero deviation drops its term rather than printing +0.
fn sum_line(note: &crate::ui::sequencer::sequence::NoteView) -> String {
    let anchor = anchor_label(note);
    let cents = note.pitch.offset_cents;
    let deviation = if cents != 0.0 {
        format!(" {cents:+.0}¢")
    } else {
        String::new()
    };
    let push = if note.micro_ticks != 0 {
        format!(" {:+}T", note.micro_ticks)
    } else {
        String::new()
    };
    let approx = if note.approx {
        format!(" {}", crate::design::signs::APPROX)
    } else {
        String::new()
    };
    format!("{anchor}{deviation}{push} = {:.1}HZ{approx}", note.hz)
}

fn draw_field(
    painter: &egui::Painter,
    rect: egui::Rect,
    label: &str,
    value: &str,
    ground: Polarity,
) {
    painter.text(
        rect.left_top() + egui::vec2(space::SM, space::XS),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
        shade(LABEL_COLOR, ground),
    );
    painter.text(
        rect.left_bottom() + egui::vec2(space::SM, -space::XS),
        egui::Align2::LEFT_BOTTOM,
        value,
        egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
        shade(INK_LEVEL, ground),
    );
}
