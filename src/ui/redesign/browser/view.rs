//! Rendering and input grammar for the sample archive.

use super::state::{BrowserState, Row, RowKind};
use super::{Intent, Outcome, View};
use crate::ui::redesign::grammar::{Motion, Sentence, Utterance};
use crate::ui::redesign::verbs::Verb;
use crate::ui::redesign::{OUTLINE, SURFACE_FRAME, focus};
use crate::ui::tokens::{font, radius, space, stroke};
use eframe::egui;

const HEADER_H: f32 = 28.0;
const SEARCH_H: f32 = 32.0;
const FOOTER_H: f32 = 24.0;
const ROW_H: f32 = 24.0;
const INDENT: f32 = 12.0;
const LOCUS_W: f32 = 10.0;

const VOID: egui::Color32 = egui::Color32::BLACK;
const BAND: egui::Color32 = egui::Color32::from_gray(13);
const HOVER: egui::Color32 = egui::Color32::from_gray(19);
const TEXT: egui::Color32 = egui::Color32::from_gray(220);
const MUTED: egui::Color32 = egui::Color32::from_gray(92);
const QUIET: egui::Color32 = egui::Color32::from_gray(48);

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's two tonal ladders stay ordered: surfaces step up from
    /// the void to the hovered row, ink steps up from quiet furniture to
    /// full-value text. Reorder either and rows stop reading as rows.
    #[test]
    fn value_ladders_stay_ordered() {
        assert!(VOID.r() < BAND.r());
        assert!(BAND.r() < HOVER.r());
        assert!(QUIET.r() < MUTED.r());
        assert!(MUTED.r() < TEXT.r());
        assert!(TEXT.r() < OUTLINE.r(), "text may not outshine focus");
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                depth: 0,
                label: "DRUMS".to_owned(),
                detail: "01".to_owned(),
                kind: RowKind::Location {
                    key: "location:drums".to_owned(),
                    open: false,
                },
            },
            Row {
                depth: 1,
                label: "IRON".to_owned(),
                detail: "WAV".to_owned(),
                kind: RowKind::Asset(std::path::PathBuf::from("/samples/iron.wav")),
            },
        ]
    }

    fn utter(
        state: &mut BrowserState,
        rows: &[Row],
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> (Outcome, SpeakEffect) {
        let mut outcome = Outcome::default();
        let effect = speak(
            state,
            rows,
            Utterance {
                count,
                verb,
                motion,
                held: false,
            },
            &mut outcome,
        );
        (outcome, effect)
    }

    #[test]
    fn counted_bare_motion_travels_rows() {
        let rows = rows();
        let mut state = BrowserState::default();
        utter(&mut state, &rows, None, Some(Motion::Down), 2);
        assert_eq!(state.cursor, 1);
        utter(&mut state, &rows, None, Some(Motion::Up), 1);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn act_toggles_a_container_and_loads_an_asset() {
        let rows = rows();
        let mut state = BrowserState::default();
        let (outcome, _) = utter(&mut state, &rows, Some(Verb::Act), None, 1);
        assert!(outcome.intents.is_empty());
        assert!(state.expanded.contains("location:drums"));

        state.cursor = 1;
        let (outcome, _) = utter(&mut state, &rows, Some(Verb::Act), None, 1);
        assert_eq!(
            outcome.intents,
            vec![Intent::SelectSample(std::path::PathBuf::from(
                "/samples/iron.wav"
            ))]
        );
    }

    #[test]
    fn search_enters_the_existing_field_and_other_verbs_refuse() {
        let rows = rows();
        let mut state = BrowserState::default();
        let (_, effect) = utter(&mut state, &rows, Some(Verb::Search), None, 1);
        assert_eq!(effect, SpeakEffect::FocusSearch);
        assert!(state.searching);

        let (outcome, effect) = utter(&mut state, &rows, Some(Verb::Solo), None, 1);
        assert!(outcome.intents.is_empty());
        assert_eq!(effect, SpeakEffect::None);
        assert_eq!(state.refusal.as_deref(), Some("SOLO: NOT HERE"));
    }

    #[test]
    fn mute_toggles_the_panel_wide_audition_sign() {
        let rows = rows();
        let mut state = BrowserState::default();
        assert_eq!(audition_sign(state.audition_enabled), "AUD+");

        state.cursor = 1;
        let mut audition = Vec::new();
        let _ = update_audition(&mut state, true, &rows, 0.0, &mut audition);
        let _ = update_audition(&mut state, true, &rows, 0.2, &mut audition);
        audition.clear();

        utter(&mut state, &rows, Some(Verb::Mute), None, 1);
        assert!(!state.audition_enabled);
        assert_eq!(audition_sign(state.audition_enabled), "AUD-");
        let _ = update_audition(&mut state, true, &rows, 0.3, &mut audition);
        assert_eq!(audition, vec![Intent::StopAudition]);

        utter(&mut state, &rows, Some(Verb::Mute), None, 1);
        assert!(state.audition_enabled);
    }

    #[test]
    fn audition_waits_for_a_stable_sample_and_stops_on_a_folder() {
        let rows = rows();
        let mut state = BrowserState::default();
        state.cursor = 1;
        let mut intents = Vec::new();

        let _ = update_audition(&mut state, true, &rows, 1.0, &mut intents);
        let _ = update_audition(&mut state, true, &rows, 1.149, &mut intents);
        assert!(intents.is_empty());
        let _ = update_audition(&mut state, true, &rows, 1.151, &mut intents);
        assert_eq!(
            intents,
            vec![Intent::AuditionSample(std::path::PathBuf::from(
                "/samples/iron.wav"
            ))]
        );

        state.cursor = 0;
        let _ = update_audition(&mut state, true, &rows, 1.2, &mut intents);
        assert_eq!(intents.last(), Some(&Intent::StopAudition));
    }

    #[test]
    fn leaving_browser_focus_stops_the_audition() {
        let rows = rows();
        let mut state = BrowserState::default();
        state.cursor = 1;
        let mut intents = Vec::new();
        let _ = update_audition(&mut state, true, &rows, 0.0, &mut intents);
        let _ = update_audition(&mut state, true, &rows, 0.2, &mut intents);
        let _ = update_audition(&mut state, false, &rows, 0.3, &mut intents);
        assert_eq!(intents.last(), Some(&Intent::StopAudition));
    }

    #[test]
    fn another_sample_stops_then_restarts_after_its_own_debounce() {
        let mut rows = rows();
        rows.push(Row {
            depth: 1,
            label: "WOOD".to_owned(),
            detail: "WAV".to_owned(),
            kind: RowKind::Asset(std::path::PathBuf::from("/samples/wood.wav")),
        });
        let mut state = BrowserState::default();
        state.cursor = 1;
        let mut intents = Vec::new();
        let _ = update_audition(&mut state, true, &rows, 0.0, &mut intents);
        let _ = update_audition(&mut state, true, &rows, 0.2, &mut intents);

        state.cursor = 2;
        let _ = update_audition(&mut state, true, &rows, 0.21, &mut intents);
        let _ = update_audition(&mut state, true, &rows, 0.361, &mut intents);
        assert_eq!(
            intents,
            vec![
                Intent::AuditionSample(std::path::PathBuf::from("/samples/iron.wav")),
                Intent::StopAudition,
                Intent::AuditionSample(std::path::PathBuf::from("/samples/wood.wav")),
            ]
        );
    }
}

pub(super) fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_FRAME)
        .corner_radius(radius::PANEL)
        .stroke(egui::Stroke::NONE)
}

