//! The session view: the clip grid and the mixer under it.
//!
//! Lifted out of `main.rs` whole, the way the browser was. Its geometry
//! was already separated from its drawing — `SessionLayout` is pure
//! rectangles so the hit tests and the paint cannot drift apart — which
//! is what made the region liftable in one piece.
use super::*;

/// Where everything in the session grid sits.
///
/// Pure geometry, separate from the drawing, so the hit tests and the paint
/// cannot drift apart and so the tests can point at a slot without going
/// through a frame.
pub(crate) struct SessionLayout {
    pub(crate) area: egui::Rect,
    pub(crate) column_w: f32,
    pub(crate) scenes: usize,
    /// Top of the first slot row — under the column headers.
    pub(crate) rows_top: f32,
    /// How far the columns are scrolled left, in pixels. Every column rect
    /// is shifted by it, so the hit tests scroll with the paint.
    pub(crate) scroll: f32,
    /// How far the rows are scrolled up, in pixels. Same contract.
    pub(crate) scroll_y: f32,
    /// The mixer section's height, from the seam the user drags.
    pub(crate) mixer_h: f32,
    pub(crate) tracks: usize,
}

impl SessionLayout {
    pub(crate) fn new(
        area: egui::Rect,
        tracks: usize,
        scenes: usize,
        scroll: f32,
        scroll_y: f32,
        mixer_h: f32,
    ) -> Self {
        let lanes_w = (area.width() - SCENE_COL_W - MASTER_COL_W).max(0.0);
        // Columns spread to fill the space they have, but never below a
        // width a clip name can live in: past that they keep their size and
        // the grid scrolls instead.
        let column_w = if tracks == 0 {
            SESSION_COL_MAX
        } else {
            (lanes_w / tracks as f32).clamp(SESSION_COL_MIN, SESSION_COL_MAX)
        };
        // The mixer may take at most half the area: a seam dragged to the
        // ceiling must not leave the grid without a grid.
        let mixer_cap = (area.height() * 0.5).max(*SESSION_MIXER_H_RANGE.start());
        let mut layout = Self {
            area,
            column_w,
            scenes,
            rows_top: area.top() + SESSION_HEAD_H,
            scroll: 0.0,
            scroll_y: 0.0,
            mixer_h: mixer_h.clamp(*SESSION_MIXER_H_RANGE.start(), mixer_cap),
            tracks,
        };
        // Clamped on construction, so no caller can hold a position that
        // points past the end — including after a track or scene is
        // deleted, or the window resized.
        layout.scroll = scroll.clamp(0.0, layout.max_scroll());
        layout.scroll_y = scroll_y.clamp(0.0, layout.max_scroll_y());
        layout
    }

    /// The strip the slot ROWS live in: under the headers, above the
    /// mixer, left of the scene column. What vertical scrolling scrolls,
    /// and what row drawing clips to.
    pub(crate) fn rows_viewport(&self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.area.left(), self.rows_top),
            egui::pos2(self.lanes_right(), self.mixer_top().max(self.rows_top)),
        )
    }

    /// Where the mixer section begins — also where the seam lives.
    pub(crate) fn mixer_top(&self) -> f32 {
        self.area.bottom() - SESSION_BAR_H - self.mixer_h
    }

    /// The draggable seam on the mixer's top edge, spanning the lanes.
    pub(crate) fn mixer_seam(&self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.area.left(), self.mixer_top() - MIXER_SEAM_H * 0.5),
            egui::pos2(self.lanes_right(), self.mixer_top() + MIXER_SEAM_H * 0.5),
        )
    }

    /// How far the rows can scroll: every row — the scenes, the stop row,
    /// and the scene column's stop-all and add-scene rows — reachable
    /// above the mixer. Zero when they already fit.
    pub(crate) fn max_scroll_y(&self) -> f32 {
        let rows = self.scenes + 2;
        let content = rows as f32 * (SLOT_H + SLOT_GAP);
        (content - self.rows_viewport().height()).max(0.0)
    }

    /// The strip the columns live in: everything left of the scene column.
    pub(crate) fn viewport(&self) -> egui::Rect {
        egui::Rect::from_min_max(
            self.area.min,
            egui::pos2(self.lanes_right(), self.area.bottom()),
        )
    }

    pub(crate) fn content_w(&self) -> f32 {
        self.tracks as f32 * self.column_w
    }

    /// How far the grid can scroll before the last column's right edge
    /// meets the scene column. Zero when everything already fits.
    pub(crate) fn max_scroll(&self) -> f32 {
        (self.content_w() - self.viewport().width()).max(0.0)
    }

    pub(crate) fn column(&self, track: usize) -> egui::Rect {
        let left = self.area.left() - self.scroll + track as f32 * self.column_w;
        egui::Rect::from_min_max(
            egui::pos2(left, self.area.top()),
            egui::pos2(left + self.column_w, self.area.bottom()),
        )
    }

    pub(crate) fn head(&self, track: usize) -> egui::Rect {
        let column = self.column(track);
        egui::Rect::from_min_max(
            column.min,
            egui::pos2(column.right(), self.area.top() + SESSION_HEAD_H),
        )
    }

    pub(crate) fn row_y(&self, scene: usize) -> f32 {
        self.rows_top - self.scroll_y + scene as f32 * (SLOT_H + SLOT_GAP)
    }

    pub(crate) fn slot(&self, track: usize, scene: usize) -> egui::Rect {
        let column = self.column(track);
        let y = self.row_y(scene);
        egui::Rect::from_min_max(
            egui::pos2(column.left() + SLOT_GAP, y),
            egui::pos2(column.right() - SLOT_GAP, y + SLOT_H),
        )
    }

    /// The stop button under a column: one row below the last scene.
    pub(crate) fn stop(&self, track: usize) -> egui::Rect {
        let column = self.column(track);
        let y = self.row_y(self.scenes);
        egui::Rect::from_min_max(
            egui::pos2(column.left() + SLOT_GAP, y),
            egui::pos2(column.right() - SLOT_GAP, y + SLOT_H),
        )
    }

    /// The mixer strip, pinned to the bottom of the area rather than to the
    /// grid: the strips stay put as scenes are added.
    pub(crate) fn mixer(&self, track: usize) -> egui::Rect {
        let column = self.column(track);
        egui::Rect::from_min_max(
            egui::pos2(column.left(), self.mixer_top()),
            egui::pos2(column.right(), self.area.bottom() - SESSION_BAR_H),
        )
    }

    /// The scrollbar's track, under the columns. Empty when nothing can
    /// scroll — a bar that cannot move is furniture, not a control.
    pub(crate) fn scrollbar(&self) -> Option<egui::Rect> {
        if self.max_scroll() <= 0.0 {
            return None;
        }
        let viewport = self.viewport();
        Some(egui::Rect::from_min_max(
            egui::pos2(viewport.left(), viewport.bottom() - SESSION_BAR_H),
            viewport.max,
        ))
    }

    /// The thumb inside it: as much of the track as is on screen, placed
    /// where the scroll has got to.
    pub(crate) fn thumb(&self) -> Option<egui::Rect> {
        let bar = self.scrollbar()?;
        let content = self.content_w();
        if content <= 0.0 {
            return None;
        }
        let width = (bar.width() * bar.width() / content)
            .max(24.0)
            .min(bar.width());
        let travel = bar.width() - width;
        let at = if self.max_scroll() > 0.0 {
            travel * (self.scroll / self.max_scroll())
        } else {
            0.0
        };
        Some(egui::Rect::from_min_size(
            egui::pos2(bar.left() + at, bar.top()),
            egui::vec2(width, bar.height()),
        ))
    }

    /// Where the scrolling lanes stop: the master column's left edge.
    pub(crate) fn lanes_right(&self) -> f32 {
        self.master_column().left()
    }

    /// The master's column, pinned beside the scenes. It does not scroll —
    /// the one strip you must always be able to reach is the one every
    /// other strip lands on.
    pub(crate) fn master_column(&self) -> egui::Rect {
        let right = self.area.right() - SCENE_COL_W;
        egui::Rect::from_min_max(
            egui::pos2(
                (right - MASTER_COL_W).max(self.area.left()),
                self.area.top(),
            ),
            egui::pos2(right, self.area.bottom()),
        )
    }

    pub(crate) fn master_head(&self) -> egui::Rect {
        let column = self.master_column();
        egui::Rect::from_min_max(
            column.min,
            egui::pos2(column.right(), self.area.top() + SESSION_HEAD_H),
        )
    }

    pub(crate) fn master_mixer(&self) -> egui::Rect {
        let column = self.master_column();
        egui::Rect::from_min_max(
            egui::pos2(column.left(), self.mixer_top()),
            egui::pos2(column.right(), self.area.bottom() - SESSION_BAR_H),
        )
    }

    pub(crate) fn scene_column(&self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.area.right() - SCENE_COL_W, self.area.top()),
            self.area.max,
        )
    }

    pub(crate) fn scene(&self, scene: usize) -> egui::Rect {
        let column = self.scene_column();
        let y = self.row_y(scene);
        egui::Rect::from_min_max(
            egui::pos2(column.left() + SLOT_GAP, y),
            egui::pos2(column.right() - SLOT_GAP, y + SLOT_H),
        )
    }

    /// The stop-all button, at the foot of the scene column.
    pub(crate) fn stop_all(&self) -> egui::Rect {
        let column = self.scene_column();
        let y = self.row_y(self.scenes);
        egui::Rect::from_min_max(
            egui::pos2(column.left() + SLOT_GAP, y),
            egui::pos2(column.right() - SLOT_GAP, y + SLOT_H),
        )
    }

    /// The "add a scene" button, one row below stop-all.
    pub(crate) fn add_scene(&self) -> egui::Rect {
        let column = self.scene_column();
        let y = self.row_y(self.scenes + 1);
        egui::Rect::from_min_max(
            egui::pos2(column.left() + SLOT_GAP, y),
            egui::pos2(column.right() - SLOT_GAP, y + SLOT_H),
        )
    }
}

