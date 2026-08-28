//! The analogue delay's device card — the repeats, drawn.
//!
//! Ids, ranges and defaults come from [`crate::params::echo`] — the one
//! table this widget, `Node::Echo::apply` and the app's edit routing all
//! read.
//!
//! # The anatomy, from `notes/20260826-instrument-screen-design-guide.md`
//!
//! ```text
//! ┌ delay ──────────────────────────────────┐  context strip
//! │ ┌ screen ─────────────────────────────┐ │
//! │ │ 1/8  35%                            │ │
//! │ │  ▌                                  │ │
//! │ │  ▌   ▌     ▌      ▌       ·         │ │  the repeats, marching
//! │ │ ─────────────────────────────────── │ │  right and dying
//! │ │  ▐   ▐     ▐      ▐       ·         │ │
//! │ │ SYNC TIME FB TONE DRIVE WOW SPR MIX │ │  parameter cells
//! │ └─────────────────────────────────────┘ │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Why this hero is WIDE where the saturator's is square
//!
//! A transfer curve has to be square: its unity diagonal must read as 45
//! degrees or the plot lies about how much is being taken off. A delay's
//! picture is a TIME AXIS, and time has no aspect ratio to honour — it
//! runs left to right and takes whatever width the card can give it.
//! Forcing it square would throw away the one dimension that carries the
//! meaning.
//!
//! So the two cards look like different instruments, and they should:
//! the screen guide's rule is that the loudest thing on a card is its most
//! musically important information, not that every card is the same shape.
//!
//! # What the picture actually shows
//!
//! Every repeat the echo will produce, at the spacing it will produce
//! them, at the height it will produce them at. Spacing is the TIME,
//! decay is the FEEDBACK, and the two rows either side of the axis are
//! the two channels — so SPREAD is visible as the right row walking out
//! of step with the left. Dragging the picture moves the two controls it
//! is mostly made of.
//!
//! It is drawn from the same numbers the engine is sent, so it cannot
//! show one echo while another one sounds.

use crate::params::echo::{DRIVE, FEEDBACK, MIX, SEND, SPREAD, SYNC, TABLE, TIME, TONE, WOW};
use crate::params::{self, echo as ep};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, adjust, card, design, knob, metrics, poly_widgets,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// How much time the picture shows, in seconds.
///
/// FIXED, not scaled to the echo. A window that stretched to fit the
/// current time would draw every delay identically — the taps always the
/// same distance apart — and the one thing the picture exists to show is
/// how far apart the repeats are. A fixed axis means a sixteenth is a
/// dense comb and a whole note is two marks, which is what those two
/// settings sound like.
const WINDOW_S: f32 = 2.0;

/// How many repeats to draw before giving up.
///
/// A bound, not a taste: at high feedback the tail is effectively
/// infinite, and a loop that ran until the repeats went inaudible would
/// be an unbounded loop in a paint call. Thirty-two marks is denser than
/// the axis can resolve anyway.
const MAX_TAPS: usize = 32;

/// Knob positions of one delay, normalized. Serialized into project
/// files, so they survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EchoUi {
    pub sync: f32,
    pub time: f32,
    pub feedback: f32,
    pub tone: f32,
    pub drive: f32,
    pub wow: f32,
    pub spread: f32,
    pub mix: f32,
    pub send: f32,
}

impl Default for EchoUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the controls use.
        let at = |id: u32| echo_norm(id, params::def(TABLE, id).default);
        Self {
            sync: at(SYNC),
            time: at(TIME),
            feedback: at(FEEDBACK),
            tone: at(TONE),
            drive: at(DRIVE),
            wow: at(WOW),
            spread: at(SPREAD),
            mix: at(MIX),
            send: at(SEND),
        }
    }
}

impl EchoUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the table cannot route an edit
    /// into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            SEND => &mut self.send,
            SYNC => &mut self.sync,
            TIME => &mut self.time,
            FEEDBACK => &mut self.feedback,
            TONE => &mut self.tone,
            DRIVE => &mut self.drive,
            WOW => &mut self.wow,
            SPREAD => &mut self.spread,
            MIX => &mut self.mix,
            _ => return None,
        })
    }
}