pub(super) fn show(
    ui: &mut egui::Ui,
    focused: bool,
    sentence: &mut Sentence,
    state: &mut BrowserState,
    view: View<'_>,
) -> Outcome {
    let area = ui.available_rect_before_wrap();
    ui.take_available_space();
    let mut outcome = Outcome::default();
    if area.width() < 80.0 || area.height() < HEADER_H + SEARCH_H + FOOTER_H {
        return outcome;
    }

    let header = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), HEADER_H));
    let search = egui::Rect::from_min_size(
        egui::pos2(area.left(), header.bottom()),
        egui::vec2(area.width(), SEARCH_H),
    );
    let footer = egui::Rect::from_min_max(
        egui::pos2(area.left(), area.bottom() - FOOTER_H),
        area.right_bottom(),
    );
    let list = egui::Rect::from_min_max(
        egui::pos2(area.left(), search.bottom()),
        egui::pos2(area.right(), footer.top()),
    );

    draw_header(ui, header, view.snapshot.assets.len(), view.scanning);
    let search_response = search_field(ui, search, state);
    if search_response.clicked() {
        outcome.claim_focus = true;
    }

    let search_has_focus = search_response.has_focus();
    state.searching = search_has_focus;
    if search_has_focus
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        ui.ctx()
            .memory_mut(|memory| memory.surrender_focus(search_response.id));
        state.searching = false;
    }
    if search_has_focus
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        ui.ctx()
            .memory_mut(|memory| memory.surrender_focus(search_response.id));
        state.searching = false;
    }

    let rows = state.rows(view.snapshot);
    if focused && !search_has_focus {
        keyboard(ui, sentence, state, &rows, search_response.id, &mut outcome);
    }

    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                for (index, row) in rows.iter().enumerate() {
                    draw_row(ui, state, row, index, focused, &mut outcome);
                }
            });
    });
    if let Some(after) = update_audition(
        state,
        focused,
        &rows,
        ui.input(|input| input.time),
        &mut outcome.intents,
    ) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(after));
    }
    let sentence_display = (!sentence.is_empty()).then(|| sentence.display());
    let overlay = sentence_display.as_deref().or(state.refusal.as_deref());
    draw_footer(ui, footer, state.searching, state.audition_enabled, overlay);
    focus::show(ui.painter(), area, focused);
    outcome
}

