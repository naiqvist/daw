//! The modulation patchbay: sources, destinations, and one wire's response.
//!
//! This is a workspace rather than a floating matrix.  The source is always
//! named, every destination keeps its channel/device path, and the response
//! chain is drawn from the same authored values and callback telemetry that
//! make the sound.  Pointer gestures call the same core operations as the
//! keyboard after painting, so the view never becomes a second authority.

use super::{chassis, palette};
use crate::PROFONT;
use crate::audio::modulation::{ModKind, ModShape, ModWire, bend_curve};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::stage::StageIntent;
use crate::ui::stage::modulation::{
    self, Focus, SourceField, Target, WireControl, rate_face, source_name, wire_control_face,
};
use eframe::egui;

const TYPE_PX: f32 = 12.0;
const INSET: f32 = 10.0;
const GAP: f32 = 8.0;
const WORK_HEAD_H: f32 = 30.0;
const ZONE_HEAD_H: f32 = 24.0;
const SOURCE_ZONE_H: f32 = 172.0;
const SOURCE_CARD_W: f32 = 218.0;
const SOURCE_GAP: f32 = 7.0;
const SOURCE_ACTION_W: f32 = 132.0;
const SOURCE_FIELD_H: f32 = 19.0;
const TRACK_H: f32 = 27.0;
const TARGET_ROW_H: f32 = 24.0;
const RESPONSE_FACT_H: f32 = 38.0;
const RESPONSE_PLOT_H: f32 = 112.0;
const CONTROL_ROW_H: f32 = 27.0;

#[derive(Clone, Copy, Debug)]
enum Action {
    Focus(Focus),
    AddLfo,
    AddFollower,
    SelectSource(usize),
    SourceField {
        source: usize,
        field: SourceField,
    },
    AdjustSource {
        source: usize,
        field: SourceField,
        forward: bool,
    },
    DeleteSource(usize),
    SelectTarget {
        track: usize,
        target: usize,
    },
    ToggleRoute {
        track: usize,
        target: usize,
    },
    EnsureRoute,
    SelectControl(WireControl),
    SetControl {
        control: WireControl,
        fraction: f32,
        begin_gesture: bool,
        end_gesture: bool,
    },
    ActivateControl(WireControl),
}

fn font() -> egui::FontId {
    egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()))
}

fn visible_span(cursor: usize, len: usize, capacity: usize) -> (usize, usize) {
    let capacity = capacity.max(1).min(len.max(1));
    if len <= capacity {
        return (0, len);
    }
    let start = cursor
        .saturating_sub(capacity / 2)
        .min(len.saturating_sub(capacity));
    (start, (start + capacity).min(len))
}

fn draw_focus_tabs(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    panel: &modulation::Panel,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    let painter = ui.painter().clone();
    let labels = [
        (Focus::Sources, "01 SOURCES"),
        (Focus::Targets, "02 DESTINATIONS"),
        (Focus::Response, "03 RESPONSE"),
    ];
    let width = rect.width() / labels.len() as f32;
    for (index, (focus, label)) in labels.into_iter().enumerate() {
        let tab = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + index as f32 * width, rect.min.y),
            egui::pos2(rect.min.x + (index + 1) as f32 * width - 2.0, rect.max.y),
        );
        if button(
            ui,
            &painter,
            ("mod-focus-tab", index),
            tab,
            label,
            panel.focus == focus,
            true,
            if panel.focus == focus { c.alert } else { c.dir },
        )
        .clicked()
        {
            *action = Some(Action::Focus(focus));
        }
    }
}

fn fit_text(text: &str, width: f32) -> String {
    let capacity = (width.max(0.0) / (TYPE_PX * 0.6)).floor() as usize;
    if text.chars().count() <= capacity {
        return text.to_owned();
    }
    if capacity <= 2 {
        return "·".repeat(capacity);
    }
    let mut out: String = text.chars().take(capacity - 2).collect();
    out.push_str("··");
    out
}

fn signed(value: Option<f32>) -> String {
    value
        .filter(|value| value.is_finite())
        .map_or_else(|| "--".to_owned(), |value| format!("{value:+.3}"))
}

#[allow(clippy::too_many_arguments)]
fn button(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: egui::Rect,
    label: &str,
    selected: bool,
    enabled: bool,
    ink: egui::Color32,
) -> egui::Response {
    let c = palette::colours();
    let response = ui
        .interact(
            rect,
            ui.id().with(id),
            if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        )
        .affords(if enabled {
            Affords::Press
        } else {
            Affords::Refuse
        });
    if selected || response.hovered() {
        painter.rect_filled(rect, 0.0, if selected { c.select } else { c.panel });
    }
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(
            1.0,
            if selected {
                c.chassis
            } else if response.hovered() {
                c.edge
            } else {
                c.rule
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font(),
        if enabled { ink } else { c.dim },
    );
    response
}

fn zone_head(
    painter: &egui::Painter,
    rect: egui::Rect,
    number: &str,
    label: &str,
    fact: &str,
    focused: bool,
) {
    let c = palette::colours();
    let head = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x, (rect.min.y + ZONE_HEAD_H).min(rect.max.y)),
    );
    chassis::instrument_rail(painter, head);
    painter.text(
        egui::pos2(head.min.x + 7.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{number} / {label}"),
        font(),
        if focused { c.bright } else { c.label },
    );
    painter.text(
        egui::pos2(head.max.x - 7.0, head.center().y),
        egui::Align2::RIGHT_CENTER,
        fact,
        font(),
        if focused { c.alert } else { c.dim },
    );
}

fn draw_lfo_shape(
    painter: &egui::Painter,
    rect: egui::Rect,
    shape: ModShape,
    measured: Option<f32>,
) {
    let c = palette::colours();
    let mid = rect.center().y.round() - 0.5;
    // The curve is a SHAPE reference, not a phase scope. Live telemetry only
    // contains the current bipolar output, so give it its own vertical meter
    // instead of inventing an x/phase coordinate it does not have.
    let plot_right = (rect.max.x - 9.0).max(rect.min.x);
    painter.line_segment(
        [egui::pos2(rect.min.x, mid), egui::pos2(plot_right, mid)],
        egui::Stroke::new(1.0, c.rule),
    );
    let points: Vec<_> = (0..=64)
        .map(|at| {
            let t = at as f32 / 64.0;
            egui::pos2(
                rect.min.x + (plot_right - rect.min.x) * t,
                rect.center().y - shape.wave(t) * (rect.height() * 0.42),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.2, c.chassis)));
    if let Some(value) = measured.filter(|value| value.is_finite()) {
        let x = rect.max.x - 3.0;
        let y = rect.center().y - value.clamp(-1.0, 1.0) * (rect.height() * 0.42);
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0, c.rule),
        );
        painter.line_segment(
            [egui::pos2(x - 5.0, y), egui::pos2(x + 2.0, y)],
            egui::Stroke::new(1.5, c.nominal),
        );
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(5.0, 5.0)),
            0.0,
            c.nominal,
        );
    }
}

