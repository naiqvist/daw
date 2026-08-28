//! The utility's device card.
//!
//! Ids, ranges and defaults come from [`crate::params::utility`] — the
//! one table this widget, `Node::Utility`'s core and the app's edit
//! routing all read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ utility ───────────────────────────────────────────┐  context strip
//! │ ┌ screen ────────────────────────────────────────┐ │
//! │ │ swap · ø L · dc                            2.0 │ │  what is engaged
//! │ │            ┌───────────────┐                   │ │
//! │ │            │      ▮        │  ← the image      │ │  width, and where
//! │ │  L ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┼┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ R  1.0     │ │  unity, dashed
//! │ │                  ▮  ≤120 Hz                    │ │  the lows, centred
//! │ ├────────────────────────────────────────────────┤ │
//! │ │ +0.0dB  C   100%  off   off   stereo  off      │ │  value
//! │ │ GAIN   PAN WIDTH  MONO PHASE CHANNEL   DC      │ │  name
//! │ │ ▁▁▁▁▁  ▁█▁ ▁▁▁▁▁  ▁▁▁▁  ▁█▁   ▁█▁▁     ▁█      │ │  bar, or detents
//! │ └────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────┘
//! ```
//!
//! # Why a field and not a meter
//!
//! Every other display on a card in this rack draws what the engine is
//! DOING — the compressor's reduction, the equaliser's curve against a
//! spectrum. This one draws what the settings MEAN, and that is the
//! right choice for this device specifically: a utility has no behaviour
//! to watch. It does exactly what its knobs say, always, and the thing
//! that is genuinely hard to hold in your head is not "how much is it
//! working" but "what shape is the image now" — a width figure, a pan
//! figure and a crossover, three numbers describing one picture.
//!
//! So the plot is that picture. The bar is the stereo image: as wide as
//! the width control makes it, sitting where the pan control puts it,
//! against a rule at unity so "wider than it was" is visible without
//! reading a percentage. Under it, when bass mono is on, the low band
//! drawn where it actually goes — narrow, in the middle.
//!
//! A goniometer would have been the other answer, and it needs the
//! engine's samples at frame rate for a display that says "this material
//! is wide" rather than "this device is making it wide". That is a
//! meter, and it belongs on a track's output, not on the card of the
//! thing setting the width.
//!
//! # Why the image is one draggable target and not three
//!
//! Pan and width are two parameters and one movement — `xy.rs` says the
//! same about cutoff and resonance — so the image carries ONE
//! interaction with its own id, and it owns a drag from press to
//! release however far the pointer wanders. The crossover is a cell and
//! not a second handle on the same picture: a vertical axis that meant
//! width for one grab and frequency for another is the shape of the bug
//! `notes/20260826-device-ui-contract.md` was written about.

use crate::params::utility as up;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The cells, in the order they read: how loud, where, how wide, what
/// stays in the middle, then the three repairs.
const CELLS: [u32; 7] = [
    up::GAIN,
    up::PAN,
    up::WIDTH,
    up::MONO_HZ,
    up::PHASE,
    up::CHANNEL,
    up::DC,
];

/// A labelled cell is TWO `POLY_CELL_H` units tall — a value over a
/// name — and this card draws one row of them, so the hero keeps the
/// rest of the height. See `notes/20260827-device-card-layout.md`.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;

/// How far off centre the field draws before it runs out of picture, in
/// pan units. Wider than the ±1 the pan control reaches, because the
/// IMAGE is what is being drawn and a hard-panned track at full width
/// has an edge out past the speaker it is panned to. At 1.5 the widest
/// legal setting still shows both of its edges.
const FIELD_SPAN: f32 = 1.5;

/// Knob positions of one utility, normalized. Serialized into project
/// files, so they survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UtilityUi {
    pub gain: f32,
    pub pan: f32,
    pub width: f32,
    pub mono: f32,
    pub phase: f32,
    pub channel: f32,
    pub dc: f32,
}

