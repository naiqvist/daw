//! The track headers: names, badges, mute and solo, pan, gain, meters —
//! and the master header beside them.
//!
//! Geometry first, drawing second. `TrackHeaderLayout` is pure
//! rectangles shared by the paint, the hit tests and the tests, because
//! the easiest way for a dense header to look homemade is for its name,
//! badge, controls and meter each to have a slightly different idea of
//! where the space ends.
use super::*;

/// The exact tiles of one arrangement track header.
///
/// Kept pure and shared by paint, hit-testing and tests: the easiest way for
/// a dense header to look homemade is for its name, badge, controls and meter
/// to each have a slightly different idea of where the available space ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackHeaderLayout {
    pub(crate) identity: egui::Rect,
    pub(crate) name: egui::Rect,
    pub(crate) kind: egui::Rect,
    pub(crate) meter: egui::Rect,
    pub(crate) controls: Option<TrackHeaderControls>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackHeaderControls {
    pub(crate) mute: egui::Rect,
    pub(crate) solo: egui::Rect,
    pub(crate) pan_value: egui::Rect,
    pub(crate) pan: egui::Rect,
}

pub(crate) fn track_header_layout(head: egui::Rect) -> TrackHeaderLayout {
    let meter = egui::Rect::from_min_max(
        egui::pos2(
            (head.right() - HEADER_METER_W).max(head.left()),
            (head.top() + space::XXS).min(head.bottom()),
        ),
        egui::pos2(head.right(), (head.bottom() - space::XXS).max(head.top())),
    );
    let content_right = (meter.left() - HEADER_METER_GAP).max(head.left());
    let identity = egui::Rect::from_min_max(
        egui::pos2(
            (head.left() + HEADER_PAD).min(content_right),
            (head.top() + HEADER_PAD * 0.5).min(head.bottom()),
        ),
        egui::pos2(
            content_right,
            (head.top() + HEADER_PAD * 0.5 + HEADER_NAME_H).min(head.bottom()),
        ),
    );

    // MIDI is shorter than AUDIO, but the cell is not: the badge column is
    // stable across the stack, which is what lets the names form one clean
    // left-aligned list instead of ragging around their metadata.
    let kind_w = HEADER_KIND_W;
    let kind = egui::Rect::from_min_max(
        egui::pos2(
            (identity.right() - kind_w).max(identity.left()),
            identity.top(),
        ),
        identity.max,
    );
    let name = egui::Rect::from_min_max(
        identity.min,
        egui::pos2(
            (kind.left() - HEADER_PAD).max(identity.left()),
            identity.bottom(),
        ),
    );

    let controls = (head.height() >= HEADER_ROWS_MIN_H).then(|| {
        let row_y = identity.bottom() + HEADER_PAD * 0.5;
        let mute = egui::Rect::from_min_size(
            egui::pos2(head.left() + HEADER_PAD, row_y),
            egui::vec2(HEADER_BTN, HEADER_BTN),
        );
        let solo = mute.translate(egui::vec2(HEADER_BTN + HEADER_CONTROL_GAP, 0.0));
        let pan = egui::Rect::from_min_size(
            egui::pos2(
                content_right - HEADER_KNOB,
                row_y + (HEADER_BTN - HEADER_KNOB) * 0.5,
            ),
            egui::vec2(HEADER_KNOB, HEADER_KNOB),
        );
        let pan_value = egui::Rect::from_min_max(
            egui::pos2(solo.right() + space::XS, row_y),
            egui::pos2(
                (pan.left() - space::XS).max(solo.right()),
                row_y + HEADER_BTN,
            ),
        );
        TrackHeaderControls {
            mute,
            solo,
            pan_value,
            pan,
        }
    });

    TrackHeaderLayout {
        identity,
        name,
        kind,
        meter,
        controls,
    }
}

/// A small bipolar pan knob, drawn into `rect`.
///
/// Deliberately not `kit::knob`: this one lives inside a lane rather than
/// on a device card, so it is sized here rather than by the theme's control
/// scale, and it has a center DETENT — pan's most-wanted value is exactly
/// 0.0, and a knob you cannot return to center by hand is a knob you end up
/// fighting. Double-click centers it outright.
///
/// `pan` is `-1..=1`. Returns true when the user moved it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PanOutcome {
    pub(crate) changed: bool,
    pub(crate) engaged: bool,
}