fn draw_follower(painter: &egui::Painter, rect: egui::Rect, measured: Option<f32>) {
    let c = palette::colours();
    let y = rect.center().y.round() - 0.5;
    painter.line_segment(
        [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
        egui::Stroke::new(2.0, c.rule),
    );
    let Some(value) = measured.filter(|value| value.is_finite()) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "AWAITING LEVEL",
            font(),
            c.dim,
        );
        return;
    };
    let x = rect.min.x + rect.width() * value.clamp(0.0, 1.0);
    painter.line_segment(
        [egui::pos2(rect.min.x, y), egui::pos2(x, y)],
        egui::Stroke::new(3.0, c.nominal),
    );
    painter.line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(1.0, c.nominal),
    );
}

fn source_field_value(kind: ModKind, field: SourceField) -> String {
    match (kind, field) {
        (ModKind::Lfo { shape, .. }, SourceField::Shape) => shape.label().to_uppercase(),
        (kind @ ModKind::Lfo { .. }, SourceField::Rate) => rate_face(kind),
        (ModKind::Lfo { free, .. }, SourceField::Mode) => {
            if free { "FREE" } else { "SYNC" }.to_owned()
        }
        (ModKind::Follower { track }, SourceField::Shape) => format!("TRACK {:02}", track + 1),
        (ModKind::Follower { .. }, SourceField::Rate) => "ENVELOPE".to_owned(),
        (ModKind::Follower { .. }, SourceField::Mode) => "AUDIO".to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_source_card(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    index: usize,
    name: &str,
    kind: ModKind,
    routes: usize,
    measured: Option<f32>,
    selected: bool,
    panel_field: SourceField,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.ground);
    chassis::frame(painter, rect, selected);

    let header = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 5.0, rect.min.y + 4.0),
        egui::pos2(rect.max.x - 5.0, rect.min.y + 25.0),
    );
    let delete =
        egui::Rect::from_min_max(egui::pos2(header.max.x - 19.0, header.min.y), header.max);
    let select = egui::Rect::from_min_max(header.min, egui::pos2(delete.min.x - 2.0, header.max.y));
    let response = ui
        .interact(
            select,
            ui.id().with(("mod-source", index)),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    if response.hovered() {
        painter.rect_filled(select, 0.0, c.panel);
    }
    if response.clicked() {
        *action = Some(Action::SelectSource(index));
    }
    painter.text(
        egui::pos2(select.min.x + 2.0, select.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font(),
        if selected { c.bright } else { c.dir },
    );
    painter.text(
        egui::pos2(select.max.x - 2.0, select.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{routes:02} PATCH"),
        font(),
        if routes > 0 { c.nominal } else { c.dim },
    );
    let delete_response = button(
        ui,
        painter,
        ("mod-source-delete", index),
        delete,
        "×",
        false,
        true,
        c.dim,
    );
    if delete_response.clicked() {
        *action = Some(Action::DeleteSource(index));
    }

    let wave = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 8.0, header.max.y + 3.0),
        egui::pos2(rect.max.x - 8.0, header.max.y + 43.0),
    );
    painter.rect_filled(wave, 0.0, c.panel);
    match kind {
        ModKind::Lfo { shape, .. } => {
            draw_lfo_shape(painter, wave.shrink2(egui::vec2(4.0, 4.0)), shape, measured)
        }
        ModKind::Follower { .. } => {
            draw_follower(painter, wave.shrink2(egui::vec2(4.0, 7.0)), measured)
        }
    }
    painter.text(
        egui::pos2(wave.max.x - 3.0, wave.min.y + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("OUT {}", signed(measured)),
        font(),
        measured.map_or(c.dim, |_| c.nominal),
    );

    let fields_top = wave.max.y + 3.0;
    for (row, field) in SourceField::ALL.into_iter().enumerate() {
        let field_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 7.0, fields_top + row as f32 * SOURCE_FIELD_H),
            egui::pos2(
                rect.max.x - 7.0,
                fields_top + (row + 1) as f32 * SOURCE_FIELD_H,
            ),
        );
        if field_rect.max.y > rect.max.y - 3.0 {
            break;
        }
        let on = selected && panel_field == field;
        if on {
            painter.rect_filled(field_rect, 0.0, c.select);
        }
        let arrow_w = 19.0;
        let left = egui::Rect::from_min_max(
            egui::pos2(field_rect.max.x - arrow_w * 2.0, field_rect.min.y),
            egui::pos2(field_rect.max.x - arrow_w, field_rect.max.y),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(field_rect.max.x - arrow_w, field_rect.min.y),
            field_rect.max,
        );
        let body =
            egui::Rect::from_min_max(field_rect.min, egui::pos2(left.min.x, field_rect.max.y));
        let response = ui
            .interact(
                body,
                ui.id().with(("mod-source-field", index, row)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if response.hovered() && !on {
            painter.rect_filled(body, 0.0, c.panel);
        }
        if response.double_clicked() {
            *action = Some(Action::AdjustSource {
                source: index,
                field,
                forward: true,
            });
        } else if response.clicked() {
            *action = Some(Action::SourceField {
                source: index,
                field,
            });
        }
        painter.text(
            egui::pos2(body.min.x + 2.0, body.center().y),
            egui::Align2::LEFT_CENTER,
            field.label(),
            font(),
            if on { c.bright } else { c.label },
        );
        painter.text(
            egui::pos2(body.max.x - 3.0, body.center().y),
            egui::Align2::RIGHT_CENTER,
            source_field_value(kind, field),
            font(),
            if on { c.alert } else { c.fg },
        );
        let adjustable = matches!(kind, ModKind::Lfo { .. });
        for (forward, arrow, label) in [(false, left, "‹"), (true, right, "›")] {
            let response = button(
                ui,
                painter,
                ("mod-source-step", index, row, forward),
                arrow,
                label,
                false,
                adjustable,
                if on { c.alert } else { c.dim },
            );
            if response.clicked() {
                *action = Some(Action::AdjustSource {
                    source: index,
                    field,
                    forward,
                });
            }
        }
        if on {
            let signature = measured.map_or(
                crate::ui::nav_cursor::Signature::Plain,
                crate::ui::nav_cursor::Signature::Sweep,
            );
            crate::ui::nav_cursor::claim_signed(
                painter,
                ("mod-source-field", index, row),
                field_rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Overlay,
                c.alert,
                signature,
            );
        }
    }
}

fn draw_source_zone(
    stage: &super::super::Stage,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    panel: &modulation::Panel,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    chassis::frame(ui.painter(), rect, panel.focus == Focus::Sources);
    zone_head(
        ui.painter(),
        rect,
        "01",
        "SOURCES",
        &format!("{:02}/16", stage.song.modulators.len()),
        panel.focus == Focus::Sources,
    );
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 6.0, rect.min.y + ZONE_HEAD_H + 5.0),
        egui::pos2(rect.max.x - 6.0, rect.max.y - 6.0),
    );
    let actions = egui::Rect::from_min_max(
        egui::pos2((body.max.x - SOURCE_ACTION_W).max(body.min.x), body.min.y),
        body.max,
    );
    let cards = egui::Rect::from_min_max(
        body.min,
        egui::pos2((actions.min.x - GAP).max(body.min.x), body.max.y),
    );
    let capacity =
        (((cards.width() + SOURCE_GAP) / (SOURCE_CARD_W + SOURCE_GAP)).floor() as usize).max(1);
    let (start, end) = visible_span(panel.source, stage.song.modulators.len(), capacity);
    if !stage.song.modulators.is_empty() && ui.rect_contains_pointer(cards) {
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let steps = ((wheel.abs() / 36.0).ceil() as usize).clamp(1, 4);
            let source = if wheel < 0.0 {
                panel
                    .source
                    .saturating_add(steps)
                    .min(stage.song.modulators.len() - 1)
            } else {
                panel.source.saturating_sub(steps)
            };
            *action = Some(Action::SelectSource(source));
            ui.ctx()
                .input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        }
    }
    let shown = end.saturating_sub(start).max(1);
    let card_w =
        ((cards.width() - SOURCE_GAP * shown.saturating_sub(1) as f32) / shown as f32).max(80.0);
    let clip = ui.painter().with_clip_rect(cards);
    for (slot, index) in (start..end).enumerate() {
        let source = stage.song.modulators[index];
        let source_rect = egui::Rect::from_min_size(
            egui::pos2(
                cards.min.x + slot as f32 * (card_w + SOURCE_GAP),
                cards.min.y,
            ),
            egui::vec2(card_w, cards.height()),
        );
        let measured = stage.mod_source_values.get(&source.id).copied();
        let routes = stage
            .song
            .mod_wires
            .iter()
            .filter(|wire| wire.source == source.id)
            .count();
        draw_source_card(
            ui,
            &clip,
            source_rect,
            index,
            &source_name(&stage.song, index),
            source.kind,
            routes,
            measured,
            index == panel.source,
            panel.source_field,
            action,
        );
    }
    if stage.song.modulators.is_empty() {
        clip.text(
            cards.center(),
            egui::Align2::CENTER_CENTER,
            "NO SOURCES // ADD AN LFO OR FOLLOW A CHANNEL",
            font(),
            c.dim,
        );
    }

    let lfo = egui::Rect::from_min_size(actions.min, egui::vec2(actions.width(), 30.0));
    let follower = lfo.translate(egui::vec2(0.0, 36.0));
    let action_painter = ui.painter().clone();
    if button(
        ui,
        &action_painter,
        "mod-add-lfo",
        lfo,
        "+ LFO",
        false,
        stage.song.modulators.len() < 16,
        c.bright,
    )
    .clicked()
    {
        *action = Some(Action::AddLfo);
    }
    if button(
        ui,
        &action_painter,
        "mod-add-follower",
        follower,
        "+ FOLLOW",
        false,
        !stage.song.tracks.is_empty() && stage.song.modulators.len() < 16,
        c.bright,
    )
    .clicked()
    {
        *action = Some(Action::AddFollower);
    }
    let source_window = if stage.song.modulators.is_empty() {
        "--".to_owned()
    } else {
        format!("{:02}–{:02}", start + 1, end)
    };
    let source_nav = egui::Rect::from_min_max(
        egui::pos2(actions.min.x, follower.max.y + 5.0),
        egui::pos2(actions.max.x, follower.max.y + 27.0),
    );
    let previous = egui::Rect::from_min_max(
        source_nav.min,
        egui::pos2(source_nav.min.x + 22.0, source_nav.max.y),
    );
    let next = egui::Rect::from_min_max(
        egui::pos2(source_nav.max.x - 22.0, source_nav.min.y),
        source_nav.max,
    );
    if button(
        ui,
        &action_painter,
        "mod-source-previous",
        previous,
        "‹",
        false,
        panel.source > 0,
        c.dim,
    )
    .clicked()
    {
        *action = Some(Action::SelectSource(panel.source - 1));
    }
    if button(
        ui,
        &action_painter,
        "mod-source-next",
        next,
        "›",
        false,
        panel.source + 1 < stage.song.modulators.len(),
        c.dim,
    )
    .clicked()
    {
        *action = Some(Action::SelectSource(panel.source + 1));
    }
    ui.painter().text(
        source_nav.center(),
        egui::Align2::CENTER_CENTER,
        source_window,
        font(),
        c.label,
    );
    ui.painter().text(
        egui::pos2(actions.min.x, actions.max.y - 26.0),
        egui::Align2::LEFT_CENTER,
        "↑↓ SOURCE · ←→ FIELD",
        font(),
        c.dim,
    );
    ui.painter().text(
        egui::pos2(actions.min.x, actions.max.y - 5.0),
        egui::Align2::LEFT_BOTTOM,
        "ENTER CHANGE · DEL REMOVE",
        font(),
        c.dim,
    );
}