impl Default for UtilityUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the controls use.
        let at = |id: u32| utility_norm(id, params::def(up::TABLE, id).default);
        Self {
            gain: at(up::GAIN),
            pan: at(up::PAN),
            width: at(up::WIDTH),
            mono: at(up::MONO_HZ),
            phase: at(up::PHASE),
            channel: at(up::CHANNEL),
            dc: at(up::DC),
        }
    }
}

impl UtilityUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the table cannot route an edit
    /// into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            up::GAIN => &mut self.gain,
            up::PAN => &mut self.pan,
            up::WIDTH => &mut self.width,
            up::MONO_HZ => &mut self.mono,
            up::PHASE => &mut self.phase,
            up::CHANNEL => &mut self.channel,
            up::DC => &mut self.dc,
            _ => return None,
        })
    }

    /// Put a control at a normalized position by wire id — what the app
    /// uses to reflect a loaded patch onto the card.
    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    fn value(&self, param: u32) -> f32 {
        let mut copy = *self;
        copy.slot(param)
            .map(|norm| utility_value(param, *norm))
            .unwrap_or_default()
    }
}

/// One control by wire id — the single place an id becomes a [`Param`].
fn param_of(id: u32) -> Param {
    let def = params::def(up::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        up::PHASE => with(Param::choice("phase", up::PHASE_NAMES)),
        up::CHANNEL => with(Param::choice("channel", up::CHANNEL_NAMES)),
        up::DC => with(Param::choice("dc", up::SWITCH_NAMES)),
        up::GAIN => with(Param::db("gain", def.min, def.max).bipolar()),
        // LOG: a crossover corner is heard in ratios, like every other
        // frequency on every other card.
        up::MONO_HZ => with(Param::hz("mono", def.min, def.max)),
        // The two the FIELD also edits. Their units are `Plain` and
        // their readouts come from [`cell_text`] instead: "0.35" is a
        // true and useless thing to print on a pan control, and the
        // cell's own formatter hook is the place to fix that without a
        // second opinion about the range living in the widget.
        up::PAN => with(
            Param::new(
                "pan",
                Mapping::Linear {
                    min: def.min,
                    max: def.max,
                },
                Unit::Plain,
            )
            .bipolar(),
        ),
        _ => with(Param::new(
            "width",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )),
    }
}

/// "L35", "C", "R100" — where the track sits, said the way a desk says
/// it. Takes the ENGINE value, which is what a cell's formatter hook is
/// handed.
fn pan_text(value: f32) -> String {
    let percent = (value * 100.0).round();
    if percent <= -1.0 {
        format!("L{:.0}", -percent)
    } else if percent >= 1.0 {
        format!("R{percent:.0}")
    } else {
        "C".to_owned()
    }
}

/// Width as a percentage of natural, so 100 % is untouched and 0 % is
/// mono. The engine value is a factor, and nobody thinks in factors here.
fn width_text(value: f32) -> String {
    format!("{:.0} %", value * 100.0)
}

/// The crossover, or the word OFF at its floor — a stage drawn as
/// bypassed while it is still running is how a null test stops nulling,
/// so the card asks [`up::mono_off`] rather than deciding for itself.
fn mono_text(value: f32) -> String {
    if up::mono_off(value) {
        "off".to_owned()
    } else {
        Unit::Hz.format(value)
    }
}

/// The cell readout that overrides the `Param`'s own, by id.
///
/// A function POINTER rather than a closure: it is handed to the cell as
/// a `&dyn Fn`, and a pointer is a value the caller can hold without a
/// box and without a lifetime argument.
fn cell_text(id: u32) -> Option<fn(f32) -> String> {
    match id {
        up::PAN => Some(pan_text),
        up::WIDTH => Some(width_text),
        up::MONO_HZ => Some(mono_text),
        _ => None,
    }
}

