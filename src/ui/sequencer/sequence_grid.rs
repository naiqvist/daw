//! The pattern as bars of steps: one row per bar, one cell per step of
//! the current grid.
//!
//! Time reads left-to-right, then continues on the next row. Vertical
//! position has no pitch meaning: `row * columns + column` is the one
//! address. The FOOTPRINT is the sixteenth grid's — four bars of sixteen
//! — and a finer or coarser resolution subdivides or merges the cells of
//! that same footprint rather than redrawing the pattern at another
//! width: a 1/32 grid is thirty-two narrower cells in the bar's row, not
//! a longer row. So the bar stays where the eye learned it.
//!
//! The window onto the bar is a CAMERA, not a count of cells: a stretch
//! of ticks and a magnification, and every mark lands where its tick
//! falls under it. Zoom scales ticks to pixels; resolution only decides
//! where the cells are cut. So a sixteenth is twice a thirty-second at
//! every zoom, a triplet is two thirds of a sixteenth, a resolution
//! change moves nothing, and a cell the window catches only part of is
//! drawn cut at the edge, not dropped or stretched.
//!
//! What is drawn is a channel, not a table. An empty step is a point;
//! the start of a beat is a plane one rung up; a trig is a mark with its
//! onset and end, its address through the lens, and a face whose
//! lightness is its velocity. Held notes continue as quieter slabs in
//! the cells they cross. A rule is drawn only where a rule is the sign
//! (the cursor's corners); nothing is furniture.

use crate::design::{Polarity, block, circuit, codex::Sign, kit::Weight, motion::pulse_ink};
use crate::pitch::Pitch;
use crate::sequencing::{DEFAULT_PATTERN_TICKS, GRID_COLUMNS, GRID_ROWS, PATTERN_STEP_TICKS};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::sequencer::grammar::{Motion, Utterance, Voice};
use crate::ui::sequencer::grid_resolution::{GridResolution, TICKS_PER_BAR};
use crate::ui::sequencer::lens::LensView;
use crate::ui::sequencer::registers::{Payload, Registers, TrigNote};
use crate::ui::sequencer::sequence::{
    ClipView, EDITOR_SWITCH_WIDTH, Editor, Intent, NoteView, editor_switch,
};
use crate::ui::sequencer::verbs::Verb;
use crate::ui::sequencer::{INK_LEVEL, phase_of, shade, wash};
use crate::ui::tokens::{font, space};
use eframe::egui;

const MAX_CELL_SIDE: f32 = 44.0;
const CELL_GAP: f32 = space::XXS;
const ROW_GAP: f32 = space::MD;
const STATUS_HEIGHT: f32 = 24.0;
const ROW_ADDRESS_WIDTH: f32 = 48.0;
/// The bar count the footprint holds: the pattern's own.
const BARS: usize = GRID_ROWS;
/// The empty-step ladder. The ground is black; a beat's first step is a
/// plane one rung up, a bar's first step a rung above that. Every other
/// step is a point on the ground, not a plane — the lattice shows as
/// rank and file, and a trig is figure against it.
/// Levels above the ground, not colours: see `sequencer::shade`.
const GROUND: u8 = 0;
const BEAT_FILL: u8 = 16;
const BAR_FILL: u8 = 24;
/// Notes leave a hair of ground above and below, but their horizontal
/// edges remain exact time positions.
const NOTE_Y_INSET: f32 = space::XXS;
/// How far a wash lifts what is under it, toward the figure.
const CURSOR_WASH: u8 = 14;
const SELECTION_WASH: u8 = 8;
/// The present moment's own width. A hairline would be lost against a
/// lit cell; wider than this and it stops being a moment.
const PLAYHEAD_W: f32 = 2.0;
const CURSOR_GAP: f32 = 2.0;
const CURSOR_CAP: f32 = 8.0;
/// Narrower than this, a note face has no room for a label.
const LABEL_MIN_W: f32 = 22.0;
/// Narrower than this, a note face keeps its label but drops the corner signs.
const SIGNS_MIN_W: f32 = 34.0;
const EDGE: u8 = 48;
const LABEL_INK: u8 = 145;
const GHOST_INK: u8 = 112;
/// Ink printed on a velocity face. The face is always lighter than this,
/// including at velocity one, so labels and signs keep their contrast.
const FACE_INK: u8 = 8;
const FACE_DETAIL_INK: u8 = 32;
/// The ruler's band, between the header and the first bar.
const RULER_HEIGHT: f32 = 18.0;
/// The container: a recess the grid sits in, with its header one rung
/// up — the same two planes the inspector is made of, and no rules.
const PANEL_PAD: f32 = space::SM;
const PANEL_FILL: u8 = 10;
const HEADER_FILL: u8 = 18;
/// Magnification of the bar: how many times its row is enlarged, with
/// the window sliding to keep the cursor in view. Powers of two, so the
/// bar's tick count divides exactly at every level.
const MAX_ZOOM: usize = 16;
/// Wider than this, a cell has room for a second line of detail.
const DETAIL_MIN_W: f32 = 64.0;
const DEFAULT_PITCH: u8 = 60;
const DEFAULT_VELOCITY: u8 = 100;