fn target_path(target: &Target) -> String {
    format!("{} / {}", target.group, target.name)
}

#[allow(clippy::too_many_arguments)]
fn draw_target_row(
    stage: &super::super::Stage,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    panel: &modulation::Panel,
    target: &Target,
    target_index: usize,
    source: Option<u64>,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    let selected = panel.focus == Focus::Targets
        && panel.track == target.track
        && panel.target == target_index;
    let wire = source.and_then(|source| {
        stage.song.mod_wires.iter().find(|wire| {
            wire.source == source && wire.track == target.track && wire.target == target.id
        })
    });
    if selected {
        ui.painter().rect_filled(rect, 0.0, c.select);
    }
    let patch_w = 63.0;
    let value_w = 72.0;
    let patch = egui::Rect::from_min_max(
        egui::pos2(rect.max.x - patch_w, rect.min.y + 2.0),
        egui::pos2(rect.max.x, rect.max.y - 2.0),
    );
    let value = egui::Rect::from_min_max(
        egui::pos2(patch.min.x - value_w, rect.min.y),
        egui::pos2(patch.min.x - 3.0, rect.max.y),
    );
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(value.min.x - 3.0, rect.max.y));
    let response = ui
        .interact(
            body,
            ui.id().with(("mod-target", target.track, target_index)),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    if response.hovered() && !selected {
        ui.painter().rect_filled(body, 0.0, c.panel);
    }
    if response.clicked() {
        *action = Some(Action::SelectTarget {
            track: target.track,
            target: target_index,
        });
    }
    let marker_x = body.min.x + 5.0;
    ui.painter().line_segment(
        [
            egui::pos2(marker_x, rect.min.y + 5.0),
            egui::pos2(marker_x, rect.max.y - 5.0),
        ],
        egui::Stroke::new(
            if wire.is_some() { 2.0 } else { 1.0 },
            wire.map_or(c.rule, |wire| if wire.enabled { c.nominal } else { c.dim }),
        ),
    );
    ui.painter().text(
        egui::pos2(body.min.x + 13.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        fit_text(&target_path(target), body.width() - 16.0),
        font(),
        if selected { c.bright } else { c.fg },
    );
    let reading = wire.map_or_else(
        || target.face(target.base),
        |wire| wire_control_face(wire, WireControl::Depth),
    );
    ui.painter().text(
        egui::pos2(value.max.x, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        reading,
        font(),
        wire.map_or(c.dim, |wire| if wire.enabled { c.nominal } else { c.dim }),
    );
    let (word, ink) = match wire {
        Some(wire) if wire.enabled => ("CUT", c.nominal),
        Some(_) => ("CUT", c.dim),
        None => ("PATCH", c.dir),
    };
    let patch_painter = ui.painter().clone();
    let patch_response = button(
        ui,
        &patch_painter,
        ("mod-patch", target.track, target_index),
        patch,
        word,
        wire.is_some(),
        source.is_some() && (wire.is_some() || stage.song.mod_wires.len() < 64),
        ink,
    );
    if patch_response.clicked() {
        *action = Some(Action::ToggleRoute {
            track: target.track,
            target: target_index,
        });
    }
    if let Some(wire) = wire
        && let Some(live) = stage.mod_wire_values.get(&wire.id).copied()
        && live.is_finite()
    {
        // Contribution units differ by target (linear units versus octaves
        // for ratio controls). This saturating indicator communicates sign
        // and activity without pretending they share one absolute scale.
        let magnitude = (live.abs() / (live.abs() + 1.0)).sqrt();
        let t = (live.signum() * magnitude + 1.0) * 0.5;
        let x = value.min.x + value.width() * t.clamp(0.0, 1.0);
        ui.painter().rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, rect.max.y - 2.0), egui::vec2(4.0, 4.0)),
            0.0,
            c.nominal,
        );
    }
    if selected {
        crate::ui::nav_cursor::claim(
            ui.painter(),
            ("mod-target", target.track, target_index),
            rect,
            crate::ui::nav_cursor::Kind::Row,
            crate::ui::nav_cursor::Layer::Overlay,
            c.alert,
        );
    }
}

