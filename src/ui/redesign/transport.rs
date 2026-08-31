//! One-row transport surface for the redesign.
//!
//! The bar receives a read-only snapshot and emits semantic intents. Backend
//! state and engine commands remain owned by the application.

use crate::ui::affordance::{Afford, Affords};
use crate::ui::redesign::midi_typing;
use crate::ui::redesign::{OUTLINE, SURFACE_FRAME, focus, signs};
use crate::ui::tokens::{font, radius, space};
use eframe::egui;

const BAR_HEIGHT: f32 = 36.0;
const BUTTON_W: f32 = 34.0;
const BUTTON_W_COMPACT: f32 = 28.0;
const GAP: f32 = 2.0;
const GROUP_GAP: f32 = 12.0;
const ACTIVE_RAIL: f32 = 2.0;
const NODE: f32 = 3.0;
const TEMPO_PER_PX: f64 = 0.1;

#[derive(Clone, Copy)]
pub struct View<'a> {
    pub playing: bool,
    pub armed: bool,
    pub loop_on: bool,
    pub metronome: bool,
    pub follow: bool,
    pub engine_on: bool,
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_unit: u32,
    pub position: &'a str,
    /// The ambient harmonic context's sign: `D DORIAN`. Always visible,
    /// never inverted — key is state, not a mode, and loudness is
    /// rationed (`notes/20260831-pitch-lens-spec.md` §4).
    pub key_sign: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    Return,
    TogglePlay,
    Pause,
    Stop,
    ToggleRecord,
    ToggleEngine,
    ToggleLoop,
    ToggleMetronome,
    ToggleFollow,
    SetTempo(f64),
    CycleBeatUnit,
}

#[derive(Default)]
pub struct Outcome {
    pub intents: Vec<Intent>,
}

#[derive(Default)]
pub(crate) struct TransportBar;

impl TransportBar {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        focused: bool,
        view: View<'_>,
        midi: midi_typing::Status,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        egui::Panel::top("redesign-transport")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(BAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE_FRAME)
                    .corner_radius(radius::PANEL)
                    .stroke(egui::Stroke::NONE),
            )
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                ui.take_available_space();
                draw(ui, rect, focused, view, midi, &mut outcome.intents);
            });
        outcome
    }
}

fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    focused: bool,
    view: View<'_>,
    midi: midi_typing::Status,
    intents: &mut Vec<Intent>,
) {
    let compact = rect.width() < 900.0;
    let button_w = if compact { BUTTON_W_COMPACT } else { BUTTON_W };
    let margin = if compact { space::SM } else { space::LG };
    let mut left = rect.left() + margin;

    let motion = [
        (
            "return",
            signs::RETURN,
            "RETURN · HOME",
            false,
            false,
            Intent::Return,
        ),
        (
            "play",
            signs::PLAY,
            "PLAY / PAUSE · SPACE",
            view.playing,
            false,
            Intent::TogglePlay,
        ),
        (
            "pause",
            signs::PAUSE,
            "PAUSE · SHIFT SPACE",
            false,
            false,
            Intent::Pause,
        ),
        (
            "stop",
            signs::STOP,
            "STOP + RETURN",
            false,
            false,
            Intent::Stop,
        ),
        (
            "record",
            signs::RECORD,
            "RECORD ARM · F9",
            view.armed,
            view.armed && view.playing,
            Intent::ToggleRecord,
        ),
    ];
    for (id, glyph, tip, active, recording, intent) in motion {
        let button = take_left(&mut left, rect, button_w, GAP);
        if signal_button(ui, button, id, glyph, tip, active, recording) {
            intents.push(intent);
        }
    }
    left += GROUP_GAP;
    phase_mark(ui, egui::pos2(left, rect.center().y));
    left += GROUP_GAP;

    let mut right = rect.right() - margin;
    let midi_w = if compact { 104.0 } else { 164.0 };
    let midi_rect = take_right(&mut right, rect, midi_w, GROUP_GAP);
    // The plate says which pitch language the letters speak: degrees of
    // the ambient key, or chromatic octaves (`^DEG` vs `OCT`).
    let midi_text = if compact {
        if midi.degree_mode {
            format!(
                "MIDI{} ^D{:+} [I]",
                if midi.enabled { "+" } else { "-" },
                midi.period_shift
            )
        } else {
            format!(
                "MIDI{} O{:+} [I]",
                if midi.enabled { "+" } else { "-" },
                midi.octave
            )
        }
    } else if midi.degree_mode {
        format!(
            "MIDI {} · ^DEG {:+} · [I·ESC]",
            if midi.enabled { "ON" } else { "OFF" },
            midi.period_shift
        )
    } else {
        format!(
            "MIDI {} · OCT {:+} · [I·ESC]",
            if midi.enabled { "ON" } else { "OFF" },
            midi.octave
        )
    };
    // The key sign sits beside the plate: quiet ink, always present.
    if !view.key_sign.is_empty() {
        let key_w = if compact { 92.0 } else { 128.0 };
        let key_rect = take_right(&mut right, rect, key_w, GAP);
        text(
            ui,
            key_rect,
            view.key_sign,
            egui::Align2::RIGHT_CENTER,
            false,
        );
    }
    if midi.enabled {
        // A mode with no sign is a trap: while the letters are an
        // instrument, its field inverts — the one white block on the
        // bar, unmissable from any distance.
        let plate = midi_rect.expand2(egui::vec2(space::XS, -space::XS));
        ui.painter().rect_filled(plate, 0.0, OUTLINE);
        ui.painter().text(
            midi_rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            &midi_text,
            egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
            egui::Color32::BLACK,
        );
    } else {
        text(ui, midi_rect, &midi_text, egui::Align2::RIGHT_CENTER, true);
    }

    let switches = [
        (
            "follow",
            signs::FOLLOW,
            "FOLLOW PLAYHEAD",
            view.follow,
            Intent::ToggleFollow,
        ),
        (
            "metro",
            signs::METRONOME,
            "METRONOME · O",
            view.metronome,
            Intent::ToggleMetronome,
        ),
        (
            "loop",
            signs::LOOP,
            "ARRANGEMENT LOOP",
            view.loop_on,
            Intent::ToggleLoop,
        ),
        (
            "engine",
            signs::ENGINE,
            "AUDIO ENGINE POWER",
            view.engine_on,
            Intent::ToggleEngine,
        ),
    ];
    for (id, glyph, tip, active, intent) in switches {
        let button = take_right(&mut right, rect, button_w, GAP);
        if signal_button(ui, button, id, glyph, tip, active, false) {
            intents.push(intent);
        }
    }
    right -= GROUP_GAP;
    phase_mark(ui, egui::pos2(right, rect.center().y));
    right -= GROUP_GAP;

    let position_w = if compact { 72.0 } else { 104.0 };
    let tempo_w = if compact { 58.0 } else { 82.0 };
    let meter_w = if compact { 42.0 } else { 52.0 };
    let data_w = position_w + tempo_w + meter_w + GAP * 2.0;
    let data_left = (left + (right - left - data_w) * 0.5).max(left);
    let mut x = data_left;

    let position = take_left(&mut x, rect, position_w, GAP);
    field_surface(ui, position, false);
    text(
        ui,
        position.shrink(space::XS),
        view.position,
        egui::Align2::CENTER_CENTER,
        true,
    );

    let tempo = take_left(&mut x, rect, tempo_w, GAP);
    let tempo_response = drag_field(
        ui,
        tempo,
        "tempo",
        &if compact {
            format!("{:.0}", view.bpm)
        } else {
            format!("BPM {:05.2}", view.bpm)
        },
        "TEMPO · DRAG HORIZONTALLY",
    );
    if tempo_response.dragged() {
        intents.push(Intent::SetTempo(
            (view.bpm + f64::from(tempo_response.drag_motion().x) * TEMPO_PER_PX)
                .clamp(20.0, 999.0),
        ));
    }

    let meter = take_left(&mut x, rect, meter_w, 0.0);
    if click_field(
        ui,
        meter,
        "meter",
        &format!("{}/{}", view.beats_per_bar, view.beat_unit),
        "TIME SIGNATURE · CLICK TO CYCLE BEAT UNIT",
    ) {
        intents.push(Intent::CycleBeatUnit);
    }

    focus::show(ui.painter(), rect, focused);
}

fn take_left(x: &mut f32, row: egui::Rect, width: f32, gap: f32) -> egui::Rect {
    let rect = egui::Rect::from_min_max(
        egui::pos2(*x, row.top()),
        egui::pos2(*x + width, row.bottom()),
    );
    *x += width + gap;
    rect
}

fn take_right(x: &mut f32, row: egui::Rect, width: f32, gap: f32) -> egui::Rect {
    let rect = egui::Rect::from_min_max(
        egui::pos2(*x - width, row.top()),
        egui::pos2(*x, row.bottom()),
    );
    *x -= width + gap;
    rect
}

/// Three square nodes: punctuation between information domains, and the
/// bar's only deliberately esoteric mark. It carries no command or state.
fn phase_mark(ui: &egui::Ui, at: egui::Pos2) {
    for offset in [
        egui::vec2(-3.0, -2.0),
        egui::vec2(3.0, -2.0),
        egui::vec2(0.0, 3.0),
    ] {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(at + offset, egui::Vec2::splat(2.0)),
            0.0,
            egui::Color32::from_gray(128),
        );
    }
}