/// The quiet powered trace shared by the sequencer's display modules.
/// Active time still earns the full LIVE signal; this is the circuitry that
/// says an idle address is awake and ready.
fn relic_ink(ground: Polarity, strength: f32) -> egui::Color32 {
    crate::design::Alphabet::for_polarity(ground)
        .live_dim
        .color
        .gamma_multiply(strength)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TrigSelection {
    pub(crate) step: usize,
    pub(crate) tick: usize,
    /// The trig's primary note, whole: address, deviation, substrate.
    pub(crate) primary: Option<NoteView>,
    pub(crate) tone_count: usize,
}

pub(crate) struct SequenceGrid {
    cursor_step: usize,
    resolution: GridResolution,
    /// How many times the bar is magnified. One shows the whole bar in
    /// its row; two shows half of it, at twice the width; and so on.
    zoom: usize,
    /// The first tick of the bar each row shows: where the camera
    /// stands. Every row shows the same window, so the bars stay
    /// aligned and a beat reads straight down through them.
    view_tick: usize,
    /// Active clip boundary; the four-bar lattice beyond it stays quiet.
    clip_ticks: usize,
    last_pitch: Pitch,
    /// The last refusal, shown in the status line until the next sentence.
    /// Silence is forbidden: an unsupported verb answers out loud.
    refusal: Option<String>,
    /// Where the cursor's cell was drawn this frame, so a surface over
    /// the grid can point at the thing under the cursor. A fact about
    /// the last draw, never about the model: `None` until drawn.
    cursor_rect: Option<egui::Rect>,
}

impl Default for SequenceGrid {
    fn default() -> Self {
        Self {
            cursor_step: 0,
            resolution: GridResolution::default(),
            zoom: 1,
            view_tick: 0,
            clip_ticks: DEFAULT_PATTERN_TICKS,
            last_pitch: Pitch::from_midi(DEFAULT_PITCH),
            refusal: None,
            cursor_rect: None,
        }
    }
}

impl SequenceGrid {
    /// Read this frame's view chords: resolution (`^1` finer, `^2`
    /// coarser, `^3` triplets) and zoom (`^+` in, `^-` out, `^0` the whole
    /// bar). Then keep the cursor on its tick and the camera around it.
    ///
    /// A resolution change does not move the camera: the same stretch of
    /// the bar stays on screen, cut into more or fewer cells. Zoom is the
    /// one way to look at more or less of the bar, and it magnifies about
    /// the cursor, so the cell under the hand holds its place.
    pub(crate) fn update_view(&mut self, ctx: &egui::Context) {
        let tick = self.cursor_tick();
        self.resolution.update(ctx);
        self.cursor_step = (tick / self.resolution.step_ticks()).min(self.steps() - 1);
        let zoom_in = ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
        });
        let zoom_out =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus));
        let zoom_fit =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num0));
        if zoom_fit {
            self.zoom = 1;
            self.view_tick = 0;
        } else if zoom_in {
            self.rezoom((self.zoom * 2).min(MAX_ZOOM));
        } else if zoom_out {
            self.rezoom((self.zoom / 2).max(1));
        }
        self.follow_cursor();
    }

    /// Steps across one row: one bar at the current resolution.
    fn columns(&self) -> usize {
        self.resolution.steps_per_bar()
    }

    /// Every step the grid addresses at the current resolution.
    fn steps(&self) -> usize {
        self.clip_ticks
            .div_ceil(self.resolution.step_ticks())
            .clamp(1, self.columns() * BARS)
    }

    /// How many ticks of the bar the window holds.
    fn visible_ticks(&self) -> usize {
        TICKS_PER_BAR / self.zoom
    }

    /// The camera for a row `row_width` wide. At ×1 the bar fills the
    /// row exactly as the sixteenth grid lays it — sixteen strides of a
    /// cell and a gap — and each doubling doubles the scale. Resolution
    /// has no say here: the scale is ticks to pixels, and a cell of any
    /// grid is as wide as the time it spans.
    fn camera(&self, row_width: f32) -> Camera {
        Camera {
            view_tick: self.view_tick,
            visible_ticks: self.visible_ticks(),
            px_per_tick: self.zoom as f32 * (row_width + CELL_GAP) / TICKS_PER_BAR as f32,
        }
    }

    /// Magnify about the cursor: the cursor's tick keeps its fraction of
    /// the window, so the cell under the hand stays put and the bar grows
    /// or shrinks around it. Then the window is kept inside the bar.
    fn rezoom(&mut self, zoom: usize) {
        let anchor = self.cursor_tick() % TICKS_PER_BAR;
        let before = self.visible_ticks();
        let offset = anchor.saturating_sub(self.view_tick).min(before);
        self.zoom = zoom;
        let after = self.visible_ticks();
        self.view_tick = anchor.saturating_sub(offset * after / before);
        self.clamp_view();
    }

    /// The window never runs past the end of the bar.
    fn clamp_view(&mut self) {
        self.view_tick = self.view_tick.min(TICKS_PER_BAR - self.visible_ticks());
    }

    /// Slide the camera the least it must to show the cursor's cell, and
    /// never past the bar's ends. Minimal by rule: a window that
    /// recentred would move the ground under a hand that only stepped
    /// one cell sideways. A cell wider than the window shows from its
    /// start.
    fn follow_cursor(&mut self) {
        let visible = self.visible_ticks();
        let start = self.cursor_tick() % TICKS_PER_BAR;
        let end = start + self.resolution.step_ticks();
        if start < self.view_tick {
            self.view_tick = start;
        } else if end > self.view_tick + visible {
            self.view_tick = (end - visible).min(start);
        }
        self.clamp_view();
    }

    #[allow(clippy::too_many_arguments)] // One read-only context per concern.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        available: egui::Rect,
        focused: bool,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        intents: &mut Vec<Intent>,
        ground: Polarity,
        playhead: Option<usize>,
    ) -> Option<Editor> {
        self.clip_ticks = clip
            .map_or(DEFAULT_PATTERN_TICKS, |clip| clip.length_ticks)
            .clamp(PATTERN_STEP_TICKS, DEFAULT_PATTERN_TICKS);
        self.cursor_step = self.cursor_step.min(self.steps() - 1);
        self.cursor_rect = None;
        if focused {
            self.keyboard(ui.ctx(), voice, clip, intents);
        }
        self.follow_cursor();
        // Sentence-in-progress outranks a stale refusal on the status line.
        let overlay = if voice.sentence.is_empty() {
            self.refusal.clone()
        } else {
            Some(voice.sentence.display())
        };
        let phase = phase_of(playhead);

        // The footprint is the sixteenth grid's, whatever the resolution
        // or zoom: a square cell per sixteenth, sixteen to a bar, four
        // bars down. Resolution and zoom decide how that width is cut.
        let sixteenth_gaps = CELL_GAP * (GRID_COLUMNS - 1) as f32;
        let width_limited =
            (available.width() - PANEL_PAD * 2.0 - ROW_ADDRESS_WIDTH - sixteenth_gaps)
                / GRID_COLUMNS as f32;
        let height_limited = (available.height()
            - PANEL_PAD * 2.0
            - STATUS_HEIGHT
            - RULER_HEIGHT
            - ROW_GAP * (BARS - 1) as f32)
            / BARS as f32;
        let cell_side = width_limited
            .min(height_limited)
            .clamp(1.0, MAX_CELL_SIDE)
            .floor();
        let row_width = cell_side * GRID_COLUMNS as f32 + sixteenth_gaps;
        let grid_height = cell_side * BARS as f32 + ROW_GAP * (BARS - 1) as f32;
        let full_width = ROW_ADDRESS_WIDTH + row_width;
        let full_height = STATUS_HEIGHT + RULER_HEIGHT + grid_height;
        let origin = egui::pos2(
            (available.center().x - full_width * 0.5).floor(),
            (available.center().y - full_height * 0.5).floor(),
        );
        let painter = ui.painter_at(available);

        // Two related but non-identical terminal casings. Their fills and
        // outlines give up the same pieces of the layout rectangle.
        let container = egui::Rect::from_min_size(origin, egui::vec2(full_width, full_height))
            .expand(PANEL_PAD);
        let mut casing = Vec::new();
        circuit::relic_frame(
            &mut casing,
            container,
            shade(PANEL_FILL, ground),
            Weight::Heavy,
            relic_ink(ground, 0.62),
        );
        casing.push(egui::Shape::closed_line(
            circuit::relic_points(container.shrink(4.0)),
            egui::Stroke::new(Weight::Hair.px(), shade(EDGE, ground)),
        ));
        for shape in casing {
            painter.add(shape);
        }
        let header = egui::Rect::from_min_max(
            container.min,
            egui::pos2(container.max.x, origin.y + STATUS_HEIGHT),
        );
        let mut header_shapes = Vec::new();
        circuit::relic_frame(
            &mut header_shapes,
            header,
            shade(HEADER_FILL, ground),
            Weight::Hair,
            relic_ink(ground, 0.82),
        );
        for shape in header_shapes {
            painter.add(shape);
        }
        self.draw_status(
            &painter,
            origin,
            full_width,
            clip,
            lens,
            overlay.as_deref(),
            ground,
        );
        let requested_editor = editor_switch(
            ui,
            egui::Rect::from_min_size(origin, egui::vec2(EDITOR_SWITCH_WIDTH, STATUS_HEIGHT)),
            Editor::Grid,
            ground,
        );

        // The camera: which stretch of each bar the rows show, and where
        // every tick of it falls. The grid says where the cells are cut;
        // the camera says where they land; the window's edges cut them.
        let columns = self.columns();
        let step_ticks = self.resolution.step_ticks();
        let camera = self.camera(row_width);
        let shown = camera.columns(step_ticks);
        let grid_origin = origin + egui::vec2(ROW_ADDRESS_WIDTH, STATUS_HEIGHT + RULER_HEIGHT);
        let tick_x = |tick_in_bar: usize| grid_origin.x + camera.x(tick_in_bar);
        let window = egui::Rect::from_min_max(
            egui::pos2(grid_origin.x, origin.y + STATUS_HEIGHT),
            egui::pos2(grid_origin.x + row_width, origin.y + full_height),
        )
        .expand2(egui::vec2(CURSOR_GAP, 0.0));
        let cells = painter.with_clip_rect(window.intersect(painter.clip_rect()));

        self.draw_ruler(
            &cells,
            egui::Rect::from_min_size(
                egui::pos2(grid_origin.x, origin.y + STATUS_HEIGHT),
                egui::vec2(row_width, RULER_HEIGHT),
            ),
            &camera,
            ground,
        );

        for row in 0..BARS {
            let row_top = row_y(grid_origin.y, cell_side, row);
            draw_row_address(
                &painter,
                egui::pos2(origin.x + ROW_ADDRESS_WIDTH - space::SM, row_top),
                row * columns + shown.start() + 1,
                row * TICKS_PER_BAR + shown.start() * step_ticks,
                cell_side,
                ground,
            );
            for column in shown.clone() {
                let step = row * columns + column;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(tick_x(column * step_ticks), row_top),
                    egui::pos2(
                        tick_x((column + 1) * step_ticks) - CELL_GAP,
                        row_top + cell_side,
                    ),
                );
                if step >= self.steps() {
                    // Past the clip's end the lattice is a ghost: a
                    // hairline outline says a cell could stand here, and
                    // the END mark on the first of them says why none does.
                    // Nothing here answers the pointer or the keys.
                    if rect.intersect(window).is_positive() {
                        let mut shapes = Vec::new();
                        shapes.push(egui::Shape::closed_line(
                            circuit::relic_points(rect),
                            egui::Stroke::new(Weight::Hair.px(), relic_ink(ground, 0.34)),
                        ));
                        cells.extend(shapes);
                        if step == self.steps() {
                            draw_clip_end(
                                &cells,
                                rect.left() - CELL_GAP * 0.5,
                                rect.top(),
                                rect.bottom(),
                                ground,
                            );
                        }
                    }
                    continue;
                }
                // A cell cut by the window's edge answers the pointer on
                // the part that shows, and nowhere else.
                let hit = rect.intersect(window);
                if !hit.is_positive() {
                    continue;
                }
                let response = ui
                    .interact(
                        hit,
                        ui.id().with(("sequence-step", step)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press);
                if response.clicked() {
                    self.cursor_step = step;
                    self.toggle(clip, intents);
                }

                let tick = step * step_ticks;
                draw_ground(&cells, rect, tick, ground);
                if response.hovered() && self.cursor_step != step {
                    // The pointer's presence is a tint, not an outline: a
                    // rule would say something, and hovering says nothing.
                    let mut shapes = Vec::new();
                    circuit::relic_frame(
                        &mut shapes,
                        rect,
                        wash(SELECTION_WASH, ground),
                        Weight::Hair,
                        relic_ink(ground, 0.72),
                    );
                    cells.extend(shapes);
                }
                self.draw_cell_events(&cells, rect, clip, lens, step, ground);
                if self.cursor_step == step {
                    draw_cursor(&cells, rect, ground);
                    self.cursor_rect = Some(rect);
                }
            }
            // Past the end the light goes down: everything beyond the
            // clip is veiled toward the ground, so the clip's extent is
            // read as brightness before it is read as a line.
            let before_end = self.steps().saturating_sub(row * columns).min(columns);
            if before_end < columns {
                let x = if before_end == 0 {
                    window.left()
                } else {
                    tick_x(before_end * step_ticks) - CELL_GAP * 0.5
                };
                let veil = egui::Rect::from_min_max(
                    egui::pos2(x.max(window.left()), row_top - CELL_GAP),
                    egui::pos2(window.right(), row_top + cell_side + CELL_GAP),
                );
                if veil.is_positive() {
                    cells.rect_filled(veil, 0.0, clip_veil(ground));
                }
            }
        }

        // The present moment, where the grid wraps it. A pattern is bars
        // stacked as rows, so a playhead is a mark on ONE row rather than
        // a line down the whole editor — the moment is in one bar, and a
        // line through every bar would say it was in all of them.
        if let Some(tick) = playhead {
            let row = tick / TICKS_PER_BAR;
            if row < BARS {
                let x = tick_x(tick % TICKS_PER_BAR);
                let top = row_y(grid_origin.y, cell_side, row);
                let head = egui::Rect::from_min_max(
                    egui::pos2(x, top),
                    egui::pos2(x + PLAYHEAD_W, top + cell_side),
                )
                .intersect(window);
                if head.is_positive() {
                    let alpha = crate::design::Alphabet::for_polarity(ground);
                    let ink = pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
                    let mut shapes = Vec::new();
                    circuit::trace(
                        &mut shapes,
                        &[head.center_top(), head.center_bottom()],
                        Weight::Heavy,
                        ink,
                    );
                    circuit::pad(
                        &mut shapes,
                        head.center_top(),
                        circuit::PAD + 1.0,
                        ink,
                        true,
                    );
                    for shape in shapes {
                        cells.add(shape);
                    }
                }
            }
        }

        requested_editor
    }

    /// The ruler: time across the window, in the bar's own units. A
    /// point at every step, a short bar at every beat, and a label
    /// wherever the next one has room — beats first, then sixteenths,
    /// then finer, as the zoom makes room for them. Every bar's row is
    /// the same window, so one ruler serves all four.
    fn draw_ruler(
        &self,
        painter: &egui::Painter,
        band: egui::Rect,
        camera: &Camera,
        ground: Polarity,
    ) {
        let step_ticks = self.resolution.step_ticks();
        let stride = step_ticks as f32 * camera.px_per_tick;
        let ruler_font = egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace);
        // Derive cadence from the actual face rather than a guessed cell
        // count. `4.4.2` is the widest word this ruler can say.
        let widest = painter.layout_no_wrap(
            "4.4.2".to_owned(),
            ruler_font.clone(),
            shade(LABEL_INK, ground),
        );
        let unit = ruler_unit(step_ticks, stride, widest.size().x + space::MD);
        for column in camera.columns(step_ticks) {
            let tick = column * step_ticks;
            let x = band.min.x + camera.x(tick);
            let beat = tick.is_multiple_of(TICKS_PER_BAR / 4);
            let mut shapes = Vec::new();
            circuit::pad(
                &mut shapes,
                egui::pos2(x, band.bottom() - 3.0),
                if beat {
                    circuit::PAD
                } else {
                    circuit::PAD - 2.0
                },
                if beat {
                    shade(LABEL_INK, ground)
                } else {
                    shade(EDGE, ground)
                },
                beat,
            );
            for shape in shapes {
                painter.add(shape);
            }
            if let Some(label) = ruler_label(tick, unit) {
                // Centred over the column, where the cell beneath keeps
                // its own content, so the word and the cell it names line
                // up. The pad at the column's edge stays the tick.
                painter.text(
                    egui::pos2(x + stride * 0.5, band.center().y - 1.0),
                    egui::Align2::CENTER_TOP,
                    label,
                    ruler_font.clone(),
                    if beat {
                        shade(INK_LEVEL, ground)
                    } else {
                        shade(LABEL_INK, ground)
                    },
                );
            }
        }
    }

    // Geometry does not compress; see `draw_cell_events`.
    #[allow(clippy::too_many_arguments)]
    fn draw_status(
        &self,
        painter: &egui::Painter,
        origin: egui::Pos2,
        width: f32,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        overlay: Option<&str>,
        ground: Polarity,
    ) {
        let rect = egui::Rect::from_min_size(origin, egui::vec2(width, STATUS_HEIGHT));
        // Words set apart by space, the one delimiter that costs no ink.
        // The harmonic context reads here: key sign and active lens,
        // beside the clip — a context with no sign is a trap.
        let words = match clip {
            Some(clip) if width >= 560.0 => format!(
                "{}   {}   {}{}   {}",
                clip.name,
                crate::ui::sequencer::grid_resolution::bars_label(clip.length_ticks),
                self.resolution.label(),
                zoom_sign(self.zoom),
                lens.status
            ),
            Some(clip) => format!(
                "{}   {}{}",
                clip.name,
                self.resolution.label(),
                zoom_sign(self.zoom)
            ),
            None if width >= 560.0 => {
                format!("NO CLIP   {}   {}", self.resolution.label(), lens.status)
            }
            None => "NO CLIP".to_owned(),
        };
        let note_at = rect.left_center() + egui::vec2(EDITOR_SWITCH_WIDTH + space::SM, 0.0);
        let words_at = note_at + egui::vec2(16.0, 0.0);
        let mut shapes = Vec::new();
        circuit::annotation_arrow(
            &mut shapes,
            note_at,
            words_at - egui::vec2(3.0, 0.0),
            shade(EDGE, ground),
        );
        for shape in shapes {
            painter.add(shape);
        }
        painter.text(
            words_at,
            egui::Align2::LEFT_CENTER,
            words,
            egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
            shade(LABEL_INK, ground),
        );
        // The right side is the grammar's mouth: the sentence-in-progress
        // or a refusal takes the spot, and the quiet is left quiet.
        if let Some(overlay) = overlay {
            painter.text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                overlay,
                egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                shade(INK_LEVEL, ground),
            );
        }
    }

    // A painter's arguments are its geometry, and geometry does not
    // compress: every one of these is a distinct fact about where and
    // how, and bundling them into a struct would move the same list one
    // line further from where it is read.
    #[allow(clippy::too_many_arguments)]
    fn draw_cell_events(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        step: usize,
        ground: Polarity,
    ) {
        let span = self.resolution.step_ticks();
        let tick = step * span;
        let primary = clip.and_then(|clip| primary_at(clip, tick, span));
        let tone_count = clip.map_or(0, |clip| tone_count_at(clip, tick, span));

        // A note begun in an earlier cell remains one continuous span of
        // time: the following cells show the portion crossing each one as
        // a dimmer slab. Draw these first so a fresh onset in this cell is
        // the figure in front of anything it overlaps.
        if let Some(clip) = clip {
            for held in clip
                .notes
                .iter()
                .filter(|note| note.enabled && note.start_ticks < tick)
            {
                if let Some(tail) = note_span_rect(rect, tick, span, held) {
                    let ink = tail_ink(held.velocity, ground);
                    let mut shapes = Vec::new();
                    circuit::trace(
                        &mut shapes,
                        &[tail.left_center(), tail.right_center()],
                        Weight::Hair,
                        ink,
                    );
                    circuit::pad(
                        &mut shapes,
                        tail.right_center(),
                        circuit::PAD - 1.0,
                        ink,
                        true,
                    );
                    for shape in shapes {
                        painter.add(shape);
                    }
                }
            }
        }

        let Some(note) = primary else {
            return;
        };
        let Some(face) = note_span_rect(rect, tick, span, note) else {
            return;
        };
        // The note is one sealed component. Its left edge is onset, its
        // right edge is end, and its face carries velocity. The onset pad
        // makes a pushed attack legible even when the face begins inside
        // a coarse cell.
        let face_fill = velocity_ink(note.velocity, ground);
        let mut shapes = Vec::new();
        circuit::relic_frame(
            &mut shapes,
            face,
            face_fill,
            Weight::Hair,
            shade(FACE_DETAIL_INK, ground),
        );
        circuit::pad(
            &mut shapes,
            face.left_center(),
            circuit::PAD,
            shade(FACE_INK, ground),
            true,
        );
        // The lock mark: a trig that holds parameter locks wears a short
        // row of pads along its foot in the live ink, one per lock up to
        // four — the deck bends its sound here, and the row says how
        // much without the figure. Drawn before the width gate below,
        // because a lock is worth a mark at any size the face still is.
        draw_lock_marks(&mut shapes, face, note.locks, ground);
        for shape in shapes {
            painter.add(shape);
        }

        // Marks degrade by dropping, never by overlapping.
        if face.width() < LABEL_MIN_W {
            return;
        }
        // With room, the label rises to make a line for the detail
        // beneath it: velocity, length, and the push if there is one.
        // The facts the inspector states, brought onto the trig once the
        // trig is wide enough to carry them without crowding.
        let detailed = face.width() >= DETAIL_MIN_W && face.height() >= 36.0;
        let label_y = if detailed {
            face.center().y - space::SM
        } else {
            face.center().y
        };
        painter.text(
            egui::pos2(face.center().x, label_y),
            egui::Align2::CENTER_CENTER,
            cell_label(note, lens),
            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
            shade(FACE_INK, ground),
        );

        if matches!(lens.active, crate::ui::sequencer::lens::ActiveLens::Degrees)
            && let Some(degree) = crate::ui::sequencer::lens::degree_of(note, &lens.key)
            && let Ok(degree) = u8::try_from(degree.rem_euclid(7))
        {
            Sign::Degree(degree).painted(
                painter,
                egui::Id::new(("sequencer-degree", step, degree)),
                egui::Rect::from_center_size(
                    face.left_center() + egui::vec2(10.0, 0.0),
                    egui::Vec2::splat(11.0),
                ),
                Weight::Hair,
                shade(FACE_INK, ground),
            );
        }
        if detailed {
            let push = if note.micro_ticks != 0 {
                format!("  {:+}T", note.micro_ticks)
            } else {
                String::new()
            };
            painter.text(
                egui::pos2(face.center().x, face.center().y + space::SM),
                egui::Align2::CENTER_CENTER,
                format!(
                    "{}  {}{push}",
                    note.velocity,
                    crate::ui::sequencer::grid_resolution::length_label(note.length_ticks)
                ),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                shade(FACE_DETAIL_INK, ground),
            );
        }
        if face.width() < SIGNS_MIN_W {
            return;
        }
        // On a slicing track the trig's locked slice is a tag at the
        // crown: the note below it is a pitch, the tag is which cut.
        if let (true, Some(slice)) = (clip.is_some_and(|clip| clip.slicing), note.slice) {
            draw_edge_tag(
                painter,
                egui::Rect::from_center_size(
                    egui::pos2(face.center().x, face.top() + 5.0),
                    egui::vec2(24.0, 10.0),
                ),
                &slice_label(slice),
                ground,
            );
        }
        if tone_count > 1 {
            painter.text(
                face.right_top() + egui::vec2(-space::XS, space::XS),
                egui::Align2::RIGHT_TOP,
                format!("+{}", tone_count - 1),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                shade(FACE_INK, ground),
            );
        }
        // A conditional trig is a rule, not an event: the document says so
        // by marking the condition on the trig (settled ruling: signed
        // conditions, `notes/20260831-command-grammar.md`). The mark is a
        // SHADE — the trig's density of occurrence as the density of a
        // sign — and the exact figure is the inspector's to state.
        if note.enabled && note.probability < 1.0 {
            draw_edge_tag(
                painter,
                egui::Rect::from_min_size(
                    face.right_bottom() - egui::vec2(13.0, 10.0),
                    egui::vec2(13.0, 10.0),
                ),
                &condition_sign(note.probability).to_string(),
                ground,
            );
        }
        let deviation = deviation_sign(note);
        if !deviation.is_empty() {
            draw_edge_tag(
                painter,
                egui::Rect::from_min_size(face.left_top(), egui::vec2(15.0, 10.0)),
                &deviation,
                ground,
            );
        }
        // A pending transform previews as a ghost: the would-be spelling
        // in ghost ink under the standing one. Nothing has changed yet.
        if let Some(clip) = clip
            && let Some(ghost) = clip
                .ghosts
                .iter()
                .filter(|ghost| ghost.start_ticks == tick)
                .min_by(|a, b| a.pitch.stack_order(&b.pitch))
        {
            painter.text(
                rect.left_bottom() + egui::vec2(space::XS, -space::XXS),
                egui::Align2::LEFT_BOTTOM,
                cell_label(ghost, lens),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                shade(GHOST_INK, ground),
            );
        }
    }

    /// Interpret this frame's utterance against the grid's noun: the trig
    /// under the cursor. Verbs the trig does not support are refused out
    /// loud, never silently dropped (`notes/20260831-command-grammar.md`).
    fn keyboard(
        &mut self,
        ctx: &egui::Context,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        let Some(utterance) = voice.sentence.consume(ctx) else {
            return;
        };
        self.refusal = None;
        self.speak(utterance, voice.registers, clip, intents);
    }

    fn speak(
        &mut self,
        utterance: Utterance,
        registers: &mut Registers,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        let count = utterance.count as isize;
        let span = self.resolution.step_ticks();
        let tick = self.cursor_step * span;
        // What is here: every tick a note starts on within the cell. A
        // verb on the cell is spoken once per such tick, so a coarse
        // cell edits everything it holds and a fine one exactly one.
        let here = starts_in(clip, tick, span);
        match (utterance.verb, utterance.motion) {
            // Hold-as-preposition: the same arrows, spoken while holding
            // the trig qualifier, edit the trig instead of travelling.
            (None, Some(motion @ (Motion::Up | Motion::Down))) if utterance.held => {
                if here.is_empty() {
                    self.refusal = Some("HOLD: NOTHING HERE".to_owned());
                } else {
                    for start in &here {
                        intents.push(Intent::AdjustVelocity {
                            tick: *start,
                            delta: count * if motion == Motion::Up { 1 } else { -1 },
                        });
                    }
                }
            }
            (None, Some(_)) if utterance.held => {
                self.refusal = Some("HOLD: UP OR DOWN".to_owned());
            }
            (None, Some(motion)) => self.move_by(count * self.motion_steps(motion)),
            (Some(Verb::Act), _) => self.toggle(clip, intents),
            (Some(Verb::Delete), _) => {
                for start in here
                    .iter()
                    .copied()
                    .chain((here.is_empty()).then_some(tick))
                {
                    intents.push(Intent::Clear { tick: start });
                }
            }
            (Some(Verb::Nudge | Verb::StackNudge), Some(motion)) => {
                let steps = count * self.motion_steps(motion);
                let delta_ticks = steps * span as isize;
                // Moving right, the last note moves first so none lands on
                // a neighbour that has not moved yet; moving left, the
                // first. An empty cell still speaks once, to be refused.
                let mut order = here.clone();
                if delta_ticks > 0 {
                    order.reverse();
                }
                for start in order.into_iter().chain((here.is_empty()).then_some(tick)) {
                    intents.push(Intent::Nudge {
                        tick: start,
                        delta_ticks,
                    });
                }
                self.move_by(steps);
            }
            (
                Some(Verb::Resize | Verb::StackResize),
                Some(motion @ (Motion::Left | Motion::Right)),
            ) => {
                let delta_ticks = count * self.motion_steps(motion) * span as isize;
                for start in here
                    .iter()
                    .copied()
                    .chain((here.is_empty()).then_some(tick))
                {
                    intents.push(Intent::Resize {
                        tick: start,
                        delta_ticks,
                    });
                }
            }
            (Some(Verb::Resize | Verb::StackResize), Some(_)) => {
                self.refusal = Some("RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::ClipResize), Some(motion @ (Motion::Left | Motion::Right))) => {
                // A step of the grid, or a whole bar with Shift held.
                let unit = if utterance.held { TICKS_PER_BAR } else { span };
                intents.push(Intent::ResizeClip {
                    delta_ticks: count
                        * if motion == Motion::Right { 1 } else { -1 }
                        * unit as isize,
                });
            }
            (Some(Verb::ClipResize), Some(_)) => {
                self.refusal = Some("CLIP RESIZE: LEFT OR RIGHT".to_owned());
            }
            (
                Some(Verb::Velocity | Verb::StackVelocity),
                Some(motion @ (Motion::Up | Motion::Down)),
            ) => {
                for start in here
                    .iter()
                    .copied()
                    .chain((here.is_empty()).then_some(tick))
                {
                    intents.push(Intent::AdjustVelocity {
                        tick: start,
                        delta: count * if motion == Motion::Up { 1 } else { -1 },
                    });
                }
            }
            (Some(Verb::Velocity | Verb::StackVelocity), Some(_)) => {
                self.refusal = Some("VELOCITY: UP OR DOWN".to_owned());
            }
            (Some(Verb::Yank | Verb::StackYank), _) => match trig_at(clip, tick, span) {
                Some(notes) => {
                    registers.yank(Payload::Trig(notes));
                    self.refusal = Some("YANKED A TRIG".to_owned());
                }
                None => self.refusal = Some("YANK: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Put | Verb::StackPut), _) => match registers.trig() {
                Ok(notes) => {
                    for start in here
                        .iter()
                        .copied()
                        .chain((here.is_empty()).then_some(tick))
                    {
                        intents.push(Intent::Clear { tick: start });
                    }
                    for note in notes {
                        intents.push(Intent::AddNote {
                            tick,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                        if note.muted {
                            intents.push(Intent::SetNoteMuted {
                                tick,
                                pitch: note.pitch,
                                muted: true,
                            });
                        }
                    }
                }
                Err(refusal) => self.refusal = Some(refusal),
            },
            (Some(Verb::Duplicate | Verb::StackDuplicate), _) => match trig_at(clip, tick, span) {
                Some(notes) => {
                    // Yank-and-put-adjacent in one keystroke: the copy
                    // lands `count` steps ahead and the cursor rides
                    // along, Elektron style. The register is untouched —
                    // duplicate is a shorthand, not a yank.
                    let steps = count.max(1);
                    let target = (self.cursor_step as isize + steps)
                        .rem_euclid(self.steps() as isize)
                        as usize
                        * span;
                    let there = starts_in(clip, target, span);
                    for start in there
                        .iter()
                        .copied()
                        .chain((there.is_empty()).then_some(target))
                    {
                        intents.push(Intent::Clear { tick: start });
                    }
                    for note in notes {
                        intents.push(Intent::AddNote {
                            tick: target,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                        if note.muted {
                            intents.push(Intent::SetNoteMuted {
                                tick: target,
                                pitch: note.pitch,
                                muted: true,
                            });
                        }
                    }
                    self.move_by(steps);
                }
                None => self.refusal = Some("DUPLICATE: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Condition), _) => {
                match clip.and_then(|clip| primary_at(clip, tick, span)) {
                    Some(note) => {
                        // `50 C` says fifty percent; bare C cycles the
                        // canonical ladder. A count of 1 is read as bare —
                        // a one-percent trig is a typo, not an intent.
                        let probability = if utterance.count > 1 {
                            (utterance.count.min(100)) as f32 / 100.0
                        } else {
                            next_probability(note.probability)
                        };
                        intents.push(Intent::SetProbability {
                            tick: note.start_ticks,
                            probability,
                        });
                    }
                    None => self.refusal = Some("CONDITION: NOTHING HERE".to_owned()),
                }
            }
            (Some(verb), _) => {
                self.refusal = Some(format!("{}: NOT HERE", verb.name()));
            }
            (None, None) => {}
        }
    }

    fn move_by(&mut self, amount: isize) {
        self.cursor_step =
            (self.cursor_step as isize + amount).rem_euclid(self.steps() as isize) as usize;
    }

    /// A horizontal step is one cell; a vertical step is one row — one
    /// bar, however many steps the current grid cuts it into.
    fn motion_steps(&self, motion: Motion) -> isize {
        match motion {
            Motion::Left => -1,
            Motion::Right => 1,
            Motion::Up => -(self.columns() as isize),
            Motion::Down => self.columns() as isize,
        }
    }

    /// ACT on the cell: gate the note that is here, or put one here. A
    /// cell holding a note placed on a finer grid gates THAT note's
    /// tick, so the toggle never adds a second note beside it.
    fn toggle(&mut self, clip: Option<ClipView<'_>>, intents: &mut Vec<Intent>) {
        let span = self.resolution.step_ticks();
        let tick = self.cursor_step * span;
        intents.push(Intent::Toggle {
            tick: clip
                .and_then(|clip| primary_at(clip, tick, span))
                .map_or(tick, |note| note.start_ticks),
            default_pitch: self.last_pitch,
            default_length_ticks: span,
            default_velocity: DEFAULT_VELOCITY,
        });
    }

    /// The cursor's address in ticks — the universal selection the
    /// palette long forms act on.
    pub(crate) fn cursor_tick(&self) -> usize {
        self.cursor_step * self.resolution.step_ticks()
    }

    /// The cursor cell as last drawn, if it was on screen.
    pub(crate) fn cursor_rect(&self) -> Option<egui::Rect> {
        self.cursor_rect
    }

    pub(crate) fn selection(&self, clip: Option<ClipView<'_>>) -> TrigSelection {
        let span = self.resolution.step_ticks();
        let tick = self.cursor_step * span;
        TrigSelection {
            step: self.cursor_step,
            tick,
            primary: clip.and_then(|clip| primary_at(clip, tick, span)).copied(),
            tone_count: clip.map_or(0, |clip| tone_count_at(clip, tick, span)),
        }
    }

    /// A typed or played pitch replaces the cell's first note, or puts
    /// one on the cursor's tick when the cell is empty.
    pub(crate) fn enter_pitch(
        &mut self,
        pitch: Pitch,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        self.last_pitch = pitch;
        let span = self.resolution.step_ticks();
        let tick = self.cursor_step * span;
        intents.push(Intent::SetPrimary {
            tick: clip
                .and_then(|clip| primary_at(clip, tick, span))
                .map_or(tick, |note| note.start_ticks),
            pitch: self.last_pitch,
            length_ticks: span,
            velocity: DEFAULT_VELOCITY,
        });
    }
}

/// The camera over the bar: the first tick each row shows, how many
/// ticks the window holds, and the scale that puts a tick on screen.
/// One camera serves every row, so a beat reads straight down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Camera {
    view_tick: usize,
    visible_ticks: usize,
    px_per_tick: f32,
}

impl Camera {
    /// Where a tick of the bar falls, from the window's left edge. Ticks
    /// before the window are negative and land under the row address;
    /// the caller's clip decides what shows.
    fn x(&self, tick_in_bar: usize) -> f32 {
        (tick_in_bar as f32 - self.view_tick as f32) * self.px_per_tick
    }

    /// The cells of a `step_ticks` grid the window catches any part of,
    /// the cut ones at either edge included.
    fn columns(&self, step_ticks: usize) -> std::ops::RangeInclusive<usize> {
        let step = step_ticks.max(1);
        let last = (self.view_tick + self.visible_ticks).saturating_sub(1);
        self.view_tick / step..=last / step
    }
}

/// The trig cell's pitch text: the address through the lens, the
/// musician's bend as a raised tick, the machine's approximation as a
/// leading `≈` — two deviation sign classes, because they mean different
/// things (`notes/20260831-pitch-lens-spec.md` §4).
fn cell_label(note: &NoteView, lens: &LensView) -> String {
    crate::ui::sequencer::lens::address_label(&lens.active, note, &lens.key)
}

/// The word for a slice: its number from one, as the editor and the
/// SLICE row count them, so every surface agrees about which cut is
/// which.
pub(crate) fn slice_label(slice: u8) -> String {
    format!("S{slice:02}")
}

/// Deviation is an edge tag, separate from the pitch address it modifies.
/// Approximation belongs to the machine and bend to the musician, so when
/// both apply the tag carries both signs rather than collapsing them.
fn deviation_sign(note: &NoteView) -> String {
    use crate::design::signs;
    let bend = if note.pitch.offset_cents > 0.0 || note.micro_ticks > 0 {
        signs::BEND_UP
    } else if note.pitch.offset_cents < 0.0 || note.micro_ticks < 0 {
        signs::BEND_DOWN
    } else {
        ""
    };
    format!("{}{bend}", if note.approx { signs::APPROX } else { "" })
}

fn draw_edge_tag(painter: &egui::Painter, rect: egui::Rect, words: &str, ground: Polarity) {
    let mut shapes = Vec::new();
    circuit::octagon(
        &mut shapes,
        rect,
        2.0,
        Some(shade(FACE_DETAIL_INK, ground)),
        Some((Weight::Hair, shade(FACE_INK, ground))),
    );
    for shape in shapes {
        painter.add(shape);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        words,
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        shade(INK_LEVEL, ground),
    );
}

/// The condition as a sign: the trig's density of occurrence drawn as
/// the density of a shade. Three rungs, because that is what the eye can
/// tell apart at a glance in a cell corner; the exact figure is read in
/// the inspector, never off the trig.
/// The lock mark's pads, along a face's foot: one per lock, at most
/// four, in the live ink. Shared by the grid and the roll so a locked
/// trig wears the same mark in both projections.
pub(crate) fn draw_lock_marks(
    out: &mut Vec<egui::Shape>,
    face: egui::Rect,
    locks: u8,
    ground: Polarity,
) {
    if locks == 0 || face.width() < 14.0 || face.height() < 10.0 {
        return;
    }
    let live = crate::design::Alphabet::for_polarity(ground).live.color;
    let y = face.bottom() - 4.0;
    let mut x = face.left() + 7.0;
    for _ in 0..locks.min(4) {
        if x + 2.0 > face.right() - 4.0 {
            break;
        }
        circuit::pad(out, egui::pos2(x, y), 3.0, live, true);
        x += 5.0;
    }
}

/// The veil laid over everything past a clip's end: the ground, mostly
/// opaque, so what is beyond is seen but not read.
pub(crate) fn clip_veil(ground: Polarity) -> egui::Color32 {
    crate::design::Alphabet::for_polarity(ground)
        .ground
        .color
        .gamma_multiply(0.62)
}

/// The clip's end: a heavy rule from `top` to `bottom` at `x`, capped
/// with pads and the word END at its head. The same mark in the grid
/// and the roll, so a clip resized in one is seen to end in the other.
pub(crate) fn draw_clip_end(
    painter: &egui::Painter,
    x: f32,
    top: f32,
    bottom: f32,
    ground: Polarity,
) {
    let ink = shade(INK_LEVEL, ground);
    let mut shapes = Vec::new();
    circuit::trace(
        &mut shapes,
        &[egui::pos2(x, top), egui::pos2(x, bottom)],
        Weight::Heavy,
        ink,
    );
    circuit::pad(&mut shapes, egui::pos2(x, top), circuit::PAD, ink, true);
    circuit::pad(&mut shapes, egui::pos2(x, bottom), circuit::PAD, ink, true);
    painter.extend(shapes);
    painter.text(
        egui::pos2(x + 4.0, top + 2.0),
        egui::Align2::LEFT_TOP,
        "END",
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        ink,
    );
}

pub(crate) fn condition_sign(probability: f32) -> char {
    if probability >= 0.7 {
        '▓'
    } else if probability >= 0.4 {
        '▒'
    } else {
        '░'
    }
}

/// The condition ladder bare C walks: certain, then thinner and thinner,
/// then certain again. A closed set of signs, not a continuous dial —
/// the exact percentages come from `count C`.
pub(crate) fn next_probability(current: f32) -> f32 {
    const LADDER: [f32; 5] = [1.0, 0.75, 0.5, 0.25, 0.1];
    let position = LADDER
        .iter()
        .position(|p| (p - current).abs() < 0.05)
        .unwrap_or(LADDER.len() - 1);
    LADDER[(position + 1) % LADDER.len()]
}

/// The notes that START within a cell: at `tick` or later, before
/// `tick + span`. A cell is a span of time, and at a coarse grid it may
/// hold notes placed on a finer one; they are the cell's, drawn with
/// their offset, and every verb spoken on the cell finds them.
fn notes_in<'a>(
    clip: ClipView<'a>,
    tick: usize,
    span: usize,
) -> impl Iterator<Item = &'a NoteView> + 'a {
    clip.notes
        .iter()
        .filter(move |note| note.start_ticks >= tick && note.start_ticks < tick + span)
}

/// Every distinct tick a note starts on within the cell, in time order.
/// Verbs that act on "what is here" act on each of these exactly.
fn starts_in(clip: Option<ClipView<'_>>, tick: usize, span: usize) -> Vec<usize> {
    let Some(clip) = clip else {
        return Vec::new();
    };
    let mut starts: Vec<usize> = notes_in(clip, tick, span)
        .map(|note| note.start_ticks)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Every note in the cell, as register payload — or `None` when the cell
/// is empty, so yank and duplicate can refuse honestly. Offsets within
/// the cell do not travel: a put lands the trig on the cursor's tick.
pub(crate) fn trig_at(
    clip: Option<ClipView<'_>>,
    tick: usize,
    span: usize,
) -> Option<Vec<TrigNote>> {
    let notes: Vec<TrigNote> = notes_in(clip?, tick, span)
        .map(|note| TrigNote {
            pitch: note.pitch,
            length_ticks: note.length_ticks,
            velocity: note.velocity,
            probability: note.probability,
            enabled: note.enabled,
            muted: note.muted,
        })
        .collect();
    (!notes.is_empty()).then_some(notes)
}

/// The cell's first note: earliest, then lowest.
fn primary_at(clip: ClipView<'_>, tick: usize, span: usize) -> Option<&NoteView> {
    notes_in(clip, tick, span).min_by(|a, b| {
        a.start_ticks
            .cmp(&b.start_ticks)
            .then_with(|| a.pitch.stack_order(&b.pitch))
    })
}

fn tone_count_at(clip: ClipView<'_>, tick: usize, span: usize) -> usize {
    notes_in(clip, tick, span).count()
}

fn row_y(grid_top: f32, cell_side: f32, row: usize) -> f32 {
    grid_top + row as f32 * (cell_side + ROW_GAP)
}

/// The row's address: the number of its first step in the current
/// grid, and beneath it the bar it is. Two lines on the cell's top and
/// bottom edges, so they align with the cells they name.
fn draw_row_address(
    painter: &egui::Painter,
    right_top: egui::Pos2,
    first_step: usize,
    tick: usize,
    cell_side: f32,
    ground: Polarity,
) {
    let row = (tick / TICKS_PER_BAR) % 16;
    let node = right_top + egui::vec2(-31.0, cell_side.min(28.0) * 0.5);
    let mut shapes = Vec::new();
    circuit::relic_node(
        &mut shapes,
        node,
        cell_side.min(22.0) * 0.42,
        relic_ink(ground, 0.72),
        shade(GROUND, ground),
    );
    for shape in shapes {
        painter.add(shape);
    }
    Sign::Register(row as u8).painted(
        painter,
        egui::Id::new(("sequencer-row-register", row)),
        egui::Rect::from_center_size(node, egui::Vec2::splat(cell_side.min(16.0))),
        Weight::Hair,
        shade(LABEL_INK, ground),
    );
    block::paint(
        painter,
        egui::Id::new(("sequencer-row-address", row, first_step)),
        right_top,
        egui::Align2::RIGHT_TOP,
        block::unit::MICRO,
        &format!("{first_step:02}"),
        shade(INK_LEVEL, ground),
    );
    if cell_side >= 32.0 {
        painter.text(
            egui::pos2(right_top.x, right_top.y + cell_side),
            egui::Align2::RIGHT_BOTTOM,
            musical_position(tick),
            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
            shade(LABEL_INK, ground),
        );
    }
}

/// The ground under a step: one dark cut-glass address. Beat and bar starts
/// wake progressively larger cores, so timing hierarchy is still read before
/// the labels while every step belongs to the same ancient display machine.
fn draw_ground(painter: &egui::Painter, rect: egui::Rect, tick: usize, ground: Polarity) {
    let mut shapes = Vec::new();
    let bar = tick.is_multiple_of(TICKS_PER_BAR);
    let beat = tick.is_multiple_of(TICKS_PER_BAR / 4);
    let power = relic_ink(
        ground,
        if bar {
            0.88
        } else if beat {
            0.70
        } else {
            0.52
        },
    );
    circuit::relic_frame(
        &mut shapes,
        rect,
        beat_fill(tick, ground),
        if bar { Weight::Bold } else { Weight::Hair },
        power,
    );
    if bar || beat {
        circuit::relic_node(
            &mut shapes,
            rect.center(),
            if bar { 6.0 } else { 4.5 },
            power,
            if bar { power } else { shade(BEAT_FILL, ground) },
        );
    } else {
        circuit::via(&mut shapes, rect.center(), power, shade(GROUND, ground));
    }
    for shape in shapes {
        painter.add(shape);
    }
}

/// The zoom as a sign on the status line: silent at one, `×2` and up
/// beyond it. A magnification the eye cannot see stated is a trap.
fn zoom_sign(zoom: usize) -> String {
    if zoom > 1 {
        format!(" ×{zoom}")
    } else {
        String::new()
    }
}

/// The finest time unit the ruler may label at this cell stride: the
/// coarsest of beat, sixteenth, thirty-second and sixty-fourth whose
/// span on screen leaves room for a label. `None` when not even a beat
/// has room.
fn ruler_unit(step_ticks: usize, stride: f32, label_width: f32) -> Option<usize> {
    [TICKS_PER_BAR / 4, 12, 6, 3]
        .into_iter()
        // Only units the grid actually has steps at: a label on a tick no
        // cell begins at would name a place the cursor cannot stand.
        .filter(|unit| unit.is_multiple_of(step_ticks.max(1)))
        .take_while(|unit| (*unit as f32 / step_ticks.max(1) as f32) * stride >= label_width)
        .last()
}

/// The ruler's word for a tick in the bar, at the labelled unit: the
/// beat alone (`2`), the beat and sixteenth (`2.3`), or those and the
/// sub-sixteenth (`2.3.2`). Ticks off the unit say nothing.
fn ruler_label(tick_in_bar: usize, unit: Option<usize>) -> Option<String> {
    let unit = unit?;
    let tick = tick_in_bar % TICKS_PER_BAR;
    if !tick.is_multiple_of(unit) {
        return None;
    }
    let beat_ticks = TICKS_PER_BAR / 4;
    let beat = tick / beat_ticks + 1;
    let within_beat = tick % beat_ticks;
    let sixteenth = within_beat / 12 + 1;
    let within_sixteenth = within_beat % 12;
    Some(if within_beat == 0 {
        format!("{beat}")
    } else if within_sixteenth == 0 {
        format!("{beat}.{sixteenth}")
    } else {
        format!("{beat}.{sixteenth}.{}", within_sixteenth / unit + 1)
    })
}

fn musical_position(tick: usize) -> String {
    let bar = tick / TICKS_PER_BAR + 1;
    let beat = tick % TICKS_PER_BAR / (TICKS_PER_BAR / 4) + 1;
    format!("{bar}.{beat}")
}

/// The part of `note` crossing this cell. Horizontal geometry is an exact
/// time projection: the left and right edges mean onset and end. Only the
/// vertical inset is decorative.
fn note_span_rect(
    cell: egui::Rect,
    cell_start: usize,
    step_ticks: usize,
    note: &NoteView,
) -> Option<egui::Rect> {
    let step = step_ticks.max(1);
    let cell_end = cell_start.saturating_add(step);
    let note_end = note.start_ticks.saturating_add(note.length_ticks);
    let overlap_start = note.start_ticks.max(cell_start);
    let overlap_end = note_end.min(cell_end);
    if overlap_end <= overlap_start {
        return None;
    }
    let x = |tick: usize| {
        cell.left()
            + (tick.saturating_sub(cell_start) as f32 / step as f32).clamp(0.0, 1.0) * cell.width()
    };
    let inset = NOTE_Y_INSET.min(cell.height() * 0.2);
    Some(egui::Rect::from_min_max(
        egui::pos2(x(overlap_start), cell.top() + inset),
        egui::pos2(x(overlap_end), cell.bottom() - inset),
    ))
}

/// The empty-step ladder by position in the bar. Ground for an ordinary
/// step: the grid draws a point there rather than a plane.
pub(crate) fn beat_fill(tick: usize, ground: Polarity) -> egui::Color32 {
    let beat_ticks = TICKS_PER_BAR / 4;
    let level = if tick.is_multiple_of(TICKS_PER_BAR) {
        BAR_FILL
    } else if tick.is_multiple_of(beat_ticks) {
        BEAT_FILL
    } else {
        GROUND
    };
    shade(level, ground)
}

/// The note face carries VELOCITY: a whisper draws a quiet face, an accent
/// a white one. Value is the axis (charter), and the whole kit's dynamics
/// read at a glance without opening a trig.
pub(crate) fn velocity_ink(velocity: u8, ground: Polarity) -> egui::Color32 {
    // 1..=127 maps into gray(112..=255): the floor keeps even the softest
    // trig clearly present, while a full accent earns the loudest value.
    let level = 112.0 + (f32::from(velocity.clamp(1, 127)) / 127.0) * 143.0;
    shade(level as u8, ground)
}

/// A held note keeps its velocity ordering, on a lower range than every
/// onset face. Even a maximum-velocity tail is therefore dimmer than the
/// softest new attack beside it.
fn tail_ink(velocity: u8, ground: Polarity) -> egui::Color32 {
    let level = 40.0 + (f32::from(velocity.clamp(1, 127)) / 127.0) * 56.0;
    shade(level as u8, ground)
}

pub(crate) fn note_name(pitch: u8) -> String {
    let octave = i16::from(pitch / 12) - 1;
    format!("{}{octave}", crate::theory::pitch_class_name(pitch))
}

pub(crate) fn draw_cursor(painter: &egui::Painter, cell: egui::Rect, ground: Polarity) {
    let rect = cell.expand(CURSOR_GAP);
    // The one place a rule is the sign: four corners, and nothing joins
    // them, so the cursor brackets a cell without boxing it.
    let mut shapes = Vec::new();
    circuit::relic_frame(
        &mut shapes,
        cell,
        wash(CURSOR_WASH, ground),
        Weight::Bold,
        relic_ink(ground, 0.92),
    );
    circuit::brackets(
        &mut shapes,
        rect,
        CURSOR_CAP,
        Weight::Bold,
        shade(INK_LEVEL, ground),
    );
    for shape in shapes {
        painter.add(shape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::PATTERN_STEPS;

    #[test]
    fn horizontal_cursor_wraps_through_rows_and_pattern_end() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 15;
        grid.move_by(1);
        assert_eq!(grid.cursor_step, 16);
        grid.cursor_step = 63;
        grid.move_by(1);
        assert_eq!(grid.cursor_step, 0);
        grid.move_by(-1);
        assert_eq!(grid.cursor_step, 63);
    }

    #[test]
    fn vertical_cursor_moves_sixteen_chronological_steps() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 5;
        grid.move_by(GRID_COLUMNS as isize);
        assert_eq!(grid.cursor_step, 21);
        grid.move_by(-(GRID_COLUMNS as isize));
        assert_eq!(grid.cursor_step, 5);
    }

    /// A finer grid cuts the same bars into more steps: the row grows
    /// in steps, not the pattern in rows, and the cursor stays on its tick.
    #[test]
    fn a_finer_grid_subdivides_the_bar_and_keeps_the_cursor_on_its_tick() {
        let mut grid = SequenceGrid::default();
        assert_eq!(grid.columns(), 16);
        assert_eq!(grid.steps(), 64);
        grid.cursor_step = 5;
        let tick = grid.cursor_tick();
        grid.resolution = GridResolution::at(32, false);
        grid.cursor_step = tick / grid.resolution.step_ticks();
        assert_eq!(grid.columns(), 32);
        assert_eq!(grid.steps(), 128, "the pattern gained or lost bars");
        assert_eq!(grid.cursor_tick(), tick, "the cursor left its moment");
        assert_eq!(grid.cursor_step, 10);
        // Down is still one bar.
        grid.move_by(grid.motion_steps(Motion::Down));
        assert_eq!(grid.cursor_tick(), tick + TICKS_PER_BAR);
        // And the end still wraps to the start.
        grid.cursor_step = 127;
        grid.move_by(1);
        assert_eq!(grid.cursor_step, 0);
    }

    /// The window is a camera over ticks, not a count of cells: the
    /// scale depends on the zoom alone, so a sixteenth is twice a
    /// thirty-second and a triplet two thirds of a sixteenth at every
    /// magnification; a resolution change leaves the camera exactly
    /// where it stood; and a cell the window half-catches is drawn cut,
    /// not dropped or stretched to fill the row.
    #[test]
    fn the_camera_scales_ticks_not_cells() {
        let mut grid = SequenceGrid::default();
        let row_width = 16.0 * 44.0 + 15.0 * CELL_GAP;
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;

        // ×1: sixteen sixteenths fill the row, each a cell and a gap.
        let at_one = grid.camera(row_width);
        assert!(close(at_one.x(12) - at_one.x(0), 44.0 + CELL_GAP));
        assert!(close(at_one.x(TICKS_PER_BAR), row_width + CELL_GAP));
        assert_eq!(at_one.columns(12), 0..=15);

        // ×8 is exactly eight times the scale, whatever the grid.
        grid.zoom = 8;
        let at_eight = grid.camera(row_width);
        assert!(close(at_eight.px_per_tick, at_one.px_per_tick * 8.0));
        grid.resolution = GridResolution::at(16, true);
        assert_eq!(
            grid.camera(row_width),
            at_eight,
            "the resolution moved the camera"
        );
        // Three 1/16T cells of 8 ticks fill the 24-tick window: each is
        // two thirds of a sixteenth's width, never a third of the row.
        assert_eq!(at_eight.columns(8), 0..=2);
        let sixteenth = 12.0 * at_eight.px_per_tick;
        assert!(close(at_eight.x(8) - at_eight.x(0), sixteenth * 2.0 / 3.0));

        // ×16 shows twelve ticks: a triplet cell and half of the next,
        // the cut one kept and the whole one two thirds of the row.
        grid.zoom = 16;
        let at_sixteen = grid.camera(row_width);
        assert_eq!(at_sixteen.visible_ticks, 12);
        assert_eq!(
            at_sixteen.columns(8),
            0..=1,
            "the cell the window half-catches was dropped"
        );
        assert!(close(
            at_sixteen.x(8) - at_sixteen.x(0),
            (row_width + CELL_GAP) * 2.0 / 3.0
        ));
        // A window standing mid-cell starts on that cell, cut.
        grid.view_tick = 4;
        assert_eq!(grid.camera(row_width).columns(8), 0..=1);
        assert!(grid.camera(row_width).x(0) < 0.0);
    }

    /// The camera slides the least it must to keep the cursor's cell in
    /// view, magnifies about the cursor, never leaves the bar, and does
    /// not move for a resolution change.
    #[test]
    fn the_camera_follows_the_cursor_and_zooms_about_it() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 20; // bar 2, column 4: tick 48 of the bar
        grid.follow_cursor();
        assert_eq!(
            (grid.zoom, grid.visible_ticks(), grid.view_tick),
            (1, 192, 0)
        );

        // Zooming in keeps the cursor's quarter of the window: 48 of 192
        // becomes 24 of 96, so the cell under the hand does not move.
        grid.rezoom(2);
        assert_eq!((grid.zoom, grid.view_tick), (2, 24));

        // Walking to the last cell in view slides nothing.
        grid.cursor_step = 16 + 9; // cell 108..120 in a 24..120 window
        grid.follow_cursor();
        assert_eq!(grid.view_tick, 24);
        // One cell further slides by exactly one cell.
        grid.cursor_step = 16 + 10;
        grid.follow_cursor();
        assert_eq!(grid.view_tick, 36);
        // Walking back left of the window slides to the cell's start.
        grid.cursor_step = 16 + 2; // cell 24..36
        grid.follow_cursor();
        assert_eq!(grid.view_tick, 24);

        // A finer grid leaves the camera where it stood.
        grid.resolution = GridResolution::at(32, false);
        grid.cursor_step = 32 + 4; // still tick 24 of bar 2
        grid.follow_cursor();
        assert_eq!((grid.zoom, grid.view_tick), (2, 24));

        // Never past the bar: the last cell pins the window to the end.
        grid.cursor_step = 32 + 31;
        grid.follow_cursor();
        assert_eq!(grid.view_tick, 96);

        // Zooming out to one shows the whole bar from its start.
        grid.rezoom(1);
        assert_eq!((grid.visible_ticks(), grid.view_tick), (192, 0));

        // A cell wider than the window shows from its start.
        grid.resolution = GridResolution::at(4, false); // 48-tick cells
        grid.cursor_step = 4 + 1; // tick 48
        grid.zoom = 16; // a 12-tick window
        grid.follow_cursor();
        assert_eq!(grid.view_tick, 48);
    }

    #[test]
    fn the_ruler_labels_the_finest_unit_that_has_room() {
        // Sixteenth cells at 46px: beats and sixteenths have room, finer
        // units do not exist in the grid.
        assert_eq!(ruler_unit(12, 46.0, 28.0), Some(12));
        assert_eq!(ruler_label(0, Some(12)).as_deref(), Some("1"));
        assert_eq!(ruler_label(12, Some(12)).as_deref(), Some("1.2"));
        assert_eq!(ruler_label(48, Some(12)).as_deref(), Some("2"));
        assert_eq!(ruler_label(6, Some(12)), None, "an off-unit tick spoke");
        // Thirty-second cells at 92px (zoomed): the sub-sixteenth speaks.
        assert_eq!(ruler_unit(6, 92.0, 28.0), Some(6));
        assert_eq!(ruler_label(6, Some(6)).as_deref(), Some("1.1.2"));
        // Thirty-second cells at 12px: only every beat has room.
        assert_eq!(ruler_unit(6, 12.0, 28.0), Some(48));
        assert_eq!(ruler_label(12, Some(48)), None);
        // Nothing has room: the ruler keeps its marks and says nothing.
        assert_eq!(ruler_unit(12, 4.0, 28.0), None);
        assert_eq!(ruler_label(0, None), None);
        assert_eq!(zoom_sign(1), "");
        assert_eq!(zoom_sign(4), " ×4");
    }

    /// A note placed on a finer grid belongs to the coarse cell that
    /// spans it: the cell shows it, and ACT gates it instead of adding a
    /// second note beside it.
    #[test]
    fn a_coarse_cell_owns_the_finer_notes_within_it() {
        let notes = [NoteView::from_midi(60, 6, 6, 100, 1.0, true)];
        let clip = one_note_clip(&notes);
        assert_eq!(primary_at(clip, 0, 12).map(|n| n.start_ticks), Some(6));
        assert_eq!(
            primary_at(clip, 0, 6),
            None,
            "a fine cell claimed a later note"
        );
        assert_eq!(starts_in(Some(clip), 0, 12), vec![6]);
        let mut grid = SequenceGrid::default();
        let mut intents = Vec::new();
        grid.toggle(Some(clip), &mut intents);
        assert!(
            matches!(intents.as_slice(), [Intent::Toggle { tick: 6, .. }]),
            "ACT on the cell did not address the note's own tick: {intents:?}"
        );
        // Delete on the coarse cell clears exactly what is there.
        let intents = utter_on(
            &mut grid,
            &mut Registers::default(),
            Some(clip),
            Some(Verb::Delete),
            None,
            1,
        );
        assert_eq!(intents, vec![Intent::Clear { tick: 6 }]);
    }

    #[test]
    fn the_condition_sign_thins_with_the_chance() {
        assert_eq!(condition_sign(0.75), '▓');
        assert_eq!(condition_sign(0.5), '▒');
        assert_eq!(condition_sign(0.25), '░');
        assert_eq!(condition_sign(0.1), '░');
    }

    #[test]
    fn duration_continues_across_the_visual_row_wrap() {
        let notes = [NoteView::from_midi(60, 15 * 12, 24, 100, 1.0, true)];
        let cell = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 40.0));
        let tail = note_span_rect(cell, 16 * 12, 12, &notes[0]).expect("tail in the next row");
        assert_eq!((tail.left(), tail.right()), (0.0, 120.0));
    }

    #[test]
    fn a_note_faces_onset_and_end_are_its_time_positions() {
        let note = NoteView::from_midi(60, 3, 6, 100, 1.0, true);
        let cell = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(120.0, 40.0));
        let face = note_span_rect(cell, 0, 12, &note).expect("note face");
        assert_eq!(face.left(), 40.0, "onset was not one quarter in");
        assert_eq!(face.right(), 100.0, "end was not three quarters in");
        assert!(face.top() > cell.top() && face.bottom() < cell.bottom());
    }

    fn utter(
        grid: &mut SequenceGrid,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        utter_on(grid, &mut registers, None, verb, motion, count)
    }

    fn utter_on(
        grid: &mut SequenceGrid,
        registers: &mut Registers,
        clip: Option<ClipView<'_>>,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        grid.refusal = None;
        grid.speak(
            Utterance {
                count,
                verb,
                motion,
                held: false,
            },
            registers,
            clip,
            &mut intents,
        );
        intents
    }

    fn one_note_clip(notes: &[NoteView]) -> ClipView<'_> {
        ClipView {
            id: 1,
            name: "test",
            length_ticks: 64 * 12,
            notes,
            ghosts: &[],
            slicing: false,
        }
    }

    fn utter_held(
        grid: &mut SequenceGrid,
        clip: Option<ClipView<'_>>,
        motion: Motion,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        let mut intents = Vec::new();
        grid.refusal = None;
        grid.speak(
            Utterance {
                count,
                verb: None,
                motion: Some(motion),
                held: true,
            },
            &mut registers,
            clip,
            &mut intents,
        );
        intents
    }

    /// Hold-as-preposition: held arrows edit the trig and never travel;
    /// a hold over nothing, or in a direction the trig cannot answer,
    /// refuses out loud.
    #[test]
    fn held_arrows_edit_velocity_and_never_travel() {
        let mut grid = SequenceGrid::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_held(&mut grid, Some(clip), Motion::Up, 8);
        assert_eq!(intents, vec![Intent::AdjustVelocity { tick: 0, delta: 8 }]);
        assert_eq!(grid.cursor_step, 0, "a held motion never travels");

        let intents = utter_held(&mut grid, Some(clip), Motion::Down, 1);
        assert_eq!(intents, vec![Intent::AdjustVelocity { tick: 0, delta: -1 }]);

        let intents = utter_held(&mut grid, Some(clip), Motion::Left, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("HOLD: UP OR DOWN"));

        let intents = utter_held(&mut grid, None, Motion::Up, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("HOLD: NOTHING HERE"));
    }

    #[test]
    fn yank_then_put_lands_the_whole_trig_elsewhere() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 0.75, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Yank),
            None,
            1,
        );
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("YANKED A TRIG"));

        grid.cursor_step = 4;
        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Put),
            None,
            1,
        );
        assert_eq!(
            intents,
            vec![
                Intent::Clear { tick: 4 * step },
                Intent::AddNote {
                    tick: 4 * step,
                    pitch: Pitch::from_midi(60),
                    length_ticks: step,
                    velocity: 100,
                    probability: 0.75,
                },
            ]
        );
    }

    #[test]
    fn yanking_an_empty_step_is_refused_and_put_says_so() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let intents = utter_on(&mut grid, &mut registers, None, Some(Verb::Yank), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("YANK: NOTHING HERE"));

        let intents = utter_on(&mut grid, &mut registers, None, Some(Verb::Put), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("PUT: NOTHING YANKED"));
    }

    #[test]
    fn bare_condition_walks_the_ladder_and_counted_condition_names_a_percentage() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Condition),
            None,
            1,
        );
        assert_eq!(
            intents,
            vec![Intent::SetProbability {
                tick: 0,
                probability: 0.75,
            }]
        );

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Condition),
            None,
            50,
        );
        assert_eq!(
            intents,
            vec![Intent::SetProbability {
                tick: 0,
                probability: 0.5,
            }]
        );

        let intents = utter_on(
            &mut grid,
            &mut registers,
            None,
            Some(Verb::Condition),
            None,
            1,
        );
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("CONDITION: NOTHING HERE"));
    }

    #[test]
    fn the_condition_ladder_is_closed_and_returns_home() {
        let mut probability = 1.0;
        for _ in 0..5 {
            probability = next_probability(probability);
        }
        assert_eq!(probability, 1.0, "five steps walk the whole ladder home");
        assert_eq!(next_probability(0.62), 1.0, "off-ladder values re-enter");
    }

    #[test]
    fn duplicate_copies_ahead_and_carries_the_cursor() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);
        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Duplicate),
            None,
            2,
        );
        assert_eq!(
            intents,
            vec![
                Intent::Clear { tick: 2 * step },
                Intent::AddNote {
                    tick: 2 * step,
                    pitch: Pitch::from_midi(60),
                    length_ticks: step,
                    velocity: 100,
                    probability: 1.0,
                },
            ]
        );
        assert_eq!(grid.cursor_step, 2);
        assert!(registers.is_empty(), "duplicate is a shorthand, not a yank");
    }

    /// Velocity reads as value on the face: bone-tinted throughout, floored
    /// so the softest trig stays a visible fact, accents reaching white.
    #[test]
    fn the_note_face_carries_velocity_as_value() {
        let soft = velocity_ink(1, Polarity::Dark);
        let mid = velocity_ink(100, Polarity::Dark);
        let hard = velocity_ink(127, Polarity::Dark);
        for ink in [soft, mid, hard] {
            assert!(crate::design::is_tint(ink));
        }
        assert!(soft.r() >= 96, "a face is a fact before it is a level");
        assert!(soft.r() < mid.r());
        assert!(mid.r() < hard.r());
        assert_eq!(hard.r(), 255, "the accent reaches white");
    }

    #[test]
    fn held_tail_slabs_are_dimmer_but_keep_velocity_order() {
        let soft = tail_ink(1, Polarity::Dark);
        let hard = tail_ink(127, Polarity::Dark);
        assert!(soft.r() < hard.r());
        assert!(hard.r() < velocity_ink(1, Polarity::Dark).r());
    }

    #[test]
    fn a_counted_motion_travels_that_far() {
        let mut grid = SequenceGrid::default();
        utter(&mut grid, None, Some(Motion::Right), 4);
        assert_eq!(grid.cursor_step, 4);
    }

    #[test]
    fn act_speaks_the_trig_toggle() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Act), None, 1);
        assert!(matches!(
            intents.as_slice(),
            [Intent::Toggle { tick: 0, .. }]
        ));
    }

    #[test]
    fn nudge_carries_the_cursor_with_the_trig() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 8;
        let step = grid.resolution.step_ticks();
        let intents = utter(&mut grid, Some(Verb::Nudge), Some(Motion::Right), 2);
        assert_eq!(
            intents,
            vec![Intent::Nudge {
                tick: 8 * step,
                delta_ticks: 2 * step as isize,
            }]
        );
        assert_eq!(grid.cursor_step, 10);
    }

    #[test]
    fn an_unsupported_verb_is_refused_out_loud() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Rename), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("RENAME: NOT HERE"));
    }

    #[test]
    fn resize_refuses_a_vertical_motion() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Resize), Some(Motion::Up), 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("RESIZE: LEFT OR RIGHT"));
    }

    #[test]
    fn clip_resize_uses_the_active_grid_unit() {
        let mut grid = SequenceGrid::default();
        let step = grid.resolution.step_ticks();
        assert_eq!(
            utter(&mut grid, Some(Verb::ClipResize), Some(Motion::Left), 3,),
            vec![Intent::ResizeClip {
                delta_ticks: -(3 * step as isize),
            }]
        );
    }

    #[test]
    fn sixty_four_sixteenths_are_four_bars() {
        let grid = SequenceGrid::default();
        assert_eq!(
            PATTERN_STEPS * grid.resolution.step_ticks(),
            TICKS_PER_BAR * 4
        );
    }
}