fn draw_target_zone(
    stage: &super::super::Stage,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    panel: &modulation::Panel,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    chassis::frame(ui.painter(), rect, panel.focus == Focus::Targets);
    let source = panel.source_id(&stage.song);
    let orphaned = source.map_or(0, |source| {
        stage
            .song
            .mod_wires
            .iter()
            .filter(|wire| wire.source == source)
            .filter(|wire| {
                !modulation::targets(&stage.song, wire.track)
                    .iter()
                    .any(|target| target.id == wire.target)
            })
            .count()
    });
    let fact = if orphaned > 0 {
        format!("{} PATCH · {orphaned} ORPHAN", stage.song.mod_wires.len())
    } else {
        format!("{:02}/64 PATCH", stage.song.mod_wires.len())
    };
    zone_head(
        ui.painter(),
        rect,
        "02",
        "DESTINATIONS",
        &fact,
        panel.focus == Focus::Targets,
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 6.0, rect.min.y + ZONE_HEAD_H + 4.0),
        egui::pos2(rect.max.x - 6.0, rect.max.y - 5.0),
    );
    let tracks = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.max.x, (inner.min.y + TRACK_H).min(inner.max.y)),
    );
    let track_capacity = (tracks.width() / 112.0).floor().max(1.0) as usize;
    let (track_start, track_end) =
        visible_span(panel.track, stage.song.tracks.len(), track_capacity);
    if !stage.song.tracks.is_empty() && ui.rect_contains_pointer(tracks) {
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let track = if wheel < 0.0 {
                (panel.track + 1).min(stage.song.tracks.len() - 1)
            } else {
                panel.track.saturating_sub(1)
            };
            *action = Some(Action::SelectTarget { track, target: 0 });
            ui.ctx()
                .input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        }
    }
    let track_count = track_end.saturating_sub(track_start).max(1);
    let track_w = tracks.width() / track_count as f32;
    let track_painter = ui.painter().clone();
    for (slot, track) in (track_start..track_end).enumerate() {
        let tab = egui::Rect::from_min_max(
            egui::pos2(tracks.min.x + slot as f32 * track_w, tracks.min.y),
            egui::pos2(
                tracks.min.x + (slot + 1) as f32 * track_w - 2.0,
                tracks.max.y,
            ),
        );
        let label = format!(
            "{:02} {}",
            track + 1,
            fit_text(&stage.song.tracks[track].name, tab.width() - 42.0)
        );
        if button(
            ui,
            &track_painter,
            ("mod-track", track),
            tab,
            &label,
            track == panel.track,
            true,
            if track == panel.track {
                c.bright
            } else {
                c.dir
            },
        )
        .clicked()
        {
            *action = Some(Action::SelectTarget { track, target: 0 });
        }
    }
    if stage.song.tracks.len() > track_capacity {
        let rail = egui::Rect::from_min_max(
            egui::pos2(tracks.min.x + 1.0, tracks.max.y + 0.5),
            egui::pos2(tracks.max.x - 1.0, tracks.max.y + 2.5),
        );
        let thumb_w = (rail.width() * track_capacity as f32 / stage.song.tracks.len() as f32)
            .clamp(14.0, rail.width());
        let travel = (rail.width() - thumb_w).max(0.0);
        let at = panel.track as f32 / stage.song.tracks.len().saturating_sub(1).max(1) as f32;
        let thumb = egui::Rect::from_min_size(
            egui::pos2(rail.min.x + travel * at, rail.min.y),
            egui::vec2(thumb_w, rail.height()),
        );
        let scroll = ui
            .interact(
                rail.expand2(egui::vec2(0.0, 4.0)),
                ui.id().with("mod-track-scroll"),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Sweep);
        ui.painter().rect_filled(rail, 0.0, c.rule);
        ui.painter().rect_filled(
            thumb,
            0.0,
            if scroll.hovered() {
                c.bright
            } else {
                c.chassis
            },
        );
        if (scroll.clicked() || scroll.dragged())
            && let Some(pointer) = scroll.interact_pointer_pos()
        {
            let fraction = ((pointer.x - rail.min.x) / rail.width().max(1.0)).clamp(0.0, 1.0);
            *action = Some(Action::SelectTarget {
                track: ((stage.song.tracks.len() - 1) as f32 * fraction).round() as usize,
                target: 0,
            });
        }
    }

    let columns = egui::Rect::from_min_max(
        egui::pos2(inner.min.x, tracks.max.y + 3.0),
        egui::pos2(inner.max.x, tracks.max.y + 21.0),
    );
    ui.painter().text(
        egui::pos2(columns.min.x + 13.0, columns.center().y),
        egui::Align2::LEFT_CENTER,
        "SIGNAL PATH / PARAMETER",
        font(),
        c.label,
    );
    ui.painter().text(
        egui::pos2(columns.max.x - 67.0, columns.center().y),
        egui::Align2::RIGHT_CENTER,
        "BASE / DEPTH",
        font(),
        c.label,
    );
    let footer_h = 18.0;
    let rows = egui::Rect::from_min_max(
        egui::pos2(inner.min.x, columns.max.y),
        egui::pos2(inner.max.x, (inner.max.y - footer_h).max(columns.max.y)),
    );
    let targets = modulation::targets(&stage.song, panel.track);
    let capacity = (rows.height() / TARGET_ROW_H).floor().max(1.0) as usize;
    let (start, end) = visible_span(panel.target, targets.len(), capacity);
    // The destination catalog can be much taller than the room. The wheel
    // moves the same selection the arrows do, so pointer and keyboard users
    // share one focus model and the selected row remains in view.
    if !targets.is_empty() && ui.rect_contains_pointer(rows) {
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let steps = ((wheel.abs() / 36.0).ceil() as usize).clamp(1, 6);
            let target = if wheel < 0.0 {
                panel.target.saturating_add(steps).min(targets.len() - 1)
            } else {
                panel.target.saturating_sub(steps)
            };
            *action = Some(Action::SelectTarget {
                track: panel.track,
                target,
            });
            ui.ctx()
                .input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        }
    }
    let rows_painter = ui.painter().with_clip_rect(rows);
    for (shown, target_index) in (start..end).enumerate() {
        let row = egui::Rect::from_min_max(
            egui::pos2(rows.min.x, rows.min.y + shown as f32 * TARGET_ROW_H),
            egui::pos2(
                rows.max.x,
                (rows.min.y + (shown + 1) as f32 * TARGET_ROW_H - 1.0).min(rows.max.y),
            ),
        );
        rows_painter.line_segment(
            [row.left_bottom(), row.right_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );
        draw_target_row(
            stage,
            ui,
            row,
            panel,
            &targets[target_index],
            target_index,
            source,
            action,
        );
    }
    if targets.is_empty() {
        ui.painter().text(
            rows.center(),
            egui::Align2::CENTER_CENTER,
            "NO DESTINATIONS ON THIS CHANNEL",
            font(),
            c.dim,
        );
    } else if targets.len() > capacity {
        let rail = egui::Rect::from_min_max(
            egui::pos2(rows.max.x - 3.0, rows.min.y + 2.0),
            egui::pos2(rows.max.x - 1.0, rows.max.y - 2.0),
        );
        let thumb_h =
            (rail.height() * capacity as f32 / targets.len() as f32).clamp(12.0, rail.height());
        let travel = (rail.height() - thumb_h).max(0.0);
        let at = panel.target as f32 / targets.len().saturating_sub(1).max(1) as f32;
        let thumb = egui::Rect::from_min_size(
            egui::pos2(rail.min.x, rail.min.y + travel * at),
            egui::vec2(rail.width(), thumb_h),
        );
        let scroll = ui
            .interact(
                rail.expand2(egui::vec2(5.0, 0.0)),
                ui.id().with("mod-target-scroll"),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Sweep);
        ui.painter().rect_filled(rail, 0.0, c.rule);
        ui.painter().rect_filled(
            thumb,
            0.0,
            if scroll.hovered() {
                c.bright
            } else {
                c.chassis
            },
        );
        if (scroll.clicked() || scroll.dragged())
            && let Some(pointer) = scroll.interact_pointer_pos()
        {
            let fraction = ((pointer.y - rail.min.y) / rail.height().max(1.0)).clamp(0.0, 1.0);
            *action = Some(Action::SelectTarget {
                track: panel.track,
                target: ((targets.len() - 1) as f32 * fraction).round() as usize,
            });
        }
    }
    ui.painter().text(
        egui::pos2(inner.min.x, inner.max.y),
        egui::Align2::LEFT_BOTTOM,
        "←→ CHANNEL · ↑↓ DESTINATION · ENTER PATCH/INSPECT",
        font(),
        c.dim,
    );
    let range = if targets.is_empty() {
        "WINDOW --".to_owned()
    } else {
        format!("WINDOW {:02}-{:02}/{:02}", start + 1, end, targets.len())
    };
    ui.painter().text(
        egui::pos2(inner.max.x, inner.max.y),
        egui::Align2::RIGHT_BOTTOM,
        if orphaned > 0 {
            format!("{range} · {orphaned} ORPHAN")
        } else {
            range
        },
        font(),
        if orphaned > 0 { c.fault } else { c.dim },
    );
}

