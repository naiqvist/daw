//! Purpose-built controls for the poly synth.
//!
//! These are studies in the brief's "every control shows what it does"
//! rule. They still speak normalized values like the rest of `device`, but
//! replace three places where a row of generic knobs hides useful musical
//! structure:
//!
//! - [`wave_picker`] identifies waves by their shape, not only a word.
//! - [`pitch_stack`] keeps octave, semitone and fine tuning in one hierarchy.
//! - [`unison_field`] makes detune and stereo spread visible as voice dots.

use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::adjust;
use crate::ui::device::design;
use crate::ui::device::metrics::Footprint;
use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

const WAVE_COLS: usize = 4;
const WAVE_ROWS: usize = 2;
const WAVE_COUNT: usize = WAVE_COLS * WAVE_ROWS;
/// The wave names are the TABLE's, not this module's: the picker shows
/// the same list the wire carries, in the same order, so a shape added to
/// the synth appears here without a second edit. The grid's own shape
/// (four across, two down) stays local — that is layout, not vocabulary.
const WAVE_NAMES: &[&str] = crate::params::poly::WAVES;

/// Most unison voices there are to draw — the table's list length, so the
/// field and the switch that sets it can never disagree about how many
/// there could be.
pub const UNISON_MAX: usize = crate::params::poly::UNISON.len();

/// The largest footprint in this module. Card layouts can reserve this and
/// swap among the three studies without moving their neighbours.
pub fn footprint(_ui: &egui::Ui, theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::WAVE_BANK_W),
        theme.sp(control::WAVE_BANK_H),
    )
}

pub fn wave_footprint(theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::WAVE_BANK_W),
        theme.sp(control::WAVE_BANK_H),
    )
}

pub fn pitch_footprint(theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::PITCH_STACK_W),
        theme.sp(control::PITCH_STACK_H),
    )
}

pub fn unison_footprint(theme: &Theme) -> Footprint {
    Footprint::new(theme.sp(control::UNISON_W), theme.sp(control::UNISON_H))
}

/// Pick one of the synth's eight oscillator tables. Click or drag over a
/// tile, wheel over the bank, or focus it and use the arrow keys.
pub fn wave_picker(ui: &mut egui::Ui, theme: &Theme, norm: &mut f32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(wave_footprint(theme).size, egui::Sense::click_and_drag());
    let current = wave_index(*norm);
    let mut want = current;

    if let Some(pos) = response.interact_pointer_pos()
        && (response.clicked() || response.dragged())
    {
        want = wave_at(rect, pos);
    }
    let step = adjust::steps(ui, &response);
    if step != 0 {
        want = (want as i32 + step).clamp(0, WAVE_COUNT as i32 - 1) as usize;
    }

    let changed = want != current;
    if changed {
        *norm = wave_norm(want);
    }
    paint_wave_bank(ui, theme, rect, want, response.hover_pos(), &response);
    changed
}

fn wave_index(norm: f32) -> usize {
    (norm.clamp(0.0, 1.0) * (WAVE_COUNT - 1) as f32).round() as usize
}

fn wave_norm(index: usize) -> f32 {
    index.min(WAVE_COUNT - 1) as f32 / (WAVE_COUNT - 1) as f32
}

fn wave_at(rect: egui::Rect, pos: egui::Pos2) -> usize {
    let col = (((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * WAVE_COLS as f32) as usize;
    let row = (((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0) * WAVE_ROWS as f32) as usize;
    row.min(WAVE_ROWS - 1) * WAVE_COLS + col.min(WAVE_COLS - 1)
}

fn paint_wave_bank(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    selected: usize,
    hover: Option<egui::Pos2>,
    response: &egui::Response,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    let cell = egui::vec2(
        rect.width() / WAVE_COLS as f32,
        rect.height() / WAVE_ROWS as f32,
    );
    let hovered = hover.map(|p| wave_at(rect, p));

    for (i, name) in WAVE_NAMES.iter().enumerate() {
        let col = i % WAVE_COLS;
        let row = i / WAVE_COLS;
        let cell_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(cell.x * col as f32, cell.y * row as f32),
            cell,
        );
        if i == selected {
            painter.rect_filled(
                cell_rect.shrink(stroke::HAIR),
                design::screen_radius(),
                theme.role_shape_dim,
            );
        }
        if col > 0 {
            painter.line_segment(
                [cell_rect.left_top(), cell_rect.left_bottom()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        if row > 0 {
            painter.line_segment(
                [cell_rect.left_top(), cell_rect.right_top()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }

        let label_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(font::LABEL)));
        let plot = egui::Rect::from_min_max(
            cell_rect.min + egui::vec2(theme.sp(stroke::FOCUS), theme.sp(stroke::FOCUS)),
            egui::pos2(
                cell_rect.right() - theme.sp(stroke::FOCUS),
                cell_rect.bottom() - label_h,
            ),
        );
        let points: Vec<egui::Pos2> = (0..=32)
            .map(|sample| {
                let phase = sample as f32 / 32.0;
                egui::pos2(
                    egui::lerp(plot.x_range(), phase),
                    plot.center().y - wave_sample(i, phase) * plot.height() * 0.38,
                )
            })
            .collect();
        let live = i == selected || hovered == Some(i);
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(
                if live { stroke::BOLD } else { stroke::HAIR },
                if live {
                    theme.role_shape
                } else {
                    theme.text_muted
                },
            ),
        ));
        painter.text(
            egui::pos2(cell_rect.center().x, cell_rect.bottom() - label_h * 0.5),
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(font::LABEL),
            if live { theme.text } else { theme.text_muted },
        );
    }
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

fn wave_sample(wave: usize, phase: f32) -> f32 {
    let x = phase * std::f32::consts::TAU;
    match wave {
        0 => x.sin(),
        1 => 1.0 - 4.0 * (phase - 0.5).abs(),
        2 => 1.0 - 2.0 * phase,
        3 => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        4 => (x.sin() + 0.45 * (x * 2.0).sin() + 0.2 * (x * 3.0).sin()) / 1.65,
        5 => (x.sin() + 0.55 * (x * 3.0).sin() + 0.25 * (x * 7.0).sin()) / 1.8,
        6 => (x.sin() + 0.6 * (x * 5.0).sin() + 0.4 * (x * 9.0).sin()) / 2.0,
        _ => (x.sin() + 0.28 * (x * 8.0).sin() + 0.18 * (x * 13.0).sin()) / 1.46,
    }
}

/// The selected oscillator table as one large picture, with its compact
/// parameters living in the dark surface below the trace.
///
/// `footer_rows` lets the caller disclose only controls that currently
/// matter. The graph takes every point the footer does not need, so hiding
/// an irrelevant row makes the waveform larger rather than leaving a hole.
pub fn wave_display(
    ui: &mut egui::Ui,
    theme: &Theme,
    norm: &mut f32,
    ghost: Option<usize>,
    footer_rows: usize,
    add_footer: impl FnOnce(&mut egui::Ui),
) -> bool {
    let gap = theme.sp(space::XXS);
    let footer_h = theme.sp(control::POLY_CELL_H) * footer_rows as f32
        + gap * footer_rows.saturating_sub(1) as f32;
    let min_plot_h = theme.sp(control::POLY_WAVE_H);
    let height = ui.available_height().max(min_plot_h + footer_h + gap);
    let size = egui::vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let footer = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - footer_h),
        rect.right_bottom(),
    );
    let plot_rect = if footer_rows > 0 {
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), footer.top() - gap))
    } else {
        rect
    };
    let response = ui
        .interact(
            plot_rect,
            ui.id().with("poly-wave-display"),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    let current = wave_index(*norm);
    let mut want = current as i32;
    let arrow_w = theme.sp(space::XL);

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        if pos.x <= rect.left() + arrow_w {
            want -= 1;
        } else if pos.x >= rect.right() - arrow_w {
            want += 1;
        } else {
            response.request_focus();
        }
    }
    want += adjust::steps(ui, &response);
    want = want.clamp(0, WAVE_COUNT as i32 - 1);
    let changed = want as usize != current;
    if changed {
        *norm = wave_norm(want as usize);
    }

    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    if footer_rows > 0 {
        painter.line_segment(
            [footer.left_top(), footer.right_top()],
            egui::Stroke::new(stroke::HAIR, theme.divider),
        );
    }
    let label_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(font::LABEL)));
    let plot = egui::Rect::from_min_max(
        egui::pos2(plot_rect.left() + arrow_w, plot_rect.top() + label_h),
        egui::pos2(
            plot_rect.right() - arrow_w,
            plot_rect.bottom() - theme.sp(space::XXS),
        ),
    );
    // The OTHER oscillator's shape rides behind the hero as a ghost —
    // one panel now speaks for both, and the dim curve is how it says
    // what the tab you are NOT on is doing without a second display.
    let curve = |wave: usize| -> Vec<egui::Pos2> {
        (0..=64)
            .map(|sample| {
                let phase = sample as f32 / 64.0;
                egui::pos2(
                    egui::lerp(plot.x_range(), phase),
                    plot.center().y - wave_sample(wave, phase) * plot.height() * 0.42,
                )
            })
            .collect()
    };
    if let Some(other) = ghost {
        painter.add(egui::Shape::line(
            curve(other.min(WAVE_COUNT - 1)),
            egui::Stroke::new(stroke::HAIR, theme.role_shape_dim),
        ));
    }
    painter.add(egui::Shape::line(
        curve(want as usize),
        egui::Stroke::new(stroke::BOLD, theme.role_shape),
    ));
    painter.text(
        egui::pos2(plot_rect.center().x, plot_rect.top() + label_h * 0.5),
        egui::Align2::CENTER_CENTER,
        WAVE_NAMES[want as usize],
        egui::FontId::proportional(font::LABEL),
        theme.text,
    );
    for (x, text) in [
        (plot_rect.left() + arrow_w * 0.5, "‹"),
        (plot_rect.right() - arrow_w * 0.5, "›"),
    ] {
        painter.text(
            egui::pos2(x, plot_rect.center().y),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font::BODY),
            if response.hovered() {
                theme.text
            } else {
                theme.text_muted
            },
        );
    }
    if response.has_focus() {
        design::focus_ring(painter, theme, plot_rect);
    }
    response.on_hover_text("waveform — click the arrows, scroll, or use arrow keys");

    let mut footer_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(footer)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    footer_ui.spacing_mut().item_spacing.y = gap;
    add_footer(&mut footer_ui);
    changed
}