/// The WIDEST reading a cell will ever show — its own formatter's answer
/// where it has one, and the `Param`'s otherwise.
///
/// Sampled across the range for the same reason
/// [`Param::widest_text`] samples: "20.0 Hz" and "500 Hz" are different
/// widths and only the formatter knows which wins. Reserving for the
/// value a cell happens to show right now is how a cell prints through
/// its neighbour the moment a knob moves.
fn widest_text(id: u32) -> String {
    let param = param_of(id);
    let Some(text) = cell_text(id) else {
        return param.widest_text();
    };
    const SAMPLES: usize = 13;
    (0..SAMPLES)
        .map(|i| text(param.value(i as f32 / (SAMPLES - 1) as f32)))
        .max_by_key(String::len)
        .unwrap_or_default()
}

/// The engine-facing value at a normalized position, by param id.
pub fn utility_value(param: u32, norm: f32) -> f32 {
    params::def(up::TABLE, param).clamp(param_of(param).value(norm))
}

/// The inverse of [`utility_value`], for a state stored in engine units.
pub fn utility_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Whether a parameter snaps to named settings rather than sweeping.
pub fn utility_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of
/// the crossover moves in ratios exactly as the control does.
pub fn utility_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn utility_edits(state: &UtilityUi) -> Vec<ParamEdit> {
    let mut state = *state;
    up::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: utility_value(def.id, *norm),
            })
        })
        .collect()
}

/// The narrowest a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over. XS either side, not SM — seven cells at
/// eight points a side is a hundred points of nothing.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, id: u32) -> f32 {
    let value = metrics::mono_w(ui, &widest_text(id), font::VALUE);
    let name = metrics::text_w(ui, &param_of(id).name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its seven cells at their narrowest,
/// which is what a card must have before it starts clipping controls
/// away. Given more, the cells share it.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, *id))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32
}

/// The card's layout: one well, and the screen fills it.
fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// What the field's corner prints: the repairs that are ENGAGED, and
/// nothing about the ones that are not.
///
/// A strip that always read "stereo · phase off · dc off" would be three
/// words saying nothing three times; the whole value of the line is that
/// it is empty until something unusual is switched on, and then it is
/// the first thing you see.
fn tag(state: &UtilityUi) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let channel = up::index(state.value(up::CHANNEL), up::CHANNEL_NAMES.len());
    if channel != up::CHANNEL_STEREO {
        parts.extend(up::CHANNEL_NAMES.get(channel as usize));
    }
    let phase = up::index(state.value(up::PHASE), up::PHASE_NAMES.len());
    if phase != up::PHASE_NONE {
        parts.extend(up::PHASE_NAMES.get(phase as usize));
    }
    if state.value(up::DC) >= 0.5 {
        parts.push("dc");
    }
    parts.join(" · ")
}

/// The stereo field. Returns the pan and width the gesture asked for, in
/// ENGINE units, or `None` when nothing was dragged.
///
/// ONE interaction for the image, allocated AFTER the background — egui
/// gives a press to the last widget added at a position, and a drag that
/// began on the image stays the image's until the button comes up.
fn field(ui: &mut egui::Ui, theme: &Theme, state: &UtilityUi, tag: &str) -> Option<(f32, f32)> {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, background) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());

    let pan = state.value(up::PAN);
    let width = state.value(up::WIDTH);
    let mono_hz = state.value(up::MONO_HZ);

    // Where a pan position and a width sit in the picture.
    let x_of = |p: f32| rect.center().x + (p / FIELD_SPAN) * rect.width() * 0.5;
    let y_of = |w: f32| rect.bottom() - (w / up::WIDTH_MAX).clamp(0.0, 1.0) * rect.height();

    // The grab is a PUCK of its own size, not the drawn bar: at width
    // zero the bar is a line with no area, and a target that vanishes
    // when a control reaches the end of its range is the second bug in
    // the device UI contract.
    let grab = theme
        .sp(control::HANDLE)
        .max(metrics::interactive_min(ui).y * 0.5);
    let centre = egui::pos2(x_of(pan), y_of(width));
    let puck = egui::Rect::from_center_size(centre, egui::vec2(grab * 2.0, grab * 2.0));
    let handle = ui
        .interact(
            puck,
            background.id.with("image"),
            egui::Sense::click_and_drag(),
        )
        .affords(Affords::Steer);

    let mut moved = None;
    if handle.dragged() {
        let delta = handle.drag_delta();
        if delta != egui::Vec2::ZERO && rect.width() > 0.0 && rect.height() > 0.0 {
            let next_pan = (pan + delta.x / (rect.width() * 0.5) * FIELD_SPAN).clamp(-1.0, 1.0);
            let next_width =
                (width - delta.y / rect.height() * up::WIDTH_MAX).clamp(0.0, up::WIDTH_MAX);
            if next_pan != pan || next_width != width {
                moved = Some((next_pan, next_width));
            }
        }
    }
    if handle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }

    let (pan, width) = moved.unwrap_or((pan, width));
    paint(
        ui,
        theme,
        rect,
        Drawn {
            pan,
            width,
            mono_hz,
            tag,
        },
        &handle,
    );
    moved
}