fn control_fraction(wire: &ModWire, control: WireControl) -> f32 {
    match control {
        WireControl::Depth => (wire.depth + 1.0) * 0.5,
        WireControl::Curve => (wire.curve + 1.0) * 0.5,
        WireControl::Steps => wire.steps as f32 / 64.0,
        WireControl::Smooth => (wire.smooth_ms / 2_000.0).clamp(0.0, 1.0).sqrt(),
        WireControl::Enabled => f32::from(wire.enabled),
        WireControl::Solo => f32::from(wire.solo),
    }
    .clamp(0.0, 1.0)
}

fn draw_scope(
    painter: &egui::Painter,
    rect: egui::Rect,
    samples: Option<&std::collections::VecDeque<f32>>,
) {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.ground);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c.rule),
        egui::StrokeKind::Inside,
    );
    let plot = rect.shrink2(egui::vec2(6.0, 15.0));
    let mid = plot.center().y.round() - 0.5;
    painter.line_segment(
        [egui::pos2(plot.min.x, mid), egui::pos2(plot.max.x, mid)],
        egui::Stroke::new(1.0, c.rule),
    );
    painter.text(
        egui::pos2(rect.min.x + 5.0, rect.min.y + 4.0),
        egui::Align2::LEFT_TOP,
        "ENGINE OUTPUT / RECENT BLOCKS",
        font(),
        c.label,
    );
    let Some(samples) = samples.filter(|samples| samples.len() > 1) else {
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "AWAITING ENGINE TELEMETRY",
            font(),
            c.dim,
        );
        return;
    };
    let peak = samples
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f32::abs)
        .fold(0.0, f32::max)
        .max(0.000_1);
    let points: Vec<_> = samples
        .iter()
        .enumerate()
        .filter_map(|(at, value)| {
            value.is_finite().then(|| {
                let t = at as f32 / (samples.len() - 1) as f32;
                egui::pos2(
                    plot.min.x + plot.width() * t,
                    plot.center().y - (value / peak).clamp(-1.0, 1.0) * plot.height() * 0.46,
                )
            })
        })
        .collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.4, c.nominal)));
    }
    painter.text(
        egui::pos2(rect.max.x - 5.0, rect.min.y + 4.0),
        egui::Align2::RIGHT_TOP,
        format!("PEAK {peak:.3}"),
        font(),
        c.nominal,
    );
}