/// The fader's travel, in dB. Unity sits where 0 dB falls on it, which is
/// about four fifths of the way up — the same place it sits on a console,
/// because the useful resolution belongs around unity and not at the
/// bottom of a fade.
pub(crate) const FADER_MIN_DB: f32 = -60.0;
pub(crate) const FADER_MAX_DB: f32 = 6.0;

/// Fader position (`0..=1`, bottom to top) to linear amplitude.
///
/// The taper is linear in DECIBELS, which is what makes a fader feel even
/// under the hand: equal distances are equal dB, not equal amplitude. The
/// bottom of the travel is silence outright rather than −60 dB, so a fader
/// pulled all the way down is off and not merely quiet.
pub(crate) fn fader_to_amp(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    if position <= 0.0 {
        return 0.0;
    }
    let db = FADER_MIN_DB + position * (FADER_MAX_DB - FADER_MIN_DB);
    10.0f32.powf(db / 20.0)
}

/// The inverse: linear amplitude back to a position on the travel.
pub(crate) fn amp_to_fader(amp: f32) -> f32 {
    if amp <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * amp.log10();
    ((db - FADER_MIN_DB) / (FADER_MAX_DB - FADER_MIN_DB)).clamp(0.0, 1.0)
}

/// A track's level, written the way a console writes it.
pub(crate) fn volume_label(amp: f32) -> String {
    if amp <= 0.0 {
        return "-inf".to_owned();
    }
    let db = 20.0 * amp.log10();
    if db >= 0.0 {
        format!("+{db:.1}")
    } else {
        format!("{db:.1}")
    }
}

/// Where the channel assembly sits inside a strip: the clip lamp, the
/// fader/meter travel, and the readout row's baseline. One function, so
/// the paint, the hit tests and the tests all measure the same rects.
pub(crate) fn strip_assembly(mixer: egui::Rect) -> (egui::Rect, egui::Rect, f32) {
    let pad = CLIP_LABEL_PAD;
    let controls_bottom = mixer.top() + pad + HEADER_BTN;
    let readout_h = HEADER_KIND_TYPE + 5.0;
    let rows_bottom = mixer.bottom() - pad;
    let center_x = mixer.center().x;
    let lamp = egui::Rect::from_min_size(
        egui::pos2(center_x - ASSEMBLY_W * 0.5, controls_bottom + pad),
        egui::vec2(ASSEMBLY_W, CLIP_LAMP_H),
    );
    let travel = egui::Rect::from_min_max(
        egui::pos2(center_x - ASSEMBLY_W * 0.5, lamp.bottom() + 2.0),
        egui::pos2(center_x + ASSEMBLY_W * 0.5, rows_bottom - readout_h),
    );
    (lamp, travel, rows_bottom)
}