/// What the picture shows — resolved values, so the paint pass does no
/// arithmetic about what a control means.
struct Drawn<'a> {
    pan: f32,
    width: f32,
    mono_hz: f32,
    tag: &'a str,
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    drawn: Drawn<'_>,
    handle: &egui::Response,
) {
    let painter = ui.painter();
    let x_of = |p: f32| rect.center().x + (p / FIELD_SPAN) * rect.width() * 0.5;
    let y_of = |w: f32| rect.bottom() - (w / up::WIDTH_MAX).clamp(0.0, 1.0) * rect.height();

    // --- the scale ----------------------------------------------------
    //
    // Drawn first and present whatever the settings are: a display with a
    // scale is an instrument at rest, and one without is a hole. The
    // speakers at ±1 and the centre between them are the whole of the
    // horizontal reading.
    for (at, label) in [(-1.0f32, "L"), (1.0, "R")] {
        let x = x_of(at);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::HAIR, theme.grid_bar),
        );
        painter.text(
            egui::pos2(x, rect.bottom() - theme.sp(space::XXS)),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    }
    painter.line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );
    // UNITY, and it is the one rule that earns a label: "wider than it
    // was" is the question this display exists to answer at a glance,
    // and it cannot be answered against a blank field.
    let unity_y = y_of(1.0);
    painter.line_segment(
        [
            egui::pos2(rect.left(), unity_y),
            egui::pos2(rect.right(), unity_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );
    painter.text(
        egui::pos2(rect.right() - theme.sp(space::XS), unity_y),
        egui::Align2::RIGHT_BOTTOM,
        "100%",
        egui::FontId::monospace(font::MICRO_LABEL),
        theme.text_muted,
    );

    // --- the image ----------------------------------------------------
    let half = drawn.width * 0.5;
    let y = y_of(drawn.width);
    let bar_h = (rect.height() * 0.09).max(stroke::BOLD);
    let left = x_of(drawn.pan - half).max(rect.left());
    let right = x_of(drawn.pan + half).min(rect.right());
    let live = handle.hovered() || handle.dragged();
    let body = egui::Rect::from_min_max(
        egui::pos2(left.min(right), y - bar_h * 0.5),
        egui::pos2(right.max(left), y + bar_h * 0.5),
    );
    painter.rect_filled(
        body,
        bar_h * 0.5,
        // `role_level`: this is a placement and a size, not a
        // modulation and not a time.
        if live {
            theme.role_level
        } else {
            theme.role_level_dim
        },
    );
    // The centre, which is where the track actually IS — a bar alone
    // says how wide and not where, and at full width the two edges are
    // off the picture entirely.
    let centre_x = x_of(drawn.pan);
    painter.line_segment(
        [
            egui::pos2(centre_x, y - bar_h),
            egui::pos2(centre_x, y + bar_h),
        ],
        egui::Stroke::new(
            if live { stroke::MARK } else { stroke::BOLD },
            theme.role_level,
        ),
    );

    // --- the low band, where bass mono puts it ------------------------
    //
    // Drawn only when the stage runs, and drawn WHERE IT GOES rather
    // than as an indicator that it is on: a stub in the middle under a
    // wide bar is the whole idea of the control in one picture.
    if !up::mono_off(drawn.mono_hz) {
        let low_y = rect.bottom() - rect.height() * 0.12;
        let stub = (bar_h * 0.75).max(stroke::BOLD);
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(centre_x, low_y), egui::vec2(stub * 2.0, stub)),
            stub * 0.5,
            theme.role_shape_dim,
        );
        painter.text(
            egui::pos2(centre_x + stub * 2.0, low_y),
            egui::Align2::LEFT_CENTER,
            format!("≤{}", Unit::Hz.format(drawn.mono_hz)),
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    }

    // --- what is engaged ----------------------------------------------
    if !drawn.tag.is_empty() {
        painter.text(
            rect.min + egui::vec2(theme.sp(space::XS), theme.sp(space::XXS)),
            egui::Align2::LEFT_TOP,
            drawn.tag,
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.role_mod,
        );
    }
}