fn draw_transfer(painter: &egui::Painter, rect: egui::Rect, wire: &ModWire) {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.ground);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c.rule),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.min.x + 5.0, rect.min.y + 4.0),
        egui::Align2::LEFT_TOP,
        "TRANSFER",
        font(),
        c.label,
    );
    let plot = rect.shrink2(egui::vec2(8.0, 15.0));
    painter.line_segment(
        [
            egui::pos2(plot.min.x, plot.center().y),
            egui::pos2(plot.max.x, plot.center().y),
        ],
        egui::Stroke::new(1.0, c.rule),
    );
    painter.line_segment(
        [
            egui::pos2(plot.center().x, plot.min.y),
            egui::pos2(plot.center().x, plot.max.y),
        ],
        egui::Stroke::new(1.0, c.rule),
    );
    let points: Vec<_> = (0..=64)
        .map(|at| {
            let t = at as f32 / 64.0;
            let mut shaped = bend_curve(t, wire.curve);
            if wire.steps > 1 {
                let steps = (wire.steps - 1) as f32;
                shaped = (shaped * steps).round() / steps;
            }
            let output = (shaped * 2.0 - 1.0) * wire.depth;
            egui::pos2(
                plot.min.x + plot.width() * t,
                plot.center().y - output.clamp(-1.0, 1.0) * plot.height() * 0.48,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.3, c.chassis)));
}

#[allow(clippy::too_many_arguments)]
fn draw_control_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    control: WireControl,
    wire: &ModWire,
    selected: bool,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    if selected {
        ui.painter().rect_filled(rect, 0.0, c.select);
    }
    let label_w = 70.0;
    let value_w = 72.0;
    let label = egui::Rect::from_min_max(
        rect.min,
        egui::pos2((rect.min.x + label_w).min(rect.max.x), rect.max.y),
    );
    let value = egui::Rect::from_min_max(
        egui::pos2((rect.max.x - value_w).max(label.max.x), rect.min.y),
        rect.max,
    );
    let rail = egui::Rect::from_min_max(
        egui::pos2(label.max.x + 4.0, rect.center().y - 3.0),
        egui::pos2(value.min.x - 5.0, rect.center().y + 3.0),
    );
    let label_response = ui
        .interact(
            label,
            ui.id().with(("mod-control-label", control.label())),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    if label_response.hovered() && !selected {
        ui.painter().rect_filled(label, 0.0, c.panel);
    }
    if label_response.clicked() {
        *action = Some(Action::SelectControl(control));
    }
    ui.painter().text(
        egui::pos2(label.min.x + 3.0, label.center().y),
        egui::Align2::LEFT_CENTER,
        control.label(),
        font(),
        if selected { c.bright } else { c.label },
    );
    let fraction = control_fraction(wire, control);
    if matches!(control, WireControl::Enabled | WireControl::Solo) {
        let switch = rail.shrink2(egui::vec2(0.0, -3.0));
        let response = ui
            .interact(
                switch,
                ui.id().with(("mod-control-switch", control.label())),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        ui.painter().rect_filled(switch, 0.0, c.panel);
        let on = fraction >= 0.5;
        let half = switch.width() * 0.5;
        let live = if on {
            egui::Rect::from_min_max(egui::pos2(switch.max.x - half, switch.min.y), switch.max)
        } else {
            egui::Rect::from_min_size(switch.min, egui::vec2(half, switch.height()))
        };
        ui.painter()
            .rect_filled(live, 0.0, if on { c.nominal } else { c.edge });
        if response.hovered() {
            ui.painter().rect_stroke(
                switch,
                0.0,
                egui::Stroke::new(1.0, c.chassis),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            *action = Some(Action::ActivateControl(control));
        }
    } else if rail.width() > 4.0 {
        let hit = rail.expand2(egui::vec2(0.0, 7.0));
        let response = ui
            .interact(
                hit,
                ui.id().with(("mod-control-rail", control.label())),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Sweep);
        ui.painter().rect_filled(rail, 0.0, c.rule);
        let x = rail.min.x + rail.width() * fraction;
        if matches!(control, WireControl::Depth | WireControl::Curve) {
            let centre = rail.center().x;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(centre.min(x), rail.min.y),
                    egui::pos2(centre.max(x), rail.max.y),
                ),
                0.0,
                c.chassis,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(centre, rail.min.y - 3.0),
                    egui::pos2(centre, rail.max.y + 3.0),
                ],
                egui::Stroke::new(1.0, c.edge),
            );
        } else {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(rail.min, egui::pos2(x, rail.max.y)),
                0.0,
                c.chassis,
            );
        }
        ui.painter().rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(x, rail.center().y),
                egui::vec2(if response.hovered() { 7.0 } else { 5.0 }, 12.0),
            ),
            0.0,
            if response.hovered() {
                c.alert
            } else {
                c.bright
            },
        );
        if response.clicked()
            || response.dragged()
            || response.drag_started()
            || response.drag_stopped()
        {
            let next = response.interact_pointer_pos().map_or(fraction, |pointer| {
                (pointer.x - rail.min.x) / rail.width().max(1.0)
            });
            *action = Some(Action::SetControl {
                control,
                fraction: next,
                begin_gesture: response.drag_started(),
                end_gesture: response.drag_stopped(),
            });
        }
    }
    let value_response = ui
        .interact(
            value,
            ui.id().with(("mod-control-value", control.label())),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    if value_response.hovered() {
        ui.painter().rect_filled(value, 0.0, c.panel);
    }
    if value_response.double_clicked() {
        *action = Some(Action::ActivateControl(control));
    } else if value_response.clicked() {
        *action = Some(Action::SelectControl(control));
    }
    ui.painter().text(
        egui::pos2(value.max.x - 3.0, value.center().y),
        egui::Align2::RIGHT_CENTER,
        wire_control_face(wire, control),
        font(),
        if selected { c.alert } else { c.fg },
    );
    if selected {
        let signature = match control {
            WireControl::Depth => crate::ui::nav_cursor::Signature::Sweep(wire.depth),
            WireControl::Curve => crate::ui::nav_cursor::Signature::Sweep(wire.curve),
            WireControl::Steps | WireControl::Smooth => {
                crate::ui::nav_cursor::Signature::Level(fraction)
            }
            WireControl::Enabled | WireControl::Solo => crate::ui::nav_cursor::Signature::Plain,
        };
        crate::ui::nav_cursor::claim_signed(
            ui.painter(),
            ("mod-control", control.label()),
            rect,
            crate::ui::nav_cursor::Kind::Row,
            crate::ui::nav_cursor::Layer::Overlay,
            c.alert,
            signature,
        );
    }
}