/// The channel assembly: a thick vertical track whose BACKGROUND is the
/// level meter — a ladder of LED segments, unlit ones faintly visible the
/// way a dark console's are — with the fader's wide flat handle riding
/// over it and a latched clip lamp above. One element, because that is
/// how Ableton draws it and it is the right call: the level you set and
/// the level you get share an axis, so the eye compares them for free.
///
/// The meter body and peak-hold line animate on the ballistics from
/// `ui::device::meter`; segments are coloured by the dB zone they sit in
/// (green to −6, amber to the top, red at full scale), and only LIGHT
/// when the level reaches them.
///
/// Drag to move the fader — absolute from the pointer, like every drag
/// here. Double-click returns to unity. Click the lamp to clear a latched
/// clip. Returns the new amplitude if moved, and whether a clip was
/// cleared.
pub(crate) fn channel_fader(
    ui: &mut egui::Ui,
    theme: &Theme,
    lamp: egui::Rect,
    rect: egui::Rect,
    amp: f32,
    ballistics: &device::meter::Ballistics,
) -> (Option<f32>, bool) {
    if rect.width() <= 0.0 || rect.height() <= 12.0 {
        return (None, false);
    }
    let id = ui
        .id()
        .with(("channel", rect.left() as i32, rect.top() as i32));
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .affords(Affords::Slide);
    let handle_h = 9.0f32.min(rect.height());
    let top = rect.top() + handle_h * 0.5;
    let bottom = rect.bottom() - handle_h * 0.5;
    let travel = (bottom - top).max(1.0);

    let mut moved = None;
    if response.double_clicked() {
        moved = Some(1.0);
    } else if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        moved = Some(fader_to_amp((bottom - pos.y) / travel));
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    let painter = ui.painter();
    // The track: a sunken well the segments live in.
    painter.rect_filled(rect, 0.0, theme.surface_sunken.gamma_multiply(0.9));

    // --- the LED ladder ---------------------------------------------------
    // Segments are placed on the FADER's dB scale, not the meter's, so the
    // handle and the level it produces line up: unity on the fader faces
    // the meter segment a 0 dBFS signal would light.
    let seg_h = 3.0;
    let seg_gap = 1.0;
    let inner = rect.shrink2(egui::vec2(3.0, 2.0));
    let count = ((inner.height() + seg_gap) / (seg_h + seg_gap))
        .floor()
        .max(1.0) as usize;
    let level = device::meter::db_to_norm(ballistics.shown_db);
    let span = FADER_MAX_DB - FADER_MIN_DB;
    for i in 0..count {
        let frac = i as f32 / count.max(1) as f32;
        let y1 = inner.bottom() - frac * inner.height();
        let seg = egui::Rect::from_min_max(
            egui::pos2(inner.left(), y1 - seg_h),
            egui::pos2(inner.right(), y1),
        );
        // The segment's own place on the scale decides its colour; whether
        // the level has reached it decides whether it is LIT. Unlit
        // segments stay faintly visible — the powered-down LED look that
        // makes a ladder read as a ladder and not as a bar chart.
        let seg_db = FADER_MIN_DB + frac * span;
        let color = if seg_db >= device::meter::CEILING_DB - 0.5 {
            theme.meter_clip
        } else if seg_db >= device::meter::HOT_DB {
            theme.meter_hot
        } else {
            theme.meter_low
        };
        // The meter maps its floor..ceiling onto the fader's min..0 dB
        // portion of the ladder; the headroom above unity stays dark until
        // something actually clips into it.
        let lit_to =
            device::meter::FLOOR_DB + level * (device::meter::CEILING_DB - device::meter::FLOOR_DB);
        let lit = level > 0.0 && seg_db <= lit_to.min(device::meter::CEILING_DB);
        painter.rect_filled(
            seg,
            0.0,
            if lit {
                color
            } else {
                color.gamma_multiply(0.13)
            },
        );
    }

    // The peak-hold line: the loudest recent moment, held long enough to
    // read, sliding down after — pure ballistics, drawn as a bright tick.
    let peak = device::meter::db_to_norm(ballistics.peak_db);
    if peak > 0.0 {
        let peak_db =
            device::meter::FLOOR_DB + peak * (device::meter::CEILING_DB - device::meter::FLOOR_DB);
        let frac = ((peak_db - FADER_MIN_DB) / span).clamp(0.0, 1.0);
        let y = inner.bottom() - frac * inner.height();
        painter.line_segment(
            [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
            egui::Stroke::new(1.5, theme.text),
        );
    }

    // The unity line, across the whole track: the one landmark a fader
    // cannot be set by eye without.
    let unity_y = bottom - amp_to_fader(1.0) * travel;
    painter.line_segment(
        [
            egui::pos2(rect.left() - 2.0, unity_y),
            egui::pos2(rect.right() + 2.0, unity_y),
        ],
        egui::Stroke::new(1.0, theme.text_muted),
    );

    // --- the handle -------------------------------------------------------
    // Wide and flat, overhanging the track on both sides — Bitwig's shape.
    // Drawn as a plate with a centre groove, so it reads as a thing to
    // grab rather than as another meter segment.
    let shown = moved.unwrap_or(amp);
    let y = bottom - amp_to_fader(shown) * travel;
    let handle = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, y),
        egui::vec2(rect.width() + 6.0, handle_h),
    );
    painter.rect_filled(
        handle,
        2.5,
        if response.dragged() || response.hovered() {
            theme.text
        } else {
            theme.text_muted
        },
    );
    painter.rect_stroke(
        handle,
        2.5,
        egui::Stroke::new(1.0, theme.surface_sunken),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(handle.left() + 3.0, handle.center().y),
            egui::pos2(handle.right() - 3.0, handle.center().y),
        ],
        egui::Stroke::new(1.5, theme.accent),
    );

    // --- the clip lamp ----------------------------------------------------
    // Latched: it stays lit after the overload has passed, because the
    // whole point is to report one you were not watching. Click to clear.
    let lamp_id = id.with("lamp");
    let lamp_response = ui
        .interact(lamp, lamp_id, egui::Sense::click())
        .affords(Affords::Press);
    painter.rect_filled(
        lamp,
        1.5,
        if ballistics.clipped {
            theme.meter_clip
        } else {
            theme.meter_clip.gamma_multiply(0.15)
        },
    );
    if lamp_response.hovered() && ballistics.clipped {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let cleared = lamp_response.clicked() && ballistics.clipped;
    (moved, cleared)
}

/// A launch triangle, pointing at the clip it would start.
pub(crate) fn launch_triangle(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let h = (rect.height() * 0.42).min(7.0);
    let c = rect.center();
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - h * 0.6, c.y - h),
            egui::pos2(c.x - h * 0.6, c.y + h),
            egui::pos2(c.x + h * 0.9, c.y),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// What the session pass hands back to the app: the things only the app
/// can settle — a clip light lives with the meters, and a tempo change
/// must reach the engine's transport.
#[derive(Default)]
pub(crate) struct SessionOutcome {
    pub(crate) cleared_clip: Option<usize>,
    /// A launched scene carried a tempo in its name.
    pub(crate) tempo: Option<f64>,
}

/// What the session grid was asked to do this frame. Collected while
/// drawing and applied after, for the same reason the clip pass does it:
/// the draw holds `arr` borrowed.
#[derive(Clone, Copy)]
pub(crate) enum SessionIntent {
    Launch(usize, usize),
    Select(usize, usize),
    Create(usize, usize),
    Clear(usize, usize),
    StopTrack(usize),
    LaunchScene(usize),
    StopAll,
    AddScene,
    SelectTrack(usize),
    SelectScene(usize),
    RenameTrack(usize),
    ReorderTrack(usize, usize),
    RenameScene(usize),
    InsertSceneBelow(usize),
    CaptureScene,
    RemoveScene(usize),
    MoveSlot((usize, usize), (usize, usize), bool),
}