fn signal_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: &'static str,
    glyph: &str,
    tip: &'static str,
    active: bool,
    recording: bool,
) -> bool {
    let response = ui
        .interact(rect, ui.id().with(("transport", id)), egui::Sense::click())
        .affords(Affords::Press)
        .on_hover_text(tip);
    field_surface(ui, rect, response.hovered());
    if active {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - ACTIVE_RAIL),
                rect.right_bottom(),
            ),
            0.0,
            OUTLINE,
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                rect.right_top() - egui::vec2(NODE, 0.0),
                egui::Vec2::splat(NODE),
            ),
            0.0,
            OUTLINE,
        );
    }
    if recording {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.right(), rect.top() + ACTIVE_RAIL),
            ),
            0.0,
            OUTLINE,
        );
    }
    text(ui, rect, glyph, egui::Align2::CENTER_CENTER, true);
    response.clicked()
}

fn field_surface(ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    ui.painter().rect_filled(
        rect.shrink(1.0),
        0.0,
        egui::Color32::from_gray(if hovered { 24 } else { 13 }),
    );
}

fn click_field(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: &'static str,
    value: &str,
    tip: &'static str,
) -> bool {
    let response = ui
        .interact(
            rect,
            ui.id().with(("transport-field", id)),
            egui::Sense::click(),
        )
        .affords(Affords::Press)
        .on_hover_text(tip);
    field_surface(ui, rect, response.hovered());
    text(
        ui,
        rect.shrink(space::XS),
        value,
        egui::Align2::CENTER_CENTER,
        false,
    );
    response.clicked()
}

fn drag_field(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: &'static str,
    value: &str,
    tip: &'static str,
) -> egui::Response {
    let response = ui
        .interact(
            rect,
            ui.id().with(("transport-field", id)),
            egui::Sense::click_and_drag(),
        )
        .affords(Affords::Sweep)
        .on_hover_text(tip);
    field_surface(ui, rect, response.hovered() || response.dragged());
    text(
        ui,
        rect.shrink(space::XS),
        value,
        egui::Align2::CENTER_CENTER,
        false,
    );
    response
}

fn text(ui: &egui::Ui, rect: egui::Rect, value: &str, align: egui::Align2, primary: bool) {
    let at = if align == egui::Align2::RIGHT_CENTER {
        rect.right_center()
    } else if align == egui::Align2::LEFT_CENTER {
        rect.left_center()
    } else {
        rect.center()
    };
    ui.painter().text(
        at,
        align,
        value,
        egui::FontId::new(
            if primary {
                font::BODY
            } else {
                font::MINI_LABEL
            },
            egui::FontFamily::Monospace,
        ),
        if primary {
            OUTLINE
        } else {
            egui::Color32::from_gray(176)
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    /// Proportion carries meaning: the transport is glanceable state, not
    /// a workspace, so it stays a thin strip. Growing it steals value
    /// hierarchy from the arrangement, which is big because it matters.
    #[test]
    fn the_transport_stays_thin() {
        assert!(BAR_HEIGHT <= 40.0);
    }

    #[test]
    fn tempo_drag_owns_only_its_field() {
        let ctx = egui::Context::default();
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 48.0));
        let tempo = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(72.0, 28.0));
        let meter = egui::Rect::from_min_size(egui::pos2(96.0, 8.0), egui::vec2(72.0, 28.0));
        let path = probe::drag_path(tempo.center(), tempo.center() + egui::vec2(48.0, 0.0), 6);
        let frames = probe::run(&ctx, view, &path, |ui| {
            let tempo = drag_field(ui, tempo, "probe-tempo", "120", "tempo");
            let meter = click_field(ui, meter, "probe-meter", "4/4", "meter");
            (tempo.dragged(), meter)
        });
        assert!(frames.iter().any(|(tempo, _)| *tempo));
        assert!(frames.iter().all(|(_, meter)| !*meter));
    }

    #[test]
    fn empty_ground_does_not_drag_tempo() {
        let ctx = egui::Context::default();
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 64.0));
        let tempo = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(72.0, 28.0));
        let path = probe::drag_path(egui::pos2(120.0, 52.0), egui::pos2(160.0, 52.0), 4);
        let frames = probe::run(&ctx, view, &path, |ui| {
            drag_field(ui, tempo, "probe-tempo", "120", "tempo").dragged()
        });
        assert!(frames.iter().all(|dragged| !*dragged));
    }
}