fn draw_response_zone(
    stage: &super::super::Stage,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    panel: &modulation::Panel,
    action: &mut Option<Action>,
) {
    let c = palette::colours();
    chassis::frame(ui.painter(), rect, panel.focus == Focus::Response);
    let target = panel.target(&stage.song);
    let wire = panel
        .wire_index(&stage.song)
        .and_then(|index| stage.song.mod_wires.get(index));
    let fact = wire.map_or_else(
        || "NO PATCH".to_owned(),
        |wire| {
            if wire.solo {
                "SOLO".to_owned()
            } else if wire.enabled {
                "LIVE".to_owned()
            } else {
                "BYPASS".to_owned()
            }
        },
    );
    zone_head(
        ui.painter(),
        rect,
        "03",
        "RESPONSE",
        &fact,
        panel.focus == Focus::Response,
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 6.0, rect.min.y + ZONE_HEAD_H + 4.0),
        egui::pos2(rect.max.x - 6.0, rect.max.y - 5.0),
    );
    let Some(wire) = wire else {
        let message = target.as_ref().map_or_else(
            || "SELECT A SOURCE AND DESTINATION".to_owned(),
            |target| format!("NO PATCH // {}", target_path(target)),
        );
        ui.painter().text(
            egui::pos2(inner.center().x, inner.center().y - 16.0),
            egui::Align2::CENTER_CENTER,
            fit_text(&message, inner.width() - 20.0),
            font(),
            c.dim,
        );
        let patch = egui::Rect::from_center_size(
            egui::pos2(inner.center().x, inner.center().y + 18.0),
            egui::vec2(150.0_f32.min(inner.width()), 28.0),
        );
        let patch_painter = ui.painter().clone();
        if button(
            ui,
            &patch_painter,
            "mod-response-patch",
            patch,
            "PATCH + INSPECT",
            false,
            panel.source_id(&stage.song).is_some()
                && target.is_some()
                && stage.song.mod_wires.len() < 64,
            c.dir,
        )
        .clicked()
        {
            *action = Some(Action::EnsureRoute);
        }
        return;
    };
    let target = target.expect("a selected wire has its offered target");
    let source = panel.source_id(&stage.song);
    let source_live = source.and_then(|id| stage.mod_source_values.get(&id).copied());
    let wire_live = stage.mod_wire_values.get(&wire.id).copied();
    let source_word = source.map_or_else(
        || "NO SOURCE".to_owned(),
        |_| source_name(&stage.song, panel.source),
    );
    let facts = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(
            inner.max.x,
            (inner.min.y + RESPONSE_FACT_H).min(inner.max.y),
        ),
    );
    ui.painter().text(
        egui::pos2(facts.min.x + 2.0, facts.min.y + 8.0),
        egui::Align2::LEFT_CENTER,
        fit_text(
            &format!(
                "{} → TR {:02} / {}",
                source_word,
                target.track + 1,
                target_path(&target)
            ),
            facts.width() - 4.0,
        ),
        font(),
        c.dir,
    );
    let live_facts = format!(
        "BASE {} · SRC {} · WIRE {}",
        target.face(target.base),
        signed(source_live),
        signed(wire_live)
    );
    ui.painter().text(
        egui::pos2(facts.max.x - 2.0, facts.max.y - 8.0),
        egui::Align2::RIGHT_CENTER,
        fit_text(&live_facts, facts.width() - 4.0),
        font(),
        if wire.enabled { c.nominal } else { c.dim },
    );

    let plots_top = facts.max.y;
    let controls_floor = WireControl::ALL.len() as f32 * 19.0 + 23.0;
    let plot_h = RESPONSE_PLOT_H.min((inner.height() - RESPONSE_FACT_H - controls_floor).max(48.0));
    let plots_bottom = (plots_top + plot_h).min(inner.max.y);
    let plots = egui::Rect::from_min_max(
        egui::pos2(inner.min.x, plots_top),
        egui::pos2(inner.max.x, plots_bottom),
    );
    let split = plots.min.x + plots.width() * 0.64;
    let scope = egui::Rect::from_min_max(plots.min, egui::pos2(split - 3.0, plots.max.y));
    let transfer = egui::Rect::from_min_max(egui::pos2(split + 3.0, plots.min.y), plots.max);
    draw_scope(ui.painter(), scope, stage.mod_wire_scopes.get(&wire.id));
    draw_transfer(ui.painter(), transfer, wire);

    let controls = egui::Rect::from_min_max(
        egui::pos2(inner.min.x, plots.max.y + 5.0),
        egui::pos2(inner.max.x, inner.max.y - 18.0),
    );
    let row_h = (controls.height() / WireControl::ALL.len() as f32)
        .min(CONTROL_ROW_H)
        .max(19.0);
    for (row, control) in WireControl::ALL.into_iter().enumerate() {
        let control_rect = egui::Rect::from_min_max(
            egui::pos2(controls.min.x, controls.min.y + row as f32 * row_h),
            egui::pos2(
                controls.max.x,
                (controls.min.y + (row + 1) as f32 * row_h - 1.0).min(controls.max.y),
            ),
        );
        if control_rect.height() < 8.0 {
            break;
        }
        draw_control_row(
            ui,
            control_rect,
            control,
            wire,
            panel.focus == Focus::Response && panel.control == control,
            action,
        );
    }
    ui.painter().text(
        egui::pos2(inner.min.x, inner.max.y),
        egui::Align2::LEFT_BOTTOM,
        "↑↓ STAGE · ←→ VALUE · SHIFT FINE · ENTER ACTIVATE",
        font(),
        c.dim,
    );
}