/// The clip launcher: tracks as columns, scenes as rows, clips as slots.
///
/// The same tracks and the same clip type as the timeline — this is a
/// different way to READ the song, not a different song. A slot's clip has
/// no timeline position: it plays from wherever it is launched, which is
/// what the whole grid means.
// Session owns its grid, mixer, transport launch edge, and external drag.
#[allow(clippy::too_many_arguments)]
pub(crate) fn session_body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    arr: &mut Arrangement,
    beats_per_bar: u32,
    launch_at: f32,
    // One per track, in track order. Short is fine: a track without an
    // entry meters silence.
    meters: &[device::meter::Ballistics],
    drag: Option<&mut DragImport>,
) -> SessionOutcome {
    let area = ui.max_rect();
    claim(ui);
    ui.painter().rect_filled(area, 0.0, theme.bg);
    // The wheel: the vertical axis scrolls the ROWS when there are rows
    // to scroll, and falls back to the columns when there are not — so a
    // plain mouse wheel always does something useful. The horizontal axis
    // (a trackpad swipe, or shift+wheel, which egui folds into it) always
    // drives the columns.
    let mut layout = SessionLayout::new(
        area,
        arr.tracks.len(),
        arr.session.scenes.len(),
        arr.session_scroll,
        arr.session_scroll_y,
        arr.session_mixer_h,
    );
    let wheel = ui.input(|i| i.smooth_scroll_delta);
    if (wheel.x != 0.0 || wheel.y != 0.0)
        && ui.ui_contains_pointer()
        && ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|p| layout.viewport().contains(p))
    {
        let mut pan_x = wheel.x;
        if layout.max_scroll_y() > 0.0 {
            arr.session_scroll_y =
                (arr.session_scroll_y - wheel.y).clamp(0.0, layout.max_scroll_y());
        } else {
            pan_x += wheel.y;
        }
        arr.session_scroll = (arr.session_scroll - pan_x).clamp(0.0, layout.max_scroll());
        layout = SessionLayout::new(
            area,
            arr.tracks.len(),
            arr.session.scenes.len(),
            arr.session_scroll,
            arr.session_scroll_y,
            arr.session_mixer_h,
        );
    }

    // The seam on the mixer's top edge: drag to resize the section. The
    // height is measured from the pointer absolutely, so it tracks the
    // hand rather than accumulating deltas.
    let seam = layout.mixer_seam();
    let seam_id = ui.id().with("mixer_seam");
    let seam_response = ui
        .interact(seam, seam_id, egui::Sense::drag())
        .affords(Affords::SeamY);
    if seam_response.hovered() || seam_response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if seam_response.dragged()
        && let Some(pos) = seam_response.interact_pointer_pos()
    {
        arr.session_mixer_h = area.bottom() - SESSION_BAR_H - pos.y;
        layout = SessionLayout::new(
            area,
            arr.tracks.len(),
            arr.session.scenes.len(),
            arr.session_scroll,
            arr.session_scroll_y,
            arr.session_mixer_h,
        );
    }
    // Whatever survived the clamps is the truth from here on — a stale
    // position (a deleted track or scene, a resized window, a seam pulled
    // past its range) is corrected the frame it is noticed, not one later.
    arr.session_scroll = layout.scroll;
    arr.session_scroll_y = layout.scroll_y;
    arr.session_mixer_h = layout.mixer_h;
    let mut intent: Option<SessionIntent> = None;
    let mut pan_edit: Option<(usize, f32)> = None;
    let mut mute: Option<usize> = None;
    let mut solo: Option<usize> = None;
    let mut volume_edit: Option<(usize, f32)> = None;
    let mut clear_clip: Option<usize> = None;
    let mut slot_drag = arr.slot_drag.take();
    // The header rename and the reorder drag are the SAME state the
    // timeline's header column uses — one rename, one drag, whichever
    // view is showing. Ridden out for the draw and handed back.
    let mut track_rename = arr.track_rename.take();
    let mut track_drag = arr.track_drag.take();
    // A Cell for the same reason as the clip pass's menu: one closure per
    // slot per frame, all reaching one slot.
    let menu: std::cell::Cell<Option<SessionIntent>> = std::cell::Cell::new(None);
    let font = egui::FontId::proportional(HEADER_NAME_TYPE);
    let small = egui::FontId::proportional(HEADER_KIND_TYPE);

    for (t, track) in arr.tracks.iter().enumerate() {
        let column = layout.column(t);
        if column.right() <= area.left() {
            // Scrolled off to the left: nothing to draw, and nothing that
            // should answer a click either.
            continue;
        }
        if column.left() >= layout.lanes_right() {
            // Past the pinned columns: the rest is reachable by scrolling.
            // Drawing it here would draw it UNDER the master and the
            // scene buttons.
            break;
        }
        let head = layout.head(t);
        let selected = arr.selected == Some(t);
        let painter = ui.painter();
        painter.rect_filled(
            head,
            0.0,
            if selected {
                theme.surface
            } else {
                theme.surface_sunken
            },
        );
        let renaming_this = track_rename.as_ref().is_some_and(|r| r.track == t);
        if !renaming_this {
            painter.with_clip_rect(head).text(
                egui::pos2(head.left() + CLIP_LABEL_PAD, head.center().y),
                egui::Align2::LEFT_CENTER,
                &track.name,
                font.clone(),
                if track_audible(&arr.tracks, t) {
                    theme.text
                } else {
                    theme.divider
                },
            );
        }
        let head_id = ui.id().with(("session_head", t));
        let head_response = ui
            .interact(head, head_id, egui::Sense::click_and_drag())
            .affords(Affords::Carry);
        if head_response.double_clicked() {
            intent = Some(SessionIntent::RenameTrack(t));
        } else if head_response.clicked() {
            intent = Some(SessionIntent::SelectTrack(t));
        }
        // The header drags horizontally to reorder the stack — the same
        // gesture as the timeline's header column, turned on its side, and
        // the same rules: an insertion line, nothing moves until release,
        // Escape abandons.
        if head_response.drag_started() {
            track_drag = Some(TrackDrag {
                from: t,
                insertion: t,
            });
            intent = Some(SessionIntent::SelectTrack(t));
        }
        if head_response.dragged()
            && let Some(d) = &mut track_drag
            && d.from == t
            && let Some(pos) = head_response.interact_pointer_pos()
        {
            d.insertion = (0..arr.tracks.len())
                .filter(|&column| pos.x > layout.column(column).center().x)
                .count();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if head_response.drag_stopped()
            && let Some(d) = &track_drag
            && d.from == t
        {
            let to = d
                .insertion
                .saturating_sub(usize::from(d.insertion > d.from))
                .min(arr.tracks.len().saturating_sub(1));
            intent = Some(SessionIntent::ReorderTrack(d.from, to));
            track_drag = None;
        }
        // The rename replaces the name label in place, exactly like the
        // timeline header's. Enter/Escape settlement is global — the app's
        // `track_rename_keys` — so the edit behaves the same in both views.
        if let Some(rename) = track_rename.as_mut().filter(|r| r.track == t) {
            let edit = egui::Rect::from_min_max(
                egui::pos2(head.left() + CLIP_LABEL_PAD, head.top() + 1.0),
                egui::pos2(head.right() - CLIP_LABEL_PAD, head.bottom() - 1.0),
            );
            let field = ui.put(
                edit,
                egui::TextEdit::singleline(&mut rename.text)
                    .font(egui::FontId::proportional(HEADER_NAME_TYPE)),
            );
            if !rename.focused {
                field.request_focus();
                rename.focused = true;
            }
        }

        // --- the slots ----------------------------------------------------
        // Rows scroll, so a row can hang off either edge of the viewport.
        // Its rect is truncated to the visible part — the paint and the
        // hit test both live in the same truncated box, so a half-hidden
        // slot neither draws over the mixer nor answers clicks from
        // under it.
        let rows_view = layout.rows_viewport();
        for scene in 0..arr.session.scenes.len() {
            let full = layout.slot(t, scene);
            if full.top() >= rows_view.bottom() {
                break;
            }
            let rect = full.intersect(rows_view);
            if rect.height() <= 1.0 || rect.width() <= 0.0 {
                continue;
            }
            let clip = arr.session.slot(t, scene);
            let playing = arr.session.is_playing(t, scene);
            let picked = arr.session.selected == Some((t, scene));
            let wid = ui.id().with(("slot", t, scene));
            focus.register(wid, rect);
            let response = ui
                .interact(rect, wid, egui::Sense::click_and_drag())
                .affords(Affords::Carry);
            if response.drag_started() && clip.is_some() {
                // A slot's clip can be pulled to another slot. Ctrl
                // carries a copy, like the timeline's clip drag; nothing
                // moves until the release.
                slot_drag = Some(SlotDrag {
                    from: (t, scene),
                    copy: ui.input(|i| i.modifiers.command),
                    target: None,
                });
            }
            let launch_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(
                    (rect.left() + SLOT_LAUNCH_W).min(rect.right()),
                    rect.bottom(),
                ),
            );
            // A slot at the right edge is half under the scene column; its
            // name must stop where the slot visibly does.
            let text_room = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(
                    rect.right()
                        .min(layout.scene_column().left() - CLIP_LABEL_PAD),
                    rect.bottom(),
                ),
            );

            if let Some(pos) = response.interact_pointer_pos() {
                if response.double_clicked() && clip.is_none() {
                    intent = Some(SessionIntent::Create(t, scene));
                } else if response.clicked() {
                    // The triangle starts it; the body selects it. An
                    // empty slot's triangle is the track's stop button,
                    // which is what Ableton puts there too.
                    intent = Some(if launch_rect.contains(pos) {
                        match clip {
                            Some(_) => SessionIntent::Launch(t, scene),
                            None => SessionIntent::StopTrack(t),
                        }
                    } else {
                        SessionIntent::Select(t, scene)
                    });
                }
            }
            if focus.activated(wid) {
                intent = Some(match clip {
                    Some(_) => SessionIntent::Launch(t, scene),
                    None => SessionIntent::Create(t, scene),
                });
            }
            if clip.is_some() {
                response.context_menu(|ui| {
                    if ui.button("Delete").clicked() {
                        menu.set(Some(SessionIntent::Clear(t, scene)));
                        ui.close();
                    }
                });
            }

            // A cell answers the pointer, filled or empty. Every slot in
            // the grid looks alike, so "which one am I on" cannot be left
            // to the hand's memory of where it moved — and an empty slot
            // has to answer too, because creating a clip happens on
            // ground that is otherwise blank.
            let lit = response.hovered();
            let painter = ui.painter();
            match clip {
                Some(clip) => {
                    painter.rect_filled(
                        rect,
                        0.0,
                        if playing {
                            theme.clip_body
                        } else if lit {
                            theme.clip_body.gamma_multiply(0.9)
                        } else {
                            theme.clip_body.gamma_multiply(0.75)
                        },
                    );
                    launch_triangle(
                        painter,
                        launch_rect,
                        if playing { theme.ok } else { theme.text },
                    );
                    if rect.width() > SLOT_LAUNCH_W + CLIP_LABEL_MIN_W {
                        // Clipped to the slot: a long name is cut off at
                        // the edge rather than spilling across the grid.
                        painter.with_clip_rect(text_room).text(
                            egui::pos2(launch_rect.right(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &clip.name,
                            small.clone(),
                            theme.text,
                        );
                    }
                    if playing {
                        painter.rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke::new(1.5, theme.ok),
                            egui::StrokeKind::Middle,
                        );
                    }
                }
                None => {
                    // An empty slot is a hairline, not a box: the eye
                    // should find the clips, not the gaps.
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(1.0, theme.divider),
                        egui::StrokeKind::Inside,
                    );
                    if response.hovered() {
                        painter.rect_filled(launch_rect, 0.0, theme.surface_raised);
                        // The stop square, drawn only where it can be hit.
                        let stop = launch_rect.shrink(6.0);
                        painter.rect_filled(stop, 0.0, theme.text_muted);
                    }
                }
            }
            if picked {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.5, theme.clip_selected),
                    egui::StrokeKind::Middle,
                );
            }
        }

        // --- the track's stop button --------------------------------------
        let stop = layout.stop(t).intersect(layout.rows_viewport());
        if stop.height() > 1.0 {
            let wid = ui.id().with(("session_stop", t));
            focus.register(wid, stop);
            let response = ui
                .interact(stop, wid, egui::Sense::click())
                .affords(Affords::Press);
            if response.clicked() || focus.activated(wid) {
                intent = Some(SessionIntent::StopTrack(t));
            }
            let painter = ui.painter();
            painter.rect_filled(
                stop,
                0.0,
                if response.hovered() {
                    theme.surface_raised
                } else {
                    theme.surface_sunken
                },
            );
            let square = egui::Rect::from_center_size(
                egui::pos2(stop.left() + SLOT_LAUNCH_W * 0.5, stop.center().y),
                egui::vec2(7.0, 7.0),
            );
            painter.rect_filled(
                square,
                0.0,
                if arr.session.playing.get(t).copied().flatten().is_some() {
                    theme.text
                } else {
                    theme.text_muted
                },
            );
        }

        // --- the mixer strip ----------------------------------------------
        let mixer = layout.mixer(t);
        let painter = ui.painter();
        painter.rect_filled(mixer, 0.0, theme.surface_sunken);
        painter.line_segment(
            [mixer.left_top(), mixer.right_top()],
            egui::Stroke::new(1.0, theme.divider),
        );
        let wid = ui.id().with(("session_mix", t));
        let btn_y = mixer.top() + CLIP_LABEL_PAD;
        let btn = |n: f32| {
            egui::Rect::from_min_size(
                egui::pos2(
                    mixer.left() + CLIP_LABEL_PAD + n * (HEADER_BTN + HEADER_CONTROL_GAP),
                    btn_y,
                ),
                egui::vec2(HEADER_BTN, HEADER_BTN),
            )
        };
        let solo_hit = btn(1.0);
        if header_toggle(
            ui,
            theme,
            btn(0.0),
            wid.with("mute"),
            "M",
            track.mute,
            theme.warn,
        )
        .clicked()
        {
            mute = Some(t);
        }
        if header_toggle(
            ui,
            theme,
            solo_hit,
            wid.with("solo"),
            "S",
            track.solo,
            theme.accent,
        )
        .clicked()
        {
            solo = Some(t);
        }
        let mut pan = track.pan;
        let knob = egui::Rect::from_min_size(
            egui::pos2(
                mixer.right() - CLIP_LABEL_PAD - HEADER_KNOB,
                btn_y + (HEADER_BTN - HEADER_KNOB) * 0.5,
            ),
            egui::vec2(HEADER_KNOB, HEADER_KNOB),
        );
        if knob.left() > solo_hit.right() {
            let pan_outcome = pan_knob(ui, theme, knob, &mut pan);
            if pan_outcome.changed {
                pan_edit = Some((t, pan));
            }
        }

        // --- the channel assembly, centred ---------------------------------
        // The strip's centrepiece rather than furniture at its edge: the
        // fader whose track IS the meter, a clip lamp above, the level in
        // a readout box underneath — the anatomy every console shares.
        let (lamp, travel, rows_bottom) = strip_assembly(mixer);
        if travel.height() > 12.0 && travel.left() > mixer.left() {
            let ballistics = meters.get(t).copied().unwrap_or_default();
            let (moved, cleared) =
                channel_fader(ui, theme, lamp, travel, track.volume, &ballistics);
            if let Some(amp) = moved {
                volume_edit = Some((t, amp));
            }
            if cleared {
                clear_clip = Some(t);
            }

            // The readout box: what the fader is set to, live while it is
            // being dragged — a console's little dB window, not a caption.
            let shown = volume_edit
                .filter(|(edited, _)| *edited == t)
                .map_or(track.volume, |(_, amp)| amp);
            let readout = egui::Rect::from_center_size(
                egui::pos2(
                    mixer.center().x,
                    rows_bottom - (HEADER_KIND_TYPE + 5.0) * 0.5,
                ),
                egui::vec2(
                    (ASSEMBLY_W + 14.0).min(mixer.width() - 4.0),
                    HEADER_KIND_TYPE + 4.0,
                ),
            );
            let painter = ui.painter();
            painter.rect_filled(readout, 0.0, theme.surface_sunken);
            painter.with_clip_rect(readout).text(
                readout.center(),
                egui::Align2::CENTER_CENTER,
                volume_label(shown),
                egui::FontId::monospace(HEADER_KIND_TYPE),
                theme.text_value,
            );
            // The one pixel modulation is allowed outside its strip: a dot
            // saying "something moves this", and nothing else.
            if arr
                .mod_wires
                .iter()
                .any(|wire| wire.track == t && wire.target == TRACK_VOLUME_TARGET)
            {
                painter.circle_filled(
                    egui::pos2(readout.left() - 5.0, readout.center().y),
                    2.0,
                    theme.accent,
                );
            }
            // The pan value rides under the knob it belongs to, not in the
            // corner: label the control, not the strip.
            painter.with_clip_rect(mixer).text(
                egui::pos2(knob.center().x, knob.bottom() + 2.0),
                egui::Align2::CENTER_TOP,
                pan_label(pan),
                egui::FontId::monospace(HEADER_KIND_TYPE - 1.0),
                theme.text_muted,
            );
        }
    }

    // --- the slot drag in flight -------------------------------------------
    if let Some(drag) = &mut slot_drag {
        // Escape abandons it; the grid never moved, so there is nothing
        // to put back.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            slot_drag = None;
        } else {
            let clip = arr.session.slot(drag.from.0, drag.from.1).cloned();
            match (clip, ui.ctx().pointer_latest_pos()) {
                (Some(clip), Some(pos)) => {
                    // The slot under the pointer, if its lane can hold the
                    // clip. Same rules as a drop from outside.
                    drag.target = None;
                    if layout.rows_viewport().contains(pos) {
                        'aim: for track in 0..arr.tracks.len() {
                            if !lane_accepts(&arr.tracks[track], &clip) {
                                continue;
                            }
                            for scene in 0..arr.session.scenes.len() {
                                let rect = layout.slot(track, scene);
                                if rect.contains(pos) {
                                    drag.target = Some((track, scene));
                                    break 'aim;
                                }
                            }
                        }
                    }
                    if let Some((track, scene)) = drag.target
                        && (track, scene) != drag.from
                    {
                        let rect = layout.slot(track, scene).intersect(layout.rows_viewport());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 0.0, theme.accent_muted.gamma_multiply(0.35));
                        painter.rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke::new(1.5, theme.accent),
                            egui::StrokeKind::Middle,
                        );
                    }
                    // The clip's name rides the pointer, marked as a copy
                    // when Ctrl says so.
                    ui.painter().text(
                        pos + egui::vec2(12.0, -10.0),
                        egui::Align2::LEFT_CENTER,
                        if drag.copy {
                            format!("+ {}", clip.name)
                        } else {
                            clip.name.clone()
                        },
                        egui::FontId::proportional(HEADER_KIND_TYPE),
                        theme.text,
                    );
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    if ui.input(|i| i.pointer.any_released()) {
                        if let Some(target) = drag.target.filter(|target| *target != drag.from) {
                            intent = Some(SessionIntent::MoveSlot(drag.from, target, drag.copy));
                        }
                        slot_drag = None;
                    }
                }
                // The clip vanished under the drag (an undo mid-gesture):
                // the drag is about a model that no longer exists.
                _ => slot_drag = None,
            }
        }
    }
    arr.slot_drag = slot_drag;

    // The reorder drag: Escape abandons, and the insertion line stands in
    // the gap the release would open.
    if track_drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        track_drag = None;
    }
    if let Some(d) = &track_drag {
        let x = if d.insertion < arr.tracks.len() {
            layout.column(d.insertion).left()
        } else {
            layout.column(arr.tracks.len().saturating_sub(1)).right()
        }
        .clamp(area.left(), layout.scene_column().left());
        ui.painter().line_segment(
            [egui::pos2(x, area.top()), egui::pos2(x, layout.mixer_top())],
            egui::Stroke::new(2.0, theme.accent),
        );
    }
    arr.track_drag = track_drag;
    arr.track_rename = track_rename;

    // The seam's mark: quiet until pointed at, like the app's other seams.
    if seam_response.hovered() || seam_response.dragged() {
        ui.painter().line_segment(
            [
                egui::pos2(seam.left(), layout.mixer_top()),
                egui::pos2(seam.right(), layout.mixer_top()),
            ],
            egui::Stroke::new(2.0, theme.accent),
        );
    }

    // --- the scene column -------------------------------------------------
    let scene_col = layout.scene_column();
    ui.painter()
        .rect_filled(scene_col, 0.0, theme.surface_sunken);
    ui.painter().line_segment(
        [scene_col.left_top(), scene_col.left_bottom()],
        egui::Stroke::new(1.0, theme.divider),
    );

    // Back to Arrangement, in the column header: lit only while session
    // clips are overriding the timeline, because that is the only time it
    // means anything — Ableton's button, Ableton's rule.
    let session_active = arr.session.playing.iter().any(Option::is_some);
    let header = egui::Rect::from_min_max(
        egui::pos2(scene_col.left() + SLOT_GAP, area.top() + 2.0),
        egui::pos2(
            scene_col.right() - SLOT_GAP,
            area.top() + SESSION_HEAD_H - 2.0,
        ),
    );
    if session_active {
        let wid = ui.id().with("back_to_arrangement");
        focus.register(wid, header);
        let response = ui
            .interact(header, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.clicked() || focus.activated(wid) {
            intent = Some(SessionIntent::StopAll);
        }
        let painter = ui.painter();
        painter.rect_filled(
            header,
            0.0,
            if response.hovered() {
                theme.warn
            } else {
                theme.warn.gamma_multiply(0.75)
            },
        );
        painter.text(
            header.center(),
            egui::Align2::CENTER_CENTER,
            "back to arrangement",
            egui::FontId::proportional(HEADER_KIND_TYPE),
            theme.bg,
        );
    } else {
        ui.painter().text(
            header.center(),
            egui::Align2::CENTER_CENTER,
            "scenes",
            egui::FontId::proportional(HEADER_KIND_TYPE),
            theme.text_muted,
        );
    }
    // The scene rows share the slots' scroll, and clip the same way — to
    // their own column, which runs to the bottom (no mixer under it).
    let scene_view =
        egui::Rect::from_min_max(egui::pos2(scene_col.left(), layout.rows_top), scene_col.max);
    let mut scene_rename = arr.scene_rename.take();
    let mut scene_rename_commit = false;
    let mut scene_rename_cancel = false;
    for (s, scene) in arr.session.scenes.iter().enumerate() {
        let full = layout.scene(s);
        if full.top() >= scene_view.bottom() {
            break;
        }
        let rect = full.intersect(scene_view);
        if rect.height() <= 1.0 {
            continue;
        }
        let selected = arr.session.selected_scene == Some(s);
        let wid = ui.id().with(("scene", s));
        focus.register(wid, rect);
        // The TRIANGLE launches; the name selects (and renames on a
        // double-click) — the same division the slots use, so a scene can
        // be aimed at without firing it.
        let launch_rect = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.left() + SLOT_LAUNCH_W, rect.bottom()),
        );
        let response = ui
            .interact(rect, wid, egui::Sense::click())
            .affords(Affords::Press);
        if let Some(pos) = response.interact_pointer_pos() {
            if response.double_clicked() && !launch_rect.contains(pos) {
                intent = Some(SessionIntent::RenameScene(s));
            } else if response.clicked() {
                intent = Some(if launch_rect.contains(pos) {
                    SessionIntent::LaunchScene(s)
                } else {
                    SessionIntent::SelectScene(s)
                });
            }
        }
        if focus.activated(wid) {
            intent = Some(SessionIntent::LaunchScene(s));
        }
        response.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                menu.set(Some(SessionIntent::RenameScene(s)));
                ui.close();
            }
            if ui.button("Insert scene below").clicked() {
                menu.set(Some(SessionIntent::InsertSceneBelow(s)));
                ui.close();
            }
            if ui.button("Capture and insert scene").clicked() {
                menu.set(Some(SessionIntent::CaptureScene));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                menu.set(Some(SessionIntent::RemoveScene(s)));
                ui.close();
            }
        });

        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, 0.0, theme.accent_muted.gamma_multiply(0.3));
        } else if response.hovered() {
            painter.rect_filled(rect, 0.0, theme.surface_raised);
        }
        launch_triangle(
            painter,
            launch_rect,
            if selected {
                theme.text
            } else {
                theme.text_muted
            },
        );
        if let Some(r) = scene_rename.as_mut().filter(|r| r.scene == s) {
            // The name becomes an edit box in place, like every other
            // rename here. Enter commits, Escape restores, clicking away
            // commits.
            let edit = egui::Rect::from_min_size(
                egui::pos2(rect.left() + SLOT_LAUNCH_W, rect.top() + 1.0),
                egui::vec2(
                    (rect.width() - SLOT_LAUNCH_W - 2.0).max(40.0),
                    rect.height() - 2.0,
                ),
            );
            let field = ui.put(edit, egui::TextEdit::singleline(&mut r.text));
            if !r.focused {
                field.request_focus();
                r.focused = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                scene_rename_cancel = true;
            } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) || field.lost_focus() {
                scene_rename_commit = true;
            }
        } else {
            // A scene carrying a tempo announces it: the number is a
            // control, not a caption, and it reads differently.
            let named_tempo = scene_tempo(&scene.name);
            painter.with_clip_rect(rect).text(
                egui::pos2(rect.left() + SLOT_LAUNCH_W, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &scene.name,
                small.clone(),
                match (selected, named_tempo.is_some()) {
                    (_, true) => theme.accent,
                    (true, false) => theme.text,
                    (false, false) => theme.text_muted,
                },
            );
        }
    }
    // Rename settlement, after the borrow of `scenes` is done.
    if scene_rename_cancel {
        scene_rename = None;
    } else if scene_rename_commit
        && let Some(r) = scene_rename.take()
        && let Some(scene) = arr.session.scenes.get_mut(r.scene)
    {
        let text = r.text.trim();
        scene.name = if text.is_empty() {
            r.original
        } else {
            text.to_owned()
        };
    }
    arr.scene_rename = scene_rename;

    let stop_all = layout.stop_all().intersect(scene_view);
    if stop_all.height() > 1.0 {
        let wid = ui.id().with("session_stop_all");
        focus.register(wid, stop_all);
        let response = ui
            .interact(stop_all, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.clicked() || focus.activated(wid) {
            intent = Some(SessionIntent::StopAll);
        }
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(stop_all, 0.0, theme.surface_raised);
        }
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(stop_all.left() + SLOT_LAUNCH_W * 0.5, stop_all.center().y),
                egui::vec2(7.0, 7.0),
            ),
            1.0,
            theme.text,
        );
        painter.text(
            egui::pos2(stop_all.left() + SLOT_LAUNCH_W, stop_all.center().y),
            egui::Align2::LEFT_CENTER,
            "stop all",
            small.clone(),
            theme.text_muted,
        );
    }

    let add = layout.add_scene().intersect(scene_view);
    if add.height() > 1.0 {
        let wid = ui.id().with("session_add_scene");
        focus.register(wid, add);
        let response = ui
            .interact(add, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.clicked() || focus.activated(wid) {
            intent = Some(SessionIntent::AddScene);
        }
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(add, 0.0, theme.surface_raised);
        }
        painter.text(
            egui::pos2(add.left() + SLOT_LAUNCH_W, add.center().y),
            egui::Align2::LEFT_CENTER,
            "+ scene",
            small,
            theme.text_muted,
        );
    }

    // --- the scrollbar ------------------------------------------------------
    if let (Some(bar), Some(thumb)) = (layout.scrollbar(), layout.thumb()) {
        let id = ui.id().with("session_scrollbar");
        let response = ui
            .interact(bar, id, egui::Sense::click_and_drag())
            .affords(Affords::Sweep);
        if response.is_pointer_button_down_on()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Where in the thumb the press landed, kept for the drag: a
            // press outside it grips the middle, which is what makes a
            // click on the track jump the thumb under the pointer.
            if ui.input(|i| i.pointer.primary_pressed()) {
                let grip = if thumb.contains(pos) {
                    pos.x - thumb.left()
                } else {
                    thumb.width() * 0.5
                };
                ui.data_mut(|d| d.insert_temp(id, grip));
            }
            let grip = ui
                .data(|d| d.get_temp::<f32>(id))
                .unwrap_or(thumb.width() * 0.5);
            let travel = bar.width() - thumb.width();
            if travel > 0.0 {
                let at = (pos.x - grip - bar.left()) / travel;
                arr.session_scroll =
                    (at.clamp(0.0, 1.0) * layout.max_scroll()).clamp(0.0, layout.max_scroll());
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let painter = ui.painter();
        painter.rect_filled(bar, 0.0, theme.surface_sunken);
        // Drawn from THIS frame's scroll, so a drag tracks the pointer
        // instead of trailing it by a frame.
        let moved = SessionLayout::new(
            area,
            arr.tracks.len(),
            arr.session.scenes.len(),
            arr.session_scroll,
            arr.session_scroll_y,
            arr.session_mixer_h,
        );
        if let Some(thumb) = moved.thumb() {
            painter.rect_filled(
                thumb.shrink2(egui::vec2(0.0, 1.5)),
                0.0,
                if response.hovered() || response.is_pointer_button_down_on() {
                    theme.accent
                } else {
                    theme.outline
                },
            );
        }
    }

    // --- a file being dragged in -------------------------------------------
    // The launcher's answer to the timeline's ghost clip: the slot under
    // the pointer lights up, and the drop lands there. A slot takes a whole
    // file, so there is no length or grid to preview — the highlight IS the
    // preview.
    if let Some(drag) = drag {
        drag.spot = drop_slot(ui, theme, &layout, arr, drag);
    }

    // --- apply -------------------------------------------------------------
    if let Some((t, pan)) = pan_edit
        && let Some(track) = arr.tracks.get_mut(t)
    {
        track.pan = pan;
    }
    if let Some(t) = mute
        && let Some(track) = arr.tracks.get_mut(t)
    {
        track.mute = !track.mute;
    }
    if let Some(t) = solo
        && let Some(track) = arr.tracks.get_mut(t)
    {
        track.solo = !track.solo;
    }
    if let Some((t, amp)) = volume_edit
        && let Some(track) = arr.tracks.get_mut(t)
    {
        track.volume = amp;
    }
    let mut outcome = SessionOutcome::default();
    match intent.or(menu.get()) {
        Some(SessionIntent::Launch(t, s)) => {
            // Launching is an instrument, not a form: the swap happens on
            // the press, not after the clip-edit debounce.
            arr.force_recompile |= arr.session.launch_at(t, s, launch_at);
            arr.session.selected = Some((t, s));
            arr.select_track(t);
        }
        Some(SessionIntent::Select(t, s)) => {
            arr.session.selected = Some((t, s));
            arr.select_track(t);
        }
        Some(SessionIntent::Create(t, s)) => {
            arr.create_slot_clip(t, s, beats_per_bar as f32);
            arr.select_track(t);
        }
        Some(SessionIntent::Clear(t, s)) => {
            // Clearing a playing slot stops it, which the engine must hear
            // now rather than at the next debounce.
            arr.force_recompile |= arr.clear_slot(t, s);
        }
        Some(SessionIntent::StopTrack(t)) => {
            arr.force_recompile |= arr.session.stop_track(t);
        }
        Some(SessionIntent::LaunchScene(s)) => {
            arr.force_recompile |= arr.session.launch_scene_at(s, launch_at);
            // A scene named with a tempo IS a tempo change, and launching
            // selects the NEXT scene — Ableton's default — so launching
            // down a song is Enter, Enter, Enter.
            outcome.tempo = scene_tempo(
                arr.session
                    .scenes
                    .get(s)
                    .map(|scene| scene.name.as_str())
                    .unwrap_or(""),
            );
            arr.session.selected_scene =
                Some((s + 1).min(arr.session.scenes.len().saturating_sub(1)));
        }
        Some(SessionIntent::StopAll) => {
            arr.force_recompile |= arr.session.stop_all();
        }
        Some(SessionIntent::AddScene) => arr.add_scene(),
        Some(SessionIntent::SelectTrack(t)) => arr.select_track(t),
        Some(SessionIntent::SelectScene(s)) => arr.session.selected_scene = Some(s),
        Some(SessionIntent::RenameTrack(t)) => {
            if let Some(track) = arr.tracks.get(t) {
                arr.selected = Some(t);
                arr.track_rename = Some(TrackRename {
                    track: t,
                    text: track.name.clone(),
                    original: track.name.clone(),
                    focused: false,
                });
            }
        }
        Some(SessionIntent::ReorderTrack(from, to)) => {
            arr.move_track(from, to);
        }
        Some(SessionIntent::RenameScene(s)) => {
            if let Some(scene) = arr.session.scenes.get(s) {
                arr.session.selected_scene = Some(s);
                arr.scene_rename = Some(SceneRename {
                    scene: s,
                    text: scene.name.clone(),
                    original: scene.name.clone(),
                    focused: false,
                });
            }
        }
        Some(SessionIntent::InsertSceneBelow(s)) => arr.insert_scene(s + 1),
        Some(SessionIntent::CaptureScene) => {
            arr.capture_scene();
        }
        Some(SessionIntent::RemoveScene(s)) => {
            arr.remove_scene(s);
        }
        Some(SessionIntent::MoveSlot(from, to, copy)) => {
            arr.move_slot(from, to, copy);
        }
        None => {}
    }
    outcome.cleared_clip = clear_clip;
    outcome
}