/// One control by wire id — the single place an id becomes a `Param`.
///
/// Every row here is already in a unit a musician would say out loud, so
/// unlike the saturator there is no engine/display conversion to own: the
/// table's number IS the displayed number. Sync is an index, exactly as
/// every other choice in the app is.
fn param_of(id: u32) -> Param {
    let def = params::def(TABLE, id);
    let pct = |name| Param::percent(name).with_default(def.default);
    match id {
        SYNC => Param::choice("sync", ep::SYNC_NAMES).with_default(def.default),
        // LOG-mapped, both of them: a delay time and a filter corner are
        // heard in ratios, and a linear dial spends most of its travel in
        // the half nobody reaches for.
        TIME => Param::new(
            "time",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Ms,
        )
        .with_default(def.default),
        TONE => Param::hz("tone", def.min, def.max).with_default(def.default),
        FEEDBACK => pct("fb"),
        DRIVE => pct("drive"),
        WOW => pct("wow"),
        SPREAD => pct("spread"),
        SEND => pct("send"),
        _ => pct("mix"),
    }
}

/// The engine-facing value at a normalized position, by param id.
pub fn echo_value(param: u32, norm: f32) -> f32 {
    params::def(TABLE, param).clamp(param_of(param).value(norm))
}

/// The inverse of [`echo_value`], for a state stored in engine units.
pub fn echo_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Whether a parameter snaps to named settings rather than sweeping.
pub fn echo_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of
/// the time or the tone moves in ratios exactly as the control does.
pub fn echo_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// The eight cells of the strip, in the order they are drawn: the signal
/// path's own order, time first and mix last.
const STRIP: [u32; 8] = [SYNC, TIME, FEEDBACK, TONE, DRIVE, WOW, SPREAD, MIX];

/// The controls that live on the SCREEN rather than in the strip.
///
/// The send is the one parameter here that is not about how the delay
/// sounds but about where it sits — so it does not belong in a row that
/// reads left to right as the signal path. It goes in the picture's top
/// right, opposite the corner tag, where it annotates the whole device
/// instead of taking a place in its chain.
const SCREEN: [u32; 1] = [SEND];

/// Every parameter as an edit, whether or not it moved.
pub fn echo_edits(state: &EchoUi) -> Vec<ParamEdit> {
    let mut state = *state;
    TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: echo_value(def.id, *norm),
            })
        })
        .collect()
}

/// What ONE cell needs: room for the widest thing it will ever print.
///
/// The strip's width is the sum of these and each cell is drawn at its
/// own, so the reservation and the drawing cannot disagree — a share is
/// not a sum, and reserving one while drawing the other is how a readout
/// ends up printing through its neighbour.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