impl super::super::Stage {
    /// Draw the complete modulation place into the central band.
    pub(super) fn draw_modulation(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(panel) = self.modulation.as_ref().cloned() else {
            return;
        };
        let c = palette::colours();
        let room = rect.shrink(INSET);
        if room.width() < 80.0 || room.height() < 100.0 {
            return;
        }
        ui.painter().rect_filled(room, 0.0, c.ground);
        chassis::frame(ui.painter(), room, true);
        chassis::marks(ui.painter(), room.shrink(3.0), 9.0);

        let header = egui::Rect::from_min_max(
            room.min,
            egui::pos2(room.max.x, (room.min.y + WORK_HEAD_H).min(room.max.y)),
        );
        chassis::instrument_rail(ui.painter(), header);
        ui.painter().text(
            egui::pos2(header.min.x + 8.0, header.center().y),
            egui::Align2::LEFT_CENTER,
            "MODULATION // PATCH ENGINE",
            font(),
            c.bright,
        );
        let live = !self.mod_source_values.is_empty() || !self.mod_wire_values.is_empty();
        let close_w = 86.0;
        let close = egui::Rect::from_min_max(
            egui::pos2(header.max.x - close_w, header.min.y + 4.0),
            egui::pos2(header.max.x - 5.0, header.max.y - 4.0),
        );
        let close_painter = ui.painter().clone();
        let close_response = button(
            ui,
            &close_painter,
            "mod-close",
            close,
            "CLOSE  ^+M",
            false,
            true,
            c.dim,
        );
        if close_response.clicked() {
            let _ = self.apply(StageIntent::Modulation);
            ui.ctx().request_repaint();
            return;
        }
        ui.painter().text(
            egui::pos2(close.min.x - 9.0, header.center().y),
            egui::Align2::RIGHT_CENTER,
            format!(
                "SRC {:02}/16 · PATCH {:02}/64 · ENGINE {}",
                self.song.modulators.len(),
                self.song.mod_wires.len(),
                if live { "LIVE" } else { "--" }
            ),
            font(),
            if live { c.nominal } else { c.dim },
        );

        let body = egui::Rect::from_min_max(
            egui::pos2(room.min.x + 5.0, header.max.y + 5.0),
            egui::pos2(room.max.x - 5.0, room.max.y - 5.0),
        );
        let mut action = None;
        // At the supported 720x480 minimum, stacking three complete zones
        // leaves neither destination rows nor response rails with a legal
        // rectangle. Use an explicit three-tab focus mode there: every zone
        // gets the full work area and every tab is directly clickable.
        let focus_mode = body.width() < 820.0 && body.height() < 650.0;
        if focus_mode {
            let tabs = egui::Rect::from_min_max(
                body.min,
                egui::pos2(body.max.x, body.min.y + ZONE_HEAD_H),
            );
            draw_focus_tabs(ui, tabs, &panel, &mut action);
            let focused =
                egui::Rect::from_min_max(egui::pos2(body.min.x, tabs.max.y + GAP * 0.5), body.max);
            match panel.focus {
                Focus::Sources => draw_source_zone(self, ui, focused, &panel, &mut action),
                Focus::Targets => draw_target_zone(self, ui, focused, &panel, &mut action),
                Focus::Response => draw_response_zone(self, ui, focused, &panel, &mut action),
            }
        } else {
            let source_h = SOURCE_ZONE_H
                .min((body.height() * 0.36).max(126.0))
                .min((body.height() - 110.0).max(70.0));
            let sources =
                egui::Rect::from_min_max(body.min, egui::pos2(body.max.x, body.min.y + source_h));
            let lower =
                egui::Rect::from_min_max(egui::pos2(body.min.x, sources.max.y + GAP), body.max);
            let (targets, response) = if lower.width() >= 820.0 {
                let split = lower.min.x + lower.width() * 0.54;
                (
                    egui::Rect::from_min_max(lower.min, egui::pos2(split - GAP * 0.5, lower.max.y)),
                    egui::Rect::from_min_max(egui::pos2(split + GAP * 0.5, lower.min.y), lower.max),
                )
            } else {
                let split = lower.min.y + lower.height() * 0.48;
                (
                    egui::Rect::from_min_max(lower.min, egui::pos2(lower.max.x, split - GAP * 0.5)),
                    egui::Rect::from_min_max(egui::pos2(lower.min.x, split + GAP * 0.5), lower.max),
                )
            };
            draw_source_zone(self, ui, sources, &panel, &mut action);
            draw_target_zone(self, ui, targets, &panel, &mut action);
            draw_response_zone(self, ui, response, &panel, &mut action);
        }

        match action {
            Some(Action::Focus(focus)) => {
                if let Some(panel) = &mut self.modulation {
                    panel.focus = focus;
                }
            }
            Some(Action::AddLfo) => {
                let _ = self.apply(StageIntent::ModAddLfo);
            }
            Some(Action::AddFollower) => {
                let _ = self.apply(StageIntent::ModAddFollower);
            }
            Some(Action::SelectSource(source)) => self.select_mod_source(source),
            Some(Action::SourceField { source, field }) => {
                self.select_mod_source(source);
                self.select_mod_source_field(field);
            }
            Some(Action::AdjustSource {
                source,
                field,
                forward,
            }) => {
                self.select_mod_source(source);
                self.select_mod_source_field(field);
                let intent = match field {
                    SourceField::Shape => StageIntent::ModShape { forward },
                    SourceField::Rate => StageIntent::ModRate { faster: forward },
                    SourceField::Mode => StageIntent::ModClock,
                };
                let _ = self.apply(intent);
            }
            Some(Action::DeleteSource(source)) => {
                self.select_mod_source(source);
                let _ = self.apply(StageIntent::ModDelete);
            }
            Some(Action::SelectTarget { track, target }) => {
                self.select_mod_target(track, target);
            }
            Some(Action::ToggleRoute { track, target }) => {
                self.select_mod_target(track, target);
                let _ = self.apply(StageIntent::ModToggleWire);
            }
            Some(Action::EnsureRoute) => {
                let _ = self.apply(StageIntent::ModToggleWire);
            }
            Some(Action::SelectControl(control)) => self.select_mod_control(control),
            Some(Action::SetControl {
                control,
                fraction,
                begin_gesture,
                end_gesture,
            }) => {
                self.select_mod_control(control);
                if begin_gesture {
                    self.begin_mod_wire_gesture();
                }
                self.set_mod_wire_fraction(control, fraction).ok();
                if end_gesture {
                    self.end_mod_wire_gesture();
                }
            }
            Some(Action::ActivateControl(control)) => {
                self.select_mod_control(control);
                let _ = self.apply(StageIntent::Enter);
            }
            None => {}
        }
        if action.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_stays_inside_each_visible_window() {
        for len in 1..32 {
            for capacity in 1..8 {
                for cursor in 0..len {
                    let (start, end) = visible_span(cursor, len, capacity);
                    assert!(start <= cursor && cursor < end);
                    assert!(end - start <= capacity);
                }
            }
        }
    }

    #[test]
    fn response_control_fractions_match_the_core_pointer_mapping() {
        let wire = ModWire {
            depth: -0.5,
            curve: 0.5,
            steps: 32,
            smooth_ms: 500.0,
            enabled: true,
            solo: false,
            ..ModWire::default()
        };
        assert_eq!(control_fraction(&wire, WireControl::Depth), 0.25);
        assert_eq!(control_fraction(&wire, WireControl::Curve), 0.75);
        assert_eq!(control_fraction(&wire, WireControl::Steps), 0.5);
        assert_eq!(control_fraction(&wire, WireControl::Smooth), 0.5);
        assert_eq!(control_fraction(&wire, WireControl::Enabled), 1.0);
        assert_eq!(control_fraction(&wire, WireControl::Solo), 0.0);
    }
}