/// A region inside a composite curve surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveRegion {
    Header,
    Plot,
    Footer,
}

/// One dark visual surface with compact parameter cells embedded above
/// and/or below it. The plot owns all height those rows do not need.
///
/// Used by the filter envelope, but deliberately phrased as composition:
/// the graph remains the control that explains the sound, while the values
/// are its quiet annotation rather than a second panel competing below it.
pub fn dark_curve_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: Option<&str>,
    plot_min_h: f32,
    header_rows: usize,
    footer_rows: usize,
    mut add: impl FnMut(&mut egui::Ui, CurveRegion),
) {
    let gap = theme.sp(space::XXS);
    let header_h = theme.sp(control::POLY_CELL_H) * header_rows as f32
        + gap * header_rows.saturating_sub(1) as f32;
    let footer_h = theme.sp(control::POLY_CELL_H) * footer_rows as f32
        + gap * footer_rows.saturating_sub(1) as f32;
    let plot_min = theme.sp(plot_min_h);
    let seams = usize::from(header_rows > 0) + usize::from(footer_rows > 0);
    let height = ui
        .available_height()
        .max(plot_min + header_h + footer_h + gap * seams as f32);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let header =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + header_h));
    let footer = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - footer_h),
        rect.right_bottom(),
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(
            rect.left(),
            if header_rows > 0 {
                header.bottom() + gap
            } else {
                rect.top()
            },
        ),
        egui::pos2(
            rect.right(),
            if footer_rows > 0 {
                footer.top() - gap
            } else {
                rect.bottom()
            },
        ),
    );

    {
        let painter = ui.painter();
        painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
        painter.rect_stroke(
            rect,
            design::screen_radius(),
            egui::Stroke::new(stroke::HAIR, theme.outline),
            egui::StrokeKind::Inside,
        );
        if header_rows > 0 {
            painter.line_segment(
                [header.left_bottom(), header.right_bottom()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        if footer_rows > 0 {
            painter.line_segment(
                [footer.left_top(), footer.right_top()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
    }

    if header_rows > 0 {
        let mut header_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        header_ui.spacing_mut().item_spacing.y = gap;
        add(&mut header_ui, CurveRegion::Header);
    }

    let mut plot_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(plot)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    add(&mut plot_ui, CurveRegion::Plot);
    if let Some(title) = title {
        ui.painter().text(
            plot.left_top() + egui::vec2(theme.sp(space::XS), theme.sp(space::XXS)),
            egui::Align2::LEFT_TOP,
            title,
            egui::FontId::proportional(font::LABEL),
            theme.text_muted,
        );
    }

    if footer_rows > 0 {
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        footer_ui.spacing_mut().item_spacing.y = gap;
        add(&mut footer_ui, CurveRegion::Footer);
    }
}

/// One compact parameter line. It shows the NAME at rest and swaps to the
/// natural-unit VALUE while touched, so narrow cells never print two pieces
/// of text through one another. Drag vertically, wheel, arrows, or
/// double-click to reset. Discrete values use the halves as back/forward.
pub fn value_cell(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    value_cell_styled(ui, theme, param, norm, false)
}

/// [`value_cell`], resting on its VALUE in muted small text instead of
/// its name. For the mechanics rows: eight resting names is a wall of
/// words, eight resting values is a status line — and the hover tooltip
/// already says which knob is which.
pub fn quiet_cell(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    value_cell_styled(ui, theme, param, norm, true)
}

fn value_cell_styled(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    quiet: bool,
) -> bool {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_CELL_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *norm;
    drive_cell(ui, rect, &response, param, norm);
    paint_value_cell(ui, theme, rect, param, *norm, &response, quiet);
    response.on_hover_text(format!(
        "{} — drag, scroll, arrows; double-click resets",
        param.name
    ));
    *norm != before
}

/// The one interaction every cell shape shares: half-click and detented
/// drag for choices, accumulator drag for continuous values, wheel and
/// arrows for both. Extracted so a new cell SHAPE cannot ship a second,
/// slightly different set of manners.
fn drive_cell(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    param: &Param,
    norm: &mut f32,
) {
    if let Some(count) = param.choices() {
        let mut want = param.index(*norm) as i32;
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            want += if pos.x < rect.center().x { -1 } else { 1 };
        }
        want += adjust::steps(ui, response);
        // DRAGGING a choice steps it in detents — without this the cell
        // answers clicks and the wheel but a click-drag does nothing at
        // all, which reads as a control that sticks. The pixel remainder
        // rides per-widget memory, because the stored value snaps to a
        // choice between frames and any progress kept on `norm` would be
        // quantized away before the next frame's delta arrives.
        if response.dragged() {
            let id = response.id.with("detent-acc");
            let mut acc: f32 = if response.drag_started() {
                0.0
            } else {
                ui.data(|d| d.get_temp(id)).unwrap_or(0.0)
            };
            // BOTH axes count, summed: the cell is wider than tall and
            // half-clicks already say left/right, so a horizontal pull
            // must step too — right and up advance, left and down go
            // back. A one-axis control whose shape suggests the other
            // axis reads as broken, not strict.
            let d = response.drag_delta();
            acc += d.x - d.y;
            let detents = (acc / CHOICE_DETENT_PX).trunc();
            acc -= detents * CHOICE_DETENT_PX;
            ui.data_mut(|d| d.insert_temp(id, acc));
            want += detents as i32;
        }
        *norm = param.at_index(want.clamp(0, count as i32 - 1) as usize);
    } else {
        if response.double_clicked() {
            *norm = param.default_norm;
        } else if response.dragged() {
            let fine = if ui.input(|i| i.modifiers.shift) {
                adjust::FINE
            } else {
                1.0
            };
            let id = response.id.with("drag-acc");
            let mut acc: f32 = if response.drag_started() {
                *norm
            } else {
                ui.data(|d| d.get_temp(id)).unwrap_or(*norm)
            };
            // Right and up both raise: the cell draws a HORIZONTAL bar,
            // so hands try horizontal pulls, and a vertical-only cell
            // answers those with nothing — which feels like a control
            // that needs miles of travel rather than one that is deaf
            // in an axis.
            let d = response.drag_delta();
            acc = (acc + (d.x - d.y) / (control::DRAG_TRAVEL / fine)).clamp(0.0, 1.0);
            ui.data_mut(|d| d.insert_temp(id, acc));
            *norm = acc;
        }
        *norm = (*norm + adjust::nudge(ui, response)).clamp(0.0, 1.0);
    }
}

/// The Wavetable-style cell: the VALUE prominent, its name in whisper
/// type directly beneath — both always visible, so a row of these is
/// neither a wall of labels nor a line of orphaned numbers. `fmt`
/// overrides the value text where a parameter is heard in a different
/// unit than it rides the wire in (a percent level shown in dB).
pub fn labeled_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    fmt: Option<&dyn Fn(f32) -> String>,
) -> bool {
    labeled_cell_styled(ui, theme, param, norm, fmt, Tail::Needle)
}

/// [`labeled_cell`] with the bipolar NEEDLE replaced by a bar drawn from
/// the centre.
///
/// For a cell whose card already shows it which way it has gone. The
/// needle earns its place where the number is the only other clue — but
/// on a screen where the graphic slides off its own crosshair as you drag
/// the value, a little dial repeating that is a second picture of one
/// fact, and it reads as a stray knob on a card that has none.
pub fn labeled_cell_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    fmt: Option<&dyn Fn(f32) -> String>,
) -> bool {
    labeled_cell_styled(ui, theme, param, norm, fmt, Tail::Bar)
}

/// What a cell draws along its bottom edge to say WHERE the value sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    /// Elektron's dial, for a bipolar amount whose side must be read at
    /// a glance.
    Needle,
    /// A bar filled from the left (or from the centre, when bipolar).
    Bar,
    /// DETENTS: one cell per choice, the current one filled.
    Steps,
}

fn labeled_cell_styled(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    fmt: Option<&dyn Fn(f32) -> String>,
    tail: Tail,
) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *norm;
    drive_cell(ui, rect, &response, param, norm);

    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    // DIM THE OTHERS: while any control on this card is being dragged,
    // every cell that is not it recedes. TE's active-parameter highlight
    // is not brightening the one you hold — it is dimming the three you
    // do not, which is what makes a four-colour screen readable at a
    // glance instead of a fruit salad.
    let elsewhere = ui.ctx().dragged_id().is_some() && !response.dragged();
    let fade = |c: egui::Color32| {
        if elsewhere { c.gamma_multiply(0.45) } else { c }
    };
    if engaged {
        painter.rect_filled(rect, design::screen_radius(), theme.surface);
    }
    let value = match fmt {
        Some(f) => f(param.value(*norm)),
        None => param.format(*norm),
    };
    let mid = rect.center().x;
    // The VALUE first and larger, its family's colour; the NAME beneath
    // in small caps. Elektron's discipline — value over label, always
    // both — in TE's palette.
    painter.text(
        egui::pos2(mid, rect.top() + rect.height() * 0.30),
        egui::Align2::CENTER_CENTER,
        value,
        egui::FontId::monospace(font::VALUE),
        fade(if engaged {
            theme.role_shape
        } else {
            role_color(theme, param)
        }),
    );
    painter.text(
        egui::pos2(mid, rect.bottom() - theme.sp(space::XXS) - 3.0),
        egui::Align2::CENTER_BOTTOM,
        param.name.to_uppercase(),
        egui::FontId::proportional(font::MINI_LABEL),
        fade(theme.text_muted),
    );
    // The norm bar, one hair at the very bottom.
    let y = rect.bottom() - stroke::HAIR;
    let start = if param.bipolar { 0.5 } else { 0.0 };
    let a = rect.left() + rect.width() * start;
    let b = rect.left() + rect.width() * norm.clamp(0.0, 1.0);
    if tail == Tail::Steps
        && let Some(count) = param.choices()
        && count > 1
    {
        // DETENTS, not a bar: a stepped value's bar is a lie about the
        // positions between the marks, and the one thing worth knowing
        // about a switch is which of HOW MANY it is on. Seven ticks the
        // width of a cell say that in the space the bar already had.
        let count = count as f32;
        let seg = rect.width() / count;
        let index = param.index(*norm).min(count as usize - 1) as f32;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(stroke::HAIR, fade(theme.outline)),
        );
        let lit = egui::Rect::from_min_max(
            egui::pos2(rect.left() + seg * index, y - stroke::HAIR),
            egui::pos2(rect.left() + seg * (index + 1.0), y + stroke::HAIR),
        );
        painter.rect_filled(lit, 0.0, fade(role_color(theme, param)));
    } else if param.bipolar && tail == Tail::Needle {
        // A BIPOLAR amount gets Elektron's dial rather than a bar: a bar
        // growing from its middle has to be read twice (which side, how
        // far), where a needle is one glance.
        let d = theme.sp(space::MD);
        let dial = egui::Rect::from_center_size(
            egui::pos2(rect.right() - d * 0.5 - stroke::HAIR, rect.center().y),
            egui::vec2(d, d),
        );
        needle_dial(painter, theme, dial, *norm, fade(role_color(theme, param)));
    } else {
        painter.line_segment(
            [egui::pos2(a.min(b), y), egui::pos2(a.max(b), y)],
            egui::Stroke::new(stroke::HAIR, fade(role_color(theme, param))),
        );
    }
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    if response.dragged() {
        note_touch(
            ui,
            Touch {
                name: param.name.to_owned(),
                value: match fmt {
                    Some(f) => f(param.value(*norm)),
                    None => param.format(*norm),
                },
                color: role_color(theme, param),
            },
        );
    }
    *norm != before
}