/// Highlight the slot a dragged audio file would drop into, and report it.
///
/// Audio tracks only, and only where the grid actually has a row: the
/// launcher's slots are the one place an audio clip can be put that is not
/// a position on the timeline.
pub(crate) fn drop_slot(
    ui: &egui::Ui,
    theme: &Theme,
    layout: &SessionLayout,
    arr: &Arrangement,
    drag: &DragImport,
) -> Option<DropSpot> {
    let pos = ui.ctx().pointer_latest_pos()?;
    let hint = |text: &str, color: egui::Color32| {
        ui.painter().text(
            pos + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::new(11.0, egui::FontFamily::Proportional),
            color,
        );
    };
    if !drag.accepted {
        hint("WAV files only", theme.warn);
        return None;
    }
    // Only slots actually on screen can be aimed at: the rows scroll, and
    // a rect that extends under the mixer or the headers is not a target.
    if !layout.rows_viewport().contains(pos) {
        return None;
    }
    for track in 0..arr.tracks.len() {
        for scene in 0..arr.session.scenes.len() {
            let rect = layout.slot(track, scene);
            if !rect.contains(pos) {
                continue;
            }
            if arr.tracks[track].kind != TrackKind::Audio {
                hint("audio tracks only", theme.text_muted);
                return None;
            }
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, theme.accent_muted.gamma_multiply(0.35));
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.5, theme.accent),
                egui::StrokeKind::Middle,
            );
            if rect.width() > SLOT_LAUNCH_W + CLIP_LABEL_MIN_W {
                painter.with_clip_rect(rect).text(
                    egui::pos2(rect.left() + SLOT_LAUNCH_W, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &drag.name,
                    egui::FontId::proportional(HEADER_KIND_TYPE),
                    theme.text,
                );
            }
            return Some(DropSpot::Slot { track, scene });
        }
    }
    None
}