pub(crate) fn pan_knob(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    pan: &mut f32,
) -> PanOutcome {
    let id = ui
        .id()
        .with(("pan", rect.left_top().x as i32, rect.top() as i32));
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .affords(Affords::Slide);
    let mut changed = false;

    if response.drag_started() {
        ui.data_mut(|data| {
            data.insert_temp(id.with("origin"), *pan);
            data.remove::<bool>(id.with("cancelled"));
        });
    }
    let cancelled = ui
        .data(|data| data.get_temp::<bool>(id.with("cancelled")))
        .unwrap_or(false);

    if response.double_clicked() {
        if *pan != 0.0 {
            *pan = 0.0;
            changed = true;
        }
    } else if response.dragged() && !cancelled {
        // Full travel over four knob-heights, tenth-speed with Shift —
        // the same feel as every other knob in the app. Absolute from the
        // press-time value: accumulated drag_delta applied to an already
        // changed value accelerates once per frame and makes a knob depend
        // on refresh rate.
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = rect.height() * 4.0 * if fine { 10.0 } else { 1.0 };
        let delta = -response.drag_delta().y / travel * 2.0;
        let origin = ui
            .data(|data| data.get_temp::<f32>(id.with("origin")))
            .unwrap_or(*pan);
        let next = (origin + delta).clamp(-1.0, 1.0);
        // The detent: crossing the middle STICKS there for a moment instead
        // of sliding through it.
        let next = if next.abs() < PAN_DETENT { 0.0 } else { next };
        if next != *pan {
            *pan = next;
            changed = true;
        }
    }

    // Escape is gesture-local: restore the value from the press, consume the
    // key, and leave no edit for history to bank.
    if let Some(origin) = ui.data(|data| data.get_temp::<f32>(id.with("origin")))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        changed |= *pan != origin;
        *pan = origin;
        ui.data_mut(|data| data.insert_temp(id.with("cancelled"), true));
    }
    if response.drag_stopped() {
        ui.data_mut(|data| {
            data.remove::<f32>(id.with("origin"));
            data.remove::<bool>(id.with("cancelled"));
        });
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    response.clone().on_hover_text(format!(
        "Pan {}\nDrag vertically · Shift for fine adjustment · Double-click to center",
        pan_label(*pan)
    ));

    // --- paint ------------------------------------------------------------
    const START: f32 = std::f32::consts::PI * 0.75;
    const SWEEP: f32 = std::f32::consts::PI * 1.5;
    let center = rect.center();
    let radius = rect.width() * 0.5 - stroke::BOLD;
    let painter = ui.painter();
    painter.circle_filled(center, radius, theme.surface_sunken);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    let arc = |from: f32, to: f32, s: egui::Stroke| {
        const STEPS: usize = 16;
        let points: Vec<egui::Pos2> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                center + kit::knob_dir(a) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };

    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let mid = START + SWEEP * 0.5;
    let at = START + SWEEP * ((*pan + 1.0) * 0.5).clamp(0.0, 1.0);
    if (at - mid).abs() > f32::EPSILON {
        arc(mid, at, egui::Stroke::new(stroke::BOLD, theme.accent));
    }
    let dir = kit::knob_dir(at);
    painter.line_segment(
        [center + dir * (radius * 0.35), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );
    // The 12-o'clock tick: where center IS, so the detent is visible.
    painter.line_segment(
        [
            egui::pos2(center.x, rect.top() - 1.0),
            egui::pos2(center.x, rect.top() + 1.0),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    PanOutcome {
        changed,
        engaged: response.clicked() || response.drag_started() || response.dragged(),
    }
}

/// A unipolar level knob: silence at the bottom, unity at twelve o'clock,
/// `MASTER_GAIN_MAX` at the top.
///
/// The pan knob's twin in feel — same travel, same fine modifier, same
/// escape-cancels-the-gesture rule — because a hand that has learned one
/// knob in this app has learned all of them. What differs is the DETENT:
/// pan sticks at centre, a fader sticks at unity, which is the value each
/// one has to be able to find without looking.
pub(crate) fn gain_knob(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    amp: &mut f32,
) -> PanOutcome {
    let id = ui
        .id()
        .with(("gain", rect.left_top().x as i32, rect.top() as i32));
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .affords(Affords::Slide);
    let mut changed = false;

    if response.drag_started() {
        ui.data_mut(|data| {
            data.insert_temp(id.with("origin"), *amp);
            data.remove::<bool>(id.with("cancelled"));
        });
    }
    let cancelled = ui
        .data(|data| data.get_temp::<bool>(id.with("cancelled")))
        .unwrap_or(false);

    if response.double_clicked() {
        if *amp != 1.0 {
            *amp = 1.0;
            changed = true;
        }
    } else if response.dragged() && !cancelled {
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = rect.height() * 4.0 * if fine { 10.0 } else { 1.0 };
        let delta = -response.drag_delta().y / travel * MASTER_GAIN_MAX;
        let origin = ui
            .data(|data| data.get_temp::<f32>(id.with("origin")))
            .unwrap_or(*amp);
        let next = (origin + delta).clamp(0.0, MASTER_GAIN_MAX);
        let next = if (next - 1.0).abs() < GAIN_DETENT {
            1.0
        } else {
            next
        };
        if next != *amp {
            *amp = next;
            changed = true;
        }
    }

    if let Some(origin) = ui.data(|data| data.get_temp::<f32>(id.with("origin")))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        changed |= *amp != origin;
        *amp = origin;
        ui.data_mut(|data| data.insert_temp(id.with("cancelled"), true));
    }
    if response.drag_stopped() {
        ui.data_mut(|data| {
            data.remove::<f32>(id.with("origin"));
            data.remove::<bool>(id.with("cancelled"));
        });
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    response.clone().on_hover_text(format!(
        "Level {} dB\nDrag vertically · Shift for fine adjustment · Double-click for unity",
        volume_label(*amp)
    ));

    const START: f32 = std::f32::consts::PI * 0.75;
    const SWEEP: f32 = std::f32::consts::PI * 1.5;
    let center = rect.center();
    let radius = rect.width() * 0.5 - stroke::BOLD;
    let painter = ui.painter();
    painter.circle_filled(center, radius, theme.surface_sunken);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );
    let arc = |from: f32, to: f32, s: egui::Stroke| {
        const STEPS: usize = 16;
        let points: Vec<egui::Pos2> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                center + kit::knob_dir(a) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };
    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let at = START + SWEEP * (*amp / MASTER_GAIN_MAX).clamp(0.0, 1.0);
    if at > START + f32::EPSILON {
        arc(START, at, egui::Stroke::new(stroke::BOLD, theme.accent));
    }
    let dir = kit::knob_dir(at);
    painter.line_segment(
        [center + dir * (radius * 0.35), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );
    // The unity tick, where the detent is — at twelve o'clock, exactly as
    // pan's centre tick is.
    let unity = kit::knob_dir(START + SWEEP * (1.0 / MASTER_GAIN_MAX));
    painter.line_segment(
        [
            center + unity * (radius * 0.9),
            center + unity * (radius + 2.0),
        ],
        egui::Stroke::new(stroke::HAIR, theme.text_muted),
    );

    PanOutcome {
        changed,
        engaged: response.clicked() || response.drag_started() || response.dragged(),
    }
}

/// How pan reads out: "C" at center, "L42" / "R42" either side. Percent,
/// because degrees would imply a precision constant-power panning does not
/// have.
pub(crate) fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        "C".to_owned()
    } else if pan < 0.0 {
        format!("L{amount}")
    } else {
        format!("R{amount}")
    }
}

/// One small square toggle — the M and S of a track header.
///
/// Returns its response. `on_fill` is the colour it takes while
/// engaged; off, it is a hairline outline and nothing else, so a header
/// with nothing engaged is quiet.
pub(crate) fn header_toggle(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    id: egui::Id,
    letter: &str,
    on: bool,
    on_fill: egui::Color32,
) -> egui::Response {
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .affords(Affords::Press);
    let painter = ui.painter();
    let fill = if response.is_pointer_button_down_on() {
        on_fill.gamma_multiply(0.72)
    } else if on {
        on_fill
    } else if response.hovered() {
        theme.surface_raised
    } else {
        theme.surface_sunken
    };
    painter.rect_filled(rect, radius::CTRL, fill);
    painter.rect_stroke(
        rect,
        radius::CTRL,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(HEADER_BTN_TYPE),
        // Engaged, the fill carries the meaning and the letter sits on it
        // in the page colour; idle, the letter is the only thing there.
        if on { theme.bg } else { theme.text_muted },
    );
    response
}

/// The arrangement header's peripheral activity rail.
///
/// This consumes the ballistics the Session mixer already advances from the
/// engine's per-track telemetry. It is paint only: no new callback message,
/// no query, and no invented stereo information.
pub(crate) fn header_meter(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    meter: Option<&mut device::meter::Ballistics>,
    audible: bool,
    id: egui::Id,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .affords(Affords::Press);
    let meter = meter.map(|meter| {
        if response.clicked() {
            meter.clipped = false;
        }
        *meter
    });
    let shown_db = meter.map_or(device::meter::FLOOR_DB, |meter| meter.shown_db);
    let peak_db = meter.map_or(device::meter::FLOOR_DB, |meter| meter.peak_db);
    let clipped = meter.is_some_and(|meter| meter.clipped);

    let clip = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), HEADER_METER_CLIP_H.min(rect.height())),
    );
    let rail = egui::Rect::from_min_max(
        egui::pos2(
            rect.left(),
            (clip.bottom() + stroke::HAIR).min(rect.bottom()),
        ),
        rect.max,
    );
    ui.painter().rect_filled(
        clip,
        0.0,
        // The lamp is the only part of a meter that can be PRESSED, and
        // a clip hold nobody knows is clearable is a clip hold that
        // stays lit all session. Hovered it lifts whether or not it is
        // holding anything, which is what says it is a control.
        match (clipped, response.hovered()) {
            (true, _) => theme.meter_clip,
            (false, true) => theme.surface_raised,
            (false, false) => theme.surface_sunken,
        },
    );
    ui.painter().rect_filled(rail, 0.0, theme.surface_sunken);

    let shown = device::meter::db_to_norm(shown_db);
    let count = ((rail.height() + HEADER_METER_SEG_GAP)
        / (HEADER_METER_SEG_H + HEADER_METER_SEG_GAP))
        .floor()
        .max(1.0) as usize;
    for index in 0..count {
        let from_bottom = index as f32 * (HEADER_METER_SEG_H + HEADER_METER_SEG_GAP);
        let y = rail.bottom() - from_bottom;
        let segment = egui::Rect::from_min_max(
            egui::pos2(
                rail.left() + stroke::HAIR,
                (y - HEADER_METER_SEG_H).max(rail.top()),
            ),
            egui::pos2(rail.right() - stroke::HAIR, y.min(rail.bottom())),
        );
        let threshold = (index + 1) as f32 / count as f32;
        let lit = shown >= threshold;
        let db = device::meter::FLOOR_DB
            + threshold * (device::meter::CEILING_DB - device::meter::FLOOR_DB);
        let color = if !lit {
            theme.divider.gamma_multiply(0.55)
        } else if db > device::meter::HOT_DB {
            theme.meter_hot
        } else {
            theme.meter_low
        };
        ui.painter().rect_filled(
            segment,
            0.0,
            if audible {
                color
            } else {
                color.gamma_multiply(0.45)
            },
        );
    }

    if peak_db > device::meter::FLOOR_DB && rail.height() > 0.0 {
        let y = rail.bottom() - rail.height() * device::meter::db_to_norm(peak_db);
        let color = if peak_db > device::meter::HOT_DB {
            theme.meter_hot
        } else {
            theme.meter_low
        };
        ui.painter().hline(
            rail.x_range(),
            y.clamp(rail.top(), rail.bottom()),
            egui::Stroke::new(stroke::HAIR, color),
        );
    }
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    let value = if shown_db <= device::meter::FLOOR_DB {
        "-inf".to_owned()
    } else {
        format!("{shown_db:.1} dBFS")
    };
    response.on_hover_text(if clipped {
        format!("Track peak {value}\nClipped · click to clear")
    } else {
        format!("Track peak {value}")
    });
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum TrackHeaderMenu {
    Rename,
    Fold,
    MoveUp,
    MoveDown,
    Mute,
    Solo,
    Delete,
}