/// [`labeled_cell`] for a STEPPED parameter: the tail becomes one detent
/// per choice, with the current one filled.
///
/// The discrete twin of [`labeled_cell_bar`], and deliberately the same
/// cell in every other respect — same value over name, same drag and
/// wheel manners, same footprint. A rail of seven labelled segments says
/// the same thing and costs four times the width; on a card that already
/// has a hero to feed, that trade is the wrong way round.
pub fn labeled_cell_steps(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    fmt: Option<&dyn Fn(f32) -> String>,
) -> bool {
    labeled_cell_styled(ui, theme, param, norm, fmt, Tail::Steps)
}

/// A value-only version of [`value_cell`] for secondary controls that do
/// not deserve a written label. Its tooltip carries the parameter name;
/// the face is reserved for the number itself.
pub fn number_cell(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_CELL_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *norm;

    if response.double_clicked() {
        *norm = param.default_norm;
    } else if response.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) {
            adjust::FINE
        } else {
            1.0
        };
        let id = response.id.with("drag-acc");
        let mut acc: f32 = if response.drag_started() {
            *norm
        } else {
            ui.data(|d| d.get_temp(id)).unwrap_or(*norm)
        };
        acc = (acc - response.drag_delta().y / (control::DRAG_TRAVEL / fine)).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(id, acc));
        *norm = acc;
    }
    *norm = (*norm + adjust::nudge(ui, &response)).clamp(0.0, 1.0);

    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        param.format(*norm),
        egui::FontId::monospace(font::LABEL),
        if response.hovered() || response.dragged() || response.has_focus() {
            theme.role_shape
        } else {
            theme.text
        },
    );
    response.on_hover_text(format!(
        "{} — drag, scroll, arrows; double-click resets",
        param.name
    ));
    *norm != before
}