fn strip_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    let sum: f32 = STRIP
        .iter()
        .map(|id| cell_width(ui, theme, &param_of(*id)))
        .sum();
    sum + ui.spacing().item_spacing.x * (STRIP.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in — two,
/// because a cell draws a value over a name and one row cannot hold both
/// without them touching.
const VALUE_ROWS: usize = 2;

/// The echo time in SECONDS at a knob position, for the picture.
///
/// The card has no transport, so a SYNCED echo is drawn at the tempo the
/// picture assumes rather than the one playing. That is a real
/// limitation and it is written down here rather than hidden: the
/// spacing of a synced delay's marks is indicative, and the cell beside
/// it names the division exactly. Feeding the card the live tempo would
/// make the picture literal, and is the obvious next thing to do to it.
const ASSUMED_BPM: f32 = 120.0;

fn time_seconds(state: &EchoUi) -> f32 {
    let sync = param_of(SYNC).index(state.sync) as u32;
    if sync == 0 {
        echo_value(TIME, state.time) * 1e-3
    } else {
        let beats = ep::DIVISION_BEATS
            .get((sync - 1) as usize)
            .copied()
            .unwrap_or(1.0);
        beats * 60.0 / ASSUMED_BPM
    }
}

/// Draw the repeats, and let them be dragged.
///
/// Vertical drag is FEEDBACK — up is more, and the tail visibly grows.
/// Horizontal drag is the TIME: the milliseconds when the echo is free,
/// and the division when it is synced, because those are the same
/// gesture asking for the same thing and a synced delay has no
/// milliseconds to scale.
///
/// Returns the ids that moved.
fn taps(ui: &mut egui::Ui, theme: &Theme, state: &mut EchoUi) -> Vec<u32> {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let mut moved = Vec::new();

    // The send's corner, reserved before the picture reads a gesture: the
    // knob sits ON the plot, so the plot has to know where not to listen.
    // Geometry rather than z-order, because both widgets are allocated in
    // the same frame and "whoever was added last wins" is not a rule this
    // card should be resting a drag on.
    let pad = theme.sp(space::XS);
    // Laid out from the RIGHT edge inward, so the list can grow without
    // the corner drifting: whatever `SCREEN` names ends up in the corner
    // opposite the tag, in the order it is written.
    let mut corner = rect.right() - pad;
    let screen: Vec<(Param, egui::Rect)> = SCREEN
        .iter()
        .map(|id| {
            let param = param_of(*id);
            let foot = knob::footprint_mini(ui, theme, &param);
            let at = egui::Rect::from_min_size(
                egui::pos2(corner - foot.width(), rect.top() + pad),
                foot.size,
            );
            corner = at.left() - pad;
            (param, at)
        })
        .collect();
    let knob_rect = screen
        .iter()
        .fold(egui::Rect::NOTHING, |all, (_, at)| all.union(*at));
    // Where the gesture STARTED, not where the pointer is now: a drag that
    // began on the dial belongs to the dial all the way, even when it
    // wanders back over the marks.
    let from_knob = response
        .interact_pointer_pos()
        .is_some_and(|at| knob_rect.contains(at));
    let over_knob = ui
        .ctx()
        .pointer_latest_pos()
        .is_some_and(|at| knob_rect.contains(at));

    if response.dragged() && !from_knob {
        let d = response.drag_delta();
        if d.y != 0.0 {
            let next = (state.feedback - d.y / rect.height()).clamp(0.0, 1.0);
            if next != state.feedback {
                state.feedback = next;
                moved.push(FEEDBACK);
            }
        }
        if d.x != 0.0 {
            let sync = param_of(SYNC);
            if sync.index(state.sync) == 0 {
                let next = (state.time + d.x / rect.width()).clamp(0.0, 1.0);
                if next != state.time {
                    state.time = next;
                    moved.push(TIME);
                }
            }
        }
    }
    // A synced echo steps DIVISIONS instead, in detents — the same
    // horizontal gesture, quantized to the only times it is allowed to
    // have.
    let sync = param_of(SYNC);
    if sync.index(state.sync) > 0 && !over_knob {
        let steps = adjust::steps(ui, &response);
        if steps != 0 {
            let count = sync.choices().unwrap_or(1) as i32;
            let want = (sync.index(state.sync) as i32 + steps).clamp(1, count - 1);
            let next = sync.at_index(want as usize);
            if next != state.sync {
                state.sync = next;
                moved.push(SYNC);
            }
        }
    }
    if response.hovered() && !over_knob {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let painter = ui.painter();
    // NO ground of its own: the screen behind this already IS the dark
    // field, and a second filled rect inside it draws a box around
    // nothing.
    let axis_y = rect.center().y;

    // Fixed time-address grid. The hero already promises a literal two-second
    // window, so these divisions are calibration marks rather than decorative
    // scope furniture. Half-seconds carry numbers; quarter-seconds stay quiet.
    for division in 0..=8 {
        let along = division as f32 / 8.0;
        let x = egui::lerp(rect.x_range(), along);
        let major = division % 2 == 0;
        painter.vline(
            x,
            rect.y_range(),
            egui::Stroke::new(
                stroke::HAIR,
                if major {
                    theme.grid_beat
                } else {
                    theme.grid_sub
                },
            ),
        );
        if major {
            let seconds = along * WINDOW_S;
            painter.text(
                egui::pos2(x, rect.bottom() - theme.sp(space::XXS)),
                if division == 0 {
                    egui::Align2::LEFT_BOTTOM
                } else if division == 8 {
                    egui::Align2::RIGHT_BOTTOM
                } else {
                    egui::Align2::CENTER_BOTTOM
                },
                format!("{seconds:.1}"),
                egui::FontId::monospace(font::MICRO_LABEL),
                theme.text_muted,
            );
        }
    }
    painter.line_segment(
        [
            egui::pos2(rect.left(), axis_y),
            egui::pos2(rect.right(), axis_y),
        ],
        egui::Stroke::new(stroke::BOLD, theme.grid_beat),
    );

    // Two tiny lane registrations explain the up/down grammar without a
    // legend. They sit on the shared zero line, like channel markings on a
    // piece of test equipment.
    let lane_font = egui::FontId::monospace(font::MICRO_LABEL);
    let lane_x = rect.left() + theme.sp(space::XS);
    painter.text(
        egui::pos2(lane_x, axis_y - theme.sp(space::XXS)),
        egui::Align2::LEFT_BOTTOM,
        "L",
        lane_font.clone(),
        theme.role_time_dim,
    );
    painter.text(
        egui::pos2(lane_x, axis_y + theme.sp(space::XXS)),
        egui::Align2::LEFT_TOP,
        "R",
        lane_font,
        theme.role_time_dim,
    );

    let time = time_seconds(state).max(1e-4);
    let feedback = echo_value(FEEDBACK, state.feedback) * 0.01;
    let spread = echo_value(SPREAD, state.spread) * 0.01;
    let mix = echo_value(MIX, state.mix) * 0.01;
    let drive = echo_value(DRIVE, state.drive) * 0.01;
    // Height is the WET level, so turning the mix down shrinks the marks
    // — the picture is of what you will hear, not of what the loop is
    // doing privately. The SQUARE ROOT of the mix, not the mix: level is
    // read by eye against the axis, and a linear amplitude spends most
    // of its travel down where two marks a few percent apart look
    // identical. At the 30% default a linear scale drew the tallest mark
    // at under a sixth of the panel — legible as a dot, not as a level.
    let full = (rect.height() * 0.5 - theme.sp(space::XS)).max(1.0) * mix.max(0.05).sqrt();

    let mut level = 1.0f32;
    for tap in 1..=MAX_TAPS {
        let at = time * tap as f32;
        if at > WINDOW_S {
            break;
        }
        // Below a quarter of a percent there is nothing to see and
        // nothing to hear.
        if level < 0.0025 {
            break;
        }
        let h = full * level;
        for (ch, dir) in [(0usize, -1.0f32), (1, 1.0)] {
            // The right channel's repeats run later by the spread, so
            // the two rows walk out of step — one control, visible.
            let when = if ch == 1 { at * (1.0 + spread) } else { at };
            if when > WINDOW_S {
                continue;
            }
            let x = rect.left() + rect.width() * (when / WINDOW_S);
            let end = egui::pos2(x, axis_y + dir * h);
            painter.line_segment(
                [egui::pos2(x, axis_y), end],
                // `role_time`: a repeat's position IS a time, and the
                // blue family is what names time everywhere else.
                egui::Stroke::new(stroke::MARK, theme.role_time),
            );
            // A short terminal cap makes each impulse read as a measured
            // event instead of an anonymous bar. Its width is constant; only
            // the bar height continues to report level.
            let cap = theme.sp(space::XXS);
            painter.line_segment(
                [end - egui::vec2(cap, 0.0), end + egui::vec2(cap, 0.0)],
                egui::Stroke::new(stroke::BOLD, theme.role_time),
            );
            if drive > 0.001 {
                // Drive colours the repeats in the DSP. A red registration
                // point grows with that amount, but does not claim to be a
                // second waveform or another tap.
                painter.circle_filled(
                    end,
                    theme.sp(stroke::HAIR) + theme.sp(space::XXS) * drive,
                    theme.role_mod_dim.lerp_to_gamma(theme.role_mod, drive),
                );
            }
        }
        // AFTER the mark, not before it: `tick` emits the pre-loop read,
        // so the first repeat leaves at unity and only the ones behind
        // it have been round the loop. Decaying before drawing put the
        // whole picture a feedback-power too low — and at feedback 0 it
        // drew nothing at all, for an echo that plainly makes one
        // repeat.
        level *= feedback;
    }

    // The corner tag, Elektron's values-at-the-display-edge: what the
    // marks are, for when your eyes are on them rather than on the cells.
    let sync_now = param_of(SYNC);
    let label = if sync_now.index(state.sync) == 0 {
        param_of(TIME).format(state.time)
    } else {
        sync_now.format(state.sync)
    };
    painter.text(
        rect.left_top() + egui::vec2(design::gap(theme), design::gap(theme)),
        egui::Align2::LEFT_TOP,
        format!("{label}  {}", param_of(FEEDBACK).format(state.feedback)),
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.top() + design::gap(theme)),
        egui::Align2::CENTER_TOP,
        "STEREO REPEAT FIELD // 2.0 S",
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.role_time_dim,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }

    // The send, last: it is drawn ON the picture, so it goes down after
    // the marks it sits over.
    for (id, (param, at)) in SCREEN.iter().zip(&screen) {
        let Some(norm) = state.slot(*id) else {
            continue;
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(*at)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        if knob::mini(&mut child, theme, param, norm) {
            moved.push(*id);
        }
    }
    moved
}

/// The card's layout: one well, and the screen fills it.
fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme), 0.0))
        .filling()])
}