/// Draw the utility card. Returns the edits the user just made.
pub fn utility_card(ui: &mut egui::Ui, theme: &Theme, state: &mut UtilityUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "utility", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        let tag = tag(state);
                        if let Some((pan, width)) = field(ui, theme, state, &tag) {
                            for (id, value) in [(up::PAN, pan), (up::WIDTH, width)] {
                                let norm = utility_norm(id, value);
                                if let Some(slot) = state.slot(id) {
                                    *slot = norm;
                                }
                                edits.push(ParamEdit {
                                    param: id,
                                    value: utility_value(id, norm),
                                });
                            }
                        }
                    }
                    poly_widgets::CurveRegion::Footer => {
                        footer(ui, theme, state, &mut edits);
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// One row of cells, and nothing else.
///
/// The share is recomputed from what is ACTUALLY left, cell by cell,
/// rather than divided up in advance: worked out ahead, any cell needing
/// more than its share spends the row's remainder and the last one is
/// pushed off the card's right edge.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut UtilityUi, edits: &mut Vec<ParamEdit>) {
    let h = ui.available_height();
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        for (drawn, id) in CELLS.iter().enumerate() {
            let id = *id;
            let param = param_of(id);
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let w = (room / left).floor().max(1.0);
            let Some(norm) = state.slot(id) else { continue };
            ui.allocate_ui_with_layout(
                egui::vec2(w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(w);
                    ui.set_height(h);
                    let text = cell_text(id);
                    let fmt = text.as_ref().map(|f| f as &dyn Fn(f32) -> String);
                    // A STEPPED control gets detents; a continuous one
                    // gets its bar. One grammar, two tails.
                    let moved = if param.choices().is_some() {
                        poly_widgets::labeled_cell_steps(ui, theme, &param, norm, fmt)
                    } else {
                        poly_widgets::labeled_cell_bar(ui, theme, &param, norm, fmt)
                    };
                    if moved {
                        edits.push(ParamEdit {
                            param: id,
                            value: utility_value(id, *norm),
                        });
                    }
                },
            );
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    /// Seven cells and one hero. Narrower than the compressor's nine,
    /// and it has no rails to blow it out.
    const MAX_W: f32 = 520.0;
    const MIN_W: f32 = 260.0;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            },
        );
        run.textures_delta.clear();
        out.unwrap()
    }

    // ----------------------------------------------- the standing six ---

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = utility_edits(&UtilityUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = up::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");

        // And the FACE shows every one of them: a row the engine has and
        // the card does not is a parameter nobody can reach.
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in up::TABLE {
            for i in 0..=40 {
                let value = utility_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            assert!((utility_value(def.id, 0.0) - def.min).abs() < (def.max - def.min) * 1e-3);
            assert!((utility_value(def.id, 1.0) - def.max).abs() < (def.max - def.min) * 1e-3);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in up::TABLE {
            for i in 0..=40 {
                let value = utility_value(def.id, i as f32 / 40.0);
                let again = utility_value(def.id, utility_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-4,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in up::TABLE {
            let mut state = UtilityUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = utility_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Drawing at rest must not move a control — a card that emits on its
    /// first frame writes its own defaults over a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UtilityUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| utility_card(ui, &theme, &mut state))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UtilityUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| utility_card(ui, &theme, &mut state))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    /// It also fits the NARROW rectangle a device panel really hands
    /// over, and still fills it rather than shrinking to a stripe.
    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UtilityUi::default();
        for panel_w in [500.0f32, 640.0, 900.0] {
            let host = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(panel_w, theme.sp(control::DEVICE_TALL_H) + 8.0),
            );
            let used = frame(&ctx, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(host)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(host.width());
                utility_card(&mut child, &theme, &mut state);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt wide",
                used.width()
            );
            assert!(
                used.width() > panel_w * 0.7,
                "at a {panel_w:.0} pt panel the card drew only {:.0} pt",
                used.width()
            );
        }
    }

    // -------------------------------------------------------- layout ---

    /// `notes/20260827-device-card-layout.md`, rule 1: a labelled cell is
    /// two `POLY_CELL_H` units, and the panel's own formula has to
    /// reserve them.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, CELL_UNITS, "one row of cells");

        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let needed = unit * CELL_UNITS as f32;
        assert!(
            reserved >= needed,
            "the footer reserves {reserved} for a row needing {needed}"
        );
        // And the field keeps enough height to be a picture.
        let plot = control::DEVICE_TALL_H - reserved;
        assert!(
            plot > unit * 3.0,
            "only {plot} left for the field after a {reserved} footer"
        );
    }

    /// Rule 3: the card declares a width wide enough for every cell's
    /// own widest reading — including the readings its custom formatters
    /// produce, which the `Param` knows nothing about.
    #[test]
    fn every_cell_fits_the_width_the_card_asks_for() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let declared = face_width(ui, &theme);
                assert!(declared > 0.0, "the card declared no width at all");
                let natural: f32 = CELLS
                    .iter()
                    .map(|id| cell_min_width(ui, &theme, *id))
                    .sum::<f32>()
                    + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32;
                assert!(
                    natural <= declared + 0.5,
                    "the row needs {natural} but the card asks for {declared}"
                );
                for id in CELLS {
                    let wanted = metrics::mono_w(ui, &widest_text(id), font::VALUE);
                    assert!(
                        cell_min_width(ui, &theme, id) >= wanted,
                        "{} cannot print its own widest value",
                        param_of(id).name
                    );
                }
            },
        );
        run.textures_delta.clear();
    }

    /// The custom readouts say what they mean. Pan especially: a cell
    /// printing "0.35" would be true and useless.
    #[test]
    fn the_custom_readouts_read() {
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-1.0), "L100");
        assert_eq!(pan_text(1.0), "R100");
        assert_eq!(pan_text(-0.35), "L35");
        assert_eq!(width_text(1.0), "100 %");
        assert_eq!(width_text(0.0), "0 %");
        assert_eq!(width_text(up::WIDTH_MAX), "200 %");
        assert_eq!(mono_text(up::MONO_MIN_HZ), "off");
        assert_ne!(mono_text(up::MONO_MAX_HZ), "off");
    }

    /// The strip names only what is ENGAGED — its whole value is that it
    /// is empty until something unusual is switched on.
    #[test]
    fn the_tag_is_empty_until_something_is_engaged() {
        let mut state = UtilityUi::default();
        assert_eq!(tag(&state), "", "an untouched utility has nothing to say");

        state.set_norm(up::PHASE, utility_norm(up::PHASE, up::PHASE_L as f32));
        state.set_norm(
            up::CHANNEL,
            utility_norm(up::CHANNEL, up::CHANNEL_SWAP as f32),
        );
        state.set_norm(up::DC, utility_norm(up::DC, 1.0));
        assert_eq!(tag(&state), "swap · ø L · dc");
    }

    // ------------------------------------------------------ pointers ---
    //
    // `notes/20260826-device-ui-contract.md`, rule 4. The five standing
    // tests above are all about PARAMETERS and none of them can see a
    // gesture bug.

    const PANEL: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(600.0, 240.0),
    };

    /// Drive the FIELD through a pointer path in a rect the test chose,
    /// and return what each frame reported.
    ///
    /// The field rather than the whole card, deliberately: a gesture test
    /// can only aim at a handle if it knows where the handle is, and
    /// re-deriving the plot's rectangle from the card's chrome would be a
    /// second opinion about the layout that goes stale the first time a
    /// padding token moves. Here the plot IS the rect, so the puck of a
    /// default state is its centre — pan 0 on the centre line, width 1.0
    /// halfway up a 2.0 axis. The card's own wiring is tested below.
    fn field_gesture(state: &UtilityUi, path: &[probe::Step]) -> Vec<Option<(f32, f32)>> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = *state;
        probe::run(&ctx, PANEL, path, |ui| {
            let moved = field(ui, &theme, &state, "");
            // WRITE BACK, exactly as the card does. `field` reads its
            // position from the state and adds the frame's drag delta, so
            // a harness that kept handing it the original values would
            // report each frame's movement instead of the gesture's — and
            // a drag over ten frames would look like a drag over one.
            if let Some((pan, width)) = moved {
                state.set_norm(up::PAN, utility_norm(up::PAN, pan));
                state.set_norm(up::WIDTH, utility_norm(up::WIDTH, width));
            }
            moved
        })
    }

    /// The last thing a gesture reported, if it reported anything.
    fn last(reports: Vec<Option<(f32, f32)>>) -> Option<(f32, f32)> {
        reports.into_iter().flatten().next_back()
    }

    fn centre() -> egui::Pos2 {
        PANEL.center()
    }

    /// A press on empty ground moves nothing. The background senses
    /// drags — it has to, or the puck could not be allocated over it —
    /// and it must not act on them.
    #[test]
    fn a_press_on_empty_ground_moves_nothing() {
        let corner = PANEL.min + egui::vec2(8.0, 8.0);
        let state = UtilityUi::default();
        let moved = last(field_gesture(
            &state,
            &probe::drag_path(corner, corner + egui::vec2(60.0, 0.0), 6),
        ));
        assert_eq!(moved, None, "empty ground reported {moved:?}");
    }

    /// And nothing reaches the CARD either, through its own chrome.
    #[test]
    fn a_press_on_the_card_s_empty_ground_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = UtilityUi::default();
        let corner = PANEL.min + egui::vec2(4.0, 4.0);
        let made = probe::run(
            &ctx,
            PANEL,
            &probe::drag_path(corner, corner + egui::vec2(40.0, 0.0), 6),
            |ui| utility_card(ui, &theme, &mut state),
        );
        assert!(
            made.iter().all(Vec::is_empty),
            "the card's corner emitted an edit"
        );
        assert_eq!(state, UtilityUi::default());
    }

    /// Dragging the image sideways pans it, and only pans it: the two
    /// parameters are one movement, but a horizontal movement is not a
    /// width change.
    #[test]
    fn dragging_the_image_sideways_pans_it() {
        let state = UtilityUi::default();
        let from = centre();
        let moved = last(field_gesture(
            &state,
            &probe::drag_path(from, from + egui::vec2(80.0, 0.0), 8),
        ));
        let (pan, width) = moved.expect("the image never took the drag");
        assert!(pan > 0.05, "the drag did not pan: {pan}");
        assert!(
            (width - 1.0).abs() < 1e-3,
            "a sideways drag changed the width"
        );
    }

    /// Dragging it upward widens it. Up is more, like every fader.
    #[test]
    fn dragging_the_image_upward_widens_it() {
        let state = UtilityUi::default();
        let from = centre();
        let moved = last(field_gesture(
            &state,
            &probe::drag_path(from, from - egui::vec2(0.0, 40.0), 8),
        ));
        let (pan, width) = moved.expect("the image never took the drag");
        assert!(width > 1.05, "up did not widen: {width}");
        assert!(pan.abs() < 1e-3, "an upward drag panned it");
    }

    /// A drag that PINS at the end of a range keeps going. This is bug 1
    /// of the device UI contract: a handle that stops following the
    /// pointer, the pointer running away from it, and the gesture dying
    /// halfway. Here the pan hits hard right and must not lose the grab,
    /// so pulling back returns.
    #[test]
    fn a_drag_that_pins_at_the_end_keeps_its_grab() {
        let state = UtilityUi::default();
        let from = centre();
        let far = from + egui::vec2(2_000.0, 0.0);
        let mut path = probe::drag_path(from, far, 10);
        // `drag_path` releases at its end; the return leg is spliced in
        // BEFORE that release, so the button never comes up mid-gesture.
        let release = path.pop();
        path.extend(probe::drag_path(far, from, 10));
        path.extend(release);

        let reports = field_gesture(&state, &path);
        let pinned = reports
            .iter()
            .flatten()
            .any(|(pan, _)| (*pan - 1.0).abs() < 1e-4);
        assert!(pinned, "the drag never reached the end of the range");
        let (pan, _) = last(reports).expect("the grab was lost");
        assert!(
            pan < 0.5,
            "the grab was lost at the end of the range: came back only to {pan}"
        );
    }

    /// A drag that began on the image stays the image's when it wanders
    /// off the display entirely — egui tracks an interaction by widget
    /// id, and this is the test that says so.
    #[test]
    fn a_drag_that_leaves_the_display_keeps_its_target() {
        let state = UtilityUi::default();
        let from = centre();
        let outside = egui::pos2(from.x + 60.0, PANEL.bottom() + 80.0);
        let moved = last(field_gesture(&state, &probe::drag_path(from, outside, 10)));
        let (pan, width) = moved.expect("the drag died when it left the display");
        assert!(pan > 0.05, "it stopped panning: {pan}");
        assert!(width < 0.95, "it stopped narrowing: {width}");
    }

    /// The CARD wires the field's answer into edits — both parameters,
    /// every time either moves, so the engine's copy cannot disagree with
    /// the card's about the other one.
    ///
    /// The puck is FOUND rather than assumed: the plot's rectangle is the
    /// card's business and this test has no opinion about it, so it drags
    /// down the centre column until something answers.
    #[test]
    fn the_card_turns_an_image_drag_into_two_edits() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        // The card need not fill the panel it is given, so its own
        // rectangle is measured rather than assumed. Its CENTRE column is
        // where a default utility's puck sits — pan 0 is the centre line,
        // and the plot spans the card's full inner width — which leaves
        // one axis to search instead of two.
        let drive = |state: &mut UtilityUi, path: &[probe::Step]| {
            probe::run(&ctx, PANEL, path, |ui| {
                ui.horizontal(|ui| utility_card(ui, &theme, state)).inner
            })
        };
        let card = probe::run(&ctx, PANEL, &[probe::Step::moved(PANEL.min)], |ui| {
            ui.horizontal(|ui| utility_card(ui, &theme, &mut UtilityUi::default()))
                .response
                .rect
        })
        .pop()
        .expect("the card never drew");

        let mut found = None;
        for row in 0..32 {
            let y = card.top() + card.height() * (row as f32 + 0.5) / 32.0;
            let from = egui::pos2(card.center().x, y);
            let mut state = UtilityUi::default();
            let made = drive(
                &mut state,
                &probe::drag_path(from, from + egui::vec2(70.0, 0.0), 8),
            );
            let Some(edits) = made.into_iter().rev().find(|made| !made.is_empty()) else {
                continue;
            };
            let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
            ids.sort_unstable();
            if ids == vec![up::PAN, up::WIDTH] {
                found = Some((state, edits));
                break;
            }
        }
        let (state, edits) = found.expect("nothing on the card took an image drag");
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![up::PAN, up::WIDTH],
            "an image drag must state both parameters"
        );
        assert!(
            utility_value(up::PAN, state.pan) > 0.05,
            "the card kept the drag to itself"
        );
        // Every edit carries the value the card now holds — a card that
        // emitted one number and kept another is the third bug in the
        // device UI contract.
        for edit in &edits {
            let mut after = state;
            let held = after.slot(edit.param).copied().unwrap_or_default();
            assert!(
                (utility_value(edit.param, held) - edit.value).abs() < 1e-4,
                "{} left as {} but the card holds {}",
                param_of(edit.param).name,
                edit.value,
                utility_value(edit.param, held)
            );
        }
    }
}