fn paint_value_cell(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    param: &Param,
    norm: f32,
    response: &egui::Response,
    quiet: bool,
) {
    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    if engaged {
        painter.rect_filled(rect, design::screen_radius(), theme.surface);
    }
    // A quiet cell rests on its VALUE, smaller and dimmer — a row of
    // them reads as a status line, not a wall of labels.
    let (text, font_id) = if engaged {
        (param.format(norm), egui::FontId::monospace(font::LABEL))
    } else if quiet {
        (
            param.format(norm),
            egui::FontId::monospace(font::MINI_LABEL),
        )
    } else {
        (
            param.name.to_owned(),
            egui::FontId::proportional(font::LABEL),
        )
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font_id,
        if engaged {
            theme.role_shape
        } else if quiet {
            theme.outline
        } else {
            theme.text_muted
        },
    );
    let y = rect.bottom() - stroke::HAIR;
    let start = if param.bipolar { 0.5 } else { 0.0 };
    let a = rect.left() + rect.width() * start;
    let b = rect.left() + rect.width() * norm.clamp(0.0, 1.0);
    painter.line_segment(
        [egui::pos2(a.min(b), y), egui::pos2(a.max(b), y)],
        egui::Stroke::new(stroke::BOLD, theme.role_shape_dim),
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

/// The hero waveform: the SELECTED oscillator's shape, alone — and the
/// corner tags double as the A/B switch, returning `Some(which)` on a
/// click. The inactive tag still names the other wave, so switching is
/// never a leap in the dark; only the curve itself is one at a time.
pub fn wave_hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    a: usize,
    b: usize,
    selected: usize,
) -> (Option<usize>, i32, Option<usize>) {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    // Edge zones still step the wave, but they are NOT drawn any more —
    // see the tags below.
    let arrow_w = theme.sp(space::XL);
    let mut wave_step = adjust::steps(ui, &response);
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        if pos.x <= rect.left() + arrow_w {
            wave_step -= 1;
        } else if pos.x >= rect.right() - arrow_w {
            wave_step += 1;
        }
    }
    let painter = ui.painter();
    // NO ground of its own. An OP-1 engine screen is one black field
    // with a drawing on it — the well behind this already IS that field,
    // and a second filled rect inside it only draws a box around
    // nothing. The curve sits directly on the dark.
    //
    // The plot is inset HARD: TE spends most of the screen on emptiness
    // and lets one thin line carry it. That margin is the design, not
    // leftover space.
    let plot = rect.shrink2(egui::vec2(theme.sp(space::LG), theme.sp(space::XS)));
    let curve = |wave: usize| -> Vec<egui::Pos2> {
        (0..=64)
            .map(|sample| {
                let phase = sample as f32 / 64.0;
                egui::pos2(
                    egui::lerp(plot.x_range(), phase),
                    plot.center().y
                        - wave_sample(wave.min(WAVE_COUNT - 1), phase) * plot.height() * 0.42,
                )
            })
            .collect()
    };
    // ONLY the selected oscillator draws — one hero, one curve. The
    // other's tag in the corner still says what it is set to; the ghost
    // overlay was tried and read as clutter rather than context.
    let front = if selected == 0 { a } else { b };
    // ONE thin stroke. TE draws every waveform, envelope and meter at
    // hair weight; the boldness came from a plugin idiom, and at this
    // size it reads as a marker pen next to their needle.
    painter.add(egui::Shape::line(
        curve(front),
        egui::Stroke::new(stroke::HAIR, theme.role_shape),
    ));

    // The corner tags ARE the oscillator switch — they already name the
    // two curves, so they carry the click instead of a tab strip above
    // the panel saying the same words a second time.
    let mut clicked = None;
    let mut picked = None;
    // TE labels in TINY CAPS, in the colour of the family the control
    // belongs to — a wave is a SHAPE, so both tags are off-white, and
    // the one you are not editing is DIMMED rather than merely greyer.
    // Dimming the inactive is their whole highlight mechanism.
    let font = egui::FontId::proportional(font::MICRO_LABEL);
    for (which, align, text) in [
        (
            0usize,
            egui::Align2::LEFT_TOP,
            format!("A {}", WAVE_NAMES[a.min(WAVE_COUNT - 1)].to_uppercase()),
        ),
        (
            1,
            egui::Align2::RIGHT_TOP,
            format!("B {}", WAVE_NAMES[b.min(WAVE_COUNT - 1)].to_uppercase()),
        ),
    ] {
        let at = match align {
            egui::Align2::LEFT_TOP => plot.left_top(),
            _ => plot.right_top(),
        };
        let galley_rect = painter
            .text(
                at,
                align,
                &text,
                font.clone(),
                if which == selected {
                    theme.role_shape
                } else {
                    theme.role_shape_dim
                },
            )
            .expand(theme.sp(space::XXS));
        let response = ui
            .interact(
                galley_rect,
                ui.id().with(("hero-osc-tag", which)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if response.clicked() && which != selected {
            clicked = Some(which);
        }
        if which == selected {
            // The active tag is underlined in the curve's own colour, so
            // "which osc am I editing" has an answer at a glance — and
            // clicking it opens the WAVE MENU. The tag prints the wave's
            // name, so the tag is where the hand goes to change it; the
            // hover-only edge arrows turned out to be furniture nobody
            // finds.
            ui.painter().line_segment(
                [galley_rect.left_bottom(), galley_rect.right_bottom()],
                egui::Stroke::new(stroke::HAIR, theme.role_shape),
            );
            let current = if which == 0 { a } else { b }.min(WAVE_COUNT - 1);
            egui::Popup::menu(&response).show(|ui| {
                for (i, name) in WAVE_NAMES.iter().enumerate() {
                    if ui.selectable_label(i == current, *name).clicked() {
                        picked = Some(i);
                    }
                }
            });
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("waveform — click to choose");
        } else {
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("edit this oscillator");
        }
    }
    (clicked, wave_step, picked)
}

/// One source level, minified: its letter, a decibel readout, and a
/// hair-thin slider under both — no bar, no chrome. Three of these say
/// "a −6.0 / b −∞ / n −12.1" in the room one old rail took.
///
/// The readout is DECIBELS over a percent parameter: the wire stays the
/// table's 0–100, and the label speaks the unit a level is actually
/// heard in. 0 % prints −∞ rather than a large negative number.
pub fn source_level(
    ui: &mut egui::Ui,
    theme: &Theme,
    letter: &str,
    param: &Param,
    norm: &mut f32,
) -> bool {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_CELL_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *norm;
    if response.double_clicked() {
        *norm = param.default_norm;
    } else if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *norm = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    }
    *norm = (*norm + adjust::nudge(ui, &response)).clamp(0.0, 1.0);

    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    let pct = param.value(*norm);
    let db = if pct <= 0.0 {
        "−∞".to_owned()
    } else {
        format!("{:+.1}", 20.0 * (pct / 100.0).log10())
    };
    let text_y = rect.top() + theme.sp(space::XXS);
    painter.text(
        egui::pos2(rect.left(), text_y),
        egui::Align2::LEFT_TOP,
        letter,
        egui::FontId::proportional(font::MINI_LABEL),
        theme.role_shape,
    );
    painter.text(
        egui::pos2(rect.left() + theme.sp(space::MD), text_y),
        egui::Align2::LEFT_TOP,
        db,
        egui::FontId::monospace(font::MINI_LABEL),
        if engaged {
            theme.text
        } else {
            theme.text_muted
        },
    );
    // The slider: one hair of track, one dot of position.
    let track_y = rect.bottom() - theme.sp(space::XXS) - 1.0;
    painter.line_segment(
        [
            egui::pos2(rect.left(), track_y),
            egui::pos2(rect.right(), track_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
    let at = egui::pos2(rect.left() + rect.width() * *norm, track_y);
    painter.circle_filled(
        at,
        2.0,
        if engaged {
            theme.role_shape
        } else {
            theme.text_muted
        },
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text(format!(
        "{} — drag, scroll; double-click resets",
        param.name
    ));
    *norm != before
}

/// Which parameter FAMILY a control belongs to, and so which of the
/// four role colours it wears. Derived from the `Param`'s own unit and
/// mapping — the same source the knob formats from — so a parameter
/// cannot read as one family here and another there.
pub fn role_color(theme: &Theme, param: &Param) -> egui::Color32 {
    match param.unit {
        // A choice is a SHAPE: which wave, which mode, which colour.
        crate::ui::device::param::Unit::Choice(_) => theme.role_shape,
        // Hz and ms are TIME and pitch, the blue family.
        // A NOTE is a pitch, and pitch is the same family as time — the
        // sampler's root note sits beside its tune and fine, and three
        // colours across three controls that do one thing would read as
        // three unrelated knobs.
        crate::ui::device::param::Unit::Hz
        | crate::ui::device::param::Unit::Ms
        | crate::ui::device::param::Unit::Seconds
        | crate::ui::device::param::Unit::Note
        | crate::ui::device::param::Unit::Semitones => theme.role_time,
        // Percent and dB are AMOUNTS — ochre — except a bipolar one,
        // which is a modulation depth by construction.
        crate::ui::device::param::Unit::Percent | crate::ui::device::param::Unit::Db => {
            if param.bipolar {
                theme.role_mod
            } else {
                theme.role_level
            }
        }
        // A RATIO is a drive: gain pushed into a nonlinearity, which is
        // the destructive edge the red role names. Unipolar, so the
        // bipolar test above would have called it a level.
        crate::ui::device::param::Unit::Ratio => theme.role_mod,
        crate::ui::device::param::Unit::Plain => theme.role_level,
    }
}

/// What the touch cartridge shows this frame: a parameter's name, its
/// value, and the family colour that names its kind.
#[derive(Clone, Default)]
pub struct Touch {
    pub name: String,
    pub value: String,
    pub color: egui::Color32,
}

impl Touch {
    fn is_set(&self) -> bool {
        !self.name.is_empty()
    }
}

/// Remember that a control is being touched, for the cartridge to draw
/// once at the end of the frame. Per-context, so nested panels cannot
/// each open their own.
pub fn note_touch(ui: &egui::Ui, touch: Touch) {
    ui.ctx().data_mut(|d| d.insert_temp(touch_id(), touch));
}

fn touch_id() -> egui::Id {
    egui::Id::new("poly-touch")
}

/// TE's slide-in cartridge: the parameter you are holding, named and
/// numbered, big enough to read without leaning in.
///
/// The OP-1 shows this against the right edge whenever an encoder moves
/// — a coloured plate with the name in caps over a black box with the
/// value in large thin numerals. It solves the problem a dense card
/// always has: the control you are adjusting is small, and the value you
/// need is the one you cannot read. Nothing permanent is spent on it,
/// because it exists only while a hand is on something.
///
/// Drawn LAST, over everything, and cleared each frame — a cartridge
/// that outlived its drag would be a lie about what you are touching.
pub fn touch_cartridge(ui: &mut egui::Ui, theme: &Theme, area: egui::Rect) {
    let touch: Option<Touch> = ui.ctx().data_mut(|d| d.remove_temp(touch_id()));
    let Some(touch) = touch.filter(Touch::is_set) else {
        return;
    };
    if ui.ctx().dragged_id().is_none() {
        return;
    }

    let pad = theme.sp(space::XS);
    let w = theme.sp(control::POLY_MINI_VALUE_W) + theme.sp(space::XL);
    let plate_h = theme.sp(space::LG);
    let value_h = theme.sp(space::XXL);
    let h = plate_h + value_h + pad * 3.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(area.right() - w - pad, area.bottom() - h - pad),
        egui::vec2(w, h),
    );
    let painter = ui.painter();
    // The cartridge body, then the coloured name plate, then the black
    // readout — TE's three stacked pieces, minus the 3D knob drawing,
    // which names a physical encoder we do not have.
    painter.rect_filled(rect, design::screen_radius(), theme.surface_raised);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(pad);
    let plate = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), plate_h));
    painter.rect_filled(plate, design::screen_radius(), touch.color);
    painter.text(
        plate.center(),
        egui::Align2::CENTER_CENTER,
        touch.name.to_uppercase(),
        egui::FontId::proportional(font::MINI_LABEL),
        theme.bg,
    );
    let readout =
        egui::Rect::from_min_max(egui::pos2(inner.left(), plate.bottom() + pad), inner.max);
    painter.rect_filled(readout, design::screen_radius(), theme.surface_sunken);
    painter.text(
        readout.center(),
        egui::Align2::CENTER_CENTER,
        touch.value,
        egui::FontId::monospace(font::TITLE),
        theme.text,
    );
}

/// Elektron's needle dial, for the one case they use it: a BIPOLAR
/// amount. Their ENV cell is a circle outline with a needle sweeping
/// from it — no numbers, no track, no cap. At cell size it says
/// "somewhat negative" or "hard positive" faster than a number does,
/// and it reads as an instrument rather than a readout.
///
/// Pure geometry over a normalized value, so the angle is testable
/// without a window: 0.0 points hard left (7 o'clock), 0.5 straight up,
/// 1.0 hard right (5 o'clock) — the 270° sweep every hardware knob uses.
fn dial_needle(center: egui::Pos2, radius: f32, norm: f32) -> egui::Pos2 {
    let sweep = core::f32::consts::PI * 1.5;
    let angle = -core::f32::consts::FRAC_PI_2 - sweep * 0.5 + sweep * norm.clamp(0.0, 1.0);
    egui::pos2(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

/// Draw the dial. Outline circle, one needle, nothing else.
pub fn needle_dial(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    norm: f32,
    color: egui::Color32,
) {
    let r = (rect.width().min(rect.height()) * 0.5 - stroke::HAIR).max(2.0);
    let center = rect.center();
    painter.circle_stroke(
        center,
        r,
        egui::Stroke::new(stroke::HAIR, theme.role_shape_dim),
    );
    painter.line_segment(
        [center, dial_needle(center, r, norm)],
        egui::Stroke::new(stroke::HAIR, color),
    );
}

/// The noise glyph's sample count across its box.
const NOISE_GLYPH_STEPS: usize = 64;

/// One deterministic pseudo-random in `[-1, 1]` from a step and a seed —
/// a hash, not a generator, so the drawing needs no state and two frames
/// with one seed are identical.
fn glyph_rand(step: usize, seed: u32) -> f32 {
    let mut x = (step as u32).wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// The noise thumbnail's waveform: a burst decaying left to right.
///
/// Pure, so the drawing's claims are testable without a window: LEVEL
/// scales every sample, DECAY reshapes where the energy sits (a low norm
/// is a needle at the left, a high one sustains across the box), and
/// COLOR is the texture — white is per-step jitter, pink the same jitter
/// through a one-pole, which is exactly what the audible difference is.
fn noise_glyph_wave(color_pink: bool, level: f32, decay_norm: f32, seed: u32) -> Vec<f32> {
    // Decay in box-widths, log-spread: 0.04 (a spike) to 8 (flat).
    let tau = 0.04f32 * (8.0f32 / 0.04).powf(decay_norm.clamp(0.0, 1.0));
    let mut smooth = 0.0f32;
    (0..NOISE_GLYPH_STEPS)
        .map(|i| {
            let t = i as f32 / NOISE_GLYPH_STEPS as f32;
            let raw = glyph_rand(i, seed);
            let sample = if color_pink {
                smooth += (raw - smooth) * 0.35;
                smooth * 1.8
            } else {
                raw
            };
            sample * (-t / tau).exp() * level.clamp(0.0, 1.0)
        })
        .collect()
}

/// The noise source as ONE living thumbnail: the box draws the burst the
/// parameters describe — its shape is the decay, its height the level,
/// its texture the color — and shimmers while audible, because noise
/// that holds still is a picture of something else.
///
/// Gestures, no labels: drag up/down for level, left/right for decay,
/// scroll or click for color, double-click resets all three. The corner
/// prints the level in dB (a bare number in a noise box cannot mean
/// anything else); hover names everything. The drawing derives from the
/// same values the engine reads, so it cannot drift into decoration.
#[allow(clippy::too_many_arguments)]
pub fn noise_glyph(
    ui: &mut egui::Ui,
    theme: &Theme,
    color_param: &Param,
    color: &mut f32,
    level_param: &Param,
    level: &mut f32,
    decay_param: &Param,
    decay: &mut f32,
) -> [bool; 3] {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = [*color, *level, *decay];

    if response.double_clicked() {
        *color = color_param.default_norm;
        *level = level_param.default_norm;
        *decay = decay_param.default_norm;
    } else if response.dragged() {
        let d = response.drag_delta();
        *level = (*level - d.y / rect.height().max(1.0)).clamp(0.0, 1.0);
        *decay = (*decay + d.x / rect.width().max(1.0)).clamp(0.0, 1.0);
    }

    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    let pct = level_param.value(*level);
    let pink = color_param.index(*color) != 0;
    // The shimmer: a new seed a few times a second while audible, frozen
    // at zero — and the repaint is only requested while there is motion
    // to show.
    let seed = if pct > 0.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(90));
        (ui.input(|i| i.time) * 11.0) as u32
    } else {
        7
    };
    let wave = noise_glyph_wave(pink, (pct / 100.0).max(0.02), *decay, seed);
    let plot = rect.shrink(theme.sp(space::XXS));
    let points: Vec<egui::Pos2> = wave
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                egui::lerp(plot.x_range(), i as f32 / (wave.len() - 1) as f32),
                plot.center().y - v * plot.height() * 0.48,
            )
        })
        .collect();
    // Pink warms the line — redundant with the texture, kind to a squint.
    let line = if pct <= 0.0 {
        theme.outline
    } else if pink {
        theme.role_shape_dim
    } else if engaged {
        theme.role_shape
    } else {
        theme.text_muted
    };
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::HAIR, line),
    ));

    let db = if pct <= 0.0 {
        "−∞".to_owned()
    } else {
        format!("{:+.1}", 20.0 * (pct / 100.0).log10())
    };
    // The corners are controls you can SEE. The dB readout is a
    // mini-slide (drag it for level, same as the box's vertical drag,
    // with a track saying so); the color word is a button that flips
    // white/pink. The wheel is retired everywhere on this box.
    let db_rect = painter
        .text(
            plot.right_top(),
            egui::Align2::RIGHT_TOP,
            db.clone(),
            egui::FontId::monospace(font::MINI_LABEL),
            if engaged {
                theme.text
            } else {
                theme.text_muted
            },
        )
        .expand(theme.sp(space::XXS));
    let db_resp = ui
        .interact(
            db_rect,
            ui.id().with("noise-level"),
            egui::Sense::click_and_drag(),
        )
        .affords(Affords::Slide);
    if db_resp.double_clicked() {
        *level = level_param.default_norm;
    } else if db_resp.dragged() {
        let d = db_resp.drag_delta();
        *level = (*level + (d.x - d.y) / theme.sp(control::DRAG_TRAVEL)).clamp(0.0, 1.0);
    }
    let db_live = db_resp.hovered() || db_resp.dragged();
    let track_y = db_rect.bottom();
    painter.line_segment(
        [
            egui::pos2(db_rect.left(), track_y),
            egui::pos2(db_rect.right(), track_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
    painter.circle_filled(
        egui::pos2(
            db_rect.left() + db_rect.width() * level.clamp(0.0, 1.0),
            track_y,
        ),
        1.5,
        if db_live {
            theme.role_shape
        } else {
            theme.text_muted
        },
    );
    db_resp.on_hover_text("noise level — drag up/down");

    let color_word = if pink { "pink" } else { "white" };
    let color_rect = painter
        .text(
            plot.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            color_word,
            egui::FontId::proportional(font::MINI_LABEL),
            if pink {
                theme.role_shape_dim
            } else {
                theme.text_muted
            },
        )
        .expand(theme.sp(space::XXS));
    let color_resp = ui
        .interact(
            color_rect,
            ui.id().with("noise-color"),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    if color_resp.clicked() {
        let count = color_param.choices().unwrap_or(2) as usize;
        *color = color_param.at_index((color_param.index(*color) + 1) % count);
    }
    if color_resp.hovered() {
        ui.painter().line_segment(
            [color_rect.left_bottom(), color_rect.right_bottom()],
            egui::Stroke::new(stroke::HAIR, theme.role_shape),
        );
    }
    color_resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("noise color — click to flip white / pink");

    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text(format!(
        "noise — {} · {} dB · {}\ndrag ↕ level, ↔ decay",
        color_word,
        db,
        decay_param.format(*decay),
    ));

    [
        *color != before[0],
        *level != before[1],
        *decay != before[2],
    ]
}

/// The voice cloud's dot positions, normalized to `[-1, 1]` on both
/// axes. Pure, so the picture's claims are testable: one dot per voice,
/// SPREAD fans them apart horizontally, DETUNE scatters them vertically,
/// and one voice sits dead centre whatever the other values say —
/// exactly the engine's own rule.
fn voice_glyph_dots(voices: usize, spread: f32, detune: f32) -> Vec<(f32, f32)> {
    let n = voices.max(1);
    (0..n)
        .map(|i| {
            if n == 1 {
                return (0.0, 0.0);
            }
            let t = (i as f32 / (n - 1) as f32) * 2.0 - 1.0;
            let x = t * spread.clamp(0.0, 1.0);
            let y = glyph_rand(i, 41) * detune.clamp(0.0, 1.0);
            (x, y)
        })
        .collect()
}

/// The voice allocator as ONE thumbnail: a dot per unison voice, fanned
/// by spread and scattered by detune — the stereo image you would hear,
/// drawn. The corner words carry what dots cannot: the mode and the
/// count.
///
/// Gestures: drag ↔ spread, ↕ detune; the corner stepper counts voices
/// and the mode word cycles on click; double-click resets. Glide lives
/// on the OSC HERO's corner, opposite the level — it is note-to-note
/// travel, so it sits with the pitch it glides.
#[allow(clippy::too_many_arguments)]
pub fn voice_glyph(
    ui: &mut egui::Ui,
    theme: &Theme,
    mode_param: &Param,
    mode: &mut f32,
    unison_param: &Param,
    unison: &mut f32,
    detune_param: &Param,
    detune: &mut f32,
    spread_param: &Param,
    spread: &mut f32,
) -> [bool; 4] {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = [*mode, *unison, *detune, *spread];

    if response.double_clicked() {
        *mode = mode_param.default_norm;
        *unison = unison_param.default_norm;
        *detune = detune_param.default_norm;
        *spread = spread_param.default_norm;
    } else if response.dragged() {
        let d = response.drag_delta();
        *spread = (*spread + d.x / rect.width().max(1.0)).clamp(0.0, 1.0);
        *detune = (*detune - d.y / rect.height().max(1.0)).clamp(0.0, 1.0);
    }

    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    let voices = unison_param.index(*unison) + 1;
    let plot = rect.shrink(theme.sp(space::XS));
    for (x, y) in voice_glyph_dots(voices, *spread, *detune) {
        let at = egui::pos2(
            plot.center().x + x * plot.width() * 0.5,
            plot.center().y + y * plot.height() * 0.42,
        );
        painter.circle_filled(
            at,
            2.0,
            if engaged {
                theme.role_shape
            } else {
                theme.text_muted
            },
        );
    }

    // The corners are BUTTONS, not gestures: the mode word cycles on
    // click, and the voice count wears its own ‹ › stepper. Everything
    // the box can do is something you can see; the wheel is retired.
    let mode_word = mode_param.format(*mode);
    let mode_rect = painter
        .text(
            plot.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            mode_word.clone(),
            egui::FontId::proportional(font::MINI_LABEL),
            theme.text_muted,
        )
        .expand(theme.sp(space::XXS));
    let mode_resp = ui
        .interact(mode_rect, ui.id().with("voice-mode"), egui::Sense::click())
        .affords(Affords::Press);
    if mode_resp.clicked() {
        let count = mode_param.choices().unwrap_or(3) as usize;
        *mode = mode_param.at_index((mode_param.index(*mode) + 1) % count);
    }
    if mode_resp.hovered() {
        ui.painter().line_segment(
            [mode_rect.left_bottom(), mode_rect.right_bottom()],
            egui::Stroke::new(stroke::HAIR, theme.role_shape),
        );
    }
    mode_resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("voice mode — click to cycle poly / mono / legato");

    let count = unison_param.choices().unwrap_or(8) as i32;
    let at = unison_param.index(*unison) as i32;
    let step_rect = painter
        .text(
            plot.right_top(),
            egui::Align2::RIGHT_TOP,
            format!("‹ ×{voices} ›"),
            egui::FontId::monospace(font::MINI_LABEL),
            if engaged {
                theme.text
            } else {
                theme.text_muted
            },
        )
        .expand(theme.sp(space::XXS));
    for (half, delta, ok) in [(0u8, -1i32, at > 0), (1, 1, at + 1 < count)] {
        let zone = if half == 0 {
            egui::Rect::from_min_max(
                step_rect.min,
                egui::pos2(step_rect.center().x, step_rect.bottom()),
            )
        } else {
            egui::Rect::from_min_max(
                egui::pos2(step_rect.center().x, step_rect.top()),
                step_rect.max,
            )
        };
        let resp = ui
            .interact(
                zone,
                ui.id().with(("voice-count", half)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if resp.clicked() && ok {
            *unison = unison_param.at_index((at + delta).max(0) as usize);
        }
        if ok {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("unison voices");
        }
    }

    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text(format!(
        "voices — {mode_word} ×{voices} · detune {} · spread {}\ndrag ↔ spread, ↕ detune",
        detune_param.format(*detune),
        spread_param.format(*spread),
    ));

    [
        *mode != before[0],
        *unison != before[1],
        *detune != before[2],
        *spread != before[3],
    ]
}

/// A discrete parameter as a DROPDOWN: click the cell, pick from the
/// list. For the mod matrix's sources and destinations — eight and nine
/// entries deep — where dragging through the ladder means passing every
/// wrong answer on the way to the right one. The cell wears a chevron
/// where the slide cells wear a bar, so the two species read apart at a
/// glance.
pub fn dropdown_cell(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let before = *norm;
    let current = param.index(*norm);

    let painter = ui.painter();
    let engaged = response.hovered() || response.has_focus();
    if engaged {
        painter.rect_filled(rect, design::screen_radius(), theme.surface);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        param.format(*norm),
        egui::FontId::proportional(font::LABEL),
        if engaged {
            theme.text
        } else {
            theme.text_muted
        },
    );
    painter.text(
        egui::pos2(rect.right() - theme.sp(space::XS), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        egui::FontId::proportional(font::MINI_LABEL),
        if engaged {
            theme.role_shape
        } else {
            theme.outline
        },
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{} — click to choose", param.name));

    let count = param.choices().unwrap_or(1) as usize;
    egui::Popup::menu(&response).show(|ui| {
        for i in 0..count {
            let name = param.format(param.at_index(i));
            if ui.selectable_label(i == current, name).clicked() {
                *norm = param.at_index(i);
            }
        }
    });
    *norm != before
}

/// The kick drop, drawn: a curve starting at the pitch-envelope's depth
/// and falling (or rising, for a negative depth) back to pitch over the
/// decay. Drag vertical = depth, horizontal = decay — the same grammar
/// as the noise burst, aimed at the synth's most character-defining
/// parameter pair.
pub fn pitch_drop_glyph(
    ui: &mut egui::Ui,
    theme: &Theme,
    depth_param: &Param,
    depth: &mut f32,
    decay_param: &Param,
    decay: &mut f32,
) -> [bool; 2] {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = [*depth, *decay];
    if response.double_clicked() {
        *depth = depth_param.default_norm;
        *decay = decay_param.default_norm;
    } else if response.dragged() {
        let travel = theme.sp(control::DRAG_TRAVEL)
            * if ui.input(|i| i.modifiers.shift) {
                1.0 / adjust::FINE
            } else {
                1.0
            };
        let d = response.drag_delta();
        *depth = (*depth - d.y / travel).clamp(0.0, 1.0);
        *decay = (*decay + d.x / travel).clamp(0.0, 1.0);
    }

    let painter = ui.painter();
    let engaged = response.hovered() || response.dragged() || response.has_focus();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    let plot = rect.shrink(theme.sp(space::XXS));
    // The resting pitch, always drawn: the drop needs a floor to fall to.
    painter.line_segment(
        [plot.left_center(), plot.right_center()],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
    // Depth as a signed start height, decay as the fall's length — the
    // same log spread the noise burst uses, so the two boxes agree on
    // what "half way" means.
    let signed = *depth * 2.0 - 1.0;
    let tau = 0.04f32 * (8.0f32 / 0.04).powf(decay.clamp(0.0, 1.0));
    let points: Vec<egui::Pos2> = (0..=48)
        .map(|i| {
            let t = i as f32 / 48.0;
            egui::pos2(
                egui::lerp(plot.x_range(), t),
                plot.center().y - signed * (-t / tau).exp() * plot.height() * 0.46,
            )
        })
        .collect();
    let live = signed.abs() > 0.005;
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(
            stroke::BOLD,
            if !live {
                theme.outline
            } else if engaged {
                theme.role_shape
            } else {
                theme.text_muted
            },
        ),
    ));
    painter.text(
        plot.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        "pitch",
        egui::FontId::proportional(font::MINI_LABEL),
        theme.text_muted,
    );
    painter.text(
        plot.right_top(),
        egui::Align2::RIGHT_TOP,
        format!(
            "{} · {}",
            depth_param.format(*depth),
            decay_param.format(*decay)
        ),
        egui::FontId::monospace(font::MINI_LABEL),
        if engaged {
            theme.text
        } else {
            theme.text_muted
        },
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response
        .on_hover_text("pitch envelope — drag ↕ depth, ↔ decay; the selected oscillator's drop");
    [*depth != before[0], *decay != before[1]]
}

/// A mini-slide anchored to a corner of a PLOT: readout text with a hair
/// track and dot beneath, drag on either axis. The corner grammar the
/// glide and dB seats established, packaged so every plot can wear one.
#[allow(clippy::too_many_arguments)]
pub fn corner_slide(
    ui: &mut egui::Ui,
    theme: &Theme,
    anchor: egui::Pos2,
    align: egui::Align2,
    id_salt: &str,
    text: String,
    norm: &mut f32,
    default: f32,
    hint: &str,
) -> bool {
    let before = *norm;
    let painter = ui.painter();
    let probe = painter.layout_no_wrap(
        text.clone(),
        egui::FontId::monospace(font::MICRO_LABEL),
        theme.text_muted,
    );
    let rect = align
        .anchor_size(anchor, probe.size())
        .expand(theme.sp(space::XXS));
    let resp = ui
        .interact(rect, ui.id().with(id_salt), egui::Sense::click_and_drag())
        .affords(Affords::Steer);
    if resp.double_clicked() {
        *norm = default;
    } else if resp.dragged() {
        let travel = theme.sp(control::DRAG_TRAVEL)
            * if ui.input(|i| i.modifiers.shift) {
                1.0 / adjust::FINE
            } else {
                1.0
            };
        let d = resp.drag_delta();
        *norm = (*norm + (d.x - d.y) / travel).clamp(0.0, 1.0);
    }
    let live = resp.hovered() || resp.dragged() || resp.has_focus();
    painter.text(
        anchor,
        align,
        text,
        egui::FontId::monospace(font::MICRO_LABEL),
        if live {
            theme.role_shape
        } else {
            theme.text_muted
        },
    );
    let track_y = rect.bottom();
    painter.line_segment(
        [
            egui::pos2(rect.left(), track_y),
            egui::pos2(rect.right(), track_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
    painter.circle_filled(
        egui::pos2(rect.left() + rect.width() * norm.clamp(0.0, 1.0), track_y),
        1.5,
        if live {
            theme.role_shape
        } else {
            theme.text_muted
        },
    );
    resp.on_hover_text(hint.to_owned());
    *norm != before
}

/// A DIAL as a control: Elektron's needle in a hollow circle, with its
/// name in micro caps beneath, draggable like everything else here.
///
/// Distinct from [`corner_slide`], which is a readout wearing a track.
/// This one has no number at all — the needle IS the value, which is the
/// whole reason to spend a circle on it. Use it where the sweep matters
/// more than the digits.
#[allow(clippy::too_many_arguments)]
pub fn dial_control(
    ui: &mut egui::Ui,
    theme: &Theme,
    center: egui::Pos2,
    id_salt: &str,
    label: &str,
    param: &Param,
    norm: &mut f32,
    hint: &str,
) -> bool {
    let before = *norm;
    let d = theme.sp(control::POLY_DIAL);
    let rect = egui::Rect::from_center_size(center, egui::vec2(d, d));
    let resp = ui
        .interact(rect, ui.id().with(id_salt), egui::Sense::click_and_drag())
        .affords(Affords::Slide);
    if resp.double_clicked() {
        *norm = param.default_norm;
    } else if resp.dragged() {
        let travel = theme.sp(control::DRAG_TRAVEL)
            * if ui.input(|i| i.modifiers.shift) {
                1.0 / adjust::FINE
            } else {
                1.0
            };
        let delta = resp.drag_delta();
        *norm = (*norm + (delta.x - delta.y) / travel).clamp(0.0, 1.0);
    }
    let live = resp.hovered() || resp.dragged() || resp.has_focus();
    let painter = ui.painter();
    needle_dial(
        painter,
        theme,
        rect,
        *norm,
        if live {
            theme.role_shape
        } else {
            role_color(theme, param)
        },
    );
    painter.text(
        egui::pos2(center.x, rect.bottom() + theme.sp(space::XXS)),
        egui::Align2::CENTER_TOP,
        label.to_uppercase(),
        egui::FontId::proportional(font::MICRO_LABEL),
        if live { theme.text } else { theme.text_muted },
    );
    if resp.dragged() {
        note_touch(
            ui,
            Touch {
                name: param.name.to_owned(),
                value: param.format(*norm),
                color: role_color(theme, param),
            },
        );
    }
    resp.on_hover_text(hint.to_owned());
    *norm != before
}

/// A dropdown CHIP at a corner of a plot: the current choice with a
/// chevron, opening the full list on click — the wire cells' dropdown,
/// shrunk to annotation size so a display can carry its own settings.
pub fn corner_menu(
    ui: &mut egui::Ui,
    theme: &Theme,
    anchor: egui::Pos2,
    align: egui::Align2,
    id_salt: &str,
    param: &Param,
    norm: &mut f32,
) -> bool {
    let before = *norm;
    let current = param.index(*norm);
    let text = format!("{} ▾", param.format(*norm));
    let painter = ui.painter();
    let probe = painter.layout_no_wrap(
        text.clone(),
        egui::FontId::proportional(font::MINI_LABEL),
        theme.text_muted,
    );
    let rect = align
        .anchor_size(anchor, probe.size())
        .expand(theme.sp(space::XXS));
    let resp = ui
        .interact(rect, ui.id().with(id_salt), egui::Sense::click())
        .affords(Affords::Press);
    let live = resp.hovered() || resp.has_focus();
    painter.text(
        anchor,
        align,
        text,
        egui::FontId::proportional(font::MINI_LABEL),
        if live { theme.text } else { theme.text_muted },
    );
    if live {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(stroke::HAIR, theme.role_shape),
        );
    }
    resp.clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{} — click to choose", param.name));
    let count = param.choices().unwrap_or(1) as usize;
    egui::Popup::menu(&resp).show(|ui| {
        for i in 0..count {
            let name = param.format(param.at_index(i));
            if ui.selectable_label(i == current, name).clicked() {
                *norm = param.at_index(i);
            }
        }
    });
    *norm != before
}

/// The ghost line a new wire grows from: "+ wire", whose click opens the
/// SOURCE dropdown — picking one makes the row real, and the list only
/// ever changes on that pick.
pub fn add_wire_cell(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let before = *norm;
    let painter = ui.painter();
    let live = response.hovered() || response.has_focus();
    if live {
        painter.rect_filled(rect, design::screen_radius(), theme.surface);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+ wire",
        egui::FontId::proportional(font::MINI_LABEL),
        if live { theme.text } else { theme.outline },
    );
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("add a wire — pick its source");
    let count = param.choices().unwrap_or(1) as usize;
    egui::Popup::menu(&response).show(|ui| {
        // Skip "off": picking nothing is what NOT clicking is for.
        for i in 1..count {
            let name = param.format(param.at_index(i));
            if ui.selectable_label(false, name).clicked() {
                *norm = param.at_index(i);
            }
        }
    });
    *norm != before
}

/// Vertical drag pixels per choice step in a discrete [`value_cell`] —
/// one detent is a deliberate flick, eight choices is about a cell
/// height's travel.
const CHOICE_DETENT_PX: f32 = 14.0;

/// A source level as a labelled horizontal rail. The rail is absolute on
/// ordinary click/drag and relative in Shift-fine mode, matching faders.
pub fn level_rail(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_RAIL_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *norm;
    if response.double_clicked() {
        *norm = param.default_norm;
    } else if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *norm = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    }
    *norm = (*norm + adjust::nudge(ui, &response)).clamp(0.0, 1.0);

    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    let fill = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(
            rect.left() + rect.width() * norm.clamp(0.0, 1.0),
            rect.bottom(),
        ),
    );
    painter.rect_filled(fill, design::screen_radius(), theme.role_shape_dim);
    let pad = theme.sp(space::XS);
    painter.text(
        egui::pos2(rect.left() + pad, rect.center().y),
        egui::Align2::LEFT_CENTER,
        param.name,
        egui::FontId::proportional(font::LABEL),
        theme.text,
    );
    painter.text(
        egui::pos2(rect.right() - pad, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        param.format(*norm),
        egui::FontId::monospace(font::LABEL),
        theme.text,
    );
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text(param.name);
    *norm != before
}

/// The poly synth's one top-level navigation rail.
///
/// Four named tabs follow the signal path, each carrying the parameter-family
/// colour of the screen it opens. The whole rail is one keyboard target:
/// arrows and the wheel traverse it, while a click lands directly on a page.
/// `badges` reports quiet persistent state such as live modulation routes.
pub fn instrument_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: &mut usize,
    badges: [usize; 4],
) -> bool {
    const LABELS: [&str; 4] = ["OSC", "FILTER", "AMP", "MOD"];

    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_TAB_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let before = (*selected).min(LABELS.len() - 1);
    *selected = before;
    let cell_w = rect.width() / LABELS.len() as f32;

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *selected = (((pos.x - rect.left()) / cell_w) as usize).min(LABELS.len() - 1);
        response.request_focus();
    }
    let step = adjust::steps(ui, &response);
    if step != 0 {
        *selected = (*selected as i32 + step).rem_euclid(LABELS.len() as i32) as usize;
    }

    let bright = [
        theme.role_shape,
        theme.role_time,
        theme.role_level,
        theme.role_mod,
    ];
    let dim = [
        theme.role_shape_dim,
        theme.role_time_dim,
        theme.role_level_dim,
        theme.role_mod_dim,
    ];
    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    for (i, label) in LABELS.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            rect.min + egui::vec2(cell_w * i as f32, 0.0),
            egui::vec2(cell_w, rect.height()),
        );
        if i > 0 {
            painter.vline(
                tab.left(),
                tab.y_range(),
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        if i == *selected {
            painter.rect_filled(tab.shrink(stroke::HAIR), design::screen_radius(), dim[i]);
            painter.hline(
                tab.x_range(),
                tab.bottom() - stroke::HAIR,
                egui::Stroke::new(stroke::BOLD, bright[i]),
            );
        }
        let text = if badges[i] > 0 {
            format!("{label} {}", badges[i])
        } else {
            (*label).to_owned()
        };
        painter.text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font::MINI_LABEL),
            if i == *selected {
                bright[i]
            } else {
                theme.text_muted
            },
        );
    }
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text("instrument page — click, wheel, or use arrow keys");
    before != *selected
}

/// A two-choice local tab bar. `badge` is appended to the second label.
pub fn local_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: [&str; 2],
    selected: &mut usize,
    badge: usize,
) -> bool {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_TAB_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let before = (*selected).min(1);
    *selected = before;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *selected = usize::from(pos.x >= rect.center().x);
    }
    let half = rect.width() * 0.5;
    for (i, label) in labels.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            rect.min + egui::vec2(half * i as f32, 0.0),
            egui::vec2(half, rect.height()),
        );
        if i == *selected {
            ui.painter()
                .rect_filled(tab, design::screen_radius(), theme.role_shape_dim);
        }
        let text = if i == 1 && badge > 0 {
            format!("{label} {badge}")
        } else {
            (*label).to_owned()
        };
        ui.painter().text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font::LABEL),
            if i == *selected {
                theme.text
            } else {
                theme.text_muted
            },
        );
    }
    ui.painter().rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    before != *selected
}