fn draw_header(ui: &egui::Ui, rect: egui::Rect, asset_count: usize, scanning: bool) {
    ui.painter().rect_filled(rect, 0.0, BAND);
    let title = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    ui.painter().text(
        rect.left_center() + egui::vec2(space::SM, 0.0),
        egui::Align2::LEFT_CENTER,
        "ARCHIVE // LOCUS",
        title.clone(),
        TEXT,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(space::SM, 0.0),
        egui::Align2::RIGHT_CENTER,
        if scanning {
            "SCAN".to_owned()
        } else {
            format!("{asset_count:03}")
        },
        title,
        MUTED,
    );
    for phase in 0..3 {
        let x = rect.right() - 42.0 + phase as f32 * 5.0;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.bottom() - 5.0), egui::vec2(3.0, 1.0)),
            0.0,
            if scanning && phase == 1 { TEXT } else { QUIET },
        );
    }
}

fn search_field(ui: &mut egui::Ui, rect: egui::Rect, state: &mut BrowserState) -> egui::Response {
    ui.painter().rect_filled(rect, 0.0, VOID);
    let label_end = ui
        .painter()
        .text(
            rect.left_center() + egui::vec2(space::SM, 0.0),
            egui::Align2::LEFT_CENTER,
            "FIND /",
            egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
            if state.searching { TEXT } else { MUTED },
        )
        .right();
    let field = egui::Rect::from_min_max(
        egui::pos2(label_end + space::XS, rect.top() + space::XS),
        egui::pos2(rect.right() - space::SM, rect.bottom() - space::XS),
    );
    ui.put(
        field,
        egui::TextEdit::singleline(&mut state.query)
            .id(ui.id().with("archive-search"))
            .font(egui::FontId::new(font::LABEL, egui::FontFamily::Monospace))
            .text_color(TEXT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO),
    )
}