/// How tall the master's row is, pinned at the foot of the arrangement.
pub(crate) const MASTER_HEAD_H: f32 = 46.0;

/// The exact tiles of the master's header, kept pure and shared by paint,
/// hit-testing and tests for the same reason [`track_header_layout`] is: a
/// control drawn in one place and reached for in another answers nothing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MasterHeaderLayout {
    /// The header itself: the header column's width, no more.
    pub(crate) head: egui::Rect,
    /// The rest of the row, beside it.
    pub(crate) beside: egui::Rect,
    pub(crate) meter: egui::Rect,
    pub(crate) gain: egui::Rect,
    pub(crate) pan: egui::Rect,
    /// Whether the two knobs fit at all. A window squeezed narrow keeps
    /// the name and the meter and drops them, rather than drawing knobs
    /// on top of the name.
    pub(crate) knobs: bool,
}

pub(crate) fn master_header_layout(row: egui::Rect, column_right: f32) -> MasterHeaderLayout {
    let head = egui::Rect::from_min_max(
        row.min,
        egui::pos2(column_right.clamp(row.left(), row.right()), row.bottom()),
    );
    let beside = egui::Rect::from_min_max(egui::pos2(head.right(), row.top()), row.max);
    // The meter keeps the lanes' geometry exactly, so every meter in the
    // window reads as one column rather than as a stack plus an oddity.
    let meter = egui::Rect::from_min_max(
        egui::pos2(
            (head.right() - HEADER_METER_W).max(head.left()),
            head.top() + space::XXS + 1.0,
        ),
        egui::pos2(head.right(), (head.bottom() - space::XXS).max(head.top())),
    );
    let content_right = (meter.left() - HEADER_METER_GAP).max(head.left());
    // Level then pan, right to left, each with room for its value under it.
    let knob_y = head.bottom() - HEADER_PAD * 0.5 - HEADER_KIND_TYPE - HEADER_KNOB;
    let pan = egui::Rect::from_min_size(
        egui::pos2(content_right - HEADER_KNOB, knob_y),
        egui::vec2(HEADER_KNOB, HEADER_KNOB),
    );
    let gain = egui::Rect::from_min_size(
        egui::pos2(pan.left() - HEADER_PAD - HEADER_KNOB, knob_y),
        egui::vec2(HEADER_KNOB, HEADER_KNOB),
    );
    MasterHeaderLayout {
        head,
        beside,
        meter,
        gain,
        pan,
        knobs: gain.left() > head.left() + HEADER_PAD,
    }
}