/// Edit octave, semitone and fine tuning as three adjacent vertical bands.
/// The hierarchy is visible in both the labels and the step size.
pub fn pitch_stack(
    ui: &mut egui::Ui,
    theme: &Theme,
    octave: &mut f32,
    semitone: &mut f32,
    fine: &mut f32,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(pitch_footprint(theme).size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    let mut changed = false;
    let width = rect.width() / 3.0;
    for (i, norm) in [&mut *octave, &mut *semitone, &mut *fine]
        .into_iter()
        .enumerate()
    {
        let band = egui::Rect::from_min_size(
            egui::pos2(rect.left() + width * i as f32, rect.top()),
            egui::vec2(width, rect.height()),
        );
        let response = ui
            .interact(
                band,
                ui.id().with(("pitch-stack", i)),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Slide);
        let before = *norm;
        if response.double_clicked() {
            *norm = 0.5;
        } else if let Some(pos) = response.interact_pointer_pos()
            && (response.clicked() || response.dragged())
        {
            *norm = ((band.bottom() - pos.y) / band.height()).clamp(0.0, 1.0);
        }
        *norm = (*norm + adjust::nudge(ui, &response)).clamp(0.0, 1.0);
        if i < 2 {
            let steps = if i == 0 { 8.0 } else { 24.0 };
            *norm = (*norm * steps).round() / steps;
        }
        changed |= *norm != before;

        if i > 0 {
            painter.line_segment(
                [band.left_top(), band.left_bottom()],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        let (name, value) = pitch_text(i, *norm);
        painter.text(
            egui::pos2(band.center().x, band.top() + band.height() * 0.28),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(font::LABEL),
            theme.text_muted,
        );
        painter.text(
            egui::pos2(band.center().x, band.top() + band.height() * 0.66),
            egui::Align2::CENTER_CENTER,
            value,
            egui::FontId::monospace(font::VALUE),
            if response.hovered() || response.has_focus() {
                theme.role_shape
            } else {
                theme.text_value
            },
        );
        if response.has_focus() {
            design::focus_ring(painter, theme, band);
        }
    }
    changed
}

/// The three pitch bands, as the wire ids whose ranges they span. Osc A's
/// rows stand for both oscillators — the table gives A and B identical
/// ranges, and the picker is drawn once per oscillator.
const PITCH_IDS: [u32; 3] = [
    crate::params::poly::A_OCT,
    crate::params::poly::A_SEMI,
    crate::params::poly::A_FINE,
];

/// A band's label and reading, in the units its table row declares.
///
/// The numbers come from the row rather than from a literal here: an
/// octave range that grew to +-5 would otherwise keep printing +-4, and
/// nothing would fail until someone noticed the readout lying.
fn pitch_text(index: usize, norm: f32) -> (&'static str, String) {
    let id = PITCH_IDS[index.min(PITCH_IDS.len() - 1)];
    let def = crate::params::def(crate::params::poly::TABLE, id);
    let name = crate::params::poly::label(def.name);
    let value = def.min + (def.max - def.min) * norm.clamp(0.0, 1.0);
    // Octave rides the wire as an index into the choice list, so it is
    // the one band whose printed number is not its raw value.
    if id == crate::params::poly::A_OCT {
        return (
            name,
            format!("{:+.0}", crate::params::poly::octave(value.round() as u32)),
        );
    }
    if id == crate::params::poly::A_FINE {
        return (name, format!("{value:+.0} ct"));
    }
    (name, format!("{value:+.0}"))
}

/// A two-axis unison editor. Across is stereo spread; up is detune. The
/// dots show the actual number of voices and fan apart as either value grows.
pub fn unison_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    voices: usize,
    spread: &mut f32,
    detune: &mut f32,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(unison_footprint(theme).size, egui::Sense::click_and_drag());
    let before = (*spread, *detune);
    if let Some(pos) = response.interact_pointer_pos()
        && (response.clicked() || response.dragged())
    {
        *spread = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        *detune = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
    }
    let (dx, dy) = adjust::nudge_xy(ui, &response);
    *spread = (*spread + dx).clamp(0.0, 1.0);
    *detune = (*detune + dy).clamp(0.0, 1.0);
    if response.double_clicked() {
        *spread = 0.0;
        *detune = 0.0;
    }

    paint_unison(ui, theme, rect, voices, *spread, *detune, &response);
    before != (*spread, *detune)
}

fn paint_unison(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    voices: usize,
    spread: f32,
    detune: f32,
    response: &egui::Response,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, design::screen_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    painter.line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let count = voices.clamp(1, 8);
    let bounds = rect.shrink(theme.sp(control::HANDLE) * 2.0);
    for i in 0..count {
        let pitch = if count == 1 {
            0.0
        } else {
            i as f32 / (count - 1) as f32 * 2.0 - 1.0
        };
        let pan = if i == 0 {
            0.0
        } else {
            let rank = i.div_ceil(2);
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            side * rank as f32 / count.div_ceil(2) as f32
        };
        let pos = egui::pos2(
            bounds.center().x + pan * spread * bounds.width() * 0.5,
            bounds.center().y - pitch * detune * bounds.height() * 0.5,
        );
        painter.circle_filled(pos, theme.sp(control::HANDLE) * 0.65, theme.role_shape);
        painter.circle_stroke(
            pos,
            theme.sp(control::HANDLE) * 0.65,
            egui::Stroke::new(stroke::HAIR, theme.text),
        );
    }
    painter.text(
        rect.left_top() + egui::vec2(theme.sp(control::HANDLE), theme.sp(control::HANDLE)),
        egui::Align2::LEFT_TOP,
        format!(
            "{count} voices  spread {:.0}%  detune {:.0}%",
            spread * 100.0,
            detune * 100.0
        ),
        egui::FontId::monospace(font::LABEL),
        theme.text_value,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wave_round_trips_through_normalized() {
        for i in 0..WAVE_COUNT {
            assert_eq!(wave_index(wave_norm(i)), i);
        }
    }

    #[test]
    fn wave_hit_test_clamps_all_edges() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(400.0, 200.0));
        assert_eq!(wave_at(rect, egui::pos2(-100.0, -100.0)), 0);
        assert_eq!(wave_at(rect, egui::pos2(409.0, 219.0)), 7);
        assert_eq!(wave_at(rect, egui::pos2(160.0, 70.0)), 1);
        assert_eq!(wave_at(rect, egui::pos2(160.0, 170.0)), 5);
    }

    #[test]
    fn pitch_readouts_use_musical_units() {
        assert_eq!(pitch_text(0, 0.5).1, "+0");
        assert_eq!(pitch_text(1, 1.0).1, "+12");
        assert_eq!(pitch_text(2, 0.0).1, "-100 ct");
    }
    /// The stuck-choice regression, in the rack's own storage pattern: a
    /// discrete cell answered clicks and the wheel but had NO drag path,
    /// so a click-drag did nothing at all. Now a drag steps it in
    /// detents, with the pixel remainder in widget memory — because the
    /// stored value snaps to a choice between frames, and progress kept
    /// on the norm would be quantized away before the next delta.
    #[test]
    fn a_slow_drag_steps_a_discrete_value_cell() {
        let param = crate::ui::device::Param::choice(
            "wave",
            &[
                "sine", "tri", "saw", "square", "bell", "glass", "metal", "air",
            ],
        );
        let ctx = egui::Context::default();
        let mut norm = param.at_index(1);
        let start = param.index(norm);
        let center = egui::pos2(60.0, 8.0);

        let run = |events: Vec<egui::Event>, norm: &mut f32| {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                ..Default::default()
            };
            input.events = events;
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let theme = crate::ui::theme::Theme::dark();
                        value_cell(ui, &theme, &param, norm);
                    });
            });
            out.textures_delta.clear();
        };

        run(vec![egui::Event::PointerMoved(center)], &mut norm);
        run(
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut norm,
        );
        // Slow upward drag, 4 px per frame, quantized between frames the
        // way the rack stores it.
        let mut at = center;
        for _ in 0..12 {
            at.y -= 4.0;
            run(vec![egui::Event::PointerMoved(at)], &mut norm);
            norm = param.at_index(param.index(norm));
        }
        run(
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut norm,
        );
        let moved = param.index(norm) as i32 - start as i32;
        assert!(
            moved >= 2,
            "48 px of slow drag moved the choice by {moved} steps"
        );
    }
    /// The noise glyph's drawing claims, held to: level scales the whole
    /// burst, decay moves the energy (short = front-loaded, long =
    /// sustained), and pink really is smoother than white — the three
    /// facts a user reads off the box must be facts.
    #[test]
    fn the_noise_glyph_draws_what_the_parameters_say() {
        let energy = |w: &[f32]| w.iter().map(|v| v * v).sum::<f32>();

        // Level scales everything.
        let quiet = noise_glyph_wave(false, 0.25, 0.5, 3);
        let loud = noise_glyph_wave(false, 1.0, 0.5, 3);
        assert!(energy(&loud) > energy(&quiet) * 4.0);

        // A short decay front-loads: most energy in the first quarter.
        let short = noise_glyph_wave(false, 1.0, 0.0, 3);
        let head = energy(&short[..NOISE_GLYPH_STEPS / 4]);
        assert!(
            head > energy(&short) * 0.95,
            "a spike's energy leaked down the box"
        );
        // A long decay sustains: the last quarter still carries real
        // energy relative to the first.
        let long = noise_glyph_wave(false, 1.0, 1.0, 3);
        let tail = energy(&long[NOISE_GLYPH_STEPS * 3 / 4..]);
        assert!(
            tail > energy(&long) * 0.1,
            "a sustained burst died inside the box"
        );

        // Pink is SMOOTHER: neighbouring samples move less than white's.
        let jitter = |w: &[f32]| {
            w.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>() / (w.len() - 1) as f32
        };
        let white = noise_glyph_wave(false, 1.0, 1.0, 3);
        let pink = noise_glyph_wave(true, 1.0, 1.0, 3);
        assert!(
            jitter(&pink) < jitter(&white) * 0.7,
            "pink {} vs white {} — the textures do not differ",
            jitter(&pink),
            jitter(&white)
        );

        // Same seed, same drawing — the shimmer is a seed change, not
        // hidden state.
        assert_eq!(
            noise_glyph_wave(true, 0.8, 0.4, 9),
            noise_glyph_wave(true, 0.8, 0.4, 9)
        );
    }
    /// The voice cloud's geometry claims: one dot per voice, spread is
    /// the horizontal fan, detune the vertical scatter, and a single
    /// voice sits dead centre whatever else says — the engine's own rule.
    #[test]
    fn the_voice_glyph_draws_what_the_allocator_does() {
        assert_eq!(voice_glyph_dots(1, 1.0, 1.0), vec![(0.0, 0.0)]);
        for n in [2usize, 4, 8] {
            assert_eq!(voice_glyph_dots(n, 0.5, 0.5).len(), n);
        }
        let x_extent = |dots: &[(f32, f32)]| dots.iter().map(|d| d.0.abs()).fold(0.0f32, f32::max);
        let y_extent = |dots: &[(f32, f32)]| dots.iter().map(|d| d.1.abs()).fold(0.0f32, f32::max);
        let narrow = voice_glyph_dots(4, 0.2, 0.5);
        let wide = voice_glyph_dots(4, 1.0, 0.5);
        assert!(x_extent(&wide) > x_extent(&narrow) * 2.0);
        let tuned = voice_glyph_dots(4, 0.5, 0.0);
        let sour = voice_glyph_dots(4, 0.5, 1.0);
        assert_eq!(y_extent(&tuned), 0.0, "no detune, no scatter");
        assert!(y_extent(&sour) > 0.2);
    }
    /// The axis promise: a HORIZONTAL pull steps a discrete cell too.
    /// The cell is wider than tall and its half-clicks already speak
    /// left/right; a control that only listened vertically read as one
    /// that barely moves however far the mouse goes.
    #[test]
    fn a_horizontal_drag_steps_a_discrete_value_cell() {
        let param = crate::ui::device::Param::choice(
            "wave",
            &[
                "sine", "tri", "saw", "square", "bell", "glass", "metal", "air",
            ],
        );
        let ctx = egui::Context::default();
        let mut norm = param.at_index(1);
        let start = param.index(norm);
        let center = egui::pos2(60.0, 8.0);

        let run = |events: Vec<egui::Event>, norm: &mut f32| {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                ..Default::default()
            };
            input.events = events;
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let theme = crate::ui::theme::Theme::dark();
                        value_cell(ui, &theme, &param, norm);
                    });
            });
            out.textures_delta.clear();
        };

        run(vec![egui::Event::PointerMoved(center)], &mut norm);
        run(
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut norm,
        );
        let mut at = center;
        for _ in 0..12 {
            at.x += 4.0;
            run(vec![egui::Event::PointerMoved(at)], &mut norm);
            norm = param.at_index(param.index(norm));
        }
        run(
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut norm,
        );
        let moved = param.index(norm) as i32 - start as i32;
        assert!(moved >= 2, "48 px rightward moved the choice by {moved}");
    }
    /// The role palette is a SYSTEM, not decoration: each family gets a
    /// distinct hue, and a bipolar amount reads as modulation rather
    /// than as a level — the one case where the unit alone is ambiguous.
    #[test]
    fn parameter_families_get_distinct_role_colours() {
        let theme = crate::ui::theme::Theme::dark();
        let cutoff = Param::hz("cutoff", 20.0, 20_000.0);
        let level = Param::percent("level");
        let wave = Param::choice("wave", &["sine", "saw"]);
        let depth = Param::new(
            "depth",
            crate::ui::device::param::Mapping::Linear {
                min: -100.0,
                max: 100.0,
            },
            crate::ui::device::param::Unit::Percent,
        )
        .bipolar();

        let time = role_color(&theme, &cutoff);
        let amount = role_color(&theme, &level);
        let shape = role_color(&theme, &wave);
        let modulation = role_color(&theme, &depth);
        assert_eq!(time, theme.role_time);
        assert_eq!(amount, theme.role_level);
        assert_eq!(shape, theme.role_shape);
        assert_eq!(
            modulation, theme.role_mod,
            "a bipolar amount is a modulation depth, not a level"
        );
        // Four families, four DIFFERENT colours — a palette that
        // collapsed would say nothing.
        let all = [time, amount, shape, modulation];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "two families share one colour");
            }
        }
    }
    /// The dial's sweep is the hardware convention: hard left at zero,
    /// straight up at centre, hard right at one — 270 degrees, so a
    /// bipolar amount's SIGN is legible from the needle alone.
    #[test]
    fn the_needle_dial_sweeps_like_a_knob() {
        let c = egui::pos2(0.0, 0.0);
        let up = dial_needle(c, 10.0, 0.5);
        assert!(up.x.abs() < 0.01, "centre must point straight up: {up:?}");
        assert!(up.y < -9.0, "and upward, not down: {up:?}");

        let lo = dial_needle(c, 10.0, 0.0);
        let hi = dial_needle(c, 10.0, 1.0);
        assert!(lo.x < 0.0 && hi.x > 0.0, "the ends must straddle centre");
        assert!(lo.y > 0.0 && hi.y > 0.0, "both ends point below the hub");
        // Symmetric about the vertical: the same distance either side.
        assert!((lo.x + hi.x).abs() < 0.01, "sweep is lopsided");
        assert!((lo.y - hi.y).abs() < 0.01);
        // Monotonic in ANGLE — not in x, which dips further left before
        // returning on any sweep wider than 180°, exactly as a real knob
        // does. Measured from straight up, so left is negative.
        let mut last = -f32::INFINITY;
        for i in 0..=20 {
            let p = dial_needle(c, 10.0, i as f32 / 20.0);
            let from_up = p.x.atan2(-p.y);
            assert!(from_up > last - 0.01, "the needle doubled back at {i}");
            last = from_up;
        }
        assert!(last > 2.0, "the sweep is too narrow to read: {last} rad");
    }
}