/// Draw the delay card. Returns the edits the user just made.
pub fn echo_card(ui: &mut egui::Ui, theme: &Theme, state: &mut EchoUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "delay", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        for param in taps(ui, theme, state) {
                            if let Some(norm) = state.slot(param) {
                                edits.push(ParamEdit {
                                    param,
                                    value: echo_value(param, *norm),
                                });
                            }
                        }
                    }
                    // Each cell at its OWN width, not an equal share, so
                    // the strip is as wide as its numbers need and a
                    // readout cannot print through its neighbour.
                    poly_widgets::CurveRegion::Footer => {
                        let h = ui.available_height();
                        ui.horizontal(|ui| {
                            for id in STRIP {
                                let param = param_of(id);
                                let w = cell_width(ui, theme, &param);
                                let Some(norm) = state.slot(id) else {
                                    continue;
                                };
                                ui.allocate_ui_with_layout(
                                    egui::vec2(w, h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(w);
                                        ui.set_height(h);
                                        if poly_widgets::labeled_cell_bar(
                                            ui, theme, &param, norm, None,
                                        ) {
                                            edits.push(ParamEdit {
                                                param: id,
                                                value: echo_value(id, *norm),
                                            });
                                        }
                                    },
                                );
                            }
                        });
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        // The card must cover the table exactly: an id the table knows
        // but the card never emits is a control that silently stopped
        // working.
        let edits = echo_edits(&EchoUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
        // And the FACE shows every one of them — a row the engine has and
        // the card does not is a parameter nobody can reach. The strip is
        // no longer the whole face: the send lives on the screen.
        let mut shown = STRIP.to_vec();
        shown.extend_from_slice(&SCREEN);
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    /// Every position a control can be dragged to is a value the engine
    /// will accept, and the ends of each control reach the ends of its
    /// row — so clamping cannot be hiding a mapping that never gets
    /// there.
    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in TABLE {
            for i in 0..=40 {
                let value = echo_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            assert_eq!(echo_value(def.id, 0.0), def.min, "{}: floor", def.name);
            assert_eq!(echo_value(def.id, 1.0), def.max, "{}: ceiling", def.name);
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let edits = echo_edits(&EchoUi::default());
        for def in TABLE {
            let got = edits
                .iter()
                .find(|e| e.param == def.id)
                .map(|e| e.value)
                .unwrap_or(f32::NAN);
            assert!(
                (got - def.default).abs() <= def.default.abs() * 1e-3 + 1e-4,
                "{} loads at {got}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// A value that comes back from the engine must put the control where
    /// that value lives. Stated over values rather than positions,
    /// because a discrete row quantizes on the way in.
    #[test]
    fn value_and_norm_round_trip() {
        for def in TABLE {
            for i in 0..=40 {
                let norm = i as f32 / 40.0;
                let value = echo_value(def.id, norm);
                let again = echo_value(def.id, echo_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-4,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    /// The card fits its silhouette — the test the saturator's first
    /// layout needed and did not have.
    #[test]
    fn the_card_stays_inside_its_budget() {
        // Wider than the saturator on purpose: eight cells, and a hero
        // that is a time axis rather than a square.
        const MAX_W: f32 = 560.0;
        const MIN_W: f32 = 300.0;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = EchoUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| echo_card(ui, &theme, &mut state))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    /// Drawing at rest must not move a control — a card that emits on its
    /// first frame writes its own defaults over a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = EchoUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| echo_card(ui, &theme, &mut state))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// The picture agrees with the engine about how far apart the repeats
    /// are: a free echo's marks are spaced by its milliseconds, and a
    /// synced one's by its division.
    #[test]
    fn the_picture_spaces_repeats_the_way_the_engine_will() {
        // Free, at a known time.
        let mut state = EchoUi {
            sync: param_of(SYNC).at_index(0),
            time: echo_norm(TIME, 500.0),
            ..EchoUi::default()
        };
        assert!((time_seconds(&state) - 0.5).abs() < 1e-3);

        // Synced: a quarter at the assumed tempo is half a second, and a
        // whole note is four times that.
        for (index, beats) in [(3usize, 1.0f32), (1, 4.0)] {
            state.sync = param_of(SYNC).at_index(index);
            let want = beats * 60.0 / ASSUMED_BPM;
            assert!(
                (time_seconds(&state) - want).abs() < 1e-3,
                "{} should be {want} s, drew {}",
                ep::SYNC_NAMES[index],
                time_seconds(&state)
            );
        }
    }
}