/// The MASTER row: the strip every lane lands on, pinned below them.
///
/// Pinned rather than stacked, and that is the point — scrolling a long
/// song must never take the mix's last stage off screen. It carries no
/// mute, no solo and no clips: a muted master is a fader at the bottom,
/// and there is nothing above it to solo against.
///
/// Clicking it points the rack at the master chain; clicking a lane header
/// points it back.
pub(crate) fn master_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    arr: &mut Arrangement,
    row: egui::Rect,
    column_right: f32,
    meter: &mut device::meter::Ballistics,
) {
    if row.height() <= 0.0 || row.width() <= 0.0 {
        return;
    }
    let layout = master_header_layout(row, column_right);
    let rect = layout.head;
    let wid = ui.id().with("master_header");
    let selected = arr.master_selected;
    let body = ui
        .interact(rect, wid.with("body"), egui::Sense::click())
        .affords(Affords::Press);
    if body.clicked() {
        arr.select_master();
    }

    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(
        rect,
        0.0,
        if selected {
            theme.surface
        } else {
            theme.surface_raised
        },
    );
    // A rule along the top: the master is not the next lane down, it is
    // what the lanes arrive at.
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(stroke::BOLD, theme.divider),
    );
    painter.text(
        egui::pos2(rect.left() + HEADER_PAD, rect.top() + HEADER_PAD * 0.5),
        egui::Align2::LEFT_TOP,
        MasterTrack::NAME,
        egui::FontId::proportional(HEADER_NAME_TYPE),
        if selected { theme.accent } else { theme.text },
    );
    if !arr.master.chain.is_empty() {
        painter.text(
            egui::pos2(
                rect.left() + HEADER_PAD,
                rect.top() + HEADER_PAD * 0.5 + 13.0,
            ),
            egui::Align2::LEFT_TOP,
            format!("{} fx", arr.master.chain.len()),
            egui::FontId::monospace(HEADER_KIND_TYPE),
            theme.text_muted,
        );
    }

    if layout.knobs {
        let mut volume = arr.master.volume;
        if gain_knob(ui, theme, layout.gain, &mut volume).changed {
            arr.master.volume = volume;
        }
        let mut pan = arr.master.pan;
        if pan_knob(ui, theme, layout.pan, &mut pan).changed {
            arr.master.pan = pan;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let small = egui::FontId::monospace(HEADER_KIND_TYPE - 1.0);
        painter.text(
            egui::pos2(layout.gain.center().x, layout.gain.bottom() + 1.0),
            egui::Align2::CENTER_TOP,
            volume_label(arr.master.volume),
            small.clone(),
            theme.text_muted,
        );
        painter.text(
            egui::pos2(layout.pan.center().x, layout.pan.bottom() + 1.0),
            egui::Align2::CENTER_TOP,
            pan_label(arr.master.pan),
            small,
            theme.text_muted,
        );
    }

    // Always audible: there is no mute and no solo rule up here.
    header_meter(
        ui,
        theme,
        layout.meter,
        Some(meter),
        true,
        wid.with("meter"),
    );

    // The strip beside it: the master's own lane, quiet until it has
    // automation to draw. Painted rather than left transparent so the row
    // reads as one thing across the window.
    if layout.beside.width() > 0.0 {
        let painter = ui.painter().with_clip_rect(layout.beside);
        painter.rect_filled(layout.beside, 0.0, theme.surface_sunken);
        painter.line_segment(
            [layout.beside.left_top(), layout.beside.right_top()],
            egui::Stroke::new(stroke::BOLD, theme.divider),
        );
    }
}