fn keyboard(
    ui: &mut egui::Ui,
    sentence: &mut Sentence,
    state: &mut BrowserState,
    rows: &[Row],
    search_id: egui::Id,
    outcome: &mut Outcome,
) {
    let Some(utterance) = sentence.consume(ui.ctx()) else {
        return;
    };
    state.refusal = None;
    if speak(state, rows, utterance, outcome) == SpeakEffect::FocusSearch {
        ui.ctx()
            .memory_mut(|memory| memory.request_focus(search_id));
        state.searching = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpeakEffect {
    None,
    FocusSearch,
}

fn speak(
    state: &mut BrowserState,
    rows: &[Row],
    utterance: Utterance,
    outcome: &mut Outcome,
) -> SpeakEffect {
    let count = utterance.count as isize;
    match (utterance.verb, utterance.motion) {
        (None, Some(Motion::Up)) => state.step(-count, rows.len()),
        (None, Some(Motion::Down)) => state.step(count, rows.len()),
        (None, Some(_)) => state.refusal = Some("MOTION: UP OR DOWN".to_owned()),
        (Some(Verb::Act), _) => {
            if !activate(state, rows, state.cursor, outcome) {
                state.refusal = Some("ACT: NOTHING HERE".to_owned());
            }
        }
        (Some(Verb::Search), _) => {
            state.searching = true;
            return SpeakEffect::FocusSearch;
        }
        (Some(Verb::Mute), _) => state.audition_enabled = !state.audition_enabled,
        (Some(verb), _) => state.refusal = Some(format!("{}: NOT HERE", verb.name())),
        (None, None) => {}
    }
    SpeakEffect::None
}

fn activate(state: &mut BrowserState, rows: &[Row], index: usize, outcome: &mut Outcome) -> bool {
    let Some(row) = rows.get(index) else {
        return false;
    };
    match &row.kind {
        RowKind::Asset(path) => {
            outcome.intents.push(Intent::SelectSample(path.clone()));
            true
        }
        RowKind::Location { .. } | RowKind::Folder { .. } => state.toggle_gate(row),
        RowKind::Message => false,
    }
}

fn draw_row(
    ui: &mut egui::Ui,
    state: &mut BrowserState,
    row: &Row,
    index: usize,
    focused: bool,
    outcome: &mut Outcome,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::click(),
    );
    let selected = index == state.cursor;
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, HOVER);
    }
    if response.clicked() {
        state.cursor = index;
        outcome.claim_focus = true;
        if row.is_container() {
            state.toggle_gate(row);
        }
    }
    if response.double_clicked()
        && let RowKind::Asset(path) = &row.kind
    {
        outcome.intents.push(Intent::SelectSample(path.clone()));
    }
    if selected && focused {
        response.scroll_to_me(Some(egui::Align::Center));
    }

    let locus = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, rect.top() + 3.0),
        egui::pos2(rect.left() + LOCUS_W, rect.bottom() - 3.0),
    );
    if selected {
        draw_locus(ui.painter(), locus, focused);
    }

    let gate_x = rect.left() + LOCUS_W + space::XS + row.depth as f32 * INDENT;
    if let RowKind::Location { open, .. } | RowKind::Folder { open, .. } = row.kind {
        draw_gate(ui.painter(), egui::pos2(gate_x, rect.center().y), open);
    }

    let text_left = gate_x
        + if row.is_container() {
            INDENT
        } else {
            space::XS
        };
    let detail_width = if row.detail.len() > 8 {
        rect.width() * 0.42
    } else {
        34.0
    };
    let detail_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - detail_width - space::SM, rect.top()),
        egui::pos2(rect.right() - space::SM, rect.bottom()),
    );
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.top()),
        egui::pos2(detail_rect.left() - space::XS, rect.bottom()),
    );
    let row_font = egui::FontId::new(font::LABEL, egui::FontFamily::Monospace);
    ui.painter().with_clip_rect(label_rect).text(
        egui::pos2(label_rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        &row.label,
        row_font,
        if matches!(row.kind, RowKind::Message) {
            MUTED
        } else {
            TEXT
        },
    );
    ui.painter().with_clip_rect(detail_rect).text(
        detail_rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        &row.detail,
        egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
}

fn draw_gate(painter: &egui::Painter, center: egui::Pos2, open: bool) {
    let ink = egui::Stroke::new(stroke::HAIR, MUTED);
    painter.line_segment(
        [
            center + egui::vec2(-3.0, -4.0),
            center + egui::vec2(-3.0, 4.0),
        ],
        ink,
    );
    painter.line_segment(
        if open {
            [
                center + egui::vec2(-3.0, 4.0),
                center + egui::vec2(4.0, 4.0),
            ]
        } else {
            [
                center + egui::vec2(-3.0, 0.0),
                center + egui::vec2(4.0, 0.0),
            ]
        },
        ink,
    );
}

fn draw_locus(painter: &egui::Painter, rect: egui::Rect, focused: bool) {
    let ink = egui::Stroke::new(
        if focused { stroke::FOCUS } else { stroke::HAIR },
        if focused { OUTLINE } else { QUIET },
    );
    painter.line_segment([rect.left_top(), rect.right_top()], ink);
    painter.line_segment([rect.left_top(), rect.left_bottom()], ink);
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], ink);
    painter.line_segment([rect.center(), rect.center() + egui::vec2(4.0, 0.0)], ink);
}

fn update_audition(
    state: &mut BrowserState,
    focused: bool,
    rows: &[Row],
    now: f64,
    intents: &mut Vec<Intent>,
) -> Option<f64> {
    let selected = if focused {
        rows.get(state.cursor)
    } else {
        None
    };
    let target = selected.and_then(|row| match &row.kind {
        RowKind::Asset(path) => Some(path.clone()),
        RowKind::Location { .. } | RowKind::Folder { .. } | RowKind::Message => None,
    });
    state.update_audition(target.as_deref(), now, intents)
}

fn audition_sign(enabled: bool) -> &'static str {
    if enabled { "AUD+" } else { "AUD-" }
}

fn draw_footer(
    ui: &egui::Ui,
    rect: egui::Rect,
    searching: bool,
    audition_enabled: bool,
    overlay: Option<&str>,
) {
    ui.painter().rect_filled(rect, 0.0, BAND);
    let message_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - 48.0, rect.bottom()));
    ui.painter().with_clip_rect(message_rect).text(
        rect.left_center() + egui::vec2(space::SM, 0.0),
        egui::Align2::LEFT_CENTER,
        if let Some(overlay) = overlay {
            overlay
        } else if searching {
            "TYPE QUERY  /  ENTER LOCUS  /  ESC EXIT"
        } else {
            "UP/DN LOCUS  /  ENTER ACT  /  / SEARCH"
        },
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(space::SM, 0.0),
        egui::Align2::RIGHT_CENTER,
        audition_sign(audition_enabled),
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        QUIET,
    );
}