/// The header column down the arrangement's left edge: one header per lane,
/// each with a name, a kind badge, mute, solo, and pan.
///
/// Headers are MOUSE-driven by design. Registering them with `Focus` would
/// put them in the arrow-key graph immediately left of the lane cells, which
/// already claim Left and Right for the grid — the ring could get in but
/// never out. The keyboard reaches all of this through the palette's track
/// verbs instead, which is why those exist.
///
/// `lanes` carries the y geometry (and only that); the x range is the
/// column's.
pub(crate) fn track_headers(
    ui: &mut egui::Ui,
    theme: &Theme,
    arr: &mut Arrangement,
    column: egui::Rect,
    lanes: &[egui::Rect],
    meters: &mut Vec<device::meter::Ballistics>,
) {
    ui.painter().rect_filled(column, 0.0, theme.surface_sunken);
    meters.resize_with(arr.tracks.len(), Default::default);

    // Intents, applied after the loop: the closure body borrows `arr`
    // immutably to read each track, so it cannot also write to it.
    let mut select: Option<usize> = None;
    let mut mute: Option<usize> = None;
    let mut solo: Option<usize> = None;
    let mut rename_open: Option<usize> = None;
    let mut fold: Option<usize> = None;
    let mut pan_edit: Option<(usize, f32)> = None;
    let mut delete: Option<usize> = None;
    let menu: std::cell::Cell<Option<(usize, TrackHeaderMenu)>> = std::cell::Cell::new(None);
    let renaming = arr.track_rename.as_ref().map(|r| r.track);
    // The reorder drag rides out of `arr` for the draw and is handed back
    // at the end, the same way the clip ghost does.
    let mut drag = arr.track_drag.take();
    let mut reorder: Option<(usize, usize)> = None;

    for (i, lane) in lanes.iter().enumerate() {
        if lane.top() > column.bottom() {
            break;
        }
        let Some(track) = arr.tracks.get(i) else {
            break;
        };
        let head = egui::Rect::from_min_max(
            egui::pos2(column.left(), lane.top()),
            egui::pos2(column.right(), lane.bottom()),
        )
        .intersect(column);
        if head.height() <= 0.0 {
            continue;
        }
        let wid = ui.id().with(("header", i));
        // What the schedule will actually wire. A track the solo rule has
        // excluded reads as silent here rather than looking live.
        let audible = track_audible(&arr.tracks, i);

        // The header BODY, created before every control on it so that the
        // name, the two toggles and the pan knob — all made after this —
        // win the pointer where they overlap. Dragging it reorders the
        // stack; clicking anywhere on it selects the lane.
        let body = ui
            .interact(head, wid.with("body"), egui::Sense::click_and_drag())
            .affords(Affords::Carry);
        if body.drag_started() {
            drag = Some(TrackDrag {
                from: i,
                insertion: i,
            });
            select = Some(i);
        }
        if body.clicked() {
            select = Some(i);
        }
        if body.secondary_clicked() {
            // The menu belongs to the track it names, not whichever track
            // happened to be active before the secondary click.
            select = Some(i);
        }
        body.context_menu(|ui| {
            if ui.button("Rename track").clicked() {
                menu.set(Some((i, TrackHeaderMenu::Rename)));
                ui.close();
            }
            if ui
                .add_enabled(i > 0, egui::Button::new("Move up"))
                .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::MoveUp)));
                ui.close();
            }
            if ui
                .add_enabled(i + 1 < arr.tracks.len(), egui::Button::new("Move down"))
                .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::MoveDown)));
                ui.close();
            }
            if track.is_group
                && ui
                    .button(if track.folded {
                        "Unfold group"
                    } else {
                        "Fold group"
                    })
                    .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::Fold)));
                ui.close();
            }
            ui.separator();
            if ui
                .button(if track.mute {
                    "Unmute track"
                } else {
                    "Mute track"
                })
                .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::Mute)));
                ui.close();
            }
            if ui
                .button(if track.solo {
                    "Unsolo track"
                } else {
                    "Solo track"
                })
                .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::Solo)));
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(arr.tracks.len() > 1, egui::Button::new("Delete track"))
                .clicked()
            {
                menu.set(Some((i, TrackHeaderMenu::Delete)));
                ui.close();
            }
        });
        if body.dragged()
            && let Some(d) = &mut drag
            && d.from == i
            && let Some(pos) = body.interact_pointer_pos()
        {
            // Where the release would land, counted in the gaps between
            // lanes: every lane whose middle the pointer has passed is a
            // lane the carried one now sits after.
            d.insertion = lanes.iter().filter(|lane| pos.y > lane.center().y).count();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if body.hovered() && drag.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if body.drag_stopped()
            && let Some(d) = &drag
            && d.from == i
        {
            // An insertion point counts gaps; an index counts lanes. Past
            // its own position the carried lane has already vacated one, so
            // the two differ by exactly that lane.
            let to = d
                .insertion
                .saturating_sub(usize::from(d.insertion > d.from))
                .min(arr.tracks.len().saturating_sub(1));
            reorder = Some((d.from, to));
            drag = None;
        }

        let layout = track_header_layout(head);

        // One machined strip, three surface states. Hover is local and one
        // step quieter than selection; neither borrows the accent fill.
        let selected = arr.selected == Some(i);
        let base_fill = if selected {
            theme.surface
        } else if body.hovered() && drag.is_none() {
            theme.surface_raised.gamma_multiply(0.72)
        } else {
            theme.surface_sunken
        };
        ui.painter().rect_filled(head, 0.0, base_fill);

        // The carried lane reads as lifted: washed, so the eye can follow
        // which header the insertion line belongs to.
        if drag.as_ref().is_some_and(|d| d.from == i) {
            ui.painter()
                .rect_filled(head, 0.0, theme.accent_muted.gamma_multiply(0.35));
        }
        if selected {
            // A selected lane gets a spine in the accent, so which track
            // the palette's verbs will hit is readable at a glance.
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    head.left_top(),
                    egui::pos2(head.left() + stroke::FOCUS, head.bottom()),
                ),
                0.0,
                theme.accent,
            );
        }

        // --- the name row -------------------------------------------------
        if layout.identity.height() > 1.0 {
            if renaming == Some(i) {
                // The edit box replaces the label in place. Escape and
                // Enter are handled by the caller of this frame's rename
                // state, below.
                if let Some(rename) = arr.track_rename.as_mut() {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(layout.name)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let field = child.add(
                        egui::TextEdit::singleline(&mut rename.text)
                            .desired_width(layout.name.width())
                            .font(egui::FontId::proportional(HEADER_NAME_TYPE)),
                    );
                    if !rename.focused {
                        field.request_focus();
                        rename.focused = true;
                    }
                }
            } else {
                let response = ui
                    .interact(layout.name, wid.with("name"), egui::Sense::click())
                    .affords(Affords::Write)
                    .on_hover_text("double-click to rename");
                if response.double_clicked() {
                    rename_open = Some(i);
                } else if response.clicked() {
                    select = Some(i);
                }
                // A RULE UNDER THE NAME WHILE THE POINTER IS ON IT.
                // Renaming is a double-click, which is a gesture nobody
                // discovers by looking — so the name has to say it is a
                // field, and the cheapest way to say that is the one
                // every text field already uses.
                if response.hovered() {
                    ui.painter().line_segment(
                        [
                            egui::pos2(layout.name.left(), layout.name.bottom() - 2.0),
                            egui::pos2(layout.name.right(), layout.name.bottom() - 2.0),
                        ],
                        egui::Stroke::new(stroke::HAIR, theme.text_muted),
                    );
                }
                // Nesting is drawn as INDENTATION here exactly as it is
                // in the session's headers: one fact, said the same way
                // in both places, so a stack read in one view is the
                // stack the other view shows.
                ui.painter().with_clip_rect(layout.name).text(
                    layout.name.left_center()
                        + egui::vec2(f32::from(track.depth) * NEST_INDENT, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &track.name,
                    egui::FontId::proportional(HEADER_NAME_TYPE),
                    match (audible, selected) {
                        (false, _) => theme.divider,
                        (true, true) => theme.text,
                        (true, false) => theme.text_muted,
                    },
                );
            }

            let kind = if track.is_group {
                "GROUP"
            } else {
                match track.kind {
                    TrackKind::Midi => "MIDI",
                    TrackKind::Audio => "AUDIO",
                }
            };
            // A square metadata cell, closer to a panel annotation than a
            // web badge. Its fill step does the grouping; another outline
            // here would put a box inside every box in the strip.
            ui.painter().rect_filled(
                layout.kind,
                0.0,
                if selected {
                    theme.surface_raised
                } else {
                    theme.surface
                },
            );
            let kind_response = ui.interact(layout.kind, wid.with("kind"), egui::Sense::hover());
            ui.painter().text(
                layout.kind.center(),
                egui::Align2::CENTER_CENTER,
                kind,
                egui::FontId::proportional(HEADER_KIND_TYPE),
                if audible {
                    theme.text_muted
                } else {
                    theme.divider
                },
            );
            kind_response.on_hover_text(match track.kind {
                TrackKind::Midi => "MIDI track",
                TrackKind::Audio => "Audio track",
            });
        }

        // --- mute, solo, pan ------------------------------------------------
        // Dropped entirely on a squeezed lane: half a button is worse than
        // no button, and a lane pulled down to a sliver is not the one
        // being worked on.
        if let Some(controls) = layout.controls {
            let mute_response = header_toggle(
                ui,
                theme,
                controls.mute,
                wid.with("mute"),
                "M",
                track.mute,
                theme.warn,
            )
            .on_hover_text("Mute track");
            if mute_response.clicked() {
                mute = Some(i);
                select = Some(i);
            }
            let solo_response = header_toggle(
                ui,
                theme,
                controls.solo,
                wid.with("solo"),
                "S",
                track.solo,
                theme.accent,
            )
            .on_hover_text("Solo track");
            if solo_response.clicked() {
                solo = Some(i);
                select = Some(i);
            }

            let mut pan = track.pan;
            let pan_outcome = pan_knob(ui, theme, controls.pan, &mut pan);
            if pan_outcome.changed {
                pan_edit = Some((i, pan));
            }
            if pan_outcome.engaged {
                select = Some(i);
            }
            ui.painter().with_clip_rect(controls.pan_value).text(
                controls.pan_value.right_center(),
                egui::Align2::RIGHT_CENTER,
                pan_label(pan),
                egui::FontId::monospace(HEADER_KIND_TYPE),
                if audible {
                    theme.text_value
                } else {
                    theme.divider
                },
            );
        }

        header_meter(
            ui,
            theme,
            layout.meter,
            meters.get_mut(i),
            audible,
            wid.with("meter"),
        );
    }

    // The column's right edge, so the headers read as their own strip
    // rather than as the first bar of the grid.
    ui.painter().line_segment(
        [column.right_top(), column.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    // Escape abandons a reorder: the stack never moved, so there is
    // nothing to put back.
    if drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        drag = None;
    }

    // The insertion line: where a release would put the carried lane, drawn
    // in the gap it would open rather than on top of a header.
    if let Some(d) = &drag {
        let y = lanes
            .get(d.insertion)
            .map(egui::Rect::top)
            .or_else(|| lanes.last().map(egui::Rect::bottom))
            .unwrap_or(column.top())
            .clamp(column.top(), column.bottom());
        ui.painter().line_segment(
            [egui::pos2(column.left(), y), egui::pos2(column.right(), y)],
            egui::Stroke::new(2.0, theme.accent),
        );
    }

    arr.track_drag = drag;

    if let Some((i, action)) = menu.get() {
        select = Some(i);
        match action {
            TrackHeaderMenu::Rename => rename_open = Some(i),
            TrackHeaderMenu::Fold => fold = Some(i),
            TrackHeaderMenu::MoveUp if i > 0 => reorder = Some((i, i - 1)),
            TrackHeaderMenu::MoveDown if i + 1 < arr.tracks.len() => {
                reorder = Some((i, i + 1));
            }
            TrackHeaderMenu::Mute => mute = Some(i),
            TrackHeaderMenu::Solo => solo = Some(i),
            TrackHeaderMenu::Delete => delete = Some(i),
            TrackHeaderMenu::MoveUp | TrackHeaderMenu::MoveDown => {}
        }
    }

    if let Some(i) = select {
        arr.select_track(i);
        arr.selected_clip = None;
        arr.selected_clip_ids.clear();
    }
    // Folding is not muting: the lane goes away, the sound does not, so
    // this is the one header intent that never touches the schedule.
    if let Some(i) = fold
        && let Some(t) = arr.tracks.get_mut(i).filter(|lane| lane.is_group)
    {
        t.folded = !t.folded;
    }
    if let Some(i) = mute
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.mute = !t.mute;
    }
    if let Some(i) = solo
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.solo = !t.solo;
    }
    if let Some((i, pan)) = pan_edit
        && let Some(t) = arr.tracks.get_mut(i)
    {
        t.pan = pan;
    }
    if let Some(i) = rename_open
        && let Some(name) = arr.tracks.get(i).map(|t| t.name.clone())
    {
        arr.select_track(i);
        arr.track_rename = Some(TrackRename {
            track: i,
            original: name.clone(),
            text: name,
            focused: false,
        });
    }

    // The reorder LAST: every intent above carries an index from before the
    // move, and `move_track` renumbers the marks itself. Applying it first
    // would let a stale index overwrite what it had just corrected.
    if let Some((from, to)) = reorder
        && arr.move_track(from, to)
        && from < meters.len()
        && to < meters.len()
    {
        let meter = meters.remove(from);
        meters.insert(to, meter);
    }
    if let Some(i) = delete
        && arr.remove_track(i)
        && i < meters.len()
    {
        meters.remove(i);
    }
}

/// Resolve an open track rename: Enter commits, Escape restores what was
/// there, and losing the keyboard commits too (clicking away is not a
/// cancel anywhere else in the app either).
///
/// Pure over UI state and called before anything else reads the keyboard,
/// so a name being typed can never also be a shortcut.
pub(crate) fn track_rename_keys(ctx: &egui::Context, arr: &mut Arrangement) {
    let Some(rename) = arr.track_rename.as_ref() else {
        return;
    };
    if !rename.focused {
        return;
    }
    let (commit, cancel) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    let still_editing = ctx.memory(|m| m.focused()).is_some();
    if !(commit || cancel || !still_editing) {
        return;
    }
    // Take it out first: every path below ends with the rename closed.
    let Some(rename) = arr.track_rename.take() else {
        return;
    };
    let Some(track) = arr.tracks.get_mut(rename.track) else {
        return;
    };
    let text = rename.text.trim();
    // An empty name is not a name. Cancelling and emptying both restore
    // what was there, so a track always has something to call itself.
    track.name = if cancel || text.is_empty() {
        rename.original
    } else {
        text.to_owned()
    };
}
